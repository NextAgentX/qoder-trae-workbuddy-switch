import { Check, CircleCheck, CircleSlash, Coins, Ellipsis, Info, KeyRound, Loader2, RefreshCw, Save, StickyNote, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DemoAction } from "@/components/demo-action";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { MenuGroupLabel, MenuHead, MenuNote, RefreshMenuGroup } from "@/components/account-menu-parts";
import type { AccountRefreshKind } from "@/lib/account-refresh";
import { shouldOpenAccountDetail } from "@/lib/account-open-guard";
import type { AccountTokenTodayEntry } from "@/lib/types";
import { compactTokens } from "@/components/usage/format";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { AVATAR_TEXT_CLASS, accountInitials, avatarTone } from "@/components/account-card";
import { displayText } from "@/lib/display-text";
import { EXPIRY_TIP_MAX_HEIGHT, summarizeExpiring } from "@/lib/credit-expiry";
import { formatAbsoluteTime, formatRelativeTime } from "@/lib/catalog-source";
import { useT, type Translate } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { TraeAccount, TraeJwtStatus, TraeProgramId, TraeRegionId, TraeVariantId } from "@/lib/trae-types";

/* 与 WB 卡片同一处理：几何交给 `Badge` 基础样式（已逐值对齐原型 `.badge`），
   这里只抵消它的 1px 透明边框（原型 `.badge` 没有边框）。 */
const chipClass = "border-0 px-[7px] font-medium";

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
        className: "bg-warning/15 text-warning",
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

/**
 * 到期日**只到日**（`MM/DD`）—— 指标行专用。
 *
 * ⚠️ 只返回日期本身：指标行的标签已经写了「到期」，若值再带后缀就成了
 * 「到期 10/03 到期」。原型 `.acct-metrics` 的值也是**纯值**（标签在行内）。
 */
