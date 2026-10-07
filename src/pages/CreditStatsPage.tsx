/**
 * 「用量」工作台 · **积分视图**（P3 重做）。
 *
 * ## 与旧页面的差别
 *
 * 旧页是「官方请求明细 + 按模型分类 + 资源包」三块拼接；本版按原型**积分四视图**重做：
 * 统计 tile（可用 / 入账 / 消耗 / 即将过期）+ 每日收支柱 + 余额曲线 + 到期分布 + 积分流水。
 * 视图参数收敛为**一条** `ViewParamBar`（范围 + 合并）。
 *
 * ## ★ 关键取舍（P3-ARCH §6）
 *
 * - **#1 入账不可得**（已核实 `events.kind==="checkin"` 无金额字段、且被裁剪）⇒
 *   每日收支柱**只画消耗侧**（单向），「区间入账」tile 显示 `—` 并注明原因；
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
import { CalendarDays, CircleAlert, Coins, Loader2, RefreshCw, TrendingDown, TrendingUp } from "lucide-react";
import { useShallow } from "zustand/react/shallow";

import { RegionBar } from "@/components/region-bar";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { ChartFrame } from "@/components/usage/chart-frame";
import { CreditBalanceArea } from "@/components/usage/credit-balance-area";
import { CreditDivergingBar } from "@/components/usage/credit-diverging-bar";
import { CreditExpiryBars } from "@/components/usage/credit-expiry-bars";
import { formatCredits } from "@/components/usage/format";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import { ViewParamBar } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { balanceCaption, singleSideCaption } from "@/lib/chart-caption";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import { regionFilterLabel } from "@/lib/region";
import type { CreditExpiry, CreditResource, CreditStatistics, RegionFilter } from "@/lib/types";
import { useScope } from "@/lib/use-scope";
import { balanceCurve, creditDailyBars, expiryBuckets, type UsageRange } from "@/lib/usage-charts";
import { useAccountsStore } from "@/stores/accounts";

const CREDIT_REGION_STORAGE_KEY = "buddy-switch:credit-stats:region";

function isRegionFilter(value: unknown): value is RegionFilter {
  return value === "cn" || value === "global" || value === "all";
}

/**
 * 读**旧的**「统计范围」偏好键 —— 只用于**初始化合并态**（`all` 是视图参数）。
 * **不再回写**（`cn` / `global` 已由 `useScope` 接管）。
 */
function readPreferredRegion(): RegionFilter {
  if (typeof window === "undefined") return "cn";
  try {
    const stored = window.localStorage.getItem(CREDIT_REGION_STORAGE_KEY);
    return isRegionFilter(stored) ? stored : "cn";
  } catch {
    return "cn";
  }
}

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

/** 会话级缓存：切走再切回不重取。 */
const statisticsCache: Partial<Record<RegionFilter, CreditStatistics>> = {};

export default function CreditStatsPage() {
  const t = useT();
  const [scopeRegion] = useScope("workbuddy");
  const [combineAll, setCombineAll] = useState(() => readPreferredRegion() === "all");
  const region: RegionFilter = combineAll ? "all" : scopeRegion;
  const [range, setRange] = useState<UsageRange>(30);
  const [stats, setStats] = useState<CreditStatistics | null>(() => statisticsCache[region] ?? null);
  const [loading, setLoading] = useState(() => !statisticsCache[region]);
  const [error, setError] = useState<string | null>(null);

  /* ★ 必须带 selector（本页含图表，store 的高频刷新不该牵动整页重渲染）。 */
  const { creditMap, fetchAllRegions } = useAccountsStore(
    useShallow((state) => ({ creditMap: state.creditMap, fetchAllRegions: state.fetchAllRegions })),
  );

  const load = useCallback(
    async (target: RegionFilter, refresh = false) => {
      if (!refresh && statisticsCache[target]) {
        setStats(statisticsCache[target]!);
        setLoading(false);
        setError(null);
        return;
      }
      setLoading(true);
      setError(null);
      try {
        // 资源包（到期分布数据源）来自 store；统计来自后端。
        await fetchAllRegions();
        const next = await api.getCreditStatistics(refresh, target);
        statisticsCache[target] = next;
        setStats(next);
      } catch (cause) {
        setError(api.asError(cause));
      } finally {
        setLoading(false);
      }
    },
    [fetchAllRegions],
  );

  useEffect(() => {
    void load(region);
  }, [load, region]);

  const bars = useMemo(() => (stats ? creditDailyBars(stats) : []), [stats]);
  const rangedBars = useMemo(() => bars.slice(-range), [bars, range]);
  const totalOut = rangedBars.reduce((sum, bar) => sum + bar.outAmt, 0);

  const balancePoints = useMemo(
    () => (stats ? balanceCurve(rangedBars, stats.summary.currentRemaining) : []),
    [rangedBars, stats],
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

  const tiles: StatTile[] = [
    {
      icon: Coins,
      label: t("wbStats.usage.credits.available"),
      value: formatCredits(stats?.summary.currentRemaining),
      hint: `${accountCount}`,
    },
    {
      icon: TrendingUp,
      label: t("wbStats.usage.credits.in"),
      value: "—",
      hint: t("wbStats.usage.caption.singleSide"),
    },
    { icon: TrendingDown, label: t("wbStats.usage.credits.out"), value: `-${formatCredits(totalOut)}` },
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
    <div className="mx-auto w-full max-w-[1180px] min-w-0 px-4 py-6 sm:px-8 sm:py-9">
      <header className="mb-4 flex min-w-0 flex-wrap items-start justify-between gap-4 sm:mb-5">
        <div className="min-w-0">
          <h1 className="text-[28px] font-semibold tracking-tight">{t("wbStats.credit.pageTitle")}</h1>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
            {regionFilterLabel(region)}
            {region === "all" && t("wbStats.credit.bothRegions")}
          </p>
        </div>
        <Button
          className="shrink-0"
          variant="outline"
          size="sm"
          onClick={() => void load(region, true)}
          disabled={loading}
        >
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          {t("wbStats.credit.refresh")}
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
          <AlertTitle>{t("wbStats.credit.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-wrap items-center gap-3">
            <span>{error}</span>
            <Button size="sm" variant="outline" onClick={() => void load(region, true)}>
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
            <Button size="sm" variant="outline" onClick={() => void load(region, true)}>
              {t("wbStats.usage.empty.refresh")}
            </Button>
          }
        />
      ) : (
        <div className="min-w-0 space-y-6">
          <StatTiles tiles={tiles} ariaLabel={t("wbStats.credit.overviewAria")} />

          {rangedBars.length > 0 ? (
            <ChartFrame
              title={t("wbStats.usage.credits.dailyTitle")}
              caption={singleSideCaption(t)}
              ariaLabel={t("wbStats.usage.credits.dailyAria")}
            >
              <CreditDivergingBar bars={rangedBars} />
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
                      {stats.events.slice(0, 50).map((event, index) => {
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
                              <td className="px-3 py-2.5 text-right tabular-nums text-amber-600 dark:text-amber-400">
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
                        return (
                          <tr key={`checkin-${event.ts}-${index}`} className="border-t border-border/60">
                            <td className="whitespace-nowrap px-4 py-2.5 text-muted-foreground">
                              {formatDateTime(event.ts)}
                            </td>
                            <td className="max-w-[160px] truncate px-3 py-2.5" title={account}>
                              {account}
                            </td>
                            <td className="px-3 py-2.5 text-right tabular-nums text-muted-foreground">—</td>
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
