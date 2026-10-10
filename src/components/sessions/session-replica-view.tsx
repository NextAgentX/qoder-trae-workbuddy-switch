/**
 * 会话副本视图（P8 T02 + 关联图）—— 卡片网格 + 关联图抽屉。
 *
 * ## 结构（对齐原型 `viewSessions` / `sessionGraphHtml`，§3.15.2）
 *
 * - 列表区 = `.sg-grid`（`minmax(260px, 1fr)`，gap 12）的**摘要卡片**：状态徽标 +「N 个副本」、
 *   标题、更新行（原型 :3153-3162）；
 * - 点开卡片 → 右侧**抽屉**（shadcn `Sheet`）渲染**会话关联图**：共同旧版 → 各账号副本分支，
 *   每个分支显示**相对共同旧版的差异**（roll up 到 `deltas`），而不是只给时间戳
 *   （原型 `sessionGraphHtml`，:3199）。抽屉里同时保留逐副本明细与同步 / 分歧出口。
 *
 * ## 只读 · 诚实标注（`P8-ARCH.md` §1.4 / §1.5）
 *
 * 数据源是 copy ledger（**内部去重账本被提升为产品语义**）⇒ 它**只覆盖「经本应用复制过」
 * 的会话**。因此每个组固定标注「**仅显示经本应用复制过会话**」（`group.partial` 恒 `true`）；
 * **禁止**任何「全部副本」口径的文案（§3.5 不假承诺）。
 *
 * ## 状态 / 分歧（**内容指纹**）
 *
 * ⚠️ **禁止**用 `updatedAt` 派生「落后 / 最新」（`P8-ARCH.md` §1.2「时间新 ≠ 内容新」）。
 * 状态一律由 `deriveGroupStatus`（jsonl 指纹公共前缀）派生；`updatedAt` **只作事实展示**
 * （卡片更新行 `formatDate(...)`、抽屉里 `formatDate(row.updatedAt)`），不参与任何判定。
 *
 * ## 白屏防线（issue #2）
 *
 * 会话标题 / 账号显示名在 `lib/session-replicas.ts` 已过 `displayText` ⇒ 这里拿到的**一定**
 * 是字符串，可安全当 React 子节点渲染。
 */

