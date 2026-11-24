import { describe, expect, it } from "vitest";
import { traeCreditBalance } from "@/lib/trae-usage-charts";
import type { UsageWindow } from "@/lib/usage-window";

const window: UsageWindow = { kind: "custom", from: "2026-11-19", to: "2026-11-22" };
const now = new Date(2026, 10, 22, 12).getTime();
const snapshot = (date: string, total: number) => ({ date, total, earned: 0, consumed: 0 });

describe("Trae balance history", () => {
  it("shows recorded balances without backfilling dates before the account", () => {
    const points = traeCreditBalance([[snapshot("2026-11-21", 0), snapshot("2026-11-22", 2100)]], window, now);
    expect(points.map(({ label, value }) => [label, value])).toEqual([["11/21", 0], ["11/22", 2100]]);
  });

  it("keeps the last known balance when another region records a new snapshot", () => {
    const points = traeCreditBalance([
      [snapshot("2026-11-18", 300), snapshot("2026-11-22", 200)],
      [snapshot("2026-11-21", 500), snapshot("2026-11-22", 400)],
    ], window, now);
    expect(points.map(({ label, value }) => [label, value])).toEqual([["11/21", 800], ["11/22", 600]]);
  });
});
