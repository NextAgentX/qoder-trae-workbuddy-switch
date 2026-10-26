/**
 * Qoder 账号卡片。
 *
 * 结构与 WorkBuddy / Trae 的账号卡**一一对应**（同样的 header / chips / body / footer
 * 四块、同样的「勾选 → 整卡可点开详情 → 页脚更多菜单」交互、同样的头像色板与几何）。
 *
 * ## 交互边界（原型 `accountCard()` 的纪律：**卡片不堆按钮**）
 *
 * 原型的「更多操作」在弹出层里明说：*「操作都在账号抽屉里 —— 点账号卡打开抽屉即可操作；
 * 刷新 Token / 手动签到 / 编辑备注 / 删除 —— 卡片不再堆按钮」*。
 * ⇒ 本卡片的**常规可点区只有三处**：勾选框 / 整卡（打开详情）/ 页脚「更多」菜单；
 * **备注编辑与删除都不在卡上**（它们住在 `account-detail-drawer.tsx` 里）。
 *
 * ## 与另两个平台卡的差异只有数据
 *
 * | 块 | WB / Trae | Qoder（本文件） |
 * |:--|:--|:--|
 * | header | 头像 + 昵称 + 身份 + 「当前登录」徽章 | 同形；**无**「当前登录」（程序位尚未落地） |
 * | chips | 使用中 N 工具 / 状态 / 临期 / 优先 | 状态槽（凭据可用 / 凭据未识别 / 无可用凭据）+ 归属异常 + 来源徽章 |
 * | body | 指标行（积分 / 今日 Token / 最后使用） | 凭据摘要 + 访问令牌掩码（积分与用量**没有数据源**，不摆空指标） |
 * | footer | 程序位行 + 更多菜单 | 仅更多菜单（程序位未落地 ⇒ 不渲染，落地后按原型补 `.progs` 行） |
 *
 * ## 三个刻意的取舍（都有出处）
 *
 * 1. **状态槽恒有一枚**（`凭据可用` / `凭据未识别` / `无可用凭据`）—— 与另两平台的
 *    状态槽同义（WB 的「已签到」/ Trae 的默认成功徽章都是恒有一枚）；「归属异常」
 *    是**正交**的独立徽章（正常时不渲染 —— 与 `travelChip` 的「默认态不渲染」同源）。
 * 2. **不渲染程序位胶囊**：Qoder 的两处官方凭据存储（桌面版 `auth.v1.dat` / CLI
 *    `~/.qoder/.auth/user`）对应的切换能力**尚未落地**（没有命令、没有写盘实现）——
 *    按「缺的就不渲染」纪律留空，绝不摆点不动的灰胶囊。
 * 3. **不渲染 `expiresAt`**：该字段单位未取证（见 `QoderAccountsView` 的说明），
 *    按毫秒渲染会把秒值显示成 1970 = 谎报。
 */

import {
  Check,
  CircleCheck,
  Download,
  Ellipsis,
  FolderTree,
  Info,
  Sparkles,
  StickyNote,
  Trash2,
} from "lucide-react";

import { AVATAR_TEXT_CLASS, accountInitials, avatarTone } from "@/components/account-card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DemoAction } from "@/components/demo-action";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  MenuGroupLabel,
  MenuHead,
  MenuNote,
  MenuStateItem,
  RefreshMenuGroup,
} from "@/components/account-menu-parts";
import type { AccountRefreshKind } from "@/lib/account-refresh";
import { shouldOpenAccountDetail } from "@/lib/account-open-guard";
import type { QoderAccountView, QoderProgramStatus } from "@/lib/api";
import { demoModeEnabled } from "@/lib/demo-mode";
import { displayText } from "@/lib/display-text";
import { useT, type Translate } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { TranslationKey } from "@/locales/zh";

/* 与 `account-card.tsx` 同一处理：几何交给 `Badge` 基础样式（已逐值对齐原型 `.badge`），
   这里只抵消它的 1px 透明边框（原型 `.badge` 没有边框）。 */
const CHIP_CLASS = "border-0 px-[7px] font-medium";

/**
 * 来源 id → 词条键（id 是**后端数据**，见 `qoder::account::SOURCE_*`）。
 *
 * ⚠️ 与 `companion-row.tsx` 的 `SOURCE_LABEL_KEY` 同一手法：**表外的新取值原样显示**，
 * 不隐藏也不猜 —— 后端加了新来源时，界面宁可显示 `mystery-source` 也不要显示空白。
 */
