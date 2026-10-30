//! 模型降级链（Model Fallback）—— **主模型在上游明确失败时，改用备选模型重发一次**。
//!
//! ## 这个模块只做三件事
//!
//! 1. **存**：`~/.buddy-switch/gateway_model_fallback.json`，按作用域分家；
//! 2. **判**：给定主模型与一次失败，回答「该不该降、降到谁、下一跳是哪个」（[`plan_for`] /
//!    [`trigger_for`] 是**唯一**判断点）；
//! 3. **报**：给管理面一份带实时健康态的视图（[`ScopeFallbackView`]）。
//!
//! 真正的「重发」在 [`crate::routes::relay`] 的 hop 循环里 —— 本模块**不碰网络**。
//!
//! ## 为什么是独立文件，而不是塞进 `GatewayConfig`
//!
//! 三条理由，任何一条都足以否掉另外两种放法：
//!
//! 1. `GatewayConfig` 的键集被 `gateway_config_serializes_with_snake_case_keys` 精确钉死，
//!    加字段要改契约测试；且它是**运行配置**，而降级链是**用户偏好**（与 `disabled_models`
//!    同一分类，见 `docs/specs/` 的 §2.7 判定）。
//! 2. 放进 `Pool` 会继承「`Pool.config` 在 `GatewayState::new` 时固化、改完必须重启网关」
//!    的坑 —— 用户点保存后不生效且**没有任何提示**，是最难自查的一类缺陷。
//! 3. 三个网关各自持一份内存副本 ⇒ 「命令写到 A、网关读 B 的陈旧副本」的假开关
//!    （`disabled_models` 的模块文档已就此论证过一次）。
//!
//! ⇒ 独立文件 + 进程内唯一 [`shared`]，**保存即生效，零重启**。
//!
//! ## `triggers` 是白名单，不是「失败就降级」
//!
//! 只有 [`Trigger`] 里列出的几类会触发。**参数错 4xx / 内容审核拒绝 / 402 / Key 配额
//! 用尽固定不在白名单里**：
//!
//! - 参数错：换模型救不了，只会把同一个错误翻译成更陌生的模型名；
//! - **内容审核**：换模型等于换一套审核尺度，效果上就是帮调用方绕过审核，
//!   还会造成「同一段内容今天被拒、明天通过」这种无法解释的行为；
//! - 402 / Key 配额：账号或密钥层面的额度问题，换模型同样不解决。
//!
//! ## 与 `degrade` 的命名纪律
//!
//! 仓里 `degrade` 已有两个含义（`outbound/prompt.rs` 的提示词降级、`qoder/degrade.rs`
//! 的参数降级），本模块一律用 **`fallback`**，不得混用 —— 再加第三个含义必然读错。

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use buddy_switch_core::modules::catalog::CatalogStore;
use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::product::Scope;
use buddy_switch_core::modules::region::Region;

use crate::disabled_models::DisabledModels;
use crate::error::GatewayError;
use crate::pool::UpstreamEvent;

/// 一条链最多几个备选。**主 + 2 跳**是硬上限。
///
/// 每跳都要走完一整轮换号循环（`max_rotate` 默认 64，实际通常 1-3 次），延迟线性叠加；
/// 第 3 跳的实际成功率极低而成本确定。上限写死在一个常量里，避免出现
/// 「配了 3 个但只用前 2 个」的死配置。
pub const MAX_TARGETS: usize = 3;

/// 一次请求的**总上游请求数**预算（跨所有跳与所有账号轮次）。
///
/// ## 为什么是「总请求数」而不是「跳数上限」
/// 延迟由**请求次数**决定，不由跳数决定：3 跳 × 每跳 3 个号 = 9 次请求，
/// 与「2 跳 × 每跳 5 个号」是同一量级。按跳数限流会给出错误的直觉。
///
/// ## 为什么是 8
/// 失败多为 429 / 5xx（1–2s 内返回），8 次约 10–16s，落在客户端 30–60s 超时之内。
/// **超时档例外**：单次上游超时是 600s（`state.rs`），一次超时就吃满整个预算 ——
/// 这是已知的、刻意不在这里处理的限制（缩短超时会影响所有正常请求）。
pub const TOTAL_REQUEST_BUDGET: usize = 8;

/// 可触发降级的上游事件（**白名单**，见模块文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// 模型闸门暂停（跨请求证据）。**零成本**：一次上游请求都不发。
    Gate,
    /// 模型级限流（`6004`，`SoftRate { model_scoped: true }`）。
    ModelRate,
    /// 模型被封禁（`11102`）。
    ModelBlocked,
    /// 上游 5xx。
    Server,
    /// 请求超时。
    ///
    /// ⚠️ 当前**无法与 5xx 区分**（传输层失败统一 `status = 0, kind = Server`，
    /// 见 `crates/buddy-switch-core/src/modules/upstream.rs` 的失败分支）。这一档先占位：
    /// 判据补上之前，UI 里标「待支持」，`trigger_for` 不会返回它。
    Timeout,
    /// 上游 404（该账号的模型清单里没有它）。
    NotFound,
}

