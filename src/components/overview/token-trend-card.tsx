/**
 * 概览 · **近 14 天 Token 用量**卡（原型 `viewOverview` 的 `.split-main`）。
 *
 * 与 `OverviewStats` 的「今日 Token」**同源**（同一个 `useOverviewMetrics` 结果里的
 * `days` 轴）——同一件事的数字只有一个来源，两处不会各说各话。
 *
 * ## 空态与「读不到」是两件事
 *
 * - **读不到**（`token === null`）：显示「读不到数据」——不知道 ≠ 0；
 * - **读到了但没有量**（14 天合计为 0）：`EmptyState`「近 14 天没有用量记录」+ 出口。
 */

import { CalendarDays } from "lucide-react";
import { Link } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { exactTokens } from "@/components/usage/format";
import { useT } from "@/lib/i18n";
import type { OverviewMetrics } from "@/lib/overview-metrics";

/** 柱高（% of 峰值）；有量但极小的日子给 2% 底线，否则会被画成「没有」。 */
function barHeight(total: number, peak: number): string {
  if (total <= 0 || peak <= 0) return "0%";
  return `${Math.max(2, Math.round((total / peak) * 100))}%`;
}

interface Props {
  token: OverviewMetrics["token"];
  /** 「查看详情」的去处（该作用域的用量页）。 */
  usageHref: string;
}

export function TokenTrendCard({ token, usageHref }: Props) {
  const t = useT();
  const total = token ? token.days.reduce((sum, day) => sum + day.total, 0) : 0;
  const peak = token ? token.days.reduce((max, day) => Math.max(max, day.total), 0) : 0;

  return (
    <Card className="gap-0 py-0">
      <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
        <span className="text-sm font-semibold">{t("wbOverview.trend.title")}</span>
        <div className="grow" />
        <Button asChild size="sm" variant="ghost" className="h-7 px-2 text-xs">
          <Link to={usageHref}>{t("wbOverview.trend.goUsage")}</Link>
        </Button>
      </div>

      {!token ? (
        <p className="px-5 py-8 text-center text-xs text-muted-foreground">
          {t("wbOverview.stat.unavailable")}
        </p>
      ) : total <= 0 ? (
        <EmptyState
          icon={CalendarDays}
          title={t("wbOverview.trend.empty")}
          description={t("wbOverview.trend.emptyHint")}
          action={
            <Button asChild size="sm" variant="outline">
              <Link to={usageHref}>{t("wbOverview.trend.goUsage")}</Link>
            </Button>
          }
        />
      ) : (
        <div className="p-4">
          {/* 纯 DOM 柱状图（14 根柱不足以动用 recharts）；标题即逐日真值，鼠标悬停可核。 */}
          <div
            className="flex h-[120px] items-end gap-[5px] pt-2"
            role="img"
            aria-label={t("wbOverview.trend.aria")}
          >
            {token.days.map((day) => (
              <div
                key={day.key}
                className="flex h-full min-w-0 flex-1 items-end"
                title={t("wbOverview.trend.barTitle", {
                  date: day.key.slice(5),
                  tokens: exactTokens(day.total),
                  records: exactTokens(day.records),
                })}
              >
                <div
                  className="w-full rounded-t-[3px] bg-primary"
                  style={{ height: barHeight(day.total, peak) }}
                />
              </div>
            ))}
          </div>
          <div className="mt-1.5 flex gap-[5px]">
            {token.days.map((day) => (
              <span
                key={day.key}
                className="min-w-0 flex-1 truncate text-center text-[9.5px] tabular-nums text-muted-foreground"
              >
                {day.key.slice(8)}
              </span>
            ))}
          </div>
        </div>
      )}
    </Card>
  );
}