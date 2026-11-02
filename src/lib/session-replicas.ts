/**
 * 会话副本归一（P8 T02）—— 把后端 `list_session_replicas` 的**原始副本组**
 * 与账号显示名 / 会话行 join 成**可渲染的视图**。
 *
 * ## 数据来源与「诚实标注」
 *
 * 后端 `list_replica_groups_for` 只回 ledger 里**确有的**字段（`accountUid` / `sessionId`
 * / `region`）。标题 / `updatedAt` / `hasHistory` **不在 ledger 里** ⇒ 由这里 join 补齐：
 * - 账号显示名来自**账号 store**（`AccountMeta[]`）—— **不重造第二份账号清单**（§1.5 / R9）；
 * - 会话行来自 `list_sessions`（**当前账号**的会话）—— 因此**只有当前账号的副本**能拿到
 *   标题；其它账号的副本落 `titleFallback`（诚实，见下）。
 *
 * ⚠️ **ledger 只覆盖「经本应用复制过」的会话** ⇒ `SessionGroupView.partial` **恒 `true`**
 * （类型锁死 + 视图据此渲染固定文案），**禁止**「全部副本」口径（§3.5）。
 *
 * ## 内容状态（`status` / `commonPrefix` / `deltas` / `sinceBaseline`）
 *
 * ★ **判据在 core**（`session::derive_content_status`），本模块只做**搬运 + 脏值兜底**。
 * 搬到后端的理由是可验证的：前端要自己算，就得把**每个副本**的前 512 行逐行 hash 拿过来
 * （真机 25,487 个副本 ≈ 200 MB JSON / 1300 万个整数），再在 JS 里逐对比较 —— 会话页
 * 因此**永远转圈**。而结论本身只有每组几个整数。
 *
 * ⚠️ **不许用 `updatedAt` 冒充「落后 / 最新」**：`updatedAt` 只能表达「谁最近被改过」，
 * 不能表达「谁落后 N 条」（§3.15.2「时间新 ≠ 内容新」）。本模块**只把 `updatedAt` 当作事实
 * 透传**，**不派生任何状态**。
 *
 * ⚠️ **展示字段出口归一为「字符串或 null」**：会话标题 / 账号显示名一律过
 * {@link displayText}（对象当 React 子节点 ⇒ 白屏，issue #2）。
 */

import { displayText } from "@/lib/display-text";
import { t } from "@/lib/i18n";
import type {
  AccountMeta,
  Session,
  SessionReplicaContent,
  SessionReplicaGroup,
  SyncSessionReplicaItem,
  UnifySessionReplicaItem,
} from "@/lib/types";

/** 一行副本（已 join 账号显示名 + 会话行，标题已过 `displayText`）。 */
export interface SessionReplicaRow {
  /** 副本所属账号 uid（ledger 的 source_uid / target_uid）。 */
  accountUid: string;
  /** 账号显示名；**uid 不在账号库 ⇒ `uid 前缀 + 「未知账号」`**（✅ #3，**绝不**留空）。 */
  accountLabel: string;
  /** 该账号里的会话 id。 */
  sessionId: string;
  /** ★ 已过 `displayText`（白屏防线，issue #2）；取不到会话行时为兜底文案。 */
  title: string;
  /** 来自 `list_sessions` 会话行；取不到为 `null`（**不臆造**）。 */
  updatedAt: number | null;
  /** 来自 `list_sessions` 会话行；取不到为 `false`。 */
  hasHistory: boolean;
}

/**
 * 组的**内容级**状态（P8）—— **由 fingerprint 的公共前缀派生**。
 *
 * ⚠️ **不是**由 `updatedAt` 派生：`updatedAt` 只能表达「谁最近被改过」，
 * 不能表达「谁落后 N 条」（`P8-ARCH.md` §1.2「时间新 ≠ 内容新」）。
 * 用时间冒充内容是**撒谎**（§3.10 第 1 条「图不能撒谎」）。
 *
 * - `latest`：所有副本**行数相同且内容一致**（公共前缀 = 行数）⇒ 已一致；
 * - `behind`：行数不等，但**公共前缀覆盖到最短副本** ⇒ 只是「有人多写了」，**无分叉**；
 * - `diverge`：**公共前缀 < 最短副本行数** ⇒ 双方在共同旧版之后**各自改过** ⇒ 有分歧；
 * - `unknown`：**有 fingerprint 缺失**（jsonl 读不到）⇒ **诚实标注，不臆造**。
 */
