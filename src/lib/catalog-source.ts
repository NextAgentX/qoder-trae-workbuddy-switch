/**
 * 模型清单「来源条」的展示契约（P10 §3.1）。
 *
 * ## 为什么把「来源」收敛成一份契约，而不是各页各写一段 JSX
 *
 * 改造前 `model-list.tsx` 把来源徽标 + 时间 + `note` 直接摊在 JSX 里，于是有两个洞：
 *
 * 1. **「非实时」可以没有原因** —— 后端 `catalog.rs` 的 `load_cached` 把 `note` 置 `None`
 *    （`catalog.rs:252-266`），一份「已保存」快照就只剩一个灰徽标。用户分不清
 *    「还在加载」与「拉不到」，只能猜。把「原因」从**可选通道**升格为**契约**：
 *    `kind !== "live"` ⇒ `reason` **恒非空**。
 * 2. **同一份「来源」语义会在两平台各写一遍**（WB 三级 / Trae 客户端缓存），迟早漂移。
 *
 * ## 两条不变量（可证伪）
 *
 * - ★ `kind !== "live"` ⇒ `reason` **恒非空**：后端 `note` 优先，缺失时 i18n 兜底。
 * - ★ 「从未识别成功」（`builtin` 且 `identifiedAt === null`）⇒ 原因**必须**含
 *   「尚未成功识别过」—— 否则用户会把「内置兜底」当成「实时清单」照单调用而扑空。
 *
 * ⚠️ 后端 `refresh()` 失败且无缓存时会走 `builtin_snapshot(region, Some(DEGRADE_NOTE))`
 *    （`catalog.rs:237`），即**内置快照可能带 note**。故「从未识别」这一支不能只靠
 *    「note 缺失才兜底」——必须显式把兜底措辞拼进去（见 `describeCatalogSource`）。
 *
 * ⚠️ 展示字段出口归一：`snapshot.note` 声明是 `string | null`，但**运行时不保证**是字符串
 *    （脏数据可能是对象）—— 直接当 React 子节点会卸载整棵树 ⇒ **白屏**（issue #2）。
 *    因此一律过 `displayText`（`@/lib/display-text`）。
 *
 * ⚠️ 本模块**不含任何平台判断**：WB 用 `describeCatalogSource`；Trae 由组件自行组装
 *    `CatalogSourceView`（来源词汇不同，见 P10 §1.2）。共用壳 `CatalogSourceBar` 只按 view 渲染。
 */

import { displayText } from "@/lib/display-text";
import type { Translate } from "@/lib/i18n";
import type { CatalogSource, CatalogSnapshot } from "@/lib/types";
import type { TranslationKey } from "@/locales/zh";

/** 三级来源（与后端 `catalog::CatalogSource` 逐字对齐：live / cached / builtin）。 */
export type CatalogSourceKind = CatalogSource;

/**
 * 来源条的**展示模型** —— 把快照归一成「可渲染的文案契约」。
 *
 * ★ 关键不变量：`kind !== "live"` ⇒ `reason` **恒非空**（绝不返回 `null`）。
 *   这是「不能只给一个灰徽标让用户猜」的可证伪落点。
 */
export interface CatalogSourceView {
  /** 来源种类（驱动徽标语义色）。 */
  kind: CatalogSourceKind;
  /** 徽标文案（**已 i18n**；调用方用 `t()` 渲染）。 */
  badgeLabel: string;
  /** 徽标语义色（`success` = 实时；`secondary` = 已保存；`warning` = 内置）。 */
  badgeVariant: "success" | "secondary" | "warning";
  /** 上次识别成功时间（ms）；从未成功（builtin）为 `null`。 */
  identifiedAt: number | null;
  /** ★ 非「实时」时的原因；`live` 恒为 `null`，非 live 恒非空。 */
  reason: string | null;
  /** 已识别的模型数（**不是**「可用」数 —— HTTP 200 ≠ 可用，§3.17.3 陷阱 3）。 */
  identifiedCount: number;
}

/** 来源 → 徽标语义色（三级固定映射，不给调用方留「伪装」余地）。 */
const VARIANT: Record<CatalogSourceKind, CatalogSourceView["badgeVariant"]> = {
  live: "success",
  cached: "secondary",
  builtin: "warning",
};

/** 来源 → 徽标文案键（**只存键不存文案**：语言切换时整张表才会跟着变）。 */
const BADGE_KEY: Record<CatalogSourceKind, TranslationKey> = {
  live: "wbStats.gateway.sourceLive",
  cached: "wbStats.gateway.sourceCached",
  builtin: "wbStats.gateway.sourceBuiltin",
};

/** 非实时来源的**兜底原因**键（后端 `note` 缺失时使用）。 */
const REASON_KEY: Record<Exclude<CatalogSourceKind, "live">, TranslationKey> = {
  cached: "wbStats.gateway.reasonCached",
  builtin: "wbStats.gateway.reasonBuiltin",
};

/**
 * 归一来源快照。`snapshot === null`（尚未加载）按「内置 + 尚未识别」处理。
 *
 * ⚠️ 原因来源优先级：① 后端 `note`（经 `displayText` 归一）；② 缺失时 i18n 兜底。
 *    **例外**：「从未识别成功」的内置快照**无条件**带上兜底措辞（后端可能给了
 *    `DEGRADE_NOTE`，但那条不含「尚未成功识别过」，不满足契约）。
 */
export function describeCatalogSource(
  snapshot: CatalogSnapshot | null,
  t: Translate,
): CatalogSourceView {
  const kind = snapshot?.source ?? "builtin";
  const identifiedAt = snapshot?.fetched_at ?? null;
  const backendReason = displayText(snapshot?.note);
  const neverIdentified = kind === "builtin" && identifiedAt === null;

  const reason = kind === "live"
    ? null
    : neverIdentified
      ? backendReason
        ? `${t("wbStats.gateway.reasonBuiltin")} · ${backendReason}`
        : t("wbStats.gateway.reasonBuiltin")
      : backendReason ?? t(REASON_KEY[kind]);

  return {
    kind,
    badgeLabel: t(BADGE_KEY[kind]),
    badgeVariant: VARIANT[kind],
    identifiedAt,
    reason,
    identifiedCount: snapshot?.models.length ?? 0,
  };
}

/**
 * 相对时间文案（「刚刚 / {n} 分钟前 / {n} 小时前 / {n} 天前」）；`ts === null` ⇒ `null`。
 *
 * `now` 可注入（便于测试与 SSR 场景）；缺省取当前时间。
 */
export function formatRelativeTime(
  ts: number | null,
  t: Translate,
  now: number = Date.now(),
): string | null {
  if (ts === null || !Number.isFinite(ts)) return null;
  const diff = Math.max(0, now - ts);
  if (diff < 60_000) return t("shared.gateway.sourceBar.justNow");
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 60) return t("shared.gateway.sourceBar.minutesAgo", { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("shared.gateway.sourceBar.hoursAgo", { n: hours });
  const days = Math.floor(hours / 24);
  return t("shared.gateway.sourceBar.daysAgo", { n: days });
}

/** 绝对时间（本地化 `YYYY-MM-DD HH:mm`），供 `title` 悬停显示（§3.17.4「悬停给绝对时间」）。 */
export function formatAbsoluteTime(ts: number | null): string {
  if (ts === null || !Number.isFinite(ts)) return "";
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    ` ${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}
