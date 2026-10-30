//! Qoder 信封流 → 下游帧：**SSE 转发**（逐帧解包）与**聚合**（非流式）。
//!
//! ## 上游帧形态（见 `core::modules::qoder::api::unwrap_envelope_line`）
//!
//! ```text
//! data:{"headers":{…},"body":"<内层 OpenAI chunk>","statusCodeValue":200}
//! data:{"body":"[DONE]"}
//! event:finish{…}   ← 计时元数据（忽略）
//! ```
//!
//! ## 两条硬约束（与 Trae / WorkBuddy 两侧同一条纪律）
//!
//! 1. **绝不悬挂连接**：无论上游正常结束、中途断流、还是报错，都必须补发
//!    `data: [DONE]` —— VS Code 系客户端会一直等到它为止。
//! 2. **`stream: false` 也走上游的流式**：上游只有 SSE 端点；非流式请求由
//!    [`aggregate`] 在本地折叠成单个 `chat.completion`。

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use buddy_switch_core::modules::qoder::api::{self, ChatFrame};

/// 一行是否含 `[DONE]` 哨兵（转发层不需要精确判断，聚合层走结构化解包）。
fn process_line(line: &str) -> (Vec<Bytes>, bool) {
    match api::unwrap_envelope_line(line) {
        ChatFrame::Skip => (Vec::new(), false),
        ChatFrame::Chunk(cleaned) => (vec![Bytes::from(format!("data: {cleaned}\n\n"))], false),
        ChatFrame::Done => (Vec::new(), true),
        ChatFrame::UpstreamError { status, detail } => {
            /* 错误帧带**可操作信息**（状态码 + 上游详情）再收尾 —— 只补 [DONE] 会让
            客户端把「上游 429」显示成「回答就这么短」。

            ★ 排队（`10605` / `isQueued`）必须单独措辞 + 429：上游把它塞在 403 信封里，
            原样报「上游错误（HTTP 403）」会让人以为账号/凭据出了问题（实测真机就是
            403 + `{"code":"10605","isQueued":true}`）。 */
            let queued = super::pool::queue_signal(&detail);
            if queued {
                super::risk::note_queued();
            }
            /* ★ issue #129：对外形状收敛到 `failover` 的三种标准类别。
            改造前 `type` 是自造的 `upstream_error` / `upstream_queued`，
            `message` 直接拼上游 detail（含 `quota` 之类短语）——
            客户端既归不了 `type`，又可能被 detail 里的配额短语抢成不可重试的 `QUOTA`。
            真实原因（detail）仍由日志与账号池承载。 */
            let error = super::pool::client_error_of(status, &detail);
            let frame = json!({
                "error": {
                    "message": error.message,
                    "type": error.kind,
                    "code": error.status,
                    // ★ issue #129：**上游原文**（客户端忽略，只给本进程的
                    // `UsageTapStream` 记日志用）。对外文案为了能被客户端归成可重试
                    // 而刻意抹掉了真实原因 —— 但「对外静默」绝不能变成「对内也静默」。
                    // 带上等价状态码：日志里「HTTP 429：quota exceeded」比只有原文
                    // 更容易与客户端的分类对上（两者都靠这个数字）。
                    "upstream": format!("HTTP {status}：{detail}")
                        .chars()
                        .take(400)
                        .collect::<String>(),
                }
            });
            (vec![Bytes::from(format!("data: {frame}\n\n"))], true)
        }
    }
}

/// 把上游字节流适配为**下游 OpenAI SSE 字节流**（逐帧解包 + [DONE] 兜底）。
pub struct QoderOpenAiStream<S> {
    inner: Pin<Box<S>>,
    buffer: Vec<u8>,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
    done_sent: bool,
    terminated: bool,
    /// ★ issue #129：断流时是否补**可重试**的错误帧（开关 `auto_failover`）。
    ///
    /// 关闭（默认）时行为与改造前逐字一致：只补 `[DONE]`。
    failover: bool,
    /// 错误帧是否已发（上游显式错误帧已发过就不再补）。
    error_frame_sent: bool,
    /// 失败槽：断流时写一条 502，供日志 / 冷却 / 粘性解绑读取。
    error_slot: Option<std::sync::Arc<std::sync::Mutex<Option<UpstreamError>>>>,
}

impl<S> QoderOpenAiStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner: Box::pin(inner),
            buffer: Vec::new(),
            pending: VecDeque::new(),
            upstream_done: false,
            done_sent: false,
            terminated: false,
            failover: false,
            error_frame_sent: false,
            error_slot: None,
        }
    }

    /// ★ issue #129：接入失败信号。
    ///
    /// `failover = false`（默认）时**完全不影响字节**：断流照旧只补 `[DONE]`。
    pub fn with_failover(
        mut self,
        failover: bool,
        error_slot: std::sync::Arc<std::sync::Mutex<Option<UpstreamError>>>,
    ) -> Self {
        self.failover = failover;
        self.error_slot = Some(error_slot);
        self
    }

    fn drain_lines(&mut self) {
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=position).collect();
            let text = String::from_utf8_lossy(&line);
            let (frames, done) = process_line(&text);
            if done && !frames.is_empty() {
                // `[DONE]` 时不产帧；产帧且结束 ⇒ 这一帧是**错误帧**（已对外发过）。
                self.error_frame_sent = true;
            }
            self.pending.extend(frames);
            if done {
                self.upstream_done = true;
            }
        }
    }

    fn push_done(&mut self) {
        if !self.done_sent {
            self.pending
                .push_back(Bytes::from_static(b"data: [DONE]\n\n"));
            self.done_sent = true;
        }
    }
}

