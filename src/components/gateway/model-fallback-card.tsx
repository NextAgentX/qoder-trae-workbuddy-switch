/**
 * 模型降级链卡片（「网关 → 模型」分区）—— 与原型 `docs/prototype/demo-app.js::fallbackCard` 同构。
 *
 * ## 为什么是「模型」分区里的一张卡，而不是第 7 个分区
 *
 * 新增分区要同时改 `GW_INDEPENDENT_SECTIONS`、三个网关页的接线、以及
 * `scripts/check-scope-registry.cjs` 的断言 —— 四处联动、漏一处 build 就红。
 * 而「降级链」与「模型清单」本来就是同一组东西：清单说有哪些模型，降级链说它们挂了怎么办。
 *
 * ## 状态点为什么读后端的 `state`，不自己算
 *
 * `FallbackTargetView.state` 由后端从闸门 + 禁用表**派生**（与账号池同源）。
 * 前端自己拼会在另一个时刻算出另一份结论 —— 卡片上绿、账号池里黄，用户没法判断该信谁。
 *
 * ## 触发标签为什么只标「增量」
 *
 * `gate` / `model_rate` / `model_blocked` 是基线（默认全开）。把它们也画成 tag，
 * 每行右侧会堆满同样三个词，反而看不出「这条链额外开了 5xx」。
 * 所以卡片上只出现 `+5xx` 这种增量，完整清单进 `title`。
 */

import { useEffect, useState, type ReactNode } from "react";
import { ChevronDown, ChevronUp, Plus, TriangleAlert } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { CardHead } from "@/components/ui/card-head";
import { EmptyState } from "@/components/ui/empty-state";
import { Switch } from "@/components/ui/switch";
import { useT } from "@/lib/i18n";
import { scopeOf, type Scope } from "@/lib/scope";
import { cn } from "@/lib/utils";
import { useGatewayStore } from "@/stores/gateway";
import type {
  FallbackChainView,
  FallbackTargetState,
  FallbackTrigger,
  Region,
} from "@/lib/types";

import { ModelFallbackSheet, type ModelOption } from "./model-fallback-sheet";

/** 状态 → 圆点底色。`gated` 是**黄**（暂时不可用），`disabled` 是**灰**（要人去改）。 */
const DOT_CLASS: Record<FallbackTargetState, string> = {
  healthy: "bg-success",
  gated: "bg-warning",
  disabled: "bg-muted-foreground/40",
  unknown: "bg-muted-foreground/40",
};

const TRIGGER_SHORT_KEY: Record<FallbackTrigger, string> = {
  gate: "shared.gateway.fallback.short.gate",
  model_rate: "shared.gateway.fallback.short.model_rate",
  model_blocked: "shared.gateway.fallback.short.model_blocked",
  server: "shared.gateway.fallback.short.server",
  timeout: "shared.gateway.fallback.short.timeout",
  not_found: "shared.gateway.fallback.short.not_found",
};

/**
 * 事件里的 `reason` 是**稳定标识符**（`Trigger::as_str`，客户端可编程判断），
 * 直接渲染会看到 `model_rate` 这种裸英文。流水给人看，这里映射回完整文案。
 */
const REASON_LABEL_KEY: Record<string, string> = {
  model_gate: "shared.gateway.fallback.trigger.gate",
  model_rate: "shared.gateway.fallback.trigger.model_rate",
  model_blocked: "shared.gateway.fallback.trigger.model_blocked",
  server: "shared.gateway.fallback.trigger.server",
  timeout: "shared.gateway.fallback.trigger.timeout",
  not_found: "shared.gateway.fallback.short.not_found",
};

interface Props {
  region: Region;
  /**
   * 作用域。缺省按 WorkBuddy + region 推导。
   *
   * Trae / Qoder 页必须**显式传**（`scopeOf("trae"|"qoder", region)`）——
   * 它们的作用域与 WorkBuddy 完全独立，配置也各存一份。
   */
  scope?: Scope;
  /**
   * 可选模型清单（`id` 是值、`name` 只做展示）。
   *
   * Trae 的清单来自客户端 `vscdb` 缓存、Qoder 的来自上游实时拉取，
   * **都不在 gateway store 里** ⇒ 由页面注入。缺省时退回 WB 的 catalog。
   */
  modelOptions?: ModelOption[];
  className?: string;
}

