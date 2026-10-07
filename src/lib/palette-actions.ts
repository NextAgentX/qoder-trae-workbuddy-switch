/**
 * 命令面板**动作扩展点** —— ★ **P7 接口**（`P6-ARCH.md` §1.11）。
 *
 * ## 为什么单独一层（依赖方向：P7 等 P6）
 *
 * P7 的两个动作（「导出全部备份」/「从备份导入」）要出现在命令面板里，但 P7 **不能**
 * 依赖一个不存在的面板。于是把「动作清单」从面板组件里**解耦**出来：
 * - P6 先在这里注册**基础动作**（刷新当前作用域 / 新增账号 / 打开数据目录）；
 * - P7 落地时只需 `registerPaletteActions([...])`，**不改面板组件**。
 *
 * ⇒ 依赖方向坐实：**P7 等 P6**（先有面板 + 扩展点，P7 再挂动作），**不是 P6 等 P7**。
 *
 * ## 为什么 run 只经 `ctx.services`（而不 import api）
 *
 * 本文件要保持**零重依赖**：它会被门禁（`scripts/check-scope-registry.cjs`）直接
 * 加载断言。若这里 import `@/lib/api`（连带 Tauri / 一大票模块），门禁就得把整条链
 * 拉起来。因此「碰 api 的部分」收敛到 `PaletteContext.services`（由组件注入），
 * 本文件只描述**动作是什么、何时可执行、调用哪个 service**。
 *
 * ## 注册语义
 *
 * `registerPaletteActions` 按 **id 去重**：同一个 id 只保留**首次**注册的那条 ——
 * 重复注册不会把清单撑成两份，也不会让后注册者悄悄覆盖先注册者。
 */

import type { TranslationKey } from "@/locales/zh";
import type { PaletteContext } from "@/lib/command-palette";

/** 一个可在命令面板里执行的动作。 */
export interface PaletteAction {
  /** 稳定 key（断言 + 去重用）。 */
  id: string;
  /** 标题文案键（已 i18n）。 */
  titleKey: TranslationKey;
  /** 副标题文案键（可空）。 */
  subtitleKey?: TranslationKey;
  /** 运行条件：返回 `false` 时该动作**不渲染**（§3.4 规则 1）。 */
  enabled?: (ctx: PaletteContext) => boolean;
  /** 执行。副作用经 `ctx.services` 注入（见文件头）。 */
  run: (ctx: PaletteContext) => void | Promise<void>;
}

/**
 * P6 基础动作。P7 往 {@link registerPaletteActions} 追加它的两个动作即可。
 *
 * ⚠️ 三个动作的 `run` 都只转发到 `ctx.services`：它们**没有**对具体平台写死判断，
 * 「当前作用域」由 `ctx.currentScope` 决定，因此对 WorkBuddy / Trae 都成立。
 */
export const BASE_PALETTE_ACTIONS: readonly PaletteAction[] = [
  {
    id: "refresh-scope",
    titleKey: "palette.action.refresh",
    run: (ctx) => ctx.services.refreshScope(ctx.currentScope),
  },
  {
    id: "add-account",
    titleKey: "palette.action.addAccount",
    run: (ctx) => ctx.services.addAccount(ctx.currentScope),
  },
  {
    id: "open-data-dir",
    titleKey: "palette.action.openDataDir",
    run: (ctx) => ctx.services.openDataDir(ctx.currentScope),
  },
];

/** 已注册动作（含基础动作）。**就地可变**：注册是显式副作用。 */
const registered: PaletteAction[] = [...BASE_PALETTE_ACTIONS];

/** 注册动作（按 id 去重，保留首次）。P7 调它挂上自己的两个动作。 */
export function registerPaletteActions(actions: readonly PaletteAction[]): void {
  for (const action of actions) {
    if (registered.some((existing) => existing.id === action.id)) continue;
    registered.push(action);
  }
}

/** 当前动作清单（基础动作 + 注册动作），供面板合并进 `PaletteContext.actions`。 */
export function collectPaletteActions(): readonly PaletteAction[] {
  return registered;
}
