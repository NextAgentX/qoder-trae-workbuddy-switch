/**
 * 用量页「计入网关」开关的**唯一**语义表。
 *
 * ## 为什么只有一枚开关
 *
 * 用户 2026-10-16 两次收紧口径：先是「每个视图各做各的网关操作」，随后是
 * 「开关太多了，一个开关控制就行」，再后来「开关默认是开启」。
 * ⇒ 页面上**只此一枚**开关（`UsagePage` 页面级持有），但它**按当前视图执行对应的操作**：
 *
 * | 视图 | 开关打开时做什么 | 数据出口 |
 * |:--|:--|:--|
 * | 趋势 | 展示「网关逐日用量」图（**独立小图**，不并进主堆叠柱） | `get_gateway_token_trend` |
 * | 模型 | 按模型并入网关请求与 Token（`网关 · <模型>` 前缀） | `get_gateway_logs` |
 * | 会话 | 单独列出「网关请求记录（非会话）」，不计作会话 | `get_gateway_logs` |
 * | 积分 | 按账号并入网关账号池余额（同账号取网关值，避免重复相加） | `gateway_status` |
 *
 * ## 为什么不并进趋势主柱（与原型刻意的差异）
 *
 * 原型的趋势是「把网关 Token 加进每日总量」，生产**刻意不这么做**（`TokenStatsPage`
 * 文件头的两条理由）：① 主柱「末柱 = 概览统计块 = 账号卡今日合计」是跨视图一致性契约，
 * 并进去会**静默改口径**；② 客户端与网关是两条独立记录，求和会把**重复计数**藏起来。
 * ⇒ 生产侧的趋势语义 = 「显示/隐藏网关那张独立图」，而不是求和。
 *
 * ## 平台可用性
 *
 * 只有 WorkBuddy 有**可与客户端统计并行**的独立网关数据：
 * - **Trae**：用量本身就从网关请求日志聚合（`get_trae_token_statistics`）⇒ 没有「不计入」
 *   这一档，开关禁用并说明原因（`hintKey` 走 `native`）；
 * - **Qoder**：用量页只有趋势且是额度口径 ⇒ 无独立网关数据可计入（`unavailable`）。
 */

import type { Product } from "@/lib/platforms/registry";
import { type ModelSlice } from "@/lib/usage-charts";
import { dateKey, usageWindowBounds, type UsageWindow } from "@/lib/usage-window";
import type { GatewayLogEntry, RegionFilter } from "@/lib/types";
import type { TranslationKey } from "@/locales/zh";

/** 用量页的四个视图（与 `UsagePage` 的 `UsageTab` **结构等价**；本模块不 import 页面以免成环）。 */
export type UsageGatewayTab = "trend" | "models" | "sessions" | "credits";

/** 开关在参数条上的展示契约（文案键 + 是否可用）。 */
export interface UsageGatewayControl {
  /** 开关名（随视图变化：计入网关 Token / 模型用量 / 请求记录 / 积分余额）。 */
  labelKey: TranslationKey;
  /** 说明文案（可用时随开/关切换，不可用时给原因）。 */
  hintKey: TranslationKey;
  /** 该视图能否真的计入网关数据（`false` ⇒ 开关 `disabled`）。 */
  available: boolean;
}

const LABEL: Record<UsageGatewayTab, TranslationKey> = {
  trend: "wbStats.usage.gateway.trend.label",
  models: "wbStats.usage.gateway.models.label",
  sessions: "wbStats.usage.gateway.sessions.label",
  credits: "wbStats.usage.gateway.credits.label",
};

/** 打开时的说明（说清「这次到底合了什么」）。 */
const HINT_ON: Record<UsageGatewayTab, TranslationKey> = {
  trend: "wbStats.usage.gateway.trend.on",
  models: "wbStats.usage.gateway.models.on",
  sessions: "wbStats.usage.gateway.sessions.on",
  credits: "wbStats.usage.gateway.credits.on",
};

