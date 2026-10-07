import { useEffect, useState } from "react";

/**
 * 媒体查询 hook（`P4-TOOLBAR-ARCH §3.12`）。
 *
 * ## 为什么要有它（而不是各处写死断点常量）
 *
 * 响应式规则「< 900px 强制卡片视图」只在**一处**判断：表格在此宽度不可读。
 * 若把 `900` 这个常量散落到页面 / 组件里，改断点时必然漏改一处，而漏改的症状
 * 是「某个宽度下表格挤成一团」这类**只在特定窗口尺寸下复现**的问题。
 * 抽成 hook 后，「窄窗 ⇒ 强制卡片」的判据只有一份。
 *
 * ## 初值与 SSR
 *
 * 无 `window` / 无 `matchMedia`（SSR、极端环境）时返回 `true`（= 宽屏可用表格）：
 * 宁可多给一个能力，也不要在未知环境里把功能**静默关掉**。
 */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState<boolean>(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return true;
    return window.matchMedia(query).matches;
  });

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const mediaQueryList = window.matchMedia(query);
    const handleChange = () => setMatches(mediaQueryList.matches);
    // 立即对齐一次：`query` 变化（或首帧与真实环境有偏差）时补正。
    handleChange();
    mediaQueryList.addEventListener("change", handleChange);
    return () => mediaQueryList.removeEventListener("change", handleChange);
  }, [query]);

  return matches;
}
