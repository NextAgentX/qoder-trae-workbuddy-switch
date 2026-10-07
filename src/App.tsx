import { Suspense, lazy, useEffect, useMemo, useRef, useState, type ComponentType, type ElementType } from "react";
import { BrowserRouter, HashRouter, Navigate, NavLink, Outlet, Route, Routes, useLocation, useNavigate, useParams } from "react-router-dom";
import { ArrowUp, GitBranch, LayoutDashboard, MessagesSquare, Monitor, Search, Server, Settings, User } from "lucide-react";

import { cn } from "@/lib/utils";
import * as api from "@/lib/api";
import type { UpdateInfo } from "@/lib/types";
import { PageFallback } from "@/components/page-fallback";
import { StatusDot, AppIconMark, TraeVariantMark, WorkBuddyMark } from "@/components/product-marks";
import { DonateButton } from "@/components/donate-dialog";
import { AppSettingsEntry } from "@/components/app-settings";
import { UpdateInstallDialog } from "@/components/update-install-dialog";
import { CommandPalette } from "@/components/command-palette/command-palette";
import { FirstRunHint } from "@/components/first-run-hint";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/sonner";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { demoModeEnabled, pagesDemoHostingEnabled } from "@/lib/demo-mode";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import { planScopeShortcuts, scopeShortcutTarget } from "@/lib/scope-shortcuts";
import { useShortcutsEnabled } from "@/lib/shortcut-preference";
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
const ApiServicePage = lazy(() => import("@/pages/ApiServicePage"));
const CreditStatsPage = lazy(() => import("@/pages/CreditStatsPage"));
const TokenStatsPage = lazy(() => import("@/pages/TokenStatsPage"));
const SettingsPage = lazy(() => import("@/pages/SettingsPage"));
const OverviewPage = lazy(() => import("@/pages/OverviewPage"));
const UsagePage = lazy(() => import("@/pages/UsagePage"));
const TraeAccountsPage = lazy(() => import("@/pages/TraeAccountsPage"));
const TraeApiServicePage = lazy(() => import("@/pages/TraeApiServicePage"));
const TraeCreditsPage = lazy(() => import("@/pages/TraeCreditsPage"));
const TraeSettingsPage = lazy(() => import("@/pages/TraeSettingsPage"));
const TraeTokenStatsPage = lazy(() => import("@/pages/TraeTokenStatsPage"));
/* P8：第 6 目的地「会话」。两平台各一个组件 —— WorkBuddy 侧是工作台骨架（`SessionsPage`），
   Trae 侧是**结构化空态**（`TraeSessionsPage`，说明「暂不支持会话管理」+ 原因）。
   ⚠️ Trae 侧**不渲染入口**（能力门控），但 URL 直达仍可达 ⇒ 仍需一个组件接住。 */
const SessionsPage = lazy(() => import("@/pages/SessionsPage"));
const TraeSessionsPage = lazy(() => import("@/pages/TraeSessionsPage"));
/* P0 收敛清单第 1 项：`Product` 的类型定义**已搬进注册表**，这里改为 import。
   （原来 App.tsx 里有一份局部 `type Product`，两处重复 —— 现在只剩一处。） */
import { DESTINATION_CAPABILITY, type Destination, DESTINATIONS, destinationOfLegacyPath, hasCapability, type Product, platformOfPath } from "@/lib/platforms/registry";
import { REGIONS, regionDescriptor } from "@/lib/region";
import { parseScope, type Scope, SCOPES, scopeOf, scopeParts } from "@/lib/scope";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useScope } from "@/lib/use-scope";
import { useTraeVariant } from "@/lib/use-trae-variant";
import { useCreditAutoRefresh } from "@/lib/use-credit-auto-refresh";
import { useWorkbuddyStatusRefresh } from "@/lib/use-workbuddy-status-refresh";
import { useAccountsStore } from "@/stores/accounts";
import { useSessionsStore } from "@/stores/sessions";

