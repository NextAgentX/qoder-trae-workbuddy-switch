//! Qoder 账号**令牌刷新**：按凭据形态选路由 → 调上游 → **只回写令牌字段**。
//!
//! ## 三条路由（按**令牌前缀**，不是按区域）
//!
//! | 凭据 | 路由 |
//! |:--|:--|
//! | `drt-`（device refresh） | `POST /api/v1/deviceToken/refresh` |
//! | `jrt-`（job refresh） | `POST /api/v1/jobToken/refresh` |
//! | 只有 `pt-`（PAT） | `POST /api/v1/jobToken/exchange`（PAT 换 jobToken） |
//!
//! 认不出的刷新令牌**不猜**：有 PAT 就落到兑换兜底，两样都没有 ⇒ 可读错误
//! （「需要重新登录/重新导入」），绝不打到一个碰运气的端点上。
//!
//! ## 为什么不走 `upsert_account_for`
//!
//! 那是导入路径，会**整条替换**账号对象 —— 刷新时用它，会把本地备注、昵称等
//! 一切上游不知道的字段清掉。刷新只动令牌字段（[`account::set_tokens_for`]）。

use serde_json::{json, Value};

use super::account::{self, RawAccount};
use super::api::{self, RefreshKind};
use super::region::QoderRegion;
use super::store;
use super::token::{self, TokenKind};

/// 刷新路由（由凭据形态选出，见模块头）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPlan {
    Device,
    Job,
    PatExchange,
}

impl RefreshPlan {
    pub fn as_str(self) -> &'static str {
        match self {
            RefreshPlan::Device => "device",
            RefreshPlan::Job => "job",
            RefreshPlan::PatExchange => "pat-exchange",
        }
    }
}

/// 由账号凭据选出刷新路由。**纯函数**（可测）：不发起任何调用。
pub fn plan_refresh(account: &RawAccount) -> Result<RefreshPlan, String> {
    if let Some(refresh) = account
        .refresh_token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        match token::kind_of(refresh) {
            Some(TokenKind::DeviceRefresh) => return Ok(RefreshPlan::Device),
            Some(TokenKind::JobRefresh) => return Ok(RefreshPlan::Job),
            // 认不出的刷新令牌**不猜**：落到 PAT 兜底（若有）。
            _ => {}
        }
    }
    if account
        .personal_token
        .as_deref()
        .map(|token| !token.is_empty())
        .unwrap_or(false)
    {
        return Ok(RefreshPlan::PatExchange);
    }
    Err("这条账号既没有可用的刷新令牌也没有 PAT —— 需要重新登录或重新导入".to_string())
}

/// 刷新一条账号的令牌并回写账号库。返回面向命令面的视图（**不含令牌明文**）。
pub async fn refresh_for(region: QoderRegion, uid: &str) -> Result<Value, String> {
    let account =
        account::find_account_for(region, uid).ok_or_else(|| format!("账号不存在: {uid}"))?;
    let plan = plan_refresh(&account)?;

    let refreshed = match plan {
        RefreshPlan::Device => {
            let refresh = account.refresh_token.as_deref().unwrap_or("");
            api::refresh_token(region, RefreshKind::Device, refresh).await?
        }
        RefreshPlan::Job => {
            let refresh = account.refresh_token.as_deref().unwrap_or("");
            api::refresh_token(region, RefreshKind::Job, refresh).await?
        }
        RefreshPlan::PatExchange => {
            let pat = account.personal_token.as_deref().unwrap_or("");
            api::exchange_pat(region, pat).await?
        }
    };

    // 空令牌一律拒绝回写 —— 写进去等于把账号静默打死。
    if refreshed.access_token.trim().is_empty() {
        return Err("刷新响应没有携带新 access token —— 已拒绝回写（不写空令牌）".to_string());
    }

    let found = account::set_tokens_for(
        region,
        uid,
        Some(&refreshed.access_token),
        refreshed.refresh_token.as_deref(),
        Some(refreshed.expires_at * 1000),
    )?;
    if !found {
        return Err("刷新成功但账号已不在库里（可能被并发删除）—— 未回写".to_string());
    }

    Ok(json!({
        "ok": true,
        "uid": uid,
        "plan": plan.as_str(),
        "accessTokenMasked": store::mask(&refreshed.access_token),
        "expiresAt": refreshed.expires_at * 1000,
    }))
}

// ---------------------------------------------------------------------------
// 令牌保活（后台排程 `ScheduleTask::QoderKeepalive` 的唯一实现）
// ---------------------------------------------------------------------------

