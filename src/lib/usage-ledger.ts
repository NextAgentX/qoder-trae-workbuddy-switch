/**
 * 用量页 · **账本适配层**（issue #63，纯函数）。
 *
 * 设计依据：`docs/specs/USAGE-GATEWAY-UI-REDESIGN.md`。
 *
 * ## 这个文件解决什么
 *
 * 客户端与网关是**两本独立记账的账**：互相重叠、口径不同、各有各的维度。
 * 页面必须让用户看得出「这个数属于哪本账」，且**绝不跨账本混算占比**。
 *
 * 三条规则（改这里之前先读）：
 *
 * 1. **一个数字只属于一本账**：占比 / 排行 / 峰值一律在单本账内算；
 * 2. **跨账本合计必须可审计**：只出现在「合计」卡，且必须能展开成算式；
 * 3. **重叠不是第三本账**：它只以数字出现（`重叠 9 条`），不给颜色、不画图。
 *
 * ## 为什么单独一层
 *
 * 「图不能撒谎」的契约必须落在**能单测的纯函数**里，而不是散在 JSX 中：
 * 渲染层测不了「按模型合计 = 网关卡」，这一层可以。
 *
 * 本模块**不导入 React**（只有 type 导入 + 零依赖工具），可被门禁直接求值。
 */

import type {
  GatewayDimEntry,
  GatewayUsageDim,
  GatewayUsageRecon,
  TokenStatsGroup,
  TokenStatsSource,
} from "@/lib/types";
import { usageWindowKeys, type UsageWindow } from "@/lib/usage-window";

/** 账本三态（与参数条的分段控件一一对应）。 */
export type UsageLedger = "all" | "client" | "gateway";

export const LEDGER_VALUES: readonly UsageLedger[] = ["all", "client", "gateway"];

/**
 * 解析 URL / 偏好里的账本值。
 *
 * 未知值 ⇒ `"all"`（**不是** `"client"`）：默认「全部」是设计拍板的选择
 * ——用户第一眼要的是规模，而合计已经可对账（见设计文档 §8）。
 */
export function parseLedger(raw: string | null | undefined): UsageLedger {
  return raw === "client" || raw === "gateway" || raw === "all" ? raw : "all";
}

/** 窗口内的日期键集合（与 `dailyModelSeries` 同源，保证切片口径一致）。 */
function windowKeySet(window: UsageWindow): Set<string> {
  return new Set(usageWindowKeys(window));
}

/**
 * 把某个维度在窗口内**逐键求和**。
 *
 * `keyOf` 取键（模型 / 账号 / 端点），空键保留为 `""`（渲染成「未记录」）——
 * **不丢量**：丢掉它会让「按 X 合计」小于网关总量，两条口径当场分叉。
 */
export function dimTotals(
  dim: GatewayUsageDim | null,
  window: UsageWindow,
  keyOf: (entry: GatewayDimEntry) => string | undefined,
): DimTotal[] {
  if (!dim) return [];
  const inWindow = windowKeySet(window);
  const map = new Map<string, DimTotal>();
  for (const day of dim.days) {
    if (!inWindow.has(day.key)) continue;
    for (const entry of day.models.concat(day.accounts, day.endpoints, day.keys)) {
      // 四个数组共用形状，但每个数组只该被读一次：用 keyOf 的 undefined 过滤掉别的维度。
      const key = keyOf(entry);
      if (key === undefined) continue;
      const current =
        map.get(key) ?? { key, tokens: 0, input: 0, cached: 0, requests: 0, failed: 0, credit: 0 };
      current.tokens += entry.total;
      current.input += entry.input;
      /* ★ issue #154：`null`（没观测到）不参与求和 —— 把 null 当 0 求和与
         「确实一次没命中」在结果上无法区分，而后者会让命中率被错误地算低。
         只要窗口内有一天给出过值，这个按钮聚合就有值。 */
      current.cached += entry.cached ?? 0;
      current.requests += entry.requests;
      current.failed += entry.failed;
      current.credit += entry.credit;
      map.set(key, current);
    }
  }
  return [...map.values()].sort((left, right) => right.tokens - left.tokens);
}

/** `dimTotals` 的一行（某维度键在窗口内的合计）。 */
export interface DimTotal {
  key: string;
  /** 输入 + 输出（与「网关总量」同口径）。 */
  tokens: number;
  /** 输入侧合计 —— 命中率的分母。 */
  input: number;
  /** 命中上游前缀缓存的输入 token 合计（`input` 的子集；0 = 窗口内没观测到）。 */
  cached: number;
  requests: number;
  failed: number;
  credit: number;
}

