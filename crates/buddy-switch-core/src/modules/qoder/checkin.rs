//! Qoder **签到**：活动平台为主、旧 `/sash/daily-check-in` 通道兜底。
//!
//! ## 通道选择（★ 改这里之前先读）
//!
//! | 通道 | 端点 | 现状 |
//! |:--|:--|:--|
//! | **活动平台**（主力） | `GET /sash/api/v1/me/campaigns` + `POST …/{id}/claim` | 官方现行机制，「每日领取 100 Credits」在这里 |
//! | 旧签到通道（兜底） | `/sash/api/v1/me/daily-check-in/*` | 实测**国内版 `DISABLED`、国际版 404**，只在仍开放时可用 |
//!
//! ⇒ 顺序固定：**先活动平台**；只有它**整区不可用**（404/405/410）时才回落旧通道。
//! 旧通道返回 `DISABLED` 也**不**再回退（那是「活动关着」，不是「通道不存在」）。
//!
//! ## 三档语义（**不许回退**，issue #46-D / #58 §6）
//!
//! | 档 | 判据 | 出口 |
//! |:--|:--|:--|
//! | 成功 | 真领到了 / 今日已领（上游 `CLAIMED`） | `ok:true` |
//! | **未开放** | 列表里**根本没有**积分类活动；或整区没有活动平台 | `ok:false` + `disabled:true` |
//! | 失败 | 上游报错 / 网络失败 | `ok:false` |
//!
//! 「未开放」写成 `ok:true` 会让界面显示「N 个成功」而额度纹丝不动 —— 那正是
//! `de932d7` 修掉的缺陷，也是 [`disabled_claim_response`] 存在的唯一理由。
//!
//! ## 「今日已签」怎么判
//!
//! - 活动平台：**上游 `claimStatus` 为准**（任一积分类活动 `CLAIMED` ⇒ 今日已签）；
//!   本地盖章（`account::last_checkin`）只作**展示提示**，不作判据。
//! - 旧通道：上游 `lastClaimedAt`（该通道自带，无需本地状态）。
//!
//! ## 排程（`ScheduleTask::QoderCheckin`）
//!
//! ⚠️ **上游每日实例在 10:00（UTC+8）切换**（窗口 10:00 → 次日 09:59）。由此定下三件事，
//! 改动其中任何一条前先读完：
//!
//! 1. 默认时点是 **10:05 / 21:00**，不是整点 09:00 —— 09:00 那一轮稳定地签在上一个
//!    实例上（上游回 `CLAIMED`），用户看到的就是「天天自动签到、天天没领到」。
//! 2. 老配置（已落盘 09:00）由**窗口感知补跑**兜住：`retry_after_ms` 判出「本轮只是
//!    重复签旧实例」后给出下一个 10:05，`scheduler::schedule_loop` 到点补跑一次。
//! 3. 每轮都写台账（`qoder/checkin_logs.<region>.json`）+ 一条运行日志 —— 后台是静默的，
//!    没有它，「跑了但无事可做 / 失败 / 压根没跑」三态在界面上无法区分。
//!
//! [`run_cycle_for`] 逐账号**顺序**调用 [`claim_for`]（并发会踩风控）；
//! 签到默认**关**（替用户去领东西，opt-in），保活默认**开**（维持用户交出来的
//! `refresh_token`）。本地盖章只用于展示与去重提示，判据仍是上游 `claimStatus` ——
//! 重复领取由上游幂等（`CLAIMED` ⇒ `already`）兜住，不依赖本地状态。

use serde_json::{json, Value};

use super::account::{self, RawAccount};
use super::region::QoderRegion;
use super::{api, campaign, paths, store};
use crate::modules::{config, runtime_log};

/// 自动签到台账保留条数（每轮一条）。
///
/// 只留最近 30 轮：用户要看的是「最近到底跑没跑 / 为什么没领到」，
/// 不是归档。无界增长会把一个诊断文件变成第二个日志系统。
const LOG_KEEP: usize = 30;

/// 每日刷新（10:00 UTC+8）之后的补跑宽限（秒）。
///
/// 上游「每天领 100 Credits」的实例窗口是 **10:00 → 次日 09:59**。排程定在 09:00
/// 时签到的必然是**上一个实例**（上游回 `CLAIMED` ⇒ `already`），什么也领不到 ——
/// 这是「自动签到天天跑、天天没有」的真正原因。因此判出「本轮只是重复签旧实例」
/// 后，要等新实例出来再跑一轮；留 5 分钟余量，避开上游切换的那一秒。
const RETRY_AFTER_RESET_SECS: i64 = 300;

/// 领取决策（纯函数，可测）：由状态推出下一步。
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimDecision {
    /// 今日已签 ⇒ 幂等成功。
    Already,
    /// 活动未开放（DISABLED 等）⇒ 成功跳过并给出上游状态词。
    Disabled(String),
    /// 可以领取。
    ClaimNow,
}

/// 由签到状态推出领取决策（见模块头）。
pub fn claim_decision(status: &api::CheckinStatus) -> ClaimDecision {
    if status.today_checked_in {
        return ClaimDecision::Already;
    }
    if !status.active {
        return ClaimDecision::Disabled(status.status.clone());
    }
    ClaimDecision::ClaimNow
}

/// 「**今日已签**」这一档的领取响应体（纯函数，可测）。
///
/// ★ 与 [`disabled_claim_response`] 成对：两个出口的 `ok` 值必须**明确相反**且各有测试，
///   否则「把未开放误报成成功」这种回归没有任何测试能接住（`claim_for` 本身要账号库，
///   测不了；出口提成纯函数才测得到）。
/// `Already` 是**真的成功**（`today_checked_in` 有上游依据），必须继续算成功。
pub fn already_claim_response(uid: &str, status: &api::CheckinStatus) -> Value {
    json!({
        "ok": true,
        "already": true,
        "uid": uid,
        "streakDays": status.streak_days,
        "rewardCredits": status.reward_credits,
    })
}

