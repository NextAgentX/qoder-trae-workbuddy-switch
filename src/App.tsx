import { Suspense, lazy, useEffect, useMemo, useState, type ComponentType, type ElementType } from "react";
import { BrowserRouter, HashRouter, Navigate, NavLink, Outlet, Route, Routes, useLocation, useNavigate, useParams } from "react-router-dom";
import { ArrowUp, GitBranch, LayoutDashboard, MessagesSquare, Monitor, Server, Settings, User } from "lucide-react";

import { cn } from "@/lib/utils";
import * as api from "@/lib/api";
import type { UpdateInfo } from "@/lib/types";
import { PageFallback } from "@/components/page-fallback";
import { AppIconMark, StatusDot } from "@/components/product-marks";
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
import type { TranslationKey } from "@/locales/zh";
import { planScopeShortcuts, scopeShortcutTarget } from "@/lib/scope-shortcuts";
import { useShortcutsEnabled } from "@/lib/shortcut-preference";
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
import { DESTINATION_CAPABILITY, type Destination, DESTINATIONS, destinationOfLegacyPath, hasCapability, platformDescriptor, type Product, platformOfPath } from "@/lib/platforms/registry";
import { parseScope, type Scope, SCOPES, scopeOf, scopeParts, writeLastScope } from "@/lib/scope";
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
 * 侧栏导航项（原型 `.nav-item`，`buddy-switch-prototype.html:166-175`）—— 逐值对齐：
 * 高 38px、内边距 `0 10px`、圆角 10px、字号 13px；悬停 `foreground 6%`；
 * **选中 `primary 16%` + 图标转 primary + 字重 550**（原型还额外强调图标，不只是背景）。
 */
