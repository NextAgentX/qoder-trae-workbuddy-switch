import { UnifiedGatewayView } from "@/components/gateway/unified-gateway-view";
import { useEffect, useState } from "react";
import { AlertTriangle, Copy, FolderOpen, Loader2, Power, RefreshCw, ShieldAlert } from "lucide-react";
import { useSearchParams } from "react-router-dom";
import { toast } from "sonner";

import { ApiKeyTable } from "@/components/gateway/api-key-table";
import {
  AccountPoolCard,
  AUTO_OPTION,
  MAX_CREDITS_OPTION,
  RANDOM_OPTION,
  WB_POOL_TILES,
} from "@/components/gateway/account-pool-card";
import { IntegrationGuide } from "@/components/gateway/integration-guide";
import { ModelList } from "@/components/gateway/model-list";
import { ModelProbeCard } from "@/components/gateway/model-probe-card";
// P12 · 请求代理面板（**C 路：base URL 重定向**）—— 默认关，应用只记"用户已按引导设置"。
// ⚠️ 只接 WB 侧：C 路本期不做 Trae（`proxy.rs` 的 `PROXY_GATEWAY_BASE` 只覆盖 WB 网关端口）。
import { ProxyPanel } from "@/components/gateway/proxy-panel";
import { RequestLog } from "@/components/gateway/request-log";
import { GatewayClientlessNotice } from "@/components/clientless-notice";
import { GatewayShell } from "@/components/gateway/gateway-shell";
import {
  GW_SECTIONS,
  parseGatewayDeepLink,
  unifiedSectionId,
  type GatewaySectionId,
  type GatewayType,
} from "@/components/gateway/gateway-subnav";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
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
import { scopeOf } from "@/lib/scope";
import { cn } from "@/lib/utils";
import type { AccountStrategy, ApiKeyRecord, GatewayConfig, Region } from "@/lib/types";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { useWorkbuddyRegion } from "@/lib/use-workbuddy-region";
import { useGatewayStore } from "@/stores/gateway";

const LOOPBACK = "127.0.0.1";
const LAN = "0.0.0.0";

/* 「按实时积分择优」/「随机」两个哨兵值从 `account-pool-card` 导入（**不**在本页另写一份）：
 * 它们不是 uid，而是 `AccountStrategy` 的两档，与 Trae / Qoder 侧逐字共用 —— 同一份真值
 * 在两个地方各写一次，漂移的后果是「用户选了随机、后端当成一个不存在的 uid 静默回落
 * 自动择优」，界面上完全看不出来。WorkBuddy 的选号策略有四档
 * （跟随登录态 / 指定账号 / 实时积分择优 / 随机），而池卡只有一个「指定账号」下拉，
 * 故把非账号的三档也放进同一个下拉里，在 `onPreferredChange` 的边界处换成策略。 */

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
 * 现在与 Trae 侧同一套心智模型：**区域来自作用域（L1）→ 整页跟随 → 子组件只渲染不选择**。
 * 区域由 URL 的 scope 段承载（`/s/wb-global/gateway`，可刷新、可分享、配合前进后退），
 * 见 `useWorkbuddyRegion`（它是 `useScope("workbuddy")` 的薄封装）。
 *
 * ⚠️ **页内不再有区域切换器**：切作用域的唯一入口是顶栏 `ScopeSelector`，
 * 这里只放一枚只读 `ScopeChip`（与原型 `viewGateway` 一致）。
 */
