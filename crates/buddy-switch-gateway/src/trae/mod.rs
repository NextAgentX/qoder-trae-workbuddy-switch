//! Trae 的 OpenAI 兼容网关（`/v1/chat/completions`）。
//!
//! ## 为什么与 WorkBuddy 网关**不共用**一套实现
//!
//! 两者除了「都是 OpenAI 兼容」之外没有任何共同点：
//!
//! | 维度 | WorkBuddy 网关（`crate::pool` / `routes`） | Trae 网关（本模块） |
//! |:---|:---|:---|
//! | 归属域 | `Region`（cn / global），凭据随域隔离 | 无域概念，账号库只有一套 |
//! | 凭据形态 | `access_token` + `domain` | `Cloud-IDE-JWT <jwt>` |
//! | 上游协议 | 直接转发 OpenAI 请求体 | 需改写成 `llm_utils_chat` 专用请求体 |
//! | 上游响应 | OpenAI SSE，可原样透传 | 私有 SOLO SSE（`event: output` 等），**必须转换** |
//! | 路由挂载 | merge 进宿主 `/v1/*` | 独立监听（否则 `/v1/chat/completions` 与 WorkBuddy 撞车） |
//!
//! 因此本模块是**平行的第二套实现**，刻意不复用 `pool` / `protocol` / `outbound`：
//! 强行抽象只会让两侧都变得难改。共用的只有真正与产品无关的基础件——
//! [`crate::logging::RequestLog`]（只存 `Value`）与 [`crate::error`] 的错误形状。
//!
//! ## 与 Trae 账号模块的关系
//!
//! 账号、设备标识、冷却、剩余积分**全部复用** `buddy_switch_core::modules::trae`：
//! 网关不另建账号库，也不另建冷却文件——否则「签到页显示正常、网关页显示冷却中」
//! 这类不一致会立刻出现。网关的错误会经
//! [`buddy_switch_core::modules::trae::credits::save_cooldown_for`] 写回**该产品线自己的**
//! 冷却文件（`account_cooldowns.json` / `account_cooldowns.trae_cn.json`），
//! 与签到页的变体分家口径一致：两个页面看到的是同一份状态。

pub mod apikey;
pub mod capture;
pub mod catalog;
pub mod payload;
pub mod pool;
pub mod routes;
pub mod sse;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::Region;
use buddy_switch_core::modules::trae::region::TraeRegion;
use buddy_switch_core::modules::trae::variant::TraeVariant;

use crate::disabled_models::DisabledModels;
use crate::sticky::StickyTable;
use buddy_switch_core::modules::trae::{paths, TRAE_DEFAULT_API_PORT};

use crate::logging::RequestLog;
use crate::GatewayHandle;

use crate::pool::PoolSummary;
use pool::TraePool;

// ---------------------------------------------------------------------------
// 上游常量
// ---------------------------------------------------------------------------

/// Trae SOLO 上游基址（**CN 默认值**）。
///
/// 注意与 `modules::trae::endpoints_for(..).account_base`（`https://api.trae.cn`，签到/积分用）
/// **不是同一个主机**：对话走 mchost.guru 的 agent 网关。两者不可互换。
///
/// 取值与 [`buddy_switch_core::modules::trae::variant`] 的 CN 端点表逐字一致
/// （实测两条 CN 产品线都是这个主机）；该表另登记了国际化的
/// `https://core-normal.trae.ai`，**但从未对真实上游跑通过**。
/// 想按变体/地区切换出站时，请改 [`TraeGatewayConfig::upstream`] 的取值来源，
/// 不要在调用点写分支 —— 该字段本来就是为"可重定向出站"设计的（e2e 测试也靠它）。
pub const TRAE_AGENT_HOST: &str = "https://trae-api-cn.mchost.guru";

/// SOLO 对话接口路径（消耗 IDE 积分，product_id 208）。
pub const TRAE_LLM_CHAT_PATH: &str = "/api/agent/v3/llm_utils_chat";

/// 客户端 App ID（参考实现硬编码值，非机密）。
pub const TRAE_APP_ID: &str = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8";

/// IDE 版本号与版本码，随上游校验字段一同下发。
pub const TRAE_IDE_VERSION: &str = "0.1.50";
pub const TRAE_IDE_VERSION_CODE: &str = "20260811";

/// `function` 字段取值 —— **按程序位分家**（2026-09-30 实测，issue #4「看得见、调不动」）。
///
/// ## 为什么必须分家（实测现场）
///
/// 上游按 `function` 做**白名单**：不属于该 function 的模型一律回
/// `event:error` / `code 4001 param is invalid`。实测（见
/// `routes.rs::tests::probe_traecode_model_names`）：
///
/// | function | 模型 | 结果 |
/// |:---|:---|:---|
/// | `solo_work_lite` | `glm-5.3` | ✅ |
/// | `solo_work_lite` | `glm-5.3-flash` | ❌ 4001 |
/// | `chat_v3` | `glm-5.3-flash` | ✅ |
/// | `solo_agent` | `glm-5.3-flash` | ✅ |
/// | `solo_work_lite` | `Doubao-Seed-Code`（只在 `solo_coder` 分组里） | ❌ 4001 |
///
/// 写死成 `solo_work_lite` 时，TraeCode 的模型（`glm-5.3-flash` 等）**必然调不动**；
/// 同时 TraeWork 对外清单里那些**不属于 `solo_work_lite`** 的模型也调不动
/// （见 [`payload::models_response_for`] 的口径）。
///
/// ## 取值依据
///
/// 分组名直接来自客户端自己的清单键（`state.vscdb` 的
/// `<uid><分隔符>AI.agent.model.model_list_map`）—— 那是上游下发的原始分组，
/// 不是我方命名的。TraeWork 的 chat 分组是 `solo_work_lite`（改造前就在用），
/// TraeCode 的 chat 分组是 `chat_v3`（`solo_agent` 实测也能过，但那是 agent 分组，
/// 与 OpenAI 兼容网关的纯 chat 语义不符，故不取）。
///
/// ⚠️ **国际版两个程序位从未对真实上游跑通过**（本机无可用国际凭据），
/// 这里按**程序**派生（`TraeWork` 家族 → `solo_work_lite`、TraeCode 家族 → `chat_v3`），
/// 与「产品线不改变端点、region 才改变端点」的既有结论同向 —— 但**未实测**。
pub const TRAE_FUNCTION_SOLO_WORK: &str = "solo_work_lite";
pub const TRAE_FUNCTION_CHAT_V3: &str = "chat_v3";

