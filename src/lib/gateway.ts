// 网关字段归一化与派生工具（对照架构设计 A-3.4 / A-3.7）。
// 后端字段名如有微调，集中在此兼容，避免兼容逻辑散落各组件。
//
// 注意：同一 gateway 模块内两套类型序列化风格不同 ——
//   · GatewayConfig（用户配置，state.rs 定义）序列化为 snake_case，camelCase 仅作反序列化 alias；
//   · GatewayStatus / gateway_status 响应（给前端读）为 camelCase。
// 因此本文件对 Config 以 snake_case 为准、对 Status 以 camelCase 为准，并各自保留别名兜底。

import { t } from "@/lib/i18n";
import type {
  GatewayConfig,
  GatewayPoolAccount,
  GatewayPoolSummary,
  GatewayStatus,
  Region,
} from "@/lib/types";

/** 网关配置缺省值（与后端 GatewayConfig::default 对齐；字段名为 snake_case）。 */
export const DEFAULT_GATEWAY_CONFIG: GatewayConfig = {
  enabled: false,
  bind_addr: "127.0.0.1",
  port: 57891,
  allow_non_loopback: false,
  log_keep: 200,
  log_bodies: false,
  per_key_rate_limit: null,
  max_context: true,
  // ★ issue #129：账号故障时自动切换，**默认关闭**（与 Rust 默认同口径）。
  auto_failover: false,
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

/**
 * 归一化「模型 → 解冻时刻(ms)」映射（模型级冷却）。
 *
 * 后端 WB 侧键名为 camelCase `modelCoolings`；此处再兜一次 snake_case `model_coolings`
 * （防御性，正常不触发）。脏条目（键为空 / 值非有限数）丢弃，整体缺省为**空对象**
 * ——前端矩阵可无脑遍历，不必判 `undefined`。
 */
function normalizeModelCoolings(raw: unknown): Record<string, number> {
  if (!raw || typeof raw !== "object") return {};
  const out: Record<string, number> = {};
  for (const [model, until] of Object.entries(raw as Record<string, unknown>)) {
    const ms = asNumber(until);
    if (model.length > 0 && ms !== undefined) out[model] = ms;
  }
  return out;
}

/** 账号池五态的缺省值（后端缺字段时兜底，界面显示全 0 而不是空白）。 */
export const EMPTY_POOL_SUMMARY: GatewayPoolSummary = {
  total: 0,
  available: 0,
  cooling: 0,
  disabled: 0,
  expired: 0,
  zeroCredits: 0,
  totalCredits: 0,
};

/** 账号状态 → 展示标签与语气（**两侧共用**，故放在本文件而不是 Trae 专用文件）。 */
export const POOL_STATUS_LABELS: Record<
  GatewayPoolAccount["status"],
  { label: string; tone: "ok" | "warn" | "danger" | "muted" }
> = {
  // 表在模块加载时就定型 ⇒ 只存文案键，标签在**读取时**现取（调用方照旧读 `meta.label`）。
  available: {
    get label() {
      return t("shared.poolStatus.available");
    },
    tone: "ok",
  },
  cooling: {
    get label() {
      return t("shared.poolStatus.cooling");
    },
    tone: "warn",
  },
  disabled: {
    get label() {
      return t("shared.poolStatus.disabled");
    },
    tone: "danger",
  },
  expired: {
    get label() {
      return t("shared.poolStatus.expired");
    },
    tone: "warn",
  },
  no_credits: {
    get label() {
      return t("shared.poolStatus.noCredits");
    },
    tone: "muted",
  },
  // ★ 排除是**用户意图**，不是账号故障 ⇒ 用 danger 提醒「它不会被选中」，
  //   但文案必须与「会话失效/禁用」区分开（那个要修，这个只需取消排除）。
  excluded: {
    get label() {
      return t("shared.poolStatus.excluded");
    },
    tone: "danger",
  },
};

/**
 * 归一化池五态计数。
 *
 * 后端一定给全两个版本（`cn` / `global`），但**老后端 / 探测应答**可能没有 ——
 * 这里一律兜底成全 0，绝不让界面出现 `undefined`。
 */
export function normalizeGatewayPoolSummary(raw: unknown): GatewayPoolSummary {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  return {
    total: asNumber(record.total) ?? 0,
    available: asNumber(record.available) ?? 0,
    cooling: asNumber(record.cooling) ?? 0,
    disabled: asNumber(record.disabled) ?? 0,
    expired: asNumber(record.expired) ?? 0,
    zeroCredits: asNumber(record.zeroCredits) ?? asNumber(record.zero_credits) ?? 0,
    excluded: asNumber(record.excluded) ?? 0,
    // ★ 模型级冷却是**正交**计数（issue #88）：账号仍可用，只是某几个模型被 429 罚了。
    //   漏了这一行 ⇒ 后端数得再对，卡片也永远显示 0（WB 侧此前整格都没有）。
    modelCooling: asNumber(record.modelCooling) ?? asNumber(record.model_cooling) ?? 0,
    // 正交计数：缺失/老后端 ⇒ 0（卡片暂不渲染，只保证数据不断链）。
    inFlightFull: asNumber(record.inFlightFull) ?? asNumber(record.in_flight_full) ?? 0,
    totalCredits: asNumber(record.totalCredits) ?? asNumber(record.total_credits) ?? 0,
  };
}

/** 归一化逐账号状态；脏值（非对象 / 缺 uid）直接丢弃，避免脏值渲染崩整棵树。 */
export function normalizeGatewayPoolAccounts(raw: unknown): GatewayPoolAccount[] {
  if (!Array.isArray(raw)) return [];
  const statuses: GatewayPoolAccount["status"][] = [
    "available",
    "cooling",
    "disabled",
    "expired",
    "no_credits",
    "excluded",
  ];
  return raw.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const record = item as Record<string, unknown>;
    const uid = typeof record.uid === "string" ? record.uid : "";
    if (!uid) return [];
    const rawStatus = typeof record.status === "string" ? record.status : "";
    const status = (statuses as string[]).includes(rawStatus)
      ? (rawStatus as GatewayPoolAccount["status"])
      : "available";
    return [
      {
        uid,
        name: typeof record.name === "string" ? record.name : uid,
        status,
        credits: asNumber(record.credits) ?? null,
        cooldownReason: typeof record.cooldownReason === "string" ? record.cooldownReason : null,
        cooldownUntil: asNumber(record.cooldownUntil) ?? null,
        cooling: record.cooling === true,
        disabled: record.disabled === true,
        // ★ 用户显式排除（互斥第六态）。**必须显式归一化**：本函数是白名单重建，
        //   漏掉一个字段就等于前端永远看不到它 —— 而「排除 / 取消排除」按钮的文案与
        //   提交值都依赖 `account.excluded`（`onExclude(uid, !account.excluded)`）。
        //   漏了它的症状不是「不显示」，而是**按钮永远显示「排除」、点下去永远传 `true`**
        //   ⇒ 已排除的账号再也取消不掉（2026-10-22 用户报障的根因）。
        excluded: record.excluded === true,
        excludedNote:
          typeof record.excludedNote === "string" && record.excludedNote
            ? record.excludedNote
            : null,
        realm:
          record.realm === "cn" || record.realm === "global" ? record.realm : "unknown",
        creditsExpireAt: asNumber(record.creditsExpireAt) ?? null,
        deviceIdMasked:
          typeof record.deviceIdMasked === "string" ? record.deviceIdMasked : null,
        // 模型级冷却（池冷却）—— 探测矩阵的被动底色用；缺省 `{}`。
        modelCoolings: normalizeModelCoolings(record.modelCoolings ?? record.model_coolings),
        // 带来源的明细（issue #88 缺口 4.1）—— 徽标用；缺省 `{}`（Trae / 旧后端没有）。
        modelCooldownDetails: normalizeModelCooldownDetails(
          record.modelCooldownDetails ?? record.model_cooldown_details,
        ),
      },
    ];
  });
}

