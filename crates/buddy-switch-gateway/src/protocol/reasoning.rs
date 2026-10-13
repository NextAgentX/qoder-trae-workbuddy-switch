//! WorkBuddy 流式 reasoning 快照差分改写器。
//!
//! # 背景与目标
//!
//! 上游 WorkBuddy（OpenAI 兼容 `/v2/chat/completions`）在流式响应中，
//! `choices[].delta.reasoning_content` 实测为「累计快照」语义（每个分片都是截至
//! 当前时刻的完整思考全文）。当客户端（如 ZCode，强制 `stream: true`）收到快照分片时，
//! 会把每个分片渲染为独立的 Thought 块，导致思考块严重刷屏。
//!
//! 本模块提供 [`ReasoningDelta`] 流式包装，在网关层将快照差分为增量：
//! - 仅下发新增思考后缀；
//! - 重复或更短的冗余快照整帧丢弃（首帧仍正常下发）；
//! - 对真增量流幂等原样放行；
//! - 快路径：不含 `reasoning_content` 的帧零反序列化原样透传。

use std::collections::{HashMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Bytes;
use futures_util::Stream;
use serde_json::{json, Value};

/// 单行缓冲区上限：畸形流（永不出现 `\n`）不得让内存无界增长。超限时原样冲刷保底，不丢字节。
const MAX_LINE_BUFFER_BYTES: usize = 256 * 1024;

/// 计算 reasoning 的增量切片。
///
/// - `fragment.starts_with(sent)` 且更长：快照模式，只下发新增后缀；
/// - `sent.starts_with(fragment)`：重复或更短旧快照，返回 `""`；
/// - 其它情形：真增量模式，原样放行。
///
/// 当 `fragment.starts_with(sent)` 成立时，`sent.len()` 必落在 UTF-8 字符边界上。
pub fn reasoning_delta<'a>(sent: &str, fragment: &'a str) -> &'a str {
    if fragment.len() >= sent.len() && fragment.starts_with(sent) {
        &fragment[sent.len()..]
    } else if sent.starts_with(fragment) {
        ""
    } else {
        fragment
    }
}

/// 检查经过差分改写后的帧是否应该被整帧丢弃。
///
/// 丢弃条件：所有 choice 的 delta 均为空对象（或 null / 缺省），且无非空 `finish_reason`，
/// 且无非 null 的 `usage`。
///
/// 注意：顶层常规元数据（`id`、`model`、`created`、`object`）在流式分片中普遍存在，
/// 不阻碍丢帧；首帧由于初始 `sent` 为空必会下发，因此客户端不会缺失 `id` 与 `model`。
/// 检查经过差分改写后的帧是否应该被整帧丢弃。
///
/// 丢弃条件：所有 choice 均无任何有意义的 delta 增量（无非空 role、content、
/// reasoning_content、tool_calls、function_call），且无非空 `finish_reason`，
/// 且无非 null 的 `usage`。
fn should_drop_frame(value: &Value) -> bool {
    let choices_all_empty = if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        if choices.is_empty() {
            true
        } else {
            choices.iter().all(|choice| {
                let finish_reason_empty = match choice.get("finish_reason") {
                    Some(Value::String(s)) => s.is_empty(),
                    Some(Value::Null) | None => true,
                    _ => false,
                };
                let delta_empty = match choice.get("delta") {
                    Some(Value::Object(map)) => {
                        let has_role = map
                            .get("role")
                            .and_then(Value::as_str)
                            .map_or(false, |s| !s.is_empty());
                        let has_content = map
                            .get("content")
                            .and_then(Value::as_str)
                            .map_or(false, |s| !s.is_empty());
                        let has_reasoning = map
                            .get("reasoning_content")
                            .or_else(|| map.get("reasoning"))
                            .and_then(Value::as_str)
                            .map_or(false, |s| !s.is_empty());
                        let has_tool_calls = map
                            .get("tool_calls")
                            .and_then(Value::as_array)
                            .map_or(false, |arr| !arr.is_empty());
                        let has_function_call = match map.get("function_call") {
                            Some(Value::Object(fc)) => {
                                let name = fc.get("name").and_then(Value::as_str).unwrap_or("");
                                let args = fc.get("arguments").and_then(Value::as_str).unwrap_or("");
                                !name.is_empty() || !args.is_empty()
                            }
                            _ => false,
                        };
                        !has_role && !has_content && !has_reasoning && !has_tool_calls && !has_function_call
                    }
                    Some(Value::Null) | None => true,
                    _ => false,
                };
                delta_empty && finish_reason_empty
            })
        }
    } else {
        true
    };

    if !choices_all_empty {
        return false;
    }

    if let Some(usage) = value.get("usage") {
        if !usage.is_null() {
            return false;
        }
    }

    true
}

