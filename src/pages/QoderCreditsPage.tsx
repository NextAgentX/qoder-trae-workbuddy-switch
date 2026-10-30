/**
 * 「用量 · 积分」· Qoder 分区 —— **实时余额快照**（不是逐日用量）。
 *
 * ## 为什么这张视图与「趋势」是两个口径
 *
 * 趋势读的是**本机客户端的会话记录**（我们转发/客户端自己花的每一笔都在里面，逐日可加总）；
 * 这张读的是**上游额度接口**（每个账号现在还剩多少）。两者的数据源、时间语义、
 * 甚至失败模式都不同 —— 合成一张图必然要在一个地方撒谎，所以分成两个子视图，
 * 并在底部把口径写清楚。
 *
 * ## 三条诚实性约束（与账号页的额度视图同源）
 *
 * 1. **部分失败如实报**：某个账号 401 时只有那一行 `读取失败 + 原因`，其余照常显示；
 *    汇总只累加成功的行，并同时给出「可用 / 失败」账号数（不把部分失败伪装成全额）。
 * 2. **超额要给徽标**：上游明确说 `isExceeded` 时才标「已超额」，不靠 `remaining === 0` 猜。
 * 3. **没有账号就明说**：空态指向「账号」页，不摆一张空表。
 *
 * ⚠️ 与 WB / Trae 的积分页**刻意不同形**：那两个平台有「每日积分收支 / 到期分布」的
 * 本地账本（签到奖励、逐日消耗），Qoder 侧没有等价数据源 ⇒ 只做**余额**这一件事，
 * 不画一条贴地的 0 线冒充趋势。
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Coins, Hash, RefreshCw, Users } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { ChartFrame } from "@/components/usage/chart-frame";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import type { UsageViewProps } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { formatCredits } from "@/components/usage/format";
import { formatDate } from "@/lib/format";
import { useAutoRefreshWhenVisible } from "@/lib/use-auto-refresh";
import { CREDIT_REFRESH_INTERVAL_MS } from "@/lib/use-credit-auto-refresh";
import { useT } from "@/lib/i18n";
import { platformOfPath } from "@/lib/platforms/registry";
import { scopeOf } from "@/lib/scope";
import { useLocation } from "react-router-dom";
import { useScope } from "@/lib/use-scope";
import { useRefreshAction } from "@/lib/use-refresh-action";

/** 文案里的「每 N 分钟」从间隔常量推导（与账号卡、WB / Trae 积分页同一个来源）。 */
const REFRESH_INTERVAL_MINUTES = Math.round(CREDIT_REFRESH_INTERVAL_MS / 60_000);

/**
 * 余额视图。
 *
 * `UsageViewProps` 里的窗口 / 合并态在这里**用不到**（余额是「此刻」的事实，与回看窗口无关），
 * 但仍按契约接收 —— 视图页的 props 形状统一，`UsagePage` 才能用同一段代码渲染所有子页。
 */
