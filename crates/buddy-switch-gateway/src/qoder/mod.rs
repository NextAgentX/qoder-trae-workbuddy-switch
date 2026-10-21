//! Qoder 的 OpenAI / Anthropic 兼容网关（**平行第三套产品线**）。
//!
//! ## 为什么又是独立一套（而不是复用 Trae 的）
//!
//! 与 Trae 侧的理由相同：归属域（区域而非程序位）、凭据形态（COSY 签名而非 JWT）、
//! 上游协议（信封 SSE）、响应形状（内层已是 OpenAI chunk）各不相同。共用的是
//! **更底层的构件**：
//!
//! - 选号算法 [`crate::pool::pick_with_preference`]（条目实现 [`pool::QoderPoolEntry`]）；
//! - 会话粘性 [`crate::StickyTable`]、请求日志 [`crate::logging::RequestLog`]；
//! - 协议转换 [`crate::protocol::anthropic`]（Anthropic ↔ OpenAI 双向）、
//!   [`crate::protocol::responses`]（Responses API ↔ OpenAI chat 双向）；
//! - 禁用偏好 [`crate::disabled_models`]（键 = **作用域**：`qoder-cn` / `qoder-global`）。
//!
//! 协议实现（COSY 签名 / 请求体构造 / 信封解包）在
//! [`buddy_switch_core::modules::qoder`]，本模块只做网关装配。
//!
//! ## ⚠️ 独立端口，绝不 merge router
//!
//! 本产品线的 `/v1/models` / `/v1/chat/completions` / `/v1/messages` 与 WB / Trae
//! **逐字重叠** —— axum 同路径 merge 会在**运行期 panic**。三个产品线各持 router、
//! 各自端口（WB 57891 / Trae 57894 / **Qoder 57895** / 统一 57892），由
//! [`crate::GatewayProduct::OWNED_PATHS`] 显式声明所有权。
//!
//! ## 真机状态
//!
//! 本机无 Qoder 账号 ⇒ 上游链路（COSY 签名 / 信封流）**未经真机验证**：所有
//! 断言都是协议级单测 + mock 上游 e2e（`state.upstream_override` 是测试挂载点）。
//! 首次真机跑通前，不要把「网关可用」当作已有事实。

pub mod apikey;
pub mod payload;
pub mod pool;
pub mod routes;
pub mod sse;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::qoder::region::{self as qoder_region, QoderRegion};
use buddy_switch_core::modules::qoder::{account, api, cosy, paths};

use crate::logging::RequestLog;
use crate::{body_limit_bytes, GatewayProduct};

/// Qoder 网关默认端口。
///
/// ★ 2026-10-17 由 `7865` 改为 `57895`：四条网关统一落在 `5789x` 一段
/// （界面 57890 · WB 57891 · 统一 57892 · hook 57893 · Trae 57894 · **Qoder 57895**）。
/// 只改默认值：已落盘的 `qoder/gateway_config.json` 里写着 `port` 的保持原端口。
pub const QODER_DEFAULT_API_PORT: u16 = 57895;

/// 模型清单缓存有效期（毫秒；10 分钟）。
const CATALOG_TTL_MS: i64 = 10 * 60 * 1000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 该区域的官方候选主机（国际版 api1→api2→api3）。
fn qoder_region_candidates(region: QoderRegion) -> Vec<&'static str> {
    qoder_region::gateway_candidates(region)
}

/// 出站 HTTP 客户端。
///
/// 与 Trae 侧的构造**同款**（刻意各持一份，见 `trae/mod.rs` 的同名函数）：
/// 只设 `connect_timeout` 与 `read_timeout`，**不设总超时** —— SSE 对话的合法时长
/// 由模型输出长度决定，加总超时会把正常的长回答掐断。
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(300))
        .pool_max_idle_per_host(20)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn default_true() -> bool {
    true
}

