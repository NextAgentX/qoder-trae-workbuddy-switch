/**
 * 账号工作台（**三平台共用容器**：WorkBuddy / Trae / Qoder）。
 *
 * ## 为什么要有这个组件
 *
 * 原型 `buddy-switch-prototype.html` 的账号页是**平台无关**的：只有一个 `viewAccounts()`
 * （`:2403-2444`）、一个 `accountCard()`、一个 `tableView()`、一个 `batchBarHtml()`，
 * 靠 `curPlatform().programs` 换数据 —— 也就是「**同一段代码 + 不同数据**」。
 *
 * 本仓此前是 `AccountsPage`(2700+ 行) 与 `TraeAccountsPage`(2400+ 行) 两份独立实现各自演化，
 * 结果是「同一件事两种做法」（批量条位置、空态出口、骨架形状、分组标题样式……逐项漂移）。
 * 本组件把**布局与交互**收成一份，页面退化成「取数 + 装配 props + 注入平台插槽」。
 *
 * ## 边界（改这个组件前先读）
 *
 * **属于本组件**：页头之下的整块骨架 —— 首屏骨架 / 首屏空态 / 提示条位置 /
 * 工具栏位置 / 结果条位置 / 批量条 / 两种列表空态 / 分桶循环 / 分组标题 /
 * 卡片网格与表格的切换 / 列表下方插槽。
 *
 * **不属于本组件**：页头（标题 + 作用域 chip）、取数、筛选排序分组**数据**、
 * 卡片与表格列的内容、抽屉装配、平台专属动作。这些由页面注入。
 *
 * ## ★ 禁止事项
 *
 * - 本组件内**禁止** `platform === …` / 产品名判断 —— 平台差异一律走
 *   `sortOptions` / `groupOptions` / `addAction` / `renderCard` / `footer`。
 *   与 `account-toolbar.tsx` 同一条纪律（见该文件头注释）。
 * - **禁止**在这里做 `Array.filter/sort` —— 页面传进来的 `buckets` 已是最终结果。
 *   容器一旦开始「顺便筛一下」，两个平台的筛选口径就又会分叉。
 *
 * ## 结构（与原型逐段对应）
 *
 * ```
 * 页头（页面持有）
 * ├─ notices        提示条（错误 / 平台细条 / 「当前登录但未入库」…）   ← 原型无，本仓能力
 * ├─ section
 * │  ├─ AccountToolbar            （原型 :2406-2429）
 * │  ├─ CheckinResultBar          （原型 `#batch-slot` :2441）
 * │  ├─ AccountBatchBar           （原型 `.batch-bar` :303-307）
 * │  └─ 列表区
 * │     ├─ 空态①「真没有」        （原型 `emptyBlock` :2462-2465）
 * │     ├─ 空态②「被筛没」        （原型 `emptyBlock` :2466-2469）
 * │     └─ 分桶 → 分组标题 + 卡片网格 / 表格（原型 :2470-2475）
 * └─ footer         列表下方插槽（WB：客户端限流台账）
 * ```
 */

import type { ReactNode, Ref } from "react";
import { SearchX, UserPlus } from "lucide-react";

import { AccountBatchBar } from "@/components/account-batch-bar";
import {
  AccountsGroupHeader,
  AccountsSkeleton,
  CheckinResultBar,
} from "@/components/account-list-parts";
import { AccountTable, type AccountColumn } from "@/components/account-table";
import { AccountToolbar, type ToolbarOption } from "@/components/account-toolbar";
import { EmptyState } from "@/components/ui/empty-state";
import type { AccountViewMode } from "@/lib/account-list-view";
import { useT } from "@/lib/i18n";

/** 一个分桶（`key === null` = 不分组）。 */
export interface AccountBucket<T> {
  key: string | null;
  items: T[];
}

/** 空态四要素（与 `EmptyState` 一致）。 */
export interface AccountEmptyCopy {
  title: string;
  description: string;
  action?: ReactNode;
}

export interface AccountWorkbenchProps<T> {
  /* ────────── 工具栏（受控；筛选/排序/分组的**数据**由页面持有） ────────── */
  query: string;
  onQueryChange: (next: string) => void;
  searchInputRef?: Ref<HTMLInputElement>;
  sort: string;
  sortOptions: ToolbarOption[];
  onSortChange: (next: string) => void;
  group: string;
  groupOptions: ToolbarOption[];
  onGroupChange: (next: string) => void;
  view: AccountViewMode;
  onViewChange: (next: AccountViewMode) => void;
  tableViewEnabled: boolean;
  selection: {
    allSelected: boolean;
    someSelected: boolean;
    onToggleAll: () => void;
    /** 已 i18n：「全选」/「取消全选」。 */
    label: string;
  };
  /** 工具栏右侧的**主操作**（两平台都是「+ 添加账号」→ 开添加抽屉）。 */
  addAction?: { label: string; onClick: () => void } | null;
  /** 回收站入口（常驻工具栏）。 */
  onOpenRecycleBin?: () => void;

