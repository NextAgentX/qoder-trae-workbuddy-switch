/**
 * 模型 · **用量排行条 + 每行迷你趋势线**（自绘，非 recharts）。
 *
 * ## 契约（P3-ARCH 契约 7）
 *
 * 每行一条迷你趋势线（`sparkline`，来自 `dailyByModel` 的逐日序列）。
 * 若后端未提供 `dailyByModel`（`spark` 为空），**不画**趋势线 —— 不画胜过硬造一条。
 *
 * ## 为什么自绘
 *
 * 「条形 + 内联趋势线 + 数值」这种复合行，recharts 要拼多个图反而更脆；
 * 用 DOM + 内联 SVG 更直接，也能把每行的语义（占比、Token）写清楚。
 */

import type { ModelSlice } from "@/lib/usage-charts";

import { compactTokens, sharePercent } from "./format";
import { seriesColor } from "./palette";

/** 迷你趋势线：只有 ≥2 个点才画（单点画不出走势）。 */
function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) return null;
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const points = values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * 100;
      const y = 100 - ((value - min) / span) * 100;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg
      className="shrink-0"
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
      style={{ width: 74, height: 20 }}
      aria-hidden="true"
    >
      <polyline
        points={points}
        fill="none"
        stroke="var(--primary)"
        strokeWidth={2}
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

export function ModelRanking({ slices }: { slices: ModelSlice[] }) {
  if (slices.length === 0) return null;
  const total = slices.reduce((sum, slice) => sum + slice.value, 0);

  return (
    <div className="space-y-3">
      {slices.map((slice, index) => (
        <div key={slice.name} className="min-w-0">
          <div className="flex min-w-0 items-center gap-3 text-xs">
            <span className="w-5 shrink-0 font-mono text-muted-foreground">
              {String(index + 1).padStart(2, "0")}
            </span>
            <span className="min-w-0 flex-1 truncate font-medium" title={slice.name}>
              {slice.name}
            </span>
            <Sparkline values={slice.spark} />
            <span
              className="shrink-0 tabular-nums"
              title={`${compactTokens(slice.value)} Token`}
            >
              {compactTokens(slice.value)}
            </span>
            <span className="w-12 shrink-0 text-right text-muted-foreground tabular-nums">
              {sharePercent(slice.value, total)}
            </span>
          </div>
          <div className="ml-8 mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full"
              style={{
                width: sharePercent(slice.value, total) === "—" ? "0%" : `${(slice.value / total) * 100}%`,
                backgroundColor: seriesColor(index),
              }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}
