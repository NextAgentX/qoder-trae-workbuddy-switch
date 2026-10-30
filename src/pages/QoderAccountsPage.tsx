/**
 * Qoder 账号页。
 *
 * ## 与原型的关系（`buddy-switch-prototype.html` 是唯一原型）
 *
 * 原型的账号页是**平台无关的工作台**：`viewAccounts()`（工具栏一行：搜索 / 排序 / 分组 /
 * 视图切换 / 全选 / 添加账号）+ `accountCard()`（勾选 → 整卡可点开详情 → 页脚更多菜单）
 * + `drawerAccount()`（详情抽屉：账号信息 / 备注编辑 / 操作区含删除）+ `batchBarHtml()`
 * （批量条）。本页**逐件复用**了共享壳 —— `AccountWorkbench`（工具栏 / 批量条 / 空态 /
 * 分桶 / 卡片栅格与表格切换）、`AccountDetailDrawer`、`AddAccountDrawer`、
 * `RecycleBinSheet`、`ImportAccountsDialog`、`ExportAccountsDialog` —— 与
 * `AccountsPage`（WB）/ `TraeAccountsPage` 是**同一条交互链**，差异只有数据。
 *
 * ## ★ 备注编辑在**抽屉**里（不在卡上）
 *
 * 与原型 `drawerAccount(id, editing)` 同构：卡片点开抽屉 → 抽屉里「编辑备注」→
 * 就地输入 + 保存 / 取消。卡片**不再堆按钮**（原型 `acct-more` 的明说：
 * 「操作都在账号抽屉里 —— 点账号卡打开抽屉即可操作」）。
 *
 * ## 与后端能力的对齐（缺的就不渲染，绝不摆假控件）
 *
 * 2026-10-15 本轮把三块「后端早就绪、前端没接线」的能力补齐，并新增两块：
 *
 * | 能力 | 落点 |
 * |:--|:--|
 * | **OAuth 设备授权** | 添加抽屉第一张卡 → `QoderOAuthDialog`（后端 PKCE S256 设备流） |
 * | **回收站**（软删除） | 工具栏回收站按钮 → 共用 `RecycleBinSheet`；删除文案随之改 |
 * | **导入 / 导出备份** | 添加抽屉「导入备份」卡 + 抽屉 / 批量条的「导出」 |
 * | **签到 / 额度** | 卡片指标行（积分）+ 状态槽（今日已签 / 未签）+ 抽屉签到区与积分包 + 批量签到 |
 *
 * 仍**不渲染**的：程序位胶囊（Qoder 的「切换客户端登录态」尚未落地）、`expiresAt`
 * （单位未取证 —— 按毫秒渲染会把秒值显示成 1970 = 谎报）。
 *
 * ## 两处刻意的加载策略
 *
 * 1. **账号列表与「额度 / 签到」分开加载**：前者是本地文件（快），后者要打上游（慢，且
 *    可能失败）。串在一起会让「上游 401」变成「整页打不开」——而账号列表本来是可读的。
 * 2. **额度 / 签到失败只影响那两块**：不弹错误条、不清空列表，卡片自然回落成
 *    「没有积分指标 / 状态槽显示凭据可用」。**未知不伪装成已知**。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useSearchParams } from "react-router-dom";
import {
  AlertTriangle,
  KeyRound,
  Loader2,
  Plus,
  RefreshCw,
  ShieldAlert,
  UserRound,
} from "lucide-react";
import { toast } from "sonner";

import {
  AccountDetailDrawer, DetailStat,
  type DetailActions,
  type DetailPack,
  type DetailSwitchTarget,
} from "@/components/account-detail-drawer";
import type { AccountColumn } from "@/components/account-table";
import { AccountsTableAvatar } from "@/components/account-list-parts";
import { AccountWorkbench } from "@/components/account-workbench";
import { AddAccountDrawer } from "@/components/add-account-drawer";
import { ExportAccountsDialog } from "@/components/export-accounts-dialog";
import { ImportAccountsDialog } from "@/components/import-accounts-dialog";
/* 账号页「限流」卡：与 WB **同一个组件**；Qoder 没有客户端限流台账 ⇒ `ledger={false}`。 */
import { ClientLimitsCard } from "@/components/client-limits-card";
import { RecycleBinSheet } from "@/components/recycle-bin-sheet";
import {
  QoderAccountCard,
  QoderAccountMenu,
  QoderForeignBadge,
  QoderStatusChips,
  qoderAccountName,
  qoderSourceLabel,
} from "@/components/qoder/qoder-account-card";
import { QoderLocalSourceRow } from "@/components/qoder/qoder-local-source-row";
import { QoderOAuthDialog } from "@/components/qoder/qoder-oauth-dialog";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ClientlessStrip } from "@/components/clientless-notice";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import * as api from "@/lib/api";
import type {
  CheckinStatusEntry,
  QoderAccountView,
  QoderCheckinRow,
  QoderCreditsRow,
  QoderLocalSourcesView,
  QoderProgramStatus,
} from "@/lib/api";
import { accountSearchText } from "@/lib/account-search";
import { PACKS_TTL_MS, packSourceLabel, packState } from "@/lib/qoder-packs";
import {
  filterAccounts,
  groupAccounts,
  sortAccounts,
  type AccountComparator,
  type AccountGroupKey,
  type AccountListConfig,
  type AccountSortKey,
  type AccountViewMode,
} from "@/lib/account-list-view";
import { formatAbsoluteTime } from "@/lib/catalog-source";
import { displayText } from "@/lib/display-text";
import { formatCredits } from "@/components/account-card";
import {
  QODER_REFRESH_KINDS,
  REFRESH_DONE_LABEL,
  useAccountRefreshBusy,
  type AccountRefreshKind,
} from "@/lib/account-refresh";
import { useT } from "@/lib/i18n";
import { platformDescriptor, platformOfPath } from "@/lib/platforms/registry";
import { scopeOf, type Scope } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import { useMediaQuery } from "@/lib/use-media-query";
import { useAutoRefreshWhenVisible } from "@/lib/use-auto-refresh";
import { useRefreshAction } from "@/lib/use-refresh-action";

/** ISO-8601 → 毫秒时间戳（解析不了给 `null` ⇒ 调用方**省掉那一行**，不显示 1970）。 */
const stampToMs = (iso: string | null | undefined): number | null => {
  if (!iso) return null;
  const ts = Date.parse(iso);
  return Number.isNaN(ts) ? null : ts;
};

/** 批量签到 / 批量刷新的失败项（`uid` 用于只重试失败项，`name` 用于展示）。 */
interface FailedItem {
  uid: string;
  name: string;
}

