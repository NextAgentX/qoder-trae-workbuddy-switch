import * as React from "react";

import { cn } from "@/lib/utils";

/**
 * shadcn `Textarea`（项目缺失的组件，按仓库既有 UI 组件约定补齐到 `components/ui/`）。
 *
 * 用途：客户端设置「键值编辑」的 JSON 值输入（值可能是对象 / 数组，需要多行）。
 * 与 `Input` 共用同一套边框 / 焦点环 / 禁用态 token，视觉与设置页其余控件一致。
 */
function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "border-input placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 flex field-sizing-content min-h-16 w-full rounded-md border bg-transparent px-3 py-2 text-sm shadow-xs transition-[color,box-shadow] outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Textarea };