export function formatExpiryDate(seconds: number | null): string | null {
  if (!seconds) return null;
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return null;
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const dd = String(date.getDate()).padStart(2, "0");
  return `${mm}/${dd}`;
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
  /**
   * 程序位标识（`trae_work` / `trae_code`，**不含区域**）。
   *
   * ★ 2026-10-12：按钮上的**可见短名**（`Work` / `Code`，原型 `pg.short`）由它决定。
   * ⚠️ **不能**用 `variant` 或 `label` 猜：`trae_cn` / `trae_code` 这类历史值在
   * `variant.rs` 里**语义不唯一**（`TraeRegion × TraeProgram` 的朴素笛卡尔积命名），
   * `label` 又是展示名（`TraeWork` / `TraeCode` / `Trae` / `… AI`）—— 两者都会把 Code 判成 Work。
   */
  program: TraeProgramId;
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
  /** 是否为**当前管理的这条产品线**的当前账号（头部高亮 + 幽灵 logo 用它）。 */
  current?: boolean;
  /**
   * 该账号可启用的 Trae 程序（**跨全部区域**，每个 `(region, program)` 一条）。
   *
   * 未传或为空时不渲染任何程序控件——这比渲染一枚「Trae」通用按钮诚实：
   * 后者在两条线并存时根本无法表达「点下去是挂到哪条线上」。
   */
  programs?: TraeProgram[];
  /** 当前正在执行的动作 key（`switch-<uid>@<变体>` / `save-<uid>` / …）。 */
  busy?: string | null;
  /** 任一账号正在切换中，用于阻止并发切换。 */
  switchBusy?: boolean;
  /** 演示模式等场景下禁用所有写操作。 */
  featuresDisabled?: boolean;
  /**
   * 把该账号挂到指定 Trae 程序上（会重启该程序）。
   *
   * ⚠️ 第三个参数是**该程序位自己的展示名**，调用方必须原样往上传，**不要**拿
   * `variant` 去反推名字：`trae_cn` 这个变体同时属于 `trae_code`（遗留）与 `trae`
   * （合并版，issue #20），反推会把合并版说成 `TraeCode`。
   * 后端下发的 `program.label` 才是唯一正确来源（本组件已按 `displayText` 归一）。
   */
  onSwitchTo?: (account: TraeAccount, variant: TraeVariantId, label: string) => void;
  /** 单账号签到。 */
  onCheckin?: (account: TraeAccount) => void;
  /**
   * 单账号**刷新积分**（issue #44）。
   *
   * 此前卡片只有一句「点右上角菜单『刷新积分』重试」，而菜单里根本没有这一项 ——
   * 用户唯一能刷新积分的路径是「全选 → 批量刷新 → 取消全选」，为看一眼某个账号的积分
   * 动整张列表。有了这个回调，卡片上（未查询到积分的那一行 + 「更多」菜单）都能一键刷新。
   */
  onRefreshCredits?: (account: TraeAccount) => void;

  /**
   * 单项刷新（积分 / 今日 Token / 签到状态 / 凭据状态）。
   * 不传 ⇒ 菜单保留原来的单项「刷新积分」（向后兼容）；传了 ⇒ 换成四项分组的「刷新」。
   */
  onRefreshPart?: (account: TraeAccount, kind: AccountRefreshKind) => void;
  /** 本账号正在进行中的刷新项。 */
  refreshingKinds?: readonly AccountRefreshKind[];
  /** 刚刚刷新成功（自行淡出）的项，菜单里显示「已更新」。 */
  doneKinds?: readonly AccountRefreshKind[];
  /**
   * 「今日是否已签到」的**局部覆盖**（页面侧刷新签到状态后立即生效）。
   *
   * ⚠️ 为什么不直接改 `account.checkedToday`：那属于账号列表的数据，刷新一个账号
   * 却重取整页会让「点一下刷新」变成整页闪烁；这里把最新事实单独交给卡片渲染。
   */
  checkedTodayOverride?: boolean;
  /**
   * 「今日 Token」（`get_trae_account_token_today` 的单账号条目；issue #47）。
   *
   * ⚠️ **缺席不补 0**：Trae 没有客户端会话落盘，客户端那一档不可得
   * （响应带 `clientSource: "unavailable"`），因此这里只有网关那一档。
   * 没有这一行时菜单里的「刷新今日 Token」会显得「点了没反应」——
   * 刷新出来的数字必须有个落点，否则那条入口就是假的。
   */
  todayTokens?: AccountTokenTodayEntry | null;
  /** 网关日志的今日部分可能被裁 ⇒ 渲染成「≥N」（下界）。 */
  todayTokensTruncated?: boolean;
  /** 本平台真的能刷的项（缺省四项）；见 `RefreshMenuGroup` 的注释。 */
  refreshKinds?: readonly AccountRefreshKind[];
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
 * 同样的「当前账号」角标与操作菜单），差异只在数据源：
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
  current = false,
  programs = [],
  busy = null,
  switchBusy = false,
  featuresDisabled = false,
  onSwitchTo,
  onCheckin,
  onRefreshCredits,
  onRefreshPart,
  refreshingKinds,
  doneKinds,
  checkedTodayOverride,
  todayTokens,
  todayTokensTruncated,
  refreshKinds,
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
  /**
   * ★ **备注优先作为主标题**（issue #83 · 1）。
   *
   * Trae 的 `name` 是客户端导入时兜底生成的机械名（`用户48465672787` / `账号839162`），
   * 用户改备注的唯一目的就是「一眼认出这个号」。此前 `remark` **只在详情抽屉里可见**，
   * 卡片主标题永远是机械名 ⇒ 改了等于没改，而界面上完全看不出「备注没生效」。
   *
   * 现在：有备注 ⇒ 主标题用备注、副标题变成「原名 · UID」；无备注 ⇒ 完全回到原行为
   * （主标题原名、副标题 UID），不引入任何新状态。
   *
   * ⚠️ 头像首字仍取**账号名**而不是备注：备注是给用户认的，头像要跟「这个号是谁」绑定，
   * 备注一改头像就变会让人以为换了账号。
   */
  const remark = displayText(account.remark)?.trim() ?? "";
  const name = displayText(account.name) || `UID · ${account.userId}`;
  const title = remark || name;
  // 忙状态 key 带上变体（`switch-<uid>@<变体>`）：同一账号可能在切另一条线，
  // 只按 uid 判定会把「切 Trae CN 中」的转圈画到 Trae Work 的按钮上。
  const switchKeyPrefix = `switch-${account.userId}@`;
  const switchingVariant: TraeVariantId | null = busy?.startsWith(switchKeyPrefix)
    ? (busy.slice(switchKeyPrefix.length) as TraeVariantId)
    : null;
  const saving = busy === `save-${account.userId}`;
  const refreshingJwt = busy === `jwt-${account.userId}`;
  /** 单账号刷新积分中（issue #44）—— 两个入口（积分提示行 / 菜单项）共用这一个忙标记。 */
  const refreshingCredits = busy === `credits-${account.userId}`;
  const thawing = busy === `thaw-${account.userId}`;
  const checkingIn = busy === `checkin-${account.userId}`;
  const deleting = busy === `delete-${account.userId}`;
  const hasCredits = account.remainingCredits !== null;

  /**
   * 状态标签 —— **槽位与顺序照原型 `.acct-chips`**（`buddy-switch-prototype.html:2505-2508`）：
   *
   * ```
   * 使用中 N 个工具 → 状态 → X 积分 N 天后过期 → 优先
   * ```
   *
   * 原型对**两个平台用同一个渲染器**，所以 Trae 卡片的标签形状与 WorkBuddy 完全一致。
   *
   * ⚠️ 2026-10-12：此前这里挂的是 Trae 专有标签（JWT 有效期 / 冷却 · 时间戳 / 分组 /
   * 自动刷新）—— 5~6 枚、折成两三行，卡片 **243px**，而原型是 **170px**。
   * 信号没有丢，只是**换进原型规定的槽位**：
   *   · JWT 过期 / `SessionDead` 冷却 ⇒ 第 2 槽「状态」（凭据已过期 / 需重新登录）
   *   · 冷却的时间与原因 ⇒ 该标签的 tooltip
   *   · 分组 / 自动刷新 JWT ⇒ 详情抽屉（chips 行是原型的固定四槽，不塞第五枚）
   */
  const activeProgramCount = programs.filter((program) => program.current).length;
  const cooling = Boolean(account.cooldownType) && (account.cooldownUntil ?? 0) * 1000 > Date.now();
  const cooldownTip = account.cooldownType
    ? [
        account.cooldownType,
        account.cooldownUntil ? t("trae.comp.card.cooldown.until", { time: shortTime(account.cooldownUntil) }) : "",
        account.cooldownReason ?? "",
      ]
        .filter(Boolean)
        .join(" · ")
    : "";

  /** 第 2 槽「状态」—— 与原型 `accountStatus()` 同一口径（互斥的一枚）。 */
  const statusChip =
    account.jwtStatus === "expired" || account.cooldownType === "SessionDead" ? (
      <Badge
        variant="destructive"
        className={chipClass}
        title={account.cooldownType === "SessionDead" ? cooldownTip : undefined}
      >
        {account.cooldownType === "SessionDead"
          ? t("trae.comp.card.chip.needRelogin")
          : t("trae.comp.card.chip.expired")}
      </Badge>
    ) : !(checkedTodayOverride ?? account.checkedToday) ? (
      <Badge variant="warning" className={chipClass} title={cooling ? cooldownTip : undefined}>
        {t("trae.comp.card.chip.unchecked")}
      </Badge>
    ) : (
      <Badge variant="success" className={chipClass} title={cooling ? cooldownTip : undefined}>
        <CircleCheck /> {t("trae.comp.card.chip.checked")}
      </Badge>
    );

  /**
   * 第 3 槽「N 积分 M 天内过期」—— 口径与 WorkBuddy 卡片**同一实现**（`summarizeExpiring`）：
   * 金额 = 窗口内全部临期之和、天数 = 其中最晚那一刻，逐批明细进 tooltip。
   *
   * ⚠️ Trae 的 `expireAt` 是**秒**（后端直接透传上游 `expire_time`），WorkBuddy 是毫秒 ——
   * 单位必须显式传给汇总函数，此前这里靠 `> 1e12` 猜单位，猜错一次就会整条徽章消失。
   */
  const expiring = summarizeExpiring(account.creditPackages, { unit: "s" });
  const expiringChip = expiring ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="warning"
          className={chipClass}
          aria-label={t("trae.comp.card.chip.expiring", {
            amount: formatCredits(expiring.total),
            days: expiring.days,
          })}
        >
          {t("trae.comp.card.chip.expiring", { amount: formatCredits(expiring.total), days: expiring.days })}
        </Badge>
      </TooltipTrigger>
      {/* issue #79：逐批明细可能有几十行 ⇒ 限高可滚，标题钉在浮层顶部。
          与 WB 卡片同一处理（`EXPIRY_TIP_MAX_HEIGHT`），见 `account-card.tsx::expiryTipBody`。 */}
      <TooltipContent side="top" style={{ maxHeight: EXPIRY_TIP_MAX_HEIGHT }}>
        <div className="sticky -top-2 -mx-3 -mt-2 bg-popover px-3 pb-1 pt-2 font-medium">
          {t("trae.comp.card.chip.expiringTipTitle", { n: expiring.batches.length })}
        </div>
        {expiring.batches.map((batch) => (
          <div key={batch.expireAt} className="whitespace-nowrap tabular-nums">
            {t("trae.comp.card.chip.expiringTipLine", {
              time: formatAbsoluteTime(batch.expireAt),
              amount: formatCredits(batch.amount),
            })}
          </div>
        ))}
      </TooltipContent>
    </Tooltip>
  ) : null;

  const statusChips = (
    <>
      {activeProgramCount >= 2 && (
        <Badge variant="info" className={chipClass}>
          <CircleCheck /> {t("trae.comp.card.chip.toolsInUse", { n: activeProgramCount })}
        </Badge>
      )}
      {statusChip}
      {expiringChip}
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
   * 形态与 WorkBuddy 卡片逐一对齐：
   * - **当前账号** → 角标（不可点，`role="status"`），WorkBuddy 同位置同形态；
   * - **演示模式** → 包一层 `DemoAction`（控件保持可见可聚焦，点击只解释为什么不可用）；
   * - **其余** → 切换按钮，未安装 / 有并发切换 / 缺回调时禁用，理由写进 tooltip。
   *
   * ⚠️ 2026-10-10：原来的**跨区域**一支已删除 —— `programs` 现在只含**当前区域**的程序位
   * （见 `TraeAccountsPage::programsFor`），跨区域按钮永远不会出现，留着就是死代码。
   */
  function programControl(program: TraeProgram) {
    const busyHere = switchingVariant === program.variant;
    const key = `${program.region}:${program.variant}`;
    // `program.label` 来自后端（`TraeProgramStatus.variantLabel`），类型声明是 `string`
    // 但运行时可能是对象 ⇒ 直接当 React 子节点会卸载整棵树（白屏）。取不到时回落到
    // **程序位标识**（`variant`），不编造展示名。下面所有用到 label 的地方（子节点 /
    // `title` / `aria-label` 插值）统一走这一个归一值，避免漏网。
    const label = displayText(program.label) || program.variant;
    /* ★ 2026-10-12：按钮上的**可见文字**改固定短名（原型 `pg.short` = `Work` / `Code`）。
       `label`（`TraeWork` / `TraeCode`）仍用于 `title` / `aria-label` 与 tooltip。
       ★ 2026-10-13（issue #20）：合并版程序位 `trae` 的短名用 `Trae` —— 它与 `trae_code`
       候选名逐字相同（客户端复用了目录名），但**是两条不同的程序位**
       （各自变体 / 快照目录 / Key 归属），按钮上必须看得出点的是哪一枚。 */
    const short =
      program.program === "trae_code" ? "Code" : program.program === "trae" ? "Trae" : "Work";

    if (program.current) {
      return (
        <Tooltip key={key}>
          <TooltipTrigger asChild>
            <span
              role="status"
              aria-label={t("trae.comp.card.currentOf", { label })}
              /* 与 `account-card` 的「当前账号」同口径（原型 `.prog.on`，`demo.css:332`：
                 底 primary **18%** + 描边 primary 40% + 字 primary + **字重 600**；
                 硬编码的 `inset 0 1px 0 rgba(255,255,255,.8)` 在深色下是一条刺眼白线）： */
              className="inline-flex h-[26px] items-center gap-1 rounded-[8px] border border-primary/40 bg-primary/18 px-2 text-[11px] font-semibold text-primary"
            >
              <Check className="size-3" strokeWidth={2.25} />
              <span>{short}</span>
            </span>
          </TooltipTrigger>
          <TooltipContent side="top">{t("trae.comp.card.currentOf", { label })}</TooltipContent>
        </Tooltip>
      );
    }

    const button = (
      <Button
        variant="outline"
        size="sm"
        className="h-[26px] gap-1 rounded-[8px] border-border bg-transparent px-2 text-[11px] !font-normal text-muted-foreground"
        disabled={featuresDisabled || switchBusy || !program.installed || !program.hasDataDir || !onSwitchTo}
        onClick={() => onSwitchTo?.(account, program.variant, label)}
        aria-label={busyHere ? t("trae.comp.card.switch.ariaBusy", { label }) : t("trae.comp.card.switch.aria", { label })}
        aria-busy={busyHere}
      >
        {busyHere ? <Loader2 className="size-3.5 animate-spin" /> : null}
        <span>{busyHere ? t("trae.comp.card.switch.busy") : short}</span>
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

  /**
   * 程序位行（**单一定义**，页脚程序位行）——与 WorkBuddy 的 `programRow` 同构。
   *
   * ⚠️ `w-full` + `flex-wrap` 与 WorkBuddy 逐字一致：程序位到 4 个（TraeWork / TraeCode ×
   * 两个区域）时不换行会溢出卡片；只加 `min-w-0` 又会让本行收缩到最小内容宽、槽位被压成多行。
   */
  const programRow = (
    <div className="flex w-full flex-wrap items-center gap-1">
      {programs.map(programControl)}
    </div>
  );

  /**
   * ★ 与 WorkBuddy 卡片**同一套结构**（原型 `.acct`，`buddy-switch-prototype.html:2497-2535`）：
   * 左侧当前竖条 → 头部（勾选 + 头像 + 昵称/「当前登录」徽章 + 身份行 + chips）→ 指标行 →
   * 正文（积分 + 到期包）→ 页脚程序位。
   *
   * ⚠️ 差异**只在数据**：Trae 的额度单位是积分、凭据是 JWT、身份行是 UID（Trae 没有邮箱）。
   * 结构与交互（整卡可点、当前登录两重信号、指标行、程序位页脚）必须逐项一致 ——
   * 这正是本次重构「用原型统一两个平台」的要求。
   */
  // 有备注时副标题要带上原名（否则「这个号是谁」的信息被备注挤掉了）。
  const identity = remark ? `${name} · UID · ${account.userId}` : `UID · ${account.userId}`;

  /* 「当前登录」徽章**单独拆出来**、与昵称同一行（原型 `:2502`）。
     ⚠️ 不能塞进下面的 `statusChips` 行：那会把头部撑成三行（WorkBuddy 侧已踩过）。 */
  const currentLoginBadge = current ? (
    <Badge variant="secondary" className={cn(chipClass, "shrink-0 border-transparent bg-primary/16 text-primary")}>
      {t("trae.comp.card.currentLogin")}
    </Badge>
  ) : null;

  /* 指标行（原型 `.acct-metrics` `:2510`）：与 WorkBuddy 同位置、同字号、同「标签 + 粗体值」形状。
     ⚠️ Trae **没有**「今日 Token」数据源 ⇒ 该项不渲染（不摆一个永远「—」的假指标）；
     取而代之的是 Trae 真实有的「到期」（WorkBuddy 侧对应的是积分包最早到期）。 */
  const updatedSeconds = account.updatedAt ? Math.floor(new Date(account.updatedAt).getTime() / 1000) : null;
  /* ★ 指标行 —— 与 WorkBuddy 卡片、原型 `.acct-metrics`（`demo.css:341-342`）同一形状：
     `display:flex; gap:14px; font-size:11.5px; color:var(--muted-fg)`，**不换行**、无下边距。
     Trae 没有「今日 Token」数据源 ⇒ 第二格用真实有的「到期」，**不摆一个永远「—」的假指标**。
     第三格与原型一致：**无标签的相对时间**（`2 分钟前`），绝对时刻挂 `title`。 */
  /* 积分总量 = 各包 `total` 之和（原型 `.acct-metrics` 是 `积分 X/Y`）。取不到包 ⇒ 不写「/Y」。 */
  const creditTotal = (() => {
    const pkgs = account.creditPackages ?? [];
    if (pkgs.length === 0) return null;
    const sum = pkgs.reduce((acc, pkg) => acc + pkg.total, 0);
    return sum > 0 ? sum : null;
  })();

  /** 今日 Token 合计（客户端 + 网关；Trae 客户端档恒为 0）。 */
  const todayTokenTotal = (entry: AccountTokenTodayEntry) =>
    entry.clientIn + entry.clientOut + entry.gatewayIn + entry.gatewayOut;

  const lastUpdatedRelative = formatRelativeTime(updatedSeconds === null ? null : updatedSeconds * 1000, t);
  const metricsRow = (
    <div className="flex min-w-0 gap-[14px] text-[11.5px] text-muted-foreground">
      <span>
        {t("trae.comp.card.metric.credits")}{" "}
        <b className="font-semibold tabular-nums text-foreground">{formatCredits(account.remainingCredits)}</b>
{creditTotal === null ? "" : `/${formatCredits(creditTotal)}`}
      </span>
      <span>
        {t("trae.comp.card.metric.expiry")}{" "}
        <b className="font-semibold tabular-nums text-foreground">
          {formatExpiryDate(account.creditsExpireAt) ?? t("trae.comp.card.expiry.permanent")}
        </b>
      </span>
      {todayTokens ? (
        <span title={t("shared.cardMenu.refresh.tokensHint")}>
          {t("shared.stat.tokens")}{" "}
          <b className="font-semibold tabular-nums text-foreground">
            {todayTokensTruncated
              ? t("wbAccounts.card.metric.atLeast", { value: compactTokens(todayTokenTotal(todayTokens)) })
              : compactTokens(todayTokenTotal(todayTokens))}
          </b>
        </span>
      ) : null}
      {lastUpdatedRelative !== null && (
        <span title={t("trae.comp.card.footer.updatedTip")}>{lastUpdatedRelative}</span>
      )}
    </div>
  );

  /* 「更多」菜单（**单一定义**，页脚 `.acct-actions` 用它）——与 WorkBuddy 卡片同形：
     演示模式下包一层 `DemoAction`（控件保持可见可聚焦，点击只解释为什么不可用）。 */
  const menu = featuresDisabled ? (
    <DemoAction>
      <Button variant="ghost" size="sm" className="!px-[10px] text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("trae.comp.card.manage.aria", { name })} title={t("trae.comp.card.manage.title")}>
        <Ellipsis className="size-4" />
      </Button>
    </DemoAction>
  ) : (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="!px-[10px] text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("trae.comp.card.manage.aria", { name })} title={t("trae.comp.card.manage.title")}>
          <Ellipsis className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <MenuHead>{title}</MenuHead>
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
        {/* ★ issue #44：菜单里**真的**要有「刷新积分」，卡片上的提示才不是空口指路。
            此前提示文案指向这个菜单项，而菜单里只有「刷新 JWT」 —— 用户点了也不会刷新积分。
            可见入口在下面未查询到积分的那一行，这里补的是「已有积分时也想强制更新」的路径。 */}
        {onRefreshPart ? (
          <RefreshMenuGroup refreshingKinds={refreshingKinds} doneKinds={doneKinds} kinds={refreshKinds} onPick={(kind) => onRefreshPart(account, kind)} />
        ) : (
          <DropdownMenuItem
            disabled={featuresDisabled || refreshingCredits || !onRefreshCredits}
            onSelect={() => onRefreshCredits?.(account)}
            title={t("trae.comp.card.action.refreshCreditsTip")}
          >
            {refreshingCredits ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("trae.comp.card.menu.refreshCredits")}
          </DropdownMenuItem>
        )}
        <MenuGroupLabel>{t("shared.cardMenu.accountGroup")}</MenuGroupLabel>
        {/* 「手动签到」与 WorkBuddy 卡片菜单同位（刷新之后、删除之前）。
            只在今日未签到时出现：已签到的账号再点一次只会被跳过策略拦下，
            摆一个点不动的入口比不摆更差。 */}
        {!(checkedTodayOverride ?? account.checkedToday) && (
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
              <MenuNote>{t("shared.cardMenu.note")}</MenuNote>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  const checkbox = (
    <Checkbox
      checked={selected}
      onCheckedChange={() => onToggleSelect?.(account)}
      disabled={!onToggleSelect}
      aria-label={t("trae.comp.card.selectAria", { name })}
      className="size-[18px]"
    />
  );

  return (
    <TooltipProvider>
      <article
        /* ★ 整卡可点开详情 —— 与 WorkBuddy 卡片**同一交互**（原型 `.acct` 是 `data-act="acct-open"`
           的整卡按钮区，`buddy-switch-prototype.html:2497`）。
           ⚠️ 内部控件（勾选框 / 程序位按钮 / 更多菜单）必须**不触发**开详情 ⇒ 用 `closest` 守卫。
           ⚠️ `role` 用 `article` 而非原型写的 `button`：卡内嵌着多个可交互控件，
           嵌套在 `button` 里是非法 ARIA；`article` + `tabIndex` + Enter/Space 是等价可达的正确写法。 */
        tabIndex={0}
        role="article"
        onClick={(e) => {
          if (!onOpenDetail) return;
          /* ★ 判据在 `lib/account-open-guard.ts`（三张卡 + 表格行共用），
             别再退回手写 `closest("button, …")` —— 菜单项是 Portal 里的
             `div[role="menuitem"]`，手写判据拦不住（2026-10-22 报障根因）。 */
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
        className={cn(
          /* ★ 原型 `.acct`（`buddy-switch-prototype.html:312-317`）：
             `display:flex; flex-direction:column; gap:10px; padding:13px 14px`
             ⇒ 卡片是「**四个块 + 统一 10px 间距**」，内边距在**根**上，各块不再自带 padding。
             与 WorkBuddy 卡片逐值一致（`rounded-lg` = `var(--radius)` = 12px）。 */
          "relative flex min-w-0 flex-col gap-2.5 overflow-hidden rounded-lg border border-border bg-card px-3.5 py-[13px] shadow-card transition-[border-color,box-shadow] hover:border-border-strong hover:shadow-card-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
          selected && "border-ring bg-primary/[0.07]",
        )}
      >
      {/* ★ 原型 `.acct.is-current::before`（`buddy-switch-prototype.html:321-324`）：
          「当前账号」用**左侧 3px 竖条**表达 —— 与头部的「当前登录」徽章构成两重信号。
          条件必须与徽章**同为** `current`，否则会出现「有徽章却无竖条」的矛盾态。 */}
      {current ? (
        <span
          aria-hidden="true"
          data-current-bar="true"
          className="pointer-events-none absolute inset-y-0 left-0 w-[3px] rounded-l-lg bg-primary"
        />
      ) : null}
        <header className="flex items-center gap-2.5">
          {/* ⚠️ `...` 菜单**不在这里** —— 原型把它放在 `.acct-actions` 里（程序位行右侧），
              见页脚 `{menu}`。
              ⚠️ 2026-10-12 按原型删掉了头部底色带（`bg-primary/5` / `bg-muted/30`）与幽灵水印：
              原型 `.acct-top` 是**透明的**，整张卡只有一个底色；底色带把它变成了一条「页眉」，
              还把卡片切成两半。 */}
          {/* 原型 `.acct-top`（`buddy-switch-prototype.html:2499-2503`）的四件套：
             `[勾选] [头像32] [.acct-id：昵称 + 身份] [当前登录徽章]`。
             ⚠️ 徽章是 `.acct-top` 的**直接子级**（行尾独立件），不是昵称那一行里的小尾巴；
             ⚠️ 状态标签**移出头部** —— 原型里 `.acct-chips` 是 `.acct-top` 的**兄弟块**。 */}
          <div className="relative z-10 flex w-full min-w-0 items-center gap-2.5">
            {checkbox}
            {/* 原型 `.avatar`（`demo.css:308-311`）：32px / 圆角 10px / `12.5px` / `640` / 近白字，
                首字取**前 2 位**（`initials()`）。与 WorkBuddy 卡片逐值一致。 */}
            <div className={cn("flex size-8 shrink-0 items-center justify-center rounded-[10px] text-[12.5px] font-[640]", AVATAR_TEXT_CLASS, avatarTone(name))}>
              {accountInitials(name)}
            </div>
            <div className="min-w-0 flex-1">
              <h3 className="truncate text-[13px] font-semibold" title={remark ? `${title}（${name}）` : title}>{title}</h3>
              <p className="truncate text-[11px] text-muted-foreground" title={identity}>{identity}</p>
            </div>
            {currentLoginBadge}
          </div>
        </header>

        {/* ★ 原型 `.acct-chips`（`buddy-switch-prototype.html:341`）：`flex-wrap; gap:5px`，
            是 `.acct-top` 的**兄弟块**（卡片共 4 个块），不是塞在头像右侧的第三行。 */}
        <div className="flex min-w-0 flex-wrap items-center gap-[5px]">{statusChips}</div>

        {/* 原型 `.acct-metrics`（`buddy-switch-prototype.html:2510`）：徽章之下的一行小字，
            **始终渲染** —— 取不到的项显示「—」，不闪不藏。
            ⚠️ 2026-10-12 按原型删掉了「大数字行 + 近期到期逐包列表」：原型卡片只有 4 个块
            （`.acct-top` / `.acct-chips` / `.acct-metrics` / `.acct-actions`），逐包明细改由
            **详情抽屉**承载（`TraeAccountsPage` 装配时读 `creditPackages`）。 */}
        <section className="flex min-w-0 flex-1 flex-col">
          {metricsRow}

          {/* 冷却 / 未查询提示：与 WorkBuddy 的积分错误行同形（同一图标位、同一配色语义）。 */}
          {account.cooldownType ? (
            <div className="flex min-w-0 items-center gap-2 py-3 text-sm text-destructive">
              <Coins className="size-4 shrink-0" />
              <span className="min-w-0 truncate">{account.cooldownReason || t("trae.comp.card.cooldown.fallback", { type: account.cooldownType })}</span>
            </div>
          ) : !hasCredits ? (
            /* ★ issue #44：这句提示原先写「点右上角菜单『刷新积分』重试」—— 而菜单里没有这一项。
               现在提示只说事实（尚未查询到积分），出口就地给一个真的能点的按钮：
               为看一眼某个账号的积分去「全选 → 批量刷新 → 取消全选」是没必要的绕路。 */
            <div className="flex min-w-0 items-center gap-2 py-3 text-sm text-muted-foreground">
              <Coins className="size-4 shrink-0" />
              <span className="min-w-0 flex-1 truncate">{t("trae.comp.card.credits.notQueried")}</span>
              <Button
                variant="outline"
                size="sm"
                className="h-7 shrink-0 gap-1 px-2 text-[11px]"
                disabled={featuresDisabled || refreshingCredits || !onRefreshCredits}
                onClick={() => onRefreshCredits?.(account)}
                title={t("trae.comp.card.action.refreshCreditsTip")}
              >
                {refreshingCredits ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
                {t("trae.comp.card.credits.refresh")}
              </Button>
            </div>
          ) : null}
        </section>

        {/* 原型 `.acct-actions`（`demo.css:343` / `accountCard` :2515-2534）：
           `display:flex; align-items:center; gap:6px; padding-top:10px; border-top:1px solid var(--border)`
           ⇒ 页脚 = **程序位行（`flex:1`）+ 「更多」按钮**（菜单在**这里**，不是头部）。
           与 WorkBuddy 卡片页脚逐字同构。 */}
        <footer className="flex items-center gap-1.5 border-t border-border pt-2.5">
          {programRow}
          {menu}
        </footer>
      </article>
    </TooltipProvider>
  );
}
