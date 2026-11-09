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
/* ★ 窗口的**定义处**在 `usage-window.ts`（含 `dateKey`）：本模块只是它的一个消费方。
   改造前 `UsageRange = 7 | 14 | 30` 与 `dateKey` 都住在这里 —— 加「今天 / 自定义时间」后
   天数不再能表达用户选的东西（见该文件头），窗口换成联合类型，故下沉。 */
import { usageWindowKeys, type UsageWindow } from "@/lib/usage-window";
import type {
  CreditExpiry,
  CreditStatistics,
  GatewayTokenTrend,
  TokenStatsGroup,
  TokenStatsSource,
  TokenStatsTotals,
} from "@/lib/types";

/** 四视图标识（与原型 `tabs` 一一对应）。 */
export type UsageView = "trend" | "models" | "sessions" | "credits";

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
  /** 轴短标签（`MM/DD`）。 */
  label: string;
  /** 本地日期键 `YYYY-MM-DD` —— 窗口切片要靠它，**不能**靠 `label`。 */
  date: string;
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

/**
 * 网关逐日图的**哨兵段名**（单段柱），由渲染层翻成「网关」。
 *
 * 为什么是哨兵而不是直接穿文案：系列名会进 `ChartConfig` 的键与 legend，
 * 穿本地化字符串进去等于让**数据带语言**（与模型名保持 id 的做法分叉）。
 */
export const GATEWAY_SEGMENT = "__gateway__";

/** 一天毫秒数。⚠️ 只用于「按天回推日期键」这类**整除**场景；
 *  「窗口内有几天」一律走 `usageWindowKeys`（夏令时那天不是 24 小时）。 */
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
    credit: 0,
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
    /* ⚠️ 积分（issue #63）：这里**必须**带上 `credit`。
       `addTotals` 是「逐字白名单」—— 漏一个字段，那个字段就在**合并多个来源**时
       静默归零（`undefined ?? 0`）。实测：`dailyByModel` 的每一点都要过
       `addGroup → addTotals`，漏掉它之后「客户端积分」整块变成 0，
       而单来源的页面（无合并）却看起来正常 —— 极难发现。 */
    credit: (a.credit ?? 0) + (b.credit ?? 0),
  };
}

/**
 * 合并两个分组的**逐日拆分**（`sessions[].daily`）。
 *
 * 两版（如 cn + global 的同名源）各自的逐日必须**按日求和**；任何一侧缺字段
 * （旧后端）就原样保留另一侧 —— 不得凭「有/无」把已有数据丢掉。
 */
function mergeDayList(
  base?: TokenStatsGroup[],
  extra?: TokenStatsGroup[],
): TokenStatsGroup[] | undefined {
  if (!base) return extra;
  if (!extra) return base;
  const map = new Map<string, TokenStatsGroup>();
  for (const group of base) map.set(group.key, { ...group });
  for (const group of extra) {
    const current = map.get(group.key);
    map.set(group.key, current ? addGroup(current, group) : { ...group });
  }
  return [...map.values()];
}

