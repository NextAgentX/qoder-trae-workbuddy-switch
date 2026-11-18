import type { ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";

import { DemoAction } from "@/components/demo-action";
import { AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import * as api from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { useT, type Translate } from "@/lib/i18n";
import type {
  AccountMeta,
  SessionGroupActionReport,
  SessionGroupClient,
  SessionGroupDetail,
  SessionGroupLinkReport,
  SessionGroupMemberDetail,
  SessionGroupPairPreview,
  SessionMemberVersionStatus,
  SessionSyncMode,
  WbVariant,
} from "@/lib/types";

export function clientLabel(t: Translate, client: SessionGroupClient): string {
  if (client === "vscodeExt") return t("sessions.workbench.vsCode");
  return client === "codebuddyIde" ? "CodeBuddy IDE" : "WorkBuddy";
}

export function variantLabel(t: Translate, variant: WbVariant): string {
  return t(variant === "ai" ? "sessions.links.region.global" : "sessions.links.region.cn");
}

export function accountLabel(t: Translate, account: AccountMeta, fallbackVariant: WbVariant = "cn"): string {
  const name = displayText(account.nickname) ?? displayText(account.email) ?? account.uid ?? account.id;
  const variant = account.region === "global" ? "ai" : account.region === "cn" ? "cn" : fallbackVariant;
  return `${name} · ${variantLabel(t, variant)}`;
}

export function statusLabel(t: Translate, status: SessionMemberVersionStatus): string {
  if (status === "missing" || status === "stale" || status === "superseded") {
    return t(`sessions.workbench.${status}`);
  }
  return t(`sessions.replica.status.${status}`);
}

export function modeLabel(t: Translate, mode: SessionSyncMode): string {
  return t(`sessions.workbench.mode.${mode === "unifyOverwrite" ? "overwrite" : mode}`);
}

export function matchesPreview(
  preview: SessionGroupPairPreview, detail: SessionGroupDetail, sourceId: string, targetId: string,
): boolean {
  return preview.client === detail.client && preview.groupId === detail.groupId
    && (preview.variantScope ?? undefined) === (detail.variantScope ?? undefined)
    && preview.sourceMemberId === sourceId && preview.targetMemberId === targetId;
}

export function restartNotice(t: Translate, client: SessionGroupClient): string {
  if (client === "vscodeExt") return t("sessions.workbench.restartVsCode");
  return t(client === "codebuddyIde" ? "sessions.workbench.ideClose" : "sessions.workbench.restartWorkBuddy");
}

export async function editorRestartRequired(client: SessionGroupClient, accountIds: (string | null)[]): Promise<boolean> {
  const ids = [...new Set(accountIds.filter((id): id is string => Boolean(id)))];
  if (client !== "vscodeExt" || ids.length === 0) return false;
  return (await api.vscodeRestartPrecheck(ids)).required;
}

export function preferredSource(detail: SessionGroupDetail, previous?: string): string {
  const eligible = detail.members.filter((member) => member.linkState === "active" && member.canBeSource);
  return [previous, detail.safeSourceMemberId].find((id) => eligible.some((member) => member.memberId === id))
    ?? eligible[0]?.memberId ?? "";
}

export function notifySync(t: Translate, report: Pick<SessionGroupActionReport, "synced" | "skipped" | "errors"
  | "restartedVariants" | "restartedEditor" | "editorError" | "temporaryFiles" | "needsRecovery">) {
  if (report.synced.length) {
    const description = report.restartedVariants?.length
      ? t("sessions.workbench.restarted", { versions: report.restartedVariants.map((variant) => variantLabel(t, variant)).join(", ") })
      : report.restartedEditor ? t("sessions.workbench.vsCodeReopened") : undefined;
    toast.success(t("sessions.workbench.synced", { count: report.synced.length }), { description });
  }
  if (report.skipped.length) toast.warning(t("sessions.workbench.syncSkipped", { count: report.skipped.length }), {
    description: report.skipped.map((item) => item.message).filter(Boolean).join("\n"),
  });
  if (report.errors.length) toast.error(t("sessions.workbench.syncErrors", { count: report.errors.length }), {
    description: report.errors.map((item) => item.error).join("\n"),
  });
  if (report.needsRecovery) toast.error(t("sessions.workbench.needsRecovery"), {
    description: report.temporaryFiles?.map((item) => item.reason).join("\n"),
  });
  if (report.editorError) toast.warning(t("sessions.workbench.vsCodeReopenFailed"), { description: report.editorError });
  if (!report.synced.length && !report.skipped.length && !report.errors.length && !report.needsRecovery) {
    toast.info(t("sessions.workbench.nothingToSync"));
  }
}

export function notifyLink(t: Translate, report: SessionGroupLinkReport, member = false): boolean {
  const description = report.restartedEditor ? t("sessions.workbench.vsCodeReopened") : undefined;
  if (report.status === "linked") {
    toast.success(member ? t("sessions.workbench.addDone")
      : t("sessions.workbench.linked", { count: report.report?.copied?.length ?? 0 }), { description });
  } else if (report.status === "alreadyLinked") {
    toast.info(t("sessions.workbench.addExisting"));
  } else if (report.status === "copiedUnlinked") {
    toast.warning(t("sessions.workbench.copiedUnlinked"), {
      description: report.linkErrors?.map((item) => item.error).join("\n"),
    });
  } else {
    toast.error(t("sessions.workbench.copyFailed"), { description: report.error });
  }
  if (report.report?.errors?.length) toast.error(t("sessions.workbench.copyFailed"), {
    description: report.report.errors.map((item) => item.error).join("\n"),
  });
  if (report.skipped?.length) toast.warning(t("sessions.workbench.linkSkipped", { count: report.skipped.length }), {
    description: report.skipped.map((item) => item.error).join("\n"),
  });
  if (report.editorError) toast.warning(t("sessions.workbench.vsCodeReopenFailed"), { description: report.editorError });
  if (report.needsRecovery) toast.error(t("sessions.workbench.needsRecovery"));
  return report.status === "linked" || report.status === "alreadyLinked";
}

export function memberLabel(t: Translate, member: SessionGroupMemberDetail): string {
  return `${member.accountName} · ${variantLabel(t, member.variant)}`;
}

export function SessionActionDialog({
  open, onClose, title, description, children, confirm, onConfirm, busy, destructive,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description: string;
  children?: ReactNode;
  confirm?: string;
  onConfirm?: () => void;
  busy?: boolean;
  destructive?: boolean;
}) {
  const t = useT();
  return (
    <AlertDialog open={open} onOpenChange={(next) => { if (!next && !busy) onClose(); }}>
      <AlertDialogContent onEscapeKeyDown={(event) => { if (busy) event.preventDefault(); }}>
        <div className="space-y-1.5">
          <AlertDialogTitle className="text-base font-semibold">{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </div>
        {children}
        <div className="flex flex-wrap justify-end gap-2">
          <AlertDialogCancel disabled={busy}>{t("sessions.workbench.cancel")}</AlertDialogCancel>
          {confirm && onConfirm && (
            <DemoAction>
              <Button variant={destructive ? "destructive" : "default"} disabled={busy} onClick={onConfirm}>
                {busy ? <Loader2 className="animate-spin" /> : null}
                {busy ? t("sessions.workbench.working") : confirm}
              </Button>
            </DemoAction>
          )}
        </div>
      </AlertDialogContent>
    </AlertDialog>
  );
}
