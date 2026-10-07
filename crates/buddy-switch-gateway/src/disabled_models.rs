//! 模型禁用偏好存储（P11）—— **两个出口共用的唯一判断点**。
//!
//! ## 为什么需要它（P11 的灵魂）
//!
//! 「禁用」要成立，**两个出口必须同时生效**：
//! - 出口 ①（`GET /v1/models`）：被禁用的模型**不出现**；
//! - 出口 ②（`POST /v1/chat/completions` / `POST /v1/messages`）：硬调用被**明确说明「已禁用」地拒**。
//!
//! 只改一个就是**假开关**（用户以为禁用了、实际还能调）。因此两个出口**读同一份数据、
//! 用同一个判断** —— 本模块的 [`is_disabled`] 是**唯一**判断点，**禁止**在别处复制逻辑
//! （复制即「两个出口会漂移」的种子）。
//!
//! ## 为什么独立文件（而不是塞进 `GatewayConfig`）
//!
//! 1. `GatewayConfig` 的键集被 `gateway_config_serializes_with_snake_case_keys` **精确钉死**，
//!    加字段要改契约测试；
//! 2. `disabled` 是**用户偏好**（§2.7），不是**运行配置**，两类东西别混；
//! 3. per-region 集合塞进扁平 config 形状别扭。
//!
//! 与既有 `account_strategy`（独立文件 `gateway_strategy.json`）**同构最自然**。
//!
//! ## 为什么是**进程内唯一**实例（`shared()`）
//!
//! WorkBuddy 网关与 Trae 网关是**两套独立实现**、各自一份 `GatewayState` / `TraeGatewayState`，
//! 而命令面（`set_model_disabled`）只应写一份。若两个网关各持一份内存副本，命令写到哪一份、
//! 另一个网关就会读到**陈旧的副本** —— 那正是「假开关」在 Trae 侧重现。
//! ⇒ 用一个进程内共享的 [`shared`]，让**两个网关 + 命令面**读到同一个 `Arc`。

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::{self, Region};

/// 按 region 的禁用模型集合（用户偏好，§2.7 不进 capabilities）。
///
/// 形状 `HashMap<String, BTreeSet<String>>`（键是 region 稳定标识 `"cn"` / `"global"`）——
/// **按 region 分家**：后端 `CatalogStore` / `GatewayState` 都是 region 粒度（`cn` / `global`），
/// 用 `wb-cn` 会引入一个后端不存在的维度。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisabledModels {
    /// 键是 region 的稳定标识（`"cn"` / `"global"`）。
    ///
    /// ⚠️ 用 `String`（**不是** `Region`）作键：`Region` 是枚举，**未知键会让整份反序列化失败**
    /// ⇒ `load()` 回落**空表** ⇒ 一个未知键（未来新增 region / 手改文件）会**清空全部** region
    /// 的禁用偏好（P11 T04 发现的脚枪）。改用字符串键后，未知键被**原样保留**、不影响已知键。
    regions: HashMap<String, BTreeSet<String>>,
}

impl DisabledModels {
    /// 从 `~/.buddy-switch/gateway_disabled_models.json` 读取；缺失 / 损坏回落空表。
    pub fn load() -> Self {
        let file = file();
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Ok(store) = serde_json::from_str::<DisabledModels>(&text) {
                return store;
            }
        }
        DisabledModels::default()
    }

    /// 原子写回。
    pub fn save(&self) -> Result<(), String> {
        let file = file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &content).map_err(|error| error.to_string())
    }

    /// 某 region 的禁用集合（只读；缺失即空集）。
    ///
    /// 用 `unwrap_or(empty_set())`（**值**，不是 `unwrap_or_else(闭包)`）：后者会让编译器把
    /// 返回类型统一成闭包的 `&'static`，进而要求 `&self` 的借用也活到 `'static`（E0521）。
    /// `empty_set()` 只是取一个 `OnceLock`，开销可忽略。
    pub fn get(&self, region: Region) -> &BTreeSet<String> {
        self.regions.get(region.as_str()).unwrap_or(empty_set())
    }

    /// 置位 / 清位；返回是否**发生了变化**（幂等）。
    ///
    /// 空白 `model_id` 视为无效输入，直接返回 `false`（不写脏数据）。
    pub fn set(&mut self, region: Region, model_id: &str, disabled: bool) -> bool {
        let id = model_id.trim();
        if id.is_empty() {
            return false;
        }
        let set = self.regions.entry(region.as_str().to_string()).or_default();
        if disabled {
            set.insert(id.to_string())
        } else {
            set.remove(id)
        }
    }
}

/// 共享的空集（`get` 找不到 region 时的回落）。
fn empty_set() -> &'static BTreeSet<String> {
    static EMPTY: OnceLock<BTreeSet<String>> = OnceLock::new();
    EMPTY.get_or_init(BTreeSet::new)
}

/// ★ **唯一判断点**（store + region 版）：两个出口（WB + Trae）都调它，**禁止在别处复制判断**。
pub fn is_disabled(store: &DisabledModels, region: Region, model_id: &str) -> bool {
    is_disabled_in(store.get(region), model_id)
}

