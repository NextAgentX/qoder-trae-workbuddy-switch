// 与 Rust 后端命令返回结构对齐的类型定义（对照 server.py 各 API 响应）

// P11：模型行视图带「归属作用域」字段（`Scope`），故引入类型（`import type` 编译期擦除）。
import type { Scope } from "./scope";

/** 目标版本：cn=国内版（WorkBuddy），global=国际版（WorkBuddy AI）。缺省一律按 "cn" 处理。 */
export type Region = "cn" | "global";

/**
 * 统计查询范围：`cn` / `global` 单版，`all` 为两版合并。
 * 与「实体归属」的 `Region` 正交 —— `"all"` 只出现在查询范围，绝不出现在实体归属字段。
 */
export type RegionFilter = Region | "all";

export interface AccountMeta {
  id: string;
  uid: string | null;
  email: string | null;
  nickname: string | null;
  enterpriseName: string | null;
  expiresAt: number | null;
  refreshExpiresAt: number | null;
  refreshedAt: number | null;
  createdAt: number | null;
  needsRelogin: boolean;
  needsReloginReason: string | null;
  /**
   * 用户自填备注（例如「DS4.1 额度 · 10/03 解禁」）。
   *
   * 后端**原样透传**：未设置时为 `null` 或缺失，前端按「有值才渲染」处理。
   */
  remark?: string | null;
  /** 该账号所属版本（后端 region 化后返回；缺省视为 "cn"）。 */
  region?: Region;
}

/** 认证文件 domain 与目标 region 不符时的结构化信息（安全红线 F）。 */
export interface RegionMismatch {
  /** 实际读到的登录域，如 "www.workbuddy.ai"。 */
  actualDomain: string;
  /** 期望的认证文件名，如 "workbuddy-desktop.info"。 */
  expectedFile: string;
  /** 期望的路径覆盖环境变量名，如 "WORKBUDDY_AUTH_FILE"。 */
  envVar: string;
  actualRegion?: Region;
  expectedRegion?: Region;
}

export interface AppStatus {
  running: boolean;
  authFile: string;
  current: {
    uid: string | null;
    nickname: string | null;
    email: string | null;
  } | null;
  appPath: string;
  version: string;
  /** 该 region 客户端是否已安装（后端探测；缺省时前端按登录态/账号库兜底判定）。 */
  installed?: boolean;
  /** 认证文件 domain 与目标 region 不符时返回（缺省表示无冲突）。 */
  regionMismatch?: RegionMismatch | null;
  /** 该状态对应的 region（后端 region 化后返回；缺省视为 "cn"）。 */
  region?: Region;
}

export interface OAuthStartResult {
  loginId: string;
  verificationUri: string;
  expiresIn: number;
}

export interface OAuthPollResult {
  done: boolean;
  result?: AccountMeta;
  error?: string;
}

/** 导出文件中的完整账号记录（含 token，仅导出命令返回；字段与账号库原始记录一致）。 */
export interface AccountRecord {
  id?: string;
  uid?: string | null;
  nickname?: string | null;
  email?: string | null;
  access_token?: string | null;
  refresh_token?: string | null;
  token_type?: string | null;
  domain?: string | null;
  expiresAt?: number | null;
  refreshExpiresAt?: number | null;
  auth_raw?: unknown;
  profile_raw?: unknown;
  createdAt?: number | null;
  [key: string]: unknown;
}

/**
 * 回收站条目（P5 §2.7）—— 线上形态（camelCase）。
 *
 * `key` 是恢复 / 永久删除要回传给后端的账号标识：WorkBuddy 侧优先 `id`、其次 `uid`
 * （两者都能命中后端 `id|uid` 谓词）；Trae 侧为 `userId`。`expiresAt` 用于渲染到期时间。
 */
export interface TrashEntry {
  /** 恢复 / 永久删除的回传键（WB：`id` 优先、`uid` 兜底；Trae：`userId`）。 */
  key: string;
  /** WorkBuddy 侧账号 `id`（Trae 侧为 `undefined`）。 */
  id?: string | null;
  /** WorkBuddy 侧账号 `uid`（Trae 侧为 `undefined`）。 */
  uid?: string | null;
  /** Trae 侧账号 `userId`（WorkBuddy 侧为 `undefined`）。 */
  userId?: string | null;
  /** 展示名（可能为空串 —— 渲染时必须走 `displayText` 兜底，不得留白屏）。 */
  name: string;
  /** 删除时间（ms）。 */
  deletedAt: number;
  /** 到期时间（ms）= `deletedAt + 30 天`。 */
  expiresAt: number;
  /** 来源作用域（`wb-cn` / `wb-global` / `trae-cn` / `trae-global`）。 */
  originScope: string;
}

