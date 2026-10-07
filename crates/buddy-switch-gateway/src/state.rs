//! 网关配置（持久化到 `~/.buddy-switch/gateway_config.json`）与可 Arc 共享的运行状态。
//!
//! ## 已移除的配置位（保留设计意图，避免后人重复踩坑）
//!
//! **`dual_port`（2026-09-28 移除）** —— 「按 region 分别监听独立端口」的配置位。
//! 它长期**只有字段、没有任何读取方**：设置页曾据此渲染一个开关，但打开与否对监听行为
//! 毫无影响（典型的假控件），UI 早已摘掉，只剩字段本身。保留的两条理由都不成立：
//! ① 「契约测试钉死键集合」是我们自己维护的测试，不是外部约束；② 「老配置文件里可能有该键」
//! —— serde 默认忽略未知字段，删字段不会让旧配置加载失败。故按「不保留死代码」整条移除。
//!
//! 若日后确要实现该能力，需要一并补：第二端口的配置位、监听生命周期、按 region 的路由分发。
//! 那时应按新需求重新设计，**不要只是把字段加回来**（加回来又是一个空开关）。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use buddy_switch_core::modules::account;
use buddy_switch_core::modules::catalog::CatalogStore;
use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::{self, Region};
use buddy_switch_core::modules::upstream::UpstreamClient;

use crate::account_strategy::{load_strategies, AccountStrategy};
use crate::apikey::ApiKeyStore;
use crate::disabled_models::DisabledModels;
use crate::logging::RequestLog;
use crate::outbound::{DegradeGate, PromptSettings};
use crate::pool::{Pool, PoolConfig, PoolSummary, RealmTag};
use crate::sticky::StickyTable;

/// 网关运行配置。
///
/// 字段名为 snake_case（对齐设计文档 A-3.4），同时接受常见 camelCase 别名，
/// 以容忍前端不同写法；缺失字段回落到 [`Default`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// 是否启用独立监听（默认 false）。合并进宿主的 `/v1/*` 路由不受该开关影响。
    pub enabled: bool,
    /// 监听地址，默认 `127.0.0.1`。
    #[serde(alias = "bindAddr")]
    pub bind_addr: String,
    /// 监听端口，默认 57891。
    pub port: u16,
    /// 是否允许非回环监听（默认 false）。
    #[serde(alias = "allowNonLoopback")]
    pub allow_non_loopback: bool,
    /// 请求日志保留条数，默认 200。
    #[serde(alias = "logKeep")]
    pub log_keep: usize,
    /// 是否记录 prompt/response 正文（默认 false，Q5）。
    #[serde(alias = "logBodies")]
    pub log_bodies: bool,
    /// 每 Key 限流（P1，默认 None=不限流）。
    #[serde(alias = "perKeyRateLimit")]
    pub per_key_rate_limit: Option<u32>,
    /// 单轮请求的最大换号次数（默认 3）。
    #[serde(alias = "maxRotate")]
    pub max_rotate: usize,
    /// 是否启用出站指纹脱敏（默认 true）。
    #[serde(alias = "sanitizeFingerprints")]
    pub sanitize_fingerprints: bool,
    /// 系统提示词模式：`passthrough`（默认）或 `custom`。
    #[serde(alias = "promptMode")]
    pub prompt_mode: String,
    /// `custom` 模式下的提示词文件路径；缺省用内置默认提示词。
    #[serde(alias = "promptFile")]
    pub prompt_file: Option<String>,
    /// 会话粘性绑定有效期（毫秒，默认 30 分钟）。
    #[serde(alias = "stickyTtlMs")]
    pub sticky_ttl_ms: i64,
    /// 账号池治理配置。
    pub pool: PoolConfig,
    /// 是否允许用模型名前缀（`cn:` / `global:`）**跨域**路由（默认 `false`）。
    ///
    /// 默认关闭是**有意的安全取舍**：API Key 与其 region 强绑定，客户端不应能自行把
    /// 请求导向另一个域。关闭时同域前缀仍会被剥离（纯归一化），跨域前缀返回可诊断的
    /// 400 而不是静默改名。详见 [`crate::model_route`]。
    #[serde(alias = "allowModelRegionPrefix")]
    pub allow_model_region_prefix: bool,
    /// 请求体大小上限（MB，默认 8；`0` 视为未设置并回落默认）。
    ///
    /// 为什么需要它：axum 的 `DefaultBodyLimit` **默认只有 2MB**，大上下文（长代码文件、
    /// 长对话）会撞上一个**裸 413**，用户几乎无法自行定位原因。参考实现用
    /// `server.max_body_mb`（默认 8）。
    ///
    /// **生效边界**：axum 的限制在 **router 构建期**固化，因此修改本项后**需重启网关**
    /// （不能像其它配置那样热生效）。
    #[serde(alias = "maxBodyMb")]
    pub max_body_mb: usize,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "127.0.0.1".to_string(),
            port: 57891,
            allow_non_loopback: false,
            log_keep: 200,
            log_bodies: false,
            per_key_rate_limit: None,
            max_rotate: 3,
            sanitize_fingerprints: true,
            prompt_mode: "passthrough".to_string(),
            prompt_file: None,
            sticky_ttl_ms: 30 * 60 * 1000,
            pool: PoolConfig::default(),
            allow_model_region_prefix: false,
            max_body_mb: DEFAULT_MAX_BODY_MB,
        }
    }
}

