/**
 * 趋势 · **累计消耗面积曲线**（recharts `AreaChart`）。
 *
 * ## 契约
 *
 * - 累计曲线**锚 0**（累计值从 0 起，基线 0 是正确的）；
 * - 图注**不写「峰值 = 末值」**（由 `lib/chart-caption.ts` 的 `cumulativeCaption` 产出）。
 *
 * 与 `credit-balance-area` 的区别：那条是**高位小幅波动**序列，基线**不锚 0**。
 */

import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";

import { compactTokens } from "./format";

export interface AreaPoint {
  label: string;
  value: number;
}

export function CumulativeArea({ points }: { points: AreaPoint[] }) {
  const t = useT();
  if (points.length === 0) return null;

  return (
    <ChartContainer config={{ value: { label: t("wbStats.usage.trend.cumulativeTitle"), color: "var(--primary)" } }} className="h-56 w-full">
      <AreaChart data={points} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <defs>
          <linearGradient id="cumulative-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--primary)" stopOpacity={0.38} />
            <stop offset="100%" stopColor="var(--primary)" stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} strokeDasharray="3 3" />
        <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={24} />
        <YAxis tickLine={false} axisLine={false} width={52} tickFormatter={(value) => compactTokens(Number(value))} />
        <ChartTooltip cursor={{ stroke: "var(--border)", strokeDasharray: "3 3" }} content={<ChartTooltipContent indicator="line" />} />
        <Area
          type="monotone"
          dataKey="value"
          stroke="var(--primary)"
          strokeWidth={2}
          fill="url(#cumulative-fill)"
          isAnimationActive={false}
        />
      </AreaChart>
    </ChartContainer>
  );
}
