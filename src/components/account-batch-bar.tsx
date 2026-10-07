import { CircleCheck, RefreshCw, Trash2, XIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * 最小批量操作条（**两平台共用**，PLAN §3.3 的「勾选 + 批量」本期最小形态）。
 *
 * ## 为什么是底部浮条而不是工具栏
 *
 * 它只在**有勾选**时出现（`selectedCount === 0` 直接不渲染），属于「选择模式的上下文动作」，
 * 放在底部浮条可以让用户在不离开列表的情况下连续勾选 + 操作；塞进顶部工具栏会与
 * 常驻开关（自动签到 / 紧凑 / 刷新）混在一起，且顶部工具栏在两个平台各有差异，
 * 不适合承载平台无关的批量语义。
 *
 * ## 动作是可选的（`onCheckin?` / `onRefresh?` / `onDelete?`）
 *
 * 平台能力不同（WB 有积分刷新、Trae 侧动作集不同），**缺哪个就不渲染哪个**，
 * 而不是渲染一枚点了没反应的按钮 —— 后者比不渲染更差。
 *
 * ## 为什么是自定义组件（AGENTS.md 要求记录理由）
 *
 * 它不是交互原语，而是「计数 + 若干按钮 + 清除」的纯展示组合，由现有 `Button` / token 拼成，
 * 不涉及焦点管理 / 键盘模式，因此不需要 Radix 层，也不必新增 shadcn 组件。
 */
export interface AccountBatchBarProps {
  /** 已选数量；`<= 0` 时整个条不渲染。 */
  selectedCount: number;
  /** 任一批量动作进行中（禁用所有动作，避免并发）。 */
  busy?: boolean;
  onCheckin?: () => void;
  onRefresh?: () => void;
  onDelete?: () => void;
  /** 清除选择（必填：这是「退出选择模式」的唯一出口）。 */
  onClear: () => void;
}

export function AccountBatchBar({
  selectedCount,
  busy = false,
  onCheckin,
  onRefresh,
  onDelete,
  onClear,
}: AccountBatchBarProps) {
  const t = useT();
  if (selectedCount <= 0) return null;

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-6 z-40 flex justify-center px-4">
      <div className="pointer-events-auto flex items-center gap-1 rounded-full border border-border bg-popover/95 px-2 py-1.5 shadow-[0_10px_30px_rgba(15,23,42,.18)] backdrop-blur">
        <span className="px-2 text-xs font-medium tabular-nums">
          {t("shared.batch.selected", { n: selectedCount })}
        </span>
        <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
        {onCheckin && (
          <Button
            variant="ghost"
            size="sm"
            className="h-8 rounded-full px-3 text-xs"
            disabled={busy}
            onClick={onCheckin}
          >
            <CircleCheck />
            {t("shared.batch.checkin")}
          </Button>
        )}
        {onRefresh && (
          <Button
            variant="ghost"
            size="sm"
            className="h-8 rounded-full px-3 text-xs"
            disabled={busy}
            onClick={onRefresh}
          >
            <RefreshCw />
            {t("shared.batch.refresh")}
          </Button>
        )}
        {onDelete && (
          <Button
            variant="ghost"
            size="sm"
            className={cn("h-8 rounded-full px-3 text-xs text-destructive hover:bg-destructive/5 hover:text-destructive")}
            disabled={busy}
            onClick={onDelete}
          >
            <Trash2 />
            {t("shared.batch.delete")}
          </Button>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="ml-0.5 size-8 rounded-full text-muted-foreground hover:text-foreground"
          onClick={onClear}
          aria-label={t("shared.batch.clear")}
          title={t("shared.batch.clear")}
        >
          <XIcon />
        </Button>
      </div>
    </div>
  );
}
