//! Region 描述符：把国内版（CN）与国际版（Global）的全部差异收敛到一张表。
//!
//! 对照参考实现 `variants.ts`。CN 为既有默认版本，其 [`RegionSpec`] 字段值等于
//! 改造前的硬编码常量原值，保证 P0-1「CN 行为零变化」；所有模块内不再散落
//! `if international` 分支，一律查表派生。

use std::path::PathBuf;

use crate::modules::config::{home_dir, official_usage_cache_file, store_dir};

/// 目标版本。CN 为既有默认，Global 为国际版。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Cn,
    Global,
}

impl Region {
    /// 稳定的 region 标识（用于文件名与序列化）。
    pub fn as_str(self) -> &'static str {
        match self {
            Region::Cn => "cn",
            Region::Global => "global",
        }
    }

    /// 从字符串解析 region；未知返回 `None`。
    pub fn parse(s: &str) -> Option<Region> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cn" | "workbuddy" => Some(Region::Cn),
            "global" | "ai" | "workbuddy-ai" => Some(Region::Global),
            _ => None,
        }
    }

    /// 全部已知 region，供遍历用。
    pub fn all() -> [Region; 2] {
        [Region::Cn, Region::Global]
    }
}

impl Default for Region {
    fn default() -> Self {
        Region::Cn
    }
}

/// 统计查询范围：单版（cn / global）或合并（all）。
///
/// 与「实体归属」的 [`Region`] **正交**：`Region` 描述一条账号/凭据/目录真正属于哪一版，
/// 是认证、账号库、网关等模块的实体维度；`RegionFilter` 只描述**统计查询**要覆盖哪些版本，
/// 允许出现 `All`。刻意不往 `Region` 里加 `All`，因为那会污染所有
/// `match region { Cn | Global }` 的调用点，且 `region_spec(All)` / `accounts_file_for(All)`
/// 等语义无定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionFilter {
    Cn,
    Global,
    All,
}

impl RegionFilter {
    /// 稳定标识：`"cn" | "global" | "all"`。
    pub fn as_str(self) -> &'static str {
        match self {
            RegionFilter::Cn => "cn",
            RegionFilter::Global => "global",
            RegionFilter::All => "all",
        }
    }

    /// 解析：接受 [`Region`] 的既有别名（cn/workbuddy、global/ai/workbuddy-ai），
    /// 并额外接受 `"all" | "*" | "合并" | "全部"`。大小写不敏感、自动 trim；
    /// 未知返回 `None`。
    pub fn parse(s: &str) -> Option<RegionFilter> {
        let lowered = s.trim().to_ascii_lowercase();
        match lowered.as_str() {
            "all" | "*" | "合并" | "全部" => Some(RegionFilter::All),
            _ => Region::parse(&lowered).map(RegionFilter::from),
        }
    }

    /// 展开为需要聚合的具体 region 列表：`All -> [Cn, Global]`，其余为单元素。
    pub fn regions(self) -> Vec<Region> {
        match self {
            RegionFilter::Cn => vec![Region::Cn],
            RegionFilter::Global => vec![Region::Global],
            RegionFilter::All => vec![Region::Cn, Region::Global],
        }
    }

    /// 是否合并视图。
    pub fn is_all(self) -> bool {
        matches!(self, RegionFilter::All)
    }

    /// 单版过滤的 region（`All` 返回 `None`，调用方需走聚合路径）。
    pub fn single(self) -> Option<Region> {
        match self {
            RegionFilter::Cn => Some(Region::Cn),
            RegionFilter::Global => Some(Region::Global),
            RegionFilter::All => None,
        }
    }
}

impl From<Region> for RegionFilter {
    fn from(region: Region) -> Self {
        match region {
            Region::Cn => RegionFilter::Cn,
            Region::Global => RegionFilter::Global,
        }
    }
}

impl Default for RegionFilter {
    fn default() -> Self {
        RegionFilter::Cn
    }
}

/// 解析查询参数中的统计范围；缺省 / 空 / 未知一律回落 `Cn`（严格保持既有默认行为）。
///
/// 与 `buddy-switch-server` / `src-tauri` 里既有的 `parse_region` **绑定规则一致**：
/// `"ai"`/`"GLOBAL"` 绑定 `Global`，`""`/`"xx"` 绑定 `Cn`，并额外支持
/// `"all" | "*" | "合并" | "全部"`。该函数放在 core，供 server 与 tauri 复用。
pub fn parse_region_filter(value: Option<&str>) -> RegionFilter {
    value.and_then(RegionFilter::parse).unwrap_or(RegionFilter::Cn)
}