/**
 * 网关在窗口内的**命中率**（`cached / input`）。
 *
 * ## 三条口径
 *
 * 1. **分母是 `input` 不是 `total`**：命中只发生在输入侧，拿 input+output 当分母
 *    会随输出变长而系统性低估命中率；
 * 2. **`cached` 是 `input` 的子集，绝不相加**（issue #154 的原始 bug 就是重复计入）；
 * 3. `input === 0` ⇒ 返回 `null`（**不是 0%**）：那天没有可判定的输入。
 *
 * `observed` 为 `false` 表示窗口内**一条命中数据都没有**（老账本 / 上游从不报）——
 * 与「命中率 0%」必须分开表述，否则用户会去查一个不存在的缓存问题。
 */
export function gatewayCacheHit(
  dim: GatewayUsageDim | null,
  window: UsageWindow,
): { hitRate: number | null; input: number; cached: number; observed: boolean } {
  const empty = { hitRate: null, input: 0, cached: 0, observed: false };
  if (!dim) return empty;
  const inWindow = windowKeySet(window);
  let input = 0;
  let cached = 0;
  let observed = false;
  for (const day of dim.days) {
    if (!inWindow.has(day.key)) continue;
    /* ⚠️ **只选一条维度**求和：四条维度由同一次扫描产出、合计相同，混着加会把
       同一个请求算四遍（命中率会翻成 4 倍这种离谱值）。
       选 `models` 而不是 `endpoints`：端点维度的 `input` 在部分历史数据里是 0
       （它当初只为「请求数分布」而生），拿它当分母会把命中率算成 null。 */
    for (const entry of day.models) {
      input += entry.input;
      if (entry.cached != null) {
        cached += entry.cached;
        observed = true;
      }
    }
  }
  if (input === 0) return { ...empty, observed };
  /* ★ 一条命中数据都没有时**必须**给 null：此时 `cached` 恒为 0，
    直接除会得到「0% 命中」—— 与「确实全 miss」在界面上无法区分，
    而那两句提示会把用户送去查一个并不存在的缓存问题（issue #154 的教训）。 */
  if (!observed) return { hitRate: null, input, cached: 0, observed };
  return { hitRate: cached / input, input, cached, observed };
}

/** 模型维度（`models[]` 的键）。 */
export const modelKey = (entry: GatewayDimEntry) => entry.model;
/** 账号维度（`accounts[]` 的键）。 */
export const accountKey = (entry: GatewayDimEntry) => entry.account;
/** 端点维度（`endpoints[]` 的键）。 */
export const endpointKey = (entry: GatewayDimEntry) => entry.endpoint;
/** 密钥维度（`keys[]` 的键：`{scope}:{id}` / `__legacy__` / `__none__`）。 */
export const keyBucket = (entry: GatewayDimEntry) => entry.key;

/**
 * 网关维度的**覆盖率**（与 `gatewayCoverage` 同构，图注披露用）。
 *
 * - `recorded`：窗口内**有落库记录**的天数（含量为 0 的天 —— 那是真实 0）；
 * - `truncated`：其中有几天标了「可能被裁」（那些天的值是**下界**）；
 * - `partial`：维度拆分不完整的日数（v2 → v3 升级播种）；
 * - `total`：窗口天数。
 */
export function dimCoverage(
  dim: GatewayUsageDim | null,
  window: UsageWindow,
): { recorded: number; total: number; truncated: number; partial: number } {
  const keys = usageWindowKeys(window);
  if (!dim) return { recorded: 0, total: keys.length, truncated: 0, partial: 0 };
  const byKey = new Map(dim.days.map((day) => [day.key, day]));
  let recorded = 0;
  let truncated = 0;
  let partial = 0;
  for (const key of keys) {
    const day = byKey.get(key);
    if (!day) continue;
    recorded += 1;
    if (day.truncated) truncated += 1;
    if (day.partial) partial += 1;
  }
  return { recorded, total: keys.length, truncated, partial };
}

/** 窗口内客户端逐日 token（与 `source.daily` 同源同窗）。 */
export function clientDailyInWindow(
  source: TokenStatsSource,
  window: UsageWindow,
): { key: string; total: number; records: number }[] {
  const inWindow = windowKeySet(window);
  return source.daily.filter((day) => inWindow.has(day.key)).map((day) => ({
    key: day.key,
    total: day.total,
    records: day.records,
  }));
}

