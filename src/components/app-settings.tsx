import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { useLocation } from "react-router-dom";
import { ArrowUpCircle, ExternalLink, Loader2, RefreshCw, Save } from "lucide-react";
import { toast } from "sonner";

import { DemoAction } from "@/components/demo-action";
import { SettingsFieldRow, SettingsGroup } from "@/components/settings-primitives";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { UpdateInstallDialog } from "@/components/update-install-dialog";
import { ScopeMark, scopeLabel } from "@/components/scope-chip";
import * as api from "@/lib/api";
import { isLocale, setLocale, useLocale, useT } from "@/lib/i18n";
import { normalizeReleaseNotes } from "@/lib/release-notes";
import { SCOPES, scopeOf, scopeParts, type Scope } from "@/lib/scope";
import { planScopeShortcuts, SCOPE_SHORTCUT_LIMIT } from "@/lib/scope-shortcuts";
import { useVisibleScopes } from "@/lib/scope-visibility";
import { useShortcutsEnabled } from "@/lib/shortcut-preference";
import { getThemePreference, setThemePreference, type ThemePreference } from "@/lib/theme";
import { useAutoCheckUpdate } from "@/lib/update-preference";
import type { GithubConfig, UpdateInfo } from "@/lib/types";
import { GITHUB_RELEASE_URL, GITHUB_REPOSITORY_URL, openReleaseUrl } from "@/lib/update";
import { platformOfPath } from "@/lib/platforms/registry";
import { useScope } from "@/lib/use-scope";
import { cn } from "@/lib/utils";
import { useAccountsStore } from "@/stores/accounts";

/**
 * **应用级设置**的唯一实现：语言 / 外观（主题）/ 开机自启 / 自动更新。
 *
 * ## 为什么这四块可以跨产品共用
 *
 * 它们改的是**本应用**的状态 —— 界面语言、主题偏好、系统登录项、GitHub 更新源，
 * 读的也是**本应用**的版本号（`update::APP_VERSION`），与 WorkBuddy / Trae 两条
 * 产品线无关。`docs/specs` 早已把它们定性为「应用级能力 / 全局共用项，不计为
 * Trae 差距」（`trae-parity-matrix.md:103,231,236`、`trae-parity-design.md:225`）。
 *
 * 与 `settings-primitives.tsx` 的分工：那里共享**纯版式原语**（无取数、无判断），
 * 这里共享**应用级业务块**（含取数）。**产品耦合的块不得搬进来** —— 依赖 `Region`
 * 的（版本与账号库 / 权限检测 / 网关）留在 `SettingsPage`，依赖 `TraeVariant` 的
 * 留在 `TraeSettingsPage`；`gateway/*` 那类耦合 `useGatewayStore` 的组件也照旧各自实现
 * （见 `trae-parity-design.md §1.5`）。
 *
 * 历史：这三块原先只存在于 WorkBuddy 设置页，Trae 设置页完全没有 —— 同一件事
 * 在两个产品上的可得性不一致。抽到共享模块后两边走同一份实现，只可能一起变。
 *
 * ## 语言为什么也放在这里而不是各产品设置页
 *
 * 界面语言是**整个应用**的属性：两个产品分区共用同一套控件与文案，把它放进某一个
 * 产品的设置页，就会出现「在 Trae 里改不了语言」这种取决于当前分区的能力差异 ——
 * 正是本模块当初要消灭的那类不一致。判据仍是「有无产品耦合」：语言完全没有。
 */

/**
 * 版式变体 —— 同一份内容有两种出口（原型里也是两处渲染）：
 *
 * - `card`（默认）：设置页的 `<section class="card">`，标题在 `.card-head` 里。
 * - `plain`：顶栏齿轮抽屉的 `.dsec`，标题是 11.5px muted 的 `h4`，行直接贴抽屉底色。
 *
 * ★ 用参数而不是复制一份组件：这五块**含取数**（`getGithubConfig` / `getLaunchAtLoginEnabled`），
 * 复制一份就会长成两份会各自漂移的实现 —— 正是本模块当初要消灭的病。
 */
export type AppSettingsVariant = "card" | "plain";

