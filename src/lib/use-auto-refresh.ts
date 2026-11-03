import { useEffect, useRef } from "react";

import { useDocumentVisible } from "@/lib/use-document-visible";
import {
  CREDITS_UPSTREAM_REFRESH_INTERVAL_MS,
  jitteredInterval,
} from "@/lib/use-credit-auto-refresh";

/**
 * 「数据到期就重取一次」的共用节拍（issue #133）。
 *
 * ## 为什么抽出来
 *
 * 三平台的积分视图（`CreditStatsPage` / `TraeCreditsPage` / `QoderCreditsPage`）与
 * 账号页的卡片余额都要「每 N 分钟自动刷新 + 显示上次刷新时间」。各写一份 `setInterval`
 * 会立刻分叉：一处记得判可见性、一处忘了、一处把「上次刷新」写成相对时间永远不会走。
 *
 * ## 判据是**数据自己的时刻**，不是定时器的时刻
 *
 * 入参 `at` 是「这份数据最后一次**成功**取到的墙钟」（`0` = 还没成功过）。
 * - 定时器到点只做一件事：**发一次**，然后重排下一轮 —— 是否该发由 `at` 决定，
 *   手动刷新过之后不会紧接着又被自动刷新打一遍；
 * - `at` 变化会让 effect 重跑 ⇒ 倒计时从「刚刷新完」重新算，节奏天然对齐数据。
 *
 * ## 抖动（issue #133 复核 · 方案 A）
 *
 * 原先用 `setInterval`，节拍是**确定性的**：代码里看得见「每 10 分钟整点发一次」。
 * 现在每轮重新掷一次 ±20% 的间隔抖动 —— 抖动作用于**间隔**，不是钟点
 * （`schedule.rs` 的 `next_fire_with_jitter` 服务的是「每天几点触发」，语义不同，别套）。
 *
 * 只抖间隔**不够**：所有实例仍会「一起开始 + 固定偏移」地同拍。所以首轮延迟也要打散
 * （见下面的 `firstDelay` 三条分支）。`at === 0` 时把相位随机落到 `[0, 间隔)` 内，
 * 这是唯一能把锚点错开的地方。
 *
 * ## 可见性
 *
 * 隐藏窗口里的定时器会被 WebView 节流，而且用户看不见结果 —— 白打上游。
 * 因此不可见时**不建定时器**；切回前台时若已过期先补一轮（那一轮本该在后台发生）。
 */
export function useAutoRefreshWhenVisible(
  at: number,
  refresh: () => void | Promise<unknown>,
  intervalMs: number = CREDITS_UPSTREAM_REFRESH_INTERVAL_MS,
  /**
   * 是否允许自动刷新（默认允许）。
   *
   * ★ 「打上游」的视图要把它接到 {@link useCreditRefreshPref}（默认关，opt-in）——
   * 见 `credit-refresh-pref.ts` 文件头。**只读本地快照**的视图不必传（永远允许）。
   */
  enabled: boolean = true,
) {
  const visible = useDocumentVisible();
  /* loader 每次渲染都是新身份（多半是 `useCallback` 带依赖的内联函数），放进 ref，
     否则 effect 会因为它换身份而重建定时器 —— 那等于「每次渲染都重置倒计时」。 */
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  useEffect(() => {
    if (!visible || !enabled) return;
    const age = at > 0 ? Date.now() - at : null;

    /* 首轮延迟三条分支（顺序即优先级）：
       - 还没成功取过（`at === 0`）⇒ 相位随机落在 `[间隔/2, 间隔)` 内。首屏那一发由各页面
         自己的 `load()` 造成，天然同时发出 —— 只抖间隔的话，大家只是「一起开始的固定偏移」。
         ⚠️ 下界取一半而不是 0：首屏请求还在飞的那几百毫秒里不能叠加第二个请求
         （旧实现直接跳过这一轮，代价是「首屏失败就永远不自动重试」）。
       - 已过期 ⇒ **立刻**补一轮，不为了抖而拖延（用户正看着一份旧数据）；
       - 还新鲜 ⇒ 等到 `at + 间隔` 才发，让刚手动刷过的数据不被紧接着再打一遍。 */
    const firstDelay =
      age === null
        ? intervalMs / 2 + Math.random() * (intervalMs / 2)
        : age >= intervalMs
          ? 0
          : intervalMs - age;

    let timer = 0;
    const schedule = (delayMs: number) => {
      timer = window.setTimeout(() => {
        void refreshRef.current();
        /* 之后每轮重新掷：抖动落在间隔上，跨轮不累积成固定节拍。 */
        schedule(jitteredInterval(intervalMs));
      }, delayMs);
    };
    schedule(firstDelay);
    return () => window.clearTimeout(timer);
  }, [visible, at, intervalMs, enabled]);
}
