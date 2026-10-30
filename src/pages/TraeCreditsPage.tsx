/**
 * 「用量」· **积分视图**（Trae 分区）—— 严格按原型实现（用户 2026-10-12 要求）。
 *
 * ## 原型块序（`docs/prototype/demo-app.js:916-996`）
 *
 * 1. 顶部说明条 + 「刷新积分」；
 * 2. 4 统计卡：可用积分合计 / 区间入账 / 区间消耗 / 即将过期；
 * 3. `每日积分收支`（双向柱）；
 * 4. g2：`积分余额变化`（面积曲线）| `积分到期分布`（30 天柱）；
 * 5. g2：`账号 Token 占比`（环形）| `网关状态码分布`（环形）；
 * 6. `积分流水`表（时间 / 账号 / 变动 / 说明）。
 *
 * 改造前本页是 Trae 自建的一套（近 N 日折线 + 账号积分明细表）—— 与原型不符：
 * 折线换成了双向柱，账号明细表换成了原型真正有的**积分流水**。
 *
 * ## 与 WorkBuddy 的差异**只在数据**
 *
 * | 维度 | WorkBuddy | Trae（本页） |
 * |:--|:--|:--|
 * | 每日收支柱 | **单向**（入账不可得 ⇒ 只画消耗侧） | **双向**（每日快照两侧都有） |
 * | 两个环形（账号占比 / 状态码） | **无数据源** ⇒ 不渲染 | 有 `accounts` / `statuses` ⇒ 按原型渲染 |
 * | 合并态 | 后端 `RegionFilter=all` | 前端跨区域聚合（`lib/trae-credits-merge.ts`） |
 *
 * 图表一律走共用原语（`CreditDivergingBar` / `CreditBalanceArea` / `CreditExpiryBars` /
 * `ModelDonut`），单双向由**数据**决定（`CreditDivergingBar` 见 `inAmt > 0` 即画双向）。
 *
 * ## 白屏防线
 *
 * 账号名过 `displayText`（后端可能吐对象 ⇒ 对象当 React 子节点会卸载整棵树，issue #2）。
 */

import { useCallback, useMemo, useState } from "react";
import {
  AlertTriangle,
  CalendarDays,
  Coins,
  Loader2,
  RefreshCw,
  TrendingDown,
  TrendingUp,
} from "lucide-react";
import { toast } from "sonner";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { CardHead } from "@/components/ui/card-head";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { ChartFrame } from "@/components/usage/chart-frame";
import { CreditBalanceArea } from "@/components/usage/credit-balance-area";
import { CreditDivergingBar } from "@/components/usage/credit-diverging-bar";
import { CreditExpiryBars } from "@/components/usage/credit-expiry-bars";
import { formatCredits, signedCredits } from "@/components/usage/format";
import { ModelDonut } from "@/components/usage/model-donut";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import type { UsageViewProps } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { balanceCaption } from "@/lib/chart-caption";
import { displayText } from "@/lib/display-text";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { mergeTraeCredits, type TraeRegionSlice } from "@/lib/trae-credits-merge";
import {
  traeAccountSlices,
  traeCreditBars,
  traeCreditExpiry,
  traeStatusSlices,
} from "@/lib/trae-usage-charts";
import type { TraeRegionId, TraeTokenScope, TraeTokenStatistics } from "@/lib/trae-types";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useAutoRefreshWhenVisible } from "@/lib/use-auto-refresh";
import { CREDIT_REFRESH_INTERVAL_MS } from "@/lib/use-credit-auto-refresh";
import { useTraeVariant } from "@/lib/use-trae-variant";
import { balanceCurve, expiryBuckets } from "@/lib/usage-charts";
import { usageWindowSpanDays } from "@/lib/usage-window";

/** 原型的两个环形只展示**前 6 名**账号（`accountShare.slice(0, 6)`）。 */
const ACCOUNT_SHARE_LIMIT = 6;

/** 文案里的「每 N 分钟」从间隔常量推导（与账号卡、WB 积分页同一个来源）。 */
const REFRESH_INTERVAL_MINUTES = Math.round(CREDIT_REFRESH_INTERVAL_MS / 60_000);

/**
 * 「用量 · 积分」视图（Trae 分区）。
 *
 * `range` / `combineAll` 由 `UsagePage` 传入（受控）；本页不放任何页内区域选择器。
 */
