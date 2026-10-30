import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

import * as api from "@/lib/api";
import type { Region } from "@/lib/types";
import { selectRegionSlice, useAccountsStore } from "@/stores/accounts";

/**
 * 积分自动刷新间隔（issue #133：用户要求 10 分钟一轮）。
 *
 * ⚠️ 这个值同时被「积分视图」(`pages/CreditStatsPage.tsx`) 的自动刷新复用 ——
 * 两处必须同一个节奏：卡片上的余额和积分页的统计若各跑各的，同一时刻会给出两个数字。
 */
export const CREDIT_REFRESH_INTERVAL_MS = 10 * 60 * 1000;

/** 两个 region 都要刷 —— 账号列表是分区的，只看 CN 会让国际版余额永远停在首屏快照上。 */
const REGIONS: readonly Region[] = ["cn", "global"] as const;

/**
 * 走一轮「按 region 判断是否需要刷新」。
 *
 * 判据是**各自 region 的 `creditRefreshedAt`**，不是同一个全局时间戳：用户切到国际版 Tab
 * 时国内版可能刚刷过，共用一份时间会让国际版被判定为「还新鲜」而跳过。
 *
 * 三种情形分开处理：
 * - **本会话还没取过**（`0`）⇒ 只 `ensureCredits`（补缓存缺失），不强制 —— 刚启动就把
 *   所有账号打一遍上游没必要，缺哪张卡补哪张；
 * - **已过间隔** ⇒ `refreshCredits(silent)` 整轮强制刷（这是「自动刷新积分」的本体）；
 * - **未过间隔** ⇒ 仍然 `ensureCredits`：这一轮可能是新导入 / 刚加进来的账号还没缓存，
 *   等满 10 分钟才显示余额等于「加了账号看不到数」。已缓存时它是**零请求**的 no-op。
 */
function refreshStaleCredits() {
  const state = useAccountsStore.getState();
  const now = Date.now();
  for (const region of REGIONS) {
    const ids = selectRegionSlice(state, region).accounts.map((account) => account.id);
    if (ids.length === 0) continue;
    const last = state.creditRefreshedAt[region] ?? 0;
    if (last > 0 && now - last >= CREDIT_REFRESH_INTERVAL_MS) {
      void state.refreshCredits(ids, { silent: true, region });
      continue;
    }
    void state.ensureCredits(ids, region);
  }
}

/**
 * 主窗口可见时每 {@link CREDIT_REFRESH_INTERVAL_MS} 静默刷新一轮积分；隐藏即停。
 *
 * 「静默」= 不点亮工具栏的批量 refreshing 态、不逐卡转圈 —— 自动刷新不该让界面看起来
 * 像有人在操作（用户没点任何东西）。失败沿用缓存值，不弹 toast（`fetchCreditExpiry` 已吞错）。
 */
export function useCreditAutoRefresh() {
  const visibleRef = useRef(document.visibilityState !== "hidden");
  const timerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    function stopTimer() {
      if (timerRef.current !== undefined) {
        window.clearInterval(timerRef.current);
        timerRef.current = undefined;
      }
    }

    function startTimer() {
      stopTimer();
      if (!visibleRef.current) return;
      timerRef.current = window.setInterval(() => {
        if (!visibleRef.current) return;
        refreshStaleCredits();
      }, CREDIT_REFRESH_INTERVAL_MS);
    }

    function setVisible(next: boolean) {
      visibleRef.current = next;
      if (next) {
        // 切回来先补一轮「已经过期」的，再起定时器 —— 否则得等满一个间隔才更新。
        refreshStaleCredits();
        startTimer();
      } else {
        stopTimer();
      }
    }

    visibleRef.current = document.visibilityState !== "hidden";
    if (visibleRef.current) {
      refreshStaleCredits();
      startTimer();
    }

    const onVisibility = () => {
      setVisible(document.visibilityState !== "hidden");
    };
    document.addEventListener("visibilitychange", onVisibility);

    /* 账号列表到位（首屏拉取 / 新导入 / 删除）就补一轮：否则「启动后一直没开账号页」时，
       积分页的到期分布与卡片余额会空到第一个 10 分钟间隔。只在列表**引用**变化时跑，
       积分本身的高频 setState 不触发（否则每写一次余额就再判一轮）。 */
    const unsubscribe = useAccountsStore.subscribe((state, prev) => {
      if (!visibleRef.current) return;
      if (state.accounts === prev.accounts && state.global.accounts === prev.global.accounts) return;
      /* 微任务里再跑：`loadCredits` 会**同步**写 store，直接在订阅回调里写等于在
         setState 通知栈里再 setState，多一层无谓的重入。 */
      queueMicrotask(refreshStaleCredits);
    });

    let unlisten: (() => void) | undefined;
    if (!api.isWebui()) {
      void listen<boolean>("main-window-visible", (event) => {
        setVisible(event.payload);
      }).then((fn) => {
        unlisten = fn;
      });
    }

    return () => {
      stopTimer();
      unsubscribe();
      document.removeEventListener("visibilitychange", onVisibility);
      unlisten?.();
    };
  }, []);
}