impl<S, E> Stream for QoderOpenAiStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Debug,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(frame) = this.pending.pop_front() {
                return Poll::Ready(Some(Ok(frame)));
            }
            if this.terminated {
                return Poll::Ready(None);
            }
            if this.upstream_done {
                this.push_done();
                this.terminated = true;
                continue;
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    this.buffer.extend_from_slice(&chunk);
                    this.drain_lines();
                }
                Poll::Ready(Some(Err(error))) => {
                    /* ★ issue #129 §3.3：断流**不是**「模型说完了」。
                    开关开启时补一条可重试错误帧并写失败槽（日志/冷却/粘性解绑读它）；
                    关闭时保持改造前行为：只补 `[DONE]`。 */
                    if this.failover && !this.error_frame_sent {
                        let detail = format!("上游连接中断：{error:?}");
                        if let Some(slot) = this.error_slot.as_ref() {
                            if let Ok(mut guard) = slot.lock() {
                                if guard.is_none() {
                                    *guard = Some(UpstreamError {
                                        message: detail.chars().take(400).collect(),
                                        code: 502,
                                    });
                                }
                            }
                        }
                        let err = super::pool::client_error_of(502, &detail);
                        let frame = json!({
                            "error": {
                                "message": err.message,
                                "type": err.kind,
                                "code": err.status,
                                "upstream": detail.chars().take(400).collect::<String>(),
                            }
                        });
                        this.pending
                            .push_back(Bytes::from(format!("data: {frame}\n\n")));
                        this.error_frame_sent = true;
                    }
                    this.upstream_done = true;
                }
                Poll::Ready(None) => {
                    // 上游自然结束：处理残余缓冲（末行可能没有换行）。
                    if !this.buffer.is_empty() {
                        let text = String::from_utf8_lossy(&this.buffer).into_owned();
                        this.buffer.clear();
                        let (frames, done) = process_line(&text);
                        this.pending.extend(frames);
                        if done {
                            this.upstream_done = true;
                        }
                    }
                    if this.upstream_done {
                        this.push_done();
                        this.terminated = true;
                    } else {
                        // 没见到 [DONE] 就断了（或末尾就是最后一帧）——补上，绝不悬挂。
                        this.upstream_done = true;
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// 上游中途报错的结构化摘要（供日志 / 探测读取）。
///
/// ★ 编译修复（2026-10-22）：本类型在并发编辑中丢失了定义，而引用处
/// （`UsageTapStream` 的错误槽 + `qoder/routes.rs` 的探针）都按
/// `{ message, code }` 构造 ⇒ 这里按**引用处的事实**补回，不新增语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamError {
    /// 可读信息（已截断，与 `AggregatedChat::upstream_error` 同源）。
    pub message: String,
    /// 上游错误码（`statusCodeValue`，缺省 502）。
    pub code: u16,
}

/// 非流式聚合结果。
#[derive(Debug, Default, Clone)]
pub struct AggregatedChat {
    pub id: Option<String>,
    pub model: Option<String>,
    pub created: Option<i64>,
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<Value>,
    pub finish_reason: String,
    pub usage: Option<Value>,
    /// 上游中途报错时的可读信息（与 `content` 并存时以错误为准）。
    pub upstream_error: Option<String>,
    /// 错误帧的 `code`（`Some(429)` = 上游排队，稍后重试）。
    ///
    /// ★ 编译修复（2026-10-22）：引用处已按此字段分流日志状态码
    /// （排队记 429 / 其它记 502），但字段定义在并发编辑中丢失 ⇒ 按引用处补回。
    pub upstream_error_code: Option<u16>,
}

/// 探测用：读一次上游流，取**第一帧上游错误**（`(status, detail)`）。
///
/// ## 为什么必须读流（探测的真机教训，2026-10-22）
///
/// Qoder 上游的失败**不一定**体现在 HTTP 状态码上 —— **排队**（`10605` / `isQueued`）
/// 就是 **HTTP 200 + 信封 `statusCodeValue:403`**。只看 HTTP 状态会把「排队」判成
/// 「可用」（假阴性）：实测同一账号同一模型，探测报「全部可用（上游 200）」，
/// 而真实流量拿到的是排队错误帧。
///
/// 与 [`aggregate`] 的区别：它**只找错误**（不做正文聚合 / 不做回声还原）——
/// 探测要的是「这一格能不能用」，不需要正文。
///
/// ## 字节上限
///
/// 探测请求是 `max_tokens:1` + 短文案，正常流极短；但为防上游不听话地狂吐正文，
/// 读满 [`PROBE_ERROR_SCAN_MAX_BYTES`] 即停止（**错误帧总在最前面**，不影响判据）。
///
/// 无错误 ⇒ `None`（正常流，或流里只有正常 chunk）。
pub async fn first_upstream_error<S, E>(upstream: S) -> Option<(u16, String)>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Debug,
{
    /* ★ 编译修复（2026-10-22）：`upstream.next()` 要求 `Unpin`，而本函数对 `S` 只有
    `Stream` 约束 ⇒ 加 `+ Unpin` 会把负担推给调用方（他们传的流未必 Unpin）。
    与同文件 [`aggregate`] 一致：用 `pin_mut!` 就地钉住，**不额外要求 Unpin**。 */
    futures_util::pin_mut!(upstream);
    let mut buffer = String::new();
    let mut read = 0usize;
    while let Some(item) = upstream.next().await {
        let Ok(chunk) = item else { break };
        read += chunk.len();
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(position) = buffer.find('\n') {
            // ★ 区间必须是**开区间** `..position + 1`（含换行）——
            //   `..=position + 1`（闭区间）在 `\n` 位于**最后一个字节**时越界 panic
            //   （`range end index N out of range for slice of length N`）。
            let line: String = buffer.drain(..position + 1).collect();
            if let Some(found) = error_in_line(&line) {
                return Some(found);
            }
        }
        if read >= PROBE_ERROR_SCAN_MAX_BYTES {
            break;
        }
    }
    // 末行可能没有换行结尾（上游偶尔如此）。
    if !buffer.is_empty() {
        return error_in_line(&buffer);
    }
    None
}

/// 探测读流的字节上限（见 [`first_upstream_error`]）。
pub const PROBE_ERROR_SCAN_MAX_BYTES: usize = 64 * 1024;

/// 一行里有没有上游错误帧？（复用 [`api::unwrap_envelope_line`] —— 判据零新造）
fn error_in_line(line: &str) -> Option<(u16, String)> {
    match api::unwrap_envelope_line(line.trim_end()) {
        api::ChatFrame::UpstreamError { status, detail } => Some((status, detail)),
        _ => None,
    }
}

/// 把上游信封流折叠成一个 `chat.completion` 数据（与官方客户端语义对齐）。
///
/// - `content` 支持字符串或 parts 数组两种 delta 形态；
/// - `tool_calls` 按 `index` 归并（分片到达的参数做字符串拼接）；
/// - `reasoning_content` 单独收集（不进正文）；
/// - **泄漏回声回读**：上游没给结构化 `tool_calls` 但正文恰好是
///   `[assistant 请求调用工具]` + JSON 数组形态 ⇒ 还原为结构化并清空正文
///   （严格整段识别，见 `core::modules::qoder::api::parse_leaked_tool_calls`）。
pub async fn aggregate<S, E>(
    upstream: S,
    allowed_tool_names: Option<&std::collections::HashSet<String>>,
) -> AggregatedChat
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Debug,
{
    let mut out = AggregatedChat {
        finish_reason: "stop".to_string(),
        ..Default::default()
    };
    let mut tool_map: std::collections::BTreeMap<i64, Map<String, Value>> =
        std::collections::BTreeMap::new();
    let stream = QoderOpenAiStream::new(upstream);
    futures_util::pin_mut!(stream);

    while let Some(item) = stream.next().await {
        let Ok(frame) = item else { break };
        let text = String::from_utf8_lossy(&frame);
        let Some(data) = text.strip_prefix("data: ") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(error) = chunk.get("error") {
            /* ★ issue #129：优先取 `upstream`（上游原文 + 等价状态码）。
            `message` 现在是对外收敛后的文案（为了能被客户端归成可重试），
            拿它当 `upstream_error` 会把真实原因从日志里抹掉。 */
            let message = error
                .get("upstream")
                .and_then(Value::as_str)
                .or_else(|| error.get("message").and_then(Value::as_str))
                .unwrap_or("上游错误")
                .to_string();
            // ★ 一并记下错误帧的 `code`（排队 ⇒ 429）：账本据此区分
            //   「上游排队（429，稍后重试）」与「上游其它错误（502）」。
            out.upstream_error_code = error
                .get("code")
                .and_then(Value::as_u64)
                .and_then(|code| u16::try_from(code).ok());
            out.upstream_error = Some(message);
            continue;
        }
        if let Some(id) = chunk.get("id").and_then(Value::as_str) {
            out.id = Some(id.to_string());
        }
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            out.model = Some(model.to_string());
        }
        if let Some(created) = chunk.get("created").and_then(Value::as_i64) {
            out.created = Some(created);
        }
        if let Some(usage) = chunk.get("usage") {
            if !usage.is_null() {
                out.usage = Some(usage.clone());
            }
        }
        let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
            continue;
        };
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                match delta.get("content") {
                    Some(Value::String(text)) => out.content.push_str(text),
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            match part {
                                Value::String(text) => out.content.push_str(text),
                                Value::Object(object) => {
                                    if let Some(text) = object.get("text").and_then(Value::as_str) {
                                        out.content.push_str(text);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
                    out.reasoning.push_str(reasoning);
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        let index = call
                            .get("index")
                            .and_then(Value::as_i64)
                            .unwrap_or(tool_map.len() as i64);
                        let entry = tool_map.entry(index).or_insert_with(|| {
                            let mut map = Map::new();
                            map.insert(
                                "id".into(),
                                call.get("id").cloned().unwrap_or_else(|| {
                                    Value::String(format!("call_{}", uuid::Uuid::new_v4().simple()))
                                }),
                            );
                            map.insert(
                                "type".into(),
                                Value::String(
                                    call.get("type")
                                        .and_then(Value::as_str)
                                        .unwrap_or("function")
                                        .to_string(),
                                ),
                            );
                            map.insert("function".into(), json!({"name": "", "arguments": ""}));
                            map
                        });
                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                            entry.insert("id".into(), Value::String(id.to_string()));
                        }
                        if let Some(function) = call.get("function") {
                            let slot = entry
                                .get_mut("function")
                                .and_then(Value::as_object_mut)
                                .expect("function 槽位已在上面建立");
                            if let Some(name) = function.get("name").and_then(Value::as_str) {
                                let current = slot
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                slot.insert(
                                    "name".into(),
                                    Value::String(format!("{current}{name}")),
                                );
                            }
                            if let Some(arguments) =
                                function.get("arguments").and_then(Value::as_str)
                            {
                                let current = slot
                                    .get("arguments")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                slot.insert(
                                    "arguments".into(),
                                    Value::String(format!("{current}{arguments}")),
                                );
                            }
                        }
                    }
                }
                if let Some(function_call) = delta.get("function_call") {
                    if function_call.get("name").and_then(Value::as_str).is_some() {
                        let entry = tool_map.entry(0).or_insert_with(|| {
                            json!({
                                "id": format!("call_{}", uuid::Uuid::new_v4().simple()),
                                "type": "function",
                                "function": {"name": "", "arguments": ""},
                            })
                            .as_object()
                            .cloned()
                            .unwrap_or_default()
                        });
                        let slot = entry
                            .get_mut("function")
                            .and_then(Value::as_object_mut)
                            .expect("function 槽位已在上面建立");
                        if let Some(name) = function_call.get("name").and_then(Value::as_str) {
                            slot.insert("name".into(), Value::String(name.to_string()));
                        }
                        if let Some(arguments) =
                            function_call.get("arguments").and_then(Value::as_str)
                        {
                            let current = slot
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            slot.insert(
                                "arguments".into(),
                                Value::String(format!("{current}{arguments}")),
                            );
                        }
                    }
                }
            }
            if let Some(finish) = choice.get("finish_reason").and_then(Value::as_str) {
                out.finish_reason = finish.to_string();
            }
        }
    }

    out.tool_calls = tool_map.into_values().map(Value::Object).collect();

    // 泄漏回声回读：只在**没有**结构化调用、且正文非空时尝试（严格整段识别）。
    if out.tool_calls.is_empty() && !out.content.is_empty() {
        let (mut recovered, mut cleaned) =
            api::parse_leaked_tool_calls(&out.content, allowed_tool_names);
        if recovered.is_none() {
            // 收尾容错：数组后面跟着 DSML 闭标签残片（issue #66 的真机形态）。
            let (tolerant, tail_cleaned) =
                api::parse_leaked_tool_calls_with_trailing(&out.content, allowed_tool_names);
            recovered = tolerant;
            cleaned = tail_cleaned;
        }
        if let Some(calls) = recovered {
            out.tool_calls = calls;
            out.content = cleaned;
            if out.finish_reason == "stop" {
                out.finish_reason = "tool_calls".to_string();
            }
        }
    }
    // DSML 回退：模型把调用写成 `<｜｜DSML｜｜ calls>` 标签时还原成结构化调用。
    // 前置：本次请求**声明了 tools** —— 没声明就没有「调用」这回事，合成出来客户端
    // 也执行不了（那条路只会把正文变成一次必然报错的工具调用）。
    let tools_declared = allowed_tool_names
        .map(|names| !names.is_empty())
        .unwrap_or(false);
    if tools_declared && out.tool_calls.is_empty() && !out.content.is_empty() {
        let (recovered, cleaned) = api::parse_dsml_tool_calls(&out.content);
        if let Some(calls) = recovered {
            out.tool_calls = calls;
            out.content = cleaned;
            if out.finish_reason == "stop" {
                out.finish_reason = "tool_calls".to_string();
            }
        }
    }
    // 最后一道：还原不了的内部协议文本（截断回声 / 工具结果回声）**吞掉**，
    // 绝不把网关自己的标记交给终端用户（issue #66）。
    if out.tool_calls.is_empty() && !out.content.is_empty() {
        if api::leaked_partial_droppable(&out.content, allowed_tool_names) {
            // 截断的调用回声：非流式也判失败（与流式同口径），否则调用方拿到
            // 「200 + 空正文」，把没执行工具的一轮当成成功。
            out.content.clear();
            out.upstream_error = Some(TRUNCATED_TOOL_CALL_ERROR.to_string());
        } else if api::tool_echo_droppable(&out.content, allowed_tool_names) {
            out.content.clear();
        }
    }
    out
}

/// 旁路取 `usage` / 上游错误的流适配器：逐帧透传，顺手把含 `usage` 或 `error`
/// 的 chunk 记进槽位。
///
/// **必须挂在 Anthropic 转换之前**（转换后帧是 Anthropic 事件，取不到 OpenAI 的
/// `usage` 字段）。放在流链里而不是放进消费循环 —— 两种入口（OpenAI / Anthropic）
/// 共用同一条链，取数自然同源。
///
/// ★ 错误槽位是「故障不隐身」的一半：流式请求的账本在**流结束**时才落，若上游中途
/// 发了错误帧而我们只记 200，日志就会显示「200 + 0 token + 无错误」——正是
/// issue #46 里最难排查的那种形态。
pub struct UsageTapStream<S> {
    inner: S,
    slot: std::sync::Arc<std::sync::Mutex<Option<Value>>>,
    error_slot: std::sync::Arc<std::sync::Mutex<Option<UpstreamError>>>,
}

impl<S> UsageTapStream<S> {
    pub fn new(
        inner: S,
        slot: std::sync::Arc<std::sync::Mutex<Option<Value>>>,
        error_slot: std::sync::Arc<std::sync::Mutex<Option<UpstreamError>>>,
    ) -> Self {
        Self {
            inner,
            slot,
            error_slot,
        }
    }
}

/// 从一帧下游数据里解析 `usage` / `error`（不含这两个键的帧直接跳过）。
fn tap_frame(
    frame: &Bytes,
    slot: &std::sync::Mutex<Option<Value>>,
    error_slot: &std::sync::Mutex<Option<UpstreamError>>,
) {
    let Ok(text) = std::str::from_utf8(frame) else {
        return;
    };
    let Some(data) = text.strip_prefix("data: ") else {
        return;
    };
    let data = data.trim();
    if data == "[DONE]" || !(data.contains("usage") || data.contains("error")) {
        return;
    }
    if let Ok(chunk) = serde_json::from_str::<Value>(data) {
        // 错误帧（`{"error":{"message":…}}`）单独留痕：客户端已经收到它，账本也要记。
        if let Some(error) = chunk.get("error") {
            /* ★ issue #129：**优先取 `upstream`**（上游原文）—— `message` 现在是对外
            收敛后的文案（含 `HTTP 429`、不含真实原因），拿它写日志等于把故障原因抹掉。 */
            let message = error
                .get("upstream")
                .and_then(Value::as_str)
                .or_else(|| error.get("message").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            // ★ 一并记下 `code`（排队 ⇒ 429）：账本据此区分「排队」与「上游其它错误」。
            let code = error
                .get("code")
                .and_then(Value::as_u64)
                .and_then(|code| u16::try_from(code).ok())
                .unwrap_or(502);
            if let Ok(mut guard) = error_slot.lock() {
                if guard.is_none() {
                    *guard = Some(UpstreamError {
                        message: message.chars().take(400).collect(),
                        code,
                    });
                }
            }
        }
        if let Some(usage) = chunk.get("usage") {
            if !usage.is_null() {
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(usage.clone());
                }
            }
        }
    }
}

impl<S, E> Stream for UsageTapStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                tap_frame(&frame, &this.slot, &this.error_slot);
                Poll::Ready(Some(Ok(frame)))
            }
            other => other,
        }
    }
}

/// 扣帧期允许累积的字节上限（超出 ⇒ fail-open 原样透传）。
///
/// 取 64 KiB：合法回声（标记 + 工具参数数组）远小于它；而正文一旦超过这个量级，
/// 它已经不可能是「模型把整段工具调用原样回声」，继续扣着只会白等用户。
const LEAK_GUARD_MAX_HELD_BYTES: usize = 64 * 1024;

/// 泄漏守卫的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeakGuardMode {
    /// 逐字透传（默认）。**只有正文看起来是标记前缀时才进入扣帧态**。
    Passthrough,
    /// 累计正文仍是 [`api::LEAK_MARKER`] 的**逐字节前缀** —— 还在等后续分片。
    Probing,
    /// 正文已越过标记前缀、且形状像回声（标记后紧跟 `[`）—— 等到流末整段判定。
    Recovering,
}

/// 还原失败时对扣住正文的处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeakResolution {
    /// 截断的回声：判本轮失败（客户端重试）。
    FailTurn,
    /// 工具结果回声：吞掉正文，本轮继续。
    Drop,
    /// 正常散文 / 未声明工具：原样透传。
    Passthrough,
}

/// 累计正文与「回声」的形状关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeakShape {
    /// 仍是标记的逐字节前缀。
    Prefix,
    /// 标记开头 + 后面是 `[`（JSON 数组的起点）—— 有可能。
    Echo,
    /// 不可能是回声。
    NotEcho,
}

