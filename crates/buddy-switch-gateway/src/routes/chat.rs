//! `POST /v1/chat/completions`：出站改写 → 账号池轮换 → 流式透传 / 非流式聚合。

use std::time::Instant;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use buddy_switch_core::modules::product::scope_of_wb_region;
use buddy_switch_core::modules::region::Region;

use crate::logging::RequestMeta;
use crate::outbound::PromptMode;
use crate::protocol::openai::{CompletionAccumulator, SsePassthrough};
use crate::routes::relay::{self, RelayKind, RelayOutcome, RelayRequest};
use crate::routes::{
    authenticate, json_response, openai_error, openai_error_with_fallback, parse_body, sse_response,
};
use crate::session_headers;
use crate::state::GatewayState;
use crate::timeutil;

/// 入口策略 —— **模型名里的前缀**如何处置。
///
/// 两个入口对前缀的语义**完全不同**，不能共用一套规则：
/// - **产品线独立端口**：`cn:` / `global:` 是「跨域重路由」，由 `allow_model_region_prefix` 把关；
/// - **统一网关**：来源已由**命名空间**（`wb-cn:`）决定，模型名里剩下的 `:` 属于模型本身
///   （如 `vendor:model`）⇒ **绝不能再解析一次**，否则会被二次改写。
pub enum PrefixPolicy {
    /// 产品线入口：按既有开关处理 `cn:` / `global:`。
    RegionPrefix {
        /// 是否允许跨域重路由（读 `GatewayConfig::allow_model_region_prefix`）。
        allow_reroute: bool,
    },
    /// 统一入口：前缀已由命名空间处理过，**不再解析**。
    AlreadyResolved,
}

/// chat completions 处理器。
pub async fn handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let record = match authenticate(&state, &headers) {
        Ok(record) => record,
        Err(error) => return openai_error(error),
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
    // ★ 归属域由 API Key 决定（不接受客户端声明）；前缀策略走产品线那一套。
    let allow_reroute = state.config_snapshot().await.allow_model_region_prefix;
    chat_for_region(
        state,
        record.region,
        headers,
        body,
        PrefixPolicy::RegionPrefix { allow_reroute },
        // ★ issue #76：Key 策略随请求进中继层（配额判定 + 账号白名单）。
        Some(actor),
    )
    .await
}

