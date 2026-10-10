/**
 * 会话工作台状态（P8 T02）—— 副本只读视图的数据编排。
 *
 * ## 为什么是一个 store（而不是页面内 state）
 *
 * 会话页的加载链是**三步 join**（ledger 组 → 账号显示名 → 会话行），且要跨「region 切换」
 * 复用。把它收口在一个 store 里，页面只负责「渲染 + 触发 load」，避免页面里堆 IO 逻辑
 * （与 `stores/gateway.ts` 同构）。
 *
 * ## 数据来源纪律
 *
 * - 副本组：`list_session_replicas`（**只读** copy ledger，**只读当前 region**，P8 ✅ #2）；
 * - 账号显示名：从**账号 store** join（`useAccountsStore`）—— **不重造第二份账号清单**（§1.5 / R9）；
 * - 会话行（标题 / `updatedAt` / `hasHistory`）：`list_sessions`（**当前账号**的会话）。
 *
 * ⚠️ **`updatedAt` 只作事实透传**，**不派生**「落后 / 分叉」（§3.15.2「时间新 ≠ 内容新」）。
 * 本 store **不得**引入任何组状态派生。
 *
 * ⚠️ **取不到会话行不致命**：副本组仍可见，标题落兜底（`buildGroupViews` 负责），
 * 并由视图的 `partial` + 固定文案**如实**标注「仅覆盖经本应用复制过的会话」。
 */

import { create } from "zustand";

import * as api from "@/lib/api";
import {
  buildAccountLabels,
  buildGroupViews,
  buildSessionsByAccount,
} from "@/lib/session-replicas";
import type { SessionGroupView } from "@/lib/session-replicas";
import type { Region, Session, SessionFingerprint, SyncSessionReplicaItem, SyncSessionReplicaResponse, UnifySessionReplicaItem, UnifySessionReplicaResponse } from "@/lib/types";
import { selectRegionSlice, useAccountsStore } from "@/stores/accounts";

interface SessionsState {
  /** 已 join 的可渲染副本组（空数组 = 无副本 / 尚未加载完成）。 */
  groups: SessionGroupView[];
  /**
   * 后端账本来源：`ledger` 有可用登记 / `empty` 无登记；**未加载时为 `null`**
   * （页面据此区分「加载中」与「真的空」，避免闪一下空态）。
   */
  source: "ledger" | "empty" | null;
  loading: boolean;
  error: string | null;
  /** 加载某 region 的副本组（账号显示名 / 会话行 join 一并完成）。 */
  load: (region: Region) => Promise<void>;
  /**
   * ★ P8 · **安全同步（增量追加）**：把源副本的新增行追加到落后副本。
   *
   * **不吞错、不自动重载** —— 逐条结果**原样**返回，由页面决定 toast 文案与
   * 「只重试失败项」；成功后再由页面触发 `load` 刷新（保持 store 无副作用）。
   */
  syncItems: (
    region: Region,
    items: SyncSessionReplicaItem[],
  ) => Promise<SyncSessionReplicaResponse>;
  /**
   * ★ P8 · **分歧处理（覆盖式统一）**：把选定保留的那一份覆盖到其余副本。
   *
   * ⚠️ **会丢弃目标的分歧内容**（后端覆盖前先备份）⇒ 调用方必须先二次确认。
   * 同样**不吞错、不自动重载**，逐条结果原样返回。
   */
  unifyItems: (
    region: Region,
    items: UnifySessionReplicaItem[],
  ) => Promise<UnifySessionReplicaResponse>;
}

/** 该 region 已加载的账号（从账号 store 读，**不另起一份清单**）。 */
function accountsFor(region: Region) {
  return selectRegionSlice(useAccountsStore.getState(), region).accounts;
}

export const useSessionsStore = create<SessionsState>((set) => ({
  groups: [],
  source: null,
  loading: false,
  error: null,

  async load(region) {
    set({ loading: true, error: null });
    try {
      const response = await api.listSessionReplicas(region);
      if (response.groups.length === 0) {
        set({ groups: [], source: response.source, loading: false });
        return;
      }

      // 账号显示名：从账号 store join。若该 region 尚未加载过账号，先拉一次
      //（`fetchAllRegions` 是账号 store 的公开入口，**唯一事实来源**不变）。
      let accounts = accountsFor(region);
      if (accounts.length === 0) {
        await useAccountsStore.getState().fetchAllRegions();
        accounts = accountsFor(region);
      }
      const accountLabels = buildAccountLabels(accounts);

      // 会话行：只列**当前账号**的会话（`list_sessions` 的既有语义）。取不到不致命 ——
      // 副本组仍可见，标题落兜底（诚实标注由 `partial` 承担）。
      let sessionsByAccount = new Map<string, Session[]>();
      try {
        const sessionResponse = await api.listSessions(region);
        sessionsByAccount = buildSessionsByAccount(
          sessionResponse.current,
          sessionResponse.sessions ?? [],
        );
      } catch {
        // 会话行不可用：保留空 map ⇒ 标题走 `titleFallback`。
      }

      // ★ P8：查**内容指纹**（组状态派生的数据源）—— **内容级**判据，不用 `updatedAt` 冒充。
      //   ⚠️ 查不到 ⇒ 各组落 `"unknown"`（**诚实**，不臆造）；**也不阻断**副本视图本身。
      let fingerprints: Record<string, SessionFingerprint | null> = {};
      try {
        const ids = response.groups.flatMap((group) =>
          group.replicas.map((replica) => replica.sessionId),
        );
        if (ids.length > 0) fingerprints = await api.sessionFingerprints(ids, region);
      } catch {
        // 指纹不可用：保留空 map ⇒ 全部组 `"unknown"`（副本视图仍可见）。
      }

      const groups = buildGroupViews(
        response.groups,
        sessionsByAccount,
        accountLabels,
        fingerprints,
      );
      set({ groups, source: response.source, loading: false });
    } catch (e) {
      set({ groups: [], error: api.asError(e), loading: false });
    }
  },

  /* ★ P8 安全同步：只转发 + 原样回结果。**不 set 任何 state** ——
     「什么时候刷新、怎么提示」是页面的决定；store 里藏一次隐式 reload 会让
     「部分失败」的 toast 与列表状态对不上（刷了一半看着像全成功）。 */
  syncItems(region, items) {
    return api.syncSessionReplicas(items, region);
  },

  /* ★ P8 分歧处理：同样只转发。**覆盖式**写操作 ⇒ 二次确认在页面侧，不在 store。 */
  unifyItems(region, items) {
    return api.unifySessionReplicas(items, region);
  },
}));
