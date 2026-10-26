import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useLocation } from "react-router-dom";
import { ChevronDown, ChevronUp, Download, Loader2, Sparkles } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import { scopeFromLegacy } from "@/lib/scope";
import { cn } from "@/lib/utils";
import type { ModelProbeAdoptRecord, ModelProbeCell, ModelProbeJob, ModelProbeState, Region } from "@/lib/types";
import { poolOf } from "@/lib/gateway";
import { useGatewayStore, useProbeCells, useProbePassive, type ProbePassiveInfo } from "@/stores/gateway";

/**
 * 限流探测卡（方案 §5.1–§5.4；原型 `docs/prototype/demo-app.js` 的 `probeCardHtml()`）。
 *
 * ## 形态：**模型分区里的普通卡片**，不是弹层（硬约束，方案 2026-10-16 改版）
 *
 * 折叠 ⇄ 展开；卡内切「选择 / 进行中 / 结果」三态。矩阵天生要宽度，塞弹层会挤成一团。
 *
 * ## 三条语义（照原型行为契约实现，别自己发明交互）
 *
 * 1. **只有「模型限流／账号限流／模型封禁」算限流**；`no_credits`（积分不足）与
 *    `auth_dead`（登录失效）**必须分列**，绝不显示成「限流」。
 * 2. **不替用户选模型**（默认空选 + 按钮置灰 + 明确提示）；**账号默认全选**。
 * 3. **被动已知**（客户端台账）与**本次实测**视觉上可区分（虚线 = 未探测，内描边 = 被动已知）。
 *
 * ## 安全约束（★ 安全审计 S1 / S2）
 *
 * - **S1 恒定明示**：卡片上始终可见「将向每个账号各发一次最小请求，消耗极少积分、会在上游留痕」。
 *   它不是纯本地操作，用户有权知道代价。
 * - **S2 `reason` 只许纯文本**：上游原文是不可信输入（控制字符 / ANSI / 标记都可能出现），
 *   一律经 `title` 纯文本渲染，**禁止** `dangerouslySetInnerHTML`。
 *
 * ## 数据同源
 *
 * 实测读 `probe.cells`、被动读派生的 `useProbePassive(region)`（客户端台账 ∪ 池冷却；store 单一口径）
 * —— 模型表那一列读**同一份**，两处各自维护必然漂移（方案 §5.4）。
 */

/** 只有这三档算「限流」（402 / 登录失效分列）。 */
export const PROBE_LIMIT_STATES: readonly ModelProbeState[] = [
  "rate_limited_model",
  "rate_limited_account",
  "model_blocked",
];
const LIMIT_STATES = PROBE_LIMIT_STATES;

/** 该状态是否算「限流」（`unknown` 与其余六档都不算）。 */
function isLimitState(state: ModelProbeState | "unknown"): boolean {
  return (LIMIT_STATES as readonly string[]).includes(state);
}

/** 统计行的展示顺序（限流在前，可用在后）。 */
const TALLY_ORDER: readonly ModelProbeState[] = [
  "rate_limited_model",
  "rate_limited_account",
  "model_blocked",
  "upstream_queued",
  "no_credits",
  "auth_dead",
  "model_unavailable",
  "upstream_error",
  "transport_error",
  "ok",
  "skipped",
];

/** 图例条目（末两个 `unknown` / 被动已知单独渲染）。 */
const LEGEND_ORDER: readonly ModelProbeState[] = [
  "ok",
  "rate_limited_model",
  "rate_limited_account",
  "upstream_queued",
  "no_credits",
  "skipped",
];

/* ── 下钻：模型表徽标 → 本卡（同分区内两个兄弟组件的轻量协作）────────────────
   用模块级 pub/sub 而不是把 UI 状态塞进 store：下钻是**一次性的 UI 动作**，
   不是数据；两个组件同屏挂载，直接通信即可。 */
type FocusListener = (modelId: string) => void;
const focusListeners = new Set<FocusListener>();

/** 从模型表下钻：展开卡片并把探测范围收窄到这一个模型（导出给 `model-list.tsx`）。 */
export function focusProbeModel(modelId: string): void {
  for (const listener of focusListeners) listener(modelId);
}

/** 状态 → 单元格底色 + 边框（颜色 + 图标双编码，色盲可用）。 */
function stateCellClass(state: ModelProbeState | "unknown", kind: "probed" | "passive" | "unknown"): string {
  const tone: Record<string, string> = {
    ok: "border-success/60 bg-success/24",
    rate_limited_model: "border-warning/60 bg-warning/28",
    rate_limited_account: "border-destructive/58 bg-destructive/26",
    model_blocked: "border-destructive/58 bg-destructive/26",
    auth_dead: "border-destructive/58 bg-destructive/26",
    model_unavailable: "border-destructive/58 bg-destructive/26",
    no_credits: "border-warning/40 bg-warning/16",
    upstream_error: "border-warning/40 bg-warning/16",
    upstream_queued: "border-warning/40 bg-warning/16",
    transport_error: "border-warning/40 bg-warning/16",
    skipped: "border-border bg-muted",
    unknown: "border-border bg-muted",
  };
  return cn(
    "flex h-[22px] w-full min-w-[46px] items-center justify-center rounded-[5px] border text-[11px] leading-none",
    tone[state] ?? tone.unknown,
    kind === "unknown" && "border-dashed opacity-55",
    kind === "passive" && "shadow-[inset_0_0_0_2px] shadow-primary/50",
  );
}

/** 单元格内的字形（与颜色**双编码**；也满足无障碍「不能只靠颜色」）。 */
function stateGlyph(state: ModelProbeState | "unknown"): string {
  switch (state) {
    case "ok":
      return "✓";
    case "rate_limited_model":
      return "▲";
    case "rate_limited_account":
    case "model_blocked":
    case "auth_dead":
    case "model_unavailable":
      return "✕";
    case "no_credits":
    case "upstream_error":
    case "upstream_queued":
    case "transport_error":
      return "!";
    default:
      return "";
  }
}

/** 状态 → 语义色（模型表徽标与图例共用）。 */
export function probeStateVariant(state: ModelProbeState | "unknown"): "success" | "warning" | "destructive" | "secondary" {
  switch (state) {
    case "ok":
      return "success";
    case "rate_limited_model":
    case "no_credits":
    case "upstream_error":
    case "upstream_queued":
    case "transport_error":
      return "warning";
    case "rate_limited_account":
    case "model_blocked":
    case "auth_dead":
    case "model_unavailable":
      return "destructive";
    default:
      return "secondary";
  }
}

