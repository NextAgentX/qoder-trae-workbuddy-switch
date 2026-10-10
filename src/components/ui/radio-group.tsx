import * as React from "react";
import * as RadioGroupPrimitive from "@radix-ui/react-radio-group";
import { Circle } from "lucide-react";

import { cn } from "@/lib/utils";

/**
 * 单选组（shadcn `RadioGroup`，基于 `@radix-ui/react-radio-group`）。
 *
 * ## 为什么新增依赖而不是用原生 `<input type="radio">` 或自己画
 *
 * `PLAN.md` 附录「需要补齐的组件」把 `radio-group` 列为 **P3 前置**，理由写得很直白：
 * 「§3.15.2 分叉时「保留哪一份」—— **单选语义缺失**」。
 * 手写 `role="radio"` + `aria-checked` 只能骗过静态检查，**方向键漫游（roving tabindex）
 * 与整组只占一个 Tab 位**这两条最容易漏，而它们正是单选组对键盘用户的意义所在。
 * 原生 `<input type="radio">` 则由系统绘制、无法跟随本仓 token 主题（同 `checkbox.tsx` 的理由）。
 *
 * ## 与 `checkbox.tsx` 保持同一套视觉重量
 *
 * 同圆角 / 同描边 / 同 focus ring / 同 disabled 处理 —— 两者会在同一张抽屉里并排出现
 * （会话树的勾选 + 分叉时的单选），视觉漂移会立刻看得出来。
 */
function RadioGroup({
  className,
  ...props
}: React.ComponentProps<typeof RadioGroupPrimitive.Root>) {
  return (
    <RadioGroupPrimitive.Root
      data-slot="radio-group"
      className={cn("grid gap-2", className)}
      {...props}
    />
  );
}

function RadioGroupItem({
  className,
  ...props
}: React.ComponentProps<typeof RadioGroupPrimitive.Item>) {
  return (
    <RadioGroupPrimitive.Item
      data-slot="radio-group-item"
      className={cn(
        "border-input dark:bg-input/30 data-[state=checked]:border-primary focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 aria-invalid:border-destructive size-4 shrink-0 cursor-pointer rounded-full border shadow-xs transition-shadow outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <RadioGroupPrimitive.Indicator
        data-slot="radio-group-indicator"
        className="relative flex items-center justify-center"
      >
        {/* 实心点用 `fill-current` 而不是再包一层 div：少一个盒子，缩放时不跑位。 */}
        <Circle className="fill-primary text-primary size-2" strokeWidth={0} />
      </RadioGroupPrimitive.Indicator>
    </RadioGroupPrimitive.Item>
  );
}

export { RadioGroup, RadioGroupItem };
