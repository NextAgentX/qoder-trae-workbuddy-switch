//! Shared, persistent risk controls for authenticated platform requests.
//!
//! The store deliberately scopes state by product, region, and account. It does
//! not change account identity, select a different account, or route traffic.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

const MAX_RPM: u32 = 600;
const MAX_CONCURRENCY: u32 = 256;
const MAX_GAP_MS: u64 = 3_600_000;
const MAX_COOLDOWN_SECS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RiskControlSettings {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub max_in_flight_per_account: u32,
    pub max_in_flight_global: u32,
    pub min_account_gap_ms: u64,
    pub max_transient_attempts: u32,
    pub fallback_cooldown_secs: u64,
}

impl Default for RiskControlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 30,
            max_in_flight_per_account: 1,
            max_in_flight_global: 1,
            min_account_gap_ms: 5_000,
            max_transient_attempts: 1,
            fallback_cooldown_secs: 60,
        }
    }
}

impl RiskControlSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.requests_per_minute > MAX_RPM {
            return Err(format!("requests_per_minute must be 0..={MAX_RPM}"));
        }
        if !(1..=MAX_CONCURRENCY).contains(&self.max_in_flight_per_account) {
            return Err(format!(
                "max_in_flight_per_account must be 1..={MAX_CONCURRENCY}"
            ));
        }
        if !(1..=MAX_CONCURRENCY).contains(&self.max_in_flight_global) {
            return Err(format!(
                "max_in_flight_global must be 1..={MAX_CONCURRENCY}"
            ));
        }
        if self.min_account_gap_ms > MAX_GAP_MS {
            return Err(format!("min_account_gap_ms must be <= {MAX_GAP_MS}"));
        }
        if self.max_transient_attempts > 10 {
            return Err("max_transient_attempts must be 0..=10".into());
        }
        if !(1..=MAX_COOLDOWN_SECS).contains(&self.fallback_cooldown_secs) {
            return Err(format!(
                "fallback_cooldown_secs must be 1..={MAX_COOLDOWN_SECS}"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RiskScope {
    pub product: String,
    pub region: String,
}

impl RiskScope {
    pub fn new(product: impl Into<String>, region: impl Into<String>) -> Result<Self, String> {
        let product = product.into().trim().to_ascii_lowercase();
        let region = region.into().trim().to_ascii_lowercase();
        if product.is_empty() || region.is_empty() || product.len() > 64 || region.len() > 64 {
            return Err("product and region must contain 1..=64 characters".into());
        }
        if !product
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(
                "product and region may contain only ASCII letters, digits, '-' and '_'".into(),
            );
        }
        Ok(Self { product, region })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccountRiskState {
    Active,
    Cooling { until_ms: u64, reason: String },
    Paused { reason: String },
}

impl Default for AccountRiskState {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskOutcome {
    Banned(String),
    RateLimited(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct AccountKey {
    scope: RiskScope,
    account_id: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Persisted {
    states: Vec<(AccountKey, AccountRiskState)>,
}

#[derive(Default)]
struct Runtime {
    states: BTreeMap<AccountKey, AccountRiskState>,
    requests: BTreeMap<AccountKey, VecDeque<u64>>,
    in_flight: BTreeMap<AccountKey, u32>,
    global_in_flight: BTreeMap<RiskScope, u32>,
    last_admitted: BTreeMap<AccountKey, u64>,
}

struct Inner {
    path: PathBuf,
    runtime: Mutex<Runtime>,
}

#[derive(Clone)]
pub struct RiskStore {
    inner: Arc<Inner>,
}

impl RiskStore {
    /// Open one process-wide runtime per state path so gateway and background
    /// callers share atomic rate and concurrency accounting.
    pub fn open_shared(path: impl AsRef<Path>) -> Result<Self, String> {
        static STORES: OnceLock<Mutex<HashMap<PathBuf, Arc<Inner>>>> = OnceLock::new();
        let path = path.as_ref();
        let key = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| format!("resolve risk state path: {error}"))?
                .join(path)
        };
        let stores = STORES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut stores = stores
            .lock()
            .map_err(|_| "risk store registry lock poisoned".to_string())?;
        if let Some(inner) = stores.get(&key).cloned() {
            return Ok(Self { inner });
        }
        let store = Self::open(&key)?;
        stores.insert(key, store.inner.clone());
        Ok(store)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let states = if path.exists() {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("read risk state: {e}"))?;
            let persisted: Persisted =
                serde_json::from_str(&text).map_err(|e| format!("parse risk state: {e}"))?;
            persisted.states.into_iter().collect()
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                runtime: Mutex::new(Runtime {
                    states,
                    ..Runtime::default()
                }),
            }),
        })
    }

    pub fn state(&self, scope: &RiskScope, account_id: &str) -> AccountRiskState {
        self.state_at(scope, account_id, now_ms())
    }

    fn remaining_gap_ms(
        &self,
        scope: &RiskScope,
        account_id: &str,
        settings: &RiskControlSettings,
        now: u64,
    ) -> Result<u64, String> {
        let runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| "risk state lock poisoned".to_string())?;
        Ok(runtime
            .last_admitted
            .get(&key(scope, account_id))
            .map(|last| {
                settings
                    .min_account_gap_ms
                    .saturating_sub(now.saturating_sub(*last))
            })
            .unwrap_or(0))
    }

    pub fn state_at(&self, scope: &RiskScope, account_id: &str, now: u64) -> AccountRiskState {
        let Ok(mut runtime) = self.inner.runtime.lock() else {
            return AccountRiskState::Paused {
                reason: "risk state lock unavailable".into(),
            };
        };
        let key = key(scope, account_id);
        let state = runtime.states.get(&key).cloned().unwrap_or_default();
        if matches!(state, AccountRiskState::Cooling { until_ms, .. } if now >= until_ms) {
            runtime.states.remove(&key);
            if persist(&self.inner.path, &runtime.states).is_err() {
                let paused = AccountRiskState::Paused {
                    reason: "failed to persist cooldown expiry".into(),
                };
                runtime.states.insert(key, paused.clone());
                return paused;
            }
            AccountRiskState::Active
        } else {
            state
        }
    }

    pub fn begin_request_at(
        &self,
        scope: &RiskScope,
        account_id: &str,
        settings: &RiskControlSettings,
        now: u64,
    ) -> Result<RiskPermit, String> {
        settings.validate()?;
        if account_id.trim().is_empty() {
            return Err("account id is required".into());
        }
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| "risk state lock poisoned".to_string())?;
        let key = key(scope, account_id);
        let current = runtime.states.get(&key).cloned().unwrap_or_default();
        match current {
            AccountRiskState::Paused { reason } => {
                return Err(format!("account automation paused: {reason}"))
            }
            AccountRiskState::Cooling { until_ms, reason } if now < until_ms => {
                return Err(format!("account cooling until {until_ms}: {reason}"))
            }
            AccountRiskState::Cooling { .. } => {
                runtime.states.remove(&key);
                persist(&self.inner.path, &runtime.states)?;
            }
            AccountRiskState::Active => {}
        }
        let requests = runtime.requests.entry(key.clone()).or_default();
        while requests
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 60_000)
        {
            requests.pop_front();
        }
        if settings.requests_per_minute > 0
            && requests.len() >= settings.requests_per_minute as usize
        {
            return Err("request rate limit reached".into());
        }
        if runtime.in_flight.get(&key).copied().unwrap_or(0) >= settings.max_in_flight_per_account {
            return Err("per-account concurrency limit reached".into());
        }
        if runtime.global_in_flight.get(scope).copied().unwrap_or(0)
            >= settings.max_in_flight_global
        {
            return Err("global concurrency limit reached".into());
        }
        if runtime
            .last_admitted
            .get(&key)
            .is_some_and(|last| now.saturating_sub(*last) < settings.min_account_gap_ms)
        {
            return Err("minimum account request gap not elapsed".into());
        }
        runtime
            .requests
            .get_mut(&key)
            .expect("entry inserted")
            .push_back(now);
        runtime.last_admitted.insert(key.clone(), now);
        *runtime.in_flight.entry(key.clone()).or_default() += 1;
        *runtime.global_in_flight.entry(scope.clone()).or_default() += 1;
        Ok(RiskPermit {
            inner: Arc::downgrade(&self.inner),
            key: Some(key),
            scope: Some(scope.clone()),
        })
    }

    pub fn record_outcome_at(
        &self,
        scope: &RiskScope,
        account_id: &str,
        outcome: RiskOutcome,
        server_reset_ms: Option<u64>,
        now: u64,
        fallback_cooldown_secs: u64,
    ) -> Result<(), String> {
        let key = key(scope, account_id);
        let state = match outcome {
            RiskOutcome::Banned(reason) => AccountRiskState::Paused { reason },
            RiskOutcome::RateLimited(reason) => AccountRiskState::Cooling {
                until_ms: server_reset_ms.filter(|t| *t > now).unwrap_or_else(|| {
                    now.saturating_add(fallback_cooldown_secs.saturating_mul(1000))
                }),
                reason,
            },
        };
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| "risk state lock poisoned".to_string())?;
        let previous = runtime.states.insert(key.clone(), state);
        if let Err(error) = persist(&self.inner.path, &runtime.states) {
            match previous {
                Some(old) => {
                    runtime.states.insert(key, old);
                }
                None => {
                    runtime.states.remove(&key);
                }
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn recover_account(&self, scope: &RiskScope, account_id: &str) -> Result<(), String> {
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| "risk state lock poisoned".to_string())?;
        let key = key(scope, account_id);
        let previous = runtime.states.remove(&key);
        if let Err(error) = persist(&self.inner.path, &runtime.states) {
            if let Some(old) = previous {
                runtime.states.insert(key, old);
            }
            return Err(error);
        }
        Ok(())
    }
}

