//! OpenAI **Responses API**（`POST /v1/responses`）协议转换。
//!
//! 与 `/v1/chat/completions`、`/v1/messages` **共用同一条中继**（选号 / 轮换 / 冷却），
//! 差别只在协议层：
//! - 入站：Responses 请求 → 上游 `chat.completions` 请求（[`to_upstream_request`]）；
//! - 出站（非流式）：上游 SSE 聚合 → Responses 对象（[`to_response_object`]）；
//! - 出站（流式）：上游 SSE → Responses 事件流（[`ResponsesSseStream`]）。
//!
//! # 支持的子集（边界写清，避免「看起来支持、实际静默降级」）
//!
//! | 字段 | 处理 |
//! |---|---|
//! | `input`（string） | → 单条 `user` 消息 |
//! | `input`（items 数组） | `message` / `function_call` / `function_call_output` 逐条翻译；`reasoning` 丢弃（上游不接受） |
//! | `instructions` | → `system` 消息 |
//! | `tools`（扁平 `{type:"function",name,description,parameters}`） | → chat `{type:"function",function:{…}}`；**内置工具**（`web_search` 等）跳过 |
//! | `tool_choice` | 字符串原样；`{type:"function",name}` → 具名；`none` 原样 |
//! | `temperature` / `top_p` / `max_output_tokens` / `parallel_tool_calls` | 直通（`max_output_tokens` → `max_tokens`） |
//!
//! **不支持**（忽略而非报错，客户端可自行降级）：`previous_response_id`（本网关无服务端
//! 会话状态）、`store`、`include`、`background`、`stream_options`、文件/音频输入。
//!
//! # 上游恒为流式
//!
//! `outbound::prepare_outbound_body` 会**强制** `stream: true` 并注入
//! `stream_options.include_usage`，因此本模块的出站路径统一按 SSE 处理：
//! 客户端要流式就转事件流，不要流式就聚合成对象。

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Bytes;
use futures_util::Stream;
use serde_json::{json, Map, Value};

use crate::protocol::openai::{sse_event, CompletionAccumulator, ToolCallAccum};

/// 单行缓冲区上限：畸形流（永不出现 `\n`）不得让内存无界增长。超限时原样冲刷保底。
const MAX_LINE_BUFFER_BYTES: usize = 256 * 1024;

/// 把 Responses 请求体翻译成上游 `chat.completions` 请求体。
pub fn to_upstream_request(body: &Value) -> Value {
    let mut out = Map::new();

    if let Some(model) = body.get("model") {
        out.insert("model".to_string(), model.clone());
    }
    for key in [
        "temperature",
        "top_p",
        "presence_penalty",
        "frequency_penalty",
        "seed",
        "parallel_tool_calls",
    ] {
        if let Some(value) = body.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    if let Some(value) = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_completion_tokens"))
    {
        out.insert("max_tokens".to_string(), value.clone());
    }
    // 上游由 `prepare_outbound_body` 强制流式；这里显式置 true，保持请求形状自洽
    // （也让本函数单测能断言「翻译结果一定是流式」）。
    out.insert("stream".to_string(), json!(true));

    let mut messages: Vec<Value> = Vec::new();
    if let Some(text) = body.get("instructions").and_then(Value::as_str) {
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }
    if let Some(input) = body.get("input") {
        input_to_messages(input, &mut messages);
    }
    out.insert("messages".to_string(), Value::Array(messages));

    // tools：Responses 的**扁平** schema → chat 的 `function` 包装。
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                match tool.get("type").and_then(Value::as_str) {
                    // `type` 缺省也按 function 处理（部分客户端省略）。
                    Some("function") | None => {
                        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
                        if name.is_empty() {
                            return None;
                        }
                        let description = tool
                            .get("description")
                            .cloned()
                            .unwrap_or_else(|| json!(""));
                        let parameters = tool
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                        Some(json!({
                            "type": "function",
                            "function": {
                                "name": name,
                                "description": description,
                                "parameters": parameters,
                            }
                        }))
                    }
                    // 内置工具（web_search / file_search / computer_use …）上游不支持 ⇒ 跳过。
                    _ => None,
                }
            })
            .collect();
        if !converted.is_empty() {
            out.insert("tools".to_string(), Value::Array(converted));
        }
    }

    if let Some(choice) = body.get("tool_choice") {
        if let Some(text) = choice.as_str() {
            out.insert("tool_choice".to_string(), json!(text));
        } else if let Some(kind) = choice.get("type").and_then(Value::as_str) {
            match kind {
                "function" => {
                    if let Some(name) = choice.get("name").and_then(Value::as_str) {
                        out.insert(
                            "tool_choice".to_string(),
                            json!({"type": "function", "function": {"name": name}}),
                        );
                    }
                }
                "none" => {
                    out.insert("tool_choice".to_string(), json!("none"));
                }
                _ => {}
            }
        }
    }

    Value::Object(out)
}

