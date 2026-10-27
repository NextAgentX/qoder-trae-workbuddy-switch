//! 统一网关的运行配置（`~/.buddy-switch/gateway_unified.json`）。
//!
//! # 与 [`crate::state::GatewayConfig`] 的关系
//!
//! **同名的键一律同义**（`enabled` / `bind_addr` / `port` / `max_body_mb` …），
//! 刻意不发明第二套心智 —— 用户在 UI 上看到的是同一批概念。
//!
//! ## 有意**不含**两项（不是遗漏）
//!
//! - `allow_model_region_prefix`：那是 **WorkBuddy 独立端口**的 `cn:` / `global:` 跨域前缀开关。
//!   统一入口用的是**命名空间**（`core::modules::gateway_namespace`），两套前缀
//!   **不跨入口混用**（见该模块头）。
//! - `pool`：账号池参数属于**各产品线自己的配置**（WB / Trae 各一份）。统一入口不持池，
//!   它只做「按命名空间把请求交给对应产品线的中继」⇒ 池的治理参数仍由各自那份配置决定。
//!
//! ## 独有的两个键
//!
//! - `namespaces`：本入口**启用**哪些命名空间。未列出的：不解析、不列清单、调用返回 400。
//! - `default_namespace`：裸模型名落到哪个命名空间。**默认 `None` = 拒绝** ——
//!   统一入口下没有前缀就没有来源，静默挑一个是「看起来在工作、实际打错上游」的典型。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::gateway_namespace::{self, NamespaceSpec};

/// 默认监听端口。`5789x` 一段的分配：57890 界面本体 · 57891 WB 网关 · **57892 本入口** ·
/// 57893 hook · 57894 Trae 网关 · 57895 Qoder 网关；另有 8899 Trae 登录代理。
pub const DEFAULT_UNIFIED_PORT: u16 = 57892;
/// 请求体上限默认值（MB）—— 与 [`crate::state::GatewayConfig`] 同值。
pub const DEFAULT_MAX_BODY_MB: usize = 8;
/// 会话粘性默认有效期（30 分钟）。
pub const DEFAULT_STICKY_TTL_MS: i64 = 30 * 60 * 1000;

/// 统一网关配置。
///
/// 字段名为 snake_case，**缺失字段回落到 [`Default`]**（与 [`crate::state::GatewayConfig`] 同款）——
/// 老版本写下的配置文件、或用户手工精简过的文件都要能读，不能因为少一个键就整份回落默认值。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UnifiedGatewayConfig {
    /// 是否启用统一入口（默认**关**，与两个既有网关一致）。
    pub enabled: bool,
    /// 监听地址（默认仅本机）。
    pub bind_addr: String,
    /// 监听端口。
    pub port: u16,
    /// 是否允许非回环监听（要经过危险确认才置真）。
    pub allow_non_loopback: bool,
    /// 请求日志保留条数。
    pub log_keep: usize,
    /// 是否记录请求体（默认关）。
    pub log_bodies: bool,
    /// 每把 Key 的限流（每分钟；`None` = 不限）。
    pub per_key_rate_limit: Option<u32>,
    /// 单轮最大换号次数。
    pub max_rotate: usize,
    /// 是否做出站指纹脱敏。
    pub sanitize_fingerprints: bool,
    /// 系统提示词模式：`passthrough` / `custom`。
    pub prompt_mode: String,
    /// `custom` 模式下的提示词文件路径。
    pub prompt_file: Option<String>,
    /// 会话粘性绑定有效期（毫秒）。
    pub sticky_ttl_ms: i64,
    /// 请求体大小上限（MB）。★ 在 **router 构建期**固化 ⇒ 改值需重启。
    pub max_body_mb: usize,
    /// 启用的命名空间 id 列表。
    pub namespaces: Vec<String>,
    /// 裸模型名的默认命名空间；`None` = 拒绝。
    pub default_namespace: Option<String>,
}

impl Default for UnifiedGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "127.0.0.1".to_string(),
            port: DEFAULT_UNIFIED_PORT,
            allow_non_loopback: false,
            log_keep: 200,
            log_bodies: false,
            per_key_rate_limit: None,
            max_rotate: 3,
            sanitize_fingerprints: true,
            prompt_mode: "passthrough".to_string(),
            prompt_file: None,
            sticky_ttl_ms: DEFAULT_STICKY_TTL_MS,
            max_body_mb: DEFAULT_MAX_BODY_MB,
            // ★ 默认启用集由命名空间表给出（派生层 = `Scope::all()` 的每一条，
            //   当前 6 条），**不在这里抄字面量** —— 新增平台时默认集自动跟着走。
            namespaces: gateway_namespace::default_specs()
                .map(|spec| spec.id.to_string())
                .collect(),
            default_namespace: None,
        }
    }
}