/// Qoder 网关配置（磁盘：`qoder/gateway_config.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QoderGatewayConfig {
    /// 是否启用独立监听。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 监听地址。
    #[serde(alias = "bindAddr")]
    pub bind_addr: String,
    /// 监听端口，默认 [`QODER_DEFAULT_API_PORT`]。
    pub port: u16,
    /// 是否允许非回环监听（默认 false）。
    #[serde(alias = "allowNonLoopback")]
    pub allow_non_loopback: bool,
    /// 请求日志保留条数。
    #[serde(alias = "logKeep")]
    pub log_keep: usize,
    /// 是否记录 prompt / response 正文。
    #[serde(alias = "logBodies")]
    pub log_bodies: bool,
    /// 请求体上限（MB；`0` 视为未设置并回落默认）。
    #[serde(alias = "maxBodyMb")]
    pub max_body_mb: usize,
    /// 未指定模型时的默认模型名（空串 ⇒ 用清单第一条；都没有 ⇒ 明确报错）。
    #[serde(alias = "defaultModel")]
    pub default_model: String,
    /// 单轮最多换号次数。
    #[serde(alias = "maxRotate")]
    pub max_rotate: usize,
    /// 「指定账号」：优先消耗该 uid（不可用时**回落**自动择优，绝不拒绝服务）。
    #[serde(alias = "preferredUid")]
    pub preferred_uid: String,
    /// 会话粘性存活时长（毫秒；`<= 0` 由 [`crate::StickyTable`] 归一为 30 分钟）。
    #[serde(alias = "stickyTtlMs")]
    pub sticky_ttl_ms: i64,
    /// **上下文上限策略**（默认 `true`）：清单声明 `context_config` 里的**最大**窗口
    /// （如 1M），并把它写进请求的 `model_config.max_input_tokens`；关闭则用默认档（如 200K）。
    #[serde(alias = "maxContext")]
    pub max_context: bool,
}

impl Default for QoderGatewayConfig {
    fn default() -> Self {
        Self {
            // ★ **默认关闭** —— 与 WB / Trae 两条产品线同一产品口径（P12 D29「默认关」）：
            //   网关会把本机的上游额度暴露成接口，不声不响地默认监听是危险默认值。
            enabled: false,
            bind_addr: "127.0.0.1".to_string(),
            port: QODER_DEFAULT_API_PORT,
            allow_non_loopback: false,
            // 与另两条产品线同值（同一管理面不该有两种节奏）。
            log_keep: 200,
            log_bodies: false,
            max_body_mb: 0,
            default_model: String::new(),
            max_rotate: 3,
            preferred_uid: String::new(),
            sticky_ttl_ms: 30 * 60 * 1000,
            max_context: true,
        }
    }
}

impl QoderGatewayConfig {
    /// 从 `qoder/gateway_config.json` 读取；缺失 / 损坏回落默认。
    pub fn load() -> Self {
        std::fs::read_to_string(paths::gateway_config_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// 原子写回。
    pub fn save(&self) -> Result<(), String> {
        let file = paths::gateway_config_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &text).map_err(|error| error.to_string())
    }

    /// 独立监听的基址。
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.bind_addr, self.port)
    }
}

