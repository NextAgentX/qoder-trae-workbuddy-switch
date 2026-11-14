//! Gateway integration helpers for the core risk admission store.

use buddy_switch_core::modules::risk_control::{RiskPermit, RiskScope, RiskStore};

pub fn admit_request(
    store: &Result<RiskStore, String>,
    product: &str,
    region: &str,
    account_id: &str,
    now_ms: u64,
) -> Result<RiskPermit, String> {
    let settings =
        buddy_switch_core::modules::risk_control::platform_risk_settings(product, region)?;
    if !settings.enabled {
        return Ok(RiskPermit::untracked());
    }
    let store = store
        .as_ref()
        .map_err(|error| format!("risk state unavailable: {error}"))?;
    let scope = RiskScope::new(product, region)?;
    if product == "qoder" {
        store.begin_request_ignoring_cooling_at(&scope, account_id, &settings, now_ms)
    } else {
        store.begin_request_at(&scope, account_id, &settings, now_ms)
    }
}

/// Exclude accounts with persisted risk states before the pool chooses one.
/// Qoder model-level cooldowns are handled by its pool, so account-wide
/// `Cooling` records do not exclude a Qoder account across all models.
/// Failure to read risk state is fail-closed; admission will reject it as well.
pub fn account_blocked(
    store: &Result<RiskStore, String>,
    product: &str,
    region: &str,
    account_id: &str,
    now_ms: u64,
) -> bool {
    let Ok(store) = store.as_ref() else {
        return true;
    };
    let Ok(scope) = RiskScope::new(product, region) else {
        return true;
    };
    // Persisted pauses remain a safety state even while the shared limiter is
    // disabled. Qoder account-wide cooling is deliberately ignored here: its
    // gateway tracks 429 cooldowns per model in the Qoder pool.
    match store.state_at(&scope, account_id, now_ms) {
        buddy_switch_core::modules::risk_control::AccountRiskState::Paused { .. } => true,
        buddy_switch_core::modules::risk_control::AccountRiskState::Cooling { .. } => {
            product != "qoder"
        }
        buddy_switch_core::modules::risk_control::AccountRiskState::Active => false,
    }
}

pub fn record_rate_limit(
    store: &Result<RiskStore, String>,
    product: &str,
    region: &str,
    account_id: &str,
    reason: String,
    reset_at_ms: Option<u64>,
    now_ms: u64,
) -> Result<(), String> {
    let settings =
        buddy_switch_core::modules::risk_control::platform_risk_settings(product, region)?;
    if !settings.enabled {
        return Ok(());
    }
    let store = store
        .as_ref()
        .map_err(|error| format!("risk state unavailable: {error}"))?;
    store.record_outcome_at(
        &RiskScope::new(product, region)?,
        account_id,
        buddy_switch_core::modules::risk_control::RiskOutcome::RateLimited(reason),
        reset_at_ms,
        now_ms,
        settings.fallback_cooldown_secs,
    )
}

pub fn pause_account(
    store: &Result<RiskStore, String>,
    product: &str,
    region: &str,
    account_id: &str,
    reason: String,
    now_ms: u64,
) -> Result<(), String> {
    if !buddy_switch_core::modules::risk_control::platform_risk_settings(product, region)?.enabled {
        return Ok(());
    }
    let store = store
        .as_ref()
        .map_err(|error| format!("risk state unavailable: {error}"))?;
    store.record_outcome_at(
        &RiskScope::new(product, region)?,
        account_id,
        buddy_switch_core::modules::risk_control::RiskOutcome::Banned(reason),
        None,
        now_ms,
        60,
    )
}

pub fn recover_account(
    store: &Result<RiskStore, String>,
    product: &str,
    region: &str,
    account_id: &str,
) -> Result<(), String> {
    let store = store
        .as_ref()
        .map_err(|error| format!("risk state unavailable: {error}"))?;
    store.recover_account(&RiskScope::new(product, region)?, account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_switch_core::modules::risk_control::{AccountRiskState, RiskOutcome};

    #[test]
    fn persisted_pause_excludes_the_account_before_pool_selection() {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-gateway-risk-{}-{}",
            std::process::id(),
            buddy_switch_core::modules::config::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = RiskStore::open(dir.join("risk.json")).unwrap();
        let scope = RiskScope::new("qoder", "global").unwrap();
        store
            .record_outcome_at(
                &scope,
                "uid-1",
                RiskOutcome::Banned("auth rejected".into()),
                None,
                1_000,
                60,
            )
            .unwrap();
        let store = Ok(store);

        assert!(account_blocked(&store, "qoder", "global", "uid-1", 1_001));
        assert!(!account_blocked(&store, "qoder", "global", "uid-2", 1_001));
        assert_eq!(
            store.as_ref().unwrap().state(&scope, "uid-1"),
            AccountRiskState::Paused {
                reason: "auth rejected".into()
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn persisted_safety_states_survive_disabled_rate_controls() {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-gateway-risk-disabled-{}-{}",
            std::process::id(),
            buddy_switch_core::modules::config::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = RiskStore::open(dir.join("risk.json")).unwrap();

        let workbuddy = RiskScope::new("workbuddy", "cn").unwrap();
        store
            .record_outcome_at(
                &workbuddy,
                "uid-cooling",
                RiskOutcome::RateLimited("upstream 429".into()),
                None,
                1_000,
                60,
            )
            .unwrap();

        let qoder = RiskScope::new("qoder", "cn").unwrap();
        store
            .record_outcome_at(
                &qoder,
                "uid-model-cooling",
                RiskOutcome::RateLimited("model 429".into()),
                None,
                1_000,
                60,
            )
            .unwrap();

        let store = Ok(store);
        assert!(
            account_blocked(&store, "workbuddy", "cn", "uid-cooling", 1_001),
            "关闭速率限制不得放行已冷却的 WorkBuddy 账号"
        );
        assert!(
            !account_blocked(&store, "qoder", "cn", "uid-model-cooling", 1_001),
            "Qoder 的模型级冷却必须由模型池判断，不能在选号前按账号整体排除"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
