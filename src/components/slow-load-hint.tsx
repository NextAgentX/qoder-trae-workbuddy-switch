import { Loader2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * 「这次加载慢得不正常」的一行提示（issue #188 第四节）。
 *
 * ## 它解决的是什么
 *
 * 骨架屏只能表达「正在取」，表达不了「已经等了很久」。两者在界面上原本**完全一样**，
 * 于是慢加载与卡死在用户眼里无法区分。这行提示把判断从猜测变成事实。
 *
 * ## 为什么不是转圈
 *
 * `docs/specs/PLAN.md` §3.6 禁止用居中 spinner **占位**（骨架 → 转圈二次跳变）。
 * 本组件不占内容区、不替换骨架：它挂在骨架**上方**，是一行状态文字（`role="status"`），
 * 文案只陈述不确定进度，不给假百分比（本仓拿不到进度）。
 *
 * ## 为什么重试是**可选**的
 *
 * 有的调用点（账号详情抽屉）拿不到刷新出口 —— 与其造一个假按钮，不如只显示文案。
 * `onRetry` 存在才渲染按钮，避免「点了没反应」。
 */
export function SlowLoadHint({
  onRetry,
  retrying = false,
  className,
}: {
  /** 重试出口。没有就不渲染按钮（不要为空回调渲染一个死按钮）。 */
  onRetry?: () => void;
  /** 重试已在进行中：按钮转 `Loader2` 并禁用，避免连点。 */
  retrying?: boolean;
  className?: string;
}) {
  const t = useT();

  return (
    <div
      role="status"
      aria-live="polite"
      data-slot="slow-load-hint"
      className={cn("flex flex-wrap items-center gap-2 text-xs text-muted-foreground", className)}
    >
      <span>{t("shared.slowLoad.hint")}</span>
      {onRetry ? (
        <Button variant="outline" size="sm" disabled={retrying} onClick={onRetry}>
          {retrying ? (
            <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
          ) : (
            <RefreshCw className="size-3.5" aria-hidden="true" />
          )}
          {t("shared.slowLoad.retry")}
        </Button>
      ) : null}
    </div>
  );
}