function addGroup(a: TokenStatsGroup, b: TokenStatsGroup): TokenStatsGroup {
  return {
    ...addTotals(a, b),
    key: a.key,
    title: a.title ?? b.title ?? null,
    project: a.project ?? b.project,
    sessionId: a.sessionId ?? b.sessionId,
    // 逐日拆分同样要合并（`sessions[].daily`）；缺字段时保留存在的那侧。
    daily: mergeDayList(a.daily, b.daily),
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

/**
 * ★ A1：窗口内的**日历日**日期键（升序）—— 委托给 `usage-window.ts` 的
 * `usageWindowKeys`，本函数只保留「用量图表」这一层的名字与文档。
 *
 * 给「日均」这类**需要知道窗口天数**的调用方用 —— 分母必须是**窗口天数**，
 * 不能是「有数据的天数」（否则 5 天数据显示成「日均 300」，而真实 30 天日均是 50）。
 *
 * ★ 签名从 `(range: 7|14|30)` 改成 `(window)`：**日期的真值是一对日期键**，
 * 「近 N 天」只是生成它的一种方式。「今天」碰巧是 1 天，但「9/20 – 9/22」是
 * 锚在日期上的一段，用天数拟合会让它随「今天」漂移（见 `usage-window.ts` 文件头）。
 */
export function calendarWindowKeys(window: UsageWindow, now: number = Date.now()): string[] {
  return usageWindowKeys(window, now);
}

/**
 * ★ A1：取窗口内的逐日点（升序），**缺失日补 0**。
 *
 * ## 为什么不 `.slice(-n)`
 *
 * `daily` 是**稀疏**的：后端 `token_stats.rs` 的 `daily: HashMap<String, Totals>`
 * 只在**有记录的日子**落条目（`.or_default()` 不会给缺失日补键）⇒
 * 「最近 N 天」不等于「数组最后 N 项」。
 *
 * 旧实现 `slice(-range)` 的两个实测后果：
 * 1. `daily` 只有 5 天数据而窗口 30 天 ⇒ **只出 5 列**（原型断言要求 `cols.length === 30`）；
 * 2. 窗口 7 天时首列落在 **59 天前** —— 却配着「近 7 天」的标签 ⇒ **图文不符**。
 *
 * ## 修法
 *
 * 按窗口的日期键建轴，缺失日补 0（与 `expiryBuckets` 补 0、`heatmap` 补 0 同一手法）。
 * **后端不动** —— 前端补齐即可，避免动 `HashMap` 契约。
 *
 * ⚠️ 补 0 会改变下游的「空态」判定路径（过去「只有 5 天数据」可能让某些图判空），
 * 这是**预期**的：现在 0 天的柱子**本就该显示为 0**，而不是让窗口悄悄缩成 5 天。
 */
function lastDays(daily: TokenStatsGroup[], window: UsageWindow): TokenStatsGroup[] {
  const byKey = new Map(daily.map((point) => [point.key, point]));
  // 缺失日补 0（`records: 0`）—— 这是「当天确实没有记录」，不是「读不到」。
  // 补的是**零值分组**，各分项字段一并归零，避免下游读到 `undefined`。
  return usageWindowKeys(window).map((key) => byKey.get(key) ?? emptyDay(key));
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

/**
 * 轴短标签：`2026-10-01` → `10/01`；不足 10 位的原样返回。
 *
 * ★ 导出（原为模块私有 `shortLabel`）：**两个数据源共用同一份标签口径** ——
 * WorkBuddy 的 `creditDailyBars` 与 Trae 的 `traeCreditBars` 都要它，
 * 各写一份必然在「补零 / 切位」上分叉。
 */
export function dateAxisLabel(date: string): string {
  return date.length >= 10 ? `${date.slice(5, 7)}/${date.slice(8, 10)}` : date;
}

/**
 * 趋势 · 每日按模型堆叠柱（契约 1：每列分段之和 === 当日总量）。
 *
 * 模型来自 `dailyByModel`；某天某模型的缺失值补 0。若某天**没有**模型明细
 * （旧后端 / 无 `dailyByModel`），退化为单段（哨兵 `TOTAL_SEGMENT`）。
 */
export function dailyModelSeries(source: TokenStatsSource, window: UsageWindow): DailyModelColumn[] {
  const days = lastDays(source.daily, window);
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
        label: dateAxisLabel(day.key),
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
    return { day: day.key, label: dateAxisLabel(day.key), total: day.total, segments };
  });
}

/**
 * 趋势 · **网关**逐日柱（与主堆叠柱同窗对齐；复用 {@link StackedDailyBar} 渲染）。
 *
 * ## 为什么不并进主堆叠柱（`dailyModelSeries`）
 *
 * ① 主柱的「末柱 = 概览统计块 = 账号卡今日合计」是**跨视图一致性契约**，
 *    把网关量并进去会静默改变那个口径（老用户的首屏数字全变）；
 * ② 客户端与网关是**两条独立记录**（配置重叠时同一请求两处各记一次），
 *    求和会让重复计数**看不见** —— 卡片至少在明细里披露，图不能比卡片更不诚实。
 *
 * ## 未记录日怎么画
 *
 * `trend.days` **只含落库日**（App 运行过 + 后台记录在跑）⇒ 缺日是「未记录」而**不是 0**。
 * 这里给缺日**空分段**（不画柱）而不是 0 段：两者视觉相同，但覆盖率由
 * {@link gatewayCoverage} 单独给出、由图注披露 —— 「没有可画的值」与「当天没用量」
 * 必须能区分。
 */
export function gatewayDailyColumns(
  trend: GatewayTokenTrend | null,
  window: UsageWindow,
): DailyModelColumn[] {
  if (!trend) return [];
  const byDate = new Map(trend.days.map((point) => [point.key, point]));
  return usageWindowKeys(window).map((key) => {
    const point = byDate.get(key);
    return {
      day: key,
      label: dateAxisLabel(key),
      total: point?.total ?? 0,
      segments: point && point.total > 0 ? [{ name: GATEWAY_SEGMENT, value: point.total }] : [],
    };
  });
}

/**
 * 网关趋势在窗口内的**覆盖率**（图注披露用）。
 *
 * - `recorded`：窗口内**有落库记录**的天数（含 gateway 量为 0 的 —— 那是**真实 0**）；
 * - `truncated`：其中有几天标了「可能被裁」（那些天的值是**下界**）；
 * - `total`：窗口天数。
 */
export function gatewayCoverage(
  trend: GatewayTokenTrend | null,
  window: UsageWindow,
): { recorded: number; total: number; truncated: number } {
  if (!trend) return { recorded: 0, total: 0, truncated: 0 };
  const byDate = new Map(trend.days.map((point) => [point.key, point]));
  let recorded = 0;
  let truncated = 0;
  const keys = usageWindowKeys(window);
  for (const key of keys) {
    const point = byDate.get(key);
    if (!point) continue;
    recorded += 1;
    if (point.truncated) truncated += 1;
  }
  return { recorded, total: keys.length, truncated };
}

/**
 * 合并两个 region 的网关趋势（`region === "all"` 时前端求和）。
 *
 * 与 {@link mergeSources} 同哲学：**合并作用域是前端的事**，后端命令保持单 region。
 * 同一天两区都有记录时逐字段求和；`truncated` 取**或**（任一侧可能是下界就是下界）。
 */
export function mergeGatewayTrends(
  trends: readonly (GatewayTokenTrend | null)[],
): GatewayTokenTrend | null {
  const present = trends.filter((trend): trend is GatewayTokenTrend => Boolean(trend));
  if (present.length === 0) return null;
  if (present.length === 1) return present[0];
  const byDate = new Map<string, GatewayTokenTrend["days"][number]>();
  for (const trend of present) {
    for (const point of trend.days) {
      const current = byDate.get(point.key);
      if (!current) {
        byDate.set(point.key, { ...point });
        continue;
      }
      current.input += point.input;
      current.output += point.output;
      current.total += point.total;
      current.truncated = current.truncated || point.truncated;
    }
  }
  const days = [...byDate.values()].sort((left, right) => left.key.localeCompare(right.key));
  return {
    date: present[0].date,
    retentionDays: present[0].retentionDays,
    days,
    total: days.reduce((sum, point) => sum + point.total, 0),
  };
}

/**
 * 趋势 · 7×24 热力图（契约 2）。
 *
 * 从 `hours` 的 `"{weekday}-{hour}"` key 还原；非法 key 忽略，缺失格补 0。
 */
export function heatmapMatrix(hours: readonly { key?: string | null; total: number }[]): HeatmapMatrix {
  const cells: number[][] = Array.from({ length: 7 }, () => Array.from({ length: 24 }, () => 0));
  for (const group of hours) {
    const [weekdayRaw, hourRaw] = (group.key ?? "").split("-");
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
export function modelSlices(source: TokenStatsSource, window: UsageWindow): ModelSlice[] {
  const days = lastDays(source.daily, window);
  const dailyByModel = source.dailyByModel ?? {};
  const slices: ModelSlice[] = Object.keys(dailyByModel).map((name) => {
    const series = dailyByModel[name];
    const spark = days.length > 0 ? alignSeries(series, days) : [];
    return { name, value: spark.reduce((acc, item) => acc + item, 0), spark };
  });
  return slices.filter((slice) => slice.value > 0).sort((left, right) => right.value - left.value);
}

/**
 * 会话视图的**渲染上限**（2026-10-12 实测驱动，**有意偏离**原型契约 8）。
 *
 * ## 原型契约 8 说了什么，为什么必须改
 *
 * 契约 8（`docs/specs/P3-ARCH.md`）要求「气泡数 = 会话数，排行条数 = 会话数」——
 * 它假设会话数是**几十**量级。但 `sessions` 是**一个 jsonl 文件一条**：
 * 实测 `~/.workbuddy-ai/projects` 有 **14463** 个文件 ⇒ 会话 ~1.5 万条。
 *
 * 全量渲染的代价（已量化）：`SessionRanking` 每个会话出「排行条 + 明细表行」两组节点
 * ⇒ 2 × 15000 ≈ **19.5 万** DOM 节点；散点再叠 1.5 万个 SVG 点。
 * 纯 innerHTML + 强制布局就要 **0.69s**，React 重建 + recharts 散点还要再翻数倍
 * ⇒ 用户感知为「切到会话 tab 要等很久」。
 *
 * ## 为什么是「截断 + 显式说明」而不是虚拟滚动
 *
 * ① 与其余三视图同构（都是「适配层切数据 → 渲染层画」），不引入新的滚动容器范式；
 * ② 「看完全部 1.5 万个会话」本身没有信息量 —— 会话排行天然只关心头部；
 * ③ **诚实**：截断必须由渲染层**显式说明**（`wbStats.usage.sessions.shownOf`
 *    写明「共 M 个」），不许让用户以为「一共就这么多」。
 *
 * ⚠️ 只截**渲染**，不截**统计**：`SessionsView` 的「会话数 / 会话总 Token /
 * 单会话均值 / 最高消耗」四个 tile 一律按**全量** `sessionPoints` 算 ——
 * 否则数字会随这个常量变化而漂移（那就成了「调样式改数据」）。
 */
export const SESSION_RENDER_LIMIT = 100;

/**
 * 会话 · 散点（契约 8/9）：x = records（**记录数**），y = total tokens。
 *
 * ## ★ 窗口口径（2026-10-15 用户报障的修法）
 *
 * 报障原文：「会话的统计数据切换时间没有变化」—— 改造前本函数**不看窗口**，
 * 四个 tile 与两张图永远是全时口径，切「今天 / 近 7 天 / 自定义」纹丝不动。
 * 而趋势 / 模型两视图早已按窗口切片（`lastDays` / `dailyModelSeries`）——
 * 「切了时间只有会话不变」既是谎报，也是同一页面内的口径分裂。
 *
 * 修法的**素材来自后端**：`sessions[].daily`（逐日拆分，与文件级 `daily` 同判据）。
 * 后端给不了「按窗口的会话合计」（窗口可能锚在任意日期段），所以后端给**逐日**、
 * 前端按窗口键求和 —— 与 dailyByModel 的分工完全同构。
 *
 * ## 回落（旧后端 / 演示数据）
 *
 * `daily` **缺失**（字段不存在的旧后端）⇒ 保留全时口径（宁可少一个能力，不编数据）；
 * `daily` **为空数组**（新后端明确告知「该会话无任何可定位到日期的记录」）⇒ 窗口内
 * 恒为 0，会话**不进任何窗口** —— 与趋势图对无时间戳记录的处理同判据。
 * 两者的区别就是「字段有没有」：`??` 不能替代 `in` 判断，否则空数组会被当成缺失。
 */
export function sessionPoints(source: TokenStatsSource, window: UsageWindow): SessionPoint[] {
  const inWindow = new Set(usageWindowKeys(window));
  return source.sessions
    .map((session) => {
      if (!session.daily) return { session, total: session.total, records: session.records };
      let total = 0;
      let records = 0;
      for (const day of session.daily) {
        if (!inWindow.has(day.key)) continue;
        total += day.total;
        records += day.records;
      }
      return { session, total, records };
    })
    .filter(({ total, records }) => total > 0 || records > 0)
    .map(({ session, total, records }) => {
      const rawTitle = displayText(session.title);
      const title = rawTitle && rawTitle.trim().length > 0 ? rawTitle.trim() : session.key;
      return {
        id: session.key,
        title,
        x: records,
        y: total,
        value: total,
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
/**
 * 积分 · 逐日柱（**按窗口对齐**，缺失日补 0）。
 *
 * ★ 为什么要窗口：改造前是 `bars.slice(-range)` —— 那是「最后 N 个**有数据的**日子」，
 * 不是「最后 N 个**日历日**」。稀疏数据下两者不同：某段日子没有消耗，
 * 「近 30 天」就会静默滑到 35 天前。这与 `lastDays` 修的是同一个 bug，判据同源。
 * ★ 加「自定义时间」后它更是**必须**的：窗口锚在过去的某一段上，`slice` 无法表达。
 */
export function creditDailyBars(stats: CreditStatistics, window: UsageWindow): CreditDailyBar[] {
  /* WB 的入账**不可得**（`events` 无金额字段）⇒ `inAmt` 恒 0，渲染层据此只画消耗侧。 */
  const byDate = new Map(stats.daily.map((point) => [point.date, point.usage]));
  return creditBars(
    usageWindowKeys(window).map((key) => ({ date: key, outAmt: byDate.get(key) ?? 0 })),
  );
}

/**
 * 积分 · 逐日收支（**两个数据源共用**）。
 *
 * `inAmt` 可选：WorkBuddy 的入账不可得（恒 0 ⇒ 渲染层画单向柱）；
 * Trae 的每日快照**两侧都有**（`earned` / `consumed`）⇒ 同一函数直接产出双向柱。
 * 共用的好处是**标签口径与排序只有一份**（各写一份必在补零 / 切位上分叉）。
 */
export function creditBars(
  points: readonly { date: string; outAmt: number; inAmt?: number }[],
): CreditDailyBar[] {
  return [...points]
    .sort((left, right) => left.date.localeCompare(right.date))
    .map((point) => ({
      label: dateAxisLabel(point.date),
      date: point.date,
      inAmt: point.inAmt ?? 0,
      outAmt: point.outAmt,
    }));
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
    /* 剩余 <= 0 的包累加进去只是加 0，但会让「这一档有没有到期积分」的判断依赖无关数据；
       与 `isUsablePack` 同一口径，直接跳过。 */
    if (!(resource.remaining > 0)) continue;
    if (resource.expireAt == null) continue;
    const diff = resource.expireAt - now;
    if (diff <= 0) continue;
    const day = Math.ceil(diff / DAY_MS);
    if (day < 1 || day > 30) continue;
    buckets[day - 1].value += resource.remaining;
  }
  return buckets;
}
