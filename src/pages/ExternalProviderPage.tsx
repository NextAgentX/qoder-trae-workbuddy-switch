import { useCallback, useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { AlertTriangle, CloudOff, Coins, Copy, FolderOpen, Info, Loader2, PlugZap, RefreshCw, Users } from "lucide-react";
import { toast } from "sonner";

import { ExternalCreditSources } from "@/components/external-credit-sources";
import { SideSubnav, type SideSubnavItem } from "@/components/shell/side-subnav";
import { GatewayShell } from "@/components/gateway/gateway-shell";
import { SettingsFieldRow, SettingsGroup } from "@/components/settings-primitives";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { CreditBalanceArea } from "@/components/usage/credit-balance-area";
import { StatTiles } from "@/components/usage/stat-tiles";
import { ViewParamBar } from "@/components/usage/view-param-bar";
import { formatCredits } from "@/components/usage/format";
import { copyText } from "@/lib/clipboard";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { platformDescriptor, platformOfPath } from "@/lib/platforms/registry";
import { scopeOf } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { getExternalBalanceSamples, getExternalPlatformSettings, recordExternalBalanceSamples, saveExternalPlatformSettings, type ExternalPlatformSettings } from "@/lib/external-platforms";
import type { TranslationKey } from "@/locales/zh";
import type { BalancePoint } from "@/lib/usage-charts";
import { dateKey, usageWindowBounds, type UsageWindow } from "@/lib/usage-window";

const PROVIDER_DETAILS: Record<api.ExternalCreditProvider, { unit: TranslationKey; auth: TranslationKey }> = {
  doubao: { unit: "externalPlatforms.unit.credits", auth: "externalPlatforms.auth.doubao" },
  loomy: { unit: "externalPlatforms.unit.credits", auth: "externalPlatforms.auth.loomy" },
  raccoon: { unit: "externalPlatforms.unit.credits", auth: "externalPlatforms.auth.raccoon" },
};

/** 路由 → provider。fail-closed：不是三家外部平台就返回 `null`，由页面渲染「不支持」空态。
 *  静默回落 `doubao` 会在新增第 4 个平台（或注册表 `home` 写错）时把豆包的账号和余额
 *  显示在另一个平台名下 —— 正是 `parseScope` 那条 fail-closed 纪律要防的错数据。 */
function providerFromPath(path: string): api.ExternalCreditProvider | null {
  const product = platformOfPath(path);
  return product === "doubao" || product === "loomy" || product === "raccoon" ? product : null;
}

type ExternalDestination = "overview" | "accounts" | "usage" | "gateway" | "settings";
type ExternalGatewaySection = "overview" | "models" | "integration" | "requests";
type ExternalSettingsSection = "refresh" | "data";

const EXTERNAL_GATEWAY_SECTIONS: readonly SideSubnavItem<ExternalGatewaySection>[] = [
  { id: "overview", labelKey: "externalPlatforms.gateway.tab.overview" },
  { id: "models", labelKey: "externalPlatforms.gateway.tab.models" },
  { id: "integration", labelKey: "externalPlatforms.gateway.tab.integration" },
  { id: "requests", labelKey: "externalPlatforms.gateway.requestLog" },
];

const EXTERNAL_SETTINGS_SECTIONS: readonly SideSubnavItem<ExternalSettingsSection>[] = [
  { id: "refresh", labelKey: "externalPlatforms.settings.subnav.refresh" },
  { id: "data", labelKey: "externalPlatforms.settings.subnav.data" },
];

function ExternalPageError({ message, onRetry }: { message: string; onRetry: () => void }) {
  const t = useT();
  return (
    <Alert variant="destructive" className="mb-5">
      <AlertTriangle />
      <AlertTitle>{t("wbStats.gateway.opFail")}</AlertTitle>
      <AlertDescription className="flex flex-col gap-3">
        <span>{message}</span>
        <div><Button size="sm" variant="outline" onClick={onRetry}><RefreshCw />{t("wbStats.gateway.retry")}</Button></div>
      </AlertDescription>
    </Alert>
  );
}

function ExternalProviderPage({ destination }: { destination: ExternalDestination }) {
  const t = useT();
  const location = useLocation();
  const navigate = useNavigate();
  const provider = providerFromPath(location.pathname);
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);
  if (!provider) {
    return (
      <div className="mx-auto flex w-full max-w-[1240px] min-w-0 flex-col gap-5 px-7 pt-[26px] pb-[60px]">
        <Card className="gap-0 py-0">
          <EmptyState
            icon={CloudOff}
            title={t("externalPlatforms.sessions.unsupportedTitle")}
            description={t("externalPlatforms.sessions.unsupportedDescription")}
            action={<Button size="sm" variant="outline" onClick={() => navigate(`/s/${scope}/overview`)}>{t("nav.overview")}</Button>}
          />
        </Card>
      </div>
    );
  }
  return <ExternalProviderBody destination={destination} provider={provider} />;
}

