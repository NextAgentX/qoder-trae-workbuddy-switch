/**
 * Trae 用量 · **数据适配层**（纯函数，可单测）。
 *
 * ## 为什么是「映射」而不是「另写一套」
 *
 * 原型的「用量」页是**平台无关**的（`demo-app.js` 的 `viewUsage`：同一段代码 + 不同数据），
 * 所以两平台必须共用同一批图表适配函数。Trae 的载荷形状与 WorkBuddy 不同
 * （网关日志 vs 客户端会话文件），但**图表需要的维度完全一致**：
 * 逐日、逐模型×逐日、周×小时、逐账号。
 *
 * 于是这里只做一件事：把 `TraeTokenStatistics` **映射**成共用的 `TokenStatsSource` 形状，
 * 之后 `dailyModelSeries` / `modelSlices` / `heatmapMatrix` **一行不改地复用**
 * —— 这正是「同一件事只有一份实现」。
 *
 * ⚠️ **不要**在这里再写一份堆叠柱 / 热力矩阵的构造逻辑：那会让两平台的
 * 「分段之和 === 当日总量」「7 行 × 24 列」这类**契约**各自演化。
 *
 * ## 积分侧的差异是**数据**差异
 *
 * WorkBuddy 的逐日入账**不可得**（`events` 无金额字段）⇒ 单向柱；
 * Trae 的每日快照**两侧都有**（`earned` / `consumed`）⇒ 同一 `creditBars()` 直接产出双向柱。
 * 渲染层 `CreditDivergingBar` 按数据自动决定单/双向（P3 决策 #1），不需要平台分支。
 */

import { TRAE_EXPIRING_SOON_MS } from "@/lib/credit-expiry";
import type { CreditExpiry, TokenStatsGroup, TokenStatsSource } from "@/lib/types";
import type {
  TraeDailySnapshot,
  TraeTokenBucket,
  TraeTokenStatistics,
} from "@/lib/trae-types";
import { calendarWindowKeys, creditBars, type CreditDailyBar, type ModelSlice } from "@/lib/usage-charts";
import type { UsageWindow } from "@/lib/usage-window";

/** Trae 的桶缺 WorkBuddy 的缓存三件套 ⇒ 补 0 / `null`，字段**严格同形**。 */
function bucketToGroup(key: string, bucket: Partial<TraeTokenBucket> | undefined): TokenStatsGroup {
  return {
    key,
    total: bucket?.total ?? 0,
    input: bucket?.input ?? 0,
    output: bucket?.output ?? 0,
    /* 缓存维度 Trae **上游不回传**（已登记为 Unsupported）⇒ 恒 0 / null，不编造。 */
    cacheRead: 0,
    cacheWrite: 0,
    uncachedInput: 0,
    records: bucket?.records ?? 0,
    cacheHitRate: null,
  };
}

/**
 * `TraeTokenStatistics` → 共用的 `TokenStatsSource`。
 *
 * - `daily` ← 后端的逐日桶；
 * - `dailyByModel` ← `modelDaily` 按模型分组（**堆叠柱与模型环形的唯一数据源**）；
 * - `hours` ← 后端的「周-小时」槽位（`"{weekday}-{HH}"`，见 Rust `local_slot`）；
 * - `projects` / `sessions` ← **恒空**：网关日志没有稳定的项目 / 会话标识
 *   （`session_id` 每请求新生成），故这两个维度在 Trae 侧**本就不存在**，
 *   由 `unsupported` 显式声明，**不画空壳**。
 */
