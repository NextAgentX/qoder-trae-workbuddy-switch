import { UnifiedGatewayView } from "@/components/gateway/unified-gateway-view";
import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Copy, FolderOpen, Loader2, Power, RefreshCw, ShieldAlert } from "lucide-react";
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
} from "@/components/gateway/account-pool-card";import { GatewayClientlessNotice } from "@/components/clientless-notice";
import { GatewayShell } from "@/components/gateway/gateway-shell";
import {
  GW_INDEPENDENT_SECTIONS_NO_PROXY,
  GW_SECTIONS,
  parseGatewayDeepLink,
  unifiedSectionId,
  type GatewaySectionId,
  type GatewayType,
} from "@/components/gateway/gateway-subnav";
import { TraeApiKeyTable } from "@/components/gateway/trae-api-key-table";
import { ModelFallbackCard } from "@/components/gateway/model-fallback-card";
import { ModelProbeCard } from "@/components/gateway/model-probe-card";
import { TraeIntegrationGuide } from "@/components/gateway/trae-integration-guide";
import { TraeModelList } from "@/components/gateway/trae-model-list";
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
import { useT } from "@/lib/i18n";
import { scopeOf } from "@/lib/scope";
import { copyText } from "@/lib/clipboard";
import {
  DEFAULT_TRAE_GATEWAY_CONFIG,
  normalizeTraeGatewayConfig,
  normalizeTraeGatewayLogs,
  normalizeTraeGatewayStatus,
  toTraeGatewayConfigRaw,
} from "@/lib/trae-gateway";
import { regionPrograms } from "@/lib/trae-variant-status";
import type { TraeGatewayConfig, TraeGatewayLogEntry, TraeGatewayStatus, TraeModelSource } from "@/lib/trae-types";
import { traeRegionOf } from "@/lib/trae-types";
import { cn } from "@/lib/utils";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { useTraeVariant } from "@/lib/use-trae-variant";

const LOOPBACK = "127.0.0.1";
const LAN = "0.0.0.0";

/**
 * Trae 网关的子导航表：**去掉「请求代理」**。
 *
 * P12 的「C 路 base URL 重定向」只覆盖 **WorkBuddy 网关端口**（`proxy.rs` 的
 * `PROXY_GATEWAY_BASE`），Trae 侧没有对应后端 ⇒ 不渲染该项 ——
 * 一个点进去什么都没有的项，比不渲染更糟（§3.4 规则 1：缺能力不画空壳）。
 */
const TRAE_GATEWAY_SUBNAV = GW_INDEPENDENT_SECTIONS_NO_PROXY;

/**
 * 本页快照：网关配置 / 监听状态 / 模型清单（按程序位）/ 请求日志。
 *
 * 它们一起缓存：都在同一次 `Promise.all` 里取、也一起被保存与清空操作改写，
 * 拆开缓存只会让「配置已保存、状态还是旧的」这种中间态有缝可钻。
 *
 * `status` 允许为 `null`（尚未取到），`config` 用默认值兜底 —— 快照还没回来时
 * 表单也还没渲染，因此默认值只服务于类型完整，不会先显示假端口再跳一下。
 */
interface ApiServiceSnapshot {
  config: TraeGatewayConfig;
  status: TraeGatewayStatus | null;
  /**
   * **客户端（上游下发）**的模型清单（issue #4），**按本区域的程序位各一份**。
   *
   * 清单是客户端级的：Trae Work 与 TraeCode 是两个客户端、两份缓存，上游下发的
   * function 分组与模型集合完全不同（TraeCode 专有的 `glm-5.3-flash` 只在它那份里）。
   * 因此这里一次取全（顺序 = 主程序在前），由卡片内的选择器决定展示哪一份 ——
   * 切换是纯前端行为，不需要新命令，也不会出现两份互不一致的快照。
   *
   * 读不到时为 `source = "missing"` 的结构（**不是** `null`）—— 界面据此呈现空态与原因。
   */
  modelSources: TraeModelSource[];
  logs: TraeGatewayLogEntry[];
}

