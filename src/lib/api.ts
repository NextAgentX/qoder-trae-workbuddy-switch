import { invoke } from "@tauri-apps/api/core";
import type {
  AccountMeta,
  AccountRecord,
  AccountStrategy,
  AccountStrategyMap,
  AccountTokenTodayResponse,
  ApiKeyRecord,
  AppStatus,
  AutoBackupConfig,
  AutoBackupEntry,
  CacheCategoryId,
  CacheClearResult,
  CacheScanReport,
  AutoRotateConfig,
  BackupConflictPolicy,
  CatalogSnapshot,
  CodeBuddyCliInstallResult,
  CodeBuddyCliStatus,
  CodeBuddyCliSwitchResult,
  CodeBuddyCnIdeStatus,
  CodeBuddyCnIdeSwitchResult,
  CodebuddyPluginDetectResult,
  CodebuddyPluginSwitchResult,
  CheckinConfig,
  CheckinLog,
  CheckinResult,
  ClientLimitsResponse,
  ClientSettingsBackupsResponse,
  ClientSettingsDiffResponse,
  ClientSettingsReadResponse,
  ClientSettingsSlotsResponse,
  ClientSettingsWriteResult,
  CreateApiKeyResult,
  CreditExpiry,
  CreditStatistics,
  TokenStatistics,
  CopyResult,
  MigrateResult,
  ModelProbeJob,
  ModelProbeLedger,
  GatewayConfig,
  GatewayLogEntry,
  GatewayStatus,
  GatewayTokenTrend,
  GatewayUsageDim,
  GatewayUsageRecon,
  AppPrefs,
  GithubConfig,
  ImportPreview,
  ImportPreviewAccount,
  ImportResult,
  ImportSummary,
  JetbrainsCodebuddyPluginStatus,
  OAuthPollResult,
  OAuthStartResult,
  ProxyStatus,
  Region,
  RegionFilter,
  RotateLog,
  RotateStatus,
  RuntimeLogClearResult,
  RuntimeLogQuery,
  RuntimeLogsResponse,
  ScheduleConfig,
  ScheduleRunResult,
  Session,
  SessionReplicaResponse,
  SkillCopyReport,
  SkillImportResult,
  SkillListResponse,
  SkillToggleResult,
  SyncSessionReplicaItem,
  SyncSessionReplicaResponse,
  UnifySessionReplicaItem,
  UnifySessionReplicaResponse,
  ExtClientId,
  ExtCopyItem,
  ExtCopyResponse,
  ExtDiffResponse,
  ExtSessionsResponse,
  ExtSyncResponse,
  FallbackPreview,
  FallbackStats,
  ModelFallbackView,
  ScopeFallbackConfig,
  SwitchConfig,
  SwitchResult,
  TrashList,
  TravelConfig,
  TravelStatus,
  UpdateInfo,
  VscodeCodebuddyPluginStatus,
} from "./types";
import { demoModeEnabled, demoUnavailableMessage } from "./demo-mode";
import { displayText } from "./display-text";
import { localizeCodedStrings, localizeError } from "./error-code";
import { t } from "./i18n";
import { screenshotDemoResponse } from "./screenshot-demo";
// P5 T03：参数化命令需要把「平台 + 区域」拼成 `scope`（值导入，非类型）。
import { scopeOf, type Scope } from "./scope";
import { traeRegionOf } from "./trae-types";
import type {
  TraeAccount,
  TraeAccountsOverview,
  TraeApiKeyCreated,
  TraeApiKeyRecord,
  TraeCapabilities,
  TraeCheckinReport,
  TraeCheckinStatus,
  TraeClientModelList,
  TraeCreditsOverview,
  TraeDeviceResetReport,
  TraeEnvStatus,
  TraeExportRecord,
  TraeGatewayConfigRaw,
  TraeGatewayLogEntry,
  TraeGatewayStatus,
  TraeGroup,
  TraeImportPreview,
  TraeLogQuery,
  TraeLogsResponse,
  TraeOAuthPollResult,
  TraeOAuthStartResult,
  TraeProfilesOverview,
  TraeSettings,
  TraeSwitchOutcome,
  TraeTokenScope,
  TraeLegacyMergeReport,
  TraeTokenStatistics,
  TraeVariantId,
  TraeVariantsStatus,
} from "./trae-types";
// P9 · 悬浮窗只读快照的类型（**type-only**：`companion.ts` 反向引用本模块的 `call`，
// 用 `import type` 把运行期依赖收成单向，避免循环导入）。
import type { CompanionSnapshot } from "./companion";
// Qoder 网关配置的**写回形状**（snake_case）—— 同样是 type-only（`qoder-gateway.ts`
// 不依赖本模块，这里只是复用它的形状定义，避免两处各写一份会漂移的接口）。
import type { QoderGatewayConfigRaw } from "./qoder-gateway";

/**
 * 双通道适配层：
 * - 桌面 App（Tauri）：`invoke` 调用 Rust commands
 * - webui（浏览器）：HTTP fetch 调用本地 buddy-switch 服务（127.0.0.1）
 *
 * ## webui 的基址为什么不能硬编码（2026-09-30 用户报障）
 *
 * 原先这里是 `const API_BASE = "http://127.0.0.1:57890"`（默认端口），于是
 * `buddy-switch serve --port 60000` 打开的页面**能渲染、却整页「操作失败」**：
 * 页面由 60000 提供，JS 却去请求 57890 —— 那里没有服务。
 * ⇒ 那个「可用 --port 指定其他端口」的参数**形同虚设**，而报错信息（`unreachable`）
 * 只说「连不上」，不指向「基址写死了」。
 *
 * 修法按**构建模式**分（`import.meta.env.DEV` 由 Vite 在构建期固化）：
 * - **生产构建**：`dist` 只会被 `serve` 或 Tauri 托管，同源 ⇒ 用 `location.origin`，
 *   于是页面在哪个端口，API 就在哪个端口。
 * - **dev**（`npm run dev`，页面来自 Vite 的 1420）：API 在**另一个进程**里，
 *   必须显式指向它 —— 保留原默认端口。
 *
 * 演示构建（`build:demo`）不受影响：`demoModeEnabled` 时 `call()` 走本地假数据，
 * 根本不进 [`httpCall`]。
 */
const API_BASE =
  import.meta.env.DEV || typeof window === "undefined"
    ? "http://127.0.0.1:57890"
    : window.location.origin;

const DEMO_READ_COMMANDS = new Set([
  "get_status", "get_accounts",
  // P5 T03 · 参数化只读命令（前端改调它，故演示模式必须有对应数据）。
  "accounts_list",
  "credits",
  "checkin_status",
  "get_codebuddy_cli_status", "get_codebuddy_cn_ide_status", "get_checkin_status",
  // 模型降级链（P13）：`get` 与 `preview` 都是**只读**语义（preview 零副作用），
  // 演示模式需要对应快照。
  "get_model_fallback",
  "preview_model_fallback",
  "get_fallback_stats",
  // 编辑器内 CodeBuddy 插件（VS Code / JetBrains）的**只读**状态。
  // ⚠️ switch / detect 是动作（detect 会读本机 secret），一律不进这里。
  "get_vscode_codebuddy_plugin_status", "get_jetbrains_codebuddy_plugin_status",
  "get_credit_expiry", "get_credit_statistics", "get_auto_checkin_config",
  "get_token_statistics",
  "get_checkin_logs", "get_auto_rotate_config", "rotate_status", "get_rotate_logs",
  "get_github_config", "check_update", "get_launch_at_login_enabled", "switch_progress",
  "get_app_prefs", "set_app_prefs",
  "get_travel_status", "get_auto_travel_config", "get_schedule_config",
  "get_switch_config",
  // P8 T02 · 会话页两个只读命令：演示站必须能渲染副本视图（否则 build:demo 报错）。
  // `list_sessions` 是**既有**命令，这里补进演示面 —— 会话页要用它 join 标题 / updatedAt。
  "list_sessions", "list_session_replicas",
  // API 网关只读命令（演示站需返回虚构数据，否则 build:demo 报错）
  "get_gateway_config", "gateway_status", "list_api_keys", "get_gateway_models",
  // ⚠️ 图片能力**读取**只读，进演示面；`set_model_image_cap` 是写操作，**不进**（与禁用同款）。
  "get_account_strategy", "get_disabled_models", "get_model_image_caps",
  "get_gateway_logs",
  // 限流探测的只读状态命令（probe_status）：演示站需返回未跑过（phase=idle）的快照。
  // `probe_ledger` 是**只读**台账（跨重启的实测结论）：演示站返回空表（从未探过）。
  // ⚠️ probe_start / probe_cancel 是**写**操作（会真发上游请求 / 置取消位），一律不进这里。
  "probe_status", "probe_ledger",
  // 「今日 Token」（**只读**观测）：演示站需返回虚构数据，否则 build:demo 报错。
  "get_account_token_today",
  // 网关**逐日**趋势（只读 `token_daily` 历史）：趋势页的网关小图用，演示站同样要虚构数据。
  "get_gateway_token_trend",
  // 账本视图（issue #63）：两个只读命令，演示站同样要虚构数据。
  "get_gateway_usage_dim", "get_usage_reconcile",
  // 账本视图的平台补齐（Trae / Qoder 的网关账本，返回体与 WB 同形）。
  "get_trae_gateway_usage_dim", "get_qoder_gateway_usage_dim",
  // Trae 客户端账本（读用户自己导出的明文库）—— 演示站虚构一份「未导入」状态。
  "probe_trae_client_db", "get_trae_client_token_stats", "get_trae_client_db_config",
  // Trae 分区只读命令（演示站需返回虚构数据，否则 build:demo 报错）。
  // 只登记**只读**命令：写操作（签到 / 增删账号 / 切换 / 重置设备…）一律不进这里，
  // 由 `DemoAction` 包裹后在演示模式下统一提示不可操作。
  "get_trae_env", "get_trae_variants", "get_trae_capabilities", "get_trae_accounts",
  "get_trae_checkin_status",
  // issue #47：逐账号「今日」只读重取（**只读**；演示站同样要虚构数据）。
  "get_trae_checkin_status_map", "get_trae_account_token_today",
  "get_trae_credits", "get_trae_token_statistics", "get_trae_logs", "get_trae_profiles",
  "get_trae_settings", "get_trae_gateway_config", "trae_gateway_status",
  "get_trae_gateway_models", "get_trae_client_models", "list_trae_api_keys",
  "get_trae_gateway_logs",
  // P9 · 悬浮窗只读快照（演示站需返回虚构数据，否则 build:demo 报错）。
  // ⚠️ 只登记**只读**命令：`companion_set_visible` / `companion_toggle` / `companion_jump`
  // 是显隐 / 聚焦（非只读），一律不进这里。
  "companion_status",
  // P15 · 外部 Agent 会话监听（**只读**状态）：演示站需返回虚构数据，否则 build:demo 报错。
  // ⚠️ `set_agent_hook_enabled` 是**写**（会改上游客户端配置），一律不进这里 ——
  // 演示环境绝不能出现「点了开关就改本机客户端配置」的假动作。
  "get_agent_hook_status",
  // P7 全局备份**只读**命令（导出 / 自动备份列表 / 自动备份配置）。
  // 预览 / 导入 / 恢复 / 保存配置是**动作**，一律不进这里（由页面侧处理不可用）。
  "export_backup", "list_auto_backups", "get_auto_backup_config",
  // 客户端限流台账（**只读**观测）：演示站需返回虚构数据，否则 build:demo 报错。
  "client_limits",
  // Skill 管理（**只读**清单）：演示站需返回虚构数据，否则 build:demo 报错。
  // ⚠️ 启停 / 跨区搬迁 / 导入 / 打开目录都是写 / 动作，一律不进这里。
  "list_skills",
  // 上游客户端 settings.json 托管（**只读** 4 条）：演示站需返回虚构数据，否则 build:demo 报错。
  // ⚠️ patch（写）/ restore（写）是写操作，一律不进这里（由页面侧处理不可用）。
  "list_client_settings_slots", "read_client_settings",
  "list_client_settings_backups", "diff_client_settings",
  // Qoder 平台（**只读**）：演示站需返回虚构数据，否则 build:demo 报错。
  // ⚠️ 导入 / 改备注 / 删除都是**写**，一律不进这里（由页面侧处理不可用）。
  // 网关五条只读：config / status / models / keys / logs —— 同样不进写操作
  //（保存配置 / 建 Key / 吊销 / 恢复 / 删除 / 清日志）。
  "get_qoder_accounts", "get_qoder_local_sources", "qoder_program_status", "qoder_usage",
  // issue #111 · 逐包链路（三条都只读；演示站要有数据，否则卡片指标行与浮层无法截图）。
  // ⚠️ `qoder_web_window_cookies` 也登记：它只回「登录窗里有哪些 cookie（名称/条数/指纹）」，
  //    零副作用、不打上游；不登记的话演示模式下前端轮询会抛「缺少只读数据」。
  "qoder_credits", "qoder_web_credit_packs", "qoder_web_session_status",
  "qoder_web_window_cookies",
  // ⚠️ `qoder_web_proxy_set` 是写操作，**不进**这里（保存失败走页面错误提示）。
  "qoder_web_proxy_get",
  // issue #47：逐账号「今日签到状态」（**只读**）。
  "qoder_checkin_status_map",
  "get_qoder_gateway_config", "qoder_gateway_status", "get_qoder_gateway_models",
  // 限额护栏视图（只读；逐账号当日用量 + 拦截原因）。
  "qoder_gateway_quota",
  "list_qoder_api_keys", "get_qoder_gateway_logs",
  // 运行日志（**只读**）：演示站需返回虚构数据，否则 build:demo 报错。
  // ⚠️ clear_runtime_logs（删文件）/ open_runtime_logs_dir（起 GUI 进程）是**动作**，
  //    一律不进这里 —— 演示环境绝不能真的删本机日志或弹文件管理器。
  "runtime_logs",
]);

export function isDemoMode(): boolean {
  return demoModeEnabled;
}

export function isWebui(): boolean {
  return typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}

/** Tauri mobile 也注入内部 API；用现有平台 UA 约定把桌面宿主与移动宿主区分开。 */
function isMobilePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  const ua = navigator.userAgent;
  return (
    /Android|iPhone|iPad|iPod/i.test(ua) ||
    (ua.includes("Macintosh") && navigator.maxTouchPoints > 1)
  );
}

/** 是否为提供桌面专属能力的 Tauri 宿主。 */
export function isDesktop(): boolean {
  return !isWebui() && !isMobilePlatform();
}

type Route = { method: "GET" | "POST"; path: string };

