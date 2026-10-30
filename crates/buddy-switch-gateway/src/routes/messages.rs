//! `POST /v1/messages`：Anthropic 协议 + tool use 双向转换。
//!
//! 与 `/v1/chat/completions` 共用同一套中继（选号 / 轮换 / 冷却），差别只在协议层：
//! 入站 Anthropic → 上游 OpenAI 形态，出站上游 SSE → Anthropic 事件流。

use std::time::Instant;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use buddy_switch_core::modules::product::scope_of_wb_region;
use buddy_switch_core::modules::region::Region;

use crate::logging::RequestMeta;
use crate::outbound::PromptMode;
use crate::protocol::anthropic::{
    aggregate_message, to_upstream_request_adaptive, AnthropicSseStream,
};
use crate::routes::relay::{self, RelayKind, RelayOutcome, RelayRequest};
use crate::routes::{
    anthropic_error, anthropic_error_with_fallback, authenticate, json_response, parse_body,
    sse_response,
};
use crate::session_headers;
use crate::state::GatewayState;
use crate::timeutil;

/// Anthropic messages 处理器。
pub async fn handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let record = match authenticate(&state, &headers) {
        Ok(record) => record,
        Err(error) => return anthropic_error(error),
    };
    // ★ issue #76：把 Key 收敛成 `KeyActor`（id + 策略 + 账本）——
    //   中继层只认这个抽象，四条产品线共用同一份判定与记账。
    //   ★ 必须在 `state` 被 move 进 `*_for_region` **之前**构造。
    let actor = std::sync::Arc::new(crate::apikey::KeyActor::new(
        record.id.clone(),
        record.policy(),
        state.key_usage.clone(),
        "wb",
    ));
    // ★ 归属域由 API Key 决定；前缀策略走产品线那一套（统一入口走 `AlreadyResolved`）。
    let allow_reroute = state.config_snapshot().await.allow_model_region_prefix;
    messages_for_region(
        state,
        record.region,
        headers,
        body,
        crate::routes::chat::PrefixPolicy::RegionPrefix { allow_reroute },
        // ★ issue #76：Key 策略随请求进中继层（配额判定 + 账号白名单）。
        Some(actor),
    )
    .await
}

