/**
 * 首次引导的**持久化** —— P6 / `P6-ARCH.md` §1.5。
 *
 * ## 为什么是「关闭即持久化」而不是「15 秒自动退场」
 *
 * 原型（`demo-app.js:2660`）是「15 秒自动退场 + 会话级」。真实应用里那等于**每次冷启动
 * 都重来一遍** —— 引导条反复出现，用户会当它是噪音。因此改成「**可关闭 + 持久化**」：
 * 关一次，以后冷启动不再出现（原型退场方式见 §8 #4，已确认改法）。
 *
 * ## 与「快捷键降级」的关系
 *
 * 本模块**只管**「要不要显示引导条」。引导条**内容**（列出哪些快捷键）由调用方
 * （`components/first-run-hint.tsx`）按**真实绑定**派生 —— 降级的快捷键**不进引导**，
 * 否则就是「引导里有、按了没反应」的假承诺（§3.4 规则 1 的延伸）。
 *
 * ## 读取 / 写入是纯函数
 *
 * 接收 `storage` 入参（可传桩），可被门禁直接断言 —— 前端没有单测框架。
 */

/** `localStorage` 键。`v1` 前缀：将来语义若变可换键而不破坏旧值。 */
export const FIRST_RUN_HINT_KEY = "buddy-switch:v1:first-run-hint-dismissed";

/**
 * 是否应显示首次引导。**未被关闭过 ⇒ `true`**。
 *
 * 存储不可用或抛异常 ⇒ `true`（显示）：引导是**一次性**的，宁可多显示一次，
 * 也不要在存储受限的环境里彻底丢失这个提示。
 */
export function shouldShowFirstRunHint(storage: Pick<Storage, "getItem"> | undefined): boolean {
  try {
    return storage?.getItem(FIRST_RUN_HINT_KEY) !== "1";
  } catch {
    return true;
  }
}

/** 关闭首次引导（持久化）。**写失败不抛**。 */
export function dismissFirstRunHint(storage: Pick<Storage, "setItem"> | undefined): void {
  try {
    storage?.setItem(FIRST_RUN_HINT_KEY, "1");
  } catch {
    /* 忽略：存储不可用时只是每次都会再出现 */
  }
}
