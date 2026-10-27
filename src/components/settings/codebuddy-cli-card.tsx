import { useCallback, useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import type { CodeBuddyCliStatus } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * CodeBuddy CLI 的**接入方式**（issue #109）。
 *
 * ## 这不是「一个功能入口」，而是凭据通道的二选一
 *
 * CLI 的凭据只有一份（`~/.codebuddy/settings.json`），两条路都要写它：
 *   · **直连上游**：本工具写上游 token（`CODEBUDDY_AUTH_TOKEN` / `apiKeyHelper`），
 *     选号由本工具的自动轮换决定（按账号、按积分到期）；
 *   · **走本机网关**：本工具写网关 Key（`CODEBUDDY_BASE_URL` / `CODEBUDDY_API_KEY`），
 *     选号由网关按模型决定（一个模型限流只影响它自己）。
 *
 * 混用 ⇒ CLI 把上游 token 发到本机网关 ⇒ 401；轮换每轮又重写一次 ⇒ **时好时坏**。
 * 后端从 PR #114 起就把它当硬约束拦着（`ensure_not_gateway_mode`），这里只是让
 * 用户**在点之前**就看见。
 *
 * ## 为什么是「状态条 + 两张模式卡」，而不是一个分段器
 *
 * 分段器（`.seg`）表达的是「同一个东西的不同取值」（主题：深 / 浅），可随时来回切、
 * 零代价。而这里是**两套凭据通道**，切换会清 token、关轮换、要重启 CLI 才生效：
 *   ① 视觉权重必须更重；
 *   ② 用户选哪条路的依据是**能力差异**（按账号切 vs 按模型切），分段器只能写名字，
 *      差异只能藏进说明文字 ⇒ 等于让人先读文档再选；
 *   ③ 打开这页的第一个问题是「我现在是什么状态、生效了吗」⇒ 状态条置顶。
 *
 * 这三条判断与 `docs/prototype/buddy-switch-prototype.html` 里拍过的稿子一致（PR #117）。
 */

type Mode = "direct" | "gateway";

/** 一行能力（✓ / —）。两卡条数必须一致，否则没法横着比。 */
interface Cap {
  text: string;
  yes: boolean;
}

/** 底部提示：切换成功/失败就地显示，不引 toast（这块只有一个动作，就地更清楚）。 */
type Notice = { tone: "ok" | "err"; text: string } | null;

export function CodebuddyCliCard() {
  const t = useT();
  const [status, setStatus] = useState<CodeBuddyCliStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [wizard, setWizard] = useState<Mode | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  /* 切换后 env 不热重载 ⇒ 标「待重启」。刷新页面后回到「已生效」是**有意的**：
     我们无法知道用户是否重启过 CLI，只能在他刚点完的那一刻提醒。 */
  const [pendingRestart, setPendingRestart] = useState(false);

  const load = useCallback(async () => {
    try {
      const next = await api.getCodebuddyCliStatus();
      setStatus(next);
      setLoadError(null);
    } catch (error) {
      setLoadError(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const gatewayUrl = status?.gatewayMode ?? null;
  const gateway = Boolean(gatewayUrl);
  /* 冲突 = settings 已指向网关，但**本工具没在管**它（例如用户手工改过 / 另一套工具写的）。
     此时「直连上游」必须禁用 —— 但后端也会拒，所以这里只是让用户提前知道。 */
  const conflict = Boolean(gatewayUrl) && !gateway;

  const caps = (mode: Mode): Cap[] =>
    mode === "direct"
      ? [
          { text: t("wbSettings.cli.cap.directAccount"), yes: true },
          { text: t("wbSettings.cli.cap.directRotate"), yes: true },
          { text: t("wbSettings.cli.cap.modelSwitch"), yes: false },
        ]
      : [
          { text: t("wbSettings.cli.cap.modelSwitch"), yes: true },
          { text: t("wbSettings.cli.cap.usage"), yes: true },
          { text: t("wbSettings.cli.cap.gatewayAccount"), yes: false },
        ];

  async function submit(target: Mode) {
    setBusy(true);
    setNotice(null);
    try {
      if (target === "gateway") {
        const result = await api.enableCodebuddyCliGateway({});
        if (!result.ok) throw new Error(result.error ?? t("wbSettings.cli.toastFailed"));
        setNotice({ tone: "ok", text: t("wbSettings.cli.toastGatewayDesc") });
      } else {
        const result = await api.disableCodebuddyCliGateway();
        if (!result.ok) throw new Error(result.error ?? t("wbSettings.cli.toastFailed"));
        /* 切回直连**不会**自动重开轮换（自动开启后台任务比自动关闭危险）⇒ 由这里说明。 */
        setNotice({
          tone: "ok",
          text: result.rotateDisabled
            ? `${t("wbSettings.cli.toastDirectDesc")} · ${t("wbSettings.cli.rotateLabel")}: ${t("wbSettings.cli.rotateOff")}`
            : t("wbSettings.cli.toastDirectDesc"),
        });
      }
      setPendingRestart(true);
      await load();
    } catch (error) {
      setNotice({
        tone: "err",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setBusy(false);
      setWizard(null);
    }
  }

  /* ── 状态条：先回答「我现在是什么状态 / 生效了吗」 ── */
  const strip = conflict ? (
    <div
      className="flex items-center gap-3 rounded-xl border border-warning/40 bg-warning/10 px-3.5 py-3"
      role="status"
    >
      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] font-semibold">
          {t("wbSettings.cli.conflictTitle", { url: gatewayUrl ?? "" })}
        </p>
        <p className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">
          {t("wbSettings.cli.conflictBody")}
        </p>
      </div>
      <Button size="sm" onClick={() => setWizard("gateway")} disabled={busy}>
        {t("wbSettings.cli.conflictAction")}
      </Button>
    </div>
  ) : (
    <div
      className={cn(
        "flex items-center gap-3 rounded-xl border px-3.5 py-3",
        gateway ? "border-primary/40 bg-primary/5" : "border-success/40 bg-success/5",
      )}
      role="status"
    >
      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] font-semibold">
          {gateway ? t("wbSettings.cli.stripGateway") : t("wbSettings.cli.stripDirect")}
        </p>
        <p className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">
          {gateway
            ? t("wbSettings.cli.stripGatewaySub", { url: gatewayUrl ?? "" })
            : t("wbSettings.cli.stripDirectSub")}
        </p>
      </div>
      {pendingRestart ? (
        <Badge variant="outline" className="border-warning/40 text-warning" title={t("wbSettings.cli.pendingRestartTip")}>
          {t("wbSettings.cli.pendingRestart")}
        </Badge>
      ) : (
        <Badge variant="outline" className="border-success/40 text-success">
          {t("wbSettings.cli.effective")}
        </Badge>
      )}
    </div>
  );

  const modeCard = (mode: Mode) => {
    const on = gateway === (mode === "gateway");
    /* 冲突时「直连上游」禁用：再写上游 token 就是混用（后端也会拒）。 */
    const blocked = conflict && mode === "direct";
    return (
      <div
        key={mode}
        className={cn(
          "flex flex-col rounded-xl border p-3.5 transition-colors",
          on ? "border-primary/50 bg-primary/5" : "border-border bg-muted/30",
          blocked && "opacity-60",
        )}
        data-mode-card={mode}
        data-active={on ? "true" : "false"}
      >
        <div className="flex items-start gap-2.5">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <span className="text-[13px] font-semibold">
                {mode === "direct" ? t("wbSettings.cli.mode.direct") : t("wbSettings.cli.mode.gateway")}
              </span>
              {!on && mode === "gateway" ? (
                <Badge variant="secondary" className="text-[10px]">
                  {t("wbSettings.cli.recommended")}
                </Badge>
              ) : null}
            </div>
            <p className="mt-0.5 text-[11.5px] text-muted-foreground">
              {mode === "direct"
                ? t("wbSettings.cli.mode.directTagline")
                : t("wbSettings.cli.mode.gatewayTagline")}
            </p>
          </div>
          {on ? (
            <Badge variant="outline" className="shrink-0 border-success/40 text-success">
              {t("wbSettings.cli.current")}
            </Badge>
          ) : null}
        </div>

        <ul className="mt-3 flex flex-col gap-1.5">
          {caps(mode).map((cap) => (
            <li
              key={cap.text}
              className={cn(
                "flex items-start gap-1.5 text-[11.5px] leading-relaxed",
                cap.yes ? "text-foreground" : "text-muted-foreground/70",
              )}
            >
              <span className={cn("mt-px shrink-0 font-mono", cap.yes ? "text-success" : "text-muted-foreground/60")}>
                {cap.yes ? "✓" : "—"}
              </span>
              <span>{cap.text}</span>
            </li>
          ))}
        </ul>

        <div className="mt-auto pt-3.5">
          {on ? (
            <Button variant="outline" size="sm" className="w-full" disabled>
              {t("wbSettings.cli.enabled")}
            </Button>
          ) : (
            <Button
              size="sm"
              variant={mode === "gateway" ? "default" : "outline"}
              className="w-full"
              disabled={busy || blocked}
              onClick={() => setWizard(mode)}
            >
              {t("wbSettings.cli.switchTo")}
            </Button>
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="space-y-4">
      {strip}

      <section className="rounded-xl border border-border bg-card">
        <div className="flex items-center gap-2 border-b border-border px-4 py-3">
          <h3 className="text-[13.5px] font-semibold">{t("wbSettings.cli.sectionTitle")}</h3>
          <div className="grow" />
          <span className="text-[11px] text-muted-foreground">{t("wbSettings.cli.sectionHint")}</span>
        </div>
        <div className="grid gap-3 p-4 md:grid-cols-2">
          {modeCard("direct")}
          {modeCard("gateway")}
        </div>
      </section>

      <section className="rounded-xl border border-border bg-card">
        <button
          type="button"
          className="flex w-full items-center gap-2 px-4 py-3 text-left text-[12.5px] font-medium hover:bg-muted/40"
          onClick={() => setAdvanced((value) => !value)}
          aria-expanded={advanced}
        >
          <span className="w-3 text-muted-foreground">{advanced ? "▾" : "▸"}</span>
          {t("wbSettings.cli.advanced")}
          <span className="text-[11px] font-normal text-muted-foreground">{t("wbSettings.cli.advancedHint")}</span>
        </button>
        {advanced ? (
          <div className="border-t border-border">
            <div className="flex items-center gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <p className="text-[12.5px] font-medium">{t("wbSettings.cli.modelLabel")}</p>
                <p className="mt-0.5 text-[11.5px] text-muted-foreground">{t("wbSettings.cli.modelDesc")}</p>
              </div>
              {/* 模型清单暂只给「留空」：真实清单要读 `/v1/models`（按启用的命名空间），
                  那是网关页已有的数据，这里不重复拉一遍 —— 想固定模型请到网关页确认模型 id 后
                  用 CLI 自己的配置，或等这一项接上清单。 */}
              <span className="text-[11.5px] text-muted-foreground">{t("wbSettings.cli.modelBlank")}</span>
            </div>
            <div className="flex items-center gap-3 border-t border-border px-4 py-3">
              <div className="min-w-0 flex-1">
                <p className="text-[12.5px] font-medium">{t("wbSettings.cli.keyLabel")}</p>
                <p className="mt-0.5 text-[11.5px] text-muted-foreground">
                  {gateway ? t("wbSettings.cli.keyDescGateway") : t("wbSettings.cli.keyDescDirect")}
                </p>
              </div>
              <span className="font-mono text-[11.5px] text-muted-foreground">
                {gateway ? "sk-wb-••••••••" : "—"}
              </span>
            </div>
            {gateway ? (
              <div className="flex items-center gap-3 border-t border-border px-4 py-3">
                <div className="min-w-0 flex-1">
                  <p className="text-[12.5px] font-medium">{t("wbSettings.cli.rotateLabel")}</p>
                  <p className="mt-0.5 text-[11.5px] text-muted-foreground">
                    {t("wbSettings.cli.rotateDescGateway")}
                  </p>
                </div>
                <Badge variant="outline" className="border-warning/40 text-warning">
                  {t("wbSettings.cli.rotateOff")}
                </Badge>
              </div>
            ) : null}
          </div>
        ) : null}
      </section>

      {loadError ? (
        <p className="text-[11.5px] text-destructive">
          {t("wbSettings.cli.loadFailed")}：{loadError}
        </p>
      ) : null}
      {notice ? (
        <p
          className={cn(
            "rounded-lg border px-3 py-2 text-[11.5px]",
            notice.tone === "ok"
              ? "border-success/40 bg-success/10 text-success"
              : "border-destructive/40 bg-destructive/10 text-destructive",
          )}
          role="status"
        >
          {notice.text}
        </p>
      ) : null}

      {/* 切换向导：先列代价，再执行。低频高影响，值得多一层。 */}
      <Dialog open={wizard !== null} onOpenChange={(open) => (open ? null : setWizard(null))}>
        <DialogContent className="max-w-[460px]">
          <DialogHeader>
            <DialogTitle>
              {wizard === "gateway"
                ? t("wbSettings.cli.wizardGatewayTitle")
                : t("wbSettings.cli.wizardDirectTitle")}
            </DialogTitle>
            <DialogDescription>{t("wbSettings.cli.wizardIntro")}</DialogDescription>
          </DialogHeader>
          <ul className="list-disc space-y-1.5 pl-5 text-[12px] leading-relaxed">
            <li>
              {wizard === "gateway"
                ? t("wbSettings.cli.wizardGatewayCost1")
                : t("wbSettings.cli.wizardDirectCost1")}
            </li>
            <li>
              {wizard === "gateway"
                ? t("wbSettings.cli.wizardGatewayCost2")
                : t("wbSettings.cli.wizardDirectCost2")}
            </li>
            <li>
              {wizard === "gateway"
                ? t("wbSettings.cli.wizardGatewayCost3")
                : t("wbSettings.cli.wizardDirectCost3")}
            </li>
          </ul>
          <p className="rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-[11.5px] leading-relaxed">
            {t("wbSettings.cli.wizardRestart")}
            {wizard === "gateway" ? <span className="mt-1 block">{t("wbSettings.cli.wizardTokenKept")}</span> : null}
          </p>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setWizard(null)} disabled={busy}>
              {t("wbSettings.cli.wizardCancel")}
            </Button>
            <Button onClick={() => wizard && void submit(wizard)} disabled={busy}>
              {t("wbSettings.cli.wizardConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
