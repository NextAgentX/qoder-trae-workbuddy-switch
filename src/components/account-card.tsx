import { Ban, Check, CircleCheck, Coins, Loader2, PlaneTakeoff, QrCode } from "lucide-react";

import { Badge } from "@/components/ui/badge";
// P0 · 程序位表（含「计划新增 / 尚未支持」态）—— 展示侧镜像，与后端 `programs.rs` 逐字对齐。
import { PLANNED_PROGRAMS } from "@/lib/platforms/registry";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DemoAction } from "@/components/demo-action";
import { AccountMoreMenu, AccountMoreMenuDemo } from "@/components/account-more-menu";
import type { AccountRefreshKind } from "@/lib/account-refresh";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { shouldOpenAccountDetail } from "@/lib/account-open-guard";
import { ENCRYPTED_CREDENTIAL_REASON } from "@/lib/api";
import { compactTokens, exactTokens } from "@/components/usage/format";
import { demoModeEnabled } from "@/lib/demo-mode";
import { displayText } from "@/lib/display-text";
import {
  EXPIRY_TIP_MAX_HEIGHT,
  daysUntil,
  nearestExpiringBatch,
  summarizeExpiring,
  upcomingExpiries,
} from "@/lib/credit-expiry";
import { formatAbsoluteTime, formatRelativeTime } from "@/lib/catalog-source";
import { useT, type Translate } from "@/lib/i18n";
import type { AccountMeta, AccountTokenTodayEntry, CreditExpiry, TravelStatus } from "@/lib/types";

/**
 * ★ 程序位按钮三态的视觉规格 —— **逐值对齐原型 `.prog`**（`demo.css:326-338 / 825`）。
 *
 * 抽成模块级常量是因为同一个形状在卡片里出现三次（IDE / CLI / 插件位），
 * 各自内联写必然漂移 —— 而这三态的**区别本身就是信息**：
 *
 * | 态 | 原型 | 含义 | 视觉 |
 * |:--|:--|:---|:---|
 * | `on` | `:332` | 该程序位**正在用** | 底 primary 18% + 描边 primary 40% + 字 primary + **字重 600** |
 * | `off` | `:335-336` | **本机没装客户端** | `opacity:.5` + **虚线**；hover 转警示色并恢复不透明度 |
 * | `planned` | `:825` | 本应用**尚未支持** | 虚线 + `opacity:.5` + 禁用 |
 *
 * ⚠️ **`off` 与 `planned` 必须视觉可分**（原型 `:823-824` 的明确要求）：
 * 「off 是去装就好，planned 是装了也没用」。二者都靠虚线弱化，
 * 靠 **hover 时的警示色**（off）与 **禁用光标+tooltip**（planned）区分。
 *
 * ⚠️ `off` **不禁用**（原型 `:333-334` 的注释）：禁用按钮既没有 tooltip 也没有反馈，
 * 用户只会觉得「坏了」—— 它必须可点，点进去才解释怎么装。
 */
const OFF_PROGRAM_CLASS =
  "border-dashed opacity-50 hover:border-warning hover:text-warning hover:opacity-80";
const PLANNED_PROGRAM_CLASS = "cursor-not-allowed border-dashed opacity-50";

/**
 * WorkBuddy 账号卡片。
 *
 * ## P4 卡片瘦身（PLAN §3.3）
 *
 * 详情（备注编辑、全部积分包）**已迁入共享抽屉**（`account-detail-drawer.tsx`），
 * 卡片只剩「允许的可点区」：**勾选框 / 程序位行 / 更多菜单**。
 * 移出的是**两类**交互 —— 备注就地编辑与资源包展开；其余（程序位、菜单、加密凭据
 * 错误态的扫码 CTA）按 §3.3 保留。
 *
 * ## 为什么程序位行只定义一处
 *
 * 程序位行是一整排 `<Button>`（最多 6 枚），静态计数看起来像 6 个可点区
 * —— 于是这里收敛成**单个 `programRow` 变量**，页脚引用同一个定义，
 * 可点区口径不再被重复计数污染（PLAN §3.3）。
 */

/**
 * 头像底色 / 首字色板（**6 选 1**，按账号名哈希稳定选取）。
 *
 * ## 历史：曾试过「语义色 + `/15`」，已废弃
 *
 * 最早两条写作 `bg-success/15 text-success` / `bg-warning/15 text-warning`，
 * 依赖 `--success` / `--warning` 跟明暗主题走。实测（oklch → sRGB → WCAG）：
 * 亮色主题下 `text-success` 压在 `bg-success/15` 上仅 **3.11:1**、`text-warning` **3.17:1**，
 * 都低于 AA 4.5:1 ⇒ 头像首字（12.5px/640）读不清。**这一条教训仍然成立**：
 * 不要用「语义色 + 低透明度底」承载小字号文字。
 *
 * 但它推不出「所以要用浅底 100 + 深字 800」—— 那是另一套取值，并非原型形态。
 * 现在的取值见下方 `AVATAR_TONES`（**深底 + 白字**，六条实测均 ≥ 5.46:1 过AA）。
 */
/**
 * ★ 头像色板 —— 对齐原型 `avatarTone()`（`buddy-switch-prototype.html:1921-1928`）。
 *
 * ## 形态：**深底 + 白字**
 *
 * 原型是 6 个 `oklch(L≤0.5)` 的深色底 + `oklch(0.99 0 0)` 白字（`demo.css:311`）。
 * 本文件此前用的是 `bg-*-100` **浅底 + `text-*-800` 深字** ⇒ 每张卡最显眼的色块
 * 与原型**相反**（原型深、我方浅），是卡片观感最大的偏差。
 *
 * ## 为什么回到深底（旧注释的判断被实测推翻）
 *
 * 旧注释的理由是「原型断言 `L ≤ 0.5` 会让白字读不清」。本次按 `oklch → sRGB → WCAG`
 * **实测换算**（不是估算）六条深底压白字的对比度：
 *
 * | oklch底 | sRGB | 对白字 |
 * |:--|:--|:--|
 * | `0.50 0.13 163` | `#00794D` | **5.46:1** |
 * | `0.48 0.14 265` | `#3658AC` | **6.70:1** |
 * | `0.50 0.13 300` | `#6F4FA1` | **6.35:1** |
 * | `0.50 0.14 60`  | `#9A4A00` | **6.24:1** |
 * | `0.48 0.13 20`  | `#99383E` | **7.00:1** |
 * | `0.50 0.12 220` | `#007192` | **5.55:1** |
 *
 * **六条全部 ≥ 4.5:1（AA）**，最低 5.46:1 ⇒ 原型取值是**过 AA 的**，
 * 旧注释担心的可读性问题不存在。而浅底方案（100 底 + 800 字）虽然也过 AA，
 * 但它不是原型形态 —— 在「以 demo 为准」的前提下没有理由保留。
 *
 * ⚠️ 深度用任意值写死（`[background:oklch(...)]`），**不依赖主题** ⇒ 明暗两套下都一样，
 * 不会像语义 token 那样在亮色下变浅。
 */
const AVATAR_TONES = [
  "[background:oklch(0.5_0.13_163)]",
  "[background:oklch(0.48_0.14_265)]",
  "[background:oklch(0.5_0.13_300)]",
  "[background:oklch(0.5_0.14_60)]",
  "[background:oklch(0.48_0.13_20)]",
  "[background:oklch(0.5_0.12_220)]",
] as const;

/**
 * 头像字色：统一近白（原型 `oklch(0.99 0 0)`，`demo.css:311`）。
 *
 * ⚠️ **导出**：Trae 账号卡的头像必须与 WorkBuddy 用同一套（字号 / 圆角 / 字色 / 首字取法），
 * 否则两个产品分区的卡片会各长一版头像 —— 而原型只有 `.avatar` 一种。
 */
export const AVATAR_TEXT_CLASS = "text-[oklch(0.99_0_0)]";

/**
 * 按账号名稳定地取一个头像底色。
 *
 * 与 `trae-account-card.tsx` 共用：两个产品分区的账号卡片必须是同一套视觉，
 * 各自维护一份色板迟早会漂移，因此这里导出而不是复制。
 */
export function avatarTone(name: string) {
  /* ⚠️ 哈希必须与原型 `avatarTone()`（`buddy-switch-prototype.html:1924-1926`）**逐步同源**：
     原型每一步都 `% 997`，我们此前用 `>>> 0`（保留整个 32 位）⇒ **同一个名字算出不同的色**
     （实测 qa-bot 原型是紫、我们是红）⇒ 逐卡对照时头像颜色对不上。 */
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) hash = (hash * 31 + name.charCodeAt(i)) % 997;
  return AVATAR_TONES[hash % AVATAR_TONES.length];
}

