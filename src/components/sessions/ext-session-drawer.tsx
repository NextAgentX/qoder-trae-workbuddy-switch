import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { FolderOpen, Loader2, MessageSquare, TriangleAlert } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { copyExtSessions, listExtSessions } from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import type { AccountMeta, ExtClientId, ExtSession, ExtSessionsResponse } from "@/lib/types";

/**
 * 账号展示名 —— 与账号列表**同一降级链**（`nickname → email → id`）。
 *
 * ⚠️ 不自己拼 `account.name`：`AccountMeta` **没有** `name` 字段（那是原型夹具的写法），
 * 而且 `nickname` 在后端是 `$wbEncrypted` 信封 ⇒ 必须过 `displayText` 归一
 * （脏值直接渲染会让 React 卸载整棵树，见 issue #2）。
 */
function accountLabel(account: AccountMeta): string {
  return displayText(account.nickname) ?? displayText(account.email) ?? account.id;
}

/**
 * P14 S4a · 插件侧「复制会话」抽屉（VS Code 插件 / CodeBuddy IDE）。
 *
 * ## 为什么**不**复用桌面版的迁移向导（`SwitchAccountDialog`）
 *
 * 两者模型根本不同：
 * - 桌面版是跨账号**副本组**（有账本、可双向同步、有「谁落后 / 谁分叉」）；
 * - 插件侧是**一次性单向复制**，副本**恒取新 id** ⇒ 构不成可同步的副本组。
 *
 * 所以这里**没有**「同步 / 统一」——它们只在副本组上才有意义。
 * 也正因如此，入口在**账号页**，而**不是**「关联会话」页的客户端 Tab
 * （塞进去只会多出一个永远空的组列表）。
 *
 * ## 三条诚实规则（每条都在原型 `verify-behaviors.cjs` 里有断言钉着）
 *
 * 1. **未检测到客户端**（`dataRoot === null`）与「这个账号没有会话」**文案与出口都不同**；
 * 2. `skipped > 0` **如实标注**读不出来的工作区数，不假装「全都列在这儿了」；
 * 3. 目标账号 = 本区域**除源账号以外**的全部账号（不写死列表）。
 *
 * ## 复制是**加法**
 *
 * 副本恒取新 id ⇒ 目标账号**已有内容一个字都不会被动**。这句话必须出现在界面上 ——
 * 用户最怕的正是「复制会不会覆盖我目标账号的会话」。
 */
interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 客户端（由账号绑定的程序位决定，见 `extClientForProgram`）。 */
  client: ExtClientId;
  /** 来源账号。 */
  account: AccountMeta;
  /** 目标候选（调用方已排除源账号）。 */
  targets: AccountMeta[];
  /**
   * 预选的目标账号 id。
   *
   * ★ issue #86：从「切换成功」的 toast 进来时，用户要的**就是**刚切到的那个账号 ——
   * 不预选的话默认落到 `targets[0]`（可能是另一个号），用户点了复制才发现复制到了别处。
   */
  initialTargetId?: string;
  /** 复制成功后的回调（供列表页刷新）。 */
  onCopied?: () => void;
}

