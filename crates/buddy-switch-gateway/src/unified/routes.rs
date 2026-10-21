//! 统一网关的处理器（当前挂 `/health`（兼容 `/healthz`）· `/status` · `/v1/models`）。
//!
//! # 不合并 router，只合并**入口**
//!
//! 两个既有网关的 `/v1/models`、`/v1/chat/completions` **逐字相同**，axum 同路径 `merge`
//! 会**运行期 panic**。故统一网关是**第三个独立端口上的 `GatewayProduct`**，
//! 自己挂自己的路径（见 [`super::UnifiedGateway::OWNED_PATHS`]）。
//!
//! # 聚合清单：为什么先做 `/v1/models`
//!
//! 它**不需要**动两条既有中继（不需要 P3 的 `relay_*` 抽取），却能完整验证命名空间设计：
//! 四段并列、id 带前缀、**每段单独标来源与原因**。「列得出来」先成立，「调得动」随后接。
//!
//! 未挂的业务端点（`/v1/chat/completions` / `/v1/messages` / `/v1/responses` / `/biz/*path`）
//! 要等 P3 抽 `pub(crate) relay_*` —— 见 `docs/specs/UNIFIED-GATEWAY-ARCH.md` §7。

use crate::GatewayProduct;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use buddy_switch_core::modules::catalog::CatalogSource;
use buddy_switch_core::modules::gateway_namespace::{self, NamespaceSpec, RelayTarget};

use super::apikey::UnifiedApiKeyRecord;
use super::config::{known_ids, UnifiedGatewayConfig};
use super::state::UnifiedGatewayState;
use crate::disabled_models::is_disabled;
use crate::routes::json_response;

/// `GET /health`：存活探针，**免鉴权**（与 WB 网关同名同义；`/healthz` 是兼容别名，
/// 两个路径由 [`crate::health_routes`] 挂到**同一个**处理器上）。
pub async fn health(State(state): State<UnifiedGatewayState>) -> Response {
    let config = state.config_snapshot().await;
    let keys = state.keys.list();
    json_response(
        StatusCode::OK,
        json!({
            "status": "ok",
            "service": "unified-gateway",
            "running": true,
            "enabled": config.enabled,
            "port": config.port,
            "namespaces": config.enabled_namespaces().iter().map(|s| s.id).collect::<Vec<_>>(),
            "keys": { "total": keys.len(), "active": keys.iter().filter(|k| k.is_active()).count() },
        }),
    )
}

/// `GET /status`：需鉴权（与两条既有网关一致 —— 状态里含端口/命名空间等部署信息）。
pub async fn status(State(state): State<UnifiedGatewayState>, headers: HeaderMap) -> Response {
    let record = match authenticate(&state, &headers).await {
        Ok(record) => record,
        Err(error) => return error,
    };
    let config = state.config_snapshot().await;
    // 只列**本 Key 可用**的命名空间（未授权的对它等于不存在）
    let available: Vec<&str> = config
        .enabled_namespaces()
        .iter()
        .filter(|spec| record.authorizes(spec.id))
        .map(|spec| spec.id)
        .collect();
    json_response(
        StatusCode::OK,
        json!({
            "status": "ok",
            "running": true,
            "started_at": state.started_at,
            "port": config.port,
            "bind_addr": config.bind_addr,
            "allow_non_loopback": config.allow_non_loopback,
            "default_namespace": config.default_namespace,
            "namespaces": available,
            "key": { "name": record.name, "prefix": record.prefix, "suffix": record.suffix },
        }),
    )
}

/// `GET /v1/models`：**聚合**该 Key 已授权且已启用的命名空间下的模型清单。
///
/// - id 一律带命名空间前缀（`wb-cn:deepseek-v4.1-flash`）⇒ 客户端**自动发现即可用**，
///   且同名模型不会再互相覆盖；
/// - 顶层 `namespaces` **逐段**给出 `source` / `fetched_at` / `note` ——
///   沿用「非实时必有原因」契约，一个总徽标糊过去是不允许的。
pub async fn models(State(state): State<UnifiedGatewayState>, headers: HeaderMap) -> Response {
    let record = match authenticate(&state, &headers).await {
        Ok(record) => record,
        Err(error) => return error,
    };
    let config = state.config_snapshot().await;
    let specs: Vec<&'static NamespaceSpec> = config
        .enabled_namespaces()
        .into_iter()
        .filter(|spec| record.authorizes(spec.id))
        .collect();
    // 监听端口**不触发重识别**（`refresh = false`）：它给客户端用，不该由一次拉清单
    // 触发上游探测。「重新识别」只在管理面（`models_preview(true)`）。
    let entries = collect_namespace_models(&specs, false).await;
    json_response(StatusCode::OK, build_models_response(&entries))
}

/// 一次可诊断的请求错误（**协议无关** —— 出站时再按端点选形状）。
struct HttpError {
    status: StatusCode,
    /// 标准错误类型（`invalid_request_error` / `permission_error` / `authentication_error`）。
    kind: &'static str,
    /// 机器可读的具体代码（如 `namespace_prefix_required`），保留下来便于用户/日志精确定位。
    code: &'static str,
    message: String,
}

/// 三个 LLM 端点 —— 差异**只有两处**。
///
/// | 端点 | 错误体形状 | Trae 是否支持 |
/// |:--|:--|:--|
/// | `/v1/chat/completions` | OpenAI | ✅ |
/// | `/v1/messages` | Anthropic | ✅ |
/// | `/v1/responses` | OpenAI | ❌ **设计如此**（Trae 分区没有这个端点） |
///
/// 其余（鉴权 / 命名空间解析 / 启用与授权 / 改写 body / 分派）**三条共用一份** ——
/// 复制三遍必然漂移，而漂移点恰恰是「哪种情况该 400、哪种该 403」这类安全相关判定。
#[derive(Clone, Copy, PartialEq, Eq)]
enum LlmEndpoint {
    Chat,
    Messages,
    Responses,
}

