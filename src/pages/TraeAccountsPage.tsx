import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import {
  AlertTriangle,
  CalendarCheck,
  Download,
  FileDown,
  ListChecks,
  Loader2,
  QrCode,
  RefreshCw,
  Rocket,
  UserPlus,
} from "lucide-react";
import { toast } from "sonner";

import { AddAccountDrawer, type AddAccountWay } from "@/components/add-account-drawer";
import { DemoAction } from "@/components/demo-action";
import { AccountsTableAvatar } from "@/components/account-list-parts";
import { AccountWorkbench } from "@/components/account-workbench";
import { type AccountColumn } from "@/components/account-table";
import { type ToolbarOption } from "@/components/account-toolbar";
import {
  AccountDetailDrawer,
  type DetailActions,
  type DetailCheckinHistoryRow,
  type DetailPack,
  type DetailProgramBadge,
  type DetailSection,
  type DetailSwitchTarget,
} from "@/components/account-detail-drawer";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { TraeAccountCard, formatCredits, jwtBadge, shortTime, type TraeProgram } from "@/components/trae-account-card";
import { TraeExportAccountsDialog } from "@/components/trae-export-accounts-dialog";
import { TraeImportAccountsDialog } from "@/components/trae-import-accounts-dialog";
import { TraeOAuthLoginDialog } from "@/components/trae-oauth-login-dialog";
import { RecycleBinSheet } from "@/components/recycle-bin-sheet";
import { RestartConfirmDialog } from "@/components/restart-confirm-dialog";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import * as api from "@/lib/api";
import { accountSearchText } from "@/lib/account-search";
import { lastSevenDays } from "@/lib/checkin-history";
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
import { displayText } from "@/lib/display-text";
import { useMediaQuery } from "@/lib/use-media-query";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import {
  loadScheduleConfig,
  saveSchedulePatch,
  SCHEDULE_CONFIG_KEY,
} from "@/lib/schedule-config";
import { traeVariantLabel, type TraeLogEntry } from "@/lib/trae-types";
import {
  TRAE_VARIANT_FALLBACK,
  loadTraeVariantLogins,
  loadTraeVariantStatuses,
  type TraeVariantLogins,
} from "@/lib/trae-variant-status";
import type { ScheduleConfig } from "@/lib/types";
import type {
  TraeAccount,
  TraeAccountsOverview,
  TraeCheckinEvent,
  TraeCheckinReport,
  TraeCheckinStatus,
  TraeCreditsOverview,
  TraeEnvStatus,
  TraeSettings,
  TraeVariantId,
  TraeVariantStatus,
} from "@/lib/trae-types";
import { cn } from "@/lib/utils";
import { scopeOf } from "@/lib/scope";
import { useCachedResource } from "@/lib/use-cached-resource";
import { showUndoToast } from "@/lib/undo-toast";
import { useTraeVariant } from "@/lib/use-trae-variant";

/**
 * 一次动作的**结果判定**（由调用方按后端报告如实给出）。
 *
 * ## 为什么要有这个类型（2026-09-29）
 *
 * 本仓不少后端写操作的失败是**正常返回的报告**，不是传输错误：
 * 切换账号回 `success:false` + `steps`、批量签到回 `failed: n`。
 * 而动作外壳原先只认「Promise 有没有 reject」⇒ 这些失败全被弹成「{label}完成」。
 * 报障原文：「切换成功，但是程序没有被打开，标记状态也没有变更」。
 *
 * 把「报成功还是报失败」变成**显式的返回值**，调用方就必须表态；
 * 省略 `message` 时按 `level` 取默认文案（success → 「{label}完成」，
 * 其余 → 「{label}失败」）。
 */
type ActionVerdict = {
  level: "success" | "warning" | "error";
  /** 省略时按 `level` 取默认文案。 */
  message?: string;
  description?: string;
};

/**
 * 账号页的快照。
 *
 * 这些数据**必须一起**缓存：卡片上的程序切换按钮要同时用到账号、程序位与各区域
 * 登录态，分开缓存会让「账号已是新的、按钮还是旧的」这种不一致有缝可钻。
 *
 * 缓存键里带 `variant`（产品线），所以切到另一条线绝不会渲染出上一条线的数据。
 */
interface AccountsSnapshot {
  overview: TraeAccountsOverview;
  env: TraeEnvStatus;
  credits: TraeCreditsOverview;
  checkin: TraeCheckinStatus;
  /** 本机全部产品线的环境状态（并排视角），账号卡片与状态条共用。 */
  variantStatuses: TraeVariantStatus[];
  /** 每条产品线各自的当前登录账号（`profiles.currentAccount` 的 uid + 展示名）。 */
  logins: TraeVariantLogins;
}

/**
 * 「账号管理」页（Trae 分区）。
 *
 * ## 与 WorkBuddy 页面的关系
 *
 * **骨架逐段照抄** WorkBuddy 的 `AccountsPage`：产品状态标记行 → 「添加与迁移账号」
 * 横幅 → 环境说明行 → 空态卡 → 账号工具栏 → 卡片栅格 → 各确认 Dialog。
 * 用户在两个分区之间切换时不需要重新学习界面。
 *
 * **文案与数据源按 Trae 实情落地**，不搬 WorkBuddy 的：
 * - 区域（国内版 / 国际版）与 WorkBuddy **语义完全对应**，且**切换入口统一在顶栏**
 *   （`ScopeSelector`，区域已并入 L1 作用域）：这些页面级切换器（`TraeVariantBar` /
 *   `TraeVariantSwitch` / `WorkbuddyRegionSwitch`）已删除，页头只留一枚**只读** `ScopeChip`。
 *   承载标识见 `useTraeVariant`（返回**区域**；旧值 `trae_work`/`trae_cn` 一律回落国内版，
 *   否则老书签会去读空库）；
 * - 卡片头部的**程序切换按钮**（该区域的每个 Trae 程序各一枚）对应 WorkBuddy 卡片上的
 *   WorkBuddy / CodeBuddy IDE / CodeBuddy CLI 三枚按钮——都是「把这个账号挂到哪个
 *   客户端上」。这个维度 Trae 叫**程序位**（TraeWork / TraeCode，见 `TraeProgram`），
 *   与「区域」是两层：区域决定账号库与端点，程序位决定写进哪个客户端；
 * - **支持 OAuth 网页登录**（`trae-oauth-login-dialog.tsx` + OAuth 三命令），
 *   粘贴 `Cloud-IDE-JWT` 只是「优先用网页登录」的兜底方式；
 * - **有自动签到**（排程任务 `trae_checkin`，默认关闭）：与 WorkBuddy 工具栏的同名开关
 *   同位同义 —— 到点由后台调度器签一轮、应用启动时若当天还没签则补一轮，两个区域各签一遍；
 *   小时点在 Trae 设置页配置。它管**何时签**；
 * - 「跳过今日已签」是**策略**开关（管**怎么签**）：手动与自动两轮都受它约束，
 *   两者是不同层，因此并排出现并不重复；
 * - 没有自动旅行（Trae 侧不存在该客户端）。
 */