/// 在一帧正文里定位「标记起点」。
///
/// - 命中完整标记 ⇒ 返回该字节偏移（标记之前的散文立即发出）；
/// - 帧尾是某个标记的**真前缀** ⇒ 返回该尾巴的偏移（跨帧补全窗口：服务端 delta
///   切分不可控，标记很可能跨两帧）；
/// - 无命中 ⇒ `None`（正常正文，逐字透传）。
fn leak_marker_split(text: &str) -> Option<usize> {
    let markers = [
        api::LEAK_MARKER,
        api::LEAK_MARKER_ALT,
        api::TOOL_RESULT_MARKER,
    ];
    let mut best: Option<usize> = None;
    for marker in markers {
        if let Some(position) = text.find(marker) {
            best = Some(best.map_or(position, |current| current.min(position)));
        }
    }
    if best.is_some() {
        return best;
    }
    for marker in markers {
        let boundaries: Vec<usize> = marker.char_indices().map(|(index, _)| index).collect();
        for index in boundaries.into_iter().rev().filter(|index| *index > 0) {
            let prefix = &marker[..index];
            if text.ends_with(prefix) {
                return Some(text.len() - prefix.len());
            }
        }
    }
    None
}

/// 截断回声 / 截断 DSML 的**失败文案**。
///
/// 为什么不「静默给空回答」：客户端会把空回答当**正常结束**，任务就停在
/// 「工具没执行」的半路（issue #66 的体感正是「断好多次」）。判失败让客户端重试，
/// 比伪装成完成诚实（参考实现 issue #72 的取舍）。
pub const TRUNCATED_TOOL_CALL_ERROR: &str =
    "上游返回的工具调用被截断（无法还原），本轮已判失败以便客户端重试；未执行任何工具。";

/// 合成一帧 OpenAI 风格的流内错误（客户端据此失败/重试）。
fn truncated_tool_call_frame() -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        json!({
            "error": {
                "message": TRUNCATED_TOOL_CALL_ERROR,
                "type": "upstream_error",
                "code": 502,
            }
        })
    ))
}

/// 去掉开头的**调用回声**标记（两种括号写法都认）。
fn strip_call_marker(text: &str) -> Option<&str> {
    text.strip_prefix(api::LEAK_MARKER)
        .or_else(|| text.strip_prefix(api::LEAK_MARKER_ALT))
}

/// 累计正文处于哪种形状（判定与 [`api::parse_leaked_tool_calls`] 同源：标记 + `[` 开头）。
fn classify(text: &str) -> LeakShape {
    for marker in [
        api::LEAK_MARKER,
        api::LEAK_MARKER_ALT,
        api::TOOL_RESULT_MARKER,
    ] {
        if marker.starts_with(text) {
            return LeakShape::Prefix;
        }
    }
    if text.starts_with(api::TOOL_RESULT_MARKER) {
        // 工具结果回声：形状不是 JSON 数组，只能整段留到流末按判据吞掉 / 放行。
        return LeakShape::Echo;
    }
    let Some(rest) = strip_call_marker(text) else {
        return LeakShape::NotEcho;
    };
    let rest = rest.trim_start();
    if rest.is_empty() || rest.starts_with('[') {
        LeakShape::Echo
    } else {
        LeakShape::NotEcho
    }
}

/// 解出 `data: {...}` 帧里的 JSON（`[DONE]` / 不可解析 ⇒ `None`）。
fn parse_frame(frame: &Bytes) -> Option<Value> {
    let text = std::str::from_utf8(frame).ok()?;
    let data = text.strip_prefix("data: ")?.trim();
    if data == "[DONE]" {
        return None;
    }
    serde_json::from_str::<Value>(data).ok()
}

