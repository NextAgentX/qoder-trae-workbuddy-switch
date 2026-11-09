/**
 * 「用量」· **Token 视图**（Trae 分区）：趋势 / 模型 —— 与 WorkBuddy **同一套骨架与图表**。
 *
 * ## 严格按原型实现（用户 2026-10-12 要求）
 *
 * 原型 `viewUsage`（`docs/prototype/demo-app.js:794-997`）是**平台无关**的
 * （同一段代码 + 不同数据），两个 Token 视图的块序逐条对齐：
 *
 * - **趋势**：4 统计卡 → `每日 Token · 按模型拆分`（堆叠柱 + 图例）
 *   → `累计消耗曲线` | `模型占比`（g2）→ `使用时段分布`（7×24 热力图）；
 * - **模型**：4 统计卡 → `模型 Token 占比`（环形）→ `模型用量排行`（横向条 + 迷你趋势线）。
 *
 * 改造前本页是 Trae 自建的一套（整年活动网格 / 每日折线 / 按模型柱 / 按模型明细 /
 * 按账号表 / 状态码卡）—— **原型里一个都没有**。现已全部替换：
 * 「按账号」与「状态码」两个维度在原型里是**积分页的两个环形**（见 `TraeCreditsPage`），
 * 不是趋势页的卡片。
 *
 * ## 与 WorkBuddy 的差异**只在数据**
 *
 * | 维度 | WorkBuddy | Trae（本页） |
 * |:--|:--|:--|
 * | 数据源 | 客户端会话文件（4 来源） | 网关账本 + **用户导出的客户端明文库**（两本账） |
 * | 「活跃账号」统计卡 | **无账号维度** ⇒ 用「模型数」替代（门禁已登记） | 有 `accounts` ⇒ 原型原样「活跃账号」 |
 * | 数据源提示条 | 无 | 「数据来源」Alert（**必须保留**，否则用户把「数字小」当 bug） |
 *
 * ## 客户端账本为什么要「导入」（第二轮改造）
 *
 * Trae 的对话库是 **SQLCipher 加密**的，本工具不内置解密 ⇒ 客户端那一本账只能由用户
 * 自己导出明文副本、再把路径交给本工具（面板见 `components/usage/trae-client-import`）。
 * 因此这本账有**三种「没有数」**，绝不能混成同一个 0：
 *
 * - **未配置**（`configured === false`）—— 用户还没指路；
 * - **读失败**（`error.code` 五种）—— 指了路但读不出来（不是明文 / 认不出 / 没用量字段 / 文件没了 / IO）；
 * - **真的是 0** —— 读到了，这段窗口里确实没用量。
 *
 * 覆盖条据此区分「客户端 0」与「客户端不可得」（`clientUnavailable`），
 * 这就是本次改造的核心：**测不到**不能写成**没用**。
 *
 * ## `all` 档为什么不把两本账相加
 *
 * 同一个请求可能既落在客户端库里、又走了网关 ⇒ 逐键相加就是**双计**；而 Trae 没有
 * 「对账」接口可以算出重叠量（`get_usage_reconcile` 仅 WorkBuddy 有）。
 * 所以 `all` 档**只**把逐日柱画成「客户端 / 网关」两段并排（`ledgerDailySeries`），
 * 统计卡与其余图表仍按**网关一本**算，并在页面上逐字写明口径（`totalGatewayTitle`）。
 * 宁可少说，也不给一个看起来对的合计。
 *
 * 图表**一律走共用原语**（`StackedDailyBar` / `CumulativeArea` / `ModelDonut` /
 * `ModelRanking` / `UsageHeatmap`），数据由 `lib/trae-usage-charts.ts` 映射成共用形状
 * ⇒ 契约（分段和=当日总量 / 7×24 / 扇区数=模型数）只有一份实现。
 *
 * ## 视图参数是**受控**的
 *
 * `range` / `combineAll` 由 `UsagePage` 传入（原型 `state.range` / `state.merge` 是页面级的）。
 * 本页**不持有**参数状态，也**不放**任何页内区域选择器 —— 区域归侧栏。
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Gauge,
  Info,
  Loader2,
  MessagesSquare,
  RefreshCw,
  Server,
  Sparkles,
  TrendingUp,
  Users,
} from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { CardHead } from "@/components/ui/card-head";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { SlowLoadHint } from "@/components/slow-load-hint";
import { scopeLabel } from "@/components/scope-chip";
import { ChartFrame } from "@/components/usage/chart-frame";
import { CumulativeArea } from "@/components/usage/cumulative-area";
import { compactTokens, exactTokens } from "@/components/usage/format";
import {
  CoverageStrip,
  LedgerModelTable,
} from "@/components/usage/ledger-parts";
import { TraeClientImport } from "@/components/usage/trae-client-import";
import { ModelDonut } from "@/components/usage/model-donut";
import { ModelRanking } from "@/components/usage/model-ranking";
import { KeysView } from "@/components/usage/key-ranking";
import { StackedDailyBar } from "@/components/usage/stacked-daily-bar";
import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import { UsageHeatmap } from "@/components/usage/usage-heatmap";
import { useUsageWindowLabel, type UsageViewProps } from "@/components/usage/view-param-bar";
import * as api from "@/lib/api";
import { cumulativeCaption } from "@/lib/chart-caption";
import { useT } from "@/lib/i18n";
import { scopeOf } from "@/lib/scope";
import { toTokenStatsSource } from "@/lib/trae-usage-charts";
import type { GatewayUsageDim } from "@/lib/types";
import type {
  TraeApiKeyRecord,
  TraeTokenScope,
  TraeTokenStatistics,
  TraeUnsupported,
} from "@/lib/trae-types";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { useScope } from "@/lib/use-scope";
import {
  dailyModelSeries,
  heatmapMatrix,
  modelSlices,
} from "@/lib/usage-charts";
import {
  clientDailyInWindow,
  clientModelTotals,
  dimCoverage,
  dimTotals,
  ledgerDailySeries,
  ledgerToTokenStatsSource,
  modelKey,
  showsClient,
  showsGateway,
  type UsageLedger,
} from "@/lib/usage-ledger";
import {
  usageWindowDays,
  usageWindowKeys,
  usageWindowSpanDays,
  type UsageWindow,
} from "@/lib/usage-window";

/** 本页承载的视图（积分视图在 `TraeCreditsPage`）。与原型 `tabs` 的前两项同名。 */
export type TraeUsageView = "trend" | "models" | "keys";

