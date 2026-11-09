//! Qoder 的**全部路径派生**。
//!
//! 分两类，**不要混**：
//!
//! | 类 | 根 | 谁写 |
//! |:--|:--|:--|
//! | **本仓自己的数据** | [`crate::modules::config::store_dir`]`/qoder/` | 本仓读写（账号库 / 台账 / 日志） |
//! | **官方客户端的凭据** | `~/.qoder*` · `%APPDATA%\com.qoder*.app.stable` | **只读**（本机导入） |
//!
//! ⚠️ **路径派生唯一规则**（`product.rs` 模块头）：作用域字符串 → 解析成枚举 → **查表**派生路径。
//! 本模块的每个函数都只收 [`QoderRegion`]（强类型枚举），**不收 `&str`** ——
//! 任何把原始 scope 串拼进 `PathBuf` 的写法都是把隔离根基拆掉。
//!
//! 数据目录布局（`~/.buddy-switch/qoder/`，`{region}` ∈ `cn` / `global`）：
//!
//! ```text
//! qoder/
//! ├── settings.json                      # 模块设置（不分家）
//! ├── accounts.{region}.json             # 账号库
//! ├── accounts.trash.{region}.json       # 账号回收站（软删除）
//! ├── account_cooldowns.{region}.json    # 签到错误冷却
//! ├── checkin_ledger.{region}.json       # 今日已签到台账（跨运行累积）
//! ├── credits_daily.{region}.json        # 每日积分快照（趋势图数据源）
//! └── logs/                              # 运行日志
//! ```
//!
//! ⚠️ **两个区域都带中缀**（不像 Trae 让「默认变体」沿用无后缀名）：
//! Qoder 是**全新平台、没有历史文件名要兼容**，统一带后缀才不会出现
//! 「凭什么 cn 特殊」的疑问。插入位置在**扩展名之前**，所以文件名仍以 `.json` 结尾。

use std::path::PathBuf;

use crate::modules::config::store_dir;
use crate::modules::qoder::region::{realm_spec, QoderRegion};

/// 本模块的数据目录：`<store_dir>/qoder/`。
pub fn qoder_dir() -> PathBuf {
    store_dir().join("qoder")
}

/// 把 `name.ext` 变成 `name.{region}.ext`（插入位置在扩展名之前）。
fn scoped_file_name(name_with_ext: &str, region: QoderRegion) -> String {
    match name_with_ext.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.{}.{ext}", region.as_str()),
        None => format!("{name_with_ext}.{}", region.as_str()),
    }
}

/// 按区域分家的数据文件路径。
fn scoped_file_for(name_with_ext: &str, region: QoderRegion) -> PathBuf {
    qoder_dir().join(scoped_file_name(name_with_ext, region))
}

/// 账号库。
pub fn accounts_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("accounts.json", region)
}

/// 账号回收站（软删除；与账号库同款按区域分家）。
pub fn accounts_trash_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("accounts.trash.json", region)
}

/// 签到错误冷却账本。
pub fn cooldowns_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("account_cooldowns.json", region)
}

/// 今日已签到台账（跨运行累积，按 uid 记）。
pub fn checkin_ledger_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("checkin_ledger.json", region)
}

/// 每日积分快照（趋势图数据源）。
pub fn credits_daily_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("credits_daily.json", region)
}

/// **自动签到**台账（每跑一轮一条，含「因窗口未刷新而推迟」的待办时刻）。
///
/// 与 [`checkin_ledger_file_for`] 刻意分开：那是「哪个 uid 今天签过」的**去重账本**，
/// 本文件是「排程那一轮做了什么」的**过程记录** —— 合并会让「跑过但无事可做」与
/// 「没跑」再次不可区分，而那正是用户报障「Qoder 总是没有自动签到」时最需要的信息。
pub fn checkin_logs_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("checkin_logs.json", region)
}

/// 网关**逐账号当日用量**（限额护栏 G2/G4 的计数来源；按区域分家）。
///
/// 归属网关（`buddy-switch-gateway::qoder::quota`）而不是账号库：它记的是**网关自己
/// 转发了多少**，与账号库（凭据事实）是两件事，写进账号库会把「凭据文件」变成账本。
pub fn gateway_usage_file_for(region: QoderRegion) -> PathBuf {
    scoped_file_for("gateway_usage.json", region)
}

