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

use super::pool::{classify_http, QoderErrKind};
use super::{payload, sse, QoderGatewayState};

/// 鉴权通过后透传给 handler 的**归属区域**（决定用哪个账号池）。
#[derive(Debug, Clone, Copy)]
pub struct QoderKeyRegion(pub QoderRegion);

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
    axum::Json(json!({ "ok": true })).into_response()
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
    body: Bytes,
) -> Response {
    chat_for_region(state, key.0, body, Protocol::OpenAi).await
}

/// `POST /v1/messages`。
pub async fn messages(
    State(state): State<QoderGatewayState>,
    Extension(key): Extension<QoderKeyRegion>,
    body: Bytes,
) -> Response {
    chat_for_region(state, key.0, body, Protocol::Anthropic).await
}

/// `POST /v1/responses`（OpenAI Responses API）。
///
/// Codex CLI 的 `wire_api = "responses"` 打的就是这个路径 —— Qoder 上游只有
/// chat SSE，故这里做**协议翻译**（入站 → chat，出站 → Responses 事件流 / 对象），
/// 中继仍是 [`chat_for_region`]（选号 / 换号 / 冷却 / 记账一份）。
pub async fn responses(
    State(state): State<QoderGatewayState>,
    Extension(key): Extension<QoderKeyRegion>,
    body: Bytes,
) -> Response {
    chat_for_region(state, key.0, body, Protocol::Responses).await
}

/// 一趟 Qoder chat 中继（**认证之后的全部逻辑**）—— 产品线入口与统一网关**共用这一份**。
pub async fn chat_for_region(
    state: QoderGatewayState,
    region: QoderRegion,
    body: Bytes,
    protocol: Protocol,
) -> Response {
    let started = Instant::now();
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
    // 非 OpenAI 入口先转成 OpenAI chat 形态（中继内部只认一份形态）。
    let openai_payload = match protocol {
        Protocol::OpenAi => parsed.clone(),
        Protocol::Anthropic => crate::protocol::anthropic::to_upstream_request(&parsed),
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
    let build_encoded = |user_type: &str| -> Result<String, String> {
        let body_value = api::build_chat_body(&openai_payload, &meta, region, user_type)?;
        Ok(cosy::qoder_encode(body_value.to_string().as_bytes()))
    };

    let mut tried: HashSet<String> = HashSet::new();
    let mut failures: Vec<String> = Vec::new();

    for _ in 0..max_rotate {
        // 池随账号库同步（导入 / 刷新后即时生效），然后选号。
        let picked = {
            let mut pools = state.pools.lock().await;
            let pool = pools.entry(region).or_default();
            pool.sync_for(region);
            pool.pick(&model, &tried, preferred.as_deref())
        };
        let Some(uid) = picked else {
            break;
        };
        tried.insert(uid.clone());

        let entry = {
            let pools = state.pools.lock().await;
            pools
                .get(&region)
                .and_then(|pool| pool.entries().find(|entry| entry.uid == uid).cloned())
        };
        let Some(entry) = entry else { continue };

        let encoded = match build_encoded("") {
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
                continue;
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
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
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
        let kind = classify_http(status_code);
        {
            let mut pools = state.pools.lock().await;
            if let Some(pool) = pools.get_mut(&region) {
                pool.apply_error(&uid, kind, &detail, &model);
            }
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
                    Some(detail.clone()),
                )
                .await;
            return protocol_error(
                protocol,
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                "upstream_error",
                &format!("上游拒绝（HTTP {status_code}）：{detail}"),
            );
        }
    }

    // 轮换耗尽 / 无可用账号。
    let message = if failures.is_empty() {
        format!(
            "{} 没有可用账号：请在「账号」页导入（账号需带可调用的访问令牌）",
            region.display_name()
        )
    } else {
        format!(
            "全部账号都失败（{} 个）：{}",
            failures.len(),
            failures.join("；")
        )
    };
    state
        .record_request(
            "chat",
            region,
            &model,
            503,
            "",
            started.elapsed().as_millis() as i64,
            wants_stream,
            0,
            0,
            Some(message.clone()),
        )
        .await;
    protocol_error(
        protocol,
        StatusCode::SERVICE_UNAVAILABLE,
        "no_available_account",
        &message,
    )
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
) -> Response {
    let upstream = response.bytes_stream();
    let stream_model = model.to_string();
    let usage_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<Value>));
    // 上游错误帧留痕：流式账本在流结束才落，没有它就只能记成「200 + 0 token」。
    let error_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let state_for_task = state.clone();
    let uid = uid.to_string();
    let model_for_task = model.to_string();

    // 转换链：信封解包 → **泄漏回声守卫** → `UsageTapStream`（旁路取 usage / 错误）
    //        →（Anthropic 入口再包一层）。
    // ⚠️ 两处顺序都是硬约束：
    //   ① 守卫必须在 `UsageTapStream` **之内** —— 它不改 `QoderOpenAiStream::new` 的签名，
    //      而 `aggregate`（非流式）内部自建流、**不经过**本层 ⇒ 结构上不可能被还原两次；
    //   ② `UsageTapStream` **必须在 Anthropic 转换之前** —— 转换后帧是 Anthropic 事件，
    //      取不到 OpenAI 的 `usage` 字段。
    let allowed_tools_shared = allowed_tools.map(std::sync::Arc::new);
    let tapped = sse::UsageTapStream::new(
        sse::LeakGuardStream::new(sse::QoderOpenAiStream::new(upstream), allowed_tools_shared),
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
        )),
    };

    tokio::spawn(async move {
        use futures_util::StreamExt;
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
        let (prompt_tokens, completion_tokens) = match &usage {
            Some(usage) => usage_counts(usage),
            None => (0, 0),
        };
        // 上游中途发过错误帧 ⇒ 记 502 + 详情，别写成「200 / 0 token / 无错误」
        // （那正是 issue #46 里最难排查的形态）。
        let upstream_error = error_slot.lock().ok().and_then(|guard| guard.clone());
        let (status, error) = match &upstream_error {
            Some(detail) => (502u16, Some(detail.clone())),
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
                error,
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
) -> Response {
    let aggregated = sse::aggregate(response.bytes_stream(), allowed_tools.as_ref()).await;
    let usage = aggregated.usage.clone();
    let (prompt_tokens, completion_tokens) = match &usage {
        Some(usage) => usage_counts(usage),
        None => (0, 0),
    };
    // ★ 账本先定「这一请求到底成没成」再落：上游只要给过错误帧就不是 200，
    //   否则日志会显示「200 + 0 token + error:null」（issue #46 的隐身故障）。
    let (log_status, log_error) = match &aggregated.upstream_error {
        Some(detail) => (502u16, Some(detail.clone())),
        None => (200u16, None),
    };
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
            log_error,
        )
        .await;

    if let Some(error) = &aggregated.upstream_error {
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
