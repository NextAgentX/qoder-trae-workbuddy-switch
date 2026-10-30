//! Trae 网关的五个端点与 Bearer 鉴权中间件。
//!
//! ## 与 WorkBuddy 网关路由的关系
//!
//! 只有 `POST /v1/chat/completions` 与 WorkBuddy 侧**同名**，实现却毫无共同点：
//! WorkBuddy 是「出站改写 → 透传 OpenAI SSE」，Trae 是「出站改写 → 转换私有 SOLO SSE」。
//! 因此本文件不引用 `crate::routes` 的任何内容，只有**错误响应体形状**刻意保持一致
//! （`{"error":{"message","type","code"}}`），让同一个 OpenAI 客户端两处都能读懂。
//!
//! ## 换号语义
//!
//! | 路径 | 连接期失败（HTTP 非 2xx / 传输层错误） | 流内失败（`event:error`） |
//! |:---|:---|:---|
//! | 流式 | 冷却该账号 → **换号重试**（最多 `max_rotate` 次） | 冷却该账号 → **不换号** |
//! | 非流式 | 冷却该账号 → **换号重试** | 冷却该账号 → **换号重试** |
//!
//! 差别来自「响应头是否已经发出」：流式一旦把 `200 + text/event-stream` 发给客户端，
//! 就只能把错误塞进事件流里（补一条 `data: {...error...}` 再 `data: [DONE]`）；
//! 非流式在聚合完成前一个字节都还没发给客户端，因此可以整轮重来。
//!
//! 流内失败也写回冷却，是为了让**下一轮**请求自动避开这个账号——否则每次都要先撞墙。

use std::collections::HashSet;
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, RawQuery, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use buddy_switch_core::modules::product::scope_of_trae_region;
use buddy_switch_core::modules::trae::account;
use buddy_switch_core::modules::trae::region::TraeRegion;
use buddy_switch_core::modules::trae::variant::TraeVariant;

use crate::protocol::anthropic;
use crate::session_headers;
use crate::sticky;

use super::capture::Capture;
use super::payload;
use super::pool::{classify_http, classify_solo, PickedTraeAccount, TraeErrKind, TraePool};
use super::sse::{self, TokenUsage};
use super::{
    now_secs, TraeGatewayState, TRAE_APP_ID, TRAE_IDE_VERSION, TRAE_IDE_VERSION_CODE,
    TRAE_LLM_CHAT_PATH,
};

/// 请求扩展：承载 Bearer Key 的**归属标识**，供 handler 选择对应的账号池与 `function`。
///
/// ## ★ 键里存的是**程序位**（2026-09-30 起可带 TraeCode）
///
/// 键记录由前端「归属程序位」下拉创建，取值是 4 个程序位：
/// `trae_work`（国内 TraeWork）/ `trae_cn`（国内 TraeCode）/ `global`（国际 TraeWork）/
/// `global_trae_code`（国际 TraeCode）。旧值 `cn` / `global` 经 [`TraeVariant::parse`]
/// 仍落**区域主程序**（TraeWork），老键行为不变。
///
/// 这里拿到的变体同时决定两件事，**两者必须同源**：
/// 1. `/v1/models` 列哪份客户端清单（[`payload::models_response_for`]）；
/// 2. 请求体里的 `function`（[`super::function_for`]）。
///
/// 只改其一就会出现「列得出来、调不动」—— 这正是 issue #4 的形态：
/// TraeCode 的模型（`glm-5.3-flash`）曾被写死的 `solo_work_lite` 一律拒成
/// `4001 param is invalid`（2026-09-30 上游实测，见
/// [`super::function_for`] 的实测表与 `tests::probe_traecode_model_names`）。
///
/// 账号池是**区域级**的（`pool.rs` 用 `entries_for_region`），两条程序位共用一本库，
/// 因此程序位只影响清单与 `function`，不需要新池。
///
/// 用「请求扩展」而非给每个 handler 加参数：鉴权中间件解析出归属后写入，
/// `chat_completions` 读出，避免中间件 → handler 的签名穿透一堆函数。
#[derive(Clone, Copy, Debug)]
pub struct TraeKeyVariant(pub TraeVariant);

/// ★ issue #76：本次请求的**整条 Key**（策略 + id）—— 供配额判定、账号白名单、
/// 按密钥记账三处使用。
///
/// 为什么单独一个 extension 而不是把字段摊进 [`TraeKeyVariant`]：
/// 那个类型只承载「用哪个账号池」这一个用途，把它撑大成「Key 全量」会让
/// 所有读它的地方都依赖 `TraeApiKeyRecord`（含 hash 字段的类型）。
/// 这里用 `Arc` 持有，中间件每请求克隆一次指针即可。
#[derive(Clone)]
pub struct TraeKeyContext(pub std::sync::Arc<crate::trae::apikey::TraeApiKeyRecord>);

/// 日志里的端点名（与 WorkBuddy 网关同名字符串，便于统一聚合）。
///
/// ★ 两条入口各有一个常量，并且**同时**决定出站协议：`/v1/messages` 要额外做一次
/// Anthropic 转换。把「协议」与「日志端点」合成同一个参数，是为了让它们**不可能**
/// 对不上 —— 否则某天会出现「按 Anthropic 转换了、日志却记成 chat completions」
/// 这种统计静默错位。
const ENDPOINT_CHAT: &str = "/v1/chat/completions";
const ENDPOINT_MESSAGES: &str = "/v1/messages";
/// `POST /v1/responses`（Responses API）。
///
/// 上游仍是 `llm_utils_chat`（只认 chat 形态）⇒ 本端点是**两次转换**：入站
/// Responses → chat（[`crate::protocol::responses::to_upstream_request`]），出站
/// chat SSE → Responses 事件流（[`crate::protocol::responses::ResponsesSseStream`]）。
/// 选号 / 换号 / 冷却 / 粘性 / 记账**一行都没改** —— 与 `/v1/messages` 同一个理由。
const ENDPOINT_RESPONSES: &str = "/v1/responses";

/// 该端点是否要 Anthropic 出站转换。
fn is_anthropic_endpoint(endpoint: &str) -> bool {
    endpoint == ENDPOINT_MESSAGES
}

/// 该端点是否要 Responses 出站转换。
fn is_responses_endpoint(endpoint: &str) -> bool {
    endpoint == ENDPOINT_RESPONSES
}

/// 上游 UA。参考实现固定为 `TraeClient/TTNet`，改它没有好处。
const TRAE_USER_AGENT: &str = "TraeClient/TTNet";

// ---------------------------------------------------------------------------
// 处理器
// ---------------------------------------------------------------------------

/// `GET /health`：存活探针，**免鉴权**。
///
/// 附带账号池摘要：探活时顺手看一眼「还有没有可用账号」比再发一次 `/status` 省事。
/// 摘要取**默认变体**的池（探针无 Key、无归属，只能给一条产品线的概览）。
pub async fn health(State(state): State<TraeGatewayState>) -> Response {
    let variant = TraeVariant::default();
    let summary = {
        let mut pools = state.pools.lock().await;
        let pool = pools
            .entry(variant)
            .or_insert_with(|| TraePool::for_variant(variant));
        pool.sync_for(variant);
        pool.summary(now_secs())
    };
    json_response(
        StatusCode::OK,
        json!({
            "status": "ok",
            "service": "trae-gateway",
            "running": true,
            "total_requests": state.total_requests.load(std::sync::atomic::Ordering::Relaxed),
            "pool": summary,
        }),
    )
}

/// Bearer 鉴权中间件：健康检查（[`crate::HEALTH_PATHS`]）免鉴权，其余端点校验
/// `Authorization: Bearer <key>`。
///
/// 与参考实现的一处**有意差异**：没有任何可用 Key 时**拒绝**而不是放行。参考实现
/// 「留空即不鉴权」意味着任何人只要猜到 57894 端口就能白嫖额度；本网关要求 Key 由
/// 用户显式创建（或经旧 `settings.apiKey` 兼容读得来），不存在「用户忘了配」的放行场景，
/// 所以宁严勿宽。
///
/// 鉴权通过后把 `record.variant`（**归属产品线**）写进请求扩展
/// （[`TraeKeyVariant`]），下游据此选择对应的账号池；同时 `touch` 刷新最近使用时间。
pub async fn bearer_auth(
    State(state): State<TraeGatewayState>,
    mut request: Request,
    next: Next,
) -> Response {
    if crate::HEALTH_PATHS.contains(&request.uri().path()) {
        return next.run(request).await;
    }

    // ★ 错误体形状按**端点**分派：Anthropic 客户端只认 `{"type":"error",...}`，
    // 给它 OpenAI 形状的话，它报的是「响应格式错误」而不是「凭据无效」——
    // 排查方向会被带偏。401 对应 Anthropic 的 `authentication_error`。
    let anthropic = request.uri().path() == "/v1/messages";
    let deny = move |message: &str| {
        if anthropic {
            anthropic_error(StatusCode::UNAUTHORIZED, "authentication_error", message)
        } else {
            openai_error(StatusCode::UNAUTHORIZED, "invalid_request_error", message)
        }
    };

    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
                .map(str::trim)
        })
        .filter(|value| !value.is_empty());

    match presented {
        None => deny("缺少凭据：请在 Authorization 头携带 `Bearer <API Key>`"),
        Some(key) => match state.key_store.verify(key) {
            Some(record) => {
                // 归属产品线透传给 handler（决定用哪个账号池）。
                request
                    .extensions_mut()
                    .insert(TraeKeyVariant(record.variant));
                // ★ issue #76：整条 Key 也塞进扩展（配额 / 白名单 / 记账要用）。
                request
                    .extensions_mut()
                    .insert(TraeKeyContext(std::sync::Arc::new(record.clone())));
                state.key_store.touch(&record.id);
                next.run(request).await
            }
            None => deny("API Key 无效：请到「API 服务」页创建或复制一把可用的 Key"),
        },
    }
}

/// `GET /status`：运行状态 + 账号明细 + 诊断。
///
/// 可选 query `variant`（`trae_work` / `trae_cn`）决定看哪个账号池；
/// 缺失或无法识别一律回落 [`TraeVariant::default`]（TraeWork），与旧行为一致。
pub async fn status(State(state): State<TraeGatewayState>, RawQuery(query): RawQuery) -> Response {
    let variant = parse_variant_query(query.as_deref());
    // 复用管理面那份组装逻辑：`/status` 与「API 服务」页显示的必须是同一份状态。
    let config = state.config_snapshot().await;
    let addr = Some(format!("{}:{}", config.bind_addr, config.port));
    let body = super::status_view(&state, true, addr, env!("CARGO_PKG_VERSION"), variant).await;
    json_response(StatusCode::OK, body)
}

/// 从 query 串解析变体参数（最简实现，与 server 的 `query_value` 同风格）。
fn parse_variant_query(query: Option<&str>) -> TraeVariant {
    query
        .unwrap_or("")
        .split('&')
        .find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key == "variant").then(|| TraeVariant::parse(value).unwrap_or_default())
        })
        .unwrap_or_default()
}

/// `GET /v1/models`：**随上游刷新**的模型清单（issue #4）。
///
/// 清单来源与「API 服务」页的模型卡**同源**：读该 Key **归属产品线**的客户端
/// `state.vscdb` 缓存（上游下发）。归属由鉴权中间件经请求扩展传入。
///
/// ## 两处刻意的取舍
///
/// 1. **归属用必需提取器**（不是 `Option<Extension<…>>`）：中间件对除 `/health`
///    外的所有路由都会注入它，**拿不到就说明鉴权链断了** —— 那属于配置错误，
///    应当 500 响亮失败，而不是静默回落默认变体（那会把「链断了」伪装成正常）。
/// 2. **读库放进阻塞池**：`models_response_for` 会同步读 SQLite（客户端可能在写，
///    最多等 core 侧设的 `busy_timeout` 那个窗口）。直接在 async handler 里做会占住 worker。
///
/// 客户端没启动过 / 没登录时读不到 ⇒ 回落静态清单（见 [`payload::models_response_for`]），
/// 保证 OpenAI 客户端**开箱即可列出模型**，而不是返回空表或报错。
pub async fn models(
    State(state): State<TraeGatewayState>,
    Extension(key_variant): Extension<TraeKeyVariant>,
) -> Response {
    let variant = key_variant.0;
    // ★ 出口 ①（P11）：读该变体所在 region 的禁用集合，交给清单构造器过滤。
    //   `region_of_variant` 把程序位映射到存储键（存储按 region 分家，决策 #4）。
    let disabled = state
        .disabled
        .read()
        .await
        .get(scope_of_trae_region(variant.region()))
        .clone();
    let fallback = disabled.clone();
    // ★ 官方模型目录（逐模型 `dev/max`）：过期就先同步一次；失败**静默**沿用旧缓存，
    //   绝不让「目录拉不到」变成「列模型失败」。
    super::catalog::ensure_fresh(variant).await;
    let max_mode = state.max_mode_now();
    // `models_response_for` 自己**不失败**（读不到就回落静态清单），所以这里的
    // `JoinError` 只可能是运行时正在关闭 —— 用（过滤后的）静态清单兜底，别让「列模型」挂掉。
    let body = tokio::task::spawn_blocking(move || {
        payload::models_response_for(variant, &disabled, max_mode)
    })
    .await
    .unwrap_or_else(|_| payload::models_response_filtered(&fallback, Some(variant), max_mode));
    json_response(StatusCode::OK, body)
}

