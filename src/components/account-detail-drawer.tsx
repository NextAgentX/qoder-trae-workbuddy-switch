import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowRight, Check, Download, Info, Loader2, MessageSquare, Pencil, RefreshCw, ShieldCheck, StickyNote, XIcon } from "lucide-react";

import { avatarTone, formatCredits } from "@/components/account-card";
import { AccountGroupPicker } from "@/components/account-group-picker";
import type { AccountRefreshKind } from "@/lib/account-refresh";
import { EmptyState } from "@/components/ui/empty-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { displayText } from "@/lib/display-text";
import {
  isDrawerTab,
  resolveDrawerTab,
  useDrawerTab,
  type DrawerTab,
} from "@/lib/drawer-tab-preference";
import { useT } from "@/lib/i18n";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { cn } from "@/lib/utils";

/**
 * 账号详情抽屉（**两平台共用壳**）。
 *
 * ## 为什么是「壳 + 数据注入」而不是两平台各写一份
 *
 * WorkBuddy 与 Trae 的详情**字段不同、结构相同**（头部 + 备注 + 明细区块 + 只读程序位）。
 * 若各写一份，两边会立刻在间距、空态、加载态上漂移 —— 本仓在 Tab、空态、片段上
 * 都踩过「两份实现各自演化」的坑（见 `gateway-snippets.ts` 的头注释）。
 * 因此这里只做**排版与交互**，平台差异一律由调用方装配成
 * `sections` / `packs` / `programs` 注入（PLAN §3.3：差异用数据表达）。
 *
 * ## 为什么是抽屉而不是模态（PLAN §3.5）
 *
 * 用户在账号列表里逐个查看详情，模态会盖住列表、打断对照；抽屉从右侧推入，
 * 列表仍在原位可见。原语是 `ui/sheet.tsx`（shadcn `Sheet`）。
 *
 * ## 两条硬约束（违反会出事故）
 *
 * 1. **所有展示字段必须是已归一的字符串**。`DetailRow.value` / `DetailPack.*Text` /
 *    头部 `name` / `subtitle` 的类型都是 `string` —— 调用方必须过 `displayText`
 *    （`@/lib/display-text`）。对象当 React 子节点渲染会让 React **卸载整棵树** ⇒ 白屏
 *    （issue #2）。这里用 `string` 而非 `unknown` 是为了让「忘了归一」在编译期暴露。
 * 2. **动作只留「必须看上下文才敢做」的那几个**。抽屉是**看**的地方：
 *    备注 / 分组就地编辑（改之前必须看见原文、看见已有哪些组）、续期 JWT、
 *    权限诊断、导出；**手动签到 / 删除**这类一步就能做完的动作只在卡片的「更多」菜单里
 *    （2026-10-18 分工判据，见 `docs/specs/PLAN.md` §3.3）—— 同一个动作两处都有入口
 *    正是上一版被吐槽的「乱」。程序位只以**只读徽标**呈现（切换属于列表页）。
 *
 * ## 状态语言（PLAN §3.6）
 *
 * 区块**首次加载**用 `<Skeleton>`；空态用 `<EmptyState>`（四要素，出口由 `empty.action` 提供）。
 * 注意：`loading` 是**区块加载态**，不是按钮操作态 —— 按钮转圈归卡片，不进这里。
 */

/** 详情里的一行「标签 / 值」。`value` 必须**已归一为字符串**（见文件头约束 1）。 */
export interface DetailRow {
  /** 已 i18n 的标签。 */
  label: string;
  /** 已过 `displayText` 出口的值；绝不可能是对象。 */
  value: string;
  /** 等宽（uid / 路径 / 时间戳）。 */
  mono?: boolean;
  /** 悬停完整值（`value` 被截断时）。 */
  title?: string;
}

/**
 * 顶部指标块（积分 / 今日 Token / 签到 / 凭据）。
 *
 * ⚠️ 四段文本**已在调用方格式化**（与本文件其余区块同一约束）：抽屉不判断单位与日期格式。
 * `refreshKind` 指向 `lib/account-refresh` 的四项之一 ⇒ 块右上角出现就地 ⟳，
 * 与卡片「更多」菜单里的四项**同源**（同一个 `onRefreshPart`）。
 */
export interface DetailStat {
  key: string;
  /** 已 i18n 的指标名。 */
  label: string;
  /** 已格式化的主值（如 `1,240/2,000`）。 */
  valueText: string;
  /** 已格式化的副文本（如「320 积分 3 天内到期」）。 */
  subText?: string;
  /** 副文本语气：临期用 warn、凭据失效用 bad。 */
  tone?: "warn" | "bad";
  /** 该指标对应的刷新项；不传 ⇒ 不渲染 ⟳。 */
  refreshKind?: AccountRefreshKind;
}

/**
 * 登录态快照的一行（只有声明了 `snapshot` 能力的平台才有）。
 *
 * ⚠️ 与 `DetailPack` 同一条约束：四段文本**已在调用方格式化**，抽屉不判断单位与日期格式。
 */
export interface DetailSnapshot {
  key: string;
  timeText: string;
  sizeText: string;
  fileCountText: string;
}

/** 详情里的一个区块（标题 + 若干行）。 */
export interface DetailSection {
  /** React key，稳定。 */
  key: string;
  /** 已 i18n 的区块标题。 */
  title: string;
  rows: DetailRow[];
}

/**
 * 积分 / 资源包明细行（WB 与 Trae 同形）。
 *
 * ⚠️ 四段文本**已在调用方格式化**（含「已用」这类标签）—— 抽屉内不再判断单位 / 日期格式，
 * 否则又会出现「同一句话两处各写一遍」。
 */
