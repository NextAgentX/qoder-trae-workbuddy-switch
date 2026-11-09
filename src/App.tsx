import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState, type ComponentType, type ElementType } from "react";
import { BrowserRouter, HashRouter, Navigate, NavLink, Outlet, Route, Routes, useLocation, useMatch, useNavigate, useParams } from "react-router-dom";
import { ArrowUp, Coffee, GitBranch, LayoutDashboard, MessagesSquare, Monitor, PanelLeftClose, PanelLeftOpen, Server, Settings, User } from "lucide-react";
import { toast } from "sonner";

import { cn } from "@/lib/utils";
import * as api from "@/lib/api";
import type { UpdateInfo } from "@/lib/types";
import { PageFallback } from "@/components/page-fallback";
import { StatusDot } from "@/components/product-marks";
import { DonateDialog } from "@/components/donate-dialog";
import { LegalConsentDialog } from "@/components/legal-consent-dialog";
import { consumeDonatePrompt, DONATE_PROMPT_AT_LAUNCH, noteLaunch } from "@/lib/donate-prompt";
import { TopBar } from "@/components/shell/top-bar";
import { UpdateInstallDialog } from "@/components/update-install-dialog";
import { CommandPalette } from "@/components/command-palette/command-palette";
import { FirstRunHint } from "@/components/first-run-hint";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Toaster } from "@/components/ui/sonner";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { demoModeEnabled, pagesDemoHostingEnabled } from "@/lib/demo-mode";
import { useT } from "@/lib/i18n";
import { scopeLabel } from "@/lib/command-palette";
import type { TranslationKey } from "@/locales/zh";
import {
  AGGREGATE_SHORTCUT_CODE,
  planScopeShortcuts,
  scopeShortcutTarget,
} from "@/lib/scope-shortcuts";
import { useVisibleScopes } from "@/lib/scope-visibility";
import { useShortcutsEnabled } from "@/lib/shortcut-preference";
import { getSidebarCollapsed, setSidebarCollapsed } from "@/lib/sidebar";
import { AUTO_CHECK_UPDATE_INTERVAL_MS, currentSeenUpdateVersion, shouldAutoShowNotes, useAutoCheckUpdate } from "@/lib/update-preference";
import { normalizeReleaseNotes } from "@/lib/release-notes";
import { useScopeStatus } from "@/lib/use-scope-status";
import { useCompanionVisibility } from "@/lib/use-companion-visibility";
import { TRAE_VARIANTS_KEY, loadTraeVariantStatuses } from "@/lib/trae-variant-status";
import type { TraeVariantStatus } from "@/lib/trae-types";

// ---------------------------------------------------------------------------
// 路由级懒加载（P0-2）
//
// 实测（`scripts/bundle-report.config.ts`，2026-09-28）：首屏单 chunk 1.47 MB 里
// **recharts 家族占 407.7 kB / 27.9%**，而它只被 4 个统计页用到（TokenStats /
// CreditStats / TraeTokenStats / TraeCredits）。用户若从不打开统计页，这 400 kB
// 每次冷启动都要解析一遍。
//
// ⇒ 每个页面各自成 chunk：首屏只加载当前路由那一个，其余按需拉取。
// **默认路由（`/` → AccountsPage）也要懒加载**：它在桌面端/WebUI 都从内嵌资源或
// `127.0.0.1` 取，多一次「入口 chunk → 页面 chunk」的往返是毫秒级，
// 换来的是「其它 9 个页面不进首屏」。
//
// ⚠️ 改这里必须同步确认：`Suspense` 仍在**路由层**（见 `Layout` 里的 `<Outlet />`），
// 且 `dist` / `dist-demo` 的 base 差异不受影响（懒加载产生的是同源相对路径）。
// `api::tests::embedded_index_html_references_only_embedded_assets` 会兜住产物一致性。
// ---------------------------------------------------------------------------
const AccountsPage = lazy(() => import("@/pages/AccountsPage"));
/**
 * 聚合账号面板（issue #50）。
 *
 * ⚠️ 它**不是第 7 个作用域**：`Scope` 是封闭字面量 `${平台}-${区域}`，而 `SCOPES`
 * 的长度被三处门禁钉死为 6（作用域数量 / 注册表一致性 / 快捷键序号）。
 * 塞一个 `aggregate` 进去会同时破坏这三处 —— 所以它走独立路由 `/aggregate`，
 * 与 `/s/:scope/:dest` 并列，并且在 `Layout` 里**不渲染侧栏**（见 `bareShell`）。
 */
const AggregateAccountsPage = lazy(() => import("@/pages/AggregateAccountsPage"));
const ApiServicePage = lazy(() => import("@/pages/ApiServicePage"));
const SettingsPage = lazy(() => import("@/pages/SettingsPage"));
const OverviewPage = lazy(() => import("@/pages/OverviewPage"));
const UsagePage = lazy(() => import("@/pages/UsagePage"));
const TraeAccountsPage = lazy(() => import("@/pages/TraeAccountsPage"));
const TraeApiServicePage = lazy(() => import("@/pages/TraeApiServicePage"));
const TraeSettingsPage = lazy(() => import("@/pages/TraeSettingsPage"));
/* P8：第 6 目的地「会话」。两平台各一个组件 —— WorkBuddy 侧是工作台骨架（`SessionsPage`），
   Trae 侧是**结构化空态**（`TraeSessionsPage`，说明「暂不支持会话管理」+ 原因）。
   ⚠️ Trae 侧**不渲染入口**（能力门控），但 URL 直达仍可达 ⇒ 仍需一个组件接住。 */
const SessionsPage = lazy(() => import("@/pages/SessionsPage"));
const TraeSessionsPage = lazy(() => import("@/pages/TraeSessionsPage"));
/* Qoder（第三个平台）：账号页 / 概览页 / 设置页是**真实页面**（真实数据 + 应用级设置）；
   用量 / 网关 / 会话依赖网关请求日志与额度缓存，尚未落地 ⇒ 共用「尚未接入」的结构化空态
   （**不**复用其它平台的页面，否则会把别的平台的账号渲染进来，用户会以为 Qoder 已经在用）。 */
const QoderAccountsPage = lazy(() => import("@/pages/QoderAccountsPage"));
const QoderOverviewPage = lazy(() => import("@/pages/QoderOverviewPage"));
const QoderSettingsPage = lazy(() => import("@/pages/QoderSettingsPage"));
const QoderPendingPage = lazy(() => import("@/pages/QoderPendingPage"));
const QoderApiServicePage = lazy(() => import("@/pages/QoderApiServicePage"));
/* P0 收敛清单第 1 项：`Product` 的类型定义**已搬进注册表**，这里改为 import。
   （原来 App.tsx 里有一份局部 `type Product`，两处重复 —— 现在只剩一处。） */
import { DESTINATION_CAPABILITY, type Destination, DESTINATIONS, destinationOfLegacyPath, hasCapability, platformDescriptor, type Product, platformOfPath } from "@/lib/platforms/registry";
import { parseScope, type Scope, scopeOf, scopeParts, writeLastScope } from "@/lib/scope";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useScope } from "@/lib/use-scope";
import { useTraeVariant } from "@/lib/use-trae-variant";
import { useCreditAutoRefresh } from "@/lib/use-credit-auto-refresh";
import { useWorkbuddyStatusRefresh } from "@/lib/use-workbuddy-status-refresh";
import { useAccountsStore } from "@/stores/accounts";
import { useGatewayStore } from "@/stores/gateway";
import { useSessionsStore } from "@/stores/sessions";

