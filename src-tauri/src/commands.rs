//! Tauri commands：前端调用的薄包装，对应 Python 版 HTTP API。
//!
//! 阶段 1 覆盖：get_status / get_accounts / delete_account / oauth_start /
//! oauth_status / import_local。

use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Mutex;

use buddy_switch_core::modules::{
    account, app_prefs, auth_file, backup, capability, checkin, client_limits, client_settings,
    config, credit_usage, credits, diagnostics, export_import, migrate, oauth, process, programs,
    qoder, refresh, region::Region, region::RegionFilter, rotate, session, skill, switch,
    token_daily, token_stats, trae, travel, update, vscode_session,
};
use tauri::{Emitter, Manager};
// P5 §2.5 第 (2) 条：capability **默认拒绝**。表数据只在 core（`product_spec`），本文件只查表。
// ⚠️ 本文件在 `crates/buddy-switch-server/src/api.rs` 有**同款**接线（刻意不去重，见共享知识）。
use buddy_switch_core::modules::product::{
    require_capability, require_qoder_region, require_trae_region, require_wb_region,
    resolve_scope_param, Product,
};
use buddy_switch_core::modules::trae::region::TraeRegion;
// P9 · 悬浮窗**只读**数据模型 + provider（core）。⚠️ 与桌面窗口模块 `crate::companion`
// **不是一回事**：前者是跨宿主的只读数据模型，后者是 Tauri 的独立窗口实现。
use buddy_switch_core::modules::companion as companion_state;
// P15 · 外部 Agent 会话监听（客户端 hook 驱动）—— 悬浮窗的**第二个数据源**。
use buddy_switch_core::modules::agent_watch;
// P15 · hook 安装面（写上游客户端配置）。`app_prefs` / `client_settings` 已在文件头
// 的大括号导入里，此处**不重复引入**。
use buddy_switch_core::modules::agent_hook_install;
use buddy_switch_core::modules::proxy;
use buddy_switch_gateway::{AccountStrategy, GatewayConfig, GatewayStatusView};
// 统一网关（第三个产品线）：配置 + 命名空间表 + 独立密钥库。
// ⚠️ 响应体的**形状**由 `unified::admin` 共享构造提供，本文件不再各自拼 JSON。
use buddy_switch_gateway::unified::UnifiedGatewayConfig;

use crate::gateway;
use crate::qoder_gateway;
use crate::trae_gateway;
use crate::unified_gateway;

/// 解析 region 参数，缺省为 `cn`（保证旧行为）。
fn parse_region(value: Option<&str>) -> Region {
    value.and_then(Region::parse).unwrap_or(Region::Cn)
}

/// ★ 参数化命令的 Trae 分支：把**区域**映射到该区域的**主程序**变体。
///
/// ## 为什么允许这样折叠（只对**区域纯**的命令）
///
/// 区域内的两条程序位（`TraeWork` / `TraeCode`）共用**同一本账号库**
/// （见 `trae::account` 的「区域轴读取」说明：`*_for(variant)` 内部都按 `variant.region()`
/// 落库）。因此对账号列表 / 备注 / 删除这类**只读区域库**的命令，把 `trae-cn` 折叠到
/// 主程序 `TraeWork` **不改变结果**。
///
/// ## ⚠️ 程序位敏感的命令**不得**走这条路径
///
/// OAuth 授权线（`oauth_line()`：TraeWork→SOLO / Trae→TRAE）、打开数据目录、启动客户端、
/// 签到（按程序位探测）都**按程序位分家**，折叠会把登录态写进错误的客户端 —— 这些命令
/// **保留 `variant` 参数**，不参与区域折叠（详见 `docs/specs/P5-ARCH.md` §2.3 的报告）。
fn trae_variant_of_region(region: TraeRegion) -> trae::variant::TraeVariant {
    // 显式穷举，不用 `unwrap_or_default()`：回落分支在这里是不可达的（区域只有两个取值），
    // 显式匹配让「区域 → 主程序」这条约定在代码里可读，也避免引入 fail-open 形态。
    //
    // ★ issue #28：必须给**合并版**。原先这里给 `TraeWork` / `Global`（两条老程序线），
    // 而它们的客户端（`TRAE SOLO CN` / `TRAE SOLO`）已被官方合并卸载 ⇒
    // 「从本机客户端导入账号」永远报「未检测到【TraeWork CN】客户端」
    // （issue #28 报障原文），而用户明明装的是合并版 `Trae CN`。
    match region {
        TraeRegion::Cn => trae::variant::TraeVariant::TraeMerged,
        TraeRegion::Global => trae::variant::TraeVariant::GlobalTraeMerged,
    }
}

/// WorkBuddy 账号列表的**唯一实现**（旧命令 `get_accounts` 与参数化 `accounts_list` 共用）。
fn wb_accounts_value(region: Region) -> Value {
    let metas: Vec<Value> = account::load_accounts_for(region)
        .iter()
        .map(|a| account::account_meta_for(region, a))
        .collect();
    json!({ "region": region, "accounts": metas })
}

/// 解析 Trae 产品线变体参数，缺省为 [`trae::variant::TraeVariant::default`]（保证旧行为）。
///
/// 与 [`parse_region`] 同风格：**缺失即回落到默认**，不做「未知值报错」——
/// 前端老版本不带该参数、或用户从探测结果里传回目录名（如 `TRAE SOLO CN`），
/// 都应被宽容接受；无法解析时等价于未传。
///
/// ⚠️ **本函数在 `crates/buddy-switch-server/src/api.rs` 有一份同款实现，
/// 两处必须保持一致**（同样的「缺失/未知 → `default()`」语义）。不要为了去重
/// 跨 crate 抽公共函数——server 与 tauri 是两个独立 crate，为这 4 行引入共享依赖
/// 不值得。两处各自带 `parse_trae_variant_*` 单测（含未知值回落护栏）钉住行为。
fn parse_trae_variant(value: Option<&str>) -> trae::variant::TraeVariant {
    value
        .and_then(trae::variant::TraeVariant::parse)
        .unwrap_or_default()
}

/// 解析**逗号分隔**的账号 id 列表（`"a,b,c"`；缺失 / 空串 ⇒ 空数组）。
///
/// ⚠️ 与 `crates/buddy-switch-server/src/api.rs` 的 `parse_id_list` **同语义、必须同步**：
/// 两条通道收同一个形状，否则「浏览器里刷新成功、桌面端一条 id 都传不进去」这种漂移
/// 只会在真机上暴露（本仓已因同类问题踩过 `trae_switch_account`）。
fn split_id_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 解析统计查询范围参数，缺省为 `cn`（保证旧行为）；额外支持 `"all"` 合并视图。
///
/// 复用 core 的 `parse_region_filter`，绑定规则与既有 [`parse_region`] 一致，
/// 并额外接受 `"all" | "*" | "合并" | "全部"`。**仅**用于两个统计命令。
fn parse_region_filter(value: Option<&str>) -> RegionFilter {
    buddy_switch_core::modules::region::parse_region_filter(value)
}

/// 把 core 的 [`auth_file::RegionMismatch`] 序列化为前端 `RegionMismatch` 契约（camelCase）。
fn mismatch_json(mismatch: &auth_file::RegionMismatch) -> Value {
    json!({
        "actualDomain": mismatch.actual_domain,
        "expectedFile": mismatch.expected_file,
        "envVar": mismatch.env_var,
        "actualRegion": mismatch.actual_region.as_str(),
        "expectedRegion": mismatch.expected_region.as_str(),
    })
}

/// 该 region 客户端是否已安装（跨平台：解析出的应用路径真实存在）。
fn region_installed(region: Region) -> bool {
    auth_file::workbuddy_app_path_for(region).exists()
}

/// 切换进度缓存：桌面端也可通过 `switch_progress` 轮询（与 webui 的
/// `/api/switch/progress` 同契约）。事件 `switch-progress` 仍照常派发。
static SWITCH_PROGRESS: Mutex<Option<String>> = Mutex::new(None);
static SWITCH_RUNNING: Mutex<bool> = Mutex::new(false);
/// 进行中的切换操作所属 region —— 供悬浮窗快照推导 / `companion_jump` 聚焦来源用。
static SWITCH_REGION: Mutex<Option<Region>> = Mutex::new(None);

/// 应用状态（序列化为 **camelCase**，与 server `/api/status` 契约一致）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    running: bool,
    region: String,
    auth_file: String,
    current: Option<Value>,
    app_path: String,
    version: String,
    /// 该 region 客户端是否已安装（前端据此判定 Tab 空态）。
    installed: bool,
    /// 认证文件 domain 与目标 region 不符时返回（安全红线 F），否则为 null。
    region_mismatch: Option<Value>,
}

/// GET /api/status —— WorkBuddy 运行状态 + 当前账号（可按 region）。
#[tauri::command]
pub async fn get_status(region: Option<String>) -> Result<AppStatus, String> {
    // Windows 的运行状态检测会启动 tasklist 子进程。同步 command 默认在
    // Tauri 主线程执行，标题栏拖拽期间一旦焦点事件触发状态刷新，就会阻塞
    // 原生窗口消息循环。放入 blocking 线程，保持窗口移动与 IPC 查询解耦。
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || build_app_status(region))
        .await
        .map_err(|error| format!("查询应用状态失败: {error}"))
}

#[allow(clippy::bind_instead_of_map)]
// 同上：与 server 侧 `api.rs` 保持逐字同构。
fn build_app_status(region: Region) -> AppStatus {
    // 安全红线 F：读取后校验凭据域归属；不匹配即拒绝使用（current 置空）。
    let (auth, region_mismatch) = match auth_file::read_auth_file_checked_for(region) {
        Ok(auth) => (auth, None),
        Err(mismatch) => (None, Some(mismatch_json(&mismatch))),
    };
    let current = auth.as_ref().and_then(|a| {
        let acct = a.get("account").cloned().unwrap_or_else(|| json!({}));
        // 展示字段先归一成「字符串或 null」再下发：认证文件里 `nickname` 可能是对象
        // （见 core `account::display_str` 的文档），裸透传会让前端整棵树崩掉
        // ⇒ 窗口一片白（issue #2）。**与 server 侧 `api.rs::api_status` 逐字同构**。
        //
        // `nickname` 额外多一道**账号库回落**：新版客户端把它存成加密信封，
        // 读不到时若直接下发 null，界面就只能显示 uid（用户报障）。回落规则与
        // 「为什么账号库是同源的」全在 `account::current_nickname_for` 一处，
        // 这里**不要**自己再写一遍条件。
        Some(json!({
            "uid": account::display_str(&acct, "uid"),
            "nickname": account::current_nickname_for(region, &acct),
            "email": account::display_str(&acct, "email"),
        }))
    });
    AppStatus {
        running: process::is_workbuddy_running_for(region),
        region: region.as_str().to_string(),
        auth_file: auth_file::auth_file_path_for(region)
            .to_string_lossy()
            .to_string(),
        current,
        app_path: auth_file::workbuddy_app_path_for(region)
            .to_string_lossy()
            .to_string(),
        version: update::APP_VERSION.to_string(),
        installed: region_installed(region),
        region_mismatch,
    }
}

/// GET /api/accounts —— 账号列表（account_meta，不含 token；可按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_list`]（带 `scope`）。本命令**保留**
/// （webui 路由 / `generate_handler!` / 前端 `ALIASES` 表都留着），实现与 [`accounts_list`]
/// 的 WorkBuddy 分支**共用** [`wb_accounts_value`]，不会漂移。
#[tauri::command]
pub fn get_accounts(region: Option<String>) -> Value {
    wb_accounts_value(parse_region(region.as_deref()))
}

// ---------------------------------------------------------------------------
// P5 T03 · 参数化命令（`scope` 同时决定平台与区域）
// ---------------------------------------------------------------------------
//
// 命名与语义见 `docs/specs/P5-ARCH.md` §2.3。本组只覆盖**区域纯且 arity 同构**的命令
// （账号列表 / 备注 / 删除）：旧命令保留为别名（见 `src/lib/api.ts` 的 `ALIASES`）。
//
// ⚠️ **`scope` 是必填维度且 fail-closed**：未知 / 空 / 缺一律 `Err`（[`resolve_scope_param`]），
// **绝不**回落到任何默认作用域 —— 静默回落会把数据读写到**错误的库**。

/// ★ 参数化命令：账号列表 —— `scope` 同时决定**平台**与**区域**。
///
/// 与旧命令的关系：`get_accounts`（WB）与 `get_trae_accounts`（Trae）保留为**别名**，
/// 三者共用同一份实现：
/// - WB 分支 → [`wb_accounts_value`]（与 `get_accounts` 逐字同源）；
/// - Trae 分支 → `trae::handlers::accounts_overview_for`（与 `get_trae_accounts` 同源）。
///
/// ## 为什么可以合并（安全性论证）
///
/// 本命令是**区域纯**的：Trae 的账号 / 分组 / 冷却全部按 `variant.region()` 落到同一本区域库
/// （见 [`trae_variant_of_region`]），故 `trae-cn` 折叠到主程序 `TraeWork` **不改变结果**。
///
/// ## 为什么返回形状按平台分叉（诚实口径）
///
/// 两平台账号视图的**字段不同**（WB `AccountMeta` vs Trae `account_view`），强行统一会丢字段。
/// 故本命令**按 `scope.product()` 返回各自的**原生形状，由前端的平台专属 wrapper 解读 ——
/// 这是 §2.3「概念同构但形状不同」的落点，**不是**类型安全的统一契约。
#[tauri::command]
pub async fn accounts_list(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => Ok(wb_accounts_value(require_wb_region(scope)?)),
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            tauri::async_runtime::spawn_blocking(move || {
                trae::handlers::accounts_overview_for(variant)
            })
            .await
            .map_err(|error| format!("读取 Trae 账号总览失败: {error}"))
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// ★ 参数化命令：设置账号备注（**字段级更新**，不触碰 token）。
///
/// 合并旧 `set_account_remark`（WB）与 `trae_set_account_remark`（Trae）。**区域纯**
/// （Trae 侧 `set_remark_for` 按 `variant.region()` 落库）。`remark` 空串 / 缺省 = 清空备注。
#[tauri::command(rename_all = "camelCase")]
pub fn account_remark(
    scope: String,
    account_id: String,
    remark: Option<String>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    match scope.product() {
        Product::Workbuddy => account::set_account_remark_for(
            require_wb_region(scope)?,
            &account_id,
            remark.as_deref(),
        ),
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            // Trae 侧「缺 remark ⇒ 清空」：与 `trae_set_account_remark` 的契约一致（空串即清空）。
            let remark = remark.unwrap_or_default();
            trae::handlers::set_remark_for(variant, &account_id, Some(remark.as_str()))
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// ★ 参数化命令：删除账号（Trae 侧可选一并删除登录态快照）。
///
/// 合并旧 `delete_account`（WB）与 `trae_delete_account`（Trae）。**区域纯**。
/// ⚠️ WB 侧仍是**硬删除**（软删除 + 回收站属 T01，不在本命令范围）。
#[tauri::command(rename_all = "camelCase")]
pub async fn account_delete(
    scope: String,
    account_id: String,
    delete_profile: Option<bool>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    match scope.product() {
        Product::Workbuddy => {
            account::delete_account_for(require_wb_region(scope)?, &account_id)?;
            Ok(json!({ "ok": true }))
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            let delete_profile = delete_profile.unwrap_or(false);
            tauri::async_runtime::spawn_blocking(move || {
                trae::handlers::delete_account_for(variant, &account_id, delete_profile)
            })
            .await
            .map_err(|error| format!("删除账号失败: {error}"))?
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

// ---------------------------------------------------------------------------
// 回收站 4 命令（P5 §2.7）—— **全新命令**（非参数化、无别名），全部带 `scope` 且 fail-closed
// ---------------------------------------------------------------------------
//
// 与 `account_delete` 的区别：这四条**不是**旧命令的「参数化替代」，而是回收站功能的新命令，
// 因此在 `api.ts` 的 `ALIASES` 里**没有**对应旧名。它们**仍**带 `scope`：scope 决定
// **平台 + 区域**，从而决定读/写哪个 `*.trash.<region>.json`。
//
// ⚠️ **fail-closed**（§2.7 硬约束 1）：非法 scope（如 `trae-glboal`）必须 `Err`，
// **绝不**静默回落到 `cn` 的回收站 —— 否则会出现「删了 A 库账号，却在 B 库回收站里找」。
// 这由 `resolve_scope_param` + `require_wb_region` / `require_trae_region` 共同保证。
//
// ⚠️ 本文件在 `crates/buddy-switch-server/src/api.rs` 有**同款**接线（刻意不去重，见共享知识）。

/// ★ 回收站列表（P5 §2.7）：返回 `{ scope, entries }`，条目按删除时间倒序。
#[tauri::command(rename_all = "camelCase")]
pub fn account_trash_list(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => Ok(account::trash_list_value(require_wb_region(scope)?)),
        Product::Trae => Ok(trae::account::trash_list_value_for_region(
            require_trae_region(scope)?,
        )),
        Product::Qoder => Ok(qoder::trash::trash_list_value(require_qoder_region(scope)?)),
    }
}

/// ★ 从回收站恢复账号（P5 §2.7）：把记录移回 live（`id` **或** `uid` 谓词）。
///
/// `key` 是账号标识：WB 侧 `id` / `uid` 皆可命中；Trae 侧为 `user_id`。
#[tauri::command(rename_all = "camelCase")]
pub fn account_restore(scope: String, key: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    if key.trim().is_empty() {
        return Err("缺少 key".to_string());
    }
    match scope.product() {
        Product::Workbuddy => {
            account::restore_account_for(require_wb_region(scope)?, &key)?;
            Ok(json!({ "ok": true }))
        }
        Product::Trae => {
            trae::account::restore_for_region(require_trae_region(scope)?, &key)?;
            Ok(json!({ "ok": true }))
        }
        Product::Qoder => {
            qoder::trash::restore_account_for(require_qoder_region(scope)?, &key)?;
            Ok(json!({ "ok": true }))
        }
    }
}

/// ★ 永久删除（purge，**唯一不可逆步**，P5 §2.7）：从回收站真删 + 销毁关联凭据 / 快照。
///
/// ⚠️ 前端**必须**先走 danger 二次确认 Dialog；本命令只负责执行（后端不做「确认状态」的假设）。
#[tauri::command(rename_all = "camelCase")]
pub fn account_purge(scope: String, key: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    if key.trim().is_empty() {
        return Err("缺少 key".to_string());
    }
    match scope.product() {
        Product::Workbuddy => {
            account::purge_account_for(require_wb_region(scope)?, &key)?;
            Ok(json!({ "ok": true }))
        }
        Product::Trae => {
            trae::account::purge_for_region(require_trae_region(scope)?, &key)?;
            Ok(json!({ "ok": true }))
        }
        Product::Qoder => {
            qoder::trash::purge_account_for(require_qoder_region(scope)?, &key)?;
            Ok(json!({ "ok": true }))
        }
    }
}

/// ★ 清空回收站（**不可逆**，P5 §2.7）：返回被删除的条数 `{ ok, removed }`。
///
/// ⚠️ 前端**必须**先走 danger 二次确认 Dialog，且**明示将删除的条数**（§2.7 验收 13）。
#[tauri::command(rename_all = "camelCase")]
pub fn account_trash_clear(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let removed = match scope.product() {
        Product::Workbuddy => account::clear_trash_for(require_wb_region(scope)?),
        Product::Trae => trae::account::clear_trash_for_region(require_trae_region(scope)?),
        Product::Qoder => qoder::trash::clear_trash_for(require_qoder_region(scope)?),
    };
    Ok(json!({ "ok": true, "removed": removed }))
}

/// ★ 参数化命令：导出账号完整记录（含 token / JWT）—— `scope` 同时决定平台与区域。
///
/// 合并旧 `export_accounts`（WB）与 `trae_export_accounts`（Trae）。**区域纯**：两边都只按
/// 区域读账号库（`export_accounts_for(region, ids)` / `export_accounts_for(variant, ids)`），
/// **不触碰客户端 / 程序位**，因此可安全折叠到 `scope`。
///
/// ## 为什么名字**不**沿用 §2.3 的 `export_accounts`
///
/// §2.3 给本行的「参数化后」名字是 `export_accounts`，但它与 **WorkBuddy 旧命令同名** ——
/// 而 §2.4 硬约束要求「旧命令保留为别名」（`generate_handler!` 不允许重名）。二者冲突时
/// 保留旧命令、给参数化命令取**不冲突的新名**（与已落地的 `accounts_list` 同款做法）。
///
/// 旧命令保留为别名（见 `src/lib/api.ts` 的 `ALIASES`）；本命令是其**同源实现**。
#[tauri::command(rename_all = "camelCase")]
pub fn accounts_export(scope: String, account_ids: Vec<String>) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => {
            export_import::export_accounts_for(require_wb_region(scope)?, &account_ids)
                .map(|records| json!({ "ok": true, "accounts": records }))
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::export_accounts_for(variant, &account_ids)
        }
        Product::Qoder => qoder::export_import::export_accounts_for(
            require_qoder_region(scope)?,
            Some(&account_ids),
        ),
    }
}

/// ★ 参数化命令：把勾选账号的完整记录写入指定路径 —— `scope` 同时决定平台与区域。
///
/// 合并旧 `export_accounts_to_path`（WB）与 `trae_export_accounts_to_path`（Trae）。**区域纯**。
/// 名字理由同 [`accounts_export`]（§2.3 的 `export_accounts_to_path` 与 WB 旧命令同名）。
#[tauri::command(rename_all = "camelCase")]
pub fn accounts_export_to_path(
    scope: String,
    account_ids: Vec<String>,
    path: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => export_import::export_accounts_to_path_for(
            require_wb_region(scope)?,
            &account_ids,
            &path,
        )
        .map(|path| json!({ "ok": true, "path": path })),
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::export_accounts_to_path_for(variant, &account_ids, &path)
        }
        Product::Qoder => qoder::export_import::export_accounts_to_path_for(
            require_qoder_region(scope)?,
            Some(&account_ids),
            &path,
        ),
    }
}

/// ★ 参数化命令：按选中索引把账号导入该区域账号库 —— `scope` 同时决定平台与区域。
///
/// 合并旧 `import_accounts`（WB）与 `trae_import_accounts`（Trae）。**区域纯**：导入只写
/// 区域账号库，不涉及客户端 / 程序位。名字理由同 [`accounts_export`]。
#[tauri::command(rename_all = "camelCase")]
pub fn accounts_import(
    scope: String,
    file_text: String,
    indexes: Vec<usize>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => {
            let result = export_import::import_accounts_for(
                require_wb_region(scope)?,
                &file_text,
                &indexes,
            )?;
            Ok(json!({
                "ok": true,
                "imported": result.imported,
                "skipped": result.skipped,
                "overwritten": result.overwritten,
            }))
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::import_accounts_for(variant, &file_text, &indexes)
        }
        Product::Qoder => qoder::export_import::import_accounts_for(
            require_qoder_region(scope)?,
            &file_text,
            Some(&indexes),
            true,
        ),
    }
}

