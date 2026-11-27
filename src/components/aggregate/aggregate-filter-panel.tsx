/**
 * 聚合账号面板的**筛选浮层**（issue #50 · R4 / D3.1）。
 *
 * ## 三条硬规则（改这个文件前先读）
 *
 * ① **应用后生效，不是 live**。面板盖住了列表，live 改动用户根本看不见 ——
 *    勾一下、背后悄悄少几张卡，是「点了没反应」的反面：**有反应但看不见**。
 * ② **作用域行的「勾选区」与「展开区」是两个命中区**。点整行就切换全选，会让
 *    「想展开看看有哪些号」这个纯浏览动作顺手清空整组选择。
 * ③ **只有一种表示**：勾作用域 = 勾它名下全部账号键。不另记一个「作用域维度」，
 *    否则两种筛选叠加时就要回答「以谁为准」—— 那个问题没有正确答案。
 *
 * ## 为什么草稿在页面层（`draft` / `onDraftChange`）
 *
 * 取消要能**原样还原**。草稿若留在面板内部，`open` 一变就得重建，
 * 而重建的时机与「用户点了取消」纠缠在一起 —— 由页面持有更直白。
 */

import { useMemo } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { ScopeMark } from "@/components/scope-chip";
import { useT } from "@/lib/i18n";
import { CORE_SCOPES, scopeParts, type Scope } from "@/lib/scope";
import {
  aggregateDisplayName,
  aggregateSearchText,
  type AggregateAccount,
} from "@/lib/aggregate-accounts";
import { scopeLabel } from "@/components/scope-chip";

export interface AggregateFilterPanelProps {
  /** 全部账号（用于列出每个作用域名下的账号）。 */
  accounts: readonly AggregateAccount[];
  /** 草稿：当前勾选的全部账号键。 */
  draft: ReadonlySet<string>;
  onDraftChange: (next: Set<string>) => void;
  /** 展开中的作用域。 */
  expanded: ReadonlySet<Scope>;
  onToggleExpanded: (scope: Scope) => void;
  onApply: () => void;
  onCancel: () => void;
  /** 面板内搜索（只过滤面板里的账号行，不改可见性）。 */
  query: string;
  onQueryChange: (next: string) => void;
}

/** 一个作用域在面板里的行状态。 */
type RowState = "all" | "some" | "none";

function rowState(keys: readonly string[], draft: ReadonlySet<string>): RowState {
  if (keys.length === 0) return "none";
  const on = keys.filter((key) => draft.has(key)).length;
  if (on === 0) return "none";
  return on === keys.length ? "all" : "some";
}