/**
 * 头像首字 —— 对齐原型 `initials()`（`buddy-switch-prototype.html:1929`）：
 * **去掉非字母数字/汉字的字符后取前 2 位**并转大写。
 *
 * ⚠️ 与 `avatarTone` 同源导出：Trae 侧账号卡也用同一套（否则两个产品线的头像会分叉）。
 * 「spare-global」→ `SP`、`jacky」→ `JA`、`花` → `花`（中文保留 1 字，够辨识）。
 */
export function accountInitials(name: string): string {
  const cleaned = (name || "?").replace(/[^a-zA-Z0-9一-龥]/g, "");
  return (cleaned || "?").slice(0, 2).toUpperCase();
}

export function formatCredits(value: number): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
}

/**
 * 「今日 Token」的**诚实**展示串（卡片与表格**同一份**实现，避免两处口径漂移）。
 *
 * 四条边界（见 `AccountTokenTodayResponse`）：
 * - `entry` 缺失 = 该账号今日**无记录 / 未知** ⇒ **「—」**（**绝不**当 0）；
 * - 出现在 `entry` 里的 `0` 是**真实 0**（`compactTokens(0)` = `"0"`）；
 * - `truncated`（网关日志的**今日**部分可能被裁）⇒ 网关部分是**下界** ⇒ 整串渲染成 **`≥N`**；
 * - 客户端 / 网关**两侧都非零** ⇒ 合计可能重复计数（两条独立记录），说明由明细
 *   （{@link todayTokensBreakdown}）带上 —— headline 只负责数字，不负责解释。
 */
export function formatTodayTokens(
  entry: AccountTokenTodayEntry | null | undefined,
  truncated: boolean,
  t: Translate,
): string {
  if (!entry) return "—";
  const total = entry.clientIn + entry.clientOut + entry.gatewayIn + entry.gatewayOut;
  const value = compactTokens(total);
  return truncated ? t("wbAccounts.card.metric.atLeast", { value }) : value;
}

/**
 * 「今日 Token」的**明细**（客户端 / 网关 各自 输入·输出），供 tooltip / `title`。
 *
 * ⚠️ `truncated` 时**必须**把「下界」的说明一并带上 —— 只给数字不说口径就是误导。
 * `entry` 缺失时返回空串（无明细可讲）。
 *
 * ★ **双桶重叠披露**（2026-10-15）：客户端与网关是**两条独立记录**（前者来自客户端
 * 自己的会话日志，后者来自网关请求日志），正常情况下互不相交；但用户可以把某个被统计
 * 的客户端**指向本网关**，那样同一请求会两处各记一次 ⇒  headline 的合计**可能重复
 * 计数**。没有可靠判据能识别这种重叠（网关行不带客户端会话 id，反向也没有），按本模块
 * 「绝不猜」的取向**不做启发式去重** —— 改为在明细里说清楚：两侧都非零时追加一行说明。
 */
export function todayTokensBreakdown(
  entry: AccountTokenTodayEntry | null | undefined,
  truncated: boolean,
  t: Translate,
): string {
  if (!entry) return "";
  const parts = [
    t("wbAccounts.card.metric.todayTokensClient", {
      input: exactTokens(entry.clientIn),
      output: exactTokens(entry.clientOut),
    }),
    t("wbAccounts.card.metric.todayTokensGateway", {
      input: exactTokens(entry.gatewayIn),
      output: exactTokens(entry.gatewayOut),
    }),
  ];
  // 只有一侧为 0 时，合计就是另一侧、没有歧义 ⇒ 不必加噪音。
  const clientTotal = entry.clientIn + entry.clientOut;
  const gatewayTotal = entry.gatewayIn + entry.gatewayOut;
  if (clientTotal > 0 && gatewayTotal > 0) {
    parts.push(t("wbAccounts.card.metric.todayTokensOverlap"));
  }
  if (truncated) parts.push(t("wbAccounts.card.metric.todayTokensTruncated"));
  return parts.join(t("shared.punct.comma"));
}

export function formatCreditExpiry(ts: number | null, t: Translate): string {
  if (!ts) return t("wbAccounts.card.forever");
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return t("wbAccounts.card.forever");
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function accountIdentity(account: AccountMeta): string {
  // 先归一再看：脏值（对象）会让下面的 `split` 直接抛 TypeError，
  // 而渲染期的 TypeError 会顺着错误边界外的路径把整棵树带走（issue #2）。
  const email = displayText(account.email);
  if (email) {
    const [local, domain] = email.split("@");
    if (!domain) return email;
    return `${local.slice(0, 1)}${"*".repeat(Math.max(3, local.length - 1))}@${domain}`;
  }
  const uid = displayText(account.uid);
  if (uid) return `UID · ${uid}`;
  return `ID · ${displayText(account.id) ?? "—"}`;
}

/**
 * ★ B2：**展示用**的账号名 —— 优先昵称，缺昵称才退到脱敏身份。
 *
 * ## 为什么不直接用 `accountIdentity`
 *
 * 两者取舍相反：`accountIdentity` 把**脱敏邮箱**排在昵称**前面**（卡片主体要的是
 * 「这是谁」的唯一凭据），而 toast / 向导标题 / 确认页要的恰恰是**好读的名字** ——
 * 若直接换过去，有昵称的账号会从「张三」变成「z***@workbuddy.ai」，是可见的**退步**。
 *
 * ## 为什么必须有它
 *
 * 全仓曾有 **14 处** `nickname || email || uid || id` 的裸回落（toast、切换向导标题、
 * 确认页、迁移摘要…）。展示层虽已脱敏，但这些**回落链绕过了脱敏** ——
 * nickname 为空时（很常见：新导入的账号）就**泄露完整邮箱**。
 *
 * ⇒ 一律改走本函数：**昵称优先 → `accountIdentity`（已脱敏，且有 `UID ·`/`ID ·` 两段回落，
 * 不会为空）**。既不泄露，也不会渲染出空白。
 */
export function accountDisplayName(account: AccountMeta): string {
  return displayText(account.nickname) || accountIdentity(account);
}

/**
 * ★ B2：同上，但接受**只带身份三字段**的轻量对象（`AppStatus.current` 没有 `id`）。
 *
 * `AppStatus.current` 的形状是 `{ uid, nickname, email }`，**不是 `AccountMeta`**
 * （没有 `id`）⇒ 不能直接喂给 `accountIdentity`。这里按同样的优先级
 * **昵称 → 脱敏邮箱 → `UID · x`** 收口，避免调用方为了图省事又写回裸回落。
 */
export function identityDisplayName(
  person: { uid?: string | null; nickname?: string | null; email?: string | null } | null | undefined,
): string {
  if (!person) return "";
  return displayText(person.nickname) || accountIdentity(person as AccountMeta);
}

/**
 * 卡片徽章的**局部微调**。
 *
 * ⚠️ 这里**不再**覆盖圆角 / 字号 / 内边距：`Badge` 基础样式已经逐值对齐原型 `.badge`
 * （圆角 6 / 字号 10.5 / 内边距 0 7 / 高 20 / 图标 11）。此前这里写 `rounded-md px-1.5
 * text-[11px]` —— 而本主题的 `rounded-md` = `calc(var(--radius) - 2px)` = **10px**，
 * 于是卡片徽章比原型圆 4px、字号大 0.5px、内边距少 1px（实测 10px/11px/6px）。
 * 只留 `border-0`：原型 `.badge` 没有边框，而 `Badge` 基础样式带 `border`（变体给的是
 * `border-transparent`）—— 1px 透明边框不显色却占位，会让徽章比原型宽 2px。
 */
const chipClass = "border-0 px-[7px] font-medium";

/**
 * 旅行标签（**带文字**）。
 *
 * ⚠️ 2026-10-12：此前这里渲染的是**一枚只有图标、没有文字的徽章** ——
 * 原型 `.acct-chips` 里**没有任何无文字的 chip**（每条都是「图标 + 文字」或纯文字），
 * 只有图标的徽章在卡片上看起来像一个坏掉的空标签。
 * ⇒ 短文字（旅行中 / 已旅行）进徽章，**细节**（地点 · 奖励 · 剩余时间）留在 tooltip。
 */
function travelIconChip({
  text,
  tooltip,
  variant,
}: {
  /** 徽章上的短文字。 */
  text: string;
  /** tooltip 里的细节（地点 · 奖励 · 剩余时间）。 */
  tooltip: string;
  variant: "secondary" | "success";
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant={variant} className={chipClass} aria-label={tooltip}>
          <PlaneTakeoff /> {text}
        </Badge>
      </TooltipTrigger>
      <TooltipContent side="top">{tooltip}</TooltipContent>
    </Tooltip>
  );
}

function formatTravelRemaining(arriveAt: number | null | undefined, t: Translate): string | null {
  if (!arriveAt || arriveAt <= 0) return null;
  const arriveMs = arriveAt > 1e12 ? arriveAt : arriveAt * 1000;
  const remainingMs = arriveMs - Date.now();
  if (remainingMs <= 0) return t("wbAccounts.card.arrivingSoon");
  const totalMinutes = Math.max(1, Math.ceil(remainingMs / 60_000));
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours > 0 && minutes > 0) return t("wbAccounts.card.remainingHM", { hours, minutes });
  if (hours > 0) return t("wbAccounts.card.remainingH", { hours });
  return t("wbAccounts.card.remainingM", { minutes });
}

