import { useState } from "react";
import { Loader2 } from "lucide-react";

import { DemoAction } from "@/components/demo-action";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import type { SessionGroupDetail, SessionGroupPairPreview, SessionSyncMode } from "@/lib/types";
import { editorRestartRequired, matchesPreview, memberLabel, modeLabel, notifySync, restartNotice } from "./linked-session-ui";

export function SessionPairDialog({
  detail, sourceId: initialSource, disabled, onChanged,
}: {
  detail: SessionGroupDetail;
  sourceId: string;
  disabled: boolean;
  onChanged: () => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [sourceId, setSourceId] = useState(initialSource);
  const [targetId, setTargetId] = useState("");
  const [preview, setPreview] = useState<SessionGroupPairPreview | null>(null);
  const [mode, setMode] = useState<SessionSyncMode | "">("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const active = detail.members.filter((member) => member.linkState === "active");
  const sources = active.filter((member) => member.canBeSource);
  const targets = active.filter((member) => member.memberId !== sourceId);
  const target = targets.find((member) => member.memberId === targetId);
  const modes = preview?.previewToken
    ? [...new Set([...preview.availableModes, ...(preview.archiveAction ? [preview.archiveAction] : [])])]
    : [];
  const variantScope = detail.variantScope ?? undefined;
  const overwrite = mode === "overwrite" || mode === "unifyOverwrite";

  function resetPreview() {
    setPreview(null);
    setMode("");
    setError("");
  }

  function changeOpen(next: boolean) {
    if (busy || loading) return;
    if (next) {
      setSourceId(initialSource);
      setTargetId(active.find((member) => member.memberId !== initialSource)?.memberId ?? "");
      resetPreview();
    }
    setOpen(next);
  }

  async function checkPair() {
    if (!sourceId || !target || loading || busy) return;
    resetPreview();
    setLoading(true);
    try {
      const result = await api.previewSessionGroupPair({
        client: detail.client, variantScope, groupId: detail.groupId, sourceMemberId: sourceId, targetMemberId: targetId,
      });
      if (!matchesPreview(result, detail, sourceId, targetId)) {
        setError(t("sessions.workbench.previewMismatch"));
        return;
      }
      setPreview(result);
      if (result.previewToken) {
        const available = [...new Set([...result.availableModes, ...(result.archiveAction ? [result.archiveAction] : [])])];
        if (available.length === 1 && available[0] === "fastForward") setMode("fastForward");
      }
    } catch (error) {
      setError(api.asError(error));
    } finally {
      setLoading(false);
    }
  }

  async function syncPair() {
    if (!preview?.previewToken || !target || !mode || !modes.includes(mode) || busy || loading
      || !matchesPreview(preview, detail, sourceId, targetId)) return;
    setBusy(true);
    setError("");
    try {
      const restart = await editorRestartRequired(detail.client, [target.accountId]);
      const result = await api.syncSessionGroupPair({
        client: detail.client, variantScope, groupId: detail.groupId,
        sourceMemberId: sourceId, targetMemberId: targetId, previewToken: preview.previewToken, mode,
        ...(restart ? { restart } : {}),
      });
      notifySync(t, result);
      setOpen(false);
      onChanged();
    } catch (error) {
      setPreview(null);
      setMode("");
      setError(api.asError(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm" disabled={disabled || sources.length === 0 || active.length < 2}>
          {t("sessions.workbench.pair")}
        </Button>
      </DialogTrigger>
      <DialogContent showCloseButton={!busy && !loading}>
        <DialogHeader>
          <DialogTitle>{t("sessions.workbench.pair")}</DialogTitle>
          <DialogDescription>{t("sessions.workbench.pairHint")}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="min-w-0 space-y-1">
            <p className="text-xs font-medium">{t("sessions.workbench.syncSource")}</p>
            <Select disabled={busy || loading} value={sourceId} onValueChange={(id) => {
              setSourceId(id);
              if (id === targetId) setTargetId("");
              resetPreview();
            }}>
              <SelectTrigger className="w-full min-w-0" aria-label={t("sessions.workbench.syncSource")}><SelectValue /></SelectTrigger>
              <SelectContent>
                {sources.map((member) => <SelectItem key={member.memberId} value={member.memberId}>{memberLabel(t, member)}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
          <div className="min-w-0 space-y-1">
            <p className="text-xs font-medium">{t("sessions.workbench.syncTarget")}</p>
            <Select disabled={busy || loading} value={targetId} onValueChange={(id) => { setTargetId(id); resetPreview(); }}>
              <SelectTrigger className="w-full min-w-0" aria-label={t("sessions.workbench.syncTarget")}>
                <SelectValue placeholder={t("sessions.workbench.syncTarget")} />
              </SelectTrigger>
              <SelectContent>
                {targets.map((member) => <SelectItem key={member.memberId} value={member.memberId}>{memberLabel(t, member)}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
        </div>
        <Button className="justify-self-start" size="sm" variant="outline" disabled={!sourceId || !target || loading || busy} onClick={() => void checkPair()}>
          {loading && <Loader2 className="animate-spin" />}
          {t(loading ? "sessions.workbench.previewing" : "sessions.workbench.preview")}
        </Button>
        {error && <p role="alert" className="break-words text-sm text-destructive">{error}</p>}
        {preview && (
          <div className="space-y-2 rounded-lg border p-3 text-sm" aria-live="polite">
            <Badge variant="outline">{t(`sessions.workbench.verdict.${preview.verdict}`)}</Badge>
            {preview.reason && <p className="break-words text-muted-foreground">{preview.reason}</p>}
            {preview.recordCount && <p className="text-xs text-muted-foreground">
              {t("sessions.workbench.previewCounts", { source: preview.recordCount.source, target: preview.recordCount.target })}
              {preview.recordCount.baseline != null && ` · ${t("sessions.workbench.previewBaseline", { count: preview.recordCount.baseline })}`}
            </p>}
            {preview.extraTargetCount > 0 && <p className="text-xs text-muted-foreground">{t("sessions.workbench.previewExtra", { count: preview.extraTargetCount })}</p>}
            {modes.length > 0 ? (
              <Select value={mode} disabled={busy} onValueChange={(next) => {
                const offered = modes.find((item) => item === next);
                if (offered) setMode(offered);
              }}>
                <SelectTrigger className="w-full" aria-label={t("sessions.workbench.sync")}>
                  <SelectValue placeholder={t("sessions.workbench.sync")} />
                </SelectTrigger>
                <SelectContent>
                  {modes.map((item) => <SelectItem key={item} value={item}>{modeLabel(t, item)}</SelectItem>)}
                </SelectContent>
              </Select>
            ) : <p className="text-xs text-muted-foreground">{t("sessions.workbench.previewUnavailable")}</p>}
            {overwrite && <Alert variant="destructive"><AlertDescription>{t("sessions.workbench.overwriteHint")}</AlertDescription></Alert>}
            {mode === "statusOnly" && <p>{t("sessions.workbench.archiveHint")}</p>}
            {mode && <p className="text-xs text-muted-foreground">{restartNotice(t, detail.client)}</p>}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" disabled={busy || loading} onClick={() => changeOpen(false)}>{t("sessions.workbench.cancel")}</Button>
          <DemoAction>
            <Button variant={overwrite ? "destructive" : "default"} disabled={!mode || busy || loading} onClick={() => void syncPair()}>
              {busy && <Loader2 className="animate-spin" />}
              {busy ? t("sessions.workbench.working") : mode ? modeLabel(t, mode) : t("sessions.workbench.sync")}
            </Button>
          </DemoAction>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