/// 模块设置（**不分家**：端口 / 客户端路径 / 全局开关与区域无关）。
pub fn settings_file() -> PathBuf {
    qoder_dir().join("settings.json")
}

/// 网关配置（**不分家**：端口与全局开关与区域无关，出口按 Key 绑定）。
pub fn gateway_config_file() -> PathBuf {
    qoder_dir().join("gateway_config.json")
}

/// 网关密钥库（**只存 `sha256` 哈希 + 前缀，无明文** —— 与 WB / Trae 侧同款，
/// 因此入备份不构成凭据泄露）。
pub fn gateway_keys_file() -> PathBuf {
    qoder_dir().join("gateway_keys.json")
}

/// 按密钥的用量账本（issue #76）：`~/.buddy-switch/qoder/gateway_key_usage.json`。
///
/// 与 Key 台账**分文件**（同 WB / Trae 侧取舍）：台账低频写、用量每请求写。
pub fn gateway_key_usage_file() -> PathBuf {
    qoder_dir().join("gateway_key_usage.json")
}

/// 日志目录（**不分家**，文件名里带区域）。
pub fn logs_dir() -> PathBuf {
    qoder_dir().join("logs")
}

// ---------------------------------------------------------------------------
// 官方客户端的凭据路径（**只读**）
// ---------------------------------------------------------------------------

/// 用户主目录（跟随 `BUDDY_SWITCH_HOME` 的测试覆盖语义，见 `config::home_dir`）。
pub fn home_dir() -> PathBuf {
    crate::modules::config::home_dir()
}

/// `%APPDATA%`（Windows Roaming）。
///
/// 先读环境变量再回落 `dirs::config_dir()`：前者让集成测试可以**重定向**到隔离目录
/// （与 `auth_file.rs` 读 `LOCALAPPDATA` 的做法一致），后者保证真实运行可用。
pub fn roaming_dir() -> PathBuf {
    if let Ok(raw) = std::env::var("APPDATA") {
        if !raw.trim().is_empty() {
            return PathBuf::from(raw);
        }
    }
    dirs::config_dir().unwrap_or_else(|| home_dir().join("AppData").join("Roaming"))
}

/// Qoder CLI 的本地目录：`~/.qoder-cn` / `~/.qoder`。
pub fn cli_home_dir(region: QoderRegion) -> PathBuf {
    home_dir().join(realm_spec(region).cli_home_dir)
}

/// Qoder CLI 的凭据文件：`<cli_home>/.auth/user`。
///
/// ⚠️ 上游还支持带 profile 后缀的形态（`.auth/user.<profile>`），本仓**只读默认 profile**
/// —— 读多份 profile 会让「同一个 uid 出现多条凭据」，用户无法判断哪条在生效。
pub fn cli_auth_file(region: QoderRegion) -> PathBuf {
    cli_home_dir(region).join(".auth").join("user")
}

/// CLI 侧的模型目录缓存：`<cli_home>/.models/<uid>/`（第三层清单来源的落点）。
pub fn cli_models_cache_dir(region: QoderRegion, uid: &str) -> PathBuf {
    cli_home_dir(region).join(".models").join(uid)
}

/// 桌面 App 的 userData 目录：`%APPDATA%\com.qoder[cn].app.stable`。
pub fn desktop_app_dir(region: QoderRegion) -> PathBuf {
    roaming_dir().join(realm_spec(region).desktop_app_dir)
}

/// 桌面 App 的凭据文件：`<app_dir>/auth.v1.dat`（Chromium `v10` + AES-256-GCM）。
pub fn desktop_auth_file(region: QoderRegion) -> PathBuf {
    desktop_app_dir(region).join("auth.v1.dat")
}

