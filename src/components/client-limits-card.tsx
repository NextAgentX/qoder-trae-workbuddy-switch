import { useCallback, useEffect, useRef, useState } from "react";
import { Gauge, Loader2, RefreshCw } from "lucide-react";

import { clientLimits } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import type { ClientLimitEntry, ClientLimitsResponse, Region } from "@/lib/types";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

/** 扫描范围：`7` / `30` / `0`（`0` = 全量）。 */
type WindowKey = "7" | "30" | "all";

const WINDOW_DAYS: Record<WindowKey, number> = { "7": 7, "30": 30, all: 0 };

const WINDOW_LABELS: Record<WindowKey, TranslationKey> = {
  "7": "wbAccounts.limits.window.7",
  "30": "wbAccounts.limits.window.30",
  all: "wbAccounts.limits.window.all",
};

/**
 * 上游**排队类**错误码（客户端 `capacityQueueBlock.*` 队列块）。
 *
 * ⚠️ 口径遵循 PRD **R7**：`6020` / `6022` 的**精确语义文案待拍板前不得臆造**，
 * 界面**只**在原始 code 旁标注 R7 明确允许的保守词「排队中」。
 * 取证见 PRD「★ 6020/6022 取证」（候选：`6020≈…title.waiting` / `6022≈…title.userLimit`，**属推断**）。
 * code 原样带出，便于后续口径拍板后回填精确文案（PRD S6）。
 */
const QUEUE_CODES = new Set([6020, 6022]);

/**
 * 客户端限流台账卡片：**账号 × 模型 × 恢复时刻**（**只读**观测）。
 *
 * 数据源是客户端自身落盘的三张只读表 join（见 core `client_limits.rs`）——
 * **不依赖网关、不代理客户端流量、不要求客户端配合**。后端已把「仍受限」置顶，
 * 本组件只负责渲染：仍受限的高亮，已恢复的灰显（PRD S4）。
 *
 * ⚠️ **只读**：不触发任何写操作、不改变账号状态（PRD R8）。
 */
export function ClientLimitsCard({ region }: { region: Region }) {
  const t = useT();
  const [windowKey, setWindowKey] = useState<WindowKey>("7");
  const [data, setData] = useState<ClientLimitsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 竞态防护：切 region / 切窗口时旧请求可能后到，用序号丢弃过期响应。
  const requestSeq = useRef(0);

  const load = useCallback(async () => {
    const seq = ++requestSeq.current;
    setLoading(true);
    setError(null);
    try {
      const next = await clientLimits(region, WINDOW_DAYS[windowKey]);
      if (seq !== requestSeq.current) return;
      setData(next);
    } catch (cause) {
      if (seq !== requestSeq.current) return;
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, [region, windowKey]);

  useEffect(() => {
    void load();
  }, [load]);

  const limits = data?.limits ?? [];

  return (
    <Card className="gap-0 py-0">
      <div className="flex flex-wrap items-start justify-between gap-3 px-5 py-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Gauge className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <h2 className="text-sm font-semibold text-foreground">{t("wbAccounts.limits.title")}</h2>
            {data ? (
              <span className="text-xs text-muted-foreground tabular-nums">
                {t("wbAccounts.limits.summary", {
                  total: data.counts.deduped,
                  active: data.counts.active,
                })}
              </span>
            ) : null}
          </div>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {t("wbAccounts.limits.subtitle")}
          </p>
        </div>

        <div className="flex items-center gap-2">
          <Tabs value={windowKey} onValueChange={(next) => setWindowKey(next as WindowKey)}>
            <TabsList aria-label={t("wbAccounts.limits.windowLabel")}>
              {(Object.keys(WINDOW_LABELS) as WindowKey[]).map((key) => (
                <TabsTrigger key={key} value={key}>
                  {t(WINDOW_LABELS[key])}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          <Button size="sm" variant="outline" onClick={() => void load()} disabled={loading}>
            {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("wbAccounts.limits.refresh")}
          </Button>
        </div>
      </div>

      <div className="border-t border-border px-5 py-4">
        {loading && !data ? (
          <div className="space-y-2">
            {[0, 1].map((index) => (
              <Skeleton key={index} className="h-16 w-full rounded-lg" />
            ))}
          </div>
        ) : error ? (
          <p className="text-sm text-destructive">
            {t("wbAccounts.limits.loadFailed")}：{error}
          </p>
        ) : limits.length === 0 ? (
          <EmptyState icon={Gauge} title={t("wbAccounts.limits.empty")} />
        ) : (
          <ul className="space-y-2">
            {limits.map((entry) => (
              <LimitRow key={`${entry.accountUid ?? "?"}:${entry.model ?? "?"}:${entry.resetAt ?? "?"}`} entry={entry} />
            ))}
          </ul>
        )}
      </div>
    </Card>
  );
}

function LimitRow({ entry }: { entry: ClientLimitEntry }) {
  const t = useT();
  const account = entry.account ?? t("wbAccounts.limits.accountUnknown");
  const model = entry.model ?? t("wbAccounts.limits.modelUnknown");
  const reset = entry.resetAt ? t("wbAccounts.limits.resetAt", { time: formatDate(entry.resetAt) }) : t("wbAccounts.limits.resetUnknown");

  return (
    <li
      className={cn(
        "flex items-start justify-between gap-4 rounded-lg border border-border px-3.5 py-3",
        entry.active ? "border-l-2 border-l-warning bg-warning/[0.04]" : "opacity-60",
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-medium" title={entry.accountUid ?? undefined}>
            {account}
          </span>
          <Badge variant="secondary" className="shrink-0">
            {model}
          </Badge>
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <span className={entry.active ? "font-medium text-warning" : undefined}>{reset}</span>
          <span>{t("wbAccounts.limits.occurredAt", { time: formatDate(entry.occurredAt) })}</span>
          {entry.code !== null ? (
            <span className="inline-flex items-center gap-1">
              {t("wbAccounts.limits.code", { code: entry.code })}
              {/* R7：排队类 code 只标注保守词「排队中」，不臆造精确语义文案。 */}
              {QUEUE_CODES.has(entry.code) ? (
                <Badge
                  variant="outline"
                  className="h-4 px-1.5 text-[10px] leading-none"
                  title={t("wbAccounts.limits.queuedHint")}
                >
                  {t("wbAccounts.limits.queued")}
                </Badge>
              ) : null}
            </span>
          ) : null}
          {/* 会话 ID 只给短码 + 全量 title，便于定位又不把长 ID 铺满一行。 */}
          <span className="font-mono" title={t("wbAccounts.limits.session", { id: entry.sessionId })}>
            {entry.sessionId.slice(0, 8)}
          </span>
        </div>
      </div>
      <Badge variant={entry.active ? "warning" : "secondary"} className="shrink-0">
        {entry.active ? t("wbAccounts.limits.active") : t("wbAccounts.limits.recovered")}
      </Badge>
    </li>
  );
}