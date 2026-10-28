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

use std::collections::{HashSet, VecDeque};
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Bytes;
use futures_util::Stream;
use serde_json::{json, Map, Value};

use crate::protocol::openai::{sse_event, CompletionAccumulator, ToolCallAccum};

/// 单行缓冲区上限：畸形流（永不出现 `\n`）不得让内存无界增长。超限时原样冲刷保底。
const MAX_LINE_BUFFER_BYTES: usize = 256 * 1024;

/// freeform（custom）工具的**出站降级说明**。
///
/// Qoder / WB 上游只有 chat 的 `function` 工具，没有 Responses 的 `custom` 概念。
/// Codex 等客户端把文件编辑工具声明成 `{"type":"custom","name":"apply_patch"}`，
/// 若不降级就会被静默丢弃 —— 客户端以为工具在，模型却永远调不到（「Codex 改不动文件」）。
const CUSTOM_TOOL_HINT: &str = "This is a freeform tool. Put the COMPLETE raw payload into \
the single 'input' string parameter, verbatim. Do not wrap it in JSON, do not wrap it in \
markdown code fences, do not add commentary.";

/// 一条工具声明是不是 Responses 的 `custom`（freeform）工具。
fn is_custom_tool(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .map(|kind| kind.eq_ignore_ascii_case("custom"))
        .unwrap_or(false)
}

/// 请求里声明为 custom（freeform）的工具名集合 —— 出站降级与入站还原共用这一份。
pub fn custom_tool_names(body: &Value) -> HashSet<String> {
    body.get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter(|tool| is_custom_tool(tool))
                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 把 `custom` 工具降级成「单 `input` 字符串参数」的 function 工具。
fn downgrade_custom_tool(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
    if name.is_empty() {
        return None;
    }
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let grammar = tool
        .get("format")
        .and_then(|format| {
            format
                .get("definition")
                .and_then(Value::as_str)
                .map(|text| format!("\n\nGrammar://n{text}"))
                .or_else(|| {
                    format
                        .get("syntax")
                        .and_then(Value::as_str)
                        .map(|text| format!("\n\nGrammar syntax: {text}"))
                })
        })
        .unwrap_or_default();
    Some(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": format!("{description}\n\n{CUSTOM_TOOL_HINT}{grammar}").trim().to_string(),
            "parameters": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Complete raw payload for this tool, verbatim.",
                    }
                },
                "required": ["input"],
            },
        }
    }))
}