import { useState } from "react";
import { Download, GitMerge, History, Loader2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { buildSyncItems } from "@/lib/session-replicas";
import type { SessionGroupView } from "@/lib/session-replicas";

export interface SessionReplicaViewProps {
  groups: SessionGroupView[];
  /**
   * ★ P8「安全同步」：把该组的落后副本一次补齐（**加法**，不删除目标内容）。
   *
   * 省略 ⇒ **不渲染同步出口**（如只读场景）。渲染与否**由数据决定**：
   * `buildSyncItems(group)` 为空（非 `behind` / 无增量）时**不给按钮**，
   * 避免「按了没反应」的假控件（§3.4 规则 1）。
   */
  onSyncGroup?: (group: SessionGroupView) => void;
  /** 正在同步的组键（按钮进「操作进行中」态；**不是**区块加载态，§3.6）。 */
  busyGroupKeys?: ReadonlySet<string>;
  /**
   * ★ P8「分歧处理」：打开「选择保留哪一份」对话框（**覆盖式**统一，§3.15.2）。
   *
   * 省略 ⇒ **不渲染分歧出口**。只在 `group.status === "diverge"` 且 ≥2 个副本时给按钮 ——
   * 分歧**无法**用加法合并（各自都有对方没有的内容），必须让用户先选保留哪一份。
   * ⚠️ 覆盖是**破坏性**的 ⇒ 真正的二次确认在 [`UnifyReplicasDialog`] 里，不在本视图。
   */
  onUnifyGroup?: (group: SessionGroupView) => void;
}

/** 组状态 → 徽标变体（与 `sessions.replica.status.*` 文案配套，**内容级**派生）。 */
function statusVariant(status: SessionGroupView["status"]) {
  if (status === "diverge") return "destructive";
  if (status === "behind") return "warning";
  if (status === "latest") return "success";
  return "secondary";
}

/**
 * 组标题：取任一**有真实标题**的副本（只有当前账号的副本能拿到会话行，其余落兜底文案）。
 * 同一份会话复制出的各副本标题一致 ⇒ 取第一个非兜底的即代表整组；全为兜底则如实显示兜底。
 */
function groupTitle(t: ReturnType<typeof useT>, group: SessionGroupView): string {
  const fallback = t("sessions.replica.titleFallback");
  const named = group.rows.find((row) => row.title !== fallback);
  return named?.title ?? group.rows[0]?.title ?? fallback;
}

/** 组的更新行 = 各副本 `updatedAt` 里**最近**的一个（**只作事实展示**，非状态派生）。 */
function latestUpdatedAt(group: SessionGroupView): number | null {
  const times = group.rows.map((row) => row.updatedAt).filter((v): v is number => v !== null);
  return times.length > 0 ? Math.max(...times) : null;
}

/** 分支徽标变体：由**内容级** `deltas` 决定（分歧优先，其次看谁增量最多）。 */
function branchVariant(group: SessionGroupView, sessionId: string) {
  if (group.status === "diverge") return "destructive";
  const delta = group.deltas.get(sessionId) ?? 0;
  if (delta <= 0) return "secondary";
  const maxDelta = Math.max(...group.rows.map((row) => group.deltas.get(row.sessionId) ?? 0));
  return delta >= maxDelta ? "success" : "warning";
}

export function SessionReplicaView({
  groups,
  onSyncGroup,
  busyGroupKeys,
  onUnifyGroup,
}: SessionReplicaViewProps) {
  const t = useT();
  const [openKey, setOpenKey] = useState<string | null>(null);
  const open = groups.find((group) => group.key === openKey) ?? null;

  return (
    <>
      {/* `.sg-grid`：`minmax(260px, 1fr)` + gap 12（原型 :794）。 */}
      <div
        className="grid gap-3"
        style={{ gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))" }}
      >
        {groups.map((group) => {
          const updated = latestUpdatedAt(group);
          return (
            <button
              key={group.key}
              type="button"
              data-session-group={group.key}
              aria-label={t("sessions.card.openAria", { title: groupTitle(t, group) })}
              onClick={() => setOpenKey(group.key)}
              className="flex flex-col items-stretch rounded-xl border bg-card p-3.5 text-left transition-colors hover:border-primary focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            >
              <div className="flex items-center justify-between gap-2">
                {/* ★ **内容级**状态（来自 fingerprint 的公共前缀，**不是** updatedAt）。 */}
                <Badge variant={statusVariant(group.status)} data-status={group.status}>
                  {t(`sessions.replica.status.${group.status}`)}
                </Badge>
                <span className="shrink-0 text-xs text-muted-foreground">
                  {t("sessions.card.replicaCount", { count: group.rows.length })}
                </span>
              </div>
              <h4 className="mt-2 mb-1.5 truncate text-[13px] font-semibold">
                {groupTitle(t, group)}
              </h4>
              {/* `updatedAt` **只作事实**展示，不派生「落后 / 最新」（§3.15.2）。 */}
              <div className="text-xs text-muted-foreground">
                {updated !== null ? formatDate(updated) : t("sessions.replica.updatedUnknown")}
              </div>
            </button>
          );
        })}
      </div>

      <Sheet
        open={open !== null}
        onOpenChange={(next) => {
          if (!next) setOpenKey(null);
        }}
      >
        <SheetContent side="right" className="w-full sm:max-w-md">
          {open ? (
            <SessionGraphSheet
              group={open}
              onSyncGroup={onSyncGroup}
              busy={busyGroupKeys?.has(open.key) === true}
              onUnifyGroup={onUnifyGroup}
            />
          ) : null}
        </SheetContent>
      </Sheet>
    </>
  );
}

interface SessionGraphSheetProps {
  group: SessionGroupView;
  onSyncGroup?: (group: SessionGroupView) => void;
  busy: boolean;
  onUnifyGroup?: (group: SessionGroupView) => void;
}

/**
 * 会话关联图抽屉（原型 `sessionGraphHtml`，§3.15.2）：共同旧版 → 各账号副本分支。
 * 每个副本显示**相对共同旧版的差异**，而不是只给时间戳（时间新 ≠ 内容新）。
 */
function SessionGraphSheet({ group, onSyncGroup, busy, onUnifyGroup }: SessionGraphSheetProps) {
  const t = useT();
  /* ★ 只在真有可同步内容时才给出口（无项 ⇒ 不渲染按钮，§3.4 规则 1）。 */
  const items = buildSyncItems(group);

  return (
    <>
      <SheetHeader>
        <SheetTitle className="truncate">{groupTitle(t, group)}</SheetTitle>
        <SheetDescription>{t("sessions.replica.groupTitle", { count: group.rows.length })}</SheetDescription>
      </SheetHeader>

      <div className="flex-1 overflow-y-auto p-5">
        {/* ★ 关联图（`commonPrefix === null` ⇒ `unknown`，**无数据不渲染**）。 */}
        {group.commonPrefix !== null ? (
          <div>
            <div className="flex flex-col gap-0.5 rounded-[10px] border-l-[3px] bg-muted px-3 py-2.5">
              <b className="text-[12.5px] font-semibold">
                {t("sessions.replica.commonPrefix", { count: group.commonPrefix })}
              </b>
            </div>
            <div className="mt-2.5 flex flex-col gap-2 border-l-2 border-dashed pl-3.5">
              {group.rows.map((row) => (
                <div
                  key={`${row.accountUid}\u001f${row.sessionId}`}
                  className="flex items-center justify-between gap-2.5 rounded-[10px] bg-muted px-2.5 py-2"
                >
                  <span className="truncate text-[12.5px] font-medium">{row.accountLabel}</span>
                  <Badge variant={branchVariant(group, row.sessionId)} className="shrink-0">
                    {t("sessions.replica.delta", { count: group.deltas.get(row.sessionId) ?? 0 })}
                  </Badge>
                </div>
              ))}
            </div>
          </div>
        ) : null}

        {/* ★ 诚实标注：ledger 只覆盖「经本应用复制过」的会话（§3.5）。
            `partial` 恒 true ⇒ 这段文案恒渲染；绑在 `partial` 上是为了让「诚实性」承重。 */}
        {group.partial ? (
          <p className="mt-3 text-xs text-muted-foreground">{t("sessions.replica.partialNote")}</p>
        ) : null}

        <div className="mt-3 divide-y rounded-xl border">
          {group.rows.map((row) => (
            <div
              key={`${row.accountUid}\u001f${row.sessionId}`}
              className="flex items-start gap-3 px-4 py-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-sm font-medium">{row.accountLabel}</span>
                  {row.hasHistory ? (
                    <Badge variant="outline" className="shrink-0 gap-1">
                      <History className="size-3" aria-hidden="true" />
                      {t("sessions.replica.hasHistory")}
                    </Badge>
                  ) : null}
                </div>
                <p className="mt-0.5 truncate text-sm text-muted-foreground">{row.title}</p>
                {/* 会话 id 是 ledger 里**确有的**身份信息：标题取不到时，用户仍能靠它核对。 */}
                <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground/70">
                  {row.sessionId}
                </p>
              </div>
              {/* `updatedAt` **只作事实**展示，不派生「落后 / 最新」（§3.15.2）。 */}
              <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                {row.updatedAt !== null
                  ? formatDate(row.updatedAt)
                  : t("sessions.replica.updatedUnknown")}
              </span>
            </div>
          ))}
        </div>

        {/* ★★ D2：未分叉组的**常驻**说明 ——「同步是加法」。
            原先只有按钮的 `title=`（hover 才可见），而这恰恰是用户**最怕被覆盖**时
            最需要看到的一句。`title` 保留（给键盘/读屏用户），但不能是唯一载体。 */}
        {items.length > 0 && onSyncGroup ? (
          <p className="mt-3 text-xs leading-5 text-muted-foreground" data-sync-additive-note>
            {t("sessions.sync.additiveNote")}
          </p>
        ) : null}

        {/* ★★ D1：分叉组的**常驻**分歧提示（`role="alert"`）。
            分歧无法用加法合并、必须让用户先选保留哪一份 —— 这是「不许静默覆盖」
            的最后一道闸。只挂在 `title=` 上等于没有：没人会为了读一句话去 hover 每个按钮。 */}
        {group.status === "diverge" && onUnifyGroup && group.rows.length >= 2 ? (
          <Alert role="alert" variant="destructive" className="mt-3" data-unify-warning>
            <GitMerge className="size-4" aria-hidden="true" />
            <AlertDescription>{t("sessions.unify.hint")}</AlertDescription>
          </Alert>
        ) : null}

        <div className="mt-4 flex flex-wrap items-center gap-2">
          {/* ★ P8「安全同步」：**只在真有可同步内容时**才给出口（`behind` + 有增量）。
              文案如实写「加法」—— 用户对「同步」最大的恐惧是被覆盖（§3.15.4）。 */}
          {(() => {
            if (items.length === 0 || !onSyncGroup) return null;
            return (
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                data-sync-group={group.key}
                title={t("sessions.sync.hint")}
                onClick={() => onSyncGroup(group)}
              >
                {busy ? (
                  <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
                ) : (
                  <Download className="size-3.5" aria-hidden="true" />
                )}
                {t("sessions.sync.action", { count: items.length })}
              </Button>
            );
          })()}
          {/* ★ P8「分歧处理」：**只在 `diverge`** 给出口 —— 分歧无法用加法合并，
              必须让用户先选保留哪一份（§3.15.2「不许静默覆盖」）。 */}
          {group.status === "diverge" && onUnifyGroup && group.rows.length >= 2 ? (
            <Button
              size="sm"
              variant="outline"
              data-unify-group={group.key}
              title={t("sessions.unify.hint")}
              onClick={() => onUnifyGroup(group)}
            >
              <GitMerge className="size-3.5" aria-hidden="true" />
              {t("sessions.unify.action", { count: group.rows.length - 1 })}
            </Button>
          ) : null}
        </div>
      </div>
    </>
  );
}
