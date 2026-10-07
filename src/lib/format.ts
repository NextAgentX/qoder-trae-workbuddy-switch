/**
 * 通用格式化（**P3 抽取**）。
 *
 * ## 为什么抽出来
 *
 * `formatDate` 在 `components/gateway/api-key-table.tsx` 与
 * `components/gateway/trae-api-key-table.tsx` **各写了一份**，而且**已经漂移**：
 *
 * | | WB 版 | Trae 版 |
 * |:---|:---|:---|
 * | `0` / 空值 | 渲染成 `01-01 08:00`（**假时间**） | 返回 `—` |
 *
 * ⇒ **取更健壮的那个版本**（与 `gateway-snippets` 的合并原则一致：
 * **以"更正确的那个"为基准，而不是取交集** —— 取交集会把缺陷继承下来）。
 *
 * ⚠️ `if (!ts)` 而不是 `if (ts == null)`：时间戳 `0` 不是有效时间，
 * 但它是 **falsy** —— 只有用 falsy 判断才能同时挡住 `0` 和 `undefined`。
 */

/** 格式化为 `MM-DD HH:mm`（本地时区）。无效输入返回 `—`。 */
export function formatDate(ts: number): string {
  if (!ts) return "—";
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "—";
  return `${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}
