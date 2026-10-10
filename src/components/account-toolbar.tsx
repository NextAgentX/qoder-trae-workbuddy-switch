import type { ReactNode, Ref } from "react";
import { LayoutGrid, Plus, Search, Table2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DemoAction } from "@/components/demo-action";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import type { AccountViewMode } from "@/lib/account-list-view";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * 账号列表工具栏（**两平台共用壳**）。
 *
 * ## 为什么共用而不是各写一份
 *
 * 逐项核对过 WB 与 Trae：搜索 / 排序 / 分组 / 视图四个核心控件的**结构完全相同**，
 * 差异**全部是「选项数据 + 专属插槽」**，**没有任何行为差异**（见 `P4-TOOLBAR-ARCH §1.3`）。
 * 这与 `account-pool-card`（差异只有 tiles 数量）同类，因此共用。
 *
 * ⚠️ **反向约束**：本组件内**禁止**出现 `platform === ...` 判断 —— 平台差异一律走
 * `sortOptions` / `groupOptions`（数据）与 `children`（插槽）。若哪天这里冒出平台分支，
 * 说明合并过度，应退回「两份 + 抽共享片段」。
 *
 * ## ★ 搜索框必须**常驻挂载**（IME 约束，`§1.4`）
 *
 * 受控 `<Input>` 的 DOM 节点在 `query` 变化时**不得被 unmount**，否则中文输入法连续
 * 组词会丢字（原型里有一条可证伪断言，`verify-demo.cjs:138-141`）。因此：
 * - 搜索框**永远渲染**（即使结果为空），**不得**放进「结果为空就换掉整块」的条件分支；
 * - 本组件的 JSX 里，`<Input>` 是直接子节点，**不**处于任何条件表达式中。
 *
 * 工具栏与列表是两个独立子树：`query` 只驱动列表子树重渲染，工具栏结构不变。
 *
 * ## 视图切换 vs 卡片密度（`compact`）
 *
 * 视图（卡片 / 表格）是**形态**，`compact` 是**密度**，两者正交。`compact` 由调用方经
 * `children` 传入，且**仅在卡片视图下渲染**（`§1.7` / 决策 #5）—— 这样任一时刻页面上
 * 只有一个「密度图标开关」，不会与视图控件构成「两个看起来能切的控件」。
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

  /** 平台专属控件插槽（WB：自动签到 / 旅行开关 + 卡片密度 + 刷新；Trae：跳过已签 + 刷新）。 */
  children?: ReactNode;

  /**
   * 工具栏右侧的**主操作**（如「+ 添加账号」，原型 `viewAccounts` :2428）。
   *
   * 由页面注入**同一个**既有入口（WB 页接 OAuth 扫码添加），壳内不含任何平台 / 业务逻辑。
   * 不传 = 不渲染该按钮（Trae 页）。
   */
  addAction?: { label: string; onClick: () => void } | null;
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
  children,
  addAction,
}: AccountToolbarProps) {
  const t = useT();

  return (
    <TooltipProvider delayDuration={400}>
      <div className="mb-3.5 flex flex-wrap items-center gap-2 rounded-[12px] border border-border bg-card px-3 py-2.5">
        {/* ★ 搜索框常驻：**不**放进任何条件分支（IME 约束，见文件头）。 */}
        <div className="relative min-w-[200px] flex-1">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            ref={searchInputRef}
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder={t("shared.toolbar.searchPlaceholder")}
            aria-label={t("shared.toolbar.searchAria")}
            autoComplete="off"
            spellCheck={false}
            className="h-8 rounded-[9px] bg-card-2 pl-8"
          />
        </div>

        {/* 排序（分桶的前置：先排后分，桶内顺序才稳定）。 */}
        <Select value={sort} onValueChange={onSortChange}>
          <SelectTrigger size="sm" className="w-[150px]" aria-label={t("shared.toolbar.sortAria")}>
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

        {/* 分组（**分桶**：none / program / status / group）。 */}
        <Select value={group} onValueChange={onGroupChange}>
          <SelectTrigger size="sm" className="w-[150px]" aria-label={t("shared.toolbar.groupAria")}>
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

        {/* 视图切换：卡片 / 表格。窄窗下表格按钮**禁用并说明原因**（不是隐藏）。 */}
        <div
          role="group"
          aria-label={t("shared.toolbar.viewAria")}
          className="inline-flex items-center gap-0.5 rounded-[10px] bg-muted p-0.5"
        >
          <Button
            type="button"
            variant={view === "card" ? "secondary" : "ghost"}
            size="sm"
            className="h-[26px] rounded-md px-2.5 text-xs"
            aria-pressed={view === "card"}
            onClick={() => onViewChange("card")}
          >
            <LayoutGrid className="size-4" />
            {t("shared.toolbar.viewCard")}
          </Button>
          <Tooltip>
            <TooltipTrigger asChild>
              <span>
                <Button
                  type="button"
                  variant={view === "table" ? "secondary" : "ghost"}
                  size="sm"
                  className="h-[26px] rounded-md px-2.5 text-xs"
                  aria-pressed={view === "table"}
                  disabled={!tableViewEnabled}
                  onClick={() => onViewChange("table")}
                >
                  <Table2 className="size-4" />
                  {t("shared.toolbar.viewTable")}
                </Button>
              </span>
            </TooltipTrigger>
            {!tableViewEnabled && (
              <TooltipContent side="top">{t("shared.toolbar.viewTableDisabled")}</TooltipContent>
            )}
          </Tooltip>
        </div>

        {/* 全选（可选）：与表头全选读写**同一份** selectedIds（键控）。 */}
        {selection ? (
          <label className="flex cursor-pointer items-center gap-2 rounded-lg px-1.5 text-xs font-medium text-muted-foreground">
            <Checkbox
              checked={selection.allSelected ? true : selection.someSelected ? "indeterminate" : false}
              onCheckedChange={() => selection.onToggleAll()}
              aria-label={selection.label}
            />
            <span className={cn(selection.someSelected && !selection.allSelected && "text-foreground")}>
              {selection.label}
            </span>
          </label>
        ) : null}

        {/* 平台专属控件插槽（含卡片密度开关与刷新按钮）。 */}
        {children}

        {/* ★ 主操作（原型 `viewAccounts` :2428：工具栏行最右侧的 `.btn.primary`）。
            与「添加与迁移账号」横幅的入口**同一动作**，不重复一套流程。
            ⚠️ `ml-auto` 必须**同时**给 `DemoAction` 与按钮本身：
            `DemoAction` 在 demo 模式下会把子元素包进 `<span className="inline-flex">`
            （见 `demo-action.tsx:44`）——那一层才是 flex 子项，只给按钮加 `ml-auto` 会失效
            （实测按钮掉到第二行**最左侧**，看起来像布局坏了）；非 demo 下 `DemoAction`
            直接返回子元素，此时生效的是按钮自己的 `ml-auto`。
            尺寸按原型 `.btn`（`:208-215`）：高 34、内边距 0 13、圆角 10、字号 12.5。 */}
        {addAction ? (
          <DemoAction className="ml-auto">
            <Button
              type="button"
              size="sm"
              className="ml-auto h-[34px] rounded-[10px] bg-primary px-[13px] text-[12.5px] text-primary-foreground hover:bg-primary/90"
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