export type SessionGroupStatus = "latest" | "behind" | "diverge" | "unknown";

/** 一组副本的可渲染视图。 */
export interface SessionGroupView {
  /** 稳定组键（= 后端给的 `key`；用于 React key）。 */
  key: string;
  rows: SessionReplicaRow[];
  /** ★ 恒 `true`：仅覆盖「经本应用复制过」的会话（诚实标注，§3.5）。 */
  partial: true;
  /** ★ 内容级状态（P8）—— fingerprint 缺失时为 `"unknown"`。 */
  status: SessionGroupStatus;
  /** 共同旧版的**条数**（所有副本两两公共前缀的**最小值**）；`unknown` 时为 `null`。 */
  commonPrefix: number | null;
  /**
   * ★ issue #141：`commonPrefix` 是**精确值**（`true`）还是**下界**（`false`）。
   *
   * 逐行 hash 只覆盖前 512 行 ⇒ 长会话在窗口之外分叉时，后端只能保证「至少 512 行相同」。
   * `false` ⇒ 界面说「至少 N 条」，**不许**说「N 条」（拿窗口上限冒充真值）。
   */
  commonPrefixExact: boolean;
  /** 各组员相对共同旧版的**新增条数**（`sessionId → delta`）；`unknown` 时为空。 */
  deltas: Map<string, number>;
  /** 行数最多的副本的条数（`latestLineCount`）；`unknown` 时为 `null`。 */
  latestLineCount: number | null;
  /**
   * ★ P14 S1 · **乐观令牌**（后端给的组结构指纹）。
   *
   * 提交时原样带回，后端复核「组还是不是你看到的那个组」。
   * ⚠️ `null` ⇒ **不给同步 / 统一的出口**（见 [`buildSyncItems`]）：宁可不让做，
   * 也不做一次**对着旧组**算出来的写操作。取不到只可能是「后端与本前端版本不匹配」。
   */
  previewToken: string | null;
  /**
   * ★ P14 S2：本组**最近一次一致点**（取 `createdAt` 最大的那条基线）。
   *
   * ⚠️ 与 `commonPrefix`（共同旧版）**不是一回事**：那个是**当前**内容的公共前缀，
   * 这个是**记录过的**历史事实。两者不同 ⇒ 有人改写过更早的内容。
   * 无基线 ⇒ `null`（**不臆造**一个一致点）。
   */
  baseline: SessionBaselineView | null;
  /**
   * ★ P14 S2：各副本自「上次一致点」以来的新增条数（`sessionId → delta`）。
   *
   * ⚠️ **值为 `null` = 前缀未被验证**（该副本自基线之后改写过更早的内容）——
   * 此时「新增了几条」这个问题**本身不成立**，不是 0、也不是「未知」。
   * **不在表里** = 该副本与这条基线无关（基线是**成对**的，不是全组一个）。
   */
  sinceBaseline: Map<string, number | null>;
}

/** [`SessionGroupView.baseline`] 的展示形态（只带界面要用的两个字段）。 */
export interface SessionBaselineView {
  createdAt: number;
  lineCount: number;
}

/** 一组「内容结论」的**诚实空值**（缺字段 / 脏值 / 旧后端 ⇒ 落这里，**不臆造**）。 */
function unknownContent(): Pick<
  SessionGroupView,
  "status" | "commonPrefix" | "commonPrefixExact" | "deltas" | "latestLineCount" | "sinceBaseline"
> {
  return {
    status: "unknown",
    commonPrefix: null,
    commonPrefixExact: false,
    deltas: new Map<string, number>(),
    latestLineCount: null,
    sinceBaseline: new Map<string, number | null>(),
  };
}

/** 后端 `content.status` 的合法取值（越界一律落 `unknown`，不把脏值透到 UI）。 */
const CONTENT_STATUSES: readonly SessionGroupStatus[] = ["latest", "behind", "diverge", "unknown"];

