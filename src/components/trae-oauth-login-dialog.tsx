import { useEffect, useRef, useState } from "react";
import { Check, Copy, Loader2, Rocket, TriangleAlert } from "lucide-react";

import { AuthLinkPanel } from "@/components/auth-link-panel";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Input } from "@/components/ui/input";
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
import { traeVariantLabel, type TraeAccount } from "@/lib/trae-types";
import { useTraeVariant } from "@/lib/use-trae-variant";

/**
 * 前端独立超时（秒）。
 *
 * 与后端 `TRAE_OAUTH_LOGIN_TIMEOUT_SECONDS`（300s）对齐并留 10s 余量：
 * 后端是最终裁决（它会真正释放端口），前端这一步只是**让用户看得见进度**，
 * 并在后端回调前先给出可操作提示，避免弹窗「像死住了一样」地干等到最后。
 * 取略大于后端，是为了让后端先超时、前端再兜底，两者不会互相打架。
 */
const TRAE_OAUTH_FRONTEND_TIMEOUT_SECONDS = 310;

/** 把剩余秒数格式化为 `M:SS`（如 272 → `4:32`）。 */

/**
 * Trae OAuth 登录（浏览器授权 + 本地回调监听）。
 *
 * ## 与 WorkBuddy 的 `OAuthLoginDialog` 是什么关系
 *
 * **交互骨架逐段照抄**：发起 → 展示验证链接并自动打开浏览器 → 每 1.5s 轮询 →
 * 成功后回调父级刷新 → 关闭。用户在两个分区之间切换时不需要重新学习。
 *
 * 差异只有两处，都是 Trae 侧客观事实决定的，不是设计选择：
 *
 * 1. **多一步「打开浏览器」是我们自己做的**。WorkBuddy 的 `verificationUri` 由
 *    服务端签发，其登录流程本身就会引导用户；Trae 的授权页是普通网页，
 *    必须由客户端打开。所以这里 `start()` 后立刻 `openInBrowser`，
 *    同时把链接展示出来作为兜底（弹窗被拦 / webui 无系统浏览器都能点）。
 * 2. **会话是「本机起监听」而非「服务端有状态」**。因此关闭对话框时要显式
 *    `traeOAuthCancel` 释放端口——WorkBuddy 侧没这个动作，会话在服务端自然过期。
 */
interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 登录成功后的回调（父级据此刷新账号列表）。 */
  onSuccess?: (account: TraeAccount) => void;
  /**
   * 本机没有该产品线的客户端（issue #62）。
   *
   * 有客户端时只需「端口转发」；没有客户端时还缺 icube 设备私钥，
   * 必须额外说清「把 storage.json 拷过来」这条路 —— 少这句用户会以为是网络问题。
   */
  clientless?: boolean;
}

