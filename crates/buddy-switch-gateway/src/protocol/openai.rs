//! OpenAI 端点协议：SSE 透传包装与上游 SSE → `chat.completion` 聚合。

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Bytes;
use futures_util::Stream;
use serde_json::{json, Value};

/// 判断字节块是否包含子串。
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// 从 SSE 文本中提取所有 `data:` 负载（按行）。
pub fn parse_sse_payloads(text: &str) -> Vec<String> {
    let mut payloads = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            let rest = rest.trim();
            if !rest.is_empty() {
                payloads.push(rest.to_string());
            }
        }
    }
    payloads
}

/// 构造一个 SSE 事件（`event:` + `data:` + 空行）。
pub fn sse_event(event: &str, data: &Value) -> Bytes {
    let payload = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event}\ndata: {payload}\n\n"))
}

/// SSE 透传包装：逐块转发上游字节；若流中途异常或结束前未见到 `[DONE]`，
/// 补发一条 `data: [DONE]`，绝不悬挂连接（P0-5 / A-4.1）。
///
/// 内部用 `Pin<Box<S>>` 存放上游流，因此**不要求上游流实现 `Unpin`**。
pub struct SsePassthrough<S> {
    inner: Pin<Box<S>>,
    saw_done: bool,
    finished: bool,
    done_probe: Vec<u8>,
}

impl<S> SsePassthrough<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner: Box::pin(inner),
            saw_done: false,
            finished: false,
            done_probe: Vec::new(),
        }
    }
}

impl<S, E> Stream for SsePassthrough<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Debug,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if !this.saw_done {
                    this.done_probe.extend_from_slice(&chunk);
                    if contains(&this.done_probe, b"[DONE]") {
                        this.saw_done = true;
                    }
                    // Keep only enough suffix to match a marker split across chunks;
                    // never retain the full response stream.
                    const DONE_MARKER_LEN: usize = 6;
                    if this.done_probe.len() > DONE_MARKER_LEN {
                        let keep_from = this.done_probe.len() - (DONE_MARKER_LEN - 1);
                        this.done_probe.drain(..keep_from);
                    }
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.finished = true;
                eprintln!("[gateway] 上游 SSE 流中途异常: {error:?}");
                if this.saw_done {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Ok(Bytes::from_static(b"data: [DONE]\n\n"))))
                }
            }
            Poll::Ready(None) => {
                this.finished = true;
                if this.saw_done {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Ok(Bytes::from_static(b"data: [DONE]\n\n"))))
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// 单个 tool_call 的增量聚合。
#[derive(Debug, Default, Clone)]
pub struct ToolCallAccum {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 上游 SSE delta 聚合器：把流式分片聚合成完整 completion。
#[derive(Debug, Default)]
pub struct CompletionAccumulator {
    pub id: Option<String>,
    pub model: Option<String>,
    pub created: Option<i64>,
    pub role: Option<String>,
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCallAccum>,
    pub finish_reason: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// 上游上报的积分消耗（`usage.credit`）。
    ///
    /// 单独保存是必要的：账号池的「实测成本账本」以 `credit / tokens` 折算单价，
    /// 若拿不到 `credit` 就写入样本，会把账号误标为「免费」，比不写更糟。
    pub credit: Option<f64>,
    /// 上游返回的原始 usage 对象，供下游协议提取 prompt_tokens_details / cached_tokens 等详情。
    pub raw_usage: Option<Value>,
}

impl CompletionAccumulator {
    /// 吞入一个上游 chunk。
    pub fn ingest_chunk(&mut self, chunk: &Value) {
        if self.id.is_none() {
            if let Some(id) = chunk.get("id").and_then(Value::as_str) {
                self.id = Some(id.to_string());
            }
        }
        if self.model.is_none() {
            if let Some(model) = chunk.get("model").and_then(Value::as_str) {
                self.model = Some(model.to_string());
            }
        }
        if self.created.is_none() {
            if let Some(created) = chunk.get("created").and_then(Value::as_i64) {
                self.created = Some(created);
            }
        }
        if let Some(usage) = chunk.get("usage") {
            if !usage.is_null() {
                self.raw_usage = Some(usage.clone());
            }
            if let Some(prompt) = usage.get("prompt_tokens").and_then(Value::as_u64) {
                self.prompt_tokens = Some(prompt);
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(Value::as_u64) {
                self.completion_tokens = Some(completion);
            }
            if let Some(credit) = usage.get("credit").and_then(Value::as_f64) {
                self.credit = Some(credit);
            }
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return;
        };

        if let Some(delta) = choice.get("delta") {
            if self.role.is_none() {
                if let Some(role) = delta.get("role").and_then(Value::as_str) {
                    self.role = Some(role.to_string());
                }
            }
            if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
                let delta_text =
                    crate::protocol::reasoning::reasoning_delta(&self.reasoning, reasoning);
                self.reasoning.push_str(delta_text);
            }
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                self.content.push_str(content);
            }
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tool_call in tool_calls {
                    let index = tool_call
                        .get("index")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .max(0) as usize;
                    while self.tool_calls.len() <= index {
                        self.tool_calls.push(ToolCallAccum::default());
                    }
                    let slot = &mut self.tool_calls[index];
                    if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                        if !id.is_empty() {
                            slot.id = id.to_string();
                        }
                    }
                    if let Some(function) = tool_call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            if !name.is_empty() {
                                slot.name.push_str(name);
                            }
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            slot.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(finish_reason.to_string());
        }
    }