/// ★ 参数化命令：从本机客户端登录态导入当前账号 —— `scope` 同时决定平台与区域。
///
/// 合并旧 `import_local`（WB，读 WorkBuddy 认证文件）与 `trae_import_local_account`
/// （Trae，读 Trae userData 登录态）。两者都是**区域纯**的：读取**该区域客户端**的登录态
/// 并写入**该区域**账号库 —— 读哪个客户端由 `scope` 决定，**不**用
/// `detected_variant` / `detect_data_dir` 猜测活跃客户端。
///
/// ## 名字为何**不**沿用 §2.3 的 `import_local`
///
/// `import_local` 与 **WorkBuddy 旧命令同名**，与 §2.4「旧命令保留为别名」冲突
/// （`generate_handler!` 不允许重名）。故取不冲突新名 `accounts_import_local`
/// （与 [`accounts_import`] / [`accounts_export`] 同族）。旧命令保留为别名
/// （见 `src/lib/api.ts` 的 `ALIASES`）。
///
/// ## 形状按平台分叉（诚实口径）
///
/// WB 返回 `{ok, account}`（单账号 `AccountMeta`）；Trae 返回 `{userId, name, accounts}`
/// （导入账号 + 最新账号视图）。与 [`accounts_list`] 同款落点，由前端平台专属 wrapper 解读。
///
/// `async` + `spawn_blocking`：要读客户端认证文件 / userData（**慢 IO**），
/// 同步命令跑在 Tauri 主线程上会阻塞原生窗口消息循环。
#[tauri::command(rename_all = "camelCase")]
pub async fn accounts_import_local(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => {
            let region = require_wb_region(scope)?;
            tauri::async_runtime::spawn_blocking(move || {
                account::import_local_for(region).map(|acc| json!({ "ok": true, "account": acc }))
            })
            .await
            .map_err(|error| format!("导入本机账号失败: {error}"))?
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            tauri::async_runtime::spawn_blocking(move || {
                trae::handlers::import_local_account_for(variant)
            })
            .await
            .map_err(|error| format!("导入本机账号失败: {error}"))?
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// ★ 参数化命令：积分 / 积分总览 —— `scope` 同时决定**平台**与**区域**。
///
/// 合并旧 `get_credit_expiry`（WB，单账号积分资源与到期时间）与 `get_trae_credits`
/// （Trae，区域级积分总览）。两者都是**区域纯**的（只按区域读账号库 / 积分缓存，
/// 不触碰客户端 / 程序位），故可折叠到 `scope`。
///
/// ## 形状按平台分叉（诚实口径）
///
/// 两平台返回形状**不同**（WB `CreditExpiry` 单账号 vs Trae `TraeCreditsOverview` 区域总览），
/// 故**按 `scope.product()` 返回各自原生形状**，由前端平台专属 wrapper 解读
/// —— 与 [`accounts_list`] 同款落点，不是类型安全的统一契约。
///
/// ## `account_id` 仅 WorkBuddy 分支使用
///
/// WB 侧是**单账号**查询（旧 `get_credit_expiry(accountId, region)` 的契约），故 WB 分支
/// **必须**带 `accountId`；Trae 侧是**区域总览**（旧 `get_trae_credits(variant)` 无账号参数），
/// 故 Trae 分支**忽略** `accountId`。这是两平台原生契约的差异，不是漏参数。
///
/// 旧命令保留为别名（见 `src/lib/api.ts` 的 `ALIASES`）。
#[tauri::command(rename_all = "camelCase")]
pub async fn credits(scope: String, account_id: Option<String>) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => {
            require_capability(Product::Workbuddy, capability::Capability::Credits)?;
            let region = require_wb_region(scope)?;
            let account_id = account_id.unwrap_or_default();
            let acc = account::find_account_for(region, &account_id).ok_or("账号不存在")?;
            Ok(credits::get_credit_expiry_for(region, &acc).await)
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            Ok(trae::handlers::credits_overview_for(variant))
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// ★ 参数化命令：签到状态 —— `scope` 同时决定**平台**与**区域**。
///
/// 合并旧 `get_checkin_status`（WB，单账号签到状态）与 `get_trae_checkin_status`
/// （Trae，区域级签到摘要 + 冷却明细）。两者都是**区域纯**的（只按区域读账号库 /
/// 签到缓存，不触碰客户端 / 程序位），故可折叠到 `scope`。
///
/// ## 形状按平台分叉（诚实口径）
///
/// 与 [`credits`] 同款落点：WB 侧是**单账号**查询（旧 `get_checkin_status(accountId, region)`
/// 的契约），故 WB 分支**必须**带 `accountId`；Trae 侧是**区域级摘要**
/// （旧 `get_trae_checkin_status(variant)` 无账号参数），故 Trae 分支**忽略** `accountId`。
///
/// ## 与 webui 批量端点的关系（**不要**在这里做批量）
///
/// webui 的 `GET /api/checkin/status` 是**整端点批量**（一次查全部账号，见 `api.ts` 的
/// `getCheckinStatusMap` 文档）；本命令**不做批量** —— 桌面端逐账号调用，webui 仍走那条
/// 批量旧路由。两端语义差异由前端 `getCheckinStatus` 收口，本命令只对齐**桌面端单账号**
/// 契约（与旧 `get_checkin_status` 逐字一致）。
///
/// 旧命令保留为别名（见 `src/lib/api.ts` 的 `ALIASES`）。
#[tauri::command(rename_all = "camelCase")]
pub async fn checkin_status(scope: String, account_id: Option<String>) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => {
            let region = require_wb_region(scope)?;
            let account_id = account_id.unwrap_or_default();
            let acc = account::find_account_for(region, &account_id).ok_or("账号不存在")?;
            Ok(checkin::get_checkin_status_for(region, &acc).await)
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            Ok(trae::handlers::checkin_status_for(variant))
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// ★ 参数化命令：在系统文件管理器中打开**该作用域客户端**的数据目录 —— `scope` 同时决定
/// 平台与区域。
///
/// 合并旧 `open_workbuddy_data_dir`（WB）与 `open_trae_data_dir`（Trae）。两平台**同形**
/// （都收一个作用域参数，返回 `{ok,path}` 或结构化 `Unsupported`）。
///
/// ## 判据说明（为何算「区域纯」）
///
/// 本命令**只读**客户端数据目录的**路径**并交给文件管理器打开，**不写**任何程序位状态
/// —— 与「会关 / 重启客户端进程」的 `switch_account` 类命令不同。目录由 `scope` 决定，
/// **不**用 `detected_variant` / `detect_data_dir` 猜测活跃客户端。
///
/// 旧命令保留为别名（见 `src/lib/api.ts` 的 `ALIASES`）。
///
/// `async` + `spawn_blocking`：`spawn explorer` 实测 ≈160 ms/次，同步执行会在点击瞬间
/// 卡住主线程（issue #48）。
#[tauri::command(rename_all = "camelCase")]
pub async fn open_data_dir(scope: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || build_open_data_dir(scope))
        .await
        .map_err(|error| format!("打开数据目录失败: {error}"))?
}

fn build_open_data_dir(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => capability::open_workbuddy_data_dir(require_wb_region(scope)?),
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::open_data_dir(variant)
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// GET /api/codebuddy-cli/status —— CodeBuddy CLI helper 轮换状态（不含 token）。
///
/// **表驱动**：本体是 `programs::status("codebuddy_cli")`（见 core `modules/programs.rs`）。
/// 命令只负责「宿主异步 + 错误包装」，不再硬编码具体模块函数 —— 新增程序位无需改本文件。
///
/// async + spawn_blocking：状态检测可能执行 ps / helper 定位等子进程，
/// 避免在账号页挂载刷新时阻塞主线程造成页面卡顿。
#[tauri::command]
pub async fn get_codebuddy_cli_status() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::status("codebuddy_cli"))
        .await
        .map_err(|error| format!("查询 CodeBuddy CLI 状态失败: {error}"))?
}

/// POST /api/codebuddy-cli/install-helper —— 显式安装/升级 CLI helper。
///
/// **表驱动**：`programs::install_helper("codebuddy_cli")`。
#[tauri::command]
pub async fn install_codebuddy_cli_helper() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::install_helper("codebuddy_cli"))
        .await
        .map_err(|e| e.to_string())?
}

/// POST /api/codebuddy-cli/switch —— 只切换 CodeBuddy CLI，不重启 WorkBuddy。
///
/// **表驱动**：`programs::switch("codebuddy_cli", …)`（CLI 不重启客户端 ⇒ `restart` 传 `None`）。
///
/// async + spawn_blocking：切换会用登录 shell 定位 node 并执行 apiKeyHelper
/// 校验账号（子进程无超时），同步 command 会阻塞主线程造成 UI 卡顿。
#[tauri::command(rename_all = "camelCase")]
pub async fn switch_codebuddy_cli_account(account_id: String) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        programs::switch("codebuddy_cli", &account_id, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// GET /api/codebuddy-cn-ide/status —— CodeBuddy IDE 安装/运行/当前账号。
///
/// **表驱动**：`programs::status("codebuddy_cn_ide")`。
///
/// async + spawn_blocking：状态检测会跑 ps / mdfind 等子进程（mdfind 可能
/// 耗时数秒），账号页每次挂载都会刷新，若在主线程执行会造成页面卡顿。
#[tauri::command]
pub async fn get_codebuddy_cn_ide_status() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::status("codebuddy_cn_ide"))
        .await
        .map_err(|error| format!("查询 CodeBuddy IDE 状态失败: {error}"))?
}

/// POST /api/codebuddy-cn-ide/switch —— 注入凭证并可选重启 CodeBuddy CN IDE。
///
/// **表驱动**：`programs::switch("codebuddy_cn_ide", …)`（IDE 切换默认重启 ⇒ `restart` 缺省即 `true`，
/// 由 adapter 归一，见 core `programs.rs`）。
///
/// async + spawn_blocking：切换会关闭并重启 CodeBuddy CN，可能阻塞数十秒，
/// 与 WorkBuddy 切换同理，若在同步 command（主线程）执行会卡死整个 UI。
#[tauri::command(rename_all = "camelCase")]
pub async fn switch_codebuddy_cn_ide_account(
    account_id: String,
    restart: Option<bool>,
) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        programs::switch("codebuddy_cn_ide", &account_id, restart)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// POST /api/codebuddy-cn-ide/detect —— 读取本机 CN IDE 当前登录并尝试匹配账号库。
///
/// **表驱动**：`programs::detect("codebuddy_cn_ide")`。
///
/// async + spawn_blocking：会通过 Keychain/secret 读取子进程，避免阻塞主线程。
#[tauri::command]
pub async fn detect_codebuddy_cn_ide_account() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::detect("codebuddy_cn_ide"))
        .await
        .map_err(|e| e.to_string())?
}

/// GET /api/vscode-codebuddy-plugin/status —— VS Code 内 CodeBuddy 扩展状态。
///
/// **表驱动**：`programs::status("vscode_codebuddy_plugin")`。
///
/// async + spawn_blocking：状态检测会跑进程探测 / 读 state.vscdb，
/// 账号页每次挂载都会刷新，若在主线程执行会造成页面卡顿。
#[tauri::command]
pub async fn get_vscode_codebuddy_plugin_status() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::status("vscode_codebuddy_plugin"))
        .await
        .map_err(|error| format!("查询 VS Code CodeBuddy 扩展状态失败: {error}"))?
}

/// POST /api/vscode-codebuddy-plugin/switch —— 注入凭证并可选重启 VS Code。
///
/// **表驱动**：`programs::switch("vscode_codebuddy_plugin", …)`（默认重启，由 adapter 归一）。
///
/// async + spawn_blocking：切换会优雅关闭并重新打开 VS Code，可能阻塞数十秒。
#[tauri::command(rename_all = "camelCase")]
pub async fn switch_vscode_codebuddy_plugin_account(
    account_id: String,
    restart: Option<bool>,
) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        programs::switch("vscode_codebuddy_plugin", &account_id, restart)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// POST /api/vscode-codebuddy-plugin/detect —— 读取本机 VS Code 扩展当前登录并尝试匹配账号库。
///
/// **表驱动**：`programs::detect("vscode_codebuddy_plugin")`。
#[tauri::command]
pub async fn detect_vscode_codebuddy_plugin_account() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::detect("vscode_codebuddy_plugin"))
        .await
        .map_err(|e| e.to_string())?
}

/// GET /api/jetbrains-codebuddy-plugin/status —— JetBrains IDE 内 CodeBuddy 插件状态。
///
/// **表驱动**：`programs::status("jetbrains_codebuddy_plugin")`。
#[tauri::command]
pub async fn get_jetbrains_codebuddy_plugin_status() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::status("jetbrains_codebuddy_plugin"))
        .await
        .map_err(|error| format!("查询 JetBrains CodeBuddy 插件状态失败: {error}"))?
}

/// POST /api/jetbrains-codebuddy-plugin/switch —— 写入 secret-storage.xml 并可选重启 IDE。
///
/// **表驱动**：`programs::switch("jetbrains_codebuddy_plugin", …)`（默认重启，由 adapter 归一）。
#[tauri::command(rename_all = "camelCase")]
pub async fn switch_jetbrains_codebuddy_plugin_account(
    account_id: String,
    restart: Option<bool>,
) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        programs::switch("jetbrains_codebuddy_plugin", &account_id, restart)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// POST /api/jetbrains-codebuddy-plugin/detect —— 读取本机 JetBrains 插件当前登录并尝试匹配账号库。
///
/// **表驱动**：`programs::detect("jetbrains_codebuddy_plugin")`。
#[tauri::command]
pub async fn detect_jetbrains_codebuddy_plugin_account() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| programs::detect("jetbrains_codebuddy_plugin"))
        .await
        .map_err(|e| e.to_string())?
}

/// POST /api/delete —— 删除账号（按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`account_delete`]（带 `scope`）。本命令保留
/// （webui 路由 / `generate_handler!` / 前端 `ALIASES` 表），是 [`account_delete`] 的
/// WorkBuddy 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn delete_account(account_id: String, region: Option<String>) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    account::delete_account_for(region, &account_id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/account/remark —— 设置账号备注（**字段级更新**，不触碰 token）。
///
/// 同步 command 即可：只读一个小 JSON、改一个键、原子写回。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`account_remark`]（带 `scope`）。本命令保留
/// （webui 路由 / `generate_handler!` / 前端 `ALIASES` 表），是 [`account_remark`] 的
/// WorkBuddy 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn set_account_remark(
    account_id: String,
    remark: Option<String>,
    region: Option<String>,
) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    let region = parse_region(region.as_deref());
    account::set_account_remark_for(region, &account_id, remark.as_deref())
}

/// 设置账号自定义分组（**字段级更新**，不触碰 token）。
///
/// 与 [`set_account_remark`] 同源：只读一个 JSON、改一个键、原子写回（**一次**读改写）。
/// `group` 空串 / 缺省 = 移出分组（删除账号库里的 `groupName` 键，缺省即「未分组」）。
#[tauri::command(rename_all = "camelCase")]
pub fn set_account_group(
    account_id: String,
    group: Option<String>,
    region: Option<String>,
) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    let region = parse_region(region.as_deref());
    account::set_account_group_for(region, &account_id, group.as_deref())
}

/// POST /api/oauth/start —— 发起 OAuth 扫码登录（按 region）。
#[tauri::command]
pub async fn oauth_start(region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::Oauth)?;
    let region = parse_region(region.as_deref());
    oauth::oauth_start_for(region).await
}

/// GET /api/oauth/status —— 轮询采集结果（按 region）。
#[tauri::command(rename_all = "camelCase")]
pub async fn oauth_status(login_id: String, region: Option<String>) -> Value {
    let region = parse_region(region.as_deref());
    oauth::oauth_poll_for(region, &login_id).await
}

/// POST /api/import-local —— 导入本机当前账号（按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_import_local`]（带 `scope`）。
/// 本命令保留（webui 路由 / `generate_handler!` / 前端 `ALIASES` 表），是
/// [`accounts_import_local`] 的 WorkBuddy 分支同源实现。
///
/// `async` + `spawn_blocking`：本命令要读客户端认证文件（**慢 IO**），
/// 同步命令跑在 Tauri **主线程**上，会阻塞原生窗口消息循环（判据见 `get_status` 的注释）。
#[tauri::command]
pub async fn import_local(region: Option<String>) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        account::import_local_for(region).map(|acc| json!({ "ok": true, "account": acc }))
    })
    .await
    .map_err(|error| format!("导入本机账号失败: {error}"))?
}

// ---------------------------------------------------------------------------
// 导出 / 导入账号
// ---------------------------------------------------------------------------

/// POST /api/export-accounts —— 按账号 id 列表导出完整记录（含 token，按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_export`]（带 `scope`）。本命令保留
/// （webui 路由 / `generate_handler!` / 前端 `ALIASES` 表），是 [`accounts_export`] 的
/// WorkBuddy 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn export_accounts(account_ids: Vec<String>, region: Option<String>) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    export_import::export_accounts_for(region, &account_ids)
        .map(|records| json!({ "ok": true, "accounts": records }))
}

/// POST /api/export-accounts-to-path —— 把勾选账号的完整记录写入用户选择的路径（保存对话框产物，按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_export_to_path`]（带 `scope`）。本命令保留，
/// 是 [`accounts_export_to_path`] 的 WorkBuddy 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn export_accounts_to_path(
    account_ids: Vec<String>,
    path: String,
    region: Option<String>,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    export_import::export_accounts_to_path_for(region, &account_ids, &path)
        .map(|path| json!({ "ok": true, "path": path }))
}

/// POST /api/import/preview —— 解析导入文件并返回脱敏预览（含文件内索引）。
///
/// **无需 region**：预览是纯函数（只解析请求里的文件文本，不触及任何 region 账号库），
/// 因此显式忽略前端可能一并带来的 `region` 参数。
#[tauri::command(rename_all = "camelCase")]
pub fn preview_import_accounts(file_text: String, scope: Option<String>) -> Result<Value, String> {
    // ⚠️ 2026-10-15：第二个参数由 `region`（一直被忽略）升为 `scope`。Qoder 的预览要按
    // **区域**判「归属不符」，而 `region=cn` 分不出 `wb-cn` 与 `qoder-cn`。
    // 缺省 / 非 Qoder 作用域 ⇒ 走既有的纯解析分支（旧行为逐字不变）。
    if let Some(raw) = scope.as_deref().filter(|value| !value.trim().is_empty()) {
        let parsed = resolve_scope_param(Some(raw))?;
        if parsed.product() == Product::Qoder {
            return qoder::export_import::preview_accounts(
                require_qoder_region(parsed)?,
                &file_text,
            );
        }
    }
    export_import::preview_accounts(&file_text)
}

/// POST /api/import —— 按选中索引把账号导入该 region 账号库，返回导入/跳过/覆盖计数。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_import`]（带 `scope`）。本命令保留，
/// 是 [`accounts_import`] 的 WorkBuddy 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn import_accounts(
    file_text: String,
    indexes: Vec<usize>,
    region: Option<String>,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let result = export_import::import_accounts_for(region, &file_text, &indexes)?;
    Ok(json!({
        "ok": true,
        "imported": result.imported,
        "skipped": result.skipped,
        "overwritten": result.overwritten,
    }))
}

/// ★ P7 T01：导出**全局备份** envelope（跨全部 4 作用域的账号 / 配置 / 网关）。
///
/// `selection` 是前端组装的勾选对象（`{ scopes, includesCredentials, appSettings }`）。
/// 后端遍历 `SCOPES` 组装；`includesCredentials=false` 时按 `CREDENTIAL_KEYS` 逐条剥离
/// 凭据（**备份文件里不得含凭据**）。
///
/// ⚠️ 与 `crates/buddy-switch-server/src/api.rs::api_export_backup` 是**同款接线**
/// （刻意不去重，见 P7 共享知识）。
#[tauri::command(rename_all = "camelCase")]
pub fn export_backup(selection: Value) -> Result<Value, String> {
    let selection: backup::BackupSelection =
        serde_json::from_value(selection).map_err(|error| format!("导出参数不合法：{error}"))?;
    backup::export_backup(&selection)
}

/// ★ P7 T02：导入**预览**（**只读安全阀**）—— 逐 scope 计数 + 需重新登录数 + 覆盖配置项。
///
/// `policy` 是冲突策略字符串（`keepLocal` / `overwrite` / `keepBoth`；空 ⇒ 默认保留本地）。
/// **不写盘**：预览不得改动任何本地数据（`backup::preview_import_backup` 只 dry-run）。
///
/// ⚠️ 与 `crates/buddy-switch-server/src/api.rs::api_preview_import_backup` 是**同款接线**
/// （刻意不去重，见 P7 共享知识）。
#[tauri::command(rename_all = "camelCase")]
pub fn preview_import_backup(file_text: String, policy: String) -> Result<Value, String> {
    let policy = export_import::ConflictPolicy::parse(&policy)?;
    backup::preview_import_backup(&file_text, policy)
}

/// ★ P7 T02：导入全局备份（**先快照** → 逐 scope 合并 → 汇总，含失败 / 可重试）。
///
/// `selection` 与导出同形（`{ scopes, includesCredentials }`）；缺省 ⇒ 全选（D14）。
/// 冲突策略默认**保留本地**（D16）⇒ 连导两次第二次全「跳过」（幂等）。
///
/// ★ `only_scopes`（C1）：非空 ⇒ **只导入这些 scope**，即「只重试失败项」。
/// 失败单元就是 scope（`backup::import_backup` 逐 scope 事务），前端把上次的
/// `retryableScopes` 原样回传即可，**无需重新选择文件**。
#[tauri::command(rename_all = "camelCase")]
pub fn import_backup(
    file_text: String,
    policy: String,
    selection: Value,
    only_scopes: Option<Vec<String>>,
) -> Result<Value, String> {
    let policy = export_import::ConflictPolicy::parse(&policy)?;
    let selection: backup::BackupSelection =
        serde_json::from_value(selection).map_err(|error| format!("导入参数不合法：{error}"))?;
    let only_scopes = only_scopes.unwrap_or_default();
    backup::import_backup(&file_text, policy, &selection, &only_scopes)
}

/// P7 T02：列出自动备份 / 导入前快照（最新在前）。
#[tauri::command]
pub fn list_auto_backups() -> Result<Value, String> {
    backup::list_auto_backups()
}

/// Q1：导出**诊断包**（只读；凭据按 `backup::CREDENTIAL_KEYS` 剥离后才返回）。
///
/// 返回 `{ name, text }`：`text` 是可直接落盘的 JSON。凭据闸门在 core 内
/// （命中即 `Err`），前端只负责存盘 —— **不必**在前端再实现一遍剥离。
#[tauri::command]
pub fn export_diagnostic() -> Result<Value, String> {
    let text = diagnostics::export_diagnostic()?;
    let name = format!(
        "buddy-switch-diagnostic-{}.json",
        buddy_switch_core::modules::cst::cst_datetime(config::now_ms()).format("%Y%m%d-%H%M%S")
    );
    Ok(serde_json::json!({ "name": name, "text": text }))
}

/// P7 T02：从自动备份 / 导入前快照**恢复**（覆盖策略；恢复本身也可撤销）。
#[tauri::command(rename_all = "camelCase")]
pub fn restore_auto_backup(file_name: String) -> Result<Value, String> {
    backup::restore_auto_backup(&file_name)
}

/// P7 T02：读取自动备份配置（**默认开**，可在设置里关掉，§7 #7）。
#[tauri::command]
pub fn get_auto_backup_config() -> Result<Value, String> {
    Ok(config::load_auto_backup_config())
}

/// P7 T02：保存自动备份配置（关掉后后台任务不再产生新备份，**已产生的保留**）。
#[tauri::command(rename_all = "camelCase")]
pub fn save_auto_backup_config(config: Value) -> Result<Value, String> {
    config::save_auto_backup_config(&config)
        .map_err(|error| format!("保存自动备份配置失败：{error}"))?;
    Ok(config::load_auto_backup_config())
}

/// 打开系统设置授权面板。默认「完全磁盘访问」（该 anchor 各版本均有效）；
/// 传 `target="app_management"` 尝试「App 管理」（macOS 15+，部分版本不支持深链）。
///
/// 使用 macOS 13+ 深链接格式（`com.apple.settings.PrivacySecurity.extension?Privacy_*`）。
#[tauri::command]
pub fn open_permission_settings(target: Option<String>) -> Result<(), String> {
    let t = target.unwrap_or_else(|| "all_files".to_string());
    let url = match t.as_str() {
        "app_management" => {
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AppManagement"
        }
        _ => {
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles"
        }
    };
    let _ = std::process::Command::new("open").arg(url).spawn();
    Ok(())
}

/// 权限自检：尝试在认证文件目录写/删探针文件，确认完全磁盘访问等授权是否生效。
///
/// `async` + `spawn_blocking`：探针是**写盘 + 删盘**（慢 IO），不能跑在 Tauri 主线程上。
/// 返回形状与判据都不变 —— 探针失败本来就要以 `ok:false` 回报，不是错误。
#[tauri::command]
pub async fn check_auth_permission() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let path = auth_file::auth_file_path();
        let probe = path.with_file_name("workbuddy-desktop.info.probe");
        match std::fs::write(&probe, "probe") {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                json!({ "ok": true, "message": "认证目录可写，权限正常" })
            }
            Err(e) => json!({
                "ok": false,
                "error": e.to_string(),
                "dir": path.parent().map(|p| p.to_string_lossy().to_string()),
                "hint": "请在 系统设置→隐私与安全性 中授权：优先「App 管理」开启 buddy-switch，若没有则去「完全磁盘访问」把 buddy-switch 拖进去；授权后需重启 App 生效",
            }),
        }
    })
    .await
    .map_err(|error| format!("权限自检失败: {error}"))
}