/// 从 `{"input": "..."}` 参数里取回 freeform 原文（取不到就原样返回）。
///
/// 模型有时直接给裸字符串、有时给 `{"input":{...}}`；两种都还原成字符串。
pub fn unwrap_custom_input(arguments: &str) -> String {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(map)) => match map.get("input") {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => arguments.to_string(),
        },
        Ok(Value::String(text)) => text,
        _ => arguments.to_string(),
    }
}

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
                    // ★ freeform（custom）工具：降级成单 `input` 参数的 function。
                    // 不降级的后果是「工具被静默丢弃」——Codex 的 apply_patch 就永远调不到。
                    Some("custom") => downgrade_custom_tool(tool),
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
            // ★ 并行的多个调用必须并进**同一条** assistant 消息的 `tool_calls`（issue #119）。
            // 每条 item 各自产出一条 assistant 消息，等于把「一次并行调用」拆成多轮：
            // 上游按「assistant 消息数与 tool 结果数」校验 ⇒ 回 400/11148
            // `tool calls and tool results do not match`。`merge_into` 记住可续写的那条消息的下标，
            // 遇到任何**真的产出**了新消息的 item 就清空（否则会把不同轮次合并）。
            let mut merge_into: Option<usize> = None;
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    // `type` 缺省：容忍 `{role, content}` 简写。
                    Some("message") | None => {
                        let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                        let content = parts_to_content(item.get("content"));
                        out.push(json!({"role": role, "content": content}));
                        merge_into = None;
                    }
                    Some("function_call") => {
                        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                        let arguments = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        push_tool_call(
                            out,
                            &mut merge_into,
                            json!({
                                "id": call_id,
                                "type": "function",
                                "function": {"name": name, "arguments": arguments},
                            }),
                        );
                    }
                    // ★ freeform 调用的历史项：还原成 chat 的 function 调用 + `{"input": …}`
                    // 参数（与出站降级的形状对得上，多轮 Codex 才不会断）。
                    Some("custom_tool_call") => {
                        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                        let input = item
                            .get("input")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        push_tool_call(
                            out,
                            &mut merge_into,
                            json!({
                                "id": call_id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": json!({"input": input}).to_string(),
                                },
                            }),
                        );
                    }
                    Some("custom_tool_call_output") | Some("function_call_output") => {
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
                        merge_into = None;
                    }
                    // `reasoning` 是**出站**产物；上游不接受 reasoning 作为输入 ⇒ 丢弃。
                    // 丢弃不打断合并：调用之间穿插 reasoning 是 Codex 的常态形状。
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// 追加一条 tool call：上一段仍是「只有 tool_calls 的 assistant」就续写，否则另起一条。
///
/// 判据刻意收窄成「上一条消息 `content` 为 `null` 且已有 `tool_calls`」——
/// 带正文的 assistant 消息不参与合并，避免把上一轮的收尾文本与这一轮的调用并成一条。
fn push_tool_call(out: &mut Vec<Value>, merge_into: &mut Option<usize>, call: Value) {
    if let Some(index) = *merge_into {
        if let Some(array) = out
            .get_mut(index)
            .and_then(|message| message.get_mut("tool_calls"))
            .and_then(Value::as_array_mut)
        {
            array.push(call);
            return;
        }
    }
    out.push(json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": [call],
    }));
    *merge_into = Some(out.len().saturating_sub(1));
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
                        // `image_url` 既可能是字符串，也可能是 `{url}` 对象；
                        // 两种都要认，否则图片被整条丢弃。
                        let url = part
                            .get("image_url")
                            .and_then(|value| {
                                value
                                    .get("url")
                                    .and_then(Value::as_str)
                                    .or_else(|| value.as_str())
                            })
                            .map(str::to_string);
                        if let Some(url) = url {
                            has_image = true;
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
    /// 本次请求里声明为 **custom（freeform）** 的工具名 —— 命中的调用要还原成
    /// `custom_tool_call`，而不是 `function_call`。
    pub custom: &'a HashSet<String>,
}

/// 由聚合结果构造 Responses 对象（非流式出口）。
pub fn to_response_object(
    accumulator: &CompletionAccumulator,
    fallback_model: &str,
    custom: &HashSet<String>,
) -> Value {
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
            custom,
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
        if parts.custom.contains(name) {
            // freeform 调用：把 `{"input": …}` 拆回原始 payload。
            output.push(json!({
                "id": new_id("fc"),
                "type": "custom_tool_call",
                "status": "completed",
                "call_id": call_id,
                "name": name,
                "input": unwrap_custom_input(arguments),
            }));
            continue;
        }
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
    /// 声明为 custom（freeform）的工具名：这些调用要按 `custom_tool_call` 事件输出。
    custom: std::sync::Arc<HashSet<String>>,
}

