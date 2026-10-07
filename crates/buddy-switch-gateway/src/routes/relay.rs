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

use serde_json::Value;

use buddy_switch_core::modules::account;
use buddy_switch_core::modules::region::Region;
use buddy_switch_core::modules::upstream::UpstreamChatResult;

use crate::account_strategy::{AccountSelector, AccountStrategy};
use crate::error::GatewayError;
use crate::outbound::{self, DegradeGate, OutboundMeta};
use crate::pool::{classify_event, RealmTag, UpstreamEvent};
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
    Biz { path: String },
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
    /// 会话轮主键（轮内复用）。
    pub conversation_request_id: String,
    /// 客户端会话 id。
    pub conversation_id: Option<String>,
    /// 调用方 trace id。
    pub trace_id: Option<String>,
}

/// 中继结果。
pub enum RelayOutcome {
    /// 上游 2xx，交回原始响应（由调用方决定透传/聚合）。
    Ok {
        /// 上游响应。
        response: reqwest::Response,
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
    /// 上游返回体片段（可能为空）。
    pub upstream_message: String,
}

/// 把一次请求送到上游；内含换号轮换与失败治理。
pub async fn relay(
    state: &GatewayState,
    request: RelayRequest,
) -> Result<RelayOutcome, RelayFailure> {
    let now_ms = timeutil::now_ms();
    let realm = realm_of(request.region);
    let config = state.config_snapshot().await;
    let tries = config.max_rotate.max(1);

    // 与账号库对齐（增量 upsert，不删除既有账号以保留治理历史）。
    {
        let accounts = account::load_accounts_for(request.region);
        let mut pool = state.pool.write().await;
        pool.sync_accounts(&accounts, Some(realm));
    }

    // 会话上下文：轮内所有重试复用同一实例。
    let context = ConversationContext {
        conversation_id: request.conversation_id.clone(),
        conversation_request_id: request.conversation_request_id.clone(),
        trace_id: request.trace_id.clone(),
    };
    let message_id = session_headers::new_hex_id();

    let mut tried: HashSet<String> = HashSet::new();
    let mut last: Option<RelayFailure> = None;

    // 会话粘性键；客户端没给会话 id 时为 `None`（整段跳过粘性，保持原行为）。
    let sticky_key = sticky_key_of(&request);

    for attempt in 0..tries {
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
        let selected = select_account(
            state,
            request.region,
            &request.model,
            realm,
            &tried,
            now_ms,
            attempt,
            preferred_uid.as_deref(),
        )
        .await;

        let Some(account_value) = selected else {
            // 池与策略都给不出账号：若已有失败记录，返回它（更有诊断价值）。
            break;
        };

        let uid = account::get_str(&account_value, "uid").unwrap_or_default();
        let account_name = account::account_display_name(&account_value);

        // 在途租约：占满则标记为已试并换号。
        if !uid.is_empty() {
            let acquired = state.pool.write().await.acquire(&uid);
            if !acquired {
                tried.insert(uid);
                continue;
            }
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
            RelayKind::Biz { path } => {
                state
                    .upstream
                    .forward_biz_with(
                        request.region,
                        &account_value,
                        path,
                        &request.prepared_body,
                        None,
                    )
                    .await
            }
        };

        // 无论成败都释放在途（本次偏差见模块文档）。
        if !uid.is_empty() {
            state.pool.write().await.release(&uid);
        }

        match result {
            UpstreamChatResult::Ok(ok) => {
                if !uid.is_empty() {
                    // 成功：清熔断域与退避指数。此处 usage 尚未到达，故不写账本——
                    // 账本由调用方在拿到 usage 后经 [`record_ledger`] 回填。
                    state
                        .pool
                        .write()
                        .await
                        .note_success(&uid, &request.model, 0.0, 0, now_ms);
                    // 会话粘性：**只在成功之后**绑定，让下一轮优先复用这个账号。
                    if let Some(key) = sticky_key.as_deref() {
                        state.sticky.write().await.bind(key, &uid, now_ms);
                    }
                }
                return Ok(RelayOutcome::Ok {
                    response: ok.response,
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
                let disabled = if uid.is_empty() {
                    false
                } else {
                    let mut pool = state.pool.write().await;
                    let disabled = pool.apply_upstream_error(&uid, &request.model, &event, now_ms);
                    disabled
                };
                if !uid.is_empty() {
                    tried.insert(uid);
                    // 会话粘性：这个账号刚失败 ⇒ 立刻**解绑**，下一轮换号重新绑。
                    // 粘性是优化而非约束，绝不能因为粘性而反复撞同一堵墙。
                    if let Some(key) = sticky_key.as_deref() {
                        state.sticky.write().await.unbind(key);
                    }
                }

                last = Some(RelayFailure {
                    error: GatewayError::Upstream {
                        kind,
                        status,
                        message: message.clone(),
                    },
                    account: account_name,
                    upstream_message: message,
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
            }
        }
    }

    let failure = last.unwrap_or(RelayFailure {
        error: GatewayError::NoCredential {
            region: request.region,
        },
        account: String::new(),
        upstream_message: String::new(),
    });
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

async fn select_account(
    state: &GatewayState,
    region: Region,
    model: &str,
    realm: RealmTag,
    tried: &HashSet<String>,
    now_ms: i64,
    attempt: usize,
    sticky_uid: Option<&str>,
) -> Option<Value> {
    // 偏好优先级：**显式固定（Pinned）> 会话粘性**。
    //
    // ★ `Pinned` 必须在这里参与池选号，而不是只作「池给不出时的回落」：池只要有候选
    // 就会出结果，若把 `Pinned` 放在池之后，它在**池非空时永远不会被用到** ——
    // 界面明明写着「固定账号」，实际请求却走池选号，UI 与行为不符且**不报错**。
    //
    // 粘性同理：它此前靠 `sticky_choice` 单独判一次可用性再直接返回，现在统一交给
    // `pick_with_preference`（判据与自动选号同源），两条偏好实现因此收敛成一条。
    let strategy = state.strategy_for(region).await;
    let preferred = match &strategy {
        AccountStrategy::Pinned { account_id } if !account_id.is_empty() => {
            Some(account_id.as_str())
        }
        _ => sticky_uid,
    };

    let picked_uid = {
        let mut pool = state.pool.write().await;
        if pool.is_empty() {
            None
        } else {
            let seed = (now_ms as u64)
                .wrapping_mul(31)
                .wrapping_add(attempt as u64)
                .wrapping_add(0x9E37_79B9_7F4A_7C15);
            // 偏好不可用时由 `pick_with_preference` 自行回落到自动择优。
            pool.pick_account(now_ms, Some(realm), model, tried, seed, preferred)
        }
    };

    if let Some(uid) = picked_uid {
        // 池里存在但账号库已删除 → 该 uid 无凭据可用，落回策略选择。
        if let Some(account_value) = account::find_account_for(region, &uid) {
            return Some(account_value);
        }
    }

    // 回落：策略模块（current / pinned / max_credits）。
    //
    // `strategy` 已在函数开头读过（`Pinned` 也已在池选号里作为偏好生效）。这里保留
    // 完整回落，是为了覆盖「**池为空**」这条路径 —— 那时 `Pinned` 仍需生效。
    let selector = AccountSelector;
    match selector.select(region, &strategy).await {
        Ok(account_value) => {
            let uid = account::get_str(&account_value, "uid").unwrap_or_default();
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
pub async fn record_ledger(state: &GatewayState, uid: &str, model: &str, credit: f64, tokens: i64) {
    if uid.is_empty() {
        return;
    }
    let now_ms = timeutil::now_ms();
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
) -> (String, String) {
    let now_ms = timeutil::now_ms();
    let degraded = state.degrade_active(now_ms).await;
    let options = state.outbound_options().await;
    let meta = outbound_meta(uid_hint, conversation_id);

    let prepared = outbound::prepare_outbound_body(region, raw_body, &options, &meta, degraded);

    let request_id =
        session_headers::resolve_conversation_request_id(inbound_request_id, None, turn_key);
    (prepared, request_id)
}

/// 触发降级期（内容拦截时调用）。
pub async fn trip_degrade(state: &GatewayState) -> bool {
    state.trip_degrade(timeutil::now_ms()).await
}

/// 当前是否处于降级期。
pub async fn degrade_gate(state: &GatewayState) -> DegradeGate {
    state.degrade.read().await.clone()
}

/// 判断是否为「内容拦截」类失败（用于决定是否启用降级重试）。
pub fn is_content_blocked(status: u16, message: &str) -> bool {
    if status == 400 && (message.contains("11128") || message.contains("content_blocked")) {
        return true;
    }
    message.contains("content policy") || message.contains("内容审核")
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
    fn record(&self, credit: f64, tokens: i64) {
        let Ok(mut pool) = self.pool.try_write() else {
            return;
        };
        pool.record_ledger(&self.uid, &self.model, credit, tokens, timeutil::now_ms());
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn realm_mapping_covers_both_regions() {
        assert_eq!(realm_of(Region::Cn), RealmTag::Cn);
        assert_eq!(realm_of(Region::Global), RealmTag::Global);
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
            conversation_request_id: "turn-1".to_string(),
            conversation_id: conversation_id.map(str::to_string),
            trace_id: None,
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
}
