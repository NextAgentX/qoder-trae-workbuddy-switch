/**
 * P8 **分歧处理**：选择保留哪一份（**覆盖式**统一）。
 *
 * ## 为什么必须是一个「先选、再确认」的对话框
 *
 * 分歧的定义是「双方在共同旧版之后**各自改过**」⇒ 加法**永远**合不到一起
 * （各自都有对方没有的内容）。`PLAN.md` §3.15.2 因此写死：
 * 「**有分叉时必须先选保留哪一份**才能统一（**不许静默覆盖**）」。
 *
 * ⇒ 本对话框承担两件事，缺一不可：
 * 1. **让用户选**（`RadioGroup` 单选语义 —— 见 `ui/radio-group.tsx` 头注释为什么不能手写）；
 * 2. **如实说清代价**：会覆盖其余副本、它们各自的 `delta` 条内容会丢、覆盖前自动备份。
 *    原型断言（`verify-behaviors.cjs:981`）要求分叉组「**只能先选保留哪一份**」，
 *    且**不许**退回成「同步到全部副本」那种静默覆盖。
 *
 * ## 默认值
 *
 * 默认选中**内容最全**的那一份（`delta` 最大 = 行数最多）—— 这是**内容级**事实，
 * 不是猜测。但默认值**不构成确认**：用户仍必须显式点「统一到其他副本」。
 */

import { useEffect, useState } from "react";
import { TriangleAlert } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { useT } from "@/lib/i18n";
import { lostLinesFor } from "@/lib/session-replicas";
import type { SessionGroupView } from "@/lib/session-replicas";

export interface UnifyReplicasDialogProps {
  /** 要处理的分歧组；`null` = 关闭。 */
  group: SessionGroupView | null;
  onOpenChange: (open: boolean) => void;
  /** 统一进行中（按钮进「操作进行中」态，防止重复提交）。 */
  busy: boolean;
  /** 用户确认后回调，参数是**要保留的那一份**的会话 id。 */
  onConfirm: (keepSessionId: string) => void;
}

export function UnifyReplicasDialog({
  group,
  onOpenChange,
  busy,
  onConfirm,
}: UnifyReplicasDialogProps) {
  const t = useT();
  const [keep, setKeep] = useState("");

  /* 每次打开都重置选择 —— 默认选**内容最全**的那一份（delta 最大）。 */
  useEffect(() => {
    if (!group) {
      setKeep("");
      return;
    }
    const deltaOf = (id: string) => group.deltas.get(id) ?? 0;
    const best = [...group.rows].sort((a, b) => deltaOf(b.sessionId) - deltaOf(a.sessionId))[0];
    setKeep(best?.sessionId ?? "");
  }, [group]);

  if (!group) return null;

  const deltaOf = (id: string) => group.deltas.get(id) ?? 0;
  const maxDelta = Math.max(...group.rows.map((row) => deltaOf(row.sessionId)));
  const targets = group.rows.filter((row) => row.sessionId !== keep);

  /* ★ 会丢多少条：逐副本求和。**任一副本说不准就整体标注「未知」** ——
     不臆造一个编出来的数字（§3.5 不许假承诺）。 */
  const losses = targets.map((row) => lostLinesFor(group, row.sessionId));
  const totalLost = losses.every((n) => n !== null)
    ? losses.reduce<number>((sum, n) => sum + (n ?? 0), 0)
    : null;

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg" data-slot="unify-dialog">
        <DialogHeader>
          <DialogTitle>{t("sessions.unify.title")}</DialogTitle>
          <DialogDescription>{t("sessions.unify.desc")}</DialogDescription>
        </DialogHeader>

        <RadioGroup value={keep} onValueChange={setKeep} aria-label={t("sessions.unify.title")}>
          {group.rows.map((row) => {
            const delta = deltaOf(row.sessionId);
            const isBest = delta === maxDelta;
            return (
              <div
                key={row.sessionId}
                className="flex items-start gap-3 rounded-lg border px-3 py-2"
                data-unify-row={row.sessionId}
              >
                <RadioGroupItem
                  value={row.sessionId}
                  id={`unify-${row.sessionId}`}
                  className="mt-0.5"
                />
                <Label
                  htmlFor={`unify-${row.sessionId}`}
                  className="min-w-0 flex-1 cursor-pointer font-normal"
                >
                  <span className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium">{row.accountLabel}</span>
                    {/* 「最完整」是**内容级**事实（行数最多），不是猜测。 */}
                    {isBest ? <Badge variant="secondary">{t("sessions.unify.mostComplete")}</Badge> : null}
                  </span>
                  <span className="mt-0.5 block truncate font-mono text-[11px] text-muted-foreground">
                    {row.sessionId}
                  </span>
                  <span className="mt-0.5 block text-xs text-muted-foreground">
                    {t("sessions.replica.delta", { count: delta })}
                  </span>
                </Label>
              </div>
            );
          })}
        </RadioGroup>

        {/* ★ 代价必须写在按钮**旁边**，不能只藏在描述里 —— 这是不可逆操作（§3.5）。 */}
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertDescription>
            {totalLost === null
              ? t("sessions.unify.overwriteUnknown", { n: targets.length })
              : t("sessions.unify.overwrite", { n: targets.length, lost: totalLost })}
          </AlertDescription>
        </Alert>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            {t("sessions.unify.cancel")}
          </Button>
          <Button
            variant="destructive"
            disabled={busy || keep === ""}
            data-slot="unify-confirm"
            onClick={() => onConfirm(keep)}
          >
            {t("sessions.unify.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