  /* ────────── 首屏 ────────── */
  /** 首次加载中（且一个账号都没有）⇒ 骨架。 */
  loading: boolean;
  /**
   * 首屏空态（「未安装 / 未登录」那一屏）。**非 null 且 `totalCount === 0` 时，
   * 整块替换**工具栏 + 列表（照原型的 `emptyBlock`：空态不带工具栏）。
   * 传 `null` 表示「首屏空态由页面在更外层处理」。
   */
  emptyState: ReactNode | null;
  /** 提示条（错误 / 平台细条 / 未入库提示…）：排在工具栏**之前**。 */
  notices?: ReactNode;

  /* ────────── 结果条与批量条 ────────── */
  /** 批量签到结果（`failedNames` 由页面按各自 id 字段装配好）。
   *  ⚠️ `skippedNames` = 「通道未开放」第三档（Qoder 专用，**可选**：不传时
   *    `CheckinResultBar` 的渲染与加这个 prop 之前逐字一致 ⇒ WB / Trae 不受影响）。 */
  checkinResult?: {
    okCount: number;
    failedNames: string[];
    skippedNames?: string[];
    /** 本次真领到的额度合计（活动通道专用；不传 = 不渲染那枚徽标）。 */
    earnedCredits?: number;
    onRetry?: () => void;
  } | null;
  /** 结果条「知道了」的回执（只在有结果条时用得到；不传 = 不渲染结果条也就无所谓）。 */
  onDismissCheckinResult?: () => void;
  /**
   * 批量动作 —— **与 `AccountBatchBar` 同为可选**（缺哪个就不渲染哪个）。
   *
   * ⚠️ 2026-10-15：此前这里把 `onCheckin` / `onRefresh` 写成必填，与批量条自己的
   * 契约（「平台能力不同，缺哪个就不渲染哪个」）相矛盾 —— 逼第三个平台
   * （Qoder 只有删除）为不存在的动作造一个空函数，而空函数**点下去什么都不会发生**，
   * 正是「假控件」。改为可选后，Qoder 只传 `onDelete`，其余按钮自动消失。
   */
  batch: {
    selectedCount: number;
    busy: boolean;
    onCheckin?: () => void;
    onRefresh?: () => void;
    onExport?: () => void;
    onDelete: () => void;
    onClear: () => void;
  };

  /* ────────── 列表 ────────── */
  listAriaLabel: string;
  /** 账号**总数**（未筛选）—— 决定「真没有」还是「被筛没」。 */
  totalCount: number;
  /** 筛选 + 排序后的行数。 */
  visibleCount: number;
  /**
   * 「真没有账号」空态（原型 `:2462-2465`，出口通常是「添加账号」）。
   *
   * ⚠️ **必填**：出口按钮的文案与动作两个平台不同（WB「扫码添加」/ Trae 走首屏空态），
   * 给一个默认值就会在某一侧悄悄出现**错误的**入口。
   */
  emptyNone: AccountEmptyCopy;
  /** 「没有匹配的账号」空态（原型 `:2466-2469`，出口是「清除筛选」）。 */
  emptyFiltered: AccountEmptyCopy;
  buckets: AccountBucket<T>[];

  /* ────────── 表格视图 ────────── */
  columns: AccountColumn<T>[];
  rowKey: (item: T) => string;
  selectedIds: Set<string>;
  onToggleSelect: (item: T) => void;
  onToggleBucketAll: (items: T[], checked: boolean) => void;
  bucketAllSelected: (items: T[]) => boolean;
  bucketSomeSelected: (items: T[]) => boolean;
  onOpenRow: (item: T) => void;
  isRowCurrent: (item: T) => boolean;
  /**
   * 行的无障碍名（已 i18n），**两个**：勾选框与「打开详情」是两件事，
   * 用同一个名字会让读屏用户分不清「按下去是选中还是打开」。
   */
  rowSelectLabel: (item: T) => string;
  rowOpenLabel: (item: T) => string;
  /** 行尾「更多」按钮（透传给 `AccountTable`；见那里的说明）。 */
  renderActions?: (item: T) => ReactNode;

  /* ────────── 卡片视图 ────────── */
  /** 渲染一张卡（页面注入 `AccountCard` 或 `TraeAccountCard`）。 */
  renderCard: (item: T) => ReactNode;

  /** 列表下方的插槽（WB：客户端限流台账）。 */
  footer?: ReactNode;
}

