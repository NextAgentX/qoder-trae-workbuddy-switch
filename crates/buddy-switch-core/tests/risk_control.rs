use buddy_switch_core::modules::risk_control::{
    AccountRiskState, RiskControlSettings, RiskOutcome, RiskScope, RiskStore,
};

fn scope(product: &str, region: &str) -> RiskScope {
    RiskScope::new(product, region).expect("valid product/region scope")
}

fn settings() -> RiskControlSettings {
    RiskControlSettings {
        enabled: true,
        requests_per_minute: 1,
        max_in_flight_per_account: 2,
        max_in_flight_global: 2,
        min_account_gap_ms: 0,
        max_transient_attempts: 1,
        fallback_cooldown_secs: 60,
    }
}

#[test]
fn invalid_limits_are_rejected_instead_of_becoming_unbounded() {
    let mut value = settings();
    value.max_in_flight_per_account = 0;
    assert!(value.validate().is_err());

    value = settings();
    value.requests_per_minute = 601;
    assert!(value.validate().is_err());
}

#[test]
fn rate_limit_is_atomic_under_concurrent_requests() {
    let dir = tempfile::tempdir().unwrap();
    let store = RiskStore::open(dir.path().join("risk.json")).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let store = store.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            store.begin_request_at(&scope("workbuddy", "cn"), "uid-1", &settings(), 1_000)
        }));
    }
    barrier.wait();
    let admitted = workers
        .into_iter()
        .map(|worker| worker.join().unwrap().is_ok())
        .filter(|admitted| *admitted)
        .count();
    assert_eq!(
        admitted, 1,
        "one request per minute must admit only one concurrent caller"
    );
}

#[test]
fn open_shared_keeps_runtime_limits_across_short_lived_handles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared-risk.json");
    let scope = scope("workbuddy", "cn");
    let first = RiskStore::open_shared(&path).unwrap();
    let permit = first
        .begin_request_at(&scope, "uid-shared", &settings(), 10_000)
        .unwrap();
    drop(permit);
    drop(first);

    let second = RiskStore::open_shared(&path).unwrap();
    assert!(second
        .begin_request_at(&scope, "uid-shared", &settings(), 10_001)
        .is_err());
}

#[test]
fn risk_state_is_isolated_by_product_and_region() {
    let dir = tempfile::tempdir().unwrap();
    let store = RiskStore::open(dir.path().join("risk.json")).unwrap();
    store
        .record_outcome_at(
            &scope("workbuddy", "cn"),
            "uid-1",
            RiskOutcome::Banned("account suspended".into()),
            None,
            10_000,
            60,
        )
        .unwrap();

    assert!(matches!(
        store.state(&scope("workbuddy", "cn"), "uid-1"),
        AccountRiskState::Paused { .. }
    ));
    assert_eq!(
        store.state(&scope("workbuddy", "global"), "uid-1"),
        AccountRiskState::Active
    );
    assert_eq!(
        store.state(&scope("trae", "cn"), "uid-1"),
        AccountRiskState::Active
    );
}

#[test]
fn rate_limit_cooldown_uses_server_reset_and_expires_at_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let store = RiskStore::open(dir.path().join("risk.json")).unwrap();
    store
        .record_outcome_at(
            &scope("qoder", "global"),
            "uid-2",
            RiskOutcome::RateLimited("429".into()),
            Some(20_000),
            10_000,
            60,
        )
        .unwrap();

    assert!(matches!(
        store.state_at(&scope("qoder", "global"), "uid-2", 19_999),
        AccountRiskState::Cooling {
            until_ms: 20_000,
            ..
        }
    ));
    assert_eq!(
        store.state_at(&scope("qoder", "global"), "uid-2", 20_000),
        AccountRiskState::Active
    );
}

#[test]
fn account_pause_survives_reload_and_requires_manual_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("risk.json");
    {
        let store = RiskStore::open(&path).unwrap();
        store
            .record_outcome_at(
                &scope("trae", "cn"),
                "uid-3",
                RiskOutcome::Banned("403 account disabled".into()),
                None,
                1_000,
                60,
            )
            .unwrap();
    }
    let store = RiskStore::open(&path).unwrap();
    assert!(matches!(
        store.state(&scope("trae", "cn"), "uid-3"),
        AccountRiskState::Paused { .. }
    ));
    assert!(store
        .begin_request_at(&scope("trae", "cn"), "uid-3", &settings(), 10_000)
        .is_err());

    store
        .recover_account(&scope("trae", "cn"), "uid-3")
        .unwrap();
    assert_eq!(
        store.state(&scope("trae", "cn"), "uid-3"),
        AccountRiskState::Active
    );
}

#[test]
fn malformed_risk_state_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("risk.json");
    std::fs::write(&path, "{broken").unwrap();
    assert!(RiskStore::open(path).is_err());
}