impl GatewayConfig {
    /// 从 `~/.buddy-switch/gateway_config.json` 读取；缺失/损坏回落默认值。
    pub fn load() -> Self {
        let file = region::gateway_config_file();
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Ok(config) = serde_json::from_str::<GatewayConfig>(&text) {
                return config;
            }
        }
        GatewayConfig::default()
    }

    /// 原子写回配置文件。
    pub fn save(&self) -> Result<(), String> {
        let file = region::gateway_config_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &content).map_err(|error| error.to_string())
    }

    /// 对外 Base URL（供接入指引展示）。
    pub fn base_url(&self) -> String {
        let host = if self.bind_addr.trim().is_empty() {
            "127.0.0.1"
        } else {
            self.bind_addr.trim()
        };
        format!("http://{host}:{}", self.port)
    }
}

/// 网关运行状态的**对外响应契约**（`GET /api/gateway/status`，见 A-3.7）。
///
/// 字段命名统一为 **snake_case**，与 [`GatewayConfig`] 的序列化风格一致，避免同一
/// 管理面里两套命名风格并存导致前端契约漂移（此前该响应由 `json!{}` 手工拼
/// camelCase，与配置的 snake_case 不一致）。
///
/// `running` / `addr` / `version` 来自运行时而非持久化配置，故 [`From<&GatewayConfig>`]
/// 只填充配置派生字段，这三项由调用方补充。
//
// ⚠️ 只 derive `PartialEq`（**不含 `Eq`**）：池视图带 `f64`（`total_credits`）与
// `serde_json::Value`（`accounts`），两者都没有 `Eq`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayStatusView {
    /// 配置中的启用开关（与 [`GatewayConfig::enabled`] 同步）。
    pub enabled: bool,
    /// 网关进程是否实际在监听独立端口。
    pub running: bool,
    /// 实际监听地址；未监听时为 `None`。
    pub addr: Option<String>,
    /// 对外 Base URL（含协议与端口，供接入指引展示）。
    pub base_url: String,
    /// 配置中的监听地址。
    pub bind_addr: String,
    /// 配置中的监听端口。
    pub port: u16,
    /// 是否允许非回环（局域网）监听。
    pub allow_non_loopback: bool,
    /// 应用版本号。
    pub version: String,
    /// 账号池五态计数，**按域分家**（键恒为 `cn` / `global`）。
    ///
    /// 形状与 Trae 侧 `TraeGatewayStatusView::pool` 同源（同一张管理面卡片读它）。
    pub pools: BTreeMap<String, PoolSummary>,
    /// 逐账号状态（含 `realm`，前端按域过滤）。键名与 Trae 侧 `accounts` 一致。
    pub accounts: Vec<serde_json::Value>,
    /// 逐账号「此刻为什么（不）能路由」，供排查「没有可用账号」。
    pub diagnose: Vec<String>,
    /// **最近一次请求失败**的可读原因（`None` = 本进程内还没失败过）。
    ///
    /// 与 Trae 侧 `TraeGatewayStatusView::last_error` 同名同义，管理面共用同一块展示。
    pub last_error: Option<String>,
}