/// 一趟 chat 中继（**认证之后的全部逻辑**）—— 产品线入口与统一网关**共用**这一份实现。
///
/// # 为什么抽出来（而不是让统一网关再写一份）
///
/// 「把请求可靠地送到上游」这件事在 `relay` 里已是唯一实现；而它**外面**的那一层
/// （改写 body → 停用判定 → 首轮 + 内容拦截降级重试 → 流式/非流式出口 → 写日志）
/// 同样不能有两份 —— 两份必然漂移，且漂移点恰恰是「换号 + 降级」这类最难复现的行为。
///
/// `state` **按值**传入（`GatewayState` 是 `Arc` 句柄，克隆极廉价）：这样函数体内既有的
/// `&state` 写法一行都不用改，抽取的 diff 面最小。
pub async fn chat_for_region(
    state: GatewayState,
    region: Region,
    headers: HeaderMap,
    body: Bytes,
    policy: PrefixPolicy,
    // ★ issue #76：调用方的 API Key（`None` = 无 Key 的本地/测试调用 ⇒ 不判配额、不记账）。
    key: Option<std::sync::Arc<crate::apikey::KeyActor>>,
) -> Response {
    let started = Instant::now();
    let mut region = region;

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
    let model = match policy {
        // 统一入口：来源已由命名空间决定，模型名**原样采用**
        // （再解析一次会把 `vendor:model` 这类模型名改写掉）。
        PrefixPolicy::AlreadyResolved => requested_model.clone(),
        PrefixPolicy::RegionPrefix { allow_reroute } => {
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
                    return openai_error(crate::error::GatewayError::BadRequest(
                        crate::model_route::rejection_message(requested, key_region),
                    ))
                }
            }
        }
    };

    // ★ 出口 ②（P11）：被禁用的模型**直接拒绝** —— 与 `/v1/models` 读**同一份** `disabled`
    //   （`is_disabled` 是唯一判断点）。只改过滤不拒绝 = 假开关（用户以为禁用了、实际还能调）。
    {
        // ★ 禁用集合按**作用域**分家（2026-10-15 起）—— WB 路由把 region 升成 Scope。
        let scope = scope_of_wb_region(region);
        let disabled = state.disabled.read().await;
        if crate::disabled_models::is_disabled(&disabled, scope, &model) {
            return openai_error(crate::error::GatewayError::ModelDisabled { scope, model });
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

    // 内容拦截自救：passthrough 模式下换中性提示词重试一次。
    // custom 模式不降级（用户已显式指定提示词，网关不应擅自替换）；
    // 已在降级期内也不重试（避免无意义的上游往返）。
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
                    endpoint: "/v1/chat/completions",
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
                    endpoint: "/v1/chat/completions",
                    key_id: key.as_deref().map(|actor| actor.key_id.clone()),
                    key_scope: key.as_deref().map(|actor| actor.scope),
                    method: "POST",
                    region,
                    account: failure.account,
                    account_uid: failure.uid,
                    model,
                    status: http_status,
                    // 失败时没有「首字节」，两字段同为「请求到失败」的耗时（诚实：不编 TTFB）。
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
            openai_error_with_fallback(failure.error, note)
        }
        Ok(RelayOutcome::Ok {
            response,
            prefix,
            account,
            uid,
            fallback,
        }) => {
            // ★ 降级后**实际使用的模型**：账本与日志都记它。
            //   记客户端请求的 origin 会让用量页的模型维度骗人 ——
            //   「claude 名下记着 glm 的消耗」是账本最不该犯的错。
            //   用 shadowing 而不是逐处替换：分支内所有 `model` 从此指向真实模型。
            let model = fallback
                .as_ref()
                .map(|info| info.to.clone())
                .unwrap_or_else(|| model.clone());
            // ★ 上游**响应头**已到 ⇒ 此刻的耗时就是 `latencyMs`（TTFB，与改造前逐字同语义）。
            //   绝不能等到流结束再算 —— 那是 `totalMs`（2026-10-15 事故根因）。
            let header_at_ms = started.elapsed().as_millis() as i64;
            // ★ issue #129：流中途失败的三个消费者（协议出口 / 请求日志 / 粘性解绑）
            //   共用同一个信号槽。开关从**本轮**配置快照读（热生效）。
            let auto_failover = state.config_snapshot().await.auto_failover;
            let failure = crate::failover::StreamFailure::new(auto_failover);
            // 粘性键用**客户端请求的模型**（与 `relay` 内部推导同一口径）——
            // 用降级后的目标模型算出来的键永远匹配不上，解绑会静默失效。
            let sticky_key = relay::sticky_key_for(
                region,
                &model,
                relay::conversation_id_of(&parsed).as_deref(),
            );
            let mut resp = if wants_stream {
                // ★ 流式请求的日志**推迟到流结束**才写（`StreamingLogSink`）：写日志的
                //   那一刻用量还没到达，此前只能把 token 写死 0（实测本机 200/200 条全 0）。
                //   账本同理 —— 两者共用 `UsageTap` 的结束回调。
                let sinks = relay::stream_sinks(
                    &state,
                    RequestMeta {
                        endpoint: "/v1/chat/completions",
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
                // 组合顺序：上游字节流 -> UsageTap (旁观原始字节 + 结束回报) -> ReasoningDelta
                // (快照差分) -> SsePassthrough (补发 [DONE])
                sse_response(
                    SsePassthrough::new(crate::protocol::reasoning::ReasoningDelta::new(
                        crate::protocol::usage_tap::UsageTap::new(
                            relay::prefix_stream(prefix, response),
                            sinks,
                        )
                        // ★ 流内信封错误（P0-3 流式半边）：逐帧旁听，命中就记冷却 +
                        //   计模型闸门。**不改字节**，客户端看到的内容与改造前完全一致。
                        .with_error_sink(relay::envelope_error_sink(
                            &state,
                            &uid,
                            &model,
                            sticky_key.as_deref(),
                        ))
                        // ★ issue #129：同一份嗅探结果也写进失败槽（出口/日志读它）。
                        .with_failure(Some(failure.clone())),
                    ))
                    // ★ issue #129：开关开启时，收尾先发可重试错误帧再补 [DONE]。
                    .with_failover(Some(failure), &account, &model),
                )
            } else {
                let text = relay::body_text_with_prefix(prefix, response).await;
                // ★★ **200 信封错误**（P0-3）：上游常把限流/封禁/会话失效装在 2xx 响应体里
                //    （`{"code":6004,...}`）。旧实现当成功处理 ⇒ 不写冷却、不解绑粘性、
                //    返回空补全给客户端；那个号在冷却表里永远干净，于是每个请求都优先选它、
                //    每次都吃同一个 200 错误 —— 「看起来正常、其实一直失败」。
                //
                //   这里只**记冷却 + 返回真实错误**，不换号：响应已完整读完，
                //    按方案 §5 决策 4 的口径，这一类不做轮换（协议上也退不回去）。
                if let Some(envelope) = crate::protocol::error_sniffer::sniff_http_body(&text) {
                    // 降级后仍然拿到「200 信封错误」：头要带上 —— 否则客户端不知道这次是备选模型的锅。
                    let mut resp = envelope_error_response(
                        &state,
                        region,
                        started,
                        header_at_ms,
                        &account,
                        &uid,
                        &model,
                        envelope,
                    )
                    .await;
                    if let Some(info) = fallback.as_ref() {
                        relay::apply_fallback_headers(&mut resp, info);
                    }
                    return resp;
                }
                let accumulator = CompletionAccumulator::from_sse(&text);
                // 只在拿到真实 credit 时写账本——否则会把账号误标为「免费」。
                if let Some(credit) = accumulator.credit {
                    let tokens = accumulator.prompt_tokens.unwrap_or(0) as i64
                        + accumulator.completion_tokens.unwrap_or(0) as i64;
                    relay::record_ledger(&state, &uid, &model, credit, tokens, key.as_deref())
                        .await;
                }
                let completion = accumulator.to_openai_completion(&model);
                state.log.record(
                    RequestMeta {
                        endpoint: "/v1/chat/completions",
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
                        prompt_tokens: accumulator.prompt_tokens.unwrap_or(0),
                        completion_tokens: accumulator.completion_tokens.unwrap_or(0),
                        credit: accumulator.credit.unwrap_or(0.0),
                        stream: false,
                        // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
                        error: None,
                    }
                    .to_value(),
                );
                json_response(StatusCode::OK, completion)
            };
            // 降级成功：把「从谁降到谁、为什么」写进响应头。
            // 客户端看到的 `model` 字段已经变了，不给这三行它只会以为网关串号了。
            if let Some(info) = fallback.as_ref() {
                relay::apply_fallback_headers(&mut resp, info);
            }
            resp
        }
    }
}

/// **200 信封错误**的统一处置：记冷却（+ 模型级失败计数）+ 写日志 + 返回真实错误。
///
/// ## 为什么单独抽出来
///
/// 三条 `if` 判断（记不记冷却、要不要计模型闸门、日志状态码写什么）散在各路由里
/// 一定会漂移。`messages` / `responses` 两条路由接入时**只调用本函数**即可。
///
/// ## 为什么不换号
///
/// 调用点都在「响应已完整读完」之后（非流式）。按方案 §5 决策 4 的口径，
/// 这类错误只能记冷却、让**下一次**请求换号 —— 已经发出去的字节退不回来。
#[allow(clippy::too_many_arguments)]
async fn envelope_error_response(
    state: &GatewayState,
    region: Region,
    started: Instant,
    header_at_ms: i64,
    account: &str,
    uid: &str,
    model: &str,
    envelope: crate::protocol::error_sniffer::SniffedError,
) -> Response {
    use crate::error::GatewayError;

    // 治理副作用与 HTTP 非 2xx 走**同一条**路径（冷却口径、熔断、模型闸门都一致），
    // 收在 `relay::note_envelope_failure` 里供三条路由共用。
    relay::note_envelope_failure(state, uid, model, &envelope.event).await;

    let elapsed_ms = started.elapsed().as_millis() as i64;
    state.log.record(
        RequestMeta {
            endpoint: "/v1/chat/completions",
            // 这条路径（200 信封错误）拿不到 Key（`envelope_error_response` 不是
            // 认证之后的函数）⇒ 记 `None`。不影响「按密钥看用量」：这类请求本来
            // 就没进过成功记账路径。
            key_id: None,
            key_scope: None,
            method: "POST",
            region,
            account: account.to_string(),
            account_uid: uid.to_string(),
            model: model.to_string(),
            // 对客户端如实报「上游失败」，而不是 200（否则客户端以为成功了）。
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

    let kind = relay::upstream_kind_of(&envelope.event);
    openai_error(GatewayError::Upstream {
        kind,
        status: envelope.status,
        message: envelope.message,
        attempts: Vec::new(),
    })
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
            openai_shape: true,
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