/// 「**通道未开放**」这一档的领取响应体（纯函数，可测）。
///
/// ## ⚠️ 为什么 `ok` 必须是 `false`（issue #46 的 D 项）
///
/// Qoder 的 legacy 签到端点对**任何** campaignKey 都回 409 `AlreadyExists`
/// —— 编造的 `campaignKey: totally_bogus_key` 同样 409，而 `status` 端点同时报
/// `DISABLED` + `totalClaimDays=0` ⇒ **这个 409 不携带幂等语义**，
/// 「通道未开放」**不等于**「领到了」。
///
/// 旧实现回 `{"ok": true, "disabled": true}`，而前端按 `res.ok` 计数
/// ⇒ 点「一键签到」显示「N 个成功」，而 `totalClaimDays=0`、额度纹丝不动。
/// 用户于是反复觉得「签到不能用」。
///
/// ## 为什么不是折进「失败」
///
/// 「重试失败项」会把这些账号反复重试，而重试**永远不会成功**（端点已关停）——
/// 那是在教用户做一件没用的操作。故单独成第三档，由前端如实呈现。
///
/// `Already`（今日已签）**继续**算成功：那条有 `today_checked_in` 作依据。
pub fn disabled_claim_response(uid: &str, upstream_status: &str) -> Value {
    json!({
        "ok": false,
        "disabled": true,
        "uid": uid,
        "status": upstream_status,
    })
}

/// 一条账号的凭据是否够发起 openapi 调用（需要 `access`；PAT 不适用签到端点的 Bearer 头）。
fn access_token_of(account: &RawAccount) -> Option<&str> {
    account
        .access_token
        .as_deref()
        .filter(|token| !token.is_empty())
}

// ---------------------------------------------------------------------------
// 活动平台 → 签到状态（纯函数，可测）
// ---------------------------------------------------------------------------

/// 一次活动快照里与「签到」有关的聚合结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CampaignSummary {
    /// 列表里**有没有**积分类活动 —— 这一条决定「未开放」这一档（issue #58 §6）。
    pub has_benefit: bool,
    /// 有活动已被领取（上游 `CLAIMED`）。
    pub claimed: bool,
    /// 有活动现在就能领。
    pub claimable: bool,
    /// 现在能领的金额合计（= 界面上的「今日可得」）。
    pub claimable_amount: i64,
    /// 已领金额合计（只有 `claimed` 时才有意义）。
    pub claimed_amount: i64,
}

/// 把活动列表聚合成签到口径（**纯函数**）。
///
/// 只统计「积分类」活动（`CREDITS` / 空 kind，见 `CampaignItem::is_credit_kind`）——
/// 兑换码 / 详情类活动不是签到，混进来会让「今日已签」变成假阳性。
pub fn summarize_campaigns(items: &[campaign::CampaignItem]) -> CampaignSummary {
    let mut summary = CampaignSummary::default();
    for item in items.iter().filter(|item| item.is_credit_kind()) {
        summary.has_benefit = true;
        match campaign::classify(item) {
            campaign::CampaignClass::Already => {
                summary.claimed = true;
                summary.claimed_amount += item.benefit_amount;
            }
            campaign::CampaignClass::Claimable => {
                summary.claimable = true;
                summary.claimable_amount += item.benefit_amount;
            }
            // 名额发完 / 被锁 / 详情类：都不是「今天领到了」，也不算「现在能领」。
            campaign::CampaignClass::Pending
            | campaign::CampaignClass::Locked
            | campaign::CampaignClass::View => {}
        }
    }
    summary
}

/// 把活动快照投影成 `CheckinStatusEntry`（**纯函数**；字段名与 WB 同构）。
///
/// - `todayCheckedIn` 只认上游 `claimStatus`（本地盖章只给 `stampedToday` 作提示）；
/// - `dailyCredit` = 现在能领的合计，没有可领时退回已领合计（否则「已签」会显示成 0）；
/// - `todayCredit` 只在**今天确实领到**时才有值（`0` 会被读成「签了但没给分」）。
fn campaign_entry(snapshot: &campaign::CampaignsSnapshot, stamped_today: bool) -> Value {
    let summary = summarize_campaigns(&snapshot.items);
    let daily = if summary.claimable_amount > 0 {
        summary.claimable_amount
    } else {
        summary.claimed_amount
    };
    let status = if summary.claimed {
        "CLAIMED"
    } else if summary.claimable {
        "CLAIMABLE"
    } else if summary.has_benefit {
        "UNAVAILABLE"
    } else {
        "NONE"
    };
    json!({
        "ok": true,
        "todayCheckedIn": summary.claimed,
        "dailyCredit": daily,
        "todayCredit": if summary.claimed { json!(summary.claimed_amount) } else { Value::Null },
        "totalCredits": summary.claimed_amount,
        // 活动平台没有「连续天数」这个概念：给 null 而不是编一个 0。
        "streakDays": Value::Null,
        "status": status,
        "active": summary.claimable || summary.claimed,
        // 身份来源（`runtime-info` / `derived`）与机器头状态 —— 国际版拿不到活动时，
        // 界面要能说清「是没装客户端」而不是「今天没有活动」。
        "source": "campaign",
        "identity": snapshot.identity,
        "machineHeaders": snapshot.machine_headers,
        "hint": snapshot.hint,
        "stampedToday": stamped_today,
    })
}