function travelTooltip(status: TravelStatus, t: Translate): string {
  const place = status.locationName?.trim();
  const credit = status.rewardCredit;
  const points = credit != null ? `+${credit}` : null;
  const remaining = formatTravelRemaining(status.arriveAt, t);
  if (status.label === "traveling") {
    const parts = [place, points ? t("wbAccounts.card.travelEst", { points }) : t("wbAccounts.card.traveling"), remaining].filter(Boolean);
    return parts.length > 0 ? parts.join(" · ") : t("wbAccounts.card.traveling");
  }
  if (status.label === "finished") {
    if (place && points) return `${place} · ${points}`;
    if (place) return `${place} · ${t("wbAccounts.card.finished")}`;
    if (points) return `${t("wbAccounts.card.finished")} · ${points}`;
    return t("wbAccounts.card.finished");
  }
  if (status.label === "no-buddy") return t("wbAccounts.card.noBuddy");
  return t("wbAccounts.card.notTraveled");
}

/** 按旅行状态渲染标签：无 Buddy / 未旅行 / 旅行中 / 已结束。 */
function travelChip(status: TravelStatus | undefined) {
  const t = useT();
  if (!status) return null;
  switch (status.label) {
    case "no-buddy":
      return <Badge variant="secondary" className={cn(chipClass, "text-muted-foreground")}>{t("wbAccounts.card.noBuddy")}</Badge>;
    case "traveling":
      return travelIconChip({ text: t("wbAccounts.card.traveling"), tooltip: travelTooltip(status, t), variant: "secondary" });
    case "finished":
      return travelIconChip({ text: t("wbAccounts.card.finished"), tooltip: travelTooltip(status, t), variant: "success" });
    case "untraveled":
    default:
      /* ★ 2026-10-12：**默认态不渲染**。
         原型 `.acct-chips` 里没有「什么都没发生」的标签，而真实数据下**每张卡**都会
         返回 `untraveled` ⇒ 每张卡都挂一枚灰标签，把 chips 行挤成两行
         （实测卡片 170 → 195px，与原型不一致）。只有**真在旅行 / 刚结束 / 无 Buddy**
         才是有信息量的状态，才值得占一枚 chip。 */
      return null;
  }
}

/** 未检测到的程序位引导目标（对应设置页里的子项）。 */
export type GuideTarget = "ide" | "cli";

interface Props {
  account: AccountMeta;
  onDelete: (a: AccountMeta) => void;
  onCheckin?: (a: AccountMeta) => void;
  onRefresh?: (a: AccountMeta) => void;
  /**
   * 单项刷新（积分 / 今日 Token / 签到状态 / 凭据状态）。
   *
   * 不传 ⇒ 菜单退回旧的单条「刷新 Token」项（向后兼容：页面还没接上时行为不变）。
   * 传了 ⇒ 渲染「刷新」分组，四项各自独立（理由见 `lib/account-refresh.ts` 文件头）。
   */
  onRefreshPart?: (a: AccountMeta, kind: AccountRefreshKind) => void;
  /** 本账号正在进行中的刷新项；用于把对应菜单项标成「刷新中…」。 */
  refreshingKinds?: readonly AccountRefreshKind[];
  /** 刚刚刷新成功（自行淡出）的项，菜单里显示「已更新」。 */
  doneKinds?: readonly AccountRefreshKind[];
  /** 「导出账号」（原型同位项）。页面侧弹导出对话框。 */
  onExport?: (a: AccountMeta) => void;
  /** 本平台真的能刷的项（缺省四项）；见 `RefreshMenuGroup` 的注释。 */
  refreshKinds?: readonly AccountRefreshKind[];
  onSwitch?: (a: AccountMeta) => void;
  todayCheckedIn?: boolean;
  /** 今日旅行状态（undefined=查询中/未知，不渲染标签） */
  travelStatus?: TravelStatus;
  credit?: CreditExpiry;
  creditLoading?: boolean;
  creditPriority?: boolean;
  workbuddyActive?: boolean;
  codebuddyCliConfigured?: boolean;
  /**
   * CLI 已被指向本机网关（issue #109）⇒「切到 CLI」置灰。
   *
   * 不传 = 非网关模式（默认可用）—— 与其它 `codebuddyCli*` props 同一约定：
   * 缺省是「没这个限制」，而不是「禁用」。聚合页与账号页**都必须传**。
   */
  codebuddyCliGatewayMode?: boolean;
  codebuddyCliActive?: boolean;
  /** 任一 CodeBuddy CLI 账号切换正在进行，用于阻止并发切换。 */
  codebuddyCliBusy?: boolean;
  onSwitchCodebuddyCli?: (a: AccountMeta) => void;
  /** 当前卡片是否为正在切换的目标账号。 */
  codebuddyCliLoading?: boolean;
  /** CodeBuddy CN IDE 是否已安装（可切换）。 */
  codebuddyCnIdeAvailable?: boolean;
  codebuddyCnIdeActive?: boolean;
  codebuddyCnIdeBusy?: boolean;
  codebuddyCnIdeLoading?: boolean;
  onSwitchCodebuddyCnIde?: (a: AccountMeta) => void;
  /**
   * 编辑器内 CodeBuddy 插件两个程序位（VS Code / JetBrains）。
   *
   * ⚠️ 与 `planned` 的措辞**必须分开**：`planned` = 本应用**尚未支持**（去装客户端也没用）；
   * 这里是**本机没装**（去装就有用）—— 两者混用会把用户引向错误动作。
   * `available` 由各自的后端状态命令给出（`installed` / `pluginInstalled`）。
   */
  vscodePluginAvailable?: boolean;
  vscodePluginActive?: boolean;
  vscodePluginBusy?: boolean;
  vscodePluginLoading?: boolean;
  onSwitchVscodePlugin?: (a: AccountMeta) => void;
  jetbrainsPluginAvailable?: boolean;
  jetbrainsPluginActive?: boolean;
  jetbrainsPluginBusy?: boolean;
  jetbrainsPluginLoading?: boolean;
  onSwitchJetbrainsPlugin?: (a: AccountMeta) => void;
  /**
   * 未检测到编辑器扩展时的出口（与「引导去设置」不同：这是**本机没装编辑器扩展**，
   * 设置页里没有对应子项 ⇒ 由调用方给一条说明性提示，指针指向「先装扩展」）。
   */
  onGuidePlugin?: (programId: string) => void;
  /**
   * 打开「OAuth 扫码添加」。
   *
   * 只在 `credit.reason === ENCRYPTED_CREDENTIAL_REASON` 时用到：这类账号的凭据
   * 我方解不开，光把错误摆出来用户不知道下一步做什么。卡片自己开不了弹窗
   *（弹窗由页面持有），所以走回调。
   */
  onAddPlaintextAccount?: () => void;
  /**
   * 未检测到的程序位（IDE / CLI）点击后**引导到设置**对应子项（PLAN §3.3 规则 1）。
   *
   * 为什么不就地禁用：一枚点不动的灰按钮**不告诉用户下一步做什么**；
   * 引导到设置页才能让「未检测到」变成可解决的问题。
   */
  onGuideToSetting?: (target: GuideTarget) => void;
  /** 是否被勾选（批量操作）。 */
  selected?: boolean;
  /** 切换勾选。 */
  onToggleSelect?: (a: AccountMeta) => void;
  /** 打开详情抽屉（抽屉状态由页面持有）。 */
  onOpenDetail?: (a: AccountMeta) => void;
  /** 「更多 → 编辑备注」：打开抽屉并进入编辑态。 */
  /** 「更多 → 设置分组」：打开抽屉并进入分组编辑态（与备注同一交互）。 */
  /**
   * 该账号「今日 Token」明细（`get_account_token_today`）。
   *
   * ⚠️ **缺席 = 今日无记录 / 未知** ⇒ 渲染「—」（**绝不当 0**）。
   */
  todayTokens?: AccountTokenTodayEntry | null;
  /** 网关日志已到保留上限 ⇒ 网关部分是下界 ⇒ 「今日 Token」渲染 `≥N`。 */
  todayTokensTruncated?: boolean;
  featuresDisabled?: boolean;
}

