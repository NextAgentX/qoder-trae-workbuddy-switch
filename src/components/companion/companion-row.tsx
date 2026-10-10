/**
 * 桌面悬浮窗的**单行**（P9 · T03）—— 只读，点击 = 聚焦来源客户端。
 *
 * ## ★ 数据防线：完整标题**默认不进 DOM**（P9 ✅ #3）
 *
 * 这条行为的目的是**防截图泄露**。原型的 `filter: blur` 只是**视觉**防线 ——
 * 完整标题**仍在 DOM 里**，DevTools / 无障碍树 / 任何读 DOM 的途径都能拿到，
 * 等于只堵住了截图这一个出口。这里改为：**未悬停 / 未聚焦时渲染占位符**，
 * 完整标题**根本不进 DOM**，只有悬停 / `:focus-visible` 才把明文写进 DOM。
 *
 * ⚠️ **无障碍代价与处理**：占位符下读屏拿不到标题 ⇒ 用 `aria-label` 给一个
 * **脱敏后的**描述（来源 / 状态 / 时间，**不含标题**）。**绝不**因为无障碍就把明文塞回去 ——
 * 那会重新打开刚被堵住的出口。
 *
 * ## ★ 只读
 *
 * 整行是一个 `<button>`，唯一动作是 `onJump`（聚焦来源客户端，P9 ✅ #1）。
 * **不切账号、不导航主窗口、不删改任何数据** —— 本文件不含任何写操作。
 *
 * ## 四态由数据决定，前端不做业务判断
 *
 * 状态点样式 / 文案**完全由 `item.state` 决定**（P9 §1.4）；前端不推断「什么算运行中」。
 */

import { useCallback, useState, type FocusEvent } from "react";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { CompanionItem, CompanionState } from "@/lib/companion";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { TranslationKey } from "@/locales/zh";

/**
 * 未悬停 / 未聚焦时渲染的占位符 —— 完整标题**根本不进 DOM**（P9 ✅ #3）。
 *
 * 固定长度的圆点：既不随标题长度变化（不泄露长度），也一眼看出「这里被遮住了」。
 */
export const COMPANION_TITLE_PLACEHOLDER = "••••••••";

/**
 * 四态 → 状态点样式。
 *
 * ★ 四个值**两两不同**：这样「四态可区分」是**结构上成立**的，不靠数据凑。
 * `running` 用 primary + 脉冲；`waiting` amber；`done` emerald；`failed` destructive（P9 §1.4）。
 *
 * ⚠️ 脉冲是**持续动画**，须尊重 `prefers-reduced-motion`（PLAN §3.6 无障碍基线）——
 * 故 `running` 额外挂 `motion-reduce:animate-none`（减弱动效时静置为实心点，状态仍可辨）。
 */
export const COMPANION_STATE_DOT_CLASS: Record<CompanionState, string> = {
  running: "bg-primary animate-pulse motion-reduce:animate-none",
  waiting: "bg-warning",
  done: "bg-success",
  failed: "bg-destructive",
};

/** 四态 → 状态文案键（与后端 `CompanionState` 的 lowercase 序列化一一对应）。 */
const STATE_LABEL_KEY: Record<CompanionState, TranslationKey> = {
  running: "shared.companion.state.running",
  waiting: "shared.companion.state.waiting",
  done: "shared.companion.state.done",
  failed: "shared.companion.state.failed",
};

/** `CompanionRow` 的入参。 */
export interface CompanionRowProps {
  /** 只读行数据（来自 `companion_status` 快照）。 */
  item: CompanionItem;
  /** 点击行 → 聚焦来源客户端（P9 ✅ #1）。**不切账号、不改数据**。 */
  onJump: (id: string) => void;
}

/**
 * 单行：状态点 + 来源 + **占位符标题** + 时间；悬停 / 聚焦展开完整标题 + 信息卡。
 *
 * ★ 完整标题只在 `revealed`（悬停 / `:focus-visible`）时求值；未展开时求值的是
 * {@link COMPANION_TITLE_PLACEHOLDER} —— 明文**不进 DOM**（见文件头文档）。
 */
export function CompanionRow({ item, onJump }: CompanionRowProps) {
  const t = useT();
  const [revealed, setRevealed] = useState(false);

  const source = displayText(item.source) ?? "";
  const hint = displayText(item.hint) ?? "";
  // fail-closed：脏数据里出现四态之外的值时，按 `failed` 呈现（不静默、不崩）。
  const dotClass = COMPANION_STATE_DOT_CLASS[item.state] ?? COMPANION_STATE_DOT_CLASS.failed;
  const stateLabel = t(STATE_LABEL_KEY[item.state] ?? STATE_LABEL_KEY.failed);

  // ★ 完整标题**只在 revealed 时求值**：未悬停 / 未聚焦时它是占位符，明文根本不进 DOM。
  const title = revealed ? displayText(item.title) ?? "" : COMPANION_TITLE_PLACEHOLDER;
  // ★ 无障碍描述**脱敏**：只报来源 / 状态 / 时间，**不含标题**（不要把明文塞回无障碍树）。
  const ariaLabel = t("shared.companion.row.aria", { source, state: stateLabel, hint });

  // `:focus-visible` 判定：**键盘**聚焦才展开，程序化 / 鼠标聚焦不展开
  //（否则窗口一打开就自动聚焦首行，会把标题直接泄露出去 —— 与 ✅ #3 冲突）。
  const handleFocus = useCallback((event: FocusEvent<HTMLButtonElement>) => {
    if (event.currentTarget.matches(":focus-visible")) setRevealed(true);
  }, []);

  return (
    // 受控 Tooltip：`open` 完全由本组件的 `revealed` 驱动（**不接 Radix 的 focus-open**），
    // 这样「程序化聚焦不展开」这条隐私纪律不会被 Radix 的默认行为破坏。
    <Tooltip open={revealed}>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={() => onJump(item.id)}
          onMouseEnter={() => setRevealed(true)}
          onMouseLeave={() => setRevealed(false)}
          onFocus={handleFocus}
          onBlur={() => setRevealed(false)}
          aria-label={ariaLabel}
          className="flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left outline-none transition-colors hover:bg-accent/60 focus-visible:bg-accent/60 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
        >
          <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-full", dotClass)} />
          <span className="shrink-0 truncate text-xs text-muted-foreground">{source}</span>
          <span
            className={cn(
              "min-w-0 flex-1 truncate text-sm",
              revealed ? "text-foreground" : "tracking-[0.2em] text-muted-foreground/70",
            )}
          >
            {title}
          </span>
          <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">{hint}</span>
        </button>
      </TooltipTrigger>
      {/* 信息卡（P9 §1.6）：完整标题 + 来源 + 时间。内容随 `revealed` 求值，
          故**关闭动画期间也不会残留明文**（`title` 已回落为占位符）。 */}
      <TooltipContent side="top" align="start" className="max-w-[16rem] space-y-0.5">
        <div className="text-[13px] font-medium">{title}</div>
        <div className="text-muted-foreground">
          {t("shared.companion.info.source")} · {source}
        </div>
        <div className="text-muted-foreground">
          {t("shared.companion.info.time")} · {hint}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
