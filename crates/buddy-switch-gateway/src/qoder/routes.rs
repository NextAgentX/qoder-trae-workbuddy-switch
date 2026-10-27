//! Qoder 网关路由：鉴权中间件 + 五类端点 + 共用中继。
//!
//! | 端点 | 说明 |
//! |:--|:--|
//! | `GET /health`（兼容 `/healthz`） | 免鉴权 |
//! | `GET /status` | 池状态 / Key 数 / 计数（管理面用） |
//! | `GET /v1/models` | 该 Key 归属区域的清单（**禁用过滤同源**） |
//! | `POST /v1/chat/completions` | OpenAI 协议入口 |
//! | `POST /v1/messages` | Anthropic 协议入口（协议层双向转换，中继**同一份**） |
//! | `POST /v1/responses` | OpenAI **Responses API**（入站翻译成 chat，出站转 Responses 事件流） |
//!
//! 三个 LLM 端点的差别只在协议层（[`Protocol`]）：中继 [`chat_for_region`] 与
//! WB/Trae 两侧同一纪律：**外层只有一份** ——
//! 出站改写 / 停用判定 / 选号换号 / 冷却 / 粘性 / 日志不能有第二份，否则漂移点
//! 恰恰是「换号 + 流内失败」这类最难复现的行为。

use std::collections::HashSet;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{Extension, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use buddy_switch_core::modules::product::scope_of_qoder_region;
use buddy_switch_core::modules::qoder::region::{self as qoder_region, QoderRegion};
use buddy_switch_core::modules::qoder::{api, cosy, PATH_CHAT_SSE, PATH_CHAT_SSE_ENCODED};

use super::pool::{classify_http_with_detail, QoderErrKind};
use super::{payload, sse, QoderGatewayState};

/// 鉴权通过后透传给 handler 的**归属区域**（决定用哪个账号池）。
#[derive(Debug, Clone, Copy)]
pub struct QoderKeyRegion(pub QoderRegion);

/// ★ issue #76：本次请求的**整条 Key**（策略 + id）—— 配额判定 / 账号白名单 / 记账。
#[derive(Clone)]
pub struct QoderKeyContext(pub std::sync::Arc<crate::qoder::apikey::QoderApiKeyRecord>);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// OpenAI 形态错误体。
fn openai_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({
            "error": { "message": message, "type": "api_error", "code": code }
        })),
    )
        .into_response()
}

/// Anthropic 形态错误体（`/v1/messages` 的客户端只认这个形状）。
fn anthropic_error(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({
            "type": "error",
            "error": { "type": kind, "message": message }
        })),
    )
        .into_response()
}

fn protocol_error(protocol: Protocol, status: StatusCode, code: &str, message: &str) -> Response {
    match protocol {
        // Responses 客户端（Codex 等）解析的就是 OpenAI 错误体（`error.message`）。
        Protocol::OpenAi | Protocol::Responses => openai_error(status, code, message),
        Protocol::Anthropic => anthropic_error(
            status,
            match code {
                "authentication_error" | "invalid_request_error" => code,
                _ => "api_error",
            },
            message,
        ),
    }
}

/// 下游协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAi,
    Anthropic,
    /// OpenAI **Responses API**（`/v1/responses`）—— 上游仍是 chat，差别在协议层双向转换。
    Responses,
}

// ---------------------------------------------------------------------------
// 基础端点
// ---------------------------------------------------------------------------

/// `GET /health`（免鉴权；`/healthz` 为兼容别名，见 [`crate::HEALTH_PATHS`]）。
pub async fn health() -> Response {
    /* 除 `ok` 外带一份**风控遥测**（排队 / 限流 / 客户端错，进程级累计）：
    运维据此判断「该加号、该降速、还是该查客户端」，不必翻日志逐条数。
    口径见 `qoder::risk`（只读、不落盘、重启清零）。 */
    axum::Json(json!({ "ok": true, "risk": super::risk::snapshot() })).into_response()
}