/// 缓存的模型清单（**带来源**——「拿不到」必须能与「拿到了」区分，绝不假装有数据）。
#[derive(Debug, Clone, Default)]
pub struct CatalogView {
    pub models: Vec<Value>,
    /// 来源三级：
    /// - `live`：最近一次拉取**成功**（含 10 分钟内的缓存命中）——清单是当前事实；
    /// - `cached`：本次拉取失败，退回上次成功的结果（`note` 说原因）；
    /// - `unavailable`：拉取失败且**没有任何缓存** —— 明确「拿不到」。
    pub source: &'static str,
    /// 上次成功拉取的时刻（毫秒）；`unavailable` 时为 `None`。
    pub fetched_at: Option<i64>,
    /// 取数说明（失败 / 无账号时的可读原因；`live` 为 `None`）。
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct CachedCatalog {
    fetched_at: i64,
    models: Vec<Value>,
}

impl CachedCatalog {
    fn into_view(self, source: &'static str, note: Option<String>) -> CatalogView {
        CatalogView {
            models: self.models,
            source,
            fetched_at: Some(self.fetched_at),
            note,
        }
    }
}

/// Qoder 网关运行状态（`Arc` 句柄，可克隆）。
#[derive(Clone)]
pub struct QoderGatewayState {
    pub config: Arc<RwLock<QoderGatewayConfig>>,
    /// 区域 → 账号池。
    pub pools: Arc<Mutex<HashMap<QoderRegion, pool::QoderPool>>>,
    /// 用户禁用的模型（P11；键 = 作用域）。
    pub disabled: Arc<RwLock<crate::disabled_models::DisabledModels>>,
    /// 会话粘性绑定表。
    pub sticky: Arc<Mutex<crate::StickyTable>>,
    /// 请求日志。
    pub log: Arc<RequestLog>,
    /// 多 API Key 存储。
    pub key_store: Arc<apikey::QoderApiKeyStore>,
    /// 模型清单缓存（区域级）。
    catalogs: Arc<RwLock<HashMap<QoderRegion, CachedCatalog>>>,
    pub started_at: i64,
    pub total_requests: Arc<AtomicU64>,
    pub last_error: Arc<RwLock<Option<String>>>,
    /// 出站 HTTP 客户端。
    pub http: reqwest::Client,
    /// 请求体上限字节数（构造期固化）。
    pub body_limit_bytes: usize,
    /// 出站基址覆盖（**测试挂载点**：把出站打到本地 mock 上游；生产恒 `None`）。
    pub upstream_override: Option<String>,
}

impl QoderGatewayState {
    pub fn new(config: QoderGatewayConfig) -> Self {
        let log = Arc::new(RequestLog::new(
            paths::logs_dir().join("gateway_requests.jsonl"),
            config.log_keep,
            config.log_bodies,
        ));
        Self {
            config: Arc::new(RwLock::new(config.clone())),
            pools: Arc::new(Mutex::new(HashMap::new())),
            disabled: crate::disabled_models::shared(),
            sticky: Arc::new(Mutex::new(crate::StickyTable::new(config.sticky_ttl_ms))),
            log,
            key_store: Arc::new(apikey::QoderApiKeyStore::new(paths::gateway_keys_file())),
            catalogs: Arc::new(RwLock::new(HashMap::new())),
            started_at: core_config::now_ms(),
            total_requests: Arc::new(AtomicU64::new(0)),
            last_error: Arc::new(RwLock::new(None)),
            http: build_http_client(),
            body_limit_bytes: body_limit_bytes(config.max_body_mb),
            upstream_override: None,
        }
    }

    /// 测试用：覆盖出站基址。
    pub fn with_upstream(mut self, upstream: String) -> Self {
        self.upstream_override = Some(upstream);
        self
    }

    pub async fn config_snapshot(&self) -> QoderGatewayConfig {
        self.config.read().await.clone()
    }

    /// 上下文上限策略（同步，供清单与请求构造用）；拿不到锁按「上限」处理。
    pub fn max_context_now(&self) -> bool {
        self.config
            .try_read()
            .map(|config| config.max_context)
            .unwrap_or(true)
    }

    /// 取模型清单（带缓存；无账号 / 拉取失败时给**可读 note**，绝不假装有数据）。
    ///
    /// 缓存有效期见 [`CATALOG_TTL_MS`]；管理面要「立即重拉」时走 [`Self::refresh_catalog`]。
    pub async fn catalog_for(&self, region: QoderRegion) -> CatalogView {
        {
            let cache = self.catalogs.read().await;
            if let Some(cached) = cache.get(&region) {
                if now_ms() - cached.fetched_at < CATALOG_TTL_MS && !cached.models.is_empty() {
                    return cached.clone().into_view("live", None);
                }
            }
        }
        self.fetch_catalog(region).await
    }

    /// 强制重拉（管理面「重新识别」；忽略 TTL 缓存）。
    ///
    /// 失败时仍然退回上次成功的结果（`source = "cached"` + note），
    /// **不**因为「这次没拉到」就把已有清单清空 —— 那会让界面在偶发超时时突然变空。
    pub async fn refresh_catalog(&self, region: QoderRegion) -> CatalogView {
        self.fetch_catalog(region).await
    }

