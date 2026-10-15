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
  GatewayConfig,
  GatewayLogEntry,
  GatewayStatus,
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
import { scopeOf } from "./scope";
import type { Scope } from "./scope";
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
  "get_account_strategy", "get_disabled_models", "get_gateway_logs",
  // 「今日 Token」（**只读**观测）：演示站需返回虚构数据，否则 build:demo 报错。
  "get_account_token_today",
  // Trae 分区只读命令（演示站需返回虚构数据，否则 build:demo 报错）。
  // 只登记**只读**命令：写操作（签到 / 增删账号 / 切换 / 重置设备…）一律不进这里，
  // 由 `DemoAction` 包裹后在演示模式下统一提示不可操作。
  "get_trae_env", "get_trae_variants", "get_trae_capabilities", "get_trae_accounts",
  "get_trae_checkin_status",
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
  // 账号池：人工解除某账号的禁用。禁用是**终态且落盘**，此前没有任何出口
  // （余额刷新被 `disabled` 挡住、`revive` 在生产代码里零调用点）。
  revive_gateway_account: { method: "POST", path: "/api/gateway/pool/revive" },
  // 统一网关（第三个产品线）：与上面两条网关线路**平行**，但它是**跨作用域**的 ——
  // 因此这些命令**不进** SCOPE_PARAMETERIZED（拿不到也不该拿 scope）。
  get_unified_gateway_config: { method: "GET", path: "/api/unified-gateway/config" },
  get_unified_gateway_models: { method: "GET", path: "/api/unified-gateway/models" },
  save_unified_gateway_config: { method: "POST", path: "/api/unified-gateway/config" },
  list_unified_api_keys: { method: "GET", path: "/api/unified-gateway/keys" },
  create_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys" },
  revoke_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys/revoke" },
  restore_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys/restore" },
  delete_unified_api_key: { method: "POST", path: "/api/unified-gateway/keys/delete" },
  get_gateway_models: { method: "GET", path: "/api/gateway/models" },
  refresh_gateway_models: { method: "POST", path: "/api/gateway/models/refresh" },
  get_account_strategy: { method: "GET", path: "/api/gateway/strategy" },
  save_account_strategy: { method: "POST", path: "/api/gateway/strategy" },
  // 模型禁用偏好（P11）：GET 列表 / POST 置位·清位，同一路径两种方法（与 `/api/gateway/strategy` 同构）。
  get_disabled_models: { method: "GET", path: "/api/gateway/disabled-models" },
  set_model_disabled: { method: "POST", path: "/api/gateway/disabled-models" },
  get_gateway_logs: { method: "GET", path: "/api/gateway/logs" },
  clear_gateway_logs: { method: "POST", path: "/api/gateway/logs/clear" },

  // ---- Trae 模块 ----
  // 键名必须与 src-tauri/src/lib.rs 的 invoke_handler 登记名、
  // 以及 crates/buddy-switch-server/src/api.rs 的路由 path+method 三方一致，
  // 由 scripts/check-api-contract.cjs 在构建前校验。
  get_trae_env: { method: "GET", path: "/api/trae/env" },
  get_trae_variants: { method: "GET", path: "/api/trae/variants" },
  get_trae_capabilities: { method: "GET", path: "/api/trae/capabilities" },
  get_trae_accounts: { method: "GET", path: "/api/trae/accounts" },
  get_trae_checkin_status: { method: "GET", path: "/api/trae/checkin/status" },
  get_trae_credits: { method: "GET", path: "/api/trae/credits" },
  get_trae_token_statistics: { method: "GET", path: "/api/trae/token-stats" },
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
  // 撤销吊销（恢复）：与 WorkBuddy 侧同形同语义。
  restore_trae_api_key: { method: "POST", path: "/api/trae/gateway/keys/restore" },
  delete_trae_api_key: { method: "POST", path: "/api/trae/gateway/keys/delete" },
  // 打开 Trae 数据目录（非 Windows 返回结构化 Unsupported）。
  open_trae_data_dir: { method: "POST", path: "/api/trae/open-data-dir" },
  // 启动该变体的 Trae 客户端（OAuth 网页登录的前置动作：客户端首次启动才写出设备凭证）。
  trae_launch_client: { method: "POST", path: "/api/trae/launch-client" },
  get_trae_gateway_logs: { method: "GET", path: "/api/trae/gateway/logs" },
  clear_trae_gateway_logs: { method: "POST", path: "/api/trae/gateway/logs/clear" },
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
  if (!isWebui()) return localizeCodedStrings(await invoke<T>(cmd, args)) as T;
  return httpCall<T>(cmd, args);
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

