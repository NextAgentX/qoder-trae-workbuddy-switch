/**
 * Qoder 资源包（积分包）的展示口径 —— issue #111。
 *
 * 单独成文件而不是塞进页面：**状态判定与阈值**是会被两个地方用到的（抽屉列表、
 * 后续的卡片浮层），散落两处必然漂移（本仓已有太多这类教训）。
 */

import type { QoderCreditPack } from "@/lib/api";
import {
  summarizeExpiring,
  type ExpiringItem,
  type ExpiringSummary,
} from "@/lib/credit-expiry";
import type { Translate } from "@/lib/i18n";

/**
 * 「临期」窗口：**7 天**。
 *
 * ★ 2026-11-09：从 14 天统一到 7 天。必须与后端 `trae::credits::EXPIRING_SOON_SECS`
 * （现 7 天）以及 WorkBuddy 的 `credits.rs::EXPIRING_SOON_DAYS`（7 天）**同值**：
 * 同一批数据在两个产品线给出不同的临期判定，用户会认为工具在乱报。
 * 后端那份是 `i64` 秒，这里是毫秒（前端时间戳统一毫秒）。
 *
 * ⚠️ 此前这里写着「必须与 Trae 的 14 天同值」，于是改 Trae 的窗口会**静默改掉 Qoder 的**
 *   口径，而改 WB 的窗口谁都不跟。现在三个数同值，但仍然分开写（跨语言不能共享常量）。
 */
export const EXPIRING_SOON_MS = 7 * 24 * 3600 * 1000;

/**
 * 逐包数据的**前端缓存有效期**：60 秒。
 *
 * 为什么需要 TTL：浮层打开即请求、卡片与抽屉都会触发 ⇒ 没有 TTL 的话，同一个账号
 * 会被点一次打一次上游（逐包接口要带会话 Cookie，打多了容易被判异常）。
 * 60s 是「够新鲜」与「别打扰上游」的折中；显式「刷新」按钮走 `force`，不受它限制。
 */
export const PACKS_TTL_MS = 60_000;

/** 包级状态（**行内差异**，不往卡片上挂徽章 —— 卡片是固定版式）。 */
export type QoderPackState = "expired" | "expiring" | "normal" | "zero";

/**
 * 判一个包的状态。
 *
 * `zero`：`limit <= 0` —— 真机上 `plan_quota` 那一条就是这种（套餐额度单独计量），
 * 它的「剩余 0/0」不是「用完了」，所以不能标红。
 */
export function packState(pack: QoderCreditPack, now: number): QoderPackState {
  if (pack.expiresAtMs !== undefined && pack.expiresAtMs <= now) return "expired";
  if (pack.limit <= 0) return "zero";
  if (
    pack.expiresAtMs !== undefined &&
    pack.expiresAtMs - now <= EXPIRING_SOON_MS
  ) {
    return "expiring";
  }
  return "normal";
}

/** 上游 `source` 取值 → 本地化标签。未知取值兜底成「资源包」，**不显示原始枚举串**。 */
export function packSourceLabel(
  t: Translate,
  source: string | undefined,
): string {
  switch (source) {
    case "PLAN":
      return t("qoder.page.accounts.web.sourcePlan");
    case "RESOURCE_PACKAGE_SOURCE_BONUS":
      return t("qoder.page.accounts.web.sourceBonus");
    case "RESOURCE_PACKAGE_SOURCE_DEDICATED":
    case "DEDICATED":
      return t("qoder.page.accounts.web.sourceDedicated");
    default:
      // 上游可能新增取值：给通用名，别把 `SOME_NEW_ENUM` 直接怼到用户脸上。
      return t("qoder.page.accounts.web.sourceOther");
  }
}

/* ══════════════════════ 到期汇总（issue #166）══════════════════════
 *
 * ## 为什么放在这里而不是卡片里
 *
 * 「临期」与「最近到期」是**同一份逐包数据的两种问法**，而逐包的清洗（无到期时刻 /
 * 剩余 ≤ 0 / 已过期）已经有 `credit-expiry` 的纯函数在做。卡片里再写一遍过滤与合并，
 * 就会出现「卡片说 300，浮层列出来 450」这种同一件事两个数字的老毛病。
 *
 * ## 与 WorkBuddy 的口径关系
 *
 * WB 卡的黄徽章（`N 积分 M 天内过期`）用**后端 `expiringSoon`**；Qoder 的逐包响应里
 * **没有这个标记**，所以这里用 `EXPIRING_SOON_MS`（7 天）自己判 —— 它是后端窗口的
 * 镜像常量，不是第二个口径（`packState()` 判每行状态用的就是它）。
 *
 * ★ 2026-11-09：中性徽章（`最近 N 积分 · X 天后过期`）已删除 —— 三平台只保留 7 天窗口内
 *   的黄徽章。所以本文件不再需要「不限窗口」的那两个薄包装（`qoderNearestExpiry` /
 *   `qoderUpcomingExpiries`）：卡片的「优先」徽章走 `lib/credit-priority.ts`，它直接用
 *   `credit-expiry` 的 `nearestExpiringBatch`，与本文件的逐包清洗是两条独立的路。
 */

/**
 * 逐包 → `credit-expiry` 的统一入参。
 *
 * ★ `expiresAtMs` 已经是**毫秒**（后端 `expires_at` 秒值只在上游归一过一次，见 #144），
 *   这里再 `* 1000` 会把 2026 年显示成 1970 年。
 */
function expiringItems(
  packs: readonly QoderCreditPack[] | null | undefined,
  now: number,
): ExpiringItem[] {
  if (!packs) return [];
  const items: ExpiringItem[] = [];
  for (const pack of packs) {
    if (typeof pack.expiresAtMs !== "number" || !Number.isFinite(pack.expiresAtMs)) continue;
    if (!Number.isFinite(pack.remaining) || pack.remaining <= 0) continue;
    items.push({
      remaining: pack.remaining,
      expireAt: pack.expiresAtMs,
      expired: pack.expiresAtMs <= now,
      expiringSoon: pack.expiresAtMs - now <= EXPIRING_SOON_MS,
    });
  }
  return items;
}

/**
 * 临期批次汇总（7 天窗口内）。
 *
 * 没有可渲染的批次时返回 `null` —— 调用方据此**整条不渲染**，绝不写一个猜出来的「N 天」。
 */
export function qoderExpiringSummary(
  packs: readonly QoderCreditPack[] | null | undefined,
  now: number = Date.now(),
): ExpiringSummary | null {
  return summarizeExpiring(expiringItems(packs, now), { now });
}
