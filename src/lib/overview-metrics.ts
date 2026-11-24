import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "@/lib/api";
import { normalizeGatewayConfig } from "@/lib/gateway";
import type { Product } from "@/lib/platforms/registry";
import { normalizeQoderGatewayConfig, normalizeQoderGatewayLogs } from "@/lib/qoder-gateway";
import { normalizeTraeGatewayConfig, normalizeTraeGatewayLogs } from "@/lib/trae-gateway";
import type { CreditStatistics, GatewayLogEntry, GatewayUsageDim, Region } from "@/lib/types";
import { mergeSources } from "@/lib/usage-charts";
import { dateKey, todayKey, usageWindowKeys } from "@/lib/usage-window";

interface TokenPoint {
  total: number;
  records: number;
  partial?: boolean;
}

interface GatewayPoint extends TokenPoint {
  credit: number;
}

export interface OverviewDay {
  key: string;
  client: TokenPoint | null;
  gateway: TokenPoint | null;
}

interface OverviewCredits {
  today: number | null;
  remaining: number;
  collectedAt?: number;
}

interface OverviewUsage {
  days: OverviewDay[] | null;
  /** 统计卡和趋势图使用同一组逐日数据。 */
  token: {
    today: number;
    partial: boolean;
    yesterday: number | null;
  } | null;
  /** 网关上报的已知消耗，缺少正值时无法确认消耗。 */
  gatewayCreditsToday: number | null;
  /** 今日网关请求数来自环形日志，达到保留上限时可能是下界。 */
  gateway: { today: number; truncated: boolean } | null;
}

export interface OverviewMetrics extends OverviewUsage {
  credits: OverviewCredits | null;
}

const TREND_DAYS = 14;

const GATEWAY_LEDGER: Record<Product, (region: Region) => Promise<GatewayUsageDim>> = {
  workbuddy: api.getGatewayUsageDim,
  trae: api.getTraeGatewayUsageDim,
  qoder: api.getQoderGatewayUsageDim,
};

async function loadClientSeries(product: Product, region: Region) {
  // Trae 和 Qoder 的用量来自网关。Trae 的旧 Token 统计也是网关日志。
  if (product !== "workbuddy") return null;
  const stats = await api.getTokenStatistics(30, region);
  return mergeSources(stats.sources ?? []).daily;
}

async function loadTraeRecentDays(product: Product, region: Region) {
  if (product !== "trae") return [];
  const stats = await api.getTraeTokenStatistics(TREND_DAYS, region);
  return stats.daily ?? [];
}

export function workbuddyOverviewCredits(stats: CreditStatistics, today: string): OverviewCredits {
  const remaining = stats.summary.currentRemaining;
  const unavailable = { today: null, remaining };
  const official = stats.officialUsage;
  if (official?.status !== "complete" || official.rangeEnd !== today) return unavailable;

  // 缓存可能来自账号变更前。只有覆盖当前整套账号时才能显示消耗合计。
  const reportedIds = new Set(
    official.accounts.filter((account) => account.ok).map((account) => account.accountId),
  );
  if (
    official.accounts.length !== stats.accounts.length ||
    stats.accounts.some((account) => !reportedIds.has(account.accountId))
  ) {
    return unavailable;
  }

  const usage = official.summary?.usageToday;
  if (typeof usage !== "number" || !Number.isFinite(usage) || usage < 0) return unavailable;
  return { today: usage, remaining, collectedAt: official.collectedAt };
}

export async function loadOverviewCredits(
  product: Product,
  region: Region,
  refresh = false,
): Promise<OverviewCredits | null> {
  switch (product) {
    case "workbuddy": {
      const stats = await api.getCreditStatistics(refresh, region).catch(() =>
        refresh ? api.getCreditStatistics(false, region) : null,
      );
      return stats ? workbuddyOverviewCredits(stats, todayKey()) : null;
    }
    case "trae": {
      const credits = await api.getTraeCredits(region);
      const cached = Object.values(credits.remaining ?? {});
      if (cached.length === 0) return null;
      const remaining = cached.reduce((sum, value) => sum + (Number.isFinite(value) ? value : 0), 0);
      const today = (credits.daily ?? []).find((point) => point.date === todayKey());
      return { today: today?.consumed ?? 0, remaining };
    }
    case "qoder":
      return null;
  }
}