/**
 * 产品分区。
 *
 * WorkBuddy 与 Trae 是两套完全独立的体系（独立账号库、独立客户端、独立配置）。
 * **平台不再是一个「切换器」**：它已并入顶栏的作用域（L1 = 平台 × 区域）——
 * 选 `wb-*` 即进 WorkBuddy，选 `trae-*` 即进 Trae，侧栏与内容区随之跟随。
 *
 * ## ★ Trae 只占**一个**作用域维度，两条产品线在页面内部切换
 *
 * `TRAE SOLO CN`（界面名 `Trae Work`）与 `Trae CN` 确实可以同机并存、各有独立
 * 安装目录与账号库，但**这不意味着要排三个作用域**：
 *
 * - Trae 的两条线**共用全部页面与路由**（`/s/trae-cn/accounts` 等）。把它们拆成
 *   两个顶级入口，等于让同一组页面出现两遍，用户要在两个看起来一样的入口之间
 *   做无意义的二选一。
 * - 产品线（程序位）是**数据维度**而非**页面维度**，因此正确的载体是 Trae 页面
 *   内部的程序位切换器（见 `TraeAccountsPage`），而不是作用域。
 *
 * Trae 的**区域**（cn/global）则属于作用域，由 URL 承载（`useTraeVariant`
 * 路径优先、回落旧 `?line=`），刷新 / 分享 / 前进后退都能保持。
 */

/* `interface NavItem` 随 `PRODUCT_NAV` 一并删除（P2）：
   侧栏现在由 `DESTINATIONS` + `DEST_META` 生成，条目形状内联在 `Layout` 的 `scopedNav` 里。
   原先的 `end?` 字段也不再需要 —— 每个目的地是**独立路径**，不存在「子页面同时点亮两条」。 */

/**
 * 侧栏导航（**P2 起由注册表生成**）。
 *
 * ## 改造前
 *
 * 这里有一张 `PRODUCT_NAV`，两个产品各 5 项**硬编码**：同样的 labelKey、同样的图标，
 * **只有路径前缀不同**（`/` vs `/trae/`）。加一个目的地要改两处，且必然漏改一处。
 *
 * ## 现在
 *
 * 侧栏由 `DESTINATIONS`（注册表，6 项，**平台无关**）+ `DEST_META`（文案键与图标）
 * 在 `Layout` 里拼出**当前 scope 的路径**（`/s/{scope}/{dest}`）。
 * ⇒ 加一个目的地只需改注册表 + `DEST_META`，**两个平台同时生效**。
 *
 * ## 为什么能合并（这不是巧合）
 *
 * 原注释自己写着「Trae 与 WorkBuddy 之间**逐字同构**：同样的五项、同样的顺序、
 * 同样的路径尾段」—— **既然逐字同构，就不该写两遍**。
 * P2 把「同构」这件事**从约定变成结构**：只有一份，想不同构都难。
 *
 * ## Trae 的两条产品线仍共用一组路径
 *
 * 路由不带程序位维度：「当前是哪条线」由 `?line=` 承载、由页面内的变体切换器驱动
 * （见 `useTraeVariant`）。理由是**程序位是数据维度而不是页面维度** ——
 * 两条线的页面结构完全相同，再造一套路由只会让每个页面组件被迫复制一份。
 */

/* 产品显示名（`PRODUCT_LABEL_KEY`）与产品首页（`PRODUCT_HOME`）**已随 P2 删除** ——
   两者都在注册表的 `PlatformDescriptor` 里（`labelKey` / `home`），
   本文件只从 `platformDescriptor(product)` 现取。原先两份常量的完整理由
   （为什么存文案键、Trae 分区为何显示 `TraeWork`、区域为何不进侧栏）已随字段迁到
   `registry.ts` 的 `PlatformDescriptor.labelKey` 注释，**不在两处各留一份**。 */

/**
 * 路由 → 产品。
 *
 * ⚠️ 实现**已收敛到注册表的 `platformOfPath`**（P0 收敛清单）——
 * 原来这里有一份同款判断，两处各写一遍必然漂移。
 */
const productFromPath = platformOfPath;

/**
 * 目的地 → 页面组件（**P2 新增**）。
 *
 * `/s/{scope}/{dest}` 用这张表把 `dest` 解析成组件 ⇒ **新增目的地只需改这里 + 注册表**，
 * 不必再写一条 `<Route>`。「侧栏固定 5 项」由此有了单一事实来源。
 *
 * ⚠️ 用 `ElementType` 而不是 `ComponentType`：这些页面是 `lazy()` 包出来的
 * `LazyExoticComponent`，它**不满足** `ComponentType`，但可以当 JSX 标签用。
 */
const SCOPED_ROUTES: Record<Product, Record<Destination, ElementType>> = {
  workbuddy: {
    overview: OverviewPage,
    accounts: AccountsPage,
    usage: UsagePage,
    sessions: SessionsPage,
    gateway: ApiServicePage,
    settings: SettingsPage,
  },
  trae: {
    /* 概览与用量两个分区**共用同一个组件**（P3）：它们从路由推平台，因此不需要两份。
       这正是「差异只剩数据与能力开关」的实例。 */
    overview: OverviewPage,
    accounts: TraeAccountsPage,
    usage: UsagePage,
    /* P8：Trae 侧**不渲染**「会话」入口（能力门控），但 URL 直达仍可达 ⇒
       给**结构化空态**组件（说明「暂不支持会话管理」+ 原因），**不**列 WorkBuddy 的会话。 */
    sessions: TraeSessionsPage,
    gateway: TraeApiServicePage,
    settings: TraeSettingsPage,
  },
  qoder: {
    /* 概览 / 设置是**真实页面**（概览：账号数 + 本机客户端四态；设置：应用级设置 + 备份）。
       网关也已落地（**第三个平台的产品线**：独立监听 57895 + 按区域分家的池与 Key 库）。
       ★ 用量页**已接回**（2026-10-18，issue #46 的 C 项）：`UsagePage` 按 `USAGE_PAGES.qoder`
         映射到 `QoderUsagePage`（后端 `qoder_usage` 命令早已存在，数据源有真实值），
         而 `usage` 入口此前还停在占位页 ⇒「有数据却显示尚未接入」。
         ⚠️ 与 `OverviewPage` 的处理**刻意不同**：概览页读的是 WorkBuddy 的 store，复用会给
         Qoder 分区渲染别的平台的数据；`UsagePage` 本身**从路由推平台**（`platformOfPath`），
         三个平台共用是设计内复用，不是妥协。
       会话**仍未落地**（数据源「会话副本」不存在）⇒ 继续给「尚未接入」空态而不是空壳 KPI。 */
    overview: QoderOverviewPage,
    accounts: QoderAccountsPage,
    usage: UsagePage,
    sessions: QoderPendingPage,
    gateway: QoderApiServicePage,
    settings: QoderSettingsPage,
  },
};

/**
 * 目的地的展示元数据（**P2 新增**）。
 *
 * ⚠️ 存**文案键**而非成品文案：本表是模块级常量，拿不到 `useT()`
 * （与 `PRODUCT_LABEL_KEY` 同一处理，否则语言切换对它失效）。
 *
 * ⚠️ **两个平台共用同一份** —— 这正是「侧栏固定 6 项」成立的依据：
 * 改造前两套 `PRODUCT_NAV` 的 labelKey 与 icon **本来就完全相同**，
 * 唯一的差别是**路径前缀**（`/` vs `/trae/`）。P2 之后路径由 scope 决定，差别就消失了。
 */
const DEST_META: Record<
  Destination,
  { labelKey: TranslationKey; icon: ComponentType<{ className?: string }> }