/**
 * 后端的内容结论 → 视图字段（**搬运 + 脏值兜底**，不做任何判据）。
 *
 * ⚠️ **判据不在这里**：`status` / `commonPrefix` / `deltas` / `latestLineCount` /
 * `sinceBaseline` 全部由 core 算完（`session::derive_content_status`）。前端重算一次 =
 * 再要一遍每个副本的逐行 hash（200 MB 级）—— 那正是本次修掉的东西。
 *
 * ★ **不得**在这里引入任何基于 `updatedAt` 的派生（§3.15.2「时间新 ≠ 内容新」）。
 */
function contentOf(
  content: SessionReplicaContent | null | undefined,
): Pick<SessionGroupView, "status" | "commonPrefix" | "commonPrefixExact" | "deltas" | "latestLineCount" | "sinceBaseline"> {
  if (!content) return unknownContent();
  const status = CONTENT_STATUSES.includes(content.status) ? content.status : "unknown";
  const deltas = new Map<string, number>();
  for (const [sessionId, delta] of Object.entries(content.deltas ?? {})) {
    if (typeof delta === "number") deltas.set(sessionId, delta);
  }
  const sinceBaseline = new Map<string, number | null>();
  for (const [sessionId, delta] of Object.entries(content.sinceBaseline ?? {})) {
    if (delta === null || typeof delta === "number") sinceBaseline.set(sessionId, delta);
  }
  return {
    status,
    commonPrefix: typeof content.commonPrefix === "number" ? content.commonPrefix : null,
    // 缺字段（旧后端）⇒ 按**精确**处理：那时后端本来就只有「未截断才给值」的语义，
    // 显式 false 才会让界面改口说「至少」。
    commonPrefixExact: content.commonPrefixExact !== false,
    deltas,
    latestLineCount:
      typeof content.latestLineCount === "number" ? content.latestLineCount : null,
    sinceBaseline,
  };
}

/** 从账号清单建 `uid → 显示名`（`nickname → email → id` 降级链；空值不登记）。 */
export function buildAccountLabels(accounts: AccountMeta[]): Map<string, string> {
  const labels = new Map<string, string>();
  for (const account of accounts) {
    const uid = displayText(account.uid);
    if (!uid) continue;
    const label =
      displayText(account.nickname) ?? displayText(account.email) ?? displayText(account.id);
    if (label && label.trim() !== "") labels.set(uid, label);
  }
  return labels;
}

/**
 * 建 `accountUid → (sessionId → 会话行)`。
 *
 * ⚠️ **只有一个键**（当前账号）：`list_sessions` 只列**当前账号**的会话。其它账号的副本
 * 拿不到会话行 ⇒ 落兜底标题。这是数据源的**已知局限**（§1.2 / §3.1 注），不是缺陷 ——
 * 视图以 `partial` + 固定文案**如实**呈现。
 *
 * ★ 内层是 **Map 而不是数组**：真机上副本行有 25k 条、当前账号的会话有 12k 条 ⇒
 * 用 `Array.find` 逐行找就是 25k × 12k = **3 亿次**字符串比较（主线程卡死几秒）。
 * 建索引是一次 O(12k)，之后每行 O(1)。
 */
export function buildSessionsByAccount(
  currentUid: string | null,
  sessions: Session[],
): Map<string, Map<string, Session>> {
  const byId = new Map<string, Session>();
  for (const session of sessions) {
    const id = displayText(session.id);
    if (id) byId.set(id, session);
  }
  const map = new Map<string, Map<string, Session>>();
  const key = displayText(currentUid);
  if (key) map.set(key, byId);
  return map;
}

/** 账号显示名：命中账号库用显示名；否则 `uid 前缀 + 「未知账号」`（**绝不**留空）。 */
function resolveAccountLabel(uid: string, labels: Map<string, string>): string {
  const known = displayText(labels.get(uid));
  if (known && known.trim() !== "") return known;
  // 前缀取前 8 位（与账号卡片的短 uid 口径一致）；uid 本身可能很短，兜底用整体。
  const prefix = uid.slice(0, 8) || uid;
  return t("sessions.replica.unknownAccount", { uid: prefix });
}

function findSession(
  sessionsByAccount: Map<string, Map<string, Session>>,
  uid: string,
  sessionId: string,
): Session | undefined {
  return sessionsByAccount.get(uid)?.get(sessionId);
}

