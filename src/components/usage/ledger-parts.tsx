/**
 * 用量页 · **账本视图的展示件**（issue #63）。
 *
 * 设计依据：`docs/specs/USAGE-GATEWAY-UI-REDESIGN.md`。
 *
 * ## 这个文件里的东西为什么放在一起
 *
 * 它们都只在「客户端 / 网关是两本账」这个前提下才有意义：
 * 覆盖条（两本账各自多少、重叠多少）、账本表（单本账的模型构成）、
 * 网关来源表（会话视图在网关账本下的替代维度）、积分双口径。
 *
 * 共同的三条约束（改这里之前先读）：
 *
 * 1. **占比一律在单本账内算** —— 跨账本合并占比的分母是假的（两本账各记一次）；
 * 2. **重叠不给颜色** —— 它不是第三本账，只以数字出现；
 * 3. **空值显示 `—` 而不是 0** —— 「不知道」与「真的是 0」必须能分开。
 */

import { Info } from "lucide-react";

import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

import { compactTokens, exactTokens, formatCredits } from "./format";

/** 账本条的图例色（与 `StackedDailyBar` 的 `palette` 同源）。 */
export const LEDGER_CLIENT_COLOR = "var(--data-series-emerald)";
export const LEDGER_GATEWAY_COLOR = "var(--data-series-indigo)";

/** 覆盖条里的一项。 */
function CoverageItem({
  color,
  label,
  value,
  detail,
}: {
  color?: string;
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <span className="inline-flex items-center gap-1.5 text-muted-foreground">
      {color ? (
        <i className="size-2 shrink-0 rounded-[3px]" style={{ background: color }} aria-hidden="true" />
      ) : null}
      {label} <b className="font-medium text-foreground tabular-nums">{value}</b>
      {detail ? <span className="text-muted-foreground/80">· {detail}</span> : null}
    </span>
  );
}

/**
 * **覆盖条**：一行说清「两本账各多少、网关覆盖到哪、重叠多少」。
 *
 * ## 为什么提到页面级（而不是留在各图图注里）
 *
 * 改造前这些信息散在三张图的图注里（「网关覆盖 1/1 天」只在网关小图上），
 * 用户要读到第三张图才明白「网关只覆盖了今天」。数据现状是**页面级事实**，
 * 就该在页面级出现一次。
 *
 * ## 为什么它不随账本切换而变
 *
 * 它描述的是**数据现状**（两本账都有多少），不是「我现在选哪本」。
 * 跟着账本变会让用户在切到「网关」时看不到客户端有多少，反而更难对照。
 */