/// 展开 Responses 的 `input`（字符串或 items 数组）到 chat `messages`。
fn input_to_messages(input: &Value, out: &mut Vec<Value>) {
    match input {
        Value::String(text) => out.push(json!({"role": "user", "content": text})),
        Value::Array(items) => {
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    // `type` 缺省：容忍 `{role, content}` 简写。
                    Some("message") | None => {
                        let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                        let content = parts_to_content(item.get("content"));
                        out.push(json!({"role": role, "content": content}));
                    }
                    Some("function_call") => {
                        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                        let arguments = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        out.push(json!({
                            "role": "assistant",
                            "content": Value::Null,
                            "tool_calls": [{
                                "id": call_id,
                                "type": "function",
                                "function": {"name": name, "arguments": arguments},
                            }],
                        }));
                    }
                    Some("function_call_output") => {
                        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                        let content = match item.get("output") {
                            Some(Value::String(text)) => text.clone(),
                            Some(other) => other.to_string(),
                            None => String::new(),
                        };
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": content,
                        }));
                    }
                    // `reasoning` 是**出站**产物；上游不接受 reasoning 作为输入 ⇒ 丢弃。
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// 把 Responses 的 content（字符串 / parts 数组）转成 chat content。
///
/// 纯文本 parts 合并成一个字符串（与上游文本协议一致）；出现图片时退化为
/// chat 的多模态数组（`[{type:"text"},{type:"image_url"}]`）。
fn parts_to_content(content: Option<&Value>) -> Value {
    match content {
        None | Some(Value::Null) => json!(""),
        Some(Value::String(text)) => json!(text),
        Some(Value::Array(parts)) => {
            let mut has_image = false;
            let mut text = String::new();
            let mut multimodal: Vec<Value> = Vec::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text") | Some("output_text") | Some("text") | None => {
                        if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                            text.push_str(part_text);
                            multimodal.push(json!({"type": "text", "text": part_text}));
                        }
                    }
                    Some("input_image") => {
                        has_image = true;
                        if let Some(url) = part.get("image_url").and_then(Value::as_str) {
                            multimodal
                                .push(json!({"type": "image_url", "image_url": {"url": url}}));
                        }
                    }
                    _ => {}
                }
            }
            if has_image {
                Value::Array(multimodal)
            } else {
                json!(text)
            }
        }
        Some(other) => other.clone(),
    }
}

/// 非流式出口的**唯一**形状构造入参。
///
/// 两条上游的聚合产物形状不同（WB 的 [`CompletionAccumulator`]、Qoder 的
/// `qoder::sse::AggregatedChat`），但 Responses 对象的形状**只能有一份** ——
/// 各自拼一遍必然漂移（改一处另一处静默给旧形状）。
#[derive(Debug, Clone, Copy)]
pub struct ResponsesParts<'a> {
    pub content: &'a str,
    pub reasoning: &'a str,
    /// chat 形态的工具调用：`{id, type, function:{name, arguments}}`。
    pub tool_calls: &'a [Value],
    pub model: Option<&'a str>,
    pub created_at: Option<i64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// 由聚合结果构造 Responses 对象（非流式出口）。
