/**
 * Trae 网关字段归一化（与 `@/lib/gateway` 平行的第二套）。
 *
 * 为什么另起一个文件而不是扩 `gateway.ts`：两个网关的类型没有任何共用字段
 * （region / Key 库 / 目录缓存 vs 账号池 / 单一 Key），混在一个文件里只会让
 * 「这个 helper 是给谁用的」变成需要读实现才能回答的问题。
 *
 * **序列化风格**：Rust 侧 `TraeGatewayConfig` 与 `TraeGatewayStatusView` 都是
 * snake_case（与 WorkBuddy 网关刻意保持一致）。因此这里以 snake_case 为准，
 * camelCase 只作防御性兜底。
 */

import type {
  TraeGatewayAccountStatus,
  TraeGatewayConfig,
  TraeGatewayConfigRaw,
  TraeGatewayLogEntry,
  TraeGatewayPoolSummary,
  TraeGatewayStatus,
} from "@/lib/trae-types";

/** 默认配置（与 Rust `TraeGatewayConfig::default` 对齐）。 */
export const DEFAULT_TRAE_GATEWAY_CONFIG: TraeGatewayConfig = {
  enabled: false,
  bindAddr: "127.0.0.1",
  port: 57894,
  allowNonLoopback: false,
  logKeep: 200,
  logBodies: false,
  maxBodyMb: 8,
  // 与 Rust 侧 `TRAE_DEFAULT_MODEL` 同源（换名要一起改，否则「配置没加载出来」时会显示旧名）。
  defaultModel: "deepseek-v4.1-flash",
  maxRotate: 3,
  // 空串 = 不指定账号（自动择优）。
  preferredUid: "",
  // 会话粘性：与 WorkBuddy 网关同默认值（30 分钟）。
  stickyTtlMs: 30 * 60 * 1000,
  // 抓取旁路默认关（会原样落盘对话正文，属敏感内容）。
  captureEnabled: false,
  // Max Mode 默认开（与 Rust `TraeGatewayConfig::default` 同值）。
  maxMode: true,
  // ★ issue #129：账号故障时自动切换，**默认关闭**（与 Rust 默认同口径）。
  autoFailover: false,
};

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function asBoolean(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

/** 取 camelCase 优先、snake_case 兜底的字段。 */
function pick<T>(record: Record<string, unknown>, camel: string, snake: string): T | undefined {
  return (record[camel] ?? record[snake]) as T | undefined;
}

/** 归一化 `get_trae_gateway_config` 的返回。 */
export function normalizeTraeGatewayConfig(raw: unknown): TraeGatewayConfig {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const read = (camel: string, snake: string) => record[camel] ?? record[snake];
  return {
    enabled: asBoolean(record.enabled) ?? DEFAULT_TRAE_GATEWAY_CONFIG.enabled,
    bindAddr:
      asString(read("bindAddr", "bind_addr")) ?? DEFAULT_TRAE_GATEWAY_CONFIG.bindAddr,
    port: asNumber(record.port) ?? DEFAULT_TRAE_GATEWAY_CONFIG.port,
    allowNonLoopback:
      asBoolean(read("allowNonLoopback", "allow_non_loopback")) ??
      DEFAULT_TRAE_GATEWAY_CONFIG.allowNonLoopback,
    logKeep: asNumber(read("logKeep", "log_keep")) ?? DEFAULT_TRAE_GATEWAY_CONFIG.logKeep,
    logBodies:
      asBoolean(read("logBodies", "log_bodies")) ?? DEFAULT_TRAE_GATEWAY_CONFIG.logBodies,
    maxBodyMb:
      asNumber(read("maxBodyMb", "max_body_mb")) ?? DEFAULT_TRAE_GATEWAY_CONFIG.maxBodyMb,
    defaultModel:
      asString(read("defaultModel", "default_model")) ??
      DEFAULT_TRAE_GATEWAY_CONFIG.defaultModel,
    maxRotate:
      asNumber(read("maxRotate", "max_rotate")) ?? DEFAULT_TRAE_GATEWAY_CONFIG.maxRotate,
    // 空串是**合法值**（= 不指定），因此 `asString` 对空串返回 undefined 时
    // 这里必须回落到空串而不是某个「默认账号」—— 否则用户清空选择后会被写回一个
    // 他并没有指定的账号。
    preferredUid:
      asString(read("preferredUid", "preferred_uid")) ??
      DEFAULT_TRAE_GATEWAY_CONFIG.preferredUid,
    stickyTtlMs:
      asNumber(read("stickyTtlMs", "sticky_ttl_ms")) ??
      DEFAULT_TRAE_GATEWAY_CONFIG.stickyTtlMs,
    // 诊断开关没有界面入口，但必须完整往返：缺这一格 ⇒ 保存时整份覆盖
    // 会把用户手开的抓取**静默打回 false**（配置缺键即回落默认）。
    captureEnabled:
      asBoolean(read("captureEnabled", "capture_enabled")) ??
      DEFAULT_TRAE_GATEWAY_CONFIG.captureEnabled,
    maxMode:
      asBoolean(read("maxMode", "max_mode")) ?? DEFAULT_TRAE_GATEWAY_CONFIG.maxMode,
    // ★ issue #129：缺键 ⇒ `false`（老配置文件 / 老后端），与「默认关闭」一致。
    autoFailover:
      asBoolean(read("autoFailover", "auto_failover")) ??
      DEFAULT_TRAE_GATEWAY_CONFIG.autoFailover,
  };
}

/**
 * 前端形状 → 后端 snake_case。
 *
 * 后端同时接受两种写法（`#[serde(alias)]`），但**写回时统一用 snake_case**：
 * 磁盘上的配置文件应该只有一种风格，否则人工编辑时会出现「同一个键两种拼法」。
 */
export function toTraeGatewayConfigRaw(config: TraeGatewayConfig): TraeGatewayConfigRaw {
  return {
    enabled: config.enabled,
    bind_addr: config.bindAddr,
    port: config.port,
    allow_non_loopback: config.allowNonLoopback,
    log_keep: config.logKeep,
    log_bodies: config.logBodies,
    max_body_mb: config.maxBodyMb,
    default_model: config.defaultModel,
    max_rotate: config.maxRotate,
    preferred_uid: config.preferredUid,
    sticky_ttl_ms: config.stickyTtlMs,
    // ★ 必须写回：保存是整份覆盖，缺键 = 抓取被静默关闭（见 `captureEnabled` 注释）。
    capture_enabled: config.captureEnabled,
    max_mode: config.maxMode,
    auto_failover: config.autoFailover,
  };
}

function normalizePool(raw: unknown): TraeGatewayPoolSummary {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  return {
    total: asNumber(record.total) ?? 0,
    available: asNumber(record.available) ?? 0,
    cooling: asNumber(record.cooling) ?? 0,
    disabled: asNumber(record.disabled) ?? 0,
    expired: asNumber(record.expired) ?? 0,
    zeroCredits: asNumber(pick(record, "zeroCredits", "zero_credits")) ?? 0,
    totalCredits: asNumber(pick(record, "totalCredits", "total_credits")) ?? 0,
    // 「用户显式排除」的账号数（互斥第六态）。
    // 后端 `TraePool::summary` 已把它从 `available` 里扣掉 ⇒ 缺这一格就会出现
    // 「3 可用」而实际只有 2 个能被选中（数字与事实不符）。
    excluded: asNumber(record.excluded) ?? 0,
  };
}

function normalizeAccount(raw: unknown): TraeGatewayAccountStatus | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Record<string, unknown>;
  const uid = asString(record.uid);
  if (!uid) return null;
  const status = asString(record.status) ?? "available";
  return {
    uid,
    name: asString(record.name) ?? uid,
    status: status as TraeGatewayAccountStatus["status"],
    credits: asNumber(record.credits) ?? null,
    creditsExpireAt: asNumber(pick(record, "creditsExpireAt", "credits_expire_at")) ?? null,
    cooling: asBoolean(record.cooling) ?? false,
    cooldownUntil: asNumber(pick(record, "cooldownUntil", "cooldown_until")) ?? null,
    cooldownReason: asString(pick(record, "cooldownReason", "cooldown_reason")) ?? null,
    disabled: asBoolean(record.disabled) ?? false,
    // ★ 用户显式排除（互斥第六态）。**必须显式归一化**：本函数是白名单重建，
    //   漏掉一个字段就等于前端永远看不到它 —— 而「排除 / 取消排除」按钮的文案与
    //   提交值都依赖 `account.excluded`（`onExclude(uid, !account.excluded)`）。
    //   漏了它的症状不是「不显示」，而是**按钮永远显示「排除」、点下去永远传 `true`**
    //   ⇒ 已排除的账号再也取消不掉（2026-10-22 用户报障的根因，WorkBuddy 侧同源）。
    excluded: record.excluded === true,
    excludedNote:
      typeof record.excludedNote === "string" && record.excludedNote
        ? record.excludedNote
        : null,
    deviceIdMasked: asString(pick(record, "deviceIdMasked", "device_id_masked")) ?? null,
  };
}

