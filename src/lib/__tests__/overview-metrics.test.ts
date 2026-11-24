import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { OverviewStats } from "@/components/overview/overview-stats";
import { TokenTrendCard } from "@/components/overview/token-trend-card";
import * as api from "@/lib/api";
import { DEFAULT_GATEWAY_CONFIG } from "@/lib/gateway";
import { loadOverviewCredits, loadOverviewUsage, workbuddyOverviewCredits } from "@/lib/overview-metrics";
import type { TraeTokenStatistics } from "@/lib/trae-types";
import type { CreditOfficialUsage, CreditStatistics, GatewayLogEntry, GatewayUsageDim } from "@/lib/types";

vi.mock("@/lib/api", () => ({
  getCreditStatistics: vi.fn(),
  getGatewayConfig: vi.fn(),
  getGatewayLogs: vi.fn(),
  getGatewayUsageDim: vi.fn(),
  getQoderGatewayConfig: vi.fn(),
  getQoderGatewayLogs: vi.fn(),
  getQoderGatewayUsageDim: vi.fn(),
  getTokenStatistics: vi.fn(),
  getTraeGatewayConfig: vi.fn(),
  getTraeGatewayLogs: vi.fn(),
  getTraeGatewayUsageDim: vi.fn(),
  getTraeTokenStatistics: vi.fn(),
}));

const today = "2026-11-21";
const collectedAt = Date.UTC(2026, 10, 21, 2);
const accountId = "workbuddy-global-test";
const officialUsage: CreditOfficialUsage = {
  status: "complete",
  rangeStart: "2026-10-22",
  rangeEnd: today,
  collectedAt,
  summary: { usageToday: 0, usage7Days: 0, usageThisMonth: 0 },
  daily: [{ date: today, usage: 0 }],
  accounts: [{
    accountId,
    accountName: "测试账号",
    ok: true,
    requestCount: 0,
    detailTruncated: false,
    usageToday: 0,
    usage7Days: 0,
    usageThisMonth: 0,
  }],
  requests: [],
  detailLimitPerAccount: 3000,
  errors: [],
};
const stats: CreditStatistics = {
  region: "global",
  generatedAt: collectedAt,
  retentionDays: 90,
  coverageStartAt: collectedAt,
  summary: {
    currentRemaining: 700,
    currentCapacity: 700,
    usageToday: 2450,
    usage7Days: 2450,
    usageThisMonth: 2450,
    todayCheckedInAccounts: 0,
    todaySuccess: 0,
    todayAlready: 0,
    todayFailed: 0,
  },
  daily: [{ date: today, usage: 2450 }],
  accounts: [{
    accountId,
    accountName: "测试账号",
    isCurrent: true,
    currentRemaining: 700,
    totalCapacity: 700,
    lastSnapshotAt: collectedAt,
    usageToday: 2450,
    usage7Days: 2450,
    usageThisMonth: 2450,
    checkedInToday: null,
    checkinStatusToday: null,
    lastCheckinAt: null,
    lastCheckinResult: null,
  }],
  events: [],
  officialUsage,
};

describe("WorkBuddy 概览积分", () => {
  it("官方未发生消耗时显示 0，并保留快照余额", () => {
    expect(workbuddyOverviewCredits(stats, today)).toEqual({ today: 0, remaining: 700, collectedAt });
  });

  it("使用官方的非零消耗", () => {
    const usage = { ...officialUsage, summary: { ...officialUsage.summary, usageToday: 12.5 } };
    expect(workbuddyOverviewCredits({ ...stats, officialUsage: usage }, today)).toEqual({
      today: 12.5,
      remaining: 700,
      collectedAt,
    });
  });

  it.each<{ reason: string; usage?: CreditOfficialUsage }>([
    { reason: "没有官方数据" },
    { reason: "官方用量不可用", usage: { ...officialUsage, status: "unavailable" } },
    { reason: "部分账号读取失败", usage: { ...officialUsage, status: "partial" } },
    { reason: "缓存跨日", usage: { ...officialUsage, rangeEnd: "2026-11-20" } },
    {
      reason: "缓存的账号和当前账号不同",
      usage: { ...officialUsage, accounts: [{ ...officialUsage.accounts[0], accountId: "old-account" }] },
    },
  ])("$reason 时仅显示余额", ({ usage }) => {
    expect(workbuddyOverviewCredits({ ...stats, officialUsage: usage }, today)).toEqual({
      today: null,
      remaining: 700,
    });
  });
});

