import type { ExternalCreditAccount, ExternalCreditProvider } from "@/lib/api";

export interface ExternalPlatformSettings {
  autoRefreshMinutes: 0 | 5 | 15 | 30 | 60;
  lowBalanceThreshold: number;
}

export interface ExternalBalanceSample {
  accountId: string;
  name: string;
  remaining: number;
  sampledAt: number;
}

const SETTINGS_KEY = "workbuddy.external-platforms.settings.v1";
const SAMPLES_KEY = "workbuddy.external-platforms.balance-samples.v1";
type SettingsStore = Partial<Record<ExternalCreditProvider, ExternalPlatformSettings>>;
type SamplesStore = Partial<Record<ExternalCreditProvider, ExternalBalanceSample[]>>;

function readJson<T>(key: string): T {
  try {
    return JSON.parse(localStorage.getItem(key) ?? "{}") as T;
  } catch {
    return {} as T;
  }
}

export function getExternalPlatformSettings(provider: ExternalCreditProvider): ExternalPlatformSettings {
  const stored = readJson<SettingsStore>(SETTINGS_KEY)[provider];
  return {
    autoRefreshMinutes: [0, 5, 15, 30, 60].includes(stored?.autoRefreshMinutes ?? 0)
      ? stored?.autoRefreshMinutes ?? 0
      : 0,
    lowBalanceThreshold: Number.isFinite(stored?.lowBalanceThreshold)
      ? Math.max(0, stored?.lowBalanceThreshold ?? 0)
      : 0,
  };
}

export function saveExternalPlatformSettings(
  provider: ExternalCreditProvider,
  settings: ExternalPlatformSettings,
): void {
  const store = readJson<SettingsStore>(SETTINGS_KEY);
  store[provider] = {
    autoRefreshMinutes: settings.autoRefreshMinutes,
    lowBalanceThreshold: Math.max(0, settings.lowBalanceThreshold),
  };
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(store));
}

/** Keep observed balances, without claiming that balance deltas equal provider-reported usage. */
export function recordExternalBalanceSamples(
  provider: ExternalCreditProvider,
  accounts: ExternalCreditAccount[],
): void {
  const store = readJson<SamplesStore>(SAMPLES_KEY);
  const samples = store[provider] ?? [];
  const next = [...samples];
  /* 用 Set 去重：这条路径每个页面挂载/手动刷新/自动刷新都会走，逐条 `next.some(...)`
     在 5000 条上限下是 O(accounts × samples)。 */
  const seen = new Set(next.map((sample) => `${sample.accountId}:${sample.sampledAt}`));
  let appended = false;
  for (const account of accounts) {
    if (account.remaining == null || account.updatedAt == null) continue;
    const key = `${account.id}:${account.updatedAt}`;
    if (seen.has(key)) continue;
    seen.add(key);
    next.push({
      accountId: account.id,
      name: account.name,
      remaining: account.remaining,
      sampledAt: account.updatedAt,
    });
    appended = true;
  }
  // 没有新采样就不要回写整个 store（那是白白的序列化 + 磁盘写入）。
  if (!appended) return;
  store[provider] = next.sort((a, b) => a.sampledAt - b.sampledAt).slice(-5000);
  localStorage.setItem(SAMPLES_KEY, JSON.stringify(store));
}

export function getExternalBalanceSamples(provider: ExternalCreditProvider): ExternalBalanceSample[] {
  const store = readJson<SamplesStore>(SAMPLES_KEY);
  return (store[provider] ?? []).filter((sample) =>
    typeof sample.accountId === "string" &&
    typeof sample.name === "string" &&
    Number.isFinite(sample.remaining) &&
    Number.isFinite(sample.sampledAt),
  );
}