/**
 * 归一化 `trae_gateway_status` 的返回。
 *
 * 传入非法值时返回 null，调用方回退到「未启动」空态。
 */
export function normalizeTraeGatewayStatus(raw: unknown): TraeGatewayStatus | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Record<string, unknown>;
  const accounts = Array.isArray(record.accounts)
    ? record.accounts
        .map(normalizeAccount)
        .filter((item): item is TraeGatewayAccountStatus => item !== null)
    : [];
  const diagnose = Array.isArray(record.diagnose)
    ? record.diagnose.filter((item): item is string => typeof item === "string")
    : [];
  return {
    enabled: asBoolean(record.enabled) ?? false,
    running: asBoolean(record.running) ?? false,
    addr: asString(record.addr) ?? null,
    baseUrl: asString(pick(record, "baseUrl", "base_url")) ?? "",
    bindAddr: asString(pick(record, "bindAddr", "bind_addr")) ?? "127.0.0.1",
    port: asNumber(record.port) ?? DEFAULT_TRAE_GATEWAY_CONFIG.port,
    allowNonLoopback:
      asBoolean(pick(record, "allowNonLoopback", "allow_non_loopback")) ?? false,
    version: asString(record.version) ?? "",
    totalRequests: asNumber(pick(record, "totalRequests", "total_requests")) ?? 0,
    lastError: asString(pick(record, "lastError", "last_error")) ?? null,
    apiKeyPrefix: asString(pick(record, "apiKeyPrefix", "api_key_prefix")) ?? "",
    pool: normalizePool(record.pool),
    accounts,
    diagnose,
    upstream: asString(record.upstream) ?? "",
  };
}

