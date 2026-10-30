/**
 * Qoder 网关字段归一化（与 `@/lib/gateway`、`@/lib/trae-gateway` 平行的第三套）。
 *
 * ## 为什么又是独立一份，而不是扩既有两个
 *
 * 与 `trae-gateway.ts` 的模块头同一条理由：三套网关的类型**没有任何共用字段**
 * （region / 程序位 / Key 库形状都不同），混在一个文件里只会让「这个 helper 是给谁用的」
 * 变成需要读实现才能回答的问题。
 *
 * ## 序列化风格
 *
 * Rust 侧 `QoderGatewayConfig` 与状态视图都是 **snake_case**（与 WB / Trae 刻意一致），
 * 因此这里以 snake_case 为准，camelCase 只作防御性兜底。
 *
 * ## 一个 Qoder 特有的形状：**一个监听、两个区域**
 *
 * 状态视图里 `enabled` / `port` / `running` 是两个区域**共用**的事实（同一个监听），
 * 只有 `pool` / `accounts` / `diagnose` / `api_key_prefix` 按 `scope` 分家
 * —— 因此「哪本账号库」由**请求参数**决定，响应体里刻意没有 `region` 字段。
 */

import type { GatewayPoolSummary } from "@/lib/types";
import type { TraeGatewayLogEntry } from "@/lib/trae-types";
import { normalizeTraeGatewayLogs } from "@/lib/trae-gateway";

/** 默认配置（与 Rust `QoderGatewayConfig::default` 对齐；默认**关**，与另两条产品线同口径）。 */
export const DEFAULT_QODER_GATEWAY_CONFIG: QoderGatewayConfig = {
  enabled: false,
  bindAddr: "127.0.0.1",
  port: 57895,
  allowNonLoopback: false,
  logKeep: 200,
  logBodies: false,
  maxBodyMb: 0,
  // 空串 = 由清单第一条兜底（不硬编码一个可能不存在的模型名）。
  defaultModel: "",
  maxRotate: 3,
  // 空串 = 不指定账号（自动择优 / 会话粘性）。
  preferredUid: "",
  stickyTtlMs: 30 * 60 * 1000,
  maxContext: true,
  // 护栏默认全关（与 Rust `QoderGatewayConfig::default` 同口径）。
  reserveCredits: 0,
  dailyTokenLimit: 0,
  dailyCreditLimit: 0,
  modelDailyTokenLimit: 0,
  // ★ issue #129：账号故障时自动切换，**默认关闭**（与 Rust 默认同口径）。
  autoFailover: false,
};

/** 网关配置（前端形状）。 */
export interface QoderGatewayConfig {
  enabled: boolean;
  bindAddr: string;
  port: number;
  allowNonLoopback: boolean;
  logKeep: number;
  logBodies: boolean;
  maxBodyMb: number;
  defaultModel: string;
  maxRotate: number;
  preferredUid: string;
  stickyTtlMs: number;
  /** 上下文上限策略（默认 true）。 */
  maxContext: boolean;
  /** G1 余额地板（`<=` 即停；`0` = 关闭）。 */
  reserveCredits: number;
  /** G2 当日 token 上限（逐账号；`0` = 关闭）。 */
  dailyTokenLimit: number;
  /** G3 当日 credit 上限（浮点；`0` = 关闭）。 */
  dailyCreditLimit: number;
  /** G4 单模型当日 token 上限（`0` = 关闭）。 */
  modelDailyTokenLimit: number;
  /**
   * ★ issue #129：**账号故障时自动切换**（默认 `false`）。
   *
   * 开启后，流中途的账号级失败不再被「补终止标记」伪装成成功，而是先发一条
   * **可重试**的错误帧（含 `HTTP 429` / `HTTP 502`）；失败账号同时写冷却、解绑粘性
   * ⇒ 客户端整步重跑时必然落到另一个号。
   */
  autoFailover: boolean;
}