interface OverviewGatewayLogs {
  logs: { ts: number }[];
  keep: number;
  workbuddyLogs?: GatewayLogEntry[];
}

const GATEWAY_LOGS: Record<Product, () => Promise<OverviewGatewayLogs>> = {
  workbuddy: async () => {
    const [{ logs }, config] = await Promise.all([
      api.getGatewayLogs(),
      api.getGatewayConfig(),
    ]);
    return { logs: logs ?? [], keep: normalizeGatewayConfig(config).log_keep, workbuddyLogs: logs };
  },
  trae: async () => {
    const [logs, config] = await Promise.all([
      api.getTraeGatewayLogs(),
      api.getTraeGatewayConfig(),
    ]);
    return {
      logs: normalizeTraeGatewayLogs(logs),
      keep: normalizeTraeGatewayConfig(config).logKeep,
    };
  },
  qoder: async () => {
    const [logs, config] = await Promise.all([
      api.getQoderGatewayLogs(),
      api.getQoderGatewayConfig(),
    ]);
    return {
      logs: normalizeQoderGatewayLogs(logs),
      keep: normalizeQoderGatewayConfig(config).logKeep,
    };
  },
};

function summarizeGatewayLogs({ logs, keep }: OverviewGatewayLogs, today: string) {
  let count = 0;
  let oldest = Number.POSITIVE_INFINITY;
  for (const entry of logs) {
    if (dateKey(new Date(entry.ts)) === today) count += 1;
    oldest = Math.min(oldest, entry.ts);
  }
  return {
    today: count,
    truncated: logs.length >= Math.max(1, keep) && dateKey(new Date(oldest)) === today,
  };
}

function gatewayLedgerDays(dim: GatewayUsageDim | null) {
  return new Map<string, GatewayPoint>((dim?.days ?? []).map((day) => [
    day.key,
    {
      total: day.models.reduce((sum, row) => sum + row.total, 0),
      records: day.models.reduce((sum, row) => sum + row.requests, 0),
      credit: day.models.reduce((sum, row) => sum + row.credit, 0),
      partial: day.truncated || day.partial,
    },
  ]));
}

/** 环形日志和账本可能覆盖同一请求。逐项取较大的已记录值。 */
function mergeRecentDay(
  days: Map<string, GatewayPoint>,
  key: string,
  recent: GatewayPoint,
) {
  const stored = days.get(key);
  const usesRecent = !stored || recent.records > stored.records || recent.total > stored.total;
  days.set(key, {
    total: Math.max(recent.total, stored?.total ?? 0),
    records: Math.max(recent.records, stored?.records ?? 0),
    credit: Math.max(recent.credit, stored?.credit ?? 0),
    partial: Boolean(stored?.partial || (usesRecent && recent.partial)),
  });
}

/** 环形日志可先显示 WorkBuddy 尚未入账的当日用量。 */
function recentWorkbuddyUsage(logs: GatewayLogEntry[], region: Region, today: string) {
  let total = 0;
  let records = 0;
  let credit = 0;
  for (const entry of logs) {
    if (entry.region !== region || !entry.account?.trim() || dateKey(new Date(entry.ts)) !== today) {
      continue;
    }
    records += 1;
    total += (entry.promptTokens ?? 0) + (entry.completionTokens ?? 0);
    if (typeof entry.credit === "number" && Number.isFinite(entry.credit) && entry.credit > 0) {
      credit += entry.credit;
    }
  }
  return { total, records, credit };
}