/** 回收站列表响应（P5 §2.7）：`scope` + 条目（按删除时间倒序，最新在前）。 */
export interface TrashList {
  scope: string;
  entries: TrashEntry[];
}

/** 导入文件账号的脱敏预览（不含 token）。 */
export interface ImportPreviewAccount {
  index: number;
  uid: string | null;
  nickname: string | null;
  email: string | null;
  hasToken: boolean;
}

/** 导入结果计数。 */
export interface ImportResult {
  ok: boolean;
  imported: number;
  skipped: number;
  overwritten: number;
}

export interface Session {
  id: string;
  title: string;
  cwd: string;
  updatedAt: number;
  hasHistory: boolean;
  /** WorkBuddy playground（侧栏「任务」）；缺省视为空间会话。 */
  isPlayground?: boolean;
}

/**
 * 一份会话在某个账号里的副本（= copy ledger 的一条边的**一端**）—— P8 T02。
 *
 * ⚠️ **字段归属**（本项目已两次踩坑）：`title` / `updatedAt` / `hasHistory` 属于
 * `list_sessions` 的会话行，**不属于** ledger 边；ledger 只给 `accountUid` / `sessionId`
 * / `region`（见 `session::list_replica_groups_for`）。别混。
 */
export interface SessionReplica {
  /** 副本所属账号 uid（ledger 的 source_uid / target_uid）。 */
  accountUid: string;
  /** 该账号里的会话 id（复制生成的新 id，或源会话 id）。 */
  sessionId: string;
  /** 副本所在 region（本期恒为**当前 region**，跨 region 边不引入，P8 ✅ #2）。 */
  region: Region;
}

/** 一份会话的跨账号副本组（由 ledger 边连通而成）。 */
export interface SessionReplicaGroup {
  /** 稳定组键（用于 React key 与抽屉定位）。 */
  key: string;
  replicas: SessionReplica[];
}

/** `list_session_replicas` 的返回（`source: "empty"` = 无可用登记 ⇒ 结构化空态）。 */
export interface SessionReplicaResponse {
  groups: SessionReplicaGroup[];
  source: "ledger" | "empty";
}

export interface CopyResult {
  id: string;
  newId: string;
  jsonlCopied: boolean;
  /** 是否成功写入目标账号的 sessions 索引行；false 表示索引库不可写（已降级为只复制 jsonl 正文）。 */
  sessionRowWritten?: boolean;
  mappingWritten: boolean;
  backup: string;
  /** 命中去重账本：该会话此前已复制到目标账号且副本仍存活，本次跳过。 */
  deduplicated?: boolean;
  /** 本次是否成功写入去重账本（仅未去重时出现）。 */
  ledgerWritten?: boolean;
  /** 降级警告（如「目标索引不可写，已复制正文，请重启 WorkBuddy 重建索引」）。 */
  warning?: string;
}

export interface SwitchResult {
  ok: boolean;
  account: string;
  backup: string | null;
  sessionCopy?: {
    sourceUid: string;
    targetUid: string;
    copied: CopyResult[];
    /** 因已存在存活副本而被跳过的会话（去重命中）。 */
    skipped?: CopyResult[];
    errors?: { id: string; error: string }[];
  };
}

/** Memory 合并计数；失败时为 `{ error }`。 */
export interface MemoryMergeSection {
  targetLines: number;
  sourceLines: number;
  appended: number;
  skippedDuplicate: number;
  changed: boolean;
  /** 改前原文的备份文件路径；未改写或目标原本不存在时为 null。 */
  backup?: string | null;
}

/** Connector 逐文件合并计数。 */
export interface ConnectorFileMerge {
  file: string;
  addedKeys: number;
  keptTargetKeys: number;
  droppedDuplicateElements: number;
  changed: boolean;
}

/** Connector 合并汇总；失败时为 `{ error }`。 */
export interface ConnectorMergeSection {
  skipped: boolean;
  addedKeys: number;
  droppedDuplicateElements: number;
  changed: boolean;
  files: ConnectorFileMerge[];
  /** 改前原文的备份**目录**（该轮多个文件同处一目录）；未改写时为 null。 */
  backup?: string | null;
}

