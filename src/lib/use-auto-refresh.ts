import { useEffect, useRef } from "react";

import { useDocumentVisible } from "@/lib/use-document-visible";
import { CREDIT_REFRESH_INTERVAL_MS } from "@/lib/use-credit-auto-refresh";

/**
 * 「数据到期就重取一次」的共用节拍（issue #133）。
 *
 * ## 为什么抽出来
 *
 * 三平台的积分视图（`CreditStatsPage` / `TraeCreditsPage` / `QoderCreditsPage`）与
 * 账号页的卡片余额都要「每 10 分钟自动刷新 + 显示上次刷新时间」。各写一份 `setInterval`
 * 会立刻分叉：一处记得判可见性、一处忘了、一处把「上次刷新」写成相对时间永远不会走。
 *
 * ## 判据是**数据自己的时刻**，不是定时器的时刻
 *
 * 入参 `at` 是「这份数据最后一次**成功**取到的墙钟」（`0` = 还没成功过）。
 * - 定时器每 `intervalMs` 只做一件事：**拿 `at` 比一下**，还新鲜就什么都不发 ——
 *   手动刷新过之后不会紧接着又被自动刷新打一遍；
 * - `at` 变化会让 effect 重跑 ⇒ 定时器从「刚刷新完」重新计时，节奏天然对齐数据，
 *   不会出现「刚刷完 1 分钟，定时器又到点」。
 *
 * ## 可见性
 *
 * 隐藏窗口里的 `setInterval` 会被 WebView 节流，而且用户看不见结果 —— 白打上游。
 * 因此不可见时**不建定时器**；切回前台时若已过期先补一轮（那一轮本该在后台发生）。
 */
export function useAutoRefreshWhenVisible(
  at: number,
  refresh: () => void | Promise<unknown>,
  intervalMs: number = CREDIT_REFRESH_INTERVAL_MS,
) {
  const visible = useDocumentVisible();
  /* loader 每次渲染都是新身份（多半是 `useCallback` 带依赖的内联函数），放进 ref，
     否则 effect 会因为它换身份而重建定时器 —— 那等于「每次渲染都重置倒计时」。 */
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  useEffect(() => {
    if (!visible) return;
    /* `at === 0` 表示首屏那次还在飞（或第一次就失败了），不叠加第二个请求。 */
    if (at > 0 && Date.now() - at >= intervalMs) void refreshRef.current();
    const timer = window.setInterval(() => {
      void refreshRef.current();
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [visible, at, intervalMs]);
}