/** 配置的线上形状（snake_case；写回时用）。 */
export interface QoderGatewayConfigRaw {
  enabled: boolean;
  bind_addr: string;
  port: number;
  allow_non_loopback: boolean;
  log_keep: number;
  log_bodies: boolean;
  max_body_mb: number;
  default_model: string;
  max_rotate: number;
  preferred_uid: string;
  sticky_ttl_ms: number;
  /** 上下文上限策略（默认 true）：清单声明最大窗口，false ⇒ 默认窗口。 */
  max_context?: boolean;
  /** G1 余额地板（`0` = 关闭）。 */
  reserve_credits: number;
  /** G2 当日 token 上限（`0` = 关闭）。 */
  daily_token_limit: number;
  /** G3 当日 credit 上限（浮点；`0` = 关闭）。 */
  daily_credit_limit: number;
  /** G4 单模型当日 token 上限（`0` = 关闭）。 */
  model_daily_token_limit: number;
  /** ★ issue #129：账号故障时自动切换。 */
  auto_failover: boolean;
}

/**
 * 池计数（共用卡的 `GatewayPoolSummary` + Qoder 独有的一格 `modelCooling`）。
 *
 * 直接**扩展**共用形状（而不是另立一份同名字段）：卡片读的就是 `GatewayPoolSummary`，
 * 少一个字段会在传参处报类型错（真踩过）——扩展后两个平台各自多带自己的格，
 * 缺的格由卡片按 0 兜底。
 */
export interface QoderGatewayPoolSummary extends GatewayPoolSummary {
  /**
   * 账号本身可用、但有**模型级**冷却的条目数。
   *
   * Qoder 上游的 429 是**按模型**报的 ⇒ 一个账号可能「对 A 模型冷却、对 B 模型仍可用」。
   * 这一格与 `cooling`（账号级）正交，不是它的子集。
   */
  modelCooling: number;
}

/** 一条池内账号（与共用池卡 `GatewayPoolAccount` 同形）。 */
export interface QoderGatewayPoolAccount {
  uid: string;
  name: string;
  /**
   * 池内状态。**`"excluded"` 是互斥第六态** —— 用户显式排除后该号不再参与选号。
   *
   * ⚠️ 不可省略：归一化时若把 `"excluded"` 落回 `"available"`，界面会显示
   * 「可用」而实际选号永远不选它（数字与事实不符）。
   */
  status: "available" | "cooling" | "disabled" | "excluded";
  /** Qoder 池不建模积分 ⇒ 恒 `null`（卡片显示「—」）。 */
  credits: number | null;
  cooling: boolean;
  cooldownUntil: number | null;
  cooldownReason: string | null;
  /** `true` = **需人工干预**（无凭据 / 凭据失效），「恢复」出口据此渲染。 */
  disabled: boolean;
  /**
   * **用户显式排除**（硬约束：选号 / 兜底 / 偏好全跳过）。
   *
   * 「排除 / 取消排除」按钮的文案与提交值都依赖它
   * （`onExclude(uid, !account.excluded)`）—— 缺了它，按钮永远显示「排除」、
   * 点下去永远传 `true` ⇒ 排除后再也取消不掉（2026-10-22 用户报障）。
   */
  excluded: boolean;
  /** 排除备注（为什么排除）。 */
  excludedNote: string | null;
}

/** 状态视图。 */
export interface QoderGatewayStatus {
  enabled: boolean;
  running: boolean;
  addr: string | null;
  baseUrl: string;
  bindAddr: string;
  port: number;
  allowNonLoopback: boolean;
  version: string;
  totalRequests: number;
  lastError: string | null;
  apiKeyPrefix: string;
  pool: QoderGatewayPoolSummary;
  accounts: QoderGatewayPoolAccount[];
  diagnose: string[];
  /** 本区域的官方候选主机（按顺序）。 */
  upstream: string;
}

/** 清单来源三级（与后端 `CatalogView::source` 逐字对齐）。 */
export type QoderCatalogSource = "live" | "cached" | "unavailable";

