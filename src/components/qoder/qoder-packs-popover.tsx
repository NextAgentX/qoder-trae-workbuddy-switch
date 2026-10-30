/**
 * Qoder 逐资源包明细浮层（issue #111 · 两级显示的**第二级**）。
 *
 * ## 为什么是浮层而不是卡片第五块
 *
 * R6 的硬约束：**聚合指标行 + 浮层逐包**，两级并存、不互相替换。卡片是固定四块版式
 * （header → chips → body → footer），逐包列表塞进去会把每张卡撑高到不可扫读。
 *
 * ## 能力四态（浮层内容与触发的唯一判据）
 *
 * | 态 | 判据 | 界面 |
 * |:--|:--|:--|
 * | `unsupported` | 非桌面宿主（webui / headless） | 说明「仅桌面端」，**不给**登录按钮（按了也没用） |
 * | `no_session` | 桌面端但没登录过网页端 | 「未登录网页端」+ **登录**按钮 |
 * | `expired` | 有缓存但过期 / 上游 401 / 归属不符 | 「需要重新登录」+ **登录**按钮 |
 * | `ready` | 拿到逐包 | 逐包列表（含未到期 / 临期 / 已过期） |
 *
 * ★ 四态**都要给出下一步**：只写「没有数据」的浮层与空白等价。
 */

import { CircleAlert, Globe, Loader2, Package } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import * as api from "@/lib/api";
import type { QoderWebCreditPacks } from "@/lib/api";
import { demoModeEnabled } from "@/lib/demo-mode";
import { useT } from "@/lib/i18n";
import { packSourceLabel, packState } from "@/lib/qoder-packs";

export interface QoderPacksPopoverProps {
  /** `undefined` = 还没请求过；`null` = 请求过但没有结果（按未授权处理）。 */
  packs?: QoderWebCreditPacks | null;
  loading?: boolean;
  /** 打开时触发取数（页面持有缓存与 TTL）。 */
  onRequest?: () => void;
  /** 打开登录窗（仅桌面端传）。 */
  onWebLogin?: () => void;
  /**
   * 「我已登录，读取逐包明细」——**显式读取入口**（#111 复核 P0-2）。
   *
   * 为什么不能只有「登录」这一个动作：登录发生在另一个窗口，用户登录完直接关窗是
   * 最常见的序列；关窗后仍要有一条用户自己能走的路，而不是干等轮询或反复刷新。
   */
  onReadPacks?: () => void;
  /** 紧凑触发器（表格里用）。 */
  compact?: boolean;
}

/** 未拿到逐包时，按原因给出**下一步**（不是一句「暂无数据」）。 */
function reasonView(
  t: ReturnType<typeof useT>,
  reason: api.QoderPacksUnavailableReason | undefined,
): { title: string; hint: string; needLogin: boolean } {
  switch (reason) {
    case "no_session":
      return {
        title: t("qoder.page.accounts.web.state.noSession"),
        hint: t("qoder.page.accounts.web.state.noSessionHint"),
        needLogin: true,
      };
    case "session_expired":
      return {
        title: t("qoder.page.accounts.web.state.expired"),
        hint: t("qoder.page.accounts.web.state.expiredHint"),
        needLogin: true,
      };
    case "session_mismatch":
      return {
        title: t("qoder.page.accounts.web.state.mismatch"),
        hint: t("qoder.page.accounts.web.state.mismatchHint"),
        needLogin: true,
      };
    case "upstream":
      return {
        title: t("qoder.page.accounts.web.state.upstream"),
        hint: t("qoder.page.accounts.web.state.upstreamHint"),
        needLogin: false,
      };
    default:
      return {
        title: t("qoder.page.accounts.web.state.unknown"),
        hint: t("qoder.page.accounts.web.state.unknownHint"),
        needLogin: true,
      };
  }
}

