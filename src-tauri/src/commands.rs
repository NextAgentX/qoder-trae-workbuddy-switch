//! Tauri commands：前端调用的薄包装，对应 Python 版 HTTP API。
//!
//! 阶段 1 覆盖：get_status / get_accounts / delete_account / oauth_start /
//! oauth_status / import_local。

use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Mutex;

use tauri::{Emitter, Manager};
use buddy_switch_core::modules::{
    account, auth_file, backup, capability, checkin, config, credit_usage, credits, export_import,
    migrate, oauth, process, programs, refresh, region::Region, region::RegionFilter, rotate, session, switch, token_stats, trae,
    travel, update,
};
// P5 §2.5 第 (2) 条：capability **默认拒绝**。表数据只在 core（`product_spec`），本文件只查表。
// ⚠️ 本文件在 `crates/buddy-switch-server/src/api.rs` 有**同款**接线（刻意不去重，见共享知识）。
use buddy_switch_core::modules::product::{
    require_capability, require_trae_region, require_wb_region, resolve_scope_param, Product,
};
use buddy_switch_core::modules::trae::region::TraeRegion;
// P9 · 悬浮窗**只读**数据模型 + provider（core）。⚠️ 与桌面窗口模块 `crate::companion`
// **不是一回事**：前者是跨宿主的只读数据模型，后者是 Tauri 的独立窗口实现。
use buddy_switch_core::modules::companion as companion_state;
use buddy_switch_core::modules::proxy;
use buddy_switch_gateway::{AccountStrategy, GatewayConfig, GatewayStatusView};