impl Default for GatewayStatusView {
    fn default() -> Self {
        Self {
            enabled: false,
            running: false,
            addr: None,
            base_url: String::new(),
            bind_addr: String::new(),
            port: 0,
            allow_non_loopback: false,
            version: String::new(),
            pools: default_pool_summaries(),
            accounts: Vec::new(),
            diagnose: Vec::new(),
            last_error: None,
        }
    }
}

/// `pools` 的缺省值：**两个域的键恒定存在**（该域没有账号时计数全为 0）。
///
/// 让前端不必处理键缺失 —— 「CN 版一个号都没有」是正常状态，不该显示成「未知」。
pub fn default_pool_summaries() -> BTreeMap<String, PoolSummary> {
    BTreeMap::from([
        ("cn".to_string(), PoolSummary::default()),
        ("global".to_string(), PoolSummary::default()),
    ])
}

impl From<&GatewayConfig> for GatewayStatusView {
    /// 由配置派生响应视图；`running` / `addr` / `version` / 池视图需由调用方补充。
    fn from(config: &GatewayConfig) -> Self {
        Self {
            enabled: config.enabled,
            running: false,
            addr: None,
            base_url: config.base_url(),
            bind_addr: config.bind_addr.clone(),
            port: config.port,
            allow_non_loopback: config.allow_non_loopback,
            version: String::new(),
            pools: default_pool_summaries(),
            accounts: Vec::new(),
            diagnose: Vec::new(),
            last_error: None,
        }
    }
}

impl GatewayStatusView {
    /// 填入账号池视图（`pools` / `accounts` / `diagnose`）；**只读**，不改动池。
    ///
    /// 由两侧宿主（server / 桌面端）在 [`sync_pool_with_accounts`] 之后调用。
    /// 之所以不在这里自己取锁：桌面端那条命令是**同步**的，只能 `try_write`。
    ///
    /// ★ 保持只读是**有意的**：本函数被单测直接覆盖，一旦它自己去读账号库
    /// （无入参的全局路径函数），用例就会被**本机真实账号库**污染（踩过）。
    pub fn fill_pool(&mut self, pool: &Pool, now_ms: i64) {
        self.pools = BTreeMap::from([
            ("cn".to_string(), pool.summary(now_ms, Some(RealmTag::Cn))),
            (
                "global".to_string(),
                pool.summary(now_ms, Some(RealmTag::Global)),
            ),
        ]);
        self.accounts = pool.status_list(now_ms);
        self.diagnose = pool.diagnose(now_ms);
    }
}

/// 用**账号库**对齐池（增量 upsert，不删条目、不动治理状态）。
///
/// ★ 为什么管理面读状态前必须先对齐一次：池条目平时只在有请求经过
/// （[`crate::routes::relay::relay`]）时才同步，于是「刚导入账号、还没发过请求」
/// 时池是空的 —— 用户打开管理页会看到「账号池为空」，而账号管理页明明列着好几个号。
///
/// 与请求路径同款（同一函数），因此不会引入第二套对齐语义。
pub fn sync_pool_with_accounts(pool: &mut Pool) {
    for (region, realm) in [
        (Region::Cn, RealmTag::Cn),
        (Region::Global, RealmTag::Global),
    ] {
        pool.sync_accounts(&account::load_accounts_for(region), Some(realm));
    }
}

/// 请求日志文件路径。
pub fn gateway_log_file() -> PathBuf {
    region::gateway_config_file().with_file_name("gateway_logs.json")
}

