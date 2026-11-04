import { useMemo } from "react";
import { create } from "zustand";

import * as api from "@/lib/api";
import {
  DEFAULT_GATEWAY_CONFIG,
  normalizeGatewayConfig,
  normalizeGatewayStatus,
  poolOf,
} from "@/lib/gateway";
import type {
  AccountStrategy,
  AccountStrategyView,
  ApiKeyRecord,
  CatalogSnapshot,
  CreateApiKeyResult,
  GatewayConfig,
  GatewayLogEntry,
  FallbackStats,
  GatewayStatus,
  ModelFallbackView,
  ModelProbeCell,
  ModelProbeJob,
  ModelProbeState,
  Region,
  ScopeFallbackConfig,
} from "@/lib/types";
import { SCOPES, type Scope } from "@/lib/scope";
import { mergeProbeCells, type ProbeCellIndex } from "@/lib/limit-link";

/** 网关配置缺省值（与后端 GatewayConfig::default 对齐）。 */
export { DEFAULT_GATEWAY_CONFIG };

/**
 * 未拉取到的作用域回落的**稳定空集**（引用稳定 ⇒ 不触发无谓重渲染）。
 * 消费者：模型列表组件（`s.disabled[scope]` 可能尚未加载）。
 */
export const EMPTY_DISABLED_SET: ReadonlySet<string> = new Set();

/**
 * 未拉取到的作用域的**图片能力覆盖表**空值（引用稳定 ⇒ 不触发无谓重渲染）。
 *
 * ⚠️ 空表 ≠「都不支持」：缺失的键语义是**跟随上游声明**（issue #101 的三态）。
 */
export const EMPTY_IMAGE_CAPS: Readonly<Record<string, boolean>> = Object.freeze({});

function defaultStrategyView(region?: Region): AccountStrategyView {
  return {
    region,
    strategy: { kind: "current" },
    selected: null,
  };
}

function defaultStrategies(): Record<Region, AccountStrategyView> {
  return { cn: defaultStrategyView("cn"), global: defaultStrategyView("global") };
}

/**
 * 限流探测的轮询间隔（约 1s，与 `switch_progress` 的进度轮询同款）。
 *
 * 依据（方案 §3.4）：本仓**没有** EventSource / SSE 前端消费，长任务一律走
 * 「前端定时 `probeStatus()`」——既有先例 `switch-account-dialog.tsx:191`。
 */
const PROBE_POLL_MS = 1000;

/**
 * 探测轮询定时器（模块级，**唯一实例**）。
 *
 * ⚠️ 必须是模块级而不是组件级：`model-probe-card` 可能因切换分区 / 页面导航卸载，
 * 组件里的 timer 会随卸载被遗忘 —— 于是「切走再切回」会留下多个并发轮询。
 * 这里由 store 单点持有，卡片卸载时显式调 `stopProbePolling()` 清掉。
 */
let probeTimer: ReturnType<typeof setInterval> | null = null;

function stopProbeTimer(): void {
  if (probeTimer !== null) {
    clearInterval(probeTimer);
    probeTimer = null;
  }
}

/** 兼容后端可能缺字段：始终补齐两个 region。 */
function normalizeStrategies(input: Partial<Record<Region, AccountStrategyView>> | null | undefined): Record<Region, AccountStrategyView> {
  return {
    cn: input?.cn ? { ...input.cn, region: input.cn.region ?? "cn" } : defaultStrategyView("cn"),
    global: input?.global ? { ...input.global, region: input.global.region ?? "global" } : defaultStrategyView("global"),
  };
}

