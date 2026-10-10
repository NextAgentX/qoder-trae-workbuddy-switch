import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  FileJson2,
  HardDriveDownload,
  Loader2,
  RefreshCw,
  RotateCcw,
  Save,
} from "lucide-react";

import { DemoAction } from "@/components/demo-action";
import { SettingsFieldRow } from "@/components/settings-primitives";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import type {
  ClientSettingsBackupsResponse,
  ClientSettingsDiffResponse,
  ClientSettingsFingerprint,
  ClientSettingsProxyGroup,
  ClientSettingsReadResponse,
  ClientSettingsSlot,
} from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * **上游客户端 settings.json 统一托管** —— 侧栏底部应用级入口。
 *
 * ## 管的是什么（**极易与相邻功能混淆**）
 *
 * 本面板读写的是**被本工具代理的上游客户端**（WorkBuddy 两版 + Trae 四个程序位）
 * 自己的 `settings.json`，**不是**本工具自己的设置：
 *
 * - `SettingsPage` / `TraeSettingsPage` 里的 `proxy*` / `traePath*` 是本工具**自己**的
 *   代理设置（落在 `~/.buddy-switch/**`），与本面板**两个不同的文件**，只是名字撞车；
 * - `SkillManagerEntry` 是另一套应用级资产（skills 目录）。
 *
 * ## 三条纪律（与后端 core `client_settings` 一一对应）
 *
 * 1. **路径由后端唯一决定**：槽位路径来自 `list_client_settings_slots`，前端**不拼路径**。
 * 2. **写前判进程**：客户端运行中写入会被其内存态覆盖 ⇒ 后端直接拒绝（错误码
 *    `client.settings.process_running`），前端只如实呈现，不做「仍尝试写入」。
 * 3. **格式保持 + 自动备份**：键值编辑走点号路径 patch，后端只改目标键那一行并在写前
 *    备份；本面板**不提供**「整文件覆盖」入口（那会抹掉用户手改的排版）。
 *
 * ## 形态与 `SkillManagerDialog` 同构
 *
 * **受控 Dialog**（`open` / `onOpenChange`，由顶栏 ⚙ 菜单驱动）：**懒挂载**，
 * 不开就不取数（读盘 + 判进程都有成本）。演示模式下写操作（patch / restore）
 * 由 `DemoAction` 遮罩，只读命令走 fixture。
 */
export function ClientSettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const t = useT();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("clientSettings.title")}</DialogTitle>
          <DialogDescription>{t("clientSettings.description")}</DialogDescription>
        </DialogHeader>
        {/* Dialog 内容默认卸载 ⇒ 面板内的取数只在打开时发生（懒挂载）。 */}
        <ClientSettingsPanel />
      </DialogContent>
    </Dialog>
  );
}

/** 面板内的一次性提示（写入 / 恢复 / 比对的结果与错误）。 */
type Notice = { type: "ok" | "err"; text: string };

/** 槽位展示名：后端只给 i18n 键（`displayKey`），文案在词表里。 */
function slotLabel(t: ReturnType<typeof useT>, slot: { displayKey: string }): string {
  return t(slot.displayKey as TranslationKey);
}

/** 把格式指纹翻译成一句人话（缩进风格 + 换行符）。 */
function formatLabel(t: ReturnType<typeof useT>, fp: ClientSettingsFingerprint | null): string {
  if (!fp) return "—";
  const indent = fp.indent;
  let style: string;
  if (indent === null) {
    style = t("clientSettings.format.compact");
  } else if (indent === "") {
    style = t("clientSettings.format.flat");
  } else if (indent === "\t") {
    style = t("clientSettings.format.tab");
  } else if (/^ +$/.test(indent)) {
    style = t("clientSettings.format.spaces", { n: indent.length });
  } else {
    style = t("clientSettings.format.other", { indent: JSON.stringify(indent) });
  }
  const newline = fp.newline === "\r\n" ? "CRLF" : "LF";
  return `${style} · ${newline}`;
}

