/**
 * 趋势 · **每日按模型堆叠柱**（recharts）。
 *
 * ## 契约
 *
 * 每列 = 一天；每列分段数 = 模型数；**各分段之和 === 当日总量** ——
 * 求和由 `lib/usage-charts.ts` 的 `dailyModelSeries` 保证（末段吃余数），
 * 本组件**只做渲染**，不重算、不裁剪。
 *
 * ## 为什么把模型名映射成 `m0/m1/...`
 *
 * recharts 的 `dataKey` 是**点分路径**：模型名 `gpt-4.1` 里的 `.` 会被当成嵌套字段
 * ⇒ 取值失败、柱子全空。故先映射成安全键 `m{index}`，模型名只放进 `config.label`。
 */

import { useMemo } from "react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";
import { TOTAL_SEGMENT, type DailyModelColumn } from "@/lib/usage-charts";

import { compactTokens } from "./format";
import { seriesColor } from "./palette";

export function StackedDailyBar({ columns }: { columns: DailyModelColumn[] }) {
  const t = useT();

  const { rows, series, config } = useMemo(() => {
    const names: string[] = [];
    for (const column of columns) {
      for (const segment of column.segments) {
        if (!names.includes(segment.name)) names.push(segment.name);
      }
    }
    const series = names.map((name, index) => ({ key: `m${index}`, name, color: seriesColor(index) }));
    const keyByName = new Map(series.map((item) => [item.name, item.key]));

    const rows = columns.map((column) => {
      const row: Record<string, number | string> = { label: column.label };
      for (const item of series) row[item.key] = 0;
      for (const segment of column.segments) {
        const key = keyByName.get(segment.name);
        if (key) row[key] = Number(row[key]) + segment.value;
      }
      return row;
    });

    const config: ChartConfig = {};
    for (const item of series) {
      config[item.key] = {
        label: item.name === TOTAL_SEGMENT ? t("wbStats.usage.trend.totalSegment") : item.name,
        color: item.color,
      };
    }
    return { rows, series, config };
  }, [columns, t]);

  if (columns.length === 0 || series.length === 0) return null;

  return (
    <ChartContainer config={config} className="h-64 w-full sm:h-72">
      <BarChart data={rows} margin={{ top: 8, right: 8, left: 0, bottom: 0 }} barCategoryGap="18%">
        <CartesianGrid vertical={false} strokeDasharray="3 3" />
        <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={24} />
        <YAxis tickLine={false} axisLine={false} width={48} tickFormatter={(value) => compactTokens(Number(value))} />
        <ChartTooltip
          cursor={{ fill: "var(--muted)", opacity: 0.4 }}
          content={<ChartTooltipContent indicator="dot" />}
        />
        {series.map((item, index) => (
          <Bar
            key={item.key}
            dataKey={item.key}
            stackId="tokens"
            fill={`var(--color-${item.key})`}
            stroke="var(--background)"
            strokeWidth={2}
            maxBarSize={28}
            radius={index === series.length - 1 ? [4, 4, 0, 0] : 0}
            isAnimationActive={false}
          />
        ))}
      </BarChart>
    </ChartContainer>
  );
}
