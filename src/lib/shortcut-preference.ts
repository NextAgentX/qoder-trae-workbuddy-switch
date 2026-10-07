/**
 * 「作用域快捷键可关闭」偏好 —— P6 / PLAN §3.1。
 *
 * ## 为什么是一个外部 store（而不是各组件各自 `useState`）
 *
 * 这个偏好有**两个消费者**：`App.tsx` 的全局键（决定 `planScopeShortcuts` 的 `enabled`）
 * 与设置页的开关（决定勾选态）。若各自 `useState`，在设置里关掉后**全局键不会立即失效**
 * —— 正是「两个控件各说各话」。因此收敛成一个模块级 store + `useSyncExternalStore`
 * （与 `lib/i18n.ts` / `lib/theme.ts` 同构：localStorage + 启动读一次 + 订阅通知）。
 *
 * ## 默认「开」
 *
 * 缺失键 ⇒ `true`。快捷键是**加速器**，默认给出来才有价值；不想要的人可显式关闭。
 *
 * ## 读取 / 写入是纯函数
 *
 * `readShortcutsEnabled` / `writeShortcutsEnabled` 接收 `storage` 入参（可传桩），
 * 因此可被门禁直接断言 —— 前端没有单测框架。
 */

import { useCallback, useSyncExternalStore } from "react";

/** `localStorage` 键。`v1` 前缀：将来语义若变可换键而不破坏旧值（同 `scope.ts` 惯例）。 */
export const SHORTCUTS_ENABLED_KEY = "buddy-switch:v1:shortcuts-enabled";

/**
 * 读取偏好。**缺失 ⇒ `true`**（默认开）；存储不可用或抛异常也 ⇒ `true`。
 *
 * 静默吞掉 `localStorage` 异常：隐私模式 / 存储被禁时不该让应用崩掉，
 * 只是「记不住这个偏好」而已。
 */
export function readShortcutsEnabled(storage: Pick<Storage, "getItem"> | undefined): boolean {
  try {
    const raw = storage?.getItem(SHORTCUTS_ENABLED_KEY);
    if (raw === "0" || raw === "false") return false;
    return true;
  } catch {
    return true;
  }
}

/** 写入偏好。**写失败不抛**（记不住不影响功能）。 */
export function writeShortcutsEnabled(
  storage: Pick<Storage, "setItem"> | undefined,
  enabled: boolean,
): void {
  try {
    storage?.setItem(SHORTCUTS_ENABLED_KEY, enabled ? "1" : "0");
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
let current: boolean | null = null;
const listeners = new Set<() => void>();

function snapshot(): boolean {
  if (current === null) current = readShortcutsEnabled(safeStorage());
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 设置偏好：持久化 + 更新缓存 + 通知订阅者（全局键与设置开关一起重渲染）。 */
export function setShortcutsEnabled(enabled: boolean): void {
  writeShortcutsEnabled(safeStorage(), enabled);
  current = enabled;
  for (const listener of listeners) listener();
}

/**
 * 订阅偏好。返回 `[enabled, setEnabled]`。
 *
 * 服务端快照（`getServerSnapshot`）固定 `true`：本应用是纯客户端，此分支不会被用到，
 * 但给出确定值可避免水合不一致。
 */
export function useShortcutsEnabled(): [boolean, (value: boolean) => void] {
  const enabled = useSyncExternalStore(subscribe, snapshot, () => true);
  const set = useCallback((value: boolean) => setShortcutsEnabled(value), []);
  return [enabled, set];
}