/// 在 Finder 中显示当前 App（便于拖拽到「完全磁盘访问」授权框）。
#[tauri::command]
pub fn reveal_app_in_finder() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(&exe)
        .spawn();
    Ok(())
}

/// 在系统文件管理器中打开目录（跨平台）。
fn reveal_dir(dir: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    #[cfg(any(target_os = "windows", target_os = "macos", unix))]
    {
        std::process::Command::new(program)
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开目录失败: {error}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        let _ = dir;
        Err("当前平台不支持打开目录".to_string())
    }
}

/// POST /api/accounts/open-dir —— 在文件管理器中打开该 region 的账号库目录。
///
/// 账号库落在 `~/.buddy-switch/`（`accounts.json` / `accounts.global.json`），
/// 因此这里统一打开账号文件所在目录；目录不存在时先创建，避免「打开失败」。
#[tauri::command]
pub async fn open_accounts_dir(region: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || build_open_accounts_dir(region))
        .await
        .map_err(|error| format!("打开账号库目录失败: {error}"))?
}

fn build_open_accounts_dir(region: Option<String>) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let file = buddy_switch_core::modules::region::accounts_file_for(region);
    let dir = file
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| file.clone());
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|error| format!("创建账号库目录失败: {error}"))?;
    }
    reveal_dir(&dir)?;
    Ok(json!({ "ok": true, "region": region.as_str(), "dir": dir.to_string_lossy() }))
}

// ---- Skill 管理（cc-switch 式）------------------------------------------------
// ⚠️ 与 `crates/buddy-switch-server/src/api.rs` 的 5 个同名 handler **同契约**
// （两宿主同步）。两侧都调同一个 core `skill::*` 函数并原样返回其结果，
// 本层**不自己拼 JSON 形状** —— 从根上杜绝「一侧透传、另一侧多包一层」的漂移（PRD §9）。
//
// 鉴权：5 条命令都读写**用户级文件**（`{data_dir}/skills`），沿用最贴近该语义的既有
// capability `SessionCopy`（同为「WorkBuddy 桌面才有的用户数据读写」）。capability 表在
// core（`product_spec`），本层只查表，**默认拒绝**。
//
// `async` + `spawn_blocking`：清单要递归读盘 + 解析 frontmatter，启停 / 搬迁 / 导入都是
// 写盘（可能几 MB），不能跑在 Tauri 主线程上（否则 loading 遮罩无法渲染）。

/// GET /api/skills —— 列出该 region 用户级 skills 目录下的 skill 清单（**只读**）。
#[tauri::command]
pub async fn list_skills(region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    tauri::async_runtime::spawn_blocking(move || skill::list_skills(region))
        .await
        .map_err(|error| format!("读取 skill 清单失败: {error}"))?
}

/// POST /api/skills/enabled —— 启停一个 skill（行级改写官方原生字段，幂等）。
#[tauri::command(rename_all = "camelCase")]
pub async fn set_skill_enabled(
    region: Option<String>,
    dir_name: String,
    enabled: bool,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    tauri::async_runtime::spawn_blocking(move || {
        skill::set_skill_enabled(region, &dir_name, enabled)
    })
    .await
    .map_err(|error| format!("启停 skill 失败: {error}"))?
}

/// POST /api/skills/copy-cross —— 跨 region「合并补齐」搬迁（冲突保留目标侧）。
///
/// `sourceRegion` / `targetRegion` **都必须显式给出**（跨区命令，无合理缺省）。
#[tauri::command(rename_all = "camelCase")]
pub async fn copy_skills_cross(
    source_region: Option<String>,
    target_region: Option<String>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    tauri::async_runtime::spawn_blocking(move || {
        skill::copy_skills_cross(source_region, target_region)
    })
    .await
    .map_err(|error| format!("跨区搬迁 skill 失败: {error}"))?
}

/// POST /api/skills/import —— 从本地目录或 zip 导入一个 skill。
#[tauri::command(rename_all = "camelCase")]
pub async fn import_skill(region: Option<String>, source_path: String) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    tauri::async_runtime::spawn_blocking(move || skill::import_skill(region, &source_path))
        .await
        .map_err(|error| format!("导入 skill 失败: {error}"))?
}

/// POST /api/skills/open-dir —— 在文件管理器中打开该 region 的 skills 目录。
#[tauri::command]
pub async fn open_skills_dir(region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (dir, payload) = skill::prepare_skills_dir(region)?;
        reveal_dir(&dir)?;
        Ok(payload)
    })
    .await
    .map_err(|error| format!("打开 skills 目录失败: {error}"))?
}

// ---- 上游客户端 settings.json 统一托管 ----------------------------------------
// ⚠️ 与 `crates/buddy-switch-server/src/api.rs` 的 6 个同名 handler **同契约**（两宿主同步）。
// 两侧都调同一个 core `client_settings::*` 函数并原样返回其结果，本层**不自己拼 JSON 形状**
// —— 从根上杜绝「一侧透传、另一侧多包一层」的漂移（PRD §9）。
//
// 无 capability 门控：本模块跨 WorkBuddy / Trae 两个产品线托管**各自客户端**的本地设置文件，
// 平台能力表里没有与之对应的能力项（不是签到 / 积分 / 会话复制之类），故不套用任何平台能力。
//
// `async` + `spawn_blocking`：读写都要落盘（含 `tasklist` 进程探测，慢），不能跑在 Tauri
// 主线程上（否则 loading 遮罩无法渲染）。

/// GET /api/client-settings/slots —— 列出 6 个槽位（未安装者 `installed:false` **仍在列表**）。
#[tauri::command]
pub async fn list_client_settings_slots() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(client_settings::list_slots)
        .await
        .map_err(|error| format!("列出客户端设置槽位失败: {error}"))
}

/// GET /api/client-settings/read —— 原文 + 解析后键值 + 格式指纹。
#[tauri::command(rename_all = "camelCase")]
pub async fn read_client_settings(slot: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || client_settings::read_settings(&slot))
        .await
        .map_err(|error| format!("读取客户端设置失败: {error}"))?
}

/// POST /api/client-settings/patch —— 键值级 patch（**格式保持**，模块内部自动备份）。
#[tauri::command(rename_all = "camelCase")]
pub async fn patch_client_settings(slot: String, patch: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || client_settings::patch_settings(&slot, &patch))
        .await
        .map_err(|error| format!("写入客户端设置失败: {error}"))?
}

/// GET /api/client-settings/backups —— 该槽位的历史备份点（按 id 降序）。
#[tauri::command(rename_all = "camelCase")]
pub async fn list_client_settings_backups(slot: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || client_settings::list_backups(&slot))
        .await
        .map_err(|error| format!("列出客户端设置备份失败: {error}"))?
}

/// POST /api/client-settings/restore —— 整文件恢复（**恢复前对当前文件再备份一次**）。
#[tauri::command(rename_all = "camelCase")]
pub async fn restore_client_settings_backup(
    slot: String,
    backup_id: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || client_settings::restore_backup(&slot, &backup_id))
        .await
        .map_err(|error| format!("恢复客户端设置失败: {error}"))?
}

/// POST /api/client-settings/diff —— 任选两个槽位比对（仅 A 有 / 仅 B 有 / 值不同）。
#[tauri::command(rename_all = "camelCase")]
pub async fn diff_client_settings(slot_a: String, slot_b: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || client_settings::diff_slots(&slot_a, &slot_b))
        .await
        .map_err(|error| format!("比对客户端设置失败: {error}"))?
}

/// POST /api/switch —— 切换账号（备份 → 关进程 → 复制会话 → 写认证 → 重启，按 region）。
///
/// async + spawn_blocking：切换中关闭/启动 WorkBuddy 会阻塞数十秒，
/// 若在同步 command（主线程）执行会卡死整个 UI（loading 遮罩无法渲染）。
#[tauri::command(rename_all = "camelCase")]
pub async fn switch_account(
    app: tauri::AppHandle,
    account_id: String,
    region: Option<String>,
    restart: Option<bool>,
    copy_session_ids: Option<Vec<String>>,
    source_region: Option<String>,
) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少 accountId".to_string());
    }
    let region = parse_region(region.as_deref());
    // 会话复制的来源版本；缺省与目标版本相同（同版本内切换，行为零变化）。
    let source_region = match source_region.as_deref() {
        Some(v) => parse_region(Some(v)),
        None => region,
    };
    let restart = restart.unwrap_or(true);
    let copy_ids = copy_session_ids.unwrap_or_default();

    // 与 webui 的 `/api/switch/progress` 同契约：同步写入进程内进度缓存，
    // 供 `switch_progress` 轮询；事件 `switch-progress` 仍照常派发。
    {
        let mut running = SWITCH_RUNNING.lock().unwrap();
        if *running {
            return Err("已有切换任务进行中".to_string());
        }
        *running = true;
        *SWITCH_PROGRESS.lock().unwrap() = Some("开始切换账号…".to_string());
        *SWITCH_REGION.lock().unwrap() = Some(region);
    }

    let progress: switch::ProgressFn = Box::new(move |message| {
        *SWITCH_PROGRESS.lock().unwrap() = Some(message.to_string());
        let _ = app.emit("switch-progress", json!({ "message": message }));
    });
    let result = tauri::async_runtime::spawn_blocking(move || {
        switch::switch_account_cross(
            region,
            source_region,
            Some(&progress),
            &account_id,
            restart,
            &copy_ids,
        )
    })
    .await;

    *SWITCH_RUNNING.lock().unwrap() = false;
    *SWITCH_REGION.lock().unwrap() = None;

    result.map_err(|e| e.to_string())?
}

/// GET /api/switch/progress —— 轮询切换进度（与 webui 同契约）。
#[tauri::command]
pub fn switch_progress() -> Value {
    let progress = SWITCH_PROGRESS.lock().unwrap().clone();
    let running = *SWITCH_RUNNING.lock().unwrap();
    json!({ "running": running, "progress": progress })
}

/// GET /api/sessions —— 当前账号的会话列表（按 region）。
///
/// 用 `list_sessions_with_fallback_for` 透传 `source` / `warning`：
/// `source: "db"` 正常、`"scan"` 表示 db 不可读已降级扫描 projects 目录、
/// `"empty"` 表示两个都空。前端据此区分「账号无会话」与「数据库不可读」。
///
/// `async` + `spawn_blocking`：要读 `workbuddy.db`（**慢 IO**；db 不可读时还会降级扫
/// `projects` 目录，更慢），不能跑在 Tauri 主线程上。
#[tauri::command]
pub async fn list_sessions(region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || match session::current_user_uid_for(region) {
        Some(uid) => {
            let resp = session::list_sessions_with_fallback_for(region, &uid);
            json!({
                "sessions": resp.get("sessions").cloned().unwrap_or_else(|| json!([])),
                "current": uid,
                "source": resp.get("source").cloned().unwrap_or_else(|| json!("db")),
                "warning": resp.get("warning").cloned(),
            })
        }
        None => json!({ "sessions": [], "current": Value::Null, "source": "empty" }),
    })
    .await
    .map_err(|error| format!("读取会话列表失败: {error}"))
}

/// GET /api/sessions/replicas —— 只读：把 copy ledger 连通成「会话副本组」（按 region）。
///
/// ★ 数据源是**内部去重账本**（`session.rs:928`），P8 把它提升为「副本归属」的产品语义
/// ⇒ 只有**只读**命令，**不改 ledger 结构、不新增模型**。**只读当前 region** 的账本
/// （P8 ✅ #2）。
///
/// ⚠️ **只回 ledger 里确有的字段**（`accountUid` / `sessionId` / `region`）；
/// `title` / `updatedAt` / `hasHistory` 由**前端**用 `list_sessions` / 账号 store join 补齐
/// —— 后端**不臆造**（`P8-ARCH.md` §3.1 注）。
///
/// ★ **内容状态（`content`）随本响应一起回**：状态由 jsonl 指纹派生，而「每个副本
/// 前 512 行逐行 hash」过 IPC 是 200 MB 级的包袱（真机 25,487 个副本）⇒ 判据留在 core
/// （`session::derive_content_status`），前端只拿**结论**（状态 / 共同前缀 / 每副本增量）。
/// ⇒ 前端**不需要**第二个请求（原 `session_fingerprints` 已随之下线）。
///
/// `async` + `spawn_blocking`：读账本 + 算指纹都是磁盘 IO，不跑在 Tauri 主线程上
/// （与 `list_sessions` 同款处理）。
#[tauri::command]
pub async fn list_session_replicas(region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || session::list_replica_groups_for(region))
        .await
        .map_err(|error| format!("读取会话副本失败: {error}"))
}

/// POST /api/sessions/sync —— P8 **安全同步（增量追加）**：把源副本的新增行追加到落后副本。
///
/// 与 server 侧 `api_session_sync` **同契约**（两宿主同步）。批量 + 逐条结果
/// ⇒ 支持**部分失败**（`PLAN.md` §3.6）；判据在 core（`session::sync_session_replicas_for`），
/// 本命令只做「鉴权 + 取参数 + 丢到阻塞线程」。
///
/// ⚠️ **写操作**：会往目标会话 jsonl **追加**行 —— 只追加，**绝不改写 / 删除**目标已有内容；
/// 出现分歧则**逐条**失败且不落一个字节。
#[tauri::command(rename_all = "camelCase")]
pub async fn sync_session_replicas(
    items: Vec<Value>,
    region: Option<String>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || session::sync_session_replicas_for(region, &items))
        .await
        .map_err(|error| format!("同步会话副本失败: {error}"))
}

/// POST /api/sessions/unify —— P8 **分歧处理（覆盖式统一）**：把用户选定保留的那一份覆盖到其余副本。
///
/// 与 server 侧 `api_session_unify` **同契约**（两宿主同步）。批量 + 逐条结果 ⇒ 支持**部分失败**
/// （`PLAN.md` §3.6）；判据在 core（`session::unify_session_replicas_for`），本命令只做
/// 「鉴权 + 取参数 + 丢到阻塞线程」。
///
/// ⚠️ **破坏性写操作**：目标副本原有的分歧内容会被丢弃（`PLAN.md` §3.15.2「不许静默覆盖」）
/// ⇒ 前端**必须**先二次确认并把 `lostLines` 如实告知；core 会**先备份再覆盖**，备份失败则不覆盖。
#[tauri::command(rename_all = "camelCase")]
pub async fn unify_session_replicas(
    items: Vec<Value>,
    region: Option<String>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        session::unify_session_replicas_for(region, &items)
    })
    .await
    .map_err(|error| format!("统一会话副本失败: {error}"))
}

/// 把前端的 `client` 字符串解析成 [`vscode_session::ExtClient`]。
///
/// ⚠️ **空串与未知值一律 `Err`**（不回落默认值）：本仓不猜「用户指的是哪个客户端」——
/// 猜错就是去读写**另一个客户端**的会话目录。前端必须显式传。
fn parse_ext_client(raw: &str) -> Result<vscode_session::ExtClient, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "vscode" => Ok(vscode_session::ExtClient::Vscode),
        "codebuddy-ide" => Ok(vscode_session::ExtClient::CodebuddyIde),
        other => Err(format!("未知的客户端标识: {other}")),
    }
}

/// GET /api/vscode-sessions —— 列出 VS Code 插件 / CodeBuddy IDE 某账号的会话（**只读**）。
///
/// ★ **不往 `list_sessions` 里塞 `client` 参数**：桌面版会话是 `workbuddy.db` + jsonl，
/// 插件侧是 **JSON 记录仓**（见 `vscode_session.rs` 模块头的布局）——**两套形状**
/// 挤进一个出口会让前端处处分支。单开命令，形状各自诚实。
///
/// `uid` **必填**：本仓不猜「插件当前登录的是哪个账号」——猜错就是去读**别人的**会话目录。
/// 前端要当前账号时，从插件登录态（`vscode_ext::status`）里拿。
///
/// `async` + `spawn_blocking`：要遍历客户端数据仓（磁盘 IO），不跑在 Tauri 主线程上。
#[tauri::command]
pub async fn list_vscode_sessions(client: String, uid: String) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let client = parse_ext_client(&client)?;
    tauri::async_runtime::spawn_blocking(move || vscode_session::list_sessions_for(client, &uid))
        .await
        .map_err(|error| format!("读取插件会话列表失败: {error}"))
}

/// POST /api/vscode-sessions/copy —— 把插件侧会话复制给另一个账号（**加法**，绝不覆盖）。
///
/// 与 server 侧 `api_copy_vscode_sessions` **同契约**（两宿主同步）。
/// 逐条结果 ⇒ 支持**部分失败**；`items` 元素形如 `{ workspaceHash, conversationId }`。
///
/// ⚠️ **数据根找不到 ⇒ `Err`**（不是「复制了 0 条」）——后者会把「客户端没装」
/// 伪装成一次成功的空操作（「失败不得谎报成功」）。
#[tauri::command]
pub async fn copy_vscode_sessions(
    client: String,
    source_uid: String,
    target_uid: String,
    items: Vec<Value>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let client = parse_ext_client(&client)?;
    let items: Vec<vscode_session::CopyItem> = items
        .into_iter()
        .map(|item| serde_json::from_value(item).map_err(|e| format!("复制条目格式不对: {e}")))
        .collect::<Result<_, _>>()?;
    tauri::async_runtime::spawn_blocking(move || {
        vscode_session::copy_conversations(client, &source_uid, &target_uid, &items)
    })
    .await
    .map_err(|error| format!("复制插件会话失败: {error}"))?
}

/// GET /api/vscode-sessions/diff —— **记录级 diff（只读）**：这份会话能不能做增量追加。
///
/// 与 server 侧 `api_diff_vscode_sessions` **同契约**（两宿主同步）。
///
/// ★ **插件侧不能照搬桌面版的「按行 append」**：桌面版是 jsonl（追加天然保序），
/// 插件侧的顺序由 `index.json` 的 `messages[]` **数组次序**决定 ⇒ 目标缺的是**中间**几条时，
/// 追加会把会话读成**错乱的顺序**。⇒ 先算 diff、再决定能不能同步。
///
/// 返回的 `status`：`error` / `absent` / `divergent` / `unordered` / `identical` / `appendable`
/// （**判据顺序即优先级**，详见 core `diff_conversation_in` 的文档）。
#[tauri::command]
pub async fn diff_vscode_sessions(
    client: String,
    source_uid: String,
    target_uid: String,
    workspace_hash: String,
    conversation_id: String,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let client = parse_ext_client(&client)?;
    tauri::async_runtime::spawn_blocking(move || {
        vscode_session::diff_conversation(
            client,
            &source_uid,
            &target_uid,
            &workspace_hash,
            &conversation_id,
        )
    })
    .await
    .map_err(|error| format!("比较插件会话失败: {error}"))
}

/// POST /api/vscode-sessions/sync —— **增量追加**（把源尾部多出来的记录补到目标）。
///
/// 与 server 侧 `api_sync_vscode_sessions` **同契约**（两宿主同步）。
///
/// ★★ **只有 diff 说 `appendable` 才会真写** —— 而且**不信任调用方**：
/// core 里进来先**自己重算** diff（调用方说「我算过了」不算数，两件事之间源可能又变了）。
/// 其余状态一律**拒绝**并**如实说原因**（`divergent` 会静默改写目标内容、
/// `unordered` 会打乱会话顺序 —— 两者都看不出来）。
///
/// ⚠️ 数据根找不到 ⇒ `Err`（不是「同步了 0 条」）。
#[tauri::command]
pub async fn sync_vscode_sessions(
    client: String,
    source_uid: String,
    target_uid: String,
    workspace_hash: String,
    conversation_id: String,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let client = parse_ext_client(&client)?;
    tauri::async_runtime::spawn_blocking(move || {
        vscode_session::sync_conversation(
            client,
            &source_uid,
            &target_uid,
            &workspace_hash,
            &conversation_id,
        )
    })
    .await
    .map_err(|error| format!("同步插件会话失败: {error}"))?
}

/// GET /api/client/limits —— 客户端限流台账（**只读**观测：账号 × 模型 × 恢复时刻）。
///
/// 与 server 侧 `api_client_limits` **同契约**（两宿主同步）。判据在 core
/// （`client_limits::client_limits_for`），本命令只做「鉴权 + 取参数 + 丢到阻塞线程」。
///
/// ⚠️ **只读**：不写 `workbuddy.db`、不改账号状态、不触发切换（PRD R4 / R8）。
/// `async` + `spawn_blocking`：要递归扫 `projects/**/*.jsonl` + 只读查 db（慢 IO）。
#[tauri::command(rename_all = "camelCase")]
pub async fn client_limits(
    region: Option<String>,
    since_days: Option<i64>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        client_limits::client_limits_for(region, since_days)
    })
    .await
    .map_err(|error| format!("读取客户端限流台账失败: {error}"))
}

/// POST /api/sessions/copy —— 把勾选会话复制到指定账号（路径 B，按 region）。
#[tauri::command(rename_all = "camelCase")]
pub async fn copy_sessions(
    target_account_id: String,
    session_ids: Vec<String>,
    region: Option<String>,
    source_region: Option<String>,
) -> Result<Value, String> {
    if target_account_id.trim().is_empty() {
        return Err("缺少 targetAccountId".to_string());
    }
    if session_ids.is_empty() {
        return Err("缺少 sessionIds".to_string());
    }
    let region = parse_region(region.as_deref());
    let source_region = match source_region.as_deref() {
        Some(v) => parse_region(Some(v)),
        None => region,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let target =
            account::find_account_for(region, &target_account_id).ok_or("目标账号不存在")?;
        // 独立复制入口没有切换进度事件通道，传 None（三段管线的耗时优化照常生效）。
        Ok(session::copy_sessions_for_switch_cross(
            source_region,
            region,
            None,
            &target,
            &session_ids,
        )
        .unwrap_or_else(|| json!({})))
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// 账号数据迁移（Memory / Connector 合并去重）
// ---------------------------------------------------------------------------

/// POST /api/migrate/account —— 把源账号的 Memory / Connector 合并到目标账号（带去重）。
///
/// 只处理普通文件，不触碰 `workbuddy.db`，因此无需关闭 WorkBuddy。
#[tauri::command(rename_all = "camelCase")]
pub async fn migrate_account_data(
    target_account_id: String,
    source_account_id: Option<String>,
    memory: Option<bool>,
    connectors: Option<bool>,
    region: Option<String>,
    source_region: Option<String>,
) -> Result<Value, String> {
    if target_account_id.trim().is_empty() {
        return Err("缺少 targetAccountId".to_string());
    }
    let region = parse_region(region.as_deref());
    let source_region = match source_region.as_deref() {
        Some(v) => parse_region(Some(v)),
        None => region,
    };
    let source_account_id = source_account_id.unwrap_or_default().trim().to_string();

    tauri::async_runtime::spawn_blocking(move || {
        let scope = migrate::MigrateScope {
            memory: memory.unwrap_or(true),
            connectors: connectors.unwrap_or(true),
        };
        if scope.is_empty() {
            return Err("未指定任何迁移范围".to_string());
        }

        let target =
            account::find_account_for(region, &target_account_id).ok_or("目标账号不存在")?;
        let target_uid = target
            .get("uid")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if target_uid.is_empty() {
            return Err("目标账号缺少 uid".to_string());
        }

        let source_uid = if source_account_id.is_empty() {
            session::current_user_uid_for(source_region).unwrap_or_default()
        } else {
            let acc = account::find_account_for(source_region, &source_account_id)
                .ok_or("源账号不存在")?;
            acc.get("uid")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        if source_uid.is_empty() {
            return Err("无法确定源账号 uid（未登录或账号缺少 uid）".to_string());
        }

        migrate::migrate_account_data_cross(source_region, &source_uid, region, &target_uid, scope)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// 阶段 3：签到 + token 刷新
// ---------------------------------------------------------------------------

/// GET /api/checkin/status —— 查询单账号签到状态（按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`checkin_status`]（带 `scope`）。本命令保留
/// （webui 路由 / `generate_handler!` / 前端 `ALIASES` 表），是 [`checkin_status`] 的
/// WorkBuddy 分支同源实现。
///
/// ⚠️ **桌面端单账号 vs webui 批量**：本命令（桌面端）是单账号的；同路径的 webui
/// `GET /api/checkin/status` 却是**整端点批量**（见 `api.rs::api_checkin_status`）。这条
/// 通道差异**刻意保留**（批量在 webui 是性能优化，B7），参数化命令 [`checkin_status`]
/// 只对齐桌面端语义。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_checkin_status(
    account_id: String,
    region: Option<String>,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let acc = account::find_account_for(region, &account_id).ok_or("账号不存在")?;
    Ok(checkin::get_checkin_status_for(region, &acc).await)
}

/// POST /api/credits —— 查询单账号积分资源及到期时间（按 region）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`credits`]（带 `scope`）。本命令保留
/// （webui 路由 / `generate_handler!` / 前端 `ALIASES` 表），是 [`credits`] 的
/// WorkBuddy 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_credit_expiry(
    account_id: String,
    region: Option<String>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::Credits)?;
    let region = parse_region(region.as_deref());
    let acc = account::find_account_for(region, &account_id).ok_or("账号不存在")?;
    Ok(credits::get_credit_expiry_for(region, &acc).await)
}

/// GET /api/credits/stats —— 本地快照与官方请求用量统计（按 region）。
/// `refresh = true` 时才重新请求官方用量；默认读缓存。
#[tauri::command]
pub async fn get_credit_statistics(refresh: Option<bool>, region: Option<String>) -> Value {
    let filter = parse_region_filter(region.as_deref());
    credit_usage::get_statistics_for_filter(filter, refresh.unwrap_or(false)).await
}

#[tauri::command]
pub async fn get_token_statistics(
    days: Option<i64>,
    region: Option<String>,
) -> Result<Value, String> {
    let filter = parse_region_filter(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        token_stats::get_statistics_for_filter(filter, days)
    })
    .await
    .map_err(|error| format!("扫描 Token 统计失败: {error}"))
}

/// 网关逐日用量趋势（读 `token_daily` 的 90 天落库历史；**只读**，不扫盘、不触发记录）。
///
/// 返回 `{ date, retentionDays, days: [{ key, input, output, total, truncated }], total }`（camelCase）。
/// ⚠️ `days` **只含落库过的日子**（App 运行过 + 后台周期记录在跑）⇒ 缺日 = 未记录、
/// **不是 0**；某日 `truncated: true` ⇒ 该日总量只是**下界**（网关日志被裁）。
/// 前端按窗口 join 后必须算覆盖率并披露（见 `usage-charts.ts` 的网关适配）。
#[tauri::command]
pub async fn get_gateway_token_trend(region: Option<String>) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || token_daily::gateway_token_trend(region))
        .await
        .map_err(|error| format!("读取网关用量趋势失败: {error}"))
}