/// 取该程序位该用的 `function`（见 [`TRAE_FUNCTION_SOLO_WORK`] 的实测表）。
///
/// 判据是**程序**而不是区域：`TraeWork` / `Global` 都是 TraeWork 客户端，
/// `Trae` / `GlobalTraeCode` 都是 TraeCode 客户端。
///
/// ## 合并后的客户端（issue #20）取 `chat_v3`
///
/// 合并版（CN `Trae CN`、国际 `Trae`）**就是** IDE 那条客户端的下一代构建
/// （客户端合并时复用了 TraeCode 的目录名与 exe 名，见 `region` 模块头），
/// 因此它的 chat 分组与 TraeCode 家族相同 = [`TRAE_FUNCTION_CHAT_V3`]。
/// CN 那一格有实测依据（`Trae CN` 本体）；国际那格与其余国际程序位一样**未实测**，
/// 按同一判据（程序家族）派生 —— 与「产品线不改变端点、region 才改变端点」同向。
pub fn function_for(variant: TraeVariant) -> &'static str {
    match variant {
        TraeVariant::TraeWork | TraeVariant::Global => TRAE_FUNCTION_SOLO_WORK,
        TraeVariant::Trae
        | TraeVariant::GlobalTraeCode
        | TraeVariant::TraeMerged
        | TraeVariant::GlobalTraeMerged => TRAE_FUNCTION_CHAT_V3,
    }
}

/// Trae 程序位 → 通用 [`Region`]（P11 禁用存储按 region 分家，与 WorkBuddy 侧同一把键）。
///
/// 为什么按 **region** 而不是程序位：后端禁用存储是 region 粒度（`cn` / `global`），
/// 用程序位会引入一个存储里不存在的维度（**决策 #4**）。
pub fn region_of_variant(variant: TraeVariant) -> Region {
    match variant.region() {
        TraeRegion::Cn => Region::Cn,
        TraeRegion::Global => Region::Global,
    }
}

/// 未指定模型时的默认值。
///
/// ## 为什么是 `deepseek-v4.1-flash` 而不是老的 `deepseek-v4-flash`（2026-09-30 实测换名）
///
/// 老值 `deepseek-v4-flash` 是**客户端早已不再提供**的旧名（客户端现在叫
/// `deepseek-v4.1-flash`）；它经 `model_config` 的别名仍能打通上游（实测），
/// 但「默认模型」是用户**看得见**的一格（模型卡的「默认 X」），指向一个客户端里
/// 找不到的名字，就是 issue #4 那句「内容滞后」的最后一处。
///
/// 新值实测（`probe_traecode_model_names` 的 Y1，`solo_work_lite` 下）：
/// `config_name=deepseek-v4.1-flash` + `model_name=deepseek-v4.1-flash__dev` → **上游接受**。
///
/// ⚠️ 换名会改掉「不带 `model` 的请求」实际调用的模型，因此
/// [`TraeGatewayConfig::load`] 带一条**窄迁移**：只把落盘值恰好等于
/// [`LEGACY_DEFAULT_MODEL`] 的改写成新值（那个值是程序自己写进去的，用户改不了它），
/// 并打印一行说明。其他值一律不动。
pub const TRAE_DEFAULT_MODEL: &str = "deepseek-v4.1-flash";

/// 历史默认值：只用于**一次性迁移**（见 [`TRAE_DEFAULT_MODEL`] 的说明）。
pub const LEGACY_DEFAULT_MODEL: &str = "deepseek-v4-flash";

/// 单请求体上限默认值（MB）。与 WorkBuddy 网关保持一致。
pub const DEFAULT_MAX_BODY_MB: usize = 8;

/// 单轮最多换号次数。
pub const DEFAULT_MAX_ROTATE: usize = 3;

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