/** Tauri command → HTTP 路由映射（webui 模式）。 */
const ROUTES: Record<string, Route> = {
  get_status: { method: "GET", path: "/api/status" },
  // 缓存清理（「清除缓存」分区）：全局单目录（`~/.buddy-switch`），**无 scope**。
  // `scan` 只读量体积；`clear` 按类别删（不可逆）。
  cache_scan: { method: "GET", path: "/api/cache/scan" },
  cache_clear: { method: "POST", path: "/api/cache/clear" },
  get_accounts: { method: "GET", path: "/api/accounts" },
  // P5 T03 · 参数化命令（`scope` 决定平台与区域）。旧路由保留为别名（见 ALIASES）。
  accounts_list: { method: "GET", path: "/api/accounts/list" },
  account_remark: { method: "POST", path: "/api/accounts/remark" },
  account_delete: { method: "POST", path: "/api/accounts/delete" },
  // P5 §2.7 · 回收站 4 命令（**全新命令**，非参数化、无别名）。`scope` 决定平台与区域，
  // 从而决定读 / 写哪个 `*.trash.<region>.json`；非法 scope ⇒ 后端 4xx（fail-closed）。
  account_trash_list: { method: "GET", path: "/api/accounts/trash" },
  account_restore: { method: "POST", path: "/api/accounts/trash/restore" },
  account_purge: { method: "POST", path: "/api/accounts/trash/purge" },
  account_trash_clear: { method: "POST", path: "/api/accounts/trash/clear" },
  // 账号导出 / 导入（区域纯）：`scope` 决定平台与区域；旧路由保留为别名（见 ALIASES）。
  accounts_export: { method: "POST", path: "/api/accounts/export" },
  accounts_export_to_path: { method: "POST", path: "/api/accounts/export-to-path" },
  accounts_import: { method: "POST", path: "/api/accounts/import" },
  // 从本机客户端登录态导入账号（区域纯）：`scope` 决定平台与区域；旧路由 `import_local`（WB）
  // / `trae_import_local_account`（Trae）保留为别名（见 ALIASES）。名字**不**沿用 §2.3 的
  // `import_local`（与 WB 旧命令同名），改取 `accounts_import_local`（与 `accounts_import` 同族）。
  accounts_import_local: { method: "POST", path: "/api/accounts/import-local" },
  // 粘贴授权凭据（issue #62，无客户端部署）：只接 WorkBuddy —— 另两家各有专用入口
  // （Qoder 粘贴 PAT、Trae 粘贴 JWT）。凭据落库走与 `accounts_import_local` 同一条链。
  accounts_paste_credential: { method: "POST", path: "/api/accounts/paste-credential" },
  // 打开客户端数据目录（只读路径）：`scope` 决定平台与区域；旧路由 `open_workbuddy_data_dir`
  // / `open_trae_data_dir` 保留为别名（见 ALIASES）。
  open_data_dir: { method: "POST", path: "/api/data-dir/open" },
  // 积分（区域纯）：`scope` 决定平台与区域；旧路由 `get_credit_expiry` / `get_trae_credits`
  // 保留为别名（见 ALIASES）。
  credits: { method: "POST", path: "/api/credits/query" },
  // 签到状态（区域纯）：`scope` 决定平台与区域；旧路由 `get_checkin_status`（WB，**批量**）
  // / `get_trae_checkin_status`（Trae）保留为别名（见 ALIASES）。本路由对齐**桌面端单账号**
  // 契约，webui 的批量语义仍走旧 `GET /api/checkin/status`。
  checkin_status: { method: "POST", path: "/api/checkin/status/query" },
  get_codebuddy_cli_status: { method: "GET", path: "/api/codebuddy-cli/status" },
  install_codebuddy_cli_helper: { method: "POST", path: "/api/codebuddy-cli/install-helper" },
  // issue #109 §PR-2：一键把 CLI 切到本机网关（建 Key + 写 settings + 关轮换）。
  codebuddy_cli_enable_gateway: { method: "POST", path: "/api/codebuddy-cli/enable-gateway" },
  codebuddy_cli_disable_gateway: { method: "POST", path: "/api/codebuddy-cli/disable-gateway" },
  switch_codebuddy_cli_account: { method: "POST", path: "/api/codebuddy-cli/switch" },
  get_codebuddy_cn_ide_status: { method: "GET", path: "/api/codebuddy-cn-ide/status" },
  switch_codebuddy_cn_ide_account: { method: "POST", path: "/api/codebuddy-cn-ide/switch" },
  detect_codebuddy_cn_ide_account: { method: "POST", path: "/api/codebuddy-cn-ide/detect" },
  // P0 §3.3 B 表（**待新增 → 已落地**）：编辑器内 CodeBuddy 插件两个程序位。
  // 路径与宿主 `commands.rs` / `api.rs` 的 `/api/<name>/…` 一一对应，命令名即路由键。
  get_vscode_codebuddy_plugin_status: { method: "GET", path: "/api/vscode-codebuddy-plugin/status" },
  switch_vscode_codebuddy_plugin_account: { method: "POST", path: "/api/vscode-codebuddy-plugin/switch" },
  detect_vscode_codebuddy_plugin_account: { method: "POST", path: "/api/vscode-codebuddy-plugin/detect" },
  get_jetbrains_codebuddy_plugin_status: { method: "GET", path: "/api/jetbrains-codebuddy-plugin/status" },
  switch_jetbrains_codebuddy_plugin_account: { method: "POST", path: "/api/jetbrains-codebuddy-plugin/switch" },
  detect_jetbrains_codebuddy_plugin_account: { method: "POST", path: "/api/jetbrains-codebuddy-plugin/detect" },
  delete_account: { method: "POST", path: "/api/delete" },
  set_account_remark: { method: "POST", path: "/api/account/remark" },
  set_account_group: { method: "POST", path: "/api/account/group" },
  // 「今日 Token」按账号聚合（**只读**）：`region` / `force` 走 query ⇒ GET。
  get_account_token_today: { method: "GET", path: "/api/account/token-today" },
  // 网关**逐日**用量趋势（只读落库历史）：`region` 走 query ⇒ GET。
  // ⚠️ `days` 只含落库日 ⇒ 缺日 = 未记录（不是 0），前端按覆盖率披露。
  get_gateway_token_trend: { method: "GET", path: "/api/account/token-trend" },
  // 网关三维度（模型 / 账号 / 端点）逐日历史：账本视图的数据源（issue #63）。
  get_gateway_usage_dim: { method: "GET", path: "/api/account/usage-dim" },
  // 同上，Trae / Qoder 各自的网关账本（`scope` 走 query）。
  get_trae_gateway_usage_dim: { method: "GET", path: "/api/trae/gateway/usage-dim" },
  get_qoder_gateway_usage_dim: { method: "GET", path: "/api/qoder/gateway/usage-dim" },
  // 今日客户端 ↔ 网关对账（只读、不落盘）：`region` 走 query。
  get_usage_reconcile: { method: "GET", path: "/api/account/usage-reconcile" },
  oauth_start: { method: "POST", path: "/api/oauth/start" },
  oauth_status: { method: "POST", path: "/api/oauth/status" },
  import_local: { method: "POST", path: "/api/import-local" },
  export_accounts: { method: "POST", path: "/api/export-accounts" },
  export_accounts_to_path: { method: "POST", path: "/api/export-accounts-to-path" },
  preview_import_accounts: { method: "POST", path: "/api/import/preview" },
  import_accounts: { method: "POST", path: "/api/import" },
  // P7 T01 · 全局备份导出（跨 4 作用域；凭据按 CREDENTIAL_KEYS 剥离）。
  export_backup: { method: "POST", path: "/api/backup/export" },
  // P7 T02 · 全局备份导入（预览 / 导入 / 自动备份 / 恢复 / 配置）。
  preview_import_backup: { method: "POST", path: "/api/backup/import/preview" },
  import_backup: { method: "POST", path: "/api/backup/import" },
  list_auto_backups: { method: "GET", path: "/api/backup/auto/list" },
  restore_auto_backup: { method: "POST", path: "/api/backup/auto/restore" },
  // Q1 · 诊断包导出（只读；凭据闸门在 core，命中即拒绝发出）。
  export_diagnostic: { method: "POST", path: "/api/diagnostics/export" },
  // 运行日志（本工具自身，排查问题用）：读 / 清 / 开目录。
  runtime_logs: { method: "GET", path: "/api/logs/runtime" },
  clear_runtime_logs: { method: "POST", path: "/api/logs/runtime/clear" },
  open_runtime_logs_dir: { method: "POST", path: "/api/logs/open-dir" },
  // issue #80：前端 invoke 计时超阈值时的诊断上报（慢命令进运行日志）。
  report_slow_call: { method: "POST", path: "/api/logs/runtime/slow-call" },
  get_auto_backup_config: { method: "GET", path: "/api/backup/auto/config" },
  save_auto_backup_config: { method: "POST", path: "/api/backup/auto/config" },
  switch_account: { method: "POST", path: "/api/switch" },
  list_sessions: { method: "GET", path: "/api/sessions" },
  // P8 T02 · 只读：copy ledger 连通成「会话副本组」（两宿主同步）。
  list_session_replicas: { method: "GET", path: "/api/sessions/replicas" },
  // P14 S4a · VS Code 插件 / CodeBuddy IDE 的会话仓（**与桌面版不是同一套存储**，
  // 所以单开路由而不是往 `/api/sessions` 塞 `client` 参数）。
  list_vscode_sessions: { method: "GET", path: "/api/vscode-sessions" },
  copy_vscode_sessions: { method: "POST", path: "/api/vscode-sessions/copy" },
  // P14 S4b · 记录级 diff（只读）+ 增量追加（只有 `appendable` 才真写）。
  diff_vscode_sessions: { method: "GET", path: "/api/vscode-sessions/diff" },
  sync_vscode_sessions: { method: "POST", path: "/api/vscode-sessions/sync" },
  // P8 · **安全同步（增量追加）** —— POST：批量传 `{ sourceCid, targetCid }[]`。
  // ★ **写操作**（会往目标 jsonl 追加行）⇒ 不进演示只读命令表。
  sync_session_replicas: { method: "POST", path: "/api/sessions/sync" },
  // P8 · **分歧处理（覆盖式统一）** —— POST：批量传 `{ sourceCid, targetCid }[]`。
  // ★ **破坏性写操作**（目标原有的分歧内容会被丢弃）⇒ 不进演示只读命令表。
  unify_session_replicas: { method: "POST", path: "/api/sessions/unify" },
  copy_sessions: { method: "POST", path: "/api/sessions/copy" },
  // 客户端限流台账（**只读**观测：账号 × 模型 × 恢复时刻）。
  client_limits: { method: "GET", path: "/api/client/limits" },
  // Skill 管理（cc-switch 式）：清单 + 启停 + 跨区搬迁 + 本地导入。
  // ⚠️ 只有 `list_skills` 是**只读**（进演示只读表）；其余 4 条是写 / 动作，不进演示表。
  list_skills: { method: "GET", path: "/api/skills" },
  set_skill_enabled: { method: "POST", path: "/api/skills/enabled" },
  copy_skills_cross: { method: "POST", path: "/api/skills/copy-cross" },
  import_skill: { method: "POST", path: "/api/skills/import" },
  open_skills_dir: { method: "POST", path: "/api/skills/open-dir" },
  // 上游客户端 settings.json 统一托管（读 · 编辑 · 备份 · 恢复 · 差异比对）。
  // ⚠️ 只读的 4 条（slots / read / backups / diff）进演示只读表；patch / restore 是写操作，不进。
  list_client_settings_slots: { method: "GET", path: "/api/client-settings/slots" },
  read_client_settings: { method: "GET", path: "/api/client-settings/read" },
  patch_client_settings: { method: "POST", path: "/api/client-settings/patch" },
  list_client_settings_backups: { method: "GET", path: "/api/client-settings/backups" },
  restore_client_settings_backup: { method: "POST", path: "/api/client-settings/restore" },
  diff_client_settings: { method: "POST", path: "/api/client-settings/diff" },
  migrate_account_data: { method: "POST", path: "/api/migrate/account" },
  get_checkin_status: { method: "GET", path: "/api/checkin/status" },
  get_credit_expiry: { method: "POST", path: "/api/credits" },
  get_credit_statistics: { method: "GET", path: "/api/credits/stats" },
  get_token_statistics: { method: "GET", path: "/api/token-stats" },
  checkin: { method: "POST", path: "/api/checkin" },
  checkin_all: { method: "POST", path: "/api/checkin/all" },
  get_auto_checkin_config: { method: "GET", path: "/api/checkin/config" },
  save_auto_checkin_config: { method: "POST", path: "/api/checkin/config" },
  get_checkin_logs: { method: "GET", path: "/api/checkin/logs" },
  get_travel_status: { method: "GET", path: "/api/travel/status" },
  get_auto_travel_config: { method: "GET", path: "/api/travel/config" },
  save_auto_travel_config: { method: "POST", path: "/api/travel/config" },
  get_switch_config: { method: "GET", path: "/api/switch/config" },
  save_switch_config: { method: "POST", path: "/api/switch/config" },
  get_auto_rotate_config: { method: "GET", path: "/api/rotate/config" },
  save_auto_rotate_config: { method: "POST", path: "/api/rotate/config" },
  get_schedule_config: { method: "GET", path: "/api/schedule/config" },
  save_schedule_config: { method: "POST", path: "/api/schedule/config" },
  run_schedule_task: { method: "POST", path: "/api/schedule/run" },
  rotate_status: { method: "GET", path: "/api/rotate/status" },
  run_rotate: { method: "POST", path: "/api/rotate/run" },
  get_rotate_logs: { method: "GET", path: "/api/rotate/logs" },
  refresh_account_token: { method: "POST", path: "/api/refresh-token" },
  get_github_config: { method: "GET", path: "/api/update/config" },
  save_github_config: { method: "POST", path: "/api/update/config" },
  check_update: { method: "GET", path: "/api/update/check" },
  switch_progress: { method: "GET", path: "/api/switch/progress" },
  open_accounts_dir: { method: "POST", path: "/api/accounts/open-dir" },
  // API 网关（对照架构设计 A-3.7）
  get_gateway_config: { method: "GET", path: "/api/gateway/config" },
  save_gateway_config: { method: "POST", path: "/api/gateway/config" },
  gateway_status: { method: "GET", path: "/api/gateway/status" },
  // P12 · C 路（base URL 重定向）—— **默认关**，应用只记"用户已按引导设置"的本地标记。
  proxy_status: { method: "GET", path: "/api/proxy/status" },
  proxy_enable: { method: "POST", path: "/api/proxy/enable" },
  proxy_disable: { method: "POST", path: "/api/proxy/disable" },
  // 打开 WorkBuddy 客户端数据目录（非 Windows 返回结构化 Unsupported）。
  open_workbuddy_data_dir: { method: "POST", path: "/api/gateway/open-data-dir" },
  list_api_keys: { method: "GET", path: "/api/gateway/keys" },
  create_api_key: { method: "POST", path: "/api/gateway/keys" },
  revoke_api_key: { method: "POST", path: "/api/gateway/keys/revoke" },
  // 撤销吊销（恢复）与吊销互逆：已吊销的 Key **两个出口都必须在**（恢复 / 删除）。
  restore_api_key: { method: "POST", path: "/api/gateway/keys/restore" },
  delete_api_key: { method: "POST", path: "/api/gateway/keys/delete" },
  // ★ issue #76：密钥策略（用量配额 + 可访问账号）。入参是**完整策略**（非补丁）——
  // 两个 radio 必选其一由抽屉保证，后端不猜「只想改一半」。
  set_key_policy: { method: "POST", path: "/api/gateway/keys/policy" },
  // 按密钥的用量快照（与 list 分开：用量每请求写，列表命令不该每次多读一个文件）。
  key_usage: { method: "GET", path: "/api/gateway/keys/usage" },
  // 账号池：人工解除某账号的禁用。禁用是**终态且落盘**，此前没有任何出口
  // （余额刷新被 `disabled` 挡住、`revive` 在生产代码里零调用点）。
  revive_gateway_account: { method: "POST", path: "/api/gateway/pool/revive" },
  // 账号池：**排除 / 取消排除**某账号（硬约束：选号 / 兜底 / 粘性 / 指定全跳过）。
  // 与 revive 的区别见 `pool_admin` 的模块文档：禁用要修，排除只是不用的选择。
  gateway_pool_exclusions: { method: "GET", path: "/api/gateway/pool/exclusions" },
  set_gateway_pool_exclusion: { method: "POST", path: "/api/gateway/pool/exclusions" },
  // 同一份排除表按 scope 分家：Trae 按变体（`trae:trae_work` / `trae:trae_cn` / …），
  // Qoder 按区域（`qoder:cn` / `qoder:global`）。选号侧已读同一份文件，这里只是入口。
  //
  // ★ 只开 **POST**（设置/取消）：列表不需要单独 GET —— 每个账号的 `excluded` 已经
  //   随 `status` 一起下发，再开一个 GET 就是一条没人调用的死路由（契约检查会打红）。
  set_trae_gateway_pool_exclusion: { method: "POST", path: "/api/trae/gateway/pool/exclusions" },
  set_qoder_gateway_pool_exclusion: { method: "POST", path: "/api/qoder/gateway/pool/exclusions" },
  // 统一网关（第三个产品线）：与上面两条网关线路**平行**，但它是**跨作用域**的 ——
  // 因此这些命令**不进** SCOPE_PARAMETERIZED（拿不到也不该拿 scope）。
  get_unified_gateway_config: { method: "GET", path: "/api/unified-gateway/config" },
  get_unified_gateway_models: { method: "GET", path: "/api/unified-gateway/models" },
  // 账号池总览（只读投影）：已启用命名空间 → 三条产品线**已有的**池状态（按池分组）。
  // 形状的唯一构造点在 gateway crate 的 `unified::admin::pool_overview_payload`。
  unified_gateway_pool_overview: { method: "GET", path: "/api/unified-gateway/pool-overview" },
  save_unified_gateway_config: { method: "POST", path: "/api/unified-gateway/config" },
  list_unified_api_keys: { method: "GET", path: "/api/unified-gateway/keys" },
  create_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys" },
  revoke_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys/revoke" },
  // ★ issue #76：统一网关密钥策略（配额 + 账号范围）与按密钥用量。
  set_unified_key_policy: { method: "POST", path: "/api/unified-gateway/keys/policy" },
  unified_key_usage: { method: "GET", path: "/api/unified-gateway/keys/usage" },
  unified_key_usage_all: { method: "GET", path: "/api/unified-gateway/keys/usage-all" },
  restore_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys/restore" },
  delete_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys/delete" },
  get_gateway_models: { method: "GET", path: "/api/gateway/models" },
  refresh_gateway_models: { method: "POST", path: "/api/gateway/models/refresh" },
  get_account_strategy: { method: "GET", path: "/api/gateway/strategy" },
  save_account_strategy: { method: "POST", path: "/api/gateway/strategy" },
  // 模型禁用偏好（P11）：GET 列表 / POST 置位·清位，同一路径两种方法（与 `/api/gateway/strategy` 同构）。
  get_disabled_models: { method: "GET", path: "/api/gateway/disabled-models" },
  set_model_disabled: { method: "POST", path: "/api/gateway/disabled-models" },
  // 模型降级链（P13）：配置独立文件，保存即生效。
  get_model_fallback: { method: "GET", path: "/api/gateway/model-fallback" },
  save_model_fallback: { method: "POST", path: "/api/gateway/model-fallback" },
  preview_model_fallback: { method: "POST", path: "/api/gateway/model-fallback/preview" },
  get_fallback_stats: { method: "GET", path: "/api/gateway/fallback-stats" },
  // 图片能力声明 / 覆盖（issue #101）：与上面两条同构 —— 同一路径两种方法。
  get_model_image_caps: { method: "GET", path: "/api/gateway/model-image-caps" },
  set_model_image_cap: { method: "POST", path: "/api/gateway/model-image-caps" },
  get_gateway_logs: { method: "GET", path: "/api/gateway/logs" },
  clear_gateway_logs: { method: "POST", path: "/api/gateway/logs/clear" },
  // 限流探测（账号 × 模型矩阵 · 一次性作业 + 轮询）：与 api.rs 路由、lib.rs 命令名三方一致。
  probe_start: { method: "POST", path: "/api/gateway/probe/start" },
  probe_status: { method: "GET", path: "/api/gateway/probe/status" },
  probe_cancel: { method: "POST", path: "/api/gateway/probe/cancel" },
  /* 探测**结果台账**（跨重启累积、按作用域隔离）：只读。与 `/status` 的分工见 `probeLedger`。 */
  probe_ledger: { method: "GET", path: "/api/gateway/probe/ledger" },

  // ---- Trae 模块 ----
  // 键名必须与 src-tauri/src/lib.rs 的 invoke_handler 登记名、
  // 以及 crates/buddy-switch-server/src/api.rs 的路由 path+method 三方一致，
  // 由 scripts/check-api-contract.cjs 在构建前校验。
  get_trae_env: { method: "GET", path: "/api/trae/env" },
  get_trae_variants: { method: "GET", path: "/api/trae/variants" },
  get_trae_capabilities: { method: "GET", path: "/api/trae/capabilities" },
  get_trae_accounts: { method: "GET", path: "/api/trae/accounts" },
  get_trae_checkin_status: { method: "GET", path: "/api/trae/checkin/status" },
  // issue #47 · 逐账号「今日」只读重取（与 WB 的同名命令**同构**，前端三平台共用一套渲染）。
  // ⚠️ `accountIds` 是**逗号分隔串**（`ids.join(",")`）而不是数组：GET 语义下
  //    `queryString` 对数组只会产出 `String([...])`，桌面端却天然拿到 `Vec<String>`
  //    ⇒ 两端形状分叉。统一收一个串，两个通道才一致（见后端 `parse_id_list`）。
  get_trae_checkin_status_map: { method: "GET", path: "/api/trae/checkin/status-map" },
  get_trae_account_token_today: { method: "GET", path: "/api/trae/account/token-today" },
  get_trae_credits: { method: "GET", path: "/api/trae/credits" },
  get_trae_token_statistics: { method: "GET", path: "/api/trae/token-stats" },
  // 客户端账本（导入明文库）：探测 / 配置 / 读用量 / 存路径。
  probe_trae_client_db: { method: "GET", path: "/api/trae/client-import/probe" },
  get_trae_client_db_config: { method: "GET", path: "/api/trae/client-import/config" },
  save_trae_client_db_path: { method: "POST", path: "/api/trae/client-import/path" },
  // 内置解密：口令由用户输入，工具内部导出明文副本（`buddy-switch-vault`）。
  unlock_trae_client_db: { method: "POST", path: "/api/trae/client-import/unlock" },
  get_trae_client_token_stats: { method: "GET", path: "/api/trae/client-usage" },
  get_trae_logs: { method: "GET", path: "/api/trae/logs" },
  get_trae_profiles: { method: "GET", path: "/api/trae/profiles" },
  get_trae_settings: { method: "GET", path: "/api/trae/settings" },
  save_trae_settings: { method: "POST", path: "/api/trae/settings" },
  trae_add_account: { method: "POST", path: "/api/trae/accounts/add" },
  trae_update_account: { method: "POST", path: "/api/trae/accounts/update" },
  trae_set_account_remark: { method: "POST", path: "/api/trae/accounts/remark" },
  trae_delete_account: { method: "POST", path: "/api/trae/accounts/delete" },
  // 账号迁移：与 WorkBuddy 的 import_local / export_accounts* / import* 同构。
  // 命令名不与 WorkBuddy 侧重名（多 `trae_` 前缀），因此 ROUTES 里必须逐条列出。
  trae_import_local_account: { method: "POST", path: "/api/trae/accounts/import-local" },
  // Trae OAuth 登录：与 WorkBuddy 的 oauth_start / oauth_status 同构，但**不带 region**
  // （Trae 只有一套账号库与一套上游，见 crates/buddy-switch-core/src/modules/trae/mod.rs）。
  trae_oauth_start: { method: "POST", path: "/api/trae/oauth/start" },
  trae_oauth_status: { method: "POST", path: "/api/trae/oauth/status" },
  // 手工提交回调链接（issue #62）：远程部署时浏览器在另一台机器，回调打不回 127.0.0.1。
  trae_oauth_submit_callback: { method: "POST", path: "/api/trae/oauth/submit-callback" },
  trae_oauth_cancel: { method: "POST", path: "/api/trae/oauth/cancel" },
  trae_export_accounts: { method: "POST", path: "/api/trae/accounts/export" },
  trae_export_accounts_to_path: { method: "POST", path: "/api/trae/accounts/export-to-path" },
  trae_preview_import_accounts: { method: "POST", path: "/api/trae/accounts/import/preview" },
  trae_import_accounts: { method: "POST", path: "/api/trae/accounts/import" },
  trae_group_op: { method: "POST", path: "/api/trae/groups" },
  trae_checkin: { method: "POST", path: "/api/trae/checkin" },
  trae_refresh_credits: { method: "POST", path: "/api/trae/credits/refresh" },
  trae_refresh_jwt: { method: "POST", path: "/api/trae/refresh-jwt" },
  trae_clear_cooldown: { method: "POST", path: "/api/trae/cooldown/clear" },
  trae_merge_legacy_regions: { method: "POST", path: "/api/trae/legacy-merge" },
  trae_switch_account: { method: "POST", path: "/api/trae/switch" },
  trae_save_login: { method: "POST", path: "/api/trae/login/save" },
  trae_backup_profile: { method: "POST", path: "/api/trae/profiles/backup" },
  trae_restore_profile: { method: "POST", path: "/api/trae/profiles/restore" },
  trae_delete_profile: { method: "POST", path: "/api/trae/profiles/delete" },
  trae_reset_device: { method: "POST", path: "/api/trae/device/reset" },
  // Trae API 网关管理面（网关本体走独立端口，见 src/pages/TraeApiServicePage）。
  get_trae_gateway_config: { method: "GET", path: "/api/trae/gateway/config" },
  save_trae_gateway_config: { method: "POST", path: "/api/trae/gateway/config" },
  trae_gateway_status: { method: "GET", path: "/api/trae/gateway/status" },
  get_trae_gateway_models: { method: "GET", path: "/api/trae/gateway/models" },
  // 客户端（上游下发）的模型清单：读客户端 state.vscdb 的缓存，随客户端刷新而变。
  get_trae_client_models: { method: "GET", path: "/api/trae/gateway/client-models" },
  // 多 Key 管理（含归属产品线）：GET 列表 / POST 创建同一路径。
  list_trae_api_keys: { method: "GET", path: "/api/trae/gateway/keys" },
  create_trae_api_key: { method: "POST", path: "/api/trae/gateway/keys" },
  revoke_trae_api_key: { method: "POST", path: "/api/trae/gateway/keys/revoke" },
  // ★ issue #76：Trae 密钥策略（配额 + 账号范围）与按密钥用量（与 WB 侧同形）。
  set_trae_key_policy: { method: "POST", path: "/api/trae/gateway/keys/policy" },
  trae_key_usage: { method: "GET", path: "/api/trae/gateway/keys/usage" },
  // 撤销吊销（恢复）：与 WorkBuddy 侧同形同语义。
  restore_trae_api_key: { method: "POST", path: "/api/trae/gateway/keys/restore" },
  delete_trae_api_key: { method: "POST", path: "/api/trae/gateway/keys/delete" },
  // 打开 Trae 数据目录（非 Windows 返回结构化 Unsupported）。
  open_trae_data_dir: { method: "POST", path: "/api/trae/open-data-dir" },
  // 启动该变体的 Trae 客户端（OAuth 网页登录的前置动作：客户端首次启动才写出设备凭证）。
  trae_launch_client: { method: "POST", path: "/api/trae/launch-client" },
  get_trae_gateway_logs: { method: "GET", path: "/api/trae/gateway/logs" },
  clear_trae_gateway_logs: { method: "POST", path: "/api/trae/gateway/logs/clear" },
  // ---- Qoder 模块（第三个平台）----
  // 键名必须与 src-tauri/src/lib.rs 的 invoke_handler 登记名、
  // 以及 crates/buddy-switch-server/src/api.rs 的路由 path+method 三方一致，
  // 由 scripts/check-api-contract.cjs 在构建前校验。
  //
  // ⚠️ 全部命令都**参数化 `scope`**（`qoder-cn` / `qoder-global`）：Qoder 的双区域是
  // 两本互不相通的账号库，缺 scope 就无从判断该读哪本 —— 故不许有「不带 scope」的重载。
  get_qoder_accounts: { method: "GET", path: "/api/qoder/accounts" },
  get_qoder_local_sources: { method: "GET", path: "/api/qoder/local-sources" },
  qoder_import_local: { method: "POST", path: "/api/qoder/accounts/import-local" },
  qoder_import_pat: { method: "POST", path: "/api/qoder/accounts/import-pat" },
  qoder_set_account_remark: { method: "POST", path: "/api/qoder/accounts/remark" },
  qoder_delete_account: { method: "POST", path: "/api/qoder/accounts/delete" },
  qoder_refresh_account: { method: "POST", path: "/api/qoder/accounts/refresh" },
  qoder_checkin_status: { method: "GET", path: "/api/qoder/checkin/status" },
  // issue #47 · 逐账号「今日签到状态」只读重取（`accountIds` 逗号分隔，见 Trae 侧同一注释）。
  qoder_checkin_status_map: { method: "GET", path: "/api/qoder/checkin/status-map" },
  qoder_checkin_claim: { method: "POST", path: "/api/qoder/checkin/claim" },
  qoder_pro_pack_status: { method: "GET", path: "/api/qoder/pro-pack/status" },
  qoder_pro_pack_claim: { method: "POST", path: "/api/qoder/pro-pack/claim" },
  qoder_credits: { method: "GET", path: "/api/qoder/credits" },
  qoder_oauth_start: { method: "POST", path: "/api/qoder/oauth/start" },
  qoder_oauth_status: { method: "POST", path: "/api/qoder/oauth/status" },
  qoder_program_status: { method: "GET", path: "/api/qoder/programs" },
  qoder_switch_account: { method: "POST", path: "/api/qoder/accounts/switch" },
  qoder_set_account_group: { method: "POST", path: "/api/qoder/accounts/group" },
  qoder_usage: { method: "GET", path: "/api/qoder/usage" },
  // issue #111 · 网页端逐资源包明细。三条都是**桌面端专属**：headless 形态（Docker /
  // npm CLI）没有 `Webview`，server 侧同名路由返回结构化「仅桌面端可用」，不返回空包列表。
  qoder_web_login_open: { method: "POST", path: "/api/qoder/web/login-open" },
  qoder_web_login_close: { method: "POST", path: "/api/qoder/web/login-close" },
  qoder_web_credit_packs_probe: { method: "POST", path: "/api/qoder/web/credit-packs/probe" },
  qoder_web_credit_packs: { method: "GET", path: "/api/qoder/web/credit-packs" },
  qoder_web_session_status: { method: "GET", path: "/api/qoder/web/session/status" },
  qoder_web_window_cookies: { method: "GET", path: "/api/qoder/web/session/window-cookies" },
  qoder_web_session_logout: { method: "POST", path: "/api/qoder/web/session/logout" },
  qoder_web_proxy_get: { method: "GET", path: "/api/qoder/web/proxy" },
  qoder_web_proxy_set: { method: "POST", path: "/api/qoder/web/proxy" },
  // Qoder API 网关管理面（网关本体走独立端口 57895，见 src/pages/QoderApiServicePage）。
  // ⚠️ **一个监听、两个区域**：状态 / 清单两条按 `scope` 看对应的那本账号库（fail-closed）；
  //   配置与监听是否起来是两区域共用的事实（故 config 不带 scope）。
  get_qoder_gateway_config: { method: "GET", path: "/api/qoder/gateway/config" },
  save_qoder_gateway_config: { method: "POST", path: "/api/qoder/gateway/config" },
  qoder_gateway_status: { method: "GET", path: "/api/qoder/gateway/status" },
  get_qoder_gateway_models: { method: "GET", path: "/api/qoder/gateway/models" },
  // 限额护栏视图：逐账号当日 token / 拦截原因 / 余额快照（按区域分家）。
  qoder_gateway_quota: { method: "GET", path: "/api/qoder/gateway/quota" },
  // 多 Key 管理（含归属区域）：GET 列表 / POST 创建同一路径。
  list_qoder_api_keys: { method: "GET", path: "/api/qoder/gateway/keys" },
  create_qoder_api_key: { method: "POST", path: "/api/qoder/gateway/keys" },
  revoke_qoder_api_key: { method: "POST", path: "/api/qoder/gateway/keys/revoke" },
  // ★ issue #76：Qoder 密钥策略（配额 + 账号范围）与按密钥用量。
  set_qoder_key_policy: { method: "POST", path: "/api/qoder/gateway/keys/policy" },
  qoder_key_usage: { method: "GET", path: "/api/qoder/gateway/keys/usage" },
  // 撤销吊销（恢复）：与另两条产品线同形同语义。
  restore_qoder_api_key: { method: "POST", path: "/api/qoder/gateway/keys/restore" },
  delete_qoder_api_key: { method: "POST", path: "/api/qoder/gateway/keys/delete" },
  get_qoder_gateway_logs: { method: "GET", path: "/api/qoder/gateway/logs" },
  clear_qoder_gateway_logs: { method: "POST", path: "/api/qoder/gateway/logs/clear" },
  // ---- 桌面悬浮窗（P9 · 只读 + 显隐 + 聚焦；与 src-tauri commands.rs / server api.rs 一一对应）----
  companion_status: { method: "GET", path: "/api/companion/status" },
  companion_set_visible: { method: "POST", path: "/api/companion/visibility" },
  companion_toggle: { method: "POST", path: "/api/companion/toggle" },
  companion_jump: { method: "POST", path: "/api/companion/jump" },
  // ---- 外部 Agent 会话监听（P15 · hook 安装面；两宿主同源）----
  // ⚠️ `set_agent_hook_enabled` 会**写上游客户端的 settings.json**（装 / 卸 hook），
  // 那是用户显式同意的动作，**不是**悬浮窗的写操作 —— P9 的只读红线仍然成立
  // （`companion_*` 命令面依旧无任何写操作）。
  get_agent_hook_status: { method: "GET", path: "/api/agent-hooks/status" },
  set_agent_hook_enabled: { method: "POST", path: "/api/agent-hooks/enabled" },
};

/**
 * 旧命令名 → 新命令名（**别名表**）—— 命令面参数化（P5）的配套，机器可读。
 *
 * ## 为什么需要它（而不是把旧命令「豁免」掉）
 *
 * 参数化后前端改调新名（如 `accounts_list`），旧 `ROUTES["get_accounts"]` 就没有任何
 * `call()` 引用了 —— 按 `check-api-contract.cjs` 第 3 条会被判**死路由**。但旧命令
 * **必须保留**（别名转发 + 可回滚）。本表把「旧名 → 新名」**显式声明**出来：第 3 条据此
 * 认定「有 `call()` 引用**或**是本表的键」才算活路由，并额外要求**值**（新名）自身是活命令
 * —— 别名链断了仍会报，而不是像「无条件豁免集」那样悄悄腐烂。
 *
 * ## 两条硬约束（写别名转发时遵守）
 *
 * 1. **别名必须硬编码自己的 scope**：旧 `checkin(id)` 恒为 WB 当前 region、旧
 *    `trae_checkin(...)` 恒为 Trae 当前 region；**不接受**「由调用方决定」或「按本机活跃
 *    客户端推断」。
 * 2. 禁止 `platform::detected_variant()` / `detect_data_dir()` 进任何写路径（由门禁第 10 条
 *    扫描 `commands.rs` / `api.rs` 兜住）。
 *
 * ⚠️ **当前为空是刻意的**：T03（命令面参数化）之前没有参数化命令，也就没有别名。
 * 不要为了「让表非空」去提前参数化任何命令。
 */
export const ALIASES: Record<string, string> = {
  // ── P5 T03 · 账号类参数化命令（旧名 → 新名）─────────────────────────────
  // 旧命令保留：桌面端仍在 `generate_handler!` 登记、webui 仍保留旧路由、前端仍可回退。
  // 值（新名）必须自身是活命令（第 3b 条据此防止别名链断裂）。
  get_accounts: "accounts_list",
  get_trae_accounts: "accounts_list",
  set_account_remark: "account_remark",
  trae_set_account_remark: "account_remark",
  delete_account: "account_delete",
  trae_delete_account: "account_delete",
  // ── P5 T03 · 账号导出 / 导入参数化命令（旧名 → 新名）─────────────────────
  // 导出 / 导入是**区域纯**的（只按区域读写账号库，不触碰客户端 / 程序位），故可折叠到
  // `scope`。旧命令保留（桌面端仍登记、webui 仍路由、前端仍可回退）。
  export_accounts: "accounts_export",
  trae_export_accounts: "accounts_export",
  export_accounts_to_path: "accounts_export_to_path",
  trae_export_accounts_to_path: "accounts_export_to_path",
  import_accounts: "accounts_import",
  trae_import_accounts: "accounts_import",
  // ── P5 T03 · 积分参数化命令（旧名 → 新名）─────────────────────────────────
  // 积分是**区域纯**的（只按区域读账号库 / 积分缓存，不触碰客户端 / 程序位），故可折叠到
  // `scope`。旧命令保留（桌面端仍登记、webui 仍路由、前端仍可回退）。
  get_credit_expiry: "credits",
  get_trae_credits: "credits",
  // ── P5 T03 · 签到状态参数化命令（旧名 → 新名）─────────────────────────────
  // 签到状态是**区域纯**的（只按区域读账号库 / 签到缓存，不触碰客户端 / 程序位），故可折叠
  // 到 `scope`。旧命令保留（桌面端仍登记、webui 仍路由、前端仍可回退）。
  // ⚠️ webui 的 `get_checkin_status` 是**批量整端点**（`getCheckinStatusMap` 依赖它），
  // 参数化命令 [`checkin_status`] 只对齐**桌面端单账号**契约 —— 两者语义差异刻意保留。
  get_checkin_status: "checkin_status",
  get_trae_checkin_status: "checkin_status",
  // ── P5 T03 · 从本机客户端导入账号参数化命令（旧名 → 新名）─────────────────
  // **只读**客户端登录态、按 `scope` 写区域账号库 ⇒ 区域纯。名字取 `accounts_import_local`
  // （`import_local` 与 WB 旧命令同名，`generate_handler!` 不允许重名）。
  import_local: "accounts_import_local",
  trae_import_local_account: "accounts_import_local",
  // ── P5 T03 · 打开数据目录参数化命令（旧名 → 新名）─────────────────────────
  // **只读**客户端数据目录路径并交文件管理器打开，**不写**程序位状态 ⇒ 区域纯。
  open_workbuddy_data_dir: "open_data_dir",
  open_trae_data_dir: "open_data_dir",
};

