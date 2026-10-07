/**
 * 桌面悬浮窗（P9 · T04）的**可见性偏好** hook —— 侧栏图标 / 托盘菜单项 / 窗口内关闭按钮
 * 读写的**同一份**状态。
 *
 * ## 载体：L0 localStorage（§2.1 / §1.5）
 *
 * 「桌面悬浮窗可见性 → localStorage」，**键名与 scope 无关**（全局一个，P9 §1.5 / §4.3）。
 * 悬浮窗是 **app 级聚合**（不属于 L0–L3、跨 scope，✅ #4），它的可见性偏好因此也不该被
 * region 切分。默认 `true`（§3.16「默认显示」）。
 *
 * ## 单一真值：三处入口收敛到 `companion_set_visible`
 *
 * 可见性**只有**三个入口：侧栏左下角图标（本 hook 的消费者）、托盘菜单项、窗口内关闭按钮。
 * 它们**不是三个独立开关**，而是**同一状态的三面** —— 全部经 `companion_set_visible`
 * 收敛（P9 §1.5 / R8）。本 hook 通过 `listen("companion-visibility")` 与另两面同步
 * （沿用 `main-window-visible` 的既有事件模式），因此**不会出现第二个「看起来能切」的控件**。
 *
 * ## 启动按偏好落地（§1.2 / §4.5）
 *
 * 设计写的是「启动：按偏好决定是否 ensure()（**默认显示**）」。偏好存在**前端**
 * （localStorage），Rust 无从读取 ⇒ 由本 hook 在挂载时把它**应用到真实窗口**：
 * 偏好 `true` ⇒ 启动即显示；偏好 `false`（用户上次关掉）⇒ 保持隐藏。
 * 这既是「默认显示」的落地，也是「记住用户选择」。
 *
 * ## webui 安全
 *
 * 悬浮窗是**桌面端**的独立窗口。webui / 移动端没有它 ⇒ 本 hook 在非桌面端
 * **不触碰** Tauri 窗口 API（既不 `listen` 也不调命令），只维护 localStorage 偏好，
 * 因此可以安全地挂在侧栏上（与 `use-document-visible` 同一处理）。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { listen } from "@tauri-apps/api/event";

import * as api from "@/lib/api";

/**
 * 可见性偏好的 localStorage 键。
 *
 * **全局一个**、与 scope 无关（P9 §1.5 / §2.1）—— 悬浮窗是 app 级聚合，不属于任何 region。
 * 命名与既有偏好键（`buddy-switch.language` / `buddy-switch.theme`）同一风格。
 */
const COMPANION_VISIBILITY_KEY = "buddy-switch.companion-visible";

/**
 * 可见性事件名（与 `src-tauri/src/companion.rs` 的 `VISIBILITY_EVENT` 逐字一致）。
 *
 * 托盘菜单项 / 窗口内关闭按钮改可见性时都会 emit 它，主窗口据此同步侧栏图标的状态。
 */
const COMPANION_VISIBILITY_EVENT = "companion-visibility";

/** 读取持久化的可见性偏好；**默认 `true`**（§3.16「默认显示」）。 */
function readCompanionVisibilityPreference(): boolean {
  if (typeof window === "undefined") return true;
  try {
    const raw = window.localStorage.getItem(COMPANION_VISIBILITY_KEY);
    // 未设置 ⇒ 默认显示；只有显式写入过 `"false"` 才判为隐藏。
    return raw === null ? true : raw === "true";
  } catch {
    // 隐私模式 / 存储被禁用：退回默认（**不抛** —— 偏好是锦上添花，不该让侧栏崩）。
    return true;
  }
}

/** 持久化可见性偏好（写失败静默 —— 本次会话内仍按内存状态工作）。 */
function writeCompanionVisibilityPreference(visible: boolean): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(COMPANION_VISIBILITY_KEY, visible ? "true" : "false");
  } catch {
    /* 存储不可用：忽略 */
  }
}

/**
 * 订阅 / 控制悬浮窗可见性，返回 `[visible, setVisible]`。
 *
 * - `visible`：当前可见性（读的是**与托盘 / 关闭按钮同一份**状态）。
 * - `setVisible(next)`：乐观更新 + 持久化 + 经 `companion_set_visible` 收敛到唯一真值；
 *   调用失败（webui / 演示模式 / 后端异常）时**回滚** —— 图标不能撒谎，否则它就是
 *   一个「看起来能切」的假控件（P9 §1.5）。
 */
export function useCompanionVisibility(): [boolean, (next: boolean) => void] {
  const [visible, setVisibleState] = useState<boolean>(readCompanionVisibilityPreference);
  // 用 ref 追踪最近一次已知可见性：事件回调与 setter 都读它，避免闭包里的过期值。
  const visibleRef = useRef<boolean>(visible);
  visibleRef.current = visible;

  useEffect(() => {
    // webui / 移动端没有悬浮窗：不碰 Tauri 窗口 API（`listen` / 命令都可能抛）。
    if (!api.isDesktop()) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;

    // ① 启动按偏好落地：把 localStorage 里的偏好应用到真实窗口（幂等，StrictMode 双挂载无害）。
    //    这是「默认显示」真正生效的一步 —— 只有前端知道偏好，Rust 无从读取（§1.2 / §4.5）。
    void api.companionSetVisible(readCompanionVisibilityPreference()).catch(() => {
      /* 后端未就绪（冷启动 / 演示模式）不致命：窗口显示与否由下次交互决定 */
    });

    // ② 与另两面同步：托盘 / 窗口内关闭按钮改可见性时，把**真实状态**同步进来并写回偏好。
    void listen<boolean>(COMPANION_VISIBILITY_EVENT, (event) => {
      if (disposed) return;
      visibleRef.current = event.payload;
      setVisibleState(event.payload);
      writeCompanionVisibilityPreference(event.payload);
    })
      .then((fn) => {
        // 订阅在卸载后才返回 ⇒ 立刻退订，避免泄漏（侧栏会随产品切换反复挂载）。
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        /* 非 Tauri 环境 / 无权限：侧栏图标仍可用，只是不再被动同步 */
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const setVisible = useCallback((next: boolean) => {
    const previous = visibleRef.current;
    // 乐观更新 + 持久化：即使命令在途，图标也立即反映用户意图。
    visibleRef.current = next;
    setVisibleState(next);
    writeCompanionVisibilityPreference(next);

    void api.companionSetVisible(next).catch(() => {
      // 命令失败（webui / 演示模式 / 后端异常）⇒ 回滚到调用前的状态。
      // ★ 不回滚的话，图标会停在一个**后端并未生效**的值上 —— 那就是「看起来能切」的假控件。
      visibleRef.current = previous;
      setVisibleState(previous);
      writeCompanionVisibilityPreference(previous);
    });
  }, []);

  return [visible, setVisible];
}
