//! WorkBuddy **程序位表**（P5 §2.5）—— 表驱动 + 统一操作面。
//!
//! ## 为什么需要这一层
//!
//! WorkBuddy 侧有若干「程序位」（CodeBuddy CLI / CodeBuddy CN IDE / VS Code CodeBuddy 插件）：
//! 每一个都能「读状态」「把登录态切到某账号」，但它们的模块 API **各不相同**：
//!
//! | 程序位 | 模块 | 状态 | 切换 | 探测当前账号 | 安装辅助 |
//! |:---|:---|:---|:---|:---|:---|
//! | `codebuddy_cli` | [`crate::modules::codebuddy_cli`] | `status()` | `set_active_account()` | —— | `install_helper()` |
//! | `codebuddy_cn_ide` | [`crate::modules::codebuddy_cn_ide`] | `status()` | `switch_account()` | `detect_current_account()` | —— |
//! | `vscode_cn_inject` | [`crate::modules::vscode_cn_inject`] | （只读 secret） | —— | —— | —— |
//!
//! 改造前，宿主的每个命令都**硬编码**调用某一个具体模块函数 —— 新增一个程序位要同时改
//! 命令本体 / `generate_handler!` / server `.route` 三处，且极易漏改（症状是「构建全绿但
//! 桌面端 command not found」）。
//!
//! ## 表驱动：加程序位 = 加一行表项 + 一个 adapter impl
//!
//! 本模块把「**有哪些程序位**」收敛成一张表 [`PROGRAMS`]，把「**怎么操作一个程序位**」
//! 收敛成 [`ProgramAdapter`] trait。派发完全**按 id 查表**（[`status`] / [`switch`] /
//! [`detect`] / [`install_helper`]），**没有任何一处按 id 硬编码**。
//!
//! ⇒ **新增一个程序位只需**：① 在 [`PROGRAMS`] 加一行；② 写一个 `ProgramAdapter` impl。
//! **派发代码零改动**。这条性质是**可证伪**的：单测
//! `adding_a_slot_needs_no_dispatch_change` 往一张**合成表**里塞一个表外程序位，
//! 断言它无需任何派发改动即可被 [`status_in`] / [`switch_in`] / [`detect_in`] 服务。
//!
//! ## 做不到的操作走结构化 `Unsupported`（**不假装成功**）
//!
//! 三个程序位的操作面**本就不一致**（见上表）。强行凑一个「最小公共面」会把
//! 「这个程序位做不到」藏进 `Err` 或空值。正确做法是：做不到的操作返回
//! [`Unsupported`]（camelCase，前端可直接渲染成一条可操作提示），**绝不** `panic`、
//! **绝不**返回一个看似成功的空对象。trait 的 [`ProgramAdapter::detect`] /
//! [`ProgramAdapter::install_helper`] 默认实现就是这条策略。
//!
//! ## ⚠️ `vscode_cn_inject` 是**只读**程序位（诚实边界）
//!
//! 它是 CodeBuddy CN 客户端的**同源加密 secret 读写器**（被 `codebuddy_cn_ide` 复用），
//! **没有**独立的「切换账号 / 探测当前账号 / 安装辅助」操作面。因此它的 adapter 只实现
//! `status`（报告能否读到 secret），其余三个操作一律结构化 `Unsupported`。
//! 这不是漏做，而是**如实声明** —— 把它的「做不到」显式化，好过替它发明一个假操作面。

use serde_json::{json, Value};

use crate::modules::capability::Unsupported;
use crate::modules::{codebuddy_cli, codebuddy_cn_ide, jetbrains, vscode_cn_inject, vscode_ext};

/// 程序位支持的**统一操作**。用于 `Unsupported` 的 `capability` 字段与诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramOp {
    /// 读取状态。
    Status,
    /// 切换登录态。
    Switch,
    /// 探测当前登录账号。
    Detect,
    /// 安装 / 升级辅助。
    InstallHelper,
}