/** 关闭时的说明（说清「现在只统计什么」）。 */
const HINT_OFF: Record<UsageGatewayTab, TranslationKey> = {
  trend: "wbStats.usage.gateway.trend.off",
  models: "wbStats.usage.gateway.models.off",
  sessions: "wbStats.usage.gateway.sessions.off",
  credits: "wbStats.usage.gateway.credits.off",
};

/** 平台 → 不可用时给的原因（只有非 WorkBuddy 会用到）。 */
const UNAVAILABLE_HINT: Partial<Record<Product, TranslationKey>> = {
  trae: "wbStats.usage.gateway.hint.native",
  qoder: "wbStats.usage.gateway.hint.unavailable",
};

/**
 * 平台 → **可以计入网关**的视图（其余视图的开关禁用并说明原因）。
 *
 * - **WorkBuddy**：四个视图都有可与客户端统计并行的网关来源；
 * - **Trae**：只有**积分**视图有 —— 「账号 Token 占比」与「网关状态分布」两个环形本来就由
 *   本机网关请求日志聚合而来，可以**拆成独立一组**随开关显示 / 隐藏；
 *   它的**趋势 / 模型**本身就整条由网关日志聚合 ⇒ 没有「不计入」这一档（原生）；
 * - **Qoder**：用量页只有趋势且是额度口径 ⇒ 无独立网关数据。
 */
const AVAILABLE_TABS: Record<Product, readonly UsageGatewayTab[]> = {
  workbuddy: ["trend", "models", "sessions", "credits"],
  trae: ["credits"],
  qoder: [],
  doubao: [],
  loomy: [],
  raccoon: [],
};

/** 少数（平台 × 视图）的开关名与说明有**专门口径**（默认表说不清时）。 */
const OVERRIDE: Partial<
  Record<
    Product,
    Partial<Record<UsageGatewayTab, { labelKey: TranslationKey; onKey: TranslationKey; offKey: TranslationKey }>>
  >
> = {
  /* Trae 积分的默认文案是「计入网关积分余额」（WorkBuddy 的口径：并入账号池余额），
     但 Trae 这边是**拆出两个环形**，没有余额合并 ⇒ 用专门口径，别让用户以为它也会改总数。 */
  trae: {
    credits: {
      labelKey: "wbStats.usage.gateway.traeCredits.label",
      onKey: "wbStats.usage.gateway.traeCredits.on",
      offKey: "wbStats.usage.gateway.traeCredits.off",
    },
  },
};

/**
 * 当前（平台 × 视图 × 开关态）下开关的文案与可用性。
 *
 * ★ 纯函数（不 import React、不碰 DOM）⇒ 门禁可直接求值，视图页也只需按结果渲染。
 */
export function usageGatewayControl(
  product: Product,
  tab: UsageGatewayTab,
  enabled: boolean,
): UsageGatewayControl {
  const override = OVERRIDE[product]?.[tab];
  const labelKey = override?.labelKey ?? LABEL[tab];
  if (!AVAILABLE_TABS[product].includes(tab)) {
    return {
      labelKey,
      hintKey: UNAVAILABLE_HINT[product] ?? "wbStats.usage.gateway.hint.unavailable",
      available: false,
    };
  }
  return {
    labelKey,
    hintKey: enabled ? (override?.onKey ?? HINT_ON[tab]) : (override?.offKey ?? HINT_OFF[tab]),
    available: true,
  };
}

/** 平台 × 视图是否可计入网关（渲染层判断「有没有可拆出的网关块」时复用同一张表）。 */
export function usageGatewayAvailable(product: Product, tab: UsageGatewayTab): boolean {
  return AVAILABLE_TABS[product].includes(tab);
}

/** 只保留属于当前查询范围的网关日志（`all` = 不过滤；与 `RegionFilter` 的语义一致）。 */
export function gatewayLogsInRange(
  logs: GatewayLogEntry[],
  region: RegionFilter,
): GatewayLogEntry[] {
  if (region === "all") return logs;
  return logs.filter((entry) => entry.region === region);
}

/**
 * 只保留落在视图窗口内的日志（**本地日历日闭区间**，与 `usageWindowBounds` 同源）。
 *
 * ⚠️ 判据必须是**真实时间戳 → 本地日期键**，不能拿「窗口天数」去 `slice` 日志：
 * 日志没有「第几天」的概念，天数拟合会让「锚在过去的自定义窗口」静默取错。
 */