impl LlmEndpoint {
    /// 该端点在 Trae 分区是否可用。`/v1/responses` 只有 WorkBuddy 与 Qoder 有。
    fn supports_trae(self) -> bool {
        !matches!(self, LlmEndpoint::Responses)
    }
}

/// `POST /v1/chat/completions`。
pub async fn chat_completions(
    State(state): State<UnifiedGatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    relay_llm(state, headers, body, LlmEndpoint::Chat).await
}

/// `POST /v1/messages`（Anthropic 协议 —— 错误体也是 Anthropic 形状）。
pub async fn messages(
    State(state): State<UnifiedGatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    relay_llm(state, headers, body, LlmEndpoint::Messages).await
}

/// `POST /v1/responses`（OpenAI Responses API）。⚠️ Trae 命名空间在本端点会被**明确拒绝**。
pub async fn responses(
    State(state): State<UnifiedGatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    relay_llm(state, headers, body, LlmEndpoint::Responses).await
}

/// 三个 LLM 端点的**唯一分派实现**：鉴权 → 取模型名 → 命名空间把关 → 改写 body → 交给既有中继。
///
/// ★ 这里**没有一行转发实现**：「把请求可靠地送到上游」唯一实现在 `relay`；
/// 它外面那层（改写 / 停用 / 选号换号 / 降级重试 / 流式）唯一实现在
/// `chat_for_region` · `messages_for_region` · `responses_for_region` · `chat_for_variant` ·
/// `messages_for_variant`。统一入口一行都不复制。
async fn relay_llm(
    state: UnifiedGatewayState,
    headers: HeaderMap,
    body: Bytes,
    endpoint: LlmEndpoint,
) -> Response {
    let record = match authenticate(&state, &headers).await {
        Ok(record) => record,
        Err(error) => return error,
    };
    let config = state.config_snapshot().await;

    // ① 取模型名（解析失败 / 缺字段 ⇒ 与两条既有网关同款的 400，别让它走进中继）
    let raw = String::from_utf8_lossy(&body).to_string();
    let requested = match requested_model_of(&raw) {
        Ok(model) => model,
        Err(error) => return endpoint_error(endpoint, error),
    };

    // ② 命名空间把关（**唯一实现**：解析 → 非空 → 启用 → 授权）
    let (spec, bare) = match guard_namespace(&config, &record, &requested) {
        Ok(pair) => pair,
        Err(error) => return endpoint_error(endpoint, error),
    };

    // ③ ★ `/v1/responses` 在 Trae 分区**不存在** —— 明确报错。
    //    不许 404（用户会以为路径写错了）、更不许静默降级到 chat（用户会以为 Responses 生效了）。
    if !endpoint.supports_trae() && gateway_namespace::trae_variant_of(spec).is_some() {
        return endpoint_error(endpoint, endpoint_not_supported_for(spec.id));
    }

    // ④ 把 body 里的 `model` 改写成**裸名**（中继与「模型禁用」判定都用裸名；
    //    留着前缀会让上游看到 `wb-cn:deepseek-…` 这种它不认识的模型名）
    let rewritten = Bytes::from(crate::model_route::rewrite_body_model(&raw, bare));

    match gateway_namespace::relay_target_of(spec) {
        RelayTarget::Wb(region) => {
            use crate::routes::chat::PrefixPolicy::AlreadyResolved;
            let wb = <crate::WbGateway as crate::GatewayProduct>::shared_state();
            match endpoint {
                LlmEndpoint::Chat => {
                    crate::routes::chat::chat_for_region(
                        wb,
                        region,
                        headers,
                        rewritten,
                        AlreadyResolved,
                    )
                    .await
                }
                LlmEndpoint::Messages => {
                    crate::routes::messages::messages_for_region(
                        wb,
                        region,
                        headers,
                        rewritten,
                        AlreadyResolved,
                    )
                    .await
                }
                LlmEndpoint::Responses => {
                    crate::routes::responses::responses_for_region(
                        wb,
                        region,
                        headers,
                        rewritten,
                        AlreadyResolved,
                    )
                    .await
                }
            }
        }
        RelayTarget::Trae(variant) => {
            let trae = <crate::trae::TraeGateway as crate::GatewayProduct>::shared_state();
            match endpoint {
                LlmEndpoint::Chat => {
                    crate::trae::routes::chat_for_variant(trae, variant, rewritten).await
                }
                LlmEndpoint::Messages => {
                    crate::trae::routes::messages_for_variant(trae, variant, rewritten).await
                }
                // 防御性兜底（上面第 ③ 步已拦）：**穷举、无 `_ =>`** ⇒ 新增端点时编译失败，
                // 而不是静默走到别的分派分支。这里返回同一份可诊断错误，不 panic。
                LlmEndpoint::Responses => {
                    endpoint_error(endpoint, endpoint_not_supported_for(spec.id))
                }
            }
        }
        /* ★ Qoder 产品线（2026-10-15 接入）：与 WB / Trae 同一条中继纪律 ——
        复用 `qoder::routes::chat_for_region`（**认证之后的逻辑只有一份**），
        绝不在这里另写一套选号 / 换号 / 流式转发。 */
        RelayTarget::Qoder(region) => {
            let qoder = <crate::qoder::QoderGateway as crate::GatewayProduct>::shared_state();
            match endpoint {
                LlmEndpoint::Chat => {
                    crate::qoder::routes::chat_for_region(
                        qoder,
                        region,
                        rewritten,
                        crate::qoder::routes::Protocol::OpenAi,
                    )
                    .await
                }
                LlmEndpoint::Messages => {
                    crate::qoder::routes::chat_for_region(
                        qoder,
                        region,
                        rewritten,
                        crate::qoder::routes::Protocol::Anthropic,
                    )
                    .await
                }
                // `/v1/responses` 对 Qoder 分区同样成立（入站翻译成 chat、出站转 Responses
                // 事件流/对象，见 `qoder::routes::Protocol::Responses`）。
                LlmEndpoint::Responses => {
                    crate::qoder::routes::chat_for_region(
                        qoder,
                        region,
                        rewritten,
                        crate::qoder::routes::Protocol::Responses,
                    )
                    .await
                }
            }
        }
    }
}