export async function loadOverviewUsage(product: Product, region: Region): Promise<OverviewUsage> {
  const [client, dim, logs, traeRecentDays] = await Promise.all([
    loadClientSeries(product, region).catch(() => null),
    GATEWAY_LEDGER[product](region).catch(() => null),
    GATEWAY_LOGS[product]().catch(() => null),
    loadTraeRecentDays(product, region).catch(() => []),
  ]);

  const now = Date.now();
  const keys = usageWindowKeys({ kind: "preset", days: TREND_DAYS }, now);
  const today = todayKey(now);
  const gateway = logs ? summarizeGatewayLogs(logs, today) : null;
  const clientDays = new Map(client?.map((day) => [day.key, day]));
  const gatewayDays = gatewayLedgerDays(dim);

  if (logs?.workbuddyLogs) {
    const recent = recentWorkbuddyUsage(logs.workbuddyLogs, region, today);
    if (recent.records > 0) {
      mergeRecentDay(gatewayDays, today, { ...recent, partial: gateway?.truncated });
    }
  }

  for (const day of traeRecentDays) {
    if (!day.key) continue;
    mergeRecentDay(gatewayDays, day.key, {
      total: day.total ?? 0,
      records: day.records ?? 0,
      credit: 0,
      partial: !logs || logs.logs.length >= Math.max(1, logs.keep),
    });
  }

  const emptyClientDay = client ? { total: 0, records: 0 } : null;
  const days: OverviewDay[] | null = client || dim || gatewayDays.size > 0
    ? keys.map((key) => ({
      key,
      client: clientDays.get(key) ?? emptyClientDay,
      gateway: gatewayDays.get(key) ?? null,
    }))
    : null;
  const source = product === "workbuddy" ? "client" : "gateway";
  const todayPoint = days?.[keys.length - 1]?.[source];
  const yesterdayPoint = days?.[keys.length - 2]?.[source];
  const gatewayCredit = gatewayDays.get(today)?.credit ?? 0;
  return {
    days,
    token: todayPoint ? {
      today: todayPoint.total,
      partial: todayPoint.partial === true,
      yesterday: yesterdayPoint?.partial ? null : yesterdayPoint?.total ?? null,
    } : null,
    gatewayCreditsToday: product === "workbuddy" && Number.isFinite(gatewayCredit) && gatewayCredit > 0
      ? gatewayCredit
      : null,
    gateway,
  };
}

const EMPTY_METRICS: OverviewMetrics = {
  days: null,
  token: null,
  credits: null,
  gatewayCreditsToday: null,
  gateway: null,
};

/** 切换作用域或再次刷新时，丢弃先前请求的结果。 */
export function useOverviewMetrics(product: Product, region: Region) {
  const scope = `${product}:${region}`;
  const [state, setState] = useState({ scope, metrics: EMPTY_METRICS });
  const active = useRef<AbortController | null>(null);

  const load = useCallback(async (refresh: boolean) => {
    active.current?.abort();
    const request = new AbortController();
    active.current = request;

    const apply = (patch: Partial<OverviewMetrics>) => {
      if (!request.signal.aborted) {
        setState((previous) => ({
          scope,
          metrics: { ...(previous.scope === scope ? previous.metrics : EMPTY_METRICS), ...patch },
        }));
      }
    };

    await Promise.all([
      loadOverviewUsage(product, region)
        .then(apply)
        .catch(() => apply({
          days: null, token: null, gateway: null, gatewayCreditsToday: null,
        })),
      loadOverviewCredits(product, region, refresh)
        .catch(() => null)
        .then((credits) => apply({ credits })),
    ]);
  }, [product, region, scope]);

  useEffect(() => {
    void load(false);
    return () => active.current?.abort();
  }, [load]);

  return {
    metrics: state.scope === scope ? state.metrics : EMPTY_METRICS,
    refresh: () => load(true),
  };
}
