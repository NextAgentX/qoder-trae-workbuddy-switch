import { useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { AlertTriangle, Filter, GitBranch, RefreshCw } from "lucide-react";

import { SessionReplicaView } from "@/components/sessions/session-replica-view";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { useT } from "@/lib/i18n";
import { platformOfPath } from "@/lib/platforms/registry";
import { scopeOf } from "@/lib/scope";
import type { SessionGroupStatus } from "@/lib/session-replicas";
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

  const groups = useSessionsStore((s) => s.groups);
  const loading = useSessionsStore((s) => s.loading);
  const error = useSessionsStore((s) => s.error);
  const load = useSessionsStore((s) => s.load);

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

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-5">
      <header className="flex items-center gap-3">
        <div className="min-w-0">
          <h1 className="text-lg font-semibold tracking-[-0.01em]">{t("sessions.title")}</h1>
          <p className="mt-0.5 text-xs text-muted-foreground">{t("sessions.subtitle")}</p>
        </div>
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
          {/* ★ 状态筛选：**至少 2 个可选状态**才渲染（单选项的切换器是假控件，§3.15.1）；
              且**只列数据里真有的状态**（§3.4 规则 1，不摆假选项）。 */}
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
          <SessionReplicaView groups={filteredGroups} />
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
    </div>
  );
}
