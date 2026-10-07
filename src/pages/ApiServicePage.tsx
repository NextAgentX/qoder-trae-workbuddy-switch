import { useEffect, useState } from "react";
import { AlertTriangle, Copy, FolderOpen, Loader2, Power, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { ApiKeyTable } from "@/components/gateway/api-key-table";
import { AccountPoolCard, AUTO_OPTION, WB_POOL_TILES } from "@/components/gateway/account-pool-card";
import { IntegrationGuide } from "@/components/gateway/integration-guide";
import { ModelList } from "@/components/gateway/model-list";
// P12 · 请求代理面板（**C 路：base URL 重定向**）—— 默认关，应用只记"用户已按引导设置"。
// ⚠️ 只接 WB 侧：C 路本期不做 Trae（`proxy.rs` 的 `PROXY_GATEWAY_BASE` 只覆盖 WB 网关端口）。
import { ProxyPanel } from "@/components/gateway/proxy-panel";
import { RequestLog } from "@/components/gateway/request-log";
import { WorkbuddyRegionSwitch } from "@/components/workbuddy-region-switch";
import { DemoAction } from "@/components/demo-action";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import * as api from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { useT } from "@/lib/i18n";
import { poolOf, resolveGatewayBaseUrl, resolveGatewayRunning } from "@/lib/gateway";
import { regionDescriptor } from "@/lib/region";
import { cn } from "@/lib/utils";
import type { AccountStrategy, ApiKeyRecord, GatewayConfig, Region } from "@/lib/types";
import { useWorkbuddyRegion } from "@/lib/use-workbuddy-region";
import { useGatewayStore } from "@/stores/gateway";

const LOOPBACK = "127.0.0.1";
const LAN = "0.0.0.0";

/**
 * 「按实时积分择优」的哨兵值。
 *
 * 它与 [`AUTO_OPTION`] 一样不是 uid：WorkBuddy 的选号策略有三档
 * （跟随登录态 / 指定账号 / 实时积分择优），而池卡只有一个「指定账号」下拉，
 * 故把非账号的两档也放进同一个下拉里，在边界处换成策略。
 */
const MAX_CREDITS_OPTION = "__max_credits__";

/** 无启用中的 Key 时返回 null，由渲染处给出占位提示文案。 */
function representativeKey(keys: ApiKeyRecord[], region: Region): string | null {
  const active = keys.find((key) => key.region === region && !key.revoked);
  return active ? `${active.prefix}…` : null;
}

/**
 * 「API 服务」页：网关开关、监听、Base URL、Key、模型、账号池、接入指引、请求日志（P0-11）。
 *
 * ## 版本是**页面级唯一**的（2026-09-30 与 Trae 页统一）
 *
 * 改造前这一页的版本轴散在四处且互不联动：接入地址并排两行、账号池并排两张、
 * 模型清单与接入指引各自内置一个 Tabs。用户在模型清单里切到国际版，
 * 上面的账号池还停在国内版 —— 同一页「当前版本」各说各话。
 *
 * 现在与 Trae 侧同一套心智模型：**页头唯一入口（`WorkbuddyRegionSwitch`）→
 * 整页跟随 → 子组件只渲染不选择**。版本由 URL 的 `?region=` 承载（可刷新、可分享、
 * 配合前进后退），见 `useWorkbuddyRegion`。
 */
export default function ApiServicePage() {
  const t = useT();
  // 只取当前值：切换动作由页头的 `WorkbuddyRegionSwitch` 自己发起（它才是那个入口）。
  const [region] = useWorkbuddyRegion();
  const config = useGatewayStore((s) => s.config);
  const status = useGatewayStore((s) => s.status);
  const keys = useGatewayStore((s) => s.keys);
  const loading = useGatewayStore((s) => s.loading);
  const error = useGatewayStore((s) => s.error);
  const loadAll = useGatewayStore((s) => s.loadAll);
  const saveConfig = useGatewayStore((s) => s.saveConfig);

  const [portDraft, setPortDraft] = useState(String(config.port));
  const [saving, setSaving] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);
  /** 数据目录按钮是否正在打开（只驱动按钮转圈）。 */
  const [openingDir, setOpeningDir] = useState(false);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    setPortDraft(String(config.port));
  }, [config.port]);

  async function persist(next: Partial<GatewayConfig>) {
    setSaving(true);
    try {
      await saveConfig({ ...config, ...next });
    } catch (e) {
      toast.error(t("wbStats.gateway.saveFail"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  function onBindAddrChange(next: string) {
    if (next === config.bind_addr) return;
    if (next === LOOPBACK) {
      void persist({ bind_addr: LOOPBACK, allow_non_loopback: false });
      return;
    }
    // 非回环监听需要风险确认（Q2 / U5）。
    setRiskOpen(true);
  }

  function confirmLan() {
    setRiskOpen(false);
    void persist({ bind_addr: LAN, allow_non_loopback: true });
  }

  function commitPort() {
    const parsed = Number.parseInt(portDraft, 10);
    if (!Number.isFinite(parsed) || parsed < 1 || parsed > 65535) {
      toast.error(t("wbStats.gateway.portRange"));
      setPortDraft(String(config.port));
      return;
    }
    if (parsed === config.port) return;
    void persist({ port: parsed });
  }

  /**
   * 在文件管理器中打开该版本的 WorkBuddy 数据目录。
   *
   * 非 Windows 时后端返回**结构化 `Unsupported`**（不是假成功）：此时如实把
   * 「当前平台不支持」提示给用户，而不是弹一句「已打开」却什么都没发生。
   */
  async function onOpenDataDir() {
    setOpeningDir(true);
    try {
      const result = await api.openWorkbuddyDataDir(region);
      if (result?.capability) {
        toast.message(t("wbStats.gateway.toast.unsupported"), {
          description: result.reason ?? t("wbStats.gateway.toast.unsupportedDesc"),
        });
        return;
      }
      toast.success(t("wbStats.gateway.toast.dirOpened"), { description: result?.path });
    } catch (e) {
      toast.error(t("wbStats.gateway.toast.dirOpenFailed"), { description: api.asError(e) });
    } finally {
      setOpeningDir(false);
    }
  }

  const baseUrl = resolveGatewayBaseUrl(status, config.bind_addr, config.port);
  const running = resolveGatewayRunning(status);

  return (
    <div className="mx-auto w-full max-w-[1180px] px-6 py-8 sm:px-8 sm:py-9">
      {/* 版本入口在**标题下方**（两个模块统一的位置）：整页跟随它。 */}
      <header className="mb-6">
        <h1 className="text-[28px] font-semibold tracking-tight">{t("wbStats.gateway.title")}</h1>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          {t("wbStats.gateway.desc")}
        </p>
        <WorkbuddyRegionSwitch className="mt-4" />
      </header>

      {error && (
        <Alert variant="destructive" className="mb-5">
          <AlertTriangle />
          <AlertTitle>{t("wbStats.gateway.opFail")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-3">
            <span>{error}</span>
            {/* 与 Trae 页同构：错误条自带重试，用户不必去猜「怎么再试一次」。 */}
            <div>
              <Button variant="outline" size="sm" onClick={() => void loadAll()}>
                <RefreshCw />
                {t("wbStats.gateway.retry")}
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}

      {/* 网关 */}
      <Card className="mb-6 gap-0 py-0">
        <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Power className="size-4 text-muted-foreground" />
              {t("wbStats.gateway.enableGateway")}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">{t("wbStats.gateway.enableDesc")}</p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {saving && <Loader2 className="size-3.5 animate-spin text-muted-foreground" />}
            <DemoAction>
              <Switch
                checked={config.enabled}
                disabled={saving}
                onCheckedChange={(enabled) => void persist({ enabled })}
                aria-label={t("wbStats.gateway.enableAria")}
              />
            </DemoAction>
            {/* 与 Trae 页同构：手动重读配置 / 状态 / 池（池是实时状态，不刷新就看不到变化）。 */}
            <DemoAction>
              <Button variant="outline" size="sm" onClick={() => void loadAll()}>
                <RefreshCw />
                {t("wbStats.gateway.refresh")}
              </Button>
            </DemoAction>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-x-6 gap-y-3 px-5 py-4">
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">{t("wbStats.gateway.bindAddr")}</span>
            <Select value={config.bind_addr} onValueChange={onBindAddrChange} disabled={saving}>
              <SelectTrigger size="sm" className="w-52" aria-label={t("wbStats.gateway.bindAddrAria")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={LOOPBACK}>{t("wbStats.gateway.loopback")}</SelectItem>
                <SelectItem value={LAN}>{t("wbStats.gateway.lan")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">{t("wbStats.gateway.port")}</span>
            <DemoAction>
              <Input
                className="h-8 w-28"
                inputMode="numeric"
                value={portDraft}
                onChange={(event) => setPortDraft(event.target.value)}
                onBlur={commitPort}
                onKeyDown={(event) => {
                  if (event.key === "Enter") commitPort();
                }}
                aria-label={t("wbStats.gateway.portAria")}
              />
            </DemoAction>
          </div>
          <span className="flex items-center gap-1.5 text-xs">
            {t("wbStats.gateway.status")}
            <span className={cn("inline-flex items-center gap-1.5 font-medium", running ? "text-emerald-600 dark:text-emerald-400" : "text-muted-foreground")}>
              <span className={cn("size-2 rounded-full", running ? "bg-emerald-500" : "bg-muted-foreground/50")} />
              {running ? t("wbStats.gateway.running") : t("wbStats.gateway.stopped")}
            </span>
          </span>
        </div>

        {/* 最近一次失败的原因（与 Trae 页同构）：否则用户只能去翻请求日志才知道为什么 502。 */}
        {status?.lastError && (
          <div className="border-t border-border/60 px-5 py-3">
            <p className="text-xs text-muted-foreground">{t("wbStats.gateway.lastError")}</p>
            <p className="mt-1 break-words text-xs text-destructive">{status.lastError}</p>
          </div>
        )}
      </Card>

      {/* 接入地址：只显示**当前版本**（版本在页头切换，不在这里并排）。 */}
      <Card className="mb-6 gap-0 py-0">
        <div className="flex flex-wrap items-center gap-3 border-b border-border/60 px-5 py-3">
          <span className="text-sm font-semibold">{t("wbStats.gateway.addr")}</span>
        </div>
        <div className="px-5 py-4">
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-xs text-muted-foreground">Base URL</span>
            <code className="rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-xs">{baseUrl}</code>
            <Button variant="ghost" size="sm" onClick={() => void copyText(baseUrl, t("wbStats.gateway.baseUrlCopied"))}>
              <Copy />
              {t("wbStats.gateway.copy")}
            </Button>
            {/* 打开的是**当前版本**的数据目录：WorkBuddy 的 CN / 国际数据目录是两个，
                页头切到哪版就打开哪版（不再并排两行让用户自己认）。 */}
            <DemoAction>
              <Button
                variant="ghost"
                size="sm"
                disabled={openingDir}
                aria-label={t("wbStats.gateway.openDataDirAria", {
                  version: regionDescriptor(region).versionLabel,
                })}
                onClick={() => void onOpenDataDir()}
              >
                {openingDir ? <Loader2 className="animate-spin" /> : <FolderOpen />}
                {t("wbStats.gateway.openDataDir")}
              </Button>
            </DemoAction>
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
            <span>API Key</span>
            <code className="font-mono">
              {representativeKey(keys, region) ?? t("wbStats.gateway.keyPlaceholder")}
            </code>
          </div>
        </div>
      </Card>

      <ApiKeyTable className="mb-6" />
      {/* 只渲染当前版本的池（版本在页头切换）。Key 表与请求日志**不做版本过滤**：
          它们是「跨版本的管理视图」，逐行带归属即可（与 Trae 侧同口径）。 */}
      <RegionPoolCard region={region} />
      <ModelList region={region} className="mb-6" />
      <IntegrationGuide baseUrl={baseUrl} region={region} className="mb-6" />
      {/* P12 · 请求代理（**默认关**）：放在接入说明之后 —— 它是"进阶能力"，
          且面板里必须**同时**给出风险与不劫持声明（原型断言 2）。 */}
      <ProxyPanel />
      <RequestLog />

      {/* 非回环监听风险确认 */}
      <Dialog open={riskOpen} onOpenChange={setRiskOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("wbStats.gateway.allowLanTitle")}</DialogTitle>
            <DialogDescription>
              {t("wbStats.gateway.allowLanDesc")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRiskOpen(false)}>
              {t("wbStats.gateway.cancel")}
            </Button>
            <Button variant="destructive" onClick={confirmLan}>
              {t("wbStats.gateway.confirmOpen")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* §3.6 状态语言：`loading` 由 `loadAll()` 驱动（进页面时并发拉配置 / 状态 / Keys /
          模型，内容还没到），属**区块首次加载** ⇒ 用 `<Skeleton>` 占位，**不是**文字型
          「加载中」+ `<Loader2>`（后者是**按钮操作中**态，见上方 `saving` / `openingDir`）。 */}
      {loading && (
        <div className="mt-6 space-y-2" aria-busy="true">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-4 w-64" />
          <Skeleton className="h-4 w-52" />
        </div>
      )}
    </div>
  );
}

/**
 * 某版本的账号池卡（与 Trae 侧**同一张** `AccountPoolCard`）。
 *
 * 两个版本各自一个池（`pools.cn` / `pools.global`），故按 region 渲染两张 ——
 * 这是 WorkBuddy 比 Trae 多出来的**版本轴**，卡片本身无差别。
 *
 * ## 「指定账号」怎么落回策略
 *
 * 池卡的 uid 就是**池条目的键**，后端 `pick_with_preference` 正是按它匹配偏好，
 * 因此这里把 uid 直接写进 `pinned`：此前策略卡写的是账号库的 `id`，池里查无此人，
 * 「固定账号」在池非空时**一直是失效的**（静默回落自动择优）。
 */
function RegionPoolCard({ region }: { region: Region }) {
  const t = useT();
  const status = useGatewayStore((s) => s.status);
  const view = useGatewayStore((s) => s.strategies[region]);
  const saveStrategy = useGatewayStore((s) => s.saveStrategy);
  const [saving, setSaving] = useState(false);

  const { pool, accounts, diagnose } = poolOf(status, region);
  const strategy = view.strategy;
  const preferredUid =
    strategy.kind === "pinned"
      ? strategy.account_id
      : strategy.kind === "max_credits"
        ? MAX_CREDITS_OPTION
        : "";

  async function persist(next: AccountStrategy) {
    setSaving(true);
    try {
      await saveStrategy(region, next);
      toast.success(t("wbStats.gateway.strategySaved"));
    } catch (e) {
      toast.error(t("wbStats.gateway.strategySaveFail"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  function onPreferredChange(next: string) {
    if (next === MAX_CREDITS_OPTION) {
      void persist({ kind: "max_credits" });
      return;
    }
    if (next === AUTO_OPTION || next === "") {
      // 取消指定 ⇒ 回到默认（跟随客户端登录态；池非空时由池自动择优）。
      void persist({ kind: "current" });
      return;
    }
    void persist({ kind: "pinned", account_id: next });
  }

  return (
    <AccountPoolCard
      pool={pool}
      accounts={accounts}
      diagnose={diagnose}
      preferredUid={preferredUid}
      onPreferredUidChange={onPreferredChange}
      saving={saving}
      // WorkBuddy 池不存积分有效期 ⇒ 没有「积分过期」格（见卡片注释）。
      tiles={WB_POOL_TILES}
      autoOptions={[
        { value: AUTO_OPTION, label: t("shared.gateway.pool.preferredAuto") },
        { value: MAX_CREDITS_OPTION, label: t("shared.gateway.pool.strategyMaxCredits") },
      ]}
      // 不传 `title`：与 Trae 侧一致用默认的「账号池」——版本由**页头切换器**承担，
      // 卡内再挂一次版本后缀只是噪音（Trae 的池卡同样不标区域）。
      className="mb-6"
    />
  );
}