export default function ApiServicePage() {
  const t = useT();
  // 只取当前值：区域来自作用域（URL 优先），页内不再有切换动作。
  const [region] = useWorkbuddyRegion();
  const scope = scopeOf("workbuddy", region);
  const config = useGatewayStore((s) => s.config);
  const status = useGatewayStore((s) => s.status);
  const models = useGatewayStore((s) => s.models);
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
  /** 「上游账号凭据 · 校验」进行中（只驱动按钮转圈）。 */
  const [verifying, setVerifying] = useState(false);
  /* 错误条「重试」与概览「刷新」两处出口各自的进行中态（两处可能同屏）。 */
  const { busy: retryBusy, run: runRetry } = useRefreshAction();
  const { busy: refreshBusy, run: runRefresh } = useRefreshAction();
  /* ★ 网关页的两个轴（`?type=` + `?sub=`）—— 深链解析兼容两代旧链（见 `parseGatewayDeepLink`）。
     与设置页（`?section=`）同一口径：深链能直达，刷新 / 分享 / 前进后退都成立。 */
  const [searchParams, setSearchParams] = useSearchParams();
  const [{ type, section }, setNav] = useState(() => parseGatewayDeepLink(searchParams));

  /** 写回 URL（保留查询串里的其他参数，而不是整体覆盖）。 */
  function writeNav(next: GatewayType, sub: GatewaySectionId) {
    setNav({ type: next, section: sub });
    const params = new URLSearchParams(searchParams);
    params.set("type", next);
    params.set("sub", sub);
    setSearchParams(params, { replace: true });
  }

  /* ★ 切类型**必须重置分区**到该类型的第一项：两个类型的分区 id 集合不相交，
     停在另一个类型的分区上会渲染成空（原型 `GW_SECTIONS[a1][0][0]` 同款做法）。 */
  function goType(next: GatewayType) {
    writeNav(next, GW_SECTIONS[next][0].id);
  }

  /** 统一网关分区内的跳转（裸分区名 → 带 `u-` 前缀的分区 id；约定只在 `unifiedSectionId` 里写一次）。 */
  function goUnified(next: string) {
    goSub(unifiedSectionId(next));
  }

  function goSub(id: GatewaySectionId) {
    writeNav(type, id);
  }

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    setPortDraft(String(config.port));
  }, [config.port]);

  async function persist(next: Partial<GatewayConfig>) {
    const merged = { ...config, ...next };
    setSaving(true);
    try {
      await saveConfig(merged);
      /* ★ 与 Trae 页同构：保存成功也给反馈。此前 WB 侧**静默** ——
         用户改完端口 / 监听地址后没有任何确认，只能靠「状态」那一行自己猜。 */
      toast.success(
        merged.enabled
          ? t("wbStats.gateway.savedStarted")
          : t("wbStats.gateway.savedDisabled"),
      );
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

  /* 概览三行的**真实取数**（原型 :3013-3018）：模型与账号池条目数均来自后端下发，缺数据渲染 0。 */
  const { pool: poolSummary, accounts: poolAccounts } = poolOf(status, region);
  const modelCount = models[region]?.models.length ?? 0;
  const poolCount = poolSummary?.total ?? poolAccounts.length;

  /**
   * 「上游账号凭据 · 校验」——复用既有的 `checkAuthPermission` 探针（写权限自检）。
   *
   * 它是本应用**唯一**的凭据可用性探针：结果如实透出（ok 用后端 message，失败用 error/hint），
   * 不写死「正常」。
   */
  async function onVerifyCreds() {
    setVerifying(true);
    try {
      const res = await api.checkAuthPermission();
      if (res.ok) {
        toast.success(t("wbStats.gateway.verifyOk"), { description: res.message });
      } else {
        toast.error(t("wbStats.gateway.verifyFail"), {
          description: res.error ?? res.hint,
        });
      }
    } catch (e) {
      toast.error(t("wbStats.gateway.verifyFail"), { description: api.asError(e) });
    } finally {
      setVerifying(false);
    }
  }

  /* 页头抽成变量：加载态与正常态**共用同一份** —— 骨架屏出现时页头不闪掉。
     （原型 `viewGateway`：作用域副标题 + 只读 scope-chip；区域来自作用域，整页跟随。） */
  const header = (
    <header className="mb-5 flex items-start gap-4">
      <div className="min-w-0">
        <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("wbStats.gateway.title")}</h1>
        <p className="mt-1.5 text-[12.5px] text-muted-foreground">
          {t("pageHead.gateway", { scope: scopeLabel(t, scope) })}
        </p>
      </div>
      <div className="grow" />
      <ScopeChip scope={scope} />
    </header>
  );

  /* ★ 首次加载（`loading && !status`）：页头常驻 + 主区骨架，与 Trae 页**同一条件、同一骨架**。
     ⚠️ 判据不是裸 `loading` —— 手动刷新时保留旧内容，不把用户正看着的配置换成骨架。
     区块首次加载用 `<Skeleton>`（§3.6）；文字型「加载中」+ `<Loader2>` 是**按钮操作中**态。 */
  if (loading && !status) {
    return (
      <div className="mx-auto w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
        {header}
        <div className="space-y-4" aria-busy="true">
          <Skeleton className="h-[34px] w-[186px]" />
          <Skeleton className="h-32 w-full" />
          <Skeleton className="h-64 w-full" />
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
      {header}

      {error && (
        <Alert variant="destructive" className="mb-5">
          <AlertTriangle />
          <AlertTitle>{t("wbStats.gateway.opFail")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-3">
            <span>{error}</span>
            {/* 与 Trae 页同构：错误条自带重试，用户不必去猜「怎么再试一次」。 */}
            <div>
              <Button
                variant="outline"
                size="sm"
                disabled={retryBusy}
                onClick={() => void runRetry(() => loadAll())}
              >
                {retryBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                {t("wbStats.gateway.retry")}
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}

      {/* ---- 主区：子导航 + 分区内容（PLAN §3.9：替代原来的 6 卡纵向堆叠）----
          方向 / 对齐 / 间距整组在 `index.css` 的 `[data-split-layout]`（原型 `.split`，
          含 900px 转纵向），故这里不再写 `flex-col` / `lg:flex-row` / `gap-*`。 */}
      {/* 无客户端形态（issue #62）：网关只依赖账号池里的凭据，与客户端是否同机无关。 */}
      <GatewayClientlessNotice scope={scope} />
      <GatewayShell
        type={type}
        section={section}
        onTypeChange={goType}
        onSelect={goSub}
      >
          {/* ---- 概览：服务状态 + 接入地址 + 接入指引 ---- */}
          {section === "overview" && (
            <>
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
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={refreshBusy}
                        onClick={() => void runRefresh(() => loadAll())}
                      >
                        {refreshBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
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
                        /* 与 Trae 页同一做法：输入期就滤掉非数字（而不是等提交时报错重置）。 */
                        onChange={(event) => setPortDraft(event.target.value.replace(/[^\d]/g, ""))}
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
                    <span className={cn("inline-flex items-center gap-1.5 font-medium", running ? "text-success" : "text-muted-foreground")}>
                      <span className={cn("size-2 rounded-full", running ? "bg-success" : "bg-muted-foreground/50")} />
                      {running ? t("wbStats.gateway.running") : t("wbStats.gateway.stopped")}
                    </span>
                  </span>
                </div>

                {/* ★ 与 Trae 页同构的说明行（2026-10-13 对齐）：讲清「默认端口为什么是 57891」
                    （两个网关都占用 `/v1/chat/completions`，必须错开），并在局域网模式下给警示。
                    此前这条只出现在 Trae 侧，WB 用户看不到端口选择的理由。 */}
                <div className="flex flex-wrap items-center gap-2 border-t border-border/60 px-5 py-3 text-xs text-muted-foreground">
                  {t("wbStats.gateway.noteLead")}
                  <code className="font-mono">/v1/chat/completions</code>
                  {t("wbStats.gateway.noteTail")}
                  {config.bind_addr === LAN && t("wbStats.gateway.lanWarning")}
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
                {/* 卡头与页内其他卡（网关 / 密钥 / 模型 / 账号池 / 日志）**同一组值**：
                    `px-5 py-3` + `text-sm font-semibold`。此前这里是 `px-4 py-[13px]` +
                    `text-[13.5px]`，只有它一个不一样（Trae 侧一开始就是标准值）。 */}
                <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
                  <span className="text-sm font-semibold">{t("wbStats.gateway.addr")}</span>
                </div>
                <div className="px-5 py-4">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="text-xs text-muted-foreground">{t("wbStats.gateway.baseUrlLabel")}</span>
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
                    <span>{t("wbStats.gateway.keyPrefixLabel")}</span>
                    <code className="font-mono">
                      {representativeKey(keys, region) ?? t("wbStats.gateway.keyPlaceholder")}
                    </code>
                  </div>
                </div>

                {/* ★ 概览三行（原型 `viewGateway` :3013-3018）：上游凭据校验 + 可用模型 / 账号池（带「查看」深链）。
                    数字全部来自后端下发，「查看」走 `?sub=` 深链（可刷新、可分享）。 */}
                <div className="divide-y divide-border/60 border-t border-border/60">
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("wbStats.gateway.upstreamCreds")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("wbStats.gateway.upstreamCredsDesc")}
                      </p>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={verifying}
                      onClick={() => void onVerifyCreds()}
                    >
                      {verifying ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                      {t("wbStats.gateway.verify")}
                    </Button>
                  </div>
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("wbStats.gateway.availableModels")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("wbStats.gateway.modelCount", { count: modelCount })}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => goSub("models")}>
                      {t("wbStats.gateway.view")}
                    </Button>
                  </div>
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("wbStats.gateway.poolEntries")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("wbStats.gateway.poolEntryCount", { count: poolCount })}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => goSub("pool")}>
                      {t("wbStats.gateway.view")}
                    </Button>
                  </div>
                </div>
              </Card>

              {/* 接入指引：把「怎么用这个网关」放在概览里，一眼可见。 */}
              <IntegrationGuide baseUrl={baseUrl} region={region} />
            </>
          )}

          {/* ---- 密钥 ---- */}
          {section === "keys" && <ApiKeyTable />}

          {/* ---- 模型 ---- */}
          {/* 限流探测卡**在表之上**（方案 §5.1：卡片在模型分区里，不是弹层）。 */}
          {section === "models" && (
            <>
              <ModelProbeCard region={region} className="mb-6" />
              <ModelList region={region} />
            </>
          )}

          {/* ---- 账号池 ---- */}
          {/* 只渲染当前版本的池（版本在页头切换）。 */}
          {section === "pool" && <RegionPoolCard region={region} />}

          {/* ---- 请求代理（P12 · 默认关）：面板里必须**同时**给出风险与不劫持声明（原型断言 2）。 ---- */}
          {section === "proxy" && <ProxyPanel />}

          {/* ---- 请求日志 ---- */}
          {/* Key 表与请求日志**不做版本过滤**：它们是「跨版本的管理视图」，逐行带归属即可。 */}
          {section === "logs" && <RequestLog />}
          {/* ---- 统一网关（第二个**类型**，跨全部作用域）----
              四个分区各自渲染一段：`UnifiedGatewayView` 只认 `section`，不自己持有导航。 */}
          {section === "u-overview" && <UnifiedGatewayView section="overview" onGoSection={goUnified} />}
          {section === "u-namespaces" && <UnifiedGatewayView section="namespaces" onGoSection={goUnified} />}
          {section === "u-keys" && <UnifiedGatewayView section="keys" onGoSection={goUnified} />}
          {section === "u-pool" && <UnifiedGatewayView section="pool" onGoSection={goUnified} />}
      </GatewayShell>

      {/* 非回环监听风险确认 */}
      <Dialog open={riskOpen} onOpenChange={setRiskOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            {/* ★ 与 Trae 页同构：危险确认的标题带警示图标（`ShieldAlert` / warning 色），
                确认按钮用 `destructive` —— 两平台的「开放局域网」是同一个危险操作。 */}
            <DialogTitle className="flex items-center gap-2">
              <ShieldAlert className="size-4 text-warning" />
              {t("wbStats.gateway.allowLanTitle")}
            </DialogTitle>
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
  const refreshStatus = useGatewayStore((s) => s.refreshStatus);
  const [saving, setSaving] = useState(false);
  const [revivingUid, setRevivingUid] = useState<string | null>(null);
  const [excludingUid, setExcludingUid] = useState<string | null>(null);

  /* ★ 探测收尾后主动刷一次池状态（issue #88）。
   *
   * 本卡只订阅 `status`，而 `refreshStatus()` 的调用点只有「进页面」与「排除/恢复」
   * 两个动作回调 —— **没有任何定时刷新，也没有在探测收尾后刷新**。于是回填明明把
   * 冷却写进了后端，池视图仍停在进页面那一刻的快照，用户看到「探测判出限流、
   * 账号池却全绿」，只能怀疑探测没生效。
   *
   * 刻意**不做定时轮询**：那会平白增加 IPC，而探测收尾是唯一需要跟进的时刻。
   * `finished_at_ms` 每轮都不同 ⇒ 连续两轮探测也能各刷一次（用 `phase` 判断会漏掉
   * 「done → done」这种第二次收尾）。 */
  const probeFinishedAt = useGatewayStore((s) => s.probe?.finished_at_ms ?? 0);
  useEffect(() => {
    if (!probeFinishedAt) return;
    void refreshStatus();
  }, [probeFinishedAt, refreshStatus]);

  const { pool, accounts, diagnose } = poolOf(status, region);
  const strategy = view.strategy;
  const preferredUid =
    strategy.kind === "pinned"
      ? strategy.account_id
      : strategy.kind === "max_credits"
        ? MAX_CREDITS_OPTION
        : strategy.kind === "random"
          ? RANDOM_OPTION
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
    /* 随机：合格候选内**均匀**抽签（不按权重）。与 Trae / Qoder 侧同一档 ——
       后端 `AccountStrategy::Random` 是三平台共用枚举，这里少一档就是该平台静默失去它。 */
    if (next === RANDOM_OPTION) {
      void persist({ kind: "random" });
      return;
    }
    if (next === AUTO_OPTION || next === "") {
      // 取消指定 ⇒ 回到默认（跟随客户端登录态；池非空时由池自动择优）。
      void persist({ kind: "current" });
      return;
    }
    void persist({ kind: "pinned", account_id: next });
  }

  /**
   * 解除某账号的禁用。
   *
   * 这是**唯一**出口：`disabled` 是终态且落盘持久化，余额刷新救不了它
   * （`thaw_hard_credit_if_recovered` 第一句就是 `if entry.disabled { return false }`）。
   * 不做「重新登录就自动复活」—— 账号库里没有任何字段能区分「用户重新登录」与
   * 「后台保活刷新 token」（`id` / `createdAt` 被刻意保留、只有 token 会变），
   * 硬做等于每次保活都把三振账号洗白（详见 `Pool::revive` 的文档）。
   */
  /**
   * 排除 / 取消排除。
   *
   * 与「恢复」的区别（文案必须讲清）：禁用是**故障**（要修），排除是**用户选择**（只要取消）。
   * 后端会同时做三件事：落盘、刷新池内快照、按 uid 解绑粘性 —— 前端只负责回读状态。
   */
  async function onExclude(uid: string, excluded: boolean) {
    setExcludingUid(uid);
    try {
      await api.setGatewayPoolExclusion(region, uid, excluded);
      toast.success(
        excluded
          ? t("shared.gateway.pool.excludeDone")
          : t("shared.gateway.pool.unexcludeDone")
      );
      // 计数格与行状态都来自 `status` ⇒ 重新拉一次，否则界面还显示旧状态。
      await refreshStatus();
    } catch (e) {
      toast.error(t("shared.gateway.pool.excludeFail"), { description: api.asError(e) });
    } finally {
      setExcludingUid(null);
    }
  }

  async function onRevive(uid: string) {
    setRevivingUid(uid);
    try {
      const result = await api.reviveGatewayAccount(uid);
      if (result.revived) {
        toast.success(t("shared.gateway.pool.reviveDone"));
      } else {
        // 幂等：本来可用。不是错误，但也别谎称「已恢复」。
        toast.info(t("shared.gateway.pool.reviveNoop"));
      }
      // 计数格与行状态都来自 `status` ⇒ 重新拉一次，否则界面还显示「已禁用」。
      await refreshStatus();
    } catch (e) {
      toast.error(t("shared.gateway.pool.reviveFail"), { description: api.asError(e) });
    } finally {
      setRevivingUid(null);
    }
  }

  return (
    <AccountPoolCard
      pool={pool}
      accounts={accounts}
      diagnose={diagnose}
      preferredUid={preferredUid}
      onPreferredUidChange={onPreferredChange}
      saving={saving}
      onRevive={onRevive}
      revivingUid={revivingUid}
      onExclude={onExclude}
      excludingUid={excludingUid}
      // WorkBuddy 池不存积分有效期 ⇒ 没有「积分过期」格（见卡片注释）。
      tiles={WB_POOL_TILES}
      autoOptions={[
        { value: AUTO_OPTION, label: t("shared.gateway.pool.preferredAuto") },
        { value: MAX_CREDITS_OPTION, label: t("shared.gateway.pool.strategyMaxCredits") },
        { value: RANDOM_OPTION, label: t("shared.gateway.pool.strategyRandom") },
      ]}
      // 不传 `title`：与 Trae 侧一致用默认的「账号池」——版本由**页头切换器**承担，
      // 卡内再挂一次版本后缀只是噪音（Trae 的池卡同样不标区域）。
      className="mb-6"
    />
  );
}