/// 今日 Token：账号 × 来源（client / gateway）的当日累计。
///
/// 返回 `{ date, accounts: [{ uid, clientIn, clientOut, gatewayIn, gatewayOut }],
/// unknownClient: { in, out } | null, gatewayTruncated: bool }`（camelCase）。
///
/// `force = true` 时强制重扫客户端 JSONL（当日数据在扫过之后仍会增长）；否则命中
/// 「今日已扫」闸门、直接读缓存。`async` + `spawn_blocking`：可能递归扫
/// `projects` 目录下的 jsonl + 只读查 db（慢 IO）。
#[tauri::command]
pub async fn get_account_token_today(
    region: Option<String>,
    force: Option<bool>,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        token_daily::record_client_today(region, force);
        token_daily::record_gateway_today(region);
        token_daily::account_token_today(region)
    })
    .await
    .map_err(|error| format!("统计今日 Token 失败: {error}"))
}

/// POST /api/checkin —— 单账号立即签到（按 region）。
#[tauri::command(rename_all = "camelCase")]
pub async fn checkin(account_id: String, region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::Checkin)?;
    let region = parse_region(region.as_deref());
    let acc = account::find_account_for(region, &account_id).ok_or("账号不存在")?;
    Ok(checkin::checkin_account_for(region, &acc).await)
}

/// POST /api/checkin/all —— 全部账号立即签到（按 region）。
#[tauri::command]
pub async fn checkin_all(region: Option<String>) -> Value {
    let region = parse_region(region.as_deref());
    // P15 · 签到进悬浮窗（首尾两条：WB 侧没有逐账号进度事件，见
    // `observe_workbuddy_checkin_start` 的说明）。
    let task = observe_workbuddy_checkin_start(region);
    let result = checkin::run_checkin_all_for(region).await;
    observe_workbuddy_checkin_end(&task, region, &result);
    result
}

/// GET /api/checkin/config —— 自动签到配置。
#[tauri::command]
pub fn get_auto_checkin_config() -> Value {
    crate::modules::config::load_checkin_config()
}

/// POST /api/checkin/config —— 保存自动签到配置。
#[tauri::command]
pub fn save_auto_checkin_config(config: Value) -> Result<Value, String> {
    crate::modules::config::save_checkin_config(&config).map_err(|e| e.to_string())?;
    Ok(crate::modules::config::load_checkin_config())
}

/// GET /api/checkin/logs —— 签到日志。
#[tauri::command]
pub fn get_checkin_logs() -> Value {
    json!({ "logs": crate::modules::config::load_checkin_logs() })
}

// ---------------------------------------------------------------------------
// 派猫猫旅行
// ---------------------------------------------------------------------------

/// GET /api/travel/status —— 查询单账号今日旅行状态标签。
#[tauri::command]
pub async fn get_travel_status(account_id: String) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::Travel)?;
    account::find_account(&account_id).ok_or("账号不存在")?;
    travel::reconcile_due_travel(Some(account_id.as_str())).await;
    Ok(travel::travel_display(&account_id))
}

/// GET /api/travel/config —— 自动旅行配置。
#[tauri::command]
pub fn get_auto_travel_config() -> Value {
    crate::modules::config::load_travel_config()
}

/// POST /api/travel/config —— 保存自动旅行配置。开启时立刻跑一轮派发/领取。
#[tauri::command]
pub fn save_auto_travel_config(config: Value) -> Result<Value, String> {
    crate::modules::config::save_travel_config(&config).map_err(|e| e.to_string())?;
    let saved = crate::modules::config::load_travel_config();
    if saved.get("enabled").and_then(Value::as_bool) == Some(true) {
        tauri::async_runtime::spawn(async {
            let _ = travel::run_travel_cycle().await;
            let _ = travel::run_travel_claim_cycle().await;
        });
    }
    Ok(saved)
}

// ---------------------------------------------------------------------------
// 账号切换 / 账号列表展示（全局单份，无需 region）
// ---------------------------------------------------------------------------

/// GET /api/switch/config —— 账号切换与账号列表展示配置。
#[tauri::command]
pub fn get_switch_config() -> Value {
    crate::modules::config::load_switch_config()
}

/// POST /api/switch/config —— 保存账号切换配置。
#[tauri::command]
pub fn save_switch_config(config: Value) -> Result<Value, String> {
    crate::modules::config::save_switch_config(&config).map_err(|e| e.to_string())?;
    Ok(crate::modules::config::load_switch_config())
}

// ---------------------------------------------------------------------------
// 定时任务排程（六类积分任务，全局单份，无需 region）
// ---------------------------------------------------------------------------

/// GET /api/schedule/config —— 六类定时任务的排程配置。
#[tauri::command]
pub fn get_schedule_config() -> Value {
    crate::modules::schedule::schedule_to_value(&crate::modules::schedule::load_schedule_config())
}

/// POST /api/schedule/config —— 保存排程配置；小时越界返回 Err（文案指向对应 `*_enabled`）。
#[tauri::command]
pub fn save_schedule_config(config: Value) -> Result<Value, String> {
    let cfg = crate::modules::schedule::save_schedule_config(&config)?;
    Ok(crate::modules::schedule::schedule_to_value(&cfg))
}

/// POST /api/schedule/run —— **立即**执行某一类定时任务，不等排程到点。
///
/// 为什么需要：排程按整点触发，保存配置后无法当场自证是否生效（「活跃地图」这类要到
/// 官网对照连登热力图才知道）。手动触发让配置改动立刻可验证——上报结果会带回每个账号
/// 的 `reported` 与 `streakDays`。
///
/// `task` 取 [`schedule::ScheduleTask::as_str`] 的稳定标识；未知标识返回 Err。
#[tauri::command]
pub async fn run_schedule_task(task: String) -> Result<Value, String> {
    let Some(task) = crate::modules::schedule::ScheduleTask::parse(&task) else {
        return Err(format!("未知定时任务: {task}"));
    };
    Ok(crate::modules::scheduler::run_scheduled_task(task).await)
}

// ---------------------------------------------------------------------------
// 自动轮换（CodeBuddy CLI）
// ---------------------------------------------------------------------------

/// GET /api/rotate/config —— 自动轮换配置。
#[tauri::command]
pub fn get_auto_rotate_config() -> Value {
    crate::modules::config::load_auto_rotate_config()
}

/// POST /api/rotate/config —— 保存自动轮换配置。
#[tauri::command]
pub fn save_auto_rotate_config(config: Value) -> Result<Value, String> {
    crate::modules::config::save_auto_rotate_config(&config).map_err(|e| e.to_string())?;
    Ok(crate::modules::config::load_auto_rotate_config())
}

/// GET /api/rotate/status —— 轮换状态（配置 + 上次检查/切换）。
#[tauri::command]
pub fn rotate_status() -> Value {
    rotate::rotate_status()
}

/// POST /api/rotate/run —— 手动触发一次轮换检查。
#[tauri::command]
pub async fn run_rotate() -> Value {
    rotate::run_rotate_cycle().await
}

/// GET /api/rotate/logs —— 最近轮换日志。
#[tauri::command]
pub fn get_rotate_logs() -> Value {
    json!({ "logs": rotate::rotate_logs() })
}

/// POST /api/refresh-token —— 单账号刷新 token。
#[tauri::command]
pub async fn refresh_account_token(
    account_id: String,
    region: Option<String>,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let acc = account::find_account_for(region, &account_id).ok_or("账号不存在")?;
    let fresh = refresh::refresh_account_token_for(region, acc).await;
    Ok(account::account_meta_for(region, &fresh))
}

// ---------------------------------------------------------------------------
// 阶段 4：自动更新
// ---------------------------------------------------------------------------

/// GET /api/update/config —— 更新源配置（owner/repo/token）。
#[tauri::command]
pub fn get_github_config() -> Value {
    update::load_github_config()
}

/// POST /api/update/config —— 保存更新源配置。
#[tauri::command]
pub fn save_github_config(config: Value) -> Result<Value, String> {
    update::save_github_config(&config).map_err(|e| e.to_string())?;
    Ok(update::load_github_config())
}

/// GET /api/update/check —— 检查 GitHub Releases 是否有新版本。
/// force=true 时绕过缓存强制刷新（设置页手动检查）。
#[tauri::command]
pub async fn check_update(proxy: Option<String>, force: Option<bool>) -> Value {
    update::update_check(proxy.as_deref(), force.unwrap_or(false)).await
}

/// 启动当前应用的新进程并退出旧进程，用于更新安装完成后的立即重启。
#[tauri::command]
pub fn relaunch_app() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| format!("无法定位应用程序: {e}"))?;
    // 更新重启是普通启动路径；不要把系统自启专用参数带给新进程。
    let args = std::env::args_os().skip(1).filter(|arg| {
        #[cfg(desktop)]
        {
            should_forward_relaunch_arg(arg.as_os_str())
        }
        #[cfg(not(desktop))]
        {
            true
        }
    });
    std::process::Command::new(executable)
        .args(args)
        .spawn()
        .map_err(|e| format!("启动应用失败: {e}"))?;
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// 应用级偏好（仅桌面端；webui 没有可关闭的窗口，故不提供同名接口）
// ---------------------------------------------------------------------------

/// GET /api/app-prefs —— 读取应用级偏好里「宿主也要读」的那一类。
///
/// 目前只有 `close_to_tray`（关闭窗口是隐藏到托盘还是退出）。
/// **纯渲染层的偏好不进这里**（主题 / 语言 / 快捷键 / 自动检查更新走 `localStorage`）——
/// 判据是「谁需要读它」，见 `buddy-switch-core::modules::app_prefs` 模块头。
#[tauri::command]
pub fn get_app_prefs() -> Value {
    app_prefs::load_app_prefs()
}

/// POST /api/app-prefs —— 保存应用级偏好，返回**归一化后的回读值**。
///
/// 回读而不是回显请求体：非法值（如 `"false"` 字符串）会被归一成默认值，
/// 前端按回读值渲染才不会出现「界面说关了、宿主仍按开处理」的两边各自正确。
#[tauri::command]
pub fn set_app_prefs(prefs: Value) -> Result<Value, String> {
    app_prefs::save_app_prefs(&prefs).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 开机自启（仅桌面端；webui 不提供同名接口）
// ---------------------------------------------------------------------------

/// GET /api/launch-at-login —— 查询系统当前的开机自启注册状态。
///
/// 以 tauri-plugin-autostart 的 OS 状态为唯一事实来源，不另存本地布尔值。
#[tauri::command]
pub fn get_launch_at_login_enabled(_app: tauri::AppHandle) -> Result<bool, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        _app.autolaunch()
            .is_enabled()
            .map_err(|e| format!("查询开机自启状态失败：{e}"))
    }
    #[cfg(not(desktop))]
    {
        Err("当前平台不支持开机自启".to_string())
    }
}

#[cfg(desktop)]
fn should_forward_relaunch_arg(arg: &std::ffi::OsStr) -> bool {
    arg != std::ffi::OsStr::new(crate::tray::SILENT_STARTUP_ARG)
}

#[cfg(all(test, desktop))]
mod relaunch_tests {
    use super::should_forward_relaunch_arg;
    use std::ffi::OsStr;

    #[test]
    fn update_relaunch_drops_only_the_exact_silent_startup_arg() {
        assert!(!should_forward_relaunch_arg(OsStr::new("--hidden")));
        assert!(should_forward_relaunch_arg(OsStr::new("--hidden-x")));
        assert!(should_forward_relaunch_arg(OsStr::new("x--hidden")));
        assert!(should_forward_relaunch_arg(OsStr::new("--debug")));
    }
}

/// `parse_trae_variant` 的回归护栏。
///
/// 覆盖：合法值、大小写与空白、**未知值回落默认**（且不 panic）。
/// 与 `crates/buddy-switch-server/src/api.rs` 的同名单测**互为镜像**，
/// 两处实现必须保持一致（见 [`parse_trae_variant`] 的文档注释）。
#[cfg(test)]
mod parse_trae_variant_tests {
    use super::parse_trae_variant;
    use buddy_switch_core::modules::trae::variant::TraeVariant;

    #[test]
    fn parses_known_canonical_values() {
        assert_eq!(parse_trae_variant(Some("trae_work")), TraeVariant::TraeWork);
        assert_eq!(parse_trae_variant(Some("trae_cn")), TraeVariant::Trae);
    }

    #[test]
    fn parses_case_and_whitespace_insensitively() {
        // `TraeVariant::parse` 内部会 `trim + to_ascii_lowercase`。
        assert_eq!(parse_trae_variant(Some("TRAE_WORK")), TraeVariant::TraeWork);
        assert_eq!(parse_trae_variant(Some(" Trae CN ")), TraeVariant::Trae);
    }

    #[test]
    fn unknown_or_empty_falls_back_to_default() {
        // 未知值 / 空串 / 纯空白 / 缺省，都应回落到 `TraeVariant::default()`。
        // 断言「不 panic」本身就是这组输入的主要价值：钉死「未知值不报错」，
        // 防止未来有人把它改成 `parse(...).unwrap()`。
        let default = TraeVariant::default();
        assert_eq!(parse_trae_variant(Some("trae_bogus")), default);
        assert_eq!(parse_trae_variant(Some("")), default);
        assert_eq!(parse_trae_variant(Some("   ")), default);
        assert_eq!(parse_trae_variant(None), default);
    }

    /// ★ issue #28：区域标识必须解析成**合并版**。
    ///
    /// `useTraeVariant()` 返回的就是 `cn` / `global`，它被原样传给
    /// `trae_launch_client` / `trae_oauth_start` / `trae_open_data_dir`
    /// 三个**程序级**命令。解析成退役变体时，那三个命令会去找已被官方合并
    /// 卸载的 `TRAE SOLO CN` / `TRAE SOLO` ⇒ 报「未检测到【TraeWork CN】客户端」。
    #[test]
    fn region_ids_resolve_to_the_merged_client() {
        assert_eq!(parse_trae_variant(Some("cn")), TraeVariant::TraeMerged);
        assert_eq!(
            parse_trae_variant(Some("global")),
            TraeVariant::GlobalTraeMerged
        );
    }
}

/// `build_app_status` 的 `current.nickname` 护栏。
///
/// **与 `crates/buddy-switch-server/src/api.rs` 的
/// `status_current_nickname_falls_back_to_account_library` 互为镜像**：
/// 两个下发点（Tauri 宿主 / webui 服务）的 `current` 对象必须逐字同构
/// （见 [`build_app_status`] 里那条互指注释），因此两边的断言也必须成对。
///
/// 现场（2026-09-24 用户截图）：新版客户端把认证文件的 `account.nickname`
/// 存成加密信封 ⇒ 读不到 ⇒ 区域页签显示一串 UUID。账号库里有名字，按 uid 取回来。
///
/// 只钉**行为契约**（走 `build_app_status` 这个真实入口），
/// 回落规则本身的分支覆盖在 core `account::current_nickname_for` 的单测里。
#[cfg(test)]
mod current_nickname_tests {
    use super::build_app_status;
    use buddy_switch_core::modules::account;
    use buddy_switch_core::modules::auth_file;
    use buddy_switch_core::modules::config::BUDDY_SWITCH_HOME_ENV;
    use buddy_switch_core::modules::region::Region;
    use serde_json::json;
    use std::sync::Mutex;

    /// 本模块是 src-tauri 里**唯一**改 `BUDDY_SWITCH_HOME` 的测试；
    /// 取锁保证它不与同 crate 的其它用例并行（进程级环境变量是共享状态）。
    static LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_home<T>(f: impl FnOnce() -> T) -> T {
        let _lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-tauri-current-name-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create temp home");
        let previous = std::env::var_os(BUDDY_SWITCH_HOME_ENV);
        // `validate_home_override` 只接受**已存在**的目录，故上面先建好再设。
        std::env::set_var(BUDDY_SWITCH_HOME_ENV, &dir);
        let out = f();
        match previous {
            Some(value) => std::env::set_var(BUDDY_SWITCH_HOME_ENV, value),
            None => std::env::remove_var(BUDDY_SWITCH_HOME_ENV),
        }
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn current_nickname_falls_back_to_account_library() {
        with_temp_home(|| {
            let auth_path = auth_file::auth_file_path_for(Region::Cn);
            std::fs::create_dir_all(auth_path.parent().unwrap()).expect("create auth dir");

            account::save_accounts_for(
                Region::Cn,
                &[json!({
                    "id": "seeded-cn-1", "uid": "uid-encrypted", "nickname": "Jackey",
                    "access_token": "AT",
                })],
            )
            .expect("seed accounts");

            // 加密信封 ⇒ 读不到 ⇒ 回落账号库。
            std::fs::write(
                &auth_path,
                r#"{"account":{"uid":"uid-encrypted","nickname":{"$wbEncrypted":1,"envelope":"eyJzdWl0ZSI6MX0="}},"auth":{"accessToken":"tok"}}"#,
            )
            .expect("seed auth file");
            let status = build_app_status(Region::Cn);
            assert_eq!(
                status.current.as_ref().and_then(|c| c["nickname"].as_str()),
                Some("Jackey"),
                "认证文件读不到昵称时必须回落账号库，否则界面只能显示 uid"
            );

            // 阳性对照：认证文件有明文昵称 ⇒ 原样用，不被账号库覆盖。
            std::fs::write(
                &auth_path,
                r#"{"account":{"uid":"uid-encrypted","nickname":"认证文件里的名字"},"auth":{"accessToken":"tok"}}"#,
            )
            .expect("reseed auth file");
            let status = build_app_status(Region::Cn);
            assert_eq!(
                status.current.as_ref().and_then(|c| c["nickname"].as_str()),
                Some("认证文件里的名字"),
                "认证文件读得到昵称时不得被账号库覆盖"
            );

            let _ = std::fs::remove_file(&auth_path);
        });
    }
}

/// POST /api/launch-at-login —— 注册 / 移除系统开机自启，并回读权威状态。
///
/// 回读结果与请求值不一致时按失败处理并返回当前真实状态，避免假装设置成功。
#[tauri::command]
pub fn set_launch_at_login_enabled(_app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let autostart = _app.autolaunch();
        let action = if enabled { "开启" } else { "关闭" };
        let result = if enabled {
            autostart.enable()
        } else {
            autostart.disable()
        };
        if let Err(e) = result {
            return Err(format!("{action}开机自启失败：{e}"));
        }
        let authoritative = autostart
            .is_enabled()
            .map_err(|e| format!("开机自启设置后回读状态失败：{e}"))?;
        if authoritative != enabled {
            return Err(format!(
                "{action}开机自启未生效（系统当前状态：{}），请稍后重试",
                if authoritative {
                    "已开启"
                } else {
                    "未开启"
                }
            ));
        }
        Ok(authoritative)
    }
    #[cfg(not(desktop))]
    {
        let _ = enabled;
        Err("当前平台不支持开机自启".to_string())
    }
}

// ---------------------------------------------------------------------------
// API 网关（管理面）
// ---------------------------------------------------------------------------

/// GET /api/gateway/config —— 网关配置。
#[tauri::command]
pub fn get_gateway_config() -> Value {
    serde_json::to_value(GatewayConfig::load()).unwrap_or(Value::Null)
}

/// POST /api/gateway/config —— 保存配置并应用（启动/重启独立监听）。
#[tauri::command]
pub async fn save_gateway_config(app: tauri::AppHandle, config: Value) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::Gateway)?;
    let submitted = config.get("config").cloned().unwrap_or(config);
    let parsed: GatewayConfig =
        serde_json::from_value(submitted).map_err(|error| format!("配置格式错误: {error}"))?;
    parsed.save()?;

    let state = gateway::shared_state();
    *state.config.write().await = parsed.clone();
    state.log.set_keep(parsed.log_keep);
    state.log.set_log_bodies(parsed.log_bodies);

    let runtime = app.state::<gateway::GatewayRuntime>();
    let addr = runtime.apply().await?;
    Ok(json!({
        "ok": true,
        "config": parsed,
        "running": addr.is_some(),
        "addr": addr,
    }))
}

/// GET /api/proxy/status —— P12 C 路（base URL 重定向）的**只读状态**。
///
/// ⚠️ 只反映**本地标记**（用户是否按引导设过），**应用不校验代理是否真的生效**
/// （`proxy::status_view()` 里 `verified` 恒为 `false`）。C 路**不做中间人、不碰 TLS**。
#[tauri::command]
pub fn proxy_status() -> Value {
    proxy::status_view()
}

/// POST /api/proxy/enable —— 记下「用户已按引导设置」的本地标记。
///
/// ⚠️ **不改任何系统设置** —— T02 定的是**引导式**：应用只**显示**要设的 env，
/// 由**用户手动设置**。这是「绝不劫持」（D29）的延伸：**不碰 hosts / DNS / 用户环境**。
#[tauri::command]
pub fn proxy_enable() -> Result<Value, String> {
    proxy::set_enabled(true)?;
    Ok(proxy::status_view())
}

/// POST /api/proxy/disable —— **一键还原**：清标记（**删文件，不留残留**）。
#[tauri::command]
pub fn proxy_disable() -> Result<Value, String> {
    proxy::set_enabled(false)?;
    Ok(proxy::status_view())
}