interface GatewayState {
  config: GatewayConfig;
  status: GatewayStatus | null;
  keys: ApiKeyRecord[];
  models: Record<Region, CatalogSnapshot | null>;
  /**
   * 每个**作用域**被用户**禁用**的模型 id 集合（P11）。
   *
   * 它是**用户偏好**（§2.7，不进 `capabilities`），与后端 `/v1/models` 过滤（出口 ①）、
   * 调用拒绝（出口 ②）**同源** —— 后端读的是同一份存储（`gateway_disabled_models.json`），
   * 故这里看到的禁用集合就是各出口真正在用的那份。
   *
   * ⚠️ 键是**作用域**（2026-10-15 从 region 升级）：三个平台互不相通的模型目录各自分家，
   * 同区域的 WB / Trae / Qoder 不再共用一把键。懒加载 ⇒ `Partial`（未拉取的作用域为 `undefined`）。
   *
   * ⚠️ 用 `Set` 是为了 O(1) 命中；更新时**必须换一个新 `Set`**（zustand 靠引用变化触发重渲染）。
   */
  disabled: Partial<Record<Scope, Set<string>>>;
  /** 模型降级链（P13）：按作用域缓存视图。缺键 = 还没读过（**不是**「没有链」）。 */
  fallback: Partial<Record<Scope, ModelFallbackView>>;
  /** 近 7 天降级统计（**落盘账本**，与 `fallback.events` 的环形缓冲不同源）。 */
  fallbackStats: Partial<Record<Scope, FallbackStats>>;
  /**
   * 每个**作用域**的图片能力**覆盖表**（issue #101）：`{ 模型 id: 是否支持图片 }`。
   *
   * ⚠️ **缺失的键 = 跟随上游声明**，不是「不支持」。三态语义见 `lib/model-image.ts`；
   * 把这里当布尔表读会得到「所有没出现过的模型都不支持图片」—— 那是错的。
   *
   * 与 `disabled` 同构：进程内唯一存储在后端（`gateway_model_image_caps.json`），
   * 三条产品线 `/v1/models` 的声明读的是同一份，所以这里改完客户端立刻能看到。
   */
  imageCaps: Partial<Record<Scope, Record<string, boolean>>>;
  strategies: Record<Region, AccountStrategyView>;
  logs: GatewayLogEntry[];
  /**
   * 最近一次（或进行中）的**限流探测作业**（方案 §3.1）。
   *
   * `null` = 从未跑过 / 已被换作用域清掉（后端以 `{ phase: "idle" }` 表示）。
   * 与后端 `ProbeJob` 的 wire 形状**逐字段一致**（snake_case，见 `model_probe.rs`）。
   */
  probe: ModelProbeJob | null;
  /**
   * **实测结论的跨作业累积索引**（`scope → uid → model → 格`）——
   * 账号页「客户端限流」与网关卡矩阵**读同一份**（口径见 `lib/limit-link.ts`）。
   *
   * ## 为什么不能只留 `probe`
   *
   * 后端作业是**单槽**：每次 `probe_start` 都新建一份从空 `cells` 开始的作业。
   * 若两处都直接读 `probe.cells`：
   * - 账号页「逐行实测」探第二行 ⇒ 第一行的结论**凭空消失**；
   * - 网关卡在矩阵里「重测这一格」⇒ 整张矩阵塌成 1 格。
   *
   * 故读侧按 `(uid, model)` 合并历史快照（`mergeProbeCells`）。**只合并不擦除**：
   * 某格探出 `ok` 就覆盖该格；某次作业没包含它**不会**被删掉。
   *
   * ⚠️ 进程内存储（与后端作业槽同生命周期）：重启即空，不假装是持久账本。
   */
  probeCells: ProbeCellIndex;
  /**
   * 每个作用域**落盘台账**的最近写入时刻（毫秒）—— `probe_ledger.updated_at_ms`。
   *
   * 只服务文案（「实测 3 格 · 限流 1（10-23 09:12）」）：没有它，重启后汇总条不知道
   * 这批结论是什么时候的。**不参与任何判据**。
   */
  probeLedgerAt: Record<string, number>;
  /**
   * **被动底色（来源之一）**：客户端限流台账（`client_limits`）里「仍受限」的 `uid × 模型`。
   *
   * ⚠️ 这只是被动底色的**一半**；另一半是**池的模型级冷却**（`GatewayPoolAccount.modelCoolings`，
   * 随 `status` 到达）。两者在读取处由 {@link useProbePassive} **派生**成一份并集 ——
   * 消费方（探测卡 / 模型表列）**只读那份派生值**，别再各自拼一份。
   * 键是**账号 uid**（与池条目同键），内层键是**模型名**。
   */
  probePassive: Record<string, Record<string, ModelProbeState>>;
  loading: boolean;
  saving: boolean;
  error: string | null;
  lastLoadedAt: number;

