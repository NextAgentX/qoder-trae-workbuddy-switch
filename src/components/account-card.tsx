import { Ban, Check, CircleCheck, Clock3, Coins, Ellipsis, Info, Loader2, PlaneTakeoff, QrCode, RefreshCw, Sparkles, Star, StickyNote, Tag, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
// P0 · 程序位表（含「计划新增 / 尚未支持」态）—— 展示侧镜像，与后端 `programs.rs` 逐字对齐。
import { PLANNED_PROGRAMS } from "@/lib/platforms/registry";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DemoAction } from "@/components/demo-action";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { CodeBuddyCnIdeMark, CodeBuddyMark, WorkBuddyMark } from "@/components/product-marks";
import { cn } from "@/lib/utils";
import { ENCRYPTED_CREDENTIAL_REASON } from "@/lib/api";
import { compactTokens, exactTokens } from "@/components/usage/format";
import { demoModeEnabled } from "@/lib/demo-mode";
import { displayText } from "@/lib/display-text";
import { formatDate } from "@/lib/format";
import { useT, type Translate } from "@/lib/i18n";
import type { AccountMeta, AccountTokenTodayEntry, CreditExpiry, CreditResource, TravelStatus } from "@/lib/types";

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
 * 紧凑模式与宽松模式各需要一种排布（图标化 / 带文字），但它们**是同一行**。
 * 改造前紧凑头部与宽松页脚各写一遍、共 6 枚 `<Button>`，静态计数看起来像 6 个可点区
 * —— 于是这里收敛成**单个 `programRow` 变量**，两个布局引用同一个定义，
 * 可点区口径不再被重复计数污染（PLAN §3.3）。
 */

/**
 * 头像底色 / 首字色板（**6 选 1**，按账号名哈希稳定选取）。
 *
 * ## ★ 为什么全部用「固定 100 / 800 色板」，不用语义色 + `/15`
 *
 * 此前其中两条写作 `bg-success/15 text-success`、`bg-warning/15 text-warning`，
 * 依赖 `--success` / `--warning` 跟明暗主题走。**实测**（oklch → sRGB → WCAG）：
 * 亮色主题下 `text-success` 压在 `bg-success/15` 上只有 **3.11:1**，
 * `text-warning` 是 **3.17:1**，**都低于 AA 的 4.5:1** ⇒ 头像首字（12.5px/640）在
 * 亮色主题下读不清。
 *
 * 之所以不能照原型断言的 `L ≤ 0.5` 判罪：原型的判据是**自己写的 `oklch()` 字面量**，
 * 而真实实现用的是 Tailwind 语义 token —— **oklch 的 L 与 WCAG 亮度不是同一个量**，
 * 断言体不可直接迁移。只能换算后实测。
 *
 * 改用固定色板后：`emerald` 6.03:1、`amber` 6.88:1，且与其余 4 条**完全同构**
 * （都是固定 100 底 + 800 字），亮/暗主题表现一致，不再有「两条跟随主题、四条不跟随」
 * 的分裂。
 */
const AVATAR_TONES = [
  "bg-emerald-100 text-emerald-800",
  "bg-violet-100 text-violet-800",
  "bg-sky-100 text-sky-800",
  "bg-amber-100 text-amber-800",
  "bg-rose-100 text-rose-800",
  "bg-teal-100 text-teal-800",
] as const;

/**
 * 按账号名稳定地取一个头像底色。
 *
 * 与 `trae-account-card.tsx` 共用：两个产品分区的账号卡片必须是同一套视觉，
 * 各自维护一份色板迟早会漂移，因此这里导出而不是复制。
 */
export function avatarTone(name: string) {
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return AVATAR_TONES[hash % AVATAR_TONES.length];
}

export function formatCredits(value: number): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
}

