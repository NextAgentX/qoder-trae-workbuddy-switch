/**
 * 用量四视图 · **数据适配层**（P3，纯函数）。
 *
 * ## 为什么单独一层
 *
 * 「图不能撒谎」的契约（PLAN §3.10 / P3-ARCH §1.4）必须落在**能单测、能被门禁加载**的
 * 纯函数里，而不是散在 recharts 的 JSX 中 —— 渲染层测不了「分段之和 = 当日总量」，
 * 适配层可以。
 *
 * ## 契约（由本文件保证，不依赖渲染层）
 *
 * 1. 堆叠柱：每列 `segments` 之和 **精确等于** `total`（末段吃舍入余数）；
 * 2. 热力图：**7 行（周一..周日）× 24 列**，缺失格补 0（不丢点）；
 * 3. 环形：`modelSlices` 的每一项都产出一个扇区（扇区数 = 模型数）；
 * 4. 余额曲线：基线**不锚 0**（高位小幅波动不压成直线）；
 * 5. 到期分布：**完整 30 天轴**（无到期的天补 0，不跳格）。
 *
 * ## 字段来源（逐字段核实，**不张冠李戴**）
 *
 * - 热力图来自 `TokenStatsSource.hours`，key = `"{weekday}-{hour}"`
 *   （`token_stats.rs:215`，weekday 0=周一 … 6=周日，hour 0–23）—— **不是** 24 桶；
 * - 会话散点的 x 轴是**记录数**（`TokenStatsGroup.records`），
 *   **真实后端没有 `msgs`（消息数）字段** —— 图注必须写「记录数」；
 * - 余额曲线**不是**后端字段：后端无逐日余额快照，必须以 `summary.currentRemaining`
 *   为终值、按净额**回推**（图注注明）；
 * - 积分入账**不可得**（见 `creditDailyBars` 的注释）。
 *
 * 本模块**不导入 React**：只有 `type` 导入（转译后消失）+ 一个零依赖的 `displayText`，
 * 因此可被 `loadTs` 门禁直接求值做运行时校验。
 */

import { displayText } from "@/lib/display-text";
import type {
  CreditExpiry,
  CreditStatistics,
  TokenStatsGroup,
  TokenStatsSource,
  TokenStatsTotals,
} from "@/lib/types";

/** 四视图标识（与原型 `tabs` 一一对应）。 */
export type UsageView = "trend" | "models" | "sessions" | "credits";

/** 视图参数：时间范围（天）。与原型 `[7,14,30]` 一致。 */
export type UsageRange = 7 | 14 | 30;

/** 趋势 · 堆叠柱：一天一列，`segments` 之和 === `total`（契约 1）。 */
export interface DailyModelColumn {
  /** ISO 日期。 */
  day: string;
  /** 轴短标签（MM/DD）。 */
  label: string;
  total: number;
  /** 末段吃舍入余数，保证求和 === total。 */
  segments: { name: string; value: number }[];
}

/** 趋势 · 热力图：7 行（周一..周日）× 24 列。 */
export interface HeatmapMatrix {
  rows: 7;
  cols: 24;
  /** `[weekday][hour]`，缺失补 0。 */
  cells: number[][];
  /** 用于相对强度着色（非绝对量）。 */
  max: number;
}

/** 模型：占比 + 迷你趋势线（契约 3 / 7）。 */
export interface ModelSlice {
  name: string;
  value: number;
  /** 逐日序列（`dailyByModel`）；后端未提供时为空数组。 */
  spark: number[];
}

/** 会话：散点。x 是**记录数**不是「消息数」（见文件头注释）。 */
export interface SessionPoint {
  id: string;
  title: string;
  /** records。 */
  x: number;
  /** total tokens。 */
  y: number;
  value: number;
}

/**
 * 积分：逐日收支。
 *
 * ⚠️ `inAmt` **恒为 0** —— 已核实入账**不可得**（P3-ARCH §6 #1）：
 * `events` 里 `kind==="checkin"` 的载荷**没有金额字段**（只有 `result`），
 * 且入账还可能是签到之外的来源 ⇒ 无法按日汇总入账额。
 * 因此消费侧（`outAmt`）真实、入账侧留 0；渲染层据此**只画消耗侧**（单向柱）。
 */