    /// 由完整 SSE 文本构造聚合器。
    pub fn from_sse(text: &str) -> Self {
        let mut accumulator = Self::default();
        for payload in parse_sse_payloads(text) {
            if payload == "[DONE]" {
                continue;
            }
            if let Ok(chunk) = serde_json::from_str::<Value>(&payload) {
                accumulator.ingest_chunk(&chunk);
            }
        }
        accumulator
    }

    /// 由**完整响应体**构造聚合器（非流式上游：`choices[0].message`）。
    ///
    /// 与 [`Self::from_sse`] 的分工：`from_sse` 吃**流式 chunk 序列**（`delta`，
    /// 需按 `index` 归并分片），本函数吃**一次性响应**（`message`，各字段已是完整值）。
    /// 两者产出**同一结构**，因此下游（Anthropic 转换、用量统计）只认聚合器，
    /// 不关心上游是流式还是非流式 —— 这正是 `/v1/messages` 能复用同一套转换的前提。
    pub fn from_response(response: &Value) -> Self {
        let mut accumulator = Self::default();
        // 顶层 `id` / `model` / `usage` 与 chunk 同形，直接复用现有解析。
        accumulator.ingest_chunk(response);

        let Some(choice) = response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return accumulator;
        };
        let Some(message) = choice.get("message") else {
            return accumulator;
        };

