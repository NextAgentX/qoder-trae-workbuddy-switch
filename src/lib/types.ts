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
  /**
   * 该账号**最近一次被实际使用**（切换 / 启动）的时刻，epoch **毫秒**。
   *
   * 口径与 `refreshedAt` / `createdAt` **完全一致**（后端 `now_ms()`，数字而非 ISO 字符串，
   * 见 `account.rs` 的 `touch_last_used_for`）；从未使用过 ⇒ `null`。
   */
  lastUsedAt?: number | null;
  needsRelogin: boolean;
  needsReloginReason: string | null;
  /**
   * 用户自填备注（例如「DS4.1 额度 · 10/03 解禁」）。
   *
   * 后端**原样透传**：未设置时为 `null` 或缺失，前端按「有值才渲染」处理。
   */
  remark?: string | null;
  /**
   * 用户自定义分组名（自由文本）。缺省 / `null` = **未分组**。
   *
   * 与 `remark` 同为**本地标注**：后端原样透传，未设置时为 `null` 或缺失。
   */
  groupName?: string | null;
  /** 该账号所属版本（后端 region 化后返回；缺省视为 "cn"）。 */
  region?: Region;
}

/**
 * 「今日 Token」里**单个账号**的用量明细（`get_account_token_today` 的一条）。
 *
 * ⚠️ **诚实边界**（UI 必须遵守，见 {@link AccountTokenTodayResponse}）：
 * 账号**缺失**于返回的 `accounts` 数组 = 「今日无记录 / 未知」；出现在这里的 `0` 是**真实 0**。
 */
export interface AccountTokenTodayEntry {
  /** 账号 uid（与 `AccountMeta.uid` 同源；`AccountMeta.uid` 为 null 的账号不会出现在此）。 */
  uid: string;
  /** **客户端自身 CLI 用量**的输入 token 数。 */
  clientIn: number;
  /** 客户端自身 CLI 用量的输出 token 数。 */
  clientOut: number;
  /** **经本网关**的输入 token 数。 */
  gatewayIn: number;
  /** 经本网关的输出 token 数。 */
  gatewayOut: number;
}

/**
 * `get_account_token_today` 的返回 —— 「今日 Token」按账号聚合（对照后端契约，camelCase）。
 *
 * ## 三条诚实边界（UI 渲染红线）
 *
 * 1. `accounts` 里**缺失**某账号 = 「今日无记录 / 未知」⇒ 必须渲染 **「—」**，**不得**当 0；
 * 2. 出现在 `accounts` 里的 `0` 是**真实 0**（当日确实没用）；
 * 3. `gatewayTruncated: true` ⇒ 网关日志已到保留上限，**网关部分只是下界** ⇒
 *    必须渲染成 **`≥N`** 并说明（tooltip / 行内注），**绝不**把网关数当完整值呈现。
 */