/// 桌面 App 的 `Local State`（`os_crypt.encrypted_key` 的所在，DPAPI 解出 AES key 用）。
pub fn desktop_local_state_file(region: QoderRegion) -> PathBuf {
    desktop_app_dir(region).join("Local State")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文件名的**中缀插入位置**必须在扩展名之前（否则不再是 `.json`）。
    #[test]
    fn scoped_file_name_keeps_the_json_extension() {
        assert_eq!(
            scoped_file_name("accounts.json", QoderRegion::Cn),
            "accounts.cn.json"
        );
        assert_eq!(
            scoped_file_name("accounts.trash.json", QoderRegion::Global),
            "accounts.trash.global.json"
        );
        assert!(scoped_file_name("accounts.json", QoderRegion::Cn).ends_with(".json"));
        // 无扩展名时退化为追加后缀（不 panic、不丢内容）。
        assert_eq!(scoped_file_name("noext", QoderRegion::Cn), "noext.cn");
    }

    /// ★ 两个区域的账号库**必须是两个不同的路径**。
    ///
    /// 可证伪：把 `scoped_file_for` 改成不查区域，本用例立即变红 ——
    /// 那正是「国际版账号写进国内版库」的缺陷形态。
    #[test]
    fn the_two_regions_never_share_a_store_path() {
        // 本用例跨多次调用比对 `store_dir()` 派生的路径，读必须是**原子**的：
        // 并行用例改 `BUDDY_SWITCH_HOME` 时，中途换根会让 `accounts_file_for(..)`
        // 与 `qoder_dir()` 落到不同 home，随机红（全量里实测约一半概率）。
        let _lock = crate::modules::config::env_lock();
        for (name, a, b) in [
            (
                "账号库",
                accounts_file_for(QoderRegion::Cn),
                accounts_file_for(QoderRegion::Global),
            ),
            (
                "回收站",
                accounts_trash_file_for(QoderRegion::Cn),
                accounts_trash_file_for(QoderRegion::Global),
            ),
            (
                "冷却账本",
                cooldowns_file_for(QoderRegion::Cn),
                cooldowns_file_for(QoderRegion::Global),
            ),
            (
                "签到台账",
                checkin_ledger_file_for(QoderRegion::Cn),
                checkin_ledger_file_for(QoderRegion::Global),
            ),
            (
                "积分快照",
                credits_daily_file_for(QoderRegion::Cn),
                credits_daily_file_for(QoderRegion::Global),
            ),
            (
                "网关用量",
                gateway_usage_file_for(QoderRegion::Cn),
                gateway_usage_file_for(QoderRegion::Global),
            ),
        ] {
            assert_ne!(a, b, "{name} 在两区同路径 —— 会互相覆盖");
        }
        // 且都落在本模块的数据目录下（不是客户端目录）。
        assert!(accounts_file_for(QoderRegion::Cn).starts_with(qoder_dir()));
        // 设置与日志**刻意不分家**。
        assert_eq!(settings_file(), qoder_dir().join("settings.json"));
        assert_eq!(logs_dir(), qoder_dir().join("logs"));
    }

    /// 客户端侧路径必须由**区域表**派生（换区就换目录）。
    #[test]
    fn client_paths_follow_the_realm_table() {
        let cn_cli = cli_home_dir(QoderRegion::Cn);
        let global_cli = cli_home_dir(QoderRegion::Global);
        assert!(cn_cli.ends_with(".qoder-cn"), "{cn_cli:?}");
        assert!(global_cli.ends_with(".qoder"), "{global_cli:?}");
        assert_ne!(cn_cli, global_cli);

        assert!(cli_auth_file(QoderRegion::Cn).ends_with("user"));
        assert!(cli_auth_file(QoderRegion::Cn)
            .parent()
            .is_some_and(|p| p.ends_with(".auth")));

        assert!(desktop_app_dir(QoderRegion::Cn).ends_with("com.qodercn.app.stable"));
        assert!(desktop_app_dir(QoderRegion::Global).ends_with("com.qoder.app.stable"));
        assert!(desktop_auth_file(QoderRegion::Cn).ends_with("auth.v1.dat"));
        assert!(desktop_local_state_file(QoderRegion::Global).ends_with("Local State"));

        // 模型缓存目录含 uid 一层。
        assert!(cli_models_cache_dir(QoderRegion::Cn, "u-123")
            .ends_with(std::path::Path::new(".models").join("u-123")));
    }
}
