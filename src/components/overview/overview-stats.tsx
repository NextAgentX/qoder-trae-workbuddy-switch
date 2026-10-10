/**
 * 概览 · **统计行**（原型 `viewOverview` 的 `.grid.g4` 四块）。
 *
 * ## 为什么是「一排四块」
 *
 * 改造前概览的统计区是**两处**：`TodayUsage`（今日 Token / 调用 / 对比昨日）+ 独立
 * `grid-cols-3` 的「账号 / 待处理 / 客户端」。于是同一排数字被拆成两段，
 * 而「待处理」在卡头已有计数、「客户端」在顶栏 `run-pill` 已有运行态 ⇒ 那三块是**重复**。
 *
 * 现按原型收成一排四块：**账号总数 / 今日 Token / 今日积分消耗 / 网关请求（今日）**。
 *
 * ## ★ 诚实性（本项目红线）
 *
 * - 读不到的项显示 **「—」**，不回落 0（见 `lib/overview-metrics.ts` 的注释）；
 * - 「对比昨日」**真算**：昨日为 0 时给「昨日无数据」，**不给** `+12%` 这类假增长（断言 P0-2）；
 * - 网关请求被日志裁剪时显示 **「≥ N」**（下界不是精确值，不能当精确值显示）。
 */

import { Coins, MessagesSquare, Route, Users } from "lucide-react";

import { StatTiles, type StatTile } from "@/components/usage/stat-tiles";
import { compactTokens, exactTokens, formatCredits } from "@/components/usage/format";
import { useT } from "@/lib/i18n";
import type { OverviewMetrics } from "@/lib/overview-metrics";

interface Props {
  metrics: OverviewMetrics;
  /** 当前作用域的账号数（来自账号 store —— 与「待处理」同一份数据）。 */
  accountCount: number;
  /** 需要处理的账号数（重新登录 + 即将过期）。 */
  attentionCount: number;
  /** 账号库首次加载中（此时账号数还不能当结论显示）。 */
  accountsLoading: boolean;
}

export function OverviewStats({ metrics, accountCount, attentionCount, accountsLoading }: Props) {
  const t = useT();
  const { token, credits, gateway } = metrics;

  /* 对比昨日：`(今日 - 昨日) / 昨日`；昨日为 0 时**不编百分比**。 */
  let delta: { value: string; tone: StatTile["tone"] } | null = null;
  if (token) {
    if (token.yesterday <= 0) {
      delta = { value: t("wbOverview.today.noBaseline"), tone: "default" };
    } else {
      const pct = ((token.today - token.yesterday) / token.yesterday) * 100;
      if (Math.abs(pct) < 0.05) {
        delta = { value: t("wbOverview.today.flat"), tone: "default" };
      } else if (pct > 0) {
        delta = { value: t("wbOverview.today.up", { pct: pct.toFixed(1) }), tone: "up" };
      } else {
        delta = { value: t("wbOverview.today.down", { pct: Math.abs(pct).toFixed(1) }), tone: "down" };
      }
    }
  }

  const tiles: StatTile[] = [
    {
      icon: Users,
      label: t("wbOverview.stat.accounts"),
      value: accountsLoading ? "—" : exactTokens(accountCount),
      hint:
        attentionCount > 0
          ? t("wbOverview.stat.accountsAttention", { n: attentionCount })
          : t("wbOverview.stat.accountsOk"),
      tone: attentionCount > 0 ? "warn" : "default",
    },
    {
      icon: MessagesSquare,
      label: t("wbOverview.stat.tokens"),
      value: token ? compactTokens(token.today) : "—",
      hint: delta?.value,
      // ★ Q3：本格只算**客户端**（不含网关）⇒ 用 tooltip 说清口径，避免与账号卡的
      //   「今日 Token」（含网关）同名而被当成同一个数。
      title: t("wbOverview.stat.tokensHintClientOnly"),
      tone: delta?.tone ?? "default",
    },
    {
      icon: Coins,
      label: t("wbOverview.stat.credits"),
      value: credits ? formatCredits(credits.today) : "—",
      hint: credits
        ? t("wbOverview.stat.creditsRemaining", { n: formatCredits(credits.remaining) })
        : t("wbOverview.stat.unavailable"),
    },
    {
      icon: Route,
      label: t("wbOverview.stat.gateway"),
      value: gateway
        ? gateway.truncated
          ? `≥ ${exactTokens(gateway.today)}`
          : exactTokens(gateway.today)
        : "—",
      hint: gateway
        ? gateway.truncated
          ? t("wbOverview.stat.gatewayTruncated")
          : t("wbOverview.stat.gatewayHint")
        : t("wbOverview.stat.unavailable"),
    },
  ];

  /* `separated`：概览按原型 `.grid.g4` 摆成**一排四张独立卡**（每块一张 `.stat`）。 */
  return <StatTiles tiles={tiles} ariaLabel={t("wbOverview.stat.aria")} separated />;
}