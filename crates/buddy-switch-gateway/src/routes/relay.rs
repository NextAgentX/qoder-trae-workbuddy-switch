//! 上游请求中继：选号 → 在途租约 → 出站 → 失败治理 → 换号重试。
//!
//! 这是「把一个请求可靠地送到上游」的唯一实现，OpenAI（`/v1/chat/completions`）
//! 与 Anthropic（`/v1/messages`）两条路由共用它，避免两套轮换逻辑各自演化。
//!
//! 轮换语义（对照参考实现 `handler.go` 的轮转循环）：
//! - 单轮最多尝试 `max_rotate` 次，`tried` 集合保证**同一账号不试第二次**；
//! - 每次失败按事件类型写入池的冷却/熔断状态，下一次选号自然避开；
//! - 账号被禁用（12153 三振）时立即跳出——继续重试没有意义；
//! - 全部失败时返回**最后一个**上游错误（而非第一个），它对用户更有诊断价值。
//!
//! 已知偏差（有意为之，非缺陷）：在途租约在**拿到响应头**后即释放，而非持有到流
//! 结束。流式场景下这会略微放松并发保护，换取实现不需要把池句柄塞进响应流；
//! 非流式场景无差别（响应体在 handler 内被消费完毕）。

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

use buddy_switch_core::modules::account;
use buddy_switch_core::modules::region::Region;
use buddy_switch_core::modules::upstream::UpstreamChatResult;

use crate::account_strategy::{AccountSelector, AccountStrategy};
use crate::error::{GatewayError, NoCredentialCause, UpstreamAttempt};
use crate::outbound::{self, DegradeGate, OutboundMeta};
use crate::pool::{
    classify_event, exclusions, InFlightLease, PickMode, PoolSummary, RealmTag, UpstreamEvent,
};
use crate::session_headers::{self, ConversationContext};

/// 会话 id 提取已提到共用层（[`crate::session_headers`]）—— Trae 网关复用同一份实现。
/// 这里 `pub use` 让既有调用点（`chat.rs` / `messages.rs`）与测试的路径保持不变。
pub use crate::session_headers::conversation_id_of;
use crate::state::GatewayState;
use crate::sticky;
use crate::timeutil;

/// 本轮中继的**协议类型** —— 决定「路径 + 头 + body 处理」，**不影响治理层**。
///
/// ⚠️ 这是 P12 T03（本地路由）加入的：**业务请求要复用同一条**换号/熔断逻辑，
/// 而不是另写一份 —— `relay` 是「把请求送到上游」的**唯一实现**（见模块文档）。
pub enum RelayKind {
    /// LLM：路径 `/v2/chat/completions`，头走 `build_chat_headers`，body 走 `chat_body_for`。
    Llm,
    /// **业务请求**（签到 / 积分 / 旅行）：路径由调用方给（**已在白名单内**），
    /// 头走 `build_auth_headers`，**body 原样透传**（不是 LLM 协议，不做改写）。
    ///
    /// ★ **必须带 `method`**：白名单按前缀收，同前缀下 GET-only 与 POST-only 混在一起
    /// （`/activity/growth/streak` 是 GET、`/lottery/draw` 是 POST）。一律 POST 出去会让
    /// GET-only 的路径被上游回 `404 page not found`（真实环境实测）。
    Biz {
        /// 目标路径（已含前导 `/`）。
        path: String,
        /// 出站方法（**透传客户端的原方法**）。
        method: BizMethod,
    },
}

/// 业务转发的**出站方法**（只允许 GET / POST —— 白名单里的端点只有这两种）。
///
/// 用本仓自己的小枚举而不是 `reqwest::Method`：中继层不该暴露整个 HTTP 方法空间，
/// 且新增方法必须**显式**在这里出现（编译期强制复核白名单是否也支持它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BizMethod {
    /// 只读查询（`/activity/growth/streak`、`/heatmap`）。
    Get,
    /// 写操作（签到 / 领奖 / 抽奖）。
    Post,
}

impl BizMethod {
    /// 转成出站用的 `reqwest::Method`。
    pub fn as_reqwest(self) -> reqwest::Method {
        match self {
            BizMethod::Get => reqwest::Method::GET,
            BizMethod::Post => reqwest::Method::POST,
        }
    }
}

/// 本轮中继的输入。
pub struct RelayRequest {
    /// 归属域（由 API Key 决定，不接受客户端声明）。
    pub region: Region,
    /// ★ **协议类型**（P12 T03 新增）。
    pub kind: RelayKind,
    /// 客户端声明的模型名（原样，仅用于冷却与账本）。
    pub model: String,
    /// 已由 [`outbound::prepare_outbound_body`] 处理过的请求体。
    pub prepared_body: String,
    /// 本轮出站改写是否动过黑名单指纹（决定「渠道校验文案」下的降级判定）。
    ///
    /// 由 [`prepare_body`] 从 [`outbound::PreparedOutbound::sanitized`] 透传，
    /// 失败时随 [`RelayFailure`] 回到路由层 —— 见 [`should_degrade_retry`]。
    pub sanitized: bool,
    /// 会话轮主键（轮内复用）。
    pub conversation_request_id: String,
    /// 客户端会话 id。
    pub conversation_id: Option<String>,
    /// 调用方 trace id。
    pub trace_id: Option<String>,
    /// ★ 这把 API Key 的策略（issue #76；`None` = 无 Key 的调用，如本地直连）。
    ///
    /// 带它进来只为两件事：**配额判定**（超限 ⇒ 429，不进换号循环）与
    /// **账号白名单**（选号候选过滤）。身份字段（hash / prefix）一概不带 ——
    /// 中继层不需要知道 Key 长什么样。
    pub key_policy: Option<crate::apikey::KeyPolicy>,
    /// 用量账本的键（= Key id）。与 `key_policy` **同生同空**：
    /// 没带 Key 的调用（本地直连）两个都是 `None` ⇒ 不记账、不判配额。
    pub key_id: Option<String>,
}

impl RelayRequest {
    /// 账本键：无 Key 的调用给一个**稳定的空键**（`""`）而不是 `Option` ——
    /// 调用点（`record_request`）不想为「没有 Key」再分一支。
    fn key_id_for_policy(&self) -> &str {
        self.key_id.as_deref().unwrap_or("")
    }
}

/// 中继结果。
pub enum RelayOutcome {
    /// 上游 2xx，交回原始响应（由调用方决定透传/聚合）。
    Ok {
        /// 上游响应（首块可能已被 [`peek_first_frame`] 取走 —— 见 `prefix`）。
        response: reqwest::Response,
        /// 已被 peek 取走、**必须补回**的首块字节（`None` = 没取过）。
        ///
        /// 为什么是这个形状：`reqwest::Response` **没有**「放回去」的构造入口，
        /// 所以「看一眼首帧」必然要把它单独交出来。调用方按「prefix + 剩余流」的顺序
        /// 拼接后，客户端收到的字节与顺序**完全不变**。
        prefix: Option<axum::body::Bytes>,
        /// 命中账号的展示名。
        account: String,
        /// 命中账号的 uid（用于后续记账本）。
        uid: String,
    },
}

/// 中继失败。
pub struct RelayFailure {
    /// 对外错误。
    pub error: GatewayError,
    /// 最后一次尝试的账号展示名（诊断用）。
    pub account: String,
    /// 最后一次尝试的账号 uid（写日志用；选不出账号时为空）。
    pub uid: String,
    /// 上游返回体片段（可能为空）。
    pub upstream_message: String,
    /// 本轮出站改写是否动过黑名单指纹（成因判定用，见 [`should_degrade_retry`]）。
    pub sanitized: bool,
}

/// 等**首帧**再决定：先取一块字节，若它本身就是错误信封就判成失败（可换号）。
///
/// ## 为什么必须在这里做
///
/// 这是整个链路里**唯一**还能换号的位置：再往后（路由把 `Response` 交给 axum）
/// 状态码与响应头就已经发出去了，错误只能「记冷却」，客户端必然拿到 200。
///
/// ## 为什么可以安全地「取一块再还回去」
///
/// 把 body 拆成 `data_stream`，取出首块后用 `iter(head).chain(rest)` 重新拼回去
/// —— 客户端收到的**字节序列与顺序完全不变**（首块只是被网关看了一眼）。
///
/// ## 超时
///
/// 上游迟迟不发首块时**不能**把整个请求卡在这里（此前卡住的是客户端读流，
/// 网关自己的 handler 是空闲的）。所以等待有上限，超时即原样交出去 ——
/// 宁可不判，也不能把「上游慢」变成「网关挂」。
pub(crate) async fn peek_first_frame(
    mut response: reqwest::Response,
) -> (
    reqwest::Response,
    Option<axum::body::Bytes>,
    Option<crate::protocol::error_sniffer::SniffedError>,
) {
    // 等首帧的上限：正常首字节在秒级，20s 足够覆盖慢上游。
    const FIRST_FRAME_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

    let first = match tokio::time::timeout(FIRST_FRAME_WAIT, response.chunk()).await {
        Ok(Ok(Some(chunk))) => chunk,
        // 超时 / 传输错误 / 空体：都不判，原样交回。
        _ => return (response, None, None),
    };

    if let Ok(text) = std::str::from_utf8(&first) {
        if let Some(found) = crate::protocol::error_sniffer::sniff_sse_chunk(text) {
            // 首帧即错误 ⇒ 调用方丢弃这个响应换号（客户端一个字节都没收到）。
            return (response, None, Some(found));
        }
    }
    // 不是错误：把这一块交回调用方补到流首（`reqwest::Response` 不能「放回去」）。
    (response, Some(first), None)
}

