import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { CompanionApp } from "./components/companion/companion-app";
import { ErrorBoundary } from "./components/error-boundary";
import { initLocale } from "./lib/i18n";
import { applyTheme, getThemePreference, watchSystemTheme } from "./lib/theme";

applyTheme(getThemePreference());
initLocale();
const stopWatchingSystemTheme = watchSystemTheme();
if (import.meta.hot) import.meta.hot.dispose(stopWatchingSystemTheme);

// A browser tab can keep an old Vite entry open across a WebUI rebuild.
// Check the served HTML before refreshing so network failures still reach the error boundary.
if (import.meta.env.PROD && /^https?:$/.test(window.location.protocol)) {
  const runningEntry = import.meta.url;
  let checkingEntry = false;
  window.addEventListener("vite:preloadError", async () => {
    if (checkingEntry) return;
    checkingEntry = true;
    try {
      const response = await fetch(window.location.href, { cache: "no-store" });
      if (!response.ok) return;
      const document = new DOMParser().parseFromString(await response.text(), "text/html");
      const liveScript = document.querySelector<HTMLScriptElement>('script[type="module"][src]');
      const liveEntry = liveScript?.getAttribute("src");
      if (!liveEntry || new URL(liveEntry, response.url).href === runningEntry) return;
      const retryKey = "buddy-switch:entry-reload";
      if (sessionStorage.getItem(retryKey) === runningEntry) return;
      sessionStorage.setItem(retryKey, runningEntry);
      window.location.reload();
    } catch {
      // Leave the original error and the manual reload button available.
    } finally {
      checkingEntry = false;
    }
  });
}

/**
 * 入口分流：桌面悬浮窗是**独立窗口**（`companion.rs` 用 `?window=companion` 打开），
 * 与主窗口共用同一份 `index.html`。这里按 URL 查询把它分流到 `CompanionApp`。
 *
 * ⚠️ **不能用 `getCurrentWindow().label`**：`main.tsx` 在 webui 也会执行，那里没有
 * Tauri 窗口 API，引入 `@tauri-apps/api` 会让 webui 崩。URL 查询两端都安全。
 */
const isCompanionWindow =
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).get("window") === "companion";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
        {/* 错误边界必须包住两者：React 在未捕获的渲染错误上会卸载整棵树，
            而桌面端没有控制台可看 —— 没有它，任何一处渲染异常都表现为「一片白」。 */}
    <ErrorBoundary>
      {isCompanionWindow ? <CompanionApp /> : <App />}
    </ErrorBoundary>
  </React.StrictMode>,
);
