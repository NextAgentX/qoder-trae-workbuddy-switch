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
 * ## 网关逐日图（2026-10-15 新增，独立小图）
 *
 * 主堆叠柱的数据源是 `token_stats`（扫客户端日志）；**网关用量走另一条源**：
 * `get_gateway_token_trend`（读 `token_daily` 的 90 天落库历史）。它渲染成趋势视图里的
 * **独立小图**而不是并进主柱，两条理由（详见 `TrendView` 内注释）：
 * ① 主柱的「末柱 = 概览统计块 = 账号卡今日合计」是**跨视图一致性契约**，并进去会静默改口径；
 * ② 客户端与网关是**两条独立记录**（配置重叠时同一请求两处各记一次），求和会把重复计数藏起来。
 * 小图图注**恒披露覆盖率** —— 历史只覆盖「App 运行过」的日子，未记录日**不是 0**。
 *
 * ## 平台
 *
 * 本页只服务 WorkBuddy（`useScope("workbuddy")`）。Trae 侧的 Token 统计数据源形状不同
 * （`TraeTokenStatistics`），由 `TraeTokenStatsPage` 承担 —— 见 UsagePage 的平台映射。
 *
 * ## 契约（详见 `docs/specs/P3-ARCH.md`）
 *
 * - ★ **视图参数是受控的**（`range` / `combineAll` / `onRangeChange` 由 `UsagePage` 传入）：
 *   本页**不持有**范围与合并态 —— 原型 `viewUsage` 的 `state.range` / `state.merge`
 *   是页面级的，切子 Tab 不重置（见 `UsagePage` 文件头）。参数条也由 `UsagePage` 渲染一次。
 * - 空数组**不渲染**该图；整视图空 ⇒ 区块级 `EmptyState`（含出口）；
 * - 区分「真的没有」与「被视图参数筛没」；
 * - 所有展示字段过 `displayText`（白屏防线）；
 * - ★ **四个视图都必须消费 `viewWindow`**（趋势 / 模型 / 积分接口不变，会话是
 *   2026-10-15 补的口径）：会话此前吃全时数据、切时间纹丝不动（用户报障），
 *   现在按 `sessions[].daily`（后端逐日拆分）在 `sessionPoints` 里按窗口求和；
 * - ★ **会话视图只渲染前 `SESSION_RENDER_LIMIT` 个**（**有意偏离** P3 契约 8
 *   「排行条数 = 气泡数 = 会话数」—— 会话数可达 1.5 万，全量渲染要等数秒；
 *   截断由渲染层显式说明「共 M 个」，统计 tile 仍按全量算。理由见该常量注释）。
 */

import { useEffect, useMemo, useState } from "react";
import { CircleAlert, Gauge, Info, Loader2, MessagesSquare, RefreshCw, Server, TrendingUp, Users } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { ChartFrame } from "@/components/usage/chart-frame";
import { CumulativeArea } from "@/components/usage/cumulative-area";
import { compactTokens, exactTokens } from "@/components/usage/format";
import { CoverageStrip, GatewaySourceTables, LEDGER_CLIENT_COLOR, LEDGER_GATEWAY_COLOR, LedgerModelTable } from "@/components/usage/ledger-parts";
import { ModelDonut } from "@/components/usage/model-donut";
import { ModelRanking } from "@/components/usage/model-ranking";
import { ReconcileSheet } from "@/components/usage/reconcile-sheet";
import { SessionRanking } from "@/components/usage/session-ranking";
import { KeysView } from "@/components/usage/key-ranking";
import { SessionScatter } from "@/components/usage/session-scatter";
import { StackedDailyBar } from "@/components/usage/stacked-daily-bar";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import { UsageHeatmap } from "@/components/usage/usage-heatmap";
import { useUsageWindowLabel, type UsageViewProps } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { cumulativeCaption, scatterAxisCaption } from "@/lib/chart-caption";
import { useT } from "@/lib/i18n";
import type {
  GatewayTokenTrend,
  GatewayUsageDim,
  GatewayUsageRecon,
  Region,
  RegionFilter,
  TokenStatistics,
  TokenStatsSource,
  ApiKeyRecord,
} from "@/lib/types";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { useScope } from "@/lib/use-scope";
import {
  SESSION_RENDER_LIMIT,
  calendarWindowKeys,
  heatmapMatrix,
  mergeGatewayTrends,
  mergeSources,
  modelSlices,
  sessionPoints,
} from "@/lib/usage-charts";
import {
  accountKey,
  clientModelTotals,
  combinedFormula,
  combinedTotal,
  dailyTotalsMap,
  dimCoverage,
  dimTotals,
  endpointKey,
  gatewayDailyInWindow,
  ledgerDailySeries,
  mergeDims,
  mergeRecons,
  modelKey,
  overlapInWindow,
  type UsageLedger,
} from "@/lib/usage-ledger";
import { usageWindowDays, type UsageWindow } from "@/lib/usage-window";