/// 一趟 Anthropic 中继（**认证之后的全部逻辑**）—— 产品线入口与统一网关**共用**这一份实现。
///
/// `state` **按值**传入（`Arc` 句柄，克隆极廉价）⇒ 函数体内既有的 `&state` 写法一行都不用改。
pub async fn messages_for_region(
    state: GatewayState,
    region: Region,
    headers: HeaderMap,
    body: Bytes,
    policy: crate::routes::chat::PrefixPolicy,
    // ★ issue #76：调用方的 API Key（`None` = 无 Key 的本地/测试调用 ⇒ 不判配额、不记账）。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let started = Instant::now();
    let mut region = region;

    let parsed = match parse_body(&body) {
        Ok(value) => value,
        Err(error) => return anthropic_error(error),
    };
    let requested_model = parsed
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let wants_stream = parsed
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    // 模型名前缀路由：与 `/v1/chat/completions` **同一策略对象**（见 `PrefixPolicy`）——
    // 统一入口传 `AlreadyResolved`，此处**不再**解析 `cn:` / `global:`。
    let model = match policy {
        crate::routes::chat::PrefixPolicy::AlreadyResolved => requested_model.clone(),
        crate::routes::chat::PrefixPolicy::RegionPrefix { allow_reroute } => {
            let route = crate::model_route::split_model_prefix(&requested_model);
            match crate::model_route::decide(region, allow_reroute, &route) {
                crate::model_route::PrefixDecision::NoPrefix => requested_model.clone(),
                crate::model_route::PrefixDecision::StripOnly => route.bare_model.clone(),
                crate::model_route::PrefixDecision::Reroute(target) => {
                    region = target;
                    route.bare_model.clone()
                }
                crate::model_route::PrefixDecision::Rejected {
                    requested,
                    key_region,
                } => {
                    return anthropic_error(crate::error::GatewayError::BadRequest(
                        crate::model_route::rejection_message(requested, key_region),
                    ))
                }
            }
        }
    };

    // ★ 出口 ②（P11）：与 `/v1/chat/completions` 同源 —— 被禁用的模型直接拒绝（读同一份 `disabled`）。
    {
        // ★ 禁用集合按**作用域**分家（2026-10-15 起）—— WB 路由把 region 升成 Scope。
        let scope = scope_of_wb_region(region);
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, scope, &model) {
            return anthropic_error(crate::error::GatewayError::ModelDisabled { scope, model });
        }
    }

    // Anthropic 请求 → 上游（OpenAI 形态）。
    // ★ 图片：默认丢（上游是否吃 `content` 里的 `image_url` parts 未实测），但丢弃
    // 时会在文本里写「已移除 N 张图片」；`BUDDY_SWITCH_ANTHROPIC_FORWARD_IMAGES=1`
    // 打开后改走带图分支。见 `protocol::anthropic::to_upstream_request_adaptive`。
    let upstream_body = to_upstream_request_adaptive(&parsed);
    let upstream_raw = serde_json::to_string(&upstream_body).unwrap_or_else(|_| "{}".to_string());
    let raw = if model == requested_model {
        upstream_raw
    } else {
        crate::model_route::rewrite_body_model(&upstream_raw, &model)
    };

    let conversation_id = relay::conversation_id_of(&parsed);
    let turn_key = session_headers::turn_key(&parsed);
    let inbound_request_id = header_of(&headers, "x-conversation-request-id");
    let trace_id = header_of(&headers, "x-trace-id");

    let key_ref = key.as_deref();
    let mut attempt = run(
        &state,
        region,
        &raw,
        &model,
        conversation_id.clone(),
        turn_key.as_deref(),
        inbound_request_id.as_deref(),
        trace_id.clone(),
        key_ref,
    )
    .await;

    // 与 chat 端点同一套降级策略。
    if let Err(failure) = &attempt {
        let status = failure.error.status();
        let mode = state.outbound_options().await.prompt.mode;
        let already_degraded = state.degrade_active(timeutil::now_ms()).await;
        if !already_degraded
            && mode == PromptMode::Passthrough
            && relay::should_degrade_retry(status, &failure.upstream_message, failure.sanitized)
            && relay::trip_degrade(&state).await
        {
            attempt = run(
                &state,
                region,
                &raw,
                &model,
                conversation_id,
                turn_key.as_deref(),
                inbound_request_id.as_deref(),
                trace_id,
                key_ref,
            )
            .await;
        }
    }

    match attempt {
        // 跨作用域降级：目标产品线已经产出了成型响应，直接返回。
        // 跨作用域降级：目标产品线已经产出了成型响应，直接返回。
        //
        // ★ 但**请求日志必须记**：否则用户看不到这次请求存在（账本也少一笔）。
        //   token 用量拿不到（响应由目标产品线封装）—— 记 0 表示「本入口未知」，
        //   不是「没消耗」；目标平台自己的账本会记它那一边的真实用量。
        Ok(RelayOutcome::Ready { response, model }) => {
            let elapsed = started.elapsed().as_millis() as i64;
            state.log.record(
                RequestMeta {
                    endpoint: "/v1/messages",
                    key_id: key.as_deref().map(|actor| actor.key_id.clone()),
                    key_scope: key.as_deref().map(|actor| actor.scope),
                    method: "POST",
                    region,
                    account: String::new(),
                    account_uid: String::new(),
                    model,
                    status: response.status().as_u16(),
                    latency_ms: elapsed,
                    total_ms: elapsed,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    credit: 0.0,
                    stream: true,
                    // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
                    error: None,
                }
                .to_value(),
            );
            return response;
        }
        Err(failure) => {
            // 先取轨迹：下面的日志会把 `failure.uid` move 走，之后再借整条 `failure` 就晚了。
            let note = relay::fallback_summary_json(&failure);
            let http_status = failure.error.status();
            let elapsed_ms = started.elapsed().as_millis() as i64;
            state.log.record(
                RequestMeta {
                    endpoint: "/v1/messages",
                    key_id: key.as_deref().map(|actor| actor.key_id.clone()),
                    key_scope: key.as_deref().map(|actor| actor.scope),
                    method: "POST",
                    region,
                    account: failure.account,
                    account_uid: failure.uid,
                    model,
                    status: http_status,
                    // 失败时没有「首字节」，两字段同值（诚实：不编 TTFB）。
                    latency_ms: elapsed_ms,
                    total_ms: elapsed_ms,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    credit: 0.0,
                    stream: wants_stream,
                    // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
                    error: None,
                }
                .to_value(),
            );
            anthropic_error_with_fallback(failure.error, note)
        }
        Ok(RelayOutcome::Ok {
            response,
            prefix,
            account,
            uid,
            fallback,
        }) => {
            // ★ 降级后记**实际使用的模型**（账本与日志同口径，见 chat 路由的同名注释）。
            let model = fallback
                .as_ref()
                .map(|info| info.to.clone())
                .unwrap_or_else(|| model.clone());
            // ★ 上游**响应头**已到 ⇒ 此刻的耗时就是 `latencyMs`（TTFB）；总耗时是 `totalMs`。
            let header_at_ms = started.elapsed().as_millis() as i64;
            // ★ issue #129：流中途失败的三个消费者（协议出口 / 请求日志 / 粘性解绑）
            //   共用同一个信号槽。开关从**本轮**配置快照读（热生效）。
            let auto_failover = state.config_snapshot().await.auto_failover;
            let failure = crate::failover::StreamFailure::new(auto_failover);
            // 粘性键用**客户端请求的模型**（与 `relay` 内部推导同一口径）。
            let sticky_key = relay::sticky_key_for(
                region,
                &model,
                relay::conversation_id_of(&parsed).as_deref(),
            );
            let mut resp = if wants_stream {
                // ★ 流式请求的日志**推迟到流结束**才写（见 `StreamingLogSink`）：
                //   写日志时用量还没到达，此前只能写死 0（实测本机 200/200 条全 0）。
                let sinks = relay::stream_sinks(
                    &state,
                    RequestMeta {
                        endpoint: "/v1/messages",
                        key_id: key.as_deref().map(|actor| actor.key_id.clone()),
                        key_scope: key.as_deref().map(|actor| actor.scope),
                        method: "POST",
                        region,
                        // 留给 `with_failover` 的对外文案（下面还要读一次）。
                        account: account.clone(),
                        account_uid: uid.clone(),
                        model: model.clone(),
                        status: 200,
                        // TTFB：现在就填实（sink **不会**覆盖它）。
                        latency_ms: header_at_ms,
                        // 占位：由 sink 在流结束时补齐。
                        total_ms: 0,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        credit: 0.0,
                        stream: true,
                        // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
                        error: None,
                    },
                    started,
                    &uid,
                    &model,
                    // ★ issue #76：按密钥的 token 记账（流结束才写）。
                    key.as_deref(),
                    // ★ issue #129：日志据此写真实状态码（不是恒 200）。
                    Some(failure.clone()),
                );
                sse_response(
                    AnthropicSseStream::new(
                        crate::protocol::usage_tap::UsageTap::new(
                            relay::prefix_stream(prefix, response),
                            sinks,
                        )
                        // ★ 流内信封错误：与 chat 路由同一判据，只是错误体形状不同。
                        .with_error_sink(relay::envelope_error_sink(
                            &state,
                            &uid,
                            &model,
                            sticky_key.as_deref(),
                        ))
                        // ★ issue #129：同一份嗅探结果也写进失败槽（出口/日志读它）。
                        .with_failure(Some(failure.clone())),
                        model.clone(),
                    )
                    // ★ issue #139：`message_start` 必须当场给 `input_tokens`，而上游
                    //   的真实 `prompt_tokens` 要到末帧 —— 先用请求侧估算占位，
                    //   否则客户端（dsh / Claude Code）的「上下文占用」恒为 0%。
                    .with_estimated_input(crate::protocol::anthropic::estimate_input_tokens(
                        &parsed,
                    ))
                    // ★ issue #129：开关开启时，收尾发 `event: error` 而不是 `message_stop`。
                    .with_failover(Some(failure), &account),
                )
            } else {
                let text = relay::body_text_with_prefix(prefix, response).await;
                // ★ 200 信封错误（P0-3）：与 chat 路由同一判据、同一治理副作用，
                //   只是对外错误体走 Anthropic 形状。
                if let Some(envelope) = crate::protocol::error_sniffer::sniff_http_body(&text) {
                    relay::note_envelope_failure(&state, &uid, &model, &envelope.event).await;
                    let elapsed_ms = started.elapsed().as_millis() as i64;
                    state.log.record(
                        RequestMeta {
                            endpoint: "/v1/messages",
                            key_id: key.as_deref().map(|actor| actor.key_id.clone()),
                            key_scope: key.as_deref().map(|actor| actor.scope),
                            method: "POST",
                            region,
                            account,
                            account_uid: uid,
                            model,
                            status: envelope.status,
                            latency_ms: header_at_ms,
                            total_ms: elapsed_ms,
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            credit: 0.0,
                            stream: false,
                            // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
                            error: None,
                        }
                        .to_value(),
                    );
                    // 降级后仍然拿到「200 信封错误」：头要带上，否则客户端不知道是备选模型的锅。
                    let mut resp = anthropic_error(crate::error::GatewayError::Upstream {
                        kind: relay::upstream_kind_of(&envelope.event),
                        status: envelope.status,
                        message: envelope.message,
                        attempts: Vec::new(),
                    });
                    if let Some(info) = fallback.as_ref() {
                        relay::apply_fallback_headers(&mut resp, info);
                    }
                    return resp;
                }
                let message = aggregate_message(&text, &model);
                let prompt_tokens = message
                    .get("usage")
                    .and_then(|usage| usage.get("input_tokens"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let completion_tokens = message
                    .get("usage")
                    .and_then(|usage| usage.get("output_tokens"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                /* 积分：与 chat / responses 同一口径（上游 `usage.credit`）。
                缺字段记 0.0（= 不知道），不猜、不按 token 折算。 */
                let credit = message
                    .get("usage")
                    .and_then(|usage| usage.get("credit"))
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                state.log.record(
                    RequestMeta {
                        endpoint: "/v1/messages",
                        key_id: key.as_deref().map(|actor| actor.key_id.clone()),
                        key_scope: key.as_deref().map(|actor| actor.scope),
                        method: "POST",
                        region,
                        account,
                        account_uid: uid,
                        model,
                        status: 200,
                        latency_ms: header_at_ms,
                        total_ms: started.elapsed().as_millis() as i64,
                        prompt_tokens,
                        completion_tokens,
                        credit,
                        stream: false,
                        // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
                        error: None,
                    }
                    .to_value(),
                );
                json_response(StatusCode::OK, message)
            };
            // 降级成功：把「从谁降到谁、为什么」写进响应头（与 chat 路由同一套头）。
            if let Some(info) = fallback.as_ref() {
                relay::apply_fallback_headers(&mut resp, info);
            }
            resp
        }
    }
}

#[allow(clippy::result_large_err)]
// `RelayFailure` 承载完整上游响应与诊断上下文，装箱会丢掉「一次就能定位」的可读性。
/// 改写 + 中继一趟（供首次尝试与降级重试共用）。
#[allow(clippy::too_many_arguments)]
async fn run(
    state: &GatewayState,
    region: Region,
    raw: &str,
    model: &str,
    conversation_id: Option<String>,
    turn_key: Option<&str>,
    inbound_request_id: Option<&str>,
    trace_id: Option<String>,
    // ★ issue #76：调用方的 API Key（`None` = 无 Key 的本地/测试调用 ⇒ 不判配额、不记账）。
    key: Option<&crate::apikey::KeyActor>,
) -> Result<RelayOutcome, relay::RelayFailure> {
    let prepared = relay::prepare_body(
        state,
        region,
        raw,
        "",
        conversation_id.as_deref(),
        turn_key,
        inbound_request_id,
    )
    .await;

    relay::relay(
        state,
        RelayRequest {
            openai_shape: false,
            region,
            kind: RelayKind::Llm,
            model: model.to_string(),
            prepared_body: prepared.body,
            sanitized: prepared.sanitized,
            conversation_request_id: prepared.conversation_request_id,
            conversation_id,
            trace_id,
            key_policy: key.map(|actor| actor.policy.clone()),
            key_id: key.as_deref().map(|actor| actor.key_id.clone()),
        },
    )
    .await
}

/// 读取请求头（去空白；空值视为未提供）。
fn header_of(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