/** 窗口内网关逐日 token（来自 `get_gateway_token_trend`）。 */
export function gatewayDailyInWindow(
  trend: { days: { key: string; total: number }[] } | null,
  window: UsageWindow,
): { key: string; total: number }[] {
  const inWindow = windowKeySet(window);
  return (trend?.days ?? []).filter((day) => inWindow.has(day.key)).map((day) => ({
    key: day.key,
    total: day.total,
  }));
}

/**
 * 合计卡的**算式**（结构化成三段，由 i18n 拼文案）。
 *
 * ## 为什么形态随拓扑变
 *
 * 客户端走网关时（`subset`），若仍写「相加再扣」，式子会变成
 * `6.51M + 28.5M − 6.16M` —— 扣掉了客户端账本的 95%，读起来像哪里错了。
 * 换成「网关 + 直连客户端」才符合事实：网关已经记全了，客户端只剩没走网关的那几条。
 *
 * ## 返回值为什么不是一个字符串
 *
 * 文案要 i18n，而「谁在前、用什么连接词」在中文/英文里不一样。这里只给**事实**
 * （三个数 + 形态），拼句子交给 `chart-caption`。
 */
export function combinedFormula(input: {
  clientTokens: number;
  gatewayTokens: number;
  overlapTokens: number;
  verdict: GatewayUsageRecon["verdict"] | null;
}): { kind: "subset" | "additive"; directClientTokens: number; overlapTokens: number } {
  return {
    kind: input.verdict === "subset" ? "subset" : "additive",
    directClientTokens: Math.max(0, input.clientTokens - input.overlapTokens),
    overlapTokens: input.overlapTokens,
  };
}

/**
 * 合计值（`C + G − 重叠`）。
 *
 * ⚠️ 两个形态**数学上等价**：`G + (C − overlap) === C + G − overlap`。
 * 形态只影响**怎么读**，不影响数。
 */
export function combinedTotal(clientTokens: number, gatewayTokens: number, overlapTokens: number): number {
  return Math.max(0, clientTokens + gatewayTokens - overlapTokens);
}

/**
 * 窗口内的重叠量。
 *
 * 对账只覆盖**今日**（网关日志是环形窗口），所以窗口内更早的日子按
 * 「客户端在窗口内的占比」缩放 —— 这是**估计**，且只在窗口跨多天时生效。
 * 单日窗口（今天）时 `scale = 1`，即对账原值。
 */
export function overlapInWindow(recon: GatewayUsageRecon | null, clientTokensInWindow: number): number {
  if (!recon || recon.clientTokens === 0) return 0;
  const scale = Math.min(1, clientTokensInWindow / recon.clientTokens);
  return Math.round(recon.matchedTokens * scale);
}

/**
 * 账本对某个视图的**可见性**：只有「全部」时才需要展示两本账。
 *
 * 用一个函数而不是到处写 `ledger !== "client"`：那类条件散开后，
 * 总有一处会写成 `!== "gateway"`（方向反了，且不报错）。
 */
export function showsClient(ledger: UsageLedger): boolean {
  return ledger !== "gateway";
}
export function showsGateway(ledger: UsageLedger): boolean {
  return ledger !== "client";
}

/** 客户端按模型的窗口内合计（`modelDays` 逐日求和，与每日总量严格同源）。 */
export function clientModelTotals(
  source: TokenStatsSource,
  window: UsageWindow,
): { name: string; tokens: number; requests: number }[] {
  const keys = usageWindowKeys(window);
  const inWindow = new Set(keys);
  return Object.entries(source.dailyByModel ?? {})
    .map(([name, points]) => {
      let tokens = 0;
      let requests = 0;
      for (const point of points) {
        if (!inWindow.has(point.key)) continue;
        tokens += point.total;
        requests += point.records;
      }
      return { name, tokens, requests };
    })
    .filter((model) => model.tokens > 0)
    .sort((left, right) => right.tokens - left.tokens);
}

/** 双色堆叠柱的一列（客户端 + 网关两段）。 */
export interface LedgerColumn {
  day: string;
  label: string;
  total: number;
  segments: { name: string; value: number }[];
}