/// SSE 流改写包装：将快照式的 `reasoning_content` 改写为增量分片，
/// 并规范化 delta 字段（剥离空 `tool_calls: []` 等畸形占位），防止下游客户端异常。
pub struct ReasoningDelta<S> {
    inner: Pin<Box<S>>,
    buffer: Vec<u8>,
    sent: HashMap<i64, String>,
    sent_role: bool,
    pending: VecDeque<Bytes>,
    frame_lines: Vec<Bytes>,
    frame_has_data: bool,
    frame_data_kept: usize,
    terminal: Option<io::Error>,
    finished: bool,
}

impl<S> ReasoningDelta<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner: Box::pin(inner),
            buffer: Vec::new(),
            sent: HashMap::new(),
            sent_role: false,
            pending: VecDeque::new(),
            frame_lines: Vec::new(),
            frame_has_data: false,
            frame_data_kept: 0,
            terminal: None,
            finished: false,
        }
    }

    /// 上游流结束或出错时，排空缓冲区残余字节。
    fn flush_tail(&mut self) {
        if !self.buffer.is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            self.frame_lines.push(Bytes::from(remaining));
        }
        if !self.frame_lines.is_empty() {
            for line in self.frame_lines.drain(..) {
                self.pending.push_back(line);
            }
        }
        self.frame_has_data = false;
        self.frame_data_kept = 0;
    }

    /// 吞入上游数据块，按换行切分并处理完整行。
    fn ingest(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line_raw: Vec<u8> = self.buffer.drain(..=pos).collect();
            self.handle_line(&line_raw);
        }
        if self.buffer.len() > MAX_LINE_BUFFER_BYTES {
            let overflow = std::mem::take(&mut self.buffer);
            self.pending.push_back(Bytes::from(overflow));
        }
    }

    /// 处理单行 SSE 数据。
    fn handle_line(&mut self, line: &[u8]) {
        let is_empty_line = line == b"\n" || line == b"\r\n";
        if is_empty_line {
            // 当前 SSE 事件结束
            let should_emit = if self.frame_has_data {
                self.frame_data_kept > 0
            } else {
                !self.frame_lines.is_empty()
            };

            if should_emit {
                for l in self.frame_lines.drain(..) {
                    self.pending.push_back(l);
                }
                self.pending.push_back(Bytes::copy_from_slice(line));
            } else {
                self.frame_lines.clear();
            }
            self.frame_has_data = false;
            self.frame_data_kept = 0;
            return;
        }

        let line_str = match std::str::from_utf8(line) {
            Ok(s) => s,
            Err(_) => {
                // 非合法 UTF-8，原样保留
                self.frame_lines.push(Bytes::copy_from_slice(line));
                return;
            }
        };

        let trimmed_end = line_str.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed_end.strip_prefix("data:") {
            self.frame_has_data = true;
            let payload = rest.trim();

            if payload == "[DONE]" || payload.is_empty() {
                self.frame_data_kept += 1;
                self.frame_lines.push(Bytes::copy_from_slice(line));
                return;
            }

            let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
                self.frame_data_kept += 1;
                self.frame_lines.push(Bytes::copy_from_slice(line));
                return;
            };

            let mut has_role_in_frame = false;

            if let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) {
                for (i, choice) in choices.iter_mut().enumerate() {
                    let index = choice
                        .get("index")
                        .and_then(Value::as_i64)
                        .unwrap_or(i as i64);
                    if let Some(delta) = choice.get_mut("delta").and_then(Value::as_object_mut) {
                        // 1. 消除空 tool_calls / null tool_calls，杜绝客户端（如 ZCode/@ai-sdk）
                        // 因 `delta.tool_calls != null` 误判「工具调用开始」而强行提前结束思考块
                        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                            if tool_calls.is_empty() {
                                delta.remove("tool_calls");
                            }
                        }
                        if delta.get("tool_calls") == Some(&Value::Null) {
                            delta.remove("tool_calls");
                        }

                        // 2. 清理无意义的 null/空占位字段，避免客户端解析混乱
                        if delta.get("extra_fields") == Some(&Value::Null) {
                            delta.remove("extra_fields");
                        }
                        if delta.get("function_call") == Some(&Value::Null) {
                            delta.remove("function_call");
                        }
                        if delta.get("refusal").and_then(Value::as_str) == Some("") {
                            delta.remove("refusal");
                        }

                        // 3. 检查并记录 role
                        if let Some(role) = delta.get("role").and_then(Value::as_str) {
                            if !role.is_empty() {
                                has_role_in_frame = true;
                                self.sent_role = true;
                            }
                        }

                        // 4. 快照差分改写 reasoning_content
                        if let Some(frag) = delta.get("reasoning_content").and_then(Value::as_str) {
                            let sent_entry = self.sent.entry(index).or_default();
                            let delta_text = reasoning_delta(sent_entry, frag);
                            if !delta_text.is_empty() {
                                sent_entry.push_str(delta_text);
                                delta.insert("reasoning_content".to_string(), json!(delta_text));
                                // 思考阶段若附带空 content:"" 占位符，予以剥离以防客户端误触内容流
                                if delta.get("content").and_then(Value::as_str) == Some("") {
                                    delta.remove("content");
                                }
                            } else {
                                delta.remove("reasoning_content");
                            }
                        }
                    }
                }
            }

            if should_drop_frame(&value) {
                // 冗余帧/全空占位帧丢弃：不增加 frame_data_kept，不推入 frame_lines
                return;
            }

            // 若首帧尚未下发 role，且本帧即将下发有意义的思考或内容，为首个 choice 补齐 role:"assistant"
            if !self.sent_role && !has_role_in_frame {
                if let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) {
                    if let Some(choice) = choices.first_mut() {
                        if let Some(delta) = choice.get_mut("delta").and_then(Value::as_object_mut) {
                            delta.insert("role".to_string(), json!("assistant"));
                            self.sent_role = true;
                        }
                    }
                }
            }

            let line_ending = if line.ends_with(b"\r\n") { "\r\n" } else { "\n" };
            let rewritten = match serde_json::to_string(&value) {
                Ok(json_str) => format!("data: {json_str}{line_ending}"),
                Err(_) => {
                    self.frame_data_kept += 1;
                    self.frame_lines.push(Bytes::copy_from_slice(line));
                    return;
                }
            };
            self.frame_data_kept += 1;
            self.frame_lines.push(Bytes::from(rewritten));
        } else {
            // 非 data: 行（event:, id:, retry:, 注释等）原样透传
            self.frame_lines.push(Bytes::copy_from_slice(line));
        }
    }
}

