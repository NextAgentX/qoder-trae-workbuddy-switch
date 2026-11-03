/**
 * Qoder「逐资源包明细」（网页会话链路）的**唯一装配**（issue #144 复核 §4.5）。
 *
 * ## 为什么抽成 hook
 *
 * 这份数据只有桌面端拿得到，且要用户**在网页端登录一次**才能读到 —— 于是它有四件事
 * 必须一起做对：TTL 缓存、并发去重、`probe` 认领会话、登录窗的两段式轮询。
 * 这些此前只写在 Qoder 账号页里，聚合页只能「自己拉一次 `qoderWebCreditPacks`」——
 * 结果是**聚合页的抽屉拿不到引导**：逐包为空时用户只看到一个空白区块，不知道
 * 「登录一次就能看」，也没有任何按钮。
 *
 * 现在两个页面共用这一份 controller：状态、动作、以及抽屉下方的 `packsNotice` 文案。
 * 与 `credit-packs.ts` 的分工一致 —— **逻辑共用，文案按平台域给**。
 *
 * ## 三条不能省的细节（照抄自原实现的注释，别改坏）
 *
 * ① **并发去重**：浮层与抽屉可能同时触发同一个账号，逐包接口要带会话 Cookie，
 *    并发打两次上游没有意义；
 * ② **`probe` 与 `load` 是两步**：`load` 只读「已认领的会话」（渲染路径），
 *    `probe` 才是「把刚登录的会话认领下来」（#111 复核 §2.1：probe 曾无调用方）；
 * ③ **登录窗两段式轮询**：先每 2 秒问本地 `qoderWebWindowCookies`（不打上游），
 *    指纹变了才 `probe` 一次 —— 直接轮询 probe 会在未登录时打出一串注定 401 的请求，
 *    而 401 会立刻清缓存。
 */

import { useCallback, useMemo, useRef, useState } from "react";
import { toast } from "sonner";

import * as api from "@/lib/api";
import type { QoderWebCreditPacks } from "@/lib/api";
import type { Translate } from "@/lib/i18n";
import { PACKS_TTL_MS } from "@/lib/qoder-packs";
import type { Scope } from "@/lib/scope";

/**
 * 逐包控制器。
 *
 * ★ **每个动作都收 `scope`**（不是 hook 绑死一个作用域）：聚合页的抽屉跨 6 个作用域，
 * 账号页只有一个 —— 把 scope 钉进 hook 就等于「聚合页得开 6 份控制器」。缓存键也随之
 * 带作用域（`${scope}:${uid}`）：同一 `uid` 在国内版与国际版是两个账号。
 */
export interface QoderWebPacksController {
  /** 逐账号的逐包明细，键为 `${scope}:${uid}`（拿不到的账号不在表里，与「有 0 个包」不是一回事）。 */
  packs: Record<string, QoderWebCreditPacks>;
  /** 逐账号的读取中态（挡并发 + 抽屉按钮转圈），键同 `packs`。 */
  loading: Record<string, boolean>;
  /** 读已认领会话的逐包（带 TTL；`force` 绕过）。 */
  load: (scope: Scope, uid: string, force?: boolean) => Promise<void>;
  /** 「读取逐包明细」：先 probe 认领会话，再刷新该账号。返回是否真拿到。 */
  read: (scope: Scope, uid: string) => Promise<boolean>;
  /** 「退出网页登录」：清掉本机会话缓存。 */
  logout: (scope: Scope, uid?: string) => Promise<void>;
  /** 「登录 Qoder 网页端」：开独立窗口 + 两段式轮询认领。 */
  login: (scope: Scope, uid: string) => Promise<void>;
}

/** 缓存键：同一 `uid` 在两个区域是两个账号，键里必须带作用域。 */
export const qoderPackKey = (scope: Scope, uid: string) => `${scope}:${uid}`;

