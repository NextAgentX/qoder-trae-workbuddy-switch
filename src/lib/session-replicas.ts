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
 * ⚠️ **不许用 `updatedAt` 冒充「落后 / 最新」**：`updatedAt` 只能表达「谁最近被改过」，
 * 不能表达「谁落后 N 条」（§3.15.2「时间新 ≠ 内容新」）。本模块**只把 `updatedAt` 当作事实
 * 透传**，**不派生任何状态**。
 *
 * ⚠️ **展示字段出口归一为「字符串或 null」**：会话标题 / 账号显示名一律过
 * {@link displayText}（对象当 React 子节点 ⇒ 白屏，issue #2）。
 */

import { displayText } from "@/lib/display-text";
import { t } from "@/lib/i18n";
import type { AccountMeta, Session, SessionFingerprint, SessionReplicaGroup } from "@/lib/types";

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
  /** 各组员相对共同旧版的**新增条数**（`sessionId → delta`）；`unknown` 时为空。 */
  deltas: Map<string, number>;
  /** 行数最多的副本的条数（`latestLineCount`）；`unknown` 时为 `null`。 */
  latestLineCount: number | null;
}

/** 两个指纹的公共前缀长度（与后端 `common_prefix_len` **同一判据**）。 */
function commonPrefixLen(a: SessionFingerprint, b: SessionFingerprint): number {
  const n = Math.min(a.prefixHashes.length, b.prefixHashes.length);
  let i = 0;
  while (i < n && a.prefixHashes[i] === b.prefixHashes[i]) i += 1;
  return i;
}

/**
 * 由**内容指纹**派生组状态（P8）。
 *
 * ⚠️ **任一副本缺 fingerprint ⇒ 整体 `unknown`**：宁可说「未知」，也不用部分数据
 * 冒充完整结论（诚实优先，同 §3.5 的 `partial` 标注）。
 */
export function deriveGroupStatus(
  rows: SessionReplicaRow[],
  fingerprints: Record<string, SessionFingerprint | null>,
): Pick<SessionGroupView, "status" | "commonPrefix" | "deltas" | "latestLineCount"> {
  const empty = {
    status: "unknown" as const,
    commonPrefix: null,
    deltas: new Map<string, number>(),
    latestLineCount: null,
  };
  if (rows.length < 2) return empty; // 单副本无「组」可言（调用方也不该建组）。

  const list: SessionFingerprint[] = [];
  for (const row of rows) {
    const fp = fingerprints[row.sessionId];
    if (!fp) return empty; // 缺一个 ⇒ 未知
    list.push(fp);
  }

  // 所有副本两两公共前缀的**最小值** = 全体的共同旧版。
  let common = Math.min(...list.map((f) => f.prefixHashes.length));
  for (let i = 0; i < list.length; i += 1) {
    for (let j = i + 1; j < list.length; j += 1) {
      common = Math.min(common, commonPrefixLen(list[i], list[j]));
    }
  }

  const counts = list.map((f) => f.lineCount);
  const maxLines = Math.max(...counts);
  const minLines = Math.min(...counts);
  const deltas = new Map<string, number>();
  rows.forEach((row, i) => deltas.set(row.sessionId, counts[i] - common));

  // 公共前缀没覆盖到最短副本 ⇒ 双方在共同旧版之后各自改过 ⇒ 分叉（优先级最高）。
  const status: SessionGroupStatus =
    common < minLines ? "diverge" : maxLines === minLines ? "latest" : "behind";

  return { status, commonPrefix: common, deltas, latestLineCount: maxLines };
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
 * 建 `accountUid → 会话行`。
 *
 * ⚠️ **只有一个键**（当前账号）：`list_sessions` 只列**当前账号**的会话。其它账号的副本
 * 拿不到会话行 ⇒ 落兜底标题。这是数据源的**已知局限**（§1.2 / §3.1 注），不是缺陷 ——
 * 视图以 `partial` + 固定文案**如实**呈现。
 */
export function buildSessionsByAccount(
  currentUid: string | null,
  sessions: Session[],
): Map<string, Session[]> {
  const map = new Map<string, Session[]>();
  const key = displayText(currentUid);
  if (key) map.set(key, sessions);
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
  sessionsByAccount: Map<string, Session[]>,
  uid: string,
  sessionId: string,
): Session | undefined {
  return sessionsByAccount.get(uid)?.find((session) => session.id === sessionId);
}

/**
 * 把后端副本组 + 账号显示名 + 会话行 → 可渲染视图。
 *
 * **纯函数**（可测）：不读 store、不发请求。`partial` 在此**字面量锁死**为 `true`。
 */
export function buildGroupViews(
  groups: SessionReplicaGroup[],
  sessionsByAccount: Map<string, Session[]>,
  accountLabels: Map<string, string>,
  /**
   * ★ P8：各副本的**内容指纹**（`sessionId → 指纹`；取不到为 `null`）。
   * 缺省 `{}` ⇒ 全部组落 `"unknown"`（**诚实**：没查过就别说状态）。
   */
  fingerprints: Record<string, SessionFingerprint | null> = {},
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
      // ★ 内容级状态（缺 fingerprint ⇒ `"unknown"`，**不臆造**）。
      ...deriveGroupStatus(rows, fingerprints),
    };
  });
}