/** 被动来源 → 展示文案（§5.3 角标「真实流量」/「客户端日志」；两来源都有时说明二者皆有、以真实流量为准）。 */
function passiveSourceText(t: ReturnType<typeof useT>, info: ProbePassiveInfo | undefined): string {
  if (!info) return t("shared.gateway.probe.sourceUnknown");
  if (info.both) return t("shared.gateway.probe.sourcePassiveBoth");
  return info.source === "flow"
    ? t("shared.gateway.probe.sourcePassiveFlow")
    : t("shared.gateway.probe.sourcePassiveLedger");
}

export function ModelProbeCard({
  region,
  accounts: accountsProp,
  models: modelsProp,
  adoptSupported = true,
  className,
}: {
  region: Region;
  /**
   * 可选：**外部注入**的账号轴 / 模型轴（平台无关形状）。
   *
   * 缺省（WB 网关页）从 `useGatewayStore` 派生；Qoder 页**必须**注入自己的轴 ——
   * Qoder 的池 / 清单**不在** WB 网关 store 里。组件据此**不按平台分叉**：
   * 「列集合 / 账号轴由作用域决定」由**调用方**保证。
   */
  accounts?: { uid: string; name: string }[];
  models?: { id: string; name: string }[];
  /**
   * 该平台是否支持**回填账号池**（默认 `true`）。
   *
   * ★ Trae 传 `false`（T1：只探测、不回填）—— Trae 池**没有「模型级冷却」**，
   * 回填只能写**账号级**冷却，会把该账号的**其它模型一起打死**（方案 §4.5.4 的 T3，明确不建议）。
   * 此时开关**禁用**并给出原因文案，且启动时**强制 `adopt: false`** ——
   * 绝不能让用户以为回填生效了（后端 `ProbeTarget::Trae::adopt` 也已是空操作，双保险）。
   */
  adoptSupported?: boolean;
  className?: string;
}) {
  const t = useT();
  const location = useLocation();
  const status = useGatewayStore((s) => s.status);
  const snapshot = useGatewayStore((s) => s.models[region]);
  const probe = useGatewayStore((s) => s.probe);
  const probePassive = useProbePassive(region);
  const startProbe = useGatewayStore((s) => s.startProbe);
  const refreshProbeStatus = useGatewayStore((s) => s.refreshProbeStatus);
  const cancelProbe = useGatewayStore((s) => s.cancelProbe);
  const loadProbePassive = useGatewayStore((s) => s.loadProbePassive);
  /** 拉**落盘台账**：矩阵在重启后也要显示上次实测结论（与账号页台账卡同一份）。 */
  const loadProbeLedger = useGatewayStore((s) => s.loadProbeLedger);
  const stopProbePolling = useGatewayStore((s) => s.stopProbePolling);

  /**
   * 作用域：由 **region + 当前路由所属平台** 推出 —— `scopeFromLegacy` 是 `lib/scope.ts` 里
   * **现成**的 region → scope 转换（`scopeOf(platformOfPath(pathname), region)`）。
   * 组件不再自己拼 `"-"`（那会与 `PLATFORM_SLUG` 漂移）。
   */
  const scope = useMemo(
    () => scopeFromLegacy(location.pathname, region),
    [location.pathname, region],
  );

  const [expanded, setExpanded] = useState(false);
  const [modelsSel, setModelsSel] = useState<Set<string>>(() => new Set());
  const [accountsSel, setAccountsSel] = useState<Set<string>>(() => new Set());
  /** 结果态下点「重探」⇒ 回到选择态（不改后端作业）。 */
  const [setupOverride, setSetupOverride] = useState(false);
  const [starting, setStarting] = useState(false);
  /** 回填开关**默认 ON**（用户已拍板）。 */
  const [adopt, setAdopt] = useState(true);

  /* 账号轴：外部注入优先（Qoder），否则取该 region 的池条目（WB）。
     ★ 白屏防线（§5.7 / issue #2）：`name` 运行时可能是对象 —— 当 React 子节点渲染会卸载整棵树，
     故一律过 `displayText` 收敛成字符串（拿不到就回落 uid）。 */
  const accounts = useMemo(
    () =>
      accountsProp ??
      poolOf(status, region).accounts.map((a) => ({
        uid: a.uid,
        name: displayText(a.name) ?? displayText(a.uid) ?? "",
      })),
    [accountsProp, status, region],
  );
  /* 模型轴：外部注入优先（Qoder），否则取 WB 目录去重后的可用模型。
     ★ 同样过 `displayText`：`id` / `name` 都是后端清单里的展示字段，脏值会白屏。 */
  const models = useMemo(() => {
    if (modelsProp) return modelsProp;
    const out: { id: string; name: string }[] = [];
    const seen = new Set<string>();
    for (const m of snapshot?.models ?? []) {
      const id = displayText(m.id) ?? "";
      if (!id || seen.has(id)) continue;
      seen.add(id);
      out.push({ id, name: displayText(m.name) ?? id });
    }
    return out;
  }, [modelsProp, snapshot]);

  const accountUids = useMemo(() => accounts.map((a) => a.uid), [accounts]);
  const modelIds = useMemo(() => models.map((m) => m.id), [models]);
  const accountKey = accountUids.join("|");
  const modelKey = modelIds.join("|");

  /* 展开 / 换作用域时对齐目标轴并**保住**用户已有的选择（原型 `probeToggle`）。 */
  useEffect(() => {
    setAccountsSel((prev) => {
      const next = new Set([...prev].filter((uid) => accountUids.includes(uid)));
      // 账号**默认全选**：首次拿到池（或原有选中全失效）时补齐。
      if (next.size === 0) for (const uid of accountUids) next.add(uid);
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accountKey]);

  useEffect(() => {
    setModelsSel((prev) => new Set([...prev].filter((id) => modelIds.includes(id))));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelKey]);

  /* 进入 / 切作用域：拉一次作业快照 + 被动底色。卸载时清轮询（防泄漏）。 */
  useEffect(() => {
    void refreshProbeStatus(scope);
    void loadProbeLedger(scope);
    void loadProbePassive(region);
    return () => stopProbePolling();
  }, [scope, region, refreshProbeStatus, loadProbeLedger, loadProbePassive, stopProbePolling]);

  /* 订阅下钻（模型表徽标 → 本卡）。 */
  const phaseRef = useRef<"setup" | "running" | "done">("setup");
  useEffect(() => {
    const listener: FocusListener = (modelId) => {
      setExpanded(true);
      if (phaseRef.current !== "done") setSetupOverride(true);
      setModelsSel(new Set([modelId]));
    };
    focusListeners.add(listener);
    return () => {
      focusListeners.delete(listener);
    };
  }, []);

  const phase: "setup" | "running" | "done" =
    setupOverride && probe?.phase !== "running"
      ? "setup"
      : probe?.phase === "running"
        ? "running"
        : probe && (probe.phase === "done" || probe.phase === "cancelled")
          ? "done"
          : "setup";
  phaseRef.current = phase;

  /* ★ 实测格索引：`uid → model → cell`。
     **读 store 的跨作业累积值**（`useProbeCells`），不是本卡 `probe.cells` ——
     三个理由（口径全文见 `lib/limit-link.ts`）：
     1. 账号页「逐行实测」发起的作业也在这里显示（同一份数据，不是两份）；
     2. 在本卡里「重测这一格」不会把整张矩阵塌成 1 格（后端每次新作业从空 `cells` 开始）；
     3. 与模型表「限流（探测）」列同源，两处不再各自拼一份。 */
  const probedMap = useProbeCells(scope);

  const selectedAccounts = useMemo(
    () => accounts.filter((a) => accountsSel.has(a.uid)),
    [accounts, accountsSel],
  );
  const selectedModels = useMemo(() => models.filter((m) => modelsSel.has(m.id)), [models, modelsSel]);

  const stateOf = (
    uid: string,
    modelId: string,
  ): {
    state: ModelProbeState | "unknown";
    kind: "probed" | "passive" | "unknown";
    /** 被动来源（仅 `kind === "passive"` 时有）—— §5.3 角标「真实流量」/「客户端日志」。 */
    passive?: ProbePassiveInfo;
  } => {
    const cell = probedMap[uid]?.[modelId];
    if (cell) return { state: cell.state, kind: "probed" };
    const pas = probePassive[uid]?.[modelId];
    if (pas) return { state: pas.state, kind: "passive", passive: pas };
    return { state: "unknown", kind: "unknown" };
  };

  const tallies = useMemo(() => {
    const acc: Record<string, number> = {};
    for (const a of selectedAccounts)
      for (const m of selectedModels) {
        const key = stateOf(a.uid, m.id).state;
        acc[key] = (acc[key] ?? 0) + 1;
      }
    return acc;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAccounts, selectedModels, probedMap, probePassive]);

  const limitProbed = useMemo(() => {
    let probed = 0;
    let passive = 0;
    for (const a of selectedAccounts)
      for (const m of selectedModels) {
        const s = stateOf(a.uid, m.id);
        if (!isLimitState(s.state)) continue;
        if (s.kind === "probed") probed += 1;
        else if (s.kind === "passive") passive += 1;
      }
    return { probed, passive };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAccounts, selectedModels, probedMap, probePassive]);

  const gridCells = selectedAccounts.length * selectedModels.length;
  const canStart = selectedModels.length > 0 && selectedAccounts.length > 0;

  const resultTime =
    probe && probe.phase !== "running" && probe.finished_at_ms
      ? formatClock(probe.finished_at_ms)
      : null;
  const adoptedAdded = probe?.adopted_added ?? 0;
  const adoptedCleared = probe?.adopted_cleared ?? 0;

  const label = (state: ModelProbeState | "unknown") =>
    t(`shared.gateway.probe.state.${state}` as never);
  const reason = (state: ModelProbeState | "unknown") =>
    t(`shared.gateway.probe.reason.${state}` as never);

  async function onStart() {
    if (!canStart) return;
    setStarting(true);
    try {
      await startProbe({
        scope,
        uids: [...accountsSel],
        models: [...modelsSel],
        // 不支持回填的平台（Trae）**强制 false** —— 开关本就禁用，这里再兜一层。
        adopt: adoptSupported ? adopt : false,
      });
      setSetupOverride(false);
    } catch (e) {
      toast.error(t("shared.gateway.probe.startFail"), { description: String(e) });
    } finally {
      setStarting(false);
    }
  }

  async function onCancel() {
    try {
      await cancelProbe(scope);
    } catch {
      /* 取消失败不打断：下一次 refreshProbeStatus 会对齐真实相位。 */
    }
  }

  /**
   * 矩阵里「重测这一格」：**只打这一格、恒 `adopt: false`**。
   *
   * - 只打一格：`uids:[uid] models:[model]` ⇒ 后端 1 格（不铺笛卡尔积）；
   * - 不回填：这是**查证**动作，和账号页的入口同口径。要回填走卡内「开始探测」的开关
   *   （那里有明示代价与逐条明细）。
   * - 不会把矩阵打塌：显示读的是跨作业累积索引（`useProbeCells`），见 `probedMap` 的注释。
   */
  async function onProbeCell(uid: string, model: string) {
    try {
      await startProbe({ scope, uids: [uid], models: [model], adopt: false });
      setSetupOverride(false);
    } catch (e) {
      toast.error(t("shared.gateway.probe.startFail"), { description: String(e) });
      void refreshProbeStatus(scope);
    }
  }

  function onExportCsv() {
    const rows: string[][] = [[
      t("shared.gateway.probe.csv.account"),
      t("shared.gateway.probe.csv.model"),
      t("shared.gateway.probe.csv.state"),
      t("shared.gateway.probe.csv.source"),
      t("shared.gateway.probe.csv.note"),
    ]];
    for (const a of selectedAccounts)
      for (const m of selectedModels) {
        const s = stateOf(a.uid, m.id);
        const src =
          s.kind === "probed"
            ? t("shared.gateway.probe.sourceProbed")
            : s.kind === "passive"
              ? passiveSourceText(t, s.passive)
              : t("shared.gateway.probe.sourceUnknown");
        // 说明列：实测用**上游原文**（不可信输入，过 `displayText` 防脏值），其余用分档文案。
        const note =
          s.kind === "probed"
            ? displayText(probedMap[a.uid][m.id].reason) ?? reason(s.state)
            : reason(s.state);
        rows.push([a.name, m.name, label(s.state), src, note]);
      }
    const csv = rows.map((r) => r.map(csvCell).join(",")).join("\n");
    downloadCsv(`model-probe-${region}.csv`, csv);
    toast.success(t("shared.gateway.probe.exportDone"), { description: t("shared.gateway.probe.exportRows", { n: rows.length - 1 }) });
  }

  /* ── 卡头（折叠态也渲染）+ S1 恒定明示 ────────────────────────────── */
  const header = (
    <div className="flex items-center gap-3">
      <h3 className="text-sm font-semibold">{t("shared.gateway.probe.title")}</h3>
      <span className="rounded-md border border-border px-1.5 py-0.5 text-[10.5px] text-muted-foreground">
        {t("shared.gateway.probe.badge")}
      </span>
      <div className="grow" />
      <span className="text-xs text-muted-foreground">
        {resultTime
          ? adoptSupported
            ? /* 本次**没回填**（账号页发起的查证，或用户关了开关）⇒ 别显示「置 0 / 清 0」，
                 那读起来像「跑了但什么都没发生」，而不是「刻意不写池」。 */
              probe && !probe.adopt
              ? t("shared.gateway.probe.summaryLastNoAdopt", { time: resultTime })
              : t("shared.gateway.probe.summaryLast", {
                  time: resultTime,
                  added: adoptedAdded,
                  cleared: adoptedCleared,
                })
            : t("shared.gateway.probe.summaryLastPlain", { time: resultTime })
          : t("shared.gateway.probe.summaryAvailable", {
              accounts: accounts.length,
              models: models.length,
            })}
      </span>
      <Button variant="ghost" size="sm" onClick={() => setExpanded((v) => !v)}>
        {expanded ? <ChevronUp /> : <ChevronDown />}
        {expanded ? t("shared.gateway.probe.collapse") : t("shared.gateway.probe.expand")}
      </Button>
    </div>
  );

  /* S1：恒定明示（不可省，安全审计硬约束）。 */
  const notice = (
    <p className="mt-2 text-[11.5px] leading-relaxed text-muted-foreground" data-act="gw-probe-notice">
      {t("shared.gateway.probe.notice")}
    </p>
  );

  return (
    <Card className={cn("gap-0 py-0", className)} data-act="gw-probe-card">
      <div className="border-b border-border/60 px-5 py-3">
        {header}
        {notice}
      </div>
      {expanded && (
        <div className="px-5 py-4">
          {phase === "running" && probe ? (
            <RunningBody
              t={t}
              job={probe}
              tallies={tallies}
              label={label}
              adopt={adoptSupported && adopt}
              onCancel={() => void onCancel()}
            />
          ) : phase === "done" && probe ? (
            <DoneBody
              t={t}
              accounts={selectedAccounts}
              models={selectedModels}
              stateOf={stateOf}
              label={label}
              reason={reason}
              probedMap={probedMap}
              gridCells={gridCells}
              limitProbed={limitProbed}
              resultTime={resultTime}
              added={adoptedAdded}
              cleared={adoptedCleared}
              adopted={probe.adopted}
              adopt={adoptSupported && adopt}
              cancelled={probe.phase === "cancelled"}
              onExport={onExportCsv}
              onRetry={() => setSetupOverride(true)}
              onProbeCell={(uid, model) => void onProbeCell(uid, model)}
            />
          ) : (
            <SetupBody
              t={t}
              models={models}
              accounts={accounts}
              modelsSel={modelsSel}
              accountsSel={accountsSel}
              onToggleModel={(id) =>
                setModelsSel((prev) => {
                  const next = new Set(prev);
                  if (next.has(id)) next.delete(id);
                  else next.add(id);
                  return next;
                })
              }
              onToggleAccount={(uid) =>
                setAccountsSel((prev) => {
                  const next = new Set(prev);
                  if (next.has(uid)) next.delete(uid);
                  else next.add(uid);
                  return next;
                })
              }
              onAllModels={(on) => setModelsSel(on ? new Set(modelIds) : new Set())}
              onAllAccounts={(on) => setAccountsSel(on ? new Set(accountUids) : new Set())}
              adopt={adopt}
              adoptSupported={adoptSupported}
              onAdopt={setAdopt}
              gridCells={gridCells}
              canStart={canStart}
              starting={starting}
              onStart={() => void onStart()}
            />
          )}
        </div>
      )}
    </Card>
  );
}

/* ══════════════════════════ 选择态 ══════════════════════════ */

function SetupBody(props: {
  t: ReturnType<typeof useT>;
  models: { id: string; name: string }[];
  accounts: { uid: string; name: string }[];
  modelsSel: Set<string>;
  accountsSel: Set<string>;
  onToggleModel: (id: string) => void;
  onToggleAccount: (uid: string) => void;
  onAllModels: (on: boolean) => void;
  onAllAccounts: (on: boolean) => void;
  adopt: boolean;
  /** 该平台是否支持回填（Trae 为 `false` ⇒ 开关禁用并说明原因）。 */
  adoptSupported: boolean;
  onAdopt: (on: boolean) => void;
  gridCells: number;
  canStart: boolean;
  starting: boolean;
  onStart: () => void;
}) {
  const { t, models, accounts, modelsSel, accountsSel } = props;
  const allM = models.length > 0 && modelsSel.size === models.length;
  const allA = accounts.length > 0 && accountsSel.size === accounts.length;
  return (
    <>
      <StepHeader
        t={t}
        n={1}
        title={t("shared.gateway.probe.stepModels")}
        selected={t("shared.gateway.probe.selected", { n: modelsSel.size, total: models.length })}
        all={allM}
        onAll={() => props.onAllModels(!allM)}
      />
      <PickGrid
        empty={t("shared.gateway.probe.noModels")}
        items={models.map((m) => ({ key: m.id, label: m.name }))}
        selected={modelsSel}
        onToggle={props.onToggleModel}
      />
      <StepHeader
        t={t}
        n={2}
        title={t("shared.gateway.probe.stepAccounts")}
        selected={t("shared.gateway.probe.selected", { n: accountsSel.size, total: accounts.length })}
        all={allA}
        onAll={() => props.onAllAccounts(!allA)}
      />
      <PickGrid
        empty={t("shared.gateway.probe.noAccounts")}
        items={accounts.map((a) => ({ key: a.uid, label: a.name }))}
        selected={accountsSel}
        onToggle={props.onToggleAccount}
      />

      <div
        className="mt-1 flex items-center gap-3 rounded-[10px] border border-border bg-muted px-3 py-2.5"
        data-act="gw-probe-adopt"
      >
        <div className="min-w-0 flex-1">
          <b className="block text-[12.5px] font-medium">
            {props.adoptSupported
              ? t("shared.gateway.probe.adoptTitle")
              : t("shared.gateway.probe.adoptUnsupportedTitle")}
          </b>
          <span className="mt-0.5 block text-[11.5px] leading-relaxed text-muted-foreground">
            {props.adoptSupported
              ? t("shared.gateway.probe.adoptDesc")
              : t("shared.gateway.probe.adoptUnsupportedDesc")}
          </span>
        </div>
        {/* 不支持回填的平台（Trae）：开关**禁用**并恒为关（绝不让用户以为回填生效了）。 */}
        <Switch
          checked={props.adoptSupported && props.adopt}
          disabled={!props.adoptSupported}
          onCheckedChange={props.onAdopt}
          aria-label={t("shared.gateway.probe.adoptAria")}
        />
      </div>

      <div className="mt-4 flex items-center gap-2 border-t border-border pt-4">
        <span className="text-xs text-muted-foreground">
          {props.canStart
            ? t("shared.gateway.probe.footCells", {
                accounts: accountsSel.size,
                models: modelsSel.size,
                cells: props.gridCells,
              })
            : t("shared.gateway.probe.pickModelFirst")}
        </span>
        <div className="grow" />
        <Button size="sm" disabled={!props.canStart || props.starting} onClick={props.onStart}>
          {props.starting ? <Loader2 className="animate-spin" /> : <Sparkles />}
          {t("shared.gateway.probe.start")}
        </Button>
      </div>
    </>
  );
}

function StepHeader(props: {
  t: ReturnType<typeof useT>;
  n: number;
  title: string;
  selected: string;
  all: boolean;
  onAll: () => void;
}) {
  return (
    <div className="mb-2.5 flex items-center gap-2.5 text-[12.5px]">
      <span className="grid size-[18px] shrink-0 place-items-center rounded-full bg-primary/18 text-[10.5px] font-semibold text-primary">
        {props.n}
      </span>
      <b className="font-medium">{props.title}</b>
      <div className="grow" />
      <span className="text-xs text-muted-foreground">{props.selected}</span>
      <Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={props.onAll}>
        {props.all ? props.t("shared.gateway.probe.clearAll") : props.t("shared.gateway.probe.selectAll")}
      </Button>
    </div>
  );
}

function PickGrid(props: {
  empty: string;
  items: { key: string; label: string }[];
  selected: Set<string>;
  onToggle: (key: string) => void;
}) {
  if (props.items.length === 0) {
    return <p className="mb-5 text-xs text-muted-foreground">{props.empty}</p>;
  }
  return (
    <div className="mb-5 grid gap-2 [grid-template-columns:repeat(auto-fill,minmax(196px,1fr))]">
      {props.items.map((item) => {
        const on = props.selected.has(item.key);
        return (
          <button
            key={item.key}
            type="button"
            role="checkbox"
            aria-checked={on}
            onClick={() => props.onToggle(item.key)}
            className={cn(
              "flex min-w-0 items-center gap-2.5 rounded-[9px] border px-2.5 py-2 text-left text-[12.5px] transition-colors",
              on
                ? "border-primary/45 bg-primary/10 text-foreground"
                : "border-border text-muted-foreground hover:border-border hover:bg-muted hover:text-foreground",
            )}
          >
            <span
              aria-hidden="true"
              className={cn(
                "grid size-[15px] shrink-0 place-items-center rounded-[4px] border-[1.5px]",
                on ? "border-primary bg-primary text-primary-foreground" : "border-border",
              )}
            >
              {on ? "✓" : ""}
            </span>
            <span className="min-w-0 truncate" title={item.label}>
              {item.label}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/* ══════════════════════════ 进行态 ══════════════════════════ */

function RunningBody(props: {
  t: ReturnType<typeof useT>;
  job: ModelProbeJob;
  tallies: Record<string, number>;
  label: (s: ModelProbeState | "unknown") => string;
  adopt: boolean;
  onCancel: () => void;
}) {
  const { t, job } = props;
  const pct = job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
  return (
    <>
      <div className="mb-2.5 flex items-center gap-2.5 text-[12.5px]">
        <span className="text-primary">▶</span>
        <b className="font-medium tabular-nums">
          {job.done} / {job.total}
        </b>
        <div className="grow" />
        <span className="text-xs text-muted-foreground">
          {t("shared.gateway.probe.runningCurrent", { current: displayText(job.current) || "…" })}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full border border-border bg-muted">
        <i className="block h-full bg-primary transition-[width] duration-150" style={{ width: `${pct}%` }} />
      </div>
      <div className="mt-3.5 flex flex-wrap gap-x-[18px] gap-y-1.5 text-xs text-muted-foreground">
        {TALLY_ORDER.filter((k) => (props.tallies[k] ?? 0) > 0).map((k) => {
          const hot = LIMIT_STATES.includes(k);
          return (
            <span key={k} className="inline-flex items-center gap-1.5">
              <i className={cn("size-2.5 rounded-[3px] border border-border", STAT_DOT[k] ?? "")} />
              {props.label(k)}
              <b className={cn("font-medium tabular-nums", hot && "text-warning")}>{props.tallies[k]}</b>
            </span>
          );
        })}
      </div>
      <p className="mt-3 text-xs leading-relaxed text-muted-foreground">{t("shared.gateway.probe.runningNote")}</p>
      <div className="mt-4 flex items-center gap-2 border-t border-border pt-4">
        <span className="text-xs text-muted-foreground">
          {props.adopt ? t("shared.gateway.probe.adoptOn") : t("shared.gateway.probe.adoptOff")}
        </span>
        <div className="grow" />
        <Button size="sm" onClick={props.onCancel}>
          {t("shared.gateway.probe.cancel")}
        </Button>
      </div>
    </>
  );
}

/* ══════════════════════════ 结果态 ══════════════════════════ */

/** 矩阵列的排序口径（§5.3：可点表头排序，默认按「限流数」降序）。 */
type ProbeSortKey = "throttle" | "name";

function DoneBody(props: {
  t: ReturnType<typeof useT>;
  accounts: { uid: string; name: string }[];
  models: { id: string; name: string }[];
  stateOf: (uid: string, modelId: string) => {
    state: ModelProbeState | "unknown";
    kind: "probed" | "passive" | "unknown";
    passive?: ProbePassiveInfo;
  };
  label: (s: ModelProbeState | "unknown") => string;
  reason: (s: ModelProbeState | "unknown") => string;
  probedMap: Record<string, Record<string, ModelProbeCell>>;
  gridCells: number;
  limitProbed: { probed: number; passive: number };
  resultTime: string | null;
  added: number;
  cleared: number;
  /** 回填**逐条明细**（后端唯一来源；§5.3「回填反馈」可展开看它）。 */
  adopted: ModelProbeAdoptRecord[];
  /** 本次是否**开着回填**（关掉时不该解释「为什么没写入」，那是刻意的）。 */
  adopt: boolean;
  cancelled: boolean;
  onExport: () => void;
  onRetry: () => void;
  /** 重测/首测这一格（`uids:[uid] models:[model]`、不回填）。 */
  onProbeCell: (uid: string, model: string) => void;
}) {
  const { t, accounts, models, stateOf } = props;
  const hasAdopt = props.added > 0 || props.cleared > 0;
  /**
   * 回填明细的**账号名 / 模型名走本卡已有的归一轴**（`accounts`：uid→name；`models`：id→name，
   * 二者都在父组件里过过 `displayText`），拿不到再回落记录自带字段、最后回落 id 本身。
   * ⚠️ 不绕过归一：记录的 `nickname` 也是展示字段，可能是脏值（issue #2 白屏防线）。
   */
  const adoptedAccountName = useMemo(() => new Map(accounts.map((a) => [a.uid, a.name])), [accounts]);
  const adoptedModelName = useMemo(() => new Map(models.map((m) => [m.id, m.name])), [models]);
  /** 排序口径（**默认「按限流账号数」降序**；§5.3）。 */
  const [sortBy, setSortBy] = useState<ProbeSortKey>("throttle");

  /** 某模型被限流的账号数（行汇总的「列版」）。 */
  const limitAccountsOf = (modelId: string): number => {
    let n = 0;
    for (const a of accounts) if (isLimitState(stateOf(a.uid, modelId).state)) n += 1;
    return n;
  };

  /* 列序：默认按限流账号数**降序**；切换后按模型名；同值时用另一维度稳定次序（避免抖动）。 */
  const orderedModels = [...models];
  orderedModels.sort((a, b) => {
    if (sortBy === "name") {
      const byName = a.name.localeCompare(b.name);
      return byName !== 0 ? byName : limitAccountsOf(b.id) - limitAccountsOf(a.id);
    }
    const byLimit = limitAccountsOf(b.id) - limitAccountsOf(a.id);
    return byLimit !== 0 ? byLimit : a.name.localeCompare(b.name);
  });

  /* 空态（§5.3）：整张矩阵全 `ok` ⇒ 显式「全部可用」；全 `skipped` ⇒ 去登录 / 导入的指引。 */
  let total = 0;
  let okCount = 0;
  let skippedCount = 0;
  for (const a of accounts)
    for (const m of models) {
      total += 1;
      const st = stateOf(a.uid, m.id).state;
      if (st === "ok") okCount += 1;
      else if (st === "skipped") skippedCount += 1;
    }
  const allOk = total > 0 && okCount === total;
  const allSkipped = total > 0 && skippedCount === total;

  /** 完成条内容（有 / 无回填明细两种渲染共用）。 */
  const doneBar = (
    <>
      <span className="text-success">✓</span>
      <b className="font-medium">{t("shared.gateway.probe.doneCells", { n: props.gridCells })}</b>
      <span className="text-xs text-muted-foreground">
        {props.cancelled ? t("shared.gateway.probe.cancelled") : t("shared.gateway.probe.done")}
        {hasAdopt ? ` · ${t("shared.gateway.probe.adoptResult", { added: props.added, cleared: props.cleared })}` : ""}
      </span>
    </>
  );

  return (
    <>
      {/**
       * 完成条（结果态）。§5.3「回填反馈」：完成条上显示回填计数 + **可展开看明细**。
       * ⚠️ 明细为空（`adopted.length === 0`）时**不渲染**这个可展开控件 —— 不给空壳。
       */}
      {props.adopted.length > 0 ? (
        <Collapsible className="mb-2.5">
          <div className="flex items-center gap-2.5 text-[12.5px]">
            {doneBar}
            <CollapsibleTrigger className="group inline-flex items-center gap-1 rounded-md border border-border px-1.5 py-0.5 text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring">
              {t("shared.gateway.probe.adoptedDetail", { n: props.adopted.length })}
              <ChevronDown className="size-3.5 transition-transform group-data-[state=open]:rotate-180" />
            </CollapsibleTrigger>
          </div>
          <CollapsibleContent className="mt-2">
            <ul
              data-act="gw-probe-adopt-detail"
              className="flex flex-col gap-1 rounded-[8px] border border-border bg-muted px-3 py-2 text-[11.5px]"
            >
              {props.adopted.map((rec, i) => {
                const added = rec.action === "added";
                // 账号 / 模型名走本卡归一轴；拿不到再回落记录字段、最后回落 id。
                const account = adoptedAccountName.get(rec.uid) || displayText(rec.nickname) || rec.uid;
                const model = adoptedModelName.get(rec.model) || displayText(rec.model) || rec.model;
                return (
                  <li
                    key={`${rec.uid}|${rec.model}|${rec.action}|${i}`}
                    className="flex flex-wrap items-baseline gap-x-1.5 gap-y-0.5"
                  >
                    <span className="font-medium text-foreground">{account}</span>
                    <span className="text-muted-foreground">·</span>
                    <span className="text-foreground">{model}</span>
                    <span className="text-muted-foreground">·</span>
                    <span className={cn("font-medium", added ? "text-warning" : "text-success")}>
                      {added ? t("shared.gateway.probe.adoptedAdded") : t("shared.gateway.probe.adoptedCleared")}
                    </span>
                    {/* `cleared` 不显示时间（`until_ms` 恒 0）；`added` 用本卡同一套时间格式化。 */}
                    {added && rec.until_ms > 0 && (
                      <span className="tabular-nums text-muted-foreground">
                        {t("shared.gateway.probe.adoptedUntil", { time: formatDateTime(rec.until_ms) })}
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>
          </CollapsibleContent>
        </Collapsible>
      ) : (
        <div className="mb-2.5 flex items-center gap-2.5 text-[12.5px]">{doneBar}</div>
      )}

      {/* ★ 回填 0 条时**必须解释**（issue #88 · E）。
       *
       * 「探出 3 格限流、回填 0 条」是最需要解释的组合，而旧实现此时**一个字都不渲染**
       * （可展开明细只在 `adopted.length > 0` 时才有），用户只能认为回填坏了。
       * 三种静默不计的原因见后端 `Pool::adopt_probe_result` 的三处 `return false`：
       * 池里没这个 uid / 账号已禁用 / 已有更晚的冷却。 */}
      {props.adopt && !hasAdopt && props.limitProbed.probed > 0 ? (
        <p
          className="mb-2.5 text-[11.5px] leading-relaxed text-muted-foreground"
          data-act="gw-probe-adopt-none"
        >
          {t("shared.gateway.probe.adoptNone", { n: props.limitProbed.probed })}
        </p>
      ) : null}

      {/* 图例（两级被动词条区分来源） + 排序控件（§5.3；用 Radix Select，不用 details/summary）。 */}
      <div className="mb-2.5 flex flex-wrap items-center gap-x-3.5 gap-y-1.5 text-[11.5px] text-muted-foreground">
        {LEGEND_ORDER.map((k) => (
          <span key={k} className="inline-flex items-center gap-1.5">
            <i className={cn("size-2.5 rounded-[3px] border border-border", STAT_DOT[k] ?? "")} />
            {props.label(k)}
          </span>
        ))}
        <span className="inline-flex items-center gap-1.5">
          <i className="size-2.5 rounded-[3px] border border-dashed border-border" />
          {t("shared.gateway.probe.legendUnknown")}
        </span>
        <span className="inline-flex items-center gap-1.5">
          <i className="size-2.5 rounded-[3px] border border-border shadow-[inset_0_0_0_2px] shadow-primary/50" />
          {t("shared.gateway.probe.legendPassiveFlow")}
        </span>
        <span className="inline-flex items-center gap-1.5">
          <i className="size-2.5 rounded-[3px] border border-border shadow-[inset_0_0_0_2px] shadow-primary/50" />
          {t("shared.gateway.probe.legendPassiveLedger")}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <span>{t("shared.gateway.probe.sortLabel")}</span>
          <Select value={sortBy} onValueChange={(v) => setSortBy(v as ProbeSortKey)}>
            <SelectTrigger
              size="sm"
              className="h-6 gap-1 px-2 text-[11.5px]"
              aria-label={t("shared.gateway.probe.sortAria")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="throttle">{t("shared.gateway.probe.sortThrottle")}</SelectItem>
              <SelectItem value="name">{t("shared.gateway.probe.sortName")}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* 空态横幅（§5.3）：全 ok / 全 skipped 各一条。 */}
      {allOk && (
        <p
          className="mb-2.5 rounded-[8px] border border-success/40 bg-success/16 px-3 py-2 text-xs text-success"
          data-act="gw-probe-all-ok"
        >
          {t("shared.gateway.probe.emptyAllOk")}
        </p>
      )}
      {allSkipped && (
        <div
          className="mb-2.5 rounded-[8px] border border-border bg-muted px-3 py-2 text-xs leading-relaxed text-muted-foreground"
          data-act="gw-probe-all-skipped"
        >
          <b className="block text-foreground">{t("shared.gateway.probe.emptyAllSkippedTitle")}</b>
          <span className="mt-0.5 block">{t("shared.gateway.probe.emptyAllSkippedDesc")}</span>
        </div>
      )}

      {accounts.length === 0 || models.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t("shared.gateway.probe.matrixEmpty")}</p>
      ) : (
        <div className="overflow-auto rounded-[10px] border border-border bg-muted p-1.5">
          <table className="border-separate border-spacing-[3px] text-[11.5px]">
            <thead>
              <tr>
                <th className="px-1.5 py-0.5 text-left font-normal text-muted-foreground">
                  {t("shared.gateway.probe.colAccount")}
                </th>
                {orderedModels.map((m) => (
                  <th key={m.id} className="max-w-24 truncate px-1.5 py-0.5 text-center font-normal text-muted-foreground" title={m.name}>
                    {m.name}
                  </th>
                ))}
                <th className="px-2 py-0.5 text-right font-normal text-muted-foreground">
                  {t("shared.gateway.probe.sumLimit")}
                </th>
              </tr>
            </thead>
            <tbody>
              {accounts.map((a) => {
                let hot = 0;
                const cells = orderedModels.map((m) => {
                  const s = stateOf(a.uid, m.id);
                  if (isLimitState(s.state)) hot += 1;
                  return s;
                });
                return (
                  <tr key={a.uid}>
                    <td className="whitespace-nowrap px-1.5 py-0.5">{a.name}</td>
                    {orderedModels.map((m, i) => {
                      const s = cells[i];
                      const cell = s.kind === "probed" ? props.probedMap[a.uid][m.id] : undefined;
                      // 来源：实测 / 被动（两来源经 `passiveSourceText` 区分）/ 未探测。
                      const src =
                        s.kind === "probed"
                          ? t("shared.gateway.probe.sourceProbed")
                          : s.kind === "passive"
                            ? passiveSourceText(t, s.passive)
                            : t("shared.gateway.probe.sourceUnknown");
                      // ★ S2 / §5.7：上游原文是不可信输入 —— 过 `displayText` 防脏值，纯文本渲染。
                      const reasonText = cell ? displayText(cell.reason) ?? "" : "";
                      const title = t("shared.gateway.probe.cellTitle", {
                        account: a.name,
                        model: m.name,
                        state: props.label(s.state),
                        source: src,
                        reason: cell ? reasonText || props.reason(s.state) : props.reason(s.state),
                      });
                      const detail = (
                        <div className="space-y-1.5">
                          <div className="flex items-center gap-1.5 font-medium">
                            <i className={cn("size-2.5 rounded-[3px] border border-border", STAT_DOT[s.state] ?? "")} />
                            {props.label(s.state)}
                          </div>
                          <div className="text-muted-foreground">
                            {t("shared.gateway.probe.detailSource")}：{src}
                          </div>
                          {cell && (
                            <>
                              <div>
                                {t("shared.gateway.probe.detailHttp")}：{cell.http_status}
                              </div>
                              <div>
                                {t("shared.gateway.probe.detailLatency")}：{cell.latency_ms} ms
                              </div>
                              {cell.reset_at_ms != null && (
                                <div>
                                  {t("shared.gateway.probe.detailReset")}：{formatDateTime(cell.reset_at_ms)}
                                </div>
                              )}
                              {reasonText && (
                                <div className="border-t border-border/60 pt-1.5 break-words whitespace-pre-wrap">
                                  <span className="text-muted-foreground">
                                    {t("shared.gateway.probe.detailReason")}：
                                  </span>
                                  {reasonText}
                                </div>
                              )}
                            </>
                          )}
                          <div className="border-t border-border/60 pt-1.5 text-muted-foreground">
                            {props.reason(s.state)}
                          </div>
                          {/* ★ 入口③：只重测这一格（与账号页「实测」同口径：单格、不回填）。
                              矩阵里「这一格是刚探的还是几小时前的」肉眼分不出 ⇒ 给重测入口。 */}
                          <div className="border-t border-border/60 pt-1.5">
                            <Button
                              size="sm"
                              variant="ghost"
                              className="h-6 px-2 text-[11.5px]"
                              onClick={() => props.onProbeCell(a.uid, m.id)}
                            >
                              {cell
                                ? t("shared.gateway.probe.reprobeCell")
                                : t("shared.gateway.probe.probeCell")}
                            </Button>
                          </div>
                        </div>
                      );
                      return (
                        <td key={m.id}>
                          <MatrixCell
                            aria={title}
                            title={title}
                            className={stateCellClass(s.state, s.kind)}
                            glyph={stateGlyph(s.state)}
                            detail={detail}
                          />
                        </td>
                      );
                    })}
                    <td className={cn("whitespace-nowrap px-2 py-0.5 text-right tabular-nums", hot ? "font-medium text-warning" : "text-muted-foreground")}>
                      {hot || "—"}
                    </td>
                  </tr>
                );
              })}
              <tr>
                <td className="whitespace-nowrap px-1.5 py-0.5 font-normal text-muted-foreground">
                  {t("shared.gateway.probe.rowLimitAccounts")}
                </td>
                {orderedModels.map((m) => {
                  const n = limitAccountsOf(m.id);
                  return (
                    <td key={m.id} className={cn("whitespace-nowrap px-2 py-0.5 text-right tabular-nums", n ? "font-medium text-warning" : "text-muted-foreground")}>
                      {n}
                    </td>
                  );
                })}
                <td />
              </tr>
            </tbody>
          </table>
        </div>
      )}

      <p className="mt-3 text-xs leading-relaxed text-muted-foreground">
        {t("shared.gateway.probe.resultProbed", { probed: props.limitProbed.probed })}
        {props.limitProbed.passive > 0 ? ` ${t("shared.gateway.probe.resultPassive", { passive: props.limitProbed.passive })}` : ""}
        {` ${t("shared.gateway.probe.resultHint")}`}
      </p>

      <div className="mt-4 flex items-center gap-2 border-t border-border pt-4">
        <span className="text-xs text-muted-foreground">
          {t("shared.gateway.probe.probedAt", { time: props.resultTime ?? "—" })}
        </span>
        <div className="grow" />
        <Button variant="ghost" size="sm" onClick={props.onExport}>
          <Download />
          {t("shared.gateway.probe.exportCsv")}
        </Button>
        <Button size="sm" onClick={props.onRetry}>
          {t("shared.gateway.probe.retry")}
        </Button>
      </div>
    </>
  );
}

/**
 * 矩阵单元格（§5.3 / §5.7）：颜色 + 字形**双编码**；`title` / `aria-label` 纯文本（S2）。
 *
 * - 键盘可聚焦（`tabIndex={0}`）+ `role="img"` + 完整 `aria-label`（§5.7 既有成果，别改坏）；
 * - **点击 / Enter / Space ⇒ 详情气泡**（Radix `Popover`，不手搓定位）。
 */
function MatrixCell(props: {
  aria: string;
  title: string;
  className: string;
  glyph: string;
  detail: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <span
          tabIndex={0}
          role="img"
          aria-label={props.aria}
          title={props.title}
          aria-haspopup="dialog"
          aria-expanded={open}
          className={cn(
            props.className,
            "cursor-pointer focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring",
            open && "ring-2 ring-ring",
          )}
          onClick={() => setOpen((v) => !v)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              setOpen((v) => !v);
            }
          }}
        >
          {props.glyph}
        </span>
      </PopoverAnchor>
      <PopoverContent align="start" className="w-[288px] p-3 text-xs">
        {props.detail}
      </PopoverContent>
    </Popover>
  );
}

/* ── 统计点色（与矩阵/图例同色系） ── */
const STAT_DOT: Record<string, string> = {
  ok: "bg-success/50",
  rate_limited_model: "bg-warning/60",
  rate_limited_account: "bg-destructive/55",
  model_blocked: "bg-destructive/55",
  auth_dead: "bg-destructive/55",
  model_unavailable: "bg-destructive/55",
  no_credits: "bg-warning/35",
  upstream_error: "bg-warning/35",
  upstream_queued: "bg-warning/35",
  transport_error: "bg-warning/35",
  skipped: "bg-muted",
  unknown: "bg-muted",
};

/* ── 小工具 ── */

function formatClock(ms: number): string {
  try {
    return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "—";
  }
}

/** `resetAt` 用：本地化的「年-月-日 时:分:秒」（探测详情气泡里展示上游给出的解冻时刻）。 */
function formatDateTime(ms: number): string {
  try {
    return new Date(ms).toLocaleString(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return "—";
  }
}

function csvCell(value: string): string {
  const s = value ?? "";
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

function downloadCsv(filename: string, content: string): void {
  const blob = new Blob([`\ufeff${content}`], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
