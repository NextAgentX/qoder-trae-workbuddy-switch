//! **200 响应体里的信封错误**识别（P0-3，WorkBuddy 侧）。
//!
//! ## 为什么需要它
//!
//! WorkBuddy 上游有一类失败是**用 200 返回的**：HTTP 状态码是 2xx，但响应体（或流里的
//! 某一帧）装着一个错误信封：
//!
//! ```json
//! {"code": 6004, "msg": "model rate limited", "data": null}
//! ```
//!
//! 旧实现对这类响应**当成功处理**：不写冷却、不解绑粘性、把空补全返回给客户端。
//! 后果是「看起来正常、其实一直失败」—— 那个号在冷却表里永远是干净的，
//! 于是每个请求都优先选中它、每次都吃同一个 200 信封错误。
//!
//! ## 判定必须**保守**
//!
//! 宁可漏判也不能把正常补全当错误：误判会让好账号被冷却（用户侧表现是「明明能用却被跳过」）。
//! 因此规则是**白名单式**的：
//!
//! 1. 有 `choices` 且里面真有元素 ⇒ **不是**错误（正常补全优先）；
//! 2. 否则看显式信号：`"error"` 键 / 非 0 的 `"code"` / SSE 的 `event: error`；
//! 3. 都没有 ⇒ 不是错误。
//!
//! ## 只记冷却，不换号
//!
//! 这里的调用点都在**响应已经完整读到之后**（非流式）或**流已经开始之后**（流式），
//! 按方案 §5 决策 4 的口径：这一类只能「记冷却 + 解绑粘性」，**不换号** ——
//! 换号要求把已经发给客户端的字节撤回来，协议上做不到。

use serde_json::Value;

use crate::error::truncate_with_marker;
use crate::pool::{classify_event, UpstreamEvent};

/// 嗅探出的错误消息上限（字符）。超限时**必须补截断标记**，
/// 否则调用方与排查者无法分辨「上游只发了这么多」与「被网关截掉了」。
const SNIFF_MESSAGE_LIMIT: usize = 300;

/// 识别出的信封错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffedError {
    /// 分类结果（可直接喂给 `Pool::apply_upstream_error`）。
    pub event: UpstreamEvent,
    /// 人话原因（取上游 `msg` / `message` / `error.message`，取不到就用原文片段）。
    pub message: String,
    /// 推断出的等价 HTTP 状态（写冷却/文案用）。
    pub status: u16,
}

/// 扫描**非流式**响应体（或一整段 SSE 文本）。
pub fn sniff_http_body(body: &str) -> Option<SniffedError> {
    if body.trim().is_empty() {
        return None;
    }
    // ★ 先分流：SSE 文本必须**逐帧**判，不能拿整段做「有 choices 就是成功」的判断
    //   —— 一段正常的 delta 帧不会让后面的错误帧变得无所谓。
    if is_sse_text(body) {
        for line in body.lines() {
            let line = line.trim();
            if line.starts_with("event:") && line.contains("error") {
                return Some(build(body));
            }
            if let Some(payload) = line.strip_prefix("data:") {
                let payload = payload.trim();
                if payload.is_empty() || payload == "[DONE]" {
                    continue;
                }
                if let Some(found) = sniff_json_object(payload) {
                    return Some(found);
                }
            }
        }
        return None;
    }

    // 非 SSE：正常补全优先（`choices` 非空 ⇒ 一定是成功，哪怕正文里出现 "error" 字样）。
    if has_non_empty_choices(body) {
        return None;
    }
    sniff_json_object(body).or_else(|| {
        // 不是合法 JSON，但带明确标记（截断/非标准包体）⇒ 仍按错误处理。
        if has_hard_marker(body) {
            Some(build(body))
        } else {
            None
        }
    })
}

/// 这段文本是不是 SSE（有 `data:` / `event:` 行）。
fn is_sse_text(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("data:") || line.starts_with("event:")
    })
}

/// 扫描**流式**增量（SSE chunk）。返回 `Some` 表示这一帧就是错误帧。
///
/// 与 [`sniff_http_body`] 的区别：这里只看**新增的这一块**，不假设它是完整 JSON；
/// 因此只认两种形态：`event: error`，或 `data:` 行里能解析成对象且命中规则。
pub fn sniff_sse_chunk(chunk: &str) -> Option<SniffedError> {
    if has_non_empty_choices(chunk) {
        return None;
    }
    for line in chunk.lines() {
        let line = line.trim();
        if line.starts_with("event:") && line.contains("error") {
            return Some(build(chunk));
        }
        if let Some(payload) = line.strip_prefix("data:") {
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            if let Some(found) = sniff_json_object(payload) {
                return Some(found);
            }
        }
    }
    None
}