        if accumulator.role.is_none() {
            if let Some(role) = message.get("role").and_then(Value::as_str) {
                accumulator.role = Some(role.to_string());
            }
        }
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            accumulator.content.push_str(content);
        }
        if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) {
            accumulator.reasoning.push_str(reasoning);
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let function = tool_call.get("function");
                let text_of = |key: &str| {
                    function
                        .and_then(|f| f.get(key))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                // 完整响应没有 `index`（不是增量分片），按数组顺序落位。
                accumulator.tool_calls.push(ToolCallAccum {
                    id: tool_call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: text_of("name"),
                    arguments: text_of("arguments"),
                });
            }
        }
        accumulator
    }

    /// 聚合为 OpenAI `chat.completion` 对象。
    pub fn to_openai_completion(&self, fallback_model: &str) -> Value {
        let id = self
            .id
            .clone()
            .unwrap_or_else(|| format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()));
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| fallback_model.to_string());
        let created = self
            .created
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        let mut message = serde_json::Map::new();
        message.insert(
            "role".to_string(),
            json!(self.role.clone().unwrap_or_else(|| "assistant".to_string())),
        );
        if !self.reasoning.is_empty() {
            message.insert("reasoning_content".to_string(), json!(self.reasoning));
        }
        if self.tool_calls.is_empty() {
            message.insert("content".to_string(), json!(self.content));
        } else {
            if self.content.is_empty() {
                message.insert("content".to_string(), Value::Null);
            } else {
                message.insert("content".to_string(), json!(self.content));
            }
            let tool_calls: Vec<Value> = self
                .tool_calls
                .iter()
                .map(|tool_call| {
                    json!({
                        "id": tool_call.id,
                        "type": "function",
                        "function": { "name": tool_call.name, "arguments": tool_call.arguments },
                    })
                })
                .collect();
            message.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }

        let prompt = self.prompt_tokens.unwrap_or(0);
        let completion = self.completion_tokens.unwrap_or(0);
        let mut usage = json!({
            "prompt_tokens": prompt,
            "completion_tokens": completion,
            "total_tokens": prompt + completion,
        });
        if let Some(raw) = &self.raw_usage {
            if let Some(ptd) = raw.get("prompt_tokens_details") {
                usage["prompt_tokens_details"] = ptd.clone();
            }
            if let Some(ctd) = raw.get("completion_tokens_details") {
                usage["completion_tokens_details"] = ctd.clone();
            }
        }
        json!({
            "id": id,
            "object": "chat.completion",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "message": Value::Object(message),
                "finish_reason": self.finish_reason.clone().unwrap_or_else(|| "stop".to_string()),
            }],
            "usage": usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[test]
    fn parse_sse_payloads_ignores_event_lines_and_done() {
        let text = "event: foo\ndata: {\"a\":1}\n\ndata: [DONE]\n\n";
        assert_eq!(parse_sse_payloads(text), vec!["{\"a\":1}", "[DONE]"]);
    }

    #[test]
    fn accumulator_builds_text_completion() {
        let sse = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"created\":1,\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let completion = CompletionAccumulator::from_sse(sse).to_openai_completion("fallback");
        assert_eq!(completion["object"], "chat.completion");
        assert_eq!(completion["choices"][0]["message"]["content"], "Hello");
        assert_eq!(completion["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn accumulator_merges_tool_call_arguments() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"SF\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        );
        let completion = CompletionAccumulator::from_sse(sse).to_openai_completion("m");
        assert_eq!(completion["choices"][0]["message"]["content"], Value::Null);
        let tool_call = &completion["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tool_call["id"], "call_1");
        assert_eq!(tool_call["function"]["name"], "get_weather");
        assert_eq!(tool_call["function"]["arguments"], "{\"city\":\"SF\"}");
        assert_eq!(completion["choices"][0]["finish_reason"], "tool_calls");
    }

    #[tokio::test]
    async fn passthrough_appends_done_on_midstream_error() {
        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: {\"a\":1}\n\n")),
            Err(std::io::Error::other("boom")),
        ];
        let stream = SsePassthrough::new(futures_util::stream::iter(chunks));
        let collected: Vec<Bytes> = stream.map(|item| item.unwrap()).collect().await;
        let text: String = collected
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect();
        assert!(text.contains("data: {\"a\":1}"));
        assert!(
            text.ends_with("data: [DONE]\n\n"),
            "流中断必须补发 [DONE]: {text}"
        );
    }

    #[tokio::test]
    async fn passthrough_does_not_duplicate_done() {
        let chunks = vec![Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"data: [DONE]\n\n",
        ))];
        let stream = SsePassthrough::new(futures_util::stream::iter(chunks));
        let collected: Vec<Bytes> = stream.map(|item| item.unwrap()).collect().await;
        let text: String = collected
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect();
        assert_eq!(text.matches("[DONE]").count(), 1);
    }

    #[tokio::test]
    async fn passthrough_does_not_duplicate_done_when_marker_spans_chunks() {
        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: [DO")),
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"NE]\n\n")),
        ];
        let stream = SsePassthrough::new(futures_util::stream::iter(chunks));
        let collected: Vec<Bytes> = stream.map(|item| item.unwrap()).collect().await;
        let text: String = collected
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect();
        assert_eq!(text.matches("[DONE]").count(), 1);
    }

    #[tokio::test]
    async fn passthrough_appends_done_when_upstream_ends_without_it() {
        let chunks = vec![Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"data: {\"x\":1}\n\n",
        ))];
        let stream = SsePassthrough::new(futures_util::stream::iter(chunks));
        let collected: Vec<Bytes> = stream.map(|item| item.unwrap()).collect().await;
        let text: String = collected
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect();
        assert!(text.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn from_sse_dedupes_snapshot_reasoning() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"AB\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"ABC\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let accum = CompletionAccumulator::from_sse(sse);
        assert_eq!(
            accum.reasoning, "ABC",
            "快照序列聚合后只保留全文，不重复拼接"
        );

        let completion = accum.to_openai_completion("m");
        assert_eq!(
            completion["choices"][0]["message"]["reasoning_content"],
            "ABC"
        );
    }

    #[test]
    fn from_sse_concatenates_incremental_reasoning() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"A\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"B\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"C\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let accum = CompletionAccumulator::from_sse(sse);
        assert_eq!(accum.reasoning, "ABC", "真增量序列完整拼接");
    }

    #[test]
    fn to_openai_completion_omits_reasoning_when_empty() {
        let accum = CompletionAccumulator {
            content: "hello".to_string(),
            ..Default::default()
        };
        let completion = accum.to_openai_completion("m");
        assert!(completion["choices"][0]["message"]
            .get("reasoning_content")
            .is_none());
    }
}
