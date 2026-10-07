/**
 * 用量四视图 · **统计 tile**（P3）。
 *
 * ## 为什么收敛到一处
 *
 * 四个视图各有 4 个 tile，形态完全一致（图标 + 标签 + 大号数字 + 可选副标题）。
 * 改造前每个页面各写一份（`TokenStatsPage` / `CreditStatsPage` / `Trae*` 里的 `StatMetric`），
 * 四处漂移。这里收成一处，四视图共用。
 *
 * ⚠️ 展示值一律由调用方**先格式化为字符串**（数字格式化 / `displayText` 归一都在调用方），
 * 本组件只渲染字符串 ⇒ 从结构上排除「对象当 React 子节点」的白屏（issue #2）。
 */

import type { LucideIcon } from "lucide-react";

import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";

export interface StatTile {
  icon: LucideIcon;
  label: string;
  value: string;
  hint?: string;
  tone?: "default" | "warn" | "up" | "down";
}

const TONE_CLASS: Record<NonNullable<StatTile["tone"]>, string> = {
  default: "text-foreground",
  warn: "text-amber-600 dark:text-amber-400",
  up: "text-emerald-600 dark:text-emerald-400",
  down: "text-amber-600 dark:text-amber-400",
};

export function StatTiles({ tiles, ariaLabel }: { tiles: StatTile[]; ariaLabel?: string }) {
  if (tiles.length === 0) return null;
  return (
    <Card
      className="min-w-0 gap-0 overflow-hidden rounded-2xl bg-card/70 py-0 shadow-none"
      aria-label={ariaLabel}
    >
      <div
        className={cn(
          "grid min-w-0 grid-cols-1 divide-y divide-border/60 sm:divide-y-0 sm:py-5",
          tiles.length >= 4 ? "sm:grid-cols-4" : tiles.length === 3 ? "sm:grid-cols-3" : "sm:grid-cols-2",
        )}
      >
        {tiles.map((tile, index) => (
          <div
            key={tile.label}
            className={cn(
              "flex min-w-0 flex-col items-center justify-center px-4 py-5 text-center sm:py-3",
              index > 0 && "sm:border-l sm:border-border/60",
            )}
          >
            <div className="flex max-w-full items-center justify-center gap-2 text-[13px] font-medium leading-5 text-muted-foreground">
              <tile.icon className="size-4 shrink-0 stroke-[1.75]" aria-hidden="true" />
              <span className="truncate">{tile.label}</span>
            </div>
            <div
              className={cn(
                "mt-3 max-w-full truncate text-[26px] font-semibold leading-8 tracking-[-0.025em] tabular-nums",
                TONE_CLASS[tile.tone ?? "default"],
              )}
              style={{
                fontFamily:
                  '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif',
              }}
            >
              {tile.value}
            </div>
            {tile.hint ? (
              <div className="mt-1.5 max-w-full truncate text-xs text-muted-foreground">
                {tile.hint}
              </div>
            ) : null}
          </div>
        ))}
      </div>
    </Card>
  );
}