/**
 * 账号数据迁移结果。memory / connectors 两项各自独立成败：
 * 失败项为 `{ error: string }`，成功项为对应的计数对象。
 */
export interface MigrateResult {
  sourceUid: string;
  targetUid: string;
  memory?: MemoryMergeSection | { error: string };
  connectors?: ConnectorMergeSection | { error: string };
  /** 是否有任意一项实际改写了文件。 */
  changed: boolean;
}

export interface CheckinConfig {
  enabled: boolean;
  keepalive_days: number;
  lazy_refresh_hours: number;
}

export interface CheckinLog {
  ts: number;
  accountId: string | null;
  email: string;
  result: string;
  error?: string;
}

export interface CheckinResult {
  result: string;
  error?: string;
}

export interface TravelConfig {
  enabled: boolean;
}

/** 账号切换与账号列表展示配置（`~/.buddy-switch/switch_config.json`，全局单份）。 */
export interface SwitchConfig {
  /** 切换账号时默认勾选「复制会话」。默认 `false`（不改变既有切换语义）。 */
  copy_sessions_by_default: boolean;
  /** 把当前登录账号排到账号列表第一位。默认 `true`。 */
  pin_current_account: boolean;
}

export type TravelStatusLabel = "untraveled" | "no-buddy" | "traveling" | "finished";

export interface TravelStatus {
  label: TravelStatusLabel;
  rewardCredit: number | null;
  locationName?: string | null;
  arriveAt?: number | null;
}

/**
 * 定时任务的排程配置（全局单份，无需 region；也不需要「当前产品」——两个产品各占一条任务）。
 *
 * 对照 `buddy-switch-core::modules::schedule::ScheduleConfig` 的扁平序列化（`schedule_to_value`）：
 * 每类任务各有独立的 `*_hours`（0-23 的整数列表）与独立的 `*_enabled` 开关。
 */
export interface ScheduleConfig {
  /** 自动签到的小时点（0-23），可多个，如 [9, 21]。 */
  checkin_hours: number[];
  /** 派猫猫旅行的小时点。 */
  travel_hours: number[];
  /** 活跃地图上报的小时点。 */
  activity_hours: number[];
  /** token 保活的小时点。 */
  keepalive_hours: number[];
  /** 开学季任务的小时点。 */
  school_hours: number[];
  /** 夜猫子（猫猫领取）的小时点。 */
  cat_hours: number[];
  /** Trae 分区自动签到的小时点（第二条产品线，签的是 Trae 自己的区域账号库）。 */
  trae_checkin_hours: number[];
  /** 自动签到开关（签 WorkBuddy 的账号）。 */
  checkin_enabled: boolean;
  /** 派猫猫旅行开关。 */
  travel_enabled: boolean;
  /** 活跃地图上报开关。 */
  activity_enabled: boolean;
  /** token 保活开关。 */
  keepalive_enabled: boolean;
  /** 开学季任务开关。 */
  school_enabled: boolean;
  /** 夜猫子任务开关。 */
  cat_enabled: boolean;
  /**
   * Trae 自动签到开关（签 Trae 的区域账号库）。
   *
   * ⚠️ **默认 `false`**（后端的默认值，与本产品其他六类不同）：它是本产品新增的能力，
   * 且会对用户没授权过的外部服务发请求 —— 默认打开等于升级后凭空拿凭据去签到。
   * 界面因此**不得**把它显示成默认开启。
   */
  trae_checkin_enabled: boolean;
  /** 活跃上报每账号每天的对话次数（后端将 0 / 负数归一为 1）。 */
  activity_report_count: number;
}

/**
 * 「立即执行」某一类定时任务的结果（`POST /api/schedule/run`）。
 *
 * 形状随任务而异：签到 / 活跃上报 / 保活按 region 逐段返回；旅行返回派出与领取两段；
 * 开学季 / 夜猫子只返回单段结果。各段即业务模块自身的返回，原样透传便于定位到具体账号。
 */
export interface ScheduleRunResult {
  /** 被执行的任务标识（与 `SCHEDULE_TASKS` 的 key 一致）。 */
  task: string;
  /** 按 region 逐段的结果（签到 / 活跃上报 / 保活）。 */
  regions?: Array<Record<string, unknown>>;
  /** 旅行派出一段的结果。 */
  dispatched?: Record<string, unknown>;
  /** 旅行领取一段的结果。 */
  claimed?: Record<string, unknown>;
  /** 单段结果（开学季 / 夜猫子）。 */
  result?: Record<string, unknown>;
}

