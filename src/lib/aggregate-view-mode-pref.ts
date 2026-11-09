/**
 * 聚合账号面板的**视图模式偏好**（分组 / 平铺）—— issue #50 的补充。
 *
 * ## 为什么记在 `localStorage`
 *
 * 判据同 `aggregate-visibility-pref.ts` / `drawer-tab-preference.ts`：只有渲染层读它
 * （聚合页决定按作用域分组还是按紧迫度平铺），宿主（Rust）、托盘、悬浮窗一辈子不看。
 * 留在渲染层 ⇒ 同步可得、零 IPC；写进 `app_prefs.json` 反而要为它加一对命令 + 四处接线。
 *
 * 也**不登记**进 `app-settings-snapshot.ts` 的 `APP_SETTINGS_STORAGE_KEYS`：那份清单
 * 只收「语言 / 外观 / 上次作用域」三个跨宿主有意义的键（同 `aggregate-visible`）。
 *
 * ## 默认「分组」
 *
 * 缺失键 / 值非法 ⇒ `group`。分组是信息量更大的默认视图（组标题承担归属），
 * 平铺是「按紧迫度混排」的另一种看法，属于用户主动选择。
 *
 * ## 读写是纯函数
 *
 * 接收 `storage` 入参（可传桩）⇒ 能被 `scripts/check-scope-registry.cjs` 直接
 * 跑真代码断言（前端没有单测框架，同仓内其它偏好模块的既有理由）。
 */

import { useCallback, useSyncExternalStore } from "react";

/** 两种视图模式的稳定 id（与工具栏按钮的判定逐字一致）。 */
export const AGGREGATE_VIEW_MODES = ["group", "flat"] as const;

export type AggregateMode = (typeof AGGREGATE_VIEW_MODES)[number];

/** 缺省模式：也是「没有记住」时的回落目标。 */
export const DEFAULT_AGGREGATE_MODE: AggregateMode = "group";

/** `localStorage` 键。`v1` 前缀沿用仓内惯例（语义若变可换键而不破坏旧值）。 */
export const AGGREGATE_VIEW_MODE_KEY = "buddy-switch:v1:aggregate-view-mode";

/** 收窄到合法模式（存储里可能是任意旧值 / 手改值）。 */
export function isAggregateMode(value: unknown): value is AggregateMode {
  return typeof value === "string" && (AGGREGATE_VIEW_MODES as readonly string[]).includes(value);
}

/**
 * 读取记住的模式。
 *
 * 缺失 / 非法值 / 存储抛异常 ⇒ **默认 `group`**（不因偏好损坏而让页面出错）。
 */
export function readAggregateMode(
  storage: Pick<Storage, "getItem"> | undefined,
): AggregateMode {
  try {
    const raw = storage?.getItem(AGGREGATE_VIEW_MODE_KEY);
    return isAggregateMode(raw) ? raw : DEFAULT_AGGREGATE_MODE;
  } catch {
    return DEFAULT_AGGREGATE_MODE;
  }
}

/** 写入模式。**写失败不抛**（本次会话内仍按内存状态工作，只是记不住）。 */
export function writeAggregateMode(
  storage: Pick<Storage, "setItem"> | undefined,
  mode: AggregateMode,
): void {
  try {
    storage?.setItem(AGGREGATE_VIEW_MODE_KEY, mode);
  } catch {
    /* 忽略：存储不可用时只是记不住 */
  }
}

/** 取可用的 `localStorage`（不可用返回 `undefined`）。 */
function safeStorage(): Storage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

/** 模块内缓存 —— 保证 `useSyncExternalStore` 的快照在两次渲染间稳定。 */
let current: AggregateMode | undefined;
const listeners = new Set<() => void>();

function snapshot(): AggregateMode {
  if (current === undefined) current = readAggregateMode(safeStorage());
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 设置模式：持久化 + 更新缓存 + 通知订阅者。 */
export function setAggregateMode(mode: AggregateMode): void {
  writeAggregateMode(safeStorage(), mode);
  current = mode;
  for (const listener of listeners) listener();
}

/**
 * 订阅模式。返回 `[mode, setMode]`。
 *
 * `getServerSnapshot` 固定默认值：本应用是纯客户端，此分支不会被用到，
 * 但给出确定值可避免水合不一致（同 `useAggregateVisible`）。
 */
export function useAggregateMode(): [AggregateMode, (mode: AggregateMode) => void] {
  const mode = useSyncExternalStore(subscribe, snapshot, () => DEFAULT_AGGREGATE_MODE);
  const set = useCallback((next: AggregateMode) => setAggregateMode(next), []);
  return [mode, set];
}

/**
 * 跨窗口 / 跨标签页同步（webui 可能开多个标签）。
 *
 * 与 `subscribeAggregateVisibleStorage` 同由：别的标签切了视图模式，这里要跟着变，
 * 否则两个标签对同一份数据呈现不同的看法、而用户不知道为什么。
 *
 * @returns 取消订阅函数（`useEffect` 里直接 return）
 */
export function subscribeAggregateModeStorage(): () => void {
  if (typeof window === "undefined") return () => undefined;
  /* ★ 注册时先重读一次：本函数由聚合页的 `useEffect` 调用，页面**离开期间**
     监听器已被移除，那时别的标签改过值也不会有 `storage` 事件补发到这里。
     不重读 ⇒ 回到聚合页会一直显示陈旧模式，直到下一次别的事件。
     （`useSyncExternalStore` 的订阅在组件 effect 里先注册，这里通知得到它。）
     ⚠️ **只在盘上确实有值时才清缓存**：`readAggregateMode` 对「读失败」和「没有值」
     都返回 `group`，无条件重读会把「存储不可用 + 用户选了平铺」的内存状态重置回
     分组 —— 而那种情况下写盘早已静默失败，内存是唯一真值。
     代价：别的标签**删掉**该键时本标签不同步。本应用没有删键入口（切换只会 `setItem`），
     代价为零；真加了「重置偏好」再改成监听 `event.newValue === null`。 */
  let stored: string | null | undefined;
  try {
    stored = safeStorage()?.getItem(AGGREGATE_VIEW_MODE_KEY);
  } catch {
    stored = null; // 读不到 ⇒ 保留内存值
  }
  if (stored != null) {
    current = undefined;
    for (const listener of listeners) listener();
  }
  const onStorage = (event: StorageEvent) => {
    if (event.key !== AGGREGATE_VIEW_MODE_KEY) return;
    /* 丢掉本进程缓存，让下一次 `snapshot()` 重读 —— 不直接信任 `event.newValue`，
       那条路径绕过 `readAggregateMode` 的容错（同 aggregate-visible 的处理）。
       这里可以无条件清：能收到事件就说明存储本身可用。 */
    current = undefined;
    for (const listener of listeners) listener();
  };
  window.addEventListener("storage", onStorage);
  return () => window.removeEventListener("storage", onStorage);
}
