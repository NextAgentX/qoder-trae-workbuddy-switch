/**
 * 命令面板（⌘K）—— P6 / PLAN §3.7 / §3.11 / §3.13。
 *
 * ## 为什么这样搭
 *
 * - **浮层用 Radix Dialog**：焦点陷阱、`Esc` 关闭、关闭后焦点归还触发按钮，都是 Radix 的
 *   默认能力 —— 手写这三件事极易漏（尤其焦点归还）。本组件**不手写**它们（§1.6）。
 * - **列表用 `cmdk`**：combobox 语义 + `aria-activedescendant` + 上下键漫游（§3.11）。
 *   但过滤**关掉 cmdk 自己的匹配**（`shouldFilter={false}`），改用
 *   {@link filterPaletteItems} —— 面板必须与列表搜索**共用同一索引**（§3.13），
 *   否则同一关键词两处结果不一致（静默错误）。
 * - **纯逻辑在 `lib/command-palette.ts`**：本组件只负责取数、组装 `PaletteContext`、
 *   渲染与执行。`buildPaletteItems` / `filterPaletteItems` 可被门禁直接断言。
 *
 * ## 状态语言（§3.6）
 *
 * - 首次打开需异步取账号 ⇒ 结果区用 `<Skeleton>`（**区块加载**）；
 * - 动作执行中 ⇒ 该行内 `<Loader2>`（**按钮操作中**，不是区块加载）；
 * - 无结果 ⇒ `<EmptyState>`（图标 + 标题 + 说明 + **出口**：清空搜索），不是一行虚线框。
 *
 * ## 白屏防线（issue #2）
 *
 * 账号展示字段在 {@link paletteAccountFromWorkbuddy} / {@link paletteAccountFromTrae}
 * 里已过 `displayText` —— 对象被当 React 子节点会卸载整棵树。本组件只渲染字符串。
 *
 * ## 不新增「看起来能切」的控件（§1.5 / §1.6）
 *
 * 面板是「搜索 / 跳转」，**不是作用域切换器**：作用域组只是导航入口（回车 = 跳到该作用域），
 * 不渲染任何 tab / 下拉。切作用域的控件仍归侧栏。
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Loader2, SearchX } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import * as api from "@/lib/api";
import {
  buildPaletteItems,
  filterPaletteItems,
  paletteAccountFromTrae,
  paletteAccountFromWorkbuddy,
  type PaletteAccount,
  type PaletteContext,
  type PaletteGroup,
  type PaletteItem,
  type PaletteServices,
} from "@/lib/command-palette";
import { collectPaletteActions } from "@/lib/palette-actions";
import { DESTINATIONS, platformOfPath } from "@/lib/platforms/registry";
import { SCOPES, scopeOf, scopeParts } from "@/lib/scope";
import { planScopeShortcuts } from "@/lib/scope-shortcuts";
import { useShortcutsEnabled } from "@/lib/shortcut-preference";
import { useScope } from "@/lib/use-scope";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import { useAccountsStore } from "@/stores/accounts";

/** 分组渲染顺序（与 `PaletteGroup` 声明一致）。 */
const GROUP_ORDER: readonly PaletteGroup[] = ["page", "scope", "account", "action"];

/** 分组标题文案键。 */
const GROUP_LABEL_KEY: Record<PaletteGroup, TranslationKey> = {
  page: "palette.group.page",
  scope: "palette.group.scope",
  account: "palette.group.account",
  action: "palette.group.action",
};

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * 拉取**跨作用域**全部账号并归一。一个作用域失败不影响其余（`allSettled`）——
 * 面板宁可少一组账号，也不要因为某个平台不可用而整个打不开。
 */
