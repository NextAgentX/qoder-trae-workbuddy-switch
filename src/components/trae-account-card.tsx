import { Check, CircleCheck, CircleSlash, Clock3, Coins, Ellipsis, Info, KeyRound, Loader2, Plug, Save, Sparkles, StickyNote, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DemoAction } from "@/components/demo-action";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { avatarTone } from "@/components/account-card";
import { TraeMark, TraeVariantMark } from "@/components/product-marks";
import { displayText } from "@/lib/display-text";
import { useT, type Translate } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { TraeAccount, TraeJwtStatus, TraeRegionId, TraeVariantId } from "@/lib/trae-types";

const chipClass = "rounded-md px-1.5 py-0 text-[11px] font-medium";

/** 剩余小时数 → 可读文案。 */
export function hoursText(hours: number | null, t: Translate): string {
  if (hours === null || !Number.isFinite(hours)) return t("trae.comp.card.hours.unknown");
  if (hours <= 0) return t("trae.comp.card.hours.expired");
  if (hours < 1) return t("trae.comp.card.hours.minutes", { count: Math.round(hours * 60) });
  if (hours < 48) return t("trae.comp.card.hours.hours", { count: hours.toFixed(1) });
  return t("trae.comp.card.hours.days", { count: Math.floor(hours / 24) });
}

