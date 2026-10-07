/**
 * `cmdk` 的**薄封装**（shadcn 风格）—— P6 / `P6-ARCH.md` §1.6。
 *
 * ## 为什么用 `cmdk` 而不是手写列表
 *
 * 命令面板是 combobox 语义：`aria-activedescendant` + 上下键漫游 + `aria-live` 播报
 * 结果数 + 过滤时的焦点管理。手写这些极易漏项（尤其无障碍契约），PLAN §3.11 明确
 * 指定「用 `cmdk`，不要手写列表」。本文件只做**样式封装**，不改行为。
 *
 * ## 为什么不在此处包 Dialog
 *
 * 浮层由**既有** `@radix-ui/react-dialog`（已装）承载 —— 焦点陷阱 / `Esc` 关闭 /
 * 关闭后焦点归还触发按钮都是 Radix 的默认能力。把 Dialog 留到面板组件里组合，
 * 本文件就只依赖 `cmdk`，职责单一、可复用（将来别的浮层也能用）。
 *
 * ## 为什么只封装这五个
 *
 * 只导出**被命令面板实际用到**的原语（`Command` / `Input` / `List` / `Group` / `Item`）
 * —— 空态 / 分隔线 / 加载态在面板里各有更合适的实现（`<EmptyState>` / `<Skeleton>`），
 * 多导出的原语只会变成**没人引用的死代码**。
 */

import * as React from "react";
import { Command as CommandPrimitive } from "cmdk";
import { Search } from "lucide-react";

import { cn } from "@/lib/utils";

/** 根容器。`shouldFilter={false}` 时由调用方自行过滤（面板要复用 `account-search`）。 */
function Command({ className, ...props }: React.ComponentProps<typeof CommandPrimitive>) {
  return (
    <CommandPrimitive
      data-slot="command"
      className={cn(
        "flex h-full w-full flex-col overflow-hidden rounded-lg bg-popover text-popover-foreground",
        className,
      )}
      {...props}
    />
  );
}

/** 搜索输入框（带左侧放大镜）。 */
function CommandInput({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Input>) {
  return (
    <div
      className="flex items-center gap-2 border-b border-border/60 px-3"
      data-slot="command-input-wrapper"
    >
      <Search className="size-4 shrink-0 text-muted-foreground/70" aria-hidden="true" />
      <CommandPrimitive.Input
        data-slot="command-input"
        className={cn(
          "flex h-11 w-full bg-transparent py-3 text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        {...props}
      />
    </div>
  );
}

/** 可滚动的结果列表。 */
function CommandList({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.List>) {
  return (
    <CommandPrimitive.List
      data-slot="command-list"
      className={cn("max-h-[360px] overflow-x-hidden overflow-y-auto p-1", className)}
      {...props}
    />
  );
}

/** 分组（标题由 cmdk 的 `heading` 渲染）。 */
function CommandGroup({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Group>) {
  return (
    <CommandPrimitive.Group
      data-slot="command-group"
      className={cn(
        "overflow-hidden p-1 text-foreground",
        "[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground",
        className,
      )}
      {...props}
    />
  );
}

/** 可选项。选中态用 `data-[selected=true]`（cmdk 管理）。 */
function CommandItem({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Item>) {
  return (
    <CommandPrimitive.Item
      data-slot="command-item"
      className={cn(
        "relative flex cursor-default select-none items-center gap-2 rounded-md px-2 py-2 text-sm outline-none",
        "data-[selected=true]:bg-accent data-[selected=true]:text-accent-foreground",
        "data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Command, CommandInput, CommandList, CommandGroup, CommandItem };