/**
 * 趋势 · **按账本**堆叠（不是按模型）。
 *
 * ## 为什么趋势图不再按模型拆
 *
 * 堆叠维度只能有一个。按账本拆回答的是 issue #63 的核心问题
 * （「网关那 6 倍到底在哪」）；按模型拆的每日走势挪到「模型」tab
 * （`ModelDailyChart`），两处各答一个问题 —— 两级堆叠在深色小图里没人读得出来。
 *
 * ## 契约
 *
 * 每列 `segments` 之和 === `total`（末段吃舍入余数，与 `dailyModelSeries` 同手法）；
 * 某段为 0 时**不产该段**（否则图例里会多出永远为 0 的系列）。
 */
export function ledgerDailySeries(input: {
  days: { key: string; total: number }[];
  clientTotals: Map<string, number>;
  gatewayTotals: Map<string, number>;
  ledger: UsageLedger;
  labelOf: (key: string) => string;
}): LedgerColumn[] {
  const showClient = showsClient(input.ledger);
  const showGateway = showsGateway(input.ledger);
  return input.days.map((day) => {
    const client = showClient ? (input.clientTotals.get(day.key) ?? 0) : 0;
    const gateway = showGateway ? (input.gatewayTotals.get(day.key) ?? 0) : 0;
    const segments: { name: string; value: number }[] = [];
    if (client > 0) segments.push({ name: LEDGER_CLIENT_SEGMENT, value: client });
    if (gateway > 0) segments.push({ name: LEDGER_GATEWAY_SEGMENT, value: gateway });
    return { day: day.key, label: input.labelOf(day.key), total: client + gateway, segments };
  });
}

/** 堆叠柱的**账本段名**（渲染层翻成「客户端 / 网关」）。 */
export const LEDGER_CLIENT_SEGMENT = "__ledger_client__";
export const LEDGER_GATEWAY_SEGMENT = "__ledger_gateway__";

/** 积分：请求口径下某本账的逐日消耗（`credit`）。 */
export function creditDailySeries(
  dim: GatewayUsageDim | null,
  window: UsageWindow,
): { key: string; credit: number }[] {
  if (!dim) return [];
  const inWindow = windowKeySet(window);
  return dim.days
    .filter((day) => inWindow.has(day.key))
    .map((day) => ({
      key: day.key,
      credit: day.models.reduce((sum, model) => sum + model.credit, 0),
    }));
}

/**
 * 积分：**两个口径的差额**。
 *
 * - `billed`：余额口径（账号余额快照差值）—— 权威、**不重复**、拆不到来源；
 * - `byRequest`：请求口径（每条请求的 `credit` 累加）—— 能拆来源、**会重复**；
 * - `gap = billed − byRequest`：**未归因**消耗（别的设备 / 没落本地日志的请求）。
 *
 * ⚠️ 差额**绝不摊到任何来源上** —— 摊了就是编数据（设计文档 §4.8）。
 * `billed < byRequest` 时（对账没扣干净 / 上游改了计费）差额记 0，不显示负数：
 * 负数只会让人以为「用少了还多算了」。
 */
export function creditGap(billed: number, byRequest: number): number {
  return Math.max(0, billed - byRequest);
}

/** 把 `TokenStatsGroup[]` 折成 `Map<日期, total>`（`ledgerDailySeries` 的入参）。 */
export function dailyTotalsMap(days: TokenStatsGroup[]): Map<string, number> {
  return new Map(days.map((day) => [day.key, day.total]));
}

/**
 * 合并两个 region 的三维度历史（`region === "all"` 时用）。
 *
 * 与 `mergeGatewayTrends` 同哲学：**合并作用域是前端的事**，后端命令保持单 region。
 * 同日同键求和；`truncated` / `partial` 取**或**（任一侧可能是下界就是下界）。
 */
