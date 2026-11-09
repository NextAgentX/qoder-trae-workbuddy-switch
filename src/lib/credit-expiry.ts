/**
 * 「临期积分」的**唯一汇总口径** —— 账号卡片上那枚徽章 + 它的逐批悬浮列表都用它。
 *
 * ## 徽章只报**最快到期的那一批**
 *
 * 徽章文案是「最近到期 {days}天 · {amount} 积分」，两个数字**必须来自同一批**
 * （`batches[0]`）：天数 = 该批距今天数（向上取整、≥1），金额 = 该批剩余积分。
 *
 * ⚠️ 不要退回「总量 + 最快天数」或「总量 + 最晚天数」这类混搭：
 *
 * - 「300 积分 1 天后过期」—— 300 里有 200 是后天到期的，假话（issue #44 的原始 bug）；
 * - 「300 积分 2 天内过期」—— 最晚那批是 2 天，但用户想知道的是「最早哪天要动手」，
 *   徽章被拉长成「还有 2 天」，会让人误以为不急。
 *
 * 窗口内的**总量**仍是真实有用的数（抽屉副文本「7 天内到期 X 积分」、聚合页 / 统计页
 * 的 `expiringSoonRemaining` 都读它），所以保留在 `total` 字段，只是**不进徽章**。
 *
 * ## 时间单位：≤ 48 小时用「小时」
 *
 * 徽章的时间单位由最快那批的剩余时长决定：**≤ 48 小时 ⇒ 小时**（`最近到期 47小时`），
 * 超过 48 小时才折算成**天**（`最近到期 3天`）。用小时是为了不让「25 小时」和「47 小时」
 * 都被说成「1 天」；天档向上取整，宁可说晚不说早。
 *
 * ## 明细放 tooltip，用绝对时刻
 *
 * `batches` 按到期时刻**升序**（最快的那批排第一），每行渲染 `YYYY-MM-DD HH:mm · 金额`。
 * 不用相对倒计时：这一行既要回答「哪天」，又要能直接和账号页 / 抽屉里的日期对照。
 *
 * ## 窗口由**后端**决定，前端不再自设
 *
 * 是否临期只认后端下发的 `expiringSoon` 标记（`credits.rs::EXPIRING_SOON_DAYS` = 7 天、
 * `trae/credits.rs::EXPIRING_SOON_SECS` = **7 天**）。此前 WorkBuddy 卡片在前端另设 3 天窗口，
 * 与统计页 / 总览页（都读后端的 `expiringSoonRemaining`）口径不一致 —— 同一件事两个数字。
 *
 * ⚠️ 不可把 `expiringSoon`（布尔）当额度求和，也不可在缺该字段时自行推窗口：
 * 缺字段一律当「未知」跳过，宁可少一枚徽章，不可编一个到期日。
 *
 * ⚠️ 拿不到后端标记的**少数**地方（积分页的到期分桶、账号抽屉的积分指标块）用
 * {@link TRAE_EXPIRING_SOON_MS} —— 它是后端窗口的**镜像**，不是第二个口径。
 */

/** 一小时的毫秒数（徽章短区间用「小时」）。 */
const HOUR_MS = 3_600_000;
/** 一天的毫秒数。 */
const DAY_MS = 86_400_000;
/** 徽章在这个区间内用「小时」，超过才用「天」：≤ 48 小时。 */
const HOUR_DISPLAY_LIMIT_MS = 48 * HOUR_MS;

/**
 * Trae 的临期窗口，**镜像后端** `trae/credits.rs::EXPIRING_SOON_SECS`（**7 天**）。
 *
 * 只给「拿不到后端 `expiringSoon` 标记」的地方用（积分页到期分桶、账号抽屉的积分指标块）。
 * 有标记时必须只认标记 —— 前端写死窗口正是 #144 复核 §3.3 的残差：
 * 10 天后到期的包在卡片上说临期、点开抽屉却不说。
 *
 * ★ 2026-11-09：从 14 天统一到 **7 天**（与 WorkBuddy 的 `credits.rs::EXPIRING_SOON_DAYS`
 * 同值）。此前三个平台两个数：同一批「10 天后到期」的积分，WB 不标黄、
 * Trae / Qoder 标黄，用户在分区之间来回看会觉得其中一个在说谎。
 *
 * ⚠️ 改后端那个常量时这里要跟着改（两个数刻意分开写，不跨语言共享，但**必须同值**）。
 */
