import { useEffect, useState } from "react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import * as api from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import type { SessionGroupDetail, SessionGroupMemberDetail, SessionSyncMode } from "@/lib/types";
import {
  accountLabel, editorRestartRequired, matchesPreview, memberLabel, notifyLink, notifySync,
  preferredSource, restartNotice, SessionActionDialog, statusLabel,
} from "./linked-session-ui";
import { SessionPairDialog } from "./session-pair-dialog";
import { SessionRelationshipGraph } from "./session-relationship-graph";

type TargetSelection = {
  targetMemberId: string;
  previewToken: string;
  mode: Exclude<SessionSyncMode, "statusOnly">;
};

type Action =
  | { type: "unlink"; member: SessionGroupMemberDetail }
  | { type: "add"; accountId: string }
  | { type: "batch" }
  | { type: "unify"; source: SessionGroupMemberDetail; targets: TargetSelection[] };

export function SessionGroupActions({ detail, onChanged, onRemoved, onDelete }: {
  detail: SessionGroupDetail;
  onChanged: () => void;
  onRemoved: () => void;
  onDelete: () => void;
}) {
  const t = useT();
  const { client, groupId } = detail;
  const variantScope = detail.variantScope ?? undefined;
  const args = { client, groupId, variantScope };
  const [sourceId, setSourceId] = useState(() => preferredSource(detail));
  const [addTarget, setAddTarget] = useState("");
  const [memberDetails, setMemberDetails] = useState<SessionGroupMemberDetail>();
  const [action, setAction] = useState<Action>();
  const [busy, setBusy] = useState(false);
  const current = detail.members.filter((member) => member.linkState === "active");
  const sources = current.filter((member) => member.canBeSource);
  const behind = current.filter((member) => member.versionStatus === "behind");
  const chosenSourceId = preferredSource(detail, sourceId);
  const canUnify = client !== "codebuddyIde" && detail.summaryStatus === "diverge";
  const [currentUids, setCurrentUids] = useState<Set<string>>(new Set());

  useEffect(() => {
    let active = true;
    const inputs = client === "workbuddy"
      ? [api.getStatus("cn").then((status) => status.current?.uid), api.getStatus("global").then((status) => status.current?.uid)]
      : client === "vscodeExt"
        ? [api.getVscodeCodebuddyPluginStatus().then((status) => status.activeAccountId)]
        : [((variantScope === "ai" ? api.getCodebuddyIdeStatus() : api.getCodebuddyCnIdeStatus())).then((status) => status.activeAccountId)];
    void Promise.allSettled(inputs).then((results) => {
      if (active) setCurrentUids(new Set(results.flatMap((result) =>
        result.status === "fulfilled" && result.value ? [result.value] : [])));
    });
    return () => { active = false; };
  }, [client, variantScope, detail]);

  async function previewUnify(source: SessionGroupMemberDetail) {
    setBusy(true);
    try {
      const targets = await Promise.all(current.filter((member) => member.memberId !== source.memberId).map(async (target) => {
        const preview = await api.previewSessionGroupPair({
          ...args, sourceMemberId: source.memberId, targetMemberId: target.memberId,
        });
        if (!matchesPreview(preview, detail, source.memberId, target.memberId)) {
          throw new Error(t("sessions.workbench.previewMismatch"));
        }
        if (preview.verdict === "identical") return [];
        const mode: TargetSelection["mode"] | undefined = preview.availableModes.includes("fastForward") ? "fastForward"
          : preview.availableModes.includes("overwrite") ? "overwrite"
            : preview.verdict === "ahead" ? "unifyOverwrite" : undefined;
        if (!preview.previewToken || !mode) {
          throw new Error(memberLabel(t, target) + " · " + preview.reason);
        }
        return [{ targetMemberId: target.memberId, previewToken: preview.previewToken, mode }];
      }));
      const selections = targets.flat();
      if (selections.length) setAction({ type: "unify", source, targets: selections });
      else toast.info(t("sessions.workbench.nothingToSync"));
    } catch (error) {
      toast.error(t("sessions.workbench.unifyBlocked"), { description: api.asError(error) });
    } finally {
      setBusy(false);
    }
  }

  async function confirm() {
    if (!action || busy) return;
    setBusy(true);
    try {
      switch (action.type) {
        case "unlink": {
          const report = await api.unlinkSessionGroupMember({ ...args, memberId: action.member.memberId });
          toast.success(t("sessions.workbench.unlinkDone", { name: action.member.accountName }));
          if (report.status === "groupRemoved") onRemoved();
          break;
        }
        case "add": {
          const restart = await editorRestartRequired(client, [action.accountId]);
          const report = await api.addSessionGroupMember({ ...args, sourceMemberId: chosenSourceId, targetAccountId: action.accountId, restart });
          if (notifyLink(t, report, true)) setAddTarget("");
          break;
        }
        case "batch": {
          const restart = await editorRestartRequired(client, behind.map((member) => member.accountId));
          notifySync(t, await api.syncSessionGroupSafeBatch(client, groupId, variantScope, restart));
          break;
        }
        case "unify": {
          if (client === "codebuddyIde") return;
          const selected = new Set(action.targets.map((target) => target.targetMemberId));
          const restart = await editorRestartRequired(client, current.filter((member) => selected.has(member.memberId)).map((member) => member.accountId));
          notifySync(t, await api.syncSessionGroupUnify({ ...args, client, sourceMemberId: action.source.memberId, targets: action.targets, restart }));
          break;
        }
      }
      setAction(undefined);
      onChanged();
    } catch (error) {
      toast.error(api.asError(error));
      if (action.type === "unify") setAction(undefined);
    } finally {
      setBusy(false);
    }
  }

  const actionTitle = action?.type === "unlink" ? t("sessions.workbench.unlinkTitle", { name: action.member.accountName })
    : action?.type === "unify" ? t("sessions.workbench.unifyTitle", { name: action.source.accountName })
      : action?.type === "batch" ? t("sessions.workbench.batchTitle", { name: detail.title })
        : t("sessions.workbench.addMember");
  const actionDescription = action?.type === "unlink" ? t("sessions.links.unlinkDetail")
    : action?.type === "unify" ? t("sessions.workbench.unifyCount", { count: action.targets.length })
      : action?.type === "batch" ? t("sessions.workbench.batchHint", { count: behind.length })
        : t("sessions.links.addDetail");

  return (
    <div className="space-y-4">
      <SessionRelationshipGraph detail={detail} renderMember={(member, common) => (
        <div className="space-y-2 rounded-lg border bg-card p-3">
          <button type="button" onClick={() => setMemberDetails(member)} className="block max-w-full truncate text-left text-sm font-medium underline-offset-4 hover:underline">
            {memberLabel(t, member)}
          </button>
          <div className="flex flex-wrap gap-1">
            <Badge variant="outline">{statusLabel(t, member.versionStatus)}</Badge>
            {currentUids.has(member.uid) || (member.accountId && currentUids.has(member.accountId))
              ? <Badge variant="secondary">{t("sessions.workbench.current")}</Badge> : null}
          </div>
          <p className="text-xs text-muted-foreground">
            {member.recordCount === null ? t("sessions.workbench.recordsUnknown") : t("sessions.workbench.records", { count: member.recordCount })}
          </p>
          <div className="flex flex-wrap gap-1">
            {canUnify && member.canBeSource && member.linkState === "active" ? (
              <Button size="sm" variant="outline" disabled={busy} onClick={() => void previewUnify(member)}>
                {t(common ? "sessions.workbench.useCommon" : "sessions.workbench.useVersion")}
              </Button>
            ) : null}
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setAction({ type: "unlink", member })}>
              {t("sessions.links.unlink")}
            </Button>
          </div>
        </div>
      )} />
      <div className="flex flex-wrap gap-2">
        <SessionPairDialog detail={detail} sourceId={chosenSourceId} disabled={busy} onChanged={onChanged} />
        {detail.hasSafeSource && behind.length > 0 ? (
          <Button size="sm" variant="outline" disabled={busy} onClick={() => setAction({ type: "batch" })}>
            {t("sessions.workbench.batch", { count: behind.length })}
          </Button>
        ) : null}
        <Button size="sm" variant="ghost" onClick={onDelete} disabled={busy}>{t("sessions.links.delete")}</Button>
      </div>
      {detail.addTargets.length > 0 && sources.length > 0 ? (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border p-3">
          <Select value={chosenSourceId} disabled={busy} onValueChange={setSourceId}>
            <SelectTrigger aria-label={t("sessions.workbench.syncSource")}><SelectValue /></SelectTrigger>
            <SelectContent>{sources.map((member) => <SelectItem key={member.memberId} value={member.memberId}>{memberLabel(t, member)}</SelectItem>)}</SelectContent>
          </Select>
          <Select value={addTarget} onValueChange={setAddTarget} disabled={busy}>
            <SelectTrigger aria-label={t("sessions.workbench.targetAccount")}><SelectValue placeholder={t("sessions.workbench.targetAccount")} /></SelectTrigger>
            <SelectContent>{detail.addTargets.map((account) => <SelectItem key={account.id} value={account.id}>{accountLabel(t, account)}</SelectItem>)}</SelectContent>
          </Select>
          <Button size="sm" disabled={!addTarget || busy} onClick={() => setAction({ type: "add", accountId: addTarget })}>
            {t("sessions.workbench.addMember")}
          </Button>
        </div>
      ) : null}
      <Dialog open={Boolean(memberDetails)} onOpenChange={(next) => { if (!next) setMemberDetails(undefined); }}>
        <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{displayText(memberDetails?.title) ?? t("sessions.workbench.replica")}</DialogTitle>
            <DialogDescription>{memberDetails ? memberLabel(t, memberDetails) : undefined}</DialogDescription>
          </DialogHeader>
          <p className="text-xs text-muted-foreground">{memberDetails?.reason}</p>
          {memberDetails?.contentPreview?.length ? (
            <section className="space-y-2">
              <h3 className="text-sm font-medium">{t("sessions.workbench.excerpt", { count: memberDetails.contentPreview.length })}</h3>
              {memberDetails.contentPreview.map((record, index) => (
                <div key={index} className="rounded-md border p-2 text-sm">
                  <p className="text-xs font-medium text-muted-foreground">{displayText(record.speaker)}</p>
                  <p className="whitespace-pre-wrap break-words">{displayText(record.text)}</p>
                </div>
              ))}
            </section>
          ) : <p className="text-sm text-muted-foreground">{t("sessions.workbench.noExcerpt")}</p>}
        </DialogContent>
      </Dialog>
      <SessionActionDialog
        open={Boolean(action)} onClose={() => setAction(undefined)}
        title={actionTitle} description={actionDescription} busy={busy}
        destructive={action?.type === "unlink" || action?.type === "unify"}
        confirm={action?.type === "unlink" ? t("sessions.links.unlink")
          : action?.type === "add" ? t("sessions.workbench.addMember")
            : action?.type === "unify" ? t("sessions.workbench.unifyConfirm") : t("sessions.workbench.sync")}
        onConfirm={() => void confirm()}
      >
        {action?.type === "unify" ? (
          <p className="text-sm text-destructive">
            {t("sessions.workbench.unifyOverwrite", { count: action.targets.filter((target) => target.mode !== "fastForward").length })}
          </p>
        ) : null}
        {action && action.type !== "unlink" ? <p className="text-xs text-muted-foreground">{restartNotice(t, client)}</p> : null}
      </SessionActionDialog>
    </div>
  );
}