/// 账号池状态文件路径（`state.json`）。
pub fn gateway_state_file() -> PathBuf {
    region::gateway_config_file().with_file_name("state.json")
}

/// 请求体上限的默认值（MB）。与参考实现的 `server.max_body_mb` 默认一致。
pub const DEFAULT_MAX_BODY_MB: usize = 8;

/// 单请求体上限（MB）→ 字节数；`0` 视为未设置并回落默认。
///
/// 独立成纯函数，是为了让「配置 → 字节」这一步可被单测直接覆盖（含 `0` 与极大值边界），
/// 而不必构造整个 [`GatewayState`]。
pub fn body_limit_bytes(max_body_mb: usize) -> usize {
    let mb = if max_body_mb == 0 {
        DEFAULT_MAX_BODY_MB
    } else {
        max_body_mb
    };
    mb.saturating_mul(1024 * 1024)
}

/// 可 Arc 共享的网关状态。
#[derive(Clone)]
pub struct GatewayState {
    pub config: Arc<RwLock<GatewayConfig>>,
    pub keys: Arc<ApiKeyStore>,
    pub catalogs: Arc<CatalogStore>,
    pub strategies: Arc<RwLock<HashMap<Region, AccountStrategy>>>,
    /// 用户禁用的模型（P11）：**两个出口共用**（`/v1/models` 过滤 + 调用拒绝）。
    ///
    /// 进程内**唯一**实例（[`crate::disabled_models::shared`]）—— 与 Trae 网关、命令面
    /// 读的是同一个 `Arc`，否则「命令改了一份、网关读另一份」就是假开关。
    pub disabled: Arc<RwLock<DisabledModels>>,
    pub upstream: Arc<UpstreamClient>,
    pub log: Arc<RequestLog>,
    pub started_at: i64,
    /// 账号池（多账号治理：选号 / 冷却 / 熔断 / 账本）。
    pub pool: Arc<RwLock<Pool>>,
    /// 生效中的系统提示词设置。
    pub prompt: Arc<RwLock<PromptSettings>>,
    /// 内容拦截降级门。
    pub degrade: Arc<RwLock<DegradeGate>>,
    /// 会话粘性绑定表。
    pub sticky: Arc<RwLock<StickyTable>>,
    /// 提示词加载失败原因（`None` 表示加载正常）；用于 `/status` 透出，
    /// 避免「配置写错但静默回落 passthrough」这种无声失效。
    pub prompt_error: Option<String>,
    /// **最近一次请求失败**的可读原因（`None` = 本次进程内还没失败过）。
    ///
    /// 由 [`crate::routes::relay::relay`] 在失败出口写入（一处覆盖全部失败路径），
    /// 管理面「网关卡」直接展示 —— 用户不必去翻请求日志才知道刚才为什么 502。
    /// **成功不清空**（与 Trae 侧同语义）：残留一条历史失败比「什么都没显示」更利于排查。
    pub last_error: Arc<RwLock<Option<String>>>,
    /// 请求体上限的**字节数**（由配置在构造时固化）。
    ///
    /// 之所以在构造时算一次而不是每次读配置：axum 的 `DefaultBodyLimit` 是 **layer**，
    /// 只在 router 构建期生效，因此本项天然是**启动期配置**。
    pub body_limit_bytes: usize,
}

