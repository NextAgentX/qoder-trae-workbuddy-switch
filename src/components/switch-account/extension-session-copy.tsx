import { useEffect, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { Skeleton } from "@/components/ui/skeleton";
import { asError, listExtSessions } from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import type { ExtClientId, ExtCopyItem, ExtSession } from "@/lib/types";

interface ExtensionSessionCopyProps {
  client: ExtClientId;
  sourceUid: string;
  selected: ExtCopyItem[];
  onChange: (items: ExtCopyItem[]) => void;
}

function sessionKey(workspaceHash: string, id: string) {
  return `${workspaceHash}/${id}`;
}

export function ExtensionSessionCopy({ client, sourceUid, selected, onChange }: ExtensionSessionCopyProps) {
  const t = useT();
  const [sessions, setSessions] = useState<ExtSession[]>([]);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string>();
  useEffect(() => {
    let active = true;
    setBusy(true);
    setError(undefined);
    onChange([]);
    void listExtSessions(client, sourceUid).then((result) => {
      if (active) setSessions(result.sessions);
    }).catch((reason) => {
      if (active) setError(asError(reason));
    }).finally(() => {
      if (active) setBusy(false);
    });
    return () => { active = false; };
  }, [client, sourceUid, onChange]);

  if (busy) return <Skeleton className="h-12 w-full" />;
  if (error) return <p role="alert" className="text-sm text-destructive">{error}</p>;
  if (sessions.length === 0) return null;
  const checked = new Set(selected.map((item) => sessionKey(item.workspaceHash, item.conversationId)));

  return (
    <section className="space-y-2 rounded-lg border p-3">
      <h3 className="text-sm font-medium">{t("sessions.switchLinks.copy")}</h3>
      <label className="flex items-center gap-2 text-xs text-muted-foreground">
        <Checkbox checked={checked.size === sessions.length} onCheckedChange={(value) => {
          onChange(value === true ? sessions.map((session) => ({ workspaceHash: session.workspaceHash, conversationId: session.id })) : []);
        }} />
        {t("sessions.switchLinks.selectCopy")}
      </label>
      <div className="max-h-36 divide-y overflow-y-auto rounded-md border">
        {sessions.map((session) => {
          const key = sessionKey(session.workspaceHash, session.id);
          return (
            <label key={key} className="flex items-center gap-2 p-2 text-sm">
              <Checkbox checked={checked.has(key)} onCheckedChange={(value) => {
                onChange(value === true
                  ? [...selected, { workspaceHash: session.workspaceHash, conversationId: session.id }]
                  : selected.filter((item) => sessionKey(item.workspaceHash, item.conversationId) !== key));
              }} />
              <span className="truncate">{displayText(session.title) ?? session.id}</span>
            </label>
          );
        })}
      </div>
    </section>
  );
}
