//! HTTP API 层：把 buddy-switch-core 暴露为本地 REST 接口，供 webui（浏览器）调用。
//!
//! 路由设计对应 Python 版 server.py 与桌面端 commands.rs。仅绑定 127.0.0.1，
//! token 不出本机。

use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::RawQuery;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde_json::{json, Value};

use buddy_switch_core::modules::{
    account, auth_file, backup, checkin, client_limits, client_settings, codebuddy_cli, config,
    credit_usage, credits, diagnostics, export_import, gateway_ledger, migrate, oauth, process,
    programs, qoder, refresh, region::Region, region::RegionFilter, rotate, schedule, scheduler,
    session, skill, switch, token_daily, token_stats, trae, travel, update, vscode_session,
};
use buddy_switch_gateway::unified::UnifiedGatewayConfig;
use buddy_switch_gateway::GatewayProduct;
use buddy_switch_gateway::{GatewayConfig, GatewayStatusView};

// P9 · 悬浮窗**只读**数据模型 + provider（core）；两宿主同源。
use buddy_switch_core::modules::companion as companion_state;
// P15 · 外部 Agent 会话监听（客户端 hook 驱动）—— 悬浮窗的**第二个数据源**。
use buddy_switch_core::modules::agent_watch;
// P15 · hook 安装面（写上游客户端配置）+ 「宿主也要读」的应用偏好。
use buddy_switch_core::modules::agent_hook_install;
use buddy_switch_core::modules::app_prefs;
use buddy_switch_core::modules::proxy;
// 运行日志（本工具自身，排查问题用）：读 / 清 / 开目录三件。桌面端有同名命令。
use buddy_switch_core::modules::runtime_log;

// P5 §2.5 第 (2) 条：capability **默认拒绝**。表数据只在 core（`product_spec`），本文件只查表。
// ⚠️ 本文件在 `src-tauri/src/commands.rs` 有**同款**接线（刻意不去重，见共享知识）。
use buddy_switch_core::modules::capability;
use buddy_switch_core::modules::product::{
    require_capability, require_qoder_region, require_trae_region, require_wb_region,
    resolve_scope_param, Product,
};
use buddy_switch_core::modules::qoder::region::QoderRegion;
use buddy_switch_core::modules::trae::region::TraeRegion;

/// WorkBuddy 运行状态缓存：Windows 上检测要跑 tasklist（慢），缓存几秒避免
/// 前端切 tab 频繁触发命令行导致卡顿/闪窗。缓存按 region 分别记录，避免两版串台。
#[cfg(target_os = "windows")]
static RUNNING_CACHE: Mutex<Option<(Region, Instant, bool)>> = Mutex::new(None);

fn cached_workbuddy_running(region: Region) -> bool {
    #[cfg(target_os = "windows")]
    {
        let mut cache = RUNNING_CACHE.lock().unwrap();
        if let Some((cached_region, t, v)) = cache.as_ref() {
            if *cached_region == region && t.elapsed() < Duration::from_secs(3) {
                return *v;
            }
        }
        let v = process::is_workbuddy_running_for(region);
        *cache = Some((region, Instant::now(), v));
        v
    }
    #[cfg(not(target_os = "windows"))]
    {
        process::is_workbuddy_running_for(region)
    }
}

#[derive(RustEmbed)]
#[folder = "../../dist/"]
struct Assets;

/// 切换进度缓存：webui 通过 GET /api/switch/progress 轮询。
static SWITCH_PROGRESS: Mutex<Option<String>> = Mutex::new(None);
static SWITCH_RUNNING: Mutex<bool> = Mutex::new(false);
/// 进行中的切换操作所属 region —— 供悬浮窗只读快照推导用（与 Tauri 宿主同款）。
static SWITCH_REGION: Mutex<Option<Region>> = Mutex::new(None);

/// webui 对外路由。
///
/// **merge 顺序（关键，A-1.3 / B-6 要点 12）**：先构造**不含 fallback** 的
/// `api_routes()`，再 `merge(gateway::router(state))`，**最后**才 `.fallback(...)`。
/// 否则 axum 会因 fallback 冲突直接 panic。
pub fn router() -> Router {
    let gateway_state = crate::gateway_host::shared_state();
    // ★ 安全审计 P0-1：**管理面必须有来源校验**。
    //
    // 这里 `.layer(...)` 的位置很关键：它在 `merge` **之前**，所以只作用于
    // `api_routes()`（`/api/*`）；网关那棵树有自己的 Bearer 鉴权与独立端口，
    // 不该被 webui 的 Host 策略牵连。
    //
    // 也不能挂在最外层：那会把 `static_handler`（SPA 静态资源）一起拦掉，
    // 而静态资源是公开的、且它的 GET 请求不带 `Content-Type`。
    api_routes()
        .layer(axum::middleware::from_fn(crate::api_guard::guard))
        .merge(buddy_switch_gateway::router(gateway_state))
        .fallback(static_handler)
}

/// 管理面 API 路由（**不含 fallback**）。
fn api_routes() -> Router {
    Router::new()
        .route("/api/status", get(api_status))
        .route("/api/accounts", get(api_accounts))
        // P5 T03 · 参数化命令（`scope` 决定平台与区域）。旧路由（/api/accounts、/api/account/remark、
        // /api/delete）保留为别名，故这里用**新路径**，避免与旧路由同路径冲突。
        .route("/api/accounts/list", get(api_accounts_list))
        .route("/api/accounts/remark", post(api_account_remark))
        .route("/api/accounts/delete", post(api_account_delete))
        // P5 §2.7 · 回收站 4 命令（**全新命令**，非参数化、无别名）。`scope` 决定平台与区域，
        // 从而决定读/写哪个 `*.trash.<region>.json`；非法 scope ⇒ 4xx 且**不落盘**（fail-closed）。
        .route("/api/accounts/trash", get(api_account_trash_list))
        .route("/api/accounts/trash/restore", post(api_account_restore))
        .route("/api/accounts/trash/purge", post(api_account_purge))
        .route("/api/accounts/trash/clear", post(api_account_trash_clear))
        .route("/api/accounts/export", post(api_accounts_export))
        .route(
            "/api/accounts/export-to-path",
            post(api_accounts_export_to_path),
        )
        .route("/api/accounts/import", post(api_accounts_import))
        // P5 T03 · 参数化「从本机客户端导入账号」（`scope` 决定平台与区域）。旧路由
        // `/api/import-local`（WB）与 `/api/trae/accounts/import-local`（Trae）保留为别名，
        // 故这里用**新路径**（与 `accounts_*` 家族同族）。
        .route(
            "/api/accounts/import-local",
            post(api_accounts_import_local),
        )
        // 粘贴授权凭据（issue #62）：无客户端时与「浏览器授权」并列的第二条路。
        .route(
            "/api/accounts/paste-credential",
            post(api_accounts_paste_credential),
        )
        // P5 T03 · 参数化「打开客户端数据目录」（`scope` 决定平台与区域）。旧路由
        // `/api/gateway/open-data-dir`（WB）与 `/api/trae/open-data-dir`（Trae）保留为别名。
        .route("/api/data-dir/open", post(api_open_data_dir))
        // 缓存清理（「清除缓存」）：`scan` 只读量体积，`clear` 按类别删（不可逆）。
        // 无 scope 参数 —— `~/.buddy-switch` 是全局单目录（见 core `config::store_dir`）。
        .route("/api/cache/scan", get(api_cache_scan))
        .route("/api/cache/clear", post(api_cache_clear))
        // P5 T03 · 参数化积分命令（`scope` 决定平台与区域）。旧路由 `/api/credits`（WB）与
        // `/api/trae/credits`（Trae）保留为别名，故这里用**新路径**，避免与旧路由同路径冲突。
        .route("/api/credits/query", post(api_credits_query))
        // P5 T03 · 参数化签到状态命令（`scope` 决定平台与区域）。旧路由 `GET /api/checkin/status`
        // （WB，**批量整端点**）与 `GET /api/trae/checkin/status`（Trae）保留为别名，故这里用
        // **新路径 + POST**，既避开同路径冲突，也避开与旧 GET 的 method 冲突。
        .route("/api/checkin/status/query", post(api_checkin_status_query))
        .route("/api/accounts/open-dir", post(api_open_accounts_dir))
        .route("/api/codebuddy-cli/status", get(api_codebuddy_cli_status))
        .route(
            "/api/codebuddy-cli/install-helper",
            post(api_codebuddy_cli_install_helper),
        )
        .route("/api/codebuddy-cli/switch", post(api_codebuddy_cli_switch))
        // issue #109 §PR-2：一键把 CLI 切到本机网关（建 Key + 写 settings + 关轮换）。
        .route(
            "/api/codebuddy-cli/enable-gateway",
            post(api_codebuddy_cli_enable_gateway),
        )
        // issue #109：切回直连（清网关键 + 重建上游凭据）。没有它，用户切过去就回不来。
        .route(
            "/api/codebuddy-cli/disable-gateway",
            post(api_codebuddy_cli_disable_gateway),
        )
        .route(
            "/api/codebuddy-cn-ide/status",
            get(api_codebuddy_cn_ide_status),
        )
        .route(
            "/api/codebuddy-cn-ide/switch",
            post(api_codebuddy_cn_ide_switch),
        )
        .route(
            "/api/codebuddy-cn-ide/detect",
            post(api_codebuddy_cn_ide_detect),
        )
        .route(
            "/api/vscode-codebuddy-plugin/status",
            get(api_vscode_codebuddy_plugin_status),
        )
        .route(
            "/api/vscode-codebuddy-plugin/switch",
            post(api_vscode_codebuddy_plugin_switch),
        )
        .route(
            "/api/vscode-codebuddy-plugin/detect",
            post(api_vscode_codebuddy_plugin_detect),
        )
        .route(
            "/api/jetbrains-codebuddy-plugin/status",
            get(api_jetbrains_codebuddy_plugin_status),
        )
        .route(
            "/api/jetbrains-codebuddy-plugin/switch",
            post(api_jetbrains_codebuddy_plugin_switch),
        )
        .route(
            "/api/jetbrains-codebuddy-plugin/detect",
            post(api_jetbrains_codebuddy_plugin_detect),
        )
        .route("/api/delete", post(api_delete))
        .route("/api/account/remark", post(api_set_account_remark))
        .route("/api/account/group", post(api_set_account_group))
        .route("/api/account/token-today", get(api_account_token_today))
        .route("/api/account/token-trend", get(api_gateway_token_trend))
        .route("/api/account/usage-dim", get(api_gateway_usage_dim))
        .route("/api/account/usage-reconcile", get(api_usage_reconcile))
        .route(
            "/api/switch/config",
            get(api_switch_config).post(api_save_switch_config),
        )
        .route("/api/oauth/start", post(api_oauth_start))
        .route("/api/oauth/status", post(api_oauth_status))
        .route("/api/import-local", post(api_import_local))
        .route("/api/export-accounts", post(api_export_accounts))
        .route(
            "/api/export-accounts-to-path",
            post(api_export_accounts_to_path),
        )
        .route("/api/import/preview", post(api_preview_import))
        .route("/api/import", post(api_import))
        // P7 T01 · 全局备份导出（跨 4 作用域；凭据按 CREDENTIAL_KEYS 剥离）。
        .route("/api/backup/export", post(api_export_backup))
        // P7 T02 · 全局备份导入（预览 / 导入 / 自动备份 / 恢复 / 配置）。
        .route(
            "/api/backup/import/preview",
            post(api_preview_import_backup),
        )
        .route("/api/backup/import", post(api_import_backup))
        .route("/api/backup/auto/list", get(api_list_auto_backups))
        .route("/api/backup/auto/restore", post(api_restore_auto_backup))
        .route(
            "/api/backup/auto/config",
            get(api_get_auto_backup_config).post(api_save_auto_backup_config),
        )
        .route("/api/switch", post(api_switch))
        // Q1 · 诊断包导出（只读；凭据闸门在 core 内，命中即拒绝发出）。
        .route("/api/diagnostics/export", post(api_export_diagnostic))
        // 运行日志（本工具自身，排查问题用）：读 / 清 / 开目录。
        // ⚠️ 与 `src-tauri/src/commands.rs` 的三条同名命令**一一对应**（刻意不去重）。
        .route("/api/logs/runtime", get(api_runtime_logs))
        .route("/api/logs/runtime/clear", post(api_clear_runtime_logs))
        .route("/api/logs/runtime/slow-call", post(api_report_slow_call))
        .route("/api/logs/open-dir", post(api_open_logs_dir))
        .route("/api/switch/progress", get(api_switch_progress))
        .route("/api/sessions", get(api_sessions))
        .route("/api/sessions/replicas", get(api_session_replicas))
        // P8 · 安全同步（增量追加）—— **写操作**：只往目标 jsonl 追加，绝不改写已有内容。
        .route("/api/sessions/sync", post(api_session_sync))
        .route("/api/sessions/unify", post(api_session_unify))
        .route("/api/sessions/copy", post(api_copy_sessions))
        // P14 S4a · VS Code 插件 / CodeBuddy IDE 的会话仓（只读枚举 + 加法复制）。
        .route("/api/vscode-sessions", get(api_vscode_sessions))
        .route("/api/vscode-sessions/copy", post(api_copy_vscode_sessions))
        // P14 S4b · 记录级 diff（只读）+ 增量追加。
        .route("/api/vscode-sessions/diff", get(api_diff_vscode_sessions))
        .route("/api/vscode-sessions/sync", post(api_sync_vscode_sessions))
        // 客户端限流可视化（**只读**）：账号 × 模型 × 恢复时刻。
        .route("/api/client/limits", get(api_client_limits))
        // Skill 管理（cc-switch 式）：清单 + 启停 + 跨区搬迁 + 本地导入。
        // `region` 决定读 / 写哪个 `{data_dir}/skills`（`skill::*` 两宿主同源）。
        .route("/api/skills", get(api_list_skills))
        .route("/api/skills/enabled", post(api_set_skill_enabled))
        .route("/api/skills/copy-cross", post(api_copy_skills_cross))
        .route("/api/skills/import", post(api_import_skill))
        .route("/api/skills/open-dir", post(api_open_skills_dir))
        // 上游客户端 settings.json 统一托管（读 · 编辑 · 备份 · 恢复 · 差异比对）。
        // ⚠️ 与 `src-tauri/src/commands.rs` 的 6 个同名命令**同契约**（两宿主同步）。两侧都调
        // 同一个 core `client_settings::*` 函数并原样返回其结果，本层**不自己拼 JSON 形状**。
        // ⚠️ 无 capability 门控：本模块跨 WorkBuddy / Trae 两个产品线托管**各自客户端**的本地
        // 设置文件，平台能力表（`capability.rs`）里没有与之对应的能力项，故不套用任何平台能力。
        .route("/api/client-settings/slots", get(api_client_settings_slots))
        .route("/api/client-settings/read", get(api_client_settings_read))
        .route(
            "/api/client-settings/patch",
            post(api_client_settings_patch),
        )
        .route(
            "/api/client-settings/backups",
            get(api_client_settings_backups),
        )
        .route(
            "/api/client-settings/restore",
            post(api_client_settings_restore),
        )
        .route("/api/client-settings/diff", post(api_client_settings_diff))
        .route("/api/migrate/account", post(api_migrate_account))
        .route("/api/checkin/status", get(api_checkin_status))
        .route("/api/credits", post(api_credits))
        .route("/api/credits/stats", get(api_credit_statistics))
        .route("/api/token-stats", get(api_token_statistics))
        .route("/api/checkin", post(api_checkin))
        .route("/api/checkin/all", post(api_checkin_all))
        .route(
            "/api/checkin/config",
            get(api_checkin_config).post(api_save_checkin_config),
        )
        .route("/api/checkin/logs", get(api_checkin_logs))
        .route("/api/travel/status", get(api_travel_status))
        .route(
            "/api/travel/config",
            get(api_travel_config).post(api_save_travel_config),
        )
        .route(
            "/api/rotate/config",
            get(api_rotate_config).post(api_save_rotate_config),
        )
        .route(
            "/api/schedule/config",
            get(api_schedule_config).post(api_save_schedule_config),
        )
        .route("/api/schedule/run", post(api_run_schedule_task))
        .route("/api/rotate/status", get(api_rotate_status))
        .route("/api/rotate/run", post(api_rotate_run))
        .route("/api/rotate/logs", get(api_rotate_logs))
        .route("/api/refresh-token", post(api_refresh_token))
        .route("/api/update/check", get(api_update_check))
        .route(
            "/api/update/config",
            get(api_update_config).post(api_save_update_config),
        )
        // —— 网关管理面（A-3.7）——
        .route(
            "/api/gateway/config",
            get(api_gateway_config).post(api_save_gateway_config),
        )
        .route("/api/proxy/status", get(api_proxy_status))
        .route("/api/proxy/enable", post(api_proxy_enable))
        .route("/api/proxy/disable", post(api_proxy_disable))
        .route("/api/gateway/status", get(api_gateway_status))
        // 账号池：人工解除某账号的禁用（禁用是终态且落盘，此前**没有任何出口**）。
        .route("/api/gateway/pool/revive", post(api_gateway_revive_account))
        .route(
            "/api/gateway/pool/exclusions",
            get(api_gateway_pool_exclusions).post(api_gateway_set_pool_exclusion),
        )
        // 打开 WorkBuddy 客户端数据目录（非 Windows 返回结构化 Unsupported）。
        .route(
            "/api/gateway/open-data-dir",
            post(api_gateway_open_data_dir),
        )
        .route(
            "/api/gateway/keys",
            get(api_gateway_list_keys).post(api_gateway_create_key),
        )
        .route("/api/gateway/keys/revoke", post(api_gateway_revoke_key))
        .route("/api/gateway/keys/policy", post(api_gateway_set_key_policy))
        .route("/api/gateway/keys/usage", get(api_gateway_key_usage))
        .route("/api/gateway/keys/restore", post(api_gateway_restore_key))
        .route("/api/gateway/keys/delete", post(api_gateway_delete_key))
        .route("/api/gateway/models", get(api_gateway_models))
        .route(
            "/api/gateway/models/refresh",
            post(api_gateway_refresh_models),
        )
        .route(
            "/api/gateway/strategy",
            get(api_gateway_strategy).post(api_gateway_save_strategy),
        )
        // 模型禁用偏好（P11）：GET 列表 / POST 置位·清位，同一路径两种方法（与 Tauri 命令一一对应）。
        .route(
            "/api/gateway/disabled-models",
            get(api_gateway_disabled_models).post(api_gateway_set_model_disabled),
        )
        // 图片能力声明 / 覆盖（issue #101）：同一路径两种方法，与 Tauri 命令一一对应。
        .route(
            "/api/gateway/model-image-caps",
            get(api_gateway_model_image_caps).post(api_gateway_set_model_image_cap),
        )
        // 模型降级链（P13）：读 / 存 / 预演，与三个 Tauri 命令一一对应。
        .route(
            "/api/gateway/model-fallback",
            get(api_gateway_model_fallback).post(api_save_gateway_model_fallback),
        )
        .route(
            "/api/gateway/model-fallback/preview",
            post(api_preview_gateway_model_fallback),
        )
        // 降级统计：读**落盘账本**（不是环形缓冲），「近 N 天」才成立。
        .route(
            "/api/gateway/fallback-stats",
            get(api_gateway_fallback_stats),
        )
        .route("/api/gateway/logs", get(api_gateway_logs))
        .route("/api/gateway/logs/clear", post(api_gateway_clear_logs))
        // 限流探测（账号 × 模型矩阵 · 一次性作业 + 轮询）：start 异步作业 / status 快照 / cancel。
        // 与 `src-tauri/src/commands.rs` 的 `probe_*` 命令、`src/lib/api.ts` 的 ROUTES 三方一致。
        .route("/api/gateway/probe/start", post(api_probe_start))
        .route("/api/gateway/probe/status", get(api_probe_status))
        .route("/api/gateway/probe/cancel", post(api_probe_cancel))
        .route("/api/gateway/probe/ledger", get(api_probe_ledger))
        // ---- 统一网关（第三个产品线 · 跨作用域）----
        // ⚠️ 前缀 `/api/unified-gateway/…` 与两条既有线路（`/api/gateway/…`、`/api/trae/…`）平行；
        //   命令与其在 `src-tauri/commands.rs` 的实现一一对应（门禁 `check:api` 两侧夹）。
        .route(
            "/api/unified-gateway/config",
            get(api_unified_gateway_config).post(api_save_unified_gateway_config),
        )
        .route(
            "/api/unified-gateway/models",
            get(api_unified_gateway_models),
        )
        .route(
            "/api/unified-gateway/pool-overview",
            get(api_unified_gateway_pool_overview),
        )
        .route(
            "/api/unified-gateway/keys",
            get(api_unified_gateway_list_keys).post(api_unified_gateway_create_key),
        )
        .route(
            "/api/unified-gateway/keys/revoke",
            post(api_unified_gateway_revoke_key),
        )
        // ★ issue #76：策略更新 + 按密钥用量（与另三条产品线同形）。
        .route(
            "/api/unified-gateway/keys/policy",
            post(api_unified_gateway_set_key_policy),
        )
        .route(
            "/api/unified-gateway/keys/usage",
            get(api_unified_gateway_key_usage),
        )
        // ★ issue #76：**批量**用量（密钥表每行一列，避免 N 次往返）。
        .route(
            "/api/unified-gateway/keys/usage-all",
            get(api_unified_gateway_key_usage_all),
        )
        .route(
            "/api/unified-gateway/keys/restore",
            post(api_unified_gateway_restore_key),
        )
        .route(
            "/api/unified-gateway/keys/delete",
            post(api_unified_gateway_delete_key),
        )
        // ---- Trae 模块（与 src-tauri commands.rs 的 trae 命令一一对应）----
        .route("/api/trae/env", get(api_trae_env))
        .route("/api/trae/variants", get(api_trae_variants))
        .route("/api/trae/capabilities", get(api_trae_capabilities))
        .route("/api/trae/accounts", get(api_trae_accounts))
        .route("/api/trae/accounts/add", post(api_trae_add_account))
        .route("/api/trae/accounts/update", post(api_trae_update_account))
        .route(
            "/api/trae/accounts/remark",
            post(api_trae_set_account_remark),
        )
        .route("/api/trae/accounts/delete", post(api_trae_delete_account))
        // ---- 账号迁移（与 WorkBuddy 的 /api/import-local、/api/export-accounts* 同构）----
        .route(
            "/api/trae/accounts/import-local",
            post(api_trae_import_local),
        )
        // ---- Trae OAuth 登录（对齐 WorkBuddy 的 /api/oauth/*）----
        .route("/api/trae/oauth/start", post(api_trae_oauth_start))
        // 手工提交回调链接（issue #62）：远程部署时浏览器在另一台机器，回调打不回 127.0.0.1。
        .route(
            "/api/trae/oauth/submit-callback",
            post(api_trae_oauth_submit_callback),
        )
        .route("/api/trae/oauth/status", post(api_trae_oauth_status))
        .route("/api/trae/oauth/cancel", post(api_trae_oauth_cancel))
        .route("/api/trae/accounts/export", post(api_trae_export_accounts))
        .route(
            "/api/trae/accounts/export-to-path",
            post(api_trae_export_accounts_to_path),
        )
        .route(
            "/api/trae/accounts/import/preview",
            post(api_trae_preview_import),
        )
        .route("/api/trae/accounts/import", post(api_trae_import_accounts))
        .route("/api/trae/groups", post(api_trae_group_op))
        .route("/api/trae/checkin/status", get(api_trae_checkin_status))
        // issue #47：逐账号「今日」只读重取（与 WB 的同名命令同构）。
        .route(
            "/api/trae/checkin/status-map",
            get(api_trae_checkin_status_map),
        )
        .route(
            "/api/trae/account/token-today",
            get(api_trae_account_token_today),
        )
        .route("/api/trae/checkin", post(api_trae_checkin))
        .route("/api/trae/credits", get(api_trae_credits))
        .route("/api/trae/token-stats", get(api_trae_token_statistics))
        // Trae 客户端账本（导入用户自己导出的明文库；不内置解密）。
        .route("/api/trae/client-import/probe", get(api_trae_client_probe))
        .route(
            "/api/trae/client-import/config",
            get(api_trae_client_config),
        )
        .route(
            "/api/trae/client-import/path",
            post(api_trae_save_client_path),
        )
        // 内置解密：口令由用户输入，工具内部完成「加密库 → 明文副本」。
        .route(
            "/api/trae/client-import/unlock",
            post(api_trae_unlock_client_db),
        )
        .route("/api/trae/client-usage", get(api_trae_client_usage))
        .route("/api/trae/logs", get(api_trae_logs))
        .route("/api/trae/credits/refresh", post(api_trae_refresh_credits))
        .route("/api/trae/refresh-jwt", post(api_trae_refresh_jwt))
        .route("/api/trae/cooldown/clear", post(api_trae_clear_cooldown))
        .route("/api/trae/legacy-merge", post(api_trae_legacy_merge))
        .route("/api/trae/profiles", get(api_trae_profiles))
        .route("/api/trae/login/save", post(api_trae_save_login))
        .route("/api/trae/profiles/backup", post(api_trae_backup_profile))
        .route("/api/trae/profiles/restore", post(api_trae_restore_profile))
        .route("/api/trae/profiles/delete", post(api_trae_delete_profile))
        .route("/api/trae/switch", post(api_trae_switch))
        // ---- Qoder（第三个平台）----
        // ⚠️ path + method 必须与 `src/lib/api.ts` 的 ROUTES、以及
        // `src-tauri/src/lib.rs` 的 invoke_handler 三方一致（门禁 check-api-contract.cjs）。
        .route("/api/qoder/accounts", get(api_qoder_accounts))
        .route("/api/qoder/local-sources", get(api_qoder_local_sources))
        .route(
            "/api/qoder/accounts/import-local",
            post(api_qoder_import_local),
        )
        .route("/api/qoder/accounts/import-pat", post(api_qoder_import_pat))
        .route(
            "/api/qoder/accounts/remark",
            post(api_qoder_set_account_remark),
        )
        .route("/api/qoder/accounts/delete", post(api_qoder_delete_account))
        .route(
            "/api/qoder/accounts/refresh",
            post(api_qoder_refresh_account),
        )
        .route("/api/qoder/checkin/status", get(api_qoder_checkin_status))
        // issue #47：逐账号「今日」只读重取（与 WB 的同名命令同构）。
        .route(
            "/api/qoder/checkin/status-map",
            get(api_qoder_checkin_status_map),
        )
        .route("/api/qoder/checkin/claim", post(api_qoder_checkin_claim))
        // 一次性 Pro 升级包（+1800）：资格查询（只读）/ 领取（幂等）。
        .route("/api/qoder/pro-pack/status", get(api_qoder_pro_pack_status))
        .route("/api/qoder/pro-pack/claim", post(api_qoder_pro_pack_claim))
        .route("/api/qoder/credits", get(api_qoder_credits))
        // OAuth 设备授权（PKCE S256）：发起 + 轮询。与 WB 的 `/api/oauth/*` 同形，
        // 但**按 scope 分家**（Qoder 的双区域各有自己的授权页与 client_id）。
        .route("/api/qoder/oauth/start", post(api_qoder_oauth_start))
        .route("/api/qoder/oauth/status", post(api_qoder_oauth_status))
        // 程序位 / 切换 / 昵称 / 分组 / 用量（2026-10-15 补齐）
        .route("/api/qoder/programs", get(api_qoder_program_status))
        .route("/api/qoder/accounts/switch", post(api_qoder_switch_account))
        .route(
            "/api/qoder/accounts/group",
            post(api_qoder_set_account_group),
        )
        .route("/api/qoder/usage", get(api_qoder_usage))
        // ---- Qoder 网页端逐资源包明细（issue #111）----
        // ★ 三条都**只对桌面端成立**：逐包数据在网页端私有接口，只认浏览器登录后的
        //   会话 Cookie，而只有 Tauri 的 `Webview::cookies()` 能读到它。本进程是
        //   headless 服务端（Docker / npm CLI / 裸二进制），没有 `Webview` ⇒ 原理上做不到。
        //   ⇒ 结构化拒绝，**不返回空包列表**（那会被读成「该账号没有资源包」，与事实相反）。
        .route(
            "/api/qoder/web/login-open",
            post(api_qoder_web_desktop_only),
        )
        .route(
            "/api/qoder/web/login-close",
            post(api_qoder_web_desktop_only),
        )
        .route(
            "/api/qoder/web/credit-packs/probe",
            post(api_qoder_web_desktop_only),
        )
        .route(
            "/api/qoder/web/credit-packs",
            get(api_qoder_web_desktop_only),
        )
        .route(
            "/api/qoder/web/session/status",
            get(api_qoder_web_desktop_only),
        )
        .route(
            "/api/qoder/web/session/window-cookies",
            get(api_qoder_web_desktop_only),
        )
        .route(
            "/api/qoder/web/session/logout",
            post(api_qoder_web_desktop_only),
        )
        // ---- Qoder 逐包出站代理（issue #111 收尾）----
        // ★ 这是**纯配置**，不是「仅桌面端可用」：headless 服务端只要有会话文件同样要取
        //   逐包，代理通道（国际版按 IP 判地区）对它是同等需要的能力 ⇒ 真实读写。
        .route(
            "/api/qoder/web/proxy",
            get(api_qoder_web_proxy_get).post(api_qoder_web_proxy_set),
        )
        // ---- Qoder API 网关（管理面；网关本体走独立端口 57895，**不** merge 进本 Router）----
        // ⚠️ 一个监听、两个区域：区域由请求所用 API Key 的归属决定（`QoderKeyRegion`），
        //   状态 / 模型两条按 `scope` 看对应的那本账号库（fail-closed）。
        .route(
            "/api/qoder/gateway/config",
            get(api_qoder_gateway_config).post(api_save_qoder_gateway_config),
        )
        .route("/api/qoder/gateway/status", get(api_qoder_gateway_status))
        // 限额护栏：逐账号当日用量 / 拦截原因 / 余额快照（只读）。
        .route("/api/qoder/gateway/quota", get(api_qoder_gateway_quota))
        .route(
            "/api/qoder/gateway/pool/exclusions",
            post(api_set_qoder_pool_exclusion),
        )
        .route("/api/qoder/gateway/models", get(api_qoder_gateway_models))
        // 多 Key 管理（含归属区域）：GET 列表 / POST 创建，同一路径两种方法。
        .route(
            "/api/qoder/gateway/keys",
            get(api_qoder_list_api_keys).post(api_qoder_create_api_key),
        )
        .route(
            "/api/qoder/gateway/keys/revoke",
            post(api_qoder_revoke_api_key),
        )
        // ★ issue #76：策略更新 + 按密钥用量（与 WB / Trae 侧同形）。
        .route(
            "/api/qoder/gateway/keys/policy",
            post(api_qoder_set_key_policy),
        )
        .route("/api/qoder/gateway/keys/usage", get(api_qoder_key_usage))
        .route(
            "/api/qoder/gateway/keys/restore",
            post(api_qoder_restore_api_key),
        )
        .route(
            "/api/qoder/gateway/keys/delete",
            post(api_qoder_delete_api_key),
        )
        .route("/api/qoder/gateway/logs", get(api_qoder_gateway_logs))
        .route(
            "/api/qoder/gateway/logs/clear",
            post(api_qoder_clear_gateway_logs),
        )
        // 网关账本（用量页「账本」视图，issue #63 的平台补齐）。
        .route(
            "/api/qoder/gateway/usage-dim",
            get(api_qoder_gateway_usage_dim),
        )
        .route("/api/trae/device/reset", post(api_trae_reset_device))
        .route(
            "/api/trae/settings",
            get(api_trae_settings).post(api_trae_save_settings),
        )
        // ---- Trae API 网关（管理面；网关本体走独立端口 57894，**不** merge 进本 Router）----
        .route(
            "/api/trae/gateway/config",
            get(api_trae_gateway_config).post(api_save_trae_gateway_config),
        )
        .route("/api/trae/gateway/status", get(api_trae_gateway_status))
        .route(
            "/api/trae/gateway/pool/exclusions",
            post(api_set_trae_pool_exclusion),
        )
        .route("/api/trae/gateway/models", get(api_trae_gateway_models))
        // 客户端（上游下发）的模型清单：与上面那条静态清单不同，它读客户端
        // `state.vscdb` 的缓存，因此随客户端刷新而变（issue #4）。
        .route(
            "/api/trae/gateway/client-models",
            get(api_trae_client_models),
        )
        // 多 Key 管理（含归属产品线）：GET 列表 / POST 创建，同一路径两种方法。
        .route(
            "/api/trae/gateway/keys",
            get(api_trae_list_api_keys).post(api_trae_create_api_key),
        )
        .route(
            "/api/trae/gateway/keys/revoke",
            post(api_trae_revoke_api_key),
        )
        // ★ issue #76：策略更新 + 按密钥用量（与 WB 侧同形）。
        .route(
            "/api/trae/gateway/keys/policy",
            post(api_trae_set_key_policy),
        )
        .route("/api/trae/gateway/keys/usage", get(api_trae_key_usage))
        // 网关账本（用量页「账本」视图，issue #63 的平台补齐）。
        .route(
            "/api/trae/gateway/usage-dim",
            get(api_trae_gateway_usage_dim),
        )
        .route(
            "/api/trae/gateway/keys/restore",
            post(api_trae_restore_api_key),
        )
        .route(
            "/api/trae/gateway/keys/delete",
            post(api_trae_delete_api_key),
        )
        // 打开 Trae 数据目录（非 Windows 返回结构化 Unsupported）。
        .route("/api/trae/open-data-dir", post(api_trae_open_data_dir))
        .route("/api/trae/launch-client", post(api_trae_launch_client))
        .route("/api/trae/gateway/logs", get(api_trae_gateway_logs))
        .route(
            "/api/trae/gateway/logs/clear",
            post(api_trae_clear_gateway_logs),
        )
        // ---- 桌面悬浮窗（P9 · 只读数据层）----
        // 与 `src-tauri/src/commands.rs` 的 `companion_*` 一一对应（两宿主同步）。
        // 悬浮窗本体是**桌面端**的独立窗口；webui 无此窗口 ⇒ 显隐 / 聚焦返回结构化
        // 「仅桌面端可用」，而只读快照两宿主同源。
        .route("/api/companion/status", get(api_companion_status))
        .route("/api/companion/visibility", post(api_companion_set_visible))
        .route("/api/companion/toggle", post(api_companion_toggle))
        .route("/api/companion/jump", post(api_companion_jump))
        // ---- 外部 Agent 会话监听（P15 · hook 安装面）----
        // 与 `src-tauri/src/commands.rs` 的 `get_agent_hook_status` / `set_agent_hook_enabled`
        // 一一对应（两宿主同步）。webui 的二进制**也有** `hook` 子命令
        //（见 `crates/buddy-switch-server/src/main.rs` 的 `"hook"` 分支）⇒ 它同样是合法的
        // hook 目标，装出来的命令指向 `buddy-switch` 而不是桌面 exe。
        .route("/api/agent-hooks/status", get(api_agent_hook_status))
        .route("/api/agent-hooks/enabled", post(api_agent_hook_set_enabled))
}