export function exportAccounts(accountIds: string[], region?: Region): Promise<{ ok: boolean; accounts: AccountRecord[] }> {
  // P5 T03：改调参数化命令 `accounts_export`（`scope` 决定平台与区域）。旧 `export_accounts`
  // 保留为别名（见 `ALIASES`），桌面端仍登记、webui 仍路由。
  return call("accounts_export", { scope: scopeOf("workbuddy", region ?? "cn"), accountIds });
}

/** 桌面端：把完整记录写入用户选择的路径（系统保存对话框产物）。 */
export function exportAccountsToPath(
  accountIds: string[],
  path: string,
  region?: Region,
): Promise<{ ok: boolean; path: string }> {
  // P5 T03：改调参数化命令 `accounts_export_to_path`。旧 `export_accounts_to_path` 保留为别名。
  return call("accounts_export_to_path", {
    scope: scopeOf("workbuddy", region ?? "cn"),
    accountIds,
    path,
  });
}

export function previewImportAccounts(
  fileText: string,
  region?: Region,
): Promise<{ accounts: ImportPreviewAccount[]; total: number }> {
  return call("preview_import_accounts", { fileText, ...regionArg(region) });
}

export function importAccounts(fileText: string, indexes: number[], region?: Region): Promise<ImportResult> {
  // P5 T03：改调参数化命令 `accounts_import`。旧 `import_accounts` 保留为别名。
  return call("accounts_import", { scope: scopeOf("workbuddy", region ?? "cn"), fileText, indexes });
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
 * 三条边界（见 {@link AccountTokenTodayResponse}）：
 * - `accounts` 里**缺失**某账号 = 「今日无记录 / 未知」⇒ UI 渲染 **「—」**，**不得**当 0；
 * - 出现在 `accounts` 里的 `0` 是**真实 0**；
 * - `gatewayTruncated: true` ⇒ 网关部分只是**下界** ⇒ UI 渲染 **`≥N`** 并说明。
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

export function saveGatewayConfig(config: GatewayConfig): Promise<GatewayConfig> {
  return call("save_gateway_config", { config: config as unknown as Record<string, unknown> });
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

export function getUnifiedGatewayModels(): Promise<UnifiedModelsPreview> {
  return call("get_unified_gateway_models");
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

/**
 * 人工解除账号池里某账号的禁用（**幂等**：本来可用时 `revived:false`，不报错）。
 *
 * 与「API Key 的恢复」同构 —— 禁用是终态且落盘持久化，不解除就**永久不可用**。
 */
export function reviveGatewayAccount(uid: string): Promise<{ ok: boolean; revived: boolean }> {
  return call("revive_gateway_account", { uid });
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
 * 读取某 region 被用户禁用的模型 id 列表（P11）。
 *
 * 后端读的是**进程内唯一**的禁用存储 —— 与 `/v1/models` 过滤（出口 ①）、
 * 调用拒绝（出口 ②）是同一份数据，因此这里看到的禁用集合就是两个出口真正在用的那份。
 *
 * 形状 `{ region, ids }`；`ids` 为空即「该 region 没有禁用任何模型」（默认全开，D27）。
 */
export function getDisabledModels(region: Region): Promise<{ region: Region; ids: string[] }> {
  return call("get_disabled_models", { region });
}

/**
 * 置位 / 清位某 region 的一个模型（P11）—— 禁用（`disabled=true`）/ 启用（`false`）。
 *
 * 后端幂等：重复置位 / 清位返回 `changed: false` 且不落盘。
 * `region` 按**后端 region 粒度**传（`cn` / `global`）—— 两个出口都按 region 键控（决策 #4）。
 */
export function setModelDisabled(
  region: Region,
  modelId: string,
  disabled: boolean,
): Promise<{ ok: boolean; region: Region; model_id: string; disabled: boolean; changed: boolean }> {
  return call("set_model_disabled", { region, modelId, disabled });
}

export function getGatewayLogs(): Promise<{ logs: GatewayLogEntry[] }> {
  return call("get_gateway_logs");
}

export function clearGatewayLogs(): Promise<{ ok: boolean }> {
  return call("clear_gateway_logs");
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
  if (!isDesktop()) return Promise.resolve({ close_to_tray: true });
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
 * 后端在**区域内全部程序位**里按序尝试（账号库按区域分家，写侧区域纯；本机哪条线
 * 登录着只有读侧探测才知道）——只装了 TraeCode CN 的机器也能直接导入，不会误报
 * 「未检测到【TraeWork CN】客户端」。`variant` 只用于折叠出**区域**作用域
 * （scope 模型 `trae-cn` / `trae-global` 没有程序位维度，程序位在服务端回退）。
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
// 网关**本体**监听独立端口（默认 7864），不经过这里的 call()：
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