impl GatewayState {
    /// 依据配置构造一份完整运行状态。
    pub fn new(config: GatewayConfig) -> Self {
        let keys = Arc::new(ApiKeyStore::new(region::gateway_keys_file()));
        let catalogs = Arc::new(CatalogStore::new());
        let log = Arc::new(RequestLog::new(
            gateway_log_file(),
            config.log_keep,
            config.log_bodies,
        ));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let upstream = Arc::new(UpstreamClient::new(http));
        let strategies = Arc::new(RwLock::new(load_strategies()));

        // 提示词：配置错误不 panic（桌面端不能因为一个配置项启动失败），
        // 但必须把原因留在 `prompt_error` 里供 `/status` 透出。
        let (prompt, prompt_error) = match PromptMode_::parse(&config.prompt_mode) {
            Ok(mode) => {
                let file = config.prompt_file.as_deref().map(std::path::Path::new);
                match PromptSettings::load(mode, file) {
                    Ok(settings) => (settings, None),
                    Err(error) => (
                        PromptSettings::default(),
                        Some(format!("{}；已回落 passthrough", error)),
                    ),
                }
            }
            Err(error) => (
                PromptSettings::default(),
                Some(format!("{}；已回落 passthrough", error)),
            ),
        };

        let mut pool = Pool::new(config.pool.clone());
        let state_file = gateway_state_file();
        pool.load(&state_file, core_config::now_ms());

        Self {
            config: Arc::new(RwLock::new(config.clone())),
            keys,
            catalogs,
            strategies,
            disabled: crate::disabled_models::shared(),
            upstream,
            log,
            started_at: core_config::now_ms(),
            pool: Arc::new(RwLock::new(pool)),
            prompt: Arc::new(RwLock::new(prompt)),
            degrade: Arc::new(RwLock::new(DegradeGate::new())),
            sticky: Arc::new(RwLock::new(StickyTable::new(config.sticky_ttl_ms))),
            prompt_error,
            last_error: Arc::new(RwLock::new(None)),
            body_limit_bytes: body_limit_bytes(config.max_body_mb),
        }
    }

    /// 记下「最近一次请求失败」的可读原因（管理面 `last_error`）。
    pub async fn note_last_error(&self, message: impl Into<String>) {
        *self.last_error.write().await = Some(message.into());
    }

    /// 读取配置快照。
    pub async fn config_snapshot(&self) -> GatewayConfig {
        self.config.read().await.clone()
    }

    /// 某 region 当前生效的账号策略（缺省 `current`）。
    pub async fn strategy_for(&self, region: Region) -> AccountStrategy {
        self.strategies
            .read()
            .await
            .get(&region)
            .cloned()
            .unwrap_or_default()
    }

    /// 当前是否处于内容拦截降级期。
    pub async fn degrade_active(&self, now_ms: i64) -> bool {
        self.degrade.read().await.active(now_ms)
    }

    /// 尝试开启降级期；返回 `true` 表示本次开启了（已在期内则 `false`，不续期）。
    pub async fn trip_degrade(&self, now_ms: i64) -> bool {
        self.degrade.write().await.trigger(now_ms)
    }

    /// 本次请求的出站改写选项（配置 + 生效提示词）。
    pub async fn outbound_options(&self) -> crate::outbound::OutboundOptions {
        let config = self.config_snapshot().await;
        crate::outbound::OutboundOptions {
            sanitize_fingerprints: config.sanitize_fingerprints,
            prompt: self.prompt.read().await.clone(),
        }
    }

    /// 把池状态落盘（失败只记录，不影响请求链路）。
    pub async fn persist_pool(&self) {
        let mut pool = self.pool.write().await;
        if let Err(error) = pool.flush_if_dirty(&gateway_state_file()) {
            eprintln!("[gateway] 账号池状态落盘失败: {error}");
        }
    }
}

