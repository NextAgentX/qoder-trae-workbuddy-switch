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
 *
 * ## 为什么自带 shape 而不是直接给 `<Bar>` 描边
 *
 * 纵轴是**线性**的，且被单日峰值撑满（实测某日 603.1B ⇒ 轴上限 800B、每像素 ≈ 4.7B）。
 * 于是「10B 的一天」只有约 2px，而「0.5B 的一天」不足 0.2px —— 在浏览器里**一个像素
 * 都不产生**。叠加原实现给每根柱子加的 `stroke="var(--background)" strokeWidth={2}`
 * （背景色描边，视觉上用来分隔堆叠段），任何 **≤ 2px 的柱子都会被自己的描边整根涂掉**：
 * 实测「23 个有量的日列里只有 6 个画出了像素」，用户看到的就是「后面明明有量却没有柱子」。
 *
 * 修法两条，都落在 {@link visibleBar} 里：
 * 1. **矮柱不描边** —— 高度不足 {@link BAR_STROKE_MIN_PX} 的分段一律不描边，
 *    否则描边会把填充盖掉；够高的分段保持原样式（堆叠段之间的分隔线不变）。
 * 2. **每列保底高度** —— 见 {@link MIN_VISIBLE_BAR_PX}，保证「有量必有柱」。
 *
 * ## 柱间距：固定像素，且**禁用 `maxBarSize`**（原型 `.cbar` = `flex` + `gap: 2px`）
 *
 * recharts 的排布是「先按整条带（band）算宽，再用 `maxBarSize` 压窄并**在带内居中**」
 * （`getBarPosition` 里的 `(originalSize - size) / 2`）⇒ 被压掉的宽度**全部变成空白**，
 * 而不是让整排柱子收拢。实测「近 7 天」：带宽 156px、`barCategoryGap="18%"`、`maxBarSize={28}`
 * ⇒ 柱只有 26px，两柱之间空出 **130px**（截图逐像素量得），看起来像「一天一根孤零零的柱」。
 *
 * 原型的写法是「柱子填满带、缝固定 2px」，所以这里：
 * - 去掉 `maxBarSize`（它正是「柱窄而缝宽」的根因）；
 * - `barCategoryGap` 给**像素值**而非百分比 —— recharts 把它**在带的两侧各扣一份**
 *   （`bandSize - 2 * offset`），实际缝隙 = 2 × 本值；取 {@link BAR_CATEGORY_GAP_PX}
 *   ⇒ 相邻两柱之间 4px（再加上描边 1px 的外溢，肉眼约 2~4px，与原型一致）。
 *
 * 副作用是「天数少时柱变胖」（近 7 天每根约 150px）—— 这正是原型的行为（`.cbar` 的
 * 列是 `flex: 1`），也是「填满」而非「留空」的必然结果。
 */

