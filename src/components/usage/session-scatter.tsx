/**
 * 会话 · **记录数 × Token 散点气泡**（recharts `ScatterChart`）。
 *
 * ## ★ 轴语义（不许写错）
 *
 * x 轴是**记录数**（`TokenStatsGroup.records`），**不是消息数** ——
 * 真实后端**没有 `msgs` 字段**（原型演示里有，真实数据里没有）。
 * 轴标签 / 图注一律走 `wbStats.usage.sessions.colRecords` 与 `scatterAxisCaption`。
 *
 * ## 留边（契约 9）
 *
 * 气泡半径按 y 归一后取 `ZAxis.range`（像素上限受限），recharts 会在绘图区内布局，
 * 不会溢出边框。
 */

import { CartesianGrid, Scatter, ScatterChart, XAxis, YAxis, ZAxis } from "recharts";

import { ChartContainer, ChartTooltip, type ChartConfig } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";
import type { SessionPoint } from "@/lib/usage-charts";

import { compactTokens, exactTokens } from "./format";

const CONFIG = { value: { label: "tokens", color: "var(--data-series-indigo)" } } satisfies ChartConfig;

/** 气泡 tooltip：显示会话标题 + 记录数 + Token。 */
function ScatterTooltip({
  active,
  payload,
}: {
  active?: boolean;
  payload?: Array<{ payload?: SessionPoint }>;
}) {
  const t = useT();
  if (!active || !payload?.length) return null;
  const point = payload[0]?.payload;
  if (!point) return null;
  return (
    <div className="grid gap-1 rounded-lg border border-border/50 bg-background px-3 py-2 text-xs shadow-xl">
      <div className="max-w-[16rem] truncate font-medium" title={point.title}>
        {point.title}
      </div>
      <div className="flex items-center justify-between gap-4 text-muted-foreground">
        <span>{t("wbStats.usage.sessions.colRecords")}</span>
        <span className="font-mono tabular-nums text-foreground">{exactTokens(point.x)}</span>
      </div>
      <div className="flex items-center justify-between gap-4 text-muted-foreground">
        <span>Token</span>
        <span className="font-mono tabular-nums text-foreground">{compactTokens(point.y)}</span>
      </div>
    </div>
  );
}

export function SessionScatter({ points }: { points: SessionPoint[] }) {
  const t = useT();
  if (points.length === 0) return null;

  return (
    <ChartContainer config={CONFIG} className="h-[220px] w-full">
      <ScatterChart margin={{ top: 12, right: 16, bottom: 12, left: 8 }}>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis
          type="number"
          dataKey="x"
          name={t("wbStats.usage.sessions.colRecords")}
          tickLine={false}
          axisLine={false}
          tickMargin={8}
          tickFormatter={(value) => exactTokens(Number(value))}
        />
        <YAxis
          type="number"
          dataKey="y"
          tickLine={false}
          axisLine={false}
          width={52}
          tickFormatter={(value) => compactTokens(Number(value))}
        />
        <ZAxis type="number" dataKey="value" range={[60, 420]} />
        <ChartTooltip cursor={{ strokeDasharray: "3 3" }} content={<ScatterTooltip />} />
        <Scatter data={points} fill="var(--data-series-indigo)" fillOpacity={0.6} isAnimationActive={false} />
      </ScatterChart>
    </ChartContainer>
  );
}