/// Trae 网关运行配置（落盘 `~/.buddy-switch/trae/api_gateway.json`）。
///
/// 字段名沿用 WorkBuddy 网关的 snake_case 约定（同一管理面的两套网关不该有两种风格），
/// 同时接受 camelCase 别名以容忍前端写法差异。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TraeGatewayConfig {
    /// 是否启用独立监听。
    pub enabled: bool,
    /// 监听地址，默认 `127.0.0.1`。
    #[serde(alias = "bindAddr")]
    pub bind_addr: String,
    /// 监听端口，默认 [`TRAE_DEFAULT_API_PORT`]（57894，与 WorkBuddy 的 57891 错开）。
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
    /// 未指定模型时的默认模型名。
    #[serde(alias = "defaultModel")]
    pub default_model: String,
    /// 单轮最多换号次数。
    #[serde(alias = "maxRotate")]
    pub max_rotate: usize,
    /// 「指定账号」：优先消耗该 uid 的积分；空串表示不指定（自动择优）。
    ///
    /// ★ 偏好是**优化而非约束**：该账号不可用（冷却 / 禁用 / 零积分 / 该模型被限流 /
    /// 已在本轮试过）时**回落**到自动择优，绝不因此拒绝服务。
    /// 判定与自动选号共用同一套可用性判据（`pool::entry_usable`），
    /// 否则会出现「明明有别的号可用却一直失败」这种不报错的偶发故障。
    #[serde(alias = "preferredUid")]
    pub preferred_uid: String,
    /// 会话粘性绑定的存活时长（毫秒；`<= 0` 由 [`StickyTable`] 归一为 30 分钟）。
    ///
    /// 同一会话（请求体 `metadata.conversation_id`）在 TTL 内**优先复用上次成功的账号**，
    /// 避免多账号池下「对话突然失忆」与跨账号的缓存前缀命中率归零。
    /// 与 WorkBuddy 网关的 `sticky_ttl_ms` 同默认值。
    #[serde(alias = "stickyTtlMs")]
    pub sticky_ttl_ms: i64,
    /// 对话转发抓取开关（诊断旁路，落盘 `~/.buddy-switch/trae/capture/<程序位>/`）。
    ///
    /// 默认 **false**：抓取会把对话正文（含提示词与响应）原样落盘，
    /// 属敏感内容，只在排查问题时手动打开；**无轮转、无上限**，用完请手工清理目录。
    ///
    /// ★ 本字段没有界面开关，靠 `api_gateway.json` 手改 + 重启网关生效 ——
    /// 因此它**必须**在管理面的整份保存里被原样带回：配置保存是「整份覆盖」，
    /// 缺键就回落 `false`（前端 `toTraeGatewayConfigRaw` 负责带上它）。
    /// 漏了这一步的后果不是报错，而是「用户以为还开着、其实已被静默关掉」。
    #[serde(alias = "captureEnabled")]
    pub capture_enabled: bool,
    /// **Max Mode**：对「官方目录里 `max > dev`」的模型注入 `is_max_mode`，并在
    /// `/v1/models` 声明它自己的上限（逐模型，**不是统一 1M**）。默认 `true`。
    ///
    /// 关闭后：清单只声明 `dev`（`__dev` 通道实际窗口），请求不再注入 —— 用于
    /// 「Max Mode 万一加价 / 拖慢，先按默认窗口用」的场景。与原型里的开关同义。
    #[serde(alias = "maxMode")]
    pub max_mode: bool,
}

/// 进程内 Trae 网关当前的 Max Mode 开关（给**命令面**等拿不到 `TraeGatewayState` 的地方用）。
pub fn max_mode_now() -> bool {
    <TraeGateway as crate::GatewayProduct>::shared_state().max_mode_now()
}

impl Default for TraeGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "127.0.0.1".to_string(),
            port: TRAE_DEFAULT_API_PORT,
            allow_non_loopback: false,
            log_keep: 200,
            log_bodies: false,
            max_body_mb: DEFAULT_MAX_BODY_MB,
            default_model: TRAE_DEFAULT_MODEL.to_string(),
            max_rotate: DEFAULT_MAX_ROTATE,
            preferred_uid: String::new(),
            sticky_ttl_ms: 30 * 60 * 1000,
            capture_enabled: false,
            max_mode: true,
        }
    }
}

impl TraeGatewayConfig {
    /// 从 `~/.buddy-switch/trae/api_gateway.json` 读取；缺失/损坏回落默认值。
    pub fn load() -> Self {
        let file = paths::api_gateway_file();
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Ok(mut config) = serde_json::from_str::<TraeGatewayConfig>(&text) {
                config.migrate_legacy_default_model();
                return config;
            }
        }
        TraeGatewayConfig::default()
    }

    /// **窄迁移**：把历史默认模型改写成当前默认（见 [`TRAE_DEFAULT_MODEL`] 的说明）。
    ///
    /// 只认**恰好等于** [`LEGACY_DEFAULT_MODEL`] 的落盘值 —— 那个值是程序自己在
    /// [`TraeGatewayConfig::default`] 里写进去的（界面没有编辑入口），所以改写它
    /// 不会覆盖用户的任何选择。其他值（包括用户手改过的）一律不动。
    ///
    /// 刻意**只改内存不落盘**：落盘交给下一次 `save()`。这样「读一次配置」不会产生
    /// 写副作用（只读路径不该改文件），而任何一次真正的保存都会把新值带下去。
    fn migrate_legacy_default_model(&mut self) {
        if self.default_model.trim() == LEGACY_DEFAULT_MODEL {
            eprintln!(
                "[trae-gateway] 默认模型由历史值 {} 迁移为 {}（前者已不在客户端清单里；\
                 如需其它模型请手改 api_gateway.json 的 default_model）",
                LEGACY_DEFAULT_MODEL, TRAE_DEFAULT_MODEL
            );
            self.default_model = TRAE_DEFAULT_MODEL.to_string();
        }
    }

    /// 原子写回配置文件。
    pub fn save(&self) -> Result<(), String> {
        let file = paths::api_gateway_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &content).map_err(|error| error.to_string())
    }

    /// 对外 Base URL（不含 `/v1` 后缀，供接入指引展示）。
    pub fn base_url(&self) -> String {
        let host = if self.bind_addr.trim().is_empty() {
            "127.0.0.1"
        } else {
            self.bind_addr.trim()
        };
        format!("http://{host}:{}", self.port)
    }
}

/// 请求体上限（MB）→ 字节数；`0` 视为未设置并回落默认。
pub fn body_limit_bytes(max_body_mb: usize) -> usize {
    let mb = if max_body_mb == 0 {
        DEFAULT_MAX_BODY_MB
    } else {
        max_body_mb
    };
    mb.saturating_mul(1024 * 1024)
}

// ---------------------------------------------------------------------------
// 运行状态
// ---------------------------------------------------------------------------