fn json_ok(v: Value) -> Response {
    Json(v).into_response()
}

fn json_err(e: String, code: StatusCode) -> Response {
    (code, Json(json!({ "ok": false, "error": e }))).into_response()
}

// ---------------------------------------------------------------------------
// 状态 / 账号
// ---------------------------------------------------------------------------

/// 从 query 中读取一个参数值（URL 解码为最简实现，仅处理 `%` 之外的常规字符）。
/// 解析 GET 查询串里某个键的值（**做过 percent-decode**）。
///
/// ## 为什么必须解码（2026-10-28 修）
///
/// 前端用 `URLSearchParams` 拼 query，它会把 Windows 路径编码成
/// `D%3A%5Ctrae-plain%5Cdatabase.db`、把空格编码成 `+`。旧实现直接
/// `value.to_string()` 取原始片段 ⇒ 服务端拿到的是一条**没解码**的串，
/// 路径必然打不开，而桌面端（走 IPC，参数不经 URL）完全正常 ——
/// 这是典型的「两个通道语义分叉」。
///
/// 这个缺陷以前看不出来：既有 GET 参数全是 `region` / `days` / `scope` / 逗号 id
/// 这类枚举值，没有任何需要编码的字符。一传**文件路径**就暴露。
///
/// 解码规则与 `application/x-www-form-urlencoded` 一致：`%XX` → 字节、`+` → 空格。
/// 非法的 `%` 序列**原样保留**（宁可让路径打不开并报错，也不要静默丢字符）。
fn query_value(query: Option<&str>, name: &str) -> Option<String> {
    query.unwrap_or("").split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (decode_query_component(key) == name).then(|| decode_query_component(value))
    })
}