export interface CreditDailyBar {
  label: string;
  inAmt: number;
  outAmt: number;
}

/** 积分：余额曲线点（`value` 回推；`baseline` 非 0）。 */
export interface BalancePoint {
  label: string;
  value: number;
  baseline: number;
}

/** 积分：到期分布（完整 30 天）。 */
export interface ExpiryBucket {
  /** 1..30。 */
  day: number;
  value: number;
}

/** 堆叠柱 / 环形在「无模型明细」时使用的哨兵名，由渲染层翻译。 */
export const TOTAL_SEGMENT = "__total__";

const DAY_MS = 24 * 60 * 60 * 1000;

function emptyTotals(): TokenStatsTotals {
  return {
    total: 0,
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    uncachedInput: 0,
    records: 0,
    cacheHitRate: null,
  };
}

function addTotals(a: TokenStatsTotals, b: TokenStatsTotals): TokenStatsTotals {
  return {
    total: a.total + b.total,
    input: a.input + b.input,
    output: a.output + b.output,
    cacheRead: a.cacheRead + b.cacheRead,
    cacheWrite: a.cacheWrite + b.cacheWrite,
    uncachedInput: a.uncachedInput + b.uncachedInput,
    records: a.records + b.records,
    cacheHitRate: null,
  };
}

function addGroup(a: TokenStatsGroup, b: TokenStatsGroup): TokenStatsGroup {
  return {
    ...addTotals(a, b),
    key: a.key,
    title: a.title ?? b.title ?? null,
    project: a.project ?? b.project,
    sessionId: a.sessionId ?? b.sessionId,
  };
}

/** 按 `key` 合并分组（数值求和），按 `total` 降序。 */
function mergeGroups(list: TokenStatsGroup[]): TokenStatsGroup[] {
  const map = new Map<string, TokenStatsGroup>();
  for (const group of list) {
    const current = map.get(group.key);
    map.set(group.key, current ? addGroup(current, group) : { ...group });
  }
  return [...map.values()].sort((left, right) => right.total - left.total);
}

function minDefined(values: (number | null | undefined)[]): number | null {
  const defined = values.filter((value): value is number => typeof value === "number");
  return defined.length ? Math.min(...defined) : null;
}

function maxDefined(values: (number | null | undefined)[]): number | null {
  const defined = values.filter((value): value is number => typeof value === "number");
  return defined.length ? Math.max(...defined) : null;
}

function emptySource(): TokenStatsSource {
  return {
    source: "workbuddy",
    summary: emptyTotals(),
    models: [],
    projects: [],
    sessions: [],
    daily: [],
    dailyByModel: {},
    hours: [],
    filesScanned: 0,
    parseErrors: 0,
    coverageStartAt: null,
    coverageEndAt: null,
  };
}

/**
 * 把若干 `TokenStatsSource` 聚合成一份（等价原型 `mergeExt` 的**全维度**聚合）。
 *
 * 用于两处：① 把同一范围下的多个来源（workbuddy / codebuddy-cli / …）合为一份；
 * ② 合并作用域（`region === "all"`）时后端会同时下发两版来源，一并求和。
 *
 * ⚠️ **图表数据必须一起聚合**（`models / dailyByModel / hours`）——
 * 否则「切了合并只有 days 变」是谎报（P3-ARCH §1.3 契约 12）。
 */