/** 一条上游模型（管理面全量清单，**未过滤**禁用的）。 */
export interface QoderGatewayModelRow {
  id: string;
  /** 展示名（后端 `display_name`，缺失时回落 id）。 */
  name: string;
  isReasoning: boolean;
  maxInputTokens: number | null;
  /** 两档原值（上下文开关本地即时切换用）。 */
  contextDefault: number | null;
  contextMax: number | null;
  /** 后端现算的一份禁用标记（渲染以 store 为准；这里用于交叉校验与计数）。 */
  disabled: boolean;
  /**
   * 官方 `price_factor`（计价倍率，**不是**金额）；缺失 = 官方没给，不编。
   */
  priceFactor: number | null;
  /** 官方标记的免费模型。 */
  isFree: boolean;
  /** 错峰活动徽标（中文优先、缺失回落英文）；无活动 = `null`。 */
  promotionBadge: string | null;
  /** 错峰时段（`"22:00-08:00"`）；无活动 = `null`。 */
  promotionWindow: string | null;
  /** 思考档位（官方词表顺序）；空数组 = 该模型不支持选档位。 */
  efforts: string[];
}

/** 模型清单视图（含来源与原因）。 */
export interface QoderGatewayCatalog {
  region: string;
  source: QoderCatalogSource;
  /** 上次成功拉取时刻（毫秒）；`unavailable` 时 `null`。 */
  fetchedAt: number | null;
  /** 拉取失败 / 无账号时的可读原因；`live` 时 `null`。 */
  note: string | null;
  rows: QoderGatewayModelRow[];
}

/** 一条 API Key（列表脱敏返回；**永不含明文与哈希**）。 */
export interface QoderApiKeyRecord {
  id: string;
  name: string;
  /** 归属区域：`"cn"` / `"global"`。 */
  region: string;
  prefix: string;
  suffix: string;
  createdAt: number;
  revokedAt: number | null;
  revoked: boolean;
  lastUsedAt: number | null;
  /* ── 策略（issue #76）—— 与 `ApiKeyRecord` / `TraeApiKeyRecord` 同形同义，
     前端共用一套构件。全部可选 ⇒ 旧台账零迁移。 ── */
  quotaMode?: "unlimited" | "limited";
  quotaLimit?: number;
  quotaUnit?: "token" | "request";
  quotaPeriod?: "none" | "daily" | "weekly" | "monthly";
  scopeMode?: "all" | "selected";
  scopeAccounts?: string[];
  scopeGroups?: string[];
}

/**
 * 请求日志条目 —— 形状与 Trae / WB **逐字相同**（字段名是三平台刻意对齐的，
 * 见 Rust 侧三处 `record_request` 的注释）⇒ 直接复用 Trae 侧的类型与归一化，
 * 不另立第二份（第二份必然漂移）。
 *
 * ⚠️ Qoder 的日志条目**多一个 `region`**（一个监听记两个区域的请求）：
 * 这里用交叉类型附加，不放进共用形状（Trae / WB 的归一化不产出它）。
 */
export type QoderGatewayLogEntry = TraeGatewayLogEntry & { region?: string };

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

/**
 * 错峰活动徽标：**中文优先、英文兜底**（与后端 `placements` 取标题同一口径）。
 *
 * 只认官方给的 `badge`；没有就是 `null`（不编「原价」这种不存在的事实）。
 */
function promotionBadge(promotion: unknown): string | null {
  const record = (promotion && typeof promotion === "object" ? promotion : {}) as Record<
    string,
    unknown
  >;
  const badge = (
    record.badge && typeof record.badge === "object" ? record.badge : {}
  ) as Record<string, unknown>;
  return asString(badge.zh) ?? asString(badge.en) ?? null;
}

/** 错峰时段（`"22:00-08:00"`）；缺任一端 ⇒ `null`（不显示半截时段）。 */
function promotionWindow(promotion: unknown): string | null {
  const record = (promotion && typeof promotion === "object" ? promotion : {}) as Record<
    string,
    unknown
  >;
  const start = asString(pick(record, "windowStart", "window_start"));
  const end = asString(pick(record, "windowEnd", "window_end"));
  return start && end ? `${start}-${end}` : null;
}