fn platform_config_path(product: &str) -> Result<PathBuf, String> {
    match product {
        "workbuddy" => Ok(crate::modules::region::gateway_config_file()),
        "trae" => Ok(crate::modules::trae::paths::api_gateway_file()),
        "qoder" => Ok(crate::modules::qoder::paths::gateway_config_file()),
        _ => return Err("unknown risk-control product".into()),
    }
}

fn shared_settings_path() -> PathBuf {
    crate::modules::config::store_dir().join("request_risk_settings.json")
}

/// Read the one application-wide request-risk configuration shared by all platforms and regions.
pub fn load_settings() -> Result<RiskControlSettings, String> {
    let path = shared_settings_path();
    if !path.exists() {
        return Ok(RiskControlSettings::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("read request risk settings: {error}"))?;
    let settings: RiskControlSettings = serde_json::from_str(&raw)
        .map_err(|error| format!("parse request risk settings: {error}"))?;
    settings.validate()?;
    Ok(settings)
}

/// Save and validate the one application-wide request-risk configuration.
pub fn save_settings(settings: &RiskControlSettings) -> Result<(), String> {
    settings.validate()?;
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("serialize request risk settings: {error}"))?;
    crate::modules::config::atomic_write(&shared_settings_path(), &raw)
        .map_err(|error| format!("write request risk settings: {error}"))
}