/** 本页承载的视图（积分视图在 `CreditStatsPage`）。 */
export type TokenUsageView = "trend" | "models" | "sessions" | "keys";

/**
 * 「按密钥」视图**只取展示名**：用量来自网关账本（本页已取的 `dim`），
 * 不再读请求日志 —— 那是环形缓冲，窗口对齐不了（见 `src/lib/key-usage.ts`）。
 * 模块级函数 ⇒ 引用稳定（`KeysView` 的 `useEffect` 依赖它）。
 */
async function loadWbKeyNames() {
  const keys = await api.listApiKeys().catch(() => ({ keys: [] as ApiKeyRecord[] }));
  return new Map((keys.keys ?? []).map((key) => [key.id, key.name] as const));
}

/** 会话级缓存：切走再切回不重取（与 `CreditStatsPage` 同一约定）。 */
const statisticsCache: Partial<Record<RegionFilter, TokenStatistics>> = {};

/**
 * 网关逐日趋势的**分 region 缓存**（与 `statisticsCache` 同约定）。
 *
 * 值可以是 `null`（该 region 没有落库历史 / 读取失败）—— 缓存 `null` 与「没取过」
 * 用 `has` 区分，避免每次切回都重发一次注定失败的请求。
 */
const gatewayTrendCache = new Map<RegionFilter, GatewayTokenTrend | null>();

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
  viewWindow,
  onViewWindowChange,
  gatewayTrend,
  dim,
  recon,
  ledger,
  onReconcile,
}: {
  source: TokenStatsSource;
  viewWindow: UsageWindow;
  onViewWindowChange: (next: UsageWindow) => void;
  gatewayTrend: GatewayTokenTrend | null;
  dim: GatewayUsageDim | null;
  recon: GatewayUsageRecon | null;
  ledger: UsageLedger;
  onReconcile: () => void;
}) {
  const t = useT();
  const windowLabel = useUsageWindowLabel();
  const keys = useMemo(() => calendarWindowKeys(viewWindow), [viewWindow]);
  const matrix = useMemo(() => heatmapMatrix(source.hours), [source]);

  /* 两本账的逐日值：客户端来自 `source.daily`，网关来自 `get_gateway_token_trend`。 */
  const clientDaily = useMemo(() => dailyTotalsMap(source.daily), [source.daily]);
  const gatewayDaily = useMemo(
    () => new Map(gatewayDailyInWindow(gatewayTrend, viewWindow).map((day) => [day.key, day.total])),
    [gatewayTrend, viewWindow],
  );
  const columns = useMemo(
    () =>
      ledgerDailySeries({
        days: keys.map((key) => ({ key, total: 0 })),
        clientTotals: clientDaily,
        gatewayTotals: gatewayDaily,
        ledger,
        labelOf: (key) => key.slice(5).replace("-", "/"),
      }),
    [keys, clientDaily, gatewayDaily, ledger],
  );

  const sumOf = (map: Map<string, number>) => keys.reduce((sum, key) => sum + (map.get(key) ?? 0), 0);
  const clientTokens = sumOf(clientDaily);
  const gatewayTokens = sumOf(gatewayDaily);
  const overlap = useMemo(() => overlapInWindow(recon, clientTokens), [recon, clientTokens]);
  const combined = combinedTotal(clientTokens, gatewayTokens, overlap);
  const formula = useMemo(() => {
    const parts = combinedFormula({
      clientTokens,
      gatewayTokens,
      overlapTokens: overlap,
      verdict: recon?.verdict ?? null,
    });
    return parts.kind === "subset"
      ? t("wbStats.usage.trend.formulaSubset", {
          gateway: compactTokens(gatewayTokens),
          direct: compactTokens(parts.directClientTokens),
        })
      : t("wbStats.usage.trend.formulaAdditive", {
          client: compactTokens(clientTokens),
          gateway: compactTokens(gatewayTokens),
          overlap: compactTokens(parts.overlapTokens),
        });
  }, [clientTokens, gatewayTokens, overlap, recon?.verdict, t]);

  const visibleTotal = ledger === "client" ? clientTokens : ledger === "gateway" ? gatewayTokens : combined;
  const total = visibleTotal;
  const dailyTotals = columns.map((column) => column.total);
  const peak = Math.max(0, ...dailyTotals);
  /* ★ A1-3：日均的分母是**窗口天数**，不是「有数据的天数」——
     后者在稀疏数据下会把 5 天用量显示成「日均 300」，而真实 30 天日均是 50。 */
  const average = Math.round(total / usageWindowDays(viewWindow));

  const rangeRecords = useMemo(() => {
    const inWindow = new Set(keys);
    return source.daily.reduce((sum, point) => (inWindow.has(point.key) ? sum + point.records : sum), 0);
  }, [source.daily, keys]);
  const filtered = source.summary.records > 0 && rangeRecords === 0 && gatewayTokens === 0;

  /* 模型占比：**按单本账各画一个环**（跨账本合并占比的分母含重复计数，是假的）。 */
  const clientSlices = useMemo(
    () =>
      clientModelTotals(source, viewWindow).map((model) => ({
        name: model.name,
        value: model.tokens,
        spark: [],
      })),
    [source, viewWindow],
  );
  const gatewaySlices = useMemo(
    () =>
      dimTotals(dim, viewWindow, modelKey)
        .filter((model) => model.tokens > 0)
        .map((model) => ({ name: model.key || t("wbStats.usage.models.unknown"), value: model.tokens, spark: [] })),
    [dim, viewWindow, t],
  );

  const coverage = useMemo(() => dimCoverage(dim, viewWindow), [dim, viewWindow]);
  const heatmapHasData = useMemo(() => matrix.cells.some((row) => row.some((value) => value > 0)), [matrix]);
  /* ★ 累计曲线必须在本函数**所有** hook 之后才能算 —— 但它自己就是 hook，
     所以只能放在早退**之前**。放在 `if (filtered) return` 之后会让 hook 数量随
     `filtered` 变化：点击「今天」从「有数据」切到「本窗口无数据」时 React 抛
     #300（Rendered more hooks than during the previous render），整页白屏（issue #131）。 */
  const cumulative = useMemo(() => {
    let running = 0;
    return columns.map((column) => {
      running += column.total;
      return { label: column.label, value: running };
    });
  }, [columns]);

  if (filtered) {
    return (
      <ViewEmpty
        icon={MessagesSquare}
        title={t("wbStats.usage.empty.filtered")}
        description={t("wbStats.usage.empty.filteredHint")}
        actionLabel={t("wbStats.usage.empty.viewAll")}
        onAction={() => onViewWindowChange({ kind: "preset", days: 30 })}
      />
    );
  }

  const totalTile: StatTile =
    ledger === "gateway"
      ? {
          icon: Server,
          label: t("wbStats.usage.trend.totalGateway"),
          value: compactTokens(gatewayTokens),
          hint: windowLabel(viewWindow),
          title: t("wbStats.usage.trend.totalGatewayTitle"),
        }
      : ledger === "client"
        ? {
            icon: MessagesSquare,
            label: t("wbStats.usage.trend.totalClient"),
            value: compactTokens(clientTokens),
            hint: windowLabel(viewWindow),
            title: t("wbStats.usage.trend.totalClientTitle"),
          }
        : {
            icon: MessagesSquare,
            label: t("wbStats.usage.trend.totalCombined"),
            value: `≈ ${compactTokens(combined)}`,
            hint: formula,
            title: t("wbStats.usage.trend.totalCombinedTitle"),
          };

  const modelCount =
    ledger === "gateway" ? gatewaySlices.length : ledger === "client" ? clientSlices.length : clientSlices.length + gatewaySlices.length;
  const tiles: StatTile[] = [
    totalTile,
    { icon: TrendingUp, label: t("wbStats.usage.trend.avg"), value: compactTokens(average), hint: t("wbStats.usage.trend.avgHint") },
    { icon: Gauge, label: t("wbStats.usage.trend.peak"), value: compactTokens(peak) },
    /* 决策 #4：WB 的第四张卡是「模型数」，不是「活跃账号」—— 账号维度在账号页，
       这里放模型数才能回答「这段时间动了几个模型」。 */
    { icon: Server, label: t("wbStats.usage.trend.models"), value: exactTokens(modelCount) },
    {
      icon: MessagesSquare,
      label: t("wbStats.usage.trend.requests"),
      value: exactTokens(
        ledger === "gateway"
          ? dimTotals(dim, viewWindow, endpointKey).reduce((sum, row) => sum + row.requests, 0)
          : rangeRecords,
      ),
      hint: t("wbStats.usage.trend.requestsHint"),
    },
  ];

  return (
    <div className="min-w-0 space-y-6">
      <StatTiles tiles={tiles} ariaLabel={t("wbStats.usage.view.trend")} />
      <CoverageStrip
        clientTokens={clientTokens}
        clientRequests={recon?.clientRows ?? rangeRecords}
        gatewayTokens={gatewayTokens}
        gatewayRequests={recon?.gatewayRows ?? 0}
        gatewayFailed={dimTotals(dim, viewWindow, endpointKey).reduce((sum, row) => sum + row.failed, 0)}
        recorded={coverage.recorded}
        windowDays={coverage.total}
        overlapRows={recon?.matched ?? 0}
        overlapTokens={overlap}
        onReconcile={recon ? onReconcile : undefined}
      />
      {columns.some((column) => column.total > 0) ? (
        <ChartFrame
          title={t("wbStats.usage.trend.stackedTitle")}
          caption={t("wbStats.usage.trend.ledgerCaption")}
          ariaLabel={t("wbStats.usage.trend.stackedAria")}
        >
          <StackedDailyBar columns={columns} palette={[LEDGER_CLIENT_COLOR, LEDGER_GATEWAY_COLOR]} />
        </ChartFrame>
      ) : null}
      <div className="grid min-w-0 gap-6 lg:grid-cols-2">
        {cumulative.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.trend.cumulativeTitle")}
            caption={cumulativeCaption(t, cumulative[cumulative.length - 1].value, peak)}
            ariaLabel={t("wbStats.usage.trend.cumulativeTitle")}
          >
            <CumulativeArea points={cumulative} />
          </ChartFrame>
        ) : null}
        {ledger !== "gateway" && clientSlices.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.trend.donutTitle")}
            caption={`${exactTokens(clientSlices.length)} · ${compactTokens(clientTokens)}`}
            ariaLabel={t("wbStats.usage.trend.donutTitle")}
            exportable={false}
          >
            <ModelDonut slices={clientSlices} size={150} />
          </ChartFrame>
        ) : null}
        {ledger !== "client" && gatewaySlices.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.models.gatewayBook")}
            caption={`${exactTokens(gatewaySlices.length)} · ${compactTokens(gatewayTokens)}`}
            ariaLabel={t("wbStats.usage.models.gatewayBook")}
            exportable={false}
          >
            <ModelDonut slices={gatewaySlices} size={150} />
          </ChartFrame>
        ) : null}
      </div>
      {ledger !== "gateway" && heatmapHasData ? (
        <ChartFrame
          title={t("wbStats.usage.trend.heatmapTitle")}
          caption={t("wbStats.usage.trend.heatmapDesc")}
          ariaLabel={t("wbStats.usage.trend.heatmapAria")}
          exportable={false}
        >
          <UsageHeatmap matrix={matrix} />
        </ChartFrame>
      ) : null}
      {ledger === "gateway" ? (
        <p className="text-[11.5px] text-muted-foreground">{t("wbStats.usage.trend.gatewayNoHours")}</p>
      ) : null}
    </div>
  );
}