/// 区域签到状态视图：逐账号状态 + 汇总。`unavailable` 是**区域级**结论。
pub async fn status_for(region: QoderRegion) -> Value {
    let file = account::load_accounts_for(region);
    let mut rows = Vec::new();
    let mut unavailable = false;
    let mut unavailable_reason: Option<String> = None;

    for account in &file.accounts {
        let name = account::display_name(account);
        let Some(access) = access_token_of(account) else {
            rows.push(json!({
                "uid": account.uid,
                "name": name,
                "ok": false,
                "error": "没有可调用的 access token（PAT 不能用于签到端点）",
                "unavailable": false,
            }));
            continue;
        };
        // ── 活动平台优先 ──
        let stamped_today = account::checkin_stamp_day(account) == Some(store::today().as_str());
        let snapshot = campaign::fetch_campaigns(region, &account.uid, access).await;
        if snapshot.ok {
            rows.push(campaign_entry(&snapshot, stamped_today));
            continue;
        }
        if snapshot.available {
            // 账号级失败（网络 / 鉴权 / 响应格式）：如实报错，**不**回落旧通道 ——
            // 旧通道已关停，回落只会把一个真实错误变成一句「未开放」。
            rows.push(json!({
                "uid": account.uid,
                "name": name,
                "ok": false,
                "error": snapshot.error,
                "unavailable": false,
                "source": "campaign",
                "identity": snapshot.identity,
                "machineHeaders": snapshot.machine_headers,
                "hint": snapshot.hint,
            }));
            continue;
        }
        // ── 整区没有活动平台（404/405/410）⇒ 旧通道兜底 ──
        match api::checkin_status(region, &account.uid, access).await {
            Ok(status) => {
                rows.push(json!({
                    "uid": account.uid,
                    "name": name,
                    "ok": true,
                    "status": status.status,
                    "active": status.active,
                    "todayCheckedIn": status.today_checked_in,
                    "streakDays": status.streak_days,
                    "totalClaimDays": status.total_claim_days,
                    "rewardCredits": status.reward_credits,
                    "totalRewardCredits": status.total_reward_credits,
                    "nextClaimAt": status.next_claim_at,
                    "lastClaimedAt": status.last_claimed_at,
                    "unavailable": false,
                }));
            }
            Err(error) if error.starts_with(api::CHECKIN_REASON_NOT_FOUND) => {
                // 区域级结论：接口不存在 ⇒ 整区标记，逐账号不再重复报。
                unavailable = true;
                unavailable_reason.get_or_insert(error.clone());
                rows.push(json!({
                    "uid": account.uid,
                    "name": name,
                    "ok": false,
                    "error": error,
                    "unavailable": true,
                }));
            }
            Err(error) => {
                rows.push(json!({
                    "uid": account.uid,
                    "name": name,
                    "ok": false,
                    "error": error,
                    "unavailable": false,
                }));
            }
        }
    }

    let checked = rows
        .iter()
        .filter(|row| row["todayCheckedIn"] == Value::Bool(true))
        .count();
    json!({
        "region": region.as_str(),
        "unavailable": unavailable,
        "reason": unavailable_reason,
        "summary": {
            "total": file.accounts.len(),
            "checked": checked,
            "claimable": rows.iter().filter(|row| row["active"] == Value::Bool(true) && row["todayCheckedIn"] != Value::Bool(true)).count(),
        },
        "accounts": rows,
    })
}

/// 单账号签到：**活动平台优先**；整区没有活动平台时回落旧通道。
///
/// 返回值同时带**旧契约**（`ok` / `already` / `rewardCredits` / `disabled`）与
/// **活动明细**（`claimed` / `alreadyItems` / `blocked` / `pending` / `locked` / `earned` /
/// `message` / `nextAvailableAt`），前端按需取用 —— 加字段是兼容的，改字段名不是。
pub async fn claim_for(region: QoderRegion, uid: &str) -> Result<Value, String> {
    let campaign_result = campaign::checkin_for(region, uid).await?;
    // `available == false` 才是「整区没有活动平台」；账号级失败仍然是活动通道的结论。
    if campaign_result.get("available").and_then(Value::as_bool) != Some(false) {
        return Ok(normalize_campaign_claim(campaign_result));
    }
    legacy_claim_for(region, uid).await
}

/// Pro 升级包：**资格查询**（只读；`available=false` ⇒ 活动已下线/本区域没有）。
pub async fn pro_pack_status_for(region: QoderRegion, uid: &str) -> Result<Value, String> {
    let access = campaign::access_token_for(region, uid)?;
    Ok(campaign::pro_pack_status(region, uid, &access).await)
}

/// Pro 升级包：**领取**（幂等；已领过返回 `ok=true, already=true`）。
pub async fn pro_pack_claim_for(region: QoderRegion, uid: &str) -> Result<Value, String> {
    let access = campaign::access_token_for(region, uid)?;
    Ok(campaign::claim_pro_pack(region, uid, &access).await)
}

/// 把活动通道的响应补成「旧契约 + 新明细」。
///
/// 只补**兼容键**，不改上游给的事实：
/// - `already` = 有已领活动（布尔，与旧契约同名同义）；
/// - `rewardCredits` = `earned`（本次真领到的额度）；
/// - `claimedCount` = 本次领取条数。
fn normalize_campaign_claim(mut value: Value) -> Value {
    let already_count = value
        .get("alreadyItems")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let claimed_count = value
        .get("claimed")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let earned = value.get("earned").and_then(api::as_num).unwrap_or(0);
    if let Some(map) = value.as_object_mut() {
        map.insert("already".to_string(), json!(already_count > 0));
        map.insert("rewardCredits".to_string(), json!(earned));
        map.insert("claimedCount".to_string(), json!(claimed_count));
    }
    value
}

/// 旧 `/sash/daily-check-in` 通道的领取（**只在整区没有活动平台时**才走）。
async fn legacy_claim_for(region: QoderRegion, uid: &str) -> Result<Value, String> {
    let account =
        account::find_account_for(region, uid).ok_or_else(|| format!("账号不存在: {uid}"))?;
    let access = access_token_of(&account)
        .ok_or_else(|| "这条账号没有可调用的 access token（PAT 不能用于签到端点）".to_string())?;

    let status = api::checkin_status(region, uid, access).await?;
    match claim_decision(&status) {
        ClaimDecision::Already => Ok(already_claim_response(uid, &status)),
        ClaimDecision::Disabled(upstream_status) => {
            Ok(disabled_claim_response(uid, &upstream_status))
        }
        ClaimDecision::ClaimNow => {
            let result = api::checkin_claim(region, uid, access).await?;
            // 上游把失败放在 body 里（`success=false`）而不是 HTTP 状态。
            if result.get("success") == Some(&Value::Bool(false)) {
                // 复查一次：上游可能已记账（先扣后报错等形态）。
                if let Ok(recheck) = api::checkin_status(region, uid, access).await {
                    if recheck.today_checked_in {
                        return Ok(
                            json!({ "ok": true, "already": true, "uid": uid, "rechecked": true }),
                        );
                    }
                }
                let detail = result
                    .get("error")
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| result.to_string());
                let detail: String = detail.chars().take(160).collect();
                return Err(format!("签到失败: {detail}"));
            }
            // ⚠️ 走 `api::as_num`（同时接受浮点）：上游把额度写成 `100.0` 时 `as_i64` 返回
            //    None ⇒ 签到成功却报「+0」，用户会以为没领到。
            let reward = result
                .get("rewardCredits")
                .and_then(api::as_num)
                .unwrap_or(0);
            Ok(json!({ "ok": true, "uid": uid, "rewardCredits": reward }))
        }
    }
}

