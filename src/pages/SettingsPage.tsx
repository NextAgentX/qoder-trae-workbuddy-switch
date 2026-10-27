import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import {
  CircleCheck,
  FolderOpen,
  Loader2,
  Play,
  RefreshCw,
  Save,
  ShieldAlert,
  Stethoscope,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { SettingsFieldRow, SettingsGroup, SettingsRow } from "@/components/settings-primitives";
// P7 T03：设置页子导航（9 项）+「备份与恢复」分区 + 通用设置分组（与侧栏弹层共用）。
import { parseSettingsSubnav, SettingsSubnav, type SettingsSubnavId } from "@/components/settings/settings-subnav";
import { BackupSection } from "@/components/settings/backup-section";
import { CodebuddyCliCard } from "@/components/settings/codebuddy-cli-card";
import { CacheSection } from "@/components/settings/cache-section";
import { RuntimeLogCard } from "@/components/settings/runtime-log-card";
import { AppSettingsGroups } from "@/components/app-settings";
// 时点表编辑器与 Trae 设置页**共用同一份实现**（校验规则属于规则，不属于版式）。
import { TimesEditor } from "@/components/schedule-times-editor";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import * as api from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { useT } from "@/lib/i18n";
import { regionDescriptor } from "@/lib/region";
import { scopeOf } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import type { TranslationKey } from "@/locales/zh";
import type {
  AutoRotateConfig,
  CheckinConfig,
  CheckinLog,
  CodeBuddyCliStatus,
  CodeBuddyCnIdeStatus,
  GatewayConfig,
  Region,
  RotateLog,
  RotateStatus,
  ScheduleConfig,
  ScheduleRunResult,
  SwitchConfig,
  TravelConfig,
} from "@/lib/types";
import { DemoAction } from "@/components/demo-action";
import { useAccountsStore } from "@/stores/accounts";
import { useGatewayStore } from "@/stores/gateway";

/**
 * 设置页的三个行原语来自 `@/components/settings-primitives`，
 * 与 Trae 设置页共用同一实现（原先两份是逐字重复，已发生过漂移）。
 * 这里重新导出只是为了让既有单测/引用路径继续可用。
 */
export { SettingsFieldRow, SettingsGroup, SettingsRow };

function formatTime(ts: number): string {
  try {
    return new Date(ts).toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return String(ts);
  }
}

function logLabel(result: string): { textKey: TranslationKey; tone: "success" | "warning" | "error" } {
  switch (result) {
    case "success":
      return { textKey: "wbSettings.checkin.resultSuccess", tone: "success" };
    case "already":
      return { textKey: "wbSettings.checkin.resultAlready", tone: "warning" };
    default:
      return { textKey: "wbSettings.checkin.resultFailed", tone: "error" };
  }
}

/** 自动签到配置 + 一键签到 + 日志。 */
function AutoCheckinCard() {
  const t = useT();
  const [cfg, setCfg] = useState<CheckinConfig | null>(null);
  const [logs, setLogs] = useState<CheckinLog[]>([]);
  const [saving, setSaving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ type: "ok" | "err"; text: string } | null>(null);

  useEffect(() => {
    void load();
  }, []);

  async function load() {
    try {
      const [c, l] = await Promise.all([api.getAutoCheckinConfig(), api.getCheckinLogs()]);
      setCfg(c);
      setLogs(l.logs);
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    }
  }

  async function save() {
    if (!cfg) return;
    setSaving(true);
    setMsg(null);
    try {
      const saved = await api.saveAutoCheckinConfig(cfg);
      setCfg(saved);
      setMsg({ type: "ok", text: t("wbSettings.common.saved") });
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  async function checkinAllNow() {
    setBusy(true);
    setMsg(null);
    try {
      const res = await api.checkinAll();
      if (res.status === "skipped" && res.reason === "already_running") {
        setMsg({ type: "err", text: t("wbSettings.checkin.alreadyRunning") });
        return;
      }
      const ok = res.accounts.filter((a) => a.result === "success").length;
      const already = res.accounts.filter((a) => a.result === "already").length;
      const err = res.accounts.filter((a) => a.result === "error").length;
      const detail = res.accounts
        .filter((a) => a.result === "error")
        .map((a) => `${a.email}${t("shared.punct.openParen")}${a.error}${t("shared.punct.closeParen")}`)
        .join(t("shared.punct.semicolon"));
      setMsg({
        type: err > 0 ? "err" : "ok",
        text: detail
          ? t("wbSettings.checkin.summaryWithDetail", { ok, already, err, detail })
          : t("wbSettings.checkin.summary", { ok, already, err }),
      });
      void load();
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  function setNum(key: keyof CheckinConfig, value: string) {
    if (!cfg) return;
    setCfg({ ...cfg, [key]: Number(value) });
  }

  return (
    <SettingsGroup
      id="settings-auto-checkin"
      title={t("wbSettings.checkin.groupTitle")}
    >
      <CardContent className="space-y-0 p-0">
        {cfg ? (
          <>
            <SettingsFieldRow
              label={t("wbSettings.checkin.enableLabel")}
              description={t("wbSettings.checkin.enableDesc")}
              htmlFor="ac-enabled"
              operational
            >
              <Switch
                id="ac-enabled"
                checked={cfg.enabled}
                onCheckedChange={(v) => setCfg({ ...cfg, enabled: v })}
              />
            </SettingsFieldRow>

            <SettingsFieldRow
              label={t("wbSettings.checkin.keepaliveLabel")}
              description={t("wbSettings.checkin.keepaliveDesc")}
              htmlFor="ac-keep"
              operational
            >
              <Input
                id="ac-keep"
                className="w-full sm:w-48"
                type="number"
                min={0}
                max={90}
                value={cfg.keepalive_days}
                onChange={(e) => setNum("keepalive_days", e.target.value)}
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              label={t("wbSettings.checkin.lazyLabel")}
              description={t("wbSettings.unit.hours")}
              htmlFor="ac-lazy"
              operational
            >
              <Input
                id="ac-lazy"
                className="w-full sm:w-48"
                type="number"
                min={1}
                max={72}
                value={cfg.lazy_refresh_hours}
                onChange={(e) => setNum("lazy_refresh_hours", e.target.value)}
              />
            </SettingsFieldRow>

            <div className="flex flex-wrap gap-2 border-b-0 border-border/60 px-4 py-3">
              <DemoAction><Button size="sm" onClick={save} disabled={saving}>
                {saving ? <Loader2 className="animate-spin" /> : <Save />}{t("wbSettings.common.saveConfig")}
              </Button></DemoAction>
              <DemoAction><Button size="sm" variant="outline" onClick={checkinAllNow} disabled={busy}>
                {busy ? <Loader2 className="animate-spin" /> : <CircleCheck />}{t("wbSettings.checkin.checkinAllBtn")}
              </Button></DemoAction>
            </div>
          </>
        ) : (
          <p className="px-4 py-3 text-sm text-muted-foreground">{t("wbSettings.common.loading")}</p>
        )}

        {msg && (
          <Alert
            variant={msg.type === "err" ? "destructive" : "default"}
            className="!w-auto mx-4 my-4 sm:mx-5"
          >
            <AlertDescription>{msg.text}</AlertDescription>
          </Alert>
        )}

        <div className="px-4 py-3">
          <p className="mb-2 text-[13px] font-medium">{t("wbSettings.checkin.logTitle")}</p>
          {logs.length === 0 ? (
            <p className="py-3 text-center text-sm text-muted-foreground">{t("wbSettings.checkin.logEmpty")}</p>
          ) : (
            <div className="max-h-64 overflow-y-auto pr-1">
              {[...logs].reverse().map((l, i) => {
                const tone = logLabel(l.result);
                return (
                  <div
                    key={i}
                    className="flex items-center justify-between border-b border-border/60 py-2 text-xs last:border-b-0"
                  >
                    <div className="min-w-0 flex-1 truncate">
                      <span className="font-medium">{l.email}</span>
                      {l.error && <span className="text-destructive">（{l.error}）</span>}
                    </div>
                    <div className="ml-2 flex shrink-0 items-center gap-2">
                      <span
                        className={
                          tone.tone === "error"
                            ? "text-destructive"
                            : tone.tone === "warning"
                              ? "text-warning"
                              : "text-success"
                        }
                      >
                        {t(tone.textKey)}
                      </span>
                      <span className="text-muted-foreground">{formatTime(l.ts)}</span>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </CardContent>
    </SettingsGroup>
  );
}

/** 自动轮换配置（CodeBuddy CLI）+ 手动检查 + 日志。 */
function AutoRotateCard() {
  const t = useT();
  const [cfg, setCfg] = useState<AutoRotateConfig | null>(null);
  const [status, setStatus] = useState<RotateStatus | null>(null);
  const [logs, setLogs] = useState<RotateLog[]>([]);
  const [saving, setSaving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ type: "ok" | "err"; text: string } | null>(null);

  useEffect(() => {
    void load();
  }, []);

  async function load() {
    try {
      const [c, s, l] = await Promise.all([
        api.getAutoRotateConfig(),
        api.getRotateStatus(),
        api.getRotateLogs(),
      ]);
      setCfg(c);
      setStatus(s);
      setLogs(l.logs);
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    }
  }

  async function save() {
    if (!cfg) return;
    setSaving(true);
    setMsg(null);
    try {
      const saved = await api.saveAutoRotateConfig(cfg);
      setCfg(saved);
      setMsg({ type: "ok", text: t("wbSettings.common.saved") });
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  async function runNow() {
    setBusy(true);
    setMsg(null);
    try {
      const res = await api.runRotate();
      setMsg({
        type: res.status === "error" ? "err" : "ok",
        text:
          res.status === "switched"
            ? t("wbSettings.rotate.switchedTo", { name: res.to ?? t("wbSettings.rotate.targetAccount") })
            : res.status === "disabled"
              ? t("wbSettings.rotate.disabledMsg")
              : (res.reason ?? t("wbSettings.rotate.checkDone", { status: res.status })),
      });
      void load();
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  function setNum(key: keyof AutoRotateConfig, value: string) {
    if (!cfg) return;
    setCfg({ ...cfg, [key]: Number(value) });
  }

  function actionLabel(action: string): { text: string; tone: "success" | "warning" | "error" } {
    switch (action) {
      case "switched":
        return { text: t("wbSettings.rotate.actionSwitched"), tone: "success" };
      case "skipped":
        return { text: t("wbSettings.rotate.actionSkipped"), tone: "warning" };
      case "disabled":
        return { text: t("wbSettings.rotate.actionDisabled"), tone: "warning" };
      case "error":
        return { text: t("wbSettings.rotate.actionError"), tone: "error" };
      default:
        return { text: action, tone: "warning" };
    }
  }

  return (
    <SettingsGroup
      id="settings-auto-rotate"
      title={t("wbSettings.rotate.groupTitle")}
    >
      <CardContent className="space-y-0 p-0">
        {status && (
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-border/60 bg-muted/25 px-4 py-3 text-xs text-muted-foreground">
            <span>
              {t("wbSettings.rotate.currentAccountLabel")}
              <b className="text-foreground">{status.activeAccountName ?? t("wbSettings.rotate.notConfigured")}</b>
            </span>
            {status.lastCheckAt && (
              <span>{t("wbSettings.rotate.lastCheck", { time: formatTime(status.lastCheckAt) })}</span>
            )}
            {status.lastSwitchAt && (
              <span>{t("wbSettings.rotate.lastSwitch", { time: formatTime(status.lastSwitchAt) })}</span>
            )}
            {!status.cliConfigured && (
              <span className="text-destructive">{t("wbSettings.rotate.cliNotInstalled")}</span>
            )}
          </div>
        )}

        {cfg ? (
          <>
            <SettingsFieldRow
              label={t("wbSettings.rotate.enableLabel")}
              description={t("wbSettings.rotate.enableDesc")}
              htmlFor="ar-enabled"
              operational
            >
              <Switch
                id="ar-enabled"
                checked={cfg.enabled}
                onCheckedChange={(v) => setCfg({ ...cfg, enabled: v })}
              />
            </SettingsFieldRow>

            <SettingsFieldRow
              label={t("wbSettings.rotate.intervalLabel")}
              description={t("wbSettings.unit.minutes")}
              htmlFor="ar-interval"
              operational
            >
              <Input
                id="ar-interval"
                className="w-full sm:w-48"
                type="number"
                min={1}
                max={1440}
                value={cfg.check_interval_minutes}
                onChange={(e) => setNum("check_interval_minutes", e.target.value)}
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              label={t("wbSettings.rotate.cooldownLabel")}
              description={t("wbSettings.unit.minutes")}
              htmlFor="ar-cooldown"
              operational
            >
              <Input
                id="ar-cooldown"
                className="w-full sm:w-48"
                type="number"
                min={1}
                max={1440}
                value={cfg.cooldown_minutes}
                onChange={(e) => setNum("cooldown_minutes", e.target.value)}
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              label={t("wbSettings.rotate.gapLabel")}
              description={t("wbSettings.unit.hours")}
              htmlFor="ar-gap"
              operational
            >
              <Input
                id="ar-gap"
                className="w-full sm:w-48"
                type="number"
                min={0}
                max={720}
                value={cfg.min_gap_hours}
                onChange={(e) => setNum("min_gap_hours", e.target.value)}
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              label={t("wbSettings.rotate.urgencyLabel")}
              description={t("wbSettings.unit.hours")}
              htmlFor="ar-urgency"
              operational
            >
              <Input
                id="ar-urgency"
                className="w-full sm:w-48"
                type="number"
                min={0}
                max={720}
                value={cfg.min_urgency_hours}
                onChange={(e) => setNum("min_urgency_hours", e.target.value)}
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              label={t("wbSettings.rotate.guardLabel")}
              description={t("wbSettings.unit.minutes")}
              htmlFor="ar-guard"
              operational
            >
              <Input
                id="ar-guard"
                className="w-full sm:w-48"
                type="number"
                min={0}
                max={1440}
                value={cfg.active_guard_minutes}
                onChange={(e) => setNum("active_guard_minutes", e.target.value)}
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              label={t("wbSettings.rotate.minCreditsLabel")}
              description={t("wbSettings.rotate.minCreditsDesc")}
              htmlFor="ar-min"
              operational
            >
              <Input
                id="ar-min"
                className="w-full sm:w-48"
                type="number"
                min={0}
                value={cfg.min_remaining_credits}
                onChange={(e) => setNum("min_remaining_credits", e.target.value)}
              />
            </SettingsFieldRow>
            <p className="border-b border-border/60 px-4 py-3 text-[13px] leading-5 text-muted-foreground">
              {t("wbSettings.rotate.timingHint")}
            </p>

            <div className="flex flex-wrap gap-2 border-b-0 border-border/60 px-4 py-3">
              <DemoAction><Button size="sm" onClick={save} disabled={saving}>
                {saving ? <Loader2 className="animate-spin" /> : <Save />}{t("wbSettings.common.saveConfig")}
              </Button></DemoAction>
              <DemoAction><Button size="sm" variant="outline" onClick={runNow} disabled={busy}>
                {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}{t("wbSettings.rotate.runNowBtn")}
              </Button></DemoAction>
            </div>
          </>
        ) : (
          <p className="px-4 py-3 text-sm text-muted-foreground">{t("wbSettings.common.loading")}</p>
        )}

        {msg && (
          <Alert
            variant={msg.type === "err" ? "destructive" : "default"}
            className="!w-auto mx-4 my-4 sm:mx-5"
          >
            <AlertDescription>{msg.text}</AlertDescription>
          </Alert>
        )}

        <div className="px-4 py-3">
          <p className="mb-2 text-[13px] font-medium">{t("wbSettings.rotate.logTitle")}</p>
          {logs.length === 0 ? (
            <p className="py-3 text-center text-sm text-muted-foreground">{t("wbSettings.rotate.logEmpty")}</p>
          ) : (
            <div className="max-h-64 overflow-y-auto pr-1">
              {logs.map((l, i) => {
                const tone = actionLabel(l.action);
                return (
                  <div
                    key={i}
                    className="flex items-center justify-between border-b border-border/60 py-2 text-xs last:border-b-0"
                  >
                    <div className="min-w-0 flex-1 truncate">
                      {l.action === "switched" && l.from && l.to && (
                        <span className="font-medium">
                          {l.from.name ?? l.from.id} → {l.to.name ?? l.to.id}
                        </span>
                      )}
                      {l.reason && <span className="text-muted-foreground">（{l.reason}）</span>}
                    </div>
                    <div className="ml-2 flex shrink-0 items-center gap-2">
                      <span
                        className={
                          tone.tone === "error"
                            ? "text-destructive"
                            : tone.tone === "success"
                              ? "text-success"
                              : "text-warning"
                        }
                      >
                        {tone.text}
                      </span>
                      <span className="text-muted-foreground">{formatTime(l.ts)}</span>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </CardContent>
    </SettingsGroup>
  );
}

/** 权限检测卡片：确认本 App 是否有权写入 WorkBuddy 认证文件。 */
function PermissionCheckCard() {
  const t = useT();
  const authFile = useAuthFile();
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<null | { ok: boolean; text: string }>(null);

  async function runCheck() {
    setChecking(true);
    setResult(null);
    try {
      const res = await api.checkAuthPermission();
      setResult({
        ok: res.ok,
        text: res.ok
          ? res.message ?? t("wbSettings.permission.okMsg")
          : `${res.error}${t("shared.punct.openParen")}${res.dir ?? ""}${t("shared.punct.closeParen")}`,
      });
    } catch (e) {
      setResult({ ok: false, text: api.asError(e) });
    } finally {
      setChecking(false);
    }
  }

  return (
    <SettingsGroup
      id="settings-permission"
      title={t("wbSettings.permission.groupTitle")}
    >
      <CardContent className="space-y-0 p-0">
        <div className="break-all border-b border-border/60 bg-muted/25 px-4 py-3 font-mono text-[11px] leading-5 text-muted-foreground">
          {authFile || t("wbSettings.permission.authFileMissing")}
        </div>
        <div className="flex flex-wrap gap-2 border-b-0 border-border/60 px-4 py-3">
          <DemoAction><Button size="sm" onClick={runCheck} disabled={checking}>
            {checking ? t("wbSettings.permission.checking") : t("wbSettings.permission.checkBtn")}
          </Button></DemoAction>
          <DemoAction><Button
            size="sm"
            variant="outline"
            onClick={() => void api.openPermissionSettings("all_files")}
          >
            {t("wbSettings.permission.openFullDisk")}
          </Button></DemoAction>
          <DemoAction><Button
            size="sm"
            variant="outline"
            onClick={() => void api.openPermissionSettings("app_management")}
          >
            {t("wbSettings.permission.openAppManagement")}
          </Button></DemoAction>
          <DemoAction><Button size="sm" variant="outline" onClick={() => void api.revealAppInFinder()}>
            {t("wbSettings.permission.revealInFinder")}
          </Button></DemoAction>
        </div>

        {result && (
          <Alert variant={result.ok ? "default" : "destructive"} className="!w-auto mx-4 my-4 sm:mx-5">
            <AlertDescription>{result.text}</AlertDescription>
          </Alert>
        )}
        {result && !result.ok && (
          <div className="mx-4 mb-4 border-l-2 border-destructive/50 bg-muted/30 px-3 py-2.5 text-xs text-muted-foreground sm:mx-5">
            <p className="mb-1 font-medium text-foreground">{t("wbSettings.permission.guideTitle")}</p>
            <ol className="list-decimal space-y-1 pl-4">
              <li>{t("wbSettings.permission.guideStep1")}</li>
              <li>{t("wbSettings.permission.guideStep2")}</li>
              <li>
                {t("wbSettings.permission.guideStep3Pre")} <b>BuddySwitch.app</b>{" "}
                {t("wbSettings.permission.guideStep3Mid")} <b>{t("wbSettings.permission.guideStep3Drag")}</b>
                {t("wbSettings.permission.guideStep3Post")}
              </li>
              <li>{t("wbSettings.permission.guideStep4")}</li>
            </ol>
          </div>
        )}
      </CardContent>
    </SettingsGroup>
  );
}

/**
 * ★ Q1「危险操作」分区（WorkBuddy 侧）。
 *
 * ## 为什么 WB 是这两项，而不是 Trae 的「重置设备标识」
 *
 * Trae 的「重置设备标识」改的是 **Trae 客户端自己的** 6 层文件（`machineid` /
 * `aha/TinyStorage` / `trae-webview` …，`trae/platform.rs::reset_device_identity_for`）。
 * WorkBuddy 客户端**没有**这套结构，且本工具在网关侧的身份是
 * `sha256("wb2a:<purpose>:<uid>")` 的**纯函数派生**（`gateway/session_headers.rs`）——
 * **不落盘、无状态，没有可「重置」的对象**。给它加一个按钮只能重置空气，
 * 那是本仓最忌的「假控件」。故此处只放**WB 真实存在**的危险面。
 */
function DangerZoneCard() {
  const t = useT();
  const [exporting, setExporting] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [clearOpen, setClearOpen] = useState(false);

  /** 导出诊断包：后端已剥离凭据并做纯文本闸门，这里只落盘。 */
  async function runExportDiagnostic() {
    if (exporting) return;
    setExporting(true);
    try {
      const { name, text } = await api.exportDiagnostic();
      const blob = new Blob([text], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = name;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
      URL.revokeObjectURL(url);
      toast.success(t("wbSettings.danger.diagSaved", { name }));
    } catch (e) {
      toast.error(t("wbSettings.danger.diagFailed"), { description: api.asError(e) });
    } finally {
      setExporting(false);
    }
  }

  /** 清空网关请求日志（不可撤销 ⇒ 必须二次确认）。 */
  async function runClearGatewayLogs() {
    if (clearing) return;
    setClearing(true);
    try {
      await api.clearGatewayLogs();
      toast.success(t("wbSettings.danger.logsCleared"));
      await useGatewayStore.getState().loadLogs();
    } catch (e) {
      toast.error(t("wbSettings.danger.logsClearFailed"), { description: api.asError(e) });
    } finally {
      setClearing(false);
      setClearOpen(false);
    }
  }

  return (
    <SettingsGroup id="settings-danger" title={t("wbSettings.danger.groupTitle")}>
      <CardContent className="space-y-0 p-0">
        <p className="border-b border-border/60 bg-muted/25 px-4 py-3 text-xs leading-5 text-muted-foreground">
          {t("wbSettings.danger.intro")}
        </p>

        <SettingsFieldRow
          label={t("wbSettings.danger.diagLabel")}
          description={t("wbSettings.danger.diagDesc")}
          operational
        >
          <Button size="sm" variant="outline" onClick={() => void runExportDiagnostic()} disabled={exporting}>
            {exporting ? <Loader2 className="animate-spin" /> : <Stethoscope />}
            {exporting ? t("wbSettings.danger.exporting") : t("wbSettings.danger.diag")}
          </Button>
        </SettingsFieldRow>

        <SettingsFieldRow
          label={t("wbSettings.danger.logsLabel")}
          description={t("wbSettings.danger.logsDesc")}
          operational
        >
          <Button size="sm" variant="destructive" onClick={() => setClearOpen(true)} disabled={clearing}>
            {clearing ? <Loader2 className="animate-spin" /> : <Trash2 />}
            {t("wbSettings.danger.logsClear")}
          </Button>
        </SettingsFieldRow>
      </CardContent>

      <Dialog open={clearOpen} onOpenChange={setClearOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <ShieldAlert className="size-4 text-destructive" />
              {t("wbSettings.danger.logsConfirmTitle")}
            </DialogTitle>
            <DialogDescription>{t("wbSettings.danger.logsConfirmDesc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setClearOpen(false)}>
              {t("shared.batch.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setClearOpen(false);
                void runClearGatewayLogs();
              }}
            >
              {t("wbSettings.danger.logsConfirmBtn")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </SettingsGroup>
  );
}

function useAuthFile(): string | undefined {
  return useAccountsStore((s) => s.status?.authFile);
}

/**
 * 账号切换：切换账号与账号列表展示的偏好。
 *
 * 两项都写 `~/.buddy-switch/switch_config.json`（**全局单份**）：
 * 它们是「我怎么用这个工具」的偏好，而随版本分家的是账号库本身。
 */
function SwitchBehaviorCard() {
  const t = useT();
  const [config, setConfig] = useState<SwitchConfig | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void api
      .getSwitchConfig()
      .then((value) => {
        if (!cancelled) setConfig(value);
      })
      .catch((e) => {
        if (!cancelled) toast.error(t("wbSettings.switch.loadFailed"), { description: api.asError(e) });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function patch(next: Partial<SwitchConfig>) {
    if (!config || saving) return;
    const previous = config;
    const merged = { ...config, ...next };
    // 乐观更新：开关必须立刻跟手，否则用户会以为没点上而连点。
    setConfig(merged);
    setSaving(true);
    try {
      setConfig(await api.saveSwitchConfig(merged));
      toast.success(t("wbSettings.switch.saved"));
    } catch (e) {
      // 失败时退回改动前的值，界面不停留在「看起来已保存」的状态。
      setConfig(previous);
      toast.error(t("wbSettings.common.saveFailed"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <SettingsGroup id="settings-switch" title={t("wbSettings.switch.groupTitle")}>
      <CardContent className="space-y-0 p-0">
        <SettingsFieldRow
          label={t("wbSettings.switch.copySessionsLabel")}
          description={t("wbSettings.switch.copySessionsDesc")}
          htmlFor="switch-copy-sessions"
          operational
        >
          <Switch
            id="switch-copy-sessions"
            checked={config?.copy_sessions_by_default ?? false}
            disabled={saving || config === null}
            onCheckedChange={(value) => void patch({ copy_sessions_by_default: value })}
            aria-label={t("wbSettings.switch.copySessionsLabel")}
          />
        </SettingsFieldRow>

        <SettingsFieldRow
          className="border-b-0"
          label={t("wbSettings.switch.pinCurrentLabel")}
          description={t("wbSettings.switch.pinCurrentDesc")}
          htmlFor="switch-pin-current"
          operational
        >
          <Switch
            id="switch-pin-current"
            checked={config?.pin_current_account ?? false}
            disabled={saving || config === null}
            onCheckedChange={(value) => void patch({ pin_current_account: value })}
            aria-label={t("wbSettings.switch.pinCurrentLabel")}
          />
        </SettingsFieldRow>
      </CardContent>
    </SettingsGroup>
  );
}

/**
 * 自动旅行：**单一真实键** `enabled`（`~/.buddy-switch/auto_travel_config.json`）。
 *
 * 与账号页工具栏的开关读写**同一份**配置（`get/save_auto_travel_config`）；原型把该行
 * 放在「签到与积分」的「积分与轮换」分组下（:1706-1708），此处对齐。
 */
function TravelSettingsCard() {
  const t = useT();
  const [cfg, setCfg] = useState<TravelConfig | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void api
      .getAutoTravelConfig()
      .then((value) => {
        if (!cancelled) setCfg(value);
      })
      .catch((e) => {
        if (!cancelled) toast.error(t("wbSettings.common.saveFailed"), { description: api.asError(e) });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function patch(enabled: boolean) {
    if (!cfg || saving) return;
    const previous = cfg;
    setCfg({ ...cfg, enabled });
    setSaving(true);
    try {
      setCfg(await api.saveAutoTravelConfig({ ...cfg, enabled }));
      toast.success(enabled ? t("wbSettings.checkin.travelOn") : t("wbSettings.checkin.travelOff"));
    } catch (e) {
      setCfg(previous);
      toast.error(t("wbSettings.common.saveFailed"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <SettingsGroup id="settings-travel" title={t("wbSettings.schedule.task.travel.label")}>
      <CardContent className="space-y-0 p-0">
        <SettingsFieldRow
          className="border-b-0"
          label={t("wbSettings.checkin.travelLabel")}
          description={t("wbSettings.checkin.travelDesc")}
          htmlFor="travel-enabled"
          operational
        >
          <Switch
            id="travel-enabled"
            checked={cfg?.enabled ?? false}
            disabled={saving || cfg === null}
            onCheckedChange={(value) => void patch(value)}
            aria-label={t("wbSettings.checkin.travelLabel")}
          />
        </SettingsFieldRow>
      </CardContent>
    </SettingsGroup>
  );
}

/**
 * 客户端检测（**只读**）：三行数据来自真实探测命令，缺数据渲染「未检测到」，绝不写死。
 *
 * - WorkBuddy Desktop：`AppStatus.installed` / `version`（当前作用域那一版）；
 * - CodeBuddy CLI：`get_codebuddy_cli_status` 的 `configured`；
 * - CodeBuddy CN IDE：`get_codebuddy_cn_ide_status` 的 `installed`。
 */
function ClientDetectCard() {
  const t = useT();
  const [region] = useScope("workbuddy");
  const status = useAccountsStore((s) => (region === "global" ? s.global.status : s.status));
  const fetchAllRegions = useAccountsStore((s) => s.fetchAllRegions);
  const [cli, setCli] = useState<CodeBuddyCliStatus | null>(null);
  const [cnIde, setCnIde] = useState<CodeBuddyCnIdeStatus | null>(null);

  useEffect(() => {
    void fetchAllRegions();
  }, [fetchAllRegions]);

  useEffect(() => {
    let cancelled = false;
    // 两个探测各自失败都不影响另一个：失败即保持 null ⇒ 该行显示「未检测到」。
    void api.getCodebuddyCliStatus().then((value) => {
      if (!cancelled) setCli(value);
    }).catch(() => {});
    void api.getCodebuddyCnIdeStatus().then((value) => {
      if (!cancelled) setCnIde(value);
    }).catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const desktopInstalled = status?.installed ?? Boolean(status?.version);
  const desktopValue = !desktopInstalled
    ? t("wbSettings.client.notDetected")
    : status?.version
      ? t("wbSettings.client.detectedVersion", { version: status.version })
      : t("wbSettings.client.detected");

  const rows: { id: string; label: string; desc: string; value: string }[] = [
    {
      id: "detect-workbuddy",
      label: t("wbSettings.client.detectWorkbuddy"),
      desc: t("wbSettings.client.detectWorkbuddyDesc"),
      value: desktopValue,
    },
    {
      id: "detect-cli",
      label: t("wbSettings.client.detectCli"),
      desc: t("wbSettings.client.detectCliDesc"),
      value: cli?.configured ? t("wbSettings.client.configured") : t("wbSettings.client.notConfigured"),
    },
    {
      id: "detect-cn-ide",
      label: t("wbSettings.client.detectCnIde"),
      desc: t("wbSettings.client.detectCnIdeDesc"),
      value: cnIde?.installed ? t("wbSettings.client.detected") : t("wbSettings.client.notDetected"),
    },
  ];

  return (
    <SettingsGroup id="settings-client-detect" title={t("wbSettings.client.detectTitle")}>
      <CardContent className="space-y-0 p-0">
        {rows.map((row, index) => (
          <SettingsFieldRow
            key={row.id}
            className={index === rows.length - 1 ? "border-b-0" : undefined}
            label={row.label}
            description={row.desc}
          >
            <span className="text-sm text-muted-foreground">{row.value}</span>
          </SettingsFieldRow>
        ))}
      </CardContent>
    </SettingsGroup>
  );
}

/**
 * P15 · 外部 Agent 会话监听：往客户端 `settings.json` 装 / 卸 hook。
 *
 * ## 为什么需要用户显式同意（默认关）
 *
 * 开启这一步会**改写别的客户端（WorkBuddy 两版）的配置文件** ——
 * 本仓对此的动作纪律是「默认关 + 用户点了才做 + 可一键还原」
 * （`app_prefs.watch_agent_sessions` 是唯一事实来源，见其默认值注释）。
 *
 * ## 为什么逐槽位显示状态，而不是一个总开关的「已开启」
 *
 * 用户很可能**只装了国内版**、或**某个客户端正开着**（开着时改不了它的配置，
 * 会被它的内存态覆盖 —— 那是 `client_settings` 的进程守卫）。
 * 把这种情况显示成一句「已开启」等于骗人：那个客户端里其实一条 hook 都没有。
 */
function AgentWatchCard() {
  const t = useT();
  const [status, setStatus] = useState<api.AgentHookStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  // 刷新用「递增 key」触发（与 `ClientDetectCard` 同款写法）：把拉取放在 effect 内，
  // 免得为了 `useCallback` 把一段一次性逻辑抬到组件顶层。
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void api
      .getAgentHookStatus()
      .then((value) => {
        if (!cancelled) setStatus(value);
      })
      .catch(() => {
        // 读不到就保持 null ⇒ 开关置灰，**不假装「关着」**（那会让人以为可以点）。
        if (!cancelled) setStatus(null);
      });
    return () => {
      cancelled = true;
    };
  }, [reloadKey]);

  async function toggle(next: boolean) {
    setBusy(true);
    setFeedback(null);
    try {
      const result = await api.setAgentHookEnabled(next);
      // ★ 部分成功是常态（只装了一个版本 / 某个客户端正在运行）⇒ `errors` 如实呈现。
      setFeedback(result.errors.length > 0 ? result.errors.join("；") : null);
    } catch (cause) {
      setFeedback(api.asError(cause));
    } finally {
      setBusy(false);
      setReloadKey((key) => key + 1);
    }
  }

  const slots = status?.slots ?? [];

  return (
    <SettingsGroup id="settings-agent-watch" title={t("wbSettings.agentWatch.title")}>
      <CardContent className="space-y-0 p-0">
        <SettingsFieldRow
          label={t("wbSettings.agentWatch.enable")}
          description={t("wbSettings.agentWatch.enableDesc")}
        >
          <Switch
            checked={status?.enabled ?? false}
            disabled={busy || status === null}
            onCheckedChange={(next) => void toggle(next)}
            aria-label={t("wbSettings.agentWatch.enable")}
          />
        </SettingsFieldRow>
        {slots.map((slot, index) => (
          <SettingsFieldRow
            key={slot.slot}
            className={index === slots.length - 1 ? "border-b-0" : undefined}
            label={t(`wbSettings.agentWatch.slot.${slot.slot}` as TranslationKey)}
            description={slot.command ?? ""}
          >
            <span className="text-sm text-muted-foreground">
              {slot.clientRunning
                ? t("wbSettings.agentWatch.state.running")
                : slot.installed
                  ? t("wbSettings.agentWatch.state.installed", { count: slot.events.length })
                  : slot.clientInstalled
                    ? t("wbSettings.agentWatch.state.off")
                    : t("wbSettings.agentWatch.state.missing")}
            </span>
          </SettingsFieldRow>
        ))}
      </CardContent>
      {feedback ? (
        <p role="alert" className="px-4 pb-3 text-[11px] leading-4 text-destructive">
          {feedback}
        </p>
      ) : null}
    </SettingsGroup>
  );
}

/** 版本与账号库：两版认证文件路径（只读展示 + 复制）、国际版 UA 版本，及打开账号库目录。 */
function VersionAccountsCard() {
  const t = useT();
  const cnStatus = useAccountsStore((s) => s.status);
  const globalStatus = useAccountsStore((s) => s.global.status);
  const fetchAllRegions = useAccountsStore((s) => s.fetchAllRegions);

  useEffect(() => {
    void fetchAllRegions();
  }, [fetchAllRegions]);

  const rows: { region: Region; authFile: string | undefined }[] = [
    { region: "cn", authFile: cnStatus?.authFile },
    { region: "global", authFile: globalStatus?.authFile },
  ];

  async function onOpen(region: Region) {
    try {
      await api.openAccountsDir(region);
    } catch (e) {
      toast.error(t("wbSettings.version.openDirFailed"), { description: api.asError(e) });
    }
  }

  return (
    <SettingsGroup id="settings-regions" title={t("wbSettings.version.groupTitle")}>
      <CardContent className="space-y-0 p-0">
        {rows.map(({ region, authFile }) => {
          const descriptor = regionDescriptor(region);
          return (
            <SettingsFieldRow
              key={region}
              label={t("wbSettings.version.authFileLabel", { version: descriptor.versionLabel })}
              description={t("wbSettings.version.authFileDesc", { env: descriptor.authEnv })}
            >
              <div className="flex w-full min-w-0 items-center gap-2 sm:w-auto">
                <code
                  className="min-w-0 flex-1 truncate rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-[11px] text-muted-foreground sm:max-w-72"
                  title={authFile || ""}
                >
                  {authFile || "—"}
                </code>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!authFile}
                  onClick={() => void copyText(authFile || "", t("wbSettings.version.copiedToast"))}
                >
                  {t("wbSettings.version.copyBtn")}
                </Button>
              </div>
            </SettingsFieldRow>
          );
        })}

        <SettingsFieldRow
          label={t("wbSettings.version.uaLabel")}
          description={t("wbSettings.version.uaDesc")}
        >
          <code className="rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-[11px] text-muted-foreground">
            WorkBuddyAI/{globalStatus?.version || "?"}
          </code>
        </SettingsFieldRow>

        <SettingsFieldRow className="border-b-0" label={t("wbSettings.version.openDirLabel")}>
          <div className="flex flex-wrap gap-2">
            <DemoAction>
              <Button variant="outline" size="sm" onClick={() => void onOpen("cn")}>
                <FolderOpen />
                {t("wbSettings.version.cn")}
              </Button>
            </DemoAction>
            <DemoAction>
              <Button variant="outline" size="sm" onClick={() => void onOpen("global")}>
                <FolderOpen />
                {t("wbSettings.version.global")}
              </Button>
            </DemoAction>
          </div>
        </SettingsFieldRow>
      </CardContent>
    </SettingsGroup>
  );
}

/** API 网关：默认监听地址 / 端口 / 日志保留条数 / 正文记录 / 独立端口模式。 */
function GatewaySettingsCard() {
  const t = useT();
  const config = useGatewayStore((s) => s.config);
  const loadConfig = useGatewayStore((s) => s.loadConfig);
  const saveConfig = useGatewayStore((s) => s.saveConfig);
  const [portDraft, setPortDraft] = useState(String(config.port));
  const [keepDraft, setKeepDraft] = useState(String(config.log_keep));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    void loadConfig();
  }, [loadConfig]);

  useEffect(() => setPortDraft(String(config.port)), [config.port]);
  useEffect(() => setKeepDraft(String(config.log_keep)), [config.log_keep]);

  async function persist(next: Partial<GatewayConfig>) {
    setSaving(true);
    try {
      await saveConfig({ ...config, ...next });
      toast.success(t("wbSettings.gateway.saved"));
    } catch (e) {
      toast.error(t("wbSettings.common.saveFailed"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  function commitPort() {
    const parsed = Number.parseInt(portDraft, 10);
    if (!Number.isFinite(parsed) || parsed < 1 || parsed > 65535) {
      toast.error(t("wbSettings.gateway.invalidPort"));
      setPortDraft(String(config.port));
      return;
    }
    if (parsed !== config.port) void persist({ port: parsed });
  }

  function commitKeep() {
    const parsed = Number.parseInt(keepDraft, 10);
    if (!Number.isFinite(parsed) || parsed < 0 || parsed > 10000) {
      toast.error(t("wbSettings.gateway.invalidKeep"));
      setKeepDraft(String(config.log_keep));
      return;
    }
    if (parsed !== config.log_keep) void persist({ log_keep: parsed });
  }

  return (
    <SettingsGroup id="settings-gateway" title={t("wbSettings.gateway.groupTitle")}>
      <CardContent className="space-y-0 p-0">
        <SettingsFieldRow
          label={t("wbSettings.gateway.enabledLabel")}
          description={t("wbSettings.gateway.enabledDesc")}
          htmlFor="gw-enabled"
          operational
        >
          <Switch
            id="gw-enabled"
            checked={config.enabled}
            disabled={saving}
            onCheckedChange={(checked) => void persist({ enabled: checked })}
            aria-label={t("wbSettings.gateway.enabledLabel")}
          />
        </SettingsFieldRow>

        <SettingsFieldRow
          label={t("wbSettings.gateway.bindLabel")}
          description={t("wbSettings.gateway.bindDesc")}
          htmlFor="gw-addr"
          operational
        >
          <Select
            value={config.bind_addr}
            onValueChange={(value) => void persist({ bind_addr: value, allow_non_loopback: value !== "127.0.0.1" })}
            disabled={saving}
          >
            <SelectTrigger
              id="gw-addr"
              size="sm"
              className="w-full sm:w-44"
              aria-label={t("wbSettings.gateway.bindLabel")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="127.0.0.1">{t("wbSettings.gateway.bindLoopback")}</SelectItem>
              <SelectItem value="0.0.0.0">{t("wbSettings.gateway.bindLan")}</SelectItem>
            </SelectContent>
          </Select>
        </SettingsFieldRow>

        <SettingsFieldRow
          label={t("wbSettings.gateway.portLabel")}
          description={t("wbSettings.gateway.portDesc")}
          htmlFor="gw-port"
          operational
        >
          <Input
            id="gw-port"
            className="w-full sm:w-44"
            inputMode="numeric"
            value={portDraft}
            onChange={(event) => setPortDraft(event.target.value)}
            onBlur={commitPort}
          />
        </SettingsFieldRow>

        <SettingsFieldRow
          label={t("wbSettings.gateway.keepLabel")}
          description={t("wbSettings.gateway.keepDesc")}
          htmlFor="gw-keep"
          operational
        >
          <Input
            id="gw-keep"
            className="w-full sm:w-44"
            inputMode="numeric"
            value={keepDraft}
            onChange={(event) => setKeepDraft(event.target.value)}
            onBlur={commitKeep}
          />
        </SettingsFieldRow>

        <SettingsFieldRow
          className="border-b-0"
          label={t("wbSettings.gateway.bodiesLabel")}
          description={t("wbSettings.gateway.bodiesDesc")}
          htmlFor="gw-bodies"
          operational
        >
          <Switch
            id="gw-bodies"
            checked={config.log_bodies}
            disabled={saving}
            onCheckedChange={(checked) => void persist({ log_bodies: checked })}
            aria-label={t("wbSettings.gateway.bodiesLabel")}
          />
        </SettingsFieldRow>
      </CardContent>
    </SettingsGroup>
  );
}

// ---------------------------------------------------------------------------
// 定时任务排程（六类任务：各自独立开关 + 独立时点表）
// ---------------------------------------------------------------------------

type ScheduleTimesField =
  | "checkin_times"
  | "travel_times"
  | "activity_times"
  | "keepalive_times"
  | "school_times"
  | "cat_times";

type ScheduleJitterField =
  | "checkin_jitter_minutes"
  | "travel_jitter_minutes"
  | "activity_jitter_minutes"
  | "keepalive_jitter_minutes"
  | "school_jitter_minutes"
  | "cat_jitter_minutes";

type ScheduleEnabledField =
  | "checkin_enabled"
  | "travel_enabled"
  | "activity_enabled"
  | "keepalive_enabled"
  | "school_enabled"
  | "cat_enabled";

interface ScheduleTaskDef {
  key: string;
  /** 任务名文案键（模块级常量只存键，渲染处再 `t(labelKey)`）。 */
  labelKey: TranslationKey;
  descKey: TranslationKey;
  timesField: ScheduleTimesField;
  jitterField: ScheduleJitterField;
  enabledField: ScheduleEnabledField;
}

/**
 * WorkBuddy 分区**六类**定时任务的展示定义（顺序与后端 `ScheduleTask::all()` 的前六类一致）。
 *
 * ⚠️ 后端 `all()` 还有**第七类** `trae_checkin`（Trae 分区的自动签到），它**刻意不在此表**：
 * 本页是 WorkBuddy 分区的设置页，把另一条产品线的开关摆进来会让人误以为它属于本产品。
 * Trae 的那份入口在 Trae 设置页，两者读写**同一份** `schedule_config.json`
 * （排程是全局单份，与 region / 产品无关）。
 */
const SCHEDULE_TASKS: ScheduleTaskDef[] = [
  { key: "checkin", labelKey: "wbSettings.schedule.task.checkin.label", descKey: "wbSettings.schedule.task.checkin.desc", timesField: "checkin_times", jitterField: "checkin_jitter_minutes", enabledField: "checkin_enabled" },
  { key: "travel", labelKey: "wbSettings.schedule.task.travel.label", descKey: "wbSettings.schedule.task.travel.desc", timesField: "travel_times", jitterField: "travel_jitter_minutes", enabledField: "travel_enabled" },
  { key: "activity", labelKey: "wbSettings.schedule.task.activity.label", descKey: "wbSettings.schedule.task.activity.desc", timesField: "activity_times", jitterField: "activity_jitter_minutes", enabledField: "activity_enabled" },
  { key: "keepalive", labelKey: "wbSettings.schedule.task.keepalive.label", descKey: "wbSettings.schedule.task.keepalive.desc", timesField: "keepalive_times", jitterField: "keepalive_jitter_minutes", enabledField: "keepalive_enabled" },
  { key: "school", labelKey: "wbSettings.schedule.task.school.label", descKey: "wbSettings.schedule.task.school.desc", timesField: "school_times", jitterField: "school_jitter_minutes", enabledField: "school_enabled" },
  { key: "cat", labelKey: "wbSettings.schedule.task.cat.label", descKey: "wbSettings.schedule.task.cat.desc", timesField: "cat_times", jitterField: "cat_jitter_minutes", enabledField: "cat_enabled" },
];

/** `HH:MM`（00:00–23:59）—— 与后端 `schedule::TimeOfDay` 的线上格式逐字一致。 */
const SCHEDULE_TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/**
 * 把「立即执行」的返回压成一句可读摘要。
 *
 * 活跃地图单独处理：它要回答的正是「官网连登到底点亮了没」，故回报上报条数与连登天数
 * （`reported` 与 `streakDays` 由后端逐账号返回）。
 */
function summarizeScheduleRun(
  task: ScheduleTaskDef,
  res: ScheduleRunResult,
): { key: TranslationKey; params?: Record<string, string | number> } {
  if (task.key !== "activity") return { key: "wbSettings.schedule.summaryExecuted" };
  let reported = 0;
  let streak: number | null = null;
  for (const region of res.regions ?? []) {
    const accounts = Array.isArray(region.accounts)
      ? (region.accounts as Record<string, unknown>[])
      : [];
    for (const account of accounts) {
      reported += Number(account.reported ?? 0);
      const days = Number(account.streakDays);
      if (Number.isFinite(days) && days > 0) {
        streak = streak === null ? days : Math.max(streak, days);
      }
    }
  }
  return streak === null
    ? { key: "wbSettings.schedule.summaryNoStreak", params: { reported } }
    : { key: "wbSettings.schedule.summaryStreak", params: { reported, streak } };
}

/** 定时任务排程配置：六类任务各自独立开关与时点表 + 活跃上报次数。 */
function ScheduleCard() {
  const t = useT();
  const [cfg, setCfg] = useState<ScheduleConfig | null>(null);
  const [saving, setSaving] = useState(false);
  /** 正在立即执行的任务 key（null 表示空闲）；一次只跑一类，避免并发打到同一批账号。 */
  const [running, setRunning] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ type: "ok" | "err"; text: string } | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api
      .getScheduleConfig()
      .then((value) => {
        if (!cancelled) setCfg(value);
      })
      .catch((e) => {
        if (!cancelled) setMsg({ type: "err", text: api.asError(e) });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  function patch(task: ScheduleTaskDef, times: string[]) {
    if (!cfg) return;
    setCfg({ ...cfg, [task.timesField]: times });
  }

  function patchJitter(task: ScheduleTaskDef, minutes: number) {
    if (!cfg) return;
    setCfg({ ...cfg, [task.jitterField]: minutes });
  }

  function toggle(task: ScheduleTaskDef, enabled: boolean) {
    if (!cfg) return;
    setCfg({ ...cfg, [task.enabledField]: enabled });
  }

  /** 前端基本范围校验：时点必须是 `HH:MM`（00:00–23:59）；已启用任务至少需一个时点。 */
  function validate(config: ScheduleConfig): string | null {
    for (const task of SCHEDULE_TASKS) {
      const times = config[task.timesField];
      const bad = times.find((time) => !SCHEDULE_TIME_PATTERN.test(time));
      if (bad !== undefined) {
        return t("wbSettings.schedule.invalidTime", { label: t(task.labelKey), time: bad });
      }
      if (config[task.enabledField] && times.length === 0) {
        return t("wbSettings.schedule.emptyTimes", { label: t(task.labelKey) });
      }
    }
    if (!Number.isInteger(config.activity_report_count) || config.activity_report_count < 1) {
      return t("wbSettings.schedule.invalidReportCount");
    }
    return null;
  }

  /** 立即执行某一类任务：不等排程到点，用于保存配置后当场自证是否生效。 */
  async function runNow(task: ScheduleTaskDef) {
    setRunning(task.key);
    setMsg(null);
    try {
      const res = await api.runScheduleTask(task.key);
      const summary = summarizeScheduleRun(task, res);
      setMsg({
        type: "ok",
        text: t("wbSettings.schedule.runDone", {
          label: t(task.labelKey),
          summary: t(summary.key, summary.params),
        }),
      });
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setRunning(null);
    }
  }

  async function save() {
    if (!cfg) return;
    const invalid = validate(cfg);
    if (invalid) {
      setMsg({ type: "err", text: invalid });
      return;
    }
    setSaving(true);
    setMsg(null);
    try {
      const saved = await api.saveScheduleConfig(cfg);
      setCfg(saved);
      setMsg({ type: "ok", text: t("wbSettings.schedule.savedMsg") });
    } catch (e) {
      // 后端 400 的 error 文案会指明具体是哪个 `*_enabled` 开关，原样展示以保留诊断价值。
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <SettingsGroup id="settings-schedule" title={t("wbSettings.schedule.groupTitle")}>
      <CardContent className="space-y-0 p-0">
        <p className="border-b border-border/60 bg-muted/25 px-4 py-3 text-xs leading-5 text-muted-foreground">
          {t("wbSettings.schedule.intro")}
        </p>

        {cfg ? (
          <>
            {SCHEDULE_TASKS.map((task) => (
              <SettingsRow
                key={task.key}
                className="flex-col items-stretch gap-3 sm:flex-row sm:items-start"
              >
                <div className="flex min-w-0 flex-1 items-start gap-3">
                  <Switch
                    id={`schedule-${task.key}-enabled`}
                    checked={cfg[task.enabledField]}
                    onCheckedChange={(v) => toggle(task, v)}
                    aria-label={t("wbSettings.schedule.enableAria", { label: t(task.labelKey) })}
                  />
                  <div className="min-w-0">
                    <Label htmlFor={`schedule-${task.key}-enabled`} className="text-[12.5px] font-[550] leading-4">
                      {t(task.labelKey)}
                    </Label>
                    <p className="mt-0.5 text-[11.5px] leading-4 text-muted-foreground">{t(task.descKey)}</p>
                    <DemoAction>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="mt-2 h-7 px-2 text-xs"
                        disabled={running !== null}
                        onClick={() => runNow(task)}
                      >
                        {running === task.key ? <Loader2 className="animate-spin" /> : <Play />}
                        {t("wbSettings.schedule.runNowBtn")}
                      </Button>
                    </DemoAction>
                  </div>
                </div>
                <TimesEditor
                  id={`schedule-${task.key}-time`}
                  times={cfg[task.timesField]}
                  jitter={cfg[task.jitterField]}
                  onChange={(times) => patch(task, times)}
                  onJitterChange={(minutes) => patchJitter(task, minutes)}
                />
              </SettingsRow>
            ))}

            <SettingsFieldRow
              label={t("wbSettings.schedule.activityCountLabel")}
              description={t("wbSettings.schedule.activityCountDesc")}
              htmlFor="schedule-activity-count"
            >
              <Input
                id="schedule-activity-count"
                className="w-full sm:w-48"
                type="number"
                min={1}
                value={cfg.activity_report_count}
                onChange={(e) => setCfg({ ...cfg, activity_report_count: Number(e.target.value) })}
              />
            </SettingsFieldRow>

            <div className="flex flex-wrap gap-2 border-b-0 border-border/60 px-4 py-3">
              <DemoAction>
                <Button size="sm" onClick={save} disabled={saving}>
                  {saving ? <Loader2 className="animate-spin" /> : <Save />}{t("wbSettings.schedule.saveBtn")}
                </Button>
              </DemoAction>
            </div>
          </>
        ) : (
          <p className="px-4 py-3 text-sm text-muted-foreground">{t("wbSettings.common.loading")}</p>
        )}

        {msg && (
          <Alert
            variant={msg.type === "err" ? "destructive" : "default"}
            className="!w-auto mx-4 my-4 sm:mx-5"
          >
            <AlertDescription>{msg.text}</AlertDescription>
          </Alert>
        )}
      </CardContent>
    </SettingsGroup>
  );
}

/**
 * 设置页：**产品级**设置（P7 起改为 **9 项子导航**，§3.9）。
 *
 * ## 为什么从「一屏长滚动」改成子导航
 *
 * 现状是 7 张卡纵向堆叠（Trae 侧 10 组），用户找一项设置要滚很久、且看不出「还有哪些类」。
 * 子导航先把**有哪几类**摆出来，再按类展示 —— 与 Trae 设置页共用同一套分区名。
 *
 * ## 9 项与内容的对应（WB 侧）
 *
 * | 子导航 | 内容 |
 * |:---|:---|
 * | 通用 | 应用级设置（外观 / 语言 / 快捷键 / 启动 / 更新，与侧栏弹层同一份实现） |
 * | 账号与登录 | 版本与账号库路径、切换行为 |
 * | 签到与积分 | 自动签到 + 日志 |
 * | 网关 | 网关配置 |
 * | 计划任务 | 六类定时任务排程 |
 * | 客户端 | CodeBuddy CLI 自动轮换 |
 * | 权限与诊断 | 认证文件写权限自检 |
 * | 日志 | WorkBuddy 无独立运行日志 ⇒ **空态 + 出口**（§3.6 四要素） |
 * | 备份与恢复 | 全局备份（P7 新增，第 9 项） |
 *
 * ⚠️ 「客户端 / 日志」是 **Trae 侧**的子项（PLAN §3.9）；WB 侧没有对应内容时**不硬塞**
 * （§3.4 规则 1：无数据不渲染），而是给一个**说明原因 + 有出口**的空态。
 */
export default function SettingsPage() {
  const t = useT();
  /* 本页是 **WorkBuddy 侧**设置（Trae 侧是 `TraeSettingsPage`）⇒ 平台固定 workbuddy，
     区域（国内版 / 国际版）仍由作用域承载 —— 与页头只读 `scope-chip` 的来源一致。 */
  const [region] = useScope("workbuddy");
  const scope = scopeOf("workbuddy", region);
  /* ★ P0-4a：`section` 的**初始值从 URL 读**（`?section=diagnostics`）——
     否则「未检测到程序位 → 去设置」只能落到设置页**首页**，到不了「权限与诊断」子项
     （原型断言 `P0-4a` 要求点了之后能跳到「权限与诊断」）。
     ⚠️ 必须**校验合法性**：非法值 fail-closed 回 "general"，不把任意串塞进 state。
     解析走 `parseSettingsSubnav`（与网关页的 `parseGatewaySubnav` 同一形状），
     Trae 设置页接的是同一个函数 —— 两平台同一件事只有一份实现。 */
  const [searchParams, setSearchParams] = useSearchParams();
  const [section, setSection] = useState<SettingsSubnavId>(() =>
    parseSettingsSubnav(searchParams.get("section")),
  );

  /**
   * 切换分区：**同时写 URL**（与网关页同构 —— 位置真值在 URL，刷新 / 前进后退行为一致）。
   * 用「保留式」写回：只覆盖 `section`，不动 `region` 之类的其他查询参数。
   */
  function goSection(id: SettingsSubnavId) {
    setSection(id);
    const next = new URLSearchParams(searchParams);
    next.set("section", id);
    setSearchParams(next, { replace: true });
  }

  return (
    <div className="mx-auto min-w-0 w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
      {/* 页头（原型 `viewSettings`，`demo.css:177-180`）：作用域副标题 + 只读 scope-chip。
          容器与其余 12 个页面同为 `.page`（1240px / 26·28·60）——
          此前这里是 `max-w-5xl`(1024px) + `px-4 py-6`，比原型窄 216px、留白也更紧。*/}
      <header className="mb-5 flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("wbSettings.page.title")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">
            {t("pageHead.settings", { scope: scopeLabel(t, scope) })}
          </p>
        </div>
        <div className="grow" />
        <ScopeChip scope={scope} />
      </header>

      {/* 两栏容器 `.split`（原型 `:369`）：方向 / 对齐 / 间距整组在 `index.css` 的
          `[data-split-layout]` 里（含 900px 转纵向），故这里不再写 `flex-col` /
          `lg:flex-row` / `gap-*` —— utilities 会盖过 base 层，导致断点失效。 */}
      <div data-split-layout className="min-w-0">
        <SettingsSubnav active={section} onSelect={goSection} />

        {/* 主区 `.split-main`：分区之间 16px（原型 `:378`）。 */}
        <div className="min-w-0 flex-1 space-y-4">
          {section === "general" && <AppSettingsGroups />}

          {section === "account" && (
            <>
              <VersionAccountsCard />
              <SwitchBehaviorCard />
            </>
          )}

          {section === "checkin" && (
            <>
              <AutoCheckinCard />
              {/* 「积分与轮换」组：轮换卡原本挂在「客户端」，与原型 :1701-1708 的分组不符
                  （原型把轮换归在「签到与积分」⇒ 客户端只留检测）。 */}
              <TravelSettingsCard />
              <AutoRotateCard />
            </>
          )}

          {section === "gateway" && <GatewaySettingsCard />}

          {/* CodeBuddy CLI 的**接入方式**（issue #109）：状态条 + 两张模式卡 + 切换向导。
              它是全局一次性配置（CLI 的凭据只有一份），与账号卡上的「切到 CLI」是两个层次。 */}
          {section === "cli" && <CodebuddyCliCard />}

          {section === "schedule" && <ScheduleCard />}

          {section === "client" && (
            <>
              <ClientDetectCard />
              {/* P15 · 会话监听开关（会写客户端配置，默认关）。 */}
              <AgentWatchCard />
            </>
          )}

          {section === "diagnostics" && (
            <>
              <PermissionCheckCard />
              <DangerZoneCard />
            </>
          )}

          {section === "logs" && (
            /* 运行日志（本工具自身）：此前这里只是个「本分区暂无运行日志」空占位，
               而宿主是 release GUI（无控制台）⇒ 用户拿不到任何可粘贴的诊断输出。
               现在直接读写 `~/.buddy-switch/logs/runtime-*.log`。 */
            <RuntimeLogCard />
          )}

          {section === "backup" && (
            <>
              <BackupSection />
              <CacheSection />
            </>
          )}
        </div>
      </div>
    </div>
  );
}