/// 可 `Arc` 共享的 Trae 网关状态。
#[derive(Clone)]
pub struct TraeGatewayState {
    /// 运行配置。
    pub config: Arc<RwLock<TraeGatewayConfig>>,
    /// **按产品线分家的账号池**：Key 的归属（`variant`）决定用哪个池。
    ///
    /// 用 `HashMap<TraeVariant, TraePool>` 而非「一个池 + 每条目带 variant」：
    /// - 两条产品线的账号库、冷却文件、积分缓存**本就分家**（`*_for(variant)`），
    ///   分成两个池可以各自 `sync_for`，不必在选号里再过滤；
    /// - 避免「同一 uid 在两条线是两个不同账号」被错误合并
    ///   （`core::modules::trae::account` 的 `find_for` 注释）。
    pub pools: Arc<Mutex<HashMap<TraeVariant, TraePool>>>,
    /// 用户禁用的模型（P11）：**两个出口共用**（`/v1/models` 过滤 + 调用拒绝）。
    ///
    /// 进程内**唯一**实例（[`crate::disabled_models::shared`]）—— 与 WorkBuddy 网关、
    /// 命令面读的是同一个 `Arc`；按 `region`（[`region_of_variant`]）键控。
    pub disabled: Arc<RwLock<DisabledModels>>,
    /// **会话粘性绑定表**：同一会话优先复用上次成功的账号。
    ///
    /// 与 WorkBuddy 网关共用 [`StickyTable`] 实现。Trae 侧此前没有这一层 ——
    /// 多账号池下同一对话被换到另一个账号，在上游表现为**对话失忆**。
    ///
    /// 绑定**按产品线分家**：键里带 variant，两条产品线的同名会话不会互相污染
    /// （它们的账号库本就分家，粘到一起会指向另一条线的账号）。
    pub sticky: Arc<Mutex<StickyTable>>,
    /// 请求日志（与 WorkBuddy 网关同一个 [`RequestLog`]，只是换了落盘路径）。
    pub log: Arc<RequestLog>,
    /// 多 API Key 存储（哈希 + 前缀 + **归属产品线**；含旧 `settings.apiKey` 兼容读）。
    pub key_store: Arc<apikey::TraeApiKeyStore>,
    /// ★ 按密钥的用量账本（issue #76）—— 与 WB 侧**同一个实现**
    /// （`crate::key_usage::KeyUsageStore`），只是路径在 `trae/` 下。
    pub key_usage: Arc<crate::key_usage::KeyUsageStore>,
    /// 进程启动时刻（毫秒）。
    pub started_at: i64,
    /// 累计请求数。
    pub total_requests: Arc<AtomicU64>,
    /// 最近一次错误摘要。
    pub last_error: Arc<RwLock<Option<String>>>,
    /// 出站 HTTP 客户端。
    pub http: reqwest::Client,
    /// 请求体上限字节数（构造期固化，见 [`body_limit_bytes`]）。
    pub body_limit_bytes: usize,
    /// 出站上游基址，默认 [`TRAE_AGENT_HOST`]。
    ///
    /// 做成字段而不是直接引用常量，是为了让集成测试能把出站打到**本地 mock 上游**：
    /// 真实 Trae 需要有效 JWT，而鉴权、换号、SSE 转换这三块逻辑恰恰最需要端到端验证。
    /// 生产代码从不改写它，因此对外行为与写死常量完全一致。
    pub upstream: String,
}

impl TraeGatewayState {
    /// 依据配置构造一份完整运行状态。
    ///
    /// **不在此处自动生成 Key**：多 Key 化后 Key 只存哈希，自动生成的明文无处可取，
    /// 会造出一把「存在但无人知道明文」的幽灵 Key。改为由用户在「API 服务」页显式创建
    /// （[`apikey::create_response`]），或由 [`ensure_api_key`] 在需要时兜底生成。
    /// 老用户则经 [`apikey::TraeApiKeyStore::load`] 的 legacy 回落继续可用。
    pub fn new(config: TraeGatewayConfig) -> Self {
        let log = Arc::new(RequestLog::new(
            paths::api_gateway_log_file(),
            config.log_keep,
            config.log_bodies,
        ));
        Self {
            config: Arc::new(RwLock::new(config.clone())),
            pools: Arc::new(Mutex::new(HashMap::new())),
            disabled: crate::disabled_models::shared(),
            sticky: Arc::new(Mutex::new(StickyTable::new(config.sticky_ttl_ms))),
            log,
            key_store: Arc::new(apikey::TraeApiKeyStore::new(paths::api_gateway_keys_file())),
            key_usage: Arc::new(crate::key_usage::KeyUsageStore::new(
                paths::api_gateway_key_usage_file(),
            )),
            started_at: core_config::now_ms(),
            total_requests: Arc::new(AtomicU64::new(0)),
            last_error: Arc::new(RwLock::new(None)),
            http: build_http_client(),
            body_limit_bytes: body_limit_bytes(config.max_body_mb),
            upstream: TRAE_AGENT_HOST.to_string(),
        }
    }

    /// 读取配置快照。
    pub async fn config_snapshot(&self) -> TraeGatewayConfig {
        self.config.read().await.clone()
    }

    /// Max Mode 当前是否开启（**同步**，供 `/v1/models` 与出站注入使用）。
    ///
    /// 用 `try_read`：这两条路径都不是 async 上下文里的「配置主流程」，拿不到读锁时
    /// 按**开启**处理（宁可多注入一次、也不要因为一次瞬时锁竞争静默把 1M 关掉）。
    pub fn max_mode_now(&self) -> bool {
        self.config
            .try_read()
            .map(|config| config.max_mode)
            .unwrap_or(true)
    }

    /// 查询会话粘性绑定（已过期视为无绑定）。
    ///
    /// `key` 为 `None`（客户端未给会话 id）时返回 `None` —— 调用方据此**整段跳过**粘性。
    pub async fn sticky_uid(&self, key: Option<&str>) -> Option<String> {
        let key = key?;
        self.sticky
            .lock()
            .await
            .get(key, core_config::now_ms())
            .map(str::to_string)
    }