/// 把 peek 取走的首块**拼回流首**，得到完整的字节流。
///
/// `prefix = None` 时与 `response.bytes_stream()` 逐字等价 —— 没 peek 过的请求零开销。
pub fn prefix_stream(
    prefix: Option<axum::body::Bytes>,
    response: reqwest::Response,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> {
    use futures_util::StreamExt;
    let head: Vec<Result<axum::body::Bytes, reqwest::Error>> = prefix.into_iter().map(Ok).collect();
    futures_util::stream::iter(head).chain(response.bytes_stream())
}

/// 非流式：把 peek 取走的首块拼回正文，再读出剩余全部字节。
pub async fn body_text_with_prefix(
    prefix: Option<axum::body::Bytes>,
    response: reqwest::Response,
) -> String {
    let mut text = String::new();
    if let Some(bytes) = prefix {
        text.push_str(&String::from_utf8_lossy(&bytes));
    }
    text.push_str(&response.text().await.unwrap_or_default());
    text
}

#[allow(clippy::result_large_err)]
// `RelayFailure` 承载完整上游响应与诊断上下文，装箱会丢掉「一次就能定位」的可读性。
/// 把一次请求送到上游；内含换号轮换与失败治理。
pub async fn relay(
    state: &GatewayState,
    request: RelayRequest,
) -> Result<RelayOutcome, RelayFailure> {
    let mut now_ms = timeutil::now_ms();
    let realm = realm_of(request.region);
    let config = state.config_snapshot().await;
    let tries = config.max_rotate.max(1);

    // 与账号库对齐（增量 upsert，不删除既有账号以保留治理历史）。
    //
    // ★ 排除表也在这里同步进池：它是**硬约束**，选号 / 兜底 / 偏好都要看它。
    //   放在「取账号」同一处，是为了让「池内容」与「排除集」永远同源同版本 ——
    //   两处各同步一次，迟早出现「新加的排除项要等下一次请求才生效」。
    {
        let scope = scope_key(request.region);
        let accounts = account::load_accounts_for(request.region);
        let mut pool = state.pool.write().await;
        pool.sync_accounts(&accounts, Some(realm));
        pool.sync_exclusions(scope, &exclusions::load_for(scope));
    }

    // ★ issue #76：**配额判定在选号之前**（规范 §6.2）——
    //   超限的请求不占用在途额度、不触发换号，一步都不发。
    //   放在这里（账号同步后、粘性前）是因为：判定要用 `state.key_usage`，
    //   而记账口径（period）要跟账号库无关地按 Key 自己的策略走。
    if let Some(policy) = request.key_policy.as_ref() {
        let now = timeutil::now_ms();
        let used = state.key_usage.used_for(
            &request.key_id_for_policy(),
            policy.quota_unit,
            policy.quota_period,
            now,
        );
        // 请求数在**进入网关后**记（业务代理没有 usage 回包，只有请求数可计）；
        // token 在流结束记（`UsageTap` 的 sink，见 `logging.rs` 同款接法）。
        state
            .key_usage
            .record_request(&request.key_id_for_policy(), policy.quota_period, now);
        if let crate::apikey::QuotaVerdict::Exceeded {
            used,
            limit,
            unit,
            period,
        } = crate::apikey::quota_verdict_for_policy(policy, used)
        {
            return Err(RelayFailure {
                error: GatewayError::KeyQuotaExceeded {
                    used,
                    limit,
                    unit,
                    period,
                },
                // 配额拒绝发生在**选号之前** ⇒ 没有账号可报（与模型闸门短路同一形状）。
                account: String::new(),
                uid: String::new(),
                upstream_message: String::new(),
                sanitized: false,
            });
        }
    }

    // 会话粘性键；客户端没给会话 id 时为 `None`（整段跳过粘性，保持原行为）。
    let sticky_key = sticky_key_of(&request);

    // 会话上下文：轮内所有重试复用同一实例。
    let context = ConversationContext {
        conversation_id: session_headers::resolve_conversation_id(
            request.conversation_id.as_deref(),
            sticky_key.as_deref(),
        ),
        conversation_request_id: request.conversation_request_id.clone(),
        trace_id: request.trace_id.clone(),
    };
    let message_id = session_headers::new_hex_id();

    let mut tried: HashSet<String> = HashSet::new();
    let mut last: Option<RelayFailure> = None;
    // 本轮尝试轨迹（换号链路）。**每个失败出口都要带上它**，否则用户无法自查
    // 「到底换号了没有、换到了谁、为什么都不行」。
    let mut attempts: Vec<UpstreamAttempt> = Vec::new();
    // 同一模型在本轮**已经失败过几个账号**。达到 2 个即判定为模型级问题、停止换号
    // （方案 §9.4：继续把第 3 次重试撞在同一堵墙上只会烧配额）。
    let mut model_failures = 0usize;

    // ★ issue #76：本密钥的允许名单（`None` = 不限额）。
    //
    // **整轮只算一次**：分组展开要读一次账号库，而名单在一次请求内不会变 ——
    // 放进循环只是白读 N 次。更关键的是循环外的失败兜底也要用它算名单口径。
    let allowed = request
        .key_policy
        .as_ref()
        .and_then(|policy| allowed_uids_for(policy, request.region));

    for attempt in 0..tries {
        // ★ 每轮重取时间：冷却靠**墙钟**到期，而重试之间真的会过去几十毫秒~几秒。
        //   用入口那个快照会让「刚到期」的号在这一轮里仍被判为冷却中。
        now_ms = timeutil::now_ms();
        // ★ 模型闸门（跨请求）：同一模型已在多个账号上失败 ⇒ 这一轮**一个请求都不发**。
        //
        // 与下面的「本轮第 2 个账号失败即停」是两个层次：那条只能挽回**当前**这一轮，
        // 这条让**后续请求**也直接快速失败 —— 否则每个新请求都要重新撞一遍同样的墙。
        if let Some(block) = state.pool.write().await.admit_model(&request.model, now_ms) {
            // ★★ issue #106：闸门短路必须**自带错误**，不能落到下面 `last == None` 的兜底。
            //
            // 改造前这里只 push 一条 attempt 就 `break`，于是 `last` 仍是 `None` ⇒ 走
            // `pool_unavailable_error`，而 `PoolBusy` 的判据（可用号全在途满）在这种场景
            // 不成立 ⇒ 一律 **401 `no_credential`**。客户端把 401 归为 `AUTH`、不可重试、
            // 渲染成「API 密钥无效」⇒ 整轮会话终止。实测证据：那次耗时 19ms、账号列为空。
            //
            // 语义上这是「上游此刻对该模型整体限流」= 可重试的临时不可用 ⇒ 503 + Retry-After。
            let remain_secs = (block.remain_ms.max(0) as u64).div_ceil(1000);
            let reason = if block.remain_ms > 0 {
                format!(
                    "{} 整体暂停（窗口内已失败 {} 次，剩 {}s）",
                    block.model, block.failures, remain_secs
                )
            } else {
                format!("{} 半开试探名额已被其它请求占用", block.model)
            };
            attempts.push(UpstreamAttempt {
                account: String::new(),
                uid: String::new(),
                status: 0,
                reason,
            });
            // 本轮**一次都没打到上游**时才用它：否则用户更需要看的是「第 1 轮为什么失败」
            // （那条错误自身已经带着完整的 `attempts`）。
            if last.is_none() {
                last = Some(RelayFailure {
                    error: GatewayError::ModelGatePaused {
                        model: block.model,
                        remain_secs,
                        failures: block.failures,
                        attempts: attempts.clone(),
                    },
                    account: String::new(),
                    uid: String::new(),
                    upstream_message: String::new(),
                    sanitized: request.sanitized,
                });
            }
            break;
        }
        // 粘性只是**偏好**：查不到就按原逻辑选号（读锁，不占选号要用的写锁）。
        let preferred_uid = match sticky_key.as_deref() {
            Some(key) => state
                .sticky
                .read()
                .await
                .get(key, now_ms)
                .map(str::to_string),
            None => None,
        };
        // ★ issue #76：本密钥的允许名单（循环外已算好，见函数上方注释）。
        let selected = select_account(
            state,
            request.region,
            &request.model,
            realm,
            &tried,
            now_ms,
            attempt,
            preferred_uid.as_deref(),
            allowed.as_ref(),
        )
        .await;

        let Some(account_value) = selected else {
            // 池与策略都给不出账号：若已有失败记录，返回它（更有诊断价值）。
            //
            // ★ 只有当**本轮一次都没打到上游**时才补这条 —— 否则它会污染换号轨迹
            //   （第 2 轮选不出号时，用户更需要看的是「第 1 轮为什么失败」）。
            if attempts.is_empty() {
                attempts.push(UpstreamAttempt {
                    account: String::new(),
                    uid: String::new(),
                    status: 0,
                    reason: "没有满足条件的账号（已排除 / 冷却 / 在途满 / 全被限流）".to_string(),
                });
            }
            break;
        };

        let uid = account::get_str(&account_value, "uid").unwrap_or_default();
        let account_name = account::account_display_name(&account_value);

        // ★ E1：走到第 2 轮及以后说明**上一轮没能用这个账号** ⇒ 记一次「换号」。
        //   判据放在**选号成功之后**：只有真的换到另一个账号才算，「同一账号重试」不计。
        if attempt > 0 && !uid.is_empty() {
            state.proxy_kpi.record_switch();
        }

        // 在途租约：占满则**先排队等一会儿**（`acquire_timeout_ms`），超时才换号。
        //
        // ★ 为什么不是「满了立刻换号」：并发稍高（客户端并行 tool calls）时，
        //   账号只是**此刻**忙，立即换号会把「忙」误报成「这个号不可用」——
        //   用户看到的是「网关拒绝服务」。排队 2s 能吃掉绝大部分瞬时拥塞。
        //
        // ★★ 租约必须绑定到变量（PR #41）：`Drop` 归还额度，于是正常返回、
        //   提前 `continue`、以及**handler future 被 axum 取消**都会归还；
        //   手写的「用完再 release」在取消路径上那行永不执行 ⇒ in_flight 漏到整池拒绝。
        let _lease = if uid.is_empty() {
            None
        } else {
            acquire_with_wait(state, &uid, config.pool.acquire_timeout_ms).await
        };
        if !uid.is_empty() && _lease.is_none() {
            attempts.push(UpstreamAttempt {
                account: account_name.clone(),
                uid: uid.clone(),
                status: 0,
                reason: "在途已满（排队超时）".to_string(),
            });
            tried.insert(uid);
            continue;
        }

        // ★ 出站按 `kind` 分派 —— **治理层（本函数其余部分：选号 / 在途 / 冷却 / 熔断 / 换号）完全共用**。
        let result = match &request.kind {
            RelayKind::Llm => {
                // 会话头族 + 派生设备标识（**LLM 专用**：业务请求不适用，故放在本分支内）。
                let mut extra = session_headers::conversation_headers(&context, &message_id);
                if !uid.is_empty() {
                    extra.extend(session_headers::device_headers(&uid));
                }
                state
                    .upstream
                    .chat_stream_with(
                        request.region,
                        &account_value,
                        &request.prepared_body,
                        None,
                        Some(&extra),
                    )
                    .await
            }
            // 业务请求：**body 原样透传**（不是 LLM 协议，不做改写）。
            RelayKind::Biz { path, method } => {
                state
                    .upstream
                    .forward_biz_with(
                        request.region,
                        &account_value,
                        path,
                        &request.prepared_body,
                        None,
                        method.as_reqwest(),
                    )
                    .await
            }
        };

        // ★ 这里原本有一句手写的 `release(&uid)`。现在归还由 `_lease` 的 `Drop` 负责
        //   （见上），**不要再加回来**：两句 release 会把计数压成负数（虽然有幂等保护，
        //   但语义上会让「同账号并发数」永久少算）。

        match result {
            UpstreamChatResult::Ok(ok) => {
                // ★★ **等首帧再决定**（2026-10-17）。
                //
                // 上游最常见的「200 信封错误」形态是：状态码 2xx、**第一帧**就是
                // `{"code":6004,...}` 或 `event: error`。此时一个字节都还没发给客户端 ——
                // 这是唯一还能**换号重试**的时机。
                //
                // 判据放在 `note_success` **之前**：一旦先记了成功再发现是错误，
                // EMA 与模型闸门都会被污染（「失败被记成成功」比不记更坏）。
                let (response, prefix, first_frame_error) = peek_first_frame(ok.response).await;
                if let Some(envelope) = first_frame_error {
                    if !uid.is_empty() {
                        let event = envelope.event.clone();
                        let (disabled, gate_opened) = {
                            let mut pool = state.pool.write().await;
                            let disabled =
                                pool.apply_upstream_error(&uid, &request.model, &event, now_ms);
                            let gate_opened = if is_model_level(&event) {
                                pool.note_model_failure(&request.model, now_ms)
                            } else {
                                false
                            };
                            (disabled, gate_opened)
                        };
                        tried.insert(uid.clone());
                        // 粘性立刻解绑：这个号在这一轮已经失败过，下一轮必须换。
                        if let Some(key) = sticky_key.as_deref() {
                            state.sticky.write().await.unbind(key);
                        }
                        attempts.push(UpstreamAttempt {
                            account: account_name.clone(),
                            uid: uid.clone(),
                            status: envelope.status,
                            reason: format!("200 但首帧即错误：{}", envelope.message),
                        });
                        last = Some(RelayFailure {
                            error: GatewayError::Upstream {
                                kind: upstream_kind_of(&event),
                                status: envelope.status,
                                message: envelope.message.clone(),
                                attempts: attempts.clone(),
                            },
                            account: account_name,
                            uid,
                            upstream_message: envelope.message,
                            sanitized: request.sanitized,
                        });
                        if disabled || gate_opened || !should_rotate(&event, attempt) {
                            break;
                        }
                        continue;
                    }
                }
                if !uid.is_empty() {
                    // 成功：清熔断域与退避指数。此处 usage 尚未到达，故不写账本——
                    // 账本由调用方在拿到 usage 后经 [`record_ledger`] 回填。
                    //
                    // ★ 同一次锁里顺手**关掉模型闸门**：这一次真的通了，就是「该模型可用」
                    //   最强的证据，不该再让它被一个陈旧的暂停挡着。
                    {
                        let mut pool = state.pool.write().await;
                        pool.note_success(&uid, &request.model, 0.0, 0, now_ms);
                        pool.note_model_success(&request.model);
                    }
                    // 会话粘性：**只在成功之后**绑定，让下一轮优先复用这个账号。
                    if let Some(key) = sticky_key.as_deref() {
                        state.sticky.write().await.bind(key, &uid, now_ms);
                    }
                }
                return Ok(RelayOutcome::Ok {
                    response,
                    prefix,
                    account: account_name,
                    uid,
                });
            }
            UpstreamChatResult::Err {
                kind,
                status,
                message,
            } => {
                let event = classify_event(status, &message);
                let (disabled, gate_opened) = if uid.is_empty() {
                    (false, false)
                } else {
                    let mut pool = state.pool.write().await;
                    let disabled = pool.apply_upstream_error(&uid, &request.model, &event, now_ms);
                    // 只有**模型级**失败才计入闸门：账号级 429 / 402 / 5xx 是「那个号的事」，
                    // 计进来会让「一个号额度耗尽」把整个模型停掉（误触发比不触发更糟）。
                    let gate_opened = if is_model_level(&event) {
                        pool.note_model_failure(&request.model, now_ms)
                    } else {
                        false
                    };
                    (disabled, gate_opened)
                };
                if !uid.is_empty() {
                    tried.insert(uid.clone());
                    // 会话粘性：这个账号刚失败 ⇒ 立刻**解绑**，下一轮换号重新绑。
                    // 粘性是优化而非约束，绝不能因为粘性而反复撞同一堵墙。
                    if let Some(key) = sticky_key.as_deref() {
                        state.sticky.write().await.unbind(key);
                    }
                }

                attempts.push(UpstreamAttempt {
                    account: account_name.clone(),
                    uid: uid.clone(),
                    status,
                    reason: describe_event(&event),
                });

                last = Some(RelayFailure {
                    error: GatewayError::Upstream {
                        kind,
                        status,
                        message: message.clone(),
                        attempts: attempts.clone(),
                    },
                    account: account_name,
                    uid,
                    upstream_message: message,
                    sanitized: request.sanitized,
                });

                // 账号已禁用 → 重试无意义。
                if disabled {
                    break;
                }

                // 会话失效标记：此处 `matches!` 仅用于文档化分支意图。
                debug_assert!(
                    !matches!(event, UpstreamEvent::SessionDead) || disabled,
                    "会话失效未达阈值时不禁用，应继续换号重试"
                );

                // ① 按**失败类型**决定值不值得换号（方案 §9.2）。
                //    参数错（400/422）换号只是把同一份坏参数喂给下一个号 —— 白烧配额。
                if !should_rotate(&event, attempt) {
                    break;
                }

                // ② 模型级失败累计到 2 个账号 ⇒ 判定为**模型**的问题，停止换号。
                //    这是「更合理」的关键：3 次重试全打在同一个模型上没有任何收益。
                if is_model_level(&event) {
                    model_failures += 1;
                    // 闸门刚被打开（跨请求阈值命中）⇒ 立即停：这一轮再换号也没有意义。
                    if gate_opened || model_failures >= MODEL_FAILURE_LIMIT {
                        break;
                    }
                }
            }
        }
    }

    let failure = match last {
        Some(failure) => failure,
        // 首轮就选不出账号 ⇒ 这里能给出**具体原因**，而不是笼统的「未找到已登录账号」。
        //
        // ★ issue #26：改造前这一档与「从没登录过」共用同一句文案（「请在桌面端登录
        //   该版本后重试」），于是「号在库里、但全被禁用 / 冷却」的用户被引去做了一件
        //   **不可能有用**的事（报障人：切换过多个账号、桌面端和 Buddy Switch 都试过）。
        //   池快照就在手边，把事实摆出来即可。
        None => {
            let (summary, scoped) = {
                let pool = state.pool.read().await;
                let summary = pool.summary(now_ms, Some(realm));
                // ★ issue #76：白名单生效时再取一份**名单口径**快照（同一个读锁，
                //   不多占锁）。白名单用户看到的必须是「他允许用的那几个号」的状态。
                let scoped = allowed
                    .as_ref()
                    .map(|uids| pool.summary_of(uids, now_ms, Some(realm)));
                (summary, scoped)
            };
            RelayFailure {
                // ★ 不要再写死 `NoCredential`：全部健康只是在途占满时那是 401 误报，
                //   会让客户端去重建登录态（见 `pool_unavailable_error`）。
                error: pool_unavailable_error(request.region, &summary, scoped.as_ref()),
                account: String::new(),
                uid: String::new(),
                upstream_message: String::new(),
                sanitized: request.sanitized,
            }
        }
    };
    // 唯一失败出口 ⇒ 在这里记一次即可覆盖全部失败路径（含「没有可用账号」）。
    // 上游原文优先于错误码文案：用户排查时「402 余额不足」比「no_credential」有用。
    state
        .note_last_error(if failure.upstream_message.is_empty() {
            failure.error.message()
        } else {
            failure.upstream_message.clone()
        })
        .await;
    Err(failure)
}

/// 同一模型在本轮失败多少个账号后**停止换号**。
///
/// 2 就够了：第 1 个号 429 可能是这个号的事，第 2 个号也 429 就基本可以判定
/// 「是这个模型此刻整体不可用」。继续换第 3 个号只会再烧一次配额，
/// 而用户拿到的错误反而更晚 —— 方案 §9.4 的第 1 条。
const MODEL_FAILURE_LIMIT: usize = 2;

/// WB 侧排除表的作用域键（与 `exclusions` 文件里的键一一对应）。
pub fn scope_key(region: Region) -> &'static str {
    match region {
        Region::Cn => "wb:cn",
        Region::Global => "wb:global",
    }
}