export function AggregateFilterPanel({
  accounts,
  draft,
  onDraftChange,
  expanded,
  onToggleExpanded,
  onApply,
  onCancel,
  query,
  onQueryChange,
}: AggregateFilterPanelProps) {
  const t = useT();

  /** 作用域 → 它名下的账号（保持 `CORE_SCOPES` 顺序；外部平台不参与聚合）。 */
  const byScope = useMemo(() => {
    const map = new Map<Scope, AggregateAccount[]>();
    for (const account of accounts) {
      const bucket = map.get(account.scope);
      if (bucket) bucket.push(account);
      else map.set(account.scope, [account]);
    }
    return map;
  }, [accounts]);

  const allKeys = useMemo(() => accounts.map((account) => account.ref), [accounts]);
  const q = query.trim().toLowerCase();
  const total = allKeys.length;
  const allOn = total > 0 && draft.size === total;

  function toggleScope(scope: Scope) {
    const keys = (byScope.get(scope) ?? []).map((account) => account.ref);
    if (keys.length === 0) return;
    const next = new Set(draft);
    const state = rowState(keys, draft);
    for (const key of keys) {
      if (state === "all") next.delete(key);
      else next.add(key);
    }
    onDraftChange(next);
  }

  function toggleAccount(ref: string) {
    const next = new Set(draft);
    if (next.has(ref)) next.delete(ref);
    else next.add(ref);
    onDraftChange(next);
  }

  function toggleAll() {
    if (allOn) onDraftChange(new Set());
    else onDraftChange(new Set(allKeys));
  }

  return (
    <div
      className="absolute left-0 top-[calc(100%+6px)] z-50 flex w-[352px] flex-col overflow-hidden rounded-xl border border-border-strong bg-popover shadow-lg"
      style={{ maxHeight: "min(66vh, 540px)" }}
      role="dialog"
      aria-label={t("aggregate.filter.title")}
    >
      <div className="border-b border-border px-3 pb-2.5 pt-3">
        <div className="text-[12.5px] font-semibold">{t("aggregate.filter.title")}</div>
        <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">{t("aggregate.filter.hint")}</p>
      </div>

      <div className="px-3 pb-1 pt-2">
        <Input
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={t("aggregate.toolbar.searchPlaceholder")}
          className="h-8 text-xs"
          aria-label={t("aggregate.toolbar.searchAria")}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-1.5 pt-1">
        {/* 「全部账号」一行：勾它 = 全选（= 等于没筛）。 */}
        <button
          type="button"
          onClick={toggleAll}
          className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-accent"
        >
          <Checkbox
            checked={allOn ? true : draft.size > 0 ? "indeterminate" : false}
            onCheckedChange={toggleAll}
            aria-label={t("aggregate.filter.all")}
            className="pointer-events-none size-[15px]"
          />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[12.5px]">{t("aggregate.filter.all")}</span>
            <span className="block text-[10.5px] text-muted-foreground">
              {t("aggregate.filter.selected", { n: draft.size, total })}
            </span>
          </span>
        </button>

        <div className="my-1 h-px bg-border" />

        {CORE_SCOPES.map((scope) => {
          const items = byScope.get(scope) ?? [];
          const keys = items.map((account) => account.ref);
          const state = rowState(keys, draft);
          const on = keys.filter((key) => draft.has(key)).length;
          const isOpen = expanded.has(scope);
          const installed = items.length > 0;
          const { product } = scopeParts(scope);

          /* 面板内搜索：命中作用域名或任一账号 ⇒ 该组保留（搜索**不改可见性**）。 */
          const scopeMatched = !q || scopeLabel(t, scope).toLowerCase().includes(q);
          const visibleItems = scopeMatched
            ? items
            : items.filter((account) => aggregateSearchText(account).includes(q));
          if (q && !scopeMatched && visibleItems.length === 0) return null;

          return (
            <div key={scope}>
              {/* ⚠️ 整行只负责「展开 / 收起」；勾选是左边的 Checkbox 自己的命中区。 */}
              <div className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 hover:bg-accent">
                <Checkbox
                  checked={state === "all" ? true : state === "some" ? "indeterminate" : false}
                  onCheckedChange={() => toggleScope(scope)}
                  disabled={keys.length === 0}
                  aria-label={t("aggregate.filter.toggleAria", { scope: scopeLabel(t, scope) })}
                  className="size-[15px]"
                />
                <button
                  type="button"
                  onClick={() => onToggleExpanded(scope)}
                  aria-expanded={isOpen}
                  aria-label={t("aggregate.filter.expandAria", { scope: scopeLabel(t, scope) })}
                  className="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                >
                  <ScopeMark product={product} size={22} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12.5px]">{scopeLabel(t, scope)}</span>
                    <span className="block text-[10.5px] text-muted-foreground">
                      {keys.length
                        ? t("aggregate.filter.selected", { n: on, total: keys.length })
                        : installed
                          ? t("aggregate.filter.noAccounts")
                          : t("aggregate.filter.noClient")}
                    </span>
                  </span>
                  {keys.length ? (
                    <span className="shrink-0 text-[10.5px] text-muted-foreground">
                      {isOpen ? t("aggregate.filter.collapse") : t("aggregate.filter.expand")}
                    </span>
                  ) : null}
                </button>
              </div>

              {isOpen && keys.length ? (
                <div className="mb-1 ml-8 flex flex-col gap-0.5">
                  {visibleItems.map((account) => (
                    <button
                      key={account.ref}
                      type="button"
                      onClick={() => toggleAccount(account.ref)}
                      className="flex w-full items-center gap-2 rounded-lg px-2 py-1 text-left hover:bg-accent"
                    >
                      <Checkbox
                        checked={draft.has(account.ref)}
                        onCheckedChange={() => toggleAccount(account.ref)}
                        aria-label={aggregateDisplayName(account)}
                        className="pointer-events-none size-[15px]"
                      />
                      <span className="min-w-0 flex-1 truncate text-[12px]">
                        {aggregateDisplayName(account)}
                      </span>
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>

      <div className="flex items-center gap-2 border-t border-border bg-card px-3 py-2.5">
        <span className="flex-1 text-[11.5px] tabular-nums text-muted-foreground">
          {allOn
            ? t("aggregate.filter.selectedAll", { n: draft.size, total })
            : t("aggregate.filter.selectedCount", { n: draft.size, total })}
        </span>
        <Button variant="ghost" size="sm" onClick={onCancel}>
          {t("aggregate.filter.cancel")}
        </Button>
        <Button size="sm" onClick={onApply}>
          {t("aggregate.filter.apply")}
        </Button>
      </div>
    </div>
  );
}