/**
 * 产品分区。
 *
 * WorkBuddy 与 Trae 是两套完全独立的体系（独立账号库、独立客户端、独立配置），
 * 侧栏一次只展示其中一个产品的导航与页面，由顶部的产品 Tab 决定。
 *
 * ## ★ Trae 只占**一个**侧栏分区，两条产品线在分区内部切换
 *
 * `TRAE SOLO CN`（界面名 `Trae Work`）与 `Trae CN` 确实可以同机并存、各有独立
 * 安装目录与账号库，但**这不意味着侧栏要排三个 Tab**：
 *
 * - 侧栏的职责是「选产品」，而 Trae 的两条线**共用全部页面与路由**
 *   （`/trae/accounts` 等）。把它们拆成两个顶级分区，等于让同一组页面
 *   在侧栏出现两遍，用户要在两个看起来一样的入口之间做无意义的二选一。
 * - 变体是**数据维度**而非**页面维度**，因此正确的载体是 Trae 页面内部的
 *   产品线切换器（见 `TraeAccountsPage` 的变体 Tab），而不是侧栏。
 * - 侧栏 220px 宽度下三个 Tab 连产品名都排不下（详见 `ProductSwitch` 的注释），
 *   压成纯图标后两条 Trae 又几乎无法区分 —— 这本身就是「不该有三个 Tab」的信号。
 *
 * 变体仍然由 URL 的 `?line=` 承载（`useTraeVariant`）：它决定 Trae 分区内部
 * 看哪条线，并且刷新 / 分享 / 前进后退都能保持。
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

/**
 * 产品显示名对应的**文案键**（侧栏顶部产品切换器与导航无障碍标签的唯一来源）。
 *
 * 存键而不存成品文案：这是模块级常量，拿不到 `useT()`。渲染处 `t(PRODUCT_LABEL_KEY[…])`
 * 一秒翻译一次，语言切换才会反映到侧栏 —— 存成中文会让侧栏永远停在启动时的语言。
 *
 * Trae 分区显示 **`TraeWork`**：它管的就是 TraeWork 这条产品线
 * （国内版 `TRAE SOLO CN` 与国际版 `TRAE SOLO`），名字与客户端
 * `product.json` 的 `nameAlias`（`TraeWork CN` / `TraeWork`）及系统注册表显示名一致。
 *
 * 但**区域（国内版 / 国际版）仍然不写进侧栏**：它由分区内部顶部的区域切换器表达。
 * 把区域名塞进侧栏会让「国内版 / 国际版」看起来像两个独立产品，
 * 而它们共用同一组页面、同一套账号管理方式。同理，程序线（TraeWork / TraeCode）
 * 是**账号卡片上的一枚按钮**，也不进侧栏。
 *
 * 注意 `Product` 的**标识**仍是 `trae`（路由前缀 `/trae/...` 与 `PRODUCT_NAV` 的键都不动）：
 * 本次只改展示名，改标识会牵动路由表与全部 `/trae` 链接。
 */
const PRODUCT_LABEL_KEY: Record<Product, TranslationKey> = {
  // 产品名**刻意不翻译**：`WorkBuddy` / `TraeWork` 是商标，两种语言下写法相同，
  // 因此这两个键的 zh/en 值一致。走词表而非硬编码，是为了让「语言切换后侧栏整体重渲染」
  // 这件事在所有文案上保持一致行为，不给未来的改名留特例。
  workbuddy: "product.workbuddy",
  trae: "product.trae",
};

/** 产品首页：切到某产品时，若当前路由不属于它，就落到这里。 */
const PRODUCT_HOME: Record<Product, string> = {
  workbuddy: "/",
  trae: "/trae/accounts",
};

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

function navLinkClass({ isActive }: { isActive: boolean }): string {
  return cn(
    "flex items-center gap-2.5 rounded-lg px-3 py-2.5 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-sidebar-ring/50",
    isActive
      ? "bg-foreground/[0.06] font-medium text-foreground"
      : "text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground",
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
    const timer = window.setInterval(() => void refresh(), 60 * 1000);
    return () => window.clearInterval(timer);
  }, [active, refresh]);

  return useMemo(
    () => Object.fromEntries((data ?? []).map((item) => [item.variant, item.running])),
    [data],
  );
}