/// `POST /v1/chat/completions`。
///
/// 归属产品线由鉴权中间件经请求扩展（[`TraeKeyVariant`]）传入，决定用哪个账号池。
pub async fn chat_completions(
    State(state): State<TraeGatewayState>,
    Extension(key_variant): Extension<TraeKeyVariant>,
    // ★ issue #76：整条 Key（配额 / 白名单 / 记账）。缺这个扩展 = 老路由 / 测试调用。
    key_ctx: Option<Extension<TraeKeyContext>>,
    _headers: HeaderMap,
    body: Bytes,
) -> Response {
    // 归属产品线由鉴权中间件经请求扩展传入 ⇒ 这里只是把它转成**显式参数**交给共用实现，
    // 让统一网关（没有 Key 归属、只有命名空间）也能调用同一份中继。
    let key = key_ctx.map(|Extension(ctx)| ctx.0);
    // ★ issue #76：收敛成 `KeyActor`（id + 策略 + **本产品线的账本**）。
    let actor = key.map(|record| {
        std::sync::Arc::new(crate::apikey::KeyActor::new(
            record.id.clone(),
            record.policy(),
            state.key_usage.clone(),
            "trae",
        ))
    });
    chat_for_variant(state, key_variant.0, body, actor).await
}

/// 一趟 Trae chat 中继（**认证之后的全部逻辑**）—— 产品线入口与统一网关**共用**这一份实现。
///
/// # 为什么抽出来
///
/// 与 WB 侧 [`crate::routes::chat::chat_for_region`] 同一个理由：中继**外面**那一层
/// （出站改写 / 停用判定 / 选号换号 / 冷却 / 粘性 / 日志）**不能有两份** ——
/// 两份必然漂移，而漂移点恰恰是「换号 + 流内失败」这类最难复现的行为。
///
/// `state` **按值**传入（[`TraeGatewayState`] 是 `Arc` 句柄，克隆极廉价）⇒ 函数体内既有的
/// `&state` 写法一行都不用改，抽取的 diff 面最小。`variant` 由调用方给：
/// 产品线入口来自 Key 归属，统一入口来自**命名空间**。
pub async fn chat_for_variant(
    state: TraeGatewayState,
    variant: TraeVariant,
    body: Bytes,
    // ★ issue #76：调用方的 Key（`None` = 无 Key 的调用 ⇒ 不判配额、不过白名单、不记账）。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let started = Instant::now();
    // ★ issue #76：配额判定（选号前）—— 超限直接 429，一步都不发。
    if let Some(actor) = key.as_deref() {
        // 判定 + 请求数记账都在 `KeyActor` 里（四条产品线同一份实现）。
        if let Some(message) =
            actor.check_quota_and_count(buddy_switch_core::modules::config::now_ms())
        {
            return openai_error(
                StatusCode::TOO_MANY_REQUESTS,
                "key_quota_exceeded",
                &message,
            );
        }
    }
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return openai_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &format!("请求体不是合法 JSON：{error}"),
            )
        }
    };

    let config = state.config_snapshot().await;
    let model = payload::model_of(&parsed, &config.default_model);
    // ★ 出口 ②（P11）：被禁用的模型直接拒绝 —— 与 `/v1/models` 读**同一份** `disabled`
    //   （`is_disabled` 是唯一判断点）。只改过滤不拒绝 = 假开关。
    // ★ 禁用集合按**作用域**分家（2026-10-15 起）—— Trae 路由把区域升成 Scope。
    let scope = scope_of_trae_region(variant.region());
    {
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, scope, &model) {
            return openai_error(
                StatusCode::FORBIDDEN,
                "model_disabled",
                &crate::disabled_models::disabled_message(scope, &model),
            );
        }
    }
    let stream = payload::wants_stream(&parsed);
    let max_rotate = config.max_rotate.max(1);
    let chat_id = format!("chatcmpl-{}", uuid_like());

    let sticky_key = sticky_key_of(&parsed, variant, &model);

    // ★ 降级链（P13）：主模型失败且命中触发条件时，改用备选模型重发。
    //   hop 循环放在**入口层**而不是 `stream_chat` 内部 —— 三个入口（chat / messages /
    //   responses）共用同一段，不会各写一份而漂移。
    let fallback = crate::routes::fallback::FallbackLoop::plan(scope, &model).await;
    let raw_body = String::from_utf8_lossy(&body).to_string();
    let mut hop = 0usize;
    // 上一跳的失败响应（越界时返回它）与触发原因（写进降级头）。
    let mut last: Option<Response> = None;
    let mut last_trigger: Option<crate::model_fallback::Trigger> = None;
    loop {
        // ★ 越界在这里挡：`model_at` 越界返回 `None`（**不回落主模型**）——
        //   回落会让「备选全被禁用」变成「把主模型原样重发一次」，
        //   还会写出 `Fallback-From: A / To: A` 的荒唐头。
        let Some(hop_model) = fallback.model_at(hop) else {
            return last.expect("第 0 跳必然执行过");
        };
        // 被用户禁用的目标直接跳过：一次请求都不发。
        if fallback.should_skip(hop, &state.disabled).await {
            hop += 1;
            continue;
        }
        // body 里没有 `model` 字段 ⇒ 改写不了 ⇒ 不降级
        // （否则每跳实际发同一个模型，却告诉客户端「降到了备选」= 对客户端撒谎）。
        let Some(hop_body) = fallback.body_for(hop, &raw_body) else {
            return last.expect("第 0 跳必然执行过");
        };
        let hop_model = hop_model.to_string();
        // 同平台跨区域：换 variant（`trae-cn` ↔ `trae-global` 协议一致，只是账号池与域名不同）。
        //
        // ★ 只在**目标作用域 ≠ 入口作用域**时才换：同作用域必须**原样保留入口的变体**。
        //   `trae_variant_of` 返回的是**合并版**（`trae_merged`），而入口可能是 `trae_cn` ——
        //   无脑替换会让请求日志里的变体名从 `trae_cn` 变成 `trae_merged`
        //   （`request_log_carries_the_variant` 钉住了这一点）。
        let hop_variant = fallback
            .scope_at(hop)
            .filter(|target_scope| *target_scope != scope)
            .and_then(crate::routes::cross::trae_variant_of)
            .unwrap_or(variant);
        let hop_body = Bytes::from(hop_body);
        // ★ 跨平台目标：交给 `cross::relay_on_scope`（同进程内调目标产品线入口）。
        //   深度由 `cross_depth()` 兜底（`FallbackLoop::plan` 已先过滤一次）。
        let response = if fallback
            .scope_at(hop)
            .is_some_and(|target_scope| target_scope.product() != scope.product())
        {
            Box::pin(crate::routes::cross::relay_on_scope(
                fallback.scope_at(hop).expect("上面已判过 Some"),
                axum::http::HeaderMap::new(),
                hop_body.clone(),
                None,
            ))
            .await
        } else if stream {
            stream_chat(
                &state,
                &hop_body,
                &config.default_model,
                &hop_model,
                &chat_id,
                max_rotate,
                &config.preferred_uid,
                sticky_key.as_deref(),
                ENDPOINT_CHAT,
                started,
                hop_variant,
                key.as_deref(),
                // chat 入口没有 custom 工具概念。
                None,
            )
            .await
        } else {
            aggregate_chat(
                &state,
                &hop_body,
                &config.default_model,
                &hop_model,
                &chat_id,
                max_rotate,
                &config.preferred_uid,
                sticky_key.as_deref(),
                started,
                hop_variant,
                key.as_deref(),
            )
            .await
        };
        if response.status().is_success() {
            let mut response = response;
            if hop > 0 {
                if let Some(trigger) = last_trigger {
                    fallback.apply_headers(hop, &mut response, trigger);
                    // 记一笔降级成功（Trae 拿不到完整轨迹，path 只记最终用的那个）。
                    crate::model_fallback::record_success(
                        scope,
                        fallback.origin(),
                        &hop_model,
                        trigger,
                        started.elapsed().as_millis() as i64,
                    );
                }
            }
            return response;
        }
        // 能不能继续降级：**状态码能映射到触发条件** 且 **该条件在用户配的白名单里**。
        match fallback.can_fallback(&response) {
            Some(trigger) => last_trigger = Some(trigger),
            None => return response,
        }
        last = Some(response);
        hop += 1;
    }
}

/// `POST /v1/messages`：Anthropic 协议入口。
///
/// ## 为什么能与 `/v1/chat/completions` 共用整条链路
///
/// 上游只认一种东西（Trae 私有的 `llm_utils_chat`），所以两条入口的差别**只在协议层**：
/// 入站 Anthropic → OpenAI 形态（[`anthropic::to_upstream_request_adaptive`]），
/// 出站 OpenAI → Anthropic（[`anthropic::AnthropicSseStream`] /
/// [`anthropic::message_from_response`]）。中间的中继（选号 / 换号 / 冷却 / 粘性 /
/// 日志）**一行都不用改** —— 这正是把这两个转换放进共用 `protocol` 层
/// （而不是 Trae 模块内）的价值。
///
/// ## 上游请求体是**重新序列化**的
///
/// `to_upstream_request` 产出 `Value`，要再序列化成 `Bytes` 才能喂给中继 ——
/// 不能透传原始 `Bytes`：Anthropic 的 `system` / `max_tokens` 与 OpenAI 的
/// `messages` / `max_completion_tokens` 不同形。
pub async fn messages(
    State(state): State<TraeGatewayState>,
    Extension(key_variant): Extension<TraeKeyVariant>,
    key_ctx: Option<Extension<TraeKeyContext>>,
    body: Bytes,
) -> Response {
    // 同 `/v1/chat/completions`：把扩展里的归属转成显式参数，交给共用实现。
    let key = key_ctx.map(|Extension(ctx)| ctx.0);
    let actor = key.map(|record| {
        std::sync::Arc::new(crate::apikey::KeyActor::new(
            record.id.clone(),
            record.policy(),
            state.key_usage.clone(),
            "trae",
        ))
    });
    messages_for_variant(state, key_variant.0, body, actor).await
}