export interface AutoRotateConfig {
  enabled: boolean;
  check_interval_minutes: number;
  cooldown_minutes: number;
  min_gap_hours: number;
  min_urgency_hours: number;
  active_guard_minutes: number;
  min_remaining_credits: number;
}

export interface RotateLog {
  ts: number;
  action: string;
  reason?: string | null;
  from?: { id: string; name?: string | null } | null;
  to?: { id: string; name?: string | null } | null;
}

export interface RotateStatus {
  config: AutoRotateConfig;
  cliConfigured: boolean;
  activeAccountId: string | null;
  activeAccountName: string | null;
  lastCheckAt: number | null;
  lastSwitchAt: number | null;
}

export interface CreditResource {
  packageCode: string | null;
  packageName: string | null;
  total: number;
  remaining: number;
  used: number;
  status: number | null;
  expireAt: number | null;
  expired: boolean;
  expiringSoon: boolean;
}

export interface CreditExpiry {
  ok: boolean;
  accountId?: string | null;
  accountName?: string;
  updatedAt?: number;
  totalCapacity?: number;
  totalRemaining?: number;
  expiringSoonRemaining?: number;
  expiredRemaining?: number;
  soonestExpireAt?: number | null;
  expiringSoon?: boolean;
  expired?: boolean;
  resources?: CreditResource[];
  error?: string;
  /**
   * **机器可读**的失败原因（后端 `credits::ENCRYPTED_CREDENTIAL_REASON`）。
   *
   * `"encrypted_credential"` = 该账号的凭据是客户端 5.6 的加密信封，我方解不开
   * ⇒ 签到 / 积分 / Token 统计都做不了，只能用「切换账号」。
   *
   * ⚠️ 判据必须用这个字段，**不要**去匹配 `error` 的中文文案 ——
   * 文案一改，界面上「该怎么办」的引导就静默消失，且不会变红。
   */
  reason?: string | null;
}

export interface CreditStatsSummary {
  currentRemaining: number;
  currentCapacity: number;
  usageToday: number;
  usage7Days: number;
  usageThisMonth: number;
  todayCheckedInAccounts: number;
  todaySuccess: number;
  todayAlready: number;
  todayFailed: number;
}

export interface CreditStatsDailyPoint {
  date: string;
  usage: number;
  /** 官方用量按模型聚合（全量，不受请求明细条数限制）；本地观察口径下为空 */
  models?: { model: string; requestCount: number; credit: number }[];
}

export interface CreditStatsAccount {
  accountId: string;
  accountName: string;
  isCurrent: boolean;
  currentRemaining: number | null;
  totalCapacity: number | null;
  lastSnapshotAt: number | null;
  usageToday: number;
  usage7Days: number;
  usageThisMonth: number;
  checkedInToday: boolean | null;
  checkinStatusToday: string | null;
  lastCheckinAt: number | null;
  lastCheckinResult: string | null;
  /** 按账号的逐日观察消耗（缺省兼容旧后端）；官方可用时趋势图优先使用官方 daily */
  daily?: CreditStatsDailyPoint[];
  /** 账号归属版本（后端注入；合并视图据此渲染归属徽标） */
  region?: Region;
}

export interface CreditStatsUsageEvent {
  kind: "usage";
  ts: number;
  date: string;
  accountId: string;
  accountName: string;
  amount: number;
}

export interface CreditStatsCheckinEvent {
  kind: "checkin";
  ts: number;
  date: string;
  accountId: string | null;
  accountName: string;
  result: string;
  error?: string | null;
}

export type CreditStatsEvent = CreditStatsUsageEvent | CreditStatsCheckinEvent;

export type CreditOfficialUsageStatus = "complete" | "partial" | "unavailable";

export interface CreditOfficialUsageSummary {
  usageToday: number;
  usage7Days: number;
  usageThisMonth: number;
}

export interface CreditOfficialUsageModel {
  model: string;
  requestCount: number;
  credit: number;
}