    /// **只读**内存里的清单缓存（**不触发任何上游往返**）—— 限流探测的模型轴用它。
    ///
    /// ## 为什么不用 [`Self::catalog_for`] / `qoder::api::fetch_model_list`
    ///
    /// - [`Self::catalog_for`] 缓存未命中时会**真的去上游拉一次**（一次网络往返）；
    /// - `qoder::api::fetch_model_list` 更是**每次**都往返。
    ///
    /// 探测每开一轮都拉一次清单是**纯浪费**（探测本身已经要逐格发请求）。本入口**只读缓存**：
    /// 模型页（`get_qoder_gateway_models`）与创建 Key 时已经把它填好；缓存为空 ⇒ 探测在
    /// `model_probe` 层给可读错误（「请先『重新识别』」），而**不是**静默联网。
    ///
    /// ## 为什么不用 CLI 缓存目录 `paths::cli_models_cache_dir`
    ///
    /// 那只是**路径助手**（全仓零读取点，见 grep），用它会要求本仓**自造**一个 CLI 清单文件的
    /// 解析格式 —— 我们不知道官方 CLI 落盘的字段，硬猜等于编数据。故不用。
    pub async fn cached_catalog_models(&self, region: QoderRegion) -> Vec<Value> {
        self.catalogs
            .read()
            .await
            .get(&region)
            .map(|cached| cached.models.clone())
            .unwrap_or_default()
    }

    /// 真的去上游拉一次（成功写缓存；失败退回缓存 + note）。
    async fn fetch_catalog(&self, region: QoderRegion) -> CatalogView {
        // 找一个有 access 的账号（拉清单要过 COSY 签名，同样需要可调用凭据）。
        let account_row = account::load_accounts_for(region)
            .accounts
            .into_iter()
            .find(|account| {
                account
                    .access_token
                    .as_deref()
                    .map(|token| !token.is_empty())
                    .unwrap_or(false)
            });
        let Some(account_row) = account_row else {
            let note =
                "没有可用账号：模型清单需要账号凭据向官方拉取（去「账号」页导入）".to_string();
            // 无账号 ⇒ 仍然给缓存（若有）：缓存里的是**上次有账号时**拉到的清单，
            // 比清空更有用；来源标成 `cached` 并不带时间上的新鲜承诺。
            let cache = self.catalogs.read().await;
            return match cache.get(&region).cloned() {
                Some(cached) if !cached.models.is_empty() => cached.into_view("cached", Some(note)),
                _ => CatalogView {
                    models: Vec::new(),
                    source: "unavailable",
                    fetched_at: None,
                    note: Some(note),
                },
            };
        };
        let input = cosy::CosyAccountInput {
            uid: &account_row.uid,
            nickname: &account::display_name(&account_row),
            access_token: account_row.access_token.as_deref().unwrap_or(""),
            refresh_token: account_row.refresh_token.as_deref().unwrap_or(""),
            user_type: None,
            org_id: None,
            org_name: None,
        };
        let hosts: Vec<&str> = match &self.upstream_override {
            Some(host) => vec![host.as_str()],
            None => qoder_region_candidates(region),
        };
        match api::fetch_model_list_at(&hosts, &input).await {
            Ok(models) => {
                let fetched_at = now_ms();
                let mut cache = self.catalogs.write().await;
                cache.insert(
                    region,
                    CachedCatalog {
                        fetched_at,
                        models: models.clone(),
                    },
                );
                CatalogView {
                    models,
                    source: "live",
                    fetched_at: Some(fetched_at),
                    note: None,
                }
            }
            Err(error) => {
                // 拉取失败：退回旧缓存（若有）+ note；都没有 ⇒ 空 + note（`unavailable`）。
                let cache = self.catalogs.read().await;
                let note = Some(format!("模型清单拉取失败：{error}"));
                match cache.get(&region).cloned() {
                    Some(cached) if !cached.models.is_empty() => cached.into_view("cached", note),
                    _ => CatalogView {
                        models: Vec::new(),
                        source: "unavailable",
                        fetched_at: None,
                        note,
                    },
                }
            }
        }
    }

    /// 会话粘性查询（已过期视为无绑定）。
    pub async fn sticky_uid(&self, key: Option<&str>) -> Option<String> {
        let key = key?;
        self.sticky
            .lock()
            .await
            .get(key, core_config::now_ms())
            .map(str::to_string)
    }

    /// 绑定会话粘性（**只在上游成功之后**调用）。
    pub async fn bind_sticky(&self, key: Option<&str>, uid: &str) {
        if let Some(key) = key {
            self.sticky
                .lock()
                .await
                .bind(key, uid, core_config::now_ms());
        }
    }