export interface DetailPack {
  key: string;
  name: string;
  remainingText: string;
  totalText: string;
  usedText: string;
  expiryText: string;
  expired: boolean;
  expiringSoon: boolean;
  /** 0..100，已夹紧。 */
  ratio: number;
}

/** 只读程序位徽标（**不是**切换控件）。 */
export interface DetailProgramBadge {
  /** 含区域，稳定唯一。 */
  key: string;
  /** 已消歧（含区域标识）。 */
  label: string;
  current: boolean;
  installed: boolean;
  hasDataDir: boolean;
}

/**
 * 可点的「切换到此账号」程序位（原型 `drawerAccount` :3480-3484）。
 *
 * ⚠️ 与只读徽标 `DetailProgramBadge` 是**两套输入**：调用方注入 `switchTargets` 时渲染可点行
 * （WB）；不注入时退回只读徽标（Trae），不改变现有行为。
 */
export interface DetailSwitchTarget {
  /** 稳定唯一。 */
  key: string;
  /** 已 i18n 的程序位名。 */
  label: string;
  /** 已是该程序位的当前账号（只读高亮，不再重复切换）。 */
  current: boolean;
  /** 该程序位可切换（已接入 / 已检测到）。false ⇒ 渲染为不可用并标注「未检测到」。 */
  available: boolean;
  busy?: boolean;
  loading?: boolean;
  onSwitch?: () => void;
}

/**
 * 「操作」区回调（原型 `drawerAccount` :3518-3527）。**全部可选** —— 缺哪个回调就不渲染哪枚按钮，
 * 不摆点不动的灰按钮。`编辑备注` 由抽屉内部的就地编辑器承担，不需回调。
 */
export interface DetailActions {
  onRefresh?: () => void;
  onCheckin?: () => void;
  /** 是否展示「手动签到」（凭据可用且今日未签到）。 */
  showCheckin?: boolean;
  onExport?: () => void;
  onPermission?: () => void;
  onDelete?: () => void;
  /** 凭据已过期 / 需重登 ⇒ 展示「不提供签到入口」的说明（与卡片 / 批量签到同口径）。 */
  credentialUnavailable?: boolean;
}

/**
 * 签到历史一行（近 7 天；原型 `checkinHistoryHtml` :3428-3439）。
 *
 * ⚠️ `amount` 来自**积分统计的签到事件**（账号 + 日期），取不到时为 `null` ⇒ 渲染「—」。
 * **绝不**用固定奖励（原型里那个 `checkinReward || 100` 是假数据，本仓不照抄）。
 */
export interface DetailCheckinHistoryRow {
  date: string;
  /** 已 i18n 的结果文案（成功 / 已签到 / 失败 / 未签到）。 */
  resultLabel: string;
  tone: "ok" | "warn" | "fail" | "none";
  /** `null` = 上游未给额度 ⇒ 渲染「—」。 */
  amount: number | null;
}