impl Trigger {
    /// 稳定标识（写进响应头 `X-Buddy-Fallback-Reason` 与错误体）。
    ///
    /// **英文标识符**而不是中文文案：客户端要能编程判断；中文解释只出现在 UI 与错误体里。
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Gate => "model_gate",
            Trigger::ModelRate => "model_rate",
            Trigger::ModelBlocked => "model_blocked",
            Trigger::Server => "server",
            Trigger::Timeout => "timeout",
            Trigger::NotFound => "not_found",
        }
    }
}

/// 默认全开的触发条件（卡片上**只标注超出这部分**的增量，见 `model-fallback-card.tsx`）。
pub fn baseline_triggers() -> BTreeSet<Trigger> {
    [Trigger::Gate, Trigger::ModelRate, Trigger::ModelBlocked]
        .into_iter()
        .collect()
}

/// 一条降级链：主模型 → 有序备选。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FallbackChain {
    /// 备选目标，按尝试顺序。超过 [`MAX_TARGETS`] 的部分在读取时截断。
    pub targets: Vec<FallbackTarget>,
    /// 触发条件白名单。**空集视为 [`baseline_triggers`]** —— 手写配置只填 `targets`
    /// 时也能工作，而不是「配了却不触发」这种最难自查的状态。
    pub triggers: BTreeSet<Trigger>,
    pub enabled: bool,
}

/// 一个备选目标：**哪个作用域**的**哪个模型**。
///
/// ## 为什么带 `scope`
/// 备选不限于同平台：`wb-cn:glm` 失败后可以降到 `wb-global:glm`（同平台跨区域，
/// 协议完全兼容）或 `trae-cn:claude`（跨平台，需要换 relay）。
/// 用 `Scope` 表达「平台 × 区域」比 `platform + region` 两个字段少一层分派。
///
/// ## 为什么 `scope` 是 `Option<String>` 而不是 `Option<Scope>`
/// 1. **向后兼容**：旧配置的 `targets` 是裸字符串数组（`["deepseek-v3"]`），
///    反序列化时补成 `None` = 「与入口同作用域」；
/// 2. **失败隔离**：未知作用域只让**这一跳**失效。用 `Scope` 作字段类型的话，
///    一个拼错的作用域会让**整份配置**反序列化失败 ⇒ `load()` 回落空表 ⇒
///    用户所有链凭空消失（与 `ModelFallback::scopes` 用 `String` 作键同一个理由）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackTarget {
    /// `None` = 与入口请求同作用域（旧配置 / 同平台同区域）。
    pub scope: Option<String>,
    /// 模型 id（目标作用域清单里的名字）。
    pub model: String,
}

impl FallbackTarget {
    /// 同作用域的备选（旧配置形态）。
    pub fn same_scope(model: impl Into<String>) -> Self {
        Self {
            scope: None,
            model: model.into(),
        }
    }

    /// 这一跳实际要去的作用域：`None` 时取入口作用域。
    pub fn scope_or(&self, entry: Scope) -> Scope {
        self.scope
            .as_deref()
            .and_then(|raw| Scope::parse(raw).ok())
            .unwrap_or(entry)
    }

    /// 该 target 是不是**跨作用域**（需要换 relay）。
    pub fn crosses_scope(&self, entry: Scope) -> bool {
        self.scope_or(entry) != entry
    }
}

impl Serialize for FallbackTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // 同作用域的目标序列化成**裸字符串**（保持旧配置可读、可手写）。
        match &self.scope {
            None => serializer.serialize_str(&self.model),
            Some(scope) => {
                use serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct("FallbackTarget", 2)?;
                state.serialize_field("scope", scope)?;
                state.serialize_field("model", &self.model)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for FallbackTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            /// 旧形态：`"deepseek-v3"`（同作用域）。
            Bare(String),
            /// 新形态：`{ "scope": "trae-cn", "model": "claude-4" }`。
            ///
            /// `scope` 可省略（前端「同作用域」就是 `{ model }`）——
            /// 省略等价于裸字符串，不必让调用方记两种形态。
            Scoped {
                #[serde(default)]
                scope: Option<String>,
                model: String,
            },
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Bare(model) => FallbackTarget::same_scope(model),
            Repr::Scoped { scope, model } => FallbackTarget { scope, model },
        })
    }
}

impl Default for FallbackChain {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            triggers: BTreeSet::new(),
            // 缺省**启用**：`triggers` 为空时退化成基线（安全），
            // 所以「手写一条链」不该还需要记得写 `enabled: true`。
            enabled: true,
        }
    }
}

impl FallbackChain {
    /// 生效的触发条件（空集 → 基线）。
    pub fn effective_triggers(&self) -> BTreeSet<Trigger> {
        if self.triggers.is_empty() {
            baseline_triggers()
        } else {
            self.triggers.clone()
        }
    }

    /// 清洗后的备选：去空白 + **大小写不敏感去重**。
    ///
    /// ⚠️ **不在这里截断** `MAX_TARGETS`。截断必须发生在「自环 / 与主模型同名」过滤**之后**
    /// （见 [`plan_for`]）—— 否则链里写一个与主模型同名的条目会白占一个配额，
    /// 把后面真正的备选挤掉（测试 `plan_is_capped_deduped_and_case_insensitive` 钉住这一点）。
    pub fn effective_targets(&self) -> Vec<FallbackTarget> {
        // ★ 去重键必须带 `scope`：`wb-cn:glm` 与 `trae-cn:glm` 是**两个不同的目标**，
        //   只按模型名去重会把跨平台的同名模型误吞掉。
        let mut seen: BTreeSet<(Option<String>, String)> = BTreeSet::new();
        let mut out = Vec::new();
        for target in &self.targets {
            let name = target.model.trim();
            if name.is_empty() {
                continue;
            }
            let key = (target.scope.clone(), name.to_ascii_lowercase());
            if !seen.insert(key) {
                continue;
            }
            out.push(FallbackTarget {
                scope: target.scope.clone(),
                model: name.to_string(),
            });
        }
        out
    }
}

