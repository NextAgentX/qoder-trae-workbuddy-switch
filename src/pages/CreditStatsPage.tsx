/**
 * 「用量」工作台 · **积分视图**（P3 重做）。
 *
 * ## 与旧页面的差别
 *
 * 旧页是「官方请求明细 + 按模型分类 + 资源包」三块拼接；本版按原型**积分四视图**重做：
 * 统计 tile（可用 / 入账 / 消耗 / 即将过期）+ 每日收支柱 + 余额曲线 + 到期分布 + 积分流水。
 * 视图参数（范围 + 合并态）由 `UsagePage` **页面级**持有并传入（`UsageViewProps`）。
 *
 * ## ★ 关键取舍（P3-ARCH §6）
 *
 * - **#1 逐条入账可得、逐日聚合仍不可得**：签到流水**逐条**已能显示「+N」
 *   （`events[].amount`，来自上游签到状态额度；取不到时显示 `—`，不猜）；但后端
 *   `daily[]` 只由余额快照推导（**只有消耗侧**）⇒ 每日收支柱仍**只画消耗侧**（单向），
 *   「区间入账」tile 仍显示 `—` 并注明原因；
 * - **#2 账号 Token 占比环形 → 不渲染**（`TokenStatsSource` 无按账号 Token 分组）；
 * - **#3 网关状态码分布环形 → 不渲染**（用量页不持有该数据源）。
 *   即：原型积分页的两个环形**都不画**（数据不支持就不画）。
 *
 * ## 白屏防线
 *
 * 资源包名回落链（`packageName || packageCode`）过 `displayText` —— 后端可能吐对象，
 * 对象当 React 子节点会卸载整棵树（issue #2）。
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { CalendarDays, CircleAlert, Coins, Info, Loader2, RefreshCw, TrendingDown, TrendingUp } from "lucide-react";
import { useShallow } from "zustand/react/shallow";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { ChartFrame } from "@/components/usage/chart-frame";
import { CreditBalanceArea } from "@/components/usage/credit-balance-area";
import { CreditDivergingBar } from "@/components/usage/credit-diverging-bar";
import { CreditExpiryBars } from "@/components/usage/credit-expiry-bars";
import { formatCredits, signedCredits } from "@/components/usage/format";
import { StackedDailyBar } from "@/components/usage/stacked-daily-bar";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import { CreditGapNote, CreditLedgerTable, LEDGER_CLIENT_COLOR, LEDGER_GATEWAY_COLOR } from "@/components/usage/ledger-parts";
import type { UsageViewProps } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { balanceCaption, singleSideCaption } from "@/lib/chart-caption";
import { formatDate } from "@/lib/format";
import { CREDIT_REFRESH_INTERVAL_MS } from "@/lib/use-credit-auto-refresh";
import { useAutoRefreshWhenVisible } from "@/lib/use-auto-refresh";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import type {
  CreditExpiry,
  CreditResource,
  CreditStatistics,
  GatewayUsageDim,
  Region,
  RegionFilter,
  TokenStatsSource,
} from "@/lib/types";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { useScope } from "@/lib/use-scope";
import { balanceCurve, creditDailyBars, expiryBuckets, mergeSources, type DailyModelColumn } from "@/lib/usage-charts";
import {
  LEDGER_CLIENT_SEGMENT,
  LEDGER_GATEWAY_SEGMENT,
  clientCreditByModel,
  clientCreditDaily,
  creditDailySeries,
  creditGap,
  dimTotals,
  mergeDims,
  modelKey,
  showsClient,
  showsGateway,
  type UsageLedger,
} from "@/lib/usage-ledger";
import { useAccountsStore } from "@/stores/accounts";

/** 会话级缓存：切走再切回不重取。`at` = 这份数据被拿到的时刻（「上次刷新」提示的唯一来源）。 */
const statisticsCache: Partial<Record<RegionFilter, { data: CreditStatistics; at: number }>> = {};

/** 文案里的「每 N 分钟」从间隔常量推导 —— 常量改了文案不会说谎。 */
const REFRESH_INTERVAL_MINUTES = Math.round(CREDIT_REFRESH_INTERVAL_MS / 60_000);

