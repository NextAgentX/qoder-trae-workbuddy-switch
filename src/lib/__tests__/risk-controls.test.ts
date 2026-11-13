import { describe, expect, it } from "vitest";

import {
  DEFAULT_RISK_CONTROL_SETTINGS,
  normalizeRiskControlSettings,
} from "@/lib/risk-controls";

describe("normalizeRiskControlSettings", () => {
  it("keeps the complete valid settings object", () => {
    const settings = {
      enabled: true,
      requests_per_minute: 12,
      max_in_flight_per_account: 3,
      max_in_flight_global: 8,
      min_account_gap_ms: 250,
      max_transient_attempts: 2,
      fallback_cooldown_secs: 90,
    };
    expect(normalizeRiskControlSettings(settings)).toEqual(settings);
  });

  it("defaults every missing or non-numeric field independently", () => {
    const settings = normalizeRiskControlSettings({
      enabled: true,
      requests_per_minute: 9,
      max_in_flight_per_account: "3",
      max_in_flight_global: Number.POSITIVE_INFINITY,
      min_account_gap_ms: null,
      max_transient_attempts: 0,
      fallback_cooldown_secs: 30,
    });
    expect(settings).toEqual({
      ...DEFAULT_RISK_CONTROL_SETTINGS,
      enabled: true,
      requests_per_minute: 9,
      max_transient_attempts: 0,
      fallback_cooldown_secs: 30,
    });
  });

  it("defaults to disabled and the shared defaults for absent input", () => {
    expect(normalizeRiskControlSettings(undefined)).toEqual(DEFAULT_RISK_CONTROL_SETTINGS);
    expect(normalizeRiskControlSettings(null)).toEqual(DEFAULT_RISK_CONTROL_SETTINGS);
    expect(normalizeRiskControlSettings([])).toEqual(DEFAULT_RISK_CONTROL_SETTINGS);
  });

  it("treats only the boolean true as enabled", () => {
    expect(normalizeRiskControlSettings({ enabled: "true" }).enabled).toBe(false);
    expect(normalizeRiskControlSettings({ enabled: 1 }).enabled).toBe(false);
    expect(normalizeRiskControlSettings({ enabled: false }).enabled).toBe(false);
  });
});