/// 这类失败值不值得**换号重试**（方案 §9.2 的决策矩阵，唯一实现）。
///
/// 两条反例最贵：
/// - **400 / 422 参数错**：同一份坏参数喂给下一个号 = 白烧 N 个账号的配额；
/// - **404**：可能是「该账号的模型清单里没有它」，换 **1 次**足够，第 3 次纯浪费。
fn should_rotate(event: &UpstreamEvent, attempt: usize) -> bool {
    match event {
        // 换号能救的：额度不足、限流、模型被封、上游抖动、会话失效。
        UpstreamEvent::HardCredit
        | UpstreamEvent::SoftRate { .. }
        | UpstreamEvent::ModelBlocked
        | UpstreamEvent::Server
        | UpstreamEvent::SessionDead => true,
        // 401/403 换号能救：池里其它号的凭据可能好好的（issue #100）。
        // 旧实现把它归到 `Client`（不换号）⇒ 池里明明有健康账号，客户端却直接吃到 401。
        UpstreamEvent::AuthRejected => true,
        // 只换一次（`attempt == 0` 时还允许再试一个号）。
        UpstreamEvent::NotFound => attempt == 0,
        // 调用方自己的问题，换谁都一样。
        UpstreamEvent::Client => false,
    }
}

/// 流内信封错误的接收方：把治理副作用丢到**后台任务**（`poll_next` 里不能 `.await`）。
///
/// 为什么敢 spawn：治理是幂等的（写冷却 / 计数闸门），晚几十毫秒无影响；
/// 而阻塞流会让客户端感知到「卡住」——那是更坏的结果。
pub struct EnvelopeErrorSink {
    state: GatewayState,
    uid: String,
    model: String,
}

