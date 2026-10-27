/**
 * 聚合账号面板的**纯函数层**（issue #50）。
 *
 * ## 这一层只做三件事，不碰 React
 *
 * ① 把 6 个作用域的账号归一成一种形状（`AggregateAccount`）；
 * ② 账号键制 `${scope}:${key}` 的拼装与解析（跨作用域**唯一**的身份表示）；
 * ③ 筛选 / 排序 / 分组的纯计算。
 *
 * 之所以刻意不 import React：前端没有单测框架，纯函数才能被
 * `scripts/check-scope-registry.cjs` 直接读源码 + 用 node 跑断言（同
 * `update-preference.ts` / `credit-expiry.ts` 的既有做法）。
 *
 * ## 为什么必须带 scope 前缀（最容易写错的一处）
 *
 * 三个平台的账号主键各自独立，而且**同一个 `userId` 在国内 / 国际是两个不同账号**
 * （`trae-cn` 与 `trae-global` 各有一份 `jackey`）。聚合页把 6 个作用域铺在一屏，
 * 裸主键必然串号 —— 勾选一张卡会连带选中另一个作用域的号，刷新转圈会互相吞掉。
 * 所以：**凡是指向「某个账号」的地方，一律用 `ref`，不用裸主键。**
 *
 * ## 「需处理」的口径按平台分开写
 *
 * 三个平台的凭据形态不同（WB 是 `expiresAt` + `needsRelogin`；Trae 是 `jwtStatus` +
 * `jwtExpTimestamp`；Qoder 是 `hasCredential` + `expiresAt`），拿一个字段硬套三处
 * 必然有一处是错的。这里逐平台判，宁可多写十行。
 */

import type { AccountMeta } from "@/lib/types";
import type { QoderAccountView } from "@/lib/api";
import type { TraeAccount, TraeJwtStatus } from "@/lib/trae-types";
import { cooldownNeedsAttention } from "@/lib/trae-cooldown";
import { SCOPES, scopeParts, type Scope } from "@/lib/scope";
import type { PlatformSlug } from "@/lib/platforms/registry";

/* ══════════════════════ 形状 ══════════════════════ */

/** 三个平台在聚合页里的统一形状。`data` 保留各自的原始类型（卡片按平台分派时要用原字段）。 */
export type AggregateAccount =
  | { ref: string; scope: Scope; platform: "workbuddy"; data: AccountMeta }
  | { ref: string; scope: Scope; platform: "trae"; data: TraeAccount }
  | { ref: string; scope: Scope; platform: "qoder"; data: QoderAccountView };

/** 分组视图的一组（`scope` 一定是有账号的作用域）。 */
export interface AggregateGroup {
  scope: Scope;
  items: AggregateAccount[];
}

/* ══════════════════════ 键制 ══════════════════════ */

/** 拼一个账号键。`key` 按平台取：WB `id` / Trae `userId` / Qoder `uid`。 */
export function aggregateRef(scope: Scope, key: string): string {
  return `${scope}:${key}`;
}

/**
 * 拆账号键。格式不符（不含 `:`、scope 不在 `SCOPES` 里）⇒ `null`。
 *
 * ⚠️ **fail-closed**：宁可丢掉一个键，也不要让一个来路不明的字符串冒充作用域
 * （`localStorage` 是用户可见可改的文件，这里必须当成不可信输入）。
 */
export function parseAggregateRef(ref: string): { scope: Scope; key: string } | null {
  const i = ref.indexOf(":");
  if (i <= 0) return null;
  const scope = ref.slice(0, i) as Scope;
  const key = ref.slice(i + 1);
  if (!key) return null;
  if (!(SCOPES as readonly string[]).includes(scope)) return null;
  return { scope, key };
}

/** 该平台账号在键里用的主键。 */
export function aggregateAccountKey(account: AggregateAccount): string {
  switch (account.platform) {
    case "workbuddy":
      return account.data.id;
    case "trae":
      return account.data.userId;
    case "qoder":
      return account.data.uid;
  }
}

