import { UnifiedGatewayView } from "@/components/gateway/unified-gateway-view";
import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Copy, Loader2, Power, RefreshCw, ShieldAlert } from "lucide-react";
import { useSearchParams } from "react-router-dom";
import { toast } from "sonner";

import { DemoAction } from "@/components/demo-action";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import {
  AUTO_OPTION,
  MAX_CREDITS_OPTION,
  RANDOM_OPTION,
  SEQUENTIAL_OPTION,
  AccountPoolCard,
  QODER_POOL_TILES,
} from "@/components/gateway/account-pool-card";
import { GatewayClientlessNotice } from "@/components/clientless-notice";
import { GatewayShell } from "@/components/gateway/gateway-shell";
import {
  GW_INDEPENDENT_SECTIONS_NO_PROXY,
  GW_SECTIONS,
  parseGatewayDeepLink,
  unifiedSectionId,
  type GatewaySectionId,
  type GatewayType,
} from "@/components/gateway/gateway-subnav";
import { QoderApiKeyTable } from "@/components/gateway/qoder-api-key-table";
import { QoderModelList } from "@/components/gateway/qoder-model-list";
import { QoderQuotaGuard } from "@/components/gateway/qoder-quota-guard";
import { ModelProbeCard } from "@/components/gateway/model-probe-card";
import { TraeIntegrationGuide } from "@/components/gateway/trae-integration-guide";
import { TraeRequestLog } from "@/components/gateway/trae-request-log";
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
import {
  DEFAULT_QODER_GATEWAY_CONFIG,
  normalizeQoderGatewayCatalog,
  normalizeQoderGatewayConfig,
  normalizeQoderGatewayLogs,
  normalizeQoderGatewayStatus,
  toQoderGatewayConfigRaw,
  type QoderGatewayCatalog,
  type QoderGatewayConfig,
  type QoderGatewayLogEntry,
  type QoderGatewayStatus,
} from "@/lib/qoder-gateway";
import { scopeOf } from "@/lib/scope";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useScope } from "@/lib/use-scope";
import { cn } from "@/lib/utils";

const LOOPBACK = "127.0.0.1";
const LAN = "0.0.0.0";

/**
 * Qoder 网关的子导航表：**去掉「请求代理」**。
 *
 * P12 的「C 路 base URL 重定向」只覆盖 **WorkBuddy 网关端口**（`proxy.rs` 的
 * `PROXY_GATEWAY_BASE`），Qoder 侧没有对应后端 ⇒ 不渲染该项 ——
 * 一个点进去什么都没有的项，比不渲染更糟（§3.4 规则 1：缺能力不画空壳）。
 */
const QODER_GATEWAY_SUBNAV = GW_INDEPENDENT_SECTIONS_NO_PROXY;

/**
 * 本页快照：配置 / 状态 / 模型清单 / 请求日志。
 *
 * 一起取、一起被保存与清空操作改写 —— 拆开缓存只会让「配置已保存、状态还是旧的」
 * 这种中间态有缝可钻。`status` 允许为 `null`（尚未取到），`config` 用默认值兜底。
 */
interface ApiServiceSnapshot {
  config: QoderGatewayConfig;
  status: QoderGatewayStatus | null;
  catalog: QoderGatewayCatalog;
  logs: QoderGatewayLogEntry[];
  /**
   * 限额护栏视图（逐账号当日用量 + 拦截原因）。
   *
   * 与其余四项**一起取**：护栏阈值存在配置里、用量存在网关里，分开取会出现
   * 「阈值已改、读数还是旧的」这种自相矛盾的中间态（与本快照存在的理由同一条）。
   */
  quota: api.QoderGatewayQuotaView | null;
}