export function CoverageStrip({
  clientTokens,
  clientRequests,
  gatewayTokens,
  gatewayRequests,
  gatewayFailed,
  recorded,
  windowDays,
  overlapRows,
  overlapTokens,
  onReconcile,
  /**
   * 客户端账本**在本平台不存在**（Trae：没有可解析的客户端用量落盘）。
   *
   * 与「客户端是 0」**必须分开**：0 是「测到了、确实没用」，这里是「测不到」。
   * 传 `true` 时该项显示「不可得」并省掉请求数 —— 写 0 等于把未知说成已知。
   */
  clientUnavailable = false,
  unit = "tokens",
  showOverlap = true,
  className,
}: {
  clientTokens: number;
  clientRequests: number;
  gatewayTokens: number;
  gatewayRequests: number;
  gatewayFailed: number;
  recorded: number;
  windowDays: number;
  overlapRows: number;
  overlapTokens: number;
  onReconcile?: () => void;
  clientUnavailable?: boolean;
  /** 数值单位。Qoder 两本账都按 **credits** 计 —— 用 token 的 `K/M` 会谎报单位。 */
  unit?: "tokens" | "credits";
  /**
   * 是否显示「两本账重叠」。
   *
   * 只有**今日对账**（`get_usage_reconcile`，仅 WorkBuddy 有）能给出这个数。
   * Trae / Qoder 没有逐条对账 ⇒ 传 `false`，否则会长期显示「重叠 0 条」——
   * 那是把「没算过」写成「没有重叠」。
   */
  showOverlap?: boolean;
  className?: string;
}) {
  const t = useT();
  const format = unit === "credits" ? formatCredits : compactTokens;
  return (
    <div
      className={cn(
        "mb-4 flex flex-wrap items-center gap-x-3.5 gap-y-1.5 rounded-[10px] border bg-card-2 px-3.5 py-2.5 text-xs",
        className,
      )}
      role="group"
      aria-label={t("wbStats.usage.coverage.aria")}
    >
      <CoverageItem
        color={LEDGER_CLIENT_COLOR}
        label={t("wbStats.usage.coverage.client")}
        value={clientUnavailable ? t("wbStats.usage.coverage.unavailable") : format(clientTokens)}
        detail={
          clientUnavailable
            ? undefined
            : t("wbStats.usage.coverage.requests", { n: exactTokens(clientRequests) })
        }
      />
      <CoverageItem
        color={LEDGER_GATEWAY_COLOR}
        label={t("wbStats.usage.coverage.gateway")}
        value={format(gatewayTokens)}
        detail={t("wbStats.usage.coverage.requests", { n: exactTokens(gatewayRequests) })}
      />
      {gatewayFailed > 0 ? (
        <span className="rounded-[5px] bg-warning/15 px-1.5 py-0.5 text-[11px] text-warning">
          {t("wbStats.usage.coverage.failed", { n: exactTokens(gatewayFailed) })}
        </span>
      ) : null}
      <span className="text-muted-foreground">
        {t("wbStats.usage.coverage.recorded", { recorded, total: windowDays })}
      </span>
      {showOverlap ? (
        <span className="inline-flex items-center gap-1.5 text-muted-foreground">
          {t("wbStats.usage.coverage.overlap", {
            n: exactTokens(overlapRows),
            tokens: format(overlapTokens),
          })}
          {onReconcile ? (
            <button
              type="button"
              onClick={onReconcile}
              className="cursor-pointer border-b border-dotted border-current text-[color:var(--data-series-indigo)] hover:text-foreground"
            >
              {t("wbStats.usage.coverage.seeRecon")}
            </button>
          ) : null}
        </span>
      ) : null}
    </div>
  );
}

/** 表内分布条（单色，宽度按行内最大值归一）。 */
function DistBar({ value, max, color }: { value: number; max: number; color: string }) {
  const width = max > 0 ? Math.max(1, (value / max) * 100) : 0;
  return (
    <div className="h-2 w-full overflow-hidden rounded-full bg-foreground/5">
      <i className="block h-full rounded-full" style={{ width: `${width}%`, background: color }} />
    </div>
  );
}

/**
 * **单本账的模型表**。
 *
 * ## 为什么是「两张表」而不是一张混合表
 *
 * 混合表的「占比」分母只能是两本账的和 —— 而那个和含重复计数，任何百分比都是假的。
 * 拆表之后每张表的分母都是自己那本账，**不需要解释**。
 *
 * ## 为什么网关表多一列「失败」
 *
 * 客户端账本不记失败（会话日志只留成功落盘的条目）⇒ 客户端表显示 `—` 而不是 0：
 * 「不知道」与「真的没有失败」是两件事。
 */