    /// 记一次请求（计数 + 日志 + 最近错误）。
    ///
    /// `prompt_tokens` / `completion_tokens` 为 `0` 表示**未取到**（不是「0 用量」）；
    /// 字段名与 WB / Trae 两侧日志对齐（`ts` / `endpoint` / `account` / `accountUid` /
    /// `model` / `status` / `latencyMs` / `promptTokens` / `completionTokens` / `stream` / `error`），
    /// 外加 `region` 归属（**一本日志记两个区域的请求** ⇒ 不标归属就分不清是哪本账号库
    /// 产生的流量；与 WB 日志带 `region` 同一理由）。
    #[allow(clippy::too_many_arguments)]
    pub async fn record_request(
        &self,
        endpoint: &'static str,
        region: QoderRegion,
        model: &str,
        status: u16,
        uid: &str,
        latency_ms: i64,
        stream: bool,
        prompt_tokens: i64,
        completion_tokens: i64,
        error: Option<String>,
    ) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.log.record(serde_json::json!({
            "ts": core_config::now_ms(),
            "endpoint": endpoint,
            "method": "POST",
            "region": region.as_str(),
            "account": uid,
            "accountUid": uid,
            "model": model,
            "status": status,
            "latencyMs": latency_ms,
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
            "stream": stream,
            "error": error,
        }));
        if let Some(message) = error {
            *self.last_error.write().await = Some(message);
        }
    }
}

// ---------------------------------------------------------------------------
// 对外状态视图（管理面）
// ---------------------------------------------------------------------------

/// 状态视图里**由运行时现算**的那部分（池摘要 / Key 前缀 / 监听地址 …）。
///
/// 拆出来的唯一目的是让键集合护栏（`status_view_keys_are_pinned`）能测**纯函数** ——
/// 否则要为一条断言构造带环境变量隔离的 `QoderGatewayState`。
#[derive(Debug, Clone, Default)]
pub struct StatusParts {
    pub running: bool,
    pub addr: Option<String>,
    pub version: String,
    pub total_requests: u64,
    pub last_error: Option<String>,
    pub api_key_prefix: String,
    pub pool: Value,
    pub accounts: Vec<Value>,
    pub diagnose: Vec<String>,
}

/// 组装状态视图（**纯函数**；入参已是现算好的值）。
///
/// ## 两个刻意的形状选择
///
/// 1. **字段名与 WB / Trae 的状态视图同款**（snake_case）—— 前端同一套归一化逻辑复用；
/// 2. **`region` 是入参、不进响应体**（与 `trae::status_view` 同一纪律）：池摘要本身
///    已隐含「这是哪本区域库」，多一个回显键只会与前端形状再对齐一次。
///
/// ## 一个 Qoder 特有的取舍：**一个监听、两个区域**
///
/// Qoder 网关只有一个独立监听（默认 57895），区域**由 API Key 的归属决定**
/// （`QoderKeyRegion`），因此 `enabled` / `port` / `running` 是**两个区域共用**的
/// 同一份事实 —— 这正是响应体里不出现区域字段的另一个理由：这些键对两个区域
/// 本来就是同一个值，按区域回显反而会造成「两个区域各有一个网关」的错觉。
///
/// 响应键集合被 `status_view_keys_are_pinned` 钉住 —— 增删键要**同时**改前端归一化
/// （`src/lib/qoder-gateway.ts`），否则前端要么丢字段、要么把 `undefined` 当默认值
/// 静默呈现。
pub fn status_view_value(
    config: &QoderGatewayConfig,
    region: QoderRegion,
    parts: StatusParts,
) -> Value {
    json!({
        "enabled": config.enabled,
        "running": parts.running,
        "addr": parts.addr,
        "base_url": config.base_url(),
        "bind_addr": config.bind_addr,
        "port": config.port,
        "allow_non_loopback": config.allow_non_loopback,
        "version": parts.version,
        "total_requests": parts.total_requests,
        "last_error": parts.last_error,
        "api_key_prefix": parts.api_key_prefix,
        "pool": parts.pool,
        "accounts": parts.accounts,
        "diagnose": parts.diagnose,
        // 上游主机（本区域的候选列表，按顺序）—— 概览卡里要写出「请求实际打到哪」。
        "upstream": qoder_region_candidates(region).join(" / "),
    })
}