impl ProgramOp {
    /// 稳定标识（进 `Unsupported.capability`，前端据此分支渲染）。
    pub fn as_str(self) -> &'static str {
        match self {
            ProgramOp::Status => "status",
            ProgramOp::Switch => "switch",
            ProgramOp::Detect => "detect",
            ProgramOp::InstallHelper => "install_helper",
        }
    }
}

/// 一个程序位的**统一操作面**。
///
/// 差异（`set_active_account` vs `switch_account` vs 无）由各 impl 收敛到本 trait 面；
/// 做不到的操作返回结构化 [`Unsupported`]（见 [`ProgramAdapter::unsupported`]），**不 panic**。
///
/// `Sync` 约束是必需的：表以 `&'static dyn ProgramAdapter` 形式放进 `static`，跨线程共享。
pub trait ProgramAdapter: Sync {
    /// 程序位的人类可读名（诊断与 `Unsupported` 文案用；面向用户的展示名走
    /// [`ProgramSpec::label_key`] 对应的 i18n 键）。
    fn label(&self) -> &'static str;

    /// 读取该程序位的状态（**每个程序位都必须实现** —— 状态是共同面）。
    fn status(&self) -> Result<Value, String>;

    /// 把登录态切到指定账号。
    ///
    /// `restart` 表达「切换后是否重启客户端」；不支持该语义的程序位忽略它。
    fn switch(&self, account_id: &str, restart: Option<bool>) -> Result<Value, String>;

    /// 探测**当前**登录账号。
    ///
    /// 默认「做不到」⇒ 结构化 `Unsupported`（不假装成功）。例如 `codebuddy_cli`
    /// 没有独立的探测入口，它的当前账号由 `status` 一并给出。
    fn detect(&self) -> Result<Value, String> {
        Ok(self.unsupported(ProgramOp::Detect))
    }

    /// 安装 / 升级该程序位的辅助。
    ///
    /// 默认「做不到」⇒ 结构化 `Unsupported`。例如 `codebuddy_cn_ide` 无需安装辅助。
    fn install_helper(&self) -> Result<Value, String> {
        Ok(self.unsupported(ProgramOp::InstallHelper))
    }

    /// 构造一条结构化 `Unsupported`（camelCase，与 [`Unsupported::to_json`] 同形）。
    ///
    /// 供各 impl 在「做不到」时复用，避免每个程序位各拼一遍 JSON（字段名迟早漂移成
    /// `supportedOn` vs `supported_on`，前端静默拿不到值）。
    fn unsupported(&self, op: ProgramOp) -> Value {
        Unsupported::new(
            op.as_str(),
            self.label(),
            "已实现该操作的程序位",
            format!("程序位「{}」未提供「{}」操作。", self.label(), op.as_str()),
        )
        .to_json()
    }
}

// ---------------------------------------------------------------------------
// 三个程序位的 adapter impl
// ---------------------------------------------------------------------------

/// `codebuddy_cli` 程序位 —— 完整面：状态 / 切换 / 安装辅助。
///
/// ⚠️ 它**没有**独立的「探测当前账号」入口：当前账号由 `status()` 的
/// `activeAccountId` 一并给出，因此 `detect` 走 trait 默认的 `Unsupported`。
struct CodebuddyCliAdapter;

impl ProgramAdapter for CodebuddyCliAdapter {
    fn label(&self) -> &'static str {
        "CodeBuddy CLI"
    }

    fn status(&self) -> Result<Value, String> {
        // `codebuddy_cli::status()` 是**不可失败**的（返回裸 `Value`）；包成 `Ok` 以统一面。
        Ok(codebuddy_cli::status())
    }

    fn switch(&self, account_id: &str, _restart: Option<bool>) -> Result<Value, String> {
        // CLI 只切换认证配置、不重启客户端 ⇒ 忽略 `restart`。
        codebuddy_cli::set_active_account(account_id)
    }

    fn install_helper(&self) -> Result<Value, String> {
        codebuddy_cli::install_helper()
    }
}

/// `codebuddy_cn_ide` 程序位 —— 完整面：状态 / 切换 / 探测当前账号。
///
/// ⚠️ 它**没有**「安装辅助」入口（IDE 由用户自行安装），故 `install_helper` 走默认 `Unsupported`。
struct CodebuddyCnIdeAdapter;

