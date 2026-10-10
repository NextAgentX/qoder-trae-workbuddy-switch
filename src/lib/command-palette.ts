/**
 * 命令面板（⌘K）的**纯逻辑层** —— P6 / PLAN §3.7 / §3.13。
 *
 * ## 为什么把逻辑抽离组件
 *
 * 面板要做三件事：① 把「页面 / 作用域 / 账号 / 动作」四组条目拼出来；② 按关键词过滤；
 * ③ 把命中项渲染成可执行的行。其中 ①② 是**纯函数**（入参全在 {@link PaletteContext}，
 * 不碰 DOM、不读全局），因此可以被门禁直接加载断言 —— 前端没有单测框架，
 * 见 `scripts/check-scope-registry.cjs` 的 P6 段。
 *
 * ## ★ 唯一搜索索引（§3.13 硬要求）
 *
 * 面板的账号匹配**必须**复用 `@/lib/account-search`（列表搜索 `/` 用的同一份实现）。
 * 若这里另写一套 `includes`，同一个关键词会在列表与面板给出**不同结果** ——
 * 那是静默错误（用户不会收到提示，只会觉得工具不一致）。因此本文件**只调用**
 * `accountSearchText` / `matchesAccountQuery`，**不自己归一化**（不出现 `toLowerCase`）。
 *
 * ## ★ 字段归属（本仓出过两次事故，逐字段核实）
 *
 * 把两个平台的账号归一成 {@link PaletteAccount} 时按**真实类型**取字段：
 *
 * | 归一字段 | WorkBuddy（`AccountMeta`） | Trae（`TraeAccount`） |
 * |:---|:---|:---|
 * | 稳定键 `key` | `id` | **`userId`**（Trae 账号**没有** `id` / `uid`） |
 * | 展示名 `displayName` | **`nickname`**（**没有** `name`） | `name` |
 * | `uid` | `uid` | 用 `userId` 充当（`accountSearchText` 的 `uid` 槽） |
 * | `remark` | `remark` | `remark` |
 * | **禁止引用** | ~~`lastUsedAt`~~（属 `ApiKeyRecord`） | ~~`lastUsedAt`~~（属 `TraeApiKeyRecord`） |
 *
 * ## 白屏防线（issue #2）
 *
 * 展示字段一律先过 {@link displayText}：后端可能把「昵称」吐成对象，
 * 对象被当 **React 子节点**渲染会卸载整棵树 ⇒ 白屏。本文件在组装 `title` 前完成归一。
 */

import { accountSearchText, matchesAccountQuery } from "@/lib/account-search";
import { displayText } from "@/lib/display-text";
import type { Destination } from "@/lib/platforms/registry";
import { platformDescriptor } from "@/lib/platforms/registry";
import type { Translate } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import type { Scope } from "@/lib/scope";
import { scopeParts } from "@/lib/scope";
import type { AccountMeta } from "@/lib/types";
import type { TraeAccount } from "@/lib/trae-types";
import type { PaletteAction } from "@/lib/palette-actions";

/** 条目分组。顺序即渲染顺序（页面 / 作用域 / 账号 / 动作）。 */
export type PaletteGroup = "page" | "scope" | "account" | "action";

/** 面板里的一行。**所有展示字段都已是字符串**（组装前过了 `displayText`）。 */
export interface PaletteItem {
  /** 稳定 key（React key + 门禁断言用）。 */
  id: string;
  group: PaletteGroup;
  /** 已 i18n 的标题。 */
  title: string;
  /** 已 i18n 的副标题（可空）。 */
  subtitle?: string;
  /** 快捷键提示（如 `"⌘3"`）；**未绑定 / 降级时为 `undefined`**（§3.4 规则 1）。 */
  shortcutHint?: string;
  /** 已归一的搜索文本（复用 `accountSearchText` 的归一规则）。 */
  searchText: string;
  /** 执行该行（导航 / 动作）。 */
  run: () => void;
}

/**
 * 跨作用域账号的**归一视图**（字段归属见文件头表）。
 *
 * ⚠️ 刻意**不含** `lastUsedAt` —— 它属 `ApiKeyRecord` / `TraeApiKeyRecord`，
 * 不是账号字段。引用它只会读到 `undefined`（本仓已因此翻过一次车）。
 */
