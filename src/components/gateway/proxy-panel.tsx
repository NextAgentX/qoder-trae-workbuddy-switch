/**
 * P12 · 请求代理面板（**C 路：base URL 重定向**）。
 *
 * ## 为什么「开启」是引导、而不是自动改配置
 *
 * `P12-ARCH.md` 的 T02 定的是**引导式（方案 A）**：客户端从**启动期环境变量**读业务域基址，
 * 应用**无法在不碰用户环境的前提下改它** —— 硬改会违反「绝不劫持」（D29）。
 * ⇒ 本面板**只显示要设的 env**，由用户手动设；应用只记一个本地标记。
 *
 * ## ★ 硬约束（全部可证伪，对应 `verify-behaviors.cjs:1306-1357`）
 *
 * - **默认关**（断言 1）：`enabled === false` 时显示「已关闭」。
 * - **关闭态必须同时说明「风险」2 条 + 「不劫持」4 条**（断言 2）—— **不能只写一句**。
 * - **开启态显示「拦截方式」+ 白名单表 + 兜底行**（断言 3 / 4）——
 *   兜底行必须是「其它全部域名 → **直连**」（**不是「也代理」**）。
 * - **一键还原回到已关闭态**（断言 5）。
 * - **⚠️ 诚实**：应用**不校验**代理是否生效 ⇒ 文案写「**已按引导设置**（应用不校验）」，
 *   **不许**写「运行中」或任何暗示「已验证生效」的措辞（`P12-ARCH.md` T04 补充 §①）。
 * - **拦截方式是 base URL 重定向**（`mode === "c-redirect"`）—— **不是「中间人」**。
 */
import { useEffect, useState } from "react";
import { Ban, Check, Copy, Loader2, RotateCcw } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { DemoAction } from "@/components/demo-action";
import * as api from "@/lib/api";
import type { ProxyStatus } from "@/lib/types";
import { useT } from "@/lib/i18n";

