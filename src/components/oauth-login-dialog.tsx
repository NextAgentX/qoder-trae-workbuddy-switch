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
import type { AccountMeta, Region } from "@/lib/types";
import { useAccountsStore } from "@/stores/accounts";
import { identityDisplayName } from "@/components/account-card";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 目标版本；缺省为国内版。 */
  region?: Region;
}

/** OAuth 扫码登录采集：发起 → 打开授权页（默认应用内）→ 轮询采集结果 → 入库。 */
export function OAuthLoginDialog({ open, onOpenChange, region }: Props) {
  const t = useT();
  const reconcileAccounts = useAccountsStore((s) => s.reconcileAccounts);
  /** 授权窗的命名键（仅用于 label / 诊断）：每次点「登录」都会开一个新窗口。 */
  const authKey = `wb-${region ?? "cn"}`;
  /** 本次授权窗的 label（关窗凭据）。★ 不能用 authKey 关窗 —— 它对应的是一类窗口，不是一个。 */
  const authLabelRef = useRef<string | null>(null);

  const [busy, setBusy] = useState(false);
  const [loginId, setLoginId] = useState<string | null>(null);
  const [uri, setUri] = useState("");
  /** 授权链接有效期（后端 `expiresIn`）—— 面板据此走倒计时。 */
  const [expiresIn, setExpiresIn] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [result, setResult] = useState<AccountMeta | null>(null);

  // 打开时重置
  useEffect(() => {
    if (open) {
      setBusy(false);
      setLoginId(null);
      setUri("");
      setExpiresIn(null);
      setError("");
      setResult(null);
    }
  }, [open]);

  /* 用户关掉弹窗 = 放弃这次登录 ⇒ 把授权窗一起收掉，别留孤儿窗口。 */
  useEffect(() => {
    if (open) return;
    void closeAuthPage(authLabelRef.current);
    authLabelRef.current = null;
  }, [open]);

  // 轮询采集结果
  useEffect(() => {
    if (!loginId) return;
    let timer: number | undefined;
    let cancelled = false;

    const poll = async () => {
      try {
        const res = await api.oauthStatus(loginId, region);
        if (res.done) {
          /* 令牌已经由后端轮询取回 ⇒ 授权窗的使命结束。
             不关的话用户会以为「还在等」，而且它挡在主窗口前面。 */
          void closeAuthPage(authLabelRef.current);
          authLabelRef.current = null;
          if (res.result) {
            await reconcileAccounts(region);
            if (!cancelled) setResult(res.result);
          } else if (!cancelled) {
            setError(res.error || t("wbAccounts.dialog.oauthLoginFail"));
          }
          if (timer !== undefined) window.clearInterval(timer);
          return;
        }
        timer = window.setTimeout(poll, 1500);
      } catch (e) {
        if (!cancelled) setError(api.asError(e));
        if (timer !== undefined) window.clearInterval(timer);
      }
    };
    poll();

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [authKey, loginId, reconcileAccounts, region]);

  async function start() {
    setBusy(true);
    setError("");
    try {
      const res = await api.oauthStart(region);
      setLoginId(res.loginId);
      setUri(res.verificationUri);
      setExpiresIn(res.expiresIn ?? null);
      /* 打开授权页是**尽力而为**：服务器 / 容器里没有 GUI，失败不能算登录失败 ——
         面板里的链接与二维码才是完整路径（issue #62）。
         桌面端默认走**应用内授权窗**（公共设置 `auth_in_app`），失败自动退回系统浏览器。
         ★ 每次发起都开**新窗**：上一轮那个（如果还在）先关掉，避免两个授权页叠着。 */
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
          <DialogTitle>{t("wbAccounts.dialog.oauthTitle")}</DialogTitle>
          <DialogDescription>
            {t("wbAccounts.dialog.oauthDesc")}
          </DialogDescription>
        </DialogHeader>

        {!loginId && !result && (
          <div className="space-y-3">
            <Button onClick={start} disabled={busy} className="w-full">
              {busy ? t("wbAccounts.dialog.oauthStarting") : t("wbAccounts.dialog.oauthStart")}
            </Button>
          </div>
        )}

        {loginId && !result && (
          /* 链接 + 二维码 + 倒计时：弹窗本身就是完整路径，不依赖「自动开浏览器」。 */
          <AuthLinkPanel
            url={uri}
            expiresIn={expiresIn}
            status={error ? "expired" : "waiting"}
          >
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              {t("shared.authPanel.noCallback")}
            </p>
          </AuthLinkPanel>
        )}

        {result && (
          <Alert>
            <AlertDescription>
              {t("wbAccounts.dialog.oauthCollected", { name: identityDisplayName(result) || result.id })}
            </AlertDescription>
          </Alert>
        )}

        {error && (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("wbAccounts.dialog.oauthClose")}
          </Button>
          {result && (
            <Button onClick={() => onOpenChange(false)}>{t("wbAccounts.dialog.oauthDone")}</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