impl ProgramAdapter for CodebuddyCnIdeAdapter {
    fn label(&self) -> &'static str {
        "CodeBuddy CN IDE"
    }

    fn status(&self) -> Result<Value, String> {
        Ok(codebuddy_cn_ide::status())
    }

    fn switch(&self, account_id: &str, restart: Option<bool>) -> Result<Value, String> {
        // IDE 切换的既有默认是「切换后重启客户端」（`restart` 缺省即 `true`）。
        codebuddy_cn_ide::switch_account(account_id, restart.unwrap_or(true))
    }

    fn detect(&self) -> Result<Value, String> {
        codebuddy_cn_ide::detect_current_account()
    }
}

/// `vscode_cn_inject` 程序位 —— **只读**：只有 `status`，其余操作结构化 `Unsupported`。
///
/// 见模块头「⚠️ `vscode_cn_inject` 是只读程序位」。
struct VscodeCnInjectAdapter;

impl ProgramAdapter for VscodeCnInjectAdapter {
    fn label(&self) -> &'static str {
        "VS Code CodeBuddy 插件"
    }

    fn status(&self) -> Result<Value, String> {
        // ⚠️ **只报告有无，绝不下发 secret 明文**（明文只在创建 / 注入时一次性使用）。
        let data_dir = vscode_cn_inject::codebuddy_cn_data_dir();
        // 读取失败（库被占用 / 损坏）**不当作程序位错误**：状态是只读探测，
        // 「读不到」本身就是一种可呈现的状态（`secretPresent: null` = 未知）。
        // 这样 `status` 不会因客户端没装 / 没登录 / 库被占而变成「程序位不可用」。
        let secret_present = match vscode_cn_inject::read_codebuddy_cn_secret(None) {
            Ok(Some(_)) => Some(true),
            Ok(None) => Some(false),
            Err(_) => None,
        };
        Ok(json!({
            "id": "vscode_cn_inject",
            "readOnly": true,
            "dataDir": data_dir.map(|path| path.to_string_lossy().to_string()),
            "secretPresent": secret_present,
        }))
    }

    fn switch(&self, _account_id: &str, _restart: Option<bool>) -> Result<Value, String> {
        // 该程序位没有独立的切换入口（secret 注入由 `codebuddy_cn_ide` 复用本模块完成）。
        Ok(self.unsupported(ProgramOp::Switch))
    }
}

/// `vscode_codebuddy_plugin` 程序位 —— VS Code 内 CodeBuddy 扩展：状态 / 切换 / 探测当前账号。
///
/// ⚠️ 它**没有**「安装辅助」入口（扩展由用户在 VS Code 内自行安装），故 `install_helper`
/// 走默认结构化 `Unsupported`。
struct VscodeCodebuddyPluginAdapter;

impl ProgramAdapter for VscodeCodebuddyPluginAdapter {
    fn label(&self) -> &'static str {
        "VS Code CodeBuddy 扩展"
    }

    fn status(&self) -> Result<Value, String> {
        // `vscode_ext::status()` 是**不可失败**的（返回裸 `Value`）；包成 `Ok` 以统一面。
        Ok(vscode_ext::status())
    }

    fn switch(&self, account_id: &str, restart: Option<bool>) -> Result<Value, String> {
        // 与 CN IDE 一致：切换的既有默认是「切换后重启编辑器」（`restart` 缺省即 `true`）。
        vscode_ext::switch_account(account_id, restart.unwrap_or(true))
    }

    fn detect(&self) -> Result<Value, String> {
        vscode_ext::detect_current_account()
    }
}

/// `jetbrains_codebuddy_plugin` 程序位 —— JetBrains IDE（IDEA / PyCharm）内 CodeBuddy 插件：
/// 状态 / 切换 / 探测当前账号。
///
/// ⚠️ 它**没有**「安装辅助」入口（插件由用户在 IDE 内自行安装），故 `install_helper`
/// 走默认结构化 `Unsupported`。
///
/// ⚠️ `ProjectAdapter::switch` 签名不携带「目标配置目录」选择，故这里对全部已装插件的
/// IDE 目录写入（`config_dirs = None`）—— 与参考实现的缺省行为一致。
struct JetbrainsCodebuddyPluginAdapter;

