//! `GET /v1/models`：按 Key 绑定 region 返回模型列表 + 来源标注（P0-4）。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::json;

use crate::disabled_models::is_disabled;
use crate::routes::{authenticate, json_response, openai_error};
use crate::state::GatewayState;

/// 模型列表处理器。
pub async fn handler(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    let record = match authenticate(&state, &headers) {
        Ok(record) => record,
        Err(error) => return openai_error(error),
    };

    let snapshot = state.catalogs.current(record.region);
    // ★ 出口 ①（P11）：语义是「**可用的**模型清单」——不可服务与被用户禁用的都**不出现**。
    //   WB 侧 `available` 恒 true（无「非可服务」概念，故此处无 no-op 过滤）；
    //   禁用过滤读的是**与出口 ② 同一份** `disabled`（`is_disabled` 是唯一判断点）。
    let disabled = state.disabled.read().await;
    let data: Vec<serde_json::Value> = snapshot
        .models
        .iter()
        .filter(|model| !is_disabled(&disabled, record.region, &model.id))
        .map(|model| {
            json!({
                "id": model.id,
                "object": "model",
                "created": 0,
                "owned_by": "workbuddy",
                "context_window": model.context_window,
                "max_tokens": model.max_tokens,
                "supports_images": model.supports_images,
                "credits": model.credits,
                "badges": model.badges,
                "free": model.free,
            })
        })
        .collect();
    drop(disabled);

    json_response(
        StatusCode::OK,
        json!({
            "object": "list",
            "data": data,
            "wb_region": snapshot.region,
            "wb_source": snapshot.source,
            "wb_fetched_at": snapshot.fetched_at,
            "wb_note": snapshot.note,
        }),
    )
}
