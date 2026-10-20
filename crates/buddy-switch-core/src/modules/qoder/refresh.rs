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
}