/**
 * 「今日 Token」的**诚实**展示串（卡片与表格**同一份**实现，避免两处口径漂移）。
 *
 * 三条边界（见 `AccountTokenTodayResponse`）：
 * - `entry` 缺失 = 该账号今日**无记录 / 未知** ⇒ **「—」**（**绝不**当 0）；
 * - 出现在 `entry` 里的 `0` 是**真实 0**（`compactTokens(0)` = `"0"`）；
 * - `truncated`（网关日志已到保留上限）⇒ 网关部分是**下界** ⇒ 整串渲染成 **`≥N`**。
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
  if (truncated) parts.push(t("wbAccounts.card.metric.todayTokensTruncated"));
  return parts.join(t("shared.punct.comma"));
}

export function formatCreditExpiry(ts: number | null, t: Translate): string {
  if (!ts) return t("wbAccounts.card.forever");
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return t("wbAccounts.card.forever");
  return `${String(date.getMonth() + 1).padStart(2, "0")}/${String(date.getDate()).padStart(2, "0")}`;
}

function formatCreditUpdatedAt(ts: number | undefined): string {
  if (!ts) return "—";
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "—";
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function expiryClass(expired: boolean, expiringSoon: boolean): string {
  if (expired) return "text-destructive";
  if (expiringSoon) return "text-warning";
  return "text-muted-foreground";
}

function creditResources(credit?: CreditExpiry): CreditResource[] {
  return (credit?.resources ?? [])
    .filter((resource) => resource.remaining > 0)
    .map((resource, index) => ({ resource, index }))
    .sort((left, right) => {
      const leftExpiry = left.resource.expireAt ?? Number.POSITIVE_INFINITY;
      const rightExpiry = right.resource.expireAt ?? Number.POSITIVE_INFINITY;
      return leftExpiry === rightExpiry ? left.index - right.index : leftExpiry - rightExpiry;
    })
    .map(({ resource }) => resource);
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

const chipClass = "rounded-md px-1.5 py-0 text-[11px] font-medium";

function travelIconChip({
  label,
  tooltip,
  variant,
}: {
  label: string;
  tooltip: string;
  variant: "secondary" | "success";
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant={variant} className={cn(chipClass, "px-1")} aria-label={label}>
          <PlaneTakeoff className="size-3.5" />
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
      return travelIconChip({ label: travelTooltip(status, t), tooltip: travelTooltip(status, t), variant: "secondary" });
    case "finished":
      return travelIconChip({ label: travelTooltip(status, t), tooltip: travelTooltip(status, t), variant: "success" });
    case "untraveled":
    default:
      return <Badge variant="secondary" className={cn(chipClass, "text-muted-foreground")}>{t("wbAccounts.card.notTraveled")}</Badge>;
  }
}

/** 未检测到的程序位引导目标（对应设置页里的子项）。 */
export type GuideTarget = "ide" | "cli";

interface Props {
  account: AccountMeta;
  onDelete: (a: AccountMeta) => void;
  onCheckin?: (a: AccountMeta) => void;
  onRefresh?: (a: AccountMeta) => void;
  onSwitch?: (a: AccountMeta) => void;
  todayCheckedIn?: boolean;
  /** 今日旅行状态（undefined=查询中/未知，不渲染标签） */
  travelStatus?: TravelStatus;
  credit?: CreditExpiry;
  creditLoading?: boolean;
  /** 该账号积分最近一次查询完成时间（时间戳） */
  creditUpdatedAt?: number;
  creditPriority?: boolean;
  workbuddyActive?: boolean;
  codebuddyCliConfigured?: boolean;
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
  onEditRemark?: (a: AccountMeta) => void;
  /** 「更多 → 设置分组」：打开抽屉并进入分组编辑态（与备注同一交互）。 */
  onEditGroup?: (a: AccountMeta) => void;
  /**
   * 该账号「今日 Token」明细（`get_account_token_today`）。
   *
   * ⚠️ **缺席 = 今日无记录 / 未知** ⇒ 渲染「—」（**绝不当 0**）。
   */
  todayTokens?: AccountTokenTodayEntry | null;
  /** 网关日志已到保留上限 ⇒ 网关部分是下界 ⇒ 「今日 Token」渲染 `≥N`。 */
  todayTokensTruncated?: boolean;
  featuresDisabled?: boolean;
  /** 紧凑模式：头部缩成一条、按钮图标化、无 footer */
  compact?: boolean;
}