export function LedgerModelTable({
  book,
  rows,
  total,
  caption,
  failed,
}: {
  book: "client" | "gateway";
  rows: {
    name: string;
    tokens: number;
    requests: number;
    failed?: number;
    /**
     * 该行的缓存命中率（0–1；`null` = 没观测到）。
     *
     * ★ issue #154：只在**网关账本**出现 —— 客户端会话日志的命中量走另一套口径
     * （`token_stats::cacheHitRate`），两个数字不可放在同一列比较。
     */
    hitRate?: number | null;
  }[];
  total: number;
  caption: string;
  /** 网关账本：失败条数是否逐行给出（当前后端只到模型级）。 */
  failed?: number;
}) {
  const t = useT();
  const client = book === "client";
  const color = client ? LEDGER_CLIENT_COLOR : LEDGER_GATEWAY_COLOR;
  const max = Math.max(1, ...rows.map((row) => row.tokens));
  return (
    <Card className="min-w-0">
      <CardHeader className="flex items-center gap-2.5 border-b px-4 py-3">
        <h3 className="text-[13.5px] font-semibold">
          {t(client ? "wbStats.usage.models.clientBook" : "wbStats.usage.models.gatewayBook")}
        </h3>
        <div className="flex-1" />
        <span className="text-[11.5px] text-muted-foreground">{caption}</span>
      </CardHeader>
      <CardContent className="p-0">
        <table className="w-full text-xs">
          <thead>
            <tr className="border-b text-[11px] text-muted-foreground">
              <th className="px-4 py-2 text-left font-medium">{t("wbStats.usage.models.colModel")}</th>
              <th className="w-[34%] px-3 py-2 text-left font-medium">{t("wbStats.usage.models.colDist")}</th>
              <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colTokens")}</th>
              <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colShare")}</th>
              <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colRequests")}</th>
              {client ? null : (
                <>
                  <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colCacheHit")}</th>
                  <th className="px-4 py-2 text-right font-medium">{t("wbStats.usage.models.colFailed")}</th>
                </>
              )}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.name} className="border-b border-border/40 last:border-0 hover:bg-foreground/[0.025]">
                <td className="px-4 py-2">
                  <span className="inline-flex items-center gap-2">
                    <i className="size-2 shrink-0 rounded-[3px]" style={{ background: color }} aria-hidden="true" />
                    <span className="font-medium">{row.name}</span>
                  </span>
                </td>
                <td className="px-3 py-2">
                  <DistBar value={row.tokens} max={max} color={color} />
                </td>
                <td className="px-3 py-2 text-right tabular-nums">{compactTokens(row.tokens)}</td>
                <td className="px-3 py-2 text-right tabular-nums text-muted-foreground">
                  {total > 0 ? `${Math.round((row.tokens / total) * 100)}%` : "—"}
                </td>
                <td className="px-3 py-2 text-right tabular-nums text-muted-foreground">
                  {exactTokens(row.requests)}
                </td>
                {client ? null : (
                  <>
                    {/* 「—」= 这个模型没观测到命中量（上游没报 / 老账本），不是 0% 命中。 */}
                    <td className="px-3 py-2 text-right tabular-nums text-muted-foreground">
                      {row.hitRate == null ? "—" : `${Math.round(row.hitRate * 100)}%`}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums text-warning">
                      {row.failed == null ? "—" : exactTokens(row.failed)}
                    </td>
                  </>
                )}
              </tr>
            ))}
          </tbody>
        </table>
        <p className="px-4 pb-3 pt-2 text-[11.5px] text-muted-foreground">
          {client
            ? t("wbStats.usage.models.clientNoFail")
            : t("wbStats.usage.models.gatewayFail", { n: exactTokens(failed ?? 0) })}
        </p>
      </CardContent>
    </Card>
  );
}

/**
 * 会话视图在**网关账本**下的替代维度：按账号 / 按端点。
 *
 * ## 为什么是「换维度」而不是「空态 + 道歉」
 *
 * 网关是逐请求转发的，日志里没有会话 ID ⇒ 会话视图**永远**装不进网关数据。
 * 但网关有账号和端点 —— 那才是它的「来源分布」。摆一张空表加一句「不支持」，
 * 等于把用户送到一个死胡同；换成按账号 / 按端点，至少回答「这台机器的网关流量是谁的」。
 */