impl<S, E> Stream for ReasoningDelta<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Debug,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(chunk) = this.pending.pop_front() {
                return Poll::Ready(Some(Ok(chunk)));
            }
            if let Some(err) = this.terminal.take() {
                this.finished = true;
                return Poll::Ready(Some(Err(err)));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    this.flush_tail();
                    this.finished = true;
                    if let Some(chunk) = this.pending.pop_front() {
                        return Poll::Ready(Some(Ok(chunk)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Err(e))) => {
                    this.flush_tail();
                    this.terminal = Some(io::Error::other(format!("{e:?}")));
                    if let Some(chunk) = this.pending.pop_front() {
                        return Poll::Ready(Some(Ok(chunk)));
                    }
                    let err = this.terminal.take().unwrap();
                    this.finished = true;
                    return Poll::Ready(Some(Err(err)));
                }
                Poll::Ready(Some(Ok(chunk))) => {
                    this.ingest(&chunk);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::openai::SsePassthrough;
    use crate::protocol::usage_tap::{UsageSink, UsageTap};
    use futures_util::StreamExt;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_reasoning_delta_snapshot_and_incremental() {
        let mut sent = String::new();
        let d1 = reasoning_delta(&sent, "我想");
        assert_eq!(d1, "我想");
        sent.push_str(d1);

        let d2 = reasoning_delta(&sent, "我想想看");
        assert_eq!(d2, "想看", "快照流只发新增后缀");
        sent.push_str(d2);
        assert_eq!(sent, "我想想看");

        // 真增量片段不以 sent 为前缀 -> 原样放行
        assert_eq!(reasoning_delta(&sent, "，你好"), "，你好");

        // 多字节字符边界安全
        assert_eq!(reasoning_delta("你好", "你好世界"), "世界");

        // 重复与更短快照
        assert_eq!(reasoning_delta("我想", "我想"), "");
        assert_eq!(reasoning_delta("我想想看", "我想"), "", "更短的旧快照返回空");
    }

    async fn collect_stream_lines<S>(stream: S) -> Vec<String>
    where
        S: Stream<Item = Result<Bytes, io::Error>> + Unpin,
    {
        let chunks: Vec<Result<Bytes, io::Error>> = stream.collect().await;
        let mut full_bytes = Vec::new();
        for chunk in chunks {
            full_bytes.extend_from_slice(&chunk.unwrap());
        }
        let full_text = String::from_utf8(full_bytes).unwrap();
        full_text
            .split("\n\n")
            .filter(|s| !s.is_empty())
            .map(|s| format!("{s}\n\n"))
            .collect()
    }

    fn chunk(s: &str) -> Result<Bytes, io::Error> {
        Ok(Bytes::from(s.to_string()))
    }

    #[tokio::test]
    async fn test_snapshot_stream_diffs_to_incremental() {
        let input = vec![
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"ABC\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 4);
        assert!(events[0].contains("\"reasoning_content\":\"AB\""));
        assert!(events[1].contains("\"reasoning_content\":\"C\""));
        assert!(events[2].contains("\"content\":\"hello\""));
        assert_eq!(events[3], "data: [DONE]\n\n");
    }

    #[tokio::test]
    async fn test_true_incremental_stream_passes_through() {
        let input = vec![
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"A\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"B\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 3);
        assert!(events[0].contains("\"reasoning_content\":\"A\""));
        assert!(events[1].contains("\"reasoning_content\":\"B\""));
        assert_eq!(events[2], "data: [DONE]\n\n");
    }

    #[tokio::test]
    async fn test_duplicate_snapshot_frame_completely_vanishes() {
        let input = vec![
            chunk("data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n"),
            // 重复快照帧：应整行消失，无裸空行
            chunk("data: {\"id\":\"2\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n"),
            // 更短快照帧：应整行消失
            chunk("data: {\"id\":\"3\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"A\"}}]}\n\n"),
            chunk("data: {\"id\":\"4\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"ABCD\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 3, "重复帧与更短帧整帧被丢弃: {events:?}");
        assert!(events[0].contains("\"reasoning_content\":\"AB\""));
        assert!(events[1].contains("\"reasoning_content\":\"CD\""));
        assert_eq!(events[2], "data: [DONE]\n\n");
    }

    #[tokio::test]
    async fn test_cross_chunk_and_utf8_split() {
        // "你好" 的 "好" 字节为 [0xE5, 0xA5, 0xBD]，拆在第一个 chunk 结尾与第二个 chunk 开头
        let raw1 = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"\xE4\xBD\xA0\xE5";
        let raw2 = b"\xA5\xBD\"}}]}\n\n";

        let input: Vec<Result<Bytes, io::Error>> = vec![
            Ok(Bytes::from_static(raw1)),
            Ok(Bytes::from_static(raw2)),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 1);
        assert!(events[0].contains("\"reasoning_content\":\"你好\""));
        assert!(!events[0].contains('\u{FFFD}'));
    }

    #[tokio::test]
    async fn test_done_passthrough_and_sse_passthrough_composition() {
        // 自带 [DONE] 的流与 SsePassthrough 组合，不产生重复 [DONE]
        let input = vec![
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = SsePassthrough::new(ReasoningDelta::new(futures_util::stream::iter(input)));
        let events = collect_stream_lines(stream).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[1], "data: [DONE]\n\n");

        // 未带 [DONE] 的流与 SsePassthrough 组合，末尾自动补恰好一个 [DONE]
        let input2 = vec![
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n"),
        ];
        let stream2 = SsePassthrough::new(ReasoningDelta::new(futures_util::stream::iter(input2)));
        let events2 = collect_stream_lines(stream2).await;
        assert_eq!(events2.len(), 2);
        assert_eq!(events2[1], "data: [DONE]\n\n");
    }

    #[tokio::test]
    async fn test_non_data_and_non_json_lines_passthrough() {
        let input = vec![
            chunk(": keep-alive comment\n\n"),
            chunk("event: ping\nid: 42\nretry: 1000\n\n"),
            chunk("data: plain text not json\n\n"),
            chunk("data:{\"choices\":[{\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n"),
            chunk("data:  {\"choices\":[{\"delta\":{\"reasoning_content\":\"ABC\"}}]}\n\n"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 5);
        assert_eq!(events[0], ": keep-alive comment\n\n");
        assert_eq!(events[1], "event: ping\nid: 42\nretry: 1000\n\n");
        assert_eq!(events[2], "data: plain text not json\n\n");
        assert!(events[3].contains("\"reasoning_content\":\"AB\""));
        assert!(events[4].contains("\"reasoning_content\":\"C\""));
    }

    #[tokio::test]
    async fn test_multi_choices_sent_isolation() {
        let input = vec![
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"X\"}},{\"index\":1,\"delta\":{\"reasoning_content\":\"1\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"XY\"}},{\"index\":1,\"delta\":{\"reasoning_content\":\"12\"}}]}\n\n"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 2);
        let val1: Value = serde_json::from_str(events[0].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(val1["choices"][0]["delta"]["reasoning_content"], "X");
        assert_eq!(val1["choices"][1]["delta"]["reasoning_content"], "1");

        let val2: Value = serde_json::from_str(events[1].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(val2["choices"][0]["delta"]["reasoning_content"], "Y");
        assert_eq!(val2["choices"][1]["delta"]["reasoning_content"], "2");
    }

    struct RecordingSink {
        tokens: AtomicI64,
    }
    impl UsageSink for RecordingSink {
        fn record(&self, _credit: f64, tokens: i64) {
            self.tokens.store(tokens, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn test_usage_tap_composition_records_usage() {
        let sink = Arc::new(RecordingSink {
            tokens: AtomicI64::new(0),
        });
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"ABC\"}}],\"usage\":{\"credit\":1.5,\"prompt_tokens\":10,\"completion_tokens\":20}}\n\n",
            "data: [DONE]\n\n",
        );
        let chunks: Vec<Result<Bytes, io::Error>> = vec![Ok(Bytes::from(raw))];
        let stream = SsePassthrough::new(ReasoningDelta::new(UsageTap::new(
            futures_util::stream::iter(chunks),
            Some(sink.clone()),
        )));
        let _ = collect_stream_lines(stream).await;

        assert_eq!(sink.tokens.load(Ordering::SeqCst), 30);
    }

    #[tokio::test]
    async fn test_trailing_line_and_error_handling() {
        let input = vec![
            chunk("data: incomplete without double newline"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let mut stream = Box::pin(stream);
        let item = stream.next().await.unwrap().unwrap();
        assert_eq!(item, Bytes::from("data: incomplete without double newline"));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_frame_with_finish_reason_or_usage_preserved() {
        let input = vec![
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n"),
            // 差分为空，但带 finish_reason: "stop" -> 帧保留，移除 reasoning_content
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"AB\"},\"finish_reason\":\"stop\"}]}\n\n"),
            // 差分为空，但带 usage -> 帧保留
            chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"AB\"}}],\"usage\":{\"total_tokens\":100}}\n\n"),
        ];
        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        assert_eq!(events.len(), 3);
        let val2: Value = serde_json::from_str(events[1].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(val2["choices"][0]["finish_reason"], "stop");
        assert!(val2["choices"][0]["delta"].get("reasoning_content").is_none());

        let val3: Value = serde_json::from_str(events[2].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(val3["usage"]["total_tokens"], 100);
        assert!(val3["choices"][0]["delta"].get("reasoning_content").is_none());
    }

    #[tokio::test]
    async fn test_workbuddy_real_world_chunks_cleaned_and_deduped() {
        // 模拟真实上游 WorkBuddy 下发的分片序列：
        // 1. 首帧：声明 role:"assistant"，带空 tool_calls:[] 及各种占位字段
        // 2. 思考帧：累积快照，带 content:"" 与 tool_calls:[]
        // 3. 重复思考快照帧：应整帧丢弃
        // 4. 空过渡帧：无思考无内容，应整帧丢弃
        // 5. 正文帧：带 content:"Hi"，带空 tool_calls:[]
        // 6. 结束帧：带 finish_reason:"stop" 与 usage
        let input = vec![
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"\",\"extra_fields\":null,\"function_call\":null,\"refusal\":\"\",\"role\":\"assistant\",\"tool_calls\":[]},\"finish_reason\":\"\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-0\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":null}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"\",\"extra_fields\":null,\"function_call\":null,\"reasoning_content\":\"The\",\"refusal\":\"\",\"tool_calls\":[]},\"finish_reason\":\"\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-1\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":null}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"\",\"extra_fields\":null,\"function_call\":null,\"reasoning_content\":\"The user\",\"refusal\":\"\",\"tool_calls\":[]},\"finish_reason\":\"\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-2\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":null}\n\n"),
            // 重复快照：应整帧丢弃
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"\",\"extra_fields\":null,\"function_call\":null,\"reasoning_content\":\"The user\",\"refusal\":\"\",\"tool_calls\":[]},\"finish_reason\":\"\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-dup\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":null}\n\n"),
            // 空过渡帧：应整帧丢弃
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"\",\"extra_fields\":null,\"function_call\":null,\"refusal\":\"\",\"tool_calls\":[]},\"finish_reason\":\"\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-trans\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":null}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"Hi\",\"extra_fields\":null,\"function_call\":null,\"refusal\":\"\",\"tool_calls\":[]},\"finish_reason\":\"\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-3\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":null}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"!\",\"extra_fields\":null,\"function_call\":null,\"refusal\":\"\",\"tool_calls\":[]},\"finish_reason\":\"stop\",\"index\":0,\"logprobs\":null}],\"created\":1791851944,\"id\":\"chunk-4\",\"model\":\"kimi-k3-1\",\"object\":\"chat.completion.chunk\",\"usage\":{\"total_tokens\":10}}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];

        let stream = ReasoningDelta::new(futures_util::stream::iter(input));
        let events = collect_stream_lines(stream).await;

        // 验证：
        // 1. 重复快照与空过渡帧被剔除：保留首帧、2个思考帧、2个正文帧、1个DONE帧 = 6帧
        assert_eq!(events.len(), 6, "帧数应精准为 6（重复与空过渡被丢弃）: {events:?}");

        // 2. 检查每一帧，绝不包含 `"tool_calls":[]`
        for (i, ev) in events.iter().enumerate() {
            assert!(!ev.contains("\"tool_calls\":[]"), "第 {i} 帧不应包含空 tool_calls: {ev}");
            assert!(!ev.contains("\"tool_calls\": []"), "第 {i} 帧不应包含空 tool_calls: {ev}");
        }

        // 3. 检查首帧包含 role:"assistant"
        let v0: Value = serde_json::from_str(events[0].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(v0["choices"][0]["delta"]["role"], "assistant");

        // 4. 检查思考帧差分为增量，且不含 content:""
        let v1: Value = serde_json::from_str(events[1].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(v1["choices"][0]["delta"]["reasoning_content"], "The");
        assert!(v1["choices"][0]["delta"].get("content").is_none());

        let v2: Value = serde_json::from_str(events[2].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(v2["choices"][0]["delta"]["reasoning_content"], " user");
        assert!(v2["choices"][0]["delta"].get("content").is_none());

        // 5. 检查正文帧正常
        let v3: Value = serde_json::from_str(events[3].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(v3["choices"][0]["delta"]["content"], "Hi");

        let v4: Value = serde_json::from_str(events[4].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(v4["choices"][0]["delta"]["content"], "!");
        assert_eq!(v4["choices"][0]["finish_reason"], "stop");

        assert_eq!(events[5], "data: [DONE]\n\n");
    }
}