function queryString(args?: Record<string, unknown>): string {
  if (!args) return "";
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(args)) {
    if (value === undefined || value === null) continue;
    params.set(key, String(value));
  }
  const text = params.toString();
  return text ? `?${text}` : "";
}

async function httpCall<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const route = ROUTES[cmd];
  if (!route) throw new Error(t("shared.api.unsupportedInWebui", { cmd }));
  let res: Response;
  try {
    const url =
      route.method === "GET"
        ? `${API_BASE}${route.path}${queryString(args)}`
        : `${API_BASE}${route.path}`;
    res = await fetch(url, {
      method: route.method,
      headers: { "Content-Type": "application/json" },
      body: route.method === "POST" ? JSON.stringify(args ?? {}) : undefined,
    });
  } catch {
    throw new Error(t("shared.api.unreachable", { base: API_BASE }));
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error(data.message || data.error || t("shared.api.requestFailed", { status: res.status }));
  }
  // 数据带上来的错误（`error` / `warning` 这类字段）在**这里**统一本地化，
  // 而不是靠每个渲染点自己记得剥结构尾 —— 见 `error-code.ts` 的说明。
  return localizeCodedStrings(data) as T;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (demoModeEnabled) {
    if (cmd === "get_credit_statistics" && args?.refresh === true) {
      throw new Error(demoUnavailableMessage());
    }
    if (!DEMO_READ_COMMANDS.has(cmd)) throw new Error(demoUnavailableMessage());
    return localizeCodedStrings(screenshotDemoResponse(cmd, args)) as T;
  }
  if (!isWebui()) {
    // ★ issue #80：给每次 IPC 往返计时。桌面端「界面卡死」类报障，唯一能自证的
    // 信息就是「哪个命令耗时长」—— 而运行日志此前只记服务端事件，IPC 耗时零记录，
    // 于是用户按请求带上来的日志总是「很干净」，反而判断不了根因。
    // 超过 `SLOW_CALL_MS` 阈值就地把 `{cmd, ms}` 报进运行日志（fire-and-forget，
    // 绝不 await、绝不影响本次调用结果）。
    const startedAt = Date.now();
    try {
      return localizeCodedStrings(await invoke<T>(cmd, args)) as T;
    } finally {
      const elapsed = Date.now() - startedAt;
      // 排除上报命令自身：否则慢上报会触发新一轮上报（递归）。
      if (cmd !== "report_slow_call" && elapsed >= SLOW_CALL_MS) {
        void reportSlowCall(cmd, elapsed).catch(() => {});
      }
    }
  }
  return httpCall<T>(cmd, args);
}

/** 慢命令上报阈值（ms）：管理面正常命令都远低于此；超了的按「疑似占住主线程」记一条 Warn。 */
const SLOW_CALL_MS = 3000;

/** 上报一次慢命令（best-effort；失败静默 —— 诊断通道永不阻断业务）。 */
export function reportSlowCall(cmd: string, ms: number): Promise<{ ok: boolean }> {
  return call("report_slow_call", { cmd, ms });
}

// ---------------------------------------------------------------------------
// 状态 / 账号
// ---------------------------------------------------------------------------

/** region 作为普通字段放进 args：GET 走 query、POST 走 JSON body（B-8.4）。缺省不传即后端按 cn 处理。 */
function regionArg(region?: Region): Record<string, unknown> {
  return region ? { region } : {};
}

/**
 * 把 `AccountMeta` 的展示字段收敛成 `string | null`（规则见 `lib/display-text.ts`）。
 *
 * 后端已经归一过一遍（Rust `account::display_str`，含回归护栏），这里是**第二道闸**：
 * `AccountMeta` 会流进十几处字符串拼接与 JSX 子节点（`{name}` / `{remark}` /
 * `email.split("@")` / `remark.trim()`），只要有一处漏了脏值就可能让整棵树崩掉。
 * 在这一层收口，比在十几个消费点各防一次可靠 —— 新增消费点自动被覆盖。
 *
 * ⚠️ **时间戳字段刻意不参与归一**：`types.ts` 声明为 `number | null`，
 * `account-card.tsx` 按 `typeof === "number"` 判定过期，字符串化会让过期提示静默消失。
 */
function normalizeAccountMeta(account: AccountMeta): AccountMeta {
  return {
    ...account,
    // `id` 在类型上是非空 `string`；脏值退化成空串，后续按 id 的操作会**响亮失败**
    //（后端回「账号不存在」），而不是把 `[object Object]` 撒进 key 与请求参数。
    id: displayText(account.id) ?? "",
    uid: displayText(account.uid),
    nickname: displayText(account.nickname),
    email: displayText(account.email),
    enterpriseName: displayText(account.enterpriseName),
    needsReloginReason: displayText(account.needsReloginReason),
    remark: displayText(account.remark),
    groupName: displayText(account.groupName),
  };
}

/**
 * 后端 `credits::ENCRYPTED_CREDENTIAL_REASON` 的前端镜像。
 *
 * 改值**必须两边同步**（`crates/buddy-switch-core/src/modules/credits.rs`）。
 * 用它的地方只有「要不要把『用 OAuth 扫码添加』这个出口摆出来」这一件事 ——
 * 判据不能退化成解析 `credit.error` 的中文文案。
 */
export const ENCRYPTED_CREDENTIAL_REASON = "encrypted_credential";

/** 同 {@link normalizeAccountMeta}，作用于 `status.current`（区域 Tab 与徽标 tooltip 都读它）。 */
function normalizeAppStatus(status: AppStatus): AppStatus {
  if (!status?.current) return status;
  return {
    ...status,
    current: {
      uid: displayText(status.current.uid),
      nickname: displayText(status.current.nickname),
      email: displayText(status.current.email),
    },
  };
}

export function getStatus(region?: Region): Promise<AppStatus> {
  return call<AppStatus>("get_status", region ? { region } : undefined).then(normalizeAppStatus);
}

export function getAccounts(region?: Region): Promise<{ accounts: AccountMeta[] }> {
  // P5 T03：改调参数化命令 `accounts_list`（`scope` 决定平台与区域）。旧 `get_accounts`
  // 保留为别名（见 `ALIASES`），桌面端仍登记、webui 仍路由。
  return call<{ accounts: AccountMeta[] }>(
    "accounts_list",
    { scope: scopeOf("workbuddy", region ?? "cn") },
  ).then((result) => ({
    ...result,
    accounts: (result.accounts ?? []).map(normalizeAccountMeta),
  }));
}

export function getCodebuddyCliStatus(): Promise<CodeBuddyCliStatus> {
  return call("get_codebuddy_cli_status");
}

export function installCodebuddyCliHelper(): Promise<CodeBuddyCliInstallResult> {
  return call("install_codebuddy_cli_helper");
}

/**
 * 把 CodeBuddy CLI 切到**本机网关**模式（issue #109 §PR-2）。
 *
 * 一次调用做三件事：建一把 CLI 专用网关 Key、写 `settings.json`（并清掉上游 token 与
 * `apiKeyHelper`）、关掉自动轮换。**互斥**：与「接入 CLI / 切号」不能同时开
 * （两条通道都往 settings 写凭据，混用会让 CLI 把上游 token 发到网关 ⇒ 401）。
 */
/**
 * 从「走本机网关」切回**直连上游**（issue #109）。
 *
 * 会做两件事：清掉 settings 里的三个网关键、按当前账号重建直连凭据。
 * **不会**自动重开自动轮换（切到网关时是被强制关掉的，但自动开启后台任务比自动关闭
 * 更危险 —— 用户可能本来就因别的原因关着）⇒ 看 `rotateDisabled` 决定要不要提示。
 */
export function disableCodebuddyCliGateway(): Promise<{
  ok: boolean;
  changed?: boolean;
  restored?: boolean;
  rotateDisabled?: boolean;
  message?: string;
  error?: string;
}> {
  return call("codebuddy_cli_disable_gateway");
}

export function enableCodebuddyCliGateway(input: {
  region?: string;
  model?: string;
}): Promise<{ ok: boolean; baseUrl?: string; message?: string; error?: string }> {
  return call("codebuddy_cli_enable_gateway", input);
}

export function switchCodebuddyCliAccount(accountId: string): Promise<CodeBuddyCliSwitchResult> {
  if (demoModeEnabled) {
    return new Promise((resolve, reject) => {
      window.setTimeout(() => {
        try {
          resolve(screenshotDemoResponse("switch_codebuddy_cli_account", { accountId }) as CodeBuddyCliSwitchResult);
        } catch (error) {
          reject(error);
        }
      }, 1200);
    });
  }
  return call("switch_codebuddy_cli_account", { accountId });
}

export function getCodebuddyCnIdeStatus(): Promise<CodeBuddyCnIdeStatus> {
  return call("get_codebuddy_cn_ide_status");
}

export function switchCodebuddyCnIdeAccount(
  accountId: string,
  restart = true,
): Promise<CodeBuddyCnIdeSwitchResult> {
  return call("switch_codebuddy_cn_ide_account", { accountId, restart });
}

export function detectCodebuddyCnIdeAccount(): Promise<{
  ok: boolean;
  found: boolean;
  matched?: boolean;
  accountId?: string;
  message?: string;
}> {
  return call("detect_codebuddy_cn_ide_account");
}

// ---------------------------------------------------------------------------
// P0 §3.3 B 表 · 编辑器内 CodeBuddy 插件（VS Code / JetBrains）两个程序位
// ---------------------------------------------------------------------------
//
// 与 `codebuddy_cn_ide` 同构（状态 / 切换 / 探测三条），差别只在**目标**：
// VS Code 写 `state.vscdb` 的 Safe Storage secret，JetBrains 写
// `options/secret-storage.xml`。后端两条命令走**同一个** `programs::switch` 出口，
// 故返回形状一致（[`CodebuddyPluginSwitchResult`]），前端也只定义一份类型。

export function getVscodeCodebuddyPluginStatus(): Promise<VscodeCodebuddyPluginStatus> {
  return call("get_vscode_codebuddy_plugin_status");
}

export function switchVscodeCodebuddyPluginAccount(
  accountId: string,
  restart = true,
): Promise<CodebuddyPluginSwitchResult> {
  return call("switch_vscode_codebuddy_plugin_account", { accountId, restart });
}

export function detectVscodeCodebuddyPluginAccount(): Promise<CodebuddyPluginDetectResult> {
  return call("detect_vscode_codebuddy_plugin_account");
}

export function getJetbrainsCodebuddyPluginStatus(): Promise<JetbrainsCodebuddyPluginStatus> {
  return call("get_jetbrains_codebuddy_plugin_status");
}

export function switchJetbrainsCodebuddyPluginAccount(
  accountId: string,
  restart = true,
): Promise<CodebuddyPluginSwitchResult> {
  return call("switch_jetbrains_codebuddy_plugin_account", { accountId, restart });
}

export function detectJetbrainsCodebuddyPluginAccount(): Promise<CodebuddyPluginDetectResult> {
  return call("detect_jetbrains_codebuddy_plugin_account");
}


export function deleteAccount(accountId: string, region?: Region): Promise<{ ok: boolean }> {
  return call("account_delete", { scope: scopeOf("workbuddy", region ?? "cn"), accountId });
}

// ---------------------------------------------------------------------------
// P5 §2.7 · 回收站 4 命令（**全新命令**，非参数化、无别名）
// ---------------------------------------------------------------------------
//
// 与既有「平台专属 wrapper」不同：回收站对 WB / Trae **同构**（列 / 恢复 / 永久删除 / 清空），
// 且**按 scope 隔离**（`wb-cn` 与 `trae-global` 互不可见）。故这里统一收 `Scope`，由调用方
// （账号页工具栏）传入**当前作用域**。scope 非法时后端 4xx（fail-closed，§2.7 硬约束 1）。

/** 读取某作用域的回收站（按删除时间倒序，最新在前）。 */
export function accountTrashList(scope: Scope): Promise<TrashList> {
  return call<TrashList>("account_trash_list", { scope });
}

/** 从回收站恢复账号（`key` 取自 `TrashEntry.key`）。 */
export function accountRestore(scope: Scope, key: string): Promise<{ ok: boolean }> {
  return call("account_restore", { scope, key });
}

/** 永久删除回收站条目（**唯一不可逆步**，调用方须先走 danger 二次确认 Dialog）。 */
export function accountPurge(scope: Scope, key: string): Promise<{ ok: boolean }> {
  return call("account_purge", { scope, key });
}

/** 清空某作用域回收站（**不可逆**），返回被删除的条数（供二次确认明示条数）。 */
export function accountTrashClear(scope: Scope): Promise<{ ok: boolean; removed: number }> {
  return call("account_trash_clear", { scope });
}

export function oauthStart(region?: Region): Promise<OAuthStartResult> {
  return call("oauth_start", region ? { region } : undefined);
}

export function oauthStatus(loginId: string, region?: Region): Promise<OAuthPollResult> {
  return call<OAuthPollResult>("oauth_status", { loginId, ...regionArg(region) }).then((poll) =>
    poll.result ? { ...poll, result: normalizeAccountMeta(poll.result) } : poll,
  );
}

export function importLocal(region?: Region): Promise<{ ok: boolean; account: AccountMeta }> {
  // P5 T03：改调参数化命令 `accounts_import_local`（`scope` 决定平台与区域）。旧 `import_local`
  // 保留为别名（见 `ALIASES`），桌面端仍登记、webui 仍路由。
  return call<{ ok: boolean; account: AccountMeta }>("accounts_import_local", {
    scope: scopeOf("workbuddy", region ?? "cn"),
  }).then((result) => ({ ...result, account: normalizeAccountMeta(result.account) }));
}

/**
 * 只读扫描数据目录的各类缓存体积（**不清任何东西**）。
 *
 * 与 `src-tauri/src/commands.rs::cache_scan` / `server::api_cache_scan` 一一对应；
 * 无 `scope` 参数 —— `~/.buddy-switch` 是全局单目录。
 */
export function cacheScan(): Promise<CacheScanReport> {
  return call<CacheScanReport>("cache_scan");
}

/**
 * 按类别清除缓存（**不可逆**）。
 *
 * 返回 `{ freedBytes, errors[] }`：**部分失败必须原样带回**，页面据此区分
 * 「全清干净」与「清了但有几项失败」—— 一律报成功就是在骗用户。
 */
export function cacheClear(categories: CacheCategoryId[]): Promise<CacheClearResult> {
  return call<CacheClearResult>("cache_clear", { categories });
}

/**
 * 粘贴授权凭据（issue #62，无客户端部署）。
 *
 * 服务器 / 容器里没有客户端目录，「从本机导入」无从读起；把另一台机器上
 * `auth/workbuddy-desktop-ai.info` 的内容整段粘过来即可入库。
 *
 * 只接 WorkBuddy：另两家各有专用入口（Qoder 粘贴 PAT、Trae 粘贴 JWT）。
 * 后端对这两家返回**明确指引**而不是静默失败。
 */
export function accountsPasteCredential(
  scope: Scope,
  text: string,
): Promise<{ ok: boolean; account: AccountMeta }> {
  return call<{ ok: boolean; account: AccountMeta }>("accounts_paste_credential", {
    scope,
    text,
  }).then((result) => ({ ...result, account: normalizeAccountMeta(result.account) }));
}

export function exportAccounts(
  accountIds: string[],
  scope?: Scope,
): Promise<{ ok: boolean; accounts: AccountRecord[] }> {
  // P5 T03：改调参数化命令 `accounts_export`（`scope` 决定平台与区域）。旧 `export_accounts`
  // 保留为别名（见 `ALIASES`），桌面端仍登记、webui 仍路由。
  //
  // ⚠️ 2026-10-15：参数由 `region` 升为 `scope` —— 三个平台的账号库互不相通，
  // `region` 只能表达 WB/Trae 的区域轴，Qoder 的 `qoder-cn` 与 WB 的 `cn` 会撞车。
  // 缺省仍是 `wb-cn`（旧调用点未传时行为不变）。
  return call("accounts_export", { scope: scope ?? scopeOf("workbuddy", "cn"), accountIds });
}

/** 桌面端：把完整记录写入用户选择的路径（系统保存对话框产物）。 */
export function exportAccountsToPath(
  accountIds: string[],
  path: string,
  scope?: Scope,
): Promise<{ ok: boolean; path: string }> {
  // P5 T03：改调参数化命令 `accounts_export_to_path`。旧 `export_accounts_to_path` 保留为别名。
  // 参数由 `region` 升为 `scope`（理由同 `exportAccounts`）。
  return call("accounts_export_to_path", {
    scope: scope ?? scopeOf("workbuddy", "cn"),
    accountIds,
    path,
  });
}

export function previewImportAccounts(
  fileText: string,
  scope?: Scope,
): Promise<{ accounts: ImportPreviewAccount[]; total: number }> {
  // ⚠️ 2026-10-15：参数由 `region` 升为 `scope`。预览本身是纯函数，但 Qoder 的预览要按
  // **区域**判「归属不符」（跨区条目导入时会被拒）—— 而 `region=cn` 无法区分 `wb-cn` 与
  // `qoder-cn`。不传则走后端既有的「纯解析、不做归属诊断」分支（旧行为不变）。
  return call("preview_import_accounts", {
    fileText,
    scope: scope ?? scopeOf("workbuddy", "cn"),
  });
}

export function importAccounts(fileText: string, indexes: number[], scope?: Scope): Promise<ImportResult> {
  // P5 T03：改调参数化命令 `accounts_import`。旧 `import_accounts` 保留为别名。
  // 参数由 `region` 升为 `scope`（理由同 `exportAccounts`）。
  return call("accounts_import", {
    scope: scope ?? scopeOf("workbuddy", "cn"),
    fileText,
    indexes,
  });
}

/**
 * 导出勾选（三态：scope 头 + 账号 / 配置 / 网关三个分区）。
 *
 * 与后端 `backup::BackupSelection` 同源（camelCase）。`scopes` 缺某作用域即该作用域
 * 默认全选（D14）；`includesCredentials` 默认关（D15）。
 */
export interface BackupSelectionState {
  scopes: Partial<Record<Scope, { accounts: boolean; config: boolean; gateway: boolean }>>;
  includesCredentials: boolean;
  /** `appSettings` 的 localStorage 段（语言 / 外观 / 上次作用域）；缺省即不含。 */
  appSettings?: Record<string, unknown>;
}

/** 备份 envelope（导出产物；字段与后端 `backup.rs` 同源）。 */
export interface BackupEnvelope {
  kind: string;
  formatVersion: number;
  appVersion: string;
  exportedAt: string;
  includesCredentials: boolean;
  scopes: Record<string, Record<string, unknown>>;
  appSettings: Record<string, unknown>;
}

/**
 * ★ P7 T01：导出**全局备份** envelope（跨 4 作用域的账号 / 配置 / 网关）。
 *
 * ## 文案红线（§1.3 / §3.5）
 *
 * **不得**写「备份完整」；必须写「凭据不含在内，导入后需重新登录」——信封剥离后本就
 * 不完整，说「完整」即假承诺。凭据开关默认关（D15）。
 *
 * ## 凭据
 *
 * 后端按 `CREDENTIAL_KEYS` 逐条剥离凭据（备份文件里不得含凭据）；`includesCredentials`
 * 为 `false` 时账号导入后需重新登录。
 */
export function exportBackup(selection: BackupSelectionState): Promise<BackupEnvelope> {
  return call<BackupEnvelope>("export_backup", { selection });
}

/**
 * ★ P7 T02：导入**预览**（**只读安全阀**）—— 逐 scope 计数 + 需重新登录数 + 覆盖配置项。
 *
 * **不写盘**：预览只 dry-run 合并，绝不改动本地数据（§1.7）。`policy` 决定「覆盖计数」与
 * 「将被覆盖的配置项」的口径（`keepLocal` 下不覆盖任何配置项）。
 */
export function previewImportBackup(
  fileText: string,
  policy: BackupConflictPolicy,
): Promise<ImportPreview> {
  return call<ImportPreview>("preview_import_backup", { fileText, policy });
}

/**
 * ★ P7 T02：导入全局备份（**先快照** → 逐 scope 合并 → 汇总）。
 *
 * 默认冲突策略**保留本地**（D16）⇒ 连导两次第二次全「跳过」（幂等）。结果走**部分失败态**
 * （`failed` / `retryableScopes` + 逐 scope 明细），**不许**只提示「导入完成」（§3.5 文案红线）。
 *
 * ★ C1 `onlyScopes`：非空 ⇒ **只导入这些 scope**，即「只重试失败项」——
 * 把上次结果的 `retryableScopes` 原样回传即可，**不需要重新选择文件**。
 * 失败单元就是 scope（后端逐 scope 事务），因此这是最细的诚实粒度。
 */
export function importBackup(
  fileText: string,
  policy: BackupConflictPolicy,
  selection: BackupSelectionState,
  onlyScopes: readonly string[] = [],
): Promise<ImportSummary> {
  return call<ImportSummary>("import_backup", {
    fileText,
    policy,
    selection,
    onlyScopes: [...onlyScopes],
  });
}

/** P7 T02：列出自动备份 / 导入前快照（最新在前）。 */
export function listAutoBackups(): Promise<{ backups: AutoBackupEntry[] }> {
  return call<{ backups: AutoBackupEntry[] }>("list_auto_backups");
}

/**
 * Q1：导出**诊断包**（只读）。
 *
 * 后端在返回前已按 `CREDENTIAL_KEYS` 递归剥离并做**纯文本闸门**（命中即报错、
 * 不返回）⇒ 前端只负责落盘，**不得**在这里再拼内容或绕过该闸门。
 */
export function exportDiagnostic(): Promise<{ name: string; text: string }> {
  return call<{ name: string; text: string }>("export_diagnostic");
}

/**
 * 读取本工具**自身**的运行日志（设置页「日志」分区）。
 *
 * 与 `getTraeLogs` 的区别：那个读的是 **Trae 客户端**的日志；本函数读的是
 * **本工具**写的 `~/.buddy-switch/logs/runtime-*.log`。两者数据源互不相干。
 *
 * 文件不存在时返回空列表（不是错误）——新装用户还没产生任何日志。
 */
export function runtimeLogs(query?: RuntimeLogQuery): Promise<RuntimeLogsResponse> {
  const args: Record<string, unknown> = {};
  if (query?.level) args.level = query.level;
  if (query?.date) args.date = query.date;
  if (query?.keyword) args.keyword = query.keyword;
  if (query?.limit) args.limit = query.limit;
  return call("runtime_logs", Object.keys(args).length > 0 ? args : undefined);
}

/**
 * 清空运行日志（**不可逆**）。
 *
 * 只删本工具自己写的 `runtime-*.log`；同目录其它文件一律不碰（后端按文件名判据筛）。
 * ⚠️ 调用前必须让用户二次确认 —— 这是删数据，不是隐藏。
 */
export function clearRuntimeLogs(): Promise<RuntimeLogClearResult> {
  return call("clear_runtime_logs");
}

/** 在文件管理器中打开日志目录（不存在则先创建）。 */
export function openRuntimeLogsDir(): Promise<{ ok: boolean; path: string }> {
  return call("open_runtime_logs_dir");
}

/** P7 T02：从自动备份 / 导入前快照**恢复**（覆盖策略；恢复本身也可撤销）。 */
export function restoreAutoBackup(fileName: string): Promise<ImportSummary> {
  return call<ImportSummary>("restore_auto_backup", { fileName });
}

/** P7 T02：读取自动备份配置（**默认开**、可在设置里关掉，§7 #7）。 */
export function getAutoBackupConfig(): Promise<AutoBackupConfig> {
  return call<AutoBackupConfig>("get_auto_backup_config");
}

/** P7 T02：保存自动备份配置（关掉后不再产生新备份，已产生的保留）。 */
export function saveAutoBackupConfig(config: AutoBackupConfig): Promise<AutoBackupConfig> {
  return call<AutoBackupConfig>("save_auto_backup_config", { config });
}

export function switchAccount(args: {
  accountId: string;
  region?: Region;
  restart?: boolean;
  copySessionIds?: string[];
  /** 会话复制的**来源**版本；缺省与 `region` 相同（同版本内切换）。 */
  sourceRegion?: Region;
}): Promise<SwitchResult> {
  return call("switch_account", args as unknown as Record<string, unknown>);
}

/**
 * 设置账号备注（**字段级更新**）。
 *
 * 刻意**不**提供「整条账号写回」的口子：前端手上只有脱敏的 `AccountMeta`，
 * 整条写回会把 `access_token` / `refresh_token` 一并抹掉（账号当场失效且无报错）。
 * 传空串即清空备注。
 */
export function setAccountRemark(
  accountId: string,
  remark: string,
  region?: Region,
): Promise<AccountMeta> {
  return call<AccountMeta>("account_remark", {
    scope: scopeOf("workbuddy", region ?? "cn"),
    accountId,
    remark,
  }).then(normalizeAccountMeta);
}

