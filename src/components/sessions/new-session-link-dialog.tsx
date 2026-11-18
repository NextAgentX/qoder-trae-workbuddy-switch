import { useEffect, useState } from "react";
import { toast } from "sonner";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import * as api from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import type { AccountMeta, SessionGroupAccountList, SessionGroupClient } from "@/lib/types";
import { accountLabel, editorRestartRequired, notifyLink, restartNotice } from "./linked-session-ui";

export function NewSessionLinkDialog({ client, onClose, onChanged }: {
  client: Exclude<SessionGroupClient, "codebuddyIde">;
  onClose: () => void;
  onChanged: () => void;
}) {
  const t = useT();
  const [accounts, setAccounts] = useState<AccountMeta[]>([]);
  const [accountsLoading, setAccountsLoading] = useState(true);
  const [accountErrors, setAccountErrors] = useState(false);
  const [source, setSource] = useState("");
  const [target, setTarget] = useState("");
  const [sourceState, setSourceState] = useState<{ id: string; list?: SessionGroupAccountList; error?: string }>();
  const [selected, setSelected] = useState(new Set<string>());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    void Promise.allSettled([
      api.getAccounts("cn").then((result) => result.accounts.map((account) => ({ ...account, region: "cn" as const }))),
      api.getAccounts("global").then((result) => result.accounts.map((account) => ({ ...account, region: "global" as const }))),
    ]).then((results) => {
      if (!active) return;
      const available = results.flatMap<AccountMeta>((result) => result.status === "fulfilled" ? result.value : []);
      setAccounts(available.filter((account) => account.id && account.uid));
      setAccountErrors(results.some((result) => result.status === "rejected"));
      setAccountsLoading(false);
    });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!source) return;
    let active = true;
    void api.listAccountSessions(source, client).then((list) => {
      if (active) setSourceState({ id: source, list });
    }).catch((error) => {
      if (active) setSourceState({ id: source, error: api.asError(error) });
    });
    return () => { active = false; };
  }, [source, client]);

  const sourceAccount = accounts.find((account) => account.id === source);
  const sourceResult = sourceState?.id === source ? sourceState : undefined;
  const targetAccounts = accounts.filter((account) => account.id !== source && (
    account.uid !== sourceAccount?.uid || (client === "workbuddy" && account.region !== sourceAccount.region)
  ));
  const workspaceGroups = new Map<string, SessionGroupAccountList["sessions"]>();
  for (const session of sourceResult?.list?.sessions ?? []) {
    const key = displayText(session.cwd) ?? session.workspaceHash ?? t("sessions.workbench.projectUnknown");
    const group = workspaceGroups.get(key) ?? [];
    group.push(session);
    workspaceGroups.set(key, group);
  }

  function selectIds(ids: string[], checked: boolean) {
    setSelected((previous) => {
      const next = new Set(previous);
      for (const id of ids) {
        if (checked) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  }

  async function submit() {
    if (!source || !target || selected.size === 0 || busy) return;
    setBusy(true);
    setError("");
    try {
      if (client === "workbuddy") {
        const report = await api.copySessionsCross(source, target, [...selected]);
        const copied = report.copied?.length ?? 0;
        const existing = report.alreadyLinked?.length ?? 0;
        if (copied) toast.success(t("sessions.workbench.linked", { count: copied }));
        if (existing) toast.info(t("sessions.workbench.alreadyLinked", { count: existing }));
        if (report.errors?.length || report.error) toast.error(t("sessions.workbench.copyFailed"), {
          description: report.error ?? report.errors?.map((error) => error.error).join("\n"),
        });
        if (report.needsRecovery) toast.error(t("sessions.workbench.needsRecovery"));
        if (copied || existing) onClose();
      } else {
        const restart = await editorRestartRequired(client, [target]);
        const report = await api.copyLinkedSessions({
          client, sourceAccountId: source, targetAccountId: target, sessionIds: [...selected], restart,
        });
        if (notifyLink(t, report)) onClose();
      }
      onChanged();
    } catch (error) {
      setError(api.asError(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
      <DialogContent className="max-h-[84vh] overflow-y-auto sm:max-w-2xl" showCloseButton={!busy}>
        <DialogHeader>
          <DialogTitle>{t("sessions.workbench.newLink")}</DialogTitle>
          <DialogDescription>{t("sessions.workbench.newLinkHint")}</DialogDescription>
        </DialogHeader>
        {accountsLoading ? <Skeleton className="h-10 w-full" /> : (
          <div className="grid gap-2 sm:grid-cols-2">
            <Select value={source} onValueChange={(value) => {
              setSource(value); setTarget(""); setSelected(new Set());
            }} disabled={busy}>
              <SelectTrigger aria-label={t("sessions.workbench.sourceAccount")}><SelectValue placeholder={t("sessions.workbench.sourceAccount")} /></SelectTrigger>
              <SelectContent>{accounts.map((account) => <SelectItem key={account.id} value={account.id}>{accountLabel(t, account)}</SelectItem>)}</SelectContent>
            </Select>
            <Select value={target} onValueChange={setTarget} disabled={!source || busy}>
              <SelectTrigger aria-label={t("sessions.workbench.targetAccount")}><SelectValue placeholder={t("sessions.workbench.targetAccount")} /></SelectTrigger>
              <SelectContent>{targetAccounts.map((account) => <SelectItem key={account.id} value={account.id}>{accountLabel(t, account)}</SelectItem>)}</SelectContent>
            </Select>
          </div>
        )}
        {accountErrors ? <Alert variant="warning"><AlertDescription>{t("sessions.workbench.accountsError")}</AlertDescription></Alert> : null}
        {!source ? <p className="text-sm text-muted-foreground">{t("sessions.workbench.chooseSource")}</p> : null}
        {source && !sourceResult ? <Skeleton className="h-20 w-full" /> : null}
        {sourceResult?.list && workspaceGroups.size === 0 ? <p className="text-sm text-muted-foreground">{t("sessions.workbench.noSourceSessions")}</p> : null}
        {sourceResult?.list?.skipped ? <p className="text-xs text-muted-foreground">{t("sessions.workbench.unreadableWorkspaces", { count: sourceResult.list.skipped })}</p> : null}
        {sourceResult?.error || error ? <Alert variant="destructive"><AlertDescription>{sourceResult?.error || error}</AlertDescription></Alert> : null}
        {workspaceGroups.size > 0 ? (
          <div className="max-h-56 divide-y overflow-y-auto rounded-md border">
            {[...workspaceGroups].map(([folder, sessions]) => (
              <section className="p-2" key={folder}>
                <label className="flex items-center gap-2 text-sm font-medium">
                  <Checkbox disabled={busy} checked={sessions.every((session) => selected.has(session.id))}
                    onCheckedChange={(value) => selectIds(sessions.map((session) => session.id), value === true)} />
                  <span className="truncate" title={folder}>{folder}</span>
                </label>
                {sessions.map((session) => (
                  <label className="ml-6 flex items-center gap-2 py-1 text-sm" key={session.id}>
                    <Checkbox disabled={busy} checked={selected.has(session.id)} onCheckedChange={(value) => selectIds([session.id], value === true)} />
                    <span className="truncate">{displayText(session.title) ?? session.id}</span>
                  </label>
                ))}
              </section>
            ))}
          </div>
        ) : null}
        <p className="text-xs text-muted-foreground">{restartNotice(t, client)}</p>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy}>{t("sessions.workbench.cancel")}</Button>
          <Button disabled={!target || selected.size === 0 || busy} onClick={() => void submit()}>
            {busy ? t("sessions.workbench.working") : t("sessions.workbench.linkCopy")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
