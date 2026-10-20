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

/// 「剩余寿命不足」阈值：进入该窗口的账号在巡检中被**提前**刷新（与参考实现同值 4 小时）。
///
/// 为什么是 4 小时而不是「等 401 再说」：`access token` 的有效期是**上游签发时决定**的
/// （`dt-` 约 30 天 / `jt-` 24 小时），而网关转发是**长连接上的即时请求** ——
/// 等 401 才刷，那一次调用已经失败了，用户看到的是「突然报错」。
pub const KEEPALIVE_WINDOW_HOURS: i64 = 4;

/// `expires_at` 归一成**毫秒**。
///
/// ⚠️ 两个来源单位不同：我们自己的刷新路径写的是**毫秒**（[`refresh_for`] 传
/// `expires_at * 1000`），而**本机导入**路径把上游 `expiresAt` **原样**抄了下来
/// （实测上游给的是**秒**）。判据用量级：小于 10^11 的一律当秒
/// （10^11 秒 ≈ 公元 5138 年，10^11 毫秒 ≈ 1973 年 —— 两侧都不会误判）。
fn expiry_ms(account: &RawAccount) -> Option<i64> {
    account
        .expires_at
        .map(|value| if value < 100_000_000_000 { value * 1000 } else { value })
}

/// 这条账号是否需要保活刷新（**纯函数**，可测）。
///
/// - 没有可用 `access token` ⇒ `true`（必须换一个；PAT 会走兑换路由）；
/// - 有过期信息且距今不足 [`KEEPALIVE_WINDOW_HOURS`] 小时 ⇒ `true`；
/// - **没有过期信息 ⇒ `false`**：不猜、不为「不知道」去打上游（由手动刷新与
///   上游 401 兜底）—— 无脑全量刷新会把「保活」变成每天白打 N 次上游。
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
        None => false,
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

    /// ★ 保活判据：**没有 access token ⇒ 必刷**；**没有过期信息 ⇒ 不刷**（不猜）。
    ///
    /// 可证伪：把 `None => false` 改成 `None => true` ⇒ 第三条断言变红
    /// （「保活」会退化成「每次巡检全量刷一遍」，每天白打 N 次上游）。
    #[test]
    fn keepalive_only_touches_expiring_or_credential_less_accounts() {
        let now = 1_800_000_000_000i64; // 毫秒
        // 无 access token ⇒ 必须刷。
        let mut account = account_with(Some("drt-abc"), None);
        account.access_token = None;
        assert!(needs_keepalive(&account, now));
        // 还有很久才过期 ⇒ 不刷。
        let mut far = account_with(Some("drt-abc"), None);
        far.expires_at = Some(now + 48 * 3_600_000);
        assert!(!needs_keepalive(&far, now));
        // 进入 4 小时窗口 ⇒ 刷。
        let mut soon = account_with(Some("drt-abc"), None);
        soon.expires_at = Some(now + 3 * 3_600_000);
        assert!(needs_keepalive(&soon, now));
        // 已过期 ⇒ 刷。
        let mut expired = account_with(Some("drt-abc"), None);
        expired.expires_at = Some(now - 1000);
        assert!(needs_keepalive(&expired, now));
        // ★ 没有过期信息 ⇒ **不刷**（不猜）。
        let unknown = account_with(Some("drt-abc"), None);
        assert_eq!(unknown.expires_at, None);
        assert!(!needs_keepalive(&unknown, now));
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
        // 秒形态且还很久 ⇒ 不刷。
        let mut seconds_far = account_with(Some("drt-abc"), None);
        seconds_far.expires_at = Some((now + 48 * 3_600_000) / 1000);
        assert!(!needs_keepalive(&seconds_far, now));
    }
}