export interface PaletteAccount {
  scope: Scope;
  /** 稳定键：WB = `AccountMeta.id`；Trae = `TraeAccount.userId`。 */
  key: string;
  /** 展示名：WB = `nickname`；Trae = `name`。 */
  displayName: string;
  email?: string | null;
  uid?: string | null;
  remark?: string | null;
}

/**
 * 动作执行所需的**副作用接口** —— 由组件（持有 `api` / `navigate`）注入。
 *
 * 动作清单（{@link PaletteAction}）住在 `palette-actions.ts` 里保持纯净、可被门禁加载；
 * 真正碰 `api` 的部分收敛到本接口，经 `ctx.services` 传入 —— 这样「基础动作在此注册」
 * 与「palette-actions.ts 零重依赖」两件事同时成立。
 */
export interface PaletteServices {
  /** 刷新某作用域的账号数据（就地，不导航）。 */
  refreshScope: (scope: Scope) => void | Promise<void>;
  /** 打开某作用域的「新增账号」入口（通常导航到账号页）。 */
  addAccount: (scope: Scope) => void;
  /** 打开某作用域的数据目录。 */
  openDataDir: (scope: Scope) => void | Promise<void>;
}

/** `buildPaletteItems` 的全部输入 —— 纯数据，便于门禁构造。 */
export interface PaletteContext {
  t: Translate;
  currentScope: Scope;
  scopes: readonly Scope[];
  /** 作用域 → 快捷键序号；降级时为空 Map（序号提示据此渲染）。 */
  scopeShortcutIndex: ReadonlyMap<Scope, number>;
  accounts: readonly PaletteAccount[];
  destinations: readonly Destination[];
  navigate: (to: string) => void;
  actions: readonly PaletteAction[];
  /** 动作副作用（见 {@link PaletteServices}）。 */
  services: PaletteServices;
}

/** 目的地 → 侧栏文案键（与 `App.tsx` 的 `DEST_META` 同键，保证面板与侧栏同名）。 */
const DESTINATION_LABEL_KEY: Record<Destination, TranslationKey> = {
  overview: "nav.overview",
  accounts: "nav.accounts",
  usage: "nav.usage",
  sessions: "nav.sessions",
  gateway: "nav.apiService",
  settings: "nav.settings",
};

/** 区域 → 版本名文案键（与 `lib/region.ts` 的 `versionLabel` 同键）。 */
const REGION_LABEL_KEY: Record<"cn" | "global", TranslationKey> = {
  cn: "shared.region.version.cn",
  global: "shared.region.version.global",
};

/**
 * 作用域的展示名（`WorkBuddy · 国内版`）—— 面板与账号副标题共用。
 *
 * ⚠️ 平台名走 `platformDescriptor().labelKey`（注册表唯一来源），区域名走 `t()`
 * 现取 —— 不 import `lib/region.ts`（它连带 i18n 词表，会让门禁加载变重且脆弱）。
 */
export function scopeLabel(scope: Scope, t: Translate): string {
  const parts = scopeParts(scope);
  const productKey = platformDescriptor(parts.product).labelKey;
  return `${t(productKey)} · ${t(REGION_LABEL_KEY[parts.region])}`;
}

/**
 * WorkBuddy 账号 → {@link PaletteAccount}。稳定键取 `id`、展示名取 **`nickname`**。
 *
 * ⚠️ 这里**不出现** `account.name` —— WB 账号类型没有 `name` 字段。
 */
export function paletteAccountFromWorkbuddy(account: AccountMeta, scope: Scope): PaletteAccount {
  return {
    scope,
    key: account.id,
    displayName: displayText(account.nickname) ?? "",
    email: displayText(account.email),
    uid: displayText(account.uid),
    remark: displayText(account.remark),
  };
}

/**
 * Trae 账号 → {@link PaletteAccount}。稳定键取 **`userId`**、展示名取 `name`。
 *
 * ⚠️ 这里**不出现** `account.id` / `account.uid` —— Trae 账号类型没有这两个字段。
 */
export function paletteAccountFromTrae(account: TraeAccount, scope: Scope): PaletteAccount {
  return {
    scope,
    key: account.userId,
    displayName: displayText(account.name) ?? "",
    uid: displayText(account.userId),
    remark: displayText(account.remark),
  };
}