export function mergeSources(sources: TokenStatsSource[]): TokenStatsSource {
  if (sources.length === 0) return emptySource();
  if (sources.length === 1) return sources[0];

  const summary = sources.reduce((acc, source) => addTotals(acc, source.summary), emptyTotals());

  const modelDayMap = new Map<string, Map<string, TokenStatsGroup>>();
  for (const source of sources) {
    for (const [model, points] of Object.entries(source.dailyByModel ?? {})) {
      let dayMap = modelDayMap.get(model);
      if (!dayMap) {
        dayMap = new Map<string, TokenStatsGroup>();
        modelDayMap.set(model, dayMap);
      }
      for (const point of points) {
        const current = dayMap.get(point.key);
        dayMap.set(point.key, current ? addGroup(current, point) : { ...point });
      }
    }
  }
  const dailyByModel: Record<string, TokenStatsGroup[]> = {};
  for (const [model, dayMap] of modelDayMap) {
    dailyByModel[model] = [...dayMap.values()].sort((left, right) =>
      left.key.localeCompare(right.key),
    );
  }

  return {
    source: sources[0].source,
    summary,
    models: mergeGroups(sources.flatMap((source) => source.models)),
    projects: mergeGroups(sources.flatMap((source) => source.projects)),
    sessions: mergeGroups(sources.flatMap((source) => source.sessions)),
    daily: mergeGroups(sources.flatMap((source) => source.daily)).sort((left, right) =>
      left.key.localeCompare(right.key),
    ),
    dailyByModel,
    hours: mergeGroups(sources.flatMap((source) => source.hours)),
    filesScanned: sources.reduce((sum, source) => sum + source.filesScanned, 0),
    parseErrors: sources.reduce((sum, source) => sum + source.parseErrors, 0),
    coverageStartAt: minDefined(sources.map((source) => source.coverageStartAt)),
    coverageEndAt: maxDefined(sources.map((source) => source.coverageEndAt)),
  };
}