/// GET /api/gateway/status —— 运行状态。
///
/// **E3.1**：统一复用 [`GatewayStatusView`]（**snake_case**，与 server 的
/// `/api/gateway/status` 契约一致），不再手工拼 camelCase。
#[tauri::command]
pub fn gateway_status(app: tauri::AppHandle) -> Value {
    let config = GatewayConfig::load();
    let runtime = app.state::<gateway::GatewayRuntime>();
    let mut view = GatewayStatusView::from(&config);
    view.running = runtime.is_running();
    view.addr = runtime.addr();
    view.version = update::APP_VERSION.to_string();
    // 账号池视图（与 server 侧 `/api/gateway/status` 逐字一致）。
    // 本命令是**同步**的，只能用 `try_write`：拿不到锁就只缺池字段，
    // 绝不能为此阻塞 UI 线程。
    let state = gateway::shared_state();
    if let Ok(mut pool) = state.pool.try_write() {
        // 先与账号库对齐（否则「刚导入、还没发过请求」的账号不在池里 ⇒ 页面显示空池）。
        buddy_switch_gateway::sync_pool_with_accounts(&mut pool);
        view.fill_pool(&pool, buddy_switch_gateway::timeutil::now_ms());
    }
    view.last_error = state
        .last_error
        .try_read()
        .ok()
        .and_then(|guard| guard.clone());

    // ★ 顺手触发一次余额刷新（**不阻塞本命令**）——与 server 侧同一理由：
    // 账号刚在本命令里同步进池、多半「从未取过余额」，而周期循环下一轮要等满一个间隔，
    // 用户会看到「刚导入账号，积分一直是『—』」。刷新器自带 `needs_refresh` 节流。
    //
    // ⚠️ 本命令是**同步**的，没有 tokio 上下文 ⇒ 必须用 tauri 的 async runtime spawn，
    // 直接 `tokio::spawn` 会 panic（no reactor running）。
    {
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let _ = buddy_switch_gateway::credits_refresh::refresh_once(&state).await;
        });
    }

    serde_json::to_value(view).unwrap_or(Value::Null)
}

/// POST /api/gateway/open-data-dir —— 打开 **WorkBuddy 客户端**的数据目录（`{region?}`）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`open_data_dir`]（带 `scope`）。本命令保留，
/// 是 [`open_data_dir`] 的 WorkBuddy 分支同源实现。
///
/// 与 Trae 侧的 `open_trae_data_dir` 同形：形状（`{ok,path}` 或结构化 `Unsupported`）
/// 由 `capability::open_workbuddy_data_dir` 唯一产出，前端用同一段分支渲染。
#[tauri::command]
pub async fn open_workbuddy_data_dir(region: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        capability::open_workbuddy_data_dir(parse_region(region.as_deref()))
    })
    .await
    .map_err(|error| format!("打开数据目录失败: {error}"))?
}

/// GET /api/gateway/keys —— Key 列表（脱敏）。
#[tauri::command]
pub fn list_api_keys() -> Value {
    let state = gateway::shared_state();
    let keys: Vec<Value> = state
        .keys
        .list()
        .iter()
        .map(|record| record.masked())
        .collect();
    json!({ "keys": keys })
}

/// ★ 后台探测某 region 的模型目录（「接入即识别」与「启动补识别」的**共用出口**）。
///
/// 复用 [`buddy_switch_gateway::catalog_probe::probe_region`]（全仓**唯一**探测入口）：
/// 它内部用**该 region 自己的凭据**（`strategy_for(region)` + `AccountSelector`，
/// 后者已按 region 校验）⇒ **不可能**用别的 region 的凭据写本 region 的清单
/// （安全边界 §3.17.5 / §2.5）。
///
/// 失败**静默**：探测只是「顺手把清单变新」，绝不该让接入 / 启动失败
/// （P10 §3.17.1 规则 3）。成功 / 失败都会写进 `CatalogStore`，由来源条如实展示
/// （失败时由 `CatalogStore::refresh` 写**具体原因**进 `note`）。
async fn probe_catalog(region: Region) {
    let state = gateway::shared_state();
    let _ = buddy_switch_gateway::catalog_probe::probe_region(&state, region).await;
}

/// POST /api/gateway/keys —— 创建 Key（返回一次性明文）。
///
/// ★ **接入即识别**（P10 §1.6 / ✅ #6）：创建 Key 即用户认知里的「接入」动作
/// （「我要让某工具连上这个网关」）⇒ 创建成功后**后台探测**该 region，
/// 用户无需手动点「重新识别」就能看到模型清单。
///
/// ⚠️ **fire-and-forget**：探测在**独立任务**里跑，本命令**立即返回**；
/// 探测失败**不得阻断接入**（§3.17.1 规则 3）。
/// ⚠️ 桌面端必须用 `tauri::async_runtime::spawn`（**不是** `tokio::spawn`）——
/// 见本文件既有的同款用法说明（同步命令 / 无 tokio 上下文时 `tokio::spawn` 会 panic）。
#[tauri::command]
pub async fn create_api_key(name: Option<String>, region: Option<String>) -> Value {
    let region = parse_region(region.as_deref());
    let name = name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "未命名".to_string());
    let (record, plaintext) = gateway::shared_state().keys.create(name, region);
    // 顺序不可颠倒：先落 Key、再触发探测 —— 探测失败也绝不能影响「Key 已创建」这个事实。
    tauri::async_runtime::spawn(async move {
        probe_catalog(region).await;
    });
    json!({ "ok": true, "key": plaintext, "record": record.masked() })
}

/// POST /api/gateway/keys/revoke —— 吊销 Key。
#[tauri::command]
pub fn revoke_api_key(id: String) -> Result<Value, String> {
    let state = gateway::shared_state();
    state.keys.revoke(&id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/gateway/keys/restore —— 撤销吊销（清 `revoked_at`，Key 重新可用）。
#[tauri::command]
pub fn restore_api_key(id: String) -> Result<Value, String> {
    let state = gateway::shared_state();
    state.keys.restore(&id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/gateway/keys/delete —— 删除已吊销 Key。
#[tauri::command]
pub fn delete_api_key(id: String) -> Result<Value, String> {
    let state = gateway::shared_state();
    state.keys.delete(&id)?;
    Ok(json!({ "ok": true }))
}

/// GET /api/gateway/pool/exclusions —— 该 region 的**排除账号**列表。
///
/// 「排除」= 用户不想让网关用这个号（硬约束：自动选号 / 兜底 / 粘性 / 指定全跳过）。
/// 与「禁用」的区别：禁用是这个号坏了（需人工恢复），排除是用户的主动选择。
///
/// `inStore=false` 表示该 uid 已不在账号库里（排除项**故意保留** ——
/// 用户意图针对「这个号」，重新导入同一 uid 仍应继续排除）。
#[tauri::command]
pub fn gateway_pool_exclusions(region: Option<String>) -> Value {
    buddy_switch_gateway::routes::pool_admin::view(parse_region(region.as_deref()))
}

/// POST /api/gateway/pool/exclusions —— 设置 / 取消排除（幂等）。
///
/// 落盘 + 刷新池内快照 + **按 uid 解绑粘性** 三件事收在
/// `pool_admin::set` 里（唯一实现，server 侧共用同一份）。
#[tauri::command]
pub async fn set_gateway_pool_exclusion(
    region: Option<String>,
    uid: String,
    excluded: bool,
    note: Option<String>,
) -> Result<Value, String> {
    let state = gateway::shared_state();
    buddy_switch_gateway::routes::pool_admin::set(
        &state,
        parse_region(region.as_deref()),
        uid.trim(),
        excluded,
        note.as_deref().unwrap_or(""),
    )
    .await
}

/// POST /api/trae/gateway/pool/exclusions —— 设置 / 取消 Trae 账号的排除。
#[tauri::command]
pub async fn set_trae_gateway_pool_exclusion(
    variant: Option<String>,
    uid: String,
    excluded: bool,
    note: Option<String>,
) -> Result<Value, String> {
    let scope = format!("trae:{}", parse_trae_variant(variant.as_deref()).as_str());
    let uid = uid.trim().to_string();
    let value = buddy_switch_gateway::routes::pool_admin::persist(
        &scope,
        &uid,
        excluded,
        note.as_deref().unwrap_or(""),
    )?;
    // 排除 ⇒ 立刻解绑指向它的会话粘性（否则每请求白跑一跳、徽标也自相矛盾）。
    if excluded {
        trae_gateway::shared_state()
            .sticky
            .lock()
            .await
            .purge_uid(&uid);
    }
    Ok(value)
}

/// POST /api/qoder/gateway/pool/exclusions —— 设置 / 取消 Qoder 账号的排除。
#[tauri::command]
pub async fn set_qoder_gateway_pool_exclusion(
    scope: String,
    uid: String,
    excluded: bool,
    note: Option<String>,
) -> Result<Value, String> {
    let region = require_qoder_region(resolve_scope_param(Some(&scope))?)?;
    let scope_key = format!("qoder:{}", region.as_str());
    let uid = uid.trim().to_string();
    let value = buddy_switch_gateway::routes::pool_admin::persist(
        &scope_key,
        &uid,
        excluded,
        note.as_deref().unwrap_or(""),
    )?;
    if excluded {
        qoder_gateway::shared_state()
            .sticky
            .lock()
            .await
            .purge_uid(&uid);
    }
    Ok(value)
}

/// POST /api/gateway/pool/revive —— **人工解除**账号池里某账号的禁用。
///
/// 与「API Key 的恢复」同构：`disabled` 是**终态且落盘持久化**，而改造前它
/// **没有任何出口** —— 余额刷新被 `if entry.disabled` 挡住，`PoolEntry::revive`
/// 在生产代码里零调用点（详见 `buddy_switch_gateway::pool::Pool::revive` 的文档）。
///
/// 幂等：账号本来可用时返回 `{ok:true,revived:false}` 而**不报错**；
/// 「不在池中」才是真错误（uid 写错 / 账号已删）。
///
/// ⚠️ 本命令是 **async**（要 `.write().await`）—— 与 `gateway_status` 那种同步命令
/// 不同，**不能**在这里用 `try_write`（拿不到锁就静默什么都没做，而用户以为点成功了）。
#[tauri::command]
pub async fn revive_gateway_account(uid: String) -> Result<Value, String> {
    let uid = uid.trim().to_string();
    if uid.is_empty() {
        return Err("缺少 uid".to_string());
    }
    let state = gateway::shared_state();
    let mut pool = state.pool.write().await;
    // 先与账号库对齐：否则「刚导入、还没发过请求」的账号在池里根本不存在，
    // 而界面刚刚明明显示过它 —— 用户会拿到一句莫名其妙的「不在池中」。
    buddy_switch_gateway::sync_pool_with_accounts(&mut pool);
    if pool.get(&uid).is_none() {
        return Err(format!("账号 {uid} 不在账号池中（可能已被删除）"));
    }
    let revived = pool.revive(&uid);
    Ok(json!({ "ok": true, "revived": revived }))
}

/// GET /api/gateway/models —— 模型列表 + 来源（按 region）。
#[tauri::command]
pub fn get_gateway_models(region: Option<String>) -> Value {
    let region = parse_region(region.as_deref());
    let state = gateway::shared_state();
    let snapshot = state.catalogs.current(region);
    // 同步命令：拿不到配置读锁时按「上限」处理（与其它同步读取口同口径）。
    let max_context = state
        .config
        .try_read()
        .map(|config| config.max_context)
        .unwrap_or(true);
    buddy_switch_gateway::routes::models::snapshot_view(&snapshot, max_context)
}

/// POST /api/gateway/models/refresh —— 手动刷新目录（按 region）。
#[tauri::command]
pub async fn refresh_gateway_models(region: Option<String>) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let state = gateway::shared_state();
    let strategy = state.strategy_for(region).await;
    let account = buddy_switch_gateway::AccountSelector
        .select(region, &strategy)
        .await
        .map_err(|error| error.message())?;
    let snapshot = state
        .catalogs
        .refresh(region, &state.upstream, &account)
        .await;
    Ok(serde_json::to_value(snapshot).unwrap_or(Value::Null))
}

/// GET /api/gateway/strategy —— 各 region 账号策略。
#[tauri::command]
pub async fn get_account_strategy() -> Value {
    let state = gateway::shared_state();
    // 先取快照，避免跨 await 持有读锁。
    let strategies = state.strategies.read().await.clone();
    let cn = strategies.get(&Region::Cn).cloned().unwrap_or_default();
    let global = strategies.get(&Region::Global).cloned().unwrap_or_default();
    json!({
        "cn": buddy_switch_gateway::account_strategy::describe_strategy(Region::Cn, &cn).await,
        "global": buddy_switch_gateway::account_strategy::describe_strategy(Region::Global, &global).await,
    })
}

/// POST /api/gateway/strategy —— 保存某 region 账号策略。
#[tauri::command]
pub async fn save_account_strategy(
    region: Option<String>,
    strategy: Value,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let parsed: AccountStrategy =
        serde_json::from_value(strategy).map_err(|error| format!("策略格式错误: {error}"))?;
    let state = gateway::shared_state();
    {
        let mut strategies = state.strategies.write().await;
        strategies.insert(region, parsed);
        buddy_switch_gateway::account_strategy::save_strategies(&strategies)?;
    }
    Ok(json!({ "ok": true }))
}

/// GET /api/gateway/disabled-models —— 某**作用域**被用户禁用的模型 id 列表（P11）。
///
/// ## 为什么这条命令**不区分** WB / Trae / Qoder 网关
///
/// 禁用存储是**进程内唯一**实例（[`buddy_switch_gateway::disabled_models::shared`]），
/// 三个网关（各自的 `/v1/models` + `/v1/chat/completions`）与命令面读的是**同一个 `Arc`**。
/// 因此这里读到的就是各出口此刻真正在用的那份 —— 不是「命令写一份、网关读另一份」的假开关。
///
/// ⚠️ 键是**作用域**（`wb-cn` / `qoder-global` …），不是裸 region —— 三个平台互不相通的
/// 模型目录各自分家（2026-10-15 升级，含旧键迁移）。`scope` **必填且 fail-closed**。
///
/// 形状 `{ "scope": …, "ids": [ … ] }`：`ids` 是该作用域被禁用的模型 id 集合
/// （`BTreeSet` ⇒ 稳定有序，前端可直接比对）。
#[tauri::command]
pub async fn get_disabled_models(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let store = buddy_switch_gateway::disabled_models::shared();
    let disabled = store.read().await;
    let ids: Vec<String> = disabled.get(scope).iter().cloned().collect();
    Ok(json!({ "scope": scope.as_str(), "ids": ids }))
}

/// POST /api/gateway/disabled-models —— 置位 / 清位某**作用域**的一个模型（P11）。
///
/// ## 为什么返回 `changed`
///
/// `DisabledModels::set` 是**幂等**的：重复禁用 / 重复启用不改动集合，也就**不必落盘**
/// （省一次磁盘写）。返回 `changed` 让前端能区分「真的改了」与「本来就如此」。
/// 空白 `model_id` 被 `set` 拒绝 ⇒ `changed: false`（不写脏数据）。
#[tauri::command]
pub async fn set_model_disabled(
    scope: String,
    model_id: String,
    disabled: bool,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let store = buddy_switch_gateway::disabled_models::shared();
    let changed = {
        let mut guard = store.write().await;
        let changed = guard.set(scope, &model_id, disabled);
        // 只在**真的变了**时落盘；写失败必须回传（否则用户以为禁用了、重启后又出现）。
        if changed {
            guard.save()?;
        }
        changed
    };
    Ok(json!({
        "ok": true,
        "scope": scope.as_str(),
        "model_id": model_id.trim(),
        "disabled": disabled,
        "changed": changed,
    }))
}

/// GET /api/gateway/logs —— 最近 N 条请求日志（元数据）。
#[tauri::command]
pub fn get_gateway_logs() -> Value {
    let state = gateway::shared_state();
    json!({ "logs": state.log.list() })
}

/// POST /api/gateway/logs/clear —— 清空日志。
#[tauri::command]
pub fn clear_gateway_logs() -> Value {
    let state = gateway::shared_state();
    state.log.clear();
    json!({ "ok": true })
}

/// POST /api/gateway/probe/start —— 启动一轮「账号 × 模型」限流探测（异步作业）。
///
/// 与 `crates/buddy-switch-server/src/api.rs::api_probe_start` **一一对应**（刻意不去重）。
/// `start` 只**建作业**（原子抢占作业位），逐格执行由
/// [`buddy_switch_gateway::model_probe::run`] 在后台任务里完成。
///
/// ★ **拿到 `Ok` 后必须 spawn `run`**：作业位是 `compare_exchange` 原子抢占的，
/// 不 spawn 会让它一直占着 —— 之后所有 `start` 都报「已有探测在进行中」。
/// 桌面端无 tokio 上下文，故用 `tauri::async_runtime::spawn`（服务端用 `tokio::spawn`）。
///
/// ★ **作用域参数化**：入参从 `region` 升级为 `scope`（`wb-cn` / `qoder-cn` …）。
/// `target_for_scope` 是 **fail-closed** 解析（未知 / 拼错 ⇒ 可读错误），并据作用域挑出
/// 对应平台的目标（WB / Qoder；Trae 本轮不做）。
///
/// `uids` / `models` **缺省 = 用池里的全部账号 / 该作用域目录**（与 `start` 的 `Option` 语义一致）。
#[tauri::command]
pub async fn probe_start(
    scope: String,
    uids: Option<Vec<String>>,
    models: Option<Vec<String>>,
    adopt: bool,
) -> Result<Value, String> {
    let target = buddy_switch_gateway::model_probe::target_for_scope(&scope)?;
    let total = buddy_switch_gateway::model_probe::start(&target, uids, models, adopt).await?;
    tauri::async_runtime::spawn(buddy_switch_gateway::model_probe::run(target));
    Ok(json!({ "started": true, "total": total }))
}

/// GET /api/gateway/probe/status —— 探测作业快照（从未跑过时 `{ "phase": "idle" }`）。
///
/// `scope` 为**必填**（fail-closed）：作业槽是进程级全局，快照本身与作用域无关，但命令面
/// 统一带 `scope`，未知 / 拼错的作用域一律拒绝（不静默接受）。
#[tauri::command]
pub fn probe_status(scope: String) -> Result<Value, String> {
    resolve_scope_param(Some(&scope))?;
    Ok(buddy_switch_gateway::model_probe::status())
}

/// POST /api/gateway/probe/cancel —— 请求取消（逐格检查；已探到的格保留）。
#[tauri::command]
pub fn probe_cancel(scope: String) -> Result<Value, String> {
    resolve_scope_param(Some(&scope))?;
    Ok(buddy_switch_gateway::model_probe::cancel())
}

// ===========================================================================
// Trae 模块命令
// ===========================================================================
//
// 与 `buddy-switch-server::api::api.rs` 的 `/api/trae/*` 路由**一一对应**，
// 返回形状由 `buddy_switch_core::modules::trae::handlers` 单点保证，
// 本层只负责「解析参数 → 调 handlers → 返回 Value」。
//
// 文件 IO / 子进程类操作一律走 `spawn_blocking`：Tauri 的异步命令运行在共享
// 运行时上，在其中做同步磁盘遍历（快照目录递归统计、9 类文件复制）会阻塞
// 同一个运行时上的其他命令与事件派发。`core` 侧的 `get_status` 已有同样的处理。

/// POST /api/trae/legacy-merge —— 旧产品线账号库并入国内版区域账号库。
///
/// **幂等**：无旧库或已并完时返回 `changed: false`。前端在账号页加载时调一次即可。
///
/// async + spawn_blocking：读写 JSON 账号库并做一次目录扫描，属文件 IO，
/// 不宜占用 Tauri 主线程（同本文件头部「文件 IO 一律走 spawn_blocking」的约定）。
#[tauri::command]
pub async fn trae_merge_legacy_regions() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(trae::handlers::merge_legacy_regions)
        .await
        .map_err(|error| format!("合并旧产品线账号库失败: {error}"))?
}

/// GET /api/trae/env —— Trae 客户端安装/运行/数据目录状态。
///
/// **单一视角**：返回自动探测挑中的那一条产品线（`variant` / `variantLabel`）。
/// 界面要**并排**显示两条产品线时用 [`get_trae_variants`]。
/// `async` + `spawn_blocking`：本命令要跑 `tasklist` 进程探测并扫两遍安装候选目录，
/// 实测 0.1–0.25 s。同步执行会在账号页 / 设置页挂载时卡主线程（issue #48）。
#[tauri::command]
pub async fn get_trae_env() -> Value {
    match tauri::async_runtime::spawn_blocking(trae::platform::env_status).await {
        Ok(value) => value,
        Err(error) => json!({ "error": format!("读取 Trae 环境失败: {error}") }),
    }
}

/// GET /api/trae/variants —— **全部** Trae 产品线的独立环境状态（数组）。
///
/// 与 [`get_trae_env`] 的分工：`env` 回答"自动挑中的是哪一条"（用于"当前在操作哪条线"
/// 的页面标题、诊断文案），本命令回答"每条各自是什么状态"（用于并排渲染多个图标，
/// 对齐 WorkBuddy 右上角三个独立产品图标）。
///
/// 返回 `{ platform, variants: [...] }`，每个元素自带
/// `variant` / `variantLabel` / `installed` / `running` / `version` / `path` /
/// `dataDir` / `dataDirExists`，前端直接遍历即可。
///
/// `async` + `spawn_blocking`：`variants_status` 内部要做**进程探测**（Windows 起
/// `tasklist` 子进程），必须离开 Tauri 主线程。
#[tauri::command]
pub async fn get_trae_variants() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(trae::platform::variants_status)
        .await
        .map_err(|error| format!("探测 Trae 产品线状态失败: {error}"))
}

/// GET /api/trae/capabilities —— 当前平台的能力与受限项说明。
#[tauri::command]
pub fn get_trae_capabilities() -> Value {
    trae::platform::capabilities()
}

/// GET /api/trae/accounts —— 账号 + 分组 + 计数。
///
/// `variant` 可选（`"trae_work"` / `"trae_cn"`）：决定读哪个账号库。缺失回落默认变体，
/// 保证老调用点行为不变。
///
/// `async` + `spawn_blocking`：要读账号库 JSON 与快照目录（**慢 IO**）。
#[tauri::command]
pub async fn get_trae_accounts(variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || trae::handlers::accounts_overview_for(variant))
        .await
        .map_err(|error| format!("读取 Trae 账号总览失败: {error}"))
}

/// GET /api/trae/checkin/status —— 最近一次签到摘要与冷却明细。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`checkin_status`]（带 `scope`）。本命令保留，
/// 是 [`checkin_status`] 的 Trae 分支同源实现（`variant` 先按**区域**折叠成 `scope`）。
#[tauri::command]
pub fn get_trae_checkin_status(variant: Option<String>) -> Value {
    trae::handlers::checkin_status_for(parse_trae_variant(variant.as_deref()))
}

/// GET /api/trae/checkin/status-map —— 逐账号「今日签到状态」（**只读**；issue #47）。
///
/// `scope` 决定区域（`trae-cn` / `trae-global`，fail-closed）；`accountIds` 是逗号分隔的
/// uid 列表（见 [`split_id_list`]）。未知 uid 不进 map；读失败回 `{ok:false, error}`。
/// **绝不**在这里触发签到。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_trae_checkin_status_map(
    scope: String,
    account_ids: Option<String>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let variant = trae_variant_of_region(require_trae_region(scope)?);
    Ok(trae::checkin::status_map_for(variant, split_id_list(account_ids)).await)
}

/// GET /api/trae/account/token-today —— 逐账号「今日 Token」（**只读**；issue #47）。
///
/// 形状与 WB 的 `get_account_token_today` 同构，但 Trae 没有客户端会话落盘 ⇒
/// 响应带 `clientSource: "unavailable"`（客户端那一档不可得，**不是 0**）。
/// 读网关日志是阻塞 IO ⇒ `spawn_blocking`。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_trae_account_token_today(
    scope: String,
    force: Option<bool>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let variant = trae_variant_of_region(require_trae_region(scope)?);
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        trae::token_stats::account_token_today(variant, force)
    })
    .await
    .map_err(|error| format!("读取今日 Token 失败: {error}"))
}