import { useMemo, type ReactElement } from "react";
import { Bar, BarChart, CartesianGrid, Rectangle, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { useT } from "@/lib/i18n";
import { GATEWAY_SEGMENT, TOTAL_SEGMENT, type DailyModelColumn } from "@/lib/usage-charts";
import { LEDGER_CLIENT_SEGMENT, LEDGER_GATEWAY_SEGMENT } from "@/lib/usage-ledger";
import { LEDGER_CLIENT_COLOR, LEDGER_GATEWAY_COLOR } from "./ledger-parts";

import { compactTokens } from "./format";
import { seriesColor } from "./palette";

/**
 * 有量的日列**最小可见高度**（px）。
 *
 * ## 为什么必须有
 *
 * 轴被单日峰值撑满时（实测 800B / 约 171px ⇒ 每像素 ≈ 4.7B），0.5B ~ 1B 的一天
 * 只有 0.1 ~ 0.2px —— **不产生任何可见像素**。没有保底，用户看到的就是
 * 「这些天没有数据」，而这与事实相反（「图不能撒谎」红线）。
 *
 * ## 为什么只抬「每列最上面那一段」
 *
 * 抬高度就是**轻微失真**，所以要把失真压到最小：只保证**整列**至少
 * `MIN_VISIBLE_BAR_PX` 高，而不是每个分段都保底。后者在「7 个模型都很小」的一天
 * 会把 1px 的一天画成 14px（放大一个数量级）。逐列保底时失真上界是
 * **每列 2px**，且只对本来就不足 2px 的列生效。
 *
 * ## 为什么不用 recharts 的 `minPointSize`
 *
 * 它**逐段**生效，且**数字型**对 `value` 为 0 的分段同样返回保底值。实测：
 * `minPointSize={2}` 下 30 天 × 7 模型 = **210 根柱子**（连「当天零用量」的
 * 10/02 也被画出 2px 的假柱）—— 那是编造数据，直接违反「图不能撒谎」。
 * 实测数据见 `logs/_v-numeric-min.json`（DOM 列数 30）对
 * `logs/_v-shape-min2.json`（DOM 列数 23）。
 */
const MIN_VISIBLE_BAR_PX = 2;

/**
 * 低于此高度的分段**不描边**（px）。
 *
 * 描边是 2px 宽、颜色等于背景色，画在填充之上 ⇒ 高度 ≤ 2px 的柱子会被完全涂掉。
 * 取 6 是为了给「上下各 1px 描边」留出至少 4px 的可见填充。
 */
const BAR_STROKE_MIN_PX = 6;

/**
 * 柱间缝隙的一半（px）—— recharts 在带的两侧各扣一份，实际缝 = 2 × 本值。
 * 详见文件头「柱间距」一节：原型 `.cbar` 的 `gap: 2px`，此处取 2 ⇒ 相邻柱之间 4px。
 */
const BAR_CATEGORY_GAP_PX = 2;

/** recharts 传给 `Bar.shape` 的入参（只列用到的字段）。 */
interface BarShapeProps {
  x?: number | string;
  y?: number | string;
  width?: number | string;
  height?: number | string;
  radius?: number | [number, number, number, number];
  fill?: string;
  /** 类别下标（= 日列下标），由 recharts 在 `renderRectanglesStatically` 注入。 */
  index?: number;
}

/**
 * 渲染一个堆叠分段：**零值不画**，非零且为「本列最高段」时保底 {@link MIN_VISIBLE_BAR_PX}。
 *
 * `topKeyByColumn` 由本组件按 `rows` 算出（每列最后一个值 > 0 的系列键）。
 */
function visibleBar(
  props: BarShapeProps,
  seriesKey: string,
  topKeyByColumn: (string | null)[],
): ReactElement | null {
  const height = Number(props.height ?? 0);
  // 真·零值：不画。这里必须返回 null —— 给 0 值一个保底高度就是编造柱子。
  if (!(height > 0)) return null;

  const columnIndex = Number(props.index ?? -1);
  const lifted =
    topKeyByColumn[columnIndex] === seriesKey ? Math.max(height, MIN_VISIBLE_BAR_PX) : height;
  const outlined = lifted >= BAR_STROKE_MIN_PX;

  return (
    <Rectangle
      x={Number(props.x ?? 0)}
      // 向上补齐：底边仍贴住堆叠基座，不改变分段在栈里的相对位置。
      y={Number(props.y ?? 0) + height - lifted}
      width={Number(props.width ?? 0)}
      height={lifted}
      radius={props.radius}
      fill={props.fill}
      stroke={outlined ? "var(--background)" : "none"}
      strokeWidth={outlined ? 2 : 0}
    />
  );
}

/**
 * 把 {@link visibleBar} 适配成 recharts `Bar.shape` 需要的签名。
 *
 * recharts 的类型是 `(props: unknown) => JSX.Element`，**不接受 `null`**；但 React 运行时
 * 支持返回 `null`，而「零值分段不画节点」正是本组件要的语义 ⇒ 在边界处做一次显式收窄，
 * 而不是为了迁就类型去渲染一堆空 `<g>`（30 天 × 7 模型 = 210 个）。
 */
function barShape(seriesKey: string, topKeyByColumn: (string | null)[]) {
  return ((props: unknown) =>
    visibleBar(props as BarShapeProps, seriesKey, topKeyByColumn)) as (props: unknown) => ReactElement;
}

export function StackedDailyBar({
  columns,
  palette,
  valueFormat,
}: {
  columns: DailyModelColumn[];
  /**
   * 指定每个分段名的颜色（**按出现顺序**）。
   *
   * ## 为什么需要
   *
   * 默认配色是「按出现顺序取 `--s1..s8`」—— 那对「按模型堆叠」没问题（模型之间没有
   * 语义先后）。但「按账本堆叠」时**颜色本身是语义**：客户端必须青绿、网关必须靛蓝，
   * 否则用户会把两本账读成「两个模型」，而这两个色相正是页面上唯一的账本标识。
   */
  palette?: string[];
  /**
   * 数值格式化（默认按 **token** 的 `1.2M` / `340K`）。
   *
   * 积分视图复用本组件画「每日积分消耗」，但积分是小数（`1.28`），
   * 用 token 的 `K/M` 会把它显示成 `1.3K` —— 单位错了，图就撒了谎。
   */
  valueFormat?: (value: number) => string;
}) {
  const t = useT();

  const { rows, series, config, topKeyByColumn } = useMemo(() => {
    const names: string[] = [];
    for (const column of columns) {
      for (const segment of column.segments) {
        if (!names.includes(segment.name)) names.push(segment.name);
      }
    }
    const series = names.map((name, index) => ({
      key: `m${index}`,
      name,
      /* ★ 账本段的颜色**不能**按出现顺序取（2026-10-28 修）。
         `ledgerDailySeries` 只在「该段有量」时才产出段 ⇒ 某天只有网关有量、另一天只有
         客户端有量时，`names` 的首次出现顺序会**反转**，于是两本账的颜色互换 ——
         而颜色在账本视图里**就是语义**（客户端青绿 / 网关靛蓝，覆盖条与账本表同色）。
         所以这两个哨兵名绑死颜色；其余名字（模型 id）才走调色板。 */
      color:
        name === LEDGER_CLIENT_SEGMENT
          ? LEDGER_CLIENT_COLOR
          : name === LEDGER_GATEWAY_SEGMENT
            ? LEDGER_GATEWAY_COLOR
            : (palette?.[index] ?? seriesColor(index)),
    }));
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

    // 每列「最上面那一段」的系列键 —— 只有它需要保底高度（见 MIN_VISIBLE_BAR_PX）。
    const topKeyByColumn = rows.map((row) => {
      let top: string | null = null;
      for (const item of series) {
        if (Number(row[item.key]) > 0) top = item.key;
      }
      return top;
    });

    const config: ChartConfig = {};
    for (const item of series) {
      config[item.key] = {
        // 两个哨兵名由渲染层翻译：`__total__`（无模型明细时的合计段）与
        // `__gateway__`（网关逐日图的单段）——其余名字是模型 id，原样进 legend。
        label:
          item.name === TOTAL_SEGMENT
            ? t("wbStats.usage.trend.totalSegment")
            : item.name === GATEWAY_SEGMENT
              ? t("wbStats.usage.trend.gatewaySegment")
              : item.name === LEDGER_CLIENT_SEGMENT
                ? t("wbStats.usage.ledger.client")
                : item.name === LEDGER_GATEWAY_SEGMENT
                  ? t("wbStats.usage.ledger.gateway")
                  : item.name,
        color: item.color,
      };
    }
    return { rows, series, config, topKeyByColumn };
  }, [columns, t, palette]);

  if (columns.length === 0 || series.length === 0) return null;

  return (
    <div className="min-w-0 space-y-3.5">
      <ChartContainer config={config} className="h-[210px] w-full">
        <BarChart
          data={rows}
          margin={{ top: 8, right: 8, left: 0, bottom: 0 }}
          barCategoryGap={BAR_CATEGORY_GAP_PX}
        >
          <CartesianGrid vertical={false} strokeDasharray="3 3" />
          <XAxis
            dataKey="label"
            tickLine={false}
            axisLine={false}
            tickMargin={8}
            minTickGap={columns.length > 12 ? 0 : 24}
            interval={columns.length > 12 ? 2 : 0}
          />
          <YAxis tickLine={false} axisLine={false} width={48} tickFormatter={(value) => (valueFormat ?? compactTokens)(Number(value))} />
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
              // 不设 maxBarSize：它会把柱压窄并在带内居中，空隙全部留白（见文件头「柱间距」）。
              radius={index === series.length - 1 ? [4, 4, 0, 0] : 0}
              isAnimationActive={false}
              // 描边与否由 visibleBar 逐段决定（矮柱描边会被自己涂掉），故不在此处设 stroke。
              shape={barShape(item.key, topKeyByColumn)}
            />
          ))}
        </BarChart>
      </ChartContainer>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {series.map((item) => (
          <span
            key={item.key}
            className="inline-flex items-center gap-1.5 text-[11.5px] text-muted-foreground"
          >
            <span
              className="size-[9px] shrink-0 rounded-[3px]"
              style={{ backgroundColor: item.color }}
              aria-hidden="true"
            />
            {config[item.key]?.label ?? item.name}
          </span>
        ))}
      </div>
    </div>
  );
}
