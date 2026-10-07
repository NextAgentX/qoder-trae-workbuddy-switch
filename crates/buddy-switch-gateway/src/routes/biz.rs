//! `POST /biz/{*path}`：**业务请求转发**（P12 T03 · 本地路由）。
//!
//! ## 为什么需要它
//!
//! 「本地路由」下客户端把 base URL 指向本机网关（**明文 http**，**不需要 CA**），
//! 于是网关会收到本该发给上游的**业务请求**（签到 / 积分 / 旅行）。
//! 本路由把**白名单内**的路径转发到上游，**复用 [`crate::routes::relay`] 的治理层**
//! （选号 / 在途租约 / 冷却 / 熔断 / 换号重试）—— **不另写一份轮换逻辑**
//! （`relay` 是「把请求送到上游」的唯一实现，见其模块文档）。
//!
//! ## ★ 硬约束
//!
//! - **白名单外一律 404**（[`buddy_switch_core::modules::biz_allowlist::is_allowed_path`]，**fail-closed**）——
//!   **不静默、不猜测、不「顺便也代理」**；
//! - **★ 绝不劫持**（D29）：本路由是**客户端自愿指向**的本地服务，**无 hosts / DNS / 透明代理写路径**；
//! - **body 原样透传**（业务请求不是 LLM 协议，**不解析、不改写**）；**不落盘正文**。
//!
//! ## ⚠️ 与 LLM 路由的差异（**只有三处**）
//!
//! | | LLM（`chat` / `messages`） | 业务（本路由） |
//! |:--|:---|:---|
//! | 路径 | 固定 `/v2/chat/completions` | **由 URL 的 `{path}` 决定**（已在白名单） |
//! | 头 | `build_chat_headers` | `build_auth_headers` |
//! | body | 走 `chat_body_for` 改写 | **原样透传** |
//!
//! ⇒ 其余（选号 / 换号 / 熔断 / 账本）**完全共用** —— 这正是 `RelayKind` 的用途。

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use buddy_switch_core::modules::biz_allowlist::is_allowed_path;

use crate::routes::relay::{self, RelayKind, RelayOutcome, RelayRequest};
use crate::routes::{authenticate, json_response, openai_error};
use crate::session_headers;
use crate::state::GatewayState;

/// 业务转发处理器。
pub async fn handler(
    State(state): State<GatewayState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // ① 鉴权（与 LLM 路由**同一条**：Key 绑定 region，不读客户端声明）。
    let record = match authenticate(&state, &headers) {
        Ok(record) => record,
        Err(error) => return openai_error(error),
    };

    // ② ★ 白名单校验 —— **fail-closed**：不在册一律 404（明确拒绝，不静默）。
    let full_path = format!("/{path}");
    if !is_allowed_path(&full_path) {
        return json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({
                "error": "路径不在业务白名单内",
                "path": full_path,
            }),
        );
    }

    // ③ body **原样透传**（空体合法：签到等就是 POST 空体）。
    let prepared_body = String::from_utf8_lossy(&body).to_string();

    // ④ 走 `relay`（**复用治理层**），`kind = Biz`。
    let outcome = relay::relay(
        &state,
        RelayRequest {
            region: record.region,
            kind: RelayKind::Biz {
                path: full_path.clone(),
            },
            // 业务请求没有「模型」概念：传空串 ⇒ 冷却/熔断按「无模型」维度记，**不与 LLM 混淆**。
            model: String::new(),
            prepared_body,
            conversation_request_id: session_headers::new_hex_id(),
            conversation_id: None,
            trace_id: None,
        },
    )
    .await;

    match outcome {
        Ok(RelayOutcome::Ok { response, .. }) => {
            // ⑤ **透传响应**（状态码 + 正文），**不做协议转换**。
            let status = response.status();
            let bytes = response.bytes().await.unwrap_or_default();
            Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(failure) => json_response(
            StatusCode::from_u16(failure.error.status()).unwrap_or(StatusCode::BAD_GATEWAY),
            serde_json::json!({
                "error": failure.error.openai_body(),
                "upstream": failure.upstream_message,
            }),
        ),
    }
}
