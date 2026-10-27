//! `GET /status`：账号池状态汇总（对照参考实现 `handler.go` 的 `/status`）。
//!
//! 鉴权与 OpenAI 端点一致（同一把 API Key）。响应在参考实现的形状之上追加本网关了
//! 解得到的治理参数（换号次数上限、脱敏开关、提示词模式与降级截止），
//! 便于「配置漂移」在管理面直接可见，而不用去翻配置文件。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{json, Value};

use crate::routes::{authenticate, json_response, openai_error};
use crate::state::GatewayState;
use crate::timeutil;

/// 服务标识（用于确认打到的是本网关而非别的进程）。
pub const SERVICE_NAME: &str = "buddy-switch-gateway";

/// status 处理器。
pub async fn handler(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    if let Err(error) = authenticate(&state, &headers) {
        return openai_error(error);
    }

    let now_ms = timeutil::now_ms();
    let config = state.config_snapshot().await;

    let pool_snapshot = { state.pool.read().await.snapshot(now_ms) };
    // ★ 安全审计 P2-10：`len()` 是**表大小**（含过期未 GC 的条目），
    //   这里要的是「此刻还粘着多少个会话」⇒ 用 `len_active(now_ms)`。
    let sticky_sessions = state.sticky.read().await.len_active(now_ms);
    let degrade_until_ms = state.degrade.read().await.until_ms();
    let prompt = state.prompt.read().await.clone();

    // 把池快照的键摊平到顶层，保持与参考实现同形（accounts/total/healthy/...）。
    let mut body = match pool_snapshot {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    body.insert("service".to_string(), json!(SERVICE_NAME));
    body.insert(
        "uptime_sec".to_string(),
        json!((now_ms.saturating_sub(state.started_at)) / 1000),
    );
    body.insert("sticky_sessions".to_string(), json!(sticky_sessions));
    body.insert("max_rotate".to_string(), json!(config.max_rotate));
    body.insert(
        "sanitize_fingerprints".to_string(),
        json!(config.sanitize_fingerprints),
    );
    body.insert(
        "prompt_mode".to_string(),
        json!(match prompt.mode {
            crate::outbound::PromptMode::Passthrough => "passthrough",
            crate::outbound::PromptMode::Custom => "custom",
        }),
    );
    body.insert("degrade_until_ms".to_string(), json!(degrade_until_ms));
    // 配置写错时的可见化出口：null 表示提示词加载正常。
    body.insert("prompt_error".to_string(), json!(state.prompt_error));

    json_response(StatusCode::OK, Value::Object(body))
}
