/**
 * 会话副本只读视图（P8 T02）—— 按「组」列出每个账号里的副本。
 *
 * ## 只读 · 诚实标注（`P8-ARCH.md` §1.4 / §1.5）
 *
 * 数据源是 copy ledger（**内部去重账本被提升为产品语义**）⇒ 它**只覆盖「经本应用复制过」
 * 的会话**。因此：
 * - 每个组头部固定标注「**仅显示经本应用复制过会话**」（由 `group.partial` 承载，恒 `true`）；
 * - **禁止**任何「全部副本」口径的文案（§3.5 不假承诺）。
 *
 * ## 状态 / 分歧（**P8 新增：内容指纹落地后解禁**）
 *
 * ⚠️ **本文件早期版本「刻意不做状态」**，依据是「ledger 值是字符串 ⇒ 无消息计数」——
 * **那个前提已不成立**：现在从会话 jsonl 取**内容指纹**（`lineCount` + 逐行 hash），
 * 由 `deriveGroupStatus` 派生 `latest` / `behind` / `diverge` / `unknown`。
 * ⇒ 因此这里**现在渲染状态徽标与共同旧版**。早期那句「不做状态」已删 —— **前提变了，
 * 结论就该变**（留着会让后来人以为这是"有意不做"）。
 *
 * ⚠️ **仍然禁止**：用 `updatedAt` 派生「落后 / 最新」（`P8-ARCH.md` §1.2「时间新 ≠ 内容新」）。
 * 下面那处 `formatDate(row.updatedAt)` 依旧**只作事实展示**，不参与任何判定。
 *
 * ## 白屏防线（issue #2）
 *
 * 会话标题 / 账号显示名在 `lib/session-replicas.ts` 已过 `displayText` ⇒ 这里拿到的**一定**
 * 是字符串，可安全当 React 子节点渲染。
 */

import { Download, GitBranch, History, Loader2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
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
}

export function SessionReplicaView({ groups, onSyncGroup, busyGroupKeys }: SessionReplicaViewProps) {
  const t = useT();

  return (
    <div className="flex flex-col gap-4">
      {groups.map((group) => (
        <Card key={group.key} className="gap-0 overflow-hidden py-0">
          <div className="flex items-center justify-between gap-2 border-b px-5 py-3">
            <div className="flex min-w-0 items-center gap-2">
              <GitBranch className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="truncate text-sm font-medium">
                {t("sessions.replica.groupTitle", { count: group.rows.length })}
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              {/* ★ **内容级**状态（来自 fingerprint 的公共前缀，**不是** updatedAt）。 */}
              <Badge
                variant={group.status === "diverge" ? "destructive" : "secondary"}
                data-status={group.status}
              >
                {t(`sessions.replica.status.${group.status}`)}
              </Badge>
              {/* ★ 诚实标注：ledger 只覆盖「经本应用复制过」的会话（§3.5）。
                  `partial` 恒 true ⇒ 这段文案恒渲染；把它绑在 `partial` 上，是为了让
                  「诚实性」在类型 / 运行时**都**是承重的（改 `partial` 即丢标注）。 */}
              {group.partial ? (
                <span className="shrink-0 text-xs text-muted-foreground">
                  {t("sessions.replica.partialNote")}
                </span>
              ) : null}
              {/* ★ P8「安全同步」：**只在真有可同步内容时**才给出口（`behind` + 有增量）。
                  文案如实写「加法」—— 用户对「同步」最大的恐惧是被覆盖（§3.15.4）。 */}
              {(() => {
                const items = buildSyncItems(group);
                if (items.length === 0 || !onSyncGroup) return null;
                const busy = busyGroupKeys?.has(group.key) === true;
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
            </div>
          </div>
          {/* ★ 共同旧版（所有副本两两公共前缀的**最小值**）—— **内容级**事实，不是时间。
              状态为 `unknown` 时不渲染（`commonPrefix === null`，**无数据不渲染**）。 */}
          {group.commonPrefix !== null ? (
            <p className="border-b bg-muted/30 px-5 py-2 text-xs text-muted-foreground">
              {t("sessions.replica.commonPrefix", { count: group.commonPrefix })}
            </p>
          ) : null}
          <CardContent className="divide-y px-0 py-0">
            {group.rows.map((row) => (
              <div
                key={`${row.accountUid}\u001f${row.sessionId}`}
                className="flex items-start gap-3 px-5 py-3"
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
                  <p className="mt-0.5 flex items-center gap-2 truncate font-mono text-[11px] text-muted-foreground/70">
                    <span className="truncate">{row.sessionId}</span>
                    {/* ★ 该副本**相对共同旧版**新增的条数（**内容级 delta**，非时间差）。 */}
                    {group.deltas.has(row.sessionId) ? (
                      <span className="shrink-0">
                        {t("sessions.replica.delta", {
                          count: group.deltas.get(row.sessionId) ?? 0,
                        })}
                      </span>
                    ) : null}
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
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