impl crate::protocol::usage_tap::StreamErrorSink for EnvelopeErrorSink {
    fn on_error(&self, error: crate::protocol::error_sniffer::SniffedError) {
        let state = self.state.clone();
        let uid = self.uid.clone();
        let model = self.model.clone();
        tokio::spawn(async move {
            note_envelope_failure(&state, &uid, &model, &error.event).await;
        });
    }
}

/// 构造流内错误接收方；`uid` 为空（没选到账号）时返回 `None` —— 没有账号就没有可冷却的对象。
pub fn envelope_error_sink(
    state: &GatewayState,
    uid: &str,
    model: &str,
) -> Option<std::sync::Arc<dyn crate::protocol::usage_tap::StreamErrorSink>> {
    if uid.is_empty() {
        return None;
    }
    Some(std::sync::Arc::new(EnvelopeErrorSink {
        state: state.clone(),
        uid: uid.to_string(),
        model: model.to_string(),
    }))
}

/// **200 信封错误**的治理副作用（冷却 / 熔断 / 模型闸门）。三条路由共用。
///
/// ## 为什么要有这个函数
///
/// `chat` / `messages` / `responses` 三条路由各自有一份「上游 200 但响应体是错误」的
/// 处理分支。若各写一遍「写冷却 + 计闸门」，迟早出现「某条路由忘了计模型闸门」
/// 这种只在统计上可见的漂移。这里收成一处，路由只负责**错误体形状**。
///
/// `uid` 为空（未选到账号）时什么也不做 —— 与 HTTP 非 2xx 的治理路径一致。
pub async fn note_envelope_failure(
    state: &GatewayState,
    uid: &str,
    model: &str,
    event: &UpstreamEvent,
) {
    if uid.is_empty() {
        return;
    }
    let now_ms = timeutil::now_ms();
    let mut pool = state.pool.write().await;
    pool.apply_upstream_error(uid, model, event, now_ms);
    if is_model_level(event) {
        pool.note_model_failure(model, now_ms);
    }
}

/// 上游事件 → 对外错误类别（`OpenAI` / `Anthropic` 两个错误体共用）。
///
/// `ModelBlocked` 对外按 `Client`（4xx）返回：模型被封禁属于「调用方看得见的拒绝」，
/// 但**冷却仍按封禁语义写**（6h 起逐次加倍），两者不冲突。
pub fn upstream_kind_of(
    event: &UpstreamEvent,
) -> buddy_switch_core::modules::upstream::UpstreamErrorKind {
    use buddy_switch_core::modules::upstream::UpstreamErrorKind;
    match event {
        UpstreamEvent::HardCredit => UpstreamErrorKind::HardCredit,
        UpstreamEvent::SoftRate { .. } => UpstreamErrorKind::SoftRate,
        UpstreamEvent::SessionDead => UpstreamErrorKind::SessionDead,
        // 401/403 对**客户端**的语义就是「这个账号的凭据不可用了」⇒ 对外仍是 401，
        // 由文案把「换号后仍失败时该去重新登录」讲清楚。
        UpstreamEvent::AuthRejected => UpstreamErrorKind::SessionDead,
        UpstreamEvent::NotFound => UpstreamErrorKind::NotFound,
        UpstreamEvent::Server => UpstreamErrorKind::Server,
        UpstreamEvent::ModelBlocked | UpstreamEvent::Client => UpstreamErrorKind::Client,
    }
}

/// 该事件是否**归因到模型**（而不是某个账号）。
///
/// 只有这两类：模型级 429（`6004`）与模型封禁（`11102`）。
/// 账号级 429 / 402 / 5xx 是「这个号的事」，换个号大概率就好 —— 不该计入模型失败数。
///
/// `pub(crate)`：`chat.rs` 的「200 信封错误」处置也要用同一判据
/// （否则同一类失败在两条路径上对闸门的贡献不同 —— 那正是最难查的漂移）。
pub(crate) fn is_model_level(event: &UpstreamEvent) -> bool {
    matches!(
        event,
        UpstreamEvent::SoftRate {
            model_scoped: true,
            ..
        } | UpstreamEvent::ModelBlocked
    )
}

/// 事件 → 一句可读原因（写进 `attempts[]`，给用户自查用）。
fn describe_event(event: &UpstreamEvent) -> String {
    match event {
        UpstreamEvent::HardCredit => "402 / 余额不足 → 硬冷却至次日 04:00".to_string(),
        UpstreamEvent::SoftRate {
            model_scoped: true, ..
        } => "429 模型级限流（6004）→ 只冷该模型".to_string(),
        UpstreamEvent::SoftRate {
            model_scoped: false,
            ..
        } => "429 账号级限流 → 软冷却".to_string(),
        UpstreamEvent::ModelBlocked => "11102 模型被封禁 → 退避".to_string(),
        UpstreamEvent::NotFound => "404 上游未找到 → 浅冷却 60s".to_string(),
        UpstreamEvent::Server => "5xx 上游异常 → 计入熔断".to_string(),
        UpstreamEvent::SessionDead => "12153 会话失效 → 三振禁用".to_string(),
        // issue #100：401/403 不再归到「调用方错误」—— 那是**该账号的凭据**被拒。
        UpstreamEvent::AuthRejected => {
            "401/403 凭据被拒 → 浅冷却 60s 并换号（保活刷新后可自愈）".to_string()
        }
        UpstreamEvent::Client => "4xx 调用方错误（不换号）".to_string(),
    }
}

/// 申请在途额度；占满时**轮询等待**至多 `timeout_ms`，超时才返回 `None`。
///
/// 返回的是**租约**而不是 `bool`（PR #41）：调用方拿到的那个值一旦被 drop 就归还额度，
/// 因此「占到了却忘了还」在类型层面不再可能发生。
///
/// ## 为什么是轮询而不是 `Notify`
///
/// `Pool::acquire` 是 `&mut self` 同步方法，而 `Pool` 被 `RwLock` 包着。
/// 加 `Notify` 要在池里引入等待者队列与唤醒时机（谁 release 谁 notify），
/// 出错的方式比轮询更隐蔽。这里 50ms 一跳、最长 2s（6 次/40 次），
/// 对「本机单用户工具」的量级完全够用；真需要时再换成信号量。
async fn acquire_with_wait(
    state: &GatewayState,
    uid: &str,
    timeout_ms: i64,
) -> Option<InFlightLease> {
    // 每次尝试都重新 clone 一份 Arc：`InFlightLease` 靠它跨 await 存活，不能借 `&state`。
    if let Some(lease) = InFlightLease::acquire(Arc::clone(&state.pool), uid).await {
        return Some(lease);
    }
    if timeout_ms <= 0 {
        return None;
    }
    let deadline = timeutil::now_ms().saturating_add(timeout_ms);
    while timeutil::now_ms() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Some(lease) = InFlightLease::acquire(Arc::clone(&state.pool), uid).await {
            return Some(lease);
        }
    }
    None
}

/// 把 Key 策略的**账号范围**展开成 uid 集合（`None` = 全部账号，不过滤）。
///
/// 分组在这里展开成 uid：池条目不认识「分组」这个概念（`PoolEntryLike` 没有
/// `group()`，为它加一个 trait 方法会波及三条产品线的实现方与全部测试 fake）。
/// 账号库就在手边（`load_accounts_for`），展开一次的成本低于为分组改 trait。
///
/// ★ 展开**只增不删**：`scope_accounts` 里的 uid 即使账号库已删除也保留 ——
///   那是用户显式写进去的，删掉会让「改完策略发现少一个号」无从解释。
fn allowed_uids_for(policy: &crate::apikey::KeyPolicy, region: Region) -> Option<HashSet<String>> {
    use crate::apikey::ScopeMode;
    if policy.scope_mode != ScopeMode::Selected {
        return None;
    }
    let mut allowed: HashSet<String> = policy.scope_accounts.iter().cloned().collect();
    if !policy.scope_groups.is_empty() {
        for account in account::load_accounts_for(region) {
            let group = account::get_str(&account, "groupName").unwrap_or_default();
            if group.is_empty() {
                continue;
            }
            if policy.scope_groups.iter().any(|wanted| wanted == &group) {
                if let Some(uid) = account::get_str(&account, "uid") {
                    allowed.insert(uid);
                }
            }
        }
    }
    Some(allowed)
}

