/**
 * 「用量」工作台（PLAN §3.9 / **P3**）—— **一级四视图**：趋势 / 模型 / 会话 / 积分。
 *
 * ## 为什么是四视图而不是「Token / 积分」两级
 *
 * 原型（`demo-app.js` 的 `viewUsage`）把用量页的**子 Tab 定为一级四视图**
 * （趋势 / 模型 / 会话 / 积分）。改造前这里是「Token 统计 / 积分统计」两个子 Tab，
 * 每个再复用一整页旧实现 —— 与原型不一致（P3-ARCH §6 #6）。
 *
 * ## 平台差异以**数据**表达（§6 #5）
 *
 * - **WorkBuddy**：四个视图齐备（趋势 / 模型 / 会话来自 Token 统计，积分来自积分统计）；
 * - **Trae**：Token 统计数据源形状不同、且**没有会话副本能力** ⇒
 *   **不渲染缺数据源的 Tab**（不画空壳）：只保留**趋势 + 积分**。
 *
 * ## 为什么仍 lazy 四个页面
 *
 * 这些页面各自很重（含 recharts）；合并进一个目的地后若都静态引入，
 * 「用量」的首次加载会同时拉多页。分开 lazy 之后，切 Tab 才加载另一个。
 */

import { lazy, Suspense, useState, type ElementType } from "react";
import { useLocation } from "react-router-dom";

import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useT } from "@/lib/i18n";
import { type Product, platformOfPath } from "@/lib/platforms/registry";
import { scopeOf } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import type { TranslationKey } from "@/locales/zh";

/* 四个页面**各自 lazy**（见文件头注释）。 */
const TokenStatsPage = lazy(() => import("@/pages/TokenStatsPage"));
const CreditStatsPage = lazy(() => import("@/pages/CreditStatsPage"));
const TraeTokenStatsPage = lazy(() => import("@/pages/TraeTokenStatsPage"));
const TraeCreditsPage = lazy(() => import("@/pages/TraeCreditsPage"));

/** 一级四视图（与原型 `tabs` 一一对应）。 */
type UsageTab = "trend" | "models" | "sessions" | "credits";

const TAB_LABEL: Record<UsageTab, TranslationKey> = {
  trend: "wbStats.usage.view.trend",
  models: "wbStats.usage.view.models",
  sessions: "wbStats.usage.view.sessions",
  credits: "wbStats.usage.view.credits",
};

/**
 * 平台 → 可用视图。
 *
 * ⚠️ Trae **不渲染**「模型 / 会话」：前者没有与 WorkBuddy 同形的 Token 四视图数据源，
 * 后者平台**无会话副本能力**（§3.15）。缺数据源就**不画空壳**（§6 #5）——
 * 空 Tab 会让用户以为「数据是 0」，而实际是「没有这个数据」。
 */
const PLATFORM_TABS: Record<Product, UsageTab[]> = {
  workbuddy: ["trend", "models", "sessions", "credits"],
  trae: ["trend", "credits"],
};

/** 平台 → 子页面（Token 页 / 积分页）。 */
const USAGE_PAGES: Record<
  Product,
  { tokens: ElementType; credits: ElementType; tokenViews: boolean }
> = {
  /* `tokenViews` = 该平台的 Token 页**是否消费 `view` 参数**（四视图）。
     WB 的 `TokenStatsPage` 接受 `view`；Trae 的 `TraeTokenStatsPage` **不接受** ——
     ⚠️ 它俩都取到 `trend`，但 Trae 侧**不该被传入**一个它不读的参数：
     静默忽略一个 prop 会让「以后给 Trae 加 `models` Tab」变成**静默显示错数据**。
     这与 `PLATFORM_TABS` 同源（Trae 本就没有 models / sessions 两个 Tab）。 */
  workbuddy: { tokens: TokenStatsPage, credits: CreditStatsPage, tokenViews: true },
  trae: { tokens: TraeTokenStatsPage, credits: TraeCreditsPage, tokenViews: false },
};

export default function UsagePage() {
  const t = useT();
  /* 平台从**路由**推（同 `OverviewPage`）：`/s/trae-cn/usage` 里就带着平台。 */
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);
  const tabs = PLATFORM_TABS[product];
  const [tab, setTab] = useState<UsageTab>("trend");
  /* 平台切换后若旧 Tab 在新平台不可用（如从 WB 的「模型」切到 Trae），回落到首个可用视图。 */
  const active = tabs.includes(tab) ? tab : tabs[0];

  const { tokens: TokenPage, credits: CreditPage, tokenViews } = USAGE_PAGES[product];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 页头（原型 `viewUsage`）：作用域副标题 + 只读 scope-chip；子 Tab 只做**目的地内部**的切换。 */}
      <div className="shrink-0 px-4 pt-6 sm:px-8 sm:pt-9">
        <header className="mx-auto flex w-full max-w-[1240px] items-start gap-4">
          <div className="min-w-0">
            <h1 className="text-[28px] font-semibold tracking-tight">{t("nav.usage")}</h1>
            <p className="mt-1.5 text-xs text-muted-foreground">
              {t("pageHead.usage", { scope: scopeLabel(t, scope) })}
            </p>
          </div>
          <div className="grow" />
          <ScopeChip scope={scope} />
        </header>
      </div>

      <div className="shrink-0 border-b border-border/60 px-5 pt-4">
        <Tabs value={active} onValueChange={(value) => setTab(value as UsageTab)}>
          <TabsList>
            {tabs.map((value) => (
              <TabsTrigger key={value} value={value}>
                {t(TAB_LABEL[value])}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </div>

      {/* ⚠️ 不在这里加 `overflow-auto`：子页面**各自有滚动容器**，再套一层会出双滚动条。 */}
      <div className="min-h-0 flex-1">
        <Suspense fallback={<Skeleton className="m-5 h-32" />}>
          {/*
            ⚠️ **不写 `product === "workbuddy" ? … : …`**（P2 收敛清单第 5 项）：
            「哪些视图存在」由 `PLATFORM_TABS` 给出、「用哪个页面」由 `USAGE_PAGES` 给出，
            两者都已是**按平台的数据**。这里再判一次平台就是**第三份**平台知识，
            新增平台时必漏改。`tokenViews` 同样是数据（该平台的 Token 页是否消费 `view`）。
          */}
          {active === "credits" ? (
            <CreditPage />
          ) : tokenViews ? (
            <TokenPage
              view={active === "models" ? "models" : active === "sessions" ? "sessions" : "trend"}
            />
          ) : (
            <TokenPage />
          )}
        </Suspense>
      </div>
    </div>
  );
}