pub fn to_response_object(accumulator: &CompletionAccumulator, fallback_model: &str) -> Value {
    let tool_calls: Vec<Value> = accumulator
        .tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": call.arguments},
            })
        })
        .collect();
    to_response_object_from(
        &ResponsesParts {
            content: &accumulator.content,
            reasoning: &accumulator.reasoning,
            tool_calls: &tool_calls,
            model: accumulator.model.as_deref(),
            created_at: accumulator.created,
            input_tokens: accumulator.prompt_tokens.unwrap_or(0),
            output_tokens: accumulator.completion_tokens.unwrap_or(0),
        },
        fallback_model,
    )
}

/// 由**部件**构造 Responses 对象（非流式出口的唯一形状实现，两条上游共用）。
pub fn to_response_object_from(parts: &ResponsesParts<'_>, fallback_model: &str) -> Value {
    let model = parts.model.unwrap_or(fallback_model).to_string();
    let created_at = parts
        .created_at
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let mut output: Vec<Value> = Vec::new();
    if !parts.reasoning.is_empty() {
        output.push(json!({
            "id": new_id("rs"),
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": parts.reasoning}],
        }));
    }
    // 无工具调用时恒输出 message（哪怕是空串），与官方「至少一个 message item」的形状一致。
    if !parts.content.is_empty() || parts.tool_calls.is_empty() {
        output.push(json!({
            "id": new_id("msg"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": parts.content, "annotations": []}],
        }));
    }
    for tool_call in parts.tool_calls {
        let call_id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let function = tool_call.get("function");
        let name = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = function
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        output.push(json!({
            "id": new_id("fc"),
            "type": "function_call",
            "status": "completed",
            "call_id": call_id,
            "name": name,
            "arguments": arguments,
        }));
    }

    let input_tokens = parts.input_tokens;
    let output_tokens = parts.output_tokens;
    json!({
        "id": new_id("resp"),
        "object": "response",
        "created_at": created_at,
        "status": "completed",
        "model": model,
        "output": output,
        // 官方非流式对象带 `output_text` 便捷字段；流式末帧没有它（见 `finish` 的注解）。
        "output_text": parts.content,
        "parallel_tool_calls": true,
        "tools": [],
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
        },
    })
}

/// 生成带前缀的随机 id（`resp_` / `msg_` / `rs_` / `fc_`）。
fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// 上游 `chat.completions` SSE → **Responses 事件流**。
///
/// 事件编排（与官方一致的最小集）：
/// `response.created` → `response.in_progress` →（按需）`response.output_item.added`
/// / `response.content_part.added` / `response.output_text.delta` … →
/// 收尾 `…done` 事件 → `response.completed`。
///
/// ⚠️ Responses 的流**不以 `data: [DONE]` 结尾**（与 chat completions 不同），
/// 因此本包装**不补** `[DONE]`，`response.completed` 即流终止信号。
pub struct ResponsesSseStream<S> {
    inner: Pin<Box<S>>,
    buffer: Vec<u8>,
    pending: VecDeque<Bytes>,
    finished: bool,
    started: bool,
    seq: i64,
    response_id: String,
    model: String,
    created_at: i64,
    message_id: String,
    reasoning_id: String,
    message_open: bool,
    reasoning_open: bool,
    tool_open: Vec<bool>,
    text: String,
    reasoning: String,
    tool_calls: Vec<ToolCallAccum>,
    usage: Option<Value>,
}

