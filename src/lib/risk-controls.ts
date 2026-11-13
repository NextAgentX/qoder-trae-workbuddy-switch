import type { RiskControlSettings } from "@/lib/types";

export const DEFAULT_RISK_CONTROL_SETTINGS: RiskControlSettings = {
  requests_per_minute: 30,
  max_in_flight_per_account: 1,
  max_in_flight_global: 1,
  min_account_gap_ms: 5000,
  max_transient_attempts: 1,
  fallback_cooldown_secs: 60,
};

export const DEFAULT_RISK_CONTROLS_BY_REGION = {
  cn: { ...DEFAULT_RISK_CONTROL_SETTINGS },
  global: { ...DEFAULT_RISK_CONTROL_SETTINGS },
};

export function normalizeRiskControlsByRegion(value: unknown) {
  const record = value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
  const normalize = (raw: unknown): RiskControlSettings => {
    const row = raw && typeof raw === "object" ? raw as Record<string, unknown> : {};
    const number = (key: keyof RiskControlSettings, fallback: number) => {
      const candidate = row[key];
      return typeof candidate === "number" && Number.isFinite(candidate) ? candidate : fallback;
    };
    return {
      requests_per_minute: number("requests_per_minute", 30),
      max_in_flight_per_account: number("max_in_flight_per_account", 1),
      max_in_flight_global: number("max_in_flight_global", 1),
      min_account_gap_ms: number("min_account_gap_ms", 5000),
      max_transient_attempts: number("max_transient_attempts", 1),
      fallback_cooldown_secs: number("fallback_cooldown_secs", 60),
    };
  };
  return {
    cn: normalize(record.cn),
    global: normalize(record.global),
  };
}
