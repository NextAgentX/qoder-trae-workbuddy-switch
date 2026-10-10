import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, CheckCircle2, Clock, Info, Loader2, Plus, RefreshCw } from "lucide-react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { toast } from "sonner";

import { OverviewStats } from "@/components/overview/overview-stats";
import { TokenTrendCard } from "@/components/overview/token-trend-card";
import { ScopeMark, scopeLabel } from "@/components/scope-chip";
import { RunPill } from "@/components/shell/run-pill";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { formatCredits } from "@/components/usage/format";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import { useOverviewMetrics } from "@/lib/overview-metrics";
import {
  platformDescriptor,
  platformOfPath,
  type Capability,
} from "@/lib/platforms/registry";
import { SCOPES, scopeOf, scopeParts, writeLastScope, type Scope } from "@/lib/scope";
import { scopeShortcutTarget } from "@/lib/scope-shortcuts";
import { useScope } from "@/lib/use-scope";
import { useScopeStatus, type ScopeStatus } from "@/lib/use-scope-status";
import { cn } from "@/lib/utils";
import { useAccountsStore } from "@/stores/accounts";

/** Token 到期前的预警窗口：24 小时。 */
const EXPIRY_WARN_MS = 24 * 60 * 60 * 1000;

/** 能力 → 文案键（取值来自注册表 `Capability`；新增能力必须同时补词条）。 */
const CAPABILITY_LABEL: Record<Capability, TranslationKey> = {
  checkin: "capability.checkin",
  travel: "capability.travel",
  credits: "capability.credits",
  oauth: "capability.oauth",
  gateway: "capability.gateway",
  autoRotate: "capability.autoRotate",
  sessionCopy: "capability.sessionCopy",
};

interface PendingItem {
  key: string;
  icon: typeof AlertTriangle;
  tone: "warn" | "danger" | "info";
  title: string;
  hint: string;
  /**
   * 行内动作（原型 `.srow` 右侧按钮）—— 两种形态：
   * - `to`：跳转到**承接该动作的目的地**（如「去处理」→ 账号页）；
   * - `run`：**就地执行**（原型 `run: "batch-checkin"` / `"add-account"`）——
   *   概览既然能回答「有什么要我处理」，就不该把每一件都推回列表页。
   */
  action: { label: string; to?: string; run?: () => void };
  /** 就地动作进行中（禁用按钮，避免重复提交）。 */
  busy?: boolean;
}

/**
 * 「概览」工作台（PLAN §3.9 / P3）—— 答**「现在什么情况、有什么要我处理」**。
 *
 * ## 为什么需要它
 *
 * 改造前没有这个页面：用户想知道「有没有账号要重新登录 / 有没有 Token 要过期」，
 * 得**逐个账号卡片看**。信息是有的，只是**没有被汇总**。
 *
 * ## 对齐原型（`docs/redesign/demo-app.js` 的 `viewOverview`）
 *
 * - **页头**：`h1` + 「作用域 · 当前登录」副标题 + 刷新（作用域是全局上下文，页内只读展示）。
 * - **待处理在前**（原型 P1-6：要你动手的事先于只读数字）：卡头带「N 项」计数，
 *   每行带**行内动作**按钮 —— 「告诉用户有事」而不给出口，等于把活又推回去。
 * - **侧栏**：其他作用域（复用 `use-scope-status`，点击即切作用域）+ 当前平台能力。
 *
 * ⚠️ **空态用 `EmptyState`**（§3.6）：没有待处理项是**好消息**，所以空态不是"暂无数据"，
 * 而是「一切正常」+ 出口（去账号页看看）。
 */
