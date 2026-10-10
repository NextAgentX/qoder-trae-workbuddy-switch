import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import type { ScopeStatus } from "@/lib/use-scope-status";

/**
 * 运行胶囊 —— **纯展示**（顶栏 + 作用域菜单共用）。
 *
 * 三态：`运行中` / `未运行` / `未检测到`。第三态由 `installed` 派生 ——
 * 「没装」与「装了但没开」是两件事：前者引导用户去安装，后者引导去启动。
 * 把两者压成同一个「未运行」会让用户对着一个装都没装的客户端找启动按钮。
 */
export type RunState = "running" | "stopped" | "missing";

const LABEL_KEY: Record<RunState, TranslationKey> = {
  running: "topbar.run.running",
  stopped: "topbar.run.stopped",
  missing: "topbar.run.missing",
};

/** 状态 → 三态（`installed=false` ⇒ 未检测到）。 */
export function runStateOf(status: ScopeStatus): RunState {
  if (!status.installed) return "missing";
  return status.running ? "running" : "stopped";
}

export function RunPill({
  status,
  compact = false,
  className,
}: {
  status: ScopeStatus;
  /** 菜单项里的紧凑形态：只留圆点（文案已由副标题承担），与原型一致。 */
  compact?: boolean;
  className?: string;
}) {
  const t = useT();
  const state = runStateOf(status);
  const on = state === "running";

  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-full font-medium",
        compact ? "h-[18px] px-[7px]" : "h-[26px] px-2.5 text-[11px]",
        /* ⚠️ 用**语义色** `--success`（原型 `.run-pill` 的 `color-mix(... var(--success) 15% ...)`，`:127`），
           不是 `primary`：两者虽同为绿系，但 primary 在浅色下偏亮、深色下偏艳，
           与「运行中」这个**状态**色不同源 —— 状态色必须全站唯一定义。 */
        on ? "bg-success/15 text-success" : "bg-muted text-muted-foreground",
        className,
      )}
    >
      <span
        aria-hidden
        className={cn("size-1.5 shrink-0 rounded-full", on ? "bg-success" : "bg-muted-foreground/50")}
      />
      {compact ? null : t(LABEL_KEY[state])}
    </span>
  );
}