export function TraeOAuthLoginDialog({ open, onOpenChange, onSuccess, clientless }: Props) {
  /**
   * 当前产品线 —— **与页面同源**（`?line=` 承载，见 `useTraeVariant`）。
   *
   * 刻意在这里自己取、而不是新增一个 prop：
   *
   * 1. 本弹窗被父级**无条件挂载**、只切 `open`，prop 需要父级每次都记得传对，
   *    漏传就会静默退回默认变体（这正是本次要修的缺陷形态）；
   * 2. 变体的唯一事实来源是 URL 查询串，这里再取一次**不可能**与页面不一致；
   * 3. 后端「按变体分家」是全链路的：授权页、账号库、数据目录都跟着它走，
   *    所以它必须由本组件直接持有，而不是靠调用方转述。
   */
  const [variant] = useTraeVariant();
  const variantLabel = traeVariantLabel(variant);
  /** 授权窗的命名键（仅用于 label / 诊断）：每次点「登录」都会开一个新窗口。 */
  const authKey = `trae-${variant}`;
  /** 本次授权窗的 label（关窗凭据）；不能用 authKey 关窗。 */
  const authLabelRef = useRef<string | null>(null);
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [loginId, setLoginId] = useState<string | null>(null);
  const [uri, setUri] = useState("");
  const [port, setPort] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [result, setResult] = useState<TraeAccount | null>(null);
  /** 正在启动客户端（与 `busy` 分开：启动与登录是两个独立动作，不该互相禁用）。 */
  const [launching, setLaunching] = useState(false);
  /** 启动客户端的结果提示（成功说清「还要再点一次登录」，失败说清原因）。 */
  const [launchNote, setLaunchNote] = useState("");
  /** 授权链接有效期（后端 `expiresIn`）—— 倒计时由 `AuthLinkPanel` 统一显示。 */
  const [expiresIn, setExpiresIn] = useState<number | null>(null);
  /** 手工粘贴的回调链接（远程部署：回调打不回本机 17388 时用）。 */
  const [callbackUrl, setCallbackUrl] = useState("");
  const [callbackBusy, setCallbackBusy] = useState(false);
  const [callbackError, setCallbackError] = useState("");
  /**
   * 是否已到前端超时。
   *
   * 用 `ref` 而非 state：它要在**定时器回调里立即读到最新值**（同一轮里既 stop
   * 轮询又置 error），用 state 会读到闭包里的旧值。它同时驱动倒计时与轮询的停摆。
   */
  const timedOutRef = useRef(false);

  /**
   * 是否已经落定（成功 / 失败 / 超时）。
   *
   * ## 为什么必须有它：终态是**一次性上报**的
   *
   * 后端 [`login_poll`] 在返回终态的**同一次调用**里就把会话从内存摘掉
   * （Rust 侧「轮询到终态后把会话摘掉」）。因此**再轮询一次**只会拿到
   * `{done:true, error:"登录请求不存在或已过期"}`。
   *
   * 前端只要多轮询一次，界面就会**同时**出现「已添加账号：X」与那句红字。
   * 实测触发路径：父级的 `onSuccess` 是内联箭头函数（每次渲染都是新引用），
   * 而它原先在轮询 effect 的依赖数组里 —— 登录成功后 `onSuccess` 里的
   * `loadAll()` 触发父级重渲染 → effect 重跑 → 立刻又轮询一次 → 撞上已被摘除的会话。
   *
   * 修法是两条一起上：① 回调进 ref、依赖里不放函数（见 `onSuccessRef`）；
   * ② 用本 ref 记「已落定」，**跨 effect 重跑也绝不再轮询**。
   * 只用其中一条也能挡住当前这条路径，但两条都留着，未来任何重跑都不会复现。
   */
  const settledRef = useRef(false);

  /**
   * `onSuccess` 的 ref 镜像：轮询 effect 的依赖里**不允许出现函数 prop**。
   *
   * 父级传的是内联箭头函数，引用每次渲染都变；放进依赖会让 effect 无谓重跑
   * （重跑的副作用见 `settledRef` 的说明）。用 ref 承接后，依赖只剩
   * `[open, loginId]` 这两个**真正决定会话身份**的值。
   */
  const onSuccessRef = useRef(onSuccess);
  useEffect(() => {
    onSuccessRef.current = onSuccess;
  }, [onSuccess]);

  // 打开时重置。**同时取消上一次遗留的会话**：用户在轮询中途关掉再打开时，
  // 上一个监听端口还开着；不取消就会泄漏端口，且旧会话超时后可能把
  // 用户后续的授权请求接走（用户会看到「授权成功但应用没反应」）。
  useEffect(() => {
    if (open) {
      setBusy(false);
      setLoginId(null);
      setUri("");
      setPort(null);
      setError("");
      setResult(null);
      setExpiresIn(null);
      setCallbackUrl("");
      setCallbackError("");
      setCallbackBusy(false);
      timedOutRef.current = false;
    }
  }, [open]);

  /* 用户关掉弹窗 = 放弃这次登录 ⇒ 把授权窗一起收掉，别留孤儿窗口。 */
  useEffect(() => {
    if (open) return;
    void closeAuthPage(authLabelRef.current);
    authLabelRef.current = null;
  }, [open]);

  // 轮询登录结果
  useEffect(() => {
    // 父组件是**无条件挂载**本弹窗、只切 `open`；`loginId` 只在 `open` 变 true
    // 时才重置。若此处不检查 `open`，用户点「关闭」后 `loginId` 仍非空，
    // 轮询会继续每 1.5s 打后端（后端此时返回 `done:true, error:"已取消"`，
    // 于是关闭状态下还在 `setError`）。因此关闭即停。
    if (!open || !loginId) return;
    // 已落定过就不再轮询。**这条判断正是缺陷入口的封堵**：effect 因依赖变化重跑时
    // 会立刻再发一次请求，而终态会话早已被后端摘掉，于是「成功」被一句
    // 「登录请求不存在或已过期」染红。见 `settledRef` 的注释。
    if (settledRef.current) return;
    let timer: number | undefined;
    let cancelled = false;

    const poll = async () => {
      // 已到前端超时就停止轮询：后端那条会话由它自己的超时兜底，
      // 这里继续轮询只会让界面在「已超时」的文案下偷偷转圈。
      if (timedOutRef.current || settledRef.current || cancelled) return;
      try {
        const res = await api.traeOAuthStatus(loginId);
        if (res.done) {
          // **先落定、再改界面**：此后任何 effect 重跑都会在入口处直接返回，
          // 不会再发第二次轮询（终端态只能被读一次）。
          settledRef.current = true;
          /* 令牌已经由后端回调/轮询取回 ⇒ 授权窗的使命结束（不关会挡住主窗口，
             也会让用户以为「还在等」）。 */
          void closeAuthPage(authLabelRef.current);
          authLabelRef.current = null;
          if (res.account) {
            // 同时校 `!timedOutRef.current`：若后端耗时超过前端 310s 才成功
            // （慢换 token / 时钟偏移），此时界面**已经**因超时置了 error。
            // 渲染层 `{result && …}` 与 `{error && …}` 是两块独立渲染，
            // 若在此 `setResult` 覆盖，会同时显示「已添加账号」与红色超时提示。
            // **刻意**忽略一个迟到的成功结果：用户已被告知失败，
            // 静默接受会让状态与提示自相矛盾。
            if (!cancelled && !timedOutRef.current) {
              setResult(res.account);
              onSuccessRef.current?.(res.account);
            }
          } else if (!cancelled && !timedOutRef.current) {
            setError(res.error || t("trae.comp.oauth.error.fallback"));
          }
          return;
        }
        timer = window.setTimeout(poll, 1500);
      } catch (e) {
        // 轮询本身不抛错（后端永不返 Err），能走到这里说明是传输层问题。
        if (!cancelled && !timedOutRef.current) {
          settledRef.current = true;
          setError(api.asError(e));
        }
      }
    };
    void poll();

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
    // 依赖里**只有**决定会话身份的两个值。`onSuccess` 走 ref（见 `onSuccessRef`），
    // 放进依赖会让父级每次重渲染都重跑本 effect —— 那正是本次修掉的缺陷。
  }, [authKey, open, loginId]);

  // 前端独立超时：会话开始时起算，到点给明确文案并停轮询。
  //
  // ★ 只留一个 `setTimeout`，不再维护「每秒 tick 的剩余秒数」：
  //   倒计时现在由 `AuthLinkPanel` 按后端 `expiresIn` 显示，
  //   两个倒计时会各走各的（面板说还剩 3 分钟、这里已经超时），
  //   而这里真正需要的只是「到点判定一次」。
  useEffect(() => {
    // 同轮询 effect：关闭弹窗后 `loginId` 仍非空，若不校 `open`，定时器会活到组件卸载。
    if (!open || !loginId || result) return undefined;
    const timer = window.setTimeout(() => {
      if (timedOutRef.current) return;
      timedOutRef.current = true;
      // 超时同样是终态：一并落下「已落定」，让轮询的语义只有一个判据。
      settledRef.current = true;
      setError(t("trae.comp.oauth.error.timeout", { seconds: TRAE_OAUTH_FRONTEND_TIMEOUT_SECONDS - 10 }));
    }, TRAE_OAUTH_FRONTEND_TIMEOUT_SECONDS * 1000);
    return () => window.clearTimeout(timer);
  }, [open, loginId, result, t]);

  /**
   * 关闭对话框：先取消会话再交给父级。
   *
   * 顺序不能反——先 `onOpenChange(false)` 会把组件卸载、`loginId` 随之丢失，
   * 那个监听端口就再也没人负责回收了。
   */
  async function close() {
    if (loginId && !result) {
      try {
        await api.traeOAuthCancel(loginId);
      } catch {
        // 取消失败不阻断关闭：后端会话有超时兜底，最坏情况是等它自己过期。
      }
    }
    onOpenChange(false);
  }

  /**
   * 启动当前产品线的 Trae 客户端（「取不到设备凭证」时的下一步动作）。
   *
   * 客户端**首次启动**才会把 icube 设备凭证写进 `storage.json`，而 OAuth 授权 URL
   * 的 `device_id` 必须与它同源（见 Rust 侧 `icube::device_identity_for` 的红线）。
   * ⇒ 客户端从没启动过时，登录**必然**失败，且用户无法靠自己点「重试」解决。
   *
   * ⚠️ **启动成功 ≠ 可以立刻登录**：客户端写出凭证需要时间，而且它大概率会弹登录页
   * 挡在前面。所以成功文案只说「已启动」，让用户自己再点一次登录 —— 不在这里替他
   * 自动重试（那会把一个不可控的时序当成确定事件，失败时更难解释）。
   */
  async function launchClient() {
    setLaunching(true);
    setLaunchNote("");
    try {
      await api.launchTraeClient(variant);
      setLaunchNote(t("trae.comp.oauth.launchOk", { variant: variantLabel }));
    } catch (e) {
      setLaunchNote(api.asError(e));
    } finally {
      setLaunching(false);
    }
  }

  async function start() {
    setBusy(true);
    setError("");
    // 重置超时标志与剩余时间：这是「重新发起」与「首次发起」共用的入口。
    // `settledRef` 必须一起重置 —— 否则「失败后重新发起」会因残留的"已落定"
    // 直接跳过轮询，新会话永远等不到结果。
    timedOutRef.current = false;
    settledRef.current = false;
    try {
      const res = await api.traeOAuthStart(variant);
      setLoginId(res.loginId);
      setUri(res.verificationUri);
      setPort(res.port);
      setExpiresIn(res.expiresIn ?? null);
      // 按公共设置打开验证页：桌面端默认内嵌授权窗，失败自动退回系统浏览器。
      // 打开失败**不是登录失败**（链接与二维码在弹窗里，远程部署照常可用）。
      // ★ 每次发起都开新窗：先关掉上一轮残留的那个。
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

  /**
   * 出错后重新发起：先取消旧会话（释放它占的端口），再走一次 [`start`]。
   *
   * 不能只调 `start`：旧会话的监听任务还在等回调，不取消会让端口泄漏，
   * 且旧会话超时后可能把新会话的回调请求接走。
   */
  async function retry() {
    if (loginId) {
      try {
        await api.traeOAuthCancel(loginId);
      } catch {
        // 取消失败不阻断重试：后端会话有超时兜底。
      }
    }
    setLoginId(null);
    setUri("");
    setPort(null);
    setResult(null);
    await start();
  }

  /**
   * 手工提交回调链接（远程部署，issue #62）。
   *
   * 与轮询不同：失败**必须**显示出来（`api.traeOAuthSubmitCallback` 会抛错），
   * 用户此刻正盯着这个动作，静默等待只会被当成「卡住」。
   */
  async function submitCallback() {
    if (!loginId) return;
    const url = callbackUrl.trim();
    if (!url) {
      setCallbackError(t("trae.comp.oauth.pasteCallback.placeholder", { port: port ?? 17388 }));
      return;
    }
    setCallbackBusy(true);
    setCallbackError("");
    try {
      const account = await api.traeOAuthSubmitCallback(loginId, url);
      settledRef.current = true;
      setResult(account);
      onSuccessRef.current?.(account);
    } catch (e) {
      setCallbackError(api.asError(e));
    } finally {
      setCallbackBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) void close();
        else onOpenChange(true);
      }}
    >
      <DialogContent>
        <DialogHeader>
          {/*
            标题里**必须**带产品线名：两条产品线的授权页、账号库、数据目录全都不同，
            而本弹窗的骨架与 WorkBuddy 的一致，用户很容易以为「点哪个都一样」。
            说清「正在为哪条线登录」是本次变体透传修复的**用户可见那一半** ——
            只把参数传对、却不告诉用户，用户仍然无法预期账号会落到哪里。
          */}
          <DialogTitle>{t("trae.comp.oauth.title", { variant: variantLabel })}</DialogTitle>
          <DialogDescription>
            {t("trae.comp.oauth.desc.lead")}{" "}
            <span className="font-medium text-foreground">{variantLabel}</span>{" "}
            {t("trae.comp.oauth.desc.mid")}{" "}
            <span className="font-medium text-foreground">{variantLabel}</span>
            {t("trae.comp.oauth.desc.tail")}
          </DialogDescription>
        </DialogHeader>

        {!loginId && !result && (
          <div className="space-y-3">
            <Button onClick={start} disabled={busy} className="w-full">
              {busy ? t("trae.comp.oauth.startBusy", { variant: variantLabel }) : t("trae.comp.oauth.start", { variant: variantLabel })}
            </Button>
          </div>
        )}

        {loginId && !result && !error && (
          <AuthLinkPanel url={uri} expiresIn={expiresIn} status="waiting">
            <TraeRemoteNote port={port} clientless={clientless} />
            {/* 端口转发之外的第二条路：把地址栏整串贴回来，由服务端补完兑换。 */}
            <div className="flex flex-col gap-2 rounded-[11px] border border-border bg-card-2 p-3">
              <b className="text-[12px] font-semibold">{t("trae.comp.oauth.pasteCallback.title")}</b>
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                {t("trae.comp.oauth.pasteCallback.hint", { port: port ?? 17388 })}
              </p>
              <div className="flex items-center gap-2">
                <Input
                  value={callbackUrl}
                  onChange={(e) => setCallbackUrl(e.target.value)}
                  placeholder={t("trae.comp.oauth.pasteCallback.placeholder", { port: port ?? 17388 })}
                  spellCheck={false}
                  autoComplete="off"
                  className="h-8 min-w-0 flex-1 font-mono text-[11px]"
                />
                <Button
                  type="button"
                  size="sm"
                  className="h-8 shrink-0 px-2.5 text-[11px]"
                  disabled={callbackBusy}
                  onClick={() => void submitCallback()}
                >
                  {callbackBusy ? <Loader2 className="size-3.5 animate-spin" /> : null}
                  {callbackBusy
                    ? t("trae.comp.oauth.pasteCallback.busy")
                    : t("trae.comp.oauth.pasteCallback.submit")}
                </Button>
              </div>
              {callbackError ? (
                <p className="break-all text-[11px] leading-relaxed text-destructive">{callbackError}</p>
              ) : null}
            </div>
          </AuthLinkPanel>
        )}

        {result && (
          <Alert>
            <AlertDescription>{t("trae.comp.oauth.result", { name: result.name || result.userId })}</AlertDescription>
          </Alert>
        )}

        {/*
          错误块**必须**与结果块互斥：`{error && …}` 单独渲染时，一次多余的轮询
          就能让界面同时出现「已添加账号：X」与红色报错（本次修掉的缺陷形态）。
          轮询侧已落定封堵，这里再互斥一次 —— 两块提示自相矛盾是最坏的用户体验，
          值得两道保险。重试按钮本来就用的 `error && !result`，这里与之对齐。
        */}
        {error && !result && (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        {/*
          「启动客户端」只在这种错误下出现：**没有回调端口** ⇒ 后端在取设备凭证那一步
          就失败了，还没走到绑端口。
          判据是**结构性的**（后端 `login_start_for` 先把身份拿到手，再绑固定端口），
          不是拿错误文案去匹配「未找到…数据目录」——那属于「代理断言」，
          文案一改就恒真，护栏却不会红（见验证纪律）。
          端口类 / 回调类错误时端口必已绑定（`port !== null`）⇒ 本按钮不出现，
          避免给出「启动客户端」这种错误建议。
        */}
        {error && !result && port === null && (
          <div className="space-y-2">
            <Button
              variant="outline"
              className="w-full"
              onClick={() => void launchClient()}
              disabled={launching}
            >
              <Rocket />
              {launching
                ? t("trae.comp.oauth.launchBusy", { variant: variantLabel })
                : t("trae.comp.oauth.launch", { variant: variantLabel })}
            </Button>
            {launchNote && (
              <p className="text-xs text-muted-foreground">{launchNote}</p>
            )}
          </div>
        )}

        {/* 出错/超时后仍把回调地址摆出来：这是排查「回调没打回本机」的唯一抓手。 */}
        {error && !result && port !== null && (
          <p className="text-xs text-muted-foreground break-all">
            {t("trae.comp.oauth.callback")}
            <code>http://127.0.0.1:{port}/authorize</code>
          </p>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => void close()}>
            {t("trae.comp.oauth.close")}
          </Button>
          {result && <Button onClick={() => void close()}>{t("trae.comp.oauth.done")}</Button>}
          {error && !result && (
            <Button onClick={() => void retry()} disabled={busy}>
              {busy ? t("trae.comp.oauth.retryBusy") : t("trae.comp.oauth.retry")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * 远程部署提示块（issue #62）—— 只在「没有客户端 / 浏览器不在本机」时才有意义。
 *
 * 两件事必须说清，少一件用户就会卡在「认证中」：
 *   ① 授权页会探 `127.0.0.1:<port>`（固定 17388，见 Rust 侧 `oauth::CALLBACK_PORT`），
 *      服务器上没有这个端口 ⇒ 给一条可直接复制的 `ssh -L`；
 *   ② 换 token 与续期都要客户端里的 icube **设备私钥**，本机没有客户端就没有这把钥匙
 *      ⇒ 只能把 `storage.json` 拷过来，或改用「导入备份」（备份不含私钥，续期会失败）。
 */
function TraeRemoteNote({ port, clientless }: { port: number | null; clientless?: boolean }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const listenPort = port ?? 17388;
  const cmd = `ssh -L ${listenPort}:127.0.0.1:${listenPort} user@your-server`;

  async function copy() {
    try {
      await navigator.clipboard.writeText(cmd);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // 非安全上下文里剪贴板会被拒；命令本身可选中复制。
    }
  }

  return (
    <div className="flex flex-col gap-2 rounded-[11px] border border-dashed border-warning/45 bg-warning/5 p-3">
      <div className="flex items-center gap-2 text-[12px] font-semibold">
        <TriangleAlert className="size-3.5 shrink-0 text-warning" aria-hidden="true" />
        <span>{t("trae.comp.oauth.tunnel.title", { port: listenPort })}</span>
      </div>
      <p className="text-[11.5px] leading-relaxed text-muted-foreground">
        {t("trae.comp.oauth.tunnel.desc", { port: listenPort })}
      </p>
      <div className="flex items-center gap-2 rounded-[9px] border border-border bg-background px-2.5 py-2">
        <code className="min-w-0 flex-1 overflow-x-auto whitespace-pre font-mono text-[11px]">{cmd}</code>
        <Button type="button" size="sm" variant="ghost" className="h-6 shrink-0 px-2 text-[11px]" onClick={() => void copy()}>
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? t("trae.comp.oauth.tunnel.copied") : t("trae.comp.oauth.tunnel.copy")}
        </Button>
      </div>
      {clientless ? (
        <p className="text-[11.5px] leading-relaxed text-muted-foreground">{t("trae.comp.oauth.keyNote")}</p>
      ) : null}
      <p className="break-all text-[11px] text-muted-foreground">
        {t("trae.comp.oauth.callback")}
        <code>http://127.0.0.1:{listenPort}/authorize</code>
      </p>
    </div>
  );
}