export function mergeDims(list: (GatewayUsageDim | null)[]): GatewayUsageDim | null {
  const present = list.filter((dim): dim is GatewayUsageDim => Boolean(dim));
  if (present.length === 0) return null;
  if (present.length === 1) return present[0];

  const byDay = new Map<string, GatewayUsageDim["days"][number]>();
  const mergeEntries = (
    target: GatewayDimEntry[],
    incoming: GatewayDimEntry[],
    keyOf: (entry: GatewayDimEntry) => string | undefined,
  ) => {
    const index = new Map(target.map((entry) => [keyOf(entry), entry]));
    for (const entry of incoming) {
      const key = keyOf(entry);
      const current = index.get(key);
      if (!current) {
        target.push({ ...entry });
        index.set(key, target[target.length - 1]);
        continue;
      }
      current.input += entry.input;
      current.output += entry.output;
      current.total += entry.total;
      current.requests += entry.requests;
      current.failed += entry.failed;
      current.credit += entry.credit;
    }
  };

  for (const dim of present) {
    for (const day of dim.days) {
      const current = byDay.get(day.key);
      if (!current) {
        byDay.set(day.key, {
          key: day.key,
          truncated: day.truncated,
          partial: day.partial,
          models: day.models.map((entry) => ({ ...entry })),
          accounts: day.accounts.map((entry) => ({ ...entry })),
          endpoints: day.endpoints.map((entry) => ({ ...entry })),
          keys: day.keys.map((entry) => ({ ...entry })),
          keysPartial: day.keysPartial,
        });
        continue;
      }
      current.truncated = current.truncated || day.truncated;
      current.partial = current.partial || day.partial;
      mergeEntries(current.models, day.models, modelKey);
      mergeEntries(current.accounts, day.accounts, accountKey);
      mergeEntries(current.endpoints, day.endpoints, endpointKey);
      mergeEntries(current.keys, day.keys, keyBucket);
      // 「至少一侧是从中间开始记的」⇒ 合并后仍是下界。
      current.keysPartial = current.keysPartial || day.keysPartial;
    }
  }

  return {
    date: present[0].date,
    retentionDays: present[0].retentionDays,
    days: [...byDay.values()].sort((left, right) => left.key.localeCompare(right.key)),
  };
}

/**
 * 合并两个 region 的对账结果（`region === "all"` 时用）。
 *
 * 逐字段求和，`verdict` 由**合并后的匹配率**重算 —— 不直接取某一侧的结论：
 * 两个区域可能一个是「客户端走网关」、另一个是「独立」，合并后的结论属于整体。
 */
export function mergeRecons(list: (GatewayUsageRecon | null)[]): GatewayUsageRecon | null {
  const present = list.filter((recon): recon is GatewayUsageRecon => Boolean(recon));
  if (present.length === 0) return null;
  if (present.length === 1) return present[0];
  const sum = (pick: (recon: GatewayUsageRecon) => number) =>
    present.reduce((total, recon) => total + pick(recon), 0);

  const clientRows = sum((recon) => recon.clientRows);
  const matched = sum((recon) => recon.matched);
  const clientTokens = sum((recon) => recon.clientTokens);
  const gatewayTokens = sum((recon) => recon.gatewayTokens);
  const matchedTokens = sum((recon) => recon.matchedTokens);
  const ratio = clientRows > 0 ? matched / clientRows : 0;
  const proxySignal = present.every((recon) => recon.proxySignal === true)
    ? true
    : present.every((recon) => recon.proxySignal === false)
      ? false
      : null;

  return {
    clientRows,
    gatewayRows: sum((recon) => recon.gatewayRows),
    matched,
    matchedTokens,
    matchedCredit: sum((recon) => recon.matchedCredit),
    clientTokens,
    clientCredit: sum((recon) => recon.clientCredit),
    gatewayTokens,
    gatewayCredit: sum((recon) => recon.gatewayCredit),
    verdict: ratio < 0.1 ? "independent" : ratio > 0.9 && proxySignal !== false ? "subset" : "mixed",
    proxySignal,
    bounds: { lower: Math.max(clientTokens, gatewayTokens), upper: clientTokens + gatewayTokens },
    estimate: Math.max(0, clientTokens + gatewayTokens - matchedTokens),
    unmatchedSamples: present.flatMap((recon) => recon.unmatchedSamples).slice(0, 3),
    window: present[0].window,
  };
}

/** 客户端按模型的**积分**窗口内合计（`dailyByModel` 的 `credit` 逐日求和）。 */
export function clientCreditByModel(
  source: TokenStatsSource,
  window: UsageWindow,
): { name: string; credit: number; requests: number }[] {
  const inWindow = new Set(usageWindowKeys(window));
  return Object.entries(source.dailyByModel ?? {})
    .map(([name, points]) => {
      let credit = 0;
      let requests = 0;
      for (const point of points) {
        if (!inWindow.has(point.key)) continue;
        credit += point.credit ?? 0;
        requests += point.records;
      }
      return { name, credit, requests };
    })
    .filter((model) => model.credit > 0)
    .sort((left, right) => right.credit - left.credit);
}

/** 客户端逐日积分（`source.daily[].credit`），窗口内。 */
export function clientCreditDaily(
  source: TokenStatsSource,
  window: UsageWindow,
): Map<string, number> {
  const inWindow = new Set(usageWindowKeys(window));
  return new Map(
    source.daily.filter((day) => inWindow.has(day.key)).map((day) => [day.key, day.credit ?? 0]),
  );
}

