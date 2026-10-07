/**
 * 概览 · **今日用量摘要**（P3 新增）。
 *
 * ## 为什么需要它
 *
 * 概览此前只有「账号 / 待处理 / 客户端」三个 tile，**答不出「今天用了多少」**。
 * 本块补上今日 Token / 调用 / 对比昨日。
 *
 * ## ★ 与用量页**同源**（PLAN §3.10 第 1 条）
 *
 * 「同一件事的数字只有一个来源」：本块走**同一个** `api.getTokenStatistics` +
 * `mergeSources`，不自己写死任何数字。任何口径变化（如某来源被排除）两处会一起变。
 *
 * ## ★ 「对比昨日」真算（禁止硬编码）
 *
 * `(今日 - 昨日) / 昨日`。昨日为 0 时**不编造百分比**，显示「昨日无数据」。
 * 断言 `P0-2` 明确禁止 `+12%` 这类假增长。
 */

import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { Activity, CalendarDays, CircleAlert, MessagesSquare, TrendingUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import * as api from "@/lib/api";
import { exactTokens, compactTokens } from "@/components/usage/format";
import { useT } from "@/lib/i18n";
import type { Product } from "@/lib/platforms/registry";
import type { Region, TokenStatistics } from "@/lib/types";
import { scopeOf } from "@/lib/scope";
import { mergeSources } from "@/lib/usage-charts";

function dateKey(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function TodayUsage({ product, region }: { product: Product; region: Region }) {
  const t = useT();
  const [stats, setStats] = useState<TokenStatistics | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    // 取近 7 天，够算「今日 vs 昨日」。
    api
      .getTokenStatistics(7, region)
      .then((result) => {
        if (!disposed) setStats(result);
      })
      .catch(() => {
        if (!disposed) setStats(null);
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [region]);

  const source = useMemo(() => (stats ? mergeSources(stats.sources) : null), [stats]);
  const today = dateKey(new Date());
  const yesterday = dateKey(new Date(Date.now() - 24 * 60 * 60 * 1000));
  const byDate = useMemo(
    () => new Map((source?.daily ?? []).map((point) => [point.key, point])),
    [source],
  );
  const todayPoint = byDate.get(today);
  const todayTotal = todayPoint?.total ?? 0;
  const todayRecords = todayPoint?.records ?? 0;
  const yesterdayTotal = byDate.get(yesterday)?.total ?? 0;

  const usageHref = `/s/${scopeOf(product, region)}/usage`;
  const hasUsage = todayTotal > 0 || todayRecords > 0;

  if (loading && !stats) {
    return (
      <section className="space-y-2" aria-busy="true">
        <Skeleton className="h-4 w-24" />
        <Skeleton className="h-24 w-full" />
      </section>
    );
  }

  if (!hasUsage) {
    return (
      <section className="space-y-2">
        <h2 className="px-1 text-sm font-semibold">{t("wbOverview.today.title")}</h2>
        <EmptyState
          icon={CalendarDays}
          title={t("wbOverview.today.empty")}
          description={t("wbOverview.today.emptyHint")}
          action={
            <Button asChild size="sm" variant="outline">
              <Link to={usageHref}>{t("wbOverview.today.goUsage")}</Link>
            </Button>
          }
        />
      </section>
    );
  }

  // ★ 真算：昨日为 0 时不给百分比（不给假增长）。
  let deltaValue: string;
  let deltaTone: StatTile["tone"] = "default";
  if (yesterdayTotal <= 0) {
    deltaValue = t("wbOverview.today.noBaseline");
  } else {
    const pct = ((todayTotal - yesterdayTotal) / yesterdayTotal) * 100;
    if (Math.abs(pct) < 0.05) {
      deltaValue = t("wbOverview.today.flat");
    } else if (pct > 0) {
      deltaValue = t("wbOverview.today.up", { pct: pct.toFixed(1) });
      deltaTone = "up";
    } else {
      deltaValue = t("wbOverview.today.down", { pct: Math.abs(pct).toFixed(1) });
      deltaTone = "down";
    }
  }

  const tiles: StatTile[] = [
    { icon: MessagesSquare, label: t("wbOverview.today.tokens"), value: compactTokens(todayTotal) },
    { icon: Activity, label: t("wbOverview.today.records"), value: exactTokens(todayRecords) },
    {
      icon: TrendingUp,
      label: t("wbOverview.today.vsYesterday"),
      value: deltaValue,
      hint: yesterdayTotal > 0 ? compactTokens(yesterdayTotal) : undefined,
      tone: deltaTone,
    },
  ];

  return (
    <section className="space-y-2">
      <div className="flex items-center justify-between gap-2 px-1">
        <h2 className="text-sm font-semibold">{t("wbOverview.today.title")}</h2>
        <Button asChild size="sm" variant="ghost" className="h-7 px-2 text-xs">
          <Link to={usageHref}>{t("wbOverview.today.goUsage")}</Link>
        </Button>
      </div>
      <StatTiles tiles={tiles} ariaLabel={t("wbOverview.today.title")} />
      {!stats ? (
        <p className="flex items-center gap-1.5 px-1 text-xs text-muted-foreground">
          <CircleAlert className="size-3.5" aria-hidden="true" />
          {t("wbStats.usage.empty.noSourceHint")}
        </p>
      ) : null}
    </section>
  );
}