/// `/v1/responses` + **Trae** 命名空间的可诊断错误（`endpoint_not_supported_for_namespace`
/// 的**唯一构造点**；Qoder 分区已支持该端点，不在这里）。
fn endpoint_not_supported_for(namespace: &str) -> HttpError {
    HttpError {
        status: StatusCode::NOT_IMPLEMENTED,
        kind: "invalid_request_error",
        code: "endpoint_not_supported_for_namespace",
        message: format!(
            "Trae 分区没有 /v1/responses 端点（WorkBuddy 与 Qoder 有），而 `{namespace}` 是 Trae 命名空间。\
             请改用 /v1/chat/completions 或 /v1/messages，或把模型前缀换成 wb-cn: / wb-global: / qoder-cn: 等。"
        ),
    }
}

/// 从请求体取 `model`（解析失败 ⇒ 400 的**唯一实现**）。
///
/// 返回的可能为空串（如 `{"model":""}`）—— 那由 [`guard_namespace`] 报
/// `namespace_prefix_required`，**不在这里**判，避免两处各判一套。
fn requested_model_of(raw: &str) -> Result<String, HttpError> {
    let parsed: Value = serde_json::from_str(raw).map_err(|_| HttpError {
        status: StatusCode::BAD_REQUEST,
        kind: "invalid_request_error",
        code: "invalid_json",
        message: "请求体不是合法 JSON。".to_string(),
    })?;
    Ok(parsed
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}

/// 命名空间把关 —— **鉴权之后、分派之前的唯一实现**。
///
/// 四道，顺序**不能换**：
/// 1. **必须在册**（整段精确匹配，禁前缀推断）⇒ 否则 400 并列出可用前缀；
/// 2. 前缀后不得为空（`wb-cn:` 是无效请求）；
/// 3. **本入口启用**（400）—— 与 Key 无关，换一把 Key 也没用，故**先于**授权判定；
/// 4. **该 Key 已授权**（403）。
///
/// 返回 `(spec, 裸模型名)`。
fn guard_namespace<'a>(
    config: &UnifiedGatewayConfig,
    record: &UnifiedApiKeyRecord,
    requested: &'a str,
) -> Result<(&'static NamespaceSpec, &'a str), HttpError> {
    let split = gateway_namespace::split(requested);
    let Some(spec) = split.spec else {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            kind: "invalid_request_error",
            code: "namespace_prefix_required",
            message: format!(
                "统一网关要求模型名带命名空间前缀。本入口启用的命名空间：{}。\
                 你填的是 `{requested}`，请改成 `{}:…` 这类形式（用不了的命名空间会被明确拒绝，不会静默改路由）。",
                enabled_prefixes(config),
                config
                    .enabled_namespaces()
                    .first()
                    .map(|spec| spec.id)
                    .unwrap_or("wb-cn")
            ),
        });
    };
    if split.bare.trim().is_empty() {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            kind: "invalid_request_error",
            code: "empty_model_name",
            message: format!("命名空间 `{}:` 后面缺少模型名。", spec.id),
        });
    }
    if !config.is_enabled(spec.id) {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            kind: "invalid_request_error",
            code: "namespace_not_enabled",
            message: format!(
                "命名空间 `{}` 未在本入口启用（统一网关设置里的「启用的命名空间」不含它）。",
                spec.id
            ),
        });
    }
    if !record.authorizes(spec.id) {
        return Err(HttpError {
            status: StatusCode::FORBIDDEN,
            kind: "permission_error",
            code: "namespace_forbidden",
            message: format!(
                "该 API Key 未被授权使用命名空间 `{}`（它只授权了 {}）。",
                spec.id,
                if record.namespaces.is_empty() {
                    "（无）".to_string()
                } else {
                    record.namespaces.join("、")
                }
            ),
        });
    }
    Ok((spec, split.bare))
}

/// `POST /biz/*path`：**业务转发**（客户端把 base URL 指向本机时，本该发给上游的业务请求）。
///
/// # 命名空间从哪来（与三个 LLM 端点**不同**）
///
/// 业务请求**没有模型名** ⇒ 命名空间只能来自**统一 Key 的 `biz_namespace`**。理由：
/// - 不采用请求头：真实调用方是**客户端自身**，它加不了自定义头；
/// - 不隐式取「第一个 wb-*」：那与「裸模型名一律拒绝」的哲学相反（**不猜**）。
///
/// 白名单校验 / fail-closed 404 由 [`crate::routes::biz::biz_for_region`] 负责（既有唯一实现）。
pub async fn biz(
    State(state): State<UnifiedGatewayState>,
    Path(path): Path<String>,
    method: axum::http::Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let record = match authenticate(&state, &headers).await {
        Ok(record) => record,
        Err(error) => return error,
    };

    // ① 该 Key 必须显式指定业务转发命名空间
    let Some(namespace) = record.biz_namespace.clone() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "biz_namespace_missing",
            format!(
                "该 API Key（「{}」）未指定业务转发命名空间，因此不能用 `/biz/*`。\
                 请到「统一入口 → 密钥」为它选择（业务转发只有 WorkBuddy 命名空间合法）。",
                record.name
            ),
        );
    };

    // ② 三道：在册 → 启用 → 授权。
    //    ⚠️ 「在册」虽然写 Key 时已校验过，但**手改台账能绕过** ⇒ 读取侧必须再查一遍
    //    （校验只在写入路径，不能当成读取路径的前提）。
    let Some(spec) = gateway_namespace::parse(&namespace) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "biz_namespace_unknown",
            format!(
                "该 API Key 指定的业务转发命名空间 `{namespace}` 不在册（改过台账？）。当前在册：{}",
                known_ids().join(" / ")
            ),
        );
    };
    let config = state.config_snapshot().await;
    if !config.is_enabled(spec.id) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "namespace_not_enabled",
            format!(
                "该 Key 的业务转发命名空间 `{}` 未在本入口启用（统一网关设置里的「启用的命名空间」不含它）。",
                spec.id
            ),
        );
    }
    if !record.authorizes(spec.id) {
        return error_response(
            StatusCode::FORBIDDEN,
            "permission_error",
            "namespace_forbidden",
            format!(
                "该 API Key 未被授权使用命名空间 `{}`（它只授权了 {}）。",
                spec.id,
                if record.namespaces.is_empty() {
                    "（无）".to_string()
                } else {
                    record.namespaces.join("、")
                }
            ),
        );
    }

    // ③ 业务转发**只有 WorkBuddy 有这条通道**（Trae 业务域不可配）
    let Some(region) = gateway_namespace::wb_region_of(spec) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "biz_namespace_not_workbuddy",
            format!(
                "业务转发只有 WorkBuddy 命名空间可用，而该 Key 指定的是 `{}`。请改选 wb-cn / wb-global。",
                spec.id
            ),
        );
    };

    // ④ 出站方法：**透传客户端的原方法**（白名单里 GET-only 与 POST-only 混在一起，
    //    一律 POST 出去会让 GET-only 的路径被上游回 `404 page not found`）。
    let Some(biz_method) = crate::routes::biz::biz_method_of(&method) else {
        return error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "invalid_request_error",
            "biz_method_not_allowed",
            format!("业务转发只接受 GET / POST，收到 {}。", method.as_str()),
        );
    };

    // ⑤ 分派（复用既有的白名单 + `relay` 治理层，**不另写轮换逻辑**）
    let wb = <crate::WbGateway as crate::GatewayProduct>::shared_state();
    crate::routes::biz::biz_for_region(wb, region, path, body, biz_method).await
}