/// 选号：优先账号池（有治理状态），池给不出时回落既有策略。
/// 由请求推导**会话粘性键**；客户端未给会话 id ⇒ `None`（整段跳过粘性）。
///
/// ★ 缺失时必须返回 `None`，**不得**退化成轮主键（`conversation_request_id` 每轮都换
/// ⇒ 绑定永不命中，接线等于没接；详见 [`sticky::sticky_key`] 的文档）。
///
/// 区域与模型都进键：跨区域的账号库本就分家；同一会话切模型时应允许落到各自最合适的账号
/// （否则「模型级限流只封锁该模型」的优势会被粘性抵消）。
fn sticky_key_of(request: &RelayRequest) -> Option<String> {
    request
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| sticky::sticky_key(request.region.as_str(), &request.model, value))
}

#[allow(clippy::too_many_arguments)]
// 同上：扁平签名与调用点一一对应，更好读。
async fn select_account(
    state: &GatewayState,
    region: Region,
    model: &str,
    realm: RealmTag,
    tried: &HashSet<String>,
    now_ms: i64,
    attempt: usize,
    sticky_uid: Option<&str>,
    allowed: Option<&HashSet<String>>,
) -> Option<Value> {
    // 偏好与策略**都来自同一个枚举** `AccountStrategy`，因此这里一次算清：
    // - `Pinned`    ⇒ 加权选号 + 偏好 uid（指定是偏好，不是约束）；
    // - `MaxCredits`⇒ 候选内按积分择优（**粘性不参与**：否则「按积分择优」会被粘性绑架成
    //                 「总落在同一个号上」，用户看到的又是假开关）；
    // - `Random`    ⇒ 候选内均匀抽签（同上，粘性不参与）；
    // - `Sequential`⇒ 粘住当前号、不可用才换（**粘性天然重复，故不参与**：它自己就是
    //                 「会话级粘性」的账号级版本，再叠一层只会让换号多绕一步）；
    // - `Current`   ⇒ 加权选号 + 粘性（粘性是自动择优的子优化）。
    let strategy = state.strategy_for(region).await;
    let (mode, preferred) = match &strategy {
        AccountStrategy::Pinned { account_id } if !account_id.is_empty() => {
            (PickMode::Weighted, Some(account_id.as_str()))
        }
        AccountStrategy::MaxCredits => (PickMode::MaxCredits, None),
        AccountStrategy::Random => (PickMode::Random, None),
        AccountStrategy::Sequential => (PickMode::Sequential, None),
        _ => (PickMode::Weighted, sticky_uid),
    };

    let (picked_uid, pool_empty) = {
        let mut pool = state.pool.write().await;
        if pool.is_empty() {
            (None, true)
        } else {
            let seed = (now_ms as u64)
                .wrapping_mul(31)
                .wrapping_add(attempt as u64)
                .wrapping_add(0x9E37_79B9_7F4A_7C15);
            // 偏好不可用时由 `pick_with_preference` 自行回落到自动择优；
            // 排除项由池自己持有（`sync_exclusions`），无需在这里传。
            (
                pool.pick_account(
                    now_ms,
                    Some(realm),
                    model,
                    tried,
                    mode,
                    seed,
                    preferred,
                    allowed,
                ),
                false,
            )
        }
    };

    if let Some(uid) = picked_uid {
        // 池里存在但账号库已删除 → 该 uid 无凭据可用，落回策略选择。
        if let Some(account_value) = account::find_account_for(region, &uid) {
            return Some(account_value);
        }
    }

    // ★★ 池里**有账号**但选不出来 ⇒ **不再回落策略**（2026-10-17，本 bug 的第三根因）。
    //
    // 旧行为：回落到 `AccountSelector`（默认 `Current` = 读桌面端认证文件 = 客户端当前登录的号），
    // 而该回落**只判 `tried`**，不看模型级冷却 / 熔断 / 在途 —— 于是把池刚刚判定为
    // 「不可用」的账号又交回来，下一轮再撞同一堵墙，用户看到的是
    // 「明明有别的号可用，请求却一直失败，而且看起来根本没换号」。
    //
    // 现在：池非空就**只信池**，给不出就返回 `None`，由调用方给出带明细的
    // `no_credential`（几个号、几个被排除 / 冷却 / 限流）。
    if !pool_empty {
        return None;
    }

    // 回落：策略模块（**仅当池为空**时才走到这里）。
    //
    // 池为空意味着「账号库里一个带 uid 的账号都没有」（`sync_accounts` 会跳过没有 uid 的记录），
    // 此时才真的可能是「没登录 / 没导入」，也正是回落策略能派上用场的场景。
    let selector = AccountSelector;
    match selector.select(region, &strategy).await {
        Ok(account_value) => {
            let uid = account::get_str(&account_value, "uid").unwrap_or_default();
            // 已排除的账号**绝不能**从这里漏回来（否则「排除」只是个好看的开关）。
            if !uid.is_empty() && state.pool.read().await.is_excluded(&uid) {
                return None;
            }
            // 池已试过的账号不再重复选择（否则会立刻二次失败）。
            if !uid.is_empty() && tried.contains(&uid) {
                None
            } else {
                Some(account_value)
            }
        }
        Err(_) => None,
    }
}

/// 池快照 → `no_credential` 的具体原因（**唯一判据**）。
///
/// `total == 0` ⇒ 该区域在池里一条都没有：账号库为空，或库里的账号都没有 `uid`
/// 而被 `sync_accounts` 跳过。否则是「有号，但一个都不可用」—— 这两种处境的可行动
/// 指引完全不同（见 [`NoCredentialCause`]）。
///
/// 刻意用 `total` 而不是「`available == 0`」：后者只说「不可用」，前者才能区分
/// 「一个号都没有」与「有号但全不可用」，而正是这个区分决定了文案该不该让人去登录。
fn no_credential_cause_of(summary: &PoolSummary) -> NoCredentialCause {
    if summary.total == 0 {
        NoCredentialCause::StoreEmpty
    } else {
        NoCredentialCause::PoolExhausted {
            total: summary.total,
            disabled: summary.disabled,
            cooling: summary.cooling,
            zero_credits: summary.zero_credits,
            excluded: summary.excluded,
            in_flight_full: summary.in_flight_full,
            model_cooling: summary.model_cooling,
        }
    }
}

/// 选号失败 → 对外错误（PR #41 新增的一层判定）。
///
/// **全部可用的号都只是「在途占满」**时回 [`GatewayError::PoolBusy`]（503）：
/// 凭据、余额、冷却都没问题，回 401 `no_credential` 会把客户端（和人）引去重新登录 ——
/// 而这件事**只需要等几秒**。
///
/// 其余情形仍归 `no_credential`（401），但计数里必须带上「在途已满」，
/// 否则「N 个号、0 禁用、0 冷却、0 零余额、0 排除」与「0 个可用」自相矛盾。
///
/// ## 与 `excluded` 的关系（本地 main 的第七态，PR 原本没有）
///
/// 被排除的号**不参与** 503 判定：它不是「健康但忙」，而是「用户不让用」。
/// 所以条件是「非排除的号全都健康且都在途满」——
/// `available > 0 && in_flight_full == available && total == available + excluded`，
/// 一旦有禁用/冷却/零余额混进来就退回 401（此时「去账号池看逐号原因」才是有效指引）。
/// `scoped` 是**名单口径**的同一份快照（issue #76；白名单未生效时为 `None`）。
///
/// 有它时**一切判定按它来**：白名单用户看到的数字必须是他**允许用的那几个号**
/// 的状态。用全池口径会告诉他「12 个号都不可用」，把他推去修一堆本来就不许用的
/// 账号 —— 而真正的解是放宽名单或恢复名单里那几个。
fn pool_unavailable_error(
    region: Region,
    summary: &PoolSummary,
    scoped: Option<&PoolSummary>,
) -> GatewayError {
    let effective = scoped.unwrap_or(summary);
    let only_excluded_are_absent = effective.available + effective.excluded == effective.total;
    let all_usable_are_in_flight_full =
        effective.available > 0 && effective.in_flight_full == effective.available;
    if only_excluded_are_absent && all_usable_are_in_flight_full {
        return GatewayError::PoolBusy {
            region,
            total: effective.total,
        };
    }
    let cause = match scoped {
        Some(scoped) => scope_empty_cause(summary.total, scoped),
        None => no_credential_cause_of(summary),
    };
    GatewayError::NoCredential { region, cause }
}

/// 名单口径快照 → [`NoCredentialCause::ScopeEmpty`]（issue #76）。
///
/// `pool_total` 取自**全池**快照，只用来表达「名单覆盖了 12 个中的 2 个」；
/// 其余计数一律名单口径（与 `no_credential_cause_of` 同源，只是换了分母）。
fn scope_empty_cause(pool_total: usize, scoped: &PoolSummary) -> NoCredentialCause {
    NoCredentialCause::ScopeEmpty {
        pool_total,
        allowed: scoped.total,
        disabled: scoped.disabled,
        cooling: scoped.cooling,
        zero_credits: scoped.zero_credits,
        excluded: scoped.excluded,
        in_flight_full: scoped.in_flight_full,
        model_cooling: scoped.model_cooling,
    }
}

/// 把 `Region` 映射为池的域标记。
pub fn realm_of(region: Region) -> RealmTag {
    match region {
        Region::Cn => RealmTag::Cn,
        Region::Global => RealmTag::Global,
    }
}

