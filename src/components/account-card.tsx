import { Ban, Check, CircleCheck, Clock3, Coins, Ellipsis, Info, Loader2, PlaneTakeoff, QrCode, RefreshCw, Sparkles, Star, StickyNote, Trash2 } from "lucide-react";

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
import { demoModeEnabled } from "@/lib/demo-mode";
import { displayText } from "@/lib/display-text";
import { useT, type Translate } from "@/lib/i18n";
import type { AccountMeta, CreditExpiry, CreditResource, TravelStatus } from "@/lib/types";

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
  if (expiringSoon) return "text-orange-600";
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
        "inline-flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-2.5 text-primary shadow-[inset_0_1px_0_rgba(255,255,255,.8)]",
        compact ? "h-7 text-xs" : "h-9",
      )}
    >
      {product === "workbuddy" ? (
        <WorkBuddyMark size={compact ? 18 : 22} />
      ) : product === "codebuddy-cn" ? (
        <CodeBuddyCnIdeMark size={compact ? 18 : 22} />
      ) : (
        <CodeBuddyMark size={compact ? 18 : 22} />
      )}
      <Check className={compact ? "size-3.5" : "size-4"} strokeWidth={2.25} />
    </span>
  );
}

export function AccountCard({ account, onDelete, onCheckin, onRefresh, onSwitch, todayCheckedIn, travelStatus, credit, creditLoading, creditUpdatedAt, creditPriority, workbuddyActive, codebuddyCliConfigured, codebuddyCliActive, codebuddyCliBusy, onSwitchCodebuddyCli, codebuddyCliLoading, codebuddyCnIdeAvailable, codebuddyCnIdeActive, codebuddyCnIdeBusy, codebuddyCnIdeLoading, onSwitchCodebuddyCnIde, onAddPlaintextAccount, onGuideToSetting, selected = false, onToggleSelect, onOpenDetail, onEditRemark, featuresDisabled = true, compact = false }: Props) {
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

  const activeProductCount = [workbuddyActive, codebuddyCliActive, codebuddyCnIdeActive].filter(Boolean).length;

  const statusChips = (
    <>
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

  /* 程序位行（**单一定义**，紧凑头部与宽松页脚共用 —— 见文件头）。 */
  const btnSize = compact ? ("icon" as const) : ("sm" as const);
  const btnClass = compact ? "size-7 rounded-lg" : "h-7 rounded-full px-2.5 pr-3.5 text-xs";
  const markSize = compact ? 15 : 18;
  const switchLabel = (label: string) => (compact ? null : <span>{label}</span>);

  // WorkBuddy 主程序位：当前账号 → 角标；演示模式 → DemoAction；其余 → 切换按钮。
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

  const programRow = (
    <div className="flex shrink-0 items-center gap-1">
      {workbuddyControl}
      {ideControl}
      {cliControl}
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
        className="flex min-w-0 flex-col overflow-hidden rounded-2xl border border-border bg-card shadow-[0_1px_2px_rgba(15,23,42,.025),0_10px_28px_rgba(15,23,42,.035)] transition-shadow hover:shadow-[0_2px_4px_rgba(15,23,42,.04),0_14px_34px_rgba(15,23,42,.055)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
      >
      <header
        className={cn(
          "relative flex items-center border-b border-border",
          compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3",
          workbuddyActive ? "bg-primary/5" : codebuddyCliActive ? "bg-muted/60" : "bg-muted/30",
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
            <div className="ml-auto">{programRow}</div>
          </div>
        ) : (
          <div className={cn("relative z-10 flex w-full min-w-0 items-center gap-3", workbuddyActive || codebuddyCliActive ? "pr-[112px]" : "pr-10")}>
            {checkbox}
            <div className={cn("flex size-12 shrink-0 items-center justify-center rounded-full text-base font-semibold ring-4 ring-white/65", avatarClass)}>{name.charAt(0).toUpperCase()}</div>
            <div className="min-w-0 flex-1">
              <h3 className="truncate text-sm font-semibold leading-5" title={name}>{name}</h3>
              <p className="mt-0.5 truncate text-xs leading-5 text-muted-foreground" title={identity}>{identity}</p>
              <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5">{statusChips}</div>
            </div>
          </div>
        )}
      </header>

      <section className={cn("flex min-w-0 flex-1 flex-col", compact ? "px-3.5 pb-3 pt-3" : "px-5 pb-4 pt-4")}>
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
                      <div className={cn("h-full rounded-full", resource.expiringSoon || resource.expired ? "bg-orange-500" : "bg-primary")} style={{ width: `${ratio}%` }} />
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