/// 按端点选**错误体形状**：`/v1/messages` 用 Anthropic 形状，其余用 OpenAI 形状。
///
/// Anthropic 形状里保留一个非标准的 `code` 字段：Anthropic 客户端会忽略未知字段，
/// 而用户/日志能拿到精确代码 —— 只给一个笼统的 `invalid_request_error` 回答不了「怎么办」。
fn endpoint_error(endpoint: LlmEndpoint, error: HttpError) -> Response {
    if endpoint == LlmEndpoint::Messages {
        return (
            error.status,
            axum::Json(json!({
                "type": "error",
                "error": {
                    "type": error.kind,
                    "code": error.code,
                    "message": error.message,
                }
            })),
        )
            .into_response();
    }
    error_response(error.status, error.kind, error.code, error.message)
}

/// 本入口已启用的命名空间前缀列表（错误文案用，避免用户对着空白猜）。
fn enabled_prefixes(config: &UnifiedGatewayConfig) -> String {
    config
        .enabled_namespaces()
        .iter()
        .map(|spec| format!("{}:", spec.id))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// **管理面专用**：把「本入口已启用的命名空间 → 该段清单」聚合成 `/v1/models` 的形状。
///
/// ## ⚠️ 与 `/v1/models` 的差别，以及为什么它**不能挂在监听端口上**
///
/// 监听端口上的 `/v1/models` 会做 **Key 授权过滤**（只列这把 Key 能用的段）；
/// 本函数**不做** —— 管理面要看的是「本入口启用了哪些、每段现在什么状态」，
/// 与「某一把握钥的权限」无关。
/// ⇒ **它绕过鉴权**（不需要 Key），因此**只允许两处宿主的管理面调用**，
/// **绝不注册进 [`super::router`]**（那条由 e2e 用例逐条钉住实际挂载集合来守）。
///
/// ## `refresh = true`（管理面「重新识别全部」）
///
/// 绕过各产品线的**识别缓存**重取一次：
/// - WorkBuddy：走 [`crate::catalog_probe::probe_region`]（全仓**唯一**探测入口）——
///   重新向上游识别并写回目录缓存；失败静默（下面读到的仍是缓存 / 内置快照，
///   具体原因由 `CatalogStore::refresh` 写进 `note`）；
/// - Qoder：走 [`crate::qoder::QoderGatewayState::refresh_catalog`]（忽略 TTL 缓存）；
/// - Trae：**无缓存可绕** —— 清单直接读客户端文件（`read_client_model_list`），
///   本次读到的就是客户端那份最新的。
///
/// ⚠️ 监听端口上的 `/v1/models` **不走这条路**（`refresh = false`）：它给客户端用。
pub async fn models_preview(refresh: bool) -> Value {
    let config = super::state::shared_state().config_snapshot().await;
    let specs: Vec<&'static NamespaceSpec> = config.enabled_namespaces();
    let entries = collect_namespace_models(&specs, refresh).await;
    build_models_response(&entries)
}

/// 一条命名空间的清单视图（`build_models_response` 的输入）。
#[derive(Debug, Clone)]
pub struct NamespaceModels {
    /// 命名空间 id。
    pub namespace: &'static str,
    /// 来源：`live` / `cached` / `builtin`（与两个既有网关、以及模型页**同一套词**）。
    pub source: &'static str,
    /// 识别时间（毫秒）；`builtin` 为 `None`。
    pub fetched_at: Option<i64>,
    /// **非实时必有原因**（契约：非 live ⇒ note 非空）。
    pub note: Option<String>,
    /// 该段的模型对象（`id` 为**裸名**，前缀由本函数加）。
    pub models: Vec<Value>,
}

/// 组装 `/v1/models` 响应体（**纯函数** —— 不起 HTTP 就能测）。
///
/// 做两件事：① 把每个模型的 `id` 改写成 `<命名空间>:<裸名>` 并补 `namespace` 字段；
/// ② 顶层给出**逐段**的来源与原因。
///
/// ⚠️ **不改写裸名以外的字段**：上游的 `context_window` / `credits` / `badges` 等原样透出，
/// 否则两段同名模型的真实差异（倍率、上下文窗口都不同）就看不见了。
pub fn build_models_response(entries: &[NamespaceModels]) -> Value {
    let mut data: Vec<Value> = Vec::new();
    let mut namespaces = serde_json::Map::new();
    for entry in entries {
        for model in &entry.models {
            let mut model = model.clone();
            if let Some(object) = model.as_object_mut() {
                let bare = object
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                object.insert(
                    "id".to_string(),
                    Value::String(gateway_namespace::compose(entry.namespace, bare)),
                );
                object.insert(
                    "namespace".to_string(),
                    Value::String(entry.namespace.to_string()),
                );
            }
            data.push(model);
        }
        namespaces.insert(
            entry.namespace.to_string(),
            json!({
                "source": entry.source,
                "fetched_at": entry.fetched_at,
                "note": entry.note,
            }),
        );
    }
    json!({ "object": "list", "data": data, "namespaces": Value::Object(namespaces) })
}

/// 逐命名空间取清单（WB 走目录缓存，Trae 走客户端清单）。
///
/// ★ 两个产品线的状态都是**进程内单例**，此处直接借用 ⇒ 「三个入口看到同一份池/清单」
/// 是结构性成立的，不靠同步逻辑维持。
///
/// `refresh = true` ⇒ 先绕过各产品线的识别缓存重取（WB 走唯一探测入口、Qoder 忽略 TTL），
/// 再按原路径读取；Trae 无缓存（直读客户端文件）。失败一律**静默降级**到缓存 / 内置，
/// 原因由各产品线自己的 `note` 如实给出（契约：非 `live` ⇒ note 非空）。
async fn collect_namespace_models(
    specs: &[&'static NamespaceSpec],
    refresh: bool,
) -> Vec<NamespaceModels> {
    if specs.is_empty() {
        return Vec::new();
    }
    let wb = <crate::WbGateway as crate::GatewayProduct>::shared_state();
    let trae = <crate::trae::TraeGateway as crate::GatewayProduct>::shared_state();
    // ⚠️ **取快照，不跨 await 持读锁**（本仓既有纪律）：循环里的 WB 探测 / Qoder 拉清单
    //    都是网络 IO，持锁会卡住「禁用模型」这类写操作（`set_model_disabled` 要写锁）。
    let wb_disabled = wb.disabled.read().await.clone();
    let trae_disabled = trae.disabled.read().await.clone();

    /* ⚠️ `Qoder` 分支需要 async 取数（清单要经账号凭据向官方拉取，带缓存）⇒
    这里用 for 循环逐项 await（`.map` 的闭包不是 async）。顺序保持不变。 */
    let mut collected: Vec<NamespaceModels> = Vec::with_capacity(specs.len());
    for spec in specs.iter() {
        collected.push(match gateway_namespace::relay_target_of(spec) {
            RelayTarget::Wb(region) => {
                if refresh {
                    // 管理面「重新识别」：唯一探测入口（内部只用该 region 自己的凭据）。
                    // 失败静默 —— 下面读到的仍是缓存 / 内置快照，并把原因写进 `note`。
                    let _ = crate::catalog_probe::probe_region(&wb, region).await;
                }
                let snapshot = wb.catalogs.current(region);
                let wb_max_context = wb.config.read().await.max_context;
                let models = snapshot
                    .models
                    .iter()
                    // ★ 出口 ① 与调用路径读**同一份** `disabled`（`is_disabled` 是唯一判断点）
                    .filter(|model| {
                        !is_disabled(
                            &wb_disabled,
                            buddy_switch_core::modules::product::scope_of_wb_region(region),
                            &model.id,
                        )
                    })
                    .map(|model| {
                        let mut entry = crate::routes::models::model_entry(model, wb_max_context);
                        if let Some(object) = entry.as_object_mut() {
                            object.insert("namespace".into(), json!(spec.id));
                        }
                        entry
                    })
                    .collect();
                NamespaceModels {
                    namespace: spec.id,
                    source: catalog_source_str(snapshot.source),
                    fetched_at: snapshot.fetched_at,
                    note: snapshot.note.clone(),
                    models,
                }
            }
            RelayTarget::Trae(variant) => {
                // 官方目录（逐模型 dev/max）：过期先同步；失败静默沿用旧缓存。
                crate::trae::catalog::ensure_fresh(variant).await;
                let list =
                    buddy_switch_core::modules::trae::model_list::read_client_model_list(variant);
                // 客户端拉过清单 ⇒ 视作「实时」（它的新鲜度 = 客户端最后一次刷新）
                let from_client = !list.groups.is_empty();
                let max_mode = crate::trae::TraeGateway::shared_state().max_mode_now();
                let models = crate::trae::payload::models_response_for(
                    variant,
                    trae_disabled.get(buddy_switch_core::modules::product::scope_of_trae_region(
                        variant.region(),
                    )),
                    max_mode,
                )
                .get("data")
                .and_then(|data| data.as_array())
                .cloned()
                .unwrap_or_default();
                NamespaceModels {
                    namespace: spec.id,
                    source: if from_client { "live" } else { "builtin" },
                    fetched_at: Some(list.read_at),
                    note: if from_client {
                        None
                    } else {
                        Some(
                            "客户端未拉取过模型清单（Trae 需至少启动过一次），展示的是静态兜底清单"
                                .to_string(),
                        )
                    },
                    models,
                }
            }
            /* Qoder 产品线（2026-10-15 接入）：清单来自官方接口（需账号凭据），
            带缓存；拉不到时**空清单 + 可读 note**（契约：非 `live` ⇒ note 非空）。 */
            RelayTarget::Qoder(region) => {
                let qoder = <crate::qoder::QoderGateway as crate::GatewayProduct>::shared_state();
                // `refresh` ⇒ 忽略 TTL 强制重拉（失败仍退回上次成功的结果 + note）。
                let catalog = if refresh {
                    qoder.refresh_catalog(region).await
                } else {
                    qoder.catalog_for(region).await
                };
                let scope = buddy_switch_core::modules::product::scope_of_qoder_region(region);
                let disabled_store = crate::disabled_models::shared();
                let disabled = disabled_store.read().await;
                let models = crate::qoder::payload::models_response(
                    &catalog.models,
                    disabled.get(scope),
                    qoder.max_context_now(),
                )
                .get("data")
                .and_then(|data| data.as_array())
                .cloned()
                .unwrap_or_default();
                drop(disabled);
                NamespaceModels {
                    namespace: spec.id,
                    source: if catalog.models.is_empty() {
                        "unavailable"
                    } else {
                        "live"
                    },
                    fetched_at: Some(buddy_switch_core::modules::config::now_ms()),
                    note: catalog.note,
                    models,
                }
            }
        });
    }
    collected
}

/* `qoder_not_wired` 已于 2026-10-15 删除：Qoder 产品线正式接入统一网关，
501/`namespace_product_line_not_wired` 不再是任何路径的出口（不保留死代码）。 */

/// 目录来源 → 稳定字符串（供 `/v1/models` 的 `namespaces[].source`）。
///
/// **穷举、无 `_ =>`**：`CatalogSource` 新增取值时编译失败，而不是静默透出一个陌生来源名。
fn catalog_source_str(source: CatalogSource) -> &'static str {
    match source {
        CatalogSource::Live => "live",
        CatalogSource::Cached => "cached",
        CatalogSource::Builtin => "builtin",
    }
}

/// 从请求头取明文 Key：**`x-api-key` 优先，其次 `Authorization: Bearer`**。
///
/// 与 WB 网关同一规则（Trae 侧只认 Bearer）。统一入口**接受两者** —— 它是一个新入口，
/// 不继承任何一侧的历史包袱；但**绝不按认证头反推产品线**（归属只由命名空间决定）。
fn presented_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(value) = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value);
        }
    }
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let raw = raw.trim();
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[allow(clippy::result_large_err)]
// `RelayFailure` 承载完整上游响应与诊断上下文，装箱会丢掉「一次就能定位」的可读性。
/// 鉴权：失败时返回**已经构造好**的错误响应（调用方 `return` 即可）。
async fn authenticate(
    state: &UnifiedGatewayState,
    headers: &HeaderMap,
) -> Result<UnifiedApiKeyRecord, Response> {
    let Some(presented) = presented_key(headers) else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_request_error",
            "missing_credentials",
            "缺少凭据：请用 `Authorization: Bearer sk-uni-…` 或 `x-api-key: sk-uni-…` 提供统一网关的密钥。",
        ));
    };
    match state.keys.verify(presented) {
        Some(record) => {
            state.keys.touch(&record.id);
            Ok(record)
        }
        // 无效 / 已吊销 / 拿的是两个既有网关的 Key —— 文案要覆盖最后这种最常见的误用
        None => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_request_error",
            "invalid_api_key",
            "密钥无效或已吊销。注意：统一入口用**独立的**密钥库（`sk-uni-` 开头）——\
             两个既有网关的 `sk-wb-` / `sk-trae-` 密钥不能在这里使用。",
        )),
    }
}