/**
 * 侧栏顶部的**作用域选择器**（P2）—— 区域轴。
 *
 * ## 为什么上移到这里（§3.1）
 *
 * 改造前「切区域」的入口散在**各页面内部**：账号页 `VariantSwitch`、统计页 `RegionBar`、
 * API 页 `WorkbuddyRegionSwitch`。同一个操作在三处出现、各自实现、各自记状态
 * ⇒ 「账号页切国际版、进统计页又变回国内版」。
 *
 * 现在上移到**侧栏标题下方**（`ProductSwitch` 之下），配合 `useScope` 的
 * 「URL 为唯一真值」：切一次，**整页跟随**。
 *
 * ⚠️ **上移之后，页面内不应再有区域选择器** —— 两个「看起来能切」的控件会让用户
 * 不确定哪个生效（§3.1 纪律）。移除页面内的是后续步骤。
 *
 * ⚠️ **只给 WorkBuddy**：Trae 的区域由它自己的 `?line=` 承载（那里还要同时表达
 * **程序位**，语义比单区域复杂）。在 P2 阶段不去动它 —— 等 P3 合并页面时统一。
 *
 * ⚠️ **macOS 避让**：本组件在侧栏**内部**，而侧栏已由 `hasUnifiedTitleBar` 加了 `pt-20`
 * 为红绿灯让位，因此这里不需要额外处理；但**不能把它提到侧栏之外**，否则会撞上红绿灯区。
 */
function ScopeSwitch({ product }: { product: Product }) {
  const [region, setRegion] = useScope(product);
  return (
    <div
      className="mb-4 flex gap-1 rounded-lg bg-sidebar-accent/40 p-1"
      role="tablist"
      aria-label="区域"
    >
      {REGIONS.map((r) => (
        <button
          key={r}
          type="button"
          role="tab"
          aria-selected={region === r}
          onClick={() => setRegion(r)}
          className={cn(
            "flex-1 truncate rounded-md px-2 py-1 text-[11.5px] transition-colors",
            region === r
              ? "bg-background text-foreground shadow-sm"
              : "text-sidebar-foreground/60 hover:text-sidebar-foreground",
          )}
        >
          {regionDescriptor(r).versionLabel}
        </button>
      ))}
    </div>
  );
}

