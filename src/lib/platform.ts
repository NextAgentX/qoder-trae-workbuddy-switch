import * as api from "@/lib/api";

/**
 * 是否处于 **macOS 的统一标题栏（Overlay）** 形态 —— 从 `App.tsx` 收敛出的唯一一处判断。
 *
 * `tauri.conf.json` 里 macOS 用 `titleBarStyle: "Overlay"` + `trafficLightPosition{x:16,y:30}`，
 * 于是系统红绿灯（关闭 / 最小化 / 最大化）会**浮在应用自己的内容之上**。
 * 顶栏（L0/L1）是横向铺满的第一行，若不给左侧留出 ≥76px，红绿灯就会压住作用域选择器。
 *
 * ⚠️ **只对桌面端 + macOS 成立**：webui / 移动端没有覆盖式标题栏，Windows / Linux
 * 用的是原生标题栏（内容区从窗口客户区开始，不存在避让问题）。
 *
 * ⚠️ **浏览器 / jsdom 抓不到这个形态**（它们既没有 `api.isDesktop()`、也没有红绿灯），
 * 因此顶栏的内边距必须靠真机截图确认。
 */
export function isMacOverlayTitleBar(): boolean {
  return (
    api.isDesktop() &&
    typeof navigator !== "undefined" &&
    navigator.userAgent.includes("Macintosh")
  );
}