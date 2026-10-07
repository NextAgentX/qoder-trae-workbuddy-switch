/**
 * 第 2 步：**三层会话树 + 「已选 N / M」计数**（P4 T03）。
 *
 * ## 三层（原型契约 B1 断言 3）
 *
 * 类型（对话 / 工作区）→ 目录（`cwd` 末段）→ 会话（`title`）。树结构与计数口径
 * **全部来自 `@/lib/session-tree`**（纯函数）—— 本组件只负责渲染与派发交互，
 * 不自己算数，因此树上的「已选 N / M」与确认页的「已选 N 个会话 / M 个目录」
 * **不可能漂移**（§1.5）。
 *
 * ## 全选 / 全不选 → 归零 / 恢复（断言 4）
 *
 * 三个层级（类型 / 目录 / 会话）都是**三态**勾选：全选 ⇒ `aria-checked=true`；
 * 半选 ⇒ `aria-checked="mixed"`；全不选 ⇒ `false`（§3.11）。翻转逻辑复用
 * `selectionState` —— 「全开则全关，否则全开」是**唯一**实现。
 */

import { ChevronDown, ChevronRight, Folder } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { useT } from "@/lib/i18n";
import {
  selectionState,
  type SessionDirNode,
  type SessionLeaf,
  type SessionTypeNode,
  type TreeSelectionStats,
} from "@/lib/session-tree";

export interface SessionTreeProps {
  tree: SessionTypeNode[];
  selected: Set<string>;
  expanded: Set<string>;
  stats: TreeSelectionStats;
  onToggleSession: (id: string) => void;
  onToggleType: (node: SessionTypeNode) => void;
  onToggleDir: (node: SessionDirNode) => void;
  onToggleExpanded: (key: string) => void;
  onSelectAll: (on: boolean) => void;
}

/** 三态勾选：半选走 `aria-checked="mixed"`（Radix 的 `indeterminate`）。 */
function TreeCheckbox({
  allOn,
  someOn,
  onChange,
  ariaLabel,
  dataAct,
}: {
  allOn: boolean;
  someOn: boolean;
  onChange: () => void;
  ariaLabel: string;
  dataAct: string;
}) {
  return (
    <Checkbox
      checked={someOn ? "indeterminate" : allOn}
      onCheckedChange={() => onChange()}
      aria-label={ariaLabel}
      data-act={dataAct}
      className="size-3.5"
    />
  );
}

/** 会话行：勾选框 + 标题（+「有正文」徽标）。 */
function SessionRow({
  leaf,
  checked,
  indentClass,
  dataAct,
  onToggle,
}: {
  leaf: SessionLeaf;
  checked: boolean;
  indentClass: string;
  dataAct: string;
  onToggle: () => void;
}) {
  const t = useT();
  return (
    <label
      className={`flex cursor-pointer items-center gap-2.5 rounded-md py-1.5 pr-2 hover:bg-accent/50 ${indentClass}`}
    >
      <Checkbox
        checked={checked}
        onCheckedChange={() => onToggle()}
        data-act={dataAct}
        className="size-3.5"
      />
      <span className="min-w-0 flex-1 truncate text-sm" title={leaf.title}>
        {leaf.title}
      </span>
      {leaf.hasHistory ? (
        <Badge variant="outline" className="shrink-0 text-[10px]">
          {t("wbAccounts.dialog.hasBody")}
        </Badge>
      ) : null}
    </label>
  );
}