export const TRAE_EXPIRING_SOON_MS = 7 * DAY_MS;

/** 一批到期：同一时刻到期的剩余积分之和。 */
export interface ExpiringBatch {
  /** 到期时刻（epoch 毫秒）。 */
  expireAt: number;
  /** 该批此刻的剩余积分之和。 */
  amount: number;
}

export interface ExpiringSummary {
  /** 窗口内全部临期剩余积分之和（抽屉副文本 / 聚合页读它；**不进徽章**）。 */
  total: number;
  /**
   * 徽章的**单位**：最快那批剩余 ≤ 48 小时 ⇒ `"hours"`，否则 `"days"`。
   * 短区间用小时是因为「1 天」会把 25 小时和 47 小时说成同一个数。
   */
  soonestUnit: "hours" | "days";
  /** 最快那批距今天数，向上取整、≥1 —— 仅 `soonestUnit === "days"` 时使用。 */
  soonestDays: number;
  /** 最快那批距今小时数，向上取整、≥1 —— 仅 `soonestUnit === "hours"` 时使用。 */
  soonestHours: number;
  /** 最快那批（`batches[0]`）的剩余积分 —— 徽章的金额，与上面的数值同批。 */
  soonestAmount: number;
  /** 按到期时刻升序的逐批明细（最快的那批排第一）。 */
  batches: ExpiringBatch[];
}

/** 汇总入参：WorkBuddy 的 `CreditResource` 与 Trae 的 `TraeCreditPackage` 都满足它。 */
export interface ExpiringItem {
  remaining: number;
  expireAt: number | null;
  /** 后端标记：已到期。 */
  expired?: boolean;
  /**
   * 后端标记：临期（窗口见后端 `EXPIRING_SOON_SECS`，**7 天**）。
   * `undefined` 表示数据源没这个字段 ⇒ 视为未知、跳过。
   */
  expiringSoon?: boolean;
}

/**
 * 汇总临期积分。没有可渲染的临期批次时返回 `null`（调用方据此**整条不渲染**，
 * 绝不写一个猜出来的「N 天」标签）。
 *
 * @param items 积分包 / 积分资源列表（可为 `null`）
 * @param options.now 判定基准时刻（毫秒），默认 `Date.now()`
 *   —— 一次渲染里多处汇总应传同一个值，否则同一张卡会出现两个「今天」。
 * @param options.unit 入参 `expireAt` 的单位。**两个平台不同，必须显式给**：
 *   WorkBuddy 的 `CreditResource.expireAt` 是**毫秒**（后端 `parse_timestamp_ms`），
 *   Trae 的 `TraeCreditPackage.expireAt` 是**秒**（后端直接透传上游 `expire_time`）。
 */
export function summarizeExpiring(
  items: readonly ExpiringItem[] | null | undefined,
  options: { now?: number; unit?: "ms" | "s" } = {},
): ExpiringSummary | null {
  if (!items || items.length === 0) return null;
  const now = options.now ?? Date.now();
  const toMs = options.unit === "s" ? (value: number) => value * 1000 : (value: number) => value;

  /* 同一到期时刻合并成一批：上游可能把一次发放拆成多个包，逐包列会让 tooltip 变成流水账。 */
  const merged = new Map<number, number>();
  for (const item of items) {
    if (item.expired) continue;
    // `expiringSoon` 必须显式为 `true`：缺字段 = 未知，不当「临期」也不当「不临期」去推测窗口。
    if (item.expiringSoon !== true) continue;
    if (!Number.isFinite(item.remaining) || item.remaining <= 0) continue;
    if (typeof item.expireAt !== "number" || !Number.isFinite(item.expireAt)) continue;
    const expireAt = toMs(item.expireAt);
    if (!Number.isFinite(expireAt) || expireAt <= now) continue;
    merged.set(expireAt, (merged.get(expireAt) ?? 0) + item.remaining);
  }
  if (merged.size === 0) return null;

  const batches = [...merged.entries()]
    .map(([expireAt, amount]) => ({ expireAt, amount }))
    .sort((left, right) => left.expireAt - right.expireAt);
  const total = batches.reduce((sum, batch) => sum + batch.amount, 0);
  if (total <= 0) return null;

  /* 徽章只看最快那批：单位、数值、金额必须来自同一批（见模块头）。
     单位（小时 / 天）与数值一律取自 {@link remainingSpan}，不在本函数里再算一遍 ——
     账号页表格的「积分到期」列也读它，两处判据分叉就会回到
     「徽章说 2 小时、表格说 1 天」的老样子。 */
  const soonest = batches[0];
  const remaining = soonest.expireAt - now;
  return {
    total,
    soonestUnit: remainingSpan(remaining).unit,
    soonestDays: Math.max(1, Math.ceil(remaining / DAY_MS)),
    soonestHours: Math.max(1, Math.ceil(remaining / HOUR_MS)),
    soonestAmount: soonest.amount,
    batches,
  };
}

