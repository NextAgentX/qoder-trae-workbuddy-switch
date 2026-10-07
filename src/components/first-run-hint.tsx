/**
 * 首次引导条 —— P6 / `P6-ARCH.md` §1.5。
 *
 * ## 为什么内容要「由真实绑定派生」
 *
 * 原型引导条（`index.html:128-132`）**写死**了几条快捷键。真实应用里那样会制造
 * 「引导里列了、按了却没反应」的**假承诺**（§3.4 规则 1）。因此这里：
 * - `⌘K` **恒列**（命令面板总在）；
 * - `⌘1..N` **仅在 `planScopeShortcuts` 的 `bindings` 非空时**才出现；
 * - `/`（列表搜索，`AccountsPage` 已实现）可列。
 *
 * 降级（>5 个作用域 / 用户关闭）⇒ 那一行**根本不渲染**，与快捷键的绑定状态同源。
 *
 * ## 形态与退场
 *
 * **可关闭的顶部提示条**（非模态、不挡操作）；关闭即**持久化**（`lib/first-run.ts`），
 * 下次冷启动不再出现。刻意不做「自动退场」—— 那会让每次冷启动都重来一遍。
 */

import { useMemo, useState } from "react";
import { X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { dismissFirstRunHint, shouldShowFirstRunHint } from "@/lib/first-run";
import { useT } from "@/lib/i18n";
import { SCOPES } from "@/lib/scope";
import { planScopeShortcuts } from "@/lib/scope-shortcuts";
import { useShortcutsEnabled } from "@/lib/shortcut-preference";

/** 取可用的 `localStorage`（不可用返回 `undefined`）。 */
function safeStorage(): Storage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

export function FirstRunHint() {
  const t = useT();
  const [shortcutsEnabled] = useShortcutsEnabled();
  const [dismissed, setDismissed] = useState(() => !shouldShowFirstRunHint(safeStorage()));

  const plan = useMemo(
    () => planScopeShortcuts({ scopes: SCOPES, enabled: shortcutsEnabled }),
    [shortcutsEnabled],
  );

  if (dismissed) return null;

  // ⌘1..N 只在**绑定存在**时列出（降级 ⇒ 不写进引导，避免假承诺）。
  const scopeHint = plan.bindings.size > 0 ? `⌘1–⌘${plan.bindings.size}` : null;

  function dismiss() {
    dismissFirstRunHint(safeStorage());
    setDismissed(true);
  }

  return (
    <div className="flex items-start gap-3 border-b border-border/60 bg-muted/40 px-4 py-2.5 text-xs sm:px-6">
      <span className="shrink-0 font-medium">{t("firstRun.title")}</span>
      <ul className="min-w-0 flex-1 space-y-0.5 text-muted-foreground">
        <li>{t("firstRun.openPalette", { shortcut: "⌘K" })}</li>
        {scopeHint ? <li>{t("firstRun.scopeShortcut", { shortcut: scopeHint })}</li> : null}
        <li>{t("firstRun.listSearch")}</li>
      </ul>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 shrink-0 px-2 text-xs"
        onClick={dismiss}
      >
        <X className="size-3.5" aria-hidden="true" />
        {t("firstRun.dismiss")}
      </Button>
    </div>
  );
}