export interface AccountDetailDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /**
   * ★ 账号的**稳定身份键**（`${scope}:${id}`，见 issue #49）。
   *
   * 只用来记住「这个账号上次停在哪个页签」：同一 `uid` 在国内版与国际版是**两个**账号，
   * 所以键里必须带作用域。设为**必填**是有意的 —— 漏传会在 `tsc` 阶段报错，
   * 而不是静默退化成「不记忆」。
   */
  accountKey: string;
  /** 头部展示名（已过 `displayText` 出口）。 */
  name: string;
  /** 头部副标题（身份 / uid，已归一）。 */
  subtitle: string;
  /** 头像底色种子（复用 `avatarTone`，与卡片同一套色板）。 */
  avatarSeed: string;
  /** 区块加载中（首次打开、数据未就绪）→ `<Skeleton>`；**不是**按钮态。 */
  loading?: boolean;
  /** 积分 / 资源包明细；空数组 = 不渲染该区块。 */
  packs?: DetailPack[];
  /**
   * 积分包区块下方的**说明 + 一个动作**（文案由调用方给，本组件不含平台专属词条）。
   *
   * 用途：某些平台的逐包明细要额外授权才拿得到（Qoder 需要网页端登录一次），
   * 此时列表里只剩聚合回落行 —— 不说明原因的话，用户会以为「这个账号没有包」。
   * 传 `null` / 不传 ⇒ 不渲染（WB / Trae 的逐包本来就齐，不需要说明）。
   */
  packsNotice?: {
    text: string;
    actionLabel?: string;
    onAction?: () => void;
    /** 动作进行中（按钮置灰 + 转圈由调用方文案体现）。 */
    busy?: boolean;
  } | null;
  /** 登录态快照（空数组 = 不渲染「快照」页签）。 */
  snapshots?: DetailSnapshot[];
  /** 顶部指标块（按序渲染；空数组 = 不渲染该区域）。 */
  stats?: DetailStat[];
  /** 本账号正在进行中的刷新项（指标块上的 ⟳ 用它显示转圈）。 */
  refreshingKinds?: readonly AccountRefreshKind[];
  /** 指标块上的就地刷新；不传 ⇒ 块上不出现 ⟳。 */
  onRefreshPart?: (kind: AccountRefreshKind) => void;
  /** 信息区块（按序渲染）。 */
  sections?: DetailSection[];
  /** 只读程序位徽标；空数组 = 不渲染。 */
  programs?: DetailProgramBadge[];
  /** 备注就地编辑（与卡片同语义：返回 true 才收起编辑器）。 */
  remark: string;
  onSaveRemark?: (remark: string) => Promise<boolean>;
  /**
   * 自定义分组就地编辑（与备注**同一交互**）。
   *
   * ⚠️ 只在调用方**注入 `onSaveGroup`** 时渲染该区块 —— 未注入（如 Trae，其分组是
   * `groupId` 而非自由文本）时保持原样，不凭空多出一个「分组」区。空串 = 未分组。
   */
  group?: string;
  onSaveGroup?: (group: string) => Promise<boolean>;
  /**
   * 本作用域里**已有的分组名**（去重，不含空串）。
   *
   * 只给「分组选择器」用：用户要能看见「已经有哪些组」（避免把同一个组打成三种写法），
   * 也要能建新组（自由文本）。缺省 = 只知道当前值 —— 选择器仍然可用，只是列表更短。
   */
  groups?: string[];
  featuresDisabled?: boolean;
  /** 由「更多 → 编辑备注」打开时，直接进入备注编辑态。 */
  autoEditRemark?: boolean;
  /** 由「更多 → 设置分组」打开时，直接进入分组编辑态。 */
  autoEditGroup?: boolean;
  /** 详情区块**全空**时的空态出口（`EmptyState` 四要素）。 */
  empty?: { title: string; description: string; action?: ReactNode };
  /**
   * ★ B5：**签到积分**数据（字段来自**上游实测响应**，不是推测）。
   *
   * ⚠️ 传 `null` / 不传 = **不渲染签到区** —— 不摆「+0」「0 天」这种无信息量的行
   * （§3.4 规则 1：无数据不渲染）。
   */
  checkin?: {
    todayCheckedIn: boolean;
    dailyCredit?: number | null;
    todayCredit?: number | null;
    totalCredits?: number | null;
    streakDays?: number | null;
    checkinDates?: string[] | null;
    /**
     * 状态说明（读不到的原因 / 无机器身份 / 本区域没有活动）。
     *
     * ★ 为什么要有它：`checkin` 过去只承载「有数据」的那一半，于是**读不到**时整个
     * 签到 tab 直接消失，用户只看到「读不到」三个字、没有任何可操作信息
     * （issue #66 的「无法签到」就是这么被感知的）。
     */
    notice?: string | null;
  } | null;
  /** 头部是否展示「当前登录」徽章（原型 :3475）。缺省不展示。 */
  current?: boolean;
  /** ★ 可点「切换到此账号」程序位；传空数组 / 不传 = 不渲染该区块（退回只读 `programs`）。 */
  switchTargets?: DetailSwitchTarget[];
  /** 「使用向导切换（可选迁移内容）」入口（原型 :3485）。 */
  onWizardSwitch?: () => void;
  /**
   * ★ P14 S4a · 打开「插件会话」抽屉（VS Code 插件 / CodeBuddy IDE 的会话仓）。
   *
   * ⚠️ 只在**该账号绑定了有会话仓的程序位**时注入（映射见 `lib/ext-sessions.ts`）——
   * JetBrains 等没有会话仓的程序位**不注入**，于是入口不渲染。
   * 渲染一个点了会去读错目录的按钮，比没有这个功能坏得多。
   */
  onExtSessions?: () => void;
  /** 入口文案里的客户端名（已 i18n）；`onExtSessions` 提供时才用得到。 */
  extClientLabel?: string;
  /** 「操作」区回调（原型 :3518-3527）。`null` / 不传 = 不渲染该区块。 */
  actions?: DetailActions | null;
  /** 签到历史（近 7 天）；`null` / 空数组 = 不渲染该区块。 */
  checkinHistory?: DetailCheckinHistoryRow[] | null;
  /**
   * 一次性福利包（如 Qoder 的 Pro 升级包 +1800）。
   *
   * ⚠️ **只在活动确实在线时才传**（`available=false` ⇒ 传 `null`）：
   * 摆一个点了必然失败的按钮，比没有这个功能坏得多。
   */
  proPack?: {
    title: string;
    description: string;
    /** 三种按钮文案由**调用方**给（本抽屉是共用组件，不含任何平台专属词条）。 */
    claimLabel: string;
    claimingLabel: string;
    claimedLabel: string;
    /** 已领 / 不可领时按钮变徽章。 */
    claimed: boolean;
    busy: boolean;
    onClaim: () => void;
  } | null;
}

function packTone(pack: DetailPack): string {
  if (pack.expired) return "text-destructive";
  if (pack.expiringSoon) return "text-warning";
  return "text-muted-foreground";
}

function programBadgeText(
  program: DetailProgramBadge,
  t: ReturnType<typeof useT>,
): string {
  if (program.current) return t("shared.drawer.program.current");
  if (!program.installed) return t("shared.drawer.program.missing");
  if (!program.hasDataDir) return t("shared.drawer.program.noDataDir");
  return t("shared.drawer.program.installed");
}