  loadConfig: () => Promise<void>;
  saveConfig: (config: GatewayConfig) => Promise<void>;
  refreshStatus: () => Promise<void>;
  loadKeys: () => Promise<void>;
  /** `customKey` 为空 ⇒ 后端自动生成；非空 ⇒ 原样使用（第三行「自定义密钥」）。 */
  createKey: (
    name: string,
    region: Region,
    customKey?: string | null,
  ) => Promise<CreateApiKeyResult>;
  revokeKey: (id: string) => Promise<void>;
  /** ★ issue #76：更新某把 Key 的策略（完整策略，非补丁）。 */
  setKeyPolicy: (id: string, policy: api.KeyPolicy) => Promise<void>;
  /** 撤销吊销：清 `revokedAt`，Key 重新可用（后端拒绝「未吊销」的恢复）。 */
  restoreKey: (id: string) => Promise<void>;
  deleteKey: (id: string) => Promise<void>;
  loadModels: (region: Region) => Promise<void>;
  refreshModels: (region: Region) => Promise<void>;
  /** 读取某**作用域**的禁用模型集合（P11）。 */
  loadDisabled: (scope: Scope) => Promise<void>;
  /** 置位 / 清位某**作用域**的一个模型（禁用 / 启用）—— 后端幂等。 */
  setModelDisabled: (scope: Scope, modelId: string, disabled: boolean) => Promise<void>;
  /** 读取某**作用域**的图片能力覆盖表（issue #101）。 */
  loadImageCaps: (scope: Scope) => Promise<void>;
  /** 读取某**作用域**的降级链（含备选的实时健康态）。 */
  loadFallback: (scope: Scope) => Promise<void>;
  /** 读取某**作用域**近 7 天的降级统计（落盘账本）。 */
  loadFallbackStats: (scope: Scope) => Promise<void>;
  /** 保存某**作用域**的降级链；后端返回最新视图，直接覆盖缓存（不再多发一次读）。 */
  saveFallback: (scope: Scope, config: ScopeFallbackConfig) => Promise<void>;
  /**
   * 写图片能力覆盖：`true` / `false` = 手动指定，**`null` = 还原上游声明**（后端删键）。
   *
   * 三态是这块的要点 —— 实现成 `false` 会让「还原」变成「明确不支持」。
   */
  setModelImageCap: (scope: Scope, modelId: string, supports: boolean | null) => Promise<void>;
  loadStrategies: () => Promise<void>;
  saveStrategy: (region: Region, strategy: AccountStrategy) => Promise<void>;
  loadLogs: () => Promise<void>;
  clearLogs: () => Promise<void>;
  /**
   * 启动一次限流探测（后端「作业 + 轮询」；第二次调用返回「已有探测进行中」）。
   *
   * `cells`（**显式格清单**）与 `uids` / `models`（**笛卡尔积**）互斥，后者优先判 `cells`。
   * 账号页台账卡传 `cells`（台账 3 条 = 3 格）；网卡选择态传两根轴（用户勾了账号与模型）。
   */
  startProbe: (args: {
    scope: Scope;
    uids?: string[];
    models?: string[];
    cells?: { uid: string; model: string }[];
    adopt: boolean;
  }) => Promise<{ started: boolean; total: number }>;
  /** 拉一次作业快照；`running` 时自动续轮询，`done` / `cancelled` 时清定时器。 */
  refreshProbeStatus: (scope: Scope) => Promise<void>;
  /** 取消剩余格（保留已探到的部分结果）。 */
  cancelProbe: (scope: Scope) => Promise<void>;
  /** 读取该 region 的**被动底色**（客户端台账；失败静默为空表）。 */
  loadProbePassive: (region: Region) => Promise<void>;
  /**
   * 拉某作用域的**实测台账**（后端落盘、跨重启）并入 `probeCells`。
   *
   * ★ 与 `refreshProbeStatus` 的分工：后者是**本轮作业**（单槽、进程内），前者是
   * 「这些格最近一次怎么样」（`probe_results.json`，按作用域隔离）。任一侧（账号页台账卡 /
   * 网卡矩阵）进入时都拉一次 ⇒ 重启后两处仍看得到同一份实测结论。
   */
  loadProbeLedger: (scope: Scope) => Promise<void>;
  /** 停掉轮询定时器（卡片卸载时调用，防泄漏）。 */
  stopProbePolling: () => void;
  /** 首次进入页面：并发拉取配置 / 状态 / Keys / 策略 / 日志 / 两版模型。 */
  loadAll: () => Promise<void>;
}