/// GET /api/trae/credits —— 剩余积分、明细、每日趋势。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`credits`]（带 `scope`）。本命令保留，
/// 是 [`credits`] 的 Trae 分支同源实现（`variant` 先按**区域**折叠成 `scope`）。
#[tauri::command]
pub fn get_trae_credits(variant: Option<String>) -> Value {
    trae::handlers::credits_overview_for(parse_trae_variant(variant.as_deref()))
}

/// GET /api/trae/token-stats —— Token 统计（聚合本机网关请求日志）。
///
/// `days` 为统计窗口天数；`None` / `<= 0` 表示全部历史。
/// `scope` 为**变体范围**筛选维度：`work` / `cn` / `unlabeled` / `all`（缺省 `all`）。
///
/// `async` + `spawn_blocking`：本命令**全量**读网关日志并解析两遍（零缓存），
/// 被概览页 / Token 统计页 / 积分页三处重复调用。同步执行时切页会卡主线程（issue #48）。
#[tauri::command]
pub async fn get_trae_token_statistics(days: Option<i64>, scope: Option<String>) -> Value {
    match tauri::async_runtime::spawn_blocking(move || {
        let scope = scope
            .as_deref()
            .map(trae::token_stats::TraeTokenScope::parse)
            .unwrap_or_default();
        trae::handlers::token_statistics(days, scope)
    })
    .await
    {
        Ok(value) => value,
        Err(error) => json!({ "error": format!("统计 Token 用量失败: {error}") }),
    }
}

/// GET /api/trae/logs —— 运行日志（系统日志页的「运行日志」标签页）。
///
/// `kind` 取 `app` / `checkin` / `switch`（缺省或 `all` 表示不限）；
/// `date` 为 `YYYY-MM-DD`；`keyword` 大小写不敏感；`limit` 缺省 500、上限 2000；
/// `variant` 决定读哪条产品线的 `checkin` / `switcher` 日志（`app.log` 共用）。
///
/// 入参在这里组装成 JSON 再交给 [`trae::handlers::logs`]：过滤逻辑只实现一份，
/// 两条通道共用（历史上两边各写一遍导致过响应形状漂移）。
///
/// `async` + `spawn_blocking`：本命令**全量读日志文件**（最慢的一个，`limit` 上限 2000 行），
/// 同步执行会明显卡住窗口。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_trae_logs(
    kind: Option<String>,
    date: Option<String>,
    keyword: Option<String>,
    limit: Option<u64>,
    variant: Option<String>,
) -> Result<Value, String> {
    let args = serde_json::json!({
        "kind": kind,
        "date": date,
        "keyword": keyword,
        "limit": limit,
        "variant": variant,
    });
    tauri::async_runtime::spawn_blocking(move || trae::handlers::logs(&args))
        .await
        .map_err(|error| format!("读取运行日志失败: {error}"))
}

/// GET /api/trae/profiles —— 登录态快照总览。
#[tauri::command]
pub async fn get_trae_profiles(variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || trae::profile::overview_for(variant))
        .await
        .map_err(|error| format!("读取快照失败: {error}"))
}

/// GET /api/trae/settings —— Trae 模块设置。
#[tauri::command]
pub fn get_trae_settings() -> Value {
    serde_json::to_value(trae::settings::load()).unwrap_or(Value::Null)
}

/// POST /api/trae/settings —— 局部更新 Trae 模块设置。
#[tauri::command]
pub fn save_trae_settings(patch: Value) -> Result<Value, String> {
    trae::handlers::save_settings(patch)
}

/// POST /api/trae/accounts/add —— 手动添加账号（粘贴 JWT）。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_add_account(
    name: String,
    jwt: String,
    group_id: Option<String>,
    variant: Option<String>,
) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    trae::handlers::add_account_for(variant, &name, &jwt, group_id.as_deref())
}

/// POST /api/trae/accounts/update —— 改名 / 换 JWT。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_update_account(
    user_id: String,
    name: Option<String>,
    jwt: Option<String>,
    variant: Option<String>,
) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    trae::handlers::update_account_for(variant, &user_id, name.as_deref(), jwt.as_deref())
}

/// POST /api/trae/accounts/remark —— 设置账号备注（字段级更新）。
///
/// `remark` 是**字符串**而不是 `Option`：空串（或全空白）就是「清空备注」，
/// 后端据此删掉账号库里的 `remark` 键。语义细节见 `account::set_remark_for`。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`account_remark`]（带 `scope`）。本命令保留，
/// 是 [`account_remark`] 的 Trae 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_set_account_remark(
    user_id: String,
    remark: String,
    variant: Option<String>,
) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    trae::handlers::set_remark_for(variant, &user_id, Some(remark.as_str()))
}

/// POST /api/trae/accounts/delete —— 删除账号（可选一并删除登录态快照）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`account_delete`]（带 `scope`）。本命令保留，
/// 是 [`account_delete`] 的 Trae 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_delete_account(
    user_id: String,
    delete_profile: Option<bool>,
    variant: Option<String>,
) -> Result<Value, String> {
    let delete_profile = delete_profile.unwrap_or(false);
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        trae::handlers::delete_account_for(variant, &user_id, delete_profile)
    })
    .await
    .map_err(|error| format!("删除账号失败: {error}"))?
}

/// POST /api/trae/accounts/import-local —— 从 Trae 客户端登录态导入当前账号。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_import_local`]（带 `scope`）。本命令
/// 保留，是 [`accounts_import_local`] 的 Trae 分支同源实现（`variant` 先按**区域**折叠成 `scope`）。
///
/// `variant` 为可选产品线（`"trae_work"` / `"trae_cn"`，也接受 `TRAE SOLO CN`
/// 之类的目录名）：决定读哪条产品线的 userData。**缺失时回落默认变体**，
/// 保证老调用点行为不变。命令名不变，仅新增参数。
#[tauri::command]
pub async fn trae_import_local_account(variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || trae::handlers::import_local_account_for(variant))
        .await
        .map_err(|error| format!("导入本机账号失败: {error}"))?
}

/// POST /api/trae/oauth/start —— 发起 Trae OAuth 登录（开本地回调监听）。
///
/// 与 WorkBuddy 侧 `oauth_start` 的区别：本命令**不带 region**，带的是 `variant`。
/// Trae 两条产品线共用一套上游，但账号库、设备身份与会话归属都按变体分家
/// （见 `modules::trae` 的模块头注释），所以 `variant` 不是假参数。
///
/// `variant` 可选：缺失 / 无法识别回落默认变体，保证老调用点行为不变。
/// 命令名不变，仅新增参数。
#[tauri::command]
pub async fn trae_oauth_start(variant: Option<String>) -> Result<Value, String> {
    require_capability(Product::Trae, capability::Capability::Oauth)?;
    trae::handlers::oauth_login_start_for(parse_trae_variant(variant.as_deref())).await
}

/// POST /api/trae/oauth/status —— 轮询登录结果。
///
/// 与 WorkBuddy 侧 `oauth_status` 的区别：本命令**不带 region**。
/// 变体也不需要传：会话自己记着它（`loginId` 是唯一入口），
/// 后端据此从正确的账号库取列表。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_oauth_status(login_id: String) -> Value {
    trae::handlers::oauth_login_status(&login_id)
}

/// POST /api/trae/oauth/cancel —— 取消登录，释放监听端口。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_oauth_cancel(login_id: String) -> Value {
    trae::handlers::oauth_login_cancel(&login_id)
}

/// POST /api/trae/accounts/export —— 按 userId 列表导出完整记录（含 JWT）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_export`]（带 `scope`）。本命令保留，
/// 是 [`accounts_export`] 的 Trae 分支同源实现（`variant` 决定读哪个区域库）。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_export_accounts(
    user_ids: Vec<String>,
    variant: Option<String>,
) -> Result<Value, String> {
    trae::handlers::export_accounts_for(parse_trae_variant(variant.as_deref()), &user_ids)
}

/// POST /api/trae/accounts/export-to-path —— 写入用户选择的路径，返回落地路径。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_export_to_path`]（带 `scope`）。本命令保留，
/// 是 [`accounts_export_to_path`] 的 Trae 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_export_accounts_to_path(
    user_ids: Vec<String>,
    path: String,
    variant: Option<String>,
) -> Result<Value, String> {
    trae::handlers::export_accounts_to_path_for(
        parse_trae_variant(variant.as_deref()),
        &user_ids,
        &path,
    )
}

/// POST /api/trae/accounts/import/preview —— 解析导入文件并回传脱敏预览。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_preview_import_accounts(file_text: String) -> Result<Value, String> {
    trae::handlers::preview_import_file(&file_text)
}

/// POST /api/trae/accounts/import —— 按选中索引导入，返回计数与最新账号视图。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`accounts_import`]（带 `scope`）。本命令保留，
/// 是 [`accounts_import`] 的 Trae 分支同源实现。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_import_accounts(
    file_text: String,
    indexes: Vec<usize>,
    variant: Option<String>,
) -> Result<Value, String> {
    trae::handlers::import_accounts_for(
        parse_trae_variant(variant.as_deref()),
        &file_text,
        &indexes,
    )
}

/// POST /api/trae/groups —— 分组操作分发（create / update / delete / move）。
#[tauri::command]
pub fn trae_group_op(
    action: String,
    params: Value,
    variant: Option<String>,
) -> Result<Value, String> {
    trae::handlers::group_op_for(parse_trae_variant(variant.as_deref()), &action, &params)
}

/// POST /api/trae/checkin —— 批量签到（进度经 `trae-checkin-progress` 事件推送）。
///
/// 变体从 `options.variant` 解析（同一份入参契约，见
/// [`trae::handlers::parse_checkin_options`]），因此本命令无需单独的 `variant` 参数。
#[tauri::command]
pub async fn trae_checkin(app: tauri::AppHandle, options: Option<Value>) -> Result<Value, String> {
    require_capability(Product::Trae, capability::Capability::Checkin)?;
    let options = options.unwrap_or_else(|| json!({}));
    let parsed = trae::handlers::parse_checkin_options(&options)?;
    let report = trae::checkin::run_checkin(parsed, move |event| {
        // 事件名带 `trae-` 前缀：与 WorkBuddy 侧的 `checkin-progress` 区分，
        // 否则 Trae 的进度会被 WorkBuddy 页面消费掉。
        let _ = app.emit("trae-checkin-progress", event.clone());
        // P15 · **同一份**进度也喂给悬浮窗 —— 两处消费同一个事件，
        // 不可能出现「页面显示 3/8、窗口显示别的」这种不一致。
        observe_trae_checkin(event);
    })
    .await;
    Ok(trae::checkin::report_json(&report))
}

/// POST /api/trae/credits/refresh —— 刷新剩余积分（单个或全部）。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_refresh_credits(
    user_id: Option<String>,
    variant: Option<String>,
) -> Result<Value, String> {
    require_capability(Product::Trae, capability::Capability::Credits)?;
    let variant = parse_trae_variant(variant.as_deref());
    trae::handlers::refresh_credits_for(variant, user_id.as_deref()).await
}

/// POST /api/trae/refresh-jwt —— 用 refresh_token 换新 JWT。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_refresh_jwt(user_id: String, variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    trae::handlers::refresh_jwt_for(variant, &user_id).await
}

/// POST /api/trae/cooldown/clear —— 清除冷却（单个或全部）。
#[tauri::command(rename_all = "camelCase")]
pub fn trae_clear_cooldown(
    user_id: Option<String>,
    variant: Option<String>,
) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    trae::handlers::clear_cooldown_for(variant, user_id.as_deref())
}

/// 进度事件名（前端订阅用，集中定义避免拼写漂移）。
const TRAE_SWITCH_PROGRESS_EVENT: &str = "trae-switch-progress";

/// POST /api/trae/switch —— 切换账号（含「先保存当前」的兜底）。
///
/// 用 `spawn_blocking`：整个过程串行做 9 类文件的递归复制 + 进程终止/启动，
/// 放在异步运行时上会阻塞事件循环，导致进度事件无法及时送达前端。
#[tauri::command]
pub async fn trae_switch_account(app: tauri::AppHandle, options: Value) -> Result<Value, String> {
    let options = trae::handlers::parse_switch_options(&options)?;
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = trae::profile::switch_account(&options, |step| {
            let _ = app.emit(TRAE_SWITCH_PROGRESS_EVENT, step.to_json());
        });
        outcome.to_json()
    })
    .await
    .map_err(|error| format!("切换账号失败: {error}"))
}

/// POST /api/trae/login/save —— 保存当前登录态到指定账号槽位。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_save_login(user_id: String, variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || trae::handlers::save_login_for(variant, &user_id))
        .await
        .map_err(|error| format!("保存登录态失败: {error}"))?
}

/// POST /api/trae/profiles/backup —— 备份当前登录态到槽位。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_backup_profile(
    user_id: String,
    variant: Option<String>,
) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        trae::handlers::backup_profile_for(variant, &user_id)
    })
    .await
    .map_err(|error| format!("备份失败: {error}"))?
}

/// POST /api/trae/profiles/restore —— 用槽位快照覆盖客户端登录态（高级操作）。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_restore_profile(
    user_id: String,
    variant: Option<String>,
) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        trae::handlers::restore_profile_for(variant, &user_id)
    })
    .await
    .map_err(|error| format!("恢复失败: {error}"))?
}

/// POST /api/trae/profiles/delete —— 删除登录态快照。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_delete_profile(slot: String, variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || trae::handlers::delete_profile_for(variant, &slot))
        .await
        .map_err(|error| format!("删除快照失败: {error}"))?
}

/// POST /api/trae/device/reset —— 重置 6 层设备标识。
#[tauri::command(rename_all = "camelCase")]
pub async fn trae_reset_device(variant: Option<String>) -> Result<Value, String> {
    let variant = parse_trae_variant(variant.as_deref());
    tauri::async_runtime::spawn_blocking(move || trae::handlers::reset_device_for(variant))
        .await
        .map_err(|error| format!("重置设备标识失败: {error}"))?
}

// ---------------------------------------------------------------------------
// Trae API 网关（管理面）
// ---------------------------------------------------------------------------
//
// 与 WorkBuddy 网关的管理命令**形状对齐**（get config / save config / status /
// models / keys / logs / clear logs）。多 Key 管理（含归属产品线）走
// `trae_gateway::shared_state().key_store`：Key 只存哈希 + 前缀 + `variant`，
// 明文仅在创建时一次性返回；形状由 gateway crate 的 `trae::apikey::{list_response,
// create_response}` 唯一产出（`MEMORY.md §二`：两条通道不得各自拼装）。

/// GET /api/trae/gateway/config —— Trae 网关配置。
#[tauri::command]
pub fn get_trae_gateway_config() -> Value {
    serde_json::to_value(buddy_switch_gateway::trae::TraeGatewayConfig::load())
        .unwrap_or(Value::Null)
}

/// POST /api/trae/gateway/config —— 保存配置并应用（启动/重启独立监听）。
///
/// 注意 `max_body_mb` 是**构造期**固化进 axum `DefaultBodyLimit` 的（与 WorkBuddy
/// 网关同理），改它要等下一次 `apply()`——而 `apply()` 每次都会重建监听，所以只要
/// 走本命令保存就一定生效。
#[tauri::command]
pub async fn save_trae_gateway_config(
    app: tauri::AppHandle,
    config: Value,
) -> Result<Value, String> {
    require_capability(Product::Trae, capability::Capability::Gateway)?;
    let submitted = config.get("config").cloned().unwrap_or(config);
    let parsed: buddy_switch_gateway::trae::TraeGatewayConfig =
        serde_json::from_value(submitted).map_err(|error| format!("配置格式错误: {error}"))?;
    parsed.save()?;

    let state = trae_gateway::shared_state();
    *state.config.write().await = parsed.clone();
    state.log.set_keep(parsed.log_keep);
    state.log.set_log_bodies(parsed.log_bodies);

    let runtime = app.state::<trae_gateway::TraeGatewayRuntime>();
    let addr = runtime.apply().await?;
    Ok(json!({
        "ok": true,
        "config": parsed,
        "running": addr.is_some(),
        "addr": addr,
    }))
}

/// GET /api/trae/gateway/status —— 运行状态 + 账号池摘要 + 账号明细 + 诊断。
///
/// `variant` 决定看哪个账号池：缺失 / 未知 → 默认变体（TraeWork），响应键集合不变。
#[tauri::command]
pub async fn trae_gateway_status(app: tauri::AppHandle, variant: Option<String>) -> Value {
    let (running, addr) = {
        let runtime = app.state::<trae_gateway::TraeGatewayRuntime>();
        (runtime.is_running(), runtime.addr())
    };
    buddy_switch_gateway::trae::status_view(
        &trae_gateway::shared_state(),
        running,
        addr,
        update::APP_VERSION,
        parse_trae_variant(variant.as_deref()),
    )
    .await
}

/// GET /api/trae/gateway/models —— 对外暴露的模型清单（**随上游刷新**）。
///
/// 与 [`get_trae_client_models`] **同源**（都读客户端 `state.vscdb` 里的上游清单），
/// 区别只在形状：这里是 OpenAI `/v1/models` 形状的**扁平去重**清单 ——
/// 也就是外部客户端连本网关时列出的那份；那边是带元数据的分组视图。
///
/// `variant` 决定读哪条产品线；缺失 / 未知 → 默认变体（TraeWork）。
/// 客户端清单读不到时**回落静态兜底清单**，保证「列模型」不会失败。
///
/// `async` + `spawn_blocking`：读客户端清单要探测候选目录并**逐个开 SQLite**
/// （`busy_timeout` 1 s），与 [`get_trae_client_models`] 被 `Promise.all` 同批打，
/// 2 区域 ⇒ 最多 4 次同步开库。同步执行会阻塞主线程（issue #48）。
#[tauri::command]
pub async fn get_trae_gateway_models(variant: Option<String>) -> Value {
    match tauri::async_runtime::spawn_blocking(move || build_trae_gateway_models(variant)).await {
        Ok(value) => value,
        Err(error) => json!({ "error": format!("读取模型清单失败: {error}") }),
    }
}

fn build_trae_gateway_models(variant: Option<String>) -> Value {
    let variant = parse_trae_variant(variant.as_deref());
    // ★ 出口 ①（P11）：与 Trae 网关本体（`trae/routes.rs`）读**同一份** `disabled`，
    //   按 `region_of_variant` 键控 —— 否则「管理面看到的清单」与「网关实际暴露的清单」会不一致。
    //
    //   本命令是**同步**的（读客户端 SQLite 属磁盘 IO，见本文件头部约定），故用 `try_read`
    //   （与 `gateway_status` 读 `last_error` 同款）。拿不到读锁时回落**空集**（= 默认全开，D27）：
    //   这只影响管理面的瞬时显示，网关真正的出口 ①（`/v1/models`）走 async `.read().await`，不受此影响。
    let disabled = buddy_switch_gateway::disabled_models::shared()
        .try_read()
        .map(|guard| {
            guard
                .get(buddy_switch_core::modules::product::scope_of_trae_region(
                    variant.region(),
                ))
                .clone()
        })
        .unwrap_or_default();
    buddy_switch_gateway::trae::payload::models_response_for(
        variant,
        &disabled,
        buddy_switch_gateway::trae::max_mode_now(),
    )
}

/// GET /api/trae/gateway/client-models —— 客户端（**上游下发**）的模型清单。
///
/// 与 [`get_trae_gateway_models`] **同源**（同一份客户端缓存），但形状不同：
/// 这里是**分组 + 元数据**（展示名 / 上下文窗口 / 是否默认 / 是否 Beta …），
/// 供「API 服务」页的模型卡渲染；那边是扁平去重后的 OpenAI 形状。
///
/// `variant` 决定读哪条产品线的客户端；缺失 / 未知 → 默认变体（TraeWork）。
/// 读不到（客户端没启动过 / 没登录 / 还没拉过清单）**不是错误**：返回
/// `source = "missing"` 与可读的 `note`，由界面呈现空态。
///
/// `async` + `spawn_blocking`：同 [`get_trae_gateway_models`]（开 SQLite + 解析大 JSON）。
#[tauri::command]
pub async fn get_trae_client_models(variant: Option<String>) -> Value {
    match tauri::async_runtime::spawn_blocking(move || build_trae_client_models(variant)).await {
        Ok(value) => value,
        Err(error) => json!({
            "source": "missing",
            "note": format!("读取客户端模型清单失败: {error}"),
        }),
    }
}

fn build_trae_client_models(variant: Option<String>) -> Value {
    let variant = parse_trae_variant(variant.as_deref());
    match serde_json::to_value(trae::model_list::read_client_model_list(variant)) {
        Ok(value) => value,
        Err(error) => json!({
            "source": "missing",
            "note": format!("序列化客户端模型清单失败: {error}"),
        }),
    }
}

/// GET /api/trae/gateway/keys —— 多 Key 列表（含归属产品线）。
///
/// 形状唯一来源：gateway crate 的 `apikey::list_response`（HTTP 通道同款）。
#[tauri::command]
pub fn list_trae_api_keys() -> Value {
    let state = trae_gateway::shared_state();
    buddy_switch_gateway::trae::apikey::list_response(&state.key_store)
}

/// POST /api/trae/gateway/keys —— 新建 Key（`{name, variant}`，明文仅此一次返回）。
#[tauri::command]
pub fn create_trae_api_key(name: Option<String>, variant: Option<String>) -> Value {
    let name = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "未命名 Key".to_string());
    let state = trae_gateway::shared_state();
    buddy_switch_gateway::trae::apikey::create_response(
        &state.key_store,
        name,
        parse_trae_variant(variant.as_deref()),
    )
}

/// POST /api/trae/gateway/keys/revoke —— 吊销 Key（`{id}`）。
#[tauri::command]
pub fn revoke_trae_api_key(id: String) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    let state = trae_gateway::shared_state();
    state.key_store.revoke(id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/trae/gateway/keys/restore —— 撤销吊销（Key 重新可用，`{id}`）。
#[tauri::command]
pub fn restore_trae_api_key(id: String) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    let state = trae_gateway::shared_state();
    state.key_store.restore(id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/trae/gateway/keys/delete —— 物理删除**已吊销**的 Key（`{id}`）。
#[tauri::command]
pub fn delete_trae_api_key(id: String) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    let state = trae_gateway::shared_state();
    state.key_store.delete(id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/trae/open-data-dir —— 打开 Trae 数据目录（`{variant?}`）。
///
/// ⚠️ **别名**：P5 T03 起前端改调参数化命令 [`open_data_dir`]（带 `scope`）。本命令保留，
/// 是 [`open_data_dir`] 的 Trae 分支同源实现（`variant` 先按**区域**折叠成 `scope`）。
///
/// 形状（`{ok,path}` 或结构化 `Unsupported`）由 `handlers::open_data_dir` 唯一产出。
#[tauri::command]
pub async fn open_trae_data_dir(variant: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        trae::handlers::open_data_dir(parse_trae_variant(variant.as_deref()))
    })
    .await
    .map_err(|error| format!("打开数据目录失败: {error}"))?
}

/// POST /api/trae/launch-client —— 启动**该变体**的 Trae 客户端（OAuth 的前置动作）。
///
/// 客户端从没启动过时，它的 `storage.json` 里没有 icube 设备凭证，OAuth 网页登录
/// 必然以 `dataDirMissing` 失败。这个命令让用户**一键**跨过这道前置条件
/// （动机与「启动成功 ≠ 凭证已就绪」的边界见 `handlers::launch_client_for`）。
#[tauri::command]
pub async fn trae_launch_client(variant: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        trae::handlers::launch_client_for(parse_trae_variant(variant.as_deref()))
    })
    .await
    .map_err(|error| format!("启动客户端失败: {error}"))?
}

/// GET /api/trae/gateway/logs —— 最近 N 条请求日志（元数据）。
#[tauri::command]
pub fn get_trae_gateway_logs() -> Value {
    let state = trae_gateway::shared_state();
    json!({ "logs": state.log.list() })
}

/// POST /api/trae/gateway/logs/clear —— 清空日志。
#[tauri::command]
pub fn clear_trae_gateway_logs() -> Value {
    let state = trae_gateway::shared_state();
    state.log.clear();
    json!({ "ok": true })
}