> = {
  overview: { labelKey: "nav.overview", icon: LayoutDashboard },
  accounts: { labelKey: "nav.accounts", icon: User },
  /* P3：`usage` 现在承载 **Token 统计 + 积分统计**（§3.9 的合并）——
     所以标签从「Token 统计」改成「用量」，否则侧栏会漏掉积分。 */
  usage: { labelKey: "nav.usage", icon: MessagesSquare },
  /* P8：第 6 目的地「会话」（跨账号副本关联）。入口受**能力门控**
     （`DESTINATION_CAPABILITY.sessions = "sessionCopy"`）——Trae 无该能力
     ⇒ 侧栏不渲染它；URL 直达由 `TraeSessionsPage` 给结构化空态。 */
  sessions: { labelKey: "nav.sessions", icon: GitBranch },
  gateway: { labelKey: "nav.apiService", icon: Server },
  settings: { labelKey: "nav.settings", icon: Settings },
};

/**
 * `/s/{scope}/{dest}` 的解析组件（**P2 新增**）。
 *
 * 把「作用域 + 目的地」翻译成一个页面组件。**非法输入一律回首页**（fail-closed）——
 * 不猜、也不回落某个默认 scope：静默回落会把用户带进**错误作用域**的数据视图
 * （同 `parseScope` 的纪律，见 `lib/scope.ts`）。
 *
 * ⚠️ 与旧路由（`/`、`/trae/*`）**并存**：旧路由**不删** ⇒ 旧书签可达（P2 验收项）。
 */
function ScopedDestination() {
  const { scope: rawScope, dest: rawDest } = useParams();

  const scope = parseScope(rawScope);
  if (!scope) return <Navigate to="/" replace />;

  if (!(DESTINATIONS as readonly string[]).includes(rawDest ?? "")) {
    return <Navigate to="/" replace />;
  }

  const { product } = scopeParts(scope);
  const Page = SCOPED_ROUTES[product][rawDest as Destination];
  return <Page />;
}

/**
 * 侧栏导航项（原型 `.nav-item`，`demo.css:147-163`）—— 逐值对齐：
 * 高 38px、内边距 `0 10px`、圆角 10px、字号 13px；悬停 `foreground 6%`；
 * **选中 `primary 16%` + 图标转primary + 字重 550**（原型还额外强调图标，不只是背景）。
 *
 * ⚠️ 选中底色是**16%** 不是 15%（原型 `:155` 是 `color-mix(primary 16%, transparent)`），
 * 与 `.nav-badge.warn` 的 22% 各不相同 —— 两处别互抄（`.brand-mark` 的 18% 已随 issue #75 删掉）。
 */
function navLinkClass({ isActive, collapsed }: { isActive: boolean; collapsed?: boolean }): string {
  return cn(
    "group flex h-[38px] items-center gap-2.5 rounded-[10px] text-[13px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-sidebar-ring/50",
    /* 折叠态：图标居中（`justify-center` + 方形 34px）—— 宽度只有 60px，
       再留 `px-2.5` 会让图标偏左，六枚图标排成一列歪的。 */
    collapsed ? "w-[34px] justify-center px-0" : "w-full px-2.5",
    isActive
      ? "bg-primary/16 font-[550] text-foreground [&>svg]:text-primary"
      : "text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground",
  );
}

/** 侧栏导航项的数据形状（`scopedNav` 的元素）。 */
type ScopedNavItem = {
  dest: Destination;
  to: string;
  labelKey: TranslationKey;
  icon: ComponentType<{ className?: string }>;
  badge: { n: number; warn: boolean } | null;
};

/**
 * 侧栏导航项（单个）。
 *
 * ⚠️⚠️ **`className` 必须是字符串，`isActive` 必须在组件内自己算** —— 这曾是一次线上事故：
 *
 * `<TooltipTrigger asChild>` 走 Radix 的 `Slot`，而 `Slot.mergeProps` 对 `className` 的处理是
 * `[slotValue, childValue].filter(Boolean).join(" ")` —— **它不会调用函数**，只会把函数
 * `String()` 成一个字符串。于是 `<NavLink className={({ isActive }) => ...}>` 合并后变成
 * `class="({isActive:...}) => ..."`：所有工具类全部失效，导航项退化成块级元素
 * （图标一行、文字一行、`h-[38px]` / 内边距 / 圆角全丢，选中态只剩一条错位的横杠）。
 * 2026-10-22 GitHub Pages 上的侧栏就是这个症状 —— 与构建、base、CSS 产物都无关，
 * 是 React 层的 className 契约被 `asChild` 打穿。
 *
 * ⇒ 判活改由 `useMatch` 完成，规则与 `NavLink` 内部**同一套**
 * （`end: false` 正是 NavLink 对非根路径的默认值），交给 `Slot` 的永远是静态字符串。
 * 别再改回 `className={({ isActive }) => ...}`。
 */
function SidebarNavItem({ item, collapsed }: { item: ScopedNavItem; collapsed: boolean }) {
  const t = useT();
  const isActive = useMatch({ path: item.to, end: false }) !== null;
  const badge = item.badge;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <NavLink to={item.to} className={navLinkClass({ isActive, collapsed })}>
          {/* 折叠态：图标自身居中（`justify-center` 由 navLinkClass 的 collapsed 变体给出），
              文字不渲染；徽章从「行尾横排」改成**图标右上角标** —— 60px 里横排放不下，
              而角标是「有未读/有待处理」的通用语言，不依赖文字。 */}
          <span className="relative">
            <item.icon className="size-4" />
            {collapsed && badge !== null ? (
              <span
                data-badge={item.dest === "sessions" ? "pending-sync" : `count-${item.dest}`}
                className={cn(
                  "absolute -right-1.5 -top-1.5 grid h-[14px] min-w-[14px] place-items-center rounded-full px-1 text-[9px] tabular-nums ring-2 ring-sidebar",
                  badge.warn ? "bg-warning/22 text-warning" : "bg-muted text-muted-foreground",
                  isActive && "bg-primary/25 text-foreground",
                )}
              >
                {badge.n}
              </span>
            ) : null}
          </span>
          {collapsed ? null : (
            <>
              {t(item.labelKey)}
              {badge !== null ? (
                <span
                  data-badge={item.dest === "sessions" ? "pending-sync" : `count-${item.dest}`}
                  className={cn(
                    "ml-auto grid h-[18px] min-w-[20px] place-items-center rounded-full px-1.5 text-[10.5px] tabular-nums",
                    badge.warn ? "bg-warning/22 text-warning" : "bg-muted text-muted-foreground",
                    isActive && "bg-primary/25 text-foreground",
                  )}
                >
                  {badge.n}
                </span>
              ) : null}
            </>
          )}
        </NavLink>
      </TooltipTrigger>
      {/* 折叠态才需要 tooltip（展开态文字就在旁边；双份显示反而冗余）。 */}
      {collapsed ? (
        <TooltipContent side="right">
          {t(item.labelKey)}
          {badge !== null ? ` · ${badge.n}` : ""}
        </TooltipContent>
      ) : null}
    </Tooltip>
  );
}

/**
 * 每条 Trae 产品线的运行状态。
 *
 * 只在 Trae 分区激活时探测：在 WorkBuddy 分区下这次探测没有意义，
 * 而演示模式下 `get_trae_variants` 会直接抛错（按「未运行」处理即可）。
 *
 * **返回两条线各自的运行状态**（而不是单一的 true/false）：
 * 侧栏底部那颗圆点跟随**当前选中的那条线**，若只探测「Trae 是否在运行」，
 * 在「Trae Work 已关闭、Trae CN 在运行」时就会显示错误的绿灯。
 *
 * ## 走快照缓存的两个理由
 *
 * 1. **不让圆点在每次切换时先灰一下再变绿**：不缓存的话，切回 Trae 分区的那一帧
 *    状态是空的，圆点会闪一次「未运行」。
 * 2. **`freshMs` 给 60 秒**：这颗圆点是**装饰性**的（真实状态在 Trae 页面的状态条
 *    上），不值得每次切换都重探一次；窗口之外照旧后台重校验，每分钟也仍有一次定时刷新。
 *
 * 键在「不在 Trae 分区」时置为 `null`（不缓存、不探测），避免在 WorkBuddy 分区下白跑。
 */