export default function TraeAccountsPage() {
  /**
   * 当前正在管理的**产品线变体**，由侧栏分区决定（URL `?line=`）。
   *
   * 为什么**不再**用「自动探测挑中的那一条」：探测是无入参、横跨全部变体的
   * 全局行为，它回答的是「本机哪条线最近活跃」，而不是「用户此刻想管哪条线」。
   * 用户点了侧栏的「Trae CN」，页面就必须是 Trae CN —— 哪怕 Trae Work 更活跃。
   *
   * 探测结果仍然有用：它作为**兜底**（URL 没有显式指定时）以及用于展示
   * 「该线的安装/运行状态」。两者分工不同，不要混为一谈。
   */
  /* ⚠️ 只取值、不取 setter：切区域的动作在**顶栏 `ScopeSwitch`**（作用域是路由状态的一部分）。
     2026-10-10 前这里还取了 setter 给卡片「跨区域程序位」引导用 —— 那条支路已随
     「国内版不显示国际版程序位」一起删除，setter 因此不再需要。 */
  const [variant] = useTraeVariant();
  const t = useT();

  /**
   * 一次取齐本页快照。四份分家数据全部按**当前选中的产品线**读取，与侧栏分区同源。
   * `env` 仍然单独取：它是「单一视角」的环境快照，用于展示该线的安装/运行状态。
   *
   * `statuses` 与「各线当前账号」则是**跨产品线**的：卡片上的程序切换按钮条
   * （对应 WorkBuddy 卡片的 WorkBuddy / CodeBuddy IDE / CLI 三枚按钮）必须知道
   * 「本机装了哪几条线、每条线当前挂的是哪个账号」，缺一条就渲染不出那一枚按钮。
   */
  const loadSnapshot = useCallback(async (): Promise<AccountsSnapshot> => {
    const [accountData, envData, creditData, checkinData, statuses] = await Promise.all([
      api.getTraeAccounts(variant),
      api.getTraeEnv(),
      api.getTraeCredits(variant),
      api.getTraeCheckinStatus(variant),
      loadTraeVariantStatuses(),
    ]);
    // 登录态读取依赖刚拿到的产品线清单（要遍历它逐条读），故串在探测之后。
    // 它自己逐条容错：某条线读不到只记 `null`，不会把整页拖成错误态。
    const currentLogins = await loadTraeVariantLogins(statuses);
    return {
      overview: accountData,
      env: envData,
      credits: creditData,
      checkin: checkinData,
      variantStatuses: statuses,
      logins: currentLogins,
    };
  }, [variant]);

  /**
   * 快照缓存（`stores/resources.ts`）。
   *
   * 数据持有权从组件搬到 store，是为了**跨挂载存活**：侧栏切到 WorkBuddy 再切回来时
   * 页面组件会卸载重建，数据留在 store 里 ⇒ 重挂载立刻渲染真数据、不再闪骨架。
   * `loadAll` 是「强制重取并等待」，变更操作（签到 / 切换 / 删除 / 导入）之后必须调它。
   */
  const {
    data: snapshot,
    loading,
    error,
    refresh: loadAll,
  } = useCachedResource<AccountsSnapshot>(`trae:accounts:${variant}`, loadSnapshot);

  /**
   * 签到跳过策略存在设置里，与设置页**共用同一把键** `trae:settings`：
   * 两个页面本来就都读同一份 `get_trae_settings`，共用键之后「设置页改完、账号页
   * 立刻是新的」在结构上成立，而不是靠各自重取去撞。
   */
  const { data: settings, patch: patchSettings } = useCachedResource<TraeSettings>(
    "trae:settings",
    api.getTraeSettings,
  );

  /**
   * 自动签到的**排程**（何时跑），与 Trae 设置页共用同一把键（`SCHEDULE_CONFIG_KEY`）。
   *
   * 它和上面那份 `trae:settings` 是**两份不同的数据**：
   * 这里回答「到点要不要跑、几点跑」，`trae:settings` 回答「跑的时候怎么跑」
   * （跳过已签 / 跳过过期 / 重试次数）。排程是**全局单份**、按**任务**分家 ——
   * WorkBuddy 的六类任务与它读写同一个 `schedule_config.json`。
   */
  const { data: schedule, patch: patchSchedule } = useCachedResource<ScheduleConfig>(
    SCHEDULE_CONFIG_KEY,
    loadScheduleConfig,
  );

  /**
   * 本机全部 Trae 产品线的环境状态（并排视角），与状态条、卡片共用同一份。
   *
   * 账号卡片上的「程序切换按钮」要按**每条线**渲染，因此这里必须拿全量，
   * 而不是 `env`（后者只是「自动挑中的那一条」的单一视角）。
   *
   * 初值是**兜底的两条线占位**而不是空数组：状态条与卡片都直接按它渲染，
   * 空数组会让首次加载期间出现「一枚 Tab 都没有」的空条（模板占了位却什么都没画）。
   * 用占位起步、加载完成后替换，形态与 WorkBuddy 的 region Tabs 一致
   * （后者也是先渲染两枚 Tab，再逐区填状态）。
   */
  const overview = snapshot?.overview ?? null;
  const credits = snapshot?.credits ?? null;
  const checkin = snapshot?.checkin ?? null;
  const variantStatuses = snapshot?.variantStatuses ?? TRAE_VARIANT_FALLBACK;
  /**
   * 每条产品线各自的当前登录账号（`profiles.currentAccount` 的 uid + 展示名）。
   *
   * **不能**只读当前这条线：卡片上每条程序按钮的「是否当前账号」是**各判各的**，
   * 同一个 Trae 账号完全可以同时是 Trae Work 的当前账号、却不是 Trae CN 的。
   * 只拿当前线的值会让另一枚按钮永远显示成「未启用」——那是谎报，不是简化。
   *
   * 身份与展示名同源（同一次 `get_trae_profiles`）：卡片用 `userId` 比身份，
   * 状态条用 `name` 显示 —— 两者绑在一个对象里，不会出现半更新。
   */
  const logins = snapshot?.logins ?? {};

  const [busy, setBusy] = useState<string | null>(null);
  /** 「跳过已签到」（签到策略，写 `trae:settings`）的保存中标志。 */
  const [autoCheckinSaving, setAutoCheckinSaving] = useState(false);
  /**
   * 「自动签到」（排程，写 `schedule_config.json`）的保存中标志。
   *
   * 与上面那个**刻意分开**：两者写的是两份不同的文件、属于两个不同的概念层
   * （何时签 vs 怎么签）。共用一个标志会让「切 A 时 B 的转圈亮起」，把操作归因到错的地方。
   */
  const [scheduleSaving, setScheduleSaving] = useState(false);
  const [oauthOpen, setOauthOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  /** 「添加账号」抽屉（两平台同一入口；原工具栏「⋯」菜单的内容都落在这里）。 */
  const [addDrawerOpen, setAddDrawerOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  /**
   * 导出目标：`null` = 导出**全部**账号（工具栏入口）；非 `null` = 导出**所选**账号（批量条入口）。
   *
   * ⚠️ 与 WorkBuddy **同名同义**（`AccountsPage` 的 `exportTargets`）：批量条上的「导出」
   * 与工具栏上的「导出备份」范围不同（所选 vs 全部），必须分成两个状态，
   * 否则「关闭时清空选择」与「打开时带选择」会互相踩。
   */
  const [exportTargets, setExportTargets] = useState<TraeAccount[] | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<TraeCheckinEvent | null>(null);
  const [report, setReport] = useState<TraeCheckinReport | null>(null);
  /**
   * P4 工具栏：搜索 / 排序 / 分组 / 视图（**页面私有 `useState`**，`PLAN §2.1 L3` / D1）。
   * 不进 URL、不进 store、不碰作用域，且**不持久化**（决策 #2）。
   *
   * ⚠️ 2026-10-10：原先另有一个「分组**筛选**」状态（`groupFilter`，只看某一组），
   * 已随工具栏上那个 Trae 独有的下拉一起删除（用户要求工具栏与 WorkBuddy 一致）。
   * 分组信息仍在：卡片上有分组标签，`group = "group"` 仍能按分组**分桶**。
   */
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<AccountSortKey>("priority");
  const [group, setGroup] = useState<AccountGroupKey>("none");
  const [view, setView] = useState<AccountViewMode>("card");
  const tableEnabled = useMediaQuery("(min-width: 900px)");
  const searchRef = useRef<HTMLInputElement>(null);

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
   * 与 `AccountsPage` 同构：读一次播种页内搜索 state，随后 `replace` 清参
   * （视图参数留页内，§2.1 L3）。`seededRef` 保证只读一次。
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

  /** 待确认删除的账号。用 shadcn Dialog 承载确认，而非 `window.confirm`——
   *  Tauri WebView 不支持原生 confirm，且原生弹窗无法保持主题与无障碍契约。 */
  const [pendingDelete, setPendingDelete] = useState<TraeAccount | null>(null);
  /** 回收站抽屉开关（P5 §2.7.1：按**当前作用域**过滤，入口在工具栏）。 */
  const [recycleOpen, setRecycleOpen] = useState(false);

  /**
   * 待确认的「切换到某个程序位」目标（P8 T03 / §3.15.3）。
   *
   * 切换 TraeWork / TraeCode 会**先关再开**客户端（`trae::profile::switch_account` 的
   * `launch` 阶段）⇒ 与 WorkBuddy / CodeBuddy IDE 走**同一条**确认流程（默认勾选自动重开 +
   * 未保存文件风险）。`restart` 映射到后端的 `launch`：取消勾选 ⇒ `launch: false`
   * （只换登录态、不拉起客户端）。
   */
  const [switchConfirm, setSwitchConfirm] = useState<{
    userId: string;
    variant: TraeVariantId;
    label: string;
  } | null>(null);

  /**
   * P4：详情抽屉 + 最小批量选择（与 WorkBuddy 账号页 `AccountsPage` 同构）。
   *
   * 抽屉状态由**页面**持有（卡片只发 `onOpenDetail`）：抽屉需要「这个账号的全部明细」，
   * 而那些数据（逐包积分、程序位、凭据）只有页面手里齐全。
   * `detailAutoEditRemark` 区分两个入口：点「查看详情」只打开抽屉；点「更多 → 编辑备注」
   * 打开抽屉并**直接进入编辑态**（`autoEditRemark`）。
   *
   * 选择集用 `Set<userId>`：Trae 账号的身份就是 `userId`（改名不改身份）。
   */
  const [detailAccount, setDetailAccount] = useState<TraeAccount | null>(null);
  /**
   * ★ 抽屉「签到历史（近 7 天）」的数据源（打开抽屉时拉取）。
   *
   * `get_trae_logs({ kind: "checkin" })` 是本机**真实**的签到日志（`TraeLogEntry` 带 `date`），
   * 与 WorkBuddy 侧的 `getCheckinLogs()` 同一层语义。取不到时 `null` ⇒ 该区块**不渲染**
   * （不摆一张空表，也不臆造「+N」）。
   */
  const [detailCheckinLogs, setDetailCheckinLogs] = useState<TraeLogEntry[] | null>(null);
  const [detailAutoEditRemark, setDetailAutoEditRemark] = useState(false);
  /**
   * ★ 跨作用域选择集按 **(scope, userId)** 键控（P5 §2.5 第 (4) 条）。
   *
   * scope 由 `variant`（本页的 `variant` 即**区域**，见 `useTraeVariant` 文档）派生：
   * `trae-cn` / `trae-global`。键里带上 scope：切区域后旧键自然失配 ⇒ `targets` 恒为 0，
   * **不会**用 A 区域的 userId 去打 B 区域的库（或静默空转）。P4 已在 `variant` 变化时清空
   * （见下方 effect），这里是**键控层面**的加固。
   */
  const scope = scopeOf("trae", variant);
  const selectionKey = (userId: string) => `${scope}:${userId}`;
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [batchBusy, setBatchBusy] = useState(false);
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  /**
   * 批量签到**结果条**（原型 `state.checkinResult`，`buddy-switch-prototype.html:2339-2354`）。
   *
   * ⚠️ 与 WorkBuddy **同一条**结果条、同一位置（工具栏与列表之间）——
   * 此前 Trae 侧是一张页面底部的大 Card，形状与位置都与 WB 不同。
   */
  const [checkinResult, setCheckinResult] = useState<{ ok: number; failedIds: string[] } | null>(null);

  /**
   * 状态条第二行（每条产品线各自已登录哪个账号）的数据由**本页持有并注入**，
   * 状态条不再自己读一遍。
   *
   * ## 为什么不再用「修订号」触发状态条重读
   *
   * 状态条原先自己读各线登录态，因此需要一个 `variantBarRevision`：否则用户执行
   * 「切换账号 / 保存登录态 / 删除账号 / OAuth 新增」之后，页面主体已更新、
   * 状态条第二行却仍写着旧账号名。
   *
   * 现在卡片上的程序切换按钮**本来就要**每条线的当前账号（否则画不出「那条线上
   * 有没有挂着这个账号」），本页于是成为唯一数据源，状态条改为接收 `logins`。
   * 一次性取齐、同一次渲染下发，不一致在结构上就不可能出现 —— 修订号随之取消，
   * 它要解决的问题已经不存在了。
   */

  /**
   * 一次性遗留迁移（幂等）：旧「产品线」账号库并入国内版区域账号库 + 客户端合并后
   * 遗留的登录态快照搬进合并版槽位。
   *
   * ## 为什么挂在页面挂载时
   *
   * 后端在没有旧库 / 已经并完 / 没有可搬的快照时立刻返回 `changed: false`（成本只是一次
   * 文件存在性判断），所以不需要前端再维护「跑过没有」的状态 —— 那反而会引入
   * 「换了台机器/清了缓存就不跑了」这类新缺陷。依赖 `loadAll` 会让切换产品线时
   * 再调一次：**这是有意的**，代价是一次廉价请求，换来的是「用户切过去时数据已就绪」。
   *
   * ## ⚠️ 两个 `changed` 必须分开看（issue #20）
   *
   * `report.changed` 只表示**账号库**被改写了，`report.snapshots.changed` 才是快照搬运。
   * 两者互相独立：账号库早就并完、只剩快照没搬的机器上，只看前者会让提示**永不出现**，
   * 用户则继续卡在「切换账号失败：登录态快照不存在」。
   *
   * ## 为什么只在真并了东西时提示
   *
   * 改写用户数据这件事，用户有权知道改了什么、旧数据备份在哪 ——
   * 因此提示里带上账号数、分组数与**备份路径**；备份失败单独警告（凭据仍在旧文件里，
   * 未被删除，所以不必阻断）。快照搬运同理：搬了几个、有没有搬失败都要说。
   *
   * 失败**不阻断页面**：读侧此刻仍按旧库工作，账号一个都没少，用户照常用。
   */
  useEffect(() => {
    if (api.isDemoMode()) return;
    let disposed = false;
    void (async () => {
      try {
        const report = await api.traeMergeLegacyRegions();
        if (disposed) return;
        const snapshots = report.snapshots;
        const snapshotsChanged = snapshots?.changed ?? false;
        // 两个 changed 任一为真才提示（见上方说明）。
        if (!report.changed && !snapshotsChanged) return;
        toast.success(
          report.changed
            ? t("trae.page.accounts.mergeDone")
            : t("trae.page.accounts.mergeSnapshotsDone"),
          {
            description: [
              // 账号库那几行只在账号库真被改写时才出现 —— 快照-only 的机器上
              // 报「并入 0 个账号」是噪声，还会让人以为账号出了问题。
              report.changed
                ? t("trae.page.accounts.mergeSummary", {
                    added: report.accountsAdded,
                    kept: report.accountsKept,
                  })
                : null,
              report.changed && report.groupsAdded > 0
                ? t("trae.page.accounts.mergeGroups", { count: report.groupsAdded })
                : null,
              report.changed && report.backup
                ? t("trae.page.accounts.mergeBackup", { path: report.backup })
                : null,
              report.changed && report.backupFailed
                ? t("trae.page.accounts.mergeBackupFailed")
                : null,
              snapshotsChanged
                ? t("trae.page.accounts.mergeSnapshots", { count: snapshots.slotsCopied })
                : null,
              snapshots?.errors?.length
                ? t("trae.page.accounts.mergeSnapshotsFailed", {
                    count: snapshots.errors.length,
                  })
                : null,
            ]
              .filter(Boolean)
              .join(" · "),
          },
        );
        await loadAll();
      } catch (e) {
        toast.error(t("trae.page.accounts.mergeFailed"), { description: api.asError(e) });
      }
    })();
    return () => {
      disposed = true;
    };
  }, [loadAll]);

  // 签到进度：仅桌面端有事件通道；webui 只在结束时拿到完整报告。
  useEffect(() => {
    if (!api.isDesktop()) return;
    let dispose: (() => void) | undefined;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        dispose = await listen<TraeCheckinEvent>("trae-checkin-progress", (event) => {
          setProgress(event.payload);
        });
      } catch {
        // 事件通道不可用不影响主流程（仍可用返回的完整报告渲染结果）。
      }
    })();
    return () => dispose?.();
  }, []);

  /**
   * 统一的动作执行：加忙标记、**判定结果**、提示、随后刷新聚合数据。
   *
   * ## ★ 为什么第 6 个参数是「判定」而不是「自定义成功文案」（2026-09-29 改）
   *
   * 旧签名只给了一个 `successMessage`（返回 `null` 就用默认的「{label}完成」）。
   * 它的语义**预设了成功** —— 于是「后端正常返回一份带失败的报告」这类动作
   * （切换的 `success:false`、签到的 `failed: n`）在界面上**永远报「完成」**，
   * 而调用方连改口的机会都没有。报障原文：「切换成功，但是程序没有被打开」。
   *
   * 现在改成返回一份**判定**（`ActionVerdict`），由调用方按后端报告如实表态；
   * 不传时保持旧行为（成功 → 「{label}完成」）。这样「失败也报完成」这类缺陷
   * 在**签名层面**就写不出来了，而不是靠每个调用点各自记得。
   */
  async function run<T>(
    key: string,
    /** 动作的文案键；渲染时才取词，切换语言后同一枚按钮读到的就是新语言。 */
    labelKey: TranslationKey,
    action: () => Promise<T>,
    labelParams?: Record<string, string | number>,
    after?: (result: T) => void,
    verdictOf?: (result: T) => ActionVerdict,
  ) {
    setBusy(key);
    const label = t(labelKey, labelParams);
    try {
      const result = await action();
      after?.(result);
      const verdict = verdictOf?.(result) ?? { level: "success" as const };
      const message =
        verdict.message ??
        t(
          verdict.level === "success"
            ? "trae.page.accounts.actionDone"
            : "trae.page.accounts.actionFailed",
          { label },
        );
      const emit =
        verdict.level === "error" ? toast.error : verdict.level === "warning" ? toast.warning : toast.success;
      emit(message, { description: verdict.description });
      await loadAll();
    } catch (e) {
      toast.error(t("trae.page.accounts.actionFailed", { label }), { description: api.asError(e) });
    } finally {
      setBusy(null);
    }
  }

  /**
   * 用户在统一重启确认框里确认后，真正把账号挂到指定程序位。
   *
   * `restart`（确认框的「自动重开」）映射到后端 `launch`：`true` ⇒ 切换后拉起客户端；
   * `false` ⇒ 只换登录态、不拉起（与 WorkBuddy 的 `restart` 同一语义）。
   *
   * ⚠️ `programLabel` 由调用方从**程序位自己的展示名**传进来（卡片/抽屉读的是后端
   * `TraeProgramStatus.label`），**不得**在这里用 `traeVariantLabel(programVariant)` 反推：
   * 变体 `trae_cn` 同时属于遗留 `trae_code` 与合并版 `trae`（issue #20，两者共用变体），
   * 反推会把合并版说成 `TraeCode` —— 用户报障原文就是「我点的是 trae 不是 TraeCode」。
   */
  async function doSwitchTo(
    userId: string,
    programVariant: TraeVariantId,
    programLabel: string,
    restart: boolean,
  ) {
    await run(
      `switch-${userId}@${programVariant}`,
      "trae.page.accounts.actionSwitch",
      () => api.traeSwitchAccount({ userId, launch: restart, variant: programVariant }),
      { line: programLabel },
    );
  }

  /**
   * 保存账号备注（与 WorkBuddy 账号页的同名函数同形）。
   *
   * 成功后**重新拉取账号列表**而不是就地改 state：备注是唯一由用户手改的字段，
   * 后端会把全空白备注归一成「没有备注」（删键）—— 前端就地改就会留下一个空串，
   * 卡片上表现为「备注还在，只是看不见」。
   *
   * 返回布尔值而不是抛异常：卡片据此决定要不要收起编辑器。失败时保持编辑态，
   * 用户能改完重试，不会把半截输入丢掉。
   */
  async function saveRemark(account: TraeAccount, remark: string): Promise<boolean> {
    try {
      await api.traeSetAccountRemark(account.userId, remark, variant);
      await loadAll();
      toast.success(
        remark.trim()
          ? t("trae.page.accounts.remarkSaved")
          : t("trae.page.accounts.remarkCleared"),
      );
      return true;
    } catch (e) {
      toast.error(t("trae.page.accounts.remarkSaveFailed"), { description: api.asError(e) });
      return false;
    }
  }

  /**
   * 「自动签到」开关 —— **排程层**的开关（到点由后台调度器签一轮、应用启动时补一轮）。
   *
   * 与 WorkBuddy 工具栏的同名开关**同位同义**（都读写 `schedule_config.json` 里各自那一类
   * 任务），差别只在 Trae 是**独立任务**：关掉 WorkBuddy 的自动签到不会连带关掉 Trae 的，
   * 两边也可以设不同的小时点。
   *
   * 保存走**整份提交**（见 `saveSchedulePatch`）：后端按整份解析，只发一个字段会让
   * 其余任务的配置被默认值覆盖 —— 那是静默的配置丢失。
   */
  async function onAutoCheckinChange(enabled: boolean) {
    if (!schedule || scheduleSaving) return;
    const previous = schedule;
    // 乐观更新写进**快照缓存**：这个值归 `SCHEDULE_CONFIG_KEY` 那把键所有，
    // 写回组件 state 会让「设置页/账号页读到同一份」在结构上不再成立。
    patchSchedule((prev) => ({ ...prev, trae_checkin_enabled: enabled }));
    setScheduleSaving(true);
    try {
      const saved = await saveSchedulePatch(previous, { trae_checkin_enabled: enabled });
      patchSchedule(() => saved);
    } catch (e) {
      patchSchedule(() => previous);
      toast.error(t("trae.page.accounts.autoCheckinSaveFailed"), { description: api.asError(e) });
    } finally {
      setScheduleSaving(false);
    }
  }

  /**
   * 「跳过今日已签到」开关。
   *
   * 与 WorkBuddy 工具栏的第二个开关**同位**（那边是「自动旅行」；Trae 没有该功能，
   * 按用户决定「Trae 无自动旅行则隐藏」处理，不塞假开关占位）。
   *
   * 它属于**签到策略**（怎么签），与上面的自动签到（何时签）是两件事：
   * 后端从设置读（`handlers::parse_checkin_options`），因此手动与自动两轮**都**受它约束。
   */
  async function onSkipCheckedChange(enabled: boolean) {
    if (!settings || autoCheckinSaving) return;
    const previous = settings;
    // 乐观更新写进**快照缓存**而不是组件 state：这个值现在归 `trae:settings` 那把键
    // 所有，写回本地 state 会让「设置页/账号页读到同一份」在结构上不再成立。
    patchSettings(() => ({ ...previous, checkinSkipChecked: enabled }));
    setAutoCheckinSaving(true);
    try {
      const saved = await api.saveTraeSettings({ checkinSkipChecked: enabled });
      patchSettings(() => saved);
    } catch (e) {
      patchSettings(() => previous);
      toast.error(t("trae.page.accounts.settingsSaveFailed"), { description: api.asError(e) });
    } finally {
      setAutoCheckinSaving(false);
    }
  }

  /**
   * 工具栏唯一图标按钮：**签到并刷新全部账号积分**（与 WorkBuddy 同名同形）。
   *
   * Trae 侧这是两个接口（签到、积分快照），WorkBuddy 是一个后端动作，
   * 但对用户的语义完全一致——「点一下把我所有账号都签一遍并把积分刷新出来」。
   * 因此这里必须**两步都做完**再收工：只签到会让卡片上的积分停留在旧值，
   * 用户点完看到的数字没动，会以为签到没生效。
   *
   * 跳过策略由 `settings.checkinSkipChecked` 决定（后端读，不在这里传）。
   */
  async function checkinAll() {
    await run(
      "checkin",
      "trae.page.accounts.actionCheckinAll",
      async () => {
        // 变体决定「签哪条产品线的账号」以及「冷却/摘要落哪份文件」，
        // 不传会让后端回落默认变体 —— 在 Trae CN 页面上点签到却签了 Trae Work。
        const result = await api.traeCheckin({ scope: "all", variant });
        // 积分刷新失败不应让整次操作报错：签到本身可能已经成功，
        // 把「积分没刷出来」当成签到失败会让用户误以为一分没拿到。
        try {
          await api.traeRefreshCredits(undefined, variant);
        } catch (e) {
          toast.warning(t("trae.page.accounts.creditsRefreshFailed"), { description: api.asError(e) });
        }
        return result;
      },
      undefined,
      (result) => setReport(result),
      // ★ 判定必须按**后端报告**如实给（2026-09-29）：批量签到是「跑完了」，
      //   但「跑完」不等于「成功」—— 全部失败时还报「签到并刷新积分完成」就是谎报，
      //   与切换那条是同一个毛病。口径对齐 WorkBuddy 的 `AccountsPage.onRefreshCredits`：
      //   全失败 ⇒ `error`；部分失败 ⇒ 仍算成功，但描述里点出失败数；
      //   一轮没轮到任何账号（「跳过今日已签」全命中）⇒ 明说「没有需要签到的账号」。
      (result) => {
        if (result.total === 0) {
          return { level: "success" as const, message: t("trae.page.accounts.noNeedCheckin") };
        }
        const parts: string[] = [];
        if (result.totalOk > 0) {
          parts.push(t("trae.page.accounts.checkinBatchSuccess", { n: result.totalOk }));
        }
        if (result.already > 0) {
          parts.push(t("trae.page.accounts.checkinBatchAlready", { n: result.already }));
        }
        if (result.failed > 0) {
          parts.push(t("trae.page.accounts.checkinBatchFailed", { n: result.failed }));
        }
        const description = parts.join(t("shared.punct.comma"));
        return result.failed === result.total
          ? { level: "error" as const, description }
          : { level: "success" as const, description };
      },
    );
  }

  /**
   * 单账号签到（卡片菜单里的「手动签到」，与 WorkBuddy 卡片的同名菜单项对齐）。
   *
   * 与 `checkinAll` 的差别只有范围：走 `scope: "selected"` + 当个 `userId`，
   * 与「全部签到」共用同一条后端路径、同一套跳过与冷却规则 ——
   * **不为单账号另开旁路**，否则「整批签到会跳过它、单独点却签了」这类不一致
   * 迟早出现，且两个入口的说辞会互相矛盾。
   *
   * 不走 `run()`：批量动作统一报「××完成」够用，但单账号签到的结果有三种真实形态
   * （签到成功 / 今天已签 / 失败），用户点一下就该直接看到是哪一种，
   * 而不是先收到一句通用的「签到完成」、再自己到底下那张结果卡里找答案。
   */
  async function checkinOne(account: TraeAccount) {
    const label = account.name || account.userId;
    setBusy(`checkin-${account.userId}`);
    try {
      const result = await api.traeCheckin({
        scope: "selected",
        userIds: [account.userId],
        variant,
      });
      setReport(result);
      const outcome = result.results[0];
      if (!outcome) {
        // 一条结果都没有 = 该账号在计划阶段就被跳过（今日已签 / 冷却中 / 凭据过期）。
        // 如实说「没有执行」，不要为了好看谎报成功。
        toast.info(t("trae.page.accounts.checkinNotRun"), {
          description:
            result.warnings[0] ??
            t("trae.page.accounts.checkinSkippedDesc", { name: label }),
        });
      } else if (outcome.action === "skip_already") {
        toast.success(t("trae.page.accounts.alreadyCheckedToday"), { description: label });
      } else if (!outcome.ok) {
        toast.error(t("trae.page.accounts.checkinFailed"), {
          description: t("trae.page.accounts.checkinFailedDesc", {
            name: label,
            reason: outcome.message || outcome.action,
          }),
        });
      } else {
        toast.success(t("trae.page.accounts.checkinSuccess"), {
          description:
            outcome.delta > 0
              ? t("trae.page.accounts.checkinSuccessDesc", { name: label, delta: outcome.delta })
              : label,
        });
      }
      await loadAll();
    } catch (e) {
      toast.error(t("trae.page.accounts.checkinFailed"), { description: api.asError(e) });
    } finally {
      setBusy(null);
    }
  }

  /** 导入本机账号：读客户端登录态里的 `Cloud-IDE-JWT`，已存在的账号就地覆盖刷新。 */
  async function importLocal() {
    if (importing) return;
    setImporting(true);
    try {
      // 把当前产品线传下去：Trae 多条产品线可同机并存，不传会让后端按「最近活跃」
      // 全局挑一条 —— 用户在 Trae Work 分区导入却可能读到 Trae CN 的目录，
      // 连报错文案都会说错产品线。
      //
      // 用已解析好的 `variant` 状态而不是 `env?.variant`：后者在 env 还没加载完时
      // 是 `null`，会让「用户抢在加载完成前点导入」落到默认变体上；
      // `variant` 初值就是默认变体、加载完成后被修正，语义更稳。
      const result = await api.traeImportLocalAccount(variant);
      const who = result.name || result.userId;
      toast.success(t("trae.page.accounts.importLocalDone"), {
        description: result.userId
          ? t("trae.page.accounts.importLocalUid", { name: who, uid: result.userId.slice(-6) })
          : who,
      });
      await loadAll();
    } catch (e) {
      toast.error(t("trae.page.accounts.importLocalFailed"), { description: api.asError(e) });
    } finally {
      setImporting(false);
    }
  }

  /**
   * P4：勾选 / 详情入口 / 最小批量动作（与 WorkBuddy 账号页同构）。
   *
   * 选择集按 `userId`（身份）记账：Trae 账号改名不改身份，按 `name` 记账会在改名后失配。
   */
  function toggleSelect(account: TraeAccount) {
    const key = selectionKey(account.userId);
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
   * P4：区域变化时清空勾选（同 WorkBuddy 账号页）。
   *
   * `selectedKeys` 是组件 state，切区域只改 URL `?line=`、**不 remount** ⇒ 选择集跨区域残留：
   * 批量条显示「已选 N」，而 `targets = accounts.filter(selectedKeys)` 恒为 0（动作静默无操作）。
   *
   * 用 effect 监听 `variant` 变化来清，而不是去接管区域 setter —— 后者是「页面内切区域」的能力，
   * P3 已明确不该有。
   */
  useEffect(() => {
    setSelectedKeys(new Set());
  }, [variant]);

  /**
   * 请求把账号切到指定 Trae 产品线：**先走统一重启确认**（TraeWork / TraeCode 需先关再开），
   * 真正的切换在确认框的 `onConfirm` 里调 `doSwitchTo`。
   *
   * ★ 抽成具名函数：卡片与抽屉（「切换到此账号」）必须是**同一条**流程 ——
   * 两处各写一遍内联箭头，迟早出现「卡片会确认、抽屉直接切」的分叉。
   *
   * ⚠️ `label` 必须由调用方传入（取自后端 `TraeProgramStatus.label`），**不要**用
   * `traeVariantLabel(programVariant)` 反推：合并版 `trae` 与遗留 `trae_code`
   * **共用变体** `trae_cn`，反推会把合并版说成 `TraeCode`（issue #20 报障）。
   */
  function requestSwitchTo(account: TraeAccount, programVariant: TraeVariantId, label: string) {
    setSwitchConfirm({
      userId: account.userId,
      variant: programVariant,
      label,
    });
  }

  /**
   * ★ 以下四个「单账号动作」抽成具名函数：卡片菜单与抽屉「操作」区必须是**同一段代码**。
   *
   * ⚠️ 它们原先全是卡片上的内联箭头。抽屉要复用就只能再抄一遍 ——
   * 而「同一动作两处实现」正是本仓反复出现的分叉来源（确认流程、保存登录态都栽过）。
   */
  function refreshJwt(account: TraeAccount) {
    void run(`jwt-${account.userId}`, "trae.page.accounts.actionRefreshJwt", () =>
      api.traeRefreshJwt(account.userId, variant),
    );
  }

  function clearCooldown(account: TraeAccount) {
    void run(`thaw-${account.userId}`, "trae.page.accounts.actionThaw", () =>
      api.traeClearCooldown(account.userId, variant),
    );
  }

  function requestDelete(account: TraeAccount) {
    setPendingDelete(account);
  }

  /** 「保存登录态」：目标是「该账号此刻登录着的那个程序位」（可能多条线同时登录）。 */
  function saveLogin(account: TraeAccount) {
    const targets = programsFor(account).filter((program) => program.current);
    if (targets.length === 0) {
      toast.error(t("trae.page.accounts.saveLoginNoProgram"), {
        description: account.name || account.userId,
      });
      return;
    }
    void run(`save-${account.userId}`, "trae.page.accounts.actionSaveLogin", async () => {
      let fileCount = 0;
      for (const program of targets) {
        const result = await api.traeSaveLogin(account.userId, program.variant);
        fileCount += result.fileCount;
      }
      return fileCount;
    });
  }

  /** 拉一次本机签到日志（抽屉的「近 7 天」用它；失败 ⇒ 该区块不渲染）。 */
  function loadDetailCheckinLogs() {
    void api
      .getTraeLogs({ kind: "checkin", variant })
      .then((res) => setDetailCheckinLogs(res.entries))
      .catch(() => setDetailCheckinLogs(null));
  }

  /** 点「查看详情」：打开抽屉。 */
  function openDetail(account: TraeAccount) {
    setDetailAutoEditRemark(false);
    setDetailAccount(account);
    loadDetailCheckinLogs();
  }

  /** 点「更多 → 编辑备注」：打开抽屉并**直接进入编辑态**。 */
  function openRemark(account: TraeAccount) {
    setDetailAutoEditRemark(true);
    setDetailAccount(account);
    loadDetailCheckinLogs();
  }

  /**
   * 批量签到：走 `scope: "selected"`（与单账号同一后端路径），跑完重取快照并展开结果卡。
   *
   * 判定口径与 `checkinAll` 一致：全失败 ⇒ `error`；一轮没轮到任何账号 ⇒ 明说
   * 「没有需要签到的账号」；其余按后端报告拼「成功 / 已签 / 失败」三段。
   *
   * ★ B4：部分失败时**把失败项留在 toast 的 action 里** ⇒ 可「只重试失败项」。
   *   原实现只汇总 `result.failed` 计数、**不保留失败 id**，Trae 用户只能把成功的一起重跑。
   *   （WB 侧 `AccountsPage::batchCheckin` 早已对齐，`runCheckinFor` 返回 `failedIds`。）
   * @param userIds 省略 = 对当前勾选项跑；传入 = 只对这批（重试路径）。
   */
  async function batchCheckin(userIds?: string[]) {
    if (batchBusy) return;
    const targets = userIds
      ? accounts.filter((account) => userIds.includes(account.userId))
      : accounts.filter((account) => selectedKeys.has(selectionKey(account.userId)));
    if (targets.length === 0) return;
    setBatchBusy(true);
    const label = t("trae.page.accounts.actionBatchCheckin");
    try {
      const result = await api.traeCheckin({
        scope: "selected",
        userIds: targets.map((account) => account.userId),
        variant,
      });
      setReport(result);
      /* ★ 结果**留在页面上**（原型 `checkinResult`），与 WorkBuddy 同一形状同一位置。 */
      setCheckinResult({
        ok: result.totalOk + result.already,
        failedIds: (result.results ?? []).filter((row) => !row.ok && row.userId).map((row) => row.userId),
      });
      const parts: string[] = [];
      if (result.totalOk > 0) parts.push(t("trae.page.accounts.checkinBatchSuccess", { n: result.totalOk }));
      if (result.already > 0) parts.push(t("trae.page.accounts.checkinBatchAlready", { n: result.already }));
      if (result.failed > 0) parts.push(t("trae.page.accounts.checkinBatchFailed", { n: result.failed }));
      const description = parts.join(t("shared.punct.comma"));
      /* ★ 从逐条结果里挑出真正失败的 id（`ok === false`）；`results` 可能缺失 ⇒ 兜底不重试。
         只在「本轮确实跑过 (`total > 0`) 且有失败项」时给出口，否则重试无意义。 */
      const failedIds = (result.results ?? [])
        .filter((row) => !row.ok && row.userId)
        .map((row) => row.userId);
      const retryAction =
        result.total > 0 && failedIds.length > 0
          ? {
              label: t("trae.page.accounts.checkinRetryFailed"),
              onClick: () => void batchCheckin(failedIds),
            }
          : undefined;
      if (result.total === 0) {
        toast.success(t("trae.page.accounts.noNeedCheckin"));
      } else if (result.failed === result.total) {
        toast.error(t("trae.page.accounts.actionFailed", { label }), { description, action: retryAction });
      } else {
        toast.success(t("trae.page.accounts.actionDone", { label }), { description, action: retryAction });
      }
      await loadAll();
    } catch (e) {
      toast.error(t("trae.page.accounts.actionFailed", { label }), { description: api.asError(e) });
    } finally {
      setBatchBusy(false);
    }
  }

  /** 批量刷新积分：逐账号走**单账号**接口（无批量端点），跑完重取快照。 */
  async function batchRefresh() {
    if (batchBusy) return;
    const targets = accounts.filter((account) => selectedKeys.has(selectionKey(account.userId)));
    if (targets.length === 0) return;
    setBatchBusy(true);
    const label = t("trae.page.accounts.actionRefreshCredits");
    try {
      for (const account of targets) {
        await api.traeRefreshCredits(account.userId, variant);
      }
      await loadAll();
      toast.success(t("trae.page.accounts.actionDone", { label }));
    } catch (e) {
      toast.error(t("trae.page.accounts.actionFailed", { label }), { description: api.asError(e) });
    } finally {
      setBatchBusy(false);
    }
  }

  /** Trae 账号的撤销 toast 副标题（过 `displayText` 白屏防线；全空则不显示副标题）。 */
  function traeUndoLabel(account: TraeAccount): string | undefined {
    return displayText(account.name) || displayText(account.userId) || undefined;
  }

  /**
   * 删除成功后弹「已删除 · 撤销」（P6 T04 §1.4）—— Trae 侧。
   *
   * ★ 撤销走 `api.accountRestore`（**与回收站同一命令**）。Trae 账号的稳定键是 `userId`
   * （不是 `id` / `uid`）。恢复后 `loadAll()` 重取快照，否则「撤销了却看不到账号回来」。
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
        await loadAll();
        if (firstError) throw firstError;
      },
    });
  }

  /**
   * 单账号删除（软删除）→ 弹「已删除 · 撤销」。
   *
   * **不复用 `run`**：`run` 会额外弹一条通用「{label}完成」成功 toast，与撤销 toast 语义
   * 重复（用户会看到两条）。这里自己管忙标记 / 错误 / 重取，语义与批量删除一致。
   */
  async function performDelete(target: TraeAccount) {
    setBusy(`delete-${target.userId}`);
    try {
      await api.traeDeleteAccount(target.userId, false, variant);
      offerUndo([target.userId], traeUndoLabel(target));
      await loadAll();
    } catch (e) {
      toast.error(t("trae.page.accounts.actionFailed", { label: t("trae.page.accounts.actionDelete") }), {
        description: api.asError(e),
      });
    } finally {
      setBusy(null);
    }
  }

  /** 批量删除（先确认）：逐个删除，任一个失败不阻断其余；删完清空选择。 */
  async function batchDelete() {
    if (batchBusy) return;
    const targets = accounts.filter((account) => selectedKeys.has(selectionKey(account.userId)));
    setBatchDeleteOpen(false);
    if (targets.length === 0) return;
    setBatchBusy(true);
    const label = t("trae.page.accounts.actionDelete");
    try {
      const deleted: TraeAccount[] = [];
      let failed = 0;
      for (const account of targets) {
        try {
          await api.traeDeleteAccount(account.userId, false, variant);
          deleted.push(account);
        } catch {
          failed += 1;
        }
      }
      if (failed > 0) {
        toast.error(t("trae.page.accounts.actionFailed", { label }), { description: `${failed}` });
      }
      clearSelection();
      await loadAll();
      if (deleted.length > 0) {
        offerUndo(
          deleted.map((account) => account.userId),
          deleted.length === 1 ? traeUndoLabel(deleted[0]) : undefined,
        );
      }
    } finally {
      setBatchBusy(false);
    }
  }

  const accounts = overview?.accounts ?? [];
  const groups = overview?.groups ?? [];

  /**
   * 该账号是不是**当前登录**（卡片头部高亮 + 幽灵 logo + 「当前登录」徽章 + 左侧竖条）。
   *
   * ## ★ 为什么不是 `logins[variant]`（2026-10-10 修的真实缺陷）
   *
   * `logins` 的键是**程序位标识**（`trae_work` / `trae_cn` / `global` / `global_trae_code`），
   * 而 `variant` 是**区域**（`cn` / `global`）。原实现写 `logins[variant]` ⇒
   * 只有「区域名恰好等于某个程序位标识」时才能查到 —— 国际版有个程序位就叫 `global`，
   * 所以那边**碰巧**正常；**国内版两个程序位叫 `trae_work` / `trae_cn`，永远查不到**
   * ⇒ 国内版作用域下**没有任何一张卡**会亮「当前登录」。
   *
   * 实测（`build:demo`，同一份数据）：`/s/trae-global/accounts` 主号卡有徽章 + 竖条；
   * `/s/trae-cn/accounts` 三张卡全无。
   *
   * 现改为**逐程序位判定**（`programsFor(account).some(p => p.current)`）——
   * 与 WorkBuddy 卡片 `isCurrentAccount`（「绑定任一程序位即当前」）**同一口径**，
   * 也与抽屉的 `detailIsCurrent` 同一实现。`logins[program.variant]` 的键是对得上的。
   */
  /* 用函数声明（与 `programsFor` 同形）而不是 `useCallback`：
     它只在渲染期被调用，包成 hook 反而要处理 `programsFor` 的依赖身份（每渲染都变）。 */
  function isCurrentAccount(account: TraeAccount): boolean {
    return programsFor(account).some((program) => program.current);
  }
  const switchBusy = busy?.startsWith("switch-") ?? false;

  /**
   * 该账号在**每条 Trae 程序**上的状态 —— 卡片上那排切换按钮的数据源。
   *
   * 与 WorkBuddy 卡片的 `workbuddyActive` / `codebuddyCnIdeActive` / `codebuddyCliActive`
   * 是同一层语义：先在本页把「有哪些程序、各自装没装、这个账号是不是它当前的账号」
   * 算清楚，卡片只负责画。**判断不下沉到卡片里** —— 否则同一账号会被各卡片各算一遍，
   * 迟早与状态条、详情弹窗的口径出现分歧。
   */
  function programsFor(account: TraeAccount): TraeProgram[] {
    // ★ 2026-10-10（**用户要求，决策反转**）：**只取当前区域**的程序位。
    //
    //   原设计（P4 §1.4 / §5）是「跨区域常显 + 显示层消歧」：任意区域页面上都列出 4 个程序位，
    //   跨区域那两枚可见但不可直接切（点了只是**引导切区域**）。实际效果是国内版页面上
    //   一直挂着「TraeWork AI 国际版 / Trae AI 国际版」两枚按钮 —— 用户明确要求
    //   **国内版不显示国际版的**。
    //
    //   改成本区域之后反而更自洽：账号库本就按区域分家（跨库切换会被后端守卫拒绝），
    //   所以本区域页面只该谈本区域的客户端；要管另一条产品线，切作用域即可（顶栏 `ScopeSwitch`）。
    //
    //   ⚠️ **仍然禁止按 `program` 去重**：`TraeProgramStatus.program` 不含区域，
    //      按它去重会把 `(cn, trae_work)` 与 `(global, trae_work)` 合并、丢掉「该写哪个客户端」。
    //      `variant`（区域限定）才是切换目标、天然唯一。
    return variantStatuses
      .filter((status) => status.variant === variant)
      .flatMap((status) =>
      status.programs.map((program) => ({
        // 来源区域（= 当前区域）。保留字段：抽屉徽标的 key 用它，且它标明「这个程序位属于哪条线」。
        region: status.variant,
        // 切换目标（区域限定）—— 真正喂给 `api.traeSwitchAccount` 的值。
        variant: program.variant,
        // 程序位标识（不含区域）：**只用于卡片按钮的可见短名**（`Work` / `Code`）。
        // ⚠️ 绝不参与去重 —— 它不含区域，按它去重会丢掉「该写哪个客户端」。
        program: program.program,
        label: program.label,
        installed: program.installed,
        // ★ 「装了」不等于「能用」：客户端从未启动过时它的 userData 目录压根不存在
        //   （本机：`Trae CN.exe` 装着、`%APPDATA%\Trae CN` 没有）⇒ 快照存不出也恢复不进，
        //   这个程序位上的切换**永远不可能成功**。取**写侧**目录的存在性
        //   （`writeDataDirExists`，与后端保存 / 恢复用的是同一个 `snapshot_data_dir_for`），
        //   而不是读侧的 `dataDirExists` —— 后者在「一个变体多个候选目录」时会指向另一个目录。
        hasDataDir: program.writeDataDirExists,
        // 登录态是**客户端级**的：比的是 `userId`（身份）而不是 `name`（展示名）
        // —— 后者改名即失配。`logins` 读不到时是 `null`，不能让 `null` 与空 userId 相互匹配。
        current:
          Boolean(account.userId) && logins[program.variant]?.userId === account.userId,
      })),
      );
  }
  /**
   * **当前区域**的探测结果（含该区域的 `dataDir` / `dataDirExists`）。
   *
   * 与 `env`（`get_trae_env`）的区别是本质的：`env` 走后端 `detect_data_dir()`，
   * 那是**横跨全部产品线**的全局探测，只适合「环境自检」。拿它在区域页里显示目录，
   * 会出现「国际版页签写着 `TRAE SOLO CN`」，而且本机一个候选都不存在时它会回落到
   * 候选表首项 ⇒ 把一个**不存在**的路径说成「已探测的登录态目录」。
   * 按区域取的是后端 `platform::select_data_dir_for(variant)`，不存在就是 `null`。
   */
  const regionStatus = variantStatuses.find((item) => item.variant === variant);

  /**
   * 「客户端环境」那一行要用的**本区域**客户端状态。
   *
   * ## ★ 为什么不能再用 `env`（`get_trae_env`）—— 用户报障现场
   *
   * `env` 是后端 `detect_data_dir()` / `detect_install()` 的**跨变体**全局探测
   * （按活跃度、首个命中挑一条线），只适合「环境自检」。放在区域页上它会**报错产品线**：
   * 本机实测国内版页签显示的是**国际版**客户端 —— `v1.107.1` +
   * `C:\...\AppData\Roaming\TRAE SOLO`，而该目录属 `packageType = SOLO_I18N`
   * （国际版）；真正装着登录态的是 `TRAE SOLO CN`（`SOLO_CN`）。
   *
   * ## 取「该区域的主程序」而不是区域级汇总
   *
   * 与状态条的登录态同源（`programs[0]`）。区域级汇总里的 `dataDir` 是**最近活跃**目录，
   * 而本行要核对的是「登录态在哪」⇒ 用**写侧**目录 `writeDataDir`
   * （与后端 `overview_for().dataDir` 同源）。本机两者不同值：
   * `writeDataDir` = `TRAE SOLO CN`（有登录态）、`dataDir` = `TRAE SOLO`（国际版、更活跃）。
   */
  function groupNameOf(account: TraeAccount): string | null {
    if (!account.groupId) return null;
    return groups.find((group) => group.id === account.groupId)?.name ?? t("trae.page.accounts.fallbackGroup");
  }

  /** 账号展示名（表格 / 无障碍标签共用；过 `displayText` 归一，防脏值白屏）。 */
  const traeAccountName = (account: TraeAccount): string =>
    displayText(account.name) || `UID · ${displayText(account.userId) ?? "—"}`;

  /** Trae 账号可用积分（`remainingCredits` 优先，回落 `credits`）。 */
  const traeCredits = (account: TraeAccount): number => account.remainingCredits ?? account.credits ?? 0;

  /**
   * 列表视图配置 —— 平台差异**全部以数据表达**（`P4-TOOLBAR-ARCH §3.1`）。
   *
   * ⚠️ **与设计稿的一处不一致（已报告 lead）**：设计 §1.5 / 决策 #3 称「Trae 有 `lastUsedAt`
   * （`trae-types.ts:796`）⇒ 排序 4 项」，但该行是 **`TraeApiKeyRecord.lastUsedAt`**（API Key
   * 记录），**不是 `TraeAccount` 的字段**。`TraeAccount` 只有 `addedAt` / `updatedAt`，没有
   * 「最近使用」口径。按决策 #3 自己的规则（**无数据不渲染**），Trae 的排序键与 WB 一致为 3 项。
   */
  const listConfig = useMemo<AccountListConfig<TraeAccount>>(() => {
    const priorityComparator: AccountComparator<TraeAccount> = (left, right) => {
      // 「积分优先级」：**快过期先用**（到期近的靠前），同到期再按剩余积分多的靠前。
      const leftExpiry = left.creditsExpireAt ?? Number.POSITIVE_INFINITY;
      const rightExpiry = right.creditsExpireAt ?? Number.POSITIVE_INFINITY;
      if (leftExpiry !== rightExpiry) return leftExpiry - rightExpiry;
      return traeCredits(right) - traeCredits(left);
    };
    const nameComparator: AccountComparator<TraeAccount> = (left, right) =>
      traeAccountName(left).localeCompare(traeAccountName(right), undefined, {
        numeric: true,
        sensitivity: "base",
      });
    const expiryComparator: AccountComparator<TraeAccount> = (left, right) =>
      (left.creditsExpireAt ?? Number.POSITIVE_INFINITY) - (right.creditsExpireAt ?? Number.POSITIVE_INFINITY);
    return {
      searchText: (account) =>
        accountSearchText({
          name: displayText(account.name),
          uid: displayText(account.userId),
          remark: displayText(account.remark),
          group: groupNameOf(account),
        }),
      comparators: { priority: priorityComparator, name: nameComparator, expiry: expiryComparator },
      bucketKey: (account, key) => {
        if (key === "program") {
          const labels = programsFor(account)
            .filter((program) => program.current)
            .map((program) => displayText(program.label) ?? traeVariantLabel(program.variant));
          return labels.length > 0 ? labels.join(" + ") : t("trae.page.accounts.toolbar.programBucket.unbound");
        }
        if (key === "status") {
          // 状态集**各平台自备**（决策 #8）：Trae 有「冷却」等独有状态，不强行与 WB 统一。
          if (account.cooldownType) return t("trae.page.accounts.toolbar.statusBucket.cooling");
          if (account.jwtStatus === "expired") return t("trae.page.accounts.toolbar.statusBucket.expired");
          if (!account.checkedToday) return t("trae.page.accounts.toolbar.statusBucket.notCheckedIn");
          return t("trae.page.accounts.toolbar.statusBucket.normal");
        }
        if (key === "group") return groupNameOf(account) ?? t("trae.page.accounts.toolbar.groupBucket.default");
        return null;
      },
      availableSorts: ["priority", "name", "expiry"],
      availableGroups: ["none", "program", "status", "group"],
    };
    // `programsFor` / `groupNameOf` 读 `variantStatuses` / `logins` / `groups`，故一并入依赖。
  }, [t, groups, variantStatuses, logins]);

  /** 筛选（搜索）→ 排序 → 分桶；视图参数全部留在页内。 */
  const filtered = filterAccounts(accounts, listConfig, query);
  const sorted = sortAccounts(filtered, sort, listConfig);
  const buckets = groupAccounts(sorted, group, listConfig);

  /** 排序 / 分组下拉选项 —— 由 `availableSorts` / `availableGroups`（数据）驱动，无假选项。 */
  const sortLabels: Partial<Record<AccountSortKey, string>> = {
    priority: t("trae.page.accounts.toolbar.sort.priority"),
    name: t("trae.page.accounts.toolbar.sort.name"),
    expiry: t("trae.page.accounts.toolbar.sort.expiry"),
  };
  const groupLabels: Partial<Record<AccountGroupKey, string>> = {
    none: t("trae.page.accounts.toolbar.group.none"),
    program: t("trae.page.accounts.toolbar.group.program"),
    status: t("trae.page.accounts.toolbar.group.status"),
    group: t("trae.page.accounts.toolbar.group.group"),
  };
  const sortOptions: ToolbarOption[] = listConfig.availableSorts.flatMap((key) => {
    const label = sortLabels[key];
    return label ? [{ value: key, label }] : [];
  });
  const groupOptions: ToolbarOption[] = listConfig.availableGroups.flatMap((key) => {
    const label = groupLabels[key];
    return label ? [{ value: key, label }] : [];
  });

  /** 已勾选的账号（按当前列表顺序）—— 批量条的「导出」用它，与 WorkBuddy 同一口径。 */
  const selectedAccounts = useMemo(
    () => accounts.filter((account) => selectedKeys.has(selectionKey(account.userId))),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [accounts, selectedKeys, scope],
  );

  /** 全选：只作用于**当前可见**（筛选后）的行；与表头全选读写同一份 `selectedKeys`。 */
  const allVisibleSelected =
    sorted.length > 0 && sorted.every((account) => selectedKeys.has(selectionKey(account.userId)));
  const someVisibleSelected =
    !allVisibleSelected && sorted.some((account) => selectedKeys.has(selectionKey(account.userId)));
  function toggleVisibleSelection() {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      for (const account of sorted) {
        if (allVisibleSelected) next.delete(selectionKey(account.userId));
        else next.add(selectionKey(account.userId));
      }
      return next;
    });
  }
  function bucketAllSelected(items: TraeAccount[]): boolean {
    return items.length > 0 && items.every((account) => selectedKeys.has(selectionKey(account.userId)));
  }
  function bucketSomeSelected(items: TraeAccount[]): boolean {
    return !bucketAllSelected(items) && items.some((account) => selectedKeys.has(selectionKey(account.userId)));
  }
  function toggleBucketSelection(items: TraeAccount[], checked: boolean) {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      for (const account of items) {
        if (checked) next.add(selectionKey(account.userId));
        else next.delete(selectionKey(account.userId));
      }
      return next;
    });
  }

  /** 「清除筛选」出口：复位搜索 + 分桶（`§1.9` 两种空态绝不混用）。 */
  function clearListFilters() {
    if (!hasActiveFilter({ query, group })) return;
    setQuery("");
    setGroup("none");
  }

  /** Trae 状态徽标（表格用；与卡片同一套语义，只是排布不同）。 */
  function renderStatusBadges(account: TraeAccount) {
    const jwt = jwtBadge(account.jwtStatus, account.jwtExpHours, t);
    return (
      <div className="flex flex-wrap items-center gap-1">
        <Badge
          variant={account.checkedToday ? "success" : "secondary"}
          className={cn("rounded-md px-1.5 py-0 text-[11px] font-medium", !account.checkedToday && "text-muted-foreground")}
        >
          {account.checkedToday ? t("trae.comp.card.chip.checked") : t("trae.comp.card.chip.unchecked")}
        </Badge>
        {account.cooldownType && (
          <Badge variant="warning" className="rounded-md px-1.5 py-0 text-[11px] font-medium">
            {t("trae.comp.card.chip.cooling")}
          </Badge>
        )}
        <Badge variant={jwt.variant} className={cn("rounded-md px-1.5 py-0 text-[11px] font-medium", jwt.className)}>
          {jwt.text}
        </Badge>
      </div>
    );
  }

  /** 表格「到期」列：积分到期（无到期时间 → 「长期有效」）。 */
  function formatAccountExpiry(account: TraeAccount): string {
    return account.creditsExpireAt
      ? t("trae.comp.card.expiry.short", { date: shortTime(account.creditsExpireAt) })
      : t("trae.comp.card.expiry.permanent");
  }

  /**
   * 表格列定义（`§1.8`）。**无「今日 Token」**（Trae 无该口径 —— 与 `traeStats` 的既有说明一致）。
   * 单元格全部返回**已归一**节点，**绝不返回对象**（对象当 React 子节点会白屏）。
   * ⚠️「程序位」列是**只读徽标**（无 `onClick`、无按钮）：表格行只有 2 个可点区。
   */
  const columns: AccountColumn<TraeAccount>[] = [
    {
      key: "account",
      header: t("trae.page.accounts.table.account"),
      render: (account) => (
        /* ★ 与 WorkBuddy 表格**同一结构**（原型 `tableView` `:2551`）：头像 + 名称行（含「当前」徽章）
           + 身份行。此前 Trae 侧没有头像 ⇒ 两个分区的表格首列不同形。 */
        <div className="flex min-w-0 items-center gap-2.5">
          <AccountsTableAvatar name={traeAccountName(account)} />
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <span className="truncate font-medium" title={traeAccountName(account)}>
                {traeAccountName(account)}
              </span>
              {isCurrentAccount(account) && (
                <Badge variant="secondary" className="rounded-md px-1.5 py-0 text-[11px] font-medium">
                  {t("trae.page.accounts.table.current")}
                </Badge>
              )}
            </div>
            <div className="truncate font-mono text-xs text-muted-foreground">UID · {displayText(account.userId) ?? "—"}</div>
          </div>
        </div>
      ),
    },
    {
      key: "status",
      header: t("trae.page.accounts.table.status"),
      render: (account) => renderStatusBadges(account),
    },
    {
      key: "credits",
      header: t("trae.page.accounts.table.credits"),
      align: "right",
      render: (account) => <span className="tabular-nums">{formatCredits(traeCredits(account))}</span>,
    },
    {
      key: "expiry",
      header: t("trae.page.accounts.table.expiry"),
      render: (account) => <span className="tabular-nums text-muted-foreground">{formatAccountExpiry(account)}</span>,
    },
    {
      key: "programs",
      header: t("trae.page.accounts.table.programs"),
      render: (account) => {
        const active = programsFor(account).filter((program) => program.current);
        return active.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {active.map((program) => (
              <Badge
                key={`${program.region}:${program.variant}`}
                variant="secondary"
                className="rounded-md px-1.5 py-0 text-[11px] font-medium"
              >
                {displayText(program.label) ?? traeVariantLabel(program.variant)}
              </Badge>
            ))}
          </div>
        ) : (
          <span className="text-muted-foreground">—</span>
        );
      },
    },
  ];

  // ---- 详情抽屉的数据装配（字段全部过 `displayText` 出口，见 display-text.ts）----
  const detail = detailAccount;
  const detailName = detail
    ? displayText(detail.name) || `UID · ${displayText(detail.userId) ?? "—"}`
    : "";
  const detailSections: DetailSection[] = detail
    ? [
        {
          key: "account",
          title: t("trae.comp.card.detail.section.account"),
          rows: [
            {
              label: t("trae.comp.card.detail.accountId"),
              value: displayText(detail.userId) ?? "—",
              mono: true,
            },
            {
              label: t("trae.comp.card.detail.cooldown"),
              value: detail.cooldownType
                ? `${detail.cooldownType}${detail.cooldownUntil ? ` · ${shortTime(detail.cooldownUntil)}` : ""}`
                : t("trae.comp.card.detail.none"),
            },
          ],
        },
        {
          key: "credits",
          title: t("trae.comp.card.detail.section.credits"),
          rows: [
            {
              label: t("trae.comp.card.credits.remaining"),
              value:
                detail.remainingCredits !== null
                  ? formatCredits(detail.remainingCredits)
                  : t("trae.comp.card.detail.notQueried"),
            },
            {
              label: t("trae.comp.card.detail.creditsExpiry"),
              value: detail.creditsExpireAt
                ? t("trae.comp.card.expiry.short", { date: shortTime(detail.creditsExpireAt) })
                : t("trae.comp.card.expiry.permanent"),
            },
          ],
        },
        {
          key: "credential",
          title: t("trae.comp.card.detail.section.credential"),
          rows: [
            {
              label: t("trae.comp.card.detail.jwtStatus"),
              value: jwtBadge(detail.jwtStatus, detail.jwtExpHours, t).text,
            },
            {
              label: t("trae.comp.card.detail.jwtAutoRefresh"),
              /* 三态：没 refresh_token（换不了）／总开关关着（只能手动）／开着（分「本次将续期」
                 与「未到期」）。★ 判据与后台续期任务同源（`account::needs_jwt_refresh`
                 → `jwtAutoRefresh`），所以这里说「本次将续期」的账号，正是下一轮会被换掉的那批。 */
              value: !detail.hasRefreshToken
                ? t("trae.comp.card.detail.noRefreshToken")
                : schedule?.trae_jwt_refresh_enabled === false
                  ? t("trae.comp.card.detail.jwtAutoDisabled")
                  : detail.jwtAutoRefresh
                    ? t("trae.comp.card.detail.jwtAutoOn")
                    : t("trae.comp.card.detail.jwtAutoOff"),
            },
            {
              label: t("trae.comp.card.detail.deviceId"),
              value: displayText(detail.deviceIdMasked) ?? t("trae.comp.card.detail.none"),
              mono: true,
            },
          ],
        },
      ]
    : [];
  const detailPacks: DetailPack[] = detail
    ? (detail.creditPackages ?? [])
        .filter((item) => item.remaining > 0)
        .map((item, index) => ({ item, index }))
        .sort((left, right) => {
          const leftExpiry = left.item.expireAt ?? Number.POSITIVE_INFINITY;
          const rightExpiry = right.item.expireAt ?? Number.POSITIVE_INFINITY;
          return leftExpiry === rightExpiry ? left.index - right.index : leftExpiry - rightExpiry;
        })
        .map(({ item }) => ({
          key: `${item.packageCode ?? item.packageName ?? "pack"}-${item.expireAt ?? "none"}`,
          // 同 WorkBuddy 账号页：`packageName` / `packageCode` 可能被后端吐成对象，
          // 直接当 React 子节点会白屏（issue #2）。过 `displayText` 出口归一。
          name: displayText(item.packageName) || displayText(item.packageCode) || t("trae.comp.card.package.fallback"),
          remainingText: formatCredits(item.remaining),
          totalText: formatCredits(item.total),
          usedText: t("trae.comp.card.package.used", { n: formatCredits(item.used) }),
          // 到期文案不再区分「几天内」——是否临期只体现在**颜色**上（见 `DetailPack.expiringSoon`）。
          expiryText: item.expired
            ? t("trae.comp.card.package.expired")
            : item.expireAt
              ? t("trae.comp.card.expiry.short", { date: shortTime(item.expireAt) })
              : t("trae.comp.card.expiry.permanent"),
          expired: item.expired,
          // ★「3 天内到期」用黄色警示。后端 `expiringSoon` 是 7 天口径且卡片徽章还在用，不动它；
          //   抽屉的 3 天口径在这里就地判定（Trae 侧 `expireAt` 是秒）。
          expiringSoon:
            !item.expired && item.expireAt != null && item.expireAt > 0 && item.expireAt * 1000 - Date.now() <= 3 * 86_400_000,
          ratio: item.total > 0 ? Math.min(100, Math.max(0, (item.remaining / item.total) * 100)) : 0,
        }))
    : [];
  // 只读程序位徽标（**非**切换控件）：复用卡片的 `programsFor`，标签带区域标识消歧。
  const detailPrograms: DetailProgramBadge[] = detail
    ? programsFor(detail).map((program) => ({
        key: `${program.region}:${program.variant}`,
        // `program.label` 来自后端 `TraeProgramStatus.label`，未经归一 —— 对象会当 React
        // 子节点卸载整棵树（issue #2）。过 `displayText` 出口，取不到回落占位。
        label: `${displayText(program.label) ?? "—"} · ${
          program.region === "global" ? t("shared.region.version.global") : t("shared.region.version.cn")
        }`,
        current: program.current,
        installed: program.installed,
        hasDataDir: program.hasDataDir,
      }))
    : [];
  const detailRemark = detail ? displayText(detail.remark)?.trim() || "" : "";

  /* ────────────── 抽屉装配：与 WorkBuddy **逐项同构** ──────────────
     ⚠️ 改造前 Trae 只注入了 `packs/sections/programs/remark` 四样，
     抽屉因此缺了「当前登录徽章 / 切换到此账号 / 操作区 / 签到历史」四块 ——
     同一个共享抽屉在两个分区里内容完全不同。下面按 WB 的口径补齐，
     差异**只在数据**（Trae 的切换目标是 Trae 产品线、凭据是 JWT）。 */

  /**
   * ★ 「当前登录」= 该账号是**任一** Trae 产品线的当前账号（与卡片 / WB 抽屉同口径，
   * 原型 `drawerAccount` :3467）。
   */
  const detailIsCurrent = detail ? isCurrentAccount(detail) : false;

  /**
   * ★ 「切换到此账号」程序位（可点）。回调复用页面既有的 `requestSwitchTo`（会走统一重启确认）。
   *
   * ⚠️ 2026-10-10：`programsFor` 现在只返回**本区域**的程序位 ⇒ 原先那条「跨区域不给 onSwitch、
   * 标签加区域后缀消歧」的分支已删除（永远走不到，留着就是死代码）。
   * 不可切换只剩一种原因：客户端没装 / 没有可读写的数据目录（`hasDataDir`）。
   */
  const detailSwitchTargets: DetailSwitchTarget[] = detail
    ? programsFor(detail).map((program) => {
        // 与卡片**同一取值**：程序位自己的展示名。抽成局部变量是因为它既要上界面，
        // 又要原样传给 `requestSwitchTo` —— 而**不能**在那里用 variant 反推
        // （合并版 `trae` 与遗留 `trae_code` 共用变体 `trae_cn`，反推会说成 `TraeCode`）。
        const label = displayText(program.label) || program.variant;
        return {
          key: `${program.region}:${program.variant}`,
          label,
          current: program.current,
          available: program.installed,
          busy: busy?.startsWith("switch-") ?? false,
          loading: busy === `switch-${detail.userId}@${program.variant}`,
          onSwitch:
            program.installed && program.hasDataDir
              ? () => requestSwitchTo(detail, program.variant, label)
              : undefined,
        };
      })
    : [];

  /**
   * 该账号凭据是否不可用（JWT 已过期 / 状态未知）—— 与卡片菜单的「不给签到入口」同口径。
   * ⚠️ `TraeJwtStatus` 只有 `ok | warn | expired | unknown`（**没有** `unparsable`，那是文案层的说法）。
   */
  const detailCredentialUnavailable = detail
    ? detail.jwtStatus === "expired" || detail.jwtStatus === "unknown"
    : false;

  /**
   * ★ 抽屉「操作」区：回调全部复用页面**既有**处理函数（刷新 JWT / 签到 / 导出 / 删除）。
   * 「手动签到」按与卡片菜单**同一门控**：今日未签到且凭据可用时才给入口。
   */
  const detailActions: DetailActions | null = detail
    ? {
        onRefresh: () => refreshJwt(detail),
        onCheckin: () => void checkinOne(detail),
        showCheckin: !detail.checkedToday && !detailCredentialUnavailable,
        onExport: () => {
          setExportTargets([detail]);
          setExportOpen(true);
        },
        onDelete: () => requestDelete(detail),
        credentialUnavailable: detailCredentialUnavailable,
      }
    : null;

  /**
   * ★ 抽屉「签到积分」（原型 B5）：`todayCheckedIn` 必给；「今日获得」来自**最近一轮**签到摘要里
   * 该账号的 `delta`（上游实测字段）。
   *
   * ⚠️ 摘要不是今天的、或该账号不在摘要里 ⇒ `todayCredit: null` ⇒ 抽屉**不渲染**那一行
   * （§3.4 规则 1：无数据不渲染）。**绝不**用固定奖励凑一个「+N」。
   */
  const detailCheckin = detail
    ? {
        todayCheckedIn: detail.checkedToday,
        todayCredit:
          checkin?.summaryIsToday
            ? checkin.summary.results.find((row) => row.userId === detail.userId && row.ok)?.delta ?? null
            : null,
      }
    : null;

  /**
   * ★ 抽屉「签到历史（近 7 天）」：日期 + 结果来自**本机签到日志**
   * （`get_trae_logs({ kind: "checkin" })`，`TraeLogEntry.date`）。
   *
   * 「获得积分」按**账号 + 日期**取 `checkin.summary.results` 里那一条的 `delta`；
   * 该摘要只覆盖**最近一轮**，取不到时为 `null` ⇒ 渲染「—」（**绝不**臆造固定奖励）。
   */
  const detailCheckinHistory: DetailCheckinHistoryRow[] | null =
    detail && detailCheckinLogs
      ? lastSevenDays().map((day) => {
          const entry = detailCheckinLogs
            .filter((log) => log.kind === "checkin" && log.date === day)
            .find((log) => log.message.includes(detail.userId));
          const outcome = checkin?.summaryIsToday
            ? checkin.summary.results.find((row) => row.userId === detail.userId && row.ok)
            : undefined;
          const amount = outcome ? outcome.delta : null;
          if (!entry) {
            return { date: day, resultLabel: t("shared.drawer.checkin.result.none"), tone: "none" as const, amount };
          }
          /* 日志行是**纯文本**，不做自然语言解析：只按「是否含失败关键字」二分。
             宁可把失败显示成「成功」以外的一档，也不猜更细的状态。 */
          const failed = /失败|error|fail/i.test(entry.message);
          return failed
            ? { date: day, resultLabel: t("shared.drawer.checkin.result.fail"), tone: "fail" as const, amount }
            : { date: day, resultLabel: t("shared.drawer.checkin.result.success"), tone: "ok" as const, amount };
        })
      : null;

  /* 页头作用域（原型 `viewAccounts`）复用上方的 `scope`；「需处理」= JWT 已过期，
     与 WorkBuddy 侧的 `needsRelogin` 同一层语义。 */
  const expiredCount = accounts.filter((account) => account.jwtStatus === "expired").length;

  return (
    <div className="mx-auto w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
      {/* 页头与 WorkBuddy 逐字同构：**只有标题与说明，不放任何动作按钮**。
          动作一律下沉到「添加与迁移账号」横幅与账号工具栏，
          这样两个分区的动作位置、视觉重量完全一致。
          曾经这里堆了 4 个按钮（刷新 / 刷新积分 / 全部签到 / 添加账号），
          与 WorkBuddy 的骨架冲突，也让同一功能出现两个入口。 */}
      <header className="mb-5 flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("trae.page.accounts.title")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">
            {expiredCount > 0
              ? t("pageHead.accounts.pending", {
                  scope: scopeLabel(t, scope),
                  n: accounts.length,
                  m: expiredCount,
                })
              : t("pageHead.accounts", { scope: scopeLabel(t, scope), n: accounts.length })}
          </p>
        </div>
        <div className="grow" />
        <ScopeChip scope={scope} />
      </header>

      {/* ★ 账号工作台（**两平台共用容器**）—— 与 `AccountsPage` 是**同一个组件**，
          差异只在 props：排序/分组选项、工具栏专属控件、卡片渲染、列定义、首屏空态。
          这正是原型 `curPlatform()` 的机制（同一段 `viewAccounts`，只换数据）。 */}
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
        /* ★ 主操作：两平台**同一入口** —— 都开「添加账号」抽屉（`AddAccountDrawer`）。
           原工具栏右侧还有 4 个 ghost 级入口（导入本机 / 粘贴 JWT / 导入备份 / 导出备份），
           已随「⋯」溢出菜单一起删除 ⇒ 全部搬进抽屉：粘贴 JWT 成为第 4 张添加方式卡，
           导出备份进「更多操作」，导入本机 / 导入备份本就与抽屉前两张卡同义（**不重复搬**）。 */
        addAction={{
          label: t("shared.addDrawer.title"),
          onClick: () => setAddDrawerOpen(true),
        }}
        /* ★ 一枚**常驻**控件（用户要求「回收站放出来」）——
           走结构化 prop 注入，几何与文案都留在工具栏壳里；其余入口仍在抽屉的「更多操作」。 */
        onOpenRecycleBin={() => setRecycleOpen(true)}
        loading={loading}
        /* 首屏空态：与 WorkBuddy 的 `EmptyRegionCard` 同构（可能原因 + 期望产物 + 动作）。 */
        emptyState={
          accounts.length === 0 ? (
            <EmptyTraeCard
          // ⚠️ 三个字段都必须取**当前区域**那份（见 `EmptyTraeCard` 的 prop 文档）：
          // `env`（`get_trae_env`）是横跨全部产品线的全局探测，在区域页里会张冠李戴
          // —— 「装了」会取自另一条产品线，`dataDir` 会写成另一个客户端的目录。
          installed={regionStatus?.installed ?? false}
          dataDir={regionStatus?.dataDir ?? null}
          dataDirExists={regionStatus?.dataDirExists ?? false}
          onRecheck={() => void loadAll()}
          onImport={() => void importLocal()}
          onOAuth={() => setOauthOpen(true)}
          onLaunch={() =>
            void run("launch-client", "trae.page.accounts.emptyLaunchClient", () =>
              api.launchTraeClient(variant),
            )
          }
              importing={importing}
              launching={busy === "launch-client"}
            />
          ) : null
        }
        notices={
          <>
            {error && (
              <Alert variant="destructive" className="mb-4">
                <AlertTriangle />
                <AlertTitle>{t("trae.page.accounts.loadFailed")}</AlertTitle>
                <AlertDescription>
                  <span>{error}</span>
                  {/* ★ 失败态**必须给一条出路**（本仓约定，与 WorkBuddy 同形）：
                      没有它，用户唯一能做的就是重启应用。此前 Trae 侧这条 Alert 只有文字。 */}
                  <div className="mt-2">
                    <Button size="sm" variant="outline" onClick={() => void loadAll()}>
                      <RefreshCw />
                      {t("trae.page.accounts.emptyRecheck")}
                    </Button>
                  </div>
                </AlertDescription>
              </Alert>
            )}
          </>
        }
        checkinResult={
          checkinResult
            ? {
                okCount: checkinResult.ok,
                failedNames: accounts
                  .filter((account) => checkinResult.failedIds.includes(account.userId))
                  .map((account) => displayText(account.name) || account.userId),
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
          /* ★ 导出所选账号：原型 `batchBarHtml` 的固定动作之一（`:2453`）。
             Trae 侧此前缺这一项 ⇒ 同一个批量条在两个分区动作集不同。 */
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
        listAriaLabel={t("trae.page.accounts.sectionAria", { count: accounts.length })}
        totalCount={accounts.length}
        visibleCount={sorted.length}
        /* 本页「真没有账号」由 `emptyState`（`EmptyTraeCard`）承担，这里只留「被筛没」出口。 */
        emptyNone={{
          title: t("shared.list.empty.none"),
          description: t("shared.list.empty.noneDesc"),
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
        rowKey={(account) => selectionKey(account.userId)}
        selectedIds={selectedKeys}
        onToggleSelect={toggleSelect}
        onToggleBucketAll={toggleBucketSelection}
        bucketAllSelected={bucketAllSelected}
        bucketSomeSelected={bucketSomeSelected}
        onOpenRow={openDetail}
        isRowCurrent={isCurrentAccount}
        rowSelectLabel={(account) => t("shared.table.selectRowAria", { name: traeAccountName(account) })}
        rowOpenLabel={(account) => t("shared.table.openRowAria", { name: traeAccountName(account) })}
        renderCard={(account) => (
                            <TraeAccountCard
                              key={account.userId}
                              account={{
                                ...account,
                                // 逐包明细来自 `get_trae_credits` 的 `packages[uid]`：账号视图（`list_account_views_for`）
                                // 只带聚合积分，包粒度明细在积分总览里，按 uid 合并后交给卡片渲染「近期到期」进度条。
                                creditPackages: credits?.packages?.[account.userId] ?? account.creditPackages ?? null,
                              }}
                              current={isCurrentAccount(account)}
                              programs={programsFor(account)}
                              busy={busy}
                              switchBusy={switchBusy}
                              /* ★ 与 WorkBuddy 卡片**同一取值**（见下方说明 / `AccountsPage` 同处）。 */
                              featuresDisabled={false}
                              selected={selectedKeys.has(selectionKey(account.userId))}
                              onToggleSelect={toggleSelect}
                              onOpenDetail={openDetail}
                              onEditRemark={openRemark}
                              /* 「把这个账号挂到哪条 Trae 线上」——对应 WorkBuddy 卡片上的
                                 WorkBuddy / CodeBuddy IDE / CodeBuddy CLI 三枚按钮。
                                 `variant` 取按钮自己那条线（不是当前页面那条）：用户就是要
                                 在当前页面上给另一条线挂账号，用页面的 `variant` 会挂错线。

                                 ★ 失败**不需要**这里兜底：`api.traeSwitchAccount` 在
                                 `outcome.success === false` 时抛错（见该函数的说明），
                                 `run()` 的 catch 会报「{label}失败」并带上后端给的可操作原因。
                                 曾经这里用 `after` 补一条 error toast，而 `run()` 的成功 toast
                                 是**无条件**弹的 ⇒ 用户同时收到「完成」与「未完成」，
                                 绿勾那条还在，正是「谎报成功」的来源（2026-09-29 报障）。 */
                              onSwitchTo={requestSwitchTo}
                              onCheckin={(target) => void checkinOne(target)}
                              /* 「保存登录态」＝把客户端**此刻**的登录态存到这个账号名下，
                                 因此目标只能是「这个账号当前登录着的那个程序位」。
                                 此前这里传的是页面所属的**区域**（`variant`），而区域只能推出
                                 该区域的主程序（`cn` → TraeWork）⇒ 两个后果：
                                 ① **TraeCode 的快照永远存不出来**，它的切换按钮是条死路
                                    （2026-09-29 报障里那枚按钮点不动就是这个原因）；
                                 ② 目标与客户端实际登录不符时被后端保存守卫拒绝，用户收到一句
                                    看不懂的「客户端当前登录的是另一个账号…」（本机实测复现过）。

                                 ⚠️ 这里**重新算一遍** `current` 而不是复用卡片渲染时的那份：
                                 卡片可能已经渲染了几十秒，期间 `logins` 被轮询刷新过 ⇒
                                 按钮的 disabled 与此刻的事实可能不一致。一个都没命中就如实报错，
                                 不要静默什么都不做（那正是本次要修的毛病）。 */
                              onSaveLogin={saveLogin}
                              onRefreshJwt={refreshJwt}
                              onClearCooldown={clearCooldown}
                              onDelete={requestDelete}
                            />
        )}
      />

      {/* 签到进度 / 结果：与「全部签到」同源，就地展开不做成独立页。 */}
      {(progress || report) && (
        <Card className="mt-6 gap-0 py-0">
          <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
            <span className="text-sm font-semibold">{t("trae.page.accounts.resultTitle")}</span>
            {progress?.type === "account" && (
              <span className="text-xs text-muted-foreground">
                {t("trae.page.accounts.resultProgress", { index: progress.index, total: report?.total ?? "…" })}
              </span>
            )}
          </div>
          <div className="divide-y divide-border/60">
            {(report?.results ?? []).map((item) => (
              <div key={item.userId + item.name} className="flex items-center justify-between gap-3 px-5 py-2.5 text-sm">
                <span className="min-w-0 truncate">{item.name}</span>
                <span className="flex shrink-0 items-center gap-3">
                  {item.delta > 0 && <span className="text-success">+{item.delta}</span>}
                  <span className="text-xs text-muted-foreground">{item.message || item.action}</span>
                  <Badge variant={item.ok ? "secondary" : "destructive"}>
                    {item.action === "skip_already"
                      ? t("trae.page.accounts.resultSkipped")
                      : item.ok
                        ? t("trae.page.accounts.resultSuccess")
                        : t("trae.page.accounts.resultFailed")}
                  </Badge>
                </span>
              </div>
            ))}
            {!report && progress?.type === "account" && (
              <div className="px-5 py-2.5 text-sm text-muted-foreground">
                {t("trae.page.accounts.resultLine", {
                  name: progress.name,
                  status:
                    progress.status === "success"
                      ? t("trae.page.accounts.resultSuccess")
                      : progress.status === "already"
                        ? t("trae.page.accounts.resultSkipped")
                        : t("trae.page.accounts.resultFailed"),
                })}
              </div>
            )}
            {/* 空队列要说明白「为什么一条都没有」，否则看着像功能坏了 ——
                打开「跳过今日已签到」后这是最常见的一种正常结果。 */}
            {report && report.results.length === 0 && (
              <div className="px-5 py-2.5 text-sm text-muted-foreground">
                {t("trae.page.accounts.resultEmpty")}
              </div>
            )}
          </div>
          {report && report.warnings.length > 0 && (
            <div className="border-t border-border/60 px-5 py-3 text-xs text-warning">
              {report.warnings.map((warning) => (
                <div key={warning}>{warning}</div>
              ))}
            </div>
          )}
        </Card>
      )}

      <AddAccountDrawer
        open={addDrawerOpen}
        onOpenChange={setAddDrawerOpen}
        scopeLabel={scopeLabel(t, scope)}
        installed={regionStatus?.installed ?? false}
        onPick={(way: AddAccountWay) => {
          /* 与 WorkBuddy 同构：先关抽屉，再进入对应流程。 */
          if (way === "oauth") setOauthOpen(true);
          else if (way === "backup") setImportOpen(true);
          else void importLocal();
        }}
        /* Trae 独有的一条兜底路径（OAuth 才是主路径）：手动粘贴 Cloud-IDE-JWT。 */
        extraOptions={[
          {
            key: "paste-jwt",
            icon: UserPlus,
            title: t("trae.page.accounts.pasteJwt"),
            desc: t("trae.page.accounts.pasteJwtTitle"),
            onClick: () => setAddOpen(true),
          },
        ]}
        /* ★ 原工具栏「⋯」溢出菜单的落点（2026-10-12 用户要求「合并到添加账号的右侧弹出栏内」）。
           与 WorkBuddy **同结构同顺序**（先两枚开关，再两枚动作）；回收站**已被用户点名「放出来」**
           （回工具栏，见 `onOpenRecycleBin`）⇒ 这里只剩 4 个；
           「从本机导入 / 导入备份」就是上面那两张卡 ⇒ 不重复搬。 */
        moreTools={[
          {
            key: "auto-checkin",
            icon: CalendarCheck,
            label: t("trae.page.accounts.autoCheckin"),
            htmlFor: "trae-auto-checkin",
            control: (
              <>
                <DemoAction>
                  <Switch
                    id="trae-auto-checkin"
                    checked={schedule?.trae_checkin_enabled ?? false}
                    disabled={!schedule || scheduleSaving}
                    onCheckedChange={(enabled) => void onAutoCheckinChange(enabled)}
                    aria-label={t("trae.page.accounts.autoCheckin")}
                  />
                </DemoAction>
                {scheduleSaving && (
                  <Loader2 className="size-3.5 animate-spin text-muted-foreground" aria-label={t("trae.page.accounts.autoCheckinSaving")} />
                )}
              </>
            ),
          },
          {
            key: "skip-checked",
            icon: ListChecks,
            label: t("trae.page.accounts.skipChecked"),
            htmlFor: "trae-skip-checked",
            control: (
              <>
                <DemoAction>
                  <Switch
                    id="trae-skip-checked"
                    checked={settings?.checkinSkipChecked ?? true}
                    disabled={!settings || autoCheckinSaving}
                    onCheckedChange={(enabled) => void onSkipCheckedChange(enabled)}
                    aria-label={t("trae.page.accounts.skipCheckedAria")}
                  />
                </DemoAction>
                {autoCheckinSaving && (
                  <Loader2 className="size-3.5 animate-spin text-muted-foreground" aria-label={t("trae.page.accounts.checkinSettingsSaving")} />
                )}
              </>
            ),
          },
          {
            key: "export",
            icon: FileDown,
            label: t("trae.page.accounts.exportTitle"),
            disabled: accounts.length === 0,
            onClick: () => {
              setAddDrawerOpen(false);
              setExportOpen(true);
            },
          },
          {
            key: "checkin-refresh",
            icon: RefreshCw,
            label: t("trae.page.accounts.checkinRefreshAll"),
            disabled: busy === "checkin" || busy === "refresh-credits" || accounts.length === 0,
            onClick: () => {
              setAddDrawerOpen(false);
              void checkinAll();
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

      <TraeOAuthLoginDialog
        open={oauthOpen}
        onOpenChange={setOauthOpen}
        onSuccess={(account) => {
          toast.success(t("trae.page.accounts.accountAdded"), {
            description: `${account.name || account.userId}${account.hasRefreshToken ? t("trae.page.accounts.autoRenewSuffix") : ""}`,
          });
          void loadAll();
        }}
      />

      <AddAccountDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        groups={groups}
        variant={variant}
        onAdded={() => void loadAll()}
      />

      <TraeExportAccountsDialog
        open={exportOpen}
        onOpenChange={(open) => {
          setExportOpen(open);
          /* 关闭时清掉「所选」范围，否则下次从工具栏打开会莫名只导出上次勾的那几个。 */
          if (!open) setExportTargets(null);
        }}
        accounts={exportTargets ?? accounts}
        variant={variant}
        onExported={(count) => toast.success(t("trae.page.accounts.exportedCount", { count }))}
      />

      <TraeImportAccountsDialog
        open={importOpen}
        onOpenChange={setImportOpen}
        variant={variant}
        onImported={(result) => {
          toast.success(t("trae.page.accounts.importDone"), {
            description: t("trae.page.accounts.importSummary", {
              added: result.imported,
              overwritten: result.overwritten,
              skipped: result.skipped,
            }),
          });
          void loadAll();
        }}
      />

      {/* 统一重启确认（P8 T03 / §3.15.3）：TraeWork / TraeCode 切换先关再开 ⇒ 与 WorkBuddy /
          CodeBuddy IDE 走**同一条**流程。`restart` 映射到 `api.traeSwitchAccount` 的 `launch`。 */}
      <RestartConfirmDialog
        open={switchConfirm !== null}
        onOpenChange={(open) => {
          if (!open) setSwitchConfirm(null);
        }}
        clientLabel={switchConfirm?.label ?? ""}
        isCli={false}
        onConfirm={(restart) => {
          const target = switchConfirm;
          setSwitchConfirm(null);
          if (target) void doSwitchTo(target.userId, target.variant, target.label, restart);
        }}
      />

      {/* 删除确认 */}
      <Dialog open={pendingDelete !== null} onOpenChange={(open) => !open && setPendingDelete(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("trae.page.accounts.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("trae.page.accounts.deleteBody", {
                name: pendingDelete?.name || pendingDelete?.userId || t("trae.page.accounts.unknownAccount"),
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingDelete(null)}>
              {t("trae.page.accounts.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={busy?.startsWith("delete-")}
              onClick={() => {
                const target = pendingDelete;
                if (!target) return;
                setPendingDelete(null);
                void performDelete(target);
              }}
            >
              {t("trae.page.accounts.delete")}
            </Button>
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
        subtitle={
          detail
            ? t("trae.comp.card.detail.subtitle", {
                name: detailName,
                uid: displayText(detail.userId) ?? "—",
              })
            : ""
        }
        avatarSeed={detailName}
        /* ★ 以下五项是**补齐**的（此前 Trae 只注入 packs/sections/programs/remark）——
           它们让共享抽屉在两个分区里内容一致：当前登录徽章 / 切换到此账号 / 操作区 / 签到历史 / 签到积分。

           ⚠️ 不传 `loading`：Trae 的抽屉数据（积分包 / 信息区块）全部来自**已加载**的账号对象，
           没有 WorkBuddy 那种「打开抽屉才逐账号拉积分」的懒加载 ⇒ 没有「未就绪」这个态。
           传一个恒 `false` 的值只会让下一个人以为这里有异步加载。
           ⚠️ 不传 `onWizardSwitch`：那是 WorkBuddy 的「会话复制向导」，Trae 没有会话复制能力
           （`PLAN` §3.15.1）—— 摆一个点不开的向导比不摆更差。 */
        current={detailIsCurrent}
        switchTargets={detailSwitchTargets}
        actions={detailActions}
        checkinHistory={detailCheckinHistory}
        checkin={detailCheckin}
        packs={detailPacks}
        sections={detailSections}
        programs={detailPrograms}
        remark={detailRemark}
        onSaveRemark={detail ? (remark) => saveRemark(detail, remark) : undefined}
        autoEditRemark={detailAutoEditRemark}
        /* ★ 与 WorkBuddy 抽屉**同一取值**：WB 侧不传（默认 `false`），这里显式写 `false`。
           ⚠️ 此前 Trae 传 `api.isDemoMode()` ⇒ 演示构建里 Trae 抽屉的备注**不可编辑**、WB 可编辑。 */
        featuresDisabled={false}
        empty={{
          title: t("shared.drawer.empty.title"),
          description: t("shared.drawer.empty.description"),
          action: (
            <Button size="sm" variant="outline" onClick={() => void loadAll()}>
              {t("trae.page.accounts.emptyRecheck")}
            </Button>
          ),
        }}
      />

      {/* 批量删除确认（桌面 App 不支持 window.confirm）。 */}
      <Dialog open={batchDeleteOpen} onOpenChange={setBatchDeleteOpen}>
        <DialogContent className="sm:max-w-md">
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

      {/* 回收站（P5 §2.7.1）：恢复 = 移回 live ⇒ 成功后重取快照，让恢复的账号立刻回到列表。 */}
      <RecycleBinSheet
        open={recycleOpen}
        onOpenChange={setRecycleOpen}
        scope={scope}
        onRestored={() => void loadAll()}
      />
    </div>
  );
}

/**
 * 空态卡：与 WorkBuddy 的 `EmptyRegionCard` 同构（可能原因 + 期望产物 + 动作）。
 *
 * ## 文案必须说实话：旧版说「登录一次即可自动识别」，那是错的
 *
 * 这张卡原先写着「Cloud-IDE-JWT 存放在该目录下，登录一次 Trae 后即可自动识别」，
 * 并把「从本机导入」作为**主**按钮。但实测 Trae 1.107.1 已改为**加密存储**凭据
 * （`storage.json` / `state.vscdb` / `Local Storage\leveldb` 里都搜不到明文
 * `Cloud-IDE-JWT`），本机导入在这条产品线上**注定失败**。
 *
 * 于是用户看到的正是那句文案：明明已经登录，却怎么都识别不到——这不是探测 bug，
 * 是我们给了一条走不通的路还写了包票。现在把 OAuth 登录提为主入口，
 * 并把本机导入的真实适用范围讲清楚。
 */
function EmptyTraeCard({
  installed,
  dataDir,
  dataDirExists,
  onRecheck,
  onImport,
  onOAuth,
  onLaunch,
  importing,
  launching,
}: {
  installed: boolean;
  /**
   * **当前区域**的客户端数据目录（后端 `platform::select_data_dir_for`）。
   *
   * 不能传 `get_trae_env` 那份 —— 它走 `detect_data_dir()`（**横跨全部产品线**
   * 的全局探测），在区域页里会显示**另一条产品线**的目录名
   * （实测：国际版页签显示 `TRAE SOLO CN`），且本机一个候选都不存在时它会
   * 回落到候选表首项，把一个**根本不存在**的路径说成「已探测」。
   */
  dataDir: string | null;
  dataDirExists: boolean;
  onRecheck: () => void;
  onImport: () => void;
  onOAuth: () => void;
  onLaunch: () => void;
  importing: boolean;
  launching: boolean;
}) {
  const t = useT();
  return (
    <Card className="gap-0 py-0">
      <div className="flex items-start gap-3 px-5 py-5">
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-medium">{t("trae.page.accounts.emptyTitle")}</h2>

          <div className="mt-3 text-sm text-muted-foreground">
            <p className="font-medium text-foreground/80">{t("trae.page.accounts.emptyReasons")}</p>
            <ul className="mt-1 list-disc space-y-1 pl-5">
              <li>{t("trae.page.accounts.emptyReasonNotInstalled")}</li>
              {installed && <li>{t("trae.page.accounts.emptyReasonNoSession")}</li>}
              <li>
                <span className="text-foreground/80">{t("trae.page.accounts.emptyReasonEncryptedLead")}</span>
                {t("trae.page.accounts.emptyReasonEncryptedMid")}
                <code className="mx-1 rounded bg-muted/60 px-1 py-0.5 font-mono text-[11px]">storage.json</code>
                {t("trae.page.accounts.emptyReasonEncryptedTail")}
              </li>
              <li>{t("trae.page.accounts.emptyReasonEmpty")}</li>
            </ul>
          </div>

          <div className="mt-4 rounded-lg border border-border bg-muted/30 px-3.5 py-3">
            <p className="text-sm text-foreground/80">
              <span className="font-medium">{t("trae.page.accounts.emptyRecommendLabel")}</span>
              {t("trae.page.accounts.emptyRecommendBody")}
            </p>
          </div>

          <div className="mt-4 flex flex-wrap gap-2">
            <Button size="sm" onClick={onOAuth}>
              <QrCode />
              {t("trae.page.accounts.oauthLogin")}
            </Button>
            {/* 启动客户端是「客户端从没启动过 ⇒ 没有设备凭证」的唯一解。
                放在这里而不是只放在 OAuth 弹窗里：本空态的「尝试从本机导入」
                同样依赖客户端数据目录，两个入口的前置条件是一样的。 */}
            <Button size="sm" variant="outline" onClick={onLaunch} disabled={launching}>
              {launching ? <Loader2 className="animate-spin" /> : <Rocket />}
              {t("trae.page.accounts.emptyLaunchClient")}
            </Button>
            <Button size="sm" variant="outline" onClick={onRecheck}>
              <RefreshCw />
              {t("trae.page.accounts.emptyRecheck")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={onImport}
              disabled={importing}
              title={t("trae.page.accounts.emptyImportHint")}
            >
              {importing ? <Loader2 className="animate-spin" /> : <Download />}
              {t("trae.page.accounts.emptyImportTry")}
            </Button>
          </div>

          <div className="mt-4">
            {/* 目录不存在时不许写「已探测」——拿一个并不存在的路径当「探测结果」
                会把用户引向「文件在、只是读不出」，而真相是客户端从没启动过。 */}
            <p className="text-sm font-medium text-foreground/80">
              {dataDirExists ? t("trae.page.accounts.emptyDetectedDir") : t("trae.page.accounts.emptyExpectedDir")}
            </p>
            <div className="mt-1.5 flex flex-wrap items-center gap-2">
              <code className="min-w-0 break-all rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-[11px] text-muted-foreground">
                {dataDir ? `${dataDir}\\User\\globalStorage\\storage.json` : t("trae.page.accounts.emptyNoDataDir")}
              </code>
              {!dataDirExists && (
                <span className="text-xs text-warning">
                  {t("trae.page.accounts.dataDirMissing")}
                </span>
              )}
            </div>
            {dataDirExists && (
              <p className="mt-1.5 text-xs text-muted-foreground">
                {t("trae.page.accounts.emptyDirNote")}
              </p>
            )}
          </div>
        </div>
      </div>
    </Card>
  );
}

/** 添加账号对话框：粘贴 JWT + 可选分组。 */
function AddAccountDialog({
  open,
  onOpenChange,
  groups,
  variant,
  onAdded,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  groups: TraeAccountsOverview["groups"];
  /** 写进哪个产品线的账号库。 */
  variant: TraeVariantId;
  onAdded: () => void;
}) {
  const t = useT();
  const [name, setName] = useState("");
  const [jwt, setJwt] = useState("");
  const [groupId, setGroupId] = useState<string>("");
  const [saving, setSaving] = useState(false);

  async function submit() {
    if (!jwt.trim()) {
      toast.error(t("trae.page.accounts.needJwt"));
      return;
    }
    setSaving(true);
    try {
      await api.traeAddAccount(name.trim(), jwt.trim(), groupId || null, variant);
      toast.success(t("trae.page.accounts.accountAdded"));
      setName("");
      setJwt("");
      setGroupId("");
      onOpenChange(false);
      onAdded();
    } catch (e) {
      toast.error(t("trae.page.accounts.addFailed"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("trae.page.accounts.addDialogTitle")}</DialogTitle>
          <DialogDescription>
            {t("trae.page.accounts.addDialogDesc")}
            <br />
            <span className="text-foreground/80">
              {t("trae.page.accounts.addDialogNote")}
            </span>
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="trae-jwt">JWT</Label>
            <Input
              id="trae-jwt"
              value={jwt}
              onChange={(event) => setJwt(event.target.value)}
              placeholder="Cloud-IDE-JWT eyJ…"
              autoComplete="off"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="trae-name">{t("trae.page.accounts.addDialogName")}</Label>
            <Input id="trae-name" value={name} onChange={(event) => setName(event.target.value)} placeholder={t("trae.page.accounts.addDialogNamePlaceholder")} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="trae-group">{t("trae.page.accounts.addDialogGroup")}</Label>
            <Select value={groupId} onValueChange={setGroupId}>
              <SelectTrigger id="trae-group" className="w-full">
                <SelectValue placeholder={t("trae.page.accounts.ungroupedPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {groups.map((group) => (
                  <SelectItem key={group.id} value={group.id}>
                    {group.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
            {t("trae.page.accounts.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={saving}>
            {saving && <Loader2 className="animate-spin" />}
            {t("trae.page.accounts.add")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