const SOURCE_LABEL_KEY: Record<string, TranslationKey> = {
  pat: "qoder.page.accounts.origin.pat",
  "desktop-app": "qoder.page.accounts.origin.desktopApp",
  cli: "qoder.page.accounts.origin.cli",
  oauth: "qoder.page.accounts.origin.oauth",
};

/** 来源的展示名（表外取值原样回落；空值给 `null` 让调用方决定省不省这一行）。 */
export function qoderSourceLabel(t: Translate, source: string | null | undefined): string | null {
  if (!source) return null;
  const key = SOURCE_LABEL_KEY[source];
  return key ? t(key) : source;
}

/**
 * 账号展示名（卡片 / 表格 / 抽屉 / toast / 无障碍名**同一份回落链**，避免各区各拼一遍）。
 *
 * 回落顺序与另两平台同一纪律：昵称 → 名字 → uid → `—`；**绝不渲染对象**
 * （脏值会让 React 卸整棵树 ⇒ 白屏，issue #2）。
 */
export function qoderAccountName(account: QoderAccountView): string {
  return displayText(account.nickname) || displayText(account.name) || displayText(account.uid) || "—";
}

/**
 * 账号的**状态徽章**（卡片 chips 行与表格「状态」列**同一份实现**，避免两处口径漂移）。
 *
 * 优先级（顺序即优先级，与 WB 侧同一纪律）：**凭据问题 > 签到态 > 正常**。
 * 凭据坏了的账号，「今天签没签到」是次要信息 —— 它根本调不动上游；反过来，
 * 把「今日未签」盖在「无可用凭据」上会让用户去点一个必然失败的签到按钮。
 *
 * ⚠️ `checkinToday` 的**三态**必须区分：
 * - `true` / `false` = 上游明确答复了；
 * - `null` / `undefined` = **还没查到**（额度查询在途、或该区域没有签到接口）。
 *   ⇒ 落回「凭据可用」而不是硬报「今日未签」—— 后者是**把未知说成已知**。
 */
export function QoderStatusChips({
  account,
  checkinToday,
}: {
  account: QoderAccountView;
  /** 三态：`true` 已签 / `false` 未签 / `null|undefined` 未知。 */
  checkinToday?: boolean | null;
}) {
  const t = useT();
  if (!account.hasCredential) {
    return (
      <Badge variant="destructive" className={CHIP_CLASS}>
        {t("qoder.page.accounts.chip.noCredential")}
      </Badge>
    );
  }
  if (account.credential.unrecognized) {
    return (
      <Badge variant="warning" className={CHIP_CLASS}>
        {t("qoder.page.accounts.chip.unrecognized")}
      </Badge>
    );
  }
  if (checkinToday === true) {
    return (
      <Badge variant="success" className={CHIP_CLASS}>
        {t("qoder.page.accounts.chip.checkedIn")}
      </Badge>
    );
  }
  if (checkinToday === false) {
    return (
      <Badge variant="warning" className={CHIP_CLASS}>
        {t("qoder.page.accounts.chip.notCheckedIn")}
      </Badge>
    );
  }
  return (
    <Badge variant="success" className={CHIP_CLASS}>
      {t("qoder.page.accounts.chip.ok")}
    </Badge>
  );
}

/** 「归属异常」徽章（与状态槽**正交**的独立一枚；正常时不渲染）。 */
export function QoderForeignBadge({ account }: { account: QoderAccountView }) {
  const t = useT();
  if (!account.domainMismatch) return null;
  return (
    <Badge variant="warning" className={CHIP_CLASS} title={t("qoder.page.accounts.chip.foreignTip")}>
      {t("qoder.page.accounts.chip.foreign")}
    </Badge>
  );
}

/** 一条账号的额度快照（来自 `qoder_credits`；取不到就是 `null`，**不编 0**）。 */
/** 程序位四态 → 展示文案（四态是四种不同的用户动作，压成一个布尔会给错引导）。 */
export function stateLabel(t: Translate, state: string): string {
  switch (state) {
    case "ready":
      return t("qoder.page.accounts.program.switchTo");
    case "unreadable":
      return t("qoder.page.accounts.program.unreadable");
    case "empty":
      return t("qoder.page.accounts.program.empty");
    default:
      return t("qoder.page.accounts.program.missing");
  }
}

