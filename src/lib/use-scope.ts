/**
 * 作用域 hook（**P1 产出**）—— **URL 为唯一真值 + localStorage 记忆**。
 *
 * ## 解决什么问题（PLAN §1.2 诊断）
 *
 * 改造前「当前是哪个区域」有**三套互不通信的存储**：
 *
 * | 页面 | 改造前 | 后果 |
 * |:---|:---|:---|
 * | API 服务页 | URL `?region=` | ✅ 唯一对的那个 |
 * | 账号管理页 | 组件 `useState("cn")` | 刷新即丢 |
 * | 统计页 ×2 | 各页独立 `localStorage` | **跨页错位**（统计页注释原话：「各页独立记忆，不与账号管理页联动」） |
 *
 * ⇒ **现象**：在账号页切到国际版，进统计页又变回国内版。
 *
 * ## 机制：URL 优先，localStorage 兜底
 *
 * 三个页面是**独立路由**（`/`、`/token-stats`、`/credit-stats`），
 * 各自带 `?region=` 时**互不共享**。所以单靠 URL 不够，必须加一层记忆：
 *
 * 1. **读**：URL 有 `?region=` ⇒ 用它（可分享、可前进后退）；
 * 2. 否则读 localStorage 的「上次作用域」⇒ **product 匹配时**用它的 region；
 * 3. 都没有 ⇒ `cn`。
 *
 * 3 是关键：**跨路由时 URL 上没有参数，靠记忆接住** —— 这就是错位的修法。
 *
 * ## 为什么存完整 `Scope` 而不是 `Region`
 *
 * 「上次作用域」是**全局一个**（PLAN §3.1：冷启动落到上次的作用域），
 * 它天然包含平台。切到 Trae 分区时，若记的是 `wb-global`，
 * 本 hook 会因 **product 不匹配**而回落 `cn`，而不是错误地套用 WB 的区域。
 *
 * ## 与「视图参数」的边界（VOCABULARY 第 1 节）
 *
 * 本 hook **只管 `cn` / `global`**。统计页的 **`all`（合并态）不是作用域** ——
 * 它只决定「怎么看」，不决定「数据从哪来」，因此**不进本 hook**，留在页面内部。
 *
 * ## 回滚
 *
 * `VITE_SCOPE_UNIFIED=0` ⇒ 退化为「只读 URL、不写记忆」，即 P1 之前的行为。
 */

import { useCallback, useMemo } from "react";
import { useParams, useSearchParams } from "react-router-dom";

import type { Region } from "@/lib/types";
import type { Product } from "@/lib/platforms/registry";
import { LAST_SCOPE_KEY, parseScope, scopeOf, scopeParts } from "@/lib/scope";

/** 承载**区域**的 URL 查询参数名。与既有 `useWorkbuddyRegion` 一致 ⇒ **旧链接继续有效**。 */
export const REGION_PARAM = "region";

/** 默认区域。**不写进 URL**（§3.1 规则 3：默认值与显式指定产生同一个 URL）。 */
export const DEFAULT_REGION: Region = "cn";

/**
 * P1 回滚开关（PLAN §5 的 P1 回滚策略）。
 *
 * 设 `VITE_SCOPE_UNIFIED=0` ⇒ 本 hook 只读 URL、不读也不写记忆，
 * 回到「各页自持状态」。**默认开启**。
 */
export function isScopeUnified(): boolean {
  return import.meta.env.VITE_SCOPE_UNIFIED !== "0";
}

/**
 * 宽容解析区域值。
 *
 * `intl` / `international` 视为 `global` —— 与 Trae 侧既有规则一致，
 * 避免同一个概念在两处接受不同的写法。
 */
export function normalizeRegion(raw: string | null | undefined): Region | null {
  const v = (raw ?? "").trim().toLowerCase();
  if (!v) return null;
  if (v === "global" || v === "intl" || v === "international") return "global";
  if (v === "cn") return "cn";
  return null;
}

/** 读「上次作用域」中属于该平台的区域。非法 / 缺失 / 平台不匹配一律返回 `null`。 */
function readRememberedScope(): string | null {
  if (typeof window === "undefined" || !isScopeUnified()) return null;
  try {
    return window.localStorage.getItem(LAST_SCOPE_KEY);
  } catch {
    return null; // 隐私模式 / 存储被禁：只是记不住，不影响功能
  }
}

