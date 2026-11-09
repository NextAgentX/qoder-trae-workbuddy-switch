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

import type { CreditExpiry, CreditResource } from "@/lib/types";
import { nearestExpiringBatch, summarizeExpiring } from "@/lib/credit-expiry";

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
 * Trae / Qoder 的**逐包数据** → `CreditExpiry` 形状，供本文件的判据复用。
 *
 * ## 为什么需要这一层
 *
 * 「建议优先」原先只有 WorkBuddy 有（判据读 `CreditExpiry`）。Trae / Qoder 的积分
 * 不在这个形状里（Trae 在账号对象的 `creditPackages`，Qoder 在逐包缓存里），
 * 于是那两个平台**一枚徽章都没有** —— 数据都在，只是形状不同，属漏做而不是缺功能。
 *
 * 给它们另写一套排序会让「同一个号在三个分区显示不同的推荐」，所以这里只做形状转换：
 * 判据、分档、胜负手全部走 {@link pickPriorityAccountId}。
 *
 * @param packs 逐包列表；`null` / 空 ⇒ **未知**（返回 `undefined`，不参与推荐）
 * @param options.unit `expireAt` 的单位：Trae 是秒、Qoder 已是毫秒
 * @param options.now 判定基准（毫秒），默认 `Date.now()`。一次渲染里同一批数据可能被
 *   汇总多次（徽章 / 抽屉 / 推荐排序），各调一次 `Date.now()` 会让「今天」漂移几百毫秒 ——
 *   跨秒边界时同一份数据给出两种临期判定。装配层应传同一个值。
 */
export function creditExpiryFromPacks(
  packs:
    | ReadonlyArray<{
        total?: number;
        remaining: number;
        used?: number;
        expireAt: number | null;
        expired?: boolean;
        expiringSoon?: boolean;
        packageCode?: string | null;
        packageName?: string | null;
        status?: number | null;
      }>
    | null
    | undefined,
  options: { unit?: "ms" | "s"; now?: number } = {},
): CreditExpiry | undefined {
  if (!packs || packs.length === 0) return undefined;
  const toMs = options.unit === "s" ? (value: number) => value * 1000 : (value: number) => value;
  const resources: CreditResource[] = packs.map((pack) => ({
    packageCode: pack.packageCode ?? null,
    packageName: pack.packageName ?? null,
    total: pack.total ?? 0,
    remaining: pack.remaining,
    used: pack.used ?? 0,
    status: pack.status ?? null,
    expireAt:
      typeof pack.expireAt === "number" && Number.isFinite(pack.expireAt) ? toMs(pack.expireAt) : null,
    expired: pack.expired === true,
    expiringSoon: pack.expiringSoon === true,
  }));
  return {
    ok: true,
    resources,
    expiringSoonRemaining: summarizeExpiring(resources, { now: options.now })?.total ?? 0,
    expired: resources.some((resource) => resource.expired && resource.remaining > 0),
  };
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