/// 一趟 Trae **Anthropic** 中继（认证之后的全部逻辑）—— 产品线入口与统一网关共用。
///
/// 与 [`chat_for_variant`] 的差别只在协议层（入站 Anthropic → 上游 OpenAI 形态，
/// 出站上游 SSE → Anthropic 事件流）；选号 / 换号 / 冷却 / 粘性 / 日志**完全同一份**。
pub async fn messages_for_variant(
    state: TraeGatewayState,
    variant: TraeVariant,
    body: Bytes,
    // ★ issue #76：同 `chat_for_variant`。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let started = Instant::now();
    // ★ issue #76：配额判定（选号前）。
    if let Some(actor) = key.as_deref() {
        if let Some(message) =
            actor.check_quota_and_count(buddy_switch_core::modules::config::now_ms())
        {
            return anthropic_error(
                StatusCode::TOO_MANY_REQUESTS,
                "key_quota_exceeded",
                &message,
            );
        }
    }

    let config = state.config_snapshot().await;
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "请求体不是合法 JSON",
        );
    };

    /* ★ 图片：默认丢（Trae 上游是否吃 `content` 里的 `image_url` parts 未实测），
    丢弃时文本里会留下「已移除 N 张图片」；`BUDDY_SWITCH_ANTHROPIC_FORWARD_IMAGES=1`
    打开后改走带图分支。`llm_utils_chat` 本来就要求 `content` 是数组，带图产出
    的 parts 数组不需要额外适配。 */
    let upstream = anthropic::to_upstream_request_adaptive(&parsed);
    let Ok(encoded) = serde_json::to_vec(&upstream) else {
        return anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "请求体无法序列化",
        );
    };
    let upstream_body = Bytes::from(encoded);

    // 模型取**转换后**的请求体（那是真正发给上游的东西）；
    // 流式开关见 [`anthropic_wants_stream`]（**必须**读原始请求体）。
    let model = payload::model_of(&upstream, &config.default_model);
    // ★ 出口 ②（P11）：与 `/v1/chat/completions` 同源 —— 被禁用的模型直接拒绝（Anthropic 错误体）。
    let scope = scope_of_trae_region(variant.region());
    {
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, scope, &model) {
            return anthropic_error(
                StatusCode::FORBIDDEN,
                "model_disabled",
                &crate::disabled_models::disabled_message(scope, &model),
            );
        }
    }
    let stream = anthropic_wants_stream(&parsed);
    let max_rotate = config.max_rotate.max(1);
    // 上游响应 id 用 `chatcmpl-` 前缀（与 `/v1/chat/completions` 一致）。
    //
    // ★ 不要用 `msg_`：`protocol::anthropic::message_from_accumulator` 会在上游 id 上
    // **再包一层** `msg_`，用 `msg_` 当上游前缀会得到 `msg_msg_xxx`（真实请求实测到）。
    let chat_id = format!("chatcmpl-{}", uuid_like());

    // 会话 id 取**原始** Anthropic 请求体：`to_upstream_request` 不保证把
    // `metadata` 原样带过去，而客户端塞会话 id 的习惯位置就是那里
    // （与 WorkBuddy 侧 `messages.rs` 同源）。拿不到 ⇒ 返回 `None` ⇒ 整段跳过粘性。
    let sticky_key = sticky_key_of(&parsed, variant, &model);

    // ★ 降级链（P13）：与 `/v1/chat/completions` 同一套 hop 循环（见那里的注释）。
    let fallback = crate::routes::fallback::FallbackLoop::plan(scope, &model).await;
    let raw_body = String::from_utf8_lossy(&upstream_body).to_string();
    let mut hop = 0usize;
    // 上一跳的失败响应（越界时返回它）与触发原因（写进降级头）。
    let mut last: Option<Response> = None;
    let mut last_trigger: Option<crate::model_fallback::Trigger> = None;
    loop {
        // ★ 越界在这里挡：`model_at` 越界返回 `None`（**不回落主模型**）——
        //   回落会让「备选全被禁用」变成「把主模型原样重发一次」，
        //   还会写出 `Fallback-From: A / To: A` 的荒唐头。
        let Some(hop_model) = fallback.model_at(hop) else {
            return last.expect("第 0 跳必然执行过");
        };
        // 被用户禁用的目标直接跳过：一次请求都不发。
        if fallback.should_skip(hop, &state.disabled).await {
            hop += 1;
            continue;
        }
        // body 里没有 `model` 字段 ⇒ 改写不了 ⇒ 不降级
        // （否则每跳实际发同一个模型，却告诉客户端「降到了备选」= 对客户端撒谎）。
        let Some(hop_body) = fallback.body_for(hop, &raw_body) else {
            return last.expect("第 0 跳必然执行过");
        };
        let hop_model = hop_model.to_string();
        // 同平台跨区域：换 variant（`trae-cn` ↔ `trae-global` 协议一致，只是账号池与域名不同）。
        //
        // ★ 只在**目标作用域 ≠ 入口作用域**时才换：同作用域必须**原样保留入口的变体**。
        //   `trae_variant_of` 返回的是**合并版**（`trae_merged`），而入口可能是 `trae_cn` ——
        //   无脑替换会让请求日志里的变体名从 `trae_cn` 变成 `trae_merged`
        //   （`request_log_carries_the_variant` 钉住了这一点）。
        let hop_variant = fallback
            .scope_at(hop)
            .filter(|target_scope| *target_scope != scope)
            .and_then(crate::routes::cross::trae_variant_of)
            .unwrap_or(variant);
        let hop_body = Bytes::from(hop_body);
        // ★ 跨平台目标：交给 `cross::relay_on_scope`（同进程内调目标产品线入口）。
        //   深度由 `cross_depth()` 兜底（`FallbackLoop::plan` 已先过滤一次）。
        let response = if fallback
            .scope_at(hop)
            .is_some_and(|target_scope| target_scope.product() != scope.product())
        {
            Box::pin(crate::routes::cross::relay_on_scope(
                fallback.scope_at(hop).expect("上面已判过 Some"),
                axum::http::HeaderMap::new(),
                hop_body.clone(),
                None,
            ))
            .await
        } else if stream {
            stream_chat(
                &state,
                &hop_body,
                &config.default_model,
                &hop_model,
                &chat_id,
                max_rotate,
                &config.preferred_uid,
                sticky_key.as_deref(),
                ENDPOINT_MESSAGES,
                started,
                hop_variant,
                key.as_deref(),
                // Anthropic 入口没有 custom 工具概念。
                None,
            )
            .await
        } else {
            match aggregate_payload(
                &state,
                &hop_body,
                &config.default_model,
                &hop_model,
                &chat_id,
                max_rotate,
                &config.preferred_uid,
                sticky_key.as_deref(),
                ENDPOINT_MESSAGES,
                started,
                hop_variant,
                key.as_deref(),
            )
            .await
            {
                Ok(payload) => {
                    // ★ issue #76：Anthropic 端点同样按密钥记 token（payload 是上游 OpenAI 形状，
                    //   所以取 `usage.total_tokens` 与 chat 端点同源）。
                    record_key_tokens(&state, key.as_deref(), &payload);
                    json_response(
                        StatusCode::OK,
                        anthropic::message_from_response(&payload, &hop_model),
                    )
                }
                Err(failure) => anthropic_error(
                    StatusCode::from_u16(failure.status).unwrap_or(StatusCode::BAD_GATEWAY),
                    &failure.code,
                    &failure.message,
                ),
            }
        };
        if response.status().is_success() {
            let mut response = response;
            if hop > 0 {
                if let Some(trigger) = last_trigger {
                    fallback.apply_headers(hop, &mut response, trigger);
                    // 记一笔降级成功（Trae 拿不到完整轨迹，path 只记最终用的那个）。
                    crate::model_fallback::record_success(
                        scope,
                        fallback.origin(),
                        &hop_model,
                        trigger,
                        started.elapsed().as_millis() as i64,
                    );
                }
            }
            return response;
        }
        // 能不能继续降级：**状态码能映射到触发条件** 且 **该条件在用户配的白名单里**。
        match fallback.can_fallback(&response) {
            Some(trigger) => last_trigger = Some(trigger),
            None => return response,
        }
        last = Some(response);
        hop += 1;
    }
}

/// `POST /v1/responses`：Responses 协议入口（Codex 等 Responses 客户端）。
///
/// 与 [`messages`] 同一个套路：把扩展里的归属转成显式参数，交给共用实现
/// [`responses_for_variant`]。
pub async fn responses(
    State(state): State<TraeGatewayState>,
    Extension(key_variant): Extension<TraeKeyVariant>,
    key_ctx: Option<Extension<TraeKeyContext>>,
    body: Bytes,
) -> Response {
    let key = key_ctx.map(|Extension(ctx)| ctx.0);
    let actor = key.map(|record| {
        std::sync::Arc::new(crate::apikey::KeyActor::new(
            record.id.clone(),
            record.policy(),
            state.key_usage.clone(),
            "trae",
        ))
    });
    responses_for_variant(state, key_variant.0, body, actor).await
}

/// 一趟 Trae **Responses** 中继（认证之后的全部逻辑）—— 产品线入口与统一网关共用。
///
/// ## 为什么这条链能复用
///
/// 上游只认 `llm_utils_chat`（chat 形态），所以本端点与 `/v1/chat/completions` 的
/// 差别**全在协议层**：入站 [`crate::protocol::responses::to_upstream_request`]，
/// 出站 [`crate::protocol::responses::ResponsesSseStream`] /
/// [`crate::protocol::responses::to_response_object`]。选号 / 换号 / 冷却 / 粘性 /
/// 记账与另外两条入口**完全同一份** —— 与 `/v1/messages` 的理由一样：另写一份必然分家。
///
/// ## 错误体
///
/// 用 OpenAI 形状（[`openai_error`]）而不是 Anthropic 形状：Responses 客户端只认前者。
pub async fn responses_for_variant(
    state: TraeGatewayState,
    variant: TraeVariant,
    body: Bytes,
    // ★ issue #76：同 `chat_for_variant`。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let started = Instant::now();
    // ★ issue #76：配额判定（选号前）。
    if let Some(actor) = key.as_deref() {
        if let Some(message) =
            actor.check_quota_and_count(buddy_switch_core::modules::config::now_ms())
        {
            return openai_error(
                StatusCode::TOO_MANY_REQUESTS,
                "key_quota_exceeded",
                &message,
            );
        }
    }

    let config = state.config_snapshot().await;
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "请求体不是合法 JSON",
        );
    };

    /* freeform（custom）工具名必须从**原始**请求体取：转换之后它们已被降级成普通
    function，`type` 没了，名字还在但语义丢失 —— 出站要靠这份集合还原成
    `custom_tool_call` 事件/条目。 */
    let responses_custom =
        std::sync::Arc::new(crate::protocol::responses::custom_tool_names(&parsed));

    // Responses 请求 → 上游 chat 形态。
    let upstream = crate::protocol::responses::to_upstream_request(&parsed);
    let Ok(encoded) = serde_json::to_vec(&upstream) else {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "请求体无法序列化",
        );
    };
    let upstream_body = Bytes::from(encoded);

    // 模型取**转换后**的请求体（那是真正发给上游的东西）。
    let model = payload::model_of(&upstream, &config.default_model);
    // ★ 出口 ②（P11）：与 `/v1/chat/completions` 同源 —— 被禁用的模型直接拒绝。
    let scope = scope_of_trae_region(variant.region());
    {
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, scope, &model) {
            return openai_error(
                StatusCode::FORBIDDEN,
                "model_disabled",
                &crate::disabled_models::disabled_message(scope, &model),
            );
        }
    }
    let stream = payload::wants_stream(&parsed);
    let max_rotate = config.max_rotate.max(1);
    let chat_id = format!("chatcmpl-{}", uuid_like());

    /* 粘性键：会话 id 优先；Responses 客户端（Codex 等）没有 `metadata`，用
    `prompt_cache_key` 顶上 —— 它本身就是「同一段会话复用同一前缀」的语义，
    与粘性同目的。两者都没有 ⇒ `None` ⇒ 整段跳过粘性（绝不退化成轮级键）。 */
    let sticky_key = session_headers::conversation_id_of(&parsed)
        .map(|conversation| conversation.trim().to_string())
        .filter(|conversation| !conversation.is_empty())
        .or_else(|| {
            parsed
                .get("prompt_cache_key")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_string)
        })
        .map(|conversation| sticky::sticky_key(variant.as_str(), &model, &conversation));

    // ★ 降级链（P13）：与 `/v1/chat/completions` 同一套 hop 循环（见那里的注释）。
    let fallback = crate::routes::fallback::FallbackLoop::plan(scope, &model).await;
    let raw_body = String::from_utf8_lossy(&upstream_body).to_string();
    let mut hop = 0usize;
    // 上一跳的失败响应（越界时返回它）与触发原因（写进降级头）。
    let mut last: Option<Response> = None;
    let mut last_trigger: Option<crate::model_fallback::Trigger> = None;
    loop {
        // ★ 越界在这里挡：`model_at` 越界返回 `None`（**不回落主模型**）——
        //   回落会让「备选全被禁用」变成「把主模型原样重发一次」，
        //   还会写出 `Fallback-From: A / To: A` 的荒唐头。
        let Some(hop_model) = fallback.model_at(hop) else {
            return last.expect("第 0 跳必然执行过");
        };
        // 被用户禁用的目标直接跳过：一次请求都不发。
        if fallback.should_skip(hop, &state.disabled).await {
            hop += 1;
            continue;
        }
        // body 里没有 `model` 字段 ⇒ 改写不了 ⇒ 不降级
        // （否则每跳实际发同一个模型，却告诉客户端「降到了备选」= 对客户端撒谎）。
        let Some(hop_body) = fallback.body_for(hop, &raw_body) else {
            return last.expect("第 0 跳必然执行过");
        };
        let hop_model = hop_model.to_string();
        // 同平台跨区域：换 variant（`trae-cn` ↔ `trae-global` 协议一致，只是账号池与域名不同）。
        //
        // ★ 只在**目标作用域 ≠ 入口作用域**时才换：同作用域必须**原样保留入口的变体**。
        //   `trae_variant_of` 返回的是**合并版**（`trae_merged`），而入口可能是 `trae_cn` ——
        //   无脑替换会让请求日志里的变体名从 `trae_cn` 变成 `trae_merged`
        //   （`request_log_carries_the_variant` 钉住了这一点）。
        let hop_variant = fallback
            .scope_at(hop)
            .filter(|target_scope| *target_scope != scope)
            .and_then(crate::routes::cross::trae_variant_of)
            .unwrap_or(variant);
        let hop_body = Bytes::from(hop_body);
        // ★ 跨平台目标：交给 `cross::relay_on_scope`（同进程内调目标产品线入口）。
        //   深度由 `cross_depth()` 兜底（`FallbackLoop::plan` 已先过滤一次）。
        let response = if fallback
            .scope_at(hop)
            .is_some_and(|target_scope| target_scope.product() != scope.product())
        {
            Box::pin(crate::routes::cross::relay_on_scope(
                fallback.scope_at(hop).expect("上面已判过 Some"),
                axum::http::HeaderMap::new(),
                hop_body.clone(),
                None,
            ))
            .await
        } else if stream {
            stream_chat(
                &state,
                &hop_body,
                &config.default_model,
                &hop_model,
                &chat_id,
                max_rotate,
                &config.preferred_uid,
                sticky_key.as_deref(),
                ENDPOINT_RESPONSES,
                started,
                hop_variant,
                key.as_deref(),
                Some(responses_custom.clone()),
            )
            .await
        } else {
            match aggregate_payload(
                &state,
                &hop_body,
                &config.default_model,
                &hop_model,
                &chat_id,
                max_rotate,
                &config.preferred_uid,
                sticky_key.as_deref(),
                ENDPOINT_RESPONSES,
                started,
                hop_variant,
                key.as_deref(),
            )
            .await
            {
                Ok(payload) => {
                    record_key_tokens(&state, key.as_deref(), &payload);
                    // 非流式：上游给的是 chat.completion，先收成聚合器再走**同一份**
                    // Responses 形状实现（两条上游共用 `to_response_object_from`）。
                    let accumulator =
                        crate::protocol::openai::CompletionAccumulator::from_response(&payload);
                    json_response(
                        StatusCode::OK,
                        crate::protocol::responses::to_response_object(
                            &accumulator,
                            &hop_model,
                            responses_custom.as_ref(),
                        ),
                    )
                }
                Err(failure) => openai_error(
                    StatusCode::from_u16(failure.status).unwrap_or(StatusCode::BAD_GATEWAY),
                    &failure.code,
                    &failure.message,
                ),
            }
        };
        if response.status().is_success() {
            let mut response = response;
            if hop > 0 {
                if let Some(trigger) = last_trigger {
                    fallback.apply_headers(hop, &mut response, trigger);
                    // 记一笔降级成功（Trae 拿不到完整轨迹，path 只记最终用的那个）。
                    crate::model_fallback::record_success(
                        scope,
                        fallback.origin(),
                        &hop_model,
                        trigger,
                        started.elapsed().as_millis() as i64,
                    );
                }
            }
            return response;
        }
        // 能不能继续降级：**状态码能映射到触发条件** 且 **该条件在用户配的白名单里**。
        match fallback.can_fallback(&response) {
            Some(trigger) => last_trigger = Some(trigger),
            None => return response,
        }
        last = Some(response);
        hop += 1;
    }
}