/**
 * 解析「当前区域」的**纯函数** —— hook 只是它的薄壳。
 *
 * ⚠️ **抽出来是为了可测**：前端没有单测框架，而这条优先级规则
 * （URL > 记忆 > 默认）正是 P1 要修的缺陷所在，必须被门禁守住。
 * hook 里的 `useMemo` 只负责把 `useSearchParams` 的结果喂进来。
 *
 * 优先级：
 * 1. **URL** 有合法值 ⇒ 用它（可分享、可前进后退）；
 * 2. 否则看**记忆**里的「上次作用域」—— **product 匹配时**才用它的 region；
 * 3. 都没有 ⇒ 默认区域。
 *
 * 第 2 步的 product 匹配很关键：记的是 `wb-global` 时，Trae 分区**不该**套用 global。
 */
export function resolveRegion(input: {
  /**
   * **路径里的作用域**（P2 引入的 `/s/{scope}/{dest}` 形态）。
   * 优先级最高 —— 它是显式的、可分享的形态。
   */
  pathScope?: string | null | undefined;
  /** URL 查询串上的原始值（P1 的过渡形态，可能缺失 / 非法 / 带 `intl` 这类别名） */
  urlValue: string | null | undefined;
  /** localStorage 里的「上次作用域」（完整 Scope 字面量，可能非法） */
  rememberedScope: string | null | undefined;
  /** 当前页面所属平台 */
  product: Product;
}): Region {
  // ① 路径里的 scope（`/s/wb-global/accounts`）—— 最显式，优先
  const fromPath = parseScope(input.pathScope);
  if (fromPath) {
    const parts = scopeParts(fromPath);
    // ⚠️ 平台不匹配 ⇒ **不采信**：`/s/trae-cn/accounts` 落在 WB 页面上属于路由错配，
    // 此时宁可往下走，也不要套用别的平台的区域。
    if (parts.product === input.product) return parts.region;
  }

  // ② 查询参数（P1 形态）
  const fromUrl = normalizeRegion(input.urlValue);
  if (fromUrl) return fromUrl;

  // ③ 记忆（跨路由的关键）
  const scope = parseScope(input.rememberedScope);
  if (scope) {
    const parts = scopeParts(scope);
    if (parts.product === input.product) return parts.region;
  }
  return DEFAULT_REGION;
}

function rememberScope(product: Product, region: Region): void {
  if (typeof window === "undefined" || !isScopeUnified()) return;
  try {
    window.localStorage.setItem(LAST_SCOPE_KEY, scopeOf(product, region));
  } catch {
    /* 同上：写失败不抛 */
  }
}

/**
 * 当前**区域**（`cn` / `global`），由 URL 承载、由 localStorage 记忆。
 *
 * @param product 当前页面所属平台（通常来自路由，见 `platformOfPath`）。
 *   **必须传对** —— 它是「记忆里的作用域是否适用于本页」的判据。
 */
export function useScope(product: Product): [Region, (next: Region) => void] {
  const [params, setParams] = useSearchParams();
  /* P2：`/s/{scope}/{dest}` 把作用域放进了**路径**。在那些路由下，
     `:scope` 就是权威来源（见 `resolveRegion` 的优先级）。 */
  const routeParams = useParams();

  const region = useMemo<Region>(
    () =>
      resolveRegion({
        pathScope: routeParams.scope,
        urlValue: params.get(REGION_PARAM),
        rememberedScope: readRememberedScope(),
        product,
      }),
    // ⚠️ `params` 变化要重算；`product` 变化也要（跨分区时记忆的适用性变了）；
    //    `routeParams.scope` 同理（同一条路径切换作用域）
    [params, routeParams.scope, product],
  );

  const setRegion = useCallback(
    (next: Region) => {
      // 先写记忆，再改 URL：即使 URL 更新被 replace 掉，记忆也已生效
      rememberScope(product, next);
      setParams(
        (prev) => {
          const draft = new URLSearchParams(prev);
          if (next === DEFAULT_REGION) {
            // 默认值**不写进 URL**：让「默认」与「显式指定」产生同一个 URL
            draft.delete(REGION_PARAM);
          } else {
            draft.set(REGION_PARAM, next);
          }
          return draft;
        },
        { replace: true },
      );
    },
    [product, setParams],
  );

  return [region, setRegion];
}