function ProductCurrentState({ product, compact = false }: { product: "workbuddy" | "codebuddy" | "codebuddy-cn"; compact?: boolean }) {
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
        "inline-flex items-center justify-center border border-primary/40 bg-primary/15 text-primary",
        /* ⚠️ 与程序位**同一 footprint**（原型 `.prog` 26px / 圆角 8px）：
           程序位已到 5 个，紧凑头部那一行只有约 156px —— 当前态若仍是带对勾的宽胶囊
           （每个约 58px），5 个槽位必然折成多行（实测 4 行、卡片高度 208→293px）。
           ⇒ 紧凑模式只留图标，`aria-label` / tooltip 仍写明「… 当前账号」（信号不丢）。 */
        compact ? "size-[26px] rounded-[8px]" : "h-[26px] gap-1.5 rounded-[8px] px-2 text-[11px]",
      )}
    >
      {product === "workbuddy" ? (
        <WorkBuddyMark size={compact ? 14 : 12} />
      ) : product === "codebuddy-cn" ? (
        <CodeBuddyCnIdeMark size={compact ? 14 : 12} />
      ) : (
        <CodeBuddyMark size={compact ? 14 : 12} />
      )}
      {compact ? null : <Check className="size-3" strokeWidth={2.25} />}
    </span>
  );
}

