import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, AlertTriangle, Coins, KeyRound, Loader2, Plus, RefreshCw, Trash2, Users } from "lucide-react";
import { toast } from "sonner";

import { AddAccountDrawer } from "@/components/add-account-drawer";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { type ToolbarOption } from "@/components/account-toolbar";
import { type AccountColumn } from "@/components/account-table";
import { AccountWorkbench } from "@/components/account-workbench";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import * as api from "@/lib/api";
import { formatCredits } from "@/components/usage/format";
import { useT } from "@/lib/i18n";
import { useSlowLoading } from "@/lib/use-slow-loading";
import { getExternalPlatformSettings, saveExternalPlatformSettings } from "@/lib/external-platforms";
import { Switch } from "@/components/ui/switch";
import type { TranslationKey } from "@/locales/zh";
import type { AccountViewMode } from "@/lib/account-list-view";

const PROVIDERS: api.ExternalCreditProvider[] = ["doubao", "loomy", "raccoon"];

function providerLabel(provider: api.ExternalCreditProvider, t: (key: TranslationKey) => string) {
  return t(`wbStats.externalCredit.provider.${provider}` as TranslationKey);
}

export function ExternalCreditSources({ lockedProvider, lowBalanceThreshold = 0, onAccountsChange }: { lockedProvider?: api.ExternalCreditProvider; lowBalanceThreshold?: number; onAccountsChange?: (accounts: api.ExternalCreditAccount[]) => void } = {}) {
  const t = useT();
  const [accounts, setAccounts] = useState<api.ExternalCreditAccount[]>([]);
  const [provider, setProvider] = useState<api.ExternalCreditProvider>(lockedProvider ?? "doubao");
  const [name, setName] = useState("");
  const [credential, setCredential] = useState("");
  const [refreshCredential, setRefreshCredential] = useState("");
  const [adding, setAdding] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [loading, setLoading] = useState(true);
  /* 与 WorkBuddy / Qoder 同一条流程：工具栏「+ 添加账号」先开右侧**抽屉**选方式，
     再进入具体表单。此前这里是「直接弹居中表单」，是全应用唯一一处不一致的添加入口。 */
  const [addDrawerOpen, setAddDrawerOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<api.ExternalCreditAccount | null>(null);
  const [removeTargets, setRemoveTargets] = useState<api.ExternalCreditAccount[]>([]);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("balance");
  const [group, setGroup] = useState("none");
  const [view, setView] = useState<AccountViewMode>("card");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    if (lockedProvider) setProvider(lockedProvider);
  }, [lockedProvider]);

  /* 与三个平台的账号页同一份「慢加载」判据（issue #188）。 */
  const slowLoading = useSlowLoading(loading);

  /* 抽屉「更多操作」里的两个开关读写同一份本机设置（与设置页同源，不会两处各存一份）。 */
  const [drawerSettings, setDrawerSettings] = useState(() => getExternalPlatformSettings(lockedProvider ?? provider));
  useEffect(() => {
    setDrawerSettings(getExternalPlatformSettings(lockedProvider ?? provider));
  }, [lockedProvider, provider]);
  const updateDrawerSettings = (next: ReturnType<typeof getExternalPlatformSettings>) => {
    setDrawerSettings(next);
    saveExternalPlatformSettings(lockedProvider ?? provider, next);
  };

  const reload = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const nextAccounts = lockedProvider
        ? await api.getExternalCreditProviderAccounts(lockedProvider)
        : await api.getExternalCreditAccounts();
      setAccounts(nextAccounts);
      onAccountsChange?.(nextAccounts);
    } catch (cause) {
      const message = api.asError(cause) || t("wbStats.externalCredit.loadFailed");
      setError(message);
      toast.error(message);
    } finally {
      setLoading(false);
    }
  }, [lockedProvider, onAccountsChange, t]);

  useEffect(() => { void reload(); }, [reload]);

  const onAdd = async () => {
    if (!credential.trim() || adding) return;
    setAdding(true);
    try {
      await api.addExternalCreditAccount({
        provider,
        name,
        credential,
        ...(provider === "raccoon" && refreshCredential.trim() ? { refreshCredential } : {}),
      });
      setName("");
      setCredential("");
      setRefreshCredential("");
      setAddOpen(false);
      const refreshed = lockedProvider
        ? await api.refreshExternalCreditProvider(lockedProvider)
        : await api.refreshExternalCredits();
      const nextAccounts = refreshed.accounts ?? [];
      setAccounts(nextAccounts);
      onAccountsChange?.(nextAccounts);
      toast.success(t("wbStats.externalCredit.added"));
    } catch (cause) {
      toast.error(api.asError(cause) || t("wbStats.externalCredit.saveFailed"));
    } finally {
      setAdding(false);
    }
  };

  const onRefresh = async () => {
    setRefreshing(true);
    setError("");
    try {
      const result = lockedProvider
        ? await api.refreshExternalCreditProvider(lockedProvider)
        : await api.refreshExternalCredits();
      const nextAccounts = result.accounts ?? [];
      setAccounts(nextAccounts);
      onAccountsChange?.(nextAccounts);
      toast.success(t("wbStats.externalCredit.refreshed"));
    } catch (cause) {
      const message = api.asError(cause);
      setError(message);
      toast.error(message);
    } finally {
      setRefreshing(false);
    }
  };

  const onRemove = async () => {
    const targets = removeTargets.length ? removeTargets : removeTarget ? [removeTarget] : [];
    if (!targets.length || removing) return;
    setRemoving(true);
    try {
      for (const target of targets) {
        if (lockedProvider) await api.removeExternalCreditProviderAccount(lockedProvider, target.id);
        else await api.removeExternalCreditAccount(target.id);
      }
      const targetIds = new Set(targets.map((target) => target.id));
      const nextAccounts = accounts.filter((account) => !targetIds.has(account.id));
      setAccounts(nextAccounts);
      onAccountsChange?.(nextAccounts);
      setRemoveTarget(null);
      setRemoveTargets([]);
      setSelectedIds(new Set());
      toast.success(t("wbStats.externalCredit.removed"));
    } catch (cause) {
      await reload();
      toast.error(api.asError(cause));
    } finally {
      setRemoving(false);
    }
  };

  const canSubmit = credential.trim().length > 0;
  const sectionTitle = lockedProvider
    ? t("externalPlatforms.accounts.title", { provider: providerLabel(lockedProvider, t) })
    : t("wbStats.externalCredit.title");
  const sectionDescription = lockedProvider
    ? t("externalPlatforms.accounts.description", { provider: providerLabel(lockedProvider, t) })
    : t("wbStats.externalCredit.description");

  const filteredAccounts = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return accounts
      .filter((account) => !needle || account.name.toLocaleLowerCase().includes(needle))
      .slice()
      .sort((a, b) => sort === "name"
        ? a.name.localeCompare(b.name)
        : sort === "updated"
          ? (b.updatedAt ?? 0) - (a.updatedAt ?? 0)
          : (b.remaining ?? -1) - (a.remaining ?? -1));
  }, [accounts, query, sort]);
  const groups = useMemo(() => {
    if (group === "none") return [{ key: "all", label: "", accounts: filteredAccounts }];
    const attention = filteredAccounts.filter((account) => account.error || (lowBalanceThreshold > 0 && account.remaining != null && account.remaining < lowBalanceThreshold));
    const healthy = filteredAccounts.filter((account) => !attention.includes(account));
    return [
      { key: "attention", label: t("externalPlatforms.accounts.groupAttention"), accounts: attention },
      { key: "healthy", label: t("externalPlatforms.accounts.groupHealthy"), accounts: healthy },
    ].filter((bucket) => bucket.accounts.length > 0);
  }, [filteredAccounts, group, lowBalanceThreshold, t]);
  const displayedIds = filteredAccounts.map((account) => account.id);
  const selectedCount = displayedIds.filter((id) => selectedIds.has(id)).length;
  const allSelected = displayedIds.length > 0 && selectedCount === displayedIds.length;
  const sortOptions: ToolbarOption[] = [
    { value: "balance", label: t("externalPlatforms.accounts.sortBalance") },
    { value: "name", label: t("externalPlatforms.accounts.sortName") },
    { value: "updated", label: t("externalPlatforms.accounts.sortUpdated") },
  ];
  const groupOptions: ToolbarOption[] = [
    { value: "none", label: t("externalPlatforms.accounts.groupNone") },
    { value: "attention", label: t("externalPlatforms.accounts.groupAttention") },
  ];
  const toggleAccount = (id: string) => setSelectedIds((current) => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  });
  const toggleAll = () => setSelectedIds((current) => {
    const next = new Set(current);
    if (allSelected) displayedIds.forEach((id) => next.delete(id));
    else displayedIds.forEach((id) => next.add(id));
    return next;
  });
  const openBatchRemove = () => setRemoveTargets(filteredAccounts.filter((account) => selectedIds.has(account.id)));

  /* 表格列：与 WorkBuddy 账号表的「账号 / 状态 / 余额」同构，让共享工具栏的
     卡片↔表格开关在外部平台同样可用（不给一个禁用着的死开关）。 */
  const columns: AccountColumn<api.ExternalCreditAccount>[] = [
    {
      key: "account",
      header: t("wbAccounts.table.account"),
      render: (account) => (
        <div className="min-w-0">
          <div className="truncate font-medium" title={account.name}>{account.name || providerLabel(account.provider, t)}</div>
          <div className="truncate text-xs text-muted-foreground">{providerLabel(account.provider, t)}</div>
        </div>
      ),
    },
    {
      key: "status",
      header: t("wbAccounts.table.status"),
      render: (account) => account.error
        ? <span className="inline-flex items-center gap-1 text-xs text-destructive"><AlertCircle className="size-3" />{account.error}</span>
        : account.updatedAt
          ? <span className="text-xs text-muted-foreground">{new Date(account.updatedAt).toLocaleString()}</span>
          : <span className="text-xs text-muted-foreground">{t("wbStats.externalCredit.notLoaded")}</span>,
    },
    {
      key: "remaining",
      header: t("externalPlatforms.overview.balance"),
      align: "right",
      render: (account) => (
        <span className="tabular-nums">
          {account.remaining == null ? "—" : `${formatCredits(account.remaining)} ${account.unit ?? t("externalPlatforms.unit.credits")}`}
        </span>
      ),
    },
  ];

  const notices = error ? <div role="alert" className="mb-3 flex items-start gap-2 rounded-lg border border-destructive/25 bg-destructive/5 px-3 py-2 text-xs text-destructive"><AlertCircle className="mt-0.5 size-4 shrink-0" /><span className="min-w-0 flex-1">{error}</span><Button size="sm" variant="outline" onClick={() => void reload()} disabled={loading}>{t("wbStats.token.retry")}</Button></div> : null;

  const renderAccountCard = (account: api.ExternalCreditAccount) => (
    <div className="flex min-w-0 items-center gap-3 rounded-xl border bg-card px-3 py-3">
      <Checkbox checked={selectedIds.has(account.id)} onCheckedChange={() => toggleAccount(account.id)} aria-label={`${t("externalPlatforms.accounts.selectAccount")} ${account.name}`} />
      <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-muted"><Coins className="size-4 text-muted-foreground" /></div>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
          <span className="font-medium">{account.name || providerLabel(account.provider, t)}</span>
          <span className="rounded-md bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{providerLabel(account.provider, t)}</span>
          {lowBalanceThreshold > 0 && account.remaining != null && account.remaining < lowBalanceThreshold ? <span className="inline-flex items-center gap-1 text-xs text-warning"><AlertTriangle className="size-3" />{t("externalPlatforms.settings.lowBalance")}</span> : null}
        </div>
        {account.error ? (
          <p className="mt-1 truncate text-xs text-destructive" title={account.error}>{account.error}</p>
        ) : (
          <p className="mt-1 truncate text-[11px] text-muted-foreground">
            {account.updatedAt ? `${t("wbStats.externalCredit.updatedAt")} ${new Date(account.updatedAt).toLocaleString()}` : t("wbStats.externalCredit.notLoaded")}
            {account.dailyRemaining != null ? ` · ${t("wbStats.externalCredit.dailyRemaining")} ${formatCredits(account.dailyRemaining)}` : ""}
          </p>
        )}
      </div>
      <div className="shrink-0 text-right">
        <div className="text-sm font-semibold tabular-nums">{account.remaining == null ? "—" : formatCredits(account.remaining)}</div>
        <div className="text-[10px] text-muted-foreground">{account.unit ?? t("externalPlatforms.unit.credits")}</div>
      </div>
      <Button size="icon" variant="ghost" aria-label={`${t("wbStats.externalCredit.remove")} ${account.name}`} onClick={() => setRemoveTarget(account)}><Trash2 /></Button>
    </div>
  );

  const dialogs = (
    <>
      {/* 第一步：与 WorkBuddy / Qoder 同一个抽屉壳（`AddAccountDrawer`）。
          外部平台只有「手动填凭据」这一种真实方式 ⇒ `ways={[]}` + `extraOptions` 注入一张卡；
          不渲染 OAuth / 从本机导入 / 导入备份 —— 那三条在外部平台没有后端命令，
          摆出来就是点下去必然失败的假控件。 */}
      <AddAccountDrawer
        open={addDrawerOpen}
        onOpenChange={setAddDrawerOpen}
        scopeLabel={providerLabel(provider, t)}
        installed
        ways={[]}
        extraOptions={[{
          key: "credential",
          icon: KeyRound,
          title: t("externalPlatforms.accounts.drawer.credentialTitle", { provider: providerLabel(provider, t) }),
          desc: t("externalPlatforms.accounts.drawer.credentialDesc"),
          onClick: () => setAddOpen(true),
        }]}
        onPick={() => undefined}
        /* 第二段「更多操作」：WorkBuddy 把平台级开关放进同一抽屉（用户 2026-10-12 的明确要求），
           外部平台此前只在设置页有这两项 —— 同一种能力两个入口位置不一致。
           这里只放**真实存在**的本机设置：自动刷新（开/关）与低余额提醒（开/关）。
           具体数值（间隔分钟数 / 阈值）仍归设置页，抽屉里只做开关，不重复摆选择器。 */
        moreTools={[
          {
            key: "auto-refresh",
            icon: RefreshCw,
            label: t("externalPlatforms.accounts.tool.autoRefresh"),
            hint: t("externalPlatforms.accounts.tool.autoRefreshHint"),
            htmlFor: "external-accounts-auto-refresh",
            control: (
              <Switch
                id="external-accounts-auto-refresh"
                checked={drawerSettings.autoRefreshMinutes > 0}
                onCheckedChange={(enabled) => updateDrawerSettings({
                  ...drawerSettings,
                  autoRefreshMinutes: enabled ? (drawerSettings.autoRefreshMinutes || 15) : 0,
                })}
                aria-label={t("externalPlatforms.accounts.tool.autoRefresh")}
              />
            ),
          },
          {
            key: "low-balance",
            icon: AlertTriangle,
            label: t("externalPlatforms.accounts.tool.lowBalance"),
            hint: t("externalPlatforms.accounts.tool.lowBalanceHint"),
            htmlFor: "external-accounts-low-balance",
            control: (
              <Switch
                id="external-accounts-low-balance"
                checked={drawerSettings.lowBalanceThreshold > 0}
                onCheckedChange={(enabled) => updateDrawerSettings({
                  ...drawerSettings,
                  lowBalanceThreshold: enabled ? (drawerSettings.lowBalanceThreshold || 50) : 0,
                })}
                aria-label={t("externalPlatforms.accounts.tool.lowBalance")}
              />
            ),
          },
          {
            key: "refresh",
            icon: RefreshCw,
            label: t("wbStats.externalCredit.refresh"),
            disabled: refreshing || loading || accounts.length === 0,
            onClick: () => {
              setAddDrawerOpen(false);
              void onRefresh();
            },
          },
        ]}
        labels={{
          title: t("shared.addDrawer.title"),
          scopeHintPrefix: t("shared.addDrawer.scopePrefix"),
          scopeHintSuffix: t("shared.addDrawer.scopeSuffix"),
          oauthTitle: t("shared.addDrawer.oauthTitle"),
          oauthDesc: t("shared.addDrawer.oauthDesc"),
          localTitle: t("shared.addDrawer.localTitle"),
          localDesc: t("shared.addDrawer.localDesc"),
          backupTitle: t("shared.addDrawer.backupTitle"),
          backupDesc: t("shared.addDrawer.backupDesc"),
          notInstalled: t("shared.addDrawer.notInstalled"),
          moreTools: t("shared.addDrawer.moreTools"),
        }}
      />

      {/* 第二步：填凭据的具体表单（保持原有字段与校验）。 */}
      <Dialog open={addOpen} onOpenChange={(open) => !adding && setAddOpen(open)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("wbStats.externalCredit.add")}</DialogTitle>
            <DialogDescription>{t("wbStats.externalCredit.description")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            {lockedProvider ? null : <div className="space-y-1.5"><Label>{t("wbStats.externalCredit.provider")}</Label><Select value={provider} onValueChange={(value) => setProvider(value as api.ExternalCreditProvider)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent>{PROVIDERS.map((item) => <SelectItem key={item} value={item}>{providerLabel(item, t)}</SelectItem>)}</SelectContent></Select></div>}
            <div className="space-y-1.5"><Label htmlFor="external-credit-name">{t("wbStats.externalCredit.name")}</Label><Input id="external-credit-name" value={name} onChange={(event) => setName(event.target.value)} placeholder={t("wbStats.externalCredit.namePlaceholder")} /></div>
            <div className="space-y-1.5"><Label htmlFor="external-credit-credential">{t("wbStats.externalCredit.credential")}</Label><Input id="external-credit-credential" type="password" autoComplete="off" value={credential} onChange={(event) => setCredential(event.target.value)} placeholder={t(("wbStats.externalCredit.credentialPlaceholder." + provider) as TranslationKey)} /></div>
            {provider === "raccoon" ? <div className="space-y-1.5"><Label htmlFor="external-credit-refresh">{t("wbStats.externalCredit.refreshCredential")}</Label><Input id="external-credit-refresh" type="password" autoComplete="off" value={refreshCredential} onChange={(event) => setRefreshCredential(event.target.value)} placeholder={t("wbStats.externalCredit.refreshCredentialPlaceholder")} /></div> : null}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddOpen(false)} disabled={adding}>{t("wbStats.externalCredit.cancel")}</Button>
            <Button onClick={() => void onAdd()} disabled={adding || !canSubmit}>{adding ? <Loader2 className="animate-spin" /> : <Plus />}{t("wbStats.externalCredit.addSubmit")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={removeTarget !== null || removeTargets.length > 0} onOpenChange={(open) => !open && !removing && (setRemoveTarget(null), setRemoveTargets([]))}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{removeTargets.length > 1 ? t("externalPlatforms.accounts.removeManyTitle", { count: removeTargets.length }) : t("wbStats.externalCredit.removeTitle")}</DialogTitle>
            <DialogDescription>{removeTargets.length > 1 ? t("externalPlatforms.accounts.removeManyDescription", { count: removeTargets.length }) : t("wbStats.externalCredit.removeDescription", { name: removeTarget?.name ?? removeTargets[0]?.name ?? "" })}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => { setRemoveTarget(null); setRemoveTargets([]); }} disabled={removing}>{t("wbStats.externalCredit.cancel")}</Button>
            <Button variant="destructive" onClick={() => void onRemove()} disabled={removing}>{removing ? <Loader2 className="animate-spin" /> : <Trash2 />}{t("wbStats.externalCredit.remove")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );

  /* 锁定平台（三个独立平台的账号页）：走 WorkBuddy / Trae / Qoder 共用的 `AccountWorkbench`，
     骨架、两种空态、分组标题、批量条位置全部同源 —— 不再自己拼一份。 */
  if (lockedProvider) {
    return (
      <>
        {notices ? <div className="mb-3">{notices}</div> : null}
        <AccountWorkbench<api.ExternalCreditAccount>
          query={query}
          onQueryChange={setQuery}
          sort={sort}
          sortOptions={sortOptions}
          onSortChange={setSort}
          group={group}
          groupOptions={groupOptions}
          onGroupChange={setGroup}
          view={view}
          onViewChange={setView}
          tableViewEnabled
          selection={{ allSelected, someSelected: selectedCount > 0 && !allSelected, onToggleAll: toggleAll, label: allSelected ? t("shared.toolbar.deselectAll") : t("shared.toolbar.selectAll") }}
          addAction={{ label: t("wbStats.externalCredit.add"), onClick: () => setAddDrawerOpen(true) }}
          loading={loading}
          slow={slowLoading}
          onReload={() => void reload()}
          emptyState={null}
          batch={{ selectedCount, busy: refreshing || removing, onRefresh: () => void onRefresh(), onDelete: openBatchRemove, onClear: () => setSelectedIds(new Set()) }}
          listAriaLabel={sectionTitle}
          totalCount={accounts.length}
          visibleCount={filteredAccounts.length}
          emptyNone={{ title: t("wbStats.externalCredit.empty"), description: t("wbStats.externalCredit.description"), action: <Button size="sm" onClick={() => setAddDrawerOpen(true)}><Plus />{t("wbStats.externalCredit.add")}</Button> }}
          emptyFiltered={{ title: t("externalPlatforms.accounts.noSearchResults"), description: t("externalPlatforms.accounts.searchHint"), action: <Button size="sm" variant="outline" onClick={() => { setQuery(""); setGroup("none"); }}>{t("shared.list.empty.clearFilters")}</Button> }}
          buckets={groups.map((bucket) => ({ key: bucket.key === "all" ? null : bucket.key, items: bucket.accounts }))}
          columns={columns}
          rowKey={(account) => account.id}
          selectedIds={selectedIds}
          onToggleSelect={(account) => toggleAccount(account.id)}
          onToggleBucketAll={(items, checked) => setSelectedIds((current) => { const next = new Set(current); items.forEach((item) => checked ? next.add(item.id) : next.delete(item.id)); return next; })}
          bucketAllSelected={(items) => items.length > 0 && items.every((item) => selectedIds.has(item.id))}
          bucketSomeSelected={(items) => items.some((item) => selectedIds.has(item.id))}
          onOpenRow={() => undefined}
          isRowCurrent={() => false}
          rowSelectLabel={(account) => `${t("externalPlatforms.accounts.selectAccount")} ${account.name}`}
          rowOpenLabel={(account) => account.name}
          renderCard={renderAccountCard}
          footer={<p className="text-[11px] text-muted-foreground">{accounts.length} {t("wbStats.externalCredit.title")}</p>}
        />
        {dialogs}
      </>
    );
  }

  return (
    <>
      <Card className="gap-0 overflow-hidden p-0">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">{sectionTitle}</h2>
            <p className="mt-0.5 text-xs text-muted-foreground">{sectionDescription}</p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button size="sm" variant="outline" onClick={() => void onRefresh()} disabled={refreshing || loading}>
              {refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("wbStats.externalCredit.refresh")}
            </Button>
            <Button size="sm" onClick={() => setAddDrawerOpen(true)}><Plus />{t("wbStats.externalCredit.add")}</Button>
          </div>
        </div>

        <div className="px-5 py-4">
          {notices}
          {loading ? <div className="space-y-2">{[0, 1, 2].map((key) => <Skeleton key={key} className="h-[66px] w-full rounded-lg" />)}</div> : accounts.length ? (
            filteredAccounts.length ? groups.map((bucket) => <section key={bucket.key} className="mb-3 last:mb-0" aria-label={bucket.label || undefined}>
              {bucket.label ? <h3 className="mb-2 text-[11.5px] font-[550] text-muted-foreground">{bucket.label} · {bucket.accounts.length}</h3> : null}
              <div className="divide-y rounded-lg border">
                {bucket.accounts.map((account) => <div key={account.id} className="min-w-0">{renderAccountCard(account)}</div>)}
              </div>
            </section>) : <EmptyState icon={Users} title={t("externalPlatforms.accounts.noSearchResults")} description={t("externalPlatforms.accounts.searchHint")} />
          ) : (
            <EmptyState icon={Users} title={t("wbStats.externalCredit.empty")} description={t("wbStats.externalCredit.description")} action={<Button size="sm" onClick={() => setAddDrawerOpen(true)}><Plus />{t("wbStats.externalCredit.add")}</Button>} />
          )}
          {accounts.length ? <p className="mt-3 text-[11px] text-muted-foreground">{accounts.length} {t("wbStats.externalCredit.title")}</p> : null}
        </div>
      </Card>
      {dialogs}
    </>
  );
}