export function AccountDetailDrawer({
  open,
  onOpenChange,
  accountKey,
  name,
  subtitle,
  avatarSeed,
  loading = false,
  packs = [],
  packsNotice = null,
  stats = [],
  snapshots = [],
  refreshingKinds,
  onRefreshPart,
  sections = [],
  programs = [],
  remark,
  onSaveRemark,
  group,
  onSaveGroup,
  groups = [],
  featuresDisabled = false,
  autoEditRemark = false,
  autoEditGroup = false,
  empty,
  checkin,
  current = false,
  switchTargets = [],
  onWizardSwitch,
  onExtSessions,
  extClientLabel,
  actions = null,
  checkinHistory = null,
  proPack = null,
}: AccountDetailDrawerProps) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  /** 「刷新 Token」按钮的进行中态（回调是 fire-and-forget ⇒ 最小展示时长保证可见）。 */
  const { busy: refreshBusy, run: runRefresh } = useRefreshAction();
  const inputRef = useRef<HTMLInputElement | null>(null);
  /**
   * 取消编辑时置位，用来抵掉紧随其后的那次 blur。
   *
   * 编辑态结束时输入框会被卸载，浏览器 / React 可能仍补发一次 blur；
   * 不拦的话「按 Esc 取消」会被那次 blur 反向提交成一次保存。
   *
   * ⚠️ 卡片时代那个 `keepRemarkFocusRef` hack **不再需要**：它绕开的是「Radix 菜单关闭后
   * 把焦点还给触发按钮」的竞态，而编辑框当时位于菜单背后。编辑器搬进抽屉后
   * 不再处于菜单之后，那个竞态在结构上消失 —— 所以这里只有一道 skip 闸。
   */
  const skipCommitRef = useRef(false);
  /** 「本次打开是否已按 `autoEditRemark` 进入过编辑态」，避免保存后因依赖变化重复进入。 */
  const autoEditDoneRef = useRef(false);

  const savedRemark = displayText(remark)?.trim() || "";
  const canEdit = !featuresDisabled && Boolean(onSaveRemark);

  /* 分组编辑器：**与备注编辑器完全同构**（同一套 editing / draft / saving / autoEdit 语义），
     只是状态与渲染分开 —— 两个字段各自独立编辑，互不干扰。仅在调用方注入 `onSaveGroup`
     时渲染（未注入 = 不显示该区块，见 `Props.group` 的注释）。 */
  const showGroup = Boolean(onSaveGroup);
  const savedGroup = displayText(group)?.trim() || "";
  const canEditGroup = !featuresDisabled && showGroup;

  useEffect(() => {
    if (!editing) return;
    // 等 DOM 提交后再聚焦。
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [editing]);

  // 打开 / 关闭时同步编辑态：关闭一律收起并丢弃草稿（下次打开重新从已保存值起算）。
  useEffect(() => {
    if (!open) {
      autoEditDoneRef.current = false;
      setEditing(false);
      setDraft("");
      return;
    }
    if (autoEditRemark && canEdit && !autoEditDoneRef.current) {
      autoEditDoneRef.current = true;
      skipCommitRef.current = false;
      setDraft(savedRemark);
      setEditing(true);
    }
    /* 分组不再有「编辑态」：选择器自己管展开（`autoEditGroup` 直接透给它）。
       这一处只保留备注的自动进入编辑。 */
  }, [open, autoEditRemark, canEdit, savedRemark]);

  function beginEdit() {
    if (!canEdit || saving) return;
    skipCommitRef.current = false;
    setDraft(savedRemark);
    setEditing(true);
  }

  function cancelEdit() {
    skipCommitRef.current = true;
    setEditing(false);
    setDraft("");
  }

  async function commit() {
    if (skipCommitRef.current) {
      skipCommitRef.current = false;
      return;
    }
    if (!onSaveRemark || saving) return;
    if (draft.trim() === savedRemark) {
      cancelEdit();
      return;
    }
    setSaving(true);
    const saved = await onSaveRemark(draft);
    setSaving(false);
    if (saved) setEditing(false);
  }

  /* 操作区只在调用方**显式注入** `actions` 时出现 —— 未注入（如 Trae）时保持原样，
     不因为共享壳新增了一个「编辑备注」入口而改变另一平台的行为。 */
  /**
   * 操作区是否渲染。
   *
   * ⚠️ 只数**抽屉独有**的动作（刷新 Token / 导出 / 权限诊断）。
   * 手动签到 / 编辑备注 / 编辑分组 / 删除**不在这里** —— 它们已经在卡片的「更多」菜单里，
   * 同一个动作两处都有入口正是上一版被吐槽的「乱」（见 PLAN §3.3 与原型的分工表）。
   */
  const hasActions = Boolean(
    actions && (actions.onRefresh || actions.onExport || actions.onPermission),
  );
  const isEmpty =
    packs.length === 0 &&
    sections.length === 0 &&
    programs.length === 0 &&
    switchTargets.length === 0 &&
    (checkinHistory === null || checkinHistory.length === 0) &&
    !hasActions;

  /**
   * 本次**可用**的页签，按渲染顺序排列（issue #49）。
   *
   * 页签的「是否渲染」与「记住的值还可用吗」**共用这一份数组** —— 两处各写一遍条件
   * 是这类缺陷的经典来源：条件漂移后会出现「记住了但触发器不存在」的空页签。
   */
  const availableTabs: DrawerTab[] = ["overview"];
  if (packs.length > 0) availableTabs.push("credits");
  if (checkin || (checkinHistory && checkinHistory.length > 0)) availableTabs.push("checkin");
  if (hasActions) availableTabs.push("credential");
  if (snapshots.length > 0) availableTabs.push("snapshot");

  const [rememberedTab, rememberTab] = useDrawerTab(accountKey);
  // 记住的页签当前不可用 ⇒ 回落第一枚可用页签，且**不改写**记忆（R3 / R4）。
  const activeTab = resolveDrawerTab(rememberedTab, availableTabs);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" showCloseButton={false} className="w-full sm:max-w-md">
        <SheetClose asChild>
          <Button
            variant="ghost"
            size="icon"
            className="absolute top-3 right-3 z-10 size-8 rounded-lg text-muted-foreground hover:text-foreground"
            aria-label={t("shared.drawer.close")}
          >
            <XIcon />
          </Button>
        </SheetClose>

        <SheetHeader>
          <div className="flex items-center gap-3 pr-9">
            <div
              className={cn(
                "flex size-11 shrink-0 items-center justify-center rounded-[13px] text-base font-semibold ring-4 ring-white/65",
                avatarTone(avatarSeed),
              )}
            >
              {name.charAt(0).toUpperCase()}
            </div>
            <div className="min-w-0 flex-1">
              <SheetTitle className="truncate" title={name}>
                {name}
              </SheetTitle>
              <SheetDescription className="mt-0.5 truncate" title={subtitle}>
                {subtitle}
              </SheetDescription>
            </div>
            {/* 与卡片同口径的「当前登录」徽章（原型 :3475）。 */}
            {current && (
              <Badge variant="secondary" className="shrink-0 border-transparent bg-primary/15 text-primary">
                {t("shared.drawer.currentLogin")}
              </Badge>
            )}
          </div>
        </SheetHeader>

        {/* ⚠️ `[&>*]:shrink-0` 不是装饰：滚动容器是 flex 列，子元素默认可收缩，
            一旦内容超高，`<Tabs>`（内含很长的 TabContent）会被压扁 —— 它内部的
            TabsList 因此被裁成一条 8px 的缝（2026-10-18 用户截图）。
            让每个直接子元素都不收缩，超出的部分交给 `overflow-y-auto` 滚动。 */}
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4 [&>*]:shrink-0">
          {/* 指标块：一打开就知道「这个账号现在什么状态」，不用滚。
              每块右上角的 ⟳ 与卡片「更多」里的四项**同一个回调**（同源，不是第二套实现）。 */}
          {!loading && stats.length > 0 && (
            <div className="grid grid-cols-2 gap-2">
              {stats.map((stat) => {
                const busy = stat.refreshKind ? (refreshingKinds ?? []).includes(stat.refreshKind) : false;
                return (
                  <div
                    key={stat.key}
                    className="min-w-0 rounded-[10px] border border-border bg-muted/40 px-2.5 py-2"
                  >
                    <div className="flex items-center gap-1">
                      <span className="truncate text-[10.5px] text-muted-foreground">{stat.label}</span>
                      {stat.refreshKind && onRefreshPart && (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => onRefreshPart(stat.refreshKind as AccountRefreshKind)}
                          aria-label={stat.label}
                          className="ml-auto grid size-5 shrink-0 place-items-center rounded-md text-muted-foreground opacity-60 transition-colors hover:bg-accent hover:text-foreground hover:opacity-100 focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none disabled:opacity-60"
                        >
                          {busy ? (
                            <Loader2 className="size-3 animate-spin" />
                          ) : (
                            <RefreshCw className="size-3" />
                          )}
                        </button>
                      )}
                    </div>
                    <div className="mt-0.5 truncate text-base font-semibold tabular-nums">{stat.valueText}</div>
                    {stat.subText ? (
                      <div
                        className={cn(
                          "mt-0.5 truncate text-[10.5px]",
                          stat.tone === "warn"
                            ? "text-warning"
                            : stat.tone === "bad"
                              ? "text-destructive"
                              : "text-muted-foreground",
                        )}
                        title={stat.subText}
                      >
                        {stat.subText}
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}

          {/* 备注区：就地编辑，自包含（失败保持编辑态让用户改完重试）。 */}
          <section>
            <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
              {t("shared.drawer.remark.title")}
            </h3>
            {editing ? (
              <Input
                ref={inputRef}
                value={draft}
                disabled={saving}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void commit();
                  } else if (event.key === "Escape") {
                    event.preventDefault();
                    cancelEdit();
                  }
                }}
                onBlur={() => void commit()}
                maxLength={80}
                placeholder={t("shared.drawer.remark.placeholder")}
                spellCheck={false}
                autoComplete="off"
                aria-label={t("shared.drawer.remark.aria", { name })}
                className="h-8 w-full text-xs"
              />
            ) : savedRemark ? (
              canEdit ? (
                <button
                  type="button"
                  onClick={beginEdit}
                  title={savedRemark}
                  aria-label={t("shared.drawer.remark.editAria", { name })}
                  className="-mx-1 flex w-[calc(100%+0.5rem)] min-w-0 items-center gap-1.5 rounded-md px-1 text-left text-xs leading-5 text-foreground/80 transition-colors hover:bg-foreground/[0.04] focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none"
                >
                  <StickyNote className="size-3.5 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate">{savedRemark}</span>
                </button>
              ) : (
                <p className="flex min-w-0 items-center gap-1.5 text-xs leading-5 text-foreground/80">
                  <StickyNote className="size-3.5 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate" title={savedRemark}>
                    {savedRemark}
                  </span>
                </p>
              )
            ) : canEdit ? (
              <button
                type="button"
                onClick={beginEdit}
                className="-mx-1 flex items-center gap-1.5 rounded-md px-1 text-left text-xs leading-5 text-muted-foreground/70 transition-colors hover:bg-foreground/[0.04] hover:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none"
              >
                <Pencil className="size-3 shrink-0" aria-hidden="true" />
                {t("shared.drawer.remark.add")}
              </button>
            ) : (
              <p className="text-xs text-muted-foreground">—</p>
            )}
          </section>

          {/* 分组区：**选择器**（既有分组 + 就地新建），不是自由文本框。
              理由见 `account-group-picker.tsx` 的文件头：分组是自由文本，但让用户每次手打
              必然把同一个组写成三种；只给下拉又建不出新组。 */}
          {showGroup && (
            <section>
              <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                {t("shared.drawer.group.title")}
              </h3>
              <AccountGroupPicker
                value={savedGroup}
                groups={groups}
                disabled={!canEditGroup}
                autoOpen={autoEditGroup}
                accountName={name}
                onCommit={(next) => (onSaveGroup ? onSaveGroup(next) : Promise.resolve(false))}
              />
            </section>
          )}

          {/* ★ 切换到此账号（原型 :3480-3486）：可点程序位行 + 向导入口。
              仅在调用方注入 `switchTargets` 时渲染；未注入时下面的只读徽标区（Trae）保持原样。 */}
          {switchTargets.length > 0 && (
            <section>
              <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                {t("shared.drawer.switch.title")}
              </h3>
              <div className="flex flex-wrap gap-1.5">
                {switchTargets.map((target) => {
                  const enabled = Boolean(target.onSwitch) && !target.busy && !target.loading;
                  return (
                    <Button
                      key={target.key}
                      type="button"
                      variant={target.current ? "secondary" : "outline"}
                      size="sm"
                      className={cn(
                        "h-[26px] rounded-[8px] px-2 text-[11px]",
                        target.current && "border-primary/40 bg-primary/15 text-primary",
                        !target.available && "border-dashed text-muted-foreground",
                      )}
                      aria-pressed={target.current}
                      disabled={target.current || !enabled}
                      onClick={() => target.onSwitch?.()}
                      title={
                        target.current
                          ? t("shared.drawer.program.current")
                          : target.available
                            ? t("shared.drawer.switch.available")
                            : t("shared.drawer.program.missing")
                      }
                    >
                      {target.current ? <Check className="size-3" /> : null}
                      {target.label}
                      {!target.available ? <span className="opacity-70">（{t("shared.drawer.program.missing")}）</span> : null}
                    </Button>
                  );
                })}
              </div>
              {onExtSessions && (
                /* ★ P14 S4a · 插件会话复制的入口。
                   为什么在**这里**、不在「关联会话」页：那边是**副本组**模型
                   （有账本、可双向同步、有「谁落后 / 谁分叉」），而插件侧是
                   **一次性单向复制**（副本恒取新 id）⇒ 塞进去只会多出一个永远空的组列表。 */
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="ml-2 mt-2.5"
                  onClick={onExtSessions}
                >
                  <MessageSquare className="size-4" />
                  {t("sessions.ext.entry", { client: extClientLabel ?? "" })}
                </Button>
              )}
            </section>
          )}

          {loading ? (
            // 区块级加载态：**不是**按钮态，所以用骨架屏而不是 Loader2。
            <div className="space-y-3" aria-busy="true" aria-label={t("shared.drawer.loading")}>
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-14 w-full" />
              <Skeleton className="h-4 w-20" />
              <Skeleton className="h-14 w-full" />
            </div>
          ) : isEmpty ? (
            empty ? (
              <EmptyState
                icon={Info}
                title={empty.title}
                description={empty.description}
                action={empty.action}
              />
            ) : null
          ) : (
            /* 页签只切「看」的部分：概览（信息区块 + 程序位）/ 积分 / 签到 / 凭据。
               ⚠️ 备注、分组、程序位切换**留在页签之上** —— 它们是身份与操作，
               藏进页签会让「改个备注」变成「先猜在哪个页签里」。 */
            <Tabs
              value={activeTab}
              onValueChange={(value) => {
                // 只记**用户主动点击**的结果；`resolveDrawerTab` 的回落不写盘（R4）。
                if (isDrawerTab(value)) rememberTab(value);
              }}
              className="min-h-0"
            >
              {/* 页签最多五个（含「快照」），抽屉又窄 ⇒ 必须**不换行**：
                  `inline-flex` 默认允许折行，五个页签会在 300px 里挤成两排（快照孤零零掉到第二行）。
                  改成 nowrap + 横向滚动：一行排开，实在放不下时能滑，而不是折行。 */}
              <TabsList className="max-w-full flex-nowrap overflow-x-auto">
                {availableTabs.map((tab) => (
                  <TabsTrigger key={tab} value={tab}>
                    {t(`shared.drawer.tab.${tab}`)}
                  </TabsTrigger>
                ))}
              </TabsList>

              <TabsContent value="overview" className="space-y-4 pt-1">
{/* 信息区块（按序渲染）。 */}
              {sections.map((section) => (
                <section key={section.key}>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {section.title}
                  </h3>
                  {/* 原型 `.kv`（`demo.css:492-495`）：`flex gap10` + `12.5px` + `padding:7px 0`
      + **`border-bottom:1px dashed --border`**（虚线分隔，此前完全没有）+末行无边框；
      `.kv .k` 是**固定 `width:96px`** 的标签列（此前 `auto` 随内容伸缩 ⇒ 每行标签起点都在变）。*/}
                  <dl className="space-y-0">
                    {section.rows.map((row, index) => (
                      <div
                        key={`${section.key}-${index}`}
                        className={cn(
                          "grid min-w-0 grid-cols-[96px_minmax(0,1fr)] items-baseline gap-2.5 border-b border-dashed border-border py-1.5 text-[12.5px] last:border-b-0",
                        )}
                      >
                        <dt className="shrink-0 text-[11.5px] text-muted-foreground">{row.label}</dt>
                        <dd
                          className={cn(
                            "min-w-0 truncate text-right text-foreground/80",
                            row.mono && "font-mono tabular-nums",
                          )}
                          title={row.title ?? row.value}
                        >
                          {row.value}
                        </dd>
                      </div>
                    ))}
                  </dl>
                </section>
              ))}

              {/* 只读程序位徽标 —— **不是**切换控件（见文件头约束 2）。 */}
              {programs.length > 0 && (
                <section>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {t("shared.drawer.programs.title")}
                  </h3>
                  <div className="flex flex-wrap gap-1.5">
                    {programs.map((program) => (
                      <Badge
                        key={program.key}
                        variant={program.current ? "success" : "secondary"}
                        className={cn(!program.current && "text-muted-foreground")}
                      >
                        {program.label}
                        <span className="opacity-80">· {programBadgeText(program, t)}</span>
                      </Badge>
                    ))}
                  </div>
                </section>
              )}

                            {actions?.onExport && (
                <Button type="button" variant="outline" size="sm" onClick={actions.onExport}>
                  <Download className="size-4" />
                  {t("shared.drawer.actions.export")}
                </Button>
              )}
              </TabsContent>

              {packs.length > 0 && (
                <TabsContent value="credits" className="space-y-4 pt-1">
{/* 积分 / 资源包：文本已由调用方格式化，这里只排版。 */}
              {packs.length > 0 && (
                <section>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {t("shared.drawer.packs.title")}
                  </h3>
                  <div className="space-y-3">
                    {packs.map((pack) => (
                      <div key={pack.key} className="min-w-0">
                        <div className="flex min-w-0 items-start justify-between gap-3">
                          <span className="min-w-0 truncate text-xs font-medium" title={pack.name}>
                            {pack.name}
                          </span>
                          <span className={cn("shrink-0 text-[11px] tabular-nums", packTone(pack))}>
                            {pack.expiryText}
                          </span>
                        </div>
                        <div className="mt-1 flex items-baseline justify-between gap-3 text-[11px]">
                          <span className="font-medium tabular-nums">
                            {pack.remainingText} / {pack.totalText}
                          </span>
                          <span className="text-muted-foreground tabular-nums">{pack.usedText}</span>
                        </div>
                        <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted" aria-hidden="true">
                          <div
                            className={cn(
                              "h-full rounded-full",
                              pack.expired ? "bg-destructive/60" : pack.expiringSoon ? "bg-warning/80" : "bg-primary/75",
                            )}
                            style={{ width: `${Math.min(100, Math.max(0, pack.ratio))}%` }}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                  {packsNotice ? (
                    <div className="mt-3 rounded-md border border-border/60 bg-muted/30 px-2.5 py-2">
                      <p className="text-[11.5px] leading-relaxed text-muted-foreground">
                        {packsNotice.text}
                      </p>
                      {packsNotice.actionLabel && packsNotice.onAction ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          className="mt-1.5 h-7"
                          disabled={packsNotice.busy}
                          onClick={packsNotice.onAction}
                        >
                          {packsNotice.actionLabel}
                        </Button>
                      ) : null}
                    </div>
                  ) : null}
                </section>
              )}

                              </TabsContent>
              )}

              {(checkin || (checkinHistory && checkinHistory.length > 0)) && (
                <TabsContent value="checkin" className="space-y-4 pt-1">
              {/* ★ 一次性福利包（Pro 升级包）：活动在线时才渲染；已领则显示徽章。 */}
              {proPack && (
                <section
                  className="flex items-center justify-between gap-3 rounded-md border border-border/60 bg-muted/30 px-3 py-2"
                  data-act="pro-pack"
                >
                  <div className="min-w-0">
                    <p className="text-[12.5px] font-medium">{proPack.title}</p>
                    <p className="text-[11.5px] text-muted-foreground">{proPack.description}</p>
                  </div>
                  {proPack.claimed ? (
                    <Badge variant="success" className="shrink-0">
                      {proPack.claimedLabel}
                    </Badge>
                  ) : (
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="shrink-0"
                      disabled={proPack.busy}
                      onClick={proPack.onClaim}
                    >
                      {proPack.busy ? proPack.claimingLabel : proPack.claimLabel}
                    </Button>
                  )}
                </section>
              )}

              {/* ★ issue #66：状态说明 —— 「读不到」必须有原因，不能让签到 tab 静默消失。 */}
              {checkin?.notice && (
                <p className="rounded-md border border-border/60 bg-muted/40 px-2.5 py-2 text-[11.5px] leading-relaxed text-muted-foreground">
                  {checkin.notice}
                </p>
              )}
{/* ★ B5：签到积分（**有数据才渲染** —— 不摆「+0」「0 天」这类无信息量的行）。 */}
              {checkin?.todayCheckedIn && (
                <section>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {t("shared.drawer.checkin.title")}
                  </h3>
                  <div className="space-y-1.5 text-xs">
                    {/* ★ 原型 B5 要求：抽屉里能看到「+100」（Trae 为 +50）。 */}
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-success">
                        +{checkin.todayCredit ?? checkin.dailyCredit ?? 0}
                      </span>
                      <span className="text-muted-foreground">
                        {t("shared.drawer.checkin.today")}
                      </span>
                    </div>
                    {typeof checkin.streakDays === "number" && checkin.streakDays > 0 && (
                      <div className="text-muted-foreground">
                        {t("shared.drawer.checkin.streak", { n: checkin.streakDays })}
                      </div>
                    )}
                    {typeof checkin.totalCredits === "number" && (
                      <div className="text-muted-foreground">
                        {t("shared.drawer.checkin.total", { n: checkin.totalCredits })}
                      </div>
                    )}
                    {/* 逐条签到日期（倒序；上游 `checkin_dates`） */}
                    {Array.isArray(checkin.checkinDates) && checkin.checkinDates.length > 0 && (
                      <ul className="flex flex-wrap gap-1 pt-0.5">
                        {checkin.checkinDates.map((d) => (
                          <li
                            key={d}
                            className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                          >
                            {d}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                </section>
              )}

              {/* ★ 签到历史（近 7 天）：日期 / 结果 / 获得积分（原型 checkinHistoryHtml :3428）。
                  日期 + 结果来自 `getCheckinLogs()`；获得积分来自积分统计的签到事件（账号 + 日期），
                  取不到时渲染「—」（绝不臆造固定奖励）。 */}
              {checkinHistory && checkinHistory.length > 0 && (
                <section>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {t("shared.drawer.checkin.historyTitle")}
                  </h3>
                  <div className="overflow-hidden rounded-lg border border-border">
                    <table className="w-full border-collapse text-[12px]">
                      <thead>
                        <tr className="border-b border-border text-muted-foreground">
                          <th scope="col" className="px-2.5 py-1.5 text-left text-[11px] font-medium">
                            {t("shared.drawer.checkin.historyDate")}
                          </th>
                          <th scope="col" className="px-2.5 py-1.5 text-left text-[11px] font-medium">
                            {t("shared.drawer.checkin.historyResult")}
                          </th>
                          <th scope="col" className="px-2.5 py-1.5 text-right text-[11px] font-medium">
                            {t("shared.drawer.checkin.historyAmount")}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        {checkinHistory.map((row) => (
                          <tr key={row.date} className="border-b border-border/60 last:border-0">
                            <td className="whitespace-nowrap px-2.5 py-1.5 tabular-nums">{row.date}</td>
                            <td className="px-2.5 py-1.5">
                              <Badge
                                variant={
                                  row.tone === "ok"
                                    ? "success"
                                    : row.tone === "fail"
                                      ? "destructive"
                                      : "secondary"
                                }
                                className={cn("rounded-md px-1.5 py-0 text-[11px] font-medium", row.tone === "none" && "text-muted-foreground")}
                              >
                                {row.resultLabel}
                              </Badge>
                            </td>
                            <td className="px-2.5 py-1.5 text-right tabular-nums">
                              {row.amount === null ? "—" : `+${formatCredits(row.amount)}`}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </section>
              )}

                              </TabsContent>
              )}

              {hasActions && (
                <TabsContent value="credential" className="space-y-4 pt-1">
{/* ★ 操作区（原型 :3518-3527）：刷新 Token / 手动签到 / 编辑备注 / 导出 / 权限诊断 / 删除。
                  每枚按钮都接**页面既有**的处理函数；缺回调就不渲染该按钮（不摆点不动的灰按钮）。
                  编辑备注复用抽屉内部就地编辑器（`beginEdit`）。 */}
              {hasActions && (
                <section>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {t("shared.drawer.tab.credential")}
                  </h3>
                  <div className="flex flex-wrap gap-2">
                    {actions?.onRefresh && (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        disabled={refreshBusy}
                        onClick={() => void runRefresh(() => actions.onRefresh?.())}
                      >
                        {refreshBusy ? (
                          <Loader2 className="size-4 animate-spin" />
                        ) : (
                          <RefreshCw className="size-4" />
                        )}
                        {t("shared.drawer.actions.refresh")}
                      </Button>
                    )}
                    {actions?.onPermission && (
                      <Button type="button" variant="outline" size="sm" onClick={actions.onPermission}>
                        <ShieldCheck className="size-4" />
                        {t("shared.drawer.actions.permission")}
                      </Button>
                    )}
                  </div>
                  {actions?.credentialUnavailable && (
                    <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
                      {t("shared.drawer.actions.credentialNote")}
                    </p>
                  )}
                </section>
              )}
                </TabsContent>
              )}
              {snapshots.length > 0 && (
                <TabsContent value="snapshot" className="space-y-4 pt-1">
                  <section>
                    <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                      {t("shared.drawer.snapshot.title")}
                    </h3>
                    <div className="overflow-hidden rounded-lg border border-border">
                      <table className="w-full text-[11.5px]">
                        <thead className="bg-muted/40 text-muted-foreground">
                          <tr>
                            <th className="px-2.5 py-1.5 text-left font-normal">
                              {t("shared.drawer.snapshot.time")}
                            </th>
                            <th className="px-2.5 py-1.5 text-right font-normal">
                              {t("shared.drawer.snapshot.size")}
                            </th>
                          </tr>
                        </thead>
                        <tbody>
                          {snapshots.map((snapshot) => (
                            <tr key={snapshot.key} className="border-t border-border">
                              <td className="px-2.5 py-1.5 tabular-nums">
                                {snapshot.timeText}
                                <span className="ml-2 text-muted-foreground">{snapshot.fileCountText}</span>
                              </td>
                              <td className="px-2.5 py-1.5 text-right tabular-nums">{snapshot.sizeText}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </section>
                </TabsContent>
              )}
            </Tabs>
          )}
        </div>

        {/* 固定底栏：全抽屉**唯一**的流程出口。
            放在滚动区外，永远在视野内 —— 上一版它在正文最下面，长列表时要滚到底才找得到。 */}
        {onWizardSwitch && (
          <div className="shrink-0 border-t border-border p-4">
            <Button type="button" className="w-full" onClick={onWizardSwitch}>
              <ArrowRight className="size-4" />
              {t("shared.drawer.switch.wizard")}
            </Button>
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}