/**
 * 客户端账本的返回体（判别式）：`{configured:false}` / `{error}` / 正常统计。
 *
 * 用 `Awaited<ReturnType<...>>` 而不是再抄一遍字段：返回形状一变，
 * 这里跟着变，**不会**出现「类型说有三本账、实现只有两本」的漂移。
 */
type TraeClientStats = Awaited<ReturnType<typeof api.getTraeClientTokenStats>>;

/**
 * 「按密钥」视图**只取展示名**：用量来自网关账本（本页已取的 `dim`），
 * 不再读请求日志 —— 那是环形缓冲，窗口对齐不了（见 `src/lib/key-usage.ts`）。
 * 模块级函数 ⇒ 引用稳定（`KeysView` 的 `useEffect` 依赖它）。
 */
async function loadTraeKeyNames() {
  const keys = await api.listTraeApiKeys().catch(() => ({ keys: [] as TraeApiKeyRecord[] }));
  return new Map((keys.keys ?? []).map((key) => [key.id, key.name] as const));
}

/**
 * 「用量 · Token」视图（Trae 分区）。
 *
 * `range` / `combineAll` 受控；`view` 决定画趋势还是模型。
 */
export default function TraeTokenStatsPage({
  view = "trend",
  viewWindow,
  combineAll,
  ledger = "gateway",
}: { view?: TraeUsageView } & UsageViewProps & { ledger?: UsageLedger }) {
  const t = useT();
  const [region] = useScope("trae");
  const scope = scopeOf("trae", region);
  /**
   * 范围筛选（`TraeTokenScope`）= **区域**档：`cn` / `global` / `unlabeled` / `all`。
   *
   * 「仅本作用域」= 当前区域；「合并全部作用域」= `all`（国内 ∪ 国际 ∪ 未标注旧日志）。
   * 与 WorkBuddy 的 `RegionFilter` 是**同一根轴的两个取值形态** ⇒ 共用同一枚控件。
   */
  const tokenScope: TraeTokenScope = combineAll ? "all" : region;
  /* ★ Trae 的统计接口按「最近 N 天」拉日志（`days <= 0` = 不限）⇒ 必须拉**覆盖整个窗口**
     的跨度，而不是窗口自己的天数：窗口可能是**锚在过去**的一段（如 9/1–9/5），
     拉 5 天只会拉到最后 5 天，那段历史**根本不在返回里**，图表会**静默全 0**。
     拉回来之后再按窗口的日历日切片 —— 多拉的日在这两步之间被丢掉。 */
  const spanDays = usageWindowSpanDays(viewWindow);

  const load = useCallback(
    () => api.getTraeTokenStatistics(spanDays, tokenScope),
    [spanDays, tokenScope],
  );

  const {
    data: stats,
    loading,
    slow,
    error,
    refresh,
  } = useCachedResource<TraeTokenStatistics>(`trae:token-stats:${spanDays}:${tokenScope}`, load);

  /** 重试按钮的进行中态（`loading` 只在无值时 true，点重试那一刻它恒为 false）。 */
  const { busy, run } = useRefreshAction();

  /**
   * ★ **客户端账本**（第二轮：读用户自己导出的明文库）。
   *
   * 与 `stats` / `dim` 都不同源：它读的是用户在设置里指定的那份 SQLCipher 明文副本
   * （本工具不内置解密）。返回体是**判别式**的 —— `{configured:false}` 表示还没指路、
   * `{error:{code}}` 表示指了路但读不出来，这两种**都不是「这段窗口没用量」**。
   *
   * 取数在**所有账本档位**下都发生：覆盖条要能回答「客户端这本账现在读得到吗」，
   * 而不是只在用户切到客户端时才去探测。
   */
  const loadClient = useCallback(
    () => api.getTraeClientTokenStats(spanDays, tokenScope),
    [spanDays, tokenScope],
  );
  const {
    data: clientStats,
    error: clientLoadError,
    refresh: refreshClient,
  } = useCachedResource<TraeClientStats>(`trae:client-stats:${spanDays}:${tokenScope}`, loadClient);

  /**
   * 客户端**读不到**的原因：后端给了判别式的 `error.code`；命令本身失败（旧版本 / 演示站）
   * 只有一条字符串，按 `io` 归类 —— 那也是一次真实的读取失败，不能当成「没配置」。
   */
  const clientError: api.TraeClientImportError | null =
    clientStats?.error ?? (clientLoadError !== null ? { code: "io", message: clientLoadError } : null);
  /** 「这本账现在可用吗」：未配置 / 读失败 / 还没取到 —— 三者都是**不可得**，不是 0。 */
  const clientReady =
    clientStats !== undefined && clientStats.configured !== false && clientError === null;
  const clientSource = useMemo(
    () =>
      /*
       * ★ 第二个参数**必须**传 `"trae-client"`：两本账形状一致、只有来源不同，
       * 而来源在账本视图里**是颜色语义**（客户端青绿 / 网关靛蓝）——
       * 缺省值 `"trae-gateway"` 会把客户端这本账染成网关色，图例与柱子当场撒谎。
       */
      clientReady && clientStats ? toTokenStatsSource(clientStats, "trae-client") : null,
    [clientReady, clientStats],
  );

  /**
   * ★ 账本（issue #63 平台补齐）：网关**逐日落库**的逐日 × 逐模型用量。
   *
   * 与 `stats`（网关日志的实时窗口）是**同一本账的两个观测点**：
   * - 日志窗口：`log_keep` 条（缺省 200），只覆盖最近一段，滚过去就没了；
   * - 账本：后台每 2 分钟增量累加、保留 90 天 ⇒ **窗口一长只有它画得出来**。
   *
   * 口径优先级：账本有数据就用账本；**还没有任何落库日**（刚升级 / 网关从未启用）时
   * 回落到日志窗口，并在说明条里逐字写清「当前显示的是日志窗口」——
   * 回落本身不是问题，**不说明**才是。
   */
  const [dim, setDim] = useState<GatewayUsageDim | null>(null);
  useEffect(() => {
    let disposed = false;
    api
      .getTraeGatewayUsageDim(tokenScope)
      .then((result) => {
        if (!disposed) setDim(result);
      })
      .catch(() => {
        // 账本取不到只让这一层缺席，不阻塞日志窗口那一半（与 CreditsPage 同一取舍）。
        if (!disposed) setDim(null);
      });
    return () => {
      disposed = true;
    };
  }, [tokenScope]);

  /** 映射成共用形状后，下面全部走与 WorkBuddy 同一批适配函数（见 `lib/trae-usage-charts.ts`）。 */
  const source = useMemo(() => (stats ? toTokenStatsSource(stats) : null), [stats]);
  const ledgerSource = useMemo(() => ledgerToTokenStatsSource(dim), [dim]);
  /** 只客户端一本（`ledger === "client"`）：此时不再看网关，也不读日志窗口。 */
  const clientBook = showsClient(ledger) && !showsGateway(ledger);
  /** 账本还没落库时回落日志窗口（网关口径，见上一条注释）。 */
  const fromLedger = showsGateway(ledger) && (dim?.days.length ?? 0) > 0;
  /**
   * 当前「这一屏的账本」用的是哪份数据。
   *
   * ⚠️ `all` 档取的是**网关**这份：两本账相加会双计（同一请求可能两处各记一次，
   * 而 Trae 没有对账接口能扣掉重叠）⇒ 统计卡与模型类图表一律单本口径，
   * 「两本并排」只出现在逐日柱上（见 `ledgerColumns`）。
   */
  const bookSource = clientBook ? clientSource : fromLedger ? ledgerSource : source;

  /** 客户端账的逐日列（窗口内）。 */
  const clientColumns = useMemo(
    () => (clientSource ? dailyModelSeries(clientSource, viewWindow) : []),
    [clientSource, viewWindow],
  );
  /** 网关账的逐日列 —— 覆盖条**恒**读它（覆盖条描述数据现状，不随账本切换）。 */
  const gatewayColumns = useMemo(() => {
    const gateway = fromLedger ? ledgerSource : source;
    return gateway ? dailyModelSeries(gateway, viewWindow) : [];
  }, [fromLedger, ledgerSource, source, viewWindow]);
  /** 这一屏的逐日 × 逐模型列（统计卡 / 累计曲线 / 峰值都按它算）。 */
  const bookColumns = clientBook ? clientColumns : gatewayColumns;
  const slices = useMemo(
    () => (bookSource ? modelSlices(bookSource, viewWindow) : []),
    [bookSource, viewWindow],
  );
  /* 热力图只认「周 × 小时」槽位，网关账本按日聚合 ⇒ 账本口径下**矩阵为空**（不画，不编）。
     客户端明文库若有 `hours` 则照常画。 */
  const matrix = useMemo(
    () => (!fromLedger && bookSource ? heatmapMatrix(bookSource.hours) : null),
    [fromLedger, bookSource],
  );

  /** 窗口内的账本覆盖（未记录日 ≠ 0，必须披露）。 */
  const coverage = useMemo(
    () => dimCoverage(dim, viewWindow),
    [dim, viewWindow],
  );

  /** 窗口内网关 token 合计（账本口径与日志窗口口径**各算各的**，不混）。 */
  const windowGatewayTokens = useMemo(
    () => gatewayColumns.reduce((sum, column) => sum + column.total, 0),
    [gatewayColumns],
  );

  /**
   * 窗口内客户端账合计（token / 记录条数）。
   *
   * 只在 `clientReady` 时才有意义；**不可得时调用点传「不可得」而不是 0**（见覆盖条）。
   */
  const clientWindow = useMemo(() => {
    if (!clientSource) return { tokens: 0, records: 0 };
    const days = clientDailyInWindow(clientSource, viewWindow);
    return {
      tokens: days.reduce((sum, day) => sum + day.total, 0),
      records: days.reduce((sum, day) => sum + day.records, 0),
    };
  }, [clientSource, viewWindow]);

  /**
   * `all` 档的逐日柱：**客户端 / 网关两段并排**，柱高不做任何扣减。
   *
   * 为什么不做统计卡相加：两本账的重叠量在 Trae 上算不出来（没有对账接口）⇒
   * 相加得到的「合计」必然偏高，且无法解释高多少。这里只把两条事实**并排**摆出来，
   * 由用户自己读（与 `ledgerDailySeries` 的契约一致：一个数字只属于一本账）。
   */
  const ledgerColumns = useMemo(() => {
    if (ledger !== "all") return null;
    const clientTotals = new Map(
      (clientSource ? clientDailyInWindow(clientSource, viewWindow) : []).map(
        (day) => [day.key, day.total] as const,
      ),
    );
    const gatewayTotals = new Map(gatewayColumns.map((column) => [column.day, column.total] as const));
    return ledgerDailySeries({
      days: usageWindowKeys(viewWindow).map((key) => ({ key, total: 0 })),
      clientTotals,
      gatewayTotals,
      ledger,
      labelOf: (key) => key.slice(5).replace("-", "/"),
    });
  }, [ledger, clientSource, gatewayColumns, viewWindow]);

  /** 窗口内的活跃账号（账本按 `accounts` 维度去重；回落时用日志窗口的账号数）。 */
  const activeAccounts = useMemo(() => {
    /* 客户端账本**没有账号维度**（读的是本机对话库，不是网关的选号记录）⇒ `null`，
       第 4 枚卡改用「模型数」（与 WB 同一枚卡），而不是拿网关的账号数冒充。 */
    if (clientBook) return null;
    if (!fromLedger || !dim) return stats?.accounts.length ?? 0;
    const inWindow = new Set(usageWindowKeys(viewWindow));
    const uids = new Set<string>();
    for (const day of dim.days) {
      if (!inWindow.has(day.key)) continue;
      for (const row of day.accounts) {
        const uid = row.account?.trim();
        if (row.total > 0 && uid) uids.add(uid);
      }
    }
    return uids.size;
  }, [clientBook, fromLedger, dim, viewWindow, stats]);

  /** 窗口内的请求数与失败数（`accounts` 维度合计 —— 与 token 总量同源同水位线）。 */
  const windowGatewayRequests = useMemo(() => {
    if (!dim) return 0;
    const inWindow = new Set(usageWindowKeys(viewWindow));
    return dim.days
      .filter((day) => inWindow.has(day.key))
      .reduce(
        (sum, day) => sum + day.accounts.reduce((inner, row) => inner + row.requests, 0),
        0,
      );
  }, [dim, viewWindow]);
  const windowGatewayFailed = useMemo(() => {
    if (!dim) return 0;
    const inWindow = new Set(usageWindowKeys(viewWindow));
    return dim.days
      .filter((day) => inWindow.has(day.key))
      .reduce(
        (sum, day) => sum + day.accounts.reduce((inner, row) => inner + row.failed, 0),
        0,
      );
  }, [dim, viewWindow]);

  /** 网关账本的模型表行（窗口内，token 口径；与 `slices` 同一份数据，另一个切法）。 */
  const ledgerModels = useMemo(
    () =>
      dim
        ? dimTotals(dim, viewWindow, modelKey)
            .filter((row) => row.tokens > 0)
            .map((row) => ({
              name: row.key || t("wbStats.usage.models.unknown"),
              tokens: row.tokens,
              requests: row.requests,
              failed: row.failed,
            }))
        : [],
    [dim, viewWindow, t],
  );

  /** 客户端账本的模型表行（窗口内；与 `clientWindow.tokens` 同一分母）。 */
  const clientModels = useMemo(
    () => (clientSource ? clientModelTotals(clientSource, viewWindow) : []),
    [clientSource, viewWindow],
  );

  /* ⚠️ 加载 / 失败态不带页头与页容器：页头由上层 `UsagePage` 统一渲染。 */
  if (loading && !stats) {
    return (
      <div className="min-w-0 space-y-6" aria-busy="true">
        {slow ? <SlowLoadHint onRetry={() => void run(() => refresh())} retrying={busy} /> : null}
        <Skeleton className="h-[74px] w-full" />
        <Skeleton className="h-64 w-full" />
        <Skeleton className="h-40 w-full" />
      </div>
    );
  }

  if (error && !stats) {
    return (
      <div className="min-w-0">
        <Alert variant="destructive">
          <AlertTriangle />
          <AlertTitle>{t("trae.stats.token.loadFailed")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-3">
            <span>{error}</span>
            <div>
              <Button variant="outline" size="sm" disabled={busy} onClick={() => void run(() => refresh())}>
                {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                {t("trae.stats.token.retry")}
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  const summary = stats?.summary;
  const unsupported = stats?.unsupported ?? [];
  /* 空窗口判据随账本走：客户端看窗口内有没有量；网关看落库量（无落库时看日志记录数）；
     `all` 两本都空才算空 —— 否则「客户端有量、网关没量」会被一句「没有记录」盖掉。 */
  const gatewayEmpty = fromLedger ? windowGatewayTokens === 0 : (summary?.records ?? 0) === 0;
  const empty = clientBook
    ? clientWindow.tokens === 0
    : ledger === "all"
      ? gatewayEmpty && clientWindow.tokens === 0
      : gatewayEmpty;
  /** 客户端这本账现在需要用户「指路」（未配置 / 读失败）⇒ 面板就是这一屏的正文。 */
  const needsImport = clientError !== null || !clientReady;

  return (
    <div className="min-w-0 space-y-6">
      {/* 数据源边界：不写清楚，用户会把「数字小」当成 bug。**必须保留**（提示条，非内容块）。
          ⚠️ 例外：客户端账需要「指路」时，下面的导入面板本身就是这一屏的正文（自带标题与
          说明）⇒ 这里不再重复一条一模一样的提示（2026-10-28 实测截图里两段文案叠在一起）。 */}
      {!(clientBook && needsImport) && (
        <Alert>
          <Info />
          <AlertTitle>
            {ledger === "all"
              ? t("wbStats.usage.ledger")
              : clientBook
                ? t("trae.stats.token.client.title")
                : fromLedger
                  ? t("trae.stats.token.ledgerTitle")
                  : t("trae.stats.token.sourceTitle")}
          </AlertTitle>
          <AlertDescription>
            <span className="break-all">
              {ledger === "all" ? (
                <>
                  {/* 「全部」档：两本账**部分重叠**（客户端副本含全部对话；网关只含经本机转发的，
                      其中还可能有第三方直调网关、不在客户端库里）。重叠量在 Trae 上算不出来
                      ⇒ 既不合并也不扣减。**完整解释放在这条提示条**（图注 `ledgerStackedCaption`
                      里的「见上方说明」指的就是这里），图注只讲「怎么读」。
                      ⚠️ 分两行而不是用「：」拼接：全角冒号在英文界面里是错的，中英都要读顺。 */}
                  <b className="block font-medium text-foreground">
                    {t("trae.stats.token.noMerge")}
                  </b>
                  <span className="mt-0.5 block">{t("trae.stats.token.noMergeBody")}</span>
                </>
              ) : clientBook ? (
                (clientStats?.note || t("trae.stats.token.client.body"))
              ) : fromLedger ? (
                t("trae.stats.token.ledgerBody")
              ) : (
                (stats?.note ?? t("trae.stats.token.sourceFallback"))
              )}
            </span>
            {!fromLedger && !clientBook && stats?.logFile && (
              <span className="mt-1 block break-all font-mono text-xs text-muted-foreground">
                {stats.logFile}
              </span>
            )}
          </AlertDescription>
        </Alert>
      )}

      {/* ★ 账本覆盖条：两本账各有多少、网关覆盖了窗口里的几天。
          客户端那一项**按实况**给：读到了就给真数（含 0），未配置 / 读失败才写「不可得」——
          这是「测不到」与「没用」的唯一区分点，不能写成常量。 */}
      <CoverageStrip
        clientTokens={needsImport ? 0 : clientWindow.tokens}
        clientRequests={needsImport ? 0 : clientWindow.records}
        gatewayTokens={windowGatewayTokens}
        gatewayRequests={windowGatewayRequests}
        gatewayFailed={windowGatewayFailed}
        recorded={coverage.recorded}
        windowDays={coverage.total}
        overlapRows={0}
        overlapTokens={0}
        /* ★ 条件而不是常量：Trae 没有逐条对账（`get_usage_reconcile` 仅 WB 有），
           所以重叠量恒不显示；而客户端「有没有读到」是能回答的，必须回答。 */
        clientUnavailable={needsImport}
        showOverlap={false}
        className="-mt-2"
      />

      {clientBook && (needsImport || empty) ? (
        /* 只客户端一本时，导入面板就是这一屏的正文：没指路 / 读不出来，用户的下一步都在卡里。
           已读到但这段窗口**没量**是另一件事 —— 先给空态（含出口），再让面板承担「换一份库」。 */
        <div className="space-y-6">
          {!needsImport && empty ? (
            <EmptyState
              icon={Server}
              title={t("trae.stats.token.ledgerEmptyTitle")}
              description={t("trae.stats.token.client.noUsageHint")}
              action={
                <Button size="sm" variant="outline" onClick={() => void refreshClient()}>
                  {t("trae.stats.token.refresh")}
                </Button>
              }
            />
          ) : null}
          <TraeClientImport
            window={viewWindow}
            error={clientError}
            onImported={() => void refreshClient()}
            onRetry={() => void refreshClient()}
          />
        </div>
      ) : empty ? (
        /* 空窗口：原型每个视图都有自己的空态块（四要素含出口）。 */
        <EmptyState
          icon={Server}
          title={fromLedger ? t("trae.stats.token.ledgerEmptyTitle") : t("trae.stats.token.emptyTitle")}
          description={
            fromLedger ? t("trae.stats.token.ledgerEmptyBody") : t("trae.stats.token.emptyBody")
          }
          action={
            <Button size="sm" variant="outline" onClick={() => void refresh()}>
              {t("trae.stats.token.refresh")}
            </Button>
          }
        />
      ) : view === "models" ? (
        <>
          <ModelsView slices={slices} onRefresh={() => void refresh()} />
          {/* 两本账**各一张表**（分母各用各的）—— `all` 档下不加成一张混合表。 */}
          {showsClient(ledger) && !needsImport && clientModels.length > 0 ? (
            <LedgerModelTable
              book="client"
              rows={clientModels}
              total={clientWindow.tokens}
              caption={`${t("trae.stats.token.client.title")} · ${compactTokens(clientWindow.tokens)}`}
            />
          ) : null}
          {showsGateway(ledger) && fromLedger && ledgerModels.length > 0 ? (
            <LedgerModelTable
              book="gateway"
              rows={ledgerModels}
              total={windowGatewayTokens}
              caption={t("wbStats.usage.models.gatewayCaption")}
              failed={windowGatewayFailed}
            />
          ) : null}
        </>
      ) : view === "keys" ? (
        /* 「按密钥」只对网关这本账有意义（Key 是网关发出去的）⇒ 恒读网关账本维度。 */
        <KeysView
          dim={dim}
          window={viewWindow}
          selfScope="trae"
          loadNames={loadTraeKeyNames}
        />
      ) : (
        <TrendView
          columns={ledgerColumns ?? bookColumns}
          bookColumns={bookColumns}
          slices={slices}
          matrix={matrix}
          accounts={activeAccounts}
          viewWindow={viewWindow}
          scopeName={combineAll ? t("shared.region.combine.all") : scopeLabel(t, scope)}
          fromLedger={fromLedger}
          stackedByLedger={ledgerColumns !== null}
        />
      )}

      {/* `all` 档下客户端读不到时，横幅不能代替「怎么修」：面板放在页末（不挡住网关那本）。 */}
      {ledger === "all" && needsImport ? (
        <TraeClientImport
          window={viewWindow}
          error={clientError}
          onImported={() => void refreshClient()}
          onRetry={() => void refreshClient()}
        />
      ) : null}

      {/* ---- 平台做不到的维度（置灰卡；形状来自 handlers::unsupported_note）----
          不是原型的内容块，而是**诚实性要求**：Trae 没有「按项目 / 会话」维度，
          不说清楚用户会以为「统计漏了」。 */}
      {unsupported.length > 0 && <UnsupportedSection items={unsupported} />}

      {/* 日志解析失败：**诚实性提示** —— 数字不完整时必须说（不是原型的内容块，
          但与 `unsupported` 同类：不说明会让用户把「少算」当成 bug）。
          ⚠️ 这是**网关日志**的解析结果 ⇒ 只看客户端那本账时不出现。 */}
      {showsGateway(ledger) && stats && stats.parseErrors > 0 && (
        <p className="flex items-center gap-1.5 px-1 text-xs text-warning">
          <AlertTriangle className="size-3.5" aria-hidden="true" />
          {t("trae.stats.token.parseErrors", { count: stats.parseErrors })}
        </p>
      )}
    </div>
  );
}

/* ────────────────────────────── 趋势（原型四块） ────────────────────────────── */

function TrendView({
  columns,
  bookColumns,
  slices,
  matrix,
  accounts,
  viewWindow,
  scopeName,
  fromLedger,
  stackedByLedger,
}: {
  /** 逐日柱的列。`all` 档是「客户端 / 网关」两段并排，其余档是按模型拆分。 */
  columns: ReturnType<typeof dailyModelSeries>;
  /** **单本账**的逐日 × 逐模型列：统计卡 / 累计曲线 / 峰值都按它算（`all` 档是网关那本）。 */
  bookColumns: ReturnType<typeof dailyModelSeries>;
  slices: ReturnType<typeof modelSlices>;
  matrix: ReturnType<typeof heatmapMatrix> | null;
  /** 窗口内的活跃账号；`null` = 这本账没有账号维度（客户端账本）⇒ 第 4 枚卡改用模型数。 */
  accounts: number | null;
  viewWindow: UsageWindow;
  scopeName: string;
  /** 当前画的是账本（逐日落库）还是日志窗口 —— 只影响热力图那一块的说明。 */
  fromLedger: boolean;
  /** 逐日柱是否按**账本**堆叠（`all` 档）。颜色与图注随之切换。 */
  stackedByLedger: boolean;
}) {
  const t = useT();
  const windowLabel = useUsageWindowLabel();

  const total = bookColumns.reduce((sum, column) => sum + column.total, 0);
  /* 日均的分母是**窗口天数**（不是「有数据的天数」）—— 与 WB 的 A1-3 同一口径。 */
  const average = Math.round(total / usageWindowDays(viewWindow));
  const peak = bookColumns.reduce(
    (best, column) => (column.total > best.total ? column : best),
    bookColumns[0] ?? { day: "", label: "", total: 0, segments: [] },
  );
  /* 累计曲线按**单本账**算：`all` 档的逐日柱是两本账并排，把它当累计分母就是双计。 */
  const cumulative = useMemo(() => {
    let running = 0;
    return bookColumns.map((column) => {
      running += column.total;
      return { label: column.label, value: running };
    });
  }, [bookColumns]);
  const peakDelta = bookColumns.reduce((max, column) => Math.max(max, column.total), 0);
  const heatmapHasData = matrix?.cells.some((row) => row.some((value) => value > 0)) ?? false;

  /* 四枚统计卡 = 原型 `statTile` 的区间总 Token / 日均 / 峰值 / **活跃账号**。
     ⚠️ 原型第 4 枚是「活跃账号」—— WB 侧**没有账号维度**，用「模型数」替代（已登记）；
     Trae 的**网关账本**有 `accounts` ⇒ 按原型原样用「活跃账号」；客户端账本没有这个维度
     ⇒ 换成「模型数」（与 WB 同一枚卡），而不是拿网关的账号数冒充。 */
  const tiles: StatTile[] = [
    {
      icon: MessagesSquare,
      label: t("wbStats.usage.trend.total"),
      value: compactTokens(total),
      hint: windowLabel(viewWindow),
    },
    {
      icon: TrendingUp,
      label: t("wbStats.usage.trend.avg"),
      value: compactTokens(average),
      hint: t("wbStats.usage.trend.avgHint"),
    },
    {
      icon: Gauge,
      label: t("wbStats.usage.trend.peak"),
      value: compactTokens(peak.total),
      hint: peak.label || undefined,
    },
    accounts === null
      ? {
          icon: Server,
          label: t("wbStats.usage.models.count"),
          value: exactTokens(slices.length),
          hint: scopeName,
        }
      : {
          icon: Users,
          label: t("trae.stats.token.summary.activeAccounts"),
          value: exactTokens(accounts),
          hint: scopeName,
        },
  ];

  return (
    <>
      <StatTiles tiles={tiles} ariaLabel={t("trae.stats.token.summaryAria")} />

      {/* `all` 档的四枚卡只算**网关**一本（两本相加会双计，重叠量在 Trae 上算不出来）——
          这句话必须留在卡下面，否则用户会把「客户端没被算进来」当成 bug。 */}
      {stackedByLedger ? (
        <p className="-mt-3 px-1 text-[11.5px] leading-5 text-muted-foreground">
          {t("wbStats.usage.trend.totalGatewayTitle")}
        </p>
      ) : null}

      {columns.length > 0 ? (
        <ChartFrame
          title={
            stackedByLedger
              ? t("trae.stats.token.ledgerStackedTitle")
              : t("wbStats.usage.trend.stackedTitle")
          }
          caption={
            stackedByLedger
              ? t("trae.stats.token.ledgerStackedCaption")
              : t("wbStats.usage.trend.stackedDesc")
          }
          ariaLabel={
            stackedByLedger
              ? t("trae.stats.token.ledgerStackedAria")
              : t("wbStats.usage.trend.stackedAria")
          }
        >
          {/* 颜色本身是语义：客户端青绿、网关靛蓝（与覆盖条、账本表同一套）。
              账本段由 `StackedDailyBar` **按段名**固定取色（不按出现顺序）——
              这里不再传 `palette`，否则会出现两套并存的取色逻辑。 */}
          <StackedDailyBar columns={columns} />
        </ChartFrame>
      ) : null}

      <div className="grid min-w-0 gap-6 lg:grid-cols-2">
        {cumulative.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.trend.cumulativeTitle")}
            caption={cumulativeCaption(t, cumulative[cumulative.length - 1].value, peakDelta)}
            ariaLabel={t("wbStats.usage.trend.cumulativeTitle")}
          >
            <CumulativeArea points={cumulative} />
          </ChartFrame>
        ) : null}
        {slices.length > 0 ? (
          <ChartFrame
            title={t("wbStats.usage.trend.donutTitle")}
            caption={`${exactTokens(slices.length)} · ${compactTokens(total)}`}
            ariaLabel={t("wbStats.usage.trend.donutTitle")}
          >
            <ModelDonut slices={slices} size={150} />
          </ChartFrame>
        ) : null}
      </div>

      {heatmapHasData && matrix ? (
        <ChartFrame
          title={t("wbStats.usage.trend.heatmapTitle")}
          caption={t("wbStats.usage.trend.heatmapDesc")}
          ariaLabel={t("wbStats.usage.trend.heatmapAria")}
          exportable={false}
        >
          <UsageHeatmap matrix={matrix} />
        </ChartFrame>
      ) : fromLedger ? (
        /* 账本按**日**聚合，没有「周 × 小时」槽位 ⇒ 这张图在账本口径下画不出来。
           不画空图，也不静默消失：一行字说明它为什么不在这里。 */
        <p className="px-1 text-xs text-muted-foreground">
          {t("trae.stats.token.heatmapLedgerUnavailable")}
        </p>
      ) : null}
    </>
  );
}

/* ────────────────────────────── 模型（原型三块） ────────────────────────────── */

function ModelsView({
  slices,
  onRefresh,
}: {
  slices: ReturnType<typeof modelSlices>;
  onRefresh: () => void;
}) {
  const t = useT();
  const total = slices.reduce((sum, slice) => sum + slice.value, 0);
  const top = slices[0];

  if (slices.length === 0) {
    /* 原型 `emptyBlock(ICON.usage, "暂无模型用量", …)` —— 出口由 §3.6 补上。 */
    return (
      <EmptyState
        icon={Server}
        title={t("wbStats.usage.empty.models")}
        description={t("wbStats.usage.empty.modelsHint")}
        action={
          <Button size="sm" variant="outline" onClick={onRefresh}>
            {t("wbStats.usage.empty.refresh")}
          </Button>
        }
      />
    );
  }

  const tiles: StatTile[] = [
    { icon: Server, label: t("wbStats.usage.models.count"), value: exactTokens(slices.length) },
    {
      icon: Sparkles,
      label: t("wbStats.usage.models.top"),
      value: top?.name ?? "—",
      hint: top ? `${((top.value / total) * 100).toFixed(0)}%` : undefined,
    },
    { icon: MessagesSquare, label: t("wbStats.usage.models.total"), value: compactTokens(total) },
    { icon: Gauge, label: t("wbStats.usage.models.avg"), value: compactTokens(Math.round(total / slices.length)) },
  ];

  return (
    <>
      <StatTiles tiles={tiles} ariaLabel={t("wbStats.usage.view.models")} />
      <ChartFrame
        title={t("wbStats.usage.models.donutTitle")}
        caption={`${exactTokens(slices.length)} · ${compactTokens(total)}`}
        ariaLabel={t("wbStats.usage.models.donutTitle")}
      >
        <ModelDonut slices={slices} />
      </ChartFrame>
      <ChartFrame
        title={t("wbStats.usage.models.rankTitle")}
        caption={t("wbStats.usage.models.rankDesc")}
        ariaLabel={t("wbStats.usage.models.rankTitle")}
        exportable={false}
      >
        <ModelRanking slices={slices} />
      </ChartFrame>
    </>
  );
}

/* ────────────────────────────── 平台做不到的维度 ────────────────────────────── */

function UnsupportedSection({ items }: { items: TraeUnsupported[] }) {
  const t = useT();
  return (
    <Card className="min-w-0 gap-0 overflow-hidden rounded-lg border-dashed py-0 shadow-none">
      <CardHead title={t("trae.stats.token.unsupportedTitle")} />
      <div className="divide-y divide-border/60">
        {items.map((item) => (
          <div key={item.capability} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-4 py-3 opacity-70">
            <span className="text-sm font-medium text-muted-foreground">{item.label}</span>
            <span className="text-xs text-muted-foreground">
              {t("trae.stats.token.unsupportedOnly", { on: item.supportedOn })}
            </span>
            <span className="w-full text-xs leading-5 text-muted-foreground">{item.reason}</span>
          </div>
        ))}
      </div>
    </Card>
  );
}