/// 鉴权中间件：健康检查（[`crate::HEALTH_PATHS`]）免鉴权；其余端点校验 `Bearer <API Key>`。
///
/// 错误体形状按**端点**分派（`/v1/messages` 用 Anthropic 形状）—— 给 Anthropic
/// 客户端 OpenAI 形状的话，它报的是「响应格式错误」而不是「凭据无效」，排查方向被带偏。
pub async fn bearer_auth(
    State(state): State<QoderGatewayState>,
    mut request: Request,
    next: Next,
) -> Response {
    if crate::HEALTH_PATHS.contains(&request.uri().path()) {
        return next.run(request).await;
    }
    let protocol = if request.uri().path() == "/v1/messages" {
        Protocol::Anthropic
    } else {
        Protocol::OpenAi
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

    let Some(key) = presented else {
        return protocol_error(
            protocol,
            StatusCode::UNAUTHORIZED,
            "invalid_request_error",
            "缺少凭据：请在 Authorization 头携带 `Bearer <API Key>`",
        );
    };
    match state.key_store.verify(key) {
        Some(record) => {
            request
                .extensions_mut()
                .insert(QoderKeyRegion(record.region));
            // ★ issue #76：整条 Key 也塞进扩展（配额 / 白名单 / 记账要用）。
            request
                .extensions_mut()
                .insert(QoderKeyContext(std::sync::Arc::new(record.clone())));
            next.run(request).await
        }
        None => protocol_error(
            protocol,
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "凭据无效或已吊销：请在「网关」页确认 API Key",
        ),
    }
}

/// `GET /status`。
pub async fn status(State(state): State<QoderGatewayState>) -> Response {
    let config = state.config_snapshot().await;
    let now = now_ms();
    let pools = state.pools.lock().await;
    let mut pool_views = serde_json::Map::new();
    for region in QoderRegion::all() {
        let entries = pools
            .get(&region)
            .map(|pool| pool.status_list(now))
            .unwrap_or_default();
        pool_views.insert(region.as_str().to_string(), Value::Array(entries));
    }
    drop(pools);
    axum::Json(json!({
        "enabled": config.enabled,
        "port": config.port,
        "defaultModel": config.default_model,
        "totalRequests": state.total_requests.load(std::sync::atomic::Ordering::Relaxed),
        "lastError": state.last_error.read().await.clone(),
        "startedAt": state.started_at,
        "keyCount": state.key_store.load().iter().filter(|key| !key.is_revoked()).count(),
        "pools": pool_views,
    }))
    .into_response()
}

/// `GET /v1/models`：该 Key 归属区域的清单 + 禁用过滤（**唯一判断点**）。
pub async fn models(
    State(state): State<QoderGatewayState>,
    Extension(key): Extension<QoderKeyRegion>,
) -> Response {
    let catalog = state.catalog_for(key.0).await;
    let scope = scope_of_qoder_region(key.0);
    let disabled = state.disabled.read().await;
    let filtered = payload::models_response(
        &catalog.models,
        disabled.get(scope),
        state.max_context_now(),
        scope,
    );
    drop(disabled);
    let mut body = filtered;
    if let Some(note) = catalog.note {
        body["note"] = Value::String(note);
    }
    axum::Json(body).into_response()
}

// ---------------------------------------------------------------------------
// 聊天中继
// ---------------------------------------------------------------------------

/// `POST /v1/chat/completions`。
pub async fn chat_completions(
    State(state): State<QoderGatewayState>,
    Extension(key): Extension<QoderKeyRegion>,
    // ★ issue #76：整条 Key（配额 / 白名单 / 记账）。
    key_ctx: Option<Extension<QoderKeyContext>>,
    body: Bytes,
) -> Response {
    // 变量名避开 `key`（那是 region 扩展）—— 两者是不同东西。
    let key_record = key_ctx.map(|Extension(ctx)| ctx.0);
    // ★ issue #76：收敛成 `KeyActor`（id + 策略 + 本产品线的账本）。
    let actor = key_record.map(|record| {
        std::sync::Arc::new(crate::apikey::KeyActor::new(
            record.id.clone(),
            record.policy(),
            state.key_usage.clone(),
            "qoder",
        ))
    });
    chat_for_region(state, key.0, body, Protocol::OpenAi, actor).await
}

/// `POST /v1/messages`。
pub async fn messages(
    State(state): State<QoderGatewayState>,
    Extension(key): Extension<QoderKeyRegion>,
    // ★ issue #76：整条 Key（配额 / 白名单 / 记账）。
    key_ctx: Option<Extension<QoderKeyContext>>,
    body: Bytes,
) -> Response {
    let key_record = key_ctx.map(|Extension(ctx)| ctx.0);
    // ★ issue #76：收敛成 `KeyActor`（id + 策略 + 本产品线的账本）。
    let actor = key_record.map(|record| {
        std::sync::Arc::new(crate::apikey::KeyActor::new(
            record.id.clone(),
            record.policy(),
            state.key_usage.clone(),
            "qoder",
        ))
    });
    chat_for_region(state, key.0, body, Protocol::Anthropic, actor).await
}

/// `POST /v1/responses`（OpenAI Responses API）。
///
/// Codex CLI 的 `wire_api = "responses"` 打的就是这个路径 —— Qoder 上游只有
/// chat SSE，故这里做**协议翻译**（入站 → chat，出站 → Responses 事件流 / 对象），
/// 中继仍是 [`chat_for_region`]（选号 / 换号 / 冷却 / 记账一份）。
pub async fn responses(
    State(state): State<QoderGatewayState>,
    Extension(key): Extension<QoderKeyRegion>,
    key_ctx: Option<Extension<QoderKeyContext>>,
    body: Bytes,
) -> Response {
    let key_record = key_ctx.map(|Extension(ctx)| ctx.0);
    // ★ issue #76：收敛成 `KeyActor`（id + 策略 + 本产品线的账本）。
    let actor = key_record.map(|record| {
        std::sync::Arc::new(crate::apikey::KeyActor::new(
            record.id.clone(),
            record.policy(),
            state.key_usage.clone(),
            "qoder",
        ))
    });
    chat_for_region(state, key.0, body, Protocol::Responses, actor).await
}

/// 把 Key 策略的账号范围展开成 uid 集合（`None` = 全部账号，不过滤）。
///
/// ★ 与 WB / Trae 侧同构；Qoder 的分组是**自由文本** `group`（与 WB 同款，
/// 不是 Trae 那种独立 membership 文件）。账号库按 region 分家。
fn allowed_uids_for(
    policy: &crate::apikey::KeyPolicy,
    region: QoderRegion,
) -> Option<HashSet<String>> {
    // ★ 安全审计 P1-7：判定与展开由 `apikey::scope_allows` **唯一提供**。
    //   这里只负责取 `(uid, 分组)` —— Qoder 的分组是账号记录上的自由文本 `group`。
    let accounts = buddy_switch_core::modules::qoder::account::load_accounts_for(region)
        .accounts
        .into_iter()
        .map(|account| (account.uid, account.group));
    crate::apikey::allowed_uids_for_policy(policy, accounts)
}

/// 一趟 Qoder chat 中继（**认证之后的全部逻辑**）—— 产品线入口与统一网关**共用这一份**。
pub async fn chat_for_region(
    state: QoderGatewayState,
    region: QoderRegion,
    body: Bytes,
    protocol: Protocol,
    // ★ issue #76：调用方的 Key（`None` = 无 Key 的调用 ⇒ 不判配额、不过白名单、不记账）。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let started = Instant::now();
    // ★ issue #76：配额判定（选号前）—— 超限直接 429，一步都不发。
    //   与 WB / Trae 同位置、同文案（复用 `GatewayError::KeyQuotaExceeded`）。
    if let Some(actor) = key.as_deref() {
        // 判定 + 请求数记账都在 `KeyActor` 里（四条产品线同一份实现）。
        if let Some(message) =
            actor.check_quota_and_count(buddy_switch_core::modules::config::now_ms())
        {
            return protocol_error(
                protocol,
                StatusCode::TOO_MANY_REQUESTS,
                "key_quota_exceeded",
                &message,
            );
        }
    }
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return protocol_error(
                protocol,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &format!("请求体不是合法 JSON：{error}"),
            )
        }
    };

    let config = state.config_snapshot().await;
    // 限额护栏（三条，`0` = 关闭）：一次请求取一次配置快照，循环里不再读锁。
    let quota_limits = config.quota_limits();
    // 非 OpenAI 入口先转成 OpenAI chat 形态（中继内部只认一份形态）。
    let openai_payload = match protocol {
        Protocol::OpenAi => parsed.clone(),
        // ★ Qoder 上游只读 message content 里的结构化 `image_url` parts ⇒
        // Anthropic 的 `image` block 必须转成多模态 part，不能只取文本
        // （否则图片被整条丢弃）。Trae / WB 未验证上游是否吃 parts，仍走
        // 不含图片的 `to_upstream_request`。
        Protocol::Anthropic => crate::protocol::anthropic::to_upstream_request_with_images(&parsed),
        Protocol::Responses => payload::responses_to_chat(&parsed),
    };
    let wants_stream = payload::wants_stream(&parsed);
    let catalog = state.catalog_for(region).await;

    // 模型：显式 → 配置默认 → 清单第一条（清单为空 ⇒ 明确报错，不猜）。
    let mut model = payload::model_of(&openai_payload, &config.default_model);
    if model.is_empty() {
        if let Some(first) = catalog.models.first() {
            model = first
                .get("id")
                .or_else(|| first.get("key"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
        }
    }
    if model.is_empty() {
        return protocol_error(
            protocol,
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "未指定模型，且模型清单为空：请显式传 `model`，或先在「账号」页导入一个账号（清单需要账号拉取）",
        );
    }

    // ★ 出口 ②：被禁用的模型直接拒绝（与 /v1/models 同源）。
    {
        let scope = scope_of_qoder_region(region);
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, scope, &model) {
            return protocol_error(
                protocol,
                StatusCode::FORBIDDEN,
                "model_disabled",
                &crate::disabled_models::disabled_message(scope, &model),
            );
        }
    }

    let meta = payload::meta_for(&catalog.models, &model, state.max_context_now());
    let allowed_tools = payload::allowed_tool_names(&openai_payload);
    /* Responses 的 **custom（freeform）工具**名只从**原始**请求体取：转换（`to_upstream_request`）
    之后它们已被降级成普通 function，名字还在，但 `type` 已经没了。 */
    let responses_custom =
        std::sync::Arc::new(crate::protocol::responses::custom_tool_names(&parsed));
    let sticky_key = payload::sticky_key_of(&parsed);
    let preferred = {
        let configured = config.preferred_uid.trim();
        if !configured.is_empty() {
            Some(configured.to_string())
        } else {
            state.sticky_uid(sticky_key.as_deref()).await
        }
    };
    let max_rotate = config.max_rotate.max(1);
    let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());

    // 出站请求体（每轮换号都要重建：`aliyun_user_type` 属账号上下文）。
    // `structured` 由网关显式决定（而不是在 `build_chat_body` 里再判一次）：
    // 自动降级需要「强制文本模式」重发，判定必须只有一处。
    let build_encoded = |user_type: &str, structured: bool| -> Result<String, String> {
        let body_value =
            api::build_chat_body_with_structured(&openai_payload, &meta, user_type, structured)?;
        Ok(cosy::qoder_encode(body_value.to_string().as_bytes()))
    };

    /* ★ issue #72：本次是否走结构化工具历史，以及「已知不被接受」的记忆。
    记忆命中 ⇒ 直接文本模式，不再每次先撞一次 400（见 `qoder::degrade`）。 */
    let mut structured = api::structured_tool_history_for(&openai_payload, &meta, region);
    if structured && super::degrade::is_unsupported(region, &meta.key) {
        structured = false;
    }

    let mut tried: HashSet<String> = HashSet::new();
    let mut failures: Vec<String> = Vec::new();
    // 本轮有多少次失败是「上游排队」（不是账号问题）。
    let mut queued = 0usize;
    // 每账号在途上限（0 = 关闭；见 `pool::max_inflight`）。
    let inflight_limit = super::pool::max_inflight();
    // 本轮有多少个号是被**限额护栏**拦下的（决定收尾文案与状态码）。
    let mut quota_blocked_count = 0usize;

    // ★ issue #76：本密钥的允许名单，**整轮算一次** —— 选号与收尾文案都要用同一份
    //   （收尾必须按名单口径说事实，否则用户会被指去修名单外的账号）。
    let allowed = key
        .as_deref()
        .and_then(|actor| allowed_uids_for(&actor.policy, region));

    for _ in 0..max_rotate {
        // 池随账号库同步（导入 / 刷新后即时生效），然后选号。
        // 选号连同**展示名**一起取：失败原因要给用户看名字，不是一串 uid
        //（护栏 / 在途 / 上游错误的文案统一用名字，混用会让同一条报错里出现两种身份）。
        let picked = {
            let mut pools = state.pools.lock().await;
            let pool = pools.entry(region).or_default();
            pool.sync_for(region);
            pool.pick(&model, &tried, preferred.as_deref(), allowed.as_ref())
                .map(|uid| {
                    let name = pool
                        .entries()
                        .find(|entry| entry.uid == uid)
                        .map(|entry| entry.name.clone())
                        .unwrap_or_else(|| uid.clone());
                    (uid, name)
                })
        };
        let Some((uid, display_name)) = picked else {
            break;
        };
        tried.insert(uid.clone());

        /* ★ 限额护栏（G1 余额地板 / G2 当日 token / G4 单模型 token / 402 停放）：
        拦下的号直接跳过并留下**可读原因**（不是静默少一个账号）。必须在取在途
        名额之前判 —— 名额是稀缺资源，给一个注定被拦的号占着会挤掉可用账号。 */
        if let Some(block) = super::quota::blocked(quota_limits, region, &uid, &model) {
            quota_blocked_count += 1;
            failures.push(format!("{}：{}", display_name, block.message()));
            continue;
        }

        /* ★ 在途闸门（默认关）：名额抢不到就**跳过这个号**，不排队等 ——
        网关在这里等会把客户端超时拖成「上游很慢」。 */
        let inflight = {
            let mut pools = state.pools.lock().await;
            pools
                .get_mut(&region)
                .and_then(|pool| pool.acquire(&uid, inflight_limit))
        };
        let Some(inflight) = inflight else {
            failures.push(format!(
                "{display_name}：该账号在途请求已满（{inflight_limit}）"
            ));
            continue;
        };

        let entry = {
            let pools = state.pools.lock().await;
            pools
                .get(&region)
                .and_then(|pool| pool.entries().find(|entry| entry.uid == uid).cloned())
        };
        let Some(entry) = entry else { continue };

        /* 同一账号最多发两次：结构化一次，被参数类拒绝后**文本模式**重发一次
        （`degrade::should_degrade` 只放行 attempt == 1）。换号不解决参数错。 */
        let mut attempt: u32 = 1;
        /* 「这次是降级重发」的标记：只有**重发成功**才把结论记进降级记忆 ——
        文本模式也 400 ⇒ 说明 400 与结构化无关（上游坏了），此时记住「结构化不被接受」
        就是**误判**，会让该模型 6 小时内都退回文本模式（正是 issue #72 要修的乱码）。 */
        let mut degrade_pending = false;
        'attempt: loop {
            let encoded = match build_encoded("", structured) {
                Ok(encoded) => encoded,
                Err(error) => {
                    return protocol_error(
                        protocol,
                        StatusCode::BAD_REQUEST,
                        "invalid_request_error",
                        &error,
                    )
                }
            };
            let account = cosy::CosyAccountInput {
                uid: &entry.uid,
                nickname: &entry.name,
                access_token: &entry.access_token,
                refresh_token: &entry.refresh_token,
                user_type: None,
                org_id: None,
                org_name: None,
            };
            let session = match cosy::shared_cache().get(&account) {
                Ok(session) => session,
                Err(error) => {
                    failures.push(format!("{}：{error}", entry.name));
                    break 'attempt;
                }
            };

            // 官方候选主机（国际版 api1→api2→api3）：签名只覆盖 path，换主机不影响签名。
            let headers = session.headers(&encoded, PATH_CHAT_SSE, Some(&meta.key), true);
            // ★ 单账号、单次发送（含候选主机 failover）——**不含换号轮换**（那在 `for _ in 0..max_rotate`
            //   外层）。抽成 [`chat_once_for_account`] 是为了让**限流探测**复用同一段发送逻辑而不误换号。
            let response = match chat_once_for_account(&state, region, &encoded, &headers).await {
                Ok(response) => response,
                Err(transport_error) => {
                    failures.push(format!("{}：{transport_error}", entry.name));
                    let mut pools = state.pools.lock().await;
                    if let Some(pool) = pools.get_mut(&region) {
                        pool.apply_error(&uid, QoderErrKind::Transient, &transport_error, &model);
                    }
                    break 'attempt;
                }
            };

            let status = response.status();
            if status.is_success() {
                if degrade_pending {
                    // 文本模式重发成功 ⇒ 结论坐实：这个 (区域, 模型) 的结构化历史不被接受。
                    super::degrade::note_rejected(region, &meta.key);
                }
                {
                    let mut pools = state.pools.lock().await;
                    if let Some(pool) = pools.get_mut(&region) {
                        pool.note_success(&uid);
                    }
                }
                state.bind_sticky(sticky_key.as_deref(), &uid).await;
                return if wants_stream {
                    // ★ `allowed_tools` 一并传进流式路径：泄漏回声守卫要按**声明过的工具名**
                    //   校验才还原（未声明的工具名不合成），与非流式同判据。
                    stream_relay(
                        state,
                        response,
                        protocol,
                        region,
                        &model,
                        &chat_id,
                        &uid,
                        started,
                        allowed_tools,
                        responses_custom.clone(),
                        inflight,
                        key.clone(),
                    )
                    .await
                } else {
                    aggregate_relay(
                        state,
                        response,
                        protocol,
                        region,
                        &model,
                        &chat_id,
                        &uid,
                        started,
                        allowed_tools,
                        responses_custom.clone(),
                        inflight,
                        key.clone(),
                    )
                    .await
                };
            }

            let status_code = status.as_u16();
            let detail: String = response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();

            /* ★ 自动降级（issue #72）：结构化历史被上游按**参数错**拒 ⇒ 记下这条结论，
            用同一账号、文本模式重发一次。
            刻意**不在这里**做三件事：不记请求日志（只记最终结果，否则一次请求记两条）、
            不调 `pool.apply_error`（不是账号的问题）、不计风控遥测（不是客户端参数的问题）。
            「结论」本身也**不在这里**落记忆 —— 要等文本模式重发成功（见上面 `degrade_pending`）。 */
            if super::degrade::should_degrade(status_code, structured, attempt) {
                structured = false;
                degrade_pending = true;
                attempt += 1;
                failures.push(format!(
                    "{}：HTTP {status_code} {}",
                    entry.name,
                    super::degrade::DEGRADED_NOTE
                ));
                continue 'attempt;
            }

            let kind = classify_http_with_detail(status_code, &detail);
            // 遥测：与分类**同一处**记一次（别在别处重复计数，见 `qoder::risk`）。
            match kind {
                QoderErrKind::Queued => super::risk::note_queued(),
                QoderErrKind::RateLimited => super::risk::note_rate_limited(),
                QoderErrKind::ClientError => super::risk::note_rejected(),
                QoderErrKind::BalanceExhausted => super::risk::note_exhausted(),
                QoderErrKind::AuthDead | QoderErrKind::Transient => {}
            }
            {
                let mut pools = state.pools.lock().await;
                if let Some(pool) = pools.get_mut(&region) {
                    pool.apply_error(&uid, kind, &detail, &model);
                }
            }
            if kind == QoderErrKind::Queued {
                queued += 1;
            }
            failures.push(format!("{}：HTTP {status_code} {detail}", entry.name));
            if !kind.should_rotate() {
                // 客户端参数错 —— 换号重试只是把同一份坏参数再喂给下一个账号。
                state
                    .record_request(
                        "chat",
                        region,
                        &model,
                        status_code,
                        &uid,
                        started.elapsed().as_millis() as i64,
                        wants_stream,
                        0,
                        0,
                        0.0,
                        Some(detail.clone()),
                        key.as_deref().map(|actor| actor.key_id.as_str()),
                        key.as_deref().map(|actor| actor.scope),
                    )
                    .await;
                return protocol_error(
                    protocol,
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                    "upstream_error",
                    &format!("上游拒绝（HTTP {status_code}）：{detail}"),
                );
            }
            break 'attempt;
        }
    }

    // 轮换耗尽 / 无可用账号。
    // ★ 若「全部失败都是排队」⇒ 明确说是**上游排队**，别让用户去查账号/凭据。
    let all_queued = queued > 0 && queued == failures.len();
    // ★ 若「全部失败都是被限额拦下」⇒ 说清是**限额**（去网关页调阈值 / 等重置），
    //   报 429 而不是 503 —— 503 会被读成「服务坏了」，这里是**预期行为**。
    let all_quota = quota_blocked_count > 0 && quota_blocked_count == failures.len();
    let message = if failures.is_empty() {
        // ★ 白名单生效时点明「名单覆盖几个、名单外不会被使用」（issue #76）：
        //   否则用户看到「没有可用账号」会去账号页反复导入，而问题在名单上。
        let scope_hint = match allowed.as_ref() {
            Some(only) => {
                let covered = {
                    let pools = state.pools.lock().await;
                    pools
                        .get(&region)
                        .map(|pool| {
                            pool.entries()
                                .filter(|entry| only.contains(&entry.uid))
                                .count()
                        })
                        .unwrap_or(0)
                };
                format!(
                    "；这把 API Key 的允许名单只覆盖 {covered} 个账号，\
                     名单外的账号不会被使用 —— 可在「网关 → 密钥 → 权限」放宽名单"
                )
            }
            None => String::new(),
        };
        format!(
            "{} 没有可用账号：请在「账号」页导入（账号需带可调用的访问令牌）{scope_hint}",
            region.display_name()
        )
    } else if all_quota {
        format!(
            "全部账号都被限额护栏拦下（{} 个）：这不是故障，请到「网关 → 账号池 → 限额护栏」调整阈值，或等下一个额度重置窗口；详情：{}",
            quota_blocked_count,
            failures.join("；")
        )
    } else if all_queued {
        format!(
            "上游排队中（{} 个账号都在排队）：这不是账号或凭据问题，稍后重试即可；详情：{}",
            queued,
            failures.join("；")
        )
    } else {
        format!(
            "全部账号都失败（{} 个）：{}",
            failures.len(),
            failures.join("；")
        )
    };
    let exhausted_status = if all_queued || all_quota {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    state
        .record_request(
            "chat",
            region,
            &model,
            exhausted_status.as_u16(),
            "",
            started.elapsed().as_millis() as i64,
            wants_stream,
            0,
            0,
            0.0,
            Some(message.clone()),
            key.as_deref().map(|actor| actor.key_id.as_str()),
            key.as_deref().map(|actor| actor.scope),
        )
        .await;
    let code = if all_queued {
        "upstream_queued"
    } else if all_quota {
        "quota_exceeded"
    } else {
        "no_available_account"
    };
    protocol_error(protocol, exhausted_status, code, &message)
}