/**
 * 一段**剩余时长**的展示粒度 —— 全仓唯一出口。
 *
 * - `≤ 48 小时` ⇒ 小时（向上取整、≥1）：25 小时与 47 小时都能各自说准；
 * - `> 48 小时` ⇒ 天（向上取整、≥1）：宁可说晚不说早 —— 49 小时算「3 天」。
 *
 * ## 谁在读它
 *
 * - {@link summarizeExpiring}（卡片徽章的 `soonestUnit`）；
 * - WB 账号页表格的「积分到期」列（`AccountsPage`）—— 同一批积分在徽章与表格里
 *   **必须同一个粒度**。此前表格直接 `daysUntil()` 只到天 ⇒ 2 小时后到期的批
 *   在徽章上是「2小时」、在表格上是「1 天」，同一屏两句话。
 *
 * ⚠️ 别在调用点重写 `<= 48` 这个边界：那正是这次要根除的「同一规则两处实现」。
 */
export function remainingSpan(remainingMs: number): {
  unit: "hours" | "days";
  value: number;
} {
  if (remainingMs <= HOUR_DISPLAY_LIMIT_MS) {
    return { unit: "hours", value: Math.max(1, Math.ceil(remainingMs / HOUR_MS)) };
  }
  return { unit: "days", value: Math.max(1, Math.ceil(remainingMs / DAY_MS)) };
}

/**
 * 「7 天内到期 X 积分」副文本的**金额闸门**：只有 `> 0` 才给出数字。
 *
 * ## 为什么 0 也要挡掉
 *
 * 抽屉 / 聚合详情里这行是一句**警示**（「7 天内到期 X 积分」），不是读数。
 * 0 是绝大多数账号的常态 —— 每张卡点开都写「7 天内到期 0 积分」，等于把「一切正常」
 * 说成一条要看的提醒；看久了这一行就被当成背景噪音（与「红徽章天天亮 = 没人看」
 * 是同一条道理）。
 *
 * ## 与「未知 ≠ 0」是**两条独立**的纪律，别混
 *
 * - 读不到（`null` / `undefined` / 非数）⇒ 不写 —— 绝不拿 0 冒充读数；
 * - 读到了且确实是 0 ⇒ 也不写 —— 「没有要过期的」是常态，不是一条信息。
 *
 * 两种输入在同一个出口返回 `null`，调用方只需判 `== null` 决定「这一行渲不渲」，
 * 不必各自记住两个分支（`#189` 曾让五个调用点各写一次，写法还不一样）。
 *
 * @param amount 后端 `expiringSoonRemaining` / `summarizeExpiring().total` / 逐包汇总
 * @returns `> 0` 的原值；其余（含 0、负数、NaN、缺失）一律 `null`
 */
export function expiringSubtextAmount(amount: number | null | undefined): number | null {
  return typeof amount === "number" && Number.isFinite(amount) && amount > 0 ? amount : null;
}

/* ══════════════════════ 「最近到期」（不限窗口）══════════════════════
 *
 * ## 为什么要有这一段（issue #69）
 *
 * `summarizeExpiring` 只看**后端临期窗口**（7 天）内的批次 —— 那是对的，因为「N 天内过期」
 * 是一句**警告**（「再不用就浪费了」）。但用户真正要回答的是另一个问题：
 * **「我好几个账号，先用哪个？」** —— 这个问题需要「**每一笔未来到期**」的相对顺序，
 * 而不是「7 天内的那批」。
 *
 * 15 天后到期、30 天后到期的积分，在 `summarizeExpiring` 里**根本不可见**
 * （`expiringSoon !== true` 直接被跳过）⇒ 用户说「两周内到期的看不到」就是这个。
 *
 * ## 三条与 `summarizeExpiring` **刻意不同**的地方
 *
 * ① **不读 `expiringSoon`**：那个布尔由后端的 7 天窗口决定，**不能**表达「30 天后」。
 * ② **不读 `credit.soonestExpireAt`**：它把**已过期**资源也算进最小值 ⇒ 拿它排序会
 *    把「已过期」排到最前（那是「已经浪费了」，不该抢占「先消耗」的位）。
 * ③ **不设天数上限**：「最近一批」是相对顺序，不是一个窗口。
 *
 * ⚠️ 仍然遵守「未知不推测」：没有 `expireAt` / `remaining <= 0` / 已过期的批次一律跳过。
 */