/// 目录请求 UA 形态：CN 用 CLI 形态，Global 用 App 形态（`WorkBuddyAI/<v>`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogUa {
    Cli,
    App,
}

/// 一个 region 的全部差异（对照参考实现 `variants.ts`）。
#[derive(Debug, Clone, Copy)]
pub struct RegionSpec {
    /// 所属 region。
    pub region: Region,
    /// 稳定 id："workbuddy" | "workbuddy-ai"。
    pub id: &'static str,
    /// 展示名："WorkBuddy" | "WorkBuddy AI"。
    pub display_name: &'static str,
    /// 诊断文案用的应用名。
    pub app_name: &'static str,
    /// 认证文件名：workbuddy-desktop.info | workbuddy-desktop-ai.info。
    pub auth_filename: &'static str,
    /// 认证文件路径环境变量名。
    pub auth_env: &'static str,
    /// 账号库文件名：accounts.json | accounts.global.json。
    pub accounts_filename: &'static str,
    /// 回收站文件名：accounts.trash.json | accounts.trash.global.json（P5 §2.7 软删除的落点）。
    pub accounts_trash_filename: &'static str,
    /// chat 上游基址。
    pub chat_base: &'static str,
    /// billing / 官网基址。
    pub billing_base: &'static str,
    /// 模型目录路径。
    pub models_path: &'static str,
    /// 目录请求 UA 形态。
    pub catalog_ua: CatalogUa,
    /// 内置兜底版本号。
    pub fallback_app_version: &'static str,
    /// 平台标识（上游 platform 参数）。
    pub platform: &'static str,
}

/// CN 描述符。字段值 = 现有硬编码常量原值（`billing_base` 取自
/// `WORKBUDDY_API_ENDPOINT = "https://www.codebuddy.cn"`）。
const CN_SPEC: RegionSpec = RegionSpec {
    region: Region::Cn,
    id: "workbuddy",
    display_name: "WorkBuddy",
    app_name: "WorkBuddy",
    auth_filename: "workbuddy-desktop.info",
    auth_env: "WORKBUDDY_AUTH_FILE",
    accounts_filename: "accounts.json",
    accounts_trash_filename: "accounts.trash.json",
    chat_base: "https://copilot.tencent.com",
    billing_base: "https://www.codebuddy.cn",
    models_path: "/console/enterprises/personal/models",
    catalog_ua: CatalogUa::Cli,
    fallback_app_version: "5.5.6",
    platform: "workbuddy",
};

/// Global（国际版）描述符。
const GLOBAL_SPEC: RegionSpec = RegionSpec {
    region: Region::Global,
    id: "workbuddy-ai",
    display_name: "WorkBuddy AI",
    app_name: "WorkBuddy AI",
    auth_filename: "workbuddy-desktop-ai.info",
    auth_env: "WORKBUDDY_AI_AUTH_FILE",
    accounts_filename: "accounts.global.json",
    accounts_trash_filename: "accounts.trash.global.json",
    chat_base: "https://www.workbuddy.ai",
    billing_base: "https://www.workbuddy.ai",
    models_path: "/v3/config",
    catalog_ua: CatalogUa::App,
    fallback_app_version: "5.5.2",
    platform: "workbuddy",
};

/// 取 region 的描述符。
pub fn region_spec(region: Region) -> &'static RegionSpec {
    match region {
        Region::Cn => &CN_SPEC,
        Region::Global => &GLOBAL_SPEC,
    }
}

/// region 的展示名（用于可读错误文案）。
pub fn region_display(region: Region) -> &'static str {
    region_spec(region).display_name
}

/// 登录域 → region。`workbuddy.ai` 及其子域为 Global，其余（含空域）为 CN。
///
/// 空域归 CN，与上游工具链的既有约定一致。
pub fn region_of(domain: &str) -> Region {
    let lowered = domain.trim().to_ascii_lowercase();
    if lowered == "workbuddy.ai" || lowered.ends_with(".workbuddy.ai") {
        Region::Global
    } else {
        Region::Cn
    }
}

