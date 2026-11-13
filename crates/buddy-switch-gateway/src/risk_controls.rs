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
    store.begin_request_at(&scope, account_id, &settings, now_ms)
}

/// Exclude accounts with a persisted pause/cooldown before the pool chooses one.
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
    // Persisted pause/cooling is a safety state, not an optional rate limiter.
    // It must continue to exclude the account even while the shared limiter is
    // disabled, otherwise turning the UI switch off silently re-enables an
    // account that the platform has already rejected.
    matches!(
        store.state_at(&scope, account_id, now_ms),
        buddy_switch_core::modules::risk_control::AccountRiskState::Paused { .. }
            | buddy_switch_core::modules::risk_control::AccountRiskState::Cooling { .. }
    )
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
}