/**
 * 把后端副本组 + 账号显示名 + 会话行 → 可渲染视图。
 *
 * **纯函数**（可测）：不读 store、不发请求。`partial` 在此**字面量锁死**为 `true`。
 *
 * ⚠️ **本函数不算内容状态**：`status` / `commonPrefix` / `deltas` / `latestLineCount` /
 * `sinceBaseline` 全部来自后端 `group.content`（core 按 jsonl 指纹算完）；
 * 这里只把「最近一次一致点」从 `group.baseline` 摊平成展示形态，并对脏响应兜底。
 */
export function buildGroupViews(
  groups: SessionReplicaGroup[],
  sessionsByAccount: Map<string, Map<string, Session>>,
  accountLabels: Map<string, string>,
): SessionGroupView[] {
  return groups.map((group) => {
    const rows: SessionReplicaRow[] = group.replicas.map((replica) => {
      const session = findSession(sessionsByAccount, replica.accountUid, replica.sessionId);
      return {
        accountUid: replica.accountUid,
        accountLabel: resolveAccountLabel(replica.accountUid, accountLabels),
        sessionId: replica.sessionId,
        // ★ 过 `displayText`：标题可能是脏对象 ⇒ 直接当 React 子节点会白屏（issue #2）。
        title: displayText(session?.title) ?? t("sessions.replica.titleFallback"),
        // `updatedAt` **只作事实透传**，不参与任何「落后 / 最新」派生（§3.15.2）。
        updatedAt: typeof session?.updatedAt === "number" ? session.updatedAt : null,
        hasHistory: session?.hasHistory === true,
      };
    });
    return {
      key: group.key,
      // ★ 恒 true：只覆盖「经本应用复制过」的会话（类型锁死诚实性，§3.5）。
      partial: true,
      rows,
      // ★ P14 S1：令牌也过 `displayText`（脏值 ⇒ `null` ⇒ 不给出口，而不是把对象当令牌送出去）。
      previewToken: displayText(group.previewToken),
      // ★ P14 S2：最近一次一致点（后端已按 `createdAt` → `a` → `b` 选出，前端不重选）。
      baseline:
        group.baseline && typeof group.baseline.lineCount === "number"
          ? { createdAt: group.baseline.createdAt, lineCount: group.baseline.lineCount }
          : null,
      // ★ 内容级结论：core 算完随响应回来（见 `contentOf`）。
      ...contentOf(group.content),
    };
  });
}

/**
 * ★ P8「安全同步」：把一组里**落后**的副本换算成 `{ sourceCid, targetCid }` 任务。
 *
 * ## 为什么源取「行数最多」的那一份
 *
 * `behind` 的语义是「公共前缀**覆盖到最短副本**」⇒ 任一副本都是**最长副本的前缀**
 * （两两公共前缀 ≥ 最短行数）⇒ 把最长的那份当源，追加进其余副本是**纯加法**。
 * ⚠️ 这不是「猜」：后端会**独立复核**「目标是不是源的严格前缀」，不成立就逐条失败
 * 且不落一个字节。前端这里只是**选一个合理方向**，判据不在前端。
 *
 * ## 什么时候返回空数组（= 不给出口）
 *
 * - 状态不是 `behind`：`latest` 无事可做；`diverge` **必须先人工决定保留哪一份**
 *   （分歧处理 D20 后置）—— 硬同步会丢内容；`unknown` 缺指纹 ⇒ **不臆造**；
 * - `deltas` 为空 / 最大增量为 0：没有可追加的内容；
 * - ★ **P14 S1：缺乐观令牌**（`previewToken` 为 `null`）—— 没有令牌就无法让后端复核
 *   「组还是不是你看到的那个组」，那就**不该发**这次写操作（宁可不让做，也不做错）。
 *
 * ⇒ 返回空数组时调用方**不渲染按钮**（§3.4 规则 1「无数据不渲染」，不摆假控件）。
 */