// ---------------------------------------------------------------------------
// 流式
// ---------------------------------------------------------------------------

/// 流式路径：先换号直到拿到 2xx，再把响应体交给转换任务。
#[allow(clippy::too_many_arguments)]
async fn stream_chat(
    state: &TraeGatewayState,
    body: &Bytes,
    default_model: &str,
    model: &str,
    chat_id: &str,
    max_rotate: usize,
    preferred_uid: &str,
    sticky_key: Option<&str>,
    // 入口端点：同时决定**出站协议**（`/v1/messages` 要额外做 Anthropic 转换）
    // 与**日志端点名** —— 合成一个参数，两者就不可能对不上。
    //
    // 上游链路完全相同（SOLO SSE → OpenAI SSE），差别只在最后一层封装 ——
    // 所以用参数而不是另写一个函数：另写一份会让换号 / 冷却 / 粘性 / 日志
    // 全部再实现一遍，两处迟早分家。
    endpoint: &'static str,
    started: Instant,
    variant: TraeVariant,
    // ★ issue #76：本密钥（白名单 / 配额 / 记账）。
    key: Option<&crate::apikey::KeyActor>,
    // Responses 入口声明的 custom（freeform）工具名：只有 `/v1/responses` 用得上，
    // chat / messages 传 `None`（它们没有这个概念）。由**原始**请求体取，
    // 转换后 `type` 已被降级成 function，拿不回来。
    responses_custom: Option<std::sync::Arc<HashSet<String>>>,
) -> Response {
    // ★ issue #76：允许名单整轮算一次，选号与失败兜底共用（理由见 `attempt_once`）。
    let allowed = key.and_then(|actor| allowed_uids_for(&actor.policy, variant));
    let mut tried: HashSet<String> = HashSet::new();
    let mut last: Option<UpstreamFailure> = None;

    for _ in 0..max_rotate {
        match attempt_once(
            state,
            body,
            default_model,
            &mut tried,
            preferred_uid,
            sticky_key,
            variant,
            allowed.as_ref(),
        )
        .await
        {
            None => break,
            Some(AttemptResult::Failed { failure, .. }) => last = Some(failure),
            Some(AttemptResult::Ok {
                account,
                response,
                capture,
            }) => {
                let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);
                let task_state = state.clone();
                let task_model = model.to_string();
                let task_chat_id = chat_id.to_string();
                // ★ issue #76：Key 要进 `tokio::spawn`（'static）⇒ 先 clone 一份。
                let task_key = key.cloned();
                tokio::spawn(async move {
                    let outcome = sse::stream_convert(
                        response,
                        tx,
                        &task_chat_id,
                        &task_model,
                        capture.as_ref(),
                    )
                    .await;
                    settle(
                        &task_state,
                        &account,
                        &task_model,
                        true,
                        started,
                        outcome.error,
                        outcome.usage,
                        endpoint,
                        variant,
                        task_key.as_ref().map(|actor| actor.key_id.as_str()),
                        task_key.as_ref().map(|actor| actor.scope),
                    )
                    .await;
                    /* ★ issue #76：按密钥 token 记账 —— 挂在**流结束**这个同一时点
                    （与 `settle` 并列）：请求前不知道用量，记了就是假账。
                    `is_meaningful()` = 上游没报用量（全 0）⇒ 不记，别写 0 样本
                    （会把「本周期已用」压低，让配额晚触发）。 */
                    if let Some(actor) = task_key.as_ref() {
                        if outcome.usage.is_meaningful() {
                            // `record_tokens` 内部判 0 跳过。
                            actor.record_tokens(
                                outcome.usage.total,
                                buddy_switch_core::modules::config::now_ms(),
                            );
                        }
                    }
                });
                return if is_anthropic_endpoint(endpoint) {
                    anthropic_sse_response(rx, model)
                } else if is_responses_endpoint(endpoint) {
                    // Responses 事件流：内层是同一条 OpenAI SSE，逐事件重编。
                    responses_sse_response(rx, model, responses_custom.clone())
                } else {
                    sse_response(rx)
                };
            }
        }
    }

    // 没拿到任何可用上游：这里**还没发过响应头**，可以正常返回 JSON 错误。
    let failure = last.unwrap_or_else(|| no_account_failure_sync(state, variant, allowed.as_ref()));
    settle_failure(
        state,
        model,
        true,
        started,
        &failure,
        endpoint,
        variant,
        key.map(|actor| actor.key_id.as_str()),
        key.map(|actor| actor.scope),
    )
    .await;
    openai_error(
        StatusCode::from_u16(failure.status).unwrap_or(StatusCode::BAD_GATEWAY),
        &failure.code,
        &failure.message,
    )
}

// ---------------------------------------------------------------------------
// 非流式
// ---------------------------------------------------------------------------

/// 非流式路径：本地聚合。**流内错误也换号**——聚合完成前客户端一个字节都没收到。
#[allow(clippy::too_many_arguments)]
async fn aggregate_payload(
    state: &TraeGatewayState,
    body: &Bytes,
    default_model: &str,
    model: &str,
    chat_id: &str,
    max_rotate: usize,
    preferred_uid: &str,
    sticky_key: Option<&str>,
    endpoint: &'static str,
    started: Instant,
    variant: TraeVariant,
    // ★ issue #76：本密钥（白名单 / 配额 / 记账）。
    key: Option<&crate::apikey::KeyActor>,
) -> Result<Value, UpstreamFailure> {
    // ★ issue #76：本密钥的允许名单，**整轮算一次** —— 选号与失败兜底都要用同一份
    //   （兜底文案必须按名单口径说事实，否则用户会被指去修名单外的账号）。
    let allowed = key.and_then(|actor| allowed_uids_for(&actor.policy, variant));
    let mut tried: HashSet<String> = HashSet::new();
    let mut last: Option<UpstreamFailure> = None;

    for _ in 0..max_rotate {
        let (account, response, capture) = match attempt_once(
            state,
            body,
            default_model,
            &mut tried,
            preferred_uid,
            sticky_key,
            variant,
            allowed.as_ref(),
        )
        .await
        {
            None => break,
            Some(AttemptResult::Failed { failure, .. }) => {
                last = Some(failure);
                continue;
            }
            Some(AttemptResult::Ok {
                account,
                response,
                capture,
            }) => (account, response, capture),
        };

        let (payload, error, usage) =
            sse::aggregate(response, chat_id, model, capture.as_ref()).await;

        match (payload, error) {
            (Some(payload), None) => {
                settle(
                    state,
                    &account,
                    model,
                    false,
                    started,
                    None,
                    usage,
                    endpoint,
                    variant,
                    key.map(|actor| actor.key_id.as_str()),
                    key.map(|actor| actor.scope),
                )
                .await;
                return Ok(payload);
            }
            (_, Some((code, message))) => {
                // 流内错误：冷却该账号 → 换下一个账号整轮重来（响应头还没发）。
                last = Some(record_stream_error(state, &account, code, &message, variant).await);
            }
            _ => {
                // 既没有 payload 也没有错误：上游给了个空流。按 5xx 处理并换号。
                last = Some(
                    record_stream_error(state, &account, 0, "上游返回空事件流", variant).await,
                );
            }
        }
    }

    let failure = last.unwrap_or_else(|| no_account_failure_sync(state, variant, allowed.as_ref()));
    settle_failure(
        state,
        model,
        false,
        started,
        &failure,
        endpoint,
        variant,
        key.map(|actor| actor.key_id.as_str()),
        key.map(|actor| actor.scope),
    )
    .await;
    Err(failure)
}

/// 非流式 OpenAI 端点：把 [`aggregate_payload`] 的结果包成 OpenAI 形状。
#[allow(clippy::too_many_arguments)]
async fn aggregate_chat(
    state: &TraeGatewayState,
    body: &Bytes,
    default_model: &str,
    model: &str,
    chat_id: &str,
    max_rotate: usize,
    preferred_uid: &str,
    sticky_key: Option<&str>,
    started: Instant,
    variant: TraeVariant,
    // ★ issue #76：本密钥（白名单 / 配额 / 记账）。
    key: Option<&crate::apikey::KeyActor>,
) -> Response {
    match aggregate_payload(
        state,
        body,
        default_model,
        model,
        chat_id,
        max_rotate,
        preferred_uid,
        sticky_key,
        ENDPOINT_CHAT,
        started,
        variant,
        key,
    )
    .await
    {
        Ok(payload) => {
            // ★ issue #76：非流式同样按密钥记 token（payload 是 OpenAI 形状，usage 在内）。
            record_key_tokens(state, key, &payload);
            json_response(StatusCode::OK, payload)
        }
        Err(failure) => openai_error(
            StatusCode::from_u16(failure.status).unwrap_or(StatusCode::BAD_GATEWAY),
            &failure.code,
            &failure.message,
        ),
    }
}

// ---------------------------------------------------------------------------
// 选号与出站
// ---------------------------------------------------------------------------

