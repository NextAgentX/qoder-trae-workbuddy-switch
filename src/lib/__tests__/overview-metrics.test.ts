import { describe, expect, it } from "vitest";

import { workbuddyOverviewCredits } from "@/lib/overview-metrics";
import type { CreditOfficialUsage, CreditStatistics } from "@/lib/types";

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
