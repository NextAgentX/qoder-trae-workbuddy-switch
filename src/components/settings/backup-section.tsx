/**
 * 「备份与恢复」分区（P7 T03 · 设置页第 9 项子导航）。
 *
 * 三块卡片，顺序即使用顺序：
 *
 * 1. **导出备份** —— 逐作用域三态勾选 + 凭据开关 + 预估（§3.6）；
 * 2. **导入备份** —— 选文件 → **先预览**（安全阀，`BackupImportPreview`）→ 落盘 → 部分失败态；
 * 3. **自动备份** —— 定时备份开关（**默认开，且必须能关**，§7 #7）+ 备份记录 + 恢复。
 *
 * ## 硬约束（逐条对应设计）
 *
 * - **状态语言**（§3.6）：区块首次加载 → `<Skeleton>`；导出 / 导入 / 恢复进行中 →
 *   按钮内 `<Loader2>`（**不是**加载态）；空态 → `<EmptyState>`（含**出口**）；
 * - **无数据不渲染**（§3.4 规则 1）：某分区计数为 0 时不渲染那个数字（不留「网关 0」这种假信息）；
 * - **展示字段过 `displayText`**（issue #2）：文件名 / 作用域标签一律归一，避免对象当 React 子节点；
 * - **★ 文案红线**（§7 #2）：凭据关闭时显示「凭据不含在内，导入后需重新登录」，**绝不**写「备份完整」；
 * - **全不勾 ⇒ 导出 `disabled`**（R10），且给出原因。
 *
 * ## ⚠️ 已知取舍（与设计的偏离，理由充分）
 *
 * 设计 §3.6 写「桌面端走保存对话框（复用 `export-accounts-dialog` 的 `revealItemInDir` 模式）」。
 * 但 T01/T02 冻结的命令面里**没有**「把任意文本写到指定路径」的命令（`export_backup` 只
 * 返回 envelope JSON，`export_accounts_to_path` 只写账号库），且未安装 `plugin-fs`。
 * 因此本实现**统一走浏览器 Blob 下载**（`<a download>`；Windows WebView2 与 webui 均支持），
 * 导出后以 toast 报出文件名。**不新增后端写文件命令**（那会越出 T03 边界、并让 `check:api`
 * 的三方一致性多一处风险）。若后续补齐写文件命令，只需替换 `doExport` 的落地方式。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  Download,
  HardDriveDownload,
  Loader2,
  RefreshCw,
  RotateCcw,
} from "lucide-react";
import { toast } from "sonner";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { SettingsFieldRow, SettingsGroup } from "@/components/settings-primitives";
import { BackupImportPreview, backupScopeLabel } from "@/components/settings/backup-import-preview";
import * as api from "@/lib/api";
import {
  APP_SETTINGS_STORAGE_KEYS,
  applyAppSettingsSnapshot,
  type AppSettingsSnapshot,
} from "@/lib/app-settings-snapshot";
import {
  BACKUP_PARTS,
  CONFIG_SECTIONS_PER_SCOPE,
  estimateBackup,
  hasAnySelection,
  scopeCheckState,
  type BackupPart,
  type ScopeStat,
  type ScopeStats,
} from "@/lib/backup-selection";
import { displayText } from "@/lib/display-text";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { SCOPES, type Scope } from "@/lib/scope";
import type { AutoBackupEntry, ImportPreviewScopeRow } from "@/lib/types";
import { useBackupStore } from "@/stores/backup";
import type { TranslationKey } from "@/locales/zh";

/** 分区标签键（顺序即展示顺序）。 */
const PART_LABEL_KEY: Record<BackupPart, TranslationKey> = {
  accounts: "backup.export.part.accounts",
  config: "backup.export.part.config",
  gateway: "backup.export.part.gateway",
};

/** 备份记录类型标签键。 */
const KIND_LABEL_KEY: Record<AutoBackupEntry["kind"], TranslationKey> = {
  auto: "backup.auto.kind.auto",
  preImport: "backup.auto.kind.preImport",
  other: "backup.auto.kind.other",
};