/// 一个作用域的降级配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeFallback {
    /// 总开关。关掉后**配置保留但一次都不触发**（不是删除）。
    pub enabled: bool,
    /// 主模型 → 链。键保持**原始大小写**（UI 要按原样回显）；判断走大小写不敏感。
    pub chains: HashMap<String, FallbackChain>,
}

/// 全部作用域的降级配置（落盘根）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelFallback {
    pub version: u32,
    /// 键是作用域 id **字符串**（[`Scope::as_str`] 的字面量）。
    ///
    /// ⚠️ 用 `String`（**不是** `Scope`）作键：`Scope` 是枚举，未知键会让整份反序列化
    /// 失败 ⇒ `load()` 回落空表 ⇒ 一个未知键会**清空全部**降级配置。
    /// 改用字符串键后，未知键被原样保留、不影响已知键（与 `disabled_models` 同一个脚枪）。
    pub scopes: HashMap<String, ScopeFallback>,
}

impl ModelFallback {
    /// 从 `~/.buddy-switch/gateway_model_fallback.json` 读取；缺失 / 损坏回落空表。
    ///
    /// **不 panic、不报错**：配置坏了是小事，网关起不来是大事。
    pub fn load() -> Self {
        let file = file();
        std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<ModelFallback>(&text).ok())
            .unwrap_or_default()
    }

    /// 原子写回（与其它 store 同一套 `atomic_write`）。
    pub fn save(&self) -> Result<(), String> {
        let file = file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &content).map_err(|error| error.to_string())
    }

    /// 某作用域的配置（缺失即默认：**总开关关、无链**）。
    pub fn scope(&self, scope: Scope) -> ScopeFallback {
        self.scopes.get(scope.as_str()).cloned().unwrap_or_default()
    }

    /// 写入某作用域（保留其它作用域与未知键）。
    pub fn set_scope(&mut self, scope: Scope, value: ScopeFallback) {
        self.scopes.insert(scope.as_str().to_string(), value);
    }
}

/// 生效中的备选序列：`[主模型, 备选1, 备选2…]`。
///
/// **唯一判断点** —— 别处不要再写「哪些跳该试」的逻辑（复制即漂移的种子）。
///
/// 返回**只含主模型**时，调用方应直接走原有路径（不进入降级循环）。
pub fn plan_for(fb: &ModelFallback, scope: Scope, model: &str) -> Vec<FallbackTarget> {
    let mut plan = vec![FallbackTarget::same_scope(model.to_string())];
    let scoped = fb.scope(scope);
    if !scoped.enabled {
        return plan;
    }
    // 大小写不敏感地找链：上游对模型名大小写不敏感，精确匹配会让「配了 glm-5.3、
    // 客户端调 GLM-5.3」直接绕过降级（与 `is_disabled_in` 同一个理由）。
    let Some(chain) = scoped
        .chains
        .iter()
        .find(|(from, _)| from.eq_ignore_ascii_case(model.trim()))
        .map(|(_, chain)| chain)
    else {
        return plan;
    };
    if !chain.enabled {
        return plan;
    }
    for target in chain.effective_targets() {
        // `plan` 含主模型，所以到 MAX_TARGETS + 1 就停。
        if plan.len() > MAX_TARGETS {
            break;
        }
        // 目标与主模型同名**且同作用域** ⇒ 不自环。
        // ★ 跨作用域的同名模型**不是**自环：`wb-cn:glm` → `trae-cn:glm` 是两个不同的上游。
        if !target.crosses_scope(scope) && target.model.eq_ignore_ascii_case(model.trim()) {
            continue;
        }
        // 同一请求内每个 (作用域, 模型) 只试一次：链里写重了也只保留一份。
        if plan.iter().any(|seen| {
            seen.scope_or(scope) == target.scope_or(scope)
                && seen.model.eq_ignore_ascii_case(&target.model)
        }) {
            continue;
        }
        plan.push(target);
    }
    plan
}

