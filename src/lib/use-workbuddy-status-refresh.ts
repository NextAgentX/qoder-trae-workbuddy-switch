import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import * as api from "@/lib/api";
import { useAccountsStore } from "@/stores/accounts";

export const WORKBUDDY_STATUS_REFRESH_INTERVAL_MS = 60 * 1000;

/**
 * 仅在主窗口可见且聚焦时刷新 WorkBuddy（国内版 + 国际版）运行状态与当前账号。
 *
 * 两个 region 都要轮询：`status.current` 同时决定「国际版/国内版」Tab 上的登录态文案
 * 与账号卡片上的「当前账号」标记，漏掉任何一个都会让标记停留在旧值。
 */
export function useWorkbuddyStatusRefresh() {
  const activeRef = useRef(false);
  const timerRef = useRef<number | undefined>(undefined);
  const abortControllerRef = useRef<AbortController | undefined>(undefined);

  useEffect(() => {
    let disposed = false;
    let documentVisible = document.visibilityState !== "hidden";
    const webui = api.isWebui();
    // Tauri emits the startup visibility decision before the WebView may have
    // installed this listener. Keep desktop inactive until isVisible() supplies
    // the authoritative initial value; later transitions arrive via the event.
    let mainWindowVisible = webui;
    let windowFocused = document.hasFocus();

    function stopTimer() {
      if (timerRef.current !== undefined) {
        window.clearInterval(timerRef.current);
        timerRef.current = undefined;
      }
    }

    function refreshStatus() {
      if (disposed || !activeRef.current) return;
      // 必须同时刷新国内版与国际版：`status.current` 是卡片「当前账号」标记的唯一来源，
      // 只轮询国内版会让国际版的状态永远停在首屏快照上——切换国际账号后标记不跟着走。
      void useAccountsStore.getState().refreshAllStatus(abortControllerRef.current?.signal);
      // ★ issue #171：`needsRelogin` 只存在于账号列表响应，光刷 status 会让后台清除
      //   标记后开着的页面徽章一直滞留。同 tick 补一次两 region 的账号列表
      //   （走 reconcile，不碰 loading，不会闪骨架屏）。
      void useAccountsStore.getState().reconcileAllAccounts();
    }

    function startTimer() {
      stopTimer();
      timerRef.current = window.setInterval(refreshStatus, WORKBUDDY_STATUS_REFRESH_INTERVAL_MS);
    }

    function syncActiveState() {
      const nextActive = documentVisible && mainWindowVisible && windowFocused;
      if (nextActive === activeRef.current) return;

      activeRef.current = nextActive;
      if (nextActive) {
        abortControllerRef.current = new AbortController();
        refreshStatus();
        startTimer();
      } else {
        abortControllerRef.current?.abort();
        abortControllerRef.current = undefined;
        stopTimer();
      }
    }

    const onVisibilityChange = () => {
      documentVisible = document.visibilityState !== "hidden";
      syncActiveState();
    };
    const onFocus = () => {
      windowFocused = true;
      syncActiveState();
    };
    const onBlur = () => {
      windowFocused = false;
      syncActiveState();
    };

    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    syncActiveState();

    let unlisten: (() => void) | undefined;
    if (!webui) {
      void (async () => {
        try {
          const fn = await listen<boolean>("main-window-visible", (event) => {
            mainWindowVisible = event.payload;
            syncActiveState();
          });
          if (disposed) {
            fn();
            return;
          }
          unlisten = fn;

          try {
            mainWindowVisible = await getCurrentWindow().isVisible();
          } catch {
            // Preserve the previous DOM-based behavior if the native query is
            // unavailable; focus is still required before polling can start.
            mainWindowVisible = documentVisible;
          }
          if (disposed) return;
          syncActiveState();
        } catch {
          if (!disposed) {
            mainWindowVisible = documentVisible;
            syncActiveState();
          }
        }
      })();
    }

    return () => {
      disposed = true;
      activeRef.current = false;
      abortControllerRef.current?.abort();
      abortControllerRef.current = undefined;
      stopTimer();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      unlisten?.();
    };
  }, []);
}