export function ProxyPanel() {
  const t = useT();
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void api
      .proxyStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  if (!status) return <Skeleton className="h-40 w-full" />;

  async function toggle(next: boolean) {
    if (busy) return;
    setBusy(true);
    try {
      setStatus(next ? await api.proxyEnable() : await api.proxyDisable());
    } catch (e) {
      toast.error(api.asError(e));
    } finally {
      setBusy(false);
    }
  }

  async function restore() {
    if (busy) return;
    setBusy(true);
    try {
      setStatus(await api.proxyDisable());
    } catch (e) {
      toast.error(api.asError(e));
    } finally {
      setBusy(false);
    }
  }

  async function copyEnv() {
    try {
      await navigator.clipboard.writeText(
        `"${status!.setting.key}": "${status!.setting.value}"`,
      );
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      /* 剪贴板不可用时不静默：提示用户手动复制 */
      toast.error(t("proxy.guide.copy"));
    }
  }

  return (
    <Card>
      <CardContent className="space-y-4 p-5">
        {/* 头部：标题 + 状态徽标 + 还原 + 开关 */}
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold">{t("proxy.title")}</h3>
              <Badge variant={status.enabled ? "default" : "secondary"} data-act="proxy-state">
                {status.enabled ? t("proxy.state.on") : t("proxy.state.off")}
              </Badge>
            </div>
            {status.enabled && (
              <p className="mt-1 text-xs text-muted-foreground">{t("proxy.state.unverified")}</p>
            )}
            {/* ★ E2：关闭态的**说明行**。此前标题下空无一物 ⇒ 用户看到「已关闭」
                却不知道「默认关闭」是产品选择、以及开启要自己动客户端。 */}
            {!status.enabled && (
              <div className="mt-1 space-y-0.5 text-xs text-muted-foreground" data-act="proxy-off-notice">
                <p className="font-medium text-foreground">{t("proxy.offNotice.title")}</p>
                <p>{t("proxy.offNotice.default")}</p>
                <p>{t("proxy.offNotice.howTo")}</p>
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <DemoAction>
              <Button
                variant="outline"
                size="sm"
                disabled={busy || !status.enabled}
                onClick={() => void restore()}
                data-act="proxy-restore"
              >
                {busy ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <RotateCcw className="size-3.5" />
                )}
                {t("proxy.restore")}
              </Button>
            </DemoAction>
            <DemoAction>
              <Switch
                checked={status.enabled}
                disabled={busy}
                onCheckedChange={(v) => void toggle(v)}
                data-act="proxy-toggle"
                aria-label={status.enabled ? t("proxy.toggle.disable") : t("proxy.toggle.enable")}
              />
            </DemoAction>
          </div>
        </div>

        {/* ★ 风险 2 条 + 不劫持 4 条 —— 原型断言 2 要求**同时**出现，不能只写一句 */}
        <div className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
          <p className="text-xs font-medium">{t("proxy.risk.title")}</p>
          <ul className="space-y-1 text-xs text-muted-foreground">
            <li>• {t("proxy.risk.traffic")}</li>
            <li>• {t("proxy.risk.noBody")}</li>
          </ul>
          <p className="pt-1 text-xs font-medium">{t("proxy.noHijack.title")}</p>
          <ul className="space-y-1 text-xs text-muted-foreground">
            {(
              [
                "proxy.noHijack.never",
                "proxy.noHijack.hosts",
                "proxy.noHijack.dns",
                "proxy.noHijack.transparent",
              ] as const
            ).map((key) => (
              <li key={key} className="flex items-center gap-1.5">
                <Ban className="size-3 text-success" />
                {t(key)}
              </li>
            ))}
          </ul>
        </div>

        {/* ★ 以下四块**只在「已开启」时出现**（原型 `proxyPanelHtml` :2964-2988 的开启态）：
            关闭态只给「风险 + 不劫持」声明，不摆白名单 / 业务路径 / 引导 —— 否则用户会以为
            「没开也在代理」，与「默认关」自相矛盾。 */}
        {status.enabled && (
          <>
            {/* ★ E1：开启态 KPI 三格（原型 `.kpi-row`）。
                ⚠️ 诚实渲染：`stats.known === false`（台账读不到 / 今天还没记过）⇒ 显示「—」，
                **不得**回落 0。「落盘正文」恒 0 —— 那是**承诺**（只记元数据），不是统计值。 */}
            <div className="grid grid-cols-3 gap-2" data-act="proxy-kpi">
              {(
                [
                  {
                    key: "proxy.kpi.requests",
                    value: status.stats?.known ? status.stats.todayRequests : null,
                  },
                  {
                    key: "proxy.kpi.switches",
                    value: status.stats?.known ? status.stats.switches : null,
                  },
                  // 承诺项：不看 known，恒显「0（不落盘）」。
                  { key: "proxy.kpi.bodies", value: 0 as number | null },
                ] as const
              ).map((cell) => (
                <div key={cell.key} className="rounded-md border border-border px-3 py-2">
                  <p className="text-[11px] text-muted-foreground">{t(cell.key)}</p>
                  <p className="text-sm font-medium tabular-nums">
                    {cell.key === "proxy.kpi.bodies"
                      ? t("proxy.kpi.bodiesZero")
                      : cell.value === null
                        ? "—"
                        : cell.value}
                  </p>
                </div>
              ))}
            </div>

            {/* 拦截方式 + 网关地址（断言 3：开启后要看得见「拦截方式」） */}
            <div className="grid gap-2 text-xs sm:grid-cols-2">
          <div className="flex items-center justify-between rounded-md border border-border px-3 py-2">
            <span className="text-muted-foreground">{t("proxy.mode.label")}</span>
            <span className="font-medium">{t("proxy.mode.cRedirect")}</span>
          </div>
          <div className="flex items-center justify-between rounded-md border border-border px-3 py-2">
            <span className="text-muted-foreground">{t("proxy.gateway.label")}</span>
            <code className="font-mono">{status.gatewayBase}</code>
          </div>
        </div>

        {/* 白名单表 + 兜底行（断言 3 / 4） */}
        <div className="space-y-1.5">
          <p className="text-xs font-medium">{t("proxy.domains.title")}</p>
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-muted-foreground">
                <th className="py-1 font-normal">{t("proxy.domains.colDomain")}</th>
                <th className="py-1 font-normal">{t("proxy.domains.colAction")}</th>
              </tr>
            </thead>
            <tbody>
              {status.domains.map((d) => (
                <tr key={d} className="border-t border-border">
                  <td className="py-1 font-mono">{d}</td>
                  <td className="py-1">{t("proxy.domains.actionProxied")}</td>
                </tr>
              ))}
              {/* ★ 兜底行：**直连**（不是「也代理」） */}
              <tr
                className="border-t border-border text-muted-foreground"
                data-act="proxy-fallback"
              >
                <td className="py-1">{t("proxy.domains.fallback")}</td>
                <td className="py-1">{t("proxy.domains.actionDirect")}</td>
              </tr>
            </tbody>
          </table>
        </div>

        {/* P12 T03：网关**实际转发**的业务路径 —— **诊断信息**（与网关的 404 判定同一份来源，
            故面板显示的与网关执行的**不会漂移**）。⚠️ 它不是「域名白名单」的替代品。 */}
        <div className="space-y-1.5">
          <p className="text-xs font-medium">{t("proxy.bizPaths.title")}</p>
          <ul className="flex flex-wrap gap-1.5">
            {status.bizPaths.map((p) => (
              <li
                key={p}
                className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground"
              >
                {p}
              </li>
            ))}
          </ul>
          <p className="text-xs text-muted-foreground">{t("proxy.bizPaths.hint")}</p>
        </div>

        {/* 引导（★ 如实：需手动设 + 需重启生效） */}
        <div className="space-y-2 rounded-lg border border-dashed border-border p-3">
          <p className="text-xs font-medium">{t("proxy.guide.title")}</p>
          <p className="text-xs text-muted-foreground">{t("proxy.guide.why")}</p>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1 font-mono text-[11px]">
              "{status.setting.key}": "{status.setting.value}"
            </code>
            <DemoAction>
              <Button variant="outline" size="sm" onClick={() => void copyEnv()}>
                {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                {copied ? t("proxy.guide.copied") : t("proxy.guide.copy")}
              </Button>
            </DemoAction>
          </div>
          <p className="text-xs text-muted-foreground">{t("proxy.guide.restart")}</p>
        </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