export function GatewaySourceTables({
  accounts,
  endpoints,
  gatewayTokens,
  gatewayRequests,
  unit = "tokens",
}: {
  accounts: { key: string; tokens: number; requests: number }[];
  endpoints: { key: string; requests: number }[];
  gatewayTokens: number;
  gatewayRequests: number;
  /**
   * 数值单位。Qoder 的网关账按 **credits** 计（token 计数恒 0），
   * 用 token 的 `1.2M` 去格式化 credits 会让图注说谎。
   */
  unit?: "tokens" | "credits";
}) {
  const t = useT();
  const format = unit === "credits" ? formatCredits : compactTokens;
  const unitColumn =
    unit === "credits" ? t("wbStats.usage.credits.col") : t("wbStats.usage.models.colTokens");
  const maxAccount = Math.max(1, ...accounts.map((row) => row.tokens));
  const maxEndpoint = Math.max(1, ...endpoints.map((row) => row.requests));
  return (
    <>
      <div className="mb-4 flex items-center gap-2.5 rounded-[10px] border border-[color:var(--data-series-indigo)]/25 bg-[color:var(--data-series-indigo)]/10 px-3.5 py-2.5 text-xs text-muted-foreground">
        <Info className="size-3.5 shrink-0 text-[color:var(--data-series-indigo)]" />
        <span>{t("wbStats.usage.sessions.gatewayNote")}</span>
      </div>
      <div className="grid min-w-0 gap-4 lg:grid-cols-2">
        <Card className="min-w-0">
          <CardHeader className="flex items-center gap-2.5 border-b px-4 py-3">
            <h3 className="text-[13.5px] font-semibold">{t("wbStats.usage.sessions.byAccount")}</h3>
          </CardHeader>
          <CardContent className="p-0">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b text-[11px] text-muted-foreground">
                  <th className="px-4 py-2 text-left font-medium">{t("wbStats.usage.sessions.account")}</th>
                  <th className="w-[32%] px-3 py-2 text-left font-medium">{t("wbStats.usage.models.colDist")}</th>
                  <th className="px-3 py-2 text-right font-medium">{unitColumn}</th>
                  <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colRequests")}</th>
                  <th className="px-4 py-2 text-right font-medium">{t("wbStats.usage.models.colShare")}</th>
                </tr>
              </thead>
              <tbody>
                {accounts.map((row) => (
                  <tr key={row.key} className="border-b border-border/40 last:border-0">
                    <td className="truncate px-4 py-2 font-medium" title={row.key}>
                      {row.key}
                    </td>
                    <td className="px-3 py-2">
                      <DistBar value={row.tokens} max={maxAccount} color={LEDGER_GATEWAY_COLOR} />
                    </td>
                    <td className="px-3 py-2 text-right tabular-nums">{format(row.tokens)}</td>
                    <td className="px-3 py-2 text-right tabular-nums text-muted-foreground">
                      {exactTokens(row.requests)}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">
                      {gatewayTokens > 0 ? `${Math.round((row.tokens / gatewayTokens) * 100)}%` : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="px-4 pb-3 pt-2 text-[11.5px] text-muted-foreground">
              {t("wbStats.usage.sessions.accountHint")}
            </p>
          </CardContent>
        </Card>
        <Card className="min-w-0">
          <CardHeader className="flex items-center gap-2.5 border-b px-4 py-3">
            <h3 className="text-[13.5px] font-semibold">{t("wbStats.usage.sessions.byEndpoint")}</h3>
          </CardHeader>
          <CardContent className="p-0">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b text-[11px] text-muted-foreground">
                  <th className="px-4 py-2 text-left font-medium">{t("wbStats.usage.sessions.endpoint")}</th>
                  <th className="w-[32%] px-3 py-2 text-left font-medium">{t("wbStats.usage.models.colDist")}</th>
                  <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colRequests")}</th>
                  <th className="px-4 py-2 text-right font-medium">{t("wbStats.usage.models.colShare")}</th>
                </tr>
              </thead>
              <tbody>
                {endpoints.map((row) => (
                  <tr key={row.key} className="border-b border-border/40 last:border-0">
                    <td className="truncate px-4 py-2 font-mono text-[11.5px]" title={row.key}>
                      {row.key || "—"}
                    </td>
                    <td className="px-3 py-2">
                      <DistBar value={row.requests} max={maxEndpoint} color={LEDGER_GATEWAY_COLOR} />
                    </td>
                    <td className="px-3 py-2 text-right tabular-nums">{exactTokens(row.requests)}</td>
                    <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">
                      {gatewayRequests > 0 ? `${Math.round((row.requests / gatewayRequests) * 100)}%` : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="px-4 pb-3 pt-2 text-[11.5px] text-muted-foreground">
              {t("wbStats.usage.sessions.endpointHint")}
            </p>
          </CardContent>
        </Card>
      </div>
    </>
  );
}

/**
 * 积分 · **单本账的积分表**（模型 × 积分 × 请求）。
 *
 * 与 `LedgerModelTable` 分开是因为**单位不同**：这里没有 token，只有积分。
 * 复用同一组件就得加一堆「这个字段在积分模式下没意义」的分支 —— 那比多一个组件更糟。
 */
export function CreditLedgerTable({
  book,
  rows,
  total,
}: {
  book: "client" | "gateway";
  /** `requests` 可缺：客户端账的逐模型请求数**不可得**（会话记录只按日给），缺省渲染 `—`。 */
  rows: { name: string; credit: number; requests?: number }[];
  total: number;
}) {
  const t = useT();
  const client = book === "client";
  const color = client ? LEDGER_CLIENT_COLOR : LEDGER_GATEWAY_COLOR;
  const max = Math.max(0.01, ...rows.map((row) => row.credit));
  return (
    <Card className="min-w-0">
      <CardHeader className="flex items-center gap-2.5 border-b px-4 py-3">
        <h3 className="text-[13.5px] font-semibold">
          {t(client ? "wbStats.usage.models.clientBook" : "wbStats.usage.models.gatewayBook")}
        </h3>
        <div className="flex-1" />
        <span className="text-[11.5px] text-muted-foreground">{formatCredits(total)}</span>
      </CardHeader>
      <CardContent className="p-0">
        <table className="w-full text-xs">
          <thead>
            <tr className="border-b text-[11px] text-muted-foreground">
              <th className="px-4 py-2 text-left font-medium">{t("wbStats.usage.models.colModel")}</th>
              <th className="w-[34%] px-3 py-2 text-left font-medium">{t("wbStats.usage.models.colDist")}</th>
              <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.credits.col")}</th>
              <th className="px-3 py-2 text-right font-medium">{t("wbStats.usage.models.colShare")}</th>
              <th className="px-4 py-2 text-right font-medium">{t("wbStats.usage.models.colRequests")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.name} className="border-b border-border/40 last:border-0">
                <td className="px-4 py-2">
                  <span className="inline-flex items-center gap-2">
                    <i className="size-2 shrink-0 rounded-[3px]" style={{ background: color }} aria-hidden="true" />
                    <span className="font-medium">{row.name}</span>
                  </span>
                </td>
                <td className="px-3 py-2">
                  <DistBar value={row.credit} max={max} color={color} />
                </td>
                <td className="px-3 py-2 text-right tabular-nums">{formatCredits(row.credit)}</td>
                <td className="px-3 py-2 text-right tabular-nums text-muted-foreground">
                  {total > 0 ? `${Math.round((row.credit / total) * 100)}%` : "—"}
                </td>
                <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">
                  {row.requests == null ? "—" : exactTokens(row.requests)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </CardContent>
    </Card>
  );
}

/**
 * 积分 · **两个口径的差额**。
 *
 * 余额口径（快照差值）权威、不重复、拆不到来源；请求口径能拆来源、会重复。
 * 差额 = 未归因消耗（别的设备 / 没落本地日志的请求）—— **绝不摊到任何来源上**。
 */
export function CreditGapNote({
  billed,
  byRequest,
  client,
  gateway,
  overlap,
  gap,
}: {
  billed: number;
  byRequest: number;
  client: number;
  gateway: number;
  overlap: number;
  gap: number;
}) {
  const t = useT();
  return (
    <Card className="mb-4 min-w-0">
      <CardHeader className="flex items-center gap-2.5 border-b px-4 py-3">
        <h3 className="text-[13.5px] font-semibold">{t("wbStats.usage.credits.gap")}</h3>
      </CardHeader>
      <CardContent className="px-4 py-3">
        <p className="text-[13px] leading-relaxed tabular-nums text-muted-foreground">
          {t("wbStats.usage.credits.gapBody", {
            billed: formatCredits(billed),
            request: formatCredits(byRequest),
            client: formatCredits(client),
            gateway: formatCredits(gateway),
            overlap: formatCredits(overlap),
            gap: formatCredits(gap),
          })}
        </p>
        <p className="mt-2 text-[11.5px] leading-relaxed text-muted-foreground">
          {t("wbStats.usage.credits.gapHint")}
        </p>
      </CardContent>
    </Card>
  );
}