/// 校验 domain 与目标 region 是否一致（安全红线 F）。
pub fn domain_matches(region: Region, domain: &str) -> bool {
    region_of(domain) == region
}

/// chat / 目录请求的 Origin / Referer 基址（对照参考实现 `originReferer`）。
pub fn origin_referer(region: Region) -> &'static str {
    region_spec(region).billing_base
}

/// 账号库文件路径：`accounts.json`（CN）/ `accounts.global.json`（Global）。
pub fn accounts_file_for(region: Region) -> PathBuf {
    store_dir().join(region_spec(region).accounts_filename)
}

/// 账号回收站文件路径：`accounts.trash.json`（CN）/ `accounts.trash.global.json`（Global）。
///
/// P5 §2.7 软删除的落点。与 [`accounts_file_for`] 同款：**枚举 → 查表**派生，
/// raw scope 字符串**从不**进 `PathBuf`（§前置 4 的路径派生唯一规则）。
pub fn accounts_trash_file_for(region: Region) -> PathBuf {
    store_dir().join(region_spec(region).accounts_trash_filename)
}

/// 网关配置文件路径：`~/.buddy-switch/gateway_config.json`。
pub fn gateway_config_file() -> PathBuf {
    store_dir().join("gateway_config.json")
}

/// 网关 API Key 文件路径：`~/.buddy-switch/gateway_keys.json`。
pub fn gateway_keys_file() -> PathBuf {
    store_dir().join("gateway_keys.json")
}

/// 目录缓存文件路径：`~/.buddy-switch/gateway_models.<region>.json`。
pub fn catalog_cache_file(region: Region) -> PathBuf {
    store_dir().join(format!("gateway_models.{}.json", region.as_str()))
}

/// 官方用量缓存文件路径。
///
/// CN 沿用既有 `official_usage_cache.json`（**零迁移**：老用户的缓存继续命中，
/// 不会因为这次 region 化而白跑一次采集）；Global 用 `official_usage_cache.global.json`
/// （与 `accounts.global.json` 同一命名习惯）。
///
/// **为什么必须分家**：该缓存是「一次性采集结果」，payload 里的 `accounts[]` 就是
/// 本次采集用的账号集合。若两版共用一个文件（或一份进程内记忆），先看 Global 视图
/// 再看 CN 视图时，CN 视图会**命中 Global 的采集结果**；而前端把
/// `officialUsage.accounts` 直接当作账号列表用（`CreditStatsPage` 的
/// `filterAccounts = official ? official.accounts : stats.accounts`），
/// 结果就是 CN 视图里列出 Global 账号——违反 PRD G1「两版互不污染」。
pub fn official_usage_cache_file_for(region: Region) -> PathBuf {
    match region {
        Region::Cn => official_usage_cache_file(),
        Region::Global => store_dir().join("official_usage_cache.global.json"),
    }
}

/// 每日 Token 记录（账号 × 来源 × 本地日）的 region 化落盘文件。
///
/// CN 用 `token_daily.json`，Global 用 `token_daily.global.json`，与
/// [`official_usage_cache_file_for`] 同一命名习惯。
///
/// **为什么必须分家**：记录的归属键是上游账号 `uid`，而两版的账号库（及客户端
/// 数据目录）各自独立；共用一个文件时，切换视图会让一版的当日用量被另一版读取或
/// 覆盖（同 [`official_usage_cache_file_for`] 踩过的坑）。
pub fn token_daily_file_for(region: Region) -> PathBuf {
    match region {
        Region::Cn => store_dir().join("token_daily.json"),
        Region::Global => store_dir().join("token_daily.global.json"),
    }
}

/// 账号库文件所在目录（与 `home_dir()` 一致），供诊断文案使用。
pub fn store_home() -> PathBuf {
    home_dir()
}

/// 凭据与目标 region 不符时返回；字段供 UI 直接渲染修复指引。
///
/// 安全红线 F：读取认证文件后若 `region_of(credential.domain) != 目标 region`，
/// 必须拒绝使用该凭据，且**不得发起任何上游请求**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionMismatch {
    /// 凭据实际所属域，例 `www.workbuddy.ai`。
    pub actual_domain: String,
    /// 目标 region 期望的认证文件名，例 `workbuddy-desktop.info`。
    pub expected_file: String,
    /// 目标 region 的认证路径环境变量名，例 `WORKBUDDY_AUTH_FILE`。
    pub env_var: String,
    /// 凭据实际所属 region。
    pub actual_region: Region,
    /// 目标 region。
    pub expected_region: Region,
}

