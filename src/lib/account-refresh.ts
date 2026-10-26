"use client";

import { useCallback, useRef, useState } from "react";

/**
 * 账号「单项刷新」的共用契约（三平台卡片 + 详情抽屉都按这一套来）。
 *
 * ## 为什么是**四项**而不是一个「刷新全部」
 *
 * 四类数据来自**不同层**，耗时与失败原因都不一样：
 *
 * | 项 | 数据来源 | 特点 |
 * |:--|:--|:--|
 * | `credits` | 上游接口（查剩余积分 / 到期） | 最慢，可能被限流 |
 * | `tokens` | 本机客户端日志 + 网关日志 | 快，离线可读 |
 * | `checkin` | 本机签到记录 | 快 |
 * | `credential` | 本机凭据（JWT 有效期） | 最快，且**凭据失效时只有它能给出答案** |
 *
 * 合成一个动作时，慢的拖住快的，而且失败原因分不清 —— 凭据过期的账号其实只有
 * 「凭据状态」能刷出真值，另外三项必然失败。拆开之后用户能只刷要用的那项，
 * 也能从失败项里看出卡在哪一层（判据见 `docs/specs/PLAN.md` §3.3）。
 *
 * ## 为什么按「账号 + 项」记进行中，而不是一个全局布尔
 *
 * 列表里可能同时有多个账号在刷，同一个账号也可能同时刷两项（比如先刷凭据再刷积分）。
 * 用一个布尔会让后开始的把先开始的转圈吞掉；用一个 `Set<accountId>` 又会让
 * 「点了积分，四项一起转」。所以键是 `id:kind`。
 */
export type AccountRefreshKind = "credits" | "tokens" | "checkin" | "credential";

/** 固定顺序：菜单分组与指标块都从这里取，避免两处各写一份顺序。 */
export const ACCOUNT_REFRESH_KINDS: readonly AccountRefreshKind[] = [
  "credits",
  "tokens",
  "checkin",
  "credential",
] as const;

/**
 * Qoder 的可用刷新项：**没有 `tokens`**。
 *
 * Qoder 没有 token 概念（见 `qoder::usage` 模块头），摆一个「刷新今日 Token」
 * 在它上面点了只能重读缓存 —— 比不摆更坏。
 *
 * ## ★ 为什么放在这里而不是页面内（issue #89 · D1）
 *
 * 渠道页与聚合页**两处都要传**，各写一份必然漂移：聚合页漏传 ⇒ 回落到
 * [`ACCOUNT_REFRESH_KINDS`] 的四项 ⇒ 菜单里多出一项「刷新今日 Token」，
 * 点了什么也不发生。常量只有一个来源，漏传才会被 `tsc`/review 抓住。
 */
export const QODER_REFRESH_KINDS: readonly AccountRefreshKind[] = [
  "credits",
  "checkin",
  "credential",
];

const keyOf = (accountId: string, kind: AccountRefreshKind) => `${accountId}:${kind}`;

/**
 * 四项各自的成功文案键。
 *
 * 写死成字面量是为了让 `t()` 的**编译期键校验**生效（拼错 key 会直接报错，而不是静默回退成 key 本身）；
 * 也避免三个页面各自写一份「已刷新」的措辞 —— 用户需要知道刷的是哪一层。
 */
export const REFRESH_DONE_LABEL = {
  credits: "shared.refreshPart.credits",
  tokens: "shared.refreshPart.tokens",
  checkin: "shared.refreshPart.checkin",
  credential: "shared.refreshPart.credential",
} as const;

/**
 * 进行中态的小 hook。
 *
 * 用法：
 * ```tsx
 * const refresh = useAccountRefreshBusy();
 * const onRefreshPart = (a, kind) => refresh.run(a.id, kind, () => api.xxx(...));
 * <AccountCard refreshingKinds={refresh.kindsOf(a.id)} onRefreshPart={onRefreshPart} />
 * ```
 *
 * ⚠️ `kindsOf` 每次返回**新数组**：调用方不要把它放进 `useMemo`/`useEffect` 依赖里
 * （会每次重算）。卡片只需要读它来决定哪一项显示转圈。
 */
