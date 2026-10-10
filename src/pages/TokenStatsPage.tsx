/**
 * 「用量」工作台 · **Token 三视图**（趋势 / 模型 / 会话）—— P3 重做。
 *
 * ## 为什么重写
 *
 * 改造前本页是**旧实现**：4 组视图参数散在页内（来源切片 / 范围 / 区域 / 合并）、
 * 图表张数与图注口径都不是原型的四视图（COVERAGE-REPORT §2.12，12 条 ⚠️）。
 * 本版改为「**装配数据 → 渲染四视图组件**」：视图参数收敛为**一条** `ViewParamBar`
 * （范围 + 合并），图表逻辑全部下沉到 `components/usage/*` 与纯函数 `lib/usage-charts.ts`。
 *
 * ## 数据装配（关键决策）
 *
 * 原型「一个作用域 = 一份用量」⇒ 本页把当前范围下的**全部来源**
 * （workbuddy / codebuddy-cli / codebuddy-ide / workbuddy-ai）用 `mergeSources` 聚合成**一份**。
 * 合并作用域（`region === "all"`）时后端同时下发两版来源，一并求和 —— 因此
 * **图表数据（模型 / 热力图）真的跟着变多**（契约 12），不是只换 days。
 *
 * ## 平台
 *
 * 本页只服务 WorkBuddy（`useScope("workbuddy")`）。Trae 侧的 Token 统计数据源形状不同
 * （`TraeTokenStatistics`），由 `TraeTokenStatsPage` 承担 —— 见 UsagePage 的平台映射。
 *
 * ## 契约（详见 `docs/redesign/P3-ARCH.md`）
 *
 * - 视图参数每页**最多一条**（区域归顶栏 `ScopeSelector`，页内只留合并态 `RegionBar`）；
 * - 空数组**不渲染**该图；整视图空 ⇒ 区块级 `EmptyState`（含出口）；
 * - 区分「真的没有」与「被视图参数筛没」；
 * - 所有展示字段过 `displayText`（白屏防线）。
 */

import { useEffect, useMemo, useState } from "react";
import { CircleAlert, Coins, Gauge, Loader2, MessagesSquare, RefreshCw, Server, Sparkles, TrendingUp, Users } from "lucide-react";

import { RegionBar } from "@/components/region-bar";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { ChartFrame } from "@/components/usage/chart-frame";
import { CumulativeArea } from "@/components/usage/cumulative-area";
import { compactTokens, exactTokens } from "@/components/usage/format";
import { ModelDonut } from "@/components/usage/model-donut";
import { ModelRanking } from "@/components/usage/model-ranking";
import { SessionRanking } from "@/components/usage/session-ranking";
import { SessionScatter } from "@/components/usage/session-scatter";
import { StackedDailyBar } from "@/components/usage/stacked-daily-bar";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import { UsageHeatmap } from "@/components/usage/usage-heatmap";
import { ViewParamBar } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { cumulativeCaption, scatterAxisCaption } from "@/lib/chart-caption";
import { useT } from "@/lib/i18n";
import { regionFilterLabel } from "@/lib/region";
import type { RegionFilter, TokenStatistics, TokenStatsSource } from "@/lib/types";
import { useScope } from "@/lib/use-scope";
import {
  calendarWindowKeys,
  dailyModelSeries,
  heatmapMatrix,
  mergeSources,
  modelSlices,
  sessionPoints,
  type UsageRange,
} from "@/lib/usage-charts";

/** 本页承载的视图（积分视图在 `CreditStatsPage`）。 */
export type TokenUsageView = "trend" | "models" | "sessions";

const TOKEN_REGION_STORAGE_KEY = "buddy-switch:token-stats:region";

/** 会话级缓存：切走再切回不重取（与 `CreditStatsPage` 同一约定）。 */
const statisticsCache: Partial<Record<RegionFilter, TokenStatistics>> = {};

function isRegionFilter(value: unknown): value is RegionFilter {
  return value === "cn" || value === "global" || value === "all";
}

/**
 * 读**旧的**「统计范围」偏好键 —— 只用于**初始化合并态**。
 *
 * ⚠️ `cn` / `global` 已由 `useScope` 接管（URL 优先 + localStorage 记忆），
 * 这里只对存着 `all` 的老用户有意义（`all` 是**视图参数**，不是作用域）。
 * **不再回写**（回写会与 `useScope` 打架，门禁亦禁止 `persistPreferredRegion`）。
 */
function readPreferredRegion(): RegionFilter {
  if (typeof window === "undefined") return "cn";
  try {
    const stored = window.localStorage.getItem(TOKEN_REGION_STORAGE_KEY);
    return isRegionFilter(stored) ? stored : "cn";
  } catch {
    return "cn";
  }
}

