//! `POST /v1/chat/completions`：出站改写 → 账号池轮换 → 流式透传 / 非流式聚合。

use std::time::Instant;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use buddy_switch_core::modules::region::Region;

use crate::logging::RequestMeta;
use crate::outbound::PromptMode;
use crate::protocol::openai::{CompletionAccumulator, SsePassthrough};
use crate::routes::relay::{self, RelayKind, RelayOutcome, RelayRequest};
use crate::routes::{authenticate, json_response, openai_error, parse_body, sse_response};
use crate::session_headers;
use crate::state::GatewayState;
use crate::timeutil;

/// chat completions 处理器。
pub async fn handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();

    let record = match authenticate(&state, &headers) {
        Ok(record) => record,
        Err(error) => return openai_error(error),
    };
    let mut region = record.region;

    let parsed = match parse_body(&body) {
        Ok(value) => value,
        Err(error) => return openai_error(error),
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

    // 模型名前缀路由（`cn:` / `global:`）。默认只剥离「同域」前缀（纯归一化，不改路由）；
    // 跨域前缀需显式开关，否则返回可诊断的 400 —— 静默改名会让上游报「模型不存在」，
    // 把「配置没开」误报成「模型名写错」，排查成本极高。
    let route = crate::model_route::split_model_prefix(&requested_model);
    let allow_reroute = state.config_snapshot().await.allow_model_region_prefix;
    let model = match crate::model_route::decide(region, allow_reroute, &route) {
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
            return openai_error(crate::error::GatewayError::BadRequest(
                crate::model_route::rejection_message(requested, key_region),
            ))
        }
    };

    // ★ 出口 ②（P11）：被禁用的模型**直接拒绝** —— 与 `/v1/models` 读**同一份** `disabled`
    //   （`is_disabled` 是唯一判断点）。只改过滤不拒绝 = 假开关（用户以为禁用了、实际还能调）。
    {
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, region, &model) {
            return openai_error(crate::error::GatewayError::ModelDisabled { region, model });
        }
    }

    let raw_source = String::from_utf8_lossy(&body);
    let raw = if model == requested_model {
        raw_source.to_string()
    } else {
        crate::model_route::rewrite_body_model(&raw_source, &model)
    };

    let conversation_id = relay::conversation_id_of(&parsed);
    let turn_key = session_headers::turn_key(&parsed);
    let inbound_request_id = header_of(&headers, "x-conversation-request-id");
    let trace_id = header_of(&headers, "x-trace-id");

    let mut attempt = run(
        &state,
        region,
        &raw,
        &model,
        conversation_id.clone(),
        turn_key.as_deref(),
        inbound_request_id.as_deref(),
        trace_id.clone(),
    )
    .await;

    // 内容拦截自救：passthrough 模式下换中性提示词重试一次。
    // custom 模式不降级（用户已显式指定提示词，网关不应擅自替换）；
    // 已在降级期内也不重试（避免无意义的上游往返）。
    if let Err(failure) = &attempt {
        let status = failure.error.status();
        let mode = state.outbound_options().await.prompt.mode;
        let already_degraded = state.degrade_active(timeutil::now_ms()).await;
        if !already_degraded
            && mode == PromptMode::Passthrough
            && relay::is_content_blocked(status, &failure.upstream_message)
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
            )
            .await;
        }
    }

    match attempt {
        Err(failure) => {
            let http_status = failure.error.status();
            state.log.record(
                RequestMeta {
                    endpoint: "/v1/chat/completions",
                    method: "POST",
                    region,
                    account: failure.account,
                    model,
                    status: http_status,
                    latency_ms: started.elapsed().as_millis() as i64,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    stream: wants_stream,
                }
                .to_value(),
            );
            openai_error(failure.error)
        }
        Ok(RelayOutcome::Ok {
            response,
            account,
            uid,
        }) => {
            if wants_stream {
                state.log.record(
                    RequestMeta {
                        endpoint: "/v1/chat/completions",
                        method: "POST",
                        region,
                        account,
                        model: model.clone(),
                        status: 200,
                        latency_ms: started.elapsed().as_millis() as i64,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        stream: true,
                    }
                    .to_value(),
                );
                // 流式也要回填成本账本，否则「账本择优」在实际流量上等于未启用。
                let sink = relay::pool_usage_sink(&state, &uid, &model);
                sse_response(SsePassthrough::new(crate::protocol::usage_tap::UsageTap::new(
                    response.bytes_stream(),
                    sink,
                )))
            } else {
                let text = response.text().await.unwrap_or_default();
                let accumulator = CompletionAccumulator::from_sse(&text);
                // 只在拿到真实 credit 时写账本——否则会把账号误标为「免费」。
                if let Some(credit) = accumulator.credit {
                    let tokens = accumulator.prompt_tokens.unwrap_or(0) as i64
                        + accumulator.completion_tokens.unwrap_or(0) as i64;
                    relay::record_ledger(&state, &uid, &model, credit, tokens).await;
                }
                let completion = accumulator.to_openai_completion(&model);
                state.log.record(
                    RequestMeta {
                        endpoint: "/v1/chat/completions",
                        method: "POST",
                        region,
                        account,
                        model,
                        status: 200,
                        latency_ms: started.elapsed().as_millis() as i64,
                        prompt_tokens: accumulator.prompt_tokens.unwrap_or(0),
                        completion_tokens: accumulator.completion_tokens.unwrap_or(0),
                        stream: false,
                    }
                    .to_value(),
                );
                json_response(StatusCode::OK, completion)
            }
        }
    }
}

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
) -> Result<RelayOutcome, relay::RelayFailure> {
    let (prepared_body, conversation_request_id) = relay::prepare_body(
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
            region,
            kind: RelayKind::Llm,
            model: model.to_string(),
            prepared_body,
            conversation_request_id,
            conversation_id,
            trace_id,
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