/// 扫描**单个 `data:` 负载**（调用方已经把 `data:` 前缀剥掉了）。
///
/// ## 为什么需要这个入口
///
/// [`sniff_sse_chunk`] 按行找 `data:` / `event:` 前缀 —— 对「整块 SSE 文本」正确，
/// 但对已经拆好帧的调用方（`AnthropicSseStream::ingest` / `ResponsesSseStream::ingest`）
/// 就是**恒不命中**：它们手里只有负载本身。把负载重新拼回 `data: ` 再传进去能work，
/// 但每次多一次分配，而且「拼回去再拆」本身容易写错（漏掉前缀就静默失效，
/// 表现为「错误帧被当成正常帧丢弃」—— 正是本 issue 要修的那种隐身故障）。
pub fn sniff_payload(payload: &str) -> Option<SniffedError> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    // 正常补全优先（有 `choices` 元素就不可能是错误信封）。
    if has_non_empty_choices(payload) {
        return None;
    }
    sniff_json_object(payload)
}

/// `choices` 数组里是否**真有元素**（有元素就不可能是错误信封）。
fn has_non_empty_choices(text: &str) -> bool {
    // 快速否决：没有 `choices` 关键字直接返回 false，避免为每个 chunk 做 JSON 解析。
    if !text.contains("\"choices\"") {
        return false;
    }
    // 只在最后一个 `"choices"` 之后找 `[{`（补全的形态是 `"choices":[{...}]`）。
    let tail = match text.rfind("\"choices\"") {
        Some(index) => &text[index..],
        None => return false,
    };
    // 空数组 `"choices":[]` = 错误信封的常见形态（没有内容可返回）。
    !tail.contains("\"choices\":[]") && !tail.contains("\"choices\": []")
}

/// 单个 JSON 对象是否是错误信封。
fn sniff_json_object(text: &str) -> Option<SniffedError> {
    let value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;

    // 显式 `error` 字段（OpenAI 风格 / 上游自定义）。
    if let Some(error) = object.get("error") {
        let means_error = match error {
            Value::Null => false,
            // `{"error": {}}` 这种空对象不算（部分上游会带空占位）。
            Value::Object(map) => !map.is_empty(),
            Value::String(text) => !text.trim().is_empty(),
            _ => true,
        };
        if means_error {
            return Some(build(text));
        }
    }

    // 数字 `code` 且非 0（上游信封形态：`{"code":6004,"msg":"..."}`）。
    if let Some(code) = object.get("code") {
        let is_error = match code {
            Value::Number(number) => number.as_i64().map(|value| value != 0).unwrap_or(true),
            Value::String(text) => text.trim() != "0" && !text.trim().is_empty(),
            _ => false,
        };
        if is_error {
            return Some(build(text));
        }
    }
    None
}

/// 有没有**足够硬**的标记（用于非 JSON 的兜底判定）。
fn has_hard_marker(text: &str) -> bool {
    ["6004", "11102", "12153", "IsModelRateLimit"]
        .iter()
        .any(|marker| text.contains(marker))
}

/// 从响应体推断等价 HTTP 状态，再交给 [`classify_event`] 分类。
fn build(body: &str) -> SniffedError {
    let status = infer_status(body);
    SniffedError {
        event: classify_event(status, body),
        message: extract_message(body).unwrap_or_else(|| truncate(body)),
        status,
    }
}

/// 信封里**显式给出**的等价状态码（`{"error":{"code":429}}` / `{"code":429}`）。
///
/// ## 为什么必须先读它
///
/// 网关自己发出的错误帧（`crate::failover`）带的是**等价 HTTP 状态码**，
/// 而 Qoder / Trae 的错误帧也把状态码放在 `code` 里。不看这个字段、只靠下面的
/// 关键词推断，会把一个明确的 `429` 判成「未知信封 ⇒ 502」—— 客户端于是从
/// `RATE_LIMIT`（可重试）变成 `SERVER`（也可重试，但语义错了，而且
/// `429` 的 Retry-After 语义丢失）。
///
/// 只认 **100..600**：上游私有码（`6004` / `11102` / `12153`）落在这个区间之外，
/// 必须继续走下面的关键词分支（它们各有专门的冷却口径）。
fn explicit_status(body: &str) -> Option<u16> {
    let value: Value = serde_json::from_str(body).ok()?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .or_else(|| value.get("code"))?;
    let number = match code {
        Value::Number(number) => number.as_u64()?,
        Value::String(text) => text.trim().parse().ok()?,
        _ => return None,
    };
    (100..600).contains(&number).then_some(number as u16)
}