export function AccountWorkbench<T>({
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
  tableViewEnabled,
  selection,
  addAction,
  onOpenRecycleBin,
  loading,
  emptyState,
  notices,
  checkinResult,
  onDismissCheckinResult,
  batch,
  listAriaLabel,
  totalCount,
  visibleCount,
  emptyNone,
  emptyFiltered,
  buckets,
  columns,
  rowKey,
  selectedIds,
  onToggleSelect,
  onToggleBucketAll,
  bucketAllSelected,
  bucketSomeSelected,
  onOpenRow,
  isRowCurrent,
  rowSelectLabel,
  rowOpenLabel,
  renderActions,
  renderCard,
  footer,
}: AccountWorkbenchProps<T>) {
  const t = useT();

  /* 首屏：加载中 → 骨架（列数 / 圆角 / 间距与真实栅格逐值一致，见 `AccountsSkeleton`）。 */
  if (loading && totalCount === 0) return <AccountsSkeleton />;
  /* 首屏空态（未安装 / 未登录）：整块替换，**不带**工具栏（原型 `emptyBlock` 也不带）。 */
  if (totalCount === 0 && emptyState) return <>{emptyState}</>;

  return (
    <>
      {notices}

      <section className="mt-5 min-w-0" aria-label={listAriaLabel}>
        {/* ★ 工具栏**常驻**（不随结果为空而消失）：搜索框的 DOM 因此不会被 unmount，
            中文输入法连续组词不丢字（IME 约束，§1.4）。工具栏与列表是两个独立子树。 */}
        <AccountToolbar
          query={query}
          onQueryChange={onQueryChange}
          searchInputRef={searchInputRef}
          sort={sort}
          sortOptions={sortOptions}
          onSortChange={onSortChange}
          group={group}
          groupOptions={groupOptions}
          onGroupChange={onGroupChange}
          view={view}
          onViewChange={onViewChange}
          tableViewEnabled={tableViewEnabled}
          selection={selection}
          addAction={addAction}
          onOpenRecycleBin={onOpenRecycleBin}
        />

        {/* ★ 批量签到结果条（原型 `#batch-slot`，:2441）：在批量条**之前**、
            工具栏与列表**之间** —— 结果与操作同处一屏，用户不必回滚页面。 */}
        {checkinResult ? (
          <CheckinResultBar
            okCount={checkinResult.okCount}
            failedNames={checkinResult.failedNames}
            skippedNames={checkinResult.skippedNames}
            onRetry={checkinResult.onRetry}
            onDismiss={onDismissCheckinResult ?? (() => undefined)}
          />
        ) : null}

        {/* ★ 批量操作条（原型 `.batch-bar`，:303-307）：**文档流内**横条，不是底部浮条。 */}
        <AccountBatchBar
          selectedCount={batch.selectedCount}
          busy={batch.busy}
          onCheckin={batch.onCheckin}
          onRefresh={batch.onRefresh}
          onExport={batch.onExport}
          onDelete={batch.onDelete}
          onClear={batch.onClear}
        />

        <div className="mt-4">
          {/* 两种空态**绝不混用**（§1.9）：真没有账号 → 添加出口；有账号但被筛没 → 清除筛选出口。 */}
          {totalCount === 0 ? (
            <EmptyState
              icon={UserPlus}
              title={emptyNone.title}
              description={emptyNone.description}
              action={emptyNone.action}
            />
          ) : visibleCount === 0 ? (
            <EmptyState
              icon={SearchX}
              title={emptyFiltered.title}
              description={emptyFiltered.description}
              action={emptyFiltered.action}
            />
          ) : (
            <div className="space-y-6">
              {buckets.map((bucket) => (
                <div key={bucket.key ?? "__all__"} className="min-w-0">
                  {/* 分组标题（原型 :2471：组名进 `badge`、数量是右侧小字）。 */}
                  {bucket.key !== null && (
                    <AccountsGroupHeader label={bucket.key} count={bucket.items.length} />
                  )}
                  {view === "table" ? (
                    <AccountTable
                      columns={columns}
                      rows={bucket.items}
                      rowKey={rowKey}
                      selectedIds={selectedIds}
                      onToggleSelect={onToggleSelect}
                      onToggleAll={(checked) => onToggleBucketAll(bucket.items, checked)}
                      allSelected={bucketAllSelected(bucket.items)}
                      someSelected={bucketSomeSelected(bucket.items)}
                      onOpen={onOpenRow}
                      isCurrent={isRowCurrent}
                      selectAllLabel={t("shared.table.selectAllAria")}
                      selectRowLabel={rowSelectLabel}
                      openRowLabel={rowOpenLabel}
                      renderActions={renderActions}
                    />
                  ) : (
                    /* 原型 `.grid.g3`（`:242-247`）：宽屏**恰好 3 列**、间距 12px，逐级降 2 / 1 列。
                       ⚠️ 不能用 `auto-fit + minmax`：列数会随容器宽度连续变化，与原型节奏不符。
                       ⚠️★ **不要加 `items-start`**：原型 `.grid` 没有 `align-items` ⇒ 默认 `stretch`
                       ⇒ **同一行的卡片等高**（卡片是 `flex-col` + 正文 `flex-1`，页脚自然落在底部对齐）。
                       实测加 `items-start` 时同行卡片高度 406/381/361（差 45px），用户直接看出
                       「卡片有些大有些小」；去掉后同行等高。 */
                    <div className="grid min-w-0 grid-cols-1 gap-3 min-[761px]:grid-cols-2 min-[1101px]:grid-cols-3">
                      {bucket.items.map((item) => renderCard(item))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </section>

      {footer}
    </>
  );
}