// ---------------------------------------------------------------------------
// 逐账号「今日签到状态」（issue #47）
// ---------------------------------------------------------------------------

/// 批量上限（与 WorkBuddy 的 `get_checkin_status_map` 同值；超出部分忽略）。
pub const STATUS_MAP_MAX_ACCOUNTS: usize = 50;

/// 同时打开的上游请求数（逐账号 GET，串行会明显卡住刷新）。
const STATUS_MAP_MAX_INFLIGHT: usize = 4;

/// 把一次上游结果投影成 `CheckinStatusEntry`（**纯函数**，便于单测）。
///
/// 字段名与 WB 的 `CheckinStatusEntry` 对齐（`dailyCredit` / `todayCredit` /
/// `totalCredits` / `streakDays`），这样前端三平台共用同一套渲染：
/// - Qoder 的「今日可得」= `reward_credits` ⇒ `dailyCredit`；
/// - 「今日已得」只在**今天确实签过**时才有意义 ⇒ `todayCredit = reward`，
///   否则 `null`（`0` 会被读成「签了但没给分」）；
/// - 「累计已得」= `total_reward_credits` ⇒ `totalCredits`。
fn status_entry(status: &api::CheckinStatus) -> Value {
    json!({
        "ok": true,
        "todayCheckedIn": status.today_checked_in,
        "dailyCredit": status.reward_credits,
        "todayCredit": if status.today_checked_in { json!(status.reward_credits) } else { Value::Null },
        "totalCredits": status.total_reward_credits,
        "streakDays": status.streak_days,
        // 上游原始状态词（`CLAIMABLE` / `CLAIMED` / `DISABLED`）——「未开放」那一档
        // 前端要据此说明原因，不能只看到一个 `false`。
        "status": status.status,
        "active": status.active,
    })
}

/// 读失败 / 无可用凭据时的条目（`ok:false`，**不得**当成「未签到」事实）。
fn failure_entry(error: String) -> Value {
    json!({
        "ok": false,
        "todayCheckedIn": false,
        "error": error,
    })
}

/// 逐账号「今日签到状态」（`get_qoder_checkin_status_map`；**只读**，issue #47）。
///
/// 返回 `Record<uid, CheckinStatusEntry>`（**与 WorkBuddy 同构**）。
///
/// ## 为什么是「逐个问上游」而不是读 `status_for` 的快照
///
/// `status_for` 是**整区域**的一次性视图（打开签到页时用），口径是「全部账号」；
/// 而这里要回答的是「**这几个**账号今天签没签」，且用户点的是单个账号的刷新。
/// 直接复用 `status_for` 会让一次单账号刷新拉全量上游请求（N 个账号 = N 次），
/// 正是 WB 侧 `getCheckinStatusMap` 文档里记录过的那个 N² 问题。
///
/// ## 边界（前端照此渲染）
///
/// 1. 请求的 uid **不在账号库** ⇒ **不进 map** ⇒ 前端显示「—」；
/// 2. 没有可用 `access`（PAT）⇒ `{ok:false, …, error}`（这是**账号自身**的结论，
///    与「上游挂了」不同，文案要说清）；
/// 3. 上游读失败 ⇒ `{ok:false, todayCheckedIn:false, error}`；
/// 4. 区域接口不存在（`CHECKIN_REASON_NOT_FOUND`，国际版）⇒ 同样走 `ok:false`，
///    但**文案保留上游原因**，前端会把它显示成「本区域未开放」而不是「未签到」；
/// 5. 只读：**绝不**在这里调 `checkin_claim`（签到是写操作，走 `claim_for`）。
pub async fn status_map_for(region: QoderRegion, uids: Vec<String>) -> Value {
    let file = account::load_accounts_for(region);
    let by_uid: std::collections::HashMap<&str, &RawAccount> = file
        .accounts
        .iter()
        .map(|account| (account.uid.as_str(), account))
        .collect();

    let wanted: Vec<String> = uids
        .into_iter()
        .map(|uid| uid.trim().to_string())
        .filter(|uid| !uid.is_empty() && by_uid.contains_key(uid.as_str()))
        .take(STATUS_MAP_MAX_ACCOUNTS)
        .collect();
    if wanted.is_empty() {
        return json!({});
    }

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(STATUS_MAP_MAX_INFLIGHT));
    let mut tasks = Vec::with_capacity(wanted.len());
    for uid in wanted {
        let access = by_uid
            .get(uid.as_str())
            .and_then(|account| access_token_of(account))
            .map(str::to_string);
        let today = store::today();
        let stamped = by_uid
            .get(uid.as_str())
            .and_then(|account| account::checkin_stamp_day(account))
            .is_some_and(|day| day == today);
        let permit = semaphore.clone();
        tasks.push(tokio::spawn(async move {
            let Ok(_permit) = permit.acquire_owned().await else {
                return (uid, failure_entry("并发闸门已关闭".to_string()));
            };
            let Some(access) = access else {
                return (
                    uid,
                    failure_entry("没有可调用的 access token（PAT 不能用于签到端点）".to_string()),
                );
            };
            // 活动平台优先；整区没有活动平台（404/405/410）才回落旧通道。
            // 判据与 `status_for` 完全同源 —— 两处各写一套会让「签到页」与
            // 「卡片上的今日状态」给出不同答案。
            let snapshot = campaign::fetch_campaigns(region, &uid, &access).await;
            if snapshot.ok {
                return (uid, campaign_entry(&snapshot, stamped));
            }
            if snapshot.available {
                return (
                    uid,
                    failure_entry(
                        snapshot
                            .error
                            .unwrap_or_else(|| "活动列表读取失败".to_string()),
                    ),
                );
            }
            match api::checkin_status(region, &uid, &access).await {
                Ok(status) => (uid, status_entry(&status)),
                Err(error) => (uid, failure_entry(error)),
            }
        }));
    }

    let mut map = serde_json::Map::with_capacity(tasks.len());
    for task in tasks {
        // 任务 panic 时**不写条目**（前端显示「—」）——绝不编一个 `false` 顶上。
        if let Ok((uid, entry)) = task.await {
            map.insert(uid, entry);
        }
    }
    Value::Object(map)
}