/// 仅供 `PromptMode` 解析引用，避免在 `state.rs` 顶层额外引入类型名。
use crate::outbound::PromptMode as PromptMode_;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn keys_of(value: &serde_json::Value) -> BTreeSet<&str> {
        value
            .as_object()
            .expect("serialized value must be an object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn gateway_status_view_serializes_with_pinned_snake_case_contract() {
        let config = GatewayConfig {
            enabled: true,
            bind_addr: "0.0.0.0".to_string(),
            port: 12345,
            allow_non_loopback: true,
            ..GatewayConfig::default()
        };
        let mut view = GatewayStatusView::from(&config);
        view.running = true;
        view.addr = Some("0.0.0.0:12345".to_string());
        view.version = "9.9.9".to_string();

        let value = serde_json::to_value(&view).expect("serialize status view");
        let expected: BTreeSet<&str> = [
            "enabled",
            "running",
            "addr",
            "base_url",
            "bind_addr",
            "port",
            "allow_non_loopback",
            "version",
            // 账号池视图（与 Trae 侧 `trae_gateway_status` 的 pool/accounts/diagnose 对位）。
            "pools",
            "accounts",
            "diagnose",
            "last_error",
        ]
        .into_iter()
        .collect();
        // 精确匹配：多一个少一个都会失败，防止契约再次漂移。
        assert_eq!(keys_of(&value), expected, "gateway_status key set must be pinned");

        // 「某域一个账号都没有」是正常状态，`pools` 的两个键必须**恒定存在**，
        // 否则前端会把「空池」显示成「未知」。
        let pools = value["pools"].as_object().expect("pools 对象");
        assert_eq!(keys_of(&value["pools"]), BTreeSet::from(["cn", "global"]));
        assert_eq!(pools["cn"]["total"], 0);

        // 不得再出现 camelCase 键（此前手工拼装的风格）。
        assert!(!value.as_object().unwrap().contains_key("baseUrl"));
        assert!(!value.as_object().unwrap().contains_key("bindAddr"));
        assert!(!value.as_object().unwrap().contains_key("allowNonLoopback"));

        // 值必须来自配置 / 运行时。
        assert_eq!(value["enabled"], true);
        assert_eq!(value["running"], true);
        assert_eq!(value["addr"], "0.0.0.0:12345");
        assert_eq!(value["base_url"], "http://0.0.0.0:12345");
        assert_eq!(value["bind_addr"], "0.0.0.0");
        assert_eq!(value["port"], 12345);
        assert_eq!(value["allow_non_loopback"], true);
        assert_eq!(value["version"], "9.9.9");
    }

    #[test]
    fn gateway_status_view_from_config_populates_config_derived_fields() {
        let config = GatewayConfig {
            enabled: true,
            bind_addr: "127.0.0.1".to_string(),
            port: 57891,
            allow_non_loopback: false,
            ..GatewayConfig::default()
        };
        let view = GatewayStatusView::from(&config);
        assert_eq!(view.base_url, "http://127.0.0.1:57891");
        assert_eq!(view.bind_addr, "127.0.0.1");
        assert_eq!(view.port, 57891);
        assert!(view.enabled);
        assert!(!view.allow_non_loopback);
        // 运行时字段保持默认，由调用方补充。
        assert!(!view.running);
        assert_eq!(view.addr, None);
        assert_eq!(view.version, "");
    }

    /// 池视图由 [`GatewayStatusView::fill_pool`] 填入：`pools` 按域分家、`accounts`
    /// 逐账号带 `realm`、`diagnose` 逐账号一行。
    ///
    /// ⚠️ 这里**只测 `fill_pool`（纯读）**：`sync_pool_with_accounts` 会读本机账号库
    /// （无入参的全局路径函数），在本进程并行跑的 lib 单测里必然读到**真实数据**，
    /// 断言只能写成「≥ 0 条」这种不痛不痒的形式 —— 那还不如不写。
    #[test]
    fn fill_pool_splits_counts_by_realm_and_tags_every_account() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("cn-1", Some(RealmTag::Cn), "CN 一号");
        pool.upsert("g-1", Some(RealmTag::Global), "国际一号");
        pool.set_credits("cn-1", 7, 0);
        pool.mark_credits_refreshed("cn-1", 1_000);

        let mut view = GatewayStatusView::default();
        view.fill_pool(&pool, 1_000);

        assert_eq!(view.pools["cn"].total, 1);
        assert_eq!(view.pools["global"].total, 1);
        assert_eq!(view.pools["cn"].total_credits, 7.0);
        assert_eq!(view.accounts.len(), 2, "两个域的账号都给出，由前端按 realm 过滤");
        assert_eq!(view.diagnose.len(), 2);

        let realms: BTreeSet<&str> = view
            .accounts
            .iter()
            .map(|item| item["realm"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(realms, BTreeSet::from(["cn", "global"]));
    }

    #[test]
    fn gateway_config_serializes_with_snake_case_keys() {
        let value = serde_json::to_value(GatewayConfig::default()).expect("serialize config");
        let expected: BTreeSet<&str> = [
            "enabled",
            "bind_addr",
            "port",
            "allow_non_loopback",
            "log_keep",
            "log_bodies",
            "per_key_rate_limit",
            "max_rotate",
            "sanitize_fingerprints",
            "prompt_mode",
            "prompt_file",
            "sticky_ttl_ms",
            "pool",
            "allow_model_region_prefix",
            "max_body_mb",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys_of(&value), expected);
    }

    #[test]
    fn gateway_config_deserializes_missing_fields_to_default() {
        let config: GatewayConfig = serde_json::from_str("{}").expect("empty config must parse");
        let default = GatewayConfig::default();
        assert_eq!(config.enabled, default.enabled);
        assert_eq!(config.bind_addr, default.bind_addr);
        assert_eq!(config.port, default.port);
        assert_eq!(config.allow_non_loopback, default.allow_non_loopback);
        assert_eq!(config.log_keep, default.log_keep);
        assert_eq!(config.log_bodies, default.log_bodies);
        assert_eq!(config.per_key_rate_limit, default.per_key_rate_limit);
    }

    #[test]
    fn gateway_config_accepts_camel_case_aliases() {
        let config: GatewayConfig = serde_json::from_str(
            r#"{
                "enabled": true,
                "bindAddr": "0.0.0.0",
                "port": 60000,
                "allowNonLoopback": true,
                "logKeep": 10,
                "logBodies": true,
                "perKeyRateLimit": 5
            }"#,
        )
        .expect("camelCase aliases must deserialize");
        assert!(config.enabled);
        assert_eq!(config.bind_addr, "0.0.0.0");
        assert_eq!(config.port, 60000);
        assert!(config.allow_non_loopback);
        assert_eq!(config.log_keep, 10);
        assert!(config.log_bodies);
        assert_eq!(config.per_key_rate_limit, Some(5));
    }

    /// 移除 `dual_port` 后，**老配置文件仍必须能加载**。
    ///
    /// 这是「删字段会不会让升级用户炸掉」的直接证据：serde 默认忽略未知字段，
    /// 因此老配置里的 `dualPort` / `dual_port` 只是被丢掉，不会反序列化失败。
    /// 若有人给 `GatewayConfig` 加上 `deny_unknown_fields`，本用例立刻变红。
    #[test]
    fn legacy_dual_port_key_is_ignored_not_rejected() {
        for body in [
            r#"{"port":60000,"dualPort":true}"#,
            r#"{"port":60000,"dual_port":true}"#,
        ] {
            let config: GatewayConfig =
                serde_json::from_str(body).unwrap_or_else(|e| panic!("老配置必须能加载 {body}: {e}"));
            assert_eq!(config.port, 60000, "其余字段不受影响");
        }
        // 序列化出口也不得再出现该键（契约测试另行钉死键集合）
        let value = serde_json::to_value(GatewayConfig::default()).expect("serialize config");
        assert!(
            value.get("dual_port").is_none(),
            "已移除的字段不得重新出现在序列化出口"
        );
    }

    #[test]
    fn gateway_config_round_trips_through_snake_case_json() {
        let config = GatewayConfig {
            enabled: true,
            bind_addr: "127.0.0.1".to_string(),
            port: 1234,
            log_keep: 7,
            per_key_rate_limit: Some(3),
            ..GatewayConfig::default()
        };
        let text = serde_json::to_string(&config).expect("serialize");
        let back: GatewayConfig = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.enabled, config.enabled);
        assert_eq!(back.bind_addr, config.bind_addr);
        assert_eq!(back.port, config.port);
        assert_eq!(back.log_keep, config.log_keep);
        assert_eq!(back.per_key_rate_limit, config.per_key_rate_limit);
    }
}