export interface AccountTokenTodayResponse {
  /** 本地日期键，如 `"2026-10-10"`（与签到日志 / 积分统计事件同口径）。 */
  date: string;
  /** 逐账号明细；**缺席**即「今日无记录 / 未知」。 */
  accounts: AccountTokenTodayEntry[];
  /** 会话所属账号解析不出（未知账号）的汇总。 */
  unknownClient: { in: number; out: number };
  /** 网关日志已到保留上限 ⇒ 网关部分为下界。 */
  gatewayTruncated: boolean;
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

/**
 * P8 · 安全同步（增量追加）的**一条**任务：把 `sourceCid` 的新增行追加到 `targetCid`。
 *
 * ⚠️ **方向由调用方定**：源必须是**行数更多**的那一份（落后的是目标）。
 * 后端**独立复核**「目标是不是源的严格前缀」⇒ 方向给反了会 `ok: false`，**不会写坏数据**。
 */
export interface SyncSessionReplicaItem {
  /** 源副本的会话 id（行数更多、内容更全的那一份）。 */
  sourceCid: string;
  /** 目标副本的会话 id（落后的那一份，只会在其末尾**追加**）。 */
  targetCid: string;
}

/** 单条同步的结果（**逐条**返回 ⇒ 批量时天然支持部分失败）。 */
export interface SyncSessionReplicaResult {
  sourceCid: string;
  targetCid: string;
  /** 是否成功（**已一致**也算成功，`appendedLines` 为 0）。 */
  ok: boolean;
  /** 实际追加的行数；仅 `ok` 时有意义。 */
  appendedLines?: number;
  /** 失败原因（`ok: false` 时必有）；分歧 / 找不到正文 / 读写失败都会在这里如实说明。 */
  error?: string;
}

/** `sync_session_replicas` 的返回（汇总条用 `okCount` / `failCount`）。 */
export interface SyncSessionReplicaResponse {
  results: SyncSessionReplicaResult[];
  okCount: number;
  failCount: number;
}

/**
 * P8 · 分歧处理的**一条**任务：把**用户选定保留的那一份**（`sourceCid`）覆盖到 `targetCid`。
 *
 * ⚠️ 与 {@link SyncSessionReplicaItem} **语义相反**：同步是**加法**（只追加），
 * 这里是**覆盖**（目标原有的分歧内容会被丢弃）⇒ UI **必须**二次确认。
 */
export interface UnifySessionReplicaItem {
  /** 用户选定**要保留**的那一份。 */
  sourceCid: string;
  /** 被覆盖的副本（它的分歧内容会被丢弃）。 */
  targetCid: string;
}

/** 单条统一的结果（**逐条**返回 ⇒ 批量时天然支持部分失败）。 */
export interface UnifySessionReplicaResult {
  sourceCid: string;
  targetCid: string;
  ok: boolean;
  /** 是否真的覆盖了（`false` = 本来就一致，未写盘）。 */
  overwritten?: boolean;
  /** 覆盖前目标的行数。 */
  previousLines?: number;
  /** 覆盖后的行数（= 选定那一份的行数）。 */
  totalLines?: number;
  /** ★ 用户**丢掉的**条数（目标相对共同旧版多出来的部分）—— 供 UI 如实告知。 */
  lostLines?: number;
  /** 备份路径（覆盖前目标的原文）；`null` = 未覆盖故无备份。 */
  backup?: string | null;
  /** 失败原因（`ok: false` 时必有）。 */
  error?: string;
}

/** `unify_session_replicas` 的返回（汇总条用 `okCount` / `failCount`）。 */
export interface UnifySessionReplicaResponse {
  results: UnifySessionReplicaResult[];
  okCount: number;
  failCount: number;
}

/**
 * 一条「客户端限流台账」条目（账号 × 模型 × 恢复时刻）。
 *
 * 数据源是**客户端自身落盘的三张只读表** join（见 core `client_limits.rs`）；
 * `account` / `model` 为 `null` 表示 **join 失败**（未知账号 / 未知模型）——
 * 该条**不会**被丢弃，由界面渲染「未知」。
 */
export interface ClientLimitEntry {
  /** 账号昵称（join 失败为 null）。 */
  account: string | null;
  /** 原始 `user_id`（= 云端 uid），便于排障。 */
  accountUid: string | null;
  /** 会话所选模型（join 失败为 null）。⚠️ `hy4-preview` 与 `hy4-preview-f` 是两个模型。 */
  model: string | null;
  /** 上游错误码（6004 模型级限流 / 6020 / 6022 排队 / …）。 */
  code: number | null;
  /** HTTP 状态码（多为 429）。 */
  status: number | null;
  /** 恢复时刻（毫秒）；文案漂移解析不出为 null。 */
  resetAt: number | null;
  /** 最近一次发生时刻（毫秒）。 */
  occurredAt: number;
  /** `resetAt > now` —— 界面主排序依据。 */
  active: boolean;
  /** **原始**上游文案（解析失败时保留原文，不静默丢弃）。 */
  message: string;
  /** 会话 ID（便于跳转定位）。 */
  sessionId: string;
  /** 会话工作目录（可能含真实项目路径）。 */
  cwd: string | null;
  /** `providerData.agent`（`cli` / 子代理名）。 */
  agent: string | null;
  /** 所属 region（`cn` / `global`）。 */
  region: string;
}

/** `client_limits` 的返回（只读观测）。 */
export interface ClientLimitsResponse {
  region: string;
  /** 扫描窗口毫秒下界；`null` 表示全量扫描。 */
  since: number | null;
  now: number;
  counts: { scanned: number; deduped: number; active: number };
  limits: ClientLimitEntry[];
}

/** 一个 skill 的视图（core `skill::SkillEntry`，camelCase）。 */
export interface SkillEntry {
  /** 目录名（唯一键，启停 / 导入都以它定位）。 */
  dirName: string;
  /** frontmatter `name`，缺失回落目录名。 */
  name: string;
  /** 优先 `description_zh`；缺失回落 `description`。 */
  description: string;
  version: string | null;
  /** 推导值：**非全停用即启用**。 */
  enabled: boolean;
  /** 两个启停字段只写了一半 ⇒ 状态不一致，界面给「修复」入口。 */
  inconsistent: boolean;
  agentCreated: boolean;
  /** 无合法 frontmatter ⇒ 不可启停（不擅自造 frontmatter）。 */
  hasFrontmatter: boolean;
}

/** `list_skills` 的返回。 */
export interface SkillListResponse {
  region: string;
  skills: SkillEntry[];
}

/** `set_skill_enabled` 的返回。 */
export interface SkillToggleResult {
  dirName: string;
  enabled: boolean;
  /** 本次是否真的改写了文件（幂等重跑为 false）。 */
  changed: boolean;
  /** 改前原文的备份路径；无改动 / 备份失败时为 null。 */
  backup: string | null;
}

/** `copy_skills_cross` 的返回（合并补齐报告）。 */
export interface SkillCopyReport {
  sourceRegion: string;
  targetRegion: string;
  copied: string[];
  skippedConflict: string[];
  skippedIdentical: string[];
  changed: boolean;
  backup: string | null;
}

/** `import_skill` 的返回。 */
export interface SkillImportResult {
  region: string;
  dirName: string;
  imported: boolean;
  conflict: boolean;
  identical: boolean;
}

// ---------------------------------------------------------------------------
// 上游客户端 settings.json 统一托管（core `client_settings::*`，camelCase）
// ---------------------------------------------------------------------------

/** 一个上游客户端槽位（core `client_settings::slot_view`）。 */
export interface ClientSettingsSlot {
  /** 稳定标识（前端传参 / 备份目录名）。 */
  key: string;
  /** i18n 键（模块级常量表存键不存文案，由前端渲染成当前语言）。 */
  displayKey: string;
  /** 所属产品：`workbuddy` / `trae`。 */
  product: string;
  /** 该槽位的 `settings.json` 是否存在（未安装者仍在列表内）。 */
  installed: boolean;
  /** 解析到的绝对路径；取不到基目录时为 null。 */
  path: string | null;
}

/** `list_client_settings_slots` 的返回。 */
export interface ClientSettingsSlotsResponse {
  slots: ClientSettingsSlot[];
}

/** 原文件的排版指纹。`indent = null` 表示**全文无换行**（压缩式）。 */
export interface ClientSettingsFingerprint {
  /** 首个成员行的前导空白（`"  "` / `"\t"` / `""`）；无换行 ⇒ null。 */
  indent: string | null;
  /** 文中首个换行符（`\r\n` 或 `\n`）。 */
  newline: string;
}

/** `read_client_settings` 的返回（原文 + 解析后键值 + 格式指纹）。 */
export interface ClientSettingsReadResponse {
  slot: string;
  displayKey: string;
  product: string;
  path: string;
  installed: boolean;
  /** 客户端进程是否正在运行（运行中禁止写入）。 */
  running: boolean;
  /** 原始文本；未安装为 null。 */
  text: string | null;
  /** 解析后的键值；未安装为 null。 */
  values: unknown | null;
  /** 顶层键顺序（`values` 已被 BTreeMap 排序，此表保留原顺序）。 */
  keyOrder: string[];
  /** 格式指纹；未安装为 null。 */
  fingerprint: ClientSettingsFingerprint | null;
}

/** `patch_client_settings` / `restore_client_settings_backup` 的返回。 */
export interface ClientSettingsWriteResult {
  slot: string;
  displayKey: string;
  product: string;
  path: string;
  installed: boolean;
  /** 本次是否真的改写了文件（幂等重跑为 false）。 */
  changed: boolean;
  /** 改前原文的备份路径；无改动 / 备份失败时为 null。 */
  backup: string | null;
  /** 本次 patch 涉及的键（点号路径）。 */
  keys: string[];
  /** 改写后的全文。 */
  text: string;
  /** 改写后的解析键值。 */
  values: unknown;
  /** 仅恢复时出现：来源备份点 id。 */
  restoredFrom?: string;
}

/** 一个历史备份点。 */
export interface ClientSettingsBackupEntry {
  /** 备份点 id（`utc_iso` 时间戳，可含 `-N` 去重后缀）。 */
  id: string;
  path: string;
  size: number;
}

/** `list_client_settings_backups` 的返回。 */
export interface ClientSettingsBackupsResponse {
  slot: string;
  displayKey: string;
  /** 该槽位的备份根目录。 */
  dir: string;
  backups: ClientSettingsBackupEntry[];
}

/** 比对里的一条「仅某一侧有」。 */
export interface ClientSettingsDiffOnlyEntry {
  /** 拍平后的点号路径。 */
  path: string;
  value: unknown;
}

/** 比对里的一条「值不同」。 */
export interface ClientSettingsDiffChangedEntry {
  path: string;
  a: unknown;
  b: unknown;
}

/** 代理语义组里某一侧的单个字段取值（core `merge_proxy_fields`）。 */
export interface ClientSettingsProxyFieldValue {
  /** 该槽位里承载该语义的点号路径。 */
  path: string;
  /** 该字段在该槽位的文件里是否真实存在（不存在时 `value` 为 null）。 */
  present: boolean;
  value: unknown;
}

/**
 * 跨产品比对时按 core `PROXY_FIELDS` 归并出的一个语义组（当前只有「代理」）。
 *
 * 两侧字段路径本就不同（WorkBuddy 是 `http.proxy` + `http.proxySupport`，
 * Trae 是 `trae.network.proxy.effectiveMode`），故 `same` 按**有效值集合**判定。
 * 归并进来的路径**不再**出现在 `onlyA` / `onlyB` / `different` 里。
 */
export interface ClientSettingsProxyGroup {
  /** 语义名（当前恒为 `proxy`）。 */
  semantic: string;
  a: ClientSettingsProxyFieldValue[];
  b: ClientSettingsProxyFieldValue[];
  /** 两侧有效值集合是否相同。 */
  same: boolean;
}

/** `diff_client_settings` 的返回（仅 A 有 / 仅 B 有 / 值不同）。 */
export interface ClientSettingsDiffResponse {
  onlyA: ClientSettingsDiffOnlyEntry[];
  onlyB: ClientSettingsDiffOnlyEntry[];
  different: ClientSettingsDiffChangedEntry[];
  counts: { onlyA: number; onlyB: number; different: number; same: number };
  /** 跨产品比对时归并出的代理语义组；同产品为空数组。 */
  proxyGroups: ClientSettingsProxyGroup[];
  slotA: { key: string; displayKey: string; path: string; installed: boolean };
  slotB: { key: string; displayKey: string; path: string; installed: boolean };
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
  /** 本次签到可得额度（逐条「+N」的 N）；上游没给时为 `null` ⇒ 显示「—」。 */
  amount?: number | null;
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

/**
 * VS Code 内 CodeBuddy 扩展（`tencent-cloud.coding-copilot`）状态。
 *
 * 与 [`CodeBuddyCnIdeStatus`] **刻意分开**：一个是桌面客户端，一个是编辑器里的扩展，
 * 「装了哪个」是两件独立的事（共用加解密流程，但探测目标不同）。
 */
export interface VscodeCodebuddyPluginStatus {
  /** VS Code 用户数据目录是否存在（≠ 扩展已安装）。 */
  installed: boolean;
  /** 扩展是否已安装：`<数据目录>/User/globalStorage/<extensionId>` 存在。 */
  extensionInstalled: boolean;
  running: boolean;
  /** `state.vscdb` 里是否存在会话 secret 行（**只查 key、不解密**，仅用于文案）。 */
  loggedIn: boolean;
  dataDir: string | null;
  dbPath: string | null;
  dbExists: boolean;
  activeAccountId: string | null;
  activeAccountName: string | null;
  detectedFrom?: string;
  statePath?: string;
}

/** JetBrains IDE 里一个受支持产品的配置目录（IDEA / PyCharm，含社区版）。 */
export interface JetbrainsTargetStatus {
  /** 配置目录名，如 `PyCharm2026.2`（= 切换时的选择键）。 */
  configDir: string;
  pluginInstalled: boolean;
  running: boolean;
  loggedIn: boolean;
  secretPath: string;
}

/**
 * JetBrains IDE 内 CodeBuddy 插件状态。
 *
 * ⚠️ 与 VS Code 不同：本机**可能同时装多个 IDE**（IDEA + PyCharm），
 * 一次切换会写入**全部装了插件**的产品 ⇒ 状态是「多目标」的（`targets`）。
 */
export interface JetbrainsCodebuddyPluginStatus {
  /** 配置根下存在任一受支持产品的配置目录。 */
  installed: boolean;
  /** 其中至少一个装了 CodeBuddy 插件。 */
  pluginInstalled: boolean;
  running: boolean;
  loggedIn: boolean;
  configRoot: string | null;
  targets: JetbrainsTargetStatus[];
  activeAccountId: string | null;
  activeAccountName: string | null;
  detectedFrom?: string;
  statePath?: string;
}

/**
 * 两个 CodeBuddy **插件**程序位（VS Code / JetBrains）的切换结果。
 *
 * 后端两条命令返回同一个形状（`programs::switch` 统一出口）⇒ 前端也只定义一份，
 * 避免两处抄写漂移。`dbPath`（VS Code）与 `written`（JetBrains）各自可缺省。
 */
export interface CodebuddyPluginSwitchResult {
  ok: boolean;
  account: string;
  accountId: string;
  dbPath?: string;
  /** JetBrains：本次实际写入的配置目录名列表。 */
  written?: string[];
  restarted?: boolean;
  closedByUs?: boolean;
  /** VS Code：写入前是否已存在会话（用于文案）。 */
  existingSession?: boolean;
  message?: string;
}

/** 两个 CodeBuddy 插件程序位的「从本机探测当前登录」结果。 */
export interface CodebuddyPluginDetectResult {
  ok: boolean;
  /** 本机是否读到该插件的登录 secret。 */
  found: boolean;
  /** 读到了且能在账号库里匹配到账号。 */
  matched?: boolean;
  accountId?: string;
  account?: AccountMeta;
  uid?: string | null;
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
  /**
   * ★ E3：尾部 4 位（如 "9f3c"）。列表显示 `prefix••••••••suffix`。
   * 旧台账无此字段 ⇒ 空串 ⇒ 展示层**省略尾部**（不报错、不补造）。
   */
  suffix?: string;
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
  /**
   * ★ C1：本行是否**可重试**。未知 scope 为 `false`（键名非法是来件的结构性缺陷，
   * 重试同一个文件必然再失败）；合并 / 写入失败为 `true`。
   */
  retryable?: boolean;
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
  /** 可重试的 scope 数（= `retryableScopes.length`）。 */
  retryable: number;
  /**
   * ★ C1：**可重试的具体 scope id**，原样回传给 `importBackup` 的 `onlyScopes`
   * 即可「只重试失败项」。未知 scope（键名非法）**不在**此列——重试必然再失败。
   */
  retryableScopes?: string[];
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
  /**
   * ★ E1：开启态 KPI（`P12-ARCH.md` §3.2 契约）。
   *
   * ⚠️ **诚实边界**：`known === false` 表示「台账读不到 / 今天还没记过」——
   * 此时 `todayRequests` / `switches` 是 `null`，界面**必须显示「—」，不得当 0**。
   * 0 的含义是「今天确实一次都没走过」，与「不知道」是两件事。
   */
  stats?: {
    todayRequests: number | null;
    switches: number | null;
    /** 恒 `0`：网关只记元数据、**不落盘请求正文**（D29 承诺，不是统计值）。 */
    persistedBodies: 0;
    known: boolean;
  };
}

