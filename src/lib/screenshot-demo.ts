import type {
  AccountMeta, AccountStrategyMap, AccountTokenTodayResponse, ApiKeyRecord, AppStatus, AutoBackupConfig, AutoBackupEntry, AutoRotateConfig, CacheScanReport, CatalogSnapshot,
  CheckinConfig, CheckinLog, CodeBuddyCliStatus, CodeBuddyCliSwitchResult, CodeBuddyCnIdeStatus, ClientLimitsResponse, CreditExpiry,
  JetbrainsCodebuddyPluginStatus,
  VscodeCodebuddyPluginStatus,
  ClientSettingsBackupsResponse, ClientSettingsDiffResponse, ClientSettingsFingerprint,
  ClientSettingsProxyGroup, ClientSettingsReadResponse, ClientSettingsSlot, ClientSettingsSlotsResponse,
  CreditOfficialUsageModel, CreditStatistics, GatewayConfig, GatewayLogEntry, GatewayPoolAccount,
  GatewayStatus, GatewayTokenTrend, GatewayUsageDim, GatewayUsageRecon, GithubConfig, Region, RotateLog,
  RotateStatus, ScheduleConfig, TokenStatistics,
  RuntimeLogsResponse, RuntimeLogLevel,
  Session, SessionReplicaResponse,
  SkillListResponse,
  TokenStatsGroup, TokenStatsSource,
  TokenStatsTotals, TravelConfig, TravelStatus,
} from "./types";
import type {
  TraeAccount,
  TraeApiKeyRecord,
  TraeCapabilities,
  TraeCheckinStatus,
  TraeClientModel,
  TraeClientModelList,
  TraeCreditsOverview,
  TraeEnvStatus,
  TraeVariantsStatus,
  TraeGatewayConfigRaw,
  TraeGatewayLogEntry,
  TraeLogsResponse,
  TraeProfilesOverview,
  TraeSettings,
  TraeTokenStatistics,
  TraeVariantId,
} from "./trae-types";
import { demoModeEnabled } from "./demo-mode";
import { t } from "./i18n";
import type { TranslationKey } from "@/locales/zh";
import type { CompanionSnapshot } from "./companion";
import type {
  AgentHookStatus,
  CheckinStatusEntry,
  QoderGatewayQuotaView,
  QoderWebWindowCookies,
} from "./api";
import type { QoderGatewayConfigRaw } from "./qoder-gateway";

export const screenshotDemoEnabled = demoModeEnabled;

/**
 * 演示夹具里的**自然语言**一律走 `t()` —— 演示站/截图站也支持中英切换，
 * 而 `npm run build:demo` 产出的 `dist-demo` 正是给人看的那一站。
 *
 * ⚠️ 两条纪律（演示数据是「看起来像真的」的样本，写错会误导截图核对）：
 *
 * 1. **键表是模块级常量 ⇒ 只能存键名，不能存文案。** 凡「一整张表在建表时定型」的地方
 *    （`creditPackageSeeds` / `tokenSessionSeeds` / `capabilityStubs` / 日志行 / 假账号名），
 *    都存 `TranslationKey`（或含键名的小结构），**在函数体/读取点现取** `t(key)`；
 *    直接把 `t()` 的结果写进表里，切语言后整张表不会更新（与 `PRODUCT_NAV` 同一个坑）。
 * 2. **刻意保持英文的字面量不要「顺手翻译」**：模型名（`glm-5.3`）、协议键
 *    （`apiProvider` / `apiBase`）、HTTP 端点（`/v1/chat/completions`）、状态字面量
 *    （`available` / `cooling` / `"限时免费"` 那种**数据值**除外）、路径、uid 都是**契约**，
 *    翻译了就不再是「与真机同形」的样本。
 */
const MODEL_NAMES = ["deepseek-v4-flash", "kimi-k3-1", "deepseek-v4-pro", "glm-5.2", "hy3"] as const;

interface ModelSeed {
  model: (typeof MODEL_NAMES)[number];
  requestCount: number;
  credit: number;
}

interface AccountUsageSeed {
  requestCount: number;
  models: ModelSeed[];
}

/**
 * 假账号的展示名与备注。
 *
 * 按 `id` 取键（**不是**数组下标）：`id` 才是稳定身份，将来插一条演示账号也不会
 * 让所有名字整体错位 —— 键与实体一一对应是这类映射的最低要求。
 */
const DEMO_ACCOUNT_KEYS: Record<string, { nickname: TranslationKey; remark?: TranslationKey }> = {
  "demo-account-a": { nickname: "shared.demo.account.a", remark: "shared.demo.account.aRemark" },
  "demo-account-b": { nickname: "shared.demo.account.b" },
  "demo-account-c": { nickname: "shared.demo.account.c" },
  "demo-account-d": { nickname: "shared.demo.account.d" },
  "demo-account-e": { nickname: "shared.demo.account.e" },
  "demo-account-f": { nickname: "shared.demo.account.f" },
  "demo-account-g": { nickname: "shared.demo.account.g" },
  "demo-account-h": { nickname: "shared.demo.account.h" },
};
/**
 * 演示账号的**静态部分**（`nickname` / `remark` 不在表内）。
 *
 * 把随语言切换的两个字段**排除在表外**是刻意的：`screenshot-demo.ts` 的夹具表在模块
 * 加载时定型，若在此处写死 `t(...)`，切语言后整张表不会更新；写成「表 + 读取时补名」
 * 又会让 `AccountMeta` 类型缺字段（它要求 `nickname` 必填）。
 * ⇒ 用 `Omit` 明确表达「这张表暂时没有那两个字段」，由 `withDemoNames` 补齐。
 */
type DemoAccountSeed = Omit<AccountMeta, "nickname" | "remark"> & { id: keyof typeof DEMO_ACCOUNT_KEYS };

/** 相对时刻（「N 分钟前 / N 小时前 / N 天前」由指标行第三格现算）。 */
const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const ago = (ms: number) => Date.now() - ms;