/**
 * **网关账本 → `TokenStatsSource`**（Trae / Qoder 的 token 视图数据源）。
 *
 * ## 为什么走映射而不是另写一套图表
 *
 * 拖两张图出来（逐日堆叠 + 模型环形/排行）需要 `daily` / `dailyByModel` / `models`
 * 三件套。账本里全都有（逐日 × 逐模型），只是形状不同 —— 于是映射一次，
 * `dailyModelSeries` / `modelSlices` **一行不改地复用**（与 `lib/trae-usage-charts.ts`
 * 的 `toTokenStatsSource` 同一理由：图表契约只有一份实现）。
 *
 * ## 两处**故意留空**
 *
 * - `hours: []` —— 账本只按**日**聚合（日志里没有时段槽位），热力图因此为空。
 *   7×24 图在账本口径下画不出来，**不编**；
 * - `projects` / `sessions` —— 网关日志没有稳定的项目 / 会话标识（Trae 的
 *   `session_id` 每请求新生成），这两个维度在该口径下**本就不存在**。
 *
 * `records` 取 `requests`（请求数），与 WB 客户端口径的「记录条数」同义。
 */
export function ledgerToTokenStatsSource(dim: GatewayUsageDim | null): TokenStatsSource | null {
  if (!dim) return null;
  const dailyByModel: Record<string, TokenStatsGroup[]> = {};
  const modelTotals = new Map<string, TokenStatsGroup>();
  const daily: TokenStatsGroup[] = [];

  for (const day of dim.days) {
    let total = 0;
    let input = 0;
    let output = 0;
    let records = 0;
    let credit = 0;
    for (const row of day.models) {
      /* `GatewayDimEntry` 的三个键都可选（同一个联合形状服务三条维度）——
         这里读的是 `models[]`，缺 `model` 的行走哨兵而不是丢掉该行（丢量 = 图撒谎）。 */
      const model = row.model?.trim() || UNKNOWN_MODEL;
      total += row.total;
      input += row.input;
      output += row.output;
      records += row.requests;
      credit += row.credit;
      const point = groupOf(day.key, row.total, row.input, row.output, row.requests, row.credit);
      (dailyByModel[model] ??= []).push(point);
      const aggregate = modelTotals.get(model);
      if (aggregate) {
        aggregate.total += row.total;
        aggregate.input += row.input;
        aggregate.output += row.output;
        aggregate.records += row.requests;
        aggregate.credit = (aggregate.credit ?? 0) + row.credit;
      } else {
        modelTotals.set(
          model,
          groupOf(model, row.total, row.input, row.output, row.requests, row.credit),
        );
      }
    }
    daily.push(groupOf(day.key, total, input, output, records, credit));
  }

  const models = [...modelTotals.values()].sort((left, right) => right.total - left.total);
  const summary = daily.reduce(
    (acc, day) => ({
      ...acc,
      total: acc.total + day.total,
      input: acc.input + day.input,
      output: acc.output + day.output,
      records: acc.records + day.records,
      credit: (acc.credit ?? 0) + (day.credit ?? 0),
    }),
    groupOf("summary", 0, 0, 0, 0, 0),
  );

  return {
    source: "trae-gateway",
    summary,
    models,
    projects: [],
    sessions: [],
    daily,
    dailyByModel,
    hours: [],
    filesScanned: 0,
    parseErrors: 0,
    coverageStartAt: null,
    coverageEndAt: null,
  };
}

/** `TokenStatsGroup` 的最小构造（缓存三件套恒 0：账本没有这几个维度，不编造）。 */
function groupOf(
  key: string,
  total: number,
  input: number,
  output: number,
  records: number,
  credit: number,
): TokenStatsGroup {
  return {
    key,
    total,
    input,
    output,
    cacheRead: 0,
    cacheWrite: 0,
    uncachedInput: 0,
    records,
    cacheHitRate: null,
    credit,
  };
}

/** 账本里模型名缺失时的哨兵（与 Rust `gateway_ledger::UNKNOWN_MODEL` 同值）。 */
export const UNKNOWN_MODEL = "__unknown__";

/** 账本的**逐日总量**（`ledgerDailySeries` 的 `days` 入参：轴由账本自己的落库日给出）。 */
export function dimDailyTotals(dim: GatewayUsageDim | null): { key: string; total: number }[] {
  if (!dim) return [];
  return dim.days.map((day) => ({
    key: day.key,
    total: day.models.reduce((sum, row) => sum + row.total, 0),
  }));
}
