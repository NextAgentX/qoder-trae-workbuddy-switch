import * as React from "react";
import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { Check } from "lucide-react";

import { cn } from "@/lib/utils";

/**
 * 勾选框（shadcn `Checkbox`，基于 `@radix-ui/react-checkbox`）。
 *
 * ## 为什么新增依赖而不是用原生 `<input type="checkbox">`
 *
 * 原生勾选框在 Tauri WebView 里由系统绘制，无法跟随本仓 token 主题，尺寸也不受控；
 * 而账号卡片的勾选框需要与「更多菜单 / 程序位按钮」同一套视觉重量（同圆角、同描边）。
 * Radix 版本同时给出无障碍语义（`role="checkbox"` + `aria-checked` + Space 切换），
 * 这是纯 `<span>` 手写勾选框给不了的。
 *
 * ## 为什么用 `Check` 而不是 `CheckIcon`
 *
 * 本仓既有组件统一用无后缀图标名（`account-card.tsx` 的 `Check`），保持一致。
 */
function Checkbox({ className, ...props }: React.ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      className={cn(
        "border-input dark:bg-input/30 data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground data-[state=checked]:border-primary focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 aria-invalid:border-destructive peer size-4 shrink-0 cursor-pointer rounded-[4px] border shadow-xs transition-shadow outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator
        data-slot="checkbox-indicator"
        className="flex items-center justify-center text-current transition-none"
      >
        <Check className="size-3.5" strokeWidth={3} />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  );
}

export { Checkbox };