function ProductCurrentState({
  product,
  short,
}: {
  product: "workbuddy" | "codebuddy" | "codebuddy-cn";
  /** 短名（原型 `pg.short`：`WB` / `IDE` / `CLI`）—— 显示在 ✓ 之后。 */
  short: string;
}) {
  const t = useT();
  const title =
    product === "workbuddy"
      ? t("wbAccounts.card.currentWorkbuddy")
      : product === "codebuddy-cn"
        ? t("wbAccounts.card.currentIde")
        : t("wbAccounts.card.currentCli");
  return (
    <span
      role="status"
      aria-label={title}
      title={title}
      className={cn(
        /* 不用硬编码的 `inset 0 1px 0 rgba(255,255,255,.8)`：深色下那是一条刺眼的白线
           （原型 `.prog.on` 只有底色 + 描边，无内阴影）。 */
        "inline-flex items-center justify-center border border-primary/40 bg-primary/18 text-primary font-semibold",
        /* ⚠️ 与程序位**同一 footprint**（原型 `.prog` 26px / 圆角 8px / `gap:4px`）。
           内容 = `✓ + 短名`（原型 `.prog.on` 逐字一致，**没有产品图标**）。 */
        "h-[26px] gap-1 rounded-[8px] px-2 text-[11px]",
      )}
    >
      <Check className="size-3" strokeWidth={2.25} />
      <span>{short}</span>
    </span>
  );
}

