import { create } from "zustand";
import * as api from "@/lib/api";
import type { AccountMeta, AppStatus, CreditExpiry, Region } from "@/lib/types";

/** 单个 region 的账号/状态/积分切片。CN 的切片与既有顶层字段同形，便于兼容既有调用方。 */
export interface RegionSlice {
  accounts: AccountMeta[];
  status: AppStatus | null;
  loading: boolean;
  error: string | null;
  creditMap: Record<string, CreditExpiry>;
  creditLoadingMap: Record<string, boolean>;
  refreshingCredits: boolean;
}

function emptySlice(): RegionSlice {
  return {
    accounts: [],
    status: null,
    loading: false,
    error: null,
    creditMap: {},
    creditLoadingMap: {},
    refreshingCredits: false,
  };
}

/** In-flight credit fetches per region, shared so a remount does not start a second round. */
const creditInflight: Record<Region, Set<string>> = { cn: new Set(), global: new Set() };
const statusInflight: Partial<Record<Region, Promise<AppStatus>>> = {};

function fetchStatus(region: Region): Promise<AppStatus> {
  const existing = statusInflight[region];
  if (existing) return existing;
  const promise = api.getStatus(region).finally(() => {
    statusInflight[region] = undefined;
  });
  statusInflight[region] = promise;
  return promise;
}

async function fetchCreditExpiry(id: string, region: Region): Promise<CreditExpiry> {
  try {
    return await api.getCreditExpiry(id, region);
  } catch (e) {
    return { ok: false, error: api.asError(e) };
  }
}

/** 从整体 state 取出某 region 的切片；CN 复用顶层字段，Global 读 `global`。 */
export function selectRegionSlice(state: AccountsState, region: Region): RegionSlice {
  if (region === "global") return state.global;
  return {
    accounts: state.accounts,
    status: state.status,
    loading: state.loading,
    error: state.error,
    creditMap: state.creditMap,
    creditLoadingMap: state.creditLoadingMap,
    refreshingCredits: state.refreshingCredits,
  };
}

/** 生成 zustand set 所需的局部状态：CN 直接写入顶层，Global 合并到 `global`。 */
function regionPatch(
  state: AccountsState,
  region: Region,
  patch: Partial<RegionSlice>,
): Partial<AccountsState> {
  if (region === "cn") return patch as Partial<AccountsState>;
  return { global: { ...state.global, ...patch } };
}

interface AccountsState {
  // —— CN（保持既有语义，兼容 App/Settings/统计页/钩子等既有调用方） ——
  accounts: AccountMeta[];
  status: AppStatus | null;
  loading: boolean;
  error: string | null;
  creditMap: Record<string, CreditExpiry>;
  creditLoadingMap: Record<string, boolean>;
  refreshingCredits: boolean;
  /**
   * 各 region「上一次积分取数完成」的墙钟（`0` = 本次会话还没取过）。
   *
   * ★ 必须**按 region 分开记**（issue #133）：自动刷新要判断「这一区是否已经过期」，
   * 共用一份时间会让刚切过去的另一个区被判成新鲜而跳过刷新。
   * 它只服务自动刷新与「上次刷新」提示，**不参与**任何数据口径判断。
   */
  creditRefreshedAt: Record<Region, number>;
  // —— 国际版 ——
  global: RegionSlice;

  /** 拉取 CN 状态 + 账号（兼容既有语义）。 */
  fetchAll: () => Promise<void>;
  /** 同时拉取 CN 与 Global 的状态 + 账号。 */
  fetchAllRegions: () => Promise<void>;
  /** 仅刷新 CN 状态（兼容既有轮询钩子）。 */
  refreshStatus: (signal?: AbortSignal) => Promise<void>;
  /** 仅刷新指定 region 状态。 */
  refreshRegionStatus: (region: Region, signal?: AbortSignal) => Promise<void>;
  /**
   * 同时刷新 CN 与 Global 的状态。
   *
   * 后台轮询必须走这个入口：`status.current` 决定卡片上的「当前账号」标记，
   * 只轮询 CN 会让国际版的状态停留在首屏快照上——切换国际账号后标记不会跟着走。
   */
  refreshAllStatus: (signal?: AbortSignal) => Promise<void>;
  deleteAccount: (id: string, region?: Region) => Promise<void>;
  /** Fetch credits only for ids not already cached. */
  ensureCredits: (accountIds: string[], region?: Region) => Promise<void>;
  /** Force-refresh credits. `silent` skips toolbar/card loading flicker (timer). */
  refreshCredits: (
    accountIds: string[],
    opts?: { silent?: boolean; region?: Region },
  ) => Promise<void>;
  importLocal: (region?: Region) => Promise<AccountMeta>;
  reconcileAccounts: (region?: Region) => Promise<void>;
}