/// 对**单个账号**发**一次**上游 chat 请求（含候选主机 failover），**不换号、不重试**。
///
/// ## 为什么单独立一个入口（限流探测的硬约束）
///
/// [`chat_for_region`] 自带 `max_rotate` **换号轮换** —— 用它探测会把结果**归因到别的账号**
/// （探测的「一格」必须只打一个 uid）。故把「一轮尝试里真正发一次请求」的那一层抽出来：
/// 调用方（中继 / 探测）自己决定要不要换号；本函数只发一次。
///
/// ## 行为与抽取前**逐字不变**（纯重构，不改逻辑）
///
/// 主机候选顺序（`upstream_override` 测试挂载点 → 官方 `gateway_candidates`）、`600s` 超时、
/// 逐个 header 注入、以及「逐个主机尝试、全部失败返回**最后一次**传输错误」都与原循环体一致。
/// 唯一差别：原循环体把 `hosts` 放在 `max_rotate` 循环里**每次重算** —— 而 `hosts` 与账号无关、
/// 重算无副作用，故移到本函数内等价。
pub(crate) async fn chat_once_for_account(
    state: &QoderGatewayState,
    region: QoderRegion,
    encoded: &str,
    headers: &[(String, String)],
) -> Result<reqwest::Response, String> {
    // 官方候选主机（国际版 api1→api2→api3）；签名只覆盖 path，换主机不影响签名。
    // `upstream_override` 是**测试挂载点**（打到本地 mock 上游），生产恒 `None`。
    let hosts: Vec<String> = match &state.upstream_override {
        Some(host) => vec![host.clone()],
        None => qoder_region::gateway_candidates(region)
            .iter()
            .map(|host| host.to_string())
            .collect(),
    };
    let mut transport_error = String::new();
    for host in hosts {
        // ★ 发送用 `?Encode=1` 的请求路径，签名仍用**无 query** 的 `PATH_CHAT_SSE`
        //   （见 `chat_once_for_account` 文档与 `core::qoder::PATH_CHAT_SSE_ENCODED`）。
        //   上游按 query 决定要不要先解码：裸路径 + 编码体 ⇒ 把自定义 base64 当 JSON
        //   解析 ⇒ HTTP 200 + `event:error`（真机取证 2026-10-17）。
        let url = format!("{host}{PATH_CHAT_SSE_ENCODED}");
        let mut request = state
            .http
            .post(&url)
            .timeout(std::time::Duration::from_secs(600))
            .body(encoded.to_string());
        for (name, value) in headers {
            request = request.header(name, value);
        }
        match request.send().await {
            Ok(response) => return Ok(response),
            Err(error) => transport_error = error.to_string(),
        }
    }
    Err(transport_error)
}