function navLinkClass({ isActive }: { isActive: boolean }): string {
  return cn(
    "group flex h-[38px] items-center gap-2.5 rounded-[10px] px-2.5 text-[13px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-sidebar-ring/50",
    isActive
      ? "bg-primary/15 font-[550] text-foreground [&>svg]:text-primary"
      : "text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground",
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
 * 侧栏底部的应用信息。
 *
 * 这里展示的是**本应用**（Buddy Switch）的版本，而不是所管理客户端的版本：
 * `status.version` 来自 `update::APP_VERSION`，此前挂在「WorkBuddy」名下会让人
 * 误以为它是 WorkBuddy 客户端的版本号。状态圆点跟随当前选中的产品。
 *
 * **自带容器**：`.sidebar-foot`（顶部边框 + 上间距）在本组件内 —— 侧栏底部现在
 * 只有它一项（应用级入口已收进顶栏 ⚙ 菜单，见 `AppMenu`），容器随它一起走。
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
  const productLabel = t(platformDescriptor(product).labelKey);

  return (
    <>
      <div className="mt-auto border-t border-sidebar-border pt-2.5">
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
  /** 四个作用域的环境状态（顶栏作用域菜单的唯一取数来源）。 */
  const { statuses, refreshTrae } = useScopeStatus();

  /* P6：命令面板（⌘K）的开关状态。入口有两个 —— 顶栏搜索按钮与全局 ⌘K。
     状态放在 `Layout`（而非 `App`）里，是为了与顶栏入口共享、且都处于 Router 上下文内。 */
  const [paletteOpen, setPaletteOpen] = useState(false);

  /* P6：作用域快捷键规划。降级（>5 个作用域或用户关闭）⇒ 空绑定 ⇒ 序号提示同步消失。 */
  const [shortcutsEnabled] = useShortcutsEnabled();
  const shortcutPlan = useMemo(
    () => planScopeShortcuts({ scopes: SCOPES, enabled: shortcutsEnabled }),
    [shortcutsEnabled],
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
   * 切作用域（顶栏作用域菜单的唯一入口）：写「上次作用域」记忆 + 导航到
   * `/s/{scope}/{dest}`——**URL 仍是唯一真值**，记忆只决定下次冷启动落到哪。
   */
  function selectScope(next: Scope) {
    if (next === scope) return;
    writeLastScope(window.localStorage, next);
    navigate(scopeShortcutTarget(next, currentDest));
  }

  /**
   * 刷新「当前作用域」的账号与状态（顶栏 ⟳）。
   *
   * - **WorkBuddy**：走账号 store 自己的命令（`refreshRegionStatus` + `reconcileAccounts`），
   *   与命令面板的「刷新当前作用域」同口径；
   * - **Trae**：走 `useScopeStatus` 聚合快照的 `refreshTrae`（顶栏状态的数据源）。
   */
  function refreshCurrentScope() {
    if (product === "workbuddy") {
      const store = useAccountsStore.getState();
      void store.refreshRegionStatus(navRegion);
      void store.reconcileAccounts(navRegion).catch(() => undefined);
    } else {
      void refreshTrae();
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
      />

      <div className="flex min-h-0 flex-1">
        {/* ══ 侧栏（L2 目的地）：品牌 + 导航 + 版本行。不随产品换血（§3.2） ══ */}
        <aside className="flex min-h-0 w-[240px] shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-3 pb-3 pt-3.5">
          {/* 品牌块（原型 `.brand` / `.brand-mark`，`:155-163`）：32px 图标底 + 双行文案。
              ⚠️ 图标必须带 `primary 18%` 底色 —— 原型不是「裸图标」。 */}
          <div className="flex items-center gap-2.5 px-1.5 pb-4">
            <span className="grid size-8 shrink-0 place-items-center rounded-[10px] bg-primary/15 text-primary">
              <AppIconMark size={17} />
            </span>
            <div className="min-w-0">
              <div
                className="truncate text-[13.5px] leading-5 tracking-[-0.01em] text-sidebar-foreground"
                style={{
                  fontFamily: '"Bricolage Grotesque Variable", "SF Pro Display", ui-sans-serif, sans-serif',
                  fontWeight: 650,
                }}
              >
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

          <nav
            className="flex min-h-0 flex-1 flex-col gap-[3px]"
            aria-label={t("product.navAria", { product: t(platformDescriptor(product).labelKey) })}
          >
            {scopedNav.map((item) => (
              <NavLink key={item.to} to={item.to} className={navLinkClass}>
                <item.icon className="size-4" />
                {t(item.labelKey)}
                {/* ★ 侧栏徽章（原型 `renderNav`）：账号数 / 网关密钥数 / 会话待同步数。
                  一律 **`>0` 才渲染**（不摆「0」，§3.4 规则 1）；`warn` ⇒ 用警示色
                  （账号「有需重登」、会话「有待同步」—— 都是「有事要处理」）。 */}
                {item.badge !== null ? (
                  /* 徽章逐值对齐原型 `.nav-badge`（`:176-182`）：min-w 20 / 高 18 / 内边距 0 6 /
                     圆角 999（**胶囊**，不是方角）/ 字号 10.5。 */
                  <span
                    data-badge={item.dest === "sessions" ? "pending-sync" : `count-${item.dest}`}
                    className={cn(
                      "ml-auto grid h-[18px] min-w-[20px] place-items-center rounded-full px-1.5 text-[10.5px] tabular-nums",
                      item.badge.warn
                        ? "bg-warning/20 text-warning"
                        : "bg-muted text-muted-foreground",
                    )}
                  >
                    {item.badge.n}
                  </span>
                ) : null}
              </NavLink>
            ))}
          </nav>

          {/* 侧栏底部只剩版本行 —— 应用级入口（通用设置 / 客户端设置 / Skill / 打赏）
              已收进顶栏 ⚙ 菜单（`AppMenu`），不再占侧栏固定高度。 */}
          {api.isWebui() && !demoModeEnabled ? null : (
            <AppFooter product={product} running={running} version={appVersion} />
          )}
        </aside>

        {/* ══ 内容区（L3 视图参数）：懒加载页面 ══ */}
        <main className="min-w-0 flex-1 overflow-y-auto bg-background overscroll-contain">
          {/* P6：首次引导条（可关闭 + 持久化；内容由真实绑定派生，降级项不列）。 */}
          <FirstRunHint />
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