/** 归一化 `get_qoder_gateway_config` 的返回。 */
export function normalizeQoderGatewayConfig(raw: unknown): QoderGatewayConfig {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const read = (camel: string, snake: string) => record[camel] ?? record[snake];
  return {
    enabled: asBoolean(record.enabled) ?? DEFAULT_QODER_GATEWAY_CONFIG.enabled,
    bindAddr: asString(read("bindAddr", "bind_addr")) ?? DEFAULT_QODER_GATEWAY_CONFIG.bindAddr,
    port: asNumber(record.port) ?? DEFAULT_QODER_GATEWAY_CONFIG.port,
    allowNonLoopback:
      asBoolean(read("allowNonLoopback", "allow_non_loopback")) ??
      DEFAULT_QODER_GATEWAY_CONFIG.allowNonLoopback,
    logKeep: asNumber(read("logKeep", "log_keep")) ?? DEFAULT_QODER_GATEWAY_CONFIG.logKeep,
    logBodies: asBoolean(read("logBodies", "log_bodies")) ?? DEFAULT_QODER_GATEWAY_CONFIG.logBodies,
    maxBodyMb: asNumber(read("maxBodyMb", "max_body_mb")) ?? DEFAULT_QODER_GATEWAY_CONFIG.maxBodyMb,
    // 空串是**合法值**（= 由清单第一条兜底）⇒ `asString` 对空串返回 undefined 时
    // 必须回落到空串，而不是某个「默认模型」（那会让用户以为配置里写了它）。
    defaultModel:
      asString(read("defaultModel", "default_model")) ?? DEFAULT_QODER_GATEWAY_CONFIG.defaultModel,
    maxRotate: asNumber(read("maxRotate", "max_rotate")) ?? DEFAULT_QODER_GATEWAY_CONFIG.maxRotate,
    preferredUid:
      asString(read("preferredUid", "preferred_uid")) ??
      DEFAULT_QODER_GATEWAY_CONFIG.preferredUid,
    stickyTtlMs:
      asNumber(read("stickyTtlMs", "sticky_ttl_ms")) ?? DEFAULT_QODER_GATEWAY_CONFIG.stickyTtlMs,
    maxContext:
      asBoolean(read("maxContext", "max_context")) ?? DEFAULT_QODER_GATEWAY_CONFIG.maxContext,
    // 阈值：负数 / 非数字一律归一为 0（= 关闭）—— 负阈值会让**所有**账号被拦。
    reserveCredits: Math.max(
      0,
      asNumber(read("reserveCredits", "reserve_credits")) ?? DEFAULT_QODER_GATEWAY_CONFIG.reserveCredits,
    ),
    dailyTokenLimit: Math.max(
      0,
      asNumber(read("dailyTokenLimit", "daily_token_limit")) ??
        DEFAULT_QODER_GATEWAY_CONFIG.dailyTokenLimit,
    ),
    // credit 阈值是**浮点**（上游按 0.0x 计价）；非有限数一律按 0（关）。
    dailyCreditLimit: Math.max(
      0,
      asNumber(read("dailyCreditLimit", "daily_credit_limit")) ??
        DEFAULT_QODER_GATEWAY_CONFIG.dailyCreditLimit,
    ),
    modelDailyTokenLimit: Math.max(
      0,
      asNumber(read("modelDailyTokenLimit", "model_daily_token_limit")) ??
        DEFAULT_QODER_GATEWAY_CONFIG.modelDailyTokenLimit,
    ),
    // ★ issue #129：缺键 ⇒ `false`（老配置文件 / 老后端），与「默认关闭」一致。
    autoFailover:
      asBoolean(read("autoFailover", "auto_failover")) ??
      DEFAULT_QODER_GATEWAY_CONFIG.autoFailover,
  };
}