export interface CreditOfficialUsageAccount {
  accountId: string;
  accountName: string;
  ok: boolean;
  requestCount: number;
  detailTruncated: boolean;
  usageToday: number | null;
  usage7Days: number | null;
  usageThisMonth: number | null;
  error?: string | null;
  reportedTotal?: number | null;
  fetchedCount?: number;
  /** 缺省兼容旧后端响应。 */
  models?: CreditOfficialUsageModel[];
  /** 按账号的逐日官方消耗（全量聚合，不受 requests 明细上限影响；缺省兼容旧后端） */
  daily?: CreditStatsDailyPoint[];
}

export interface CreditOfficialUsageRequest {
  accountId: string;
  accountName: string;
  requestId: string;
  credit: number;
  model: string;
  client: string;
  requestTime: string;
}

export interface CreditOfficialUsageError {
  accountId: string;
  accountName: string;
  error: string;
}

export interface CreditOfficialUsage {
  status: CreditOfficialUsageStatus;
  rangeStart: string;
  rangeEnd: string;
  /** 官方用量最近一次采集时间；缓存命中时保持采集当时的时间。 */
  collectedAt?: number;
  summary: CreditOfficialUsageSummary;
  daily: CreditStatsDailyPoint[];
  accounts: CreditOfficialUsageAccount[];
  requests: CreditOfficialUsageRequest[];
  /** 官方全部有效请求按模型汇总；不受 requests 明细上限影响。 */
  models?: CreditOfficialUsageModel[];
  detailLimitPerAccount: number;
  errors: CreditOfficialUsageError[];
}

export interface CreditStatistics {
  /** 查询范围（后端返回：cn / global / all）；缺省兼容旧后端。 */
  region?: RegionFilter;
  generatedAt: number;
  retentionDays: number;
  coverageStartAt: number | null;
  summary: CreditStatsSummary;
  daily: CreditStatsDailyPoint[];
  accounts: CreditStatsAccount[];
  events: CreditStatsEvent[];
  /** 官方接口不可用时仍使用上述本地观察字段；缺省兼容旧后端。 */
  officialUsage?: CreditOfficialUsage;
}

export interface TokenStatsTotals { total: number; input: number; output: number; cacheRead: number; cacheWrite: number; uncachedInput: number; records: number; cacheHitRate: number | null; }
export interface TokenStatsGroup extends TokenStatsTotals { key: string; title?: string | null; project?: string; sessionId?: string; }
export interface TokenStatsSource { source: "workbuddy" | "codebuddy-cli" | "codebuddy-ide" | "workbuddy-ai"; summary: TokenStatsTotals; models: TokenStatsGroup[]; projects: TokenStatsGroup[]; sessions: TokenStatsGroup[]; daily: TokenStatsGroup[]; /** Optional model-specific daily series for trend filtering. */ dailyByModel?: Record<string, TokenStatsGroup[]>; hours: TokenStatsGroup[]; filesScanned: number; parseErrors: number; coverageStartAt?: number | null; coverageEndAt?: number | null; }
export interface TokenStatistics { /** 查询范围（后端返回：cn / global / all）；缺省兼容旧后端。 */ region?: RegionFilter; generatedAt: number; rangeDays?: number | null; sources: TokenStatsSource[]; }

export interface CodeBuddyCliStatus {
  configured: boolean;
  authMode?: "settings-env" | "api-key-helper";
  environmentOverride?: boolean;
  settingsPresent: boolean;
  helperPresent: boolean;
  helperSupportsAccountIds: boolean;
  helperCurrent?: boolean;
  migrationRequired?: boolean;
  syncPending?: boolean;
  activeIndex: number | null;
  activeAccountId: string | null;
  activeAccountName: string | null;
  accountCount: number;
  statePath: string;
}

export interface CodeBuddyCliSwitchResult {
  ok: boolean;
  configured: boolean;
  synced: boolean;
  verified?: boolean;
  authMode?: "settings-env" | "api-key-helper";
  activeIndex?: number;
  activeAccountId?: string;
  source?: string;
  skipped?: boolean;
  message?: string;
  error?: string;
}

export interface CodeBuddyCliInstallResult {
  ok: boolean;
  configured: boolean;
  helperPresent: boolean;
  helperSupportsAccountIds: boolean;
  verified?: boolean;
  authMode?: "settings-env" | "api-key-helper";
  message?: string;
  error?: string;
}

export interface GithubConfig {
  owner?: string;
  repo?: string;
  proxy?: string;
}

export interface UpdateInfo {
  ok: boolean;
  current?: string;
  latest?: string;
  latestTag?: string;
  hasUpdate?: boolean;
  releaseName?: string;
  releaseUrl?: string;
  publishedAt?: string;
  error?: string;
  message?: string;
}

