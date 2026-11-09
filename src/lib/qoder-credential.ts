/**
 * Qoder 凭据是否已过期 —— **全仓唯一判据**。
 *
 * ## 四个消费方（此前各写一套，判据还不一样）
 *
 * 1. 卡片红徽章 —— `qoder-account-card.tsx::QoderStatusChips`
 * 2. 抽屉「凭据」指标块 —— `QoderAccountsPage.tsx::detailStats`
 * 3. 聚合页「今天有事要做」—— `aggregate-accounts.ts::aggregateNeedsAttention`
 * 4. 聚合页平铺视图的排序依据 —— `aggregate-accounts.ts::aggregateUrgency`
 *
 * 此前 1 带毫秒量级守卫、2 直接 `expiresAt < Date.now()`、3 与 4 用 `expiredAt`（`> 0`），
 * 于是同一个账号在卡片上说「凭据可用」、点开抽屉却是红色「已过期」；而 3 与 4 之间
 * 还会互相打脸 —— 「需处理」计数里没有它，平铺视图却把它排到第一档（最前）。
 *
 * ⚠️ 新增消费方**一律走这里**。第 4 条就是这么漏掉的：上一轮只收了 1/2/3，
 * 而「同一个字段的第二处判据」往往藏在排序逻辑里，不在渲染逻辑里。
 *
 * ## 为什么需要量级守卫
 *
 * `QoderAccountView.expiresAt` 的单位**未取证**：
 * - 我们自己的刷新路径写入的是**毫秒**；
 * - 导入账号那个值没人核过（见 `lib/api.ts::QoderAccountView` 与后端
 *   `qoder/account.rs::set_tokens_for` 的说明）。
 *
 * 秒值（约 `1.7e9`）直接与 `Date.now()`（约 `1.7e12`）比较**恒为**「已过期」：
 * 满屏红徽章、凭据块永远是红的，而账号其实好着。所以只判**毫秒量级**（`> 1e12`）的值；
 * 量级不对 ⇒ 返回 `false` —— 「不知道」不说成「已过期」（同一条纪律：
 * 未知不得说成已知）。
 */

/**
 * 毫秒时间戳的量级下界。
 *
 * `1e12` ms ≈ 2001-09-09。任何 2001 年之后的毫秒时间戳都大于它；而秒值在今天约
 * `1.8e9`，差三个数量级 —— 不会误判。
 */
const MS_MAGNITUDE_THRESHOLD = 1e12;

/**
 * 凭据是否已过期（**只对毫秒量级的值给结论**）。
 *
 * @param expiresAt `QoderAccountView.expiresAt`；`null` / `undefined` ⇒ 「没有到期时间」
 *   （不是「已过期」，也不是「长期有效」—— 由调用方各自决定怎么表述）
 * @param now 判定基准（毫秒），默认 `Date.now()`。一次渲染里多处判定应传同一个值。
 */
export function qoderCredentialExpired(
  expiresAt: number | null | undefined,
  now: number = Date.now(),
): boolean {
  if (expiresAt == null || !Number.isFinite(expiresAt)) return false;
  if (expiresAt <= MS_MAGNITUDE_THRESHOLD) return false;
  return expiresAt < now;
}