/**
 * 归一化「模型冷却明细」（issue #88 缺口 4.1）。
 *
 * 逐项校验而不是整体信任：这是后端新增字段，Trae / Qoder 侧与旧后端都没有它，
 * 演示数据也可能缺 —— 任何一项形状不对就**跳过该模型**，而不是让整个对象变成
 * `undefined` 后在徽标里炸。
 */
function normalizeModelCooldownDetails(
  raw: unknown,
): NonNullable<GatewayPoolAccount["modelCooldownDetails"]> {
  if (!raw || typeof raw !== "object") return {};
  const out: NonNullable<GatewayPoolAccount["modelCooldownDetails"]> = {};
  for (const [model, value] of Object.entries(raw as Record<string, unknown>)) {
    if (!value || typeof value !== "object" || model.length === 0) continue;
    const item = value as Record<string, unknown>;
    const untilMs = asNumber(item.untilMs);
    if (untilMs === undefined) continue;
    out[model] = {
      untilMs,
      resetAtMs: asNumber(item.resetAtMs) ?? 0,
      // 上游文案里声明的时刻（展示用）；旧后端没有该键 ⇒ `0`（徽标回落兜底时刻）。
      upstreamResetAtMs: asNumber(item.upstreamResetAtMs) ?? 0,
      // 后端直接给 `fallback`；缺失时**由 `resetAtMs === 0` 推出**（同一判据，不猜）。
      fallback: typeof item.fallback === "boolean" ? item.fallback : (asNumber(item.resetAtMs) ?? 0) === 0,
      reason: typeof item.reason === "string" ? item.reason : "",
      hits: asNumber(item.hits) ?? 0,
    };
  }
  return out;
}

/** 归一化诊断行（只保留非空字符串）。 */
export function normalizeGatewayDiagnose(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  return raw.filter((line): line is string => typeof line === "string" && line.length > 0);
}

/**
 * 归一化 get_gateway_config / save_gateway_config 的返回。
 *
 * 后端 GatewayConfig 序列化为 snake_case（camelCase 仅为后端 `#[serde(alias)]`，
 * 反序列化容忍、序列化不产出）。此处 camelCase 兜底属防御性，正常不应被触发。
 */