/**
 * 设置账号自定义分组（**字段级更新**）。
 *
 * 与 {@link setAccountRemark} 同源：只改 `groupName` 一个键、绝不整条写回（那会把 token 抹掉）。
 * 传空串即移出分组（后端会删掉账号库里的 `groupName` 键，而不是写一个空串）。
 */
export function setAccountGroup(
  accountId: string,
  group: string,
  region?: Region,
): Promise<AccountMeta> {
  return call<AccountMeta>("set_account_group", {
    accountId,
    group,
    region: region ?? "cn",
  }).then(normalizeAccountMeta);
}

/**
 * 读取「今日 Token」按账号聚合（**只读**观测）。
 *
 * ## 数据来源与诚实边界（UI 渲染红线）
 *
 * 后端把**两路**用量合到一起：① 客户端自身 CLI 落盘的用量；② 经本网关的请求日志。
 * 四条边界（见 {@link AccountTokenTodayResponse}）：
 * - `accounts` 里**缺失**某账号 = 「今日无记录 / 未知」⇒ UI 渲染 **「—」**，**不得**当 0；
 * - 出现在 `accounts` 里的 `0` 是**真实 0**；
 * - `gatewayTruncated: true` ⇒ 网关的**今日**部分可能被裁 ⇒ 网关部分只是**下界**
 *   ⇒ UI 渲染 **`≥N`** 并说明（判据不是「日志到过上限」，见后端注释）；
 * - 两路是**独立记录**，客户端被指向本网关时同一请求会两处各记一次 ⇒ 合计**可能重复
 *   计数**，UI 明细须披露（后端无 join 键、不做去重）。
 *
 * @param force 强制重算（忽略后端当日缓存）；缺省 `false`。
 */
export function getAccountTokenToday(
  region?: Region,
  force = false,
): Promise<AccountTokenTodayResponse> {
  const args: Record<string, unknown> = {};
  if (region) args.region = region;
  if (force) args.force = true;
  return call<AccountTokenTodayResponse>(
    "get_account_token_today",
    Object.keys(args).length > 0 ? args : undefined,
  );
}

/**
 * 网关**逐日**用量趋势（只读 `token_daily` 的 90 天落库历史；不扫盘、不触发记录）。
 *
 * ⚠️ `days` **只含落库过的日子**（App 运行过 + 后台周期记录在跑）⇒ **缺日 = 未记录，
 * 不是 0**。`region: "all"`（合并作用域）时由前端分两区各取一次再求和
 * （与 `mergeSources` 同哲学；后端命令是单 region 的）。
 */
export function getGatewayTokenTrend(region?: Region): Promise<GatewayTokenTrend> {
  const args: Record<string, unknown> = {};
  if (region) args.region = region;
  return call<GatewayTokenTrend>(
    "get_gateway_token_trend",
    Object.keys(args).length > 0 ? args : undefined,
  );
}

/**
 * 网关**三维度**逐日历史（模型 / 账号 / 端点）—— 用量页「账本」视图的数据源。
 *
 * 三条维度由同一次聚合产出、共用同一水位线 ⇒「按模型合计 = 按账号合计 = 按端点合计 = 网关卡」。
 * ⚠️ 只读：不扫盘、不触发记录；`days` 只含落库日（缺日 = 未记录，不是 0）。
 */
export function getGatewayUsageDim(region?: Region): Promise<GatewayUsageDim> {
  const args: Record<string, unknown> = {};
  if (region) args.region = region;
  return call<GatewayUsageDim>(
    "get_gateway_usage_dim",
    Object.keys(args).length > 0 ? args : undefined,
  );
}

/**
 * Trae 的网关**逐日账本**（issue #63 的平台补齐）—— 返回体与 {@link getGatewayUsageDim} 同形。
 *
 * `scope` 是**区域档**（`cn` / `global` / `unlabeled` / `all`），`unlabeled` = 早于
 * `variant` 字段的旧日志（单列一档，不并入任何区域）。
 * ⚠️ `all` 的合并**在后端**做：Trae 有三档，前端逐档取再递归合并容易漏一档。
 * ⚠️ 只读；`days` 只含落库日（缺日 = 未记录，不是 0）。
 */
export function getTraeGatewayUsageDim(scope?: string): Promise<GatewayUsageDim> {
  return call<GatewayUsageDim>(
    "get_trae_gateway_usage_dim",
    scope ? { scope } : undefined,
  );
}

/** Qoder 的网关逐日账本 —— 与 {@link getTraeGatewayUsageDim} 同一形状与口径。 */
export function getQoderGatewayUsageDim(scope?: string): Promise<GatewayUsageDim> {
  return call<GatewayUsageDim>(
    "get_qoder_gateway_usage_dim",
    scope ? { scope } : undefined,
  );
}

/**
 * 今日**客户端 ↔ 网关对账**（只读、不落盘）。
 *
 * 回答「两本账重叠多少」：`matched` 条能在两边对上，`estimate = 客户端 + 网关 − 重叠`。
 * ⚠️ 只覆盖**今日**（网关日志是环形窗口，更早的请求已滚出去）。
 */
export function getUsageReconcile(region?: Region): Promise<GatewayUsageRecon> {
  const args: Record<string, unknown> = {};
  if (region) args.region = region;
  return call<GatewayUsageRecon>(
    "get_usage_reconcile",
    Object.keys(args).length > 0 ? args : undefined,
  );
}

/** 读取账号切换与账号列表展示配置（全局单份）。 */
export function getSwitchConfig(): Promise<SwitchConfig> {
  return call("get_switch_config");
}

export function saveSwitchConfig(config: SwitchConfig): Promise<SwitchConfig> {
  return call("save_switch_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

/** 切换进度（webui 轮询用；桌面端走事件，此函数无副作用）。 */
export function switchProgress(): Promise<{ running: boolean; progress: string | null }> {
  return call("switch_progress");
}

export function listSessions(region?: Region): Promise<{
  sessions: Session[];
  current: string | null;
  /**
   * 会话来源：`db`=正常索引；`scan`=索引库不可读已降级扫描 projects 目录；
   * `empty`=库与 projects 皆空（账号确实没有会话）；
   * `no-dir`=数据目录里连 `workbuddy.db` / `projects/` 都不存在（客户端刚重装 / 重置过），
   * 必须与 `empty` 区分显示，否则会把「数据目录空了」误导成「账号没有会话」。
   */
  source?: "db" | "scan" | "empty" | "no-dir";
  /** 降级 / 异常提示（索引库不可读、扫描结果不完整等）。普通场景为 null。 */
  warning?: string | null;
}> {
  return call("list_sessions", region ? { region } : undefined);
}

/**
 * ★ P8 T02：只读「会话副本组」——把 copy ledger 连通成跨账号副本归属。
 *
 * 数据源是**内部去重账本**被提升为产品语义（见后端 `session::list_replica_groups_for` 的
 * 函数头注释）⇒ **只读当前 region**（P8 ✅ #2）。返回值**只含 ledger 里确有的字段**
 * （`accountUid` / `sessionId` / `region`）；`title` / `updatedAt` / `hasHistory`
 * 由 store 用 {@link listSessions} / 账号 store join 补齐。
 *
 * ⚠️ **不得**用 `updatedAt` 派生「落后 / 分叉」状态（§3.15.2「时间新 ≠ 内容新」）。
 */
export function listSessionReplicas(region?: Region): Promise<SessionReplicaResponse> {
  return call("list_session_replicas", region ? { region } : undefined);
}

/**
 * P8 · **安全同步（增量追加）** —— 把源副本里**目标还没有的尾部行**追加过去。
 *
 * ## 语义（`PLAN.md` §3.15.4）
 *
 * - **同步是加法**：只追加，**绝不删除 / 改写目标已有内容**（用户对「同步」最大的恐惧）；
 * - **只处理「落后」**：目标必须与源**逐字同前缀**；后端**独立复核**，不信任前端判断；
 * - **分歧 fail-closed**：公共前缀 < 较短一方 ⇒ 该条 `ok: false`，**一个字节都不写**；
 * - **方向由调用方给**：`sourceCid` 取**行数更多**的那份（`SessionGroupView.latestLineCount`
 *   对应的行），`targetCid` 取落后的那份。
 *
 * ## 为什么是批量
 *
 * 一次同步多个落后副本是**批量写操作** ⇒ 必须能表达**部分失败**（§3.6）：
 * 逐条返回 `ok` / `error`，调用方据此给「汇总条 + 只重试失败项」的出口。
 *
 * ⚠️ **写操作**：不进演示站的只读命令表（`DEMO_READ_COMMANDS`）。
 */
export function syncSessionReplicas(
  items: SyncSessionReplicaItem[],
  region?: Region,
): Promise<SyncSessionReplicaResponse> {
  return call("sync_session_replicas", { items, ...(region ? { region } : {}) });
}

/**
 * P8 · **分歧处理（覆盖式统一）** —— 把**用户选定保留的那一份**覆盖到其余副本。
 *
 * ## ⚠️ 这是本仓**唯一会丢弃会话内容**的写操作
 *
 * 分歧的定义是「双方在共同旧版之后**各自改过**」⇒ 加法**永远**合不到一起
 * （各自都有对方没有的内容）。`PLAN.md` §3.15.2 因此写死：
 * 「**有分叉时必须先选保留哪一份**才能统一（**不许静默覆盖**）」。
 *
 * ⇒ 调用方**必须**先二次确认，并把 `lostLines`（会丢几条）**如实**告诉用户；
 * 后端**覆盖前先备份**目标原文（`backup` 字段回传路径），备份不成功就**不覆盖**。
 *
 * ⚠️ **写操作**：不进演示站的只读命令表（`DEMO_READ_COMMANDS`）。
 */
export function unifySessionReplicas(
  items: UnifySessionReplicaItem[],
  region?: Region,
): Promise<UnifySessionReplicaResponse> {
  return call("unify_session_replicas", { items, ...(region ? { region } : {}) });
}

/**
 * P14 S4a · **插件侧会话仓**：列出某账号在 VS Code 插件 / CodeBuddy IDE 里的会话（**只读**）。
 *
 * ## 为什么是独立命令而不是给 `listSessions` 加 `client` 参数
 *
 * 桌面版会话是 `workbuddy.db` + **jsonl**，插件侧是 **JSON 记录仓**（目录 + 索引数组）
 * ⇒ 两套数据形状完全不同。挤进一个出口会让每个调用点都长出分支。
 *
 * `uid` **必填**：本仓不猜「插件当前登录的是哪个账号」——猜错就是去读**别人的**会话目录。
 *
 * `dataRoot === null` ⇒ **未检测到客户端**（与「该账号没有会话」是两件事，文案不同）。
 */
export function listExtSessions(client: ExtClientId, uid: string): Promise<ExtSessionsResponse> {
  return call("list_vscode_sessions", { client, uid });
}

/**
 * P14 S4a · 把插件侧会话复制给另一个账号。
 *
 * ## 语义（与桌面版的同步**不是一回事**）
 *
 * - **恒取新 id** ⇒ **加法**，**永不覆盖**目标已有会话；
 * - 消息 / 请求 id 会**递归精确重映射**（副本内部的互相引用跟着换，否则扩展显示空会话）；
 * - 逐条隔离 ⇒ 部分失败由 `errors[]` 如实表达。
 *
 * ⚠️ **写操作**：不进演示站的只读命令表（`DEMO_READ_COMMANDS`）。
 */
export function copyExtSessions(
  client: ExtClientId,
  sourceUid: string,
  targetUid: string,
  items: ExtCopyItem[],
): Promise<ExtCopyResponse> {
  return call("copy_vscode_sessions", { client, sourceUid, targetUid, items });
}

/**
 * P14 S4b · **记录级 diff（只读）**：这份会话能不能做增量追加。
 *
 * ★ 为什么插件侧需要先算 diff：桌面版的会话是 **jsonl**（追加**天然保序**），
 * 插件侧的顺序由 `index.json` 的 `messages[]` **数组次序**决定 ⇒ 目标缺的是**中间**几条时，
 * 追加会把会话读成**错乱的顺序** —— 而且是**静默**的（JSON 依然合法）。
 *
 * ⇒ **只有 `appendable === true` 才允许调 [`syncExtSession`]**；其余状态是**拒绝**，
 * `reason` 里会说清后果（不要自己改写文案，那是产品口径）。
 */
export function diffExtSession(
  client: ExtClientId,
  sourceUid: string,
  targetUid: string,
  workspaceHash: string,
  conversationId: string,
): Promise<ExtDiffResponse> {
  return call("diff_vscode_sessions", {
    client,
    sourceUid,
    targetUid,
    workspaceHash,
    conversationId,
  });
}

/**
 * P14 S4b · **增量追加**：把源尾部多出来的记录补到目标（**只加不改**）。
 *
 * ★★ 后端**不信任调用方**：进来先**自己重算** diff，只有 `appendable` 才真写
 * （调用方说「我算过了」不算数 —— 两件事之间源可能又变了）。
 * 目标已有的消息文件**一个字节都不碰**，索引也只替换 `messages` / `requests` 两条序列。
 *
 * ⚠️ **写操作**：不进演示站的只读命令表（`DEMO_READ_COMMANDS`）。
 */
export function syncExtSession(
  client: ExtClientId,
  sourceUid: string,
  targetUid: string,
  workspaceHash: string,
  conversationId: string,
): Promise<ExtSyncResponse> {
  return call("sync_vscode_sessions", {
    client,
    sourceUid,
    targetUid,
    workspaceHash,
    conversationId,
  });
}

/**
 * 客户端限流台账（**只读**观测）：账号 × 模型 × 恢复时刻。
 *
 * 从客户端自身落盘的三张只读表 join 得出 —— **不依赖网关、不代理客户端流量**：
 * ① `projects` 下递归的 `.jsonl`（限流错误本体）② `workbuddy.db` sessions（model + user_id，**只读**）
 * ③ 账号库（uid → nickname）。详见 core `client_limits.rs`。
 *
 * @param sinceDays `0` = 全量扫描；缺省 = 近 7 天（有界扫描，PRD R10）。
 */
export function clientLimits(
  region?: Region,
  sinceDays?: number,
): Promise<ClientLimitsResponse> {
  const args: Record<string, unknown> = {};
  if (region) args.region = region;
  if (typeof sinceDays === "number") args.sinceDays = sinceDays;
  return call("client_limits", Object.keys(args).length > 0 ? args : undefined);
}

export function copySessions(
  targetAccountId: string,
  sessionIds: string[],
  region?: Region,
  sourceRegion?: Region,
): Promise<{
  sourceUid: string;
  targetUid: string;
  copied: CopyResult[];
  skipped?: CopyResult[];
  errors?: { id: string; error: string }[];
}> {
  return call("copy_sessions", {
    targetAccountId,
    sessionIds,
    ...regionArg(region),
    ...(sourceRegion ? { sourceRegion } : {}),
  });
}

/**
 * 把源账号的 Memory / Connector 合并到目标账号（带去重）。
 *
 * 只处理普通文件，不触碰 `workbuddy.db`，因此无需关闭 WorkBuddy。
 * `sourceAccountId` 缺省时取当前登录账号；`memory` / `connectors` 缺省均为 true。
 */
export function migrateAccountData(
  targetAccountId: string,
  options?: {
    sourceAccountId?: string;
    memory?: boolean;
    connectors?: boolean;
    region?: Region;
    /** 数据来源版本；缺省与 `region` 相同（同版本内迁移）。 */
    sourceRegion?: Region;
  },
): Promise<MigrateResult> {
  const { region, sourceRegion, ...rest } = options ?? {};
  return call("migrate_account_data", {
    targetAccountId,
    ...rest,
    ...regionArg(region),
    ...(sourceRegion ? { sourceRegion } : {}),
  });
}

/** 打开系统设置授权面板（桌面端专用；webui 模式由服务进程权限决定，无操作）。 */
export function openPermissionSettings(
  target?: "app_management" | "all_files",
): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(demoUnavailableMessage()));
  if (isWebui()) return Promise.resolve();
  return call("open_permission_settings", { target: target ?? "app_management" });
}

/** 权限自检：桌面端写探针；webui 模式由服务进程权限决定。 */
export function checkAuthPermission(): Promise<{
  ok: boolean;
  message?: string;
  error?: string;
  dir?: string;
  hint?: string;
}> {
  if (demoModeEnabled) return Promise.reject(new Error(demoUnavailableMessage()));
  if (isWebui()) {
    return Promise.resolve({
      ok: true,
      message: t("shared.api.webuiPermissionByProcess"),
      hint: "",
    });
  }
  return call("check_auth_permission");
}

/** 在 Finder 中显示当前 App（桌面端专用；webui 无操作）。 */
export function revealAppInFinder(): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(demoUnavailableMessage()));
  if (isWebui()) return Promise.resolve();
  return call("reveal_app_in_finder");
}

// ---------------------------------------------------------------------------
// 阶段 3：签到 + token 刷新
// ---------------------------------------------------------------------------

export async function getCheckinStatus(accountId: string, region?: Region): Promise<{
  ok: boolean;
  todayCheckedIn: boolean;
  error?: string;
  raw?: unknown;
}> {
  if (demoModeEnabled) {
    return screenshotDemoResponse("get_checkin_status", { accountId, ...regionArg(region) }) as {
      ok: boolean;
      todayCheckedIn: boolean;
      error?: string;
      raw?: unknown;
    };
  }
  if (isWebui()) {
    // webui 端为批量接口，按 accountId 过滤
    const all = await httpCall<{
      accounts: {
        accountId: string;
        email: string;
        ok: boolean;
        todayCheckedIn: boolean;
        error?: string;
        raw?: unknown;
      }[];
    }>("get_checkin_status", region ? { region } : undefined);
    const one = all.accounts.find((a) => a.accountId === accountId);
    return one
      ? {
          ok: one.ok,
          todayCheckedIn: one.todayCheckedIn,
          error: one.error,
          // ★ B5：webui 走**批量旧路由**，其条目可能尚未带这些字段 ⇒ 从 `raw` 兜底解析
          //   （与桌面端**同一份**解析逻辑，避免两宿主展示不一致）。
          ...checkinCreditsOf(one),
          raw: one.raw,
        }
      : { ok: false, todayCheckedIn: false, error: t("shared.api.accountNotFound") };
  }
  // P5 T03：改调参数化命令 `checkin_status`（`scope` 决定平台与区域）。旧 `get_checkin_status`
  // 保留为别名（见 `ALIASES`）；webui 分支仍走**批量**旧路由（见上方）。
  return call("checkin_status", { scope: scopeOf("workbuddy", region ?? "cn"), accountId });
}

/** 单个账号的签到状态条目。 */
export type CheckinStatusEntry = {
  ok: boolean;
  todayCheckedIn: boolean;
  error?: string;
  /**
   * ★ B5：上游的**积分数据**（此前只透传 `raw`，前端无从展示「逐条 +N」）。
   *
   * ⚠️ 字段名来自**实测响应**（`checkin-activity-status`），不是推测：
   * `daily_credit` / `today_credit` / `total_credits` / `streak_days` / `checkin_dates`。
   */
  dailyCredit?: number | null;
  todayCredit?: number | null;
  totalCredits?: number | null;
  streakDays?: number | null;
  streakBonusCredit?: number | null;
  /** 已签到的日期（倒序，如 `["2026-10-07", …]`）。 */
  checkinDates?: string[] | null;
  /**
   * ★ issue #47：`todayCheckedIn` 的**来源**。
   *
   * - `"upstream"` ⇒ 上游明确回答（可信）；
   * - `"ledger"` ⇒ 上游回成功但没给「今天签没签」⇒ 回落到本地台账（本工具签过的记录）。
   *   台账看不到「用户在 IDE 里手动签」，所以这一档要在 tooltip 里说明来源，
   *   不能与 `"upstream"` 说成同一件事。
   * - 缺省 ⇒ WB 侧（无此概念）。
   */
  source?: "upstream" | "ledger";
  /** ★ Qoder 专有：上游原始状态词（`CLAIMABLE` / `CLAIMED` / `DISABLED`）。 */
  status?: string;
  /** ★ Qoder 专有：活动是否活跃（`false` ⇒ 本区域未开放，前端据此换文案）。 */
  active?: boolean;
  raw?: unknown;
};

/**
 * 从签到条目里取**积分数据**（★ B5）。
 *
 * ⚠️ **两个来源都要认**：
 * ① 后端**显式提取**的字段（桌面端 `checkin_status`，已改）；
 * ② 上游原始 `raw`（webui 的批量旧路由，或后端尚未升级）—— **字段名同源**。
 * ⇒ 两宿主展示一致，且**后端升级前也能用**（不会因为漏改一处就显示空白）。
 */
export function checkinCreditsOf(entry: {
  dailyCredit?: number | null;
  todayCredit?: number | null;
  totalCredits?: number | null;
  streakDays?: number | null;
  streakBonusCredit?: number | null;
  checkinDates?: string[] | null;
  raw?: unknown;
}): Pick<
  CheckinStatusEntry,
  "dailyCredit" | "todayCredit" | "totalCredits" | "streakDays" | "streakBonusCredit" | "checkinDates"
> {
  const raw = (entry.raw ?? {}) as Record<string, unknown>;
  const num = (a: unknown, b: unknown): number | null =>
    typeof a === "number" ? a : typeof b === "number" ? b : null;
  const dates = Array.isArray(entry.checkinDates)
    ? entry.checkinDates
    : Array.isArray(raw.checkin_dates)
      ? (raw.checkin_dates as string[])
      : null;
  return {
    dailyCredit: num(entry.dailyCredit, raw.daily_credit),
    todayCredit: num(entry.todayCredit, raw.today_credit),
    totalCredits: num(entry.totalCredits, raw.total_credits),
    streakDays: num(entry.streakDays, raw.streak_days),
    streakBonusCredit: num(entry.streakBonusCredit, raw.streak_bonus_credit),
    checkinDates: dates,
  };
}

/**
 * **批量**查询多个账号的今日签到状态（`accountId → 条目`；失败/未命中的账号不出现在结果里）。
 *
 * ## 为什么必须有这个批量入口（2026-09-28，B7）
 *
 * 两条通道的**同名接口语义不同**：
 * - **webui** 的 `get_checkin_status` 是**整端点** —— 它把**全部账号**各查一次上游后一起返回；
 * - **桌面端**的同名命令是**单账号**的。
 *
 * 而账号页原本对每个账号调一次 [`getCheckinStatus`] ⇒ 在 webui 下**每次调用都拉全量**，
 * 于是 **N 个账号 = N × N 次上游签到查询**（20 个账号就是 400 次）。
 *
 * 这正是「批量入口必须由 api 层统一提供」的理由：把「webui 一次、桌面端逐个」这条
 * 通道差异**收口在这里**，调用方不必（也无法）自己判断该用哪种方式。
 * 若日后桌面端命令也支持批量，只改本函数即可。
 */
export async function getCheckinStatusMap(
  accountIds: string[],
  region?: Region,
): Promise<Record<string, CheckinStatusEntry>> {
  const map: Record<string, CheckinStatusEntry> = {};
  if (accountIds.length === 0) return map;

  if (demoModeEnabled) {
    // 演示数据是**按账号**的，逐个取（不发网络请求）。
    for (const accountId of accountIds) {
      map[accountId] = await getCheckinStatus(accountId, region);
    }
    return map;
  }

  if (isWebui()) {
    // ★ 一次批量调用（见上方文档）—— 绝不在这里逐个循环。
    const all = await httpCall<{
      accounts: ({ accountId: string } & CheckinStatusEntry)[];
    }>("get_checkin_status", region ? { region } : undefined);
    const wanted = new Set(accountIds);
    for (const item of all.accounts) {
      if (!wanted.has(item.accountId)) continue;
      const { accountId, ...rest } = item;
      map[accountId] = rest;
    }
    return map;
  }

  // 桌面端命令是单账号的，无法再合并 ⇒ 并行发出。
  await Promise.all(
    accountIds.map(async (accountId) => {
      try {
        map[accountId] = await call<CheckinStatusEntry>("checkin_status", {
          scope: scopeOf("workbuddy", region ?? "cn"),
          accountId,
        });
      } catch {
        // 单个失败不拖垮整批：调用方按「未命中」处理（保留原值）。
      }
    }),
  );
  return map;
}

export function getCreditExpiry(accountId: string, region?: Region): Promise<CreditExpiry> {
  // P5 T03：改调参数化命令 `credits`（`scope` 决定平台与区域）。旧 `get_credit_expiry`
  // 保留为别名（见 `ALIASES`），桌面端仍登记、webui 仍路由。
  return call("credits", { scope: scopeOf("workbuddy", region ?? "cn"), accountId });
}

export function getCreditStatistics(refresh = false, region?: RegionFilter): Promise<CreditStatistics> {
  const args: Record<string, unknown> = {};
  if (refresh) args.refresh = true;
  if (region) args.region = region;
  return call("get_credit_statistics", Object.keys(args).length > 0 ? args : undefined);
}

export function getTokenStatistics(days?: number, region?: RegionFilter): Promise<TokenStatistics> {
  const args: Record<string, unknown> = {};
  if (days) args.days = days;
  if (region) args.region = region;
  return call("get_token_statistics", Object.keys(args).length > 0 ? args : undefined);
}

export function checkin(accountId: string, region?: Region): Promise<CheckinResult> {
  return call("checkin", { accountId, ...regionArg(region) });
}

export function checkinAll(region?: Region): Promise<{
  accounts: { accountId: string; email: string; result: string; error?: string }[];
  status?: string;
  reason?: string;
}> {
  return call("checkin_all", region ? { region } : undefined);
}

export function getAutoCheckinConfig(): Promise<CheckinConfig> {
  return call("get_auto_checkin_config");
}

