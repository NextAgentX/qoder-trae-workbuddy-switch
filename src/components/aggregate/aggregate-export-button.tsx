/**
 * 聚合页的**导出**（issue #50）。
 *
 * ## 为什么要有这一步
 *
 * 导出命令 `accounts_export(scope, ids)` / `trae_export_accounts` 是**单作用域**的
 * （scope 决定平台与区域，Qoder 的 `qoder-cn` 与 WB 的 `cn` 会撞车）⇒
 * 跨作用域的账号**不能一次导出**。
 *
 * ⇒ 聚合页的导出做成「**先选作用域，再走既有的弹窗**」：
 *   · 只有一个作用域有目标 ⇒ 直接开弹窗；
 *   · 多个 ⇒ 列出各作用域与账号数，让用户选一个（不静默只导第一个）。
 *
 * ## 为什么复用既有两个弹窗
 *
 * `ExportAccountsDialog`（WB / Qoder）与 `TraeExportAccountsDialog`（Trae，
 * 因为它的导出文件键名是 `UserID` 大写、命令也不同）已经处理了
 * 「勾选列表 → 保存对话框 → 资源管理器定位」三段。**重写一份必然漂**
 * （尤其桌面端不能用 `<a download>`，必须走后端写文件）。
 *
 * ⚠️ 跨作用域时**不合并成一个 JSON** —— 那需要后端新命令，且用户要的是
 * 「各作用域各一份」（凭据格式本来就不同平台不同）。
 */

import { useMemo, useState } from "react";
import { Download } from "lucide-react";

import { ExportAccountsDialog } from "@/components/export-accounts-dialog";
import { TraeExportAccountsDialog } from "@/components/trae-export-accounts-dialog";
import { ScopeMark, scopeLabel } from "@/components/scope-chip";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useT } from "@/lib/i18n";
import { SCOPES, scopeParts, type Scope } from "@/lib/scope";
import type { TraeAccount } from "@/lib/trae-types";
import { AGGREGATE_SOURCES, aggregateDisplayName, type AggregateAccount } from "@/lib/aggregate-accounts";

/** 一次导出的目标：一个作用域 + 该作用域下的账号。 */
export interface ExportTarget {
  scope: Scope;
  accounts: AggregateAccount[];
}

export function AggregateExportButton({
  targets,
  onExported,
}: {
  /** 待导出的账号（**已按作用域分组**）。 */
  targets: readonly ExportTarget[];
  onExported?: (count: number) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [current, setCurrent] = useState<ExportTarget | null>(null);

  const nonEmpty = useMemo(
    () => targets.filter((target) => target.accounts.length > 0),
    [targets],
  );

  if (nonEmpty.length === 0) {
    return (
      <Button variant="ghost" size="sm" disabled title={t("aggregate.export.none")}>
        <Download className="size-3.5" />
        {t("aggregate.export.button")}
      </Button>
    );
  }

  return (
    <>
      <DropdownMenu
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          /* 只有一个作用域 ⇒ 不必让用户多选一步，直接开弹窗。 */
          if (next && nonEmpty.length === 1) {
            setCurrent(nonEmpty[0]);
            setOpen(false);
          }
        }}
      >
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm">
            <Download className="size-3.5" />
            {t("aggregate.export.button")}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-[280px]">
          {nonEmpty.length > 1 ? (
            <div className="px-2.5 pb-1 pt-2 text-[10.5px] font-medium tracking-[0.02em] text-muted-foreground">
              {t("aggregate.export.pickScope")}
            </div>
          ) : null}
          {nonEmpty.map((target) => {
            const source = AGGREGATE_SOURCES.find((item) => item.scope === target.scope);
            const { product } = scopeParts(target.scope);
            return (
              <DropdownMenuItem
                key={target.scope}
                onSelect={() => {
                  setCurrent(target);
                  setOpen(false);
                }}
                className="gap-2.5 rounded-[10px] px-2.5 py-2"
              >
                <ScopeMark product={product} size={22} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12.5px]">{scopeLabel(t, target.scope)}</span>
                  <span className="block text-[10.5px] text-muted-foreground">
                    {t("aggregate.export.count", { n: target.accounts.length })}
                  </span>
                </span>
                {/* 平台标识（WB / Trae / Qoder 走的是不同的导出命令）。 */}
                <span className="shrink-0 text-[10.5px] text-muted-foreground">
                  {source?.platform === "trae"
                    ? t("aggregate.export.platform.trae")
                    : source?.platform === "qoder"
                      ? t("aggregate.export.platform.qoder")
                      : t("aggregate.export.platform.workbuddy")}
                </span>
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>

      <AggregateExportDialog
        target={current}
        onOpenChange={(next) => {
          if (!next) setCurrent(null);
        }}
        onExported={onExported}
      />
    </>
  );
}

/**
 * 受控的导出弹窗（**单作用域**）。
 *
 * ## 为什么要单独导出这个组件（issue #89 · B9）
 *
 * 卡片「更多 → 导出账号」只有**一个**账号，而 `AggregateExportButton` 自带下拉与
 * 「先选作用域」那一步 —— 把按钮塞进卡片菜单既不合版式，也没必要。
 * 抽出受控弹窗后：工具栏按钮走「下拉 + 本组件」，卡片菜单走「直接传一个账号」，
 * **两处复用同一套弹窗**（两个平台的导出弹窗本来就不同构，重写必然漂）。
 */
export function AggregateExportDialog({
  target,
  onOpenChange,
  onExported,
}: {
  target: ExportTarget | null;
  onOpenChange: (open: boolean) => void;
  onExported?: (count: number) => void;
}) {
  if (!target) return null;
  return target.accounts[0]?.platform === "trae" ? (
    <TraeExportAccountsDialog
      open
      onOpenChange={onOpenChange}
      variant={traeVariantOf(target.scope)}
      /* ⚠️ Trae 弹窗收的是**完整账号对象**（它自己算展示名，且导出文件键名是 `UserID`），
         与通用弹窗的 `{id,label}` 形状不同 —— 两个弹窗本来就不同构。 */
      accounts={target.accounts.map((account) => account.data as TraeAccount)}
      onExported={onExported}
    />
  ) : (
    <ExportAccountsDialog
      open
      onOpenChange={onOpenChange}
      scope={target.scope}
      accounts={target.accounts.map((account) => ({
        id:
          account.platform === "workbuddy"
            ? account.data.id
            : account.platform === "qoder"
              ? account.data.uid
              : "",
        label: aggregateDisplayName(account),
      }))}
      onExported={onExported}
    />
  );
}

/** 该作用域的 Trae 账号库变体（导出命令用）。 */
function traeVariantOf(scope: Scope) {
  const source = AGGREGATE_SOURCES.find((item) => item.scope === scope);
  return source && source.platform === "trae" ? source.variant : "trae_merged";
}

/** 按作用域分组（导出目标）。 */
export function groupExportTargets(
  accounts: readonly AggregateAccount[],
): ExportTarget[] {
  const map = new Map<Scope, AggregateAccount[]>();
  for (const account of accounts) {
    const bucket = map.get(account.scope);
    if (bucket) bucket.push(account);
    else map.set(account.scope, [account]);
  }
  /* 顺序按 `SCOPES`（与页面上的分组顺序一致）。 */
  return SCOPES.filter((scope) => map.has(scope)).map((scope) => ({
    scope,
    accounts: map.get(scope) ?? [],
  }));
}