impl<S> ResponsesSseStream<S> {
    pub fn new(inner: S, model: String) -> Self {
        Self {
            inner: Box::pin(inner),
            buffer: Vec::new(),
            pending: VecDeque::new(),
            finished: false,
            started: false,
            seq: 0,
            response_id: new_id("resp"),
            model,
            created_at: chrono::Utc::now().timestamp(),
            message_id: new_id("msg"),
            reasoning_id: new_id("rs"),
            message_open: false,
            reasoning_open: false,
            tool_open: Vec::new(),
            text: String::new(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            usage: None,
        }
    }

    /// 事件序号：官方从 0 起严格递增。
    fn next_seq(&mut self) -> i64 {
        let seq = self.seq;
        self.seq += 1;
        seq
    }

    /// 组装 `response` 对象的**公共骨架**（`output` / `usage` 由调用方补）。
    fn response_skeleton(&self, status: &str) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "parallel_tool_calls": true,
            "tools": [],
        })
    }

    fn push_event(&mut self, event: &str, mut data: Value) {
        let seq = self.next_seq();
        if let Some(object) = data.as_object_mut() {
            object.insert("sequence_number".to_string(), json!(seq));
        }
        self.pending.push_back(sse_event(event, &data));
    }

    /// 首次产出前补 `response.created` + `response.in_progress`。
    fn ensure_started(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let created = self.response_skeleton("in_progress");
        self.push_event(
            "response.created",
            json!({"type": "response.created", "response": created}),
        );
        let in_progress = self.response_skeleton("in_progress");
        self.push_event(
            "response.in_progress",
            json!({"type": "response.in_progress", "response": in_progress}),
        );
    }

    fn open_reasoning(&mut self) {
        if self.reasoning_open {
            return;
        }
        self.reasoning_open = true;
        let id = self.reasoning_id.clone();
        self.push_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {"id": id, "type": "reasoning", "summary": []},
            }),
        );
        let id = self.reasoning_id.clone();
        self.push_event(
            "response.reasoning_summary_part.added",
            json!({
                "type": "response.reasoning_summary_part.added",
                "item_id": id,
                "output_index": 0,
                "summary_index": 0,
                "part": {"type": "summary_text", "text": ""},
            }),
        );
    }

    fn open_message(&mut self) {
        if self.message_open {
            return;
        }
        self.message_open = true;
        let id = self.message_id.clone();
        self.push_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": 1,
                "item": {
                    "id": id,
                    "type": "message",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": [],
                },
            }),
        );
        let id = self.message_id.clone();
        self.push_event(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "item_id": id,
                "output_index": 1,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []},
            }),
        );
    }

    fn open_tool(&mut self, index: usize) {
        while self.tool_open.len() <= index {
            self.tool_open.push(false);
        }
        if self.tool_open[index] {
            return;
        }
        self.tool_open[index] = true;
        let item = &self.tool_calls[index];
        let item_id = if item.id.is_empty() {
            new_id("fc")
        } else {
            item.id.clone()
        };
        let name = item.name.clone();
        let output_index = 2 + index;
        self.push_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": {
                    "id": item_id,
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": item.id,
                    "name": name,
                    "arguments": "",
                },
            }),
        );
    }

    /// 吞入一个上游 SSE `data:` 负载，产出 0..n 个 Responses 事件。
    fn handle_payload(&mut self, payload: &str) {
        if payload == "[DONE]" || payload.is_empty() {
            return;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        if let Some(usage) = chunk.get("usage") {
            if !usage.is_null() {
                self.usage = Some(usage.clone());
            }
        }
        // 上游中途报错（Qoder 的 `event:error` 帧、WB 的 `{"error":{…}}` 分片）——
        // **必须转成 Responses 的 `error` 事件**：否则本层只看到「没有 choices」，
        // 静默走完 `response.completed` ⇒ 客户端拿到「200 + 空回答」，故障隐身。
        if let Some(error) = chunk.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            // OpenAI 的 `code` 是字符串；Qoder 给的是数字状态码 ⇒ 统一成字符串。
            let code = match error.get("code") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Number(number)) => number.to_string(),
                _ => "upstream_error".to_string(),
            };
            self.ensure_started();
            self.push_event(
                "error",
                json!({"type": "error", "code": code, "message": message}),
            );
            return;
        }
        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return;
        };
        let Some(delta) = choice.get("delta") else {
            return;
        };

        if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
            if !reasoning.is_empty() {
                self.ensure_started();
                self.open_reasoning();
                self.reasoning.push_str(reasoning);
                let id = self.reasoning_id.clone();
                self.push_event(
                    "response.reasoning_summary_text.delta",
                    json!({
                        "type": "response.reasoning_summary_text.delta",
                        "item_id": id,
                        "output_index": 0,
                        "summary_index": 0,
                        "delta": reasoning,
                    }),
                );
            }
        }

        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                self.ensure_started();
                self.open_message();
                self.text.push_str(content);
                let id = self.message_id.clone();
                self.push_event(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": id,
                        "output_index": 1,
                        "content_index": 0,
                        "delta": content,
                    }),
                );
            }
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
                let mut argument_delta = String::new();
                if let Some(function) = tool_call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        if !name.is_empty() {
                            slot.name.push_str(name);
                        }
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                        slot.arguments.push_str(arguments);
                        argument_delta = arguments.to_string();
                    }
                }
                self.ensure_started();
                self.open_tool(index);
                if !argument_delta.is_empty() {
                    let item_id = self.tool_calls[index].id.clone();
                    self.push_event(
                        "response.function_call_arguments.delta",
                        json!({
                            "type": "response.function_call_arguments.delta",
                            "item_id": item_id,
                            "output_index": 2 + index,
                            "delta": argument_delta,
                        }),
                    );
                }
            }
        }
    }

    /// 收尾：关闭所有已开 item，再发 `response.completed`。
    fn finish(&mut self) {
        if !self.started {
            // 上游一个增量都没给（例如直接报错）：仍给出完整的 created/completed 骨架，
            // 避免下游拿到「零事件」的空流而挂起。
            self.ensure_started();
        }
        if self.reasoning_open {
            let text = self.reasoning.clone();
            let id = self.reasoning_id.clone();
            self.push_event(
                "response.reasoning_summary_text.done",
                json!({
                    "type": "response.reasoning_summary_text.done",
                    "item_id": id,
                    "output_index": 0,
                    "summary_index": 0,
                    "text": text,
                }),
            );
            let id = self.reasoning_id.clone();
            self.push_event(
                "response.reasoning_summary_part.done",
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": id,
                    "output_index": 0,
                    "summary_index": 0,
                    "part": {"type": "summary_text", "text": self.reasoning},
                }),
            );
            let id = self.reasoning_id.clone();
            self.push_event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": {
                        "id": id,
                        "type": "reasoning",
                        "summary": [{"type": "summary_text", "text": self.reasoning}],
                    },
                }),
            );
        }
        if self.message_open {
            let id = self.message_id.clone();
            self.push_event(
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": id,
                    "output_index": 1,
                    "content_index": 0,
                    "text": self.text,
                }),
            );
            let id = self.message_id.clone();
            self.push_event(
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "item_id": id,
                    "output_index": 1,
                    "content_index": 0,
                    "part": {"type": "output_text", "text": self.text, "annotations": []},
                }),
            );
            let id = self.message_id.clone();
            self.push_event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": 1,
                    "item": {
                        "id": id,
                        "type": "message",
                        "status": "completed",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": self.text, "annotations": []}],
                    },
                }),
            );
        }
        for index in 0..self.tool_calls.len() {
            if !self.tool_open.get(index).copied().unwrap_or(false) {
                continue;
            }
            let item = &self.tool_calls[index];
            let item_id = if item.id.is_empty() {
                new_id("fc")
            } else {
                item.id.clone()
            };
            let arguments = item.arguments.clone();
            self.push_event(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": item_id,
                    "output_index": 2 + index,
                    "arguments": arguments,
                }),
            );
            let item_id = self.tool_calls[index].id.clone();
            let name = self.tool_calls[index].name.clone();
            let arguments = self.tool_calls[index].arguments.clone();
            self.push_event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": 2 + index,
                    "item": {
                        "id": item_id,
                        "type": "function_call",
                        "status": "completed",
                        "call_id": self.tool_calls[index].id,
                        "name": name,
                        "arguments": arguments,
                    },
                }),
            );
        }

        let input_tokens = self
            .usage
            .as_ref()
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output_tokens = self
            .usage
            .as_ref()
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        let mut output: Vec<Value> = Vec::new();
        if self.reasoning_open {
            output.push(json!({
                "id": self.reasoning_id,
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": self.reasoning}],
            }));
        }
        if self.message_open {
            output.push(json!({
                "id": self.message_id,
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{"type": "output_text", "text": self.text, "annotations": []}],
            }));
        }
        for index in 0..self.tool_calls.len() {
            if !self.tool_open.get(index).copied().unwrap_or(false) {
                continue;
            }
            let item = &self.tool_calls[index];
            output.push(json!({
                "id": item.id,
                "type": "function_call",
                "status": "completed",
                "call_id": item.id,
                "name": item.name,
                "arguments": item.arguments,
            }));
        }

        let mut response = self.response_skeleton("completed");
        if let Some(object) = response.as_object_mut() {
            object.insert("output".to_string(), Value::Array(output));
            object.insert(
                "usage".to_string(),
                json!({
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "total_tokens": input_tokens + output_tokens,
                }),
            );
        }
        self.push_event(
            "response.completed",
            json!({"type": "response.completed", "response": response}),
        );
    }

    /// 按行切分上游字节并处理完整的 SSE 行。
    fn ingest(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        while let Some(position) = self.buffer.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=position).collect();
            let text = String::from_utf8_lossy(&line);
            let trimmed = text.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                let payload = rest.trim();
                if !payload.is_empty() {
                    self.handle_payload(payload);
                }
            }
        }
        if self.buffer.len() > MAX_LINE_BUFFER_BYTES {
            self.buffer.clear();
        }
    }
}