/// 这次失败**是否值得降级**，以及归到哪个触发条件。
///
/// `None` = 不可降级，调用方应原样抛出原始错误。
///
/// ## 为什么不复用 `relay::is_model_level`
///
/// 那个函数只认 6004 / 11102，是**为了「计入闸门」而刻意收窄**的（账号级 429/402/5xx
/// 不该把整个模型停掉）。降级判据更宽（还含闸门本身、5xx、404），两者是不同的问题 ——
/// 宽窄差异是**刻意的**，不要合并。
pub fn trigger_for(error: &GatewayError, event: Option<&UpstreamEvent>) -> Option<Trigger> {
    // 闸门暂停没有上游事件（一次请求都没发）⇒ 先按错误类型判。
    if matches!(error, GatewayError::ModelGatePaused { .. }) {
        return Some(Trigger::Gate);
    }
    let event = event?;
    match event {
        UpstreamEvent::SoftRate {
            model_scoped: true, ..
        } => Some(Trigger::ModelRate),
        UpstreamEvent::ModelBlocked => Some(Trigger::ModelBlocked),
        UpstreamEvent::Server => Some(Trigger::Server),
        // 超时单独一档（`UpstreamErrorKind::Timeout` 由 `reqwest::Error::is_timeout()` 判定）：
        // 与 5xx 的处置不同 —— 换一个更快的模型有效，换号多半无用。
        UpstreamEvent::Timeout => Some(Trigger::Timeout),
        UpstreamEvent::NotFound => Some(Trigger::NotFound),
        // ★ 以下**刻意不降级**（见模块文档）：HardCredit / SessionDead / AuthRejected / Client。
        //   内容审核（11-128）落在 `Client`，也在这里被挡掉。
        UpstreamEvent::HardCredit
        | UpstreamEvent::SoftRate {
            model_scoped: false,
            ..
        }
        | UpstreamEvent::SessionDead
        | UpstreamEvent::AuthRejected
        | UpstreamEvent::Client => None,
    }
}

/// 该失败是否在链的触发条件白名单里。
pub fn allows(chain: &FallbackChain, trigger: Trigger) -> bool {
    chain.effective_triggers().contains(&trigger)
}

/// 取某主模型的链（大小写不敏感）。供中继在 hop 循环里查白名单。
pub fn chain_of(fb: &ModelFallback, scope: Scope, model: &str) -> Option<FallbackChain> {
    let scoped = fb.scope(scope);
    if !scoped.enabled {
        return None;
    }
    scoped
        .chains
        .iter()
        .find(|(from, _)| from.eq_ignore_ascii_case(model.trim()))
        .map(|(_, chain)| chain.clone())
        .filter(|chain| chain.enabled)
}

/// 降级目标的**实时**健康态（视图用，不参与判断）。
///
/// 与账号池那张表读同一份数据（闸门 + 禁用），所以卡片上的点和池里的点不会「这边绿那边黄」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    /// 可用。
    Healthy,
    /// 闸门暂停中（`remain_secs` 见视图的 `gated_for_secs`）。
    Gated,
    /// 用户禁用。
    Disabled,
    /// 不在该作用域的模型目录里（配置被手工改坏时的兜底）。
    Unknown,
}

/// 一个备选在视图里的样子。
#[derive(Debug, Clone, Serialize)]
pub struct TargetView {
    pub model: String,
    /// 该目标所在的作用域（`wb-cn` / `trae-cn` …）。
    ///
    /// 前端据此标注「跨平台」—— 跨作用域的备选会消耗**另一个平台**的账号额度，
    /// 这一点必须在卡片上说清，否则用户不知道额度去哪了。
    pub scope: String,
    pub state: TargetState,
    /// 闸门剩余秒数（仅 `Gated` 有意义）。
    pub gated_for_secs: Option<u64>,
    /// 目标相对**主模型**缺的能力（目前只有 `"image"`）。
    ///
    /// 只表达「主模型明确支持、目标明确不支持」这一种**静态**差异 ——
    /// 任一侧未知（不在目录里）就不写，绝不把「不知道」说成「不支持」。
    /// 请求级的实际跳过发生在中继（见 `routes::relay::fallback_skip_reason`）。
    pub caps_lost: Vec<&'static str>,
}

/// 一条链在视图里的样子。
#[derive(Debug, Clone, Serialize)]
pub struct ChainView {
    pub from: String,
    pub targets: Vec<TargetView>,
    pub triggers: Vec<Trigger>,
    pub enabled: bool,
}

/// 一条降级事件（**进程内环形缓冲**，不落盘）。
///
/// ## 为什么只保留最近 N 条、且不落盘
///
/// 它是**诊断**用的，不是统计口径。任何「近 7 天降级率」都必须来自账本新增的维度 ——
/// 拿环形缓冲算窗口统计是本仓踩过的坑（请求日志默认 200 条，繁忙时只覆盖十几分钟）。
/// 不落盘则是因为：重启后这条信息已经没有价值，而写盘会让每个失败请求多一次 IO。
#[derive(Debug, Clone, Serialize)]
pub struct FallbackEvent {
    /// 作用域（`wb-cn` / `trae-cn` / …）。
    ///
    /// ★ 流水**按作用域分**：三个平台的降级混在一张表里，用户根本分不清
    /// 哪条是 WorkBuddy 的、哪条是 Trae 的。
    pub scope: String,
    /// 发生时刻（毫秒墙钟）。
    pub at_ms: i64,
    /// 客户端原本请求的模型。
    pub from: String,
    /// 真正尝试过的备选（按顺序）。
    pub path: Vec<String>,
    /// 触发原因（[`Trigger::as_str`]）。
    pub reason: String,
    /// 最后一跳的账号（诊断用）。
    pub account: String,
    /// 本轮的墙钟耗时（毫秒）。
    pub ms: i64,
    /// `degraded`（降级成功）/ `failed`（备选也失败）/ `blocked`（按规则不降级）。
    pub result: &'static str,
}

/// 环形缓冲容量。
pub const MAX_EVENTS: usize = 50;

