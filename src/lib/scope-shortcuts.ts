/**
 * 作用域快捷键的**纯规划层** —— P6 / PLAN §3.1。
 *
 * ## 规则（来自 PLAN §3.1）
 *
 * 按**作用域在注册表中的序号**动态生成 `Ctrl/Cmd + 1..N`（N = 作用域总数）；**超过 5 个**
 * 降级为不绑定；用户可**关闭**。三条规则收敛到一个纯函数 {@link planScopeShortcuts}，
 * 于是「绑定」与「降级」有了**唯一判据**。
 *
 * ## ★ 绑定与「序号提示」同源（§3.4 规则 1）
 *
 * 面板「作用域」组的 `⌘N` 提示、首次引导里的快捷键、设置项说明 —— **只能**在
 * `bindings` 非空时出现。调用方统一读 `bindings`（`PaletteContext.scopeShortcutIndex`
 * 就是它），降级时空 Map ⇒ 提示**自动消失**。
 *
 * 反面案例是「显示了 ⌘3、按了却没反应」的**假控件** —— 它比不显示更糟：用户会以为
 * 应用坏了。因此降级必须**不绑定 + 不提示**，两件事同时发生。
 *
 * ## ⚠️ 降级分支当前**永不触发**（真实应用只有 4 个作用域）
 *
 * `SCOPE_SHORTCUT_LIMIT = 5`，而 `SCOPES` 只有 4 个（`wb-cn` / `wb-global` / `trae-cn` /
 * `trae-global`）⇒ `reason="too-many"` 这条分支在真实应用里**走不到**。
 * 保留它是**防御性**的（将来加平台/区域即生效），但**必须有断言模拟 ≥6 个作用域**验证它 ——
 * 否则它是**永远不执行的死代码**，而「永不执行」和「正确」在测试里长得一样。
 * 见 `scripts/check-scope-registry.cjs` 的 P6 降级断言。
 */

import type { Scope } from "@/lib/scope";
import { DESTINATIONS, type Destination } from "@/lib/platforms/registry";

/** 超过此数量即降级为不绑定（PLAN §3.1）。 */
export const SCOPE_SHORTCUT_LIMIT = 5;

/** 快捷键规划结果。 */
export interface ScopeShortcutPlan {
  /** 绑定：scope → 序号（1..N）。降级时为空 Map。 */
  bindings: ReadonlyMap<Scope, number>;
  degraded: boolean;
  /** 降级原因（供 UI 解释）；未降级为 `null`。 */
  reason: "too-many" | "user-disabled" | null;
}

/**
 * 规划作用域快捷键。**唯一判据**（面板 / 引导 / 设置项都读它的结果）。
 *
 * 优先级：用户关闭 > 数量超限 > 正常绑定。两个降级原因**分列**（`user-disabled` /
 * `too-many`）—— 设置项据此决定「显示可关闭的开关」还是「改为一句说明」：数量超限时
 * 没有「可开启之物」，给开关反而是假控件。
 */
export function planScopeShortcuts(input: {
  scopes: readonly Scope[];
  /** 用户偏好（§3.1「可关闭」）。 */
  enabled: boolean;
}): ScopeShortcutPlan {
  if (!input.enabled) {
    return { bindings: new Map(), degraded: true, reason: "user-disabled" };
  }
  if (input.scopes.length > SCOPE_SHORTCUT_LIMIT) {
    return { bindings: new Map(), degraded: true, reason: "too-many" };
  }
  const bindings = new Map<Scope, number>();
  input.scopes.forEach((scope, index) => bindings.set(scope, index + 1));
  return { bindings, degraded: false, reason: null };
}

/**
 * 快捷键目标 URL：`/s/{scope}/{dest}`。
 *
 * 当前目的地缺失或非法（旧路由、根路径）⇒ **回落 `accounts`** —— 快捷键是纯加速器，
 * 不该因「当前页面没有对应的新路由」而按了没反应。
 *
 * ⚠️ 只返回路径，**不做导航**：导航由调用方（`App.tsx` 的全局键）执行，
 * 本函数保持纯净、可门禁。
 */
export function scopeShortcutTarget(scope: Scope, currentDest: string | null | undefined): string {
  const dest = (DESTINATIONS as readonly string[]).includes(currentDest ?? "")
    ? (currentDest as Destination)
    : "accounts";
  return `/s/${scope}/${dest}`;
}