/// 批量签到：对区域内**全部**账号各跑一次（顺序，避免并发打上游触发风控）。
///
/// 供后台排程（`ScheduleTask::QoderCheckin`）与将来的「一键签到」入口使用；返回逐账号
/// 结果 + 汇总（部分失败**如实报**，不把「一个账号成功」说成「整批成功」）。
///
/// ⚠️ 为什么顺序而不是并发：签到是**写操作**且直接对应上游风控计数；`status_map_for`
/// 那种只读查询才适合并发。这里沿用官方客户端「逐个来」的节奏。
///
/// ## ★ 为什么返回值里多了 `progress` / `retryAtMs`
///
/// 上游每日实例在 **10:00（UTC+8）** 切换，而排程的默认时点在这之前 —— 09:00 那一轮
/// 签的必然是上一个实例（`claimStatus: CLAIMED`），于是「每轮都成功、每月都没领到」。
/// 这里把「本轮是否真的领到东西」与「要不要等新实例再跑一轮」结构化地带出来，
/// 由排程循环（`scheduler::schedule_loop`）决定是否补跑，**不在本函数里 sleep**
/// （那会阻塞排程并让手动入口也跟着卡住）。
pub async fn run_cycle_for(region: QoderRegion) -> Value {
    let file = account::load_accounts_for(region);
    let mut results = Vec::new();
    let mut ok_count = 0usize;
    let mut failures = 0usize;
    let mut already_count = 0usize;
    let mut earned = 0i64;
    let mut progress = false;
    for account in &file.accounts {
        match claim_for(region, &account.uid).await {
            Ok(value) => {
                let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
                if ok {
                    ok_count += 1;
                }
                // 「未开放」（列表里没有积分类活动 / 整区没有活动平台）不是失败：
                // 记成失败会让台账每天一行红字，而它其实什么也没坏。
                let disabled = value
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !ok && !disabled {
                    failures += 1;
                }
                if value.get("already").and_then(Value::as_bool) == Some(true) {
                    already_count += 1;
                }
                let row_earned = value.get("earned").and_then(api::as_num).unwrap_or(0);
                earned += row_earned;
                let claimed_count = value
                    .get("claimed")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                // 「有进展」只认**真的领到**：`ok:true` 也可能只是「上游说今天已领」。
                if row_earned > 0 || claimed_count > 0 {
                    progress = true;
                }
                results.push(json!({
                    "uid": account.uid,
                    "name": account::display_name(account),
                    "result": value,
                }));
            }
            Err(error) => {
                failures += 1;
                results.push(json!({
                    "uid": account.uid,
                    "name": account::display_name(account),
                    "result": { "ok": false, "error": error },
                }));
            }
        }
    }

    let now_ms = config::now_ms();
    let retry_at_ms = retry_after_ms(now_ms / 1000, progress, failures, already_count);

    let entry = json!({
        "atMs": now_ms,
        "at": store::timestamp(),
        "total": file.accounts.len(),
        "okCount": ok_count,
        "earned": earned,
        "progress": progress,
        "failures": failures,
        "retryAtMs": retry_at_ms.map_or(Value::Null, |at| json!(at)),
        "accounts": results
            .iter()
            .map(|row| json!({
                "uid": row["uid"],
                "name": row["name"],
                "ok": row["result"]["ok"],
                "already": row["result"]["already"],
                "earned": row["result"]["earned"],
                "disabled": row["result"]["disabled"],
                "error": row["result"]["error"],
            }))
            .collect::<Vec<_>>(),
    });
    record_cycle(region, entry, retry_at_ms);

    json!({
        "region": region.as_str(),
        "total": file.accounts.len(),
        "okCount": ok_count,
        "earned": earned,
        "progress": progress,
        "failures": failures,
        "retryAtMs": retry_at_ms.map_or(Value::Null, |at| json!(at)),
        "results": results,
    })
}

/// 「本轮只是重复签上一个实例」⇒ 给出新实例出来后的补跑时刻（纯函数，可测）。
///
/// 四个条件缺一不可：
/// - **没有进展**：真领到东西就不需要补跑；
/// - **没有失败**：失败是上游/网络问题，重试只会多打一次上游（真实故障留给下一轮排程）；
/// - **至少一个账号报「已签」**：否则说明今天本来就领不到（未开放 / 无可领项）；
/// - **当前在 10:00 之前**：此时「今天的实例」还没出来，已签的必然是昨天的那个。
pub fn retry_after_ms(
    now_secs: i64,
    progress: bool,
    failures: usize,
    already: usize,
) -> Option<i64> {
    if progress || failures > 0 || already == 0 || !before_daily_reset(now_secs) {
        return None;
    }
    let (next_at, _) = campaign::next_checkin_window(now_secs);
    let at_ms = (next_at + RETRY_AFTER_RESET_SECS) * 1000;
    (at_ms > now_secs * 1000).then_some(at_ms)
}

/// 当天 10:00（UTC+8）之前 —— 上游「今天的那个实例」尚未刷新。
fn before_daily_reset(now_secs: i64) -> bool {
    const DAY: i64 = 86_400;
    const RESET: i64 = 10 * 3600;
    (now_secs + 8 * 3600).rem_euclid(DAY) < RESET
}

/// 读本区域的签到台账（损坏 / 不存在 ⇒ 空台账，**不报错**）。
fn load_cycle_log(region: QoderRegion) -> Value {
    let value: Value = store::read_json(&paths::checkin_logs_file_for(region));
    if value.is_object() {
        value
    } else {
        json!({ "entries": [], "pendingRetryMs": Value::Null })
    }
}

fn save_cycle_log(region: QoderRegion, value: &Value) {
    if let Err(error) = store::write_json(&paths::checkin_logs_file_for(region), value) {
        // 台账是**诊断**数据：写不进去不能影响签到本身，但必须留痕
        //（否则「界面没有记录」会被误读成「后台没跑」）。
        runtime_log::warn("qoder-checkin", format!("签到台账落盘失败: {error}"));
    }
}

