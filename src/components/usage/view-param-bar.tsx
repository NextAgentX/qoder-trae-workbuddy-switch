/**
 * 用量 · **视图参数条**（P3，每页唯一）。
 *
 * ## 为什么收敛成一条（PLAN §3.9）
 *
 * 改造前同一页有 **4 组**切换器（来源切片 / 时间范围 / 区域 / 合并），
 * 用户分不清哪一组决定「数据从哪来」、哪一组只是「怎么看」。
 * 收敛后：**范围（7/14/30）在本条**，**区域归侧栏 `ScopeSwitch`**，
 * **合并态**由调用方以 `children` 传入（`<RegionBar value={combineAll} … />`）。
 *
 * ## 为什么合并控件用 `children` 插槽而不是内置
 *
 * 合并态不是纯视图参数条能自洽的（它需要页面级的 `combineAll` 状态与 `RegionBar`）。
 * 用插槽传入，既保证**视觉上是一条**，也让 `RegionBar` 字面量留在页面里
 * （门禁 `check-scope-registry` 据此确认「页内只留合并态、没有区域选择器」）。
 */

import type { ReactNode } from "react";

import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import { cn } from "@/lib/utils";
import type { UsageRange } from "@/lib/usage-charts";

const RANGES: UsageRange[] = [7, 14, 30];

const RANGE_LABEL: Record<UsageRange, TranslationKey> = {
  7: "wbStats.usage.range.7",
  14: "wbStats.usage.range.14",
  30: "wbStats.usage.range.30",
};

export interface ViewParamBarProps {
  range: UsageRange;
  onRangeChange: (range: UsageRange) => void;
  /** 合并态开关（调用方传入 `<RegionBar value={combineAll} … />`）。 */
  children?: ReactNode;
  className?: string;
}

export function ViewParamBar({ range, onRangeChange, children, className }: ViewParamBarProps) {
  const t = useT();
  return (
    <div className={cn("flex min-w-0 flex-wrap items-center gap-3", className)}>
      <span className="text-xs text-muted-foreground">{t("wbStats.usage.param.title")}</span>
      <div
        className="flex gap-1 rounded-lg bg-muted p-1"
        role="group"
        aria-label={t("wbStats.usage.param.aria")}
      >
        {RANGES.map((value) => (
          <button
            key={value}
            type="button"
            className={cn(
              "cursor-pointer rounded-md px-2.5 py-1.5 text-xs transition-colors",
              range === value
                ? "bg-background font-medium text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground",
            )}
            onClick={() => onRangeChange(value)}
            aria-pressed={range === value}
          >
            {t(RANGE_LABEL[value])}
          </button>
        ))}
      </div>
      <div className="grow" />
      {children}
    </div>
  );
}