/** CodeBuddy CN IDE（桌面客户端）状态；与 CodeBuddy CLI 独立。 */
export interface CodeBuddyCnIdeStatus {
  installed: boolean;
  running: boolean;
  dataDir: string | null;
  dbPath: string | null;
  dbExists: boolean;
  appPath: string | null;
  activeAccountId: string | null;
  activeAccountName: string | null;
  detectedFrom?: string;
  statePath?: string;
}

export interface CodeBuddyCnIdeSwitchResult {
  ok: boolean;
  account: string;
  accountId: string;
  dbPath?: string;
  restarted?: boolean;
  message?: string;
}

// ---------------------------------------------------------------------------
// API 网关（对应架构设计 A-3.4 / A-3.7）
// ---------------------------------------------------------------------------

/** 网关运行配置（持久化到 ~/.buddy-switch/gateway_config.json）。字段名与后端 serde 序列化一致。 */
export interface GatewayConfig {
  enabled: boolean;
  bind_addr: string;
  port: number;
  allow_non_loopback: boolean;
  log_keep: number;
  log_bodies: boolean;
  per_key_rate_limit?: number | null;
}

/**
 * 网关运行状态（gateway_status 返回，规范形状）。
 * 后端原始字段（GatewayStatusView，snake_case）经 `normalizeGatewayStatus` 归一化后填充：
 * addr 为实际监听地址（由后端 `addr` / 兼容旧字段 `bind_addr` 归一），
 * allowNonLoopback 为是否允许非回环监听（由 `allow_non_loopback` 归一）。
 */
export interface GatewayStatus {
  /** 配置中的启用开关（与 GatewayConfig.enabled 同步）。 */
  enabled: boolean;
  /** 网关进程是否实际在监听。 */
  running: boolean;
  /** 实际监听地址，如 "127.0.0.1" / "0.0.0.0"。 */
  addr: string;
  /** 实际监听端口。 */
  port: number;
  /** 是否允许非回环（局域网）监听。 */
  allowNonLoopback: boolean;
  /** 后端返回的 Base URL（源自 snake_case `base_url`，如 http://127.0.0.1:57891/v1），优先使用。 */
  baseUrl?: string;
  /**
   * **最近一次请求失败**的可读原因（源自 snake_case `last_error`；`null` = 还没失败过）。
   *
   * 与 Trae 的 `TraeGatewayStatus.lastError` 同名同义 —— 两侧网关卡展示同一块信息。
   * （原先这里有个 `error` 字段，后端**从不产出**、前端**从不读取**，已删。）
   */
  lastError?: string | null;
  /** 账号池五态计数，**按版本分家**（键恒为 `cn` / `global`）。 */
  pools?: Record<Region, GatewayPoolSummary>;
  /** 逐账号状态（含 `realm`，前端按版本过滤）。 */
  accounts?: GatewayPoolAccount[];
  /** 逐账号「为什么不能路由」，供排查「没有可用账号」。 */
  diagnose?: string[];
}

/**
 * 账号池五态计数（**WorkBuddy 与 Trae 共用**：同一张 `AccountPoolCard` 读它）。
 *
 * 五态互斥且穷尽。`expired` 只有 Trae 侧会产出 —— WorkBuddy 池不存积分有效期，
 * 没有判据就不造数，卡片也因此不渲染该格（见 `AccountPoolCard` 的 `tiles`）。
 */
export interface GatewayPoolSummary {
  total: number;
  available: number;
  cooling: number;
  disabled: number;
  expired: number;
  zeroCredits: number;
  totalCredits: number;
}

/** 池内单个账号的可路由状态（两侧同形）。 */
export interface GatewayPoolAccount {
  uid: string;
  name: string;
  status: "available" | "cooling" | "disabled" | "expired" | "no_credits";
  /** 剩余积分；`null` = 尚未刷新过（未知），显示为「—」。 */
  credits: number | null;
  cooldownReason?: string | null;
  cooldownUntil?: number | null;
  cooling?: boolean;
  disabled?: boolean;
  /** 仅 WorkBuddy：所属版本，前端据此把一个池拆成两张卡。 */
  realm?: Region | "unknown";
  /** 仅 Trae：积分到期时刻。 */
  creditsExpireAt?: number | null;
  /** 仅 Trae：设备标识掩码。 */
  deviceIdMasked?: string | null;
}