function ExternalProviderBody({ destination, provider }: { destination: ExternalDestination; provider: api.ExternalCreditProvider }) {
  const t = useT();
  const location = useLocation();
  const navigate = useNavigate();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);
  const [accounts, setAccounts] = useState<api.ExternalCreditAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [accountError, setAccountError] = useState("");
  const [gatewayError, setGatewayError] = useState("");
  const [requestUsageError, setRequestUsageError] = useState("");
  const [settings, setSettings] = useState<ExternalPlatformSettings>(() => getExternalPlatformSettings(provider));
  const [gateway, setGateway] = useState<api.ExternalGatewayStatus | null>(null);
  const [gatewayLoading, setGatewayLoading] = useState(false);
  const [gatewayBusy, setGatewayBusy] = useState(false);
  const [gatewayModels, setGatewayModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [requestUsage, setRequestUsage] = useState<Array<{ id: string; protocol: string; model: string; status: number; inputTokens?: number; outputTokens?: number; totalTokens?: number; completedAt: number }>>([]);
  const [requestUsageLoading, setRequestUsageLoading] = useState(false);
  const [usageView, setUsageView] = useState<"trend" | "requests" | "samples">("trend");
  const [viewWindow, setViewWindow] = useState<UsageWindow>({ kind: "preset", days: 14 });
  const { busy: refreshBusy, run: runRefresh } = useRefreshAction();
  const productName = t(platformDescriptor(product).labelKey);
  const sectionParam = new URLSearchParams(location.search).get("section");
  const gatewaySection: ExternalGatewaySection = sectionParam === "models" || sectionParam === "integration" || sectionParam === "requests" ? sectionParam : "overview";
  const settingsSection: ExternalSettingsSection = sectionParam === "data" ? "data" : "refresh";
  const navigateSection = (section: ExternalGatewaySection | ExternalSettingsSection) => navigate({ pathname: location.pathname, search: `?section=${section}` });

  const reload = useCallback(async () => {
    setLoading(true);
    setAccountError("");
    try {
      const next = await api.getExternalCreditProviderAccounts(provider);
      setAccounts(next);
      recordExternalBalanceSamples(provider, next);
    } catch (cause) {
      setAccountError(api.asError(cause));
    } finally {
      setLoading(false);
    }
  }, [provider]);

  useEffect(() => { void reload(); }, [reload]);

  /* 平台切换（同一组件实例被复用）时，上一家的账号、网关与用量都要立刻清空，
     否则会先显示别家的余额再被新数据覆盖。 */
  useEffect(() => {
    setAccounts([]);
    setAccountError("");
    setGateway(null);
    setGatewayModels([]);
    setGatewayError("");
    setRequestUsage([]);
    setRequestUsageError("");
  }, [provider]);

  const refreshGateway = useCallback(async () => {
    setGatewayLoading(true);
    setGatewayError("");
    try {
      const status = await api.getExternalGatewayStatus(provider);
      setGateway(status);
      if (status.running) {
        setModelsLoading(true);
        try { setGatewayModels((await api.getExternalGatewayModels(provider)).data.map((model) => model.id)); }
        catch (cause) { setGatewayModels([]); setGatewayError(api.asError(cause)); }
        finally { setModelsLoading(false); }
      } else setGatewayModels([]);
    } catch (cause) { setGatewayError(api.asError(cause)); }
    finally { setGatewayLoading(false); }
  }, [provider]);

  useEffect(() => { if (destination === "gateway" || destination === "overview") void refreshGateway(); }, [destination, refreshGateway]);

  const refreshRequestUsage = useCallback(async () => {
    setRequestUsageLoading(true);
    setRequestUsageError("");
    try {
      const body = await api.getExternalGatewayUsage(provider);
      setRequestUsage(body.data ?? []);
    } catch (cause) { setRequestUsageError(api.asError(cause)); }
    finally { setRequestUsageLoading(false); }
  }, [provider]);

  useEffect(() => { if (destination === "usage") void refreshRequestUsage(); }, [destination, refreshRequestUsage]);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    setAccountError("");
    try {
      const result = await api.refreshExternalCreditProvider(provider);
      const next = result.accounts ?? [];
      setAccounts(next);
      recordExternalBalanceSamples(provider, next);
    } catch (cause) { setAccountError(api.asError(cause)); }
    finally { setRefreshing(false); }
  }, [provider]);

  useEffect(() => { setSettings(getExternalPlatformSettings(provider)); }, [provider]);

  /* 账号页抽屉里的「更多操作」开关写的是同一份本机设置；这里在页面重新获得焦点时重读一次，
     避免「在抽屉里改了、切到设置页还是旧值」这种同源不同步。 */
  useEffect(() => {
    const sync = () => setSettings(getExternalPlatformSettings(provider));
    window.addEventListener("focus", sync);
    return () => window.removeEventListener("focus", sync);
  }, [provider]);

  useEffect(() => {
    if (!settings.autoRefreshMinutes || !accounts.length || !["overview", "usage"].includes(destination)) return;
    const timer = window.setInterval(() => { if (!refreshing) void refresh(); }, settings.autoRefreshMinutes * 60_000);
    return () => window.clearInterval(timer);
  }, [accounts.length, destination, refreshing, refresh, settings.autoRefreshMinutes]);

  const updateSettings = (next: ExternalPlatformSettings) => {
    setSettings(next);
    saveExternalPlatformSettings(provider, next);
    toast.success(t("externalPlatforms.settings.saved"));
  };

  const setGatewayRunning = async (running: boolean) => {
    setGatewayBusy(true);
    setGatewayError("");
    try {
      const status = running ? await api.startExternalGateway(provider) : await api.stopExternalGateway(provider);
      setGateway(status);
      if (status.running) {
        setModelsLoading(true);
        try {
          setGatewayModels((await api.getExternalGatewayModels(provider)).data.map((model) => model.id));
        } finally {
          setModelsLoading(false);
        }
      } else {
        setGatewayModels([]);
      }
    } catch (cause) { setGatewayError(api.asError(cause)); }
    finally { setGatewayBusy(false); }
  };

  const samples = useMemo(() => getExternalBalanceSamples(provider), [provider, accounts]);
  const isLow = (account: api.ExternalCreditAccount) =>
    settings.lowBalanceThreshold > 0 && account.remaining != null && account.remaining < settings.lowBalanceThreshold;
  const lastUpdated = useMemo(() => {
    const latest = Math.max(0, ...accounts.map((account) => account.updatedAt ?? 0));
    return latest > 0 ? new Date(latest).toLocaleString() : null;
  }, [accounts]);
  const totalBalance = accounts.reduce((sum, account) => sum + (account.remaining ?? 0), 0);
  const hasBalance = accounts.some((account) => account.remaining != null);
  const accountShortcut = <Button size="sm" variant="outline" onClick={() => navigate(`/s/${scope}/accounts`)}>{t("nav.accounts")}</Button>;
  const pageTitle = destination === "accounts" ? t("nav.accounts")
    : destination === "usage" ? t("nav.usage")
      : destination === "gateway" ? t("nav.apiService")
        : destination === "settings" ? t("nav.settings") : t("nav.overview");

  const header = (
    <header className="mb-5 flex items-start gap-4">
      <div className="min-w-0">
        <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{pageTitle}</h1>
        <p className="mt-1.5 text-[12.5px] text-muted-foreground">
          {destination === "overview" ? t("externalPlatforms.pageHead.overview", { scope: scopeLabel(t, scope), provider: productName })
            : destination === "accounts" ? t("pageHead.accounts", { scope: scopeLabel(t, scope), n: accounts.length })
              : destination === "usage" ? t("externalPlatforms.pageHead.usage", { scope: scopeLabel(t, scope), provider: productName })
                : destination === "gateway" ? t("pageHead.gateway", { scope: scopeLabel(t, scope) })
                  : t("pageHead.settings", { scope: scopeLabel(t, scope) })}
        </p>
      </div>
      <div className="grow" />
      {destination === "overview" || destination === "usage" ? (
        <Button className="shrink-0" size="default" variant="outline" onClick={() => void runRefresh(refresh)} disabled={refreshBusy || refreshing || loading}>
          {refreshBusy || refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}{t("wbStats.externalCredit.refresh")}
        </Button>
      ) : null}
      <ScopeChip scope={scope} />
    </header>
  );

  let content;
  if (destination === "accounts") {
    content = <ExternalCreditSources lockedProvider={provider} lowBalanceThreshold={settings.lowBalanceThreshold} onAccountsChange={setAccounts} />;
  } else if (destination === "gateway") {
    content = (
      /* 与 WorkBuddy 网关页同一个骨架 `GatewayShell`：左侧「类型 + 子导航」栏、右侧内容区。 */
      <GatewayShell
        type="independent"
        section={gatewaySection as never}
        sections={EXTERNAL_GATEWAY_SECTIONS as never}
        onTypeChange={() => undefined}
        onSelect={(id) => navigateSection(id as ExternalGatewaySection)}
      >
      <div className="space-y-4">
        {gatewayError ? <ExternalPageError message={gatewayError} onRetry={() => void refreshGateway()} /> : null}
        <Card className="gap-0 overflow-hidden py-0">
          <div className="flex flex-wrap items-center gap-3 border-b border-border/60 px-5 py-3">
            <PlugZap className="size-4 text-muted-foreground" />
            <div className="min-w-0 grow"><h2 className="text-[13px] font-[550]">{t("externalPlatforms.gateway.title", { provider: productName })}</h2><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.gateway.description")}</p></div>
            {gatewayLoading && !gateway ? <Skeleton className="h-6 w-16 rounded-md" aria-busy="true" /> : <Badge variant={gateway?.running ? "success" : "secondary"} className="gap-1.5">{gatewayLoading ? <Loader2 className="size-3 animate-spin" aria-hidden="true" /> : null}{gateway?.running ? t("externalPlatforms.gateway.running") : t("externalPlatforms.gateway.stopped")}</Badge>}
            {gateway?.running ? <Button size="sm" variant="outline" onClick={() => void refreshGateway()} disabled={modelsLoading || gatewayLoading}>{modelsLoading || gatewayLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}{t("externalPlatforms.gateway.refreshModels")}</Button> : null}
            <Button size="sm" onClick={() => void setGatewayRunning(!gateway?.running)} disabled={gatewayBusy || gatewayLoading || !gateway}>{gatewayBusy ? <Loader2 className="animate-spin" /> : null}{gateway?.running ? t("externalPlatforms.gateway.stop") : t("externalPlatforms.gateway.start")}</Button>
          </div>
          <div className="px-5 py-4">
            {gatewaySection === "overview" ? (
              <div className="space-y-4">
                <p className="text-[12px] text-muted-foreground">{t(provider === "doubao" ? "externalPlatforms.gateway.doubaoLimit" : provider === "loomy" ? "externalPlatforms.gateway.loomyLimit" : "externalPlatforms.gateway.raccoonLimit")}</p>
                {gatewayLoading && !gateway ? <div className="space-y-2" aria-busy="true"><Skeleton className="h-10 w-full" /><Skeleton className="h-10 w-2/3" /></div> : gateway ? <dl className="grid gap-3 rounded-lg border bg-muted/20 p-3 sm:grid-cols-2"><div><dt className="text-[11.5px] text-muted-foreground">{t("externalPlatforms.gateway.endpoint")}</dt><dd className="mt-1 flex items-center gap-2"><span className="min-w-0 flex-1 select-all break-all font-mono text-[12px]">{gateway.baseUrl}/v1</span><Button size="icon" variant="ghost" aria-label={t("externalPlatforms.gateway.copyEndpoint")} onClick={() => void copyText(`${gateway.baseUrl}/v1`, t("wbStats.gateway.baseUrlCopied"))}><Copy /></Button></dd></div><div><dt className="text-[11.5px] text-muted-foreground">{t("externalPlatforms.gateway.apiKey")}</dt><dd className="mt-1 flex items-center gap-2"><span className="min-w-0 flex-1 select-all break-all font-mono text-[12px]">{gateway.apiKey}</span><Button size="icon" variant="ghost" aria-label={t("externalPlatforms.gateway.copyKey")} onClick={() => void copyText(gateway.apiKey)}><Copy /></Button></dd></div></dl> : <EmptyState icon={PlugZap} title={t("externalPlatforms.gateway.statusUnavailable")} description={gatewayError || undefined} action={<Button size="sm" variant="outline" onClick={() => void refreshGateway()}>{t("wbStats.gateway.retry")}</Button>} />}
                {!gateway?.running ? accountShortcut : null}
                {/* 端口 / 绑定地址 / 账号池 / 模型数：WorkBuddy 网关概览里的那组事实行。 */}
                {gateway ? (
                  <div className="flex flex-wrap items-center gap-x-6 gap-y-3 rounded-lg border px-4 py-3 text-xs">
                    <span className="flex items-center gap-2">
                      <span className="text-muted-foreground">{t("externalPlatforms.gateway.portLabel")}</span>
                      <code className="font-mono">{gateway.port}</code>
                    </span>
                    <span className="flex items-center gap-2">
                      <span className="text-muted-foreground">{t("externalPlatforms.gateway.bindLabel")}</span>
                      <span>{t("externalPlatforms.gateway.loopback")}</span>
                    </span>
                    <span className="flex items-center gap-2">
                      <span className="text-muted-foreground">{t("externalPlatforms.gateway.poolTitle")}</span>
                      {accounts.length
                        ? <span>{t("externalPlatforms.gateway.poolCount", { n: accounts.length })}</span>
                        : <span className="text-warning">{t("externalPlatforms.gateway.poolEmpty")}</span>}
                    </span>
                    {gateway.running ? (
                      <span className="flex items-center gap-2">
                        <span className="text-muted-foreground">{t("externalPlatforms.gateway.models")}</span>
                        <span>{modelsLoading ? "…" : t("externalPlatforms.gateway.modelCount", { n: gatewayModels.length })}</span>
                      </span>
                    ) : null}
                    <span className="flex items-center gap-2">
                      <Button size="sm" variant="ghost" onClick={() => void refreshRequestUsage()}>
                        {requestUsageLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                        {t("externalPlatforms.gateway.requestLog")}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => void api.openDataDir(scope)}>
                        <FolderOpen />{t("externalPlatforms.gateway.openDataDir")}
                      </Button>
                    </span>
                  </div>
                ) : null}
                {/* 账号池隔离说明（与 WorkBuddy 网关「说明行」同一位置与语气）。 */}
                <p className="text-[11.5px] text-muted-foreground">
                  {t("externalPlatforms.gateway.poolNote")}
                </p>
                {requestUsageError ? <ExternalPageError message={requestUsageError} onRetry={() => void refreshRequestUsage()} /> : null}
                {/* 最近失败原因：启动失败/上游报错时唯一能自证的信息。 */}
                {gatewayError ? (
                  <div className="rounded-lg border px-4 py-3">
                    <p className="text-[11.5px] text-muted-foreground">{t("externalPlatforms.gateway.lastError")}</p>
                    <p className="mt-1 break-words text-xs text-destructive">{gatewayError}</p>
                  </div>
                ) : null}
              </div>
            ) : null}
            {gatewaySection === "models" ? (
              <div className="space-y-4">
                {gatewayLoading || modelsLoading ? <div className="space-y-2" aria-busy="true"><Skeleton className="h-8 w-56" /><Skeleton className="h-8 w-80" /></div> : gateway?.running ? gatewayModels.length ? <div className="flex flex-wrap gap-2">{gatewayModels.map((model) => <span key={model} className="rounded-md border bg-card px-2 py-1 font-mono text-xs">{model}</span>)}</div> : <EmptyState icon={PlugZap} title={t("externalPlatforms.gateway.noModels")} description={t("externalPlatforms.gateway.description")} /> : <EmptyState icon={PlugZap} title={t("externalPlatforms.gateway.startForModels")} description={t("externalPlatforms.gateway.modelsNote")} action={<Button size="sm" onClick={() => void setGatewayRunning(true)} disabled={gatewayBusy || !gateway}>{t("externalPlatforms.gateway.start")}</Button>} />}
              </div>
            ) : null}
            {gatewaySection === "integration" ? (
              <div className="space-y-3">
                {gatewayLoading || modelsLoading ? <div className="space-y-2" aria-busy="true"><Skeleton className="h-4 w-48" /><Skeleton className="h-28 w-full" /></div> : gateway?.running ? <><div><h3 className="text-[13px] font-[550]">{t("externalPlatforms.gateway.integrationTitle")}</h3><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.gateway.integrationDescription")}</p></div><div className="space-y-2"><Label>{t("externalPlatforms.gateway.example")}</Label><pre className="overflow-x-auto rounded-lg border bg-muted/40 p-3 text-xs">{`curl ${gateway.baseUrl}/v1/chat/completions \\
  -H "Authorization: Bearer ${gateway.apiKey}" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"${gatewayModels[0] ?? "<model-id>"}","messages":[{"role":"user","content":"Hello"}],"stream":true}'`}</pre></div>{provider === "doubao" ? <div className="space-y-2 border-t pt-4">{[
                  { model: "doubao-image", path: "images/generations", prompt: "一只柴犬在樱花树下" },
                  { model: "doubao-video", path: "video/generations", prompt: "海边日落，镜头缓慢向前推进" },
                  { model: "doubao-music", path: "audio/generations", prompt: "轻快的爵士钢琴纯音乐" },
                ].map((sample) => <div key={sample.model} className="space-y-2"><Label>{sample.model}</Label><pre className="overflow-x-auto rounded-lg border bg-muted/40 p-3 text-xs">{`curl ${gateway.baseUrl}/v1/${sample.path} \\
  -H "Authorization: Bearer ${gateway.apiKey}" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"${sample.model}","prompt":"${sample.prompt}"}'`}</pre></div>)}</div> : null}</> : <EmptyState icon={PlugZap} title={t("externalPlatforms.gateway.integrationUnavailable")} description={t("externalPlatforms.gateway.modelsNote")} action={<Button size="sm" onClick={() => void setGatewayRunning(true)} disabled={gatewayBusy || !gateway}>{t("externalPlatforms.gateway.startForIntegration")}</Button>} />}
              </div>
            ) : null}
            {/* 请求记录：WorkBuddy 网关概览有「请求日志」视图，外部平台此前只在用量页有，
                网关页看不到 —— 这里补同一份账本视图（数据源 `getExternalGatewayUsage`）。 */}
            {gatewaySection === "requests" ? (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-3">
                  <p className="min-w-0 grow text-[11.5px] text-muted-foreground">{t("externalPlatforms.usage.requestsNote")}</p>
                  <Button size="sm" variant="outline" onClick={() => void refreshRequestUsage()} disabled={requestUsageLoading}>
                    {requestUsageLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                    {t("wbStats.externalCredit.refresh")}
                  </Button>
                </div>
                {requestUsageLoading ? (
                  <div className="space-y-2" aria-busy="true"><Skeleton className="h-12 w-full" /><Skeleton className="h-12 w-full" /></div>
                ) : requestUsage.length ? (
                  <>
                    <p className="text-[11px] text-muted-foreground">{t("externalPlatforms.gateway.requestCount", { n: requestUsage.length })}</p>
                    <div className="max-h-96 divide-y rounded-lg border">
                      {requestUsage.slice().sort((a, b) => b.completedAt - a.completedAt).map((row) => (
                        <div key={row.id} className="grid gap-1 px-3 py-2.5 text-xs sm:grid-cols-[1fr_auto_auto]">
                          <span className="truncate font-mono">{row.model} · {row.protocol}</span>
                          <span className={row.status >= 400 ? "text-destructive" : "text-muted-foreground"}>HTTP {row.status}</span>
                          <span className="text-muted-foreground">{row.inputTokens != null || row.outputTokens != null ? `${row.inputTokens ?? 0} + ${row.outputTokens ?? 0} tokens` : row.totalTokens != null ? `${row.totalTokens} tokens` : "—"}</span>
                          <span className="text-muted-foreground sm:col-span-3">{new Date(row.completedAt).toLocaleString()}</span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <EmptyState icon={PlugZap} title={t("externalPlatforms.usage.noRequests")} description={t("externalPlatforms.gateway.noRequestsYet")} />
                )}
              </div>
            ) : null}
          </div>
        </Card>
      </div>
      </GatewayShell>
    );
  } else if (destination === "settings") {
    content = (
      <div data-split-layout className="min-w-0">
        <SideSubnav active={settingsSection} items={EXTERNAL_SETTINGS_SECTIONS} onSelect={navigateSection} ariaLabel={t("externalPlatforms.settings.title", { provider: productName })} />
        <div className="min-w-0 flex-1 space-y-4">
          {/* 与 WorkBuddy / Trae 设置页同一形态（issue #122 的「这一页影响谁」说明条）。 */}
          <div className="flex items-start gap-2 rounded-[10px] bg-muted px-3 py-2 text-[12px] leading-[1.5] text-muted-foreground">
            <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
            <span>{t("externalPlatforms.settings.scopeNote", { provider: productName })}</span>
          </div>
          {settingsSection === "refresh" ? <SettingsGroup id="external-platform-settings" title={t("externalPlatforms.settings.title", { provider: productName })}>
            <SettingsFieldRow label={t("externalPlatforms.settings.refreshInterval")} description={t("externalPlatforms.settings.refreshDescription")}>
              <Select value={String(settings.autoRefreshMinutes)} onValueChange={(value) => updateSettings({ ...settings, autoRefreshMinutes: Number(value) as ExternalPlatformSettings["autoRefreshMinutes"] })}>
                <SelectTrigger className="h-8 w-full sm:w-[240px]"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="0">{t("externalPlatforms.settings.refreshOff")}</SelectItem>{[5, 15, 30, 60].map((minutes) => <SelectItem key={minutes} value={String(minutes)}>{t("externalPlatforms.settings.refreshMinutes", { minutes })}</SelectItem>)}</SelectContent>
              </Select>
            </SettingsFieldRow>
            <SettingsFieldRow htmlFor="external-low-balance" label={t("externalPlatforms.settings.lowBalance")} description={t("externalPlatforms.settings.lowBalanceHelp")}>
              <Input id="external-low-balance" className="h-8 w-full sm:w-[240px]" type="number" min="0" step="1" value={settings.lowBalanceThreshold} onChange={(event) => setSettings({ ...settings, lowBalanceThreshold: Math.max(0, Number(event.target.value) || 0) })} onBlur={() => { if (getExternalPlatformSettings(provider).lowBalanceThreshold !== settings.lowBalanceThreshold) updateSettings(settings); }} />
            </SettingsFieldRow>
          </SettingsGroup> : null}
          {settingsSection === "data" ? <SettingsGroup id="external-platform-account-info" title={t("externalPlatforms.settings.localDataTitle")}>
            <SettingsFieldRow label={t("externalPlatforms.settings.auth")}><span className="text-[12px] text-muted-foreground">{t(PROVIDER_DETAILS[provider].auth)}</span></SettingsFieldRow>
            <SettingsFieldRow label={t("externalPlatforms.settings.data")}><span className="text-[12px] text-muted-foreground">{t("externalPlatforms.settings.dataValue")}</span></SettingsFieldRow>
            <SettingsFieldRow label={t("externalPlatforms.settings.accounts")}><Button size="sm" variant="outline" onClick={() => navigate(`/s/${scope}/accounts`)}>{t("nav.accounts")}</Button></SettingsFieldRow>
          </SettingsGroup> : null}
        </div>
      </div>
    );
  } else if (destination === "usage") {
    const currentIds = new Set(accounts.map((account) => account.id));
    const { from, to } = usageWindowBounds(viewWindow);
    const inWindow = (timestamp: number) => {
      const day = dateKey(new Date(timestamp));
      return day >= from && day <= to;
    };
    const history = samples.filter((sample) => currentIds.has(sample.accountId) && inWindow(sample.sampledAt)).slice().sort((a, b) => b.sampledAt - a.sampledAt).slice(0, 500);
    /* 每个账号「上一条采样」一次算好：逐行现扫再排序是 O(n² log n)，500 行 × 5000 条会卡住渲染。
       用**全量** `samples` 而不是窗口内的 `history` —— 窗口第一条的「上一条」可能在窗口之外。
       只在「本机快照」视图需要，别的视图不算。 */
    const previousBySample = usageView === "samples" ? (() => {
      const index = new Map<string, (typeof samples)[number] | null>();
      const byAccount = new Map<string, (typeof samples)[number][]>();
      for (const sample of samples) {
        const bucket = byAccount.get(sample.accountId);
        if (bucket) bucket.push(sample);
        else byAccount.set(sample.accountId, [sample]);
      }
      for (const bucket of byAccount.values()) {
        bucket.sort((a, b) => a.sampledAt - b.sampledAt);
        bucket.forEach((sample, position) => {
          index.set(`${sample.accountId}-${sample.sampledAt}`, position > 0 ? bucket[position - 1] : null);
        });
      }
      return index;
    })() : new Map<string, (typeof samples)[number] | null>();
    const filteredRequests = requestUsage.filter((row) => inWindow(row.completedAt)).slice().sort((a, b) => b.completedAt - a.completedAt);
    const daily = new Map<string, Map<string, (typeof history)[number]>>();
    for (const sample of history) {
      const day = dateKey(new Date(sample.sampledAt));
      const perAccount = daily.get(day) ?? new Map();
      const previous = perAccount.get(sample.accountId);
      if (!previous || previous.sampledAt < sample.sampledAt) perAccount.set(sample.accountId, sample);
      daily.set(day, perAccount);
    }
    const rows = [...daily.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([day, perAccount]) => ({
      label: new Date(`${day}T00:00:00`).toLocaleDateString(undefined, { month: "numeric", day: "numeric" }),
      value: [...perAccount.values()].reduce((sum, sample) => sum + sample.remaining, 0),
    }));
    const values = rows.map((row) => row.value);
    const min = Math.min(...values);
    const max = Math.max(...values);
    const baseline = rows.length ? min - Math.max((max - min) * 0.35, Math.abs(min) * 0.01, 1) : 0;
    const points: BalancePoint[] = rows.map((row) => ({ ...row, baseline }));
    const knownTokens = filteredRequests.reduce((sum, row) => sum + (row.totalTokens ?? ((row.inputTokens ?? 0) + (row.outputTokens ?? 0))), 0);
    const hasKnownTokens = filteredRequests.some((row) => row.totalTokens != null || row.inputTokens != null || row.outputTokens != null);
    const usageTiles = usageView === "trend"
      ? [
          { icon: Coins, label: t("externalPlatforms.overview.balance"), value: loading ? "—" : hasBalance ? `${formatCredits(totalBalance)} ${t(PROVIDER_DETAILS[provider].unit)}` : "—" },
          { icon: RefreshCw, label: t("externalPlatforms.usage.sampleCount"), value: String(history.length), hint: t("externalPlatforms.usage.sampleNote") },
          { icon: Users, label: t("externalPlatforms.overview.accounts"), value: loading ? "—" : String(accounts.length) },
        ]
      : usageView === "requests"
        ? [
            { icon: PlugZap, label: t("externalPlatforms.usage.requestsTitle"), value: requestUsageLoading ? "—" : String(filteredRequests.length) },
            { icon: Coins, label: t("externalPlatforms.usage.tokensReported"), value: requestUsageLoading ? "—" : hasKnownTokens ? formatCredits(knownTokens) : "—", hint: t("externalPlatforms.usage.tokenSource") },
            { icon: AlertTriangle, label: t("externalPlatforms.usage.failedRequests"), value: requestUsageLoading ? "—" : String(filteredRequests.filter((row) => row.status >= 400).length) },
          ]
        : [
            { icon: RefreshCw, label: t("externalPlatforms.usage.sampleCount"), value: String(history.length) },
            { icon: Users, label: t("externalPlatforms.overview.accounts"), value: String(new Set(history.map((sample) => sample.accountId)).size) },
            { icon: Coins, label: t("externalPlatforms.usage.sampledBalance"), value: history.length ? `${formatCredits(history[0].remaining)} ${t(PROVIDER_DETAILS[provider].unit)}` : "—" },
          ];
    content = (
      <div className="space-y-4">
        {/* 与 WorkBuddy 用量页同一节奏：Tab 条 `mb-4` → 参数条 `mb-4` → 内容。
            ⚠️ 参数条**不放进 `Tabs` 内部**：它不属于任何一个 Tab 面板，
            放进去会让切 Tab 时连带重建，且结构上多一层 gap。 */}
        <Tabs value={usageView} onValueChange={(value) => setUsageView(value as typeof usageView)}>
          <TabsList className="mb-4">
            <TabsTrigger value="trend">{t("externalPlatforms.usage.view.trend")}</TabsTrigger>
            <TabsTrigger value="requests">{t("externalPlatforms.usage.requestsTitle")}</TabsTrigger>
            <TabsTrigger value="samples">{t("externalPlatforms.usage.samples")}</TabsTrigger>
          </TabsList>
          <ViewParamBar
            viewWindow={viewWindow}
            onViewWindowChange={setViewWindow}
            combineAll={false}
            onCombineAllChange={() => undefined}
            canMergeScopes={false}
            className="mb-4"
          />
          <StatTiles tiles={usageTiles} ariaLabel={t("externalPlatforms.usage.title", { provider: productName })} />
        {accountError ? <ExternalPageError message={accountError} onRetry={() => void runRefresh(() => reload())} /> : null}
        {requestUsageError ? <ExternalPageError message={requestUsageError} onRetry={() => void refreshRequestUsage()} /> : null}
          <TabsContent value="trend">
            <Card className="gap-0 overflow-hidden py-0">
              <div className="border-b border-border/60 px-5 py-3"><h2 className="text-sm font-semibold">{t("externalPlatforms.usage.view.trend")}</h2><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.usage.sampleNote")}</p></div>
              <div className="px-5 py-4">{loading ? <div className="space-y-2" aria-busy="true"><Skeleton className="h-10 w-full" /><Skeleton className="h-10 w-2/3" /></div> : points.length ? <CreditBalanceArea points={points} /> : <EmptyState icon={Coins} title={t("externalPlatforms.usage.noSamples")} description={t("externalPlatforms.usage.noHistory")} action={accountShortcut} />}</div>
            </Card>
          </TabsContent>
          <TabsContent value="requests">
            <Card className="gap-0 overflow-hidden py-0">
              <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3"><div className="min-w-0 grow"><h2 className="text-sm font-semibold">{t("externalPlatforms.usage.requestsTitle")}</h2><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.usage.requestsNote")}</p></div><Button size="sm" variant="outline" onClick={() => void refreshRequestUsage()} disabled={requestUsageLoading}>{requestUsageLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}{t("wbStats.externalCredit.refresh")}</Button></div>
              <div className="px-5 py-4">{requestUsageLoading ? <div className="space-y-2" aria-busy="true"><Skeleton className="h-12 w-full" /><Skeleton className="h-12 w-full" /></div> : filteredRequests.length ? <div className="max-h-96 divide-y rounded-lg border">{filteredRequests.map((row) => <div key={row.id} className="grid gap-1 px-3 py-2.5 text-xs sm:grid-cols-[1fr_auto_auto]"><span className="truncate font-mono">{row.model} · {row.protocol}</span><span className={row.status >= 400 ? "text-destructive" : "text-muted-foreground"}>HTTP {row.status}</span><span className="text-muted-foreground">{row.inputTokens != null || row.outputTokens != null ? `${row.inputTokens ?? 0} + ${row.outputTokens ?? 0} tokens` : row.totalTokens != null ? `${row.totalTokens} tokens` : "—"}</span><span className="text-muted-foreground sm:col-span-3">{new Date(row.completedAt).toLocaleString()}</span></div>)}</div> : <EmptyState icon={PlugZap} title={t("externalPlatforms.usage.noRequests")} description={t("externalPlatforms.usage.requestsNote")} />}</div>
            </Card>
          </TabsContent>
          <TabsContent value="samples">
            <Card className="gap-0 overflow-hidden py-0">
              <div className="border-b border-border/60 px-5 py-3"><h2 className="text-sm font-semibold">{t("externalPlatforms.usage.samples")}</h2><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.usage.sampleNote")}</p></div>
              <div className="px-5 py-4">{history.length ? <div className="max-h-96 divide-y rounded-lg border">{history.map((sample) => { const previous = previousBySample.get(`${sample.accountId}-${sample.sampledAt}`) ?? null; const delta = previous ? sample.remaining - previous.remaining : null; return <div key={`${sample.accountId}-${sample.sampledAt}`} className="grid gap-1 px-3 py-2.5 text-xs sm:grid-cols-[1fr_auto_auto] sm:items-center sm:gap-4"><span className="truncate">{sample.name}</span><span className="tabular-nums font-medium">{formatCredits(sample.remaining)} {t(PROVIDER_DETAILS[provider].unit)}</span><span className="text-muted-foreground">{delta == null ? "—" : `${t("externalPlatforms.usage.delta")}: ${delta > 0 ? "+" : ""}${formatCredits(delta)}`} · {new Date(sample.sampledAt).toLocaleString()}</span></div>; })}</div> : <EmptyState icon={RefreshCw} title={t("externalPlatforms.usage.noSamples")} description={t("externalPlatforms.usage.sampleNote")} />}</div>
            </Card>
          </TabsContent>
        </Tabs>
      </div>
    );
  } else {
    const currentIds = new Set(accounts.map((account) => account.id));
    const overviewSamples = samples.filter((sample) => currentIds.has(sample.accountId));
    const byDay = new Map<string, Map<string, (typeof overviewSamples)[number]>>();
    for (const sample of overviewSamples) {
      const day = new Date(sample.sampledAt).toLocaleDateString();
      const perAccount = byDay.get(day) ?? new Map();
      const previous = perAccount.get(sample.accountId);
      if (!previous || previous.sampledAt < sample.sampledAt) perAccount.set(sample.accountId, sample);
      byDay.set(day, perAccount);
    }
    const trendRows = [...byDay.entries()].slice(-14).map(([label, perAccount]) => ({
      label,
      value: [...perAccount.values()].reduce((sum, sample) => sum + sample.remaining, 0),
    }));
    const trendValues = trendRows.map((row) => row.value);
    const trendMin = Math.min(...trendValues);
    const trendMax = Math.max(...trendValues);
    const trendBaseline = trendRows.length ? trendMin - Math.max((trendMax - trendMin) * 0.35, Math.abs(trendMin) * 0.01, 1) : 0;
    const trendPoints: BalancePoint[] = trendRows.map((row) => ({ ...row, baseline: trendBaseline }));
    const lowAccounts = accounts.filter(isLow);
    const erroredAccounts = accounts.filter((account) => account.error);
    const attentionCount = (accounts.length === 0 && !loading && !accountError ? 1 : 0)
      + lowAccounts.length + erroredAccounts.length + (accountError ? 1 : 0) + (gatewayError ? 1 : 0);
    /* 与 WorkBuddy 概览同一套 tile 语言：`hint` 说明派生关系、`title` 解释口径、
       `tone` 表达好坏（低余额 / 有错误账号用 warn），而不是只有四个裸数字。 */
    const tiles = [
      {
        icon: Users,
        label: t("externalPlatforms.overview.accounts"),
        value: loading ? "—" : String(accounts.length),
        hint: attentionCount > 0
          ? t("externalPlatforms.overview.accountsAttention", { n: attentionCount })
          : t("externalPlatforms.overview.accountsOk"),
        tone: attentionCount > 0 ? ("warn" as const) : undefined,
      },
      {
        icon: Coins,
        label: t("externalPlatforms.overview.balance"),
        value: loading ? "—" : hasBalance ? `${formatCredits(totalBalance)} ${t(PROVIDER_DETAILS[provider].unit)}` : "—",
        hint: hasBalance ? t("externalPlatforms.overview.balanceHint") : t("externalPlatforms.overview.balanceUnknown"),
        title: t("externalPlatforms.overview.balanceTitle"),
      },
      {
        icon: PlugZap,
        label: t("externalPlatforms.overview.gateway"),
        value: gatewayLoading || !gateway ? "—" : gateway.running ? t("externalPlatforms.gateway.running") : t("externalPlatforms.gateway.stopped"),
        tone: gateway?.running ? ("up" as const) : undefined,
      },
      {
        icon: RefreshCw,
        label: t("externalPlatforms.overview.updated"),
        value: loading ? "—" : lastUpdated ?? "—",
        hint: lastUpdated ? undefined : t("externalPlatforms.overview.neverUpdated"),
      },
    ];
    content = (
      <div className="space-y-5">
        <Card className="gap-0 overflow-hidden py-0">
          <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
            <h2 className="text-sm font-semibold">{t("externalPlatforms.overview.pending")}</h2>
            <div className="grow" />
            <span className="text-[11.5px] text-muted-foreground">{t("externalPlatforms.overview.pendingCount", { count: attentionCount })}</span>
          </div>
          {loading ? <div className="space-y-2 px-5 py-4" aria-busy="true"><Skeleton className="h-10 w-full" /><Skeleton className="h-10 w-2/3" /></div>
            : attentionCount === 0 ? <EmptyState icon={Users} title={t("externalPlatforms.overview.allClear")} description={t("externalPlatforms.overview.noPending")} />
              : <div className="divide-y">
                {accounts.length === 0 && !accountError ? <div className="flex flex-wrap items-center gap-3 px-5 py-3"><div className="min-w-0 grow"><p className="text-[12.5px] font-[550]">{t("externalPlatforms.overview.noAccountsTitle")}</p><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.overview.noAccountsDescription")}</p></div><Button size="sm" onClick={() => navigate(`/s/${scope}/accounts`)}>{t("externalPlatforms.overview.addAccount")}</Button></div> : null}
                {accountError ? <div className="flex flex-wrap items-center gap-3 px-5 py-3"><AlertTriangle className="size-4 shrink-0 text-destructive" /><div className="min-w-0 grow"><p className="text-[12.5px] font-[550]">{t("externalPlatforms.overview.refreshFailed")}</p><p className="mt-0.5 break-words text-[11.5px] text-destructive">{accountError}</p></div><Button size="sm" variant="outline" onClick={() => void runRefresh(() => reload())}>{t("wbStats.gateway.retry")}</Button></div> : null}
                {erroredAccounts.map((account) => <div key={`error-${account.id}`} className="flex flex-wrap items-center gap-3 px-5 py-3"><AlertTriangle className="size-4 shrink-0 text-destructive" /><div className="min-w-0 grow"><p className="text-[12.5px] font-[550]">{account.name}</p><p className="mt-0.5 break-words text-[11.5px] text-destructive">{account.error}</p></div><Button size="sm" variant="outline" onClick={() => void runRefresh(refresh)}>{t("wbStats.externalCredit.refresh")}</Button></div>)}
                {lowAccounts.map((account) => <div key={`low-${account.id}`} className="flex flex-wrap items-center gap-3 px-5 py-3"><AlertTriangle className="size-4 shrink-0 text-amber-600" /><div className="min-w-0 grow"><p className="text-[12.5px] font-[550]">{t("externalPlatforms.overview.lowBalanceTitle", { account: account.name })}</p><p className="mt-0.5 text-[11.5px] text-muted-foreground">{formatCredits(account.remaining ?? 0)} / {formatCredits(settings.lowBalanceThreshold)} {t(PROVIDER_DETAILS[provider].unit)}</p></div><Button size="sm" variant="outline" onClick={() => navigate(`/s/${scope}/accounts`)}>{t("externalPlatforms.overview.manageAccount")}</Button></div>)}
                {gatewayError ? <div className="flex flex-wrap items-center gap-3 px-5 py-3"><AlertTriangle className="size-4 shrink-0 text-destructive" /><div className="min-w-0 grow"><p className="text-[12.5px] font-[550]">{t("externalPlatforms.overview.gatewayUnavailable")}</p><p className="mt-0.5 break-words text-[11.5px] text-destructive">{gatewayError}</p></div><Button size="sm" variant="outline" onClick={() => void refreshGateway()}>{t("wbStats.gateway.retry")}</Button></div> : null}
              </div>}
        </Card>
        <StatTiles tiles={tiles} ariaLabel={t("externalPlatforms.overview.title", { provider: productName })} />
        {/* 主区 + 侧栏：用原型 `.split`（`[data-split-layout]`，gap 22px、900px 转纵向），
            与网关 / 设置两页同一份版式，而不是各写一套 `lg:` 断点。 */}
        <div data-split-layout className="min-w-0">
          <section className="min-w-0 flex-1">
            <Card className="gap-0 overflow-hidden py-0">
              <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3"><div className="min-w-0"><h2 className="text-sm font-semibold">{t("externalPlatforms.overview.trendTitle")}</h2><p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("externalPlatforms.overview.trendNote")}</p></div><div className="grow" /><Button size="sm" variant="ghost" onClick={() => navigate(`/s/${scope}/usage`)}>{t("externalPlatforms.overview.viewUsage")}</Button></div>
              <div className="px-5 py-4">{trendPoints.length ? <CreditBalanceArea points={trendPoints} /> : <EmptyState icon={RefreshCw} title={t("externalPlatforms.overview.noTrend")} description={t("externalPlatforms.overview.noTrendDescription")} action={<Button size="sm" variant="outline" onClick={() => void runRefresh(refresh)} disabled={refreshing || loading}>{t("wbStats.externalCredit.refresh")}</Button>} />}</div>
            </Card>
          </section>
          <aside className="flex w-full flex-col gap-4 lg:w-[290px] lg:shrink-0">
            <Card className="gap-0 overflow-hidden py-0">
              <div className="flex items-center border-b border-border/60 px-5 py-3"><h2 className="text-sm font-semibold">{t("externalPlatforms.overview.accountsSummary")}</h2><div className="grow" /><Button size="sm" variant="ghost" onClick={() => navigate(`/s/${scope}/accounts`)}>{t("nav.accounts")}</Button></div>
              {loading ? <div className="space-y-2 p-3" aria-busy="true"><Skeleton className="h-8 w-full" /><Skeleton className="h-8 w-full" /></div> : accounts.length ? <div className="divide-y">{accounts.slice(0, 5).map((account) => <div key={account.id} className="flex items-center justify-between gap-3 px-3 py-2.5 text-xs"><span className="flex min-w-0 items-center gap-2 truncate">{isLow(account) ? <AlertTriangle className="size-3.5 shrink-0 text-amber-500" /> : null}{account.name}</span><span className="shrink-0 tabular-nums font-medium">{account.remaining == null ? "—" : `${formatCredits(account.remaining)} ${account.unit ?? t(PROVIDER_DETAILS[provider].unit)}`}</span></div>)}</div> : <EmptyState icon={Users} title={t("wbStats.externalCredit.empty")} description={t("externalPlatforms.overview.description")} action={accountShortcut} />}
            </Card>
            <Card className="gap-0 overflow-hidden py-0"><div className="border-b border-border/60 px-5 py-3"><h2 className="text-sm font-semibold">{t("externalPlatforms.overview.capabilities")}</h2></div><div className="flex flex-wrap gap-1.5 p-3">{[t("externalPlatforms.overview.capability.credits"), t("externalPlatforms.overview.capability.gateway"), t("externalPlatforms.overview.capability.requestLog")].map((label) => <span key={label} className="rounded-md border bg-muted/40 px-2 py-1 text-[11px] text-muted-foreground">{label}</span>)}</div></Card>
          </aside>
        </div>
      </div>
    );
  }

  /* 与 WorkBuddy 五页同构：页头自带 `mb-5`，容器不再叠加 `gap-5`（否则页头下会多出 20px）。 */
  return <div className="mx-auto flex w-full max-w-[1240px] min-w-0 flex-col px-7 pt-[26px] pb-[60px]">{header}{content}</div>;
}

