/**
 * 页内**左侧子导航**（L2.5）—— 网关 / 设置两个工作台共用同一份版式。
 *
 * ## 为什么抽出来
 *
 * 设置页（9 项）与网关页（6 项）的子导航**外观与行为完全相同**：
 * 窄屏横向滚动、宽屏纵向列表、`aria-current` 标当前位置、点击只切视图。
 * 两份实现必然漂移（本仓已发生过多次），故只留一份。
 *
 * ## 为什么不做成「可折叠 + 记住位置」
 *
 * 子导航回答的是「这一类里有哪些项」，不是「我在哪一步」；位置真值在 URL
 * （网关用 `?sub=`、设置用 `?section=`）。本组件只负责**切换视图**，
 * 选中态交给调用方（`active` / `onSelect`），**不自持状态** —— 否则
 * 「刷新后回到第一项」这类问题要在两个地方各修一遍。
 */

import type { LucideIcon } from "lucide-react";

import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { TranslationKey } from "@/locales/zh";

export interface SideSubnavItem<Id extends string = string> {
  id: Id;
  /** 标签键（应用级导航，落在 `shell` 域）。 */
  labelKey: TranslationKey;
  icon: LucideIcon;
}

interface Props<Id extends string> {
  active: Id;
  items: readonly SideSubnavItem<Id>[];
  onSelect: (id: Id) => void;
  /** 无障碍名称：各页自己的说法（如「设置分区」「网关分区」）。 */
  ariaLabel: string;
  className?: string;
}

/**
 * 左侧子导航。
 *
 * 窄屏横向滚动（避免把内容挤成一条）、宽屏纵向列表（与内容并排）。
 */
export function SideSubnav<Id extends string>({
  active,
  items,
  onSelect,
  ariaLabel,
  className,
}: Props<Id>) {
  const t = useT();
  return (
    <nav
      aria-label={ariaLabel}
      className={cn(
        "flex min-w-0 gap-1 overflow-x-auto pb-1 lg:flex-col lg:overflow-visible lg:pb-0",
        className,
      )}
    >
      {items.map((item) => {
        const on = item.id === active;
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            type="button"
            aria-current={on ? "page" : undefined}
            onClick={() => onSelect(item.id)}
            className={cn(
              "flex shrink-0 cursor-pointer items-center gap-2 whitespace-nowrap rounded-lg px-3 py-2 text-[13px] font-medium transition-colors lg:w-full",
              on
                ? "bg-foreground/[0.06] text-foreground"
                : "text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground",
            )}
          >
            <Icon className="size-4 shrink-0" aria-hidden="true" />
            <span className="truncate">{t(item.labelKey)}</span>
          </button>
        );
      })}
    </nav>
  );
}