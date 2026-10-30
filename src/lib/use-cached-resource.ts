import { useCallback, useEffect, useRef } from "react";

import { asError } from "@/lib/api";
import { useResourceStore, type ResourceEntry, type ResourceLoadOptions } from "@/stores/resources";

/**
 * 把一个「键 → 异步快照」接进组件。
 *
 * 语义（改造前 Trae 页面是「挂载即 `setLoading(true)` + 全套请求」）：
 *
 * | 情况 | `loading` | 界面 |
 * | --- | --- | --- |
 * | 该键**已有**快照 | `false` | 立刻渲染真数据，请求在后台跑 |
 * | 该键**没有**快照 | `true` | 走骨架屏（与改造前一致） |
 * | 取数失败且无快照 | `false` | 渲染错误提示 + 空态（不留用户干等） |
 * | 取数失败但有旧快照 | `false` | 继续显示旧数据 + 错误提示 |
 *
 * 因此「切到 WorkBuddy 再切回 TraeWork」不再闪骨架 —— 这正是本次优化要解决的问题。
 */
export interface CachedResource<T> {
  /** 快照值；`undefined` = 还没成功取到过。 */
  data: T | undefined;
  /** **仅在没有可用值**时为 true。有旧值时后台刷新不会让页面退回骨架屏。 */
  loading: boolean;
  /**
   * 是否有请求在飞（含**有旧值时的后台刷新**与轮询）。
   *
   * ⚠️ 与 `loading` 的分工：`loading` 是**骨架屏判据**（没有可用值才为 true），
   * 而「点刷新按钮」发生在**有旧值**的时候 —— 那时 `loading` 恒为 false。
   * 刷新类控件若绑 `loading`，图标永远不会转，用户读到的就是「点了没反应」。
   * 需要展示「正在刷新」的地方绑本字段（可再叠最小展示时长，见 `useRefreshAction`）。
   */
  pending: boolean;
  /** 已经格式化过的错误文案（用 `api.asError`，与页面改造前的 `setError` 同源）。 */
  error: string | null;
  /**
   * 最后一次**成功**取到值（或重新校验成功）的时刻（ms）；`0` = 从未成功过。
   *
   * 供「上次刷新」提示（issue #133）与自动刷新判「是否已过期」使用 —— 它是**数据的事实**，
   * 不要用定时器自己的时间冒充。
   */
  at: number;
  /** 强制取一次新值并等待完成。**变更操作之后必须调用它**，否则界面停在变更前。 */
  refresh: () => Promise<void>;
  /**
   * 「像刚挂载那样**再校验一次**」——**不强发**。
   *
   * 语义 = 挂载时那次取数：`freshMs` 窗口内已有值就**连请求都不发**，同键在飞请求复用。
   * 与 [`refresh`] 的分工：
   *
   * | | 窗口内已有值 | 同键在飞请求 | 用途 |
   * |:---|:---|:---|:---|
   * | `refresh()` | **照发**（`force`） | 不复用 | 变更操作之后（必须拿到新值） |
   * | `revalidate()` | 跳过 | 复用 | **轮询心跳** |
   *
   * ## 为什么轮询必须用它，而不是 `refresh`
   *
   * 同一把键可以有**多个消费者同时挂载**（例如 `useScopeStatus` 同时被顶栏与概览页用）。
   * 各自建定时器调 `refresh()` ⇒ 每个周期发**两遍**请求（该快照一次是 3 个后端调用，
   * 其中还有进程探测）。走本函数后，后到的那次落在 `freshMs` 窗口内直接返回 ——
   * 一个周期只有一次真请求，且**不需要**在 hook 外面另造一份「谁负责建定时器」的协调机制。
   *
   * ⚠️ **调用方的轮询间隔必须大于 `freshMs`**：两者相等时，tick 会因为「距上次写入还差
   * 几十毫秒不到一个窗口」而被跳过，实际节奏退化成两倍间隔（抖动决定命中与否）。
   */
  revalidate: () => Promise<void>;
  /**
   * 就地改写快照（乐观更新，如设置项开关先落本地再回读）。
   *
   * `current` **一定**有值：调用点都在「已经渲染出表单」的位置，而表单只在
   * 快照就绪后才渲染。快照尚未就绪时该调用是空操作（没什么可改的，
   * 紧接着完成的那次取数会带来权威值），因此不需要调用方自己判空。
   */
  patch: (updater: (current: T) => T) => void;
}

export function useCachedResource<T>(
  key: string | null,
  loader: () => Promise<T>,
  options?: ResourceLoadOptions,
): CachedResource<T> {
  const entry = useResourceStore((s) => (key === null ? undefined : s.entries[key])) as
    | ResourceEntry<T>
    | undefined;
  const load = useResourceStore((s) => s.load);
  const patchEntry = useResourceStore((s) => s.patch);

  // loader 每次渲染都是新函数（`useCallback` 依赖里带着变体等），所以只把它放进 ref：
  // effect 的依赖必须是**键**。否则「同一把键、loader 换了个身份」会白白多发一轮请求，
  // 而键相同就意味着结果相同 —— 那正是要避免的重复加载。
  const loaderRef = useRef(loader);
  loaderRef.current = loader;
  const freshMs = options?.freshMs ?? 0;

  useEffect(() => {
    if (key === null) return;
    void load(key, () => loaderRef.current(), { freshMs });
  }, [key, load, freshMs]);

  const refresh = useCallback(async () => {
    if (key === null) return;
    await load(key, () => loaderRef.current(), { force: true });
  }, [key, load]);

  // 与挂载时那次取数**逐字同一份语义**（同样的 `freshMs`、同样不强发）——
  // 这正是「revalidate = 再挂载一次」的定义，也是它与 `refresh` 的唯一差别。
  const revalidate = useCallback(async () => {
    if (key === null) return;
    await load(key, () => loaderRef.current(), { freshMs });
  }, [key, load, freshMs]);

  const patch = useCallback(
    (updater: (current: T) => T) => {
      if (key === null) return;
      patchEntry(key, (current) => (current === undefined ? undefined : updater(current as T)));
    },
    [key, patchEntry],
  );

  const data = entry?.value;

  return {
    data,
    // 出错且无值时不留在 loading：让页面渲染错误提示 + 空态，而不是永远转圈。
    loading: key !== null && data === undefined && entry?.error == null,
    pending: entry?.pending === true,
    error: entry?.error == null ? null : asError(entry.error),
    at: entry?.at ?? 0,
    refresh,
    revalidate,
    patch,
  };
}
