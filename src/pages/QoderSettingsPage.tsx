import { useEffect, useState } from "react";
import { useLocation, useSearchParams } from "react-router-dom";
import { Globe, MonitorSmartphone, ShieldAlert } from "lucide-react";

import { AppSettingsGroups } from "@/components/app-settings";
import { QoderLocalSourceRow } from "@/components/qoder/qoder-local-source-row";
import { TimesEditor } from "@/components/schedule-times-editor";
import { BackupSection } from "@/components/settings/backup-section";
import { SettingsFieldRow, SettingsGroup } from "@/components/settings-primitives";
import {
  parseSettingsSubnav,
  SettingsSubnav,
  SETTINGS_SUBNAV,
  type SettingsSubnavId,
} from "@/components/settings/settings-subnav";
import { ScopeChip, scopeLabel } from "@/components/scope-chip";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import * as api from "@/lib/api";
import type { QoderLocalSourcesView, QoderWebProxyConfig } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { platformOfPath } from "@/lib/platforms/registry";
import {
  loadScheduleConfig,
  saveSchedulePatch,
  SCHEDULE_CONFIG_KEY,
} from "@/lib/schedule-config";
import type { ScheduleConfig } from "@/lib/types";
import { useCachedResource } from "@/lib/use-cached-resource";
import { scopeOf } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import type { TranslationKey } from "@/locales/zh";
import { toast } from "sonner";

/**
 * Qoder 设置页。
 *
 * ## 与另两个设置页的关系
 *
 * 子导航**共用同一张表**（`SETTINGS_SUBNAV`，PLAN §3.9 / D12），版式同构：
 * `data-split-layout` + `parseSettingsSubnav` 的 URL 承载 + 主区 16px 间距。
 *
 * ## ★ 为什么只列「已有内容」的分区
 *
 * 表里 9 项是按 WB / Trae 的内容收敛出来的；Qoder 侧目前**只有四处有真内容**：
 * 通用（应用级设置）、客户端检测（本机凭据四态探测）、计划任务（签到 / 令牌保活排程）、
 * 备份与恢复。其余五项（账号 / 签到 / 网关 / 权限与诊断 / 日志）在 Qoder 侧**没有设置项**
 * —— 摆上去就是 §1.7 说的**假控件**（点了只有一句「尚未接入」，比不摆更糟）。
 * 因此用 `SettingsSubnav` 的 `items` 收窄，而不是复制一份表（复制即漂移的种子）。
 * 对应功能落地后，把它的 id 加进 {@link QODER_SETTINGS_SECTIONS} 即可。
 */
const QODER_SECTION_IDS: readonly SettingsSubnavId[] = [
  "general",
  "client",
  "network",
  "schedule",
  "backup",
];

/** 收窄后的子导航项（顺序取自共享表，不另排）。 */
const QODER_SETTINGS_SECTIONS = SETTINGS_SUBNAV.filter((item) => QODER_SECTION_IDS.includes(item.id));

