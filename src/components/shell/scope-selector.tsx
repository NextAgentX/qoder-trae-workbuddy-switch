import { ChevronDown } from "lucide-react";

import { ScopeMark, scopeLabel } from "@/components/scope-chip";
import { RunPill } from "@/components/shell/run-pill";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useT } from "@/lib/i18n";
import { SCOPES, type Scope, scopeParts } from "@/lib/scope";
import type { ScopeStatus, ScopeStatusMap } from "@/lib/use-scope-status";
import { cn } from "@/lib/utils";

/**
 * **L1 作用域选择器** —— 顶栏左一（§3.1）。
 *
 * ## 为什么是顶栏左一的菜单，而不是侧栏的 Tab
 *
 * 作用域 = 平台 × 区域，是「我现在管的是哪一套账号」的**全局上下文**。它决定侧栏与
 * 内容区看到的**全部**数据，因此必须是一个**位置固定、不随产品换血**的控件。
 * 侧栏的产品 Tab（旧 `ProductSwitch`）会把作用域拆成两层，且侧栏 220px 放不下
 * 「已登录 xxx · N 个账号」这类副标题 —— 正是原型把它压成顶栏一枚菜单按钮的原因。
 *
 * ## 与「区域切换器」的关系
 *
 * 页面内**不再有**区域选择器（`ScopeSwitch` 已删）：这里是切作用域的**唯一**入口。
 * 选中的作用域 = `writeLastScope` + 导航到 `/s/{scope}/{dest}`，URL 仍是唯一真值。
 *
 * ## `⌘N` 只是提示
 *
 * 序号来自 `planScopeShortcuts` 的绑定结果 —— 降级（>5 个作用域或用户关闭）时 Map 为空
 * ⇒ 提示**自动消失**（不显示一个按了没反应的假快捷键）。快捷键本身在 `Layout` 里注册。
 */
export function ScopeSelector({
  scope,
  statuses,
  shortcutBindings,
  onSelect,
}: {
  scope: Scope;
  statuses: ScopeStatusMap;
  shortcutBindings: ReadonlyMap<Scope, number>;
  onSelect: (next: Scope) => void;
}) {
  const t = useT();

  const nameOf = (s: Scope): string => scopeLabel(t, s);

  const subOf = (s: Scope): string => {
    const status = statuses[s];
    const n = status.accountCount;
    const base = status.loggedInName
      ? t("topbar.scope.sub.loggedIn", { name: status.loggedInName, n })
      : t(status.installed ? "topbar.scope.sub.loggedOut" : "topbar.scope.sub.missing", { n });
    /* 原型 `scope-menu`（`buddy-switch-prototype.html:3889`）：有需处理项时再缀一段 ——
       目的是「切之前就能看出哪个作用域有账号待处理」，不必先切过去看账号页。
       ⚠️ `attentionCount === null`（未知）⇒ **不缀**，而不是写「0 个需处理」（那是撒谎）。 */
    return status.attentionCount != null && status.attentionCount > 0
      ? base + t("topbar.scope.sub.attention", { n: status.attentionCount })
      : base;
  };

  const current = scopeParts(scope);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={t("topbar.scope.aria")}
          /* 与原型 `.scope-trigger`（`:110-116`）逐项对齐：
             `background: var(--card-2)`（凹陷面，**不是** muted/40 —— 深色下 muted 与 card 同值会隐形）、
             hover 时 `background: var(--accent)` + `border-color: var(--border-strong)`。 */
          className="flex h-10 max-w-[300px] shrink-0 items-center gap-2.5 rounded-[12px] border border-border bg-card-2 pl-2 pr-2.5 text-left outline-none transition-colors hover:border-border-strong hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/40 data-[state=open]:border-ring data-[state=open]:ring-2 data-[state=open]:ring-ring/20"
        >
          <ScopeMark product={current.product} size={26} />
          <span className="flex min-w-0 flex-col leading-tight">
            <span className="truncate text-[12.5px] font-semibold">{nameOf(scope)}</span>
            <span className="truncate text-[10.5px] text-muted-foreground">{subOf(scope)}</span>
          </span>
          <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[340px] rounded-[14px] p-1.5">
        {/* 分组标题逐值对齐原型 `.scope-menu .grp`（`:448`）：内边距 8/10/4、字号 10.5、字距 .02em。 */}
        <div className="px-2.5 pb-1 pt-2 text-[10.5px] font-medium tracking-[0.02em] text-muted-foreground">
          {t("topbar.scope.menuLabel")}
        </div>
        {SCOPES.map((s) => {
          const status: ScopeStatus = statuses[s];
          const index = shortcutBindings.get(s);
          return (
            <DropdownMenuItem
              key={s}
              onSelect={() => onSelect(s)}
              /* 原型 `.scope-item`（`:449-460`）：内边距 8/10、圆角 10、gap 10；
                 选中态 `primary 13%`（不是 10%）。 */
              className={cn("gap-2.5 rounded-[10px] px-2.5 py-2", s === scope && "bg-primary/[0.13]")}
            >
              <ScopeMark product={scopeParts(s).product} size={28} />
              <span className="flex min-w-0 flex-1 flex-col leading-tight">
                <span className="truncate text-[12.5px] font-medium">{nameOf(s)}</span>
                <span className="truncate text-[11px] text-muted-foreground">{subOf(s)}</span>
              </span>
              <RunPill status={status} compact />
              {index ? (
                <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">⌘{index}</span>
              ) : null}
            </DropdownMenuItem>
          );
        })}
        {/* 原型 `scope-menu` 底部说明行（`buddy-switch-prototype.html:3893`）：
            「作用域决定我现在管的是哪一套账号」—— 一句话解释切换的影响面。 */}
        <div className="mt-1 border-t border-border/60 px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
          {t("topbar.scope.hint")}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}