/// 追加一条台账（新的在前，最多 [`LOG_KEEP`] 条）+ 写运行日志。
fn record_cycle(region: QoderRegion, entry: Value, retry_at_ms: Option<i64>) {
    let mut log = load_cycle_log(region);
    let mut entries = log
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries.insert(0, entry.clone());
    entries.truncate(LOG_KEEP);
    if let Some(map) = log.as_object_mut() {
        map.insert("entries".to_string(), Value::Array(entries));
        map.insert(
            "pendingRetryMs".to_string(),
            retry_at_ms.map_or(Value::Null, |at| json!(at)),
        );
    }
    save_cycle_log(region, &log);

    let total = entry["total"].as_u64().unwrap_or(0);
    let ok_count = entry["okCount"].as_u64().unwrap_or(0);
    let earned = entry["earned"].as_i64().unwrap_or(0);
    let failures = entry["failures"].as_u64().unwrap_or(0);
    let mut message = format!(
        "{} 账号 {total} · 成功 {ok_count} · 领到 {earned}",
        region.as_str()
    );
    if failures > 0 {
        let first = entry["accounts"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["ok"] != Value::Bool(true))
                    .and_then(|row| row["error"].as_str())
            })
            .unwrap_or("未知原因");
        message.push_str(&format!(" · 失败 {failures}（首个: {first}）"));
        runtime_log::warn("qoder-checkin", message);
        return;
    }
    if entry["progress"] == Value::Bool(true) {
        runtime_log::info("qoder-checkin", message);
        return;
    }
    if let Some(at) = retry_at_ms {
        let at_text = chrono::DateTime::from_timestamp(at / 1000, 0)
            .map(|instant| {
                instant
                    .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).expect("UTC+8 恒有效"))
                    .format("%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        message.push_str(&format!(
            " · 今日实例已领完（上游 10:00 才刷新），已排 {at_text} 补跑"
        ));
    } else {
        message.push_str(" · 本轮无事可做");
    }
    runtime_log::info("qoder-checkin", message);
}

/// 该区域「等新实例刷新后再跑一轮」的待办时刻（epoch 毫秒）。
pub fn pending_retry_ms(region: QoderRegion) -> Option<i64> {
    load_cycle_log(region)
        .get("pendingRetryMs")
        .and_then(Value::as_i64)
}

/// 清掉待办时刻（已到点执行 / 任务被关闭时调用）。
///
/// 只在**确实有值**时才写盘：排程循环会周期性地查这个值，无待办时反复重写
/// 一个文件既浪费，也会让文件的「最后修改时间」失去诊断意义。
pub fn clear_pending_retry(region: QoderRegion) {
    let mut log = load_cycle_log(region);
    if log
        .get("pendingRetryMs")
        .map(Value::is_null)
        .unwrap_or(true)
    {
        return;
    }
    if let Some(map) = log.as_object_mut() {
        map.insert("pendingRetryMs".to_string(), Value::Null);
    }
    save_cycle_log(region, &log);
}