async function loadPaletteAccounts(): Promise<PaletteAccount[]> {
  const [wbCn, wbGlobal, traeCn, traeGlobal] = await Promise.allSettled([
    api.getAccounts("cn"),
    api.getAccounts("global"),
    api.getTraeAccounts("trae_merged"), // 国内 Trae（区域纯，任取该区域一条程序位即可；收敛后用合并版）
    api.getTraeAccounts("global"),
  ]);

  const list: PaletteAccount[] = [];
  if (wbCn.status === "fulfilled") {
    for (const account of wbCn.value.accounts) list.push(paletteAccountFromWorkbuddy(account, "wb-cn"));
  }
  if (wbGlobal.status === "fulfilled") {
    for (const account of wbGlobal.value.accounts) {
      list.push(paletteAccountFromWorkbuddy(account, "wb-global"));
    }
  }
  if (traeCn.status === "fulfilled") {
    for (const account of traeCn.value.accounts) list.push(paletteAccountFromTrae(account, "trae-cn"));
  }
  if (traeGlobal.status === "fulfilled") {
    for (const account of traeGlobal.value.accounts) {
      list.push(paletteAccountFromTrae(account, "trae-global"));
    }
  }
  return list;
}

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const t = useT();
  const navigate = useNavigate();
  const location = useLocation();
  const [shortcutsEnabled] = useShortcutsEnabled();

  // 当前作用域：平台来自路由，区域来自 `useScope`（URL 唯一真值）。
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const currentScope = scopeOf(product, region);

  const plan = useMemo(
    () => planScopeShortcuts({ scopes: SCOPES, enabled: shortcutsEnabled }),
    [shortcutsEnabled],
  );

  const [query, setQuery] = useState("");
  const [accounts, setAccounts] = useState<readonly PaletteAccount[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [runningActionId, setRunningActionId] = useState<string | null>(null);

  /** 重新拉取账号（首次打开 + 「刷新当前作用域」动作共用）。 */
  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setAccounts(await loadPaletteAccounts());
    } finally {
      setLoading(false);
    }
  }, []);

  // 首次打开时取一次账号（之后由「刷新」动作更新，避免每次开合都打后端）。
  useEffect(() => {
    if (!open || accounts !== null) return;
    let cancelled = false;
    setLoading(true);
    void loadPaletteAccounts().then((list) => {
      if (cancelled) return;
      setAccounts(list);
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [open, accounts]);

  // 关闭时清空查询（下次打开从空开始）。
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  /**
   * 动作副作用（注入 `PaletteContext.services`）。
   *
   * - `refreshScope`：刷新面板账号列表；WorkBuddy 侧顺带让账号页的 store 重读（数据同源）；
   * - `addAccount`：导航到该作用域账号页（新增入口在那里）；
   * - `openDataDir`：调既有命令打开数据目录，失败走 `toast.error`（**不静默**）。
   */
  const services = useMemo<PaletteServices>(
    () => ({
      refreshScope: (scope) => {
        const parts = scopeParts(scope);
        if (parts.product === "workbuddy") {
          void useAccountsStore
            .getState()
            .reconcileAccounts(parts.region)
            .catch(() => undefined);
        }
        void reload();
      },
      addAccount: (scope) => {
        onOpenChange(false);
        navigate(`/s/${scope}/accounts`);
      },
      openDataDir: (scope) => {
        /* ⚠️ **不按平台分支**：平台与区域都在 `scope` 里，由参数化命令 `open_data_dir`
           自己解析（`api.openDataDir`）。写 `product === "workbuddy" ? … : …` 会让
           新增平台时漏改（§1.2 收敛纪律）。 */
        const call = api.openDataDir(scope);
        void call.catch((error) => toast.error(api.asError(error)));
      },
    }),
    [navigate, onOpenChange, reload],
  );

  const context = useMemo<PaletteContext>(
    () => ({
      t,
      currentScope,
      scopes: SCOPES,
      scopeShortcutIndex: plan.bindings,
      accounts: accounts ?? [],
      destinations: DESTINATIONS,
      navigate,
      actions: collectPaletteActions(),
      services,
    }),
    [t, currentScope, plan, accounts, navigate, services],
  );

  const items = useMemo(() => buildPaletteItems(context), [context]);
  const filtered = useMemo(() => filterPaletteItems(items, query), [items, query]);

  const showLoading = loading && accounts === null;

  /** 执行一行：动作保持面板打开（让行内 `<Loader2>` 可见）；导航类则关闭面板。 */
  function runItem(item: PaletteItem) {
    if (item.group === "action") {
      const action = context.actions.find((candidate) => `action:${candidate.id}` === item.id);
      if (action) {
        setRunningActionId(action.id);
        void Promise.resolve(action.run(context)).finally(() =>
          setRunningActionId((current) => (current === action.id ? null : current)),
        );
        return;
      }
    }
    item.run();
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        /* 对齐原型 `.palette`（`buddy-switch-prototype.html:470-473`）：
           宽 `min(600px, 92vw)`、最大高 **62vh**、圆角 **16px**、阴影 `--shadow-lg`（= `shadow-overlay`）。
           原先 576 / 70vh / 12（`rounded-lg` = 主题的 `--radius`）三处都偏。 */
        className="top-[12%] max-h-[62vh] w-[min(600px,92vw)] translate-y-0 gap-0 overflow-hidden rounded-[16px] p-0 shadow-overlay sm:max-w-[600px]"
      >
        {/* Radix 要求 Title / Description 存在（无障碍）；视觉上由输入框承担，故 sr-only。 */}
        <DialogTitle className="sr-only">{t("palette.title")}</DialogTitle>
        <DialogDescription className="sr-only">{t("palette.description")}</DialogDescription>

        <Command shouldFilter={false} label={t("palette.aria")}>
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder={t("palette.placeholder")}
            aria-label={t("palette.placeholder")}
          />

          {/* 结果数播报（§3.11）：视觉隐藏，屏幕阅读器可闻。 */}
          <span className="sr-only" aria-live="polite" aria-atomic="true">
            {t("palette.results", { count: filtered.length })}
          </span>

          {showLoading ? (
            <div className="space-y-2 p-3" aria-busy="true">
              {Array.from({ length: 4 }).map((_, index) => (
                <Skeleton key={index} className="h-8 w-full" />
              ))}
            </div>
          ) : filtered.length === 0 ? (
            <EmptyState
              icon={SearchX}
              title={t("palette.empty.title")}
              description={t("palette.empty.description")}
              action={
                <Button variant="outline" size="sm" onClick={() => setQuery("")}>
                  {t("palette.empty.clear")}
                </Button>
              }
            />
          ) : (
            <CommandList>
              {GROUP_ORDER.map((group) => {
                const groupItems = filtered.filter((item) => item.group === group);
                if (groupItems.length === 0) return null; // 空组不渲染（§1.7）
                return (
                  <CommandGroup key={group} heading={t(GROUP_LABEL_KEY[group])}>
                    {groupItems.map((item) => (
                      <CommandItem key={item.id} value={item.id} onSelect={() => runItem(item)}>
                        <span className="min-w-0 flex-1 truncate">{item.title}</span>
                        {item.subtitle ? (
                          <span className="shrink-0 truncate text-xs text-muted-foreground">
                            {item.subtitle}
                          </span>
                        ) : null}
                        {item.group === "action" &&
                        runningActionId === item.id.replace(/^action:/, "") ? (
                          <Loader2 className="size-3.5 shrink-0 animate-spin" aria-hidden="true" />
                        ) : item.shortcutHint ? (
                          /* 键帽的字体/内边距/圆角/底色由 `index.css` 的全局 `kbd` 规则统一提供
                            （原型 `:71-75`），此处只保留列表布局需要的收缩与字色。 */
                          <kbd className="shrink-0 tabular-nums text-muted-foreground/70">
                            {item.shortcutHint}
                          </kbd>
                        ) : null}
                      </CommandItem>
                    ))}
                  </CommandGroup>
                );
              })}
            </CommandList>
          )}
        </Command>
      </DialogContent>
    </Dialog>
  );
}