/** 账号条目的展示名：展示名为空时按「邮箱 → uid → 稳定键」回落，避免空标题。 */
function accountTitle(account: PaletteAccount): string {
  return account.displayName || account.email || account.uid || account.key;
}

/**
 * 组装四组条目。**纯函数**：不导航、不取数，`run` 只是闭包（调用方执行）。
 *
 * 三条硬约束落在实现里：
 * 1. **绑定与序号提示同源**（§3.4 规则 1）：`shortcutHint` 只在 `scopeShortcutIndex`
 *    命中时产生 —— 降级（空 Map）时序号提示**自动消失**，绝不出现「显示了 ⌘3、按了没反应」；
 * 2. **空组不渲染**（§1.7）：无账号 ⇒ 不产账号条目；调用方按 `group` 分组后空组自然消失；
 * 3. **动作走扩展点**：`ctx.actions`（含 P6 基础动作 + P7 注册动作），`enabled` 为假则不产条目。
 */
export function buildPaletteItems(ctx: PaletteContext): PaletteItem[] {
  const items: PaletteItem[] = [];

  // ── 页面组：当前作用域的 5 个目的地 ──────────────────────────────
  for (const dest of ctx.destinations) {
    const title = ctx.t(DESTINATION_LABEL_KEY[dest]);
    items.push({
      id: `page:${dest}`,
      group: "page",
      title,
      searchText: accountSearchText({ name: title }),
      run: () => ctx.navigate(`/s/${ctx.currentScope}/${dest}`),
    });
  }

  // ── 作用域组：全部作用域（含各自登录态摘要的位置留给调用方）────────
  for (const scope of ctx.scopes) {
    const index = ctx.scopeShortcutIndex.get(scope);
    const title = scopeLabel(scope, ctx.t);
    items.push({
      id: `scope:${scope}`,
      group: "scope",
      title,
      subtitle: scope === ctx.currentScope ? ctx.t("palette.scope.current") : undefined,
      // ★ 只有绑定存在时才给提示（降级时 index 为 undefined ⇒ 无提示）。
      shortcutHint: typeof index === "number" ? `⌘${index}` : undefined,
      searchText: accountSearchText({ name: title, group: scope }),
      run: () => ctx.navigate(`/s/${scope}/accounts`),
    });
  }

  // ── 账号组：**跨作用域**全部账号 ────────────────────────────────
  for (const account of ctx.accounts) {
    const name = accountTitle(account);
    items.push({
      id: `account:${account.scope}:${account.key}`,
      group: "account",
      title: name,
      subtitle: ctx.t("palette.account.inScope", { scope: scopeLabel(account.scope, ctx.t) }),
      // 搜索文本与列表搜索**同字段同顺序**（复用 accountSearchText 的归一）。
      searchText: accountSearchText({
        name: account.displayName,
        email: account.email,
        uid: account.uid,
        remark: account.remark,
      }),
      // 回车 → 跳到该作用域账号页并带一次性 `?q=<名>`（§1.8 / §3.13）。
      run: () => ctx.navigate(`/s/${account.scope}/accounts?q=${encodeURIComponent(name)}`),
    });
  }

  // ── 动作组：基础动作（P6）+ 注册动作（P7）────────────────────────
  for (const action of ctx.actions) {
    if (action.enabled && !action.enabled(ctx)) continue; // 条件不满足 ⇒ 不渲染（§3.4 规则 1）
    const title = ctx.t(action.titleKey);
    items.push({
      id: `action:${action.id}`,
      group: "action",
      title,
      subtitle: action.subtitleKey ? ctx.t(action.subtitleKey) : undefined,
      searchText: accountSearchText({ name: title }),
      run: () => {
        void action.run(ctx);
      },
    });
  }

  return items;
}

/**
 * 过滤条目。**空查询返回全部**（与列表搜索一致）；命中判定复用 `matchesAccountQuery`。
 *
 * ⚠️ 这里**不另写匹配**：`matchesAccountQuery` 已做「小写 + 去空白」归一，
 * 面板与列表因此对同一关键词给出一致结果（§3.13）。
 */
export function filterPaletteItems(items: readonly PaletteItem[], query: string): PaletteItem[] {
  if (!query.trim()) return [...items];
  return items.filter((item) => matchesAccountQuery(item.searchText, query));
}
