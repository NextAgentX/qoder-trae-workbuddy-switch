import { useCallback, useMemo } from "react";
import { useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom";

import { parseScope, scopeOf, scopeParts } from "@/lib/scope";
import { scopeShortcutTarget } from "@/lib/scope-shortcuts";
import type { TraeRegionId } from "@/lib/trae-types";

/** 承载**区域**的 URL 查询参数名。 */
const VARIANT_PARAM = "line";

/** `/s/{scope}/{dest}` 的目的地段（用于在路径形态下改作用域时保住当前页）。 */
function destOfPath(pathname: string): string | null {
  return /^\/s\/[^/]+\/([^/]+)/.exec(pathname)?.[1] ?? null;
}

/**
 * 当前正在管理的 Trae **区域**（国内版 / 国际版），由 URL 承载。
 *
 * ## 名称说明（历史名，含义已变）
 *
 * 函数名与文件名里的 `variant` 是改造前的叫法。轴翻到**区域**之后，它返回的是
 * 区域标识（`cn` / `global`）；改名排在收尾（见
 * `.trellis/tasks/09-21-trae-region-program-model` 的段 3 清单）。
 *
 * ## 为什么放在 URL 而不是 React state / 全局 store
 *
 * 侧栏只有一个「TraeWork」分区，但区域是**数据维度**：若放在全局 state 里，
 * 会出现三个问题 —— 刷新即丢失、链接不可分享、浏览器前进/后退与状态分歧。
 * 放进查询串后，区域成为**路由状态的一部分**，上述三条自然消解。
 *
 * ## 取值：**路径作用域优先**，回落 `?line=`（P2 桥接）
 *
 * P2 把作用域（平台 × 区域）搬进路径（`/s/trae-global/accounts`）——区域本就属于
 * 作用域，若还让 `?line=` 说了算，同一件事实就有两个真值，迟早互相打架。
 * 因此：
 *
 * 1. 路径里有 `trae` 作用域 ⇒ **以它为准**（最显式、可分享）；
 * 2. 否则回落 `?line=`（P1 形态）—— 旧链接 / 旧书签继续有效；
 * 3. 都没有 ⇒ 国内版。
 *
 * 写入同理：在路径形态下改区域 = **改作用域段**（导航到 `/s/trae-{region}/{dest}`），
 * 在旧形态下才写 `?line=`。两者都只留下**一个**真值。
 *
 * ## 取值与别名
 *
 * - `cn`（默认，**不写进 URL**）：国内版。旧值 `trae_work` / `trae_cn` / `work`
 *   一律视为国内版 —— 改造前那两个产品线**都是国内构建**（见 Rust 侧
 *   `TraeVariant::region()`），旧书签不能落到国际版去读一本空库。
 * - `global`：国际版。
 * - 未知值一律回落国内版（与 Rust 侧 `parse_variant_param` 的宽容规则一致）。
 */
export function useTraeVariant(): [TraeRegionId, (next: TraeRegionId) => void] {
  const [params, setParams] = useSearchParams();
  const routeParams = useParams();
  const location = useLocation();
  const navigate = useNavigate();

  /** 路径里的 Trae 区域（`/s/trae-global/...`）—— 最权威；非 Trae 作用域时为 `null`。 */
  const pathRegion = useMemo<TraeRegionId | null>(() => {
    const scope = parseScope(routeParams.scope);
    if (!scope) return null;
    const parts = scopeParts(scope);
    return parts.product === "trae" ? parts.region : null;
  }, [routeParams.scope]);

  const region = useMemo<TraeRegionId>(() => {
    if (pathRegion) return pathRegion;
    const raw = (params.get(VARIANT_PARAM) ?? "").trim().toLowerCase().replace(/-/g, "_");
    return raw === "global" || raw === "intl" || raw === "international" ? "global" : "cn";
  }, [params, pathRegion]);

  const setRegion = useCallback(
    (next: TraeRegionId) => {
      // ① 路径形态：改作用域段，保住当前目的地（不再写 `?line=`，避免第二真值）。
      if (pathRegion !== null) {
        if (next === pathRegion) return;
        navigate(scopeShortcutTarget(scopeOf("trae", next), destOfPath(location.pathname) ?? "accounts"), {
          replace: true,
        });
        return;
      }
      // ② 旧形态（`/trae/*` + `?line=`）：写查询参数。
      setParams(
        (prev) => {
          const draft = new URLSearchParams(prev);
          if (next === "cn") {
            // 默认区域**不写进 URL**：让「默认」与「显式指定」产生同一个 URL，
            // 避免两条看起来不同的链接其实等价、也避免默认值污染可读性。
            draft.delete(VARIANT_PARAM);
          } else {
            draft.set(VARIANT_PARAM, next);
          }
          return draft;
        },
        { replace: true },
      );
    },
    [pathRegion, location.pathname, navigate, setParams],
  );

  return [region, setRegion];
}