export function ExtSessionDrawer({
  open,
  onOpenChange,
  client,
  account,
  targets,
  initialTargetId,
  onCopied,
}: Props) {
  const t = useT();
  const [loading, setLoading] = useState(false);
  const [response, setResponse] = useState<ExtSessionsResponse | null>(null);
  /** 读取失败（与「未检测到客户端」**不是一回事**：那是后端如实返回的 `dataRoot: null`）。 */
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [targetId, setTargetId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** 逐条结果（部分失败时**如实列出**失败的那些，不把整批说成成功）。 */
  const [failures, setFailures] = useState<Array<{ title: string; error: string }>>([]);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    setFailures([]);
    try {
      const res = await listExtSessions(client, account.id);
      setResponse(res);
      // 默认**全选**：复制是加法（新 id、不覆盖）⇒ 默认带走全部是安全且省事的。
      setPicked(new Set(res.sessions.map((session) => session.id)));
      // 预选目标（见 `initialTargetId`）；没有预选值时才回落第一个候选。
      const preferred = initialTargetId
        ? targets.find((item) => item.id === initialTargetId)
        : undefined;
      setTargetId(preferred?.id ?? targets[0]?.id ?? null);
    } catch (cause) {
      setResponse(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [client, account.id, targets, initialTargetId]);

  useEffect(() => {
    if (open) void reload();
  }, [open, reload]);

  const sessions: ExtSession[] = response?.sessions ?? [];
  const target = targets.find((item) => item.id === targetId) ?? null;
  const allPicked = sessions.length > 0 && picked.size === sessions.length;

  const runCopy = async () => {
    if (!target || picked.size === 0) return;
    setBusy(true);
    setFailures([]);
    try {
      const report = await copyExtSessions(
        client,
        account.id,
        target.id,
        sessions.filter((session) => picked.has(session.id)).map((session) => ({
          workspaceHash: session.workspaceHash,
          conversationId: session.id,
        })),
      );
      const titleOf = (id: string) => sessions.find((session) => session.id === id)?.title ?? id;
      setFailures(
        report.errors.map((item) => ({
          title: titleOf(item.conversationId),
          error: item.error,
        })),
      );
      if (report.copiedCount > 0) {
        toast.success(
          t("sessions.ext.copied", { count: report.copiedCount, target: accountLabel(target) }),
        );
        onCopied?.();
      }
      if (report.failCount > 0) {
        // ★ 部分失败**不当成功报**：逐条如实列出（§3.6）。
        toast.warning(t("sessions.ext.partial", { count: report.failCount }));
      }
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="w-full sm:max-w-md">
        <SheetHeader>
          <SheetTitle className="truncate">
            {t("sessions.ext.title", { client: t(`sessions.ext.client.${client === "vscode" ? "vscode" : "codebuddyIde"}`) })}
          </SheetTitle>
          <SheetDescription>{accountLabel(account)}</SheetDescription>
        </SheetHeader>

        <div className="flex-1 overflow-y-auto p-5">
          {loading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground" aria-busy="true">
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
              {t("sessions.ext.loading")}
            </div>
          ) : error ? (
            /* 读取**失败**（网络 / 后端报错）—— 与「未检测到客户端」不同，必须能重试。 */
            <EmptyState
              icon={TriangleAlert}
              title={t("sessions.ext.error.title")}
              description={error}
              action={
                <Button type="button" size="sm" onClick={() => void reload()}>
                  {t("sessions.ext.retry")}
                </Button>
              }
            />
          ) : response && response.dataRoot === null ? (
            /* ★ 规则 1（前半）：**未检测到客户端** —— 与「没有会话」是两件事。 */
            <EmptyState
              icon={FolderOpen}
              title={t("sessions.ext.missing.title")}
              description={t("sessions.ext.missing.desc")}
            />
          ) : sessions.length === 0 ? (
            /* ★ 规则 1（后半）：装了客户端但该账号没有会话 —— 给出**数据目录**作为可核对的事实。 */
            <EmptyState
              icon={MessageSquare}
              title={t("sessions.ext.empty.title")}
              description={t("sessions.ext.empty.desc", { root: response?.dataRoot ?? "" })}
            />
          ) : (
            <>
              {response && response.skipped > 0 ? (
                /* ★ 规则 2：读不出来的工作区**如实标注**（不假装全列出来了）。 */
                <p
                  className="mb-3 flex items-start gap-1.5 rounded-[10px] border-l-[3px] border-[var(--warning)] bg-muted px-3 py-2 text-[12px] text-muted-foreground"
                  data-ext-skipped={response.skipped}
                >
                  <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                  {t("sessions.ext.skipped", { count: response.skipped })}
                </p>
              ) : null}

              <div className="mb-2 flex items-center justify-between gap-2">
                <h4 className="text-[12.5px] font-semibold">
                  {t("sessions.ext.pick")} <Badge variant="secondary">{sessions.length}</Badge>
                </h4>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-[26px] px-2 text-[11px]"
                  onClick={() =>
                    setPicked(allPicked ? new Set() : new Set(sessions.map((s) => s.id)))
                  }
                >
                  {allPicked ? t("sessions.ext.pickNone") : t("sessions.ext.pickAll")}
                </Button>
              </div>

              <div className="flex flex-col gap-0.5" data-ext-list="1">
                {sessions.map((session) => {
                  const on = picked.has(session.id);
                  return (
                    <label
                      key={session.id}
                      className="flex cursor-pointer items-start gap-2 rounded-[8px] px-2 py-1.5 hover:bg-muted"
                      data-ext-session={session.id}
                    >
                      <Checkbox
                        className="mt-0.5"
                        checked={on}
                        onCheckedChange={() =>
                          setPicked((prev) => {
                            const next = new Set(prev);
                            if (next.has(session.id)) next.delete(session.id);
                            else next.add(session.id);
                            return next;
                          })
                        }
                      />
                      <span className="min-w-0 flex-1">
                        <b className="block truncate text-[12.5px] font-medium">{session.title}</b>
                        <i className="block truncate font-mono text-[11px] not-italic text-muted-foreground/70">
                          {session.workspaceHash}
                          {session.hasHistory ? "" : ` · ${t("sessions.ext.noHistory")}`}
                        </i>
                      </span>
                    </label>
                  );
                })}
              </div>

              <h4 className="mb-2 mt-4 text-[12.5px] font-semibold">{t("sessions.ext.target")}</h4>
              {targets.length === 0 ? (
                <p className="text-xs text-muted-foreground">{t("sessions.ext.noTarget")}</p>
              ) : (
                <div className="flex flex-wrap gap-1.5">
                  {targets.map((item) => (
                    <Button
                      key={item.id}
                      type="button"
                      variant={item.id === targetId ? "secondary" : "outline"}
                      size="sm"
                      className="h-[26px] rounded-[8px] px-2 text-[11px]"
                      aria-pressed={item.id === targetId}
                      onClick={() => setTargetId(item.id)}
                    >
                      {accountLabel(item)}
                    </Button>
                  ))}
                </div>
              )}

              {/* ★ 复制是**加法** —— 用户最怕的就是「会不会覆盖我目标账号的会话」。 */}
              <p className="mt-3 text-[11.5px] text-muted-foreground">
                {t("sessions.ext.additiveNote")}
              </p>

              <Button
                type="button"
                className="mt-2.5 w-full"
                disabled={busy || picked.size === 0 || !target}
                onClick={() => void runCopy()}
              >
                {busy ? <Loader2 className="size-4 animate-spin" aria-hidden="true" /> : null}
                {t("sessions.ext.copy", { count: picked.size, target: target ? accountLabel(target) : "" })}
              </Button>
              {picked.size === 0 ? (
                <p className="mt-1.5 text-[11.5px] text-muted-foreground">
                  {t("sessions.ext.pickFirst")}
                </p>
              ) : null}

              {failures.length > 0 ? (
                <div className="mt-3 rounded-[10px] border border-destructive/40 bg-destructive/5 p-3">
                  <b className="text-[12px] text-destructive">
                    {t("sessions.ext.failed", { count: failures.length })}
                  </b>
                  <ul className="mt-1.5 flex flex-col gap-1">
                    {failures.map((item) => (
                      <li key={`${item.title}-${item.error}`} className="text-[11.5px] text-muted-foreground">
                        <span className="font-medium">{item.title}</span>：{item.error}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