function useTraeVariantRunning(active: boolean): Record<string, boolean> {
  const { data, refresh } = useCachedResource<TraeVariantStatus[]>(
    active ? TRAE_VARIANTS_KEY : null,
    loadTraeVariantStatuses,
    { freshMs: 60 * 1000 },
  );

  useEffect(() => {
    if (!active) return;
    // ⚠️ 这里刻意用 `refresh()`（强制）而不是 `revalidate()`：本 hook 是**唯一**的定时器
    // 持有者，而它的 `freshMs` 恰好等于间隔（60s）。换成 `revalidate()` 后，tick 会因为
    // 「距上次写入还差几十毫秒不到一个窗口」被窗口闸门吃掉 ⇒ 实际节奏退化成 **120 秒**，
    // 且不报任何错。要用 `revalidate()` 就必须同时把 `freshMs` 调小于间隔
    // （`use-scope-status.ts` 就是这么做的，那里有多个消费者、非去重不可）。
    const timer = window.setInterval(() => void refresh(), 60 * 1000);
    return () => window.clearInterval(timer);
  }, [active, refresh]);

  return useMemo(
    () => Object.fromEntries((data ?? []).map((item) => [item.variant, item.running])),
    [data],
  );
}

/**
 * 侧栏左下角的「会话悬浮窗」入口（P9 · T04）。
 *
 * ## 为什么是**开关**，而不是第二个切换器
 *
 * 可见性**只有**三处：侧栏（本组件）/ 托盘菜单项 / 窗口内关闭按钮。三者经
 * `companion_set_visible` 收敛到**同一份**偏好（`useCompanionVisibility`），
 * 因此本组件是「同一状态的一面」，**不**再摆下拉 / Tab 之类「看起来能切」的控件
 * （P9 §1.5 / R8）。它**不占侧栏 5 项** —— 位置在侧栏左下角（与原型 `index.html:67`
 * 的 `.mini-btn` 同位，即版本行里的 mini 图标按钮）。
 *
 * ## 入口不是一次性的
 *
 * 悬浮窗「关闭 = 隐藏」（**不销毁**，见 `companion.rs` 的 `CloseRequested`）⇒ 关掉后
 * 仍能从**这里**或托盘重开（P9 B4）。默认可见（§3.16「默认显示」）。
 *
 * ## 状态自明
 *
 * `aria-pressed` + 视觉明暗反映**当前可见性** —— 读的是与托盘 / 关闭按钮同一份状态，
 * 不会各说各话。仅在**桌面端**渲染：webui / 移动端没有悬浮窗，摆一个点不动的按钮
 * 就是「看起来能切」的假控件。
 */
function CompanionToggle() {
  const t = useT();
  const [visible, setVisible] = useCompanionVisibility();
  const label = t("shared.companion.entry");

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          /* 几何与原型 `.mini-btn`（`buddy-switch-prototype.html:188-192`）一致：20px、**圆角 999（胶囊）**、图标 11px。
             颜色**保留开关语义**（开=实心 primary，与原型静止态一致；关=弱化），
             —— 原型那两枚是普通动作按钮，这一枚是「显示/隐藏悬浮窗」的开关，
             若一律涂成实心就丢掉了开关状态这个信号。 */
          className={cn(
            "size-5 shrink-0 rounded-full p-0",
            visible
              ? "bg-primary text-primary-foreground hover:bg-primary/90"
              : "text-sidebar-foreground/40 hover:text-sidebar-foreground/70",
          )}
          aria-label={label}
          aria-pressed={visible}
          onClick={() => setVisible(!visible)}
        >
          <Monitor className="size-[11px]" aria-hidden="true" />
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * 侧栏底部的打赏入口（**版本号上方**）。
 *
 * ## 为什么在侧栏底部，而不是只留在顶栏 ⚙ 菜单
 *
 * 打赏是「顺手支持一下」的动作，没人会去翻设置菜单找它；侧栏底部常驻可见，
 * 紧贴版本号放着，语义正好是「这个应用免费开源，可以请作者喝饮料」。
 * 顶栏 ⚙ 菜单里那一项**保留**（与「更新」弹层同款：同一弹层允许两个入口，
 * 各自持有自己的 `open`），因此这里**不**再复制一份收款码 UI。
 *
 * ## 为什么是「图标 + 文字」的整行按钮，不是无文字图标
 *
 * 侧栏底部已经有状态圆点与更新按钮两处图标入口，再加一枚无文字图标，
 * 用户只能靠猜。整行按钮语义自明。
 *
 * ## 弹层仍是**受控**的
 *
 * `DonateDialog` 自己不带触发按钮（与顶栏 ⚙ 菜单共用同一份实现），
 * 入口只负责持有 `open` 状态 —— 收款码只有一处实现。
 */
function SidebarDonate({ collapsed }: { collapsed: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);

  /* ★ 折叠态收成 34px 图标钮（与导航项同尺寸）—— tooltip 顶上文字缺失。
     ⚠️ 不要改成无文字图标：侧栏底部已有两枚图标入口，再加一枚只能靠猜。 */
  const button = (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      /* 左内边距与 `navLinkClass` 同为 `px-2.5` ⇒ 图标与上面 5 个导航项对齐。 */
      className={cn(
        "h-8 rounded-[10px] text-[12.5px] font-normal text-sidebar-foreground/70 hover:bg-foreground/[0.06] hover:text-sidebar-foreground",
        collapsed ? "w-[34px] justify-center px-0" : "w-full justify-start gap-2.5 px-2.5",
      )}
      onClick={() => setOpen(true)}
      title={t("wbSettings.donate.trigger")}
    >
      <Coffee className="size-3.5" aria-hidden="true" />
      {collapsed ? null : t("wbSettings.donate.trigger")}
    </Button>
  );

  return (
    <>
      {collapsed ? (
        <Tooltip>
          <TooltipTrigger asChild>{button}</TooltipTrigger>
          <TooltipContent side="right">{t("wbSettings.donate.trigger")}</TooltipContent>
        </Tooltip>
      ) : (
        button
      )}
      <DonateDialog open={open} onOpenChange={setOpen} />
    </>
  );
}

/**
 * 侧栏底部的应用信息。
 *
 * 这里展示的是**本应用**（Buddy Switch）的版本，而不是所管理客户端的版本：
 * `status.version` 来自 `update::APP_VERSION`，此前挂在「WorkBuddy」名下会让人
 * 误以为它是 WorkBuddy 客户端的版本号。状态圆点跟随当前选中的产品。
 *
 * **只负责内容**：底部区块的边框、上间距与纵向间隔归 `Layout` 里那个
 * `<section>` —— 同一块里还要放打赏入口（`SidebarDonate`），而它在 webui 下
 * **仍然显示**（版本行则不显示），把容器留在 `Layout` 才能只写一处。
 * webui 下整行不渲染（没有桌面运行态可展示）。
 */
function AppFooter({
  product,
  running,
  version,
}: {
  product: Product;
  running: boolean;
  version: string | undefined;
}) {
  const t = useT();
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  /* 「自动检查更新」开关（顶栏 ⚙ 抽屉里那一行）。关掉后**挂载时的首次检查也不做** ——
     「自动」包含首查；想手动查的人走设置里的「检查更新」按钮。 */
  const [autoCheck] = useAutoCheckUpdate();
  /* 已经主动弹过的版本（本次运行内）。同一版只弹一次：用户「稍后升级」是正常决策，
     30 分钟后再把同一份说明糊回脸上就是骚扰。 */
  const autoShownRef = useRef<string | null>(null);

  useEffect(() => {
    if (!autoCheck) {
      // 关掉自动检查时清空已有结果：否则上一次查到的新版本角标会一直挂着，
      // 看起来像「关了还在查」。
      setInfo(null);
      return;
    }
    let disposed = false;

    async function checkForUpdate() {
      try {
        const result = await api.checkUpdate();
        if (disposed) return;
        if (!result.ok) {
          setInfo(null);
          return;
        }
        /* 版本说明跨了「外部资产 → 渲染」的边界，进 state 前先按白名单归一
           （`lib/release-notes.ts`）：宁可少显示一条，也不把未知结构送进渲染树。 */
        const normalized: UpdateInfo = { ...result, notes: normalizeReleaseNotes(result.notes) };
        setInfo(normalized);
        /* ★ 新版本第一次被发现时**主动弹一次说明**。
           只挂角标已被证明无效 —— 想知道的人不会去点，于是永远不知道改了什么（用户原话）。
           判据全在 `shouldAutoShowNotes`：桌面端 / 确有更新 / 这一版没看过。 */
        if (
          autoShownRef.current !== normalized.latest &&
          shouldAutoShowNotes({
            desktop: api.isDesktop(),
            hasUpdate: Boolean(normalized.hasUpdate),
            latest: normalized.latest,
            seenVersion: currentSeenUpdateVersion(),
          })
        ) {
          autoShownRef.current = normalized.latest ?? null;
          setDialogOpen(true);
        }
      } catch {
        // 左下角只展示可操作的升级状态，网络错误不打扰正常使用。
      }
    }

    void checkForUpdate();
    const timer = window.setInterval(() => void checkForUpdate(), AUTO_CHECK_UPDATE_INTERVAL_MS);

    /* ★ 窗口重新可见 / 重新获得焦点时**补一次**检查。
     *
     * 为什么需要它：`setInterval` 在隐藏窗口里会被 WebView 节流（最小化、收进托盘、
     * 被别的窗口盖住），轮询不再准时 —— 用户回到应用时看到的可能仍是几十分钟前的结论。
     * 而「回来时正好有新版」恰恰是最需要提示的时刻。
     *
     * 为什么**不**传 `force`：焦点切换是高频事件（alt-tab 一次一个），强制刷新会把
     * 它变成 GitHub 请求风暴。后端的成功结果缓存（`update.rs::CACHE_TTL_SECS`，5 分钟）
     * 就是这里的节流器：5 分钟内重复触发直接命中缓存，不发网络请求；超过 5 分钟才真正
     * 重新检查 —— 这也正好是我们要的语义。
     *
     * 两个事件都要，且**判据不同**：
     * - `visibilitychange` 覆盖最小化 / 托盘恢复 —— 只在 `visible` 时查（切走时不该发请求）；
     * - `focus` 覆盖「窗口一直可见但用户刚切回来」——**不判** `visibilityState`：能获得焦点
     *   就一定是可见的，而 WebView2 在托盘隐藏时的 `visibilityState` 未必可靠，多一道门
     *   只可能让「恢复后一次都不查」。 */
    function onVisibilityChange() {
      if (document.visibilityState === "visible") void checkForUpdate();
    }
    function onWindowFocus() {
      void checkForUpdate();
    }
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("focus", onWindowFocus);

    return () => {
      disposed = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("focus", onWindowFocus);
    };
  }, [autoCheck]);

  const hasUpdate = Boolean(info?.ok && info.hasUpdate && info.latest);
  const productLabel = t(platformDescriptor(product).labelKey);

  return (
    <>
      <div>
        <div className="flex items-center gap-2 px-1 text-[11.5px] text-muted-foreground">
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">
                <StatusDot on={running} />
              </span>
            </TooltipTrigger>
            <TooltipContent side="top">
              {t(running ? "sidebar.running" : "sidebar.notRunning", { product: productLabel })}
            </TooltipContent>
          </Tooltip>
          {/* 版本号用 tabular-nums，让数字变化时纵向对齐、不跳动。 */}
          <span className="min-w-0 flex-1 truncate tabular-nums text-sidebar-foreground/70">
            v{version || "?"}
          </span>
          <div className="flex shrink-0 items-center gap-1.5">
            {/* P9 T04：会话悬浮窗入口（桌面端；与原型版本行的 `.mini-btn` 同位）。
                它是**可见性开关**，与托盘 / 窗口内关闭按钮收敛到同一份偏好 —— 不新增第二个切换器。 */}
            {api.isDesktop() ? <CompanionToggle /> : null}
            {hasUpdate && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    size="icon"
                    /* 与 `.mini-btn` 同几何（20px / 胶囊 / 图标 11px）。 */
                    className="size-5 rounded-full p-0"
                    aria-label={t("sidebar.update")}
                    onClick={() => setDialogOpen(true)}
                  >
                    <ArrowUp className="size-[11px]" strokeWidth={2.5} aria-hidden="true" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="top">{t("sidebar.update")}</TooltipContent>
              </Tooltip>
            )}
          </div>
        </div>
      </div>
      <UpdateInstallDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        update={info}
      />
    </>
  );
}

