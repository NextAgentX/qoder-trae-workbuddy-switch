import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowRight, Check, Download, Info, Pencil, RefreshCw, ShieldCheck, Sparkles, StickyNote, Tag, Trash2, XIcon } from "lucide-react";

import { avatarTone, formatCredits } from "@/components/account-card";
import { EmptyState } from "@/components/ui/empty-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
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
 * 2. **不放「看起来能切」的第二个控件**。抽屉是纯展示 + 备注就地编辑；程序位只以
 *    **只读徽标**呈现，绝不渲染切换按钮（区域 / 程序位的切换属于列表页）。
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
  } | null;
  /** 头部是否展示「当前登录」徽章（原型 :3475）。缺省不展示。 */
  current?: boolean;
  /** ★ 可点「切换到此账号」程序位；传空数组 / 不传 = 不渲染该区块（退回只读 `programs`）。 */
  switchTargets?: DetailSwitchTarget[];
  /** 「使用向导切换（可选迁移内容）」入口（原型 :3485）。 */
  onWizardSwitch?: () => void;
  /** 「操作」区回调（原型 :3518-3527）。`null` / 不传 = 不渲染该区块。 */
  actions?: DetailActions | null;
  /** 签到历史（近 7 天）；`null` / 空数组 = 不渲染该区块。 */
  checkinHistory?: DetailCheckinHistoryRow[] | null;
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
  name,
  subtitle,
  avatarSeed,
  loading = false,
  packs = [],
  sections = [],
  programs = [],
  remark,
  onSaveRemark,
  group,
  onSaveGroup,
  featuresDisabled = false,
  autoEditRemark = false,
  autoEditGroup = false,
  empty,
  checkin,
  current = false,
  switchTargets = [],
  onWizardSwitch,
  actions = null,
  checkinHistory = null,
}: AccountDetailDrawerProps) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
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
  const [editingGroup, setEditingGroup] = useState(false);
  const [groupDraft, setGroupDraft] = useState("");
  const [savingGroup, setSavingGroup] = useState(false);
  const groupInputRef = useRef<HTMLInputElement | null>(null);
  const skipGroupCommitRef = useRef(false);
  const autoEditGroupDoneRef = useRef(false);
  const showGroup = Boolean(onSaveGroup);
  const savedGroup = displayText(group)?.trim() || "";
  const canEditGroup = !featuresDisabled && showGroup;

  useEffect(() => {
    if (!editing) return;
    // 等 DOM 提交后再聚焦。
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [editing]);

  useEffect(() => {
    if (!editingGroup) return;
    const timer = window.setTimeout(() => groupInputRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [editingGroup]);

  // 打开 / 关闭时同步编辑态：关闭一律收起并丢弃草稿（下次打开重新从已保存值起算）。
  useEffect(() => {
    if (!open) {
      autoEditDoneRef.current = false;
      setEditing(false);
      setDraft("");
      autoEditGroupDoneRef.current = false;
      setEditingGroup(false);
      setGroupDraft("");
      return;
    }
    if (autoEditRemark && canEdit && !autoEditDoneRef.current) {
      autoEditDoneRef.current = true;
      skipCommitRef.current = false;
      setDraft(savedRemark);
      setEditing(true);
    }
    if (autoEditGroup && canEditGroup && !autoEditGroupDoneRef.current) {
      autoEditGroupDoneRef.current = true;
      skipGroupCommitRef.current = false;
      setGroupDraft(savedGroup);
      setEditingGroup(true);
    }
  }, [open, autoEditRemark, canEdit, savedRemark, autoEditGroup, canEditGroup, savedGroup]);

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

  function beginGroupEdit() {
    if (!canEditGroup || savingGroup) return;
    skipGroupCommitRef.current = false;
    setGroupDraft(savedGroup);
    setEditingGroup(true);
  }

  function cancelGroupEdit() {
    skipGroupCommitRef.current = true;
    setEditingGroup(false);
    setGroupDraft("");
  }

  async function commitGroup() {
    if (skipGroupCommitRef.current) {
      skipGroupCommitRef.current = false;
      return;
    }
    if (!onSaveGroup || savingGroup) return;
    if (groupDraft.trim() === savedGroup) {
      cancelGroupEdit();
      return;
    }
    setSavingGroup(true);
    const saved = await onSaveGroup(groupDraft);
    setSavingGroup(false);
    if (saved) setEditingGroup(false);
  }

  /* 操作区只在调用方**显式注入** `actions` 时出现 —— 未注入（如 Trae）时保持原样，
     不因为共享壳新增了一个「编辑备注」入口而改变另一平台的行为。 */
  const hasActions = Boolean(
    actions &&
      (canEdit ||
        canEditGroup ||
        actions.onRefresh ||
        actions.onCheckin ||
        actions.onExport ||
        actions.onPermission ||
        actions.onDelete),
  );
  const isEmpty =
    packs.length === 0 &&
    sections.length === 0 &&
    programs.length === 0 &&
    switchTargets.length === 0 &&
    (checkinHistory === null || checkinHistory.length === 0) &&
    !hasActions;

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

        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
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

          {/* 分组区：与备注区**同构**的就地编辑（仅在注入 `onSaveGroup` 时渲染，见 `Props.group`）。 */}
          {showGroup && (
            <section>
              <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                {t("shared.drawer.group.title")}
              </h3>
              {editingGroup ? (
                <Input
                  ref={groupInputRef}
                  value={groupDraft}
                  disabled={savingGroup}
                  onChange={(event) => setGroupDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void commitGroup();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      cancelGroupEdit();
                    }
                  }}
                  onBlur={() => void commitGroup()}
                  maxLength={40}
                  placeholder={t("shared.drawer.group.placeholder")}
                  spellCheck={false}
                  autoComplete="off"
                  aria-label={t("shared.drawer.group.aria", { name })}
                  className="h-8 w-full text-xs"
                />
              ) : savedGroup ? (
                canEditGroup ? (
                  <button
                    type="button"
                    onClick={beginGroupEdit}
                    title={savedGroup}
                    aria-label={t("shared.drawer.group.editAria", { name })}
                    className="-mx-1 flex w-[calc(100%+0.5rem)] min-w-0 items-center gap-1.5 rounded-md px-1 text-left text-xs leading-5 text-foreground/80 transition-colors hover:bg-foreground/[0.04] focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none"
                  >
                    <Tag className="size-3.5 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" />
                    <span className="min-w-0 flex-1 truncate">{savedGroup}</span>
                  </button>
                ) : (
                  <p className="flex min-w-0 items-center gap-1.5 text-xs leading-5 text-foreground/80">
                    <Tag className="size-3.5 shrink-0 stroke-[1.75] text-muted-foreground" aria-hidden="true" />
                    <span className="min-w-0 flex-1 truncate" title={savedGroup}>
                      {savedGroup}
                    </span>
                  </p>
                )
              ) : canEditGroup ? (
                <button
                  type="button"
                  onClick={beginGroupEdit}
                  className="-mx-1 flex items-center gap-1.5 rounded-md px-1 text-left text-xs leading-5 text-muted-foreground/70 transition-colors hover:bg-foreground/[0.04] hover:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none"
                >
                  <Pencil className="size-3 shrink-0" aria-hidden="true" />
                  {t("shared.drawer.group.add")}
                </button>
              ) : (
                /* 不可编辑时的兜底（与备注区同款「—」；实际 WB 恒可编辑，此分支仅在 featuresDisabled 时命中）。 */
                <p className="text-xs text-muted-foreground">—</p>
              )}
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
              {onWizardSwitch && (
                <Button type="button" size="sm" className="mt-2.5" onClick={onWizardSwitch}>
                  <ArrowRight className="size-4" />
                  {t("shared.drawer.switch.wizard")}
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
            <>
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
                </section>
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

              {/* 信息区块（按序渲染）。 */}
              {sections.map((section) => (
                <section key={section.key}>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {section.title}
                  </h3>
                  <dl className="space-y-1.5">
                    {section.rows.map((row, index) => (
                      <div
                        key={`${section.key}-${index}`}
                        className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-baseline gap-3 text-xs"
                      >
                        <dt className="shrink-0 text-muted-foreground">{row.label}</dt>
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

              {/* ★ 操作区（原型 :3518-3527）：刷新 Token / 手动签到 / 编辑备注 / 导出 / 权限诊断 / 删除。
                  每枚按钮都接**页面既有**的处理函数；缺回调就不渲染该按钮（不摆点不动的灰按钮）。
                  编辑备注复用抽屉内部就地编辑器（`beginEdit`）。 */}
              {hasActions && (
                <section>
                  <h3 className="mb-2 text-[11.5px] font-medium text-muted-foreground">
                    {t("shared.drawer.actions.title")}
                  </h3>
                  <div className="flex flex-wrap gap-2">
                    {actions?.onRefresh && (
                      <Button type="button" variant="outline" size="sm" onClick={actions.onRefresh}>
                        <RefreshCw className="size-4" />
                        {t("shared.drawer.actions.refresh")}
                      </Button>
                    )}
                    {actions?.showCheckin && actions.onCheckin && (
                      <Button type="button" variant="outline" size="sm" onClick={actions.onCheckin}>
                        <Sparkles className="size-4" />
                        {t("shared.drawer.actions.checkin")}
                      </Button>
                    )}
                    {canEdit && (
                      <Button type="button" variant="outline" size="sm" onClick={beginEdit}>
                        <StickyNote className="size-4" />
                        {t("shared.drawer.actions.editRemark")}
                      </Button>
                    )}
                    {canEditGroup && (
                      <Button type="button" variant="outline" size="sm" onClick={beginGroupEdit}>
                        <Tag className="size-4" />
                        {t("shared.drawer.actions.editGroup")}
                      </Button>
                    )}
                    {actions?.onExport && (
                      <Button type="button" variant="outline" size="sm" onClick={actions.onExport}>
                        <Download className="size-4" />
                        {t("shared.drawer.actions.export")}
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
                  {actions?.onDelete && (
                    <Button type="button" variant="destructive" size="sm" className="mt-2.5" onClick={actions.onDelete}>
                      <Trash2 className="size-4" />
                      {t("shared.drawer.actions.delete")}
                    </Button>
                  )}
                </section>
              )}
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