export function ModelFallbackCard({ region, scope: scopeProp, modelOptions, className }: Props) {
  const t = useT();
  const scope = scopeProp ?? scopeOf("workbuddy", region);
  const view = useGatewayStore((s) => s.fallback[scope]);
  const loadFallback = useGatewayStore((s) => s.loadFallback);
  const saveFallback = useGatewayStore((s) => s.saveFallback);
  // ★ 近 7 天统计来自**落盘账本**（`fallbackStats`），与下面「降级流水」的
  //   环形缓冲**不同源**：流水只有最近几十条、覆盖十几分钟，绝不能拿它算窗口统计。
  const stats = useGatewayStore((s) => s.fallbackStats[scope]);
  const loadStats = useGatewayStore((s) => s.loadFallbackStats);
  /** `undefined` = 抽屉关闭；`null` = 新建；`string` = 编辑该主模型。 */
  const [editing, setEditing] = useState<string | null | undefined>(undefined);
  const [eventsOpen, setEventsOpen] = useState(false);
  /**
   * 折叠 / 展开（与「限流探测」卡同一套形态，默认**收起**）。
   *
   * 收起态仍保留标题、状态徽标、恒定明示与近 7 天统计 —— 那几句回答的是
   * 「会不会偷偷换模型」，属于卡片的身份，不能随配置区一起折掉。
   */
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (!view) void loadFallback(scope);
  }, [scope, view, loadFallback]);

  useEffect(() => {
    void loadStats(scope);
  }, [scope, loadStats]);

  if (!view) {
    // 首帧还没读到：不画骨架（这张卡不是页面主体，闪一下比空骨架更安静）。
    return null;
  }

  const chains = view.chains;
  // 取成局部量：`view` 在闭包里会重新放宽为 `ModelFallbackView | undefined`，
  // 直接在 async 函数体里读会报「possibly undefined」。
  const fallbackEnabled = view.enabled;
  const activeCount = chains.filter((chain) => chain.enabled).length;
  const badge = !chains.length
    ? { text: t("shared.gateway.fallback.badgeOff"), tone: "muted" as const }
    : view.enabled
      ? {
          text: t("shared.gateway.fallback.badgeOn", { count: activeCount }),
          tone: "ok" as const,
        }
      : { text: t("shared.gateway.fallback.badgeClosed"), tone: "muted" as const };

  async function onToggle(enabled: boolean) {
    // 只改总开关：把现有链原样带上（后端是**整作用域覆盖**，不带等于删光）。
    await saveFallback(scope, {
      enabled,
      chains: Object.fromEntries(
        chains.map((chain) => [
          chain.from,
          {
            targets: chain.targets.map((target) => ({
              // 同作用域存 `null`（配置里最简形态）；跨作用域才带 scope。
              scope: target.scope === scope ? null : target.scope,
              model: target.model,
            })),
            triggers: chain.triggers,
            enabled: chain.enabled,
          },
        ]),
      ),
    });
  }

  async function onToggleChain(chain: FallbackChainView) {
    await saveFallback(scope, {
      enabled: fallbackEnabled,
      chains: Object.fromEntries(
        chains.map((item) => [
          item.from,
          {
            targets: item.targets.map((target) => ({
              scope: target.scope === scope ? null : target.scope,
              model: target.model,
            })),
            triggers: item.triggers,
            enabled: item.from === chain.from ? !item.enabled : item.enabled,
          },
        ]),
      ),
    });
  }

  return (
    <Card className={cn("gap-0 py-0", className)} data-act="gw-fallback-card">
      <CardHead
        title={t("shared.gateway.fallback.title")}
        actions={
          <>
            <Badge
              variant="secondary"
              className={cn(
                badge.tone === "ok" && "bg-success/15 text-success",
                badge.tone === "muted" && "text-muted-foreground",
              )}
            >
              {badge.text}
            </Badge>
            <span className="hidden text-[11px] text-muted-foreground sm:inline">
              {t("shared.gateway.fallback.limit", { max: view.max_targets })}
            </span>
            <Button
              variant="ghost"
              size="sm"
              data-act="gw-fallback-events"
              onClick={() => {
                // 收起态点「降级流水」：先展开卡片，否则表渲染在折叠区里、点下去毫无反应。
                setExpanded(true);
                setEventsOpen((value) => !value);
              }}
              aria-expanded={eventsOpen}
            >
              {eventsOpen
                ? t("shared.gateway.fallback.eventsHide")
                : t("shared.gateway.fallback.events")}
              {eventsOpen ? <ChevronUp /> : <ChevronDown />}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              data-act="gw-fallback-expand"
              onClick={() => setExpanded((value) => !value)}
              aria-expanded={expanded}
              aria-controls="gw-fallback-body"
            >
              {expanded ? <ChevronUp /> : <ChevronDown />}
              {expanded
                ? t("shared.gateway.fallback.collapse")
                : t("shared.gateway.fallback.expand")}
            </Button>
          </>
        }
      />

      {/* 恒定明示：这两句必须一直在，不能塞进 tooltip ——
          「会不会偷偷换模型」是这个功能最需要被回答的疑问。 */}
      <p className="px-4 pt-3 text-[11.5px] leading-relaxed text-muted-foreground">
        {t("shared.gateway.fallback.notice")}
      </p>

      {stats && stats.total.degraded + stats.total.failed > 0 ? (
        <p className="px-4 pt-1.5 text-[11.5px] tabular-nums text-muted-foreground">
          {t("shared.gateway.fallback.stats", {
            days: stats.days.length,
            total: stats.total.degraded + stats.total.failed,
            ok: stats.total.degraded,
            fail: stats.total.failed,
          })}
        </p>
      ) : null}

      <div id="gw-fallback-body">
        {expanded ? (
          <>
            <div className="flex items-center gap-2.5 border-b border-dashed border-border px-4 py-3">
              <Switch
                checked={view.enabled}
                onCheckedChange={(checked) => void onToggle(checked)}
                aria-label={t("shared.gateway.fallback.enable")}
              />
              <div className="min-w-0 flex-1">
                <b className="block text-[12.5px] font-medium">
                  {t("shared.gateway.fallback.enable")}
                </b>
                <span className="text-[11px] text-muted-foreground">
                  {t("shared.gateway.fallback.enableHint")}
                </span>
              </div>
              <Button size="sm" data-act="gw-fallback-new" onClick={() => setEditing(null)}>
                <Plus />
                {t("shared.gateway.fallback.new")}
              </Button>
            </div>

            {chains.length ? (
              <div className="px-4">
                {chains.map((chain) => (
                  <ChainRow
                    key={chain.from}
                    chain={chain}
                    scope={scope}
                    maxTargets={view.max_targets}
                    baseline={view.baseline}
                    onEdit={() => setEditing(chain.from)}
                    onToggle={() => void onToggleChain(chain)}
                  />
                ))}
                <p className="py-3 text-[11px] leading-relaxed text-muted-foreground">
                  {t("shared.gateway.fallback.footNote")}
                </p>
              </div>
            ) : (
              <EmptyState
                icon={ChevronDown}
                title={t("shared.gateway.fallback.emptyTitle")}
                description={t("shared.gateway.fallback.emptyDesc")}
                action={
                  <Button size="sm" onClick={() => setEditing(null)}>
                    <Plus />
                    {t("shared.gateway.fallback.new")}
                  </Button>
                }
              />
            )}

            {eventsOpen ? <EventsTable events={view.events} scope={view.scope} /> : null}
          </>
        ) : null}
      </div>

      <ModelFallbackSheet
        scope={scope}
        region={region}
        modelOptions={modelOptions}
        view={view}
        editing={editing}
        onClose={() => setEditing(undefined)}
      />
    </Card>
  );
}