/// 台账的下发形态（只读命令 `qoder_checkin_logs` 用；不回任何令牌）。
pub fn logs_for(region: QoderRegion) -> Value {
    let log = load_cycle_log(region);
    json!({
        "region": region.as_str(),
        "entries": log.get("entries").cloned().unwrap_or_else(|| json!([])),
        "pendingRetryMs": log.get("pendingRetryMs").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CST 时刻 → epoch 秒（测试里只用来钉窗口边界）。
    fn cst_secs(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .expect("测试时刻必须合法")
            .timestamp()
    }

    /// 上游每日实例 10:00（UTC+8）切换 ⇒ **10:00 之前那一轮签的必然是上一个实例**。
    ///
    /// 本用例钉住「什么时候该补跑」这张表：
    /// - 09:00 且只有 `already` ⇒ 排到当天 10:05（这就是「Qoder 总是没有自动签到」的修复点）；
    /// - 21:00 且只有 `already` ⇒ **不排**（今天真领过了，再补跑只是多打一次上游）；
    /// - 有进展 / 有失败 / 没人报已签 ⇒ 都不排（失败留给下一轮排程，不做每小时重试）。
    ///
    /// 可证伪：去掉 `before_daily_reset` ⇒ 第二条变红；去掉 `already == 0` ⇒ 第四条变红。
    #[test]
    fn retry_is_scheduled_only_before_the_daily_reset() {
        let at_nine = cst_secs("2026-11-09T09:00:00+08:00");
        assert_eq!(
            retry_after_ms(at_nine, false, 0, 2),
            Some(cst_secs("2026-11-09T10:05:00+08:00") * 1000),
            "09:00 只是重复签上一个实例 ⇒ 必须排到 10:00 刷新后补跑"
        );
        assert_eq!(
            retry_after_ms(cst_secs("2026-11-09T21:00:00+08:00"), false, 0, 2),
            None,
            "21:00 的「已签」是当天实例，不该再排"
        );
        assert_eq!(retry_after_ms(at_nine, true, 0, 2), None, "真领到就不补跑");
        assert_eq!(retry_after_ms(at_nine, false, 1, 2), None, "有失败不做补跑");
        assert_eq!(
            retry_after_ms(at_nine, false, 0, 0),
            None,
            "没人报已签 ⇒ 无从判断"
        );
    }

    fn status(status: &str, today: bool) -> api::CheckinStatus {
        api::CheckinStatus {
            status: status.into(),
            active: status == "CLAIMABLE" || status == "CLAIMED",
            today_checked_in: today,
            streak_days: 3,
            total_claim_days: 12,
            reward_credits: 100,
            total_reward_credits: 1200,
            next_claim_at: 0,
            last_claimed_at: 0,
            reward_expires_at: 0,
        }
    }

    /// 状态映射：字段名必须与 WB 的 `CheckinStatusEntry` 对齐，且「读不到」≠「没签」。
    #[test]
    fn status_entry_maps_fields_and_keeps_failures_distinguishable() {
        let checked = status_entry(&status("CLAIMED", true));
        assert_eq!(checked["ok"], Value::Bool(true));
        assert_eq!(checked["todayCheckedIn"], Value::Bool(true));
        assert_eq!(checked["dailyCredit"], 100, "今日可得 = reward_credits");
        assert_eq!(checked["todayCredit"], 100, "今天签过才有今日已得");
        assert_eq!(checked["totalCredits"], 1200);
        assert_eq!(checked["streakDays"], 3);
        assert_eq!(checked["status"], "CLAIMED");

        // 今天没签 ⇒ `todayCredit` 是 null（0 会被读成「签了但没给分」）。
        let pending = status_entry(&status("CLAIMABLE", false));
        assert_eq!(pending["todayCheckedIn"], Value::Bool(false));
        assert_eq!(pending["todayCredit"], Value::Null);

        // 读失败：`ok:false` + 文案，且**没有** `status` 之类的伪事实字段。
        let failed = failure_entry("HTTP 500".to_string());
        assert_eq!(failed["ok"], Value::Bool(false));
        assert_eq!(failed["todayCheckedIn"], Value::Bool(false));
        assert_eq!(failed["error"], "HTTP 500");
        assert!(
            failed.get("status").is_none(),
            "读失败不得带上游状态词（会被读成「未开放」）"
        );
    }

    /// 空数组 / 未知 uid ⇒ 一条上游请求都不发，直接回 `{}`。
    #[tokio::test]
    async fn status_map_returns_empty_without_upstream_calls() {
        // 账号库读的是 `$HOME` 下的文件 ⇒ 必须隔离（本仓单测纪律）。
        let home = std::env::temp_dir().join(format!("qoder-status-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("创建隔离目录");
        let _guard = crate::modules::config::HomeOverrideGuard::set(&home);

        assert_eq!(
            status_map_for(QoderRegion::Cn, Vec::new()).await,
            json!({}),
            "空数组 ⇒ 不发请求"
        );
        assert_eq!(
            status_map_for(QoderRegion::Cn, vec!["不存在".to_string()]).await,
            json!({}),
            "未知账号不进 map（前端显示「—」）"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 变异：把「已签」判据从 `today_checked_in` 改成只看 status ⇒ 断言红
    /// （跨日时 CLAIMED 但今天没签的账号会被误判成已签而漏签）。
    /// ★ 「今日已签」**仍然**是成功（issue #46 的 D 项的另一半）。
    ///
    /// 与 `disabled_is_not_reported_as_success` 成对：两个出口的 `ok` 必须相反，
    /// 否则「把已签说成失败」与「把未开放说成成功」同样会骗到用户。
    #[test]
    fn already_is_reported_as_success() {
        let mut state = status("CLAIMED", true);
        state.streak_days = 3;
        state.reward_credits = 100;
        assert_eq!(
            claim_decision(&state),
            ClaimDecision::Already,
            "today_checked_in ⇒ Already"
        );
        let response = already_claim_response("u1", &state);
        assert_eq!(response["ok"], Value::Bool(true), "已签是真成功");
        assert_eq!(response["already"], Value::Bool(true));
        assert_eq!(response["streakDays"], 3);
        assert_eq!(response["rewardCredits"], 100);
        // 两档的 ok 必须相反（防止有人顺手把两处都写成 true / 都写成 false）。
        assert_ne!(
            response["ok"],
            disabled_claim_response("u1", "DISABLED")["ok"],
            "「已签」与「未开放」的 ok 必须相反"
        );
    }

    /// ★ 「通道未开放」**不得**报成功（issue #46 的 D 项的回归钉子）。
    ///
    /// 可证伪：把 `disabled_claim_response` 里的 `ok` 改回 `true` ⇒ 第一条断言红。
    #[test]
    fn disabled_is_not_reported_as_success() {
        let response = disabled_claim_response("u1", "DISABLED");
        assert_eq!(
            response["ok"],
            Value::Bool(false),
            "上游 409 不携带幂等语义（编造的 campaignKey 也 409）⇒ 未开放 ≠ 领到了"
        );
        assert_eq!(
            response["disabled"],
            Value::Bool(true),
            "第三档要有自己的标记"
        );
        assert_eq!(response["status"], "DISABLED", "上游状态词如实透出");
        // 前端按 `ok` 计数 ⇒ 这里必须是 false，否则批量计数会说谎。
        assert_eq!(
            claim_decision(&status("DISABLED", false)),
            ClaimDecision::Disabled("DISABLED".into())
        );
    }

    #[test]
    fn claim_decision_covers_three_states() {
        assert_eq!(
            claim_decision(&status("CLAIMED", true)),
            ClaimDecision::Already
        );
        assert_eq!(
            claim_decision(&status("DISABLED", false)),
            ClaimDecision::Disabled("DISABLED".into())
        );
        assert_eq!(
            claim_decision(&status("CLAIMABLE", false)),
            ClaimDecision::ClaimNow
        );
        // CLAIMED 但今天没签（跨日）⇒ 仍要尝试领取（不能只看状态词）。
        assert_eq!(
            claim_decision(&status("CLAIMED", false)),
            ClaimDecision::ClaimNow
        );
    }
    // ---- 活动平台通道（issue #58）----

    fn campaign_item(status: &str, amount: i64, kind: &str) -> campaign::CampaignItem {
        campaign::CampaignItem {
            campaign_id: "c1".into(),
            campaign_key: "daily".into(),
            title: "每日领取 100 Credits".into(),
            description: String::new(),
            detail_url: String::new(),
            button_text: String::new(),
            action_type: "CLAIM_BENEFIT".into(),
            claim_status: status.into(),
            benefit_kind: kind.into(),
            benefit_amount: amount,
            required_achievement_key: String::new(),
            achievement_completed: true,
            unavailable_reason: String::new(),
            start_at: 0,
            end_at: 0,
        }
    }

    fn snapshot(items: Vec<campaign::CampaignItem>) -> campaign::CampaignsSnapshot {
        campaign::CampaignsSnapshot {
            ok: true,
            available: true,
            show_campaign: true,
            claimable: true,
            campaign_url: String::new(),
            items,
            identity: "derived",
            machine_headers: "omitted",
            hint: String::new(),
            error: None,
        }
    }

    /// 聚合口径：只认积分类活动；已领 / 可领 / 都没有 三种形状。
    #[test]
    fn summarize_campaigns_splits_claimed_and_claimable() {
        let claimable = summarize_campaigns(&[campaign_item("CLAIMABLE", 100, "CREDITS")]);
        assert!(claimable.has_benefit && claimable.claimable && !claimable.claimed);
        assert_eq!(claimable.claimable_amount, 100);

        let claimed = summarize_campaigns(&[campaign_item("CLAIMED", 100, "CREDITS")]);
        assert!(claimed.claimed && !claimed.claimable);
        assert_eq!(claimed.claimed_amount, 100);

        // 空列表 / 只有兑换码活动 ⇒ 都不是签到活动（`has_benefit` 必须是 false，
        // 否则「未开放」会被当成「已签」）。
        assert!(!summarize_campaigns(&[]).has_benefit);
        assert!(
            !summarize_campaigns(&[campaign_item("CLAIMABLE", 0, "REDEMPTION_CODE")]).has_benefit
        );
    }

    /// 状态投影：`todayCheckedIn` 只认上游 `claimStatus`；没签时 `todayCredit` 是 null。
    #[test]
    fn campaign_entry_reports_checked_in_from_upstream_status_only() {
        let claimed = campaign_entry(
            &snapshot(vec![campaign_item("CLAIMED", 100, "CREDITS")]),
            false,
        );
        assert_eq!(claimed["todayCheckedIn"], Value::Bool(true));
        assert_eq!(claimed["todayCredit"], 100);
        assert_eq!(claimed["dailyCredit"], 100, "已签时今日可得退回已领额度");
        assert_eq!(claimed["status"], "CLAIMED");

        let pending = campaign_entry(
            &snapshot(vec![campaign_item("CLAIMABLE", 100, "CREDITS")]),
            false,
        );
        assert_eq!(pending["todayCheckedIn"], Value::Bool(false));
        assert_eq!(
            pending["todayCredit"],
            Value::Null,
            "没签就不能有「今日已得」"
        );
        assert_eq!(pending["dailyCredit"], 100);
        assert_eq!(pending["status"], "CLAIMABLE");
        assert_eq!(pending["active"], Value::Bool(true));

        // 本地盖章**不是**判据：盖章了但上游没 CLAIMED ⇒ 仍然算没签。
        let stamped_only = campaign_entry(
            &snapshot(vec![campaign_item("CLAIMABLE", 100, "CREDITS")]),
            true,
        );
        assert_eq!(stamped_only["todayCheckedIn"], Value::Bool(false));
        assert_eq!(stamped_only["stampedToday"], Value::Bool(true));
    }

    /// 旧契约兼容：`already` 必须是布尔、`rewardCredits` 取 `earned`。
    #[test]
    fn normalize_campaign_claim_keeps_legacy_contract() {
        let claimed = normalize_campaign_claim(json!({
            "ok": true, "earned": 100,
            "claimed": [{"campaignId": "c1", "amount": 100}],
            "alreadyItems": [],
        }));
        assert_eq!(claimed["ok"], Value::Bool(true));
        assert_eq!(claimed["already"], Value::Bool(false));
        assert_eq!(claimed["rewardCredits"], 100);
        assert_eq!(claimed["claimedCount"], 1);

        let already = normalize_campaign_claim(json!({
            "ok": true, "earned": 0, "claimed": [], "alreadyItems": [{"campaignId": "c1"}],
        }));
        assert_eq!(already["already"], Value::Bool(true));
        assert_eq!(already["rewardCredits"], 0);

        // 未开放：`ok:false` 原样保留（这条是 de932d7 的核心修复，不许被 normalize 洗白）。
        let disabled = normalize_campaign_claim(json!({
            "ok": false, "disabled": true, "earned": 0, "claimed": [], "alreadyItems": [],
        }));
        assert_eq!(disabled["ok"], Value::Bool(false));
        assert_eq!(disabled["disabled"], Value::Bool(true));
    }
    // ── 真机探针：**会真的领取**今日奖励（消耗当天额度），只在手动跑时执行 ──
    //
    //   cargo test -p buddy-switch-core --lib live_checkin_claim -- --ignored --nocapture
    //
    // 它走的是生产路径（`claim_for` → 活动平台 → 落盘），因此能一次性验证
    // 「拉列表 / 领取 / 解析 / 盖章」整条链路。**不要在 CI 里跑**（CI 没有账号库）。
    #[tokio::test]
    #[ignore = "真机探针：会真的领取今日奖励（消耗当天额度）"]
    async fn live_checkin_claim() {
        let uid = std::env::var("QODER_LIVE_UID").expect("需 QODER_LIVE_UID（账号库里的 uid）");
        let region = match std::env::var("QODER_LIVE_REGION").as_deref() {
            Ok("global") => QoderRegion::Global,
            _ => QoderRegion::Cn,
        };
        let result = claim_for(region, &uid).await.expect("签到调用本身失败");
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
        assert_eq!(result["source"], "campaign", "应走活动平台");
    }

    // ── 真机探针：跑**整轮**签到（会真的领取），并验证台账落盘与窗口判定 ──
    //
    //   cargo test -p buddy-switch-core --lib live_cycle_writes_ledger -- --ignored --nocapture
    //
    // 与上面的单账号探针分开：那条只验「一次 claim」，本探针验的是**排程那一轮**的
    // 形状 —— `progress` / `already` / `retryAtMs` 与 `qoder/checkin_logs.<region>.json`。
    // 今天已签时预期：`progress=false`；`retryAtMs` 只有在 10:00（UTC+8）之前才非空。
    #[tokio::test]
    #[ignore = "真机探针：会真的跑一轮签到（已签时幂等，未签时会领取）"]
    async fn live_cycle_writes_ledger() {
        let value = run_cycle_for(QoderRegion::Cn).await;
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
        let ledger = logs_for(QoderRegion::Cn);
        println!(
            "{}",
            serde_json::to_string_pretty(&ledger).unwrap_or_default()
        );
        assert!(
            ledger["entries"]
                .as_array()
                .is_some_and(|entries| !entries.is_empty()),
            "一轮签到必须落一条台账（否则界面上的「最近一次」永远是空的）"
        );
        assert!(
            ledger["entries"][0]
                .get("at")
                .and_then(Value::as_str)
                .is_some(),
            "台账条目要带可读时间"
        );
    }
}