fn usage_counts(usage: &Value) -> (u64, u64) {
    let prompt = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completion = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (prompt, completion)
}

/// 本发消耗的 **credit**（上游 `usage.credits`，浮点；缺 `credits` 时回落
/// `original_credits` —— 某些模型只给后者）。
///
/// ⚠️ 这是 **G3 的计数来源**，也是日志里的 `credits`：拿不到就记 0（少记 = 晚触发，
/// 不会误拦）。
fn usage_credits(usage: &Value) -> f64 {
    usage
        .get("credits")
        .and_then(Value::as_f64)
        .or_else(|| usage.get("original_credits").and_then(Value::as_f64))
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(0.0)
}

/// 流式中继：驱动转换流 → mpsc → 响应体；**流结束回调里**记账（绝不提前写日志）。
#[allow(clippy::too_many_arguments)]
async fn stream_relay(
    state: QoderGatewayState,
    response: reqwest::Response,
    protocol: Protocol,
    region: QoderRegion,
    model: &str,
    _chat_id: &str,
    uid: &str,
    started: Instant,
    allowed_tools: Option<HashSet<String>>,
    responses_custom: std::sync::Arc<HashSet<String>>,
    // 在途名额：随流结束 / 客户端断开由 Drop 归还（见 `pool::InflightGuard`）。
    inflight: super::pool::InflightGuard,
    // ★ issue #76：本密钥（流结束时按 token 记账）。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let upstream = response.bytes_stream();
    let stream_model = model.to_string();
    let usage_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<Value>));
    // 上游错误帧留痕：流式账本在流结束才落，没有它就只能记成「200 + 0 token」。
    let error_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<sse::UpstreamError>));
    let state_for_task = state.clone();
    let uid = uid.to_string();
    let model_for_task = model.to_string();
    // ★ issue #76：Key 要进 `tokio::spawn`（'static）⇒ clone 一份 Arc。
    let key_for_task = key.clone();

    // 转换链：信封解包 → **泄漏回声守卫** → **DSML 调用块守卫** → `UsageTapStream`
    //        （旁路取 usage / 错误）→（Anthropic 入口再包一层）。
    // ⚠️ 三处顺序都是硬约束：
    //   ① 两道守卫都必须在 `UsageTapStream` **之内** —— 它们不改 `QoderOpenAiStream::new`
    //      的签名，而 `aggregate`（非流式）内部自建流、**不经过**这两层 ⇒ 结构上不可能
    //      被还原两次；
    //   ② DSML 守卫排在泄漏守卫**之后**：前者合成的帧已是结构化 `tool_calls`，后者据此
    //      判定「不再合成第二组」，顺序反了会各还原一遍；
    //   ③ `UsageTapStream` **必须在 Anthropic 转换之前** —— 转换后帧是 Anthropic 事件，
    //      取不到 OpenAI 的 `usage` 字段。
    let allowed_tools_shared = allowed_tools.map(std::sync::Arc::new);
    let tapped = sse::UsageTapStream::new(
        sse::DsmlGuardStream::new(
            sse::LeakGuardStream::new(
                sse::QoderOpenAiStream::new(upstream),
                allowed_tools_shared.clone(),
            ),
            allowed_tools_shared,
        ),
        usage_slot.clone(),
        error_slot.clone(),
    );
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    // ⚠️ 三个入口的协议转换都排在 `UsageTapStream` **之外** —— 转换后帧是 Anthropic 事件
    //    / Responses 事件，取不到 OpenAI 的 `usage` 字段。
    let converter: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send>,
    > = match protocol {
        Protocol::OpenAi => Box::pin(tapped),
        Protocol::Anthropic => Box::pin(crate::protocol::anthropic::AnthropicSseStream::new(
            tapped,
            stream_model.clone(),
        )),
        // Responses 事件流：`response.created` … `response.completed`（**不补 `[DONE]`**，
        // 上游那一条会被本层吞掉 —— 见 `protocol::responses::ResponsesSseStream`）。
        Protocol::Responses => Box::pin(crate::protocol::responses::ResponsesSseStream::new(
            tapped,
            stream_model.clone(),
            responses_custom,
        )),
    };

    tokio::spawn(async move {
        use futures_util::StreamExt;
        // 名额随本任务结束归还（流正常结束 / 客户端断开 / 转换器出错都会走到 Drop）。
        let _inflight = inflight;
        let mut converter = converter;
        while let Some(item) = converter.next().await {
            match item {
                Ok(frame) => {
                    if tx.send(Ok(frame)).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        // ★ 账本在**流结束**之后落（usage 只在末帧到达；提前写会记成 0）。
        let usage = usage_slot.lock().ok().and_then(|guard| guard.clone());
        let (prompt_tokens, completion_tokens, credits) = match &usage {
            Some(usage) => {
                let (prompt, completion) = usage_counts(usage);
                (prompt, completion, usage_credits(usage))
            }
            None => (0, 0, 0.0),
        };
        // 上游中途发过错误帧 ⇒ 记 502 + 详情，别写成「200 / 0 token / 无错误」
        // （那正是 issue #46 里最难排查的形态）。
        //
        // ★ **排队单列 429**：下游错误帧的 `code` 就是**客户端看到的那个码**
        //   （`sse::process_line` 对排队信号发 `code:429`），账本用它 ⇒ 日志与客户端
        //   永远一致。账本若写 502，日志就会把「上游在排队、稍后重试」说成
        //   「网关挂了」—— 2026-10-22 实测踩到（用户看到 502 以为服务坏了）。
        // ★ issue #76：按密钥记 token（**流结束**才记 —— 提前记就是假账）。
        //   拿不到用量（0）就不记：写 0 样本会把「本周期已用」压低，让配额晚触发。
        if let Some(actor) = key_for_task.as_ref() {
            // `record_tokens` 内部判 0 跳过（不写 0 样本）。
            actor.record_tokens(
                (prompt_tokens + completion_tokens) as u64,
                buddy_switch_core::modules::config::now_ms(),
            );
        }
        let upstream_error = error_slot.lock().ok().and_then(|guard| guard.clone());
        let (status, error) = match &upstream_error {
            Some(detail) => (
                if detail.code == 429 { 429u16 } else { 502u16 },
                Some(detail.message.clone()),
            ),
            None => (200u16, None),
        };
        state_for_task
            .record_request(
                "chat",
                region,
                &model_for_task,
                status,
                &uid,
                started.elapsed().as_millis() as i64,
                true,
                prompt_tokens as i64,
                completion_tokens as i64,
                credits,
                error,
                key_for_task.as_ref().map(|actor| actor.key_id.as_str()),
                key_for_task.as_ref().map(|actor| actor.scope),
            )
            .await;
    });

    sse::response_from_receiver(rx)
}

/// 非流式中继：聚合上游流 → 单个 completion（OpenAI / Anthropic 形状按入口分派）。
#[allow(clippy::too_many_arguments)]
async fn aggregate_relay(
    state: QoderGatewayState,
    response: reqwest::Response,
    protocol: Protocol,
    region: QoderRegion,
    model: &str,
    chat_id: &str,
    uid: &str,
    started: Instant,
    allowed_tools: Option<HashSet<String>>,
    responses_custom: std::sync::Arc<HashSet<String>>,
    // 非流式：函数结束即释放（含所有早退分支）。
    _inflight: super::pool::InflightGuard,
    // ★ issue #76：本密钥（聚合完成后按 token 记账）。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let aggregated = sse::aggregate(response.bytes_stream(), allowed_tools.as_ref()).await;
    let usage = aggregated.usage.clone();
    let (prompt_tokens, completion_tokens, credits) = match &usage {
        Some(usage) => {
            let (prompt, completion) = usage_counts(usage);
            (prompt, completion, usage_credits(usage))
        }
        None => (0, 0, 0.0),
    };
    // ★ 账本先定「这一请求到底成没成」再落：上游只要给过错误帧就不是 200，
    //   否则日志会显示「200 + 0 token + error:null」（issue #46 的隐身故障）。
    //
    //   **排队单列 429** —— 与客户端拿到的语义一致：下游错误帧的 `code` 就是
    //   客户端看到的那个码（排队 ⇒ 429），账本用它 ⇒ 日志与客户端永远一致。
    //   （上游**真**的 429 限流也是 429 —— 同样是「稍后重试」，归一没有歧义。）
    let (log_status, log_error) = match &aggregated.upstream_error {
        Some(detail) => (
            if aggregated.upstream_error_code == Some(429) {
                429u16
            } else {
                502u16
            },
            Some(detail.clone()),
        ),
        None => (200u16, None),
    };
    // ★ issue #76：非流式同样按密钥记 token（拿不到用量就不记）。
    if let Some(actor) = key.as_ref() {
        actor.record_tokens(
            (prompt_tokens + completion_tokens) as u64,
            buddy_switch_core::modules::config::now_ms(),
        );
    }
    state
        .record_request(
            "chat",
            region,
            model,
            log_status,
            uid,
            started.elapsed().as_millis() as i64,
            false,
            prompt_tokens as i64,
            completion_tokens as i64,
            credits,
            log_error,
            key.as_ref().map(|actor| actor.key_id.as_str()),
            key.as_ref().map(|actor| actor.scope),
        )
        .await;

    if let Some(error) = &aggregated.upstream_error {
        // 信封层排队（403 + `10605` / `isQueued`）：报 429 + 明确「不是账号问题」。
        if super::pool::queue_signal(error) {
            // 计数已在信封解包处记过一次（`sse::process_line`）⇒ 这里**不再记**，免得双计。
            return protocol_error(
                protocol,
                StatusCode::TOO_MANY_REQUESTS,
                "upstream_queued",
                &super::pool::queue_message(error),
            );
        }
        return protocol_error(protocol, StatusCode::BAD_GATEWAY, "upstream_error", error);
    }

    // 三个出口各一份形状；`chat.completion` 只在 OpenAi / Anthropic 分支构造（懒求值）。
    let chat = || chat_completion_of(&aggregated, chat_id, model, usage.clone());
    match protocol {
        // 非流式 Responses 对象：形状由 `protocol::responses` 唯一构造（与 WB 那条线同源）。
        Protocol::Responses => axum::Json(crate::protocol::responses::to_response_object_from(
            &crate::protocol::responses::ResponsesParts {
                content: &aggregated.content,
                reasoning: &aggregated.reasoning,
                tool_calls: &aggregated.tool_calls,
                model: aggregated.model.as_deref(),
                created_at: aggregated.created,
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
                custom: &responses_custom,
            },
            model,
        ))
        .into_response(),
        Protocol::OpenAi => axum::Json(chat()).into_response(),
        Protocol::Anthropic => {
            let message = crate::protocol::anthropic::message_from_response(&chat(), model);
            axum::Json(message).into_response()
        }
    }
}

/// 聚合结果 → `chat.completion`（OpenAI / Anthropic 两个出口共用的一份形状）。
fn chat_completion_of(
    aggregated: &sse::AggregatedChat,
    chat_id: &str,
    model: &str,
    usage: Option<Value>,
) -> Value {
    let mut message = serde_json::Map::new();
    message.insert("role".into(), Value::String("assistant".into()));
    message.insert("content".into(), Value::String(aggregated.content.clone()));
    if !aggregated.reasoning.is_empty() {
        message.insert(
            "reasoning_content".into(),
            Value::String(aggregated.reasoning.clone()),
        );
    }
    if !aggregated.tool_calls.is_empty() {
        message.insert(
            "tool_calls".into(),
            Value::Array(aggregated.tool_calls.clone()),
        );
    }
    json!({
        "id": aggregated.id.clone().unwrap_or_else(|| chat_id.to_string()),
        "object": "chat.completion",
        "created": aggregated.created.unwrap_or_else(|| now_ms() / 1000),
        "model": aggregated.model.clone().unwrap_or_else(|| model.to_string()),
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": aggregated.finish_reason,
        }],
        "usage": usage.unwrap_or_else(|| json!({
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `usage_credits`：取 `credits`，缺失回落 `original_credits`；非法值一律 0。
    ///
    /// 可证伪：把 `.filter(|value| value.is_finite() && *value > 0.0)` 删掉 ⇒ NaN / 负数
    /// 会进 G3 的累加，把「当日已用」算成负的（护栏永远不触发，而面板显示奇怪的小数）。
    #[test]
    fn usage_credits_reads_credits_with_fallback() {
        assert_eq!(
            usage_credits(&serde_json::json!({"credits": 0.3486})),
            0.3486
        );
        assert_eq!(
            usage_credits(&serde_json::json!({"original_credits": 1.5})),
            1.5,
            "只给 original_credits 的模型也要计"
        );
        // `credits` 优先于 `original_credits`（前者是实际计费值）。
        assert_eq!(
            usage_credits(&serde_json::json!({"credits": 0.5, "original_credits": 2.0})),
            0.5
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"credits": 0}),
            serde_json::json!({"credits": -1}),
            serde_json::json!({"credits": "0.3"}),
        ] {
            assert_eq!(usage_credits(&bad), 0.0, "{bad}");
        }
    }

    /// `/health` 的返回体带**风控遥测**（排队 / 限流 / 客户端错）。
    ///
    /// 键名是 wire 契约（运维 / 前端按它读）；缺了就等于把遥测藏起来。
    #[tokio::test]
    async fn health_reports_risk_telemetry() {
        let response = health().await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("读得到 body");
        let body: Value = serde_json::from_slice(&bytes).expect("必须是 JSON");
        assert_eq!(body["ok"], true);
        for key in ["queued", "rateLimited", "rejected", "exhausted"] {
            assert!(
                body["risk"][key].is_u64(),
                "缺 `risk.{key}`（遥测契约）: {body}"
            );
        }
    }
}