/* ────────────────────────────── 模型 ────────────────────────────── */

function ModelsView({
  source,
  viewWindow,
  dim,
  ledger,
  onRefresh,
}: {
  source: TokenStatsSource;
  viewWindow: UsageWindow;
  dim: GatewayUsageDim | null;
  ledger: UsageLedger;
  onRefresh: () => void | Promise<void>;
}) {
  const t = useT();
  const { busy, run } = useRefreshAction();
  const coverage = useMemo(() => dimCoverage(dim, viewWindow), [dim, viewWindow]);
  const clientRows = useMemo(
    () =>
      clientModelTotals(source, viewWindow).map((model) => ({
        name: model.name,
        tokens: model.tokens,
        requests: model.requests,
      })),
    [source, viewWindow],
  );
  const gatewayRows = useMemo(
    () =>
      dimTotals(dim, viewWindow, modelKey).map((model) => ({
        name: model.key || t("wbStats.usage.models.unknown"),
        tokens: model.tokens,
        requests: model.requests,
        failed: model.failed,
      })),
    [dim, viewWindow, t],
  );
  const clientTotal = clientRows.reduce((sum, row) => sum + row.tokens, 0);
  const gatewayTotal = gatewayRows.reduce((sum, row) => sum + row.tokens, 0);
  const gatewayFailed = gatewayRows.reduce((sum, row) => sum + (row.failed ?? 0), 0);

  if (clientRows.length === 0 && gatewayRows.length === 0) {
    return (
      <EmptyState
        icon={Server}
        title={t("wbStats.usage.empty.models")}
        description={t("wbStats.usage.empty.modelsHint")}
        action={
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(() => onRefresh())}>
            {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("wbStats.usage.empty.refresh")}
          </Button>
        }
      />
    );
  }

  return (
    <div className="min-w-0 space-y-4">
      {ledger === "all" ? (
        <div className="flex items-center gap-2.5 rounded-[10px] border border-[color:var(--data-series-indigo)]/25 bg-[color:var(--data-series-indigo)]/10 px-3.5 py-2.5 text-xs text-muted-foreground">
          <Info className="size-3.5 shrink-0 text-[color:var(--data-series-indigo)]" />
          <span>
            <b className="font-medium text-foreground">{t("wbStats.usage.models.noMerge")}</b>
            {t("wbStats.usage.models.noMergeBody")}
          </span>
        </div>
      ) : null}
      {/* 客户端账本用**排行条 + 迷你趋势线**：它有 `dailyByModel`，能画出「每个模型这段
          时间怎么走」；网关账本用表（要 `请求 / 失败` 两列，那是网关才有的事实）。 */}
      {ledger !== "gateway" ? (
        <ChartFrame
          title={t("wbStats.usage.models.clientBook")}
          caption={`${t("wbStats.usage.models.clientBookHint")} · ${compactTokens(clientTotal)}`}
          ariaLabel={t("wbStats.usage.models.clientBook")}
          exportable={false}
        >
          <ModelRanking slices={modelSlices(source, viewWindow)} />
        </ChartFrame>
      ) : null}
      {ledger !== "client" ? (
        <LedgerModelTable
          book="gateway"
          rows={gatewayRows}
          total={gatewayTotal}
          caption={t("wbStats.usage.models.gatewayBookHint", {
            recorded: coverage.recorded,
            total: coverage.total,
          })}
          failed={gatewayFailed}
        />
      ) : null}
    </div>
  );
}