impl<S, E> Stream for ResponsesSseStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Debug,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(chunk) = this.pending.pop_front() {
                return Poll::Ready(Some(Ok(chunk)));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    this.finish();
                    this.finished = true;
                    if let Some(chunk) = this.pending.pop_front() {
                        return Poll::Ready(Some(Ok(chunk)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Err(error))) => {
                    eprintln!("[gateway] Responses 上游 SSE 流中途异常: {error:?}");
                    this.finish();
                    this.finished = true;
                    if let Some(chunk) = this.pending.pop_front() {
                        return Poll::Ready(Some(Ok(chunk)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Ok(chunk))) => this.ingest(&chunk),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    fn chunk(text: &str) -> Result<Bytes, std::io::Error> {
        Ok(Bytes::from(text.to_string()))
    }

    async fn collect_events<S>(stream: S) -> Vec<Value>
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin,
    {
        let chunks: Vec<Result<Bytes, std::io::Error>> = stream.collect().await;
        let mut bytes = Vec::new();
        for chunk in chunks {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        let text = String::from_utf8(bytes).unwrap();
        text.split("\n\n")
            .filter(|block| !block.trim().is_empty())
            .filter_map(|block| {
                block
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
            })
            .collect()
    }

    fn event_types(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| event.get("type").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn upstream_request_translates_input_and_instructions() {
        let body = json!({
            "model": "glm-5.2",
            "instructions": "You are terse.",
            "input": "hi",
            "max_output_tokens": 128,
            "temperature": 0.3,
        });
        let upstream = to_upstream_request(&body);
        assert_eq!(upstream["model"], "glm-5.2");
        assert_eq!(upstream["max_tokens"], 128);
        assert_eq!(upstream["temperature"], 0.3);
        assert_eq!(upstream["stream"], json!(true), "上游恒为流式");
        assert_eq!(upstream["messages"][0]["role"], "system");
        assert_eq!(upstream["messages"][0]["content"], "You are terse.");
        assert_eq!(upstream["messages"][1]["role"], "user");
        assert_eq!(upstream["messages"][1]["content"], "hi");
    }

    #[test]
    fn upstream_request_translates_items_tools_and_tool_choice() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "part one "},
                    {"type": "input_text", "text": "part two"}
                ]},
                {"type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{\"city\":\"SF\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "sunny"},
                {"type": "reasoning", "summary": []}
            ],
            "tools": [
                {"type": "function", "name": "get_weather", "description": "d", "parameters": {"type": "object"}},
                {"type": "web_search"}
            ],
            "tool_choice": {"type": "function", "name": "get_weather"},
        });
        let upstream = to_upstream_request(&body);
        // 纯文本 parts 合并成字符串
        assert_eq!(upstream["messages"][0]["content"], "part one part two");
        // function_call → assistant.tool_calls
        assert_eq!(upstream["messages"][1]["role"], "assistant");
        assert_eq!(
            upstream["messages"][1]["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
        // function_call_output → tool 消息
        assert_eq!(upstream["messages"][2]["role"], "tool");
        assert_eq!(upstream["messages"][2]["tool_call_id"], "call_1");
        assert_eq!(upstream["messages"][2]["content"], "sunny");
        // reasoning item 被丢弃（上游不接受）
        assert_eq!(upstream["messages"].as_array().unwrap().len(), 3);
        // 内置工具被跳过，只剩 function
        assert_eq!(upstream["tools"].as_array().unwrap().len(), 1);
        assert_eq!(upstream["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(upstream["tool_choice"]["function"]["name"], "get_weather");
    }

    #[test]
    fn upstream_request_maps_image_parts_to_multimodal() {
        let body = json!({
            "model": "m",
            "input": [{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "look"},
                {"type": "input_image", "image_url": "https://example.com/a.png"}
            ]}],
        });
        let upstream = to_upstream_request(&body);
        let content = &upstream["messages"][0]["content"];
        assert!(content.is_array(), "含图片时必须走多模态数组: {content}");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "https://example.com/a.png");
    }

