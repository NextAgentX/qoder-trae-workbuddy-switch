import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Skeleton } from "@/components/ui/skeleton";
import { asError, previewSwitchSessionLinks } from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import {
  getArchiveSyncDefault,
  setArchiveSyncDefault,
  type LinkedSwitchClient,
  type LinkedSwitchPreviewItem,
  type LinkedSwitchSelection,
} from "@/lib/switch-session-links";
import type { Region } from "@/lib/types";

function buildSelection(item: LinkedSwitchPreviewItem): LinkedSwitchSelection | null {
  const mode = item.availableModes[0] ?? item.archiveAction;
  return item.previewToken && mode ? { groupId: item.groupId, previewToken: item.previewToken, mode } : null;
}

interface LinkedSessionsProps {
  accountId: string;
  client: LinkedSwitchClient;
  region: Region;
  sourceRegion?: Region;
  disabled?: boolean;
  selected: LinkedSwitchSelection[];
  onChange: (selected: LinkedSwitchSelection[]) => void;
}

export function LinkedSessions({ accountId, client, region, sourceRegion, disabled, selected, onChange }: LinkedSessionsProps) {
  const t = useT();
  const [items, setItems] = useState<LinkedSwitchPreviewItem[]>([]);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string>();
  const [reload, setReload] = useState(0);
  const [archiveDefault, setArchiveDefault] = useState(getArchiveSyncDefault);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (!expanded) return;
    let active = true;
    setBusy(true);
    setError(undefined);
    onChange([]);
    void previewSwitchSessionLinks({ accountId, client, region, sourceRegion }).then((preview) => {
      if (!active) return;
      setItems(preview.groups);
      if (preview.storeStatus === "unavailable" || preview.storeStatus === "unsupported") {
        setError(preview.storeError || t("sessions.switchLinks.unavailable"));
        return;
      }
      onChange(preview.groups.flatMap((item) => {
        const selection = buildSelection(item);
        if (!selection || selection.mode === "overwrite") return [];
        return selection.mode === "statusOnly" && getArchiveSyncDefault() ? [selection] : [];
      }));
    }).catch((reason) => {
      if (active) setError(asError(reason));
    }).finally(() => {
      if (active) setBusy(false);
    });
    return () => { active = false; };
  }, [accountId, client, region, sourceRegion, reload, onChange, t, expanded]);

  const direct = items.flatMap((item) => {
    const selection = buildSelection(item);
    return selection?.mode === "fastForward" ? [selection] : [];
  });
  const checked = new Set(selected.map((item) => item.groupId));
  return (
    <Collapsible open={expanded} onOpenChange={setExpanded} className="rounded-lg border p-3">
      <CollapsibleTrigger disabled={disabled} className="flex w-full items-center justify-between gap-2 text-left text-sm font-medium">
        <span>{t("sessions.switchLinks.title")}</span>
        <span className="text-xs font-normal text-muted-foreground">
          {expanded ? t("sessions.switchLinks.hide") : t("sessions.switchLinks.show")}
          {selected.length > 0 ? ` · ${selected.length}` : ""}
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-2 pt-3" aria-busy={busy}>
        {busy ? <Skeleton className="h-12 w-full" /> : null}
        {!busy && error ? (
          <div className="space-y-2">
            <p role="alert" className="text-sm text-destructive">{error}</p>
            <Button size="sm" variant="outline" disabled={disabled} onClick={() => setReload((value) => value + 1)}>{t("sessions.switchLinks.retry")}</Button>
          </div>
        ) : null}
        {!busy && !error && items.length === 0 ? <p className="text-xs text-muted-foreground">{t("sessions.switchLinks.empty")}</p> : null}
        {!busy && !error && selected.length > 0 ? (
          <p className="text-xs text-muted-foreground">
            {t("sessions.switchLinks.selectedHint", { n: selected.length })}
          </p>
        ) : null}
        {!busy && !error && client === "workbuddy" ? (
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            <Checkbox checked={archiveDefault} disabled={disabled} onCheckedChange={(value) => {
              setArchiveDefault(value === true);
              setArchiveSyncDefault(value === true);
              const archive = items.flatMap((item) => {
                const selection = buildSelection(item);
                return selection?.mode === "statusOnly" ? [selection] : [];
              });
              onChange([...selected.filter((item) => item.mode !== "statusOnly"), ...(value === true ? archive : [])]);
            }} />
            {t("sessions.switchLinks.archiveDefault")}
          </label>
        ) : null}
        {!busy && !error && direct.length > 0 ? (
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            <Checkbox checked={direct.every((item) => checked.has(item.groupId))} disabled={disabled} onCheckedChange={(value) => {
              onChange([...selected.filter((item) => item.mode !== "fastForward"), ...(value === true ? direct : [])]);
            }} />
            {t("sessions.switchLinks.selectSafe")}
          </label>
        ) : null}
        {!busy && !error && items.length > 0 ? (
          <div className="max-h-52 divide-y overflow-y-auto rounded-md border">
            {items.map((item) => {
              const selection = buildSelection(item);
              return (
                <label key={item.groupId} className="flex items-start gap-2 p-2 text-sm">
                  <Checkbox checked={checked.has(item.groupId)} disabled={disabled || !selection} onCheckedChange={(value) => {
                    onChange(value === true && selection ? [...selected, selection] : selected.filter((value) => value.groupId !== item.groupId));
                  }} />
                  <span className="min-w-0 flex-1">
                    <span className="flex flex-wrap items-center gap-2">
                      <span className="truncate font-medium">{displayText(item.title) ?? item.groupId}</span>
                      {selection ? <Badge variant={selection.mode === "overwrite" ? "destructive" : "outline"}>{t(`sessions.switchLinks.${selection.mode}`)}</Badge> : null}
                    </span>
                    <span className="block text-xs text-muted-foreground">{displayText(item.reason)}</span>
                  </span>
                </label>
              );
            })}
          </div>
        ) : null}
      </CollapsibleContent>
    </Collapsible>
  );
}