export function AccountCard({ account, onDelete, onCheckin, onRefresh, onSwitch, todayCheckedIn, travelStatus, credit, creditLoading, creditUpdatedAt, creditPriority, workbuddyActive, codebuddyCliConfigured, codebuddyCliActive, codebuddyCliBusy, onSwitchCodebuddyCli, codebuddyCliLoading, codebuddyCnIdeAvailable, codebuddyCnIdeActive, codebuddyCnIdeBusy, codebuddyCnIdeLoading, onSwitchCodebuddyCnIde, vscodePluginAvailable, vscodePluginActive, vscodePluginBusy, vscodePluginLoading, onSwitchVscodePlugin, jetbrainsPluginAvailable, jetbrainsPluginActive, jetbrainsPluginBusy, jetbrainsPluginLoading, onSwitchJetbrainsPlugin, onGuidePlugin, onAddPlaintextAccount, onGuideToSetting, selected = false, onToggleSelect, onOpenDetail, onEditRemark, onEditGroup, todayTokens, todayTokensTruncated = false, featuresDisabled = true, compact = false }: Props) {
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
  const resources = creditResources(credit);
  const visibleResources = resources.slice(0, 2);
  const expiringAmount = credit?.ok ? credit.expiringSoonRemaining ?? 0 : 0;

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
   * 指标行（原型 `.acct-metrics`，`buddy-switch-prototype.html:341`：`积分 used/total · 今日 42.1K · 2 分钟前`）。
   *
   * 三项都来自**真实数据源**，逐项按诚实边界渲染：
   * - **积分 已用/总量**：由 `credit.resources[].used/total` 汇总；未就绪 ⇒ 值渲染「—」；
   * - **今日 Token**：`get_account_token_today` 按账号聚合；账号**缺席** ⇒ 「—」（**绝不当 0**）；
   *   网关日志被裁剪（`todayTokensTruncated`）⇒ 下界 `≥N`，明细与下界说明放 tooltip；
   * - **最后使用**：`AccountMeta.lastUsedAt`（epoch 毫秒）；**缺席则整项省略**（不打印「—」）。
   *
   * ⚠️ 整行**始终渲染**（含数据全缺时）—— 不闪不藏，缺值以「—」如实呈现。
   */
  const metricResources = credit?.ok ? credit.resources ?? [] : [];
  const metricCreditUsed =
    credit?.ok && metricResources.length > 0
      ? metricResources.reduce((sum, resource) => sum + (Number.isFinite(resource.used) ? resource.used : 0), 0)
      : null;
  const metricCreditTotal =
    credit?.ok && metricResources.length > 0
      ? metricResources.reduce((sum, resource) => sum + (Number.isFinite(resource.total) ? resource.total : 0), 0)
      : null;
  /** 「最后使用」：仅当有真实的 `lastUsedAt`（epoch 毫秒）才渲染该项；否则整项省略。 */
  const lastUsedAt = typeof account.lastUsedAt === "number" && account.lastUsedAt > 0 ? account.lastUsedAt : null;

  const metricsRow = (
    <div className={cn("flex flex-wrap gap-x-3.5 gap-y-1 text-[11.5px] text-muted-foreground", compact ? "mb-2.5" : "mb-3")}>
      <span>
        {t("wbAccounts.card.metric.credits")}{" "}
        {metricCreditUsed === null ? (
          <b className="font-semibold tabular-nums text-foreground">—</b>
        ) : (
          <>
            <b className="font-semibold tabular-nums text-foreground">
              {formatCredits(metricCreditUsed)}
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
      {lastUsedAt !== null && (
        <span>
          {t("wbAccounts.card.metric.lastUsed")}{" "}
          <b className="font-semibold tabular-nums text-foreground">{formatDate(lastUsedAt)}</b>
        </span>
      )}
    </div>
  );

  const statusChips = (
    <>
      {isCurrentAccount && <Badge variant="secondary" className={cn(chipClass, "border-transparent bg-primary/15 text-primary")}>{t("wbAccounts.card.currentLogin")}</Badge>}
      {todayCheckedIn !== undefined && (
        <Badge variant={todayCheckedIn ? "success" : "secondary"} className={cn(chipClass, !todayCheckedIn && "text-muted-foreground")}><CircleCheck /> {todayCheckedIn ? t("wbAccounts.card.checkedIn") : t("wbAccounts.card.notCheckedIn")}</Badge>
      )}
      {travelChip(travelStatus)}
      {(account.needsRelogin || expired) && <Badge variant="warning" className={chipClass}>{account.needsRelogin ? t("wbAccounts.card.needRelogin") : t("wbAccounts.card.tokenExpired")}</Badge>}
      {creditPriority && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Badge variant="warning" className={cn(chipClass, "px-1")} aria-label={t("wbAccounts.card.priorityAria")}>
              <Star className="size-3.5" />
            </Badge>
          </TooltipTrigger>
          <TooltipContent side="top">{t("wbAccounts.card.priorityTip")}</TooltipContent>
        </Tooltip>
      )}
      {!compact && activeProductCount >= 2 && <Badge variant="secondary" className={cn(chipClass, "text-muted-foreground")}>{t("wbAccounts.card.toolsInUse", { n: activeProductCount })}</Badge>}
    </>
  );

  /* 程序位行（**单一定义**，紧凑头部与宽松页脚共用 —— 见文件头）。
     ⚠️ 尺寸与形状对齐原型 `.prog`（`buddy-switch-prototype.html:345-357`）：
     高 26px、内边距 0 8px、**圆角 8px（不是胶囊）**、字号 11px、图标 12px。
     ⚠️ 圆角必须写**字面 8px**：本主题里 `rounded-lg` = `--radius` = 12px、`rounded-xl` = 16px，
     都不是原型的 8px —— 26px 的方块配 12px 圆角会「近似圆形」，与截图明显不同。 */
  const btnSize = compact ? ("icon" as const) : ("sm" as const);
  const btnClass = compact
    ? "size-[26px] rounded-[8px]"
    : "h-[26px] rounded-[8px] px-2 text-[11px]";
  const markSize = compact ? 14 : 12;
  const switchLabel = (label: string) => (compact ? null : <span>{label}</span>);

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
    <ProductCurrentState product="workbuddy" compact />
  ) : demoModeEnabled ? (
    <DemoAction>
      <Button variant="outline" size={btnSize} className={btnClass} aria-label={t("wbAccounts.card.setCurrentWorkbuddy")}>
        <WorkBuddyMark size={markSize} />
        {switchLabel(t("wbAccounts.card.setCurrent"))}
      </Button>
    </DemoAction>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={btnClass} disabled={featuresDisabled || !onSwitch} onClick={() => onSwitch?.(account)} aria-label={t("wbAccounts.card.setCurrentWorkbuddy")}>
          <WorkBuddyMark size={markSize} />
          {switchLabel(t("wbAccounts.card.setCurrent"))}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.setCurrentWorkbuddyRestart")}</TooltipContent>
    </Tooltip>
  );

  // CodeBuddy IDE：未检测到 → **引导设置**（不摆一枚点不动的灰按钮）。
  const ideControl = codebuddyCnIdeActive ? (
    <ProductCurrentState product="codebuddy-cn" compact />
  ) : codebuddyCnIdeAvailable ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={btnClass} disabled={featuresDisabled || !onSwitchCodebuddyCnIde || codebuddyCnIdeBusy} onClick={() => onSwitchCodebuddyCnIde?.(account)} aria-label={codebuddyCnIdeLoading ? t("wbAccounts.card.switchingIde") : t("wbAccounts.card.switchIde")} aria-busy={codebuddyCnIdeLoading}>
          {codebuddyCnIdeLoading ? <Loader2 className={compact ? "size-3.5 animate-spin" : "size-4 animate-spin"} /> : <CodeBuddyCnIdeMark size={markSize} />}
          {switchLabel(codebuddyCnIdeLoading ? t("wbAccounts.card.switching") : "IDE")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.switchIdeRestart")}</TooltipContent>
    </Tooltip>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={cn(btnClass, "text-muted-foreground")} disabled={featuresDisabled || !onGuideToSetting} onClick={() => onGuideToSetting?.("ide")} aria-label={t("wbAccounts.card.guideIdeSetting")}>
          <CodeBuddyCnIdeMark size={markSize} />
          {switchLabel("IDE")}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.guideIdeSetting")}</TooltipContent>
    </Tooltip>
  );

  // CodeBuddy CLI：未接入 → **引导设置**（同上）。
  const cliControl = codebuddyCliActive ? (
    <ProductCurrentState product="codebuddy" compact />
  ) : codebuddyCliConfigured ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={btnClass} disabled={featuresDisabled || !onSwitchCodebuddyCli || codebuddyCliBusy} onClick={() => onSwitchCodebuddyCli?.(account)} aria-label={codebuddyCliLoading ? t("wbAccounts.card.switchingCli") : t("wbAccounts.card.setCurrentCli")} aria-busy={codebuddyCliLoading}>
          {codebuddyCliLoading ? <Loader2 className={compact ? "size-3.5 animate-spin" : "size-4 animate-spin"} /> : <CodeBuddyMark size={markSize} />}
          {switchLabel(codebuddyCliLoading ? t("wbAccounts.card.switching") : t("wbAccounts.card.cliCurrent"))}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{t("wbAccounts.card.setCurrentCli")}</TooltipContent>
    </Tooltip>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size={btnSize} className={cn(btnClass, "text-muted-foreground")} disabled={featuresDisabled || !onGuideToSetting} onClick={() => onGuideToSetting?.("cli")} aria-label={t("wbAccounts.card.guideCliSetting")}>
          <CodeBuddyMark size={markSize} />
          {switchLabel(t("wbAccounts.card.cliCurrent"))}
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
    /** 短标签（非紧凑模式显示；与 IDE / CLI 的写法一致）。 */
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
            "inline-flex items-center justify-center border border-primary/40 bg-primary/15 text-primary",
            /* 与 `ProductCurrentState` 同一口径：紧凑模式用方形图标（26px / 圆角 8px），
               否则 5 个槽位会被挤成多行。 */
            compact ? "size-[26px] rounded-[8px]" : "h-[26px] gap-1.5 rounded-[8px] px-2 text-[11px]",
          )}
        >
          <CodeBuddyMark size={compact ? 14 : 12} />
          {compact ? null : <Check className="size-3" strokeWidth={2.25} />}
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
                <Loader2 className={compact ? "size-3.5 animate-spin" : "size-4 animate-spin"} />
              ) : (
                <CodeBuddyMark size={markSize} />
              )}
              {switchLabel(loading ? t("wbAccounts.card.switching") : short)}
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
            className={cn(btnClass, "text-muted-foreground")}
            disabled={featuresDisabled || !onGuidePlugin}
            onClick={() => onGuidePlugin?.(id)}
            aria-label={t("wbAccounts.card.guidePlugin", { client: label })}
            data-program-missing={id}
          >
            <CodeBuddyMark size={markSize} />
            {switchLabel(short)}
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
          className={cn(btnClass, "cursor-not-allowed opacity-50")}
          disabled
          data-program-planned={p.id}
          aria-label={`${t(p.labelKey)}（${t("wbAccounts.program.plannedHint")}）`}
        >
          <Ban className={compact ? "size-3.5" : "size-4"} />
          {switchLabel(t(p.labelKey))}
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
      className={compact ? "size-4" : "size-[18px]"}
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
        onKeyDown={(e) => {
          if (!onOpenDetail) return;
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
        className={cn(
          "relative flex min-w-0 flex-col overflow-hidden rounded-lg border border-border bg-card shadow-card transition-[border-color,box-shadow] hover:border-border-strong hover:shadow-card-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
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
      <header
        className={cn(
          "relative flex items-center border-b border-border",
          compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3",
          /* ★ B3：底色原先第三档用 `codebuddyCliActive`，即**只绑 IDE/插件**的账号
             会掉进 `bg-muted/30` 的「非当前」底色 —— 与左侧竖条 + 徽章矛盾
             （那两重信号用的是 `isCurrentAccount`，覆盖面更广）。
             现在统一读 `isCurrentAccount`：只要绑了**任一**程序位就是「当前登录」。 */
          isCurrentAccount ? "bg-primary/5" : "bg-muted/30",
        )}
      >
        <div className="pointer-events-none absolute inset-0 overflow-hidden">
          <div
            className={cn(
              "absolute -right-10 -top-16 rounded-full blur-2xl",
              compact ? "size-20" : "size-24",
              workbuddyActive ? "bg-primary/15" : codebuddyCliActive ? "bg-muted/50" : "bg-muted/30",
            )}
          />
          {workbuddyActive && (
            <div className={cn("absolute top-[64%] -translate-y-1/2 opacity-[0.075] saturate-50 grayscale-[10%]", codebuddyCliActive ? "right-[68px] rotate-[8deg]" : "right-5 rotate-[7deg]")}>
              <WorkBuddyMark size={compact ? 40 : 56} />
            </div>
          )}
          {codebuddyCliActive && (
            <div className={cn("absolute top-[63%] -translate-y-1/2 opacity-[0.065] saturate-50 grayscale-[18%]", workbuddyActive ? "right-1 -rotate-[8deg]" : "right-5 -rotate-[7deg]")}>
              <CodeBuddyMark size={compact ? 38 : 54} />
            </div>
          )}
        </div>

        <div className={cn("absolute z-20", compact ? "right-2.5 top-1/2 -translate-y-1/2" : "right-3.5 top-3.5")}>
          {demoModeEnabled ? (
            <DemoAction>
              <Button variant="ghost" size="icon" className={cn("rounded-lg text-muted-foreground hover:text-foreground", compact ? "size-7" : "size-8")} aria-label={t("wbAccounts.card.manageAria", { name })} title={t("wbAccounts.card.moreActions")}>
                <Ellipsis />
              </Button>
            </DemoAction>
          ) : (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon" className={cn("rounded-lg text-muted-foreground hover:text-foreground", compact ? "size-7" : "size-8")} aria-label={t("wbAccounts.card.manageAria", { name })} title={t("wbAccounts.card.moreActions")}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuItem disabled={!onOpenDetail} onSelect={() => onOpenDetail?.(account)}>
                  <Info />{t("wbAccounts.card.viewDetail")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem disabled={featuresDisabled || !onRefresh} onSelect={() => onRefresh?.(account)}>
                  <RefreshCw />{t("wbAccounts.card.refreshToken")}
                </DropdownMenuItem>
                {/* ★ F5：**过期 / 需重登**账号不给「手动签到」——token 已失效，签到必失败；
                    批量签到也按同一口径把这类账号算「结构性失败」。
                    ⚠️ 缺这个门控就是「两处口径自相矛盾」（卡片给入口、批量却当失败）。 */}
                {todayCheckedIn === false && !expired && !account.needsRelogin && (
                  <DropdownMenuItem disabled={featuresDisabled || !onCheckin} onSelect={() => onCheckin?.(account)}>
                    <CircleCheck />{t("wbAccounts.card.manualCheckin")}
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem disabled={featuresDisabled || !onEditRemark} onSelect={() => onEditRemark?.(account)}>
                  <StickyNote />{t("wbAccounts.card.editRemark")}
                </DropdownMenuItem>
                <DropdownMenuItem disabled={featuresDisabled || !onEditGroup} onSelect={() => onEditGroup?.(account)}>
                  <Tag />{t("wbAccounts.card.editGroup")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem className="text-destructive focus:bg-destructive/5 focus:text-destructive" onSelect={() => onDelete(account)}>
                  <Trash2 />{t("wbAccounts.card.deleteAccount")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>

        {compact ? (
          <div className="relative z-10 flex w-full min-w-0 items-center gap-2.5 pr-10">
            {checkbox}
            <h3 className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-5" title={name}>{name}</h3>
            <div className="hidden shrink-0 items-center gap-1 min-[420px]:flex">{statusChips}</div>
            <div className="ml-auto shrink-0">{programRow}</div>
          </div>
        ) : (
          <div className={cn("relative z-10 flex w-full min-w-0 items-center gap-3", workbuddyActive || codebuddyCliActive ? "pr-[112px]" : "pr-10")}>
            {checkbox}
            {/* 原型 `.avatar`（`buddy-switch-prototype.html:327-330`）：32px / 圆角 10px / **无外环**。
                原先的 `ring-4 ring-white/65` 是硬编码白环 —— 深色下变成一圈白晕，且原型没有它。 */}
            <div className={cn("flex size-8 shrink-0 items-center justify-center rounded-[10px] text-[12.5px] font-[640]", avatarClass)}>{name.charAt(0).toUpperCase()}</div>
            <div className="min-w-0 flex-1">
              <h3 className="truncate text-[13px] font-semibold leading-5" title={name}>{name}</h3>
              <p className="mt-0.5 truncate text-[11px] leading-5 text-muted-foreground" title={identity}>{identity}</p>
              <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5">{statusChips}</div>
            </div>
          </div>
        )}
      </header>

      <section className={cn("flex min-w-0 flex-1 flex-col", compact ? "px-3.5 pb-3 pt-3" : "px-5 pb-4 pt-4")}>
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
        ) : (
          <>
            <div className="flex items-baseline gap-x-3 gap-y-1">
              <span className="flex items-center gap-1.5">
                <Sparkles className="size-4 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" />
                <strong className={cn("font-semibold leading-none tabular-nums tracking-[-0.025em]", compact ? "text-[20px]" : "text-[22px]")} style={{ fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif' }}>{formatCredits(credit.totalRemaining ?? 0)}</strong>
              </span>
              <span className={cn("text-muted-foreground", compact ? "text-[11px]" : "text-xs")}>{t("wbAccounts.card.creditPacks", { n: resources.length })}</span>
              <div className={cn("ml-auto flex items-center gap-1.5 text-muted-foreground", compact ? "text-[11px]" : "text-xs")} title={expiringAmount > 0 ? t("wbAccounts.card.expireSoon", { amount: formatCredits(expiringAmount) }) : resources[0]?.expireAt ? t("wbAccounts.card.nextExpiry", { date: formatCreditExpiry(resources[0].expireAt, t) }) : t("wbAccounts.card.creditsForever")}>
                <Clock3 className="size-3.5 shrink-0" />
                <span className="whitespace-nowrap tabular-nums">{creditUpdatedAt ? `${formatCreditUpdatedAt(creditUpdatedAt)} ${t("wbAccounts.card.updated")}` : "—"}</span>
              </div>
            </div>

            <div className={cn("text-[11px] font-medium text-muted-foreground", compact ? "mt-3" : "mt-4")}>{t("wbAccounts.card.expiringSoonTitle")}</div>
            <div className={cn(compact ? "mt-1.5 space-y-2" : "mt-2 space-y-2.5")}>
              {visibleResources.length > 0 ? visibleResources.map((resource, index) => {
                const resourceName = displayText(resource.packageName) || displayText(resource.packageCode) || t("wbAccounts.card.creditPackFallback");
                const ratio = resource.total > 0 ? Math.min(100, Math.max(0, (resource.remaining / resource.total) * 100)) : 0;
                return (
                  <div key={`${resource.packageCode ?? "resource"}-${resource.expireAt ?? "none"}-${index}`} className="min-w-0" title={t("wbAccounts.card.resourceTitle", { name: resourceName, remaining: formatCredits(resource.remaining), total: formatCredits(resource.total), expiry: formatCreditExpiry(resource.expireAt, t) })}>
                    <div className={cn("grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3", compact ? "text-[11px]" : "text-xs")}>
                      <span className={cn("rounded-lg bg-muted/80 font-medium tabular-nums text-foreground", compact ? "px-1.5 py-0.5" : "px-2 py-1")}>{t("wbAccounts.card.creditsUnit", { n: formatCredits(resource.remaining) })}</span>
                      <span className="truncate text-muted-foreground">{resourceName}</span>
                      <span className={cn("whitespace-nowrap tabular-nums", expiryClass(resource.expired, resource.expiringSoon))}>{formatCreditExpiry(resource.expireAt, t)}</span>
                    </div>
                    <div className={cn("h-1 overflow-hidden rounded-full bg-muted", compact ? "mt-1" : "mt-1.5")} aria-hidden="true">
                      <div className={cn("h-full rounded-full", resource.expiringSoon || resource.expired ? "bg-warning" : "bg-primary")} style={{ width: `${ratio}%` }} />
                    </div>
                  </div>
                );
              }) : <div className="py-1 text-[11px] text-muted-foreground">{t("wbAccounts.card.noCredits")}</div>}
            </div>
          </>
        )}
      </section>

      {!compact && (
        <footer className="flex flex-wrap items-center gap-2.5 border-t px-5 py-2.5">
          {programRow}
        </footer>
      )}
      </article>
    </TooltipProvider>
  );
}