export default function QoderSettingsPage() {
  const t = useT();
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  const scope = scopeOf(product, region);

  /**
   * 计划任务（签到 / 令牌保活）。
   *
   * 排程是**全局单份**（`schedule_config.json`，按任务分家而不是按产品 / 区域），
   * 与 Trae / WorkBuddy 的设置页读写同一份文件、共用 `SCHEDULE_CONFIG_KEY` ⇒
   * 一处改完另一处下次挂载即拿到新值。
   */
  const { data: schedule, patch: patchSchedule } = useCachedResource<ScheduleConfig>(
    SCHEDULE_CONFIG_KEY,
    loadScheduleConfig,
  );
  const [scheduleSaving, setScheduleSaving] = useState(false);

  /**
   * 逐包取数的出站代理（issue #111 收尾）。
   *
   * 一份配置、两个区域（CN / 国际版）。草稿与已存值分家：输入过程中不触发保存，
   * 点「保存」才整份提交 —— 后端按整份解析，避免半份覆盖。
   */
  const [proxy, setProxy] = useState<QoderWebProxyConfig | null>(null);
  const [proxyDraft, setProxyDraft] = useState<QoderWebProxyConfig>({ cn: "", global: "" });
  const [proxySaving, setProxySaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .qoderWebProxyGet()
      .then((config) => {
        if (cancelled) return;
        setProxy(config);
        setProxyDraft(config);
      })
      .catch(() => {
        /* 读不到就保持空草稿：保存时后端会给形状校验错误，比静默失败好定位。 */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function saveProxy() {
    if (proxySaving) return;
    const previous = proxy;
    setProxySaving(true);
    try {
      const saved = await api.qoderWebProxySet(proxyDraft);
      setProxy(saved.config);
      setProxyDraft(saved.config);
      toast.success(t("qoder.page.settings.proxy.saved"));
    } catch (error) {
      // 失败回滚草稿到已存值（后端校验错误会原样给用户看，改完再存）。
      if (previous) setProxyDraft(previous);
      toast.error(t("qoder.page.settings.proxy.saveFailed"), { description: api.asError(error) });
    } finally {
      setProxySaving(false);
    }
  }

  /**
   * 保存排程：乐观落本地 → **整份**提交 → 回读权威值；失败回滚并提示。
   *
   * 必须带当前整份配置（见 `saveSchedulePatch`）：后端按整份解析，只发一个字段
   * 会让其余字段被默认值覆盖 —— 那是静默的配置丢失。
   */
  async function saveSchedule(next: Partial<ScheduleConfig>, failureKey: TranslationKey) {
    if (!schedule || scheduleSaving) return;
    const previous = schedule;
    setScheduleSaving(true);
    patchSchedule((prev) => ({ ...prev, ...next }));
    try {
      const saved = await saveSchedulePatch(previous, next);
      patchSchedule(() => saved);
    } catch (error) {
      patchSchedule(() => previous);
      toast.error(t(failureKey), { description: api.asError(error) });
    } finally {
      setScheduleSaving(false);
    }
  }

  const [searchParams, setSearchParams] = useSearchParams();
  /**
   * ★ 初始分区从 URL 读，且**按本页的分区集校验**：`?section=gateway` 这类
   * （表里有、本页没有）必须 fail-closed 回 `general` —— 否则会渲染出一个空的主区，
   * 用户看到「点了没反应」。
   */
  const [section, setSection] = useState<SettingsSubnavId>(() => {
    const parsed = parseSettingsSubnav(searchParams.get("section"));
    return QODER_SECTION_IDS.includes(parsed) ? parsed : "general";
  });

  const [sources, setSources] = useState<QoderLocalSourcesView | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setError("");
    api
      .getQoderLocalSources(scope)
      .then((view) => {
        if (!cancelled) setSources(view);
      })
      .catch((err) => {
        if (!cancelled) setError(api.asError(err));
      });
    return () => {
      cancelled = true;
    };
  }, [scope]);

  /** 切换分区：**同时写 URL**（与另两个设置页同构 —— 位置真值在 URL）。 */
  function goSection(id: SettingsSubnavId) {
    setSection(id);
    const next = new URLSearchParams(searchParams);
    next.set("section", id);
    setSearchParams(next, { replace: true });
  }

  return (
    <div className="mx-auto min-w-0 w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]">
      <header className="mb-5 flex items-start gap-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-[640] tracking-[-0.02em]">{t("qoder.page.settings.title")}</h1>
          <p className="mt-1.5 text-[12.5px] text-muted-foreground">
            {t("pageHead.settings", { scope: scopeLabel(t, scope) })}
          </p>
        </div>
        <div className="grow" />
        <ScopeChip scope={scope} />
      </header>

      {/* 两栏容器：方向 / 断点整组在 `index.css` 的 `[data-split-layout]` 里。 */}
      <div data-split-layout className="min-w-0">
        <SettingsSubnav items={QODER_SETTINGS_SECTIONS} active={section} onSelect={goSection} />

        <div className="min-w-0 flex-1 space-y-4">
          {section === "general" && <AppSettingsGroups />}

          {section === "client" && (
            <Card className="gap-0 py-0">
              <div className="flex flex-wrap items-start gap-3 px-4 py-3.5">
                <MonitorSmartphone className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                <div className="min-w-0 grow">
                  <div className="text-[13px] font-[550]">{t("qoder.page.settings.section.client")}</div>
                  <div className="mt-0.5 text-[11.5px] text-muted-foreground">
                    {t("qoder.page.settings.client.description")}
                  </div>
                </div>
              </div>
              {error ? (
                <div className="flex items-start gap-2.5 border-t px-4 py-3 text-[12px] text-destructive">
                  <ShieldAlert className="mt-0.5 size-4 shrink-0" />
                  <div className="min-w-0">{t("qoder.page.settings.client.error", { error })}</div>
                </div>
              ) : sources ? (
                <div className="border-t">
                  {sources.sources.map((source) => (
                    <QoderLocalSourceRow key={source.kind} source={source} />
                  ))}
                </div>
              ) : (
                <div className="space-y-2 border-t px-4 py-3">
                  <Skeleton className="h-3 w-40" />
                  <Skeleton className="h-3 w-64" />
                </div>
              )}
            </Card>
          )}

          {/* ---- 计划任务：Qoder 的两类排程（自动签到 / 令牌保活）----
              排程是**全局单份**（`schedule_config.json`），本页只呈现 Qoder 的两类任务。
              两组同处一个分区，因为它们是同一份配置的两个字段。 */}
          {section === "schedule" && (
            <>
              <SettingsGroup
                id="qoder-settings-auto-checkin"
                title={t("qoder.page.settings.schedule.checkinGroup")}
              >
                <CardContent className="space-y-0 p-0">
                  <SettingsFieldRow
                    label={t("qoder.page.settings.schedule.checkinEnable")}
                    description={t("qoder.page.settings.schedule.checkinDesc")}
                  >
                    <Switch
                      checked={schedule?.qoder_checkin_enabled ?? false}
                      disabled={!schedule || scheduleSaving}
                      onCheckedChange={(checked) =>
                        void saveSchedule(
                          { qoder_checkin_enabled: checked },
                          "qoder.page.settings.schedule.saveFailed",
                        )
                      }
                      aria-label={t("qoder.page.settings.schedule.checkinEnable")}
                    />
                  </SettingsFieldRow>
                  <SettingsFieldRow
                    label={t("qoder.page.settings.schedule.checkinTimesLabel")}
                    description={t("qoder.page.settings.schedule.checkinTimesDesc")}
                    htmlFor="qoder-checkin-time"
                  >
                    <TimesEditor
                      id="qoder-checkin-time"
                      times={schedule?.qoder_checkin_times ?? []}
                      jitter={schedule?.qoder_checkin_jitter_minutes ?? 0}
                      disabled={!schedule || scheduleSaving}
                      onChange={(times) =>
                        void saveSchedule(
                          { qoder_checkin_times: times },
                          "qoder.page.settings.schedule.saveFailed",
                        )
                      }
                      onJitterChange={(minutes) =>
                        void saveSchedule(
                          { qoder_checkin_jitter_minutes: minutes },
                          "qoder.page.settings.schedule.saveFailed",
                        )
                      }
                    />
                  </SettingsFieldRow>
                </CardContent>
              </SettingsGroup>

              {/* 令牌保活：与上面同形，但默认**开**（refresh_token 的唯一用途就是续期，
                  它是用户网页登录时明确交出来的）。不摆「立即续期」按钮 ——
                  手动续期在账号卡片的菜单里，重复摆第二个入口就是假控件。 */}
              <SettingsGroup
                id="qoder-settings-keepalive"
                title={t("qoder.page.settings.schedule.keepaliveGroup")}
              >
                <CardContent className="space-y-0 p-0">
                  <SettingsFieldRow
                    label={t("qoder.page.settings.schedule.keepaliveEnable")}
                    description={t("qoder.page.settings.schedule.keepaliveDesc")}
                  >
                    <Switch
                      checked={schedule?.qoder_keepalive_enabled ?? false}
                      disabled={!schedule || scheduleSaving}
                      onCheckedChange={(checked) =>
                        void saveSchedule(
                          { qoder_keepalive_enabled: checked },
                          "qoder.page.settings.schedule.saveFailed",
                        )
                      }
                      aria-label={t("qoder.page.settings.schedule.keepaliveEnable")}
                    />
                  </SettingsFieldRow>
                  <SettingsFieldRow
                    label={t("qoder.page.settings.schedule.keepaliveTimesLabel")}
                    description={t("qoder.page.settings.schedule.keepaliveTimesDesc")}
                    htmlFor="qoder-keepalive-time"
                  >
                    <TimesEditor
                      id="qoder-keepalive-time"
                      times={schedule?.qoder_keepalive_times ?? []}
                      jitter={schedule?.qoder_keepalive_jitter_minutes ?? 0}
                      disabled={!schedule || scheduleSaving}
                      onChange={(times) =>
                        void saveSchedule(
                          { qoder_keepalive_times: times },
                          "qoder.page.settings.schedule.saveFailed",
                        )
                      }
                      onJitterChange={(minutes) =>
                        void saveSchedule(
                          { qoder_keepalive_jitter_minutes: minutes },
                          "qoder.page.settings.schedule.saveFailed",
                        )
                      }
                    />
                  </SettingsFieldRow>
                </CardContent>
              </SettingsGroup>
            </>
          )}

          {/* ---- 网络：逐包取数的出站代理（issue #111 收尾）----
              按区域分两份；留空 = 直连。★ 登录窗走的是系统代理（WebView2 继承），
              不受这里影响 —— 文案必须写清，否则用户会以为配了代理反而把登录窗弄断。 */}
          {section === "network" && (
            <SettingsGroup id="qoder-settings-proxy" title={t("qoder.page.settings.proxy.group")}>
              <CardContent className="space-y-0 p-0">
                <SettingsFieldRow
                  label={t("qoder.page.settings.proxy.cn")}
                  description={t("qoder.page.settings.proxy.cnHint")}
                  htmlFor="qoder-proxy-cn"
                >
                  <Input
                    id="qoder-proxy-cn"
                    value={proxyDraft.cn}
                    placeholder="http://127.0.0.1:7890"
                    className="h-8 w-64 font-mono text-[12px]"
                    onChange={(event) => setProxyDraft((prev) => ({ ...prev, cn: event.target.value }))}
                  />
                </SettingsFieldRow>
                <SettingsFieldRow
                  label={t("qoder.page.settings.proxy.global")}
                  description={t("qoder.page.settings.proxy.globalHint")}
                  htmlFor="qoder-proxy-global"
                >
                  <Input
                    id="qoder-proxy-global"
                    value={proxyDraft.global}
                    placeholder="http://127.0.0.1:7890"
                    className="h-8 w-64 font-mono text-[12px]"
                    onChange={(event) =>
                      setProxyDraft((prev) => ({ ...prev, global: event.target.value }))
                    }
                  />
                </SettingsFieldRow>
                <div className="flex items-center justify-between gap-3 border-t px-4 py-3">
                  <p className="min-w-0 text-[11.5px] leading-relaxed text-muted-foreground">
                    {t("qoder.page.settings.proxy.note")}
                  </p>
                  <Button
                    type="button"
                    size="sm"
                    className="h-7 shrink-0"
                    disabled={proxySaving || !proxy}
                    onClick={() => void saveProxy()}
                  >
                    <Globe className="size-3.5" />
                    {proxySaving
                      ? t("qoder.page.settings.proxy.saving")
                      : t("qoder.page.settings.proxy.save")}
                  </Button>
                </div>
              </CardContent>
            </SettingsGroup>
          )}

          {section === "backup" && <BackupSection />}

          {/* 收窄子导航的**理由**写在界面上 —— 用户看不到「少了几项」却不知道原因。 */}
          <p className="text-[11.5px] text-muted-foreground">{t("qoder.page.settings.filtered")}</p>
        </div>
      </div>
    </div>
  );
}