impl ProgramAdapter for JetbrainsCodebuddyPluginAdapter {
    fn label(&self) -> &'static str {
        "JetBrains CodeBuddy 插件"
    }

    fn status(&self) -> Result<Value, String> {
        Ok(jetbrains::status())
    }

    fn switch(&self, account_id: &str, restart: Option<bool>) -> Result<Value, String> {
        jetbrains::switch_account(account_id, restart.unwrap_or(true), None)
    }

    fn detect(&self) -> Result<Value, String> {
        jetbrains::detect_current_account()
    }
}

// ---------------------------------------------------------------------------
// 表（单一事实来源）
// ---------------------------------------------------------------------------

/// 一个**程序位**的描述符。
///
/// ⚠️ `label_key` 存**文案键而不是成品文案**：本表在模块加载时就定型，写中文会让
/// 语言切换对它失效（同 `platforms/registry.ts` 的 `labelKey` 处理）。
#[derive(Clone, Copy)]
pub struct ProgramSpec {
    /// 稳定标识（前端回传、命令参数用）：
    /// `"codebuddy_cli"` / `"codebuddy_cn_ide"` / `"vscode_cn_inject"`。
    pub id: &'static str,
    /// 展示名文案键（i18n 键，**不存成品文案**）。
    pub label_key: &'static str,
    /// 统一操作面的实现。
    pub adapter: &'static dyn ProgramAdapter,
}

impl ProgramSpec {
    /// 该程序位的展示名文案键（供宿主 / 前端查词表）。
    pub fn label_key(&self) -> &'static str {
        self.label_key
    }

    /// 人类可读名（诊断用；面向用户的展示名走 [`Self::label_key`]）。
    pub fn label(&self) -> &'static str {
        self.adapter.label()
    }
}

