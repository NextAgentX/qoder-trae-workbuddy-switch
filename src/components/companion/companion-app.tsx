/**
 * 桌面悬浮窗（`?window=companion`）的根组件 —— **P9 · T03**。
 *
 * ## 为什么是独立入口
 *
 * `PLAN §3.16` 把悬浮窗定为「本应用里唯一一个常驻桌面、脱离主窗口的 UI」：它是
 * **独立窗口**（Rust 侧 `companion.rs` 的 `WebviewWindow`），不是主页面里的
 * `position:fixed` 浮层。因此它**不走 Router / 侧栏 / 主窗口的任何结构**，而由
 * `main.tsx` 按 URL 查询 `?window=companion` 分流到这里。
 *
 * ## ★ 只读红线
 *
 * 悬浮窗**不含任何写操作**（§3.16 明确「不做的」：账号切换 / 批量操作 / 自定义布局）。
 * 本组件的命令面只有三种：**读**（`companion_status`，经 `useCompanionStatus`）、
 * **显隐**（`companion_set_visible`）、**聚焦**（`companion_jump`）。四态行渲染
 * （含标题数据防线、悬停信息卡）在 {@link CompanionRow}。
 *
 * ## 状态语言（§3.6）
 *
 * 首次加载 → `<Skeleton>`；无数据源 / 无会话 → `<EmptyState>`（四要素含出口「刷新」）；
 * 读取失败 → 失败空态（区别于「确实没有会话」，不静默吞错）。
 */

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { Inbox, TriangleAlert, X } from "lucide-react";

import { CompanionRow } from "@/components/companion/companion-row";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { TooltipProvider } from "@/components/ui/tooltip";
import { asError, companionJump, companionSetVisible } from "@/lib/api";
import { useCompanionStatus } from "@/lib/companion";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** 点击条目后的反馈（**明确反馈，不静默失败**，P9 B5 / ✅ #1）。 */
interface CompanionFeedback {
  kind: "ok" | "error";
  text: string;
}

/** 首次加载的骨架屏（§3.6 状态语言；读屏只播报一句，不逐条念骨架）。 */
function CompanionSkeleton({ label }: { label: string }) {
  return (
    <div className="space-y-2 p-3" aria-busy="true">
      <span className="sr-only" aria-live="polite">
        {label}
      </span>
      {[0, 1, 2].map((row) => (
        <Skeleton key={row} className="h-6 w-full" />
      ))}
    </div>
  );
}

