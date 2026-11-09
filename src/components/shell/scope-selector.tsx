import { ChevronDown, LayoutGrid } from "lucide-react";
import { useState } from "react";

import { ScopeMark, scopeLabel } from "@/components/scope-chip";
import { RunPill } from "@/components/shell/run-pill";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useT } from "@/lib/i18n";
import { AGGREGATE_SHORTCUT_HINT } from "@/lib/scope-shortcuts";
import { SCOPES, type Scope, scopeParts } from "@/lib/scope";
import type { Product } from "@/lib/platforms/registry";
import type { ScopeStatus, ScopeStatusMap } from "@/lib/use-scope-status";
import { cn } from "@/lib/utils";
import { useVisibleScopes } from "@/lib/scope-visibility";
import { useAggregateAccountTotal } from "@/lib/use-aggregate-accounts";

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
 * 序号来自 `planScopeShortcuts` 的绑定结果 —— 降级（>9 个作用域或用户关闭）时 Map 为空
 * ⇒ 提示**自动消失**（不显示一个按了没反应的假快捷键）。快捷键本身在 `Layout` 里注册。
 */
export function ScopeSelector({
  scope,
  statuses,
  shortcutBindings,
  onSelect,
  aggregate = false,
  onSelectAggregate,
}: {
  scope: Scope;
  statuses: ScopeStatusMap;
  shortcutBindings: ReadonlyMap<Scope, number>;
  onSelect: (next: Scope) => void;
  /** 是否处于聚合视图（issue #50）：为 true 时高亮「聚合」项，6 个作用域项都不高亮。 */
  aggregate?: boolean;
  /** 进入聚合视图。不传 ⇒ 不渲染「聚合」入口（例如未来某个只读场景）。 */
  onSelectAggregate?: () => void;
}) {
  const t = useT();
  const visibleScopes = useVisibleScopes((s) => s.visibleScopes);
  /** 菜单是否展开（只在展开时才去取聚合总数）。 */
  const [menuOpen, setMenuOpen] = useState(false);
  /* 聚合账号总数：与聚合页**同一份快照**（见 use-aggregate-accounts.ts 的说明）。 */
  const { total: aggregateTotal } = useAggregateAccountTotal(menuOpen);

  /**
   * 聚合项的副标题：**跨 6 个作用域**。
   *
   * ⚠️ **账号总数不在这儿算**：这里若用 `statuses` 求和，会与聚合页的数字对不上 ——
   * `useScopeStatus` 的 WB 侧只读 store 的**当前区域切片**（没进过国际版账号页时
   * 国际版是 0），而聚合页数的是自己拉的 6 个作用域。两个数据源 ⇒ 两个数。
   * 所以总数由 `aggregateTotal`（**同一份快照**）提供，与聚合页逐字一致。
   *
   * ⚠️ `attentionCount` 任一为 `null`/缺失（未知）⇒ **不显示需处理数**，而不是当 0 求和。
   * 拿未知冒充 0 会让「9 个需处理」在某些时刻**变小**，而用户无从知道原因。
   */
  const aggregateAttention = SCOPES.every((item) => statuses[item]?.attentionCount != null)
    ? SCOPES.reduce((sum, item) => sum + (statuses[item]?.attentionCount ?? 0), 0)
    : null;

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

  /**
   * 聚合项的副标题：`N 个账号 · 跨 6 个作用域`（+ 需处理数，未知时不缀）。
   *
   * 与 6 个作用域项的 `subOf` **刻意分开**：那些说的是「这一套账号」的登录态，
   * 这里说的是「全部账号」的规模 —— 混用一个函数会让聚合项显示「未登录 · 0 个账号」。
   */
  /**
   * 聚合项的副标题。
   *
   * @param total 账号总数（`null` = 还没取到 ⇒ **不显示数字**，而不是显示 0）
   */
  function aggregateSub(total: number | null, attention: number | null): string {
    const base = total === null
      ? t("aggregate.scopeMenu.entrySubUnknown", { scopes: SCOPES.length })
      : t("aggregate.scopeMenu.entrySub", { n: total, scopes: SCOPES.length });
    return attention != null && attention > 0
      ? `${base} · ${t("aggregate.scopeMenu.entryAttention", { n: attention })}`
      : base;
  }

  return (
    <DropdownMenu
      onOpenChange={(open) => {
        /* 菜单打开时才去取聚合总数：菜单外那个数字没人看，
           而 6 个作用域的账号列表是 6 次后端调用。 */
        setMenuOpen(open);
      }}
    >
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          data-testid="scope-trigger"
          aria-label={t("topbar.scope.aria")}
          /* 与原型 `.scope-trigger`（`demo.css:91-97`）逐项对齐：
             `background: var(--card-2)`（凹陷面，**不是** muted/40 —— 深色下 muted 与 card 同值会隐形）、
             hover 时 `background: var(--accent)` + `border-color: var(--border-strong)`。
             ⚠️ 展开环是 `0 0 0 3px ring 22%`（原型 `:97`），此前写 2px/20%，环细了一圈。
             ⚠️ `shrink-0` 已去掉：桌面端最小窗口720px（`tauri.conf.json` 的 `minWidth`），
             若作用域按钮不可收缩，300px 上限会把右侧图标钮挤出视口。改成可收缩 +
             `min-w-[132px]`；内部两行本就都有 `truncate` 兜底，宽屏仍是原型观感。 */
          className="flex h-10 min-w-[132px] max-w-[300px] shrink items-center gap-2.5 rounded-[12px] border border-border bg-card-2 pl-2 pr-2.5 text-left outline-none transition-colors hover:border-border-strong hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/40 data-[state=open]:border-ring data-[state=open]:ring-[3px] data-[state=open]:ring-ring/22"
        >
          <ScopeMarkOrAggregate aggregate={aggregate} product={current.product} />
          <span className="flex min-w-0 flex-col leading-tight">
            <span className="truncate text-[12.5px] font-semibold">
              {aggregate ? t("aggregate.scopeMenu.entryName") : nameOf(scope)}
            </span>
            <span className="truncate text-[10.5px] text-muted-foreground">
              {aggregate ? aggregateSub(aggregateTotal, aggregateAttention) : subOf(scope)}
            </span>
          </span>          <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[340px] rounded-[14px] p-1.5">
        {/* ★「聚合」是**第一项**，并用分隔线 + 独立分组标题与 6 个作用域隔开（issue #50 D1）。
            两个理由：
              · 它是最高频入口（默认想看的就是全量），排在 6 个作用域之后意味着每次都要扫完菜单；
              · 它**不是第 7 个作用域** —— 分隔线让「聚合 ≠ 作用域」在视觉上成立，
                否则用户会以为多了一个能切的作用域（而 `Scope` 是封闭字面量，根本没有这一项）。 */}
        {onSelectAggregate ? (
          <>
            <div className="px-2.5 pb-1 pt-2 text-[10.5px] font-medium tracking-[0.02em] text-muted-foreground">
              {t("aggregate.scopeMenu.group")}
            </div>
            <DropdownMenuItem
              /* `data-aggregate` 是门禁锚点：它证明「菜单第一项是聚合」且可与作用域项区分。 */
              data-aggregate="true"
              onSelect={() => onSelectAggregate()}
              className={cn(
                "gap-2.5 rounded-[10px] px-2.5 py-2",
                aggregate && "bg-primary/[0.13]",
              )}
            >
              <span className="grid size-7 shrink-0 place-items-center rounded-[9px] bg-primary/20 text-primary">
                <LayoutGrid className="size-3.5" />
              </span>
              <span className="flex min-w-0 flex-1 flex-col leading-tight">
                <span className="truncate text-[12.5px] font-medium">
                  {t("aggregate.scopeMenu.entryName")}
                </span>
                <span className="truncate text-[11px] text-muted-foreground">
                  {aggregateSub(aggregateTotal, aggregateAttention)}
                </span>
              </span>
              {/* 与 6 个作用域项的 `⌘N` 同位。文案来自 `AGGREGATE_SHORTCUT_HINT` —— 与
                  `App.tsx` 的全局键**同源**，改键位只需改那一处，提示不会与实现漂移。 */}
              <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">
                {AGGREGATE_SHORTCUT_HINT}
              </span>
            </DropdownMenuItem>
            <div className="my-1 h-px bg-border" />
          </>
        ) : null}
        {/* 分组标题逐值对齐原型 `.scope-menu .grp`（`:448`）：内边距 8/10/4、字号 10.5、字距 .02em。 */}
        <div className="px-2.5 pb-1 pt-2 text-[10.5px] font-medium tracking-[0.02em] text-muted-foreground">
          {t("topbar.scope.menuLabel")}
        </div>
        {SCOPES.filter((s) => visibleScopes.includes(s)).map((s) => {
          const status: ScopeStatus = statuses[s];
          const index = shortcutBindings.get(s);
          return (
            <DropdownMenuItem
              key={s}
              onSelect={() => onSelect(s)}
              /* 原型 `.scope-item`（`:449-460`）：内边距 8/10、圆角 10、gap 10；
                 选中态 `primary 13%`（不是 10%）。聚合态下 6 项都**不**高亮。 */
              className={cn(
                "gap-2.5 rounded-[10px] px-2.5 py-2",
                !aggregate && s === scope && "bg-primary/[0.13]",
              )}
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

/**
 * 触发按钮左侧的图标：普通作用域用平台图标，聚合态用四宫格。
 *
 * ⚠️ 聚合**不能**复用某个平台图标 —— 那会让顶栏读起来像「当前在 WorkBuddy」，
 * 而聚合横跨三个平台。四宫格在这里是唯一不撒谎的选择。
 */
function ScopeMarkOrAggregate({ aggregate, product }: { aggregate: boolean; product: Product }) {
  if (!aggregate) return <ScopeMark product={product} size={26} />;
  return (
    <span className="grid size-[26px] shrink-0 place-items-center rounded-[8px] bg-primary/20 text-primary">
      <LayoutGrid className="size-4" />
    </span>
  );
}