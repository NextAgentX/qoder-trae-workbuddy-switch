/**
 * 空态（PLAN §3.6）—— **全应用唯一的空态组件**。
 *
 * ## 为什么需要它
 *
 * §3.6 把「空」定为**唯一形态**，但改造前**没有这个组件**：
 * 12+ 个文件各自写空态，从「一行灰字」到「图标 + 标题 + 说明 + 按钮」什么都有。
 * 同一个「没有数据」在不同页面长得不一样，用户得重新理解一次。
 *
 * ## 为什么是自定义组件（AGENTS.md 要求记录理由）
 *
 * shadcn **没有** EmptyState，且它**不是交互原语** —— 它是纯展示的组合
 * （图标 + 两行文字 + 可选出口），由项目现有 token 与 `cn()` 拼成，
 * 不涉及焦点管理 / 键盘交互 / 无障碍模式，因此不需要 Radix 层。
 *
 * ## 四要素（缺一不可）
 *
 * `icon` · `title` · `description` · `action` —— 其中 **`action` 是「出口」**：
 * 空态必须告诉用户**下一步做什么**，只给一句「暂无数据」等于把用户丢在原地。
 *
 * ⚠️ **两种空态要给不同出口**（§3.15.1）：
 * - 「平台不支持」⇒ 说明原因，**不要**引导用户去设置里找开关（那里没有）；
 * - 「被你自己关掉了」⇒ 给「去设置」入口。
 * 本组件只负责渲染，**出口由调用方决定** —— 这正是 `action` 存在的原因。
 */

import type { ComponentType, ReactNode } from "react";

import { cn } from "@/lib/utils";

export interface EmptyStateProps {
  /** 图标组件（lucide 图标即可）。 */
  icon: ComponentType<{ className?: string }>;
  /** 一句话说明「这里本该有什么」。 */
  title: string;
  /** 补充原因或后果（可选，但**有原因时一定要写**）。 */
  description?: string;
  /** **出口**：下一步做什么（按钮 / 链接）。 */
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ icon: Icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-2 px-6 py-10 text-center",
        className,
      )}
    >
      <Icon className="size-8 text-muted-foreground/50" aria-hidden="true" />
      <p className="text-sm font-medium">{title}</p>
      {description ? (
        <p className="max-w-sm text-xs leading-relaxed text-muted-foreground">{description}</p>
      ) : null}
      {action ? <div className="mt-1.5">{action}</div> : null}
    </div>
  );
}
