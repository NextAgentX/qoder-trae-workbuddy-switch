import { CalendarDays } from "lucide-react";
import { Link } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { exactTokens } from "@/components/usage/format";
import { useT } from "@/lib/i18n";
import type { OverviewDay } from "@/lib/overview-metrics";
import { cn } from "@/lib/utils";

const SERIES = {
  client: { label: "wbStats.usage.ledger.client", color: "bg-[var(--data-series-emerald)]" },
  gateway: { label: "wbStats.usage.ledger.gateway", color: "bg-[var(--data-series-indigo)]" },
} as const;

type Series = keyof typeof SERIES;

interface Props {
  days: OverviewDay[] | null;
  usageHref: string;
}

export function TokenTrendCard({ days, usageHref }: Props) {
  const t = useT();
  const sourceKeys: Series[] = ["client", "gateway"];
  const series = sourceKeys.filter((key) =>
    days?.some((day) => (day[key]?.total ?? 0) > 0 || (day[key]?.records ?? 0) > 0),
  );
  const peak = days?.reduce(
    (max, day) => Math.max(max, day.client?.total ?? 0, day.gateway?.total ?? 0), 0,
  ) ?? 0;

  return (
    <Card className="gap-0 py-0">
      <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
        <span className="text-sm font-semibold">{t("wbOverview.trend.title")}</span>
        <div className="grow" />
        <Button asChild size="sm" variant="ghost" className="h-7 px-2 text-xs">
          <Link to={usageHref}>{t("wbOverview.trend.goUsage")}</Link>
        </Button>
      </div>

      {!days ? (
        <p className="px-5 py-8 text-center text-xs text-muted-foreground">
          {t("wbOverview.stat.unavailable")}
        </p>
      ) : series.length === 0 ? (
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
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            {series.map((key) => (
              <span key={key} className="flex items-center gap-1.5">
                <span aria-hidden="true" className={cn("size-2 rounded-sm", SERIES[key].color)} />
                {t(SERIES[key].label)}
              </span>
            ))}
          </div>
          {peak === 0 && (
            <p className="mt-3 text-xs text-muted-foreground">{t("wbOverview.trend.noTokens")}</p>
          )}
          <div
            className={cn("flex items-end gap-[5px] pt-2", peak > 0 ? "h-[120px]" : "h-7")}
            role="img"
            aria-label={t("wbOverview.trend.aria")}
          >
            {days.map((day) => (
              <div key={day.key} className="flex h-full min-w-0 flex-1 items-end gap-px">
                {series.map((key) => {
                  const point = day[key];
                  const bound = point?.partial ? "≥ " : "";
                  const title = point
                    ? t("wbOverview.trend.barTitle", {
                      date: day.key,
                      source: t(SERIES[key].label),
                      tokens: `${bound}${exactTokens(point.total)}`,
                      records: `${bound}${exactTokens(point.records)}`,
                    })
                    : undefined;
                  const height = point && peak > 0 && point.total > 0
                    ? `${Math.max(2, (point.total / peak) * 100)}%`
                    : 0;
                  return (
                    <div key={key} className="flex h-full min-w-0 flex-1 items-end" title={title}>
                      <div
                        className={cn("w-full rounded-t-[3px]", SERIES[key].color)}
                        style={{ height }}
                      />
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
          <div className="mt-1.5 flex gap-[5px]">
            {days.map((day) => (
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
