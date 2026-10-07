import { useEffect, useMemo } from "react";
import { AlertTriangle, CheckCircle2, Clock, RefreshCw } from "lucide-react";
import { Link, useLocation } from "react-router-dom";

import { TodayUsage } from "@/components/overview/today-usage";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { useT } from "@/lib/i18n";
import { platformOfPath } from "@/lib/platforms/registry";
import { regionDescriptor } from "@/lib/region";
import { scopeOf } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import { cn } from "@/lib/utils";
import { useAccountsStore } from "@/stores/accounts";

/** Token 到期前的预警窗口：24 小时。 */
const EXPIRY_WARN_MS = 24 * 60 * 60 * 1000;

interface PendingItem {
  key: string;
  icon: typeof AlertTriangle;
  tone: "warn" | "danger";
  title: string;
  hint: string;
}

/**
 * 「概览」工作台（PLAN §3.9 / P3）—— 答**「现在什么情况、有什么要我处理」**。
 *
 * ## 为什么需要它
 *
 * 改造前没有这个页面：用户想知道「有没有账号要重新登录 / 有没有 Token 要过期」，
 * 得**逐个账号卡片看**。信息是有的，只是**没有被汇总**。
 *
 * ## P3 的两处调整（对齐原型）
 *
 * 1. **待处理上移到统计块之前** —— 原型 `P1-6` 判据：
 *    `indexOf("待处理") < indexOf("账号总数")`。「要你动手的事」应当先于「只读的数字」。
 * 2. **新增「今日用量摘要」** —— 概览此前答不出「今天用了多少」；
 *    该块与用量页**同源**（同一个统计接口），不自己写死数字。
 *
 * ⚠️ **空态用 `EmptyState`**（§3.6）：没有待处理项是**好消息**，所以空态不是"暂无数据"，
 * 而是「一切正常」+ 出口（去账号页看看）。
 */
export default function OverviewPage() {
  const t = useT();
  /* 平台从**路由**推（`/s/{scope}/{dest}` 的 scope 里就带着它）——
     这样同一个组件在 WB 与 Trae 分区下都能用，不必各写一份。 */
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);

  const accounts = useAccountsStore((s) => s.accounts);
  const status = useAccountsStore((s) => s.status);
  const loading = useAccountsStore((s) => s.loading);
  const globalSlice = useAccountsStore((s) => s.global);
  const fetchAllRegions = useAccountsStore((s) => s.fetchAllRegions);

  useEffect(() => {
    void fetchAllRegions();
  }, [fetchAllRegions]);

  /* CN 侧是**扁平字段**、Global 侧是 `RegionSlice` —— 这里抹平两者的形状。
     ⚠️ 不要"顺手"把 store 改成两侧同形：那会牵动 App/统计页/多个钩子（见 store 里的注释）。 */
  const slice = region === "global" ? globalSlice : { accounts, status };
  const descriptor = regionDescriptor(region);

  const pending = useMemo<PendingItem[]>(() => {
    const list: PendingItem[] = [];
    const now = Date.now();

    const needRelogin = slice.accounts.filter((a) => a.needsRelogin);
    if (needRelogin.length > 0) {
      list.push({
        key: "relogin",
        icon: AlertTriangle,
        tone: "danger",
        title: t("wbOverview.pending.relogin", { count: needRelogin.length }),
        hint: t("wbOverview.pending.reloginHint"),
      });
    }

    const expiring = slice.accounts.filter(
      (a) => !a.needsRelogin && a.expiresAt != null && a.expiresAt - now < EXPIRY_WARN_MS,
    );
    if (expiring.length > 0) {
      list.push({
        key: "expiring",
        icon: Clock,
        tone: "warn",
        title: t("wbOverview.pending.expiring", { count: expiring.length }),
        hint: t("wbOverview.pending.expiringHint"),
      });
    }

    return list;
  }, [slice.accounts, t]);

  const running = Boolean(slice.status?.running);
  const accountsHref = `/s/${scopeOf(product, region)}/accounts`;

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-5">
      <header className="flex items-center gap-3">
        <div className="min-w-0">
          <h1 className="text-lg font-semibold tracking-[-0.01em]">{t("wbOverview.title")}</h1>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {descriptor.displayName} · {descriptor.versionLabel}
          </p>
        </div>
        <div className="grow" />
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void fetchAllRegions()}
          disabled={loading}
          aria-label={t("wbOverview.refresh")}
        >
          <RefreshCw className={cn(loading && "animate-spin")} />
          {t("wbOverview.refresh")}
        </Button>
      </header>

      {/* ① 待处理 —— **上移到统计块之前**（原型 P1-6：要你动手的事先于只读数字）。 */}
      <Card className="gap-0 py-0">
        <div className="border-b border-border/60 px-5 py-3">
          <span className="text-sm font-semibold">{t("wbOverview.pendingTitle")}</span>
        </div>
        {loading && slice.accounts.length === 0 ? (
          /* 首次加载用骨架行（§3.6：区块级加载占位一律 `Skeleton`）。 */
          <div className="flex flex-col gap-2 px-5 py-4">
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-4 w-1/2" />
          </div>
        ) : pending.length === 0 ? (
          /* 「没有待处理项」是**好消息**，所以空态不是"暂无数据"而是「一切正常」+ 出口。 */
          <EmptyState
            icon={CheckCircle2}
            title={t("wbOverview.allGood")}
            description={t("wbOverview.allGoodHint")}
            action={
              <Button asChild size="sm" variant="outline">
                <Link to={accountsHref}>{t("wbOverview.allGoodAction")}</Link>
              </Button>
            }
          />
        ) : (
          <ul className="divide-y divide-border/60">
            {pending.map((item) => (
              <li key={item.key} className="flex items-start gap-3 px-5 py-3">
                <item.icon
                  className={cn(
                    "mt-0.5 size-4 shrink-0",
                    item.tone === "danger" ? "text-destructive" : "text-amber-600 dark:text-amber-400",
                  )}
                  aria-hidden="true"
                />
                <div className="min-w-0">
                  <p className="text-sm font-medium">{item.title}</p>
                  <p className="mt-0.5 text-xs text-muted-foreground">{item.hint}</p>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      {/* ② 今日用量摘要（与用量页同源；「对比昨日」真算）。 */}
      <TodayUsage product={product} region={region} />

      {/* ③ 状态摘要：只放三个"一眼要看到"的数字，不放明细（明细在各工作台）。 */}
      <div className="grid grid-cols-3 gap-3">
        <SummaryTile label={t("wbOverview.tile.accounts")} value={slice.accounts.length} loading={loading} />
        <SummaryTile
          label={t("wbOverview.tile.pending")}
          value={pending.length}
          loading={loading}
          tone={pending.length > 0 ? "warn" : undefined}
        />
        <SummaryTile
          label={t("wbOverview.tile.client")}
          value={running ? t("wbOverview.running") : t("wbOverview.stopped")}
          loading={loading}
          tone={running ? undefined : "warn"}
        />
      </div>
    </div>
  );
}

function SummaryTile({
  label,
  value,
  loading,
  tone,
}: {
  label: string;
  value: number | string;
  loading: boolean;
  tone?: "warn";
}) {
  return (
    <Card className="gap-0 px-4 py-3">
      <p className="text-xs text-muted-foreground">{label}</p>
      {loading ? (
        <Skeleton className="mt-1.5 h-6 w-12" />
      ) : (
        <p className={cn("mt-1 text-xl font-semibold tabular-nums", tone === "warn" && "text-amber-600 dark:text-amber-400")}>
          {value}
        </p>
      )}
    </Card>
  );
}
