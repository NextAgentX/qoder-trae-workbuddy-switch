import { useEffect, useState } from "react";

/**
 * 「这次加载已经超过 N 毫秒」—— 把「是不是卡死了」从**猜测**变成界面上**可读的事实**。
 *
 * ## 为什么需要它（issue #188 第四节）
 *
 * 本仓的区块加载态只有「正在加载（骨架）」与「加载完」两种，而失败分支只覆盖
 * 「请求**返回了错误**」。请求**既不返回也不报错**时，界面与「正在加载」一模一样，
 * 用户唯一能做的就是杀掉进程 —— 慢和死无法区分。
 *
 * 本 hook 不改变任何取数逻辑，只是在超时后提供一个布尔量，让调用点追加一行
 * 「仍在加载」的诚实表达（本仓拿不到进度百分比，所以不做进度条）。
 *
 * ## 判据
 *
 * - 只认 `active` 的**持续性**：`active` 变 false 立即归零（重试后重新计时）。
 * - 默认 8s：短于它的等待大多是正常慢请求，提前提示只会制造焦虑。
 * - 阈值留给调用点调（`TokenStatsPage` 一次拉整本流水，比设置页慢得多）。
 *
 * ⚠️ 阈值本身**不是**超时：本 hook 不会取消请求，也不该拿来判定失败。
 */
export function useSlowLoading(active: boolean, thresholdMs = 8000): boolean {
  const [slow, setSlow] = useState(false);

  useEffect(() => {
    if (!active) {
      setSlow(false);
      return;
    }
    const timer = window.setTimeout(() => setSlow(true), thresholdMs);
    return () => window.clearTimeout(timer);
  }, [active, thresholdMs]);

  return slow;
}
