/**
 * 「打上游」类自动刷新是否开启 —— **默认关，opt-in**（issue #133 复核 §3 / 方案 C）。
 *
 * ## 为什么默认关
 *
 * 这个偏好管的是**会真的向上游发请求**的那几处自动刷新（WB 统计页、Qoder 积分页 /
 * 账号页、账号卡余额）。本仓对「替用户做动作」类能力已有判据：`trae_checkin_enabled` /
 * `qoder_checkin_enabled` 都是默认 `false`，理由写在 `default_schedule()` 注释里
 * ——「替用户去领东西，他没要求过 ⇒ opt-in」。**每 10 分钟打一轮上游，性质同类。**
 *
 * 反过来，「只读本地快照」的视图（Trae 两页）不受这个开关影响：它们零上游成本，
 * 关掉只会让界面停在旧数字上，没有任何收益。
 *
 * ## 为什么是 `localStorage` 而不是 `app_prefs.json`
 *
 * 读者**只有前端**（三个页面的定时器 + `useCreditAutoRefresh` 的循环），同步可得、零 IPC；
 * 为一枚纯前端偏好加一对后端命令，收益是负的。与 `scope-visibility.ts` /
 * `aggregate-visibility-pref.ts` 同一判断。
 *
 * ## 为什么是 zustand store 而不是 `useState`
 *
 * 四个读者分散在不同页面与一个全局 hook 里，各存一份 `useState` 时，设置页关掉开关
 * 其余入口要刷新页面才生效（`storage` 事件只在跨 tab 触发）。唯一真值放 store，
 * 写入即全应用同步 —— 与 `scope-visibility.ts` 同构。
 */

import { create } from "zustand";

/** `v1` 前缀是仓内惯例（语义若变可换键而不破坏旧值）。 */
const STORAGE_KEY = "buddy-switch:v1:credits-auto-refresh-upstream";

function safeStorage(): Pick<Storage, "getItem" | "setItem"> | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

/**
 * 读偏好。
 *
 * ⚠️ **缺键 ⇒ `false`**（默认关）。这里刻意不写「缺键时猜 true」：默认值决定了
 * 升级后的第一轮行为，猜 true 等于替所有老用户继续打上游。
 */
function readEnabled(storage: Pick<Storage, "getItem"> | undefined): boolean {
  try {
    return storage?.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

interface CreditRefreshPrefState {
  /** 是否允许「打上游」的自动刷新。默认 `false`。 */
  upstreamAutoRefresh: boolean;
  setUpstreamAutoRefresh: (next: boolean) => void;
}

export const useCreditRefreshPref = create<CreditRefreshPrefState>((set) => ({
  upstreamAutoRefresh: readEnabled(safeStorage()),
  setUpstreamAutoRefresh: (next) => {
    try {
      safeStorage()?.setItem(STORAGE_KEY, String(next));
    } catch {
      /* 存储不可用（隐私模式）⇒ 记不住，不影响本次会话 */
    }
    set({ upstreamAutoRefresh: next });
  },
}));

/**
 * 非 React 读者（`useCreditAutoRefresh` 的定时器回调、store 层）用这个读当前值。
 *
 * ⚠️ 不要在组件里用它代替 `useCreditRefreshPref(...)` —— 那样改了开关不会重渲染。
 */
export function isUpstreamAutoRefreshEnabled(): boolean {
  return useCreditRefreshPref.getState().upstreamAutoRefresh;
}

/* 跨 tab 同步：本 store 的写入已即时通知本 tab 的所有订阅者，
   `storage` 事件补上「另一个 tab 改了开关」这一条路。 */
if (typeof window !== "undefined") {
  window.addEventListener("storage", (e) => {
    if (e.key === STORAGE_KEY) {
      useCreditRefreshPref.setState({ upstreamAutoRefresh: readEnabled(window.localStorage) });
    }
  });
}
