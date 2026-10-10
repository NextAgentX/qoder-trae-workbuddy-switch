/**
 * 积分 · **到期分布**（recharts `BarChart`，**完整 30 天轴**）。
 *
 * ## 契约（P3-ARCH 契约 5 / 10）
 *
 * x 轴固定 **D+1 … D+30** 共 30 根柱，**无到期的天补 0**（不跳格）——
 * 否则「D+3 有到期」和「D+27 有到期」在图上看起来一样密。
 *
 * 若 30 天全为 0（没有即将到期的积分），调用方**不渲染本图**（无数据不渲染）。
 */

import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";
import type { ExpiryBucket } from "@/lib/usage-charts";

import { formatCredits } from "./format";

export function CreditExpiryBars({ buckets }: { buckets: ExpiryBucket[] }) {
  const t = useT();
  if (buckets.length === 0) return null;
  const data = buckets.map((bucket) => ({ label: `D+${bucket.day}`, value: bucket.value }));

  return (
    <ChartContainer
      config={{ value: { label: t("wbStats.usage.credits.expiryTitle"), color: "var(--data-series-amber)" } }}
      className="h-[170px] w-full"
    >
      <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid vertical={false} strokeDasharray="3 3" />
        <XAxis
          dataKey="label"
          tickLine={false}
          axisLine={false}
          tickMargin={8}
          interval={4}
          tickFormatter={(value) => String(value).replace("D+", "")}
        />
        <YAxis tickLine={false} axisLine={false} width={48} tickFormatter={(value) => formatCredits(Number(value))} />
        <ChartTooltip cursor={{ fill: "var(--muted)", opacity: 0.4 }} content={<ChartTooltipContent indicator="dot" />} />
        <Bar dataKey="value" fill="var(--color-value)" radius={[4, 4, 0, 0]} maxBarSize={18} isAnimationActive={false} />
      </BarChart>
    </ChartContainer>
  );
}
