import type { ReactNode } from "react";

import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";

/**
 * 账号列表表格（**两平台共用壳**，列定义驱动）。
 *
 * ## 为什么共用
 *
 * 与 `AccountToolbar` 同一判断（`P4-TOOLBAR-ARCH §1.3`）：表格**结构相同**，
 * 差异只是**列定义**（WB 与 Trae 列不同、单元格内容不同）。因此壳只管
 * 「表头 + 行 + 选中 + 打开详情」，列由各页以 `columns` 注入。
 *
 * ⚠️ **禁止**在本组件内写平台判断；出现 `platform === ...` 即说明合并过度。
 *
 * ## ★ 可点区 ≤ 2（`§3.3`）
 *
 * 每行只有**两个**可点区：勾选框（1）+ 行本身（点行打开详情，1）。
 * 程序位列由列定义渲染成**只读徽标**，**不是**切换控件（与抽屉同纪律）。
 *
 * ## ★ 白屏防线（`issue #2`）
 *
 * 单元格内容由 `column.render(row)` 提供，各列**必须**返回**已归一**的节点
 * （字符串 / 元素）—— 后端把展示字段吐成对象时，对象当 React 子节点会
 * 卸载整棵树 ⇒ 白屏。渲染前一律过 `displayText`（`@/lib/display-text`）。
 *
 * ## 表头全选
 *
 * 表头第一列是「全选」勾选框，支持 `aria-checked="mixed"` 半选语义（Radix `Checkbox`
 * 的 `checked="indeterminate"` 即产出该属性）。它与工具栏全选**读写同一份** `selectedIds`。
 */
export interface AccountColumn<T> {
  key: string;
  /** 已 i18n。 */
  header: string;
  align?: "left" | "right";
  /** 列宽（CSS 值，可选）。 */
  width?: string;
  /** ★ 必须返回**已归一**的节点（字符串 / 元素）；**禁止返回对象**（白屏防线）。 */
  render: (row: T) => ReactNode;
}

export interface AccountTableProps<T> {
  columns: AccountColumn<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  /** 勾选态（键控，与批量条 / 工具栏全选共用）。 */
  selectedIds: ReadonlySet<string>;
  onToggleSelect: (row: T) => void;
  /** 表头全选。 */
  onToggleAll: (checked: boolean) => void;
  allSelected: boolean;
  someSelected: boolean;
  /** 点行打开详情。 */
  onOpen: (row: T) => void;
  /** 行是否「当前登录」（高亮）。 */
  isCurrent?: (row: T) => boolean;
  /** 无障碍标签（全选 / 选择某行 / 打开某行详情）。 */
  selectAllLabel: string;
  selectRowLabel: (row: T) => string;
  openRowLabel: (row: T) => string;
}

export function AccountTable<T>({
  columns,
  rows,
  rowKey,
  selectedIds,
  onToggleSelect,
  onToggleAll,
  allSelected,
  someSelected,
  onOpen,
  isCurrent,
  selectAllLabel,
  selectRowLabel,
  openRowLabel,
}: AccountTableProps<T>) {
  return (
    <div className="overflow-x-auto rounded-[12px] border border-border">
      <table className="w-full border-collapse text-[12.5px]">
        <thead>
          <tr className="border-b border-border text-muted-foreground">
            <th scope="col" className="sticky top-0 z-[1] w-10 bg-card-2 px-3 py-2.5 text-left text-[11.5px] font-medium">
              <Checkbox
                checked={allSelected ? true : someSelected ? "indeterminate" : false}
                onCheckedChange={(checked) => onToggleAll(checked === true)}
                aria-label={selectAllLabel}
              />
            </th>
            {columns.map((column) => (
              <th
                key={column.key}
                scope="col"
                style={column.width ? { width: column.width } : undefined}
                className={cn("sticky top-0 z-[1] bg-card-2 px-3 py-2.5 text-[11.5px] font-medium", column.align === "right" ? "text-right" : "text-left")}
              >
                {column.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const key = rowKey(row);
            const selected = selectedIds.has(key);
            return (
              <tr
                key={key}
                tabIndex={0}
                aria-label={openRowLabel(row)}
                onClick={() => onOpen(row)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") onOpen(row);
                }}
                className={cn(
                  "cursor-pointer border-b border-border/60 last:border-0 hover:bg-muted/40 focus-visible:bg-muted/40 focus-visible:outline-none",
                  isCurrent?.(row) && "bg-primary/[0.07]",
                )}
              >
                {/* 可点区 1：勾选框（阻止冒泡，避免顺带打开详情）。 */}
                <td className="px-3 py-2.5 align-middle text-[11px]" onClick={(event) => event.stopPropagation()}>
                  <Checkbox
                    checked={selected}
                    onCheckedChange={() => onToggleSelect(row)}
                    aria-label={selectRowLabel(row)}
                  />
                </td>
                {/* 可点区 2：行本身（在上面的 <tr onClick>）。程序位列由列定义渲染成只读徽标。 */}
                {columns.map((column) => (
                  <td
                    key={column.key}
                    className={cn("px-3 py-2.5 align-middle text-[11px]", column.align === "right" ? "text-right" : "text-left")}
                  >
                    {column.render(row)}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
