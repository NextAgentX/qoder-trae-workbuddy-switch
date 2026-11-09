import * as React from "react";

import { cn } from "@/lib/utils";

/**
 * 骨架屏 —— **扫光动画**（原型 `.sk`，`demo.css:365-366`）。
 *
 * ## 为什么不是 `animate-pulse`
 *
 * 此前用的是 Tailwind 的 `animate-pulse`（整块透明度呼吸），而原型是
 * **一道 200% 宽的高光从左扫到右**（`background-position:-200% 0`，1.3s 循环）。
 * 两者观感不同：脉冲是「整体闪」，扫光是「内容像被刷过」——
 * 后者更贴合「正在加载一块内容」的语义，且循环不中断（起点与终点同色）。
 *
 * ## 为什么类名是 `.skeleton-sweep` 而不是 `bg-muted + animate-*`（#188）
 *
 * 旧写法 `animate-skeleton-sweep bg-muted` 只有 `background-color`，**没有 `background-image`**
 * ⇒ `background-position` 没有作用对象，动画在跑但屏幕完全静止（一块不动的纯色块）；
 * 且深色 / 纯黑下 `--muted` 与 `--card` 同值 ⇒ 骨架在卡片和抽屉里直接隐形。
 * 现在渐变、两档色、延迟淡入、`prefers-reduced-motion` 全部收在 `src/index.css` 的
 * `.skeleton-sweep` 一条规则里（两档色是专用 token `--skeleton-base` / `--skeleton-hi`），
 * 全局 95 处调用点无需改动。
 *
 * 圆角对齐原型的 `7px`（此前 `rounded-md` = 10px）。
 *
 * ⚠️ 用 `cn()` 而非字符串拼接：调用点（如 `page-fallback.tsx`）传的
 * `rounded-2xl` 必须能**覆盖**这里的 `rounded-[7px]`，`cn` 的 tailwind-merge 才有这个能力。
 */
function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="skeleton"
      className={cn("skeleton-sweep rounded-[7px]", className)}
      {...props}
    />
  );
}

export { Skeleton };