function Layout() {
  const t = useT();
  const location = useLocation();
  const navigate = useNavigate();

  /**
   * 当前产品分区（来自路由前缀）。
   *
   * 路径只能回答「是 WorkBuddy 还是 Trae」，回答不了「Trae 里是看哪条产品线」——
   * 两条线共用 `/trae/...`。产品线的选择由页面内部读取（见 `useTraeVariant`）。
   */
  const product: Product = productFromPath(location.pathname);

  /**
   * ★ 聚合视图 = **裸壳路由**（issue #50 D5）。
   *
   * 它没有左侧导航栏：侧栏那 6 个目的地（概览 / 账号 / 用量 / 会话 / 网关 / 设置）
   * **全部是单作用域概念**，聚合下没有「概览」「用量」—— 留着只会让人点了落回某个
   * 作用域，制造「我明明在聚合页」的矛盾。
   *
   * ⚠️ `Layout` 只知道「这条路由不要侧栏」，**不知道「聚合」这件事** ——
   * 判断放在路由形状上，外壳的职责仍然是纯外壳（不塞业务概念）。
   */
  const bareShell = Boolean(useMatch("/aggregate"));

  /**
   * ★ 当前作用域（L1）= 平台 × 区域，顶栏与侧栏都读它。
   *
   * - **平台**来自路由；**区域**也来自 URL（`/s/{scope}/{dest}` 的路径段优先）。
   * - Trae 侧区域走 `useTraeVariant`（它还要兼容旧 `?line=`），WorkBuddy 侧走 `useScope`。
   *   两者同源：都存在 URL 里，切一次**整页跟随**（§3.1）。
   */
  const [wbRegion] = useScope(product);
  const [traeRegion] = useTraeVariant();
  const navRegion = product === "trae" ? traeRegion : wbRegion;
  const scope = scopeOf(product, navRegion);
  /** 六个作用域的环境状态（顶栏作用域菜单的唯一取数来源）。 */
  const { statuses, refreshProduct } = useScopeStatus();

  /* P6：命令面板（⌘K）的开关状态。入口有两个 —— 顶栏搜索按钮与全局 ⌘K。
     状态放在 `Layout`（而非 `App`）里，是为了与顶栏入口共享、且都处于 Router 上下文内。 */
  const [paletteOpen, setPaletteOpen] = useState(false);

  /* ★ 侧栏折叠（只显示图标）—— 2026-10-22 加。
     为什么值得做：侧栏固定 240px，在 1440 宽的窗口上占到 17%；用户截图反馈
     「集成到这个位置太拥挤」的同一类问题 —— 内容区才是被看的对象，侧栏是导航。
     折叠是**纯渲染层偏好**（只有前端读）⇒ localStorage（`lib/sidebar.ts`），
     不进 core `app_prefs.json`（那条路是给「宿主也读」的偏好留的）。 */
  const [sidebarCollapsed, setSidebarCollapsedState] = useState(getSidebarCollapsed);
  const toggleSidebar = useCallback(() => {
    setSidebarCollapsedState((collapsed) => {
      const next = !collapsed;
      setSidebarCollapsed(next);
      return next;
    });
  }, []);

  /* ⌘B / Ctrl+B：开关侧栏折叠。与 ⌘K（命令面板）、⌘1..9（切作用域）同族同规则：
     修饰键必须带、不能是组合（alt/shift 一律不接）、输入框内不接。 */
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key.toLowerCase() !== "b") return;
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      if (isTypingTarget(event.target)) return;
      event.preventDefault();
      toggleSidebar();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggleSidebar]);

  /* P6：作用域快捷键规划。降级（>9 个作用域或用户关闭）⇒ 空绑定 ⇒ 序号提示同步消失。
     P17：只用**可见**作用域 —— 隐藏的作用域不给 ⌘N（面板提示与全局键同源，不会漂移）。 */
  const [shortcutsEnabled] = useShortcutsEnabled();
  const visibleScopes = useVisibleScopes((s) => s.visibleScopes);
  const shortcutPlan = useMemo(
    () => planScopeShortcuts({ scopes: visibleScopes, enabled: shortcutsEnabled }),
    [visibleScopes, shortcutsEnabled],
  );

  /* P2：**侧栏由注册表生成** —— 平台之间只差 scope（§3.1「侧栏固定 6 项」）。
     改造前是两套硬编码的 `PRODUCT_NAV`（5:5），新增目的地要改两处。
     ⚠️ 路径由**当前作用域**拼出，所以切作用域时侧栏链接**跟着变**（URL 成为唯一真值）。
     P8：入口先按**能力门控**过滤（`DESTINATION_CAPABILITY` + `hasCapability`）——
     缺能力的平台**不渲染**该入口（Trae 无 `sessionCopy` ⇒ 无「会话」），
     但**不封路由**：URL 直达仍由页面自己给空态（§3.2 规则 2）。 */
  /**
   * ★ P8：侧栏「会话」徽章 = **待同步组数**（`behind` + `diverge`）。
   *
   * ⚠️ **只读「已加载」的 groups** —— 不在 `App` 里主动拉会话数据（那是页面的职责，
   * 否则每次开应用都要读一遍 jsonl）。**未加载 ⇒ 不显示徽章**（诚实，不臆造 0）。
   * ⚠️ 判据是**内容级状态**（fingerprint 派生），**不是** `updatedAt`。
   */
  const sessionGroups = useSessionsStore((s) => s.groups);
  const pendingSyncCount = useMemo(
    () => sessionGroups.filter((g) => g.status === "behind" || g.status === "diverge").length,
    [sessionGroups],
  );
  /**
   * ★ 侧栏徽章的数据源 —— **只读已加载的 store**（原型 `renderNav` 的三枚徽章）。
   *
   * ⚠️ **不在 `App` 里主动 load**：会话要读一遍 jsonl、网关密钥要打一次宿主接口 ——
   * 为了三枚徽章让应用每次冷启动都做这些事，代价与收益不成比例。
   * 未加载 ⇒ 不显示徽章（诚实，不臆造 `0`）。
   *
   * ① 账号数：来自 `useScopeStatus`（顶栏作用域菜单的同一份事实，**不额外取数**）；
   *    有「需处理」账号（凭据已过期 / 需重登）时转警示色（原型 `warn: expiredCount > 0`）；
   * ② 网关密钥数：网关 store 的 `keys`（**只有进过网关页才非空**）；
   * ③ 会话待同步组数：`behind + diverge`，恒警示色（有事要处理）。
   */
  const gatewayKeys = useGatewayStore((s) => s.keys);

  function navBadge(dest: Destination): { n: number; warn: boolean } | null {
    if (dest === "accounts") {
      /* 与顶栏作用域菜单**同源**（`useScopeStatus`）：数出与「N 个需处理」不会再对不上。
         `attentionCount` 为 `null`（未知）时按「无警示」处理 —— 不拿未知冒充有事。 */
      const n = statuses[scope].accountCount;
      const attention = statuses[scope].attentionCount ?? 0;
      return n > 0 ? { n, warn: attention > 0 } : null;
    }
    if (dest === "sessions") {
      return pendingSyncCount > 0 ? { n: pendingSyncCount, warn: true } : null;
    }
    if (dest === "gateway") {
      return gatewayKeys.length > 0 ? { n: gatewayKeys.length, warn: false } : null;
    }
    return null;
  }

  const scopedNav = useMemo(
    () =>
      DESTINATIONS.filter((dest) => {
        const capability = DESTINATION_CAPABILITY[dest];
        return capability === undefined || hasCapability(product, capability);
      }).map((dest) => ({
        dest,
        to: `/s/${scopeOf(product, navRegion)}/${dest}`,
        labelKey: DEST_META[dest].labelKey,
        icon: DEST_META[dest].icon,
        badge: navBadge(dest),
      })),
    // `navBadge` 读到的事实都在下面这几个依赖里（含 `statuses` / 网关密钥 / 待同步数）。
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [product, navRegion, pendingSyncCount, statuses, gatewayKeys],
  );

  const appVersion = useAccountsStore((s) => s.status?.version || s.global.status?.version);
  const workbuddyRunning = useAccountsStore((s) =>
    Boolean(s.status?.running || s.global.status?.running),
  );
  const traeRunning = useTraeVariantRunning(product === "trae");
  // Trae 分区的状态圆点跟随**当前选中的区域**（而不是「Trae 是否有任意一条在跑」）：
  // 在「Trae CN 已关闭、Trae 国际版在运行」时，只探「Trae 是否运行」会显示错误的绿灯。
  const running = product === "workbuddy" ? workbuddyRunning : Boolean(traeRunning[traeRegion]);

  useCreditAutoRefresh();
  useWorkbuddyStatusRefresh();

  /* P6 全局键 —— 判据沿用 `AccountsPage` 的输入框豁免（`INPUT` / `TEXTAREA` / 可编辑区
     聚焦时不劫持，§1.10）。两个键都只在**非输入态**生效，避免与打字冲突。 */
  function isTypingTarget(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    const tag = el?.tagName;
    return tag === "INPUT" || tag === "TEXTAREA" || el?.isContentEditable === true;
  }

  /** ⌘K / Ctrl+K：开关命令面板（面板本身不是切换器，只是搜索/跳转入口）。 */
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key.toLowerCase() !== "k") return;
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      if (isTypingTarget(event.target)) return;
      event.preventDefault();
      setPaletteOpen((open) => !open);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  /* 当前目的地：`/s/{scope}/{dest}` 取路径段；旧路由走注册表映射（缺失回落 accounts）。 */
  const currentDest = useMemo(() => {
    const match = /^\/s\/[^/]+\/([^/]+)/.exec(location.pathname);
    if (match) return match[1];
    return destinationOfLegacyPath(location.pathname);
  }, [location.pathname]);

  /** ⌘1..N / Ctrl+1..N：跳到绑定的作用域（**纯键盘加速器**，不新增任何控件，§1.5）。 */
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      if (!/^[1-9]$/.test(event.key)) return;
      if (isTypingTarget(event.target)) return;
      const index = Number(event.key);
      let target: Scope | undefined;
      for (const [scope, bound] of shortcutPlan.bindings) {
        if (bound === index) {
          target = scope;
          break;
        }
      }
      if (!target) return; // 未绑定（降级）⇒ 按下无反应（不提示、不导航）
      event.preventDefault();
      navigate(scopeShortcutTarget(target, currentDest));
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [shortcutPlan, currentDest, navigate]);

  /**
   * `Ctrl` + 反引号键（数字 `1` 左边那枚）⇒ 跳到**聚合页签**（`/aggregate`）。
   *
   * 与 `⌘B` / `⌘K` / `⌘1..N` 同族同规则：必须带修饰键、`alt`/`shift` 一律不接、
   * 输入框内不接（§1.10）。
   *
   * ⚠️ 键位读 `AGGREGATE_SHORTCUT_CODE`（物理键，`lib/scope-shortcuts.ts`）——
   * **不许在这里硬编码字面量**：门禁断言本文件不出现该字面量，改键位只改常量。
   * `event.key` 只作兜底（少量环境不填 `code`）；不能反过来只认字符 —— 非美式布局上
   * 这枚键产出的不一定是反引号。
   * ⚠️ 已在聚合页时**直接 return**（不导航、也不 `preventDefault`）：目标就是当前页，
   * 任何动作都是噪音，也不该顺手吞掉别人的键。这里**比 `⌘1..N` 更严格** —— 后者对
   * 「已是当前作用域」会 `navigate` 同一路径，凭空 push 一条重复历史；本键不写历史。
   * ⚠️ 不受「作用域快捷键」开关控制：那个开关只作用于 `⌘1..N`，而聚合不是第 7 个
   * 作用域（见 `lib/scope-shortcuts.ts` 的说明），`⌘B` / `⌘K` 同样不受它控制。
   */
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.code !== AGGREGATE_SHORTCUT_CODE && event.key !== "`") return;
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      if (isTypingTarget(event.target)) return;
      if (bareShell) return; // 已在聚合页 ⇒ 无操作
      event.preventDefault();
      navigate("/aggregate");
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [bareShell, navigate]);

  /**
   * 切作用域（顶栏作用域菜单的唯一入口）：写「上次作用域」记忆 + 导航到
   * `/s/{scope}/{dest}`——**URL 仍是唯一真值**，记忆只决定下次冷启动落到哪。
   */
  function selectScope(next: Scope) {
    /* 从聚合页选任一作用域 = 离开聚合。目的地固定为**账号页**：
       聚合页的主任务就是账号，回到「你上次看的那个非账号页面」反而意外。 */
    if (bareShell) {
      writeLastScope(window.localStorage, next);
      navigate(`/s/${next}/accounts`);
      return;
    }
    if (next === scope) return;
    writeLastScope(window.localStorage, next);
    navigate(scopeShortcutTarget(next, currentDest));
  }

  /**
   * 刷新「当前作用域」的账号与状态（顶栏 ⟳）。
   *
   * - **WorkBuddy**：走账号 store 自己的命令（`refreshRegionStatus` + `reconcileAccounts`），
   *   与命令面板的「刷新当前作用域」同口径；
   * - **Trae / Qoder**：走 `useScopeStatus` 聚合快照的 `refreshProduct`（顶栏状态的数据源；
   *   两条产品线的快照是两个键，所以必须按产品分派）。
   *
   * ★ 返回 Promise 且**自带结果反馈**（原型 `doRefresh` 的「已刷新」toast）：
   * 成功/失败都给出显式提示，不再让请求静默跑完 —— 顶栏按钮的进行中态由它驱动
   * （`top-bar.tsx` 用 `useRefreshAction` 包装）。
   */
  async function refreshCurrentScope() {
    try {
      if (product === "workbuddy") {
        const store = useAccountsStore.getState();
        // 运行状态探测沿用「静默」口径（`refreshRegionStatus` 内部吞错，下一轮轮询会再探）；
        // 账号列表重读是本轮刷新的主判据，失败必须冒泡给下面的 toast。
        void store.refreshRegionStatus(navRegion);
        await store.reconcileAccounts(navRegion);
      } else {
        // Trae / Qoder 的刷新失败不抛错（写进快照的 `error` 字段）⇒ 用显式结果判成败。
        const aggregateError = await refreshProduct(product);
        if (aggregateError) throw new Error(aggregateError);
      }
      toast.success(t("topbar.refreshDone"), {
        description: t("topbar.refreshDoneDesc", { scope: scopeLabel(scope, t) }),
      });
    } catch (error) {
      toast.error(t("topbar.refreshFailed"), { description: api.asError(error) });
    }
  }

  return (
    <div className="flex h-screen min-h-0 flex-col overflow-hidden bg-background">
      {/* ══ 顶部应用栏（L0/L1）：作用域 + 运行态 + 搜索 + 刷新 + 主题 + 应用设置 ══ */}
      <TopBar
        scope={scope}
        statuses={statuses}
        shortcutBindings={shortcutPlan.bindings}
        onSelectScope={selectScope}
        searchLabel={t("palette.entry")}
        onOpenSearch={() => setPaletteOpen(true)}
        onRefresh={refreshCurrentScope}
        aggregateMode={bareShell}
        onSelectAggregate={() => navigate("/aggregate")}
      />

      <div className="flex min-h-0 flex-1">
        {/* ══ 侧栏（L2 目的地）：品牌 + 导航 + 版本行。不随产品换血（§3.2） ══
            ⚠️ 裸壳路由（聚合视图）下整块不渲染 —— 那 6 个目的地全是单作用域概念。 */}
        {bareShell ? null : (
        <aside
          className={cn(
            "flex min-h-0 shrink-0 flex-col border-r border-sidebar-border bg-sidebar pb-3 pt-3.5",
            /* 两态宽度：展开 240px（原型 `.side`）/ 收起 60px（34px 图标 + 两侧 13px 内边距，
               与顶栏 `.icon-btn` 的 34px 同源）。★ `transition-[width]` 只过渡宽度一项 ——
               `all` 会让 `border`、`padding` 一起动，视觉上整条侧栏在「抖」。 */
            "transition-[width] duration-200 ease-out",
            sidebarCollapsed ? "w-[60px] px-2" : "w-[240px] px-3",
          )}
          /* 收起态给读屏器一个明确信号：这一块只有图标，名称在 tooltip 与 aria-label 里。 */
          aria-label={t("product.navAria", { product: t(platformDescriptor(product).labelKey) })}
        >
          {/* 品牌块（原型 `.brand` / `.brand-text`，`docs/prototype/demo.css`）：**只有双行文案，不画应用图标**。
              issue #75：左缘原本纵向排着三枚同族圆角方块 —— Windows 原生标题栏应用图标（系统绘制）、
              顶栏平台图标（26px，唯一可交互）、这里 32px 的应用图标（纯 div）。可交互的那个最小、
              不可交互的两个更大 ⇒ 用户找「切换平台」入口时会点错。
              去掉这枚图标后左缘只剩「平台图标 + 文字」，混淆源头消失（方案 1，
              依据 `docs/prototype/sidebar-brand-design.html`）。
              ★ 折叠态**整块不渲染**：删掉图标后 60px 里没有可承载的内容（应用名在折叠态本就不可见）。
              ⚠️ 别再往这里加应用图标 —— 那正是本 issue 要根治的东西。 */}
          {sidebarCollapsed ? null : (
            <div className="flex items-center gap-2.5 px-2.5 pb-4">
              <div className="min-w-0">
                {/* 字重 650 / 字距 -.01em 对齐原型 `.brand-text b`（`docs/prototype/demo.css`）。
                    字体走 body 继承 —— 原型全站统一 `ui-sans-serif`，不另设展示字体。 */}
                <div className="truncate text-[13.5px] font-[650] leading-5 tracking-[-0.01em] text-sidebar-foreground">
                  Buddy Switch
                </div>
                {demoModeEnabled ? (
                  <Badge variant="secondary" className="mt-0.5 h-4 border-0 px-1.5 text-[10px] text-sidebar-foreground/60 shadow-none">
                    {t("app.demoBadge")}
                  </Badge>
                ) : (
                  <div className="truncate text-[10.5px] leading-4 text-muted-foreground">
                    {t("sidebar.brand.tagline")}
                  </div>
                )}
              </div>
            </div>
          )}

          <nav
            className={cn("flex min-h-0 flex-1 flex-col gap-[3px]", sidebarCollapsed && "items-center")}
            aria-label={t("product.navAria", { product: t(platformDescriptor(product).labelKey) })}
          >
            {scopedNav.map((item) => (
              <SidebarNavItem key={item.to} item={item} collapsed={sidebarCollapsed} />
            ))}
          </nav>

          {/* 侧栏底部区块：**打赏入口在版本号上方**。
              打赏在 webui 下**也显示**（版本行不显示），因此容器放在这里、
              由两个子项共用边框与上间距，而不是塞进 `AppFooter`。
              ★ 折叠态：打赏收成图标钮，版本行整行隐藏（低频信息），
              但**运行状态点保留** —— 「客户端在不在跑」是随时要瞟一眼的。 */}
          <section className={cn("mt-auto flex flex-col gap-2.5 border-t border-sidebar-border pt-3", sidebarCollapsed && "items-center gap-2")}>
            <SidebarDonate collapsed={sidebarCollapsed} />
            {api.isWebui() && !demoModeEnabled ? null : (
              sidebarCollapsed ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="grid size-6 shrink-0 place-items-center rounded-lg text-muted-foreground/70">
                      <StatusDot on={running} className="size-2" />
                    </span>
                  </TooltipTrigger>
                  <TooltipContent side="right">
                    {t(running ? "sidebar.running" : "sidebar.notRunning", {
                      product: t(platformDescriptor(product).labelKey),
                    })}
                  </TooltipContent>
                </Tooltip>
              ) : (
                <AppFooter product={product} running={running} version={appVersion} />
              )
            )}
          </section>

          {/* 折叠开关：放在侧栏**最底部**（设置项下方、版本行之下）——
              顶部品牌区不放控件（身份区），底部边缘是「抽屉把手」的位置（VS Code 同款思路）。
              收起时它自己就是「展开」按钮 ⇒ 图标随状态翻转，位置不动。 */}
          <div className={cn("mt-2.5 flex border-t border-sidebar-border pt-2.5", sidebarCollapsed && "justify-center")}>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={toggleSidebar}
                  aria-expanded={!sidebarCollapsed}
                  aria-controls="sidebar-nav"
                  title={t(sidebarCollapsed ? "sidebar.expand" : "sidebar.collapse")}
                  className={cn(
                    "h-8 gap-2 rounded-[10px] text-[12.5px] font-normal text-sidebar-foreground/70 hover:bg-foreground/[0.06] hover:text-sidebar-foreground",
                    sidebarCollapsed ? "w-8 justify-center px-0" : "w-full justify-start px-2.5",
                  )}
                >
                  {sidebarCollapsed ? <PanelLeftOpen className="size-3.5" /> : <PanelLeftClose className="size-3.5" />}
                  {sidebarCollapsed ? null : t("sidebar.collapse")}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="right">{t(sidebarCollapsed ? "sidebar.expandHint" : "sidebar.collapseHint")}</TooltipContent>
            </Tooltip>
          </div>
        </aside>
        )}

        {/* ══ 内容区（L3 视图参数）：懒加载页面 ══ */}
        <main className="min-w-0 flex-1 overflow-y-auto bg-background overscroll-contain">
          {/* P6：首次引导条（可关闭 + 持久化；内容由真实绑定派生，降级项不列）。
              ⚠️ 裸壳路由下不渲染：它的文案是「去账号页勾选多项 / 点卡片开抽屉」，
              在聚合页上指错了地方。 */}
          {bareShell ? null : <FirstRunHint />}
          {/* 懒加载页面在这里挂起。Suspense **必须留在路由层**：
              放在更外层会让侧栏一起被 fallback 替换（切页时整屏闪），
              放在页面内部则页面自己的 chunk 还没到、根本没人接住 Suspense。 */}
          <Suspense fallback={<PageFallback />}>
            <Outlet />
          </Suspense>
        </main>
      </div>

      {/* P6：命令面板（Radix Dialog + cmdk）。挂在 Layout 内 ⇒ 与顶栏入口共享开关状态。 */}
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </div>
  );
}