use crate::gateway;
use crate::trae_gateway;

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
/// **保留 `variant` 参数**，不参与区域折叠（详见 `docs/redesign/P5-ARCH.md` §2.3 的报告）。
fn trae_variant_of_region(region: TraeRegion) -> trae::variant::TraeVariant {
    // 显式穷举，不用 `unwrap_or_default()`：回落分支在这里是不可达的（区域只有两个取值），
    // 显式匹配让「区域 → 主程序」这条约定在代码里可读，也避免引入 fail-open 形态。
    match region {
        TraeRegion::Cn => trae::variant::TraeVariant::TraeWork,
        TraeRegion::Global => trae::variant::TraeVariant::Global,
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
// 命名与语义见 `docs/redesign/P5-ARCH.md` §2.3。本组只覆盖**区域纯且 arity 同构**的命令
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
            tauri::async_runtime::spawn_blocking(move || trae::handlers::accounts_overview_for(variant))
                .await
                .map_err(|error| format!("读取 Trae 账号总览失败: {error}"))
        }
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
        Product::Workbuddy => {
            account::set_account_remark_for(require_wb_region(scope)?, &account_id, remark.as_deref())
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            // Trae 侧「缺 remark ⇒ 清空」：与 `trae_set_account_remark` 的契约一致（空串即清空）。
            let remark = remark.unwrap_or_default();
            trae::handlers::set_remark_for(variant, &account_id, Some(remark.as_str()))
        }
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
        Product::Trae => Ok(trae::account::trash_list_value_for_region(require_trae_region(
            scope,
        )?)),
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
        Product::Workbuddy => export_import::export_accounts_for(require_wb_region(scope)?, &account_ids)
            .map(|records| json!({ "ok": true, "accounts": records })),
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::export_accounts_for(variant, &account_ids)
        }
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
        Product::Workbuddy => {
            export_import::export_accounts_to_path_for(require_wb_region(scope)?, &account_ids, &path)
                .map(|path| json!({ "ok": true, "path": path }))
        }
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::export_accounts_to_path_for(variant, &account_ids, &path)
        }
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
            let result = export_import::import_accounts_for(require_wb_region(scope)?, &file_text, &indexes)?;
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
#[tauri::command(rename_all = "camelCase")]
pub fn open_data_dir(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    match scope.product() {
        Product::Workbuddy => capability::open_workbuddy_data_dir(require_wb_region(scope)?),
        Product::Trae => {
            let variant = trae_variant_of_region(require_trae_region(scope)?);
            trae::handlers::open_data_dir(variant)
        }
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
pub fn preview_import_accounts(file_text: String, _region: Option<String>) -> Result<Value, String> {
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
    let selection: backup::BackupSelection = serde_json::from_value(selection)
        .map_err(|error| format!("导出参数不合法：{error}"))?;
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
#[tauri::command(rename_all = "camelCase")]
pub fn import_backup(file_text: String, policy: String, selection: Value) -> Result<Value, String> {
    let policy = export_import::ConflictPolicy::parse(&policy)?;
    let selection: backup::BackupSelection = serde_json::from_value(selection)
        .map_err(|error| format!("导入参数不合法：{error}"))?;
    backup::import_backup(&file_text, policy, &selection)
}

/// P7 T02：列出自动备份 / 导入前快照（最新在前）。
#[tauri::command]
pub fn list_auto_backups() -> Result<Value, String> {
    backup::list_auto_backups()
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
pub fn open_accounts_dir(region: Option<String>) -> Result<Value, String> {
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
/// `async` + `spawn_blocking`：读账本文件是磁盘 IO，不跑在 Tauri 主线程上（与 `list_sessions`
/// 同款处理）。
#[tauri::command]
pub async fn list_session_replicas(region: Option<String>) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || session::list_replica_groups_for(region))
        .await
        .map_err(|error| format!("读取会话副本失败: {error}"))
}

/// POST /api/sessions/fingerprints —— **会话内容指纹**（P8「组状态派生」的数据源）。
///
/// 返回 `{ [sessionId]: SessionFingerprint | null }`；取不到的为 `null`（**不臆造**）。
///
/// 用途：算两副本的**公共前缀**（= 共同旧版）与各自的增量 —— 这是**内容级**判据。
/// ⚠️ **不许用 `updatedAt` 冒充「落后 / 最新」**（`P8-ARCH.md` §1.2：「时间新 ≠ 内容新」）。
///
/// `async` + `spawn_blocking`：读 jsonl 是磁盘 IO，不跑在 Tauri 主线程上（与 `list_sessions` 同款）。
#[tauri::command(rename_all = "camelCase")]
pub async fn session_fingerprints(
    session_ids: Vec<String>,
    region: Option<String>,
) -> Result<Value, String> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)?;
    let region = parse_region(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = serde_json::Map::new();
        for cid in session_ids {
            let fingerprint = session::session_fingerprint_for(region, &cid);
            out.insert(
                cid,
                serde_json::to_value(fingerprint).unwrap_or(Value::Null),
            );
        }
        Value::Object(out)
    })
    .await
    .map_err(|error| format!("读取会话指纹失败: {error}"))
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
        Ok(session::copy_sessions_for_switch_cross(source_region, region, &target, &session_ids)
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

        let target = account::find_account_for(region, &target_account_id)
            .ok_or("目标账号不存在")?;
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

        migrate::migrate_account_data_cross(
            source_region,
            &source_uid,
            region,
            &target_uid,
            scope,
        )
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
pub async fn get_checkin_status(account_id: String, region: Option<String>) -> Result<Value, String> {
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
pub async fn get_credit_expiry(account_id: String, region: Option<String>) -> Result<Value, String> {
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
pub async fn get_token_statistics(days: Option<i64>, region: Option<String>) -> Result<Value, String> {
    let filter = parse_region_filter(region.as_deref());
    tauri::async_runtime::spawn_blocking(move || token_stats::get_statistics_for_filter(filter, days))
        .await
        .map_err(|error| format!("扫描 Token 统计失败: {error}"))
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
    checkin::run_checkin_all_for(region).await
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
        return _app
            .autolaunch()
            .is_enabled()
            .map_err(|e| format!("查询开机自启状态失败：{e}"));
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
        assert_eq!(
            parse_trae_variant(Some("trae_work")),
            TraeVariant::TraeWork
        );
        assert_eq!(parse_trae_variant(Some("trae_cn")), TraeVariant::Trae);
    }

    #[test]
    fn parses_case_and_whitespace_insensitively() {
        // `TraeVariant::parse` 内部会 `trim + to_ascii_lowercase`。
        assert_eq!(
            parse_trae_variant(Some("TRAE_WORK")),
            TraeVariant::TraeWork
        );
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
pub fn open_workbuddy_data_dir(region: Option<String>) -> Result<Value, String> {
    capability::open_workbuddy_data_dir(parse_region(region.as_deref()))
}

/// GET /api/gateway/keys —— Key 列表（脱敏）。
#[tauri::command]
pub fn list_api_keys() -> Value {
    let state = gateway::shared_state();
    let keys: Vec<Value> = state.keys.list().iter().map(|record| record.masked()).collect();
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

/// POST /api/gateway/keys/delete —— 删除已吊销 Key。
#[tauri::command]
pub fn delete_api_key(id: String) -> Result<Value, String> {
    let state = gateway::shared_state();
    state.keys.delete(&id)?;
    Ok(json!({ "ok": true }))
}

/// GET /api/gateway/models —— 模型列表 + 来源（按 region）。
#[tauri::command]
pub fn get_gateway_models(region: Option<String>) -> Value {
    let region = parse_region(region.as_deref());
    let state = gateway::shared_state();
    let snapshot = state.catalogs.current(region);
    serde_json::to_value(snapshot).unwrap_or(Value::Null)
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
pub async fn save_account_strategy(region: Option<String>, strategy: Value) -> Result<Value, String> {
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

/// GET /api/gateway/disabled-models —— 某 region 被用户禁用的模型 id 列表（P11）。
///
/// ## 为什么这条命令**不区分** WB / Trae 网关
///
/// 禁用存储是**进程内唯一**实例（[`buddy_switch_gateway::disabled_models::shared`]），
/// WB 网关（`/v1/models` + `/v1/chat/completions`）、Trae 网关与命令面读的是**同一个 `Arc`**。
/// 因此这里读到的就是两个出口此刻真正在用的那份 —— 不是「命令写一份、网关读另一份」的假开关。
///
/// 形状 `{ "region": …, "ids": [ … ] }`：`ids` 是该 region 被禁用的模型 id 集合
/// （`BTreeSet` ⇒ 稳定有序，前端可直接比对）。
#[tauri::command]
pub async fn get_disabled_models(region: Option<String>) -> Value {
    let region = parse_region(region.as_deref());
    let store = buddy_switch_gateway::disabled_models::shared();
    let disabled = store.read().await;
    let ids: Vec<String> = disabled.get(region).iter().cloned().collect();
    json!({ "region": region.as_str(), "ids": ids })
}

/// POST /api/gateway/disabled-models —— 置位 / 清位某 region 的一个模型（P11）。
///
/// ## 为什么返回 `changed`
///
/// `DisabledModels::set` 是**幂等**的：重复禁用 / 重复启用不改动集合，也就**不必落盘**
/// （省一次磁盘写）。返回 `changed` 让前端能区分「真的改了」与「本来就如此」。
/// 空白 `model_id` 被 `set` 拒绝 ⇒ `changed: false`（不写脏数据）。
#[tauri::command]
pub async fn set_model_disabled(
    region: Option<String>,
    model_id: String,
    disabled: bool,
) -> Result<Value, String> {
    let region = parse_region(region.as_deref());
    let store = buddy_switch_gateway::disabled_models::shared();
    let changed = {
        let mut guard = store.write().await;
        let changed = guard.set(region, &model_id, disabled);
        // 只在**真的变了**时落盘；写失败必须回传（否则用户以为禁用了、重启后又出现）。
        if changed {
            guard.save()?;
        }
        changed
    };
    Ok(json!({
        "ok": true,
        "region": region.as_str(),
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
#[tauri::command]
pub fn get_trae_env() -> Value {
    trae::platform::env_status()
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
#[tauri::command]
pub fn get_trae_token_statistics(days: Option<i64>, scope: Option<String>) -> Value {
    let scope = scope
        .as_deref()
        .map(trae::token_stats::TraeTokenScope::parse)
        .unwrap_or_default();
    trae::handlers::token_statistics(days, scope)
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
pub fn trae_export_accounts(user_ids: Vec<String>, variant: Option<String>) -> Result<Value, String> {
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
    trae::handlers::export_accounts_to_path_for(parse_trae_variant(variant.as_deref()), &user_ids, &path)
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
    trae::handlers::import_accounts_for(parse_trae_variant(variant.as_deref()), &file_text, &indexes)
}

/// POST /api/trae/groups —— 分组操作分发（create / update / delete / move）。
#[tauri::command]
pub fn trae_group_op(action: String, params: Value, variant: Option<String>) -> Result<Value, String> {
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
pub fn trae_clear_cooldown(user_id: Option<String>, variant: Option<String>) -> Result<Value, String> {
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
pub async fn trae_save_login(
    user_id: String,
    variant: Option<String>,
) -> Result<Value, String> {
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
    tauri::async_runtime::spawn_blocking(move || trae::handlers::backup_profile_for(variant, &user_id))
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
    tauri::async_runtime::spawn_blocking(move || trae::handlers::restore_profile_for(variant, &user_id))
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
#[tauri::command]
pub fn get_trae_gateway_models(variant: Option<String>) -> Value {
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
                .get(buddy_switch_gateway::trae::region_of_variant(variant))
                .clone()
        })
        .unwrap_or_default();
    buddy_switch_gateway::trae::payload::models_response_for(variant, &disabled)
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
#[tauri::command]
pub fn get_trae_client_models(variant: Option<String>) -> Value {
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
pub fn open_trae_data_dir(variant: Option<String>) -> Result<Value, String> {
    trae::handlers::open_data_dir(parse_trae_variant(variant.as_deref()))
}

/// POST /api/trae/launch-client —— 启动**该变体**的 Trae 客户端（OAuth 的前置动作）。
///
/// 客户端从没启动过时，它的 `storage.json` 里没有 icube 设备凭证，OAuth 网页登录
/// 必然以 `dataDirMissing` 失败。这个命令让用户**一键**跨过这道前置条件
/// （动机与「启动成功 ≠ 凭证已就绪」的边界见 `handlers::launch_client_for`）。
#[tauri::command]
pub fn trae_launch_client(variant: Option<String>) -> Result<Value, String> {
    trae::handlers::launch_client_for(parse_trae_variant(variant.as_deref()))
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
    /// 2026-09-28 之前它们全是同步的（审计 `docs/perf-audit-2026-09-24.md` 的 P1-3）。
    /// 把它们改回同步会让本用例变红。
    const MUST_BE_ASYNC: &[&str] = &[
        "import_local",          // 读客户端认证文件
        "check_auth_permission", // 写 / 删探针文件
        "list_sessions",         // 读 workbuddy.db（不可读时降级扫 projects 目录）
        "get_trae_variants",     // 进程探测（Windows 起 tasklist 子进程）
        "get_trae_accounts",     // 读账号库 + 快照目录
        "get_trae_logs",         // 全量读日志文件
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
        checkin_status, credits, open_data_dir,
    };

    #[test]
    fn parameterized_account_commands_reject_unknown_scope() {
        // 未知 / 拼错 / 空一律 Err（fail-closed），且都在**落盘之前**失败（不触达任何库）。
        assert!(accounts_export("trae-glboal".to_string(), Vec::new()).is_err());
        assert!(accounts_export_to_path("nope".to_string(), Vec::new(), "x".to_string()).is_err());
        assert!(accounts_import(String::new(), String::new(), Vec::new()).is_err());
        // `open_data_dir` 是**同步**参数化命令：未知 scope 必须在**定位目录 / 起子进程之前**失败。
        assert!(open_data_dir("trae-glboal".to_string()).is_err());
        assert!(open_data_dir(String::new()).is_err());

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