const accountSeeds: DemoAccountSeed[] = [
  // ★ 2026-10-12：与原型 `SCOPES["wb-cn"].accounts`（`buddy-switch-prototype.html:1229-1266`）
  //   **逐账号对应** —— 名字 / 邮箱 / 最后使用 / 需重登 / 凭据过期全部照抄，
  //   演示站因此可以与原型**逐卡对照**（用户要求的 1:1 核验）。
  //   刻意覆盖的状态：当前登录 · 需重新登录 · 凭据已过期 · 今日未签到 · 优先 · 临期积分。
  { id: "demo-account-a", uid: "demo-user-001", email: "jackey@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(2 * MINUTE_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
  { id: "demo-account-b", uid: "demo-user-002", email: "shared@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(1 * HOUR_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
  { id: "demo-account-c", uid: "demo-user-003", email: "backup01@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(3 * DAY_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
  { id: "demo-account-d", uid: "demo-user-004", email: "qa@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(5 * MINUTE_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
  // design-lab：`expiresAt` 在过去 ⇒ 卡片状态标签走「凭据已过期」。
  { id: "demo-account-e", uid: "demo-user-005", email: "design@example.com", enterpriseName: "Demo Workspace", expiresAt: ago(1 * DAY_MS), refreshExpiresAt: 0, lastUsedAt: ago(8 * DAY_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
  { id: "demo-account-f", uid: "demo-user-006", email: "ops02@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(1 * DAY_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
  // spare-global：需重新登录。
  { id: "demo-account-g", uid: "demo-user-007", email: "spare@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(12 * DAY_MS), refreshedAt: 0, createdAt: 0, needsRelogin: true, needsReloginReason: null },
  { id: "demo-account-h", uid: "demo-user-008", email: "demo@example.com", enterpriseName: "Demo Workspace", expiresAt: 0, refreshExpiresAt: 0, lastUsedAt: ago(4 * HOUR_MS), refreshedAt: 0, createdAt: 0, needsRelogin: false, needsReloginReason: null },
];

/**
 * 给账号补上随语言切换的展示字段（`nickname` / `remark`）。
 *
 * 调用点必须**在渲染期**（不是模块加载期）：`nickname` 直接进 React 树，
 * 用户切语言时它们跟着变，而不是停留在建表时的那一种语言。
 */
function withDemoNames(list: readonly DemoAccountSeed[]): AccountMeta[] {
  return list.map((account) => {
    const keys = DEMO_ACCOUNT_KEYS[account.id];
    return {
      ...account,
      nickname: t(keys.nickname),
      remark: keys.remark ? t(keys.remark) : null,
    };
  });
}

/**
 * 单条账号的展示名（`nickname → email → id`）。
 *
 * 给「手上只有 `accountSeeds[...]`（静态表，无 `nickname`）却要填 `accountName`」的
 * 调用点用 —— 与 `account.nickname ?? account.email ?? account.id` 同一条降级链，
 * 只是把第一环换成**现取**词表的昵称。
 */
function demoAccountName(account: DemoAccountSeed): string {
  return t(DEMO_ACCOUNT_KEYS[account.id].nickname) || account.email || account.id;
}

/** 演示模式中的临时 CLI 当前账号，仅存在于本次页面会话。 */
/* ★ 2026-10-12：默认指向 **ops-02** 而不是 jackey —— 原型里 jackey 只绑了
   WB + IDE（卡片显示「使用中 2 个工具」），CLI / VSC 都不绑它。 */
let demoActiveCliAccountId = accountSeeds[5].id;

/**
 * 应用级偏好（`app_prefs.json`）的演示态 —— 仅存在于本次页面会话。
 *
 * 为什么要有可变状态：`get_app_prefs` / `set_app_prefs` 是一对**读写同一份数据**的命令，
 * 若 `get` 恒返回常量，演示站里把开关关掉、重开抽屉又变回开 —— 看起来像开关坏了。
 * 后端那一侧的真值是文件，这里就对应「进程内的一份」。
 */
let demoAppPrefs = { close_to_tray: true, legal_consent_at: 0 };

// Counts and relative model roles follow anonymous aggregates from the sanitized local cache.
// No upstream request row or identifier is copied into this fixture.
const usageSeeds: AccountUsageSeed[] = [
  {
    requestCount: 2243,
    models: [
      { model: "deepseek-v4-flash", requestCount: 2133, credit: 1794.39 },
      { model: "kimi-k3-1", requestCount: 24, credit: 2497.16 },
      { model: "deepseek-v4-pro", requestCount: 23, credit: 3.63 },
      { model: "glm-5.2", requestCount: 1, credit: 33.63 },
      { model: "hy3", requestCount: 62, credit: 0 },
    ],
  },
  {
    requestCount: 679,
    models: [
      { model: "deepseek-v4-flash", requestCount: 659, credit: 1270.62 },
      { model: "hy3", requestCount: 20, credit: 0 },
    ],
  },
  {
    requestCount: 318,
    models: [
      { model: "deepseek-v4-flash", requestCount: 309, credit: 595.08 },
      { model: "hy3", requestCount: 9, credit: 0 },
    ],
  },
];

/**
 * 演示数据的单账号明细上限，与后端 `OFFICIAL_USAGE_DETAIL_LIMIT` 保持一致。
 *
 * 夹具必须按同一上限生成行数，否则演示页会出现「提示说只截断了 N 条、实际却只给 100 行」
 * 这种自相矛盾的状态（截图会被当成真实界面参考）。
 */
const DEMO_DETAIL_LIMIT = 3000;

/** 积分包名称的键（三组账号用同一批名字，数值不同 —— 值留在 `creditPackageSeeds`）。 */
const PACK_FISSION: TranslationKey = "shared.demo.pack.fission";
const PACK_CREDITS: TranslationKey = "shared.demo.pack.credits";
const PACK_TRIAL: TranslationKey = "shared.demo.pack.trial";
const PACK_CHECKIN: TranslationKey = "shared.demo.pack.checkin";
const PACK_ACTIVITY: TranslationKey = "shared.demo.pack.activity";

/** `[包名键, 总量, 剩余, 距到期天数]` */
/**
 * 每个账号的积分包（`[名称键, 总量, 剩余, 到期天数][]`）—— 与原型逐账号对应。
 *
 * 卡片的「积分 `剩余`/`总量`」= 本行的求和；「`N` 积分 `M` 天后过期」= 行内
 * `expiringSoon`（≤7 天）那部分之和。jackey / qa-bot 刻意拆成**两个包**：
 * 这样「临期 320」与「总额 2000」能同时成立（单包做不到）。
 */
const creditPackageSeeds: [TranslationKey, number, number, number][][] = [
  // jackey：积分 1,240/2,000，其中 320 三天后到期（原型 `expiring: 320, expireInDays: 3`）
  [
    [PACK_CREDITS, 500, 320, 3],
    [PACK_FISSION, 1500, 920, 30],
  ],
  [[PACK_CREDITS, 1000, 860, 21]],   // team-shared 860/1,000
  // backup-01 1,580/2,000（两个包 ⇒ 抽屉里的逐包明细仍有多个包名可看）
  [
    [PACK_CREDITS, 1500, 1180, 44],
    [PACK_TRIAL, 500, 400, 44],
  ],
  [[PACK_CREDITS, 2000, 120, 2]],    // qa-bot     120/2,000（全部两天后到期）
  [[PACK_CREDITS, 1000, 430, 12]],   // design-lab 430/1,000
  // ops-02 990/2,000
  [
    [PACK_CREDITS, 1800, 890, 27],
    [PACK_CHECKIN, 200, 100, 27],
  ],
  [[PACK_CREDITS, 3000, 2050, 58]],  // spare-global 2,050/3,000
  // demo 640/1,000
  [
    [PACK_CREDITS, 700, 440, 9],
    [PACK_ACTIVITY, 300, 200, 9],
  ],
];

function startOfToday(): Date {
  const date = new Date();
  date.setHours(0, 0, 0, 0);
  return date;
}

function localDate(daysAgo: number): string {
  const date = startOfToday();
  date.setDate(date.getDate() - daysAgo);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function atLocalTime(daysAgo: number, hour: number, minute: number): number {
  const date = startOfToday();
  date.setDate(date.getDate() - daysAgo);
  date.setHours(hour, minute, 0, 0);
  return date.getTime();
}

function futureAt(daysAhead: number, hour = 23, minute = 59): number {
  const date = startOfToday();
  date.setDate(date.getDate() + daysAhead);
  date.setHours(hour, minute, 0, 0);
  return date.getTime();
}

function hydratedAccounts(): AccountMeta[] {
  return withDemoNames(accountSeeds).map((account, index) => ({
    ...account,
    /* ⚠️ 种子给了**非 0** 的 `expiresAt` 就尊重它 —— 演示「凭据已过期」需要它落在过去，
       而此前这里无条件覆盖成未来时刻 ⇒ 那个状态**永远演示不出来**。 */
    expiresAt: account.expiresAt !== 0 && account.expiresAt !== null ? account.expiresAt : futureAt(12 + index * 5, 18, 30),
    refreshExpiresAt: futureAt(40 + index * 7),
    refreshedAt: atLocalTime(0, 9, 12 + index * 7),
    createdAt: atLocalTime(45 + index * 19, 10, 0),
  }));
}

function creditExpiry(accountId: string): CreditExpiry {
  const index = Math.max(0, accountSeeds.findIndex((account) => account.id === accountId));
  const account = accountSeeds[index] ?? accountSeeds[0];
  const resources = creditPackageSeeds[index].map(([nameKey, total, remaining, expireDays], packageIndex) => ({
    packageCode: `demo-package-${index + 1}-${packageIndex + 1}`,
    packageName: t(nameKey),
    total,
    remaining,
    used: Number((total - remaining).toFixed(2)),
    status: 1,
    expireAt: futureAt(expireDays, 0, 0),
    expired: false,
    expiringSoon: expireDays <= 7,
  }));
  const totalCapacity = resources.reduce((sum, resource) => sum + resource.total, 0);
  const totalRemaining = resources.reduce((sum, resource) => sum + resource.remaining, 0);
  const expiringSoonRemaining = resources.filter((resource) => resource.expiringSoon).reduce((sum, resource) => sum + resource.remaining, 0);

  return {
    ok: true,
    accountId: account.id,
    accountName: demoAccountName(account),
    updatedAt: Date.now() - (index + 1) * 4 * 60 * 1000,
    totalCapacity,
    totalRemaining: Number(totalRemaining.toFixed(2)),
    expiringSoonRemaining: Number(expiringSoonRemaining.toFixed(2)),
    expiredRemaining: 0,
    soonestExpireAt: Math.min(...resources.map((resource) => resource.expireAt)),
    expiringSoon: expiringSoonRemaining > 0,
    expired: false,
    resources,
  };
}

function dailyWeight(accountIndex: number, dayIndex: number): number {
  const weekdayWave = [0.72, 1.08, 0.93, 1.22, 0.84, 1.16, 1.01][dayIndex % 7];
  const quiet = (dayIndex + accountIndex * 4) % 13 === 0 ? 0.16 : 1;
  return weekdayWave * quiet * (1 + accountIndex * 0.035);
}

function distributeModels(seed: AccountUsageSeed, accountIndex: number) {
  const weights = Array.from({ length: 30 }, (_, dayIndex) => dailyWeight(accountIndex, dayIndex));
  const weightTotal = weights.reduce((sum, weight) => sum + weight, 0);
  const countSeries = seed.models.map((model) => {
    const raw = weights.map((weight) => (model.requestCount * weight) / weightTotal);
    const values = raw.map(Math.floor);
    let remaining = model.requestCount - values.reduce((sum, value) => sum + value, 0);
    const byFraction = raw.map((value, index) => ({ index, fraction: value - Math.floor(value) })).sort((left, right) => right.fraction - left.fraction);
    for (let index = 0; index < remaining; index += 1) values[byFraction[index].index] += 1;
    return values;
  });
  const creditSeries = seed.models.map((model) => {
    const values = weights.map((weight) => Number(((model.credit * weight) / weightTotal).toFixed(2)));
    const drift = Number((model.credit - values.reduce((sum, value) => sum + value, 0)).toFixed(2));
    values[values.length - 1] = Number((values[values.length - 1] + drift).toFixed(2));
    return values;
  });
  return Array.from({ length: 30 }, (_, dayIndex) => {
    const models = seed.models.map((model, modelIndex) => ({
      model: model.model,
      requestCount: countSeries[modelIndex][dayIndex],
      credit: creditSeries[modelIndex][dayIndex],
    }));
    return {
      date: localDate(29 - dayIndex),
      usage: Number(models.reduce((sum, model) => sum + model.credit, 0).toFixed(2)),
      models,
    };
  });
}

function sumModels(rows: { models: CreditOfficialUsageModel[] }[]): CreditOfficialUsageModel[] {
  const totals = new Map<string, CreditOfficialUsageModel>();
  for (const row of rows) {
    for (const model of row.models) {
      const current = totals.get(model.model) ?? { model: model.model, requestCount: 0, credit: 0 };
      current.requestCount += model.requestCount;
      current.credit = Number((current.credit + model.credit).toFixed(2));
      totals.set(model.model, current);
    }
  }
  return [...totals.values()].sort((left, right) => right.credit - left.credit);
}

function visibleRequests(accountIndex: number) {
  const account = accountSeeds[accountIndex];
  /* ⚠️ **取模**：`accountSeeds`（与原型逐账号对应的 8 条）比 `usageSeeds`（3 条用量模板）多，
     直接下标会越界 ⇒ 演示构建下 WB 积分页整页报
     `Cannot read properties of undefined (reading 'requestCount')`。
     用量模板是**演示素材**，按账号循环复用即可（真实数据来自后端）。 */
  const seed = usageSeeds[accountIndex % usageSeeds.length];
  const hours = [16, 15, 17, 14, 1, 0, 3];
  const flashCredits = [0.13, 0.04, 0.2, 1, 0.08, 3.99, 0.45, 8.5, 24.56];
  const kimiCredits = [86.4, 103.2, 112.8, 128.4, 74.6];
  const proCredits = [0.04, 0.13, 0.2, 0.45];
  // 与后端一致：单账号最多下发 DEMO_DETAIL_LIMIT 条明细（按请求时间倒序取最近 N 条）。
  const rowCount = Math.min(seed.requestCount, DEMO_DETAIL_LIMIT);
  const weightedModels = seed.models.flatMap((model) =>
    Array.from({ length: Math.max(1, Math.round((model.requestCount / seed.requestCount) * rowCount)) }, () => model.model),
  );

  return Array.from({ length: rowCount }, (_, rowIndex) => {
    // 行数随账号规模放大后，日期要摊在 31 天窗口内（原来 100 行时 /8 只用到第 12 天）。
    const daysAgo = Math.floor((rowIndex * 30) / rowCount);
    const hour = hours[(rowIndex + accountIndex * 2) % hours.length];
    const minute = (rowIndex * 7 + accountIndex * 11) % 60;
    const ts = new Date(atLocalTime(daysAgo, hour, minute));
    const model = rowIndex < seed.models.length
      ? seed.models[rowIndex].model
      : weightedModels[rowIndex % weightedModels.length];
    const credit = model === "hy3"
      ? 0
      : model === "kimi-k3-1"
        ? kimiCredits[(rowIndex + accountIndex) % kimiCredits.length]
        : model === "glm-5.2"
          ? 33.63
          : model === "deepseek-v4-pro"
            ? proCredits[(rowIndex + accountIndex) % proCredits.length]
            : flashCredits[(rowIndex + accountIndex * 3) % flashCredits.length];
    return {
      accountId: account.id,
      accountName: demoAccountName(account),
      requestId: `demo-request-${String(accountIndex + 1).padStart(2, "0")}-${String(rowIndex + 1).padStart(4, "0")}`,
      credit,
      model,
      client: rowIndex % 50 === 0 ? "CodeBuddyIDE" : "CLI",
      requestTime: `${localDate(daysAgo)} ${String(ts.getHours()).padStart(2, "0")}:${String(ts.getMinutes()).padStart(2, "0")}:00`,
    };
  });
}

function buildStatistics(): CreditStatistics {
  const demoAccounts = hydratedAccounts();
  /* ⚠️ 逐账号都要有日序列（账号数 > 用量模板数，见 `visibleRequests` 的说明）——
     按模板循环复用，否则 `accountDaily[index]` 越界（`reading 'length'`）。 */
  const accountDaily = demoAccounts.map((_, index) =>
    distributeModels(usageSeeds[index % usageSeeds.length], index),
  );
  const daily = accountDaily[0].map((_, dayIndex) => {
    const models = new Map<string, CreditOfficialUsageModel>();
    for (const rows of accountDaily) {
      for (const model of rows[dayIndex].models) {
        const current = models.get(model.model) ?? { model: model.model, requestCount: 0, credit: 0 };
        current.requestCount += model.requestCount;
        current.credit = Number((current.credit + model.credit).toFixed(2));
        models.set(model.model, current);
      }
    }
    const modelRows = [...models.values()];
    return { date: accountDaily[0][dayIndex].date, usage: Number(modelRows.reduce((sum, model) => sum + model.credit, 0).toFixed(2)), models: modelRows };
  });
  const sumRecent = (rows: { usage: number }[], count: number) => Number(rows.slice(-count).reduce((sum, row) => sum + row.usage, 0).toFixed(2));
  const monthPrefix = localDate(0).slice(0, 7);
  const sumMonth = (rows: { date: string; usage: number }[]) => Number(rows.filter((row) => row.date.startsWith(monthPrefix)).reduce((sum, row) => sum + row.usage, 0).toFixed(2));
  const generatedAt = Date.now() - 3 * 60 * 1000;
  const creditRows = accountSeeds.map((account) => creditExpiry(account.id));
  const officialAccounts = demoAccounts.map((account, index) => {
    const seed = usageSeeds[index % usageSeeds.length];
    return {
      accountId: account.id,
      accountName: account.nickname ?? account.email ?? account.id,
      ok: true,
      requestCount: seed.requestCount,
      detailTruncated: seed.requestCount > DEMO_DETAIL_LIMIT,
      usageToday: accountDaily[index][accountDaily[index].length - 1]?.usage ?? 0,
      usage7Days: sumRecent(accountDaily[index], 7),
      usageThisMonth: sumMonth(accountDaily[index]),
      reportedTotal: seed.requestCount,
      fetchedCount: seed.requestCount,
      models: sumModels(accountDaily[index]),
      daily: accountDaily[index],
    };
  });
  const usageToday = daily[daily.length - 1]?.usage ?? 0;
  const usage7Days = sumRecent(daily, 7);
  const usageThisMonth = sumMonth(daily);
  const totalRemaining = creditRows.reduce((sum, credit) => sum + (credit.totalRemaining ?? 0), 0);
  const totalCapacity = creditRows.reduce((sum, credit) => sum + (credit.totalCapacity ?? 0), 0);

  return {
    generatedAt,
    retentionDays: 90,
    coverageStartAt: atLocalTime(29, 0, 0),
    summary: { currentRemaining: Number(totalRemaining.toFixed(2)), currentCapacity: totalCapacity, usageToday, usage7Days, usageThisMonth, todayCheckedInAccounts: 3, todaySuccess: 2, todayAlready: 1, todayFailed: 0 },
    daily,
    accounts: demoAccounts.map((account, index) => ({
      accountId: account.id,
      accountName: account.nickname ?? account.email ?? account.id,
      isCurrent: index === 0,
      currentRemaining: creditRows[index].totalRemaining ?? null,
      totalCapacity: creditRows[index].totalCapacity ?? null,
      lastSnapshotAt: generatedAt - index * 120_000,
      usageToday: officialAccounts[index].usageToday ?? 0,
      usage7Days: officialAccounts[index].usage7Days ?? 0,
      usageThisMonth: officialAccounts[index].usageThisMonth ?? 0,
      checkedInToday: true,
      checkinStatusToday: index === 1 ? "already" : "success",
      lastCheckinAt: atLocalTime(0, 8, 6 + index * 9),
      lastCheckinResult: index === 1 ? "already" : "success",
      daily: accountDaily[index],
    })),
    events: demoAccounts.map((account, index) => ({ kind: "checkin" as const, ts: atLocalTime(0, 8, 6 + index * 9), date: localDate(0), accountId: account.id, accountName: account.nickname ?? account.email ?? account.id, result: index === 1 ? "already" : "success" })),
    officialUsage: {
      status: "complete",
      rangeStart: localDate(29),
      rangeEnd: localDate(0),
      collectedAt: generatedAt,
      summary: { usageToday, usage7Days, usageThisMonth },
      daily,
      accounts: officialAccounts,
      requests: accountSeeds.flatMap((_, index) => visibleRequests(index)),
      models: sumModels(daily),
      detailLimitPerAccount: DEMO_DETAIL_LIMIT,
      errors: [],
    },
  };
}

function checkinConfig(): CheckinConfig {
  return { enabled: true, keepalive_days: 7, lazy_refresh_hours: 12 };
}

function travelConfig(): TravelConfig {
  return { enabled: true };
}

function travelStatus(_accountId: string): TravelStatus {
  /* ★ 2026-10-12：演示统一返回 `untraveled`（**默认态**，卡片不渲染 chip）。
     原型 `SCOPES["wb-cn"]` 里没有旅行概念，而此前这里按 `index % 3` 造出
     「旅行中 / 已结束 / 无 Buddy」⇒ 每张卡都多一枚标签，chips 行折成两行、
     卡片比原型高 25px，目视无法逐卡对照。 */
  return { label: "untraveled", rewardCredit: null, locationName: null, arriveAt: null };
}

function rotateConfig(): AutoRotateConfig {
  return { enabled: true, check_interval_minutes: 15, cooldown_minutes: 120, min_gap_hours: 24, min_urgency_hours: 72, active_guard_minutes: 30, min_remaining_credits: 50 };
}

function checkinLogs(): CheckinLog[] {
  return hydratedAccounts().flatMap((account, accountIndex) => [0, 1, 2].map((daysAgo) => ({ ts: atLocalTime(daysAgo, 8, 6 + accountIndex * 9), accountId: account.id, email: account.nickname ?? account.email ?? account.id, result: accountIndex === 1 && daysAgo === 0 ? "already" : "success" })));
}

function rotateLogs(): RotateLog[] {
  return [
    { ts: atLocalTime(0, 9, 30), action: "skipped", reason: t("shared.demo.rotate.reasonPinned"), from: { id: accountSeeds[0].id, name: t("shared.demo.account.a") }, to: null },
    { ts: atLocalTime(1, 16, 20), action: "switched", reason: t("shared.demo.rotate.reasonExpiring"), from: { id: accountSeeds[1].id, name: t("shared.demo.account.b") }, to: { id: accountSeeds[0].id, name: t("shared.demo.account.a") } },
  ];
}

/**
 * 演示分组的一行。
 *
 * `credit` 按**实测比例**从 token 折算（≈ 34K token / 1 积分，实测自本机 jsonl：
 * `44,276 token → credit 1.28`）。演示站拿不到真实 `usage.credit`，但**不能留 0** ——
 * 积分视图的「客户端账本」会整块显示 0，与真实后端（有值）的样子不一致，
 * 评审就会以为「客户端没有积分」是真结论。
 */
function demoTokenTotals(input: number, output: number, cacheRead: number, cacheWrite: number, records: number): TokenStatsTotals {
  const total = input + output + cacheWrite;
  return {
    total,
    input,
    output,
    cacheRead,
    cacheWrite,
    uncachedInput: Math.max(0, input - cacheRead),
    records,
    cacheHitRate: input > 0 ? cacheRead / input : null,
    credit: Math.round((total / 34_000) * 1000) / 1000,
  };
}

function demoTokenGroup(key: string, input: number, output: number, cacheRead: number, cacheWrite: number, records: number): TokenStatsGroup {
  return { key, ...demoTokenTotals(input, output, cacheRead, cacheWrite, records) };
}

function demoTokenSession(key: string, title: string, project: string, input: number, output: number, cacheRead: number, cacheWrite: number, records: number): TokenStatsGroup {
  const keyParts = key.split(" · ");
  return { ...demoTokenGroup(key, input, output, cacheRead, cacheWrite, records), title, project, sessionId: keyParts[keyParts.length - 1] };
}

/**
 * 会话在演示日轴上的活动跨度（偏移区间，闭区间；偏移 0 = 今天）。
 *
 * ⚠️ 刻意让第 4 个会话**不含今天**（只活跃在 8–13 天前）⇒ 切「今天 / 近 7 天」时
 * 它会从窗口里**整个消失**，演示能覆盖「会话集合随窗口变化」与「筛没」空态路径；
 * 其余三个跨度不同 ⇒ 「近 7 天」与「近 30 天」的集合也不同（2 vs 4）。
 */
const SESSION_DAY_SPANS: [number, number][] = [
  [0, 13], // 全程活跃（含今天）
  [1, 8], // 从昨天开始
  [3, 9],
  [8, 13], // 只在 8–13 天前活跃：今天 / 近 7 天窗口里不出现
];

/**
 * 会话的**逐日拆分**（演示数据）—— 五个字段按同一组权重分配，**和恒等于种子总量**
 * （余数补在最近一天；有 token 的天至少 1 条记录）。
 *
 * ⚠️ 没有它，演示站的「会话」视图就退回全时口径（`sessionPoints` 对缺失 `daily` 的
 * 回落，供旧后端用）——切「今天 / 近 7 天」时统计纹丝不动，恰是用户报障的那个形态。
 */
function demoSessionDaily(
  seedIndex: number,
  input: number,
  output: number,
  cacheRead: number,
  cacheWrite: number,
  records: number,
): TokenStatsGroup[] {
  const [start, end] = SESSION_DAY_SPANS[seedIndex % SESSION_DAY_SPANS.length];
  const offsets = Array.from({ length: end - start + 1 }, (_, index) => start + index);
  const weights = offsets.map((_, index) => offsets.length - index); // 越靠今天越重
  const weightSum = weights.reduce((sum, weight) => sum + weight, 0);
  const split = (value: number): number[] => {
    const parts = weights.map((weight) => Math.floor((value * weight) / weightSum));
    parts[0] += value - parts.reduce((sum, part) => sum + part, 0); // 余数补最近一天
    return parts;
  };
  const inputs = split(input);
  const outputs = split(output);
  const reads = split(cacheRead);
  const writes = split(cacheWrite);
  const recordParts = split(records);
  for (let index = 1; index < offsets.length; index += 1) {
    if (inputs[index] > 0 && recordParts[index] === 0) {
      recordParts[index] = 1;
      recordParts[0] = Math.max(0, recordParts[0] - 1);
    }
  }
  return offsets.map((offset, index) =>
    demoTokenGroup(
      localDate(offset),
      inputs[index],
      outputs[index],
      reads[index],
      writes[index],
      recordParts[index],
    ),
  );
}

/**
 * 演示会话标题（**存键不存文案**）。
 *
 * 结构：`[会话 key, 标题键, 项目名, 输入, 输出, 缓存读, 缓存写, 记录数]`。
 * 会话 key 与项目名保持英文 —— 它们模拟的是真实的仓库 / 会话标识。
 * 标题的 `t()` 在 `demoTokenSource` 内现取，故切语言时跟着变。
 */
const tokenSessionSeeds: [string, TranslationKey, string, number, number, number, number, number][] = [
  ["buddy-switch-rust · token-stats-dashboard", "shared.demo.session.tokenDashboard", "buddy-switch-rust", 18_700_000, 1_050_000, 16_100_000, 160_000, 72],
  ["my-code-teams · settings-agent-acp", "shared.demo.session.agentAcp", "my-code-teams", 13_200_000, 890_000, 11_300_000, 120_000, 55],
  ["LetterTotTown · character-audio", "shared.demo.session.characterAudio", "LetterTotTown", 8_600_000, 640_000, 7_200_000, 80_000, 38],
  ["buddy-switch-rust · account-card-redesign", "shared.demo.session.accountCard", "buddy-switch-rust", 6_300_000, 410_000, 5_400_000, 50_000, 29],
];

function demoTokenSource(source: TokenStatsSource["source"], scale: number): TokenStatsSource {
  const daily = Array.from({ length: 14 }, (_, index) => {
    const wave = [0.62, 0.86, 1.1, 0.72, 1.3, 0.94, 0.38][index % 7] * scale;
    return demoTokenGroup(localDate(13 - index), Math.round(7_600_000 * wave), Math.round(480_000 * wave), Math.round(6_650_000 * wave), Math.round(95_000 * wave), Math.round(24 * wave));
  });
  const summary = daily.reduce((sum, row) => demoTokenTotals(sum.input + row.input, sum.output + row.output, sum.cacheRead + row.cacheRead, sum.cacheWrite + row.cacheWrite, sum.records + row.records), demoTokenTotals(0, 0, 0, 0, 0));
  const hours = Array.from({ length: 7 * 24 }, (_, index) => {
    const day = Math.floor(index / 24); const hour = index % 24;
    const active = Math.max(0.02, Math.exp(-Math.pow(hour - (day >= 5 ? 22 : 15), 2) / 22));
    return demoTokenGroup(`${day}-${hour}`, Math.round(720_000 * active * scale), Math.round(41_000 * active * scale), Math.round(610_000 * active * scale), 0, Math.max(1, Math.round(6 * active * scale)));
  });
  const projects = [
    demoTokenGroup("buddy-switch-rust", 42_800_000 * scale, 2_400_000 * scale, 37_100_000 * scale, 420_000 * scale, Math.round(148 * scale)),
    demoTokenGroup("my-code-teams", 25_600_000 * scale, 1_650_000 * scale, 21_900_000 * scale, 260_000 * scale, Math.round(96 * scale)),
    demoTokenGroup("LetterTotTown", 11_900_000 * scale, 920_000 * scale, 9_700_000 * scale, 110_000 * scale, Math.round(51 * scale)),
  ];
  const models = [
    demoTokenGroup("deepseek-v4-flash", 56_400_000 * scale, 3_200_000 * scale, 49_100_000 * scale, 530_000 * scale, Math.round(210 * scale)),
    demoTokenGroup("kimi-k3-1", 17_300_000 * scale, 1_140_000 * scale, 14_200_000 * scale, 180_000 * scale, Math.round(61 * scale)),
    demoTokenGroup("glm-5.2", 6_600_000 * scale, 630_000 * scale, 5_400_000 * scale, 80_000 * scale, Math.round(24 * scale)),
  ];
  // 会话标题在**这里**取（`t(key)`），而不是写进模块级常量 —— 常量会在切语言时定格。
  const sessions = tokenSessionSeeds.map(([key, titleKey, project, input, output, cacheRead, cacheWrite, records], index) => {
    const scaled = {
      input: input * scale,
      output: output * scale,
      cacheRead: cacheRead * scale,
      cacheWrite: cacheWrite * scale,
      records: Math.round(records * scale),
    };
    return {
      ...demoTokenSession(key, t(titleKey), project, scaled.input, scaled.output, scaled.cacheRead, scaled.cacheWrite, scaled.records),
      // 逐日拆分：与 `daily` 同一演示日轴（近 14 天，偏移 0 = 今天）。
      daily: demoSessionDaily(index, scaled.input, scaled.output, scaled.cacheRead, scaled.cacheWrite, scaled.records),
    };
  });
  /**
   * 逐模型 × 逐日 —— 堆叠柱 / 模型环形 / 排行迷你趋势线的**唯一**数据源。
   *
   * ⚠️ 缺了它，原型的「每日 Token · 按模型拆分」「模型占比」「模型用量排行」
   * 在演示站会**整块消失**（`modelSlices` 只认 `dailyByModel`，按 A2 的裁定
   * **不回落** `models[].total` —— 那是全时段累计，口径不同）。
   * 比例之和 = 1 ⇒ 每个模型的逐日和与总量口径一致。
   */
  const modelShares: [string, number][] = [
    ["deepseek-v4-flash", 0.62],
    ["kimi-k3-1", 0.24],
    ["glm-5.2", 0.14],
  ];
  const dailyByModel: Record<string, TokenStatsGroup[]> = {};
  for (const [name, share] of modelShares) {
    dailyByModel[name] = daily.map((row) =>
      demoTokenGroup(
        row.key,
        Math.round(row.input * share),
        Math.round(row.output * share),
        Math.round(row.cacheRead * share),
        Math.round(row.cacheWrite * share),
        Math.max(1, Math.round(row.records * share)),
      ),
    );
  }
  const now = Date.now();
  return { source, summary, models, projects, sessions, daily, dailyByModel, hours, filesScanned: source === "workbuddy" ? 63 : source === "codebuddy-ide" ? 17 : 41, parseErrors: 0, coverageStartAt: now - 13 * 86_400_000, coverageEndAt: now };
}

function demoTokenStatistics(days?: number): TokenStatistics {
  return { generatedAt: Date.now(), rangeDays: days ?? null, sources: [demoTokenSource("workbuddy", 1), demoTokenSource("codebuddy-cli", 0.58), demoTokenSource("codebuddy-ide", 0.36)] };
}

// ---------------------------------------------------------------------------
// API 网关（演示数据，只读）
// ---------------------------------------------------------------------------

function demoRegionStatus(region: Region): AppStatus {
  const demoAccounts = hydratedAccounts();
  const isGlobal = region === "global";
  const account = isGlobal ? demoAccounts[1] : demoAccounts[0];
  return {
    running: true,
    authFile: isGlobal
      ? "/demo/WorkBuddy AI/auth/workbuddy-desktop-ai.info"
      : "/demo/WorkBuddy/auth/workbuddy-desktop.info",
    current: { uid: account.uid, nickname: account.nickname, email: account.email },
    appPath: isGlobal ? "/demo/WorkBuddy AI.app" : "/demo/WorkBuddy.app",
    /* ⚠️ 这是**本应用**的版本号（真机取自 `update::APP_VERSION`），侧栏与设置抽屉都显示它。
       演示值跟发行线走：截图会被直接放进 README，写着半年前的版本会让人以为项目停更了。 */
    version: "2026.10.16",
    installed: true,
    regionMismatch: null,
    region,
  };
}

function demoGatewayConfig(): GatewayConfig {
  return {
    enabled: true,
    bind_addr: "127.0.0.1",
    port: 57891,
    allow_non_loopback: false,
    log_keep: 200,
    log_bodies: false,
    per_key_rate_limit: null,
    max_context: true,
  };
}

/**
 * 网关状态（演示）。
 *
 * ⚠️ 池数据必须与真实后端**同形**（`pools` 两个版本键恒定存在、`accounts` 带 `realm`、
 * `diagnose` 逐账号一行）——演示图/截图走的就是这条数据，形状错了会掩盖真实缺陷。
 * WorkBuddy 侧**不产出 `expired`**（池不存积分有效期），演示数据也不该造。
 */
function demoGatewayStatus(): GatewayStatus {
  const cnA = { uid: "demo-cn-a", name: t("shared.demo.account.a") };
  const cnB = { uid: "demo-cn-b", name: t("shared.demo.account.b") };
  const cnD = { uid: "demo-cn-d", name: t("shared.demo.account.d") };
  const globalC = { uid: "demo-global-c", name: t("shared.demo.account.c") };
  const coolingUntil = Math.floor(Date.now() / 1000) + 5400;
  /* 模型级冷却（演示）：**按上游声明的时刻**冷却（2026-10-29 口径）。
     徽标显示「{time} 解冻」，这个时刻就是上游 `6004` 文案里写的那个 —— 演示数据必须
     与之同形，否则截图会教人以为界面上永远是兜底值。 */
  const modelUpstreamMs = Date.now() + 5 * 60 * 60_000;
  const accounts: GatewayPoolAccount[] = [
    {
      uid: cnA.uid,
      name: cnA.name,
      realm: "cn",
      status: "available",
      credits: 120,
      cooling: false,
      cooldownUntil: null,
      cooldownReason: null,
      disabled: false,
      // 账号可用、但**某个模型**在冷却（正交两维，演示这张卡最容易画错的地方）。
      modelCoolings: { "deepseek-v4.1-flash": modelUpstreamMs },
      modelCooldownDetails: {
        "deepseek-v4.1-flash": {
          untilMs: modelUpstreamMs,
          resetAtMs: modelUpstreamMs,
          upstreamResetAtMs: modelUpstreamMs,
          fallback: false,
          reason: "6004 model rate limit (probe)",
          hits: 0,
        },
      },
    },
    {
      uid: cnD.uid,
      name: cnD.name,
      realm: "cn",
      status: "available",
      credits: 96,
      cooling: false,
      cooldownUntil: null,
      cooldownReason: null,
      disabled: false,
      /* 演示「可用但在途已满」：状态照旧是 `available`（账号没问题），
         计数那一格会说「其中 1 个在途已满」。这正是 2026-10-17 那个
         「4 个可用却一个都跑不起来」报障的观感版本。 */
      inFlight: 3,
    },
    {
      uid: cnB.uid,
      name: cnB.name,
      realm: "cn",
      status: "cooling",
      credits: 64.5,
      cooling: true,
      cooldownUntil: coolingUntil,
      cooldownReason: t("shared.demo.pool.reasonRateLimited"),
      disabled: false,
    },
    {
      uid: globalC.uid,
      name: globalC.name,
      realm: "global",
      status: "disabled",
      credits: 8,
      cooling: false,
      cooldownUntil: null,
      cooldownReason: t("shared.demo.pool.reasonSessionDead"),
      disabled: true,
    },
  ];
  const diagnose = accounts.map((account) =>
    t("shared.demo.pool.diagnose", {
      name: account.name,
      tail: account.uid.slice(-8),
      reason: account.cooldownReason ?? t("shared.poolStatus.available"),
      credits: account.credits ?? t("shared.demo.pool.creditsUnknown"),
    }),
  );
  return {
    enabled: true,
    running: true,
    addr: "127.0.0.1",
    port: 57891,
    allowNonLoopback: false,
    baseUrl: "http://127.0.0.1:57891/v1",
    lastError: null,
    pools: {
      // cn：3 个号 = 2 可用（其中一个在途已满）+ 1 冷却。
      // ★ `inFlightFull` 是**正交**计数，已含在 `available` 里 —— 与真实后端同形。
      // ★ `modelCooling` 同样正交：cnA 可用但 `deepseek-v4.1-flash` 在冷却。
      cn: {
        total: 3,
        available: 2,
        cooling: 1,
        disabled: 0,
        expired: 0,
        zeroCredits: 0,
        excluded: 0,
        inFlightFull: 1,
        modelCooling: 1,
        totalCredits: 280.5,
      },
      global: { total: 1, available: 0, cooling: 0, disabled: 1, expired: 0, zeroCredits: 0, totalCredits: 8 },
    },
    accounts,
    diagnose,
  };
}

function demoApiKeys(): ApiKeyRecord[] {
  /* ⚠️ 必须带 `suffix`（尾部 4 位）：真实后端自 E3 起**每把新 Key 都产出**它，
     列表按 `prefix••••••••suffix` 渲染。夹具缺它 ⇒ 演示站显示 `sk-wb-a1b2…`
     而真机显示 `sk-wb-a1b2••••••••7d31` —— 那正是「演示数据自造结构」的红线。 */
  return [
    { id: "demo-key-1", name: "Cursor", region: "cn", prefix: "sk-wb-a1b2", suffix: "7d31", createdAt: atLocalTime(3, 17, 3), revokedAt: null, revoked: false, lastUsedAt: atLocalTime(0, 17, 10) },
    { id: "demo-key-2", name: "Claude Code", region: "global", prefix: "sk-wb-c3d4", suffix: "9e02", createdAt: atLocalTime(3, 17, 5), revokedAt: null, revoked: false, lastUsedAt: atLocalTime(0, 16, 41) },
    { id: "demo-key-3", name: "OpenWebUI", region: "cn", prefix: "sk-wb-e5f6", suffix: "b4a8", createdAt: atLocalTime(4, 10, 0), revokedAt: atLocalTime(1, 12, 30), revoked: true, lastUsedAt: atLocalTime(2, 9, 12) },
  ];
}

function demoCatalog(region: Region): CatalogSnapshot {
  const models = region === "global"
    ? ["GPT-5.6", "Claude-Sonnet-4.5", "Gemini-3-Pro", "GLM-5.3", "DeepSeek-V4-Pro"]
    : ["GLM-5.3", "GLM-5.2", "DeepSeek-V4-Pro", "Kimi-K3", "hy3"];
  /* 能力面（issue #65）：「思考档位 / 积分倍率」两列。演示站**必须有值** ——
     否则新加的两列在截图里全是「—」，看起来像没做。数值取真机目录的典型值，
     `hy3` 刻意给倍率 0（= 免费），让「免费」这一档也能在演示里看到。 */
  const ability: Record<string, { rate: number; efforts: string[]; defaultEffort: string | null }> = {
    "GPT-5.6": { rate: 3.31, efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "high" },
    "Claude-Sonnet-4.5": { rate: 1.33, efforts: ["low", "high"], defaultEffort: "high" },
    "Gemini-3-Pro": { rate: 0.99, efforts: ["low", "high", "max"], defaultEffort: "high" },
    "GLM-5.3": { rate: 0.79, efforts: ["low", "high", "max"], defaultEffort: "high" },
    "GLM-5.2": { rate: 0.11, efforts: ["high", "xhigh"], defaultEffort: "high" },
    "DeepSeek-V4-Pro": { rate: 0.29, efforts: ["low", "high", "xhigh"], defaultEffort: "high" },
    "Kimi-K3": { rate: 0.03, efforts: ["medium"], defaultEffort: null },
    "hy3": { rate: 0, efforts: [], defaultEffort: null },
  };
  return {
    region,
    source: region === "global" ? "cached" : "live",
    fetched_at: Date.now() - 3 * 60 * 1000,
    models: models.map((name, index) => {
      const info = ability[name] ?? { rate: 0.2, efforts: [], defaultEffort: null };
      return {
        id: name.toLowerCase(),
        name,
        context_window: 131072,
        max_tokens: 8192,
        supports_images: index % 3 === 0,
        credits: null,
        credits_rate: info.rate,
        badges: index === 0 ? [t("shared.demo.catalog.promo")] : [],
        // 免费 = 倍率为 0（与后端 `is_free()` 同判据，别再各算一套）。
        free: info.rate === 0,
        reasoning_efforts: info.efforts,
        default_effort: info.defaultEffort,
        efforts_source: info.efforts.length > 0 ? "static" : null,
      };
    }),
    note: region === "global" ? t("shared.demo.catalog.globalStale") : null,
  };
}

function demoStrategyMap(): AccountStrategyMap {
  const demoAccounts = withDemoNames(hydratedAccounts());
  return {
    cn: { region: "cn", strategy: { kind: "current" }, selected: demoAccounts[0] },
    global: { region: "global", strategy: { kind: "max_credits" }, selected: null, note: t("shared.demo.strategy.realtime") },
  };
}

function demoGatewayLogs(): GatewayLogEntry[] {
  return [
    { ts: atLocalTime(0, 17, 10), endpoint: "/v1/chat/completions", method: "POST", region: "cn", account: t("shared.demo.account.a"), model: "GLM-5.3", status: 200, latencyMs: 1200, promptTokens: 800, completionTokens: 434, stream: true },
    { ts: atLocalTime(0, 17, 9), endpoint: "/v1/messages", method: "POST", region: "global", account: t("shared.demo.account.b"), model: "GPT-5.6", status: 402, latencyMs: 800, promptTokens: null, completionTokens: null, stream: true },
    { ts: atLocalTime(0, 17, 7), endpoint: "/v1/chat/completions", method: "POST", region: "cn", account: t("shared.demo.account.a"), model: "DeepSeek-V4-Pro", status: 200, latencyMs: 2400, promptTokens: 2100, completionTokens: 900, stream: true },
    { ts: atLocalTime(0, 17, 2), endpoint: "/v1/messages", method: "POST", region: "global", account: t("shared.demo.account.b"), model: "Claude-Sonnet-4.5", status: 429, latencyMs: 300, promptTokens: null, completionTokens: null, stream: true },
  ];
}

// ---------------------------------------------------------------------------
// 会话工作台（P8 T02 · 演示数据，只读）
// ---------------------------------------------------------------------------
//
// ⚠️ 两个 case 的 **uid / sessionId 必须自洽**：`list_session_replicas` 的副本 uid 指向
// `accountSeeds` 的 uid（`demo-user-001` …），会话 id 指向 `list_sessions` 的会话行 ——
// 否则演示站的「账号显示名 / 标题」会落兜底，把 join 的缺陷藏起来。

/** 演示：当前账号的会话行（供会话页 join 标题 / `updatedAt` / `hasHistory`）。 */
function demoSessions(): Session[] {
  return [
    { id: "demo-session-1", title: t("shared.demo.session.replicaAlpha"), cwd: "/demo/projects/alpha", updatedAt: atLocalTime(0, 15, 20), hasHistory: true, isPlayground: false },
    { id: "demo-session-2", title: t("shared.demo.session.replicaBeta"), cwd: "/demo/projects/beta", updatedAt: atLocalTime(1, 9, 5), hasHistory: false, isPlayground: false },
  ];
}

/**
 * 演示：copy ledger 连通出的副本组。
 *
 * 形状与后端 `session::list_replica_groups_for` **逐字段一致**（只回
 * `accountUid` / `sessionId` / `region` + `previewToken` + `baseline` + `content` + `source`）
 * —— 演示站与真机走同一套归一化逻辑，形状不一致会让「演示好看、真机崩」在打包后才暴露。
 *
 * ⚠️ `previewToken` 是**照后端算法真算出来的**（`session::group_fingerprint`），
 * 不是随手编的 hex —— 本仓红线「演示数据不许自造结构」。那两个值与
 * `session.rs::tests::group_fingerprint_values_are_frozen_for_demo_fixtures`
 * **成对钉住**：算法一改，Rust 侧先红，演示夹具才不至于静默失真。
 *
 * ⚠️ `baseline` 刻意留 `null`：基线只在「复制成功 / 分歧统一成功」的那一刻建立，
 * 而演示站里没有发生过任何写操作。**编一条假基线**会让「上次一致点」这块 UI
 * 显示一个假的读数 —— 宁可演示站不展示这个特性，也不撒谎（§3.5）。
 *
 * ⚠️ `content` 由 core 算（前端不再自己派生）⇒ 演示值必须**自洽**（判据见
 * `session::derive_content_status`）：第一组 21 行 vs 18 行且公共前缀 = 18
 * ⇒ `behind`（有人多写了，无分叉）；第二组两侧同为 12 行 ⇒ `latest`。
 * **不臆造**第三种形态（分叉 / 未知）——演示站没有能证明它们的数据。
 */
function demoSessionReplicas(): SessionReplicaResponse {
  return {
    groups: [
      {
        key: "demo-user-001\u001fdemo-session-1",
        replicas: [
          { accountUid: "demo-user-001", sessionId: "demo-session-1", region: "cn" },
          { accountUid: "demo-user-002", sessionId: "demo-session-1-copy", region: "cn" },
        ],
        previewToken: "71186784553e0e0b",
        baseline: null,
        content: {
          status: "behind",
          commonPrefix: 18,
          deltas: { "demo-session-1": 3, "demo-session-1-copy": 0 },
          latestLineCount: 21,
          sinceBaseline: {},
        },
      },
      {
        key: "demo-user-001\u001fdemo-session-2",
        replicas: [
          { accountUid: "demo-user-001", sessionId: "demo-session-2", region: "cn" },
          { accountUid: "demo-user-003", sessionId: "demo-session-2-copy", region: "cn" },
        ],
        previewToken: "7f64fb4bd44b030c",
        baseline: null,
        content: {
          status: "latest",
          commonPrefix: 12,
          deltas: { "demo-session-2": 0, "demo-session-2-copy": 0 },
          latestLineCount: 12,
          sinceBaseline: {},
        },
      },
    ],
    source: "ledger",
  };
}

// ---------------------------------------------------------------------------
// 「今日 Token」按账号聚合（演示数据，只读）
// ---------------------------------------------------------------------------

/**
 * 演示：「今日 Token」按账号聚合（`get_account_token_today`）。
 *
 * 演示数据**刻意覆盖四条诚实边界**（见 `AccountTokenTodayResponse`）：
 * - `demo-user-001`（有真值）+ `demo-user-003`（**真实 0**）出现在 `accounts`；
 * - `demo-user-002` **缺席** ⇒ 界面渲染「—」（今日无记录 / 未知，**不是 0**）；
 * - `gatewayTruncated: true` ⇒ 网关部分是下界 ⇒ 界面渲染 `≥N` 并说明；
 * - `client*` 与 `gateway*` **两侧都非零** ⇒ 明细追加「可能重复计数」说明。
 * 非有限值不会出现在这里（演示数据是「看起来像真的」的样本，不造脏值）。
 */
function demoAccountTokenToday(): AccountTokenTodayResponse {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  /**
   * ★ 2026-10-12：**八个账号全给**、且 `gatewayTruncated: false`。
   *
   * 此前只有两个账号、且 `gatewayTruncated: true` ⇒ 卡片上出现 `今日 —`（缺席）与
   * `今日 ≥N`（下界），而原型是**干净的数值**（`42.1K` / `0`）。
   * 诚实边界的**代码路径**仍由 `formatTodayTokens` 承担（门禁在钉），
   * 但演示数据本身应当与原型逐卡一致，否则无法做 1:1 目视核验。
   */
  return {
    date,
    accounts: [
      { uid: "demo-user-001", clientIn: 28000, clientOut: 9000, gatewayIn: 4000, gatewayOut: 1100 }, // 42.1K
      { uid: "demo-user-002", clientIn: 18000, clientOut: 7000, gatewayIn: 2600, gatewayOut: 800 }, // 28.4K
      { uid: "demo-user-003", clientIn: 0, clientOut: 0, gatewayIn: 0, gatewayOut: 0 }, // 0
      { uid: "demo-user-004", clientIn: 20000, clientOut: 8000, gatewayIn: 2400, gatewayOut: 800 }, // 31.2K
      { uid: "demo-user-005", clientIn: 0, clientOut: 0, gatewayIn: 0, gatewayOut: 0 }, // 0
      { uid: "demo-user-006", clientIn: 6000, clientOut: 2000, gatewayIn: 500, gatewayOut: 200 }, // 8.7K
      { uid: "demo-user-007", clientIn: 0, clientOut: 0, gatewayIn: 0, gatewayOut: 0 }, // 0
      { uid: "demo-user-008", clientIn: 12000, clientOut: 4500, gatewayIn: 1400, gatewayOut: 400 }, // 18.3K
    ],
    unknownClient: { in: 0, out: 0 },
    gatewayTruncated: false,
  };
}

/**
 * 演示：网关**逐日**用量趋势（`get_gateway_token_trend`，只读）。
 *
 * 演示数据**刻意覆盖三条边界**（与 `demoAccountTokenToday` 同一取向）：
 * ① 有量的天（柱子）；② **缺日**（中间空一天 —— 覆盖率披露「已记录 X/N 天」要能看见）；
 * ③ 一天标 `truncated: true` ⇒ 图注追加「1 天为下界」。
 * 尺度与演示账号的今日 Token（42.1K 等）同量级，保证小图看起来不违和。
 */
function demoGatewayTokenTrend(): GatewayTokenTrend {
  const pad = (n: number) => String(n).padStart(2, "0");
  const dateKey = (daysAgo: number) => {
    const date = new Date(Date.now() - daysAgo * 86_400_000);
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  };
  // 索引 0 = 最早一天；`-1` = **未记录**（App 没在跑），要被过滤掉以制造缺口。
  const perDay = [
    18600, 24300, 9800, 31500, 12400, 7600, -1, 41200, 28700, 15300, 22800, 19400, 8600, 33100,
    26800, 11200, 47500, 20900, 16700, 24300, 13800, 39400, 21600, 18400, 9200, 28700, 35600,
    24100, 17900, 26300,
  ];
  const days = perDay
    .map((total, index) => ({ total, key: dateKey(perDay.length - 1 - index) }))
    .filter((point) => point.total >= 0)
    .map((point, index) => {
      const input = Math.round(point.total * 0.86);
      return {
        key: point.key,
        input,
        output: point.total - input,
        total: point.total,
        truncated: index === 8,
      };
    });
  return {
    date: dateKey(0),
    retentionDays: 90,
    days,
    total: days.reduce((sum, point) => sum + point.total, 0),
  };
}

/**
 * 演示：网关四维度（密钥 / 模型 / 账号 / 端点）逐日历史。
 *
 * 与 `demoGatewayTokenTrend` **同一批日键**，且四条维度的合计**等于**该日网关总量
 * （按权重摊、末位吃余数）—— 演示数据也必须守住「按模型合计 = 网关卡」这条自洽约束，
 * 否则评审时会被当成真 bug。
 */
function demoGatewayUsageDim(): GatewayUsageDim {
  const trend = demoGatewayTokenTrend();
  const models = ["claude-sonnet-4.5", "gpt-5-codex", "deepseek-v4.1-flash"];
  const weights = [0.56, 0.3, 0.14];
  const accounts = ["uid-a1", "uid-a2", "uid-a3"];
  const accountWeights = [0.52, 0.34, 0.14];
  const endpoints = ["/v1/chat/completions", "/v1/messages", "/v1/responses"];
  const endpointWeights = [0.92, 0.05, 0.03];
  /* 「按密钥」：演示站必须**有数**，否则该 Tab 全是「—」，看起来像没做。
     桶键带 `wb:` 归属（与后端 `token_daily::key_bucket_of` 同格式），
     `__none__` 留一小份「无 Key 调用」—— 那一行在真机上很常见。 */
  const keyBuckets = ["wb:demo-key-1", "wb:demo-key-2", "wb:demo-key-3", "__none__"];
  const keyWeights = [0.55, 0.32, 0.1, 0.03];

  const spread = (total: number, share: number[]) => {
    let acc = 0;
    return share.map((weight, index) => {
      const value = index === share.length - 1 ? Math.max(0, total - acc) : Math.round(total * weight);
      acc += value;
      return value;
    });
  };

  return {
    date: trend.date,
    retentionDays: trend.retentionDays,
    days: trend.days.map((day, index) => {
      const modelTokens = spread(day.total, weights);
      const keyTokens = spread(day.total, keyWeights);
      const accountTokens = spread(day.total, accountWeights);
      const endpointRequests = spread(Math.max(1, Math.round(day.total / 148000)), endpointWeights);
      const failed = index % 7 === 3 ? 5 : index % 5 === 0 ? 2 : 0;
      return {
        key: day.key,
        truncated: day.truncated,
        partial: index === 0,
        models: modelTokens.map((total, i) => ({
          model: models[i],
          input: Math.round(total * 0.9),
          output: total - Math.round(total * 0.9),
          total,
          requests: Math.max(1, Math.round(total / 148000)),
          failed: i === 0 ? failed : 0,
          credit: Math.round((total / 34000) * 1000) / 1000,
        })),
        accounts: accountTokens.map((total, i) => ({
          account: accounts[i],
          input: Math.round(total * 0.9),
          output: total - Math.round(total * 0.9),
          total,
          requests: Math.max(1, Math.round(total / 148000)),
          failed: i === 0 ? failed : 0,
          credit: Math.round((total / 34000) * 1000) / 1000,
        })),
        endpoints: endpointRequests.map((requests, i) => ({
          endpoint: endpoints[i],
          input: 0,
          output: 0,
          total: 0,
          requests,
          failed: i === 0 ? failed : 0,
          credit: 0,
        })),
        keys: keyTokens.map((total, i) => ({
          key: keyBuckets[i],
          input: Math.round(total * 0.9),
          output: total - Math.round(total * 0.9),
          total,
          requests: Math.max(1, Math.round(total / 148000)),
          failed: i === 0 ? failed : 0,
          credit: Math.round((total / 34000) * 1000) / 1000,
        })),
        /* 首日只是下界（v4 之前没有 key 维度）—— 演示「覆盖披露」那一行。 */
        keysPartial: index === 0,
      };
    }),
  };
}

/**
 * 演示：今日客户端 ↔ 网关对账。
 *
 * 刻意演示 **`independent`（两本账独立）**：issue #63 作者那台机器就是这个形态，
 * 也是「合计 = 相加」这一支。切到别的形态要看 `subset`，改 `matched` 即可。
 */
function demoUsageReconcile(): GatewayUsageRecon {
  const clientTokens = 2_876_406;
  const gatewayTokens = 18_500_000;
  return {
    clientRows: 117,
    gatewayRows: 126,
    matched: 9,
    matchedTokens: 350_000,
    matchedCredit: 10.3,
    clientTokens,
    clientCredit: 84.6,
    gatewayTokens,
    gatewayCredit: 544.1,
    verdict: "independent",
    proxySignal: null,
    bounds: { lower: Math.max(clientTokens, gatewayTokens), upper: clientTokens + gatewayTokens },
    estimate: clientTokens + gatewayTokens - 350_000,
    unmatchedSamples: [
      { ts: Date.now() - 1_200_000, model: "step-5-preview", tokens: 42_100, credit: 1.24 },
      { ts: Date.now() - 900_000, model: "hy3", tokens: 28_600, credit: 0.84 },
      { ts: Date.now() - 600_000, model: "deepseek-v4.1-flash", tokens: 15_400, credit: 0.45 },
    ],
    window: { date: new Date().toISOString().slice(0, 10), logRows: 126 },
  };
}

// ---------------------------------------------------------------------------
// 客户端限流台账（演示数据，只读）
// ---------------------------------------------------------------------------

/**
 * 演示：客户端限流台账（账号 × 模型 × 恢复时刻）。
 *
 * 形状与 core `client_limits::ClientLimitEntry` **逐字段一致**。演示数据为**虚构**，
 * 覆盖三种状态与两条易错分支：
 * - `state: "active"`（`resetAt` 在未来 ⇒ 置顶高亮）；
 * - `state: "unknown"`（`resetAt` 为 `null`，且之后没再试过 ⇒ 「状态未知」，
 *   **不得**画成「已恢复」，也不得画成「仍受限」）；
 * - `state: "recovered"`（**实测**：该错误之后又有成功产出，故 `evidenceAt` 非空、
 *   且 `resetAt` 仍在未来 —— 上游文案里的恢复时刻是配额窗口，不是解锁时刻）；
 *   该条同时演示 `occurrences > 1`（同一次限流扇出到多个会话文件）；
 * - `account` / `model` 为 `null` 的条目表示 join 失败（未知兜底）。
 */
function demoClientLimits(): ClientLimitsResponse {
  const now = Date.now();
  return {
    region: "cn",
    since: now - 7 * 24 * 60 * 60 * 1000,
    now,
    // `entries` 必须等于 `limits.length`（后端保证两者一致）；写错会让汇总条与列表自相矛盾。
    counts: { scanned: 7, entries: 3, active: 1, unknown: 1, skipped: 3 },
    limits: [
      {
        account: "demo-user-001",
        accountUid: "demo-user-001",
        model: "deepseek-v4.1-flash",
        code: 6004,
        codeText: null,
        status: 429,
        resetAt: now + 45 * 60 * 1000,
        occurredAt: now - 5 * 60 * 1000,
        evidenceAt: null,
        state: "active",
        occurrences: 2,
        message: "429 您的使用量已超出频率限制，将在 <时间> UTC+8 重置，您也可以切换其他模型继续使用。",
        sessionId: "demo-session-1",
        cwd: "/demo/projects/alpha",
        agent: "cli",
        region: "cn",
      },
      {
        account: null,
        accountUid: null,
        model: "hy4-preview-f",
        code: 6020,
        codeText: null,
        status: 429,
        resetAt: null,
        occurredAt: now - 40 * 60 * 1000,
        evidenceAt: null,
        state: "unknown",
        occurrences: 5,
        message: "429 queue.waiting.title",
        sessionId: "demo-session-2",
        cwd: null,
        agent: "Explore",
        region: "cn",
      },
      {
        account: "demo-user-002",
        accountUid: "demo-user-002",
        model: "deepseek-v4.1-flash",
        code: 6004,
        codeText: null,
        status: 429,
        resetAt: now + 9 * 60 * 60 * 1000,
        occurredAt: now - 90 * 60 * 1000,
        // 已恢复靠的是**实测**：该错误之后确实又产出了（配额窗口其实还没到点）。
        evidenceAt: now - 60 * 60 * 1000,
        state: "recovered",
        occurrences: 1,
        message: "429 usage exceeds frequency limit, your usage will reset at <时间> UTC+8.",
        sessionId: "demo-session-3",
        cwd: "/demo/projects/beta",
        agent: "cli",
        region: "cn",
      },
    ],
  };
}

/**
 * Skill 清单（演示数据，只读）—— 覆盖「启用 / 停用 / 状态不一致 / 无 frontmatter / 来源徽章」
 * 五种形态，便于截图一次看全列表的各个分支。
 *
 * 演示数据**虚构**：名称与描述都是通用示例，不照抄真实用户的 skill。
 */
function demoSkills(region: Region): SkillListResponse {
  return {
    region,
    skills: [
      {
        dirName: "design-router",
        name: "design-router",
        description: "根据需求自动路由到合适的设计工具。",
        version: "1.2.0",
        enabled: true,
        inconsistent: false,
        agentCreated: false,
        hasFrontmatter: true,
      },
      {
        dirName: "ardot-poster",
        name: "ardot-poster",
        description: "生成营销海报。",
        version: "0.9.3",
        enabled: false,
        inconsistent: false,
        agentCreated: false,
        hasFrontmatter: true,
      },
      {
        dirName: "my-notes",
        name: "my-notes",
        description: "把会话要点整理成笔记。",
        version: null,
        enabled: true,
        inconsistent: true,
        agentCreated: true,
        hasFrontmatter: true,
      },
      {
        dirName: "raw-scratch",
        name: "raw-scratch",
        description: "",
        version: null,
        enabled: true,
        inconsistent: false,
        agentCreated: false,
        hasFrontmatter: false,
      },
    ],
  };
}

// ---------------------------------------------------------------------------
// 上游客户端 settings.json 托管（演示数据，只读）
// ---------------------------------------------------------------------------
//
// ⚠️ 管的是**被代理的上游客户端**自己的 `settings.json`（不是本工具的 `~/.buddy-switch/**`）。
// 演示数据**虚构**，但刻意覆盖几个真实分支：**未安装**（仍在列表）、**运行中**（写被拒）、
// 四种排版风格（2 空格 / 无缩进 / Tab / CRLF），便于一次截图核对面板各状态。

/** 演示槽位：6 个齐列，其中两个**未安装**（覆盖「未安装仍在列表」）。 */
function demoClientSettingsSlots(): ClientSettingsSlotsResponse {
  const slots: ClientSettingsSlot[] = [
    {
      key: "workbuddy-cn",
      displayKey: "clientSettings.slot.workbuddyCn",
      product: "workbuddy",
      installed: true,
      path: "C:\\Users\\demo\\.workbuddy\\settings.json",
    },
    {
      key: "workbuddy-global",
      displayKey: "clientSettings.slot.workbuddyGlobal",
      product: "workbuddy",
      installed: false,
      path: "C:\\Users\\demo\\.workbuddy-ai\\settings.json",
    },
    {
      key: "trae-code-cn",
      displayKey: "clientSettings.slot.traeCodeCn",
      product: "trae",
      installed: true,
      path: "C:\\Users\\demo\\AppData\\Roaming\\Trae CN\\User\\settings.json",
    },
    {
      key: "trae-code-global",
      displayKey: "clientSettings.slot.traeCodeGlobal",
      product: "trae",
      installed: true,
      path: "C:\\Users\\demo\\AppData\\Roaming\\Trae\\User\\settings.json",
    },
  ];
  return { slots };
}

/** 演示键值（虚构）：形状与真机同为「顶层键 → 值」，嵌套对象供点号路径编辑演示。 */
function demoClientSettingsValues(slotKey: string): Record<string, unknown> {
  if (slotKey.startsWith("workbuddy")) {
    return {
      claw: {
        channels: { wechatmp: { enabled: false }, feishu: { enabled: true } },
        users: { default: "demo-user" },
        legacyOwnerUid: null,
      },
      http: { proxy: "", proxySupport: "on" },
      sandbox: { extraAllowWrite: [] },
      enabledPlugins: ["claw"],
      autoLaunchDesired: true,
      enableModelOptimization: { enabled: true },
    };
  }
  return {
    trae: { network: { proxy: { effectiveMode: "system" } } },
    window: { autoDetectColorScheme: true },
    update: { mode: "default", enableWindowsBackgroundUpdates: true },
    AI: {
      rules: { importClaudeMd: true },
      agent: { v2: { browserChromeEnabled: false } },
      toolcall: { v2: { ide: { command: { mode: "auto" } }, solo: { command: { mode: "auto" } } } },
    },
  };
}

/** 各槽位演示排版指纹（覆盖真机实测的四种风格）。 */
function demoClientSettingsFingerprint(slotKey: string): ClientSettingsFingerprint {
  if (slotKey === "trae-code-cn") return { indent: "", newline: "\n" };
  if (slotKey === "trae-work-cn") return { indent: "\t", newline: "\r\n" };
  if (slotKey.startsWith("workbuddy")) return { indent: "  ", newline: "\r\n" };
  return { indent: "  ", newline: "\n" };
}

/** 演示读取：未安装 ⇒ `installed:false` 且三件套为空（**不伪造**成功）。 */
function demoClientSettingsRead(slotKey: string): ClientSettingsReadResponse {
  const all = demoClientSettingsSlots().slots;
  const slot = all.find((entry) => entry.key === slotKey) ?? all[0];
  if (!slot.installed) {
    return {
      slot: slot.key,
      displayKey: slot.displayKey,
      product: slot.product,
      path: slot.path ?? "",
      installed: false,
      running: false,
      text: null,
      values: null,
      keyOrder: [],
      fingerprint: null,
    };
  }
  const values = demoClientSettingsValues(slot.key);
  const fingerprint = demoClientSettingsFingerprint(slot.key);
  return {
    slot: slot.key,
    displayKey: slot.displayKey,
    product: slot.product,
    path: slot.path ?? "",
    installed: true,
    // 运行态：Trae CN 演示为「运行中」（写入会被后端拒绝），其余为「未运行」。
    running: slot.key === "trae-code-cn",
    text: JSON.stringify(values, null, fingerprint.indent ?? undefined),
    values,
    keyOrder: Object.keys(values),
    fingerprint,
  };
}

/** 演示备份点（虚构，两条，覆盖「有历史备份」列表形态）。 */
function demoClientSettingsBackups(slotKey: string): ClientSettingsBackupsResponse {
  const all = demoClientSettingsSlots().slots;
  const slot = all.find((entry) => entry.key === slotKey) ?? all[0];
  const dir = `C:\\Users\\demo\\.buddy-switch\\backups\\client-settings\\${slot.key}`;
  return {
    slot: slot.key,
    displayKey: slot.displayKey,
    dir,
    backups: [
      { id: "2026-10-07T09:12:30Z", path: `${dir}\\2026-10-07T09:12:30Z\\settings.json`, size: 1675 },
      { id: "2026-10-05T21:40:02Z", path: `${dir}\\2026-10-05T21:40:02Z\\settings.json`, size: 1650 },
    ],
  };
}

/** 拍平嵌套对象为点号路径（与 core `client_settings::flatten` 同语义）。 */
function flattenDemo(value: unknown, prefix: string, out: Map<string, unknown>): void {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
      flattenDemo(child, prefix ? `${prefix}.${key}` : key, out);
    }
    return;
  }
  if (prefix) out.set(prefix, value);
}

/** 演示用的代理语义映射（与 core `client_settings::PROXY_FIELDS` 同口径）。 */
const DEMO_PROXY_FIELDS: Record<string, string[]> = {
  workbuddy: ["http.proxy", "http.proxySupport"],
  trae: ["trae.network.proxy.effectiveMode"],
};

/**
 * 演示比对：按 core `diff_flat` + `merge_proxy_fields` 的口径现算，
 * 避免写死一份「对不上」的假结果。
 */
function demoClientSettingsDiff(slotA: string, slotB: string): ClientSettingsDiffResponse {
  const all = demoClientSettingsSlots().slots;
  const a = all.find((entry) => entry.key === slotA) ?? all[0];
  const b = all.find((entry) => entry.key === slotB) ?? all[0];
  const aFlat = new Map<string, unknown>();
  const bFlat = new Map<string, unknown>();
  if (a.installed) flattenDemo(demoClientSettingsValues(a.key), "", aFlat);
  if (b.installed) flattenDemo(demoClientSettingsValues(b.key), "", bFlat);

  // 跨产品：代理字段按映射表归并成一组（同产品两侧路径本就相同，不做归并）。
  const proxyGroups: ClientSettingsProxyGroup[] = [];
  if (a.product !== b.product) {
    const aPaths = DEMO_PROXY_FIELDS[a.product] ?? [];
    const bPaths = DEMO_PROXY_FIELDS[b.product] ?? [];
    if (aPaths.length > 0 && bPaths.length > 0) {
      const fields = (flat: Map<string, unknown>, paths: string[]) =>
        paths.map((path) => ({
          path,
          present: flat.has(path),
          value: flat.has(path) ? flat.get(path) : null,
        }));
      const aFields = fields(aFlat, aPaths);
      const bFields = fields(bFlat, bPaths);
      const valuesOf = (list: { present: boolean; value: unknown }[]) =>
        JSON.stringify(list.filter((field) => field.present).map((field) => field.value));
      proxyGroups.push({
        semantic: "proxy",
        a: aFields,
        b: bFields,
        same: valuesOf(aFields) === valuesOf(bFields),
      });
      // 归并进来的路径不再出现在三类里。
      for (const path of aPaths) aFlat.delete(path);
      for (const path of bPaths) bFlat.delete(path);
    }
  }

  const onlyA: ClientSettingsDiffResponse["onlyA"] = [];
  const onlyB: ClientSettingsDiffResponse["onlyB"] = [];
  const different: ClientSettingsDiffResponse["different"] = [];
  let same = 0;
  for (const [path, value] of aFlat) {
    if (!bFlat.has(path)) onlyA.push({ path, value });
    else if (JSON.stringify(bFlat.get(path)) === JSON.stringify(value)) same += 1;
    else different.push({ path, a: value, b: bFlat.get(path) });
  }
  for (const [path, value] of bFlat) {
    if (!aFlat.has(path)) onlyB.push({ path, value });
  }

  return {
    onlyA,
    onlyB,
    different,
    counts: { onlyA: onlyA.length, onlyB: onlyB.length, different: different.length, same },
    proxyGroups,
    slotA: { key: a.key, displayKey: a.displayKey, path: a.path ?? "", installed: a.installed },
    slotB: { key: b.key, displayKey: b.displayKey, path: b.path ?? "", installed: b.installed },
  };
}

// ---------------------------------------------------------------------------
// Trae 分区（演示数据，只读）
// ---------------------------------------------------------------------------
//
// 形状刻意与 `webui` 的 HTTP 响应**逐字段一致**（含 snake_case 的网关字段）：
// 前端页面在两种模式下走同一套归一化逻辑，形状不一致会让「演示站好看、真实站崩」
// 这类差异在打包后才暴露。数值与 `.qa-tmp/mock_api.py` 对齐，两种模式的截图可比。

/**
 * 演示账号的**用户名 / 分组名 / 套餐名 / 冷却原因**一律走 `t()`。
 *
 * ⚠️ `userId`（`7481920`）是**身份**，中英共用、永不翻译 —— 它是几个假数据块
 * （账号列表、签到结果、登录态快照、网关账号池）**互相指向**的键，翻译了就会
 * 让「名字与账号列表对不上」。同理 `cooldownType`（`SoftRate`/`SessionDead`）
 * 是协议值，只有 `cooldownReason` 是给人看的文案。
 */
function traeDemoAccounts(): TraeAccount[] {
  /**
   * ★ 2026-10-12：与原型 `SCOPES["trae-cn"].accounts`（`buddy-switch-prototype.html:1292-1317`）
   * **逐账号对应** —— 名字 / 积分 / 临期 / 签到 / 最后使用全部照抄，
   * 演示站因此可以与原型**逐卡对照**（用户要求的 1:1 核验）。
   *
   * 每个账号拆**两个积分包**（月度包 + 临期包）是为了让「剩余 880」与「临期 140」同时成立：
   * 单包做不到「总量 1,500 里只有一部分快到期」。
   */
  return [
    // jackey：880/1,500，其中 140 五天后到期；work + code 两个程序位都绑
    {
      userId: "7481920", name: t("shared.demo.trae.name.jackey"), groupId: "g1", jwt: "", jwtExpHours: 320.5,
      jwtExpTimestamp: atLocalTime(0, 9, 12), jwtStatus: "ok", checkedToday: true,
      credits: 880, remainingCredits: 880, creditsExpireAt: Math.floor(futureAt(5, 0, 0) / 1000),
      creditPackages: [
        { packageCode: "pkg_work_month", packageName: t("shared.demo.trae.pack.month"), total: 1360, remaining: 740, used: 620, expireAt: Math.floor(futureAt(30, 0, 0) / 1000), expired: false, expiringSoon: false },
        { packageCode: "pkg_work_bonus", packageName: t("shared.demo.trae.pack.checkinBonus"), total: 140, remaining: 140, used: 0, expireAt: Math.floor(futureAt(5, 0, 0) / 1000), expired: false, expiringSoon: true },
      ],
      deviceIdMasked: "a1b2…9f", cooldownType: null, cooldownUntil: null, cooldownReason: null, cooldownSince: null,
      hasRefreshToken: true, jwtAutoRefresh: true,
      addedAt: "2026-09-10T02:11:00Z", updatedAt: new Date(Date.now() - 30 * 1000).toISOString(),
      remark: t("shared.demo.trae.remark.jackey"),
    },
    // code-lab：1,320/1,500，33 天后到期；只绑 code
    {
      userId: "7481999", name: t("shared.demo.trae.name.codeLab"), groupId: "g1", jwt: "", jwtExpHours: 210.4,
      jwtExpTimestamp: atLocalTime(0, 15, 20), jwtStatus: "ok", checkedToday: true,
      credits: 1320, remainingCredits: 1320, creditsExpireAt: Math.floor(futureAt(33, 0, 0) / 1000),
      creditPackages: [
        { packageCode: "pkg_cn_plan", packageName: t("shared.demo.trae.pack.cnPlan"), total: 1500, remaining: 1320, used: 180, expireAt: Math.floor(futureAt(33, 0, 0) / 1000), expired: false, expiringSoon: false },
      ],
      deviceIdMasked: "c3d4…7e", cooldownType: null, cooldownUntil: null, cooldownReason: null, cooldownSince: null,
      hasRefreshToken: true, jwtAutoRefresh: true,
      addedAt: "2026-09-12T08:00:00Z", updatedAt: new Date(Date.now() - 20 * 60_000).toISOString(),
      remark: null,
    },
    // solo-test：90/500，全部一天后到期；**今日未签到**
    {
      userId: "7482044", name: t("shared.demo.trae.name.soloTest"), groupId: "g2", jwt: "", jwtExpHours: 96.8,
      jwtExpTimestamp: atLocalTime(1, 5, 30), jwtStatus: "ok", checkedToday: false,
      credits: 90, remainingCredits: 90, creditsExpireAt: Math.floor(futureAt(1, 0, 0) / 1000),
      creditPackages: [
        { packageCode: "pkg_trial", packageName: t("shared.demo.trae.pack.trial"), total: 500, remaining: 90, used: 410, expireAt: Math.floor(futureAt(1, 0, 0) / 1000), expired: false, expiringSoon: true },
      ],
      deviceIdMasked: "e5f6…1a", cooldownType: null, cooldownUntil: null, cooldownReason: null, cooldownSince: null,
      hasRefreshToken: true, jwtAutoRefresh: true,
      addedAt: "2026-09-14T09:30:00Z", updatedAt: new Date(Date.now() - 2 * 86_400_000).toISOString(),
      remark: null,
    },
    // idle-01：480/500，15 天后到期；今日用量 0
    {
      userId: "7482101", name: t("shared.demo.trae.name.idle"), groupId: "g3", jwt: "", jwtExpHours: 640.2,
      jwtExpTimestamp: atLocalTime(3, 11, 0), jwtStatus: "ok", checkedToday: true,
      credits: 480, remainingCredits: 480, creditsExpireAt: Math.floor(futureAt(15, 0, 0) / 1000),
      creditPackages: [
        { packageCode: "pkg_cn_plan", packageName: t("shared.demo.trae.pack.cnPlan"), total: 500, remaining: 480, used: 20, expireAt: Math.floor(futureAt(15, 0, 0) / 1000), expired: false, expiringSoon: false },
      ],
      deviceIdMasked: "1122…3b", cooldownType: null, cooldownUntil: null, cooldownReason: null, cooldownSince: null,
      hasRefreshToken: true, jwtAutoRefresh: true,
      addedAt: "2026-09-18T03:20:00Z", updatedAt: new Date(Date.now() - 11 * 86_400_000).toISOString(),
      remark: null,
    },
    // snap-acc：700/1,500，25 天后到期
    {
      userId: "7482102", name: t("shared.demo.trae.name.snap"), groupId: "g3", jwt: "", jwtExpHours: 512.0,
      jwtExpTimestamp: atLocalTime(2, 14, 5), jwtStatus: "ok", checkedToday: true,
      credits: 700, remainingCredits: 700, creditsExpireAt: Math.floor(futureAt(25, 0, 0) / 1000),
      creditPackages: [
        { packageCode: "pkg_work_month", packageName: t("shared.demo.trae.pack.month"), total: 1500, remaining: 700, used: 800, expireAt: Math.floor(futureAt(25, 0, 0) / 1000), expired: false, expiringSoon: false },
      ],
      deviceIdMasked: "7788…4c", cooldownType: null, cooldownUntil: null, cooldownReason: null, cooldownSince: null,
      hasRefreshToken: true, jwtAutoRefresh: true,
      addedAt: "2026-09-20T06:45:00Z", updatedAt: new Date(Date.now() - 5 * 86_400_000).toISOString(),
      remark: null,
    },
  ];
}

/** 签到结果（演示）。`message` 是给人看的，`errorType` / `action` 是协议值。 */
function traeDemoCheckinResults() {
  return [
    { name: t("shared.demo.trae.name.jackey"), userId: "7481920", ok: true, code: 200, message: t("shared.demo.trae.checkin.ok"), action: "claim", credits: 120, delta: 20, errorType: null, cooldownUntil: null },
    { name: t("shared.demo.trae.name.codeLab"), userId: "7481999", ok: true, code: 200, message: t("shared.demo.trae.checkin.already"), action: "skip_already", credits: 64.5, delta: 0, errorType: null, cooldownUntil: null },
    { name: t("shared.demo.trae.name.soloTest"), userId: "7482044", ok: false, code: 401, message: t("shared.demo.trae.checkin.sessionDead"), action: "claim", credits: null, delta: 0, errorType: "SessionDead", cooldownUntil: 9_999_999_999 },
  ];
}

/**
 * 演示数据：「清除缓存」的扫描结果。
 *
 * 数值取**真实量级**（会话复制备份是大头，与线上 18GB 的成因同构）——
 * 演示站要能显示「哪一类最占地方」，全 0 会让勾选与预估失去演示意义。
 */
function demoCacheScan(): CacheScanReport {
  return {
    storePath: "C:\\Users\\demo\\.buddy-switch",
    totalBytes: 1823221145,
    categories: [
      { id: "session_backups", bytes: 1760937431, entries: 3042 },
      { id: "snapshots", bytes: 3984588, entries: 47 },
      { id: "caches", bytes: 87031807, entries: 12 },
      { id: "logs", bytes: 262019, entries: 9 },
    ],
  };
}

function demoTraeEnv(): TraeEnvStatus {
  return {
    installed: true, running: true, version: "1.107.1",
    // 演示数据刻意用「装在非系统盘」的真实形态：这正是自动探测要覆盖的场景，
    // 也让「同机多产品线」的能力在截图里可见。平台是 win32，路径就用 Windows 形态。
    path: "D:\\Programs\\TRAE SOLO CN\\TRAE SOLO CN.exe",
    dataDir: "C:\\Users\\demo\\AppData\\Roaming\\TRAE SOLO CN",
    dataDirExists: true, platform: "win32", configuredPath: null,
    // `configuredPath` 为空 ⇒ 本字段恒 `false`（后端同款语义）：演示站上
    // 设置页的「路径已失效」提示因此不出现，符合「没填就没有失效」。
    configuredPathExists: false,
    // ★ 按区域分家的完整视图（issue #20）：键必须两个区域都在，否则设置页在
    // 缺键的区域上会读到 `undefined` —— 演示站能跑、真实站要等归一化才生效，
    // 这类差异只会在真机暴露。
    configuredPaths: { cn: null, global: null },
    configuredPathsExist: { cn: false, global: false },
    // 变体字段必须与真实 HTTP 响应同形状：fixture 少一个键，演示站就看不到
    // 产品线标签，而这个差异只会在真机暴露。
    variant: "trae_merged", variantLabel: t("trae.program.traeCn"),
  };
}

/**
 * 演示数据：**两条** Trae 产品线并排。
 *
 * 刻意让两条线的状态不同（一条运行中、一条未运行），这样截图/演示站上
 * 「并排两个图标各自独立」这件事才看得出来 —— 两条都同状态的话，
 * 分不清是「两条独立探测」还是「同一条画了两遍」。
 */
/**
 * 演示用的**区域 + 程序位**状态。
 *
 * 形状自 2026-09-21 起改为「按区域列条目、条目内含程序位」：区域才是账号体系的分界，
 * 程序位只决定客户端。演示数据刻意让两个区域状态不同（国内装齐两条程序、
 * 国际只装了 TraeWork），这样截图/演示站上能看出**区域与程序是两层**，
 * 而不是「同一条线画了两遍」。
 */
function demoTraeVariants(): TraeVariantsStatus {
  /** 程序位展示名走词表（`shared.demo.trae.program.*`）；`variant` 是回传后端的标识，保持英文。 */
  const program = (
    labelKey: TranslationKey,
    nameAliasKey: TranslationKey,
    variant: "trae_merged" | "global_trae_merged",
    installed: boolean,
    running: boolean,
    path: string,
    dataDir: string,
  ) => ({
    // ★ issue #28 收敛：程序位标识恒为 `trae`（合并版），区域差异由 `variant` 承载。
    program: "trae" as const,
    label: t(labelKey),
    nameAlias: t(nameAliasKey),
    variant,
    installed,
    running,
    version: installed ? "1.107.1" : null,
    path: installed ? path : null,
    dataDir: installed ? dataDir : null,
    dataDirExists: installed,
    // 登录态文件是否存在（issue #136）：演示数据里「装了」就等于「写得出 storage.json」。
    storageJsonExists: installed,
    // 演示数据里两个目录取同一个值（真实机器上它们可能分叉，见
    // `TraeProgramStatus.writeDataDir` 的说明）—— 截图场景不需要复现那个分叉。
    writeDataDir: installed ? dataDir : null,
    writeDataDirExists: installed,
  });

  return {
    platform: "win32",
    variants: [
      {
        variant: "cn",
        variantLabel: t("trae.region.cn"),
        consoleBase: "https://www.trae.cn",
        installed: true,
        running: true,
        version: "1.107.1",
        path: "D:\\Programs\\Trae CN\\Trae CN.exe",
        dataDir: "C:\\Users\\demo\\AppData\\Roaming\\Trae CN",
        dataDirExists: true,
        // 登录态文件是否存在（issue #136）：演示里国内区已登录 ⇒ 文件存在。
        storageJsonExists: true,
        writeDataDir: "C:\\Users\\demo\\AppData\\Roaming\\Trae CN",
        writeDataDirExists: true,
        programs: [
          // ★ issue #28 收敛：每区域**只有一个**程序位（官方合并后的单一客户端）。
          //   演示数据让国内区「已安装 + 运行中」、国际区「未安装」，
          //   这样截图里能看出**两个区域各自的客户端状态**是独立探测的。
          program(
            "shared.demo.trae.program.trae",
            "shared.demo.trae.program.trae",
            "trae_merged",
            true,
            true,
            "D:\\Programs\\Trae CN\\Trae CN.exe",
            "C:\\Users\\demo\\AppData\\Roaming\\Trae CN",
          ),
        ],
      },
      {
        variant: "global",
        variantLabel: t("trae.region.global"),
        consoleBase: "https://www.trae.ai",
        installed: false,
        running: false,
        version: null,
        path: null,
        dataDir: null,
        dataDirExists: false,
        storageJsonExists: false,
        writeDataDir: null,
        writeDataDirExists: false,
        programs: [
          // 国际区的同一个客户端：演示数据里**未安装**（与国内区形成对比），
          // 便于截图展示「某区域没装客户端」时按钮的禁用态与文案。
          program(
            "shared.demo.trae.program.traeGlobalLabel",
            "shared.demo.trae.program.traeGlobalLabel",
            "global_trae_merged",
            false,
            false,
            "",
            "",
          ),
        ],
      },
    ],
  };
}

/**
 * 「平台做不到的维度」（置灰卡）的**键名表**。
 *
 * 形状与 Rust `handlers::unsupported_note` 逐字一致：`supportedOn` 里的
 * `"WorkBuddy"` / `"—"` 是**产品标识与占位符**（不是文案），故保持原样；
 * 只有 `label` / `reason` 是给人看的，走词表。
 *
 * ⚠️ 用 `as const` 让 `label`/`reason` 保留字面量类型 —— `TranslationKey` 才能校验。
 */
const CAPABILITY_STUBS = {
  capabilities: [
    { capability: "auto_travel", label: "shared.demo.cap.travel", supportedOn: "WorkBuddy", reason: "shared.demo.cap.travelReason" },
    { capability: "codebuddy_cli", label: "shared.demo.cap.cli", supportedOn: "WorkBuddy", reason: "shared.demo.cap.cliReason" },
    { capability: "account_data_migration", label: "shared.demo.cap.migration", supportedOn: "WorkBuddy", reason: "shared.demo.cap.migrationReason" },
    { capability: "session_tree", label: "shared.demo.cap.sessionTree", supportedOn: "WorkBuddy", reason: "shared.demo.cap.sessionTreeReason" },
  ],
  credits: [
    { capability: "official_credit_by_model", label: "shared.demo.cap.officialByModel", supportedOn: "—", reason: "shared.demo.cap.officialByModelReason" },
  ],
  tokenStats: [
    { capability: "cache_metrics", label: "shared.demo.cap.cacheMetrics", supportedOn: "—", reason: "shared.demo.cap.cacheMetricsReason" },
    { capability: "project_dimension", label: "shared.demo.cap.projectDimension", supportedOn: "—", reason: "shared.demo.cap.projectDimensionReason" },
    { capability: "session_cost", label: "shared.demo.cap.sessionCost", supportedOn: "—", reason: "shared.demo.cap.sessionCostReason" },
  ],
} as const;

/** 把键名表化成实际文案。**每次读取都重算** —— 语言切换后才不会停在旧语言。 */
function localizedStubs(stubs: readonly { capability: string; label: TranslationKey; supportedOn: string; reason: TranslationKey }[]) {
  return stubs.map((stub) => ({ ...stub, label: t(stub.label), reason: t(stub.reason) }));
}

function demoTraeCapabilities(): TraeCapabilities {
  return {
    platform: "win32", processControl: true, clientDetection: true,
    userDataDir: "C:\\Users\\demo\\AppData\\Roaming\\TRAE SOLO CN",
    machineGuidReset: true, scheduledTask: true,
    // 产品级不支持项（WorkBuddy 有、Trae 无），形状与 `platform::Unsupported` 逐字一致。
    unsupported: localizedStubs(CAPABILITY_STUBS.capabilities),
  };
}

function demoTraeAccounts(): unknown {
  const list = traeDemoAccounts();
  return {
    accounts: list,
    /* 分组照原型（常用 / 测试 / 闲置）；count 与上面 5 个账号的 groupId 一致。 */
    groups: [
      { id: "g1", name: t("shared.demo.trae.group.common"), color: "#888", order: 0, count: 2 },
      { id: "g2", name: t("shared.demo.trae.group.test"), color: "#888", order: 1, count: 1 },
      { id: "g3", name: t("shared.demo.trae.group.idle"), color: "#888", order: 2, count: 2 },
    ],
    total: list.length, cooling: 0, ungrouped: 0,
  };
}

/**
 * Qoder 演示账号（**只读**）—— 形状与真机 `qoder::account::accounts_view_for` 同构
 * （字段名 / 掩码 / 凭据形态逐字段对齐）。⚠️ 令牌**只给掩码**：演示数据也**不出现**明文。
 *
 * 账号与原型 `SCOPES["qoder-cn"|"qoder-global"].accounts` 的前两条对应，
 * 演示站因此可以与原型逐卡对照。
 */
function demoQoderAccounts(scope: string): unknown {
  const global = scope.endsWith("global");
  const region = global ? "global" : "cn";
  const domain = global ? "qoder.com" : "qoder.com.cn";
  const account = (
    uid: string,
    name: string,
    remark: string,
    accessMasked: string,
    canRefresh: boolean,
  ): unknown => ({
    uid,
    name,
    nickname: null,
    domain,
    region,
    /* ★ 与后端常量逐字一致（`qoder::account::SOURCE_DESKTOP_APP = "desktop-app"`）——
       写 "desktop" 会让来源徽章查表落空、回落显示原始值（演示站与真机不同形）。 */
    source: "desktop-app",
    remark,
    expiresAt: futureAt(21, 0, 0),
    hasAccessToken: true,
    hasRefreshToken: canRefresh,
    hasPersonalToken: false,
    accessTokenMasked: accessMasked,
    credential: {
      access: "device_access",
      refresh: canRefresh ? "device_refresh" : null,
      pat: false,
      unrecognized: false,
    },
    credentialSummary: canRefresh
      ? t("shared.demo.qoder.cred.device")
      : t("shared.demo.qoder.cred.deviceOnly"),
    hasCredential: true,
    domainMismatch: false,
    /* ★ 2026-10-15 起 `account_view` 多一个 `group`（自定义分组，自由文本）——
       演示数据缺它会让「按分组分桶」在演示站里永远落空。 */
    group: null,
    addedAt: "2026-09-22T03:00:00Z",
    updatedAt: new Date(Date.now() - 5 * 60_000).toISOString(),
  });
  const accounts = global
    ? [account("qg1", t("shared.demo.qoder.name.intl"), t("shared.demo.qoder.remark.intl"), "dt-9a3f…2c", true)]
    : [
        account("q1", t("shared.demo.qoder.name.jackey"), t("shared.demo.qoder.remark.main"), "dt-a1b2…9f", true),
        account("q2", t("shared.demo.qoder.name.team"), t("shared.demo.qoder.remark.team"), "dt-c3d4…7e", false),
      ];
  return { region, domain, accounts, foreignDomainCount: 0 };
}

/**
 * Qoder 程序位状态（**只读**）。
 *
 * ⚠️ 演示场景与 `demoQoderLocalSources` **必须同源**：那边说「桌面版 ready、CLI missing」，
 * 这边就不能给 CLI 一个 `ready` —— 同一份事实显示成两种状态，用户会以为界面在随机跳。
 * `currentUid` 取演示账号里那个「主号」（cn = `q1` / global = `qg1`），这样卡片上的
 * 「当前登录」徽章与「切换到此账号」的可用性在演示站里也能看到真实形态。
 */
function demoQoderPrograms(scope: string): unknown {
  const global = scope.endsWith("global");
  return {
    region: global ? "global" : "cn",
    programs: [
      {
        id: "desktop-app",
        label: t("qoder.page.accounts.program.desktop"),
        state: "ready",
        detail: null,
        /* 演示：桌面客户端「在跑」—— 界面上「切换」按钮应呈置灰并说明原因
           （客户端运行时写回会被它退出时的内存态覆盖）。 */
        running: true,
        currentUid: global ? "qg1" : "q1",
      },
      {
        id: "cli",
        label: t("qoder.page.accounts.program.cli"),
        state: "missing",
        detail: null,
        running: false,
        currentUid: null,
      },
    ],
  };
}

/**
 * Qoder 逐日用量（**只读**）。
 *
 * ★★ 单位是 **credits，不是 token** —— 演示数据也必须是 credits：Qoder CN 客户端按
 * credits 计费，客户端三处存储里的 token 计数**恒为 0**（见 `qoder::usage` 模块头）。
 * 演示站若把 credits 画成 token，等于把「图在撒谎」搬进演示环境。
 */
function demoQoderUsage(scope: string): unknown {
  const global = scope.endsWith("global");
  const model = global ? "ultimate" : "dfmodel";
  const model2 = global ? "performance" : "gm51model";
  /* 固定偏移（不用随机）：演示截图要可复现，随机数会让同一份数据每次截图都不一样。 */
  const rows: Array<[number, number, number]> = [
    [6, 12.4, 9],
    [5, 0, 0],
    [4, 31.7, 22],
    [3, 8.2, 6],
    [2, 44.9, 31],
    [1, 17.3, 12],
    [0, 5.6, 4],
  ];
  const days = rows
    .filter(([, credits]) => credits > 0)
    .map(([offset, credits, requests]) => {
      const date = new Date();
      date.setDate(date.getDate() - offset);
      const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
      return {
        date: key,
        credits,
        requests,
        byModel: [
          { model, credits: Math.round(credits * 0.62 * 1000) / 1000 },
          { model: model2, credits: Math.round(credits * 0.38 * 1000) / 1000 },
        ],
      };
    })
    .reverse();
  const totalCredits = days.reduce((sum, day) => sum + day.credits, 0);
  const totalRequests = days.reduce((sum, day) => sum + day.requests, 0);
  /* 会话排行：与 `days` 同源（拆成 3 个会话，credits 加起来等于总量）——
     ⚠️ 演示数据也要守「分段和 = 总量」这条纪律，否则截图会自打嘴巴。
     `title` 是真实会出现在 Qoder jsonl 里那种「用户第一句话」。 */
  const sessions = days.length
    ? (() => {
        const titles = ["多账号轮询与冷却", "网关 429 定位", "签名链路排查"];
        const shares = [0.5, 0.3, 0.2];
        return titles.map((title, index) => {
          const credits = Math.round(totalCredits * shares[index] * 1000) / 1000;
          const requests = Math.max(1, Math.round(totalRequests * shares[index]));
          return {
            id: `demo-sess-${index + 1}`,
            title,
            credits,
            requests,
            models: index === 0 ? [model, model2] : [model],
            firstDate: days[0].date,
            lastDate: days[days.length - 1].date,
          };
        });
      })()
    : [];
  return {
    region: global ? "global" : "cn",
    unit: "credits",
    days,
    sessions,
    totals: {
      credits: Math.round(totalCredits * 1000) / 1000,
      requests: totalRequests,
      sessions: days.length,
      models: 2,
    },
    byModel: [
      { model, credits: Math.round(totalCredits * 0.62 * 1000) / 1000 },
      { model: model2, credits: Math.round(totalCredits * 0.38 * 1000) / 1000 },
    ],
    source: {
      kind: "client-sessions",
      paths: [],
      sessions: days.length,
      exists: true,
      tokenCountsAvailable: false,
      note: t("qoder.page.usage.sourceNote"),
    },
  };
}

/**
 * Qoder 本机来源探测（**只读**）。
 *
 * ⚠️ 演示场景**如实**取「有客户端、只有桌面版登录过」：桌面来源 `ready`（可导入）、
 * CLI `missing`（没装 / 没登录过）—— 四态是四种不同的引导动作，压成布尔会给错方向。
 * 路径按 Windows 形态写（演示环境即 Windows），与 `qoder::paths` 的派生一致。
 */
/** 演示用：Qoder 额度视图（issue #111 之后卡片指标行与浮层都需要它）。 */
function demoQoderCredits(scope: string): unknown {
  const global = scope.endsWith("global");
  const rows = global
    ? [{ uid: "qg1", name: "Qoder Global", remaining: 300, used: 100, total: 400, plan: "Pro" }]
    : [
        { uid: "q1", name: "jackey", remaining: 890, used: 10, total: 900, plan: "Pro" },
        { uid: "q2", name: "team-qoder", remaining: 0, used: 100, total: 100, plan: null },
      ];
  const sum = rows.reduce(
    (acc, row) => ({ remaining: acc.remaining + row.remaining, total: acc.total + row.total }),
    { remaining: 0, total: 0 },
  );
  return {
    region: global ? "global" : "cn",
    summary: { okAccounts: rows.length, failedAccounts: 0, ...sum },
    accounts: rows.map((row) => ({
      uid: row.uid,
      name: row.name,
      ok: true,
      authRaw: null,
      refreshedAt: Date.now(),
      updatedAt: Date.now(),
      remaining: row.remaining,
      used: row.used,
      total: row.total,
      isExceeded: row.remaining <= 0,
      plan: row.plan,
    })),
  };
}

/**
 * 演示用：逐包明细。
 *
 * 刻意给四种形态：**正常 / 临期（5 天后）/ 已过期 / 零额度套餐条目** ——
 * 否则演示站只能看到一种颜色，等于没演示行内差异。
 */
function demoQoderWebPacks(uid: string): unknown {
  const DAY = 24 * 3600 * 1000;
  const now = Date.now();
  const pack = (
    id: string,
    group: string,
    source: string,
    limit: number,
    used: number,
    expiresAtMs: number,
    extra: Record<string, unknown> = {},
  ) => ({
    id,
    group,
    source,
    status: expiresAtMs > 0 && expiresAtMs < now ? "EXPIRED" : "ACTIVE",
    limit,
    used,
    remaining: Math.max(0, limit - used),
    unit: "credits",
    usagePercentage: limit > 0 ? Math.round((used / limit) * 100) : 0,
    isActive: !(expiresAtMs > 0 && expiresAtMs < now),
    expiresAtMs: expiresAtMs > 0 ? expiresAtMs : undefined,
    ...extra,
  });

  const packs =
    uid === "q2"
      ? [pack("demo-p3", "resource_package_quota", "RESOURCE_PACKAGE_SOURCE_BONUS", 100, 100, now - DAY)]
      : [
          pack("demo-p0", "plan_quota", "PLAN", 0, 0, 0),
          pack("demo-p1", "resource_package_quota", "RESOURCE_PACKAGE_SOURCE_BONUS", 100, 0, now + 30 * DAY),
          pack("demo-p2", "resource_package_quota", "RESOURCE_PACKAGE_SOURCE_BONUS", 500, 110, now + 5 * DAY),
        ];

  const counted = packs.filter((p) => p.limit > 0);
  return {
    available: true,
    packs,
    summary: {
      count: counted.length,
      zeroCount: packs.length - counted.length,
      limit: counted.reduce((n, p) => n + p.limit, 0),
      used: counted.reduce((n, p) => n + p.used, 0),
      remaining: counted.reduce((n, p) => n + p.remaining, 0),
    },
    verifiedAtMs: now,
  };
}

function demoQoderLocalSources(scope: string): unknown {
  const global = scope.endsWith("global");
  const region = global ? "global" : "cn";
  const domain = global ? "qoder.com" : "qoder.com.cn";
  return {
    region,
    domain,
    sources: [
      {
        kind: "desktop",
        label: "Qoder 桌面版",
        path: `%APPDATA%\\${global ? "com.qoder.app.stable" : "com.qodercn.app.stable"}\\auth.v1.dat`,
        state: "ready",
        detail: null,
      },
      {
        kind: "cli",
        label: "Qoder CLI",
        path: `~/${global ? ".qoder" : ".qoder-cn"}/.auth/user`,
        state: "missing",
        detail: null,
      },
    ],
  };
}

/**
 * Qoder 网关配置（**snake_case 原始形状**，与 HTTP 通道逐字同形 ⇒ 页面走同一套归一化）。
 *
 * ⚠️ `default_model` 演示**空串**（真机默认就是空，由清单第一条兜底）—— 演示站若填一个
 * 编出来的模型名，会掩盖「不清空就不写默认模型」的真实行为。
 * `preferred_uid` 指向下方账号池里那个可用的主账号（uid 必须对得上，否则选择器会显示
 * 「已指定的账号（当前不在池中）」，看起来像 bug）。
 */
function demoQoderGatewayConfig(): QoderGatewayConfigRaw {
  return {
    enabled: true,
    bind_addr: "127.0.0.1",
    port: 57895,
    allow_non_loopback: false,
    log_keep: 200,
    log_bodies: false,
    max_body_mb: 0,
    default_model: "",
    max_rotate: 3,
    preferred_uid: "q1",
    sticky_ttl_ms: 30 * 60 * 1000,
    // 护栏演示：开着「当日 token」这一条（截图里要能看到「已拦」与用量列，
    // 全关的话面板只会显示「未启用」，等于没演示）。
    reserve_credits: 200,
    daily_token_limit: 500_000,
    daily_credit_limit: 50,
    model_daily_token_limit: 0,
  };
}

/**
 * 限额护栏视图（演示）：一条已拦、一条正常、一条停放。
 *
 * 三个账号故意各占一种状态 —— 截图能同时看到「可用 / 触到日限 / 余额耗尽停放」，
 * 也顺带证明表里没有「恒 0」的假指标（token 是实打实记出来的数）。
 */
function demoQoderGatewayQuota(): QoderGatewayQuotaView {
  const date = new Date().toISOString().slice(0, 10);
  return {
    date,
    limits: {
      reserveCredits: 200,
      dailyTokenLimit: 500_000,
      dailyCreditLimit: 50,
      modelDailyTokenLimit: 0,
      enabled: true,
    },
    accounts: [
      {
        uid: "q1",
        name: "演示账号 · 主",
        tokens: 128_430,
        credits: 18.42,
        perModel: [
          { model: "qmodel_38max", tokens: 96_120 },
          { model: "glm-5.1", tokens: 32_310 },
        ],
        blocked: null,
        blockedReason: null,
        parkedUntil: null,
        parkedReason: null,
        creditsRemaining: 4_820,
        creditsAt: Date.now(),
      },
      {
        uid: "q2",
        name: "演示账号 · 备用",
        tokens: 500_000,
        credits: 62.5,
        perModel: [{ model: "qmodel_38max", tokens: 500_000 }],
        blocked: "daily_tokens",
        blockedReason: "已达当日 token 上限（500000 / 500000）",
        parkedUntil: null,
        parkedReason: null,
        creditsRemaining: 1_950,
        creditsAt: Date.now(),
      },
      {
        uid: "q3",
        name: "演示账号 · 待重置",
        tokens: 42_800,
        credits: 7.15,
        perModel: [{ model: "qmodel_38max", tokens: 42_800 }],
        blocked: "parked",
        blockedReason: "余额耗尽停放中（明天 10:00 后自动恢复）",
        parkedUntil: Date.now() + 12 * 3600 * 1000,
        parkedReason: "HTTP 402：余额不足",
        creditsRemaining: 0,
        creditsAt: Date.now(),
      },
    ],
  };
}

/**
 * Qoder 网关状态（**snake_case**；一个监听、两个区域 —— 池按 `scope` 分家）。
 *
 * ★ 三处数字**必须自洽**（本仓红线：同一个量在各处显示成不同的数就是图在撒谎）：
 * - `pool` 的三格 = `accounts` 里逐条状态数出来的（available + cooling + disabled = total）；
 * - `pool.modelCooling` = 带模型级冷却的条目数（与账号级 `cooling` 正交）；
 * - `api_key_prefix` = 下方 Key 列表里该区域那把**未吊销**的 Key 的前缀。
 */
function demoQoderGatewayStatus(scope: string): unknown {
  const global = scope.endsWith("global");
  const coolingUntil = Math.floor(Date.now() / 1000) + 5400;
  const accounts = global
    ? [
        {
          uid: "qg1",
          name: t("shared.demo.qoder.name.intl"),
          status: "available",
          credits: null,
          cooling: false,
          cooldownUntil: null,
          cooldownReason: null,
          disabled: false,
        },
      ]
    : [
        {
          uid: "q1",
          name: t("shared.demo.qoder.name.jackey"),
          status: "available",
          credits: null,
          cooling: false,
          cooldownUntil: null,
          cooldownReason: null,
          disabled: false,
        },
        {
          uid: "q2",
          name: t("shared.demo.qoder.name.team"),
          status: "disabled",
          credits: null,
          cooling: false,
          cooldownUntil: null,
          // 需人工干预（刷新令牌）—— 与「临时冷却」是两个状态，卡片据此给「恢复」出口。
          cooldownReason: t("shared.demo.qoder.gateway.reasonSessionDead"),
          disabled: true,
        },
        {
          uid: "q3",
          name: t("shared.demo.qoder.name.trial"),
          status: "cooling",
          credits: null,
          cooling: true,
          cooldownUntil: coolingUntil,
          cooldownReason: t("shared.demo.qoder.gateway.reasonCooling"),
          disabled: false,
        },
      ];
  const available = accounts.filter((item) => item.status === "available").length;
  const cooling = accounts.filter((item) => item.status === "cooling").length;
  const disabled = accounts.filter((item) => item.status === "disabled").length;
  return {
    enabled: true,
    running: true,
    addr: "127.0.0.1:57895",
    base_url: "http://127.0.0.1:57895",
    bind_addr: "127.0.0.1",
    port: 57895,
    allow_non_loopback: false,
    version: "2026.9.17",
    total_requests: global ? 43 : 268,
    last_error: null,
    api_key_prefix: global ? "sk-qoder-3f61" : "sk-qoder-5b2e",
    pool: {
      total: accounts.length,
      available,
      cooling,
      disabled,
      expired: 0,
      zero_credits: 0,
      total_credits: 0,
      // 演示：国内版有一个账号「对某个模型冷着、对其它模型仍可用」（Qoder 的 429 是按模型报的）。
      model_cooling: global ? 0 : 1,
    },
    accounts,
    diagnose: global
      ? [`${t("shared.demo.qoder.name.intl")}(qg1:可用)`]
      : [
          `${t("shared.demo.qoder.name.jackey")}(q1:可用（模型冷却：MiniMax-M3）)`,
          `${t("shared.demo.qoder.name.team")}(q2:${t("shared.demo.qoder.gateway.reasonSessionDead")})`,
          `${t("shared.demo.qoder.name.trial")}(q3:${t("shared.demo.qoder.gateway.reasonCooling")})`,
        ],
    upstream: global
      ? "https://api1.qoder.com / https://api2.qoder.com / https://api3.qoder.com"
      : "https://api1.qoder.com.cn / https://api2.qoder.com.cn",
  };
}

/**
 * Qoder 对外模型清单（**未过滤** + 逐条 `disabled`；与真机同形）。
 *
 * ★ 与真机同一条纪律：管理面给**全量**清单（禁用的也在），才能让「已禁用」的行可见；
 * 「网关真正暴露的条数」= 全量 − 已禁用（前端按同一份偏好现算，与出口 ① 同源）。
 *
 * 两个区域**刻意不同**（国内 4 条、国际 2 条，且有 `Ultimate` 这样的区域独占模型），
 * 并**演示来源三级**：国内 = `live`；国际 = `cached` + 原因（「本次探测超时，用上次成功的结果」）
 * —— 「非实时必须带原因」是本仓的来源契约，演示站也必须遵守。
 */
function demoQoderGatewayModels(scope: string): unknown {
  const global = scope.endsWith("global");
  /* `extra` 承载官方的计费与能力事实（价格倍率 / 免费 / 错峰徽标 / 思考档位），
     与真机管理面同形；演示里不填 = 该字段官方没给（渲染「—」）。 */
  const model = (
    name: string,
    displayName: string,
    reasoning: boolean,
    disabled = false,
    extra: Record<string, unknown> = {},
  ) => ({
    id: name,
    object: "model",
    owned_by: "qoder",
    display_name: displayName,
    is_reasoning: reasoning,
    max_input_tokens: 180_000,
    disabled,
    ...extra,
  });
  return global
    ? {
        object: "list",
        region: "global",
        source: "cached",
        fetchedAt: Date.now() - 15 * 60 * 60 * 1000,
        note: t("shared.demo.qoder.gateway.catalogCached"),
        data: [
          model("Ultimate", "Ultimate", true, false, { price_factor: 1, is_free: false }),
          model("Qwen3.8-Max", "Qwen3.8-Max", true, false, {
            price_factor: 0.5,
            thinking_efforts: ["low", "medium", "xhigh"],
          }),
        ],
      }
    : {
        object: "list",
        region: "cn",
        source: "live",
        fetchedAt: Date.now() - 8 * 60 * 1000,
        note: null,
        data: [
          model("Qwen3.8-Max", "Qwen3.8-Max", true, false, {
            price_factor: 0.5,
            is_free: true,
            thinking_efforts: ["low", "medium", "xhigh"],
          }),
          model("GLM-5.1", "GLM-5.1", true, false, {
            price_factor: 0.5,
            promotion: {
              badge: { zh: "错峰4折", en: "Off-peak 60% off" },
              window_start: "22:00",
              window_end: "08:00",
            },
            thinking_efforts: ["low", "medium"],
          }),
          // 已禁用：**仍留在清单里**（标灰 + 「启用」出口），与真机同形。
          model("Kimi-K3", "Kimi-K3", false, true, { price_factor: 0.1 }),
          model("MiniMax-M3", "MiniMax-M3", false, false, { is_free: true }),
        ],
      };
}

/**
 * Qoder 多 Key（脱敏；**明文 / 哈希一律不出现**）。
 *
 * 每个区域各一组，且**都带一条已吊销的**（与 WB / Trae 演示数据同形）：已吊销行必须
 * 同时给出「恢复 + 删除」两个出口，这条产品纪律要在演示里就能看见。
 */
function demoQoderApiKeys(): { keys: unknown[] } {
  return {
    keys: [
      {
        id: "demo-qoder-key-1",
        name: t("shared.demo.qoder.key.defaultCn"),
        region: "cn",
        prefix: "sk-qoder-5b2e",
        suffix: "c14f",
        createdAt: atLocalTime(23, 10, 12),
        revokedAt: null,
        revoked: false,
        lastUsedAt: atLocalTime(0, 17, 5),
      },
      {
        id: "demo-qoder-key-2",
        name: t("shared.demo.qoder.key.cliCn"),
        region: "cn",
        prefix: "sk-qoder-9a07",
        suffix: "4e83",
        createdAt: atLocalTime(13, 15, 40),
        revokedAt: atLocalTime(1, 9, 20),
        revoked: true,
        lastUsedAt: atLocalTime(0, 15, 2),
      },
      {
        id: "demo-qoder-key-3",
        name: t("shared.demo.qoder.key.global"),
        region: "global",
        prefix: "sk-qoder-3f61",
        suffix: "b7d2",
        createdAt: atLocalTime(15, 8, 0),
        revokedAt: null,
        revoked: false,
        lastUsedAt: atLocalTime(1, 22, 10),
      },
    ],
  };
}

/** Qoder 请求日志（元数据；条目带 `region` —— 一本日志记两个区域的请求）。 */
function demoQoderGatewayLogs(): unknown {
  const now = Math.floor(Date.now() / 1000);
  const logs = [
    { ts: (now - 70) * 1000, endpoint: "/v1/chat/completions", method: "POST", region: "cn", account: "q1", model: "Qwen3.8-Max", status: 200, latencyMs: 764, promptTokens: 1420, completionTokens: 386, stream: true, error: null },
    { ts: (now - 620) * 1000, endpoint: "/v1/messages", method: "POST", region: "cn", account: "q1", model: "GLM-5.1", status: 200, latencyMs: 588, promptTokens: 986, completionTokens: 254, stream: false, error: null },
    { ts: (now - 1800) * 1000, endpoint: "/v1/chat/completions", method: "POST", region: "cn", account: "q3", model: "MiniMax-M3", status: 429, latencyMs: 96, promptTokens: 0, completionTokens: 0, stream: true, error: t("shared.demo.qoder.gateway.modelRateLimited") },
    { ts: (now - 4200) * 1000, endpoint: "/v1/chat/completions", method: "POST", region: "global", account: "qg1", model: "Ultimate", status: 200, latencyMs: 918, promptTokens: 1104, completionTokens: 312, stream: true, error: null },
  ];
  return { logs };
}

function demoTraeCheckinStatus(): TraeCheckinStatus {
  return {
    summary: {
      time: `${localDate(0)}T01:00:00+08:00`,
      results: traeDemoCheckinResults(),
      totalOk: 1, already: 1, failed: 1,
      warnings: [t("shared.demo.trae.warn.altBSessionDead")],
    },
    summaryIsToday: true,
    cooldowns: [
      { userId: "7481999", type: "SoftRate", until: Math.floor(Date.now() / 1000) + 5400, reason: t("shared.demo.trae.cooldown.reasonRateLimited"), permanent: false },
      { userId: "7482044", type: "SessionDead", until: 9_999_999_999, reason: t("shared.demo.trae.cooldown.reasonSessionDead"), permanent: true },
    ],
    cooldownCount: 2,
    logFile: "/demo/buddy-switch/trae/logs/checkin.log",
  };
}

/**
 * 演示夹具里 `accountIds` 的解析：**与线上同一形状**（逗号分隔串）。
 *
 * 演示若自己改成数组，就会掩盖「两端参数形状分叉」这类缺陷 —— 夹具必须与契约同形。
 */
function demoAccountIds(args: unknown): string[] {
  const raw = (args as { accountIds?: unknown } | undefined)?.accountIds;
  return typeof raw === "string"
    ? raw.split(",").map((item) => item.trim()).filter((item) => item.length > 0)
    : [];
}

/**
 * 演示：Trae 逐账号「今日 Token」（`get_trae_account_token_today`；issue #47）。
 *
 * 刻意保留三条边界：① `clientSource: "unavailable"`（客户端一档不可得，**不是 0**）；
 * ② 某个账号**缺席**（`7482044`）⇒ 抽屉显示「—」；③ 有量的账号给干净数值。
 */
function demoTraeAccountTokenToday(): AccountTokenTodayResponse {
  return {
    date: localDate(0),
    accounts: [
      { uid: "7481920", clientIn: 0, clientOut: 0, gatewayIn: 28000, gatewayOut: 9000 },
      { uid: "7481999", clientIn: 0, clientOut: 0, gatewayIn: 6000, gatewayOut: 2000 },
      // `7482044` 故意缺席：今日无记录 ⇒「—」（缺席 ≠ 0 的活样本）。
    ],
    unknownClient: { in: 0, out: 0 },
    gatewayTruncated: false,
    clientSource: "unavailable",
  };
}

/** 演示：Trae 逐账号签到状态（`get_trae_checkin_status_map`；issue #47）。 */
function demoTraeCheckinStatusMap(ids: string[]): Record<string, CheckinStatusEntry> {
  const map: Record<string, CheckinStatusEntry> = {};
  for (const uid of ids) {
    // 缺席 ⇒「—」；读失败 ⇒「读不到」（与「未签到」区分开）。
    if (uid === "7482044") continue;
    if (uid === "7481999") {
      map[uid] = { ok: false, todayCheckedIn: false, error: "HTTP 503（演示：上游读不到）" };
      continue;
    }
    map[uid] = { ok: true, todayCheckedIn: true, dailyCredit: 150, source: "upstream" };
  }
  return map;
}

/** 演示：Qoder 逐账号签到状态（`qoder_checkin_status_map`；issue #47）。 */
function demoQoderCheckinStatusMap(ids: string[]): Record<string, CheckinStatusEntry> {
  const map: Record<string, CheckinStatusEntry> = {};
  for (const uid of ids) {
    if (uid === "demo-global-c") continue;
    if (uid === "demo-cn-d") {
      // 上游说「活动未开放」⇒ 有状态词但没签（前端据此说「本区域未开放」）。
      map[uid] = {
        ok: true,
        todayCheckedIn: false,
        status: "DISABLED",
        active: false,
        dailyCredit: 100,
        streakDays: 0,
      };
      continue;
    }
    map[uid] = {
      ok: true,
      todayCheckedIn: true,
      status: "CLAIMED",
      active: true,
      dailyCredit: 100,
      todayCredit: 100,
      totalCredits: 1200,
      streakDays: 3,
    };
  }
  return map;
}

function demoTraeCredits(): TraeCreditsOverview {  const list = traeDemoAccounts();
  const daily = [188, 172.5, 180, 165.25, 158, 148.5, 140, 192.5].map((total, index, all) => ({
    date: localDate(all.length - 1 - index),
    total,
    earned: [0, 0, 12, 0, 0, 0, 0, 20][index],
    consumed: [0, 15.5, 4.5, 14.75, 7.25, 9.5, 8.5, 0][index],
  }));
  return {
    remaining: { "7481920": 120, "7481999": 64.5, "7482044": 8 },
    expireTimes: {
      "7481920": Math.floor(futureAt(26) / 1000),
      "7481999": Math.floor(futureAt(5) / 1000),
      "7482044": Math.floor(futureAt(3) / 1000),
    },
    // 逐包明细与账号卡共用同一份假数据（`traeDemoAccounts[..].creditPackages`）。
    packages: {
      "7481920": list[0].creditPackages ?? [],
      "7481999": list[1].creditPackages ?? [],
      "7482044": list[2].creditPackages ?? [],
    },
    updatedAt: `${localDate(0)}T09:12:00+08:00`,
    balances: list.map((account) => ({
      userId: account.userId,
      credits: account.credits ?? 0,
      date: localDate(0),
    })),
    /* 积分流水（原型「积分流水」表的数据源）：**与 `daily` 的 earned / consumed 同源**
       —— 每一行的 `delta` 都能在上面找到对应日的收支，不另编一套数。
       刻意**不给 `delta: 0` 的行**：那既不是入账也不是消耗，摆在流水里只会让人困惑。 */
    records: [
      { date: localDate(0), userId: "7481920", credits: 120, delta: 20 },
      { date: localDate(2), userId: "7481920", credits: 158, delta: 12 },
      { date: localDate(6), userId: "7481999", credits: 64.5, delta: -8.5 },
    ],
    daily,
    todayEarned: 20,
    historyDays: 8,
    // 「官方积分消耗按模型」在 Trae 侧无数据源——形状与 `handlers::unsupported_note` 逐字一致。
    unsupported: localizedStubs(CAPABILITY_STUBS.credits),
  };
}

/**
 * 演示用的登录态快照总览（**按程序位分家**）。
 *
 * 快照是**客户端级**的（只能恢复到采集它的那个客户端），因此这里按**程序位**给数据：
 * 国内 TraeWork / 国内 TraeCode / 国际版 TraeWork 三者槽位与目录都不同 ——
 * 截图上必须能一眼看出「这是各自独立的快照」，而不是同一份数据被渲染了两次。
 *
 * ★ issue #28 收敛后**只有两个程序位**：CN 合并版（`trae_merged`）与国际合并版
 * （`global_trae_merged`）。入参 `variant` 仍兼容**历史标识**（`cn` / `trae_work` /
 * `trae_cn` / `trae_code` …）—— 老界面、老链接、老脚本都还在传它们 ——
 * 判据只看它**落在哪个区域**，与后端 `variant.region()` 的映射一致。
 */
function demoTraeProfiles(args?: Record<string, unknown>): TraeProfilesOverview {
  const raw = typeof args?.variant === "string" ? args.variant : "trae_merged";
  if (raw.startsWith("global")) {
    return {
      profiles: [
        { slot: "7481920", sizeBytes: 3_180_000, fileCount: 9, lastModified: `${localDate(2)} 09:31`, sizeText: "3.0 MB" },
      ],
      currentAccount: "9201733",
      // 刻意给 `null`：这个 uid 不在 `traeAccounts` 里，正好演示
      // 「客户端登录着一个库里没有的账号」这一**正常状态**——界面回落到 uid，
      // 而不是把它渲染成「未知账号」（那会抹掉唯一可核对的线索）。
      currentAccountName: null,
      dataDir: "C:\\Users\\demo\\AppData\\Roaming\\TRAE SOLO",
      clientRunning: false,
      coreEntryCount: 9,
    };
  }
  const isCn = raw === "trae_cn";
  if (isCn) {
    return {
      profiles: [
        { slot: "9201733", sizeBytes: 2_610_000, fileCount: 9, lastModified: `${localDate(1)} 14:05`, sizeText: "2.5 MB" },
      ],
      currentAccount: "7481920",
      // ★ 2026-10-12：与原型一致 —— jackey **两个程序位都绑**（卡片显示「使用中 2 个工具」）。
      //   原先这里是「客户端登录着一个库里没有的账号」的演示态，现挪到 `global` 分支
      //   （同一类正常状态，仍然被覆盖到）。
      currentAccountName: t("shared.demo.trae.name.jackey"),
      dataDir: "C:\\Users\\demo\\AppData\\Roaming\\Trae CN",
      clientRunning: false,
      coreEntryCount: 9,
    };
  }
  return {
    profiles: [
      { slot: "7481920", sizeBytes: 4_820_000, fileCount: 9, lastModified: `${localDate(0)} 09:12`, sizeText: "4.6 MB" },
      { slot: "7481999", sizeBytes: 3_140_000, fileCount: 9, lastModified: `${localDate(2)} 18:40`, sizeText: "3.0 MB" },
    ],
    currentAccount: "7481920",
    currentAccountName: t("shared.demo.trae.name.jackey"),
    dataDir: "C:\\Users\\demo\\AppData\\Roaming\\TRAE SOLO CN",
    clientRunning: true,
    coreEntryCount: 9,
  };
}

function demoTraeSettings(): TraeSettings {
  return {
    proxyPort: 8899, theme: "dark", launchMinimized: false, autoStartProxy: true, tray: true,
    language: "zh-CN", checkinSkipChecked: true, checkinSkipExpired: true, retry: 2,
    notify: "system", traePaths: {}, traePath: null, browserPath: null, logRetentionDays: 7,
    proxyDomains: "", apiPort: 57894, apiKey: "sk-trae-9f2c1a4b6d8e0f3a5b7c9d1e2f4a6b8c",
    apiDefaultModel: "deepseek-v4-flash",
  };
}

/**
 * Trae 网关配置。
 *
 * 返回 **snake_case 原始形状**（`TraeGatewayConfigRaw`）而不是前端的 camelCase 形状：
 * 演示模式必须与 HTTP 通道返回同一种形状，页面才会走同一套 `normalizeTraeGatewayConfig`。
 * 用 camelCase 的话演示站能跑、真实站要等归一化才生效，差异只在真机暴露。
 */
function demoTraeGatewayConfig(): TraeGatewayConfigRaw {
  return {
    enabled: true, bind_addr: "127.0.0.1", port: 57894, allow_non_loopback: false,
    log_keep: 200, log_bodies: false, max_body_mb: 8,
    default_model: "deepseek-v4-flash", max_rotate: 3,
    // 演示「已指定账号」的状态：指向下方账号池里那个可用的主账号
    // （`demoTraeGatewayStatus` 的 uid 必须与这里对得上，否则选择器会显示
    // 「已指定的账号（当前不在池中）」，看起来像 bug）。
    preferred_uid: "7481920",
    sticky_ttl_ms: 30 * 60 * 1000,
    // 抓取旁路默认关：演示站也不该呈现「已开启诊断抓取」的假状态。
    capture_enabled: false,
    max_mode: true,
  };
}

/** Trae 网关状态（**snake_case**，含账号池与诊断）。 */
function demoTraeGatewayStatus(): unknown {
  return {
    enabled: true, running: true, addr: "127.0.0.1:57894",
    base_url: "http://127.0.0.1:57894", bind_addr: "127.0.0.1", port: 57894,
    allow_non_loopback: false, version: "2026.9.17", total_requests: 137,
    last_error: null, api_key_prefix: "sk-trae-9f2c",
    pool: { total: 3, available: 1, cooling: 1, disabled: 1, expired: 0, zero_credits: 0, total_credits: 192.5 },
    accounts: [
      { uid: "7481920", name: t("shared.demo.trae.name.jackey"), status: "available", credits: 120, creditsExpireAt: Math.floor(futureAt(26) / 1000), cooling: false, cooldownUntil: null, cooldownReason: null, disabled: false, deviceIdMasked: "a1b2…9f0e" },
      { uid: "7481999", name: t("shared.demo.trae.name.codeLab"), status: "cooling", credits: 64.5, creditsExpireAt: Math.floor(futureAt(5) / 1000), cooling: true, cooldownUntil: Math.floor(Date.now() / 1000) + 5400, cooldownReason: t("shared.demo.trae.cooldown.reasonRateLimited"), disabled: false, deviceIdMasked: "c3d4…1122" },
      { uid: "7482044", name: t("shared.demo.trae.name.soloTest"), status: "disabled", credits: 8, creditsExpireAt: null, cooling: false, cooldownUntil: 9_999_999_999, cooldownReason: t("shared.demo.trae.cooldown.reasonSessionDead"), disabled: true, deviceIdMasked: "e5f6…3344" },
    ],
    diagnose: [
      t("shared.demo.trae.diagnose.main"),
      t("shared.demo.trae.diagnose.altA"),
      t("shared.demo.trae.diagnose.altB"),
    ],
    upstream: "https://trae-api-cn.mchost.guru",
  };
}

/**
 * TraeWork 的对外清单（演示用）= 本机实测的 `solo_work_lite` 分组。
 *
 * ⚠️ 名字必须与**客户端清单里逐字相同**（含大小写）：真实后端的 `/v1/models` 返回的
 * 就是客户端清单里的 `name`，卡片据此逐个标记「网关提不提供」。演示数据若写成小写
 * （例如 `doubao-seed-2.1-pro`），就会把**每一个**芯片都标成「网关不提供」——
 * 演示图会变成假象。
 */
const TRAE_GATEWAY_MODEL_NAMES = [
  "Doubao-Seed-Evolving", "Doubao-Seed-2.1-Pro", "Doubao-Seed-2.1-Turbo",
  "step-5-preview", "glm-5.3", "glm-5.2", "deepseek-v4.1-flash",
  "DeepSeek-V4-Flash-Official", "DeepSeek-V4-Pro-Official", "kimi-k3",
  "kimi-k2.7-code", "kimi-k2.6", "minimax-m3", "qwen3.8-max",
  "qwen-3.7-plus",
];

/**
 * TraeCode 的对外清单（演示用）。
 *
 * 与 TraeWork 那份**刻意不同**：`/v1/models` 按 API Key 的归属程序位取，
 * 且只列该程序位 `function` 能调的那批（上游按 function 做白名单，2026-09-30 实测）。
 * 演示站若两份给一样的名字，就会把「归属程序位真的改变对外清单」这件事藏起来。
 */
const TRAE_GATEWAY_MODEL_NAMES_TRAE_CODE = [
  "Doubao-Seed-Evolving", "Doubao-Seed-2.1-Pro", "step-5-preview",
  "glm-5.3-flashx", "glm-5.3-flash", "glm-5.3", "glm-5.2",
  "deepseek-v4.1-flash", "kimi-k3", "qwen3.8-flash", "qwen3.8-max",
];

/** `args.variant` 决定给哪份清单（缺省 = TraeWork，与后端默认变体一致）。
 *
 * ⚠️ 合并版（`trae_merged` / `global_trae_merged`）与遗留 TraeCode **同属 IDE 家族**
 * ⇒ 给同一份清单（客户端家族决定 chat 分组，见 Rust 侧 `function_for`）。 */
function demoTraeGatewayModels(args?: Record<string, unknown>): unknown {
  const codeFamily = ["trae_cn", "global_trae_code", "trae_merged", "global_trae_merged"];
  const names =
    typeof args?.variant === "string" && codeFamily.includes(args.variant)
      ? TRAE_GATEWAY_MODEL_NAMES_TRAE_CODE
      : TRAE_GATEWAY_MODEL_NAMES;
  return {
    object: "list",
    data: names.map((name) => ({
      id: name, object: "model", created: 1753600000, owned_by: "trae",
    })),
  };
}

/**
 * 演示用的**客户端（上游下发）**模型清单，**按程序位各一份**。
 *
 * 刻意与 `demoTraeGatewayModels` 的静态清单**不同**（多出 `Doubao-Seed-Evolving`、
 * `deepseek-v4.1-flash` 这类真实名字，且按 function 分成两组）—— 截图要能看出
 * 「这是客户端里那份随上游刷新的清单，不是写死的对外清单」，否则演示站会掩盖两者的差别。
 * 目录与 uid 一律是**明显的虚构值**，不照抄任何真机数据。
 *
 * ⚠️ 两条程序位给的是**不同内容**（TraeCode 那份带 `builder` / `chat_v3` 分组与
 * 专有的 `glm-5.3-flash`）：演示站必须能看出「切程序位会换一份清单」，
 * 否则这个选择器在截图里看起来像个假控件。
 */
function demoTraeClientModels(args?: Record<string, unknown>): TraeClientModelList {
  const model = (
    name: string,
    displayName: string,
    isDefault = false,
    // Max Mode 口径的上下文上限（`trae-types.ts` 的 `contextWindowMax`，必填）。
    // 默认 `null`（取不到）；旗舰模型给 1M，让演示站能看出这个新字段的差异。
    contextWindowMax: number | null = null,
    // 能力面（issue #65）：档位 + 积分倍率。不传 = 该条目没下发（渲染「未下发」）。
    ability: Partial<Pick<TraeClientModel, "efforts" | "defaultEffort" | "thinking" | "creditsRate">> = {},
  ): TraeClientModel => ({
    name,
    displayName,
    modelType: "reasoning_model",
    multimodal: true,
    isDefault,
    isPreset: true,
    isNew: false,
    isBeta: false,
    contextWindow: 256000,
    contextWindowMax,
    promptMaxTokens: 224000,
    ...ability,
  });

  // 合并版与遗留 TraeCode 同属 IDE 家族 ⇒ 同一份客户端清单；
  // 回显的 `variant` / `variantLabel` 用**请求值**，否则演示站会把 Trae 说成 TraeCode
  // （那正是用户报过一次的那类错位）。
  const requested = typeof args?.variant === "string" ? args.variant : "";
  const codeFamily = ["trae_cn", "global_trae_code", "trae_merged", "global_trae_merged"];
  if (codeFamily.includes(requested)) {
    const merged = requested === "trae_merged" || requested === "global_trae_merged";
    // 展示名与程序位表同源（`trae.program.*`），规则 = **国内带 ` CN`、国际不带后缀**。
    const variantLabel =
      requested === "trae_cn"
        ? "TraeCode CN"
        : requested === "global_trae_code"
          ? "TraeCode"
          : requested === "trae_merged"
            ? "Trae CN"
            : "Trae";
    return {
      variant: (merged ? requested : "trae_cn") as TraeVariantId,
      variantLabel,
      source: "client-cache",
      readAt: 1753600000000,
      dataDir: "C:\\Users\\Demo\\AppData\\Roaming\\Trae CN",
      uid: "7000000000000002",
      groups: [
        {
          function: "chat_v3",
          models: [
            model("Doubao-Seed-Evolving", "Seed-Evolving", false, null, {
              thinking: "unsupported",
              creditsRate: 0.8,
            }),
            model("glm-5.3-flash", "GLM-5.3-Flash", false, null, {
              efforts: ["light", "high"],
              defaultEffort: "high",
              thinking: "supported",
              creditsRate: 0.2,
            }),
            model("glm-5.3", "GLM-5.3", false, 1_000_000, {
              efforts: ["light", "high", "extra_high"],
              defaultEffort: "high",
              thinking: "supported",
              creditsRate: 0.83,
            }),
            model("deepseek-v4.1-flash", "DeepSeek-V4.1-Flash", false, null, {
              efforts: ["light", "high"],
              defaultEffort: "high",
              thinking: "supported",
              creditsRate: 0.06,
            }),
            model("openrouter//stealth/ox-alpha", "ox-alpha"),
          ],
        },
        {
          function: "builder",
          models: [model("Doubao-Seed-Code", "Seed-Code", true)],
        },
      ],
      note: null,
    };
  }

  return {
    variant: "trae_work",
    variantLabel: "TraeWork CN",
    source: "client-cache",
    readAt: 1753600000000,
    dataDir: "C:\\Users\\Demo\\AppData\\Roaming\\TRAE SOLO CN",
    uid: "7000000000000001",
    groups: [
      {
        function: "solo_work_lite",
        models: [
          model("Doubao-Seed-Evolving", "Seed-Evolving", false, 1_000_000, { creditsRate: 0.8 }),
          model("Doubao-Seed-2.1-Pro", "Seed-2.1-Pro-0915", false, null, { creditsRate: 0.8 }),
          /* ★ 真机实测（2026-10-21）：`solo_work_lite` 22 条里**只有 1 条**带档位，
             所以演示数据也只给这一条 —— 其余显示「该程序位未下发」，而不是假装
             「都不支持思考」。这条差异正是 issue #65 要暴露的事实。 */
          model("deepseek-v4.1-flash", "DeepSeek-V4.1-Flash", false, null, {
            efforts: ["light", "high"],
            defaultEffort: "high",
            thinking: "supported",
            creditsRate: 0.06,
          }),
          model("glm-5.3", "GLM-5.3", false, null, { creditsRate: 0.83 }),
          model("kimi-k3", "Kimi-K3", false, null, { creditsRate: 0.06 }),
          // 真实客户端清单里每个分组都带这一条（`provider = "openrouter"`）——
          // 演示数据也要有，否则「第三方路由条目被标成网关不提供」这条断言在演示下无从验证。
          model("openrouter//stealth/ox-alpha", "ox-alpha"),
        ],
      },
      {
        function: "solo_coder",
        models: [model("Doubao-Seed-2.0-Code", "Doubao-Seed-2.0-Code", true)],
      },
    ],
    note: null,
  };
}

/**
 * 演示用的 Trae 多 Key 列表（**4 个程序位各一把** + 一条历史归属）。
 *
 * 刻意覆盖全部 4 个程序位（`trae_work` / `trae_cn` / `global` / `global_trae_code`
 * 中的前三个，第四个本机未装国际版 TraeCode，演示里也不假装有），
 * 加一条**改造前建的历史 Key**（存的是区域标识 `cn`，且已吊销）：
 * 截图要能看出「归属程序位」列与「状态」列的差异，否则演示站会掩盖归属列的存在。
 *
 * ⚠️ **回归证据（2026-09-30 方向反转，别照着旧注释改回去）**：
 * 那条历史 Key 存的是 `cn`，后端 `TraeVariant::parse("cn")` 落**区域主程序 TraeWork**，
 * 因此它与第一条**必须都显示「TraeWork」**。旧注释要求它显示「国内版」——
 * 那个口径已随「归属列改回程序位」作废（程序位现在真的决定 `/v1/models` 与 `function`）。
 * 若哪天有人把归属列改回按区域取名，这条会显示成「国内版」，与第一条并列成一处**假差异**。
 *
 * 明文 / hash 一律不出现（与真实 `list_response` 的脱敏白名单一致）。
 */
function demoTraeApiKeys(): { keys: TraeApiKeyRecord[] } {
  /* 与 WB 侧同规则：每条都带 `suffix`（新 Key 是明文末 4 位）。
     第 4 条是 **legacy 形态**：后端把 legacy 的展示串**拆成两段**
     （`prefix` = 前 12 字符、`suffix` = 末 4 位，见 `trae/apikey.rs` 的 `legacy_prefix`），
     所以这里也必须是 `sk-trae-0a1b` + `cdef`，**不是**带省略号的整串。 */
  return {
    keys: [
      { id: "demo-trae-key-1", name: t("shared.demo.trae.key.cursorCn"), variant: "trae_work", prefix: "sk-trae-9f2c", suffix: "7d31", createdAt: atLocalTime(3, 17, 3), revokedAt: null, revoked: false, lastUsedAt: atLocalTime(0, 17, 10) },
      { id: "demo-trae-key-2", name: t("shared.demo.trae.key.continueCode"), variant: "trae_cn", prefix: "sk-trae-6d18", suffix: "a0e9", createdAt: atLocalTime(2, 20, 12), revokedAt: null, revoked: false, lastUsedAt: atLocalTime(0, 16, 58) },
      { id: "demo-trae-key-3", name: t("shared.demo.trae.key.cherryGlobal"), variant: "global", prefix: "sk-trae-4b7e", suffix: "2f4a", createdAt: atLocalTime(2, 9, 40), revokedAt: null, revoked: false, lastUsedAt: atLocalTime(0, 16, 41) },
      { id: "demo-trae-key-4", name: t("shared.demo.trae.key.legacy"), variant: "cn", prefix: "sk-trae-0a1b", suffix: "cdef", createdAt: atLocalTime(9, 8, 0), revokedAt: atLocalTime(1, 12, 30), revoked: true, lastUsedAt: atLocalTime(3, 10, 5) },
    ],
  };
}

function demoTraeGatewayLogs(): unknown {
  const now = Math.floor(Date.now() / 1000);
  const logs: TraeGatewayLogEntry[] = [
    { ts: (now - 60) * 1000, endpoint: "/v1/chat/completions", method: "POST", account: "7481920", model: "deepseek-v4-flash", status: 200, latencyMs: 1840, promptTokens: 1204, completionTokens: 386, stream: true, error: null },
    { ts: (now - 900) * 1000, endpoint: "/v1/chat/completions", method: "POST", account: "7481963", model: "glm-5.3", status: 429, latencyMs: 220, promptTokens: 0, completionTokens: 0, stream: true, error: t("shared.demo.trae.gatewayUpstreamError", { name: t("shared.demo.trae.name.codeLab"), status: 429 }) },
    { ts: (now - 3600) * 1000, endpoint: "/v1/chat/completions", method: "POST", account: "7481920", model: "glm-5.3", status: 200, latencyMs: 3120, promptTokens: 4021, completionTokens: 1188, stream: false, error: null },
  ];
  return { logs };
}

/**
 * Trae Token 统计（源 = 网关请求日志）。
 *
 * ★ **所有数字都从 `dayTotals` 派生**：同一个量在「统计卡 / 堆叠柱 / 环形 / 热力图」
 * 里显示成不同的数，就是**图在撒谎**（本仓的红线）。改造前这里各写各的常量，
 * `summary.total`（6791）与 `daily` 之和（15981）就对不上。
 */
function demoTraeTokenStatistics(days?: number): TraeTokenStatistics {
  const dayTotals = [0, 0, 0, 0, 1860, 4210, 3120, 6791];
  const dayKeys = dayTotals.map((_, index) => localDate(dayTotals.length - 1 - index));
  const total = dayTotals.reduce((sum, value) => sum + value, 0);
  const records = dayTotals.filter((value) => value > 0).length;
  const errors = 1;
  /** 输入 / 输出按固定比例拆（保证 `input + output === total`）。 */
  const input = Math.round(total * 0.77);
  const output = total - input;

  const bucket = (key: string, value: number, count: number, failed = 0) => ({
    key,
    total: value,
    input: Math.round(value * 0.77),
    output: value - Math.round(value * 0.77),
    records: count,
    errors: failed,
    streamRequests: Math.max(0, count - 1),
    avgLatencyMs: value > 0 ? 1727 : 0,
    p95LatencyMs: value > 0 ? 3120 : 0,
  });

  /* 模型 / 账号各占多少：**第二个吃余数**，保证求和 === total。 */
  const modelSplit = [
    { key: "deepseek-v4-flash", share: 0.62, errors: 0 },
    { key: "glm-5.3", share: 0.38, errors },
  ];
  const accountSplit = [
    { key: "7481920", name: t("shared.demo.trae.name.jackey"), share: 0.85, errors: 0 },
    { key: "7481963", name: t("shared.demo.trae.name.codeLab"), share: 0.15, errors },
  ];
  const splitByShare = <T extends { share: number }>(rows: T[], value: number) => {
    let remaining = value;
    return rows.map((row, index) => {
      const part = index === rows.length - 1 ? remaining : Math.round(value * row.share);
      remaining -= part;
      return { ...row, value: part };
    });
  };

  const models = splitByShare(modelSplit, total).map((row) => bucket(row.key, row.value, 2, row.errors));
  const accounts = splitByShare(accountSplit, total).map((row) => ({
    ...bucket(row.key, row.value, 2, row.errors),
    name: row.name,
    shortId: row.key,
  }));

  /* 按天 × 模型：每天按同一比例拆（求和 === 当天总量 ⇒ 堆叠柱与逐日柱同源）。 */
  const modelDaily = dayKeys.flatMap((date, index) => {
    const dayTotal = dayTotals[index];
    if (dayTotal === 0) return [];
    return splitByShare(modelSplit, dayTotal).map((row) => ({
      date,
      model: row.key,
      total: row.value,
      input: Math.round(row.value * 0.77),
      output: row.value - Math.round(row.value * 0.77),
      records: 1,
    }));
  });

  /* 「周-小时」槽位（`"{weekday}-{HH}"`，周一 = 0）：只给**有调用的槽位**（后端亦然）。
     share 之和 = 1 ⇒ 槽位总量之和 === `total`（热力图与其它图同源）。 */
  const hotSlots = [
    { weekday: 0, hour: 10, share: 0.2 },
    { weekday: 1, hour: 11, share: 0.11 },
    { weekday: 2, hour: 10, share: 0.26 },
    { weekday: 3, hour: 14, share: 0.09 },
    { weekday: 4, hour: 9, share: 0.16 },
    { weekday: 5, hour: 15, share: 0.06 },
    { weekday: 6, hour: 16, share: 0.12 },
  ];
  const hours = splitByShare(hotSlots, total).map((slot) =>
    bucket(`${slot.weekday}-${String(slot.hour).padStart(2, "0")}`, slot.value, 1),
  );

  return {
    source: "trae-gateway", label: t("shared.demo.trae.tokenSource"),
    generatedAt: Date.now(), rangeDays: days ?? 30,
    logFile: "/demo/buddy-switch/trae/api_gateway_logs.json",
    summary: { total, input, output, records, errors, streamRequests: records - errors, avgLatencyMs: 1727, p95LatencyMs: 3120 },
    models,
    accounts,
    daily: dayKeys.map((key, index) =>
      bucket(key, dayTotals[index], dayTotals[index] > 0 ? 1 : 0, index === dayKeys.length - 1 ? errors : 0),
    ),
    hours,
    statuses: [{ key: "200", records: records - errors }, { key: "429", records: errors }],
    modelDaily,
    variantCounts: { cn: 3, global: 1, unlabeled: 1, all: 5 },
    unsupported: localizedStubs(CAPABILITY_STUBS.tokenStats),
    filesScanned: 1, parseErrors: 0,
    coverageStartAt: Date.now() - 3600_000, coverageEndAt: Date.now(),
    note: t("shared.demo.trae.tokenNote"),
  };
}

/**
 * 运行日志（演示数据）。
 *
 * 文案逐字取自 `store::append_log` 的真实调用点，这样演示站看到的行
 * 与真实运行时的行是同一种形状，视觉核对才有意义。
 */
/**
 * 运行日志（本工具自身）的演示数据。
 *
 * ⚠️ 演示站**只**演示读；`clear_runtime_logs` / `open_runtime_logs_dir` 是动作，
 * 刻意不在此列（演示环境不能真删本机日志或弹文件管理器）。
 */
function demoRuntimeLogs(args?: Record<string, unknown>): RuntimeLogsResponse {
  const day0 = localDate(0);
  const day1 = localDate(1);
  const stamp = (daysAgo: number, hour: number, minute: number, second = 0) => {
    const date = new Date(atLocalTime(daysAgo, hour, minute));
    date.setSeconds(second);
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}:${String(date.getSeconds()).padStart(2, "0")}`;
  };
  const all = [
    { level: "info" as const, scope: "startup", time: stamp(1, 9, 2), ts: atLocalTime(1, 9, 2), message: "Buddy Switch 桌面端 启动 · 版本 2026.10.221931 · windows x86_64 · pid 18244 · 日志级别 info" },
    { level: "info" as const, scope: "gateway", time: stamp(1, 9, 2, 1), ts: atLocalTime(1, 9, 2) + 1000, message: "已启动: http://127.0.0.1:57891" },
    { level: "warn" as const, scope: "trae-gateway", time: stamp(1, 9, 2, 2), ts: atLocalTime(1, 9, 2) + 2000, message: "启动失败: 端口 57894 已被占用" },
    { level: "info" as const, scope: "switch", time: stamp(0, 9, 14), ts: atLocalTime(0, 9, 14), message: "切换成功 · 目标账号 7481999 · 客户端已重启" },
    { level: "error" as const, scope: "checkin", time: stamp(0, 8, 6), ts: atLocalTime(0, 8, 6), message: "签到失败: 上游返回 401（凭据可能已过期）" },
    { level: "debug" as const, scope: "pool", time: stamp(0, 9, 15), ts: atLocalTime(0, 9, 15), message: "选号明细: 可用 3 / 冷却 1 / 熔断 0" },
  ].filter((entry) => entry.time.startsWith(day0) || entry.time.startsWith(day1));

  const level = typeof args?.level === "string" ? (args.level as RuntimeLogLevel) : "";
  const date = typeof args?.date === "string" ? args.date : "";
  const keyword = typeof args?.keyword === "string" ? args.keyword.toLowerCase() : "";
  const order: RuntimeLogLevel[] = ["error", "warn", "info", "debug"];
  const minRank = level ? order.indexOf(level) : -1;
  const entries = all.filter((entry) => {
    if (date && !entry.time.startsWith(date)) return false;
    if (minRank >= 0 && order.indexOf(entry.level) > minRank) return false;
    if (keyword && !`${entry.scope} ${entry.message}`.toLowerCase().includes(keyword)) return false;
    return true;
  });

  const counts: Record<RuntimeLogLevel, number> = { error: 0, warn: 0, info: 0, debug: 0 };
  for (const entry of all) counts[entry.level] += 1;

  return {
    entries: entries.slice().sort((a, b) => b.ts - a.ts),
    total: entries.length,
    truncated: false,
    limit: 500,
    dates: [day0, day1],
    counts,
    level: "info",
    logDir: "/demo/buddy-switch/logs",
    keepDays: 7,
    maxPartBytes: 4194304,
    maxPartsPerDay: 4,
  };
}

function demoTraeLogs(args?: Record<string, unknown>): TraeLogsResponse {  const day0 = localDate(0);
  const day1 = localDate(1);
  const day2 = localDate(2);
  const stamp = (daysAgo: number, hour: number, minute: number) => {
    const date = new Date(atLocalTime(daysAgo, hour, minute));
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}:${String(date.getSeconds()).padStart(2, "0")}`;
  };
  const all = [
    { kind: "checkin" as const, time: stamp(0, 8, 6), date: day0, message: t("shared.demo.trae.log.checkinDone", { ok: 1, already: 1, failed: 1, total: 3 }) },
    { kind: "app" as const, time: stamp(0, 8, 41), date: day0, message: t("shared.demo.trae.log.thawed", { uid: "7481999", credits: 64.5 }) },
    { kind: "switch" as const, time: stamp(0, 9, 12), date: day0, message: t("shared.demo.trae.log.savedLogin", { uid: "7481920", count: 9 }) },
    { kind: "checkin" as const, time: stamp(0, 9, 20), date: day0, message: t("shared.demo.trae.log.jwtRefreshed", { uid: "7481920", hours: 320.5 }) },
    { kind: "app" as const, time: stamp(0, 10, 3), date: day0, message: t("shared.demo.trae.log.deviceReset", { count: 6 }) },
    { kind: "checkin" as const, time: stamp(1, 8, 5), date: day1, message: t("shared.demo.trae.log.checkinDone", { ok: 3, already: 0, failed: 0, total: 3 }) },
    { kind: "switch" as const, time: stamp(1, 16, 20), date: day1, message: t("shared.demo.trae.log.savedLogin", { uid: "7481999", count: 9 }) },
    { kind: "app" as const, time: stamp(2, 22, 10), date: day2, message: t("shared.demo.trae.log.thawed", { uid: "7482044", credits: 8 }) },
  ];

  const kind = typeof args?.kind === "string" ? args.kind : "";
  const date = typeof args?.date === "string" ? args.date : "";
  const keyword = typeof args?.keyword === "string" ? args.keyword.toLowerCase() : "";
  const entries = all.filter((entry) => {
    if (kind && kind !== "all" && entry.kind !== kind) return false;
    if (date && entry.date !== date) return false;
    if (keyword && !entry.message.toLowerCase().includes(keyword)) return false;
    return true;
  });

  const dates = [...new Set(all.map((entry) => entry.date))].sort().reverse();
  return {
    entries,
    total: entries.length,
    limit: 500,
    dates,
    counts: {
      all: all.length,
      app: all.filter((entry) => entry.kind === "app").length,
      checkin: all.filter((entry) => entry.kind === "checkin").length,
      switch: all.filter((entry) => entry.kind === "switch").length,
    },
    sources: [
      { kind: "app", label: t("shared.demo.trae.log.app"), path: "/demo/buddy-switch/trae/logs/app.log", exists: true },
      { kind: "checkin", label: t("shared.demo.trae.log.checkin"), path: "/demo/buddy-switch/trae/logs/checkin.log", exists: true },
      { kind: "switch", label: t("shared.demo.trae.log.switch"), path: "/demo/buddy-switch/trae/logs/switcher.log", exists: true },
    ],
    logDir: "/demo/buddy-switch/trae/logs",
    note: t("shared.demo.trae.logNote"),
  };
}

/**
 * 全局备份导出（演示）。
 *
 * 形状与 `backup.rs` 的 envelope **同源**（`kind` / `formatVersion` / `exportedAt` /
 * `includesCredentials` / 按 scope 分组的 `scopes` / `appSettings`）。
 *
 * ⚠️ **刻意不含任何凭据键**：演示文件也必须满足红线（文件里搜 `access_token` ⇒ 0 命中），
 * 否则演示站会把「默认不含凭据」这件事演成反例。
 */
function demoBackupEnvelope(): unknown {
  return {
    kind: "buddy-switch-backup",
    formatVersion: 1,
    appVersion: "2026.10.1",
    exportedAt: new Date().toISOString(),
    includesCredentials: false,
    scopes: Object.fromEntries(
      ["wb-cn", "wb-global", "trae-cn", "trae-global"].map((scope) => [
        scope,
        { accounts: [], config: {}, gateway: { keys: [], models: [], pool: [] } },
      ]),
    ),
    appSettings: { "buddy-switch.language": "zh", "buddy-switch.theme": "system" },
  };
}

/** 自动备份 / 导入前快照（演示，最新在前）。 */
function demoAutoBackups(): { backups: AutoBackupEntry[] } {
  return {
    backups: [
      { fileName: "pre-import-2026-10-04-0912.json", path: "/demo/buddy-switch/backups/pre-import-2026-10-04-0912.json", sizeBytes: 184_320, modifiedAt: atLocalTime(0, 9, 12), kind: "preImport" },
      { fileName: "auto-backup-2026-10-04-0300.json", path: "/demo/buddy-switch/backups/auto-backup-2026-10-04-0300.json", sizeBytes: 176_128, modifiedAt: atLocalTime(0, 3, 0), kind: "auto" },
      { fileName: "auto-backup-2026-10-03-0300.json", path: "/demo/buddy-switch/backups/auto-backup-2026-10-03-0300.json", sizeBytes: 174_080, modifiedAt: atLocalTime(1, 3, 0), kind: "auto" },
    ],
  };
}

/** Read-only demo response provider. It never reads or mutates real user data. */
export function screenshotDemoResponse(command: string, args?: Record<string, unknown>): unknown {
  const demoAccounts = hydratedAccounts();
  const activeIndex = Math.max(0, demoAccounts.findIndex((account) => account.id === demoActiveCliAccountId));
  const activeAccount = demoAccounts[activeIndex] ?? demoAccounts[0];
  const cliStatus: CodeBuddyCliStatus = { configured: true, settingsPresent: true, helperPresent: true, helperSupportsAccountIds: true, activeIndex, activeAccountId: activeAccount.id, activeAccountName: activeAccount.nickname, accountCount: demoAccounts.length, statePath: "/demo/codebuddy-cli-state.json" };
  const config = rotateConfig();
  const rotateStatus: RotateStatus = { config, cliConfigured: true, activeAccountId: demoAccounts[0].id, activeAccountName: demoAccounts[0].nickname, lastCheckAt: atLocalTime(0, 9, 30), lastSwitchAt: atLocalTime(1, 16, 20) };
  const githubConfig: GithubConfig = { owner: "zhangjia", repo: "buddy-switch", proxy: "" };
  switch (command) {
    case "get_status": return demoRegionStatus(args?.region === "global" ? "global" : "cn");
    case "get_accounts": return { accounts: demoAccounts };
    /* P5 T03：`accounts_list` 是 WorkBuddy / Trae **共用**的参数化命令（靠 `scope` 区分平台），
       所以演示数据也必须按 scope 分 —— 否则 Trae 页面在演示模式下会拿到 WB 的账号
       （那是最难发现的一类假通过：页面有数据、只是数据是错的）。 */
    case "accounts_list": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return scope.startsWith("trae") ? demoTraeAccounts() : { accounts: demoAccounts };
    }
    /* P5 T03：`credits` 是 WorkBuddy / Trae **共用**的参数化命令（靠 `scope` 区分平台），
       演示数据也必须按 scope 分 —— 否则 Trae 页面在演示模式下会拿到 WB 的积分形状。 */
    case "credits": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return scope.startsWith("trae")
        ? demoTraeCredits()
        : creditExpiry(String(args?.accountId ?? ""));
    }
    /* P5 T03：`checkin_status` 是 WorkBuddy / Trae **共用**的参数化命令（靠 `scope` 区分平台），
       演示数据也必须按 scope 分 —— 否则 Trae 页面在演示模式下会拿到 WB 的单账号签到形状。 */
    case "checkin_status": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return scope.startsWith("trae")
        ? demoTraeCheckinStatus()
        : { ok: true, todayCheckedIn: true };
    }
    case "get_codebuddy_cli_status": return cliStatus;
    case "get_codebuddy_cn_ide_status": return {
      installed: true,
      running: false,
      dataDir: "/demo/CodeBuddy CN",
      dbPath: "/demo/CodeBuddy CN/User/globalStorage/state.vscdb",
      dbExists: true,
      appPath: "/demo/CodeBuddy CN.app",
      activeAccountId: demoAccounts[0].id,
      activeAccountName: demoAccounts[0].nickname,
      detectedFrom: "state.vscdb",
      statePath: "/demo/codebuddy-cn-ide-state.json",
    } satisfies CodeBuddyCnIdeStatus;
    /* 编辑器内 CodeBuddy 插件（VS Code / JetBrains）：演示态刻意给**两种档位** ——
       VS Code 已装且记录了当前账号（渲染「当前账号」角标），JetBrains 已装但未记录
       （渲染可切换按钮）⇒ 截图与 `build:demo` 冒烟一次覆盖两种形态。 */
    case "get_vscode_codebuddy_plugin_status": return {
      installed: true,
      extensionInstalled: true,
      running: false,
      loggedIn: true,
      dataDir: "/demo/Code",
      dbPath: "/demo/Code/User/globalStorage/state.vscdb",
      dbExists: true,
      activeAccountId: demoAccounts[1].id,
      activeAccountName: demoAccounts[1].nickname,
      detectedFrom: "state",
      statePath: "/demo/vscode-ext-state.json",
    } satisfies VscodeCodebuddyPluginStatus;
    case "get_jetbrains_codebuddy_plugin_status": return {
      installed: true,
      pluginInstalled: true,
      running: false,
      loggedIn: false,
      configRoot: "/demo/JetBrains",
      targets: [
        {
          configDir: "IntelliJIdea2026.2",
          pluginInstalled: true,
          running: false,
          loggedIn: false,
          secretPath: "/demo/JetBrains/IntelliJIdea2026.2/options/secret-storage.xml",
        },
      ],
      activeAccountId: null,
      activeAccountName: null,
      detectedFrom: "secret-storage",
      statePath: "/demo/jetbrains-state.json",
    } satisfies JetbrainsCodebuddyPluginStatus;
    case "switch_codebuddy_cli_account": {
      const target = demoAccounts.find((account) => account.id === args?.accountId);
      if (!target) throw new Error(t("shared.demo.error.accountMissing"));
      demoActiveCliAccountId = target.id;
      return { ok: true, configured: true, synced: true, verified: true, activeIndex: demoAccounts.indexOf(target), activeAccountId: target.id, message: t("shared.demo.switchDone") } satisfies CodeBuddyCliSwitchResult;
    }
    case "get_checkin_status": {
      /* ★ 按账号给：原型里 qa-bot / design-lab 是「今日未签到」，其余「已签到」。
         `api.getCheckinStatusMap` 在演示模式下**逐个账号**调这里（见 `api.ts`）。 */
      const accountId = String((args as { accountId?: unknown } | undefined)?.accountId ?? "");
      const notCheckedIn = accountId === "demo-account-d" || accountId === "demo-account-e";
      return { ok: true, todayCheckedIn: !notCheckedIn };
    }
    case "get_credit_expiry": return creditExpiry(String(args?.accountId ?? ""));
    case "get_credit_statistics": return buildStatistics();
    case "get_token_statistics": return demoTokenStatistics(typeof args?.days === "number" ? args.days : undefined);
    case "get_auto_checkin_config": return checkinConfig();
    case "get_checkin_logs": return { logs: checkinLogs() };
    case "get_travel_status": return travelStatus(String(args?.accountId ?? ""));
    case "get_auto_travel_config": return travelConfig();
    case "get_switch_config": return { copy_sessions_by_default: false, pin_current_account: true };
    case "get_schedule_config": return {
      // 演示数据用**非整点**（09:07 / 21:23）：截图要能自证「分钟粒度真的生效」，
      // 全用整点的话，控件退化成只能选整点也看不出来。
      checkin_times: ["09:07", "21:23"],
      travel_times: ["09:00", "21:00"],
      activity_times: ["10:00"],
      keepalive_times: ["22:00"],
      school_times: ["12:00"],
      cat_times: ["01:00"],
      trae_checkin_times: ["09:00", "21:00"],
      trae_jwt_refresh_times: ["09:00", "21:00"],
      qoder_checkin_times: ["09:00", "21:00"],
      qoder_keepalive_times: ["22:00"],
      // 签到开 ±15 分钟抖动：截图要能自证「这个能力存在且真的在界内」，
      // 全为 0 的话控件看起来就只是个永远不动的下拉框。
      checkin_jitter_minutes: 15,
      travel_jitter_minutes: 0,
      activity_jitter_minutes: 0,
      keepalive_jitter_minutes: 0,
      school_jitter_minutes: 0,
      cat_jitter_minutes: 0,
      trae_checkin_jitter_minutes: 0,
      trae_jwt_refresh_jitter_minutes: 0,
      qoder_checkin_jitter_minutes: 0,
      qoder_keepalive_jitter_minutes: 0,
      checkin_enabled: true,
      travel_enabled: true,
      activity_enabled: true,
      keepalive_enabled: true,
      school_enabled: true,
      cat_enabled: true,
      // 与后端默认值一致（Trae 自动签到默认关闭）——演示数据也必须如实，
      // 否则截图会误导成「装上就是开着的」。
      trae_checkin_enabled: false,
      // 与后端默认值一致（Trae JWT 续期**默认开启**，与上面那条方向相反）。
      trae_jwt_refresh_enabled: true,
      // Qoder 两条同样照后端默认值（签到关、保活开）。
      qoder_checkin_enabled: false,
      qoder_keepalive_enabled: true,
      activity_report_count: 5,
    } satisfies ScheduleConfig;
    case "get_auto_rotate_config": return config;
    case "rotate_status": return rotateStatus;
    case "get_rotate_logs": return { logs: rotateLogs() };
    case "get_github_config": return githubConfig;
    case "check_update": return { ok: true, current: "2026.9.16", latest: "2026.9.17", latestTag: "v2026.9.17", hasUpdate: true, releaseName: t("shared.demo.updateTitle"), releaseUrl: "https://github.com/NextAgentX/qoder-trae-workbuddy-switch/releases/tag/v2026.9.17" };
    case "get_launch_at_login_enabled": return true;
    /* 应用级偏好（宿主也要读的那一类）。演示站里「关闭窗口」本来就没有真窗口，
       但开关必须能显示、也必须能记住 → 读写同一份会话内状态，归一化规则与后端
       `app_prefs::normalize_app_prefs` 一致（非布尔 ⇒ 回落默认 true）。 */
    case "get_app_prefs": return { ...demoAppPrefs };
    // 「清除缓存」的只读扫描（清除动作是写操作，由 DemoAction 统一拦截）。
    case "cache_scan": return demoCacheScan();
    case "set_app_prefs": {
      const raw = (args?.prefs as { close_to_tray?: unknown } | undefined)?.close_to_tray;
      /* 每次只改 `close_to_tray`，其余键保持原值 —— 与后端「部分字段写入不会
         抹掉其它已知键」的语义一致（同意时间戳不能被一次开关切换清掉）。 */
      demoAppPrefs = {
        close_to_tray: typeof raw === "boolean" ? raw : true,
        legal_consent_at: demoAppPrefs.legal_consent_at,
      };
      return { ...demoAppPrefs };
    }
    case "switch_progress": return { running: false, progress: null };
    /* P8 T02 · 会话页两个只读命令。`list_session_replicas` 形状与后端逐字段一致；
       `list_sessions` 是既有命令，补进演示面以便会话页 join 标题 / updatedAt。 */
    case "list_sessions":
      return { sessions: demoSessions(), current: "demo-user-001", source: "db" };
    case "list_session_replicas":
      return demoSessionReplicas();
    /* 客户端限流台账（只读观测）。演示数据为**虚构**：一条仍受限 + 一条已恢复，
       用于呈现「仍受限置顶 / 已恢复灰显」的排序与状态（不伪造真实账号数据）。 */
    case "client_limits": return demoClientLimits();
    /* 「今日 Token」按账号聚合（只读）。演示数据**如实演示三条诚实边界**：
       demo-user-002 **刻意缺席** ⇒ 卡片渲染「—」（今日无记录/未知，非 0）；
       demo-user-003 值为**全 0** ⇒ 渲染真实 `0`；`gatewayTruncated: true` ⇒ 渲染下界 `≥N`。 */
    case "get_account_token_today": return demoAccountTokenToday();
    /* 网关逐日趋势（只读）。演示数据覆盖「有量 / 缺日 / 下界」三条边界（见上）。 */
    case "get_gateway_token_trend": return demoGatewayTokenTrend();
    /* 账本视图（issue #63）：三维度历史 + 今日对账。演示数据守住「三维度合计 = 网关卡」。 */
    case "get_gateway_usage_dim": return demoGatewayUsageDim();
    /* Trae / Qoder 的账本与 WB **返回体同形** ⇒ 演示站共用同一份构造：
       演示只演「形状与自洽约束」，不追求与真实平台同模型名。 */
    case "get_trae_gateway_usage_dim":
    case "get_qoder_gateway_usage_dim": return demoGatewayUsageDim();
    /* Trae 客户端账本（导入明文库）：演示站固定演「还没导入」这一态 ——
       它正是这条链路最常见、也最需要讲清楚的状态（不是空图，是「你还没导出」）。 */
    case "probe_trae_client_db":
      return {
        ok: false,
        path: "",
        error: { code: "missing", message: "还没有指定明文库" },
        tables: [],
      };
    case "get_trae_client_db_config": return { path: null, exists: false };
    case "get_trae_client_token_stats":
      return { source: "trae-client", configured: false };
    case "get_usage_reconcile": return demoUsageReconcile();
    /* Skill 管理清单（只读）。演示数据覆盖启用 / 停用 / 不一致 / 无 frontmatter 四种形态。 */
    case "list_skills": return demoSkills(args?.region === "global" ? "global" : "cn");
    /* 上游客户端 settings.json 托管（**只读** 4 条）。演示数据**虚构**，覆盖
       未安装 / 运行中 / 四种排版等分支（patch / restore 是写操作，不进演示只读面）。 */
    case "list_client_settings_slots": return demoClientSettingsSlots();
    case "read_client_settings":
      return demoClientSettingsRead(String(args?.slot ?? "workbuddy-cn"));
    case "list_client_settings_backups":
      return demoClientSettingsBackups(String(args?.slot ?? "workbuddy-cn"));
    case "diff_client_settings":
      return demoClientSettingsDiff(
        String(args?.slotA ?? "workbuddy-cn"),
        String(args?.slotB ?? "trae-code-cn"),
      );
    case "get_gateway_config": return demoGatewayConfig();
    case "gateway_status": return demoGatewayStatus();
    case "list_api_keys": return { keys: demoApiKeys() };
    case "get_gateway_models": return demoCatalog(args?.region === "global" ? "global" : "cn");
    case "get_account_strategy": return demoStrategyMap();
    /* P11：禁用偏好只读命令。演示数据**如实为空**（默认全开，D27）——
       截图里若凭空标出「已禁用」会误导用户以为装上就有模型被关掉。
       ⚠️ **例外**：Qoder 国内版演示里**真的**有一条被禁用（`demoQoderGatewayModels`
       把 `Kimi-K3` 标成 `disabled`，与原型 `GATEWAY["qoder-cn"]` 一致）。
       两份数据必须**同源**：清单说「已禁用」而这里说「没禁用」，表格与计数就会各说各话
       （实测：行不标灰、计数显示 4/4 —— 正是「图在撒谎」）。 */
    case "get_disabled_models": {
      /* 形状与真机同构（键 = **作用域**）。 */
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "wb-cn");
      return { scope, ids: scope === "qoder-cn" ? ["Kimi-K3"] : [] };
    }
    /* 模型降级链（P13）：演示站给**真实形状**的链 —— 目标里留一个「闸门中」的，
       这样状态点（黄）与诊断带都会出现。全绿的空壳截图看不出这功能在解决什么问题。
       ⚠️ 模型名必须与 `demoCatalog` **同源**（GLM-5.3 / GLM-5.2 / DeepSeek-V4-Pro）：
       两边不一致时抽屉里的下拉一个都匹配不上，链会显示成一串空占位符。 */
    case "get_model_fallback": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "wb-cn");
      const now = Date.now();
      const chains =
        scope === "wb-cn"
          ? [
              {
                from: "GLM-5.3",
                targets: [
                  // 目录里 GLM-5.2 也不支持图片（见表格「图片输入」列）⇒ 与真实派生保持一致：
                  // 一个目标可以同时「闸门中」+「不支持图片」，两条诊断都会出现。
                  { model: "GLM-5.2", state: "gated", gated_for_secs: 42, caps_lost: ["image"] },
                  // 这一条让「不支持图片输入」的诊断在演示站也能看见（真实数据来自目录声明）。
                  { model: "DeepSeek-V4-Pro", state: "healthy", gated_for_secs: null, caps_lost: ["image"] },
                ],
                triggers: ["gate", "model_rate", "model_blocked", "server"],
                enabled: true,
              },
              {
                from: "GLM-5.2",
                targets: [
                  { model: "DeepSeek-V4-Pro", state: "healthy", gated_for_secs: null, caps_lost: [] },
                ],
                triggers: ["gate", "model_rate"],
                enabled: true,
              },
            ]
          : [];
      return {
        scope,
        enabled: chains.length > 0,
        chains,
        max_targets: 2,
        baseline: ["gate", "model_rate", "model_blocked"],
        events:
          scope === "wb-cn"
            ? [
                {
                  scope: "wb-cn",
                  at_ms: now - 6 * 60_000,
                  from: "GLM-5.3",
                  path: ["GLM-5.2", "DeepSeek-V4-Pro"],
                  reason: "model_rate",
                  account: "jackey",
                  ms: 3800,
                  result: "degraded",
                },
                {
                  scope: "wb-cn",
                  at_ms: now - 24 * 60_000,
                  from: "GLM-5.3",
                  path: ["GLM-5.2"],
                  reason: "model_gate",
                  account: "jackey",
                  ms: 1200,
                  result: "degraded",
                },
                {
                  scope: "wb-cn",
                  at_ms: now - 41 * 60_000,
                  from: "GLM-5.3",
                  path: [],
                  reason: "",
                  account: "jackey",
                  ms: 300,
                  result: "blocked",
                },
              ]
            : [],
      };
    }
    /* 降级链**预演**（P13）：后端是纯计算（零副作用），所以演示模式也照形状返回。
       演示数据里 GLM-5.2 是「闸门中」的那个 —— 预演要如实跳过它，否则截图在撒谎。 */
    case "get_fallback_stats": {
      const scope = String((args as { scope?: unknown })?.scope ?? "wb-cn");
      const days = 7;
      const items = Array.from({ length: days }, (_, index) => {
        const date = new Date(Date.now() - index * 86_400_000);
        const iso = date.toISOString().slice(0, 10);
        // 前三天给点真实感：降级确实发生过，不是「全 0」的空壳。
        const degraded = index === 0 ? 3 : index === 1 ? 5 : index === 2 ? 2 : 0;
        const failed = index === 0 ? 1 : 0;
        return { date: iso, degraded, failed };
      });
      return {
        scope,
        days: items,
        total: {
          degraded: items.reduce((sum, item) => sum + item.degraded, 0),
          failed: items.reduce((sum, item) => sum + item.failed, 0),
        },
      };
    }
    case "preview_model_fallback": {
      const payload = (args ?? {}) as { from?: unknown; targets?: unknown };
      const from = String(payload.from ?? "");
      const targets = Array.isArray(payload.targets)
        ? payload.targets.filter((item): item is string => typeof item === "string")
        : [];
      const steps = [
        { model: from, role: "primary", action: "try", gated_for_secs: null as number | null },
        ...targets.slice(0, 2).map((model) =>
          model === "GLM-5.2"
            ? { model, role: "target", action: "skip_gated", gated_for_secs: 42 }
            : { model, role: "target", action: "try", gated_for_secs: null as number | null },
        ),
      ];
      return { ok: true, scope: String((args as { scope?: unknown })?.scope ?? "wb-cn"), steps };
    }
    /* 图片能力覆盖（issue #101）：演示站给一条「手动指定」样例，让三态都能画出来
       （其余模型缺键 = 跟随上游）。写操作 `set_model_image_cap` 不进演示面。 */
    case "get_model_image_caps": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "wb-cn");
      return { scope, caps: scope === "wb-cn" ? { "deepseek-v4.1-flash": true } : {} };
    }
    case "get_gateway_logs": return { logs: demoGatewayLogs() };
    /* 限流探测**只读**状态：演示站从未跑过探测 ⇒ 如实返回 idle（与真机 `model_probe::status` 同形）。
       `probe_start` / `probe_cancel` 是写操作，不进 DEMO_READ_COMMANDS，由 DemoAction 统一拦截。 */
    case "probe_status": return { phase: "idle" };
    /* 探测结果台账（跨重启）：演示站从未探过 ⇒ 空表 + `updated_at_ms: null`。
       ⚠️ 返回 `cells: []` 而不是 `null`：前端按 `cells.length` 判断「没探过」。 */
    case "probe_ledger":
      return {
        scope: String((args as { scope?: unknown } | undefined)?.scope ?? "wb-cn"),
        updated_at_ms: null,
        cells: [],
      };
    // ---- Trae 分区（只读；写操作不进这里，由 DemoAction 统一拦截）----
    case "get_trae_env": return demoTraeEnv();
    case "get_trae_variants": return demoTraeVariants();
    case "get_trae_capabilities": return demoTraeCapabilities();
    case "get_trae_accounts": return demoTraeAccounts();
    case "get_trae_checkin_status": return demoTraeCheckinStatus();
    /* issue #47：逐账号「今日」只读重取（演示数据保留「缺席 / 读失败 / 不可得」三条边界）。 */
    case "get_trae_checkin_status_map": return demoTraeCheckinStatusMap(demoAccountIds(args));
    case "get_trae_account_token_today": return demoTraeAccountTokenToday();
    case "get_trae_credits": return demoTraeCredits();
    case "get_trae_profiles": return demoTraeProfiles(args);
    case "get_trae_settings": return demoTraeSettings();
    case "get_trae_token_statistics":
      return demoTraeTokenStatistics(typeof args?.days === "number" ? args.days : undefined);
    case "get_trae_logs": return demoTraeLogs(args);
    case "runtime_logs": return demoRuntimeLogs(args);
    case "get_trae_gateway_config": return demoTraeGatewayConfig();
    case "trae_gateway_status": return demoTraeGatewayStatus();
    case "get_trae_gateway_models": return demoTraeGatewayModels(args);
    case "get_trae_client_models": return demoTraeClientModels(args);
    case "list_trae_api_keys": return demoTraeApiKeys();
    case "get_trae_gateway_logs": return demoTraeGatewayLogs();
    /* P9 · 悬浮窗**只读**快照。
       ⚠️ 演示数据**如实**反映「本期只接已有数据源的来源」：演示时没有进行中的操作 ⇒
       **结构化空态**（`items: []` + `source: "unavailable"`），由 T03 渲染空态四要素。
       **绝不**为「看起来丰富」塞演示行（P9 §1.3 / §6 R7）——真实 provider 同款
       （见 `src-tauri/src/commands.rs::companion_snapshot`）。 */
    case "companion_status": return {
      items: [],
      source: "unavailable",
      updated_at: Date.now(),
    } satisfies CompanionSnapshot;
    /* P15 · 外部 Agent 会话监听状态（**只读**）。
       ⚠️ 演示站**必须**如实反映「默认关、什么都没装」：`enabled: false` + 空 `slots`。
       演示环境绝不能出现「看起来已经装了 hook」的假象 —— 那会让读者以为本工具
       会偷偷改客户端配置（真实实现是默认关 + 用户显式同意，见 P15 决策 D41）。 */
    case "get_agent_hook_status": return {
      enabled: false,
      port: 57893,
      slots: [],
    } satisfies AgentHookStatus;
    /* Qoder（第三个平台）· 只读两条 —— 与 `DEMO_READ_COMMANDS` 的登记一一对应
       （门禁 `check-api-contract.cjs` 校验 case 存在，缺了演示模式会抛
       「演示模式缺少只读数据」）。 */
    case "get_qoder_accounts": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return demoQoderAccounts(scope);
    }
    case "get_qoder_local_sources": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return demoQoderLocalSources(scope);
    }
    /* issue #111 · 逐资源包明细（只读）。演示里给三条**不同状态**的包 + 一条套餐占位，
       截图能同时看到「正常 / 临期 / 已过期 / 零额度」四种行内形态。 */
    case "qoder_credits": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return demoQoderCredits(scope);
    }
    case "qoder_web_credit_packs": {
      const uid = String((args as { uid?: unknown } | undefined)?.uid ?? "");
      return demoQoderWebPacks(uid);
    }
    case "qoder_web_session_status":
      /* 演示站「已登录」：浮层直接展示逐包，不需要登录引导。 */
      return {
        hasSession: true,
        expired: false,
        pending: false,
        source: "stored",
        cookieNames: ["qoder_session"],
        savedAtMs: Date.now() - 3_600_000,
        verifiedAtMs: Date.now() - 600_000,
      };
    /* 登录窗 Cookie 探针（只读、不打上游）：演示站没有登录窗 ⇒ 如实回「没开」。
       ⚠️ 不要在这里假装 `windowOpen: true` + 有 cookie：那会让演示端的登录轮询
       永远「发现变化」并去调 probe，而 probe 在演示面里是动作（不可用）。 */
    case "qoder_web_window_cookies":
      return {
        windowOpen: false,
        cookieCount: 0,
        cookieNames: [] as string[],
        fingerprint: "",
      } satisfies QoderWebWindowCookies;
    case "qoder_program_status": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return demoQoderPrograms(scope);
    }
    case "qoder_usage": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "");
      return demoQoderUsage(scope);
    }
    /* issue #47：逐账号「今日签到状态」（只读；演示数据保留「缺席 / 未开放」两条边界）。 */
    case "qoder_checkin_status_map": return demoQoderCheckinStatusMap(demoAccountIds(args));
    /* 限额护栏视图（只读）：演示里三条状态各占一个账号，截图能同时看到。 */
    case "qoder_gateway_quota":
      return demoQoderGatewayQuota();
    /* 一次性 Pro 升级包（+1800）：演示里**活动在线且可领**，截图才看得到入口。 */
    case "qoder_pro_pack_status":
      return { available: true, eligible: true, error: null };
    case "qoder_pro_pack_claim":
      return {
        ok: true,
        already: false,
        available: true,
        message: "已领取 Pro 升级包（+1800 积分）",
      };
    /* Qoder 网关（只读五条）—— 形状与真机逐字段同构（snake_case / 未过滤清单 +
       逐条 `disabled` / 来源三级）。写操作（保存配置 / 建 Key / 吊销…）不进演示只读面。 */
    case "get_qoder_gateway_config": return demoQoderGatewayConfig();
    case "qoder_gateway_status": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "qoder-cn");
      return demoQoderGatewayStatus(scope);
    }
    case "get_qoder_gateway_models": {
      const scope = String((args as { scope?: unknown } | undefined)?.scope ?? "qoder-cn");
      return demoQoderGatewayModels(scope);
    }
    case "list_qoder_api_keys": return demoQoderApiKeys();
    case "get_qoder_gateway_logs": return demoQoderGatewayLogs();
    /* P7 全局备份：只登记**只读**命令（导出 / 预览 / 导入 / 恢复是动作，不进演示只读面 ——
       由页面侧的错误提示处理）。`get_auto_backup_config` **如实返回默认开**（§7 #7），
       否则截图会误导成「装上就是关着的」。 */
    case "export_backup": return demoBackupEnvelope();
    case "list_auto_backups": return demoAutoBackups();
    case "get_auto_backup_config": return { enabled: true } satisfies AutoBackupConfig;
    default: throw new Error(t("shared.demo.error.missingReadOnly", { command }));
  }
}