/**
 * 前端形状 → 后端 snake_case。
 *
 * 后端同时接受两种写法（`#[serde(alias)]`），但**写回时统一用 snake_case**：
 * 磁盘上的配置文件应该只有一种风格。
 */
export function toQoderGatewayConfigRaw(config: QoderGatewayConfig): QoderGatewayConfigRaw {
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
    max_context: config.maxContext,
    reserve_credits: config.reserveCredits,
    daily_token_limit: config.dailyTokenLimit,
    daily_credit_limit: config.dailyCreditLimit,
    model_daily_token_limit: config.modelDailyTokenLimit,
    auto_failover: config.autoFailover,
  };
}

/** 归一化 `qoder_gateway_status` 的返回。非法值 ⇒ `null`（调用方回退到「未启动」空态）。 */
export function normalizeQoderGatewayStatus(raw: unknown): QoderGatewayStatus | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Record<string, unknown>;
  const pool = (record.pool && typeof record.pool === "object" ? record.pool : {}) as Record<
    string,
    unknown
  >;
  const accounts = Array.isArray(record.accounts)
    ? record.accounts
        .map((item): QoderGatewayPoolAccount | null => {
          if (!item || typeof item !== "object") return null;
          const entry = item as Record<string, unknown>;
          const uid = asString(entry.uid);
          if (!uid) return null;
          const status = asString(entry.status) ?? "available";
          return {
            uid,
            name: asString(entry.name) ?? uid,
            status:
              status === "cooling" || status === "disabled" || status === "excluded"
                ? (status as QoderGatewayPoolAccount["status"])
                : "available",
            credits: asNumber(entry.credits) ?? null,
            cooling: asBoolean(entry.cooling) ?? false,
            cooldownUntil: asNumber(pick(entry, "cooldownUntil", "cooldown_until")) ?? null,
            cooldownReason: asString(pick(entry, "cooldownReason", "cooldown_reason")) ?? null,
            disabled: asBoolean(entry.disabled) ?? false,
            // ★ 用户显式排除（互斥第六态）。**必须显式归一化**：漏了它，
            //   「排除 / 取消排除」按钮的文案恒为「排除」、提交值恒为 `true`
            //   ⇒ 排除后再也取消不掉（与 WorkBuddy / Trae 同源，2026-10-22 用户报障）。
            excluded: entry.excluded === true,
            excludedNote:
              typeof entry.excludedNote === "string" && entry.excludedNote
                ? entry.excludedNote
                : null,
          };
        })
        .filter((item): item is QoderGatewayPoolAccount => item !== null)
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
    port: asNumber(record.port) ?? DEFAULT_QODER_GATEWAY_CONFIG.port,
    allowNonLoopback: asBoolean(pick(record, "allowNonLoopback", "allow_non_loopback")) ?? false,
    version: asString(record.version) ?? "",
    totalRequests: asNumber(pick(record, "totalRequests", "total_requests")) ?? 0,
    lastError: asString(pick(record, "lastError", "last_error")) ?? null,
    apiKeyPrefix: asString(pick(record, "apiKeyPrefix", "api_key_prefix")) ?? "",
    pool: {
      total: asNumber(pool.total) ?? 0,
      available: asNumber(pool.available) ?? 0,
      cooling: asNumber(pool.cooling) ?? 0,
      disabled: asNumber(pool.disabled) ?? 0,
      // Qoder 池不建模积分 ⇒ 后端恒给 0；这里照实归一（不造数），卡片也不渲染这两格。
      expired: asNumber(pool.expired) ?? 0,
      zeroCredits: asNumber(pick(pool, "zeroCredits", "zero_credits")) ?? 0,
      totalCredits: asNumber(pick(pool, "totalCredits", "total_credits")) ?? 0,
      // 「用户显式排除」的账号数（互斥第六态）。后端 `QoderPool::summary` 已把它
      // 从 `available` 里扣掉 ⇒ 缺这一格就会出现「3 可用」而实际只有 2 个能被选中。
      excluded: asNumber(pool.excluded) ?? 0,
      modelCooling: asNumber(pick(pool, "modelCooling", "model_cooling")) ?? 0,
    },
    accounts,
    diagnose,
    upstream: asString(record.upstream) ?? "",
  };
}

