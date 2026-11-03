/**
 * 资源包明细的**唯一装配口径**（issue #144 复核 §5 的 L1 + L2）。
 *
 * ## 为什么要有这一层
 *
 * 「资源包明细」这件事此前有**四份实现**：WB / Trae / Qoder 三个渠道页各一段
 * `detailPacks`，聚合页又一份 `aggregate-detail.ts`。四份各自决定「过滤什么、怎么排、
 * `ratio` 乘不乘 100、临期窗口几天」，于是同一件事在四个页面上给出四种答案：
 *
 * - 聚合页 `ratio` 忘了 `* 100` ⇒ 进度条只有 1/100 宽（#144 本体）；
 * - 聚合页 Trae 分支把 **Unix 秒**当毫秒渲染 ⇒ 到期列全是 `1970-01-22`（#144 回归）；
 * - 渠道页抽屉把**已过期**的包按到期升序排到**最前**，卡片却垫底；
 * - Qoder 聚合页过滤 `remaining > 0`、渠道页一个都不过滤 —— 修复把两者推向相反方向。
 *
 * 这类错误的共同点：**契约（`DetailPack`）写在共享组件里，实现散在每个调用方**。
 * 每加一个调用方就多一次踩契约的机会。所以把最容易错的三件事（量纲、过滤、排序）
 * 收归一处，只把**文案**留给平台 —— 文案域本来就要分开（混用会串），
 * 而量纲/排序错了不会报错，只会显示成一个「看起来挺合理」的错数字。
 *
 * ## 三条硬口径
 *
 * ① **量纲在入口归一**：本模块只认 `expireAtMs`（epoch 毫秒）。Trae 上游给的是
 *    **秒**，`* 1000` 只允许出现在 Trae 的适配层（`traePacks`），且由门禁钉住。
 *    签名里没有「可能秒可能毫秒」的余地 —— 让编译器替我们记住这件事。
 * ② **`remaining <= 0` 不列**：`#143` / `#144` 共用的判据。「剩 0」不是「还有 0 可用」，
 *    它只是历史记录，列出来会让人以为还有额度。
 * ③ **排序**：已过期整体垫底（已经浪费掉的别抢占视线）→ 未过期按到期升序 →
 *    `expireAtMs === null`（永久有效）排在未过期的最后。
 *
 * ⚠️ **临期窗口不在这里推**：`expiringSoon` 只认后端下发的标记（`credit-expiry.ts`
 * 文件头那条规范）。Qoder 没有后端标记 ⇒ 由它的适配层用 `qoder-packs.ts` 的
 * `EXPIRING_SOON_MS` 显式算好再传进来，本模块不猜。
 */

import type { DetailPack } from "@/components/account-detail-drawer";

/**
 * 归一后的一个资源包。
 *
 * 字段名刻意用 `expireAtMs`（而不是上游的 `expireAt`）：一旦有人把秒口径的值塞进来，
 * 变量名与签名就对不上，评审时看得见。
 */
export interface CreditPackInput {
  /** 稳定的 React key。 */
  key: string;
  /** 已过 `displayText` 出口的包名（对象当子节点会白屏，issue #2）。 */
  name: string;
  total: number;
  remaining: number;
  used: number;
  /** 到期时刻，**epoch 毫秒**；`null` = 永久有效（不是「未知」）。 */
  expireAtMs: number | null;
  /** 后端 `expired` 标记（缺省 = 未知）。 */
  expired?: boolean;
  /** 后端 `expiringSoon` 标记（缺省 = 未知，**不推测**）。 */
  expiringSoon?: boolean;
}

/** 平台文案。四段文本由各平台给（词条域不同，混用会串），逻辑由本模块统一。 */
export interface CreditPackTexts {
  remainingText: (value: number) => string;
  totalText: (value: number) => string;
  usedText: (value: number) => string;
  /**
   * 到期文案。
   *
   * @param expireAtMs `null` ⇒ 永久有效（**必须**给专门的词条，不要退回 `—`）
   * @param expired 已到期
   */
  expiryText: (expireAtMs: number | null, expired: boolean) => string;
}

/** 归一后的展示行（`ratio` 已夹紧到 0..100）。 */
export interface NormalizedPack extends CreditPackInput {
  expired: boolean;
  expiringSoon: boolean;
  /** 0..100，已夹紧 —— `DetailPack.ratio` 的契约。 */
  ratio: number;
}