/** 视图为空时的统一空态（四要素：icon + title + description + 出口）。 */
function ViewEmpty({
  icon,
  title,
  description,
  actionLabel,
  onAction,
}: {
  icon: typeof MessagesSquare;
  title: string;
  description: string;
  actionLabel: string;
  onAction: () => void;
}) {
  return (
    <EmptyState
      icon={icon}
      title={title}
      description={description}
      action={
        <Button size="sm" variant="outline" onClick={onAction}>
          {actionLabel}
        </Button>
      }
    />
  );
}

/* ────────────────────────────── 趋势 ────────────────────────────── */

function TrendView({
  source,
  range,
  onRangeChange,
}: {
  source: TokenStatsSource;
  range: UsageRange;
  onRangeChange: (range: UsageRange) => void;
}) {
  const t = useT();
  const columns = useMemo(() => dailyModelSeries(source, range), [source, range]);
  const matrix = useMemo(() => heatmapMatrix(source), [source]);
  const slices = useMemo(() => modelSlices(source, range), [source, range]);
  const heatmapHasData = useMemo(
    () => matrix.cells.some((row) => row.some((value) => value > 0)),
    [matrix],
  );

  const rangeRecords = useMemo(
    () => {
      // ★ A1：窗口按**日历日**取（与 `dailyModelSeries` 同源），不再 `slice(-range)`
      //   —— 稀疏数组下那会取到窗口外的历史天。
      const inWindow = new Set(calendarWindowKeys(range));
      return source.daily.reduce(
        (sum, point) => (inWindow.has(point.key) ? sum + point.records : sum),
        0,
      );
    },
    [source.daily, range],
  );
  /** 「被视图参数筛没」：全时段有记录，但当前范围窗口内为 0。 */
  const filtered = source.summary.records > 0 && rangeRecords === 0;

  const total = columns.reduce((sum, column) => sum + column.total, 0);
  /* ★ A1-3：日均的分母是**窗口天数**（`range`），不是「有数据的天数」。
     旧实现除以 `columns.length`（补 0 前 = 有数据的天数）⇒ 实测 1500 Token / 5 天
     显示「日均 300」，而真实 30 天日均应为 50 —— 与 `wbStats.zh.ts` 里
     「按自然日平均」的说明**互相矛盾**。 */
  const average = range > 0 ? Math.round(total / range) : 0;
  const peak = columns.reduce(
    (best, column) => (column.total > best.total ? column : best),
    columns[0] ?? { day: "", label: "", total: 0, segments: [] },
  );
  const cumulative = useMemo(() => {
    let running = 0;
    return columns.map((column) => {
      running += column.total;
      return { label: column.label, value: running };
    });
  }, [columns]);
  const peakDelta = columns.reduce((max, column) => Math.max(max, column.total), 0);

  if (filtered) {
    return (
      <ViewEmpty
        icon={MessagesSquare}
        title={t("wbStats.usage.empty.filtered")}
        description={t("wbStats.usage.empty.filteredHint")}
        actionLabel={t("wbStats.usage.empty.viewAll")}
        onAction={() => onRangeChange(30)}
      />
    );
  }

  const tiles: StatTile[] = [
    { icon: MessagesSquare, label: t("wbStats.usage.trend.total"), value: compactTokens(total), hint: t(`wbStats.usage.range.${range}`) },
    { icon: TrendingUp, label: t("wbStats.usage.trend.avg"), value: compactTokens(average), hint: t("wbStats.usage.trend.avgHint") },
    { icon: Gauge, label: t("wbStats.usage.trend.peak"), value: compactTokens(peak.total), hint: peak.label || undefined },
    { icon: Coins, label: t("wbStats.usage.trend.models"), value: exactTokens(slices.length) },
  ];

  return (
    <div className="min-w-0 space-y-6">
      <StatTiles tiles={tiles} ariaLabel={t("wbStats.usage.view.trend")} />
      {columns.length > 0 ? (
        <ChartFrame
          title={t("wbStats.usage.trend.stackedTitle")}
          caption={t("wbStats.usage.trend.stackedDesc")}
          ariaLabel={t("wbStats.usage.trend.stackedAria")}
        >
          <StackedDailyBar columns={columns} />
        </ChartFrame>
      ) : null}
      <div className="grid min-w-0 gap-6 lg:grid-cols-2">
        {cumulative.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.trend.cumulativeTitle")}
            caption={cumulativeCaption(t, cumulative[cumulative.length - 1].value, peakDelta)}
            ariaLabel={t("wbStats.usage.trend.cumulativeTitle")}
          >
            <CumulativeArea points={cumulative} />
          </ChartFrame>
        ) : null}
        {slices.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.trend.donutTitle")}
            caption={`${exactTokens(slices.length)} · ${compactTokens(total)}`}
            ariaLabel={t("wbStats.usage.trend.donutTitle")}
          >
            <ModelDonut slices={slices} size={150} />
          </ChartFrame>
        ) : null}
      </div>
      {heatmapHasData ? (
        <ChartFrame
          title={t("wbStats.usage.trend.heatmapTitle")}
          caption={t("wbStats.usage.trend.heatmapDesc")}
          ariaLabel={t("wbStats.usage.trend.heatmapAria")}
          exportable={false}
        >
          <UsageHeatmap matrix={matrix} />
        </ChartFrame>
      ) : null}
    </div>
  );
}

