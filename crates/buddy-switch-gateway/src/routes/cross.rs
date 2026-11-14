//! 跨作用域执行：按**目标作用域**把一次请求交给对应产品线的 relay。
//!
//! ## 它解决什么
//!
//! 降级链的备选可以指向别的作用域（`wb-cn:glm` → `wb-global:glm` 或 `wb-cn` → `trae-cn`）。
//! 同平台跨区域只是「换 region 参数」，但**跨平台**要换整条 relay —— 本模块就是那层分派。
//!
//! ## 为什么能直接调各产品线的入口函数
//!
//! 四条产品线在**同一进程**里（`shared_state()` 是进程级单例），且它们的入站/出站
//! 都是 **OpenAI 形态**（`/v1/chat/completions` 进、OpenAI SSE 出）。
//! 所以跨平台降级在协议层不需要任何转换。
//!
//! ## 两条刻意的限制
//!
//! 1. **只支持 OpenAI 形态的入口**（`/v1/chat/completions`）。Anthropic / Responses
//!    入口跨平台需要**反向**流式转换（目标平台只吐 OpenAI SSE），那是另一件事。
//! 2. **不把调用方的 Key 带过去**（`key: None`）。配额与白名单在**入口**已经判过一次，
//!    再判一次会让一次降级消耗两份配额；而目标平台的账本按它自己的账号记，与入口 Key 无关。
//!
//! ## 判据的代价（必须知道）
//!
//! 目标产品线的入口返回 `Response`，拿不到「上游事件类型」⇒ 外层只能按**状态码**判
//! 是否继续降级（5xx / 429）。这与 Trae / Qoder 自身中继的判据同一档，不是新增缺陷。

use std::sync::Arc;

use axum::body::Bytes;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::Response;
use buddy_switch_core::modules::product::{Product, Scope};
use buddy_switch_core::modules::qoder::region::QoderRegion;
use buddy_switch_core::modules::trae::region::TraeRegion;
use buddy_switch_core::modules::trae::variant::{all_specs, TraeVariant};

tokio::task_local! {
    /// 跨平台降级的**嵌套深度**。
    ///
    /// ## 为什么需要它
    /// `A 入口 → cross → B 入口 → B 的降级链 → cross → A 入口 → …` 会无限递归。
    /// 编译器只能要求 `Box::pin`，管不了运行时。
    ///
    /// ## 为什么用 `task_local` 而不是参数
    /// 入口函数（`chat_for_region` / `chat_for_variant`）的签名已经很长，为这个
    /// 内部约束再加一个参数会污染四个入口与统一网关。`task_local` **跟随任务**
    /// （不是线程）—— tokio 在 await 点迁移任务时它仍然正确，这正是 `thread_local` 做不到的。
    static CROSS_DEPTH: u8;
}

/// 允许的跨平台嵌套层数。
///
/// 1 = 「A 降级到 B」可以做，但 B 内部**不能再跨平台**（B 自己的降级链只走同作用域）。
/// 这个上限把「无限递归」变成「最多两层」，同时保留双向跨平台的能力。
pub const MAX_CROSS_DEPTH: u8 = 1;

/// 当前跨平台嵌套深度（不在跨平台调用里时为 0）。
pub fn cross_depth() -> u8 {
    CROSS_DEPTH.try_with(|depth| *depth).unwrap_or(0)
}

/// 把请求交给 `target` 作用域对应的产品线，返回它的响应。
///
/// 调用方负责判断响应是否成功（以及是否继续降级）。
pub async fn relay_on_scope(
    target: Scope,
    headers: HeaderMap,
    body: Bytes,
    key: Option<Arc<crate::apikey::KeyActor>>,
) -> Response {
    // ★ 限深：已经跨出去过一次的请求，目标平台内部**不许再跨**。
    //   没有这一条，`A → B → A` 会一直递归下去（编译器只挡得住「future 大小」，
    //   挡不住运行时递归）。
    let depth = cross_depth();
    if depth >= MAX_CROSS_DEPTH {
        return depth_exceeded(target);
    }
    CROSS_DEPTH
        .scope(depth + 1, relay_on_scope_inner(target, headers, body, key))
        .await
}