/** Unix 秒 → `MM-DD HH:mm`。 */
export function shortTime(seconds: number | null): string {
  if (!seconds) return "—";
  const date = new Date(seconds * 1000);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** JWT 状态 → Badge 变体与文案。 */
export function jwtBadge(status: TraeJwtStatus, hours: number | null, t: Translate) {
  switch (status) {
    case "ok":
      return {
        variant: "secondary" as const,
        className: "",
        text: t("trae.comp.card.jwt.valid", { hours: hoursText(hours, t) }),
      };
    case "warn":
      return {
        variant: "secondary" as const,
        className: "bg-amber-500/15 text-amber-700 dark:text-amber-400",
        text: t("trae.comp.card.jwt.warn", { hours: hoursText(hours, t) }),
      };
    case "expired":
      return { variant: "destructive" as const, className: "", text: t("trae.comp.card.jwt.expired") };
    default:
      return { variant: "outline" as const, className: "", text: t("trae.comp.card.jwt.unparsable") };
  }
}

/** 数字 → 千分位字符串（`null` / 非有限值回落 `—`）。导出给账号页装配 `DetailPack` 复用，避免两处格式化漂移。 */
export function formatCredits(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "—";
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
}

/** Unix 秒 → `MM/DD 到期`（与 WorkBuddy 卡片的 `formatCreditExpiry` 同形）。 */
function formatExpiryShort(seconds: number | null, t: Translate): string {
  if (!seconds) return t("trae.comp.card.expiry.permanent");
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return t("trae.comp.card.expiry.permanent");
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const dd = String(date.getDate()).padStart(2, "0");
  return t("trae.comp.card.expiry.short", { date: `${mm}/${dd}` });
}

/** 到期语气：临期 / 已过期用琥珀，其余用次要前景色（语义色走 class，不写裸色值）。 */
function expiryTone(expired: boolean, expiringSoon: boolean): string {
  return expired || expiringSoon ? "text-amber-600 dark:text-amber-400" : "text-muted-foreground";
}

/** 区域展示名（用于跨区域程序位的**消歧**）。 */
function regionLabelOf(region: TraeRegionId, t: Translate): string {
  return region === "global" ? t("shared.region.version.global") : t("shared.region.version.cn");
}

/**
 * 一个「Trae 程序」维度：该账号可以在哪条 Trae 产品线上启用。
 *
 * ## 与 WorkBuddy 卡片的关系
 *
 * WorkBuddy 的账号卡片头部有一排**工具切换按钮**（WorkBuddy / CodeBuddy IDE /
 * CodeBuddy CLI）——同一个账号可以分别被设为这几个客户端各自的当前账号。
 * Trae 侧的对应物就是本类型：本机装着的每条 Trae 产品线（Trae Work / Trae CN）
 * 都是一枚按钮，账号可以分别在这两条线上启用。
 *
 * 两者是**同一个交互**，不是「Trae 抄 WorkBuddy 的外观」：用户学会
 * 「卡片右上角那排图标 = 把这个账号挂到哪个客户端上」之后，两个分区行为一致。
 */
export interface TraeProgram {
  /**
   * 程序位所属**区域**。
   *
   * ★ 跨区域常显后**必须**带上：`TraeProgramStatus.program` 只有 2 个取值
   * （`trae_work` / `trae_code`，不含区域），跨区域会重复；去重后又会丢失
   * 「该写哪个客户端」。`variant` 是切换目标（区域限定），`region` 用于**消歧与语义**。
   */
  region: TraeRegionId;
  /** 切换目标（区域限定，后端 `TraeProgramStatus.variant`）—— 真正喂给 `traeSwitchAccount` 的值。 */
  variant: TraeVariantId;
  /** 展示名（`Trae Work` / `Trae CN`），取自后端 `variantLabel`。 */
  label: string;
  /** 本机是否检测到该程序（未检测到时按钮禁用，理由写进 tooltip）。 */
  installed: boolean;
  /**
   * 该程序位有没有**可读写的客户端数据目录**（后端 `writeDataDirExists`）。
   *
   * ## 为什么「装了」还不够（2026-09-29 报障）
   *
   * 装了 ≠ 用过。客户端**从未启动过**时，它的 userData 目录压根不存在
   * （本机实测：`D:\Programs\Trae CN\Trae CN.exe` 装着，`%APPDATA%\Trae CN` 没有）
   * ⇒ 既存不出快照、也恢复不进去，这个程序位上的「切换」**永远不可能成功**。
   *
   * 只按 `installed` 判定可用，会渲染出一枚**看着能点、点了必然失败**的按钮 ——
   * 用户看到的是「切换成功但什么都没发生」。有了本字段，按钮才能如实表达
   * 「现在切得过去吗」，并在 tooltip 里说清下一步该做什么。
   */
  hasDataDir: boolean;
  /** 该账号是否正是这个程序的当前账号（由该线登录态快照的 `currentAccount` 判定）。 */
  current: boolean;
}

interface Props {
  account: TraeAccount;
  /** 分组名（用于展示归属；未分组时传 null）。 */
  groupName?: string | null;
  /** 是否为**当前管理的这条产品线**的当前账号（头部高亮 + 幽灵 logo 用它）。 */
  current?: boolean;
  /**
   * 该账号可启用的 Trae 程序（**跨全部区域**，每个 `(region, program)` 一条）。
   *
   * 未传或为空时不渲染任何程序控件——这比渲染一枚「Trae」通用按钮诚实：
   * 后者在两条线并存时根本无法表达「点下去是挂到哪条线上」。
   */
  programs?: TraeProgram[];
  /**
   * **当前页面所在的区域**（`cn` / `global`）。
   *
   * 用来判定「哪些程序位跨区域」：跨区域按钮**可见但不可直接切换** ——
   * 账号库按区域分家，跨库直接切会被后端切换 / 保存守卫拒绝，而那句拒绝用户看不懂
   * （PLAN §3.3）。点击改为**引导切换区域**（`onSwitchRegion`）。
   */
  currentRegion?: TraeRegionId;
  /** 紧凑模式：头部缩成一条、按钮图标化、无 footer */
  compact?: boolean;
  /** 当前正在执行的动作 key（`switch-<uid>@<变体>` / `save-<uid>` / …）。 */
  busy?: string | null;
  /** 任一账号正在切换中，用于阻止并发切换。 */
  switchBusy?: boolean;
  /** 演示模式等场景下禁用所有写操作。 */
  featuresDisabled?: boolean;
  /** 把该账号挂到指定 Trae 程序上（会重启该程序）。 */
  onSwitchTo?: (account: TraeAccount, variant: TraeVariantId) => void;
  /**
   * 跨区域程序位被点击时，**引导用户先切到该区域**（改页面的 `?line=`）。
   *
   * 不实现「跨区域直接切换」：那会被后端守卫拒绝，且报错用户看不懂。
   */
  onSwitchRegion?: (region: TraeRegionId) => void;
  /** 单账号签到。 */
  onCheckin?: (account: TraeAccount) => void;
  onSaveLogin: (account: TraeAccount) => void;
  onRefreshJwt: (account: TraeAccount) => void;
  onClearCooldown: (account: TraeAccount) => void;
  onDelete: (account: TraeAccount) => void;
  /** 是否被勾选（批量操作）。 */
  selected?: boolean;
  onToggleSelect?: (account: TraeAccount) => void;
  /** 打开详情抽屉（抽屉状态由页面持有）。 */
  onOpenDetail?: (account: TraeAccount) => void;
  /** 「更多 → 编辑备注」：打开抽屉并进入编辑态。 */
  onEditRemark?: (account: TraeAccount) => void;
}

/**
 * Trae 账号卡片。
 *
 * 结构与 WorkBuddy 的 `AccountCard` 一一对应（同样的 header/body/footer 三段、
 * 同样的紧凑模式断点、同样的「当前账号」角标与操作菜单），差异只在数据源：
 * Trae 的额度单位是积分、登录凭据是 JWT、切换动作是重启 Trae 客户端。
 *
 * ## P4 卡片瘦身
 *
 * 详情（备注编辑、账号明细）**已迁入共享抽屉**，页脚的「保存登录态 / 刷新 JWT」
 * 两枚按钮收回「更多」菜单（菜单本就有同名项，其说明挂在菜单项的 `title` 上）。
 * 卡片常规可点区只剩：**勾选框 / 程序位行 / 更多菜单**。
 *
 * ## 「挂到哪个程序上」= WorkBuddy 卡片的那排工具按钮
 *
 * WorkBuddy 卡片右上角是三枚工具按钮（WorkBuddy / CodeBuddy IDE / CodeBuddy CLI），
 * Trae 侧就是 {@link TraeProgram} 每枚按钮对应一条 Trae 产品线。**每条产品线各判各的
 * 「当前账号」**：同一个 Trae 账号完全可以同时是 Trae Work 的当前账号与 Trae CN 的
 * 非当前账号，因此每枚按钮必须独立取状态，不能由卡片的 `current` 一刀切。
 */
export function TraeAccountCard({
  account,
  groupName,
  current = false,
  programs = [],
  currentRegion = "cn",
  compact = false,
  busy = null,
  switchBusy = false,
  featuresDisabled = false,
  onSwitchTo,
  onSwitchRegion,
  onCheckin,
  onSaveLogin,
  onRefreshJwt,
  onClearCooldown,
  onDelete,
  selected = false,
  onToggleSelect,
  onOpenDetail,
  onEditRemark,
}: Props) {
  const t = useT();
  const name = displayText(account.name) || `UID · ${account.userId}`;
  const badge = jwtBadge(account.jwtStatus, account.jwtExpHours, t);
  // 忙状态 key 带上变体（`switch-<uid>@<变体>`）：同一账号可能在切另一条线，
  // 只按 uid 判定会把「切 Trae CN 中」的转圈画到 Trae Work 的按钮上。
  const switchKeyPrefix = `switch-${account.userId}@`;
  const switchingVariant: TraeVariantId | null = busy?.startsWith(switchKeyPrefix)
    ? (busy.slice(switchKeyPrefix.length) as TraeVariantId)
    : null;
  const saving = busy === `save-${account.userId}`;
  const refreshingJwt = busy === `jwt-${account.userId}`;
  const thawing = busy === `thaw-${account.userId}`;
  const checkingIn = busy === `checkin-${account.userId}`;
  const deleting = busy === `delete-${account.userId}`;
  const hasCredits = account.remainingCredits !== null;

  /**
   * 「近期到期」要展示的积分包：只取**还有剩余**的包，按最早到期排序，最多 3 条。
   *
   * 数据源是 `account.creditPackages`（T03 经 `credits_overview_for` 的 `packages` 透出，
   * 页面按 uid 合并进账号对象）；未刷新过积分时为 `null` → 空数组 → 如实显示「暂无可用积分」。
   * **绝不**用聚合积分伪造一条假进度条（分母 / 到期日都无从得知）。
   */
  const visiblePackages = (account.creditPackages ?? [])
    .filter((item) => item.remaining > 0)
    .sort(
      (left, right) =>
        (left.expireAt ?? Number.MAX_SAFE_INTEGER) - (right.expireAt ?? Number.MAX_SAFE_INTEGER),
    )
    .slice(0, 3);

  /**
   * 状态标签。
   *
   * 紧凑模式的卡片只有约 300px 宽，标签是 `shrink-0` 的，塞不下四个（冷却标签还带时间戳）。
   * 因此紧凑模式只保留「凭据状态 + 签到状态 + 冷却中」三枚短标签，冷却详情与分组
   * 分别由卡片正文与宽松模式承载——宁可少展示，也不要让标签溢出到按钮上。
   */
  const statusChips = (
    <>
      <Badge variant={badge.variant} className={cn(chipClass, badge.className)}>
        {badge.text}
      </Badge>
      <Badge variant={account.checkedToday ? "success" : "secondary"} className={cn(chipClass, !account.checkedToday && "text-muted-foreground")}>
        {account.checkedToday ? t("trae.comp.card.chip.checked") : t("trae.comp.card.chip.unchecked")}
      </Badge>
      {account.cooldownType &&
        (compact ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Badge variant="warning" className={chipClass}>
                {t("trae.comp.card.chip.cooling")}
              </Badge>
            </TooltipTrigger>
            <TooltipContent side="top">
              {account.cooldownType}
              {account.cooldownUntil ? t("trae.comp.card.cooldown.until", { time: shortTime(account.cooldownUntil) }) : ""}
              {account.cooldownReason ? ` · ${account.cooldownReason}` : ""}
            </TooltipContent>
          </Tooltip>
        ) : (
          <Badge variant="warning" className={chipClass}>
            {account.cooldownType}
            {account.cooldownUntil ? ` · ${shortTime(account.cooldownUntil)}` : ""}
          </Badge>
        ))}
      {!compact && groupName && (
        <Badge variant="secondary" className={cn(chipClass, "text-muted-foreground")}>
          {groupName}
        </Badge>
      )}
      {!compact && account.hasRefreshToken && (
        <Badge variant="secondary" className={cn(chipClass, "text-muted-foreground")}>
          {account.jwtAutoRefresh ? t("trae.comp.card.chip.jwtAuto") : t("trae.comp.card.chip.jwtManual")}
        </Badge>
      )}
    </>
  );

  /**
   * 程序按钮的 tooltip：**先说清为什么不能点，再说点了会怎样**。
   *
   * 三种不可用/可用形态互斥，顺序即优先级：没装 → 装了但没数据目录 → 可切。
   * 「装了但没数据目录」这一支此前缺失，于是那种按钮被当成可切（报障来源）。
   */
  const switchTooltip = (program: TraeProgram) => {
    // 同 `programControl`：`program.label` 未归一（可能是对象），取不到时回落程序位标识。
    const label = displayText(program.label) || program.variant;
    return !program.installed
      ? t("trae.comp.card.switch.missing", { label })
      : program.hasDataDir
        ? t("trae.comp.card.switch.tip", { label })
        : t("trae.comp.card.switch.noDataDir", { label });
  };

  /**
   * 「保存登录态」是否可用：**该账号必须此刻正登录在某个程序上**。
   *
   * 语义上「保存登录态」＝「把客户端**此刻**的登录态存到这个账号名下」，
   * 因此唯一有效的目标就是「这个账号当前登录着的那个程序位」；一个都没有时
   * 后端保存守卫也会拒绝（本机实测会回「客户端当前登录的是另一个账号…」）。
   * 与其让用户点了再吃一句看不懂的拒绝，不如就地禁用并说明。
   */
  const canSaveLogin = programs.some((program) => program.current);

  /**
   * 单个程序的控件。
   *
   * 形态与 WorkBuddy 卡片逐一对齐，另加**跨区域**一支：
   * - **当前账号** → 角标（不可点，`role="status"`），WorkBuddy 同位置同形态；
   * - **跨区域** → 可见但**不可直接切换**，点击引导「先切到该区域」（PLAN §3.3）；
   * - **演示模式** → 包一层 `DemoAction`（控件保持可见可聚焦，点击只解释为什么不可用）；
   * - **其余** → 切换按钮，未安装 / 有并发切换 / 缺回调时禁用，理由写进 tooltip。
   */
  function programControl(program: TraeProgram) {
    const busyHere = switchingVariant === program.variant;
    const key = `${program.region}:${program.variant}`;
    const crossRegion = program.region !== currentRegion;
    // `program.label` 来自后端（`TraeProgramStatus.variantLabel`），类型声明是 `string`
    // 但运行时可能是对象 ⇒ 直接当 React 子节点会卸载整棵树（白屏）。取不到时回落到
    // **程序位标识**（`variant`），不编造展示名。下面所有用到 label 的地方（子节点 /
    // `title` / `aria-label` 插值）统一走这一个归一值，避免漏网。
    const label = displayText(program.label) || program.variant;

    if (program.current) {
      return compact ? (
        <Tooltip key={key}>
          <TooltipTrigger asChild>
            <span
              role="status"
              aria-label={t("trae.comp.card.currentOf", { label })}
              className="relative inline-flex size-7 items-center justify-center rounded-lg border border-primary/25 bg-primary/10 text-primary"
            >
              <TraeVariantMark variant={program.variant} size={15} />
              <span className="absolute -right-1 -top-1 flex size-3.5 items-center justify-center rounded-full bg-primary text-primary-foreground">
                <Check className="size-2.5" strokeWidth={3} />
              </span>
            </span>
          </TooltipTrigger>
          <TooltipContent side="top">{t("trae.comp.card.currentOf", { label })}</TooltipContent>
        </Tooltip>
      ) : (
        <Tooltip key={key}>
          <TooltipTrigger asChild>
            <span
              role="status"
              aria-label={t("trae.comp.card.currentOf", { label })}
              className="inline-flex h-7 items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-2.5 text-xs text-primary shadow-[inset_0_1px_0_rgba(255,255,255,.8)]"
            >
              <TraeVariantMark variant={program.variant} size={18} />
              <Check className="size-3.5" strokeWidth={2.25} />
            </span>
          </TooltipTrigger>
          <TooltipContent side="top">{t("trae.comp.card.currentOf", { label })}</TooltipContent>
        </Tooltip>
      );
    }

    // 跨区域：可见但**不直接切换**，点击引导「先切到该区域」。同名按钮必须消歧。
    if (crossRegion) {
      const regionLabel = regionLabelOf(program.region, t);
      const hint = t("trae.comp.card.switch.regionHint", { region: regionLabel });
      const button = compact ? (
        <Button
          variant="outline"
          size="icon"
          className="size-7 rounded-lg text-muted-foreground"
          disabled={featuresDisabled || !onSwitchRegion}
          onClick={() => onSwitchRegion?.(program.region)}
          aria-label={hint}
        >
          <TraeVariantMark variant={program.variant} size={15} />
        </Button>
      ) : (
        <Button
          variant="outline"
          size="sm"
          className="h-7 rounded-full px-2.5 pr-3 text-xs text-muted-foreground"
          disabled={featuresDisabled || !onSwitchRegion}
          onClick={() => onSwitchRegion?.(program.region)}
          aria-label={hint}
        >
          <TraeVariantMark variant={program.variant} size={18} />
          <span>{label}</span>
          <span className="rounded bg-muted px-1 text-[10px] font-medium">{regionLabel}</span>
        </Button>
      );
      return featuresDisabled ? (
        <DemoAction key={key}>{button}</DemoAction>
      ) : (
        <Tooltip key={key}>
          <TooltipTrigger asChild>
            <span className="inline-flex">{button}</span>
          </TooltipTrigger>
          <TooltipContent side="top">{hint}</TooltipContent>
        </Tooltip>
      );
    }

    const button = compact ? (
      <Button
        variant="outline"
        size="icon"
        className="size-7 rounded-lg"
        disabled={featuresDisabled || switchBusy || !program.installed || !program.hasDataDir || !onSwitchTo}
        onClick={() => onSwitchTo?.(account, program.variant)}
        aria-label={busyHere ? t("trae.comp.card.switch.ariaBusy", { label }) : t("trae.comp.card.switch.aria", { label })}
        aria-busy={busyHere}
      >
        {busyHere ? <Loader2 className="size-3.5 animate-spin" /> : <TraeVariantMark variant={program.variant} size={15} />}
      </Button>
    ) : (
      <Button
        variant="outline"
        size="sm"
        className="h-7 rounded-full px-2.5 pr-3.5 text-xs"
        disabled={featuresDisabled || switchBusy || !program.installed || !program.hasDataDir || !onSwitchTo}
        onClick={() => onSwitchTo?.(account, program.variant)}
        aria-label={busyHere ? t("trae.comp.card.switch.ariaBusy", { label }) : t("trae.comp.card.switch.aria", { label })}
        aria-busy={busyHere}
      >
        {busyHere ? <Loader2 className="size-4 animate-spin" /> : <TraeVariantMark variant={program.variant} size={18} />}
        <span>{busyHere ? t("trae.comp.card.switch.busy") : label}</span>
      </Button>
    );

    return featuresDisabled ? (
      <DemoAction key={key}>{button}</DemoAction>
    ) : (
      <Tooltip key={key}>
        <TooltipTrigger asChild>
          <span className="inline-flex">{button}</span>
        </TooltipTrigger>
        <TooltipContent side="top">{switchTooltip(program)}</TooltipContent>
      </Tooltip>
    );
  }

  const programControls = programs.map(programControl);

  const menu = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className={cn("rounded-lg text-muted-foreground hover:text-foreground", compact ? "size-7" : "size-8")} aria-label={t("trae.comp.card.manage.aria", { name })} title={t("trae.comp.card.manage.title")}>
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-44">
        <DropdownMenuItem disabled={!onOpenDetail} onSelect={() => onOpenDetail?.(account)}>
          <Info />{t("trae.comp.card.detail.open")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          disabled={featuresDisabled || saving || !canSaveLogin}
          onSelect={() => onSaveLogin(account)}
          title={canSaveLogin ? t("trae.comp.card.action.saveTip") : t("trae.comp.card.action.saveNoLogin")}
        >
          <Save />{t("trae.comp.card.menu.save")}
        </DropdownMenuItem>
        {account.hasRefreshToken && (
          <DropdownMenuItem disabled={featuresDisabled || refreshingJwt} onSelect={() => onRefreshJwt(account)} title={t("trae.comp.card.action.refreshJwtTip")}>
            <KeyRound />{t("trae.comp.card.menu.refreshJwt")}
          </DropdownMenuItem>
        )}
        {/* 「手动签到」与 WorkBuddy 卡片菜单同位（刷新之后、删除之前）。
            只在今日未签到时出现：已签到的账号再点一次只会被跳过策略拦下，
            摆一个点不动的入口比不摆更差。 */}
        {!account.checkedToday && (
          <DropdownMenuItem disabled={featuresDisabled || checkingIn} onSelect={() => onCheckin?.(account)}>
            <CircleCheck />{t("trae.comp.card.menu.checkin")}
          </DropdownMenuItem>
        )}
        {account.cooldownType && (
          <DropdownMenuItem disabled={featuresDisabled || thawing} onSelect={() => onClearCooldown(account)}>
            <CircleSlash />{t("trae.comp.card.menu.thaw")}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem disabled={featuresDisabled || !onEditRemark} onSelect={() => onEditRemark?.(account)}>
          <StickyNote />{t("trae.comp.card.menu.remark")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          className="text-destructive focus:bg-destructive/5 focus:text-destructive"
          disabled={deleting}
          onSelect={() => onDelete(account)}
        >
          <Trash2 />{t("trae.comp.card.menu.delete")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  const checkbox = (
    <Checkbox
      checked={selected}
      onCheckedChange={() => onToggleSelect?.(account)}
      disabled={!onToggleSelect}
      aria-label={t("trae.comp.card.selectAria", { name })}
      className={compact ? "size-4" : "size-[18px]"}
    />
  );

  return (
    <TooltipProvider>
      <article className="flex min-w-0 flex-col overflow-hidden rounded-2xl border border-border bg-card shadow-[0_1px_2px_rgba(15,23,42,.025),0_10px_28px_rgba(15,23,42,.035)] transition-shadow hover:shadow-[0_2px_4px_rgba(15,23,42,.04),0_14px_34px_rgba(15,23,42,.055)]">
        <header
          className={cn(
            "relative flex items-center border-b border-border",
            compact ? "min-h-[52px] px-3.5 py-1.5" : "min-h-[104px] px-5 py-3",
            current ? "bg-primary/5" : "bg-muted/30",
          )}
        >
          <div className="pointer-events-none absolute inset-0 overflow-hidden">
            <div className={cn("absolute -right-10 -top-16 rounded-full blur-2xl", compact ? "size-20" : "size-24", current ? "bg-primary/15" : "bg-muted/30")} />
            {current && (
              <div className={cn("absolute right-5 top-[64%] -translate-y-1/2 rotate-[7deg] opacity-[0.075] saturate-50 grayscale-[10%]")}>
                {/* 幽灵水印用**品牌 logo**（`TraeMark`）而不是某一变体的图标：
                    它表达的是「这张卡当前挂在这条 Trae 产品线上」，不是「挂在 Trae CN 上」。 */}
                <TraeMark size={compact ? 40 : 56} />
              </div>
            )}
          </div>

          <div className={cn("absolute z-20", compact ? "right-2.5 top-1/2 -translate-y-1/2" : "right-3.5 top-3.5")}>
            {featuresDisabled ? (
              <DemoAction>
                <Button variant="ghost" size="icon" className={cn("rounded-lg text-muted-foreground hover:text-foreground", compact ? "size-7" : "size-8")} aria-label={t("trae.comp.card.manage.aria", { name })} title={t("trae.comp.card.manage.title")}>
                  <Ellipsis />
                </Button>
              </DemoAction>
            ) : (
              menu
            )}
          </div>

          {compact ? (
            <div className="relative z-10 flex w-full min-w-0 items-center gap-2.5 pr-10">
              {checkbox}
              <h3 className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-5" title={name}>{name}</h3>
              <div className="hidden min-w-0 overflow-hidden shrink-0 items-center gap-1 min-[420px]:flex">{statusChips}</div>
              <div className="ml-auto flex shrink-0 items-center gap-1">{programControls}</div>
            </div>
          ) : (
            <div className="relative z-10 flex w-full min-w-0 items-center gap-3 pr-[112px]">
              {checkbox}
              <div className={cn("flex size-12 shrink-0 items-center justify-center rounded-full text-base font-semibold ring-4 ring-white/65", avatarTone(name))}>
                {name.charAt(0).toUpperCase()}
              </div>
              <div className="min-w-0 flex-1">
                <h3 className="truncate text-sm font-semibold leading-5" title={name}>{name}</h3>
                <p className="mt-0.5 truncate text-xs leading-5 text-muted-foreground" title={account.userId}>
                  UID · {account.userId}
                </p>
                <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5">{statusChips}</div>
              </div>
            </div>
          )}
        </header>

        <section className={cn("flex min-w-0 flex-1 flex-col", compact ? "px-3.5 pb-3 pt-3" : "px-5 pb-4 pt-4")}>
          {/* 大数字行：与 WorkBuddy 卡片同一位置、同一字号层级与图标用法。 */}
          <div className="flex items-baseline gap-x-3 gap-y-1">
            <span className="flex items-center gap-1.5">
              <Sparkles className="size-4 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" />
              <strong
                className={cn("font-semibold leading-none tabular-nums tracking-[-0.025em]", compact ? "text-[20px]" : "text-[22px]")}
                style={{ fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif' }}
              >
                {formatCredits(account.remainingCredits)}
              </strong>
            </span>
            <span className={cn("text-muted-foreground", compact ? "text-[11px]" : "text-xs")}>
              {hasCredits ? t("trae.comp.card.credits.remaining") : t("trae.comp.card.credits.unknown")}
            </span>
            <div
              className={cn("ml-auto flex items-center gap-1.5 text-muted-foreground", compact ? "text-[11px]" : "text-xs")}
              title={
                account.cooldownUntil
                  ? t("trae.comp.card.expiry.cooldownUntil", { time: shortTime(account.cooldownUntil) })
                  : account.creditsExpireAt
                    ? t("trae.comp.card.expiry.earliest", { time: shortTime(account.creditsExpireAt) })
                    : t("trae.comp.card.expiry.none")
              }
            >
              <Clock3 className="size-3.5 shrink-0" />
              <span className="whitespace-nowrap tabular-nums">
                {account.creditsExpireAt ? formatExpiryShort(account.creditsExpireAt, t) : t("trae.comp.card.expiry.noneShort")}
              </span>
            </div>
          </div>

          {/* 冷却 / 未查询提示：与 WorkBuddy 的积分错误行同形（同一图标位、同一配色语义）。 */}
          {account.cooldownType ? (
            <div className={cn("flex min-w-0 items-center gap-2 text-destructive", compact ? "mt-3 text-[11px]" : "mt-4 text-xs")}>
              <Coins className="size-4 shrink-0" />
              <span className="min-w-0 truncate">{account.cooldownReason || t("trae.comp.card.cooldown.fallback", { type: account.cooldownType })}</span>
            </div>
          ) : !hasCredits ? (
            <div className={cn("flex min-w-0 items-center gap-2 text-muted-foreground", compact ? "mt-3 text-[11px]" : "mt-4 text-xs")}>
              <Coins className="size-4 shrink-0" />
              <span className="min-w-0 truncate">{t("trae.comp.card.credits.notQueried")}</span>
            </div>
          ) : null}

          {/* 近期到期：与 WorkBuddy `account-card.tsx` 的积分包区块逐段对齐（剩余徽标 / 包名 /
              到期日 / 比例条），数据源＝本账号的逐包明细 `creditPackages`。
              无包级数据（未刷新过积分）时如实显示「暂无可用积分」，不渲染占位进度条。 */}
          <div className={cn("text-[11px] font-medium text-muted-foreground", compact ? "mt-3" : "mt-4")}>{t("trae.comp.card.section.expiring")}</div>
          <div className={cn(compact ? "mt-1.5 space-y-2" : "mt-2 space-y-2.5")}>
            {visiblePackages.length > 0 ? (
              visiblePackages.map((item, index) => {
                const packageName = displayText(item.packageName) || displayText(item.packageCode) || t("trae.comp.card.package.fallback");
                const ratio = item.total > 0 ? Math.min(100, Math.max(0, (item.remaining / item.total) * 100)) : 0;
                return (
                  <div
                    key={`${item.packageCode ?? "resource"}-${item.expireAt ?? "none"}-${index}`}
                    className="min-w-0"
                    title={t("trae.comp.card.package.tip", {
                      name: packageName,
                      remaining: formatCredits(item.remaining),
                      total: formatCredits(item.total),
                      expiry: formatExpiryShort(item.expireAt, t),
                    })}
                  >
                    <div className={cn("grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3", compact ? "text-[11px]" : "text-xs")}>
                      <span className={cn("rounded-lg bg-muted/80 font-medium tabular-nums text-foreground", compact ? "px-1.5 py-0.5" : "px-2 py-1")}>
                        {t("trae.comp.card.package.remaining", { credits: formatCredits(item.remaining) })}
                      </span>
                      <span className="truncate text-muted-foreground">{packageName}</span>
                      <span className={cn("whitespace-nowrap tabular-nums", expiryTone(item.expired, item.expiringSoon))}>
                        {formatExpiryShort(item.expireAt, t)}
                      </span>
                    </div>
                    <div className={cn("h-1 overflow-hidden rounded-full bg-muted", compact ? "mt-1" : "mt-1.5")} aria-hidden="true">
                      <div
                        className={cn("h-full rounded-full", item.expiringSoon || item.expired ? "bg-amber-500" : "bg-primary")}
                        style={{ width: `${ratio}%` }}
                      />
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="py-1 text-[11px] text-muted-foreground">{t("trae.comp.card.package.empty")}</div>
            )}
          </div>
        </section>

        {!compact && (
          <footer className="flex flex-wrap items-center gap-2.5 border-t px-5 py-2.5">
            {/* 程序切换位：与 WorkBuddy 卡片页脚的工具按钮同位同形。 */}
            {programControls}

            <Tooltip>
              <TooltipTrigger asChild>
                <span className="ml-auto inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  <Plug className="size-3.5" />
                  {account.updatedAt
                    ? t("trae.comp.card.footer.updatedAt", { time: shortTime(Math.floor(new Date(account.updatedAt).getTime() / 1000)) })
                    : t("trae.comp.card.footer.noUpdate")}
                </span>
              </TooltipTrigger>
              <TooltipContent side="top">{t("trae.comp.card.footer.updatedTip")}</TooltipContent>
            </Tooltip>
          </footer>
        )}
      </article>
    </TooltipProvider>
  );
}