export default function App() {
  const Router = pagesDemoHostingEnabled ? HashRouter : BrowserRouter;
  /** 第三次启动时自动弹一次的打赏引导（关闭后不再自动出现）。 */
  const [donatePromptOpen, setDonatePromptOpen] = useState(false);

  useEffect(() => {
    /* 自动引导只在**桌面包**里数启动次数：
       - webui 每次刷新都会算一次「启动」 ⇒ 会把引导变成骚扰；
       - demo / 截图模式更不能自动弹，否则 README 截图会全部变成同一张打赏弹窗。 */
    if (demoModeEnabled || !api.isDesktop()) return;
    if (noteLaunch() < DONATE_PROMPT_AT_LAUNCH) return;
    void api
      .getAppPrefs()
      .then((prefs) => {
        // 风险确认还没通过 ⇒ 本次不弹，也**不消耗**「已提示」：下次启动还有机会。
        if (!(prefs.legal_consent_at > 0)) return;
        if (consumeDonatePrompt()) setDonatePromptOpen(true);
      })
      // 读不到偏好就安静跳过 —— 不在错误状态下再叠一张弹窗。
      .catch(() => {});
  }, []);

  return (
    <TooltipProvider delayDuration={250}>
      <Router>
        <Routes>
          <Route element={<Layout />}>
            {/* P2：作用域路由 —— `/s/{scope}/{dest}`（如 `/s/wb-global/accounts`）。
                ⚠️ 与下面的旧路由**并存**：旧路由不删 ⇒ 旧书签与外部链接继续可达（P2 验收项）。 */}
            <Route path="/s/:scope/:dest" element={<ScopedDestination />} />
            {/* 聚合账号面板（issue #50）：独立路由、与 `/s/:scope/:dest` 并列。
                ⚠️ **不进 `SCOPED_ROUTES`** —— 门禁断言「每个 dest 在作用域路由里恰好 3 次」，
                把聚合塞进去会红；而它本来也不是一个 dest（dest 是单作用域内的页面）。 */}
            <Route path="/aggregate" element={<AggregateAccountsPage />} />
            <Route path="/overview" element={<OverviewPage />} />
            <Route path="/" element={<AccountsPage />} />
            <Route path="/credit-stats" element={<UsagePage defaultTab="credits" />} />
            <Route path="/token-stats" element={<UsagePage />} />
            <Route path="/api-service" element={<ApiServicePage />} />
            <Route path="/settings" element={<SettingsPage />} />
            {/* Trae 侧与 WorkBuddy 侧逐条同构；`/trae` 本身重定向到账号管理，
                避免旧书签或外部链接落在空路由上。 */}
            <Route path="/trae" element={<Navigate to="/trae/accounts" replace />} />
            <Route path="/trae/accounts" element={<TraeAccountsPage />} />
            <Route path="/trae/token-stats" element={<UsagePage />} />
            <Route path="/trae/credits" element={<UsagePage defaultTab="credits" />} />
            <Route path="/trae/api-service" element={<TraeApiServicePage />} />
            <Route path="/trae/settings" element={<TraeSettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
        <Toaster />
      </Router>
      {/* 首启风险确认：全屏模态，只在桌面包且 `legal_consent_at == 0` 时出现。
          放在 Router 之外 —— 它不依赖路由，也不该因导航而重挂载。 */}
      <LegalConsentDialog />
      {/* 第三次启动自动弹一次打赏引导（含 Star 入口）；每台设备只自动弹一次，
          节奏与去重见 `lib/donate-prompt.ts`。 */}
      <DonateDialog open={donatePromptOpen} onOpenChange={setDonatePromptOpen} />
    </TooltipProvider>
  );
}