export function SessionTree({
  tree,
  selected,
  expanded,
  stats,
  onToggleSession,
  onToggleType,
  onToggleDir,
  onToggleExpanded,
  onSelectAll,
}: SessionTreeProps) {
  const t = useT();

  return (
    <section className="space-y-2">
      <div className="flex items-center gap-2">
        <h4 className="text-sm font-medium">{t("wbAccounts.wizard.treeTitle")}</h4>
        <div className="flex-1" />
        {/* 「已选 N / M」—— 计数口径来自 `treeSelectionStats`（唯一实现）。 */}
        <span className="text-xs text-muted-foreground" data-testid="wizard-tree-count">
          {t("wbAccounts.wizard.treeCount", { n: stats.selected, m: stats.total })}
        </span>
      </div>

      <div className="flex items-center gap-1.5">
        <Button
          type="button"
          variant="outline"
          size="sm"
          data-act="wiz-tree-all|1"
          onClick={() => onSelectAll(true)}
        >
          {t("wbAccounts.wizard.treeSelectAll")}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          data-act="wiz-tree-all|0"
          onClick={() => onSelectAll(false)}
        >
          {t("wbAccounts.wizard.treeSelectNone")}
        </Button>
      </div>

      <div className="max-h-[min(22rem,42vh)] overflow-y-auto pr-1">
        {tree.map((type, typeIndex) => {
          const typeOpen = expanded.has(type.key);
          const typeIds = type.dirs.flatMap((dir) => dir.sessions.map((leaf) => leaf.id));
          const typeSel = selectionState(typeIds, selected);
          return (
            <div key={type.key} className="mb-0.5">
              <div className="flex items-center gap-1.5 rounded-md px-1.5 py-1">
                <TreeCheckbox
                  allOn={typeSel.allOn}
                  someOn={typeSel.someOn}
                  onChange={() => onToggleType(type)}
                  ariaLabel={t("wbAccounts.dialog.selectKind", { label: type.label })}
                  dataAct={`wiz-tree|type|${typeIndex}`}
                />
                <button
                  type="button"
                  className="flex min-w-0 flex-1 items-center gap-1 rounded px-1 py-0.5 text-left hover:bg-accent/50"
                  onClick={() => onToggleExpanded(type.key)}
                  aria-expanded={typeOpen}
                  aria-label={t(
                    typeOpen ? "wbAccounts.dialog.collapse" : "wbAccounts.dialog.expand",
                    { label: type.label },
                  )}
                >
                  <span className="min-w-0 flex-1 truncate text-sm font-medium">
                    {type.label}
                    <span className="ml-1 font-normal text-muted-foreground">({typeIds.length})</span>
                  </span>
                  {typeOpen ? (
                    <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
                  ) : (
                    <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
                  )}
                </button>
              </div>

              {typeOpen
                ? type.dirs.map((dir, dirIndex) => {
                    const dirOpen = expanded.has(dir.key);
                    const dirSel = selectionState(
                      dir.sessions.map((leaf) => leaf.id),
                      selected,
                    );
                    return (
                      <div key={dir.key}>
                        <div className="flex items-center gap-1.5 px-1.5 py-0.5 pl-7">
                          <TreeCheckbox
                            allOn={dirSel.allOn}
                            someOn={dirSel.someOn}
                            onChange={() => onToggleDir(dir)}
                            ariaLabel={t("wbAccounts.dialog.selectFolder", { label: dir.label })}
                            dataAct={`wiz-tree|dir|${typeIndex}/${dirIndex}`}
                          />
                          <button
                            type="button"
                            className="flex min-w-0 flex-1 items-center gap-1.5 rounded px-1 py-0.5 text-left hover:bg-accent/50"
                            onClick={() => onToggleExpanded(dir.key)}
                            aria-expanded={dirOpen}
                            aria-label={t(
                              dirOpen
                                ? "wbAccounts.dialog.collapseFolder"
                                : "wbAccounts.dialog.expandFolder",
                              { label: dir.label },
                            )}
                          >
                            <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                            <span className="min-w-0 flex-1 truncate text-sm">{dir.label}</span>
                            {dirOpen ? (
                              <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
                            ) : (
                              <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
                            )}
                          </button>
                        </div>

                        {dirOpen
                          ? dir.sessions.map((leaf, sessionIndex) => (
                              <SessionRow
                                key={leaf.id}
                                leaf={leaf}
                                checked={selected.has(leaf.id)}
                                indentClass="pl-12"
                                dataAct={`wiz-tree|sess|${typeIndex}/${dirIndex}/${sessionIndex}`}
                                onToggle={() => onToggleSession(leaf.id)}
                              />
                            ))
                          : null}
                      </div>
                    );
                  })
                : null}
            </div>
          );
        })}
      </div>
    </section>
  );
}
