/**
 * 积分 · **每日收支柱**（recharts）—— 双向（条件）→ 单向（默认）。
 *
 * ## ★ 条件渲染（P3-ARCH §6 #1）
 *
 * 已核实积分**入账明细不可得**（`events` 里 `kind==="checkin"` 无金额字段、
 * 且被保留期 / 条数上限裁剪）⇒ `creditDailyBars` 的 `inAmt` **恒为 0**。
 *
 * 本组件据此**数据驱动**地决定形态：
 * - `bars` 里存在 `inAmt > 0` ⇒ 画**双向柱**（上=入账 / 下=消耗，零线居中）；
 * - 否则 ⇒ **只画消耗侧**（单向向下柱）+ 图注说明「入账明细不可得」。
 *
 * 这是「数据不支持就不画」的落地：**宁可少画一半，不要画错一半**。
 */

import { Bar, BarChart, CartesianGrid, ReferenceLine, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, type ChartConfig } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";
import type { CreditDailyBar } from "@/lib/usage-charts";

import { formatCredits } from "./format";

const CONFIG = {
  up: { label: "in", color: "var(--data-series-emerald)" },
  down: { label: "out", color: "var(--data-series-amber)" },
} satisfies ChartConfig;

/** tooltip：双向时列出两侧，单向时只列消耗侧。 */
function DivergingTooltip({
  active,
  payload,
  showInflow,
}: {
  active?: boolean;
  payload?: Array<{ payload?: CreditDailyBar }>;
  showInflow: boolean;
}) {
  const t = useT();
  if (!active || !payload?.length) return null;
  const bar = payload[0]?.payload;
  if (!bar) return null;
  return (
    <div className="grid gap-1 rounded-lg border border-border/50 bg-background px-3 py-2 text-xs shadow-xl">
      <div className="font-medium">{bar.label}</div>
      {showInflow ? (
        <div className="flex items-center justify-between gap-4">
          <span className="text-muted-foreground">{t("wbStats.usage.credits.in")}</span>
          <span className="font-mono tabular-nums text-emerald-600 dark:text-emerald-400">
            +{formatCredits(bar.inAmt)}
          </span>
        </div>
      ) : null}
      <div className="flex items-center justify-between gap-4">
        <span className="text-muted-foreground">{t("wbStats.usage.credits.out")}</span>
        <span className="font-mono tabular-nums text-amber-600 dark:text-amber-400">
          -{formatCredits(bar.outAmt)}
        </span>
      </div>
    </div>
  );
}

export function CreditDivergingBar({ bars }: { bars: CreditDailyBar[] }) {
  if (bars.length === 0) return null;

  const showInflow = bars.some((bar) => bar.inAmt > 0);
  const data = bars.map((bar) => ({ label: bar.label, up: bar.inAmt, down: -bar.outAmt }));

  return (
    <ChartContainer config={CONFIG} className="h-56 w-full">
      <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }} barCategoryGap="18%">
        <CartesianGrid vertical={false} strokeDasharray="3 3" />
        <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={24} />
        <YAxis
          tickLine={false}
          axisLine={false}
          width={48}
          tickFormatter={(value) => formatCredits(Math.abs(Number(value)))}
        />
        <ChartTooltip
          cursor={{ fill: "var(--muted)", opacity: 0.4 }}
          content={<DivergingTooltip showInflow={showInflow} />}
        />
        <ReferenceLine y={0} stroke="var(--border)" />
        {showInflow ? (
          <Bar
            dataKey="up"
            stackId="flow"
            fill="var(--color-up)"
            stroke="var(--background)"
            strokeWidth={1}
            maxBarSize={28}
            isAnimationActive={false}
          />
        ) : null}
        <Bar
          dataKey="down"
          stackId="flow"
          fill="var(--color-down)"
          stroke="var(--background)"
          strokeWidth={1}
          maxBarSize={28}
          radius={showInflow ? 0 : [0, 0, 4, 4]}
          isAnimationActive={false}
        />
      </BarChart>
    </ChartContainer>
  );
}