/// 推断等价状态码（决定 `classify_event` 走哪条分支、以及对外错误码）。
fn infer_status(body: &str) -> u16 {
    if let Some(status) = explicit_status(body) {
        return status;
    }
    if body.contains("6004") || body.contains("IsModelRateLimit") {
        // 模型级限流：必须给 429，否则 `classify_event` 落不到 `SoftRate` 分支。
        429
    } else if body.contains("11102") {
        // 模型封禁：上游可能用 400/403 返回。
        400
    } else if body.contains("12153") || body.contains("Offline user session not found") {
        401
    } else if body.to_lowercase().contains("insufficient")
        || body.to_lowercase().contains("no credit")
    {
        402
    } else {
        // 未知信封错误 ⇒ 按 5xx 处理（计入熔断，而不是静默放过）。
        502
    }
}

/// 取一句人话原因。
fn extract_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let object = value.as_object()?;
    for key in ["msg", "message", "error_description", "detail"] {
        if let Some(Value::String(text)) = object.get(key) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(truncate_with_marker(text, SNIFF_MESSAGE_LIMIT));
            }
        }
    }
    if let Some(Value::Object(error)) = object.get("error") {
        for key in ["message", "msg"] {
            if let Some(Value::String(text)) = error.get(key) {
                return Some(truncate_with_marker(text, SNIFF_MESSAGE_LIMIT));
            }
        }
    }
    Some(truncate(body))
}

fn truncate(body: &str) -> String {
    truncate_with_marker(body.trim(), SNIFF_MESSAGE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_completion_is_never_an_error() {
        // 正常补全：即使正文里出现 "error" 字样也不能误判。
        let body = r#"{"id":"x","choices":[{"index":0,"message":{"content":"the word error appears here"}}]}"#;
        assert!(sniff_http_body(body).is_none(), "有 choices 就是成功");
    }

    #[test]
    fn envelope_with_numeric_code_is_detected() {
        let body = r#"{"code":6004,"msg":"model rate limited","data":null}"#;
        let found = sniff_http_body(body).expect("必须识别");
        assert_eq!(found.status, 429);
        assert_eq!(
            found.event,
            UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: true
            }
        );
        assert!(found.message.contains("model rate limited"));
    }

    #[test]
    fn empty_choices_array_counts_as_envelope_error() {
        // 上游「无内容」也常以 `"choices":[]` + error 返回。
        //
        // ⚠️ 措辞要避开余额类标记（`quota exhausted` 会被 `classify_event` 判成
        //    `HardCredit` —— 那是**正确**的分类，只是不适合这条用例）。
        let body = r#"{"choices":[],"error":{"message":"model blocked","code":11102}}"#;
        let found = sniff_http_body(body).expect("必须识别");
        assert_eq!(found.status, 400);
        assert_eq!(found.event, UpstreamEvent::ModelBlocked);
        assert!(found.message.contains("model blocked"));
    }

    #[test]
    fn session_dead_envelope_maps_to_401() {
        let body = r#"{"code":12153,"msg":"Offline user session not found"}"#;
        let found = sniff_http_body(body).expect("必须识别");
        assert_eq!(found.status, 401);
        assert_eq!(found.event, UpstreamEvent::SessionDead);
    }

    #[test]
    fn zero_code_and_empty_error_are_not_errors() {
        assert!(sniff_http_body(r#"{"code":0,"data":{"ok":true}}"#).is_none());
        assert!(sniff_http_body(r#"{"error":{},"data":1}"#).is_none());
        assert!(
            sniff_http_body(r#"{"choices":[],"data":1}"#).is_none(),
            "空 choices 但无错误信号"
        );
    }

    #[test]
    fn sse_error_frame_is_detected() {
        let chunk = "event: error\ndata: {\"code\":6004,\"msg\":\"rate limited\"}\n\n";
        let found = sniff_sse_chunk(chunk).expect("必须识别");
        assert_eq!(found.status, 429);
    }

    #[test]
    fn sse_normal_delta_is_not_an_error() {
        let chunk = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
        assert!(sniff_sse_chunk(chunk).is_none());
        assert!(sniff_sse_chunk("data: [DONE]\n\n").is_none());
    }

    /// 整段 SSE 文本（非流式读到的 body）里夹着错误帧。
    #[test]
    fn whole_sse_text_with_error_frame_is_detected() {
        let text = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"code\":6004,\"msg\":\"limited\"}\n\n";
        let found = sniff_http_body(text).expect("必须识别");
        assert_eq!(found.status, 429);
    }

    /// issue #67 §3.4：截断必须带标记 —— 否则调用方无法分辨
    /// 「上游只发了这么多」与「被网关截掉了」。
    #[test]
    fn oversized_message_carries_a_truncation_marker() {
        let body = format!(r#"{{"code":12153,"msg":"{}"}}"#, "x".repeat(500));
        let found = sniff_http_body(&body).expect("非零 code 必须识别为错误信封");
        assert_eq!(found.message.chars().count(), SNIFF_MESSAGE_LIMIT);
        assert!(
            found.message.ends_with("…[truncated]"),
            "截断必须带标记: {}",
            found.message
        );
    }
}
