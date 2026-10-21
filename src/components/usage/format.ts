/**
 * 用量四视图 · **数字格式化**（纯函数）。
 *
 * 图表轴 / 图例 / tile 都用同一套格式化，避免「同一件事的数字在两处显示成不同样式」。
 * 非有限值统一显示 `—`（不显示 `NaN`）。
 */

const compact = new Intl.NumberFormat("en-US", {
  notation: "compact",
  compactDisplay: "short",
  maximumFractionDigits: 1,
});

const exact = new Intl.NumberFormat("en-US");

const credits = new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 });

/** 紧凑 Token（12.3K / 1.2M），单位字母大写。 */
export function compactTokens(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return compact.format(value).replace(/([kmb])$/i, (unit) => unit.toUpperCase());
}

/** 精确 Token（1,234,567）。 */
export function exactTokens(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return exact.format(value);
}

/** 积分（最多两位小数）。 */
export function formatCredits(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return credits.format(value);
}

/**
 * **带方向符号**的积分（入账 `+` / 消耗 `-`）。
 *
 * 零值不带符号：`-${formatCredits(0)}` 得到的是 `-0`，读起来像解析失败。
 * 「0 消耗」是真实值，照实显示 `0`（与签到流水「拿不到额度就不摆 +0」同一条口径）。
 * 非有限值仍是 `—`（**不带符号**，避免出现 `-—`）。
 */
export function signedCredits(value: number | null | undefined, sign: "+" | "-"): string {
  const text = formatCredits(value);
  if (text === "—" || value === 0) return text;
  return `${sign}${text}`;
}

/** 占比文案：分母为 0 时给 `—`（不给 `0%`，避免「有 0% 数据」的错觉）。 */
export function sharePercent(value: number, total: number): string {
  return total > 0 ? `${((value / total) * 100).toFixed(1)}%` : "—";
}