function ClientSettingsPanel() {
  const t = useT();
  const [slots, setSlots] = useState<ClientSettingsSlot[] | null>(null);
  const [slotsError, setSlotsError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const [read, setRead] = useState<ClientSettingsReadResponse | null>(null);
  const [readLoading, setReadLoading] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);

  const [editKey, setEditKey] = useState("");
  const [editValue, setEditValue] = useState("");
  const [saving, setSaving] = useState(false);

  const [backups, setBackups] = useState<ClientSettingsBackupsResponse | null>(null);
  const [backupsLoading, setBackupsLoading] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);

  const [diffA, setDiffA] = useState<string | null>(null);
  const [diffB, setDiffB] = useState<string | null>(null);
  const [diffResult, setDiffResult] = useState<ClientSettingsDiffResponse | null>(null);
  const [diffing, setDiffing] = useState(false);

  const [notice, setNotice] = useState<Notice | null>(null);

  // 竞态防护：切槽位 / 刷新时旧请求可能后到，用序号丢弃过期响应。
  const requestSeq = useRef(0);

  const loadSlots = useCallback(async () => {
    setSlotsError(null);
    try {
      const next = await api.listClientSettingsSlots();
      setSlots(next.slots);
      // 首次进入：优先选中第一个已安装槽位；都未安装则选第一个（仍可看路径）。
      setSelected((prev) => prev ?? next.slots.find((s) => s.installed)?.key ?? next.slots[0]?.key ?? null);
    } catch (cause) {
      setSlotsError(api.asError(cause));
    }
  }, []);

  useEffect(() => {
    void loadSlots();
  }, [loadSlots]);

  const loadBackups = useCallback(async (slot: string) => {
    setBackupsLoading(true);
    try {
      const next = await api.listClientSettingsBackups(slot);
      setBackups(next);
    } catch {
      // 备份列表读失败不阻断主视图：置空即可（面板顶部已有错误位）。
      setBackups(null);
    } finally {
      setBackupsLoading(false);
    }
  }, []);

  const loadRead = useCallback(
    async (slot: string) => {
      const seq = ++requestSeq.current;
      setReadLoading(true);
      setReadError(null);
      setNotice(null);
      setShowRaw(false);
      try {
        const next = await api.readClientSettings(slot);
        if (seq !== requestSeq.current) return;
        setRead(next);
        // 编辑框预填第一个键，省一次输入；值留空由用户填。
        setEditKey(next.keyOrder[0] ?? "");
        setEditValue("");
      } catch (cause) {
        if (seq !== requestSeq.current) return;
        setRead(null);
        setReadError(api.asError(cause));
      } finally {
        if (seq === requestSeq.current) setReadLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (!selected) return;
    void loadRead(selected);
    void loadBackups(selected);
  }, [selected, loadRead, loadBackups]);

  const currentSlot = useMemo(
    () => slots?.find((slot) => slot.key === selected) ?? null,
    [slots, selected],
  );

  /** 顶层键 → 值（`values` 已被后端 BTreeMap 排序，按 `keyOrder` 还原原顺序）。 */
  const topLevel = useMemo(() => {
    const values = read?.values;
    if (!values || typeof values !== "object" || Array.isArray(values)) return [];
    const obj = values as Record<string, unknown>;
    const keys = read && read.keyOrder.length > 0 ? read.keyOrder : Object.keys(obj);
    return keys.map((key) => ({ key, value: obj[key] }));
  }, [read]);

  async function onSave() {
    if (!selected || saving) return;
    const key = editKey.trim();
    if (!key) {
      setNotice({ type: "err", text: t("clientSettings.edit.emptyKey") });
      return;
    }
    let parsed: unknown;
    try {
      parsed = editValue.trim() === "" ? "" : JSON.parse(editValue);
    } catch (cause) {
      setNotice({
        type: "err",
        text: t("clientSettings.edit.invalidJson", { message: api.asError(cause) }),
      });
      return;
    }
    setSaving(true);
    setNotice(null);
    try {
      const result = await api.patchClientSettings(selected, { [key]: parsed });
      if (!result.changed) {
        setNotice({ type: "ok", text: t("clientSettings.edit.unchanged") });
      } else {
        const backupText = result.backup
          ? t("clientSettings.edit.backup", { path: result.backup })
          : t("clientSettings.edit.noBackup");
        setNotice({
          type: "ok",
          text: `${t("clientSettings.edit.done", { keys: result.keys.join(", ") })} · ${backupText}`,
        });
      }
      await loadRead(selected);
      await loadBackups(selected);
    } catch (cause) {
      setNotice({ type: "err", text: t("clientSettings.edit.failed", { message: api.asError(cause) }) });
    } finally {
      setSaving(false);
    }
  }

  async function onRestore(id: string) {
    if (!selected || restoringId) return;
    setRestoringId(id);
    setNotice(null);
    try {
      await api.restoreClientSettingsBackup(selected, id);
      setNotice({ type: "ok", text: t("clientSettings.backups.done", { id }) });
      await loadRead(selected);
      await loadBackups(selected);
    } catch (cause) {
      setNotice({
        type: "err",
        text: t("clientSettings.backups.failed", { message: api.asError(cause) }),
      });
    } finally {
      setRestoringId(null);
    }
  }

  async function onDiff() {
    if (!diffA || !diffB || diffing) return;
    setDiffing(true);
    setNotice(null);
    try {
      setDiffResult(await api.diffClientSettings(diffA, diffB));
    } catch (cause) {
      setDiffResult(null);
      setNotice({ type: "err", text: t("clientSettings.diff.failed", { message: api.asError(cause) }) });
    } finally {
      setDiffing(false);
    }
  }

  if (slotsError) {
    return <p className="text-sm text-destructive">{t("clientSettings.error", { message: slotsError })}</p>;
  }

  if (!slots) {
    return (
      <div className="space-y-2">
        {[0, 1, 2].map((index) => (
          <Skeleton key={index} className="h-14 w-full rounded-lg" />
        ))}
      </div>
    );
  }

  const installed = read?.installed ?? currentSlot?.installed ?? false;
  const running = read?.running ?? false;

  return (
    <div className="max-h-[72vh] space-y-4 overflow-y-auto pr-1">
      {/* webui 直连远程服务端：明确标注「操作的是服务端机器上的文件」。 */}
      {api.isWebui() && !api.isDemoMode() ? (
        <Alert variant="warning">
          <AlertDescription>{t("clientSettings.webuiNote")}</AlertDescription>
        </Alert>
      ) : null}

      {/* 槽位选择器 + 状态 */}
      <div className="flex flex-wrap items-center gap-3">
        <Select
          value={selected ?? undefined}
          onValueChange={(value) => setSelected(value)}
        >
          <SelectTrigger className="w-[240px]" aria-label={t("clientSettings.title")}>
            <SelectValue placeholder={t("clientSettings.loading")} />
          </SelectTrigger>
          <SelectContent>
            {(["workbuddy", "trae"] as const).map((product) => {
              const group = slots.filter((slot) => slot.product === product);
              if (group.length === 0) return null;
              return (
                <SelectGroup key={product}>
                  <SelectLabel>{t(`clientSettings.product.${product}` as TranslationKey)}</SelectLabel>
                  {group.map((slot) => (
                    <SelectItem key={slot.key} value={slot.key}>
                      {slotLabel(t, slot)}
                    </SelectItem>
                  ))}
                </SelectGroup>
              );
            })}
          </SelectContent>
        </Select>

        {currentSlot ? (
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={installed ? "success" : "secondary"}>
              {installed
                ? t("clientSettings.status.installed")
                : t("clientSettings.status.notInstalled")}
            </Badge>
            {installed ? (
              <Badge variant={running ? "warning" : "secondary"}>
                {running ? t("clientSettings.status.running") : t("clientSettings.status.stopped")}
              </Badge>
            ) : null}
          </div>
        ) : null}

        <div className="ml-auto flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              void loadSlots();
              if (selected) {
                void loadRead(selected);
                void loadBackups(selected);
              }
            }}
            disabled={readLoading}
          >
            {readLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("clientSettings.reload")}
          </Button>
        </div>
      </div>

      {/* 路径 + 排版指纹 */}
      {currentSlot ? (
        <Card className="gap-0 overflow-hidden rounded-xl py-0 shadow-none">
          <SettingsFieldRow label={t("clientSettings.path")}>
            <span className="max-w-full truncate font-mono text-xs text-muted-foreground" title={currentSlot.path ?? ""}>
              {currentSlot.path ?? "—"}
            </span>
          </SettingsFieldRow>
          {installed ? (
            <SettingsFieldRow className="border-b-0" label={t("clientSettings.format")}>
              <span className="text-xs text-muted-foreground">
                {formatLabel(t, read?.fingerprint ?? null)}
              </span>
            </SettingsFieldRow>
          ) : null}
        </Card>
      ) : null}

      {/* 未安装：结构化空态，不伪造成功 */}
      {!installed ? (
        <EmptyState
          icon={FileJson2}
          title={t("clientSettings.notInstalled.title")}
          description={t("clientSettings.notInstalled.description")}
        />
      ) : readError ? (
        <p className="text-sm text-destructive">
          {t("clientSettings.error", { message: readError })}
        </p>
      ) : readLoading && !read ? (
        <Skeleton className="h-40 w-full rounded-xl" />
      ) : read ? (
        <>
          {/* 当前键值（顶层） */}
          <section className="min-w-0 space-y-2">
            <div className="flex items-center justify-between px-1">
              <h3 className="text-[13px] font-medium leading-5">{t("clientSettings.raw")}</h3>
              <span className="text-xs text-muted-foreground tabular-nums">
                {t("clientSettings.keys", { n: topLevel.length })}
              </span>
            </div>
            <Card className="gap-0 overflow-hidden rounded-xl py-0 shadow-none">
              {topLevel.length === 0 ? (
                <p className="px-4 py-3 text-xs text-muted-foreground">
                  {t("clientSettings.keys", { n: 0 })}
                </p>
              ) : (
                topLevel.map((entry, index) => (
                  <SettingsFieldRow
                    key={entry.key}
                    className={cn(index === topLevel.length - 1 && "border-b-0")}
                    label={<span className="font-mono text-xs">{entry.key}</span>}
                  >
                    <button
                      type="button"
                      className="max-w-full truncate rounded px-1 font-mono text-xs text-muted-foreground hover:text-foreground"
                      title={JSON.stringify(entry.value)}
                      onClick={() => {
                        // 点一行即把键预填进编辑器（值也一并带入，便于微调）。
                        setEditKey(entry.key);
                        setEditValue(JSON.stringify(entry.value));
                      }}
                    >
                      {JSON.stringify(entry.value)}
                    </button>
                  </SettingsFieldRow>
                ))
              )}
            </Card>
            <div className="px-1">
              <Button
                size="sm"
                variant="ghost"
                className="h-7 px-2 text-xs text-muted-foreground"
                onClick={() => setShowRaw((prev) => !prev)}
              >
                {showRaw ? t("clientSettings.raw.hide") : t("clientSettings.raw.show")}
              </Button>
            </div>
            {showRaw && read.text ? (
              <pre className="max-h-56 overflow-auto rounded-xl border border-border bg-muted/40 p-3 text-xs leading-5">
                {read.text}
              </pre>
            ) : null}
          </section>

          {/* 键值编辑（格式保持 + 写前备份由后端保证） */}
          <section className="min-w-0 space-y-2">
            <div className="px-1">
              <h3 className="text-[13px] font-medium leading-5">{t("clientSettings.edit.title")}</h3>
              <p className="mt-0.5 text-xs leading-4 text-muted-foreground/75">
                {t("clientSettings.edit.hint")}
              </p>
            </div>
            <Card className="min-w-0 gap-0 overflow-hidden rounded-xl p-4 shadow-none">
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="min-w-0 space-y-1">
                  <label htmlFor="client-settings-key" className="text-xs text-muted-foreground">
                    {t("clientSettings.edit.key")}
                  </label>
                  <Input
                    id="client-settings-key"
                    value={editKey}
                    onChange={(event) => setEditKey(event.target.value)}
                    placeholder={t("clientSettings.edit.keyPlaceholder")}
                    className="font-mono text-xs"
                    spellCheck={false}
                    autoComplete="off"
                  />
                </div>
                <div className="min-w-0 space-y-1">
                  <label htmlFor="client-settings-value" className="text-xs text-muted-foreground">
                    {t("clientSettings.edit.value")}
                  </label>
                  <Textarea
                    id="client-settings-value"
                    value={editValue}
                    onChange={(event) => setEditValue(event.target.value)}
                    placeholder={t("clientSettings.edit.valuePlaceholder")}
                    className="min-h-9 font-mono text-xs"
                    spellCheck={false}
                    autoComplete="off"
                  />
                </div>
              </div>
              <div className="mt-3 flex justify-end">
                <DemoAction>
                  <Button size="sm" onClick={() => void onSave()} disabled={saving}>
                    {saving ? <Loader2 className="animate-spin" /> : <Save />}
                    {saving ? t("clientSettings.edit.saving") : t("clientSettings.edit.save")}
                  </Button>
                </DemoAction>
              </div>
            </Card>
          </section>

          {/* 备份点 + 恢复 */}
          <section className="min-w-0 space-y-2">
            <div className="flex items-center justify-between px-1">
              <h3 className="text-[13px] font-medium leading-5">{t("clientSettings.backups.title")}</h3>
              {backups ? (
                <span className="text-xs text-muted-foreground tabular-nums">
                  {t("clientSettings.backups.count", { n: backups.backups.length })}
                </span>
              ) : null}
            </div>
            {backupsLoading && !backups ? (
              <Skeleton className="h-14 w-full rounded-lg" />
            ) : !backups || backups.backups.length === 0 ? (
              <p className="px-1 text-xs text-muted-foreground">{t("clientSettings.backups.empty")}</p>
            ) : (
              <Card className="gap-0 overflow-hidden rounded-xl py-0 shadow-none">
                {backups.backups.map((entry, index) => (
                  <SettingsFieldRow
                    key={entry.id}
                    className={cn(index === backups.backups.length - 1 && "border-b-0")}
                    operational
                    label={<span className="font-mono text-xs">{entry.id}</span>}
                    description={`${entry.size} B`}
                  >
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void onRestore(entry.id)}
                      disabled={restoringId !== null}
                    >
                      {restoringId === entry.id ? <Loader2 className="animate-spin" /> : <RotateCcw />}
                      {restoringId === entry.id
                        ? t("clientSettings.backups.restoring")
                        : t("clientSettings.backups.restore")}
                    </Button>
                  </SettingsFieldRow>
                ))}
              </Card>
            )}
          </section>

          {/* 差异比对 */}
          <section className="min-w-0 space-y-2">
            <div className="px-1">
              <h3 className="text-[13px] font-medium leading-5">{t("clientSettings.diff.title")}</h3>
              <p className="mt-0.5 text-xs leading-4 text-muted-foreground/75">
                {t("clientSettings.diff.hint")}
              </p>
            </div>
            <Card className="min-w-0 gap-0 overflow-hidden rounded-xl p-4 shadow-none">
              <div className="flex flex-wrap items-end gap-3">
                <SlotPicker label={t("clientSettings.diff.a")} value={diffA} slots={slots} onChange={setDiffA} />
                <SlotPicker label={t("clientSettings.diff.b")} value={diffB} slots={slots} onChange={setDiffB} />
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => void onDiff()}
                  disabled={diffing || !diffA || !diffB || diffA === diffB}
                >
                  {diffing ? <Loader2 className="animate-spin" /> : <HardDriveDownload />}
                  {diffing ? t("clientSettings.diff.running") : t("clientSettings.diff.run")}
                </Button>
              </div>
              {diffResult ? <DiffSummary result={diffResult} /> : null}
            </Card>
          </section>
        </>
      ) : null}

      {notice ? (
        <Alert variant={notice.type === "err" ? "destructive" : "default"}>
          <AlertDescription>{notice.text}</AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}

interface SlotPickerProps {
  label: string;
  value: string | null;
  slots: ClientSettingsSlot[];
  onChange: (value: string) => void;
}

/** 比对用的槽位下拉（与顶部选择器同源，但各自独立）。 */
function SlotPicker({ label, value, slots, onChange }: SlotPickerProps) {
  const t = useT();
  return (
    <div className="min-w-0 space-y-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <Select value={value ?? undefined} onValueChange={onChange}>
        <SelectTrigger className="w-[220px]" size="sm" aria-label={label}>
          <SelectValue placeholder={t("clientSettings.diff.a")} />
        </SelectTrigger>
        <SelectContent>
          {slots.map((slot) => (
            <SelectItem key={slot.key} value={slot.key}>
              {slotLabel(t, slot)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

/** 比对结果：三类分组 + 计数 + 跨产品归并出的代理语义组。 */
function DiffSummary({ result }: { result: ClientSettingsDiffResponse }) {
  const t = useT();
  const total = result.counts.onlyA + result.counts.onlyB + result.counts.different;
  const hasProxy = result.proxyGroups.length > 0;

  if (total === 0 && !hasProxy) {
    return <p className="mt-3 text-xs text-muted-foreground">{t("clientSettings.diff.empty")}</p>;
  }

  return (
    <div className="mt-3 space-y-3">
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">
          {t("clientSettings.diff.onlyA")} {result.counts.onlyA}
        </span>
        <span className="font-medium text-foreground">
          {t("clientSettings.diff.onlyB")} {result.counts.onlyB}
        </span>
        <span className="font-medium text-foreground">
          {t("clientSettings.diff.different")} {result.counts.different}
        </span>
        <span>{t("clientSettings.diff.same", { n: result.counts.same })}</span>
      </div>

      {/* 跨产品：代理字段按语义映射表归并成一组（不落进三类）。 */}
      {result.proxyGroups.map((group) => (
        <ProxyGroupRows key={group.semantic} group={group} />
      ))}

      {result.onlyA.length > 0 ? (
        <DiffGroup title={t("clientSettings.diff.onlyA")} rows={result.onlyA.map((e) => `${e.path} = ${JSON.stringify(e.value)}`)} />
      ) : null}
      {result.onlyB.length > 0 ? (
        <DiffGroup title={t("clientSettings.diff.onlyB")} rows={result.onlyB.map((e) => `${e.path} = ${JSON.stringify(e.value)}`)} />
      ) : null}
      {result.different.length > 0 ? (
        <DiffGroup
          title={t("clientSettings.diff.different")}
          rows={result.different.map((e) => `${e.path}: ${JSON.stringify(e.a)} → ${JSON.stringify(e.b)}`)}
        />
      ) : null}
    </div>
  );
}

/** 一个代理语义组：两侧字段并列（A 侧在上、B 侧在下），并标注是否等价。 */
function ProxyGroupRows({ group }: { group: ClientSettingsProxyGroup }) {
  const t = useT();
  const label = (field: ClientSettingsProxyGroup["a"][number]) =>
    `${field.path} = ${field.present ? JSON.stringify(field.value) : t("clientSettings.diff.proxyAbsent")}`;
  const rows = [
    ...group.a.map((field) => `A · ${label(field)}`),
    ...group.b.map((field) => `B · ${label(field)}`),
  ];
  return (
    <div className="space-y-1">
      <p className="text-xs font-medium text-foreground">
        {t("clientSettings.diff.proxy")}
        <span className="ml-2 font-normal text-muted-foreground">
          {group.same ? t("clientSettings.diff.proxySame") : t("clientSettings.diff.proxyDifferent")}
        </span>
      </p>
      <ul className="space-y-0.5 rounded-lg border border-border bg-muted/40 p-2">
        {rows.map((row) => (
          <li key={row} className="break-all font-mono text-xs text-muted-foreground">
            {row}
          </li>
        ))}
      </ul>
    </div>
  );
}

function DiffGroup({ title, rows }: { title: string; rows: string[] }) {
  return (
    <div className="space-y-1">
      <p className="text-xs font-medium text-foreground">{title}</p>
      <ul className="max-h-40 space-y-0.5 overflow-auto rounded-lg border border-border bg-muted/40 p-2">
        {rows.map((row) => (
          <li key={row} className="break-all font-mono text-xs text-muted-foreground">
            {row}
          </li>
        ))}
      </ul>
    </div>
  );
}