/** 由 scope + 平台 + 原始数据造一条 `AggregateAccount`（取数层用）。 */
export function makeAggregateAccount(
  scope: Scope,
  platform: "workbuddy" | "trae" | "qoder",
  data: AccountMeta | TraeAccount | QoderAccountView,
): AggregateAccount {
  const key =
    platform === "workbuddy"
      ? (data as AccountMeta).id
      : platform === "trae"
        ? (data as TraeAccount).userId
        : (data as QoderAccountView).uid;
  return { ref: aggregateRef(scope, key), scope, platform, data } as AggregateAccount;
}

/* ══════════════════════ 展示 ══════════════════════ */

/**
 * 展示名。三个平台的「名字」字段不同名（`nickname` / `name` / `name`），
 * 且都可能是 `null` ⇒ 统一回落到主键，绝不给空串（空串会让卡片标题变一条空白）。
 *
 * ⚠️ 这里**不做 HTML 转义**：返回的是纯文本，交给 React 渲染；
 * 后端返回值仍需过 `displayText`（见 `display-text.ts` 的 issue #2）。
 */
export function aggregateDisplayName(account: AggregateAccount): string {
  const raw =
    account.platform === "workbuddy"
      ? (account.data.nickname ?? account.data.uid ?? account.data.id)
      : account.platform === "trae"
        ? account.data.name
        : (account.data.nickname ?? account.data.name ?? account.data.uid);
  const text = typeof raw === "string" ? raw.trim() : "";
  return text || aggregateAccountKey(account);
}

/** 副标题（邮箱 / 域名）。没有就回落到备注，再没有就是空串（调用方决定要不要渲染）。 */
export function aggregateSubtitle(account: AggregateAccount): string {
  if (account.platform === "workbuddy") {
    return (account.data.email ?? "").trim() || (account.data.remark ?? "").trim();
  }
  if (account.platform === "qoder") {
    return (account.data.domain ?? "").trim() || (account.data.remark ?? "").trim();
  }
  return (account.data.remark ?? "").trim();
}

/** 搜索用的文本（昵称 / 邮箱 / 备注 / 分组 / 作用域名都由调用方拼接）。 */
export function aggregateSearchText(account: AggregateAccount): string {
  const parts = [aggregateDisplayName(account), aggregateSubtitle(account)];
  if (account.platform === "workbuddy") {
    parts.push(account.data.groupName ?? "", account.data.enterpriseName ?? "");
  } else if (account.platform === "trae") {
    parts.push(account.data.groupId ?? "");
  } else {
    parts.push(account.data.group ?? "");
  }
  return parts.join(" ").toLowerCase();
}

/* ══════════════════════ 「需处理」与紧迫度 ══════════════════════ */

/** 凭据是否已过期（`expiresAt` 是 epoch 毫秒；`null` = 没有到期概念 ⇒ 不算过期）。 */
function expiredAt(expiresAt: number | null | undefined, now: number): boolean {
  return typeof expiresAt === "number" && expiresAt > 0 && expiresAt < now;
}

/**
 * 该账号是否需要人处理。
 *
 * ⚠️ 与侧栏徽章的 `attentionCount`（只数凭据问题）**不是同一个口径**：
 * 那个说的是「凭据待处理」，这里说的是「今天有事要做」。两者不能互相冒充。
 */
export function aggregateNeedsAttention(account: AggregateAccount, now: number = Date.now()): boolean {
  if (account.platform === "workbuddy") {
    const a = account.data;
    return Boolean(a.needsRelogin) || expiredAt(a.expiresAt, now);
  }
  if (account.platform === "trae") {
    const a = account.data;
    const dead: TraeJwtStatus[] = ["expired"];
    if (dead.includes(a.jwtStatus)) return true;
    // ★ issue #108 §P1-A：改造前聚合页**完全不看** `cooldownType`，于是同一个账号
    //   在渠道页显示「需重新登录」、在这里显示「一切正常」。会话作废是需要人动手的，
    //   必须进「待处理」；其余冷却会自己到期，算进来会让这个数字永不为 0。
    if (cooldownNeedsAttention(a.cooldownType)) return true;
    return expiredAt(a.jwtExpTimestamp, now);
  }
  const a = account.data;
  if (!a.hasCredential) return true;
  return expiredAt(a.expiresAt, now);
}

