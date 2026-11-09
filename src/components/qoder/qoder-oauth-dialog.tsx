/**
 * Qoder 设备授权登录弹窗。
 *
 * ## 为什么不是直接复用 `OAuthLoginDialog`（WB 那个）
 *
 * WB 的壳与 `useAccountsStore`（WB 的账号 store）**绑死**：它成功后调
 * `reconcileAccounts(region)` 去刷新 WB 账号库。Qoder 有自己的命令族与作用域参数，
 * 塞进去只会长出一堆平台分支。
 * 但**交互链刻意逐字同构**（发起 → 打开浏览器 → 每 1.5s 轮询 → 成功后回调刷新）——
 * 「同一件事同一做法」体现在**步骤与节奏**上，不是体现在共用同一个文件上。
 *
 * ## 授权 URL 由后端构造，前端**不拼参数**
 *
 * 双区域的 `client_id` / `redirect_uri` / nonce 形态都不同（国内带 `redirect_uri`、
 * 国际不带；nonce 一个带横线一个 32 位 hex）。前端拼一遍必然与后端漂移 ——
 * 这里只负责「把后端给的 URL 打开」。
 */

import { useEffect, useRef, useState } from "react";

import { AuthLinkPanel } from "@/components/auth-link-panel";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { closeAuthPage, openAuthPage } from "@/lib/auth-open";
import { useT } from "@/lib/i18n";
import type { Scope } from "@/lib/scope";

/** 轮询间隔（与 WB 的 `OAuthLoginDialog` 同值：1.5s）。 */
const POLL_INTERVAL_MS = 1500;

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 目标作用域（`qoder-cn` / `qoder-global`）—— 决定授权页与 client_id。 */
  scope: Scope;
  /** 采集成功后回调（页面据此重取账号列表）。 */
  onCollected?: (name: string) => void;
}

export function QoderOAuthDialog({ open, onOpenChange, scope, onCollected }: Props) {
  const t = useT();
  /** 授权窗的命名键（仅用于 label / 诊断）：每次点「登录」都会开一个新窗口。 */
  const authKey = `qoder-${scope}`;
  /** 本次授权窗的 label（关窗凭据）；不能用 authKey 关窗。 */
  const authLabelRef = useRef<string | null>(null);

  const [busy, setBusy] = useState(false);
  const [loginId, setLoginId] = useState<string | null>(null);
  const [uri, setUri] = useState("");
  /** 授权链接有效期（后端 `expiresIn`）—— 面板据此走倒计时。 */
  const [expiresIn, setExpiresIn] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [collectedName, setCollectedName] = useState<string | null>(null);

  /** 打开即复位：上一次的 loginId / 结果不得泄漏到下一次。 */
  useEffect(() => {
    if (!open) return;
    setBusy(false);
    setLoginId(null);
    setUri("");
    setExpiresIn(null);
    setError("");
    setCollectedName(null);
  }, [open]);

  /* 用户关掉弹窗 = 放弃这次登录 ⇒ 把授权窗一起收掉，别留孤儿窗口。 */
  useEffect(() => {
    if (open) return;
    void closeAuthPage(authLabelRef.current);
    authLabelRef.current = null;
  }, [open]);

  /** 轮询：只在拿到 loginId 后启动，`done` 即停。 */
  useEffect(() => {
    if (!open || !loginId) return;
    let timer: number | undefined;
    let cancelled = false;

    const poll = async () => {
      try {
        const res = await api.qoderOauthStatus(scope, loginId);
        if (cancelled) return;
        if (res.done) {
          /* 令牌已入库 ⇒ 关掉授权窗（不关会挡住主窗口，也让用户以为还在等）。 */
          void closeAuthPage(authLabelRef.current);
          authLabelRef.current = null;
          if (res.result) {
            // ★ 展示名走卡片的**唯一回落链**（`qoderAccountName`），与列表 / 抽屉同一份。
            const name = res.result.nickname || res.result.name || res.result.uid || "";
            setCollectedName(name);
            onCollected?.(name);
          } else {
            setError(res.error || t("qoder.page.accounts.oauth.failed"));
          }
          return;
        }
        timer = window.setTimeout(poll, POLL_INTERVAL_MS);
      } catch (e) {
        if (!cancelled) setError(api.asError(e));
      }
    };
    void poll();

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [authKey, open, loginId, scope, onCollected, t]);

  async function start() {
    setBusy(true);
    setError("");
    try {
      const res = await api.qoderOauthStart(scope);
      setLoginId(res.loginId);
      setUri(res.verificationUri);
      setExpiresIn(res.expiresIn ?? null);
      /* 尽力而为：容器里没有 GUI，开不起来不算失败 —— 链接与二维码才是完整路径。
         桌面端默认走**应用内授权窗**（公共设置 `auth_in_app`），失败自动退回系统浏览器。
         ★ 每次发起都开新窗：先关掉上一轮残留的那个。 */
      void closeAuthPage(authLabelRef.current);
      authLabelRef.current = null;
      void openAuthPage(authKey, res.verificationUri)
        .then((label) => {
          authLabelRef.current = label;
        })
        .catch(() => {});
    } catch (e) {
      setError(api.asError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("qoder.page.accounts.oauth.title")}</DialogTitle>
          <DialogDescription>{t("qoder.page.accounts.oauth.description")}</DialogDescription>
        </DialogHeader>

        {!loginId && !collectedName ? (
          <Button onClick={() => void start()} disabled={busy} className="w-full">
            {busy ? t("qoder.page.accounts.oauth.starting") : t("qoder.page.accounts.oauth.start")}
          </Button>
        ) : null}

        {loginId && !collectedName ? (
          /* 链接 + 二维码 + 倒计时：与 WB / Trae 同一套面板，弹窗本身即完整路径。 */
          <AuthLinkPanel url={uri} expiresIn={expiresIn} status={error ? "expired" : "waiting"}>
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              {t("shared.authPanel.noCallback")}
            </p>
          </AuthLinkPanel>
        ) : null}

        {collectedName ? (
          <Alert>
            <AlertDescription>
              {t("qoder.page.accounts.oauth.collected", { name: collectedName })}
            </AlertDescription>
          </Alert>
        ) : null}

        {error ? (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("qoder.page.accounts.oauth.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