export default function QoderAccountsPage() {
  const t = useT();
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);

  const [accounts, setAccounts] = useState<QoderAccountView[] | null>(null);
  const [sources, setSources] = useState<QoderLocalSourcesView | null>(null);
  const [error, setError] = useState("");
  const [importing, setImporting] = useState(false);

  /* ── 额度 / 签到（**上游调用**，与账号列表分开加载；见文件头「加载策略」） ── */
  const [credits, setCredits] = useState<Record<string, QoderCreditsRow>>({});
  /** 额度快照最后一次成功读到的时刻（issue #133，自动刷新判过期用；`0` = 还没读到）。 */
  const [creditsAt, setCreditsAt] = useState(0);
  const [checkinRows, setCheckinRows] = useState<Record<string, QoderCheckinRow>>({});
  /* 程序位状态（只读）：决定页脚程序位行、「当前登录」徽章与「切换到此账号」的可用性。 */
  const [programs, setPrograms] = useState<QoderProgramStatus[] | null>(null);

  /* ── 视图参数（页内 state，不进 URL、不进全局 store —— `account-list-view.ts` 的纪律） ── */
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<AccountSortKey>("name");
  const [group, setGroup] = useState<AccountGroupKey>("none");
  const [view, setView] = useState<AccountViewMode>("card");
  const tableEnabled = useMediaQuery("(min-width: 900px)");
  const searchRef = useRef<HTMLInputElement>(null);

  /* ── 选择（批量） ── */
  const [selected, setSelected] = useState<Set<string>>(new Set());

  /* ── 抽屉 / 弹层 ── */
  const [addDrawerOpen, setAddDrawerOpen] = useState(false);
  const [patOpen, setPatOpen] = useState(false);
  const [oauthOpen, setOauthOpen] = useState(false);
  const [recycleOpen, setRecycleOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportTargets, setExportTargets] = useState<QoderAccountView[] | null>(null);
  const [detail, setDetail] = useState<QoderAccountView | null>(null);
  /**
   * Pro 升级包资格（按 uid 缓存）。
   *
   * ⚠️ `available=false`（活动下线，真机 404 就是这档）⇒ **不渲染入口**，不摆死按钮。
   * 只在打开抽屉时查一次（单账号 1 个请求），不在列表首页批查（会翻 N 倍）。
   */
  const [proPack, setProPack] = useState<Record<string, api.QoderProPackStatus>>({});
  /**
   * 逐包明细（issue #111）：按 uid 缓存。
   *
   * ⚠️ 与 `credits` **并存、不互替**：`credits` 是 openapi 的聚合三标量（任何形态都有），
   * 逐包要走网页会话（仅桌面端 + 需登录一次）。拿不到逐包时**回落聚合行**，
   * 绝不显示空区块 —— 那是「不知道」伪装成「没有」。
   */
  const [webPacks, setWebPacks] = useState<Record<string, api.QoderWebCreditPacks>>({});
  /** 正在取逐包的账号（浮层里显示「读取中」而不是「未登录」）。 */
  const [packsLoading, setPacksLoading] = useState<Record<string, boolean>>({});
  /** 每账号上次取数时刻（TTL 用 ref：改它不该触发重渲染）。 */
  const packsFetchedAt = useRef<Record<string, number>>({});
  const [proBusy, setProBusy] = useState<string | null>(null);
  const [detailAutoEditRemark, setDetailAutoEditRemark] = useState(false);
  const [detailAutoEditGroup, setDetailAutoEditGroup] = useState(false);
  /** 待确认的切换（账号 × 程序位）。 */
  const [switchTarget, setSwitchTarget] = useState<{ account: QoderAccountView; program: QoderProgramStatus } | null>(null);
  const [switchBusy, setSwitchBusy] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<QoderAccountView | null>(null);
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);

  /** 批量动作 / 删除的进行中态（**按钮态**，与区块加载态分家）。 */
  const [busy, setBusy] = useState(false);
  /** 批量签到结果条（含失败项，可只重试失败项）。`null` = 不渲染。 */
  /* 三档：成功数 / 通道未开放 / 失败（见 `runCheckin` 的口径说明）。 */
  const [checkinResult, setCheckinResult] = useState<{
    okCount: number;
    /** 本次**真领到**的额度合计（活动通道；只作展示，不参与成败判定）。 */
    earned: number;
    disabled: FailedItem[];
    failed: FailedItem[];
  } | null>(null);

  /** 失败态「重新检测」按钮的进行中态（操作进行中 ⇒ 按钮内 Loader2）。 */
  const { busy: refreshBusy, run } = useRefreshAction();

  const reload = useCallback(async () => {
    setError("");
    try {
      const [accountsView, localSources] = await Promise.all([
        api.getQoderAccounts(scope),
        api.getQoderLocalSources(scope),
      ]);
      setAccounts(accountsView.accounts ?? []);
      setSources(localSources);
    } catch (e) {
      setError(api.asError(e));
      setAccounts((prev) => prev ?? []);
    }
  }, [scope]);

  useEffect(() => {
    void reload();
  }, [reload]);

  /**
   * 拉额度与签到状态。
   *
   * ⚠️ `allSettled`：两条互不依赖，**一条失败不该拖垮另一条**（区域未开放签到 ≠ 额度也拿不到）。
   * 失败时**保持上一份快照**（不清空）—— 清空会把「暂时取不到」显示成「没有额度」。
   */
  const loadEnrichment = useCallback(async () => {
    const [creditsView, checkinView, programView] = await Promise.allSettled([
      api.qoderCredits(scope),
      api.qoderCheckinStatus(scope),
      api.qoderProgramStatus(scope),
    ]);
    if (programView.status === "fulfilled") {
      setPrograms(programView.value.programs ?? []);
    }
    if (creditsView.status === "fulfilled") {
      setCredits(Object.fromEntries(creditsView.value.accounts.map((row) => [row.uid, row])));
      setCreditsAt(Date.now());
    }
    if (checkinView.status === "fulfilled") {
      setCheckinRows(Object.fromEntries(checkinView.value.accounts.map((row) => [row.uid, row])));
    }
  }, [scope]);

  const hasAccounts = accounts !== null && accounts.length > 0;
  useEffect(() => {
    if (!hasAccounts) {
      // 空库时清掉旧快照：切到另一个区域后残留上一个区域的额度 = 撒谎。
      setCredits({});
      setCheckinRows({});
      setPrograms(null);
      return;
    }
    void loadEnrichment();
  }, [hasAccounts, loadEnrichment]);

  /**
   * ★ 卡片上的额度每 10 分钟自动重读（issue #133）。
   *
   * 空库时不发（`qoderCredits` 会逐账号打上游，没有账号就没有可刷的东西）。
   * 失败时保留上一份快照（`loadEnrichment` 内部已 `allSettled`），不把「暂时取不到」显示成「没有额度」。
   */
  const refreshEnrichment = useCallback(() => {
    if (!hasAccounts) return;
    return loadEnrichment();
  }, [hasAccounts, loadEnrichment]);
  useAutoRefreshWhenVisible(creditsAt, refreshEnrichment);

  /** `/` 键聚焦搜索框（与另两平台同一条规则）；在输入框里打字时不劫持。 */
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
   * P6：命令面板跳过来的 **`?q=` 一次性种子** —— 读一次播种页内搜索 state，
   * 随后 `replace` 清参（视图参数留页内）。`seededRef` 保证只读一次。
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

  /* ── 取数后的派生数据 ── */

  const rows = accounts ?? [];
  const attentionCount = rows.filter(
    (a) => !a.hasCredential || a.credential.unrecognized || a.domainMismatch,
  ).length;
  const foreignAccounts = rows.filter((a) => a.domainMismatch);

  /**
   * 列表配置 —— 差异**全部以数据表达**（共享组件里不写平台判断）。
   *
   * 排序键三个，各有数据支撑：
   * - `priority`（复用共享的排序键名，语义 = **积分余额降序**，与 WB 的「优先级」同义）：
   *   额度来自 `qoder_credits`；**未取到额度的账号排最后**（`-1`），不是排最前
   *   —— 把「不知道」当成「最高」会让用户去用一个可能没余额的号。
   * - `name` / `recent`（`updatedAt`，记录最后一次被导入 / 改备注的时刻）。
   *
   * **不提供**「按到期」：`expiresAt` 单位未取证（见文件头）。
   */
  const listConfig = useMemo<AccountListConfig<QoderAccountView>>(() => {
    const nameComparator: AccountComparator<QoderAccountView> = (left, right) =>
      qoderAccountName(left).localeCompare(qoderAccountName(right), undefined, {
        numeric: true,
        sensitivity: "base",
      });
    const recentComparator: AccountComparator<QoderAccountView> = (left, right) =>
      (stampToMs(right.updatedAt) ?? -Infinity) - (stampToMs(left.updatedAt) ?? -Infinity);
    const creditComparator: AccountComparator<QoderAccountView> = (left, right) =>
      (credits[right.uid]?.remaining ?? -1) - (credits[left.uid]?.remaining ?? -1);
    return {
      searchText: (account) =>
        accountSearchText({
          name: displayText(account.name),
          uid: displayText(account.uid),
          remark: displayText(account.remark),
        }),
      comparators: { priority: creditComparator, name: nameComparator, recent: recentComparator },
      bucketKey: (account, key) => {
        if (key === "status") {
          if (!account.hasCredential) return t("qoder.page.accounts.toolbar.statusBucket.noCredential");
          if (account.credential.unrecognized) return t("qoder.page.accounts.toolbar.statusBucket.unrecognized");
          return t("qoder.page.accounts.toolbar.statusBucket.normal");
        }
        if (key === "program") {
          /* 按**程序位**分桶（与 WB 的 `program` 桶同义）：这条账号当前登在哪个程序位里。
             没登在任何程序位 ⇒ 单独的桶名（**不能返回 null** —— 那会让这些行落进「无桶名」桶，
             与其它桶的渲染不一致）。 */
          const hit = (programs ?? []).filter((program) => program.currentUid === account.uid);
          if (hit.length === 0) return t("qoder.page.accounts.toolbar.programBucket.none");
          return hit.map((program) => program.label).join(" + ");
        }
        if (key === "group") {
          // 未分组**必须有自己的桶名**（不能返回 null）—— 返回 null 会让这些行落进
          // 「无桶名」桶、与其它桶的渲染不一致（WB 侧同样的处理）。
          return displayText(account.group) ?? t("qoder.page.accounts.toolbar.groupBucket.ungrouped");
        }
        return null;
      },
      availableSorts: ["priority", "name", "recent"],
      availableGroups: ["none", "program", "status", "group"],
    };
  }, [t, credits, programs]);

  const sortOptions = useMemo(
    () =>
      (
        [
          ["priority", t("qoder.page.accounts.toolbar.sort.credits")],
          ["name", t("qoder.page.accounts.toolbar.sort.name")],
          ["recent", t("qoder.page.accounts.toolbar.sort.recent")],
        ] as const
      ).map(([value, label]) => ({ value, label })),
    [t],
  );
  const groupOptions = useMemo(
    () =>
      (
        [
          ["none", t("qoder.page.accounts.toolbar.group.none")],
          ["program", t("qoder.page.accounts.toolbar.group.program")],
          ["status", t("qoder.page.accounts.toolbar.group.status")],
          ["group", t("qoder.page.accounts.toolbar.group.group")],
        ] as const
      ).map(([value, label]) => ({ value, label })),
    [t],
  );

  const filtered = useMemo(() => filterAccounts(rows, listConfig, query), [rows, listConfig, query]);
  const sorted = useMemo(() => sortAccounts(filtered, sort, listConfig), [filtered, sort, listConfig]);
  const buckets = useMemo(() => groupAccounts(sorted, group, listConfig), [sorted, group, listConfig]);

  /**
   * 逐账号签到状态（`qoder_checkin_status_map`）——**只覆盖被刷新过的账号**。
   *
   * 刻意不在进页面时全量拉：那要逐账号打上游（N 个账号 = N 次请求），
   * 而 `qoder_checkin_status` 的区域快照足够画初值。用户点刷新才去问。
   */
  const [checkinMap, setCheckinMap] = useState<Record<string, CheckinStatusEntry>>({});

  /** 今日签到三态（`null` = 未知，**不伪装成未签**）。
   *
   * ⚠️ `checkinMap`（用户点「刷新签到状态」后问上游拿到的真值）**优先**于列表快照：
   * 否则那条入口点完卡片纹丝不动 —— 用户会以为没生效而反复点（issue #44 同款教训）。
   */
  const checkinTodayOf = useCallback(
    (account: QoderAccountView): boolean | null => {
      const fresh = checkinMap[account.uid];
      if (fresh?.ok) return fresh.todayCheckedIn === true;
      const row = checkinRows[account.uid];
      if (!row || !row.ok) return null;
      return row.todayCheckedIn === true;
    },
    [checkinRows, checkinMap],
  );

  /* ── 选择（照 Trae：全选只作用于**当前可见**的行） ── */

  const allVisibleSelected = sorted.length > 0 && sorted.every((a) => selected.has(a.uid));
  const someVisibleSelected = !allVisibleSelected && sorted.some((a) => selected.has(a.uid));
  function toggleSelect(account: QoderAccountView) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(account.uid)) next.delete(account.uid);
      else next.add(account.uid);
      return next;
    });
  }
  function toggleVisibleSelection() {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const account of sorted) {
        if (allVisibleSelected) next.delete(account.uid);
        else next.add(account.uid);
      }
      return next;
    });
  }
  function bucketAllSelected(items: QoderAccountView[]): boolean {
    return items.length > 0 && items.every((a) => selected.has(a.uid));
  }
  function bucketSomeSelected(items: QoderAccountView[]): boolean {
    return !bucketAllSelected(items) && items.some((a) => selected.has(a.uid));
  }
  function toggleBucketSelection(items: QoderAccountView[], checked: boolean) {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const account of items) {
        if (checked) next.add(account.uid);
        else next.delete(account.uid);
      }
      return next;
    });
  }

  /** 当前选中的账号行（按列表顺序，不是 Set 的插入顺序）。 */
  const selectedRows = useMemo(() => rows.filter((a) => selected.has(a.uid)), [rows, selected]);

  /* ── 动作 ── */

  /** 探测 + 导入（一个动作走完：结果由 toast 汇报；来源四态在首屏空态卡里可查）。 */
  async function onImportLocal() {
    if (importing) return;
    setImporting(true);
    try {
      const outcome = await api.qoderImportLocal(scope);
      const imported = outcome.imported ?? 0;
      const updated = outcome.updated ?? 0;
      if (imported + updated === 0) {
        /* 没导到东西时把**逐来源的问题说明**带上 —— 只说「没有可导入的」等于什么都不说。 */
        const detail = (outcome.problems ?? [])
          .map((problem) => `${problem.label}：${problem.detail ?? problem.state}`)
          .join("；");
        if (detail) toast.message(t("qoder.page.accounts.import.none"), { description: detail });
        else toast.message(t("qoder.page.accounts.import.none"));
      } else {
        toast.success(
          t("qoder.page.accounts.import.result", {
            imported: String(imported),
            updated: String(updated),
          }),
        );
      }
      await reload();
    } catch (e) {
      toast.error(api.asError(e));
    } finally {
      setImporting(false);
    }
  }

  /**
   * 保存备注（抽屉的就地编辑器调用）。
   *
   * ⚠️ 后端在 uid 不存在时返回 `{ ok: false }`（**不是**抛错）—— 必须如实报错，
   * 否则用户会以为「改成功了」，而库里根本没这条账号。
   * 成功后**同步抽屉里的快照**：抽屉的展示值来自 `remark` prop，只调 `reload()`
   * 不会让它重渲（那是「保存成功但界面还是旧值」的经典缺陷）。
   */
  async function saveRemark(account: QoderAccountView, remark: string): Promise<boolean> {
    const value = remark.trim();
    try {
      const result = await api.qoderSetAccountRemark(scope, account.uid, value === "" ? null : value);
      if (!result.ok) {
        toast.error(t("qoder.page.accounts.remark.missing"));
        return false;
      }
      setDetail((prev) => (prev && prev.uid === account.uid ? { ...prev, remark: value === "" ? null : value } : prev));
      await reload();
      toast.success(t("qoder.page.accounts.remark.saved"));
      return true;
    } catch (e) {
      toast.error(api.asError(e));
      return false;
    }
  }

  /**
   * 抽屉里的「切换到此账号」目标（原型 :3480-3486）。
   *
   * `available` 的判据 = **该程序位已就绪且客户端没在跑**：
   * - 四态不是 `ready` ⇒ 没装 / 没登录 / 读不出来，切换无处可写；
   * - 客户端在跑 ⇒ 后端会拒绝（退出时内存态会覆盖），界面提前置灰。
   * `current` 为真时不再重复切换（只读高亮）。
   */
  const switchTargetsFor = useCallback(
    (account: QoderAccountView): DetailSwitchTarget[] =>
      (programs ?? []).map((program) => ({
        key: program.id,
        label: program.label,
        current: program.currentUid === account.uid,
        available: program.state === "ready" && !program.running,
        busy: switchBusy,
        onSwitch: () => setSwitchTarget({ account, program }),
      })),
    [programs, switchBusy],
  );

  /** 保存自定义分组（与备注同构）。 */
  async function saveGroup(account: QoderAccountView, group: string): Promise<boolean> {
    const value = group.trim();
    try {
      const result = await api.qoderSetAccountGroup(scope, account.uid, value === "" ? null : value);
      if (!result.ok) {
        toast.error(t("qoder.page.accounts.group.missing"));
        return false;
      }
      setDetail((prev) => (prev && prev.uid === account.uid ? { ...prev, group: value === "" ? null : value } : prev));
      await reload();
      toast.success(t("qoder.page.accounts.group.saved"));
      return true;
    } catch (e) {
      toast.error(api.asError(e));
      return false;
    }
  }

  /**
   * 执行切换（把凭据写回指定程序位的登录态）。
   *
   * ⚠️ 后端会在客户端运行时拒绝、写后校验失败时自动还原备份 —— 前端**如实转述**后端的话，
   * 不要自己编「切换成功」。
   */
  async function confirmSwitch() {
    if (!switchTarget || switchBusy) return;
    const { account, program } = switchTarget;
    setSwitchBusy(true);
    try {
      const result = await api.qoderSwitchAccount(scope, program.id, account.uid);
      toast.success(t("qoder.page.accounts.switch.done", { name: qoderAccountName(account) }), {
        description: result.backupPath
          ? t("qoder.page.accounts.switch.backup", { path: result.backupPath })
          : undefined,
      });
      setSwitchTarget(null);
      /* 切换改的是**客户端登录态**⇒ 必须重取程序位状态（「当前登录」徽章才会跟着动）。 */
      await loadEnrichment();
    } catch (e) {
      toast.error(api.asError(e));
    } finally {
      setSwitchBusy(false);
    }
  }

  /**
   * 本平台真的能刷的项。
   *
   * issue #47 新增 `checkin`：Qoder 的签到读侧此前只有**整区域**快照
   * （`qoder_checkin_status`），现在多了**逐账号**的只读重取
   * （`qoder_checkin_status_map`）。仍然**没有** `tokens` —— Qoder 的计量单位是
   * credits、没有 token 概念（见 `qoder::usage` 模块头），摆一个点了只能重读缓存的
   * 菜单项比不摆更坏。
   */
  const refreshPartBusy = useAccountRefreshBusy();


  /** 单项刷新（与卡片「更多」里的四项同源）。 */
  const onRefreshPart = useCallback(
    async (account: QoderAccountView, kind: AccountRefreshKind) => {
      const label = qoderAccountName(account);
      await refreshPartBusy.run(account.uid, kind, async () => {
        try {
          if (kind === "credits") {
            await api.qoderRefreshAccount(scope, account.uid);
            await reload();
          } else if (kind === "checkin") {
            /* 只问这一个账号（批量接口传 1 个 id 即可，不新增「单账号」变体）。 */
            const map = await api.qoderCheckinStatusMap(scope, [account.uid]);
            setCheckinMap((prev) => ({ ...prev, ...map }));
          } else {
            await reload();
          }
          toast.success(t(REFRESH_DONE_LABEL[kind]), { description: label });
        } catch (e) {
          toast.error(t("shared.refreshPart.fail"), { description: api.asError(e) });
        }
      });
    },
    [refreshPartBusy, scope, reload, t],
  );

  /**
   * 刷新一条账号的令牌。
   *
   * ★ 走的是**同一份** `qoder_refresh_account`（网关页的「恢复」用的也是它）——
   * 不另造一条「账号页专用刷新」。
   */
  async function refreshOne(account: QoderAccountView) {
    try {
      await api.qoderRefreshAccount(scope, account.uid);
      toast.success(t("qoder.page.accounts.refresh.done", { name: qoderAccountName(account) }));
      await loadEnrichment();
    } catch (e) {
      toast.error(api.asError(e));
    }
  }

  /**
   * 批量 / 单条签到。
   *
   * ⚠️ 后端只有**单账号** `qoder_checkin_claim` ⇒ 这里顺序逐条调，并**如实汇报成败**
   * （批量操作必然可能部分失败，不能让「批量签到」看起来永远 100% 成功）。
   * `baseOk` 用于「只重试失败项」时把上一轮的成果累计进去。
   *
   * ★ **三档**（issue #46 的 D 项，2026-10-18）：
   * ① 成功（含 `already` 今日已签 —— 那有 `today_checked_in` 作依据）；
   * ② **通道未开放**（`disabled`）—— 既不是成功、也**不进失败桶**：
   *    legacy 签到端点对**任何** campaignKey 都回 409（含编造的），上游已关停，
   *    「重试失败项」对它永远无效，混进失败只会教用户做没用的操作；
   * ③ 失败（真失败，可重试）。
   */
  async function runCheckin(targets: QoderAccountView[], baseOk = 0, baseEarned = 0) {
    if (targets.length === 0 || busy) return;
    setBusy(true);
    const failed: FailedItem[] = [];
    const disabled: FailedItem[] = [];
    let ok = 0;
    let earned = 0;
    for (const account of targets) {
      const item = { uid: account.uid, name: qoderAccountName(account) };
      try {
        const res = await api.qoderCheckinClaim(scope, account.uid);
        if (res.ok) {
          ok += 1;
          // 活动通道会带「真领到多少」；旧通道没这个字段 ⇒ 记 0，不编数。
          earned += res.rewardCredits ?? 0;
        } else if (res.disabled) disabled.push(item);
        else failed.push(item);
      } catch {
        failed.push(item);
      }
    }
    setBusy(false);
    setCheckinResult({ okCount: baseOk + ok, earned: baseEarned + earned, disabled, failed });
    await loadEnrichment();
  }

  /** 批量刷新令牌（同样顺序逐条；后端只有单账号命令）。 */
  async function runRefresh(targets: QoderAccountView[]) {
    if (targets.length === 0 || busy) return;
    setBusy(true);
    let ok = 0;
    let fail = 0;
    for (const account of targets) {
      try {
        await api.qoderRefreshAccount(scope, account.uid);
        ok += 1;
      } catch {
        fail += 1;
      }
    }
    setBusy(false);
    if (fail === 0) toast.success(t("qoder.page.accounts.refresh.batchDone", { n: String(ok) }));
    else
      toast.error(
        t("qoder.page.accounts.checkin.batchPartial", { ok: String(ok), fail: String(fail) }),
      );
    await loadEnrichment();
  }

  /**
   * 取某账号的逐包明细（带 TTL；`force` 绕过 TTL）。
   *
   * ★ 并发去重：浮层与抽屉可能同时触发同一个账号 —— 用 `packsLoading` 挡第二次，
   * 否则会并发打两次上游（逐包接口要带会话 Cookie，重复调用没有意义）。
   */
  async function loadPacks(uid: string, force = false) {
    if (!uid) return;
    if (packsLoading[uid]) return;
    const last = packsFetchedAt.current[uid] ?? 0;
    if (!force && webPacks[uid] && Date.now() - last < PACKS_TTL_MS) return;
    setPacksLoading((prev) => ({ ...prev, [uid]: true }));
    try {
      const result = await api.qoderWebCreditPacks(scope, uid);
      packsFetchedAt.current[uid] = Date.now();
      setWebPacks((prev) => ({ ...prev, [uid]: result }));
    } catch {
      /* 静默：详情拿不到不是错误，浮层会按 `packs` 为空给出引导。 */
    } finally {
      setPacksLoading((prev) => {
        const next = { ...prev };
        delete next[uid];
        return next;
      });
    }
  }

  /**
   * 探一次逐包接口：**认领**登录窗里的会话（带 uid 归属校验），成功即刷新该账号逐包。
   *
   * 返回 `true` = 真拿到了明细（调用方据此停止轮询 / 不再提示）。
   *
   * ★ 与 `loadPacks` 的区别：`loadPacks` 只读「已认领的会话」（渲染路径），本函数才是
   * 「把刚登录的会话认领下来」的那一步（#111 复核 §2.1：probe 此前**无任何调用方**，
   * 于是缓存永远为空 —— 刷新一百次也还是「未登录」）。
   */
  async function probeAndLoad(account: QoderAccountView): Promise<boolean> {
    const uid = account.uid;
    setPacksLoading((prev) => ({ ...prev, [uid]: true }));
    try {
      const probe = await api.qoderWebCreditPacksProbe(scope, uid);
      if (!probe.hasSession) return false;
      if (probe.uidMatches === false) {
        // 归属不符：后端已丢弃该会话，界面必须说清楚「不是这个账号」，不能当成没登录。
        toast.error(t("qoder.page.accounts.web.readMismatch"));
        await loadPacks(uid, true);
        return false;
      }
      await loadPacks(uid, true);
      toast.success(t("qoder.page.accounts.web.readDone", { n: String(probe.packs?.length ?? 0) }));
      return true;
    } catch {
      /* 上游拒绝 / 网络失败：让浮层与抽屉按结构化原因给下一步，这里不重复报错。 */
      await loadPacks(uid, true);
      return false;
    } finally {
      setPacksLoading((prev) => {
        const next = { ...prev };
        delete next[uid];
        return next;
      });
    }
  }

  /**
   * 「我已登录，读取逐包明细」（#111 复核 P0-2）。
   *
   * 为什么必须有这个显式入口：登录是在**另一个窗口**里完成的，用户很自然会「登录完直接
   * 关窗」。自动轮询能覆盖大部分情况，但关窗那一刻的时序不保证；给一个用户能自己按的
   * 按钮，比让用户猜「是不是还要再等」强。
   */
  async function readPacks(account: QoderAccountView) {
    const ok = await probeAndLoad(account);
    if (!ok) toast.error(t("qoder.page.accounts.web.readFailed"));
  }

  /**
   * 「退出网页登录」（清除本机会话缓存）。
   *
   * ★ 为什么必须有：会话一旦落盘，用户从界面没有任何办法清掉它（#111 复核 §2.6）。
   * 一份等价于完整身份凭据的 Cookie 不能只靠「删文件」才能退出。
   */
  async function logoutWebSession() {
    try {
      await api.qoderWebSessionLogout(scope);
      setWebPacks({});
      packsFetchedAt.current = {};
      toast.success(t("qoder.page.accounts.web.logoutDone"));
      const uid = detail?.uid;
      if (uid) await loadPacks(uid, true);
    } catch (e) {
      toast.error(api.asError(e));
    }
  }

  /**
   * 「登录 Qoder 网页端」（issue #111）：开一个独立窗口让用户登录一次。
   *
   * 为什么需要这一步：逐包明细只存在于网页端私有接口，只认浏览器会话 Cookie，
   * 而本仓 Qoder 的设备码登录**从不产生网页会话**。登录态落在本地缓存后，
   * 以后取逐包就不必再登录（见 `web_session`）。
   *
   * ## 两段式轮询（别改成每 2 秒直接 probe）
   *
   * ① 每 2 秒问一次「登录窗里的 cookie 集合变了吗」——`qoderWebWindowCookies` 是**本地**
   *    命令（不打上游），变了就说明用户刚完成登录；
   * ② 变了才调**一次** `probe`（真打上游 + 落盘）。
   *
   * 为什么不直接轮询 probe：probe 一旦拿到非空 Cookie 就会真打上游，未登录状态下的匿名
   * Cookie 换来 401，而 401 会立刻清缓存 —— 每 3 秒一次等于打出一串注定失败的请求，
   * 还每次把刚写的缓存抹掉（#111 复核 §5 P0 的 ⚠️）。
   */
  async function openWebLogin(account: QoderAccountView) {
    try {
      await api.qoderWebLoginOpen(scope);
      toast.success(t("qoder.page.accounts.web.loginToast"));
    } catch (e) {
      toast.error(api.asError(e));
      return;
    }
    /* 基线指纹：开窗那一刻的 cookie 集合。之后「与它不同」= 登录态变了。 */
    const baseline = await api.qoderWebWindowCookies(scope).catch(() => null);
    let seen = baseline?.fingerprint ?? "";
    let probes = 0;
    /* 最多等 2 分钟（60 × 2s）：再长说明用户没打算现在登录，没必要一直转。 */
    for (let attempt = 0; attempt < 60; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 2000));
      const current = await api.qoderWebWindowCookies(scope).catch(() => null);
      if (!current) continue; // 单次失败不终止（窗口可能正处在导航中）
      if (!current.windowOpen) {
        /* 窗口被关了 —— 「登录完直接关窗」是最常见的操作序列。
           关窗前后端已兜底收割（`install_close_harvest`），此时 `pending` 为真才值得
           探一次；否则说明用户根本没登录，别白发一次注定 401 的请求。 */
        const status = await api.qoderWebSessionStatus(scope, account.uid).catch(() => null);
        if (status?.pending) await probeAndLoad(account);
        return;
      }
      if (!current.fingerprint || current.fingerprint === seen) continue;
      seen = current.fingerprint;
      probes += 1;
      if (await probeAndLoad(account)) return;
      /* 指纹变了但没认领成功（未登录的匿名 cookie / 别人的会话）：最多试 3 次，
         避免用户在这个窗口里反复登录时把上游打成筛子。 */
      if (probes >= 3) return;
    }
  }

  function openDetail(account: QoderAccountView) {
    setDetailAutoEditRemark(false);
    setDetailAutoEditGroup(false);
    setDetail(account);
  }

  /** 「更多 → 编辑备注」：打开抽屉并**直接进入编辑态**（原型 `drawerAccount(id, true)`）。 */
  function openRemark(account: QoderAccountView) {
    setDetailAutoEditRemark(true);
    setDetailAutoEditGroup(false);
    setDetail(account);
  }

  /** 「更多 → 设置分组」。 */
  function openGroup(account: QoderAccountView) {
    setDetailAutoEditRemark(false);
    setDetailAutoEditGroup(true);
    setDetail(account);
  }

  /**
   * 单账号删除（**软删除**：移入回收站，可恢复）。
   *
   * 弹层确认仍是必需的（账号会离开列表），但文案已从「永久删除、没有回收站」改成
   * 「可在回收站恢复」—— 后端自 2026-10-15 起就是软删除，旧文案会**谎报**。
   * 删完给「撤销」入口（`accountRestore`），与回收站同一条命令。
   */
  async function confirmSingleDelete() {
    if (!pendingDelete || busy) return;
    const target = pendingDelete;
    setBusy(true);
    try {
      await api.qoderDeleteAccount(scope, target.uid);
      setPendingDelete(null);
      toast.success(t("qoder.page.accounts.removed", { name: qoderAccountName(target) }), {
        action: {
          label: t("qoder.page.accounts.trash.undo"),
          onClick: () => {
            void api
              .accountRestore(scope, target.uid)
              .then(() => {
                toast.success(t("qoder.page.accounts.trash.restored", { name: qoderAccountName(target) }));
                return reload();
              })
              .catch((e) => toast.error(api.asError(e)));
          },
        },
      });
      await reload();
    } catch (e) {
      toast.error(api.asError(e));
    } finally {
      setBusy(false);
    }
  }

  /** 批量删除：顺序逐条调单删命令，并如实汇报成败。 */
  async function confirmBatchDelete() {
    if (busy) return;
    const targets = selectedRows;
    if (targets.length === 0) return;
    setBusy(true);
    let ok = 0;
    let fail = 0;
    for (const account of targets) {
      try {
        await api.qoderDeleteAccount(scope, account.uid);
        ok += 1;
      } catch {
        fail += 1;
      }
    }
    setBusy(false);
    setBatchDeleteOpen(false);
    if (fail === 0) toast.success(t("qoder.page.accounts.delete.done", { n: String(ok) }));
    else toast.error(t("qoder.page.accounts.delete.partial", { ok: String(ok), fail: String(fail) }));
    setSelected(new Set());
    await reload();
  }

  /* ── 抽屉与表格的装配 ── */

  const detailRows = useMemo(() => {
    if (!detail) return [];
    const rowsOut = [
      { label: t("qoder.page.accounts.drawer.uid"), value: displayText(detail.uid) ?? "—", mono: true },
      { label: t("qoder.page.accounts.drawer.domain"), value: displayText(detail.domain) ?? "—" },
      { label: t("qoder.page.accounts.drawer.source"), value: qoderSourceLabel(t, detail.source) ?? "—" },
    ];
    const addedAt = stampToMs(detail.addedAt);
    if (addedAt !== null) {
      rowsOut.push({ label: t("qoder.page.accounts.drawer.addedAt"), value: formatAbsoluteTime(addedAt), mono: false });
    }
    const updatedAt = stampToMs(detail.updatedAt);
    if (updatedAt !== null) {
      rowsOut.push({ label: t("qoder.page.accounts.drawer.updatedAt"), value: formatAbsoluteTime(updatedAt), mono: false });
    }
    rowsOut.push({ label: t("qoder.page.accounts.drawer.credential"), value: displayText(detail.credentialSummary) ?? "—", mono: false });
    if (detail.accessTokenMasked) {
      rowsOut.push({ label: t("qoder.page.accounts.drawer.accessToken"), value: detail.accessTokenMasked, mono: true });
    }
    return rowsOut;
  }, [detail, t]);

  /**
   * 抽屉的积分包：**有逐包就用逐包，没有就回落聚合行**（issue #111）。
   *
   * 逐包来自网页会话（`qoderWebCreditPacks`），需要桌面端 + 登录过一次；
   * 拿不到时显示 openapi 的聚合三标量 —— 那在任何形态都拿得到，不是空区块。
   * 聚合行的 `expiryText` 仍给 `—`：`quota/usage` 的 `expiresAt` 是套餐级、不是包级，
   * 按它渲染「到期」会张冠李戴。
   */
  /** 分组候选（本作用域已用过的组名）—— 抽屉里的分组选择器用，见 WB 页同名变量。 */
  const detailGroups = useMemo(() => {
    const names = new Set<string>();
    for (const account of accounts ?? []) {
      const name = displayText(account.group)?.trim();
      if (name) names.add(name);
    }
    return [...names].sort((a, b) => a.localeCompare(b, "zh-Hans-CN"));
  }, [accounts]);

  /**
   * 抽屉顶部的指标块（Qoder 侧只有三个：积分 / 签到 / 凭据 —— 没有「今日 Token」的数据源，
   * 不摆一个永远「—」的块）。
   *
   * ⚠️ 文本全部在这里格式化（抽屉只排版）；`refreshKind` 只给本平台真的能刷的项。
   */
  const detailStats = useMemo<DetailStat[]>(() => {
    if (!detail) return [];
    const row = credits[detail.uid];
    const checked = checkinTodayOf(detail);
    const expired = detail.expiresAt != null && detail.expiresAt < Date.now();
    const credBad = expired || !detail.hasCredential;
    return [
      {
        key: "credits",
        label: t("shared.stat.credits"),
        valueText:
          row && row.ok
            ? `${formatCredits(row.remaining ?? 0)}/${formatCredits(row.total ?? 0)}`
            : "—",
        subText: row?.ok && row.plan ? t("shared.stat.plan", { plan: row.plan }) : undefined,
        refreshKind: "credits",
      },
      {
        key: "checkin",
        label: t("shared.stat.checkin"),
        // 三档必须分开（issue #47）：被刷新过的账号按上游读数渲染；读失败显示「读不到」
        // 而不是「未签到」；上游说活动未开放则显示「本区域未开放」。
        // 没刷新过的退回区域快照口径，并如实标明来源是本地台账。
        valueText: (() => {
          const entry = detail ? checkinMap[detail.uid] : undefined;
          if (entry && !entry.ok) return t("shared.stat.checkinUnreadable");
          if (entry) {
            return entry.todayCheckedIn
              ? t("shared.stat.checkedIn")
              : entry.active === false
                ? t("shared.stat.checkinUnavailable")
                : t("shared.stat.notCheckedIn");
          }
          return checked === null ? "—" : checked ? t("shared.stat.checkedIn") : t("shared.stat.notCheckedIn");
        })(),
        subText:
          detail && checkinMap[detail.uid]?.source === "ledger"
            ? t("shared.stat.checkinFromLedger")
            : !detail || checkinMap[detail.uid]
              ? undefined
              : t("shared.stat.checkinFromLedger"),
        refreshKind: "checkin",
      },
      {
        key: "credential",
        label: t("shared.stat.credential"),
        valueText: credBad
          ? expired
            ? t("shared.stat.credentialExpired")
            : t("shared.stat.credentialRelogin")
          : t("shared.stat.credentialOk"),
        subText: credBad ? t("shared.stat.credentialBadSub") : t("shared.stat.credentialOkSub"),
        tone: credBad ? "bad" : undefined,
        refreshKind: "credential",
      },
    ];
  }, [detail, credits, checkinTodayOf, checkinMap, t]);

  const detailPacks = useMemo<DetailPack[]>(() => {
    if (!detail) return [];
    const web = webPacks[detail.uid];

    // ① 有逐包 ⇒ 逐条渲染（这才回答了「我有几个包、各剩多少、何时到期」）。
    if (web?.available && web.packs.length > 0) {
      const now = Date.now();
      const seen = new Map<string, number>();
      return web.packs.map((pack, index) => {
        const source = packSourceLabel(t, pack.source);
        const ordinal = (seen.get(source) ?? 0) + 1;
        seen.set(source, ordinal);
        const state = packState(pack, now);
        const expired = state === "expired";
        const expiringSoon = state === "expiring";
        const ratio =
          pack.limit > 0
            ? Math.min(100, Math.max(0, (pack.remaining / pack.limit) * 100))
            : 0;
        return {
          key: pack.id ?? `${pack.group}-${index}`,
          name: t("qoder.page.accounts.web.packName", {
            source,
            n: String(ordinal),
          }),
          remainingText: String(pack.remaining),
          totalText: String(pack.limit),
          usedText: String(pack.used),
          expiryText: expired
            ? t("qoder.page.accounts.web.expired")
            : pack.expiresAtMs !== undefined
              ? formatAbsoluteTime(pack.expiresAtMs)
              : t("qoder.page.accounts.web.neverExpires"),
          expired,
          expiringSoon,
          ratio,
        };
      });
    }

    // ② 没有逐包 ⇒ **回落聚合行**（不是空区块；聚合三标量在任意形态都拿得到）。
    const row = credits[detail.uid];
    if (!row || !row.ok) return [];
    const remaining = row.remaining ?? 0;
    const total = row.total ?? 0;
    const used = row.used ?? 0;
    return [
      {
        key: detail.uid,
        name: displayText(row.plan) ?? t("qoder.page.accounts.metric.credits"),
        remainingText: String(remaining),
        totalText: String(total),
        usedText: String(used),
        expiryText: "—",
        expired: false,
        expiringSoon: false,
        ratio: total > 0 ? Math.min(100, Math.max(0, (remaining / total) * 100)) : 0,
      },
    ];
  }, [detail, credits, t, webPacks]);

  /**
   * 抽屉里积分包区块下方的说明与动作（#111 复核 §2.6 / 验收 #16）。
   *
   * 三态：
   * - 拿到逐包 ⇒ 「N 个包 · 会话已存本机」+「退出网页登录」（否则用户没有清除凭据的入口）；
   * - 没拿到（桌面端）⇒ 「需要登录一次」+「读取逐包明细」（不留白，也不只说一句「没有数据」）；
   * - headless ⇒ 不渲染：浮层已说明「当前形态看不到」，这里再摆一个必然失败的按钮更糟。
   */
  const detailPacksNotice = useMemo(() => {
    if (!detail) return null;
    const web = webPacks[detail.uid];
    if (web?.available) {
      return {
        text: t("qoder.page.accounts.web.packsNoticeReady", { n: String(web.packs.length) }),
        actionLabel: t("qoder.page.accounts.web.logout"),
        onAction: () => void logoutWebSession(),
        busy: false,
      };
    }
    if (api.isWebui()) return null;
    return {
      text: t("qoder.page.accounts.web.packsNotice"),
      actionLabel: t("qoder.page.accounts.web.read"),
      onAction: () => void readPacks(detail),
      busy: !!packsLoading[detail.uid],
    };
    // `logoutWebSession` / `readPacks` 每次渲染都是新函数，进依赖会让 memo 恒失效；
    // 两者只读当前闭包里的 scope / detail，随 `detail` / `webPacks` 一起变即可。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail, webPacks, packsLoading, t]);

  /** 打开抽屉时取一次逐包（TTL 内不重复请求）。 */
  useEffect(() => {
    const uid = detail?.uid;
    if (uid) void loadPacks(uid);
    // `loadPacks` 是纯读取 + setState，不依赖其它 props；只按 uid 变化触发。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail?.uid]);

  /** 打开抽屉时查一次 Pro 升级包资格（活动下线/接口不可用 ⇒ 静默不渲染）。 */
  useEffect(() => {
    const uid = detail?.uid;
    if (!uid) return;
    let cancelled = false;
    api
      .qoderProPackStatus(scope, uid)
      .then((status) => {
        if (!cancelled) setProPack((prev) => ({ ...prev, [uid]: status }));
      })
      .catch(() => {
        /* 资格查询失败**不打扰用户**：入口不渲染即可（真正失败会在领取时报）。 */
      });
    return () => {
      cancelled = true;
    };
  }, [detail?.uid, scope]);

  /**
   * 领取 Pro 升级包。
   *
   * 成功/已领过都算成功（后端是幂等的）；领完把资格置为「已领」并**刷新额度**
   * —— 否则用户看不到积分到账，会怀疑没领上。
   */
  const claimProPack = useCallback(
    async (uid: string) => {
      setProBusy(uid);
      try {
        const result = await api.qoderProPackClaim(scope, uid);
        toast.success(result.message);
        setProPack((prev) => ({
          ...prev,
          [uid]: { available: result.available, eligible: false, error: null },
        }));
        void loadEnrichment();
      } catch (error) {
        toast.error(api.asError(error));
      } finally {
        setProBusy(null);
      }
    },
    [scope, loadEnrichment],
  );

  /** 抽屉里的福利包区块（活动在线才给）。 */
  const detailProPack = useMemo(() => {
    if (!detail) return null;
    const status = proPack[detail.uid];
    if (!status?.available) return null;
    return {
      title: t("qoder.page.accounts.pro.title"),
      description: t("qoder.page.accounts.pro.desc"),
      claimLabel: t("qoder.page.accounts.pro.claim"),
      claimingLabel: t("qoder.page.accounts.pro.claiming"),
      claimedLabel: t("qoder.page.accounts.pro.claimed"),
      claimed: !status.eligible,
      busy: proBusy === detail.uid,
      onClaim: () => void claimProPack(detail.uid),
    };
  }, [detail, proPack, proBusy, t, claimProPack]);

  /**
   * 抽屉的签到区 —— 状态**明确可读**时给数值；读不到 / 无机器身份 / 本区域没活动时
   * 给一条**原因说明**（issue #66：从前这些情况整个 tab 直接消失，用户只看到「读不到」）。
   */
  const detailCheckin = useMemo(() => {
    if (!detail) return null;
    const row = checkinRows[detail.uid];
    if (!row) return null;
    const notice = !row.ok
      ? t("qoder.page.accounts.checkin.noticeUnreadable", {
          error: row.error ?? t("shared.stat.checkinUnreadable"),
        })
      : row.machineHeaders === "omitted"
        ? t("qoder.page.accounts.checkin.noticeNoMachine")
        : row.status === "NONE"
          ? t("qoder.page.accounts.checkin.noticeNoCampaign")
          : null;
    if (row.todayCheckedIn === undefined && !notice) return null;
    return {
      todayCheckedIn: row.todayCheckedIn === true,
      rewardCredits: row.rewardCredits ?? null,
      streakDays: row.streakDays ?? null,
      notice,
    };
  }, [detail, checkinRows, t]);

  const detailActions = useMemo<DetailActions | null>(() => {
    if (!detail) return null;
    const credentialUnavailable = !detail.hasCredential;
    return {
      onRefresh: () => void refreshOne(detail),
      onCheckin: () => void runCheckin([detail]),
      /* 「手动签到」只在凭据可用时出现（凭据坏了点它必然失败）。 */
      showCheckin: detail.hasCredential,
      onExport: () => {
        setExportTargets([detail]);
        setExportOpen(true);
      },
      onDelete: () => {
        const target = detail;
        setDetail(null);
        setPendingDelete(target);
      },
      credentialUnavailable,
    };
  }, [detail]);

  const columns: AccountColumn<QoderAccountView>[] = [
    {
      key: "account",
      header: t("qoder.page.accounts.table.account"),
      render: (account) => (
        /* 与 WB / Trae 表格同一结构：头像 + 名称 + 身份行。 */
        <div className="flex min-w-0 items-center gap-2.5">
          <AccountsTableAvatar name={qoderAccountName(account)} />
          <div className="min-w-0">
            <div className="truncate font-medium" title={qoderAccountName(account)}>
              {qoderAccountName(account)}
            </div>
            <div className="truncate font-mono text-xs text-muted-foreground">
              UID · {displayText(account.uid) ?? "—"}
            </div>
          </div>
        </div>
      ),
    },
    {
      key: "status",
      header: t("qoder.page.accounts.table.status"),
      render: (account) => (
        <div className="flex flex-wrap items-center gap-1">
          <QoderStatusChips account={account} checkinToday={checkinTodayOf(account)} />
          <QoderForeignBadge account={account} />
        </div>
      ),
    },
    {
      key: "credits",
      header: t("qoder.page.accounts.metric.credits"),
      render: (account) => {
        const row = credits[account.uid];
        if (!row || !row.ok) return <span className="text-muted-foreground">—</span>;
        return (
          <span className="tabular-nums">
            {t("qoder.page.accounts.metric.creditsValue", {
              remaining: String(row.remaining ?? 0),
              total: String(row.total ?? 0),
            })}
          </span>
        );
      },
    },
    {
      key: "source",
      header: t("qoder.page.accounts.table.source"),
      render: (account) => (
        <span className="text-muted-foreground">{qoderSourceLabel(t, account.source) ?? "—"}</span>
      ),
    },
    {
      key: "remark",
      header: t("qoder.page.accounts.table.remark"),
      render: (account) => (
        <span className="text-muted-foreground">{displayText(account.remark) ?? "—"}</span>
      ),
    },
  ];

  /* ── 首屏空态（「真没有账号」由它承担：出口 = 添加账号抽屉） ── */

  const emptyState =
    accounts !== null && accounts.length === 0 ? (
      <QoderEmptyCard
        error={error}
        sources={sources}
        onAdd={() => setAddDrawerOpen(true)}
        onReload={reload}
      />
    ) : null;

  /**
   * 「从本机导入」是否可用：任一来源不是 `missing` 即可点（`unreadable` 也要放行 ——
   * 点下去会给出可读的原因；禁用它等于把「读不出来」变成「点不动」）。
   * 探测未返回时**放行**：点了最坏也是一条诚实的报错，比一个闪烁的禁用态好。
   */
  const localInstalled = sources === null ? true : sources.sources.some((s) => s.state !== "missing");

  const productLabel = t(platformDescriptor(product).labelKey);

  return (
    <div className="mx-auto w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
      <header className="flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("qoder.page.accounts.title")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">
            {attentionCount > 0
              ? t("pageHead.accounts.pending", {
                  scope: scopeLabel(t, scope),
                  n: rows.length,
                  m: attentionCount,
                })
              : t("pageHead.accounts", { scope: scopeLabel(t, scope), n: rows.length })}
          </p>
        </div>
        <div className="grow" />
        <ScopeChip scope={scope} />
      </header>

      {/* ★ 账号工作台（**三平台共用容器**）—— 与 `AccountsPage` / `TraeAccountsPage`
          是**同一个组件**；差异只在数据（排序/分组选项、卡片渲染、列定义、空态）。 */}
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
        /* ★ 主操作 = 原型工具栏最右侧的 `+ 添加账号`（打开添加抽屉，方式卡由页面注入）。 */
        addAction={{
          label: t("shared.addDrawer.title"),
          onClick: () => setAddDrawerOpen(true),
        }}
        /* ★ 回收站入口（软删除的唯一恢复路径；后端已是软删除，没有入口 = 假承诺）。 */
        onOpenRecycleBin={() => setRecycleOpen(true)}
        loading={accounts === null}
        emptyState={emptyState}
        notices={
          <>
            {/* 无客户端形态（issue #62）：本机没有 Qoder 来源时，登录授权与网关照常可用 ——
                缺的只是「从本机导入」这条读客户端目录的路。空态卡片仍保留来源四态诊断。 */}
            {sources !== null && !localInstalled ? (
              <ClientlessStrip scopeLabel={scopeLabel(t, scopeOf("qoder", region))} />
            ) : null}
            {error && rows.length > 0 ? (
              <Alert variant="destructive" className="mb-4">
                <AlertTriangle />
                <AlertTitle>{t("qoder.page.accounts.loadFailed")}</AlertTitle>
                <AlertDescription>
                  <span>{error}</span>
                  {/* ★ 失败态**必须给一条出路**：没有它，用户唯一能做的就是重启应用。 */}
                  <div className="mt-2">
                    <Button size="sm" variant="outline" onClick={() => void run(() => reload())} disabled={refreshBusy}>
                      {refreshBusy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                      {t("qoder.page.accounts.recheck")}
                    </Button>
                  </div>
                </AlertDescription>
              </Alert>
            ) : null}
            {/* 归属不符的账号**逐条列出**（不静默丢弃）—— 它们确实在这个库里，
                只是 domain 属于另一个区域；删除或迁走是用户要做的决定。 */}
            {foreignAccounts.length > 0 ? (
              <Card className="mb-4 gap-0 border-warning/40 py-0">
                <div className="flex items-start gap-2.5 px-4 py-3.5">
                  <ShieldAlert className="mt-0.5 size-4 shrink-0 text-[var(--warning)]" />
                  <div className="min-w-0 text-[12.5px]">
                    {t("qoder.page.accounts.domainMismatch")}：
                    {foreignAccounts.map((a) => qoderAccountName(a)).join("、")}
                  </div>
                </div>
              </Card>
            ) : null}
          </>
        }
        checkinResult={
          checkinResult
            ? {
                okCount: checkinResult.okCount,
                failedNames: checkinResult.failed.map((item) => item.name),
                /* ⚠️ 「通道未开放」单独一档：不算成功、也不进重试桶。 */
                skippedNames: checkinResult.disabled.map((item) => item.name),
                /* 真领到的额度合计：只说「成功 1 个」不足以证明到账。 */
                earnedCredits: checkinResult.earned,
                /* 只重试失败项（原型 `checkin-bar` 的既有语义）。 */
                onRetry:
                  checkinResult.failed.length > 0
                    ? () => {
                        const failedUids = new Set(checkinResult.failed.map((item) => item.uid));
                        void runCheckin(
                          rows.filter((a) => failedUids.has(a.uid)),
                          checkinResult.okCount,
                          checkinResult.earned,
                        );
                      }
                    : undefined,
              }
            : null
        }
        onDismissCheckinResult={() => setCheckinResult(null)}
        batch={{
          selectedCount: selected.size,
          busy,
          onCheckin: () => void runCheckin(selectedRows),
          onRefresh: () => void runRefresh(selectedRows),
          onExport: () => {
            setExportTargets(selectedRows);
            setExportOpen(true);
          },
          onDelete: () => setBatchDeleteOpen(true),
          onClear: () => setSelected(new Set()),
        }}
        listAriaLabel={t("qoder.page.accounts.sectionAria", { count: rows.length })}
        totalCount={rows.length}
        visibleCount={sorted.length}
        emptyNone={{
          title: t("shared.list.empty.none"),
          description: t("shared.list.empty.noneDesc"),
        }}
        emptyFiltered={{
          title: t("shared.list.empty.filtered"),
          description: t("shared.list.empty.filteredDesc"),
          action: (
            <Button
              variant="outline"
              onClick={() => {
                setQuery("");
                setGroup("none");
              }}
            >
              {t("shared.list.empty.clearFilters")}
            </Button>
          ),
        }}
        buckets={buckets}
        columns={columns}
        rowKey={(account) => displayText(account.uid) ?? ""}
        selectedIds={selected}
        onToggleSelect={toggleSelect}
        onToggleBucketAll={toggleBucketSelection}
        bucketAllSelected={bucketAllSelected}
        bucketSomeSelected={bucketSomeSelected}
        onOpenRow={openDetail}
        /* 「当前登录」= 任一程序位登着这条账号（与卡片上的徽章**同一判据**）。 */
        isRowCurrent={(account) =>
          (programs ?? []).some((program) => program.currentUid === account.uid)
        }
        rowSelectLabel={(account) => t("shared.table.selectRowAria", { name: qoderAccountName(account) })}
        rowOpenLabel={(account) => t("shared.table.openRowAria", { name: qoderAccountName(account) })}
        /* ★ 表格视图的「更多」：与卡片**同一个菜单组件**（WB 的 `renderActions` 同形）。
           此前 Qoder 表格没有任何行尾动作 ⇒ 表格里除了点行就没有别的出口。 */
        renderActions={(account) => (
          <QoderAccountMenu
            compact
            account={account}
            name={qoderAccountName(account)}
            checkinToday={checkinTodayOf(account)}
            onOpenDetail={openDetail}
            onEditRemark={openRemark}
            onSetGroup={openGroup}
            onCheckin={(target) => void runCheckin([target])}
            onExport={(target) => {
              setExportTargets([target]);
              setExportOpen(true);
            }}
            onWebLogin={(target) => void openWebLogin(target)}
            onDelete={setPendingDelete}
            onRefreshPart={(kind) => onRefreshPart(account, kind)}
            refreshingKinds={refreshPartBusy.kindsOf(account.uid)}
            doneKinds={refreshPartBusy.doneKindsOf(account.uid)}
            refreshKinds={QODER_REFRESH_KINDS}
          />
        )}
        renderCard={(account) => {
          const row = credits[account.uid];
          return (
            <QoderAccountCard
              key={account.uid}
              account={account}
              selected={selected.has(account.uid)}
              onToggleSelect={toggleSelect}
              onOpenDetail={openDetail}
              onEditRemark={openRemark}
              onRefreshPart={onRefreshPart}
              doneKinds={refreshPartBusy.doneKindsOf(account.uid)}
              refreshingKinds={refreshPartBusy.kindsOf(account.uid)}
              refreshKinds={QODER_REFRESH_KINDS}
              onDelete={setPendingDelete}
              onSetGroup={openGroup}
              /* 签到 / 导出与批量操作**同一条路径**：菜单只是把目标缩到一个账号。 */
              onCheckin={(account) => void runCheckin([account])}
              onExport={(account) => {
                setExportTargets([account]);
                setExportOpen(true);
              }}
              onWebLogin={(target) => void openWebLogin(target)}
              onReadPacks={(target) => void readPacks(target)}
              packs={webPacks[account.uid]}
              packsLoading={!!packsLoading[account.uid]}
              onRequestPacks={(target) => void loadPacks(target.uid)}
              credits={
                row && row.ok
                  ? { remaining: row.remaining ?? 0, total: row.total ?? 0, plan: row.plan ?? null }
                  : null
              }
              checkinToday={checkinTodayOf(account)}
              programs={programs}
              onSwitch={(program) => setSwitchTarget({ account, program })}
            />
          );
        }}
        /* 列表下方的平台插槽：**限流**卡（与 WB 同一个 `ClientLimitsCard`、同一几何与时间口径）。
           ⚠️ `ledger={false}`：Qoder 没有可反推的客户端日志（`client_limits` 扫的是 WB 的 jsonl），
           不传它会把 **WB 的**限流记录画到本页上 —— 串台比空表坏得多。 */
        footer={
          <section className="mt-7 min-w-0">
            <ClientLimitsCard
              region={region}
              ledger={false}
              probePath="/qoder/api-service?sub=models"
            />
          </section>
        }
      />

      {/* 产品名走注册表（唯一来源）—— 本页在 Qoder 分区下渲染，但产品名不写死。 */}
      <p className="mt-6 text-[11.5px] text-muted-foreground">{productLabel}</p>

      {/* ── 添加账号抽屉（原型 `drawerAdd`：方式卡 + 平台专属注入） ── */}
      <AddAccountDrawer
        open={addDrawerOpen}
        onOpenChange={setAddDrawerOpen}
        scopeLabel={scopeLabel(t, scope)}
        installed={localInstalled}
        /* 基础三张卡**全部已落地**：OAuth 设备授权（本轮新增）/ 从本机导入 / 导入备份。
           粘贴 PAT 走 extraOptions（它不属于原型的三种「添加方式」）。 */
        ways={["oauth", "local", "backup"]}
        onPick={(way) => {
          /* 本机没有 Qoder 来源时，「从本机导入」没有目录可读 —— 给替代路径（issue #62）。 */
          if (way === "local" && !localInstalled) {
            toast.error(t("shared.srv.localUnavailable"), {
              description: t("shared.srv.localUnavailableDesc"),
            });
            return;
          }
          if (way === "oauth") setOauthOpen(true);
          if (way === "local") void onImportLocal();
          if (way === "backup") setImportOpen(true);
        }}
        extraOptions={[
          {
            key: "paste-pat",
            icon: KeyRound,
            title: t("qoder.page.accounts.pat.title"),
            desc: t("qoder.page.accounts.pat.description"),
            onClick: () => setPatOpen(true),
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

      {/* ── OAuth 设备授权弹窗（本轮新增） ── */}
      <QoderOAuthDialog
        open={oauthOpen}
        onOpenChange={setOauthOpen}
        scope={scope}
        onCollected={() => {
          void reload().then(() => loadEnrichment());
        }}
      />

      {/* ── 粘贴 PAT 弹层（「从本机导入」读不出来时的兜底路径） ── */}
      <PastePatDialog
        open={patOpen}
        onOpenChange={setPatOpen}
        scope={scope}
        onImported={() => void reload()}
      />

      {/* ── 导入 / 导出备份（**共用壳**，按 scope 分家） ── */}
      <ImportAccountsDialog
        open={importOpen}
        onOpenChange={setImportOpen}
        scope={scope}
        onImported={(result) => {
          toast.success(
            t("qoder.page.accounts.import.result", {
              imported: String(result.imported),
              updated: String(result.overwritten),
            }),
          );
          void reload();
        }}
      />
      <ExportAccountsDialog
        open={exportOpen}
        onOpenChange={(open) => {
          setExportOpen(open);
          // 关闭即清掉「导出所选」的目标：下次从抽屉进来仍是「全部」，不残留上一次的范围。
          if (!open) setExportTargets(null);
        }}
        accounts={(exportTargets ?? rows).map((account) => ({
          id: account.uid,
          label: qoderAccountName(account),
        }))}
        scope={scope}
      />

      {/* ── 回收站抽屉（**三平台共用壳**；恢复 = 移回 live，必须重取列表） ── */}
      <RecycleBinSheet
        open={recycleOpen}
        onOpenChange={setRecycleOpen}
        scope={scope}
        onRestored={() => void reload()}
      />

      {/* ── 切换确认（**写客户端登录态**，与 WB 的切换同级 ⇒ 必须先确认） ── */}
      <Dialog open={switchTarget !== null} onOpenChange={(open) => !open && setSwitchTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.page.accounts.switch.title")}</DialogTitle>
            <DialogDescription>
              {switchTarget
                ? t("qoder.page.accounts.switch.desc", {
                    name: qoderAccountName(switchTarget.account),
                    program: switchTarget.program.label,
                  })
                : ""}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setSwitchTarget(null)} disabled={switchBusy}>
              {t("qoder.page.accounts.switch.cancel")}
            </Button>
            <Button onClick={() => void confirmSwitch()} disabled={switchBusy}>
              {switchBusy ? <Loader2 className="size-4 animate-spin" /> : null}
              {t("qoder.page.accounts.switch.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── 账号详情抽屉（备注编辑在这里；操作区 = 刷新 / 签到 / 导出 / 删除） ── */}
      <AccountDetailDrawer
        open={detail !== null}
        onOpenChange={(open) => {
          if (!open) setDetail(null);
        }}
        // 页签记忆的身份键：`scope:uid`（国内版 / 国际版的同一个 uid 是两个账号）。
        accountKey={detail ? `${scope}:${displayText(detail.uid) ?? ""}` : ""}
        name={detail ? qoderAccountName(detail) : ""}
        subtitle={
          detail
            ? t("qoder.page.accounts.drawer.subtitle", {
                name: qoderAccountName(detail),
                uid: displayText(detail.uid) ?? "—",
              })
            : ""
        }
        avatarSeed={detail ? qoderAccountName(detail) : ""}
        sections={detail ? [{ key: "account", title: t("qoder.page.accounts.drawer.info"), rows: detailRows }] : []}
        packs={detailPacks}
        packsNotice={detailPacksNotice}
        stats={detailStats}
        refreshingKinds={detail ? refreshPartBusy.kindsOf(detail.uid) : []}
        onRefreshPart={detail ? (kind) => void onRefreshPart(detail, kind) : undefined}
        checkin={detailCheckin}
        proPack={detailProPack}
        remark={detail?.remark ?? ""}
        onSaveRemark={detail ? (remark) => saveRemark(detail, remark) : undefined}
        autoEditRemark={detailAutoEditRemark}
        group={detail?.group ?? ""}
        onSaveGroup={detail ? (group) => saveGroup(detail, group) : undefined}
        groups={detailGroups}
        autoEditGroup={detailAutoEditGroup}
        switchTargets={detail ? switchTargetsFor(detail) : []}
        /* 与 WB / Trae 抽屉**同一取值**（备注是否可编辑不受演示模式影响）。 */
        featuresDisabled={false}
        actions={detailActions}
      />

      {/* ── 单账号删除确认（**软删除** ⇒ 文案如实说「可在回收站恢复」） ── */}
      <Dialog open={pendingDelete !== null} onOpenChange={(open) => !open && setPendingDelete(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.page.accounts.delete.title")}</DialogTitle>
            <DialogDescription>
              {pendingDelete
                ? t("qoder.page.accounts.delete.desc", { name: qoderAccountName(pendingDelete) })
                : ""}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingDelete(null)} disabled={busy}>
              {t("qoder.page.accounts.delete.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmSingleDelete()} disabled={busy}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : null}
              {t("qoder.page.accounts.delete.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── 批量删除确认 ── */}
      <Dialog open={batchDeleteOpen} onOpenChange={setBatchDeleteOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.page.accounts.delete.batchTitle")}</DialogTitle>
            <DialogDescription>
              {t("qoder.page.accounts.delete.batchDesc", { n: String(selected.size) })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setBatchDeleteOpen(false)} disabled={busy}>
              {t("qoder.page.accounts.delete.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmBatchDelete()} disabled={busy}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : null}
              {t("qoder.page.accounts.delete.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/** 展示名走卡片模块的**唯一回落链**（页面、卡片、抽屉、toast 同一份）。 */

/**
 * 首屏空态卡（「这个区域还没有账号」）。
 *
 * 照原型 `emptyBlock`（标题 / 说明 / 出口按钮），出口 = 「添加账号」抽屉；并附
 * **本机来源四态**（`missing / unreadable / empty / ready` 对应四种不同的用户动作：
 * 去装 / 去客户端重新登录 / 换个来源 / 直接导入 —— 压成一个「未检测到」就会给错引导）。
 * 失败信息也在这里兜底展示（工作台在首屏空态时会整块替换，含 notices 槽）。
 */
function QoderEmptyCard({
  error,
  sources,
  onAdd,
  onReload,
}: {
  error: string;
  sources: QoderLocalSourcesView | null;
  onAdd: () => void;
  onReload: () => Promise<void>;
}) {
  const t = useT();
  const { busy, run } = useRefreshAction();
  return (
    <Card className="gap-0 py-0">
      {error ? (
        <div className="flex flex-wrap items-center gap-2.5 border-b px-4 py-3.5">
          <ShieldAlert className="size-4 shrink-0 text-destructive" />
          <div className="min-w-0 grow text-[12.5px]">{error}</div>
          <Button size="sm" variant="outline" onClick={() => void run(() => onReload())} disabled={busy}>
            {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("qoder.page.accounts.recheck")}
          </Button>
        </div>
      ) : null}
      <EmptyState
        icon={UserRound}
        title={t("qoder.page.accounts.empty.title")}
        description={t("qoder.page.accounts.empty.description")}
        action={
          <Button onClick={onAdd}>
            <Plus className="size-4" />
            {t("qoder.page.accounts.empty.action")}
          </Button>
        }
      />
      {sources ? (
        <div className="border-t">
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 px-4 pb-1 pt-3">
            <span className="text-[12.5px] font-[550]">{t("qoder.page.accounts.empty.sources")}</span>
            <span className="text-[11px] text-muted-foreground">
              {t("qoder.page.accounts.import.description")}
            </span>
          </div>
          {sources.sources.map((source) => (
            <QoderLocalSourceRow key={source.kind} source={source} />
          ))}
        </div>
      ) : null}
    </Card>
  );
}

/** 粘贴 PAT 弹层：前缀校验（`pt-`）在客户端先拦一道，导入走 `qoder_import_pat`。 */
function PastePatDialog({
  open,
  onOpenChange,
  scope,
  onImported,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  scope: Scope;
  onImported: () => void;
}) {
  const t = useT();
  const [pat, setPat] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  async function submit() {
    const value = pat.trim();
    if (!value.startsWith("pt-")) {
      setError(t("qoder.page.accounts.pat.invalid"));
      return;
    }
    setError("");
    setSaving(true);
    try {
      await api.qoderImportPat(scope, value);
      toast.success(t("qoder.page.accounts.import.result", { imported: "1", updated: "0" }));
      setPat("");
      onOpenChange(false);
      onImported();
    } catch (e) {
      setError(api.asError(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("qoder.page.accounts.pat.title")}</DialogTitle>
          <DialogDescription>{t("qoder.page.accounts.pat.description")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          <Label htmlFor="qoder-pat">PAT</Label>
          <Input
            id="qoder-pat"
            value={pat}
            onChange={(event) => {
              setPat(event.target.value);
              setError("");
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !saving) void submit();
            }}
            placeholder={t("qoder.page.accounts.pat.placeholder")}
            className="font-mono text-[12.5px]"
            spellCheck={false}
          />
          {error ? <div className="text-[11.5px] text-destructive">{error}</div> : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
            {t("qoder.page.accounts.pat.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={saving || !pat.trim()}>
            {saving ? <Loader2 className="size-4 animate-spin" /> : null}
            {t("qoder.page.accounts.pat.action")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