export function AccountCard({ account, onDelete, onCheckin, onRefresh, onRefreshPart, refreshingKinds, doneKinds, refreshKinds, onExport, onSwitch, todayCheckedIn, travelStatus, credit, creditLoading, creditPriority, workbuddyActive, codebuddyCliConfigured, codebuddyCliGatewayMode, codebuddyCliActive, codebuddyCliBusy, onSwitchCodebuddyCli, codebuddyCliLoading, codebuddyCnIdeAvailable, codebuddyCnIdeActive, codebuddyCnIdeBusy, codebuddyCnIdeLoading, onSwitchCodebuddyCnIde, vscodePluginAvailable, vscodePluginActive, vscodePluginBusy, vscodePluginLoading, onSwitchVscodePlugin, jetbrainsPluginAvailable, jetbrainsPluginActive, jetbrainsPluginBusy, jetbrainsPluginLoading, onSwitchJetbrainsPlugin, onGuidePlugin, onAddPlaintextAccount, onGuideToSetting, selected = false, onToggleSelect, onOpenDetail, todayTokens, todayTokensTruncated = false, featuresDisabled = true }: Props) {
  const t = useT();
  /**
   * 兜底归一（第三道闸）。
   *
   * 后端与 `lib/api.ts` 都已归一，但本组件是**纯展示**的，也可能被 demo 数据、
   * 测试或未来的新调用方直接喂进来。而 `name` 会**直接当 React 子节点渲染** ——
   * 对象会让 React 抛错并卸载整棵树 ⇒ 窗口一片白（issue #2）。
   * 宁可显示「未命名账号」，也不能崩。
   */
  const name = displayText(account.nickname) || displayText(account.uid) || t("wbAccounts.card.unnamed");
  const expired = typeof account.expiresAt === "number" && account.expiresAt < Date.now();
  const identity = accountIdentity(account);
  const avatarClass = avatarTone(name);

/**
 * ★ B1：「使用中 N 个工具」= **程序位全表**里处于激活态的项数。
 *
 * ## 为什么要重写
 *
 * 旧实现 `[workbuddyActive, codebuddyCliActive, codebuddyCnIdeActive]` 写死 3 个，
 * **漏了**下方 `programRow` 已经接入的 `vscodePluginActive` / `jetbrainsPluginActive`
 * ⇒ 一个「只绑了 VSCode 插件」的账号会显示「使用中 0 个工具」，
 * 可它明明**正在被使用**（徽章、竖条、底色全都亮着）。
 *
 * 这不只是漏数，而是**违反本文件自己写的纪律**：紧随其后的 `isCurrentAccount`
 * 数的是**同样 5 个**信号，两个口径对同一个问题给出不同答案 ⇒ 自相矛盾。
 *
 * ## 现在
 *
 * 两个值**从同一组信号派生**（`programSlots`），因此「徽章亮 ⇔ 计数 ≥ 1」
 * 成为**结构上的恒等式**，而不是靠两处手工同步维持。
 * （`programSlots` 同时驱动 `programRow`，见下方。）
 */
const programSlots = [
  { key: "workbuddy", active: workbuddyActive, available: true },
  { key: "codebuddy-cli", active: codebuddyCliActive, available: codebuddyCliConfigured },
  { key: "codebuddy-cn-ide", active: codebuddyCnIdeActive, available: codebuddyCnIdeAvailable },
  { key: "vscode-plugin", active: vscodePluginActive, available: vscodePluginAvailable },
  { key: "jetbrains-plugin", active: jetbrainsPluginActive, available: jetbrainsPluginAvailable },
] as const;

const activeProductCount = programSlots.filter((slot) => slot.active).length;

/**
 * 「当前登录」= 账号绑定到**任一**程序位（原型 `accountCard` :2493-2495：多对多，不是只看第一个）。
 *
 * ⚠️ 与左侧竖条口径一致：竖条 + 徽章是同一信号的**两重表达**，二者必须在同一条件下出现，
 * 否则会出现「有徽章却无竖条」的矛盾态。判定直接读 `programSlots` ⇒ 与上面的
 * `activeProductCount` **同源**，不可能再分叉。
 */
const isCurrentAccount = programSlots.some((slot) => slot.active);

  /**
   * 指标行（原型 `.acct-metrics`，`buddy-switch-prototype.html:341` / `accountCard` :2510-2514）：
   * `积分 <b>当前</b>/<b>总额</b> · 今日 <b>42.1K</b> · 2 分钟前`。
   *
   * ⚠️★ 2026-10-10 **口径修正**：原先这里显示的是 `已用/总额`（由 `resources[].used` 汇总），
   * 而原型显示的是 `当前/总额`（`a.credits/a.creditsTotal`，即「剩余 / 总额」）。
   * 同一个「积分 x/y」在两处含义不同 ⇒ 用户按原型对不上数。现在改为**剩余/总额**。
   *
   * 三项都来自**真实数据源**，逐项按诚实边界渲染：
   * - **积分 剩余/总额**：剩余取 `credit.totalRemaining`；总额由 `resources[].total` 汇总；
   *   未就绪 ⇒ 值渲染「—」；
   * - **今日 Token**：`get_account_token_today` 按账号聚合；账号**缺席** ⇒ 「—」（**绝不当 0**）；
   *   网关日志的**今日**部分可能被裁（`todayTokensTruncated`）⇒ 下界 `≥N`，明细与下界说明放 tooltip；
   *   客户端 / 网关**两侧都非零** ⇒ 明细追加「可能重复计数」说明（两条独立记录，无 join 键可去重）；
   * - **最后使用**：`AccountMeta.lastUsedAt`（epoch 毫秒）；**缺席则整项省略**（不打印「—」）。
   *
   * ⚠️ 整行**始终渲染**（含数据全缺时）—— 不闪不藏，缺值以「—」如实呈现。
   * ⚠️ 行间距由卡片根的 `gap-2.5` 承担，这里不再自带 `mb-*`。
   */
  const metricResources = credit?.ok ? credit.resources ?? [] : [];
  /** 当前（剩余）积分 —— 与原型 `a.credits` 同义。 */
  const metricCreditCurrent =
    credit?.ok && typeof credit.totalRemaining === "number" ? credit.totalRemaining : null;
  const metricCreditTotal =
    credit?.ok && metricResources.length > 0
      ? metricResources.reduce((sum, resource) => sum + (Number.isFinite(resource.total) ? resource.total : 0), 0)
      : null;
  /** 「最后使用」：仅当有真实的 `lastUsedAt`（epoch 毫秒）才渲染该项；否则整项省略。 */
  const lastUsedAt = typeof account.lastUsedAt === "number" && account.lastUsedAt > 0 ? account.lastUsedAt : null;

  /* ★ 指标行 —— 逐项对齐原型 `.acct-metrics`（`demo.css:341-342`：
     `display:flex; gap:14px; font-size:11.5px; color:var(--muted-fg)`，**不换行**、无下边距）：
     `积分 1,240/2,000` · `今日 42.1K` · `2 分钟前`。
     ⚠️ 第三项原型是**相对时间、且没有标签**（`<span class="muted">${a.lastUsed}</span>`），
     此前我们写「最后使用 10-12 17:10」——标签与绝对日期都是本仓自加的，与原型不同。
     绝对时刻改挂 `title`（悬停可见，不占版面）。 */
  const lastUsedRelative = formatRelativeTime(lastUsedAt, t);
  const metricsRow = (
    <div className="flex min-w-0 gap-[14px] text-[11.5px] text-muted-foreground">
      <span>
        {t("wbAccounts.card.metric.credits")}{" "}
        {metricCreditCurrent === null ? (
          <b className="font-semibold tabular-nums text-foreground">—</b>
        ) : (
          <>
            <b className="font-semibold tabular-nums text-foreground">
              {formatCredits(metricCreditCurrent)}
            </b>
            {metricCreditTotal === null ? "" : `/${formatCredits(metricCreditTotal)}`}
          </>
        )}
      </span>
      <span className="inline-flex items-center gap-1">
        {t("wbAccounts.card.metric.todayTokens")}{" "}
        {todayTokens ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <b className="cursor-default font-semibold tabular-nums text-foreground">
                {formatTodayTokens(todayTokens, todayTokensTruncated, t)}
              </b>
            </TooltipTrigger>
            <TooltipContent side="top">
              {todayTokensBreakdown(todayTokens, todayTokensTruncated, t)}
            </TooltipContent>
          </Tooltip>
        ) : (
          <b className="font-semibold tabular-nums text-foreground">—</b>
        )}
      </span>
      {lastUsedRelative !== null && (
        <span title={t("wbAccounts.card.metric.lastUsedTip", { time: formatAbsoluteTime(lastUsedAt) })}>
          {lastUsedRelative}
        </span>
      )}
    </div>
  );

  /* ★ 「当前登录」徽章**单独拆出来**（原型 `buddy-switch-prototype.html:2502`）：
     它在 `.acct-top` 内、与昵称**同一行**（不是掉进下面的 chips 行）。
     这既对齐原型，也顺带把头部从三行压回一行。 */
  const currentLoginBadge = isCurrentAccount ? (
    <Badge variant="info" className={cn(chipClass, "shrink-0")}>
      {t("wbAccounts.card.currentLogin")}
    </Badge>
  ) : null;

  /**
   * 逐批到期明细（两枚徽章的 tooltip 共用一套渲染，**只有取哪一段批次**不同）。
   *
   * ★ issue #79：明细是**一行一批**，用户实测有 41 批 ⇒ 浮层比窗口还高，
   * `side="top"` 把**最关键的最近几批**顶出窗口上沿，且没有滚动条（Radix tooltip
   * 内容 `overflow` 默认可视，超出即被切）。
   *
   * 现在三件事一起做：
   * ① `EXPIRY_TIP_MAX_HEIGHT` 限高（`TooltipContent` 基础样式另有视口兜底）；
   * ② 限高后必定可滚 ⇒ 标题 `sticky` 钉在浮层顶部，滚到第 30 行也知道「共 N 笔」；
   * ③ 标题用负外边距铺满浮层宽度并盖住自己的 `py-2` 上边距 —— 否则滚动时行会从
   *    标题上方那 8px 里穿过去，看起来像标题被切了一半。
   */
  const expiryTipBody = (batches: readonly { expireAt: number; amount: number }[]) => (
    <>
      <div className="sticky -top-2 -mx-3 -mt-2 bg-popover px-3 pb-1 pt-2 font-medium">
        {t("wbAccounts.card.expiringTipTitle", { n: batches.length })}
      </div>
      {batches.map((batch) => (
        <div key={batch.expireAt} className="whitespace-nowrap tabular-nums">
          {t("wbAccounts.card.expiringTipLine", {
            time: formatAbsoluteTime(batch.expireAt),
            amount: formatCredits(batch.amount),
          })}
        </div>
      ))}
    </>
  );

  /**
   * 「N 积分 M 天内过期」——原型 `.acct-chips` 的第三条（`buddy-switch-prototype.html:2507`）：
   * `a.expiring ? <span class="badge warn">${fmt(a.expiring)} 积分 ${a.expireInDays} 天后过期</span> : ""`。
   *
   * ★ 2026-10-17 **口径修正**（issue #44）：此前金额取「3 天内临期之和」、天数取「最早那一刻」，
   * 两者不是同一批积分 ⇒ 100 明天到期 + 200 后天到期会显示「300 积分 1 天后过期」（假话）。
   * 现在统一由 `summarizeExpiring` 汇总：**金额 = 全部临期之和、天数 = 其中最晚那一刻**
   * ⇒「N 积分 M 天内过期」对每一笔都成立；逐批明细（最急的排在首位）放进 tooltip。
   *
   * ⚠️ 窗口不再是前端自设的 3 天，而是**后端的 `expiringSoon`（7 天）**——统计页 / 总览页
   * 读的就是同一口径（`expiringSoonRemaining`），卡片若另设窗口，同一件事会出现两个数字。
   * 拿不到临期批次 ⇒ **整条不渲染**（绝不写一个猜出来的「N 天」标签）。
   */
  const expiring = credit?.ok ? summarizeExpiring(metricResources) : null;
  const expiringChip = expiring ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="warning"
          className={chipClass}
          aria-label={t("wbAccounts.card.expiringChip", {
            amount: formatCredits(expiring.total),
            days: expiring.days,
          })}
        >
          {t("wbAccounts.card.expiringChip", { amount: formatCredits(expiring.total), days: expiring.days })}
        </Badge>
      </TooltipTrigger>
      <TooltipContent side="top" style={{ maxHeight: EXPIRY_TIP_MAX_HEIGHT }}>
        {expiryTipBody(expiring.batches)}
      </TooltipContent>
    </Tooltip>
  ) : null;

  /**
   * 「最近 N 积分 · X 天后过期」—— **中性**徽章（issue #69）。
   *
   * ## 它补的是什么
   *
   * `expiringChip`（黄色「N 积分 M 天内过期」）只覆盖**后端 7 天临期窗口**内的批次。
   * 用户多账号时真正要答的是「**先用哪个**」—— 8 天 / 30 天后到期的那批在窗口外，
   * **完全不可见** ⇒ 用户说「两周内到期的看不到」。
   *
   * ## 口径
   *
   * · `amount` = **最近一批**的剩余积分（同日多包合并）—— 不是总额、不是已用量；
   * · `days`   = 最近一批距今天数（向上取整）；
   * · 无未来到期 / 积分查询失败 / `resources` 为空 ⇒ **整条不渲染**（不写「0 天后」）。
   *
   * ## 为什么不并进黄色徽章
   *
   * 黄色那枚是**警告**（「再不用就浪费」），7 天口径**不变**（issue #69 明确要求）；
   * 中性那枚是**信息**（「什么时候到期」）。合并会让「7 天内」的警告强度被稀释。
   *
   * ⚠️ 不新增块级元素：卡片结构必须保持 header → chips → metrics → actions
   * （`scripts/check-scope-registry.cjs` 有反向门禁）。
   */
  const nearestBatch = credit?.ok ? nearestExpiringBatch(metricResources) : null;
  /**
   * 未来全部批次 —— **算一次**。
   *
   * 此前 title 与明细各调一次 `upcomingExpiries`：同一份数据被过滤+合并+排序两遍，
   * 而且两次调用之间若跨过某个到期时刻，标题里的「共 N 笔」会和下面列出来的行数不一致。
   */
  const upcoming = nearestBatch ? upcomingExpiries(metricResources) : [];
  const nearestChip = nearestBatch && !expiring ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="secondary"
          className={chipClass}
          aria-label={t("wbAccounts.card.nearestExpiringChip", {
            amount: formatCredits(nearestBatch.amount),
            days: daysUntil(nearestBatch.expireAt),
          })}
        >
          {t("wbAccounts.card.nearestExpiringChip", {
            amount: formatCredits(nearestBatch.amount),
            days: daysUntil(nearestBatch.expireAt),
          })}
        </Badge>
      </TooltipTrigger>
      <TooltipContent side="top" style={{ maxHeight: EXPIRY_TIP_MAX_HEIGHT }}>
        {expiryTipBody(upcoming)}
      </TooltipContent>
    </Tooltip>
  ) : null;

  /**
   * 状态标签 —— 与原型 `accountStatus()`（`buddy-switch-prototype.html:2390-2395`）**同一口径**：
   *
   * ```
   * 凭据已过期(bad) → 需重新登录(bad) → 今日未签到(warn) → null（渲染「已签到」ok）
   * ```
   *
   * ⚠️ 原型是**互斥的一枚**。此前本仓把「已签到」与「需重新登录」当两枚并列渲染
   * ⇒ 同一张卡会同时挂着「已签到」和「需重新登录」两个互相矛盾的标签。
   */
  const statusChip = expired ? (
    <Badge variant="destructive" className={chipClass}>{t("wbAccounts.card.tokenExpired")}</Badge>
  ) : account.needsRelogin ? (
    <Badge variant="destructive" className={chipClass}>{t("wbAccounts.card.needRelogin")}</Badge>
  ) : todayCheckedIn === false ? (
    <Badge variant="warning" className={chipClass}>{t("wbAccounts.card.notCheckedIn")}</Badge>
  ) : todayCheckedIn === true ? (
    <Badge variant="success" className={chipClass}><CircleCheck /> {t("wbAccounts.card.checkedIn")}</Badge>
  ) : null;

  const statusChips = (
    <>
      {/* 顺序照原型 `.acct-chips`（`buddy-switch-prototype.html:2505-2508`）：
          `使用中 N 个工具` → `状态` → `X 积分 N 天后过期` → `优先`。 */}
      {activeProductCount >= 2 && (
        <Badge variant="info" className={chipClass}>
          <CircleCheck /> {t("wbAccounts.card.toolsInUse", { n: activeProductCount })}
        </Badge>
      )}
      {statusChip}
      {travelChip(travelStatus)}
      {expiringChip}
      {nearestChip}
      {creditPriority && (
        <Badge variant="info" className={chipClass} title={t("wbAccounts.card.priorityTip")}>
          {t("wbAccounts.card.priority")}
        </Badge>
      )}
    </>
  );

  /* 程序位行（**单一定义**，页脚程序位行 —— 见文件头）。
     ⚠️ 尺寸与形状对齐原型 `.prog`（`buddy-switch-prototype.html:345-357`）：
     高 26px、内边距 0 8px、**圆角 8px（不是胶囊）**、字号 11px、图标 12px。
     ⚠️ 圆角必须写**字面 8px**：本主题里 `rounded-lg` = `--radius` = 12px、`rounded-xl` = 16px，
     都不是原型的 8px —— 26px 的方块配 12px 圆角会「近似圆形」，与截图明显不同。 */
  const btnSize = "sm" as const;
  /* ⚠️ `gap-1`（4px）是原型 `.prog` 的 `gap:4px` —— 此前漏写，按钮比原型宽 2px。 */
  /* ★ 严格对齐原型 `.prog`（`demo.css:326-332`）：**透明底** + `--border` 边框 +
     **字重 400** + `muted-fg` 字色。`Button variant="outline"` 自带 `bg-background` /
     `border-input`，基础样式里还有 `font-medium` ⇒ 必须逐项压回去。
     ⚠️ `font-normal` 与 `font-medium` **同特异性**，只能靠 `!` 稳定覆盖
     （仓库里 `sonner.tsx` 也是这么压的）。 */
  const btnClass =
    "h-[26px] gap-1 rounded-[8px] border-border bg-transparent px-2 text-[11px] !font-normal text-muted-foreground";

  /**
   * 程序位按钮的**内容** —— 严格照原型 `.prog`（`buddy-switch-prototype.html:2530`）：
   * `${on ? ICON.check : ""}${pg.short}` —— **短名是纯文字，没有产品图标**。
   */
  const progLabel = (text: string) => <span>{text}</span>;

  // WorkBuddy 主程序位：当前账号 → 角标；演示模式 → DemoAction；其余 → 切换按钮。
  /**
 * ★ B5：卡片上的程序位控件 = **注册表 5 项 ∪ {WorkBuddy 主位}**。
 *
 * ## 为什么不把主位塞进 `WORKBUDDY_PROGRAMS`
 *
 * `registry.ts::WORKBUDDY_PROGRAMS` 与后端 `programs.rs::PROGRAMS` 是**同一份 5 项**
 * （`codebuddy_cli` / `codebuddy_cn_ide` / `vscode_cn_inject` /
 * `vscode_codebuddy_plugin` / `jetbrains_codebuddy_plugin`），**都不含主位**。
 * 而卡片上除了这 5 个，**还多一个 WorkBuddy 主位按钮**（`workbuddyControl`）。
 *
 * ⇒ 两侧各差一个 id，但**这个差是刻意的、且合理的**：
 * WorkBuddy 主位是「**把账号写进客户端自己的认证文件**」，与上面 5 个
 * 「切换某个外部程序位」**不是一类操作**；把它塞进 `PROGRAMS` 会让后端
 * `PROGRAMS` 长出一个**没有 adapter** 的空槽，而 `PROGRAMS` 的自动派生
 * （`status`/`switch`/`detect`/`install_helper` 全覆盖）会因此产生**假能力**。
 *
 * 另一条路是给主位补一个真 adapter（工作量在 Rust，且要过
 * `product_spec_matches_frontend_registry_slug_and_capabilities` 跨语言护栏）——
 * 收益只是让两个列表长度一致，**不值**。
 *
 * ⇒ 定为**显式例外**，并由 `check-scope-registry.cjs` 的 B5 断言把
 * 「卡片 = 表 ∪ {主位}」这个公式**钉住**，避免后人误把它当 bug「修」掉。
 */
