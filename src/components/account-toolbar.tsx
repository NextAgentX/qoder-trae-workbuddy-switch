import type { Ref } from "react";
import { LayoutGrid, Plus, RefreshCw, Search, Table2, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { DemoAction } from "@/components/demo-action";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import type { AccountViewMode } from "@/lib/account-list-view";
import { useT } from "@/lib/i18n";

/**
 * 账号列表工具栏（**三平台共用壳**：WorkBuddy / Trae / Qoder）。
 *
 * ## 为什么共用而不是各写一份
 *
 * 逐项核对过 WB 与 Trae：搜索 / 排序 / 分组 / 视图四个核心控件的**结构完全相同**，
 * 差异**全部是「选项数据 + 开关数据」**，**没有任何行为差异**（见 `P4-TOOLBAR-ARCH §1.3`）。
 * 这与 `account-pool-card`（差异只有 tiles 数量）同类，因此共用。
 *
 * ⚠️ **反向约束**：本组件内**禁止**出现 `platform === ...` 判断 —— 平台差异一律走
 * `sortOptions` / `groupOptions`（数据）与 `addAction`（主操作）。若哪天这里冒出平台分支，
 * 说明合并过度，应退回「两份 + 抽共享片段」。
 *
 * ## ★ 工具栏 = 原型那一行 + 一枚「常驻小控件」（2026-10-12 定稿）
 *
 * 原型的 `viewAccounts` 工具栏（`buddy-switch-prototype.html:2406-2429`）只有：
 * 搜索 · 排序 · 分组 · 视图 `.seg` · `[grow]` · 全选 · `+ 添加账号`。
 *
 * 本仓多出的入口**先收进「⋯」溢出菜单，再整体删掉那枚菜单**，改由「添加账号」右侧抽屉
 * 承载（`add-account-drawer.tsx` 的 `extraOptions` / `moreTools`）。
 *
 * ⚠️★ 用户要求「**回收站放出来**」⇒ 这一枚**例外**：它是**常驻**控件，
 * 用**结构化 prop**（`onOpenRecycleBin`）注入、摆在视图切换之后。
 * 其余入口（自动签到 / 第二枚开关 / 导出备份 / 刷新）仍在抽屉的「更多操作」里。
 *
 * ⇒ ⚠️ **不要再往这里加控件槽位**（`children` / `secondaryActions` 这类 ReactNode 插槽已删，
 * 且门禁钉住）。要加入口，加进那个抽屉；确实需要常驻的，加**结构化 prop** 并同步门禁。
 *
 * ## ★ 搜索框必须**常驻挂载**（IME 约束，`§1.4`）
 *
 * 受控 `<Input>` 的 DOM 节点在 `query` 变化时**不得被 unmount**，否则中文输入法连续
 * 组词会丢字（原型里有一条可证伪断言，`verify-demo.cjs:138-141`）。因此：
 * - 搜索框**永远渲染**（即使结果为空），**不得**放进「结果为空就换掉整块」的条件分支；
 * - 本组件的 JSX 里，`<Input>` 是直接子节点，**不**处于任何条件表达式中。
 *
 * 工具栏与列表是两个独立子树：`query` 只驱动列表子树重渲染，工具栏结构不变。
 */
export interface ToolbarOption<V extends string = string> {
  value: V;
  /** 已 i18n。 */
  label: string;
}

export interface AccountToolbarProps {
  /** 搜索（受控）。 */
  query: string;
  onQueryChange: (next: string) => void;
  /** 搜索框 ref（`/` 快捷键聚焦；由页面持有并聚焦）。 */
  searchInputRef?: Ref<HTMLInputElement>;

  /** 排序（受控；值集由 `sortOptions` 定义）。 */
  sort: string;
  sortOptions: ToolbarOption[];
  onSortChange: (next: string) => void;

  /** 分组（受控；**分桶**，与「分组筛选」不同）。 */
  group: string;
  groupOptions: ToolbarOption[];
  onGroupChange: (next: string) => void;

  /** 视图（受控）。 */
  view: AccountViewMode;
  onViewChange: (next: AccountViewMode) => void;
  /** 表格视图是否可用（< 900px 强制卡片 ⇒ false ⇒ 禁用表格按钮并给原因）。 */
  tableViewEnabled?: boolean;

  /** 全选控制（列表支持勾选时传入）。 */
  selection?: {
    allSelected: boolean;
    someSelected: boolean;
    onToggleAll: () => void;
    /** 已 i18n：「全选」/「取消全选」。 */
    label: string;
  };

  /**
   * 工具栏右侧的**主操作**（如「+ 添加账号」，原型 `viewAccounts` :2428）。
   *
   * 由页面注入**同一个**既有入口（两平台都接「添加账号」抽屉），壳内不含任何平台 / 业务逻辑。
   * 不传 = 不渲染该按钮。
   */
  addAction?: { label: string; onClick: () => void } | null;

  /** Provider-scoped account refresh; omitted when the page's refresh is elsewhere. */
  refreshAction?: { label: string; onClick: () => void; disabled?: boolean; busy?: boolean } | null;

  /**
   * ★ 回收站入口（**常驻控件**，用户要求「回收站放出来」）。
   *
   * 走**结构化 prop** 而不是 ReactNode 插槽，几何与文案都留在这个壳里，两平台不可能漂移；
   * 两平台都有 `RecycleBinSheet`，按**当前作用域**过滤。
   */
  onOpenRecycleBin?: () => void;
}

export function AccountToolbar({
  query,
  onQueryChange,
  searchInputRef,
  sort,
  sortOptions,
  onSortChange,
  group,
  groupOptions,
  onGroupChange,
  view,
  onViewChange,
  tableViewEnabled = true,
  selection,
  addAction,
  refreshAction,
  onOpenRecycleBin,
}: AccountToolbarProps) {
  const t = useT();

  return (
    <TooltipProvider delayDuration={400}>
      <div className="mb-3.5 flex flex-wrap items-center gap-2 rounded-[12px] border border-border bg-card px-3 py-2.5">
        {/* ★ 搜索框常驻：**不**放进任何条件分支（IME 约束，见文件头）。
            结构与原型 `label.field.search`（`demo.css:280-292`）一致：
            图标与输入**同处一个 32px 高的字段**里（`gap:6px` + `padding:0 10px`），
            不是「把图标绝对定位、再给输入框留 `pl-8`」——后者左内边距是 32px，
            比原型的 10+14+6=30px 多 2px，且图标与文字不在同一条基线上。 */}
        <label className="flex h-8 min-w-[180px] flex-1 items-center gap-1.5 rounded-[9px] border border-border bg-card-2 px-2.5 text-muted-foreground focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/20">
          <Search className="size-3.5 shrink-0" aria-hidden="true" />
          <Input
            ref={searchInputRef}
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder={t("shared.toolbar.searchPlaceholder")}
            aria-label={t("shared.toolbar.searchAria")}
            autoComplete="off"
            spellCheck={false}
            className="h-auto min-w-0 flex-1 border-0 bg-transparent p-0 text-[12.5px] text-foreground shadow-none focus-visible:ring-0"
          />
        </label>

        {/* 排序（分桶的前置：先排后分，桶内顺序才稳定）。
            原型 `label.field` 里**先写「排序」再跟下拉**（不是只有一枚下拉）。 */}
        <div className="flex h-8 items-center gap-1.5 rounded-[9px] border border-border bg-card-2 px-2.5 text-[12.5px] text-muted-foreground">
          <span className="shrink-0">{t("shared.toolbar.sort")}</span>
          <Select value={sort} onValueChange={onSortChange}>
            <SelectTrigger
              size="sm"
              className="h-auto w-auto min-w-[83px] gap-0 border-0 bg-transparent px-0 py-0 text-[12.5px] shadow-none focus-visible:ring-0"
              aria-label={t("shared.toolbar.sortAria")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sortOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {/* 分组（**分桶**：none / program / status / group）。 */}
        <div className="flex h-8 items-center gap-1.5 rounded-[9px] border border-border bg-card-2 px-2.5 text-[12.5px] text-muted-foreground">
          <span className="shrink-0">{t("shared.toolbar.group")}</span>
          <Select value={group} onValueChange={onGroupChange}>
            <SelectTrigger
              size="sm"
              className="h-auto w-auto min-w-[83px] gap-0 border-0 bg-transparent px-0 py-0 text-[12.5px] shadow-none focus-visible:ring-0"
              aria-label={t("shared.toolbar.groupAria")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {groupOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {/* 视图切换：**纯图标**。原型 `.seg button` 里只有 `ICON.grid` / `ICON.list`，
            没有文字（`buddy-switch-prototype.html:2421-2424`）—— 文字改由
            `title` / `aria-label` 承担，按钮因此从 62px 收到 38px。
            窄窗下表格按钮**禁用并说明原因**（不是隐藏）。 */}
        <div
          role="group"
          aria-label={t("shared.toolbar.viewAria")}
          className="inline-flex items-center gap-0.5 rounded-[10px] bg-muted p-0.5"
        >
          <Button
            type="button"
            variant={view === "card" ? "secondary" : "ghost"}
            size="sm"
            className="h-[26px] rounded-[8px] has-[>svg]:px-[11px] data-[on=true]:bg-card data-[on=true]:font-[550] data-[on=true]:shadow-sm"
            aria-pressed={view === "card"}
            aria-label={t("shared.toolbar.viewCard")}
            title={t("shared.toolbar.viewCard")}
            data-on={view === "card"}
            onClick={() => onViewChange("card")}
          >
            <LayoutGrid className="size-4" />
          </Button>
          <Tooltip>
            <TooltipTrigger asChild>
              <span>
                <Button
                  type="button"
                  variant={view === "table" ? "secondary" : "ghost"}
                  size="sm"
                  className="h-[26px] rounded-[8px] has-[>svg]:px-[11px] data-[on=true]:bg-card data-[on=true]:font-[550] data-[on=true]:shadow-sm"
                  aria-pressed={view === "table"}
                  aria-label={t("shared.toolbar.viewTable")}
                  title={t("shared.toolbar.viewTable")}
                  data-on={view === "table"}
                  disabled={!tableViewEnabled}
                  onClick={() => onViewChange("table")}
                >
                  <Table2 className="size-4" />
                </Button>
              </span>
            </TooltipTrigger>
            {!tableViewEnabled && (
              <TooltipContent side="top">{t("shared.toolbar.viewTableDisabled")}</TooltipContent>
            )}
          </Tooltip>
        </div>

        {/* ★ 回收站—— 用户要求「回收站放出来」⇒ 常驻工具栏（P5 §2.7.1）。
            按**当前作用域**过滤（只列本作用域被软删除的账号）；入口放账号页而非设置页，
            避免与 P7「备份与恢复」的整库级语义混淆。 */}
        {onOpenRecycleBin ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span>
                <DemoAction>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-[34px] rounded-lg"
                    onClick={onOpenRecycleBin}
                    aria-label={t("recycleBin.title")}
                  >
                    <Trash2 />
                  </Button>
                </DemoAction>
              </span>
            </TooltipTrigger>
            <TooltipContent side="top">{t("recycleBin.title")}</TooltipContent>
          </Tooltip>
        ) : null}

        {/* ★ `grow` 把右侧分组推到行尾（原型 `<div class="grow">` `:2425`）。
            ⚠️ 有了它，主操作**不再**需要 `ml-auto` —— 两个都写会让自由空间被分两半。 */}
        <div className="grow" />

        {refreshAction ? (
          <Button type="button" variant="outline" size="sm" onClick={refreshAction.onClick} disabled={refreshAction.disabled}>
            {refreshAction.busy ? <RefreshCw className="animate-spin" /> : <RefreshCw />}
            {refreshAction.label}
          </Button>
        ) : null}

        {/* 全选：原型是**一枚 ghost 文字按钮**（`btn ghost sm`，`:2426`），
            不是「勾选框 + 文字」。勾选框仍保留在表格表头（`.check.mixed`）。 */}
        {selection ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            /* ★ 原型 `.btn.ghost` = `.btn` 的 `border: 1px solid var(--border)`
               + `border-color: transparent`（`demo.css:194/201`）⇒ 盒宽 46 而非 44。
               我们的 `ghost` 变体没有边框，这里补回来，尺寸才与原型一致。 */
            className="border border-transparent"
            aria-pressed={selection.allSelected}
            onClick={() => selection.onToggleAll()}
          >
            {selection.label}
          </Button>
        ) : null}

        {/* ★ 主操作（原型 `viewAccounts` :2428：工具栏行最右侧的 `.btn.primary`）。
            与「添加与迁移账号」横幅的入口**同一动作**，不重复一套流程。
            尺寸用 `size="default"`（高 34 / 圆角 10 / 字号 12.5 = 原型 `.btn`），
            内边距补回 `13px`：`Button` 基础样式对「带图标」会收到 11px，而原型 `.btn`
            没有 `has(svg)` 规则、恒为 13px。 */}
        {addAction ? (
          <DemoAction>
            <Button
              type="button"
              className="has-[>svg]:px-[13px]"
              onClick={addAction.onClick}
            >
              <Plus className="size-4" />
              {addAction.label}
            </Button>
          </DemoAction>
        ) : null}
      </div>
    </TooltipProvider>
  );
}