/**
 * 「API 服务」页（Trae 分区）。
 *
 * 与 WorkBuddy 的「API 服务」页是**两套东西**：Trae 网关的上游是
 * `trae-api-cn.mchost.guru`，凭据是 `Cloud-IDE-JWT`，响应是私有 SOLO SSE，
 * 因此它的独立监听端口（默认 57894）与 WorkBuddy 网关（57891）必须分开——
 * 两个网关都占 `/v1/chat/completions`，合并到同一端口会直接冲突。
 *
 * ## 本轮结构：容器 + 五个 Trae 专用组件
 *
 * Key 管理已由后端升级为**多 Key 库（含归属产品线）**
 * （`list/create/revoke/restore/delete_trae_api_key`），
 * 因此单 Key 卡与其「重新生成」入口（R6）一并删除，改由 `TraeApiKeyTable` 承担
 * （「归属产品线」列取代 WorkBuddy 的「归属版本」列）。接入指引 / 账号池 / 模型清单 /
 * 请求日志分别下沉为 `TraeIntegrationGuide` / `AccountPoolCard`（与 WorkBuddy 共用）/
 * `TraeModelList` /
 * `TraeRequestLog`——**页面不再保留任何内联副本**。
 *
 * **骨架与 WorkBuddy 刻意同构**（容器宽度、页头字号、卡片节奏、工具条排布），
 * 但下列控件因 Trae 无对应能力而**不出现**：`Region` Tabs / 「按版本」双条目 /
 * `AccountStrategyCard`（Trae 无 `accountStrategy`）——分别以单条 Base URL、
 * 账号池卡替代。
 *
 * ## 模型清单：数据源已换成**客户端缓存**（issue #4），且**按程序位**取（后续报障）
 *
 * 上一版这里的清单来自 `get_trae_gateway_models`（网关对外暴露的**静态**常量），
 * 因此当时刻意不加刷新按钮 —— 刷新永不改变结果。issue #4 之后改用
 * `get_trae_client_models`（读客户端 `state.vscdb` 里上游下发的清单缓存），
 * 清单**随客户端刷新而变**，刷新按钮因此恢复。
 *
 * ⚠️ 但一个区域下有**两条程序位**（TraeWork / TraeCode），清单是客户端级的、
 * 两份缓存内容完全不同 —— 只读主程序那一份时，TraeCode 专有的模型
 * （实测 `glm-5.3-flash`）在界面上永远看不到。故本页按 `regionPrograms()` 把
 * 本区域各程序位**一次取全**交给卡片，由卡片内的选择器切换展示。
 */