export function useQoderWebPacks(t: Translate): QoderWebPacksController {
  const [packs, setPacks] = useState<Record<string, QoderWebCreditPacks>>({});
  const [loading, setLoading] = useState<Record<string, boolean>>({});
  /** TTL 的时钟（不进 state：它变了不需要重渲染）。 */
  const fetchedAt = useRef<Record<string, number>>({});
  /* 回调里要读「当前」的缓存与加载态，但不能把它们放进依赖 —— 否则每次 setState 都会
     重建回调，调用方的 effect / memo 跟着失效。 */
  const packsRef = useRef(packs);
  packsRef.current = packs;
  const loadingRef = useRef(loading);
  loadingRef.current = loading;

  const setBusy = useCallback((uid: string, busy: boolean) => {
    setLoading((prev) => {
      const next = { ...prev };
      if (busy) next[uid] = true;
      else delete next[uid];
      return next;
    });
  }, []);

  const load = useCallback(
    async (scope: Scope, uid: string, force = false) => {
      if (!uid) return;
      const key = qoderPackKey(scope, uid);
      if (loadingRef.current[key]) return;
      const last = fetchedAt.current[key] ?? 0;
      if (!force && packsRef.current[key] && Date.now() - last < PACKS_TTL_MS) return;
      setBusy(key, true);
      try {
        const result = await api.qoderWebCreditPacks(scope, uid);
        fetchedAt.current[key] = Date.now();
        setPacks((prev) => ({ ...prev, [key]: result }));
      } catch {
        /* 静默：详情拿不到不是错误，抽屉会按「有没有区块」给出引导。 */
      } finally {
        setBusy(key, false);
      }
    },
    [setBusy],
  );

  /**
   * 探一次逐包接口：**认领**登录窗里的会话（带 uid 归属校验），成功即刷新该账号逐包。
   *
   * 返回 `true` = 真拿到了明细（调用方据此停止轮询 / 不再提示）。
   */
  const probeAndLoad = useCallback(
    async (scope: Scope, uid: string): Promise<boolean> => {
      const key = qoderPackKey(scope, uid);
      setBusy(key, true);
      try {
        const probe = await api.qoderWebCreditPacksProbe(scope, uid);
        if (!probe.hasSession) return false;
        if (probe.uidMatches === false) {
          // 归属不符：后端已丢弃该会话，界面必须说清楚「不是这个账号」，不能当成没登录。
          toast.error(t("qoder.page.accounts.web.readMismatch"));
          await load(scope, uid, true);
          return false;
        }
        await load(scope, uid, true);
        toast.success(t("qoder.page.accounts.web.readDone", { n: String(probe.packs?.length ?? 0) }));
        return true;
      } catch {
        /* 上游拒绝 / 网络失败：让抽屉按结构化原因给下一步，这里不重复报错。 */
        await load(scope, uid, true);
        return false;
      } finally {
        setBusy(key, false);
      }
    },
    [load, setBusy, t],
  );

  /**
   * 「我已登录，读取逐包明细」。
   *
   * 为什么必须有这个显式入口：登录是在**另一个窗口**里完成的，用户很自然会「登录完直接
   * 关窗」。自动轮询能覆盖大部分情况，但关窗那一刻的时序不保证；给一个用户能自己按的
   * 按钮，比让用户猜「是不是还要再等」强。
   */
  const read = useCallback(
    async (scope: Scope, uid: string) => {
      const ok = await probeAndLoad(scope, uid);
      if (!ok) toast.error(t("qoder.page.accounts.web.readFailed"));
      return ok;
    },
    [probeAndLoad, t],
  );

  /**
   * 「退出网页登录」（清除本机会话缓存）。
   *
   * 为什么必须有：会话一旦落盘，用户从界面没有任何办法清掉它（#111 复核 §2.6）。
   * 一份等价于完整身份凭据的 Cookie 不能只靠「删文件」才能退出。
   */
  const logout = useCallback(
    async (scope: Scope, uid?: string) => {
      try {
        await api.qoderWebSessionLogout(scope);
        setPacks({});
        fetchedAt.current = {};
        toast.success(t("qoder.page.accounts.web.logoutDone"));
        if (uid) await load(scope, uid, true);
      } catch (e) {
        toast.error(api.asError(e));
      }
    },
    [load, t],
  );

  /**
   * 「登录 Qoder 网页端」：开一个独立窗口让用户登录一次。
   *
   * 逐包明细只存在于网页端私有接口，只认浏览器会话 Cookie，而本仓 Qoder 的设备码登录
   * **从不产生网页会话**。登录态落在本地缓存后，以后取逐包就不必再登录。
   */
  const login = useCallback(
    async (scope: Scope, uid: string) => {
      try {
        await api.qoderWebLoginOpen(scope);
        toast.success(t("qoder.page.accounts.web.loginToast"));
      } catch (e) {
        toast.error(api.asError(e));
        return;
      }
      /* 基线指纹：开窗那一刻的 cookie 集合。之后「与它不同」= 登录态变了。 */
      const baseline = await api.qoderWebWindowCookies(scope).catch(() => null);
      let seen = baseline?.fingerprint ?? "";
      let probes = 0;
      /* 最多等 2 分钟（60 × 2s）：再长说明用户没打算现在登录，没必要一直转。 */
      for (let attempt = 0; attempt < 60; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 2000));
        const current = await api.qoderWebWindowCookies(scope).catch(() => null);
        if (!current) continue; // 单次失败不终止（窗口可能正处在导航中）
        if (!current.windowOpen) {
          /* 窗口被关了 —— 「登录完直接关窗」是最常见的操作序列。
             关窗前后端已兜底收割（`install_close_harvest`），此时 `pending` 为真才值得
             探一次；否则说明用户根本没登录，别白发一次注定 401 的请求。 */
          const status = await api.qoderWebSessionStatus(scope, uid).catch(() => null);
          if (status?.pending) await probeAndLoad(scope, uid);
          return;
        }
        if (!current.fingerprint || current.fingerprint === seen) continue;
        seen = current.fingerprint;
        probes += 1;
        if (await probeAndLoad(scope, uid)) return;
        /* 指纹变了但没认领成功（未登录的匿名 cookie / 别人的会话）：最多试 3 次，
           避免用户在这个窗口里反复登录时把上游打成筛子。 */
        if (probes >= 3) return;
      }
    },
    [probeAndLoad, t],
  );

  /* 稳定的对象身份：调用方会把它放进依赖数组（`qoderPacksNotice(t, ctrl, …)`），
     每次渲染新建一个对象会让下游 memo 恒失效。 */
  return useMemo(() => ({ packs, loading, load, read, logout, login }), [packs, loading, load, read, logout, login]);
}