/// 回填实测用量到账本（拿到 `usage` 之后调用）。
///
/// 账本是「免费/便宜的账号优先」的依据，只在拿到真实 token 数时才写入——
/// 宁可不写，也不要写 0 成本样本把好账号误标成免费。
pub async fn record_ledger(
    state: &GatewayState,
    uid: &str,
    model: &str,
    credit: f64,
    tokens: i64,
    // ★ issue #76：非流式路径的按密钥 token 记账（流式走 `stream_sinks` 的 KeyUsageSink）。
    key: Option<&crate::apikey::KeyActor>,
) {
    let now_ms = timeutil::now_ms();
    if let Some(actor) = key {
        // `record_tokens` 内部已判 `total == 0` 跳过（不写 0 样本）。
        if tokens > 0 {
            actor.record_tokens(tokens as u64, now_ms);
        }
    }
    if uid.is_empty() {
        return;
    }
    state
        .pool
        .write()
        .await
        .record_ledger(uid, model, credit, tokens, now_ms);
}

/// 从聚合后的 usage 字段提取 `(credit, token 总数)`。
pub fn usage_of(usage: Option<&Value>) -> (f64, i64) {
    let Some(usage) = usage else {
        return (0.0, 0);
    };
    let credit = usage.get("credit").and_then(Value::as_f64).unwrap_or(0.0);
    let prompt = usage
        .get("prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("completion_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (credit, prompt.max(0) + completion.max(0))
}

/// 构造出站改写所需的元信息。
pub fn outbound_meta(uid_hint: &str, conversation_id: Option<&str>) -> OutboundMeta {
    OutboundMeta {
        uid: uid_hint.to_string(),
        conversation_id: conversation_id.map(str::to_string),
    }
}

/// 便捷函数：读降级门并执行出站改写，同时解析本轮会话主键。
#[allow(clippy::too_many_arguments)]
pub async fn prepare_body(
    state: &GatewayState,
    region: Region,
    raw_body: &str,
    uid_hint: &str,
    conversation_id: Option<&str>,
    turn_key: Option<&str>,
    inbound_request_id: Option<&str>,
) -> PreparedBody {
    let now_ms = timeutil::now_ms();
    let degraded = state.degrade_active(now_ms).await;
    let options = state.outbound_options().await;
    let meta = outbound_meta(uid_hint, conversation_id);

    let prepared =
        outbound::prepare_outbound_body_report(region, raw_body, &options, &meta, degraded);

    let request_id =
        session_headers::resolve_conversation_request_id(inbound_request_id, None, turn_key);
    PreparedBody {
        body: prepared.body,
        conversation_request_id: request_id,
        sanitized: prepared.sanitized,
    }
}

/// 出站改写结果：请求体 + 会话轮主键 + 脱敏诊断位。
pub struct PreparedBody {
    /// 发往上游的请求体。
    pub body: String,
    /// 会话轮主键（轮内复用）。
    pub conversation_request_id: String,
    /// 本轮是否改写过黑名单指纹（透传给 [`RelayRequest::sanitized`]）。
    pub sanitized: bool,
}

/// 触发降级期（内容拦截时调用）。
pub async fn trip_degrade(state: &GatewayState) -> bool {
    state.trip_degrade(timeutil::now_ms()).await
}

/// 当前是否处于降级期。
pub async fn degrade_gate(state: &GatewayState) -> DegradeGate {
    state.degrade.read().await.clone()
}

/// 判断是否为渠道校验拦截（400 + conversation id missing / channel verification / 渠道校验）。
///
/// 上游对内容审核与渠道校验回同一个 HTTP 400 + code: 11128，但两者处置截然不同：
/// - 内容审核（blocked by security policy 等）：属于提示词风控，走降级重试；
/// - 渠道校验（conversation id missing / 渠道校验）：属于客户端请求头族不齐或渠道未认证，
///   重试换号或降级提示词毫无意义，换多少次都是同一套头，白耗账号配额。
pub fn is_channel_invalid(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("conversation id missing")
        || lower.contains("channel verification")
        || message.contains("渠道校验")
        || lower.contains("unapproved channel")
}

/// 判断是否为「内容拦截」类失败（用于决定是否启用降级重试）。
pub fn is_content_blocked(status: u16, message: &str) -> bool {
    // 渠道校验先排除：虽同为 11128，但绝不能走降级重试链
    if is_channel_invalid(message) {
        return false;
    }
    if status == 400 && (message.contains("11128") || message.contains("content_blocked")) {
        return true;
    }
    message.contains("content policy") || message.contains("内容审核")
}

/// 决定**是否启用降级重试**（换中性提示词自救一次）的唯一判据。
///
/// 上游对两类失败回**完全相同**的报错（400 + `11-128` + `unapproved channel`）：
///
/// | 成因 | 触发条件 | 换提示词是否有用 |
/// |:--|:--|:--|
/// | 渠道校验 | 请求头族不齐 / 渠道未认证 | 无用，只会白耗一次配额 |
/// | 内容前缀拦截 | system 偏移 0 命中黑名单字面串 | **有用** |
///
/// 两者靠 `message` 分不开（字面一致），所以用网关自己掌握的事实来分：
/// `sanitized` = 本轮出站改写**确实动过**黑名单指纹。动过却仍被判渠道校验
/// ⇒ 成因在请求体而非请求头 ⇒ 降级重试有意义（issue #67 方案 B2）。
///
/// 为什么不把 `11-128` 判定提前（方案 A）：那会让真正的渠道校验也进降级链，
/// 回退 [`is_content_blocked`] 已固化的正确判断，还会把一次误判放大成
/// 「当天所有 passthrough 请求都被换成中性提示词」。
pub fn should_degrade_retry(status: u16, message: &str, sanitized: bool) -> bool {
    is_content_blocked(status, message)
        || (status == 400 && sanitized && is_channel_invalid(message))
}

/// 账号池的用量回报接收方（流式路径专用）。
///
/// 它是在**同步**上下文（`Stream::poll_next`）里被调用的，因此只能用 `try_write`：
/// 拿不到写锁就跳过本次样本。宁可少记一个样本，也**不能阻塞流式响应**。
struct PoolUsageSink {
    pool: std::sync::Arc<tokio::sync::RwLock<crate::pool::Pool>>,
    uid: String,
    model: String,
}

impl crate::protocol::usage_tap::UsageSink for PoolUsageSink {
    fn finish(&self, usage: Option<crate::protocol::usage_tap::UsageReport>) {
        // ★ 与日志类 sink **相反**：只在拿到真实用量时记账本。
        //   写一个 0 成本样本会把账号误标成「免费」，比没有样本更糟。
        let Some(usage) = usage else {
            return;
        };
        let Ok(mut pool) = self.pool.try_write() else {
            return;
        };
        pool.record_ledger(
            &self.uid,
            &self.model,
            usage.credit,
            usage.total_tokens(),
            timeutil::now_ms(),
        );
    }
}

/// 为当前命中账号构造流式用量回报接收方；uid 为空时返回 `None`（无账号可归因）。
///
/// 流式与非流式两条路径**必须**都能回填账本——否则「账本择优」在客户端默认的
/// 流式模式下等于未启用（详见 `protocol::usage_tap` 的模块文档）。
pub fn pool_usage_sink(
    state: &GatewayState,
    uid: &str,
    model: &str,
) -> Option<std::sync::Arc<dyn crate::protocol::usage_tap::UsageSink>> {
    if uid.is_empty() {
        return None;
    }
    Some(std::sync::Arc::new(PoolUsageSink {
        pool: state.pool.clone(),
        uid: uid.to_string(),
        model: model.to_string(),
    }))
}

/// 按密钥的 token 记账 sink（issue #76）。
///
/// 与 [`PoolUsageSink`] 同一性质：**账本类** ⇒ `usage == None` 时**跳过**
/// （写一个 0 会污染「本周期已用」，让配额提前或延后触发，比不记更糟）。
///
/// ★ 只记 token 口径：请求数在 `relay` 入口（进入网关后）就记了 ——
///   两条路径记两件事，互不重叠（账本里两个口径也各自独立，见 `key_usage`）。
struct KeyUsageSink {
    usage: std::sync::Arc<crate::key_usage::KeyUsageStore>,
    key_id: String,
    period: crate::apikey::QuotaPeriod,
}

impl crate::protocol::usage_tap::UsageSink for KeyUsageSink {
    fn finish(&self, report: Option<crate::protocol::usage_tap::UsageReport>) {
        // ★ 账本类 sink：拿不到用量就**跳过**（不写 0 样本）。
        let Some(report) = report else { return };
        let tokens = report.total_tokens();
        if tokens <= 0 {
            return;
        }
        self.usage.record_tokens(
            &self.key_id,
            tokens as u64,
            self.period,
            crate::timeutil::now_ms(),
        );
    }
}

/// 流式路径的 sink 组合：**请求日志**（必写）+ **成本账本**（有真实用量才写）
/// + **按密钥用量**（issue #76；有真实用量才写）。
///
/// ## 为什么日志也走 sink（而不是在路由里就地写）
///
/// 流式响应的 `usage` 只在 SSE 末帧才到达，而日志条目此前在响应返回前就落盘 ⇒
/// token 只能写死 0（2026-10-14 实测本机 200/200 条全 0）。⇒ 日志改挂到**流结束回调**
/// （[`crate::logging::StreamingLogSink`]），与账本共用同一个结束点。
///
/// 三条 WB 路由（`chat` / `messages` / `responses`）的流式分支**必须都走这里** ——
/// 各自拼一份就会出现「修了两条、漏了一条」。
///
/// `meta` 的 token / 耗时字段是**占位**，由 sink 在流结束时覆盖。
pub fn stream_sinks(
    state: &GatewayState,
    meta: crate::logging::RequestMeta,
    started: std::time::Instant,
    uid: &str,
    model: &str,
    // ★ issue #76：调用方的 `KeyActor`（`None` = 无 Key 的调用 ⇒ 不记按密钥用量）。
    //   用 actor 而不是具体 Key 类型：统一网关的 Key 是第四套台账，也能带进来。
    key: Option<&crate::apikey::KeyActor>,
) -> Vec<std::sync::Arc<dyn crate::protocol::usage_tap::UsageSink>> {
    let mut sinks: Vec<std::sync::Arc<dyn crate::protocol::usage_tap::UsageSink>> =
        vec![std::sync::Arc::new(crate::logging::StreamingLogSink::new(
            state.log.clone(),
            meta,
            started,
        ))];
    if let Some(ledger) = pool_usage_sink(state, uid, model) {
        sinks.push(ledger);
    }
    if let Some(actor) = key {
        sinks.push(std::sync::Arc::new(KeyUsageSink {
            usage: actor.usage.clone(),
            key_id: actor.key_id.clone(),
            period: actor.policy.quota_period,
        }));
    }
    sinks
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn realm_mapping_covers_both_regions() {
        assert_eq!(realm_of(Region::Cn), RealmTag::Cn);
        assert_eq!(realm_of(Region::Global), RealmTag::Global);
    }

    /// ★ 池快照必须能区分「一个号都没有」与「有号但全不可用」。
    ///
    /// 这两者的可行动指引相反（去登录 / 别去登录），所以判据本身要能被测住。
    /// 阳性对照：把两个分支合并成同一个返回值 ⇒ 本用例必红。
    #[test]
    fn no_credential_cause_splits_empty_store_from_exhausted_pool() {
        assert_eq!(
            no_credential_cause_of(&PoolSummary::default()),
            NoCredentialCause::StoreEmpty,
            "池内该区域 0 条 ⇒ 「账号库为空」这一档"
        );

        let exhausted = PoolSummary {
            total: 5,
            available: 0,
            cooling: 2,
            disabled: 3,
            ..PoolSummary::default()
        };
        assert_eq!(
            no_credential_cause_of(&exhausted),
            NoCredentialCause::PoolExhausted {
                total: 5,
                disabled: 3,
                cooling: 2,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
            "有号但全不可用 ⇒ 必须带上分类计数，不能退化成「未登录」"
        );
    }

    /// 账号全健康、但在途全占 ⇒ **503 `pool_busy`**，而不是 401。
    ///
    /// 阳性对照：去掉 `pool_unavailable_error` 里的 `PoolBusy` 分支 ⇒ 第一条断言必红。
    #[test]
    fn busy_pool_maps_to_503_not_401() {
        // ① 纯在途占满：凭据/余额/冷却全正常。
        let busy = PoolSummary {
            total: 1,
            available: 1,
            in_flight_full: 1,
            ..PoolSummary::default()
        };
        let error = pool_unavailable_error(Region::Global, &busy, None);
        assert_eq!(
            error.status(),
            503,
            "并发占满不是认证问题，回 401 会把客户端引去重新登录"
        );
        assert_eq!(error.type_name(), "pool_busy");
        assert!(
            error.message().contains("在途"),
            "文案必须点名在途额度，否则又变成「三个 0 却说 0 个可用」: {}",
            error.message()
        );

        // ② 混着禁用 ⇒ 仍归 401，但必须把「在途已满」列进计数（否则计数自相矛盾）。
        let mixed = PoolSummary {
            total: 2,
            available: 1,
            disabled: 1,
            in_flight_full: 1,
            ..PoolSummary::default()
        };
        let error = pool_unavailable_error(Region::Cn, &mixed, None);
        assert_eq!(
            error.status(),
            401,
            "有号被禁用时「去账号池看原因」才是有效指引"
        );
        assert!(
            error.message().contains("1 个在途已满"),
            "混合情形也要说出在途，否则计数仍然对不上: {}",
            error.message()
        );

        // ③ 排除 + 在途占满：排除不是「不健康」，剩下的号确实只是忙 ⇒ 仍是 503。
        let excluded_and_busy = PoolSummary {
            total: 2,
            available: 1,
            excluded: 1,
            in_flight_full: 1,
            ..PoolSummary::default()
        };
        assert_eq!(
            pool_unavailable_error(Region::Cn, &excluded_and_busy, None).status(),
            503,
            "排除项不该把「只是忙」降级成「未登录」"
        );

        // ④ 一个号都没有 ⇒ 绝对不能说「占满」，那是两码事。
        assert_eq!(
            pool_unavailable_error(Region::Cn, &PoolSummary::default(), None).status(),
            401,
            "空池必须仍然是 no_credential / StoreEmpty"
        );
    }

    /// issue #76：白名单生效 ⇒ 报**名单口径**的 `ScopeEmpty`，不是全池的 `PoolExhausted`。
    ///
    /// 阳性对照：去掉 `pool_unavailable_error` 里的 `scoped` 分支（退回全池口径）⇒
    /// 第一条断言必红。这条正是「白名单用户被告诉『12 个号都不可用』」的根。
    #[test]
    fn scoped_pool_reports_scope_empty_with_allowlist_counts() {
        // 全池 12 个号：10 个禁用 + 名单允许的那 2 个此刻都在冷却。
        let pool_wide = PoolSummary {
            total: 12,
            disabled: 10,
            cooling: 2,
            ..PoolSummary::default()
        };
        // 名单口径：只有那 2 个，全冷却。
        let scoped = PoolSummary {
            total: 2,
            cooling: 2,
            ..PoolSummary::default()
        };
        let error = pool_unavailable_error(Region::Cn, &pool_wide, Some(&scoped));
        match &error {
            GatewayError::NoCredential { cause, .. } => assert_eq!(
                *cause,
                NoCredentialCause::ScopeEmpty {
                    pool_total: 12,
                    allowed: 2,
                    disabled: 0,
                    cooling: 2,
                    zero_credits: 0,
                    excluded: 0,
                    in_flight_full: 0,
                    model_cooling: 0,
                },
                "白名单生效时必须报名单口径，不能把全池 12 个号摆给用户"
            ),
            other => panic!("应为 no_credential，实际 {other:?}"),
        }
        // 状态码契约不变：仍是 401（`type_name` 恒 `no_credential`）。
        assert_eq!(error.status(), 401);

        // 名单覆盖 0 个（名单里的 uid 都不在本区域池）⇒ 同样 ScopeEmpty，allowed = 0。
        let error = pool_unavailable_error(Region::Cn, &pool_wide, Some(&PoolSummary::default()));
        assert!(
            matches!(
                error,
                GatewayError::NoCredential {
                    cause: NoCredentialCause::ScopeEmpty { allowed: 0, .. },
                    ..
                }
            ),
            "名单没覆盖本区域时要明说，而不是报「账号库为空」"
        );

        // 名单内全在途满 ⇒ 503 `pool_busy`（判定同样按名单口径，不是全池）。
        let busy_scoped = PoolSummary {
            total: 2,
            available: 2,
            in_flight_full: 2,
            ..PoolSummary::default()
        };
        assert_eq!(
            pool_unavailable_error(Region::Cn, &pool_wide, Some(&busy_scoped)).status(),
            503,
            "名单内只是并发占满 ⇒ 让客户端退避，别去改登录态"
        );
    }

    #[test]
    fn usage_extraction_handles_missing_and_partial_fields() {
        assert_eq!(usage_of(None), (0.0, 0));
        assert_eq!(usage_of(Some(&json!({}))), (0.0, 0));

        let usage = json!({"credit": 1.5, "prompt_tokens": 10, "completion_tokens": 5});
        assert_eq!(usage_of(Some(&usage)), (1.5, 15));

        // 只有 token 没有 credit
        let partial = json!({"prompt_tokens": 7, "completion_tokens": 3});
        assert_eq!(usage_of(Some(&partial)), (0.0, 10));

        // 负数被钳制，避免污染账本
        let negative = json!({"prompt_tokens": -5, "completion_tokens": 2});
        assert_eq!(usage_of(Some(&negative)), (0.0, 2));
    }

    #[test]
    fn content_blocked_detection_is_narrow() {
        assert!(is_content_blocked(400, "code=11128 blocked"));
        assert!(is_content_blocked(400, "content_blocked"));
        assert!(is_content_blocked(200, "内容审核未通过"));
        assert!(!is_content_blocked(400, "invalid request"));
        assert!(!is_content_blocked(429, "rate limited"));
        assert!(!is_content_blocked(500, "boom"));

        // 渠道校验拦截（同为 11128）不得被判为内容拦截
        assert!(!is_content_blocked(
            400,
            r#"{"code":11128,"msg":"conversation id missing"}"#
        ));
        assert!(!is_content_blocked(
            400,
            r#"{"code":11128,"displayTips":{"en":"This call failed channel verification"}}"#
        ));
        assert!(!is_content_blocked(
            400,
            r#"{"code":11128,"displayTips":{"zh":"这次调用没通过渠道校验，可能不是官方客户端发出的。"}}"#
        ));
        assert!(!is_content_blocked(400, "unapproved channel"));
        assert!(is_channel_invalid("conversation id missing"));
        assert!(is_channel_invalid("这次调用没通过渠道校验"));
    }

    /// issue #67 方案 B2：降级门的**成因判定**。
    ///
    /// 两条边界都必须守住：
    /// - 没改写过指纹的渠道校验 ⇒ **不**降级（既有正确行为，不许回退）；
    /// - 改写过指纹却仍被判渠道校验 ⇒ 成因在请求体 ⇒ **才**降级（本案新增）。
    ///
    /// 可证伪：删掉 `sanitized &&`（等价 issue §4.2 方案 A）⇒ 第 1 条断言红。
    #[test]
    fn degrade_retry_needs_evidence_from_the_body() {
        let channel =
            r#"{"code":11-128,"msg":"Illegal API invocation from an unapproved channel"}"#;
        assert!(
            !should_degrade_retry(400, channel, false),
            "纯渠道校验不得进降级链"
        );
        assert!(
            should_degrade_retry(400, channel, true),
            "改写过指纹却仍被拦 ⇒ 成因在请求体，降级重试才有意义"
        );

        // 非 400 一律不降级（不靠文案漂移兜底）。
        assert!(!should_degrade_retry(429, channel, true));
        assert!(!should_degrade_retry(500, channel, true));

        // 既有内容拦截判定逐条不变。
        assert!(should_degrade_retry(400, "code=11128 blocked", false));
        assert!(should_degrade_retry(200, "内容审核未通过", false));
        assert!(!should_degrade_retry(400, "invalid request", true));
    }

    #[test]
    fn outbound_meta_carries_uid_and_conversation() {
        let meta = outbound_meta("u1", Some("c1"));
        assert_eq!(meta.uid, "u1");
        assert_eq!(meta.conversation_id.as_deref(), Some("c1"));
        assert!(outbound_meta("u1", None).conversation_id.is_none());
    }

    // -----------------------------------------------------------------------
    // 会话粘性（B8 接线）
    // -----------------------------------------------------------------------

    fn relay_request(conversation_id: Option<&str>, model: &str, region: Region) -> RelayRequest {
        RelayRequest {
            region,
            // 既有测试全部针对 **LLM** 路径 ⇒ 行为必须与加 `kind` 之前**逐字相同**。
            kind: RelayKind::Llm,
            model: model.to_string(),
            prepared_body: String::new(),
            sanitized: false,
            conversation_request_id: "turn-1".to_string(),
            conversation_id: conversation_id.map(str::to_string),
            trace_id: None,
            // ★ issue #76：既有粘性测试都不带 Key ⇒ `None`（不判配额、不记账、不过滤）。
            key_policy: None,
            key_id: None,
        }
    }

    /// ★ **不破坏既有使用**：客户端没给会话 id（或只给空白）⇒ 粘性整段跳过。
    ///
    /// 这条最重要：绝大多数既有调用点不带 `metadata.conversation_id`，
    /// 它们的行为必须与接线前**逐字相同**（不查表、不绑定）。
    #[test]
    fn sticky_key_is_none_without_a_conversation_id() {
        assert_eq!(sticky_key_of(&relay_request(None, "m", Region::Cn)), None);
        assert_eq!(
            sticky_key_of(&relay_request(Some(""), "m", Region::Cn)),
            None,
            "空串不得当作有效会话 id"
        );
        assert_eq!(
            sticky_key_of(&relay_request(Some("   "), "m", Region::Cn)),
            None,
            "纯空白不得当作有效会话 id（前后空白也要 trim）"
        );
    }

    /// 有会话 id ⇒ 键含**区域与模型**。
    ///
    /// 模型必须进键：同一会话切模型时应允许落到各自最合适的账号，
    /// 否则「模型级限流只封锁该模型」的优势会被粘性抵消。
    #[test]
    fn sticky_key_carries_region_and_model() {
        let cn = sticky_key_of(&relay_request(Some("conv-1"), "hy4", Region::Cn)).unwrap();
        assert_eq!(cn, "cn|hy4|conv-1");
        assert_eq!(
            sticky_key_of(&relay_request(Some("conv-1"), "hy4", Region::Global)).unwrap(),
            "global|hy4|conv-1",
            "区域必须进键（CN/Global 账号库本就分家）"
        );
        assert_ne!(
            cn,
            sticky_key_of(&relay_request(Some("conv-1"), "other-model", Region::Cn)).unwrap(),
            "模型必须进键"
        );
        assert_eq!(
            sticky_key_of(&relay_request(Some("  conv-1  "), "hy4", Region::Cn)).unwrap(),
            "cn|hy4|conv-1",
            "会话 id 前后空白要 trim（否则同一会话会派生出两个键）"
        );
    }

    // ⚠️ 这里原本有两个用例守 `sticky_choice`（「粘性绝不导致失败」与「只探询被选中的
    // uid」）。该函数已**删除** —— 偏好判定统一交给 `pool::pick_with_preference`
    // （判据与自动选号同源，不再有第二套）。对应性质现在由 `pool/pick.rs` 的
    // `pick_with_preference` 系列用例与 `pool::tests::pick_account_honours_preferred_uid_and_falls_back`
    // 守住；Trae 侧另有 `trae::pool::preferred_uid_falls_back_when_unusable` 做行为级印证。

    /// ★ issue #100：上游 401/403 必须**换号**。
    ///
    /// 旧实现把它归到 `UpstreamEvent::Client`（「调用方错误，不换号」）⇒ 池里明明有
    /// 健康账号，客户端却直接吃到 401。用户原话：「之前旧的登录状态直接报 401」。
    ///
    /// 变异：把 `should_rotate` 里的 `AuthRejected` 挪回「不换号」⇒ 第一条断言红。
    #[test]
    fn auth_rejected_rotates_to_another_account() {
        use crate::pool::UpstreamEvent;
        assert!(should_rotate(&UpstreamEvent::AuthRejected, 0));
        assert!(
            should_rotate(&UpstreamEvent::AuthRejected, 5),
            "401/403 不受 attempt 预算影响（它换号就能救）"
        );
        // 对照：参数错（400/422）仍然不换号 —— 换号只是把同一份坏参数喂给下一个号。
        assert!(!should_rotate(&UpstreamEvent::Client, 0));
    }
}

/// **首帧窥探**的集成测试：起一个只应答一次的**真 HTTP 服务**，让 `peek_first_frame`
/// 面对真实的 `reqwest::Response`（这个类型没有公开构造入口，只能这么测）。
///
/// 服务端用 `std::net` + 阻塞线程实现：`tokio` 这边只开了 `net` 特性（没开 `io-util`），
/// 为了一个测试去加特性会把依赖面扩大；而「写死一段 HTTP 响应」正是阻塞 IO 最擅长的。
#[cfg(test)]
mod first_frame_tests {
    use super::*;
    use futures_util::StreamExt;

    /// 起一个只应答一次的 HTTP 服务，返回它的 URL。
    fn spawn_upstream(body: &'static str, content_type: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("local_addr");
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            // 读掉请求头就够（我们只关心响应方向的字节）。
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: {content_type}\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes());
            let _ = socket.flush();
        });
        format!("http://{addr}/")
    }

    async fn get(url: String) -> reqwest::Response {
        reqwest::Client::new().get(url).send().await.expect("send")
    }

    /// ★ 首帧即错误信封 ⇒ 必须判成失败，且**不**交出 prefix（调用方会丢弃该响应换号）。
    #[tokio::test]
    async fn first_frame_envelope_is_reported_as_error() {
        let url = spawn_upstream(
            "data: {\"code\":6004,\"msg\":\"rate limited\"}\n\n",
            "text/event-stream",
        );
        let response = get(url).await;
        let (_response, prefix, error) = peek_first_frame(response).await;

        let error = error.expect("首帧即错误必须被识别");
        assert_eq!(error.status, 429, "6004 ⇒ 429（模型级限流）");
        assert!(prefix.is_none(), "判定为错误时不得交出 prefix");
    }

    /// ★ 正常首帧 ⇒ 识别为「不是错误」，且 `prefix + 剩余流` 必须**逐字节还原**正文。
    ///
    /// 变异：让 `peek_first_frame` 丢掉 `prefix`（`(response, None, None)`）⇒ 本用例红
    /// —— 那正是「窥探一眼」最容易引入的破坏：客户端少收到一段字节。
    #[tokio::test]
    async fn normal_first_frame_is_preserved_verbatim() {
        const BODY: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n\
                            data: [DONE]\n\n";
        let url = spawn_upstream(BODY, "text/event-stream");
        let response = get(url).await;
        let (response, prefix, error) = peek_first_frame(response).await;
        assert!(error.is_none(), "正常帧不得被判成错误");

        let mut collected: Vec<u8> = Vec::new();
        if let Some(prefix) = prefix {
            collected.extend_from_slice(&prefix);
        }
        let mut stream = Box::pin(prefix_stream(None, response));
        while let Some(chunk) = stream.next().await {
            collected.extend_from_slice(&chunk.expect("chunk"));
        }
        assert_eq!(
            String::from_utf8_lossy(&collected),
            BODY,
            "peek 之后必须逐字节还原（多一个或少一个都是客户端可见的破坏）"
        );
    }

    /// 非流式 JSON 补全（有 `choices`）不得被判成错误 —— 误判会把好账号冷却掉。
    #[tokio::test]
    async fn json_completion_is_not_an_error() {
        const BODY: &str = r#"{"id":"x","choices":[{"index":0,"message":{"content":"ok"}}]}"#;
        let url = spawn_upstream(BODY, "application/json");
        let response = get(url).await;
        let (_response, _prefix, error) = peek_first_frame(response).await;
        assert!(error.is_none(), "有 choices 的 200 必定是成功");
    }

    /// 非流式路径的正文拼接：`body_text_with_prefix` 必须还原完整 JSON，
    /// 否则聚合器会把一条好好的补全解析成空。
    #[tokio::test]
    async fn body_text_with_prefix_reconstructs_the_payload() {
        const BODY: &str = r#"{"id":"y","choices":[{"index":0,"message":{"content":"hello"}}]}"#;
        let url = spawn_upstream(BODY, "application/json");
        let response = get(url).await;
        let (response, prefix, error) = peek_first_frame(response).await;
        assert!(error.is_none());

        let text = body_text_with_prefix(prefix, response).await;
        assert_eq!(text, BODY, "拼回来的正文必须与上游一字不差");
        assert!(text.contains("\"hello\""));
    }
}