export function gatewayLogsInWindow(
  logs: GatewayLogEntry[],
  window: UsageWindow,
  now: number = Date.now(),
): GatewayLogEntry[] {
  const { from, to } = usageWindowBounds(window, now);
  return logs.filter((entry) => {
    if (!entry.ts) return false;
    const key = dateKey(new Date(entry.ts));
    return key >= from && key <= to;
  });
}

/** 单条日志的 Token 数（上游可能只给一侧 / 都不给 ⇒ 缺失按 0，不猜）。 */
function logTokens(entry: GatewayLogEntry): number {
  return (entry.promptTokens ?? 0) + (entry.completionTokens ?? 0);
}

/**
 * 网关请求日志 → **按模型的切片**（`网关 · <模型>` 前缀，避免与客户端同名模型混淆）。
 *
 * ★ 值与客户端模型**分开列**（前缀 + 独立切片），不做同名合并 ——
 * 二者是两条独立记录，合并会把重复计数藏起来（与趋势页「不并进主柱」同一条理由）。
 * 无模型名的记录归入「未知模型」，不静默丢弃。
 */
export function gatewayModelSlices(
  logs: GatewayLogEntry[],
  region: RegionFilter,
  unknownLabel: string,
): ModelSlice[] {
  const byModel = new Map<string, ModelSlice>();
  for (const entry of gatewayLogsInRange(logs, region)) {
    const name = (entry.model ?? "").trim() || unknownLabel;
    const slice = byModel.get(name) ?? { name, value: 0, spark: [] };
    slice.value += logTokens(entry);
    byModel.set(name, slice);
  }
  return [...byModel.values()].sort((left, right) => right.value - left.value);
}

/**
 * 积分视图：「客户端账号余额」与「网关账号池余额」合并成**一个合计**。
 *
 * ★ 合并口径 = **按账号名取并集、同账号以网关值为准**：
 * - 同名 ⇒ 用网关值（网关读的是上游实时余额，比客户端快照新），**不相加**；
 * - 只有网关有 ⇒ 补进来（客户端还没有这个账号）；
 * - 只有客户端有 ⇒ 保留客户端值。
 *
 * 为什么不直接相加：同一个账号在两边各有一份读数，相加 = **重复计量**
 * （与趋势页「网关与客户端不求和」同一条理由）。
 * 名字大小写/空白差异按 `trim().toLowerCase()` 归一后再比。
 */
export function mergeGatewayCredits(
  clientAccounts: { name: string; credits: number }[],
  poolAccounts: { name: string; credits: number | null }[],
): { total: number; count: number } {
  const key = (name: string) => name.trim().toLowerCase();
  const poolByName = new Map<string, number>();
  for (const account of poolAccounts) {
    if (typeof account.credits !== "number") continue;
    const id = key(account.name);
    if (!id) continue;
    poolByName.set(id, account.credits);
  }
  const used = new Set<string>();
  let total = 0;
  let count = 0;
  for (const account of clientAccounts) {
    const id = key(account.name);
    const fromPool = poolByName.get(id);
    if (fromPool !== undefined) used.add(id);
    total += fromPool ?? account.credits;
    count += 1;
  }
  for (const [id, credits] of poolByName) {
    if (used.has(id)) continue;
    total += credits;
    count += 1;
  }
  return { total, count };
}

/**
 * 网关请求日志 → 会话视图下方那张「非会话」表的行（**新 → 旧**，最多 `limit` 行）。
 *
 * ⚠️ 网关日志**没有会话标识** ⇒ 这些行只作为「本作用域经网关的请求」列出，
 * **绝不**并进客户端会话数 / 排行 / 散点（那会是谎报「网关请求 = 一个会话」）。
 */
export function gatewayRequestRows(
  logs: GatewayLogEntry[],
  region: RegionFilter,
  limit: number,
): GatewayLogEntry[] {
  return [...gatewayLogsInRange(logs, region)]
    .sort((left, right) => (right.ts ?? 0) - (left.ts ?? 0))
    .slice(0, limit);
}