export default function TraeCreditsPage({ viewWindow, combineAll }: UsageViewProps) {
  const t = useT();
  /** 当前区域（由路径作用域 / `?line=` 决定），决定默认读哪一份积分与账号数据。 */
  const [region] = useTraeVariant();
  const [refreshing, setRefreshing] = useState(false);
  const scope: TraeTokenScope = combineAll ? "all" : region;
  /* ★ 同 `TraeTokenStatsPage`：拉**覆盖整个窗口**的跨度，再按日历日切片。
     窗口锚在过去时只拉「窗口天数」会让那段历史根本不在返回里。 */
  const spanDays = usageWindowSpanDays(viewWindow);

  /**
   * 积分与账号两份数据都按**区域**读：它们分家存放，混读会让「积分趋势」是国内版的、
   * 而「账号列表」是国际版的。合并态 ⇒ 两区各取一份，交给纯函数聚合。
   */
  const loadSnapshot = useCallback(async (): Promise<TraeRegionSlice[]> => {
    const regions: TraeRegionId[] = combineAll ? ["cn", "global"] : [region];
    return Promise.all(
      regions.map(async (target) => {
        const [credits, overview] = await Promise.all([
          api.getTraeCredits(target),
          api.getTraeAccounts(target),
        ]);
        return { region: target, credits, overview };
      }),
    );
  }, [combineAll, region]);

  const {
    data: snapshot,
    loading,
    error,
    refresh: loadAll,
    revalidate: revalidateSnapshot,
    at: snapshotAt,
  } = useCachedResource<TraeRegionSlice[]>(
    `trae:credits:${combineAll ? "all" : region}`,
    loadSnapshot,
  );

  /**
   * ★ 每 10 分钟自动重取一轮（issue #133），无手动操作也能看到新余额。
   *
   * ⚠️ 只**重读快照**，不调 `traeRefreshCredits`（那才是「去上游同步」，会真的打接口）。
   * 上游同步由后台 `TRAE_CREDITS_REFRESH_INTERVAL_MS` 循环负责 —— 页面跟着把新读数读回来即可。
   * 两者各司其职：页面每 10 分钟读一次，后台每 10 分钟写一次，节奏同源。
   */
  useAutoRefreshWhenVisible(snapshotAt, revalidateSnapshot);

  /**
   * 原型的两个环形（账号 Token 占比 / 网关状态码分布）数据在 **Token 统计**里，
   * 不在积分载荷里 ⇒ 这里再读一次 token 统计。
   *
   * ⚠️ 缓存键与 Token 页**逐字相同** ⇒ 两页命中同一份快照，来回切不重复取数。
   * 取不到（或失败）时两个环形**不渲染** —— 积分页本身不受影响（不把配角的失败
   * 升级成整页错误）。
   */
  const loadTokens = useCallback(
    () => api.getTraeTokenStatistics(spanDays, scope),
    [spanDays, scope],
  );
  const { data: tokenStats } = useCachedResource<TraeTokenStatistics>(
    `trae:token-stats:${spanDays}:${scope}`,
    loadTokens,
  );

  /** 单区也走同一个聚合函数 ⇒ 只有一条渲染路径（见 `lib/trae-credits-merge.ts`）。 */
  const merged = useMemo(() => mergeTraeCredits(snapshot ?? []), [snapshot]);
  const credits = snapshot?.[0]?.credits ?? null;

  const bars = useMemo(
    () => traeCreditBars(merged.daily, viewWindow),
    [merged.daily, viewWindow],
  );
  const balancePoints = useMemo(() => balanceCurve(bars, merged.total), [bars, merged.total]);
  const buckets = useMemo(() => expiryBuckets(traeCreditExpiry(merged.rows)), [merged.rows]);
  const hasExpiry = buckets.some((bucket) => bucket.value > 0);
  const expiringTotal = buckets.reduce((sum, bucket) => sum + bucket.value, 0);
  const soonestDay = buckets.find((bucket) => bucket.value > 0)?.day ?? null;

  const inAmount = bars.reduce((sum, bar) => sum + bar.inAmt, 0);
  const outAmount = bars.reduce((sum, bar) => sum + bar.outAmt, 0);
  const net = inAmount - outAmount;

  const accountSlices = useMemo(
    () => (tokenStats ? traeAccountSlices(tokenStats).slice(0, ACCOUNT_SHARE_LIMIT) : []),
    [tokenStats],
  );
  const statusSlices = useMemo(
    () => (tokenStats ? traeStatusSlices(tokenStats) : []),
    [tokenStats],
  );

  /** 账号名回表（流水表要给人看的名字）；行键带区域前缀，不会串。 */
  const nameByKey = useMemo(
    () => new Map(merged.rows.map((row) => [row.key, displayText(row.name) || row.userId])),
    [merged.rows],
  );

  const balanceCaptionText =
    balancePoints.length > 0
      ? balanceCaption(
          t,
          balancePoints[0].value,
          balancePoints[balancePoints.length - 1].value,
          balancePoints[0].baseline,
        )
      : "";

  const tiles: StatTile[] = [
    {
      icon: Coins,
      label: t("wbStats.usage.credits.available"),
      value: formatCredits(merged.total),
      hint: t("wbStats.usage.credits.accountsHint", { count: merged.rows.length }),
    },
    {
      icon: TrendingUp,
      label: t("wbStats.usage.credits.in"),
      value: signedCredits(inAmount, "+"),
      hint: t("trae.stats.credits.inHint"),
    },
    {
      icon: TrendingDown,
      label: t("wbStats.usage.credits.out"),
      value: signedCredits(outAmount, "-"),
      hint: t("trae.stats.credits.netHint", { net: `${net >= 0 ? "+" : ""}${formatCredits(net)}` }),
    },
    {
      icon: CalendarDays,
      label: t("wbStats.usage.credits.expiring"),
      value: formatCredits(expiringTotal),
      hint: soonestDay
        ? t("trae.stats.credits.soonestHint", { days: soonestDay })
        : t("trae.stats.credits.noExpiryHint"),
    },
  ];

  async function refreshCredits() {
    setRefreshing(true);
    try {
      /* 合并态 ⇒ 两区都同步；否则只同步当前区域（**不**给没在看的那一区发请求）。 */
      const regions: TraeRegionId[] = combineAll ? ["cn", "global"] : [region];
      await Promise.all(regions.map((target) => api.traeRefreshCredits(undefined, target)));
      await loadAll();
      toast.success(t("trae.stats.credits.refreshDone"));
    } catch (e) {
      toast.error(t("trae.stats.credits.refreshFailed"), { description: api.asError(e) });
    } finally {
      setRefreshing(false);
    }
  }

  return (
    /* ⚠️ 页头 / 子 Tab / **视图参数条**都由**上层** `UsagePage` 统一渲染
       （原型 `viewUsage` 只有一个「用量」页头、一条参数条）⇒ 本页只渲染视图内容。 */
    <div className="min-w-0 space-y-6">
      {/* 原型 `creditsHeadBar()`：说明 + 「刷新积分」。
          它是**独立的上游动作**（真的去上游同步），不能只靠页头那个「刷新」（那只重挂视图）。 */}
      <div className="flex min-w-0 flex-wrap items-center gap-3">
        <span className="text-[11px] text-muted-foreground">{t("trae.stats.credits.subtitle")}</span>
        <span className="text-[11px] text-muted-foreground">
          {t("shared.credit.autoEvery", { n: REFRESH_INTERVAL_MINUTES })}
        </span>
        {/* 「上次刷新」是事实：这份数字何时读到的（绝对时刻；相对时间不会自己走）。 */}
        {snapshotAt > 0 && (
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {t("shared.credit.lastRefresh", { time: formatDate(snapshotAt) })}
          </span>
        )}
        <div className="grow" />
        <Button variant="outline" size="sm" disabled={refreshing} onClick={() => void refreshCredits()}>
          {refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          {refreshing ? t("trae.stats.credits.syncing") : t("trae.stats.credits.sync")}
        </Button>
      </div>

      {error && (
        <Alert variant="destructive">
          <AlertTriangle />
          <AlertTitle>{t("trae.stats.credits.loadFailed")}</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {loading && !credits ? (
        <div className="space-y-6" aria-busy="true">
          <Skeleton className="h-[74px] w-full" />
          <Skeleton className="h-64 w-full" />
        </div>
      ) : merged.rows.length === 0 && merged.daily.length === 0 ? (
        <EmptyState
          icon={Coins}
          title={t("trae.stats.credits.trendEmptyAccounts")}
          description={t("trae.stats.credits.trendEmptyData")}
          action={
            <Button size="sm" variant="outline" disabled={refreshing} onClick={() => void refreshCredits()}>
              {refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("trae.stats.credits.sync")}
            </Button>
          }
        />
      ) : (
        <>
          <StatTiles tiles={tiles} ariaLabel={t("trae.stats.credits.overviewAria")} />

          {/* 3 · 每日积分收支（**双向**：Trae 的入账可得） */}
          {bars.length > 0 ? (
            <ChartFrame
              title={t("trae.stats.credits.dailyTitle")}
              caption={t("trae.stats.credits.dailyNote")}
              ariaLabel={t("trae.stats.credits.dailyAria")}
            >
              <CreditDivergingBar bars={bars} />
            </ChartFrame>
          ) : null}

          {/* 4 · 余额曲线 | 到期分布 */}
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
            <ChartFrame
              title={t("wbStats.usage.credits.expiryTitle")}
              caption={t("wbStats.usage.credits.expiryDesc")}
              ariaLabel={t("wbStats.usage.credits.expiryAria")}
            >
              {hasExpiry ? (
                <CreditExpiryBars buckets={buckets} />
              ) : (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  {t("trae.stats.credits.expiryEmpty")}
                </p>
              )}
            </ChartFrame>
          </div>

          {/* 5 · 账号 Token 占比 | 网关状态码分布（原型在这一页放这两个环形） */}
          <div className="grid min-w-0 gap-6 lg:grid-cols-2">
            <ChartFrame
              title={t("trae.stats.credits.accountShareTitle")}
              caption={t("trae.stats.credits.shareNote")}
              ariaLabel={t("trae.stats.credits.accountShareAria")}
            >
              {accountSlices.length > 0 ? (
                <ModelDonut slices={accountSlices} size={150} />
              ) : (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  {t("trae.stats.credits.accountShareEmpty")}
                </p>
              )}
            </ChartFrame>
            <ChartFrame
              title={t("trae.stats.credits.statusTitle")}
              caption={t("trae.stats.credits.statusNote")}
              ariaLabel={t("trae.stats.credits.statusAria")}
            >
              {statusSlices.length > 0 ? (
                <ModelDonut slices={statusSlices} size={150} />
              ) : (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  {t("trae.stats.credits.statusEmpty")}
                </p>
              )}
            </ChartFrame>
          </div>

          {/* 6 · 积分流水（原型：时间 / 账号 / 变动 / 说明） */}
          {merged.records.length > 0 ? (
            <Card className="min-w-0 gap-0 overflow-hidden rounded-lg py-0 shadow-none">
              <CardHead
                title={t("wbStats.usage.credits.flowTitle")}
                caption={t("trae.stats.credits.flowNote")}
              />
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
                    {merged.records.map((record) => {
                      const name = nameByKey.get(`${record.region}:${record.userId}`) ?? record.userId;
                      return (
                        <tr key={record.key} className="border-t border-border/60">
                          <td className="whitespace-nowrap px-4 py-2.5 text-muted-foreground">
                            {record.date}
                          </td>
                          <td className="max-w-[200px] truncate px-3 py-2.5" title={name}>
                            {name}
                            {/* 合并态下同名账号可能来自两个区域 ⇒ 标出归属。 */}
                            {combineAll
                              ? ` · ${t(record.region === "cn" ? "shared.region.version.cn" : "shared.region.version.global")}`
                              : ""}
                          </td>
                          <td
                            className={
                              record.delta >= 0
                                ? "px-3 py-2.5 text-right tabular-nums text-success"
                                : "px-3 py-2.5 text-right tabular-nums text-warning"
                            }
                          >
                            {record.delta >= 0 ? "+" : ""}
                            {formatCredits(record.delta)}
                          </td>
                          <td className="px-4 py-2.5 text-muted-foreground">
                            {record.delta >= 0
                              ? t("trae.stats.credits.recordEarned")
                              : t("trae.stats.credits.recordSpent")}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </Card>
          ) : (
            <EmptyState
              icon={Coins}
              title={t("trae.stats.credits.flowEmptyTitle")}
              description={t("trae.stats.credits.flowEmptyBody")}
              action={
                <Button size="sm" variant="outline" disabled={refreshing} onClick={() => void refreshCredits()}>
                  {refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                  {t("trae.stats.credits.sync")}
                </Button>
              }
            />
          )}
        </>
      )}
    </div>
  );
}