/* ────────────────────────────── 会话 ────────────────────────────── */

function SessionsView({
  source,
  viewWindow,
  dim,
  ledger,
  onViewWindowChange,
  onRefresh,
  onSwitchLedger,
}: {
  source: TokenStatsSource;
  viewWindow: UsageWindow;
  dim: GatewayUsageDim | null;
  ledger: UsageLedger;
  onViewWindowChange: (next: UsageWindow) => void;
  onRefresh: () => void | Promise<void>;
  onSwitchLedger: (next: UsageLedger) => void;
}) {
  const t = useT();
  const windowLabel = useUsageWindowLabel();
  const { busy, run } = useRefreshAction();
  const points = useMemo(() => sessionPoints(source, viewWindow), [source, viewWindow]);
  const visible = useMemo(() => points.slice(0, SESSION_RENDER_LIMIT), [points]);
  const total = points.reduce((sum, point) => sum + point.value, 0);
  const capNote =
    points.length > visible.length
      ? t("wbStats.usage.sessions.shownOf", { shown: visible.length, total: points.length })
      : null;

  /* 网关账本：**换维度**（网关没有 sessionId，但有账号与端点）。 */
  if (ledger === "gateway") {
    const accounts = dimTotals(dim, viewWindow, accountKey);
    const endpoints = dimTotals(dim, viewWindow, endpointKey);
    return (
      <GatewaySourceTables
        accounts={accounts.map((row) => ({ key: row.key, tokens: row.tokens, requests: row.requests }))}
        endpoints={endpoints.map((row) => ({ key: row.key, requests: row.requests }))}
        gatewayTokens={accounts.reduce((sum, row) => sum + row.tokens, 0)}
        gatewayRequests={endpoints.reduce((sum, row) => sum + row.requests, 0)}
      />
    );
  }

  if (points.length === 0) {
    const hasAnySession = source.sessions.some((session) => session.total > 0 || session.records > 0);
    if (hasAnySession) {
      return (
        <ViewEmpty
          icon={Users}
          title={t("wbStats.usage.empty.filtered")}
          description={t("wbStats.usage.empty.filteredHint")}
          actionLabel={t("wbStats.usage.empty.viewAll")}
          onAction={() => onViewWindowChange({ kind: "preset", days: 30 })}
        />
      );
    }
    return (
      <EmptyState
        icon={Users}
        title={t("wbStats.usage.empty.sessions")}
        description={t("wbStats.usage.empty.sessionsHint")}
        action={
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(() => onRefresh())}>
            {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("wbStats.usage.empty.refresh")}
          </Button>
        }
      />
    );
  }

  const tiles: StatTile[] = [
    { icon: Users, label: t("wbStats.usage.sessions.count"), value: exactTokens(points.length), hint: windowLabel(viewWindow) },
    { icon: MessagesSquare, label: t("wbStats.usage.sessions.total"), value: compactTokens(total), hint: windowLabel(viewWindow) },
    { icon: Gauge, label: t("wbStats.usage.sessions.avg"), value: compactTokens(Math.round(total / points.length)) },
    { icon: TrendingUp, label: t("wbStats.usage.sessions.max"), value: compactTokens(points[0].value), hint: points[0].title },
  ];

  return (
    <div className="min-w-0 space-y-6">
      {ledger === "all" ? (
        <div className="flex items-center gap-2.5 rounded-[10px] border border-[color:var(--data-series-indigo)]/25 bg-[color:var(--data-series-indigo)]/10 px-3.5 py-2.5 text-xs text-muted-foreground">
          <Info className="size-3.5 shrink-0 text-[color:var(--data-series-indigo)]" />
          <span>
            {t("wbStats.usage.sessions.gatewayMissing")}
            {" · "}
            <button
              type="button"
              onClick={() => onSwitchLedger("gateway")}
              className="cursor-pointer border-b border-dotted border-current text-[color:var(--data-series-indigo)] hover:text-foreground"
            >
              {t("wbStats.usage.sessions.jumpGateway")}
            </button>
          </span>
        </div>
      ) : null}
      <StatTiles tiles={tiles} ariaLabel={t("wbStats.usage.view.sessions")} />
      <div className="grid min-w-0 gap-6 lg:grid-cols-2">
        <ChartFrame
          title={t("wbStats.usage.sessions.rankTitle")}
          caption={capNote ?? `${exactTokens(points.length)} · ${compactTokens(total)}`}
          ariaLabel={t("wbStats.usage.sessions.rankTitle")}
          exportable={false}
        >
          <SessionRanking points={visible} />
        </ChartFrame>
        <ChartFrame
          title={t("wbStats.usage.sessions.scatterTitle")}
          caption={`${scatterAxisCaption(t)} · ${t("wbStats.usage.sessions.scatterDesc")}${capNote ? ` · ${capNote}` : ""}`}
          ariaLabel={t("wbStats.usage.sessions.scatterAria")}
        >
          <SessionScatter points={visible} />
        </ChartFrame>
      </div>
    </div>
  );
}

