//! 路由组装、鉴权与错误响应辅助。

pub mod biz;
pub mod chat;
pub mod health;
pub mod messages;
pub mod models;
pub mod relay;
pub mod responses;
pub mod status;

use axum::body::Bytes;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::apikey::ApiKeyRecord;
use crate::error::GatewayError;
use crate::state::GatewayState;

/// 从请求头提取 API Key：优先 `x-api-key`，其次 `Authorization: Bearer <key>`。
pub(crate) fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-api-key").and_then(|value| value.to_str().ok()) {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .unwrap_or(raw)
        .trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// 校验 API Key 并返回记录（Key 绑定 region，不读客户端声明）。
pub(crate) fn authenticate(
    state: &GatewayState,
    headers: &HeaderMap,
) -> Result<ApiKeyRecord, GatewayError> {
    let presented = extract_bearer(headers).ok_or_else(|| {
        GatewayError::Unauthorized("缺少凭据：请在 Authorization 头携带 `Bearer <API Key>`".to_string())
    })?;
    let record = state
        .keys
        .verify(&presented)
        .ok_or_else(|| GatewayError::Unauthorized("API Key 无效或已吊销".to_string()))?;
    state.keys.touch(&record.id);
    Ok(record)
}

/// JSON 响应。
pub(crate) fn json_response(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// OpenAI 端点错误响应。
pub(crate) fn openai_error(error: GatewayError) -> Response {
    let status = StatusCode::from_u16(error.status()).unwrap_or(StatusCode::BAD_GATEWAY);
    json_response(status, error.openai_body())
}

/// Anthropic 端点错误响应。
pub(crate) fn anthropic_error(error: GatewayError) -> Response {
    let status = StatusCode::from_u16(error.status()).unwrap_or(StatusCode::BAD_GATEWAY);
    json_response(status, error.anthropic_body())
}

/// 解析请求体为 JSON（空体视为空对象）。
pub(crate) fn parse_body(bytes: &Bytes) -> Result<Value, GatewayError> {
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(bytes)
        .map_err(|error| GatewayError::BadRequest(format!("请求体不是合法 JSON: {error}")))
}

/// 构造 SSE 响应（用于流式透传/转换）。
pub(crate) fn sse_response<S>(stream: S) -> Response
where
    S: futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(axum::body::Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
