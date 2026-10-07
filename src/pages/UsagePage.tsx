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

import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useT } from "@/lib/i18n";
import { type Product, platformOfPath } from "@/lib/platforms/registry";
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
const USAGE_PAGES: Record<Product, { tokens: ElementType; credits: ElementType }> = {
  workbuddy: { tokens: TokenStatsPage, credits: CreditStatsPage },
  trae: { tokens: TraeTokenStatsPage, credits: TraeCreditsPage },
};

export default function UsagePage() {
  const t = useT();
  /* 平台从**路由**推（同 `OverviewPage`）：`/s/trae-cn/usage` 里就带着平台。 */
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const tabs = PLATFORM_TABS[product];
  const [tab, setTab] = useState<UsageTab>("trend");
  /* 平台切换后若旧 Tab 在新平台不可用（如从 WB 的「模型」切到 Trae），回落到首个可用视图。 */
  const active = tabs.includes(tab) ? tab : tabs[0];

  const TokenPage = USAGE_PAGES[product].tokens;
  const CreditPage = USAGE_PAGES[product].credits;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 子 Tab 只做**目的地内部**的切换；作用域（区域）不在这里 —— 那是侧栏的事。 */}
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
          {active === "credits" ? (
            <CreditPage />
          ) : product === "workbuddy" ? (
            <TokenStatsPage
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