    /// 绑定会话粘性（**只在上游成功之后**调用）。
    ///
    /// `key` 为 `None` 时什么都不做 —— 绝不退化成轮级键（那会造出一个永不命中的绑定，
    /// 让「粘性会话数」看起来非 0 却毫无作用，见 [`crate::sticky::sticky_key`]）。
    pub async fn bind_sticky(&self, key: Option<&str>, uid: &str) {
        if let Some(key) = key {
            self.sticky
                .lock()
                .await
                .bind(key, uid, core_config::now_ms());
        }
    }

    /// 解绑会话粘性（该账号失败之后调用）。
    ///
    /// 粘性是优化而非约束：账号一失败就必须解绑，否则下一轮会先撞同一个死号。
    pub async fn unbind_sticky(&self, key: Option<&str>) {
        if let Some(key) = key {
            self.sticky.lock().await.unbind(key);
        }
    }

    /// 记一次请求（计数 + 日志 + 最近错误）。
    ///
    /// 日志条目字段名与 WorkBuddy 网关的 [`RequestMeta::to_value`] 对齐
    /// （`ts` / `endpoint` / `method` / `account` / `accountUid` / `model` / `status` /
    /// `latencyMs` / `promptTokens` / `completionTokens` / `stream`），只是多了 `error` 与
    /// `variant`。Trae 侧 `account` 本就是 uid，故 `accountUid` 与它同值（与 WB 侧
    /// 「`account`=展示名、`accountUid`=uid」的形状对齐，供 `token_daily` 统一精确归属）。
    /// 这样「Token 统计」页可以用同一套归一化逻辑聚合两侧日志，且能按变体过滤。
    ///
    /// [`RequestMeta::to_value`]: crate::logging::RequestMeta::to_value
    #[allow(clippy::too_many_arguments)]
    pub async fn record_request(
        &self,
        endpoint: &'static str,
        model: &str,
        status: u16,
        uid: &str,
        latency_ms: i64,
        stream: bool,
        prompt_tokens: u64,
        completion_tokens: u64,
        error: Option<String>,
        variant: TraeVariant,
        // ★ issue #76：调用方的 Key id（`None` = 无 Key 的调用 / 老条目）。
        key_id: Option<&str>,
        // ★ 归属台账（`"trae"` / `"unified"` / `None`）—— 统一网关转发来的请求
        //   带的是网关的 Key，没有这个标签会与本产品线的 Key 混淆。
        key_scope: Option<&'static str>,
    ) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.log.record(serde_json::json!({
            "ts": core_config::now_ms(),
            "endpoint": endpoint,
            "method": "POST",
            "account": uid,
            "accountUid": uid,
            "model": model,
            "status": status,
            "latencyMs": latency_ms,
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
            "stream": stream,
            "error": error,
            // 归属产品线：Token 统计页据此出「变体范围条」（筛选维度）。
            // 用 as_str() 的下划线形态，与 Key/查询参数口径一致（勿用派生 serde）。
            "variant": variant.as_str(),
            // ★ issue #76：调用方 Key id。**无 Key 写 `null`**（键固定，前端只判一个条件）；
            //   升级前写入的老条目确实没有这个键 ⇒ 前端两种都要容忍。
            "keyId": key_id,
            "keyScope": key_scope,
        }));
        if let Some(message) = error {
            *self.last_error.write().await = Some(message);
        }
    }
}

/// 出站客户端。
///
/// - `no_proxy()`：与 Trae 账号模块同理，绝不能走进本机 MITM 代理，否则请求会在
///   自己的代理里打转。
/// - 只设 `connect_timeout` 与 `read_timeout`，**不设总超时**：SSE 对话的合法时长
///   由模型输出长度决定，加总超时会把正常的长回答掐断。
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(20)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// 确保 Key 库里**至少有一把可用 Key**（load + legacy 优先，都没有才新建）。
///
/// 语义（T01）：返回**新建时**的明文（`Some`）；若已存在至少一把未吊销的 Key，
/// 则不新建、返回 `None` —— 明文不落库、不可复原，因此已有 Key 的明文无从返回。
///
/// 升级用户：`key_store.load()` 会回落 `settings.apiKey`，故这里不会给他们凭空造 Key，
/// 走的是「已存在 → 返回 None」分支，行为与升级前一致。
///
/// 新装用户：键库与 `settings.apiKey` 皆空 → 这里生成一把 `sk-trae-<32hex>` 并落库，
/// 明文由调用方（首启流程 / 管理命令）一次性呈现，之后不再可取。
pub fn ensure_api_key(store: &apikey::TraeApiKeyStore) -> Option<String> {
    if store.list().iter().any(|record| !record.is_revoked()) {
        return None;
    }
    let (_record, plaintext) = store.create("默认 Key".to_string(), TraeVariant::default());
    Some(plaintext)
}

/// 生成新 Key 明文：`sk-trae-` + 32 位 hex。
///
/// 仅用于测试 / 诊断的形态校验；正常发号走 [`apikey::TraeApiKeyStore::create`]
/// （它同时算出前缀与哈希并落库）。
pub fn generate_api_key() -> String {
    let secret = uuid::Uuid::new_v4().simple().to_string();
    format!("sk-trae-{secret}")
}

// ---------------------------------------------------------------------------
// 对外状态视图
// ---------------------------------------------------------------------------