/// Read the shared settings after validating the requested product/region scope.
pub fn platform_risk_settings(product: &str, region: &str) -> Result<RiskControlSettings, String> {
    platform_config_path(product)?;
    if !matches!(region, "cn" | "global") {
        return Err("unknown risk-control region".into());
    }
    load_settings()
}

/// Core authenticated request entry point used outside the gateway relay.
/// It reads the same per-region settings saved by each platform gateway.
fn try_admit_platform_request(
    product: &str,
    region: &str,
    account_id: &str,
) -> Result<RiskPermit, String> {
    let config_path = platform_config_path(product)?;
    let settings = platform_risk_settings(product, region)?;
    if !settings.enabled {
        return Ok(RiskPermit::untracked());
    }
    let scope = RiskScope::new(product, region)?;
    RiskStore::open_shared(config_path.with_file_name("risk_control.json"))?.begin_request_at(
        &scope,
        account_id,
        &settings,
        now_ms().max(0) as u64,
    )
}

/// Background workflows may make several intentional requests for one account
/// (for example, list then claim). Wait out only the configured spacing window;
/// all other admission failures remain immediate and fail closed.
pub async fn admit_platform_request(
    product: &str,
    region: &str,
    account_id: &str,
) -> Result<RiskPermit, String> {
    let settings = platform_risk_settings(product, region)?;
    if !settings.enabled {
        return Ok(RiskPermit::untracked());
    }
    let config_path = platform_config_path(product)?;
    let scope = RiskScope::new(product, region)?;
    let store = RiskStore::open_shared(config_path.with_file_name("risk_control.json"))?;
    loop {
        match try_admit_platform_request(product, region, account_id) {
            Ok(permit) => return Ok(permit),
            Err(error) if error == "minimum account request gap not elapsed" => {
                let now = now_ms().max(0) as u64;
                let delay = store.remaining_gap_ms(&scope, account_id, &settings, now)?;
                tokio::time::sleep(std::time::Duration::from_millis(delay.max(1))).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Persist the small set of platform response outcomes that can be identified
/// from the normalized HTTP helper response shape.
pub fn record_platform_response(
    product: &str,
    region: &str,
    account_id: &str,
    code: i64,
    reason: String,
) -> Result<(), String> {
    if !platform_risk_settings(product, region)?.enabled {
        return Ok(());
    }
    let state_path = match product {
        "workbuddy" => {
            crate::modules::region::gateway_config_file().with_file_name("risk_control.json")
        }
        "trae" => {
            crate::modules::trae::paths::api_gateway_file().with_file_name("risk_control.json")
        }
        "qoder" => {
            crate::modules::qoder::paths::gateway_config_file().with_file_name("risk_control.json")
        }
        _ => return Err("unknown risk-control product".into()),
    };
    let store = RiskStore::open_shared(state_path)?;
    let scope = RiskScope::new(product, region)?;
    let now = now_ms().max(0) as u64;
    match code {
        401 | 403 => store.record_outcome_at(
            &scope,
            account_id,
            RiskOutcome::Banned(reason),
            None,
            now,
            60,
        ),
        429 => {
            let cooldown = load_settings()?.fallback_cooldown_secs;
            store.record_outcome_at(
                &scope,
                account_id,
                RiskOutcome::RateLimited(reason),
                None,
                now,
                cooldown,
            )
        }
        _ => Ok(()),
    }
}

pub struct RiskPermit {
    inner: Weak<Inner>,
    key: Option<AccountKey>,
    scope: Option<RiskScope>,
}

impl RiskPermit {
    /// Represents a request made while the optional risk controls are disabled.
    pub fn untracked() -> Self {
        Self {
            inner: Weak::new(),
            key: None,
            scope: None,
        }
    }
}

impl Drop for RiskPermit {
    fn drop(&mut self) {
        if let (Some(key), Some(scope)) = (&self.key, &self.scope) {
            if let Some(inner) = self.inner.upgrade() {
                if let Ok(mut runtime) = inner.runtime.lock() {
                    decrement(&mut runtime.in_flight, key);
                    decrement(&mut runtime.global_in_flight, scope);
                }
            }
        }
    }
}

fn decrement<K: Ord>(map: &mut BTreeMap<K, u32>, key: &K) {
    if let Some(value) = map.get_mut(key) {
        *value = value.saturating_sub(1);
        if *value == 0 {
            map.remove(key);
        }
    }
}

fn key(scope: &RiskScope, account_id: &str) -> AccountKey {
    AccountKey {
        scope: scope.clone(),
        account_id: account_id.to_owned(),
    }
}

fn persist(path: &Path, states: &BTreeMap<AccountKey, AccountRiskState>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create risk state directory: {e}"))?;
    }
    let content = serde_json::to_string(&Persisted {
        states: states
            .iter()
            .map(|(key, state)| (key.clone(), state.clone()))
            .collect(),
    })
    .map_err(|e| format!("serialize risk state: {e}"))?;
    crate::modules::config::atomic_write(path, &content)
        .map_err(|e| format!("write risk state: {e}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