export function CompanionApp() {
  const t = useT();
  const { snapshot, loading, error, refresh } = useCompanionStatus();
  const [feedback, setFeedback] = useState<CompanionFeedback | null>(null);
  // 防重入：一次只允许一个 `companion_jump` 在途（用 ref 而非 state，避免多余重渲染）。
  const jumpingRef = useRef(false);

  // 悬浮窗是 `transparent(true)`（圆角卡片外观，见 `companion.rs`），但 webview 内容背景
  // 默认不透明 —— 圆角会被 html / body 的方角背景盖住，透明窗口就白设了。这里把两层背景
  // 置空让透明生效。用「记录并还原内联样式」而非新增 CSS 类：`main.tsx` 在同一文档里分流，
  // 主窗口路径完全不受影响。
  useEffect(() => {
    const { documentElement, body } = document;
    const previousHtml = documentElement.style.background;
    const previousBody = body.style.background;
    documentElement.style.background = "transparent";
    body.style.background = "transparent";
    return () => {
      documentElement.style.background = previousHtml;
      body.style.background = previousBody;
    };
  }, []);

  // Esc = 隐藏窗口（P9 §1.7）。关闭 = **隐藏而非销毁**，可从侧栏左下角 / 托盘重开。
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") void companionSetVisible(false);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const items = snapshot?.items ?? [];
  // 头部徽标 = 「待确认」计数：**完全由快照数据决定**，前端不做业务判断（P9 §1.4）。
  const waitingCount = items.filter((item) => item.state === "waiting").length;

  const handleJump = useCallback(
    (id: string) => {
      if (jumpingRef.current) return;
      jumpingRef.current = true;
      const source = displayText(items.find((item) => item.id === id)?.source) ?? "";
      setFeedback(null);
      void (async () => {
        try {
          await companionJump(id);
          setFeedback({ kind: "ok", text: t("shared.companion.jump.ok", { source }) });
        } catch (cause) {
          // ★ 来源客户端未运行 ⇒ 后端返回可读错误；这里渲染成明确反馈，**不静默失败**。
          setFeedback({
            kind: "error",
            text: t("shared.companion.jump.failed", { message: asError(cause) }),
          });
        } finally {
          jumpingRef.current = false;
        }
      })();
    },
    [items, t],
  );

  const handleClose = useCallback(() => {
    void companionSetVisible(false);
  }, []);

  const refreshButton = (
    <Button type="button" variant="outline" size="sm" onClick={() => void refresh()}>
      {t("shared.companion.empty.refresh")}
    </Button>
  );

  let content: ReactNode;
  if (loading && !snapshot) {
    content = <CompanionSkeleton label={t("shared.companion.loading")} />;
  } else if (!snapshot && error) {
    // 读取失败 ≠ 没有会话：给**失败空态**（含出口），不静默吞错。
    content = (
      <EmptyState
        icon={TriangleAlert}
        title={t("shared.companion.error.title")}
        description={error}
        action={refreshButton}
      />
    );
  } else if (items.length === 0) {
    content = (
      <EmptyState
        icon={Inbox}
        title={t("shared.companion.empty.title")}
        description={t("shared.companion.empty.description")}
        action={refreshButton}
      />
    );
  } else {
    content = (
      <ul className="py-1">
        {items.map((item) => (
          <li key={item.id}>
            <CompanionRow item={item} onJump={handleJump} />
          </li>
        ))}
      </ul>
    );
  }

  return (
    <TooltipProvider delayDuration={0}>
      <div className="flex h-screen w-screen flex-col overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-lg">
        {/* 拖动区：窗口 `decorations(false)` 没有系统标题栏，靠 `data-tauri-drag-region`
            触发**原生**拖动（不产生可聚焦元素，不抢焦点）。Rust 侧的位置记忆
            （`WindowEvent::Moved`）正是依赖这条拖动路径。关闭按钮**不带**该属性 ⇒ 可点。 */}
        <header data-tauri-drag-region className="flex select-none items-center gap-2 px-3 py-2">
          <span data-tauri-drag-region className="text-xs font-medium text-muted-foreground">
            {t("shared.companion.title")}
          </span>
          {waitingCount > 0 ? (
            <span
              data-tauri-drag-region
              className="rounded-full bg-amber-500/15 px-1.5 text-[10px] font-medium leading-4 text-amber-600"
            >
              {t("shared.companion.badge.waiting", { count: waitingCount })}
            </span>
          ) : null}
          <span data-tauri-drag-region className="h-4 min-w-0 flex-1" />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-5 shrink-0"
            aria-label={t("shared.companion.close")}
            onClick={handleClose}
          >
            <X className="size-3" aria-hidden="true" />
          </Button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto">{content}</div>

        {feedback ? (
          <p
            role={feedback.kind === "error" ? "alert" : "status"}
            aria-live="polite"
            className={cn(
              "px-3 pb-1 text-[11px] leading-4",
              feedback.kind === "error" ? "text-destructive" : "text-muted-foreground",
            )}
          >
            {feedback.text}
          </p>
        ) : null}

        <footer className="border-t border-border/60 px-3 py-1.5">
          <p className="text-[10px] leading-4 text-muted-foreground">
            {t("shared.companion.footnote")}
          </p>
        </footer>
      </div>
    </TooltipProvider>
  );
}