export const useGatewayStore = create<GatewayState>((set, get) => ({
  config: DEFAULT_GATEWAY_CONFIG,
  status: null,
  keys: [],
  models: { cn: null, global: null },
  // 默认全开（D27）：各作用域的 `disabled` 懒加载，未拉取即视为空集，不替用户做决定。
  disabled: {},
  /** 模型降级链（P13）：按作用域缓存视图。空 = 还没读过（不是「没有链」）。 */
  fallback: {},
  fallbackStats: {},
  // 图片能力覆盖懒加载：空表 = 全部跟随上游声明（**不是**「都不支持」，issue #101）。
  imageCaps: {},
  strategies: defaultStrategies(),
  logs: [],
  // 探测未跑过（后端 `{ phase: "idle" }` 归一为 `null`）；被动底色懒加载，默认空表。
  probe: null,
  probeCells: {},
  probeLedgerAt: {},
  probePassive: {},
  loading: false,
  saving: false,
  error: null,
  lastLoadedAt: 0,

  async loadConfig() {
    try {
      const config = await api.getGatewayConfig();
      set({ config: normalizeGatewayConfig(config) });
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async saveConfig(config) {
    set({ saving: true });
    try {
      const saved = await api.saveGatewayConfig(config);
      set({ config: normalizeGatewayConfig(saved), error: null });
      await get().refreshStatus();
    } finally {
      set({ saving: false });
    }
  },

  async refreshStatus() {
    try {
      set({ status: normalizeGatewayStatus(await api.gatewayStatus()) });
    } catch (e) {
      set({ status: null, error: api.asError(e) });
    }
  },

  async loadKeys() {
    try {
      const { keys } = await api.listApiKeys();
      set({ keys: keys ?? [] });
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async createKey(name, region, customKey) {
    const result = await api.createApiKey(name, region, customKey);
    /* 失败（如自定义密钥与台账里已有 Key 撞车）时后端**没有建出记录**，
       这里必须直接返回：否则会顺带做一次列表刷新 + 模型快照刷新，
       页面看起来「动了但没变化」，还会把 `error` 覆盖成模型侧的报错。 */
    if (result.ok === false) return result;
    if (result.record) {
      set((s) => ({ keys: [result.record!, ...s.keys.filter((key) => key.id !== result.record!.id)] }));
    } else {
      await get().loadKeys();
    }
    // ★ 「接入即识别」（P10 §1.6 / T03）：创建 Key 后**顺手拉一次**该 region 快照。
    // 后端在 create_api_key 成功后 fire-and-forget 探测（可能此刻尚未完成），故这里只是
    // **best-effort**：拿到的是「当前」快照（探测已写 live 就是实时，否则仍是缓存 / 内置）。
    // 真正的定论发生在下次 `loadModels`（进入模型页 / 手动「重新识别」）—— 与 §3.7 时序一致。
    // 失败静默：`loadModels` 内部已 catch（绝不 reject），不会污染「Key 已创建」的成功语义。
    await get().loadModels(region);
    return result;
  },

  async revokeKey(id) {
    await api.revokeApiKey(id);
    await get().loadKeys();
  },

  async restoreKey(id) {
    await api.restoreApiKey(id);
    await get().loadKeys();
  },

  async deleteKey(id) {
    await api.deleteApiKey(id);
    await get().loadKeys();
  },

  async setKeyPolicy(id, policy) {
    await api.setKeyPolicy(id, policy);
    // 保存后**重新拉列表**：策略写进台账，列表要反映新值（含掩码视图的七件套）。
    await get().loadKeys();
  },

  async loadModels(region) {
    try {
      const snapshot = await api.getGatewayModels(region);
      set((s) => ({ models: { ...s.models, [region]: snapshot } }));
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async refreshModels(region) {
    const snapshot = await api.refreshGatewayModels(region);
    set((s) => ({ models: { ...s.models, [region]: snapshot } }));
  },

  async loadDisabled(scope) {
    try {
      const { ids } = await api.getDisabledModels(scope);
      set((s) => ({ disabled: { ...s.disabled, [scope]: new Set(ids ?? []) } }));
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async setModelDisabled(scope, modelId, disabled) {
    // 先落库（后端幂等），成功后再更新内存 —— 失败时集合保持不变，行不会「假装切了」。
    await api.setModelDisabled(scope, modelId, disabled);
    set((s) => {
      const next = new Set(s.disabled[scope]);
      if (disabled) next.add(modelId);
      else next.delete(modelId);
      return { disabled: { ...s.disabled, [scope]: next } };
    });
  },

  async loadFallback(scope) {
    try {
      const view = await api.getModelFallback(scope);
      set((s) => ({ fallback: { ...s.fallback, [scope]: view } }));
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async loadFallbackStats(scope) {
    try {
      const stats = await api.getFallbackStats(scope);
      set((s) => ({ fallbackStats: { ...s.fallbackStats, [scope]: stats } }));
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async saveFallback(scope, config) {
    // 后端返回**最新视图**（含实时健康态）⇒ 直接覆盖缓存，不再多发一次读。
    // 状态点因此会立刻反映「刚配的目标现在是否在闸门里」。
    const view = await api.saveModelFallback(scope, config);
    set((s) => ({ fallback: { ...s.fallback, [scope]: view } }));
  },

  async loadImageCaps(scope) {
    try {
      const { caps } = await api.getModelImageCaps(scope);
      set((s) => ({ imageCaps: { ...s.imageCaps, [scope]: caps ?? {} } }));
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async setModelImageCap(scope, modelId, supports) {
    // 与禁用同款：先落库，成功后再改内存 —— 失败时覆盖表保持不变。
    await api.setModelImageCap(scope, modelId, supports);
    set((s) => {
      const next = { ...(s.imageCaps[scope] ?? {}) };
      // `null` = 还原上游 ⇒ **删键**（不是写 false）。这条与后端 `set(.., None)` 同义。
      if (supports === null) delete next[modelId];
      else next[modelId] = supports;
      return { imageCaps: { ...s.imageCaps, [scope]: next } };
    });
  },

  async loadStrategies() {
    try {
      const map = await api.getAccountStrategy();
      set({ strategies: normalizeStrategies(map) });
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async saveStrategy(region, strategy) {
    await api.saveAccountStrategy(region, strategy);
    await get().loadStrategies();
  },

  async loadLogs() {
    try {
      const { logs } = await api.getGatewayLogs();
      set({ logs: [...(logs ?? [])].sort((a, b) => b.ts - a.ts) });
    } catch (e) {
      set({ error: api.asError(e) });
    }
  },

  async clearLogs() {
    await api.clearGatewayLogs();
    set({ logs: [] });
  },

  async startProbe(args) {
    const result = await api.probeStart(args);
    // 启动后立刻拉一次快照（`running` 会在 `refreshProbeStatus` 里起轮询）。
    await get().refreshProbeStatus(args.scope);
    return result;
  },

  async refreshProbeStatus(scope) {
    try {
      const job = await api.probeStatus(scope);
      // 后端用 `{ phase: "idle" }` 表示「从未跑过」⇒ 归一成 `null`（其余档字段齐全）。
      // ★ 还要按 **scope 收窄**：后端作业是**全局单槽**，`probe_status` 不按作用域过滤 ——
      //   少了这一步，wb-cn 的探测结果会画到 qoder-cn 的矩阵上（同名模型、不同上游）。
      const raw =
        job && (job as ModelProbeJob).phase && (job as ModelProbeJob).phase !== "idle"
          ? (job as ModelProbeJob)
          : null;
      const next = raw && raw.scope === scope ? raw : null;
      /* ★ 读侧合并（见 `probeCells` 的注释）：账号页与网关卡矩阵都读这份。
         只合并不擦除；没有新格时 `mergeProbeCells` 原样返回同一引用 ⇒ 不空转重渲染。 */
      set((state) => ({
        probe: next,
        probeCells: next ? mergeProbeCells(state.probeCells, scope, next.cells) : state.probeCells,
      }));
      if (next && next.phase === "running") {
        // `running` ⇒ 续轮询（`setInterval` 单实例；已在跑时先清再起，杜绝并发）。
        stopProbeTimer();
        probeTimer = setInterval(() => {
          void get().refreshProbeStatus(scope);
        }, PROBE_POLL_MS);
      } else {
        // `done` / `cancelled`（或 idle）⇒ 必须清掉定时器，否则会一直空转。
        stopProbeTimer();
        /* 作业收尾 ⇒ 再对齐一次**落盘台账**：后端在 `finish` 里合并并写盘，这里读回来
           才能保证「前端索引」与「服务端台账」不会因为漏收快照而长期分叉。 */
        if (next && (next.phase === "done" || next.phase === "cancelled")) {
          void get().loadProbeLedger(scope);
        }
      }
    } catch (e) {
      stopProbeTimer();
      set({ error: api.asError(e) });
    }
  },

  async cancelProbe(scope) {
    try {
      await api.probeCancel(scope);
    } finally {
      // 取消后立即对齐一次：后端会把 phase 置 `cancelled` 并保留部分结果。
      await get().refreshProbeStatus(scope);
    }
  },

  async loadProbeLedger(scope) {
    try {
      const ledger = await api.probeLedger(scope);
      if (!ledger?.cells?.length) return;
      set((state) => ({
        probeCells: mergeProbeCells(state.probeCells, scope, ledger.cells),
        probeLedgerAt: ledger.updated_at_ms
          ? { ...state.probeLedgerAt, [scope]: ledger.updated_at_ms }
          : state.probeLedgerAt,
      }));
    } catch {
      // 台账是**加分项**（跨重启的便利）：拉不到就保持现有数据，绝不污染 error 条。
    }
  },

  async loadProbePassive(region) {
    try {
      const res = await api.clientLimits(region);
      const map: Record<string, Record<string, ModelProbeState>> = {};
      for (const entry of res.limits ?? []) {
        // 只认**仍受限**（`active`）的条目：`unknown` 推不出「仍受限」，`recovered` 已恢复。
        if (entry.state !== "active") continue;
        const uid = entry.accountUid;
        const model = entry.model;
        if (!uid || !model) continue;
        (map[uid] ??= {})[model] = "rate_limited_model";
      }
      set({ probePassive: map });
    } catch {
      // 被动底色是**加分项**：拉不到就当空表，绝不让它污染主流程的错误条。
      set({ probePassive: {} });
    }
  },

  stopProbePolling() {
    stopProbeTimer();
  },

  async loadAll() {
    set({ loading: true, error: null });
    try {
      await Promise.all([
        get().loadConfig(),
        get().refreshStatus(),
        get().loadKeys(),
        get().loadStrategies(),
        get().loadLogs(),
        get().loadModels("cn"),
        get().loadModels("global"),
        /* 禁用偏好按**作用域**键控 ⇒ 六个作用域全拉（每个只是一次小 GET），
           任一平台的模型页进入时都已有数据，不必各自等待。 */
        ...SCOPES.map((scope) => get().loadDisabled(scope)),
      ]);
      set({ lastLoadedAt: Date.now() });
    } finally {
      set({ loading: false });
    }
  },
}));

/**
 * 被动底色的**来源**（§5.3 角标「真实流量」/「客户端日志」—— 两者必须能区分）。
 *
 * - `flow` = 池的**模型级冷却**（`accounts[].modelCoolings`）—— 经网关**真实流量**打出来的；
 * - `ledger` = **客户端限流台账**（`client_limits`）—— 只是「客户端日志里出现过」。
 */
export type ProbePassiveSource = "flow" | "ledger";

/** 未拉取过的作用域回落的**稳定空表**（见 `useProbeCells`）。 */
const EMPTY_PROBE_SCOPE: Record<string, Record<string, ModelProbeCell>> = {};

/**
 * 被动底色的**一格**：状态 + **来源**。
 *
 * ⚠️ 两个来源语义不同（一个是网关实测、一个只是客户端日志推断），此前被并成一档只发一个
 * 状态串 ⇒ 用户分不出。这里把来源一并带出，消费方（探测卡矩阵 / 模型表列）**读同一份**。
 */
export interface ProbePassiveInfo {
  /** 被动状态（池里只有模型级冷却一档；台账也记这一档）。 */
  state: ModelProbeState;
  /** 来源 —— **两来源都有时以 `flow`（真实流量）优先**。 */
  source: ProbePassiveSource;
  /** 两来源**同时**有记录（`source` 仍以 flow 优先，文案据此说明二者皆有）。 */
  both: boolean;
}

/**
 * 被动底色的**唯一合并口径**（纯函数，可测）：客户端限流台账 ∪ 池的模型级冷却。
 *
 * - 客户端台账（`ledger` = `probePassive`，来自 `client_limits`）：键 `uid → 模型名 → 状态`，
 *   来源记 `ledger`（客户端日志）；
 * - 池冷却（`accounts[].modelCoolings`，camelCase `{模型名: 解冻ms}`）：后端只发未过期的，
 *   这里**再判一次** `until_ms > now`（防御 + 便于单测），一律记 `rate_limited_model`
 *  （池里只有模型级冷却这一档），来源记 `flow`（真实流量）。
 *
 * ⚠️ **两来源同一格时 `flow` 优先**（`source: "flow"` + `both: true`）—— 真实流量是更强的证据；
 * `both` 让文案能说明「客户端日志里也有」。
 *
 * ⚠️ 这是**派生**，不落 state —— 存成独立 state 会与两个来源脱节（换来源不跟进）。
 * 读取处走 {@link useProbePassive}（两来源任一变化即重算）。
 */
export function unionProbePassive(
  accounts: readonly { uid: string; modelCoolings?: Record<string, number> }[],
  ledger: Record<string, Record<string, ModelProbeState>>,
): Record<string, Record<string, ProbePassiveInfo>> {
  const out: Record<string, Record<string, ProbePassiveInfo>> = {};
  for (const uid of Object.keys(ledger)) {
    const models: Record<string, ProbePassiveInfo> = {};
    for (const [model, state] of Object.entries(ledger[uid])) {
      models[model] = { state, source: "ledger", both: false };
    }
    out[uid] = models;
  }
  const now = Date.now();
  for (const account of accounts) {
    for (const [model, untilMs] of Object.entries(account.modelCoolings ?? {})) {
      if (typeof untilMs !== "number" || untilMs <= now) continue;
      const bucket = (out[account.uid] ??= {});
      bucket[model] = {
        state: "rate_limited_model",
        source: "flow",
        // 台账里也有同一格 ⇒ 两来源都有（状态仍以真实流量为准）。
        both: bucket[model] !== undefined,
      };
    }
  }
  return out;
}

/**
 * **实测结论**（`uid → model → 格`），只取当前作用域。
 *
 * 账号页「客户端限流」与网关卡矩阵**共用**这一份：任一侧发起探测，
 * 另一侧下一次渲染读到的就是同一份（口径见 `lib/limit-link.ts`）。
 *
 * ⚠️ 返回**引用稳定**：未拉取过的 scope 回落到同一个空对象，否则每次渲染都换引用
 * ⇒ `useMemo` 全部失效、组件空转。
 */
export function useProbeCells(scope: Scope): Record<string, Record<string, ModelProbeCell>> {
  const index = useGatewayStore((s) => s.probeCells);
  return index[scope] ?? EMPTY_PROBE_SCOPE;
}

/**
 * **派生**的被动底色（卡片矩阵与模型表「限流（探测）」列**共用同一份**）。
 *
 * 订阅两个来源（`probePassive` 台账 + `status` 池冷却），任一变化即 `useMemo` 重算 ⇒
 * 二者**必然同时**跟着变，不会两处各拼一份、也不会与来源脱节。
 *
 * @param region 只取该区域池条目的冷却（Trae / Qoder 无 `modelCoolings` ⇒ 缺省空）。
 */
export function useProbePassive(region: Region): Record<string, Record<string, ProbePassiveInfo>> {
  const ledger = useGatewayStore((s) => s.probePassive);
  const status = useGatewayStore((s) => s.status);
  return useMemo(
    () => unionProbePassive(poolOf(status, region).accounts, ledger),
    [ledger, status, region],
  );
}