/// 组装管理面状态视图（Tauri 命令与 webui 路由**共用**）。
///
/// `running` / `addr` / `version` 由宿主提供 —— 只有宿主知道监听是否真的起来了、
/// 应用版本是多少；池摘要与账号明细针对**指定区域**的池现算，因此两个宿主看到的
/// 永远是同一份。
pub async fn status_view(
    state: &QoderGatewayState,
    running: bool,
    addr: Option<String>,
    version: &str,
    region: QoderRegion,
) -> Value {
    let now = core_config::now_ms();
    let config = state.config_snapshot().await;

    // 池随账号库同步（导入 / 刷新后即时生效），再取本次视图要用的三份。
    let (pool, accounts, diagnose) = {
        let mut pools = state.pools.lock().await;
        let pool = pools.entry(region).or_default();
        pool.sync_for(region);
        (
            pool.summary(now),
            pool.accounts_view(now),
            pool.diagnose(now),
        )
    };

    // 最近一把**本区域**未吊销 Key 的前缀（本区域之外的 Key 连不上这本账号库，
    // 列它的前缀只会误导）。明文不可复原，故只给前缀。
    let api_key_prefix = state
        .key_store
        .list()
        .into_iter()
        .filter(|record| !record["revoked"].as_bool().unwrap_or(false))
        .filter(|record| record["region"].as_str() == Some(region.as_str()))
        .max_by_key(|record| record["createdAt"].as_i64().unwrap_or(0))
        .and_then(|record| record["prefix"].as_str().map(str::to_string))
        .unwrap_or_default();

    status_view_value(
        &config,
        region,
        StatusParts {
            running,
            addr,
            version: version.to_string(),
            total_requests: state.total_requests.load(Ordering::Relaxed),
            last_error: state.last_error.read().await.clone(),
            api_key_prefix,
            pool,
            accounts,
            diagnose,
        },
    )
}

// ---------------------------------------------------------------------------
// 路由装配与产品线注册
// ---------------------------------------------------------------------------

/// 组装 Qoder 网关路由（**不含 fallback**）。
pub fn router(state: QoderGatewayState) -> axum::Router {
    use axum::routing::{get, post};
    let body_limit = state.body_limit_bytes;
    axum::Router::new()
        .merge(crate::health_routes(routes::health))
        .route("/status", get(routes::status))
        .route("/v1/models", get(routes::models))
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/messages", post(routes::messages))
        .route("/v1/responses", post(routes::responses))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            routes::bearer_auth,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .with_state(state)
}

/// 按配置启动独立监听（只回环，除非显式允许；端口占用给可读错误）。
pub async fn spawn_listener(state: QoderGatewayState) -> anyhow::Result<crate::GatewayHandle> {
    let config = state.config_snapshot().await;
    let bind_addr = config.bind_addr.clone();
    let port = config.port;

    let ip: IpAddr = bind_addr.parse().map_err(|_| {
        anyhow::anyhow!("Qoder 网关监听地址无效：{bind_addr}（应为 IPv4/IPv6 字面量）")
    })?;
    if !ip.is_loopback() && !config.allow_non_loopback {
        return Err(anyhow::anyhow!(
            "已拒绝监听非回环地址 {bind_addr}：局域网内任何设备都可消耗你的 Qoder 额度，\
             请确认可信网络后，在设置中显式开启『允许局域网访问』再重试"
        ));
    }
    let addr = SocketAddr::new(ip, port);
    let listener = TcpListener::bind(addr).await.map_err(|error| {
        anyhow::anyhow!(
            "无法监听 {addr}：端口可能被占用或不可用（{error}）。请在设置中改用其他端口。"
        )
    })?;
    let local = listener.local_addr().unwrap_or(addr);
    let app = router(state);
    let (tx, rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    Ok(crate::GatewayHandle::new(local, tx, join))
}

/// Qoder 网关产品线（[`crate::GatewayProduct`] 实现）。
pub struct QoderGateway;

static QODER_SHARED_STATE: OnceLock<QoderGatewayState> = OnceLock::new();

impl GatewayProduct for QoderGateway {
    type State = QoderGatewayState;

    const OWNED_PATHS: &'static [&'static str] = &[
        "/health",
        "/healthz",
        "/status",
        "/v1/models",
        "/v1/chat/completions",
        "/v1/messages",
        "/v1/responses",
    ];

    fn shared_state() -> QoderGatewayState {
        QODER_SHARED_STATE
            .get_or_init(|| QoderGatewayState::new(QoderGatewayConfig::load()))
            .clone()
    }

    fn is_enabled(state: &QoderGatewayState) -> impl std::future::Future<Output = bool> + Send {
        let state = state.clone();
        async move { state.config_snapshot().await.enabled }
    }

    fn router(state: QoderGatewayState) -> axum::Router {
        self::router(state)
    }

    fn spawn_listener(
        state: QoderGatewayState,
    ) -> impl std::future::Future<Output = anyhow::Result<crate::GatewayHandle>> + Send {
        self::spawn_listener(state)
    }
}

