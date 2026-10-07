/**
 * 桌面悬浮窗（P9）的**只读**数据层：类型 + `useCompanionStatus()`。
 *
 * ## 为什么走专用只读命令，而不是 accounts store
 *
 * 账号 store 是**按 region 分家**的账号 / 积分域；悬浮窗展示的是**跨来源的
 * 「会话 / 任务状态」**（运行中 / 待确认 / 已完成 / 失败），**不是账号列表**，也
 * **不属于任何单一 scope**（`PLAN §3.16`）。故数据来自专用只读命令 `companion_status`
 * （app 级聚合，无 scope 参数）。
 *
 * ## 为什么刷新全在前端 webview（★ 不新增 Rust 后台循环）
 *
 * 项目硬约束「**登记即执行**」：新增任何「写状态 / 周期性」出口都要先登记进
 * `BackgroundTask` 注册表，漏登记 ⇒ 任务**永不执行**（这类事故本项目已出过三次）。
 * 因此刷新**全部**发生在这个 webview 内 —— 20s 轮询 + 订阅**既有**进度事件即时刷新，
 * 从而**不触碰** `BackgroundTask` 注册表，把 R9 的风险归零（P9 §1.3 / R9）。
 *
 * ## 四态由数据决定，前端不做业务判断
 *
 * `CompanionItem.state` 是唯一判据：前端只按字段渲染 class / 文案 / 徽标，
 * **不自己推断**「什么算运行中」（P9 §1.4）。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { listen } from "@tauri-apps/api/event";

import { asError, companionStatus } from "./api";

/** 会话 / 任务四态（与后端 `CompanionState` 同源；序列化为 **lowercase**）。 */
export type CompanionState = "running" | "waiting" | "done" | "failed";

/** 悬浮窗的一行（只读）。 */
export interface CompanionItem {
  /** 稳定键（聚焦 / 去重用）。 */
  id: string;
  /** 来源客户端名。 */
  source: string;
  /**
   * 会话 / 任务标题。
   *
   * ⚠️ **敏感字段**：T03 渲染时**默认不进 DOM**（未悬停 / 未聚焦渲染占位符，
   * 悬停 / `:focus-visible` 才展开），见 P9 §1.6 / ✅ #3。
   */
  title: string;
  /** 四态之一。 */
  state: CompanionState;
  /** 相对时间 / 提示。 */
  hint: string;
}

/** 悬浮窗快照（只读）。 */
export interface CompanionSnapshot {
  /** 行列表；无数据源时为空。 */
  items: CompanionItem[];
  /** 数据来源状态：`"live"` | `"cached"` | `"unavailable"`。 */
  source: string;
  /** 快照生成时间（ms）。 */
  updated_at: number;
}

/**
 * 低频轮询间隔（P9 §1.3）。
 *
 * 20s：足够让「运行中 / 待确认」的状态跟得上，又不至于把只读命令变成热路径。
 * 真正的即时性靠下面的**既有事件**订阅，轮询只是兜底。
 */
export const COMPANION_POLL_INTERVAL_MS = 20_000;

/**
 * 既有进度事件：命中即**立即**再拉一次。
 *
 * 与后端 provider 的**数据源同源**（都由这些操作推导会话 / 任务状态）—— 事件一到就刷新，
 * 用户不必等下一次轮询。这些事件名与 `commands.rs` 的 emit 点逐字对齐。
 */
export const COMPANION_REFRESH_EVENTS = [
  "switch-progress",
  "checkin-progress",
  "trae-checkin-progress",
] as const;

/** 可见性事件：窗口**重新可见**时立即拉一次（关闭 = 隐藏，重新打开要看到最新状态）。 */
const COMPANION_VISIBILITY_EVENT = "companion-visibility";

/** `useCompanionStatus()` 的返回形状。 */
export interface CompanionStatusState {
  /** 最近一次成功快照；首帧为 `null`。 */
  snapshot: CompanionSnapshot | null;
  /** 首次加载中（T03 据此渲染骨架）。 */
  loading: boolean;
  /** 最近一次失败原因（已本地化）；成功一次即清空。 */
  error: string | null;
  /**
   * 手动刷新（**只读**）—— 供空态的「刷新」出口调用，与轮询走**同一条**只读路径。
   *
   * 悬浮窗没有任何写操作，故这是唯一的用户可触发动作（另两个是显隐与聚焦）。
   */
  refresh: () => Promise<void>;
}

/**
 * 订阅悬浮窗只读快照：挂载拉一次 + 20s 轮询 + 既有事件即时刷新 + 重新可见时刷新。
 *
 * 卸载时**一定**清掉定时器与全部事件订阅（悬浮窗会被反复显示 / 隐藏，泄漏会累积）。
 */
export function useCompanionStatus(): CompanionStatusState {
  const [snapshot, setSnapshot] = useState<CompanionSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const disposedRef = useRef(false);

  // 抽成 `useCallback`（而非留在 effect 内）：空态出口需要能手动触发**同一条**刷新路径。
  const refresh = useCallback(async () => {
    try {
      const next = await companionStatus();
      if (disposedRef.current) return;
      setSnapshot(next);
      setError(null);
    } catch (cause) {
      if (disposedRef.current) return;
      setError(asError(cause));
    } finally {
      if (!disposedRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    disposedRef.current = false;

    // ① 首次挂载拉一次。
    void refresh();
    // ② 低频轮询（★ 前端轮询，**不是** Rust 后台循环）。
    const timer = window.setInterval(() => void refresh(), COMPANION_POLL_INTERVAL_MS);

    // ③ 既有事件驱动 + 窗口重新可见 ⇒ 立即再拉一次。
    const unlisteners: Array<() => void> = [];
    void (async () => {
      try {
        for (const event of COMPANION_REFRESH_EVENTS) {
          const unlisten = await listen(event, () => void refresh());
          if (disposedRef.current) {
            unlisten();
            return;
          }
          unlisteners.push(unlisten);
        }
        const onVisibility = await listen<boolean>(COMPANION_VISIBILITY_EVENT, (event) => {
          if (event.payload) void refresh();
        });
        if (disposedRef.current) {
          onVisibility();
          return;
        }
        unlisteners.push(onVisibility);
      } catch {
        // 事件订阅失败（非 Tauri 环境 / 无权限）**不**致命：轮询仍会刷新。
      }
    })();

    return () => {
      disposedRef.current = true;
      window.clearInterval(timer);
      for (const unlisten of unlisteners) unlisten();
    };
  }, [refresh]);

  return { snapshot, loading, error, refresh };
}