/// 唯一判断点的**核心**：判断一个**已按 region 取出的**禁用集合是否命中某模型。
///
/// ## 为什么**大小写不敏感**（P11 T04 发现的真 bug）
///
/// 上游对模型名**大小写不敏感**：Trae 的 `trae::payload::model_config` 就是先
/// `name.to_lowercase()` 归一后再发给上游。若这里做**精确**匹配，用户禁用 `glm-5.3` 后，
/// 客户端调用 `GLM-5.3` 会**绕过**禁用（不命中 ⇒ 放行 ⇒ 上游接受）—— 正是 P11 要消灭的
/// 「假开关」。故判断一律走 [`eq_ignore_ascii_case`]。
///
/// ## 为什么**存储保持原始大小写**（而不是写入时就归一成小写）
///
/// `get()` 的返回值经命令面（`get_disabled_models`）喂给前端做 `disabledSet.has(model.id)`
/// **精确匹配**。若把存储归一成小写，混合大小写的模型（如 `DeepSeek-V4-Flash-Official`）在
/// UI 上会**显示成未禁用**（用户明明禁用了却看不出）—— 那是更糟的困惑。
/// ⇒ 只让**判断**大小写不敏感，**存储与 `get()` 保持原样**。
pub fn is_disabled_in(set: &BTreeSet<String>, model_id: &str) -> bool {
    let id = model_id.trim();
    // 空 / 空白 id 永远不命中（`set` 也不会写入它）。
    if id.is_empty() {
        return false;
    }
    set.iter().any(|candidate| candidate.eq_ignore_ascii_case(id))
}

/// 禁用模型的统一可读文案 —— WB（`GatewayError::ModelDisabled::message()`）与 Trae 两个出口
/// **共用同一句**，避免「同一个禁用两种说法」。
///
/// 必须**说明是「被禁用」**（不是笼统 4xx），并给出出路（§3.18.1 / R4）。
pub fn disabled_message(region: Region, model: &str) -> String {
    format!(
        "模型 `{model}`（{}）已在「模型」页被禁用，不会出现在 `/v1/models`；\
         如需调用请在「模型」页重新启用",
        region::region_display(region)
    )
}

/// 禁用偏好文件路径：`~/.buddy-switch/gateway_disabled_models.json`（与 `gateway_config.json` 同目录）。
pub fn file() -> PathBuf {
    region::gateway_config_file().with_file_name("gateway_disabled_models.json")
}

/// 进程内**唯一**的禁用存储句柄：WB 网关 / Trae 网关 / 命令面**共用同一份**（见模块文档）。
pub fn shared() -> Arc<RwLock<DisabledModels>> {
    static SHARED: OnceLock<Arc<RwLock<DisabledModels>>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(RwLock::new(DisabledModels::load())))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_is_idempotent_and_reports_change() {
        let mut store = DisabledModels::default();
        // 首次禁用 ⇒ 有变化
        assert!(store.set(Region::Cn, "glm-5.3", true));
        // 重复禁用 ⇒ 无变化
        assert!(!store.set(Region::Cn, "glm-5.3", true));
        assert!(is_disabled(&store, Region::Cn, "glm-5.3"));
        // 启用 ⇒ 有变化
        assert!(store.set(Region::Cn, "glm-5.3", false));
        // 重复启用 ⇒ 无变化
        assert!(!store.set(Region::Cn, "glm-5.3", false));
        assert!(!is_disabled(&store, Region::Cn, "glm-5.3"));
    }

    #[test]
    fn regions_are_independent() {
        let mut store = DisabledModels::default();
        store.set(Region::Cn, "m", true);
        // 禁用了 CN 的 m，不影响 Global 的 m —— 两个 region 是两套账号体系。
        assert!(is_disabled(&store, Region::Cn, "m"));
        assert!(!is_disabled(&store, Region::Global, "m"));
    }

    #[test]
    fn blank_id_is_rejected_and_trimmed_ids_match() {
        let mut store = DisabledModels::default();
        assert!(!store.set(Region::Cn, "   ", true), "空白 id 不得写入");
        assert!(!is_disabled(&store, Region::Cn, "   "));

        store.set(Region::Cn, "  glm-5.3  ", true);
        // 判断点统一 trim ⇒ 前后空格不影响命中。
        assert!(is_disabled(&store, Region::Cn, "glm-5.3"));
        assert!(is_disabled(&store, Region::Cn, "  glm-5.3 "));
    }

    #[test]
    fn empty_store_has_no_disabled_models() {
        let store = DisabledModels::default();
        assert!(!is_disabled(&store, Region::Cn, "anything"));
        assert!(store.get(Region::Cn).is_empty());
    }

    #[test]
    fn serde_round_trips_with_region_keys() {
        let mut store = DisabledModels::default();
        store.set(Region::Cn, "a", true);
        store.set(Region::Global, "b", true);
        let text = serde_json::to_string(&store).unwrap();
        // region 键是稳定的 `cn` / `global`（不是 Rust 的 `Cn`）。
        assert!(text.contains("\"cn\""), "{text}");
        assert!(text.contains("\"global\""), "{text}");
        let back: DisabledModels = serde_json::from_str(&text).unwrap();
        assert!(is_disabled(&back, Region::Cn, "a"));
        assert!(is_disabled(&back, Region::Global, "b"));
    }

    #[test]
    fn corrupted_or_missing_file_falls_back_to_empty() {
        // 损坏内容 ⇒ 回落空表（不 panic、不报错）。
        let store: DisabledModels = serde_json::from_str("{ not json").unwrap_or_default();
        assert!(!is_disabled(&store, Region::Cn, "x"));
    }

    #[test]
    fn disabled_message_names_the_model_and_says_disabled() {
        let message = disabled_message(Region::Cn, "glm-5.3");
        assert!(message.contains("glm-5.3"), "必须点名模型：{message}");
        assert!(message.contains("被禁用"), "必须说明是「被禁用」：{message}");
        assert!(message.contains("/v1/models"), "必须说明两个出口：{message}");
    }
}
