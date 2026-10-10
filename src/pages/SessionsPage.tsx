import { useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { AlertTriangle, Download, Filter, GitBranch, Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { SessionReplicaView } from "@/components/sessions/session-replica-view";
import { UnifyReplicasDialog } from "@/components/sessions/unify-replicas-dialog";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { platformOfPath } from "@/lib/platforms/registry";
import { scopeOf } from "@/lib/scope";
import { buildSyncItems, buildUnifyItems } from "@/lib/session-replicas";
import type { SessionGroupStatus, SessionGroupView } from "@/lib/session-replicas";
import type { SyncSessionReplicaItem, UnifySessionReplicaItem } from "@/lib/types";
import { useScope } from "@/lib/use-scope";
import { useSessionsStore } from "@/stores/sessions";

/**
 * 「会话」工作台（WorkBuddy 侧目的地）—— 副本只读视图（P8 T01 骨架 + T02 接线）。
 *
 * ## 本期范围（`P8-ARCH.md` §1.2 / §1.6）
 *
 * 数据源 = 后端既有的 copy ledger（`session.rs:928`），只读命令 `list_session_replicas`
 * 由 T02 接线。有登记 ⇒ 渲染副本只读视图；无登记 ⇒ **结构化空态**（§1.5）。
 *
 * ⚠️ 刻意**不**渲染的东西（§1.9）：
 * - **客户端 Tab**：会话复制只有 WorkBuddy 桌面 ⇒ 只有 1 项 ⇒ 单选项的切换器是假控件
 *   （§3.15.1），**不渲染，也不保留结构**（✅ #7）。
 *
 * ★ **状态筛选已解禁**（P8：内容指纹落地后，组状态可派生）—— 早期版本写「无消息计数 ⇒
 * 一个选项都算不出 ⇒ 整条不渲染」，**那个前提已不成立**。但现在仍**按需出现**：
 * 数据里没有的状态**不渲染**（§3.4 规则 1），且**至少 2 个可选状态**才渲染整条
 * —— 单选项的切换器依旧是假控件（§3.15.1）。
 *
 * ⚠️ **不用 `updatedAt` 冒充「落后 / 最新」**：`updatedAt` 只能表达「谁最近被改过」，
 * 不能表达「谁落后 N 条」（§3.15.2「时间新 ≠ 内容新」）。状态**一律来自内容指纹**。
 *
 * ## 状态语言（§3.6）
 *
 * 区块首次加载 → `<Skeleton>`；空态 → `<EmptyState>`（四要素含**出口**）；
 * 失败 → 明确错误 + **重试**（不静默、不假装空）。
 */
export default function SessionsPage() {
  const t = useT();
  const navigate = useNavigate();
  const location = useLocation();
  /* 平台从**路由**推（`/s/{scope}/sessions` 的 scope 里就带着它）——
     这样同一个骨架在 WB 分区下可用，也不必写平台判断。 */
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);

  const groups = useSessionsStore((s) => s.groups);
  const loading = useSessionsStore((s) => s.loading);
  const error = useSessionsStore((s) => s.error);
  const load = useSessionsStore((s) => s.load);
  const syncItems = useSessionsStore((s) => s.syncItems);
  const unifyItems = useSessionsStore((s) => s.unifyItems);

  /* region 变化即重载（副本按 region 分文件，P8 ✅ #2）。 */
  useEffect(() => {
    void load(region);
  }, [load, region]);

  /**
   * ★ P8：状态筛选 —— **按需出现**。
   *
   * ⚠️ 两条纪律同时生效：
   * ① **数据里没有的状态不渲染**（§3.4 规则 1）—— 不摆「0 个有分歧」这种假选项；
   * ② **至少 2 个可选状态**才渲染整条 —— 单选项的切换器是假控件（§3.15.1）。
   */
  const [statusFilter, setStatusFilter] = useState<SessionGroupStatus | "all">("all");

  /* ★ P8 安全同步的「操作进行中」态。⚠️ 这是**按钮态**，不是区块加载态 ——
     所以用 `Loader2`（按钮内）而**不是** `Skeleton`（§3.6 两种状态语言不混用）。 */
  const [syncBusy, setSyncBusy] = useState(false);
  const [busyGroupKeys, setBusyGroupKeys] = useState<ReadonlySet<string>>(new Set());
  const statusCounts = useMemo(() => {
    const counts = new Map<SessionGroupStatus, number>();
    for (const group of groups) {
      counts.set(group.status, (counts.get(group.status) ?? 0) + 1);
    }
    return counts;
  }, [groups]);
  const availableStatuses = useMemo(() => [...statusCounts.keys()], [statusCounts]);
  const filteredGroups = useMemo(
    () => (statusFilter === "all" ? groups : groups.filter((g) => g.status === statusFilter)),
    [groups, statusFilter],
  );

  /* 筛选态在数据变化后可能失效（该状态已无组）⇒ 自动回落到 `all`，避免"空屏 + 假筛选"。 */
  useEffect(() => {
    if (statusFilter !== "all" && !statusCounts.has(statusFilter)) setStatusFilter("all");
  }, [statusCounts, statusFilter]);

  /**
   * ★ P8「安全同步」：把落后副本一次补齐（**加法**，绝不删除 / 改写目标已有内容）。
   *
   * ## 三条纪律
   *
   * ① **批量 + 部分失败**（`PLAN.md` §3.6）：逐条结果 → 汇总条（N 成功 / M 失败）
   *    + **「只重试失败项」出口**（失败项留在 toast 的 action 里，不必把成功的重跑）；
   * ② **全部失败才用 error**，部分成功用 success —— 否则用户会以为整批都没做；
   * ③ **同步完必须重载**：列表状态来自**内容指纹**，不重载就会继续显示旧状态
   *    （看起来像「同步没生效」）。
   *
   * ⚠️ 分歧组**不给**这条出口（`buildSyncItems` 只对 `behind` 出项）——
   *    分歧要先人工决定保留哪一份（D20 后置），硬同步会丢内容。
   */
  async function runSync(items: SyncSessionReplicaItem[], busyKeys: string[]) {
    if (syncBusy || items.length === 0) return;
    setSyncBusy(true);
    setBusyGroupKeys(new Set(busyKeys));
    try {
      const response = await syncItems(region, items);
      await load(region);

      const failed = response.results
        .filter((r) => !r.ok)
        .map((r) => ({ sourceCid: r.sourceCid, targetCid: r.targetCid }));
      const appended = response.results.reduce((sum, r) => sum + (r.appendedLines ?? 0), 0);

      const parts: string[] = [];
      if (response.okCount > 0) {
        parts.push(t("sessions.sync.toast.ok", { n: response.okCount, lines: appended }));
      }
      if (response.failCount > 0) {
        parts.push(t("sessions.sync.toast.failed", { n: response.failCount }));
      }
      const summary = parts.join(t("shared.punct.comma"));

      /* ★ 有失败 ⇒ 给「只重试失败项」出口（否则用户只能把成功的一起重跑）。 */
      const retryAction =
        failed.length > 0
          ? {
              label: t("sessions.sync.toast.retryFailed"),
              onClick: () => void runSync(failed, busyKeys),
            }
          : undefined;

      if (response.okCount === 0) {
        toast.error(t("sessions.sync.toast.failAll"), {
          description: summary,
          action: retryAction,
        });
      } else {
        toast.success(t("sessions.sync.toast.done"), {
          description: summary,
          action: retryAction,
        });
      }
    } catch (e) {
      toast.error(t("sessions.sync.toast.error"), { description: api.asError(e) });
    } finally {
      setSyncBusy(false);
      setBusyGroupKeys(new Set());
    }
  }

  /** 单个组：同步它所有落后的副本。 */
  function syncGroup(group: SessionGroupView) {
    void runSync(buildSyncItems(group), [group.key]);
  }

  /** 页头批量：把所有「有可同步内容」的组一次跑完（仍逐条返回 ⇒ 部分失败可重试）。 */
  const allSyncable = useMemo(
    () => groups.map((group) => ({ group, items: buildSyncItems(group) })).filter((x) => x.items.length > 0),
    [groups],
  );

  function syncAll() {
    void runSync(
      allSyncable.flatMap((x) => x.items),
      allSyncable.map((x) => x.group.key),
    );
  }

  /* ★ P8 分歧处理：待处理的分歧组（非 null ⇒ 打开对话框）+ 进行中态。 */
  const [unifyTarget, setUnifyTarget] = useState<SessionGroupView | null>(null);
  const [unifyBusy, setUnifyBusy] = useState(false);

  /**
   * ★ P8「分歧处理」：把用户**选定保留的那一份**覆盖到其余副本（**破坏性**）。
   *
   * ⚠️ 与「安全同步」**语义相反**：同步是加法，这里**会丢弃**目标的分歧内容。
   * 二次确认在 [`UnifyReplicasDialog`] 里（选保留哪一份 + 如实报「会丢几条」）；
   * 本函数只负责**执行 + 汇总**，沿用与同步同一套「部分失败 + 只重试失败项」纪律（§3.6）。
   */
  async function runUnify(items: UnifySessionReplicaItem[]) {
    if (unifyBusy || items.length === 0) return;
    setUnifyBusy(true);
    try {
      const response = await unifyItems(region, items);
      // 列表状态来自内容指纹 ⇒ 不重载会继续显示「有分歧」（看起来像没生效）。
      await load(region);

      const failed = response.results
        .filter((r) => !r.ok)
        .map((r) => ({ sourceCid: r.sourceCid, targetCid: r.targetCid }));
      const lost = response.results.reduce((sum, r) => sum + (r.lostLines ?? 0), 0);

      const parts: string[] = [];
      if (response.okCount > 0) {
        parts.push(t("sessions.unify.toast.ok", { n: response.okCount, lost }));
      }
      if (response.failCount > 0) {
        parts.push(t("sessions.unify.toast.failed", { n: response.failCount }));
      }
      const summary = parts.join(t("shared.punct.comma"));

      const retryAction =
        failed.length > 0
          ? {
              label: t("sessions.unify.toast.retryFailed"),
              onClick: () => void runUnify(failed),
            }
          : undefined;

      if (response.okCount === 0) {
        toast.error(t("sessions.unify.toast.failAll"), { description: summary, action: retryAction });
      } else {
        toast.success(t("sessions.unify.toast.done"), { description: summary, action: retryAction });
      }
    } catch (e) {
      toast.error(t("sessions.unify.toast.error"), { description: api.asError(e) });
    } finally {
      setUnifyBusy(false);
      // 无论成败都收起对话框：失败项留在 toast 的「只重试失败项」里，不必让用户重选。
      setUnifyTarget(null);
    }
  }

  /** 对话框确认：把「选定保留的那一份」换算成覆盖任务（`buildUnifyItems` 会挡住非法输入）。 */
  function confirmUnify(keepSessionId: string) {
    if (!unifyTarget) return;
    void runUnify(buildUnifyItems(unifyTarget, keepSessionId));
  }

  return (
    <div className="mx-auto flex w-full max-w-[1240px] flex-col gap-5 px-7 pt-[26px] pb-[60px]">
      {/* 页头（原型 `viewSessions`）：28px 标题 + 作用域副标题 + 只读 scope-chip。
          批量出口仍**只在真有可同步项时**出现（§3.4 规则 1，不摆假控件）。 */}
      <header className="flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-semibold tracking-tight">{t("sessions.title")}</h1>
          <p className="mt-1.5 text-xs text-muted-foreground">
            {t("pageHead.sessions", { scope: scopeLabel(t, scope) })}
          </p>
        </div>
        <div className="grow" />
        {allSyncable.length > 0 ? (
          <Button
            size="sm"
            variant="outline"
            disabled={syncBusy}
            data-sync-all="true"
            title={t("sessions.sync.hint")}
            onClick={syncAll}
          >
            {syncBusy ? (
              <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
            ) : (
              <Download className="size-3.5" aria-hidden="true" />
            )}
            {t("sessions.sync.all", { count: allSyncable.reduce((n, x) => n + x.items.length, 0) })}
          </Button>
        ) : null}
        <ScopeChip scope={scope} />
      </header>

      {loading ? (
        // 区块首次加载 → 骨架（与全应用一致的状态语言，§3.6）。
        <div className="flex flex-col gap-3" aria-busy="true">
          <Skeleton className="h-16 rounded-xl" />
          <Skeleton className="h-16 rounded-xl" />
          <Skeleton className="h-16 rounded-xl" />
        </div>
      ) : error ? (
        // 失败不静默：给出原因 + 重试（**不**退化成「暂无数据」，那会误导用户）。
        <Card className="items-center gap-3 py-8">
          <AlertTriangle className="size-8 text-muted-foreground/60" aria-hidden="true" />
          <p className="max-w-md text-center text-sm text-muted-foreground">{error}</p>
          <Button variant="outline" size="sm" onClick={() => void load(region)}>
            <RefreshCw className="size-3.5" aria-hidden="true" />
            {t("sessions.error.retry")}
          </Button>
        </Card>
      ) : groups.length > 0 ? (
        <>
          {/* ★ 工具栏（原型 `viewSessions`，:3183）：标题 =「{客户端} 会话」+ 计数徽标。
              ⚠️ **不渲染客户端 Tab**（会话复制只有 WorkBuddy ⇒ 单选项是假控件，§3.15.1）
              ⇒ 标题退化成静态文本。状态筛选**按需出现**：**至少 2 个可选状态**才渲染整条
              （单选项的切换器依旧是假控件），且**只列数据里真有的状态**（§3.4 规则 1）。 */}
          <div className="flex flex-wrap items-center gap-2 rounded-xl border bg-card px-3 py-2.5">
            <h2 className="flex items-center gap-2 text-[13.5px] font-semibold">
              {t("sessions.toolbar.title", { client: t("sessions.toolbar.client") })}
              <Badge variant="secondary">{groups.length}</Badge>
            </h2>
            <div className="grow" />
            {availableStatuses.length >= 2 ? (
              <div className="flex flex-wrap items-center gap-1.5">
                <Filter className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <Button
                  size="sm"
                  variant={statusFilter === "all" ? "default" : "outline"}
                  onClick={() => setStatusFilter("all")}
                  data-status-filter="all"
                >
                  {t("sessions.filter.all", { count: groups.length })}
                </Button>
                {availableStatuses.map((status) => (
                  <Button
                    key={status}
                    size="sm"
                    variant={statusFilter === status ? "default" : "outline"}
                    onClick={() => setStatusFilter(status)}
                    data-status-filter={status}
                  >
                    {t(`sessions.replica.status.${status}`)}
                    {` (${statusCounts.get(status) ?? 0})`}
                  </Button>
                ))}
              </div>
            ) : null}
          </div>
          <SessionReplicaView
            groups={filteredGroups}
            onSyncGroup={syncGroup}
            busyGroupKeys={busyGroupKeys}
            onUnifyGroup={setUnifyTarget}
          />
        </>
      ) : (
        <Card className="gap-0 py-0">
          {/* 空态四要素（§3.6）：图标 + 标题 + 说明 + **出口**。
              ⚠️ 出口是「去账号页复制」——空态必须告诉用户下一步做什么，
              只给一句「暂无数据」等于把用户丢在原地（见 EmptyState 头注释）。 */}
          <EmptyState
            icon={GitBranch}
            title={t("sessions.empty.title")}
            description={t("sessions.empty.description")}
            action={
              <Button size="sm" onClick={() => navigate(`/s/${scopeOf(product, region)}/accounts`)}>
                {t("sessions.empty.action")}
              </Button>
            }
          />
        </Card>
      )}

      {/* ★ P8 分歧处理：**覆盖式**统一 —— 必须「先选保留哪一份、再确认」（§3.15.2）。
          `group` 为 null ⇒ 不渲染（`UnifyReplicasDialog` 内部亦早退）。 */}
      <UnifyReplicasDialog
        group={unifyTarget}
        onOpenChange={(open) => {
          if (!open) setUnifyTarget(null);
        }}
        busy={unifyBusy}
        onConfirm={confirmUnify}
      />
    </div>
  );
}