    #[test]
    fn response_object_from_accumulator_has_expected_shape() {
        let sse = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"created\":7,\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        );
        let accumulator = CompletionAccumulator::from_sse(sse);
        let response = to_response_object(&accumulator, "fallback");
        assert_eq!(response["object"], "response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["model"], "m");
        assert_eq!(response["created_at"], 7);
        assert_eq!(response["output_text"], "Hello");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["content"][0]["text"], "Hello");
        assert_eq!(response["usage"]["input_tokens"], 3);
        assert_eq!(response["usage"]["output_tokens"], 2);
        assert_eq!(response["usage"]["total_tokens"], 5);
        assert!(
            response["id"].as_str().unwrap().starts_with("resp_"),
            "id 需带 resp_ 前缀"
        );
    }

    #[test]
    fn response_object_includes_function_calls() {
        let sse =             "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n";
        let accumulator = CompletionAccumulator::from_sse(sse);
        let response = to_response_object(&accumulator, "m");
        assert_eq!(response["output"][0]["type"], "function_call");
        assert_eq!(response["output"][0]["call_id"], "call_1");
        assert_eq!(response["output"][0]["name"], "f");
        // 无正文时不产生空 message item
        assert!(
            response["output"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["type"] != "message"),
            "有工具调用且无正文时不应输出空 message: {}",
            response["output"]
        );
    }