/* ────────────────────────────── 模型 ────────────────────────────── */

function ModelsView({
  source,
  range,
  onRefresh,
}: {
  source: TokenStatsSource;
  range: UsageRange;
  onRefresh: () => void;
}) {
  const t = useT();
  const slices = useMemo(() => modelSlices(source, range), [source, range]);
  const total = slices.reduce((sum, slice) => sum + slice.value, 0);
  const top = slices[0];

  if (slices.length === 0) {
    /* 区块级空态（§1.6）——**四要素含出口**：缺出口等于把用户丢在原地。 */
    return (
      <EmptyState
        icon={Server}
        title={t("wbStats.usage.empty.models")}
        description={t("wbStats.usage.empty.modelsHint")}
        action={
          <Button size="sm" variant="outline" onClick={onRefresh}>
            {t("wbStats.usage.empty.refresh")}
          </Button>
        }
      />
    );
  }

  const tiles: StatTile[] = [
    { icon: Server, label: t("wbStats.usage.models.count"), value: exactTokens(slices.length) },
    {
      icon: Sparkles,
      label: t("wbStats.usage.models.top"),
      value: top?.name ?? "—",
      hint: top ? `${((top.value / total) * 100).toFixed(0)}%` : undefined,
    },
    { icon: MessagesSquare, label: t("wbStats.usage.models.total"), value: compactTokens(total) },
    { icon: Gauge, label: t("wbStats.usage.models.avg"), value: compactTokens(Math.round(total / slices.length)) },
  ];

  return (
    <div className="min-w-0 space-y-6">
      <StatTiles tiles={tiles} ariaLabel={t("wbStats.usage.view.models")} />
      <ChartFrame
        title={t("wbStats.usage.models.donutTitle")}
        caption={`${exactTokens(slices.length)} · ${compactTokens(total)}`}
        ariaLabel={t("wbStats.usage.models.donutTitle")}
      >
        <ModelDonut slices={slices} />
      </ChartFrame>
      <ChartFrame
        title={t("wbStats.usage.models.rankTitle")}
        caption={t("wbStats.usage.models.rankDesc")}
        ariaLabel={t("wbStats.usage.models.rankTitle")}
        exportable={false}
      >
        <ModelRanking slices={slices} />
      </ChartFrame>
    </div>
  );
}

/* ────────────────────────────── 会话 ────────────────────────────── */

function SessionsView({ source, onRefresh }: { source: TokenStatsSource; onRefresh: () => void }) {
  const t = useT();
  const points = useMemo(() => sessionPoints(source), [source]);
  const total = points.reduce((sum, point) => sum + point.value, 0);

  if (points.length === 0) {
    /* 区块级空态（§1.6）——**四要素含出口**。 */
    return (
      <EmptyState
        icon={Users}
        title={t("wbStats.usage.empty.sessions")}
        description={t("wbStats.usage.empty.sessionsHint")}
        action={
          <Button size="sm" variant="outline" onClick={onRefresh}>
            {t("wbStats.usage.empty.refresh")}
          </Button>
        }
      />
    );
  }

  const tiles: StatTile[] = [
    { icon: Users, label: t("wbStats.usage.sessions.count"), value: exactTokens(points.length) },
    { icon: MessagesSquare, label: t("wbStats.usage.sessions.total"), value: compactTokens(total) },
    { icon: Gauge, label: t("wbStats.usage.sessions.avg"), value: compactTokens(Math.round(total / points.length)) },
    { icon: TrendingUp, label: t("wbStats.usage.sessions.max"), value: compactTokens(points[0].value), hint: points[0].title },
  ];

  return (
    <div className="min-w-0 space-y-6">
      <StatTiles tiles={tiles} ariaLabel={t("wbStats.usage.view.sessions")} />
      <div className="grid min-w-0 gap-6 lg:grid-cols-2">
        <ChartFrame
          title={t("wbStats.usage.sessions.rankTitle")}
          caption={`${exactTokens(points.length)} · ${compactTokens(total)}`}
          ariaLabel={t("wbStats.usage.sessions.rankTitle")}
          exportable={false}
        >
          <SessionRanking points={points} />
        </ChartFrame>
        <ChartFrame
          title={t("wbStats.usage.sessions.scatterTitle")}
          caption={`${scatterAxisCaption(t)} · ${t("wbStats.usage.sessions.scatterDesc")}`}
          ariaLabel={t("wbStats.usage.sessions.scatterAria")}
        >
          <SessionScatter points={points} />
        </ChartFrame>
      </div>
    </div>
  );
}

