/**
 * 趋势 · **7×24 热力图**（自绘 DOM 网格，非 recharts）。
 *
 * ## 契约（`lib/usage-charts.ts` 的 `heatmapMatrix` 保证）
 *
 * **7 行（周一..周日）× 24 列（小时）**；缺失格补 0（不丢点）。
 *
 * ## 为什么不用 recharts
 *
 * recharts 没有 7×24 网格原语；用 CSS grid 画更直观、无障碍更好（每格可带 aria-label）。
 *
 * ## 相对强度，不是绝对量
 *
 * 用透明度表示「相对强度」（该格 / 最大值），**刻意不声称是绝对 Token** ——
 * 热力图的价值是「哪个时段最忙」，把它当绝对量读会误导。aria-label 也只说强度。
 */

import { useLocale, useT } from "@/lib/i18n";
import type { HeatmapMatrix } from "@/lib/usage-charts";

const LEVEL_CLASS = [
  "bg-muted/70",
  "bg-primary/20",
  "bg-primary/40",
  "bg-primary/65",
  "bg-primary",
] as const;

/** 以「已知的周一」推 7 个星期的短名（避免为星期加 7 条文案键）。 */
function weekdayLabels(locale: string): string[] {
  const base = new Date(2024, 0, 1); // 2024-01-01 是周一
  const formatter = new Intl.DateTimeFormat(locale === "en" ? "en-US" : "zh-CN", { weekday: "short" });
  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date(base);
    date.setDate(base.getDate() + index);
    return formatter.format(date);
  });
}

export function UsageHeatmap({ matrix }: { matrix: HeatmapMatrix }) {
  const t = useT();
  const locale = useLocale();
  const labels = weekdayLabels(locale);
  const hourTicks = [0, 6, 12, 18, 23];

  return (
    <div className="min-w-0">
      <div className="min-w-0 space-y-1">
        {matrix.cells.map((row, rowIndex) => (
          <div key={labels[rowIndex]} className="flex items-center gap-2">
            <span className="w-8 shrink-0 text-right text-[11px] text-muted-foreground">
              {labels[rowIndex]}
            </span>
            <div
              className="grid min-w-0 flex-1 gap-1"
              style={{ gridTemplateColumns: `repeat(${matrix.cols}, minmax(0, 1fr))` }}
            >
              {row.map((value, colIndex) => {
                const level = value > 0 ? Math.max(1, Math.ceil(Math.sqrt(value / matrix.max) * 4)) : 0;
                return (
                  <span
                    key={colIndex}
                    className={`aspect-square min-w-0 rounded-[2px] ${LEVEL_CLASS[level]}`}
                    title={`${labels[rowIndex]} ${String(colIndex).padStart(2, "0")}:00 · ${Math.round((value / matrix.max) * 100)}%`}
                  />
                );
              })}
            </div>
          </div>
        ))}
      </div>
      <div className="mt-2 flex items-center gap-2">
        <span className="w-8 shrink-0" aria-hidden="true" />
        <div
          className="grid min-w-0 flex-1 text-[10px] text-muted-foreground"
          style={{ gridTemplateColumns: `repeat(${matrix.cols}, minmax(0, 1fr))` }}
          aria-hidden="true"
        >
          {Array.from({ length: matrix.cols }, (_, hour) => (
            <span key={hour} className="whitespace-nowrap">
              {hourTicks.includes(hour) ? `${hour}:00` : ""}
            </span>
          ))}
        </div>
      </div>
      <p className="mt-1 text-[10px] text-muted-foreground">{t("wbStats.usage.trend.heatmapDesc")}</p>
    </div>
  );
}
