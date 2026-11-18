import { useEffect, useState } from "react";
import { ChevronLeft, ChevronRight, Link2, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/lib/api";
import { displayText } from "@/lib/display-text";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import type { SessionGroupClient, SessionGroupDetail, SessionGroupList, SessionGroupStatus, WbVariant } from "@/lib/types";
import { clientLabel, statusLabel } from "./linked-session-ui";
import { SessionGroupActions } from "./session-group-actions";
import { NewSessionLinkDialog } from "./new-session-link-dialog";
import { SessionActionDialog } from "./linked-session-ui";

const CLIENTS: SessionGroupClient[] = ["workbuddy", "vscodeExt", "codebuddyIde"];
const PAGE_SIZE = 8;

export function LinkedSessionWorkspace({ client, onClientChange }: {
  client: SessionGroupClient;
  onClientChange: (client: SessionGroupClient) => void;
}) {
  const t = useT();
  const [ideVariant, setIdeVariant] = useState<WbVariant>("cn");
  const variantScope = client === "codebuddyIde" ? ideVariant : undefined;
  const contextKey = client + ":" + (variantScope ?? "");
  const [directory, setDirectory] = useState<{ key: string; list?: SessionGroupList; error?: string }>();
  const [revision, setRevision] = useState(0);
  const [filter, setFilter] = useState<SessionGroupStatus | "all">("all");
  const [sort, setSort] = useState("recent");
  const [page, setPage] = useState(1);
  const [openId, setOpenId] = useState<string>();
  const [detail, setDetail] = useState<{ key: string; detail?: SessionGroupDetail; error?: string }>();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string }>();
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    let active = true;
    setDirectory(undefined);
    void api.listSessionGroups(client, variantScope).then((list) => {
      if (active) setDirectory({ key: contextKey, list });
    }).catch((error) => {
      if (active) setDirectory({ key: contextKey, error: api.asError(error) });
    });
    return () => { active = false; };
  }, [client, variantScope, contextKey, revision]);

  const detailKey = contextKey + ":" + (openId ?? "");
  useEffect(() => {
    if (!openId) return;
    let active = true;
    void api.getSessionGroup(client, openId, variantScope).then((result) => {
      if (active) setDetail({ key: detailKey, detail: result });
    }).catch((error) => {
      if (active) setDetail({ key: detailKey, error: api.asError(error) });
    });
    return () => { active = false; };
  }, [client, openId, variantScope, detailKey, revision]);

  function changeContext(next: SessionGroupClient, variant?: WbVariant) {
    onClientChange(next);
    if (variant) setIdeVariant(variant);
    setOpenId(undefined);
    setCreateOpen(false);
    setDeleteTarget(undefined);
    setFilter("all");
    setPage(1);
  }

  async function deleteGroup() {
    if (!deleteTarget || deleting) return;
    setDeleting(true);
    try {
      const result = await api.deleteSessionGroup({ client, variantScope, groupId: deleteTarget.id });
      toast.success(t("sessions.workbench.deleteDone", { count: result.removed }));
      setDeleteTarget(undefined);
      setOpenId(undefined);
      setRevision((value) => value + 1);
    } catch (error) {
      toast.error(api.asError(error));
    } finally {
      setDeleting(false);
    }
  }

  const current = directory?.key === contextKey ? directory : undefined;
  const groups = current?.list?.groups ?? [];
  const states = [...new Set(groups.map((group) => group.summaryStatus))];
  const filtered = groups.filter((group) => filter === "all" || group.summaryStatus === filter)
    .sort((a, b) => (sort === "recent" ? b.latestActivityAt - a.latestActivityAt : a.latestActivityAt - b.latestActivityAt));
  const pages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const pageIndex = Math.min(page, pages);
  const start = (pageIndex - 1) * PAGE_SIZE;
  const selectedDetail = detail?.key === detailKey ? detail : undefined;
  const storeError = current?.error ?? (current?.list?.storeStatus === "unavailable"
    ? current.list.storeError ?? t("sessions.workbench.storeUnavailable") : undefined);

  return (
    <section className="mx-auto w-full max-w-[1240px] space-y-5 px-7 py-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <h1 className="text-[28px] font-[640] tracking-tight">{t("sessions.workbench.title", { client: clientLabel(t, client) })}</h1>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" onClick={() => setRevision((value) => value + 1)}>
            <RefreshCw aria-hidden />{t("sessions.workbench.refresh")}
          </Button>
          {client !== "codebuddyIde" ? (
            <Button onClick={() => setCreateOpen(true)} disabled={Boolean(storeError)}>
              <Link2 aria-hidden />{t("sessions.workbench.newLink")}
            </Button>
          ) : null}
        </div>
      </header>
      <div className="flex flex-wrap items-center gap-3">
        <Tabs value={client} onValueChange={(value) => changeContext(value as SessionGroupClient)}>
          <TabsList aria-label={t("sessions.workbench.clients")}>
            {CLIENTS.map((item) => <TabsTrigger key={item} value={item}>{clientLabel(t, item)}</TabsTrigger>)}
          </TabsList>
        </Tabs>
        {client === "codebuddyIde" ? (
          <Tabs value={ideVariant} onValueChange={(value) => changeContext(client, value as WbVariant)}>
            <TabsList aria-label={t("sessions.workbench.ideScope")}>
              <TabsTrigger value="cn">{t("sessions.links.region.cn")}</TabsTrigger>
              <TabsTrigger value="ai">{t("sessions.links.region.global")}</TabsTrigger>
            </TabsList>
          </Tabs>
        ) : null}
        {states.length > 1 || filter !== "all" ? (
          <Select value={filter} onValueChange={(value) => { setFilter(value as typeof filter); setPage(1); }}>
            <SelectTrigger aria-label={t("sessions.workbench.filter")}><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("sessions.filter.all", { count: groups.length })}</SelectItem>
              {states.map((state) => <SelectItem key={state} value={state}>{statusLabel(t, state)}</SelectItem>)}
            </SelectContent>
          </Select>
        ) : null}
        {groups.length > 1 ? (
          <Select value={sort} onValueChange={(value) => { setSort(value); setPage(1); }}>
            <SelectTrigger aria-label={t("sessions.sort.aria")}><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="recent">{t("sessions.sort.recent")}</SelectItem>
              <SelectItem value="oldest">{t("sessions.sort.oldest")}</SelectItem>
            </SelectContent>
          </Select>
        ) : null}
      </div>

      {!current ? <Skeleton className="h-32 w-full" /> : null}
      {storeError ? <Alert variant="destructive"><AlertDescription>{storeError}</AlertDescription></Alert> : null}
      {current && !storeError && filtered.length === 0 ? (
        <div className="rounded-xl border px-4 py-8 text-center">
          <h2 className="font-semibold">{t(groups.length ? "sessions.workbench.noMatches" : "sessions.workbench.noGroups")}</h2>
          <p className="mt-2 text-sm text-muted-foreground">
            {t(client === "codebuddyIde" ? "sessions.workbench.ideSeed" : "sessions.workbench.noGroupsHint")}
          </p>
          {filter !== "all" ? <Button className="mt-2" variant="outline" onClick={() => setFilter("all")}>{t("sessions.workbench.clearFilter")}</Button> : null}
        </div>
      ) : null}
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {filtered.slice(start, start + PAGE_SIZE).map((group) => (
          <button key={group.key} type="button" onClick={() => setOpenId(group.groupId)}
            className="space-y-2 rounded-xl border bg-card p-4 text-left outline-none hover:border-primary focus-visible:ring-2 focus-visible:ring-ring">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant={group.summaryStatus === "diverge" ? "destructive" : "secondary"}>{statusLabel(t, group.summaryStatus)}</Badge>
              <span className="text-xs text-muted-foreground">{t("sessions.workbench.members", { count: group.activeMemberCount })}</span>
            </div>
            <p className="truncate font-medium">{displayText(group.title) ?? group.groupId}</p>
            <p className="truncate text-xs text-muted-foreground">{displayText(group.projectLabel) ?? t("sessions.workbench.projectUnknown")}</p>
            <p className="truncate text-xs text-muted-foreground">{group.accountNames.map(displayText).filter(Boolean).join(" · ")}</p>
            {group.latestActivityAt > 0 ? <p className="text-xs text-muted-foreground">{formatDate(group.latestActivityAt)}</p> : null}
          </button>
        ))}
      </div>
      {pages > 1 ? (
        <nav className="flex items-center justify-between gap-2" aria-label={t("sessions.workbench.pagination")}>
          <p className="text-xs text-muted-foreground">{t("sessions.workbench.range", { first: start + 1, last: Math.min(start + PAGE_SIZE, filtered.length), count: filtered.length })}</p>
          <div className="flex gap-2">
            <Button variant="outline" size="icon" disabled={pageIndex === 1} onClick={() => setPage(pageIndex - 1)} aria-label={t("sessions.workbench.previous")}><ChevronLeft /></Button>
            <Button variant="outline" size="icon" disabled={pageIndex === pages} onClick={() => setPage(pageIndex + 1)} aria-label={t("sessions.workbench.next")}><ChevronRight /></Button>
          </div>
        </nav>
      ) : null}

      <Dialog open={Boolean(openId)} onOpenChange={(next) => { if (!next && !deleting) setOpenId(undefined); }}>
        <DialogContent className="max-h-[86vh] overflow-y-auto sm:max-w-4xl">
          <DialogHeader>
            <DialogTitle>{displayText(selectedDetail?.detail?.title) ?? t("sessions.workbench.detail")}</DialogTitle>
            <DialogDescription>{clientLabel(t, client)}</DialogDescription>
          </DialogHeader>
          {!selectedDetail ? <Skeleton className="h-36 w-full" /> : null}
          {selectedDetail?.error ? <Alert variant="destructive"><AlertDescription>{selectedDetail.error}</AlertDescription></Alert> : null}
          {selectedDetail?.detail ? (
            <SessionGroupActions
              key={detailKey}
              detail={selectedDetail.detail}
              onChanged={() => setRevision((value) => value + 1)}
              onRemoved={() => { setOpenId(undefined); setRevision((value) => value + 1); }}
              onDelete={() => setDeleteTarget({ id: selectedDetail.detail!.groupId, name: selectedDetail.detail!.title })}
            />
          ) : null}
        </DialogContent>
      </Dialog>
      <SessionActionDialog
        open={deleteTarget !== undefined}
        onClose={() => setDeleteTarget(undefined)}
        title={t("sessions.workbench.deleteTitle", { name: deleteTarget?.name ?? "" })}
        description={t("sessions.links.unlinkDetail")}
        confirm={t("sessions.links.delete")}
        onConfirm={() => void deleteGroup()}
        destructive busy={deleting}
      />
      {createOpen && client !== "codebuddyIde" ? (
        <NewSessionLinkDialog
          client={client} onClose={() => setCreateOpen(false)}
          onChanged={() => setRevision((value) => value + 1)}
        />
      ) : null}
    </section>
  );
}