export function QoderPacksPopover({
  packs,
  loading = false,
  onRequest,
  onWebLogin,
  onReadPacks,
  compact = false,
}: QoderPacksPopoverProps) {
  const t = useT();
  // ★ 形态门控在**前端也判一次**：headless 宿主根本没有 `Webview`，
  //   给一个能点但永远失败的按钮比不给更糟（后端也会拒，但那时用户已经点过了）。
  //
  // 演示站（`demoModeEnabled`）**按「桌面端」渲染**：演示数据里有完整的逐包，
  // 展示不出来的话这个页面等于没有该功能。真正的「不支持」分支由 webui 实机覆盖。
  const desktop = demoModeEnabled || !api.isWebui();
  const available = desktop && packs?.available === true;
  const count = packs?.summary?.count ?? 0;
  const remaining = packs?.summary?.remaining ?? 0;

  // ★ `packs === undefined` = **还没取过**，此时说「未登录」就是假信息（用户可能已登录）。
  //   三态必须分开：未知（中性标签）/ 在读 / 已知（有包或确实没登录）。
  const label = !desktop
    ? t("qoder.page.accounts.web.triggerUnsupported")
    : loading
      ? t("qoder.page.accounts.web.triggerLoading")
      : packs === undefined
        ? t("qoder.page.accounts.web.triggerNeutral")
        : available
          ? t("qoder.page.accounts.web.triggerPacks", { n: String(count) })
          : t("qoder.page.accounts.web.triggerNoSession");

  const view = reasonView(t, packs?.reason);

  return (
    <Popover
      onOpenChange={(open) => {
        // 打开即取数（页面按 TTL 决定要不要真发请求）。
        if (open) onRequest?.();
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          title={t("qoder.page.accounts.web.triggerHint")}
          className={cn(
            "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11.5px]",
            "text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
            compact && "text-[11px]",
          )}
        >
          <Package className="size-3" aria-hidden="true" />
          <span className="tabular-nums">{label}</span>
        </button>
      </PopoverTrigger>
      {/* ★ 浮层必须限高可滚（本仓硬约束）：不限高 + 内容长 ⇒ 被窗口切掉且没有滚动条。 */}
      <PopoverContent align="start" className="w-72 p-3">
        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="text-[11.5px] font-medium">{t("qoder.page.accounts.web.title")}</span>
          {available && packs?.summary ? (
            <span className="text-[11px] tabular-nums text-muted-foreground">
              {t("qoder.page.accounts.web.summary", {
                n: String(count),
                remaining: String(remaining),
              })}
            </span>
          ) : null}
        </div>

        {loading ? (
          <div className="flex items-center gap-2 py-3 text-[11.5px] text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" />
            {t("qoder.page.accounts.web.loading")}
          </div>
        ) : available ? (
          <ul className="max-h-[min(60vh,320px)] space-y-2 overflow-y-auto pr-1">
            {packs!.packs.map((pack, index) => {
              const state = packState(pack, Date.now());
              const expired = state === "expired";
              const expiringSoon = state === "expiring";
              const ratio =
                pack.limit > 0
                  ? Math.min(100, Math.max(0, (pack.remaining / pack.limit) * 100))
                  : 0;
              return (
                <li key={pack.id ?? `${pack.group}-${index}`} className="min-w-0">
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="min-w-0 truncate text-[11.5px]" title={pack.id}>
                      {packSourceLabel(t, pack.source)}
                    </span>
                    <span
                      className={cn(
                        "shrink-0 text-[10.5px] tabular-nums",
                        expired
                          ? "text-destructive"
                          : expiringSoon
                            ? "text-warning"
                            : "text-muted-foreground",
                      )}
                    >
                      {expired
                        ? t("qoder.page.accounts.web.expired")
                        : pack.expiresAtMs !== undefined
                          ? t("qoder.page.accounts.web.expiresAt", {
                              date: new Date(pack.expiresAtMs).toLocaleDateString(),
                            })
                          : t("qoder.page.accounts.web.neverExpires")}
                    </span>
                  </div>
                  <div className="mt-1 flex items-baseline justify-between gap-2 text-[11px]">
                    <span className="font-medium tabular-nums">
                      {pack.remaining} / {pack.limit}
                    </span>
                    <span className="text-muted-foreground tabular-nums">
                      {t("qoder.page.accounts.web.used", { n: String(pack.used) })}
                    </span>
                  </div>
                  <div className="mt-1 h-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
                    <div
                      className={cn(
                        "h-full rounded-full",
                        expired ? "bg-destructive/60" : expiringSoon ? "bg-warning/80" : "bg-primary/75",
                      )}
                      style={{ width: `${ratio}%` }}
                    />
                  </div>
                </li>
              );
            })}
          </ul>
        ) : (
          <div className="space-y-2 py-1">
            <div className="flex items-start gap-1.5 text-[11.5px]">
              <CircleAlert className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
              <div className="min-w-0">
                <div className="font-medium">
                  {desktop ? view.title : t("qoder.page.accounts.web.state.unsupported")}
                </div>
                <p className="mt-0.5 text-muted-foreground">
                  {desktop ? view.hint : t("qoder.page.accounts.web.state.unsupportedHint")}
                </p>
              </div>
            </div>
            {desktop && view.needLogin ? (
              <div className="space-y-1">
                {onWebLogin ? (
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 w-full gap-1.5"
                    onClick={onWebLogin}
                  >
                    <Globe className="size-3.5" />
                    {t("qoder.page.accounts.web.loginMenu")}
                  </Button>
                ) : null}
                {/* ★ 显式读取入口：用户已登录（或刚关掉登录窗）时不必再开一次窗。 */}
                {onReadPacks ? (
                  <button
                    type="button"
                    disabled={loading}
                    onClick={onReadPacks}
                    className="w-full rounded-md py-0.5 text-center text-[11px] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline disabled:opacity-50"
                  >
                    {t("qoder.page.accounts.web.read")}
                  </button>
                ) : null}
              </div>
            ) : null}
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