/**
 * 抽屉「积分 / 资源包」区块下方的说明 + 一个动作（`packsNotice`）。
 *
 * ★ 两个页面共用这一份（issue #144 复核 §4.5）：此前只有 Qoder 账号页注入，聚合页
 * 的用户**只看到空白区块** —— 而空白既可能是「没登录」也可能是「真的没有包」。
 *
 * 三种情形：
 * - 已拿到逐包 ⇒ 说明「读了 N 个包」+ 「退出网页登录」（会话等价于身份凭据，必须能退）；
 * - WebUI ⇒ `null`（网页会话链路只有桌面端有，摆按钮是假控件）；
 * - 其余 ⇒ 说明「登录一次即可看逐包」+ 「读取逐包明细」。
 */
export function qoderPacksNotice(
  t: Translate,
  controller: QoderWebPacksController,
  scope: Scope,
  uid: string | undefined,
): { text: string; actionLabel: string; onAction: () => void; busy: boolean } | null {
  if (!uid) return null;
  const key = qoderPackKey(scope, uid);
  const web = controller.packs[key];
  if (web?.available) {
    return {
      text: t("qoder.page.accounts.web.packsNoticeReady", { n: String(web.packs.length) }),
      actionLabel: t("qoder.page.accounts.web.logout"),
      onAction: () => void controller.logout(scope, uid),
      busy: false,
    };
  }
  if (api.isWebui()) return null;
  return {
    text: t("qoder.page.accounts.web.packsNotice"),
    actionLabel: t("qoder.page.accounts.web.read"),
    onAction: () => void controller.read(scope, uid),
    busy: !!controller.loading[key],
  };
}