export default function QoderCreditsPage(_props: UsageViewProps) {
  const t = useT();
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);

  const [data, setData] = useState<api.QoderCreditsView | null>(null);
  const [error, setError] = useState("");
  /** 「上次刷新」的墙钟（issue #133）：只表达「这份余额是什么时候读到的」，不派生状态。 */
  const [lastRefreshAt, setLastRefreshAt] = useState(0);
  const { busy, run } = useRefreshAction();

  const load = useCallback(async () => {
    setError("");
    try {
      setData(await api.qoderCredits(scope));
      setLastRefreshAt(Date.now());
    } catch (e) {
      setError(api.asError(e));
    }
  }, [scope]);

  useEffect(() => {
    void load();
  }, [load]);

  /** ★ 每 10 分钟自动重读一轮（issue #133）：`qoderCredits` 本身就是逐账号取上游，读到的即当下余额。 */
  useAutoRefreshWhenVisible(lastRefreshAt, load);

  const tiles = useMemo<StatTile[]>(() => {
    if (!data) return [];
    return [
      {
        icon: Coins,
        label: t("qoder.page.usage.creditsView.remaining"),
        value: formatCredits(data.summary.remaining),
        hint: t("qoder.page.usage.unit.credits"),
      },
      {
        icon: Hash,
        label: t("qoder.page.usage.creditsView.total"),
        value: formatCredits(data.summary.total),
      },
      {
        icon: Users,
        label: t("qoder.page.usage.creditsView.okAccounts"),
        value: formatCredits(data.summary.okAccounts),
      },
      {
        icon: AlertTriangle,
        label: t("qoder.page.usage.creditsView.failedAccounts"),
        /* 0 个失败是「正常」而不是「未知」⇒ 照实显示 0（这里不该用 `—`）。 */
        value: formatCredits(data.summary.failedAccounts),
      },
    ];
  }, [data, t]);

  if (error) {
    return (
      <Alert variant="destructive">
        <AlertTriangle />
        <AlertTitle>{t("qoder.page.usage.creditsView.loadFailed")}</AlertTitle>
        <AlertDescription>
          <span>{error}</span>
          <div className="mt-2">
            <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(() => load())}>
              {busy ? <RefreshCw className="animate-spin" /> : <RefreshCw />}
              {t("qoder.page.usage.creditsView.reload")}
            </Button>
          </div>
        </AlertDescription>
      </Alert>
    );
  }

  if (!data) return <Skeleton className="h-32" />;

  if (data.accounts.length === 0) {
    return (
      <Card className="gap-0 py-0">
        <EmptyState
          icon={Coins}
          title={t("qoder.page.usage.creditsView.empty")}
          description={t("qoder.page.usage.creditsView.note")}
        />
      </Card>
    );
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="flex min-w-0 flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
        <span>{t("shared.credit.autoEvery", { n: REFRESH_INTERVAL_MINUTES })}</span>
        {lastRefreshAt > 0 && (
          <span className="tabular-nums">
            {t("shared.credit.lastRefresh", { time: formatDate(lastRefreshAt) })}
          </span>
        )}
      </div>

      <StatTiles tiles={tiles} />

      <ChartFrame
        title={t("qoder.page.usage.creditsView.perAccount")}
        ariaLabel={t("qoder.page.usage.creditsView.perAccount")}
        exportable={false}
      >
        <div className="space-y-3">
          {data.accounts.map((row) => {
            const name = row.name || row.uid;
            if (!row.ok) {
              return (
                <div key={row.uid} className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="truncate text-[12.5px] font-medium" title={name}>
                      {name}
                    </div>
                    <div className="mt-0.5 text-[11px] text-destructive">
                      {t("qoder.page.usage.creditsView.rowFailed", {
                        error: row.error ?? "—",
                      })}
                    </div>
                  </div>
                  <Badge variant="destructive" className="shrink-0">
                    {t("qoder.page.usage.creditsView.failedAccounts")}
                  </Badge>
                </div>
              );
            }
            const remaining = row.remaining ?? 0;
            const total = row.total ?? 0;
            const ratio = total > 0 ? Math.min(100, Math.max(0, (remaining / total) * 100)) : 0;
            return (
              <div key={row.uid} className="min-w-0">
                <div className="flex min-w-0 items-baseline gap-3 text-xs">
                  <span className="min-w-0 flex-1 truncate font-medium" title={name}>
                    {name}
                  </span>
                  {row.plan ? (
                    <span className="shrink-0 text-[11px] text-muted-foreground">
                      {t("qoder.page.usage.creditsView.plan", { plan: row.plan })}
                    </span>
                  ) : null}
                  {row.isExceeded ? (
                    <Badge variant="warning" className="shrink-0">
                      {t("qoder.page.usage.creditsView.exceeded")}
                    </Badge>
                  ) : null}
                  <span className="shrink-0 tabular-nums">
                    {t("qoder.page.usage.creditsView.value", {
                      remaining: formatCredits(remaining),
                      total: formatCredits(total),
                    })}
                  </span>
                </div>
                <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-foreground/[0.06]">
                  <div
                    className="h-full rounded-full bg-primary"
                    style={{ width: `${ratio}%` }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      </ChartFrame>

      <Alert>
        <AlertDescription>
          {data.summary.failedAccounts > 0
            ? `${t("qoder.page.usage.creditsView.partial", {
                n: String(data.summary.failedAccounts),
              })} `
            : ""}
          {t("qoder.page.usage.creditsView.note")}
        </AlertDescription>
      </Alert>
    </div>
  );
}