/// 「剩余寿命不足」阈值：进入该窗口的账号在巡检中被**提前**刷新。
///
/// ## ★★ 为什么是 48 小时（issue #130）
///
/// 判据只有一条，三个渠道共用：**巡检间隔必须严格小于触发窗口**。
/// 不满足时，每一轮巡检都会落在窗口之外、判为「还不需要刷新」，账号在两轮之间到期
/// —— 看起来任务在跑，实际上一个号都没刷到。
///
/// 改造前这里是 **4 小时**，而 `qoder_keepalive_times` 默认**每天 1 次**（22:00）：
/// `4h < 24h` ⇒ 每个令牌生命周期里只有约 `4/24 ≈ 17%` 的概率被抓住，其余情况账号
/// 到期后**最长约 24 小时**处于过期态，而且 Qoder 侧不会自愈（401 ⇒ `auth_dead`，
/// 只有刷新成功才清）。
///
/// 现在取 48：`24h（间隔） < 48h（窗口）` ⇒ 每轮巡检必然覆盖所有「48 小时内到期」的账号。
/// 为什么不取 26（刚好大于 24）：`jt-` 寿命只有约 24 小时，刚刷出来的号在下一轮巡检时
/// 剩余约 0 小时 —— 数学上够，但**没有任何余量**，巡检被抖动或进程重启推迟几小时就破窗。
/// 48 小时给了一整天余量。
///
/// 代价：每天全量刷一次上游（与 WorkBuddy 的 `keepalive_days = 0` 语义一致）。
/// 保活是**逐账号顺序执行**的（并发刷新同一批账号更容易触发上游风控），个人号池量级
/// 下这个代价可接受；换成「加密巡检到 < 4h」会多 11 个排程时点，设置页交互与
/// 「抖动」语义都要跟着改，收益却更小。
pub const KEEPALIVE_WINDOW_HOURS: i64 = 48;

/// `expires_at` 归一成**毫秒**。
///
/// ⚠️ 两个来源单位不同：我们自己的刷新路径写的是**毫秒**（[`refresh_for`] 传
/// `expires_at * 1000`），而**本机导入**路径把上游 `expiresAt` **原样**抄了下来
/// （实测上游给的是**秒**）。判据用量级：小于 10^11 的一律当秒
/// （10^11 秒 ≈ 公元 5138 年，10^11 毫秒 ≈ 1973 年 —— 两侧都不会误判）。
fn expiry_ms(account: &RawAccount) -> Option<i64> {
    account.expires_at.map(|value| {
        if value < 100_000_000_000 {
            value * 1000
        } else {
            value
        }
    })
}

/// 这条账号是否需要保活刷新（**纯函数**，可测）。
///
/// - 没有可用 `access token` ⇒ `true`（必须换一个；PAT 会走兑换路由）；
/// - 有过期信息且距今不足 [`KEEPALIVE_WINDOW_HOURS`] 小时 ⇒ `true`；
/// - **没有过期信息 ⇒ 按「最近更新时刻」判**（issue #130 §6.2-2）。
///
/// ## 为什么「没有过期信息」不能无条件跳过（改造前的 `None => false`）
///
/// 本机导入路径把上游 `expiresAt` 原样抄下来，但**不是每个账号都有这个字段**
/// （PAT 兑换、老版本导入、上游改字段都可能缺）。改造前这类账号 `needs_keepalive`
/// 恒为 `false` ⇒ **永远不会被保活**，只能等 401 —— 而 Qoder 的 401 会判 `AuthDead`，
/// 只有刷新成功才清，等于要用户手动「恢复」。
///
/// 现在退化为「距上次写盘超过一个窗口就刷一次」：`updated_at` 在每次令牌回写时都会更新
/// （`account::write_back_tokens_for`），所以「刷过一次的号」在 48 小时内不会重复刷。
/// 两个字段都缺（`updated_at` 也没）时按「该刷」处理 —— 保守方向与 WorkBuddy 的
/// `keepalive_days = 0`（每天无条件全刷）一致：宁可多刷一次，也不要让号静默到期。
pub fn needs_keepalive(account: &RawAccount, now_ms: i64) -> bool {
    let has_access = account
        .access_token
        .as_deref()
        .map(|token| !token.is_empty())
        .unwrap_or(false);
    if !has_access {
        return true;
    }
    match expiry_ms(account) {
        Some(at) => at - now_ms <= KEEPALIVE_WINDOW_HOURS * 3_600_000,
        None => stale_since_update(account, now_ms),
    }
}