export function saveAutoCheckinConfig(config: CheckinConfig): Promise<CheckinConfig> {
  return call("save_auto_checkin_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function getCheckinLogs(): Promise<{ logs: CheckinLog[] }> {
  return call("get_checkin_logs");
}

export async function getTravelStatus(accountId: string, region?: Region): Promise<TravelStatus> {
  if (demoModeEnabled) {
    return screenshotDemoResponse("get_travel_status", { accountId, ...regionArg(region) }) as TravelStatus;
  }
  if (isWebui()) {
    // webui 端为批量接口，按 accountId 过滤
    const all = await httpCall<{
      accounts: { accountId: string; email: string; label: TravelStatus["label"]; rewardCredit: number | null; locationName?: string | null; arriveAt?: number | null }[];
    }>("get_travel_status", region ? { region } : undefined);
    const one = all.accounts.find((a) => a.accountId === accountId);
    return one
      ? { label: one.label, rewardCredit: one.rewardCredit, locationName: one.locationName ?? null, arriveAt: one.arriveAt ?? null }
      : { label: "untraveled", rewardCredit: null, locationName: null, arriveAt: null };
  }
  return call("get_travel_status", { accountId, ...regionArg(region) });
}

/**
 * **批量**查询多个账号的今日旅行状态（`accountId → 状态`；失败/未命中的不出现在结果里）。
 *
 * 与 [`getCheckinStatusMap`] 同因同治：webui 的 `get_travel_status` 是**整端点**，
 * 逐个调用会变成 N 次全量拉取；桌面端命令是单账号的。通道差异收口在这里。
 */
export async function getTravelStatusMap(
  accountIds: string[],
  region?: Region,
): Promise<Record<string, TravelStatus>> {
  const map: Record<string, TravelStatus> = {};
  if (accountIds.length === 0) return map;

  if (demoModeEnabled) {
    for (const accountId of accountIds) {
      map[accountId] = await getTravelStatus(accountId, region);
    }
    return map;
  }

  if (isWebui()) {
    const all = await httpCall<{
      accounts: {
        accountId: string;
        email: string;
        label: TravelStatus["label"];
        rewardCredit: number | null;
        locationName?: string | null;
        arriveAt?: number | null;
      }[];
    }>("get_travel_status", region ? { region } : undefined);
    const wanted = new Set(accountIds);
    for (const item of all.accounts) {
      if (!wanted.has(item.accountId)) continue;
      map[item.accountId] = {
        label: item.label,
        rewardCredit: item.rewardCredit,
        locationName: item.locationName ?? null,
        arriveAt: item.arriveAt ?? null,
      };
    }
    return map;
  }

  await Promise.all(
    accountIds.map(async (accountId) => {
      try {
        map[accountId] = await call<TravelStatus>("get_travel_status", {
          accountId,
          ...regionArg(region),
        });
      } catch {
        // 单个失败不拖垮整批。
      }
    }),
  );
  return map;
}

export function getAutoTravelConfig(): Promise<TravelConfig> {
  return call("get_auto_travel_config");
}

export function saveAutoTravelConfig(config: TravelConfig): Promise<TravelConfig> {
  return call("save_auto_travel_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function getAutoRotateConfig(): Promise<AutoRotateConfig> {
  return call("get_auto_rotate_config");
}

export function saveAutoRotateConfig(config: AutoRotateConfig): Promise<AutoRotateConfig> {
  return call("save_auto_rotate_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

// ---------------------------------------------------------------------------
// 定时任务排程（六类任务，全局单份，无需 region）
// ---------------------------------------------------------------------------

export function getScheduleConfig(): Promise<ScheduleConfig> {
  return call("get_schedule_config");
}

export function saveScheduleConfig(config: ScheduleConfig): Promise<ScheduleConfig> {
  return call("save_schedule_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

/** 立即执行某一类定时任务（不等排程到点），用于保存排程后当场自证是否生效。 */
export function runScheduleTask(task: string): Promise<ScheduleRunResult> {
  return call("run_schedule_task", { task });
}

export function getRotateStatus(): Promise<RotateStatus> {
  return call("rotate_status");
}

export function runRotate(): Promise<{ status: string; reason?: string; error?: string; to?: string }> {
  return call("run_rotate");
}

export function getRotateLogs(): Promise<{ logs: RotateLog[] }> {
  return call("get_rotate_logs");
}

export function refreshAccountToken(accountId: string, region?: Region): Promise<AccountMeta> {
  return call<AccountMeta>("refresh_account_token", { accountId, ...regionArg(region) }).then(
    normalizeAccountMeta,
  );
}

// ---------------------------------------------------------------------------
// API 网关（对照架构设计 A-3.7）
// ---------------------------------------------------------------------------

export function getGatewayConfig(): Promise<GatewayConfig> {
  return call("get_gateway_config");
}

export async function saveGatewayConfig(config: GatewayConfig): Promise<GatewayConfig> {
  const raw = await call<Record<string, unknown>>("save_gateway_config", {
    config: config as unknown as Record<string, unknown>,
  });
  /* 命令面回的是 `{ ok, config, running, addr }`（桌面与 web 同形）。
     ★ 以前直接把整包喂给 `normalizeGatewayConfig` ⇒ 读不到字段、全部回落默认值，
     表现为「保存后开关又弹回默认」（如 `max_context` 的默认 true）。 */
  const inner = raw?.config;
  if (inner && typeof inner === "object") return inner as GatewayConfig;
  /* 兼容异常形状：万一宿主没带 `config`，以**提交值**为准，绝不回落到默认配置
     （否则会出现「保存成功但开关被重置」这种假故障）。 */
  return (raw && "enabled" in raw ? raw : config) as GatewayConfig;
}

export function gatewayStatus(): Promise<GatewayStatus> {
  return call("gateway_status");
}

/**
 * P12 · C 路（base URL 重定向）的**只读状态**（含引导信息）。
 *
 * ⚠️ `verified` **恒为 `false`** —— 应用**不校验**代理是否真的生效，只反映本地标记。
 * 前端文案必须如实（不许暗示"已验证生效"，见 `P12-ARCH.md` T04 补充 §①）。
 */
export function proxyStatus(): Promise<ProxyStatus> {
  return call("proxy_status");
}

/** 记下「用户已按引导设置」的本地标记（**不改任何系统设置** —— T02 定的是引导式）。 */
export function proxyEnable(): Promise<ProxyStatus> {
  return call("proxy_enable");
}

/** **一键还原**：清标记（删文件，**不留残留**）。 */
export function proxyDisable(): Promise<ProxyStatus> {
  return call("proxy_disable");
}

/**
 * 打开**某作用域**的客户端数据目录（在文件管理器中）—— **参数化命令 `open_data_dir` 的单一入口**。
 *
 * 平台与区域**都由 `scope` 决定** ⇒ 调用方不必再写
 * `product === "workbuddy" ? openWorkbuddyDataDir(…) : openTraeDataDir(…)`
 * 这类平台分支（§1.2 的收敛纪律：平台差异走数据，不走组件分支）。
 *
 * 返回形状：成功 `{ok, path, region}`；平台不支持时是**结构化 `Unsupported`**
 * （`{capability, label, supportedOn, reason}`，**不带 `ok`**）—— 调用方据此区分
 * 「做不到」与「失败」，而不是把两者都当错误弹。
 */
export function openDataDir(scope: Scope): Promise<{
  ok?: boolean;
  path?: string;
  capability?: string;
  label?: string;
  supportedOn?: string;
  reason?: string;
}> {
  return call("open_data_dir", { scope });
}

/**
 * 打开 **WorkBuddy 客户端**的数据目录（在文件管理器中）。
 *
 * ⚠️ 这是 {@link openDataDir} 的**薄壳**（只为既有调用点保留）。新代码请直接传 `scope` ——
 * 多一层「按平台选函数」的分支正是 P2 要消除的形态。
 */
export function openWorkbuddyDataDir(region?: Region): Promise<{
  ok?: boolean;
  path?: string;
  capability?: string;
  label?: string;
  supportedOn?: string;
  reason?: string;
}> {
  // P5 T03：改调参数化命令 `open_data_dir`（`scope` 决定平台与区域）。旧 `open_workbuddy_data_dir`
  // 保留为别名（见 `ALIASES`）。
  return openDataDir(scopeOf("workbuddy", region ?? "cn"));
}

export function listApiKeys(): Promise<{ keys: ApiKeyRecord[] }> {
  return call("list_api_keys");
}

export function createApiKey(name: string, region: Region): Promise<CreateApiKeyResult> {
  return call("create_api_key", { name, region });
}

export function revokeApiKey(id: string): Promise<{ ok: boolean }> {
  return call("revoke_api_key", { id });
}

/* ─────────── 统一网关（第三个产品线 · 跨作用域）───────────
   与 WorkBuddy / Trae 两条线路同形：**读**配置与密钥、**写**配置与密钥。
   字段名与后端 `UnifiedGatewayConfig` 的 snake_case 键一一对应（不做 camelCase 转换）。 */

/** 统一网关配置（`~/.buddy-switch/gateway_unified.json`）。 */
export interface UnifiedGatewayConfigView {
  enabled: boolean;
  bind_addr: string;
  port: number;
  allow_non_loopback: boolean;
  log_keep: number;
  log_bodies: boolean;
  per_key_rate_limit: number | null;
  max_rotate: number;
  sanitize_fingerprints: boolean;
  prompt_mode: string;
  prompt_file: string | null;
  sticky_ttl_ms: number;
  max_body_mb: number;
  /** 启用的命名空间 id（后端按命名空间表顺序返回，与文件里的书写顺序无关）。 */
  namespaces: string[];
  /** 裸模型名落到哪个命名空间；`null` = 拒绝裸名（默认）。 */
  default_namespace: string | null;
}

/** `GET /api/unified-gateway/config` 的响应：配置 + 运行状态 + 命名空间 + 密钥计数。 */
export interface UnifiedGatewaySnapshot {
  config: UnifiedGatewayConfigView;
  /** 已启用的命名空间（表顺序）。 */
  namespaces: string[];
  /** 全部在册的命名空间（含**未启用**的扩展层），供「按平台分组勾选」渲染。 */
  allNamespaces: string[];
  running: boolean;
  addr: string | null;
  keys: { total: number; active: number };
}

/** 统一密钥的**脱敏**视图（不含 hash，明文只在创建时返回一次）。 */
export interface UnifiedApiKeyView {
  id: string;
  name: string;
  prefix: string;
  suffix: string;
  /** 该 Key 被授权的命名空间。 */
  namespaces: string[];
  /** 业务转发（`/biz/*path`）用哪个命名空间；`null` ⇒ 该端点返回 400。 */
  bizNamespace: string | null;
  createdAt: number;
  revokedAt: number | null;
  revoked: boolean;
  lastUsedAt: number | null;
  /* ── 策略（issue #76）—— 与另三条产品线同形同义，前端共用一套构件。
     ⚠️ 本视图另有 `namespaces` / `bizNamespace`（**最外层**授权：能访问哪些平台），
     与这里的「账号范围 + 配额」是两个维度。 ── */
  quotaMode?: "unlimited" | "limited";
  quotaLimit?: number;
  quotaUnit?: "token" | "request";
  quotaPeriod?: "none" | "daily" | "weekly" | "monthly";
  scopeMode?: "all" | "selected";
  scopeAccounts?: string[];
  scopeGroups?: string[];
}

export function getUnifiedGatewayConfig(): Promise<UnifiedGatewaySnapshot> {
  return call("get_unified_gateway_config");
}

/** 一段命名空间的清单来源（**非实时必有原因** —— 契约恒非空）。 */
export interface UnifiedNamespaceCatalog {
  source: string;
  fetched_at: number | null;
  note: string | null;
}

/**
 * `GET /api/unified-gateway/models` 的响应：本入口**已启用**命名空间的清单预览。
 *
 * ★ 与监听端口的 `/v1/models` 不同：**不过滤 API Key 的授权**（管理面看的是「本入口启用了哪些」）。
 * 因此它只在管理面暴露，不能当客户端清单用。
 */
export interface UnifiedModelsPreview {
  object: string;
  data: Array<Record<string, unknown>>;
  namespaces: Record<string, UnifiedNamespaceCatalog>;
}

export function getUnifiedGatewayModels(refresh = false): Promise<UnifiedModelsPreview> {
  // `refresh = true` ⇒ 后端绕过识别缓存重取（WB / Qoder；Trae 直读客户端文件）。
  // 不传时保持 `call` 的零参形态（webui 走裸 GET，不多一个假查询参数）。
  return call("get_unified_gateway_models", refresh ? { refresh: true } : undefined);
}

/**
 * 一条池内账号 —— 三个产品线**共用的池卡形状**（`GatewayPoolAccount`，见 `lib/gateway.ts`）。
 *
 * 字段由各产品线自己的 `status_list` / `accounts_view` 产出，本层**原样透传**：
 * - `credits` 为 `null` = **不可知**（前端渲染「—」；绝不是 0「没额度了」）；
 * - Qoder 的账号行另有 `modelCooldowns`（按模型冷却，与账号级 `cooling` 正交）。
 */
export interface UnifiedPoolAccount {
  uid: string;
  name: string;
  status: string;
  credits: number | null;
  [key: string]: unknown;
}

/**
 * 一个**池**（按 `relay_target_of` 的池标识分组的结果）。
 *
 * ★ 多条命名空间可共用一个池（如 `traework-cn` / `traecode-cn` 复用 `trae-cn` 的区域池）
 * ⇒ 计数与账号行**只出一条**，`namespaces` 列出指向它的已启用段。
 */
export interface UnifiedGatewayPool {
  /** 池标识 —— 作用域字面量（`wb-cn` / `trae-global` / `qoder-cn` …）。 */
  target: string;
  /** 指向该池的**已启用**命名空间（命名空间表顺序）。 */
  namespaces: string[];
  /** 五态计数（WB / Trae / Qoder 各自已有的摘要摊平）。 */
  total: number;
  available: number;
  cooling: number;
  disabled: number;
  expired?: number;
  zero_credits?: number;
  total_credits?: number;
  /** Qoder 独有：此刻有模型级冷却的账号数（与账号级 `cooling` 正交）。 */
  model_cooling?: number;
  /**
   * **状态可用、但在途已满**的账号数（**正交计数，已含在 `available` 里**）。
   *
   * 缺了它就会重演「4 个可用，却一个都跑不起来」：账号是好的，只是并发占满。
   * 目前只有 WorkBuddy 池产出（Trae / Qoder 池无在途治理 ⇒ 缺失按 0）。
   */
  in_flight_full?: number;
  accounts: UnifiedPoolAccount[];
}

/** `GET /api/unified-gateway/pool-overview` 的响应：只列**已启用**命名空间指向的池。 */
export interface UnifiedGatewayPoolOverview {
  pools: UnifiedGatewayPool[];
}

/**
 * 账号池总览（**只读投影**：复用三条产品线已有的池状态，不新增选号逻辑）。
 *
 * 统一网关没有自己的池 —— 数据来自各产品线自己的池视图，因此与独立网关页
 * 「账号池」看到的是**同一份**状态。
 */
export function getUnifiedGatewayPoolOverview(): Promise<UnifiedGatewayPoolOverview> {
  return call("unified_gateway_pool_overview");
}

export function saveUnifiedGatewayConfig(
  config: UnifiedGatewayConfigView,
): Promise<{ ok: boolean; config: UnifiedGatewayConfigView; running: boolean; addr: string | null }> {
  return call("save_unified_gateway_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function listUnifiedApiKeys(): Promise<{ keys: UnifiedApiKeyView[] }> {
  return call("list_unified_api_keys");
}

/**
 * 创建统一密钥。★ **明文只返回这一次**（库里只存 sha256）。
 *
 * `bizNamespace` 传 `null` 表示该 Key 不做业务转发（调用 `/biz/*` 会被明确拒绝）。
 */
export function createUnifiedApiKey(
  name: string,
  namespaces: string[],
  bizNamespace: string | null,
): Promise<CreateApiKeyResult> {
  return call("create_unified_api_key", { name, namespaces, bizNamespace });
}

export function revokeUnifiedApiKey(id: string): Promise<{ ok: boolean }> {
  return call("revoke_unified_api_key", { id });
}

/** ★ issue #76：更新某把统一 Key 的策略（完整策略，非补丁；**不动命名空间授权**）。 */
export function setUnifiedKeyPolicy(id: string, policy: KeyPolicy): Promise<{ ok: boolean }> {
  return call("set_unified_key_policy", { id, policy });
}

/** ★ issue #76：读某把统一 Key 的用量快照。 */
export function unifiedKeyUsage(id: string): Promise<KeyUsageSnapshot> {
  return call("unified_key_usage", { id });
}

/** ★ issue #76：**批量**读统一网关所有 Key 的用量（密钥表每行一列）。 */
export function unifiedKeyUsageAll(): Promise<Record<string, KeyUsageSnapshot>> {
  return call("unified_key_usage_all");
}

/** 撤销吊销 —— **同一把 Key 重新可用**，客户端配置不用改。 */
export function restoreUnifiedApiKey(id: string): Promise<{ ok: boolean }> {
  return call("restore_unified_api_key", { id });
}

/** 物理删除（后端**仅允许删除已吊销**的 Key）。 */
export function deleteUnifiedApiKey(id: string): Promise<{ ok: boolean }> {
  return call("delete_unified_api_key", { id });
}

/** 撤销吊销：清 `revoked_at`，Key **重新可用**（未吊销的 Key 会被后端拒绝）。 */
export function restoreApiKey(id: string): Promise<{ ok: boolean }> {
  return call("restore_api_key", { id });
}

export function deleteApiKey(id: string): Promise<{ ok: boolean }> {
  return call("delete_api_key", { id });
}

/* ─────────── 密钥策略（issue #76）───────────
   类型定义在 `./types`（本文件已单向依赖它，反向 import 会成环）——
   这里只 re-export，历史 import 路径（`@/lib/api`）继续可用。 */
import type { QuotaMode, QuotaUnit, QuotaPeriod, ScopeMode } from "./types";
export type { QuotaMode, QuotaUnit, QuotaPeriod, ScopeMode };

export interface KeyPolicy {
  quotaMode: QuotaMode;
  quotaLimit: number;
  quotaUnit: QuotaUnit;
  quotaPeriod: QuotaPeriod;
  scopeMode: ScopeMode;
  /** 允许访问的账号 uid（`scopeMode === "selected"` 时有效）。 */
  scopeAccounts: string[];
  /** 允许访问的**分组** id（与 `scopeAccounts` 取并集）—— 组内换号时策略不用改。 */
  scopeGroups: string[];
}

/** 某把 Key 的用量快照（`key_usage` 命令的返回）。 */
export interface KeyUsageSnapshot {
  ok: boolean;
  keyId: string;
  unit: QuotaUnit;
  period: QuotaPeriod;
  /** 当前周期键（`daily:2026-10-22` 等）；累计口径为 `null`。 */
  periodKey: string | null;
  /** 当前口径下的已用量（周期切换时自动归零）。 */
  used: number;
  /** 累计用量（不随周期重置）。 */
  total: number;
}

/** 更新某把 Key 的策略（完整策略，非补丁）。矛盾值由后端收敛到安全的一边。 */
export function setKeyPolicy(id: string, policy: KeyPolicy): Promise<{ ok: boolean }> {
  return call("set_key_policy", { id, policy });
}

/** 读某把 Key 的用量快照。 */
export function keyUsage(id: string): Promise<KeyUsageSnapshot> {
  return call("key_usage", { id });
}

/**
 * 人工解除账号池里某账号的禁用（**幂等**：本来可用时 `revived:false`，不报错）。
 *
 * 与「API Key 的恢复」同构 —— 禁用是终态且落盘持久化，不解除就**永久不可用**。
 */
export function reviveGatewayAccount(uid: string): Promise<{ ok: boolean; revived: boolean }> {
  return call("revive_gateway_account", { uid });
}

/** 排除项（`inStore=false` = 该账号已不在账号库里，排除项按设计保留）。 */
export interface GatewayPoolExclusionItem {
  uid: string;
  name: string;
  note: string;
  at: number;
  inStore: boolean;
}

export function getGatewayPoolExclusions(
  region: Region
): Promise<{ scope: string; items: GatewayPoolExclusionItem[] }> {
  return call("gateway_pool_exclusions", { region });
}

/** Trae 侧：设置 / 取消排除（`variant` 缺省 = 默认变体）。 */
export function setTraeGatewayPoolExclusion(
  variant: string | undefined,
  uid: string,
  excluded: boolean,
  note = ""
): Promise<{ scope: string; items: GatewayPoolExclusionItem[] }> {
  return call("set_trae_gateway_pool_exclusion", { variant, uid, excluded, note });
}

/** Qoder 侧：设置 / 取消排除（`scope` 形如 `qoder-cn`）。 */
export function setQoderGatewayPoolExclusion(
  scope: string,
  uid: string,
  excluded: boolean,
  note = ""
): Promise<{ scope: string; items: GatewayPoolExclusionItem[] }> {
  return call("set_qoder_gateway_pool_exclusion", { scope, uid, excluded, note });
}

/** 设置 / 取消排除（幂等）。返回该 region 的最新列表。 */
export function setGatewayPoolExclusion(
  region: Region,
  uid: string,
  excluded: boolean,
  note = ""
): Promise<{ scope: string; items: GatewayPoolExclusionItem[] }> {
  return call("set_gateway_pool_exclusion", { region, uid, excluded, note });
}

export function getGatewayModels(region: Region): Promise<CatalogSnapshot> {
  return call("get_gateway_models", { region });
}

export function refreshGatewayModels(region: Region): Promise<CatalogSnapshot> {
  return call("refresh_gateway_models", { region });
}

export function getAccountStrategy(): Promise<AccountStrategyMap> {
  return call("get_account_strategy");
}

export function saveAccountStrategy(region: Region, strategy: AccountStrategy): Promise<{ ok: boolean }> {
  return call("save_account_strategy", {
    region,
    strategy: strategy as unknown as Record<string, unknown>,
  });
}

/**
 * 读取某**作用域**被用户禁用的模型 id 列表（P11）。
 *
 * 后端读的是**进程内唯一**的禁用存储 —— 与各网关 `/v1/models` 过滤（出口 ①）、
 * 调用拒绝（出口 ②）是同一份数据，因此这里看到的禁用集合就是各出口真正在用的那份。
 *
 * 形状 `{ scope, ids }`；`ids` 为空即「该作用域没有禁用任何模型」（默认全开，D27）。
 * ⚠️ 键是**作用域**（`wb-cn` / `qoder-global` …）—— 三个平台互不相通的模型目录各自分家。
 */
export function getDisabledModels(scope: Scope): Promise<{ scope: Scope; ids: string[] }> {
  return call("get_disabled_models", { scope });
}

/**
 * 置位 / 清位某**作用域**的一个模型（P11）—— 禁用（`disabled=true`）/ 启用（`false`）。
 *
 * 后端幂等：重复置位 / 清位返回 `changed: false` 且不落盘。
 * `scope` 必填且 **fail-closed**（拼错即 400，绝不回落默认作用域）。
 */
export function setModelDisabled(
  scope: Scope,
  modelId: string,
  disabled: boolean,
): Promise<{ ok: boolean; scope: Scope; model_id: string; disabled: boolean; changed: boolean }> {
  return call("set_model_disabled", { scope, modelId, disabled });
}

/* ── 模型降级链（P13）──────────────────────────────────────────────
   `preview` 是**纯计算**：后端只读闸门与禁用表，不写、不领探针票、不发上游请求。
   前端用它做「模拟路径」，让用户不必先保存再等一次真实失败。 */

export function getModelFallback(scope: Scope): Promise<ModelFallbackView> {
  return call("get_model_fallback", { scope });
}

export function saveModelFallback(
  scope: Scope,
  config: ScopeFallbackConfig,
): Promise<ModelFallbackView> {
  return call("save_model_fallback", { scope, config });
}

export function getFallbackStats(scope: Scope, days = 7): Promise<FallbackStats> {
  return call("get_fallback_stats", { scope, days });
}

export function previewModelFallback(
  scope: Scope,
  from: string,
  targets: string[],
): Promise<FallbackPreview> {
  return call("preview_model_fallback", { scope, from, targets });
}

/**
 * 读取某**作用域**的图片能力覆盖（issue #101）。
 *
 * 形状 `{ scope, caps }`：`caps` 只包含**用户手动指定过**的模型（`true` = 支持图片输入）。
 * 缺失的键 = 跟随上游声明（**不是**「不支持」）—— 三态语义见 `lib/model-image.ts`。
 */
export function getModelImageCaps(
  scope: Scope,
): Promise<{ scope: Scope; caps: Record<string, boolean> }> {
  return call("get_model_image_caps", { scope });
}

/**
 * 指定 / 清除某作用域一个模型的图片能力覆盖（issue #101）。
 *
 * `supports`：`true` / `false` = 手动指定；**`null` = 还原上游声明**（后端删键，不是写 `false`）。
 * 后端幂等：重复写同值返回 `changed: false`。
 */
export function setModelImageCap(
  scope: Scope,
  modelId: string,
  supports: boolean | null,
): Promise<{
  ok: boolean;
  scope: Scope;
  model_id: string;
  supports: boolean | null;
  changed: boolean;
}> {
  return call("set_model_image_cap", { scope, modelId, supports });
}

export function getGatewayLogs(): Promise<{ logs: GatewayLogEntry[] }> {
  return call("get_gateway_logs");
}

export function clearGatewayLogs(): Promise<{ ok: boolean }> {
  return call("clear_gateway_logs");
}

/**
 * 启动一轮「账号 × 模型」限流探测（**写**操作 · 异步作业）。
 *
 * 只**建作业**并立刻返回 `{ started, total }`；逐格进度由 `probeStatus(scope)` 轮询。
 * 若已有探测在进行中，后端报错（作业位原子抢占）。
 *
 * ★ **作用域参数化**：入参从 `region` 升级为 `scope`（`wb-cn` / `qoder-cn` …）——
 * 后端据此挑出探测目标（WB / Qoder 的账号库与目录）。未知 / 拼错的 scope 由后端 fail-closed 拒绝。
 */
export function probeStart(params: {
  scope: Scope;
  uids?: string[];
  models?: string[];
  /**
   * **显式格清单**（与 `uids` / `models` **互斥**，给了它就只探点名的格）。
   *
   * 账号页台账卡用它：台账里 3 条（3 账号 / 2 模型）是 **3 格**，而两根轴会展开成 6 格 ——
   * 多打的 3 格既费配额又答非所问。
   */
  cells?: { uid: string; model: string }[];
  adopt: boolean;
}): Promise<{ started: boolean; total: number }> {
  // ⚠️ 展开成**对象字面量**（而不是直接传 `params`）：`check-api-contract.cjs` 第 8 条要求
  // 参数化命令的 `call(...)` 顶层键可静态读到 `scope`（传变量会让护栏无法校验）。
  return call("probe_start", {
    scope: params.scope,
    uids: params.uids,
    models: params.models,
    cells: params.cells,
    adopt: params.adopt,
  });
}

/**
 * 读取探测作业快照（**只读**命令）。
 *
 * ⚠️ 从未跑过时返回 `{ phase: "idle" }`（**没有** `scope` 等其余字段）；
 * 其余字段仅在 `running` / `done` / `cancelled` 时存在 —— 故返回联合类型。
 *
 * `scope` 为命令面统一必填（作业槽是进程级全局，但未知 / 拼错的作用域一律拒绝）。
 */
export function probeStatus(scope: Scope): Promise<{ phase: "idle" } | ModelProbeJob> {
  return call("probe_status", { scope });
}

/**
 * 读取某作用域的**实测台账**（**只读**）：这些格**最近一次**的实判，跨重启保留。
 *
 * 与 {@link probeStatus} 的分工：
 * - `probeStatus` = **本轮作业**（进度 / 相位 / 本次回填），进程内单槽，新作业覆盖；
 * - `probeLedger` = **累积结论**（落盘 `probe_results.json`，按作用域隔离）。
 *
 * ★ 账号页「客户端限流」重启后仍要看到实测结论 ⇒ 必须读它（只靠 store 的 `probeCells`
 * 只能活一个进程）。无记录时 `cells` 为空、`updated_at_ms` 为 `null`（**不是错误**）。
 */
export function probeLedger(scope: Scope): Promise<ModelProbeLedger> {
  return call("probe_ledger", { scope });
}

/** 请求取消当前探测作业（**写**操作；逐格检查，已探到的格保留）。 */
export function probeCancel(scope: Scope): Promise<{ ok: boolean }> {
  return call("probe_cancel", { scope });
}

/** 在系统文件管理器中打开该版本的账号库所在目录（设置页）。 */
export function openAccountsDir(region?: Region): Promise<{ ok: boolean }> {
  return call("open_accounts_dir", region ? { region } : undefined);
}

// ---------------------------------------------------------------------------
// Skill 管理（cc-switch 式）：清单 + 启停 + 跨区搬迁 + 本地导入
// ---------------------------------------------------------------------------

/**
 * 列出该 region 用户级 skills 目录下的 skill 清单（**只读**）。
 *
 * ⚠️ 读的是**运行本应用那台机器**上的 `{data_dir}/skills`。webui 直连远程服务端时，
 * 那是**服务端**的目录（界面据此给出「机器来源」提示）。
 */
export function listSkills(region?: Region): Promise<SkillListResponse> {
  return call("list_skills", region ? { region } : undefined);
}

/**
 * 启停一个 skill —— 行级改写 `SKILL.md` frontmatter 的两个官方原生字段。
 *
 * 幂等：已是目标状态时后端直接返回 `changed: false`（不写盘、不备份）。
 */
export function setSkillEnabled(
  region: Region | undefined,
  dirName: string,
  enabled: boolean,
): Promise<SkillToggleResult> {
  return call("set_skill_enabled", { ...regionArg(region), dirName, enabled });
}

/**
 * 跨 region「合并补齐」搬迁：目标侧缺的补，同名冲突**保留目标侧**（绝不静默覆盖）。
 *
 * 天然幂等：重复执行不产生新内容。两个 region 都必须显式给出。
 */
export function copySkillsCross(
  sourceRegion: Region,
  targetRegion: Region,
): Promise<SkillCopyReport> {
  return call("copy_skills_cross", { sourceRegion, targetRegion });
}

/** 从**本地目录或 zip** 导入一个 skill（同名冲突保留目标侧）。 */
export function importSkill(
  region: Region | undefined,
  sourcePath: string,
): Promise<SkillImportResult> {
  return call("import_skill", { ...regionArg(region), sourcePath });
}

/** 在系统文件管理器中打开该 region 的 skills 目录（不存在时先创建）。 */
export function openSkillsDir(region?: Region): Promise<{ ok: boolean; region: string; dir: string }> {
  return call("open_skills_dir", region ? { region } : undefined);
}

// ---------------------------------------------------------------------------
// 上游客户端 settings.json 统一托管（读 · 编辑 · 备份 · 恢复 · 差异比对）
// ---------------------------------------------------------------------------
//
// ⚠️ 管的是**被本工具代理的上游客户端**（WorkBuddy 两版 + Trae 四个程序位）自己的
// `settings.json`，**不是**本工具自己的 `~/.buddy-switch/**`。槽位路径由后端规格表
// （core `client_settings::CLIENT_SLOTS`）唯一决定，前端**不得**自行拼路径。
//
// ⚠️ 只读的 4 条（slots / read / backups / diff）进演示只读表；patch / restore 是**写操作**，
// 演示模式下由页面侧统一提示不可用（`call` 对非只读命令会直接 reject）。

/** 列出 6 个槽位（未安装者 `installed:false` **仍在列表**）。 */
export function listClientSettingsSlots(): Promise<ClientSettingsSlotsResponse> {
  return call("list_client_settings_slots");
}

/** 读取某槽位的原文 + 解析后键值 + 格式指纹。未安装**不是错误**（`installed:false`）。 */
export function readClientSettings(slot: string): Promise<ClientSettingsReadResponse> {
  return call("read_client_settings", { slot });
}

/**
 * 键值级 patch（**格式保持**：缩进 / 换行 / 未知键 / 键顺序逐字节不变）。
 *
 * @param patch 形如 `{ "<点号路径>": <新值> }`（如 `claw.channels.wechatmp.enabled`）。
 *   幂等：值与现状相同时后端返回 `changed: false`（不写盘、不备份）。
 *   客户端运行中会被后端拒绝（`client.settings.process_running`）。
 */
export function patchClientSettings(
  slot: string,
  patch: Record<string, unknown>,
): Promise<ClientSettingsWriteResult> {
  return call("patch_client_settings", { slot, patch });
}

/** 列出某槽位的历史备份点（按 id 降序）。 */
export function listClientSettingsBackups(slot: string): Promise<ClientSettingsBackupsResponse> {
  return call("list_client_settings_backups", { slot });
}

/** 整文件恢复到某备份点（**恢复前对当前文件再备份一次**）。 */
export function restoreClientSettingsBackup(
  slot: string,
  backupId: string,
): Promise<ClientSettingsWriteResult> {
  return call("restore_client_settings_backup", { slot, backupId });
}

/** 任选两个槽位比对（仅 A 有 / 仅 B 有 / 值不同）。 */
export function diffClientSettings(
  slotA: string,
  slotB: string,
): Promise<ClientSettingsDiffResponse> {
  return call("diff_client_settings", { slotA, slotB });
}

// ---------------------------------------------------------------------------
// 阶段 4：自动更新
// ---------------------------------------------------------------------------

export function getGithubConfig(): Promise<GithubConfig> {
  return call("get_github_config");
}

export function saveGithubConfig(config: GithubConfig): Promise<GithubConfig> {
  return call("save_github_config", {
    config: config as unknown as Record<string, unknown>,
  });
}

export function checkUpdate(proxy?: string, force?: boolean): Promise<UpdateInfo> {
  return call("check_update", { proxy: proxy?.trim() || null, force: force ?? false });
}

/** 重启 App（桌面端专用；webui 无操作）。守卫在 wrapper 内部，保证「webui 不可达」由本函数自证。 */
export function relaunchApp(): Promise<void> {
  if (demoModeEnabled) return Promise.reject(new Error(demoUnavailableMessage()));
  if (isWebui()) return Promise.resolve();
  return call("relaunch_app");
}

// ---------------------------------------------------------------------------
// 应用级偏好（仅桌面端；webui 没有可关闭的窗口，故不提供同名接口，卡片也不在 webui 渲染）
// ---------------------------------------------------------------------------

/**
 * 读取应用级偏好里「宿主也要读」的那一类（目前只有 `close_to_tray`）。
 *
 * 纯渲染层的偏好（主题 / 语言 / 快捷键 / 自动检查更新）**不走这里** ——
 * 它们在 `localStorage`，同步可得，见 `lib/update-preference.ts` 等。
 */
export function getAppPrefs(): Promise<AppPrefs> {
  if (demoModeEnabled) return call("get_app_prefs");
  /* webui 没有宿主进程，`app_prefs.json` 无人维护 ⇒ 这里给「未确认风险」的默认形状，
     风险确认弹层在 webui 下不渲染（见 `legal-consent-dialog.tsx`）。 */
  if (!isDesktop()) return Promise.resolve({ close_to_tray: true, legal_consent_at: 0 });
  return call("get_app_prefs");
}

/** 保存应用级偏好，返回**归一化后的回读值**（非法值回落默认，见后端模块头）。 */
export function setAppPrefs(prefs: AppPrefs): Promise<AppPrefs> {
  /* 演示模式下这条命令**真的能读写**（会话内状态），不是假控件 ——
     它只改本应用自己的偏好，演示站没有理由拒绝它。 */
  if (demoModeEnabled) return call("set_app_prefs", { prefs: prefs as unknown as Record<string, unknown> });
  if (!isDesktop()) return Promise.resolve(prefs);
  return call("set_app_prefs", { prefs: prefs as unknown as Record<string, unknown> });
}

// ---------------------------------------------------------------------------
// 开机自启（仅桌面端；webui 不提供同名接口，卡片也不在 webui 渲染）
// ---------------------------------------------------------------------------

/** 查询系统当前的开机自启注册状态（桌面端）。 */
export function getLaunchAtLoginEnabled(): Promise<boolean> {
  if (demoModeEnabled) return call("get_launch_at_login_enabled");
  if (!isDesktop()) return Promise.resolve(false);
  return call("get_launch_at_login_enabled");
}

/** 注册 / 移除系统开机自启，返回回读后的权威状态（桌面端）。 */
export function setLaunchAtLoginEnabled(enabled: boolean): Promise<boolean> {
  if (demoModeEnabled) return Promise.reject(new Error(demoUnavailableMessage()));
  if (!isDesktop()) return Promise.resolve(false);
  return call("set_launch_at_login_enabled", { enabled });
}

/**
 * 把 Tauri command / HTTP 抛出的错误统一为 Error，并**按当前语言**渲染。
 *
 * 这里是全应用错误文案的唯一裁决点：后端把「文本 + 错误码 + 参数」编进同一个字符串
 * （见 `lib/error-code.ts`），本函数解出码后交给 `localizeError` 选文案。
 * 因此**所有已经用 `asError(e)` 的调用点无需逐个改造**，就同时获得中英两种文案。
 *
 * 中文界面下结果与改造前**逐字节相同**：`localizeError` 对中文直接返回后端原文，
 * 而结构尾已在解码时剥掉。
 */
export function asError(e: unknown): string {
  if (typeof e === "string") return localizeError(e);
  if (e instanceof Error) return localizeError(e.message);
  if (e === null || e === undefined) return t("common.unknownError");
  const serialized = JSON.stringify(e);
  return localizeError(serialized ?? t("common.unknownError"));
}

// ---------------------------------------------------------------------------
// Trae 模块
// ---------------------------------------------------------------------------
//
// 每个 wrapper 都必须以裸 `call(…)` 形式发起调用（字符串字面量为命令名）：
// `scripts/check-api-contract.cjs` 正是据此扫描出调用点，再校验
// 「ROUTES 条目 ←→ Tauri invoke_handler 登记 ←→ server 路由」三方一致。
// 注意不要在注释里写出形如 `call(` 加引号命令名的字样——那会被扫描器当成真实调用点。
// 演示模式（demoModeEnabled）下这些命令不在 DEMO_READ_COMMANDS 中，会统一抛出
// 「演示模式不可用」，页面侧按空态/提示处理即可。

/**
 * 把可选的 `variant` 组装成调用参数（`undefined` 表示「不传」）。
 *
 * 不传与传 `null` 对 Rust 侧**是同一件事**（`Option<String>` 都反序列化成 `None`），
 * 但少传一个键能让请求体更干净、也让「老调用点行为不变」这件事在代码里显式可见。
 * 因此**统一走本函数**，不要在各 wrapper 里重复这段判断。
 */
function variantArgs(variant?: TraeVariantId | null): Record<string, unknown> | undefined {
  if (!variant) return undefined;
  return { variant };
}

/** Trae 客户端安装/运行/数据目录状态（**自动挑中的那一条**，单一视角）。 */
export function getTraeEnv(): Promise<TraeEnvStatus> {
  return call("get_trae_env");
}

/**
 * **全部** Trae 产品线的独立环境状态（并排视角）。
 *
 * 与 [`getTraeEnv`] 的分工：`getTraeEnv` 回答「自动挑中的是哪一条」（用于页面标题、
 * 诊断文案）；本函数回答「每条各自是什么状态」，用于**并排**渲染多个产品图标
 * （对齐 WorkBuddy 右上角三个独立产品图标）。
 *
 * 演示模式下该命令会抛错，调用方按空数组处理即可。
 */
export function getTraeVariants(): Promise<TraeVariantsStatus> {
  return call("get_trae_variants");
}

/** 当前平台的能力与受限项说明。 */
export function getTraeCapabilities(): Promise<TraeCapabilities> {
  return call("get_trae_capabilities");
}

/** 账号 + 分组 + 计数（`variant` 决定读哪个区域库）。
 *
 * P5 T03：改调参数化命令 `accounts_list`；`variant` 先按**区域**折叠成 `scope`
 * （Trae 账号数据是**区域纯**的：区域内两条程序位共用同一本库）。
 */
export function getTraeAccounts(variant?: TraeVariantId | null): Promise<TraeAccountsOverview> {
  /* P5 T03：参数化命令 `accounts_list` 只认 `scope`（平台 + 区域），
     所以要把 Trae 的 `variant`（区域 × 程序位）先**折叠成区域** —— 账号数据是区域纯的。
     ⚠️ `?? "cn"` 与 WorkBuddy 侧同款（那边是 `region ?? "cn"`）：调用方不传时落国内库。
     这不是「猜」，而是**两平台一致的既有默认**；真实调用方（两个 Trae 页面）都会显式传 variant。 */
  return call("accounts_list", { scope: scopeOf("trae", traeRegionOf(variant ?? "cn")) });
}

/** 最近一次签到摘要与冷却明细（`variant` 决定读哪条产品线的数据）。
 *
 * P5 T03：改调参数化命令 `checkin_status`；`variant` 先按**区域**折叠成 `scope`
 * （签到数据是**区域纯**的：区域内两条程序位共用同一份签到缓存）。
 * 旧 `get_trae_checkin_status` 保留为别名（见 `ALIASES`）。
 */
export function getTraeCheckinStatus(variant?: TraeVariantId | null): Promise<TraeCheckinStatus> {
  return call("checkin_status", { scope: scopeOf("trae", traeRegionOf(variant ?? "cn")) });
}

/** 剩余积分、签到明细与每日趋势（`variant` 决定读哪条产品线的数据）。
 *
 * P5 T03：改调参数化命令 `credits`；`variant` 先按**区域**折叠成 `scope`
 * （积分数据是**区域纯**的：区域内两条程序位共用同一份积分缓存）。
 */
export function getTraeCredits(variant?: TraeVariantId | null): Promise<TraeCreditsOverview> {
  return call("credits", { scope: scopeOf("trae", traeRegionOf(variant ?? "cn")) });
}

/**
 * Token 统计（聚合本机 Trae 网关请求日志）。
 *
 * `days` 为统计窗口天数；不传或传 `<= 0` 表示全部历史。
 * `scope` 为**变体范围**筛选维度：`work` / `cn` / `unlabeled` / `all`（不传 = `all`）。
 * 只统计**经过本网关**的调用——直接在 Trae IDE 里对话不产生记录。
 */
export function getTraeTokenStatistics(
  days?: number,
  scope?: TraeTokenScope,
): Promise<TraeTokenStatistics> {
  const args: Record<string, unknown> = {};
  if (days !== undefined) args.days = days;
  if (scope !== undefined) args.scope = scope;
  return call("get_trae_token_statistics", Object.keys(args).length > 0 ? args : undefined);
}

/**
 * 「客户端账本」= 读**用户自己导出的明文库**（Trae 的对话库是 SQLCipher 加密的，
 * 本工具不内置解密 —— 见 `trae/client_import.rs` 的模块头）。
 *
 * 返回体是**判别式**的，前端必须按 `error.code` 分支，不能一律当「空数据」：
 * - `{ configured: false }` —— 还没指定明文库（不是「这段没用量」）
 * - `{ error: { code } }` —— 指定了但读不了（五种原因各自有文案）
 * - `{ source: "trae-client", summary, ... }` —— 正常统计（与网关统计同形）
 */
export interface TraeClientImportError {
  code: "missing" | "not_plaintext" | "unrecognized" | "no_usage" | "io";
  message: string;
}

/** `probe_trae_client_db` 的返回：探测一个明文库能否读（不保存配置）。 */
export interface TraeClientProbe {
  ok: boolean;
  path: string;
  error?: TraeClientImportError;
  /** 库里实际存在的表名 —— 认不出结构时**必须**回显，否则用户无法反馈适配。 */
  tables: string[];
  detected?: {
    messagesTable: string;
    textColumn: string;
    timeColumn: string;
    usageMode: string;
    sessionColumn?: string | null;
    modelColumn?: string | null;
    sampleRows?: number;
    usageHint?: boolean;
    sessionsEstimated?: boolean;
    truncated?: boolean;
  };
  /**
   * 计数。⚠️ **可选**：`Unrecognized` / `NotPlaintext` 分支根本走不到统计，
   * 后端不会给这一格 ⇒ 前端必须判空（写 `counts.sessions` 会在失败时炸）。
   */
  counts?: {
    sessions: number;
    messages: number;
    withUsage: number;
    credits: number;
    /**
     * 一行里含**多份 usage** 的行数。
     *
     * `> 0` ⇒ 这些行按累加计算，若那些 usage 属于同一段对话的历史轮次，统计值会偏大。
     * 后端会在 `note` 里逐字披露；前端也该让用户看得见（不要当成正常数据静默显示）。
     */
    multiUsageRows?: number;
  };
}

/** `get_trae_client_db_config` 的返回。`exists: false` = 路径还在但文件没了。 */
export interface TraeClientConfig {
  path: string | null;
  exists: boolean;
}

/** 内置解密的错误分类（与 `buddy-switch-vault::VaultError` 一一对应）。 */
export interface TraeVaultError {
  code: "missing" | "already_plaintext" | "wrong_key" | "not_sqlite" | "io";
  message: string;
}

/** `unlock_trae_client_db` 的返回。 */
export interface TraeUnlockResult {
  ok: boolean;
  /** `decrypted` = 口令对且已导出明文副本；`plaintext` = 你给的本来就是明文，直接用它。 */
  mode?: "decrypted" | "plaintext";
  /** 生效的 `cipher_compatibility`（4 新库 / 3 老库）。 */
  compatibility?: number;
  tables?: string[];
  /** 最终被设为数据源的路径（`decrypted` 时是固定落点，不是用户选的那个）。 */
  path?: string;
  /** 解锁后顺带跑的探测（与 {@link probeTraeClientDb} 同形）。 */
  probe?: TraeClientProbe;
  error?: TraeVaultError;
}

/**
 * 用口令把**加密的** Trae 对话库导出成明文副本，并设为客户端账本的数据源。
 *
 * ⚠️ 口令必须由你自己提供：它由 Trae 的 native 进程持有、磁盘上没有副本，
 * 本工具不内置任何提取手段（不解密「别人给你的密文」，只解「你自己的库」）。
 *
 * 若传入的本来就是明文库，后端**不会**做无谓的解密与复制，直接返回 `mode: "plaintext"`。
 */
export function unlockTraeClientDb(
  path: string,
  passphrase: string,
): Promise<TraeUnlockResult> {
  return call("unlock_trae_client_db", { path, passphrase });
}

/** 探测一个明文库（「校验」按钮）。 */
export function probeTraeClientDb(path: string): Promise<TraeClientProbe> {
  return call("probe_trae_client_db", { path });
}

/** 已保存的导入路径。 */
export function getTraeClientDbConfig(): Promise<TraeClientConfig> {
  return call("get_trae_client_db_config");
}

/** 保存 / 清除导入路径（`path = null` 即清除）。 */
export function saveTraeClientDbPath(
  path: string | null,
): Promise<{ ok: boolean; path: string | null }> {
  return call("save_trae_client_db_path", { path });
}

/**
 * 客户端账本用量（读导入的明文库）。
 *
 * 与 {@link getTraeTokenStatistics} **同形**（`source` 为 `"trae-client"`），
 * 因此前端两个源可以共用同一套图表适配；差别只在「读的是哪本账」。
 */
export function getTraeClientTokenStats(
  days?: number,
  scope?: TraeTokenScope,
): Promise<TraeTokenStatistics & { configured?: boolean; error?: TraeClientImportError }> {
  const args: Record<string, unknown> = {};
  if (days !== undefined) args.days = days;
  if (scope !== undefined) args.scope = scope;
  return call(
    "get_trae_client_token_stats",
    Object.keys(args).length > 0 ? args : undefined,
  );
}

/**
 * 逐账号「今日 Token」（`get_trae_account_token_today`；issue #47）。
 *
 * ⚠️ **与 WB 同构、但客户端一档不可得**：Trae 没有可解析的客户端会话落盘，所以响应带
 * `clientSource: "unavailable"` —— 客户端那一档是「没测到」，**不是**「没用」（见
 * {@link AccountTokenTodayResponse} 的边界 5）。`accounts` 里缺席的账号同样显示「—」。
 */
export function getTraeAccountTokenToday(
  variant: TraeVariantId | null | undefined,
  force = false,
): Promise<AccountTokenTodayResponse> {
  return call("get_trae_account_token_today", {
    scope: scopeOf("trae", traeRegionOf(variant ?? "cn")),
    force,
  });
}

/**
 * 逐账号「今日签到状态」（`get_trae_checkin_status_map`；issue #47）。
 *
 * 形状与 WB 的 `get_checkin_status_map` 同构（`uid → 条目`）。三条边界：
 * ① 未知 uid **不进** map ⇒ 显示「—」；② `ok:false` ⇒ 显示「读不到」而不是「未签到」；
 * ③ `source: "ledger"` 表示上游没给明确答案、回落到本地台账（tooltip 用得上）。
 *
 * `accountIds` 以**逗号串**下发（见 `ROUTES` 里同名条目的注释）。
 */
export function getTraeCheckinStatusMap(
  variant: TraeVariantId | null | undefined,
  accountIds: string[],
): Promise<Record<string, CheckinStatusEntry>> {
  if (accountIds.length === 0) return Promise.resolve({});
  return call("get_trae_checkin_status_map", {
    scope: scopeOf("trae", traeRegionOf(variant ?? "cn")),
    accountIds: accountIds.join(","),
  });
}

/**
 * 运行日志（系统日志页的「运行日志」标签页）。
 *
 * 只读本机 `logs/` 下的纯文本日志（app / checkin / switcher）；文件不存在时返回空列表，
 * 不抛错——新装用户三个文件都还没有。
 */
export function getTraeLogs(query?: TraeLogQuery): Promise<TraeLogsResponse> {
  const args: Record<string, unknown> = {};
  if (query?.kind && query.kind !== "all") args.kind = query.kind;
  if (query?.date) args.date = query.date;
  if (query?.keyword) args.keyword = query.keyword;
  if (query?.limit) args.limit = query.limit;
  if (query?.variant) args.variant = query.variant;
  return call("get_trae_logs", Object.keys(args).length > 0 ? args : undefined);
}

/** 登录态快照总览（按产品线分家）。 */
export function getTraeProfiles(variant?: TraeVariantId | null): Promise<TraeProfilesOverview> {
  return call("get_trae_profiles", variantArgs(variant));
}

/** Trae 模块设置。 */
export function getTraeSettings(): Promise<TraeSettings> {
  return call("get_trae_settings");
}

/** 局部更新设置：只需提交要改的键。 */
export function saveTraeSettings(patch: Partial<TraeSettings>): Promise<TraeSettings> {
  return call("save_trae_settings", { patch: patch as Record<string, unknown> });
}

/** 手动添加账号（粘贴 JWT）。`variant` 决定写进哪个账号库。 */
export function traeAddAccount(
  name: string,
  jwt: string,
  groupId?: string | null,
  variant?: TraeVariantId | null,
): Promise<{ userId: string; accounts: TraeAccount[] }> {
  return call("trae_add_account", {
    name,
    jwt,
    groupId: groupId ?? null,
    variant: variant ?? null,
  });
}

/** 改名 / 换 JWT。 */
export function traeUpdateAccount(
  userId: string,
  patch: { name?: string; jwt?: string },
  variant?: TraeVariantId | null,
): Promise<{ accounts: TraeAccount[] }> {
  return call("trae_update_account", {
    userId,
    name: patch.name ?? null,
    jwt: patch.jwt ?? null,
    variant: variant ?? null,
  });
}

/**
 * 设置账号备注（**字段级**更新）。
 *
 * 传空串即清空备注（后端会删掉账号库里的 `remark` 键，而不是写一个空串）。
 * 与 {@link traeUpdateAccount} 分开：那条的 `name` 是「空串＝不动」，
 * 而备注必须支持「清空」—— 两种语义挤进同一个参数位迟早静默错一边。
 */
export function traeSetAccountRemark(
  userId: string,
  remark: string,
  variant?: TraeVariantId | null,
): Promise<{ accounts: TraeAccount[] }> {
  return call("trae_set_account_remark", { userId, remark, variant: variant ?? null });
}

/** 删除账号（可选一并删除登录态快照）。 */
export function traeDeleteAccount(
  userId: string,
  deleteProfile = false,
  variant?: TraeVariantId | null,
): Promise<{ deleted: string; profileDeleted: boolean; accounts: TraeAccount[] }> {
  return call("trae_delete_account", { userId, deleteProfile, variant: variant ?? null });
}

/**
 * 从 Trae 客户端登录态导入当前账号（对齐 WorkBuddy 的「导入本机账号」）。
 *
 * 客户端已登录时读 `Cloud-IDE-JWT`：账号已存在则**覆盖刷新 JWT**（保留名字与分组），
 * 不存在则新建。因此重复点击是安全的，不会产生重复条目。
 *
 * `variant` 决定读**哪条产品线**的 userData（`"trae_work"` / `"trae_cn"`）：
 * Trae 多条产品线可同机并存，不指定时后端按默认变体处理（保持向后兼容）。
 * 分区页会把当前管理的产品线传进来，避免「在 Trae Work 分区导入却读了 Trae CN」。
 */
export function traeImportLocalAccount(variant?: TraeVariantId | null): Promise<{
  userId: string;
  name: string;
  accounts: TraeAccount[];
}> {
  // P5 T03：改调参数化命令 `accounts_import_local`；`variant` 先按**区域**折叠成 `scope`。
  // 旧 `trae_import_local_account` 保留为别名（见 `ALIASES`）。
  return call("accounts_import_local", { scope: scopeOf("trae", traeRegionOf(variant ?? "cn")) });
}

// ---------------------------------------------------------------------------
// Trae OAuth 登录（浏览器授权 + 本地回调监听）
// ---------------------------------------------------------------------------
//
// 与 WorkBuddy 的 `oauthStart` / `oauthStatus` 同构，但**不带 region 参数**：
// Trae 只有一套账号库与一套上游，加 region 会是永远被忽略的假参数。

/**
 * 发起登录：后端在本机 `127.0.0.1` 起临时回调监听并返回授权 URL。
 *
 * **本函数不会打开浏览器**——调用方拿到 `verificationUri` 后自行打开。
 * 分开的理由：webui 场景没有系统浏览器可开，只能把链接展示给用户点。
 *
 * ## `variant` 是**必填**（编译期护栏）
 *
 * 两条产品线各有自己的客户端、数据目录与账号库，授权页也因此不同。
 * 漏传 = 后端按默认变体（`Trae Work`）处理，于是「在 Trae CN 页面点登录」
 * 实际会发起 Trae Work 的登录，账号还会落到 Trae Work 的账号库 ——
 * 用户看到的是「登录成功了但 Trae CN 的列表还是空的」。
 *
 * 这里**必填**是有意的：漏传从「运行时静默走错产品线」变成「编译不过」。
 * 变体的唯一来源是 {@link useTraeVariant}（URL `?line=` 承载），它**永远**返回
 * 一个变体，所以必填不会让任何调用点写不出来。
 *
 * ## 为什么只在这一层必填
 *
 * Tauri 命令（`src-tauri/src/commands.rs`）与 server 路由仍收 `Option<String>`：
 * 那是**跨进程线协议**，老版本客户端可能不传，收紧会破坏兼容。
 * **「编译期护栏」与「wire 兼容」是两个层次，不要一起改。**
 */
export function traeOAuthStart(variant: TraeVariantId): Promise<TraeOAuthStartResult> {
  return call("trae_oauth_start", variantArgs(variant));
}

/**
 * 轮询登录结果。
 *
 * **永不抛错**：`{ done: false }` 表示「还没好」，前端据此继续轮询。
 * 终态时后端会把会话摘掉，再轮询会得到 `done: true` + `error: 不存在或已过期`。
 */
/**
 * 手工提交 Trae 授权回调链接（issue #62，远程部署）。
 *
 * 授权页把回调打到固定 `127.0.0.1:17388`；容器部署时用户的浏览器在另一台机器上，
 * 那条回调回不到服务端。用户把地址栏整串粘进来，后端用**与监听路径同一套**逻辑
 * （解析 / CSRF / 兑换 / 落库）补完。
 *
 * ★ 与轮询不同，本函数**会抛错**：用户此刻正盯着这个动作，
 * 静默等待（`{done:false}`）只会被当成「卡住」。
 */
export function traeOAuthSubmitCallback(
  loginId: string,
  callbackUrl: string,
): Promise<TraeAccount> {
  return call<TraeAccount>("trae_oauth_submit_callback", { loginId, callbackUrl });
}

export function traeOAuthStatus(loginId: string): Promise<TraeOAuthPollResult> {
  return call("trae_oauth_status", { loginId });
}

/** 取消登录（用户关掉对话框），后端随即释放监听端口。 */
export function traeOAuthCancel(loginId: string): Promise<{ cancelled: boolean }> {
  return call("trae_oauth_cancel", { loginId });
}

/** 导出勾选账号的完整记录（含 JWT），由前端落盘。 */
export function traeExportAccounts(
  userIds: string[],
  variant?: TraeVariantId | null,
): Promise<{ accounts: TraeExportRecord[] }> {
  // P5 T03：改调参数化命令 `accounts_export`；`variant` 先按**区域**折叠成 `scope`
  // （导出只按区域读账号库，不触碰客户端 / 程序位）。旧 `trae_export_accounts` 保留为别名。
  return call("accounts_export", {
    scope: scopeOf("trae", traeRegionOf(variant ?? "cn")),
    accountIds: userIds,
  });
}

/** 桌面端：把完整记录写入用户选择的路径（系统保存对话框产物）。 */
export function traeExportAccountsToPath(
  userIds: string[],
  path: string,
  variant?: TraeVariantId | null,
): Promise<{ path: string; count: number }> {
  // P5 T03：改调参数化命令 `accounts_export_to_path`（同上，按区域折叠）。
  return call("accounts_export_to_path", {
    scope: scopeOf("trae", traeRegionOf(variant ?? "cn")),
    accountIds: userIds,
    path,
  });
}

/** 解析导入文件并回传脱敏预览（不含 JWT 明文，只报有无）。 */
export function traePreviewImportAccounts(fileText: string): Promise<TraeImportPreview> {
  return call("trae_preview_import_accounts", { fileText });
}

/** 按选中索引导入账号，返回计数与最新账号列表。 */
export function traeImportAccounts(
  fileText: string,
  indexes: number[],
  variant?: TraeVariantId | null,
): Promise<{ imported: number; skipped: number; overwritten: number; accounts: TraeAccount[] }> {
  // P5 T03：改调参数化命令 `accounts_import`（按区域折叠 `variant`）。旧命令保留为别名。
  return call("accounts_import", {
    scope: scopeOf("trae", traeRegionOf(variant ?? "cn")),
    fileText,
    indexes,
  });
}

/** 分组操作：`create` / `update` / `delete` / `move`。 */
export function traeGroupOp(
  action: "create" | "update" | "delete" | "move",
  params: Record<string, unknown>,
  variant?: TraeVariantId | null,
): Promise<{ id?: string; groups?: TraeGroup[]; accounts?: TraeAccount[] }> {
  return call("trae_group_op", { action, params, variant: variant ?? null });
}

/**
 * 批量签到。
 *
 * 桌面端会同时派发 `trae-checkin-progress` 事件（逐账号进度）；webui 只在结束时
 * 拿到完整报告。两端的**返回值形状相同**。
 *
 * `variant` 包在 `options` 里（而不是像其他 wrapper 那样放顶层）：
 * 签到的入参本就整体是一个 options 对象，后端也按这个契约解析，
 * 再单独加一个顶层字段会让两条通道的契约分叉。
 */
export function traeCheckin(options?: {
  scope?: "all" | "selected" | `group:${string}`;
  userIds?: string[];
  skipCheckedIn?: boolean;
  skipExpired?: boolean;
  retry?: number;
  variant?: TraeVariantId | null;
}): Promise<TraeCheckinReport> {
  return call("trae_checkin", { options: options ?? {} });
}

/** 刷新剩余积分（不传 userId 即刷新该产品线全部）。 */
export function traeRefreshCredits(
  userId?: string,
  variant?: TraeVariantId | null,
): Promise<{ scope: "single" | "all"; userId?: string; credits?: number; refreshed: number }> {
  return call("trae_refresh_credits", { userId: userId ?? null, variant: variant ?? null });
}

/** 用 refresh_token 换新 JWT。 */
export function traeRefreshJwt(
  userId: string,
  variant?: TraeVariantId | null,
): Promise<{ userId: string; jwt: string; jwtExpHours: number | null; accounts: TraeAccount[] }> {
  return call("trae_refresh_jwt", { userId, variant: variant ?? null });
}

/** 清除冷却（不传 userId 即清除该产品线全部）。 */
export function traeClearCooldown(
  userId?: string,
  variant?: TraeVariantId | null,
): Promise<{ scope: "single" | "all"; userId?: string; cleared?: number }> {
  return call("trae_clear_cooldown", { userId: userId ?? null, variant: variant ?? null });
}

/**
 * 把旧「产品线」账号库并入**国内版**区域账号库（幂等）。
 *
 * 无参数：输入是磁盘上的旧库文件，输出是合并报告。**无旧库或已并完时 `changed`
 * 为 false**，因此页面加载时调一次是安全且廉价的。合并前后端会先把目标库
 * 备份到 `trae/backups/region-merge/<utc>/`，路径随 `backup` 回传。
 */
export function traeMergeLegacyRegions(): Promise<TraeLegacyMergeReport> {
  return call("trae_merge_legacy_regions");
}

/**
 * 切换账号（含「先保存当前登录态」的兜底）。
 *
 * `variant` 决定读/写哪条产品线的快照与客户端目录。它是**数据维度**，
 * 故与其他可选参数同层放在 `options` 里，由后端 `parse_switch_options` 一并解析。
 *
 * ⚠️ **整包必须嵌在 `options` 键下**：桌面端命令签名是
 * `trae_switch_account(app, options: Value)`（单个 `Value` 参数），Tauri 按参数名
 * 取值，平铺传参会直接报 `missing required key options` —— 只有 webui 通道
 * （`api_trae_switch` 里 `body.get("options").unwrap_or(body)`）能容忍平铺。
 * 与 `traeCheckin` 同形；`scripts/check-api-contract.cjs` 的「单 Value 参数」规则
 * 会守住这条不变式。
 *
 * ## ★ 失败必须**抛错**，不能把 `success:false` 当正常返回值交出去
 *
 * 后端 `switch_account` 的失败是**正常返回**一份带 `steps` 的报告（`success:false`），
 * 不是传输错误。于是「Promise resolve 了」与「切换成功了」被混为一谈：页面在 resolve
 * 之后无条件弹「{label}完成」，用户在绿勾里看到的是**一句谎话**。
 *
 * 真机现场（2026-09-29 报障「切换成功但程序没打开、标记也没变」）：TraeCode 的目标
 * 账号没有登录态快照，后端在预检查就 return，`success:false` +「目标快照不存在」，
 * 而界面弹的是「切换TraeCode账号完成」—— 客户端当然没启动，当前账号标记当然没变。
 *
 * 所以在此收口，对齐 WorkBuddy 侧 `switch_account`（走 `Result`：失败即 `Err`，
 * 前端 `catch` 报错、`toast.success` 只在成功路径上）。**本函数是 Trae 侧唯一入口**，
 * 在这里判一次，未来新增调用点自动被覆盖，不必指望每个页面都记得看 `success`。
 */
export async function traeSwitchAccount(options: {
  userId: string;
  launch?: boolean;
  proxyPort?: number | null;
  resetDevice?: boolean;
  variant?: TraeVariantId | null;
}): Promise<TraeSwitchOutcome> {
  const outcome = await call<TraeSwitchOutcome>("trae_switch_account", {
    options: {
      userId: options.userId,
      launch: options.launch ?? true,
      proxyPort: options.proxyPort ?? null,
      resetDevice: options.resetDevice ?? false,
      variant: options.variant ?? null,
    },
  });
  if (!outcome?.success) {
    // 优先用后端给的 `error`（预检查失败时它已是可操作的一整句），
    // 退一步取最后一步的说明，两者都缺才用通用兜底。
    const last = outcome?.steps?.[outcome.steps.length - 1];
    throw new Error(
      outcome?.error ?? last?.message ?? t("trae.page.accounts.switchIncomplete"),
    );
  }
  return outcome;
}

/** 保存当前登录态到指定账号槽位。 */
export function traeSaveLogin(
  userId: string,
  variant?: TraeVariantId | null,
): Promise<{ userId: string; fileCount: number }> {
  return call("trae_save_login", { userId, variant: variant ?? null });
}

/** 备份当前登录态到槽位。 */
export function traeBackupProfile(
  slot: string,
  variant?: TraeVariantId | null,
): Promise<{ slot: string; fileCount: number }> {
  return call("trae_backup_profile", { userId: slot, variant: variant ?? null });
}

/** 用槽位快照覆盖客户端登录态（高级操作，应在客户端关闭时使用）。 */
export function traeRestoreProfile(
  slot: string,
  variant?: TraeVariantId | null,
): Promise<{ slot: string; fileCount: number }> {
  return call("trae_restore_profile", { userId: slot, variant: variant ?? null });
}

/** 删除登录态快照。 */
export function traeDeleteProfile(
  slot: string,
  variant?: TraeVariantId | null,
): Promise<{ slot: string }> {
  return call("trae_delete_profile", { slot, variant: variant ?? null });
}

/** 重置 6 层设备标识。 */
export function traeResetDevice(variant?: TraeVariantId | null): Promise<TraeDeviceResetReport> {
  return call("trae_reset_device", variantArgs(variant));
}

// ---------------------------------------------------------------------------
// Trae API 网关（管理面）
// ---------------------------------------------------------------------------
//
// 网关**本体**监听独立端口（默认 57894），不经过这里的 call()：
// 它是给外部 OpenAI 客户端用的，本模块只负责「配置 / 状态 / 日志 / Key」这四件事。

/** 读取网关配置。 */
export function getTraeGatewayConfig(): Promise<unknown> {
  return call("get_trae_gateway_config");
}

/**
 * 保存配置并应用（会启动/重启独立监听）。
 *
 * 参数用 **snake_case 原始形状**：后端两种拼法都接受（`#[serde(alias)]`），
 * 但磁盘上的配置文件只应该有一种风格，否则人工编辑时会出现同一个键两种写法。
 * 页面侧用 `toTraeGatewayConfigRaw()` 从 camelCase 形状转换。
 */
export function saveTraeGatewayConfig(config: TraeGatewayConfigRaw): Promise<unknown> {
  return call("save_trae_gateway_config", { config });
}

/**
 * 网关运行状态 + 账号池摘要 + 逐账号明细。
 *
 * `variant` 决定看**哪条产品线的账号池**（池已按产品线分家）。缺省 = 默认变体，
 * 键集合不变（`status_view` 的 `api_key_prefix` 等字段始终存在）。
 */
export function getTraeGatewayStatus(variant?: TraeVariantId | null): Promise<unknown> {
  return call("trae_gateway_status", variantArgs(variant));
}

/**
 * 网关对外暴露的模型清单（OpenAI `/v1/models` 形状，**随上游刷新**）。
 *
 * 与 `getTraeClientModels` **同源**（都读客户端 `state.vscdb` 里的上游清单），
 * 区别只在形状：这里是扁平去重后的清单 —— 即外部客户端连本网关时看到的那份。
 * 客户端清单读不到时后端回落静态兜底清单，因此**不会为空**。
 */
export function getTraeGatewayModels(variant?: TraeVariantId | null): Promise<unknown> {
  return call("get_trae_gateway_models", variantArgs(variant));
}

/**
 * 客户端（**上游下发**）的模型清单 —— 「随上游刷新」的数据源。
 *
 * 读的是 Trae 客户端 `state.vscdb` 里上游下发的清单缓存，因此客户端刷新过之后
 * 这里读到的就是新清单。`variant` 决定读哪条产品线的客户端；缺省 = 默认变体。
 *
 * 读不到（客户端没启动过 / 没登录 / 还没拉过清单）不是错误：
 * 返回 `source = "missing"` 与可读的 `note`，由界面呈现空态。
 */
export function getTraeClientModels(
  variant?: TraeVariantId | null,
): Promise<TraeClientModelList> {
  return call("get_trae_client_models", variantArgs(variant));
}

/** 多 Key 列表（含归属产品线；不含 hash 与明文）。 */
export function listTraeApiKeys(): Promise<{ keys: TraeApiKeyRecord[] }> {
  return call("list_trae_api_keys");
}

/**
 * 新建 API Key：明文**仅此一次**返回（`{ok,key,record}`）。
 *
 * `variant` 决定该 Key 归属哪条产品线，缺省 = TraeWork（默认变体）。
 */
export function createTraeApiKey(
  name: string,
  variant?: TraeVariantId | null,
): Promise<TraeApiKeyCreated> {
  const args: Record<string, unknown> = { name };
  if (variant) args.variant = variant;
  return call("create_trae_api_key", args);
}

/** 吊销 API Key（置 `revokedAt`，不物理删除）。 */
export function revokeTraeApiKey(id: string): Promise<{ ok: boolean }> {
  return call("revoke_trae_api_key", { id });
}

/** ★ issue #76：更新某把 Trae Key 的策略（完整策略，非补丁）。 */
export function setTraeKeyPolicy(id: string, policy: KeyPolicy): Promise<{ ok: boolean }> {
  return call("set_trae_key_policy", { id, policy });
}

/** ★ issue #76：读某把 Trae Key 的用量快照。 */
export function traeKeyUsage(id: string): Promise<KeyUsageSnapshot> {
  return call("trae_key_usage", { id });
}

/** 撤销吊销（清 `revokedAt`，Key 重新可用；未吊销会被后端拒绝）。 */
export function restoreTraeApiKey(id: string): Promise<{ ok: boolean }> {
  return call("restore_trae_api_key", { id });
}

/** 物理删除**已吊销**的 API Key（未吊销会被后端拒绝）。 */
export function deleteTraeApiKey(id: string): Promise<{ ok: boolean }> {
  return call("delete_trae_api_key", { id });
}

/**
 * 打开 Trae 数据目录（非 Windows 返回结构化 `Unsupported`）。
 *
 * ⚠️ 这是 {@link openDataDir} 的**薄壳**（只为既有调用点保留）：`variant` 先按
 * **区域**折叠成 `scope`，再交给参数化命令。新代码请直接传 `scope`。
 */
export function openTraeDataDir(
  variant?: TraeVariantId | null,
): Promise<{ ok?: boolean; path?: string }> {
  // P5 T03：改调参数化命令 `open_data_dir`；`variant` 先按**区域**折叠成 `scope`。
  // 旧 `open_trae_data_dir` 保留为别名（见 `ALIASES`）。
  return openDataDir(scopeOf("trae", traeRegionOf(variant ?? "cn")));
}

/**
 * 启动**该变体**的 Trae 客户端。
 *
 * 客户端从没启动过时没有 icube 设备凭证，OAuth 网页登录必然以 `dataDirMissing` 失败；
 * 这个动作让用户一键跨过前置条件。**成功只表示已发起启动**，
 * 凭证是否已就绪要由用户点登录后再判（见 Rust 侧 `handlers::launch_client_for`）。
 */
export function launchTraeClient(
  variant?: TraeVariantId | null,
): Promise<{ ok?: boolean; path?: string; variant?: string }> {
  return call("trae_launch_client", variantArgs(variant));
}

/** 最近 N 条网关请求日志（元数据）。 */
export function getTraeGatewayLogs(): Promise<unknown> {
  return call("get_trae_gateway_logs");
}

/** 清空网关请求日志。 */
export function clearTraeGatewayLogs(): Promise<{ ok: boolean }> {
  return call("clear_trae_gateway_logs");
}

// ---------------------------------------------------------------------------
// Qoder（第三个平台）
// ---------------------------------------------------------------------------
//
// ★ 全部命令**参数化 `scope`**：双区域是两本互不相通的账号库，缺 scope 就无从判断
//   该读哪本。`scope` 由 `resolve_scope_param` 在 core 侧 **fail-closed** 解析
//   （拼错即拒，绝不回落默认作用域）—— 前端这边只负责**如实传**。

/** Qoder 账号列表（按区域）。 */
export function getQoderAccounts(scope: Scope): Promise<QoderAccountsView> {
  return call<QoderAccountsView>("get_qoder_accounts", { scope });
}

/** 本机凭据来源探测（只读；**不落库**）。 */
export function getQoderLocalSources(scope: Scope): Promise<QoderLocalSourcesView> {
  return call<QoderLocalSourcesView>("get_qoder_local_sources", { scope });
}

/** 从本机已登录的 Qoder 客户端导入账号。 */
export function qoderImportLocal(scope: Scope): Promise<QoderImportOutcome> {
  return call<QoderImportOutcome>("qoder_import_local", { scope });
}

/** 由 PAT 导入一条账号（纯本地，不访问上游）。 */
export function qoderImportPat(
  scope: Scope,
  pat: string,
  nickname?: string,
): Promise<QoderImportOutcome> {
  return call<QoderImportOutcome>("qoder_import_pat", { scope, pat, nickname: nickname ?? null });
}

/** 改备注（唯一的「只改一个字段」入口）。 */
export function qoderSetAccountRemark(
  scope: Scope,
  uid: string,
  remark: string | null,
): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("qoder_set_account_remark", { scope, uid, remark });
}

/** 删除一条账号。 */
export function qoderDeleteAccount(scope: Scope, uid: string): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("qoder_delete_account", { scope, uid });
}

/** 刷新一条 Qoder 账号的令牌（device / job / PAT 三路由；见 `qoder::refresh` 模块头）。 */
export interface QoderRefreshResult {
  ok: boolean;
  uid: string;
  /** `device` / `job` / `pat-exchange`。 */
  plan: string;
  accessTokenMasked: string;
  /** 新令牌过期时刻（毫秒时间戳）。 */
  expiresAt: number;
}

export function qoderRefreshAccount(scope: Scope, uid: string): Promise<QoderRefreshResult> {
  return call<QoderRefreshResult>("qoder_refresh_account", { scope, uid });
}

/** 一条账号的签到状态行（`ok=false` 时带 `error`；`unavailable` 是区域级结论）。 */
export interface QoderCheckinRow {
  uid: string;
  name: string;
  ok: boolean;
  error?: string;
  unavailable?: boolean;
  status?: string;
  active?: boolean;
  todayCheckedIn?: boolean;
  streakDays?: number;
  totalClaimDays?: number;
  rewardCredits?: number;
  totalRewardCredits?: number;
  nextClaimAt?: number;
  lastClaimedAt?: number;
  /**
   * 状态来自哪条通道：`campaign`（活动平台，现行）/ `legacy`（旧签到通道兜底）。
   *
   * ⚠️ 只有 `campaign` 才能表达「本区域有活动但今天领不到」；`legacy` 只说明
   * 「活动平台这一发没打上」，不能据此告诉用户「未开放」。
   */
  source?: string;
  /** 风控身份来源：`runtime-info`（官方组件）/ `derived`（派生兜底）。 */
  identity?: string;
  /** 机器头状态：`native`（发了全套）/ `omitted`（一个都没发，见 `hint`）。 */
  machineHeaders?: string;
  /** 后端给出的可读说明（国际版无客户端时为什么会看不到活动）。 */
  hint?: string;
  /** 本地台账今天是否已盖章（只作提示，判据永远是 `todayCheckedIn`）。 */
  stampedToday?: boolean;
  /** 今日可得 / 今日已得 / 累计（活动平台口径）。 */
  dailyCredit?: number;
  todayCredit?: number | null;
  totalCredits?: number;
}

/** 区域签到视图（国际版 `unavailable=true` + `reason`，不代表失败）。 */
export interface QoderCheckinStatusView {
  region: string;
  unavailable: boolean;
  reason: string | null;
  summary: { total: number; checked: number; claimable: number };
  accounts: QoderCheckinRow[];
}

export function qoderCheckinStatus(scope: Scope): Promise<QoderCheckinStatusView> {
  return call<QoderCheckinStatusView>("qoder_checkin_status", { scope });
}

/**
 * 逐账号「今日签到状态」（`qoder_checkin_status_map`；issue #47）。
 *
 * 与 Trae 侧、WB 侧**同一个条目形状**（`CheckinStatusEntry`），前端共用一套渲染。
 * 边界：未知 uid 不进 map（显示「—」）；`ok:false` ⇒ 「读不到」；国际版接口不存在时
 * 条目带上游原因，前端据此显示「本区域未开放」而不是「未签到」。
 */
export function qoderCheckinStatusMap(
  scope: Scope,
  accountIds: string[],
): Promise<Record<string, CheckinStatusEntry>> {
  if (accountIds.length === 0) return Promise.resolve({});
  return call("qoder_checkin_status_map", { scope, accountIds: accountIds.join(",") });
}

/** 单账号签到结果（状态驱动：`already` 幂等 / `disabled` 活动未开放）。 */
export interface QoderCheckinClaimResult {
  ok: boolean;
  uid: string;
  already?: boolean;
  disabled?: boolean;
  rewardCredits?: number;
  status?: string;
  rechecked?: boolean;
}

export function qoderCheckinClaim(scope: Scope, uid: string): Promise<QoderCheckinClaimResult> {
  return call<QoderCheckinClaimResult>("qoder_checkin_claim", { scope, uid });
}

/** Pro 升级包资格（**只读**；`available=false` = 活动已下线/本区域没有 ⇒ 不渲染入口）。 */
export interface QoderProPackStatus {
  available: boolean;
  eligible: boolean;
  /** 真正的失败（网络/5xx）；「不可领」不是错误。 */
  error?: string | null;
}

export interface QoderProPackClaimResult {
  ok: boolean;
  /** 已领过（幂等成功）。 */
  already: boolean;
  /** 活动是否仍在线；`false` ⇒ 入口应当消失。 */
  available: boolean;
  message: string;
}

/** 查询 Pro 升级包资格（单账号）。 */
export function qoderProPackStatus(scope: Scope, uid: string): Promise<QoderProPackStatus> {
  return call<QoderProPackStatus>("qoder_pro_pack_status", { scope, uid });
}

/** 领取 Pro 升级包（幂等；已领过返回 `ok=true, already=true`）。 */
export function qoderProPackClaim(
  scope: Scope,
  uid: string,
): Promise<QoderProPackClaimResult> {
  return call<QoderProPackClaimResult>("qoder_pro_pack_claim", { scope, uid });
}

// ---------------------------------------------------------------------------
// issue #111 · Qoder 网页端逐资源包明细
// ---------------------------------------------------------------------------
//
// ⚠️ 这三条**仅桌面端可用**：逐包数据在网页端私有接口，只认浏览器登录后的会话
// Cookie，而只有 Tauri 的 `Webview::cookies()` 能读到它。headless 形态（Docker /
// npm CLI / 裸二进制）调用会得到结构化的「仅桌面端可用」错误 —— 那**不是**「该账号
// 没有资源包」，两者不得混为一谈。

/** 逐包探针结果（认领会话 + 取一次明细；`raw` 是上游原始响应，不含 Cookie）。 */
export interface QoderWebCreditProbe {
  /** 是否拿到了会话（`window` 现读 / `staged` 关窗前暂存 / `stored` 缓存）。 */
  hasSession: boolean;
  /** 会话来源：登录窗现读 / 关窗前暂存 / 本地缓存 / 无。 */
  source?: "window" | "staged" | "stored" | "none";
  cookieCount: number;
  /** 只给名称，**不含值**（Cookie 等价于完整身份凭据）。 */
  cookieNames: string[];
  /** 会话归属者是否与 `uid` 一致；不一致 ⇒ 明细必须丢弃。 */
  uidMatches?: boolean;
  /** 响应形状摘要（分组名 / 行数 / 行内键名），不含额度数值。 */
  shape?: unknown;
  /** 解析后的资源包（S3，字段映射由真样本锚定）。 */
  packs?: QoderCreditPack[];
  /** 上游原始响应（仅探针返回）。 */
  raw?: unknown;
  message?: string;
}

/** 打开（或前置）Qoder 网页端登录窗，供用户登录一次。 */
export function qoderWebLoginOpen(
  scope: Scope,
): Promise<{ opened: boolean; url: string }> {
  return call<{ opened: boolean; url: string }>("qoder_web_login_open", { scope });
}

/** 关闭 Qoder 网页端登录窗（已关闭 ⇒ `closed=false`，不报错）。 */
export function qoderWebLoginClose(): Promise<{ closed: boolean }> {
  return call<{ closed: boolean }>("qoder_web_login_close", {});
}

/** 收割会话 Cookie 并探一次逐包接口（取证；命中即缓存，供下次免登录）。 */
export function qoderWebCreditPacksProbe(
  scope: Scope,
  uid: string,
): Promise<QoderWebCreditProbe> {
  return call<QoderWebCreditProbe>("qoder_web_credit_packs_probe", { scope, uid });
}

/** 网页会话状态（前端据此决定显示「登录」还是「已登录 / 需重新登录」）。 */
export interface QoderWebSessionStatus {
  /** 有新鲜且 uid 匹配的会话。 */
  hasSession: boolean;
  /** 有缓存但已过期或换过账号 ⇒ 该引导重新登录，而非当作「已登录」。 */
  expired: boolean;
  /**
   * 有会话但**还没被 uid 认领**（关窗前刚收割到）。
   *
   * ★ 这是「登录完关窗、还没探过一次」这个真实时刻的状态：此时 `hasSession` 仍是 false，
   * 但用户已经登录了 —— 界面必须给「读取逐包」而不是再说一次「去登录」。
   */
  pending?: boolean;
  source: "window" | "staged" | "stored" | "none";
  cookieNames: string[];
  savedAtMs: number;
  verifiedAtMs: number;
}

/**
 * 登录窗里现有的 Cookie（**只有名称 / 条数 / 指纹，没有值**）。
 *
 * 用途：前端轮询它来判断「用户是否已经在窗口里登录完」。为什么不轮询 probe ——
 * probe 一旦拿到非空 Cookie 就会真打上游，未登录时的匿名 Cookie 换来 401，
 * 401 又会清缓存，40 次轮询就是一串注定失败的请求 + 反复抹掉刚写的缓存。
 */
export interface QoderWebWindowCookies {
  /** 登录窗当前是否开着。 */
  windowOpen: boolean;
  cookieCount: number;
  cookieNames: string[];
  /** 集合指纹（不可逆）；空串 = 窗口没开 / 还没读到。变了即「登录态可能变了」。 */
  fingerprint: string;
}

/** 一个 Qoder 资源包（积分包）。字段名与上游 snake_case 逐字对应。 */
export interface QoderCreditPack {
  group: string;
  id?: string;
  source?: string;
  status?: string;
  limit: number;
  used: number;
  remaining: number;
  unit?: string;
  usagePercentage: number;
  isActive: boolean;
  /** 毫秒；缺省 = 无到期时间（上游 `expires_at = 0`）。 */
  expiresAtMs?: number;
}

/** 逐包汇总（后端算好，前端只显示）。 */
export interface QoderPackSummary {
  /** 有额度的包数（`limit > 0`）。 */
  count: number;
  /** 额度为 0 的占位条目数（套餐条目）。 */
  zeroCount: number;
  limit: number;
  used: number;
  remaining: number;
}

/** 「拿不到逐包」的原因 —— 前端按它选文案，不要统一显示「出错了」。 */
export type QoderPacksUnavailableReason =
  | "no_session"
  | "session_expired"
  | "session_mismatch"
  | "upstream";

export interface QoderWebCreditPacks {
  available: boolean;
  reason?: QoderPacksUnavailableReason;
  message?: string;
  packs: QoderCreditPack[];
  summary?: QoderPackSummary;
  verifiedAtMs?: number;
}

/** 读已缓存的网页会话取逐包明细（不弹登录窗；拿不到时给结构化原因）。 */
export function qoderWebCreditPacks(
  scope: Scope,
  uid: string,
): Promise<QoderWebCreditPacks> {
  return call<QoderWebCreditPacks>("qoder_web_credit_packs", { scope, uid });
}

/** 查询网页会话状态（只读）。 */
export function qoderWebSessionStatus(
  scope: Scope,
  uid: string,
): Promise<QoderWebSessionStatus> {
  return call<QoderWebSessionStatus>("qoder_web_session_status", { scope, uid });
}

/** 逐包取数的出站代理配置（两个区域各一个 URL；空 = 直连）。 */
export interface QoderWebProxyConfig {
  /** 国内版。缺省直连，一般不用填。 */
  cn: string;
  /** 国际版。国内网络直连会被上游按 IP 拦下 ⇒ 通常要填。 */
  global: string;
}

/** 读逐包出站代理配置（只读）。 */
export function qoderWebProxyGet(): Promise<QoderWebProxyConfig> {
  return call<QoderWebProxyConfig>("qoder_web_proxy_get", {});
}

/** 保存逐包出站代理配置（两个 URL 都先过后端形状校验）。 */
export function qoderWebProxySet(
  config: QoderWebProxyConfig,
): Promise<{ ok: boolean; config: QoderWebProxyConfig }> {
  return call("qoder_web_proxy_set", { config });
}

/**
 * 读登录窗里现有的 Cookie **名称与条数**（只读、不打上游）。
 *
 * 登录窗开着时轮询它：`fingerprint` 变化 ⇒ 用户在窗口里登录完了 ⇒ 再调一次
 * {@link qoderWebCreditPacksProbe} 认领会话。副作用：后端会顺手把这份 Cookie
 * 暂存在内存里，于是渲染路径（{@link qoderWebCreditPacks}）立刻就能用上它。
 */
export function qoderWebWindowCookies(scope: Scope): Promise<QoderWebWindowCookies> {
  return call<QoderWebWindowCookies>("qoder_web_window_cookies", { scope });
}

/** 清除本地网页会话（「退出网页登录」；幂等）。 */
export function qoderWebSessionLogout(scope: Scope): Promise<{ cleared: boolean }> {
  return call<{ cleared: boolean }>("qoder_web_session_logout", { scope });
}

/** 限额护栏视图（`GET /api/qoder/gateway/quota`）。 */
export interface QoderGatewayQuotaView {
  /** 记账日期（CST，`YYYY-MM-DD`）。 */
  date: string;
  limits: {
    reserveCredits: number;
    dailyTokenLimit: number;
    /** G3：当日 credit 上限（浮点；0 = 关）。 */
    dailyCreditLimit: number;
    modelDailyTokenLimit: number;
    /** 三条是否有任意一条开着（全关时界面要说明「护栏未启用」）。 */
    enabled: boolean;
  };
  accounts: {
    uid: string;
    name: string;
    /** 今日经网关转发的 token 总量（prompt + completion）。 */
    tokens: number;
    /** 今日经网关消耗的 credits（上游 `usage.credits` 累加；浮点）。 */
    credits: number;
    perModel: { model: string; tokens: number }[];
    /** 被拦的原因代号（`null` = 放行）：`parked` / `reserve` / `daily_tokens` / `model_tokens`。 */
    blocked: string | null;
    blockedReason: string | null;
    /** 402 停放解冻时刻（毫秒；`null` = 没停放）。 */
    parkedUntil?: number | null;
    parkedReason?: string | null;
    /** 最近一次余额读数（未知 = `null`，不是 0）。 */
    creditsRemaining?: number | null;
    creditsAt?: number | null;
  }[];
}

/** 一条账号的额度行（`ok=false` 时带 `error`；部分失败不被伪装成全额）。 */
export interface QoderCreditsRow {
  uid: string;
  name: string;
  ok: boolean;
  error?: string;
  remaining?: number;
  used?: number;
  total?: number;
  isExceeded?: boolean;
  plan?: string | null;
  raw?: unknown;
}

/** 区域额度 / 套餐视图。 */
export interface QoderCreditsView {
  region: string;
  summary: { okAccounts: number; failedAccounts: number; remaining: number; total: number };
  accounts: QoderCreditsRow[];
}

export function qoderCredits(scope: Scope): Promise<QoderCreditsView> {
  return call<QoderCreditsView>("qoder_credits", { scope });
}

/**
 * 发起 Qoder 设备授权登录（PKCE S256）。
 *
 * ★ 形状与 WB 的 `oauthStart` **逐字同构**（`{loginId, verificationUri, expiresIn}`），
 * 因为前端复用同一套「发起 → 打开浏览器 → 轮询」的交互壳。差异只有一处：**必须带 scope**
 * —— 双区域各有自己的授权页与 `client_id`，缺 scope 无从判断该发哪一区的登录。
 */
export function qoderOauthStart(scope: Scope): Promise<QoderOAuthStartResult> {
  return call<QoderOAuthStartResult>("qoder_oauth_start", { scope });
}

/** 轮询一次 Qoder 登录结果（`{done, result?, error?}`；形状同 WB 的 `oauthStatus`）。 */
export function qoderOauthStatus(scope: Scope, loginId: string): Promise<QoderOAuthPollResult> {
  return call<QoderOAuthPollResult>("qoder_oauth_status", { scope, loginId });
}

/** OAuth 发起结果（与 `OAuthStartResult` 同形，独立命名以便按平台演进）。 */
export interface QoderOAuthStartResult {
  loginId: string;
  verificationUri: string;
  expiresIn: number;
}

/** OAuth 轮询结果：`result` 是**下发形态**的账号视图（不含令牌明文）。 */
export interface QoderOAuthPollResult {
  done: boolean;
  result?: QoderAccountView;
  error?: string;
}

/**
 * 一个程序位的状态（只读）。
 *
 * ★ `state` 是**四态**（`ready` / `missing` / `unreadable` / `empty`），不是布尔 ——
 * 四种状态对应四种不同的用户动作，压成「未检测到」会把用户引去重装一个装好的客户端。
 * ★ `running` 单独给：客户端在跑时不能切换（退出时内存态会覆盖我们写的）。
 */
export interface QoderProgramStatus {
  id: string;
  label: string;
  state: string;
  detail: string | null;
  running: boolean;
  /** 该程序位当前登着的账号 uid（`null` = 读不到）。 */
  currentUid: string | null;
}

export interface QoderProgramsView {
  region: string;
  programs: QoderProgramStatus[];
}

/** 切换结果（**不含令牌明文**，只有掩码与写入目标）。 */
export interface QoderSwitchResult {
  ok: boolean;
  program: string;
  region: string;
  uid: string;
  backupPath: string | null;
  written: { path: string; form: string; tokenMasked: string };
}

/** 程序位状态（只读）。 */
export function qoderProgramStatus(scope: Scope): Promise<QoderProgramsView> {
  return call<QoderProgramsView>("qoder_program_status", { scope });
}

/**
 * 切换账号：把该账号的凭据**写回**指定程序位的登录态。
 *
 * ⚠️ 这是写客户端文件的动作：客户端在跑时后端会拒绝（进程守卫），写后校验失败会自动还原备份。
 */
export function qoderSwitchAccount(
  scope: Scope,
  program: string,
  uid: string,
): Promise<QoderSwitchResult> {
  return call<QoderSwitchResult>("qoder_switch_account", { scope, program, uid });
}

/** 改自定义分组（与备注同构；`null` / 空串 = 未分组）。 */
export function qoderSetAccountGroup(
  scope: Scope,
  uid: string,
  group: string | null,
): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("qoder_set_account_group", { scope, uid, group });
}

/** 逐日用量的一天（**单位是 credits，不是 token** —— 见 `qoder::usage` 模块头）。 */
export interface QoderUsageDay {
  date: string;
  credits: number;
  requests: number;
  byModel: Array<{ model: string; credits: number }>;
}

/** 逐日用量视图。`unit` 恒为 `"credits"`；`source` 说明数据来源与局限（要展示给用户）。 */
export interface QoderUsageView {
  region: string;
  unit: string;
  days: QoderUsageDay[];
  totals: { credits: number; requests: number; sessions: number; models: number };
  byModel: Array<{ model: string; credits: number }>;
  /**
   * 会话排行（credits 降序，最多 200 行）。
   *
   * ⚠️ `title` 可能是**空串**（客户端那条会话没有 user 消息 / 结构不认识）——
   * 前端回落成短 id，不编「未命名会话」。
   */
  sessions: QoderUsageSession[];
  source: {
    kind: string;
    paths: string[];
    sessions: number;
    exists: boolean;
    tokenCountsAvailable: boolean;
    note: string;
  };
}

/** 一条会话的用量（会话视图的行；来自本机客户端 jsonl，按 sessionId 聚合）。 */
export interface QoderUsageSession {
  id: string;
  /** 首个 user 消息正文（截断 80 字）；空串 ⇒ 前端回落短 id。 */
  title: string;
  credits: number;
  requests: number;
  /** 该会话用过的模型（credits 降序）；空数组 = 全是没有模型名的行。 */
  models: string[];
  firstDate: string;
  lastDate: string;
}

/**
 * 逐日用量（`days` = 回看窗口，含今天；缺省由调用方决定）。
 *
 * ★ 单位是 **credits**：Qoder 客户端按 credits 计费，token 计数恒为 0 ⇒ 不伪造 token。
 */
export function qoderUsage(scope: Scope, days?: number): Promise<QoderUsageView> {
  // ⚠️ 实参必须是**对象字面量**（`check:api` 第 8 条要静态读出顶层 `scope`）——
  // 写成三元表达式会让门禁无法校验，从而漏掉「参数化只做一半」。
  return call<QoderUsageView>("qoder_usage", { scope, days: days ?? null });
}

// ---------------------------------------------------------------------------
// Qoder API 网关管理面（Tauri 命令 + webui 路由同源）
// ---------------------------------------------------------------------------
//
// ⚠️ 形状与 WB / Trae 两条网关线路**刻意对齐**（snake_case 状态视图 / 同一个脱敏
// `{keys:[…]}` 列表 / 同一份日志字段）；差异只有两处：归属轴是**区域**、
// **一个监听服务两个区域**。
//
// ⚠️ 三条命令登记在 `SCOPE_PARAMETERIZED`（`qoder_gateway_status` /
//   `get_qoder_gateway_models` / `create_qoder_api_key`）—— 参数化只做一半会在
//   桌面端与 webui 之间分叉，而构建全绿（见 `check-api-contract.cjs` 第 8 条）。

/** Qoder 网关配置（snake_case 原始形状，交给 `normalizeQoderGatewayConfig`）。 */
export function getQoderGatewayConfig(): Promise<unknown> {
  return call("get_qoder_gateway_config");
}

/** 保存配置并应用（启动 / 重启独立监听）。 */
export function saveQoderGatewayConfig(config: QoderGatewayConfigRaw): Promise<unknown> {
  return call("save_qoder_gateway_config", { config });
}

/** 网关运行状态 + 该区域账号池（`scope` 决定看哪本账号库）。 */
export function qoderGatewayStatus(scope: Scope): Promise<unknown> {
  return call("qoder_gateway_status", { scope });
}

/**
 * 限额护栏视图（逐账号当日用量 + 拦截原因 + 余额快照）。
 *
 * 与 `/gateway/status` 同一条纪律：`scope` 必填（两区两本计数文件，缺 scope 不该读任何一本）。
 */
export function qoderGatewayQuota(scope: Scope): Promise<QoderGatewayQuotaView> {
  return call<QoderGatewayQuotaView>("qoder_gateway_quota", { scope });
}

/** 对外暴露的模型清单（**未过滤**禁用 + 来源 / 原因）。`refresh` ⇒ 绕过 10 分钟缓存。 */
export function getQoderGatewayModels(scope: Scope, refresh = false): Promise<unknown> {
  // ⚠️ 两个分支都必须是**对象字面量**：门禁 `check-api-contract.cjs` 第 8 条据此校验
  //    `scope` 在场 —— 写成 `call("<cmd>", args)`（变量实参）会让「参数化只做一半」的
  //    漂移无法被机器拦住（桌面端与 webui 行为分叉，而构建全绿）。
  return refresh
    ? call("get_qoder_gateway_models", { scope, refresh: true })
    : call("get_qoder_gateway_models", { scope });
}

/** 多 Key 列表（含归属区域；脱敏）。 */
export function listQoderApiKeys(): Promise<unknown> {
  return call("list_qoder_api_keys");
}

/** 新建 Key —— **明文仅此一次返回**。 */
export interface QoderCreateApiKeyResult {
  ok: boolean;
  /** 一次性明文（`sk-qoder-` 前缀）。 */
  key?: string;
  record?: { id: string; name: string; region: string; prefix: string };
}

export function createQoderApiKey(scope: Scope, name: string): Promise<QoderCreateApiKeyResult> {
  return call<QoderCreateApiKeyResult>("create_qoder_api_key", { scope, name });
}

/** 吊销 Key（置 `revokedAt`；可逆）。 */
export function revokeQoderApiKey(id: string): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("revoke_qoder_api_key", { id });
}

/** ★ issue #76：更新某把 Qoder Key 的策略（完整策略，非补丁）。 */
export function setQoderKeyPolicy(id: string, policy: KeyPolicy): Promise<{ ok: boolean }> {
  return call("set_qoder_key_policy", { id, policy });
}

/** ★ issue #76：读某把 Qoder Key 的用量快照。 */
export function qoderKeyUsage(id: string): Promise<KeyUsageSnapshot> {
  return call("qoder_key_usage", { id });
}

/** 撤销吊销（Key 重新可用；明文未变，客户端无需改配置）。 */
export function restoreQoderApiKey(id: string): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("restore_qoder_api_key", { id });
}

/** 物理删除**已吊销**的 Key。 */
export function deleteQoderApiKey(id: string): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("delete_qoder_api_key", { id });
}

/** 请求日志（元数据；条目含 `region` 归属）。 */
export function getQoderGatewayLogs(): Promise<unknown> {
  return call("get_qoder_gateway_logs");
}

/** 清空日志。 */
export function clearQoderGatewayLogs(): Promise<{ ok: boolean }> {
  return call<{ ok: boolean }>("clear_qoder_gateway_logs");
}

/** 一条 Qoder 账号的**下发形态**（**不含令牌明文**）。 */
export interface QoderAccountView {
  uid: string;
  name: string;
  nickname: string | null;
  domain: string;
  region: string;
  source: string | null;
  remark: string | null;
  /** 自定义分组（自由文本；`null` = 未分组）。 */
  group: string | null;
  expiresAt: number | null;
  hasAccessToken: boolean;
  hasRefreshToken: boolean;
  hasPersonalToken: boolean;
  accessTokenMasked: string | null;
  credential: {
    access: string | null;
    refresh: string | null;
    pat: boolean;
    unrecognized: boolean;
  };
  credentialSummary: string;
  hasCredential: boolean;
  domainMismatch: boolean;
  addedAt: string | null;
  updatedAt: string | null;
}

/** 账号列表（含归属不符计数）。 */
export interface QoderAccountsView {
  region: string;
  domain: string;
  accounts: QoderAccountView[];
  foreignDomainCount: number;
}

/** 一处本机凭据来源的探测结果。 */
export interface QoderLocalSource {
  kind: string;
  label: string;
  path: string;
  /** `missing` / `unreadable` / `empty` / `ready` —— 四态必须分开（引导动作不同）。 */
  state: "missing" | "unreadable" | "empty" | "ready";
  detail: string | null;
}

/** 本机来源探测视图。 */
export interface QoderLocalSourcesView {
  region: string;
  domain: string;
  sources: QoderLocalSource[];
}

/** 导入结果（新增 / 更新分开报，以及逐来源的问题说明）。 */
export interface QoderImportOutcome {
  region: string;
  imported?: number;
  updated?: number;
  found?: number;
  uid?: string;
  outcome?: "added" | "updated";
  masked?: string;
  problems?: Array<{ kind: string; label: string; state: string; detail: string }>;
  failed?: Array<{ uid: string; kind: string; error: string }>;
}

// ---------------------------------------------------------------------------
// 桌面悬浮窗（P9 · 只读 + 显隐 + 聚焦）
// ---------------------------------------------------------------------------
//
// ★ 只读红线（`PLAN §3.16`）：命令面**只有**读 / 显隐 / 聚焦，**无任何写操作**。
//   尤其 `companionJump` **不切账号、不导航主窗口** —— 它只「把你带到来源客户端」。
//
// ★ 悬浮窗本体是**桌面端**的独立窗口：webui（浏览器）没有它，`set_visible` /
//   `toggle` / `jump` 在 webui 会返回结构化「仅桌面端可用」。故这三条**不**进
//   `DEMO_READ_COMMANDS`（演示模式统一提示不可操作）。

/** 读取悬浮窗的**只读**快照（app 级聚合，无 scope 参数）。 */
export function companionStatus(): Promise<CompanionSnapshot> {
  return call<CompanionSnapshot>("companion_status");
}

/** 显示 / 隐藏悬浮窗（可见性的**唯一收敛点**：侧栏图标 / 托盘 / 窗口内关闭按钮都走它）。 */
export function companionSetVisible(visible: boolean): Promise<void> {
  return call<void>("companion_set_visible", { visible });
}

/** 翻转悬浮窗可见性，返回翻转**之后**的状态。 */
export function companionToggle(): Promise<boolean> {
  return call<boolean>("companion_toggle");
}

/**
 * 聚焦 / 唤起**来源客户端**（P9 ✅ #1）。
 *
 * ★ **不切账号、不导航主窗口、不删改任何数据**。来源客户端未运行 ⇒ 后端返回
 * **可理解的错误**，调用方须渲染成明确反馈（**不静默失败**）。
 */
export function companionJump(id: string): Promise<void> {
  return call<void>("companion_jump", { id });
}

// ---------------------------------------------------------------------------
// 外部 Agent 会话监听（P15 · hook 安装面）
// ---------------------------------------------------------------------------
//
// ★ 这与悬浮窗的**只读红线不冲突**：那一条约束的是**窗口**（`companion_*`）。
//   本段改的是**上游客户端的配置文件**（装 / 卸 hook），必须是用户显式动作，
//   且默认关（`app_prefs.watch_agent_sessions = false`）。
//
// ★ 装 / 卸**不新造写入路径**：一律走 `client_settings` 的补丁 ——
//   它自带格式保持、写前备份、**进程守卫**（客户端在运行就拒写，会被它的内存态覆盖）。

/** 单个客户端的 hook 安装状态。 */
export interface AgentHookSlot {
  /** `client_settings` 的槽位键（如 `workbuddy-cn`）。 */
  slot: string;
  /** 会话来源 id（同时是 hook 命令 `--source` 的取值）。 */
  source: string;
  /** 该客户端**是否安装**（未安装的槽位不会被写配置）。 */
  clientInstalled: boolean;
  /**
   * 该客户端**是否正在运行**。
   *
   * ⚠️ 运行中改不了它的配置（进程守卫）⇒ 这一位为 `true` 时开启会失败，
   * 调用方必须提示用户「先退出客户端」，**不能只说一句失败**。
   */
  clientRunning: boolean;
  /** 是否已装了我们**至少一条** hook。 */
  installed: boolean;
  /** 已装了哪些事件（空数组 = 一条都没有）。 */
  events: string[];
  /** 装出去的那条命令（诊断用；取不到当前程序路径时为 `null`）。 */
  command: string | null;
}

/** hook 安装状态（**只读**）。 */
export interface AgentHookStatus {
  /** 用户偏好里的开关（**唯一事实来源**）。 */
  enabled: boolean;
  /** 接收端端口。 */
  port: number;
  slots: AgentHookSlot[];
}

/** 读取 hook 安装状态（**只读**）。 */
export function getAgentHookStatus(): Promise<AgentHookStatus> {
  return call<AgentHookStatus>("get_agent_hook_status");
}

/** 开启 / 关闭外部 Agent 会话监听的结果。 */
export interface AgentHookToggleResult {
  enabled: boolean;
  /** 真正改动的槽位。 */
  changed: Array<{ slot: string; ok: boolean }>;
  /**
   * 失败原因（**逐槽位**）。
   *
   * ★ 部分成功是常态（国内版装了、国际版没装，或某个客户端正在运行）——
   * 调用方必须把 `errors` 如实呈现，**不静默失败**。
   */
  errors: string[];
}

/**
 * 开启 / 关闭外部 Agent 会话监听。
 *
 * ⚠️ 开启会**往上游客户端 `settings.json` 写 hook 配置**（默认关，须用户显式同意）。
 */
export function setAgentHookEnabled(enabled: boolean): Promise<AgentHookToggleResult> {
  return call<AgentHookToggleResult>("set_agent_hook_enabled", { enabled });
}

export type { TraeGatewayLogEntry, TraeGatewayStatus };