/// `GET /api/trae/gateway/status` 的响应契约。
///
/// 字段命名与 WorkBuddy 网关的 `GatewayStatusView` 保持一致（snake_case），
/// 便于同一套前端归一化逻辑复用。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TraeGatewayStatusView {
    pub enabled: bool,
    pub running: bool,
    pub addr: Option<String>,
    pub base_url: String,
    pub bind_addr: String,
    pub port: u16,
    pub allow_non_loopback: bool,
    pub version: String,
    /// 累计请求数。
    pub total_requests: u64,
    /// 最近一次错误摘要。
    pub last_error: Option<String>,
    /// API Key 的脱敏展示（`sk-trae-abcd…`）。
    pub api_key_prefix: String,
    /// 账号池摘要。
    /// 池五态计数（形状与 WorkBuddy 侧 `GatewayStatusView::pools` 同源）。
    pub pool: PoolSummary,
}

impl TraeGatewayStatusView {
    /// 由配置派生；`running` / `addr` / `version` / 运行时字段由调用方补充。
    pub fn from_config(config: &TraeGatewayConfig) -> Self {
        Self {
            enabled: config.enabled,
            running: false,
            addr: None,
            base_url: config.base_url(),
            bind_addr: config.bind_addr.clone(),
            port: config.port,
            allow_non_loopback: config.allow_non_loopback,
            version: String::new(),
            total_requests: 0,
            last_error: None,
            api_key_prefix: String::new(),
            pool: PoolSummary::default(),
        }
    }
}

/// Key 脱敏：保留前缀与末 4 位，中间省略。
///
/// 返回的字符串**不包含**可复原的完整 Key，因此可安全下发到前端。
///
/// ## ⚠️ 一律按**字符**切，不按字节（安全审计 P2-18）
///
/// 改造前这里是 `&trimmed[..12]` / `&trimmed[trimmed.len() - 4..]` —— **字节**切片。
/// Trae 的 legacy Key 由用户从 `settings.json` 手填，未必是纯 ASCII，而字节切片
/// 落在多字节字符中间会**直接 panic**（`byte index is not a char boundary`）。
///
/// 生产路径早已改用 `trae/apikey.rs` 的 `legacy_prefix` / `legacy_suffix`
/// （它们就是按 `chars` 切的），所以这个函数只剩测试在用 —— 但「留着一个会 panic 的
/// 脱敏函数」本身是陷阱：谁按 doc 拿它做展示就会炸。现在两者口径完全一致
/// （`legacy_prefix` 取前 12 个字符、`legacy_suffix` 取末 4 个），
/// 测试里那条「两段式与整串脱敏逐段一致」也因此对非 ASCII 也成立。
pub fn mask_api_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let total = trimmed.chars().count();
    if total <= 12 {
        return format!("{}…", trimmed.chars().take(4).collect::<String>());
    }
    let head: String = trimmed.chars().take(12).collect();
    let tail: String = trimmed.chars().skip(total - 4).collect();
    format!("{head}…{tail}")
}

/// 当前 Unix 秒。
///
/// 池的冷却与积分到期时间都以**秒**为单位（与 `credits::expire_times` 一致），
/// 全模块统一从这里取，避免某处混进毫秒。
pub fn now_secs() -> i64 {
    chrono::Local::now().timestamp()
}

/// 组装管理面状态视图（Tauri 命令与 webui 路由**共用**）。
///
/// `running` / `addr` / `version` 由宿主提供——只有宿主知道监听是否真的起来了、
/// 应用版本是多少。池摘要与账号明细针对**指定变体**的池现算，因此两个宿主看到的永远是同一份。
///
/// **响应键集合不变**（`trae/mod.rs` 的 `status_view_keys_are_pinned` 护栏仍通过）：
/// `variant` 只是入参，不进入响应体——池摘要本身已隐含「这是哪条产品线」，
/// 多一个键反而要与前端重新对齐形状。
pub async fn status_view(
    state: &TraeGatewayState,
    running: bool,
    addr: Option<String>,
    version: &str,
    variant: TraeVariant,
) -> serde_json::Value {
    let now = now_secs();
    let config = state.config_snapshot().await;
    let mut view = TraeGatewayStatusView::from_config(&config);
    view.running = running;
    view.addr = addr;
    view.version = version.to_string();
    view.total_requests = state.total_requests.load(Ordering::Relaxed);
    view.last_error = state.last_error.read().await.clone();
    // 最近一把未吊销 Key 的前缀（缺省空串；明文不可复原，故只给前缀）。
    view.api_key_prefix = state
        .key_store
        .list()
        .into_iter()
        .filter(|record| !record.is_revoked())
        .max_by_key(|record| record.created_at)
        .map(|record| record.prefix)
        .unwrap_or_default();

    let (summary, accounts, diagnose) = {
        let mut pools = state.pools.lock().await;
        let pool = pools
            .entry(variant)
            .or_insert_with(|| TraePool::for_variant(variant));
        pool.sync_for(variant);
        (pool.summary(now), pool.status_list(now), pool.diagnose(now))
    };
    view.pool = summary;

    let mut body = match serde_json::to_value(&view) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    body.insert("accounts".into(), serde_json::json!(accounts));
    body.insert("diagnose".into(), serde_json::json!(diagnose));
    body.insert("upstream".into(), serde_json::json!(state.upstream));
    serde_json::Value::Object(body)
}

// ---------------------------------------------------------------------------
// 路由与监听
// ---------------------------------------------------------------------------

/// 组装 Trae 网关路由（**不含 fallback**）。
///
/// 挂载 `GET /health`（兼容 `/healthz`，见 [`crate::HEALTH_PATHS`]）、`GET /status`、
/// `GET /v1/models`、`POST /v1/chat/completions`、`POST /v1/messages`、`POST /v1/responses`。
/// 鉴权由 [`routes::bearer_auth`] 统一处理，健康检查免鉴权。
pub fn router(state: TraeGatewayState) -> axum::Router {
    use axum::routing::{get, post};
    let body_limit = state.body_limit_bytes;
    axum::Router::new()
        .merge(crate::health_routes(routes::health))
        .route("/status", get(routes::status))
        .route("/v1/models", get(routes::models))
        .route("/v1/chat/completions", post(routes::chat_completions))
        // Anthropic 协议入口：与上面那条共用整条中继，只在协议层做双向转换。
        .route("/v1/messages", post(routes::messages))
        // Responses 协议入口：同样只换协议层（入站 Responses → chat、出站 chat SSE
        // → Responses 事件流），中继与 `/v1/chat/completions` 是同一份。
        .route("/v1/responses", post(routes::responses))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            routes::bearer_auth,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .with_state(state)
    // 不设 fallback：宿主可能把它 merge 进更大的 Router。
}