export interface QoderCardCredits {
  remaining: number;
  total: number;
  plan?: string | null;
}

/**
 * 账号「更多」菜单（**卡片与表格共用同一份**）。
 *
 * ## 为什么抽出来
 *
 * 与 WB 的 `AccountMoreMenu` 同一条纪律：表格视图与卡片视图列的是同一批账号、同一批动作。
 * 此前 Qoder 的表格**没有任何行尾动作**（`renderActions` 没接），用户在表格里只能点行进详情 ——
 * 「同一能力两种形态」的典型分叉。抽成组件后，菜单结构与门控只有一处定义。
 *
 * ## 三态签到（F5，与 WB / Trae 同口径）
 *
 * 已签到 → 状态行；凭据不可用 → 「需先登录」状态行；其余 → 可点入口。
 * ⚠️ Qoder 的签到态**经常是未知**（只有区域快照 + 手动刷新）—— 未知时**保留可点项**
 * 并说明「点一下即向上游确认」，否则菜单里永远看不到签到入口（用户实际报障）。
 */
export interface QoderAccountMenuProps {
  account: QoderAccountView;
  /** 展示名（已过 `displayText`）。 */
  name: string;
  /** 今日签到三态：`true` 已签 / `false` 未签 / `null|undefined` 未知。 */
  checkinToday?: boolean | null;
  onOpenDetail?: (account: QoderAccountView) => void;
  onEditRemark?: (account: QoderAccountView) => void;
  onSetGroup?: (account: QoderAccountView) => void;
  /** 手动签到（与批量 / 抽屉同一条路径）。 */
  onCheckin?: (account: QoderAccountView) => void;
  onExport?: (account: QoderAccountView) => void;
  onDelete?: (account: QoderAccountView) => void;
  onRefreshPart?: (kind: AccountRefreshKind) => void;
  refreshingKinds?: readonly AccountRefreshKind[];
  doneKinds?: readonly AccountRefreshKind[];
  refreshKinds?: readonly AccountRefreshKind[];
  /** 表格里用更紧凑的触发器（与 WB 的 `compact` 同形）。 */
  compact?: boolean;
}

