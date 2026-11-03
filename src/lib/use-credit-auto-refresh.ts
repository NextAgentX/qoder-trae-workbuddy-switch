import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

import * as api from "@/lib/api";
import { useCreditRefreshPref } from "@/lib/credit-refresh-pref";
import type { Region } from "@/lib/types";
import { selectRegionSlice, useAccountsStore } from "@/stores/accounts";

/**
 * ★ **两个节拍，不是一件事**（issue #133 复核 §3「把打上游与读本地拆开」）。
 *
 * 原先只有一个 `CREDIT_REFRESH_INTERVAL_MS`，同时管着两类成本完全不同的刷新：
 *
 * | 类 | 视图 | 成本 |
 * |:--|:--|:--|
 * | **打上游** | WB 统计页、Qoder 积分页 / 账号页、卡片余额 | 真发请求，可能被上游限流 |
 * | **读本地快照** | Trae 积分页 / 账号页 | 零上游成本，只是把后台已写入的读数读回来 |
 *
 * 共用一个常量意味着「想让 Trae 跟得更紧」会连带把上游请求量一起抬上去。
 * 现在分开：读本地的可以更短，打上游的保持保守。
 */
export const CREDITS_UPSTREAM_REFRESH_INTERVAL_MS = 10 * 60 * 1000;

/** 「读本地快照」类视图的节拍（零上游成本 ⇒ 可以更短）。 */
export const CREDITS_LOCAL_REFRESH_INTERVAL_MS = 3 * 60 * 1000;

/**
 * 间隔抖动幅度：±20%（10 分钟 ⇒ 8~12 分钟，3 分钟 ⇒ 2.4~3.6 分钟）。
 *
 * 幅度按**比例**给，不照搬 `schedule-times-editor` 的 `[0, 5, 15, 30]` 分钟绝对值档位 ——
 * 对 10 分钟间隔而言 ±30 分钟是荒谬的，±5 分钟也已占到 50%。
 */
const JITTER_RATIO = 0.2;

/**
 * 给间隔加抖动。抖动作用于**间隔**，不是钟点（那是「每天几点触发」的语义，见
 * `schedule.rs::next_fire_with_jitter`，两者不可互换）。
 *
 * 为什么要抖：`setInterval` 的确定性在代码里看得见 —— 所有实例都在整十分钟发一轮，
 * 这种规律本身就是「不是人在用」的指纹。
 */
export function jitteredInterval(baseMs: number, ratio: number = JITTER_RATIO): number {
  return Math.max(1_000, Math.round(baseMs * (1 + (Math.random() * 2 - 1) * ratio)));
}

/** 两个 region 都要刷 —— 账号列表是分区的，只看 CN 会让国际版余额永远停在首屏快照上。 */
const REGIONS: readonly Region[] = ["cn", "global"] as const;

/**
 * 走一轮「按 region 判断是否需要刷新」。
 *
 * 判据是**各自 region 的 `creditRefreshedAt`**，不是同一个全局时间戳：用户切到国际版 Tab
 * 时国内版可能刚刷过，共用一份时间会让国际版被判定为「还新鲜」而跳过。
 *
 * @param minAgeMs 判定「过期」的阈值。调用方传**本轮实际用的间隔**（已含抖动）——
 *   否则定时器按 8 分钟响、这里按 10 分钟判「还新鲜」，就会白白跳过一轮。
 *
 * 三种情形分开处理：
 * - **本会话还没取过**（`0`）⇒ 只 `ensureCredits`（补缓存缺失），不强制 —— 刚启动就把
 *   所有账号打一遍上游没必要，缺哪张卡补哪张；
 * - **已过间隔** ⇒ `refreshCredits(silent)` 整轮强制刷（这是「自动刷新积分」的本体）；
 * - **未过间隔** ⇒ 仍然 `ensureCredits`：这一轮可能是新导入 / 刚加进来的账号还没缓存，
 *   等满一个间隔才显示余额等于「加了账号看不到数」。已缓存时它是**零请求**的 no-op。
 */
function refreshStaleCredits(minAgeMs: number = CREDITS_UPSTREAM_REFRESH_INTERVAL_MS) {
  const state = useAccountsStore.getState();
  const now = Date.now();
  for (const region of REGIONS) {
    const ids = selectRegionSlice(state, region).accounts.map((account) => account.id);
    if (ids.length === 0) continue;
    const last = state.creditRefreshedAt[region] ?? 0;
    if (last > 0 && now - last >= minAgeMs) {
      void state.refreshCredits(ids, { silent: true, region });
      continue;
    }
    void state.ensureCredits(ids, region);
  }
}

/**
 * 主窗口可见时每 {@link CREDITS_UPSTREAM_REFRESH_INTERVAL_MS}（±20%）静默刷新一轮积分；
 * 隐藏即停。
 *
 * 「静默」= 不点亮工具栏的批量 refreshing 态、不逐卡转圈 —— 自动刷新不该让界面看起来
 * 像有人在操作（用户没点任何东西）。失败沿用缓存值，不弹 toast（`fetchCreditExpiry` 已吞错）。
 */
export function useCreditAutoRefresh() {
  const visibleRef = useRef(document.visibilityState !== "hidden");
  const timerRef = useRef<number | undefined>(undefined);
  /* ★ 默认关，opt-in（issue #133 复核 §3 / 方案 C）：这条循环会**真的**按 region 打上游
     刷一遍余额，与「替用户领东西」同类 ⇒ 不替用户决定。开关在「设置 · 通用」。
     用 store 订阅而不是读一次常量：设置页关掉后本 hook 立刻停表，不必等下一轮。 */
  const upstreamEnabled = useCreditRefreshPref((state) => state.upstreamAutoRefresh);

  useEffect(() => {
    function stopTimer() {
      if (timerRef.current !== undefined) {
        window.clearTimeout(timerRef.current);
        timerRef.current = undefined;
      }
    }

    /* ★ `setInterval` → `setTimeout` 链：每轮重新掷一次抖动，节拍不再固定。
       阈值与延迟取**同一个** `delay`，否则会「到点却判还新鲜」而空转一轮。 */
    function startTimer() {
      stopTimer();
      if (!visibleRef.current || !upstreamEnabled) return;
      const loop = () => {
        const delay = jitteredInterval(CREDITS_UPSTREAM_REFRESH_INTERVAL_MS);
        timerRef.current = window.setTimeout(() => {
          if (!visibleRef.current) return;
          refreshStaleCredits(delay);
          loop();
        }, delay);
      };
      loop();
    }

    function setVisible(next: boolean) {
      visibleRef.current = next;
      if (next) {
        /* ⚠️ 这里也要判开关：`main-window-visible` 事件会走这条路，
           漏判的话「关掉开关后切一次窗口」就又打了一轮上游。 */
        if (upstreamEnabled) {
          // 切回来先补一轮「已经过期」的，再起定时器 —— 否则得等满一个间隔才更新。
          refreshStaleCredits();
          startTimer();
        }
      } else {
        stopTimer();
      }
    }

    visibleRef.current = document.visibilityState !== "hidden";
    if (visibleRef.current && upstreamEnabled) {
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
      if (!visibleRef.current || !upstreamEnabled) return;
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
  }, [upstreamEnabled]);
}
