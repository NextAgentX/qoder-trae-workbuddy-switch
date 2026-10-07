/**
 * 用量图表 · **图注生成器**（P3，纯函数）。
 *
 * ## 为什么要有统一出口
 *
 * 旧页面的图注会写「峰值 = 末值」这类**看起来像结论、其实是废话**的句子
 * （PLAN §3.10 第 5 条点名）。把图注收敛到一个生成器，能让「口径」变成**代码契约**：
 *
 * - 累计曲线：写「累计 + 单日峰值」，**不写「峰值 = 末值」**；
 * - 余额曲线：**必须写明基线非 0**（否则读者以为轴从 0 起，高位波动被误读成没变）；
 * - 序列超长：注明「已按周聚合」（不默默丢点）；
 * - 会话散点：注明「横轴 = 记录数」（**不是消息数** —— 真实后端没有消息数字段）。
 *
 * ## 为什么函数多带一个 `t`
 *
 * 设计原签名是 `cumulativeCaption(v, peak)`。但本仓 zh + en **必须同步**，
 * 图注是面向用户的文案 ⇒ 生成器需要 `t` 才能出本地化文本。故每个函数**首参为 `t`**
 * （类型 `CaptionTranslate`，只依赖 `TranslationKey` 的**类型**导入 ⇒ 本模块零运行时依赖，
 * 可被门禁 `loadTs` 直接求值）。
 *
 * ⚠️ 图注**只**从这里产出：调用方**不得手写**图注字符串（防「废话」回流）。
 */

import type { TranslationKey } from "@/locales/zh";

/** 与 `lib/i18n.ts` 的 `Translate` 同形；此处本地声明以保持零运行时导入。 */
export type CaptionTranslate = (
  key: TranslationKey,
  params?: Record<string, string | number>,
) => string;

const compact = new Intl.NumberFormat("en-US", {
  notation: "compact",
  compactDisplay: "short",
  maximumFractionDigits: 1,
});

/** 紧凑数字（12.3K / 1.2M），单位字母大写。 */
function compactNumber(value: number): string {
  if (!Number.isFinite(value)) return "—";
  return compact.format(value).replace(/([kmb])$/i, (unit) => unit.toUpperCase());
}

/** 累计消耗曲线图注：`区间累计 {total}（单日峰值 {peak}）`。 */
export function cumulativeCaption(t: CaptionTranslate, total: number, peak: number): string {
  return t("wbStats.usage.caption.cumulative", {
    total: compactNumber(total),
    peak: compactNumber(peak),
  });
}

/**
 * 积分余额曲线图注。
 *
 * `baseline > 0` ⇒ 写明「非 0」；若基线确实落在 0（曲线触零），
 * 则**不写「非 0」**（写就是撒谎），改用不含该措辞的变体。
 */
export function balanceCaption(
  t: CaptionTranslate,
  first: number,
  last: number,
  baseline: number,
): string {
  if (baseline > 0) {
    return t("wbStats.usage.caption.balance", {
      first: compactNumber(first),
      last: compactNumber(last),
      baseline: compactNumber(baseline),
    });
  }
  return t("wbStats.usage.caption.balanceZero", {
    first: compactNumber(first),
    last: compactNumber(last),
  });
}

/** 会话散点横轴语义：**记录数**，不是消息数。 */
export function scatterAxisCaption(t: CaptionTranslate): string {
  return t("wbStats.usage.caption.scatterAxis");
}

/** 单向柱说明：入账明细不可得（P3-ARCH §6 #1）。 */
export function singleSideCaption(t: CaptionTranslate): string {
  return t("wbStats.usage.caption.singleSide");
}
