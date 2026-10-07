/**
 * 会话 · **Token 排行 + 明细表**（自绘）。
 *
 * 排行条数与表格行数都 = 会话数（不截断，保证与散点同源、同一批会话）。
 * 会话标题过 `displayText`（白屏防线：后端可能把展示字段吐成对象）。
 */

import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import type { SessionPoint } from "@/lib/usage-charts";

import { compactTokens, exactTokens, sharePercent } from "./format";

export function SessionRanking({ points }: { points: SessionPoint[] }) {
  const t = useT();
  if (points.length === 0) return null;
  const total = points.reduce((sum, point) => sum + point.value, 0);

  return (
    <div className="min-w-0 space-y-5">
      <div className="space-y-3">
        {points.map((point, index) => {
          const label = displayText(point.title) || t("wbStats.usage.sessions.unnamed");
          const share = sharePercent(point.value, total);
          return (
            <div key={point.id}>
              <div className="mb-1.5 flex min-w-0 items-center gap-3 text-xs">
                <span className="w-5 shrink-0 font-mono text-muted-foreground">
                  {String(index + 1).padStart(2, "0")}
                </span>
                <span className="min-w-0 flex-1 truncate font-medium" title={label}>
                  {label}
                </span>
                <span className="shrink-0 tabular-nums" title={`${exactTokens(point.value)} Token`}>
                  {compactTokens(point.value)}
                </span>
                <span className="w-12 shrink-0 text-right text-muted-foreground tabular-nums">{share}</span>
              </div>
              <div className="ml-8 h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-primary"
                  style={{ width: share === "—" ? "0%" : `${(point.value / total) * 100}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>

      <div className="min-w-0 overflow-x-auto">
        <table className="w-full min-w-[560px] text-left text-xs">
          <thead className="bg-muted/45 text-muted-foreground">
            <tr>
              <th scope="col" className="px-3 py-2.5 font-medium">
                {t("wbStats.usage.sessions.colTitle")}
              </th>
              <th scope="col" className="px-3 py-2.5 text-right font-medium">
                {t("wbStats.usage.sessions.colRecords")}
              </th>
              <th scope="col" className="px-3 py-2.5 text-right font-medium">
                {t("wbStats.usage.sessions.colTokens")}
              </th>
              <th scope="col" className="px-3 py-2.5 text-right font-medium">
                {t("wbStats.usage.sessions.colShare")}
              </th>
            </tr>
          </thead>
          <tbody>
            {points.map((point) => {
              const label = displayText(point.title) || t("wbStats.usage.sessions.unnamed");
              return (
                <tr key={point.id} className="border-t border-border/60">
                  <td className="max-w-[280px] truncate px-3 py-2.5 font-medium" title={label}>
                    {label}
                  </td>
                  <td className="px-3 py-2.5 text-right tabular-nums">{exactTokens(point.x)}</td>
                  <td className="px-3 py-2.5 text-right tabular-nums">{compactTokens(point.y)}</td>
                  <td className="px-3 py-2.5 text-right tabular-nums text-muted-foreground">
                    {sharePercent(point.value, total)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