/// 按配置启动独立监听。
///
/// 与 WorkBuddy 网关一致：只监听回环地址，除非显式允许；端口占用返回**可读错误**
/// 而不是 panic。
pub async fn spawn_listener(state: TraeGatewayState) -> anyhow::Result<GatewayHandle> {
    let config = state.config_snapshot().await;
    let bind_addr = config.bind_addr.clone();
    let port = config.port;

    let ip: IpAddr = bind_addr.parse().map_err(|_| {
        anyhow::anyhow!("Trae 网关监听地址无效：{bind_addr}（应为 IPv4/IPv6 字面量）")
    })?;

    if !ip.is_loopback() && !config.allow_non_loopback {
        return Err(anyhow::anyhow!(
            "已拒绝监听非回环地址 {bind_addr}：局域网内任何设备都可消耗你的 Trae 积分，\
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

    Ok(GatewayHandle::new(local, tx, join))
}

// ---------------------------------------------------------------------------
// 网关宿主泛化（P5 §2.4）—— Trae 产品线实现
// ---------------------------------------------------------------------------

/// Trae 网关产品线（[`crate::GatewayProduct`] 的 Trae 实现）。
///
/// **路径所有权**：`GET /health`、`GET /status`、`GET /v1/models`、
/// `POST /v1/chat/completions`、`POST /v1/messages`（见 [`router`]）。
///
/// ⚠️ 与 [`crate::WbGateway`] 的 `/v1/models` / `/v1/chat/completions` **逐字重叠** ⇒
/// 两者**各持一个 router、各自独立端口**（Trae 57894 / WorkBuddy 57891），**绝不 merge**
/// （axum 同路径 merge 会 panic）。见 [`crate::GatewayProduct`]。
pub struct TraeGateway;

/// Trae 网关的进程内共享状态（惰性初始化，读一次配置）。
static TRAE_SHARED_STATE: OnceLock<TraeGatewayState> = OnceLock::new();

impl crate::GatewayProduct for TraeGateway {
    type State = TraeGatewayState;

    const OWNED_PATHS: &'static [&'static str] = &[
        "/health",
        "/healthz",
        "/status",
        "/v1/models",
        "/v1/chat/completions",
        "/v1/messages",
    ];

    fn shared_state() -> TraeGatewayState {
        TRAE_SHARED_STATE
            .get_or_init(|| TraeGatewayState::new(TraeGatewayConfig::load()))
            .clone()
    }

    fn is_enabled(state: &TraeGatewayState) -> impl std::future::Future<Output = bool> + Send {
        // 克隆一份（`TraeGatewayState` 是 Arc 句柄，克隆极廉价）使返回的 future 不借 `state`。
        let state = state.clone();
        async move { state.config_snapshot().await.enabled }
    }

    fn router(state: TraeGatewayState) -> axum::Router {
        // `self::` 明确指向本模块的自由函数（与 trait 方法同名）。
        self::router(state)
    }

    fn spawn_listener(
        state: TraeGatewayState,
    ) -> impl std::future::Future<Output = anyhow::Result<GatewayHandle>> + Send {
        self::spawn_listener(state)
    }
}

/// Trae 网关请求日志文件路径（供管理命令透出）。
pub fn log_file() -> PathBuf {
    paths::api_gateway_log_file()
}

#[cfg(test)]
mod tests {

    /// ★★ 护栏：默认模型**只在恰好等于历史值时**被迁移（其余值一律不动）。
    ///
    /// 反例（改坏会红）：
    /// - 迁移条件写成 `contains` / `starts_with` ⇒ 用户手改的
    ///   `deepseek-v4-flash-x` 会被悄悄改成别的模型；
    /// - 干脆不迁移 ⇒ 老用户永远看到「默认 deepseek-v4-flash」这个客户端里
    ///   早已不存在的名字（issue #4「内容滞后」的最后一处）。
    #[test]
    fn legacy_default_model_is_migrated_narrowly() {
        assert_ne!(TRAE_DEFAULT_MODEL, LEGACY_DEFAULT_MODEL);
        assert_eq!(
            TraeGatewayConfig::default().default_model,
            TRAE_DEFAULT_MODEL,
            "默认配置必须直接用当前默认值"
        );

        let mut legacy = TraeGatewayConfig {
            default_model: LEGACY_DEFAULT_MODEL.to_string(),
            ..Default::default()
        };
        legacy.migrate_legacy_default_model();
        assert_eq!(
            legacy.default_model, TRAE_DEFAULT_MODEL,
            "恰好等于旧值 ⇒ 迁移"
        );

        // 其余值（用户手改过的、或只差一个字符的）**一律不动**。
        for keep in ["glm-5.3", "deepseek-v4-flash-x", "DeepSeek-V4-Flash", ""] {
            let mut config = TraeGatewayConfig {
                default_model: keep.to_string(),
                ..Default::default()
            };
            config.migrate_legacy_default_model();
            assert_eq!(
                config.default_model, keep,
                "只有恰好等于旧值才迁移：{keep:?}"
            );
        }
    }
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
    fn config_serializes_with_pinned_snake_case_keys() {
        let value = serde_json::to_value(TraeGatewayConfig::default()).expect("serialize config");
        let expected: BTreeSet<&str> = [
            "enabled",
            "bind_addr",
            "port",
            "allow_non_loopback",
            "log_keep",
            "log_bodies",
            "max_body_mb",
            "default_model",
            "max_rotate",
            "preferred_uid",
            "sticky_ttl_ms",
            "capture_enabled",
            "max_mode",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys_of(&value), expected, "config key set must be pinned");
    }

    #[test]
    fn config_defaults_are_loopback_and_off_by_default() {
        let config = TraeGatewayConfig::default();
        assert!(!config.enabled, "默认不得自动开始监听");
        assert!(config.max_mode, "Max Mode 默认开启（逐模型按目录上限）");
        assert_eq!(config.bind_addr, "127.0.0.1");
        assert!(!config.allow_non_loopback);
        assert_eq!(config.port, TRAE_DEFAULT_API_PORT);
        // ★ 2026-10-17：四条网关统一落在 5789x 一段，本端口必须与**同段**既有端口全部错开，
        //   否则同时启用会互相抢端口。
        assert_eq!(config.port, 57894, "Trae 网关默认端口应为 57894");
        for taken in [
            57890u16, // 界面本体
            57891,    // WorkBuddy 网关
            57892,    // 统一网关
            57893,    // 客户端会话监听 hook
            crate::qoder::QODER_DEFAULT_API_PORT,
            8899, // Trae 登录代理
        ] {
            assert_ne!(config.port, taken, "不得与既有端口 {taken} 撞车");
        }
        assert_eq!(
            config.base_url(),
            format!("http://127.0.0.1:{TRAE_DEFAULT_API_PORT}")
        );
        // 会话粘性默认 30 分钟，与 WorkBuddy 网关保持一致（同一管理面不该有两种默认）。
        assert_eq!(config.sticky_ttl_ms, 30 * 60 * 1000);
        assert!(config.preferred_uid.is_empty(), "默认不指定账号");
    }

    #[test]
    fn config_accepts_camel_case_aliases_and_missing_fields() {
        let config: TraeGatewayConfig = serde_json::from_str(
            r#"{"enabled":true,"bindAddr":"0.0.0.0","port":60001,"allowNonLoopback":true,
                "logKeep":10,"logBodies":true,"maxBodyMb":4,"defaultModel":"glm-5.3","maxRotate":5}"#,
        )
        .expect("camelCase aliases must deserialize");
        assert!(config.enabled);
        assert_eq!(config.bind_addr, "0.0.0.0");
        assert_eq!(config.port, 60001);
        assert!(config.allow_non_loopback);
        assert_eq!(config.log_keep, 10);
        assert!(config.log_bodies);
        assert_eq!(config.max_body_mb, 4);
        assert_eq!(config.default_model, "glm-5.3");
        assert_eq!(config.max_rotate, 5);

        let empty: TraeGatewayConfig = serde_json::from_str("{}").expect("empty must parse");
        assert_eq!(empty.port, TraeGatewayConfig::default().port);
    }

    #[test]
    fn body_limit_falls_back_to_default_when_zero() {
        assert_eq!(body_limit_bytes(0), DEFAULT_MAX_BODY_MB * 1024 * 1024);
        assert_eq!(body_limit_bytes(2), 2 * 1024 * 1024);
    }

    /// ★★ 安全审计 P2-18：脱敏函数**不得因多字节字符 panic**。
    ///
    /// 改造前按**字节**切片（`&trimmed[..12]`）⇒ 用户从 `settings.json` 手填的非 ASCII
    /// legacy Key 会在这里 `byte index is not a char boundary` 崩掉。
    /// 变异验证：改回字节切片 ⇒ 本用例 panic。
    #[test]
    fn api_key_mask_handles_multibyte_keys_without_panicking() {
        // 13 个中文字符：第 12 个字节边界落在字符中间。
        let key = "密钥密钥密钥密钥密钥密钥密";
        let masked = mask_api_key(key);
        // 前 12 个字符 + 省略号 + 末 4 个字符（13 字符的 Key）。
        assert_eq!(masked, "密钥密钥密钥密钥密钥密钥…钥密钥密");
        // 短串（≤ 12 字符）走另一条分支，同样不能 panic。
        let short = mask_api_key("短密钥");
        assert_eq!(short, "短密钥…", "不足 4 个字符时不能凭空补字");
        assert_eq!(mask_api_key("一二三四五六"), "一二三四…");
    }

    #[test]
    fn api_key_mask_never_leaks_the_middle() {
        let key = "sk-trae-0123456789abcdef0123456789abcdef";
        let masked = mask_api_key(key);
        assert!(masked.starts_with("sk-trae-0123"));
        assert!(masked.ends_with("cdef"));
        // 中段绝不出现在脱敏结果里。
        assert!(!masked.contains("456789abcdef0123456789ab"));
        assert!(masked.len() < key.len());

        assert_eq!(mask_api_key(""), "");
        assert_eq!(mask_api_key("short"), "shor…");
    }

    #[test]
    fn generated_api_key_has_stable_shape() {
        let key = generate_api_key();
        assert!(key.starts_with("sk-trae-"));
        assert_eq!(key.len(), "sk-trae-".len() + 32);
        assert!(key["sk-trae-".len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
        assert_ne!(key, generate_api_key(), "每次生成必须不同");
    }

    #[test]
    fn status_view_keys_are_pinned() {
        let view = TraeGatewayStatusView::from_config(&TraeGatewayConfig::default());
        let value = serde_json::to_value(&view).expect("serialize status");
        let expected: BTreeSet<&str> = [
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
        ]
        .into_iter()
        .collect();
        assert_eq!(keys_of(&value), expected, "status key set must be pinned");
        // 不得出现 camelCase 键。
        assert!(!value.as_object().unwrap().contains_key("baseUrl"));
        assert!(!value.as_object().unwrap().contains_key("apiKey"));
    }
}