/** 空统计（4 作用域；配置分区数是**真实常量**，不是每作用域数据）。 */
function emptyStats(): ScopeStats {
  const out = {} as ScopeStats;
  for (const scope of SCOPES) {
    out[scope] = { accounts: 0, config: CONFIG_SECTIONS_PER_SCOPE, gateway: 0 };
  }
  return out;
}

/** 某分区在某作用域下的条目数（账号 / 配置 / 网关）。 */
function partCount(part: BackupPart, stat: ScopeStat): number {
  if (part === "accounts") return stat.accounts;
  if (part === "config") return stat.config;
  return stat.gateway;
}

/** 预览 / 结果行按字段求和（容忍后端把行做成非数组 / 字段缺失）。 */
function sumField(
  rows: ImportPreviewScopeRow[] | undefined,
  key: "added" | "overwritten" | "skipped",
): number {
  if (!Array.isArray(rows)) return 0;
  return rows.reduce((sum, row) => sum + (Number(row[key]) || 0), 0);
}

/** 字节量级（`—` 表示未知；不猜）。 */
function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 浏览器 Blob 下载（webui 与 Windows WebView2 均支持）。 */
function downloadJson(filename: string, data: unknown): void {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  URL.revokeObjectURL(url);
}

/**
 * 把导入文件里的 `appSettings` **回写** localStorage（§7 #3：前端负责该段）。
 *
 * 只认本模块声明的四个键；解析失败 / 无该段 / 值命中凭据判据一律**静默跳过**
 * （不影响导入本身 —— 应用设置没搬过去不该让整次导入失败）。
 */
function applyImportedAppSettings(fileText: string): void {
  let parsed: unknown;
  try {
    parsed = JSON.parse(fileText);
  } catch {
    return;
  }
  if (!parsed || typeof parsed !== "object") return;
  const raw = (parsed as { appSettings?: unknown }).appSettings;
  if (!raw || typeof raw !== "object") return;
  const record = raw as Record<string, unknown>;
  const snapshot: AppSettingsSnapshot = {};
  for (const key of APP_SETTINGS_STORAGE_KEYS) {
    const value = record[key];
    if (typeof value === "string") snapshot[key] = value;
  }
  applyAppSettingsSnapshot(snapshot);
}