export function ExternalOverviewPage() {
  return <ExternalProviderPage destination="overview" />;
}

export function ExternalAccountsPage() {
  return <ExternalProviderPage destination="accounts" />;
}

export function ExternalUsagePage() {
  return <ExternalProviderPage destination="usage" />;
}

export function ExternalGatewayPage() {
  return <ExternalProviderPage destination="gateway" />;
}

export function ExternalSettingsPage() {
  return <ExternalProviderPage destination="settings" />;
}

export function ExternalSessionsPage() {
  const t = useT();
  const location = useLocation();
  const navigate = useNavigate();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);

  return (
    <div className="mx-auto flex w-full max-w-[1240px] min-w-0 flex-col gap-5 px-7 pt-[26px] pb-[60px]">
      <header className="flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("nav.sessions")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">{t("pageHead.sessions", { scope: scopeLabel(t, scope) })}</p>
        </div>
        <div className="grow" />
        <ScopeChip scope={scope} />
      </header>
      <Card className="gap-0 py-0">
        <EmptyState
          icon={CloudOff}
          title={t("externalPlatforms.sessions.unsupportedTitle")}
          description={t("externalPlatforms.sessions.unsupportedDescription")}
          action={<Button size="sm" variant="outline" onClick={() => navigate(`/s/${scope}/overview`)}>{t("nav.overview")}</Button>}
        />
      </Card>
    </div>
  );
}