/// Anthropic 请求是否要流式响应。
///
/// ★ 必须读**原始** Anthropic 请求体，不能读 [`anthropic::to_upstream_request`] 的产物：
/// 后者会把 `stream` **强制为 `true`**（上游只接受流式）⇒ 拿它判断的话，
/// 非流式客户端会被当成流式，收到一条事件流而不是 JSON。
fn anthropic_wants_stream(parsed: &Value) -> bool {
    parsed
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 由请求体、产品线与模型派生**会话粘性键**；客户端未给会话 id 时返回 `None`。
///
/// ★ 返回 `None` 时调用方必须**整段跳过**粘性 —— 不得退化成轮级键：那会造出一个
/// 永不命中的绑定，让「粘性会话数」看起来非 0 却毫无作用
/// （见 [`sticky::sticky_key`] 的文档）。
///
/// 变体进键是必须的：两条产品线的账号库**分家**，粘到一起会指向另一条线的账号。
fn sticky_key_of(parsed: &Value, variant: TraeVariant, model: &str) -> Option<String> {
    session_headers::conversation_id_of(parsed)
        // ★ 必须 trim：`conversation_id_of` 只排除**空串**，纯空白（`"   "`）会漏过来。
        // 直接拿它当键的话，所有「填了空白」的客户端会**共用同一个绑定**
        // （表现为不同对话互相串号），而且不报错。WorkBuddy 侧的 `sticky_key_of` 同样 trim。
        .map(|conversation| conversation.trim().to_string())
        .filter(|conversation| !conversation.is_empty())
        .map(|conversation| sticky::sticky_key(variant.as_str(), model, &conversation))
}

/// 合并两个偏好来源，得到本次选号的偏好 uid。
///
/// 优先级：**显式指定 > 会话粘性**。
///
/// 为什么显式指定优先：粘性记录的是「上次成功的账号」，是一个**陈旧**的观察；
/// 而 `preferred_uid` 是用户此刻的明确要求。若让粘性压过它，用户改完设置后会看到
/// 「下一轮仍走旧账号」—— 表现为设置不生效。
///
/// ★ 本函数只决定「优先试谁」，**不保证一定用谁**：两者都不可用时由
/// [`crate::pool::pick_with_preference`] 回落到自动择优。粘性是优化而非约束。
fn resolve_preference<'a>(explicit_uid: &'a str, sticky_uid: Option<&'a str>) -> Option<&'a str> {
    if explicit_uid.is_empty() {
        sticky_uid
    } else {
        Some(explicit_uid)
    }
}

/// 一次出站尝试的结果。
enum AttemptResult {
    /// 上游返回 2xx（响应体尚未读取）。
    Ok {
        account: PickedTraeAccount,
        response: reqwest::Response,
        /// 抓取旁路句柄（诊断用）：`Some` 表示本次尝试开着抓取，编号已生成、
        /// 请求对已落盘；响应消费方接着用同一编号追加原始流。
        capture: Option<Capture>,
    },
    /// 出站失败：已分类、已写回冷却文件。
    Failed {
        #[allow(dead_code)]
        account: PickedTraeAccount,
        failure: UpstreamFailure,
    },
}

/// 把 Key 策略的账号范围展开成 uid 集合（`None` = 全部账号，不过滤）。
///
/// ★ 与 WB 侧 `allowed_uids_for` 同构，但**分组字段不同**：
/// Trae 的分组是 `groupId`（id），WB 是自由文本 `groupName`。账号库按 variant 分家
/// （两条产品线的账号库本就独立），所以展开时按 `variant` 读。
fn allowed_uids_for(
    policy: &crate::apikey::KeyPolicy,
    variant: TraeVariant,
) -> Option<HashSet<String>> {
    // ★ 安全审计 P1-7：判定与展开由 `apikey::scope_allows` **唯一提供**。
    //   这里只负责取 `(uid, groupId)` —— Trae 的分组是**独立文件**
    //   （`user_id -> group_id` 的 `membership` 映射），不是账号记录上的字段。
    //
    //   `chain(membership)` 是**必须**的：membership 里可能有账号库里已不存在的 uid
    //   （账号被删但分组映射还没清），改造前那份实现就是直接遍历 membership 的，
    //   只遍历 `accounts` 会静默缩小授权范围。
    let groups = buddy_switch_core::modules::trae::account::load_groups_for(variant);
    let from_accounts = buddy_switch_core::modules::trae::account::load_accounts_for(variant)
        .accounts
        .into_iter()
        .filter_map(|account| {
            let uid = account.user_id?;
            let group = groups.membership.get(&uid).cloned();
            Some((uid, group))
        });
    let from_membership = groups
        .membership
        .iter()
        .map(|(uid, group)| (uid.clone(), Some(group.clone())));
    crate::apikey::allowed_uids_for_policy(policy, from_accounts.chain(from_membership))
}

/// 非流式路径的按密钥 token 记账（issue #76）。
///
/// 从 OpenAI 形状的 `payload.usage.total_tokens` 取数；缺失 / 0 ⇒ **不记**
/// （与流式 `is_meaningful()` 同一判据：写 0 样本会让配额晚触发）。
fn record_key_tokens(
    // 不再需要 `state`：账本从 actor 取（`KeyActor` 自带 `usage`）。
    _state: &TraeGatewayState,
    key: Option<&crate::apikey::KeyActor>,
    payload: &Value,
) {
    let Some(actor) = key else { return };
    let total = payload
        .get("usage")
        .and_then(|usage| usage.get("total_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // `record_tokens` 内部判 0 跳过（不写 0 样本）。
    actor.record_tokens(total, buddy_switch_core::modules::config::now_ms());
}

/// 选号 + 出站一次。`tried` 在内部累加，调用方反复调用即可自动换号。
///
/// 返回 `None` 表示**已经挑不出新账号**（不是失败，是没得试了）。
#[allow(clippy::too_many_arguments)]
async fn attempt_once(
    state: &TraeGatewayState,
    body: &Bytes,
    default_model: &str,
    tried: &mut HashSet<String>,
    preferred_uid: &str,
    sticky_key: Option<&str>,
    variant: TraeVariant,
    // ★ issue #76：本密钥的允许名单（`None` = 不过白名单）。由调用方算好传进来 ——
    //   失败兜底（`no_account_failure_sync`）也要用它，两处必须同一份。
    allowed: Option<&HashSet<String>>,
) -> Option<AttemptResult> {
    // 先读粘性（**不持 pools 锁**）：两把锁不嵌套 —— 既不延长池的持锁时间，
    // 也避免日后有人以相反顺序取锁造成死锁。
    let sticky_uid = state.sticky_uid(sticky_key).await;

    let picked = {
        let mut pools = state.pools.lock().await;
        let pool = pools
            .entry(variant)
            .or_insert_with(|| TraePool::for_variant(variant));
        // 每次选号前重新同步：另一个入口（签到页 / 桌面端）可能刚写了冷却或刷新了积分。
        pool.sync_for(variant);
        // 偏好优先级：**显式指定 > 会话粘性 > 自动择优**（见 [`resolve_preference`]）。
        // 两者都不可用时 `pick_with_preference` 会自行回落到自动择优，绝不拒绝服务。
        let preferred = resolve_preference(preferred_uid, sticky_uid.as_deref());
        // 白名单：由调用方展开后传入（分组展开只在 `scope_groups` 非空时读一次文件）。
        pool.pick(now_secs(), tried, preferred, allowed)
    }?;
    tried.insert(picked.uid.clone());

    // 抓取（诊断旁路）：开关打开时为**本次尝试**建句柄（目录解析 + 编号各一次）。
    // 关闭时连目录都不碰 —— 热路径上零 IO 开销。编号随 `AttemptResult::Ok`
    // 交给响应消费方，同一编号继续追加原始流（请求与响应靠它配对）。
    let capture = if state.config_snapshot().await.capture_enabled {
        Some(Capture::new(variant))
    } else {
        None
    };

    match chat_once_for_account(
        state,
        &picked,
        variant,
        default_model,
        body,
        capture.as_ref(),
    )
    .await
    {
        Ok(response) => {
            // 会话粘性：**只在成功之后**绑定，让下一轮优先复用这个账号。
            state.bind_sticky(sticky_key, &picked.uid).await;
            Some(AttemptResult::Ok {
                account: picked,
                response,
                capture,
            })
        }
        Err((status, detail)) => {
            let kind = classify_http(status, &detail);
            let failure = UpstreamFailure {
                // 传输层错误没有 HTTP 状态码，对客户端统一报 502（网关上游不可达）。
                status: if status == 0 { 502 } else { status },
                code: if status == 0 {
                    "upstream_unreachable".to_string()
                } else {
                    kind.as_str().to_string()
                },
                message: format!(
                    "账号「{}」上游失败（HTTP {}）：{}",
                    picked.name,
                    if status == 0 {
                        "—".into()
                    } else {
                        status.to_string()
                    },
                    detail
                ),
            };
            {
                let mut pools = state.pools.lock().await;
                pools
                    .entry(variant)
                    .or_insert_with(|| TraePool::for_variant(variant))
                    .apply_error(&picked.uid, kind, &failure.message);
            }
            *state.last_error.write().await = Some(failure.message.clone());
            // 会话粘性：这个账号刚失败 ⇒ 立刻**解绑**，下一轮换号重新绑。
            // 粘性是优化而非约束，绝不能因为粘性而反复撞同一堵墙。
            state.unbind_sticky(sticky_key).await;
            Some(AttemptResult::Failed {
                account: picked,
                failure,
            })
        }
    }
}

/// 对**单个账号**发**一次** `llm_utils_chat`（含请求体改写），**不换号、不重试、不写池**。
///
/// ## 为什么单独立一个入口（限流探测的硬约束）
///
/// [`chat_for_variant`]（经 [`stream_chat`] / [`aggregate_payload`] → [`attempt_once`]）
/// 自带 `max_rotate` **换号轮换**，且失败时会 `apply_error` **写回池冷却** —— 用它探测
/// 会把结果**归因到别的账号**上，还会「自己把号打成冷却」。故把「拿到选定账号后、
/// 真正发一次请求」的那一层（**改写请求体 + 出站**）抽出来：调用方（中继 / 探测）
/// 自己决定要不要换号、要不要写池；本函数只发一次、**只读**。
///
/// ## 行为与抽取前**逐字不变**（纯重构）
///
/// 原 [`attempt_once`] 里的两步（[`payload::prepare_llm_chat_body`] + [`send_llm_chat`]）
/// 原样搬进来，无任何逻辑改动；`attempt_once` 改为调本函数 ⇒ 真实流量行为逐字不变。
///
/// ## 抓取旁路
///
/// `capture` 为 `Some` 时，改写完成后立刻落盘**请求对**（入口体 + 改写后上游体）：
/// 两者形状可比对，是排查「上游为什么不认」的第一手材料。探测路径传 `None`。
pub(crate) async fn chat_once_for_account(
    state: &TraeGatewayState,
    account: &PickedTraeAccount,
    variant: TraeVariant,
    default_model: &str,
    body: &[u8],
    capture: Option<&Capture>,
) -> Result<reqwest::Response, (u16, String)> {
    let converted = payload::prepare_llm_chat_body(
        body,
        variant,
        default_model,
        &account.uid,
        &account.device_id,
        &account.machine_id,
        state.max_mode_now(),
    );
    // 抓取（诊断旁路）：此时改写已完成、尚未发出 —— 请求对就在这一刻落盘。
    if let Some(capture) = capture {
        capture.request_pair(body, &converted);
    }
    send_llm_chat(state, account, &converted, capture).await
}

/// 发一次 `llm_utils_chat`：拿到响应头即返回，不读 body。
///
/// 头部逐字对齐参考实现（`x-ide-token` 用裸 JWT，**不带** `Cloud-IDE-JWT ` 前缀，
/// 前缀只在 `Authorization` 场景使用）。`accept-encoding` 显式写 `identity`：
/// 本 crate 的 reqwest 未开 gzip/br/zstd 特性，若上游压缩返回，读出来就是乱码。
///
/// `capture` 为 `Some` 时，非 2xx 的完整错误体额外落盘一份
/// （`_upstream_http_error.txt`）—— 回传给客户端的只有 300 字预览，
/// 排查时真正要看的是完整原文。
async fn send_llm_chat(
    state: &TraeGatewayState,
    account: &PickedTraeAccount,
    body: &[u8],
    capture: Option<&Capture>,
) -> Result<reqwest::Response, (u16, String)> {
    let url = format!("{}{TRAE_LLM_CHAT_PATH}", state.upstream);
    let trace_id = trace_id();

    let response = state
        .http
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "*/*")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::USER_AGENT, TRAE_USER_AGENT)
        .header(header::REFERER, &url)
        .header("x-ide-token", &account.jwt)
        .header("x-app-id", TRAE_APP_ID)
        .header("x-app-version", "default")
        .header("x-app-version-code", TRAE_IDE_VERSION_CODE)
        .header("x-ide-version", TRAE_IDE_VERSION)
        .header("x-ide-version-code", TRAE_IDE_VERSION_CODE)
        .header("x-ide-version-type", "stable")
        .header("x-device-type", "windows")
        .header("x-device-brand", "CREFG-XX")
        .header("x-device-cpu", "Intel")
        .header("x-device-id", &account.device_id)
        .header("x-machine-id", &account.machine_id)
        .header("x-os-version", "Windows 11 Home China")
        .header("request-traffic-type", "prod")
        .header("package-type", "stable_cn")
        .header("x-lgw-req-sdk-type", "3")
        .header("x-lscbd-aid", "787976")
        .header("x-lscbd-platform", "windows")
        .header("x-ss-dp", "787976")
        .header("app-version", TRAE_IDE_VERSION)
        .header("x-custom-trace-id", &trace_id[..16])
        .header(
            "x-flow-traceparent",
            format!("04-{}-{}-01", &trace_id[3..35], uuid_like()),
        )
        .header("x-tt-trace-id", &trace_id)
        .header("x-request-id", format!("req_{}", uuid_like()))
        .body(body.to_vec())
        .send()
        .await
        .map_err(|error| (0u16, describe_transport_error(&error)))?;

    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(response);
    }

    // 错误体可能很长（含堆栈），截断后再回传与落日志。
    let text = response.text().await.unwrap_or_default();
    // 抓取（诊断旁路）：完整错误体另存一份（回传客户端的只是截断预览）。
    if let Some(capture) = capture {
        capture.write("_upstream_http_error.txt", text.as_bytes());
    }
    Err((status, preview(&text, 300)))
}

