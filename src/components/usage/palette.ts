/**
 * 用量图表 · **共享色板**。
 *
 * 只用项目既有 CSS 变量（`--data-series-*`），不新增裸色值 —— 浅色/深色主题由变量切换。
 * 四个视图的图例顺序必须一致，否则同一模型在不同图里颜色不同，读者会认错。
 */

export const SERIES_COLORS = [
  "var(--data-series-emerald)",
  "var(--data-series-teal)",
  "var(--data-series-violet)",
  "var(--data-series-amber)",
  "var(--data-series-rose)",
  "var(--data-series-indigo)",
  "var(--data-series-sky)",
  "var(--data-series-lime)",
] as const;

export function seriesColor(index: number): string {
  return SERIES_COLORS[index % SERIES_COLORS.length];
}