/* ─────────── 一条链 ─────────── */

function ChainRow({
  chain,
  scope,
  maxTargets,
  baseline,
  onEdit,
  onToggle,
}: {
  chain: FallbackChainView;
  /** 入口作用域：用来判断哪些备选是**跨作用域**（要标出平台前缀）。 */
  scope: Scope;
  maxTargets: number;
  baseline: FallbackTrigger[];
  onEdit: () => void;
  onToggle: () => void;
}) {
  const t = useT();
  const extra = chain.triggers.filter((trigger) => !baseline.includes(trigger));
  const triggerTitle = chain.triggers
    .map((trigger) => t(TRIGGER_SHORT_KEY[trigger] as never))
    .join(" / ");

  return (
    <div
      className={cn(
        // 第一列按内容宽度收：用 1fr 会让主模型与箭头之间裂开一大片空白。
        "grid grid-cols-[auto_18px_minmax(0,1fr)_auto] items-center gap-2.5",
        "border-b border-border py-3 last:border-b-0",
        !chain.enabled && "opacity-60",
      )}
    >
      <ModelChip model={chain.from} state="healthy" source />

      <span className="text-center text-xs text-muted-foreground/70" aria-hidden>
        →
      </span>

      <div className="flex min-w-0 flex-wrap items-center gap-1.5">
        {chain.targets.map((target, index) => (
          <span key={`${target.scope}:${target.model}`} className="flex items-center gap-1.5">
            {index > 0 ? (
              <span className="text-[11px] text-muted-foreground/50" aria-hidden>
                ›
              </span>
            ) : null}
            {/* 跨作用域的目标带上前缀：否则用户以为还是同一个池的模型，
                而它实际消耗的是**另一个平台/区域**的账号额度。 */}
            {target.scope !== scope ? (
              <span className="shrink-0 rounded bg-card-2 px-1 py-0.5 font-mono text-[10px] text-muted-foreground">
                {target.scope}
              </span>
            ) : null}
            <ModelChip model={target.model} state={target.state} />
          </span>
        ))}
        {chain.targets.length < maxTargets ? (
          <button
            type="button"
            onClick={onEdit}
            className="rounded-md border border-dashed border-border-strong px-2 py-0.5 text-[11px] text-muted-foreground transition-colors hover:border-primary hover:text-foreground"
          >
            {t("shared.gateway.fallback.addTarget")}
          </button>
        ) : null}
      </div>

      <div className="flex items-center gap-2">
        {chain.enabled ? (
          <div className="flex flex-wrap justify-end gap-1" title={triggerTitle}>
            {extra.map((trigger) => (
              <span
                key={trigger}
                className="rounded border border-border bg-card-2 px-1.5 py-px text-[10px] text-muted-foreground"
              >
                +{t(TRIGGER_SHORT_KEY[trigger] as never)}
              </span>
            ))}
          </div>
        ) : (
          <Badge variant="secondary" className="text-[10px]">
            {t("shared.gateway.fallback.chainOff")}
          </Badge>
        )}
        <Button variant="ghost" size="sm" onClick={onToggle}>
          {chain.enabled
            ? t("shared.gateway.fallback.chainDisable")
            : t("shared.gateway.fallback.chainEnable")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          data-act="gw-fallback-edit"
          onClick={onEdit}
          aria-label={t("shared.gateway.fallback.drawer.edit", { model: chain.from })}
        >
          ⋯
        </Button>
      </div>

      {/* 诊断带贴在链行**下方**，而不是弹 toast：降级链的问题几乎全是
          「配置对了但不会生效」，必须在配置的那一刻看得见。
          **一条目标可以有多条诊断**（闸门中 + 不支持图片同时成立），所以用 flatMap 而不是三目。 */}
      {chain.targets.flatMap((target, index) => {
        const hop = index + 1;
        const items: ReactNode[] = [];
        const wrap = (key: string, text: string) => (
          <div
            key={key}
            className="col-span-full mt-1 flex items-start gap-1.5 rounded-md border border-warning/25 bg-warning/10 px-2.5 py-1.5 text-[11px] leading-relaxed text-warning"
          >
            <TriangleAlert className="mt-0.5 size-3 shrink-0" />
            <span>{text}</span>
          </div>
        );
        if (target.state === "gated") {
          items.push(
            wrap(
              `${target.model}-gated`,
              t("shared.gateway.fallback.diag.gated", {
                hop,
                model: target.model,
                secs: target.gated_for_secs ?? 0,
              }),
            ),
          );
        } else if (target.state === "disabled") {
          items.push(
            wrap(
              `${target.model}-disabled`,
              t("shared.gateway.fallback.diag.disabled", { hop, model: target.model }),
            ),
          );
        }
        if (target.caps_lost.includes("image")) {
          items.push(
            wrap(
              `${target.model}-vision`,
              t("shared.gateway.fallback.diag.noVision", { hop, model: target.model }),
            ),
          );
        }
        return items;
      })}
    </div>
  );
}

function ModelChip({
  model,
  state,
  source,
}: {
  model: string;
  state: FallbackTargetState;
  source?: boolean;
}) {
  return (
    <span
      className={cn(
        "inline-flex max-w-full items-center gap-1.5 rounded-md border px-2 py-0.5 font-mono text-[11.5px]",
        source ? "border-primary/40 bg-primary/10" : "border-border bg-card-2",
      )}
    >
      <i className={cn("size-1.5 shrink-0 rounded-full", DOT_CLASS[state])} aria-hidden />
      <span className="truncate">{model}</span>
    </span>
  );
}

/* ─────────── 降级流水 ─────────── */

function EventsTable({
  events,
  scope,
}: {
  events: import("@/lib/types").FallbackEvent[];
  scope: string;
}) {
  const t = useT();
  // ★ 按作用域过滤：三个平台的降级共用一个进程级环形缓冲，
  //   不分家的话用户根本分不清哪条是哪条平台的。
  const scoped = events.filter((event) => event.scope === scope);
  if (!scoped.length) {
    return (
      <p className="border-t border-border px-4 py-6 text-center text-[11.5px] text-muted-foreground">
        {t("shared.gateway.fallback.event.empty")}
      </p>
    );
  }
  return (
    <div className="border-t border-border">
      <div className="overflow-x-auto">
        <table className="w-full text-[12px]">
          <thead>
            <tr className="border-b border-border text-[10.5px] uppercase tracking-wide text-muted-foreground">
              <th className="px-2.5 py-2 text-left font-medium">
                {t("shared.gateway.fallback.event.time")}
              </th>
              <th className="px-2.5 py-2 text-left font-medium">
                {t("shared.gateway.fallback.event.path")}
              </th>
              <th className="px-2.5 py-2 text-left font-medium">
                {t("shared.gateway.fallback.event.reason")}
              </th>
              <th className="px-2.5 py-2 text-left font-medium">
                {t("shared.gateway.fallback.event.account")}
              </th>
              <th className="px-2.5 py-2 text-left font-medium">
                {t("shared.gateway.fallback.event.cost")}
              </th>
              <th className="px-2.5 py-2 text-left font-medium">
                {t("shared.gateway.fallback.event.result")}
              </th>
            </tr>
          </thead>
          <tbody>
            {scoped.map((event, index) => (
              <tr
                key={`${event.at_ms}-${index}`}
                className="border-b border-border last:border-b-0"
              >
                <td className="whitespace-nowrap px-2.5 py-2 font-mono text-[11px] text-muted-foreground">
                  {new Date(event.at_ms).toLocaleTimeString()}
                </td>
                <td className="px-2.5 py-2 font-mono text-[11.5px]">
                  <span className="text-muted-foreground">{event.from}</span>
                  {event.path.length
                    ? event.path.map((model) => (
                        <span key={model}>
                          {" → "}
                          <b className="font-semibold">{model}</b>
                        </span>
                      ))
                    : " → —"}
                </td>
                <td className="px-2.5 py-2">
                  <Badge variant="secondary" className="text-[10px]">
                    {event.reason
                      ? t((REASON_LABEL_KEY[event.reason] ?? "shared.gateway.fallback.short.not_found") as never)
                      : "—"}
                  </Badge>
                </td>
                <td className="px-2.5 py-2 text-[11.5px] text-muted-foreground">
                  {event.account || "—"}
                </td>
                <td className="px-2.5 py-2 font-mono text-[11px] tabular-nums">
                  {(event.ms / 1000).toFixed(1)}s
                </td>
                <td className="px-2.5 py-2">
                  <Badge
                    variant="secondary"
                    className={cn(
                      "text-[10px]",
                      event.result === "degraded" && "bg-success/15 text-success",
                      event.result === "failed" && "bg-destructive/15 text-destructive",
                    )}
                  >
                    {event.result === "degraded"
                      ? t("shared.gateway.fallback.event.degraded")
                      : event.result === "failed"
                        ? t("shared.gateway.fallback.event.failed")
                        : t("shared.gateway.fallback.event.blocked")}
                  </Badge>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="px-4 py-2.5 text-[11px] leading-relaxed text-muted-foreground">
        {t("shared.gateway.fallback.event.note", { count: scoped.length })}
      </p>
    </div>
  );
}