impl<S> ResponsesSseStream<S> {
    /// `custom` 来自**原始** Responses 请求体（[`custom_tool_names`]），
    /// 与出站降级共用同一份集合 ⇒ 声明了 custom 的调用一定会被还原成 custom 事件。
    pub fn new(inner: S, model: String, custom: std::sync::Arc<HashSet<String>>) -> Self {
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
            custom,
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
        let custom = self.custom.contains(&name);
        let output_index = 2 + index;
        let item = if custom {
            // freeform 工具：官方事件的 item 形状是 `custom_tool_call` + `input`。
            json!({
                "id": item_id,
                "type": "custom_tool_call",
                "status": "in_progress",
                "call_id": item.id,
                "name": name,
                "input": "",
            })
        } else {
            json!({
                "id": item_id,
                "type": "function_call",
                "status": "in_progress",
                "call_id": item.id,
                "name": name,
                "arguments": "",
            })
        };
        self.push_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": item,
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
                    let custom = self.custom.contains(&self.tool_calls[index].name);
                    /* ★ freeform 的增量**刻意不发**：上游给的是 `{"input":"…"}` 的 JSON 片段，
                    而 `custom_tool_call_input.delta` 的语义是**原文片段**。把 JSON 片段当原文
                    发出去，按规范累加 delta 的客户端会拼出一坨 `{"input":"…"}`（协议违约）。
                    原文只在 `.done` / `output_item.done` 里给（Codex 正是读那里）。
                    （参考实现发的是 JSON 片段，实测 Codex 能用，但对别的客户端是隐患。） */
                    if !custom {
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
            let custom = self.custom.contains(&item.name);
            if custom {
                // freeform：`.done` 给**拆好的原文**（`{"input": …}` → 原文），
                // 客户端据此拿到 apply_patch 的完整补丁文本。
                self.push_event(
                    "response.custom_tool_call_input.done",
                    json!({
                        "type": "response.custom_tool_call_input.done",
                        "item_id": item_id,
                        "output_index": 2 + index,
                        "input": unwrap_custom_input(&arguments),
                    }),
                );
            } else {
                self.push_event(
                    "response.function_call_arguments.done",
                    json!({
                        "type": "response.function_call_arguments.done",
                        "item_id": item_id,
                        "output_index": 2 + index,
                        "arguments": arguments,
                    }),
                );
            }
            let item_id = self.tool_calls[index].id.clone();
            let name = self.tool_calls[index].name.clone();
            let arguments = self.tool_calls[index].arguments.clone();
            let done_item = if custom {
                json!({
                    "id": item_id,
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": self.tool_calls[index].id,
                    "name": name,
                    "input": unwrap_custom_input(&arguments),
                })
            } else {
                json!({
                    "id": item_id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": self.tool_calls[index].id,
                    "name": name,
                    "arguments": arguments,
                })
            };
            self.push_event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": 2 + index,
                    "item": done_item,
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
            if self.custom.contains(&item.name) {
                output.push(json!({
                    "id": item.id,
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": item.id,
                    "name": item.name,
                    "input": unwrap_custom_input(&item.arguments),
                }));
            } else {
                output.push(json!({
                    "id": item.id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": item.id,
                    "name": item.name,
                    "arguments": item.arguments,
                }));
            }
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

    /// 没有 freeform 工具的常规用例。
    fn empty_custom() -> std::sync::Arc<HashSet<String>> {
        std::sync::Arc::new(HashSet::new())
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

    /// issue #119：并行的多个 `function_call` 若各自产出一条 assistant 消息，上游按
    /// 「tool calls ↔ tool results 对不上」回 400/11148。必须并进同一条消息。
    #[test]
    fn parallel_function_calls_share_one_assistant_message() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "go"}]},
                {"type": "reasoning", "summary": []},
                {"type": "function_call", "call_id": "c0", "name": "exec_command", "arguments": "{\"cmd\":\"ls\"}"},
                // 调用之间穿插 reasoning 不打断合并（Codex 的常态形状）
                {"type": "reasoning", "summary": []},
                {"type": "function_call", "call_id": "c1", "name": "exec_command", "arguments": "{\"cmd\":\"pwd\"}"},
                {"type": "function_call_output", "call_id": "c0", "output": "ok"},
                {"type": "function_call_output", "call_id": "c1", "output": "ok"}
            ]
        });
        let upstream = to_upstream_request(&body);
        let messages = upstream["messages"].as_array().unwrap();
        assert_eq!(
            messages.len(),
            4,
            "user + 一条 assistant + 两条 tool: {messages:?}"
        );
        assert_eq!(messages[1]["role"], "assistant");
        let calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "并行调用必须在同一条 assistant 消息里: {calls:?}"
        );
        assert_eq!(calls[0]["id"], "c0");
        assert_eq!(calls[1]["id"], "c1");
        assert_eq!(messages[2]["tool_call_id"], "c0");
        assert_eq!(messages[3]["tool_call_id"], "c1");
    }

    /// 反向守卫：调用 → 结果 → 再调用（第二轮）**不得**被并进上一轮的 assistant 消息。
    #[test]
    fn tool_calls_after_tool_result_start_a_new_assistant_message() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "go"}]},
                {"type": "function_call", "call_id": "c0", "name": "f", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c0", "output": "ok"},
                {"type": "function_call", "call_id": "c1", "name": "f", "arguments": "{}"},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "done"}]},
                {"type": "function_call", "call_id": "c2", "name": "f", "arguments": "{}"}
            ]
        });
        let upstream = to_upstream_request(&body);
        let messages = upstream["messages"].as_array().unwrap();
        // user / assistant(c0) / tool / assistant(c1) / assistant(text) / assistant(c2)
        assert_eq!(messages.len(), 6, "{messages:?}");
        assert_eq!(messages[1]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(messages[3]["tool_calls"].as_array().unwrap().len(), 1);
        // 带正文的 assistant 消息不参与合并
        assert_eq!(messages[4]["content"], "done");
        assert!(
            messages[4].get("tool_calls").is_none(),
            "文本消息不得被塞 tool_calls"
        );
        assert_eq!(messages[5]["tool_calls"].as_array().unwrap().len(), 1);
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
        let response = to_response_object(&accumulator, "fallback", &HashSet::new());
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
        let response = to_response_object(&accumulator, "m", &HashSet::new());
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
        let stream = ResponsesSseStream::new(
            futures_util::stream::iter(input),
            "m".to_string(),
            empty_custom(),
        );
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
        let stream = ResponsesSseStream::new(
            futures_util::stream::iter(input),
            "m".to_string(),
            empty_custom(),
        );
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
        let stream = ResponsesSseStream::new(
            futures_util::stream::iter(input),
            "m".to_string(),
            empty_custom(),
        );
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
        let stream = ResponsesSseStream::new(
            futures_util::stream::iter(input),
            "m".to_string(),
            empty_custom(),
        );
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
        let stream = ResponsesSseStream::new(
            futures_util::stream::iter(input),
            "m".to_string(),
            empty_custom(),
        );
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
        let stream = ResponsesSseStream::new(
            futures_util::stream::iter(input),
            "m".to_string(),
            empty_custom(),
        );
        let events = collect_events(stream).await;
        let completed = events.last().unwrap();
        assert_eq!(
            completed["response"]["output"][0]["content"][0]["text"], "ab",
            "跨 chunk 切分的 JSON 必须正确拼回"
        );
    }
    // ---------------------------------------------------------------------
    // freeform（custom）工具：Codex 的 `apply_patch` 走这条路
    // ---------------------------------------------------------------------

    /// 出站：`custom` 降级成单 `input` 参数的 function；内置工具（web_search）仍被丢弃。
    ///
    /// 可证伪：删掉 `Some("custom") => downgrade_custom_tool(tool)` ⇒ 工具整条消失，
    /// 模型永远调不到 apply_patch（「Codex 改不动文件」）。
    #[test]
    fn upstream_request_downgrades_custom_tools() {
        let body = json!({
            "model": "m",
            "input": "fix it",
            "tools": [
                {"type": "custom", "name": "apply_patch", "description": "apply a patch",
                 "format": {"type": "grammar", "syntax": "lark", "definition": "start: patch"}},
                {"type": "function", "name": "shell", "description": "run",
                 "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}},
                {"type": "web_search"},
            ],
        });
        let upstream = to_upstream_request(&body);
        let tools = upstream["tools"].as_array().expect("必须有工具");
        assert_eq!(tools.len(), 2, "web_search 必须被跳过: {tools:?}");
        let patch = &tools[0];
        assert_eq!(patch["type"], "function");
        assert_eq!(patch["function"]["name"], "apply_patch");
        // freeform 提示 + 语法都要进 description（模型才知道把原文塞进 input）
        let description = patch["function"]["description"].as_str().unwrap();
        assert!(description.contains("freeform"), "{description}");
        assert!(description.contains("start: patch"), "{description}");
        // 单 `input` 字符串参数、必填
        assert_eq!(patch["function"]["parameters"]["required"][0], "input");
        assert_eq!(
            patch["function"]["parameters"]["properties"]["input"]["type"],
            "string"
        );
        // 普通 function 工具不受影响
        assert_eq!(tools[1]["function"]["name"], "shell");

        // custom 名字集合与降级同源
        let names = custom_tool_names(&body);
        assert!(names.contains("apply_patch"));
        assert!(!names.contains("shell"));
    }

    /// 入站历史：`custom_tool_call` / `custom_tool_call_output` 必须还原成 chat 形状，
    /// 否则 Codex 多轮对话会在第二次请求时丢掉"我刚调过 apply_patch"。
    #[test]
    fn upstream_request_translates_custom_tool_history() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "fix"}]},
                {"type": "custom_tool_call", "call_id": "call_1", "name": "apply_patch", "input": "*** Begin Patch"},
                {"type": "custom_tool_call_output", "call_id": "call_1", "output": "done"},
            ],
        });
        let upstream = to_upstream_request(&body);
        let messages = upstream["messages"].as_array().unwrap();
        let assistant = messages
            .iter()
            .find(|message| message["role"] == "assistant")
            .expect("必须还原成 assistant 调用");
        assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            assistant["tool_calls"][0]["function"]["name"],
            "apply_patch"
        );
        let arguments = assistant["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(unwrap_custom_input(arguments), "*** Begin Patch");
        let tool = messages
            .iter()
            .find(|message| message["role"] == "tool")
            .expect("必须有 tool 结果");
        assert_eq!(tool["tool_call_id"], "call_1");
        assert_eq!(tool["content"], "done");
    }

    /// 非流式出站：custom 调用还原成 `custom_tool_call`，`input` 是**拆好的原文**。
    #[test]
    fn response_object_restores_custom_tool_calls() {
        let sse = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"apply_patch\",\"arguments\":\"{\\\"input\\\":\\\"*** Begin Patch\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n";
        let accumulator = CompletionAccumulator::from_sse(sse);
        let custom: HashSet<String> = ["apply_patch".to_string()].into_iter().collect();
        let response = to_response_object(&accumulator, "m", &custom);
        let item = &response["output"][0];
        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["name"], "apply_patch");
        assert_eq!(item["input"], "*** Begin Patch", "{item}");

        // 未声明为 custom 的同名调用仍走 function_call（集合是唯一判据）
        let response = to_response_object(&accumulator, "m", &HashSet::new());
        assert_eq!(response["output"][0]["type"], "function_call");
    }

    /// 流式出站：custom 调用发 `custom_tool_call_input.done`，item 类型是 `custom_tool_call`。
    #[tokio::test]
    async fn stream_emits_custom_tool_call_events() {
        let input = vec![
            chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"apply_patch\",\"arguments\":\"{\\\"input\\\":\\\"*** Begin Patch\\\"}\"}}]}}]}\n\n"),
            chunk("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"),
            chunk("data: [DONE]\n\n"),
        ];
        let custom = std::sync::Arc::new(
            ["apply_patch".to_string()]
                .into_iter()
                .collect::<HashSet<String>>(),
        );
        let stream =
            ResponsesSseStream::new(futures_util::stream::iter(input), "m".to_string(), custom);
        let events = collect_events(stream).await;
        let types = event_types(&events);
        assert!(
            types.contains(&"response.custom_tool_call_input.done".to_string()),
            "缺 custom 收尾事件（原文只在这里给）: {types:?}"
        );
        assert!(
            !types.contains(&"response.custom_tool_call_input.delta".to_string()),
            "custom 不得发 delta（上游是 JSON 片段，按规范会被拼进原文）: {types:?}"
        );
        assert!(
            !types.contains(&"response.function_call_arguments.delta".to_string()),
            "custom 调用不得发 function 增量: {types:?}"
        );
        let done = events
            .iter()
            .find(|event| event["type"] == "response.custom_tool_call_input.done")
            .unwrap();
        assert_eq!(done["input"], "*** Begin Patch");
        let completed = events.last().unwrap();
        let item = &completed["response"]["output"][0];
        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["input"], "*** Begin Patch");
    }

    /// `unwrap_custom_input`：字符串 / `{"input": …}` / 裸 JSON 三种形态都要能取回原文。
    #[test]
    fn unwrap_custom_input_handles_all_shapes() {
        assert_eq!(unwrap_custom_input("{\"input\":\"raw\"}"), "raw");
        assert_eq!(unwrap_custom_input("\"bare\""), "bare");
        assert_eq!(unwrap_custom_input("not json"), "not json");
        assert_eq!(unwrap_custom_input("{\"input\":{\"a\":1}}"), "{\"a\":1}");
    }
}