fn events() -> &'static std::sync::Mutex<std::collections::VecDeque<FallbackEvent>> {
    static EVENTS: OnceLock<std::sync::Mutex<std::collections::VecDeque<FallbackEvent>>> =
        OnceLock::new();
    EVENTS.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()))
}

/// 记一条降级事件（超出容量时丢最旧的）。
///
/// 用 `std::sync::Mutex` 而不是 tokio 的：临界区只有一次 push，不跨 `.await`，
/// 且调用点在热路径上 —— 异步锁在这里只会多一次调度。
pub fn record_event(event: FallbackEvent) {
    // 先落账本（**跨重启的窗口统计只认这个**），再进环形缓冲（给卡片看最近几条）。
    persist_to_ledger(&event);
    let Ok(mut queue) = events().lock() else {
        return;
    };
    if queue.len() >= MAX_EVENTS {
        queue.pop_front();
    }
    queue.push_back(event);
}

/// 把一条事件累加进**日账本**（`token_daily::fallback_days`）。
///
/// ## 为什么必须 spawn 到 blocking 池
///
/// 落账本是**文件读写**（`atomic_write` 整份替换）。调用点在请求路径上
/// （`relay_hops` 的成功分支），同步做会把这个请求的收尾卡在磁盘上 ——
/// 本仓的纪律是「慢 IO 一律 `spawn_blocking`」。
///
/// 没有 tokio 运行时（单元测试）时就地同步做：那里本来就没有请求在等。
fn persist_to_ledger(event: &FallbackEvent) {
    use buddy_switch_core::modules::token_daily::{record_fallback, FallbackResult};
    let Some(scope) = buddy_switch_core::modules::product::Scope::parse(&event.scope).ok() else {
        return;
    };
    let region = region_of(scope);
    let result = match event.result {
        "degraded" => FallbackResult::Degraded,
        "failed" => FallbackResult::Failed,
        // `blocked` = 压根没降级，账本不计（见 `FallbackDayCounts` 的文档）。
        _ => return,
    };
    let at_ms = event.at_ms;
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(move || {
                record_fallback(region, result, at_ms);
            });
        }
        Err(_) => {
            record_fallback(region, result, at_ms);
        }
    }
}

/// 记一条「降级成功」事件（薄封装：省得每个调用点拼一堆字段）。
///
/// Trae / Qoder 的中继拿不到完整的「试过哪些备选」（它们的换号轨迹不外露），
/// 所以 `path` 只记**最终用的那个** —— 宁可少记，不要编。
pub fn record_success(scope: Scope, from: &str, to: &str, trigger: Trigger, ms: i64) {
    record_event(FallbackEvent {
        scope: scope.as_str().to_string(),
        at_ms: crate::timeutil::now_ms(),
        from: from.to_string(),
        path: vec![to.to_string()],
        reason: trigger.as_str().to_string(),
        account: String::new(),
        ms,
        result: "degraded",
    });
}

/// 最近的事件（**新的在前**）。
pub fn recent_events() -> Vec<FallbackEvent> {
    let Ok(queue) = events().lock() else {
        return Vec::new();
    };
    queue.iter().rev().cloned().collect()
}

/// 管理面视图（命令返回值）。
#[derive(Debug, Clone, Serialize)]
pub struct ScopeFallbackView {
    pub scope: String,
    pub enabled: bool,
    pub chains: Vec<ChainView>,
    /// 生效上限，前端据此决定「+ 备选」是否还能点。
    pub max_targets: usize,
    /// 基线的触发条件（前端用它判断「哪些 tag 算增量」）。
    pub baseline: Vec<Trigger>,
    /// 最近的降级事件（新的在前）。
    ///
    /// ⚠️ 只有最近 [`MAX_EVENTS`] 条，**不参与任何统计口径** —— UI 必须把这句话写出来，
    /// 否则用户会把它当成功率的分母。
    pub events: Vec<FallbackEvent>,
}