export default function TraeApiServicePage() {
  const t = useT();
  /** 当前产品线：决定读哪个账号池（`trae_gateway_status(variant)`）与归属列默认值。 */
  const [variant] = useTraeVariant();
  const [saving, setSaving] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);
  const [openingDir, setOpeningDir] = useState(false);
  /** 正在「恢复」的 uid（池卡上该行按钮显示操作进行中）。 */
  const [revivingUid, setRevivingUid] = useState<string | null>(null);
  /** 模型清单「重新读取」的进行中状态（只驱动按钮的转圈，不参与快照）。 */
  const [refreshingModels, setRefreshingModels] = useState(false);
  /* 快照刷新两处入口（错误 Alert 重试 / 概览刷新）各自的进行中态：`void loadAll()` 是
     fire-and-forget —— `loading` 在「有旧值」时恒为 false，只有本 hook 才给得出点击反馈。
     与 WorkBuddy 的 `ApiServicePage` **同一做法**（两处入口各一个 hook，互不串转圈）。 */
  const { busy: retryBusy, run: runRetry } = useRefreshAction();
  const { busy: refreshBusy, run: runRefresh } = useRefreshAction();
  /* ★ 子导航选中项：**初值从 URL 读**（`?sub=logs`），非法值 fail-closed 回 `overview`
     （与设置页 `?section=`、WB 网关页同一口径）。
     ⚠️ 原实现只**读**不**写** —— 注释承诺了「同一口径」，但点击子导航后 URL 不变，
     刷新就回到 `overview`。这里补齐写入，与 WB 网关页 / 两平台设置页完全同构。 */
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
    // 本区域的两条程序位（TraeWork / TraeCode）：清单按程序位分家，一次取全。
    const programs = regionPrograms(variant);
    // 每个程序位取**两**份：客户端清单（分组视图）+ 网关对外清单的条数。
    // 后者必须按程序位各取一次 —— `/v1/models` 只列该程序位 `function` 能调的模型，
    // 两条程序位的条数不一样（TraeWork 与 TraeCode 是两个 function）。
    const [configRaw, statusRaw, logsRaw, ...perProgram] = await Promise.all([
      api.getTraeGatewayConfig(),
      api.getTraeGatewayStatus(variant),
      api.getTraeGatewayLogs(),
      ...programs.map(async (program) => {
        const gateway = await api.getTraeGatewayModels(program.variant);
        return {
          data: await api.getTraeClientModels(program.variant),
          gatewayNames: readModelNames(gateway),
          // 逐模型上下文：官方目录的 dev/max（不是统一 1M），表里直接显示。
          gatewayContexts: readModelContexts(gateway),
        };
      }),
    ]);
    return {
      config: normalizeTraeGatewayConfig(configRaw),
      status: normalizeTraeGatewayStatus(statusRaw),
      modelSources: programs.map((program, index) => ({
        ...program,
        data: perProgram[index]?.data ?? null,
        gatewayNames: perProgram[index]?.gatewayNames ?? null,
        gatewayContexts: perProgram[index]?.gatewayContexts ?? null,
      })),
      logs: normalizeTraeGatewayLogs(logsRaw),
    };
  }, [variant]);

  /**
   * 快照缓存：切到 WorkBuddy 再切回来时不再闪骨架。变更操作（保存配置、清空日志）
   * 走 `patch` 就地改写快照，而不是写组件 state —— 值归缓存所有，两处写迟早分歧。
   */
  const {
    data: snapshot,
    loading,
    error,
    refresh: loadAll,
    patch,
  } = useCachedResource<ApiServiceSnapshot>(`trae:api-service:${variant}`, loadSnapshot);

  const config = snapshot?.config ?? DEFAULT_TRAE_GATEWAY_CONFIG;
  const status = snapshot?.status ?? null;
  const modelSources = snapshot?.modelSources ?? [];
  const logs = snapshot?.logs ?? [];

  /**
   * 限流探测卡（与 WB / Qoder 页**同一个组件**）要的**账号轴 / 模型轴**。
   *
   * ⚠️ Trae 的池 / 清单**不在** WB 网关 store 里，故这里**显式注入**自己的轴 ——
   * 「列集合 / 账号轴由作用域决定」由本页保证，组件本身**不按平台分叉**
   * （与 `QoderApiServicePage` 同一写法）。
   */
  const probeAccounts = useMemo(
    () => (status?.accounts ?? []).map((account) => ({ uid: account.uid, name: account.name })),
    [status],
  );
  /**
   * 模型轴：与后端 `model_probe::catalog_models` 的 Trae 分支**同源** —— 取该区域当前
   * 程序位的**网关对外清单**（`gatewayNames`，即 `/v1/models` 真正会列的模型；后端
   * `models_response_for` 也是这一份）。显示名回落客户端清单里的 `displayName`。
   *
   * 收敛后每个区域只有一个程序位（合并版），故直接取 `modelSources[0]`。
   */
  const probeModels = useMemo(() => {
    const source = modelSources[0] ?? null;
    const display = new Map<string, string>();
    for (const group of source?.data?.groups ?? []) {
      for (const model of group.models) {
        if (!display.has(model.name)) display.set(model.name, model.displayName || model.name);
      }
    }
    const seen = new Set<string>();
    const out: { id: string; name: string }[] = [];
    for (const id of source?.gatewayNames ?? []) {
      if (!id || seen.has(id)) continue;
      seen.add(id);
      out.push({ id, name: display.get(id) ?? id });
    }
    return out;
  }, [modelSources]);

  /**
   * 「重新读取」模型清单。
   *
   * 走快照的 `refresh`（而不是自己再发一次请求）：这样「重新读取」与「切产品线后加载」
   * 共用同一条链路，不会出现两份互不一致的快照。失败时不额外弹错 —— 卡片自身会显示
   * `source = missing` 与后端给出的原因，页面顶部的错误条也会出现。
   */
  const onRefreshModels = useCallback(async () => {
    setRefreshingModels(true);
    try {
      await loadAll();
    } finally {
      setRefreshingModels(false);
    }
  }, [loadAll]);

  /**
   * 端口输入框的草稿值。
   *
   * 与 `config.port` 同步的时机是「`config.port` 本身变了」：加载完成、保存成功后
   * 都会变，而用户打字期间 `config.port` 不动，所以输入不会被打断。
   * 初值直接取当前配置，因此**带着缓存重挂载时不会先显示默认端口再跳一下**。
   */
  const [portDraft, setPortDraft] = useState(() => String(config.port));
  useEffect(() => {
    setPortDraft(String(config.port));
  }, [config.port]);

  /** 就地改写快照（保留其余字段）。 */
  const patchSnapshot = useCallback(
    (next: Partial<ApiServiceSnapshot>) => {
      patch((current) => ({ ...current, ...next }));
    },
    [patch],
  );

  async function persist(next: Partial<TraeGatewayConfig>) {
    const merged = { ...config, ...next };
    // 乐观更新：开关立刻跟手，失败再回滚 —— 本地保存很快，但「点了没反应」的体感
    // 通常就是等 IO 这段。注意主流程仍以服务端返回为准（下面 `loadAll` 会重读）。
    const previous = config;
    patchSnapshot({ config: merged });
    setSaving(true);
    try {
      await api.saveTraeGatewayConfig(toTraeGatewayConfigRaw(merged));
      // 保存会启动/重启监听，状态要重读（仍按当前产品线）。
      const statusRaw = await api.getTraeGatewayStatus(variant);
      patchSnapshot({ status: normalizeTraeGatewayStatus(statusRaw) });
      // ★ 清单里的「上下文」是按 `/v1/models` 当前声明抓的 ⇒ **只有改开关**才需要重取
      //   （改端口/地址不必白跑一趟）；否则表格停在旧值，就是「点了、列表没变」。
      // 表格已按开关本地选档，重取只用于对齐服务端计数/来源，**不阻塞交互**。
      if (next.maxMode !== undefined) void onRefreshModels();
      toast.success(
        merged.enabled
          ? t("trae.stats.api.toast.savedStarted")
          : t("trae.stats.api.toast.savedDisabled"),
      );
    } catch (e) {
      patchSnapshot({ config: previous });
      toast.error(t("trae.stats.api.toast.saveFailed"), { description: api.asError(e) });
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
      toast.error(t("trae.stats.api.toast.portInvalid"), {
        description: t("trae.stats.api.toast.portRange"),
      });
      setPortDraft(String(config.port));
      return;
    }
    if (parsed === config.port) return;
    void persist({ port: parsed });
  }

  async function onClearLogs() {
    setClearing(true);
    try {
      await api.clearTraeGatewayLogs();
      patchSnapshot({ logs: [] });
      toast.success(t("trae.stats.api.toast.logsCleared"));
    } catch (e) {
      toast.error(t("trae.stats.api.toast.clearFailed"), { description: api.asError(e) });
    } finally {
      setClearing(false);
    }
  }

  /**
   * 打开 Trae 数据目录。
   *
   * 非 Windows 上后端返回**结构化 `Unsupported`**（不是假成功）：此时如实把
   * 「当前平台不支持」提示给用户，而不是弹一句「已打开」却什么都没发生。
   */
  async function onOpenDataDir() {
    setOpeningDir(true);
    try {
      const result = (await api.openTraeDataDir(variant)) as {
        ok?: boolean;
        path?: string;
        capability?: string;
        reason?: string;
      };
      if (result?.capability) {
        toast.message(t("trae.stats.api.toast.unsupported"), {
          description: result.reason ?? t("trae.stats.api.toast.unsupportedDesc"),
        });
        return;
      }
      toast.success(t("trae.stats.api.toast.dirOpened"), { description: result?.path });
    } catch (e) {
      toast.error(t("trae.stats.api.toast.dirOpenFailed"), { description: api.asError(e) });
    } finally {
      setOpeningDir(false);
    }
  }

  /**
   * 解除某账号的禁用（Trae 侧）。
   *
   * ## 为什么复用 `trae_clear_cooldown` 而不是新造一条命令
   *
   * Trae 的 `disabled` **不是池自己的状态**，而是每次 `sync_for` 从**冷却文件**派生的
   * （`error_type == "SessionDead"`，见 `trae/pool.rs`）；`clear_cooldown_for` 无条件
   * 删掉整条记录 ⇒ `disabled` 随之消失。所以「恢复」= 清冷却，后端出口**本来就存在**
   * （core `clear_cooldown_for` + 命令 `trae_clear_cooldown` + 签到页菜单），
   * 缺的只是**池卡上的入口** —— 用户在网关页看到「已禁用」却找不到任何按钮。
   *
   * ⚠️ 因此**不要**在 Trae 侧照抄 WorkBuddy 的 `revive_gateway_account`：WB 的
   * `disabled` 是池内状态（需同时清冷却与熔断截止），Trae 的是磁盘冷却记录的投影
   * （清记录即可，Trae 没有熔断这一维）。差异走**数据与插槽**，前端共用同一张池卡。
   */
  /**
   * 排除 / 取消排除（Trae 侧）。
   *
   * 与「恢复」的区别：恢复是修**故障**（清冷却记录），排除是记**用户意图**。
   * 后端落盘后会立刻被下一次选号读到（Trae 池每次都从磁盘重建），
   * 因此只需重新拉一次状态。
   */
  async function onExclude(uid: string, excluded: boolean) {
    setExcludingUid(uid);
    try {
      await api.setTraeGatewayPoolExclusion(variant, uid, excluded);
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
      await api.traeClearCooldown(uid, variant);
      toast.success(t("shared.gateway.pool.reviveDone"));
      // 池状态来自 `status` ⇒ 重新拉一次，否则界面还显示「已禁用」。
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
  /* ★ 概览两行的取数（与 WB 侧 `ApiServicePage` 的概览行**同源口径**）：
     - 「可用模型」= **网关对外清单**（`/v1/models` 真正会列出的那批）的条数，
       取本区域**主程序位**那一份（清单卡内的程序位选择器只管卡片自己的展示，
       概览行给的是区域主线的口径 —— 与 WB 的「当前 region 清单条数」对应）；
     - 「账号池」= 池条目总数（`pool.total`，缺数据时回落到账号数）。 */
  const modelCount = modelSources[0]?.gatewayNames?.length ?? 0;
  const poolCount = status?.pool?.total ?? status?.accounts?.length ?? 0;
  /* 页头作用域：平台固定 trae，区域来自 `useTraeVariant`（URL 作用域优先）。 */
  const scope = scopeOf("trae", variant);
  const [excludingUid, setExcludingUid] = useState<string | null>(null);

  /* 页头抽成变量：加载态与正常态**共用同一份** —— 骨架屏出现时页头不闪掉。
     （原型 `viewGateway`：作用域副标题 + 只读 scope-chip。区域切换已上移到顶栏的
     `ScopeSelector`（L1 作用域），页内不再有区域选择器。） */
  const header = (
    <header className="mb-5 flex items-start gap-4">
      <div className="min-w-0">
        <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("trae.stats.api.title")}</h1>
        <p className="mt-1.5 text-[12.5px] text-muted-foreground">
          {t("pageHead.gateway", { scope: scopeLabel(t, scope) })}
        </p>
      </div>
      <div className="grow" />
      <ScopeChip scope={scope} />
    </header>
  );

  /* ★ 首次加载（`loading && !status`）：页头常驻 + 主区骨架，与 WB 页**同一条件、同一骨架**。
     ⚠️ 此前这里是「整页替换」且骨架块形状不同（`h-9 w-48` / `h-32` / `h-64`）——
     与 WB 的「页面底部追加 3 行文字骨架」是两种做法。现在两页完全一致。 */
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
          <AlertTitle>{t("trae.stats.api.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-3">
            <span>{error}</span>
            <div>
              <Button variant="outline" size="sm" disabled={retryBusy} onClick={() => void runRetry(() => loadAll())}>
                {retryBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                {t("trae.stats.api.retry")}
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}

      {/* ---- 主区：子导航 + 分区内容（PLAN §3.9：替代原来的 6 卡纵向堆叠）----
          方向 / 对齐 / 间距整组在 `index.css` 的 `[data-split-layout]`（原型 `.split`，
          含 900px 转纵向）。⚠️ 此前这里**缺 `data-split-layout`** ⇒ 900px 以下不转纵向、
          子导航也不换行，与 WB 网关页行为不同。 */}
      {/* 无客户端形态（issue #62）：网关只依赖账号池里的凭据，与客户端是否同机无关。 */}
      <GatewayClientlessNotice scope={scope} />
      <GatewayShell
        type={type}
        section={section}
        sections={type === "independent" ? TRAE_GATEWAY_SUBNAV : undefined}
        onTypeChange={goType}
        onSelect={goSub}
      >
          {/* ---- 概览：服务状态 + 接入地址 + 接入指引 ---- */}
          {section === "overview" && (
            <>
              {/* ---- 网关 ---- */}
              <Card className="mb-6 gap-0 py-0">
                <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-4">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2 text-sm font-medium">
                      <Power className="size-4 text-muted-foreground" />
                      {t("trae.stats.api.gateway.enable")}
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {t("trae.stats.api.gateway.desc", {
                        upstream: status?.upstream || "trae-api-cn.mchost.guru",
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
                        aria-label={t("trae.stats.api.gateway.enable")}
                      />
                    </DemoAction>
                    <DemoAction>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={refreshBusy}
                        onClick={() => void runRefresh(() => loadAll())}
                      >
                        {refreshBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                        {t("trae.stats.api.refresh")}
                      </Button>
                    </DemoAction>
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-x-6 gap-y-3 px-5 py-4">
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">{t("trae.stats.api.gateway.bindAddr")}</span>
                    <Select value={config.bindAddr} onValueChange={onBindAddrChange} disabled={saving}>
                      <SelectTrigger size="sm" className="w-52" aria-label={t("trae.stats.api.gateway.bindAddr")}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={LOOPBACK}>{t("trae.stats.api.gateway.bindLoopback")}</SelectItem>
                        <SelectItem value={LAN}>{t("trae.stats.api.gateway.bindLan")}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">{t("trae.stats.api.gateway.port")}</span>
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
                        aria-label={t("trae.stats.api.gateway.port")}
                      />
                    </DemoAction>
                  </div>
                  <span className="flex items-center gap-1.5 text-xs">
                    {t("trae.stats.api.gateway.statusLabel")}
                    <span
                      className={cn(
                        "inline-flex items-center gap-1.5 font-medium",
                        running ? "text-success" : "text-muted-foreground",
                      )}
                    >
                      <span className={cn("size-2 rounded-full", running ? "bg-success" : "bg-muted-foreground/50")} />
                      {running
                        ? t("trae.stats.api.gateway.running")
                        : t("trae.stats.api.gateway.stopped")}
                    </span>
                  </span>
                  {/* ★「打开数据目录」已移到下方「接入地址」卡的 Base URL 行 ——
                      与 WB 侧**同位置同 variant**（打开的是这个网关对应的客户端数据目录，
                      属「接入信息」而不是「服务开关」）。 */}
                </div>

                <div className="flex flex-wrap items-center gap-2 border-t border-border/60 px-5 py-3 text-xs text-muted-foreground">
                  {t("trae.stats.api.gateway.noteLead")}
                  <code className="font-mono">/v1/chat/completions</code>
                  {t("trae.stats.api.gateway.noteTail")}
                  {config.bindAddr === LAN && t("trae.stats.api.gateway.lanWarning")}
                </div>

                {status?.lastError && (
                  <div className="border-t border-border/60 px-5 py-3">
                    <p className="text-xs text-muted-foreground">{t("trae.stats.api.gateway.lastError")}</p>
                    <p className="mt-1 break-words text-xs text-destructive">{status.lastError}</p>
                  </div>
                )}
              </Card>

              {/* ---- 接入地址 ---- */}
              {/* WorkBuddy 此处是「按版本」逐 region 一行；Trae 无 region，故只有一条。 */}
              <Card className="mb-6 gap-0 py-0">
                <div className="border-b border-border/60 px-5 py-3">
                  <span className="text-sm font-semibold">{t("trae.stats.api.endpoint.title")}</span>
                </div>
                <div className="px-5 py-4">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="text-xs text-muted-foreground">{t("trae.stats.api.endpoint.baseUrl")}</span>
                    <code className="min-w-0 break-all rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-xs">
                      {baseUrl}
                    </code>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void copyText(baseUrl, t("trae.stats.api.endpoint.copied"))}
                    >
                      <Copy />
                      {t("trae.stats.api.copy")}
                    </Button>
                    {/* ★ 与 WB 侧同位置同 variant（ghost）：打开的是**本程序位**的数据目录。 */}
                    <DemoAction>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={openingDir}
                        onClick={() => void onOpenDataDir()}
                      >
                        {openingDir ? <Loader2 className="animate-spin" /> : <FolderOpen />}
                        {t("trae.stats.api.gateway.openDataDir")}
                      </Button>
                    </DemoAction>
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                    <span>{t("trae.stats.api.endpoint.keyPrefix")}</span>
                    <code className="font-mono">
                      {keyPrefix ? `${keyPrefix}…` : t("trae.stats.api.endpoint.noKey")}
                    </code>
                  </div>
                </div>

                {/* ★ 概览两行（2026-10-13 跨平台对齐）：与 WB 的概览行**同位置、同结构、
                    同 `?sub=` 深链**。数字全部来自后端下发，「查看」跳到对应分区。
                    ⚠️ 比 WB 少一行「上游账号凭据 · 校验」：那是 WB 的 `check_auth_permission`
                    写权限探针，Trae 侧没有对应命令 —— 缺能力不画空壳（§3.4 规则 1）。 */}
                <div className="divide-y divide-border/60 border-t border-border/60">
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("trae.stats.api.overview.models")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("trae.stats.api.overview.modelsCount", { count: modelCount })}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => goSub("models")}>
                      {t("trae.stats.api.overview.view")}
                    </Button>
                  </div>
                  <div className="flex items-center justify-between gap-3 px-5 py-3">
                    <div className="min-w-0">
                      <div className="text-sm">{t("trae.stats.api.overview.pool")}</div>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("trae.stats.api.overview.poolCount", { count: poolCount })}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => goSub("pool")}>
                      {t("trae.stats.api.overview.view")}
                    </Button>
                  </div>
                </div>
              </Card>

              {/* ---- 接入指引（单条 Base URL，无 region Tabs） ---- */}
              <TraeIntegrationGuide
                baseUrl={baseUrl}
                model={config.defaultModel}
                keyPrefix={keyPrefix}
              />
            </>
          )}

          {/* ---- 密钥（多 Key + 归属产品线） ---- */}
          {/* 替代 WorkBuddy 的单 Key 卡：这里支持多把 Key，每把绑定一条产品线。 */}
          {section === "keys" && (
            <TraeApiKeyTable defaultVariant={variant} onChanged={() => void loadAll()} />
          )}

          {/* ---- 模型清单（按程序位读客户端缓存，可重新读取：见 TraeModelList 的模块头） ---- */}
          {section === "models" && (
            <>
              {/* 限流探测卡**在表之上**（与 WB / Qoder 页同一组件、同一位置；方案 §5.1）。
                  ⚠️ 账号轴 / 模型轴由本页按区域注入（Trae 的数据不在 WB 网关 store 里），
                  组件不按平台分叉。
                  ★ `adoptSupported={false}`：Trae 池**没有「模型级冷却」**，回填只能写账号级、
                  会误伤同账号其它模型（方案 §4.5.4 的 T1）⇒ 开关禁用并说明原因。 */}
              <ModelProbeCard
                region={traeRegionOf(variant)}
                accounts={probeAccounts}
                models={probeModels}
                adoptSupported={false}
                className="mb-6"
              />
              {/* 降级链：与 WB 页同一组件，但作用域与模型清单由本页注入
                  （Trae 的清单来自客户端 `vscdb`，不在 gateway store 里）。 */}
              <ModelFallbackCard
                region={traeRegionOf(variant)}
                scope={scopeOf("trae", traeRegionOf(variant))}
                modelOptions={probeModels.map((model) => model.name)}
                className="mb-6"
              />
              <TraeModelList
                sources={modelSources}
                region={traeRegionOf(variant)}
                defaultModel={config.defaultModel}
                refreshing={refreshingModels}
                onRefresh={() => onRefreshModels()}
                maxMode={config.maxMode}
                onToggleMaxMode={(checked) => void persist({ maxMode: checked })}
              />
            </>
          )}

          {/* ---- 账号池（与 WorkBuddy 侧共用同一张卡） ---- */}
          {section === "pool" && (
            <AccountPoolCard
              pool={status?.pool ?? null}
              accounts={status?.accounts ?? []}
              diagnose={status?.diagnose ?? []}
              preferredUid={config.preferredUid}
              onPreferredUidChange={(uid) => void persist({ preferredUid: uid })}
              saving={saving}
              // 「恢复」出口：Trae 的 `disabled` 派生自冷却文件，故走 `trae_clear_cooldown`
              // （复用既有命令，见 `onRevive` 的说明）。
              onRevive={onRevive}
              revivingUid={revivingUid}
              // 排除：scope 按**变体**分家（`trae:trae_work` / `trae:trae_cn` / …）。
              onExclude={(uid, excluded) => void onExclude(uid, excluded)}
              excludingUid={excludingUid}
              // ★ issue #129：账号故障时自动切换（网关级，与会话粘性同层）。
              autoFailover={config.autoFailover}
              onAutoFailoverChange={(checked) => void persist({ autoFailover: checked })}
              totalRequests={status?.totalRequests ?? null}
              /* 选号策略（与 WorkBuddy 同一张卡、同一套哨兵值）。
                 值经后端 `pool::parse_preferred` 解释成 `PickMode` —— 见该函数的文档：
                 哨兵值必须与后端常量逐字相同，否则会静默回落成加权（假开关）。 */
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
              {t("trae.stats.api.lanDialog.title")}
            </DialogTitle>
            <DialogDescription>{t("trae.stats.api.lanDialog.desc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRiskOpen(false)}>
              {t("trae.stats.api.lanDialog.cancel")}
            </Button>
            {/* ★ 与 WB 页同构：标题带警示图标 + 确认按钮 `destructive`（同一处危险操作，
                此前 Trae 用默认 variant + `<Power>` 图标，两边长得不一样）。 */}
            <Button variant="destructive" onClick={confirmLan}>
              {t("trae.stats.api.lanDialog.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/**
 * `get_trae_gateway_models` 返回 OpenAI `/v1/models` 形状，这里取**模型 id 列表**。
 *
 * 取 id 而不只取条数：卡片既要显示「网关对外 N 个」，也要**逐个标记**哪些模型
 * 不在对外清单里（那就是「看得见、调不动」的那批）。只解析 `id` 一个字段，
 * 不必再建一份模型类型。
 */
/**
 * 同上，但取 `id → context_length` 映射（`/v1/models` 已经按官方目录声明了每个模型**自己的**
 * 上限：`max > dev` 时是 max，否则是 dev）。表格的「上下文」列直接用这份数字。
 */
function readModelContexts(
  raw: unknown,
): Record<string, { dev: number | null; max: number | null }> | null {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  if (!Array.isArray(record.data)) return null;
  const out: Record<string, { dev: number | null; max: number | null }> = {};
  for (const item of record.data) {
    if (!item || typeof item !== "object") continue;
    const row = item as Record<string, unknown>;
    if (typeof row.id !== "string") continue;
    const chosen = typeof row.context_length === "number" ? row.context_length : null;
    const dev = typeof row.context_length_default === "number" ? row.context_length_default : chosen;
    const max = typeof row.context_length_max === "number" ? row.context_length_max : null;
    out[row.id] = { dev, max };
  }
  return out;
}

function readModelNames(raw: unknown): string[] | null {
  const record = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  if (!Array.isArray(record.data)) return null;
  return record.data
    .map((item) => (item && typeof item === "object" ? (item as Record<string, unknown>).id : null))
    .filter((id): id is string => typeof id === "string");
}