/**
 * 「已更新」的停留时长（ms）。
 *
 * ⚠️ 它是**刚刚发生了什么**的反馈，不是常驻状态（原型同样只留 900ms）：
 * 常驻会让用户以为「这个值只在我点的那一刻是对的」。
 */
export const REFRESH_DONE_MS = 900;

export function useAccountRefreshBusy() {
  const [busy, setBusy] = useState<Map<string, Set<AccountRefreshKind>>>(() => new Map());
  const [done, setDone] = useState<Map<string, Set<AccountRefreshKind>>>(() => new Map());
  /** 定时器句柄：同一项重复刷新时先撤掉旧的，避免「早就撤了却被旧定时器清了新状态」。 */
  const doneTimers = useRef(new Map<string, number>());

  const setOne = useCallback((accountId: string, kind: AccountRefreshKind, on: boolean) => {
    setBusy((prev) => {
      const current = prev.get(accountId);
      if (on) {
        if (current?.has(kind)) return prev;
        const next = new Map(prev);
        const set = new Set(current ?? []);
        set.add(kind);
        next.set(accountId, set);
        return next;
      }
      if (!current?.has(kind)) return prev;
      const next = new Map(prev);
      const set = new Set(current);
      set.delete(kind);
      if (set.size === 0) next.delete(accountId);
      else next.set(accountId, set);
      return next;
    });
  }, []);

  const kindsOf = useCallback(
    (accountId: string): AccountRefreshKind[] => {
      const set = busy.get(accountId);
      return set ? [...set] : [];
    },
    [busy],
  );

  const isBusy = useCallback(
    (accountId: string, kind: AccountRefreshKind): boolean => busy.get(accountId)?.has(kind) ?? false,
    [busy],
  );

  /** 置 / 清「已更新」。 */
  const markDone = useCallback((accountId: string, kind: AccountRefreshKind, on: boolean) => {
    setDone((prev) => {
      const current = prev.get(accountId);
      if (on) {
        const set = new Set(current ?? []);
        set.add(kind);
        const next = new Map(prev);
        next.set(accountId, set);
        return next;
      }
      if (!current?.has(kind)) return prev;
      const next = new Map(prev);
      const set = new Set(current);
      set.delete(kind);
      if (set.size === 0) next.delete(accountId);
      else next.set(accountId, set);
      return next;
    });
  }, []);

  const doneKindsOf = useCallback(
    (accountId: string): AccountRefreshKind[] => {
      const set = done.get(accountId);
      return set ? [...set] : [];
    },
    [done],
  );

  /**
   * 跑一次单项刷新：先标记进行中，**无论成功失败都撤掉**（`finally`）。
   *
   * 成功后额外亮 {@link REFRESH_DONE_MS} 毫秒的「已更新」—— 与原型一致：菜单里要能看出
   * 刚刚发生了什么，而不只是一个转圈的瞬间。
   * 失败的处理（toast / 重试）交给调用方 —— 这个 hook 只管状态。
   */
  const run = useCallback(
    async (accountId: string, kind: AccountRefreshKind, task: () => Promise<unknown>): Promise<boolean> => {
      setOne(accountId, kind, true);
      try {
        await task();
        markDone(accountId, kind, true);
        const timerKey = keyOf(accountId, kind);
        const previous = doneTimers.current.get(timerKey);
        if (previous !== undefined) window.clearTimeout(previous);
        doneTimers.current.set(
          timerKey,
          window.setTimeout(() => {
            markDone(accountId, kind, false);
            doneTimers.current.delete(timerKey);
          }, REFRESH_DONE_MS),
        );
        return true;
      } finally {
        setOne(accountId, kind, false);
      }
    },
    [setOne, markDone],
  );

  return { kindsOf, doneKindsOf, isBusy, run, keyOf };
}