/** 「备份与恢复」分区。 */
export function BackupSection() {
  const t = useT();

  const selection = useBackupStore((s) => s.selection);
  const creds = useBackupStore((s) => s.creds);
  const conflict = useBackupStore((s) => s.conflict);
  const preview = useBackupStore((s) => s.preview);
  const result = useBackupStore((s) => s.result);
  const pending = useBackupStore((s) => s.pending);
  const autoConfig = useBackupStore((s) => s.autoConfig);
  const autoBackups = useBackupStore((s) => s.autoBackups);
  const togglePart = useBackupStore((s) => s.togglePart);
  const toggleScope = useBackupStore((s) => s.toggleScope);
  const setCreds = useBackupStore((s) => s.setCreds);
  const setConflict = useBackupStore((s) => s.setConflict);
  const setPreview = useBackupStore((s) => s.setPreview);
  const setResult = useBackupStore((s) => s.setResult);
  const setPending = useBackupStore((s) => s.setPending);
  const setAutoConfig = useBackupStore((s) => s.setAutoConfig);
  const setAutoBackups = useBackupStore((s) => s.setAutoBackups);
  const buildSelectionState = useBackupStore((s) => s.selectionState);

  const [stats, setStats] = useState<ScopeStats>(emptyStats);
  const [statsLoading, setStatsLoading] = useState(true);
  const [statsError, setStatsError] = useState("");
  const [credsConfirmOpen, setCredsConfirmOpen] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const [importError, setImportError] = useState("");
  const [fileName, setFileName] = useState("");
  const [reading, setReading] = useState(false);
  const [autoLoading, setAutoLoading] = useState(true);
  const [savingConfig, setSavingConfig] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [restoreTarget, setRestoreTarget] = useState<AutoBackupEntry | null>(null);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const fileTextRef = useRef("");

  /* ---- 各作用域条目数（驱动勾选旁的数字与预估；§3.10 第 1 条：不写死）---- */
  useEffect(() => {
    let cancelled = false;
    setStatsLoading(true);
    void Promise.all([
      api
        .getAccounts("cn")
        .then((r) => r.accounts.length)
        .catch(() => 0),
      api
        .getAccounts("global")
        .then((r) => r.accounts.length)
        .catch(() => 0),
      api
        .getTraeAccounts("cn")
        .then((r) => r.total)
        .catch(() => 0),
      api
        .getTraeAccounts("global")
        .then((r) => r.total)
        .catch(() => 0),
    ])
      .then(([wbCn, wbGlobal, traeCn, traeGlobal]) => {
        if (cancelled) return;
        setStats({
          "wb-cn": { accounts: wbCn, config: CONFIG_SECTIONS_PER_SCOPE, gateway: 0 },
          "wb-global": { accounts: wbGlobal, config: CONFIG_SECTIONS_PER_SCOPE, gateway: 0 },
          "trae-cn": { accounts: traeCn, config: CONFIG_SECTIONS_PER_SCOPE, gateway: 0 },
          "trae-global": { accounts: traeGlobal, config: CONFIG_SECTIONS_PER_SCOPE, gateway: 0 },
        });
        setStatsError("");
        setStatsLoading(false);
      })
      .catch((e) => {
        if (cancelled) return;
        setStatsError(api.asError(e));
        setStatsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  /* ---- 自动备份配置 + 记录 ---- */
  const loadAuto = useCallback(async () => {
    setAutoLoading(true);
    try {
      const [config, list] = await Promise.all([api.getAutoBackupConfig(), api.listAutoBackups()]);
      setAutoConfig(config);
      setAutoBackups(Array.isArray(list.backups) ? list.backups : []);
    } catch (e) {
      toast.error(t("backup.auto.loadFailed"), { description: api.asError(e) });
    } finally {
      setAutoLoading(false);
    }
  }, [setAutoBackups, setAutoConfig, t]);

  useEffect(() => {
    void loadAuto();
  }, [loadAuto]);

  const estimate = estimateBackup(selection, creds, stats);
  const canExport = hasAnySelection(selection);

  /* ---- 导出 ---- */
  async function doExport() {
    if (pending || !canExport) return;
    setPending(true);
    try {
      const envelope = await api.exportBackup(buildSelectionState());
      const filename = `buddy-switch-backup-${new Date().toISOString().slice(0, 10)}.json`;
      downloadJson(filename, envelope);
      toast.success(t("backup.export.saved", { path: filename }));
    } catch (e) {
      toast.error(t("backup.export.error"), { description: api.asError(e) });
    } finally {
      setPending(false);
    }
  }

  /** 凭据开关：**打开前先二次确认**（§3.6）；关闭直接生效。 */
  function onCredsChange(next: boolean) {
    if (next) {
      setCredsConfirmOpen(true);
      return;
    }
    setCreds(false);
  }

  /* ---- 导入 ---- */
  function chooseFile() {
    fileInputRef.current?.click();
  }

  function runPreview(text: string, policy = conflict) {
    setReading(true);
    setPreviewError("");
    api
      .previewImportBackup(text, policy)
      .then((res) => setPreview(res))
      .catch((e) => {
        setPreview(null);
        setPreviewError(api.asError(e));
      })
      .finally(() => setReading(false));
  }

  function onFileChange(event: React.ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    // 清空 value：允许再次选择同一文件（否则第二次不会触发 change）。
    event.target.value = "";
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const text = String(reader.result ?? "");
      fileTextRef.current = text;
      setFileName(file.name);
      setResult(null);
      setPreview(null);
      setImportError("");
      runPreview(text);
    };
    reader.readAsText(file);
  }

  function cancelPreview() {
    setPreview(null);
    setPreviewError("");
    setImportError("");
    setFileName("");
    fileTextRef.current = "";
  }

  /** 换冲突策略要**重新预览** —— 计数口径随策略变化（`keepLocal` 下不覆盖任何配置项）。 */
  function onConflictChange(policy: typeof conflict) {
    setConflict(policy);
    const text = fileTextRef.current;
    if (text) runPreview(text, policy);
  }

  async function runImport() {
    const text = fileTextRef.current;
    if (!text || pending) return;
    setPending(true);
    setImportError("");
    try {
      const summary = await api.importBackup(text, conflict, buildSelectionState());
      setResult(summary);
      setPreview(null);
      // §7 #3：localStorage 段由前端回写（导入文件里带的应用设置）。
      applyImportedAppSettings(text);
      toast.success(t("backup.import.result.done"));
      void loadAuto();
    } catch (e) {
      setImportError(api.asError(e));
    } finally {
      setPending(false);
    }
  }

  /**
   * ★ C1「重试失败项」= **真重试**：只把失败的那几个 scope 再导一次。
   *
   * 此前这个按钮的 `onClick` 是 `chooseFile` —— 打开文件选择器、整份重来（覆盖语义），
   * 与文案承诺的「只重试失败项」相反，属安全相关的**假控件**。
   *
   * 三个要点：
   * 1. 复用 `fileTextRef` 里**同一份**文件文本 —— 不需要用户再选一次文件；
   * 2. 只传 `retryableScopes`（后端逐 scope 事务 ⇒ 失败单元就是 scope），
   *    已成功的分区**连读都不读**，不会被重新覆盖；
   * 3. 未知 scope（键名非法）不在重试名单里 —— 那是来件的结构性缺陷，重试必然再失败。
   */
  async function retryFailed() {
    const text = fileTextRef.current;
    const scopes = result?.retryableScopes ?? [];
    // 三重守卫：没有源文本 / 没有可重试分区 / 正在跑 —— 都不该发请求。
    if (!text || scopes.length === 0 || pending) return;
    setPending(true);
    setImportError("");
    try {
      const summary = await api.importBackup(text, conflict, buildSelectionState(), scopes);
      setResult(summary);
      if (summary.failed === 0) {
        toast.success(t("backup.import.result.retryDone"));
      } else {
        // 仍有失败 ⇒ 如实说明，不假装「重试成功」。
        toast.error(t("backup.import.result.retryPartial", { count: summary.failed }));
      }
      void loadAuto();
    } catch (e) {
      setImportError(api.asError(e));
    } finally {
      setPending(false);
    }
  }

  function finishImport() {
    setResult(null);
    setPreview(null);
    setFileName("");
    fileTextRef.current = "";
  }

  /* ---- 自动备份 ---- */
  async function toggleAuto(next: boolean) {
    const previous = autoConfig;
    // 乐观更新：开关必须跟手，否则用户会以为没点上而连点。
    setAutoConfig({ enabled: next });
    setSavingConfig(true);
    try {
      const saved = await api.saveAutoBackupConfig({ enabled: next });
      setAutoConfig(saved);
      toast.success(t("backup.auto.saved"));
    } catch (e) {
      // 失败退回改动前：界面不停留在「看起来已保存」的状态。
      setAutoConfig(previous);
      toast.error(t("backup.auto.saveFailed"), { description: api.asError(e) });
    } finally {
      setSavingConfig(false);
    }
  }

  async function doRestore() {
    const target = restoreTarget;
    if (!target || restoring) return;
    setRestoring(true);
    try {
      const summary = await api.restoreAutoBackup(displayText(target.fileName) ?? "");
      setResult(summary);
      setPreview(null);
      toast.success(t("backup.auto.restored"));
      void loadAuto();
    } catch (e) {
      toast.error(t("backup.auto.restoreFailed"), { description: api.asError(e) });
    } finally {
      setRestoring(false);
      setRestoreTarget(null);
    }
  }

  return (
    <>
      {/* ══════════ 导出 ══════════ */}
      <SettingsGroup id="settings-backup-export" title={t("backup.export.title")}>
        <CardContent className="space-y-0 p-0">
          <p className="border-b border-border/60 bg-muted/25 px-4 py-3 text-xs leading-5 text-muted-foreground sm:px-5">
            {t("backup.export.intro")}
          </p>

          {statsError && (
            <div className="px-4 pt-3 sm:px-5">
              <Alert variant="destructive">
                <AlertTriangle />
                <AlertDescription>{statsError}</AlertDescription>
              </Alert>
            </div>
          )}

          {statsLoading ? (
            <div className="space-y-2 px-4 py-3 sm:px-5">
              <Skeleton className="h-9 w-full" />
              <Skeleton className="h-9 w-full" />
              <Skeleton className="h-9 w-full" />
              <Skeleton className="h-9 w-full" />
            </div>
          ) : (
            <div className="divide-y divide-border/50">
              {SCOPES.map((scope: Scope) => {
                const tri = scopeCheckState(selection, scope);
                const parts = selection[scope];
                const stat = stats[scope];
                const label = backupScopeLabel(scope, t);
                return (
                  <div
                    key={scope}
                    className="flex flex-col gap-2 px-4 py-2.5 sm:flex-row sm:items-center sm:justify-between sm:px-5"
                  >
                    <label className="flex min-w-0 cursor-pointer items-center gap-2.5">
                      <Checkbox
                        checked={tri === "all" ? true : tri === "mixed" ? "indeterminate" : false}
                        onCheckedChange={() => toggleScope(scope)}
                        aria-checked={tri === "all" ? true : tri === "mixed" ? "mixed" : false}
                        aria-label={t("backup.export.scopeAria", { scope: label })}
                      />
                      <span className="min-w-0 truncate text-[13px] font-medium">{label}</span>
                    </label>

                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 pl-6 sm:pl-0">
                      {BACKUP_PARTS.map((part) => {
                        const count = partCount(part, stat);
                        return (
                          <label key={part} className="flex cursor-pointer items-center gap-1.5">
                            <Checkbox
                              checked={parts[part]}
                              onCheckedChange={() => togglePart(scope, part)}
                              aria-label={t("backup.export.partAria", {
                                scope: label,
                                part: t(PART_LABEL_KEY[part]),
                              })}
                            />
                            <span className="text-xs text-muted-foreground">
                              {t(PART_LABEL_KEY[part])}
                              {/* §3.4 规则 1：无数据不渲染 —— 0 不显示，避免「网关 0」这种假信息 */}
                              {count > 0 && (
                                <span className="ml-1 tabular-nums text-muted-foreground/70">
                                  {count}
                                </span>
                              )}
                            </span>
                          </label>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          <SettingsFieldRow
            className="border-t border-border/60"
            label={t("backup.export.credsLabel")}
            description={t("backup.export.credsDesc")}
            htmlFor="backup-creds"
            operational
          >
            <Switch
              id="backup-creds"
              checked={creds}
              onCheckedChange={onCredsChange}
              aria-label={t("backup.export.credsLabel")}
            />
          </SettingsFieldRow>

          {creds ? (
            <div className="px-4 pb-3 sm:px-5">
              <Alert variant="destructive">
                <AlertTriangle />
                <AlertDescription>{t("backup.export.credsWarning")}</AlertDescription>
              </Alert>
            </div>
          ) : (
            /* ★ 文案红线：**不得**写「备份完整」——凭据不含在内 ⇒ 导入后需重新登录 */
            <p className="px-4 pb-3 text-xs leading-5 text-muted-foreground sm:px-5">
              {t("backup.credentials.notice")}
            </p>
          )}

          <div className="border-t border-border/60 px-4 py-3 text-xs tabular-nums text-muted-foreground sm:px-5">
            {t("backup.export.estimate", {
              size: Math.max(1, Math.round(estimate.bytes / 1024)),
              accounts: estimate.accounts,
              scopes: estimate.scopes,
              parts: estimate.parts,
            })}
            {" · "}
            {creds ? t("backup.export.credentialsOn") : t("backup.export.credentialsOff")}
          </div>

          <div className="flex flex-wrap items-center gap-2 border-t border-border/60 px-4 py-3 sm:px-5">
            <Button size="sm" onClick={() => void doExport()} disabled={pending || !canExport}>
              {pending ? <Loader2 className="animate-spin" /> : <Download />}
              {creds ? t("backup.export.buttonCreds") : t("backup.export.button")}
            </Button>
            {!canExport && (
              <span className="text-xs text-muted-foreground">{t("backup.export.emptyHint")}</span>
            )}
          </div>
        </CardContent>
      </SettingsGroup>

      {/* ══════════ 导入 ══════════ */}
      <SettingsGroup id="settings-backup-import" title={t("backup.import.title")}>
        <CardContent className="space-y-0 p-0">
          <input
            ref={fileInputRef}
            type="file"
            accept=".json,application/json"
            className="hidden"
            onChange={onFileChange}
          />

          {result ? (
            <div className="space-y-3 px-4 py-3 sm:px-5">
              <p className="text-[13px] font-medium">{t("backup.import.result.title")}</p>
              <p className="text-sm tabular-nums">
                {t("backup.import.result.summary", {
                  added: sumField(result.perScope, "added"),
                  overwritten: sumField(result.perScope, "overwritten"),
                  skipped: sumField(result.perScope, "skipped"),
                })}
              </p>
              {result.failed > 0 && (
                <Alert variant="destructive">
                  <AlertTriangle />
                  <AlertDescription className="flex flex-wrap items-center gap-2">
                    <span>{t("backup.import.result.failed", { count: result.failed })}</span>
                    {/* ★ C1：真重试（`retryFailed`），不再是「重新选文件、整份重来」。 */}
                    {(result.retryableScopes?.length ?? 0) > 0 && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void retryFailed()}
                        disabled={reading || pending}
                      >
                        {pending ? <Loader2 className="animate-spin" /> : null}
                        {t("backup.import.result.retry")}
                      </Button>
                    )}
                  </AlertDescription>
                </Alert>
              )}
              <p className="text-xs leading-5 text-muted-foreground">
                {t("backup.import.result.snapshot")}
              </p>
              <Button size="sm" variant="outline" onClick={finishImport}>
                {t("backup.import.result.done")}
              </Button>
            </div>
          ) : preview ? (
            <BackupImportPreview
              preview={preview}
              conflict={conflict}
              onConflictChange={onConflictChange}
              onCancel={cancelPreview}
              onRun={() => void runImport()}
              pending={pending}
            />
          ) : (
            <>
              <p className="px-4 pt-3 text-xs leading-5 text-muted-foreground sm:px-5">
                {t("backup.import.intro")}
              </p>
              <div className="flex flex-wrap items-center gap-2 px-4 py-3 sm:px-5">
                <Button size="sm" variant="outline" onClick={chooseFile} disabled={reading || pending}>
                  {reading ? <Loader2 className="animate-spin" /> : <HardDriveDownload />}
                  {reading ? t("backup.import.reading") : t("backup.import.choose")}
                </Button>
                {fileName && (
                  <span className="truncate text-xs text-muted-foreground">{fileName}</span>
                )}
              </div>
            </>
          )}

          {previewError && (
            <div className="px-4 pb-3 sm:px-5">
              <Alert variant="destructive">
                <AlertTriangle />
                <AlertDescription>
                  {t("backup.import.previewError")}：{previewError}
                </AlertDescription>
              </Alert>
            </div>
          )}
          {importError && (
            <div className="px-4 pb-3 sm:px-5">
              <Alert variant="destructive">
                <AlertTriangle />
                <AlertDescription>
                  {t("backup.import.error")}：{importError}
                </AlertDescription>
              </Alert>
            </div>
          )}
        </CardContent>
      </SettingsGroup>

      {/* ══════════ 自动备份 ══════════ */}
      <SettingsGroup id="settings-backup-auto" title={t("backup.auto.title")}>
        <CardContent className="space-y-0 p-0">
          {/* ★ 定时备份开关：默认开，**必须能关**（§7 #7）—— 删掉这个 Switch 即失去关闭入口 */}
          <SettingsFieldRow
            label={t("backup.auto.enableLabel")}
            description={t("backup.auto.enableDesc")}
            htmlFor="backup-auto-enabled"
            operational
          >
            <Switch
              id="backup-auto-enabled"
              checked={autoConfig?.enabled ?? false}
              disabled={autoLoading || savingConfig || autoConfig === null}
              onCheckedChange={(value) => void toggleAuto(value)}
              aria-label={t("backup.auto.enableLabel")}
            />
          </SettingsFieldRow>

          <div className="flex items-center justify-between border-t border-border/60 px-4 py-2.5 sm:px-5">
            <span className="text-[13px] font-medium">{t("backup.auto.listTitle")}</span>
            <Button size="sm" variant="ghost" disabled={autoLoading} onClick={() => void loadAuto()}>
              {autoLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {t("backup.auto.refresh")}
            </Button>
          </div>

          {autoLoading ? (
            <div className="space-y-2 px-4 py-3 sm:px-5">
              <Skeleton className="h-8 w-full" />
              <Skeleton className="h-8 w-full" />
            </div>
          ) : autoBackups.length === 0 ? (
            <EmptyState
              icon={HardDriveDownload}
              title={t("backup.auto.empty")}
              description={t("backup.auto.emptyBody")}
              action={
                <Button size="sm" variant="outline" onClick={() => void loadAuto()}>
                  <RefreshCw />
                  {t("backup.auto.refresh")}
                </Button>
              }
            />
          ) : (
            <div className="divide-y divide-border/50">
              {autoBackups.map((entry, index) => (
                <div
                  key={`${displayText(entry.fileName) ?? "backup"}-${index}`}
                  className="flex flex-wrap items-center justify-between gap-2 px-4 py-2.5 sm:px-5"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="min-w-0 truncate text-[13px] font-medium">
                        {displayText(entry.fileName) ?? "—"}
                      </span>
                      <Badge variant="outline">
                        {t(KIND_LABEL_KEY[entry.kind] ?? "backup.auto.kind.other")}
                      </Badge>
                    </div>
                    <div className="mt-0.5 flex flex-wrap items-center gap-x-3 text-xs text-muted-foreground">
                      <span>{formatDate(entry.modifiedAt)}</span>
                      <span>{formatBytes(entry.sizeBytes)}</span>
                    </div>
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={restoring}
                    onClick={() => setRestoreTarget(entry)}
                  >
                    <RotateCcw />
                    {t("backup.auto.restore")}
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </SettingsGroup>

      {/* 凭据二次确认（打开开关时弹） */}
      <Dialog open={credsConfirmOpen} onOpenChange={setCredsConfirmOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("backup.export.credsConfirmTitle")}</DialogTitle>
            <DialogDescription>{t("backup.export.credsConfirmBody")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCredsConfirmOpen(false)}>
              {t("backup.export.credsConfirmCancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setCreds(true);
                setCredsConfirmOpen(false);
              }}
            >
              {t("backup.export.credsConfirmOk")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 从备份恢复确认 */}
      <Dialog open={restoreTarget !== null} onOpenChange={(open) => !open && setRestoreTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("backup.auto.restoreTitle")}</DialogTitle>
            <DialogDescription>{t("backup.auto.restoreBody")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRestoreTarget(null)} disabled={restoring}>
              {t("backup.auto.restoreCancel")}
            </Button>
            <Button onClick={() => void doRestore()} disabled={restoring}>
              {restoring ? <Loader2 className="animate-spin" /> : null}
              {t("backup.auto.restoreOk")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
