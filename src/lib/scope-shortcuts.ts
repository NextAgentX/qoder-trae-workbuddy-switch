/**
 * 作用域快捷键的**纯规划层** —— P6 / PLAN §3.1。
 *
 * ## 规则（来自 PLAN §3.1，v2.7 修订）
 *
 * 按**作用域在注册表中的序号**动态生成 `Ctrl/Cmd + 1..N`（N = 作用域总数）；**超过 9 个**
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
 * ## ⚠️ 降级分支当前**仍走不到**（真实应用 6 个作用域 ≤ 上限 9）
 *
 * `SCOPE_SHORTCUT_LIMIT = 9`，而 `SCOPES` 有 6 个（3 平台 × 2 区域）⇒ `reason="too-many"`
 * 这条分支在真实应用里**走不到**。保留它是**防御性**的（继续加平台/区域即生效），
 * 但**必须有断言模拟 >9 个作用域**验证它 —— 否则它是**永远不执行的死代码**，
 * 而「永不执行」和「正确」在测试里长得一样。
 * 见 `scripts/check-scope-registry.cjs` 的 P6 降级断言。
 *
 * ### 为什么上限是 9 而不是 5（v2.7 修订的背景）
 *
 * Qoder 集成让作用域从 4 个变成 **6 个**（3 平台 × 2 区域）。若沿用旧上限 5，
 * 「加一个平台」会把**既有用户已经在用的** `⌘1..⌘4` 一起关掉 —— 那是功能倒退，
 * 不是降级。数字键的**天然范围**本就是 `1..9`（`App.tsx` 的全局键同样按 `[1-9]` 匹配），
 * 故上限取 9：6 个照常绑定 `⌘1..⌘6`，>9 才降级。
 */

import type { Scope } from "@/lib/scope";
import { DESTINATIONS, type Destination } from "@/lib/platforms/registry";

/**
 * 超过此数量即降级为不绑定（PLAN §3.1，v2.7 修订：5 → 9）。
 *
 * 取 9 = 数字键的天然范围（`⌘1..⌘9`）；6 个作用域（3 平台 × 2 区域）照常绑定。
 * ★ 改小它会**关掉既有用户的快捷键** —— 那是功能倒退，不是降级。
 */
export const SCOPE_SHORTCUT_LIMIT = 9;

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
 * 聚合页签快捷键的**物理键**（`KeyboardEvent.code`）。
 *
 * 用 `code` 而不是 `key`：非美式布局上这枚（数字 `1` 左边的）键产出的字符不一定是
 * 反引号，而 `code` 与布局无关。`event.key` 只能作兜底（少量环境不填 `code`）。
 *
 * ⚠️ `App.tsx` 的全局键**必须**读这个常量 —— 门禁断言它不得再出现该字面量。
 * 改键位只改这里；漏改 `App.tsx` 会被 `scripts/check-scope-registry.cjs` 拦下。
 */
export const AGGREGATE_SHORTCUT_CODE = "Backquote";

/**
 * 聚合页签的快捷键提示：`Ctrl` + 反引号键（键盘上数字 `1` 左边那枚）。
 *
 * ## 为什么和 `⌘1..N` 分开，不并进 `planScopeShortcuts`
 *
 * 聚合**不是第 7 个作用域**（`Scope` 是封闭字面量，`SCOPES` 长度被门禁钉死为 6）。
 * 它是一条独立路由 `/aggregate`，因此也不参与「>9 个作用域降级」与
 * 「作用域快捷键开关」—— 那两个概念的对象都是作用域。这枚键只是**入口加速器**，
 * 与 `⌘B`（侧栏折叠）同族：恒在，不进规划结果。
 *
 * ## 为什么提示写 `Ctrl` 而不是 `⌘`
 *
 * macOS 的 `Cmd` + 反引号 是系统级「循环切换窗口」（Move focus to next window），
 * 可能在系统层就被吃掉、到不了 WebView；真实可达的是 `Ctrl` + 反引号。
 * 绑定本身仍接受 `ctrlKey || metaKey`（与其它全局键同族），但提示必须说真话。
 *
 * ## 提示按美式键帽写（已知限制）
 *
 * 提示是静态字符串，不随键盘布局变。非美式布局上这枚**物理键**的键帽可能不是反引号
 * （德语 `^` / `´`、法语 `²`…）—— 功能照常触发（判据是 `code`），只有文字与键帽不符。
 * 用 `navigator.keyboard.getLayoutMap()` 动态生成能修掉，但那是异步 + 特性检测的 API，
 * 为一个键不值当；主要用户群用标准布局。
 *
 * ## 与实现的同源关系（门禁可证）
 *
 * 键位来自 {@link AGGREGATE_SHORTCUT_CODE}、文案来自本常量，两处都在本文件；
 * `App.tsx` 只允许 import 前者、`scope-selector.tsx` 只允许 import 后者。
 * 门禁三查：物理键值 = `Backquote`、`App.tsx` 无该字面量、菜单渲染了本常量。
 */
export const AGGREGATE_SHORTCUT_HINT = "Ctrl+`";

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