/* ────────────────────────────── 页面 ────────────────────────────── */

export default function TokenStatsPage({
  view = "trend",
  viewWindow,
  combineAll,
  onViewWindowChange,
  ledger = "all",
  onLedgerChange,
}: { view?: TokenUsageView; ledger?: UsageLedger; onLedgerChange?: (next: UsageLedger) => void } & UsageViewProps) {
  const t = useT();
  /* P1：`cn` / `global` 统一走 `useScope`（URL 优先 + localStorage 记忆）。
     ⚠️ 只取值、不取 setter —— 切区域的动作在**侧栏**（`ScopeSwitch`），页内不再有区域选择器。 */
  const [scopeRegion] = useScope("workbuddy");
  const region: RegionFilter = combineAll ? "all" : scopeRegion;
  const [stats, setStats] = useState<TokenStatistics | null>(() => statisticsCache[region] ?? null);
  const [loading, setLoading] = useState(() => !statisticsCache[region]);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  /* 三处「重试 / 刷新」出口（错误 Alert、两种空态）共用同一个 reload 触发 ⇒ 共用一个实例。 */
  const { busy: reloadBusy, run: runReload } = useRefreshAction();
  const [gatewayTrend, setGatewayTrend] = useState<GatewayTokenTrend | null>(
    () => gatewayTrendCache.get(region) ?? null,
  );
  /* 账本（issue #63）：三维度历史 + 今日对账。两者都与统计**并行**取，
     单区失败只让该区缺席（渐进增强，不弹错噪屏）。 */
  const [dim, setDim] = useState<GatewayUsageDim | null>(null);
  const [recon, setRecon] = useState<GatewayUsageRecon | null>(null);
  const [reconOpen, setReconOpen] = useState(false);

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
    /* 网关逐日趋势与统计**并行**取（互不依赖）：单 region 命令，`all` 时前端分区取再合并
       （与 `mergeSources` 同哲学）。单区失败只让该区缺席（`.catch(() => null)`）——
       整条失败 ⇒  trend 为 null ⇒ 网关小图不渲染（渐进增强，不弹错噪屏）。
       ⚠️ 老服务端没有这条路由时会 404：同样按「不出图」处理，不影响主统计。 */
    const gatewayRegions: Region[] = region === "all" ? ["cn", "global"] : [region];
    Promise.all(
      gatewayRegions.map((gatewayRegion) =>
        api.getGatewayTokenTrend(gatewayRegion).catch(() => null),
      ),
    )
      .then((results) => {
        if (disposed) return;
        const merged = mergeGatewayTrends(results);
        gatewayTrendCache.set(region, merged);
        setGatewayTrend(merged);
      })
      .catch(() => {
        if (!disposed) setGatewayTrend(null);
      });
    /* 三维度历史：与趋势同源（`token_daily`），但它是**模型 / 账号 / 端点**的拆分。
       `all` 时分区取再合并（与 `mergeGatewayTrends` 同哲学）。 */
    Promise.all(
      gatewayRegions.map((gatewayRegion) => api.getGatewayUsageDim(gatewayRegion).catch(() => null)),
    )
      .then((results) => {
        if (!disposed) setDim(mergeDims(results));
      })
      .catch(() => {
        if (!disposed) setDim(null);
      });
    /* 对账：只覆盖今日（网关日志是环形窗口）。`all` 时两区各对一次再合并。 */
    Promise.all(
      gatewayRegions.map((gatewayRegion) => api.getUsageReconcile(gatewayRegion).catch(() => null)),
    )
      .then((results) => {
        if (!disposed) setRecon(mergeRecons(results));
      })
      .catch(() => {
        if (!disposed) setRecon(null);
      });
    return () => {
      disposed = true;
    };
  }, [region, reload]);

  const source = useMemo(() => (stats ? mergeSources(stats.sources) : null), [stats]);
  const hasSource = (stats?.sources.length ?? 0) > 0;
  /* 网关用量（`gatewayTrend`，扫 `gateway_logs`）是客户端（扫 `projects/*.jsonl`）
     之外的另一条记录源，按窗口取。 */
  const gatewayInWindow = useMemo(() => {
    const keys = new Set(calendarWindowKeys(viewWindow));
    return (gatewayTrend?.days ?? []).reduce(
      (sum, day) => (keys.has(day.key) ? sum + day.total : sum),
      0,
    );
  }, [gatewayTrend, viewWindow]);
  /**
   * 有来源但**两个数据源都翻不出用量** ⇒ 整页空态（区分于「被视图参数筛没」，后者在视图内处理）。
   *
   * ⚠️ 两侧口径**故意不对称**，勿强行统一：
   * - 网关侧按窗口判：`gatewayTrend` 含 90 天落库历史，不筛窗会把窗外历史算成
   *   「有数据」，而当前窗口其实为空。
   * - 客户端侧按全量判：`getTokenStatistics` 未按窗口传参（`days` 缺省 ⇒ 后端
   *   不加 cutoff），窗口内被筛没的由各视图自行展示 ⇒ 这里只需「有没有任何来源
   *   记录」来决定是否渲染页面，避免空态把可能有值的视图整片吞掉。 */
  const clientHasRecords = (source?.summary.records ?? 0) > 0;
  const trulyEmpty = Boolean(source) && !clientHasRecords && gatewayInWindow === 0;

  return (
    /* ⚠️ 页头 / 子 Tab / **视图参数条**都由**上层** `UsagePage` 统一渲染
       （原型 `viewUsage` 只有一个「用量」页头、一条参数条）⇒ 本页只渲染视图内容。
       `view` 决定画趋势 / 模型 / 会话哪一个。 */
    <div className="min-w-0">
      {error && (
        <Alert variant="destructive" className="mb-5">
          <CircleAlert />
          <AlertTitle>{t("wbStats.token.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-wrap items-center gap-3">
            <span>{error}</span>
            <Button
              size="sm"
              variant="outline"
              disabled={reloadBusy}
              onClick={() => void runReload(() => setReload((value) => value + 1))}
            >
              {reloadBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
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
            <Button
              size="sm"
              variant="outline"
              disabled={reloadBusy}
              onClick={() => void runReload(() => setReload((value) => value + 1))}
            >
              {reloadBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
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
            <Button
              size="sm"
              variant="outline"
              disabled={reloadBusy}
              onClick={() => void runReload(() => setReload((value) => value + 1))}
            >
              {reloadBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("wbStats.usage.empty.refresh")}
            </Button>
          }
        />
      ) : view === "models" ? (
        <ModelsView
          source={source}
          viewWindow={viewWindow}
          dim={dim}
          ledger={ledger}
          onRefresh={() => setReload((value) => value + 1)}
        />
      ) : view === "keys" ? (
        <KeysView
          dim={dim}
          window={viewWindow}
          selfScope="wb"
          loadNames={loadWbKeyNames}
        />
      ) : view === "sessions" ? (
        <SessionsView
          source={source}
          viewWindow={viewWindow}
          dim={dim}
          ledger={ledger}
          onViewWindowChange={onViewWindowChange}
          onRefresh={() => setReload((value) => value + 1)}
          onSwitchLedger={() => onLedgerChange?.("gateway")}
        />
      ) : (
        <TrendView
          source={source}
          viewWindow={viewWindow}
          onViewWindowChange={onViewWindowChange}
          gatewayTrend={gatewayTrend}
          dim={dim}
          recon={recon}
          ledger={ledger}
          onReconcile={() => setReconOpen(true)}
        />
      )}
      <ReconcileSheet open={reconOpen} onOpenChange={setReconOpen} recon={recon} />
    </div>
  );
}
