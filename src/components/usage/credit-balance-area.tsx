/**
 * 积分 · **余额曲线**（recharts `AreaChart`，**基线不锚 0**）。
 *
 * ## ★ 为什么基线不锚 0（PLAN §3.10 第 3 条）
 *
 * 余额是**高位小幅波动**的序列（如 6406 → 7910）。若从 0 起画，
 * 波动会被压成一条直线，**视觉上等于在撒谎说「没变」**。
 * 故 Y 轴下限取 `baseline`（= `min - (max-min)*0.35`，由适配层算好），
 * 并在图注写明「基线非 0」（`balanceCaption`）。
 *
 * ⚠️ 曲线值是**回推**的（后端无逐日余额快照）—— 图注一并注明。
 */

import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";
import type { BalancePoint } from "@/lib/usage-charts";

import { formatCredits } from "./format";

export function CreditBalanceArea({ points }: { points: BalancePoint[] }) {
  const t = useT();
  if (points.length === 0) return null;
  const baseline = points[0].baseline;

  return (
    <ChartContainer
      config={{ value: { label: t("wbStats.usage.credits.balanceTitle"), color: "var(--data-series-teal)" } }}
      className="h-56 w-full"
    >
      <AreaChart data={points} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <defs>
          <linearGradient id="balance-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--data-series-teal)" stopOpacity={0.35} />
            <stop offset="100%" stopColor="var(--data-series-teal)" stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} strokeDasharray="3 3" />
        <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={24} />
        <YAxis
          tickLine={false}
          axisLine={false}
          width={52}
          domain={[baseline, "auto"]}
          tickFormatter={(value) => formatCredits(Number(value))}
        />
        <ChartTooltip cursor={{ stroke: "var(--border)", strokeDasharray: "3 3" }} content={<ChartTooltipContent indicator="line" />} />
        <Area
          type="monotone"
          dataKey="value"
          stroke="var(--data-series-teal)"
          strokeWidth={2}
          fill="url(#balance-fill)"
          isAnimationActive={false}
        />
      </AreaChart>
    </ChartContainer>
  );
}