/**
 * 紧迫度（**平铺视图的排序依据**）。
 *
 * 平铺如果只是「打散分组、顺序仍按作用域」，它没有任何新信息 —— 那就不该存在。
 * 换依据之后两个视图各有明确用途：
 *   · 分组 = 找某个渠道的号、组内操作；
 *   · 平铺 = 扫全场的待处理项（过期 → 需重登 → 未签到 → 临期 → 其余）。
 */
export function aggregateUrgency(account: AggregateAccount, now: number = Date.now()): number {
  if (account.platform === "workbuddy") {
    const a = account.data;
    if (expiredAt(a.expiresAt, now)) return 0;
    if (a.needsRelogin) return 1;
    return 4;
  }
  if (account.platform === "trae") {
    const a = account.data;
    if (a.jwtStatus === "expired") return 0;
    if (a.jwtStatus === "warn") return 2;
    if (!a.checkedToday) return 2;
    return 4;
  }
  const a = account.data;
  if (!a.hasCredential) return 0;
  if (expiredAt(a.expiresAt, now)) return 0;
  return 4;
}

/* ══════════════════════ 可见性筛选 ══════════════════════ */

/**
 * 把持久化读回来的键数组**修剪**成合法可见集。
 *
 * 三件事一次做完（顺序不能换）：
 *   ① 丢掉失效键 —— 账号被删 / 作用域失效 / 格式不符。不做就会存下一堆幽灵键；
 *   ② **规范化「选满」** —— 与全集相等 ⇒ `null`（等于没筛）。
 *      否则会出现「存了 32 个键，新增第 33 个账号之后看不见」的诡异态；
 *   ③ 空集也 ⇒ `null`。空集在语义上是「什么都不想看」，但那不是一个值得
 *      持久化的偏好（下次打开看到全量比看到空屏更合理）。
 *
 * @param raw      读盘得到的原始值（可能是 `null` / 脏数组）
 * @param universe 当前真实存在的全部账号键
 */
export function pruneVisibleKeys(
  raw: readonly string[] | null | undefined,
  universe: readonly string[],
): Set<string> | null {
  if (!raw || raw.length === 0) return null;
  const live = new Set(universe);
  const kept = new Set<string>();
  for (const key of raw) {
    if (typeof key === "string" && live.has(key)) kept.add(key);
  }
  if (kept.size === 0) return null;
  if (kept.size === live.size) return null;
  return kept;
}

/** 写盘前把可见集**规范化**成「要存的数组或 `null`」（与 {@link pruneVisibleKeys} 对称）。 */
export function visibleKeysToStore(
  visible: ReadonlySet<string> | null,
  universe: readonly string[],
): string[] | null {
  if (!visible || visible.size === 0) return null;
  if (visible.size >= universe.length) return null;
  return [...visible].sort();
}

/** 可见性判定（`null` = 全量可见）。 */
export function isVisible(visible: ReadonlySet<string> | null, ref: string): boolean {
  return visible === null || visible.has(ref);
}

/**
 * 「筛选生效期间新出现的账号」（issue #50 的 Q3 方案 C）。
 *
 * ## 为什么需要它
 *
 * 可见性是**账号键的集合**，所以筛选生效期间**新入库的账号不在集合里** ⇒ 默认不可见。
 * 用户会以为「这个账号没被识别出来」。给一条明确的提示 + 一键加入，比让用户自己猜好。
 *
 * ## 判定口径
 *
 * 只看**当前可见范围之外、但确实存在**的账号。
 * ⚠️ 「新出现」是**相对筛选偏好**而言，不是相对时间 —— 用户上周设的筛选，
 * 今天加的账号同样算「新出现」，同样该提示。
 */
export function newcomerAccounts(
  all: readonly AggregateAccount[],
  visible: ReadonlySet<string> | null,
): AggregateAccount[] {
  if (visible === null) return [];
  return all.filter((account) => !visible.has(account.ref));
}

/* ══════════════════════ 组合计算 ══════════════════════ */