export function buildSyncItems(group: SessionGroupView): SyncSessionReplicaItem[] {
  if (group.status !== "behind") return [];
  if (group.rows.length < 2 || group.deltas.size === 0) return [];

  // ★ P14 S1：令牌必须先落地成局部量 —— 否则在下面的 `.map()` 回调里 TS 会丢掉收窄。
  const previewToken = group.previewToken;
  if (!previewToken) return [];

  const deltaOf = (row: SessionReplicaRow) => group.deltas.get(row.sessionId) ?? 0;
  const maxDelta = Math.max(...group.rows.map(deltaOf));
  if (maxDelta <= 0) return [];

  const source = group.rows.find((row) => deltaOf(row) === maxDelta);
  if (!source) return [];

  return group.rows
    .filter((row) => row.sessionId !== source.sessionId && deltaOf(row) < maxDelta)
    .map((row) => ({
      sourceCid: source.sessionId,
      targetCid: row.sessionId,
      groupKey: group.key,
      previewToken,
    }));
}

/**
 * ★ P8 **分歧处理**：把「用户选定保留的那一份」换算成 `{ sourceCid, targetCid }` 任务。
 *
 * ⚠️ 与 [`buildSyncItems`] **语义相反**：那边是**加法**（只追加），这里是**覆盖** ——
 * 目标原有的分歧内容会被丢弃。`PLAN.md` §3.15.2 要求「**不许静默覆盖**」⇒
 * 调用方必须先二次确认，并把 [`lostLinesFor`] 的条数**如实**告诉用户。
 *
 * ## 只在 `diverge` 给出口
 *
 * `latest` / `behind` 用加法就够了（`behind` 走 `buildSyncItems`），**没必要**让用户
 * 在「覆盖」和「追加」之间做无谓的选择；`unknown` 缺指纹 ⇒ 连哪份新都说不准，
 * **不许**拿它去覆盖（§3.4 规则 1：无数据不渲染）。
 *
 * `keepSessionId` 不在组里 ⇒ 返回空数组（防止 UI 传了个陈旧 id 就开覆盖）。
 *
 * ★ **P14 S1：缺乐观令牌 ⇒ 同样返回空数组** —— 覆盖是**破坏性**操作，比同步更需要
 * 「组还是不是你看到的那个组」这一层保证。
 */
export function buildUnifyItems(
  group: SessionGroupView,
  keepSessionId: string,
): UnifySessionReplicaItem[] {
  if (group.status !== "diverge") return [];
  if (!group.rows.some((row) => row.sessionId === keepSessionId)) return [];

  const previewToken = group.previewToken;
  if (!previewToken) return [];

  return group.rows
    .filter((row) => row.sessionId !== keepSessionId)
    .map((row) => ({
      sourceCid: keepSessionId,
      targetCid: row.sessionId,
      groupKey: group.key,
      previewToken,
    }));
}

/**
 * ★ P14 S1：从逐条结果里取出**失败的会话组键**（去重、保序）。
 *
 * ## 为什么需要它
 *
 * 批量提交部分失败后要给「只重试失败项」的出口。但**重试不能复用上一次的 items** ——
 * 那些 items 带着**旧令牌**，而令牌失效恰恰意味着组已经变了 ⇒ 重试必然再失败。
 * 正确做法是拿这组 `groupKey` 回**刷新后的 store** 重算任务（见 `SessionsPage.retrySync`）。
 *
 * 后端会把每条任务的 `groupKey` 回显在结果里；取不到的条目（理论上不该有）被跳过 ——
 * 宁可少给一个重试入口，也不送一次**没有令牌**的写操作。
 */
export function failedGroupKeys(
  results: ReadonlyArray<{ ok: boolean; groupKey?: string | null }>,
): string[] {
  const keys: string[] = [];
  const seen = new Set<string>();
  for (const result of results) {
    if (result.ok) continue;
    const key = displayText(result.groupKey);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    keys.push(key);
  }
  return keys;
}

/**
 * 某个副本被覆盖时**会丢掉的条数**（= 它相对共同旧版的新增条数，即它的 `delta`）。
 *
 * 推导：该副本的内容 = 共同旧版 + 它自己的新增；覆盖成选定那一份之后，
 * 它自己的那部分就没了 ⇒ **丢的就是它的 `delta`**。
 *
 * 缺指纹 / 该副本不在 `deltas` 里 ⇒ 返回 `null` —— **不臆造**（UI 如实显示「未知」，
 * 而不是显示一个编出来的 0）。
 */
export function lostLinesFor(group: SessionGroupView, sessionId: string): number | null {
  if (group.commonPrefix === null) return null;
  const delta = group.deltas.get(sessionId);
  return delta === undefined ? null : delta;
}