/** 本地日期键 `YYYY-MM-DD`（与后端逐日桶同口径，不走 `toISOString` —— 那会偏时区）。 */
function dateKey(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/**
 * ★ A1：最近 `range` 个**日历日**的日期键（升序，恒 `range` 项）。
 *
 * 给「日均」这类**需要知道窗口天数**的调用方用 —— 分母必须是**窗口天数**，
 * 不能是「有数据的天数」（否则 5 天数据显示成「日均 300」，而真实 30 天日均是 50）。
 */
export function calendarWindowKeys(range: UsageRange, now: number = Date.now()): string[] {
  return Array.from({ length: range }, (_, offset) =>
    dateKey(new Date(now - (range - 1 - offset) * DAY_MS)),
  );
}

/**
 * ★ A1：取**最近 `range` 个日历日**的逐日点（升序），**缺失日补 0**。
 *
 * ## 为什么不 `.slice(-range)`
 *
 * `daily` 是**稀疏**的：后端 `token_stats.rs` 的 `daily: HashMap<String, Totals>`
 * 只在**有记录的日子**落条目（`.or_default()` 不会给缺失日补键）⇒
 * 「最近 N 天」不等于「数组最后 N 项」。
 *
 * 旧实现 `slice(-range)` 的两个实测后果：
 * 1. `daily` 只有 5 天数据而 `range=30` ⇒ **只出 5 列**（原型断言要求 `cols.length === 30`）；
 * 2. `range=7` 时首列落在 **59 天前** —— 却配着「近 7 天」的标签 ⇒ **图文不符**。
 *
 * ## 修法
 *
 * 按 `Date.now()` 倒推 N 个**日历日**建轴，缺失日补 0（与 `expiryBuckets` 补 0、
 * `heatmap` 补 0 同一手法）。**后端不动** —— 前端补齐即可，避免动 `HashMap` 契约。
 *
 * ⚠️ 补 0 会改变下游的「空态」判定路径（过去「只有 5 天数据」可能让某些图判空），
 * 这是**预期**的：现在 0 天的柱子**本就该显示为 0**，而不是让窗口悄悄缩成 5 天。
 */
function lastDays(daily: TokenStatsGroup[], range: UsageRange): TokenStatsGroup[] {
  const byKey = new Map(daily.map((point) => [point.key, point]));
  const days: TokenStatsGroup[] = [];
  for (let offset = range - 1; offset >= 0; offset -= 1) {
    const key = dateKey(new Date(Date.now() - offset * DAY_MS));
    // 缺失日补 0（`records: 0`）—— 这是「当天确实没有记录」，不是「读不到」。
    // 补的是**零值分组**，各分项字段一并归零，避免下游读到 `undefined`。
    days.push(byKey.get(key) ?? emptyDay(key));
  }
  return days;
}

/** 一个「当天无记录」的零值日（补 0 用；字段与 `TokenStatsGroup` 严格同形）。 */
function emptyDay(key: string): TokenStatsGroup {
  return {
    key,
    total: 0,
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    uncachedInput: 0,
    records: 0,
    cacheHitRate: null,
  };
}

function shortLabel(date: string): string {
  return date.length >= 10 ? `${date.slice(5, 7)}/${date.slice(8, 10)}` : date;
}

/**
 * 趋势 · 每日按模型堆叠柱（契约 1：每列分段之和 === 当日总量）。
 *
 * 模型来自 `dailyByModel`；某天某模型的缺失值补 0。若某天**没有**模型明细
 * （旧后端 / 无 `dailyByModel`），退化为单段（哨兵 `TOTAL_SEGMENT`）。
 */
export function dailyModelSeries(source: TokenStatsSource, range: UsageRange): DailyModelColumn[] {
  const days = lastDays(source.daily, range);
  if (days.length === 0) return [];

  const dailyByModel = source.dailyByModel ?? {};
  const lookup = new Map<string, Map<string, number>>();
  const modelTotals = new Map<string, number>();
  for (const [model, points] of Object.entries(dailyByModel)) {
    const byDate = new Map<string, number>();
    for (const point of points) byDate.set(point.key, point.total);
    lookup.set(model, byDate);
    let sum = 0;
    for (const day of days) sum += byDate.get(day.key) ?? 0;
    modelTotals.set(model, sum);
  }
  const orderedModels = [...modelTotals.entries()]
    .filter(([, total]) => total > 0)
    .sort((left, right) => right[1] - left[1])
    .map(([model]) => model);

  return days.map((day) => {
    const segments = orderedModels.map((name) => ({
      name,
      value: lookup.get(name)?.get(day.key) ?? 0,
    }));
    if (segments.length === 0) {
      return {
        day: day.key,
        label: shortLabel(day.key),
        total: day.total,
        segments: day.total > 0 ? [{ name: TOTAL_SEGMENT, value: day.total }] : [],
      };
    }
    // ★ 契约 1：末段吃余数，保证求和精确等于当日总量。
    const sum = segments.reduce((acc, segment) => acc + segment.value, 0);
    const remainder = day.total - sum;
    if (remainder !== 0) {
      const last = segments[segments.length - 1];
      last.value = Math.max(0, last.value + remainder);
    }
    return { day: day.key, label: shortLabel(day.key), total: day.total, segments };
  });
}

/**
 * 趋势 · 7×24 热力图（契约 2）。
 *
 * 从 `hours` 的 `"{weekday}-{hour}"` key 还原；非法 key 忽略，缺失格补 0。
 */
export function heatmapMatrix(source: TokenStatsSource): HeatmapMatrix {
  const cells: number[][] = Array.from({ length: 7 }, () => Array.from({ length: 24 }, () => 0));
  for (const group of source.hours) {
    const [weekdayRaw, hourRaw] = group.key.split("-");
    const weekday = Number.parseInt(weekdayRaw, 10);
    const hour = Number.parseInt(hourRaw, 10);
    if (!Number.isInteger(weekday) || weekday < 0 || weekday > 6) continue;
    if (!Number.isInteger(hour) || hour < 0 || hour > 23) continue;
    cells[weekday][hour] += group.total;
  }
  const max = Math.max(1, ...cells.flat());
  return { rows: 7, cols: 24, cells, max };
}

/** 把某模型的逐日序列对齐到 `days` 的日期窗口。 */
function alignSeries(series: TokenStatsGroup[], days: TokenStatsGroup[]): number[] {
  const byDate = new Map(series.map((point) => [point.key, point.total]));
  return days.map((day) => byDate.get(day.key) ?? 0);
}

/**
 * 模型 · 占比 + 迷你趋势线（契约 3 / 7）。
 *
 * ★ A2：**只取 `dailyByModel`**（区间内明细），**不再回落 `models[].total`**。
 *
 * ## 为什么去掉回落
 *
 * 旧实现无明细时回落 `model.total`。但 `model.total` 是**全时段累计**，
 * 与其余模型的「窗口内」值混进同一张占比图 ⇒ **时间口径不一致**。
 * 更要命的是**同源不一致**：堆叠柱只取 `dailyByModel` 的 key，环形图却遍历 `source.models`
 * ⇒ 实测 `models=[m-a:10, m-orphan:99]`、`dailyByModel` 只有 `m-a` 时，
 * **堆叠 1 段、环形 2 扇区**，原型断言「扇区数 === 模型数」判红。
 *
 * ## 无明细的模型怎么办
 *
 * **不产扇区**。理由：它没有窗口内的值，硬给一个数就是**编造**；
 * 而「它在窗口内是否被用过」正是这张图要回答的问题。
 * 代价是「一个模型都没出现在 `dailyByModel`」时环形图为空 ——
 * 那种情况下堆叠柱同样没有分段，两图**自洽**（原型断言两处都等于 `modelDays.length`）。
 */
export function modelSlices(source: TokenStatsSource, range: UsageRange): ModelSlice[] {
  const days = lastDays(source.daily, range);
  const dailyByModel = source.dailyByModel ?? {};
  const slices: ModelSlice[] = Object.keys(dailyByModel).map((name) => {
    const series = dailyByModel[name];
    const spark = days.length > 0 ? alignSeries(series, days) : [];
    return { name, value: spark.reduce((acc, item) => acc + item, 0), spark };
  });
  return slices.filter((slice) => slice.value > 0).sort((left, right) => right.value - left.value);
}

/** 会话 · 散点（契约 8/9）：x = records（**记录数**），y = total tokens。 */
export function sessionPoints(source: TokenStatsSource): SessionPoint[] {
  return source.sessions
    .filter((session) => session.total > 0 || session.records > 0)
    .map((session) => {
      const rawTitle = displayText(session.title);
      const title = rawTitle && rawTitle.trim().length > 0 ? rawTitle.trim() : session.key;
      return {
        id: session.key,
        title,
        x: session.records,
        y: session.total,
        value: session.total,
      };
    })
    .sort((left, right) => right.value - left.value);
}

/**
 * 积分 · 逐日收支。
 *
 * ★ 入账**不可得**（P3-ARCH §6 #1，已核实）⇒ `inAmt` 恒为 0；
 * 渲染层据此只画**消耗侧**（单向柱）+ 图注说明「入账明细不可得」。
 */
export function creditDailyBars(stats: CreditStatistics): CreditDailyBar[] {
  return [...stats.daily]
    .sort((left, right) => left.date.localeCompare(right.date))
    .map((point) => ({ label: shortLabel(point.date), inAmt: 0, outAmt: point.usage }));
}

/**
 * 积分 · 余额曲线（**回推值**，基线**非 0**）。
 *
 * 后端无逐日余额快照 ⇒ 以 `currentRemaining` 为终值、按逐日净额回推
 * （`inAmt` 恒 0，故净额 = -消耗）。全部为 0 时返回空数组（**无数据不渲染**）。
 * `baseline = min - (max-min)*0.35`（同原型），恒 < min ⇒ 非 0（只要曲线有值）。
 */
export function balanceCurve(bars: CreditDailyBar[], currentRemaining: number): BalancePoint[] {
  if (bars.length === 0) return [];
  const net = bars.map((bar) => bar.inAmt - bar.outAmt);
  const totalNet = net.reduce((acc, value) => acc + value, 0);
  let balance = currentRemaining - totalNet;
  const values = net.map((value) => {
    balance += value;
    return Math.max(0, balance);
  });
  const max = Math.max(...values);
  const min = Math.min(...values);
  if (max === min && max === 0) return [];
  const baseline = Math.max(0, min - (max - min) * 0.35);
  return bars.map((bar, index) => ({ label: bar.label, value: values[index], baseline }));
}

/**
 * 积分 · 到期分布（契约 5：**完整 30 天轴**，无到期的天补 0）。
 *
 * 只统计未到期、且在未来 30 天内的资源；`day = ceil((expireAt - now) / 天)`，钳到 1..30。
 */
export function expiryBuckets(expiry: CreditExpiry, now: number = Date.now()): ExpiryBucket[] {
  const buckets: ExpiryBucket[] = Array.from({ length: 30 }, (_, index) => ({
    day: index + 1,
    value: 0,
  }));
  for (const resource of expiry.resources ?? []) {
    if (resource.expired) continue;
    if (resource.expireAt == null) continue;
    const diff = resource.expireAt - now;
    if (diff <= 0) continue;
    const day = Math.ceil(diff / DAY_MS);
    if (day < 1 || day > 30) continue;
    buckets[day - 1].value += resource.remaining;
  }
  return buckets;
}