// ---------------------------------------------------------------------------
// 桌面悬浮窗（P9 · T02 只读数据层）
// ---------------------------------------------------------------------------
//
// ★ 只读红线（`PLAN §3.16`「不做的」）：命令面**只有**读 / 显隐 / 聚焦，
//   **绝不含任何写操作**（账号切换 / 批量 / 删除 / 导出 / 禁用模型 / 置顶…）。
//   门禁 `scripts/check-scope-registry.cjs` 的「只读红线」条会逐条扫本段命令体。
//
// ★ 刷新**全在前端 webview**（`src/lib/companion.ts` 的 20s 轮询 + 既有事件订阅）
//   ⇒ 本段**不新增任何 Rust 后台循环**，不触碰 `lib.rs` 的 `BackgroundTask` 注册表
//   （把「任务未登记 ⇒ 永不执行」的风险归零，P9 §1.3 / R9）。

/// 悬浮窗快照的 provider（**只读**，P9 §1.3）。
///
/// ## 本期数据源范围（P9 ✅ #2：只做「已有数据源」的来源）
///
/// 只接由**现有**进度状态（`switch-progress`）推导出的「进行中操作」。跨来源客户端
/// （Codex / CodeBuddy 等）与 P8 会话工作台**不在本期** —— 这里**预留**与 P8 同源的
/// 接口（[`companion_state::LiveOperation`]）但**不依赖**它：P8 就绪时把它的会话状态
/// 也映射成 `LiveOperation` 即可，本函数与前端**无需改动**。
///
/// ## fail-closed，不造假
///
/// 无进行中操作 ⇒ 结构化空态（`source: "unavailable"` + `items: []`），
/// **绝不用演示数据填充**。
fn companion_snapshot() -> companion_state::CompanionSnapshot {
    let mut operations: Vec<companion_state::LiveOperation> = Vec::new();

    // WorkBuddy 切换：进行中 ⇒ 一条 running 项（数据源 = 现有 `switch-progress` 状态）。
    let running = *SWITCH_RUNNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if running {
        let region = SWITCH_REGION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .unwrap_or(Region::Cn);
        let progress = SWITCH_PROGRESS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap_or_default();
        operations.push(companion_state::LiveOperation {
            // id 里带上 region：`companion_jump` 据此聚焦**正在切换的那个**客户端。
            id: format!("workbuddy:{}:switch", region.as_str()),
            source: process::region_app_stems(region)[0].to_string(),
            title: progress,
            hint: String::new(),
            state: companion_state::CompanionState::Running,
        });
    }

    // ★ P15 · 外部 Agent 会话：由客户端 hook 事件驱动（接收端见 `agent_hook_rx`）。
    // ★ 零周期任务 —— 过期在 `operations()` 里**读取时惰性判定**（见 `agent_watch` 模块头）。
    // 两个数据源**合并进同一个快照**：悬浮窗是一个窗口，不是两个（`P15-SPIKE-RESULTS §2 Q1`）。
    {
        let watch = agent_watch::shared()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operations.extend(watch.operations(config::now_ms()));
    }

    companion_state::snapshot_from_operations(&operations, config::now_ms())
}

/// GET /api/companion/status —— 悬浮窗的**只读**快照（P9 §1.3）。
///
/// **只读**：无参数（app 级聚合，✅ #4）、无副作用、不改任何状态。
#[tauri::command]
pub async fn companion_status() -> Result<companion_state::CompanionSnapshot, String> {
    Ok(companion_snapshot())
}

/// POST /api/companion/visibility —— 显示 / 隐藏悬浮窗。
///
/// ★ 这是可见性的**唯一收敛点**：侧栏图标 / 托盘菜单 / 窗口内关闭按钮都经它改变状态，
/// 因此「不许出现第二个看起来能切的控件」这条纪律靠它守住（P9 §1.5 / R8）。
#[tauri::command]
pub async fn companion_set_visible(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    #[cfg(desktop)]
    {
        crate::companion::set_visible(&app, visible).map_err(|error| error.to_string())
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, visible);
        Err("桌面悬浮窗仅在桌面端可用".to_string())
    }
}

/// POST /api/companion/toggle —— 翻转可见性，返回翻转**之后**的状态。
#[tauri::command]
pub async fn companion_toggle(app: tauri::AppHandle) -> Result<bool, String> {
    #[cfg(desktop)]
    {
        crate::companion::toggle(&app).map_err(|error| error.to_string())
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Err("桌面悬浮窗仅在桌面端可用".to_string())
    }
}

/// POST /api/companion/jump —— 聚焦 / 唤起来源客户端（P9 ✅ #1）。
///
/// ★ **不切账号、不导航主窗口、不删改任何数据** —— 悬浮窗只「把你带到那儿」。
/// ⚠️ 来源客户端**未运行** ⇒ 返回**可理解的错误**（前端渲染明确反馈），**不静默失败**。
#[tauri::command]
pub async fn companion_jump(id: String) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("缺少条目 id".to_string());
    }
    // 本期唯一来源 = WorkBuddy（跨来源 Codex / CodeBuddy 不在本期，P9 ✅ #2）。
    // id 形如 `workbuddy:<region>:switch`：region 决定聚焦**哪一个**客户端。
    let mut parts = id.split(':');
    let source = parts.next().unwrap_or_default();
    let region = parts.next().and_then(Region::parse);
    if source != "workbuddy" {
        return Err(format!("未知的来源条目：{id}"));
    }
    let Some(region) = region else {
        return Err(format!("未知的来源条目：{id}"));
    };
    // `focus_workbuddy_for` 内部的 `is_workbuddy_running_for` 在 Windows 上会起 tasklist
    // 子进程 ⇒ 放 blocking 线程，不阻塞 Tauri 主线程（判据见 `get_status` 的注释）。
    tauri::async_runtime::spawn_blocking(move || process::focus_workbuddy_for(region))
        .await
        .map_err(|error| format!("唤起客户端失败: {error}"))?
}

// ---------------------------------------------------------------------------
// 外部 Agent 会话监听（P15 · hook 安装面）
// ---------------------------------------------------------------------------
//
// ★ 与 P9 的**只读红线不冲突**：那一条约束的是**悬浮窗**（`companion_*` 命令面）。
//   本段改的是**上游客户端的配置文件**（装 / 卸 hook），是用户显式同意的动作，
//   不是窗口上的写操作。两者的可见性开关也各自独立。

/// hook 的安装目标：**只列「机制已取证」的客户端**。
///
/// ## 逐个来源为什么在 / 不在表里
///
/// | 目标 | 在表里？ | 依据 |
/// |:---|:---|:---|
/// | `workbuddy-cn` / `workbuddy-global` | ✅ | `settings.json` 的 `hooks` 键（`client_settings::CLIENT_SLOTS`） |
/// | `claude`（Claude Code） | ✅ | `~/.claude/settings.json` 的 `hooks` 键 —— **本机实测**（已有一条第三方 hook） |
/// | `codebuddy`（CodeBuddy CLI） | ✅ | `~/.codebuddy/settings.json` 的 `hooks` 键 —— **本机实测**（已抓过真实 payload） |
/// | `trae-hooks-cn` / `trae-hooks-global` | ✅ | `~/.trae-cn/hooks.json` / `~/.trae/hooks.json`；结构由 Trae 自带 hook 执行器的 serde 结构取证。★ **文件不存在则不创建**（见 `agent_hook_install::missing_config_message`） |
/// | `codex` | ❌ | 机制存在（`~/.codex/hooks.json`），但**路径与 schema 未一手取证**（本机未安装） |
/// | `qoder-*` | ❌ | **对 Qoder 是否有 hooks 机制没有任何取证** |
///
/// ⚠️ 未取证的客户端**刻意不进本表**：往一个不存在的机制上写配置，只会污染用户的
/// 客户端文件 —— 比「暂时不支持」糟得多。它们**仍然是合法的来源**
/// （`agent_watch::HOOK_SOURCES`），用户自行接好 hook 后悬浮窗照常显示。
const AGENT_HOOK_TARGETS: &[(&str, &str)] = &[
    ("workbuddy-cn", agent_watch::SOURCE_WORKBUDDY_CN),
    ("workbuddy-global", agent_watch::SOURCE_WORKBUDDY_GLOBAL),
    ("claude", agent_watch::SOURCE_CLAUDE),
    ("codebuddy", agent_watch::SOURCE_CODEBUDDY),
    ("trae-hooks-cn", agent_watch::SOURCE_TRAE_CN),
    ("trae-hooks-global", agent_watch::SOURCE_TRAE_GLOBAL),
];

/// 组装 hook 安装状态（**只读**，无副作用）。
///
/// 每一项都回答「**这个槽位**现在是什么状态」而不是给一个含糊的总开关：
/// 用户可能只装了国内版、国际版压根没装 —— 那时把「未安装」显示成「已关闭」是误导。
fn agent_hook_status() -> Value {
    let exe = std::env::current_exe().ok();
    let slots: Vec<Value> = AGENT_HOOK_TARGETS
        .iter()
        .map(|(slot_key, source)| {
            let read = client_settings::read_settings(slot_key)
                .unwrap_or_else(|error| json!({ "error": error }));
            let values = read.get("values").cloned().unwrap_or(Value::Null);
            let events = if values.is_null() {
                Vec::new()
            } else {
                agent_hook_install::installed_events_of(
                    &values,
                    agent_hook_install::profile_for(slot_key),
                )
            };
            json!({
                "slot": slot_key,
                "source": source,
                "clientInstalled": read.get("installed").and_then(Value::as_bool).unwrap_or(false),
                // 客户端**正在运行**时改不了它的配置（进程守卫）—— UI 据此提示「请先退出客户端」。
                "clientRunning": read.get("running").and_then(Value::as_bool).unwrap_or(false),
                "installed": !events.is_empty(),
                "events": events,
                "command": exe.as_deref().map(|exe| agent_hook_install::hook_command(exe, source)),
            })
        })
        .collect();

    json!({
        "enabled": app_prefs::watch_agent_sessions(),
        // 接收端端口（排在 UNIFIED-GATEWAY-ARCH §2.2 的端口表末尾，见 `agent_hook_rx::HOOK_PORT`）。
        "port": buddy_switch_core::modules::agent_hook_rx::HOOK_PORT,
        "slots": slots,
    })
}

/// 装 / 卸 hook，并同步偏好。**先做动作、成功了再改偏好**。
///
/// ## 为什么「装不上就不写偏好」
///
/// 偏好是「用户希望它开着」的**唯一事实来源**，UI 与后续启动都以它为准。
/// 若安装全失败却把偏好置成 `true`，用户会看到一个「已开启」的开关，
/// 而客户端里一条 hook 都没有 —— 那种「两边都以为自己对」的状态最难查。
/// 反过来，**卸载**是例外：无论成功与否都把偏好置回 `false`，
/// 因为用户表达的是「别再监听了」，残留的 hook 比一个诚实的「关」更糟。
fn apply_agent_hook_enabled(enabled: bool) -> Result<Value, String> {
    let mut errors: Vec<String> = Vec::new();
    let mut changed: Vec<Value> = Vec::new();

    if enabled {
        let exe =
            std::env::current_exe().map_err(|error| format!("取当前程序路径失败：{error}"))?;
        for (slot_key, source) in AGENT_HOOK_TARGETS {
            let command = agent_hook_install::hook_command(&exe, source);
            match agent_hook_install::install(slot_key, &command) {
                Ok(_) => changed.push(json!({ "slot": slot_key, "ok": true })),
                Err(error) => errors.push(format!("{slot_key}：{error}")),
            }
        }
        if changed.is_empty() {
            return Err(format!("没有任何客户端装上 hook —— {}", errors.join("；")));
        }
    } else {
        for (slot_key, _) in AGENT_HOOK_TARGETS {
            match agent_hook_install::uninstall(slot_key) {
                Ok(_) => changed.push(json!({ "slot": slot_key, "ok": true })),
                Err(error) => errors.push(format!("{slot_key}：{error}")),
            }
        }
    }

    let mut prefs = app_prefs::load_app_prefs();
    prefs["watch_agent_sessions"] = json!(enabled);
    app_prefs::save_app_prefs(&prefs).map_err(|error| format!("保存偏好失败：{error}"))?;

    Ok(json!({ "enabled": enabled, "changed": changed, "errors": errors }))
}

/// GET /api/agent-hooks/status —— hook 安装状态（**只读**）。
///
/// `async` + `spawn_blocking`：本命令要逐槽位读客户端配置文件、并做**进程探测**
/// （Windows 上起 `tasklist` 子进程，Trae 槽位还要逐变体探测），实测 0.5–1.1 s。
/// 同步执行会把这段时长原样压在 Tauri 主线程上 ⇒ 设置页一打开窗口就发灰（issue #48）。
#[tauri::command]
pub async fn get_agent_hook_status() -> Value {
    match tauri::async_runtime::spawn_blocking(agent_hook_status).await {
        Ok(value) => value,
        Err(error) => json!({ "error": format!("读取 hook 状态失败: {error}") }),
    }
}

/// POST /api/agent-hooks/enabled —— 开启 / 关闭外部 Agent 会话监听。
///
/// `async` + `spawn_blocking`：装 hook 要**读写客户端配置文件**（慢 IO），
/// 不能跑在 Tauri 主线程上（判据见下方 `command_threading_tests`）。
#[tauri::command]
pub async fn set_agent_hook_enabled(enabled: bool) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || apply_agent_hook_enabled(enabled))
        .await
        .map_err(|error| format!("切换监听失败：{error}"))?
}

// ---------------------------------------------------------------------------
// P15 · 把「本工具自己的长操作」也送进悬浮窗（签到）
// ---------------------------------------------------------------------------
//
// ★ 依据是 `P9-ARCH §1.3`：数据源写成「`switch-progress` / `checkin-progress` /
//   `trae-checkin-progress`」三个，但实现里后两个**长期缺席** ——
//   `src/lib/companion.ts` 一直在订阅一个**从未被 emit 过的** `checkin-progress`。
//   本节把签到接上（走 `agent_watch::record_local`，与客户端 hook 共用同一张表）。
//
// ★ 「本工具自己的操作」与「客户端里的任务」在窗口里用**不同的来源**区分
//   （`buddy-switch` vs `workbuddy-cn`），标题也写明是什么操作。

/// Trae 批量签到的进度事件 → 悬浮窗里的一行。
///
/// Trae 侧有**逐账号**进度（`start` / `account` / `done`），标题随进度更新。
fn observe_trae_checkin(event: &Value) {
    let now = config::now_ms();
    match event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "start" => {
            let total = event.get("total").and_then(Value::as_u64).unwrap_or(0);
            agent_watch::record_local(
                "trae-checkin",
                &format!("Trae 签到（共 {total} 个账号）"),
                companion_state::CompanionState::Running,
                now,
            );
        }
        "account" => {
            let name = event.get("name").and_then(Value::as_str).unwrap_or("账号");
            let status = match event
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "success" => "成功",
                "already" => "今日已签",
                _ => "失败",
            };
            agent_watch::record_local(
                "trae-checkin",
                &format!("Trae 签到：{name} {status}"),
                companion_state::CompanionState::Running,
                now,
            );
        }
        // 终态判定与 webui 侧**共用同一段逻辑**（字段名相同，见 `observe_trae_checkin_summary`）。
        "done" => observe_trae_checkin_summary("trae-checkin", event),
        _ => {}
    }
}

/// Trae 签到的**汇总**（`{ok, already, failed, total}`）→ 悬浮窗终态。
///
/// ⚠️ 与 `crates/buddy-switch-server/src/api.rs` 的同名函数**逐字同款**：
/// 桌面侧从 `done` 事件取、webui 侧从一次性报告取，**字段名相同** ⇒
/// 两条通道的终态判定不可能分叉。
fn observe_trae_checkin_summary(task: &str, summary: &Value) {
    let ok = summary.get("ok").and_then(Value::as_u64).unwrap_or(0);
    let already = summary.get("already").and_then(Value::as_u64).unwrap_or(0);
    let failed = summary.get("failed").and_then(Value::as_u64).unwrap_or(0);
    let total = summary.get("total").and_then(Value::as_u64).unwrap_or(0);
    // **计划 0 个** ⇒ 这一行本就不该出现（一个字都没签）：撤掉，
    // 而不是留下「成功 0 · 已签 0 · 失败 0」这种没信息量的噪音行。
    if total == 0 {
        forget_local(task);
        return;
    }
    // **一个都没成**（既没有成功、也没有「今日已签」）才算失败。
    let state = if ok == 0 && already == 0 {
        companion_state::CompanionState::Failed
    } else {
        companion_state::CompanionState::Done
    };
    agent_watch::record_local(
        task,
        &format!("Trae 签到：成功 {ok} · 已签 {already} · 失败 {failed}"),
        state,
        config::now_ms(),
    );
}

/// 撤掉一条「本工具自己的操作」（见 `agent_watch::AgentWatch::forget`）。
fn forget_local(task: &str) {
    agent_watch::shared()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .forget(agent_watch::SOURCE_THIS_TOOL, task);
}

/// WorkBuddy 批量签到的**开始**一条，返回 task id（结束时要用**同一个** id 才能覆盖成终态）。
///
/// ⚠️ WB 侧**没有**逐账号进度事件（`run_checkin_all_for` 一次性返回）——
/// 所以这里只有首尾两条。那是**如实反映客户端能力**，不是漏做。
fn observe_workbuddy_checkin_start(region: Region) -> String {
    let task = format!("workbuddy-checkin:{}", region.as_str());
    agent_watch::record_local(
        &task,
        &format!("WorkBuddy 签到（{}）", workbuddy_region_label(region)),
        companion_state::CompanionState::Running,
        config::now_ms(),
    );
    task
}

/// 见 [`observe_workbuddy_checkin_start`]。
fn observe_workbuddy_checkin_end(task: &str, region: Region, result: &Value) {
    // 带 `status` = 这轮**根本没跑**（`skipped` / `unsupported`）⇒ 把刚写的那行**撤掉**。
    // 显示成「已完成」或「已中止」都是在编造一件没发生的事。
    if result.get("status").is_some() {
        forget_local(task);
        return;
    }
    let accounts = result
        .get("accounts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = accounts.len();
    // **账号表是空的** ⇒ 同样什么都没发生（撤掉，不留「成功 0 / 共 0」的噪音行）。
    if total == 0 {
        forget_local(task);
        return;
    }
    let failed = accounts
        .iter()
        .filter(|account| account.get("error").is_some_and(|error| !error.is_null()))
        .count();
    let ok = total.saturating_sub(failed);
    let state = if ok == 0 {
        companion_state::CompanionState::Failed
    } else {
        companion_state::CompanionState::Done
    };
    agent_watch::record_local(
        task,
        &format!(
            "WorkBuddy 签到（{}）：成功 {ok} / 共 {total}",
            workbuddy_region_label(region)
        ),
        state,
        config::now_ms(),
    );
}

/// WorkBuddy 区域的展示名（**只用于标题**，不进 `source`）。
fn workbuddy_region_label(region: Region) -> &'static str {
    if region == Region::Global {
        "国际版"
    } else {
        "国内版"
    }
}

/// 护栏：**慢 IO / 起子进程**的命令必须 `async`（否则阻塞 Tauri 主线程）。
///
/// ## 判据来自本文件自己的约定
///
/// `get_status` 的注释写得很明确：同步 command 默认在 Tauri **主线程**执行，一旦涉及
/// 子进程或慢 IO，就会阻塞原生窗口消息循环（拖拽标题栏时尤其明显）。同文件的
/// `get_token_statistics` / `get_trae_profiles` / `trae_import_local_account` 都按此办理
/// —— 说明「慢 IO 却仍同步」的那些是**遗漏而非设计**（审计 P1-3）。
///
/// ## 为什么用源码扫描而不是行为断言
///
/// 「是否阻塞主线程」需要真实 Tauri 运行时 + 卡顿度量，单测里做不了；而「声明是不是
/// `async`」是**确定性、可判定**的性质。两者之间只差一步推理（同步 ⇒ 跑在主线程），
/// 而那一步正是本文件注释所陈述的约定 —— 把它写下来，比让它继续靠口口相传好。
#[cfg(test)]
mod command_threading_tests {
    /// 本文件自己的源码（`include_str!` 相对路径 ⇒ 与本文件同目录）。
    const SOURCE: &str = include_str!("commands.rs");

    /// 这些命令做**慢 IO 或起子进程**，必须 `async` + `spawn_blocking`。
    ///
    /// 2026-09-28 之前它们全是同步的（审计 `docs/history/perf-audit-2026-09-24.md` 的 P1-3）。
    /// 把它们改回同步会让本用例变红。
    ///
    /// 2026-10-20 按 issue #48 补齐第二批（S 级清单）：都是「点一下就灰屏」的命令 ——
    /// 起 `tasklist` 子进程、开 SQLite、全量读日志、`spawn explorer`。
    const MUST_BE_ASYNC: &[&str] = &[
        "import_local",           // 读客户端认证文件
        "check_auth_permission",  // 写 / 删探针文件
        "list_sessions",          // 读 workbuddy.db（不可读时降级扫 projects 目录）
        "get_trae_variants",      // 进程探测（Windows 起 tasklist 子进程）
        "get_trae_accounts",      // 读账号库 + 快照目录
        "get_trae_logs",          // 全量读日志文件
        "set_agent_hook_enabled", // 读写上游客户端 settings.json（P15 · hook 安装面）
        // ---- issue #48 第二批 ----
        // 注：本数组的尾部注释**不手工对齐** —— 新版 rustfmt 会按最长项重排，
        // 手工对齐反而会让 `cargo fmt --check` 红（CI 用的是较新的 stable）。
        "get_agent_hook_status", // 6 槽位读配置 + 逐个 tasklist（0.5–1.1 s）
        "get_trae_gateway_models", // 候选目录 + 开 SQLite + 解析 319 KB JSON
        "get_trae_client_models", // 同上，且与上一条被 Promise.all 同批打
        "get_trae_token_statistics", // 全量读 + 两遍解析网关日志（零缓存）
        "get_trae_env",          // tasklist + 扫两遍安装候选 + 逐目录 stat
        "open_data_dir",         // spawn explorer ≈160 ms
        "open_accounts_dir",     // 建目录 + spawn explorer
        "open_workbuddy_data_dir", // 同上（WB 别名）
        "open_trae_data_dir",    // 同上（Trae 别名）
        "trae_launch_client",    // 起客户端进程
    ];

    #[test]
    fn slow_io_commands_must_be_declared_async() {
        for name in MUST_BE_ASYNC {
            assert!(
                SOURCE.contains(&format!("pub async fn {name}")),
                "`{name}` 做慢 IO / 起子进程，必须声明为 `async` 并用 `spawn_blocking` \
                 （否则会阻塞 Tauri 主线程，判据见 `get_status` 的注释）。"
            );
            assert!(
                !SOURCE.contains(&format!("pub fn {name}(")),
                "`{name}` 被改回同步了：同步 command 跑在 Tauri 主线程上。"
            );
        }
    }
}

/// P5 T03 参数化命令的 **fail-closed** 护栏。
///
/// **与 `crates/buddy-switch-server/src/api.rs` 的
/// `parameterized_account_commands_reject_unknown_scope` 互为镜像**：两个宿主对同一个
/// `scope` 参数必须**同一判据**（未知 / 空 / 缺一律 `Err`），否则桌面端与 webui 会分叉
/// —— 这正是 P5 要防的一类漂移（同款实现散落两 crate 是刻意不去重，见共享知识）。
///
/// 可证伪：把某条命令的 `resolve_scope_param(...)` 换成 `unwrap_or(Scope::WbCn)` 式回落，
/// 断言立即变红 —— 那正是「拼错 `trae-glboal` 静默读写 `wb-cn` 的库」的缺陷形态。
#[cfg(test)]
mod parameterized_scope_tests {
    use super::{
        accounts_export, accounts_export_to_path, accounts_import, accounts_import_local,
        build_open_data_dir, checkin_status, credits,
    };

