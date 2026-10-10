/**
 * 概览 · **工作台指标**（数据层，P3）。
 *
 * ## 为什么单独一层
 *
 * 概览要回答的四个数字（今日 Token / 今日积分消耗 / 今日网关请求 / 账号总数）里，
 * **前三个在两平台来自不同的命令**：
 *
 * | 指标 | WorkBuddy | Trae |
 * | --- | --- | --- |
 * | 逐日 Token | `get_token_statistics` + `mergeSources` | `get_trae_token_statistics` |
 * | 积分 | `get_credit_statistics`（`summary.usageToday`） | `get_trae_credits`（当日快照 `consumed`） |
 * | 网关请求 | `get_gateway_logs` | `get_trae_gateway_logs` |
 *
 * 这些差异是**「数据从哪来」**，不是「能力有没有」⇒ 用 `Record<Product, …>` 表声明，
 * 与 `UsagePage` 的 `PLATFORM_TABS` / `USAGE_PAGES` 同一口径（**不写 `product === "trae"`**）。
 *
 * ## ★ 数据诚实性（本项目红线）
 *
 * 1. **每项指标独立可失败**：某项读不到就是 `null`，界面显示「—」——
 *    **不回落 0**。0 是「确实是 0」，读不到是「不知道」，两者不能混（§6 #5）。
 * 2. **今日网关请求来自本机请求日志**，而后端日志是**环形裁剪**的（保留最近 `log_keep` 条）
 *    ⇒ 被裁过时它只是**下界**，`truncated` 如实标出，界面显示「≥ N」。
 */

import { useEffect, useState } from "react";

import * as api from "@/lib/api";
import { normalizeGatewayConfig } from "@/lib/gateway";
import type { Product } from "@/lib/platforms/registry";
import { normalizeTraeGatewayConfig, normalizeTraeGatewayLogs } from "@/lib/trae-gateway";
import type { Region } from "@/lib/types";
import { mergeSources } from "@/lib/usage-charts";

/** 逐日 Token 点（两平台归一后的**最小**形状：只有柱状图真正要用的三个字段）。 */
export interface OverviewDay {
  /** `YYYY-MM-DD`（本地日期，与后端逐日桶同口径）。 */
  key: string;
  /** 当日 Token（输入 + 输出）。 */
  total: number;
  /** 当日调用记录数。 */
  records: number;
}

/** 概览统计行与趋势卡共用的指标（每项 `null` = 读不到，**不编数**）。 */
export interface OverviewMetrics {
  /** 首次加载中（尚无任何结果）。 */
  loading: boolean;
  /** 近 14 天逐日 Token（升序，恒 14 项、缺日补 0）。 */
  token: { days: OverviewDay[]; today: number; yesterday: number } | null;
  /** 今日积分消耗 + 剩余合计（无缓存时 `null`）。 */
  credits: { today: number; remaining: number } | null;
  /** 今日网关请求数（**仅本机请求日志**）；`truncated` ⇒ 该数是下界。 */
  gateway: { today: number; truncated: boolean } | null;
}

const TREND_DAYS = 14;
const DAY_MS = 24 * 60 * 60 * 1000;