impl std::fmt::Debug for ProgramSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn ProgramAdapter` 不是 `Debug`，只打印描述符本身。
        f.debug_struct("ProgramSpec")
            .field("id", &self.id)
            .field("label_key", &self.label_key)
            .field("label", &self.adapter.label())
            .finish()
    }
}

/// CodeBuddy CLI 程序位。
static CODEBUDDY_CLI: CodebuddyCliAdapter = CodebuddyCliAdapter;
/// CodeBuddy CN IDE 程序位。
static CODEBUDDY_CN_IDE: CodebuddyCnIdeAdapter = CodebuddyCnIdeAdapter;
/// VS Code CodeBuddy 插件程序位（只读）。
static VSCODE_CN_INJECT: VscodeCnInjectAdapter = VscodeCnInjectAdapter;
/// VS Code CodeBuddy 扩展程序位（读写）。
static VSCODE_CODEBUDDY_PLUGIN: VscodeCodebuddyPluginAdapter = VscodeCodebuddyPluginAdapter;
/// JetBrains CodeBuddy 插件程序位（读写）。
static JETBRAINS_CODEBUDDY_PLUGIN: JetbrainsCodebuddyPluginAdapter = JetbrainsCodebuddyPluginAdapter;

/// WorkBuddy 的**全部程序位**（顺序即 UI 展示顺序）。
///
/// ★ **加程序位无需改派发代码**：在本表加一行 + 写一个 `ProgramAdapter` impl 即可，
/// [`status`] / [`switch`] / [`detect`] / [`install_helper`] 自动覆盖它。
/// 可证伪判据：单测 `adding_a_slot_needs_no_dispatch_change`。
pub static PROGRAMS: &[ProgramSpec] = &[
    ProgramSpec {
        id: "codebuddy_cli",
        label_key: "wbAccounts.program.codebuddyCli",
        adapter: &CODEBUDDY_CLI,
    },
    ProgramSpec {
        id: "codebuddy_cn_ide",
        label_key: "wbAccounts.program.codebuddyCnIde",
        adapter: &CODEBUDDY_CN_IDE,
    },
    ProgramSpec {
        id: "vscode_cn_inject",
        label_key: "wbAccounts.program.vscodeCnInject",
        adapter: &VSCODE_CN_INJECT,
    },
    ProgramSpec {
        id: "vscode_codebuddy_plugin",
        label_key: "wbAccounts.program.vscodeCodebuddyPlugin",
        adapter: &VSCODE_CODEBUDDY_PLUGIN,
    },
    ProgramSpec {
        id: "jetbrains_codebuddy_plugin",
        label_key: "wbAccounts.program.jetbrainsCodebuddyPlugin",
        adapter: &JETBRAINS_CODEBUDDY_PLUGIN,
    },
];

/// 全部程序位（供遍历用）。
pub fn all_programs() -> &'static [ProgramSpec] {
    PROGRAMS
}

/// 全部程序位 id（顺序稳定）。
pub fn program_ids() -> Vec<&'static str> {
    PROGRAMS.iter().map(|spec| spec.id).collect()
}

/// 按 id 取程序位描述符；未知 id 返回 `None`（供诊断 / 存在性判断）。
pub fn program_spec(id: &str) -> Option<&'static ProgramSpec> {
    PROGRAMS.iter().find(|spec| spec.id == id)
}

// ---------------------------------------------------------------------------
// 表驱动派发（`*_in` 接任意表；`*` 是 `PROGRAMS` 的便捷入口）
// ---------------------------------------------------------------------------

/// 按 id 在**给定表**里查找程序位（派发的**唯一实现** —— 无任何按 id 硬编码）。
///
/// `*_in` 系列接受任意表，是为了让单测能注入一张**合成表**证明「加一行即可」；
/// 宿主一律用 [`status`] / [`switch`] / [`detect`] / [`install_helper`]（接 [`PROGRAMS`]）。
fn find<'a>(table: &'a [ProgramSpec], id: &str) -> Result<&'a ProgramSpec, String> {
    table.iter().find(|spec| spec.id == id).ok_or_else(|| {
        let known: Vec<&str> = table.iter().map(|spec| spec.id).collect();
        format!(
            "未知的程序位「{}」：合法取值为 {}。",
            id,
            known.join(" / ")
        )
    })
}

/// 读取某程序位状态（表驱动）。
pub fn status_in(table: &[ProgramSpec], id: &str) -> Result<Value, String> {
    find(table, id)?.adapter.status()
}

/// 切换某程序位到指定账号（表驱动）。
pub fn switch_in(
    table: &[ProgramSpec],
    id: &str,
    account_id: &str,
    restart: Option<bool>,
) -> Result<Value, String> {
    find(table, id)?.adapter.switch(account_id, restart)
}

/// 探测某程序位当前登录账号（表驱动；做不到 ⇒ 结构化 `Unsupported`）。
pub fn detect_in(table: &[ProgramSpec], id: &str) -> Result<Value, String> {
    find(table, id)?.adapter.detect()
}

/// 安装 / 升级某程序位的辅助（表驱动；做不到 ⇒ 结构化 `Unsupported`）。
pub fn install_helper_in(table: &[ProgramSpec], id: &str) -> Result<Value, String> {
    find(table, id)?.adapter.install_helper()
}

/// 读取程序位状态（[`PROGRAMS`] 便捷入口）。
pub fn status(id: &str) -> Result<Value, String> {
    status_in(PROGRAMS, id)
}

/// 切换程序位账号（[`PROGRAMS`] 便捷入口）。
pub fn switch(id: &str, account_id: &str, restart: Option<bool>) -> Result<Value, String> {
    switch_in(PROGRAMS, id, account_id, restart)
}

/// 探测程序位当前账号（[`PROGRAMS`] 便捷入口）。
pub fn detect(id: &str) -> Result<Value, String> {
    detect_in(PROGRAMS, id)
}

/// 安装 / 升级程序位辅助（[`PROGRAMS`] 便捷入口）。
pub fn install_helper(id: &str) -> Result<Value, String> {
    install_helper_in(PROGRAMS, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表外程序位（**合成**）的 adapter —— 用来证明「加一行 + 一个 impl」即可被派发。
    struct MockAdapter;

    impl ProgramAdapter for MockAdapter {
        fn label(&self) -> &'static str {
            "Mock"
        }

        fn status(&self) -> Result<Value, String> {
            Ok(json!({ "ok": true, "id": "mock" }))
        }

        fn switch(&self, account_id: &str, restart: Option<bool>) -> Result<Value, String> {
            Ok(json!({ "ok": true, "switchedTo": account_id, "restart": restart }))
        }
    }

    static MOCK: MockAdapter = MockAdapter;

    /// 合成表：只多了一行 + 一个 impl，**没有**任何派发代码改动。
    static SYNTHETIC: &[ProgramSpec] = &[ProgramSpec {
        id: "mock",
        label_key: "mock.label",
        adapter: &MOCK,
    }];

    /// ★★ **本用例就是「加第 3 个程序位无需改代码」的证明**（可证伪）。
    ///
    /// 它往一张**表外**的合成表里放一个全新程序位，并断言**同一套** [`status_in`] /
    /// [`switch_in`] / [`detect_in`] 无需任何修改即可服务它。
    ///
    /// 可证伪：若派发里出现任何按 id 的硬编码（例如 `match id { "codebuddy_cli" => … }`），
    /// 本用例立即变红 —— 那正是「新增程序位要改派发代码」的缺陷形态。
    #[test]
    fn adding_a_slot_needs_no_dispatch_change() {
        // 状态 / 切换：走合成 adapter 的实现。
        let status = status_in(SYNTHETIC, "mock").expect("合成程序位应可读状态");
        assert_eq!(status["id"], json!("mock"));

        let switched = switch_in(SYNTHETIC, "mock", "u-1", Some(true)).expect("合成程序位应可切换");
        assert_eq!(switched["switchedTo"], json!("u-1"));
        assert_eq!(switched["restart"], json!(true));

        // 探测：合成 adapter 未实现 ⇒ trait 默认的结构化 `Unsupported`（不 panic、不假装成功）。
        let detected = detect_in(SYNTHETIC, "mock").expect("默认 detect 应返回 Unsupported 而非 Err");
        assert_eq!(detected["capability"], json!("detect"));
        assert!(detected.get("ok").is_none(), "做不到就不能带 ok:true");

        // 未知 id：可读 `Err`（而不是 panic / 静默空值）。
        let error = status_in(SYNTHETIC, "nope").unwrap_err();
        assert!(error.contains("nope"), "{error}");
        assert!(error.contains("mock"), "错误文案应列出合法取值：{error}");
    }

    /// 表钉住五个程序位（顺序稳定）。**移除任一项 ⇒ 本用例变红**（变异验证 ④）。
    #[test]
    fn programs_table_lists_the_five_wb_slots() {
        assert_eq!(
            program_ids(),
            vec![
                "codebuddy_cli",
                "codebuddy_cn_ide",
                "vscode_cn_inject",
                "vscode_codebuddy_plugin",
                "jetbrains_codebuddy_plugin",
            ],
            "WB 程序位表漂了 —— 顺序即 UI 展示顺序"
        );
        // 第 3 个程序位（`vscode_cn_inject`）必须真的在表里（「第 3 个程序位」用例的锚点）。
        assert_eq!(PROGRAMS[2].id, "vscode_cn_inject");
        assert_eq!(PROGRAMS[2].label_key, "wbAccounts.program.vscodeCnInject");
        // 新增的 VS Code / JetBrains 插件程序位必须真的在表里（label_key 与前端逐字一致）。
        assert_eq!(PROGRAMS[3].id, "vscode_codebuddy_plugin");
        assert_eq!(
            PROGRAMS[3].label_key,
            "wbAccounts.program.vscodeCodebuddyPlugin"
        );
        assert_eq!(PROGRAMS[4].id, "jetbrains_codebuddy_plugin");
        assert_eq!(
            PROGRAMS[4].label_key,
            "wbAccounts.program.jetbrainsCodebuddyPlugin"
        );
    }

    /// id 驱动查表：每个表项都能按 id 取回；未知 id ⇒ `None` / `Err`。
    #[test]
    fn program_spec_lookup_is_id_driven() {
        for spec in PROGRAMS {
            let found = program_spec(spec.id).expect("表项必须能按 id 取回");
            assert_eq!(found.id, spec.id);
            assert_eq!(found.label_key(), spec.label_key);
        }
        assert!(program_spec("nope").is_none());
        assert!(status("nope").is_err());
        assert!(switch("nope", "u", None).is_err());
        assert!(detect("nope").is_err());
        assert!(install_helper("nope").is_err());
    }

    /// 做不到的操作必须是**结构化 `Unsupported`**（`Ok` + 四字段），而不是 `Err` 或假成功。
    #[test]
    fn unimplemented_ops_are_structured_unsupported_not_faked() {
        // CLI 没有独立的「探测当前账号」⇒ trait 默认的结构化 `Unsupported`。
        let cli_detect = detect("codebuddy_cli").expect("CLI detect 应返回 Ok(Unsupported)");
        assert_eq!(cli_detect["capability"], json!("detect"));
        assert!(cli_detect.get("supportedOn").is_some());
        assert!(cli_detect.get("reason").is_some());
        assert!(cli_detect.get("ok").is_none());

        // IDE 没有「安装辅助」。
        let ide_install = install_helper("codebuddy_cn_ide").expect("IDE install_helper 应返回 Ok(Unsupported)");
        assert_eq!(ide_install["capability"], json!("install_helper"));
        assert!(ide_install.get("ok").is_none());
    }

    /// 第 3 个程序位（`vscode_cn_inject`）是**只读**的：`status` 可用，其余三个操作一律
    /// 结构化 `Unsupported`（诚实边界，见模块头）。
    #[test]
    fn third_slot_is_read_only_with_unsupported_writes() {
        // status：只报告有无，**不含** secret 明文。
        let status_value = status("vscode_cn_inject").expect("只读程序位的 status 必须可用");
        assert_eq!(status_value["id"], json!("vscode_cn_inject"));
        assert_eq!(status_value["readOnly"], json!(true));
        assert!(status_value.get("secret").is_none(), "绝不下发 secret 明文");
        assert!(status_value.get("secretPresent").is_some());

        // switch / detect / install_helper：结构化 Unsupported。
        for (op, value) in [
            ("switch", switch("vscode_cn_inject", "u-1", None).unwrap()),
            ("detect", detect("vscode_cn_inject").unwrap()),
            ("install_helper", install_helper("vscode_cn_inject").unwrap()),
        ] {
            assert_eq!(value["capability"], json!(op), "{op} 应声明为 Unsupported");
            assert!(value.get("ok").is_none(), "{op} 做不到就不能带 ok:true");
        }
    }

    /// `ProgramOp::as_str` 稳定且互不重复（进 `Unsupported.capability`，前端据此分支）。
    #[test]
    fn program_op_ids_are_stable_and_unique() {
        let ids = [
            ProgramOp::Status.as_str(),
            ProgramOp::Switch.as_str(),
            ProgramOp::Detect.as_str(),
            ProgramOp::InstallHelper.as_str(),
        ];
        assert_eq!(ids, ["status", "switch", "detect", "install_helper"]);
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "ProgramOp 取值不得重复");
    }

    /// 每个程序位都有非空 id / label_key / label（表不得出现空描述符）。
    #[test]
    fn every_slot_has_identity_and_label() {
        for spec in PROGRAMS {
            assert!(!spec.id.is_empty(), "程序位 id 不得为空");
            assert!(!spec.label_key().is_empty(), "{} 缺 label_key", spec.id);
            assert!(!spec.label().is_empty(), "{} 缺 label", spec.id);
        }
        // id 互不重复（重复会让「按 id 取回」产生歧义）。
        let mut ids = program_ids();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PROGRAMS.len(), "程序位 id 不得重复");
    }
}