/* ────────────────────────────── 页面 ────────────────────────────── */

export default function TokenStatsPage({ view = "trend" }: { view?: TokenUsageView }) {
  const t = useT();
  /* P1：`cn` / `global` 统一走 `useScope`（URL 优先 + localStorage 记忆）。
     ⚠️ 只取值、不取 setter —— 切区域的动作在**侧栏**（`ScopeSwitch`），页内不再有区域选择器。 */
  const [scopeRegion] = useScope("workbuddy");
  const [combineAll, setCombineAll] = useState(() => readPreferredRegion() === "all");
  const region: RegionFilter = combineAll ? "all" : scopeRegion;
  const [range, setRange] = useState<UsageRange>(30);
  const [stats, setStats] = useState<TokenStatistics | null>(() => statisticsCache[region] ?? null);
  const [loading, setLoading] = useState(() => !statisticsCache[region]);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);

  // region 是页面级数据源：切换即重新取数；旧结果保留到新结果返回，避免闪白。
  useEffect(() => {
    let disposed = false;
    setLoading(true);
    setError(null);
    api
      .getTokenStatistics(undefined, region)
      .then((result) => {
        if (disposed) return;
        statisticsCache[region] = result;
        setStats(result);
      })
      .catch((cause) => {
        if (!disposed) setError(api.asError(cause));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [region, reload]);

  const source = useMemo(() => (stats ? mergeSources(stats.sources) : null), [stats]);
  const hasSource = (stats?.sources.length ?? 0) > 0;
  /** 有来源但**一点用量都没有**（区分于「被视图参数筛没」，后者在视图内处理）。 */
  const trulyEmpty =
    Boolean(source) &&
    source!.summary.records === 0 &&
    source!.daily.length === 0 &&
    source!.models.length === 0 &&
    source!.sessions.length === 0;

  return (
    <div className="mx-auto w-full max-w-[1240px] min-w-0 px-7 pt-[26px] pb-[60px]">
      <header className="mb-4 flex min-w-0 flex-wrap items-start justify-between gap-4 sm:mb-5">
        <div className="min-w-0">
          <h2 className="text-xl font-semibold tracking-tight">{t("wbStats.token.pageTitle")}</h2>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
            {regionFilterLabel(region)}
            {region === "all" && t("wbStats.token.bothRegions")}
          </p>
        </div>
        <Button
          className="shrink-0"
          variant="outline"
          size="sm"
          onClick={() => setReload((value) => value + 1)}
          disabled={loading}
        >
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          {t("wbStats.token.refresh")}
        </Button>
      </header>

      {/* 每页**唯一**的视图参数条：范围 + 合并态（区域归侧栏）。 */}
      <div className="mb-6 border-b pb-4">
        <ViewParamBar range={range} onRangeChange={setRange}>
          <RegionBar value={combineAll} onChange={setCombineAll} disabled={loading} />
        </ViewParamBar>
      </div>

      {error && (
        <Alert variant="destructive" className="mb-5">
          <CircleAlert />
          <AlertTitle>{t("wbStats.token.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-wrap items-center gap-3">
            <span>{error}</span>
            <Button size="sm" variant="outline" onClick={() => setReload((value) => value + 1)}>
              {t("wbStats.token.retry")}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {loading && !stats ? (
        <div className="space-y-4" aria-busy="true">
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-64 w-full" />
          <Skeleton className="h-40 w-full" />
        </div>
      ) : !hasSource || !source ? (
        <EmptyState
          icon={CircleAlert}
          title={t("wbStats.usage.empty.noSource")}
          description={t("wbStats.usage.empty.noSourceHint")}
          action={
            <Button size="sm" variant="outline" onClick={() => setReload((value) => value + 1)}>
              {t("wbStats.usage.empty.refresh")}
            </Button>
          }
        />
      ) : trulyEmpty ? (
        <EmptyState
          icon={MessagesSquare}
          title={t("wbStats.usage.empty.noData")}
          description={t("wbStats.usage.empty.noDataHint")}
          action={
            <Button size="sm" variant="outline" onClick={() => setReload((value) => value + 1)}>
              {t("wbStats.usage.empty.refresh")}
            </Button>
          }
        />
      ) : view === "models" ? (
        <ModelsView source={source} range={range} onRefresh={() => setReload((value) => value + 1)} />
      ) : view === "sessions" ? (
        <SessionsView source={source} onRefresh={() => setReload((value) => value + 1)} />
      ) : (
        <TrendView source={source} range={range} onRangeChange={setRange} />
      )}
    </div>
  );
}