const loggedAt = new Date(2026, 10, 21, 12);
const yesterday = "2026-11-20";
const log: GatewayLogEntry = {
  ts: +loggedAt,
  endpoint: "/v1/responses",
  method: "POST",
  region: "cn",
  account: "测试账号",
  model: "test-model",
  status: 200,
  latencyMs: 0,
  promptTokens: 60,
  completionTokens: 40,
  credit: 0.25,
  stream: true,
};

function ledgerDay(key: string): GatewayUsageDim["days"][number] {
  return {
    key,
    truncated: false,
    partial: false,
    accounts: [],
    endpoints: [],
    keys: [],
    keysPartial: false,
    models: [{
      model: "test-model", input: 60, output: 40, total: 100, requests: 1, failed: 0, credit: 0.25,
    }],
  };
}

describe("概览网关用量和刷新", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(loggedAt);
    vi.resetAllMocks();
    vi.mocked(api.getTokenStatistics).mockRejectedValue(new Error("客户端统计不可用"));
    vi.mocked(api.getGatewayConfig).mockResolvedValue(DEFAULT_GATEWAY_CONFIG);
    vi.mocked(api.getGatewayLogs).mockResolvedValue({ logs: [] });
    vi.mocked(api.getGatewayUsageDim).mockResolvedValue({
      date: today,
      retentionDays: 90,
      days: [ledgerDay(yesterday), ledgerDay(today)],
    });
  });

  afterEach(() => vi.useRealTimers());

  it("客户端统计读不到时仍显示所选区域的网关历史，重复记录只计一次", async () => {
    vi.mocked(api.getGatewayLogs).mockResolvedValue({ logs: [log, { ...log, region: "global" }] });

    const usage = await loadOverviewUsage("workbuddy", "cn");

    expect(api.getGatewayUsageDim).toHaveBeenCalledWith("cn");
    expect(usage.token).toBeNull();
    expect(usage.days).toHaveLength(14);
    expect(usage.days?.[12]?.gateway).toMatchObject({ total: 100, records: 1 });
    expect(usage.days?.[13]).toMatchObject({
      key: today,
      client: null,
      gateway: { total: 100, records: 1 },
    });
    expect(usage.gateway).toEqual({ today: 1, truncated: false });
    expect(usage.gatewayCreditsToday).toBe(0.25);
  });

  it.each(["trae", "qoder"] as const)("%s 概览的网关请求数按区域过滤", async (product) => {
    const entries = [
      { ...log, region: "cn" },
      { ...log, region: "global" },
    ];
    if (product === "trae") {
      vi.mocked(api.getTraeGatewayLogs).mockResolvedValue({ logs: entries });
      vi.mocked(api.getTraeGatewayConfig).mockResolvedValue({});
      vi.mocked(api.getTraeGatewayUsageDim).mockResolvedValue({
        date: today,
        retentionDays: 90,
        days: [],
      });
      vi.mocked(api.getTraeTokenStatistics).mockResolvedValue({ daily: [] } as unknown as TraeTokenStatistics);
    } else {
      vi.mocked(api.getQoderGatewayLogs).mockResolvedValue({ logs: entries });
      vi.mocked(api.getQoderGatewayConfig).mockResolvedValue({});
      vi.mocked(api.getQoderGatewayUsageDim).mockResolvedValue({
        date: today,
        retentionDays: 90,
        days: [],
      });
    }

    const usage = await loadOverviewUsage(product, "cn");
    expect(usage.gateway).toEqual({ today: 1, truncated: false });
  });

  it("区域逐日账本可补上环形日志中已裁掉的请求，且不会与日志相加", async () => {
    const day = ledgerDay(today);
    day.models[0].requests = 4;
    vi.mocked(api.getGatewayUsageDim).mockResolvedValue({
      date: today,
      retentionDays: 90,
      days: [day],
    });
    vi.mocked(api.getGatewayLogs).mockResolvedValue({ logs: [log] });

    const usage = await loadOverviewUsage("workbuddy", "cn");
    expect(usage.gateway).toEqual({ today: 4, truncated: false });
  });

  it("完整逐日账本覆盖已裁剪的日志，不把精确值标为下界", async () => {
    const day = ledgerDay(today);
    day.models[0].requests = 4;
    vi.mocked(api.getGatewayConfig).mockResolvedValue({ ...DEFAULT_GATEWAY_CONFIG, log_keep: 1 });
    vi.mocked(api.getGatewayUsageDim).mockResolvedValue({
      date: today,
      retentionDays: 90,
      days: [day],
    });
    vi.mocked(api.getGatewayLogs).mockResolvedValue({ logs: [log] });

    const usage = await loadOverviewUsage("workbuddy", "cn");
    expect(usage.gateway).toEqual({ today: 4, truncated: false });
  });

  it("展示尚未入账的网关调用及其 Token 和上报积分", async () => {
    vi.mocked(api.getTokenStatistics).mockResolvedValue({ generatedAt: +loggedAt, sources: [] });
    vi.mocked(api.getGatewayUsageDim).mockResolvedValue({
      date: today,
      retentionDays: 90,
      days: [ledgerDay(yesterday)],
    });
    vi.mocked(api.getGatewayLogs).mockResolvedValue({ logs: [log] });

    const usage = await loadOverviewUsage("workbuddy", "cn");
    expect(usage.days?.[13]?.gateway).toMatchObject({ total: 100, records: 1 });
    expect(usage.gatewayCreditsToday).toBe(0.25);
    const trend = renderToStaticMarkup(createElement(MemoryRouter, {
      children: createElement(TokenTrendCard, { days: usage.days, usageHref: "/s/wb-cn/usage" }),
    }));
    expect(trend).toContain("网关 · 100 Token · 1 次调用");
    expect(trend).not.toContain("近 14 天没有用量记录");

    const statTiles = renderToStaticMarkup(createElement(OverviewStats, {
      metrics: { ...usage, credits: { today: null, remaining: 700 } },
      product: "workbuddy",
      accountCount: 1,
      attentionCount: 0,
      accountsLoading: false,
    }));
    expect(statTiles).toContain("今日网关上报积分");
    expect(statTiles).toContain(">0.25<");
    expect(statTiles).toContain("剩余合计 700");
  });

  it("网关只记录到调用次数时保留调用记录，积分仍是未知", async () => {
    vi.mocked(api.getGatewayUsageDim).mockRejectedValue(new Error("暂时读不到账本"));
    vi.mocked(api.getGatewayLogs).mockResolvedValue({
      logs: [{ ...log, promptTokens: null, completionTokens: null, credit: 0 }],
    });

    const usage = await loadOverviewUsage("workbuddy", "cn");
    expect(usage.days?.[13]?.gateway).toMatchObject({ total: 0, records: 1 });
    expect(usage.gatewayCreditsToday).toBeNull();
  });

  it("页头刷新请求重新采集所选区域的官方积分", async () => {
    vi.mocked(api.getCreditStatistics)
      .mockResolvedValueOnce({
        ...stats, officialUsage: { ...officialUsage, rangeEnd: yesterday },
      })
      .mockResolvedValueOnce(stats);

    await expect(loadOverviewCredits("workbuddy", "global")).resolves.toMatchObject({ today: null });
    await expect(loadOverviewCredits("workbuddy", "global", true)).resolves.toMatchObject({ today: 0 });
    expect(api.getCreditStatistics).toHaveBeenNthCalledWith(1, false, "global");
    expect(api.getCreditStatistics).toHaveBeenNthCalledWith(2, true, "global");
  });
});