export function normalizeGatewayConfig(raw: unknown): GatewayConfig {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  return {
    enabled: asBoolean(record.enabled) ?? DEFAULT_GATEWAY_CONFIG.enabled,
    bind_addr:
      asString(record.bind_addr) ?? asString(record.bindAddr) ?? DEFAULT_GATEWAY_CONFIG.bind_addr,
    port: asNumber(record.port) ?? DEFAULT_GATEWAY_CONFIG.port,
    allow_non_loopback:
      asBoolean(record.allow_non_loopback) ??
      asBoolean(record.allowNonLoopback) ??
      DEFAULT_GATEWAY_CONFIG.allow_non_loopback,
    log_keep: asNumber(record.log_keep) ?? asNumber(record.logKeep) ?? DEFAULT_GATEWAY_CONFIG.log_keep,
    log_bodies: asBoolean(record.log_bodies) ?? asBoolean(record.logBodies) ?? DEFAULT_GATEWAY_CONFIG.log_bodies,
    per_key_rate_limit:
      asNumber(record.per_key_rate_limit) ?? asNumber(record.perKeyRateLimit) ?? null,
    max_context: asBoolean(record.max_context) ?? DEFAULT_GATEWAY_CONFIG.max_context,
    // ★ issue #129：缺键 ⇒ `false`（老配置文件 / 老后端），与「默认关闭」一致。
    auto_failover:
      asBoolean(record.auto_failover) ??
      asBoolean(record.autoFailover) ??
      DEFAULT_GATEWAY_CONFIG.auto_failover,
  };
}

/**
 * 将 gateway_status 原始响应归一化为前端规范形状。
 *
 * 后端 gateway_status（GatewayStatusView）现为 snake_case：
 * `base_url` / `bind_addr` / `allow_non_loopback`。此处以 snake_case 为准，
 * 并保留 camelCase 别名兜底（防御性，正常不应触发）。
 * 传入非法值时返回 null，调用方据此回退到配置值。
 */
export function normalizeGatewayStatus(raw: unknown): GatewayStatus | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Record<string, unknown>;
  const addr =
    asString(record.addr) ?? asString(record.bindAddr) ?? asString(record.bind_addr) ?? "127.0.0.1";
  const port = asNumber(record.port) ?? 57891;
  const allowNonLoopback =
    asBoolean(record.allowNonLoopback) ?? asBoolean(record.allow_non_loopback) ?? false;
  // 池视图：`pools` 两个版本的键恒定存在（后端保证），这里再兜一次底。
  const poolsRaw = (record.pools && typeof record.pools === "object" ? record.pools : {}) as Record<
    string,
    unknown
  >;
  const pools: Record<Region, GatewayPoolSummary> = {
    cn: normalizeGatewayPoolSummary(poolsRaw.cn),
    global: normalizeGatewayPoolSummary(poolsRaw.global),
  };
  return {
    enabled: asBoolean(record.enabled) ?? false,
    running: asBoolean(record.running) ?? false,
    addr,
    port,
    allowNonLoopback,
    baseUrl: asString(record.baseUrl) ?? asString(record.base_url),
    lastError: asString(record.last_error) ?? asString(record.lastError) ?? null,
    pools,
    accounts: normalizeGatewayPoolAccounts(record.accounts),
    diagnose: normalizeGatewayDiagnose(record.diagnose),
  };
}

/** 取某版本的池明细（计数 + 该版本的账号 + 诊断行）。 */
export function poolOf(status: GatewayStatus | null, region: Region): {
  pool: GatewayPoolSummary;
  accounts: GatewayPoolAccount[];
  diagnose: string[];
} {
  const accounts = (status?.accounts ?? []).filter((account) => account.realm === region);
  return {
    pool: status?.pools?.[region] ?? EMPTY_POOL_SUMMARY,
    accounts,
    // 诊断行不按版本标记 ⇒ 全量给出（行内自带账号名与 uid 尾号，用户能对上号）。
    diagnose: status?.diagnose ?? [],
  };
}

/**
 * 计算对外展示的 Base URL。
 *
 * 优先使用后端返回的 `baseUrl`；否则按监听地址与端口拼接（兜底，正常不应触发）。
 * `0.0.0.0` 对外不可直接使用，展示时回退为 `127.0.0.1`。
 */
export function resolveGatewayBaseUrl(
  status: GatewayStatus | null,
  fallbackAddr: string,
  fallbackPort: number,
): string {
  if (status?.baseUrl) return status.baseUrl;
  const rawHost = status?.addr ?? fallbackAddr;
  const host = rawHost === "0.0.0.0" ? "127.0.0.1" : rawHost;
  const port = status?.port ?? fallbackPort;
  return `http://${host}:${port}/v1`;
}

/** 网关进程是否实际在监听（缺省视为已停止）。 */
export function resolveGatewayRunning(status: GatewayStatus | null): boolean {
  return status?.running ?? false;
}