    #[tokio::test]
    async fn stream_emits_full_event_sequence_for_text() {
        let input = vec![
            chunk("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string());
        let events = collect_events(stream).await;
        let types = event_types(&events);

        assert_eq!(types[0], "response.created");
        assert_eq!(types[1], "response.in_progress");
        assert!(types.contains(&"response.output_item.added".to_string()));
        assert!(types.contains(&"response.content_part.added".to_string()));
        assert!(types.contains(&"response.output_text.delta".to_string()));
        assert!(types.contains(&"response.output_text.done".to_string()));
        assert_eq!(*types.last().unwrap(), "response.completed");

        // 序号从 0 起严格递增（下游按序号归并）
        let seqs: Vec<i64> = events
            .iter()
            .map(|event| event["sequence_number"].as_i64().unwrap())
            .collect();
        assert_eq!(seqs, (0..seqs.len() as i64).collect::<Vec<_>>());

        // 文本增量正确
        let deltas: Vec<String> = events
            .iter()
            .filter(|event| event["type"] == "response.output_text.delta")
            .map(|event| event["delta"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(deltas, vec!["Hel", "lo"]);

        // 最终 completed 携带完整文本与用量
        let completed = events.last().unwrap();
        assert_eq!(completed["response"]["status"], "completed");
        assert_eq!(completed["response"]["output_text"], Value::Null);
        assert_eq!(
            completed["response"]["output"][0]["content"][0]["text"],
            "Hello"
        );
        assert_eq!(completed["response"]["usage"]["total_tokens"], 5);
    }

    #[tokio::test]
    async fn stream_does_not_emit_done_marker() {
        let input = vec![
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string());
        let chunks: Vec<Result<Bytes, std::io::Error>> = stream.collect().await;
        let text: String = chunks
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk.as_ref().unwrap()).to_string())
            .collect();
        assert!(
            !text.contains("[DONE]"),
            "Responses 流不应补 [DONE]（与 chat completions 不同）"
        );
    }

    #[tokio::test]
    async fn stream_splits_reasoning_and_text_items() {
        let input = vec![
            chunk("data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string());
        let events = collect_events(stream).await;
        let types = event_types(&events);
        assert!(types.contains(&"response.reasoning_summary_text.delta".to_string()));
        assert!(types.contains(&"response.output_text.delta".to_string()));

        let completed = events.last().unwrap();
        let output = completed["response"]["output"].as_array().unwrap();
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(output[0]["summary"][0]["text"], "think");
        assert_eq!(output[1]["type"], "message");
        assert_eq!(output[1]["content"][0]["text"], "answer");
    }

    #[tokio::test]
    async fn stream_with_no_content_still_completes() {
        let input: Vec<Result<Bytes, std::io::Error>> = vec![];
        let stream = ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string());
        let events = collect_events(stream).await;
        let types = event_types(&events);
        assert_eq!(types.first().unwrap(), "response.created");
        assert_eq!(types.last().unwrap(), "response.completed");
    }

    /// 上游中途报错帧 ⇒ **`error` 事件**（不能静默成「200 + 空回答」，issue #64）。
    ///
    /// 可证伪：去掉 `handle_payload` 里那段 `chunk.get("error")` ⇒ 本用例一条断言都拿不到。
    #[tokio::test]
    async fn stream_surfaces_upstream_error_frame_as_error_event() {
        let input = vec![
            chunk("data: {\"error\":{\"message\":\"上游错误（HTTP 429）：rate limited\",\"type\":\"upstream_error\",\"code\":429}}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string());
        let events = collect_events(stream).await;
        let error = events
            .iter()
            .find(|event| event["type"] == "error")
            .expect("必须有 error 事件");
        assert!(
            error["message"].as_str().unwrap().contains("429"),
            "{error}"
        );
        // `code` 必须是字符串（OpenAI schema；Qoder 给的是数字 429）。
        assert_eq!(error["code"], "429");
    }

    #[tokio::test]
    async fn stream_handles_payload_split_across_chunks() {
        let input = vec![
            chunk("data: {\"choices\":[{\"delta\":{\"content\":\"a"),
            chunk("b\"}}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let stream = ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string());
        let events = collect_events(stream).await;
        let completed = events.last().unwrap();
        assert_eq!(
            completed["response"]["output"][0]["content"][0]["text"], "ab",
            "跨 chunk 切分的 JSON 必须正确拼回"
        );
    }
}