export function QoderAccountMenu({
  account,
  name,
  checkinToday,
  onOpenDetail,
  onEditRemark,
  onSetGroup,
  onCheckin,
  onExport,
  onDelete,
  onRefreshPart,
  refreshingKinds,
  doneKinds,
  refreshKinds,
  compact = false,
}: QoderAccountMenuProps) {
  const t = useT();
  /* 凭据不可用（缺失 / 不认识）⇒ 签到项只能显示「需先登录」，不给可点入口。
     与 `QoderStatusChips` 的优先级同源（凭据问题 > 签到态）。 */
  const credentialDead = !account.hasCredential || account.credential.unrecognized;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className={cn(
            "text-muted-foreground hover:bg-accent hover:text-foreground",
            compact ? "!size-7 !px-0" : "!px-[10px]",
          )}
          aria-label={t("qoder.page.accounts.card.manageAria", { name })}
          title={t("qoder.page.accounts.card.moreActions")}
        >
          <Ellipsis className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <MenuHead>{displayText(account.name) || account.uid}</MenuHead>
        <DropdownMenuItem disabled={!onOpenDetail} onSelect={() => onOpenDetail?.(account)}>
          <Info />
          {t("qoder.page.accounts.card.viewDetail")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {onRefreshPart && (
          <>
            <RefreshMenuGroup refreshingKinds={refreshingKinds} doneKinds={doneKinds} kinds={refreshKinds} onPick={onRefreshPart} />
            <DropdownMenuSeparator />
            <MenuGroupLabel>{t("shared.cardMenu.accountGroup")}</MenuGroupLabel>
          </>
        )}
        {/* ★ 签到是**三态**（与 WB / Trae 卡同一规则，原型 `acctMenuInner` 同形）：
            已签到 / 需先登录 / 可点。凭据坏了的账号不给可点项 —— 点了必然失败。
            ⚠️ 与 WB 的差异：Qoder 的签到状态**常常是「未知」**（只有区域快照 + 手动刷新），
            若照 WB「未知就不渲染」，菜单里就永远看不到签到入口（用户实际报障）。
            因此未知时**保留可点项**，并在 title 里说清「点一下即向上游确认」。 */}
        {checkinToday === true ? (
          <MenuStateItem
            icon={<Check className="size-4 text-primary" />}
            hint={t("shared.cardMenu.checkedTodayHint")}
          >
            {t("shared.cardMenu.checkedToday")}
          </MenuStateItem>
        ) : credentialDead ? (
          <MenuStateItem
            icon={<Sparkles />}
            hint={t("shared.cardMenu.needLoginHint")}
            state={t("shared.cardMenu.needLogin")}
          >
            {t("qoder.page.accounts.card.checkin")}
          </MenuStateItem>
        ) : (
          <DropdownMenuItem
            disabled={!onCheckin}
            title={
              checkinToday === false
                ? undefined
                : t("qoder.page.accounts.card.checkinUnknownHint")
            }
            onSelect={() => onCheckin?.(account)}
          >
            <CircleCheck />
            {t("qoder.page.accounts.card.checkin")}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem disabled={!onExport} onSelect={() => onExport?.(account)}>
          <Download />
          {t("shared.cardMenu.export")}
        </DropdownMenuItem>
        <DropdownMenuItem disabled={!onEditRemark} onSelect={() => onEditRemark?.(account)}>
          <StickyNote />
          {t("qoder.page.accounts.card.editRemark")}
        </DropdownMenuItem>
        <DropdownMenuItem disabled={!onSetGroup} onSelect={() => onSetGroup?.(account)}>
          <FolderTree />
          {t("qoder.page.accounts.card.setGroup")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {/* 删除是**危险动作**（虽可恢复，但会移出账号库）⇒ 落到页面统一弹确认。 */}
        <DropdownMenuItem
          className="text-destructive focus:bg-destructive/5 focus:text-destructive"
          onSelect={() => onDelete?.(account)}
        >
          <Trash2 />
          {t("qoder.page.accounts.card.deleteAccount")}
        </DropdownMenuItem>
              <MenuNote>{t("shared.cardMenu.note")}</MenuNote>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export interface QoderAccountCardProps {
  account: QoderAccountView;
  /** 是否被勾选（批量操作）。 */
  selected?: boolean;
  /** 切换勾选。 */
  onToggleSelect?: (account: QoderAccountView) => void;
  /** 打开详情抽屉（抽屉状态由页面持有）。 */
  onOpenDetail?: (account: QoderAccountView) => void;
  /** 「更多 → 编辑备注」：打开抽屉并进入编辑态。 */
  onEditRemark?: (account: QoderAccountView) => void;
  /**
   * 「更多 → 删除账号」：页面弹确认。
   *
   * ⚠️ 自 2026-10-15 起 Qoder 侧是**软删除**（进回收站，可恢复）—— 弹层文案随之改了，
   * 别再写「没有回收站」。
   */
  onDelete?: (account: QoderAccountView) => void;
  /** 额度快照；`null` / 不传 ⇒ 不渲染积分指标（无数据不渲染，不摆「0 / 0」）。 */
  credits?: QoderCardCredits | null;
  /** 今日签到三态（`null` / 不传 = 未知）。 */
  checkinToday?: boolean | null;
  /**
   * 程序位状态（只读）。
   *
   * 传入才渲染页脚的**程序位行** —— 未传入（例如程序位数据还没回来）时不渲染，
   * 不摆一排「未检测到」的灰胶囊：那是「没数据」伪装成「没装」。
   */
  programs?: QoderProgramStatus[] | null;
  /** 点击某个程序位胶囊 → 页面弹切换确认。未传 ⇒ 胶囊只读（不可点）。 */
  onSwitch?: (program: QoderProgramStatus) => void;
  /**
   * 任一账号正在切换中 ⇒ 锁住所有程序位按钮（issue #89 · 2.2）。
   *
   * 为什么是**全局**而不是逐账号：切换是「改写客户端登录态 + 客户端可能正在重启」，
   * 并发点第二个会在前一个还没落盘时覆盖它。渠道页四个 `*Busy` 同样取
   * 「任一账号切换中就锁全部」的语义，这里保持一致。
   */
  switchBusy?: boolean;
  /** 「更多 → 设置分组」。 */
  onSetGroup?: (account: QoderAccountView) => void;
  /**
   * 「更多 → 手动签到」：与批量签到 / 抽屉里的签到**同一条路径**
   * （`runCheckin` → 单账号 `qoder_checkin_claim`），不另造一条。
   */
  onCheckin?: (account: QoderAccountView) => void;
  /** 「更多 → 导出账号」：与批量导出共用同一个弹层（只把目标换成一个账号）。 */
  onExport?: (account: QoderAccountView) => void;
  /**
   * 单项刷新（积分 / 今日 Token / 签到状态 / 凭据状态）。
   * 传了 ⇒ 菜单出现「刷新」分组（四项各自独立）；不传 ⇒ 菜单与原来一致。
   */
  onRefreshPart?: (account: QoderAccountView, kind: AccountRefreshKind) => void;
  /** 本账号正在进行中的刷新项。 */
  refreshingKinds?: readonly AccountRefreshKind[];
  /** 刚刚刷新成功（自行淡出）的项，菜单里显示「已更新」。 */
  doneKinds?: readonly AccountRefreshKind[];
  /** 本平台真的能刷的项（缺省四项）；见 `RefreshMenuGroup` 的注释。 */
  refreshKinds?: readonly AccountRefreshKind[];
}

export function QoderAccountCard({
  account,
  selected = false,
  onToggleSelect,
  onOpenDetail,
  onEditRemark,
  onDelete,
  credits = null,
  checkinToday,
  programs = null,
  onSwitch,
  switchBusy = false,
  onSetGroup,
  onCheckin,
  onExport,
  onRefreshPart,
  refreshingKinds,
  doneKinds,
  refreshKinds,
}: QoderAccountCardProps) {
  const t = useT();
  const name = qoderAccountName(account);
  const uid = displayText(account.uid);
  const identity = uid ? `UID · ${uid}` : "—";
  const sourceLabel = qoderSourceLabel(t, account.source);
  const credentialSummary = displayText(account.credentialSummary);
  /* 「当前登录」= **任一**程序位当前登着的就是这个 uid。
     判定放在卡片里（同一个 currentUid 要跟列表每条账号比），后端只给事实。 */
  const currentPrograms = (programs ?? []).filter((program) => program.currentUid === account.uid);
  const isCurrent = currentPrograms.length > 0;

  const menu = demoModeEnabled ? (
    <DemoAction>
      <Button
        variant="ghost"
        size="sm"
        className="!px-[10px] text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={t("qoder.page.accounts.card.manageAria", { name })}
        title={t("qoder.page.accounts.card.moreActions")}
      >
        <Ellipsis className="size-4" />
      </Button>
    </DemoAction>
  ) : (
    <QoderAccountMenu
      account={account}
      name={name}
      checkinToday={checkinToday}
      onOpenDetail={onOpenDetail}
      onEditRemark={onEditRemark}
      onSetGroup={onSetGroup}
      onCheckin={onCheckin}
      onExport={onExport}
      onDelete={onDelete}
      onRefreshPart={onRefreshPart ? (kind) => onRefreshPart(account, kind) : undefined}
      refreshingKinds={refreshingKinds}
      doneKinds={doneKinds}
      refreshKinds={refreshKinds}
    />
  );

  return (
    <article
      /* 键盘可达：整卡可 Tab 聚焦，Enter / Space 打开详情（与 WB / Trae 卡同一交互）。 */
      tabIndex={0}
      role="article"
      onClick={(event) => {
        if (!onOpenDetail) return;
        /* ★ 判据在 `lib/account-open-guard.ts`（三张卡 + 表格行共用），
           别再退回手写 `closest("button, …")` —— 菜单项是 Portal 里的
           `div[role="menuitem"]`，手写判据拦不住（2026-10-22 报障根因）。 */
        if (!shouldOpenAccountDetail(event)) return;
        onOpenDetail(account);
      }}
      onKeyDown={(event) => {
        if (!onOpenDetail || event.target !== event.currentTarget) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpenDetail(account);
        }
      }}
      className={cn(
        "relative flex min-w-0 flex-col gap-2.5 overflow-hidden rounded-lg border border-border bg-card px-3.5 py-[13px] shadow-card transition-[border-color,box-shadow] hover:border-border-strong hover:shadow-card-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
        selected && "border-ring bg-primary/[0.07]",
      )}
    >
      {/* header：勾选 + 头像 32 + 昵称/身份 —— 原型 `.acct-top` 的四件套（无「当前登录」）。 */}
      <header className="flex items-center gap-2.5">
        <div className="relative z-10 flex w-full min-w-0 items-center gap-2.5">
          <Checkbox
            checked={selected}
            onCheckedChange={() => onToggleSelect?.(account)}
            disabled={!onToggleSelect}
            aria-label={t("qoder.page.accounts.card.selectAria", { name })}
            className="size-[17px]"
          />
          <div
            className={cn(
              "flex size-8 shrink-0 items-center justify-center rounded-[10px] text-[12.5px] font-[640]",
              AVATAR_TEXT_CLASS,
              avatarTone(name),
            )}
          >
            {accountInitials(name)}
          </div>
          <div className="min-w-0 flex-1">
            <h3 className="truncate text-[13px] font-semibold" title={name}>
              {name}
            </h3>
            <p className="truncate text-[11px] text-muted-foreground" title={identity}>
              {identity}
            </p>
          </div>
        </div>
      </header>

      {/* chips：状态槽（恒一枚） + 归属异常（正交） + 来源徽章。 */}
      <div className="flex min-w-0 flex-wrap items-center gap-[5px]">
        <QoderStatusChips account={account} checkinToday={checkinToday} />
        <QoderForeignBadge account={account} />
        {isCurrent ? (
          <Badge variant="info" className={CHIP_CLASS}>
            {t("qoder.page.accounts.program.current")}
          </Badge>
        ) : null}
        {sourceLabel ? (
          <Badge variant="secondary" className={cn(CHIP_CLASS, "text-muted-foreground")}>
            {sourceLabel}
          </Badge>
        ) : null}
      </div>

      {/* body：积分指标（有额度快照才渲染）+ 凭据摘要 + 访问令牌掩码（**只给掩码**）。 */}
      <section className="flex min-w-0 flex-1 flex-col gap-1">
        {credits ? (
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5 text-[11.5px]">
            <span className="text-muted-foreground">
              {t("qoder.page.accounts.metric.credits")}{" "}
              <span className="font-medium tabular-nums text-foreground">
                {t("qoder.page.accounts.metric.creditsValue", {
                  remaining: String(credits.remaining),
                  total: String(credits.total),
                })}
              </span>
            </span>
            {credits.plan ? (
              <span className="text-muted-foreground">
                {t("qoder.page.accounts.metric.plan")}{" "}
                <span className="text-foreground">{credits.plan}</span>
              </span>
            ) : null}
          </div>
        ) : null}
        {credentialSummary ? (
          <div className="truncate text-[11.5px] text-muted-foreground" title={credentialSummary}>
            {credentialSummary}
          </div>
        ) : null}
        {account.accessTokenMasked ? (
          <div className="truncate font-mono text-[11px] text-muted-foreground" title={account.accessTokenMasked}>
            {account.accessTokenMasked}
          </div>
        ) : null}
      </section>

      {/* footer：原型 `.acct-actions`（程序位行 + 更多菜单）。
          程序位行只在**拿到程序位状态**时渲染 —— 没数据时不摆一排「未检测到」的灰胶囊。 */}
      <footer className="flex items-center gap-1.5 border-t border-border pt-2.5">
        {programs && programs.length > 0 ? (
          <div className="flex min-w-0 flex-wrap items-center gap-1">
            {programs.map((program) => {
              const active = program.currentUid === account.uid;
              /* 三态：当前（✓） / 可切换（可点） / 未就绪（禁用 + 原因 tooltip）。
                 ★ 客户端在跑时**不可切换** —— 后端会拒绝（进程守卫），界面提前置灰并说明，
                 而不是让用户点了才失败。 */
              const ready = program.state === "ready" && !program.running;
              const title = program.running
                ? t("qoder.page.accounts.program.runningHint")
                : stateLabel(t, program.state);
              return (
                <button
                  key={program.id}
                  type="button"
                  disabled={active || !ready || !onSwitch || switchBusy}
                  title={switchBusy ? t("aggregate.toast.switching") : title}
                  onClick={() => onSwitch?.(program)}
                  className={cn(
                    "rounded-md border px-1.5 py-[2px] text-[11px] transition-colors",
                    active
                      ? "border-primary/40 bg-primary/10 font-medium text-foreground"
                      : ready && onSwitch && !switchBusy
                        ? "border-border text-muted-foreground hover:border-border-strong hover:text-foreground"
                        : "border-dashed border-border text-muted-foreground/60",
                  )}
                >
                  {program.label}
                </button>
              );
            })}
          </div>
        ) : null}
        <div className="grow" />
        {menu}
      </footer>
    </article>
  );
}