/// 构造视图。`gates` 来自 [`crate::pool::Pool::model_gates`]。
pub fn build_view(
    fb: &ModelFallback,
    disabled: &DisabledModels,
    scope: Scope,
    catalogs: &CatalogStore,
    gates: &[(&str, i64)],
    now_ms: i64,
) -> ScopeFallbackView {
    let scoped = fb.scope(scope);
    let region = region_of(scope);
    // 目录只取一次（`current` 会 clone 整份快照，逐 target 取会放大成 N 份）。
    let snapshot = catalogs.current(region);
    let image_of = |model: &str| -> Option<bool> {
        let upstream = snapshot
            .models
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(model))
            .map(|entry| entry.supports_images);
        crate::model_capability::resolve_effective(scope, model, upstream).map(|cap| cap.supports)
    };
    let mut chains: Vec<ChainView> = scoped
        .chains
        .iter()
        .map(|(from, chain)| {
            let from_image = image_of(from);
            ChainView {
                from: from.clone(),
                targets: chain
                    .effective_targets()
                    .into_iter()
                    .take(MAX_TARGETS)
                    .map(|target| {
                        let model = target.model.clone();
                        let target_scope = target.scope_or(scope);
                        // 闸门列表是**入口作用域**的：跨作用域的目标查不到它的闸门，
                        // 只按禁用状态判（并在视图里带 `scope`，前端据此标注「跨平台」）。
                        let gate = if target_scope == scope {
                            gates
                                .iter()
                                .find(|(name, _)| name.eq_ignore_ascii_case(&model))
                        } else {
                            None
                        };
                        let state = if crate::disabled_models::is_disabled(
                            disabled,
                            target_scope,
                            &model,
                        ) {
                            TargetState::Disabled
                        } else if gate.is_some() {
                            TargetState::Gated
                        } else {
                            TargetState::Healthy
                        };
                        let gated_for_secs =
                            gate.map(|(_, until)| ((*until - now_ms).max(0) as u64).div_ceil(1000));
                        // 只有「主模型明确支持、目标明确不支持」才算冲突（见 TargetView 文档）。
                        let mut caps_lost = Vec::new();
                        if from_image == Some(true) && image_of(&model) == Some(false) {
                            caps_lost.push("image");
                        }
                        TargetView {
                            model,
                            scope: target_scope.as_str().to_string(),
                            state,
                            gated_for_secs,
                            caps_lost,
                        }
                    })
                    .collect(),
                triggers: chain.effective_triggers().into_iter().collect(),
                enabled: chain.enabled,
            }
        })
        .collect();
    // 稳定顺序：主模型名升序。HashMap 的迭代顺序每次进程重启都不同，
    // 不排序的话卡片上的链会「自己换位置」，用户会以为配置被改了。
    chains.sort_by(|a, b| a.from.cmp(&b.from));
    ScopeFallbackView {
        scope: scope.as_str().to_string(),
        enabled: scoped.enabled,
        chains,
        max_targets: MAX_TARGETS,
        baseline: baseline_triggers().into_iter().collect(),
        events: recent_events(),
    }
}

/// 写入前**规范化**一份作用域配置（Tauri 命令与 WebUI 共用，避免两处漂移）。
///
/// 做三件事：丢掉空 `from`、剔除与主模型同名的 target、去重后截断到 [`MAX_TARGETS`]。
/// 在**写盘之前**挡掉脏值，而不是等中继读到再兜底 —— 后者会让卡片上出现
/// 「一条指向自己的链」这种没人能解释的东西。
pub fn normalize(input: ScopeFallback) -> ScopeFallback {
    let mut normalized = input;
    normalized.chains.retain(|from, _| !from.trim().is_empty());
    for (from, chain) in normalized.chains.iter_mut() {
        // 去重键带 `scope`（同 `effective_targets` 的理由：跨平台同名模型是两个目标）。
        let mut seen: BTreeSet<(Option<String>, String)> = BTreeSet::new();
        chain.targets.retain(|target| {
            let name = target.model.trim();
            if name.is_empty() || name.eq_ignore_ascii_case(from.trim()) {
                return false;
            }
            seen.insert((target.scope.clone(), name.to_ascii_lowercase()))
        });
        chain.targets.truncate(MAX_TARGETS);
    }
    normalized
}

/// 预演的一步（管理面「模拟路径」用）。
#[derive(Debug, Clone, Serialize)]
pub struct PreviewStep {
    pub model: String,
    /// `primary` / `target`。
    pub role: &'static str,
    /// `try` / `skip_disabled` / `skip_gated`。
    pub action: &'static str,
    pub gated_for_secs: Option<u64>,
}

/// 保存前**预演**一次降级路径（**纯计算，零副作用**：只读闸门与禁用表）。
///
/// ## 为什么收在这里，而不是各宿主各写一份
///
/// 桌面命令（`src-tauri`）与 WebUI（`buddy-switch-server`）都要这个预演。
/// 两处各抄一份时，两个管理面迟早给出**不同的**模拟结果 —— 而用户会得出
/// 「预览是准的、线上是错的」这种最难排查的结论。
///
/// ## 为什么不判能力
///
/// 预演只看配置与当下健康态，**不知道真实请求带什么**（有没有图、有没有 tools）。
/// 图片跳过发生在中继里，按请求判（见 `routes::relay::fallback_skip_reason`）。
pub fn preview_payload(
    scope: Scope,
    from: &str,
    targets: &[String],
    disabled: &DisabledModels,
    gates: &[(&str, i64)],
    now_ms: i64,
) -> serde_json::Value {
    let mut steps = vec![PreviewStep {
        model: from.to_string(),
        // 第 1 步永远是主模型：它不看状态、总是先试（「不做粘性降级」的体现）。
        role: "primary",
        action: "try",
        gated_for_secs: None,
    }];
    for target in targets.iter().take(MAX_TARGETS) {
        let name = target.trim();
        if name.is_empty() {
            continue;
        }
        let gate = gates
            .iter()
            .find(|(model, _)| model.eq_ignore_ascii_case(name));
        let (action, gated_for_secs) = if crate::disabled_models::is_disabled(disabled, scope, name)
        {
            ("skip_disabled", None)
        } else if let Some((_, until)) = gate.filter(|(_, until)| *until > now_ms) {
            (
                "skip_gated",
                Some(((*until - now_ms).max(0) as u64).div_ceil(1000)),
            )
        } else {
            ("try", None)
        };
        steps.push(PreviewStep {
            model: name.to_string(),
            role: "target",
            action,
            gated_for_secs,
        });
    }
    serde_json::json!({ "ok": true, "scope": scope.as_str(), "steps": steps })
}