async function loadRegion(region: Region): Promise<void> {
  useAccountsStore.setState((s) => regionPatch(s, region, { loading: true, error: null }));
  try {
    const [status, { accounts }] = await Promise.all([fetchStatus(region), api.getAccounts(region)]);
    useAccountsStore.setState((s) => regionPatch(s, region, { status, accounts, loading: false }));
  } catch (e) {
    useAccountsStore.setState((s) => regionPatch(s, region, { error: api.asError(e), loading: false }));
  }
}

export const useAccountsStore = create<AccountsState>((set, get) => ({
  ...emptySlice(),
  creditRefreshedAt: { cn: 0, global: 0 },
  global: emptySlice(),

  async fetchAll() {
    await loadRegion("cn");
  },

  async fetchAllRegions() {
    await Promise.all([loadRegion("cn"), loadRegion("global")]);
  },

  async refreshStatus(signal) {
    try {
      const status = await fetchStatus("cn");
      if (!signal?.aborted) set({ status });
    } catch {
      // 后台探测失败时保留最后一次成功状态，下一轮轮询继续尝试。
    }
  },

  async refreshRegionStatus(region, signal) {
    try {
      const status = await fetchStatus(region);
      if (!signal?.aborted) set((s) => regionPatch(s, region, { status }));
    } catch {
      // 保留最后一次成功状态。
    }
  },

  async refreshAllStatus(signal) {
    // 两个 region 各自独立：一个失败不影响另一个（`refreshRegionStatus` 内部已吞掉错误）。
    await Promise.all([
      get().refreshStatus(signal),
      get().refreshRegionStatus("global", signal),
    ]);
  },

  async deleteAccount(id, region = "cn") {
    await api.deleteAccount(id, region);
    creditInflight[region].delete(id);
    set((s) => {
      const slice = selectRegionSlice(s, region);
      const nextCredits = { ...slice.creditMap };
      const nextLoading = { ...slice.creditLoadingMap };
      delete nextCredits[id];
      delete nextLoading[id];
      return regionPatch(s, region, {
        accounts: slice.accounts.filter((a) => a.id !== id),
        creditMap: nextCredits,
        creditLoadingMap: nextLoading,
      });
    });
  },

  async ensureCredits(accountIds, region = "cn") {
    await loadCredits(accountIds, false, false, region);
  },

  async refreshCredits(accountIds, opts) {
    await loadCredits(accountIds, true, opts?.silent === true, opts?.region ?? "cn");
  },

  async importLocal(region = "cn") {
    const res = await api.importLocal(region);
    await get().reconcileAccounts(region);
    return res.account;
  },

  async reconcileAccounts(region = "cn") {
    const { accounts } = await api.getAccounts(region);
    set((s) => regionPatch(s, region, { accounts }));
  },
}));

async function loadCredits(accountIds: string[], force: boolean, silent: boolean, region: Region) {
  const ids = [...new Set(accountIds.filter(Boolean))];
  if (ids.length === 0) return;

  const state = useAccountsStore.getState();
  const slice = selectRegionSlice(state, region);
  const toFetch = force
    ? ids
    : ids.filter((id) => slice.creditMap[id] === undefined && !creditInflight[region].has(id));
  if (toFetch.length === 0) return;

  for (const id of toFetch) creditInflight[region].add(id);
  if (!silent) {
    useAccountsStore.setState((s) => {
      const cur = selectRegionSlice(s, region);
      const creditLoadingMap = { ...cur.creditLoadingMap };
      for (const id of toFetch) creditLoadingMap[id] = true;
      return regionPatch(s, region, {
        creditLoadingMap,
        refreshingCredits: force ? true : cur.refreshingCredits,
      });
    });
  }

  await Promise.all(
    toFetch.map(async (id) => {
      const result = await fetchCreditExpiry(id, region);
      creditInflight[region].delete(id);
      useAccountsStore.setState((s) => {
        const cur = selectRegionSlice(s, region);
        return regionPatch(s, region, {
          creditMap: { ...cur.creditMap, [id]: result },
          creditLoadingMap: silent ? cur.creditLoadingMap : { ...cur.creditLoadingMap, [id]: false },
        });
      });
    }),
  );

  useAccountsStore.setState((s) => {
    const cur = selectRegionSlice(s, region);
    return regionPatch(s, region, {
      refreshingCredits: silent ? cur.refreshingCredits : force ? false : cur.refreshingCredits,
    });
  });

  // 「上一次取数时间」按 region 记（自动刷新判过期 + 界面「上次刷新」提示共用这一份）。
  useAccountsStore.setState((s) => ({
    creditRefreshedAt: { ...s.creditRefreshedAt, [region]: Date.now() },
  }));
}