/** 归一化 `get_trae_gateway_logs` 的返回（按时间升序，与后端一致）。 */
export function normalizeTraeGatewayLogs(raw: unknown): TraeGatewayLogEntry[] {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const list = Array.isArray(record.logs)
    ? record.logs
    : Array.isArray(raw)
      ? (raw as unknown[])
      : [];
  return list
    .map((item): TraeGatewayLogEntry | null => {
      if (!item || typeof item !== "object") return null;
      const entry = item as Record<string, unknown>;
      return {
        ts: asNumber(entry.ts) ?? 0,
        endpoint: asString(entry.endpoint) ?? "",
        method: asString(entry.method) ?? "POST",
        account: asString(entry.account) ?? null,
        model: asString(entry.model) ?? null,
        status: asNumber(entry.status) ?? 0,
        latencyMs: asNumber(pick(entry, "latencyMs", "latency_ms")) ?? 0,
        promptTokens: asNumber(pick(entry, "promptTokens", "prompt_tokens")) ?? null,
        completionTokens:
          asNumber(pick(entry, "completionTokens", "completion_tokens")) ?? null,
        stream: asBoolean(entry.stream) ?? false,
        error: asString(entry.error) ?? null,
        /* ★ issue #76：**必须保留「键不存在」与「值为 null」的区别** ——
           `null` = 确实没有 Key 的调用，`undefined` = 升级前写入的老条目。
           用 `hasOwnProperty` 判断而不是 `asString(...) ?? null`（那会把两者都变 null，
           让「按密钥看用量」凭空多出一批匿名调用）。 */
        keyId: Object.prototype.hasOwnProperty.call(entry, "keyId")
          ? (asString(entry.keyId) ?? null)
          : undefined,
        /* 归属台账：同 `keyId` 的两种「没有」语义（`null` vs `undefined`）。 */
        keyScope: Object.prototype.hasOwnProperty.call(entry, "keyScope")
          ? (asString(entry.keyScope) ?? null)
          : undefined,
      };
    })
    .filter((item): item is TraeGatewayLogEntry => item !== null);
}

// 账号状态标签原先在本文件（`TRAE_POOL_STATUS_LABELS`），账号池卡**两侧共用**后
// 迁到 `@/lib/gateway` 的 `POOL_STATUS_LABELS`（文案键 `shared.poolStatus.*`）——
// 别在这里再建第二份，两侧文案会各自漂移。

/**
 * 归一化 `get_trae_gateway_models` 的返回 → 降级链下拉要的 `{id, name}` 列表。
 *
 * Trae 的 `/v1/models` 只给 `id`（给人看的 `displayName` 在**客户端目录**
 * `get_trae_client_models` 里，那张表本模块不解析），所以 `name` 回落成 id。
 * 与 WB / Qoder 的清单同形，降级链抽屉不必按平台分叉。
 */
export function normalizeTraeGatewayModels(raw: unknown): { id: string; name: string }[] {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  if (!Array.isArray(record.data)) return [];
  const seen = new Set<string>();
  const out: { id: string; name: string }[] = [];
  for (const item of record.data) {
    if (!item || typeof item !== "object") continue;
    const row = item as Record<string, unknown>;
    const id = asString(row.id)?.trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push({ id, name: id });
  }
  return out;
}