impl UnifiedGatewayConfig {
    /// 从 `~/.buddy-switch/gateway_unified.json` 读取；缺失 / 损坏回落默认值。
    pub fn load() -> Self {
        let file = unified_config_file();
        if let Ok(text) = std::fs::read_to_string(&file) {
            if let Ok(config) = serde_json::from_str::<UnifiedGatewayConfig>(&text) {
                return config;
            }
            // 损坏时**明确说一句**：否则用户改了文件却不生效，只能猜。
            eprintln!(
                "[unified-gateway] 配置解析失败，已回落默认值：{}",
                file.display()
            );
        }
        Self::default()
    }

    /// 原子写回。
    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let file = unified_config_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &content).map_err(|error| error.to_string())
    }

    /// 校验：命名空间 id 必须在册、`default_namespace` 同理。
    ///
    /// ★ **保存时拒绝**（而不是静默丢弃）：用户填了一个拼错的 id，静默丢弃会让他
    /// 「明明加了却没生效」；明确报错并给出在册清单，一次就说清。
    pub fn validate(&self) -> Result<(), String> {
        for id in &self.namespaces {
            if gateway_namespace::parse(id).is_none() {
                return Err(format!(
                    "命名空间「{id}」不在册。当前在册：{}",
                    known_ids().join(" / ")
                ));
            }
        }
        if let Some(default) = &self.default_namespace {
            if gateway_namespace::parse(default).is_none() {
                return Err(format!(
                    "默认命名空间「{default}」不在册。当前在册：{}",
                    known_ids().join(" / ")
                ));
            }
        }
        Ok(())
    }

    /// 本入口**实际启用**的命名空间（按命名空间表的展示顺序，不是配置文件里数组的顺序）。
    ///
    /// 顺序取表 ⇒ UI 与 `/v1/models` 的分组顺序稳定，与用户编辑配置文件的顺序无关。
    ///
    /// ## 空列表的处置：**回落到默认集**（而不是「一个都不启用」）
    ///
    /// 空数组会让统一入口变成一个「怎么调都 400」的死入口，而用户本来只想「先清空再勾几个」——
    /// 中途保存过一次空配置就自锁了。UI 已经禁止取消最后一条，这里对**手改配置文件**
    /// 也做同样的兜底，并打一行说明（不静默）。
    pub fn enabled_namespaces(&self) -> Vec<&'static NamespaceSpec> {
        let enabled = self.effective_namespaces();
        // 只有「配置里确实一条都没落上」时才提示（否则每次 `/v1/models` 都会刷一行）。
        if enabled.len() != self.namespaces.len() || enabled.is_empty() {
            if self
                .namespaces
                .iter()
                .all(|id| !enabled.iter().any(|spec| spec.id == id))
            {
                eprintln!(
                    "[unified-gateway] namespaces 里没有任何在册项 ⇒ 回落默认集（{}），否则统一入口会变成「怎么调都 400」的死入口",
                    enabled.iter().map(|spec| spec.id).collect::<Vec<_>>().join(" / ")
                );
            }
        }
        enabled
    }

    /// **实际生效**的命名空间集合（唯一判据，**不打日志**）。
    ///
    /// ## 为什么必须与 `is_enabled` 同源（安全审计 P1-4）
    ///
    /// 改造前 `enabled_namespaces()` 对空列表回落到默认集，而 `is_enabled()` **只看原始
    /// 数组**，两个把关点（`unified/routes.rs` 的两处）用的都是后者。于是当
    /// `gateway_unified.json` 里 `namespaces` 是 `[]`（手改过、或写了个拼错的 id）：
    ///
    /// - `GET /v1/models` 与 `/unified/config` 列出**全部 6 条**（说明「可用」）；
    /// - 而任何请求都被 400 `namespace_not_enabled`，文案还指着那个说「含它」的设置页。
    ///
    /// ⇒ 一个**无法自我诊断**的死入口。这里把它收敛成一份：凡是问「启没启用」的地方
    /// 都读 [`Self::effective_namespaces`]。
    fn effective_namespaces(&self) -> Vec<&'static NamespaceSpec> {
        let enabled: Vec<&'static NamespaceSpec> = gateway_namespace::all_specs()
            .iter()
            .filter(|spec| self.namespaces.iter().any(|id| id == spec.id))
            .collect();
        if enabled.is_empty() {
            return gateway_namespace::default_specs().collect();
        }
        enabled
    }

    /// 是否启用了某个命名空间。
    ///
    /// ★ 与 [`Self::enabled_namespaces`] **同一判据**（含空列表回落）：两者若分家，
    /// 就会出现「清单说可用、每个请求都 400」的死入口。
    pub fn is_enabled(&self, id: &str) -> bool {
        self.effective_namespaces().iter().any(|spec| spec.id == id)
    }
}

