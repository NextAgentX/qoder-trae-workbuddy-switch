/**
 * 「建议优先」的判定与排序（issue #69 / #89 · B6）。
 *
 * ## 为什么抽成模块
 *
 * 这四条判据原先写在 `AccountsPage.tsx` 里（页面私有），聚合页要显示同一个
 * ⭐「建议优先」徽章就只能再抄一份 —— 而「同一个量两处各算一次」正是本仓
 * 反复出现的分叉来源。抽出来之后账号页与聚合页**共用同一份口径**：
 * 徽章出现在哪个账号上，两处必然一致。
 *
 * ## 分档（rank 越小越优先）
 *
 * | rank | 条件 | 含义 |
 * |:--|:--|:--|
 * | 0 | 临期窗口内（后端 `expiringSoon`）有剩余 | **再不用就浪费**（警告强度最高） |
 * | 1 | 正常（有未来到期 / 无到期） | **先消耗快到期的**（用户真正要的顺序） |
 * | 2 | 有**已过期**积分 | 已经浪费了 ⇒ 排到正常之后，别抢「先消耗」的位 |
 * | 3 | 积分查询失败 | 未知排最后 |
 *
 * ⚠️ 2026-10-21 前 rank 1 = 已过期、rank 2 = 正常 ⇒ 「有过期积分」的账号排在
 * 「14 天后到期」的账号**前面** —— 那正好把用户要的相对顺序倒过来了。
 */

import type { CreditExpiry } from "@/lib/types";
import { nearestExpiringBatch } from "@/lib/credit-expiry";

/** 临期窗口内的剩余积分（查询失败 / 无数据按 0）。 */
export function expiringSoonAmount(credit?: CreditExpiry): number {
  return credit?.ok ? credit.expiringSoonRemaining ?? 0 : 0;
}

/** 是否有临期积分（**只有 `ok === true` 且金额 > 0** 才算，未知不算）。 */
export function hasExpiringSoonCredits(credit?: CreditExpiry): boolean {
  return credit?.ok === true && expiringSoonAmount(credit) > 0;
}

/**
 * 「最近一批未来到期」的到期时刻；没有则 `Infinity`（排最后）。
 *
 * ⚠️ **不看 `expiringSoon`**（那只是警告窗口），**不看 `credit.soonestExpireAt`**
 * （它把已过期资源也算进最小值 ⇒「已经浪费的」会抢占「先消耗」的位）。
 * 统一走 `nearestExpiringBatch` —— 只认「未来 + 剩余 > 0 + 有到期时刻」。
 */
export function nearestCreditExpiry(credit?: CreditExpiry): number {
  const batch = credit?.ok ? nearestExpiringBatch(credit.resources) : null;
  return batch ? batch.expireAt : Number.POSITIVE_INFINITY;
}

/** 分档（见文件头表格）。 */
export function creditPriorityRank(credit?: CreditExpiry): number {
  if (!credit?.ok) return 3;
  if (hasExpiringSoonCredits(credit)) return 0;
  if (credit.expired) return 2;
  return 1;
}

/**
 * 「积分优先级」比较器 —— **账号页的排序**与**聚合页的徽章**共用这一个。
 *
 * 三级键：分档 → 最近到期升序 → 临期金额降序。
 */
export function compareCreditPriority(left?: CreditExpiry, right?: CreditExpiry): number {
  const rankDifference = creditPriorityRank(left) - creditPriorityRank(right);
  if (rankDifference !== 0) return rankDifference;
  const leftExpiry = nearestCreditExpiry(left);
  const rightExpiry = nearestCreditExpiry(right);
  if (leftExpiry !== rightExpiry) return leftExpiry - rightExpiry;
  return expiringSoonAmount(right) - expiringSoonAmount(left);
}

/**
 * 挑出「该给 ⭐ 建议优先 徽章」的账号 id（**每组只给一个**）。
 *
 * 语义与账号页逐字一致：按积分优先级排一遍，取**第一个**有临期积分的账号 ——
 * 没有临期积分的组返回 `undefined`（徽章不出现，而不是给一个「其实不紧急」的号）。
 *
 * @param rows 同一作用域内的账号
 * @param creditOf 取该账号的积分快照
 */
export function pickPriorityAccountId<T extends { id: string }>(
  rows: readonly T[],
  creditOf: (row: T) => CreditExpiry | undefined,
): string | undefined {
  /* `Array.prototype.sort` 自 ES2019 起保证稳定 ⇒ 与账号页「先 sortAccounts 再 find」
     的结果逐项相同（相等项保持原序）。 */
  return [...rows]
    .sort((left, right) => compareCreditPriority(creditOf(left), creditOf(right)))
    .find((row) => hasExpiringSoonCredits(creditOf(row)))?.id;
}