async fn relay_on_scope_inner(
    target: Scope,
    headers: HeaderMap,
    body: Bytes,
    key: Option<Arc<crate::apikey::KeyActor>>,
) -> Response {
    match target.product() {
        Product::Workbuddy => {
            let state = <crate::WbGateway as crate::GatewayProduct>::shared_state();
            crate::routes::chat::chat_for_region(
                state,
                crate::model_fallback::region_of(target),
                headers,
                body,
                crate::routes::chat::PrefixPolicy::AlreadyResolved,
                key,
            )
            .await
        }
        Product::Trae => {
            let Some(variant) = trae_variant_of(target) else {
                return unsupported(target);
            };
            let state = <crate::trae::TraeGateway as crate::GatewayProduct>::shared_state();
            crate::trae::routes::chat_for_variant(state, variant, body, key).await
        }
        Product::Qoder => {
            let state = <crate::qoder::QoderGateway as crate::GatewayProduct>::shared_state();
            crate::qoder::routes::chat_for_region(
                state,
                qoder_region_of(target),
                body,
                crate::qoder::routes::Protocol::OpenAi,
                key,
                headers,
            )
            .await
        }
    }
}

/// 该作用域能不能作为**跨作用域**的降级目标（当前进程里有没有这条产品线）。
///
/// 只判「变体存在」这类**结构性**问题；账号有没有、模型有没有由各产品线自己回答
/// （它们会给出比这里更准的错误）。
pub fn is_reachable(target: Scope) -> bool {
    match target.product() {
        Product::Workbuddy | Product::Qoder => true,
        Product::Trae => trae_variant_of(target).is_some(),
    }
}

/// 作用域 → Trae 的**合并版**变体（按区域）。
///
/// 取 `all_specs()`（合并版两条）而不是 `all_known_specs()`（含退役变体）：
/// 跨作用域降级要的是「这个区域当前在用的那一个」，不是历史变体。
pub fn trae_variant_of(scope: Scope) -> Option<TraeVariant> {
    let region = if scope.as_str().ends_with("-global") {
        TraeRegion::Global
    } else {
        TraeRegion::Cn
    };
    all_specs()
        .iter()
        .find(|spec| spec.variant.region() == region)
        .map(|spec| spec.variant)
}

pub fn qoder_region_of(scope: Scope) -> QoderRegion {
    if scope.as_str().ends_with("-global") {
        QoderRegion::Global
    } else {
        QoderRegion::Cn
    }
}

/// 跨平台嵌套超限（`A → B → A`）。
///
/// 返回 502 而不是 400：这是**网关自己的**能力边界，不是客户端请求有问题。
fn depth_exceeded(target: Scope) -> Response {
    use axum::response::IntoResponse;
    (
        StatusCode::BAD_GATEWAY,
        axum::Json(serde_json::json!({
            "error": {
                "message": format!(
                    "降级目标 {} 需要再次跨平台，超过允许的嵌套层数（{}）",
                    target.as_str(),
                    MAX_CROSS_DEPTH
                ),
                "type": "server_error",
                "code": "fallback_cross_depth_exceeded",
            }
        })),
    )
        .into_response()
}

fn unsupported(target: Scope) -> Response {
    use axum::response::IntoResponse;
    (
        StatusCode::BAD_REQUEST,
        axum::Json(serde_json::json!({
            "error": {
                "message": format!(
                    "降级目标 {} 不可用：该平台的变体未在当前构建中启用",
                    target.as_str()
                ),
                "type": "invalid_request_error",
                "code": "fallback_target_unavailable",
            }
        })),
    )
        .into_response()
}