export function toTokenStatsSource(
  stats: TraeTokenStatistics,
  /**
   * 这本账的来源标识。
   *
   * Trae 有**两本账**：网关日志（`trae-gateway`）与客户端库导入（`trae-client`）。
   * 两者形状完全一致，只有来源不同 —— 而来源在账本视图里**是颜色语义**
   * （客户端青绿 / 网关靛蓝），所以不能像改造前那样写死成网关。
   */
  source: "trae-gateway" | "trae-client" = "trae-gateway",
): TokenStatsSource {
  const daily = (stats.daily ?? []).map((point) => bucketToGroup(point.key ?? "", point));
  const hours = (stats.hours ?? []).map((point) => bucketToGroup(point.key ?? "", point));
  const models = (stats.models ?? []).map((point) => bucketToGroup(point.key ?? "", point));

  const dailyByModel: Record<string, TokenStatsGroup[]> = {};
  for (const point of stats.modelDaily ?? []) {
    const list = dailyByModel[point.model] ?? (dailyByModel[point.model] = []);
    list.push(bucketToGroup(point.date, point));
  }

  return {
    source,
    summary: bucketToGroup("summary", stats.summary),
    models,
    projects: [],
    sessions: [],
    daily,
    dailyByModel,
    hours,
    filesScanned: stats.filesScanned ?? 0,
    parseErrors: stats.parseErrors ?? 0,
    coverageStartAt: stats.coverageStartAt ?? null,
    coverageEndAt: stats.coverageEndAt ?? null,
  };
}

/**
 * 积分 · 逐日收支（**双向**：Trae 的每日快照两侧都有）。
 *
 * 按参数条上的窗口（`range`）建轴、缺日补 0 —— 与 `dailyModelSeries` 同一手法，
 * 避免「近 30 天」的图上只有 3 根柱子。
 */
export function traeCreditBars(
  daily: readonly TraeDailySnapshot[],
  window: UsageWindow,
): CreditDailyBar[] {
  const byDate = new Map(daily.map((point) => [point.date, point]));
  return creditBars(
    calendarWindowKeys(window).map((key) => {
      const point = byDate.get(key);
      return { date: key, inAmt: point?.earned ?? 0, outAmt: point?.consumed ?? 0 };
    }),
  );
}

/**
 * 积分 · 到期分布数据源：把**合并后的行**（`{ credits, expireAt }`）并成 `CreditExpiry` 形状，
 * 交给共用的 `expiryBuckets()`（完整 30 天轴、已过期剔除）。
 *
 * ⚠️ 输入是**行**而不是「`uid → 秒` 映射」：合并态下行键已带区域前缀
 * （见 `trae-credits-merge.ts`），用映射会按 uid 折叠两个区域的账号。
 *
 * ⚠️ 行里的 `expireAt` 是**秒**，而 `CreditResource.expireAt` 是**毫秒**（WB 侧同口径）——
 * 不换算会让「D+1」变成「D+0.00001」而被当成已过期剔除，**静默少算**。
 */
export function traeCreditExpiry(
  rows: readonly { credits: number | null; expireAt: number | null }[],
  now: number = Date.now(),
): CreditExpiry {
  const resources = rows
    .filter((row): row is { credits: number; expireAt: number | null } => row.credits !== null)
    .map((row) => {
      const expireAt = row.expireAt !== null && row.expireAt > 0 ? row.expireAt * 1000 : null;
      return {
        packageCode: null,
        packageName: null,
        total: row.credits,
        remaining: row.credits,
        used: 0,
        status: null,
        expireAt,
        expired: expireAt !== null && expireAt <= now,
        expiringSoon:
          expireAt !== null && expireAt > now && expireAt - now <= TRAE_EXPIRING_SOON_MS,
      };
    });
  return { ok: true, resources };
}

/**
 * 环形切片（`ModelSlice`）—— 共用 `ModelDonut` 的输入形状。
 *
 * 用于原型积分页的两个环形：**账号 Token 占比**与**网关状态码分布**。
 * 两者在 WorkBuddy 侧**没有数据源**（账号维度的 Token 分组在 WB 的 `TokenStatsSource` 里不存在），
 * 故 WB 不渲染 —— 这是**数据差异**，不是做法差异。
 */
function shares(items: { name: string; value: number }[]): ModelSlice[] {
  return items
    .filter((item) => item.value > 0)
    .sort((left, right) => right.value - left.value)
    .map((item) => ({ name: item.name, value: item.value, spark: [] }));
}

/** 账号 Token 占比（源：`accounts[]`，已按用量降序）。 */
export function traeAccountSlices(stats: TraeTokenStatistics): ModelSlice[] {
  return shares((stats.accounts ?? []).map((account) => ({ name: account.name || account.key, value: account.total })));
}

/** 网关状态码分布（源：`statuses[]`）。 */
export function traeStatusSlices(stats: TraeTokenStatistics): ModelSlice[] {
  return shares((stats.statuses ?? []).map((item) => ({ name: item.key, value: item.records })));
}