/// 渠道日志文件（供管理命令透出）。
pub fn log_file() -> PathBuf {
    paths::logs_dir().join("gateway_requests.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_dedicated_port() {
        let config = QoderGatewayConfig::default();
        assert_eq!(config.port, QODER_DEFAULT_API_PORT);
        // ★ 2026-10-17：四条网关统一落在 5789x 一段，本端口必须与**同段**的既有端口全部错开。
        assert_eq!(config.port, 57895, "Qoder 网关默认端口应为 57895");
        for taken in [
            57890u16,                                                // 界面本体
            57891,                                                   // WorkBuddy 网关
            57892,                                                   // 统一网关
            57893,                                                   // 客户端会话监听 hook
            buddy_switch_core::modules::trae::TRAE_DEFAULT_API_PORT, // Trae 网关
            8899,                                                    // Trae 登录代理
        ] {
            assert_ne!(config.port, taken, "不得与既有端口 {taken} 撞车");
        }
        assert_eq!(
            config.base_url(),
            format!("http://127.0.0.1:{QODER_DEFAULT_API_PORT}")
        );
        // 默认模型为**空** —— 由清单第一条兜底，不硬编码一个可能不存在的名字。
        assert!(config.default_model.is_empty());
        // ★ 默认**关**（与 WB / Trae 同一产品口径；变异：改成 true ⇒ 本条红）。
        assert!(
            !config.enabled,
            "网关默认必须关闭 —— 不声不响地默认监听是危险默认值"
        );
        assert!(!config.allow_non_loopback);
        assert_eq!(config.log_keep, 200, "与另两条产品线同值");
    }

    /// 路径所有权必须与既有产品线**重叠**（独立端口的前提；premise 变了要复核）。
    #[test]
    fn owned_paths_overlap_with_other_products() {
        let mine = QoderGateway::OWNED_PATHS;
        for path in ["/v1/models", "/v1/chat/completions"] {
            assert!(mine.contains(&path), "Qoder 应拥有 {path}");
        }
    }

    /// ★ 状态视图的键集合被**钉住**：增删键要**同时**改前端归一化
    /// （`src/lib/qoder-gateway.ts` 的 `normalizeQoderGatewayStatus`），
    /// 否则前端要么丢字段、要么把 `undefined` 当默认值静默呈现。
    ///
    /// 变异：删掉 `status_view_value` 里任一键（如 `pool`）⇒ 本条红。
    #[test]
    fn status_view_keys_are_pinned() {
        let config = QoderGatewayConfig::default();
        let value = status_view_value(
            &config,
            QoderRegion::Cn,
            StatusParts {
                version: "0.0.0-test".to_string(),
                ..StatusParts::default()
            },
        );
        let expected: std::collections::BTreeSet<&str> = [
            "enabled",
            "running",
            "addr",
            "base_url",
            "bind_addr",
            "port",
            "allow_non_loopback",
            "version",
            "total_requests",
            "last_error",
            "api_key_prefix",
            "pool",
            "accounts",
            "diagnose",
            "upstream",
        ]
        .into_iter()
        .collect();
        let keys: std::collections::BTreeSet<&str> = value
            .as_object()
            .expect("status view 是对象")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, expected, "status key set must be pinned");
        // 不得出现 camelCase 键（与 WB / Trae 的状态视图同款 snake_case）。
        assert!(!value.as_object().unwrap().contains_key("baseUrl"));
        // 区域**不回显**（`:一个监听两个区域` —— 回显会造出「两个网关」的错觉）。
        assert!(!value.as_object().unwrap().contains_key("region"));
        // 上游是本区域的候选主机（概览卡要写出请求实际打到哪）。
        assert!(
            value["upstream"].as_str().unwrap().contains("qoder"),
            "upstream 应给出本区域候选主机：{}",
            value["upstream"]
        );
    }
}