/// 一帧 delta 里的正文（字符串或 parts 数组都拼起来）；没有正文 ⇒ `None`。
fn delta_content(chunk: &Value) -> Option<String> {
    let choices = chunk.get("choices")?.as_array()?;
    let mut text = String::new();
    let mut found = false;
    for choice in choices {
        let Some(delta) = choice.get("delta") else {
            continue;
        };
        match delta.get("content") {
            Some(Value::String(value)) => {
                text.push_str(value);
                found = true;
            }
            Some(Value::Array(parts)) => {
                for part in parts {
                    match part {
                        Value::String(value) => {
                            text.push_str(value);
                            found = true;
                        }
                        Value::Object(object) => {
                            if let Some(value) = object.get("text").and_then(Value::as_str) {
                                text.push_str(value);
                                found = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    found.then_some(text)
}

/// 一帧是否**已经带结构化 `tool_calls`**。
///
/// ★ 这是「绝不双发」的判据（P0-A0）：上游既给了结构化调用、又在正文里带了标记时，
/// 守卫**不再合成第二组** —— 否则客户端会把同一组工具执行两次。
fn has_structured_tool_calls(chunk: &Value) -> bool {
    chunk
        .get("choices")
        .and_then(Value::as_array)
        .map(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("delta")
                    .and_then(|delta| delta.get("tool_calls"))
                    .and_then(Value::as_array)
                    .map(|calls| !calls.is_empty())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// 把扣住的一帧改写成「无正文」（`finish_reason` 可选改写为 `tool_calls`）。
///
/// ⚠️ **必须无条件去掉 `delta.content`** —— 纯正文帧（没有 `finish_reason`）同样是回声，
///   留着就会把标记原样放回客户端（第一版实现的两处 bug 都由
///   `leak_guard_recovers_echo_into_structured_tool_calls` 与
///   `leak_guard_never_synthesizes_a_second_tool_call_set` 钉住）。
/// `saw_finish` 是**出参**：本帧是否改写过一个非空 `finish_reason`（决定要不要补收尾帧）。
/// 不可解析 ⇒ 原样字节。
fn strip_content(frame: &Bytes, finish_reason: Option<&str>, saw_finish: &mut bool) -> Bytes {
    let Some(text) = std::str::from_utf8(frame).ok() else {
        return frame.clone();
    };
    let Some(rest) = text.strip_prefix("data: ") else {
        return frame.clone();
    };
    let Ok(Value::Object(mut chunk)) = serde_json::from_str::<Value>(rest.trim()) else {
        return frame.clone();
    };
    let Some(choices) = chunk.get_mut("choices").and_then(Value::as_array_mut) else {
        return frame.clone();
    };
    for choice in choices.iter_mut() {
        let Some(object) = choice.as_object_mut() else {
            continue;
        };
        if let Some(delta) = object.get_mut("delta").and_then(Value::as_object_mut) {
            delta.remove("content");
        }
        if let Some(reason) = finish_reason {
            if object
                .get("finish_reason")
                .map(|value| !value.is_null())
                .unwrap_or(false)
            {
                object.insert("finish_reason".into(), Value::String(reason.into()));
                *saw_finish = true;
            }
        }
    }
    Bytes::from(format!("data: {}\n\n", Value::Object(chunk)))
}

/// 流式**泄漏回声守卫**：把模型原样回声的 `[assistant 请求调用工具]` + JSON 数组
/// 还原成结构化 `tool_calls` 帧，与非流式 [`aggregate`] 同一判据、同一函数
/// （`api::parse_leaked_tool_calls`）。
///
/// ## 为什么必须有这一层（issue #46 的 A 项）
///
/// 文本模式下历史里的工具调用会被序列化进正文，模型很可能**原样回声**；
/// 非流式路径在 [`aggregate`] 末尾兜底还原，而**流式中继是逐帧透传的** ——
/// 泄漏文本会直接打到客户端界面。
///
/// ## 接入点（为什么在 `QoderOpenAiStream` **之外**、`UsageTapStream` **之内**）
///
/// - 流式入口 `routes::stream_relay` 的链是
///   `QoderOpenAiStream → 本守卫 → UsageTapStream →（Anthropic 时）转换`，
///   本层**必经**，且合成的是 OpenAI 形状的帧 ⇒ Anthropic 入口自动覆盖；
/// - [`aggregate`] 内部自建 `QoderOpenAiStream`、**不经过本守卫** ⇒
///   **结构上不可能被处理两次**（两次还原会各生成一组随机 id 的 `tool_calls`，
///   客户端会把同一组工具执行两遍）；
/// - [`QoderOpenAiStream::new`] 的签名与全部调用点保持不变。
///
/// ## 正常流**逐字不变**
///
/// 只有「正文看起来是标记开头」才扣帧；一旦分叉（或超上限 / 上游已给结构化调用 /
/// 流末仍不是合法回声）即**原样透传**（fail-open）。首帧若是正文，回执延迟至多一帧。
///
/// ## 兜底合成形状（`finish_reason` 必须在 usage 之前）
///
/// `[anthropic::push_tool_call]` 读 `chunk["choices"][0]["delta"]["tool_calls"][i]["index"]`
/// 且 `unwrap_or(0)` ⇒ 合成帧**必须**逐个带 `index`，否则并行调用会被压成同一个；
/// `AnthropicSseStream` 又按「**见到 finish_reason 就补 usage**」收尾
/// ⇒ 合成帧的 `finish_reason` 必须落在上游 usage 帧**之前**，否则 usage 永远补不出来。
pub struct LeakGuardStream<S> {
    inner: S,
    mode: LeakGuardMode,
    text: String,
    held: std::collections::VecDeque<Bytes>,
    held_bytes: usize,
    saw_tool_calls: bool,
    allowed_tools: Option<std::sync::Arc<std::collections::HashSet<String>>>,
    out: std::collections::VecDeque<Bytes>,
    finished: bool,
}

impl<S> LeakGuardStream<S> {
    pub fn new(
        inner: S,
        allowed_tools: Option<std::sync::Arc<std::collections::HashSet<String>>>,
    ) -> Self {
        Self {
            inner,
            mode: LeakGuardMode::Passthrough,
            text: String::new(),
            held: std::collections::VecDeque::new(),
            held_bytes: 0,
            saw_tool_calls: false,
            allowed_tools,
            out: std::collections::VecDeque::new(),
            finished: false,
        }
    }

    /// 放弃扣帧：把扣住的帧**原样**按顺序放出（fail-open 的唯一出口）。
    fn flush_held(&mut self) {
        while let Some(frame) = self.held.pop_front() {
            self.out.push_back(frame);
        }
        self.held_bytes = 0;
        self.text.clear();
        self.saw_tool_calls = false;
        self.mode = LeakGuardMode::Passthrough;
    }

    /// 剥掉扣住帧里的回声正文，但**保留上游自己的 `finish_reason`**（fail-open 的「清泄漏」半边）。
    ///
    /// 用在「上游已给结构化 `tool_calls`、绝不能合成第二组」那条路上：
    /// 不合成调用是防双发，剥正文是清泄漏 —— **两件事必须分别做**，
    /// 合成不了就整帧放行等于把标记原样还给用户（那正是 issue #46 的症状）。
    fn flush_held_without_leaking(&mut self) {
        let held: Vec<Bytes> = self.held.drain(..).collect();
        let mut saw_finish = false;
        for frame in held {
            self.out
                .push_back(strip_content(&frame, None, &mut saw_finish));
        }
        self.held_bytes = 0;
        self.text.clear();
        self.saw_tool_calls = false;
        self.mode = LeakGuardMode::Passthrough;
    }

    /// 扣帧期收尾：只对**确实像回声**的情形做还原，其余一律原样透传。
    fn finalize(&mut self) {
        match self.mode {
            LeakGuardMode::Passthrough => {}
            /* 标记只写了一半（上游断流 / 恰好停在前缀）⇒ 原样透传：
            宁可让用户看见半句标记，也不要凭空造出工具调用。 */
            LeakGuardMode::Probing => match self.resolution() {
                LeakResolution::FailTurn => self.fail_turn(),
                LeakResolution::Drop => self.flush_held_without_leaking(),
                _ => self.flush_held(),
            },
            LeakGuardMode::Recovering => {
                if self.saw_tool_calls {
                    /* 上游已给结构化调用 ⇒ 绝不合成第二组（双发）。
                    ⚠️ 但**仍要剥掉回声正文**：不合成 ≠ 把标记原样还给用户
                    （那是 issue #46 的症状）。`finish_reason` 保持上游原值。 */
                    self.flush_held_without_leaking();
                    return;
                }
                let allowed = self.allowed_tools.as_deref();
                let (mut calls, mut cleaned) = api::parse_leaked_tool_calls(&self.text, allowed);
                if calls.is_none() {
                    // 收尾容错：`marker + JSON 数组 + DSML 闭标签残片`（issue #66 的真机形态）。
                    let (tolerant, tail_cleaned) =
                        api::parse_leaked_tool_calls_with_trailing(&self.text, allowed);
                    calls = tolerant;
                    cleaned = tail_cleaned;
                }
                let Some(calls) = calls.filter(|calls| !calls.is_empty()) else {
                    // 还原不了：截断回声 ⇒ 判失败（客户端重试）；工具结果回声 ⇒ 吞正文；
                    // 其余（正常散文、未声明工具）⇒ 原样透传。
                    match self.resolution() {
                        LeakResolution::FailTurn => self.fail_turn(),
                        LeakResolution::Drop => self.flush_held_without_leaking(),
                        _ => self.flush_held(),
                    }
                    return;
                };
                self.emit_recovered(calls, &cleaned);
            }
        }
    }

    /// 还原失败时，这段正文是否属于「绝对不能透传」的内部协议文本。
    ///
    /// 两条判据都在 core 里（与 `aggregate` 共用，不另写一套）：
    /// - 截断的调用回声（JSON 数组没写完）；
    /// - `[工具结果…]` 回声块。
    ///
    /// 两者都以「本次请求声明了 tools」为前置 —— 未声明工具时模型不可能在调工具，
    /// 此时整段只可能是正常讨论，必须 fail-open 透传。
    fn resolution(&self) -> LeakResolution {
        let allowed = self.allowed_tools.as_deref();
        if api::leaked_partial_droppable(&self.text, allowed) {
            // 截断的调用回声：还原不了 ⇒ **判失败**（让客户端重试，而不是当空回答结束）。
            return LeakResolution::FailTurn;
        }
        if api::tool_echo_droppable(&self.text, allowed) {
            // 工具结果回声：吞掉即可（本轮本来就该继续，不需要重试）。
            return LeakResolution::Drop;
        }
        LeakResolution::Passthrough
    }

    /// 判本轮失败：丢掉扣住的帧，发一帧流内错误，并把模式复位。
    fn fail_turn(&mut self) {
        self.held.clear();
        self.held_bytes = 0;
        self.text.clear();
        self.saw_tool_calls = false;
        self.mode = LeakGuardMode::Passthrough;
        self.out.push_back(truncated_tool_call_frame());
    }

    /// 还原成功：合成 `tool_calls` 帧 + 把扣住的帧去正文后按原顺序放回。
    ///
    /// `cleaned` 是 [`api::parse_leaked_tool_calls`] 给出的「清掉标记后的正文」
    /// （识别成功时为空串）。它被当作**唯一**正文来源：扣住的帧一律去正文，
    /// 而 `cleaned` 若非空（防御性：识别成功却带出正文）则**先**补一帧正文 ——
    /// 这样「正文」永远等于判据算出来的结果，而不是「我们以为的正文」。
    fn emit_recovered(&mut self, calls: Vec<Value>, cleaned: &str) {
        // 模板取最后一个能解析的帧（沿用上游的 `id` / `model` / `created`）。
        let mut template = Map::new();
        for frame in &self.held {
            if let Some(chunk) = parse_frame(frame) {
                if let Some(object) = chunk.as_object() {
                    for key in ["id", "model", "created"] {
                        if let Some(value) = object.get(key) {
                            template.insert(key.to_string(), value.clone());
                        }
                    }
                }
            }
        }
        // ⚠️ 逐个带 `index`：Anthropic 侧靠它归并并行调用（`unwrap_or(0)` 会压成同一个）。
        let tool_calls: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(index, call)| {
                let mut entry = Map::new();
                entry.insert("index".into(), Value::from(index));
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    if !id.is_empty() {
                        entry.insert("id".into(), Value::String(id.to_string()));
                    }
                }
                entry.insert(
                    "type".into(),
                    call.get("type")
                        .cloned()
                        .unwrap_or_else(|| Value::String("function".into())),
                );
                entry.insert(
                    "function".into(),
                    call.get("function").cloned().unwrap_or_else(|| json!({})),
                );
                Value::Object(entry)
            })
            .collect();
        // 没有上游 id 时补一个（客户端要求 `tool_calls[].id` 非空）。
        let tool_calls: Vec<Value> = tool_calls
            .into_iter()
            .map(|mut call| {
                if let Some(object) = call.as_object_mut() {
                    if !object.contains_key("id") {
                        object.insert(
                            "id".into(),
                            Value::String(format!("call_{}", uuid::Uuid::new_v4().simple())),
                        );
                    }
                }
                call
            })
            .collect();

        template.insert(
            "choices".into(),
            json!([{ "index": 0, "delta": { "tool_calls": tool_calls }, "finish_reason": null }]),
        );
        let synthesized = Bytes::from(format!("data: {}\n\n", Value::Object(template.clone())));

        let held: Vec<Bytes> = self.held.drain(..).collect();
        let mut saw_finish = false;
        let mut tail = Vec::new();
        // `cleaned` 非空时先补正文（识别成功却带出正文的防御分支）。
        if !cleaned.trim().is_empty() {
            let mut body = template.clone();
            body.insert(
                "choices".into(),
                json!([{ "index": 0, "delta": { "content": cleaned }, "finish_reason": null }]),
            );
            tail.push(Bytes::from(format!("data: {}\n\n", Value::Object(body))));
        }
        tail.push(synthesized);
        for frame in held {
            tail.push(strip_content(&frame, Some("tool_calls"), &mut saw_finish));
        }
        if !saw_finish {
            // 上游这一路没有 `finish_reason`（罕见）⇒ 补一帧收尾，
            // 否则客户端会一直等 `finish_reason`（Anthropic 侧还会补不出 usage）。
            tail.push(Bytes::from(
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            ));
        }
        for frame in tail {
            self.out.push_back(frame);
        }
        self.held_bytes = 0;
        self.text.clear();
        self.saw_tool_calls = false;
        self.mode = LeakGuardMode::Passthrough;
    }

    /// 处理一帧。
    fn push_frame(&mut self, frame: Bytes) {
        if self.mode == LeakGuardMode::Passthrough {
            let Some(chunk) = parse_frame(&frame) else {
                self.out.push_back(frame);
                return;
            };
            if has_structured_tool_calls(&chunk) {
                self.saw_tool_calls = true;
            }
            let Some(text) = delta_content(&chunk) else {
                self.out.push_back(frame);
                return;
            };
            let Some(split) = leak_marker_split(&text) else {
                self.out.push_back(frame);
                return;
            };
            /* 散文与标记落在**同一帧**：标记之前的散文立即发出（不拖首字延迟），
            标记及其后进扣帧流程。参考实现踩过这条：只判「整帧以标记开头」会漏，
            整帧透传 ⇒ 标记泄漏。 */
            let remainder = &text[split..];
            if split > 0 {
                self.out
                    .push_back(head_content_frame(&frame, &text[..split]));
            }
            self.text.push_str(remainder);
            self.held_bytes += remainder.len();
            self.held.push_back(replace_content(&frame, remainder));
            self.mode = match classify(&self.text) {
                LeakShape::Prefix => LeakGuardMode::Probing,
                _ => LeakGuardMode::Recovering,
            };
            return;
        }

        // 扣帧期：超上限 ⇒ fail-open（放行全部，不再判断）。
        let len = frame.len();
        if self.held_bytes + len > LEAK_GUARD_MAX_HELD_BYTES {
            self.flush_held();
            self.out.push_back(frame);
            return;
        }
        self.held_bytes += len;
        let Some(chunk) = parse_frame(&frame) else {
            self.held.push_back(frame);
            return;
        };
        if has_structured_tool_calls(&chunk) {
            self.saw_tool_calls = true;
        }
        let Some(text) = delta_content(&chunk) else {
            self.held.push_back(frame);
            return;
        };
        self.text.push_str(&text);
        /* 三态在这里**双向**流转：前缀继续等分片（Probing），越过标记前缀就转
        Recovering 等流末整段判定，分叉则立刻原样透传（fail-open）。
        ⚠️ 只判「不是回声就放行」是不够的 —— 那样 Probing 会一直停在 Probing，
        流末按 fail-open 放行，整段回声就会漏给客户端。 */
        self.mode = match classify(&self.text) {
            LeakShape::Prefix => LeakGuardMode::Probing,
            LeakShape::Echo => LeakGuardMode::Recovering,
            LeakShape::NotEcho => {
                self.held.push_back(frame);
                self.flush_held();
                return;
            }
        };
        self.held.push_back(frame);
    }
}

impl<S, E> Stream for LeakGuardStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(frame) = this.out.pop_front() {
                return Poll::Ready(Some(Ok(frame)));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => this.push_frame(frame),
                Poll::Ready(Some(Err(_error))) => {
                    /* 传输错误：把扣着的帧原样放出（fail-open），不悬挂。 */
                    this.flush_held();
                    this.finished = true;
                }
                Poll::Ready(None) => {
                    this.finalize();
                    this.finished = true;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// DSML 守卫最多扣住多少字节（正常调用块远小于此；超限 fail-open，绝不无限压住客户端）。
const DSML_GUARD_MAX_BUFFER: usize = 1024 * 1024;
/// 还没见到 `>` 的标签尾巴最长多少字符仍算「可能是标签开头」。
const DSML_TAG_TAIL_CHARS: usize = 40;

/// 一帧 `delta.content` 里**可能**开始一个 DSML 标签的字节偏移。
///
/// 两级判据：
/// 1. 帧里出现 `DSML` 字样 ⇒ 从该处向前回退到最近的 `<` 开始扣（一帧里可能已经是
///    完整块，不能漏）；
/// 2. 否则只认「帧尾是可能的标签开头」：`<` 之后没有 `>`，且短到还可能是标签名
///    （无空白）。数学式 `a < b`、`<div>` 一律不扣帧 —— 误判的代价是首字延迟，
///    所以要把误判压到最低。
fn dsml_candidate_start(text: &str) -> Option<usize> {
    let upper = text.to_ascii_uppercase();
    if let Some(position) = upper.find("DSML") {
        return Some(text[..position].rfind('<').unwrap_or(0));
    }
    let idx = text.rfind('<')?;
    let tail = &text[idx..];
    if tail.contains('>') {
        return None;
    }
    if tail.chars().count() < 10 && !tail.chars().any(char::is_whitespace) {
        return Some(idx);
    }
    None
}

/// 缓冲内容**是否仍是** DSML 块的候选（一旦证伪就立刻放行，不拖字）。
fn still_dsml_candidate(text: &str) -> bool {
    let t = text.trim_start();
    if !t.starts_with('<') {
        return false;
    }
    match t.find('>') {
        // 第一个标签已闭合：只有含 DSML 才是我们的块。
        Some(end) => t[..=end].to_ascii_uppercase().contains("DSML"),
        // 还没闭合：短尾巴仍可能是「标签名写到一半」。
        None => t.chars().count() < DSML_TAG_TAIL_CHARS,
    }
}

/// 一帧的 `finish_reason`（任一 choice 非 null 即算）。
fn chunk_finish_reason(chunk: &Value) -> Option<String> {
    chunk
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|choice| {
            choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

/// 把一帧的 `delta.content` 换成给定文本（其余字段原样保留）。
fn replace_content(frame: &Bytes, content: &str) -> Bytes {
    let Some(text) = frame
        .strip_prefix(b"data: ")
        .and_then(|rest| std::str::from_utf8(rest).ok())
    else {
        return frame.clone();
    };
    let Ok(Value::Object(mut chunk)) = serde_json::from_str::<Value>(text.trim()) else {
        return frame.clone();
    };
    if let Some(choices) = chunk.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices.iter_mut() {
            if let Some(delta) = choice.get_mut("delta").and_then(Value::as_object_mut) {
                delta.insert("content".into(), Value::String(content.to_string()));
            }
        }
    }
    Bytes::from(format!("data: {}\n\n", Value::Object(chunk)))
}

/// 把一帧改写成「只有这段正文」的中间帧：`content` 换成给定文本，`finish_reason` **强制为 null**。
///
/// ★ 为什么必须清 `finish_reason`：扣帧期可能是「散文 + 标记 + 收尾」同帧，散文先发出时
/// 若把上游的 `finish_reason` 一起带走，收尾就**排在合成调用之前** —— Anthropic 侧靠
/// 「见到 finish_reason 就补 usage」收尾，顺序一乱 usage 永远补不出来（同 #46-A 的约束）。
/// 真正的收尾由扣住的那一帧负责（它保留上游原值）。
fn head_content_frame(frame: &Bytes, content: &str) -> Bytes {
    let rewritten = replace_content(frame, content);
    let Some(text) = rewritten
        .strip_prefix(b"data: ")
        .and_then(|rest| std::str::from_utf8(rest).ok())
    else {
        return rewritten;
    };
    let Ok(Value::Object(mut chunk)) = serde_json::from_str::<Value>(text.trim()) else {
        return rewritten;
    };
    if let Some(choices) = chunk.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices.iter_mut() {
            if let Some(object) = choice.as_object_mut() {
                object.insert("finish_reason".into(), Value::Null);
            }
        }
    }
    Bytes::from(format!("data: {}\n\n", Value::Object(chunk)))
}

/// 去掉 `content` 后 `delta` 是否已空（空帧不必发）。
fn delta_is_empty(frame: &Bytes) -> bool {
    let Some(chunk) = parse_frame(frame) else {
        return false;
    };
    chunk
        .get("choices")
        .and_then(Value::as_array)
        .map(|choices| {
            choices.iter().all(|choice| {
                choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .map(|delta| delta.is_empty())
                    .unwrap_or(true)
            })
        })
        .unwrap_or(false)
}

/// 流式 **DSML 调用块守卫**：模型把工具调用写成 `<｜｜DSML｜｜ calls>` 标签时，
/// 把整块扣住、还原成结构化 `tool_calls`，并把标签从正文里剥掉。
///
/// ## 为什么必须有这一层（issue #66）
///
/// 客户端（CodeBuddy / WorkBuddy 一系）在自己的 system 里教模型用 DSML 语法发起
/// 工具调用；Qoder 上游跑在**文本模式**时模型会照学，把调用写成标签塞进正文。
/// 网关若原样透传：① 客户端把标签当正文渲染出来（用户看到的正是
/// `[assistant 请求调用工具] … </｜｜DSML｜｜ parameter>`）；② 真正的工具一次也没
/// 执行 ⇒ 任务在该调工具的位置反复断掉。
///
/// ## 边界（与 [`LeakGuardStream`] 同一套纪律）
///
/// - **正常流逐字不变**：只有「正文里出现 `<` 且可能是 DSML 标签头」才扣帧；
///   一帧内标签之前的散文**立即发出**，不拖首字延迟；
/// - 证伪（`<div>` 之类）立刻放行；超 [`DSML_GUARD_MAX_BUFFER`] 放行；
/// - 上游已给结构化 `tool_calls` 时**不再合成第二组**，但仍剥掉标签文本；
/// - 流末仍是半截 DSML（含 `DSML` 字样）⇒ **整段吞掉**，绝不把内部标签交给用户；
///   其余情况 fail-open 放行。
pub struct DsmlGuardStream<S> {
    inner: S,
    /// 本次请求**声明了 tools** 才启用。没声明工具的请求里出现 DSML 只可能是模型幻觉，
    /// 此时合成 `tool_calls` 是错的（客户端根本没法执行）⇒ 原样透传。
    enabled: bool,
    buffering: bool,
    buffer: String,
    /// 被扣住的收尾帧（`finish_reason` 必须在合成调用**之后**发，否则 Anthropic 侧补不出 usage）。
    pending_finish: Option<Bytes>,
    saw_tool_calls: bool,
    /// 本次是否**合成过**结构化调用（决定收尾要不要改写成 `tool_calls`）。
    emitted_calls: bool,
    /// 上游是否发过任何 `finish_reason`（没有 ⇒ 我们自己补一帧收尾）。
    saw_finish: bool,
    meta: Map<String, Value>,
    index_base: i64,
    out: std::collections::VecDeque<Bytes>,
    finished: bool,
}

impl<S> DsmlGuardStream<S> {
    /// `allowed_tools` 与 [`LeakGuardStream`] 同一来源（本次请求声明的工具名集合）。
    /// 为空 / `None` ⇒ 本层**纯透传**（不来声明 tools 的请求里不该出现工具调用）。
    pub fn new(
        inner: S,
        allowed_tools: Option<std::sync::Arc<std::collections::HashSet<String>>>,
    ) -> Self {
        let enabled = allowed_tools
            .as_ref()
            .map(|names| !names.is_empty())
            .unwrap_or(false);
        Self {
            inner,
            enabled,
            buffering: false,
            buffer: String::new(),
            pending_finish: None,
            saw_tool_calls: false,
            emitted_calls: false,
            saw_finish: false,
            meta: Map::new(),
            index_base: 0,
            out: std::collections::VecDeque::new(),
            finished: false,
        }
    }

    /// 记住上游的 `id` / `model` / `created`（合成帧沿用）。
    fn remember(&mut self, chunk: &Value) {
        for key in ["id", "model", "created"] {
            if let Some(value) = chunk.get(key) {
                self.meta.insert(key.to_string(), value.clone());
            }
        }
    }

    /// 合成一帧（沿用上游 meta）。
    fn frame(&self, delta: Value, finish: Option<&str>) -> Bytes {
        let mut chunk = self.meta.clone();
        chunk.insert(
            "choices".into(),
            json!([{ "index": 0, "delta": delta, "finish_reason": finish }]),
        );
        Bytes::from(format!("data: {}\n\n", Value::Object(chunk)))
    }

    /// 把合成的 `tool_calls` 帧塞进出队序列。
    fn push_calls(&mut self, calls: Vec<Value>) {
        let base = self.index_base;
        let tool_calls: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(offset, call)| {
                let index = base + offset as i64;
                let mut entry = Map::new();
                entry.insert("index".into(), Value::from(index));
                entry.insert(
                    "id".into(),
                    call.get("id").cloned().unwrap_or_else(|| {
                        Value::String(format!("call_{}", uuid::Uuid::new_v4().simple()))
                    }),
                );
                entry.insert(
                    "type".into(),
                    call.get("type")
                        .cloned()
                        .unwrap_or_else(|| Value::String("function".into())),
                );
                entry.insert(
                    "function".into(),
                    call.get("function").cloned().unwrap_or_else(|| json!({})),
                );
                Value::Object(entry)
            })
            .collect();
        self.index_base = base + tool_calls.len() as i64;
        self.emitted_calls = true;
        self.out
            .push_back(self.frame(json!({ "tool_calls": tool_calls }), None));
    }

    /// 把缓冲当正文发出（fail-open 的唯一出口）。
    fn flush_buffer(&mut self) {
        if !self.buffer.is_empty() {
            let text = std::mem::take(&mut self.buffer);
            self.out
                .push_back(self.frame(json!({ "content": text }), None));
        }
        self.buffer.clear();
        self.buffering = false;
    }

    /// 结算缓冲：能还原就还原，能证伪就放行，否则继续等（跨帧）。
    // ⚠️ `allow(clippy::never_loop)` 是**刻意**的：这个 `loop` 的每条分支都 `break`，
    // 它确实不循环（clippy::never_loop 说得对）。
    // 但**不能**把它改写成「顺序流程 + 提前 return」——
    // 原 `loop` 里 `if !self.buffering { break; }` 会跳出后**继续**走函数末尾的
    // `if !self.buffering { self.emit_pending_finish(); }`（收尾帧由此发出）；
    // 改成 return 就会**漏掉收尾帧** ⇒ 客户端收不到 `finish_reason`。
    // ⇒ 保留 `loop` + 显式 allow：语义零变化，且意图写在代码里。
    #[allow(clippy::never_loop)]
    fn settle(&mut self) {
        loop {
            if !self.buffering {
                break;
            }
            if self.buffer.is_empty() {
                self.buffering = false;
                break;
            }
            let (recovered, cleaned) = api::parse_dsml_tool_calls(&self.buffer);
            if let Some(calls) = recovered.filter(|calls| !calls.is_empty()) {
                if !self.saw_tool_calls {
                    self.push_calls(calls);
                }
                if !cleaned.is_empty() {
                    self.out
                        .push_back(self.frame(json!({ "content": cleaned }), None));
                }
                self.buffer.clear();
                self.buffering = false;
                break;
            }
            if !still_dsml_candidate(&self.buffer) || self.buffer.len() > DSML_GUARD_MAX_BUFFER {
                self.flush_buffer();
                break;
            }
            // 仍可能是半截 DSML：等下一帧。
            break;
        }
        if !self.buffering {
            self.emit_pending_finish();
        }
    }

    /// 发出被扣住的收尾帧。
    ///
    /// ★ 合成过调用时必须把 `finish_reason` 改写成 `tool_calls`（与 [`LeakGuardStream`]
    /// 同一口径）：否则客户端收到「有 tool_calls 但 finish_reason=stop」，部分客户端
    /// 会把它当普通回答、**不执行工具** —— 那正是 issue #66 的表症。
    fn emit_pending_finish(&mut self) {
        let Some(frame) = self.pending_finish.take() else {
            return;
        };
        if !self.emitted_calls {
            self.out.push_back(frame);
            return;
        }
        let mut saw = false;
        self.out
            .push_back(strip_content(&frame, Some("tool_calls"), &mut saw));
    }

    /// 处理一帧。
    fn push_frame(&mut self, frame: Bytes) {
        if !self.enabled {
            self.out.push_back(frame);
            return;
        }
        let Some(chunk) = parse_frame(&frame) else {
            if self.buffering {
                // 不可解析的帧（错误帧 / 空行）：先结算，再原样放行。
                self.finish_buffer();
            }
            self.out.push_back(frame);
            return;
        };
        self.remember(&chunk);
        if has_structured_tool_calls(&chunk) {
            self.saw_tool_calls = true;
        }
        // ⚠️ 空字符串也算「无正文」：上游常在收尾帧里带 `"content":""`，若当成正文就会
        // 走「正常帧」分支、绕过收尾改写（实测：合成调用后 finish 仍是 `stop`）。
        let content = delta_content(&chunk).filter(|text| !text.is_empty());
        let has_finish = chunk_finish_reason(&chunk).is_some();
        if has_finish {
            self.saw_finish = true;
        }

        if self.buffering {
            if let Some(text) = &content {
                self.buffer.push_str(text);
            }
            if has_finish {
                let mut saw = false;
                self.pending_finish = Some(strip_content(&frame, None, &mut saw));
            } else if content.is_some() {
                // 非正文部分（role / reasoning / 结构化调用）不能丢。
                let mut saw = false;
                let stripped = strip_content(&frame, None, &mut saw);
                if !delta_is_empty(&stripped) {
                    self.out.push_back(stripped);
                }
            } else {
                self.out.push_back(frame);
            }
            self.settle();
            return;
        }

        let Some(text) = content else {
            // 收尾帧：合成过调用 ⇒ 必须改写成 `tool_calls`（否则客户端当普通回答、不执行工具）。
            if has_finish && self.emitted_calls {
                let mut saw = false;
                self.out
                    .push_back(strip_content(&frame, Some("tool_calls"), &mut saw));
            } else {
                self.out.push_back(frame);
            }
            return;
        };
        match dsml_candidate_start(&text) {
            None => self.out.push_back(frame),
            Some(index) => {
                let head = &text[..index];
                if !head.is_empty() {
                    self.out.push_back(head_content_frame(&frame, head));
                }
                if has_finish {
                    // 收尾帧扣住：`finish_reason` 必须排在合成调用之后。
                    let mut saw = false;
                    self.pending_finish = Some(strip_content(&frame, None, &mut saw));
                }
                self.buffer.push_str(&text[index..]);
                self.buffering = true;
                self.settle();
            }
        }
    }

    /// 流末收尾：半截 DSML 吞掉（内部标签绝不交给用户），其余放行。
    fn finish_buffer(&mut self) {
        if self.buffering {
            // 最后一段里可能刚好凑齐了块（`</…calls>` 落在末帧）。
            self.settle();
        }
        if self.buffering {
            if self.buffer.to_ascii_uppercase().contains("DSML") {
                // 截断的 DSML 块：还原不了 ⇒ 绝不透传，并且**判失败**（客户端重试），
                // 而不是给一个「空回答 + 正常结束」让任务停在半路。
                self.buffer.clear();
                self.buffering = false;
                self.out.push_back(truncated_tool_call_frame());
            } else {
                self.flush_buffer();
            }
        }
        self.emit_pending_finish();
        // 上游没给任何 `finish_reason`（罕见）但我们已经合成了调用 ⇒ 补一帧收尾，
        // 否则客户端会一直等收尾（Anthropic 侧还会补不出 usage）。
        if self.emitted_calls && !self.saw_finish {
            self.out
                .push_back(self.frame(json!({}), Some("tool_calls")));
        }
    }
}

impl<S, E> Stream for DsmlGuardStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(frame) = this.out.pop_front() {
                return Poll::Ready(Some(Ok(frame)));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => this.push_frame(frame),
                Poll::Ready(Some(Err(_error))) => {
                    // 传输错误：扣着的正文放行（不悬挂、不改写事实）。
                    this.finish_buffer();
                    this.finished = true;
                }
                Poll::Ready(None) => {
                    this.finish_buffer();
                    this.finished = true;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// mpsc 接收端 → 流。
pub fn stream_from_receiver(
    rx: tokio::sync::mpsc::Receiver<Result<Bytes, io::Error>>,
) -> impl Stream<Item = Result<Bytes, io::Error>> {
    futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}

/// SSE 响应（`text/event-stream` + 无缓存头；客户端断开即 drop）。
pub fn response_from_receiver(
    rx: tokio::sync::mpsc::Receiver<Result<Bytes, io::Error>>,
) -> axum::response::Response {
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from_stream(stream_from_receiver(rx)))
        .unwrap_or_else(|_| axum::response::Response::new(axum::body::Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream_from(frames: &[String]) -> impl Stream<Item = Result<Bytes, io::Error>> {
        let items: Vec<Result<Bytes, io::Error>> = frames
            .iter()
            .map(|frame| Ok(Bytes::from(frame.clone())))
            .collect();
        futures_util::stream::iter(items)
    }

    /// 用 serde 构造信封帧（body 是内层 JSON 的**字符串**）—— 免去手写转义。
    fn frame(body: &Value) -> String {
        format!(
            "data:{}\n",
            json!({ "body": body.to_string(), "statusCodeValue": 200 })
        )
    }

    async fn collect(
        stream: &mut (impl Stream<Item = Result<Bytes, io::Error>> + Unpin),
    ) -> String {
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            out.push_str(&String::from_utf8_lossy(&item.unwrap()));
        }
        out
    }

    /// 信封 → 下游帧：chunk 透传（已清洗）、[DONE] 兜底、错误帧带信息。
    ///
    /// 变异：删掉 [DONE] 兜底 ⇒ 第二条断言红（客户端会悬挂）。
    #[tokio::test]
    async fn stream_relays_frames_and_always_sends_done() {
        // ① 正常流：chunk + [DONE]。
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": "你好"}}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = QoderOpenAiStream::new(upstream_from(&frames));
        let text = collect(&mut stream).await;
        assert!(text.contains("\"content\":\"你好\""), "{text}");
        assert!(text.trim_end().ends_with("data: [DONE]"), "{text}");

        // ② 上游**没发** [DONE] 就断流 ⇒ 仍然补上。
        let frames = vec![frame(&json!({"choices": [{"delta": {"content": "x"}}]}))];
        let mut stream = QoderOpenAiStream::new(upstream_from(&frames));
        let text = collect(&mut stream).await;
        assert!(
            text.trim_end().ends_with("data: [DONE]"),
            "断流必须补 [DONE]: {text}"
        );

        // ③ 错误帧带状态码与详情，然后 [DONE]。
        let frames = vec!["data:{\"body\":\"boom\",\"statusCodeValue\":429}\n".to_string()];
        let mut stream = QoderOpenAiStream::new(upstream_from(&frames));
        let text = collect(&mut stream).await;
        assert!(text.contains("\"code\":429"), "{text}");
        assert!(text.contains("boom"), "{text}");
        assert!(text.trim_end().ends_with("data: [DONE]"), "{text}");
    }

    /// 聚合：content/reasoning/tool_calls 归并 + usage + finish_reason。
    #[tokio::test]
    async fn aggregate_folds_chunks() {
        let frames = vec![
            frame(&json!({"id": "c1", "created": 123, "choices": [{"delta": {"content": "你"}}]})),
            frame(
                &json!({"id": "c1", "choices": [{"delta": {"content": "好", "reasoning_content": "想"}}]}),
            ),
            frame(
                &json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "sea", "arguments": "{\"q\":\""}}]}}]}),
            ),
            frame(
                &json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "bj\""}}]}, "finish_reason": "tool_calls"}]}),
            ),
            frame(
                &json!({"usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}),
            ),
        ];
        let out = aggregate(upstream_from(&frames), None).await;
        assert_eq!(out.content, "你好");
        assert_eq!(out.reasoning, "想");
        assert_eq!(out.id.as_deref(), Some("c1"));
        assert_eq!(out.created, Some(123));
        assert_eq!(out.tool_calls.len(), 1);
        assert_eq!(out.tool_calls[0]["id"], "call_1");
        // 分片拼接：name 分片 `sea`；arguments `{"q":"` + `bj"`。
        assert_eq!(out.tool_calls[0]["function"]["name"], "sea");
        assert_eq!(out.tool_calls[0]["function"]["arguments"], "{\"q\":\"bj\"");
        assert_eq!(out.finish_reason, "tool_calls");
        assert_eq!(out.usage.unwrap()["total_tokens"], 15);
    }

    /// 泄漏回声：严格形态被还原成结构化调用并清空正文。
    #[tokio::test]
    async fn aggregate_recovers_leaked_tool_calls() {
        let content = "[assistant 请求调用工具]\n[{\"name\":\"search\",\"arguments\":\"{}\"}]";
        let frames = vec![frame(
            &json!({"choices": [{"delta": {"content": content}}]}),
        )];
        let out = aggregate(upstream_from(&frames), None).await;
        assert_eq!(out.content, "", "回声必须从正文清掉");
        assert_eq!(out.tool_calls.len(), 1);
        assert_eq!(out.tool_calls[0]["function"]["name"], "search");
        assert_eq!(out.finish_reason, "tool_calls");
    }

    /// 上游错误帧：聚合结果里带可读错误。
    #[tokio::test]
    async fn aggregate_surfaces_upstream_error() {
        let frames =
            vec!["data:{\"body\":\"quota exceeded\",\"statusCodeValue\":429}\n".to_string()];
        let out = aggregate(upstream_from(&frames), None).await;
        let error = out.upstream_error.expect("必须带上游错误");
        assert!(error.contains("429"), "{error}");
        assert!(error.contains("quota exceeded"), "{error}");
    }

    /// ★ 聚合结果要带**错误码**（`upstream_error_code`）—— 账本靠它把
    /// 「排队（429，稍后重试）」与「上游其它错误（502）」分开（2026-10-22 实测踩到：
    /// 日志把上游排队写成 502，用户以为服务坏了）。
    ///
    /// 可证伪：删掉 `aggregate` 里读 `error.code` 的那几行 ⇒ 断言红。
    #[tokio::test]
    async fn aggregate_carries_the_upstream_error_code() {
        // 排队信号：上游 200 + 信封里 403（`sse::process_line` 对排队发 `code:429`）。
        let queued_body = json!({
            "code": "403",
            "message": "{\"code\":\"10605\",\"isQueued\":true}",
        })
        .to_string();
        let frames = vec![format!(
            "data:{{\"body\":{},\"statusCodeValue\":403}}\n",
            json!(queued_body)
        )];
        let out = aggregate(upstream_from(&frames), None).await;
        assert!(out.upstream_error.is_some(), "必须带错误信息");
        assert_eq!(
            out.upstream_error_code,
            Some(429),
            "排队 ⇒ 客户端拿到 429 ⇒ 账本也必须记 429"
        );
    }

    /// ★ 探测用：读一次流取**第一帧上游错误**。
    ///
    /// 覆盖三种形态：
    /// - 信封错误（`statusCodeValue != 200`）⇒ 取出 `(状态码, 详情)`；
    /// - 正常 chunk ⇒ `None`（**不能**把正文当错误）；
    /// - 排队信号（`10605` / `isQueued`）⇒ 详情里带 `10605`（供 `queue_signal` 判定）。
    ///
    /// 这条是 2026-10-22 那个假阴性的钉子：探测只看 HTTP 状态 ⇒ 排队被报成「可用」。
    #[tokio::test]
    async fn first_upstream_error_finds_the_first_error_frame() {
        // ① 信封错误：状态码与详情都要拿到。
        let frames =
            vec!["data:{\"body\":\"quota exceeded\",\"statusCodeValue\":429}\n".to_string()];
        let found = first_upstream_error(upstream_from(&frames)).await;
        let (status, detail) = found.expect("必须找到错误帧");
        assert_eq!(status, 429);
        assert!(detail.contains("quota exceeded"), "{detail}");

        // ② 正常流 ⇒ 无错误（**不能**把正文当错误报上去）。
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": "好"}}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        assert!(
            first_upstream_error(upstream_from(&frames)).await.is_none(),
            "正常流不得被报成错误"
        );

        // ③ 排队信号：详情里带 `10605` / `isQueued`（`pool::queue_signal` 据此判定）。
        let queued_body = json!({"code": "10605", "isQueued": true}).to_string();
        let frames = vec![format!(
            "data:{{\"body\":{},\"statusCodeValue\":403}}\n",
            json!(queued_body)
        )];
        let (status, detail) = first_upstream_error(upstream_from(&frames))
            .await
            .expect("排队也是一帧错误，必须被抓到");
        assert_eq!(status, 403, "信封里的状态码");
        assert!(
            detail.contains("10605") && detail.contains("isQueued"),
            "排队细节必须完整：{detail}"
        );
    }

    /// 错误帧必须被**旁路留痕**：流式账本在流结束才落，没有这一槽就只能记
    /// 「200 + 0 token + error:null」——正是 issue #46 里隐身的那条日志。
    ///
    /// 可证伪：去掉 `tap_frame` 里的 error 分支 ⇒ 第一条断言红。
    #[tokio::test]
    async fn usage_tap_records_error_frames() {
        use std::sync::{Arc, Mutex};

        let frames = vec!["data:{\"body\":\"boom\",\"statusCodeValue\":429}\n".to_string()];
        let usage = Arc::new(Mutex::new(None::<Value>));
        let error = Arc::new(Mutex::new(None::<UpstreamError>));
        let mut tap = UsageTapStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            usage.clone(),
            error.clone(),
        );
        let text = collect(&mut tap).await;
        assert!(
            text.contains("\"code\":429"),
            "错误帧仍要透传给客户端: {text}"
        );
        let captured = error
            .lock()
            .expect("锁未中毒")
            .clone()
            .expect("错误必须留痕");
        // ★ 结构化错误（带 `code`）—— 比原字符串 `contains("429")` 精确。
        assert_eq!(captured.code, 429, "错误码必须留痕：{captured:?}");
        assert!(
            usage.lock().expect("锁未中毒").is_none(),
            "错误帧不带 usage"
        );

        // 正常流：usage 被取到，错误槽保持空（别把正常请求记成失败）。
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": "好"}}]})),
            frame(
                &json!({"usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}}),
            ),
        ];
        let usage = Arc::new(Mutex::new(None::<Value>));
        let error = Arc::new(Mutex::new(None::<UpstreamError>));
        let mut tap = UsageTapStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            usage.clone(),
            error.clone(),
        );
        let _ = collect(&mut tap).await;
        assert_eq!(
            usage.lock().expect("锁未中毒").as_ref().unwrap()["total_tokens"],
            7
        );
        assert!(error.lock().expect("锁未中毒").is_none(), "正常流不得记错");
    }

    /// P0：**普通流经守卫后必须字节级不变**（issue #46 的 A 项的回归钉子）。
    ///
    /// 可证伪：把守卫改成「无条件扣帧到流末」⇒ 第一条 `==` 失败。
    #[tokio::test]
    async fn leak_guard_is_transparent_for_normal_streams() {
        use std::sync::Arc;

        let frames = vec![
            frame(&json!({"id": "c1", "model": "m", "choices": [{"delta": {"content": "你好"}}]})),
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": "，世界"}}]})),
            frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 5, "completion_tokens": 6, "total_tokens": 11}}),
            ),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut guarded = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(Arc::new(std::collections::HashSet::from([
                "search".to_string()
            ]))),
        );
        let guarded_text = collect(&mut guarded).await;
        let mut bare = QoderOpenAiStream::new(upstream_from(&frames));
        let bare_text = collect(&mut bare).await;
        assert_eq!(guarded_text, bare_text, "正常流必须逐字不变");
        assert!(guarded_text.contains("你好"), "{guarded_text}");
        assert!(
            guarded_text.trim_end().ends_with("data: [DONE]"),
            "{guarded_text}"
        );
    }

    /// 合法回声 ⇒ 还原成结构化 `tool_calls` + `finish_reason:"tool_calls"`，
    /// 且**整条流只出现一组** `tool_calls`（双发会被客户端执行两遍）。
    #[tokio::test]
    async fn leak_guard_recovers_echo_into_structured_tool_calls() {
        use std::sync::Arc;

        let echo = format!(
            "{}\n{}",
            api::LEAK_MARKER,
            json!([{ "name": "search", "arguments": "{\"q\":\"北京\"}" }])
        );
        let frames = vec![
            frame(&json!({"id": "c1", "model": "m", "choices": [{"delta": {"content": echo}}]})),
            frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 7, "completion_tokens": 9, "total_tokens": 16}}),
            ),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut guarded = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(Arc::new(std::collections::HashSet::from([
                "search".to_string()
            ]))),
        );
        let text = collect(&mut guarded).await;
        assert!(
            !text.contains(api::LEAK_MARKER),
            "标记不得泄漏到客户端: {text}"
        );
        let chunks: Vec<Value> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter(|data| data.trim() != "[DONE]")
            .map(|data| serde_json::from_str::<Value>(data.trim()).expect("帧是 JSON"))
            .collect();
        let call_frames: Vec<&Value> = chunks
            .iter()
            .filter(|chunk| {
                chunk
                    .get("choices")
                    .and_then(Value::as_array)
                    .map(|choices| {
                        choices.iter().any(|choice| {
                            choice
                                .get("delta")
                                .and_then(|delta| delta.get("tool_calls"))
                                .and_then(Value::as_array)
                                .map(|calls| !calls.is_empty())
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(call_frames.len(), 1, "整条流只允许一组 tool_calls: {text}");
        let calls = call_frames[0]["choices"][0]["delta"]["tool_calls"]
            .as_array()
            .expect("tool_calls 是数组");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0]["index"], 0,
            "并行调用靠 index 归并（Anthropic 侧 unwrap_or(0)）"
        );
        assert_eq!(calls[0]["function"]["name"], "search");
        assert!(
            !calls[0]["id"].as_str().unwrap_or_default().is_empty(),
            "id 不能空"
        );
        assert_eq!(call_frames[0]["id"], "c1", "沿用上游 id");
        // finish_reason 必须改成 tool_calls（否则客户端当普通回答结束）。
        let finish = chunks
            .iter()
            .find_map(|chunk| {
                chunk["choices"][0]["finish_reason"]
                    .as_str()
                    .filter(|reason| !reason.is_empty())
                    .map(str::to_string)
            })
            .expect("必须有 finish_reason");
        assert_eq!(finish, "tool_calls", "{text}");
        assert!(
            text.contains("\"total_tokens\":16"),
            "usage 帧必须还在: {text}"
        );
        assert!(text.trim_end().ends_with("data: [DONE]"), "{text}");
    }

    /// 标记**分片到达**（模型分多次吐标记）⇒ 仍能还原。
    #[tokio::test]
    async fn leak_guard_recovers_marker_arriving_in_pieces() {
        use std::sync::Arc;

        let mut frames = Vec::new();
        for piece in [
            "[assistant ",
            "请求调用",
            "工具]\n",
            "[{\"name\":\"search\",\"arguments\":\"{}\"}]",
        ] {
            frames.push(frame(&json!({"choices": [{"delta": {"content": piece}}]})));
        }
        frames.push(frame(
            &json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
        ));
        frames.push("data:{\"body\":\"[DONE]\"}\n".to_string());
        let mut guarded = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(Arc::new(std::collections::HashSet::from([
                "search".to_string()
            ]))),
        );
        let text = collect(&mut guarded).await;
        assert!(!text.contains(api::LEAK_MARKER), "{text}");
        assert!(text.contains("\"tool_calls\""), "{text}");
        assert!(text.contains("\"finish_reason\":\"tool_calls\""), "{text}");
    }

    /// 散文混排（标记开头但不是合法回声）⇒ **原样透传**，不误杀。
    #[tokio::test]
    async fn leak_guard_does_not_kill_prose_that_starts_with_the_marker() {
        let frames = vec![
            frame(
                &json!({"choices": [{"delta": {"content": "[assistant 请求调用工具] 这个功能怎么用？"}}]}),
            ),
            frame(&json!({"choices": [{"delta": {"finish_reason": "stop"}}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut guarded =
            LeakGuardStream::new(QoderOpenAiStream::new(upstream_from(&frames)), None);
        let text = collect(&mut guarded).await;
        assert!(text.contains("这个功能怎么用"), "散文必须原样送达: {text}");
        assert!(
            !text.contains("\"tool_calls\""),
            "不得凭空造工具调用: {text}"
        );
    }

    /// 未声明的工具名**不还原**（否则会凭空造出客户端没要求的能力）。
    #[tokio::test]
    async fn leak_guard_respects_declared_tool_names() {
        use std::sync::Arc;

        let echo = format!(
            "{}\n{}",
            api::LEAK_MARKER,
            json!([{ "name": "delete_everything", "arguments": "{}" }])
        );
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": echo}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut guarded = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(Arc::new(std::collections::HashSet::from([
                "search".to_string()
            ]))),
        );
        let text = collect(&mut guarded).await;
        assert!(
            text.contains("delete_everything"),
            "未声明工具应原样透传: {text}"
        );
        assert!(
            !text.contains("\"tool_calls\""),
            "不得还原未声明的工具: {text}"
        );
    }

    /// P0-A0：上游**已经**给了结构化 `tool_calls`、正文又带标记 ⇒ 绝不合成第二组。
    ///
    /// 两组 `tool_calls`（各带随机 id）会被客户端**执行两遍**同一个副作用。
    #[tokio::test]
    async fn leak_guard_never_synthesizes_a_second_tool_call_set() {
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": api::LEAK_MARKER}}]})),
            frame(&json!({"choices": [{
                "delta": { "content": "\n[{\"name\":\"search\",\"arguments\":\"{}\"}]",
                            "tool_calls": [{ "index": 0, "id": "upstream-1", "type": "function",
                                              "function": { "name": "search", "arguments": "{}" } }] },
                "finish_reason": "tool_calls",
            }]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut guarded =
            LeakGuardStream::new(QoderOpenAiStream::new(upstream_from(&frames)), None);
        let text = collect(&mut guarded).await;
        let call_count = text.matches("\"tool_calls\":[").count();
        assert_eq!(call_count, 1, "整条流只能有一组 tool_calls: {text}");
        assert!(text.contains("upstream-1"), "必须保留上游那份: {text}");
        /* ⚠️ 这条才是本用例的**主断言**：不合成 ≠ 可以把标记原样还给用户。
        可证伪：把 `flush_held_without_leaking()` 改回 `flush_held()` ⇒ 这里红。
        （第一版实现就是「整帧放行」，症状与 issue #46 一字不差。） */
        assert!(
            !text.contains(api::LEAK_MARKER),
            "上游已给结构化调用时也**必须**剥掉回声正文: {text}"
        );
    }

    /// 上游在标记中间断掉（没有 `[DONE]`）⇒ 扣住的内容**必须释放**，且仍补 `[DONE]`。
    #[tokio::test]
    async fn leak_guard_releases_held_frames_when_upstream_drops() {
        let frames = vec![frame(
            &json!({"choices": [{"delta": {"content": "[assistant 请求"}}]}),
        )];
        let mut guarded =
            LeakGuardStream::new(QoderOpenAiStream::new(upstream_from(&frames)), None);
        let text = collect(&mut guarded).await;
        assert!(
            text.contains("[assistant 请求"),
            "扣住的内容必须释放: {text}"
        );
        assert!(
            text.trim_end().ends_with("data: [DONE]"),
            "仍要补 [DONE]: {text}"
        );
    }

    /// 超过扣帧上限 ⇒ fail-open 立刻透传（不让用户为了一个误判等 64 KiB）。
    #[tokio::test]
    async fn leak_guard_fails_open_when_held_bytes_exceed_the_cap() {
        let huge = "x".repeat(LEAK_GUARD_MAX_HELD_BYTES + 16);
        let frames = vec![frame(&json!({"choices": [{"delta": {"content": huge}}]}))];
        let mut guarded =
            LeakGuardStream::new(QoderOpenAiStream::new(upstream_from(&frames)), None);
        let text = collect(&mut guarded).await;
        assert!(
            text.contains("xxxx"),
            "超上限必须原样透传: {} 字节",
            text.len()
        );
    }

    /// 非流式路径（[`aggregate`]）**只还原一次**：自带结构化调用与自带 id 都不重复。
    #[tokio::test]
    async fn aggregate_recovers_the_echo_exactly_once() {
        let echo = format!(
            "{}\n{}",
            api::LEAK_MARKER,
            json!([{ "name": "search", "arguments": "{}" }])
        );
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": echo}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let out = aggregate(upstream_from(&frames), None).await;
        assert_eq!(out.tool_calls.len(), 1, "{:?}", out.tool_calls);
        let ids: std::collections::HashSet<String> = out
            .tool_calls
            .iter()
            .filter_map(|call| call["id"].as_str().map(str::to_string))
            .collect();
        assert_eq!(ids.len(), out.tool_calls.len(), "工具 id 不得重复: {out:?}");
        assert_eq!(out.content, "", "回声必须从正文清掉");
        assert_eq!(out.finish_reason, "tool_calls");
    }

    /// 流式端到端：合成帧经过**完整生产链**后，客户端只看到一组 `tool_calls`。
    #[tokio::test]
    async fn streaming_chain_yields_exactly_one_tool_call_set() {
        use std::sync::Arc;

        let echo = format!(
            "{}\n{}",
            api::LEAK_MARKER,
            json!([{ "name": "search", "arguments": "{}" }, { "name": "ls", "arguments": "{}" }])
        );
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": echo}}]})),
            frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": {"total_tokens": 5}}),
            ),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let tapped = UsageTapStream::new(
            LeakGuardStream::new(
                QoderOpenAiStream::new(upstream_from(&frames)),
                Some(Arc::new(std::collections::HashSet::from([
                    "search".to_string(),
                    "ls".to_string(),
                ]))),
            ),
            std::sync::Arc::new(std::sync::Mutex::new(None::<Value>)),
            std::sync::Arc::new(std::sync::Mutex::new(None::<UpstreamError>)),
        );
        let mut stream = tapped;
        let text = collect(&mut stream).await;
        assert!(!text.contains(api::LEAK_MARKER), "{text}");
        // 两个工具 ⇒ 两个 index（Anthropic 侧靠它归并）。
        assert!(text.contains("\"index\":0"), "{text}");
        assert!(text.contains("\"index\":1"), "{text}");
        assert_eq!(text.matches("\"tool_calls\":[").count(), 1, "{text}");
    }

    /// issue #66：模型把调用写成 **DSML 标签**（跨多帧到达）⇒ 还原成结构化调用，
    /// 且标签文本一个字符都不许出现在下游。
    #[tokio::test]
    async fn dsml_guard_recovers_streamed_calls_and_hides_tags() {
        let pieces = [
            "先探一下端口。\n<｜｜DSML｜｜ calls>\n<｜｜DSML｜｜ invoke name=\"pwsh\">\n",
            "<｜｜DSML｜｜ parameter name=\"command\">Get-Process",
            "</｜｜DSML｜｜ parameter>\n</｜｜DSML｜｜ invoke>\n",
            "</｜｜DSML｜｜ calls>",
        ];
        let mut frames: Vec<String> = pieces
            .iter()
            .map(|piece| {
                frame(
                    &json!({"id": "c1", "model": "m", "choices": [{"delta": {"content": piece}}]}),
                )
            })
            .collect();
        frames.push(frame(
            // ★ 上游收尾帧常带 `"content":""` —— 必须仍被识别为收尾（否则不改写 finish）。
            &json!({"choices": [{"delta": {"content": ""}, "finish_reason": "stop"}], "usage": {"total_tokens": 9}}),
        ));
        frames.push("data:{\"body\":\"[DONE]\"}\n".to_string());

        let allowed = std::sync::Arc::new(std::collections::HashSet::from(["pwsh".to_string()]));
        let mut stream = DsmlGuardStream::new(
            LeakGuardStream::new(
                QoderOpenAiStream::new(upstream_from(&frames)),
                Some(allowed.clone()),
            ),
            Some(allowed),
        );
        let text = collect(&mut stream).await;
        assert!(
            !text.to_ascii_uppercase().contains("DSML"),
            "标签泄漏: {text}"
        );
        assert!(
            text.contains("\"tool_calls\":["),
            "必须合成结构化调用: {text}"
        );
        assert!(text.contains("\"name\":\"pwsh\""), "{text}");
        assert!(text.contains("Get-Process"), "{text}");
        assert!(
            text.contains("先探一下端口。"),
            "标签前的散文必须保留: {text}"
        );
        // `finish_reason` 必须排在合成调用**之后**（Anthropic 侧靠它补 usage），
        // 且必须改写成 `tool_calls`（写成 `stop` 会让部分客户端当普通回答、不执行工具）。
        let calls_at = text.find("\"tool_calls\":[").expect("有调用");
        let finish_at = text
            .find("\"finish_reason\":\"tool_calls\"")
            .unwrap_or_else(|| panic!("收尾必须是 tool_calls: {text}"));
        assert!(calls_at < finish_at, "收尾帧顺序错了: {text}");
        assert!(
            !text.contains("\"finish_reason\":\"stop\""),
            "上游的 stop 必须被改写: {text}"
        );
    }

    /// 上游没给任何 `finish_reason` 时，合成调用后必须**补一帧收尾**（否则客户端悬挂）。
    #[tokio::test]
    async fn dsml_guard_synthesizes_finish_when_upstream_omits_it() {
        let frames = vec![
            frame(
                &json!({"id": "c1", "choices": [{"delta": {"content": "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"ls\"><｜｜DSML｜｜ parameter name=\"path\">/tmp</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke></｜｜DSML｜｜ calls>"}}]}),
            ),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let allowed = std::sync::Arc::new(std::collections::HashSet::from(["ls".to_string()]));
        let mut stream = DsmlGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(allowed),
        );
        let text = collect(&mut stream).await;
        assert!(text.contains("\"tool_calls\":["), "{text}");
        assert!(text.contains("\"finish_reason\":\"tool_calls\""), "{text}");
    }

    /// P0：普通流（含数学 `<` 与 HTML 标签）经 DSML 守卫后**逐字不变**。
    #[tokio::test]
    async fn dsml_guard_is_transparent_for_ordinary_streams() {
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": "1 < 2 且"}}]})),
            frame(&json!({"choices": [{"delta": {"content": " <div>是标签"}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let allowed = std::sync::Arc::new(std::collections::HashSet::from(["shell".to_string()]));
        let mut guarded = DsmlGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(allowed),
        );
        let guarded_text = collect(&mut guarded).await;
        let mut bare = QoderOpenAiStream::new(upstream_from(&frames));
        let bare_text = collect(&mut bare).await;
        assert_eq!(guarded_text, bare_text, "普通流必须逐字不变");
    }

    /// 没声明 tools 的请求：DSML 标签**原样透传**（合成一个客户端执行不了的调用更糟）。
    #[tokio::test]
    async fn dsml_guard_is_passthrough_when_no_tools_were_declared() {
        let frames = vec![
            frame(
                &json!({"choices": [{"delta": {"content": "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"x\"></｜｜DSML｜｜ invoke></｜｜DSML｜｜ calls>"}}]}),
            ),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut guarded =
            DsmlGuardStream::new(QoderOpenAiStream::new(upstream_from(&frames)), None);
        let guarded_text = collect(&mut guarded).await;
        let mut bare = QoderOpenAiStream::new(upstream_from(&frames));
        let bare_text = collect(&mut bare).await;
        assert_eq!(guarded_text, bare_text, "未声明 tools 时不得改写流");
        assert!(guarded_text.contains("DSML"), "{guarded_text}");
    }

    /// 截断的 DSML 块：还原不了，也绝不把内部标签交给用户。
    #[tokio::test]
    async fn dsml_guard_drops_truncated_block() {
        let frames = vec![
            frame(
                &json!({"choices": [{"delta": {"content": "<｜｜DSML｜｜ calls>\n<｜｜DSML｜｜ invoke name=\"pw"}}]}),
            ),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "length"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let allowed = std::sync::Arc::new(std::collections::HashSet::from(["shell".to_string()]));
        let mut stream = DsmlGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(allowed),
        );
        let text = collect(&mut stream).await;
        assert!(
            !text.to_ascii_uppercase().contains("DSML"),
            "截断块泄漏: {text}"
        );
        // 截断的 DSML 同样**判失败**（客户端重试），不静默结束。
        assert!(text.contains("\"code\":502"), "必须发流内错误: {text}");
    }

    /// issue #66：**截断的调用回声**不许透传（既还原不了，也不能把标记交给用户）。
    #[tokio::test]
    async fn leak_guard_drops_truncated_echo_instead_of_leaking_marker() {
        let truncated = format!(
            "{}\n[{{\"name\":\"pwsh\",\"arguments\":\"{{\\\"comm",
            api::LEAK_MARKER
        );
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": truncated}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "length"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(std::sync::Arc::new(std::collections::HashSet::from([
                "pwsh".to_string(),
            ]))),
        );
        let text = collect(&mut stream).await;
        assert!(!text.contains(api::LEAK_MARKER), "标记泄漏: {text}");
        // 截断回声还原不了 ⇒ **判本轮失败**（客户端重试），而不是「空回答 + 正常结束」。
        assert!(text.contains("\"code\":502"), "必须发流内错误: {text}");
        assert!(
            text.contains(TRUNCATED_TOOL_CALL_ERROR),
            "错误文案要可读: {text}"
        );
    }

    /// issue #66：`[工具结果…]` 回声块同样吞掉（不还原，只保证用户看不到）。
    #[tokio::test]
    async fn leak_guard_drops_tool_result_echo() {
        let echo = format!(
            "{} (pwsh)]\n{{\"ok\":true}}\n[工具结果结束]",
            api::TOOL_RESULT_MARKER
        );
        let frames = vec![
            frame(&json!({"choices": [{"delta": {"content": echo}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(std::sync::Arc::new(std::collections::HashSet::from([
                "pwsh".to_string(),
            ]))),
        );
        let text = collect(&mut stream).await;
        assert!(
            !text.contains(api::TOOL_RESULT_MARKER),
            "工具结果回声泄漏: {text}"
        );
    }

    /// 非流式：DSML 块折叠成 `tool_calls`，正文剥掉调用块。
    #[tokio::test]
    async fn aggregate_recovers_dsml_calls() {
        let dsml = "说明。\n<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"ls\">\
                    <｜｜DSML｜｜ parameter name=\"path\">/tmp</｜｜DSML｜｜ parameter>\
                    </｜｜DSML｜｜ invoke></｜｜DSML｜｜ calls>";
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": dsml}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let allowed: std::collections::HashSet<String> = ["ls".to_string()].into_iter().collect();
        let out = aggregate(upstream_from(&frames), Some(&allowed)).await;
        assert_eq!(out.tool_calls.len(), 1, "{out:?}");
        assert_eq!(out.tool_calls[0]["function"]["name"], "ls");
        assert_eq!(out.content, "说明。");
        assert_eq!(out.finish_reason, "tool_calls");

        // 未声明 tools ⇒ 不合成（客户端没有可执行的工具，合成只会换来一次必然报错）。
        let out = aggregate(upstream_from(&frames), None).await;
        assert!(out.tool_calls.is_empty(), "{out:?}");
        assert!(out.content.contains("DSML"), "{out:?}");
    }

    /// 尖括号标记（`<assistant 请求调用工具>`）也要能还原 —— 换个括号不能绕过治理。
    #[tokio::test]
    async fn leak_guard_recovers_angle_bracket_marker() {
        let echo = format!(
            "{}\n{}",
            api::LEAK_MARKER_ALT,
            json!([{ "name": "ls", "arguments": "{}" }])
        );
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": echo}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(std::sync::Arc::new(std::collections::HashSet::from([
                "ls".to_string()
            ]))),
        );
        let text = collect(&mut stream).await;
        assert!(!text.contains(api::LEAK_MARKER_ALT), "标记泄漏: {text}");
        assert!(text.contains("\"tool_calls\":["), "必须还原: {text}");
    }

    /// 信封层排队（403 + `10605`/`isQueued`）⇒ 错误帧是 **可重试的 429**。
    ///
    /// ★ issue #129：对外形状从自造的 `upstream_queued` 收敛成标准 `rate_limit_error`
    /// + `HTTP 429` 文案 —— 客户端（`openai-completions`）**只做文本正则**，
    /// 自造 `type` 它不认识，`403` 还会被归成 `AUTH`（终态 + 提示「密钥无效」）。
    /// 真实原因（排队/上游原文）留在 `upstream` 字段里给日志用。
    #[tokio::test]
    async fn queued_envelope_is_reported_as_retryable_rate_limit() {
        let body = "{\"code\":\"10605\",\"isQueued\":true,\"retryAfterSeconds\":30}";
        let frame_line = format!("data:{}\n", json!({ "body": body, "statusCodeValue": 403 }));
        let frames = vec![frame_line];
        let mut stream = QoderOpenAiStream::new(upstream_from(&frames));
        let text = collect(&mut stream).await;
        assert!(text.contains("rate_limit_error"), "{text}");
        assert!(text.contains("\"code\":429"), "{text}");
        assert!(text.contains("HTTP 429"), "文案必须带等价状态码: {text}");
        // 上游原文（含排队信号）仍要留痕，否则日志里看不到真实原因。
        assert!(text.contains("10605"), "{text}");
        // 真·鉴权失败不能被误判成排队（`token_invalid` 优先）—— 但对外同样是 429。
        let auth = format!(
            "data:{}\n",
            json!({ "body": "{\"code\":\"TOKEN_INVALID\",\"isQueued\":true}", "statusCodeValue": 401 })
        );
        let mut stream = QoderOpenAiStream::new(upstream_from(&[auth]));
        let text = collect(&mut stream).await;
        assert!(text.contains("rate_limit_error"), "{text}");
        assert!(text.contains("HTTP 429"), "{text}");
        assert!(!text.contains("排队"), "鉴权失败不得说成排队: {text}");
    }

    /// 非流式：截断回声 ⇒ `upstream_error`（调用方据此回 502），不是「200 + 空正文」。
    #[tokio::test]
    async fn aggregate_fails_turn_on_truncated_echo() {
        let truncated = format!(
            "{}\n[{{\"name\":\"shell\",\"arguments\":\"{{\\\"comm",
            api::LEAK_MARKER
        );
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": truncated}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "length"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let allowed: std::collections::HashSet<String> =
            ["shell".to_string()].into_iter().collect();
        let out = aggregate(upstream_from(&frames), Some(&allowed)).await;
        assert!(out.tool_calls.is_empty(), "{out:?}");
        assert!(out.content.is_empty(), "不得把半截 JSON 当正文: {out:?}");
        assert_eq!(
            out.upstream_error.as_deref(),
            Some(TRUNCATED_TOOL_CALL_ERROR),
            "必须判失败"
        );
    }

    /// 「散文 + 标记 + 收尾」同帧：收尾必须**排在合成调用之后**（否则 Anthropic 侧补不出 usage）。
    #[tokio::test]
    async fn head_frame_does_not_carry_the_upstream_finish_reason() {
        let echo = format!(
            "先看看。\n{}\n{}",
            api::LEAK_MARKER,
            json!([{ "name": "ls", "arguments": "{}" }])
        );
        let frames = vec![
            frame(
                &json!({"id": "c1", "choices": [{"delta": {"content": echo}, "finish_reason": "stop"}]}),
            ),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(std::sync::Arc::new(std::collections::HashSet::from([
                "ls".to_string()
            ]))),
        );
        let text = collect(&mut stream).await;
        let calls_at = text.find("\"tool_calls\":[").expect("有调用");
        let finish_at = text
            .find("\"finish_reason\":\"tool_calls\"")
            .expect("有收尾");
        assert!(calls_at < finish_at, "收尾必须排在调用之后: {text}");
        assert_eq!(
            text.matches("\"finish_reason\":\"").count(),
            1,
            "只允许一帧收尾: {text}"
        );
        assert!(text.contains("先看看。"), "{text}");
    }

    /// issue #66：**散文 + 标记落在同一帧**时，散文必须立刻发出、标记进扣帧流程。
    #[tokio::test]
    async fn leak_guard_splits_prose_from_marker_in_one_frame() {
        let echo = format!(
            "先看看目录。\n{}\n{}",
            api::LEAK_MARKER,
            json!([{ "name": "ls", "arguments": "{}" }])
        );
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": echo}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(std::sync::Arc::new(std::collections::HashSet::from([
                "ls".to_string()
            ]))),
        );
        let text = collect(&mut stream).await;
        assert!(!text.contains(api::LEAK_MARKER), "标记泄漏: {text}");
        assert!(text.contains("先看看目录。"), "散文必须保留: {text}");
        assert!(
            text.contains("\"tool_calls\":["),
            "必须还原成结构化调用: {text}"
        );
    }

    /// issue #66 真机形态：**数组后面跟 DSML 闭标签残片**（模型把两种格式混着复述）。
    /// 严格整段识别失败 ⇒ 收尾容错必须救回来，否则用户看到一坨标记、工具一次都没执行。
    #[tokio::test]
    async fn leak_guard_recovers_echo_with_trailing_dsml_fragments() {
        let echo = format!(
            "{}\n{}\n</｜｜DSML｜｜ parameter> </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls>",
            api::LEAK_MARKER,
            json!([{ "name": "pwsh", "arguments": json!({"command": "Get-Process"}).to_string() }])
        );
        let frames = vec![
            frame(&json!({"id": "c1", "choices": [{"delta": {"content": echo}}]})),
            frame(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})),
            "data:{\"body\":\"[DONE]\"}\n".to_string(),
        ];
        let mut stream = LeakGuardStream::new(
            QoderOpenAiStream::new(upstream_from(&frames)),
            Some(std::sync::Arc::new(std::collections::HashSet::from([
                "pwsh".to_string(),
            ]))),
        );
        let text = collect(&mut stream).await;
        assert!(!text.contains(api::LEAK_MARKER), "标记泄漏: {text}");
        assert!(
            !text.to_ascii_uppercase().contains("DSML"),
            "残片泄漏: {text}"
        );
        assert!(text.contains("\"name\":\"pwsh\""), "{text}");
        assert!(text.contains("Get-Process"), "{text}");
    }
}