    #[test]
    fn parameterized_account_commands_reject_unknown_scope() {
        // 未知 / 拼错 / 空一律 Err（fail-closed），且都在**落盘之前**失败（不触达任何库）。
        assert!(accounts_export("trae-glboal".to_string(), Vec::new()).is_err());
        assert!(accounts_export_to_path("nope".to_string(), Vec::new(), "x".to_string()).is_err());
        assert!(accounts_import(String::new(), String::new(), Vec::new()).is_err());
        // `open_data_dir` 是**异步**参数化命令（慢 IO 不得跑在主线程，issue #48）：
        // 未知 scope 必须在**定位目录 / 起子进程之前**失败 —— 断言打的是它的同步内核，
        // 这样不必为「参数校验」引入一个 async 运行时。
        assert!(build_open_data_dir("trae-glboal".to_string()).is_err());
        assert!(build_open_data_dir(String::new()).is_err());

        // 错误文案必须列出合法取值，便于用户对照正确拼写。
        let error = accounts_export("trae-glboal".to_string(), Vec::new()).unwrap_err();
        assert!(error.contains("wb-cn"), "{error}");
        assert!(error.contains("trae-global"), "{error}");
    }

    /// `credits` 是 **async** 参数化命令：未知 scope 必须在**取账号 / 读缓存之前**失败。
    ///
    /// 与上面那条同步断言同源（同款实现散落两 crate 是刻意不去重）。
    #[tokio::test]
    async fn parameterized_credits_rejects_unknown_scope() {
        let error = credits("trae-glboal".to_string(), None).await.unwrap_err();
        assert!(error.contains("wb-cn"), "{error}");
        assert!(error.contains("trae-global"), "{error}");
        // 缺 scope（空串）同样 Err，绝不回落默认作用域。
        assert!(credits(String::new(), None).await.is_err());
    }

    /// `checkin_status` 同 [`credits`]：**async** 参数化命令，未知 scope 必须在
    /// **取账号 / 读签到缓存之前**失败（fail-closed）。
    ///
    /// 与 server 侧 `parameterized_account_commands_reject_unknown_scope` 互为镜像。
    #[tokio::test]
    async fn parameterized_checkin_status_rejects_unknown_scope() {
        let error = checkin_status("trae-glboal".to_string(), None)
            .await
            .unwrap_err();
        assert!(error.contains("wb-cn"), "{error}");
        assert!(error.contains("trae-global"), "{error}");
        assert!(checkin_status(String::new(), None).await.is_err());
    }

    /// `accounts_import_local` 是 **async** 参数化命令：未知 scope 必须在**读客户端登录态 /
    /// 写账号库之前**失败（fail-closed）。
    ///
    /// 与 server 侧 `parameterized_account_commands_reject_unknown_scope` 互为镜像。
    #[tokio::test]
    async fn parameterized_accounts_import_local_rejects_unknown_scope() {
        let error = accounts_import_local("trae-glboal".to_string())
            .await
            .unwrap_err();
        assert!(error.contains("wb-cn"), "{error}");
        assert!(error.contains("trae-global"), "{error}");
        assert!(accounts_import_local(String::new()).await.is_err());
    }
}

// ---------------------------------------------------------------------------
// 统一网关（第三个产品线 · 管理面）
//
// ★ 它**跨平台**（把两个产品线的模型合成一个入口），因此不属于任何单一产品。
//   能力门控表是按平台组织的，这里取 WorkBuddy 的 `gateway` 能力作门控 ——
//   桌面端恒具备，且「网关类能力可用与否」对两个产品线是一致的。
//
// ★ 这组命令（9 条）的名字与路径前缀（`/api/unified-gateway/...`）刻意与
//   `*_trae_api_key` / `list_api_keys` 两条既有线路**同形**：命名面一致，
//   前端与门禁（`scripts/check-api-contract.cjs`）都能按同一套规则校验。
// ---------------------------------------------------------------------------

/// GET /api/unified-gateway/config —— 配置 + 运行状态 + 命名空间（已启用 / 全部在册）。
#[tauri::command]
pub async fn get_unified_gateway_config(app: tauri::AppHandle) -> Value {
    let state = unified_gateway::shared_state();
    // ★ 共同字段一律走**共享构造**（两处宿主同形；`running`/`addr` 是本宿主独占的两项）。
    let mut payload = buddy_switch_gateway::unified::admin::config_payload(&state).await;
    let runtime = app.state::<unified_gateway::UnifiedGatewayRuntime>();
    payload["running"] = json!(runtime.is_running());
    payload["addr"] = json!(runtime.addr());
    payload
}

/// POST /api/unified-gateway/config —— 保存并应用（启动 / 重启独立监听）。
#[tauri::command]
pub async fn save_unified_gateway_config(
    app: tauri::AppHandle,
    config: Value,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::Gateway)?;
    let submitted = config.get("config").cloned().unwrap_or(config);
    let parsed: UnifiedGatewayConfig =
        serde_json::from_value(submitted).map_err(|error| format!("配置格式错误: {error}"))?;
    // ★ 不在册的命名空间 / 非法的默认命名空间 ⇒ **明确报错**（不静默丢弃用户填的值，
    //   否则用户会看到「明明加了却没生效」）
    parsed.validate()?;
    parsed.save()?;

    let state = unified_gateway::shared_state();
    state.set_config(parsed.clone()).await;

    let runtime = app.state::<unified_gateway::UnifiedGatewayRuntime>();
    let addr = runtime.apply().await?;
    Ok(json!({ "ok": true, "config": parsed, "running": addr.is_some(), "addr": addr }))
}

/// GET /api/unified-gateway/models —— 本入口已启用命名空间的清单预览。
///
/// ★ **管理面专用**：它不过滤 Key 授权、也不需要 Key（看的是「本入口启用了哪些」），
/// 因此**只在管理面暴露**，绝不挂在网关监听端口上。
///
/// `refresh = true` ⇒ 绕过各产品线的识别缓存重取（WB 走唯一探测入口、Qoder 忽略 TTL；
/// Trae 无缓存）—— 「重新识别全部」按钮靠它才不是假控件。
/// ⚠️ 与 server 侧 `api_unified_gateway_models` **刻意不去重**（两宿主各一份），
/// 行为必须一致（见 `crates/buddy-switch-server/src/api.rs` 的同名 handler）。
#[tauri::command]
pub async fn get_unified_gateway_models(refresh: Option<bool>) -> Value {
    buddy_switch_gateway::unified::routes::models_preview(refresh.unwrap_or(false)).await
}

/// GET /api/unified-gateway/pool-overview —— 已启用命名空间指向的**账号池总览**（只读投影）。
///
/// 统一网关没有自己的池：它按命名空间把请求分派到三条产品线各自的池。本命令把它们
/// **已有的**池视图投影成一份跨平台汇总（**不新增任何选号逻辑**）。
///
/// ★ 形状 / 分组的**唯一构造点**在 `buddy_switch_gateway::unified::admin::pool_overview_payload`
///   （见那里的模块文档：两处宿主曾各拼一份 JSON 导致桌面端与 webui 形状分裂）。
/// ⚠️ 与 server 侧 `api_unified_gateway_pool_overview` **刻意不去重**。
#[tauri::command]
pub async fn unified_gateway_pool_overview() -> Value {
    buddy_switch_gateway::unified::admin::pool_overview_payload().await
}

/// GET /api/unified-gateway/keys —— 统一密钥列表（**只出脱敏**，绝不含 hash/明文）。
#[tauri::command]
pub fn list_unified_api_keys() -> Value {
    // ★ 形状 `{ "keys": [...] }`，与两个既有网关**同形** —— 见
    //   `buddy_switch_gateway::unified::admin` 的模块文档（裸数组曾让桌面端整页白屏）。
    buddy_switch_gateway::unified::admin::keys_list_payload(&unified_gateway::shared_state().keys)
}

/// POST /api/unified-gateway/keys —— 创建统一密钥；**明文只返回这一次**。
///
/// 授权集不合规（空 / 不在册 / 业务转发非 WB 或不在授权集内）⇒ 直接报错，
/// 校验发生在**落盘之前**（不留一把废 Key）。
#[tauri::command]
pub fn create_unified_api_key(
    name: Option<String>,
    namespaces: Vec<String>,
    biz_namespace: Option<String>,
) -> Result<Value, String> {
    let name = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "未命名".to_string());
    let (record, plaintext) =
        unified_gateway::shared_state()
            .keys
            .create(name, namespaces, biz_namespace)?;
    Ok(json!({ "ok": true, "key": plaintext, "record": record.masked() }))
}

/// POST /api/unified-gateway/keys/revoke —— 吊销（置标记，**可逆**）。
#[tauri::command]
pub fn revoke_unified_api_key(id: String) -> Result<Value, String> {
    unified_gateway::shared_state().keys.revoke(&id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/unified-gateway/keys/restore —— 撤销吊销（**同一把 Key 重新可用**）。
///
/// 与 `delete` 互逆：只接受**已吊销**的 Key。撤销一次误吊销不需要重建 Key、
/// 也不需要改动任何客户端配置。
#[tauri::command]
pub fn restore_unified_api_key(id: String) -> Result<Value, String> {
    unified_gateway::shared_state().keys.restore(&id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/unified-gateway/keys/delete —— 物理删除（**仅限已吊销**，不可逆）。
#[tauri::command]
pub fn delete_unified_api_key(id: String) -> Result<Value, String> {
    unified_gateway::shared_state().keys.delete(&id)?;
    Ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Qoder（第三个平台）
// ---------------------------------------------------------------------------
//
// ★ 全部命令**参数化 `scope`**，且一律经 `resolve_scope_param` **fail-closed** 解析
//   （拼错即拒，绝不回落默认作用域）—— 与既有 `accounts_list` 等参数化命令同一纪律。
//   Qoder 的双区域是**两本互不相通的账号库**，缺 scope 就无从判断该读哪本。
//
// ⚠️ 本文件在 `crates/buddy-switch-server/src/api.rs` 有**同款**接线（刻意不去重，
//   见文件头说明）。改一处必须 grep 另一处。

/// 该端点**只服务 WorkBuddy / Trae** —— Qoder 作用域一律给可诊断的错误。
///
/// ★ **刻意不回落**到任何既有平台：把 Qoder 的请求静默打到 WorkBuddy 的账号库上，
/// 比报错坏得多（用户会看到「账号凭空消失」或「改错了别的平台的号」）。
/// Qoder 有自己的命令族（`qoder_*`）。
fn qoder_scope_not_applicable() -> Result<Value, String> {
    Err("该端点只服务 WorkBuddy / Trae 作用域，不支持 Qoder —— \
         Qoder 的账号管理请用 Qoder 分区自己的命令（qoder_*）。"
        .to_string())
}

/// GET /api/qoder/accounts —— 账号列表（**不含令牌明文**）。
#[tauri::command]
pub async fn get_qoder_accounts(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || qoder::account::accounts_view_for(region))
        .await
        .map_err(|error| format!("读取 Qoder 账号失败: {error}"))
}

/// GET /api/qoder/local-sources —— 本机凭据来源探测（**只读、不落库**）。
#[tauri::command]
pub async fn get_qoder_local_sources(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || qoder::profile::probe_view_for(region))
        .await
        .map_err(|error| format!("探测 Qoder 本机凭据失败: {error}"))
}

/// POST /api/qoder/accounts/import-local —— 从本机已登录的客户端导入账号。
#[tauri::command]
pub async fn qoder_import_local(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || qoder::profile::import_from_local_for(region))
        .await
        .map_err(|error| format!("导入 Qoder 本机账号失败: {error}"))?
}

/// POST /api/qoder/accounts/import-pat —— 由 PAT 导入一条账号（**纯本地**，不访问上游）。
#[tauri::command]
pub async fn qoder_import_pat(
    scope: String,
    pat: String,
    nickname: Option<String>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || {
        qoder::profile::import_from_pat_for(region, &pat, nickname)
    })
    .await
    .map_err(|error| format!("导入 Qoder PAT 失败: {error}"))?
}

/// POST /api/qoder/accounts/remark —— 改备注（唯一的「只改一个字段」入口）。
#[tauri::command]
pub async fn qoder_set_account_remark(
    scope: String,
    uid: String,
    remark: Option<String>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let changed = tauri::async_runtime::spawn_blocking(move || {
        qoder::account::set_remark_for(region, &uid, remark)
    })
    .await
    .map_err(|error| format!("保存 Qoder 备注失败: {error}"))??;
    Ok(json!({ "ok": changed }))
}

/// POST /api/qoder/accounts/delete —— 删除一条账号（**软删除**：移入回收站，可恢复）。
#[tauri::command]
pub async fn qoder_delete_account(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let removed =
        tauri::async_runtime::spawn_blocking(move || qoder::trash::trash_account_for(region, &uid))
            .await
            .map_err(|error| format!("删除 Qoder 账号失败: {error}"))??;
    Ok(json!({ "ok": removed }))
}

/// POST /api/qoder/accounts/refresh —— 刷新一条账号的令牌（device / job / PAT 三路由）。
#[tauri::command]
pub async fn qoder_refresh_account(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    qoder::refresh::refresh_for(region, &uid).await
}

/// GET /api/qoder/checkin/status —— 区域签到状态（逐账号 + 汇总；国际版如实报未开放）。
#[tauri::command]
pub async fn qoder_checkin_status(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    Ok(qoder::checkin::status_for(region).await)
}

/// POST /api/qoder/checkin/claim —— 单账号签到（状态驱动；见 checkin 模块头）。
#[tauri::command]
pub async fn qoder_checkin_claim(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    qoder::checkin::claim_for(region, &uid).await
}

/// GET /api/qoder/checkin/status-map —— 逐账号「今日签到状态」（**只读**；issue #47）。
///
/// `account_ids` 是**逗号分隔**的 uid 列表（与 HTTP 通道同一形状，见 server 侧
/// `parse_id_list` 的说明）：两个通道收同一个串，才不会出现「浏览器好用、桌面端拿不到 id」。
#[tauri::command(rename_all = "camelCase")]
pub async fn qoder_checkin_status_map(
    scope: String,
    account_ids: Option<String>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    Ok(qoder::checkin::status_map_for(region, split_id_list(account_ids)).await)
}

/// GET /api/qoder/credits —— 区域额度 / 套餐视图（逐账号 + 汇总；部分失败如实报）。
#[tauri::command]
pub async fn qoder_credits(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    Ok(qoder::credits::region_view(region).await)
}

/// POST /api/qoder/oauth/start —— 发起设备授权，返回 `{ loginId, verificationUri, expiresIn }`。
///
/// 与 WB 的 `oauth_start` **同形**（前端 `OAuthLoginDialog` 是共用壳），但**必须带 scope**：
/// 双区域各有自己的授权页与 `client_id`，缺 scope 无从判断该发哪一区的登录。
#[tauri::command]
pub async fn qoder_oauth_start(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    qoder::oauth::login_start_for(region).await
}

/// POST /api/qoder/oauth/status —— 轮询一次登录结果（`{ done, result?, error? }`）。
#[tauri::command]
pub async fn qoder_oauth_status(scope: String, login_id: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    Ok(qoder::oauth::login_poll_for(region, &login_id).await)
}

/// GET /api/qoder/programs —— 程序位状态（只读；界面据此渲染胶囊行与「当前登录」徽章）。
#[tauri::command]
pub async fn qoder_program_status(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || qoder::switch::program_status_for(region))
        .await
        .map_err(|error| format!("读取 Qoder 程序位状态失败: {error}"))
}

/// POST /api/qoder/accounts/switch —— 切换账号（把凭据写回指定程序位的登录态）。
///
/// ⚠️ 这是**写客户端文件**的动作（本项目里与 `switch_account` 同级）：客户端在跑时会被
/// 拒绝（`switch::program_running` 守卫），失败自动还原备份。
#[tauri::command]
pub async fn qoder_switch_account(
    scope: String,
    program: String,
    uid: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let program = qoder::switch::QoderProgram::parse(&program)
        .ok_or_else(|| format!("未知的 Qoder 程序位: {program}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        qoder::switch::switch_account_for(region, program, &uid)
    })
    .await
    .map_err(|error| format!("切换 Qoder 账号失败: {error}"))?
}

/// POST /api/qoder/accounts/group —— 改自定义分组（与备注同构）。
#[tauri::command]
pub async fn qoder_set_account_group(
    scope: String,
    uid: String,
    group: Option<String>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || qoder::account::set_group_for(region, &uid, group))
        .await
        .map_err(|error| format!("保存 Qoder 分组失败: {error}"))?
        .map(|changed| json!({ "ok": changed }))
}

/// GET /api/qoder/usage —— 逐日用量（**单位是 credits，不是 token**）。
///
/// ★ 为什么不是 token：Qoder CN 客户端按 credits 计费，客户端三处存储里的 token 计数
/// **恒为 0**（会话 jsonl / 运行日志 / 客户端 SQLite 的 `tokenCountsAvailable:false`）。
/// 详见 `qoder::usage` 模块头。**宁可换个诚实的单位，也不伪造 token 数字。**
#[tauri::command]
pub async fn qoder_usage(scope: String, days: Option<u32>) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    tauri::async_runtime::spawn_blocking(move || qoder::usage::usage_view_for(region, days))
        .await
        .map_err(|error| format!("读取 Qoder 用量失败: {error}"))
}

// ---------------------------------------------------------------------------
// Qoder API 网关（管理面）
// ---------------------------------------------------------------------------
//
// 与 Trae 侧的管理命令**形状对齐**（get config / save config / status /
// models / keys / logs / clear logs）。差异有两处：
// ① 归属轴是**区域**而不是程序位（Key 绑定 `region`）；
// ② **一个监听服务两个区域** —— `enabled` / `port` / `running` 是两区域共用的事实，
//    只有池 / Key 前缀 / 清单按 `scope` 分家。
//
// 多 Key 管理走 `qoder_gateway::shared_state().key_store`：Key 只存哈希 + 前缀 +
// `region`，明文仅在创建时一次性返回；形状由 gateway crate 的 `QoderApiKeyStore`
// 唯一产出（`MEMORY.md §二`：两条通道不得各自拼装）。

/// GET /api/qoder/gateway/config —— Qoder 网关配置（**全局唯一**，不分区域）。
#[tauri::command]
pub fn get_qoder_gateway_config() -> Value {
    serde_json::to_value(buddy_switch_gateway::qoder::QoderGatewayConfig::load())
        .unwrap_or(Value::Null)
}

/// POST /api/qoder/gateway/config —— 保存配置并应用（启动 / 重启独立监听）。
///
/// 注意 `max_body_mb` 是**构造期**固化进 axum `DefaultBodyLimit` 的（与另两个网关
/// 同理），改它要等下一次 `apply()` —— 而本命令每次都会重建监听，所以只要走这里就一定生效。
#[tauri::command]
pub async fn save_qoder_gateway_config(
    app: tauri::AppHandle,
    config: Value,
) -> Result<Value, String> {
    require_capability(Product::Qoder, capability::Capability::Gateway)?;
    let submitted = config.get("config").cloned().unwrap_or(config);
    let parsed: buddy_switch_gateway::qoder::QoderGatewayConfig =
        serde_json::from_value(submitted).map_err(|error| format!("配置格式错误: {error}"))?;
    parsed.save()?;

    let state = qoder_gateway::shared_state();
    *state.config.write().await = parsed.clone();
    state.log.set_keep(parsed.log_keep);
    state.log.set_log_bodies(parsed.log_bodies);

    let runtime = app.state::<qoder_gateway::QoderGatewayRuntime>();
    let addr = runtime.apply().await?;
    Ok(json!({
        "ok": true,
        "config": parsed,
        "running": addr.is_some(),
        "addr": addr,
    }))
}

/// GET /api/qoder/gateway/status —— 运行状态 + **该区域**账号池 + 账号明细 + 诊断。
///
/// `scope` 必填且 fail-closed（`qoder-cn` / `qoder-global`）：Qoder 的双区域是两本
/// 互不相通的账号库，缺 scope 就无从判断该看哪本 —— **不**回落任意一本。
#[tauri::command]
pub async fn qoder_gateway_status(app: tauri::AppHandle, scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let (running, addr) = {
        let runtime = app.state::<qoder_gateway::QoderGatewayRuntime>();
        (runtime.is_running(), runtime.addr())
    };
    Ok(buddy_switch_gateway::qoder::status_view(
        &qoder_gateway::shared_state(),
        running,
        addr,
        update::APP_VERSION,
        region,
    )
    .await)
}

/// GET /api/qoder/gateway/models —— 对外暴露的模型清单（**随上游刷新**）。
///
/// 与 Trae 侧不同：Qoder **没有静态常量清单**，清单只能向官方拉（需账号凭据）。
/// 拉不到时给空清单 + 可读 `note` + `source`（`live` / `cached` / `unavailable`），
/// **绝不**伪造一份「像真的」清单（`source = unavailable` 时前端呈现空态与原因）。
///
/// 返回的是**未过滤**的全量清单 + 逐条 `disabled` 标记：表格要能同时显示
/// 「已禁用」的行（过滤掉的话，用户会以为这个模型从上游消失了）；「网关真正暴露的
/// 条数」= 全量 − 已禁用，由前端按同一份 `disabled` 现算（与出口 ① 同源）。
///
/// 响应形状由 gateway crate 的 `payload::admin_models_response` **唯一产出**
/// （两宿主共用，本层不拼装）。`refresh = true` ⇒ 绕过 10 分钟缓存强制重拉。
#[tauri::command]
pub async fn get_qoder_gateway_models(
    scope: String,
    refresh: Option<bool>,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let state = qoder_gateway::shared_state();
    let catalog = if refresh.unwrap_or(false) {
        state.refresh_catalog(region).await
    } else {
        state.catalog_for(region).await
    };
    // 本命令是**异步**的（清单可能要向上游拉，属网络 IO），故用 `.read().await`；
    // 与网关本体（`qoder/routes.rs`）读的是**同一份** `disabled`（按作用域键控）。
    let disabled = {
        let guard = state.disabled.read().await;
        guard
            .get(buddy_switch_core::modules::product::scope_of_qoder_region(
                region,
            ))
            .clone()
    };
    Ok(buddy_switch_gateway::qoder::payload::admin_models_response(
        region,
        &catalog,
        &disabled,
        state.max_context_now(),
    ))
}

/// GET /api/qoder/gateway/keys —— 多 Key 列表（含归属区域）。
///
/// 形状唯一来源：gateway crate 的 `QoderApiKeyStore::list`（HTTP 通道同款）。
#[tauri::command]
pub fn list_qoder_api_keys() -> Value {
    let state = qoder_gateway::shared_state();
    json!({ "keys": state.key_store.list() })
}

/// POST /api/qoder/gateway/keys —— 新建 Key（`{name, scope}`，明文仅此一次返回）。
#[tauri::command]
pub fn create_qoder_api_key(scope: String, name: Option<String>) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let name = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "未命名 Key".to_string());
    let state = qoder_gateway::shared_state();
    let (record, plaintext) = state.key_store.create(name, region)?;
    Ok(json!({
        "ok": true,
        "key": plaintext,
        "record": record.masked(),
    }))
}

/// POST /api/qoder/gateway/keys/revoke —— 吊销 Key（`{id}`）。
#[tauri::command]
pub fn revoke_qoder_api_key(id: String) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    let state = qoder_gateway::shared_state();
    state.key_store.revoke(id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/qoder/gateway/keys/restore —— 撤销吊销（Key 重新可用，`{id}`）。
#[tauri::command]
pub fn restore_qoder_api_key(id: String) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    let state = qoder_gateway::shared_state();
    state.key_store.restore(id)?;
    Ok(json!({ "ok": true }))
}

/// POST /api/qoder/gateway/keys/delete —— 物理删除**已吊销**的 Key（`{id}`）。
#[tauri::command]
pub fn delete_qoder_api_key(id: String) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    let state = qoder_gateway::shared_state();
    state.key_store.delete(id)?;
    Ok(json!({ "ok": true }))
}

/// GET /api/qoder/gateway/logs —— 最近 N 条请求日志（元数据；含 `region` 归属）。
#[tauri::command]
pub fn get_qoder_gateway_logs() -> Value {
    let state = qoder_gateway::shared_state();
    json!({ "logs": state.log.list() })
}

/// POST /api/qoder/gateway/logs/clear —— 清空日志。
#[tauri::command]
pub fn clear_qoder_gateway_logs() -> Value {
    let state = qoder_gateway::shared_state();
    state.log.clear();
    json!({ "ok": true })
}