/**
 * 一个包是否已到期。
 *
 * ★ 取「后端标记 **或** 到期时刻已过」的并集，而不是只信 `expired`：
 * 后端 `expired` 覆盖的区间与「`expire_at <= now`」是不是恒等，是**后端的不变量**，
 * 前端依赖它就成了隐式耦合（后端口径一改，界面就把过期包标成临期）。
 * 反过来多算一次也不会错：过去时刻就是过期。
 */
export function packIsExpired(pack: CreditPackInput, now: number): boolean {
  if (pack.expired) return true;
  return pack.expireAtMs !== null && Number.isFinite(pack.expireAtMs) && pack.expireAtMs <= now;
}

/**
 * `DetailPack.ratio` 的**唯一算式**：0..100 且已夹紧（抽屉直接拼 `%`）。
 *
 * 四个调用方里曾经有三份一模一样的 `Math.min(100, Math.max(0, …))`、一份忘了 `* 100`
 * —— 抽成函数是为了让「漏乘 100」变成不可能，而不是靠评审记得。
 */
export function packRatio(remaining: number, total: number): number {
  if (!(total > 0) || !Number.isFinite(remaining)) return 0;
  return Math.max(0, Math.min(100, (remaining / total) * 100));
}

/**
 * Unix **秒** → epoch **毫秒**。
 *
 * ★ 全仓**唯一**允许把 Trae 的 `expireAt` 换算成毫秒的地方（两个调用点：
 * `TraeAccountsPage` 的抽屉装配、`aggregate-detail.ts` 的 Trae 分支）。
 *
 * 为什么要单独成一个函数而不是就地 `* 1000`：`#144` 那个「到期列全是 1970-01-22」
 * 就是「就地乘」漏了一次。有名字的函数可以被门禁跑一次真断言，`* 1000` 不能。
 */
export function secondsToMs(seconds: number | null): number | null {
  return seconds === null ? null : seconds * 1000;
}

/**
 * 过滤 + 排序 + 算比例。
 *
 * 入参**不会被修改**（`sort` 在副本上做）—— 调用方常常直接传 store 里的数组。
 */
export function normalizePacks(
  packs: readonly CreditPackInput[] | null | undefined,
  now: number = Date.now(),
): NormalizedPack[] {
  if (!packs || packs.length === 0) return [];

  const rows = packs
    /* ② `remaining <= 0` 不列；`total <= 0` 是「不计量」的那类（Qoder 真机上 `plan_quota`
       的 `limit_value = 0`），它的「0 / 0」会被读成「用完了」，同样不列。 */
    .filter((pack) => Number.isFinite(pack.remaining) && pack.remaining > 0 && pack.total > 0)
    .map((pack) => {
      const expired = packIsExpired(pack, now);
      return {
        ...pack,
        expired,
        /* ⚠️ 只认后端标记（`=== true`）：缺字段 = 未知，不推测窗口。
           已过期的不再算临期（两枚徽章同时亮是自相矛盾）。 */
        expiringSoon: !expired && pack.expiringSoon === true,
        ratio: packRatio(pack.remaining, pack.total),
      } satisfies NormalizedPack;
    });

  return rows.sort(byExpiry);
}

/** ③ 排序：已过期垫底 → 未过期按到期升序 → `null`（永久）垫在未过期的最后。 */
function byExpiry(left: NormalizedPack, right: NormalizedPack): number {
  if (left.expired !== right.expired) return left.expired ? 1 : -1;
  const leftAt = left.expireAtMs ?? Number.POSITIVE_INFINITY;
  const rightAt = right.expireAtMs ?? Number.POSITIVE_INFINITY;
  return leftAt - rightAt;
}

/**
 * 归一 → `DetailPack[]`（抽屉入参）。
 *
 * `expiryText` 在这里生成而不是由调用方拼：四个调用方各自拼日期格式，正是
 * 「同一句话两处各写一遍」的来源。
 */
export function toDetailPacks(
  packs: readonly CreditPackInput[] | null | undefined,
  texts: CreditPackTexts,
  now: number = Date.now(),
): DetailPack[] {
  return normalizePacks(packs, now).map((pack) => ({
    key: pack.key,
    name: pack.name,
    remainingText: texts.remainingText(pack.remaining),
    totalText: texts.totalText(pack.total),
    usedText: texts.usedText(pack.used),
    expiryText: texts.expiryText(pack.expireAtMs, pack.expired),
    expired: pack.expired,
    expiringSoon: pack.expiringSoon,
    ratio: pack.ratio,
  }));
}