/**
 * 「API 服务」页（Qoder 分区）。
 *
 * ## 与另两个分区的关系：**结构同构、数据分家**
 *
 * 页头 / 子导航 / 卡片节奏与 WB、Trae 两个分区**刻意一致**（原型
 * `viewGateway` 只有一套结构，按 `state.scope` 换数据）。真正不同、且**必须**不同的只有：
 *
 * 1. **一个监听服务两个区域**：`enabled` / `port` / `running` 是两个区域**共用**的事实
 *    （同一个监听），而池 / Key 前缀 / 清单按 `scope` 分家 —— 区域由 URL 作用域承载，
 *    页内不再有区域切换器（与两个分区同一条纪律）。
 * 2. **Key 的归属是区域**（不是程序位、不是版本），三态动作与另两条产品线同形。
 * 3. **清单只能向上游拉**（没有本机缓存）⇒ 来源三级是「实时 / 上次成功 / 暂无清单」。
 *
 * ⚠️ 本页**不渲染**「打开数据目录」：Qoder 没有对应能力（通用命令对 Qoder 作用域
 * 一律可诊断拒绝，见 `commands.rs::open_data_dir`）——缺能力不画空壳。
 */
export default function QoderApiServicePage() {
  const t = useT();
  /** 当前区域（URL 作用域优先，见 `useScope`）。 */
  const [region] = useScope("qoder");
  /** 本页所在**作用域**：状态 / 清单 / 禁用偏好都按它分家。 */
  const scope = scopeOf("qoder", region);
  const [excludingUid, setExcludingUid] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);
  /** 模型清单「重新识别」的进行中状态（只驱动按钮转圈，不参与快照）。 */
  const [refreshingModels, setRefreshingModels] = useState(false);
  /** 正在「恢复」（刷新令牌）的 uid。 */
  const [revivingUid, setRevivingUid] = useState<string | null>(null);
  /* ★ 子导航选中项：**初值从 URL 读**（`?sub=logs`），非法值 fail-closed 回 `overview`
     （与设置页 `?section=`、另两个网关页同一口径），点击时写回 URL（深链可刷新、可分享）。 */
  const [searchParams, setSearchParams] = useSearchParams();
  /* ★ 两个轴（`?type=` + `?sub=`）；深链解析兼容两代旧链，见 `parseGatewayDeepLink`。 */
  const [{ type, section }, setNav] = useState(() => parseGatewayDeepLink(searchParams));

  /** 写回 URL（保留查询串里的其他参数，而不是整体覆盖）。 */
  function writeNav(next: GatewayType, sub: GatewaySectionId) {
    setNav({ type: next, section: sub });
    const params = new URLSearchParams(searchParams);
    params.set("type", next);
    params.set("sub", sub);
    setSearchParams(params, { replace: true });
  }

  /* ★ 切类型**必须重置分区**到该类型的第一项（两个类型的分区 id 集合不相交）。 */
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

  const loadSnapshot = useCallback(async (): Promise<ApiServiceSnapshot> => {
    const [configRaw, statusRaw, catalogRaw, logsRaw, quota] = await Promise.all([
      api.getQoderGatewayConfig(),
      api.qoderGatewayStatus(scope),
      api.getQoderGatewayModels(scope),
      api.getQoderGatewayLogs(),
      // 护栏视图失败**不能拖垮整页**（配置 / 池 / 清单仍要能看）：失败即 `null`，
      // 卡片会显示「还没读到」，而不是把整页打成错误态。
      api.qoderGatewayQuota(scope).catch(() => null),
    ]);
    return {
      config: normalizeQoderGatewayConfig(configRaw),
      status: normalizeQoderGatewayStatus(statusRaw),
      catalog: normalizeQoderGatewayCatalog(catalogRaw),
      logs: normalizeQoderGatewayLogs(logsRaw),
      quota,
    };
  }, [scope]);

  /**
   * 快照缓存：切到别的分区再切回来时不闪骨架。变更操作（保存配置、清空日志）
   * 走 `patch` 就地改写快照，而不是写组件 state —— 值归缓存所有，两处写迟早分歧。
   */
  const {
    data: snapshot,
    loading,
    error,
    refresh: loadAll,
    patch,
  } = useCachedResource<ApiServiceSnapshot>(`qoder:api-service:${scope}`, loadSnapshot);

  const config = snapshot?.config ?? DEFAULT_QODER_GATEWAY_CONFIG;
  const status = snapshot?.status ?? null;
  const catalog = snapshot?.catalog ?? {
    region: "",
    source: "unavailable" as const,
    fetchedAt: null,
    note: null,
    rows: [],
  };
  const logs = snapshot?.logs ?? [];

  /**
   * 限流探测卡（与 WB 网关页**同一个组件**）要的**账号轴 / 模型轴**。
   *
   * ⚠️ Qoder 的池 / 清单**不在** WB 网关 store 里，故这里**显式注入**自己的轴 ——
   * 「列集合 / 账号轴由作用域决定」由本页保证，组件本身**不按平台分叉**。
   */
  const probeAccounts = useMemo(
    () => (status?.accounts ?? []).map((account) => ({ uid: account.uid, name: account.name })),
    [status],
  );
  const probeModels = useMemo(
    () => catalog.rows.map((row) => ({ id: row.id, name: row.name })),
    [catalog],
  );

  /** 就地改写快照（保留其余字段）。 */
  const patchSnapshot = useCallback(
    (next: Partial<ApiServiceSnapshot>) => {
      patch((current) => ({ ...current, ...next }));
    },
    [patch],
  );

  /**
   * 端口输入框的草稿值。
   *
   * 与 `config.port` 同步的时机是「`config.port` 本身变了」（加载完成、保存成功都会变），
   * 用户打字期间 `config.port` 不动 ⇒ 输入不会被打断；初值直接取当前配置，
   * 带着缓存重挂载时不会先显示默认端口再跳一下。
   */
  const [portDraft, setPortDraft] = useState(() => String(config.port));
  useEffect(() => {
    setPortDraft(String(config.port));
  }, [config.port]);

  async function persist(next: Partial<QoderGatewayConfig>) {
    const merged = { ...config, ...next };
    // 乐观更新（与 Trae 同口径）：开关立刻跟手，失败回滚。
    const previous = config;
    patchSnapshot({ config: merged });
    setSaving(true);
    try {
      await api.saveQoderGatewayConfig(toQoderGatewayConfigRaw(merged));
      // 保存会启动 / 重启监听，状态要重读（仍按当前作用域）。
      const statusRaw = await api.qoderGatewayStatus(scope);
      patchSnapshot({ status: normalizeQoderGatewayStatus(statusRaw) });
      // 清单数字按开关选档 ⇒ 只有改开关才重取（不强制打上游，见 `refetchCatalog`）。
      if (next.maxContext !== undefined) void refetchCatalog();
      toast.success(
        merged.enabled
          ? t("qoder.gateway.savedStarted")
          : t("qoder.gateway.savedDisabled"),
      );
    } catch (e) {
      patchSnapshot({ config: previous });
      toast.error(t("qoder.gateway.saveFailed"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  function onBindAddrChange(next: string) {
    if (next === config.bindAddr) return;
    if (next === LOOPBACK) {
      void persist({ bindAddr: LOOPBACK, allowNonLoopback: false });
      return;
    }
    setRiskOpen(true);
  }

  function confirmLan() {
    setRiskOpen(false);
    void persist({ bindAddr: LAN, allowNonLoopback: true });
  }

  function commitPort() {
    const parsed = Number.parseInt(portDraft, 10);
    if (!Number.isFinite(parsed) || parsed < 1 || parsed > 65535) {
      toast.error(t("qoder.gateway.portInvalid"), {
        description: t("qoder.gateway.portRange"),
      });
      setPortDraft(String(config.port));
      return;
    }
    if (parsed === config.port) return;
    void persist({ port: parsed });
  }

  /** 正在重取护栏视图（只驱动按钮转圈）。 */
  const [refreshingQuota, setRefreshingQuota] = useState(false);

  /** 重取护栏视图（逐账号用量会随请求变化，值得一个显式刷新出口）。 */
  async function onRefreshQuota() {
    setRefreshingQuota(true);
    try {
      const quota = await api.qoderGatewayQuota(scope);
      patchSnapshot({ quota });
    } catch (e) {
      toast.error(t("qoder.gateway.quota.refreshFailed"), { description: api.asError(e) });
    } finally {
      setRefreshingQuota(false);
    }
  }

  /**
   * 保存三条阈值。
   *
   * 走同一条 `persist`（保存 → 重启监听 → 重读状态），成功后**重取护栏视图** ——
   * 阈值变了但表里的「已拦」还是按旧阈值算的，会让人以为保存没生效。
   */
  async function onSaveQuota(next: {
    reserveCredits: number;
    dailyTokenLimit: number;
    dailyCreditLimit: number;
    modelDailyTokenLimit: number;
  }) {
    await persist(next);
    void onRefreshQuota();
  }

  async function onClearLogs() {
    setClearing(true);
    try {
      await api.clearQoderGatewayLogs();
      patchSnapshot({ logs: [] });
      toast.success(t("qoder.gateway.logsCleared"));
    } catch (e) {
      toast.error(t("qoder.gateway.logsClearFailed"), { description: api.asError(e) });
    } finally {
      setClearing(false);
    }
  }

  /**
   * 断言式重取清单（**不强制打上游**）。
   *
   * 开关改的是「服务端按哪一档序列化」，不是数据本身 ⇒ 重新 GET 一次就够；
   * 用 `force=true` 反而会在离线时把清单打空，用户看到「模型全没了」。
   */
  async function refetchCatalog() {
    try {
      const catalogRaw = await api.getQoderGatewayModels(scope, false);
      patchSnapshot({ catalog: normalizeQoderGatewayCatalog(catalogRaw) });
    } catch {
      /* 读缓存失败：保留现有快照，不打断开关操作。 */
    }
  }

  /**
   * 「重新识别」模型清单 —— **强制重拉**（绕过后端 10 分钟缓存）。
   *
   * ⚠️ 必须带 `refresh = true`：走普通取数会命中缓存 ⇒ 按钮点了而清单不变，那就成了
   * **假控件**（本仓明令禁止：一个永不改变结果的按钮不该存在）。清单只能向官方拉，
   * 所以这里就是「真的再去问一次上游」。
   *
   * 只就地改写快照里的 `catalog`（配置 / 状态 / 日志不受影响），值仍归缓存所有 ——
   * 与「保存配置」同一条纪律：变更后不自己另存一份 state。
   */
  async function onRefreshModels() {
    setRefreshingModels(true);
    try {
      const catalogRaw = await api.getQoderGatewayModels(scope, true);
      patchSnapshot({ catalog: normalizeQoderGatewayCatalog(catalogRaw) });
    } catch (e) {
      toast.error(t("qoder.gateway.models.refreshFailed"), { description: api.asError(e) });
    } finally {
      setRefreshingModels(false);
    }
  }

  /**
   * 解除某账号的「需干预」状态 —— Qoder 侧的出口是**刷新令牌**。
   *
   * 与 Trae 侧（清冷却记录）同理但动作不同：Qoder 池的 `disabled` 由
   * 无凭据 / 凭据失效（401/403）派生，唯一的复原路径是刷新令牌
   * （`qoder_refresh_account`；PAT 导入的账号没有刷新路由，会在 toast 里显示原因）。
   * 刷新成功后池的下一次 `sync_for` 会带上新令牌 ⇒ `auth_dead` 自动清除。
   */
  /**
   * 排除 / 取消排除（Qoder 侧）。
   *
   * 与「恢复」的区别：恢复是修**故障**（清冷却记录），排除是记**用户意图**。
   * 后端落盘后会立刻被下一次选号读到（Qoder 池每次 `sync_for` 重建），
   * 因此只需重新拉一次状态。
   */
  async function onExclude(uid: string, excluded: boolean) {
    setExcludingUid(uid);
    try {
      await api.setQoderGatewayPoolExclusion(scope, uid, excluded);
      toast.success(
        excluded
          ? t("shared.gateway.pool.excludeDone")
          : t("shared.gateway.pool.unexcludeDone")
      );
      await loadAll();
    } catch (e) {
      toast.error(t("shared.gateway.pool.excludeFail"), { description: api.asError(e) });
    } finally {
      setExcludingUid(null);
    }
  }

  async function onRevive(uid: string) {
    setRevivingUid(uid);
    try {
      await api.qoderRefreshAccount(scope, uid);
      toast.success(t("shared.gateway.pool.reviveDone"));
      // 池状态来自 `status` ⇒ 重新拉一次，否则界面还显示「需刷新」。
      await loadAll();
    } catch (e) {
      toast.error(t("shared.gateway.pool.reviveFail"), { description: api.asError(e) });
    } finally {
      setRevivingUid(null);
    }
  }

  const running = status?.running ?? false;
  const baseUrl = useMemo(() => {
    const host = config.bindAddr === LAN ? LOOPBACK : config.bindAddr;
    return `http://${host}:${config.port}/v1`;
  }, [config.bindAddr, config.port]);

  const keyPrefix = status?.apiKeyPrefix || "";
  /* ★ 概览两行的取数（与另两个分区**同源口径**）：
     - 「可用模型」= **网关对外清单**的条数（= 全量 − 已禁用；与出口 ① 同源）。
       ⚠️ 不能直接用 `catalog.rows.length`（那是含已禁用的全量）—— 那会让「禁用」在概览里看不见。
     - 「账号池」= 池条目总数（`pool.total`，缺数据时回落到账号数）。 */
  const disabledCount = catalog.rows.filter((row) => row.disabled).length;
  const modelCount = Math.max(0, catalog.rows.length - disabledCount);
  const poolCount = status?.pool?.total ?? status?.accounts?.length ?? 0;

  const header = (
    <header className="mb-5 flex items-start gap-4">
      <div className="min-w-0">
        <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("qoder.gateway.title")}</h1>
        <p className="mt-1.5 text-[12.5px] text-muted-foreground">
          {t("pageHead.gateway", { scope: scopeLabel(t, scope) })}
        </p>
      </div>
      <div className="grow" />
      <ScopeChip scope={scope} />
    </header>
  );

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
          <AlertTitle>{t("qoder.gateway.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-3">
            <span>{error}</span>
            <div>
              <Button variant="outline" size="sm" onClick={() => void loadAll()}>
                <RefreshCw />
                {t("qoder.gateway.retry")}
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}

      {/* ---- 主区：子导航 + 分区内容（与另两个分区同一套骨架）---- */}
      {/* 无客户端形态（issue #62）：网关只依赖账号池里的凭据，与客户端是否同机无关。 */}
      <GatewayClientlessNotice scope={scope} />
      <GatewayShell
        type={type}
        section={section}
        sections={type === "independent" ? QODER_GATEWAY_SUBNAV : undefined}
        onTypeChange={goType}
        onSelect={goSub}
      >
          {/* ---- 概览：服务状态 + 接入地址 + 接入指引 ---- */}
          {section === "overview" && (
            <>
              <Card className="mb-6 gap-0 py-0">
                <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-4">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2 text-sm font-medium">
                      <Power className="size-4 text-muted-foreground" />
                      {t("qoder.gateway.enable")}
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {t("qoder.gateway.enableDesc", {
                        upstream: status?.upstream || "api1.qoder.com.cn",
                      })}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {saving && <Loader2 className="size-3.5 animate-spin text-muted-foreground" />}
                    <DemoAction>
                      <Switch
                        checked={config.enabled}
                        disabled={saving}
                        onCheckedChange={(checked) => void persist({ enabled: checked })}
                        aria-label={t("qoder.gateway.enable")}
                      />
                    </DemoAction>
                    <DemoAction>
                      <Button variant="outline" size="sm" onClick={() => void loadAll()}>
                        <RefreshCw />
                        {t("qoder.gateway.refresh")}
                      </Button>
                    </DemoAction>
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-x-6 gap-y-3 px-5 py-4">
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">{t("qoder.gateway.bindAddr")}</span>
                    <Select value={config.bindAddr} onValueChange={onBindAddrChange} disabled={saving}>
                      <SelectTrigger size="sm" className="w-52" aria-label={t("qoder.gateway.bindAddr")}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={LOOPBACK}>{t("qoder.gateway.bindLoopback")}</SelectItem>
                        <SelectItem value={LAN}>{t("qoder.gateway.bindLan")}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">{t("qoder.gateway.port")}</span>
                    <DemoAction>
                      <Input
                        className="h-8 w-28"
                        inputMode="numeric"
                        value={portDraft}
                        onChange={(event) => setPortDraft(event.target.value.replace(/[^\d]/g, ""))}
                        onBlur={commitPort}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") commitPort();
                        }}
                        aria-label={t("qoder.gateway.port")}
                      />
                    </DemoAction>
                  </div>
                  <span className="flex items-center gap-1.5 text-xs">
                    {t("qoder.gateway.statusLabel")}
                    <span
                      className={cn(
                        "inline-flex items-center gap-1.5 font-medium",
                        running ? "text-success" : "text-muted-foreground",
                      )}
                    >
                      <span className={cn("size-2 rounded-full", running ? "bg-success" : "bg-muted-foreground/50")} />
                      {running ? t("qoder.gateway.running") : t("qoder.gateway.stopped")}
                    </span>
                  </span>
                </div>

                <div className="flex flex-wrap items-center gap-2 border-t border-border/60 px-5 py-3 text-xs text-muted-foreground">
                  {t("qoder.gateway.noteLead")}
                  <code className="font-mono">/v1/chat/completions</code>
                  {t("qoder.gateway.noteTail")}
                  {config.bindAddr === LAN && t("qoder.gateway.lanWarning")}
                </div>

                {status?.lastError && (
                  <div className="border-t border-border/60 px-5 py-3">
                    <p className="text-xs text-muted-foreground">{t("qoder.gateway.lastError")}</p>
                    <p className="mt-1 break-words text-xs text-destructive">{status.lastError}</p>
                  </div>
                )}
              </Card>

              {/* ---- 接入地址（单条 Base URL：区域由作用域承载）---- */}
              <Card className="mb-6 gap-0 py-0">
                <div className="border-b border-border/60 px-5 py-3">
                  <span className="text-sm font-semibold">{t("qoder.gateway.endpoint.title")}</span>
                </div>
                <div className="px-5 py-4">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="text-xs text-muted-foreground">{t("qoder.gateway.endpoint.baseUrl")}</span>
                    <code className="min-w-0 break-all rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-xs">
                      {baseUrl}
                    </code>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void copyText(baseUrl, t("qoder.gateway.endpoint.copied"))}
                    >
                      <Copy />
                      {t("qoder.gateway.copy")}
                    </Button>
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                    <span>{t("qoder.gateway.endpoint.keyPrefix")}</span>
                    <code className="font-mono">
                      {keyPrefix ? `${keyPrefix}…` : t("qoder.gateway.endpoint.noKey")}
                    </code>
                  </div>
                </div>

                {/* ★ 概览两行：与另两个分区**同位置、同结构、同 `?sub=` 深链**。 */}
                <div className="divide-y divide-border/60 border-t border-border/60">
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("qoder.gateway.overview.models")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("qoder.gateway.overview.modelsCount", { count: modelCount })}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => goSub("models")}>
                      {t("qoder.gateway.overview.view")}
                    </Button>
                  </div>
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("qoder.gateway.overview.pool")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("qoder.gateway.overview.poolCount", { count: poolCount })}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => goSub("pool")}>
                      {t("qoder.gateway.overview.view")}
                    </Button>
                  </div>
                </div>
              </Card>

              {/* 接入指引：与 Trae 分区**共用同一个组件**（差异走 props）。 */}
              <TraeIntegrationGuide
                baseUrl={baseUrl}
                model={config.defaultModel}
                keyPrefix={keyPrefix}
                noKeyLabel={t("qoder.gateway.guide.noKey")}
                streamNote={t("qoder.gateway.guide.streamNote")}
              />
            </>
          )}

          {/* ---- 密钥（多 Key + 归属区域） ---- */}
          {section === "keys" && (
            <QoderApiKeyTable defaultRegion={region} onChanged={() => void loadAll()} />
          )}

          {/* ---- 模型清单（向上游实时拉取 + 禁用 / 启用） ---- */}
          {section === "models" && (
            <>
              {/* 限流探测卡**在表之上**（与 WB 网关页同一组件、同一位置；方案 §5.1）。
                  ⚠️ 账号轴 / 模型轴由本页按 scope 注入（Qoder 的数据不在 WB 网关 store 里），
                  组件不按平台分叉。 */}
              <ModelProbeCard
                region={region}
                accounts={probeAccounts}
                models={probeModels}
                className="mb-6"
              />
              <QoderModelList
                catalog={catalog}
                region={region}
                refreshing={refreshingModels}
                onRefresh={() => void onRefreshModels()}
                maxContext={config.maxContext}
                onToggleMaxContext={(checked) => void persist({ maxContext: checked })}
              />
            </>
          )}

          {/* ---- 账号池（三平台共用同一张卡；tiles 与「恢复」动作走数据 / 插槽） ---- */}
          {section === "pool" && (
            <>
            {/* 限额护栏在池卡**之上**：它决定「谁能被选」，是池的上游条件。 */}
            <QoderQuotaGuard
              quota={snapshot?.quota ?? null}
              refreshing={refreshingQuota}
              saving={saving}
              onRefresh={() => void onRefreshQuota()}
              onSave={(next) => void onSaveQuota(next)}
            />
            <AccountPoolCard
              pool={status?.pool ?? null}
              accounts={status?.accounts ?? []}
              diagnose={status?.diagnose ?? []}
              preferredUid={config.preferredUid}
              onPreferredUidChange={(uid) => void persist({ preferredUid: uid })}
              saving={saving}
              // 「恢复」出口：Qoder 的 `disabled` 派生自「无凭据 / 凭据失效」，
              // 出口是刷新令牌（见 `onRevive`）。
              onRevive={onRevive}
              revivingUid={revivingUid}
              // 排除：与 WorkBuddy 同一语义（硬约束），只是 scope 换成 `qoder:<region>`。
              onExclude={(uid, excluded) => void onExclude(uid, excluded)}
              excludingUid={excludingUid}
              totalRequests={status?.totalRequests ?? null}
              tiles={QODER_POOL_TILES}
              /* 选号策略（哨兵值由后端 `pool::parse_preferred` 解释成 `PickMode`）。 */
              autoOptions={[
                { value: AUTO_OPTION, label: t("shared.gateway.pool.preferredAuto") },
                {
                  value: MAX_CREDITS_OPTION,
                  label: t("shared.gateway.pool.strategyMaxCredits"),
                },
                { value: RANDOM_OPTION, label: t("shared.gateway.pool.strategyRandom") },
                {
                  value: SEQUENTIAL_OPTION,
                  label: t("shared.gateway.pool.strategySequential"),
                },
              ]}
            />
            </>
          )}

          {/* ---- 请求日志 ---- */}
          {section === "logs" && (
            <TraeRequestLog logs={logs} clearing={clearing} onClear={() => void onClearLogs()} />
          )}
          {section === "u-overview" && <UnifiedGatewayView section="overview" onGoSection={goUnified} />}
          {section === "u-namespaces" && <UnifiedGatewayView section="namespaces" onGoSection={goUnified} />}
          {section === "u-keys" && <UnifiedGatewayView section="keys" onGoSection={goUnified} />}
          {section === "u-pool" && <UnifiedGatewayView section="pool" onGoSection={goUnified} />}
      </GatewayShell>

      {/* ---- 局域网风险确认 ---- */}
      <Dialog open={riskOpen} onOpenChange={setRiskOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <ShieldAlert className="size-4 text-warning" />
              {t("qoder.gateway.lanDialog.title")}
            </DialogTitle>
            <DialogDescription>{t("qoder.gateway.lanDialog.desc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRiskOpen(false)}>
              {t("qoder.gateway.lanDialog.cancel")}
            </Button>
            <Button variant="destructive" onClick={confirmLan}>
              {t("qoder.gateway.lanDialog.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