// ---------------------------------------------------------------------------
// 收尾：写回池 + 落日志
// ---------------------------------------------------------------------------

/// 一次请求的收尾（流式与非流式共用）。
#[allow(clippy::too_many_arguments)]
async fn settle(
    state: &TraeGatewayState,
    account: &PickedTraeAccount,
    model: &str,
    stream: bool,
    started: Instant,
    error: Option<(i64, String)>,
    usage: TokenUsage,
    endpoint: &'static str,
    variant: TraeVariant,
    // ★ issue #76：调用方 Key id（写进日志，供「按密钥看用量」与定位）。
    key_id: Option<&str>,
    // ★ 归属台账（统一网关转发来的请求带的是网关的 Key）。
    key_scope: Option<&'static str>,
) {
    let latency_ms = started.elapsed().as_millis() as i64;

    let message = error.as_ref().map(|(code, detail)| {
        format!(
            "账号「{}」流内错误（code={code}）：{}",
            account.name,
            preview(detail, 200)
        )
    });

    if let Some((code, detail)) = &error {
        let kind = classify_solo(*code, detail);
        if kind != TraeErrKind::None {
            let reason = message.clone().unwrap_or_default();
            state
                .pools
                .lock()
                .await
                .entry(variant)
                .or_insert_with(|| TraePool::for_variant(variant))
                .apply_error(&account.uid, kind, &reason);
        }
    } else {
        state
            .pools
            .lock()
            .await
            .entry(variant)
            .or_insert_with(|| TraePool::for_variant(variant))
            .note_success(&account.uid);
    }

    /* ★ issue #129 §5.2 改动 4：流中途失败必须写**真实**状态码。
    改造前这里恒写 200（「SSE 已经以 200 开头发出」），于是「账号在流中途挂掉」
    在日志页显示成一次**成功**请求 —— 对外静默的代价绝不能是「对内也静默」。
    Qoder 侧早就是按错误帧回填 429/502 的，这里对齐它。 */
    let status = error
        .as_ref()
        .map(|(code, _)| super::pool::client_error_of(*code, "").status)
        .unwrap_or(200);
    state
        .record_request(
            endpoint,
            model,
            status,
            &account.uid,
            latency_ms,
            stream,
            usage.prompt,
            usage.completion,
            message,
            variant,
            key_id,
            key_scope,
        )
        .await;
}

/// 流内错误 → 写回冷却 + 构造可重试的失败。
async fn record_stream_error(
    state: &TraeGatewayState,
    account: &PickedTraeAccount,
    code: i64,
    detail: &str,
    variant: TraeVariant,
) -> UpstreamFailure {
    let kind = classify_solo(code, detail);
    let message = format!(
        "账号「{}」流内错误（code={code}）：{}",
        account.name,
        preview(detail, 200)
    );
    if kind != TraeErrKind::None {
        state
            .pools
            .lock()
            .await
            .entry(variant)
            .or_insert_with(|| TraePool::for_variant(variant))
            .apply_error(&account.uid, kind, &message);
    }
    *state.last_error.write().await = Some(message.clone());
    UpstreamFailure {
        status: 502,
        code: kind.as_str().to_string(),
        message,
    }
}

/// 落一条失败请求日志（用于「网关页能看到最近为什么全失败」）。
async fn settle_failure(
    state: &TraeGatewayState,
    model: &str,
    stream: bool,
    started: Instant,
    failure: &UpstreamFailure,
    endpoint: &'static str,
    variant: TraeVariant,
    key_id: Option<&str>,
    key_scope: Option<&'static str>,
) {
    state
        .record_request(
            endpoint,
            model,
            failure.status,
            "",
            started.elapsed().as_millis() as i64,
            stream,
            0,
            0,
            Some(failure.message.clone()),
            variant,
            key_id,
            key_scope,
        )
        .await;
}