/// OpenAI 形状的错误体（与两条既有网关**同形**，让同一个客户端三处都能读懂）。
fn error_response(
    status: StatusCode,
    kind: &str,
    code: &str,
    message: impl Into<String>,
) -> Response {
    (
        status,
        axum::Json(json!({
            "error": { "message": message.into(), "type": kind, "code": code }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::UnifiedApiKeyStore;

    fn entry(namespace: &'static str, source: &'static str, models: &[&str]) -> NamespaceModels {
        NamespaceModels {
            namespace,
            source,
            fetched_at: Some(1_760_400_000_000),
            note: None,
            models: models
                .iter()
                .map(|id| json!({ "id": id, "object": "model" }))
                .collect(),
        }
    }

    /// ★ id 必须被改写成 `<命名空间>:<裸名>`，且补上 `namespace` 字段。
    #[test]
    fn ids_are_prefixed_and_tagged() {
        let response = build_models_response(&[entry("wb-cn", "live", &["deepseek-v4.1-flash"])]);
        assert_eq!(response["object"], "list");
        assert_eq!(response["data"][0]["id"], "wb-cn:deepseek-v4.1-flash");
        assert_eq!(response["data"][0]["namespace"], "wb-cn");
        assert_eq!(
            response["data"][0]["object"], "model",
            "其余字段必须原样保留"
        );
    }

    /// ★★ 同名模型来自两个命名空间 ⇒ **两条并列**，正是本需求要的结果（不再互相覆盖）。
    #[test]
    fn the_same_model_from_two_namespaces_coexists() {
        let response = build_models_response(&[
            entry("wb-cn", "live", &["deepseek-v4.1-flash"]),
            entry("wb-global", "cached", &["deepseek-v4.1-flash"]),
        ]);
        let ids: Vec<&str> = response["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["wb-cn:deepseek-v4.1-flash", "wb-global:deepseek-v4.1-flash"]
        );
        assert_ne!(ids[0], ids[1], "两条 id 必须不同，否则客户端仍会按 id 去重");
    }

    /// ★ 逐段来源与原因：顶层 `namespaces` 必须**每段一条**，未实时的那段 note 非空。
    #[test]
    fn each_namespace_carries_its_own_source_and_reason() {
        let with_reason = NamespaceModels {
            namespace: "trae-cn",
            source: "builtin",
            fetched_at: None,
            note: Some("客户端未拉取过模型清单".to_string()),
            models: vec![],
        };
        let response = build_models_response(&[
            entry("wb-cn", "live", &["a"]),
            entry("wb-global", "cached", &["b"]),
            with_reason,
        ]);
        let namespaces = response["namespaces"].as_object().unwrap();
        assert_eq!(namespaces.len(), 3, "每段都要有自己的来源条目");
        assert_eq!(namespaces["wb-cn"]["source"], "live");
        assert_eq!(namespaces["wb-cn"]["note"], Value::Null, "live 不需要解释");
        assert_eq!(namespaces["wb-global"]["source"], "cached");
        assert_eq!(namespaces["trae-cn"]["source"], "builtin");
        assert_eq!(namespaces["trae-cn"]["fetched_at"], Value::Null);
        assert!(
            !namespaces["trae-cn"]["note"]
                .as_str()
                .unwrap()
                .trim()
                .is_empty(),
            "非实时必须有原因（恒非空契约）"
        );
    }

    /// 空命名空间集 ⇒ 空清单 + 空 `namespaces`（不是 `null`、不是 500）。
    #[test]
    fn empty_input_yields_an_empty_but_well_formed_body() {
        let response = build_models_response(&[]);
        assert_eq!(response["data"].as_array().unwrap().len(), 0);
        assert_eq!(response["namespaces"].as_object().unwrap().len(), 0);
        assert_eq!(response["object"], "list");
    }

    /// 一段里没有模型（例如全部被禁用）⇒ 该段仍在 `namespaces` 里出现（**能被看见**），
    /// 但 `data` 里没有它的行 —— 否则用户分不清「这段没有模型」还是「这段没启用」。
    #[test]
    fn a_namespace_with_no_models_still_appears_in_the_source_map() {
        let response = build_models_response(&[entry("wb-cn", "live", &[])]);
        assert!(response["data"].as_array().unwrap().is_empty());
        assert_eq!(response["namespaces"].as_object().unwrap().len(), 1);
        assert_eq!(response["namespaces"]["wb-cn"]["source"], "live");
    }

    /// 缺 `id` 的脏条目不得 panic（上游字段会变；脏值不能让整条请求 500）。
    #[test]
    fn a_model_without_an_id_does_not_panic() {
        let dirty = NamespaceModels {
            namespace: "wb-cn",
            source: "live",
            fetched_at: None,
            note: None,
            models: vec![json!({ "object": "model" })],
        };
        let response = build_models_response(&[dirty]);
        assert_eq!(
            response["data"][0]["id"], "wb-cn:",
            "裸名缺失时只留前缀，不 panic"
        );
    }

    /// 取 Key 的规则：`x-api-key` 优先；`Bearer`（大小写 b 都认）；都没有 ⇒ `None`。
    #[test]
    fn key_extraction_prefers_x_api_key_and_accepts_bearer() {
        let mut headers = HeaderMap::new();
        assert!(presented_key(&headers).is_none(), "空头必须返回 None");

        headers.insert("x-api-key", "sk-uni-aaaa".parse().unwrap());
        assert_eq!(presented_key(&headers), Some("sk-uni-aaaa"));

        headers.insert(header::AUTHORIZATION, "Bearer sk-uni-bbbb".parse().unwrap());
        assert_eq!(
            presented_key(&headers),
            Some("sk-uni-aaaa"),
            "x-api-key 优先"
        );

        headers.remove("x-api-key");
        assert_eq!(presented_key(&headers), Some("sk-uni-bbbb"));

        headers.insert(header::AUTHORIZATION, "bearer sk-uni-cccc".parse().unwrap());
        assert_eq!(
            presented_key(&headers),
            Some("sk-uni-cccc"),
            "小写 bearer 也要认"
        );

        headers.insert(header::AUTHORIZATION, "  sk-uni-dddd  ".parse().unwrap());
        assert_eq!(presented_key(&headers), None, "无 Bearer 前缀的裸值不认");

        headers.insert(header::AUTHORIZATION, "Bearer    ".parse().unwrap());
        assert_eq!(presented_key(&headers), None, "空 token 不认");
    }

    // ─────────────── `/v1/chat/completions` 的命名空间把关（全部发生在**分派之前**）───────────────

    /// 建一把 Key 并拿到已构建好的 router。
    ///
    /// ★ 这些用例**只走「分派之前」的拒绝路径**，因此**不会碰真实账号池 / 日志 / 账本** ——
    /// 不会在用户的 `~/.buddy-switch/` 下留任何痕迹（转发成功的那条路必须有账号才能跑，不放在单测里）。
    async fn app_with_key(namespaces: &[&str]) -> (axum::Router, std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-unified-chat-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state = UnifiedGatewayState::with_keys(
            UnifiedGatewayConfig::default(),
            UnifiedApiKeyStore::new(dir.join("keys.json")),
        );
        let owner = state.clone();
        let (_record, plaintext) = owner
            .keys
            .create(
                "测试".to_string(),
                namespaces.iter().map(|id| id.to_string()).collect(),
                None,
            )
            .unwrap();
        let app = super::super::router(state);
        (app, dir, plaintext)
    }

    /// 发一条 POST 到 `/v1/chat/completions` 并取 (状态码, JSON 体)。
    async fn post(app: &axum::Router, auth: Option<&str>, body: &str) -> (StatusCode, Value) {
        post_to(app, "/v1/chat/completions", auth, body).await
    }

    /// 发一条 POST 到指定路径并取 (状态码, JSON 体)。
    async fn post_to(
        app: &axum::Router,
        path: &str,
        auth: Option<&str>,
        body: &str,
    ) -> (StatusCode, Value) {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(token) = auth {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    /// ★ 裸模型名被拒，且错误文案**列出本入口启用的前缀**（用户一次就能改对）。
    #[tokio::test]
    async fn bare_model_name_is_rejected_with_the_enabled_prefixes() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        let (status, body) = post(&app, Some(&key), r#"{"model":"deepseek-v4.1-flash"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "namespace_prefix_required");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("wb-cn:"), "应列出可用前缀：{message}");
        assert!(
            message.contains("trae-global:"),
            "应列出全部启用项：{message}"
        );
        assert!(
            message.contains("deepseek-v4.1-flash"),
            "应回显用户填的名字：{message}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ 未授权的命名空间 ⇒ 403（且点名「这把 Key 有什么」）。
    #[tokio::test]
    async fn unauthorized_namespace_is_forbidden() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        let (status, body) = post(
            &app,
            Some(&key),
            r#"{"model":"trae-cn:deepseek-v4.1-flash"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "namespace_forbidden");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("trae-cn"), "{message}");
        assert!(
            message.contains("wb-cn"),
            "应说明这把 Key 只授权了什么：{message}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ 退役的命名空间必须在**鉴权之前**就被拒（issue #28 收敛后的等价判据）。
    ///
    /// ## 为什么换掉了原来那条用例
    ///
    /// 原用例拿 `traecode-cn`（扩展层、`default_on: false`）验「在册但未启用 ⇒ 400」。
    /// 收敛后 Trae 的四条扩展层命名空间随程序位一并退役、扩展层**清空**，
    /// 全表只剩 6 条派生层且**全部 `default_on: true`** ⇒ 「在册但未启用」这个状态
    /// 在本仓**不再存在**，原用例的前提消失。
    ///
    /// 收敛后的等价风险是「**已退役的 id 仍被当成合法命名空间**」—— 那会让用户给
    /// 一个已卸载的客户端发流量，得到的是更难排查的 503/空池而不是 400。
    /// ⇒ 这里改验「退役 id fail-closed」。
    #[tokio::test]
    async fn retired_namespace_is_rejected_before_the_authorization_check() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        for retired in [
            "traework-cn",
            "traework-global",
            "traecode-cn",
            "traecode-global",
        ] {
            let (status, body) = post(
                &app,
                Some(&key),
                &format!(r#"{{"model":"{retired}:glm-5.3"}}"#),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "退役命名空间 {retired} 必须 400（而不是放行去打上游）"
            );
            assert_ne!(
                body["error"]["code"], "namespace_not_enabled",
                "{retired} 已不在册，不该报「未启用」"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 缺凭据 ⇒ 401；且文案说明「统一入口用独立的 `sk-uni-` 库」（最常见的误用）。
    #[tokio::test]
    async fn missing_or_wrong_credentials_are_actionable() {
        let (app, dir, _key) = app_with_key(&["wb-cn"]).await;

        let (status, body) = post(&app, None, r#"{"model":"wb-cn:x"}"#).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "missing_credentials");

        // 拿两个既有网关的 Key 来用（最常见误用）
        let (status, body) = post(&app, Some("sk-wb-deadbeef"), r#"{"model":"wb-cn:x"}"#).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "invalid_api_key");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("sk-uni-"),
            "要点明统一入口的 Key 前缀：{message}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// `wb-cn:` 后面没有模型名 ⇒ 400（不是把它当合法请求送去上游）。
    #[tokio::test]
    async fn empty_model_name_after_the_prefix_is_rejected() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        let (status, body) = post(&app, Some(&key), r#"{"model":"wb-cn:"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "empty_model_name");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 非法 JSON ⇒ 400（与两条既有网关同款），**不进中继**。
    #[tokio::test]
    async fn invalid_json_is_rejected() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        let (status, body) = post(&app, Some(&key), "not json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_json");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ Trae 命名空间在**分派之前**的把关同样成立（未授权 ⇒ 403，不会被偷偷转发到别处）。
    ///
    /// ⚠️ **不在这里测「已授权的 Trae 请求真的转发」**：那一跳会用到**真实的** Trae 账号池与落盘路径
    /// （冷却 / 账本 / 日志），跑一次就会在用户 `~/.buddy-switch/trae/` 下留痕迹。
    /// 该路径由「直接调 `chat_for_variant` 的既有 Trae e2e（自带隔离状态与假上游）」间接覆盖。
    #[tokio::test]
    async fn trae_namespace_is_guarded_before_dispatch() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        let (status, body) = post(&app, Some(&key), r#"{"model":"trae-global:glm-5.3"}"#).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "未授权的 Trae 命名空间必须被拦住"
        );
        assert_eq!(body["error"]["code"], "namespace_forbidden");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ `trae_namespace_is_guarded_before_dispatch` 的对应断言 + 两个新端点的设计点。
    ///
    /// ⚠️ 与 Trae 转发同款：**只测分派之前**（`/v1/responses` + Trae 那条在触碰账号池之前就返回）。
    #[tokio::test]
    async fn messages_endpoint_uses_the_anthropic_error_shape() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        let (status, body) = post_to(
            &app,
            "/v1/messages",
            Some(&key),
            r#"{"model":"deepseek-v4.1-flash","max_tokens":16}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Anthropic 形状：顶层 `type: "error"`；`code` 是非标准字段，客户端会忽略但日志能用
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["code"], "namespace_prefix_required");
        assert_eq!(body["error"]["type"], "invalid_request_error");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ `/v1/responses` + Trae 命名空间 ⇒ **501**（设计如此），不是 404、也不是静默降级到 chat。
    #[tokio::test]
    async fn responses_endpoint_rejects_trae_namespaces_with_501() {
        let (app, dir, key) = app_with_key(&["trae-cn"]).await;
        let (status, body) = post_to(
            &app,
            "/v1/responses",
            Some(&key),
            r#"{"model":"trae-cn:deepseek-v4.1-flash"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(
            body["error"]["code"],
            "endpoint_not_supported_for_namespace"
        );
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("wb-cn"), "要给出出路：{message}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 三个 LLM 端点**共用同一份**把关：裸名在任一端点都被同样地拒绝。
    ///
    /// 可证伪：若有人给某个端点另写一份判断而漏掉某道 ⇒ 本用例在该端点变红。
    #[tokio::test]
    async fn all_three_llm_endpoints_share_the_same_guard() {
        let (app, dir, key) = app_with_key(&["wb-cn"]).await;
        for path in ["/v1/chat/completions", "/v1/messages", "/v1/responses"] {
            let (status, body) = post_to(&app, path, Some(&key), r#"{"model":"glm-5.3"}"#).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert_eq!(body["error"]["code"], "namespace_prefix_required", "{path}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