/** 资源包名回落链 —— **必须**过 `displayText`（对象当 React 子节点 ⇒ 白屏，issue #2）。 */
function resourceName(resource: CreditResource): string | null {
  return displayText(resource.packageName) || displayText(resource.packageCode) || null;
}

function formatDateTime(timestamp: number | null | undefined): string {
  if (!timestamp) return "—";
  return new Date(timestamp).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default function CreditStatsPage({
  viewWindow,
  combineAll,
  ledger = "all",
}: UsageViewProps & { ledger?: UsageLedger }) {
  const t = useT();
  const [scopeRegion] = useScope("workbuddy");
  const region: RegionFilter = combineAll ? "all" : scopeRegion;
  const [stats, setStats] = useState<CreditStatistics | null>(() => statisticsCache[region]?.data ?? null);
  const [loading, setLoading] = useState(() => !statisticsCache[region]);
  const [error, setError] = useState<string | null>(null);
  /**
   * 「上次刷新」的墙钟（issue #133）。
   *
   * 语义是**事实**而不是状态：只显示「这份数字是什么时候取的」，不据此派生「新鲜 / 过期」，
   * 也不参与任何口径判断。缓存命中时沿用缓存自己的时刻，不刷新成 `Date.now()`。
   */
  const [lastRefreshAt, setLastRefreshAt] = useState<number | null>(
    () => statisticsCache[region]?.at ?? null,
  );
  /* 三处「刷新积分」出口（错误重试 / 空态 / 顶部条）共用同一个 `load(region, true)`。 */
  const { busy, run } = useRefreshAction();

  /* 账本（issue #63）：积分要按**来源**拆消耗，而来源只在请求口径里有
     （客户端 jsonl 的 `usage.credit` / 网关日志的 `credit`）。
     所以这里额外取两样东西：token 统计（客户端积分）与网关三维度（网关积分）。 */
  const [tokenSource, setTokenSource] = useState<TokenStatsSource | null>(null);
  const [dim, setDim] = useState<GatewayUsageDim | null>(null);

  /* ★ 必须带 selector（本页含图表，store 的高频刷新不该牵动整页重渲染）。 */
  const { creditMap, fetchAllRegions } = useAccountsStore(
    useShallow((state) => ({ creditMap: state.creditMap, fetchAllRegions: state.fetchAllRegions })),
  );

  const load = useCallback(
    async (target: RegionFilter, refresh = false, silent = false) => {
      if (!refresh && statisticsCache[target]) {
        const cached = statisticsCache[target]!;
        setStats(cached.data);
        setLastRefreshAt(cached.at);
        setLoading(false);
        setError(null);
        return;
      }
      /* 静默（自动刷新）不点亮 `loading`：那会让顶部按钮转圈、空态闪骨架 ——
         用户没点任何东西，界面不该看起来像在等人操作。 */
      if (!silent) setLoading(true);
      setError(null);
      try {
        // 资源包（到期分布数据源）来自 store；统计来自后端。
        await fetchAllRegions();
        const next = await api.getCreditStatistics(refresh, target);
        const at = Date.now();
        statisticsCache[target] = { data: next, at };
        setStats(next);
        setLastRefreshAt(at);
      } catch (cause) {
        setError(api.asError(cause));
      } finally {
        if (!silent) setLoading(false);
      }
    },
    [fetchAllRegions],
  );

  useEffect(() => {
    void load(region);
  }, [load, region]);

  /**
   * ★ 自动刷新（issue #133）：每 {@link CREDIT_REFRESH_INTERVAL_MS} 重取一轮，切后台即停。
   *
   * 间隔与账号卡的自动刷新**共用同一个常量** —— 两处各写一个数字，必然有一天两边不一致，
   * 于是卡片上的余额和这一页的统计会在同一时刻互相矛盾。
   * 静默刷新失败时保留上一次数据并显示错误条（`error`），不把整页清空。
   */
  const autoRefresh = useCallback(() => load(region, true, true), [load, region]);
  useAutoRefreshWhenVisible(lastRefreshAt ?? 0, autoRefresh);

  /* 请求口径的两份素材（与余额口径**并行**取；任一失败只让该来源缺席，
     不阻塞余额那一半 —— 余额是权威口径，不能因为附加维度取不到就整页报错）。 */
  useEffect(() => {
    let disposed = false;
    const regions: Region[] = region === "all" ? ["cn", "global"] : [region];
    api
      .getTokenStatistics(undefined, region)
      .then((result) => {
        if (!disposed) setTokenSource(mergeSources(result.sources));
      })
      .catch(() => {
        if (!disposed) setTokenSource(null);
      });
    Promise.all(regions.map((item) => api.getGatewayUsageDim(item).catch(() => null)))
      .then((results) => {
        if (!disposed) setDim(mergeDims(results));
      })
      .catch(() => {
        if (!disposed) setDim(null);
      });
    return () => {
      disposed = true;
    };
  }, [region]);

  /* ★ 窗口直接进 `creditDailyBars`（**按日历日对齐、缺失日补 0**）。
     改造前是 `bars.slice(-range)` —— 那是「最后 N 个**有数据的**日子」，不是
     「最后 N 个**日历日**」：某段没消耗的日子会被静默跳过，窗口悄悄变长。
     加「自定义时间」后 `slice` 更是无法表达（窗口锚在过去的某一段上）。 */
  const bars = useMemo(
    () => (stats ? creditDailyBars(stats, viewWindow) : []),
    [stats, viewWindow],
  );
  const totalOut = bars.reduce((sum, bar) => sum + bar.outAmt, 0);

  const balancePoints = useMemo(
    () => (stats ? balanceCurve(bars, stats.summary.currentRemaining) : []),
    [bars, stats],
  );

  /** 到期分布数据源：把各账号的资源包并成一份（`CreditExpiry` 形状）。 */
  const mergedExpiry = useMemo<CreditExpiry>(() => {
    const resources: CreditResource[] = [];
    for (const entry of Object.values(creditMap)) {
      if (entry?.resources) resources.push(...entry.resources);
    }
    return { ok: true, resources };
  }, [creditMap]);

  const buckets = useMemo(() => expiryBuckets(mergedExpiry), [mergedExpiry]);
  const hasExpiry = buckets.some((bucket) => bucket.value > 0);
  const expiringTotal = useMemo(
    () => Object.values(creditMap).reduce((sum, entry) => sum + (entry?.expiringSoonRemaining ?? 0), 0),
    [creditMap],
  );
  const soonestDay = buckets.find((bucket) => bucket.value > 0)?.day;

  const accountCount = stats?.accounts.length ?? 0;
  const hasAnyData =
    Boolean(stats) &&
    (bars.some((bar) => bar.outAmt > 0) || hasExpiry || (stats?.events.length ?? 0) > 0);

  /* ── 请求口径（按来源拆消耗）──
     客户端：`token_stats` 的 `credit`（逐日 / 逐模型）；网关：三维度桶的 `credit`。 */
  const clientCreditMap = useMemo(
    () => (tokenSource ? clientCreditDaily(tokenSource, viewWindow) : new Map<string, number>()),
    [tokenSource, viewWindow],
  );
  const gatewayCreditMap = useMemo(
    () => new Map(creditDailySeries(dim, viewWindow).map((day) => [day.key, day.credit])),
    [dim, viewWindow],
  );
  const creditColumns = useMemo<DailyModelColumn[]>(
    () =>
      bars.map((bar) => {
        const client = showsClient(ledger) ? (clientCreditMap.get(bar.date) ?? 0) : 0;
        const gateway = showsGateway(ledger) ? (gatewayCreditMap.get(bar.date) ?? 0) : 0;
        const segments: { name: string; value: number }[] = [];
        if (client > 0) segments.push({ name: LEDGER_CLIENT_SEGMENT, value: client });
        if (gateway > 0) segments.push({ name: LEDGER_GATEWAY_SEGMENT, value: gateway });
        return { day: bar.date, label: bar.label, total: client + gateway, segments };
      }),
    [bars, clientCreditMap, gatewayCreditMap, ledger],
  );
  const clientCreditRows = useMemo(
    () => (tokenSource ? clientCreditByModel(tokenSource, viewWindow) : []),
    [tokenSource, viewWindow],
  );
  const gatewayCreditRows = useMemo(
    () =>
      dimTotals(dim, viewWindow, modelKey)
        .filter((row) => row.credit > 0)
        .map((row) => ({ name: row.key || t("wbStats.usage.models.unknown"), credit: row.credit, requests: row.requests })),
    [dim, viewWindow, t],
  );
  const clientCreditTotal = clientCreditRows.reduce((sum, row) => sum + row.credit, 0);
  const gatewayCreditTotal = gatewayCreditRows.reduce((sum, row) => sum + row.credit, 0);
  const creditRequestTotal = clientCreditTotal + gatewayCreditTotal;

  const tiles: StatTile[] = [
    {
      icon: Coins,
      label: t("wbStats.usage.credits.available"),
      value: formatCredits(stats?.summary.currentRemaining),
      hint: t("wbStats.usage.credits.accountsHint", { count: accountCount }),
    },
    {
      icon: TrendingUp,
      label: t("wbStats.usage.credits.in"),
      value: "—",
      hint: t("wbStats.usage.caption.singleSide"),
    },
    { icon: TrendingDown, label: t("wbStats.usage.credits.out"), value: signedCredits(totalOut, "-") },
    {
      icon: CalendarDays,
      label: t("wbStats.usage.credits.expiring"),
      value: formatCredits(expiringTotal),
      hint: soonestDay ? `D+${soonestDay}` : undefined,
    },
  ];

  const balanceCaptionText =
    balancePoints.length > 0
      ? balanceCaption(
          t,
          balancePoints[0].value,
          balancePoints[balancePoints.length - 1].value,
          balancePoints[0].baseline,
        )
      : "";

  return (
    /* ⚠️ 页头 / 子 Tab / **视图参数条**都由**上层** `UsagePage` 统一渲染
       （原型 `viewUsage` 只有一个「用量」页头、一条参数条）⇒ 本页只渲染视图内容。 */
    <div className="min-w-0">
      {error && (
        <Alert variant="destructive" className="mb-5">
          <CircleAlert />
          <AlertTitle>{t("wbStats.credit.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-wrap items-center gap-3">
            <span>{error}</span>
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => void run(() => load(region, true))}
            >
              {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("wbStats.credit.retry")}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {loading && !stats ? (
        <div className="space-y-4" aria-busy="true">
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-56 w-full" />
          <Skeleton className="h-40 w-full" />
        </div>
      ) : !stats ? (
        <EmptyState
          icon={CircleAlert}
          title={t("wbStats.usage.empty.credits")}
          description={t("wbStats.usage.empty.creditsHint")}
          action={
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => void run(() => load(region, true))}
            >
              {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("wbStats.usage.empty.refresh")}
            </Button>
          }
        />
      ) : (
        <div className="min-w-0 space-y-6">
          {/* 积分页顶部条（原型 `creditsHeadBar`）：说明 + 「刷新积分」入口。 */}
          <div className="flex min-w-0 flex-wrap items-center gap-3">
            <span className="text-xs text-muted-foreground">{t("wbStats.credit.headBarHint")}</span>
            <span className="text-xs text-muted-foreground">
              {t("shared.credit.autoEvery", { n: REFRESH_INTERVAL_MINUTES })}
            </span>
            {/* 「上次刷新」是**事实**（这份数字何时取的）：用绝对时刻而不是「x 分钟前」——
                相对时间不会自己走，停在那里会一直说「刚刚」。 */}
            {lastRefreshAt !== null && (
              <span className="text-xs tabular-nums text-muted-foreground">
                {t("shared.credit.lastRefresh", { time: formatDate(lastRefreshAt) })}
              </span>
            )}
            <div className="grow" />
            <Button
              size="sm"
              variant="outline"
              onClick={() => void run(() => load(region, true))}
              disabled={busy || loading}
            >
              {busy || loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("wbStats.credit.refreshCredits")}
            </Button>
          </div>

          <StatTiles tiles={tiles} ariaLabel={t("wbStats.credit.overviewAria")} />

          {/* ── 请求口径：**按来源**拆积分消耗（余额口径拆不到来源，见下面这行说明）── */}
          <div className="flex items-center gap-2.5 rounded-[10px] border border-[color:var(--data-series-indigo)]/25 bg-[color:var(--data-series-indigo)]/10 px-3.5 py-2.5 text-xs text-muted-foreground">
            <Info className="size-3.5 shrink-0 text-[color:var(--data-series-indigo)]" />
            <span>{t("wbStats.usage.credits.caliberNote")}</span>
          </div>

          {creditColumns.some((column) => column.total > 0) ? (
            <ChartFrame
              title={t("wbStats.usage.credits.requestCaliber")}
              caption={t("wbStats.usage.credits.dailyHint")}
              ariaLabel={t("wbStats.usage.credits.requestCaliber")}
            >
              <StackedDailyBar
                columns={creditColumns}
                palette={[LEDGER_CLIENT_COLOR, LEDGER_GATEWAY_COLOR]}
                valueFormat={(value) => formatCredits(value)}
              />
            </ChartFrame>
          ) : null}

          <div className="grid min-w-0 gap-4 lg:grid-cols-2">
            {ledger !== "gateway" ? (
              <CreditLedgerTable book="client" rows={clientCreditRows} total={clientCreditTotal} />
            ) : null}
            {ledger !== "client" ? (
              <CreditLedgerTable book="gateway" rows={gatewayCreditRows} total={gatewayCreditTotal} />
            ) : null}
          </div>

          {ledger === "all" && creditRequestTotal > 0 ? (
            <CreditGapNote
              billed={totalOut}
              byRequest={creditRequestTotal}
              client={clientCreditTotal}
              gateway={gatewayCreditTotal}
              overlap={0}
              gap={creditGap(totalOut, creditRequestTotal)}
            />
          ) : null}

          {bars.length > 0 ? (
            <ChartFrame
              title={t("wbStats.usage.credits.dailyTitle")}
              caption={singleSideCaption(t)}
              ariaLabel={t("wbStats.usage.credits.dailyAria")}
            >
              <CreditDivergingBar bars={bars} />
            </ChartFrame>
          ) : null}

          <div className="grid min-w-0 gap-6 lg:grid-cols-2">
            {balancePoints.length > 0 ? (
              <ChartFrame
                title={t("wbStats.usage.credits.balanceTitle")}
                caption={balanceCaptionText}
                ariaLabel={t("wbStats.usage.credits.balanceTitle")}
              >
                <CreditBalanceArea points={balancePoints} />
              </ChartFrame>
            ) : null}
            {hasExpiry ? (
              <ChartFrame
                title={t("wbStats.usage.credits.expiryTitle")}
                caption={t("wbStats.usage.credits.expiryDesc")}
                ariaLabel={t("wbStats.usage.credits.expiryAria")}
              >
                <CreditExpiryBars buckets={buckets} />
              </ChartFrame>
            ) : null}
          </div>

          {stats.events.length > 0 ? (
            <section className="min-w-0 space-y-2.5" aria-labelledby="credit-ledger-title">
              <h2 id="credit-ledger-title" className="px-1 text-[13px] font-medium leading-5">
                {t("wbStats.usage.credits.flowTitle")}
              </h2>
              <Card className="min-w-0 gap-0 overflow-hidden rounded-xl py-0 shadow-none">
                <div className="min-w-0 overflow-x-auto">
                  <table className="w-full min-w-[560px] text-left text-xs">
                    <thead className="bg-muted/45 text-muted-foreground">
                      <tr>
                        <th scope="col" className="px-4 py-2.5 font-medium">
                          {t("wbStats.usage.credits.colTime")}
                        </th>
                        <th scope="col" className="px-3 py-2.5 font-medium">
                          {t("wbStats.usage.credits.colAccount")}
                        </th>
                        <th scope="col" className="px-3 py-2.5 text-right font-medium">
                          {t("wbStats.usage.credits.colAmount")}
                        </th>
                        <th scope="col" className="px-4 py-2.5 font-medium">
                          {t("wbStats.usage.credits.colNote")}
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {stats.events.map((event, index) => {
                        const account = displayText(event.accountName) || t("wbStats.usage.credits.unknownAccount");
                        if (event.kind === "usage") {
                          return (
                            <tr key={`usage-${event.ts}-${index}`} className="border-t border-border/60">
                              <td className="whitespace-nowrap px-4 py-2.5 text-muted-foreground">
                                {formatDateTime(event.ts)}
                              </td>
                              <td className="max-w-[160px] truncate px-3 py-2.5" title={account}>
                                {account}
                              </td>
                              <td className="px-3 py-2.5 text-right tabular-nums text-warning">
                                -{formatCredits(event.amount)}
                              </td>
                              <td className="px-4 py-2.5 text-muted-foreground">
                                {t("wbStats.usage.credits.usageNote")}
                              </td>
                            </tr>
                          );
                        }
                        const note =
                          event.result === "success"
                            ? t("wbStats.usage.credits.checkinSuccess")
                            : event.result === "already"
                              ? t("wbStats.usage.credits.checkinAlready")
                              : t("wbStats.usage.credits.checkinError");
                        // 逐条「+N」：拿到上游额度才显示，拿不到保持「—」（不猜、不摆「+0」）。
                        const reward =
                          typeof event.amount === "number" && event.amount > 0 ? event.amount : null;
                        return (
                          <tr key={`checkin-${event.ts}-${index}`} className="border-t border-border/60">
                            <td className="whitespace-nowrap px-4 py-2.5 text-muted-foreground">
                              {formatDateTime(event.ts)}
                            </td>
                            <td className="max-w-[160px] truncate px-3 py-2.5" title={account}>
                              {account}
                            </td>
                            <td
                              className={
                                reward === null
                                  ? "px-3 py-2.5 text-right tabular-nums text-muted-foreground"
                                  : "px-3 py-2.5 text-right tabular-nums text-success"
                              }
                            >
                              {reward === null ? "—" : `+${formatCredits(reward)}`}
                            </td>
                            <td className="px-4 py-2.5 text-muted-foreground">
                              {note}
                              {displayText(event.error) ? ` · ${displayText(event.error)}` : ""}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </Card>
            </section>
          ) : null}

          {accountCount > 0 ? (
            <section className="min-w-0 space-y-2.5" aria-labelledby="credit-resource-title">
              <h2 id="credit-resource-title" className="px-1 text-[13px] font-medium leading-5">
                {t("wbStats.credit.creditDetailTitle")}
              </h2>
              <Card className="min-w-0 gap-0 overflow-hidden rounded-xl py-0 shadow-none">
                <div className="divide-y divide-border/60">
                  {stats.accounts.map((account) => {
                    const entry = creditMap[account.accountId];
                    const resources = entry?.resources ?? [];
                    const name = displayText(account.accountName) || account.accountId;
                    return (
                      <div key={account.accountId} className="min-w-0 px-4 py-3 sm:px-5">
                        <div className="mb-1.5 flex min-w-0 items-center justify-between gap-2 text-xs">
                          <span className="min-w-0 truncate font-medium" title={name}>
                            {name}
                          </span>
                          <span className="shrink-0 tabular-nums text-muted-foreground">
                            {formatCredits(account.currentRemaining)}
                          </span>
                        </div>
                        {resources.length === 0 ? (
                          <p className="text-[11px] text-muted-foreground">
                            {t("wbStats.credit.noCurrentResource")}
                          </p>
                        ) : (
                          <ul className="space-y-1">
                            {resources.map((resource, index) => {
                              const ratio =
                                resource.total > 0
                                  ? Math.min(100, Math.max(0, (resource.remaining / resource.total) * 100))
                                  : 0;
                              return (
                                <li key={`${resource.packageCode ?? resource.packageName ?? "res"}-${index}`} className="min-w-0">
                                  <div className="flex min-w-0 items-center justify-between gap-2 text-[11px]">
                                    <span className="min-w-0 truncate">
                                      {resourceName(resource) ?? t("wbStats.credit.unnamedResource")}
                                    </span>
                                    <span className="shrink-0 tabular-nums text-muted-foreground">
                                      {formatCredits(resource.remaining)} / {formatCredits(resource.total)}
                                    </span>
                                  </div>
                                  <div className="mt-1 h-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
                                    <div className="h-full rounded-full bg-primary/75" style={{ width: `${ratio}%` }} />
                                  </div>
                                </li>
                              );
                            })}
                          </ul>
                        )}
                      </div>
                    );
                  })}
                </div>
              </Card>
            </section>
          ) : null}

          {!hasAnyData ? (
            <EmptyState
              icon={Coins}
              title={t("wbStats.usage.empty.credits")}
              description={t("wbStats.usage.empty.creditsHint")}
              action={
                <Button size="sm" variant="outline" onClick={() => void load(region, true)}>
                  {t("wbStats.usage.empty.refresh")}
                </Button>
              }
            />
          ) : null}
        </div>
      )}
    </div>
  );
}