/**
 * 分组正文容器。
 *
 * ⚠️ `plain` 下**必须直接展开**（`<>`）：原型 `.dsec` 靠 `gap:9px` 给行距，
 * 而 `gap` 只作用于**直接子级**。夹一层 `CardContent` 会让行距塌成 0，行与行
 * 只剩一条分隔线贴着 —— 与抽屉里其它分区的节奏对不上。
 */
function GroupBody({ variant, children }: { variant: AppSettingsVariant; children: ReactNode }) {
  if (variant === "plain") return <>{children}</>;
  return <CardContent className="space-y-0 p-0">{children}</CardContent>;
}

/** 自动更新：检查公开 GitHub Releases 源 + 安装签名更新。 */
function UpdateCard({ variant }: { variant: AppSettingsVariant }) {
  const t = useT();
  const version = useAccountsStore((s) => s.status?.version);
  const [autoCheck, setAutoCheck] = useAutoCheckUpdate();
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [msg, setMsg] = useState<{ type: "ok" | "err"; text: string } | null>(null);
  const [githubConfig, setGithubConfig] = useState<GithubConfig>({});
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxySaving, setProxySaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void api
      .getGithubConfig()
      .then((config) => {
        if (cancelled) return;
        setGithubConfig(config);
        setProxyUrl(config.proxy ?? "");
      })
      .catch((e) => {
        if (!cancelled) setMsg({ type: "err", text: api.asError(e) });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function check() {
    setChecking(true);
    setMsg(null);
    try {
      const r = await api.checkUpdate(proxyUrl, true);
      /* 版本说明跨了「外部资产 → 渲染」的边界，进 state 前先按白名单归一（同 AppFooter）。 */
      setInfo({ ...r, notes: normalizeReleaseNotes(r.notes) });
      if (!r.ok) {
        setMsg({ type: "err", text: r.message || r.error || t("appSettings.update.checkFailed") });
      }
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setChecking(false);
    }
  }

  async function saveProxy() {
    const value = proxyUrl.trim();
    if (value) {
      try {
        const parsed = new URL(value);
        if (!parsed.hostname || !["http:", "https:"].includes(parsed.protocol)) {
          throw new Error("unsupported proxy protocol");
        }
      } catch {
        setMsg({ type: "err", text: t("appSettings.update.proxy.invalid") });
        return;
      }
    }

    setProxySaving(true);
    setMsg(null);
    try {
      const saved = await api.saveGithubConfig({ ...githubConfig, proxy: value });
      setGithubConfig(saved);
      setProxyUrl(saved.proxy ?? "");
      setMsg({
        type: "ok",
        text: value ? t("appSettings.update.proxy.saved") : t("appSettings.update.proxy.cleared"),
      });
    } catch (e) {
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setProxySaving(false);
    }
  }

  /** 卡片形态下给「代理 / 更新源」这一组行铺一层 `--muted` 底，抽屉形态下不铺（原型 `.srow` 无底色）。 */
  const tint = variant === "card" ? "bg-muted/25" : undefined;

  return (
    <SettingsGroup
      id="settings-updates"
      title={t("appSettings.update.title")}
      variant={variant}
    >
      <GroupBody variant={variant}>
        {/* ★ 自动检查更新：原型这一行的说明就是「每 30 分钟检查一次」
            （`buddy-switch-prototype.html:4459`），间隔常量与轮询实现同源
            （`lib/update-preference.ts` 的 `AUTO_CHECK_UPDATE_INTERVAL_MS` → `App.tsx` 的 AppFooter）。
            ⚠️ 不加 `operational`：这是 localStorage 偏好，演示模式下**真的能用**，
            遮起来反而变成假控件。 */}
        <SettingsFieldRow
          stacked={variant === "plain"}
          label={t("appSettings.update.auto.label")}
          description={t("appSettings.update.auto.description")}
          htmlFor="update-auto-check"
        >
          <Switch
            id="update-auto-check"
            checked={autoCheck}
            onCheckedChange={(value) => setAutoCheck(value)}
            aria-label={t("appSettings.update.auto.label")}
          />
        </SettingsFieldRow>

        <div className="border-b border-border px-4 py-3 text-sm">
          {t("appSettings.update.current")}
          <span className="font-mono">v{version || "?"}</span>
        </div>

        <div className={cn("flex min-w-0 items-center justify-between gap-3 border-b border-border px-4 py-3 text-sm", tint)}>
          <div className="min-w-0 flex-1">
            <div className="font-medium">{t("appSettings.update.source")}</div>
            <div className="truncate text-xs text-muted-foreground">{GITHUB_REPOSITORY_URL}</div>
          </div>
          <DemoAction><Button
            variant="ghost"
            size="icon"
            title={t("appSettings.update.openReleaseTitle")}
            onClick={() => void openReleaseUrl(GITHUB_RELEASE_URL)}
          >
            <ExternalLink />
          </Button></DemoAction>
        </div>

        <SettingsFieldRow
          label={t("appSettings.update.proxy.label")}
          description={t("appSettings.update.proxy.description")}
          htmlFor="update-proxy"
          className={tint}
          operational
          /* 抽屉形态（460px）必须纵向堆叠：`sm:` 是视口断点，在抽屉里会误判成够宽。 */
          stacked={variant === "plain"}
        >
          <Input
            id="update-proxy"
            className={cn("w-full", variant === "card" && "sm:w-80")}
            value={proxyUrl}
            onChange={(event) => setProxyUrl(event.target.value)}
            placeholder={t("appSettings.update.proxy.placeholder")}
            spellCheck={false}
            autoComplete="off"
          />
        </SettingsFieldRow>

        <div className={cn("flex flex-wrap gap-2 border-b border-border px-4 py-3", tint)}>
          <DemoAction><Button size="sm" variant="outline" onClick={() => void saveProxy()} disabled={proxySaving}>
            {proxySaving ? <Loader2 className="animate-spin" /> : <Save />}
            {t("appSettings.update.proxy.save")}
          </Button></DemoAction>
        </div>

        <div className="flex flex-wrap gap-2 border-b border-border px-4 py-3">
          <DemoAction><Button size="sm" variant="outline" onClick={check} disabled={checking}>
            {checking ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("appSettings.update.check")}
          </Button></DemoAction>
        </div>

        {info?.ok && (
          <Alert variant="default" className={cn("!w-auto mx-4 my-4 sm:mx-5", info.hasUpdate && "border-primary/35 bg-primary/[0.06]")}>
            {info.hasUpdate && <ArrowUpCircle className="text-primary" />}
            <AlertDescription className="space-y-2">
              <AlertTitle className={cn(info.hasUpdate && "text-primary")}>
                {info.hasUpdate ? t("appSettings.update.foundTitle") : t("appSettings.update.doneTitle")}
              </AlertTitle>
              <div className="text-sm">
                {info.hasUpdate
                  ? t("appSettings.update.foundDetail", {
                      latest: info.latest ?? "?",
                      current: info.current ?? "?",
                    })
                  : t("appSettings.update.upToDate", { current: info.current ?? "?" })}
                {info.releaseName && <span className="text-muted-foreground"> · {info.releaseName}</span>}
              </div>
              {info.hasUpdate && (
                /* 入口文案从「立即升级」改成「查看更新内容」：点开的先是**说明**弹窗，
                   下载要用户在里面再点一次（见 UPDATE-NOTES-ARCH.md）。 */
                <DemoAction><Button size="sm" onClick={() => setInstallOpen(true)}>
                  <ArrowUpCircle />
                  {t("appSettings.update.viewNotes")}
                </Button></DemoAction>
              )}
              {info.releaseUrl && (
                <DemoAction><Button
                  variant="link"
                  size="sm"
                  className="h-auto p-0"
                  onClick={() => void openReleaseUrl(info.releaseUrl!)}
                >
                  {t("appSettings.update.openRelease")}
                </Button></DemoAction>
              )}
            </AlertDescription>
          </Alert>
        )}
        {msg && (
          <Alert
            variant={msg.type === "err" ? "destructive" : "default"}
            className="!w-auto mx-4 my-4 sm:mx-5"
          >
            <AlertDescription>{msg.text}</AlertDescription>
          </Alert>
        )}
        <UpdateInstallDialog
          open={installOpen}
          onOpenChange={setInstallOpen}
          update={info}
        />
      </GroupBody>
    </SettingsGroup>
  );
}

/**
 * 启动与托盘（仅桌面端渲染）—— 两行，都是「应用在窗口之外怎么活着」。
 *
 * - **开机时静默启动到托盘**：真值来自系统登录项（`tauri-plugin-autostart`）。
 * - **关闭窗口时保持后台运行**：真值来自 `app_prefs.json`（`close_to_tray`）；
 *   宿主在 `WindowEvent::CloseRequested` 里读它决定「隐藏还是退出」。
 *
 * ## 为什么两行同卡
 *
 * 原型把「开机自启」与「最小化到托盘」摆在同一列（`buddy-switch-prototype.html:4458-4460`），
 * 两件事也确实成对：一个管「开机后进不进托盘」，一个管「关窗后留不留在托盘」。
 * 拆成两张单行卡只会多一条分组标题。
 *
 * ## 两个开关的载体不同，是**刻意**的
 *
 * 开机自启的权威在**操作系统注册项**（应用不另存布尔值，否则会出现「系统已禁用但设置显示已开启」）；
 * 关闭到托盘的权威在**本应用自己的偏好文件**（宿主需要读，见 `app_prefs.rs` 模块头）。
 * 两者都用「写后回读、失败回滚」的同一种交互，但**不共用存储**。
 */
function StartupCard({ variant }: { variant: AppSettingsVariant }) {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ type: "ok" | "err"; text: string } | null>(null);
  /** `close_to_tray`：与开机自启**分开**的 busy / 错误态 —— 一个失败不该锁住另一个。 */
  const [closeToTray, setCloseToTray] = useState<boolean | null>(null);
  const [trayBusy, setTrayBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void api
      .getLaunchAtLoginEnabled()
      .then((value) => {
        if (!cancelled) setEnabled(value);
      })
      .catch((e) => {
        if (!cancelled) setMsg({ type: "err", text: api.asError(e) });
      });
    void api
      .getAppPrefs()
      .then((prefs) => {
        if (!cancelled) setCloseToTray(prefs.close_to_tray);
      })
      .catch((e) => {
        if (!cancelled) setMsg({ type: "err", text: api.asError(e) });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function onToggle(value: boolean) {
    if (busy || enabled === null) return;
    const previous = enabled;
    setBusy(true);
    setMsg(null);
    try {
      // 后端回读 OS 权威状态；即使与请求一致，也以回读值显示。
      const authoritative = await api.setLaunchAtLoginEnabled(value);
      setEnabled(authoritative);
      setMsg({
        type: "ok",
        text: authoritative
          ? t("appSettings.startup.enabled")
          : t("appSettings.startup.disabled"),
      });
    } catch (e) {
      // 失败时恢复到最后一次确认的状态，并显示可读错误。
      setEnabled(previous);
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  async function onToggleCloseToTray(value: boolean) {
    if (trayBusy || closeToTray === null) return;
    const previous = closeToTray;
    // 乐观更新：开关必须立刻跟手（宿主是「点关闭那一刻」才读文件，中间没有状态可漂）。
    setCloseToTray(value);
    setTrayBusy(true);
    setMsg(null);
    try {
      /* 先读回完整偏好再写：`set_app_prefs` 是按**已知键**归一化的，只传
         `close_to_tray` 会把 `legal_consent_at`（风险确认时间）冲回默认 0 ——
         用户点一次托盘开关就被要求重新确认风险，属于跨功能的状态破坏。 */
      const current = await api.getAppPrefs();
      // 按**回读值**渲染：后端会把非法值归一成默认值，回显请求值会与宿主真实行为分叉。
      const saved = await api.setAppPrefs({ ...current, close_to_tray: value });
      setCloseToTray(saved.close_to_tray);
      if (saved.close_to_tray !== value) {
        setMsg({ type: "err", text: t("appSettings.tray.saveMismatch") });
      }
    } catch (e) {
      setCloseToTray(previous);
      setMsg({ type: "err", text: api.asError(e) });
    } finally {
      setTrayBusy(false);
    }
  }

  const label = t("appSettings.startup.silent.label");
  const trayLabel = t("appSettings.tray.closeToTray.label");

  return (
    <SettingsGroup
      id="settings-startup"
      title={t("appSettings.startup.title")}
      variant={variant}
    >
      <GroupBody variant={variant}>
        <SettingsFieldRow
          label={label}
          description={t("appSettings.startup.silent.description")}
          htmlFor="startup-silent"
          operational
        >
          <Switch
            id="startup-silent"
            checked={enabled ?? false}
            disabled={busy || enabled === null}
            onCheckedChange={(v) => void onToggle(v)}
            aria-label={label}
          />
        </SettingsFieldRow>

        <SettingsFieldRow
          className="border-b-0"
          stacked={variant === "plain"}
          label={trayLabel}
          description={t("appSettings.tray.closeToTray.description")}
          htmlFor="tray-close-to-tray"
        >
          <Switch
            id="tray-close-to-tray"
            checked={closeToTray ?? true}
            disabled={trayBusy || closeToTray === null}
            onCheckedChange={(v) => void onToggleCloseToTray(v)}
            aria-label={trayLabel}
          />
        </SettingsFieldRow>

        {msg && (
          <Alert
            variant={msg.type === "err" ? "destructive" : "default"}
            className="!w-auto mx-4 my-4 sm:mx-5"
          >
            <AlertDescription>{msg.text}</AlertDescription>
          </Alert>
        )}
      </GroupBody>
    </SettingsGroup>
  );
}

/** 外观：主题选择（持久化到 localStorage）。 */
function AppearanceCard({ variant }: { variant: AppSettingsVariant }) {
  const t = useT();
  const [theme, setTheme] = useState<ThemePreference>(getThemePreference);

  function onThemeChange(value: string) {
    if (value !== "system" && value !== "light" && value !== "dark") return;
    setThemePreference(value);
    setTheme(value);
  }

  return (
    <SettingsGroup
      id="settings-appearance"
      title={t("appSettings.appearance.title")}
      variant={variant}
    >
      <GroupBody variant={variant}>
        <SettingsFieldRow
          className="border-b-0"
          label={t("appSettings.appearance.theme.label")}
          description={t("appSettings.appearance.theme.description")}
          htmlFor="appearance-theme"
        >
          <Select value={theme} onValueChange={onThemeChange}>
            <SelectTrigger
              id="appearance-theme"
              size="sm"
              className="w-full sm:w-40"
              aria-label={t("appSettings.appearance.theme.aria")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="system">{t("appSettings.theme.system")}</SelectItem>
              <SelectItem value="light">{t("appSettings.theme.light")}</SelectItem>
              <SelectItem value="dark">{t("appSettings.theme.dark")}</SelectItem>
            </SelectContent>
          </Select>
        </SettingsFieldRow>
      </GroupBody>
    </SettingsGroup>
  );
}

/**
 * 语言：界面语言选择。
 *
 * 与主题**同一个归宿**（localStorage，设备级、不上传、不入后端配置）：两者都是
 * 「这台机器上这个人怎么看界面」的偏好，没有跨设备同步的诉求。做成后端配置反而会
 * 让 webui 与桌面端两条通道都要各加一组命令与登记点（见 §二「两条通道形状一致」）。
 *
 * 两个选项的文案是**自名的**（`简体中文` / `English`）：语言菜单用目标语言写自己的名字，
 * 是通行做法 —— 当前界面语言看不懂时，用户仍能认出自己要去哪一项。
 */
function LanguageCard({ variant }: { variant: AppSettingsVariant }) {
  const t = useT();
  const locale = useLocale();

  return (
    <SettingsGroup
      id="settings-language"
      title={t("appSettings.language.title")}
      variant={variant}
    >
      <GroupBody variant={variant}>
        <SettingsFieldRow
          className="border-b-0"
          label={t("appSettings.language.label")}
          description={t("appSettings.language.description")}
          htmlFor="appearance-language"
        >
          <Select
            value={locale}
            onValueChange={(value) => {
              if (isLocale(value)) setLocale(value);
            }}
          >
            <SelectTrigger
              id="appearance-language"
              size="sm"
              className="w-full sm:w-40"
              aria-label={t("appSettings.language.label")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="zh">{t("appSettings.language.zh")}</SelectItem>
              <SelectItem value="en">{t("appSettings.language.en")}</SelectItem>
            </SelectContent>
          </Select>
        </SettingsFieldRow>
      </GroupBody>
    </SettingsGroup>
  );
}

/**
 * 键盘快捷键（P6 §3.1）：作用域快捷键「可关闭」开关。
 *
 * ## 为什么降级时**不显示开关**
 *
 * 快捷键在「作用域 > 9 个」时降级为不绑定（`planScopeShortcuts`）。此时**没有可开启之物**
 * —— 再摆一个开关就是假控件（§1.7）。因此 `reason === "too-many"` 时改为一句说明，
 * 与快捷键的真实绑定状态**同源**（都读 `planScopeShortcuts` 的结果）。
 *
 * ## 为什么这是**应用级**设置
 *
 * 数字快捷键作用于整个应用的全局键（不区分 WorkBuddy / Trae），与主题 / 语言同类，
 * 因此放在共享的 `AppSettingsGroups` 里，两个产品分区走同一份实现。
 */
function ShortcutsCard({ variant }: { variant: AppSettingsVariant }) {
  const t = useT();
  const [enabled, setEnabled] = useShortcutsEnabled();
  const visibleScopes = useVisibleScopes((s) => s.visibleScopes);
  const plan = useMemo(
    () => planScopeShortcuts({ scopes: visibleScopes, enabled }),
    [visibleScopes, enabled],
  );
  const isMac = typeof navigator !== "undefined" && navigator.userAgent.includes("Macintosh");
  const label = t("shortcuts.settings.label");
  // 降级（作用域超过上限）⇒ 无可开启之物，不显示开关。
  const degradedByCount = plan.reason === "too-many";

  return (
    <SettingsGroup id="settings-shortcuts" title={t("shortcuts.settings.title")} variant={variant}>
      <GroupBody variant={variant}>
        <SettingsFieldRow
          className="border-b-0"
          label={label}
          description={
            degradedByCount
              ? t("shortcuts.settings.degraded", { limit: SCOPE_SHORTCUT_LIMIT })
              : t("shortcuts.settings.description", {
                  mod: isMac ? "⌘" : "Ctrl",
                  n: plan.bindings.size,
                })
          }
          htmlFor="shortcuts-enabled"
          operational
        >
          {degradedByCount ? null : (
            <Switch
              id="shortcuts-enabled"
              checked={enabled}
              onCheckedChange={(value) => setEnabled(value)}
              aria-label={label}
            />
          )}
        </SettingsFieldRow>
      </GroupBody>
    </SettingsGroup>
  );
}

/**
 * 作用域可见性设置 —— **白名单**模型（存可见的作用域）。
 *
 * ## 为什么是应用级设置
 *
 * 作用域可见性是全局偏好，与语言 / 主题属于同一层级：它影响整个应用的导航入口，
 * 而不是某个产品线的局部行为。
 *
 * ## 约束规则（★ 禁用判据必须**方向敏感**，issue #85）
 *
 * - 至少保留 1 个可见：**只有「已可见且是最后一个可见项」**才禁用（关了就空了）；
 * - 当前作用域可见时不可隐藏 —— 它是顶栏选择器与全局键的锚点。
 *
 * ## ★★ 为什么禁用判据不能与方向无关（issue #85 真因）
 *
 * 旧实现是 `scope === currentScope || visibleScopes.length === 1` ——
 * 后者**与勾选状态无关**：一旦可见集合降到 1 个，**6 个开关全部 disabled**，
 * 于是「关掉的作用域再也打不开」，而 `setVisibleScopes` 又拒绝写空集合 ⇒
 * UI 层与 store 层互相锁死，只能清 localStorage 才能恢复。
 *
 * 同理 `scope === currentScope` 也不能无条件禁用：当前作用域被隐藏时（URL 直达、
 * 或在另一产品的设置页里关掉了它），那一行就是 `disabled + off`，**永远点不回来**。
 * 现在只在「当前作用域**已可见**」时禁用 —— 隐藏态下它必须能点回来。
 */
function ScopeVisibilityCard() {
  const t = useT();
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const currentScope = scopeOf(product, region);
  const visibleScopes = useVisibleScopes((s) => s.visibleScopes);
  const setVisibleScopes = useVisibleScopes((s) => s.setVisibleScopes);

  function toggleScope(scope: Scope) {
    const isCurrentlyVisible = visibleScopes.includes(scope);
    const next = isCurrentlyVisible
      ? visibleScopes.filter((s) => s !== scope)
      : [...visibleScopes, scope];
    setVisibleScopes(next);
    // 隐藏到只剩 1 个时提示（开启方向不存在降级，无需提示）
    if (isCurrentlyVisible && next.length === 1) {
      toast.info(t("appSettings.scopeVisibility.onlyOneLeft"));
    }
  }

  return (
    <SettingsGroup id="settings-scope-visibility" title={t("appSettings.scopeVisibility.groupTitle")}>
      <CardContent className="space-y-0 p-0">
        {SCOPES.map((scope) => {
          const { product: scopeProduct } = scopeParts(scope);
          const isVisible = visibleScopes.includes(scope);
          // ★ 方向敏感（issue #85）：只在「关掉它会破坏约束」时禁用。
          //   - 已可见且是最后一个可见项 ⇒ 不能再关（否则可见集合为空）；
          //   - 当前作用域**已可见** ⇒ 不可隐藏（顶栏选择器 / 全局键的锚点）。
          //   未勾选项**一律可点** —— 这正是旧实现缺的那一半，它让关到只剩 1 个之后
          //   其余 5 个永远打不开。当前作用域若已被隐藏，也必须允许点回来。
          const disabled = isVisible && (visibleScopes.length === 1 || scope === currentScope);

          return (
            <SettingsFieldRow
              key={scope}
              label={
                <div className="flex items-center gap-2">
                  <ScopeMark product={scopeProduct} size={14} className="shrink-0" />
                  <span>{scopeLabel(t, scope)}</span>
                </div>
              }
              operational
              htmlFor={`scope-visible-${scope}`}
            >
              <Switch
                id={`scope-visible-${scope}`}
                checked={isVisible}
                disabled={disabled}
                onCheckedChange={() => toggleScope(scope)}
                aria-label={t("appSettings.scopeVisibility.toggleAria", { scope })}
              />
            </SettingsFieldRow>
          );
        })}

        {/* 全局提示 */}
        <p className="border-t border-border/60 px-4 py-3 text-xs text-muted-foreground">
          {t("appSettings.scopeVisibility.hint")}
        </p>
      </CardContent>
    </SettingsGroup>
  );
}

/**
 * 五个应用级设置块，**门禁与原先设置页逐条对齐**。
 *
 * 门禁必须留在这里而不是交给调用方：本模块同时被设置页与顶栏 ⚙「应用设置」抽屉引用，
 * 把判断写在页面上会让两条路径各写一遍 —— 这正是当初 `settings-primitives`
 * 出现漂移的同一种病。
 *
 * 顺序把**语言放在最前**：它改的是其余所有文案的语言，用户在找「设置」时最可能先要它。
 *
 * ★ `variant` 由调用方给：设置页传 `card`（默认），抽屉传 `plain`。**内容只有一份**，
 * 两个出口不会各长一套。
 */
export function AppSettingsGroups({ variant = "card" }: { variant?: AppSettingsVariant } = {}) {
  return (
    <>
      <LanguageCard variant={variant} />
      <AppearanceCard variant={variant} />
      <ShortcutsCard variant={variant} />
      {api.isDesktop() || api.isDemoMode() ? <StartupCard variant={variant} /> : null}
      {api.isWebui() && !api.isDemoMode() ? null : <UpdateCard variant={variant} />}
      <ScopeVisibilityCard />
    </>
  );
}