/// 没有过期信息时的退化判据：**距上次写盘是否已超过一个保活窗口**。
///
/// `updated_at` 是 [`store::now_iso`] 写的本地时区 RFC3339（`2026-10-30T09:00:00+08:00`），
/// 解析失败或缺失一律按「已过期」处理（理由见 [`needs_keepalive`] 的文档）。
fn stale_since_update(account: &RawAccount, now_ms: i64) -> bool {
    let window_ms = KEEPALIVE_WINDOW_HOURS * 3_600_000;
    match account
        .updated_at
        .as_deref()
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text.trim()).ok())
    {
        Some(updated) => now_ms.saturating_sub(updated.timestamp_millis()) >= window_ms,
        None => true,
    }
}

/// 区域令牌保活：逐账号刷新「即将到期 / 已无可用 access token」的那些。
///
/// 返回逐账号结果 + 汇总（**部分失败如实报**，不伪装成全量成功）。
/// 顺序执行：并发刷新同一批账号只会更容易触发上游风控。
pub async fn run_keepalive_cycle_for(region: QoderRegion) -> Value {
    let file = account::load_accounts_for(region);
    let now = chrono::Utc::now().timestamp_millis();
    let mut refreshed: Vec<String> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut skipped = 0usize;

    for account in &file.accounts {
        // 没有可用的刷新路由（既无刷新令牌也无 PAT）⇒ 跳过：刷也刷不动，
        // 报了只是噪音（用户该做的是重新登录 / 重新导入）。
        if plan_refresh(account).is_err() {
            skipped += 1;
            continue;
        }
        if !needs_keepalive(account, now) {
            skipped += 1;
            continue;
        }
        match refresh_for(region, &account.uid).await {
            Ok(_) => refreshed.push(account.uid.clone()),
            Err(error) => failed.push(json!({ "uid": account.uid, "error": error })),
        }
    }

    json!({
        "region": region.as_str(),
        "total": file.accounts.len(),
        "refreshed": refreshed.len(),
        "skipped": skipped,
        "failed": failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_with(refresh: Option<&str>, pat: Option<&str>) -> RawAccount {
        RawAccount {
            uid: "q1".into(),
            nickname: None,
            domain: "qoder.com.cn".into(),
            access_token: Some("dt-old".into()),
            refresh_token: refresh.map(str::to_string),
            personal_token: pat.map(str::to_string),
            expires_at: None,
            source: None,
            remark: None,
            group: None,
            ..Default::default()
        }
    }

    /// 变异：把 `drt-` 也允许走 job 路由 / 去掉 PAT 兜底 ⇒ 断言红。
    #[test]
    fn plan_refresh_routes_by_token_prefix() {
        assert_eq!(
            plan_refresh(&account_with(Some("drt-abc"), None)),
            Ok(RefreshPlan::Device)
        );
        assert_eq!(
            plan_refresh(&account_with(Some("jrt-abc"), None)),
            Ok(RefreshPlan::Job)
        );
        // 认不出的刷新令牌 + PAT ⇒ 兑换兜底。
        assert_eq!(
            plan_refresh(&account_with(Some("xx-abc"), Some("pt-abc"))),
            Ok(RefreshPlan::PatExchange)
        );
        // 只有 PAT ⇒ 兑换。
        assert_eq!(
            plan_refresh(&account_with(None, Some("pt-abc"))),
            Ok(RefreshPlan::PatExchange)
        );
        // 认不出的刷新令牌且无 PAT ⇒ 可读错误（不猜端点）。
        assert!(plan_refresh(&account_with(Some("xx-abc"), None)).is_err());
        // 什么都没有 ⇒ 可读错误。
        let err = plan_refresh(&account_with(None, None)).unwrap_err();
        assert!(err.contains("重新登录"), "{err}");
    }

    /// ★ 保活判据：**没有 access token ⇒ 必刷**；**没有过期信息 ⇒ 按 `updated_at` 退化判**。
    ///
    /// 可证伪：把 `None => stale_since_update(...)` 改回 `None => false` ⇒ 最后两条断言红
    /// （这类账号**永远不会被保活**，只能等 401 —— 而 Qoder 的 401 会判 `AuthDead`）。
    #[test]
    fn keepalive_only_touches_expiring_or_credential_less_accounts() {
        let now = 1_800_000_000_000i64; // 毫秒
                                        // 无 access token ⇒ 必须刷。
        let mut account = account_with(Some("drt-abc"), None);
        account.access_token = None;
        assert!(needs_keepalive(&account, now));
        // 还有很久才过期（5 天后，远在 48h 窗口之外）⇒ 不刷。
        let mut far = account_with(Some("drt-abc"), None);
        far.expires_at = Some(now + 5 * 24 * 3_600_000);
        assert!(!needs_keepalive(&far, now));
        // 进入 48 小时窗口 ⇒ 刷。
        let mut soon = account_with(Some("drt-abc"), None);
        soon.expires_at = Some(now + 3 * 3_600_000);
        assert!(needs_keepalive(&soon, now));
        // 已过期 ⇒ 刷。
        let mut expired = account_with(Some("drt-abc"), None);
        expired.expires_at = Some(now - 1000);
        assert!(needs_keepalive(&expired, now));
        // ★ 没有过期信息、且刚更新过 ⇒ 不刷（不重复打上游）。
        let mut fresh_unknown = account_with(Some("drt-abc"), None);
        fresh_unknown.updated_at = Some(iso_of(now - 3_600_000));
        assert_eq!(fresh_unknown.expires_at, None);
        assert!(
            !needs_keepalive(&fresh_unknown, now),
            "刚写盘一小时 ⇒ 一个窗口内不重复刷"
        );
        // ★ 没有过期信息、且久未更新 ⇒ **必须刷**（改造前这里是永久跳过）。
        let mut stale_unknown = account_with(Some("drt-abc"), None);
        stale_unknown.updated_at = Some(iso_of(now - 49 * 3_600_000));
        assert!(
            needs_keepalive(&stale_unknown, now),
            "超过一个窗口没更新过 ⇒ 必须刷一次，否则这个号永远等 401"
        );
        // ★ 两个字段都缺 ⇒ 按「该刷」处理（保守方向与 WB 的每天全刷一致）。
        let unknown = account_with(Some("drt-abc"), None);
        assert_eq!(unknown.expires_at, None);
        assert!(needs_keepalive(&unknown, now));
    }

    /// 毫秒时间戳 → RFC3339（`store::now_iso` 写的也是这个形态）。
    fn iso_of(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms)
            .expect("合法时间戳")
            .to_rfc3339()
    }

    /// ★★ issue #130 的核心不变量：**巡检间隔必须严格小于触发窗口**。
    ///
    /// 改造前是 `4h 窗口 / 24h 巡检` ⇒ 每个令牌生命周期只有约 17% 的概率被抓住。
    /// 这条断言把「窗口 ≥ 一天的巡检间隔」钉住：谁把窗口改回小于 24 小时，本用例红。
    #[test]
    fn keepalive_window_covers_the_daily_sweep() {
        let day_ms = 24 * 3_600_000;
        assert!(
            KEEPALIVE_WINDOW_HOURS * 3_600_000 > day_ms,
            "窗口（{KEEPALIVE_WINDOW_HOURS}h）必须**严格大于**默认巡检间隔（24h），\
             否则每一轮巡检都会落在窗口外、判为「不需要刷」"
        );
        // 反证：23 小时后到期的号，必须在**当天那一轮**巡检里被刷到。
        let now = 1_800_000_000_000i64;
        let mut soon = account_with(Some("jrt-abc"), None);
        soon.expires_at = Some(now + 23 * 3_600_000);
        assert!(needs_keepalive(&soon, now), "23h 后到期 ⇒ 本轮必须命中");
    }

    /// ★ `expires_at` 的**两种单位**都要判对（导入路径抄的是秒，刷新路径写的是毫秒）。
    ///
    /// 可证伪：删掉 `expiry_ms` 的量级归一 ⇒ 秒形态（本机导入的账号）会被当成
    /// 「1970 年就过期了」或「还有 50000 年」⇒ 要么全量乱刷、要么永不保活。
    #[test]
    fn expiry_units_are_normalized_by_magnitude() {
        let now = 1_800_000_000_000i64;
        // 毫秒形态：3 小时后过期 ⇒ 需刷。
        let mut ms = account_with(Some("drt-abc"), None);
        ms.expires_at = Some((now + 3 * 3_600_000) / 1000 * 1000);
        assert!(needs_keepalive(&ms, now));
        // 秒形态：同一天稍后过期（秒值 ≈ 1.8e9）⇒ 同样需刷（而不是被判成 1970 年）。
        let mut seconds = account_with(Some("drt-abc"), None);
        seconds.expires_at = Some((now + 3 * 3_600_000) / 1000);
        assert!(needs_keepalive(&seconds, now));
        // 秒形态且还很久（5 天后）⇒ 不刷。
        let mut seconds_far = account_with(Some("drt-abc"), None);
        seconds_far.expires_at = Some((now + 5 * 24 * 3_600_000) / 1000);
        assert!(!needs_keepalive(&seconds_far, now));
    }
}