const workbuddyControl = workbuddyActive ? (
    <ProductCurrentState product="workbuddy" short="WB" />
  ) : demoModeEnabled ? (
    <DemoAction>
      <Button variant="outline" size={btnSize} className={btnClass} aria-label={t("wbAccounts.card.setCurrentWorkbuddy")}>
        {progLabel("WB")}
      </Button>
    </DemoAction>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={btnClass} disabled={featuresDisabled || !onSwitch} onClick={() => onSwitch?.(account)} aria-label={t("wbAccounts.card.setCurrentWorkbuddy")}>
          {progLabel("WB")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.setCurrentWorkbuddyRestart")}</TooltipContent>
    </Tooltip>
  );

  // CodeBuddy IDE：未检测到 → **引导设置**（不摆一枚点不动的灰按钮）。
  const ideControl = codebuddyCnIdeActive ? (
    <ProductCurrentState product="codebuddy-cn" short="IDE" />
  ) : codebuddyCnIdeAvailable ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={btnClass} disabled={featuresDisabled || !onSwitchCodebuddyCnIde || codebuddyCnIdeBusy} onClick={() => onSwitchCodebuddyCnIde?.(account)} aria-label={codebuddyCnIdeLoading ? t("wbAccounts.card.switchingIde") : t("wbAccounts.card.switchIde")} aria-busy={codebuddyCnIdeLoading}>
          {codebuddyCnIdeLoading ? <Loader2 className="size-4 animate-spin" /> : null}
          {progLabel(codebuddyCnIdeLoading ? t("wbAccounts.card.switching") : "IDE")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.switchIdeRestart")}</TooltipContent>
    </Tooltip>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={cn(btnClass, OFF_PROGRAM_CLASS)} disabled={featuresDisabled || !onGuideToSetting} onClick={() => onGuideToSetting?.("ide")} aria-label={t("wbAccounts.card.guideIdeSetting")}>
          {progLabel("IDE")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.guideIdeSetting")}</TooltipContent>
    </Tooltip>
  );

  // CodeBuddy CLI：未接入 → **引导设置**（同上）。
  /* ★ issue #109：CLI 已走本机网关 ⇒ 选号由网关按模型决定，**切号无效**。
     后端会拒绝（`ensure_not_gateway_mode`），但让用户点了才看到报错是下策 —— 界面先说清。
     ⚠️ 用 `<span>` 包住 disabled 按钮：`disabled` 元素不派发 pointer 事件，
     Radix Tooltip 收不到 hover，提示就永远弹不出来（那等于「灰按钮 + 无解释」）。 */
  const cliControl = codebuddyCliGatewayMode ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={cn("inline-flex", btnClass)}>
          <Button
            variant="outline"
            size={btnSize}
            className="opacity-50"
            disabled
            aria-label={t("wbAccounts.card.cliGatewayMode")}
          >
            {progLabel("CLI")}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.cliGatewayMode")}</TooltipContent>
    </Tooltip>
  ) : codebuddyCliActive ? (
    <ProductCurrentState product="codebuddy" short="CLI" />
  ) : codebuddyCliConfigured ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={btnClass} disabled={featuresDisabled || !onSwitchCodebuddyCli || codebuddyCliBusy} onClick={() => onSwitchCodebuddyCli?.(account)} aria-label={codebuddyCliLoading ? t("wbAccounts.card.switchingCli") : t("wbAccounts.card.setCurrentCli")} aria-busy={codebuddyCliLoading}>
          {codebuddyCliLoading ? <Loader2 className="size-4 animate-spin" /> : null}
          {progLabel(codebuddyCliLoading ? t("wbAccounts.card.switching") : "CLI")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.setCurrentCli")}</TooltipContent>
    </Tooltip>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={cn(btnClass, OFF_PROGRAM_CLASS)} disabled={featuresDisabled || !onGuideToSetting} onClick={() => onGuideToSetting?.("cli")} aria-label={t("wbAccounts.card.guideCliSetting")}>
          {progLabel("CLI")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.guideCliSetting")}</TooltipContent>
    </Tooltip>
  );

  /**
   * 编辑器内 CodeBuddy 插件的一个程序位（VS Code / JetBrains）—— 与 `ideControl` **同构**：
   * 当前账号 ⇒ 角标；已装 ⇒ 可切换；未检测到 ⇒ **可点引导**（不是「点不动的灰按钮」，§3.3）。
   *
   * ⚠️ 未检测到的文案**只说「本机没装」**，绝不写「尚未支持」—— 后者是 `planned` 的口径
   * （本应用还没做，去装客户端也没用），两者混用会把用户引向错误动作。
   */
  function pluginControl(opts: {
    /** 程序位 id（供 `onGuidePlugin` 与门禁锚点使用）。 */
    id: string;
    /** 短标签（与 IDE / CLI 的写法一致）。 */
    short: string;
    /** 程序位显示名（词条）—— 与注册表 `labelKey` 同一份文案。 */
    label: string;
    available?: boolean;
    active?: boolean;
    busy?: boolean;
    loading?: boolean;
    onSwitch?: (a: AccountMeta) => void;
  }) {
    const { id, short, label, available, active, busy, loading, onSwitch } = opts;
    if (active) {
      return (
        <span
          role="status"
          aria-label={t("wbAccounts.card.currentPlugin", { client: label })}
          title={t("wbAccounts.card.currentPlugin", { client: label })}
          className={cn(
            /* 与 `ProductCurrentState` 同一口径（同底色/描边，去掉了硬编码内阴影）。 */
            "inline-flex items-center justify-center border border-primary/40 bg-primary/18 text-primary font-semibold",
            /* 与 `ProductCurrentState` 同一口径：26px / 圆角 8px。 */
            "h-[26px] gap-1 rounded-[8px] px-2 text-[11px]",
          )}
        >
          <Check className="size-3" strokeWidth={2.25} />
          <span>{short}</span>
        </span>
      );
    }
    if (available) {
      return (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size={btnSize}
              className={btnClass}
              disabled={featuresDisabled || !onSwitch || busy}
              onClick={() => onSwitch?.(account)}
              aria-label={loading ? t("wbAccounts.card.switching") : label}
              aria-busy={loading}
              data-program-switch={id}
            >
              {loading ? (
                <Loader2 className="size-4 animate-spin" />
              ) : null}
              {progLabel(loading ? t("wbAccounts.card.switching") : short)}
            </Button>
          </TooltipTrigger>
          <TooltipContent side="top">{label}</TooltipContent>
        </Tooltip>
      );
    }
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="outline"
            size={btnSize}
            className={cn(btnClass, OFF_PROGRAM_CLASS)}
            disabled={featuresDisabled || !onGuidePlugin}
            onClick={() => onGuidePlugin?.(id)}
            aria-label={t("wbAccounts.card.guidePlugin", { client: label })}
            data-program-missing={id}
          >
            {progLabel(short)}
          </Button>
        </TooltipTrigger>
        <TooltipContent side="top">{t("wbAccounts.card.guidePlugin", { client: label })}</TooltipContent>
      </Tooltip>
    );
  }

  const vscodePluginControl = pluginControl({
    id: "vscode_codebuddy_plugin",
    short: "VSC",
    label: t("wbAccounts.program.vscodeCodebuddyPlugin"),
    available: vscodePluginAvailable,
    active: vscodePluginActive,
    busy: vscodePluginBusy,
    loading: vscodePluginLoading,
    onSwitch: onSwitchVscodePlugin,
  });

  const jetbrainsPluginControl = pluginControl({
    id: "jetbrains_codebuddy_plugin",
    short: "JB",
    label: t("wbAccounts.program.jetbrainsCodebuddyPlugin"),
    available: jetbrainsPluginAvailable,
    active: jetbrainsPluginActive,
    busy: jetbrainsPluginBusy,
    loading: jetbrainsPluginLoading,
    onSwitch: onSwitchJetbrainsPlugin,
  });

  /* ★ 计划新增（**尚未支持**）的程序位 —— **必须渲染**（原型断言 `verify-behaviors.cjs:1297-1299`：
     「注册表声明了 planned 的程序位，卡片上却没有对应样式」⇒ 红）。
     ⚠️ 说明必须是「尚未支持」，**不许**用「未检测到客户端」——
     后者会**引导用户去装，而装了也没用**（`:1302` 反向禁止这条措辞）。 */
  const plannedControls = PLANNED_PROGRAMS.map((p) => (
    <Tooltip key={p.id}>
      <TooltipTrigger asChild>
        <Button
          variant="outline"
          size={btnSize}
          className={cn(btnClass, PLANNED_PROGRAM_CLASS)}
          disabled
          data-program-planned={p.id}
          aria-label={`${t(p.labelKey)}（${t("wbAccounts.program.plannedHint")}）`}
        >
          <Ban className="size-4" />
          {progLabel(t(p.labelKey))}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.program.plannedHint")}</TooltipContent>
    </Tooltip>
  ));

  /* ⚠️ `flex-wrap` + `w-full` 是**必须**的：程序位已到 5 个（WB / IDE / CLI / VS Code / JetBrains），
       「当前账号」档是带图标+对勾的胶囊 —— 不换行就会溢出卡片、最后一个槽位被裁掉
       （实测：4 个胶囊同时点亮时 JB 被挤出可视区）；而只加 `min-w-0` 会让本行**收缩到
       最小内容宽（一个胶囊）**，5 个槽位被压成 5 行（实测卡片高度 208→293px）。
       ⇒ 必须先**占满整行**（`w-full`）再在行内换行。 */
  const programRow = (
    <div className="flex w-full flex-wrap items-center gap-1">
      {workbuddyControl}
      {ideControl}
      {cliControl}
      {/* 编辑器内 CodeBuddy 插件（VS Code / JetBrains）：与 IDE 同构的三个档位。 */}
      {vscodePluginControl}
      {jetbrainsPluginControl}
      {plannedControls}
    </div>
  );

  const checkbox = (
    <Checkbox
      checked={selected}
      onCheckedChange={() => onToggleSelect?.(account)}
      disabled={!onToggleSelect}
      aria-label={t("wbAccounts.card.selectAria", { name })}
      className="size-[17px]"
    />
  );

  /**
   * 「更多」菜单。
   *
   * ⚠️★ 2026-10-10：**从头部移到页脚**（原型 `.acct-actions` 的顺序是
   * `.progs` → 更多按钮，`buddy-switch-prototype.html:2515-2534`）。
   * 原先它绝对定位在头部右上角 —— 那是本仓自己的做法，原型里没有。
   */
  /**
   * 「更多」菜单 —— 与**表格视图共用同一份**（`account-more-menu.tsx`）。
   *
   * ⚠️ 这里不再内联菜单结构：同一个账号在卡片和表格里是同一批动作，
   * 两处各写一遍必然漂移（上一版就出现过「卡片有刷新、表格没有」）。
   */
  const menu = demoModeEnabled ? (
    <AccountMoreMenuDemo />
  ) : (
    <AccountMoreMenu
      accountName={name}
      onRefreshPart={onRefreshPart ? (kind) => onRefreshPart(account, kind) : undefined}
      refreshingKinds={refreshingKinds}
      doneKinds={doneKinds}
      refreshKinds={refreshKinds}
      onRefresh={onRefresh ? () => onRefresh(account) : undefined}
      todayCheckedIn={todayCheckedIn}
      expired={expired}
      needsRelogin={account.needsRelogin}
      onCheckin={onCheckin ? () => onCheckin(account) : undefined}
      onExport={onExport ? () => onExport(account) : undefined}
      onDelete={() => onDelete(account)}
      featuresDisabled={featuresDisabled}
    />
  );

  return (
    <TooltipProvider>
      <article
        /* ★ P1-5 键盘可达：**卡片本身可 Tab 聚焦**（此前只有内部勾选框可聚焦）。
           ⚠️ 可聚焦元素**必须有用途**，否则是无障碍反模式（WCAG 2.4.3「可聚焦但无操作」）⇒
           Enter / Space 打开详情，与菜单里的「查看详情」同义（无 onOpenDetail 时不拦按键）。 */
        tabIndex={0}
        role="article"
        onClick={(e) => {
          if (!onOpenDetail) return;
          /* ★ 判据在 `lib/account-open-guard.ts`（三张卡 + 表格行共用）。
             ⚠️ 此处**不能**手写 `closest("button, …")`：菜单经 Portal 挂到 body，
             DOM 已不在卡片内，而菜单项是 `div[role="menuitem"]` 不是 button ——
             旧判据一条也不命中 ⇒ 点任何菜单项都会顺带打开详情抽屉（2026-10-22 报障根因）。 */
          if (!shouldOpenAccountDetail(e)) return;
          onOpenDetail(account);
        }}
        onKeyDown={(e) => {
          if (!onOpenDetail || e.target !== e.currentTarget) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onOpenDetail(account);
          }
        }}
        /* 与原型 `.acct`（`buddy-switch-prototype.html:312-318`）对齐：
           `border-radius: var(--radius)` = 12px（⇒ `rounded-lg`，不是 `rounded-2xl`）；
           静止 `var(--shadow-sm)` / 悬停 `var(--shadow-md)` + `border-color: var(--border-strong)`。
           ⚠️ 原先这里硬编码了一串 `rgba(15,23,42,…)` 阴影 —— 深色下那种「冷灰」阴影在深底上
           几乎不可见，且与卡片体系里其它阴影不一致。 */
        /* ★ 原型 `.acct`（`buddy-switch-prototype.html:312-317`）：
           `display:flex; flex-direction:column; gap:10px; padding:13px 14px`
           ⇒ 卡片是「**四个块 + 统一 10px 间距**」，内边距在**根**上，各块不再自带 padding。
           ⚠️ 此前内边距散在 header / section / footer 三处，块间距靠各自的 `mt-*`，
           结果块间距在 6~16px 之间乱跳，且头部那条底色带把 `.acct-top` 变成了一条「页眉」。 */
        className={cn(
          "relative flex min-w-0 flex-col gap-2.5 overflow-hidden rounded-lg border border-border bg-card px-3.5 py-[13px] shadow-card transition-[border-color,box-shadow] hover:border-border-strong hover:shadow-card-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
          selected && "border-ring bg-primary/[0.07]",
        )}
      >
      {/* ★ 原型 `.acct.is-current::before`（`buddy-switch-prototype.html:321-324`）：
          「当前账号」用**左侧 3px 竖条**表达 —— 与头部的「当前登录」徽章一起构成两重信号。
          ⚠️ 竖条只在**左侧两条圆角**上贴合卡片（`rounded-l-lg`），否则四角会露出直角方块。
          ⚠️ 条件必须与徽章**同为** `isCurrentAccount`（绑定任一程序位）：两者是同一信号的两重表达，
          否则会出现「有徽章却无竖条」/「有竖条却无徽章」的矛盾态。 */}
      {isCurrentAccount ? (
        <span
          aria-hidden="true"
          data-current-bar="true"
          className="pointer-events-none absolute inset-y-0 left-0 w-[3px] rounded-l-lg bg-primary"
        />
      ) : null}
      {/* ★ 原型 `.acct-top`（`buddy-switch-prototype.html:326`）：
         `display:flex; align-items:center; gap:10px` —— **就是一行**，且**没有底色**。
         ⚠️ 2026-10-10：删掉了原先那条头部底色带（`bg-primary/5` / `bg-muted/30`）与幽灵水印。
         原型 `.acct-top` 是**透明的**，整张卡一个底色；那条底色带把它变成了一条「页眉」，
         还让卡片看起来被切成了两半。 */}
      <header className="flex items-center gap-2.5">
        {/* ⚠️ `...` 菜单**不在这里** —— 原型把它放在 `.acct-actions` 里（程序位行右侧），
            见本文件末尾的页脚 `{menu}`。 */}
        {/* ★ 原型 `.acct-top` 的**四件套，顺序与嵌套完全一致**（`buddy-switch-prototype.html:2499-2503`）：
           `[勾选] [头像32] [.acct-id：昵称 + 身份] [当前登录徽章]`。
           ⚠️★ 2026-10-10 两处修正：
             ① 徽章**移出 `.acct-id`**、回到 `.acct-top` 的**直接子级** —— 原型的徽章是
                行尾、**垂直居中**的独立件，不是昵称那一行里的小尾巴（截图对比一眼可见）；
             ② 状态标签**移出头部** —— 原型里 `.acct-chips` 是 `.acct-top` 的**兄弟块**
                （卡片是 4 个块），不是塞在头像右侧的第三行。 */}
        <div className="relative z-10 flex w-full min-w-0 items-center gap-2.5">
          {checkbox}
          {/* 原型 `.avatar`（`demo.css:308-311`）：32px / 圆角 10px / `12.5px` / `640` /
              **字色近白**（`oklch(0.99 0 0)`）。
              ⚠️ 首字取**前 2 位**（原型 `initials()`，`buddy-switch-prototype.html:1929`）——
              单字在「jacky / spare-global」这类名字下辨识度明显不足。*/}
          <div className={cn("flex size-8 shrink-0 items-center justify-center rounded-[10px] text-[12.5px] font-[640]", AVATAR_TEXT_CLASS, avatarClass)}>
            {accountInitials(name)}
          </div>
          {/* `.acct-id`：`min-width:0; flex:1`，昵称 13px/600、身份 11px muted（均单行省略）。 */}
          <div className="min-w-0 flex-1">
            <h3 className="truncate text-[13px] font-semibold" title={name}>{name}</h3>
            <p className="truncate text-[11px] text-muted-foreground" title={identity}>{identity}</p>
          </div>
          {currentLoginBadge}
        </div>
      </header>

      {/* ★ 原型 `.acct-chips`（`buddy-switch-prototype.html:341`）：`flex-wrap; gap:5px`，
          是 `.acct-top` 的**兄弟块**（不是头部里的一行）。 */}
      <div className="flex min-w-0 flex-wrap items-center gap-[5px]">{statusChips}</div>

      {/* ⚠️ 内边距与块间距都已在**根**上（`px-3.5 py-[13px] gap-2.5`，照原型 `.acct`），
          这里不再自带 padding —— 否则块间距会变成「根 gap + 自己的 mt」两份叠加。 */}
      <section className="flex min-w-0 flex-1 flex-col">
        {/* ★ 指标行（原型 `.acct-metrics` :2510）：徽章之下、积分明细之上的一行小字。
            始终渲染 —— 积分未就绪时显示「—」，不闪不藏。 */}
        {metricsRow}
        {creditLoading ? (
          <div className="flex items-center gap-2 py-3 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />{t("wbAccounts.card.loadingCredits")}</div>
        ) : !credit ? (
          <div className="py-3 text-sm text-muted-foreground">{t("wbAccounts.card.waitingCredits")}</div>
        ) : !credit.ok ? (
          /*
           * ★ 凭据是加密信封（客户端 5.6 起）时**不能**走下面那条 `truncate` 单行：
           * 那句错误文案本身就是「该怎么办」的唯一指引（重新登录 / OAuth 扫码添加），
           * 截成 `…` 等于把出路一起截掉 —— 2026-09-24 用户截图里就是这种形态。
           *
           * 判据用结果自带的 `reason`（后端契约常量），**不解析文案**。
           *
           * ⚠️ 这条错误态 CTA 是 §3.6 权限/错误态的**行内出口**，**不是**常规可点区 ——
           * 它随错误态一起保留，不计入「≤4」的口径（PLAN §3.3 的保留例外）。
           */
          credit.reason === ENCRYPTED_CREDENTIAL_REASON ? (
            <div className="min-w-0 py-3 text-sm text-destructive">
              <div className="flex items-start gap-2">
                <Coins className="mt-0.5 size-4 shrink-0" />
                <span className="min-w-0 flex-1">{credit.error || t("wbAccounts.card.creditFailed")}</span>
              </div>
              {onAddPlaintextAccount && (
                <div className="mt-2.5 pl-6">
                  <Button size="sm" variant="outline" onClick={onAddPlaintextAccount}>
                    <QrCode />
                    {t("wbAccounts.card.addPlaintextAccount")}
                  </Button>
                </div>
              )}
            </div>
          ) : (
            <div className="flex min-w-0 items-center gap-2 py-3 text-sm text-destructive" title={credit.error}>
              <Coins className="size-4 shrink-0" />
              <span className="min-w-0 truncate">{credit.error || t("wbAccounts.card.creditFailed")}</span>
            </div>
          )
        ) : null}
      </section>

      {/* ★ 原型 `.acct-actions`（`buddy-switch-prototype.html:343` / `accountCard` :2515-2534）：
         `display:flex; align-items:center; gap:6px; padding-top:10px; border-top:1px solid var(--border)`
         ⇒ 页脚 = **程序位行（`flex:1`）+ 「更多」按钮**（菜单在**这里**，不是头部）。
         ⚠️ 不再自带左右内边距：原型 `.acct` 的 `padding:13px 14px` 已在卡片根上，
         页脚的 `border-top` 因此与内容同宽（原型也如此）。 */}
      <footer className="flex items-center gap-1.5 border-t border-border pt-2.5">
        {programRow}
        {menu}
      </footer>
      </article>
    </TooltipProvider>
  );
}