/// `%XX` 与 `+` 的解码（[`query_value`] 的唯一实现）。
fn decode_query_component(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|digits| u8::from_str_radix(digits, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    // 非法转义：原样收下 `%`，继续往后走（不吞字符）。
                    None => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 解析**逗号分隔**的账号 id 列表（`accountIds=a,b,c`；缺失 / 空串 ⇒ 空数组）。
///
/// ## 为什么是逗号串而不是重复键 / JSON 数组
///
/// 这两条「按账号」的读命令是 **GET**（只读、可缓存、可重试）。而 `httpCall` 的
/// `queryString` 对数组只会产出 `String(["a","b"])` = `"a,b"`，桌面端则天然拿到
/// `Vec<String>` —— 若两端各用一套形状，就会出现「浏览器好用、桌面端拿不到 id」的
/// 分叉（本仓已因同类漂移踩过 `trae_switch_account`）。
/// 因此**两个通道统一收一个逗号串**，前端只需 `ids.join(",")`。
///
/// 逐项 `trim` 并丢弃空项：`"a, ,b,"` 与 `"a,b"` 等价。
fn parse_id_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 解析 region 参数，缺省为 `cn`（保证旧行为）。
fn parse_region(value: Option<&str>) -> Region {
    value.and_then(Region::parse).unwrap_or(Region::Cn)
}

/// 解析 Trae 产品线变体参数，缺省为 [`trae::variant::TraeVariant::default`]（保证旧行为）。
///
/// 与 [`parse_region`] 同风格：**缺失即回落到默认**，不做「未知值报错」——
/// 老版本前端不带该参数、或从探测结果里回传目录名（如 `TRAE SOLO CN`）都应被接受；
/// 无法解析时等价于未传。
///
/// ★ 实际映射规则在 core 的 `trae::handlers::parse_variant_id`：
/// **区域标识（`cn` / `global`）必须落到本区域的合并版变体**，否则国际版登录会打开
/// TraeWork（SOLO 线）的授权页，用户看到「TraeWork 在所在地区不可用」。
/// 本函数只保留「可选参数 + 缺省回落」这层形态，规则**不在两条通道各写一份**
/// （``src-tauri/src/commands.rs`` 的同名录事）。
fn parse_trae_variant(value: Option<&str>) -> trae::variant::TraeVariant {
    trae::handlers::parse_variant_id(value)
}

/// 解析统计查询范围参数，缺省为 `cn`（保证旧行为）；额外支持 `"all"` 合并视图。
///
/// 复用 core 的 [`buddy_switch_core::modules::region::parse_region_filter`]，其绑定规则与
/// 本模块既有 [`parse_region`] 完全一致（`"ai"`/`"GLOBAL"` → Global、`""`/`"xx"` → Cn），
/// 并额外接受 `"all" | "*" | "合并" | "全部"`。**仅**用于 `/api/token-stats` 与
/// `/api/credits/stats` 两个统计路由。
fn parse_region_filter(value: Option<&str>) -> RegionFilter {
    buddy_switch_core::modules::region::parse_region_filter(value)
}

/// 该 region 客户端是否已安装。
///
/// 跨平台判定：解析出的应用路径（macOS bundle / Windows exe：进程→缓存→注册表→
/// 盘符扫描，最后回落默认安装路径）**真实存在**即为已安装。比 `identity::
/// installed_app_version` 更适合 Windows（后者只在 macOS 读 bundle 元数据）。
fn region_installed(region: Region) -> bool {
    auth_file::workbuddy_app_path_for(region).exists()
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

#[allow(clippy::bind_instead_of_map)]
// 闭包末尾构造 `Some(...)`，与 tauri 侧同构；改成 `map` 会让两边 diff 变大。
async fn api_status(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    // 安全红线 F：读取后校验凭据域归属；不匹配即拒绝使用该凭据（current 置空），
    // 并把结构化冲突信息交给前端渲染修复指引。
    let (auth, region_mismatch) = match auth_file::read_auth_file_checked_for(region) {
        Ok(auth) => (auth, Value::Null),
        Err(mismatch) => (None, mismatch_json(&mismatch)),
    };
    let current = auth.as_ref().and_then(|a| {
        let acct = a.get("account").cloned().unwrap_or_else(|| json!({}));
        // 展示字段先归一成「字符串或 null」再下发：认证文件里 `nickname` 可能是对象
        // （见 core `account::display_str` 的文档），裸透传会让前端整棵树崩掉
        // ⇒ 窗口一片白（issue #2）。**与 tauri 侧 `commands.rs::build_app_status` 逐字同构**。
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
    json_ok(json!({
        "running": cached_workbuddy_running(region),
        "region": region,
        "authFile": auth_file::auth_file_path_for(region).to_string_lossy(),
        "current": current,
        "appPath": auth_file::workbuddy_app_path_for(region).to_string_lossy(),
        "version": update::APP_VERSION,
        "installed": region_installed(region),
        "regionMismatch": region_mismatch,
    }))
}

/// ★ 参数化命令的 Trae 分支：把**区域**映射到该区域的**主程序**变体。
///
/// 与 `src-tauri/src/commands.rs` 的同名函数**同款实现**（刻意不去重，见共享知识）。
/// 只对**区域纯**的命令（账号列表 / 备注 / 删除）使用 —— 理由见那边的文档注释。
fn trae_variant_of_region(region: TraeRegion) -> trae::variant::TraeVariant {
    // ★ issue #28：必须给**合并版**（与 `src-tauri/src/commands.rs` 同款）。
    // 原先给 `TraeWork` / `Global` 两条老程序线，而它们的客户端已被官方合并卸载 ⇒
    // 「从本机客户端导入账号」永远报「未检测到【TraeWork CN】客户端」（issue #28 报障原文）。
    match region {
        TraeRegion::Cn => trae::variant::TraeVariant::TraeMerged,
        TraeRegion::Global => trae::variant::TraeVariant::GlobalTraeMerged,
    }
}

/// 从查询串的 `scope` 解析 Trae 变体（区域 → 主程序）；缺失 / 拼错 / 非 Trae 作用域 ⇒ `Err`。
///
/// 供 issue #47 的两条「按账号」读命令使用：P5 T03 之后新增的 Trae 命令一律收 `scope`
/// （平台 + 区域），不再收 `variant` —— 同一族命令并存两套参数形状是漂移的温床。
fn trae_variant_from_scope_query(
    query: Option<&str>,
) -> Result<trae::variant::TraeVariant, String> {
    let scope = resolve_scope_param(query_value(query, "scope").as_deref())?;
    Ok(trae_variant_of_region(require_trae_region(scope)?))
}

/// WorkBuddy 账号列表的**唯一实现**（旧 `api_accounts` 与参数化 `api_accounts_list` 共用）。
fn wb_accounts_value(region: Region) -> Value {
    json!({
        "region": region,
        "accounts": account::load_accounts_for(region)
            .iter()
            .map(|a| account::account_meta_for(region, a))
            .collect::<Vec<_>>(),
        "current": auth_file::read_auth_file_for(region)
            .and_then(|a| a.get("account").and_then(|x| x.get("uid")).and_then(|x| x.as_str()).map(String::from)),
    })
}

async fn api_accounts(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    json_ok(wb_accounts_value(region))
}

// ---------------------------------------------------------------------------
// P5 T03 · 参数化命令（`scope` 同时决定平台与区域）
// ---------------------------------------------------------------------------
//
// 与 `src-tauri/src/commands.rs` 的同名命令**一一对应**（刻意不去重）。旧路由保留为别名。
// ⚠️ `scope` 必填且 fail-closed：未知 / 空 / 缺一律 4xx（[`resolve_scope_param`]），**不落盘**。

/// GET /api/accounts/list —— 参数化账号列表（`scope` 决定平台与区域）。
///
/// 返回形状按 `scope.product()` 分叉（WB `{region, accounts, current}` / Trae 账号总览），
/// 由前端平台专属 wrapper 解读。理由见 `src-tauri/src/commands.rs::accounts_list`。
async fn api_accounts_list(RawQuery(query): RawQuery) -> Response {
    let scope = match resolve_scope_param(query_value(query.as_deref(), "scope").as_deref()) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => json_ok(wb_accounts_value(region)),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => json_ok(trae::handlers::accounts_overview_for(
                trae_variant_of_region(region),
            )),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// POST /api/accounts/remark —— 参数化设置账号备注（`scope` 决定平台与区域）。
async fn api_account_remark(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let account_id = body.get("accountId").and_then(Value::as_str).unwrap_or("");
    if account_id.trim().is_empty() {
        return json_err("缺少 accountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let remark = body.get("remark").and_then(Value::as_str);
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => match account::set_account_remark_for(region, account_id, remark) {
                Ok(meta) => json_ok(meta),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => {
                let variant = trae_variant_of_region(region);
                match trae::handlers::set_remark_for(
                    variant,
                    account_id,
                    Some(remark.unwrap_or("")),
                ) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// POST /api/accounts/delete —— 参数化删除账号（Trae 侧可选一并删除登录态快照）。
async fn api_account_delete(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let account_id = body.get("accountId").and_then(Value::as_str).unwrap_or("");
    if account_id.trim().is_empty() {
        return json_err("缺少 accountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let delete_profile = body
        .get("deleteProfile")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => match account::delete_account_for(region, account_id) {
                Ok(()) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => {
                let variant = trae_variant_of_region(region);
                let owned = account_id.to_string();
                match tokio::task::spawn_blocking(move || {
                    trae::handlers::delete_account_for(variant, &owned, delete_profile)
                })
                .await
                {
                    Ok(Ok(value)) => json_ok(value),
                    Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
                    Err(error) => json_err(
                        format!("删除账号失败: {error}"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    ),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

// ---------------------------------------------------------------------------
// P5 §2.7 · 回收站 4 命令（**全新命令**，非参数化、无别名）
// ---------------------------------------------------------------------------
//
// 与 `src-tauri/src/commands.rs` 的四条同名命令**一一对应**（刻意不去重，见共享知识）。
// `scope` 决定平台 + 区域，从而决定读 / 写哪个 `*.trash.<region>.json`；非法 scope ⇒ 4xx
// 且**不落盘**（fail-closed，§2.7 硬约束 1）。

/// GET /api/accounts/trash —— 回收站列表（`scope` 决定平台与区域）。返回 `{ scope, entries }`。
async fn api_account_trash_list(RawQuery(query): RawQuery) -> Response {
    let scope = match resolve_scope_param(query_value(query.as_deref(), "scope").as_deref()) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => json_ok(account::trash_list_value(region)),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => json_ok(trae::account::trash_list_value_for_region(region)),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => json_ok(qoder::trash::trash_list_value(region)),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
    }
}

/// POST /api/accounts/trash/restore —— 从回收站恢复账号（`id` **或** `uid` 谓词）。
async fn api_account_restore(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let key = body.get("key").and_then(Value::as_str).unwrap_or("");
    if key.trim().is_empty() {
        return json_err("缺少 key".to_string(), StatusCode::BAD_REQUEST);
    }
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => match account::restore_account_for(region, key) {
                Ok(()) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => match trae::account::restore_for_region(region, key) {
                Ok(()) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => match qoder::trash::restore_account_for(region, key) {
                Ok(_) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
    }
}

/// POST /api/accounts/trash/purge —— 永久删除（**唯一不可逆步**，前端须二次确认）。
async fn api_account_purge(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let key = body.get("key").and_then(Value::as_str).unwrap_or("");
    if key.trim().is_empty() {
        return json_err("缺少 key".to_string(), StatusCode::BAD_REQUEST);
    }
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => match account::purge_account_for(region, key) {
                Ok(()) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => match trae::account::purge_for_region(region, key) {
                Ok(()) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => match qoder::trash::purge_account_for(region, key) {
                Ok(_) => json_ok(json!({ "ok": true })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
    }
}

/// POST /api/accounts/trash/clear —— 清空回收站（**不可逆**），返回 `{ ok, removed }`。
async fn api_account_trash_clear(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let removed = match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => account::clear_trash_for(region),
            Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => trae::account::clear_trash_for_region(region),
            Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => qoder::trash::clear_trash_for(region),
            Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
        },
    };
    json_ok(json!({ "ok": true, "removed": removed }))
}

/// 从请求体读取 `key` 字段为 `Vec<String>`（非数组 / 缺省 ⇒ 空表）。
///
/// 与既有 `api_export_accounts` / `api_import` 内联的解析**同款**，抽成小工具避免三条新
/// handler 各写一遍（只做「过滤非字符串条目」，不改变既有语义）。
fn body_string_list(body: &Value, key: &str) -> Vec<String> {
    body.get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// POST /api/accounts/export —— 参数化导出账号完整记录（`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::accounts_export` **一一对应**（刻意不去重）。旧路由
/// `/api/export-accounts`（WB）与 `/api/trae/accounts/export`（Trae）保留为别名。
async fn api_accounts_export(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let account_ids = body_string_list(&body, "accountIds");
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => match export_import::export_accounts_for(region, &account_ids) {
                Ok(records) => json_ok(json!({ "ok": true, "accounts": records })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => {
                let variant = trae_variant_of_region(region);
                match trae::handlers::export_accounts_for(variant, &account_ids) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => {
                match qoder::export_import::export_accounts_for(region, Some(&account_ids)) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
    }
}

/// POST /api/accounts/export-to-path —— 参数化导出到指定路径（`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::accounts_export_to_path` 一一对应（刻意不去重）。
async fn api_accounts_export_to_path(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let account_ids = body_string_list(&body, "accountIds");
    let path = body.get("path").and_then(Value::as_str).unwrap_or("");
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => {
                match export_import::export_accounts_to_path_for(region, &account_ids, path) {
                    Ok(path) => json_ok(json!({ "ok": true, "path": path })),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => {
                let variant = trae_variant_of_region(region);
                match trae::handlers::export_accounts_to_path_for(variant, &account_ids, path) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => {
                match qoder::export_import::export_accounts_to_path_for(
                    region,
                    Some(&account_ids),
                    path,
                ) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
    }
}

/// POST /api/accounts/import —— 参数化导入账号（`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::accounts_import` 一一对应（刻意不去重）。
async fn api_accounts_import(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let text = body.get("fileText").and_then(Value::as_str).unwrap_or("");
    let indexes: Vec<usize> = body
        .get("indexes")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|x| x.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => match export_import::import_accounts_for(region, text, &indexes) {
                Ok(result) => json_ok(json!({
                    "ok": true,
                    "imported": result.imported,
                    "skipped": result.skipped,
                    "overwritten": result.overwritten,
                })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => {
                let variant = trae_variant_of_region(region);
                match trae::handlers::import_accounts_for(variant, text, &indexes) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => match require_qoder_region(scope) {
            Ok(region) => {
                match qoder::export_import::import_accounts_for(region, text, Some(&indexes), true)
                {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
    }
}

/// POST /api/accounts/import-local —— 参数化「从本机客户端登录态导入账号」
/// （`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::accounts_import_local` **一一对应**（刻意不去重）。旧路由
/// `/api/import-local`（WB）与 `/api/trae/accounts/import-local`（Trae）保留为别名。形状按
/// `scope.product()` 分叉：WB 返回 `{ok, account}`，Trae 返回 `{userId, name, accounts}`。
async fn api_accounts_import_local(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => {
            let region = match require_wb_region(scope) {
                Ok(region) => region,
                Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
            };
            match account::import_local_for(region) {
                Ok(acc) => json_ok(json!({ "ok": true, "account": acc })),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            }
        }
        Product::Trae => {
            let variant = match require_trae_region(scope) {
                Ok(region) => trae_variant_of_region(region),
                Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
            };
            match tokio::task::spawn_blocking(move || {
                trae::handlers::import_local_account_for(variant)
            })
            .await
            {
                Ok(Ok(value)) => json_ok(value),
                Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
                Err(error) => json_err(
                    format!("导入本机账号失败: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                ),
            }
        }
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// POST /api/data-dir/open —— 参数化「打开客户端数据目录」（`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::open_data_dir` **一一对应**（刻意不去重）。旧路由
/// `/api/gateway/open-data-dir`（WB）与 `/api/trae/open-data-dir`（Trae）保留为别名。
/// 两平台同形（`{ok,path}` 或结构化 `Unsupported`）。
async fn api_open_data_dir(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => match require_wb_region(scope) {
            Ok(region) => {
                match buddy_switch_core::modules::capability::open_workbuddy_data_dir(region) {
                    Ok(value) => json_ok(value),
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                }
            }
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => match trae::handlers::open_data_dir(trae_variant_of_region(region)) {
                Ok(value) => json_ok(value),
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            },
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// GET /api/cache/scan —— 只读扫描数据目录各类缓存体积（**不清任何东西**）。
///
/// 与 `src-tauri/src/commands.rs::cache_scan` **一一对应**（刻意不去重）。
///
/// `spawn_blocking`：扫描要遍历几千个目录，同步跑会占住 tokio worker
/// （与 `api_codebuddy_cli_status` 同款纪律）。
async fn api_cache_scan() -> Response {
    match tokio::task::spawn_blocking(buddy_switch_core::modules::cache::scan).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("扫描缓存失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/cache/clear —— 按类别清除缓存（**不可逆**）。
///
/// body：`{ "categories": ["session_backups", …] }`（缺失 ⇒ 空列表）。
/// 未知类别 / 空列表 ⇒ 400 且**一个字节都不删**（fail-closed 由 core 保证：
/// 先校验全部 id 再动手）。与 `src-tauri/src/commands.rs::cache_clear` 一一对应。
///
/// `spawn_blocking`：清除可能要卸载几十 GB（core 内有界并行），同步跑会占住
/// tokio worker。
async fn api_cache_clear(Json(body): Json<Value>) -> Response {
    let categories: Vec<String> = body
        .get("categories")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    match tokio::task::spawn_blocking(move || buddy_switch_core::modules::cache::clear(&categories))
        .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("清除缓存失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/credits/query —— 参数化积分查询（`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::credits` **一一对应**（刻意不去重）。旧路由 `/api/credits`
/// （WB）与 `/api/trae/credits`（Trae）保留为别名。形状按 `scope.product()` 分叉：
/// WB 分支返回单账号 `CreditExpiry`（**必须**带 `accountId`），Trae 分支返回区域级
/// `TraeCreditsOverview`（忽略 `accountId`）。
/// POST /api/accounts/paste-credential —— 粘贴授权凭据（issue #62）。
///
/// 只接 WorkBuddy：另两家各有专用入口（Qoder `paste PAT`、Trae `import JWT`），
/// 让一个「看起来通用」的端点在家家上静默失败是最坏的选择。
async fn api_accounts_paste_credential(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let text = match body.get("text").and_then(Value::as_str) {
        Some(text) => text.to_string(),
        None => return json_err("缺少 text 参数".to_string(), StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => {
            let region = match require_wb_region(scope) {
                Ok(region) => region,
                Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
            };
            match auth_file::import_account_from_text_for(region, &text) {
                Ok(collected) => match account::save_collected_account_for(region, collected) {
                    Ok(saved) => json_ok(json!({
                        "ok": true,
                        "account": account::account_meta_for(region, &saved),
                    })),
                    Err(error) => {
                        json_err(format!("写入账号库失败: {error}"), StatusCode::BAD_REQUEST)
                    }
                },
                Err(error) => json_err(error, StatusCode::BAD_REQUEST),
            }
        }
        Product::Trae => json_err(
            "Trae 请用「粘贴 JWT」或「浏览器授权」——换 token 需要客户端里的设备私钥".to_string(),
            StatusCode::BAD_REQUEST,
        ),
        Product::Qoder => json_err(
            "Qoder 请用「粘贴 PAT」".to_string(),
            StatusCode::BAD_REQUEST,
        ),
    }
}

/// POST /api/trae/oauth/submit-callback —— 手工提交回调链接（issue #62）。
async fn api_trae_oauth_submit_callback(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Trae, capability::Capability::Oauth) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let login_id = match body.get("loginId").and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => value.to_string(),
        _ => return json_err("缺少 loginId".to_string(), StatusCode::BAD_REQUEST),
    };
    let callback_url = match body
        .get("callbackUrl")
        .and_then(Value::as_str)
        .or_else(|| body.get("url").and_then(Value::as_str))
    {
        Some(value) if !value.trim().is_empty() => value.to_string(),
        _ => return json_err("缺少 callbackUrl".to_string(), StatusCode::BAD_REQUEST),
    };
    match trae::handlers::oauth_login_submit_callback(&login_id, &callback_url).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_credits_query(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => {
            if let Err(error) =
                require_capability(Product::Workbuddy, capability::Capability::Credits)
            {
                return json_err(error, StatusCode::FORBIDDEN);
            }
            let region = match require_wb_region(scope) {
                Ok(region) => region,
                Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
            };
            let id = body.get("accountId").and_then(Value::as_str).unwrap_or("");
            let Some(acc) = account::find_account_for(region, id) else {
                return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
            };
            json_ok(credits::get_credit_expiry_for(region, &acc).await)
        }
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => json_ok(trae::handlers::credits_overview_for(
                trae_variant_of_region(region),
            )),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => qoder_scope_not_applicable(),
    }
}

/// POST /api/checkin/status/query —— 参数化签到状态查询（`scope` 决定平台与区域）。
///
/// 与 `src-tauri/src/commands.rs::checkin_status` **一一对应**（刻意不去重）。旧路由
/// `GET /api/checkin/status`（WB，**批量整端点**）与 `GET /api/trae/checkin/status`（Trae）
/// 保留为别名。形状按 `scope.product()` 分叉：WB 分支返回单账号签到状态（**必须**带
/// `accountId`），Trae 分支返回区域级摘要（忽略 `accountId`）。
///
/// ⚠️ 本 handler **不做批量** —— 它对齐的是**桌面端单账号**契约；webui 的批量语义仍由旧
/// 路由 `api_checkin_status` 承担（见其文档）。
async fn api_checkin_status_query(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match scope.product() {
        Product::Workbuddy => {
            let region = match require_wb_region(scope) {
                Ok(region) => region,
                Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
            };
            let id = body.get("accountId").and_then(Value::as_str).unwrap_or("");
            let Some(acc) = account::find_account_for(region, id) else {
                return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
            };
            json_ok(checkin::get_checkin_status_for(region, &acc).await)
        }
        Product::Trae => match require_trae_region(scope) {
            Ok(region) => json_ok(trae::handlers::checkin_status_for(trae_variant_of_region(
                region,
            ))),
            Err(error) => json_err(error, StatusCode::BAD_REQUEST),
        },
        Product::Qoder => qoder_scope_not_applicable(),
    }
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
/// 目录不存在时先创建，避免「打开失败」。
async fn api_open_accounts_dir(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let file = buddy_switch_core::modules::region::accounts_file_for(region);
    let dir = file
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| file.clone());
    if !dir.exists() {
        if let Err(error) = std::fs::create_dir_all(&dir) {
            return json_err(
                format!("创建账号库目录失败: {error}"),
                StatusCode::INTERNAL_SERVER_ERROR,
            );
        }
    }
    match reveal_dir(&dir) {
        Ok(()) => json_ok(json!({
            "ok": true,
            "region": region.as_str(),
            "dir": dir.to_string_lossy(),
        })),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// ⚠️ 与 `src-tauri/src/commands.rs` 的 `get_codebuddy_cli_status` 等**同款接线**
// （刻意不去重，见共享知识「同款实现散落两 crate」）。**表驱动**：本体是
// `programs::*("codebuddy_cli")`（core `modules/programs.rs`），新增程序位无需改本文件。

async fn api_codebuddy_cli_status() -> Response {
    match programs::status("codebuddy_cli") {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_codebuddy_cli_install_helper() -> Response {
    match programs::install_helper("codebuddy_cli") {
        Ok(result) => json_ok(result),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/codebuddy-cli/enable-gateway —— 把 CLI 切到**本机网关**模式（issue #109 §PR-2）。
///
/// 与桌面宿主的同名命令**刻意各写一份**（本仓惯例）：两宿主的接线各自独立，
/// 但都只调 core 的 `codebuddy_cli::enable_gateway_mode` + `rotate::disable_auto_rotate`，
/// 判定逻辑不重复。
async fn api_codebuddy_cli_enable_gateway(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let state = crate::gateway_host::shared_state();
    let port = state.config_snapshot().await.port;
    let base_url = format!("http://127.0.0.1:{port}");
    let (record, plaintext) = state
        .keys
        .create("CodeBuddy CLI（网关）".to_string(), region);
    match codebuddy_cli::enable_gateway_mode(&base_url, &plaintext, model) {
        Ok(detail) => {
            let rotate_disabled = rotate::disable_auto_rotate().is_ok();
            json_ok(json!({
                "ok": true,
                "baseUrl": base_url,
                "keyPrefix": record.prefix,
                "key": record.masked(),
                "rotateDisabled": rotate_disabled,
                "detail": detail,
                "message": "已切到本机网关模式：重启终端与 CodeBuddy CLI 后生效（env 不支持热重载）",
            }))
        }
        Err(error) => {
            // 写失败 ⇒ 吊销刚建的 Key，不留一把没人用的凭据。
            let _ = state.keys.revoke(&record.id);
            json_err(error, StatusCode::BAD_REQUEST)
        }
    }
}

/// POST /api/codebuddy-cli/disable-gateway —— 从「走本机网关」切回直连（issue #109）。
///
/// 与桌面宿主同名命令**刻意各写一份**（本仓惯例），但逻辑一致：先清网关键、再重建凭据。
async fn api_codebuddy_cli_disable_gateway() -> Response {
    let cleared = match codebuddy_cli::disable_gateway_mode() {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    /* `models.json` 里仍有网关地址 ⇒ 本工具没写过那个文件，停下让用户处理。 */
    if let Some(warning) = cleared.get("warning").and_then(Value::as_str) {
        return json_err(warning.to_string(), StatusCode::BAD_REQUEST);
    }

    let restored = tokio::task::spawn_blocking(|| programs::install_helper("codebuddy_cli"))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);

    let rotate_enabled = config::load_auto_rotate_config()
        .get("enabled")
        .and_then(Value::as_bool)
        == Some(true);
    match restored {
        Ok(_) => json_ok(json!({
            "ok": true,
            "changed": cleared.get("changed").cloned().unwrap_or(json!(false)),
            "restored": true,
            "rotateDisabled": !rotate_enabled,
            "message": if rotate_enabled {
                "已切回直连上游：网关配置已清理，直连凭据已重建；重启终端与 CLI 后生效"
            } else {
                "已切回直连上游：网关配置已清理，直连凭据已重建。⚠ 自动轮换仍是关闭的 —— 需要的话在「签到与积分」里开启"
            },
        })),
        Err(error) => json_err(
            format!("网关配置已清理，但重建直连凭据失败：{error}"),
            StatusCode::BAD_REQUEST,
        ),
    }
}

async fn api_codebuddy_cli_switch(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    match programs::switch("codebuddy_cli", id, None) {
        Ok(result) => json_ok(result),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_codebuddy_cn_ide_status() -> Response {
    match programs::status("codebuddy_cn_ide") {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_codebuddy_cn_ide_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    match programs::switch("codebuddy_cn_ide", account_id, Some(restart)) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_codebuddy_cn_ide_detect() -> Response {
    match programs::detect("codebuddy_cn_ide") {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_vscode_codebuddy_plugin_status() -> Response {
    match programs::status("vscode_codebuddy_plugin") {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_vscode_codebuddy_plugin_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    match programs::switch("vscode_codebuddy_plugin", account_id, Some(restart)) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_vscode_codebuddy_plugin_detect() -> Response {
    match programs::detect("vscode_codebuddy_plugin") {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_jetbrains_codebuddy_plugin_status() -> Response {
    match programs::status("jetbrains_codebuddy_plugin") {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_jetbrains_codebuddy_plugin_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    match programs::switch("jetbrains_codebuddy_plugin", account_id, Some(restart)) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_jetbrains_codebuddy_plugin_detect() -> Response {
    match programs::detect("jetbrains_codebuddy_plugin") {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_delete(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    match account::delete_account_for(region, id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_import_local(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    match account::import_local_for(region) {
        Ok(acc) => json_ok(json!({ "ok": true, "account": acc })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/account/remark —— 设置账号备注（**字段级更新**）。
///
/// 只改 `remark` 一个键：请求体里带的是脱敏 meta，整条写回会抹掉 token。
/// `remark` 缺失或为空串都表示**清空**（core 侧统一 trim 后删键）。
async fn api_set_account_remark(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let id = body.get("accountId").and_then(Value::as_str).unwrap_or("");
    if id.trim().is_empty() {
        return json_err("缺少 accountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let remark = body.get("remark").and_then(Value::as_str);
    match account::set_account_remark_for(region, id, remark) {
        Ok(meta) => json_ok(meta),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/account/group —— 设置账号自定义分组（**字段级更新**）。
///
/// 只改 `groupName` 一个键：请求体里带的是脱敏 meta，整条写回会抹掉 token。
/// `group` 缺失或为空串都表示**移出分组**（core 侧统一 trim 后删键）。
async fn api_set_account_group(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let id = body.get("accountId").and_then(Value::as_str).unwrap_or("");
    if id.trim().is_empty() {
        return json_err("缺少 accountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let group = body.get("group").and_then(Value::as_str);
    match account::set_account_group_for(region, id, group) {
        Ok(meta) => json_ok(meta),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/account/token-today —— 逐账号「今日 Token」（网关 + 客户端两路，按 region）。
///
/// 与 `src-tauri/src/commands.rs` 的同名命令**一一对应**（刻意不去重，两宿主行为必须一致）。
///
/// ⚠️ 两条诚实边界（由 core 保证，展示层负责如实呈现）：
/// ① `force=1` 才重扫客户端 `projects` 目录下的 jsonl —— 该扫描要读 500+ 个文件（最大 ~18MB），
///    故 core 侧有「今日已扫」闸门：不带 `force` 的重复调用只读缓存，不会每次轮询重扫；
/// ② 网关那路受 `log_keep`（默认 200）截断 ⇒ 响应里的 `gatewayTruncated` 表示该数字只是**下界**，
///    展示层须写成 `≥N` 并说明，不得当作完整总量。
///    ⚠️ 判据是「**今日**部分可能被裁」而非「日志到过上限」：滚动窗口可能被昨天的流量顶满，
///    而今天的条目全在（FIFO 裁剪）—— 那种情况下今日总量是**完整**的，不标 `true`
///    （旧判据只看条数，会误报）。见 core `token_daily::gateway_log_truncated_today`。
async fn api_account_token_today(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    let force = matches!(
        query_value(query.as_deref(), "force").as_deref(),
        Some("1") | Some("true")
    );
    token_daily::record_client_today(region, force);
    token_daily::record_gateway_today(region);
    json_ok(token_daily::account_token_today(region))
}

/// GET /api/account/token-trend —— 网关**逐日**用量趋势（读 `token_daily` 90 天落库历史）。
///
/// 与 `src-tauri/src/commands.rs` 的同名命令**一一对应**（刻意不去重，两宿主行为必须一致）。
///
/// ⚠️ 只读：不扫盘、不触发记录。`days` **只含落库过的日子** ⇒ 缺日 = 未记录（App 没在跑），
/// **不是 0** —— 前端 join 窗口后按覆盖率披露，不许把未记录日画成「当天没用量」；
/// 某日 `truncated: true` ⇒ 该日只是**下界**（判据见 `token_daily::gateway_log_truncated_today`）。
async fn api_gateway_token_trend(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    json_ok(token_daily::gateway_token_trend(region))
}

/// GET /api/account/usage-dim —— 网关四维度（密钥 / 模型 / 账号 / 端点）逐日历史。
///
/// 与 `src-tauri/src/commands.rs` 的 `get_gateway_usage_dim` **一一对应**。
async fn api_gateway_usage_dim(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    json_ok(token_daily::gateway_usage_dim(region))
}

/// GET /api/trae/gateway/usage-dim —— Trae 网关账本（四维度逐日）。
///
/// 与 `src-tauri/src/commands.rs` 的 `get_trae_gateway_usage_dim` **一一对应**。
/// `scope` 取 `cn` / `global` / `unlabeled` / `all`（缺省 `all`，合并放后端）。
async fn api_trae_gateway_usage_dim(RawQuery(query): RawQuery) -> Response {
    let scope = query_value(query.as_deref(), "scope").unwrap_or_else(|| "all".to_string());
    json_ok(gateway_ledger::usage_dim(
        gateway_ledger::LedgerProduct::Trae,
        &scope,
    ))
}

/// GET /api/qoder/gateway/usage-dim —— Qoder 网关账本（四维度逐日）。
///
/// 与 `src-tauri/src/commands.rs` 的 `get_qoder_gateway_usage_dim` **一一对应**。
async fn api_qoder_gateway_usage_dim(RawQuery(query): RawQuery) -> Response {
    let scope = query_value(query.as_deref(), "scope").unwrap_or_else(|| "all".to_string());
    json_ok(gateway_ledger::usage_dim(
        gateway_ledger::LedgerProduct::Qoder,
        &scope,
    ))
}

/// GET /api/account/usage-reconcile —— 今日客户端 ↔ 网关对账。
///
/// 与 `src-tauri/src/commands.rs` 的 `get_usage_reconcile` **一一对应**。
/// ⚠️ 只读、**不落盘**（结果随网关日志窗口滚动而变）。
async fn api_usage_reconcile(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    json_ok(token_daily::reconcile_today(region))
}

/// GET /api/switch/config —— 账号切换与账号列表展示配置。
async fn api_switch_config() -> Response {
    json_ok(config::load_switch_config())
}

/// POST /api/switch/config —— 保存账号切换配置。
async fn api_save_switch_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match config::save_switch_config(submitted) {
        Ok(()) => json_ok(config::load_switch_config()),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// 导出 / 导入账号
// ---------------------------------------------------------------------------

async fn api_export_accounts(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let ids: Vec<String> = body
        .get("accountIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    match export_import::export_accounts_for(region, &ids) {
        Ok(records) => json_ok(json!({ "ok": true, "accounts": records })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_export_accounts_to_path(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let ids: Vec<String> = body
        .get("accountIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match export_import::export_accounts_to_path_for(region, &ids, &path) {
        Ok(path) => json_ok(json!({ "ok": true, "path": path })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/import/preview —— 解析导入文件并返回脱敏预览。
///
/// **无需 region**：`preview_accounts` 是纯函数，只解析请求里的文件文本，不触及任何
/// region 账号库（core 也未提供 `_for` 变体）。region 由后续 `/api/import` 决定落库位置。
async fn api_preview_import(Json(body): Json<Value>) -> Response {
    let text = body
        .get("fileText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // ★ 2026-10-15：新增**可选** `scope`。Qoder 的预览要按区域判「归属不符」（跨区条目
    //   导入时会被拒）—— 而裸 `region=cn` 分不出 `wb-cn` 与 `qoder-cn`。
    //   缺省 / 非 Qoder 作用域 ⇒ 走既有的「纯解析、不做归属诊断」分支（旧行为逐字不变）。
    if let Some(raw) = body
        .get("scope")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        match resolve_scope_param(Some(raw)) {
            Ok(scope) if scope.product() == Product::Qoder => {
                return match require_qoder_region(scope) {
                    Ok(region) => match qoder::export_import::preview_accounts(region, &text) {
                        Ok(value) => json_ok(value),
                        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                    },
                    Err(error) => json_err(error, StatusCode::BAD_REQUEST),
                };
            }
            // 非 Qoder 作用域：继续走下面的通用分支（不看 scope，保持旧行为）。
            Ok(_) => {}
            Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
        }
    }
    match export_import::preview_accounts(&text) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_import(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let text = body
        .get("fileText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let indexes: Vec<usize> = body
        .get("indexes")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    match export_import::import_accounts_for(region, &text, &indexes) {
        Ok(result) => json_ok(json!({
            "ok": true,
            "imported": result.imported,
            "skipped": result.skipped,
            "overwritten": result.overwritten,
        })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/backup/export —— 导出**全局备份** envelope（跨 4 作用域的账号 / 配置 / 网关）。
///
/// 与 `src-tauri/src/commands.rs::export_backup` **一一对应**（刻意不去重）。请求体为
/// `{ selection: { scopes, includesCredentials, appSettings } }`；`includesCredentials=false`
/// 时按 `CREDENTIAL_KEYS` 逐条剥离凭据（**备份文件里不得含凭据**）。
async fn api_export_backup(Json(body): Json<Value>) -> Response {
    // 兼容 `{ selection: {...} }` 与平铺两种载荷（与其余 handler 的容错口径一致）。
    let selection_value = body.get("selection").cloned().unwrap_or(body);
    let selection: backup::BackupSelection = match serde_json::from_value(selection_value) {
        Ok(selection) => selection,
        Err(error) => return json_err(format!("导出参数不合法：{error}"), StatusCode::BAD_REQUEST),
    };
    match backup::export_backup(&selection) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/backup/import/preview —— 导入**预览**（**只读安全阀**，§1.7）。
///
/// 与 `src-tauri/src/commands.rs::preview_import_backup` **一一对应**（刻意不去重）。
/// 请求体 `{ fileText, policy }`；`policy` 为空 ⇒ 默认保留本地（D16）。**不写盘**。
async fn api_preview_import_backup(Json(body): Json<Value>) -> Response {
    let file_text = body
        .get("fileText")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let policy = body.get("policy").and_then(Value::as_str).unwrap_or("");
    let policy = match export_import::ConflictPolicy::parse(policy) {
        Ok(policy) => policy,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match backup::preview_import_backup(&file_text, policy) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/backup/import —— 导入全局备份（先快照 → 逐 scope 合并 → 汇总）。
///
/// 请求体 `{ fileText, policy, selection }`；`selection` 缺省 ⇒ 全选（D14）。
/// ★ `onlyScopes`（C1）：非空 ⇒ **只导入这些 scope**（「只重试失败项」）；
/// 缺省 ⇒ 全量。与 `commands.rs::import_backup` 形状一一对应。
async fn api_import_backup(Json(body): Json<Value>) -> Response {
    let file_text = body
        .get("fileText")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let policy = body.get("policy").and_then(Value::as_str).unwrap_or("");
    let policy = match export_import::ConflictPolicy::parse(policy) {
        Ok(policy) => policy,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let selection_value = body.get("selection").cloned().unwrap_or_else(|| json!({}));
    let selection: backup::BackupSelection = match serde_json::from_value(selection_value) {
        Ok(selection) => selection,
        Err(error) => return json_err(format!("导入参数不合法：{error}"), StatusCode::BAD_REQUEST),
    };
    let only_scopes: Vec<String> = body
        .get("onlyScopes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    match backup::import_backup(&file_text, policy, &selection, &only_scopes) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/diagnostics/export —— 导出诊断包（Q1；只读）。
///
/// ⚠️ 与 `commands.rs::export_diagnostic` 是**同款接线**（刻意不去重），返回形状
/// 必须一致：`{ name, text }`。凭据闸门在 core（`diagnostics::export_diagnostic`），
/// 命中即 `Err` ⇒ 两端都不会发出一份含凭据的文件。
async fn api_export_diagnostic() -> Response {
    match diagnostics::export_diagnostic() {
        Ok(text) => {
            let name = format!(
                "buddy-switch-diagnostic-{}.json",
                buddy_switch_core::modules::cst::cst_datetime(config::now_ms())
                    .format("%Y%m%d-%H%M%S")
            );
            json_ok(json!({ "name": name, "text": text }))
        }
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/logs/runtime —— 读取本工具**自身**的运行日志。
///
/// 参数：`level`（最低级别）/ `date`（`YYYY-MM-DD`）/ `keyword` / `limit`，全部可选。
/// 过滤与解析逻辑只在 core（`runtime_log::read`）—— 两宿主共用一份，杜绝形状漂移。
///
/// 读若干文件 + 逐行解析属于阻塞 IO，故放进 blocking 线程池。
async fn api_runtime_logs(RawQuery(query): RawQuery) -> Response {
    let read = |key: &str| query_value(query.as_deref(), key);
    let limit = read("limit").and_then(|value| value.parse::<u64>().ok());
    let params = json!({
        "level": read("level"),
        "date": read("date"),
        "keyword": read("keyword"),
        "limit": limit,
    });
    match tokio::task::spawn_blocking(move || runtime_log::read(&params)).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("读取运行日志失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/logs/runtime/clear —— 清空全部运行日志（**不可逆**，前端须二次确认）。
async fn api_clear_runtime_logs() -> Response {
    match tokio::task::spawn_blocking(runtime_log::clear).await {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("清空运行日志失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/logs/open-dir —— 在文件管理器中打开日志目录（不存在则先创建）。
///
/// ⚠️ 会真的起一个 GUI 进程（`explorer` / `open` / `xdg-open`）—— Docker 部署时
/// 容器里没有文件管理器，调用会失败并返回错误，属预期（webui 侧据此提示）。
async fn api_open_logs_dir() -> Response {
    match tokio::task::spawn_blocking(runtime_log::open_logs_dir).await {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("打开日志目录失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/logs/runtime/slow-call —— 前端上报「慢 IPC 命令」（issue #80 诊断通道）。
///
/// 与桌面端 `commands::report_slow_call` 同形：webui 走 HTTP，慢的是整条请求链路
/// （fetch 往返 + handler），同样值得留一条。命令名是代码标识符、不含用户数据。
async fn api_report_slow_call(Json(body): Json<Value>) -> Response {
    let cmd = body
        .get("cmd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let ms = body.get("ms").and_then(Value::as_u64).unwrap_or(0);
    if cmd.is_empty() || ms == 0 {
        return json_err(
            "参数不完整（cmd / ms）".to_string(),
            StatusCode::BAD_REQUEST,
        );
    }
    let _ = tokio::task::spawn_blocking(move || {
        runtime_log::warn(
            "ipc-slow",
            format!("命令 {cmd} 耗时 {ms}ms（超过前端计时阈值）"),
        );
    })
    .await;
    json_ok(json!({ "ok": true }))
}

/// GET /api/backup/auto/list —— 列出自动备份 / 导入前快照（最新在前）。
async fn api_list_auto_backups() -> Response {
    match backup::list_auto_backups() {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/backup/auto/restore —— 从自动备份 / 导入前快照恢复（覆盖策略）。
async fn api_restore_auto_backup(Json(body): Json<Value>) -> Response {
    let file_name = body
        .get("fileName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match backup::restore_auto_backup(&file_name) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/backup/auto/config —— 读取自动备份配置（**默认开**）。
async fn api_get_auto_backup_config() -> Response {
    json_ok(config::load_auto_backup_config())
}

/// POST /api/backup/auto/config —— 保存自动备份配置（关掉后不再产生新备份）。
async fn api_save_auto_backup_config(Json(body): Json<Value>) -> Response {
    // 兼容 `{ config: {...} }` 与平铺两种载荷（与其余 handler 的容错口径一致）。
    let cfg = body.get("config").cloned().unwrap_or(body);
    match config::save_auto_backup_config(&cfg) {
        Ok(()) => json_ok(config::load_auto_backup_config()),
        Err(error) => json_err(
            format!("保存自动备份配置失败：{error}"),
            StatusCode::BAD_REQUEST,
        ),
    }
}

// ---------------------------------------------------------------------------
// OAuth 登录
// ---------------------------------------------------------------------------

async fn api_oauth_start(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::Oauth) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(body.get("region").and_then(Value::as_str));
    match oauth::oauth_start_for(region).await {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_oauth_status(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let login_id = body
        .get("loginId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    json_ok(oauth::oauth_poll_for(region, &login_id).await)
}

// ---------------------------------------------------------------------------
// 切换
// ---------------------------------------------------------------------------

async fn api_switch(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    // 会话复制的**来源**版本；缺省与目标版本相同（同版本内切换，行为零变化）。
    let source_region = match body.get("sourceRegion").and_then(Value::as_str) {
        Some(v) => parse_region(Some(v)),
        None => region,
    };
    let account_id = body
        .get("accountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if account_id.trim().is_empty() {
        return json_err("缺少 accountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    // ★ 非法条目**整包拒绝**，不静默丢弃（对照上游 `c614e1f7`）。
    //
    // 静默丢弃的后果不是报错而是**半成品**：切换照常执行、只是少复制几条会话，
    // 报告里数量对不上，用户以为全复制了。明确 400 比「看起来成功」好得多。
    // 这也让本路由与 Tauri 侧对称 —— 那边是 `Option<Vec<String>>`，
    // serde 反序列化失败即整包报错（见 `src-tauri/src/commands.rs` 的 `copy_session_ids`）。
    let copy_ids: Vec<String> = match body.get("copySessionIds") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(id) => ids.push(id.to_string()),
                    None => {
                        return json_err(
                            format!("copySessionIds 含非法条目（每条必须是字符串）: {item}"),
                            StatusCode::BAD_REQUEST,
                        )
                    }
                }
            }
            ids
        }
        Some(other) => {
            return json_err(
                format!("copySessionIds 必须是字符串数组，收到: {other}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };

    {
        let mut running = SWITCH_RUNNING.lock().unwrap();
        if *running {
            return json_err("已有切换任务进行中".to_string(), StatusCode::CONFLICT);
        }
        *running = true;
        *SWITCH_PROGRESS.lock().unwrap() = Some("开始切换账号…".to_string());
        *SWITCH_REGION.lock().unwrap() = Some(region);
    }

    let progress: switch::ProgressFn = Box::new(|msg| {
        *SWITCH_PROGRESS.lock().unwrap() = Some(msg.to_string());
    });

    let result = tokio::task::spawn_blocking(move || {
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

    match result {
        Ok(Ok(v)) => json_ok(v),
        Ok(Err(e)) => json_err(e, StatusCode::BAD_REQUEST),
        Err(e) => json_err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_switch_progress() -> Response {
    let p = SWITCH_PROGRESS.lock().unwrap().clone();
    let running = *SWITCH_RUNNING.lock().unwrap();
    json_ok(json!({ "running": running, "progress": p }))
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

async fn api_sessions(RawQuery(query): RawQuery) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    match session::current_user_uid_for(region) {
        Some(uid) => {
            let resp = session::list_sessions_with_fallback_for(region, &uid);
            json_ok(json!({
                "sessions": resp.get("sessions").cloned().unwrap_or_else(|| json!([])),
                "current": uid,
                "source": resp.get("source").cloned().unwrap_or_else(|| json!("db")),
                "warning": resp.get("warning").cloned(),
            }))
        }
        None => json_ok(json!({ "sessions": [], "current": null, "source": "empty" })),
    }
}

/// GET /api/sessions/replicas —— 只读：copy ledger 连通成「会话副本组」（按 region）。
///
/// 与 Tauri 侧 `commands::list_session_replicas` **同契约**（两宿主同步）。
/// 只回 ledger 里确有的字段（`accountUid` / `sessionId` / `region`）；
/// `title` / `updatedAt` / `hasHistory` 由前端 join 补齐（见 `session::list_replica_groups_for`）。
///
/// ★ **内容状态随本响应一起回**（`groups[].content`）：状态判据在 core
/// （`session::derive_content_status`），前端只拿结论 —— 逐行 hash 不过 IPC。
async fn api_session_replicas(RawQuery(query): RawQuery) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    json_ok(session::list_replica_groups_for(region))
}

/// POST /api/sessions/sync —— P8 **安全同步（增量追加）**：把源副本的新增行追加到落后副本。
///
/// 与 Tauri 侧 `commands::sync_session_replicas` **同契约**（两宿主同步）。
/// 批量 + 逐条结果 ⇒ 支持**部分失败**（`PLAN.md` §3.6）；判据在 core
/// （`session::sync_session_replicas_for`），本处理器只做「鉴权 + 取参数」。
///
/// ⚠️ **写操作**：会往目标会话 jsonl **追加**行 —— 只追加，**绝不改写 / 删除**目标已有内容；
/// 出现分歧则**逐条**失败且不落一个字节。
async fn api_session_sync(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    json_ok(session::sync_session_replicas_for(region, &items))
}

/// POST /api/sessions/unify —— P8 **分歧处理（覆盖式统一）**：把选定保留的那一份覆盖到其余副本。
///
/// 与 Tauri 侧 `commands::unify_session_replicas` **同契约**（两宿主同步）。
/// 批量 + 逐条结果 ⇒ 支持**部分失败**（`PLAN.md` §3.6）；判据在 core
/// （`session::unify_session_replicas_for`），本处理器只做「鉴权 + 取参数」。
///
/// ⚠️ **破坏性写操作**：目标副本原有的分歧内容会被丢弃（`PLAN.md` §3.15.2「不许静默覆盖」）
/// ⇒ 前端**必须**先二次确认并把 `lostLines` 如实告知；core 会**先备份再覆盖**，备份失败则不覆盖。
async fn api_session_unify(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    json_ok(session::unify_session_replicas_for(region, &items))
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
/// 与 Tauri 侧 `commands::list_vscode_sessions` **同契约**（两宿主同步）。
/// 查询参数：`client`（`vscode` / `codebuddy-ide`，**必填**）、`uid`（**必填**）。
///
/// ★ 与桌面版会话**不是同一套存储**（那边 `workbuddy.db` + jsonl，这边 JSON 记录仓）
/// ⇒ 单开命令，**不往 `/api/sessions` 里塞 `client` 参数**（两套形状挤一个出口会让前端处处分支）。
async fn api_vscode_sessions(RawQuery(query): RawQuery) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let client = match parse_ext_client(
        query_value(query.as_deref(), "client")
            .as_deref()
            .unwrap_or_default(),
    ) {
        Ok(client) => client,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let Some(uid) = query_value(query.as_deref(), "uid").filter(|uid| !uid.trim().is_empty())
    else {
        return json_err("缺少 uid".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(vscode_session::list_sessions_for(client, uid.trim()))
}

/// POST /api/vscode-sessions/copy —— 把插件侧会话复制给另一个账号（**加法**，绝不覆盖）。
///
/// 与 Tauri 侧 `commands::copy_vscode_sessions` **同契约**（两宿主同步）。
/// body：`{ client, sourceUid, targetUid, items: [{ workspaceHash, conversationId }] }`。
///
/// ⚠️ **数据根找不到 ⇒ 4xx**（不是「复制了 0 条」）——后者会把「客户端没装」
/// 伪装成一次成功的空操作（「失败不得谎报成功」）。
async fn api_copy_vscode_sessions(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let client = match parse_ext_client(
        body.get("client")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ) {
        Ok(client) => client,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let source_uid = body
        .get("sourceUid")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let target_uid = body
        .get("targetUid")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let items: Vec<vscode_session::CopyItem> = match body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| serde_json::from_value(item).map_err(|e| format!("复制条目格式不对: {e}")))
        .collect::<Result<_, _>>()
    {
        Ok(items) => items,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match vscode_session::copy_conversations(client, &source_uid, &target_uid, &items) {
        Ok(report) => json_ok(report),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/vscode-sessions/diff —— **记录级 diff（只读）**：这份会话能不能做增量追加。
///
/// 与 Tauri 侧 `commands::diff_vscode_sessions` **同契约**（两宿主同步）。
/// 查询参数：`client`（必填）、`sourceUid`、`targetUid`、`workspaceHash`、`conversationId`。
///
/// ★ 插件侧**不能**照搬桌面版的「按行 append」：顺序由 `index.json` 的 `messages[]`
/// **数组次序**决定 ⇒ 目标缺的是**中间**几条时追加会**打乱会话顺序**。
async fn api_diff_vscode_sessions(RawQuery(query): RawQuery) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let client = match parse_ext_client(
        query_value(query.as_deref(), "client")
            .as_deref()
            .unwrap_or_default(),
    ) {
        Ok(client) => client,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let text = |key: &str| {
        query_value(query.as_deref(), key)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    json_ok(vscode_session::diff_conversation(
        client,
        &text("sourceUid"),
        &text("targetUid"),
        &text("workspaceHash"),
        &text("conversationId"),
    ))
}

/// POST /api/vscode-sessions/sync —— **增量追加**（只有 diff 说 `appendable` 才真写）。
///
/// 与 Tauri 侧 `commands::sync_vscode_sessions` **同契约**（两宿主同步）。
/// body：`{ client, sourceUid, targetUid, workspaceHash, conversationId }`。
///
/// ★★ **不信任调用方**：core 里进来先自己重算 diff。其余状态一律**拒绝**并如实说原因
/// （`divergent` 会静默改写目标内容、`unordered` 会打乱会话顺序 —— 两者都看不出来）。
async fn api_sync_vscode_sessions(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let client = match parse_ext_client(
        body.get("client")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ) {
        Ok(client) => client,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let text = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    match vscode_session::sync_conversation(
        client,
        &text("sourceUid"),
        &text("targetUid"),
        &text("workspaceHash"),
        &text("conversationId"),
    ) {
        Ok(report) => json_ok(report),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/client/limits —— 客户端限流台账（**只读**观测：账号 × 模型 × 恢复时刻）。
///
/// 与 Tauri 侧 `commands::client_limits` **同契约**（两宿主同步）。判据在 core
/// （`client_limits::client_limits_for`），本处理器只做「鉴权 + 取参数」。
///
/// 查询参数：`region`（cn / global）、`sinceDays`（`0` = 全量；缺省 = 近 7 天）。
async fn api_client_limits(RawQuery(query): RawQuery) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
    {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    let since_days = query_value(query.as_deref(), "sinceDays").and_then(|v| v.parse::<i64>().ok());
    json_ok(client_limits::client_limits_for(region, since_days))
}

// ---- Skill 管理（cc-switch 式）------------------------------------------------
// ⚠️ 与 `src-tauri/src/commands.rs` 的 5 个同名命令**同契约**（两宿主同步）。本层只做
// 「鉴权 + 取参数 + 丢给 core `skill::*`」，**不自己拼 JSON 形状** —— 两侧都调同一个 core
// 函数，从根上杜绝「一侧透传、另一侧多包一层」的漂移（PRD §9）。
//
// 鉴权：5 条命令都读写**用户级文件**（`{data_dir}/skills`），沿用最贴近该语义的既有
// capability `SessionCopy`（同为「WorkBuddy 桌面才有的用户数据读写」，见 `capability.rs`
// 的枚举注释）。capability 表在 core（`product_spec`），本层只查表，**默认拒绝**。

/// Skill 管理 5 条命令共用的 capability 门禁（缺能力即 403，不落盘）。
fn skill_capability_denied() -> Option<Response> {
    require_capability(Product::Workbuddy, capability::Capability::SessionCopy)
        .err()
        .map(|error| json_err(error, StatusCode::FORBIDDEN))
}

/// GET /api/skills —— 列出该 region 用户级 skills 目录下的 skill 清单（**只读**）。
async fn api_list_skills(RawQuery(query): RawQuery) -> Response {
    if let Some(denied) = skill_capability_denied() {
        return denied;
    }
    let region = query_value(query.as_deref(), "region");
    match skill::list_skills(region) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/skills/enabled —— 启停一个 skill（行级改写官方原生字段）。
///
/// 请求体：`{ region?, dirName, enabled }`。
async fn api_set_skill_enabled(Json(body): Json<Value>) -> Response {
    if let Some(denied) = skill_capability_denied() {
        return denied;
    }
    let region = body
        .get("region")
        .and_then(Value::as_str)
        .map(str::to_string);
    let dir_name = body.get("dirName").and_then(Value::as_str).unwrap_or("");
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return json_err("缺少 enabled（布尔）".to_string(), StatusCode::BAD_REQUEST);
    };
    match skill::set_skill_enabled(region, dir_name, enabled) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/skills/copy-cross —— 跨 region「合并补齐」搬迁（冲突保留目标侧）。
///
/// 请求体：`{ sourceRegion, targetRegion }`（**两者都必须显式给出**）。
async fn api_copy_skills_cross(Json(body): Json<Value>) -> Response {
    if let Some(denied) = skill_capability_denied() {
        return denied;
    }
    let source = body
        .get("sourceRegion")
        .and_then(Value::as_str)
        .map(str::to_string);
    let target = body
        .get("targetRegion")
        .and_then(Value::as_str)
        .map(str::to_string);
    match skill::copy_skills_cross(source, target) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/skills/import —— 从本地目录或 zip 导入一个 skill。
///
/// 请求体：`{ region?, sourcePath }`。
async fn api_import_skill(Json(body): Json<Value>) -> Response {
    if let Some(denied) = skill_capability_denied() {
        return denied;
    }
    let region = body
        .get("region")
        .and_then(Value::as_str)
        .map(str::to_string);
    let source_path = body.get("sourcePath").and_then(Value::as_str).unwrap_or("");
    match skill::import_skill(region, source_path) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/skills/open-dir —— 在文件管理器中打开该 region 的 skills 目录。
///
/// 目录不存在时先创建（避免「打开失败」）；响应形状由 core `skill::prepare_skills_dir`
/// 统一产出，本层只负责平台相关的 `reveal_dir`。
async fn api_open_skills_dir(Json(body): Json<Value>) -> Response {
    if let Some(denied) = skill_capability_denied() {
        return denied;
    }
    let region = body
        .get("region")
        .and_then(Value::as_str)
        .map(str::to_string);
    match skill::prepare_skills_dir(region) {
        Ok((dir, payload)) => match reveal_dir(&dir) {
            Ok(()) => json_ok(payload),
            Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
        },
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// ---- 上游客户端 settings.json 统一托管 ----------------------------------------
// ⚠️ 与 `src-tauri/src/commands.rs` 的 6 个同名命令**同契约**（两宿主同步）。两侧都调同一个
// core `client_settings::*` 函数并原样返回其结果，本层**不自己拼 JSON 形状** —— 从根上杜绝
// 「一侧透传、另一侧多包一层」的漂移（PRD §9）。
//
// 无 capability 门控：本模块跨 WorkBuddy / Trae 两个产品线托管**各自客户端**的本地设置文件，
// 平台能力表里没有与之对应的能力项（不是签到 / 积分 / 会话复制之类），故不套用任何平台能力。

/// GET /api/client-settings/slots —— 列出 6 个槽位（未安装者 `installed:false` **仍在列表**）。
async fn api_client_settings_slots() -> Response {
    json_ok(client_settings::list_slots())
}

/// GET /api/client-settings/read?slot=… —— 原文 + 解析后键值 + 格式指纹。
///
/// 解析失败 ⇒ 带 `client.settings.parse_failed` 码的错误，**绝不**返回空对象；
/// 未安装（文件不存在）**不是错误**（`installed:false`，`text`/`values` 为 `null`）。
async fn api_client_settings_read(RawQuery(query): RawQuery) -> Response {
    let slot = query_value(query.as_deref(), "slot").unwrap_or_default();
    match client_settings::read_settings(&slot) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/client-settings/patch —— 键值级 patch（**格式保持**，模块内部自动备份）。
///
/// 请求体：`{ slot, patch }`，`patch` 形如 `{ "<点号路径>": <新值> }`。
async fn api_client_settings_patch(Json(body): Json<Value>) -> Response {
    let slot = body.get("slot").and_then(Value::as_str).unwrap_or("");
    let Some(patch) = body.get("patch") else {
        return json_err("缺少 patch 对象".to_string(), StatusCode::BAD_REQUEST);
    };
    match client_settings::patch_settings(slot, patch) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/client-settings/backups?slot=… —— 该槽位的历史备份点（按 id 降序）。
async fn api_client_settings_backups(RawQuery(query): RawQuery) -> Response {
    let slot = query_value(query.as_deref(), "slot").unwrap_or_default();
    match client_settings::list_backups(&slot) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/client-settings/restore —— 整文件恢复（**恢复前对当前文件再备份一次**）。
///
/// 请求体：`{ slot, backupId }`。
async fn api_client_settings_restore(Json(body): Json<Value>) -> Response {
    let slot = body.get("slot").and_then(Value::as_str).unwrap_or("");
    let backup_id = body.get("backupId").and_then(Value::as_str).unwrap_or("");
    match client_settings::restore_backup(slot, backup_id) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/client-settings/diff —— 任选两个槽位比对（仅 A 有 / 仅 B 有 / 值不同）。
///
/// 请求体：`{ slotA, slotB }`。
async fn api_client_settings_diff(Json(body): Json<Value>) -> Response {
    let slot_a = body.get("slotA").and_then(Value::as_str).unwrap_or("");
    let slot_b = body.get("slotB").and_then(Value::as_str).unwrap_or("");
    match client_settings::diff_slots(slot_a, slot_b) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_copy_sessions(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    // 会话来源版本；缺省与目标版本相同（同版本内复制，行为零变化）。
    let source_region = match body.get("sourceRegion").and_then(Value::as_str) {
        Some(v) => parse_region(Some(v)),
        None => region,
    };
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_ids: Vec<String> = body
        .get("sessionIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let Some(target) = account::find_account_for(region, &target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    // copy_sessions_for_switch_for 已返回完整报告（sourceUid / targetUid / copied /
    // skipped? / errors?）。这里必须**原样透传**，不能再包一层 "copied" —— 否则
    // 响应会变成 {copied:{copied:[...]}}，与 Tauri 通道及前端 CopyResult[] 契约不一致。
    // webui 侧没有进度事件通道，逐会话进度传 None（复制本身的时间优化不受影响）。
    match session::copy_sessions_for_switch_cross(
        source_region,
        region,
        None,
        &target,
        &session_ids,
    ) {
        Some(report) => json_ok(report),
        None => json_ok(json!({
            "sourceUid": Value::Null,
            "targetUid": target.get("uid"),
            "copied": [],
        })),
    }
}

// ---------------------------------------------------------------------------
// 账号数据迁移（Memory / Connector 合并去重）
// ---------------------------------------------------------------------------

/// POST /api/migrate/account —— 把源账号的 Memory / Connector 合并到目标账号（带去重）。
///
/// 请求体：`{ sourceAccountId?, targetAccountId, memory?, connectors?, region?, sourceRegion? }`
/// `sourceAccountId` 缺省时取**来源版本**的当前登录账号。两个范围默认都启用。
/// 只处理普通文件，不触碰 `workbuddy.db`，因此**无需关闭 WorkBuddy**。
async fn api_migrate_account(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    // 数据来源版本；缺省与目标版本相同（同版本内迁移，行为零变化）。
    let source_region = match body.get("sourceRegion").and_then(Value::as_str) {
        Some(v) => parse_region(Some(v)),
        None => region,
    };
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if target_account_id.is_empty() {
        return json_err("缺少 targetAccountId".to_string(), StatusCode::BAD_REQUEST);
    }

    let source_account_id = body
        .get("sourceAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let scope = migrate::MigrateScope {
        memory: body.get("memory").and_then(Value::as_bool).unwrap_or(true),
        connectors: body
            .get("connectors")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    };
    if scope.is_empty() {
        return json_err("未指定任何迁移范围".to_string(), StatusCode::BAD_REQUEST);
    }

    let Some(target) = account::find_account_for(region, &target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let target_uid = target
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if target_uid.is_empty() {
        return json_err("目标账号缺少 uid".to_string(), StatusCode::BAD_REQUEST);
    }

    // 源账号：显式指定则解析其 uid，否则退回「当前登录账号」。
    let source_uid = if source_account_id.is_empty() {
        session::current_user_uid_for(source_region).unwrap_or_default()
    } else {
        match account::find_account_for(source_region, &source_account_id) {
            Some(acc) => acc
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            None => return json_err("源账号不存在".to_string(), StatusCode::BAD_REQUEST),
        }
    };
    if source_uid.is_empty() {
        return json_err(
            "无法确定源账号 uid（未登录或账号缺少 uid）".to_string(),
            StatusCode::BAD_REQUEST,
        );
    }

    match migrate::migrate_account_data_cross(
        source_region,
        &source_uid,
        region,
        &target_uid,
        scope,
    ) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// 签到 / 保活
// ---------------------------------------------------------------------------

async fn api_checkin_status(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    let list = account::load_accounts_for(region);
    let mut items = Vec::new();
    for acc in &list {
        let status = checkin::get_checkin_status_for(region, acc).await;
        items.push(checkin_status_item(acc, status));
    }
    json_ok(json!({ "accounts": items }))
}

fn checkin_status_item(account: &Value, mut status: Value) -> Value {
    status["accountId"] = account.get("id").cloned().unwrap_or(Value::Null);
    status["email"] = json!(account::account_display_name(account));
    status
}

async fn api_credits(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::Credits) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let Some(acc) = account::find_account_for(region, id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(credits::get_credit_expiry_for(region, &acc).await)
}

fn query_flag_enabled(query: Option<&str>, name: &str) -> bool {
    query.unwrap_or("").split('&').any(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, "true"));
        key == name && matches!(value, "" | "1" | "true" | "yes")
    })
}

async fn api_credit_statistics(RawQuery(query): RawQuery) -> Response {
    let filter = parse_region_filter(query_value(query.as_deref(), "region").as_deref());
    let refresh = query_flag_enabled(query.as_deref(), "refresh");
    json_ok(credit_usage::get_statistics_for_filter(filter, refresh).await)
}

async fn api_token_statistics(RawQuery(query): RawQuery) -> Response {
    let filter = parse_region_filter(query_value(query.as_deref(), "region").as_deref());
    let days = query.as_deref().and_then(|value| {
        value
            .split('&')
            .find_map(|part| part.strip_prefix("days=")?.parse::<i64>().ok())
    });
    match tokio::task::spawn_blocking(move || token_stats::get_statistics_for_filter(filter, days))
        .await
    {
        Ok(statistics) => json_ok(statistics),
        Err(error) => json_err(
            format!("扫描 Token 统计失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn api_checkin(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::Checkin) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let Some(acc) = account::find_account_for(region, id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(checkin::checkin_account_for(region, &acc).await)
}

async fn api_checkin_all(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    // P15 · 签到进悬浮窗（首尾两条；WB 侧没有逐账号进度事件）。
    let task = observe_workbuddy_checkin_start(region);
    let result = checkin::run_checkin_all_for(region).await;
    observe_workbuddy_checkin_end(&task, region, &result);
    json_ok(result)
}

/// GET /api/checkin/config —— 自动签到开关。
///
/// **无需 region**：签到配置是**全局单份**（`auto_checkin_config.json`），
/// 两个 region 共用同一套「是否自动签到」开关；签到日志亦为全局。
async fn api_checkin_config() -> Response {
    json_ok(config::load_checkin_config())
}

async fn api_save_checkin_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match config::save_checkin_config(submitted) {
        Ok(()) => json_ok(config::load_checkin_config()),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

/// GET /api/checkin/logs —— 签到日志（全局单份，按账号 id 归档，无需 region）。
async fn api_checkin_logs() -> Response {
    json_ok(json!({ "logs": config::load_checkin_logs() }))
}

/// GET /api/travel/status —— 派猫猫旅行状态（**CN 专有**）。
///
/// core 的 `travel` 模块没有 `_for(region, …)` 变体（旅行状态机与缓存按账号 id
/// 全局单份，region 化改动面大、风险高），因此本路由保持 CN 语义。国际版无对应
/// 上游能力时按「该功能独立降级」处理：前端按 region 账号 id 过滤后自然为空，
/// 不影响同页其它功能。已知缺口，见发布报告。
async fn api_travel_status() -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::Travel) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    travel::reconcile_due_travel(None).await;
    let items = account::load_accounts()
        .iter()
        .map(|acc| {
            let id = acc.get("id").and_then(Value::as_str).unwrap_or("");
            let mut value = travel::travel_display(id);
            value["accountId"] = acc.get("id").cloned().unwrap_or(Value::Null);
            value["email"] = json!(account::account_display_name(acc));
            value
        })
        .collect::<Vec<_>>();
    json_ok(json!({ "accounts": items }))
}

async fn api_travel_config() -> Response {
    json_ok(config::load_travel_config())
}

async fn api_save_travel_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match config::save_travel_config(submitted) {
        Ok(()) => {
            let saved = config::load_travel_config();
            if saved.get("enabled").and_then(Value::as_bool) == Some(true) {
                tokio::spawn(async {
                    let _ = travel::run_travel_cycle().await;
                    let _ = travel::run_travel_claim_cycle().await;
                });
            }
            json_ok(saved)
        }
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn api_refresh_token(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let Some(acc) = account::find_account_for(region, id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(refresh::refresh_account_token_for(region, acc).await)
}

// ---------------------------------------------------------------------------
// 自动轮换（CodeBuddy CLI）
//
// **CN 专有**：自动轮换驱动的是 CodeBuddy CLI（国内版工具链），core 的 `rotate`
// 模块无 `_for(region, …)` 变体，因此本组路由保持 CN 语义。已知缺口，见发布报告。
// ---------------------------------------------------------------------------

async fn api_rotate_config() -> Response {
    json_ok(config::load_auto_rotate_config())
}

async fn api_save_rotate_config(Json(body): Json<Value>) -> Response {
    match config::save_auto_rotate_config(&body) {
        Ok(()) => json_ok(json!({ "ok": true, "config": config::load_auto_rotate_config() })),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn api_rotate_status() -> Response {
    json_ok(rotate::rotate_status())
}

async fn api_rotate_run() -> Response {
    json_ok(rotate::run_rotate_cycle().await)
}

async fn api_rotate_logs() -> Response {
    json_ok(json!({ "logs": rotate::rotate_logs() }))
}

// ---------------------------------------------------------------------------
// 定时任务排程（六类积分任务，全局单份，无需 region）
// ---------------------------------------------------------------------------

/// GET /api/schedule/config —— 六类定时任务的排程配置。
async fn api_schedule_config() -> Response {
    json_ok(schedule::schedule_to_value(
        &schedule::load_schedule_config(),
    ))
}

/// POST /api/schedule/config —— 写排程配置；小时越界返回 400（错误文案指向对应 `*_enabled`）。
async fn api_save_schedule_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match schedule::save_schedule_config(submitted) {
        Ok(cfg) => json_ok(schedule::schedule_to_value(&cfg)),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/schedule/run —— **立即**执行某一类定时任务，不等排程到点。
///
/// 与桌面的 `run_schedule_task` 命令同源（共用 [`scheduler::run_scheduled_task`]）：排程按
/// 整点触发，保存配置后无法当场自证是否生效，手动触发让改动立刻可验证。
async fn api_run_schedule_task(Json(body): Json<Value>) -> Response {
    let Some(task) = body.get("task").and_then(Value::as_str) else {
        return json_err("缺少 task 字段".to_string(), StatusCode::BAD_REQUEST);
    };
    match schedule::ScheduleTask::parse(task) {
        Some(task) => json_ok(scheduler::run_scheduled_task(task).await),
        None => json_err(format!("未知定时任务: {task}"), StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// 更新
// ---------------------------------------------------------------------------

async fn api_update_check() -> Response {
    json_ok(update::update_check(None, false).await)
}

async fn api_update_config() -> Response {
    json_ok(update::load_github_config())
}

async fn api_save_update_config(Json(body): Json<Value>) -> Response {
    match update::save_github_config(&body) {
        Ok(()) => json_ok(json!({ "ok": true, "config": update::load_github_config() })),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// API 网关（管理面，A-3.7）
// ---------------------------------------------------------------------------

async fn api_gateway_config() -> Response {
    match serde_json::to_value(GatewayConfig::load()) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_save_gateway_config(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::Gateway) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let submitted = body.get("config").cloned().unwrap_or(body);
    let config: GatewayConfig = match serde_json::from_value(submitted) {
        Ok(config) => config,
        Err(error) => return json_err(format!("配置格式错误: {error}"), StatusCode::BAD_REQUEST),
    };
    if let Err(error) = config.save() {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    let state = crate::gateway_host::shared_state();
    let previous = state.config.read().await.clone();
    *state.config.write().await = config.clone();
    state.log.set_keep(config.log_keep);
    state.log.set_log_bodies(config.log_bodies);
    // 只有监听相关字段变了才重新 apply（理由同桌面命令：纯策略改动不该因端口被占而失败）。
    let listener_changed = previous.enabled != config.enabled
        || previous.bind_addr != config.bind_addr
        || previous.port != config.port
        || previous.allow_non_loopback != config.allow_non_loopback;
    let addr = if listener_changed {
        match crate::gateway_host::apply().await {
            Ok(addr) => addr,
            Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
        }
    } else if config.enabled {
        Some(format!("{}:{}", config.bind_addr, config.port))
    } else {
        None
    };
    json_ok(json!({
        "ok": true,
        "config": config,
        "running": addr.is_some(),
        "addr": addr,
    }))
}

/// GET /api/proxy/status —— P12 C 路（base URL 重定向）的**只读状态**。
///
/// ⚠️ 只反映**本地标记**（用户是否按引导设过），**应用不校验代理是否真的生效**。
async fn api_proxy_status() -> Response {
    json_ok(proxy::status_view())
}

/// POST /api/proxy/enable —— 记下「用户已按引导设置」的本地标记（**不改系统设置**）。
async fn api_proxy_enable() -> Response {
    match proxy::set_enabled(true) {
        Ok(()) => json_ok(proxy::status_view()),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/proxy/disable —— **一键还原**：清标记（删文件，不留残留）。
async fn api_proxy_disable() -> Response {
    match proxy::set_enabled(false) {
        Ok(()) => json_ok(proxy::status_view()),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_gateway_status() -> Response {
    let config = GatewayConfig::load();
    let (running, addr) = crate::gateway_host::status().await;
    // 统一走显式契约结构体（snake_case，与 GatewayConfig 一致），不再手工拼 json!。
    let mut view = GatewayStatusView::from(&config);
    view.running = running;
    view.addr = addr;
    view.version = update::APP_VERSION.to_string();
    // 账号池视图（五态 / 逐账号 / 诊断）：形状与 Trae 侧同源，管理面共用一张「账号池」卡。
    let state = crate::gateway_host::shared_state();
    {
        let mut pool = state.pool.write().await;
        // 先与账号库对齐：否则「刚导入、还没发过请求」的账号在池里不存在，页面显示空池。
        buddy_switch_gateway::sync_pool_with_accounts(&mut pool);
        view.fill_pool(&pool, buddy_switch_gateway::timeutil::now_ms());
    }
    // 最近一次请求失败的原因（与 Trae 侧同名同义）。
    view.last_error = state.last_error.read().await.clone();

    // ★ 顺手触发一次余额刷新 —— **不阻塞本响应**（在另一个任务里跑）。
    //
    // 账号是在本函数里刚被同步进池的（见上面的 `sync_pool_with_accounts`），此刻它们
    // 多半「从未取过余额」；而周期循环下一轮要等满一个刷新间隔（默认 30 分钟）——
    // 用户看到的就是「刚导入账号，页面上积分一直是『—』」（2026-09-30 实测：
    // 19:54 起服务、20:24 才出现第一条读数）。
    //
    // 刷新器自带 `needs_refresh` 节流（阈值内不发请求），所以反复读状态**不会**打上游。
    // 与「绝不在**推理请求**路径上拉余额」那条硬约束不冲突：这是管理面查询，
    // 且取数在别的任务里跑，本响应不等它。
    {
        let state = state.clone();
        tokio::spawn(async move {
            let _ = buddy_switch_gateway::credits_refresh::refresh_once(&state).await;
        });
    }
    match serde_json::to_value(view) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /api/gateway/pool/exclusions —— 该 region 的**排除账号**列表（`?region=cn|global`）。
///
/// 与 Tauri 命令 `gateway_pool_exclusions` **同一份实现**
/// （`buddy_switch_gateway::routes::pool_admin::view`）—— 两个入口各写一遍必然漂移。
async fn api_gateway_pool_exclusions(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let region = parse_region(query.get("region").map(String::as_str));
    json_ok(buddy_switch_gateway::routes::pool_admin::view(region))
}

/// POST /api/gateway/pool/exclusions —— 设置 / 取消排除（`{region?, uid, excluded, note?}`）。
///
/// 落盘 + 刷新池内快照 + 按 uid 解绑粘性，三件事收在 `pool_admin::set`（唯一实现）。
async fn api_gateway_set_pool_exclusion(Json(body): Json<Value>) -> Response {
    let uid = body.get("uid").and_then(Value::as_str).unwrap_or("").trim();
    if uid.is_empty() {
        return json_err("缺少 uid".to_string(), StatusCode::BAD_REQUEST);
    }
    let excluded = body
        .get("excluded")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let note = body.get("note").and_then(Value::as_str).unwrap_or("");
    let region = parse_region(body.get("region").and_then(Value::as_str));

    let state = crate::gateway_host::shared_state();
    match buddy_switch_gateway::routes::pool_admin::set(&state, region, uid, excluded, note).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/gateway/pool/revive —— **人工解除**账号池里某账号的禁用（`{uid}`）。
///
/// 与「API Key 的恢复」同构：`disabled` 是**终态且落盘持久化**，而改造前它
/// **没有任何出口** —— 余额刷新被 `if entry.disabled` 挡住，`PoolEntry::revive`
/// 在生产代码里零调用点（详见 `buddy_switch_gateway::pool::Pool::revive` 的文档）。
///
/// 幂等：账号本来可用时返回 `{ok:true,revived:false}` 而**不报错** —— 并发点两次、
/// 或界面状态陈旧，都不该变成一个错误弹窗。
///
/// 「不在池中」是**真错误**（400）：那说明 uid 写错了或账号已被删除，
/// 静默返回成功会让用户以为修好了。
async fn api_gateway_revive_account(Json(body): Json<Value>) -> Response {
    let uid = body.get("uid").and_then(Value::as_str).unwrap_or("").trim();
    if uid.is_empty() {
        return json_err("缺少 uid".to_string(), StatusCode::BAD_REQUEST);
    }

    let state = crate::gateway_host::shared_state();
    let mut pool = state.pool.write().await;
    // 先与账号库对齐：否则「刚导入、还没发过请求」的账号在池里根本不存在，
    // 而界面刚刚明明显示过它 —— 用户会拿到一句莫名其妙的「不在池中」。
    buddy_switch_gateway::sync_pool_with_accounts(&mut pool);
    if pool.get(uid).is_none() {
        return json_err(
            format!("账号 {uid} 不在账号池中（可能已被删除）"),
            StatusCode::BAD_REQUEST,
        );
    }
    let revived = pool.revive(uid);
    json_ok(json!({ "ok": true, "revived": revived }))
}

/// POST /api/gateway/open-data-dir —— 打开 **WorkBuddy 客户端**的数据目录（`{region?}`）。
///
/// 形状（`{ok,path}` 或结构化 `Unsupported`）由 `capability::open_workbuddy_data_dir` 唯一产出，
/// 与 Trae 侧的 `open_trae_data_dir` 同形。
async fn api_gateway_open_data_dir(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    match buddy_switch_core::modules::capability::open_workbuddy_data_dir(region) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_gateway_list_keys() -> Response {
    let state = crate::gateway_host::shared_state();
    let keys: Vec<Value> = state
        .keys
        .list()
        .iter()
        .map(|record| record.masked())
        .collect();
    json_ok(json!({ "keys": keys }))
}

async fn api_gateway_create_key(Json(body): Json<Value>) -> Response {
    let state = crate::gateway_host::shared_state();
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("未命名")
        .to_string();
    let (record, plaintext) = state.keys.create(name, region);
    // ★ 「接入即识别」（P10 §1.6 / ✅ #6）：同桌面端 —— 创建 Key 成功后**后台探测**该 region
    // （复用全仓唯一探测入口 `catalog_probe::probe_region`，它只用本 region 的凭据）。
    // fire-and-forget：探测失败不得阻断接入（§3.17.1 规则 3）；服务端已在 tokio 上下文，
    // 故用 `tokio::spawn`（桌面端因无该上下文改用 `tauri::async_runtime::spawn`）。
    // 顺序不可颠倒：先落 Key、再触发探测。
    tokio::spawn(async move {
        let _ = buddy_switch_gateway::catalog_probe::probe_region(&state, region).await;
    });
    json_ok(json!({ "ok": true, "key": plaintext, "record": record.masked() }))
}

async fn api_gateway_revoke_key(Json(body): Json<Value>) -> Response {
    let state = crate::gateway_host::shared_state();
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match state.keys.revoke(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/gateway/keys/restore —— 撤销吊销（清 `revoked_at`，Key 重新可用）。
async fn api_gateway_restore_key(Json(body): Json<Value>) -> Response {
    let state = crate::gateway_host::shared_state();
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match state.keys.restore(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_gateway_delete_key(Json(body): Json<Value>) -> Response {
    let state = crate::gateway_host::shared_state();
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match state.keys.delete(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/gateway/keys/policy —— 更新某把 Key 的策略（配额 + 账号范围）。
///
/// ★ 入参是**完整策略**（不是补丁）：两个 radio 必选其一由前端抽屉保证，
/// 后端不猜「用户是不是想只改一半」。矛盾值（不限额却带上限）由
/// [`KeyPolicy::normalized`] 收敛到安全的一边，空白名单直接拒绝。
async fn api_gateway_set_key_policy(Json(body): Json<Value>) -> Response {
    use buddy_switch_gateway::apikey::KeyPolicy;
    let state = crate::gateway_host::shared_state();
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    let policy: KeyPolicy =
        match serde_json::from_value(body.get("policy").cloned().unwrap_or(Value::Null)) {
            Ok(policy) => policy,
            Err(error) => {
                return json_err(format!("策略字段不合法: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    match state.keys.update_policy(id, policy) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/gateway/keys/usage?id=… —— 读某把 Key 的用量快照（累计 + 本周期）。
///
/// 与 `list` 分开的理由：用量是**每请求写**的账本，让列表命令每次多读一个文件
/// 不划算；且用量只在打开抽屉/展开行时才需要。
async fn api_gateway_key_usage(RawQuery(query): RawQuery) -> Response {
    let state = crate::gateway_host::shared_state();
    let id = query_value(query.as_deref(), "id").unwrap_or_default();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let now = buddy_switch_core::modules::config::now_ms();
    let Some(record) = state.keys.list().into_iter().find(|record| record.id == id) else {
        return json_err("API Key 不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let usage = state.key_usage.snapshot(&id);
    let used = state
        .key_usage
        .used_for(&id, record.quota_unit, record.quota_period, now);
    json_ok(json!({
        "ok": true,
        "keyId": id,
        "unit": record.quota_unit,
        "period": record.quota_period,
        "periodKey": record.quota_period.period_key(now),
        "used": used,
        // 累计用量按**同一个口径**取（`KeyUsage` 现在是两个口径各自一份计数，
        // 没有单一的 `total` —— 见 `key_usage::KeyUsage` 的文档）。
        "total": usage
            .as_ref()
            .map(|entry| entry.counters(record.quota_unit).total)
            .unwrap_or(0),
    }))
}

async fn api_gateway_models(RawQuery(query): RawQuery) -> Response {
    let region = parse_region(query_value(query.as_deref(), "region").as_deref());
    let state = crate::gateway_host::shared_state();
    let snapshot = state.catalogs.current(region);
    let max_context = state.config.read().await.max_context;
    json_ok(buddy_switch_gateway::routes::models::snapshot_view(
        &snapshot,
        max_context,
    ))
}

async fn api_gateway_refresh_models(Json(body): Json<Value>) -> Response {
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let state = crate::gateway_host::shared_state();
    let strategy = state.strategy_for(region).await;
    let account = match buddy_switch_gateway::AccountSelector
        .select(region, &strategy)
        .await
    {
        Ok(account) => account,
        Err(error) => return json_err(error.message(), StatusCode::BAD_REQUEST),
    };
    let snapshot = state
        .catalogs
        .refresh(region, &state.upstream, &account)
        .await;
    json_ok(serde_json::to_value(snapshot).unwrap_or(Value::Null))
}

async fn api_gateway_strategy() -> Response {
    let state = crate::gateway_host::shared_state();
    // 先取快照，避免跨 await 持有读锁。
    let strategies = state.strategies.read().await.clone();
    let cn = strategies.get(&Region::Cn).cloned().unwrap_or_default();
    let global = strategies.get(&Region::Global).cloned().unwrap_or_default();
    json_ok(json!({
        "cn": buddy_switch_gateway::account_strategy::describe_strategy(Region::Cn, &cn).await,
        "global": buddy_switch_gateway::account_strategy::describe_strategy(Region::Global, &global).await,
    }))
}

async fn api_gateway_save_strategy(Json(body): Json<Value>) -> Response {
    let state = crate::gateway_host::shared_state();
    let region = parse_region(body.get("region").and_then(Value::as_str));
    let strategy_value = body.get("strategy").cloned().unwrap_or_else(|| {
        let mut clone = body.clone();
        if let Some(object) = clone.as_object_mut() {
            object.remove("region");
        }
        clone
    });
    let strategy: buddy_switch_gateway::AccountStrategy =
        match serde_json::from_value(strategy_value) {
            Ok(strategy) => strategy,
            Err(error) => {
                return json_err(format!("策略格式错误: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    {
        let mut strategies = state.strategies.write().await;
        strategies.insert(region, strategy);
        if let Err(error) = buddy_switch_gateway::account_strategy::save_strategies(&strategies) {
            return json_err(error, StatusCode::BAD_REQUEST);
        }
    }
    json_ok(json!({ "ok": true }))
}

/// GET /api/gateway/disabled-models —— 某**作用域**被用户禁用的模型 id 列表（P11）。
///
/// 读的是**进程内唯一**的禁用存储（[`buddy_switch_gateway::disabled_models::shared`]），
/// 即三个网关的出口此刻真正在用的那一份 —— 命令面与出口同源。
///
/// ⚠️ 键是**作用域**（`wb-cn` / `trae-global` / `qoder-cn` …），不是裸 region ——
/// 三个平台互不相通的模型目录各自分家（2026-10-15 升级，含旧键迁移）。
async fn api_gateway_disabled_models(RawQuery(query): RawQuery) -> Response {
    let scope = match resolve_scope_param(query_value(query.as_deref(), "scope").as_deref()) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let store = buddy_switch_gateway::disabled_models::shared();
    let disabled = store.read().await;
    let ids: Vec<String> = disabled.get(scope).iter().cloned().collect();
    json_ok(json!({ "scope": scope.as_str(), "ids": ids }))
}

/// POST /api/gateway/disabled-models —— 置位 / 清位某**作用域**的一个模型（`{scope, modelId, disabled}`）。
///
/// ⚠️ `modelId` **必须带 camelCase 双读**（`modelId` → `model_id`）：前端经 `call()` 原样发
/// camelCase，只读 snake 会造成**静默 no-op**（`set` 收空 id ⇒ `changed:false` 但前端仍乐观
/// 翻转本地集合 = 假开关）。与 `accountId` / `bizNamespace` 的既有双读同一手法。
///
/// `set` 幂等：重复置位 / 清位不改集合、不落盘，`changed: false`。
async fn api_gateway_set_model_disabled(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let model_id = body
        .get("modelId")
        .or_else(|| body.get("model_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let disabled = body
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store = buddy_switch_gateway::disabled_models::shared();
    let changed = {
        let mut guard = store.write().await;
        let changed = guard.set(scope, model_id, disabled);
        if changed {
            if let Err(error) = guard.save() {
                return json_err(error, StatusCode::BAD_REQUEST);
            }
        }
        changed
    };
    json_ok(json!({
        "ok": true,
        "scope": scope.as_str(),
        "model_id": model_id.trim(),
        "disabled": disabled,
        "changed": changed,
    }))
}

/// 某作用域降级链的视图 JSON。
///
/// **与 Tauri 命令共用同一份派生**（`model_fallback::build_view`）—— 两个管理面
/// （桌面 / WebUI）如果各拼一份，状态点迟早会在两边给出不同结论。
async fn fallback_view_json(scope: buddy_switch_core::modules::product::Scope) -> Value {
    let state = crate::gateway_host::shared_state();
    let fb = state.fallback.read().await.clone();
    let disabled = state.disabled.read().await.clone();
    let gates: Vec<(String, i64)> = {
        let pool = state.pool.read().await;
        pool.model_gates()
            .iter()
            .map(|gate| (gate.model.clone(), gate.until_ms))
            .collect()
    };
    let refs: Vec<(&str, i64)> = gates
        .iter()
        .map(|(model, until)| (model.as_str(), *until))
        .collect();
    let view = buddy_switch_gateway::model_fallback::build_view(
        &fb,
        &disabled,
        scope,
        &state.catalogs,
        &refs,
        buddy_switch_gateway::timeutil::now_ms(),
    );
    serde_json::to_value(view).unwrap_or(Value::Null)
}

/// GET /api/gateway/model-fallback —— 某作用域的降级链（含备选**实时**健康态）。
async fn api_gateway_model_fallback(RawQuery(query): RawQuery) -> Response {
    let scope = match resolve_scope_param(query_value(query.as_deref(), "scope").as_deref()) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    json_ok(fallback_view_json(scope).await)
}

/// POST /api/gateway/model-fallback —— 保存某作用域的降级链（**立即生效**）。
///
/// `config` 是**整作用域覆盖**（与 Tauri 命令同一语义）：前端提交时必须把其它链带上。
async fn api_save_gateway_model_fallback(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let raw = body.get("config").cloned().unwrap_or(Value::Null);
    let parsed: buddy_switch_gateway::model_fallback::ScopeFallback =
        match serde_json::from_value(raw) {
            Ok(value) => value,
            Err(error) => {
                return json_err(format!("降级链格式错误: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    let normalized = buddy_switch_gateway::model_fallback::normalize(parsed);
    let store = buddy_switch_gateway::model_fallback::shared();
    {
        let mut guard = store.write().await;
        guard.set_scope(scope, normalized);
        // 写失败必须回传：否则用户以为保存了，重启后配置又变回去。
        if let Err(error) = guard.save() {
            return json_err(error, StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    json_ok(fallback_view_json(scope).await)
}

/// GET /api/gateway/fallback-stats —— 某作用域近 `days` 天的降级统计（**落盘账本**）。
async fn api_gateway_fallback_stats(RawQuery(query): RawQuery) -> Response {
    let scope = match resolve_scope_param(query_value(query.as_deref(), "scope").as_deref()) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let days = query_value(query.as_deref(), "days")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(7);
    json_ok(buddy_switch_gateway::model_fallback::stats_payload(
        scope, days,
    ))
}

/// POST /api/gateway/model-fallback/preview —— 保存前**预演**（**纯计算，零副作用**）。
async fn api_preview_gateway_model_fallback(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let from = body.get("from").and_then(Value::as_str).unwrap_or("");
    let targets: Vec<String> = body
        .get("targets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let state = crate::gateway_host::shared_state();
    let now = buddy_switch_gateway::timeutil::now_ms();
    let disabled = state.disabled.read().await.clone();
    let gates: Vec<(String, i64)> = {
        let pool = state.pool.read().await;
        pool.model_gates()
            .iter()
            .map(|gate| (gate.model.clone(), gate.until_ms))
            .collect()
    };
    let refs: Vec<(&str, i64)> = gates
        .iter()
        .map(|(model, until)| (model.as_str(), *until))
        .collect();
    json_ok(buddy_switch_gateway::model_fallback::preview_payload(
        scope, from, &targets, &disabled, &refs, now,
    ))
}

/// GET /api/gateway/model-image-caps —— 某**作用域**被用户手动指定的图片能力覆盖（issue #101）。
///
/// 形状 `{ scope, caps: { "<模型 id>": true|false } }`：**缺失的键 = 跟随上游**。
/// 读的是进程内唯一存储，与三条产品线 `/v1/models` 的声明同源。
async fn api_gateway_model_image_caps(RawQuery(query): RawQuery) -> Response {
    let scope = match resolve_scope_param(query_value(query.as_deref(), "scope").as_deref()) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let caps = match tokio::task::spawn_blocking(move || {
        buddy_switch_gateway::model_capability::overrides_of(scope)
    })
    .await
    {
        Ok(caps) => caps,
        Err(error) => {
            return json_err(
                format!("读取图片能力失败: {error}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };
    json_ok(json!({ "scope": scope.as_str(), "caps": caps }))
}

/// POST /api/gateway/model-image-caps —— 指定 / 清除某作用域一个模型的图片能力覆盖。
///
/// `supports`：`true` / `false` = 手动指定；**`null` = 还原上游声明**（删键，不是写 `false`）。
/// ⚠️ `modelId` 按 camelCase 读（与前端 `call()` 原样发送的键一致）；只读 snake 会造成
/// 静默 no-op —— 与 `api_gateway_set_model_disabled` 同一手法。
async fn api_gateway_set_model_image_cap(Json(body): Json<Value>) -> Response {
    let scope = match resolve_scope_param(body.get("scope").and_then(Value::as_str)) {
        Ok(scope) => scope,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let model_id = body
        .get("modelId")
        .or_else(|| body.get("model_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if model_id.is_empty() {
        return json_err("模型 id 不能为空".to_string(), StatusCode::BAD_REQUEST);
    }
    // `supports` 允许**显式 null**（= 还原上游）⇒ 不能把「缺失」当 `false`。
    let supports = match body.get("supports") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => {
            return json_err(
                "`supports` 只能是 true / false / null".to_string(),
                StatusCode::BAD_REQUEST,
            )
        }
    };
    // 闭包要 `move` 走 id，但响应体还要回显它 ⇒ 先克隆一份给闭包。
    let id_for_save = model_id.clone();
    let changed = match tokio::task::spawn_blocking(move || {
        buddy_switch_gateway::model_capability::set_and_save(scope, &id_for_save, supports)
    })
    .await
    {
        Ok(Ok(changed)) => changed,
        Ok(Err(error)) => return json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => {
            return json_err(
                format!("保存图片能力失败: {error}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };
    json_ok(json!({
        "ok": true,
        "scope": scope.as_str(),
        "model_id": model_id,
        "supports": supports,
        "changed": changed,
    }))
}

async fn api_gateway_logs() -> Response {
    let state = crate::gateway_host::shared_state();
    json_ok(json!({ "logs": state.log.list() }))
}

async fn api_gateway_clear_logs() -> Response {
    let state = crate::gateway_host::shared_state();
    state.log.clear();
    json_ok(json!({ "ok": true }))
}

/// POST /api/gateway/probe/start —— 启动一轮「账号 × 模型」限流探测（异步作业）。
///
/// 形状照抄 `src-tauri/src/commands.rs::probe_start`：`start` 只**建作业**（原子抢占作业位），
/// 逐格执行由 [`buddy_switch_gateway::model_probe::run`] 在后台任务里完成。
///
/// ★ **拿到 `Ok` 后必须 spawn `run`**：作业位是 `compare_exchange` 原子抢占的，
/// 不 spawn 会让它一直占着 —— 之后所有 `start` 都报「已有探测在进行中」。
/// 服务端已在 tokio 上下文，故用 `tokio::spawn`（桌面端改用 `tauri::async_runtime::spawn`）。
///
/// `uids` / `models` **缺省 = 用池里的全部账号 / 该作用域目录**（与 `start` 的 `Option` 语义一致）；
/// 显式传空数组则走到 `start` 的「没有选中任何账号 / 没有可探测的模型」校验。
///
/// ★ `cells`（**显式格清单**）与那两根轴**互斥**：给了它就只探点名的格（账号页台账卡走这条）。
///
/// ★ **作用域参数化**：入参从 `region` 升级为 `scope`（`wb-cn` / `qoder-cn` …）。
/// `target_for_scope` 是 **fail-closed** 解析（未知 / 拼错 ⇒ 可读错误），并据作用域挑出对应平台的目标。
async fn api_probe_start(Json(body): Json<Value>) -> Response {
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let target = match buddy_switch_gateway::model_probe::target_for_scope(&scope) {
        Ok(target) => target,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uids: Option<Vec<String>> = body.get("uids").and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect()
    });
    let models: Option<Vec<String>> = body.get("models").and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect()
    });
    /* ★ 显式格清单（与 `uids` / `models` 互斥）：账号页台账卡只探点名的格。
    逐项反序列化，坏项丢弃而不是整请求 400 —— 与 `uids` / `models` 的
    `filter_map` 同款宽容度（前端偶尔会带 `null`）。 */
    let cells: Option<Vec<buddy_switch_gateway::model_probe::ProbeStartCell>> =
        body.get("cells").and_then(Value::as_array).map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        });
    let adopt = body.get("adopt").and_then(Value::as_bool).unwrap_or(false);
    match buddy_switch_gateway::model_probe::start(&target, uids, models, cells, adopt).await {
        Ok(total) => {
            tokio::spawn(buddy_switch_gateway::model_probe::run(target));
            json_ok(json!({ "started": true, "total": total }))
        }
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/gateway/probe/status —— 探测作业快照（从未跑过时 `{ "phase": "idle" }`）。
///
/// `scope` 为**必填**（fail-closed）：作业槽是进程级全局，快照本身与作用域无关，但命令面
/// 统一带 `scope`，未知 / 拼错的作用域一律拒绝（不静默接受）。
async fn api_probe_status(RawQuery(query): RawQuery) -> Response {
    let scope = query_value(query.as_deref(), "scope").unwrap_or_default();
    if let Err(error) = resolve_scope_param(Some(&scope)) {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    json_ok(buddy_switch_gateway::model_probe::status())
}

/// GET /api/gateway/probe/ledger —— 某作用域的**实测台账**（跨重启累积）。
///
/// 与 `/status` 的分工：`status` = 本轮作业（进度 / 相位）；本路由 = 这些格**最近一次**的
/// 实判（落盘 `probe_results.json`，按作用域隔离）。账号页重启后仍要看到实测结论 ⇒ 读它。
async fn api_probe_ledger(RawQuery(query): RawQuery) -> Response {
    let scope = query_value(query.as_deref(), "scope").unwrap_or_default();
    if let Err(error) = resolve_scope_param(Some(&scope)) {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    json_ok(buddy_switch_gateway::model_probe::ledger_snapshot(&scope))
}

/// POST /api/gateway/probe/cancel —— 请求取消（逐格检查；已探到的格保留）。
async fn api_probe_cancel(RawQuery(query): RawQuery) -> Response {
    let scope = query_value(query.as_deref(), "scope").unwrap_or_default();
    if let Err(error) = resolve_scope_param(Some(&scope)) {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    json_ok(buddy_switch_gateway::model_probe::cancel())
}

// ---------------------------------------------------------------------------
// Trae 模块路由
// ---------------------------------------------------------------------------
//
// 与 `src-tauri/src/commands.rs` 的同名命令**一一对应**，返回形状由
// `buddy_switch_core::modules::trae::handlers` 单点保证。本层只做参数解析与
// HTTP 状态码映射，不自行拼装业务对象——这正是为了避免 `copy_sessions` 那种
// 「两条通道返回形状不同」的问题。

async fn api_trae_env() -> Response {
    json_ok(trae::platform::env_status())
}

/// GET /api/trae/variants —— **全部** Trae 产品线的独立环境状态（数组）。
///
/// 与 `/api/trae/env` 的分工：`env` 回答"自动挑中的是哪一条"，本端点回答
/// "每条各自是什么状态"（供前端并排渲染多个图标）。与 Tauri 的
/// `get_trae_variants` 一一对应。
async fn api_trae_variants() -> Response {
    json_ok(trae::platform::variants_status())
}

async fn api_trae_capabilities() -> Response {
    json_ok(trae::platform::capabilities())
}

/// GET /api/trae/accounts —— 账号 + 分组 + 计数（`variant` 可选，缺省默认变体）。
async fn api_trae_accounts(RawQuery(query): RawQuery) -> Response {
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    json_ok(trae::handlers::accounts_overview_for(variant))
}

/// GET /api/trae/checkin/status —— 签到摘要与冷却（`variant` 可选）。
async fn api_trae_checkin_status(RawQuery(query): RawQuery) -> Response {
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    json_ok(trae::handlers::checkin_status_for(variant))
}

/// GET /api/trae/credits —— 剩余积分、明细、每日趋势（`variant` 可选）。
async fn api_trae_credits(RawQuery(query): RawQuery) -> Response {
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    json_ok(trae::handlers::credits_overview_for(variant))
}

/// GET /api/trae/checkin/status-map —— 逐账号「今日签到状态」（**只读**；issue #47）。
///
/// `scope` 决定区域（`trae-cn` / `trae-global`，**fail-closed**：缺失 / 拼错 ⇒ 400）；
/// `accountIds` 是**逗号分隔**的 uid 列表（见 [`parse_id_list`]）；空 / 缺失 ⇒ `{}`
/// （一次上游请求都不发）。未知 uid **不进** map —— 前端显示「—」，不是「未签到」。
///
/// ⚠️ 这是**读**：绝不在这里触发 claim（签到是写操作，走 `/api/trae/checkin`）。
async fn api_trae_checkin_status_map(RawQuery(query): RawQuery) -> Response {
    let variant = match trae_variant_from_scope_query(query.as_deref()) {
        Ok(variant) => variant,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let ids = parse_id_list(query_value(query.as_deref(), "accountIds"));
    json_ok(trae::checkin::status_map_for(variant, ids).await)
}

/// GET /api/trae/account/token-today —— 逐账号「今日 Token」（**只读**；issue #47）。
///
/// 形状与 WB 的 `/api/account/token-today` 同构；`clientSource: "unavailable"` 表示
/// Trae 没有客户端会话落盘 ⇒ 客户端那一档**不可得**（不是 0）。见
/// [`trae::token_stats::account_token_today`]。
///
/// 读网关日志是阻塞 IO ⇒ 放进 blocking 线程池。
async fn api_trae_account_token_today(RawQuery(query): RawQuery) -> Response {
    let variant = match trae_variant_from_scope_query(query.as_deref()) {
        Ok(variant) => variant,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let force = query_flag_enabled(query.as_deref(), "force");
    match tokio::task::spawn_blocking(move || {
        trae::token_stats::account_token_today(variant, force)
    })
    .await
    {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("读取今日 Token 失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// GET /api/trae/token-stats —— Token 统计（`days` / `scope` 可选）。
///
/// `scope` 为**变体范围**筛选维度：`work` / `cn` / `unlabeled` / `all`（缺省 `all`）。
async fn api_trae_token_statistics(RawQuery(query): RawQuery) -> Response {
    let days = query_value(query.as_deref(), "days").and_then(|value| value.parse::<i64>().ok());
    let scope = query_value(query.as_deref(), "scope")
        .map(|value| trae::token_stats::TraeTokenScope::parse(&value))
        .unwrap_or_default();
    json_ok(trae::handlers::token_statistics(days, scope))
}

/// GET /api/trae/client-import/probe —— 探测一个**用户自己导出的明文库**能否读。
///
/// 与 `src-tauri/src/commands.rs` 的 `probe_trae_client_db` **一一对应**。
/// 只探测、不保存配置；`error.code` 决定界面文案（五种错误必须分开显示）。
async fn api_trae_client_probe(RawQuery(query): RawQuery) -> Response {
    let path = query_value(query.as_deref(), "path").unwrap_or_default();
    let value = tokio::task::spawn_blocking(move || trae::client_import::probe(&path))
        .await
        .unwrap_or_else(|error| {
            serde_json::json!({
                "ok": false,
                "path": "",
                "error": { "code": "io", "message": format!("探测失败: {error}") },
                "tables": [],
            })
        });
    json_ok(value)
}

/// GET /api/trae/client-import/config —— 已保存的导入路径与「文件还在不在」。
async fn api_trae_client_config() -> Response {
    let value = tokio::task::spawn_blocking(|| {
        let path = trae::client_import::configured_path();
        let exists = path
            .as_deref()
            .map(|target| std::path::Path::new(target).is_file())
            .unwrap_or(false);
        serde_json::json!({ "path": path, "exists": exists })
    })
    .await
    .unwrap_or_else(
        |error| serde_json::json!({ "path": null, "exists": false, "error": error.to_string() }),
    );
    json_ok(value)
}

/// POST /api/trae/client-import/path —— 保存 / 清除导入路径（`path` 缺省或空串即清除）。
async fn api_trae_save_client_path(Json(body): Json<Value>) -> Response {
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let value = tokio::task::spawn_blocking(move || match path.as_deref() {
        Some(target) => serde_json::json!({
            "ok": trae::client_import::save_config(target),
            "path": target,
        }),
        None => serde_json::json!({
            "ok": trae::client_import::clear_config(),
            "path": Value::Null,
        }),
    })
    .await
    .unwrap_or_else(
        |error| serde_json::json!({ "ok": false, "path": null, "error": error.to_string() }),
    );
    json_ok(value)
}

/// POST /api/trae/client-import/unlock —— 用口令把**加密库**导出成明文副本。
///
/// 与 `src-tauri/src/commands.rs` 的 `unlock_trae_client_db` **一一对应**（同一套分支语义：
/// `decrypted` / `plaintext` / `error`）。
async fn api_trae_unlock_client_db(Json(body): Json<Value>) -> Response {
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let passphrase = body
        .get("passphrase")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let value = tokio::task::spawn_blocking(move || {
        let source = std::path::PathBuf::from(&path);
        let plain = trae::client_import::plaintext_path();
        let target = plain.to_string_lossy().to_string();
        match buddy_switch_vault::verify(&source, &passphrase) {
            Ok(info) => {
                if let Err(error) =
                    buddy_switch_vault::export_plaintext(&source, &passphrase, &plain)
                {
                    return serde_json::json!({
                        "ok": false,
                        "error": { "code": error.code(), "message": error.message() },
                    });
                }
                let saved = trae::client_import::save_config(&target);
                serde_json::json!({
                    "ok": saved,
                    "mode": "decrypted",
                    "compatibility": info.compatibility,
                    "tables": info.tables,
                    "path": target,
                    "probe": trae::client_import::probe(&target),
                })
            }
            Err(buddy_switch_vault::VaultError::AlreadyPlaintext) => {
                let saved = trae::client_import::save_config(&path);
                serde_json::json!({
                    "ok": saved,
                    "mode": "plaintext",
                    "path": path,
                    "probe": trae::client_import::probe(&path),
                })
            }
            Err(error) => serde_json::json!({
                "ok": false,
                "error": { "code": error.code(), "message": error.message() },
            }),
        }
    })
    .await
    .unwrap_or_else(|error| {
        serde_json::json!({
            "ok": false,
            "error": { "code": "io", "message": format!("解锁失败: {error}") },
        })
    });
    json_ok(value)
}

/// GET /api/trae/client-usage —— 客户端账本（读导入的明文库）。
///
/// `path` 缺省时用已保存的路径；两者都没有 ⇒ `{ configured: false }`
/// （**不是空统计**：「没导入」与「导入了但这段没用量」必须能分开）。
async fn api_trae_client_usage(RawQuery(query): RawQuery) -> Response {
    let path = query_value(query.as_deref(), "path").filter(|value| !value.trim().is_empty());
    let days = query_value(query.as_deref(), "days").and_then(|value| value.parse::<i64>().ok());
    let scope = query_value(query.as_deref(), "scope")
        .map(|value| trae::token_stats::TraeTokenScope::parse(&value))
        .unwrap_or_default();
    let value = tokio::task::spawn_blocking(move || {
        let target = path
            .or_else(trae::client_import::configured_path)
            .unwrap_or_default();
        if target.trim().is_empty() {
            return serde_json::json!({ "source": "trae-client", "configured": false });
        }
        trae::client_import::summarize(&target, days, scope)
    })
    .await
    .unwrap_or_else(|error| {
        serde_json::json!({
            "source": "trae-client",
            "error": { "code": "io", "message": format!("读取客户端账本失败: {error}") },
        })
    });
    json_ok(value)
}

/// GET /api/trae/logs —— 运行日志（`kind` / `date` / `keyword` / `limit` / `variant` 均可选）。
///
/// 读取 `logs/` 下的纯文本日志文件，属于阻塞 IO，因此放进 blocking 线程池。
async fn api_trae_logs(RawQuery(query): RawQuery) -> Response {
    let read = |key: &str| query_value(query.as_deref(), key);
    let limit = read("limit").and_then(|value| value.parse::<u64>().ok());
    let params = serde_json::json!({
        "kind": read("kind"),
        "date": read("date"),
        "keyword": read("keyword"),
        "limit": limit,
        "variant": read("variant"),
    });
    match tokio::task::spawn_blocking(move || trae::handlers::logs(&params)).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("读取日志失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// GET /api/trae/profiles —— 登录态快照总览（`variant` 可选）。
///
/// 快照目录是递归统计（逐文件求大小），可能耗时数十毫秒以上，
/// 因此放进 blocking 线程池，避免占用 HTTP 工作线程。
async fn api_trae_profiles(RawQuery(query): RawQuery) -> Response {
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    match tokio::task::spawn_blocking(move || trae::profile::overview_for(variant)).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("读取快照失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn api_trae_settings() -> Response {
    match serde_json::to_value(trae::settings::load()) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("读取设置失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn api_trae_save_settings(Json(body): Json<Value>) -> Response {
    let patch = body.get("patch").cloned().unwrap_or(body);
    match trae::handlers::save_settings(patch) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_add_account(Json(body): Json<Value>) -> Response {
    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    let jwt_value = body.get("jwt").and_then(Value::as_str).unwrap_or("");
    let group_id = body.get("groupId").and_then(Value::as_str);
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::add_account_for(variant, name, jwt_value, group_id) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_update_account(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    let name = body.get("name").and_then(Value::as_str);
    let jwt_value = body.get("jwt").and_then(Value::as_str);
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::update_account_for(variant, user_id, name, jwt_value) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/accounts/remark —— 设置账号备注（字段级更新）。
///
/// `remark` 是字符串：空串（或全空白）＝清空备注（后端删掉账号库里的 `remark` 键）。
/// 与 `accounts/update` 分开，因为那条的 `name` 是「空串＝不动」——
/// 两种语义挤进同一个参数位，迟早在某条路径上静默错一边。
async fn api_trae_set_account_remark(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    // 缺 `remark` 键按空串处理：与「清空」同义，而不是报 400。
    let remark = body
        .get("remark")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::set_remark_for(variant, user_id, Some(remark.as_str())) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_delete_account(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    let delete_profile = body
        .get("deleteProfile")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    let owned = user_id.to_string();
    match tokio::task::spawn_blocking(move || {
        trae::handlers::delete_account_for(variant, &owned, delete_profile)
    })
    .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("删除账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/accounts/import-local —— 从 Trae 客户端登录态导入当前账号。
///
/// 与 WorkBuddy 侧 `/api/import-local` 语义一致：读客户端 userData 里的
/// `Cloud-IDE-JWT`，已存在的账号**覆盖**（刷新 JWT，保留名字/分组），不存在则新建。
///
/// body 可带可选 `variant`（`"trae_work"` / `"trae_cn"`，也接受 `TRAE SOLO CN`
/// 之类的目录名），决定读哪条产品线的 userData；**缺失时回落默认变体**，
/// 保证旧调用点行为不变。
async fn api_trae_import_local(Json(body): Json<Value>) -> Response {
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match tokio::task::spawn_blocking(move || trae::handlers::import_local_account_for(variant))
        .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("导入本机账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/oauth/start —— 发起 Trae OAuth 登录（开本地回调监听）。
///
/// body: `{ variant? }`（`"trae_work"` / `"trae_cn"`，缺失 / 无法识别回落默认变体，
/// 与 WorkBuddy 侧 `/api/oauth/start` 的 `{ region? }` 同构）。
/// 返回 `{ loginId, verificationUri, expiresIn, port, variant, variantLabel,
/// deviceCredential }`，由调用方负责打开 `verificationUri`。
///
/// 变体必须**透传到 core**：授权 URL 的设备身份、会话归属、成功后落库的账号库
/// 全部由它决定；在这里丢弃它会让两条产品线的登录互相串号。
async fn api_trae_oauth_start(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Trae, capability::Capability::Oauth) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let variant = trae::handlers::parse_variant_param(&body);
    match trae::handlers::oauth_login_start_for(variant).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/oauth/status —— 轮询登录结果（body: `{ loginId }`）。
///
/// 与 WorkBuddy 侧 `/api/oauth/status` 同构：**永不返 Err**，
/// 「还没好」用 `{ done: false }` 表达，前端无需靠抛错驱动轮询。
/// 变体不需要单独传：会话自己记着它（`loginId` 是唯一入口）。
async fn api_trae_oauth_status(Json(body): Json<Value>) -> Response {
    let login_id = body
        .get("loginId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    json_ok(trae::handlers::oauth_login_status(&login_id))
}

/// POST /api/trae/oauth/cancel —— 取消登录（body: `{ loginId }`），释放端口。
async fn api_trae_oauth_cancel(Json(body): Json<Value>) -> Response {
    let login_id = body
        .get("loginId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    json_ok(trae::handlers::oauth_login_cancel(&login_id))
}

/// POST /api/trae/accounts/export —— 按 userId 列表导出完整记录（含 JWT）。
async fn api_trae_export_accounts(Json(body): Json<Value>) -> Response {
    let ids = body
        .get("userIds")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match tokio::task::spawn_blocking(move || trae::handlers::export_accounts_for(variant, &ids))
        .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("导出账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/accounts/export-to-path —— 写入用户选择的路径，返回落地路径。
async fn api_trae_export_accounts_to_path(Json(body): Json<Value>) -> Response {
    let ids = body
        .get("userIds")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match tokio::task::spawn_blocking(move || {
        trae::handlers::export_accounts_to_path_for(variant, &ids, &path)
    })
    .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("导出账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/accounts/import/preview —— 解析导入文件并回传脱敏预览。
///
/// 纯函数（只读请求体文本，不触及账号库），因此**不接受也不忽略 variant**——
/// 预览阶段还没有落库位置，给个参数只会是永远被忽略的假参数。
async fn api_trae_preview_import(Json(body): Json<Value>) -> Response {
    let text = body
        .get("fileText")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match trae::handlers::preview_import_file(&text) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/accounts/import —— 按选中索引导入，返回计数与最新账号视图。
async fn api_trae_import_accounts(Json(body): Json<Value>) -> Response {
    let text = body
        .get("fileText")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let indexes = body
        .get("indexes")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_u64().map(|n| n as usize))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match tokio::task::spawn_blocking(move || {
        trae::handlers::import_accounts_for(variant, &text, &indexes)
    })
    .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("导入账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/legacy-merge —— 旧产品线账号库并入国内版区域账号库（幂等）。
///
/// 无请求体（无参数）：合并的输入是磁盘上的旧库文件，输出是合并报告。
async fn api_trae_legacy_merge() -> Response {
    match trae::handlers::merge_legacy_regions() {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/groups —— 分组操作分发。
///
/// 动作由 `action` 字段指定（`create` / `update` / `delete` / `move`），
/// 与 Tauri 的 `trae_group_op(action, params, variant)` 参数顺序一致。
async fn api_trae_group_op(Json(body): Json<Value>) -> Response {
    let action = body
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let params = body.get("params").cloned().unwrap_or_else(|| json!({}));
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::group_op_for(variant, &action, &params) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/checkin —— 批量签到。
///
/// webui 无事件推送通道，因此这里一次性返回完整报告；Tauri 侧同名命令会额外
/// 派发 `trae-checkin-progress` 事件。**两者的响应体形状相同**。
///
/// 变体从 `options.variant` 解析（与其他端点「顶层 `variant`」不同：签到的入参
/// 本就整体包在 `options` 里，再套一层顶层字段会让两条通道的契约分叉）。
async fn api_trae_checkin(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Trae, capability::Capability::Checkin) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let options = body.get("options").cloned().unwrap_or(body);
    let parsed = match trae::handlers::parse_checkin_options(&options) {
        Ok(parsed) => parsed,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    // P15 · 签到也进悬浮窗。⚠️ webui **没有事件通道** ⇒ 只能给首尾两条；
    // 桌面侧（`commands.rs` 的 `trae_checkin`）有逐账号进度 —— 那是两条通道的
    // **能力差异**，如实反映，不在这边编造中间进度。
    let task = "trae-checkin";
    agent_watch::record_local(
        task,
        "Trae 签到",
        companion_state::CompanionState::Running,
        config::now_ms(),
    );
    let report = trae::handlers::run_checkin_report(parsed).await;
    observe_trae_checkin_summary(task, &report);
    json_ok(report)
}

async fn api_trae_refresh_credits(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Trae, capability::Capability::Credits) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let user_id = body.get("userId").and_then(Value::as_str);
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::refresh_credits_for(variant, user_id).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_refresh_jwt(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::refresh_jwt_for(variant, user_id).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_clear_cooldown(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str);
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::clear_cooldown_for(variant, user_id) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_save_login(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    let variant = trae::handlers::parse_variant_param(&body);
    let owned = user_id.to_string();
    match tokio::task::spawn_blocking(move || trae::handlers::save_login_for(variant, &owned)).await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("保存登录态失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn api_trae_backup_profile(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    let variant = trae::handlers::parse_variant_param(&body);
    let owned = user_id.to_string();
    match tokio::task::spawn_blocking(move || trae::handlers::backup_profile_for(variant, &owned))
        .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("备份失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn api_trae_restore_profile(Json(body): Json<Value>) -> Response {
    let user_id = body.get("userId").and_then(Value::as_str).unwrap_or("");
    if user_id.is_empty() {
        return json_err("缺少 userId".to_string(), StatusCode::BAD_REQUEST);
    }
    let variant = trae::handlers::parse_variant_param(&body);
    let owned = user_id.to_string();
    match tokio::task::spawn_blocking(move || trae::handlers::restore_profile_for(variant, &owned))
        .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("恢复失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn api_trae_delete_profile(Json(body): Json<Value>) -> Response {
    let slot = body.get("slot").and_then(Value::as_str).unwrap_or("");
    if slot.is_empty() {
        return json_err("缺少 slot".to_string(), StatusCode::BAD_REQUEST);
    }
    let variant = trae::handlers::parse_variant_param(&body);
    let owned = slot.to_string();
    match tokio::task::spawn_blocking(move || trae::handlers::delete_profile_for(variant, &owned))
        .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("删除快照失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/switch —— 切换账号。
///
/// 返回体是完整的切换报告（含逐步进度），与 Tauri 侧一致；进度事件仅 Tauri 有。
async fn api_trae_switch(Json(body): Json<Value>) -> Response {
    let options = body.get("options").cloned().unwrap_or(body);
    let parsed = match trae::handlers::parse_switch_options(&options) {
        Ok(parsed) => parsed,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match tokio::task::spawn_blocking(move || {
        trae::profile::switch_account(&parsed, |_| {}).to_json()
    })
    .await
    {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("切换账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/trae/device/reset —— 重置设备标识（`variant` 可选）。
async fn api_trae_reset_device(Json(body): Json<Value>) -> Response {
    let variant = trae::handlers::parse_variant_param(&body);
    match tokio::task::spawn_blocking(move || trae::handlers::reset_device_for(variant)).await {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("重置设备标识失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

// ---------------------------------------------------------------------------
// Trae API 网关（管理面）
// ---------------------------------------------------------------------------
//
// 与上面 WorkBuddy 网关的管理面形状一致，少一套多 Key 管理（Trae 只有一把钥匙）。

async fn api_trae_gateway_config() -> Response {
    match serde_json::to_value(buddy_switch_gateway::trae::TraeGatewayConfig::load()) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_save_trae_gateway_config(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Trae, capability::Capability::Gateway) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let submitted = body.get("config").cloned().unwrap_or(body);
    let config: buddy_switch_gateway::trae::TraeGatewayConfig =
        match serde_json::from_value(submitted) {
            Ok(config) => config,
            Err(error) => {
                return json_err(format!("配置格式错误: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    if let Err(error) = config.save() {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    let state = crate::trae_gateway_host::shared_state();
    *state.config.write().await = config.clone();
    state.log.set_keep(config.log_keep);
    state.log.set_log_bodies(config.log_bodies);
    let addr = match crate::trae_gateway_host::apply().await {
        Ok(addr) => addr,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    json_ok(json!({
        "ok": true,
        "config": config,
        "running": addr.is_some(),
        "addr": addr,
    }))
}

async fn api_trae_gateway_status(RawQuery(query): RawQuery) -> Response {
    let (running, addr) = crate::trae_gateway_host::status().await;
    // `variant` 决定看哪个账号池；缺失 / 未知 → 默认变体（键集合不变，见 status_view）。
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    json_ok(
        buddy_switch_gateway::trae::status_view(
            &crate::trae_gateway_host::shared_state(),
            running,
            addr,
            update::APP_VERSION,
            variant,
        )
        .await,
    )
}

/// GET /api/trae/gateway/models —— 对外暴露的模型清单（**随上游刷新**）。
///
/// 与 `api_trae_client_models` 同源（同一份客户端缓存），形状不同：
/// 这里是 OpenAI `/v1/models` 形状的扁平去重清单。`variant` 缺失 / 未知 → 默认变体。
async fn api_trae_gateway_models(RawQuery(query): RawQuery) -> Response {
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    // ★ 出口 ①（P11）：与 Trae 网关本体读**同一份** `disabled`（按**作用域**键控），
    //   否则「管理面看到的清单」与「网关实际暴露的清单」会不一致。
    let disabled = {
        let store = buddy_switch_gateway::disabled_models::shared();
        let guard = store.read().await;
        guard
            .get(buddy_switch_core::modules::product::scope_of_trae_region(
                variant.region(),
            ))
            .clone()
    };
    buddy_switch_gateway::trae::catalog::ensure_fresh(variant).await;
    let max_mode = buddy_switch_gateway::trae::TraeGateway::shared_state().max_mode_now();
    json_ok(buddy_switch_gateway::trae::payload::models_response_for(
        variant, &disabled, max_mode,
    ))
}

/// GET /api/trae/gateway/client-models —— 客户端（**上游下发**）的模型清单。
///
/// 与 [`api_trae_gateway_models`]（静态对外清单）**不是一回事**：这里读客户端
/// `state.vscdb` 里上游下发的清单缓存（见 core 的 `trae::model_list`），
/// 因此随客户端刷新而变。`variant` 缺失 / 未知 → 默认变体（TraeWork）。
async fn api_trae_client_models(RawQuery(query): RawQuery) -> Response {
    let variant = parse_trae_variant(query_value(query.as_deref(), "variant").as_deref());
    json_ok(
        serde_json::to_value(trae::model_list::read_client_model_list(variant))
            .unwrap_or_else(|error| json!({ "source": "missing", "note": error.to_string() })),
    )
}

/// GET /api/trae/gateway/keys —— 多 Key 列表（形状与 Tauri 命令逐字一致）。
///
/// **形状唯一来源**：直接调 gateway crate 的 `apikey::list_response`，
/// 本层不拼装（`MEMORY.md §二`：`copy_sessions` 事故的直接对策）。
async fn api_trae_list_api_keys() -> Response {
    let state = crate::trae_gateway_host::shared_state();
    json_ok(buddy_switch_gateway::trae::apikey::list_response(
        &state.key_store,
    ))
}

/// POST /api/trae/gateway/keys —— 新建 Key（`{name, variant}`）。
///
/// 明文仅此一次返回；`variant` 缺省 / 未知 → 默认变体（TraeWork）。
async fn api_trae_create_api_key(Json(body): Json<Value>) -> Response {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未命名 Key")
        .to_string();
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    let state = crate::trae_gateway_host::shared_state();
    json_ok(buddy_switch_gateway::trae::apikey::create_response(
        &state.key_store,
        name,
        variant,
    ))
}

/// POST /api/trae/gateway/keys/revoke —— 吊销 Key（`{id}`）。
async fn api_trae_revoke_api_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::trae_gateway_host::shared_state();
    match state.key_store.revoke(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/gateway/keys/policy —— 更新某把 Trae Key 的策略（issue #76）。
///
/// 与 WB 侧同形：入参是**完整策略**（非补丁），矛盾值由 `KeyPolicy::normalized`
/// 收敛到安全的一边，空白名单拒绝。
async fn api_trae_set_key_policy(Json(body): Json<Value>) -> Response {
    use buddy_switch_gateway::apikey::KeyPolicy;
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let policy: KeyPolicy =
        match serde_json::from_value(body.get("policy").cloned().unwrap_or(Value::Null)) {
            Ok(policy) => policy,
            Err(error) => {
                return json_err(format!("策略字段不合法: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    let state = crate::trae_gateway_host::shared_state();
    match state.key_store.update_policy(id, policy) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/trae/gateway/keys/usage?id=… —— 读某把 Trae Key 的用量快照。
async fn api_trae_key_usage(RawQuery(query): RawQuery) -> Response {
    let id = query_value(query.as_deref(), "id").unwrap_or_default();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::trae_gateway_host::shared_state();
    let now = buddy_switch_core::modules::config::now_ms();
    let Some(record) = state
        .key_store
        .list()
        .into_iter()
        .find(|record| record.id == id)
    else {
        return json_err("API Key 不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let usage = state.key_usage.snapshot(&id);
    let used = state
        .key_usage
        .used_for(&id, record.quota_unit, record.quota_period, now);
    let exceeded = matches!(
        buddy_switch_gateway::apikey::quota_verdict_for_policy(&record.policy(), used),
        buddy_switch_gateway::apikey::QuotaVerdict::Exceeded { .. }
    );
    json_ok(json!({
        "ok": true,
        "keyId": id,
        "unit": record.quota_unit,
        "period": record.quota_period,
        "periodKey": record.quota_period.period_key(now),
        "used": used,
        "total": usage
            .as_ref()
            .map(|entry| entry.counters(record.quota_unit).total)
            .unwrap_or(0),
        "exceeded": exceeded,
    }))
}

/// POST /api/trae/gateway/keys/restore —— 撤销吊销（Key 重新可用，`{id}`）。
async fn api_trae_restore_api_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::trae_gateway_host::shared_state();
    match state.key_store.restore(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/gateway/keys/delete —— 物理删除已吊销的 Key（`{id}`）。
async fn api_trae_delete_api_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::trae_gateway_host::shared_state();
    match state.key_store.delete(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/open-data-dir —— 打开 Trae 数据目录（`{variant?}`）。
///
/// 形状（`{ok,path}` 或结构化 `Unsupported`）由 `handlers::open_data_dir` 唯一产出。
async fn api_trae_open_data_dir(Json(body): Json<Value>) -> Response {
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::open_data_dir(variant) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/trae/launch-client —— 启动该变体的 Trae 客户端（OAuth 的前置动作）。
async fn api_trae_launch_client(Json(body): Json<Value>) -> Response {
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    match trae::handlers::launch_client_for(variant) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_trae_gateway_logs() -> Response {
    let state = crate::trae_gateway_host::shared_state();
    json_ok(json!({ "logs": state.log.list() }))
}

async fn api_trae_clear_gateway_logs() -> Response {
    let state = crate::trae_gateway_host::shared_state();
    state.log.clear();
    json_ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// 桌面悬浮窗（P9 · 只读数据层）
// ---------------------------------------------------------------------------
//
// 与 `src-tauri/src/commands.rs` 的 `companion_*` 一一对应（两宿主同步）。
// ★ 只读红线：命令面**只有**读 / 显隐 / 聚焦，无任何写操作。
// ★ 刷新全在前端 webview（20s 轮询 + 既有事件）⇒ 不新增 Rust 后台循环。

/// 悬浮窗快照的 provider（**只读**，P9 §1.3）——与 Tauri 宿主**同一份数据域**：
/// 只接由**现有**进度状态（`switch-progress`）推导出的「进行中操作」。
///
/// 无进行中操作 ⇒ 结构化空态（`source: "unavailable"` + `items: []`），**不造假**。
fn companion_snapshot() -> companion_state::CompanionSnapshot {
    let mut operations: Vec<companion_state::LiveOperation> = Vec::new();

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

/// GET /api/companion/status —— 悬浮窗的**只读**快照（两宿主同源）。
async fn api_companion_status() -> Response {
    json_ok(json!(companion_snapshot()))
}

/// POST /api/companion/visibility —— 显隐悬浮窗。
///
/// webui 是**浏览器**，没有 Tauri 独立窗口 ⇒ 结构化「仅桌面端可用」（**不假装成功**）。
async fn api_companion_set_visible() -> Response {
    json_err(
        "桌面悬浮窗仅在桌面端可用".to_string(),
        StatusCode::BAD_REQUEST,
    )
}

/// GET /api/qoder/web/proxy —— 读逐包出站代理配置（两个区域各一个 URL；空 = 直连）。
async fn api_qoder_web_proxy_get() -> Response {
    match serde_json::to_value(buddy_switch_core::modules::qoder::web_proxy::WebProxyConfig::load())
    {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/qoder/web/proxy —— 保存逐包出站代理配置（两个字段都先过形状校验）。
async fn api_qoder_web_proxy_set(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").cloned().unwrap_or(body);
    let config: buddy_switch_core::modules::qoder::web_proxy::WebProxyConfig =
        match serde_json::from_value(submitted) {
            Ok(config) => config,
            Err(error) => {
                return json_err(
                    format!("代理配置格式错误: {error}"),
                    StatusCode::BAD_REQUEST,
                )
            }
        };
    if let Err(error) = config.save() {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    json_ok(json!({ "ok": true, "config": config }))
}

/// POST /api/qoder/web/{login-open,login-close,credit-packs/probe} —— Qoder 网页端逐包链路。
///
/// ★ **仅桌面端可用**，且是**原理性**限制而非未实现：逐包数据在网页端私有接口
/// `/api/v2/me/usages/big_model_credits`，只认浏览器登录后的会话 Cookie；本进程是
/// headless 服务端（Docker / npm CLI / 裸二进制），没有 Tauri 的 `Webview` ⇒ 拿不到
/// 那份 Cookie（issue #111 §4）。
///
/// 三条路由共用同一句拒绝，理由见 [`api_companion_set_visible`] 的同一取舍：
/// **不假装成功**。尤其不能返回 `packs: []` —— 那会被前端读成「该账号没有资源包」，
/// 与「当前形态拿不到」是两件完全不同的事。
async fn api_qoder_web_desktop_only() -> Response {
    json_err(
        "Qoder 逐包明细需要桌面版：当前运行的是一体化服务端/命令行形态，没有内嵌浏览器，无法采集网页会话。聚合额度（剩余/已用/总量）在任意形态都可用。".to_string(),
        StatusCode::BAD_REQUEST,
    )
}

/// POST /api/companion/toggle —— 翻转可见性（webui 无此窗口，同上）。
async fn api_companion_toggle() -> Response {
    json_err(
        "桌面悬浮窗仅在桌面端可用".to_string(),
        StatusCode::BAD_REQUEST,
    )
}

/// POST /api/companion/jump —— 聚焦来源客户端（webui 无悬浮窗入口，同上）。
async fn api_companion_jump() -> Response {
    json_err(
        "桌面悬浮窗仅在桌面端可用".to_string(),
        StatusCode::BAD_REQUEST,
    )
}

// ---------------------------------------------------------------------------
// 外部 Agent 会话监听（P15 · hook 安装面）
// ---------------------------------------------------------------------------
//
// ⚠️ 与 `src-tauri/src/commands.rs` 的 `agent_hook_status` / `apply_agent_hook_enabled`
// **同款实现**（本仓惯例：两宿主各留一份 + 注释互指，不抽公共层 —— 抽了会让
// 「桌面要改、webui 不能改」这类真实差异无处安放）。

/// hook 的安装目标（与桌面侧同一份清单，见 `commands.rs` 的 `AGENT_HOOK_TARGETS`）。
///
/// ⚠️ **只列机制已取证的客户端**：WorkBuddy 两区域 + Claude Code + CodeBuddy CLI + Trae 两个区域。
/// `codex` / `qoder-*` 的 hook 路径或 schema **未一手取证** ⇒ 只作来源、不作安装目标
/// （往不存在的机制上写配置只会污染用户文件）。
///
/// ★ Trae 的目标是**它的 hooks 文件**（`~/.trae-cn/hooks.json` / `~/.trae/hooks.json`），
/// 不是 `settings.json` —— 该文件由客户端自己生成，**不存在时我们不创建**
/// （见 `agent_hook_install::missing_config_message`）。
const AGENT_HOOK_TARGETS: &[(&str, &str)] = &[
    ("workbuddy-cn", agent_watch::SOURCE_WORKBUDDY_CN),
    ("workbuddy-global", agent_watch::SOURCE_WORKBUDDY_GLOBAL),
    ("claude", agent_watch::SOURCE_CLAUDE),
    ("codebuddy", agent_watch::SOURCE_CODEBUDDY),
    ("trae-hooks-cn", agent_watch::SOURCE_TRAE_CN),
    ("trae-hooks-global", agent_watch::SOURCE_TRAE_GLOBAL),
];

/// 组装 hook 安装状态（**只读**，无副作用）。形状与桌面侧逐字段一致。
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
                "clientRunning": read.get("running").and_then(Value::as_bool).unwrap_or(false),
                "installed": !events.is_empty(),
                "events": events,
                "command": exe.as_deref().map(|exe| agent_hook_install::hook_command(exe, source)),
            })
        })
        .collect();

    json!({
        "enabled": app_prefs::watch_agent_sessions(),
        "port": buddy_switch_core::modules::agent_hook_rx::HOOK_PORT,
        "slots": slots,
    })
}

/// 装 / 卸 hook 并同步偏好。语义见桌面侧同名函数的文档（「装不上就不写偏好」）。
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

/// GET /api/agent-hooks/status —— hook 安装状态（**只读**，两宿主同源）。
async fn api_agent_hook_status() -> Response {
    json_ok(agent_hook_status())
}

/// POST /api/agent-hooks/enabled —— 开关监听（会**写上游客户端配置**，见 P15 决策 D41）。
async fn api_agent_hook_set_enabled(Json(body): Json<Value>) -> Response {
    let enabled = body
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // 写客户端配置文件是慢 IO ⇒ 放 blocking 线程，不阻塞 webui 的事件循环。
    match tokio::task::spawn_blocking(move || apply_agent_hook_enabled(enabled)).await {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("切换监听失败：{error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

// ---------------------------------------------------------------------------
// P15 · 「本工具自己的长操作」也送进悬浮窗（签到）
// ---------------------------------------------------------------------------
//
// ⚠️ 与 `src-tauri/src/commands.rs` 的 `observe_*` **同款实现**（本仓惯例：两宿主各留一份
// + 注释互指）。依据见 `P9-ARCH §1.3`：数据源本应有三路
//（`switch-progress` / `checkin-progress` / `trae-checkin-progress`），后两路**长期缺席**
// —— `src/lib/companion.ts` 一直在订阅一个从未被 emit 过的 `checkin-progress`。
//
// ★ webui **没有事件通道** ⇒ Trae 签到只能给**首尾两条**；桌面侧有逐账号进度。
//   那是两条通道的**能力差异**，如实反映，不在这一侧编造中间进度。

/// WorkBuddy 批量签到的**开始**一条，返回 task id（结束时用同一个 id 才能覆盖为终态）。
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

/// Trae 签到的**汇总**（`{ok, already, failed, total}`）→ 悬浮窗终态。
///
/// 桌面侧从 `done` 事件里取这几个字段、webui 侧从一次性报告里取 —— **字段名相同**，
/// 所以两条通道的终态判定共用一个来源，不可能分叉。
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

/// WorkBuddy 区域的展示名（**只用于标题**，不进 `source`）。
fn workbuddy_region_label(region: Region) -> &'static str {
    if region == Region::Global {
        "国际版"
    } else {
        "国内版"
    }
}

// ---------------------------------------------------------------------------
// 静态前端
// ---------------------------------------------------------------------------

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".js") || path.ends_with(".mjs") {
        "text/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}

/// 静态资源的缓存策略（按**被服务的资源名**判定，与 MIME 同源）。
///
/// - **`assets/*`** —— Vite 产物文件名带**内容哈希**（`index-C7PHZRwD.js`、
///   `bricolage-grotesque-latin-wght-normal-DLoelf7F.woff2`）：内容一变文件名就变，
///   所以可以放心长缓存 + `immutable`（连条件请求都省掉）。这是首屏 1.4 MB 里
///   除 `index.html` 之外的全部体积，此前**一个缓存头都没有**，每次刷新都重传。
/// - **其余（含 `index.html` 与 SPA 深链回退）** —— 必须 `no-cache`。
///   `index.html` 里引用的是**带哈希的资源名**：一旦它被缓存住，用户会一直拿到旧 HTML，
///   而旧 HTML 指向的旧哈希文件可能已被新构建删掉 ⇒ **白屏**。这与
///   `embedded_index_html_references_only_embedded_assets` 是同一类事故的两个面。
///
/// 注意 `no-cache` 不是 `no-store`：语义是「用前必须回源校验」。这里没有 ETag/Last-Modified，
/// 因此实际效果就是每次重新取（`index.html` 约 1 KB，代价可忽略），换取「永远不会陈旧」。
fn cache_control_for(served_path: &str) -> &'static str {
    if served_path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

async fn static_handler(uri: Uri) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() || path == "index.html" {
        path = "index.html".to_string();
    }
    // 前端路由回退到 index.html。
    //
    // Content-Type 必须由**实际被服务的资源名**推导，而不是请求路径：SPA 深链
    // （如 `/accounts`）在 embed 里落空后会回退到 index.html 的内容，但若仍按请求
    // 路径（无扩展名）算 MIME，会得到 `application/octet-stream`，浏览器会把 HTML
    // 当成二进制**下载**而不是渲染应用（`App.tsx` 用的是 `BrowserRouter`，深链是常态）。
    let (data, served_path) = match Assets::get(&path) {
        Some(file) => (Some(file), path),
        None => match Assets::get("index.html") {
            Some(file) => (Some(file), "index.html".to_string()),
            None => (None, path),
        },
    };
    match data {
        Some(f) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type(&served_path))
            .header(header::CACHE_CONTROL, cache_control_for(&served_path))
            .body(Body::from(f.data.into_owned()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    // 测试串行锁（`test_guard()`）故意持有到用例结束：它防的是多个用例同时改进程级
    // 状态（env / 全局配置 / 端口）。`#[tokio::test]` 默认 current-thread ⇒ 无误报风险。
    #![allow(clippy::await_holding_lock)]

    use super::{
        api_routes, cache_control_for, checkin_status_item, parse_region, parse_region_filter,
        query_flag_enabled, query_value, router, Assets,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request, StatusCode};
    use buddy_switch_core::modules::config::BUDDY_SWITCH_HOME_ENV;
    use buddy_switch_core::modules::region::{Region, RegionFilter};
    use serde_json::{json, Value};
    use std::collections::BTreeSet;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tower::ServiceExt;

    // -----------------------------------------------------------------------
    // 路由级测试脚手架
    //
    // 本 crate 是**二进制 crate**（`[[bin]]`，无 lib target），集成测试无法 import
    // `api_routes()`，故路由级测试放在此处的单元测试里（可访问私有 `api_routes`）。
    //
    // 关键：`gateway_host::shared_state()` 是进程级 `OnceLock`，且 `GatewayConfig::load()`
    // 每次实时读取 `home_dir()`。因此必须在**任何**触及 home 的调用之前，把
    // `BUDDY_SWITCH_HOME` 指向隔离目录——否则会读写真实 `~/.buddy-switch/`。
    // -----------------------------------------------------------------------

    /// 串行化所有路由测试：既避免并行修改进程级环境变量，也避免多个写操作测试
    /// 争抢同一份共享网关状态（Key 文件 / 配置）。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 进程内唯一的隔离 home（首次调用时创建并把 `BUDDY_SWITCH_HOME` 指向它）。
    static ISOLATED_HOME: OnceLock<std::path::PathBuf> = OnceLock::new();

    fn test_guard() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 返回隔离的 home 目录。必须作为每个测试的**第一步**调用。
    fn isolated_home() -> &'static std::path::Path {
        ISOLATED_HOME
            .get_or_init(|| {
                let dir = std::env::temp_dir().join(format!(
                    "buddy-switch-server-api-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                ));
                std::fs::create_dir_all(&dir).expect("create isolated home");
                // OnceLock 保证恰好设置一次，且对并发调用串行化。
                std::env::set_var(BUDDY_SWITCH_HOME_ENV, &dir);
                dir
            })
            .as_path()
    }

    fn exact_keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("value must be a JSON object")
            .keys()
            .cloned()
            .collect()
    }

    fn set_of(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|key| key.to_string()).collect()
    }

    fn assert_has_keys(value: &Value, keys: &[&str]) {
        let actual = exact_keys(value);
        for key in keys {
            assert!(
                actual.contains(*key),
                "response is missing key `{key}`: {actual:?}"
            );
        }
    }

    /// 是否含有连续 `minimum` 个十六进制字符的片段（用于抓 64 位 sha256 摘要）。
    fn contains_hex_run(text: &str, minimum: usize) -> bool {
        let mut run = 0usize;
        for ch in text.chars() {
            if ch.is_ascii_hexdigit() {
                run += 1;
                if run >= minimum {
                    return true;
                }
            } else {
                run = 0;
            }
        }
        false
    }

    /// 驱动 `api_routes()`（**不含 fallback**）并返回 (状态码, JSON)。
    async fn call_api(method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = match body {
            Some(value) => Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&value).unwrap()))
                .unwrap(),
            None => Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        };
        let response = api_routes().oneshot(request).await.expect("router call");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    /// 驱动完整 `router()`（含静态 fallback），返回 (状态码, content-type, body 文本)。
    async fn call_full(method: Method, uri: &str) -> (StatusCode, String, String) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:57890")
            .body(Body::empty())
            .unwrap();
        let response = router().oneshot(request).await.expect("router call");
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (
            status,
            content_type,
            String::from_utf8_lossy(&bytes).to_string(),
        )
    }

    /// 驱动**完整** `router()`（含 `api_guard` 中间件），只回状态码。
    ///
    /// 三个 header 分别对应 guard 的三条判据，`None` 表示该头**不发送**。
    async fn status_with_headers(
        method: Method,
        uri: &str,
        host: Option<&str>,
        origin: Option<&str>,
        sec_fetch_site: Option<&str>,
        content_type: Option<&str>,
    ) -> StatusCode {
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in [
            ("host", host),
            ("origin", origin),
            ("sec-fetch-site", sec_fetch_site),
            ("content-type", content_type),
        ] {
            if let Some(value) = value {
                builder = builder.header(name, value);
            }
        }
        router()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .expect("router call")
            .status()
    }

    /// ★★ 安全审计 P0-1：**`api_guard` 必须真的挂在 `router()` 上**。
    ///
    /// 这条是「中间件写好了但没接线」的唯一防线 —— `api_guard` 自己的单测只能证明
    /// `judge()` 判得对，证明不了它被装进了管理面。变异验证：把 `router()` 里的
    /// `.layer(...)` 去掉，本用例立刻红。
    #[tokio::test]
    async fn management_surface_rejects_foreign_host_and_cross_site_requests() {
        let _home = isolated_home();

        // ① 正常同源访问 ⇒ 放行。
        assert_ne!(
            status_with_headers(
                Method::GET,
                "/api/status",
                Some("127.0.0.1:57890"),
                None,
                None,
                None
            )
            .await,
            StatusCode::FORBIDDEN,
            "回环 Host 必须放行"
        );

        // ② DNS rebinding 形态：Host 与 Origin **同源**，但主机名是攻击者的。
        assert_eq!(
            status_with_headers(
                Method::GET,
                "/api/status",
                Some("attacker.com:57890"),
                Some("http://attacker.com:57890"),
                None,
                None,
            )
            .await,
            StatusCode::FORBIDDEN,
            "同源 Origin 不是防线，Host 必须是回环"
        );

        // ③ 普通跨站（Host 是回环、Origin 是别人的）⇒ 也拒。
        assert_eq!(
            status_with_headers(
                Method::GET,
                "/api/status",
                Some("127.0.0.1:57890"),
                Some("http://evil.example"),
                None,
                None,
            )
            .await,
            StatusCode::FORBIDDEN
        );

        // ④ 完全没有 Host（HTTP/1.0 / 伪造）⇒ 拒。
        assert_eq!(
            status_with_headers(Method::GET, "/api/status", None, None, None, None).await,
            StatusCode::FORBIDDEN
        );

        // ⑤ 写接口必须是 JSON —— 挡「`text/plain` 简单请求绕过预检」的 CSRF。
        assert_eq!(
            status_with_headers(
                Method::POST,
                "/api/accounts/remark",
                Some("127.0.0.1:57890"),
                None,
                None,
                Some("text/plain"),
            )
            .await,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );

        // ⑥ 静态资源（fallback）**不受**管理面校验影响：公开可读，且 GET 没有 Content-Type。
        let (status, _, _) = call_full(Method::GET, "/").await;
        assert_ne!(
            status,
            StatusCode::FORBIDDEN,
            "SPA 首页不能被管理面校验拦掉（它不在 /api/* 上）"
        );
    }

    /// 取响应的 `Cache-Control`（无该头时返回空串）。
    async fn call_cache_control(uri: &str) -> (StatusCode, String) {
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("host", "127.0.0.1:57890")
            .body(Body::empty())
            .unwrap();
        let response = router().oneshot(request).await.expect("router call");
        let status = response.status();
        let value = response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        (status, value)
    }

    // -----------------------------------------------------------------------
    // 纯函数
    // -----------------------------------------------------------------------

    #[test]
    fn parse_region_defaults_to_cn_and_binds_known_values() {
        assert_eq!(
            parse_region(None),
            Region::Cn,
            "missing region must default to CN"
        );
        assert_eq!(parse_region(Some("cn")), Region::Cn);
        assert_eq!(parse_region(Some("global")), Region::Global);
        assert_eq!(parse_region(Some("ai")), Region::Global);
        assert_eq!(parse_region(Some("GLOBAL")), Region::Global);
        // 未知 / 空值回落到 CN（保证旧行为）。
        assert_eq!(parse_region(Some("")), Region::Cn);
        assert_eq!(parse_region(Some("xx")), Region::Cn);
        assert_ne!(parse_region(None), Region::Global);
    }

    #[test]
    fn parse_region_filter_supports_all_and_keeps_legacy_bindings() {
        // 与既有 `parse_region` 绑定规则一致。
        assert_eq!(parse_region_filter(None), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("")), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("xx")), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("cn")), RegionFilter::Cn);
        assert_eq!(parse_region_filter(Some("ai")), RegionFilter::Global);
        assert_eq!(parse_region_filter(Some("GLOBAL")), RegionFilter::Global);
        // 新增：合并视图。
        assert_eq!(parse_region_filter(Some("all")), RegionFilter::All);
        assert_eq!(parse_region_filter(Some("ALL")), RegionFilter::All);
        assert_eq!(parse_region_filter(Some("合并")), RegionFilter::All);
    }

    #[test]
    fn query_value_reads_first_match_and_handles_missing() {
        assert_eq!(
            query_value(Some("region=global"), "region").as_deref(),
            Some("global")
        );
        assert_eq!(
            query_value(Some("a=1&region=global&b=2"), "region").as_deref(),
            Some("global")
        );
        assert_eq!(query_value(Some("a=1"), "region"), None);
        assert_eq!(query_value(None, "region"), None);
        assert_eq!(query_value(Some("flag"), "flag").as_deref(), Some(""));
        assert_eq!(query_value(Some("x=1&x=2"), "x").as_deref(), Some("1"));
    }

    /// GET 参数必须解码：Windows 路径经 `URLSearchParams` 编码后要能还原。
    ///
    /// 这条是 2026-10-28 的缺陷回归测试：不解码时 `path=D%3A%5Cx` 会原样传给后端，
    /// 桌面端（IPC）却正常 —— 两个通道语义分叉，且只在「传路径」时暴露。
    #[test]
    fn query_value_decodes_percent_and_plus() {
        assert_eq!(
            query_value(Some("path=D%3A%5Ctrae-plain%5Cdatabase.db"), "path").as_deref(),
            Some(r"D:\trae-plain\database.db")
        );
        // `URLSearchParams` 把空格编码成 `+`（不是 `%20`），必须还原。
        assert_eq!(
            query_value(Some("path=C%3A%5CProgram+Files%5Cx.db"), "path").as_deref(),
            Some(r"C:\Program Files\x.db")
        );
        // 非法的 `%` 序列原样保留（不静默丢字符）。
        assert_eq!(
            query_value(Some("q=100%ZZ"), "q").as_deref(),
            Some("100%ZZ")
        );
        // 中文路径（多字节 UTF-8）也要能还原。
        assert_eq!(
            query_value(Some("path=%E6%A1%8C%E9%9D%A2%5Cx.db"), "path").as_deref(),
            Some("桌面\\x.db")
        );
    }

    #[test]
    fn query_flag_enabled_matches_truthy_forms() {
        assert!(query_flag_enabled(Some("refresh"), "refresh"));
        assert!(query_flag_enabled(Some("refresh="), "refresh"));
        assert!(query_flag_enabled(Some("refresh=1"), "refresh"));
        assert!(query_flag_enabled(Some("refresh=true"), "refresh"));
        assert!(query_flag_enabled(Some("refresh=yes"), "refresh"));
        assert!(!query_flag_enabled(Some("refresh=false"), "refresh"));
        assert!(!query_flag_enabled(Some("refresh=0"), "refresh"));
        assert!(!query_flag_enabled(Some("refresh=no"), "refresh"));
        assert!(!query_flag_enabled(Some("other=1"), "refresh"));
        assert!(!query_flag_enabled(None, "refresh"));
        assert!(query_flag_enabled(Some("a=1&refresh=yes"), "refresh"));
    }

    // -----------------------------------------------------------------------
    // 只读路由
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn core_read_only_routes_return_expected_shapes() {
        let _guard = test_guard();
        isolated_home();

        let (status, body) = call_api(Method::GET, "/api/status", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_has_keys(
            &body,
            &[
                "running",
                "region",
                "authFile",
                "current",
                "appPath",
                "version",
                "installed",
                "regionMismatch",
            ],
        );
        assert_eq!(body["region"], json!("cn"));
        // E3.3：契约必须产出 `installed` / `regionMismatch` 两个字段（前端读点）。
        assert!(
            body["installed"].is_boolean(),
            "status.installed 必须是布尔：{body}"
        );
        assert!(
            body["regionMismatch"].is_null(),
            "隔离 home 无认证文件，regionMismatch 必须为 null：{body}"
        );

        let (status, body) = call_api(Method::GET, "/api/accounts", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_has_keys(&body, &["region", "accounts", "current"]);
        assert_eq!(body["region"], json!("cn"));
        // 空库上的 `== []` 无法证伪「账号映射」是否真的生效（数据层回归不可见），
        // 故此处仅断言形状；内容映射由
        // `accounts_route_returns_seeded_account_contents` 播种后精确断言。
        assert!(
            body["accounts"].is_array(),
            "accounts must be a JSON array: {body}"
        );
        assert_eq!(body["current"], Value::Null);

        let (status, body) = call_api(Method::GET, "/api/sessions", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_has_keys(&body, &["sessions", "current"]);
        // 空库上的 `== []` 无法证伪「uid → sessions 表 → 响应」这条链路，
        // 故此处仅断言形状；内容映射由
        // `sessions_route_returns_seeded_session_rows` 播种真实 workbuddy.db 后精确断言。
        assert!(
            body["sessions"].is_array(),
            "sessions must be a JSON array: {body}"
        );

        let (status, body) = call_api(Method::GET, "/api/checkin/status", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(exact_keys(&body), set_of(&["accounts"]));
        // 空库上的 `== []` 无法证伪「账号 → 条目」的映射（`checkin_status_item` 拼的
        // `accountId` / `email` 两个字段此前完全没被断言过），故此处仅断言形状；
        // 映射由 `checkin_status_route_maps_seeded_account_identity` 播种账号后精确断言。
        assert!(
            body["accounts"].is_array(),
            "checkin accounts must be a JSON array: {body}"
        );

        let (status, body) = call_api(Method::GET, "/api/switch/progress", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(exact_keys(&body), set_of(&["running", "progress"]));
        assert_eq!(body["running"], json!(false));
    }

    /// Q1 修复：`/api/accounts` 的「账号库 → 响应」映射必须在**有数据**时断言，
    /// 否则空隔离库上的 `== []` 恒真、无法发现数据层回归。此处播种 1 条 CN 账号，
    /// 断言返回**恰好该条**且具体字段正确；断言前清理，保持其它用例的空库前置
    /// （否则 `/api/checkin/status` 会因此对播种账号发起真实网络请求）。
    #[tokio::test]
    async fn accounts_route_returns_seeded_account_contents() {
        let _guard = test_guard();
        let home = isolated_home();

        let accounts_file = home.join(".buddy-switch").join("accounts.json");
        std::fs::create_dir_all(accounts_file.parent().unwrap()).expect("create store dir");
        std::fs::write(
            &accounts_file,
            r#"[{"id":"seeded-cn-1","uid":"uid-seeded","nickname":"Seeded CN","email":"seeded@example.com"}]"#,
        )
        .expect("seed accounts");

        let (status, body) = call_api(Method::GET, "/api/accounts", None).await;
        // 先清理，避免种子泄漏到其它用例（即便断言随后失败也不会污染后续用例）。
        std::fs::remove_file(&accounts_file).expect("remove seeded accounts");

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["region"], json!("cn"));
        let accounts = body["accounts"].as_array().expect("accounts array");
        assert_eq!(
            accounts.len(),
            1,
            "a seeded store must surface exactly one account: {body}"
        );
        assert_eq!(accounts[0]["id"], json!("seeded-cn-1"));
        assert_eq!(accounts[0]["uid"], json!("uid-seeded"));
        assert_eq!(accounts[0]["email"], json!("seeded@example.com"));
        assert_eq!(accounts[0]["nickname"], json!("Seeded CN"));
        assert_eq!(accounts[0]["needsRelogin"], json!(false));
    }

    /// 替换 `/api/checkin/status` 的空态断言：播种一个账号，断言**真实的映射**。
    ///
    /// ## 为什么用 Global 区域
    ///
    /// `checkin::get_checkin_status_for` 对 Global 是**不联网的短路分支**（直接返回
    /// Unsupported）⇒ 既能断言「账号 → 条目」的映射，又**不会打真实上游**。
    /// 这正是原注释里「播种会触发真实网络请求」的死结的出口：CN 才需要联网。
    ///
    /// 修前该用例无法存在（原断言是空库上的 `accounts == []`，恒真）——
    /// `checkin_status_item` 拼的 `accountId` / `email` 两个字段此前**没有任何断言**，
    /// 把它删掉也不会有测试变红。
    #[tokio::test]
    async fn checkin_status_route_maps_seeded_account_identity() {
        let _guard = test_guard();
        isolated_home();

        seed_accounts(
            Region::Global,
            json!([{
                "id": "global-checkin-1",
                "uid": "uid-global-checkin",
                "email": "gc@example.com"
            }]),
        );

        let (status, body) = call_api(Method::GET, "/api/checkin/status?region=global", None).await;
        // 先清理再断言：即便断言失败也不把种子留给后续用例。
        clear_accounts(Region::Global);

        assert_eq!(status, StatusCode::OK);
        let items = body["accounts"].as_array().expect("accounts array");
        assert_eq!(items.len(), 1, "播种一个账号必须恰好回一条：{body}");

        // `checkin_status_item` 的两个映射字段（`accountId` ← 账号 id、`email` ← 展示名）
        assert_eq!(
            items[0]["accountId"],
            json!("global-checkin-1"),
            "accountId 必须来自账号 id：{body}"
        );
        assert_eq!(
            items[0]["email"],
            json!("gc@example.com"),
            "email 必须来自 account_display_name（email 优先）：{body}"
        );

        // Global 的短路分支必须**明确报不支持**，而不是伪造一个「已签到」。
        assert_eq!(items[0]["ok"], json!(false), "Global 不得伪造成功：{body}");
        assert!(
            items[0]["error"].is_string(),
            "Global 必须带明确错误文案：{body}"
        );
    }

    /// 替换 `/api/gateway/logs` 的空态断言：写入一条日志，断言它能被读回。
    ///
    /// 原断言是空日志上的 `logs == []`（恒真）——把 `list()` 换成 `Vec::new()`、
    /// 甚至把整个 handler 写死空数组，都不会有任何测试变红。
    #[tokio::test]
    async fn gateway_logs_route_returns_recorded_entries() {
        let _guard = test_guard();
        isolated_home();

        let state = crate::gateway_host::shared_state();
        // 共享状态是进程级 `OnceLock`，先清空避免受其它用例残留影响。
        state.log.clear();
        state.log.record(json!({
            "ts": 1_700_000_000_000i64,
            "endpoint": "/v1/chat/completions",
            "method": "POST",
            "region": "cn",
            "account": "seeded-account",
            "model": "hy4-preview",
            "status": 200,
            "latencyMs": 42,
            "promptTokens": 7,
            "completionTokens": 9,
            "stream": false,
        }));

        let (status, body) = call_api(Method::GET, "/api/gateway/logs", None).await;
        state.log.clear();

        assert_eq!(status, StatusCode::OK);
        let logs = body["logs"].as_array().expect("logs array");
        assert_eq!(logs.len(), 1, "写入一条就必须读回一条：{body}");
        assert_eq!(logs[0]["endpoint"], json!("/v1/chat/completions"));
        assert_eq!(logs[0]["model"], json!("hy4-preview"));
        assert_eq!(logs[0]["status"], json!(200));
        assert_eq!(logs[0]["promptTokens"], json!(7));
    }

    /// 端到端证明「池治理状态确实会落盘」——补上此前**完全缺失**的那条路径。
    ///
    /// ## 回归背景（2026-09-28 实测发现）
    ///
    /// `Pool::flush_if_dirty` 存在、类型全对、单测全绿，但**生产上没有任何调用点**：
    /// 唯一的包装 `GatewayState::persist_pool` 自身零调用者。而 `Pool::load` 在启动时
    /// 读的正是它写的那个文件 ⇒ **文件永远是旧的**，冷却 / 熔断 / 成功率 EMA /
    /// 余额读数 / `credits_refreshed_ms` 每次重启全部静默归零。
    ///
    /// 这与注册表文档里记的 `set_credits` 事故是**同一类**（「函数存在但没人调用」）。
    /// 本用例走的就是后台 `BackgroundTask::PoolPersist` 循环调用的那条路：
    /// 改池 → `persist_pool()` → 文件里真的出现该账号。
    #[tokio::test]
    async fn persist_pool_writes_governance_state_to_disk() {
        let _guard = test_guard();
        isolated_home();

        let state = crate::gateway_host::shared_state();
        let state_file = buddy_switch_gateway::state::gateway_state_file();
        let _ = std::fs::remove_file(&state_file);

        {
            let mut pool = state.pool.write().await;
            pool.sync_accounts(
                &[json!({
                    "uid": "persist-probe",
                    "email": "persist@example.com",
                    "jwt": "jwt-persist-probe"
                })],
                None,
            );
            // 制造一次真实治理改动（`apply_upstream_error` 会置 `dirty`）。
            pool.apply_upstream_error(
                "persist-probe",
                "hy4-preview",
                &buddy_switch_gateway::pool::UpstreamEvent::SoftRate {
                    reset_at_ms: None,
                    model_scoped: false,
                },
                1_700_000_000_000,
            );
        }

        state.persist_pool().await;

        let text = std::fs::read_to_string(&state_file)
            .unwrap_or_else(|error| panic!("落盘后 {state_file:?} 必须存在: {error}"));
        assert!(
            text.contains("persist-probe"),
            "状态文件必须含被治理过的账号（否则 `Pool::load` 读到的永远是旧内容）：{text}"
        );

        let _ = std::fs::remove_file(&state_file);
    }

    /// 回归护栏（issue #2 / win11）：**展示字段若不是字符串，接口不得原样透出**。
    ///
    /// 现场：CN `workbuddy-desktop.info` 的 `account.nickname` 不是字符串（对象）⇒
    /// ① 区域 Tab 用模板串拼出「已登录: [object Object]」；
    /// ② 「当前账号：{name}」把同一个值当 **React 子节点**渲染 ⇒ React 抛
    ///    「Objects are not valid as a React child」⇒ 没有 ErrorBoundary ⇒ 整棵树
    ///    卸载 ⇒ 窗口一片白（用户看到的现象）。
    ///
    /// 这里钉的是**形状不变量**而不是某个具体字段：`current` 的每个展示字段
    /// 只能是字符串或 null。三种非字符串形态（对象 / 数组 / 数字）一起覆盖，
    /// 因为「后端某天把数字写进 nickname」与「对象」对前端是同一类事故
    /// （前者会 `[object Object]` 之外还让 `{name}.trim()` 之类直接抛错）。
    #[tokio::test]
    async fn status_current_never_leaks_non_string_display_fields() {
        let _guard = test_guard();
        isolated_home();

        // 凭据域留空 ⇒ `region_of("")` 判定为 Cn（与真实 CN 认证文件同 region），
        // 否则会被安全红线 F 拦成 regionMismatch、`current` 直接为 null，用例恒真。
        let auth_file = buddy_switch_core::modules::auth_file::auth_file_path_for(Region::Cn);
        std::fs::create_dir_all(auth_file.parent().unwrap()).expect("create auth dir");

        // ---- 第一段：非字符串形态（对象 / 数组）不得透出 ----
        std::fs::write(
            &auth_file,
            r#"{"account":{"uid":{"nested":true},"nickname":{"zh":"小明"},"email":["a@b.c"]},"auth":{"accessToken":"tok"}}"#,
        )
        .expect("seed auth file");

        let (status, body) = call_api(Method::GET, "/api/status?region=cn", None).await;
        assert_eq!(status, StatusCode::OK);
        let current = &body["current"];
        assert!(
            current.is_object(),
            "认证文件存在且域匹配时 current 必须是对象：{body}"
        );
        for key in ["uid", "nickname", "email"] {
            let value = &current[key];
            assert!(
                value.is_null() || value.is_string(),
                "status.current.{key} 必须是字符串或 null，实际透出了 {value}：{body}"
            );
        }

        // ---- 第二段（阳性对照）：合法值不得被归一误伤 ----
        // 纯数字昵称是**合法数据**（有人昵称就是数字），必须保留成字符串。
        // 没有这一段的话，「无脑全置 null」的偷懒实现也能让上面的断言全绿。
        std::fs::write(
            &auth_file,
            r#"{"account":{"uid":"u-1","nickname":12345,"email":"a@b.c"},"auth":{"accessToken":"tok"}}"#,
        )
        .expect("reseed auth file");

        let (status, body) = call_api(Method::GET, "/api/status?region=cn", None).await;
        // 最后再清理，避免种子泄漏到其它用例。
        let _ = std::fs::remove_file(&auth_file);

        assert_eq!(status, StatusCode::OK);
        let current = &body["current"];
        assert_eq!(
            current["uid"],
            json!("u-1"),
            "字符串字段必须原样保留：{body}"
        );
        assert_eq!(
            current["nickname"],
            json!("12345"),
            "数字昵称必须归一成字符串、而不是被丢掉：{body}"
        );
        assert_eq!(
            current["email"],
            json!("a@b.c"),
            "字符串字段必须原样保留：{body}"
        );
    }

    /// ★ 新版客户端的加密昵称：`/api/status` 的 `current.nickname` 必须回落**账号库**。
    ///
    /// 现场（2026-09-24 用户截图）：认证文件里 `account.nickname` 是
    /// `{"$wbEncrypted":1,"envelope":"…"}` ⇒ `display_str` 给 `null` ⇒
    /// 区域页签拼出「已登录: 31da0a95-6637-4f5e-adee-f7f08a6f86fd」（一串 UUID）。
    /// 账号库里本来就有同一个 uid 的昵称，按 uid 取回来即可。
    ///
    /// **阳性对照**（第 3 段）是这条用例的关键：它排「无条件覆盖」的偷懒实现 ——
    /// 老客户端明文昵称必须压过账号库里的（可能是过期的）名字。
    #[tokio::test]
    async fn status_current_nickname_falls_back_to_account_library() {
        let _guard = test_guard();
        isolated_home();

        let auth_file = buddy_switch_core::modules::auth_file::auth_file_path_for(Region::Cn);
        std::fs::create_dir_all(auth_file.parent().unwrap()).expect("create auth dir");

        // ---- 第一段：认证文件读不到昵称（加密信封）⇒ 用账号库里的名字 ----
        seed_accounts(
            Region::Cn,
            json!([{
                "id": "seeded-cn-1", "uid": "uid-encrypted", "nickname": "Jackey",
                "access_token": "AT",
            }]),
        );
        std::fs::write(
            &auth_file,
            r#"{"account":{"uid":"uid-encrypted","nickname":{"$wbEncrypted":1,"envelope":"eyJzdWl0ZSI6MX0="}},"auth":{"accessToken":"tok"}}"#,
        )
        .expect("seed auth file");

        let (status, body) = call_api(Method::GET, "/api/status?region=cn", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["current"]["nickname"],
            json!("Jackey"),
            "读不到认证文件昵称时必须回落账号库，否则界面只能显示 uid：{body}"
        );
        assert_eq!(body["current"]["uid"], json!("uid-encrypted"));

        // ---- 第二段：账号库里也没有该 uid ⇒ 如实落 null，把回落链留给前端 ----
        std::fs::write(
            &auth_file,
            r#"{"account":{"uid":"uid-unknown","nickname":{"$wbEncrypted":1}},"auth":{"accessToken":"tok"}}"#,
        )
        .expect("reseed auth file");
        let (status, body) = call_api(Method::GET, "/api/status?region=cn", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["current"]["nickname"],
            Value::Null,
            "库里没有该账号时不得凭空造名字：{body}"
        );

        // ---- 第三段（阳性对照）：认证文件有明文昵称 ⇒ 必须原样用，不被库覆盖 ----
        std::fs::write(
            &auth_file,
            r#"{"account":{"uid":"uid-encrypted","nickname":"认证文件里的名字"},"auth":{"accessToken":"tok"}}"#,
        )
        .expect("reseed auth file");
        let (status, body) = call_api(Method::GET, "/api/status?region=cn", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["current"]["nickname"],
            json!("认证文件里的名字"),
            "认证文件读得到昵称时不得被账号库覆盖：{body}"
        );

        // 清理，避免种子泄漏到其它用例。
        let _ = std::fs::remove_file(&auth_file);
        clear_accounts(Region::Cn);
    }

    /// 同一条不变量的**账号库**版本：`account_meta` 是脱敏白名单，白名单里的展示
    /// 字段同样只能是字符串或 null。`/api/accounts` 与 Tauri `get_accounts` 共用它，
    /// 所以这里断言的就是桌面端账号卡片拿到的东西。
    ///
    /// 现场对照：卡片用 `const name = account.nickname || account.uid || "未命名账号"`
    /// 直接渲染 `{name}`，对象会走与上面完全相同的崩溃路径；
    /// `account.email.split("@")`、`account.remark?.trim()` 则会直接抛 TypeError。
    #[tokio::test]
    async fn account_meta_never_leaks_non_string_display_fields() {
        let _guard = test_guard();
        let home = isolated_home();

        let accounts_file = home.join(".buddy-switch").join("accounts.json");
        std::fs::create_dir_all(accounts_file.parent().unwrap()).expect("create store dir");

        // ---- 第一段：白名单里每个展示字段都喂脏值（`id` 也要覆盖 ——
        // 导入文件可以自带 `id`，见 `export_import::merge_import_record`）----
        std::fs::write(
            &accounts_file,
            r#"[{"id":{"nested":true},"uid":{"nested":true},"nickname":{"zh":"小明"},"email":["a@b.c"],
                "enterpriseName":{"name":"某公司"},"needs_relogin_reason":{"code":1},"remark":{"text":"备注"}}]"#,
        )
        .expect("seed accounts");

        let (status, body) = call_api(Method::GET, "/api/accounts", None).await;
        assert_eq!(status, StatusCode::OK);
        let account = &body["accounts"][0];
        for key in [
            "id",
            "uid",
            "nickname",
            "email",
            "enterpriseName",
            "needsReloginReason",
            "remark",
        ] {
            let value = &account[key];
            assert!(
                value.is_null() || value.is_string(),
                "accounts[0].{key} 必须是字符串或 null，实际透出了 {value}：{body}"
            );
        }

        // ---- 第二段（阳性对照）：干净数据必须逐字不变，且**时间戳不得被字符串化** ----
        // `expiresAt` 必须保持数字，否则 `account-card.tsx` 的
        // `typeof account.expiresAt === "number"` 会静默失效（过期提示再也不出现）。
        std::fs::write(
            &accounts_file,
            r#"[{"id":"a1","uid":"u1","nickname":12345,"email":"x@y.z","enterpriseName":"某公司",
                "needs_relogin":true,"needs_relogin_reason":"刷新失败","remark":"备注","expiresAt":123456}]"#,
        )
        .expect("reseed accounts");

        let (status, body) = call_api(Method::GET, "/api/accounts", None).await;
        let _ = std::fs::remove_file(&accounts_file);

        assert_eq!(status, StatusCode::OK);
        let account = &body["accounts"][0];
        assert_eq!(account["id"], json!("a1"), "字符串 id 必须原样保留：{body}");
        assert_eq!(
            account["uid"],
            json!("u1"),
            "字符串 uid 必须原样保留：{body}"
        );
        assert_eq!(
            account["nickname"],
            json!("12345"),
            "数字昵称必须归一成字符串、而不是被丢掉：{body}"
        );
        assert_eq!(account["email"], json!("x@y.z"));
        assert_eq!(account["enterpriseName"], json!("某公司"));
        assert_eq!(account["needsRelogin"], json!(true));
        assert_eq!(account["needsReloginReason"], json!("刷新失败"));
        assert_eq!(account["remark"], json!("备注"));
        assert_eq!(
            account["expiresAt"],
            json!(123456),
            "时间戳必须保持数字（前端按 number 判定过期）：{body}"
        );
    }

    /// 修复 `/api/sessions` 的空态断言（原 `sessions == []` 跑在「没有 workbuddy.db」的
    /// 隔离 home 上，**恒真、打不红**，无法发现「认证 uid → sessions 表 → 响应」这条
    /// 链路上的任何回归）。此处播种一个真实的 `workbuddy.db` 与认证文件，断言**恰好**
    /// 返回应返回的两条及其字段值，并覆盖 4 类必须被过滤 / 跳过的行。
    #[tokio::test]
    async fn sessions_route_returns_seeded_session_rows() {
        const UID: &str = "uid-sessions-seeded";

        let _guard = test_guard();
        let home = isolated_home();

        // 1) 播种 CN 会话库 `<home>/.workbuddy/workbuddy.db`。
        let data_dir = home.join(".workbuddy");
        let project_dir = data_dir.join("projects").join("ws-beta");
        std::fs::create_dir_all(&project_dir).expect("create projects dir");

        let db_path = data_dir.join("workbuddy.db");
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open seeded db");
            conn.execute_batch(
                "CREATE TABLE sessions (
                    id TEXT, user_id TEXT, cwd TEXT, title TEXT, custom_title TEXT,
                    updated_at INTEGER, deleted_at INTEGER, is_playground INTEGER
                 );",
            )
            .expect("create sessions table");
            let insert = "INSERT INTO sessions \
                 (id, user_id, cwd, title, custom_title, updated_at, deleted_at, is_playground) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";
            let seed = |id: &str,
                        uid: &str,
                        cwd: &str,
                        title: &str,
                        custom: Option<&str>,
                        updated_at: i64,
                        deleted_at: Option<i64>,
                        playground: i64| {
                conn.execute(
                    insert,
                    rusqlite::params![
                        id, uid, cwd, title, custom, updated_at, deleted_at, playground
                    ],
                )
                .expect("insert seeded session");
            };
            // 可见 1：普通会话，无 custom_title；无正文 jsonl → hasHistory=false。
            seed(
                "cid-alpha",
                UID,
                "/home/u/ws-alpha",
                "Alpha 原标题",
                None,
                2_000,
                None,
                0,
            );
            // 可见 2：custom_title 应**覆盖** title；is_playground=1 → isPlayground=true。
            seed(
                "cid-beta",
                UID,
                "/home/u/ws-beta",
                "Beta 原标题",
                Some("Beta 自定义名"),
                3_000,
                None,
                1,
            );
            // 过滤 3：已软删除（updated_at 最新，若漏过滤会排在首位，最易被发现）。
            seed(
                "cid-deleted",
                UID,
                "/home/u/ws-alpha",
                "已删除",
                None,
                4_000,
                Some(1_000),
                0,
            );
            // 过滤 4：属于其它账号。
            seed(
                "cid-otheruser",
                "uid-other",
                "/home/u/ws-alpha",
                "别人的",
                None,
                5_000,
                None,
                0,
            );
            // 跳过 5：claw 工作区（账号绑定 IM 渠道，换账号后不可用）。
            seed(
                "cid-claw",
                UID,
                "/home/u/claw",
                "Claw",
                None,
                6_000,
                None,
                0,
            );
        }
        // cid-beta 有正文 jsonl → hasHistory=true；cid-alpha 没有 → false。
        std::fs::write(
            project_dir.join("cid-beta.jsonl"),
            "{\"sessionId\":\"cid-beta\"}\n",
        )
        .expect("seed session jsonl");

        // 2) 播种 CN 认证文件：`current_user_uid()` 正是从这里取 uid。
        let auth_file = buddy_switch_core::modules::auth_file::auth_file_path_for(Region::Cn);
        std::fs::create_dir_all(auth_file.parent().unwrap()).expect("create auth dir");
        std::fs::write(&auth_file, format!(r#"{{"account":{{"uid":"{UID}"}}}}"#))
            .expect("seed auth file");

        // 3) 驱动路由，然后**先清理**再断言——即便断言失败也不污染后续用例
        //    （`core_read_only_routes_return_expected_shapes` 仍以空库为前提）。
        let (status, body) = call_api(Method::GET, "/api/sessions", None).await;
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_file(&auth_file);

        assert_eq!(status, StatusCode::OK);
        // `source` / `warning` 由 `list_sessions_with_fallback_for` 透传：db 可读时为
        // `source:"db"`、`warning:null`（降级扫描时 warning 才为非空文本）。
        assert_eq!(
            exact_keys(&body),
            set_of(&["sessions", "current", "source", "warning"])
        );
        assert_eq!(
            body["current"],
            json!(UID),
            "current must come from the seeded auth file"
        );
        assert_eq!(body["source"], json!("db"), "db 可读时应标 source=db");

        let sessions = body["sessions"].as_array().expect("sessions array");
        let ids: Vec<&str> = sessions.iter().filter_map(|s| s["id"].as_str()).collect();
        // 恰好两条：软删除 / 其它账号被过滤，claw 被跳过。
        assert_eq!(
            ids,
            vec!["cid-beta", "cid-alpha"],
            "must return exactly the visible, non-claw sessions ordered by updated_at DESC: {body}"
        );

        let beta = &sessions[0];
        assert_eq!(
            exact_keys(beta),
            set_of(&[
                "id",
                "title",
                "cwd",
                "updatedAt",
                "hasHistory",
                "isPlayground"
            ]),
            "session item key set must be pinned"
        );
        assert_eq!(
            beta["title"],
            json!("Beta 自定义名"),
            "custom_title must win over title"
        );
        assert_eq!(beta["cwd"], json!("/home/u/ws-beta"));
        assert_eq!(beta["updatedAt"], json!(3_000));
        assert_eq!(
            beta["hasHistory"],
            json!(true),
            "cid-beta has a project jsonl"
        );
        assert_eq!(beta["isPlayground"], json!(true));

        let alpha = &sessions[1];
        assert_eq!(
            alpha["title"],
            json!("Alpha 原标题"),
            "no custom_title → fall back to title"
        );
        assert_eq!(alpha["updatedAt"], json!(2_000));
        assert_eq!(
            alpha["hasHistory"],
            json!(false),
            "cid-alpha has no project jsonl"
        );
        assert_eq!(alpha["isPlayground"], json!(false));
    }

    #[tokio::test]
    async fn gateway_read_only_routes_expose_pinned_contracts() {
        let _guard = test_guard();
        isolated_home();

        // status：键集合精确匹配（E3 契约，snake_case）。
        let (status, body) = call_api(Method::GET, "/api/gateway/status", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            exact_keys(&body),
            set_of(&[
                "enabled",
                "running",
                "addr",
                "base_url",
                "bind_addr",
                "port",
                "allow_non_loopback",
                "version",
                // 账号池视图 + 最近失败原因（与 Trae 侧 `trae_gateway_status` 对位）。
                "pools",
                "accounts",
                "diagnose",
                "last_error",
            ]),
            "gateway_status key set must be pinned (snake_case)"
        );
        // 前端归一化读取 base_url；不得再出现 camelCase 键。
        assert!(
            body["base_url"].is_string(),
            "base_url must be present for the frontend"
        );
        assert!(!body.as_object().unwrap().contains_key("baseUrl"));
        // `pools` 的两个版本键**恒定存在**：空池不是「未知」，前端据此直接渲染两张卡。
        assert_eq!(
            exact_keys(&body["pools"]),
            set_of(&["cn", "global"]),
            "pools 必须恒含 cn / global 两个键"
        );
        assert!(body["accounts"].is_array(), "accounts 必须是数组（可为空）");
        assert!(body["diagnose"].is_array(), "diagnose 必须是数组（可为空）");

        // config：snake_case 键集合。
        let (status, body) = call_api(Method::GET, "/api/gateway/config", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            exact_keys(&body),
            set_of(&[
                "enabled",
                "bind_addr",
                "port",
                "allow_non_loopback",
                "log_keep",
                "log_bodies",
                "per_key_rate_limit",
                "max_rotate",
                "sanitize_fingerprints",
                "prompt_mode",
                "prompt_file",
                "sticky_ttl_ms",
                "pool",
                "allow_model_region_prefix",
                "max_body_mb",
                "max_context",
                // ★ issue #129：账号故障时自动切换（WB / Trae / Qoder 三份配置同名）。
                "auto_failover",
            ])
        );

        // keys：空列表 + 仅 keys 键。
        let (status, body) = call_api(Method::GET, "/api/gateway/keys", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(exact_keys(&body), set_of(&["keys"]));
        assert_eq!(body["keys"], json!([]));

        // models：CatalogSnapshot 顶层键。
        let (status, body) = call_api(Method::GET, "/api/gateway/models", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            exact_keys(&body),
            set_of(&["region", "source", "fetched_at", "models", "note"])
        );

        // logs
        let (status, body) = call_api(Method::GET, "/api/gateway/logs", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(exact_keys(&body), set_of(&["logs"]));
        // 空日志上的 `== []` 无法证伪「写入 → 读出」这条链路，故此处仅断言形状；
        // 内容由 `gateway_logs_route_returns_recorded_entries` 写入一条后精确断言。
        assert!(
            body["logs"].is_array(),
            "gateway logs must be a JSON array: {body}"
        );

        // strategy
        let (status, body) = call_api(Method::GET, "/api/gateway/strategy", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(exact_keys(&body), set_of(&["cn", "global"]));
    }

    #[tokio::test]
    async fn unknown_paths_fall_back_to_static_index() {
        let _guard = test_guard();
        isolated_home();

        // api_routes() 无 fallback：未知 /api 路径 → 404。
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/definitely-missing")
            .body(Body::empty())
            .unwrap();
        let response = api_routes().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // router() 有静态 fallback：未知静态路径 → SPA 回退 index.html（F1 修复后
        // MIME 由**实际服务的资源**推导，未知路径也回退为 HTML）。
        let (status, content_type, body) = call_full(Method::GET, "/definitely-missing").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "text/html; charset=utf-8");
        assert!(
            body.to_lowercase().contains("<!doctype html"),
            "unknown static path should fall back to index.html"
        );

        // 未知 /api 路径在完整 router() 上同样回退到静态处理器（不是 JSON 404）。
        let (status, content_type, body) = call_full(Method::GET, "/api/definitely-missing").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "text/html; charset=utf-8");
        assert!(
            body.to_lowercase().contains("<!doctype html"),
            "unknown /api path should fall back to the static index, not a JSON 404"
        );

        // 命中 static_handler 的已知资源。
        let (status, content_type, body) = call_full(Method::GET, "/index.html").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            content_type.contains("text/html"),
            "content-type: {content_type}"
        );
        assert!(body.to_lowercase().contains("<!doctype html"));

        let (status, content_type, _) = call_full(Method::GET, "/vite.svg").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            content_type.contains("image/svg+xml"),
            "content-type: {content_type}"
        );
    }

    // -----------------------------------------------------------------------
    // F1 回归：静态资源的 Content-Type 必须由「**实际被服务的资源**」推导
    //
    // 生产 Bug：`static_handler` 曾用**请求路径**推导 MIME。SPA 深链（如 `/accounts`）
    // 在 embed 里落空 → 回退成 index.html 的**内容**，但 MIME 仍按无扩展名的请求路径
    // 计算，得到 `application/octet-stream`，浏览器把 HTML 当二进制**下载**而不是
    // 渲染应用（`App.tsx` 用的是 `BrowserRouter`，深链是常态）。
    // -----------------------------------------------------------------------

    /// F1：SPA 深链必须作为 HTML 返回——状态码 200、MIME 严格为 `text/html; charset=utf-8`、
    /// 且响应体确实是 index.html 的内容。
    #[tokio::test]
    async fn spa_deep_links_are_served_as_html_index() {
        let _guard = test_guard();
        isolated_home();

        // 基准：真实 index.html 的响应体，用来证明深链返回的是**同一份**内容。
        let (index_status, index_type, index_body) = call_full(Method::GET, "/index.html").await;
        assert_eq!(index_status, StatusCode::OK);
        assert_eq!(index_type, "text/html; charset=utf-8");
        assert!(index_body.to_lowercase().contains("<!doctype html"));

        for deep_link in ["/accounts", "/gateway", "/settings/nested/deep"] {
            let (status, content_type, body) = call_full(Method::GET, deep_link).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "SPA deep link {deep_link} must be 200"
            );
            assert_eq!(
                content_type, "text/html; charset=utf-8",
                "SPA deep link {deep_link} must be served as HTML, not an octet-stream download"
            );
            assert_eq!(
                body, index_body,
                "SPA deep link {deep_link} must serve the exact index.html body"
            );
        }

        // 根路径同样回退 index.html，且是 HTML。
        let (status, content_type, body) = call_full(Method::GET, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "text/html; charset=utf-8");
        assert_eq!(body, index_body);
    }

    /// F1：**真实存在的**静态资源 MIME 不受影响——仍由其自身扩展名推导。
    #[tokio::test]
    async fn real_static_asset_mime_is_derived_from_asset_name() {
        let _guard = test_guard();
        isolated_home();

        // 稳定的、非哈希命名资源。
        let (status, content_type, _) = call_full(Method::GET, "/vite.svg").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "image/svg+xml");

        let (status, content_type, _) = call_full(Method::GET, "/icon.png").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "image/png");

        let (status, content_type, _) = call_full(Method::GET, "/index.html").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "text/html; charset=utf-8");

        // 哈希命名的构建产物：从嵌入清单里取**真实**路径，避免硬编码哈希名失稳。
        let css = Assets::iter()
            .find(|path| path.ends_with(".css"))
            .expect("dist must embed a css asset");
        let (status, content_type, _) = call_full(Method::GET, &format!("/{css}")).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "embedded asset {css} must be served"
        );
        assert_eq!(content_type, "text/css", "asset {css} mime");

        let js = Assets::iter()
            .find(|path| path.ends_with(".js"))
            .expect("dist must embed a js asset");
        let (status, content_type, _) = call_full(Method::GET, &format!("/{js}")).await;
        assert_eq!(status, StatusCode::OK, "embedded asset {js} must be served");
        assert_eq!(content_type, "text/javascript", "asset {js} mime");
    }

    /// P0-5 护栏：**哈希产物可长缓存，`index.html` 必须回源校验**。
    ///
    /// 修前这条必红——所有静态响应都没有 `Cache-Control`（实测全仓零匹配）。
    ///
    /// 为什么两边都不能放松：
    /// - 若 `index.html` 也给了 `immutable`，用户会被钉在旧 HTML 上，而它引用的
    ///   旧哈希资源在新构建里已不存在 ⇒ **白屏**（正是 `embedded_index_html_*` 那条护栏的同族事故）。
    /// - 若 `assets/*` 不给长缓存，首屏 1.4 MB 每次刷新都重传（本地/局域网虽快，但纯属浪费）。
    #[tokio::test]
    async fn static_assets_are_immutable_but_index_must_revalidate() {
        let _guard = test_guard();
        isolated_home();

        // index.html：no-cache（且明确**不是** immutable）
        let (status, cache) = call_cache_control("/index.html").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache, "no-cache", "index.html 必须回源校验");

        // SPA 深链回退的也是 index.html ⇒ 同策略
        for deep_link in ["/accounts", "/gateway", "/settings/nested/deep", "/"] {
            let (status, cache) = call_cache_control(deep_link).await;
            assert_eq!(status, StatusCode::OK, "SPA 深链 {deep_link}");
            assert_eq!(
                cache, "no-cache",
                "SPA 深链 {deep_link} 回退 index.html，必须同策略"
            );
        }

        // 哈希产物：immutable 长缓存。从嵌入清单取**真实**路径，避免硬编码哈希名失稳。
        let hashed = Assets::iter()
            .find(|path| path.starts_with("assets/") && path.ends_with(".js"))
            .expect("dist must embed a hashed assets/*.js");
        let (status, cache) = call_cache_control(&format!("/{hashed}")).await;
        assert_eq!(status, StatusCode::OK, "embedded asset {hashed}");
        assert!(
            cache.contains("immutable") && cache.contains("max-age=31536000"),
            "哈希产物 {hashed} 必须长缓存，实际 Cache-Control = `{cache}`"
        );

        // 根目录下的**非**哈希文件（favicon 等）不得被当成不可变资源
        let (status, cache) = call_cache_control("/icon.png").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache, "no-cache", "非哈希根文件必须回源校验");
    }

    /// `cache_control_for` 的纯函数边界：只看前缀，`assets/` 之外的任何路径都必须 `no-cache`。
    #[test]
    fn cache_control_only_relaxes_inside_the_hashed_assets_dir() {
        assert!(cache_control_for("assets/index-abc123.js").contains("immutable"));
        assert!(cache_control_for("assets/x.woff2").contains("immutable"));
        assert_eq!(cache_control_for("index.html"), "no-cache");
        assert_eq!(cache_control_for("icon.png"), "no-cache");
        assert_eq!(cache_control_for("vite.svg"), "no-cache");
        // 形似但不在目录内的路径不得被误判（前缀必须带斜杠）
        assert_eq!(cache_control_for("assetsfoo/x.js"), "no-cache");
        assert_eq!(cache_control_for("assets"), "no-cache");
    }

    /// Q2 边界：SPA 回退 MIME 与请求**大小写 / query** 无关——`uri.path()` 已剥离 query，
    /// 大小写不同的深链同样「embed 落空 → index.html」回退，仍必须是 HTML。
    #[tokio::test]
    async fn spa_fallback_mime_is_case_and_query_insensitive() {
        let _guard = test_guard();
        isolated_home();

        let (_, _, index_body) = call_full(Method::GET, "/index.html").await;
        for uri in ["/Accounts", "/GATEWAY", "/accounts?tab=keys", "/settings/"] {
            let (status, content_type, body) = call_full(Method::GET, uri).await;
            assert_eq!(status, StatusCode::OK, "{uri} must be 200");
            assert_eq!(
                content_type, "text/html; charset=utf-8",
                "{uri} must be served as HTML regardless of case/query"
            );
            assert_eq!(
                body, index_body,
                "{uri} must serve the exact index.html body"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 构建产物一致性护栏：`dist/` 只能装 WebUI 构建
    //
    // 背景（真实事故）：`npm run build`（Vite base `/`，供 rust-embed / Tauri
    // `frontendDist` / `scripts/fix-app.sh` 使用）与 `npm run build:demo`
    // （base `/trae-workbuddy-switch/`，仅供 GitHub Pages）**曾共同输出到 `dist/`**。
    // 若编译期 `dist/` 恰好是演示构建，`index.html` 会请求
    // `/trae-workbuddy-switch/assets/index-*.js`——该前缀在 embed 里不存在 →
    // `static_handler` 回退成 HTML → 浏览器模块脚本 MIME 校验失败：
    //   Failed to load module script: Expected a JavaScript-or-Wasm module script
    //   but the server responded with a MIME type of "text/html".
    // → **webui 与桌面端双双空白页**（实测 `#root` 子节点数为 0）。
    //
    // 该约定已由 `build:demo --outDir dist-demo` 保证，下面的测试是它的护栏：
    // 一旦有人把演示构建塞回 `dist/`，本测试立刻变红，而不是等到用户看到白屏。
    // -----------------------------------------------------------------------

    /// 取出 HTML 中所有**根绝对**资源引用（`src="/…"` / `href="/…"`）。
    ///
    /// 只收根绝对路径：跳过 `//cdn…`（协议相对）、`https://…`、`data:`、`#hash`。
    fn asset_refs_in(html: &str) -> Vec<String> {
        let mut refs: Vec<String> = Vec::new();
        for attr in ["src=\"", "href=\""] {
            let mut rest = html;
            while let Some(found) = rest.find(attr) {
                let tail = &rest[found + attr.len()..];
                let Some(end) = tail.find('"') else { break };
                let value = &tail[..end];
                if value.starts_with('/') && !value.starts_with("//") {
                    refs.push(value.to_string());
                }
                rest = &tail[end..];
            }
        }
        refs.sort();
        refs.dedup();
        refs
    }

    /// 取出 JS 产物里所有**引号包裹的** `assets/<name>.js|.css` 资源名。
    ///
    /// Vite 把「动态 import 的文件清单」放在入口 chunk 的 `__vite__mapDeps` 数组里，
    /// 形如 `["assets/AccountsPage-C5GdpMAe.js","assets/chart-DDcOHMGW.js",…]`。
    /// 这里只认「引号包起来、以 `assets/` 开头、以 `.js`/`.css` 结尾」的整串，
    /// 因此不会把恰好出现在字符串里的普通文本误判成资源名。
    fn quoted_asset_names_in(js: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for quote in ['"', '\''] {
            let mut rest = js;
            while let Some(found) = rest.find(quote) {
                let tail = &rest[found + 1..];
                let Some(end) = tail.find(quote) else { break };
                let value = &tail[..end];
                if value.starts_with("assets/")
                    && (value.ends_with(".js") || value.ends_with(".css"))
                {
                    names.push(value.to_string());
                }
                rest = &tail[end..];
            }
        }
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn asset_refs_in_extracts_only_root_absolute_paths() {
        let html = concat!(
            r#"<link rel="icon" href="/icon.png">"#,
            r#"<script src="/assets/a.js"></script>"#,
            r#"<link href="//cdn.example.com/x.css">"#,
            r#"<img src="https://example.com/y.png">"#,
            "<a href=\"#top\">top</a>",
            r#"<a href="/relative-ok.css">css</a>"#,
        );
        assert_eq!(
            asset_refs_in(html),
            vec!["/assets/a.js", "/icon.png", "/relative-ok.css"]
        );
        assert!(asset_refs_in("<p>no refs</p>").is_empty());
    }

    /// 护栏：内嵌 `index.html` 引用的每个根绝对资源都必须能在 embed 中命中。
    ///
    /// 这条断言同时覆盖三类真实故障：① `dist/` 被演示构建覆盖（base 前缀不匹配）；
    /// ② 前端删了资源但 embed 未重编；③ `dist/` 与 embed 不同步。
    #[test]
    fn embedded_index_html_references_only_embedded_assets() {
        let index = Assets::get("index.html").expect("dist/index.html must be embedded");
        let html = String::from_utf8_lossy(&index.data).to_string();
        let refs = asset_refs_in(&html);
        assert!(
            !refs.is_empty(),
            "dist/index.html must reference at least one root-absolute asset: {html}"
        );
        for reference in &refs {
            let key = reference.trim_start_matches('/');
            assert!(
                Assets::get(key).is_some(),
                "dist/index.html references `{reference}` which is NOT embedded. \
                 `dist/` most likely holds a **demo** build (Vite base `/qoder-trae-workbuddy-switch/`). \
                 Run `npm run build` (NOT `npm run build:demo`) before compiling the server / Tauri app. \
                 Embedded paths: {:?}",
                Assets::iter().collect::<Vec<_>>()
            );
        }
    }

    /// 护栏的对照面：**正确**构建下，`index.html` 的脚本预加载同源路径也必须命中。
    /// （`/` 与 `/index.html` 两个入口都要能命中，避免只修一个出口。）
    #[test]
    fn embedded_index_html_is_servable_at_root_and_index() {
        assert!(
            Assets::get("index.html").is_some(),
            "dist/index.html must be embedded"
        );
        let files: Vec<String> = Assets::iter().map(|path| path.to_string()).collect();
        assert!(
            files.iter().any(|f| f.starts_with("assets/")),
            "dist must embed at least one assets/* file, got {files:?}"
        );
    }

    /// ★ 护栏（B2 路由懒加载配套）：入口 chunk 里列出的**每个动态 chunk** 都必须也在 embed 中。
    ///
    /// ## 为什么必须补这一条
    ///
    /// `embedded_index_html_references_only_embedded_assets` 只校验 `index.html` 里的
    /// **根绝对引用**。启用路由懒加载后，绝大多数资源改由**入口 JS** 动态 import
    /// （Vite 的 `__vite__mapDeps` 清单），`index.html` 里**根本看不见它们**
    /// ⇒ 那条护栏对懒加载 chunk **零覆盖**。
    ///
    /// ## 它防的是什么事故
    ///
    /// 与它守护的同一类：`dist/` 与 embed 不同步（改前端后未重编宿主），
    /// 或 `dist/` 被演示构建覆盖。症状比原来**更隐蔽** —— 首屏照常打开，
    /// 一点侧栏那个 chunk 就 404 ⇒ 页面**永久停在 `Suspense` 的 fallback 骨架**上，
    /// 用户看到的是「点了没反应」，而不是白屏。
    ///
    /// ## 不许空转
    ///
    /// 若提取器失效（比如 Vite 换了输出格式），`referenced` 会变成空集、
    /// 本测试就会恒真。故先断言它非空。
    #[test]
    fn embedded_entry_chunk_references_only_embedded_lazy_chunks() {
        let index = Assets::get("index.html").expect("dist/index.html must be embedded");
        let html = String::from_utf8_lossy(&index.data).to_string();
        let entry = asset_refs_in(&html)
            .into_iter()
            .find(|reference| reference.starts_with("/assets/") && reference.ends_with(".js"))
            .expect("index.html 必须引用一个入口脚本");
        let key = entry.trim_start_matches('/');
        let chunk = Assets::get(key).expect("入口脚本必须在 embed 中");
        let source = String::from_utf8_lossy(&chunk.data).to_string();

        let referenced = quoted_asset_names_in(&source);
        assert!(
            !referenced.is_empty(),
            "入口脚本 {key} 里没找到任何 assets/* 引用 —— 要么懒加载被移除、\
             要么提取器失效。本护栏**不得空转**：请先修提取器再放行。"
        );

        let embedded: Vec<String> = Assets::iter().map(|path| path.to_string()).collect();
        for name in &referenced {
            assert!(
                embedded.iter().any(|entry| entry == name),
                "入口脚本引用了 `{name}`，但它不在 embed 中。\
                 `dist/` 与 embed 不同步（改前端后未重编宿主），或 `dist/` 被演示构建覆盖。\n\
                 已嵌入的 assets/*：{:?}",
                embedded
                    .iter()
                    .filter(|entry| entry.starts_with("assets/"))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn quoted_asset_names_in_extracts_only_real_bundle_names() {
        let js = concat!(
            r#"const d=["assets/AccountsPage-C5GdpMAe.js","assets/chart-DDcOHMGW.js"];"#,
            r#"import("./index-BOrx8Cn2.js");"#,
            r#"const note="assets/not-a-file.txt";"#,
            r#"const other="src/components/foo.js";"#,
            r#"const cdn="https://cdn.example.com/assets/x.js";"#,
        );
        assert_eq!(
            quoted_asset_names_in(js),
            vec![
                "assets/AccountsPage-C5GdpMAe.js",
                "assets/chart-DDcOHMGW.js"
            ],
            "只收「引号包裹 + assets/ 前缀 + .js/.css 结尾」的整串"
        );
        assert!(quoted_asset_names_in("no assets here").is_empty());
    }

    /// Q2 独立证明：F2 护栏要求覆盖目录**已存在**，否则 `set_var` 会被静默忽略
    /// 并回落真实 home。这里直接断言生产 `home_dir()` 解析到隔离临时目录——
    /// 若隔离失效则 `home_dir()` 会返回真实 home（不在 temp 下），本断言即变红。
    #[test]
    fn isolated_home_override_is_actually_effective() {
        let _guard = test_guard();
        let home = isolated_home();
        let resolved = buddy_switch_core::modules::config::home_dir();
        assert!(
            resolved.starts_with(std::env::temp_dir()),
            "home_dir() must resolve inside the temp dir, got {resolved:?}"
        );
        assert_eq!(
            resolved, home,
            "BUDDY_SWITCH_HOME override must resolve home_dir() to the isolated temp dir"
        );
    }

    // -----------------------------------------------------------------------
    // 写操作路由（隔离 home 下串行）
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn gateway_key_lifecycle_never_leaks_secrets_over_http() {
        let _guard = test_guard();
        isolated_home();

        // create：一次性明文必须返回（且只在 create 响应里）。
        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/keys",
            Some(json!({"name": "e2-lifecycle", "region": "cn"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], json!(true));
        let plaintext = body["key"]
            .as_str()
            .expect("create must return once-only plaintext")
            .to_string();
        assert!(plaintext.starts_with("sk-wb-"), "plaintext: {plaintext}");
        assert_eq!(plaintext.len(), "sk-wb-".len() + 32);
        let record_id = body["record"]["id"]
            .as_str()
            .expect("create record id")
            .to_string();

        // list：脱敏红线——仅 prefix，绝无 hash / 明文 / 64 位摘要。
        let (status, body) = call_api(Method::GET, "/api/gateway/keys", None).await;
        assert_eq!(status, StatusCode::OK);
        let keys = body["keys"].as_array().expect("keys array");
        assert_eq!(keys.len(), 1);
        assert_eq!(
            exact_keys(&keys[0]),
            set_of(&[
                "id",
                "name",
                "region",
                "prefix",
                // ⚠️ `suffix`（明文末 4 位）是 `apikey.rs` 的**显式白名单**成员
                // （E3 新增，用户靠它确认是哪一把）。本用例原先漏了这一格，
                // 于是 `suffix` 一加进来就变红 —— 属**用例未同步**，不是泄露。
                // 真正的红线在下面三条断言（无 hash / 无明文 / 无 sha256 摘要）。
                "suffix",
                "createdAt",
                "revokedAt",
                "revoked",
                "lastUsedAt",
                // ★ issue #76：策略七件套（`apikey.rs` 同款白名单已同步）。
                //   与 `suffix` 同一性质：**显式批准**才进掩敏视图；
                //   真正的红线是下面三条（无 hash / 无明文 / 无摘要）。
                "quotaMode",
                "quotaLimit",
                "quotaUnit",
                "quotaPeriod",
                "scopeMode",
                "scopeAccounts",
                "scopeGroups",
            ]),
            "masked record must be an explicit non-secret whitelist"
        );
        assert!(!keys[0].as_object().unwrap().contains_key("hash"));

        let serialized = serde_json::to_string(&body).unwrap();
        assert!(
            !serialized.contains("hash"),
            "list body must not contain a `hash` field"
        );
        assert!(
            !serialized.contains(&plaintext),
            "list body must never echo the plaintext key"
        );
        assert!(
            !serialized.contains(&buddy_switch_gateway::apikey::sha256_hex(&plaintext)),
            "list body must not contain the sha256 digest"
        );
        assert!(
            !contains_hex_run(&serialized, 64),
            "list body must not contain any 64-char hex digest: {serialized}"
        );

        // 未吊销直接删除 → 400 + 可读错误。
        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/keys/delete",
            Some(json!({"id": record_id})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], json!(false));
        assert!(
            body["error"].as_str().unwrap_or("").contains("吊销"),
            "error should instruct to revoke first: {body}"
        );

        // 未吊销直接恢复 → 400 + 可读错误（恢复只对**已吊销**的 Key 有意义）。
        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/keys/restore",
            Some(json!({"id": record_id})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], json!(false));
        assert!(
            body["error"].as_str().unwrap_or("").contains("未吊销"),
            "error should say the key is not revoked: {body}"
        );

        // 吊销 → 恢复 → 列表回到「启用」（两条出口都可用，故用户在吊销后**两个选择都在**）。
        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/keys/revoke",
            Some(json!({"id": record_id})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], json!(true));

        let (_, body) = call_api(Method::GET, "/api/gateway/keys", None).await;
        assert_eq!(body["keys"][0]["revoked"], json!(true));
        assert!(
            body["keys"][0]["revokedAt"].as_i64().is_some(),
            "revokedAt 应落时间戳"
        );

        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/keys/restore",
            Some(json!({"id": record_id})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], json!(true));

        let (_, body) = call_api(Method::GET, "/api/gateway/keys", None).await;
        assert_eq!(body["keys"][0]["revoked"], json!(false), "恢复后应回到启用");
        assert!(
            body["keys"][0]["revokedAt"].is_null(),
            "恢复必须清空 revokedAt"
        );

        // 再吊销 → 删除成功 → 列表清空（吊销 → 恢复 → 再吊销 → 删除 全链路可反复）。
        let (status, _) = call_api(
            Method::POST,
            "/api/gateway/keys/revoke",
            Some(json!({"id": record_id})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/keys/delete",
            Some(json!({"id": record_id})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], json!(true));

        let (_, body) = call_api(Method::GET, "/api/gateway/keys", None).await;
        assert_eq!(body["keys"], json!([]));
    }

    #[tokio::test]
    async fn gateway_save_config_rejects_invalid_and_persists_disabled() {
        let _guard = test_guard();
        isolated_home();

        // 端口类型错误 → 400（而非 500）。
        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/config",
            Some(json!({"port": "not-a-number"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], json!(false));
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .contains("配置格式错误"),
            "error: {body}"
        );

        // 合法但禁用监听：不得真的绑定端口。
        let (status, body) = call_api(
            Method::POST,
            "/api/gateway/config",
            Some(json!({
                "enabled": false,
                "bind_addr": "127.0.0.1",
                "port": 57891,
                "allow_non_loopback": false,
                "log_keep": 50,
                "log_bodies": false
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["running"], json!(false));
        assert_eq!(body["addr"], Value::Null);

        // 配置必须落盘到隔离 home（证明读写未触碰真实用户目录）。
        let config_file = isolated_home()
            .join(".buddy-switch")
            .join("gateway_config.json");
        assert!(
            config_file.is_file(),
            "config must persist to the isolated home"
        );
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&config_file).unwrap()).unwrap();
        assert_eq!(saved["enabled"], json!(false));
        assert_eq!(saved["log_keep"], json!(50));
    }

    #[test]
    fn web_checkin_status_keeps_account_identity() {
        let item = checkin_status_item(
            &json!({"id": "account-1", "email": "user@example.com"}),
            json!({"ok": true, "todayCheckedIn": true}),
        );

        assert_eq!(item["accountId"], "account-1");
        assert_eq!(item["email"], "user@example.com");
        assert_eq!(item["todayCheckedIn"], true);
    }

    #[test]
    fn web_checkin_status_preserves_failure_state() {
        let item = checkin_status_item(
            &json!({"id": "account-2"}),
            json!({"ok": false, "todayCheckedIn": false, "error": "status failed"}),
        );

        assert_eq!(item["accountId"], "account-2");
        assert_eq!(item["ok"], false);
        assert_eq!(item["error"], "status failed");
    }

    /// 排程配置路由：GET 输出新键 `*_times`；POST 校验非法时点、旧键仍被接受，
    /// 且错误指向 `*_enabled`。
    ///
    /// issue #27 起时点粒度到分钟（`"HH:MM"`）。路由层是**唯一**把配置直接交给客户端的
    /// 出口，因此这里要钉住三件事：① 响应只出新键（旧键留着会让两套字段长期并存）；
    /// ② 分钟值原样往返（被截成整点就等于功能没生效）；③ 老客户端提交的 `*_hours`
    /// 不会 400、也不会静默落默认。
    #[tokio::test]
    async fn schedule_config_route_validates_times() {
        let _guard = test_guard();
        isolated_home();

        let (status, body) = call_api(Method::GET, "/api/schedule/config", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["checkin_times"], json!(["09:00", "21:00"]));
        assert_eq!(body["activity_times"], json!(["10:00"]));
        assert_eq!(body["activity_report_count"], json!(5));
        assert_eq!(body["checkin_enabled"], json!(true));
        assert!(
            // 用 `contains_key` 而不是 `body.get("…")`：`check:api` 的 [naming] 规则要求
            // 服务端任何 `.get("snake_key")` 都必须带 camelCase 双读（前端发 camelCase），
            // 而这里要断言的恰恰是「旧键**不存在**」，双读语义完全不适用。
            !body
                .as_object()
                .is_some_and(|map| map.contains_key("checkin_hours")),
            "响应不得再输出旧键（新旧两套字段并存会让「哪份是真的」无从判断）：{body}"
        );

        // 分钟粒度必须原样往返。
        let (status, body) = call_api(
            Method::POST,
            "/api/schedule/config",
            Some(json!({"checkin_times": ["09:37"]})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["checkin_times"], json!(["09:37"]));

        // 旧键（整点整数）仍可写：等价于 `hour:00`。
        let (status, body) = call_api(
            Method::POST,
            "/api/schedule/config",
            Some(json!({"travel_hours": [7, 19]})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["travel_times"], json!(["07:00", "19:00"]));

        // 抖动幅度：默认 0（关闭）→ 可写 → 非法值 400。
        let (status, body) = call_api(Method::GET, "/api/schedule/config", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["checkin_jitter_minutes"], json!(0), "抖动默认必须关闭");

        let (status, body) = call_api(
            Method::POST,
            "/api/schedule/config",
            Some(json!({"checkin_jitter_minutes": 15})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["checkin_jitter_minutes"], json!(15));

        let (status, body) = call_api(
            Method::POST,
            "/api/schedule/config",
            Some(json!({"checkin_jitter_minutes": 61})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .contains("checkin_enabled"),
            "抖动越界的文案必须指向 *_enabled：{body}"
        );

        // 非法时点（分钟越界）→ 400，且文案指向开关。
        let (status, body) = call_api(
            Method::POST,
            "/api/schedule/config",
            Some(json!({"checkin_times": ["24:00"]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], json!(false));
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .contains("checkin_enabled"),
            "错误文案必须指向 *_enabled：{body}"
        );
    }

    // -----------------------------------------------------------------------
    // region 隔离（E1，可证伪断言）
    //
    // 目的：证明宿主**真的按请求里的 region 分派**到 core 的 `*_for(region, …)`
    // 变体，而不是忽略 region 恒按 CN。以下用例都是「改坏即变红」的形态：
    //   · 若把 `region` 从 args 里丢掉 / 恒按 CN 处理 → 断言立刻失败。
    // -----------------------------------------------------------------------

    /// 播种某 region 的账号库（`~/.buddy-switch/accounts.json` / `accounts.global.json`）。
    fn seed_accounts(region: Region, accounts: Value) {
        let file = buddy_switch_core::modules::region::accounts_file_for(region);
        std::fs::create_dir_all(file.parent().unwrap()).expect("create store dir");
        std::fs::write(&file, serde_json::to_string(&accounts).unwrap()).expect("seed accounts");
    }

    /// 清理某 region 的账号库，避免种子泄漏到其它用例。
    fn clear_accounts(region: Region) {
        let file = buddy_switch_core::modules::region::accounts_file_for(region);
        let _ = std::fs::remove_file(&file);
    }

    fn account_ids(body: &Value) -> Vec<String> {
        body["accounts"]
            .as_array()
            .expect("accounts array")
            .iter()
            .filter_map(|a| a["id"].as_str().map(String::from))
            .collect()
    }

    /// 两套独立账号库各自只对本 region 可见：`?region=cn` 只看得到 CN，
    /// `?region=global` 只看得到 Global；缺省 region 仍等价于 cn。
    #[tokio::test]
    async fn accounts_route_is_region_isolated() {
        let _guard = test_guard();
        isolated_home();

        seed_accounts(
            Region::Cn,
            json!([{"id": "cn-only", "uid": "uid-cn", "email": "cn@example.com"}]),
        );
        seed_accounts(
            Region::Global,
            json!([{"id": "global-only", "uid": "uid-global", "email": "global@example.com"}]),
        );

        let (status_cn, body_cn) = call_api(Method::GET, "/api/accounts?region=cn", None).await;
        let (status_global, body_global) =
            call_api(Method::GET, "/api/accounts?region=global", None).await;
        let (status_default, body_default) = call_api(Method::GET, "/api/accounts", None).await;

        // 先清理，再断言：即便断言失败也不会把种子留给后续用例。
        clear_accounts(Region::Cn);
        clear_accounts(Region::Global);

        assert_eq!(status_cn, StatusCode::OK);
        assert_eq!(status_global, StatusCode::OK);
        assert_eq!(status_default, StatusCode::OK);

        assert_eq!(body_cn["region"], json!("cn"));
        assert_eq!(
            account_ids(&body_cn),
            vec!["cn-only"],
            "region=cn 只能看到 CN 账号库：{body_cn}"
        );

        assert_eq!(body_global["region"], json!("global"));
        assert_eq!(
            account_ids(&body_global),
            vec!["global-only"],
            "region=global 只能看到 Global 账号库：{body_global}"
        );

        // 旧行为不破：缺省 region 等价于 cn。
        assert_eq!(body_default["region"], json!("cn"));
        assert_eq!(
            account_ids(&body_default),
            vec!["cn-only"],
            "缺省 region 必须等价于 cn：{body_default}"
        );
    }

    /// 对一个**只存在于 Global 库**的账号执行删除：`{region:"cn"}` 必须失败，
    /// `{region:"global"}` 必须成功。
    ///
    /// 这是「宿主真的按 region 分派」的可证伪断言——若宿主忽略 region、恒按 CN 删除，
    /// 则成功分支会因 CN 库里没有该 id 而 400，本用例必然变红。
    #[tokio::test]
    async fn delete_route_dispatches_by_region() {
        let _guard = test_guard();
        isolated_home();

        seed_accounts(Region::Cn, json!([]));
        seed_accounts(
            Region::Global,
            json!([{"id": "global-victim", "uid": "uid-g", "email": "g@example.com"}]),
        );

        // region=cn：该账号在 CN 库不存在 → 400，且不得影响 Global 库。
        let (status, body) = call_api(
            Method::POST,
            "/api/delete",
            Some(json!({"accountId": "global-victim", "region": "cn"})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "CN 库不得删除 Global 账号：{body}"
        );
        assert_eq!(body["ok"], json!(false));

        let (_, still_there) = call_api(Method::GET, "/api/accounts?region=global", None).await;
        assert_eq!(
            account_ids(&still_there),
            vec!["global-victim"],
            "失败的删除不得改动 Global 库：{still_there}"
        );

        // region=global：命中 → 200 + ok。
        let (status, body) = call_api(
            Method::POST,
            "/api/delete",
            Some(json!({"accountId": "global-victim", "region": "global"})),
        )
        .await;
        let (_, emptied) = call_api(Method::GET, "/api/accounts?region=global", None).await;

        clear_accounts(Region::Cn);
        clear_accounts(Region::Global);

        assert_eq!(
            status,
            StatusCode::OK,
            "Global 必须能删除 Global 账号：{body}"
        );
        assert_eq!(body["ok"], json!(true));
        assert!(
            account_ids(&emptied).is_empty(),
            "删除后 Global 库应为空：{emptied}"
        );
    }

    /// P5 T03：参数化命令（导出 / 导入 / 积分）的 `scope` 必须 fail-closed（未知 / 空 / 缺 ⇒ 400）。
    ///
    /// 与 `src-tauri/src/commands.rs` 的 `parameterized_scope_tests` **互为镜像**
    /// （同款实现散落两 crate 是刻意不去重，见共享知识）。
    ///
    /// 可证伪：把某条 handler 的 `resolve_scope_param(...)` 换成 `unwrap_or(Scope::WbCn)`
    /// 式回落，第一条断言立即变红 —— 那正是「拼错 `trae-glboal` 静默读写 `wb-cn` 的库」的缺陷形态。
    #[tokio::test]
    async fn parameterized_account_commands_reject_unknown_scope() {
        let _guard = test_guard();
        isolated_home();

        for uri in [
            "/api/accounts/export",
            "/api/accounts/export-to-path",
            "/api/accounts/import",
            "/api/accounts/import-local",
            "/api/data-dir/open",
            "/api/credits/query",
            "/api/checkin/status/query",
        ] {
            // 未知 / 拼错的作用域 ⇒ 400（`resolve_scope_param` 在落盘之前失败）。
            let (status, body) = call_api(
                Method::POST,
                uri,
                Some(json!({ "scope": "trae-glboal", "accountIds": [] })),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
            assert_eq!(body["ok"], json!(false), "{uri}: {body}");
            assert!(
                body["error"].as_str().unwrap_or("").contains("wb-cn"),
                "{uri} 的错误文案必须列出合法作用域: {body}"
            );

            // 缺 scope 同样 400（scope 是必填维度，缺它无从判断该读哪个库）。
            let (status, body) = call_api(Method::POST, uri, Some(json!({}))).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{uri} 缺 scope 必须 400: {body}"
            );
        }
    }
}

/// `parse_trae_variant` 的回归护栏。
///
/// 覆盖：合法值、大小写与空白、**区域标识落到本区域合并版**、**未知值回落默认**。
/// 与 `src-tauri/src/commands.rs` 的同名单测**互为镜像**，
/// 两处实现必须保持一致（规则本身在 `trae::handlers::parse_variant_id`）。
#[cfg(test)]
mod parse_trae_variant_tests {
    use super::parse_trae_variant;
    use buddy_switch_core::modules::trae::variant::TraeVariant;

    #[test]
    fn parses_known_canonical_values() {
        assert_eq!(parse_trae_variant(Some("trae_work")), TraeVariant::TraeWork);
        assert_eq!(parse_trae_variant(Some("trae_cn")), TraeVariant::Trae);
    }

    /// ★ 前端只传区域（`cn` / `global`）：必须落到**本区域的合并版**。
    ///
    /// 反例（2026-10-20 用户报障）：`global` → 国际版 TraeWork ⇒ 授权页
    /// `auth_from=solo` ⇒ 「TraeWork is currently not available in your region」。
    #[test]
    fn region_ids_resolve_to_the_regions_merged_client() {
        assert_eq!(parse_trae_variant(Some("cn")), TraeVariant::TraeMerged);
        assert_eq!(
            parse_trae_variant(Some("global")),
            TraeVariant::GlobalTraeMerged
        );
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
}

// ---------------------------------------------------------------------------
// 统一网关（第三个产品线 · 管理面）
//
// ★ 它**跨平台**（把两个产品线的模型合成一个入口）⇒ 能力门控取 WorkBuddy 的 `gateway`
//   能力（桌面端恒具备；「网关类能力可用与否」对两个产品线是一致的）。
// ---------------------------------------------------------------------------

/// GET /api/unified-gateway/config —— 配置 + 运行状态 + 命名空间（已启用 / 全部在册）。
async fn api_unified_gateway_config() -> Response {
    let state = crate::unified_gateway_host::shared_state();
    // ★ 共同字段一律走**共享构造**（与桌面端同源；只有 `running`/`addr` 由本宿主补）。
    let mut payload = buddy_switch_gateway::unified::admin::config_payload(&state).await;
    let (running, addr) = crate::unified_gateway_host::status().await;
    payload["running"] = json!(running);
    payload["addr"] = json!(addr);
    json_ok(payload)
}

/// POST /api/unified-gateway/config —— 保存并应用（启动 / 重启独立监听）。
async fn api_save_unified_gateway_config(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Workbuddy, capability::Capability::Gateway) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let submitted = body.get("config").cloned().unwrap_or(body);
    let config: UnifiedGatewayConfig = match serde_json::from_value(submitted) {
        Ok(config) => config,
        Err(error) => return json_err(format!("配置格式错误: {error}"), StatusCode::BAD_REQUEST),
    };
    // ★ 不在册的命名空间 ⇒ 明确报错（不静默丢弃用户填的值）
    if let Err(error) = config.validate() {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    if let Err(error) = config.save() {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    let state = crate::unified_gateway_host::shared_state();
    state.set_config(config.clone()).await;
    match crate::unified_gateway_host::apply().await {
        Ok(addr) => json_ok(json!({
            "ok": true,
            "config": config,
            "running": addr.is_some(),
            "addr": addr,
        })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/unified-gateway/models —— 已启用命名空间的清单预览（管理面专用，不过滤 Key 授权）。
///
/// `?refresh=true` ⇒ 绕过各产品线的识别缓存重取（同桌面端
/// `get_unified_gateway_models(refresh)`；见 `unified::routes::models_preview` 的文档）。
/// ⚠️ 与 `src-tauri/src/commands.rs` 的同名命令**刻意不去重**（两宿主各一份）。
async fn api_unified_gateway_models(RawQuery(query): RawQuery) -> Response {
    // 查询串只做「是否为真」判定：`true` / `1` 都算（手工调 API 的人两种都会写）。
    let refresh = matches!(
        query_value(query.as_deref(), "refresh").as_deref(),
        Some("true") | Some("1")
    );
    json_ok(buddy_switch_gateway::unified::routes::models_preview(refresh).await)
}

/// GET /api/unified-gateway/pool-overview —— 已启用命名空间指向的**账号池总览**（只读投影）。
///
/// 统一网关没有自己的池：它按命名空间把请求分派到三条产品线各自的池。本 handler 把它们
/// **已有的**池视图投影成一份跨平台汇总（**不新增任何选号逻辑**）。
///
/// ★ 形状 / 分组的**唯一构造点**在 gateway crate 的
///   `unified::admin::pool_overview_payload`（两处宿主不各自拼 JSON，见那里的模块文档）。
/// ⚠️ 与桌面端 `unified_gateway_pool_overview` **刻意不去重**。
async fn api_unified_gateway_pool_overview() -> Response {
    json_ok(buddy_switch_gateway::unified::admin::pool_overview_payload().await)
}

/// GET /api/unified-gateway/keys —— 统一密钥列表（**只出脱敏**）。
async fn api_unified_gateway_list_keys() -> Response {
    json_ok(buddy_switch_gateway::unified::admin::keys_list_payload(
        &crate::unified_gateway_host::shared_state().keys,
    ))
}

/// POST /api/unified-gateway/keys —— 创建（**明文只返回这一次**）。
async fn api_unified_gateway_create_key(Json(body): Json<Value>) -> Response {
    let namespaces: Vec<String> = body
        .get("namespaces")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    // ⚠️ 前端发 **camelCase**（门禁按 `commands.rs` 的形参名比对），手工调 API 的人可能发
    //   snake_case ⇒ **两种都收**（服务端是手工读 body，不像 Tauri 那样自动映射）。
    let biz_namespace = body
        .get("bizNamespace")
        .or_else(|| body.get("biz_namespace"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("未命名")
        .to_string();
    match crate::unified_gateway_host::shared_state()
        .keys
        .create(name, namespaces, biz_namespace)
    {
        Ok((record, plaintext)) => {
            json_ok(json!({ "ok": true, "key": plaintext, "record": record.masked() }))
        }
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/unified-gateway/keys/revoke —— 吊销（置标记，**可逆**）。
async fn api_unified_gateway_revoke_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match crate::unified_gateway_host::shared_state().keys.revoke(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/unified-gateway/keys/policy —— 更新某把统一 Key 的策略（issue #76）。
///
/// ★ **不动命名空间授权**（`namespaces` / `biz_namespace`）—— 那是「能访问哪些平台」，
/// 与配额是两个维度，混进同一个入口会让「只改配额」意外清掉授权。
async fn api_unified_gateway_set_key_policy(Json(body): Json<Value>) -> Response {
    use buddy_switch_gateway::apikey::KeyPolicy;
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    let policy: KeyPolicy =
        match serde_json::from_value(body.get("policy").cloned().unwrap_or(Value::Null)) {
            Ok(policy) => policy,
            Err(error) => {
                return json_err(format!("策略字段不合法: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    match crate::unified_gateway_host::shared_state()
        .keys
        .update_policy(id, policy)
    {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/unified-gateway/keys/usage-all —— **批量**用量（密钥表每行一列）。
///
/// 为什么不复用单 Key 命令循环调用：密钥表一屏十几行，N 次往返会让打开分区明显变慢；
/// 账本本来就是一次 `load()` 全量读，批量反而更省。
async fn api_unified_gateway_key_usage_all() -> Response {
    let state = crate::unified_gateway_host::shared_state();
    let now = buddy_switch_core::modules::config::now_ms();
    let mut rows = serde_json::Map::new();
    for record in state.keys.list() {
        let policy = record.policy();
        let used =
            state
                .key_usage
                .used_for(&record.id, policy.quota_unit, policy.quota_period, now);
        let exceeded = matches!(
            buddy_switch_gateway::apikey::quota_verdict_for_policy(&policy, used),
            buddy_switch_gateway::apikey::QuotaVerdict::Exceeded { .. }
        );
        rows.insert(
            record.id.clone(),
            json!({
                "ok": true,
                "keyId": record.id,
                "unit": policy.quota_unit,
                "period": policy.quota_period,
                "periodKey": policy.quota_period.period_key(now),
                "used": used,
                "total": state
                    .key_usage
                    .snapshot(&record.id)
                    .map(|entry| entry.counters(policy.quota_unit).total)
                    .unwrap_or(0),
                "exceeded": exceeded,
            }),
        );
    }
    json_ok(Value::Object(rows))
}

/// GET /api/unified-gateway/keys/usage?id=… —— 读某把统一 Key 的用量快照。
async fn api_unified_gateway_key_usage(RawQuery(query): RawQuery) -> Response {
    let id = query_value(query.as_deref(), "id").unwrap_or_default();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::unified_gateway_host::shared_state();
    let now = buddy_switch_core::modules::config::now_ms();
    let Some(record) = state.keys.list().into_iter().find(|record| record.id == id) else {
        return json_err("API Key 不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let policy = record.policy();
    let usage = state.key_usage.snapshot(&id);
    let used = state
        .key_usage
        .used_for(&id, policy.quota_unit, policy.quota_period, now);
    let exceeded = matches!(
        buddy_switch_gateway::apikey::quota_verdict_for_policy(&policy, used),
        buddy_switch_gateway::apikey::QuotaVerdict::Exceeded { .. }
    );
    json_ok(json!({
        "ok": true,
        "keyId": id,
        "unit": policy.quota_unit,
        "period": policy.quota_period,
        "periodKey": policy.quota_period.period_key(now),
        "used": used,
        "total": usage
            .as_ref()
            .map(|entry| entry.counters(policy.quota_unit).total)
            .unwrap_or(0),
        "exceeded": exceeded,
    }))
}

/// POST /api/unified-gateway/keys/restore —— 撤销吊销（同一把 Key 重新可用）。
async fn api_unified_gateway_restore_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match crate::unified_gateway_host::shared_state().keys.restore(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/unified-gateway/keys/delete —— 物理删除（**仅限已吊销**）。
async fn api_unified_gateway_delete_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match crate::unified_gateway_host::shared_state().keys.delete(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// Qoder（第三个平台）
// ---------------------------------------------------------------------------
//
// ★ 全部 handler **参数化 `scope`**，一律经 `resolve_scope_param` **fail-closed** 解析。
//   Qoder 的双区域是**两本互不相通的账号库**，缺 scope 就无从判断该读哪本。
//
// ⚠️ 本文件在 `src-tauri/src/commands.rs` 有**同款**接线（刻意不去重，见文件头说明）。
//   改一处必须 grep 另一处。

/// 该端点**只服务 WorkBuddy / Trae** —— Qoder 作用域一律给可诊断的 400。
///
/// ★ **刻意不回落**到任何既有平台：把 Qoder 的请求静默打到 WorkBuddy 的账号库上，
/// 比报错坏得多（用户会看到「账号凭空消失」或「改错了别的平台的号」）。
fn qoder_scope_not_applicable() -> Response {
    json_err(
        "该端点只服务 WorkBuddy / Trae 作用域，不支持 Qoder —— \
         Qoder 的账号管理请用 Qoder 分区自己的端点（/api/qoder/*）。"
            .to_string(),
        StatusCode::BAD_REQUEST,
    )
}

/// 解析 `?scope=`（GET）—— 失败即 400，**不回落默认作用域**。
fn qoder_region_from_query(query: Option<&str>) -> Result<QoderRegion, String> {
    let scope = resolve_scope_param(query_value(query, "scope").as_deref())?;
    require_qoder_region(scope)
}

/// 从请求体取 `scope` 并解析成区域。
fn qoder_region_from_body(body: &Value) -> Result<QoderRegion, String> {
    let scope = resolve_scope_param(body.get("scope").and_then(Value::as_str))?;
    require_qoder_region(scope)
}

/// GET /api/qoder/accounts —— 账号列表（**不含令牌明文**）。
async fn api_qoder_accounts(RawQuery(query): RawQuery) -> Response {
    match qoder_region_from_query(query.as_deref()) {
        Ok(region) => json_ok(qoder::account::accounts_view_for(region)),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/local-sources —— 本机凭据来源探测（**只读、不落库**）。
async fn api_qoder_local_sources(RawQuery(query): RawQuery) -> Response {
    match qoder_region_from_query(query.as_deref()) {
        Ok(region) => json_ok(qoder::profile::probe_view_for(region)),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/accounts/import-local —— 从本机已登录的客户端导入账号。
async fn api_qoder_import_local(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match tokio::task::spawn_blocking(move || qoder::profile::import_from_local_for(region)).await {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("导入 Qoder 本机账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/qoder/accounts/import-pat —— 由 PAT 导入一条账号（**纯本地**，不访问上游）。
async fn api_qoder_import_pat(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let pat = body
        .get("pat")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let nickname = body
        .get("nickname")
        .and_then(Value::as_str)
        .map(str::to_string);
    match tokio::task::spawn_blocking(move || {
        qoder::profile::import_from_pat_for(region, &pat, nickname)
    })
    .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("导入 Qoder PAT 失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/qoder/accounts/remark —— 改备注（唯一的「只改一个字段」入口）。
async fn api_qoder_set_account_remark(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = body
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let remark = body
        .get("remark")
        .and_then(Value::as_str)
        .map(str::to_string);
    match tokio::task::spawn_blocking(move || qoder::account::set_remark_for(region, &uid, remark))
        .await
    {
        Ok(Ok(changed)) => json_ok(json!({ "ok": changed })),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("保存 Qoder 备注失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/qoder/accounts/delete —— 删除一条账号（**软删除**：移入回收站，可恢复）。
async fn api_qoder_delete_account(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = body
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match tokio::task::spawn_blocking(move || qoder::trash::trash_account_for(region, &uid)).await {
        Ok(Ok(removed)) => json_ok(json!({ "ok": removed })),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("删除 Qoder 账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/qoder/accounts/refresh —— 刷新一条账号的令牌（device / job / PAT 三路由）。
async fn api_qoder_refresh_account(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = body
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match qoder::refresh::refresh_for(region, &uid).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/checkin/status —— 区域签到状态（逐账号 + 汇总；国际版如实报未开放）。
async fn api_qoder_checkin_status(RawQuery(query): RawQuery) -> Response {
    match qoder_region_from_query(query.as_deref()) {
        Ok(region) => json_ok(qoder::checkin::status_for(region).await),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/checkin/status-map —— 逐账号「今日签到状态」（**只读**；issue #47）。
///
/// `scope` 决定区域（`qoder-cn` / `qoder-global`），`accountIds` 是逗号分隔的 uid 列表。
/// 未知 uid **不进** map；读失败回 `{ok:false, error}` —— 前端据 `ok` 区分「没签」与
/// 「读不到」。**绝不**在这里调 claim。
async fn api_qoder_checkin_status_map(RawQuery(query): RawQuery) -> Response {
    let region = match qoder_region_from_query(query.as_deref()) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let ids = parse_id_list(query_value(query.as_deref(), "accountIds"));
    json_ok(qoder::checkin::status_map_for(region, ids).await)
}

/// POST /api/qoder/checkin/claim —— 单账号签到（状态驱动；见 checkin 模块头）。
async fn api_qoder_checkin_claim(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = body
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match qoder::checkin::claim_for(region, &uid).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/pro-pack/status —— Pro 升级包资格（**只读**）。
///
/// `available=false` ⇒ 活动已下线 / 本区域没有（真机 404 就是这一档）⇒ 前端**不渲染**入口。
async fn api_qoder_pro_pack_status(RawQuery(query): RawQuery) -> Response {
    let region = match qoder_region_from_query(query.as_deref()) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = query_value(query.as_deref(), "uid").unwrap_or_default();
    match qoder::checkin::pro_pack_status_for(region, &uid).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/pro-pack/claim —— 领取 Pro 升级包（幂等；已领过 `already=true`）。
async fn api_qoder_pro_pack_claim(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = body.get("uid").and_then(Value::as_str).unwrap_or("");
    match qoder::checkin::pro_pack_claim_for(region, uid).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/credits —— 区域额度 / 套餐视图（逐账号 + 汇总；部分失败如实报）。
async fn api_qoder_credits(RawQuery(query): RawQuery) -> Response {
    match qoder_region_from_query(query.as_deref()) {
        Ok(region) => {
            let view = qoder::credits::region_view(region).await;
            // ★ 顺手喂**限额护栏**的余额快照（G1 的唯一数据来源）：
            //   余额是慢变数据，只有这里真的查过一次才有值；没查过 ⇒ G1 恒不触发。
            //   「余额 > 0」也是解除 402 停放的唯一路径（见 `quota::note_credits_view`）。
            buddy_switch_gateway::qoder::quota::note_credits_view(region, &view);
            json_ok(view)
        }
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/oauth/start —— 发起设备授权，返回 `{ loginId, verificationUri, expiresIn }`。
///
/// ⚠️ **不发网络**（只读本机 machine_id + 本地算 PKCE），故无需 `spawn_blocking`。
async fn api_qoder_oauth_start(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match qoder::oauth::login_start_for(region).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/oauth/status —— 轮询一次登录结果（`{ done, result?, error? }`）。
///
/// ★ 形状与 WB 的 `/api/oauth/status` **逐字一致**：前端 `OAuthLoginDialog` 是共用壳，
/// 它只读 `done` / `result` / `error` 三个键。
async fn api_qoder_oauth_status(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let login_id = body
        .get("loginId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    json_ok(qoder::oauth::login_poll_for(region, &login_id).await)
}

/// GET /api/qoder/programs —— 程序位状态（只读；界面据此渲染胶囊行与「当前登录」徽章）。
async fn api_qoder_program_status(RawQuery(query): RawQuery) -> Response {
    match qoder_region_from_query(query.as_deref()) {
        Ok(region) => {
            match tokio::task::spawn_blocking(move || qoder::switch::program_status_for(region))
                .await
            {
                Ok(value) => json_ok(value),
                Err(error) => json_err(
                    format!("读取 Qoder 程序位状态失败: {error}"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                ),
            }
        }
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/accounts/switch —— 切换账号（把凭据写回指定程序位的登录态）。
///
/// ⚠️ 写客户端文件（与 `switch_account` 同级）：客户端在跑时被守卫拒绝，失败自动还原备份。
async fn api_qoder_switch_account(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let raw_program = body
        .get("program")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let Some(program) = qoder::switch::QoderProgram::parse(&raw_program) else {
        return json_err(
            format!("未知的 Qoder 程序位: {raw_program}"),
            StatusCode::BAD_REQUEST,
        );
    };
    let uid = body
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match tokio::task::spawn_blocking(move || {
        qoder::switch::switch_account_for(region, program, &uid)
    })
    .await
    {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("切换 Qoder 账号失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/qoder/accounts/group —— 改自定义分组（与备注同构）。
async fn api_qoder_set_account_group(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let uid = body
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let group = body
        .get("group")
        .and_then(Value::as_str)
        .map(str::to_string);
    match tokio::task::spawn_blocking(move || qoder::account::set_group_for(region, &uid, group))
        .await
    {
        Ok(Ok(changed)) => json_ok(json!({ "ok": changed })),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(
            format!("保存 Qoder 分组失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// GET /api/qoder/usage —— 逐日用量（**单位是 credits，不是 token**；理由见 `qoder::usage`）。
async fn api_qoder_usage(RawQuery(query): RawQuery) -> Response {
    let region = match qoder_region_from_query(query.as_deref()) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let days = query_value(query.as_deref(), "days").and_then(|raw| raw.trim().parse::<u32>().ok());
    match tokio::task::spawn_blocking(move || qoder::usage::usage_view_for(region, days)).await {
        Ok(value) => json_ok(value),
        Err(error) => json_err(
            format!("读取 Qoder 用量失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

// ---------------------------------------------------------------------------
// Qoder API 网关（管理面）
// ---------------------------------------------------------------------------
//
// 与 Trae 侧的管理面形状一致（config / status / models / keys / logs），差异有两处：
// ① 归属轴是**区域**而不是程序位（Key 绑定 `region`）；② **一个监听服务两个区域** ——
//    监听是否起来、端口是多少是两区域共用的事实，只有池 / Key 前缀按 `scope` 分家。

/// GET /api/qoder/gateway/config —— Qoder 网关配置（**全局唯一**，不分区域）。
async fn api_qoder_gateway_config() -> Response {
    match serde_json::to_value(buddy_switch_gateway::qoder::QoderGatewayConfig::load()) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/qoder/gateway/config —— 保存配置并应用（启动 / 重启独立监听）。
///
/// ⚠️ `max_body_mb` 是**构造期**固化进 axum `DefaultBodyLimit` 的，改它要等下一次
/// `apply()` —— 而本 handler 每次都会 `apply()`，所以只要走这条路由就一定生效。
async fn api_save_qoder_gateway_config(Json(body): Json<Value>) -> Response {
    if let Err(error) = require_capability(Product::Qoder, capability::Capability::Gateway) {
        return json_err(error, StatusCode::FORBIDDEN);
    }
    let submitted = body.get("config").cloned().unwrap_or(body);
    let config: buddy_switch_gateway::qoder::QoderGatewayConfig =
        match serde_json::from_value(submitted) {
            Ok(config) => config,
            Err(error) => {
                return json_err(format!("配置格式错误: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    if let Err(error) = config.save() {
        return json_err(error, StatusCode::BAD_REQUEST);
    }
    let state = crate::qoder_gateway_host::shared_state();
    *state.config.write().await = config.clone();
    state.log.set_keep(config.log_keep);
    state.log.set_log_bodies(config.log_bodies);
    let addr = match crate::qoder_gateway_host::apply().await {
        Ok(addr) => addr,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    json_ok(json!({
        "ok": true,
        "config": config,
        "running": addr.is_some(),
        "addr": addr,
    }))
}

/// POST /api/trae/gateway/pool/exclusions —— 设置 / 取消排除（`{variant?, uid, excluded, note?}`）。
async fn api_set_trae_pool_exclusion(Json(body): Json<Value>) -> Response {
    let uid = body.get("uid").and_then(Value::as_str).unwrap_or("").trim();
    if uid.is_empty() {
        return json_err("缺少 uid".to_string(), StatusCode::BAD_REQUEST);
    }
    let excluded = body
        .get("excluded")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let note = body.get("note").and_then(Value::as_str).unwrap_or("");
    let variant = parse_trae_variant(body.get("variant").and_then(Value::as_str));
    let scope = format!("trae:{}", variant.as_str());

    match buddy_switch_gateway::routes::pool_admin::persist(&scope, uid, excluded, note) {
        Ok(value) => {
            if excluded {
                crate::trae_gateway_host::shared_state()
                    .sticky
                    .lock()
                    .await
                    .purge_uid(uid);
            }
            json_ok(value)
        }
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/qoder/gateway/pool/exclusions —— 设置 / 取消排除。
async fn api_set_qoder_pool_exclusion(Json(body): Json<Value>) -> Response {
    let uid = body.get("uid").and_then(Value::as_str).unwrap_or("").trim();
    if uid.is_empty() {
        return json_err("缺少 uid".to_string(), StatusCode::BAD_REQUEST);
    }
    let excluded = body
        .get("excluded")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let note = body.get("note").and_then(Value::as_str).unwrap_or("");
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let scope = format!("qoder:{}", region.as_str());

    match buddy_switch_gateway::routes::pool_admin::persist(&scope, uid, excluded, note) {
        Ok(value) => {
            if excluded {
                crate::qoder_gateway_host::shared_state()
                    .sticky
                    .lock()
                    .await
                    .purge_uid(uid);
            }
            json_ok(value)
        }
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /api/qoder/gateway/status —— 运行状态 + **该区域**账号池 + 账号明细 + 诊断。
///
/// `scope` 必填且 fail-closed（`qoder-cn` / `qoder-global`）：Qoder 的双区域是两本
/// 互不相通的账号库，缺 scope 就无从判断该看哪本 —— **不**回落任意一本。
async fn api_qoder_gateway_status(RawQuery(query): RawQuery) -> Response {
    let region = match qoder_region_from_query(query.as_deref()) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let (running, addr) = crate::qoder_gateway_host::status().await;
    json_ok(
        buddy_switch_gateway::qoder::status_view(
            &crate::qoder_gateway_host::shared_state(),
            running,
            addr,
            update::APP_VERSION,
            region,
        )
        .await,
    )
}

/// GET /api/qoder/gateway/quota —— **限额护栏**视图（逐账号当日用量 + 拦截原因）。
///
/// `scope` 必填且 fail-closed（与 `/status` 同一条纪律）：两个区域的计数是**两份文件**，
/// 缺 scope 就无从判断该看哪一份。
///
/// 表里的 `blocked` / `blockedReason` 由**选号用的同一个函数**产出
/// （`quota::blocked`）⇒ 面板说「已拦」就是真的会拦，不会两处口径。
async fn api_qoder_gateway_quota(RawQuery(query): RawQuery) -> Response {
    match qoder_region_from_query(query.as_deref()) {
        Ok(region) => {
            let state = crate::qoder_gateway_host::shared_state();
            json_ok(state.quota_view(region).await)
        }
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/gateway/models —— 对外暴露的模型清单（**随上游刷新**）。
///
/// 与 Trae 侧不同：Qoder **没有静态常量清单**，清单只能向官方拉（需账号凭据）。
/// 拉不到时给空清单 + 可读 `note` + `source`（`live` / `cached` / `unavailable`），
/// **绝不**伪造一份「像真的」清单。
///
/// `?refresh=1`（或 `refresh=true`）绕过 10 分钟缓存强制重拉（管理面「重新识别」按钮）。
/// 响应形状由 gateway crate 的 `payload::admin_models_response` **唯一产出**
/// （两宿主共用，本层不拼装）。
async fn api_qoder_gateway_models(RawQuery(query): RawQuery) -> Response {
    let region = match qoder_region_from_query(query.as_deref()) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let force = matches!(
        query_value(query.as_deref(), "refresh").as_deref(),
        Some("1") | Some("true")
    );
    let state = crate::qoder_gateway_host::shared_state();
    let catalog = if force {
        state.refresh_catalog(region).await
    } else {
        state.catalog_for(region).await
    };
    // ★ 出口 ①（P11）：与网关本体（`qoder/routes.rs` 的 `models`）读**同一份** `disabled`
    //   （按**作用域**键控），否则「管理面看到的清单」与「网关实际暴露的清单」会不一致。
    let disabled = {
        let guard = state.disabled.read().await;
        guard
            .get(buddy_switch_core::modules::product::scope_of_qoder_region(
                region,
            ))
            .clone()
    };
    json_ok(buddy_switch_gateway::qoder::payload::admin_models_response(
        region,
        &catalog,
        &disabled,
        state.max_context_now(),
    ))
}

/// GET /api/qoder/gateway/keys —— 多 Key 列表（含归属区域，形状与 Tauri 命令逐字一致）。
///
/// **形状唯一来源**：直接调 gateway crate 的 `list`，本层不拼装
/// （`MEMORY.md §二`：`copy_sessions` 事故的直接对策）。
async fn api_qoder_list_api_keys() -> Response {
    let state = crate::qoder_gateway_host::shared_state();
    json_ok(json!({ "keys": state.key_store.list() }))
}

/// POST /api/qoder/gateway/keys —— 新建 Key（`{name, scope}`）。
///
/// 明文仅此一次返回；`scope` 决定归属区域（fail-closed）。
async fn api_qoder_create_api_key(Json(body): Json<Value>) -> Response {
    let region = match qoder_region_from_body(&body) {
        Ok(region) => region,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未命名 Key")
        .to_string();
    let state = crate::qoder_gateway_host::shared_state();
    match state.key_store.create(name, region) {
        Ok((record, plaintext)) => json_ok(json!({
            "ok": true,
            "key": plaintext,
            "record": record.masked(),
        })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/gateway/keys/revoke —— 吊销 Key（`{id}`）。
async fn api_qoder_revoke_api_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::qoder_gateway_host::shared_state();
    match state.key_store.revoke(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/gateway/keys/policy —— 更新某把 Qoder Key 的策略（issue #76）。
async fn api_qoder_set_key_policy(Json(body): Json<Value>) -> Response {
    use buddy_switch_gateway::apikey::KeyPolicy;
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let policy: KeyPolicy =
        match serde_json::from_value(body.get("policy").cloned().unwrap_or(Value::Null)) {
            Ok(policy) => policy,
            Err(error) => {
                return json_err(format!("策略字段不合法: {error}"), StatusCode::BAD_REQUEST)
            }
        };
    let state = crate::qoder_gateway_host::shared_state();
    match state.key_store.update_policy(id, policy) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/qoder/gateway/keys/usage?id=… —— 读某把 Qoder Key 的用量快照。
///
/// ⚠️ Qoder 的 `key_store.list()` 返回**掩码视图**（`Vec<Value>`）——按 `id` 取出后
/// 从 JSON 键读策略（`quotaUnit` / `quotaPeriod`），不是记录本体。
async fn api_qoder_key_usage(RawQuery(query): RawQuery) -> Response {
    let id = query_value(query.as_deref(), "id").unwrap_or_default();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::qoder_gateway_host::shared_state();
    let now = buddy_switch_core::modules::config::now_ms();
    let Some(row) = state
        .key_store
        .list()
        .into_iter()
        .find(|row| row["id"] == serde_json::json!(id))
    else {
        return json_err("API Key 不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    // 掩码视图是 camelCase ⇒ 按字符串解析枚举（与前端同形态）。
    let policy: buddy_switch_gateway::apikey::KeyPolicy = match serde_json::from_value(row.clone())
    {
        Ok(policy) => policy,
        Err(error) => {
            return json_err(
                format!("策略字段异常: {error}"),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        }
    };
    let usage = state.key_usage.snapshot(&id);
    let used = state
        .key_usage
        .used_for(&id, policy.quota_unit, policy.quota_period, now);
    let exceeded = matches!(
        buddy_switch_gateway::apikey::quota_verdict_for_policy(&policy, used),
        buddy_switch_gateway::apikey::QuotaVerdict::Exceeded { .. }
    );
    json_ok(json!({
        "ok": true,
        "keyId": id,
        "unit": policy.quota_unit,
        "period": policy.quota_period,
        "periodKey": policy.quota_period.period_key(now),
        "used": used,
        "total": usage
            .as_ref()
            .map(|entry| entry.counters(policy.quota_unit).total)
            .unwrap_or(0),
        "exceeded": exceeded,
    }))
}

/// POST /api/qoder/gateway/keys/restore —— 撤销吊销（Key 重新可用，`{id}`）。
async fn api_qoder_restore_api_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::qoder_gateway_host::shared_state();
    match state.key_store.restore(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/qoder/gateway/keys/delete —— 物理删除已吊销的 Key（`{id}`）。
async fn api_qoder_delete_api_key(Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return json_err("缺少 id".to_string(), StatusCode::BAD_REQUEST);
    }
    let state = crate::qoder_gateway_host::shared_state();
    match state.key_store.delete(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_qoder_gateway_logs() -> Response {
    let state = crate::qoder_gateway_host::shared_state();
    json_ok(json!({ "logs": state.log.list() }))
}

async fn api_qoder_clear_gateway_logs() -> Response {
    let state = crate::qoder_gateway_host::shared_state();
    state.log.clear();
    json_ok(json!({ "ok": true }))
}