/** API Key 记录（列表脱敏返回，绝不含明文）。字段名与后端 masked() 输出一致。 */
export interface ApiKeyRecord {
  id: string;
  name: string;
  region: Region;
  /** 可辨识前缀，如 "sk-wb-a1b2"。 */
  prefix: string;
  createdAt: number;
  revokedAt: number | null;
  revoked: boolean;
  lastUsedAt: number | null;
}

/** 创建 API Key 的返回：完整 key 仅此一次返回，之后无法再获取。 */
export interface CreateApiKeyResult {
  ok: boolean;
  /** 一次性完整明文（sk-wb- 前缀）。 */
  key?: string;
  /** 创建后可直接加入列表的脱敏记录。 */
  record?: ApiKeyRecord;
  error?: string;
}

/** 模型目录来源标注。 */
export type CatalogSource = "live" | "cached" | "builtin";

export interface CatalogModel {
  id: string;
  name: string;
  context_window: number;
  max_tokens: number;
  supports_images: boolean;
  credits?: string | null;
  badges: string[];
  free: boolean;
}

export interface CatalogSnapshot {
  region: Region;
  source: CatalogSource;
  fetched_at: number | null;
  models: CatalogModel[];
  /** 降级说明（如「上游接口可能已变更」）。 */
  note?: string | null;
}

/**
 * 模型行（P11 §3.4）= 上游清单（**派生** `available`）∪ 用户偏好（**存储** `disabled`）。
 *
 * ## 两个维度正交（P11 的灵魂）
 *
 * - `available` 是**上游事实**（这个模型事实上能不能调），**派生、不落库**：
 *   WorkBuddy 侧「在清单里即 `true`」（无「非可服务」概念）；
 *   Trae 侧 = 「该模型 ∈ 网关对外清单（`servable_entries`）」。
 * - `disabled` 是**用户偏好**（用户主动把它从暴露清单里拿掉），来自后端独立存储
 *   （按 region 分家，与 `/v1/models` 过滤、调用拒绝**同源**）。
 *
 * 两者**不得压成一个字段** —— 压了之后「上游不可用」与「你自己禁用了」就分不清
 * （四组合：可用·启用 / 可用·禁用 / 不可用·启用 / 不可用·禁用）。
 *
 * ⚠️ **不新增 `available` 到 `CatalogModel`**：它是派生，落库会引入第二份真相。
 */
export interface GatewayModelRow {
  /** 模型 id（调用时真正要传的名字）。 */
  id: string;
  /** 展示名。 */
  name: string;
  /** 归属作用域。**UI 只渲染当前 region（决策 #1）⇒ 不渲染「归属」列**；
   *  保留字段供按 region 键控与未来跨 scope 渲染。 */
  scope: Scope;
  /** 上游事实：能不能调（**派生**，不落库）。WB 恒 `true`；Trae = ∈ servable_entries。 */
  available: boolean;
  /** 用户偏好：是否被禁用（来自后端存储）。 */
  disabled: boolean;
}

/**
 * 暴露计数：`available && !disabled` —— 与出口 ①（`GET /v1/models`）**同源**。
 *
 * 出口 ① 的语义是「**可用的**模型清单」：不可服务（`available=false`）与被禁用的
 * 都**不出现**。因此这里算的 `served` 就是外部客户端在 `/v1/models` 实际会看到的条数。
 *
 * ⚠️ 口径**只能有一处**：本函数是唯一实现，两个模型页（WB / Trae）都调它 ——
 * 页面里各写一份 `filter(...).length` 就是「计数与出口 ① 会漂移」的种子。
 */
export function servedCount(rows: GatewayModelRow[]): { served: number; total: number } {
  let served = 0;
  for (const row of rows) {
    if (row.available && !row.disabled) served += 1;
  }
  return { served, total: rows.length };
}

/** 账号选择策略（按 region 各自独立配置）。 */
export type AccountStrategy =
  | { kind: "current" }
  | { kind: "pinned"; account_id: string }
  | { kind: "max_credits" };

/** 某 region 的策略 + 当前选用账号（selected 与 AccountMeta 同构）。 */
export interface AccountStrategyView {
  region?: Region;
  strategy: AccountStrategy;
  selected: AccountMeta | null;
  error?: string | null;
  note?: string | null;
}

/** 各 region 策略视图。 */
export type AccountStrategyMap = Record<Region, AccountStrategyView>;

