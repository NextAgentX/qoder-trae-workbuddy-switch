import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { toast } from "sonner";
import {
  AlertTriangle,
  CalendarCheck,
  Download,
  FileDown,
  Loader2,
  Plane,
  QrCode,
  RefreshCw,
  Terminal,
} from "lucide-react";

import { AccountCard, accountIdentity, accountDisplayName, formatCredits, formatCreditExpiry, formatTodayTokens, todayTokensBreakdown, type GuideTarget } from "@/components/account-card";
import { AccountsTableAvatar } from "@/components/account-list-parts";
import { AccountWorkbench } from "@/components/account-workbench";
import { type AccountColumn } from "@/components/account-table";
import { type ToolbarOption } from "@/components/account-toolbar";
import { AddAccountDrawer, type AddAccountWay } from "@/components/add-account-drawer";
import { AccountDetailDrawer, type DetailActions, type DetailCheckinHistoryRow, type DetailPack, type DetailProgramBadge, type DetailSection, type DetailSwitchTarget } from "@/components/account-detail-drawer";
import { ClientLimitsCard } from "@/components/client-limits-card";
import { DemoAction } from "@/components/demo-action";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ExportAccountsDialog } from "@/components/export-accounts-dialog";
import { ImportAccountsDialog } from "@/components/import-accounts-dialog";
import { OAuthLoginDialog } from "@/components/oauth-login-dialog";
import { RecycleBinSheet } from "@/components/recycle-bin-sheet";
import { RestartConfirmDialog } from "@/components/restart-confirm-dialog";
import { SwitchAccountDialog } from "@/components/switch-account-dialog";
import { ExtSessionDrawer } from "@/components/sessions/ext-session-drawer";
import * as api from "@/lib/api";
// ★ B5：`checkinCreditsOf` 是**值**（从条目/raw 里取积分字段），`CheckinStatusEntry` 是**类型**。
import { checkinCreditsOf } from "@/lib/api";
import type { CheckinStatusEntry } from "@/lib/api";
import { accountSearchText } from "@/lib/account-search";
import { lastSevenDays, localDateKey } from "@/lib/checkin-history";
import {
  filterAccounts,
  groupAccounts,
  hasActiveFilter,
  sortAccounts,
  type AccountComparator,
  type AccountGroupKey,
  type AccountListConfig,
  type AccountSortKey,
  type AccountViewMode,
} from "@/lib/account-list-view";
import { copyText } from "@/lib/clipboard";
import { displayText } from "@/lib/display-text";
import { formatDate } from "@/lib/format";
import { regionDescriptor } from "@/lib/region";
import { scopeOf } from "@/lib/scope";
import type {
  AccountMeta,
  AccountTokenTodayEntry,
  AppStatus,
  CheckinConfig,
  CheckinLog,
  CodeBuddyCliStatus,
  CodeBuddyCnIdeStatus,
  CreditExpiry,
  CreditStatsEvent,
  JetbrainsCodebuddyPluginStatus,
  ExtClientId,
  Region,
  SwitchConfig,
  TravelConfig,
  TravelStatus,
  VscodeCodebuddyPluginStatus,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { useDocumentVisible } from "@/lib/use-document-visible";
import { useMediaQuery } from "@/lib/use-media-query";
import { useScope } from "@/lib/use-scope";
import { useAccountsStore } from "@/stores/accounts";
import { useT } from "@/lib/i18n";
import { showUndoToast } from "@/lib/undo-toast";

function expiringSoonAmount(credit?: CreditExpiry): number {
  return credit?.ok ? credit.expiringSoonRemaining ?? 0 : 0;
}

/* 日期口径（`localDateKey` / `lastSevenDays`）已上提到 `@/lib/checkin-history` ——
   Trae 侧也要装配「近 7 天」抽屉区块，两处必须同口径（差一天就会把「昨天签的」显示成「未签到」）。 */

function hasExpiringSoonCredits(credit?: CreditExpiry): boolean {
  return credit?.ok === true && expiringSoonAmount(credit) > 0;
}

/**
 * 需要走「统一重启确认」的客户端（PLAN §3.15.3）。
 *
 * - `cli`：**例外** —— 不重开客户端，但当前 CLI 会话会中断（确认框不渲染「自动重开」）；
 * - `ide` / `vscode` / `jetbrains`：都是「先关 → 写凭证 → 再开」⇒ 默认勾选自动重开，
 *   并显式提示未保存文件风险。
 */
type SwitchClient = "cli" | "ide" | "vscode" | "jetbrains";

function soonestRelevantExpiry(credit?: CreditExpiry): number {
  const soonestExpiringCredit = (credit?.resources ?? [])
    .filter((resource) => resource.remaining > 0 && resource.expiringSoon && resource.expireAt != null)
    .map((resource) => resource.expireAt as number)
    .reduce((soonest, expireAt) => Math.min(soonest, expireAt), Number.POSITIVE_INFINITY);
  return Number.isFinite(soonestExpiringCredit)
    ? soonestExpiringCredit
    : credit?.soonestExpireAt ?? Number.POSITIVE_INFINITY;
}

function creditPriorityRank(credit?: CreditExpiry): number {
  if (!credit?.ok) return 3;
  if (hasExpiringSoonCredits(credit)) return 0;
  if (credit.expired) return 1;
  return 2;
}

function isWorkbuddyCurrent(account: AccountMeta, current: AppStatus["current"] | undefined): boolean {
  if (!current) return false;
  return Boolean(
    (current.uid && (account.uid === current.uid || account.id === current.uid)) ||
      (current.email && account.email === current.email),
  );
}

/**
 * 并行查询今日签到；失败的账号不写入，由调用方保留原值。
 *
 * ★ 必须走 `api.getCheckinStatusMap`（**批量**），不要在这里对每个账号调
 * `api.getCheckinStatus` —— webui 端那个端点是**整端点**，逐个调用会变成
 * N × N 次上游签到查询（详见 `api.ts` 里该函数的文档）。
 */
async function fetchTodayCheckinMap(
  accountIds: string[],
  region: Region,
  isStale?: () => boolean,
): Promise<Record<string, boolean>> {
  const entries = await api.getCheckinStatusMap(accountIds, region);
  // 整批回来后统一判一次陈旧：过期的结果一条都不用。
  if (isStale?.()) return {};
  const next: Record<string, boolean> = {};
  for (const [id, entry] of Object.entries(entries)) {
    if (entry.ok) next[id] = entry.todayCheckedIn;
  }
  return next;
}

/**
 * 并行查询各账号今日旅行状态；失败的账号不写入，由调用方保留原值。
 *
 * ★ 同上：必须走批量的 `api.getTravelStatusMap`。
 */
async function fetchTravelMap(
  accountIds: string[],
  region: Region,
  isStale?: () => boolean,
): Promise<Record<string, TravelStatus>> {
  const entries = await api.getTravelStatusMap(accountIds, region);
  if (isStale?.()) return {};
  return entries;
}

// ---------------------------------------------------------------------------
// 版本状态（Tab 徽标 / 空态判定）
// ---------------------------------------------------------------------------

type RegionPresence = "logged-in" | "installed" | "absent";

/**
 * 判定某版本的展示状态：
 * - logged-in：该 region 有当前登录账号（status.current 非空）。
 * - installed：客户端已安装（后端 status.installed === true）或账号库非空但未登录。
 * - absent：未检测到该版本（后端 status.installed === false，或状态与账号库均为空）。
 */
function regionPresence(status: AppStatus | null, accounts: AccountMeta[]): RegionPresence {
  if (status?.current) return "logged-in";
  if (status?.installed === false) return "absent";
  if (status?.installed === true || accounts.length > 0) return "installed";
  return "absent";
}

export default function AccountsPage() {
  /* P1：区域统一走 `useScope`（URL 优先 + localStorage 记忆），不再用组件 state。
     原实现是 `useState<Region>("cn")` —— 刷新即丢，且与统计页互不通信（§1.2 诊断）。 */
  const [activeRegion] = useScope("workbuddy");
  const fetchAllRegions = useAccountsStore((s) => s.fetchAllRegions);
  const regionAccounts = useAccountsStore((s) =>
    activeRegion === "cn" ? s.accounts : s.global.accounts,
  );
  const t = useT();

  useEffect(() => {
    void fetchAllRegions();
  }, [fetchAllRegions]);

  const pendingCount = regionAccounts.filter((a) => a.needsRelogin).length;

  return (
    <div className="mx-auto w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
      {/* 页头：作用域副标题 + 只读 scope-chip（区域已并入 L1 作用域，切作用域走顶栏）。 */}
      <header className="mb-5 flex items-start gap-4">
        <div className="min-w-0">
          {/* 原型 `.page-head h1`（`buddy-switch-prototype.html:179`）：28px / 字重 640 / `-.02em`。 */}
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("wbAccounts.page.title")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">
            {pendingCount > 0
              ? t("pageHead.accounts.pending", {
                  scope: scopeLabel(t, scopeOf("workbuddy", activeRegion)),
                  n: regionAccounts.length,
                  m: pendingCount,
                })
              : t("pageHead.accounts", {
                  scope: scopeLabel(t, scopeOf("workbuddy", activeRegion)),
                  n: regionAccounts.length,
                })}
          </p>
        </div>
        <div className="grow" />
        <ScopeChip scope={scopeOf("workbuddy", activeRegion)} />
      </header>

      {/* 只渲染当前版本的区块（等价于原先 `TabsContent` 的惰性挂载：
          未选中的版本不取数、不挂载）。 */}
      {activeRegion === "cn" ? <RegionPanel region="cn" /> : <RegionPanel region="global" />}
    </div>
  );
}

/** 单个版本的完整账号管理区块（账号卡片、签到开关、积分、刷新等）。 */
function RegionPanel({ region }: { region: Region }) {
  const accounts = useAccountsStore((s) => (region === "cn" ? s.accounts : s.global.accounts));
  const status = useAccountsStore((s) => (region === "cn" ? s.status : s.global.status));
  const loading = useAccountsStore((s) => (region === "cn" ? s.loading : s.global.loading));
  const error = useAccountsStore((s) => (region === "cn" ? s.error : s.global.error));
  const creditMap = useAccountsStore((s) => (region === "cn" ? s.creditMap : s.global.creditMap));
  const creditLoadingMap = useAccountsStore((s) => (region === "cn" ? s.creditLoadingMap : s.global.creditLoadingMap));
  const refreshingCredits = useAccountsStore((s) => (region === "cn" ? s.refreshingCredits : s.global.refreshingCredits));
  const reconcileAccounts = useAccountsStore((s) => s.reconcileAccounts);
  const refreshRegionStatus = useAccountsStore((s) => s.refreshRegionStatus);
  const deleteAccountStore = useAccountsStore((s) => s.deleteAccount);
  const ensureCredits = useAccountsStore((s) => s.ensureCredits);
  const refreshCredits = useAccountsStore((s) => s.refreshCredits);
  const importLocalStore = useAccountsStore((s) => s.importLocal);

  const descriptor = regionDescriptor(region);

  const [oauthOpen, setOauthOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  /* ★「添加账号」三选抽屉（原型 `drawerAdd()`）。选完方式后**关闭抽屉**再开对应流程
     （原型 `case "add-way"` 也是 `closeDrawer()` 之后才继续）。 */
  const [addDrawerOpen, setAddDrawerOpen] = useState(false);
  /**
   * 导出目标：`null` = 导出**全部**账号（工具栏入口）；非 `null` = 导出**所选**账号（批量条入口）。
   *
   * 与 `exportOpen` 分开是因为它们回答两个问题：「导哪些」与「弹窗开着没」——
   * 合成一个状态会让「关闭时清空选择」和「打开时带选择」互相踩。
   */
  const [exportTargets, setExportTargets] = useState<AccountMeta[] | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [switchAccount, setSwitchAccount] = useState<AccountMeta | null>(null);
  /**
   * ★ P14 S4a · 插件会话抽屉的目标（`null` = 关闭）。
   *
   * 与 `switchAccount` **分开两个状态**：两者是**不同的抽屉、不同的流程**
   * （切换向导是桌面版迁移；插件会话是一次性单向复制），共用一个状态会让
   * 「关掉一个却打开了另一个」这种缺陷无法表达。
   */
  const [extDrawer, setExtDrawer] = useState<{ client: ExtClientId; account: AccountMeta } | null>(
    null,
  );
  /**
   * 待确认的客户端切换目标（P8 T03 / §3.15.3）。
   *
   * **所有**客户端切换都走**同一条**确认流程：`client` 决定走「CLI 例外」文案（不重开、当前
   * CLI 会话会中断）还是「需重启」文案（默认勾选自动重开 + 未保存文件风险）。
   *
   * ⚠️ **同一功能在不同客户端行为不一致是缺陷** —— 用户会发现「为什么切 CLI 会提醒、切 IDE
   * 不提醒」，然后**不再相信这个提醒**。故 IDE 与 CLI 共用本状态、共用同一个确认框。
   *
   * ★ 2026-10-09：确认对象从 `isCli` 布尔扩成 `client` 四值 —— 新增的编辑器内 CodeBuddy
   * 插件（VS Code / JetBrains）同样是「需先关再开」，必须走**同一条**确认（否则又出现
   * 「有的客户端提醒、有的不提醒」）。
   */
  const [restartConfirm, setRestartConfirm] = useState<{
    account: AccountMeta;
    client: SwitchClient;
  } | null>(null);
  const [importing, setImporting] = useState(false);
  const [autoCheckinConfig, setAutoCheckinConfig] = useState<CheckinConfig | null>(null);
  const [autoCheckinSaving, setAutoCheckinSaving] = useState(false);
  /** 账号 id -> 今日是否已签到（undefined=查询中/未知） */
  const [checkinMap, setCheckinMap] = useState<Record<string, boolean>>({});
  const [autoTravelConfig, setAutoTravelConfig] = useState<TravelConfig | null>(null);
  const [autoTravelSaving, setAutoTravelSaving] = useState(false);
  /**
   * 账号切换 / 账号列表展示配置（全局单份，不随 region 分家）。
   *
   * 未加载完时保持 `null`：两处消费方都按「拿不到就不用这项偏好」处理 ——
   * `pin_current_account` 视为关（不改排序）、`copy_sessions_by_default` 视为关
   * （不改变切换语义）。宁可退化成改造前的行为，也不要凭猜测展示。
   */
  const [switchConfig, setSwitchConfig] = useState<SwitchConfig | null>(null);
  /** 账号 id -> 今日旅行状态（undefined=查询中/未知） */
  const [travelMap, setTravelMap] = useState<Record<string, TravelStatus>>({});
  /**
   * 「今日 Token」按账号聚合（`get_account_token_today`）：`uid -> 明细`。
   *
   * ⚠️ 用 **Map** 而非缺省对象是刻意的：`accounts` 里**缺席**某 uid = 「今日无记录 / 未知」，
   * 与「有一行值全 0（真实 0）」是两件事（诚实边界）。`map.get(uid)` 为 `undefined` 即缺席。
   */
  const [tokenTodayMap, setTokenTodayMap] = useState<Map<string, AccountTokenTodayEntry>>(new Map());
  /** 网关日志已到保留上限 ⇒ 网关部分为下界（「今日 Token」渲染 `≥N`）。 */
  const [tokenTodayTruncated, setTokenTodayTruncated] = useState(false);
  const [codebuddyCli, setCodebuddyCli] = useState<CodeBuddyCliStatus | null>(null);
  const [codebuddyCliSwitchingId, setCodebuddyCliSwitchingId] = useState<string | null>(null);
  const [codebuddyCnIde, setCodebuddyCnIde] = useState<CodeBuddyCnIdeStatus | null>(null);
  const [codebuddyCnIdeSwitchingId, setCodebuddyCnIdeSwitchingId] = useState<string | null>(null);
  /* ★ 2026-10-09：编辑器内 CodeBuddy 插件两个程序位（VS Code / JetBrains）的安装状态与切换态。 */
  const [vscodePlugin, setVscodePlugin] = useState<VscodeCodebuddyPluginStatus | null>(null);
  const [vscodePluginSwitchingId, setVscodePluginSwitchingId] = useState<string | null>(null);
  const [jetbrainsPlugin, setJetbrainsPlugin] = useState<JetbrainsCodebuddyPluginStatus | null>(null);
  const [jetbrainsPluginSwitchingId, setJetbrainsPluginSwitchingId] = useState<string | null>(null);
  const [installingCodebuddyCli, setInstallingCodebuddyCli] = useState(false);
  /** 刷新按钮触发的批量签到进行中 */
  const [checkinAllRunning, setCheckinAllRunning] = useState(false);
  /** 接入/升级 CLI helper 确认框 */
  const [installConfirmOpen, setInstallConfirmOpen] = useState(false);
  /** 删除账号确认目标（null=关闭） */
  const [deleteTarget, setDeleteTarget] = useState<AccountMeta | null>(null);
  /** 回收站抽屉开关（P5 §2.7.1：按**当前作用域**过滤，入口在工具栏）。 */
  const [recycleOpen, setRecycleOpen] = useState(false);
  /** 区域不匹配详情展开 */
  const [mismatchDetailOpen, setMismatchDetailOpen] = useState(false);
  /**
   * P4 工具栏：搜索 / 排序 / 分组 / 视图。
   *
   * 四项都是**页面私有 `useState`**（`PLAN §2.1 L3` / `DECISIONS.md` D1）：不进 URL、
   * 不进 store、不碰作用域，且**不持久化**（决策 #2 —— 视图参数是临时的，持久化会让
   * 「为什么我的排序怪怪的」变成谜题）。唯一预留的 `?q=` 种子属 P6，本轮**不接线**（决策 #1）。
   */
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<AccountSortKey>("priority");
  const [group, setGroup] = useState<AccountGroupKey>("none");
  const [view, setView] = useState<AccountViewMode>("card");
  /** < 900px 强制卡片视图（`§3.12`）；断点只在 `useMediaQuery` 一处判断，不散落常量。 */
  const tableEnabled = useMediaQuery("(min-width: 900px)");
  const searchRef = useRef<HTMLInputElement>(null);
  const t = useT();
  const navigate = useNavigate();

  /** `/` 键聚焦搜索框（`§3.4`）；在输入框 / 文本域 / 可编辑区里打字时不劫持。 */
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;
      event.preventDefault();
      searchRef.current?.focus();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  /**
   * P6：命令面板跳过来的 **`?q=` 一次性种子**（§1.8）。
   *
   * 面板搜到**别的作用域**的账号时，回车会跳到本页并带上 `q`。这里**读一次**播种页内
   * 搜索 state，随后 `replace` 清掉参数 —— 此后 URL 不再承载 `q`，视图参数留在页内
   * （§2.1 L3）。用 `seededRef` 保证只读一次（StrictMode 双调用 / 后续 URL 变化都不会重播）。
   */
  const [searchParams, setSearchParams] = useSearchParams();
  const seedAppliedRef = useRef(false);
  useEffect(() => {
    if (seedAppliedRef.current) return;
    seedAppliedRef.current = true;
    const seed = searchParams.get("q");
    if (!seed) return;
    setQuery(seed);
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("q");
        return next;
      },
      { replace: true },
    );
  }, [searchParams, setSearchParams]);

  /**
   * P4：详情抽屉 + 最小批量选择。
   *
   * 抽屉状态由**页面**持有（卡片只发 `onOpenDetail`）—— 抽屉需要「这个账号的全部明细」，
   * 而那些数据（积分包、签到态、程序位）只有页面手里齐全。
   *
   * `detailAutoEditRemark` 区分两个入口：点「查看详情」只打开抽屉；点「更多 → 编辑备注」
   * 打开抽屉并**直接进入编辑态**（`autoEditRemark`）。
   */
  const [detailAccount, setDetailAccount] = useState<AccountMeta | null>(null);
  /**
   * ★ B5：抽屉里的**签到积分详情**（按账号 id 缓存）。
   *
   * ⚠️ 与 `checkinMap`（只记「是否已签到」）**不是一回事**：这里要的是
   * `daily_credit` / `streak_days` / `checkin_dates` 等**上游实测字段**。
   */
  const [checkinDetailMap, setCheckinDetailMap] = useState<Record<string, CheckinStatusEntry>>({});
  const [detailAutoEditRemark, setDetailAutoEditRemark] = useState(false);
  /** 由「更多 → 设置分组」打开抽屉时，直接进入**分组**编辑态（与备注同一交互）。 */
  const [detailAutoEditGroup, setDetailAutoEditGroup] = useState(false);
  /**
   * ★ 抽屉「签到历史（近 7 天）」的数据源（打开抽屉时拉取）：
   * - `checkinLogs` = `getCheckinLogs()`：**真实**的日期 + 结果；
   * - `creditEvents` = `getCreditStatistics()` 的事件流：按**账号 + 日期**匹配签到事件的 `amount`
   *   ⇒ 拿到「获得积分」。取不到时为 `null`，表格渲染「—」（**绝不**臆造固定奖励）。
   */
  const [detailCheckinLogs, setDetailCheckinLogs] = useState<CheckinLog[] | null>(null);
  const [detailCreditEvents, setDetailCreditEvents] = useState<CreditStatsEvent[] | null>(null);
  /**
   * ★ 跨作用域选择集按 **(scope, id)** 键控（P5 §2.5 第 (4) 条）。
   *
   * 键里带上 scope：切作用域后旧键自然失配 ⇒ `targets` 恒为 0，**不会**用 A 作用域的 id
   * 去打 B 作用域的库（或静默空转）。P4 已在 `region` 变化时清空（见下方 effect），
   * 这里是**键控层面**的加固 —— 即便哪天清空 effect 被误删，键控本身仍能挡住跨作用域误操作。
   */
  const scope = scopeOf("workbuddy", region);
  const selectionKey = (id: string) => `${scope}:${id}`;
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [batchBusy, setBatchBusy] = useState(false);
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  /**
   * 批量签到**结果**（原型 `state.checkinResult`，`buddy-switch-prototype.html:2339-2354`）。
   *
   * ⚠️ 不能只靠 toast：toast 一闪而过，用户来不及看失败的是谁，且「一键签到」看起来
   * 永远 100% 成功。结果条**留在页面上**（工具栏与列表之间），带「只重试失败项」出口。
   */
  const [checkinResult, setCheckinResult] = useState<{ ok: number; failedIds: string[] } | null>(null);

  function toggleSelect(account: AccountMeta) {
    const key = selectionKey(account.id);
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function clearSelection() {
    setSelectedKeys(new Set());
  }

  /**
   * P4：区域变化时清空勾选。
   *
   * `selectedKeys` 是**组件 state**，而切区域时 `<RegionPanel>` 只换 `region` prop、
   * **不 remount** ⇒ 选择集会跨区域残留：批量条仍显示「已选 N」，但
   * `targets = accounts.filter(selectedKeys)` 恒为 0 ⇒ 点动作**静默无操作**。
   *
   * 为什么用 effect 而不是在切换处清：P3 已定「页面内不该有能力切区域」，
   * 不能为了清选择去接管区域 setter；监听 `region` 变化是唯一不越界的方式。
   */
  useEffect(() => {
    setSelectedKeys(new Set());
  }, [region]);

  function openDetail(account: AccountMeta) {
    setDetailAutoEditRemark(false);
    setDetailAutoEditGroup(false);
    setDetailAccount(account);
    // ★ B5：顺带查一次**签到详情**（抽屉要显示「+100」+ 连签天数 + 日期列表）。
    //   此前只在批量接口里取了「是否已签到」，**详情从没查过** ⇒ 抽屉无从展示。
    void api
      .getCheckinStatus(account.id, region)
      .then((entry) => setCheckinDetailMap((prev) => ({ ...prev, [account.id]: entry })))
      .catch(() => {
        /* 查询失败不阻断抽屉：签到区**不渲染**即可（不摆空数据）。 */
      });
    // ★ 签到历史（近 7 天）：日期 + 结果来自签到日志；「获得积分」来自积分统计的签到事件。
    //   两者都**真实**取数；任一失败则该列回落「—」，不臆造。
    void api
      .getCheckinLogs()
      .then((res) => setDetailCheckinLogs(res.logs))
      .catch(() => setDetailCheckinLogs(null));
    void api
      .getCreditStatistics(false, region)
      .then((stats) => setDetailCreditEvents(stats.events))
      .catch(() => setDetailCreditEvents(null));
  }

  function openRemark(account: AccountMeta) {
    setDetailAutoEditRemark(true);
    setDetailAutoEditGroup(false);
    setDetailAccount(account);
  }

  /** 「更多 → 设置分组」：打开抽屉并进入**分组**编辑态（与 `openRemark` 同一交互）。 */
  function openGroup(account: AccountMeta) {
    setDetailAutoEditRemark(false);
    setDetailAutoEditGroup(true);
    setDetailAccount(account);
  }

  /**
   * 未检测到的程序位（IDE / CLI）点击 → 引导到设置页对应子项。
   *
   * 为什么不就地处理：IDE 路径与 CLI 接入都是**设置页**的能力，页面里没有第二处入口；
   * 给一句 toast + 跳过去，才能让「未检测到」变成可解决的问题（PLAN §3.3 规则 1）。
   */
  function onGuideToSetting(target?: GuideTarget) {
    /* 卡片上的「未检测程序位」入口带 `target` ⇒ 给一句针对性 toast；
       抽屉里的「切换权限诊断」不带 `target` ⇒ 只跳转，不改变既有卡片文案。 */
    if (target) {
      toast.info(
        target === "ide" ? t("wbAccounts.toast.guideIdeSetting") : t("wbAccounts.toast.guideCliSetting"),
      );
    }
    /* ★ P0-4a：必须带**子项锚点** ⇒ 直接落到「权限与诊断」（`PermissionCheckCard`），
       而不是设置页首页（那样用户还得自己找子项，原型断言 `P0-4a` 明确要求"跳到权限与诊断"）。 */
    navigate(`/s/${scopeOf("workbuddy", region)}/settings?section=diagnostics`);
  }

  /**
   * 对一组账号跑签到（**全量批签与「只重试失败项」共用这一条路径**）。
   * 返回**失败 id 清单**，供 toast 的 action 复用 —— 原型 P1-2 要求部分失败态**可只重试失败项**。
   */
  async function runCheckinFor(ids: string[]) {
    let ok = 0;
    let already = 0;
    const failedIds: string[] = [];
    for (const id of ids) {
      try {
        const res = await api.checkin(id, region);
        if (res.result === "success") ok += 1;
        else if (res.result === "already") already += 1;
        else failedIds.push(id);
      } catch {
        failedIds.push(id);
      }
    }
    const next = await fetchTodayCheckinMap(ids, region);
    if (Object.keys(next).length > 0) setCheckinMap((prev) => ({ ...prev, ...next }));
    await refreshCredits(ids, { region });
    void reconcileAccounts(region);
    return { ok, already, failedIds };
  }

  /**
   * 批量签到：逐个走**单账号**接口（与单卡菜单同一路径）。
   * ★ 部分失败时**把失败项留在 toast 的 action 里** ⇒ 用户可「只重试失败项」，
   *   不必把已成功的再跑一遍（原型 P1-2：部分失败态不能只给一句 toast 就完事）。
   * @param ids 省略 = 对当前勾选项跑；传入 = 只对这批（重试路径）。
   */
  async function batchCheckin(ids?: string[]) {
    if (batchBusy) return;
    const targetIds =
      ids ?? accounts.filter((a) => selectedKeys.has(selectionKey(a.id))).map((a) => a.id);
    if (targetIds.length === 0) return;
    setBatchBusy(true);
    try {
      const { ok, already, failedIds } = await runCheckinFor(targetIds);
      /* ★ 结果**留在页面上**（原型 `checkinResult`）—— toast 只做即时反馈，不承担留档。 */
      setCheckinResult({ ok: ok + already, failedIds });
      const parts: string[] = [];
      if (ok > 0) parts.push(t("wbAccounts.toast.checkinBatchSuccess", { n: ok }));
      if (already > 0) parts.push(t("wbAccounts.toast.checkinBatchAlready", { n: already }));
      if (failedIds.length > 0) {
        parts.push(t("wbAccounts.toast.checkinBatchFailed", { n: failedIds.length }));
      }
      const summary =
        parts.length > 0 ? parts.join(t("shared.punct.comma")) : t("wbAccounts.toast.checkinNone");
      /* ★ 有失败 ⇒ 给「只重试失败项」出口（否则用户只能把成功的一起重跑）。 */
      const retryAction =
        failedIds.length > 0
          ? {
              label: t("wbAccounts.toast.checkinRetryFailed"),
              onClick: () => void batchCheckin(failedIds),
            }
          : undefined;
      if (failedIds.length === targetIds.length) {
        toast.error(t("wbAccounts.toast.checkinFailBatch"), { description: summary, action: retryAction });
      } else {
        toast.success(t("wbAccounts.toast.checkinDone"), { description: summary, action: retryAction });
      }
    } finally {
      setBatchBusy(false);
    }
  }

  /** 批量刷新积分 + 旅行状态（只针对已勾选账号）。 */
  async function batchRefresh() {
    if (batchBusy) return;
    const ids = accounts.filter((a) => selectedKeys.has(selectionKey(a.id))).map((a) => a.id);
    if (ids.length === 0) return;
    setBatchBusy(true);
    try {
      await refreshCredits(ids, { region });
      await loadTravelMap(ids);
      toast.success(t("wbAccounts.toast.creditsRefreshed"));
    } finally {
      setBatchBusy(false);
    }
  }

  /** 账号的撤销 toast 副标题（过 `displayText` 白屏防线；全空则不显示副标题）。 */
  function accountUndoLabel(account: AccountMeta): string | undefined {
    return displayText(account.nickname) || displayText(account.email) || displayText(account.id) || undefined;
  }

  /**
   * 删除成功后弹「已删除 · 撤销」（P6 T04 §1.4）。
   *
   * ★ 撤销必须**真的能撤销**：删除是软删除（账号进回收站，P5 §2.7），撤销就是把账号从
   * 回收站移回 live —— 走 `api.accountRestore`（**与回收站同一命令**，不另写恢复逻辑）。
   * 恢复后 `reconcileAccounts` 重取列表，否则用户「撤销了却看不到账号回来」。
   *
   * 连续删多个时每条 toast 各自独立（`showUndoToast` 内部**不传固定 id**）。
   */
  function offerUndo(keys: readonly string[], description?: string) {
    showUndoToast({
      title: keys.length === 1 ? t("undo.deletedTitle") : t("undo.batchTitle", { n: keys.length }),
      description,
      undoLabel: t("undo.action"),
      undoneTitle: t("undo.undone"),
      undo: async () => {
        // 逐个恢复：一个失败不阻断其余；全部尝试后再刷新，最后把首个错误抛出（不静默）。
        let firstError: unknown = null;
        for (const key of keys) {
          try {
            await api.accountRestore(scope, key);
          } catch (e) {
            if (!firstError) firstError = e;
          }
        }
        await reconcileAccounts(region);
        if (firstError) throw firstError;
      },
    });
  }

  /** 批量删除（先确认）。逐个删除，任一个失败不阻断其余。 */
  async function batchDelete() {
    if (batchBusy) return;
    const targets = accounts.filter((a) => selectedKeys.has(selectionKey(a.id)));
    setBatchDeleteOpen(false);
    if (targets.length === 0) return;
    setBatchBusy(true);
    try {
      const deleted: AccountMeta[] = [];
      let failed = 0;
      for (const a of targets) {
        try {
          await deleteAccountStore(a.id, region);
          deleted.push(a);
        } catch {
          failed += 1;
        }
      }
      if (failed > 0) toast.error(t("wbAccounts.toast.deleteFail"), { description: `${failed}` });
      clearSelection();
      if (deleted.length > 0) {
        offerUndo(
          deleted.map((a) => a.id),
          deleted.length === 1 ? accountUndoLabel(deleted[0]) : undefined,
        );
      }
    } finally {
      setBatchBusy(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    void api
      .getAutoCheckinConfig()
      .then((config) => {
        if (!cancelled) setAutoCheckinConfig(config);
      })
      .catch((e) => {
        if (!cancelled) {
          toast.error(t("wbAccounts.toast.autoCheckinLoadFail"), { description: api.asError(e) });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void api
      .getAutoTravelConfig()
      .then((config) => {
        if (!cancelled) setAutoTravelConfig(config);
      })
      .catch((e) => {
        if (!cancelled) {
          toast.error(t("wbAccounts.toast.autoTravelLoadFail"), { description: api.asError(e) });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void api
      .getSwitchConfig()
      .then((config) => {
        if (!cancelled) setSwitchConfig(config);
      })
      .catch(() => {
        // 静默按默认值走：这两项只是偏好（排序 / 默认勾选），读不到不影响核心功能，
        // 而每次挂载都弹一次错误提示的代价远大于收益。
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function refreshCodebuddyCliStatus() {
    try {
      setCodebuddyCli(await api.getCodebuddyCliStatus());
    } catch {
      setCodebuddyCli(null);
    }
  }

  async function refreshCodebuddyCnIdeStatus() {
    try {
      setCodebuddyCnIde(await api.getCodebuddyCnIdeStatus());
    } catch {
      setCodebuddyCnIde(null);
    }
  }

  /**
   * 编辑器内 CodeBuddy 插件（VS Code / JetBrains）的**安装 / 运行 / 登录态**。
   *
   * ⚠️ 只拉 `status`，**不**在挂载时调 `detect`：`detect` 要**解密** secret（macOS 会弹
   * 钥匙串授权）⇒ 放进每次挂载的轮询路径会骚扰用户；「当前账号」改由本地状态文件给出，
   * 用户主动点「从本机导入」时才解密（与 `vscode_ext::status` 的注释同一口径）。
   */
  async function refreshVscodePluginStatus() {
    try {
      setVscodePlugin(await api.getVscodeCodebuddyPluginStatus());
    } catch {
      setVscodePlugin(null);
    }
  }

  async function refreshJetbrainsPluginStatus() {
    try {
      setJetbrainsPlugin(await api.getJetbrainsCodebuddyPluginStatus());
    } catch {
      setJetbrainsPlugin(null);
    }
  }

  useEffect(() => {
    let cancelled = false;
    void refreshCodebuddyCliStatus();
    /* 两个编辑器插件的状态：与 CLI / IDE 同批拉取（都是**本机只读探测**，无网络）。 */
    void refreshVscodePluginStatus();
    void refreshJetbrainsPluginStatus();
    void (async () => {
      if (!api.isDemoMode()) {
        try {
          await api.detectCodebuddyCnIdeAccount();
        } catch {
          /* 未登录或钥匙串拒绝时静默，下面仍拉安装/运行状态 */
        }
      }
      if (!cancelled) await refreshCodebuddyCnIdeStatus();
    })();
    return () => {
      cancelled = true;
    };
  }, [accounts.length]);

  /**
   * 自动导入「已登录、但还不在账号库里」的本机账号。
   *
   * ⚠️ 旧实现是一个「每版本只尝试一次」的布尔闸（`autoImportTried`），且要求
   * `accounts.length === 0`。于是**登录发生在页面加载之后**时必然错过：
   * 用户在客户端登录（或重新登录）后回到本页，闸门已经合上 ⇒ 不再导入 ⇒
   * 账号列表一直空着，只剩一张「当前登录」提示卡（2026-09-24 用户报障：
   * 「下面怎么是空的了」）。
   *
   * 改为**按 uid 记账**：只要「当前登录的 uid」还没为它试过一次，就试一次；
   * 也不再因 `accounts.length > 0` 提前返回 —— 库非空时当前账号仍可能不在库里。
   * 导入成功后 `accounts` 里就有该 uid，`accounts.some(...)` 自然成立，不会重复调用；
   * 导入失败则 uid 已入账，同样不会变成重试循环。
   */
  const autoImportTriedUids = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (loading) return;
    const current = status?.current;
    if (!current) return;
    const uid = current.uid;
    if (!uid) return;
    if (accounts.some((a) => isWorkbuddyCurrent(a, current))) return;
    if (autoImportTriedUids.current.has(uid)) return;
    autoImportTriedUids.current.add(uid);
    void importLocalStore(region)
      .then(() => void reconcileAccounts(region))
      .catch(() => {
        /* 本机无可用登录态时静默，不打扰用户 */
      });
  }, [accounts, status, loading, importLocalStore, reconcileAccounts, region]);

  // 切到该版本时立刻重查一次状态：`status.current` 决定卡片上的「当前账号」标记，
  // 若沿用上一次的快照，切换账号（尤其是国际版）后标记会停在旧账号上。
  useEffect(() => {
    void refreshRegionStatus(region);
  }, [refreshRegionStatus, region]);

  // 账号列表变化后并行查询各账号今日签到状态。
  //
  // 这是一次「被动请求」：用户没点任何东西，是我们自己发起的批量查询，所以必须受自动签到开关约束 ——
  // 开关关掉后还继续查，只会白白消耗上游额度、制造噪声。
  // `autoCheckinConfig` 为 `null` 表示配置还没读回来，此时同样不发：拿一个「尚未读到」的状态当默认值，
  // 会在开关实际为关的机器上先误发一轮，等配置回来才停 —— 已经产生的请求收不回来，违背开关本意。
  // 依赖里带上 `enabled`，开关切换后 effect 才会重跑：开→关立即停，关→开立即补一次。
  useEffect(() => {
    if (!autoCheckinConfig?.enabled) return;
    if (!accounts.length) return;
    let cancelled = false;
    void fetchTodayCheckinMap(
      accounts.map((account) => account.id),
      region,
      () => cancelled,
    ).then((next) => {
      if (!cancelled && Object.keys(next).length > 0) {
        setCheckinMap((prev) => ({ ...prev, ...next }));
      }
    });
    return () => {
      cancelled = true;
    };
  }, [accounts, region, autoCheckinConfig?.enabled]);

  async function loadTravelMap(accountIds: string[], isStale?: () => boolean) {
    const next = await fetchTravelMap(accountIds, region, isStale);
    if (!isStale?.() && Object.keys(next).length > 0) {
      setTravelMap((prev) => ({ ...prev, ...next }));
    }
  }

  // 账号列表变化后并行查询旅行状态；后台领取后每 60 秒再拉一次，避免卡片停在「旅行中」。
  //
  // 同样是「被动请求」，必须受自动旅行开关约束：关掉后不仅首查不发，连这个 60 秒轮询定时器都不该建立，
  // 否则会以每分钟一次的频率持续空打上游 —— 定时器的代价比单次请求更高，更要拦在建立之前。
  // `autoTravelConfig` 为 `null`（配置尚未读回）时同样不发，理由同上：不能把「未知」当作默认开启先打一轮。
  // 依赖里带上 `enabled`，开关切换后 effect 才会重跑：关掉时清理函数会顺手 clearInterval，开关打开时重新建表。
  //
  // ★ 拆成两个 effect：**首查**与**轮询**对可见性的要求不同（见下）。
  const documentVisible = useDocumentVisible();

  // 首查：不受可见性门控 —— 否则「切到后台再切回来」会因为 effect 重跑被跳过而看到空态。
  useEffect(() => {
    if (!autoTravelConfig?.enabled) return;
    if (!accounts.length) return;
    let cancelled = false;
    const ids = accounts.map((account) => account.id);
    void loadTravelMap(ids, () => cancelled);
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, region, autoTravelConfig?.enabled]);

  // 轮询：**加可见性门控** —— 应用在后台时每分钟空打一次上游毫无意义（用户看不见）。
  // 只门控「要不要建这个定时器」，间隔 60 秒本身不动。
  useEffect(() => {
    if (!autoTravelConfig?.enabled) return;
    if (!accounts.length) return;
    if (!documentVisible) return;
    let cancelled = false;
    const ids = accounts.map((account) => account.id);
    const timer = window.setInterval(() => {
      void loadTravelMap(ids, () => cancelled);
    }, 60_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, region, autoTravelConfig?.enabled, documentVisible]);

  // 只给尚未缓存的账号拉积分；切回首页不重复请求。点「刷新积分」才强制更新。
  useEffect(() => {
    if (!accounts.length) return;
    void ensureCredits(accounts.map((account) => account.id), region);
  }, [accounts, ensureCredits, region]);

  /**
   * 「今日 Token」按账号聚合（只读）—— 卡片指标行 / 表格列的数据源。
   *
   * ⚠️ 读不到时**清空为「今日无记录」**（卡片 / 表格渲染「—」），**绝不**沿用上一次的数字：
   * 换 region 后旧数字会张冠李戴（本仓踩过字段张冠李戴的坑）。取数失败不弹错、不打扰用户。
   *
   * ## ★ `force` 的边界（2026-10-12）
   *
   * 后端 `force` 会**绕过**「距上次扫描不足 TTL」的闸门、立刻重扫客户端会话日志；
   * 默认路径**不给** —— 由后端按 TTL 自己决定（否则每次进页面都全量重扫）。
   * 只有**用户显式点刷新**才给 `true`。
   *
   * 重扫本身不贵：后端带 mtime 预筛，只读「当天动过的文件」（实测 40GB → 当天那几 MB）。
   */
  const tokenTodayGenRef = useRef(0);
  const loadTokenToday = useCallback(
    async (force: boolean) => {
      /* 代次：region 变化后晚到的旧响应**不得**覆盖新结果（否则又变成张冠李戴）。 */
      const gen = (tokenTodayGenRef.current += 1);
      const current = () => gen === tokenTodayGenRef.current;
      try {
        const res = await api.getAccountTokenToday(region, force);
        if (!current()) return;
        const map = new Map<string, AccountTokenTodayEntry>();
        for (const entry of res.accounts ?? []) map.set(entry.uid, entry);
        setTokenTodayMap(map);
        setTokenTodayTruncated(Boolean(res.gatewayTruncated));
      } catch {
        if (!current()) return;
        setTokenTodayMap(new Map());
        setTokenTodayTruncated(false);
      }
    },
    [region],
  );

  useEffect(() => {
    void loadTokenToday(false);
  }, [loadTokenToday, accounts.length]);

  async function onRecheck() {
    await Promise.all([refreshRegionStatus(region), reconcileAccounts(region)]);
  }

  async function onImport() {
    setImporting(true);
    try {
      const acc = await importLocalStore(region);
      toast.success(t("wbAccounts.toast.imported"), { description: accountDisplayName(acc) });
    } catch (e) {
      toast.error(t("wbAccounts.toast.importFail"), { description: api.asError(e) });
    } finally {
      setImporting(false);
    }
  }

  async function onAutoCheckinChange(enabled: boolean) {
    if (!autoCheckinConfig || autoCheckinSaving) return;
    const previous = autoCheckinConfig;
    const next = { ...previous, enabled };
    setAutoCheckinConfig(next);
    setAutoCheckinSaving(true);
    try {
      setAutoCheckinConfig(await api.saveAutoCheckinConfig(next));
    } catch (e) {
      setAutoCheckinConfig(previous);
      toast.error(t("wbAccounts.toast.autoCheckinSaveFail"), { description: api.asError(e) });
    } finally {
      setAutoCheckinSaving(false);
    }
  }

  async function onAutoTravelChange(enabled: boolean) {
    if (!autoTravelConfig || autoTravelSaving) return;
    const previous = autoTravelConfig;
    const next = { ...previous, enabled };
    setAutoTravelConfig(next);
    setAutoTravelSaving(true);
    try {
      setAutoTravelConfig(await api.saveAutoTravelConfig(next));
      if (enabled) {
        toast.success(t("wbAccounts.toast.autoTravelOn"), { description: t("wbAccounts.toast.autoTravelDispatching") });
        window.setTimeout(() => {
          void loadTravelMap(accounts.map((account) => account.id));
        }, 2500);
      }
    } catch (e) {
      setAutoTravelConfig(previous);
      toast.error(t("wbAccounts.toast.autoTravelSaveFail"), { description: api.asError(e) });
    } finally {
      setAutoTravelSaving(false);
    }
  }

  /** 导出完成提示（含安全提醒）。 */
  function onExported(count: number) {
    const text = t("wbAccounts.toast.exportDetail", { count });
    toast.success(t("wbAccounts.toast.exportSuccess"), { description: text });
  }

  /** 导入完成提示：计数 + token 可能过期提醒，并刷新列表。 */
  function onImported(result: { imported: number; skipped: number; overwritten: number }) {
    void reconcileAccounts(region);
    const overwriteText = result.overwritten > 0 ? t("wbAccounts.toast.importOverwrite", { n: result.overwritten }) : "";
    const text = t("wbAccounts.toast.importDetail", { imported: result.imported, overwrite: overwriteText, skipped: result.skipped });
    toast.success(t("wbAccounts.toast.importSuccess"), { description: text });
  }

  function onDelete(a: AccountMeta) {
    // 桌面 App（Tauri WebView）不支持 window.confirm，改用 Dialog 确认
    setDeleteTarget(a);
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    const a = deleteTarget;
    setDeleteTarget(null);
    try {
      await deleteAccountStore(a.id, region);
      // 软删除成功 ⇒ 弹「已删除 · 撤销」（撤销走 account_restore，与回收站同一命令）。
      offerUndo([a.id], accountUndoLabel(a));
    } catch (e) {
      toast.error(t("wbAccounts.toast.deleteFail"), { description: api.asError(e) });
    }
  }

  async function onCheckin(a: AccountMeta) {
    try {
      const res = await api.checkin(a.id, region);
      const label =
        res.result === "success"
          ? t("wbAccounts.toast.checkinSuccess")
          : res.result === "already"
            ? t("wbAccounts.toast.checkedInToday")
            : t("wbAccounts.toast.checkinFail");
      const description = `${accountDisplayName(a)}${res.error ? `${t("shared.punct.colon")}${res.error}` : ""}`;
      if (res.result === "error") toast.error(label, { description });
      else toast.success(label, { description });
      // 刷新该账号的今日签到状态
      try {
        const st = await api.getCheckinStatus(a.id, region);
        if (st.ok) setCheckinMap((prev) => ({ ...prev, [a.id]: st.todayCheckedIn }));
      } catch {
        /* ignore */
      }
      void reconcileAccounts(region);
      // 签到成功/已签到会带来积分变动，force 刷新该账号积分
      if (res.result !== "error") void refreshCredits([a.id], { region });
    } catch (e) {
      toast.error(t("wbAccounts.toast.checkinFail"), { description: api.asError(e) });
    }
  }

  async function onRefresh(a: AccountMeta) {
    try {
      const res = await api.refreshAccountToken(a.id, region);
      const label = accountDisplayName(a);
      if (res.needsRelogin) {
        // `needsReloginReason` 是后端给出的**自足**原因（自带「需重新登录…」这一步），
        // 旧实现把它塞进「需重新登录（…）」的括号里 ⇒ 同一句话在一条 toast 里出现两遍
        //（2026-09-24 用户截图：「Andev：需重新登录（缺少 refresh token，无法刷新，需重新登录）」）。
        // 有原因就**只用原因**；只有原因缺失时才退回本地化的「需重新登录」。
        toast.error(t("wbAccounts.toast.tokenRefreshFail"), {
          description: res.needsReloginReason
            ? `${label}${t("shared.punct.colon")}${res.needsReloginReason}`
            : `${label}${t("shared.punct.colon")}${t("wbAccounts.toast.needRelogin")}`,
        });
      } else {
        toast.success(t("wbAccounts.toast.tokenRefreshed"), { description: label });
      }
      void reconcileAccounts(region);
    } catch (e) {
      toast.error(t("wbAccounts.toast.tokenRefreshFail"), { description: api.asError(e) });
    }
  }

  /** 刷新按钮：先跑一轮批量签到并重查今日签到状态，再强制刷新全部积分与「今日 Token」。 */
  async function onRefreshCredits() {
    if (!accounts.length || refreshingCredits || checkinAllRunning) return;
    setCheckinAllRunning(true);
    try {
      try {
        const res = await api.checkinAll(region);
        const entries = res.accounts ?? [];
        const success = entries.filter((e) => e.result === "success").length;
        const already = entries.filter((e) => e.result === "already").length;
        const failed = entries.filter((e) => e.result === "error").length;
        const parts: string[] = [];
        if (success > 0) parts.push(t("wbAccounts.toast.checkinBatchSuccess", { n: success }));
        if (already > 0) parts.push(t("wbAccounts.toast.checkinBatchAlready", { n: already }));
        if (failed > 0) parts.push(t("wbAccounts.toast.checkinBatchFailed", { n: failed }));
        const summary = parts.length > 0 ? parts.join(t("shared.punct.comma")) : t("wbAccounts.toast.checkinNone");
        if (entries.length > 0 && failed === entries.length) {
          toast.error(t("wbAccounts.toast.checkinFailBatch"), { description: summary });
        } else {
          toast.success(t("wbAccounts.toast.checkinDone"), { description: summary });
        }
        // 批量签到后重查全部账号的今日签到状态，无需切换页面即反映最新结果
        const next = await fetchTodayCheckinMap(accounts.map((account) => account.id), region);
        if (Object.keys(next).length > 0) {
          setCheckinMap((prev) => ({ ...prev, ...next }));
        }
      } catch (e) {
        toast.error(t("wbAccounts.toast.batchCheckinFail"), { description: api.asError(e) });
      }
      await refreshCredits(accounts.map((account) => account.id), { region });
      await loadTravelMap(accounts.map((account) => account.id));
      /* 显式刷新 ⇒ 强制重算「今日 Token」（绕过后端 TTL 闸门）。 */
      await loadTokenToday(true);
      toast.success(t("wbAccounts.toast.creditsRefreshed"));
    } finally {
      setCheckinAllRunning(false);
    }
  }

  /** 点「切换 CLI」：先走**统一重启确认**（CLI 例外 —— 不重开，但当前 CLI 会话会中断）。 */
  function onSwitchCodebuddyCli(account: AccountMeta) {
    if (codebuddyCliSwitchingId !== null) return;
    setRestartConfirm({ account, client: "cli" });
  }

  /** 用户在确认框里点了「仍要切换」后，真正执行 CLI 切换。 */
  async function doSwitchCodebuddyCli(account: AccountMeta) {
    if (codebuddyCliSwitchingId !== null) return;
    setCodebuddyCliSwitchingId(account.id);
    const toastId = toast.loading(t("wbAccounts.toast.switchCliLoading"), {
      description: t("wbAccounts.toast.switchCliLoadingDesc", { name: accountDisplayName(account) }),
    });
    try {
      const result = await api.switchCodebuddyCliAccount(account.id);
      await refreshCodebuddyCliStatus();
      toast.success(t("wbAccounts.toast.switchCliUpdated"), {
        id: toastId,
        description: `${accountDisplayName(account)}${t("shared.punct.colon")}${result.message || t("wbAccounts.toast.configUpdated")}`,
      });
    } catch (error) {
      toast.error(t("wbAccounts.toast.switchCliFail"), {
        id: toastId,
        description: api.asError(error),
      });
    } finally {
      setCodebuddyCliSwitchingId(null);
    }
  }

  /** 点「切换到 CodeBuddy IDE」：先走**统一重启确认**（IDE 需重启，默认勾选自动重开）。 */
  function onSwitchCodebuddyCnIde(account: AccountMeta) {
    if (codebuddyCnIdeSwitchingId !== null) return;
    setRestartConfirm({ account, client: "ide" });
  }

  /** 用户在确认框里点了确认后，真正执行 IDE 切换（`restart` 由确认框决定）。 */
  async function doSwitchCodebuddyCnIde(account: AccountMeta, restart: boolean) {
    if (codebuddyCnIdeSwitchingId !== null) return;
    setCodebuddyCnIdeSwitchingId(account.id);
    const toastId = toast.loading(t("wbAccounts.toast.switchIdeLoading"), {
      // 取消勾选「自动重开」时**不得**说「并重启」—— 那是假承诺（§3.5）。
      description: restart
        ? t("wbAccounts.toast.switchIdeLoadingDesc")
        : t("sessions.restart.switchingNoRestart", {
            client: t("wbAccounts.program.codebuddyCnIde"),
          }),
    });
    try {
      const result = await api.switchCodebuddyCnIdeAccount(account.id, restart);
      await refreshCodebuddyCnIdeStatus();
      toast.success(t("wbAccounts.toast.switchIdeUpdated"), {
        id: toastId,
        description: result.message || result.account,
      });
    } catch (error) {
      toast.error(t("wbAccounts.toast.switchIdeFail"), {
        id: toastId,
        description: api.asError(error),
      });
    } finally {
      setCodebuddyCnIdeSwitchingId(null);
    }
  }

  /**
   * 编辑器内 CodeBuddy 插件（VS Code / JetBrains）两个程序位：**一条**切换实现，参数化客户端。
   *
   * ## 为什么合成一条
   *
   * 两者只在「切哪个程序位 / 显示什么名字」上不同，时序完全一致（确认 → 关编辑器 →
   * 写凭证 → 重开）。各写一份必然漂移（一处改了 toast 文案、另一处没改）—— 那正是
   * §3.15.3「不要各写一套」要防的。
   *
   * ## 诚实边界
   *
   * `restart=false`（用户取消「自动重开」）时**不得**说「并重启」：文案改用
   * `sessions.restart.switchingNoRestart`，如实写「稍后手动打开」。
   */
  async function runPluginSwitch(
    account: AccountMeta,
    restart: boolean,
    busyId: string | null,
    setBusy: (id: string | null) => void,
    label: string,
    doSwitch: (accountId: string, restart: boolean) => Promise<{ message?: string; account?: string }>,
    refresh: () => Promise<void>,
  ) {
    if (busyId !== null) return;
    setBusy(account.id);
    const toastId = toast.loading(t("wbAccounts.toast.switchPluginLoading", { client: label }), {
      description: restart
        ? t("wbAccounts.toast.switchPluginLoadingDesc", { client: label })
        : t("sessions.restart.switchingNoRestart", { client: label }),
    });
    try {
      const result = await doSwitch(account.id, restart);
      await refresh();
      toast.success(t("wbAccounts.toast.switchPluginUpdated", { client: label }), {
        id: toastId,
        description: result.message || result.account,
      });
    } catch (error) {
      toast.error(t("wbAccounts.toast.switchPluginFail", { client: label }), {
        id: toastId,
        description: api.asError(error),
      });
    } finally {
      setBusy(null);
    }
  }

  /** 点「切换 VS Code 扩展」：先走**统一重启确认**（写 `state.vscdb` 前 VS Code 必须退出）。 */
  function onSwitchVscodePlugin(account: AccountMeta) {
    if (vscodePluginSwitchingId !== null) return;
    setRestartConfirm({ account, client: "vscode" });
  }

  function doSwitchVscodePlugin(account: AccountMeta, restart: boolean) {
    void runPluginSwitch(
      account,
      restart,
      vscodePluginSwitchingId,
      setVscodePluginSwitchingId,
      t("wbAccounts.program.vscodeCodebuddyPlugin"),
      api.switchVscodeCodebuddyPluginAccount,
      refreshVscodePluginStatus,
    );
  }

  /** 点「切换 JetBrains 插件」：同上（IDE 运行中写入会被内存态覆盖 ⇒ 必须先关）。 */
  function onSwitchJetbrainsPlugin(account: AccountMeta) {
    if (jetbrainsPluginSwitchingId !== null) return;
    setRestartConfirm({ account, client: "jetbrains" });
  }

  function doSwitchJetbrainsPlugin(account: AccountMeta, restart: boolean) {
    void runPluginSwitch(
      account,
      restart,
      jetbrainsPluginSwitchingId,
      setJetbrainsPluginSwitchingId,
      t("wbAccounts.program.jetbrainsCodebuddyPlugin"),
      api.switchJetbrainsCodebuddyPluginAccount,
      refreshJetbrainsPluginStatus,
    );
  }

  /**
   * 未检测到编辑器扩展时的出口（§3.3 规则 1：**不摆点不动的灰按钮**）。
   *
   * ⚠️ 与「去设置」分开：设置页里**没有**「安装 VS Code / JetBrains 扩展」这类子项 ⇒
   * 把用户送到设置页帮不上忙。真正指向正确动作的是**说明**：先去编辑器里装扩展。
   */
  function onGuidePlugin(programId: string) {
    const client =
      programId === "vscode_codebuddy_plugin"
        ? t("wbAccounts.program.vscodeCodebuddyPlugin")
        : t("wbAccounts.program.jetbrainsCodebuddyPlugin");
    toast.info(t("wbAccounts.card.guidePlugin", { client }));
  }

  /**
   * 统一重启确认框里的客户端显示名 —— 与注册表 `labelKey` **同一份文案**（不另抄一份）。
   */
  function switchClientLabel(client: SwitchClient): string {
    switch (client) {
      case "cli":
        return t("wbAccounts.program.codebuddyCli");
      case "ide":
        return t("wbAccounts.program.codebuddyCnIde");
      case "vscode":
        return t("wbAccounts.program.vscodeCodebuddyPlugin");
      default:
        return t("wbAccounts.program.jetbrainsCodebuddyPlugin");
    }
  }

  async function onInstallCodebuddyCli() {
    // 桌面 App（Tauri WebView）不支持 window.confirm，改用 Dialog 确认
    setInstallConfirmOpen(true);
  }

  async function confirmInstallCodebuddyCli() {
    setInstallConfirmOpen(false);
    setInstallingCodebuddyCli(true);
    try {
      const result = await api.installCodebuddyCliHelper();
      toast.success(t("wbAccounts.toast.cliIntegrationUpdated"), { description: result.message });
      await refreshCodebuddyCliStatus();
    } catch (error) {
      toast.error(t("wbAccounts.toast.cliIntegrationFail"), { description: api.asError(error) });
    } finally {
      setInstallingCodebuddyCli(false);
    }
  }

  const current = status?.current;

  /**
   * 保存账号备注。
   *
   * 成功后**重新拉取账号列表**而不是就地改 state：备注是唯一由用户手改的字段，
   * 让后端回传的脱敏 meta 成为唯一真相源，可以避免两边说法不一致
   * （后端会把全空白备注归一成「没有备注」，前端就地改就会留下一个空串）。
   *
   * 返回布尔值而不是抛异常：失败时卡片要保持编辑态，让用户改完重试，
   * 而不是把已输入的文字丢掉。
   */
  async function onSaveRemark(account: AccountMeta, remark: string): Promise<boolean> {
    try {
      await api.setAccountRemark(account.id, remark, region);
      await reconcileAccounts(region);
      toast.success(remark.trim() ? t("wbAccounts.toast.remarkSaved") : t("wbAccounts.toast.remarkCleared"));
      return true;
    } catch (e) {
      toast.error(t("wbAccounts.toast.remarkSaveFail"), { description: api.asError(e) });
      return false;
    }
  }

  /**
   * 保存账号自定义分组（**字段级更新**，与 {@link onSaveRemark} 同一交互 / 同一口径）。
   *
   * 成功后重取账号库（后端把空串归一成「未分组」⇒ 让后端返回的 meta 成为唯一真相源）。
   * 返回布尔值而非抛异常：失败时抽屉保持编辑态，让用户改完重试。
   */
  async function onSaveGroup(account: AccountMeta, group: string): Promise<boolean> {
    try {
      await api.setAccountGroup(account.id, group, region);
      await reconcileAccounts(region);
      toast.success(
        group.trim() ? t("wbAccounts.toast.groupSaved") : t("wbAccounts.toast.groupCleared"),
      );
      return true;
    } catch (e) {
      toast.error(t("wbAccounts.toast.groupSaveFail"), { description: api.asError(e) });
      return false;
    }
  }

  const creditOrderingReady =
    accounts.length > 0 &&
    accounts.every((account) => Boolean(creditMap[account.id]) && !creditLoadingMap[account.id]);

  const cliCurrentAccountId = codebuddyCli?.activeAccountId;
  // 归一是必须的：`t()` 的插值走 `String(params[name])`，脏值会原样变成
  // 「已登录: [object Object]」（issue #2 用户截图里的那一行）。
  const workbuddyCurrentName = current
    ? displayText(current.nickname) ||
      displayText(current.email) ||
      displayText(current.uid) ||
      t("wbAccounts.common.unknownAccount")
    : t("wbAccounts.common.notLoggedIn");
  const cnIdeCurrentAccountId = codebuddyCnIde?.activeAccountId;
  /* ★ 2026-10-09：编辑器内两个插件的「当前账号」——同样来自**本地状态文件**
     （`status.activeAccountId`），不为了这一行去解密 secret（那会弹钥匙串授权）。 */
  const vscodePluginCurrentAccountId = vscodePlugin?.activeAccountId;
  const jetbrainsPluginCurrentAccountId = jetbrainsPlugin?.activeAccountId;
  const codebuddyUsesSettingsEnv = codebuddyCli?.authMode === "settings-env";

  /** 账号展示名（表格 / 无障碍标签共用；过 `displayText` 归一，防脏值白屏）。 */
  const wbAccountName = (account: AccountMeta): string =>
    displayText(account.nickname) ||
    displayText(account.email) ||
    displayText(account.uid) ||
    displayText(account.id) ||
    t("wbAccounts.card.unnamed");

  /**
   * 列表视图配置 —— 平台差异**全部以数据表达**（`P4-TOOLBAR-ARCH §3.1`）。
   *
   * 共享组件不写平台判断，只按本配置渲染。排序比较器与置顶的语义与改造前逐条对齐：
   * - `priority` = 现状写死的「积分优先级」（快过期 / 建议优先靠前）；`creditOrderingReady`
   *   为假时返回 0 ⇒ 稳定排序保持原序（与旧实现的 `: accounts` 分支等价）；
   * - `name` / `expiry` 是新增键；
   * - `recent` = 按 `AccountMeta.lastUsedAt`（epoch 毫秒）**倒序**；从未使用的账号（缺该字段）
   *   排到最后。⚠️ 2026-10-10 起后端为 `AccountMeta` 记录 `lastUsedAt`，故「最近使用」不再是
   *   假选项（决策 #3 的原判据「无字段」已不成立 —— 见 `check-scope-registry.cjs` 对应断言）。
   */
  const listConfig = useMemo<AccountListConfig<AccountMeta>>(() => {
    const priorityComparator: AccountComparator<AccountMeta> = (left, right) => {
      const leftCredit = creditMap[left.id];
      const rightCredit = creditMap[right.id];
      const rankDifference = creditPriorityRank(leftCredit) - creditPriorityRank(rightCredit);
      if (rankDifference !== 0) return rankDifference;
      const leftExpiry = soonestRelevantExpiry(leftCredit);
      const rightExpiry = soonestRelevantExpiry(rightCredit);
      if (leftExpiry !== rightExpiry) return leftExpiry - rightExpiry;
      return expiringSoonAmount(rightCredit) - expiringSoonAmount(leftCredit);
    };
    const nameComparator: AccountComparator<AccountMeta> = (left, right) =>
      wbAccountName(left).localeCompare(wbAccountName(right), undefined, { numeric: true, sensitivity: "base" });
    const expiryComparator: AccountComparator<AccountMeta> = (left, right) =>
      (left.expiresAt ?? Number.POSITIVE_INFINITY) - (right.expiresAt ?? Number.POSITIVE_INFINITY);
    /* 「最近使用」：按 `lastUsedAt` **倒序**（新的在前）；缺席（从未使用）用 `-Infinity`
       兜底 ⇒ 一律排到最后，而不是被当成「很久以前」（0 会混进中间去）。 */
    const recentComparator: AccountComparator<AccountMeta> = (left, right) =>
      (right.lastUsedAt ?? Number.NEGATIVE_INFINITY) - (left.lastUsedAt ?? Number.NEGATIVE_INFINITY);
    return {
      searchText: (account) =>
        accountSearchText({
          name: displayText(account.nickname),
          email: displayText(account.email),
          uid: displayText(account.uid),
          remark: displayText(account.remark),
        }),
      comparators: {
        priority: creditOrderingReady ? priorityComparator : () => 0,
        name: nameComparator,
        expiry: expiryComparator,
        recent: recentComparator,
      },
      /**
       * `pin_current_account` 打开时把当前登录账号整体提到第一位。
       *
       * 这是**显式覆盖**而不是往比较函数里插一条分支：置顶与积分排序是同一根轴的两端
       * （「该切到谁」vs「正在用谁」），插进比较函数会让两者互相打架，结果取决于哪条规则
       * 先命中 —— 那种「有时候置顶、有时候不置顶」最难排查。覆盖之后其余账号的相对顺序
       * 完全不动，⭐「建议优先」徽章也照旧渲染。
       */
      pinFirst: switchConfig?.pin_current_account
        ? (rows) => {
            const pinned = rows.find((account) => isWorkbuddyCurrent(account, current));
            if (!pinned || rows[0]?.id === pinned.id) return rows;
            return [pinned, ...rows.filter((account) => account.id !== pinned.id)];
          }
        : undefined,
      bucketKey: (account, key) => {
        if (key === "program") {
          const labels: string[] = [];
          if (isWorkbuddyCurrent(account, current)) labels.push("WorkBuddy");
          if (account.id === cnIdeCurrentAccountId) labels.push("CodeBuddy IDE");
          if (account.id === cliCurrentAccountId) labels.push("CodeBuddy CLI");
          return labels.length > 0 ? labels.join(" + ") : t("wbAccounts.toolbar.programBucket.unbound");
        }
        if (key === "status") {
          if (account.needsRelogin) return t("wbAccounts.toolbar.statusBucket.needsRelogin");
          if (typeof account.expiresAt === "number" && account.expiresAt < Date.now()) {
            return t("wbAccounts.toolbar.statusBucket.expired");
          }
          if (checkinMap[account.id] === false) return t("wbAccounts.toolbar.statusBucket.notCheckedIn");
          return t("wbAccounts.toolbar.statusBucket.normal");
        }
        if (key === "group") {
          /* 按用户自定义分组名分桶；**空 / 缺省 = 未分组**落到显式「未分组」桶
             （不给它一个空桶名 ⇒ 必须有一个可读的桶标题）。 */
          const name = displayText(account.groupName)?.trim();
          return name ? name : t("wbAccounts.toolbar.groupBucket.ungrouped");
        }
        return null;
      },
      availableSorts: ["priority", "name", "expiry", "recent"],
      availableGroups: ["none", "program", "status", "group"],
    };
  }, [creditOrderingReady, creditMap, switchConfig, current, cnIdeCurrentAccountId, cliCurrentAccountId, checkinMap, t]);

  /** 过滤 → 排序 → 分桶（视图参数全部留在页内，纯函数不改后端）。 */
  const filtered = filterAccounts(accounts, listConfig, query);
  const sorted = sortAccounts(filtered, sort, listConfig);
  const buckets = groupAccounts(sorted, group, listConfig);
  /** 只服务「建议优先」徽章：按积分优先级排一遍（与当前 `sort` 无关）。 */
  const priorityOrdered = sortAccounts(accounts, "priority", listConfig);
  const priorityAccountId = creditOrderingReady
    ? priorityOrdered.find((account) => hasExpiringSoonCredits(creditMap[account.id]))?.id
    : undefined;

  /** 排序 / 分组下拉选项 —— 由 `availableSorts` / `availableGroups`（数据）驱动，无假选项。 */
  const sortLabels: Partial<Record<AccountSortKey, string>> = {
    priority: t("wbAccounts.toolbar.sort.priority"),
    name: t("wbAccounts.toolbar.sort.name"),
    expiry: t("wbAccounts.toolbar.sort.expiry"),
    recent: t("wbAccounts.toolbar.sort.recent"),
  };
  const groupLabels: Partial<Record<AccountGroupKey, string>> = {
    none: t("wbAccounts.toolbar.group.none"),
    program: t("wbAccounts.toolbar.group.program"),
    status: t("wbAccounts.toolbar.group.status"),
    group: t("wbAccounts.toolbar.group.custom"),
  };
  const sortOptions: ToolbarOption[] = listConfig.availableSorts.flatMap((key) => {
    const label = sortLabels[key];
    return label ? [{ value: key, label }] : [];
  });
  const groupOptions: ToolbarOption[] = listConfig.availableGroups.flatMap((key) => {
    const label = groupLabels[key];
    return label ? [{ value: key, label }] : [];
  });

  /** 全选：只作用于**当前可见**（过滤后）的行；与表头全选读写同一份 `selectedKeys`。 */
  const allVisibleSelected =
    sorted.length > 0 && sorted.every((account) => selectedKeys.has(selectionKey(account.id)));
  const someVisibleSelected =
    !allVisibleSelected && sorted.some((account) => selectedKeys.has(selectionKey(account.id)));
  function toggleVisibleSelection() {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      for (const account of sorted) {
        if (allVisibleSelected) next.delete(selectionKey(account.id));
        else next.add(selectionKey(account.id));
      }
      return next;
    });
  }
  function bucketAllSelected(items: AccountMeta[]): boolean {
    return items.length > 0 && items.every((account) => selectedKeys.has(selectionKey(account.id)));
  }
  function bucketSomeSelected(items: AccountMeta[]): boolean {
    return !bucketAllSelected(items) && items.some((account) => selectedKeys.has(selectionKey(account.id)));
  }
  function toggleBucketSelection(items: AccountMeta[], checked: boolean) {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      for (const account of items) {
        if (checked) next.add(selectionKey(account.id));
        else next.delete(selectionKey(account.id));
      }
      return next;
    });
  }

  /** 「清除筛选」出口：复位搜索与分组（`§1.9` 两种空态绝不混用）。 */
  function clearListFilters() {
    if (!hasActiveFilter({ query, group })) return;
    setQuery("");
    setGroup("none");
  }

  /** WB 状态徽标（表格用；与卡片同一套语义，只是排布不同）。 */
  function renderStatusBadges(account: AccountMeta) {
    const checkedIn = checkinMap[account.id];
    const expired = typeof account.expiresAt === "number" && account.expiresAt < Date.now();
    const travel = travelMap[account.id];
    return (
      <div className="flex flex-wrap items-center gap-1">
        {checkedIn !== undefined && (
          <Badge
            variant={checkedIn ? "success" : "secondary"}
            className={cn("rounded-md px-1.5 py-0 text-[11px] font-medium", !checkedIn && "text-muted-foreground")}
          >
            {checkedIn ? t("wbAccounts.card.checkedIn") : t("wbAccounts.card.notCheckedIn")}
          </Badge>
        )}
        {travel?.label === "traveling" && (
          <Badge variant="secondary" className="rounded-md px-1.5 py-0 text-[11px] font-medium text-muted-foreground">
            {t("wbAccounts.card.traveling")}
          </Badge>
        )}
        {(account.needsRelogin || expired) && (
          <Badge variant="warning" className="rounded-md px-1.5 py-0 text-[11px] font-medium">
            {account.needsRelogin ? t("wbAccounts.card.needRelogin") : t("wbAccounts.card.tokenExpired")}
          </Badge>
        )}
      </div>
    );
  }

  /** 表格「到期」列：Token 到期（长期有效 → 「长期有效」；已过期 → 「Token 已过期」）。 */
  function formatAccountExpiry(account: AccountMeta): string {
    if (typeof account.expiresAt !== "number") return t("wbAccounts.card.forever");
    if (account.expiresAt < Date.now()) return t("wbAccounts.card.tokenExpired");
    return formatDate(account.expiresAt);
  }

  /**
   * 表格列定义（`§1.8`）。列头文案已 i18n；单元格全部返回**已归一**节点
   * （字符串 / 元素），**绝不返回对象** —— 对象当 React 子节点会卸载整棵树 ⇒ 白屏。
   *
   * ⚠️「程序位」列是**只读徽标**（无 `onClick`、无按钮）：表格行只有 2 个可点区
   * （勾选 + 行本身），门禁对此有静态断言。
   */
  const columns: AccountColumn<AccountMeta>[] = [
    {
      key: "account",
      header: t("wbAccounts.table.account"),
      render: (account) => (
        <div className="flex min-w-0 items-center gap-2.5">
          {/* 表格头像走共享件（原型 `.avatar`：26px / 圆角 8px / 2 字）——
              此前这里写的是 `rounded-full` + 1 字，与原型不符，也与卡片头像规格混用。 */}
          <AccountsTableAvatar name={wbAccountName(account)} />
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <span className="truncate font-medium" title={wbAccountName(account)}>
                {wbAccountName(account)}
              </span>
              {isWorkbuddyCurrent(account, current) && (
                <Badge variant="secondary" className="rounded-md px-1.5 py-0 text-[11px] font-medium">
                  {t("wbAccounts.table.current")}
                </Badge>
              )}
            </div>
            <div className="truncate text-xs text-muted-foreground" title={accountIdentity(account)}>
              {accountIdentity(account)}
            </div>
          </div>
        </div>
      ),
    },
    {
      key: "status",
      header: t("wbAccounts.table.status"),
      render: (account) => renderStatusBadges(account),
    },
    {
      key: "credits",
      header: t("wbAccounts.table.credits"),
      align: "right",
      render: (account) => (
        <span className="tabular-nums">{formatCredits(creditMap[account.id]?.totalRemaining ?? 0)}</span>
      ),
    },
    {
      /* 「今日 Token」列：与卡片指标行**同一份**诚实实现（`formatTodayTokens`）——
         账号缺席 ⇒ 「—」（绝不当 0）；网关被裁剪 ⇒ 下界 `≥N`（`title` 说明下界口径）。 */
      key: "todayTokens",
      header: t("wbAccounts.table.todayTokens"),
      align: "right",
      render: (account) => {
        const entry = account.uid ? tokenTodayMap.get(account.uid) : undefined;
        return (
          <span
            className={cn("tabular-nums", !entry && "text-muted-foreground")}
            title={entry ? todayTokensBreakdown(entry, tokenTodayTruncated, t) : undefined}
          >
            {formatTodayTokens(entry, tokenTodayTruncated, t)}
          </span>
        );
      },
    },
    {
      key: "expiry",
      header: t("wbAccounts.table.expiry"),
      render: (account) => (
        <span className="tabular-nums text-muted-foreground">{formatAccountExpiry(account)}</span>
      ),
    },
    {
      key: "programs",
      header: t("wbAccounts.table.programs"),
      render: (account) => {
        const active: string[] = [];
        if (isWorkbuddyCurrent(account, current)) active.push("WorkBuddy");
        if (account.id === cnIdeCurrentAccountId) active.push("CodeBuddy IDE");
        if (account.id === cliCurrentAccountId) active.push("CodeBuddy CLI");
        return active.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {active.map((label) => (
              <Badge key={label} variant="secondary" className="rounded-md px-1.5 py-0 text-[11px] font-medium">
                {label}
              </Badge>
            ))}
          </div>
        ) : (
          <span className="text-muted-foreground">—</span>
        );
      },
    },
  ];

  const presence = regionPresence(status, accounts);
  /**
   * 账号库为空 **且** 当前没有任何登录态，才显示「未检测到 / 未登录」空态。
   *
   * ⚠️ 只看 `accounts.length` 会漏掉「客户端已登录、但账号还没保存进账号库」的用户
   * （从未点过「从本机导入」，或账号库刚丢）—— 那时页签写着「已登录: …」，
   * 下面却是一张「未检测到 WorkBuddy …」的卡片，同一屏自相矛盾
   * （2026-09-24 用户报障）。已登录时改走正常面板 + 下方的「当前登录」提示卡片。
   *
   * ⚠️⚠️ `!error` 这一项同样不可省：**「这次没取到」与「确实没有账号」是两件事**。
   * 少了它，账号库读取失败时会渲染出「未检测到 WorkBuddy / 未登录」外加一串
   * 「可能原因：未安装客户端…」—— 那是在**诊断一个不存在的问题**；而真正的错误
   * 反而一句都不显示（下方 `error &&` 那条 Alert 位于**非空态**分支内，
   * 空态分支根本走不到它）。用户会去重装客户端，而问题只是取数失败。
   * 加了门控后，失败时会落到正常分支并显示错误 Alert + 「重新检测」。
   */
  const showEmpty = accounts.length === 0 && !current && !error;
  const mismatch = status?.regionMismatch ?? null;
  const expectedAuthFile = status?.authFile || descriptor.authFilename;

  // ---- 详情抽屉的数据装配（字段全部过 `displayText` 出口，见 display-text.ts）----
  const detail = detailAccount;
  const detailName = detail
    ? displayText(detail.nickname) || displayText(detail.uid) || t("wbAccounts.card.unnamed")
    : "";
  const detailCredit = detail ? creditMap[detail.id] : undefined;
  const detailLoading = detail ? Boolean(creditLoadingMap[detail.id]) : false;
  const detailSections: DetailSection[] = detail
    ? [
        {
          key: "account",
          title: t("wbAccounts.detail.section.account"),
          rows: [
            { label: t("wbAccounts.detail.email"), value: displayText(detail.email) ?? "—", mono: true },
            { label: t("wbAccounts.detail.uid"), value: displayText(detail.uid) ?? "—", mono: true },
            { label: t("wbAccounts.detail.id"), value: displayText(detail.id) ?? "—", mono: true },
            {
              label: t("wbAccounts.detail.tokenExpiry"),
              value:
                typeof detail.expiresAt === "number"
                  ? detail.expiresAt < Date.now()
                    ? t("wbAccounts.detail.expired")
                    : formatDate(detail.expiresAt)
                  : t("wbAccounts.card.forever"),
            },
            {
              label: t("wbAccounts.detail.checkedIn"),
              value:
                checkinMap[detail.id] === true
                  ? t("wbAccounts.card.checkedIn")
                  : checkinMap[detail.id] === false
                    ? t("wbAccounts.card.notCheckedIn")
                    : "—",
            },
          ],
        },
      ]
    : [];
  const detailPacks: DetailPack[] =
    detail && detailCredit?.ok
      ? (detailCredit.resources ?? [])
          .filter((resource) => resource.remaining > 0)
          .map((resource, index) => ({ resource, index }))
          .sort((left, right) => {
            const leftExpiry = left.resource.expireAt ?? Number.POSITIVE_INFINITY;
            const rightExpiry = right.resource.expireAt ?? Number.POSITIVE_INFINITY;
            return leftExpiry === rightExpiry ? left.index - right.index : leftExpiry - rightExpiry;
          })
          .map(({ resource }) => ({
            key: `${resource.packageCode ?? resource.packageName ?? "pack"}-${resource.expireAt ?? "none"}`,
            // `packageName` / `packageCode` 声明为 `string | null`，但后端可能吐对象
            // （`api.ts` 只归一了账号字段，没归一 credit 的这两个）⇒ 直接当 React 子节点会
            // 卸载整棵树（issue #2 白屏）。过 `displayText` 出口归一，取不到再回落。
            name: displayText(resource.packageName) || displayText(resource.packageCode) || t("wbAccounts.card.creditPackFallback"),
            remainingText: formatCredits(resource.remaining),
            totalText: formatCredits(resource.total),
            usedText: t("wbAccounts.card.used", { n: formatCredits(resource.used) }),
            // 到期文案不再区分「几天内」——是否临期只体现在**颜色**上（见 `DetailPack.expiringSoon`）。
            expiryText: resource.expired
              ? t("wbAccounts.card.expired")
              : resource.expireAt
                ? t("wbAccounts.card.expiresOn", { date: formatCreditExpiry(resource.expireAt, t) })
                : t("wbAccounts.card.forever"),
            expired: resource.expired,
            // ★「3 天内到期」用黄色警示。后端 `expiringSoon` 是 7 天口径，且卡片徽章还在用，
            //   不动它 —— 抽屉的 3 天口径在这里就地判定（WB 侧 `expireAt` 是毫秒）。
            expiringSoon:
              !resource.expired && resource.expireAt != null && resource.expireAt - Date.now() <= 3 * 86_400_000,
            ratio: resource.total > 0 ? Math.min(100, Math.max(0, (resource.remaining / resource.total) * 100)) : 0,
          }))
      : [];
  const detailPrograms: DetailProgramBadge[] = detail
    ? [
        {
          key: "workbuddy",
          label: "WorkBuddy",
          current: isWorkbuddyCurrent(detail, current),
          installed: true,
          hasDataDir: true,
        },
        {
          key: "ide",
          label: "CodeBuddy IDE",
          current: detail.id === cnIdeCurrentAccountId,
          installed: Boolean(codebuddyCnIde?.installed),
          hasDataDir: Boolean(codebuddyCnIde?.installed),
        },
        {
          key: "cli",
          label: "CodeBuddy CLI",
          current: detail.id === cliCurrentAccountId,
          installed: Boolean(codebuddyCli?.configured),
          hasDataDir: Boolean(codebuddyCli?.configured),
        },
      ]
    : [];

  /**
   * ★ 抽屉：账号是否「当前登录」= 绑定了**任一**程序位（与卡片 / 原型 :3467 同口径）。
   */
  const detailIsCurrent = detail
    ? isWorkbuddyCurrent(detail, current) ||
      detail.id === cnIdeCurrentAccountId ||
      detail.id === cliCurrentAccountId ||
      detail.id === vscodePluginCurrentAccountId ||
      detail.id === jetbrainsPluginCurrentAccountId
    : false;

  /**
   * ★ 抽屉「切换到此账号」程序位（可点）。
   *
   * 每个程序的 `onSwitch` 只在**可用**时提供 —— 不可用的渲染成「未检测到」的禁用项，
   * 不把用户引向一个必然失败的切换（与卡片 `.prog.off` 的措辞同一纪律）。
   * 回调全部复用页面**既有**的切换处理函数（各程序位自己的确认流程）。
   */
  const detailSwitchTargets: DetailSwitchTarget[] = detail    ? [
        {
          key: "workbuddy",
          label: "WorkBuddy",
          current: isWorkbuddyCurrent(detail, current),
          available: true,
          onSwitch: () => setSwitchAccount(detail),
        },
        {
          key: "ide",
          label: "CodeBuddy IDE",
          current: detail.id === cnIdeCurrentAccountId,
          available: Boolean(codebuddyCnIde?.installed),
          busy: codebuddyCnIdeSwitchingId !== null,
          loading: codebuddyCnIdeSwitchingId === detail.id,
          onSwitch: codebuddyCnIde?.installed ? () => onSwitchCodebuddyCnIde(detail) : undefined,
        },
        {
          key: "cli",
          label: "CodeBuddy CLI",
          current: detail.id === cliCurrentAccountId,
          available: Boolean(codebuddyCli?.configured && !codebuddyCli.migrationRequired && !codebuddyCli.syncPending),
          busy: codebuddyCliSwitchingId !== null,
          loading: codebuddyCliSwitchingId === detail.id,
          onSwitch:
            codebuddyCli?.configured && !codebuddyCli.migrationRequired && !codebuddyCli.syncPending
              ? () => onSwitchCodebuddyCli(detail)
              : undefined,
        },
        {
          key: "vscode",
          label: t("wbAccounts.program.vscodeCodebuddyPlugin"),
          current: detail.id === vscodePluginCurrentAccountId,
          available: Boolean(vscodePlugin?.extensionInstalled),
          busy: vscodePluginSwitchingId !== null,
          loading: vscodePluginSwitchingId === detail.id,
          onSwitch: vscodePlugin?.extensionInstalled ? () => onSwitchVscodePlugin(detail) : undefined,
        },
        {
          key: "jetbrains",
          label: t("wbAccounts.program.jetbrainsCodebuddyPlugin"),
          current: detail.id === jetbrainsPluginCurrentAccountId,
          available: Boolean(jetbrainsPlugin?.pluginInstalled),
          busy: jetbrainsPluginSwitchingId !== null,
          loading: jetbrainsPluginSwitchingId === detail.id,
          onSwitch: jetbrainsPlugin?.pluginInstalled ? () => onSwitchJetbrainsPlugin(detail) : undefined,
        },
      ]
    : [];

  /**
   * ★ P14 S4a · 插件会话复制的来源客户端（`null` ⇒ 入口不渲染）。
   *
   * ⚠️ **`jetbrains` 刻意不在映射里**：会话仓的客户端段只有 `VSCode` / `CodeBuddyIDE`
   * 两个值（见 `lib/ext-sessions.ts`）⇒ JetBrains 插件**没有**对应的会话树。
   * 给它入口等于去读一棵不属于它的树 —— 比没有这个功能坏得多。
   *
   * ⚠️ **与原型的一处已知差异（未消除）**：原型用「该账号**绑定了**插件程序位」作条件；
   * 真实页面的 `switchTargets` 没有「本账号是否绑定」这个位，只有「该程序位是否已接入」
   * ⇒ 这里退化为「**该程序位已接入**」。记在 `P14-TASKS.md` 的待办里。
   */
  const detailExtClient: ExtClientId | null = (() => {
    const map: Record<string, ExtClientId> = { ide: "codebuddy-ide", vscode: "vscode" };
    for (const target of detailSwitchTargets) {
      const client = map[target.key];
      if (client && target.available) return client;
    }
    return null;
  })();

  const detailExpired =
    detail != null && typeof detail.expiresAt === "number" && detail.expiresAt < Date.now();

  /**
   * ★ 抽屉「操作」区：回调全部复用页面**既有**处理函数（刷新 / 签到 / 导出 / 权限诊断 / 删除）。
   * 「手动签到」按 F5 口径门控（过期 / 需重登不给入口）—— 与卡片菜单、批量签到同一条规则。
   */
  const detailActions: DetailActions | null = detail
    ? {
        onRefresh: () => onRefresh(detail),
        onCheckin: () => onCheckin(detail),
        showCheckin: checkinMap[detail.id] === false && !detailExpired && !detail.needsRelogin,
        onExport: () => {
          setExportTargets([detail]);
          setExportOpen(true);
        },
        onPermission: () => onGuideToSetting(),
        onDelete: () => onDelete(detail),
        credentialUnavailable: Boolean(detail.needsRelogin || detailExpired),
      }
    : null;

  /**
   * ★ 抽屉「签到历史（近 7 天）」。
   *
   * 日期 + 结果来自 `getCheckinLogs()`（真实）；「获得积分」按**账号 + 日期**取积分统计的签到事件
   * `amount`，取不到时为 `null` ⇒ 渲染「—」（**绝不**臆造固定奖励）。
   */
  const detailCheckinHistory: DetailCheckinHistoryRow[] | null =
    detail && detailCheckinLogs
      ? lastSevenDays().map((day) => {
          const log = detailCheckinLogs
            .filter(
              (entry) =>
                localDateKey(entry.ts) === day &&
                (entry.accountId === detail.id ||
                  (entry.accountId == null && Boolean(detail.email) && entry.email === detail.email)),
            )
            .sort((left, right) => right.ts - left.ts)[0];
          const event = (detailCreditEvents ?? []).find(
            (item) => item.kind === "checkin" && item.date === day && item.accountId === detail.id,
          );
          const amount = event && event.kind === "checkin" ? event.amount ?? null : null;
          if (!log) {
            return { date: day, resultLabel: t("shared.drawer.checkin.result.none"), tone: "none", amount };
          }
          if (log.result === "success") {
            return { date: day, resultLabel: t("shared.drawer.checkin.result.success"), tone: "ok", amount };
          }
          if (log.result === "already") {
            return { date: day, resultLabel: t("shared.drawer.checkin.result.already"), tone: "warn", amount };
          }
          return { date: day, resultLabel: t("shared.drawer.checkin.result.fail"), tone: "fail", amount };
        })
      : null;

  /**
   * 已勾选的账号（按当前列表顺序）—— 批量条上的动作共用这一份，避免各处再 filter 一遍。
   */
  const selectedAccounts = useMemo(
    () => accounts.filter((a) => selectedKeys.has(selectionKey(a.id))),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [accounts, selectedKeys, scope],
  );

  return (
    <>
      {/* 区域不匹配（安全红线） */}
      {mismatch && (
        <Alert variant="warning" className="mb-4">
          <AlertTriangle />
          <AlertTitle>{t("wbAccounts.page.mismatchTitle")}</AlertTitle>
          <AlertDescription>
            <p>
              {t("wbAccounts.page.mismatchBody1", { region: regionDescriptor(mismatch.actualRegion ?? "global").displayName, version: descriptor.versionLabel, domain: mismatch.actualDomain })}
            </p>
            <p className="mt-1">
              {t("wbAccounts.page.mismatchBody2", { env: mismatch.envVar || descriptor.authEnv, version: descriptor.versionLabel })}
            </p>
            <Button className="mt-2" size="sm" variant="outline" onClick={() => setMismatchDetailOpen(true)}>
              {t("wbAccounts.page.mismatchDetail")}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {/* ★ 账号工作台（**两平台共用容器**）：布局与交互全部下沉到 `AccountWorkbench`，
          本页只负责取数 + 装配 props + 注入平台插槽。

          ⚠️ 改造前这里是 450 行手写 JSX，`TraeAccountsPage` 里有一份「差不多但不一样」的副本
          —— 骨架形状、空态出口、批量条位置、分组标题样式逐项漂移。原型的账号页是
          **平台无关**的（只有 `viewAccounts()` + `curPlatform().programs`），本容器就是那个
          「一段代码」。 */}
      <AccountWorkbench
        query={query}
        onQueryChange={setQuery}
        searchInputRef={searchRef}
        sort={sort}
        sortOptions={sortOptions}
        onSortChange={(next) => setSort(next as AccountSortKey)}
        group={group}
        groupOptions={groupOptions}
        onGroupChange={(next) => setGroup(next as AccountGroupKey)}
        view={view}
        onViewChange={setView}
        tableViewEnabled={tableEnabled}
        selection={{
          allSelected: allVisibleSelected,
          someSelected: someVisibleSelected,
          onToggleAll: toggleVisibleSelection,
          label: allVisibleSelected ? t("shared.toolbar.deselectAll") : t("shared.toolbar.selectAll"),
        }}
        addAction={{
          label: t("wbAccounts.toolbar.addAccount"),
          onClick: () => setAddDrawerOpen(true),
        }}
        /* ★ 一枚**常驻**控件（用户要求「回收站放出来」）——
           走结构化 prop 注入，几何与文案都留在工具栏壳里；其余入口仍在抽屉的「更多操作」。 */
        onOpenRecycleBin={() => setRecycleOpen(true)}
        /* ★ 工具栏**只有** 搜索 / 排序 / 分组 / 视图 / 回收站 / 全选 / 主操作。
           原先挂在这里的「⋯」溢出菜单已删除，其余入口改由「添加账号」抽屉承载
           （见下方 `<AddAccountDrawer moreTools={…}>`）。 */
        loading={loading}
        /* 首屏空态（未安装 / 未登录）：整块替换工具栏与列表（原型 `emptyBlock` 也不带工具栏）。 */
        emptyState={
          showEmpty ? (
            <EmptyRegionCard
              region={region}
              installed={presence !== "absent"}
              expectedAuthFile={expectedAuthFile}
              onRecheck={() => void onRecheck()}
              onImport={() => void onImport()}
              importing={importing}
            />
          ) : null
        }
        notices={
          <>
          {error && (
            <Alert variant="destructive" className="mb-4">
              <AlertTitle>{t("wbAccounts.page.loadFailed")}</AlertTitle>
              <AlertDescription>
                <p>{error}</p>
                {/* 失败态必须给一条出路：没有它，用户唯一能做的就是重启应用。 */}
                <Button
                  className="mt-2"
                  size="sm"
                  variant="outline"
                  onClick={() => void onRecheck()}
                >
                  {t("wbAccounts.empty.recheck")}
                </Button>
              </AlertDescription>
            </Alert>
          )}

          {/* ★ CodeBuddy CLI 接入提示：**压成一行细条**，不再占一整张卡。
            原型没有这个场景（`viewAccounts` 里没有 CLI 接入提示），但本仓账号页确实会撞上
            「装了 CodeBuddy 却没接入 CLI」—— 这是**真实待办**，不能删。
            ⇒ 保留信息与出口，但按原型 `.note-warn`（`demo.css:833-836`）那种
            「一行说明 + 一个按钮」的密度：11.5px 文字、圆角 9px、无标题层级。*/}
          {codebuddyCli &&
            (!codebuddyCli.configured ||
              (!codebuddyUsesSettingsEnv && !codebuddyCli.helperSupportsAccountIds) ||
              codebuddyCli.migrationRequired ||
              codebuddyCli.syncPending) && (
              <div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[9px] border border-border bg-card-2 px-3 py-2">
                <Terminal className="size-[15px] shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="min-w-0 flex-1 text-[11.5px] leading-relaxed text-muted-foreground">
                  <span className="font-medium text-foreground">{t("wbAccounts.page.cliTitle")}</span>
                  {" · "}
                  {codebuddyUsesSettingsEnv
                    ? codebuddyCli.environmentOverride
                      ? t("wbAccounts.page.cliDescEnvOverride")
                      : codebuddyCli.syncPending
                        ? t("wbAccounts.page.cliDescEnvSyncPending")
                        : codebuddyCli.migrationRequired
                          ? t("wbAccounts.page.cliDescEnvMigration")
                          : t("wbAccounts.page.cliDescEnvDefault")
                    : codebuddyCli.migrationRequired
                      ? t("wbAccounts.page.cliDescHelperMigration")
                      : codebuddyCli.configured
                        ? t("wbAccounts.page.cliDescHelperConfigured")
                        : t("wbAccounts.page.cliDescDefault")}
                </span>
                <DemoAction>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-[28px] shrink-0 rounded-[8px] px-[10px] text-[12px]"
                    onClick={() => void onInstallCodebuddyCli()}
                    disabled={installingCodebuddyCli}
                  >
                    {installingCodebuddyCli && <Loader2 className="animate-spin" />}
                    {codebuddyUsesSettingsEnv
                      ? codebuddyCli.configured ? t("wbAccounts.page.cliBtnUpdateAuth") : t("wbAccounts.page.cliBtnConnect")
                      : codebuddyCli.configured || codebuddyCli.migrationRequired ? t("wbAccounts.page.cliBtnUpgradeHelper") : t("wbAccounts.page.cliBtnConnect")}
                  </Button>
                </DemoAction>
              </div>
            )}

          {/*
            「当前登录账号，但还不在账号库里」的提示卡片。
            账号库为空 ≠ 没有登录态：客户端可能已登录，只是用户还没点过「从本机导入」。
            这时必须把「当前登录的是谁」明确显示出来，而不是给一张「未检测到」空态卡片。
          */}
          {current && !accounts.some((a) => isWorkbuddyCurrent(a, current)) && (
            <div className="mt-7 flex flex-wrap items-center gap-x-5 gap-y-4 rounded-2xl border border-border bg-muted/30 px-5 py-4">
              <div className="min-w-[200px] flex-1">
                <h2 className="text-sm font-semibold text-foreground">
                  {t("wbAccounts.currentLogin.title", { name: workbuddyCurrentName })}
                </h2>
                <p className="mt-1 text-xs leading-5 text-muted-foreground">
                  {t("wbAccounts.currentLogin.unsaved")}
                </p>
              </div>
              <DemoAction>
                <Button className="h-9 px-3" variant="outline" onClick={onImport} disabled={importing}>
                  {importing ? <Loader2 className="animate-spin" /> : <Download />}
                  {t("wbAccounts.currentLogin.save")}
                </Button>
              </DemoAction>
            </div>
          )}

          </>
        }
        checkinResult={
          checkinResult
            ? {
                okCount: checkinResult.ok,
                failedNames: accounts
                  .filter((account) => checkinResult.failedIds.includes(account.id))
                  .map((account) => wbAccountName(account)),
                onRetry:
                  checkinResult.failedIds.length > 0
                    ? () => void batchCheckin(checkinResult.failedIds)
                    : undefined,
              }
            : null
        }
        onDismissCheckinResult={() => setCheckinResult(null)}
        batch={{
          selectedCount: selectedKeys.size,
          busy: batchBusy,
          onCheckin: () => void batchCheckin(),
          onRefresh: () => void batchRefresh(),
          onExport:
            selectedAccounts.length > 0
              ? () => {
                  setExportTargets(selectedAccounts);
                  setExportOpen(true);
                }
              : undefined,
          onDelete: () => setBatchDeleteOpen(true),
          onClear: clearSelection,
        }}
        listAriaLabel={t("wbAccounts.page.accountsAria", { count: accounts.length })}
        totalCount={accounts.length}
        visibleCount={sorted.length}
        emptyNone={{
          title: t("shared.list.empty.none"),
          description: t("shared.list.empty.noneDesc"),
          action: (
            <Button onClick={() => setOauthOpen(true)}>
              <QrCode />
              {t("wbAccounts.page.oauthAdd")}
            </Button>
          ),
        }}
        emptyFiltered={{
          title: t("shared.list.empty.filtered"),
          description: t("shared.list.empty.filteredDesc"),
          action: (
            <Button variant="outline" onClick={clearListFilters}>
              {t("shared.list.empty.clearFilters")}
            </Button>
          ),
        }}
        buckets={buckets}
        columns={columns}
        rowKey={(account) => selectionKey(account.id)}
        selectedIds={selectedKeys}
        onToggleSelect={toggleSelect}
        onToggleBucketAll={toggleBucketSelection}
        bucketAllSelected={bucketAllSelected}
        bucketSomeSelected={bucketSomeSelected}
        onOpenRow={openDetail}
        isRowCurrent={(account) => isWorkbuddyCurrent(account, current)}
        rowSelectLabel={(account) => t("shared.table.selectRowAria", { name: wbAccountName(account) })}
        rowOpenLabel={(account) => t("shared.table.openRowAria", { name: wbAccountName(account) })}
        renderCard={(a) => (
                            <AccountCard
                              key={a.id}
                              account={a}
                              onDelete={onDelete}
                              onSwitch={setSwitchAccount}
                              onCheckin={onCheckin}
                              onRefresh={onRefresh}
                              todayCheckedIn={checkinMap[a.id]}
                              travelStatus={travelMap[a.id]}
                              todayTokens={a.uid ? tokenTodayMap.get(a.uid) : undefined}
                              todayTokensTruncated={tokenTodayTruncated}
                              credit={creditMap[a.id]}
                              creditLoading={creditLoadingMap[a.id]}
                              creditPriority={a.id === priorityAccountId}
                              workbuddyActive={isWorkbuddyCurrent(a, current)}
                              codebuddyCliConfigured={codebuddyCli?.configured && !codebuddyCli.migrationRequired && !codebuddyCli.syncPending}
                              codebuddyCliActive={a.id === cliCurrentAccountId}
                              codebuddyCliBusy={codebuddyCliSwitchingId !== null}
                              onSwitchCodebuddyCli={onSwitchCodebuddyCli}
                              codebuddyCliLoading={codebuddyCliSwitchingId === a.id}
                              codebuddyCnIdeAvailable={Boolean(codebuddyCnIde?.installed)}
                              codebuddyCnIdeActive={a.id === cnIdeCurrentAccountId}
                              codebuddyCnIdeBusy={codebuddyCnIdeSwitchingId !== null}
                              codebuddyCnIdeLoading={codebuddyCnIdeSwitchingId === a.id}
                              onSwitchCodebuddyCnIde={onSwitchCodebuddyCnIde}
                              vscodePluginAvailable={Boolean(vscodePlugin?.extensionInstalled)}
                              vscodePluginActive={a.id === vscodePluginCurrentAccountId}
                              vscodePluginBusy={vscodePluginSwitchingId !== null}
                              vscodePluginLoading={vscodePluginSwitchingId === a.id}
                              onSwitchVscodePlugin={onSwitchVscodePlugin}
                              jetbrainsPluginAvailable={Boolean(jetbrainsPlugin?.pluginInstalled)}
                              jetbrainsPluginActive={a.id === jetbrainsPluginCurrentAccountId}
                              jetbrainsPluginBusy={jetbrainsPluginSwitchingId !== null}
                              jetbrainsPluginLoading={jetbrainsPluginSwitchingId === a.id}
                              onSwitchJetbrainsPlugin={onSwitchJetbrainsPlugin}
                              onGuidePlugin={onGuidePlugin}
                              onAddPlaintextAccount={() => setOauthOpen(true)}
                              onGuideToSetting={onGuideToSetting}
                              selected={selectedKeys.has(selectionKey(a.id))}
                              onToggleSelect={toggleSelect}
                              onOpenDetail={openDetail}
                              onEditRemark={openRemark}
                              onEditGroup={openGroup}
                              /* ★ 两平台**同一取值**（Trae 侧同为 `false`）。
                                 ⚠️ 此前 WB 传 `false`、Trae 传 `api.isDemoMode()` ⇒ 同一个演示构建里
                                 「WB 的按钮能点、Trae 的按钮点不动」，用户看到两个平台行为不一致。
                                 统一取 `false`：演示构建里写操作由 `api.ts` 统一拦截并给可读原因
                                 （`demoUnavailableMessage`），比一枚**点不动又不说明**的按钮好；
                                 这也是截图演示一直依赖的行为。 */
                              featuresDisabled={false}
                            />
        )}
        /* 列表下方的平台专属插槽：客户端限流台账（**只读**观测，账号 × 模型 × 恢复时刻）。
           与账号强相关、无需切页；与网关无关，故不放「API 服务」页（PRD S4「挂载位置已定」）。 */
        footer={
          <section className="mt-7 min-w-0">
            <ClientLimitsCard region={region} />
          </section>
        }
      />

      <AddAccountDrawer
        open={addDrawerOpen}
        onOpenChange={setAddDrawerOpen}
        scopeLabel={scopeLabel(t, scopeOf("workbuddy", region))}
        installed={presence !== "absent"}
        onPick={(way: AddAccountWay) => {
          /* 与原型 `case "add-way"`（`:4236`）同构：先关抽屉，再进入对应流程。 */
          if (way === "oauth") setOauthOpen(true);
          else if (way === "backup") setImportOpen(true);
          else void onImport();
        }}
        /* ★ 原工具栏「⋯」溢出菜单的落点（2026-10-12 用户要求「合并到添加账号的右侧弹出栏内」）。
           抽屉里**本来就有**的两个入口（从本机导入 / 导入备份）就是上面那两张卡 ⇒ **不重复搬**；
           回收站**已被用户点名「放出来」**（回工具栏，见 `onOpenRecycleBin`）
           ⇒ 这里只剩 4 个：两枚开关 + 两枚动作。
           动作行先关抽屉再开各自的弹层，避免两层遮罩叠在一起。 */
        moreTools={[
          {
            key: "auto-checkin",
            icon: CalendarCheck,
            label: t("wbAccounts.page.autoCheckin"),
            htmlFor: `accounts-auto-checkin-${region}`,
            control: (
              <>
                <DemoAction>
                  <Switch
                    id={`accounts-auto-checkin-${region}`}
                    checked={autoCheckinConfig?.enabled ?? false}
                    disabled={!autoCheckinConfig || autoCheckinSaving}
                    onCheckedChange={(enabled) => void onAutoCheckinChange(enabled)}
                    aria-label={t("wbAccounts.page.autoCheckin")}
                  />
                </DemoAction>
                {autoCheckinSaving && (
                  <Loader2 className="size-3.5 animate-spin text-muted-foreground" aria-label={t("wbAccounts.page.autoCheckinSaving")} />
                )}
              </>
            ),
          },
          {
            key: "auto-travel",
            icon: Plane,
            label: t("wbAccounts.page.autoTravel"),
            htmlFor: `accounts-auto-travel-${region}`,
            control: (
              <>
                <DemoAction>
                  <Switch
                    id={`accounts-auto-travel-${region}`}
                    checked={autoTravelConfig?.enabled ?? false}
                    disabled={!autoTravelConfig || autoTravelSaving}
                    onCheckedChange={(enabled) => void onAutoTravelChange(enabled)}
                    aria-label={t("wbAccounts.page.autoTravel")}
                  />
                </DemoAction>
                {autoTravelSaving && (
                  <Loader2 className="size-3.5 animate-spin text-muted-foreground" aria-label={t("wbAccounts.page.autoTravelSaving")} />
                )}
              </>
            ),
          },
          {
            key: "export",
            icon: FileDown,
            label: t("wbAccounts.page.exportTitle"),
            disabled: accounts.length === 0,
            onClick: () => {
              setAddDrawerOpen(false);
              setExportOpen(true);
            },
          },
          {
            key: "refresh",
            icon: RefreshCw,
            label: t("wbAccounts.page.refreshCredits"),
            disabled: refreshingCredits || checkinAllRunning || accounts.length === 0,
            onClick: () => {
              setAddDrawerOpen(false);
              void onRefreshCredits();
            },
          },
        ]}
        labels={{
          title: t("shared.addDrawer.title"),
          scopeHintPrefix: t("shared.addDrawer.scopePrefix"),
          scopeHintSuffix: t("shared.addDrawer.scopeSuffix"),
          oauthTitle: t("shared.addDrawer.oauthTitle"),
          oauthDesc: t("shared.addDrawer.oauthDesc"),
          localTitle: t("shared.addDrawer.localTitle"),
          localDesc: t("shared.addDrawer.localDesc"),
          backupTitle: t("shared.addDrawer.backupTitle"),
          backupDesc: t("shared.addDrawer.backupDesc"),
          notInstalled: t("shared.addDrawer.notInstalled"),
          moreTools: t("shared.addDrawer.moreTools"),
        }}
      />

      <OAuthLoginDialog open={oauthOpen} onOpenChange={setOauthOpen} region={region} />
      <ExportAccountsDialog
        open={exportOpen}
        onOpenChange={(open) => {
          setExportOpen(open);
          // 关闭即清掉「导出所选」的目标：下次从工具栏进来仍是「全部」，不残留上一次的范围。
          if (!open) setExportTargets(null);
        }}
        accounts={exportTargets ?? accounts}
        onExported={onExported}
        region={region}
      />
      <ImportAccountsDialog
        open={importOpen}
        onOpenChange={setImportOpen}
        onImported={onImported}
        region={region}
      />
      {/* ★ P14 S4a · 插件会话抽屉（VS Code 插件 / CodeBuddy IDE）。
          与切换向导**并列而不是嵌套**：两者是不同流程，嵌套会让「关一个开一个」无法表达。 */}
      {extDrawer ? (
        <ExtSessionDrawer
          open
          onOpenChange={(open) => {
            if (!open) setExtDrawer(null);
          }}
          client={extDrawer.client}
          account={extDrawer.account}
          /* 目标候选 = 本区域**除源账号以外**的全部账号（不写死列表）。 */
          targets={accounts.filter((item) => item.id !== extDrawer.account.id)}
        />
      ) : null}

      <SwitchAccountDialog
        open={switchAccount !== null}
        onOpenChange={(o) => {
          if (!o) setSwitchAccount(null);
        }}
        account={switchAccount}
        region={region}
        copySessionsByDefault={Boolean(switchConfig?.copy_sessions_by_default)}
        onDone={() => {
          // 切换完成后必须同时刷新账号库与状态：账号库决定卡片内容，
          // `status.current` 决定「当前账号」标记；只刷账号库会让标记留在旧账号上。
          void reconcileAccounts(region);
          void refreshRegionStatus(region);
          void refreshCodebuddyCliStatus();
          void refreshCodebuddyCnIdeStatus();
        }}
      />

      {/* 回收站（P5 §2.7.1）：恢复 = 移回 live ⇒ 成功后重取账号库，让恢复的账号立刻回到列表。 */}
      <RecycleBinSheet
        open={recycleOpen}
        onOpenChange={setRecycleOpen}
        scope={scope}
        onRestored={() => void reconcileAccounts(region)}
      />

      {/* 统一重启确认（P8 T03 / §3.15.3）：**所有**客户端切换走同一条流程。
          CLI 例外（`client === "cli"`）⇒ 不渲染「自动重开」，文案如实写「当前 CLI 会话会中断」；
          「先关再开」的三个客户端（IDE / VS Code 扩展 / JetBrains 插件）⇒ 「需重启」文案 +
          默认勾选自动重开 + 未保存文件风险。 */}
      <RestartConfirmDialog
        open={restartConfirm !== null}
        onOpenChange={(o) => {
          if (!o) setRestartConfirm(null);
        }}
        clientLabel={switchClientLabel(restartConfirm?.client ?? "ide")}
        isCli={(restartConfirm?.client ?? "ide") === "cli"}
        onConfirm={(restart) => {
          const target = restartConfirm;
          setRestartConfirm(null);
          if (!target) return;
          switch (target.client) {
            case "cli":
              void doSwitchCodebuddyCli(target.account);
              return;
            case "ide":
              void doSwitchCodebuddyCnIde(target.account, restart);
              return;
            case "vscode":
              doSwitchVscodePlugin(target.account, restart);
              return;
            default:
              doSwitchJetbrainsPlugin(target.account, restart);
          }
        }}
      />

      {/* 接入/升级 CLI 认证确认（桌面 App 不支持 window.confirm） */}
      <Dialog open={installConfirmOpen} onOpenChange={setInstallConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {codebuddyUsesSettingsEnv
                ? t("wbAccounts.dialog.installTitleUpdate")
                : codebuddyCli?.configured || codebuddyCli?.migrationRequired
                  ? t("wbAccounts.dialog.installTitleUpgrade")
                  : t("wbAccounts.dialog.installTitleConnect")}
            </DialogTitle>
            <DialogDescription>
              {codebuddyUsesSettingsEnv ? (
                <>
                  {t("wbAccounts.dialog.installDescSettingsA")}
                  <code className="mx-1 rounded bg-muted px-1">~/.codebuddy/settings.json</code>
                  {t("wbAccounts.dialog.installDescSettingsB")}
                </>
              ) : (
                <>
                  {t("wbAccounts.dialog.installDescOtherA", { verb: codebuddyCli?.configured || codebuddyCli?.migrationRequired ? t("wbAccounts.dialog.installVerbUpgrade") : t("wbAccounts.dialog.installVerbConnect") })}
                </>
              )}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setInstallConfirmOpen(false)}>
              {t("wbAccounts.dialog.cancel")}
            </Button>
            <Button onClick={() => void confirmInstallCodebuddyCli()}>{t("wbAccounts.dialog.continue")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除账号确认 */}
      <Dialog open={deleteTarget !== null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("wbAccounts.dialog.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("wbAccounts.dialog.deleteDesc", {
                name:
                  displayText(deleteTarget?.nickname) ||
                  displayText(deleteTarget?.email) ||
                  displayText(deleteTarget?.id) ||
                  t("wbAccounts.common.unknownAccount"),
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              {t("wbAccounts.dialog.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmDelete()}>
              {t("wbAccounts.dialog.deleteConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 区域不匹配详情 */}
      <Dialog open={mismatchDetailOpen} onOpenChange={setMismatchDetailOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("wbAccounts.page.mismatchDetailTitle")}</DialogTitle>
            <DialogDescription>
              {t("wbAccounts.page.mismatchDetailDesc")}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5 text-sm">
            <div className="flex justify-between gap-4">
              <span className="text-muted-foreground">{t("wbAccounts.page.mismatchActualDomain")}</span>
              <code className="font-mono">{mismatch?.actualDomain || "—"}</code>
            </div>
            <div className="flex justify-between gap-4">
              <span className="text-muted-foreground">{t("wbAccounts.page.mismatchExpectedFile")}</span>
              <code className="font-mono">{mismatch?.expectedFile || descriptor.authFilename}</code>
            </div>
            <div className="flex justify-between gap-4">
              <span className="text-muted-foreground">{t("wbAccounts.page.mismatchEnvVar")}</span>
              <code className="font-mono">{mismatch?.envVar || descriptor.authEnv}</code>
            </div>
          </div>
          <DialogFooter>
            <Button onClick={() => setMismatchDetailOpen(false)}>{t("wbAccounts.dialog.oauthClose")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 账号详情抽屉（P4）：共享壳 + 本页装配的 sections / packs / programs。 */}
      <AccountDetailDrawer
        open={detail !== null}
        onOpenChange={(open) => {
          if (!open) setDetailAccount(null);
        }}
        name={detailName}
        subtitle={detail ? accountIdentity(detail) : ""}
        avatarSeed={detailName}
        loading={detailLoading}
        packs={detailPacks}
        sections={detailSections}
        programs={detailPrograms}
        /* ★ 头部「当前登录」徽章 + 可点「切换到此账号」程序位 + 向导入口（原型 :3475 / :3480-3486）。 */
        current={detailIsCurrent}
        switchTargets={detailSwitchTargets}
        onWizardSwitch={detail ? () => setSwitchAccount(detail) : undefined}
        /* ★ P14 S4a · 插件会话复制入口：只在**有会话仓的程序位已接入**时注入
           （`jetbrains` 刻意不映射 —— 它没有对应的会话树）。 */
        onExtSessions={
          detail && detailExtClient
            ? () => setExtDrawer({ client: detailExtClient, account: detail })
            : undefined
        }
        extClientLabel={
          detailExtClient
            ? t(
                detailExtClient === "vscode"
                  ? "sessions.ext.client.vscode"
                  : "sessions.ext.client.codebuddyIde",
              )
            : undefined
        }
        /* ★ 「操作」区（原型 :3518-3527）：刷新 / 签到 / 导出 / 权限诊断 / 删除（复用页面既有处理函数）。 */
        actions={detailActions}
        /* ★ 「签到历史（近 7 天）」（原型 :3428）：日期 / 结果来自签到日志，金额来自积分统计（缺失「—」）。 */
        checkinHistory={detailCheckinHistory}
        // ★ B5：签到积分（**有数据才渲染**；查询未回来 / 失败时为 `null`）。
        checkin={
          detail && checkinDetailMap[detail.id]
            ? {
                todayCheckedIn: checkinDetailMap[detail.id].todayCheckedIn,
                ...checkinCreditsOf(checkinDetailMap[detail.id]),
              }
            : null
        }
        remark={detail ? displayText(detail.remark)?.trim() || "" : ""}
        onSaveRemark={detail ? (remark) => onSaveRemark(detail, remark) : undefined}
        autoEditRemark={detailAutoEditRemark}
        /* ★ 自定义分组（与备注**同一交互**）：注入 `onSaveGroup` ⇒ 抽屉渲染「分组」区块。 */
        group={detail ? displayText(detail.groupName)?.trim() || "" : ""}
        onSaveGroup={detail ? (group) => onSaveGroup(detail, group) : undefined}
        autoEditGroup={detailAutoEditGroup}
        empty={{
          title: t("shared.drawer.empty.title"),
          description: t("shared.drawer.empty.description"),
          action: (
            <Button size="sm" variant="outline" onClick={() => void onRecheck()}>
              {t("wbAccounts.empty.recheck")}
            </Button>
          ),
        }}
      />

      {/* 批量删除确认（桌面 App 不支持 window.confirm）。 */}
      <Dialog open={batchDeleteOpen} onOpenChange={setBatchDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("shared.batch.deleteTitle")}</DialogTitle>
            <DialogDescription>{t("shared.batch.deleteDesc", { n: selectedKeys.size })}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setBatchDeleteOpen(false)}>
              {t("shared.batch.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void batchDelete()}>
              {t("shared.batch.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

/** 该版本未安装 / 未登录时的空态卡片（PRD §3.3.1）。 */
function EmptyRegionCard({
  region,
  installed,
  expectedAuthFile,
  onRecheck,
  onImport,
  importing,
}: {
  region: Region;
  installed: boolean;
  expectedAuthFile: string;
  onRecheck: () => void;
  onImport: () => void;
  importing: boolean;
}) {
  const descriptor = regionDescriptor(region);
  const t = useT();
  return (
    <Card className="gap-0 py-0">
      <div className="flex items-start gap-3 px-5 py-5">
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          {/*
            ⚠️ 标题必须按 `installed` 分叉：客户端已安装、只是没有登录态时说「未检测到」，
            用户会读成「应用认为我没装」（2026-09-24 报障），而且与页签上那句
            「未登录」自相矛盾。原因列表同理 —— 已安装时不该再列「未安装客户端」。
          */}
          <h2 className="text-sm font-medium">
            {installed
              ? t("wbAccounts.empty.notLoggedInTitle", { version: descriptor.versionLabel })
              : t("wbAccounts.empty.notDetected", { version: descriptor.versionLabel })}
          </h2>

          <div className="mt-3 text-sm text-muted-foreground">
            <p className="font-medium text-foreground/80">{t("wbAccounts.empty.possibleReasons")}</p>
            <ul className="mt-1 list-disc space-y-1 pl-5">
              {!installed && (
                <li>{t("wbAccounts.empty.reasonNotInstalled", { version: descriptor.versionLabel })}</li>
              )}
              <li>{t("wbAccounts.empty.reasonNoLogin")}</li>
              {installed && <li>{t("wbAccounts.empty.reasonNoSession")}</li>}
            </ul>
          </div>

          <div className="mt-4">
            <p className="text-sm font-medium text-foreground/80">{t("wbAccounts.empty.expectedAuthFile")}</p>
            <div className="mt-1.5 flex flex-wrap items-center gap-2">
              <code className="min-w-0 break-all rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-[11px] text-muted-foreground">
                {expectedAuthFile}
              </code>
              <Button variant="ghost" size="sm" onClick={() => void copyText(expectedAuthFile, t("wbAccounts.empty.copyPathToast"))}>
                {t("wbAccounts.empty.copyPath")}
              </Button>
            </div>
          </div>

          <div className="mt-4 flex flex-wrap gap-2">
            <Button size="sm" variant="outline" onClick={onRecheck}>
              <RefreshCw />
              {t("wbAccounts.empty.recheck")}
            </Button>
            <Button size="sm" onClick={onImport} disabled={importing}>
              {importing ? <Loader2 className="animate-spin" /> : <Download />}
              {t("wbAccounts.empty.importLocal")}
            </Button>
          </div>
        </div>
      </div>
    </Card>
  );
}