/**
 * 叠加「可见性筛选」与「搜索词」。
 *
 * ⚠️ 只**决定这次渲染哪些**，不改变账号集合本身，也不写后端。
 * 这两个条件都作用在**同一份数据**上，因此不存在「两套筛选状态谁为准」的歧义。
 */
export function filterAggregateAccounts(
  all: readonly AggregateAccount[],
  visible: ReadonlySet<string> | null,
  query: string,
): AggregateAccount[] {
  const q = query.trim().toLowerCase();
  return all.filter((account) => {
    if (!isVisible(visible, account.ref)) return false;
    if (!q) return true;
    return aggregateSearchText(account).includes(q);
  });
}

/**
 * 按作用域分组。**空组不返回** —— 调用方另外处理「未安装 / 没有账号」的说明行。
 *
 * 组序固定为 `SCOPES` 的声明顺序（平台顺序 → 区域顺序），因此刷新后卡片不会跳位。
 */
export function groupAggregateAccounts(all: readonly AggregateAccount[]): AggregateGroup[] {
  const byScope = new Map<Scope, AggregateAccount[]>();
  for (const account of all) {
    const bucket = byScope.get(account.scope);
    if (bucket) bucket.push(account);
    else byScope.set(account.scope, [account]);
  }
  return SCOPES.filter((scope) => byScope.has(scope)).map((scope) => ({
    scope,
    items: byScope.get(scope) ?? [],
  }));
}

/** 按紧迫度混排（平铺视图）。同紧迫度时按作用域声明序，保证顺序确定。 */
export function sortByUrgency(
  all: readonly AggregateAccount[],
  now: number = Date.now(),
): AggregateAccount[] {
  const scopeOrder = new Map<string, number>(SCOPES.map((scope, i) => [scope, i]));
  return [...all].sort((x, y) => {
    const u = aggregateUrgency(x, now) - aggregateUrgency(y, now);
    if (u !== 0) return u;
    return (scopeOrder.get(x.scope) ?? 99) - (scopeOrder.get(y.scope) ?? 99);
  });
}

/** 汇总数字（顶栏副标题 / 空态文案用）。`attention` 的口径见 {@link aggregateNeedsAttention}。 */
export function aggregateTotals(
  all: readonly AggregateAccount[],
  now: number = Date.now(),
): { total: number; attention: number } {
  let attention = 0;
  for (const account of all) if (aggregateNeedsAttention(account, now)) attention += 1;
  return { total: all.length, attention };
}

/* ══════════════════════ 取数计划 ══════════════════════ */

/**
 * 每个作用域该用哪个平台的哪条命令取数（`use-aggregate-accounts.ts` 照着跑）。
 *
 * 单独抽出来是为了让「6 个作用域 ⇄ 3 条命令」的映射**只有一处**：
 * 取数层、失败重试、演示数据三处都读它，不会出现「重试时用了错的命令」。
 */
export type AggregateSource =
  | { scope: Scope; platform: "workbuddy"; region: "cn" | "global" }
  | { scope: Scope; platform: "trae"; variant: "trae_merged" | "global_trae_merged" }
  | { scope: Scope; platform: "qoder" };

/** Trae 作用域 → 账号库变体 id（与 `use-scope-status.ts` 的两条快照同源）。 */
const TRAE_VARIANT: Record<string, "trae_merged" | "global_trae_merged"> = {
  "trae-cn": "trae_merged",
  "trae-global": "global_trae_merged",
};

export const AGGREGATE_SOURCES: readonly AggregateSource[] = SCOPES.map((scope) => {
  const { product, region } = scopeParts(scope);
  if (product === "workbuddy") {
    return { scope, platform: "workbuddy", region: region as "cn" | "global" };
  }
  if (product === "trae") {
    return { scope, platform: "trae", variant: TRAE_VARIANT[scope] ?? "trae_merged" };
  }
  return { scope, platform: "qoder" };
});

/** 平台短标识（用于 `platformOfScope` 这类判断，避免各处再 `scopeParts` 一遍）。 */
export function platformOfScope(scope: Scope): PlatformSlug {
  return scopeParts(scope).product === "workbuddy"
    ? "wb"
    : scopeParts(scope).product === "trae"
      ? "trae"
      : "qoder";
}