export default function OverviewPage() {
  const t = useT();
  /* 平台从**路由**推（`/s/{scope}/{dest}` 的 scope 里就带着它）——
     这样同一个组件在 WB 与 Trae 分区下都能用，不必各写一份。 */
  const location = useLocation();
  const navigate = useNavigate();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);
  const { statuses } = useScopeStatus();
  const scopeStatus = statuses[scope];

  const accounts = useAccountsStore((s) => s.accounts);
  const status = useAccountsStore((s) => s.status);
  const loading = useAccountsStore((s) => s.loading);
  const globalSlice = useAccountsStore((s) => s.global);
  const fetchAllRegions = useAccountsStore((s) => s.fetchAllRegions);

  useEffect(() => {
    void fetchAllRegions();
  }, [fetchAllRegions]);

  /* CN 侧是**扁平字段**、Global 侧是 `RegionSlice` —— 这里抹平两者的形状。
     ⚠️ 不要"顺手"把 store 改成两侧同形：那会牵动 App/统计页/多个钩子（见 store 里的注释）。 */
  const slice = region === "global" ? globalSlice : { accounts, status };

  const accountsHref = `/s/${scope}/accounts`;
  const settingsHref = `/s/${scope}/settings`;
  const usageHref = `/s/${scope}/usage`;

  /* 概览统计行与「近 14 天」趋势卡共用的一份指标（平台差异在数据层内消化）。 */
  const metrics = useOverviewMetrics(product, region);

  /* ---- ★ 新原型 `viewOverview`（`buddy-switch-prototype.html:2058-2069`）带来的两个数据源 ---- */

  /**
   * 今日签到态（`accountId → 是否已签`）。
   *
   * `AccountMeta` 里**没有**签到字段 —— 签到是**上游实时**事实，必须单独取；
   * 通道差异（webui 整端点 / 桌面端单账号）由 `getCheckinStatusMap` 收口。
   * ⚠️ 取不到 ⇒ 那条待处理**不显示**：不知道就说不知道，绝不臆造「未签到」把用户骗去点签到。
   */
  const [checkinMap, setCheckinMap] = useState<Record<string, boolean>>({});
  /** 「一键签到」进行中（禁用按钮 + 图标转圈 —— 与账号页批量签到同一状态语言）。 */
  const [checkinBusy, setCheckinBusy] = useState(false);
  /**
   * 积分到期（与账号卡片**同一个来源** `creditMap`，不为这一行重复取数）。
   * ⚠️ 只认 `ok === true` 的条目：读失败（凭据是加密信封、解不开）时**不猜**它有没有积分要过期。
   */
  const creditMap = useAccountsStore((s) => (region === "global" ? s.global.creditMap : s.creditMap));
  const refreshCredits = useAccountsStore((s) => s.refreshCredits);
  const reconcileAccounts = useAccountsStore((s) => s.reconcileAccounts);

  /** 账号 id 的稳定键 —— `slice.accounts` 每次刷新都换数组身份，直接当依赖会反复取数。 */
  const accountIdsKey = useMemo(() => slice.accounts.map((a) => a.id).join("|"), [slice.accounts]);

  useEffect(() => {
    if (!accountIdsKey) {
      setCheckinMap({});
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const entries = await api.getCheckinStatusMap(accountIdsKey.split("|"), region);
        if (cancelled) return;
        const next: Record<string, boolean> = {};
        for (const [id, entry] of Object.entries(entries)) next[id] = entry.todayCheckedIn === true;
        setCheckinMap(next);
      } catch {
        /* 读不到签到态 ⇒ 那条不显示（见上方注释）。 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [accountIdsKey, region]);

  /** 积分即将过期的账号（原型 `:2067`：「N 个账号的积分将在 5 天内过期」）。 */
  const expiringCredits = useMemo(() => {
    const rows = slice.accounts
      .map((account) => creditMap[account.id])
      .filter((credit) => credit?.ok === true && (credit.expiringSoonRemaining ?? 0) > 0);
    return {
      count: rows.length,
      amount: rows.reduce((sum, credit) => sum + (credit?.expiringSoonRemaining ?? 0), 0),
    };
  }, [slice.accounts, creditMap]);

  /**
   * 概览「一键签到」（原型 `viewOverview` 的行内动作 `run: "batch-checkin"`）。
   *
   * ## 与账号页批量签到**同一套纪律**（不另立口径）
   *
   * ① **只签「已知今天没签」的账号** —— 已签的跳过（服务端也会挡，但不该白跑一趟）；
   * ② **部分失败必须给出口**：失败项留在 toast 的 `action` 里「只重试失败项」，
   *    绝不把「3 成功 / 2 失败」说成成功；
   * ③ **签完重拉签到态 + 积分**：本地 `checkinMap` 不刷新，那条待处理会一直挂着
   *    （看起来像「签到没生效」）。
   */
  async function runOneClickCheckin(ids: string[]) {
    if (checkinBusy || ids.length === 0) return;
    setCheckinBusy(true);
    const toastId = toast.loading(t("wbOverview.toast.checkinLoading"));
    try {
      let ok = 0;
      let already = 0;
      const failedIds: string[] = [];
      for (const id of ids) {
        try {
          const res = await api.checkin(id, region);
          if (res.result === "success") ok += 1;
          else if (res.result === "already") already += 1;
          else failedIds.push(id);
        } catch {
          failedIds.push(id);
        }
      }
      const entries = await api.getCheckinStatusMap(ids, region).catch(() => null);
      if (entries) {
        const next: Record<string, boolean> = {};
        for (const [id, entry] of Object.entries(entries)) next[id] = entry.todayCheckedIn === true;
        setCheckinMap((prev) => ({ ...prev, ...next }));
      }
      await refreshCredits(ids, { region });
      void reconcileAccounts(region);

      const summary = t("wbOverview.toast.checkinSummary", {
        ok,
        already,
        failed: failedIds.length,
      });
      const retryAction =
        failedIds.length > 0
          ? {
              label: t("wbOverview.toast.checkinRetryFailed"),
              onClick: () => void runOneClickCheckin(failedIds),
            }
          : undefined;
      if (failedIds.length === ids.length) {
        toast.error(t("wbOverview.toast.checkinFail"), {
          id: toastId,
          description: summary,
          action: retryAction,
        });
      } else if (failedIds.length > 0) {
        toast.warning(t("wbOverview.toast.checkinPartial"), {
          id: toastId,
          description: summary,
          action: retryAction,
        });
      } else {
        toast.success(t("wbOverview.toast.checkinOk"), { id: toastId, description: summary });
      }
    } catch (e) {
      toast.error(t("wbOverview.toast.checkinFail"), { id: toastId, description: api.asError(e) });
    } finally {
      setCheckinBusy(false);
    }
  }

  const pending = useMemo<PendingItem[]>(() => {
    const list: PendingItem[] = [];
    const now = Date.now();

    const needRelogin = slice.accounts.filter((a) => a.needsRelogin);
    if (needRelogin.length > 0) {
      list.push({
        key: "relogin",
        icon: AlertTriangle,
        tone: "danger",
        title: t("wbOverview.pending.relogin", { count: needRelogin.length }),
        hint: t("wbOverview.pending.reloginHint"),
        action: { label: t("wbOverview.pending.action.handle"), to: accountsHref },
      });
    }

    /* ★ 「凭据已过期」与「登录态即将过期」是**两件事**（原型 `:2065` vs `:2067`）：
       前者**已经**不能用（切过去就失败），后者**还能用但快到期**。
       ⚠️ 判据必须**互斥** —— 已过期的不要再算进「即将过期」，否则同一批账号被说两遍
       （旧实现用 `expiresAt - now < WARN` 连带把已过期的也算进去，正是这个毛病）。 */
    const expired = slice.accounts.filter(
      (a) => !a.needsRelogin && a.expiresAt != null && a.expiresAt <= now,
    );
    if (expired.length > 0) {
      list.push({
        key: "expired",
        icon: AlertTriangle,
        tone: "danger",
        title: t("wbOverview.pending.expired", { count: expired.length }),
        hint: t("wbOverview.pending.expiredHint"),
        action: { label: t("wbOverview.pending.action.handle"), to: accountsHref },
      });
    }

    const expiringSoon = slice.accounts.filter(
      (a) =>
        !a.needsRelogin &&
        a.expiresAt != null &&
        a.expiresAt > now &&
        a.expiresAt - now < EXPIRY_WARN_MS,
    );
    if (expiringSoon.length > 0) {
      list.push({
        key: "expiring",
        icon: Clock,
        tone: "warn",
        title: t("wbOverview.pending.expiring", { count: expiringSoon.length }),
        hint: t("wbOverview.pending.expiringHint"),
        action: { label: t("wbOverview.pending.action.handle"), to: accountsHref },
      });
    }

    /* ★ 「今日未签到」——只在**拿到签到态**（`checkinMap` 非空）时才说：
       空 map 表示「还没取到 / 取不到」⇒ 不显示，不拿未知冒充「未签到」。 */
    const notChecked = slice.accounts.filter((a) => !a.needsRelogin && checkinMap[a.id] === false);
    if (notChecked.length > 0) {
      const ids = notChecked.map((a) => a.id);
      list.push({
        key: "notChecked",
        icon: CheckCircle2,
        tone: "warn",
        title: t("wbOverview.pending.notChecked", { count: notChecked.length }),
        hint: t("wbOverview.pending.notCheckedHint"),
        // ★ **就地执行**（原型 `run: "batch-checkin"`）—— 不是跳去账号页再找一遍。
        action: {
          label: t("wbOverview.pending.action.checkin"),
          run: () => void runOneClickCheckin(ids),
        },
        busy: checkinBusy,
      });
    }

    if (expiringCredits.count > 0) {
      list.push({
        key: "expiringCredits",
        icon: Clock,
        tone: "warn",
        title: t("wbOverview.pending.expiringCredits", { count: expiringCredits.count }),
        hint: t("wbOverview.pending.expiringCreditsHint", {
          amount: formatCredits(expiringCredits.amount),
        }),
        action: { label: t("wbOverview.pending.action.handle"), to: accountsHref },
      });
    }

    /* 「未运行」只在**确实装了但没开**时说（`installed=false` 是「未检测到」，是另一件事：
       该引导安装，而不是去点启动）。 */
    if (scopeStatus.installed && !scopeStatus.running) {
      list.push({
        key: "notRunning",
        icon: Info,
        tone: "info",
        title: t("wbOverview.pending.notRunning"),
        hint: t("wbOverview.pending.notRunningHint"),
        action: { label: t("wbOverview.pending.action.settings"), to: settingsHref },
      });
    }

    /* 「还没有账号」在**首屏加载完之后**才说，否则加载中的空库会闪一下这条提示。 */
    if (!loading && scopeStatus.accountCount === 0) {
      list.push({
        key: "noAccounts",
        icon: Plus,
        tone: "info",
        title: t("wbOverview.pending.noAccounts"),
        hint: t("wbOverview.pending.noAccountsHint"),
        action: { label: t("wbOverview.pending.action.addAccount"), to: accountsHref },
      });
    }

    return list;
  }, [
    slice.accounts,
    scopeStatus,
    loading,
    t,
    accountsHref,
    settingsHref,
    checkinMap,
    checkinBusy,
    expiringCredits,
  ]);

  /* 「需处理账号数」= 需重新登录 + 凭据已过期 / 即将过期（与待处理项同判据；过期含在窗口内）。
     ⚠️ 只数**账号**，不含「客户端未运行」「还没有账号」这类非账号项。 */
  const attentionCount = useMemo(() => {
    const now = Date.now();
    return slice.accounts.filter(
      (a) => a.needsRelogin || (a.expiresAt != null && a.expiresAt - now < EXPIRY_WARN_MS),
    ).length;
  }, [slice.accounts]);

  /* 页头副标题：作用域 + 当前登录（作用域标签与顶栏 `ScopeSelector` 同源，见 `scopeLabel`）。 */
  const subtitle = scopeStatus.loggedInName
    ? t("pageHead.overview.signedIn", {
        scope: scopeLabel(t, scope),
        name: scopeStatus.loggedInName,
      })
    : t("pageHead.overview.signedOut", { scope: scopeLabel(t, scope) });

  /** 其他作用域：点击即切（写记忆 + 导航到该作用域的概览，与顶栏同口径）。 */
  function goScope(next: Scope) {
    if (next === scope) return;
    writeLastScope(window.localStorage, next);
    navigate(scopeShortcutTarget(next, "overview"));
  }

  const capabilities = platformDescriptor(product).capabilities;

  return (
    <div className="mx-auto flex w-full max-w-[1240px] flex-col gap-5 px-7 pt-[26px] pb-[60px]">
      <header className="flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("wbOverview.title")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">{subtitle}</p>
        </div>
        <div className="grow" />
        <Button
          variant="outline"
          size="sm"
          onClick={() => void fetchAllRegions()}
          disabled={loading}
          aria-label={t("wbOverview.refresh")}
        >
          <RefreshCw className={cn(loading && "animate-spin")} />
          {t("wbOverview.refresh")}
        </Button>
      </header>

      {/* ① 待处理 —— **上移到统计块之前**（原型 P1-6：要你动手的事先于只读数字）。 */}
      <Card className="gap-0 py-0">
        <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
          <span className="text-sm font-semibold">{t("wbOverview.pendingTitle")}</span>
          <div className="grow" />
          <span className="text-xs text-muted-foreground">
            {t("wbOverview.pending.count", { n: pending.length })}
          </span>
        </div>
        {loading && slice.accounts.length === 0 ? (
          /* 首次加载用骨架行（§3.6：区块级加载占位一律 `Skeleton`）。 */
          <div className="flex flex-col gap-2 px-5 py-4">
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-4 w-1/2" />
          </div>
        ) : pending.length === 0 ? (
          /* 「没有待处理项」是**好消息**，所以空态不是"暂无数据"而是「一切正常」+ 出口。 */
          <EmptyState
            icon={CheckCircle2}
            title={t("wbOverview.allGood")}
            description={t("wbOverview.allGoodHint")}
            action={
              <Button asChild size="sm" variant="outline">
                <Link to={accountsHref}>{t("wbOverview.allGoodAction")}</Link>
              </Button>
            }
          />
        ) : (
          <ul className="divide-y divide-border/60">
            {pending.map((item) => (
              <li key={item.key} className="flex items-center gap-3 px-5 py-3">
                <item.icon className={cn("size-4 shrink-0", TONE_TEXT[item.tone])} aria-hidden="true" />
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium">{item.title}</p>
                  <p className="mt-0.5 text-xs text-muted-foreground">{item.hint}</p>
                </div>
                {/* 行内动作两种形态（原型 `.srow`）：跳转 或 **就地执行**。 */}
                {item.action.to ? (
                  <Button asChild size="sm" variant="outline" className="shrink-0">
                    <Link to={item.action.to}>{item.action.label}</Link>
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    className="shrink-0"
                    disabled={item.busy}
                    data-pending-run={item.key}
                    onClick={item.action.run}
                  >
                    {item.busy ? <Loader2 className="animate-spin" aria-hidden="true" /> : null}
                    {item.action.label}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>

      {/* ② 统计行（原型 `.grid.g4`）：一排四块只读快照（要你动手的事已在上方）。 */}
      <OverviewStats
        metrics={metrics}
        accountCount={scopeStatus.accountCount}
        attentionCount={attentionCount}
        accountsLoading={loading}
      />

      {/* ③ 工作台主体 + 侧栏（原型 `.split`：主区 | 290px 侧栏）。 */}
      <div className="flex flex-col gap-[22px] lg:flex-row lg:items-start">
        <div className="flex min-w-0 flex-1 flex-col gap-4">
          {/* 近 14 天 Token 用量（与统计行「今日 Token」同源，同一份 `metrics.token`）。 */}
          <TokenTrendCard token={metrics.token} usageHref={usageHref} />
        </div>

        <aside className="flex w-full flex-col gap-4 lg:w-[290px] lg:shrink-0">
          {/* 其他作用域（原型 `.ov-side` 第一张卡）：跨作用域一眼看全，点击即切。 */}
          <Card className="gap-0 py-0">
            <div className="border-b border-border/60 px-5 py-3">
              <span className="text-sm font-semibold">{t("wbOverview.otherScopes.title")}</span>
            </div>
            <div className="flex flex-col gap-0.5 p-2">
              {SCOPES.filter((s) => s !== scope).map((s) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => goScope(s)}
                  className="flex w-full items-center gap-2.5 rounded-[10px] px-2.5 py-2 text-left outline-none transition-colors hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/40"
                >
                  <ScopeMark
                    product={scopeParts(s).product}
                    size={28}
                    className="rounded-[9px]"
                  />
                  <span className="flex min-w-0 flex-1 flex-col leading-tight">
                    <span className="truncate text-[12.5px] font-medium">{scopeLabel(t, s)}</span>
                    <span className="truncate text-[11px] text-muted-foreground">
                      {otherScopeSub(t, statuses[s])}
                    </span>
                  </span>
                  <RunPill status={statuses[s]} compact />
                </button>
              ))}
            </div>
          </Card>

          {/* 能力（原型 `.ov-side` 第二张卡）：当前平台**固有**能力，数据来自注册表。 */}
          <Card className="gap-0 py-0">
            <div className="border-b border-border/60 px-5 py-3">
              <span className="text-sm font-semibold">{t("wbOverview.capabilities.title")}</span>
            </div>
            <div className="flex flex-wrap gap-1.5 px-5 py-4">
              {capabilities.map((cap) => (
                <Badge
                  key={cap}
                  variant="outline"
                  className="border-primary/30 bg-primary/15 font-normal text-primary"
                >
                  {t(CAPABILITY_LABEL[cap])}
                </Badge>
              ))}
            </div>
          </Card>
        </aside>
      </div>
    </div>
  );
}

/** 待处理项的语气 → 图标色（`info` 是中性提示，不与"要处理"的风险色混淆）。 */
const TONE_TEXT: Record<PendingItem["tone"], string> = {
  danger: "text-destructive",
  warn: "text-warning",
  info: "text-muted-foreground",
};

/** 其他作用域项的副标题（口径与顶栏 `ScopeSelector.subOf` 一致）。 */
function otherScopeSub(t: ReturnType<typeof useT>, status: ScopeStatus): string {
  if (status.loggedInName) {
    return t("wbOverview.otherScopes.loggedIn", {
      name: status.loggedInName,
      n: status.accountCount,
    });
  }
  return t(status.installed ? "wbOverview.otherScopes.loggedOut" : "wbOverview.otherScopes.missing", {
    n: status.accountCount,
  });
}