/** 一批「未来到期」：同一时刻到期的剩余积分之和。 */
interface UpcomingBatch {
  /** 到期时刻（epoch 毫秒）。 */
  expireAt: number;
  /** 该批此刻的剩余积分之和。 */
  amount: number;
}

/**
 * 所有**未来**到期批次：过滤「已过期 / 剩余 ≤ 0 / 无到期时刻 / 已到时刻」，
 * 按到期升序，**同一时刻合并**。
 *
 * ⚠️ **不导出**：它只被 {@link nearestExpiringBatch} 用来取第一个。此前是 `export`，
 *   但删除中性徽章后外部零调用（唯一的外部消费方是已删的 `qoderUpcomingExpiries`）——
 *   留着导出会让下一个人以为「有人要全部批次」而去找那个不存在的调用方。
 *
 * @param options.now 判定基准时刻（毫秒），默认 `Date.now()`
 * @param options.unit 入参 `expireAt` 的单位（WB 毫秒 / Trae 秒）—— 同 `summarizeExpiring`。
 */
function upcomingExpiries(
  items: readonly ExpiringItem[] | null | undefined,
  options: { now?: number; unit?: "ms" | "s" } = {},
): UpcomingBatch[] {
  if (!items || items.length === 0) return [];
  const now = options.now ?? Date.now();
  const toMs = options.unit === "s" ? (value: number) => value * 1000 : (value: number) => value;

  const merged = new Map<number, number>();
  for (const item of items) {
    if (item.expired) continue;
    if (!Number.isFinite(item.remaining) || item.remaining <= 0) continue;
    if (typeof item.expireAt !== "number" || !Number.isFinite(item.expireAt)) continue;
    const expireAt = toMs(item.expireAt);
    if (!Number.isFinite(expireAt) || expireAt <= now) continue;
    merged.set(expireAt, (merged.get(expireAt) ?? 0) + item.remaining);
  }

  return [...merged.entries()]
    .map(([expireAt, amount]) => ({ expireAt, amount }))
    .sort((left, right) => left.expireAt - right.expireAt);
}

/** 最近一批未来到期；没有未来到期返回 `null`（调用方据此**整条不渲染**）。 */
export function nearestExpiringBatch(
  items: readonly ExpiringItem[] | null | undefined,
  options: { now?: number; unit?: "ms" | "s" } = {},
): UpcomingBatch | null {
  return upcomingExpiries(items, options)[0] ?? null;
}

/* 曾有一个 `daysUntil(expireAt)` 只返回「向上取整的天数」，WB 表格的「积分到期」列用它 ——
   2 小时后到期的批被说成「1 天」，与本模块 {@link remainingSpan} 的「≤48 小时用小时」
   直接矛盾。2026-11-09 已删：需要天数就 `remainingSpan(剩余毫秒).value`，
   判据只有一处，别再长出第二个只到天的函数。 */

/* ══════════════════════ 渲染约束 ══════════════════════ */

/**
 * 逐批明细 tooltip 的高度上限（issue #79）。
 *
 * ## 为什么需要它
 *
 * 明细是**一行一批**：积分包多的时候能有四五十批（实测用户 41 批），
 * 一行 20px 就是 800px+，一定比可视空间高。
 *
 * 两层限制缺一不可：
 *
 * ① `var(--radix-popper-available-height)` —— Radix popper 算出的**当前方向可用高度**（已扣
 *    `collisionPadding`）。`TooltipContent` 基础样式已带同一条，这里再写是为了让 `min()` 能取到它；
 * ② `56vh` —— 硬上限。只用 ① 的话，徽章恰好在下半屏时浮层会长到半屏高，
 *    把用户正要看的账号卡整个盖住。hover 是为了「看这张卡」，不是换一屏。
 *
 * ⚠️ 定值不要外提到 CSS 变量里：它只服务「逐批明细」这一种长列表，
 * 短 tooltip 加上限只会让内容无谓地出现滚动条。
 */
export const EXPIRY_TIP_MAX_HEIGHT = "min(56vh, var(--radix-popper-available-height))";