/** 归一化 `get_qoder_gateway_models` 的返回（清单 + 来源 + 原因）。 */
export function normalizeQoderGatewayCatalog(raw: unknown): QoderGatewayCatalog {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const source = asString(record.source);
  const rows = Array.isArray(record.data)
    ? record.data
        .map((item): QoderGatewayModelRow | null => {
          if (!item || typeof item !== "object") return null;
          const entry = item as Record<string, unknown>;
          const id = asString(entry.id);
          if (!id) return null;
          return {
            id,
            name: asString(pick(entry, "displayName", "display_name")) ?? id,
            isReasoning: asBoolean(pick(entry, "isReasoning", "is_reasoning")) ?? false,
            maxInputTokens:
              asNumber(pick(entry, "maxInputTokens", "max_input_tokens")) ?? null,
            contextDefault:
              asNumber(pick(entry, "contextDefault", "context_length_default")) ?? null,
            contextMax: asNumber(pick(entry, "contextMax", "context_length_max")) ?? null,
            disabled: asBoolean(entry.disabled) ?? false,
            priceFactor: asNumber(pick(entry, "priceFactor", "price_factor")) ?? null,
            isFree: asBoolean(pick(entry, "isFree", "is_free")) ?? false,
            promotionBadge: promotionBadge(entry.promotion),
            promotionWindow: promotionWindow(entry.promotion),
            efforts: Array.isArray(entry.thinking_efforts)
              ? entry.thinking_efforts.filter((item): item is string => typeof item === "string")
              : [],
          };
        })
        .filter((item): item is QoderGatewayModelRow => item !== null)
    : [];
  return {
    region: asString(record.region) ?? "",
    source:
      source === "live" || source === "cached" || source === "unavailable"
        ? (source as QoderCatalogSource)
        : "unavailable",
    fetchedAt: asNumber(pick(record, "fetchedAt", "fetched_at")) ?? null,
    note: asString(record.note) ?? null,
    rows,
  };
}

/** 归一化 `list_qoder_api_keys` 的返回。 */
export function normalizeQoderApiKeys(raw: unknown): QoderApiKeyRecord[] {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const list = Array.isArray(record.keys) ? record.keys : Array.isArray(raw) ? raw : [];
  return (list as unknown[])
    .map((item): QoderApiKeyRecord | null => {
      if (!item || typeof item !== "object") return null;
      const entry = item as Record<string, unknown>;
      const id = asString(entry.id);
      if (!id) return null;
      return {
        id,
        name: asString(entry.name) ?? id,
        region: asString(entry.region) ?? "cn",
        prefix: asString(entry.prefix) ?? "",
        suffix: asString(entry.suffix) ?? "",
        createdAt: asNumber(pick(entry, "createdAt", "created_at")) ?? 0,
        revokedAt: asNumber(pick(entry, "revokedAt", "revoked_at")) ?? null,
        revoked: asBoolean(entry.revoked) ?? false,
        lastUsedAt: asNumber(pick(entry, "lastUsedAt", "last_used_at")) ?? null,
      };
    })
    .filter((item): item is QoderApiKeyRecord => item !== null);
}

/** 归一化 `get_qoder_gateway_logs` 的返回（复用 Trae 侧同一实现 + 附加 `region`）。 */
export function normalizeQoderGatewayLogs(raw: unknown): QoderGatewayLogEntry[] {
  const entries = normalizeTraeGatewayLogs(raw);
  const list = (raw && typeof raw === "object" && Array.isArray((raw as { logs?: unknown[] }).logs)
    ? (raw as { logs: unknown[] }).logs
    : []) as unknown[];
  return entries.map((entry, index) => {
    const item = list[index];
    const region =
      item && typeof item === "object" ? asString((item as Record<string, unknown>).region) : undefined;
    return region ? { ...entry, region } : entry;
  });
}