/** 网关请求日志（仅元数据；默认不记录正文）。字段名与后端 logging 输出一致。 */
export interface GatewayLogEntry {
  ts: number;
  endpoint: string;
  method: string;
  region: Region;
  account: string | null;
  model: string | null;
  status: number;
  latencyMs: number;
  promptTokens?: number | null;
  completionTokens?: number | null;
  stream: boolean;
}

// ── P7 全局备份：导入预览 / 结果 / 自动备份（与后端 `backup.rs` 同源）─────────────

/**
 * 冲突策略：命中同 uid 时怎么办（与后端 `ConflictPolicy` 同源）。
 *
 * ⚠️ 本期 UI **只给三种批量策略**（§7 #5）：`keepLocal`（默认，D16）/ `overwrite` /
 * `keepBoth`。后端枚举里还有一个 `Ask`（逐条询问），但**不渲染**入口 ⇒ 用户侧恒取默认。
 */
export type BackupConflictPolicy = "keepLocal" | "overwrite" | "keepBoth";

/** 导入预览的逐 scope 行（`rejected` = 未知 scope 被整体拒绝，fail-closed）。 */
export interface ImportPreviewScopeRow {
  scope: string;
  added: number;
  overwritten: number;
  skipped: number;
  conflicts: number;
  /** 未知 scope ⇒ 该分区整体拒绝（不静默跳过、不回落，§1.6）。 */
  rejected?: boolean;
  error?: string;
}

/** 导入预览（**不落盘**；供安全阀展示，§1.7）。 */
export interface ImportPreview {
  formatVersion: number;
  exportedAt: string;
  includesCredentials: boolean;
  scopes: ImportPreviewScopeRow[];
  /** 将被覆盖的配置项（签到 / 旅行 / 轮换 / 切换）。 */
  configOverwrites: string[];
  /** 不含凭据 ⇒ 需重新登录的账号数（断言）。 */
  needRelogin: number;
}

/** 导入结果（部分失败态：失败计数 + 可重试 + 快照路径）。 */
export interface ImportSummary {
  perScope: ImportPreviewScopeRow[];
  failed: number;
  retryable: number;
  /** 导入前强制快照的路径（D18；可「从自动备份恢复」）。 */
  snapshotPath: string;
}

/** 自动备份 / 导入前快照的一条记录。 */
export interface AutoBackupEntry {
  fileName: string;
  path: string;
  sizeBytes: number;
  modifiedAt: number;
  kind: "auto" | "preImport" | "other";
}

/** 自动备份配置（**默认开**；可在设置里关掉，§7 #7）。 */
export interface AutoBackupConfig {
  enabled: boolean;
}

/**
 * P8 · 一个会话的**内容指纹**（组状态派生的数据源）。
 *
 * ⚠️ 这是**内容级**判据 —— **不得**用 `updatedAt` 派生「落后 / 分叉」
 * （`P8-ARCH.md` §1.2：「时间新 ≠ 内容新」）。
 */
export interface SessionFingerprint {
  /** **完整**行数（≈ 消息数：jsonl 每行一条 `type:"message"`）。 */
  lineCount: number;
  /** 前 N 行（后端上限 512）的逐行 hash —— **不含正文**，仅用于算公共前缀。 */
  prefixHashes: number[];
}

/**
 * P12 · C 路（base URL 重定向）的状态视图。
 *
 * ⚠️ `verified` **恒为 `false`** —— 应用**不校验**代理是否真的生效（只反映本地标记）。
 * `mode` 在 C 路是 `"c-redirect"`（**不是「中间人」**，不许含糊）。
 */
export interface ProxyStatus {
  enabled: boolean;
  /** 恒 `false`：应用**不校验**（前端文案必须如实，不许暗示"已验证生效"）。 */
  verified: boolean;
  mode: "c-redirect";
  /** 网关本机基址（C 路让客户端 `endpoint` 指向它）。 */
  gatewayBase: string;
  /** 白名单域（兜底行是「其它全部域名 → 直连」）。 */
  domains: string[];
  /**
   * 网关**实际转发**的业务路径前缀（P12 T03）—— 与网关 404 判定**同一份来源**。
   * ⚠️ 它**不是**给用户看的"域名表"，而是**诊断信息**（说明网关到底认哪些路径）。
   */
  bizPaths: string[];
  /** 要引导用户设置的**客户端设置键**（应用**只显示**，由用户手动设）。 */
  setting: { key: string; value: string };
}