impl RegionMismatch {
    /// 构造一条 region 不匹配错误。
    pub fn new(actual_domain: String, actual_region: Region, expected_region: Region) -> Self {
        let spec = region_spec(expected_region);
        Self {
            actual_domain,
            expected_file: spec.auth_filename.to_string(),
            env_var: spec.auth_env.to_string(),
            actual_region,
            expected_region,
        }
    }

    /// 可读的修复指引（UI 直接渲染）。
    pub fn message(&self) -> String {
        format!(
            "检测到 {} 的登录凭据出现在 {} 的认证文件位置（domain: {}）。\
             已拒绝使用该凭据。请把 {} 指向 {} 的登录态，或移除该文件。",
            region_display(self.actual_region),
            region_display(self.expected_region),
            self.actual_domain,
            self.env_var,
            region_display(self.expected_region),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_of_maps_global_and_cn_domains() {
        assert_eq!(region_of("www.workbuddy.ai"), Region::Global);
        assert_eq!(region_of("workbuddy.ai"), Region::Global);
        assert_eq!(region_of("app.workbuddy.ai"), Region::Global);
        assert_eq!(region_of("WWW.WORKBUDDY.AI"), Region::Global);

        assert_eq!(region_of("www.codebuddy.cn"), Region::Cn);
        assert_eq!(region_of("www.workbuddy.cn"), Region::Cn);
        assert_eq!(region_of(""), Region::Cn);
        assert_eq!(region_of("notworkbuddy.ai"), Region::Cn);
    }

    #[test]
    fn domain_matches_is_region_of_equality() {
        assert!(domain_matches(Region::Global, "www.workbuddy.ai"));
        assert!(domain_matches(Region::Cn, "www.codebuddy.cn"));
        assert!(!domain_matches(Region::Cn, "www.workbuddy.ai"));
        assert!(!domain_matches(Region::Global, "www.codebuddy.cn"));
    }

    #[test]
    fn cn_spec_keeps_hardcoded_constant_values() {
        let spec = region_spec(Region::Cn);
        assert_eq!(spec.billing_base, "https://www.codebuddy.cn");
        assert_eq!(spec.chat_base, "https://copilot.tencent.com");
        assert_eq!(spec.auth_filename, "workbuddy-desktop.info");
        assert_eq!(spec.accounts_filename, "accounts.json");
        assert_eq!(spec.accounts_trash_filename, "accounts.trash.json");
        assert_eq!(spec.models_path, "/console/enterprises/personal/models");
        assert_eq!(spec.catalog_ua, CatalogUa::Cli);
        assert_eq!(spec.fallback_app_version, "5.5.6");
    }

    #[test]
    fn global_spec_is_international() {
        let spec = region_spec(Region::Global);
        assert_eq!(spec.billing_base, "https://www.workbuddy.ai");
        assert_eq!(spec.chat_base, "https://www.workbuddy.ai");
        assert_eq!(spec.auth_filename, "workbuddy-desktop-ai.info");
        assert_eq!(spec.accounts_filename, "accounts.global.json");
        assert_eq!(spec.accounts_trash_filename, "accounts.trash.global.json");
        assert_eq!(spec.models_path, "/v3/config");
        assert_eq!(spec.catalog_ua, CatalogUa::App);
        assert_eq!(spec.fallback_app_version, "5.5.2");
    }

    #[test]
    fn region_parse_accepts_aliases() {
        assert_eq!(Region::parse("cn"), Some(Region::Cn));
        assert_eq!(Region::parse("WORKBUDDY"), Some(Region::Cn));
        assert_eq!(Region::parse("global"), Some(Region::Global));
        assert_eq!(Region::parse(" workbuddy-ai "), Some(Region::Global));
        assert_eq!(Region::parse("xx"), None);
    }

    #[test]
    fn region_filter_parse_accepts_aliases_and_all() {
        assert_eq!(RegionFilter::parse("cn"), Some(RegionFilter::Cn));
        assert_eq!(RegionFilter::parse("WORKBUDDY"), Some(RegionFilter::Cn));
        assert_eq!(RegionFilter::parse("global"), Some(RegionFilter::Global));
        assert_eq!(RegionFilter::parse("ai"), Some(RegionFilter::Global));
        assert_eq!(RegionFilter::parse("GLOBAL"), Some(RegionFilter::Global));
        assert_eq!(RegionFilter::parse(" workbuddy-ai "), Some(RegionFilter::Global));
        assert_eq!(RegionFilter::parse("all"), Some(RegionFilter::All));
        assert_eq!(RegionFilter::parse("ALL"), Some(RegionFilter::All));
        assert_eq!(RegionFilter::parse("*"), Some(RegionFilter::All));
        assert_eq!(RegionFilter::parse("合并"), Some(RegionFilter::All));
        assert_eq!(RegionFilter::parse("全部"), Some(RegionFilter::All));
        assert_eq!(RegionFilter::parse("xx"), None);
    }

    #[test]
    fn parse_region_filter_keeps_legacy_default_and_bindings() {
        // 与 api.rs / commands.rs 既有 `parse_region` 的绑定规则一致。
        assert_eq!(parse_region_filter(None), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("")), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("xx")), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("cn")), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("ai")), RegionFilter::Global);
        assert_eq!(parse_region_filter(Some("GLOBAL")), RegionFilter::Global);
        assert_eq!(parse_region_filter(Some("all")), RegionFilter::All);
        assert_eq!(parse_region_filter(Some("合并")), RegionFilter::All);
    }

    #[test]
    fn region_filter_maps_str_regions_and_single() {
        assert_eq!(RegionFilter::Cn.as_str(), "cn");
        assert_eq!(RegionFilter::Global.as_str(), "global");
        assert_eq!(RegionFilter::All.as_str(), "all");

        assert_eq!(RegionFilter::Cn.regions(), vec![Region::Cn]);
        assert_eq!(RegionFilter::Global.regions(), vec![Region::Global]);
        assert_eq!(RegionFilter::All.regions(), vec![Region::Cn, Region::Global]);

        assert_eq!(RegionFilter::from(Region::Cn), RegionFilter::Cn);
        assert_eq!(RegionFilter::from(Region::Global), RegionFilter::Global);

        assert_eq!(RegionFilter::Cn.single(), Some(Region::Cn));
        assert_eq!(RegionFilter::Global.single(), Some(Region::Global));
        assert_eq!(RegionFilter::All.single(), None);

        assert!(!RegionFilter::Cn.is_all());
        assert!(RegionFilter::All.is_all());
    }

    /// ★ 回收站文件路径必须**按 region 分家**且与 live 账号库**不同文件**（P5 §2.7）。
    ///
    /// 可证伪：把 `accounts_trash_file_for` 改成写死 CN 的文件名 ⇒ 两版回收站互相覆盖，
    /// 第二条断言变红（用户「删了 A 库的账号，却在 B 库的回收站里看到」）。
    #[test]
    fn accounts_trash_file_for_is_region_scoped_and_distinct_from_live() {
        let cn = accounts_trash_file_for(Region::Cn);
        let global = accounts_trash_file_for(Region::Global);

        assert_eq!(
            cn.file_name().and_then(|n| n.to_str()),
            Some("accounts.trash.json"),
            "CN 回收站文件名"
        );
        assert_eq!(
            global.file_name().and_then(|n| n.to_str()),
            Some("accounts.trash.global.json"),
            "Global 回收站文件名"
        );
        assert_ne!(cn, global, "两版回收站不得指向同一文件");
        // 与 live 账号库必须是**不同文件**，否则软删除会覆盖 live。
        assert_ne!(cn, accounts_file_for(Region::Cn));
        assert_ne!(global, accounts_file_for(Region::Global));
    }

    #[test]
    fn region_mismatch_message_names_file_and_env() {
        let mismatch = RegionMismatch::new(
            "www.workbuddy.ai".to_string(),
            Region::Global,
            Region::Cn,
        );
        let message = mismatch.message();
        assert!(message.contains("www.workbuddy.ai"), "{message}");
        assert!(message.contains("WORKBUDDY_AUTH_FILE"), "{message}");
        assert!(message.contains("WorkBuddy AI"), "{message}");
        assert_eq!(mismatch.expected_file, "workbuddy-desktop.info");
    }
}
