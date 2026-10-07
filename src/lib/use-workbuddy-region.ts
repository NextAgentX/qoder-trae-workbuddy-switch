import type { Region } from "@/lib/types";
import { useScope } from "@/lib/use-scope";

/**
 * 当前正在查看的 WorkBuddy **区域**（国内版 / 国际版），由 URL 承载。
 *
 * ## 为什么要有它（2026-09-30 用户报障：两个模块的版本交互不一致）
 *
 * 改造前 WorkBuddy 的「API 服务」页把版本轴**摊在三个地方**：接入地址并排两行、
 * 账号池并排两张、模型清单与接入指引**各自内置一个版本 Tabs**。而 Trae 侧一直是
 * **页面级单一区域**（`?line=` + 页头切换器，各区块跟随）。同一个产品里的两个页面
 * 用两套心智模型，用户每切一次页面都要重新找「版本在哪切」。
 *
 * 现在统一到 Trae 的模式：**页面持有唯一区域，各区块跟随，子组件不再自带选择器**。
 *
 * ## 为什么不放 React state / 全局 store
 *
 * 与 `useTraeVariant` 同一理由：放 state 里会**刷新即丢**、
 * 链接不可分享、浏览器前进/后退与页面状态分歧。放进查询串后，区域成为**路由状态的一部分**，
 * 上述三条自然消解。
 *
 * ## ⚠️ P1 起：本 hook 是 `useScope` 的**薄封装**
 *
 * 原来它自带一套 URL 读写逻辑；P1 把「当前区域」统一到 `useScope`
 * （**URL 优先 + localStorage 记忆**）之后，本 hook 只固定「平台 = workbuddy」这一个入参。
 *
 * **保留它**而不是让调用点直接改用 `useScope("workbuddy")`，是为了**不动既有调用点**：
 * 本文件的行为变化全部来自 `useScope`，调用方零改动即可获得
 * 「账号页切国际版 → 进统计页仍是国际版」的修复。
 *
 * ## 参数名与取值
 *
 * 参数名 `region`（不是 Trae 的 `line`）—— 两个页面是不同路由，参数互不干扰；
 * 但语义不同（WorkBuddy 是**区域**，Trae 是**区域 + 程序位**），分开命名避免串味。
 *
 * 取值：`cn`（默认，**不写进 URL**）/ `global`（`intl` / `international` 同义）。
 * 未知值一律回落国内版（宽容规则与 Trae 侧一致）。
 */
export function useWorkbuddyRegion(): [Region, (next: Region) => void] {
  return useScope("workbuddy");
}