/// 在册命名空间 id（用于错误文案，避免用户对着空白猜）。
pub fn known_ids() -> Vec<&'static str> {
    gateway_namespace::all_specs()
        .iter()
        .map(|spec| spec.id)
        .collect()
}

/// 统一网关配置文件路径。
pub fn unified_config_file() -> PathBuf {
    core_config::store_dir().join("gateway_unified.json")
}

/// 统一密钥文件路径。
pub fn unified_keys_file() -> PathBuf {
    core_config::store_dir().join("gateway_unified_keys.json")
}

/// 按密钥的用量账本（issue #76）：`~/.buddy-switch/gateway_unified_key_usage.json`。
///
/// 与 Key 台账**分文件**（同另三条产品线取舍）：台账低频写、用量每请求写。
pub fn unified_key_usage_file() -> PathBuf {
    core_config::store_dir().join("gateway_unified_key_usage.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 键集契约：**精确钉死**（照 `GatewayConfig` 的同款用例）。
    ///
    /// 加字段必须同步改本断言 —— 否则前端按 `snake_case` 读一个不存在的键会静默拿到
    /// `undefined`，界面上表现为「某个开关点了没反应」。
    #[test]
    fn unified_config_serializes_with_snake_case_keys() {
        let value = serde_json::to_value(UnifiedGatewayConfig::default()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        let mut expected = vec![
            "allow_non_loopback",
            "bind_addr",
            "default_namespace",
            "enabled",
            "log_bodies",
            "log_keep",
            "max_body_mb",
            "max_rotate",
            "namespaces",
            "per_key_rate_limit",
            "prompt_file",
            "prompt_mode",
            "port",
            "sanitize_fingerprints",
            "sticky_ttl_ms",
        ];
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }

    /// ★ **有意不含**那两项：`allow_model_region_prefix`（属 WB 独立端口）与 `pool`（属各产品线）。
    ///
    /// 可证伪：有人「顺手」把 `GatewayConfig` 的字段抄过来 ⇒ 本用例变红，
    /// 提醒他统一入口用的是命名空间、池参数由各产品线自己持有。
    #[test]
    fn unified_config_deliberately_omits_wb_only_and_pool_keys() {
        let value = serde_json::to_value(UnifiedGatewayConfig::default()).unwrap();
        let object = value.as_object().unwrap();
        for absent in ["allow_model_region_prefix", "pool"] {
            assert!(
                !object.contains_key(absent),
                "{absent} 不该出现在统一网关配置里（见本模块文档「有意不含两项」）"
            );
        }
    }

    /// 缺失字段回落默认值（老配置文件 / 手工精简过的文件都要能读）。
    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let config: UnifiedGatewayConfig = serde_json::from_str("{}").expect("空对象必须能解析");
        let default = UnifiedGatewayConfig::default();
        assert_eq!(config.enabled, default.enabled);
        assert_eq!(config.port, DEFAULT_UNIFIED_PORT);
        assert_eq!(config.bind_addr, default.bind_addr);
        assert_eq!(config.max_body_mb, DEFAULT_MAX_BODY_MB);
        assert_eq!(config.sticky_ttl_ms, default.sticky_ttl_ms);
        assert!(config.default_namespace.is_none(), "裸名默认必须是拒绝");
        assert_eq!(config.namespaces, default.namespaces);
    }

    /// 默认启用集 **= 派生层全部条目**（不硬编码字面量，避免与表漂移）。
    ///
    /// ★ 2026-10-15：Qoder 加入派生层 ⇒ 4 → **6 条**（3 平台 × 2 区域）。
    #[test]
    fn default_namespaces_are_the_derived_layer() {
        let config = UnifiedGatewayConfig::default();
        let ids: Vec<&str> = config.namespaces.iter().map(|s| s.as_str()).collect();
        let expected: Vec<&str> = gateway_namespace::default_specs().map(|s| s.id).collect();
        assert_eq!(ids, expected);
        assert_eq!(
            ids.len(),
            6,
            "派生层 = 3 平台 × 2 区域（Qoder 于 2026-10-15 加入）"
        );
        assert!(
            ids.contains(&"qoder-cn") && ids.contains(&"qoder-global"),
            "Qoder 两条必须在默认启用集里"
        );
        // 扩展层默认**不在**启用集里（老程序线的客户端已被合并版取代 ⇒ 会「假可用」）
        assert!(!ids.contains(&"traework-cn"));
        assert!(!ids.contains(&"traecode-global"));
    }

    /// `enabled_namespaces` 按**表顺序**返回，与配置文件里的书写顺序无关。
    #[test]
    fn enabled_namespaces_follow_table_order_not_file_order() {
        // 故意写反：先 trae-global 再 wb-cn
        let config = UnifiedGatewayConfig {
            namespaces: vec!["trae-global".into(), "wb-cn".into()],
            ..Default::default()
        };
        let ids: Vec<&str> = config.enabled_namespaces().iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["wb-cn", "trae-global"], "顺序应取自命名空间表");
    }

    /// ★ 空列表**回落默认集**（不是「一个都不启用」）—— 否则手改配置会自锁成死入口。
    #[test]
    fn empty_namespaces_fall_back_to_defaults() {
        let mut config = UnifiedGatewayConfig::default();
        config.namespaces.clear();
        let ids: Vec<&str> = config.enabled_namespaces().iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), 6, "空列表必须回落默认集，不能变成死入口");
    }

    /// 未在册的 id **在保存时报错**，并给出在册清单（用户一次就能改对）。
    #[test]
    fn unknown_namespace_is_rejected_with_the_known_list() {
        let config = UnifiedGatewayConfig {
            namespaces: vec!["wb-cn".into(), "wb-cnx".into()],
            ..Default::default()
        };
        let error = config.validate().expect_err("不在册的 id 必须被拒绝");
        assert!(error.contains("wb-cnx"), "应回显出错的那个 id：{error}");
        assert!(error.contains("wb-cn"), "应给出在册清单：{error}");

        // 默认命名空间同理
        // 变体标识，不是命名空间
        let config = UnifiedGatewayConfig {
            default_namespace: Some("trae".into()),
            ..Default::default()
        };
        assert!(
            config.validate().is_err(),
            "变体标识不该被接受为默认命名空间"
        );
    }

    /// ★★ 安全审计 P1-4：`is_enabled` 必须与 `enabled_namespaces` **同一判据**。
    ///
    /// 改造前这条用例断言的是 `assert!(!config.is_enabled("wb-cn"))`（清空后为 false），
    /// 而配套的 `empty_namespaces_fall_back_to_defaults` 断言「空表回落 6 条」——
    /// **两条测试各钉一半，正好把这个矛盾永久固化**：清单列 6 个可用、每个请求 400。
    ///
    /// 这与 #108 修复时发现的三条「用例存在、断言精确、方向反了」完全同型。
    ///
    /// 变异验证：把 `is_enabled` 改回 `self.namespaces.iter().any(...)` ⇒ 第一条断言红。
    #[test]
    fn is_enabled_agrees_with_enabled_namespaces_after_empty_fallback() {
        let mut config = UnifiedGatewayConfig::default();
        config.namespaces.clear();
        assert!(
            config.is_enabled("wb-cn"),
            "空列表会回落默认集 ⇒ 请求必须被放行（否则清单说可用、请求全 400）"
        );
        // 逐条对齐：`is_enabled` 与 `enabled_namespaces()` 对每个在册 id 的结论必须一致。
        let enabled: Vec<&str> = config.enabled_namespaces().iter().map(|s| s.id).collect();
        for id in known_ids() {
            assert_eq!(
                config.is_enabled(id),
                enabled.contains(&id),
                "`{id}` 在两个入口上的结论必须一致"
            );
        }

        // 只勾一个：此时**只有它**是启用的（不再回落）。
        config.namespaces = vec!["trae-global".into()];
        assert!(config.is_enabled("trae-global"));
        assert!(!config.is_enabled("wb-cn"));
    }

    /// 全部是**未在册**的 id（手写错）⇒ 同样回落默认集，而不是变成死入口。
    #[test]
    fn namespaces_with_only_unknown_ids_fall_back_to_defaults() {
        let config = UnifiedGatewayConfig {
            namespaces: vec!["wb-cnx".into(), "typo".into()],
            ..Default::default()
        };
        let ids: Vec<&str> = config.enabled_namespaces().iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), 6, "全是未知 id 等价于空列表");
        assert!(
            config.is_enabled("wb-cn"),
            "与 `enabled_namespaces` 同源 ⇒ 也必须是启用的"
        );
    }

    /// 落盘文件名是**对外事实**（文档与用户手改文件都用它）⇒ 钉住文件名，不是断言
    /// 「join 的结果等于 join 的结果」（那是恒真的自证）。
    #[test]
    fn file_names_match_the_documented_ones() {
        assert_eq!(
            unified_config_file().file_name().and_then(|n| n.to_str()),
            Some("gateway_unified.json")
        );
        assert_eq!(
            unified_keys_file().file_name().and_then(|n| n.to_str()),
            Some("gateway_unified_keys.json")
        );
        // 两者必须同在 store 根下（同一个父目录），否则「一份配置一处备份」的假设就破了
        assert_eq!(unified_config_file().parent(), unified_keys_file().parent());
    }
}