/// 某作用域近 `days` 天的降级统计（**落盘账本**，不是环形缓冲）。
///
/// 桌面命令与 WebUI 共用这一份 —— 两个管理面各写一份的话，同一天会给出两个数字。
pub fn stats_payload(scope: Scope, days: u32) -> serde_json::Value {
    let region = region_of(scope);
    let mut payload = buddy_switch_core::modules::token_daily::fallback_stats(region, days);
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "scope".to_string(),
            serde_json::Value::String(scope.as_str().to_string()),
        );
    }
    payload
}

/// `Scope` → 它所属的 `Region`（模型目录与上游协议都按 region 分家）。
///
/// 六个作用域全是 `<平台>-<cn|global>` 形状，后缀判定就够；不引入新映射表
/// 是为了避免「新增作用域时忘了同步」——那类漏法只在特定页面上才现形。
pub fn region_of(scope: Scope) -> Region {
    if scope.as_str().ends_with("-global") {
        Region::Global
    } else {
        Region::Cn
    }
}

/// 降级配置文件的路径（与 `gateway_config.json` 同目录）。
pub fn file() -> PathBuf {
    buddy_switch_core::modules::region::gateway_config_file()
        .with_file_name("gateway_model_fallback.json")
}

/// 进程内**唯一**的降级配置句柄：三个网关 + 命令面共用同一份（见模块文档）。
pub fn shared() -> Arc<RwLock<ModelFallback>> {
    static SHARED: OnceLock<Arc<RwLock<ModelFallback>>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(RwLock::new(ModelFallback::load())))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(targets: &[&str], triggers: &[Trigger]) -> FallbackChain {
        FallbackChain {
            targets: targets
                .iter()
                .map(|s| FallbackTarget::same_scope(*s))
                .collect(),
            triggers: triggers.iter().copied().collect(),
            enabled: true,
        }
    }

    /// 只取模型名做断言（作用域另有专门用例）。
    fn names(plan: &[FallbackTarget]) -> Vec<&str> {
        plan.iter().map(|target| target.model.as_str()).collect()
    }

    fn store(scope: Scope, scoped: ScopeFallback) -> ModelFallback {
        let mut fb = ModelFallback::default();
        fb.set_scope(scope, scoped);
        fb
    }

    #[test]
    fn plan_is_only_the_primary_when_disabled_or_missing() {
        let mut scoped = ScopeFallback {
            enabled: false,
            ..Default::default()
        };
        scoped
            .chains
            .insert("a".into(), chain(&["b", "c"], &[Trigger::Gate]));
        let fb = store(Scope::WbCn, scoped);
        assert_eq!(
            names(&plan_for(&fb, Scope::WbCn, "a")),
            vec!["a"],
            "总开关关 ⇒ 只主模型"
        );

        // 完全没有该作用域的配置。
        assert_eq!(
            names(&plan_for(&ModelFallback::default(), Scope::WbCn, "a")),
            vec!["a"]
        );
    }

    #[test]
    fn plan_is_capped_deduped_and_case_insensitive() {
        let mut scoped = ScopeFallback {
            enabled: true,
            ..Default::default()
        };
        // 3 个备选 + 1 个与主模型同名（自环）+ 1 个重复（大小写不同）。
        scoped.chains.insert(
            "A".into(),
            chain(&["b", "B", "a", "c", "d"], &[Trigger::Gate]),
        );
        let fb = store(Scope::WbCn, scoped);

        // 大小写不敏感命中主模型 "A"，且上限 2、去重、不自环。
        let plan = plan_for(&fb, Scope::WbCn, "a");
        assert_eq!(
            names(&plan),
            vec!["a", "b", "c", "d"],
            "上限 3 + 去重 + 不自环：{plan:?}"
        );
    }

    /// 跨作用域的目标：**同名不算自环**、去重键必须带 scope。
    ///
    /// 变异验证：把去重键退回「只看模型名」⇒ `trae-cn:glm` 会被 `wb-cn:glm` 吞掉，本用例红。
    #[test]
    fn cross_scope_targets_are_not_self_loops_and_dedupe_by_scope() {
        let mut scoped = ScopeFallback {
            enabled: true,
            ..Default::default()
        };
        scoped.chains.insert(
            "glm".into(),
            FallbackChain {
                targets: vec![
                    // 与主模型同名，但**另一个作用域** —— 不是自环。
                    FallbackTarget {
                        scope: Some("trae-cn".into()),
                        model: "glm".into(),
                    },
                    // 同作用域的重复项（大小写不同）⇒ 应被去重。
                    FallbackTarget::same_scope("deepseek"),
                    FallbackTarget::same_scope("DeepSeek"),
                    // 同平台跨区域。
                    FallbackTarget {
                        scope: Some("wb-global".into()),
                        model: "glm".into(),
                    },
                ],
                triggers: [Trigger::Gate].into_iter().collect(),
                enabled: true,
            },
        );
        let fb = store(Scope::WbCn, scoped);
        let plan = plan_for(&fb, Scope::WbCn, "glm");
        assert_eq!(
            names(&plan),
            vec!["glm", "glm", "deepseek", "glm"],
            "plan={plan:?}"
        );
        let scopes: Vec<&str> = plan
            .iter()
            .map(|t| t.scope_or(Scope::WbCn).as_str())
            .collect();
        assert_eq!(
            scopes,
            vec!["wb-cn", "trae-cn", "wb-cn", "wb-global"],
            "同名模型必须按作用域区分：{scopes:?}"
        );
    }

    #[test]
    fn disabled_chain_does_not_plan() {
        let mut scoped = ScopeFallback {
            enabled: true,
            ..Default::default()
        };
        let mut c = chain(&["b"], &[Trigger::Gate]);
        c.enabled = false;
        scoped.chains.insert("a".into(), c);
        let fb = store(Scope::WbCn, scoped);
        assert_eq!(names(&plan_for(&fb, Scope::WbCn, "a")), vec!["a"]);
    }

    #[test]
    fn empty_triggers_fall_back_to_baseline() {
        let c = FallbackChain {
            targets: vec![FallbackTarget::same_scope("b")],
            triggers: BTreeSet::new(),
            enabled: true,
        };
        assert_eq!(c.effective_triggers(), baseline_triggers());
        assert!(allows(&c, Trigger::Gate));
        assert!(!allows(&c, Trigger::Server), "5xx 不在基线里，必须显式勾");
    }

    /// ★ 红线：参数错 / 内容审核 / 402 / 401 / 会话失效 **一律不降级**。
    ///
    /// 变异测试：把 `UpstreamEvent::Client` 加进 `trigger_for` 的 Some 分支，这条必须红。
    #[test]
    fn never_fallback_for_client_credit_or_auth() {
        use UpstreamEvent::*;
        let err = GatewayError::BadRequest("bad".into());
        for event in [
            Client,
            HardCredit,
            SessionDead,
            AuthRejected,
            SoftRate {
                reset_at_ms: None,
                model_scoped: false,
            },
        ] {
            assert_eq!(
                trigger_for(&err, Some(&event)),
                None,
                "这些事件不得触发降级：{event:?}"
            );
        }
    }

    #[test]
    fn fallback_triggers_cover_the_whitelist() {
        use UpstreamEvent::*;
        let err = GatewayError::Internal("x".into());
        assert_eq!(
            trigger_for(
                &err,
                Some(&SoftRate {
                    reset_at_ms: None,
                    model_scoped: true
                })
            ),
            Some(Trigger::ModelRate)
        );
        assert_eq!(
            trigger_for(&err, Some(&ModelBlocked)),
            Some(Trigger::ModelBlocked)
        );
        assert_eq!(trigger_for(&err, Some(&Server)), Some(Trigger::Server));
        // 超时是独立一档：与 5xx 同处置、但降级判据不同（换模型有效、换号多半无用）。
        assert_eq!(trigger_for(&err, Some(&Timeout)), Some(Trigger::Timeout));
        assert_eq!(trigger_for(&err, Some(&NotFound)), Some(Trigger::NotFound));
    }

    #[test]
    fn gate_pause_needs_no_upstream_event() {
        let err = GatewayError::ModelGatePaused {
            model: "a".into(),
            remain_secs: 42,
            failures: 3,
            cooled_accounts: 0,
            attempts: Vec::new(),
        };
        assert_eq!(trigger_for(&err, None), Some(Trigger::Gate));
    }

    #[test]
    fn unknown_scope_keys_survive_round_trip() {
        let text = r#"{
            "version": 1,
            "scopes": {
                "wb-cn": { "enabled": true, "chains": { "a": { "targets": ["b"] } } },
                "future-scope": { "enabled": true, "chains": {} }
            }
        }"#;
        let fb: ModelFallback = serde_json::from_str(text).expect("必须能读进来");
        assert_eq!(names(&plan_for(&fb, Scope::WbCn, "a")), vec!["a", "b"]);
        let round = serde_json::to_string(&fb).unwrap();
        assert!(
            round.contains("future-scope"),
            "未知作用域必须原样保留：{round}"
        );
    }

    #[test]
    fn corrupted_file_falls_back_to_empty() {
        let fb: ModelFallback = serde_json::from_str("{ not json").unwrap_or_default();
        assert!(fb.scopes.is_empty());
        assert_eq!(names(&plan_for(&fb, Scope::WbCn, "a")), vec!["a"]);
    }

    #[test]
    fn view_marks_gated_and_disabled_targets() {
        let mut scoped = ScopeFallback {
            enabled: true,
            ..Default::default()
        };
        scoped
            .chains
            .insert("a".into(), chain(&["b", "c"], &[Trigger::Gate]));
        let fb = store(Scope::WbCn, scoped);

        let mut disabled = DisabledModels::default();
        disabled.set(Scope::WbCn, "c", true);

        let now = 1_000_000;
        // b 的闸门还有 42s。
        let gates = [("b", now + 42_000)];
        // 空目录 ⇒ 能力未知 ⇒ 不写 `caps_lost`（正是「不知道就不猜」的行为）。
        let catalogs = CatalogStore::with_cache_dir(std::env::temp_dir().join("bs-fb-view-test"));
        let view = build_view(&fb, &disabled, Scope::WbCn, &catalogs, &gates, now);
        assert_eq!(view.chains.len(), 1);
        let targets = &view.chains[0].targets;
        assert_eq!(targets[0].state, TargetState::Gated);
        assert_eq!(targets[0].gated_for_secs, Some(42));
        assert_eq!(targets[1].state, TargetState::Disabled);
    }
}