function dateKey(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** 今天零点（本地时区）——「今日」的判据只有这一处。 */
function startOfToday(): number {
  const date = new Date();
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/**
 * 逐日 Token 序列（**平台口径各自的**）。
 *
 * 两个平台都取 `TREND_DAYS` 天：窗口一致，「近 14 天」才是同一件事。
 */
const TOKEN_SERIES: Record<Product, (region: Region) => Promise<OverviewDay[]>> = {
  workbuddy: async (region) => {
    const stats = await api.getTokenStatistics(TREND_DAYS, region);
    /* 与用量页**同源**：多个来源（workbuddy / codebuddy-cli / …）经同一个 `mergeSources`
       聚合，任何口径变化两处会一起变（PLAN §3.10 第 1 条）。 */
    return mergeSources(stats.sources ?? []).daily.map((point) => ({
      key: point.key,
      total: point.total,
      records: point.records,
    }));
  },
  trae: async (region) => {
    // Trae 的 `scope` 档位与区域同名（`cn` / `global`），直接传区域。
    const stats = await api.getTraeTokenStatistics(TREND_DAYS, region);
    return (stats.daily ?? [])
      .filter((point) => Boolean(point.key))
      .map((point) => ({
        key: point.key as string,
        total: point.total ?? 0,
        records: point.records ?? 0,
      }));
  },
};

/** 今日积分消耗 + 剩余合计；**无数据源可读时返回 `null`**（不回落 0）。 */
const CREDIT_SNAPSHOT: Record<
  Product,
  (region: Region) => Promise<{ today: number; remaining: number } | null>
> = {
  workbuddy: async (region) => {
    const stats = await api.getCreditStatistics(false, region);
    return { today: stats.summary.usageToday, remaining: stats.summary.currentRemaining };
  },
  trae: async (region) => {
    const credits = await api.getTraeCredits(region);
    // 积分缓存只存 uid ⇒ 空表表示「还没采集过」，此时**不给 0**（0 是「确实没积分」）。
    const cached = Object.values(credits.remaining ?? {});
    if (cached.length === 0) return null;
    const remaining = cached.reduce((sum, value) => sum + (Number.isFinite(value) ? value : 0), 0);
    const today = dateKey(new Date());
    const snapshot = (credits.daily ?? []).find((point) => point.date === today);
    return { today: snapshot?.consumed ?? 0, remaining };
  },
};

/** 今日网关请求数（**仅本机请求日志**）。 */
const GATEWAY_TODAY: Record<Product, () => Promise<{ today: number; truncated: boolean }>> = {
  workbuddy: async () => {
    const [{ logs }, rawConfig] = await Promise.all([
      api.getGatewayLogs(),
      api.getGatewayConfig(),
    ]);
    return summarizeGatewayLogs(logs ?? [], normalizeGatewayConfig(rawConfig).log_keep);
  },
  trae: async () => {
    const [rawLogs, rawConfig] = await Promise.all([
      api.getTraeGatewayLogs(),
      api.getTraeGatewayConfig(),
    ]);
    return summarizeGatewayLogs(
      normalizeTraeGatewayLogs(rawLogs),
      normalizeTraeGatewayConfig(rawConfig).logKeep,
    );
  },
};

/**
 * 按请求日志求「今日请求数」。
 *
 * ⚠️ **后端日志是环形裁剪的**（`RequestLog::trim_and_save` 保留最近 `keep` 条）⇒
 * 当「仍在册的最旧一条」已经是今天的，说明今天的更早记录被裁掉了，本数只是**下界**。
 * 这个判别必须做：把下界当成精确值显示，就是对一个真实数字的**谎报**。
 */
function summarizeGatewayLogs(
  logs: readonly { ts: number }[],
  keep: number,
): { today: number; truncated: boolean } {
  const start = startOfToday();
  const today = logs.filter((entry) => entry.ts >= start).length;
  const oldest = logs.reduce(
    (min, entry) => (entry.ts < min ? entry.ts : min),
    Number.POSITIVE_INFINITY,
  );
  const truncated =
    Number.isFinite(oldest) && oldest >= start && logs.length >= Math.max(1, keep);
  return { today, truncated };
}

/** 把逐日序列铺到「最近 14 天」的固定轴上（缺日补 0，不跳格）。 */
function buildTrendAxis(series: readonly OverviewDay[]): OverviewDay[] {
  const byKey = new Map(series.map((point) => [point.key, point]));
  const axis: OverviewDay[] = [];
  for (let offset = TREND_DAYS - 1; offset >= 0; offset -= 1) {
    const key = dateKey(new Date(Date.now() - offset * DAY_MS));
    const point = byKey.get(key);
    axis.push({ key, total: point?.total ?? 0, records: point?.records ?? 0 });
  }
  return axis;
}

async function loadOverviewMetrics(
  product: Product,
  region: Region,
): Promise<Omit<OverviewMetrics, "loading">> {
  /* 三项**各自**兜错：某一项失败不该把另外两项也变成「—」。 */
  const [series, credits, gateway] = await Promise.all([
    TOKEN_SERIES[product](region).catch(() => null),
    CREDIT_SNAPSHOT[product](region).catch(() => null),
    GATEWAY_TODAY[product]().catch(() => null),
  ]);

  let token: OverviewMetrics["token"] = null;
  if (series) {
    const days = buildTrendAxis(series);
    const totalOf = (offset: number) =>
      days.find((day) => day.key === dateKey(new Date(Date.now() - offset * DAY_MS)))?.total ?? 0;
    token = { days, today: totalOf(0), yesterday: totalOf(1) };
  }

  return { token, credits, gateway };
}

/** 概览统计行 / 趋势卡的数据钩子（作用域变化即重取）。 */
export function useOverviewMetrics(product: Product, region: Region): OverviewMetrics {
  const [state, setState] = useState<OverviewMetrics>({
    loading: true,
    token: null,
    credits: null,
    gateway: null,
  });

  useEffect(() => {
    let disposed = false;
    setState((prev) => ({ ...prev, loading: true }));
    loadOverviewMetrics(product, region)
      .then((next) => {
        if (!disposed) setState({ ...next, loading: false });
      })
      .catch(() => {
        if (!disposed) setState({ loading: false, token: null, credits: null, gateway: null });
      });
    return () => {
      disposed = true;
    };
  }, [product, region]);

  return state;
}