/** 侧栏顶部的产品切换：下方的导航与主区域页面都跟随它。 */
function ProductSwitch({
  product,
  onChange,
}: {
  product: Product;
  onChange: (next: Product) => void;
}) {
  const t = useT();

  /*
   * 两个产品 Tab：**图标 + 文字**。
   *
   * ## 为什么现在是两个而不是三个
   *
   * 曾经这里是三个 Tab（WorkBuddy / Trae Work / Trae CN），因为两条 Trae 产品线
   * 可以同机并存。但侧栏固定 220px、扣掉 `px-3` 后 tablist 只有 195px，
   * 三等分每格约 60px，而 12px 字号下 `WorkBuddy` = 65px、`Trae Work` = 56px、
   * `Trae CN` = 43px —— **三个全都放不下**。当时的妥协是压成纯图标 + Tooltip。
   *
   * 但纯图标本身就是一个更强的信号：**用户分不清两个几乎一样的 Trae 图标该点哪个**。
   * 两条线共用全部页面与路由，拆成两个顶级入口本来就多一层无意义的二选一，
   * 因此合并成一个 `Trae`。产品线的选择下沉到 Trae 页面内部（那里宽度充足，
   * 可以完整展示 `Trae Work` / `Trae CN` 并带各自的运行状态）。
   *
   * ## 为什么两个 Tab 也**不能等分**（浏览器实测，别再改回 `grid-cols-2`）
   *
   * 合并成两个后，`grid-cols-2` 的等分**仍然放不下** `WorkBuddy`：
   * tablist 外框 195px、扣 `p-1` 后内容区 187px、`gap-0.5` 占 2px ⇒ 每格 92px；
   * 再减 `px-2`（16px）+ 图标 15px + `gap-1.5`（6px），**留给文字的只有 55px**，
   * 而 12px/500 的 `WorkBuddy` 实测 `scrollWidth = 64px` —— 短 9px，被截成 `WorkBu…`。
   *
   * 因此改成**按内容自适应并居中**（`grid-cols-[auto_auto]` + `justify-center`）。
   * **实测（2026-09-21，1333×900，`px-1.5`）**：`WorkBuddy` 格 97px、`TraeWork` 格 85px，
   * 加 2px 间隙共 184px ≤ 内容区 193px，**余量 9px**；两个标签的
   * `scrollWidth == clientWidth`（64/64、52/52）⇒ **均未被截断**。
   *
   * 刻意**不**用「缩字号 / 压 padding 硬塞进等分格」：要等分放下 `WorkBuddy`，
   * `padX + gap` 只能有 13px（12px 字号）或 16px（11px 字号），余量仅 1~3px；
   * 换 DPI / 字体回退时会再次截断，不可靠。按内容排布不依赖这点余量。
   *
   * label 上的 `truncate` 保留，作为将来产品名变长时的兜底（当前不触发）。
   *
   * 两个图标仍然如实反映「这是两个独立体系」——这正是 WorkBuddy 侧的做法。
   * 注意第二格的标签是 **`TraeWork`**（本分区管的产品线），而它内部的**区域**
   * （国内版 / 国际版）与**程序位**（TraeWork / TraeCode）都在页面里选，见
   * `useTraeVariant` 与账号页的状态条 —— 侧栏只表达「进哪个产品分区」。
   */
  return (
    <Tabs
      value={product}
      onValueChange={(value) => onChange(value as Product)}
      className="mb-3 shrink-0"
    >
      <TabsList
        className="grid h-9 w-full grid-cols-[auto_auto] justify-center gap-0.5 rounded-xl border border-sidebar-border bg-sidebar-accent/60 p-1"
        aria-label={t("product.switchAria")}
      >
        <TabsTrigger
          value="workbuddy"
          className="h-7 w-full min-w-0 gap-1.5 rounded-lg px-1.5 text-xs font-medium data-[state=active]:bg-primary/15 data-[state=active]:shadow-none"
        >
          <WorkBuddyMark size={15} />
          <span className="truncate">{t(PRODUCT_LABEL_KEY.workbuddy)}</span>
        </TabsTrigger>
        <TabsTrigger
          value="trae"
          className="h-7 w-full min-w-0 gap-1.5 rounded-lg px-1.5 text-xs font-medium data-[state=active]:bg-primary/15 data-[state=active]:shadow-none"
        >
          {/* 两个产品的图标各自如实呈现；Trae 分区用 TraeWork 的图标
              （这里不区分区域与程序位，都在页面内部选）。 */}
          <TraeVariantMark variant="trae_work" size={15} />
          <span className="truncate">{t(PRODUCT_LABEL_KEY.trae)}</span>
        </TabsTrigger>
      </TabsList>
    </Tabs>
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
          className={cn(
            "size-5 shrink-0 rounded-md p-0",
            visible
              ? "text-sidebar-foreground"
              : "text-sidebar-foreground/40 hover:text-sidebar-foreground/70",
          )}
          aria-label={label}
          aria-pressed={visible}
          onClick={() => setVisible(!visible)}
        >
          <Monitor className="size-3" aria-hidden="true" />
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * 侧栏底部的应用信息。
 *
 * 这里展示的是**本应用**（Buddy Switch）的版本，而不是所管理客户端的版本：
 * `status.version` 来自 `update::APP_VERSION`，此前挂在「WorkBuddy」名下会让人
 * 误以为它是 WorkBuddy 客户端的版本号。状态圆点跟随当前选中的产品。
 *
 * **只负责内容**：底部区块的边框、外边距与水平内边距归 `Layout` 里的那个
 * `<section>`——同一块里还要放打赏入口（见 `DonateButton`），而它在 webui 下
 * **仍然显示**（版本行则不显示），把容器留在 `Layout` 才能只写一处。
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

  useEffect(() => {
    let disposed = false;

    async function checkForUpdate() {
      try {
        const result = await api.checkUpdate();
        if (!disposed) setInfo(result.ok ? result : null);
      } catch {
        // 左下角只展示可操作的升级状态，网络错误不打扰正常使用。
      }
    }

    void checkForUpdate();
    const timer = window.setInterval(() => void checkForUpdate(), 30 * 60 * 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, []);

  const hasUpdate = Boolean(info?.ok && info.hasUpdate && info.latest);
  const productLabel = t(PRODUCT_LABEL_KEY[product]);

  return (
    <>
      <div className="flex items-center gap-2 text-[13px] text-sidebar-foreground">
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
        <span className="min-w-0 flex-1 truncate">{t("sidebar.version")}</span>
        <div className="flex shrink-0 items-center gap-1.5">
          {/* 这里固定显示「版本」二字而非产品名：产品名已在侧栏顶部 Tab 与标题栏出现，
              重复一遍反而挤占了版本号的位置。版本号用更小字号并保持 tabular-nums，
              让数字在版本号变化时纵向对齐、不跳动。 */}
          <span className="shrink-0 text-[11px] tabular-nums text-sidebar-foreground/50">v{version || "?"}</span>
          {/* P9 T04：会话悬浮窗入口（桌面端；与原型版本行的 `.mini-btn` 同位）。
              它是**可见性开关**，与托盘 / 窗口内关闭按钮收敛到同一份偏好 —— 不新增第二个切换器。 */}
          {api.isDesktop() ? <CompanionToggle /> : null}
          {hasUpdate && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  size="icon"
                  className="size-5 rounded-full p-0"
                  aria-label={t("sidebar.update")}
                  onClick={() => setDialogOpen(true)}
                >
                  <ArrowUp className="size-3" strokeWidth={2.5} aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top">{t("sidebar.update")}</TooltipContent>
            </Tooltip>
          )}
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

  /** 每个产品各自记住上次停留的页面，来回切换不会丢上下文。 */
  const lastPathRef = useRef<Record<Product, string>>({ ...PRODUCT_HOME });

  /**
   * 当前的产品分区。
   *
   * 路径只能回答「是 WorkBuddy 还是 Trae」，回答不了「Trae 里是看哪条产品线」——
   * 两条线共用 `/trae/...`。因此变体由 URL 里的 `?line=` 查询参数承载，
   * 但它**不再是分区维度**：它决定 Trae 页面内部展示哪条线，
   * 由 `TraeAccountsPage` 等页面自己读取（见 `useTraeVariant`）。
   */
  const [traeVariant, setTraeVariant] = useTraeVariant();
  const product: Product = productFromPath(location.pathname);

  /* P6：命令面板（⌘K）的开关状态。入口有两个 —— 侧栏「搜索或跳转」按钮与全局 ⌘K。
     状态放在 `Layout`（而非 `App`）里，是为了与侧栏入口共享、且都处于 Router 上下文内。 */
  const [paletteOpen, setPaletteOpen] = useState(false);

  /* P6：作用域快捷键规划。降级（>5 个作用域或用户关闭）⇒ 空绑定 ⇒ 序号提示同步消失。 */
  const [shortcutsEnabled] = useShortcutsEnabled();
  const shortcutPlan = useMemo(
    () => planScopeShortcuts({ scopes: SCOPES, enabled: shortcutsEnabled }),
    [shortcutsEnabled],
  );

  /* P2：**侧栏由注册表生成** —— 平台之间只差 scope（§3.1「侧栏固定 6 项」）。
     改造前是两套硬编码的 `PRODUCT_NAV`（5:5），新增目的地要改两处。
     ⚠️ scope 取「当前区域」，所以切区域时侧栏链接**跟着变**（URL 成为唯一真值）。
     P8：入口先按**能力门控**过滤（`DESTINATION_CAPABILITY` + `hasCapability`）——
     缺能力的平台**不渲染**该入口（Trae 无 `sessionCopy` ⇒ 无「会话」），
     但**不封路由**：URL 直达仍由页面自己给空态（§3.2 规则 2）。 */
  const [navRegion] = useScope(product);
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
  const scopedNav = useMemo(
    () =>
      DESTINATIONS.filter((dest) => {
        const capability = DESTINATION_CAPABILITY[dest];
        return capability === undefined || hasCapability(product, capability);
      }).map((dest) => ({
        to: `/s/${scopeOf(product, navRegion)}/${dest}`,
        labelKey: DEST_META[dest].labelKey,
        icon: DEST_META[dest].icon,
        // ★ P8：仅「会话」条目带待同步徽章；**>0 才带**（`null` = 不渲染）。
        badge: dest === "sessions" && pendingSyncCount > 0 ? pendingSyncCount : null,
      })),
    [product, navRegion, pendingSyncCount],
  );

  const appVersion = useAccountsStore((s) => s.status?.version || s.global.status?.version);
  const workbuddyRunning = useAccountsStore((s) =>
    Boolean(s.status?.running || s.global.status?.running),
  );
  const traeRunning = useTraeVariantRunning(product === "trae");
  // Trae 分区的状态圆点跟随**当前选中的那条产品线**（而不是「Trae 是否有任意一条在跑」）：
  // 在「Trae Work 已关闭、Trae CN 在运行」时，只探「Trae 是否运行」会显示错误的绿灯。
  const running =
    product === "workbuddy" ? workbuddyRunning : Boolean(traeRunning[traeVariant]);

  const hasUnifiedTitleBar =
    api.isDesktop() && typeof navigator !== "undefined" && navigator.userAgent.includes("Macintosh");
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

  useEffect(() => {
    lastPathRef.current[product] = location.pathname;
  }, [product, location.pathname]);

  function switchProduct(next: Product) {
    if (next === product) return;
    // Trae 内部的产品线由 `?line=` 保持，切走再切回来时**不应该被重置**——
    // 用户上次看的是 Trae CN，回来时就还该是 Trae CN。这里的 `setTraeVariant`
    // 只在切**向** Trae 且 URL 尚无该参数时兜底（默认变体不写进 URL，
    // 因此这一步通常是空操作，保留它是为了「切走时 URL 上残留了别的产品的参数」
    // 这类边界不会让变体漂到错误的值）。
    if (next === "trae") setTraeVariant(traeVariant);
    navigate(lastPathRef.current[next] || PRODUCT_HOME[next]);
  }

  return (
    <div className="flex h-screen min-h-0 overflow-hidden bg-background">
      {hasUnifiedTitleBar ? (
        <div
          data-tauri-drag-region
          className="fixed inset-x-0 top-0 z-50 h-8"
          aria-hidden="true"
        />
      ) : null}
      <aside
        className={cn(
          "flex min-h-0 w-[220px] shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-3 pb-4",
          hasUnifiedTitleBar ? "pt-20" : "pt-4",
        )}
      >
        <div className="flex items-center gap-2.5 px-1 pb-5">
          <AppIconMark size={36} className="drop-shadow-sm" />
          <div className="min-w-0">
            <div
              className="truncate text-[15px] leading-5 tracking-[-0.02em] text-sidebar-foreground/90"
              style={{
                fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif',
                fontWeight: 640,
              }}
            >
              Buddy Switch
            </div>
            {demoModeEnabled && (
              <Badge variant="secondary" className="mt-1 h-5 border-0 px-1.5 text-[10px] text-sidebar-foreground/60 shadow-none">
                {t("app.demoBadge")}
              </Badge>
            )}
          </div>
        </div>

        <ProductSwitch product={product} onChange={switchProduct} />
        {/* P2：作用域选择器上移到侧栏标题下方（§3.1）。只给 WorkBuddy ——
            Trae 的区域由它自己的 `?line=` 承载（那里还带程序位维度）。 */}
        {product === "workbuddy" ? <ScopeSwitch product={product} /> : null}

        {/* P6：命令面板入口。它是**搜索/跳转**按钮（不是作用域切换器）——
            切作用域的控件仍只有上方的 ProductSwitch / ScopeSwitch（§1.5 纪律）。 */}
        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          className="mb-2 flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground outline-none transition-colors hover:bg-foreground/[0.04] hover:text-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring/50"
        >
          <Search className="size-4 shrink-0" aria-hidden="true" />
          <span className="min-w-0 flex-1 truncate text-left">{t("palette.entry")}</span>
          <kbd className="shrink-0 rounded border border-sidebar-border px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground/70">
            ⌘K
          </kbd>
        </button>

        <nav
          className="flex min-h-0 flex-1 flex-col gap-0.5"
          aria-label={t("product.navAria", { product: t(PRODUCT_LABEL_KEY[product]) })}
        >
          {scopedNav.map((item) => (
            <NavLink key={item.to} to={item.to} className={navLinkClass}>
              <item.icon className="size-4" />
              {t(item.labelKey)}
              {/* ★ P8：待同步组数（`behind` + `diverge`）—— **>0 才渲染**（不摆「0」）。 */}
              {item.badge !== null ? (
                <Badge
                  variant="secondary"
                  className="ml-auto h-5 border-0 px-1.5 text-[10px] tabular-nums shadow-none"
                  data-badge="pending-sync"
                >
                  {item.badge}
                </Badge>
              ) : null}
            </NavLink>
          ))}
        </nav>

        {/* 侧栏底部区块：**通用设置**入口紧贴**版本号上方**，打赏在其上。
            选择这一位置的依据是「这里是侧栏唯一不随产品切换的区域」，因此**应用级设置**
            （外观 / 开机自启 / 自动更新）的入口固定在此：两个产品分区走同一份实现，
            见 `AppSettingsEntry`。它也不属于 `PRODUCT_NAV`，所以不会把
            「侧栏 5 项 / 路由 5:5」的既有约束变成 6:6。
            打赏在 webui 下**也显示**（版本行不显示），因此容器放在这里、
            由各子项共用边框与内边距，而不是塞进 `AppFooter`。 */}
        <section className="mt-auto flex flex-col gap-2.5 border-t border-sidebar-border px-2 pt-3 text-xs">
          <DonateButton />
          <AppSettingsEntry />
          {api.isWebui() && !demoModeEnabled ? null : (
            <AppFooter product={product} running={running} version={appVersion} />
          )}
        </section>
      </aside>
      <main
        className={cn(
          "min-w-0 flex-1 overflow-y-auto bg-background overscroll-contain",
          hasUnifiedTitleBar && "pt-16 [&>div]:pt-4",
        )}
      >
        {/* P6：首次引导条（可关闭 + 持久化；内容由真实绑定派生，降级项不列）。 */}
        <FirstRunHint />
        {/* 懒加载页面在这里挂起。Suspense **必须留在路由层**：
            放在更外层会让侧栏一起被 fallback 替换（切页时整屏闪），
            放在页面内部则页面自己的 chunk 还没到、根本没人接住 Suspense。 */}
        <Suspense fallback={<PageFallback />}>
          <Outlet />
        </Suspense>
      </main>

      {/* P6：命令面板（Radix Dialog + cmdk）。挂在 Layout 内 ⇒ 与侧栏入口共享开关状态。 */}
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </div>
  );
}

export default function App() {
  const Router = pagesDemoHostingEnabled ? HashRouter : BrowserRouter;

  return (
    <TooltipProvider delayDuration={250}>
      <Router>
        <Routes>
          <Route element={<Layout />}>
            {/* P2：作用域路由 —— `/s/{scope}/{dest}`（如 `/s/wb-global/accounts`）。
                ⚠️ 与下面的旧路由**并存**：旧路由不删 ⇒ 旧书签与外部链接继续可达（P2 验收项）。 */}
            <Route path="/s/:scope/:dest" element={<ScopedDestination />} />
            <Route path="/overview" element={<OverviewPage />} />
            <Route path="/" element={<AccountsPage />} />
            <Route path="/credit-stats" element={<CreditStatsPage />} />
            <Route path="/token-stats" element={<TokenStatsPage />} />
            <Route path="/api-service" element={<ApiServicePage />} />
            <Route path="/settings" element={<SettingsPage />} />
            {/* Trae 侧与 WorkBuddy 侧逐条同构；`/trae` 本身重定向到账号管理，
                避免旧书签或外部链接落在空路由上。 */}
            <Route path="/trae" element={<Navigate to="/trae/accounts" replace />} />
            <Route path="/trae/accounts" element={<TraeAccountsPage />} />
            <Route path="/trae/token-stats" element={<TraeTokenStatsPage />} />
            <Route path="/trae/credits" element={<TraeCreditsPage />} />
            <Route path="/trae/api-service" element={<TraeApiServicePage />} />
            <Route path="/trae/settings" element={<TraeSettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
        <Toaster />
      </Router>
    </TooltipProvider>
  );
}
