import { Eraser, Loader2, ScrollText } from "lucide-react";

import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { DemoAction } from "@/components/demo-action";
import { formatCredit } from "@/lib/gateway";
import { useT } from "@/lib/i18n";
import type { TraeGatewayLogEntry } from "@/lib/trae-types";
import { cn } from "@/lib/utils";

/** 单次渲染的日志行上限（与 WB 侧 `request-log.tsx` **同值**）。 */
const MAX_LOG_ROWS = 50;

function formatClock(ts: number): string {
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "—";
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}:${String(date.getSeconds()).padStart(2, "0")}`;
}

function formatTokens(value: number | null | undefined): string {
  if (value == null) return "—";
  return new Intl.NumberFormat("zh-CN").format(value);
}

function statusTone(status: number): string {
  if (status >= 200 && status < 300) return "text-success";
  if (status === 429) return "text-warning";
  return "text-destructive";
}

/**
 * 最近 N 条 Trae 网关请求日志（仅元数据）+ 清空。
 *
 * 对齐 WorkBuddy `gateway/request-log.tsx` 骨架，但**不搬 `regionDescriptor`**：
 * Trae 无 region，日志行里也就没有版本列。
 *
 * 「积分」列同理是**可选**的（`credit` 取值函数）：Qoder 的上游逐请求回报
 * `usage.credits`，Trae 的上游不回报 —— 让 Trae 页也画一列恒 0 的积分，
 * 读者只会得出「Trae 免费用」这个错误结论。
 *
 * 本组件是**纯展示**：数据与清空动作由页面持有（页面的 `loadAll` 已经拉了日志，
 * 组件再自拉一次只会得到两份可能不同步的快照）。
 *
 * ## 2026-10-15 骨架对齐原型：去掉 `<Card>`
 *
 * 原型（`viewGateway()` 的 `logs` 分支）= **纯裸表**（无工具条、无卡头、无说明段）。
 * 本组件此前套了一整张 `<Card>`（卡头 = 「请求日志」标题 + 清空按钮）。标题与左栏
 * 分区导航重复，已去掉；**清空按钮必须保留**（本分区唯一的写操作出口），独占表前一行
 * （与 WB 侧 `request-log.tsx` 同口径）。
 */
export function TraeRequestLog<T extends TraeGatewayLogEntry>({
  logs,
  clearing,
  onClear,
  className,
  credit,
}: {
  logs: T[];
  clearing: boolean;
  onClear: () => void;
  className?: string;
  /**
   * 可选：**积分取值函数**。给了才渲染「积分」列（列位在「耗时」与「Token」之间）。
   *
   * 为什么是函数而不是布尔开关：Qoder 的日志键叫 `credits`（复数）、Trae 的日志
   * 压根没有这个键，且将来归一化口径可能再变 —— 取值方式交给调用方，组件只负责排版。
   * 返回值语义：`null`/`undefined` ⇒ `—`（键不存在：升级前的老条目）；`0` ⇒ 显示 `0`
   * （上游没上报，**不等于免费**，表头带提示）。
   */
  credit?: (entry: T) => number | null | undefined;
}) {
  const t = useT();

  /* 与 WB 侧**同一判据**：按 `ts` 降序（新在前）+ 上限 `MAX_LOG_ROWS`。
     ⚠️ 不用 `reverse()` —— 那依赖「后端一定按追加序返回」这个隐含前提；
     按 `ts` 显式排序后，无论后端什么顺序，两页看到的次序都一致。 */
  const rows = [...logs].sort((a, b) => b.ts - a.ts).slice(0, MAX_LOG_ROWS);

  return (
    <div className={cn(className)}>
      {/* 清空出口独占一行（分区名由左栏导航承担，不再有标题）。 */}
      <div className="mb-3 flex items-center justify-end">
        <DemoAction>
          <Button variant="ghost" size="sm" disabled={clearing || logs.length === 0} onClick={onClear}>
            {clearing ? <Loader2 className="animate-spin" /> : <Eraser />}
            {t("trae.gateway.log.clear")}
          </Button>
        </DemoAction>
      </div>

      {rows.length === 0 ? (
        /* P3：空态改用全应用唯一的 `EmptyState`（§3.6）——
           原来是「一行灰字」，与其它页面的空态形态不一致。 */
        <EmptyState icon={ScrollText} title={t("trae.gateway.log.empty")} className="px-5 py-6" />
      ) : (
        <div className="max-h-80 overflow-y-auto">
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-card text-xs text-muted-foreground">
              <tr className="border-b border-border/60">
                <th className="px-5 py-2 text-left font-medium">{t("trae.gateway.log.col.time")}</th>
                <th className="px-3 py-2 text-left font-medium">{t("trae.gateway.log.col.account")}</th>
                <th className="px-3 py-2 text-left font-medium">{t("trae.gateway.log.col.model")}</th>
                <th className="px-3 py-2 text-right font-medium">{t("trae.gateway.log.col.status")}</th>
                <th className="px-3 py-2 text-right font-medium">{t("trae.gateway.log.col.latency")}</th>
                {credit ? (
                  /* 同 WB 侧：原生 `title` 触屏 / 纯键盘读不到「0 ≠ 免费」，
                     改用共用 `Tooltip` + 可聚焦 `button`，两种输入方式都能唤出。 */
                  <th className="px-3 py-2 text-right font-medium">
                    <TooltipProvider delayDuration={250}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <button
                            type="button"
                            className="cursor-help focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                          >
                            {t("shared.gateway.log.col.credit")}
                          </button>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-80 text-xs leading-relaxed">
                          {t("shared.gateway.log.col.creditHint")}
                        </TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                  </th>
                ) : null}
                <th className="px-5 py-2 text-right font-medium">{t("trae.gateway.log.col.tokens")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((entry, index) => (
                <tr key={`${entry.ts}-${index}`} className="border-b border-border/40 last:border-0">
                  <td className="whitespace-nowrap px-5 py-2 font-mono text-xs">{formatClock(entry.ts)}</td>
                  {/* 账号：显示**原值** + `truncate`（完整值放 `title`）。
                      此前只取末 8 位 —— 与 WB 侧的「显示全名」对不上，同一个账号在
                      两个页面显示成两个样子。 */}
                  <td
                    className="max-w-[10rem] truncate px-3 py-2 text-xs text-muted-foreground"
                    title={entry.account ?? undefined}
                  >
                    {entry.account || "—"}
                  </td>
                  <td
                    className="max-w-[10rem] truncate px-3 py-2 text-xs text-muted-foreground"
                    title={entry.model ?? undefined}
                  >
                    {entry.model || "—"}
                  </td>
                  <td className={cn("px-3 py-2 text-right font-mono text-xs", statusTone(entry.status))}>
                    {entry.status}
                  </td>
                  <td className="px-3 py-2 text-right font-mono text-xs text-muted-foreground">
                    {entry.latencyMs}ms
                  </td>
                  {credit ? (
                    <td className="px-3 py-2 text-right font-mono text-xs text-muted-foreground">
                      {formatCredit(credit(entry))}
                    </td>
                  ) : null}
                  <td className="px-5 py-2 text-right font-mono text-xs text-muted-foreground">
                    {formatTokens((entry.promptTokens ?? 0) + (entry.completionTokens ?? 0))}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