/// 池里挑不出账号时的失败：带上**逐账号原因**，且**文案必须点名用户当前所在的区域**。
///
/// 只回一句「没有可用账号」等于把排查成本转嫁给用户；把 `diagnose()` 的结果拼进去，
/// 用户能立刻看出是「全部冷却中」还是「积分都过期了」。
///
/// 按区域取名的原因：Key 的**归属就是区域**（账号库、冷却、积分都按区域分家），
/// 账号管理页的切换器也是「国内版 / 国际版」两个 Tab。文案若按程序位取名
/// （`Trae Work` / `Trae`），用户会被指到界面上**根本不存在**的入口去加账号。
fn no_account_failure_sync(
    state: &TraeGatewayState,
    variant: TraeVariant,
    // ★ issue #76：白名单生效时诊断与文案都按**名单口径**（`None` = 不过白名单）。
    allowed: Option<&HashSet<String>>,
) -> UpstreamFailure {
    // 这里不能 await（调用点在 `unwrap_or_else` 里），用阻塞锁读一次内存视图即可：
    // 池状态在 `attempt_once` 里刚同步过，读到的不会比磁盘旧。
    let diagnose = match state.pools.try_lock() {
        Ok(pools) => pools
            .get(&variant)
            .map(|pool| pool.diagnose_for(now_secs(), allowed))
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    // 选中的池为空时，顺手看一眼**另一个区域**是否有账号：若另一个有而这个没有，
    // 用户多半是「账号加在了另一个区域、Key 却归属这个区域」——直接点出该怎么改。
    //
    // ⚠️ 必须遍历 [`TraeRegion::all`]，**不能**用 `TraeVariant::all()`：后者只有两条
    // **国内**程序位，于是「国内 Key + 国内库为空 + 国际版库有账号」这一情形
    // 永远得不到提示 —— 真正可能有账号的那条线根本不在候选里。提示要双向覆盖，
    // 否则国际版接入后，只在「国际版 Key 配国内账号」这一个方向上给指引。
    //
    // ★ 白名单生效时**不给这条提示**：名单是用户自己划的，让他「换个区域的 Key」
    //   等于绕开他自己的策略，反而添乱。
    let other_region_with_accounts = if diagnose.is_empty() && allowed.is_none() {
        TraeRegion::all()
            .into_iter()
            .filter(|candidate| *candidate != variant.region())
            .find(|candidate| !account::entries_for_region(*candidate).is_empty())
    } else {
        None
    };
    UpstreamFailure {
        status: 503,
        code: "no_healthy_account".to_string(),
        message: no_account_message(
            variant,
            &diagnose,
            other_region_with_accounts,
            // 名单生效时诊断里只剩名单内的账号 ⇒ 条数就是「名单覆盖了几个」。
            allowed.map(|_| diagnose.len()),
        ),
    }
}

/// 空池文案的唯一构造点（纯函数，便于单测钉住「区域名必须出现」）。
///
/// `other_region_with_accounts` 为「另一个区域有账号」时的提示对象；`None` 表示不提。
/// `scoped_allowed` 为 `Some(n)` 表示这把 Key 的白名单生效、名单覆盖 n 个账号
/// （issue #76）—— 此时必须点明「名单外的账号不会被使用」，否则用户会以为
/// 网关把池里其它号藏起来了。
///
/// 取名一律走 [`TraeVariant::region`] + [`TraeRegion::display_name`]：
/// `TraeWork` 与 `Trae` **同属国内区域**，因此两者产出的文案**逐字相同**。
fn no_account_message(
    variant: TraeVariant,
    diagnose: &[String],
    other_region_with_accounts: Option<TraeRegion>,
    scoped_allowed: Option<usize>,
) -> String {
    let name = variant.region().display_name();
    let base = if diagnose.is_empty() {
        format!(
            "「{name}」没有可用账号：请先在「账号管理」切到 {name} 添加账号，\
             或在「一键签到」页查看冷却原因"
        )
    } else {
        format!("「{name}」没有可用账号：{}", diagnose.join("、"))
    };
    let with_region = match other_region_with_accounts {
        Some(other) => {
            let other_name = other.display_name();
            format!("{base}（检测到 {other_name} 有账号：请改用归属 {other_name} 的 API Key）")
        }
        None => base,
    };
    match scoped_allowed {
        Some(count) => format!(
            "{with_region}（这把 API Key 的允许名单只覆盖 {count} 个账号，\
             名单外的账号不会被使用；可在「网关 → 密钥 → 权限」放宽名单）"
        ),
        None => with_region,
    }
}

/// 出站失败的统一形状。
struct UpstreamFailure {
    status: u16,
    code: String,
    message: String,
}

// ---------------------------------------------------------------------------
// 响应与工具
// ---------------------------------------------------------------------------

/// JSON 响应。
fn json_response(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// OpenAI 端点错误响应。
fn openai_error(status: StatusCode, code: &str, message: &str) -> Response {
    json_response(status, sse::error_body(code, message))
}

/// Anthropic 端点错误响应：`{"type":"error","error":{"type","message"}}`。
///
/// 形状与 WorkBuddy 网关的 `GatewayError::anthropic_body` **刻意保持一致** ——
/// 同一个 Anthropic 客户端连两条网关都要能读懂。Trae 模块不复用 `crate::routes`
/// 的类型（见模块文档），所以按同一形状自己构造。
fn anthropic_error(status: StatusCode, kind: &str, message: &str) -> Response {
    json_response(
        status,
        json!({
            "type": "error",
            "error": { "type": kind, "message": message },
        }),
    )
}

/// SSE 响应。
fn sse_response(rx: mpsc::Receiver<Result<Bytes, std::io::Error>>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(sse::body_from_receiver(rx))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Anthropic SSE 响应：把同一条 OpenAI SSE 逐事件转成 Anthropic 事件流。
///
/// `AnthropicSseStream` 要求内层 `S: Unpin`，而 `stream_from_receiver` 产出的是
/// `unfold`（含 async block，**不是** `Unpin`）⇒ 必须 `Box::pin` 包一层。
/// 少了这一步会得到一句与运行时毫无关系的 `Unpin` 约束报错。
fn anthropic_sse_response(
    rx: mpsc::Receiver<Result<Bytes, std::io::Error>>,
    model: &str,
) -> Response {
    let inner = Box::pin(sse::stream_from_receiver(rx));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(anthropic::AnthropicSseStream::new(
            inner,
            model.to_string(),
        )))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Responses SSE 响应：把同一条 OpenAI SSE 逐事件重编成 Responses 事件流。
///
/// 与 [`anthropic_sse_response`] 同构，`Box::pin` 的理由也一样
/// （`ResponsesSseStream` 要求内层 `Unpin`，而 `stream_from_receiver` 产出的是
/// `unfold`）。差别只在最后一层封装 —— 上游链路、换号、记账完全不区分端点。
///
/// ⚠️ Responses 的流**不以 `data: [DONE]` 结尾**（`response.completed` 即终止信号），
/// 这个差异由 `ResponsesSseStream` 内部处理，这里既不补也不截。
fn responses_sse_response(
    rx: mpsc::Receiver<Result<Bytes, std::io::Error>>,
    model: &str,
    custom: Option<std::sync::Arc<HashSet<String>>>,
) -> Response {
    let inner = Box::pin(sse::stream_from_receiver(rx));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(
            crate::protocol::responses::ResponsesSseStream::new(
                inner,
                model.to_string(),
                custom.unwrap_or_default(),
            ),
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn uuid_like() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// W3C traceparent 形态的上游追踪号：`00-<32hex>-<32hex>-01`（71 字符）。
///
/// 长度是硬约束：`x-custom-trace-id` 取 `[..16]`、`x-flow-traceparent` 取 `[3..35]`，
/// 越界会 panic。所以这里不做「短一点更省」的优化。
fn trace_id() -> String {
    format!("00-{}-{}-01", uuid_like(), uuid_like())
}

/// 截断到 `max` 个**字符**（不是字节），避免把多字节字符切一半。
fn preview(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max).collect();
    format!("{head}…（共 {} 字符）", trimmed.chars().count())
}

/// 传输层失败的**可读**描述。
///
/// 用户该看到「连接超时」而不是 `error sending request for url (...)`，
/// 但原始错误也不能丢——排查时它是唯一线索。
///
/// 实现在 core 的 `modules::net`，**本 crate 不再自己维护一份**：
/// 两个 crate 各写一遍必然出现「同一个故障两种说法」，而且 core 那份还会
/// 额外展开 `source` 链（DNS / TCP / TLS / 超时的具体原因），排查时才够用。
fn describe_transport_error(error: &reqwest::Error) -> String {
    buddy_switch_core::modules::net::describe_transport_error(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trae::TraeGatewayConfig;

    /// ★ 出口协议判据（纯函数）：三个端点各对应一种出站封装，**不能互相串**。
    ///
    /// 变异：把 `is_responses_endpoint` 写成恒 `false` ⇒ Responses 客户端会收到
    /// 原始 chat SSE（它的解析器认不出 `choices[].delta`）—— 这条会红。
    #[test]
    fn endpoint_predicates_pick_the_outbound_protocol() {
        assert!(is_anthropic_endpoint(ENDPOINT_MESSAGES));
        assert!(!is_anthropic_endpoint(ENDPOINT_RESPONSES));
        assert!(!is_anthropic_endpoint(ENDPOINT_CHAT));

        assert!(is_responses_endpoint(ENDPOINT_RESPONSES));
        assert!(!is_responses_endpoint(ENDPOINT_MESSAGES));
        // chat 必须原样透传：误判成任何一种转换都会让普通 OpenAI 客户端解析失败。
        assert!(!is_responses_endpoint(ENDPOINT_CHAT));
    }

    /// ★ 粘性表的接线往返：绑定后能查到、解绑后查不到、`None` 键**整段跳过**。
    ///
    /// 这一层是 `StickyTable` 的**薄封装**，但封装自己也会错 —— 最典型的是
    /// 「`None` 键被当成空串键写进表里」，那会让所有没有会话 id 的客户端
    /// **共用同一个绑定**（表现为「不同对话互相串号」），而且不报错。
    ///
    /// （构造 `TraeGatewayState` 无磁盘副作用：`RequestLog::new` 只读、Key store 只存路径。）
    #[tokio::test]
    async fn sticky_bind_unbind_round_trip_and_none_key_is_inert() {
        let state = TraeGatewayState::new(TraeGatewayConfig::default());
        let key = Some("trae_work|deepseek-v4-flash|conv-1");

        assert_eq!(state.sticky_uid(key).await, None, "未绑定时应查不到");

        state.bind_sticky(key, "uid-a").await;
        assert_eq!(state.sticky_uid(key).await.as_deref(), Some("uid-a"));

        state.unbind_sticky(key).await;
        assert_eq!(state.sticky_uid(key).await, None, "解绑后必须查不到");

        // `None` 键（客户端没给会话 id）⇒ 不绑、不查、不报错。
        assert_eq!(state.sticky_uid(None).await, None);
        state.bind_sticky(None, "uid-a").await;
        assert_eq!(
            state.sticky_uid(None).await,
            None,
            "None 键不得产生任何绑定"
        );
    }

    /// ★ 会话粘性键必须带**产品线**与**模型**。
    ///
    /// - 产品线：两条线的账号库分家，同名会话粘到一起会指向另一条线的账号；
    /// - 模型：同一会话切模型时应允许落到各自最合适的账号，否则「模型级限流只封锁该模型」
    ///   的优势会被粘性抵消。
    #[test]
    fn sticky_key_carries_variant_and_model() {
        let body = json!({"metadata": {"conversation_id": "conv-1"}});
        let work = sticky_key_of(&body, TraeVariant::TraeWork, "deepseek-v4-flash").unwrap();
        assert!(work.contains("conv-1"), "键里要带会话 id：{work}");

        assert_ne!(
            work,
            sticky_key_of(&body, TraeVariant::Trae, "deepseek-v4-flash").unwrap(),
            "不同产品线不得共享绑定"
        );
        assert_ne!(
            work,
            sticky_key_of(&body, TraeVariant::TraeWork, "glm-5.3").unwrap(),
            "不同模型不得共享绑定"
        );
        // 同一输入必须稳定（否则绑定永远命中不了）。
        assert_eq!(
            work,
            sticky_key_of(&body, TraeVariant::TraeWork, "deepseek-v4-flash").unwrap()
        );
    }

    /// ★ 客户端没给会话 id ⇒ 必须返回 `None`，调用方据此**整段跳过**粘性。
    ///
    /// **不得**退化成轮级键 —— 那会造出一个永不命中的绑定，
    /// 让「粘性会话数」看起来非 0 却毫无作用。
    #[test]
    fn sticky_key_is_none_without_a_conversation_id() {
        assert_eq!(sticky_key_of(&json!({}), TraeVariant::TraeWork, "m"), None);
        assert_eq!(
            sticky_key_of(
                &json!({"metadata": {"conversation_id": "   "}}),
                TraeVariant::TraeWork,
                "m"
            ),
            None,
            "空白串不算会话 id"
        );
        assert_eq!(
            sticky_key_of(&json!({"metadata": {}}), TraeVariant::TraeWork, "m"),
            None
        );
    }

    /// ★ 流式开关必须来自**原始** Anthropic 请求体。
    ///
    /// 回归背景（本用例写下之前刚修掉）：`to_upstream_request` 会把 `stream` 强制为
    /// `true`（上游只接受流式），若拿**转换后**的体判断，非流式客户端会收到事件流
    /// 而不是 JSON —— 而且不报错。
    ///
    /// 末段把「两个体在这一字段上必然不同」这个**前提**也钉住：否则哪天上游支持了
    /// 非流式，本用例会静默失去意义（仍绿，但已测不到东西）。
    #[test]
    fn anthropic_stream_flag_comes_from_the_raw_request() {
        assert!(!anthropic_wants_stream(&json!({})), "缺 stream ⇒ 非流式");
        assert!(!anthropic_wants_stream(&json!({"stream": false})));
        assert!(anthropic_wants_stream(&json!({"stream": true})));
        assert!(
            !anthropic_wants_stream(&json!({"stream": "yes"})),
            "非布尔值按非流式处理（不猜）"
        );

        let upstream = anthropic::to_upstream_request(&json!({"model": "m", "max_tokens": 1}));
        assert_eq!(
            upstream.get("stream"),
            Some(&json!(true)),
            "前提：上游请求体的 stream 被强制为 true —— 这正是不能拿它判断的原因"
        );
    }

    /// ★ 偏好优先级：**显式指定压过粘性**。    ///
    /// 粘性记录的是「上次成功的账号」，是一个陈旧观察；`preferred_uid` 是用户此刻的
    /// 明确要求。若让粘性压过它，用户改完设置后会看到「下一轮仍走旧账号」—— 设置不生效。
    #[test]
    fn explicit_preference_wins_over_sticky() {
        assert_eq!(resolve_preference("", Some("sticky")), Some("sticky"));
        assert_eq!(
            resolve_preference("", None),
            None,
            "两个来源都空 ⇒ 走自动择优"
        );
        assert_eq!(
            resolve_preference("explicit", Some("sticky")),
            Some("explicit"),
            "显式指定必须压过粘性"
        );
        assert_eq!(resolve_preference("explicit", None), Some("explicit"));
    }

    #[test]
    fn trace_id_has_the_length_the_header_slices_require() {
        let trace = trace_id();
        // 71 = "00-" + 32 + "-" + 32 + "-01"
        assert_eq!(trace.len(), 71, "trace id 长度是切片安全的前提");
        assert!(trace.starts_with("00-"));
        assert!(trace.ends_with("-01"));
        // 这两处切片一旦越界就是 panic，必须在这里钉住。
        assert_eq!(&trace[..16].len(), &16);
        assert_eq!(&trace[3..35].len(), &32);
        assert_ne!(trace, trace_id());
    }

    #[test]
    fn uuid_like_is_32_hex_chars() {
        let value = uuid_like();
        assert_eq!(value.len(), 32);
        assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn preview_truncates_by_char_not_by_byte() {
        assert_eq!(preview("abc", 10), "abc");
        assert_eq!(preview("  abc  ", 10), "abc");
        // 中文按字符截断：3 个字符不该被腰斩成乱码。
        let text = "一二三四五";
        let cut = preview(text, 3);
        assert!(cut.starts_with("一二三"));
        assert!(cut.contains("共 5 字符"));
        assert!(!cut.contains('\u{FFFD}'));
    }

    #[test]
    fn error_body_is_openai_shaped() {
        let response = openai_error(
            StatusCode::UNAUTHORIZED,
            "invalid_request_error",
            "缺少凭据",
        );
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = sse::error_body("no_healthy_account", "没有可用账号");
        assert_eq!(body["error"]["code"], "no_healthy_account");
        assert_eq!(body["error"]["type"], "api_error");
        assert_eq!(body["error"]["message"], "没有可用账号");
    }

    #[test]
    fn no_account_failure_message_is_actionable_and_region_aware() {
        // 这条文案是用户唯一能看到的排障线索：必须包含「下一步做什么」且**点名区域**。
        let cn = no_account_message(TraeVariant::TraeWork, &[], None, None);
        assert!(cn.contains("账号管理"), "{cn}");
        assert!(cn.contains("一键签到"), "{cn}");
        // ⚠️ 区域名现在是**客户端产品名**（`Trae CN` / `Trae`，2026-10-13 用户定）。
        // ★ 判据用**带「」的整名**（文案格式是 `「{name}」没有可用账号`）——
        //   裸 `contains("Trae")` 会因为 `Trae` 是 `Trae CN` 的子串而**恒真**。
        assert!(cn.contains("「Trae CN」"), "文案必须点名选中的区域: {cn}");

        // ⚠️ 契约在 2026-09-21 变了：网关的号池按**区域**分区（`pool.rs::sync_for`
        // 走 `entries_for_region`），Key 的归属也是区域，因此文案按区域取名。
        // `TraeWork` 与 `Trae` 是**程序位**、同属国内区域 ⇒ 两段文案必须**逐字相同**。
        // 若哪天又按程序位取名（用户会在账号管理页找不到那个入口），这条 `assert_eq!` 会红。
        let trae = no_account_message(TraeVariant::Trae, &[], None, None);
        assert_eq!(
            cn, trae,
            "国内两个程序位共用一本账号库，文案不得按程序位分叉"
        );

        // 国际版必须是另一个名字（否则用户分不清该去哪一版加账号）。
        let global = no_account_message(TraeVariant::Global, &[], None, None);
        assert!(global.contains("「Trae」"), "{global}");
        assert_ne!(cn, global, "两个区域的文案必须可区分: {cn} / {global}");

        // 有逐账号原因时，原因必须出现在文案里。
        let with_reason = no_account_message(
            TraeVariant::Trae,
            &["主号(7481920:冷却中,积分=0)".to_string()],
            None,
            None,
        );
        assert!(with_reason.contains("「Trae CN」"), "{with_reason}");
        assert!(with_reason.contains("冷却中"), "{with_reason}");

        // 「另一个区域有账号」的提示必须**双向**都给。
        // 方向①：国际版 Key + 只有国内账号（R8①的原始场景）。
        let hinted_global =
            no_account_message(TraeVariant::Global, &[], Some(TraeRegion::Cn), None);
        assert!(
            hinted_global.contains("「Trae」"),
            "选中的区域必须出现: {hinted_global}"
        );
        assert!(
            hinted_global.contains("检测到 Trae CN 有账号"),
            "另一个有账号的区域必须出现（否则用户不知道该往哪加 Key）: {hinted_global}"
        );
        // 方向②：国内版 Key + 只有国际版账号。**这条在修复前做不到** ——
        // 旧实现遍历的是 `TraeVariant::all()`（两条国内程序位），候选里根本没有国际版。
        let hinted_cn =
            no_account_message(TraeVariant::TraeWork, &[], Some(TraeRegion::Global), None);
        assert!(hinted_cn.contains("「Trae CN」"), "{hinted_cn}");
        assert!(
            hinted_cn.contains("检测到 Trae 有账号"),
            "另一个有账号的区域必须出现: {hinted_cn}"
        );

        // ★ issue #76：白名单生效时必须点明「名单外的账号不会被使用」——
        //   否则用户看到「没有可用账号」会以为网关把池里其它号藏起来了。
        let scoped = no_account_message(
            TraeVariant::Trae,
            &["主号(7481920:冷却中,积分=0)".to_string()],
            None,
            Some(1),
        );
        assert!(scoped.contains("允许名单只覆盖 1 个账号"), "{scoped}");
        assert!(scoped.contains("名单外的账号不会被使用"), "{scoped}");
        // 反向：没有白名单时**不得**出现名单提示（否则是在凭空编造策略）。
        assert!(!cn.contains("允许名单"), "无白名单时不得出现名单提示: {cn}");
    }

    #[test]
    fn status_query_variant_parsing_falls_back_to_default() {
        assert_eq!(
            parse_variant_query(Some("variant=trae_cn")),
            TraeVariant::Trae
        );
        assert_eq!(
            parse_variant_query(Some("variant=trae_work")),
            TraeVariant::TraeWork
        );
        // 缺失 / 未知 → 默认（TraeWork），老客户端行为不变。
        assert_eq!(parse_variant_query(None), TraeVariant::default());
        assert_eq!(parse_variant_query(Some("")), TraeVariant::default());
        assert_eq!(
            parse_variant_query(Some("variant=unknown")),
            TraeVariant::default()
        );
        assert_eq!(parse_variant_query(Some("days=7")), TraeVariant::default());
    }

    /// **实测探针**：上游认不认 TraeCode 的 `function` 与模型名（issue #4「看得见、调不动」）。
    ///
    /// # 为什么必须真发请求
    ///
    /// 网关发上游的 `config_name` / `model_name` **无法从客户端本地推导**：
    /// TraeWork 的清单每条都带 `config_name`，而 **TraeCode 的清单里根本没有这个字段**
    /// （本机 2026-09-30 逐条核对）。所以「`glm-5.3-flash` 该配哪个 `function`、
    /// 哪个上游模型名」只能问上游。本仓的既有裁定也是这一条：映射对不对**只能实测**。
    ///
    /// # 会真实调用上游（消耗少量积分）
    ///
    /// 6 次极短请求（prompt = `ping`）。**不写任何状态**：直接调 [`send_llm_chat`]，
    /// 不经过 [`attempt`] ⇒ 不写冷却、不落错误、不动粘性表。账号库只读。
    ///
    /// 运行：`cargo test -p buddy-switch-gateway --lib -- --ignored --nocapture probe_traecode`
    ///
    /// # 判读方式（★ 有对照，不是裸测）
    ///
    /// - `C1` 是**转录对照**：`solo_work_lite` + `glm-5.3` 是网关**当前已在用**的组合，
    ///   它必须成功 —— 若它也失败，说明探针本身（请求体 / 头 / 凭据）有问题，
    ///   后面几行**一律不可采信**（假阴性）。
    /// - `C2` 是**function 对照**：`glm-5.3` 也在 TraeCode 的 `chat_v3` 清单里。
    ///   若 C2 成功而 `V2` 失败 ⇒ 差别只在模型名；若 C2 也失败 ⇒ `chat_v3` 这个
    ///   function 名不对（或该账号没有 TraeCode 权益）。
    #[tokio::test]
    #[ignore = "实测：会真实调用上游并消耗少量积分；需本机有可用 Trae 账号"]
    async fn probe_traecode_model_names() {
        let state = TraeGatewayState::new(TraeGatewayConfig::default());

        // 真实 home 的 CN 池：只读选号（`sync_for` 只读账号库 / 冷却 / 积分）。
        let picked = {
            let mut pool = TraePool::for_variant(TraeVariant::TraeWork);
            pool.sync_for(TraeVariant::TraeWork);
            pool.pick(now_secs(), &HashSet::new(), None, None)
        };
        let Some(picked) = picked else {
            panic!("本机 CN 账号池没有可用账号，探针无法进行");
        };
        println!("[probe] 账号 {}（uid {}）", picked.name, picked.uid);

        // (标签, function, config_name, model_name)
        let cases: [(&str, &str, &str, &str); 13] = [
            (
                "C1 对照·已知可用",
                "solo_work_lite",
                "glm-5.3",
                "glm-5.3__dev",
            ),
            ("C2 function 对照", "chat_v3", "glm-5.3", "glm-5.3__dev"),
            (
                "V1 错配 function",
                "solo_work_lite",
                "glm-5.3-flash",
                "glm-5.3-flash__dev",
            ),
            (
                "V2 正配·猜 __dev",
                "chat_v3",
                "glm-5.3-flash",
                "glm-5.3-flash__dev",
            ),
            (
                "V3 无 __dev 后缀",
                "chat_v3",
                "glm-5.3-flash",
                "glm-5.3-flash",
            ),
            (
                "V4 换 solo_agent",
                "solo_agent",
                "glm-5.3-flash",
                "glm-5.3-flash__dev",
            ),
            // ★ 决定「`/v1/models` 是否过度宣传」：`Doubao-Seed-Code` 只出现在 TraeWork 的
            //   `solo_coder` 分组里，**不在** `solo_work_lite`。若它也 4001 ⇒ 当前对外清单
            //   里那些「不属于本 function 的模型」同样调不动（与 glm-5.3-flash 同一类缺陷）。
            (
                "V5 分组外的模型",
                "solo_work_lite",
                "Doubao-Seed-Code",
                "Doubao-Seed-Code__dev",
            ),
            // ★ 确认 `config_name = name` / `model_name = name + "__dev"` 这条规则能推广。
            (
                "V6 规则推广",
                "chat_v3",
                "qwen3.8-flash",
                "qwen3.8-flash__dev",
            ),
            // ★★ 第三轮：旧对外清单里那三个「既不在客户端 `solo_work_lite` 分组里、
            //   也没被实测过」的名字 —— 决定它们是**保留**还是**从清单里删掉**。
            (
                "X1 旧名 deepseek-v4-pro",
                "solo_work_lite",
                "DeepSeek-V4-Pro",
                "deepseek_v4_pro__dev",
            ),
            (
                "X2 旧名 glm-5-turbo",
                "solo_work_lite",
                "glm-5-turbo",
                "glm-5-turbo__dev",
            ),
            ("X3 旧名 glm-5", "solo_work_lite", "glm-5", "glm-5__dev"),
            // ★★ 第四轮：候选**新默认模型**（旧默认 `deepseek-v4-flash` 是客户端已不再
            //   提供的旧名，虽仍可用；换默认前必须先量新名字能不能过）。
            (
                "Y1 新默认候选",
                "solo_work_lite",
                "deepseek-v4.1-flash",
                "deepseek-v4.1-flash__dev",
            ),
            (
                "Y2 正式版候选",
                "solo_work_lite",
                "DeepSeek-V4-Flash-Official",
                "DeepSeek-V4-Flash-Official__dev",
            ),
        ];

        for (label, function, config_name, model_name) in cases {
            let src = format!(
                r#"{{"model":"{config_name}","messages":[{{"role":"user","content":"ping"}}]}}"#
            );
            let converted = payload::prepare_llm_chat_body(
                src.as_bytes(),
                TraeVariant::TraeWork,
                config_name,
                &picked.uid,
                &picked.device_id,
                &picked.machine_id,
                true,
            );
            // 只改这三个字段 —— 其余字段与真实流量逐字相同（复用同一个构造器）。
            let mut body: Value =
                serde_json::from_slice(&converted).expect("构造器产出必然是合法 JSON");
            body["function"] = serde_json::json!(function);
            body["config_name"] = serde_json::json!(config_name);
            body["model_name"] = serde_json::json!(model_name);
            let encoded = serde_json::to_vec(&body).expect("序列化");

            match send_llm_chat(&state, &picked, &encoded, None).await {
                Ok(mut response) => {
                    // 累积若干块，直到看见 `event:metadata`（上游认了）或 `event:error`（上游拒了）。
                    // ⚠️ 只看**首块**会把 `event:progress_notice`（排队中）误判成结论 —— 踩过。
                    let mut buffer = String::new();
                    let mut verdict = "<45s 内没等到 metadata/error>".to_string();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
                    while std::time::Instant::now() < deadline {
                        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                        match tokio::time::timeout(remaining, response.chunk()).await {
                            Ok(Ok(Some(bytes))) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));
                                if let Some(index) = buffer.find("event:error") {
                                    verdict = buffer[index..]
                                        .chars()
                                        .take(160)
                                        .collect::<String>()
                                        .replace('\n', "\\n");
                                    break;
                                }
                                if buffer.contains("event:metadata") {
                                    verdict = "event:metadata（上游接受）".to_string();
                                    break;
                                }
                            }
                            Ok(Ok(None)) => {
                                verdict = "<响应体结束，未出现 metadata/error>".to_string();
                                break;
                            }
                            Ok(Err(error)) => {
                                verdict = format!("<读 body 失败：{error}>");
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    println!(
                        "[{tag}] {label}  function={function} config_name={config_name} model_name={model_name}\n        HTTP {} | {verdict}",
                        response.status(),
                        tag = if verdict.starts_with("event:metadata") { "OK  " } else { "FAIL" },
                    );
                }
                Err((status, detail)) => println!(
                    "[FAIL] {label}  function={function} config_name={config_name} model_name={model_name}\n        HTTP {status} | {detail}"
                ),
            }
        }
    }

    /// **第二轮实测**：枚举本机 TraeWork `solo_work_lite` 分组里的**每一个**模型，
    /// 逐个问上游「认不认」—— 结论直接决定静态兜底清单 `MODEL_NAMES` 该收哪些名字。
    ///
    /// 与上一条的分工：那条钉**规则**（function 白名单 / 名字派生），这条钉**清单内容**。
    /// 两者都 `#[ignore]`、都真实调用上游、都不写任何状态。
    ///
    /// 运行：`cargo test -p buddy-switch-gateway --lib -- --ignored --nocapture probe_work_list`
    #[tokio::test]
    #[ignore = "实测：会真实调用上游并消耗少量积分；需本机有可用 Trae 账号"]
    async fn probe_work_list_names() {
        use buddy_switch_core::modules::trae::model_list::read_client_model_list;

        let state = TraeGatewayState::new(TraeGatewayConfig::default());
        let picked = {
            let mut pool = TraePool::for_variant(TraeVariant::TraeWork);
            pool.sync_for(TraeVariant::TraeWork);
            pool.pick(now_secs(), &HashSet::new(), None, None)
        };
        let Some(picked) = picked else {
            panic!("本机 CN 账号池没有可用账号");
        };

        let function = crate::trae::function_for(TraeVariant::TraeWork);
        let list = read_client_model_list(TraeVariant::TraeWork);
        let group = list.groups.iter().find(|group| group.function == function);
        let Some(group) = group else {
            panic!("本机客户端清单里没有 `{function}` 分组，无法枚举");
        };
        println!(
            "[probe] 账号 {} | 分组 `{function}` 共 {} 条 | 客户端数据目录 {:?}",
            picked.name,
            group.models.len(),
            list.data_dir
        );

        // 负对照：客户端清单里**不存在**的名字，必须被拒（否则说明探针没有鉴别力）。
        let mut names: Vec<String> = vec!["sagitta".to_string(), "aquila".to_string()];
        names.extend(group.models.iter().map(|model| model.name.clone()));

        for name in names {
            let (config_name, model_name) = payload::model_config(&name);
            let src =
                format!(r#"{{"model":"{name}","messages":[{{"role":"user","content":"ping"}}]}}"#);
            let converted = payload::prepare_llm_chat_body(
                src.as_bytes(),
                TraeVariant::TraeWork,
                &name,
                &picked.uid,
                &picked.device_id,
                &picked.machine_id,
                true,
            );
            let mut body: Value = serde_json::from_slice(&converted).expect("构造器产出合法 JSON");
            body["function"] = serde_json::json!(function);
            body["config_name"] = serde_json::json!(config_name);
            body["model_name"] = serde_json::json!(model_name);
            let encoded = serde_json::to_vec(&body).expect("序列化");

            match send_llm_chat(&state, &picked, &encoded, None).await {
                Ok(mut response) => {
                    let mut buffer = String::new();
                    let mut verdict = "超时".to_string();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
                    while std::time::Instant::now() < deadline {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        match tokio::time::timeout(remaining, response.chunk()).await {
                            Ok(Ok(Some(bytes))) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));
                                if let Some(index) = buffer.find("event:error") {
                                    verdict = buffer[index..].chars().take(90).collect();
                                    break;
                                }
                                if buffer.contains("event:metadata") {
                                    verdict = "OK".to_string();
                                    break;
                                }
                            }
                            Ok(Ok(None)) => break,
                            Ok(Err(error)) => {
                                verdict = format!("读 body 失败：{error}");
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    println!(
                        "[{:4}] {name:34} config_name={config_name:34} {verdict}",
                        if verdict == "OK" { "OK" } else { "FAIL" }
                    );
                }
                Err((status, detail)) => println!("[FAIL] {name:34} HTTP {status} | {detail}"),
            }
        }
    }
}
