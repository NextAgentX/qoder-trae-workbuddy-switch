import { matchesAccountQuery } from "@/lib/account-search";

/**
 * 账号列表的**视图参数类型与纯函数**（搜索 / 排序 / 分组）。
 *
 * ## 为什么是纯函数、且不 import 任何页面 / 组件
 *
 * 视图参数（搜索 / 排序 / 分组 / 视图形态）的载体是**页面内参数条**（`PLAN §2.1 L3`）：
 * **不进 URL、不进全局 store、不碰作用域**（`DECISIONS.md` D1：「作用域决定数据从哪来，
 * 开关决定怎么看」）。把这些「怎么看」的规则收进本模块的纯函数，页面只负责持有 state、
 * 调用纯函数、渲染结果。
 *
 * 差异（哪些排序键有数据、怎么分桶）**全部以数据表达**：平台通过
 * {@link AccountListConfig} 注入比较器与桶键，共享组件里**不写平台判断**。
 *
 * ⚠️ 本模块可单测（不 import React / 页面 / 组件 / store）。
 */

/** 排序键。默认 `priority`（= 现状写死的「自动积分优先级」）。 */
export type AccountSortKey = "priority" | "name" | "expiry" | "recent";

/**
 * 分组键（**分桶**，与「分组筛选」`groupFilter` 语义不同 —— 见 `P4-TOOLBAR-ARCH §1.6`）。
 *
 * ⚠️ `group`（按自定义组）**仅对有分组概念的平台可用**（Trae；WB 的 `AccountMeta` 无 group 字段）。
 */
export type AccountGroupKey = "none" | "program" | "status" | "group";

/** 视图形态。 */
export type AccountViewMode = "card" | "table";

/** 一个分组桶。`key === null` 表示不分组（单桶）或「无桶名」桶。 */
export interface AccountBucket<T> {
  key: string | null;
  items: T[];
}

export type AccountComparator<T> = (a: T, b: T) => number;

/**
 * 平台侧提供的视图配置 —— 差异**全部在这里以数据表达**，共享组件不写平台判断。
 *
 * ⚠️ 与设计稿（`P4-TOOLBAR-ARCH §3.1`）的一处**必要细化**：`comparators` 由
 * `Record` 收成 `Partial<Record>`。原因：WB 的 `AccountMeta` **无 `lastUsed` 字段**
 * ⇒ 不提供 `recent` 排序（决策 #3）。若接口强制要求全部 4 个键，WB 就得写一条
 * **永远不会被调用**的比较器（死代码），或拿 `refreshedAt` 假扮「最近使用」
 * （正是决策 #3 要禁止的**假选项**）。收成 `Partial` 后，`sortAccounts` 对缺失键
 * **保持原序**（稳定），既无死代码也不造假。
 */
export interface AccountListConfig<T> {
  /** 每个账号的可搜索文本（`§3.13` 共享索引）。 */
  searchText: (row: T) => string;
  /** 排序比较器。**只需覆盖 `availableSorts`**；缺的键 `sortAccounts` 保持原序。 */
  comparators: Partial<Record<AccountSortKey, AccountComparator<T>>>;
  /** 排序后置顶（如 WB「当前账号置顶」）；缺省不动。 */
  pinFirst?: (sorted: T[]) => T[];
  /** 分组取桶键；返回 `null` ⇒ 该行落入「无桶名」桶（不渲染分组标题）。 */
  bucketKey: (row: T, group: AccountGroupKey) => string | null;
  /** 可用的排序 / 分组选项（**按需出现**，`§3.4` 规则 1：无数据不摆假选项）。 */
  availableSorts: AccountSortKey[];
  availableGroups: AccountGroupKey[];
}

/** 过滤（就地，不改后端）。空查询恒返回原数组。 */
export function filterAccounts<T>(rows: T[], config: AccountListConfig<T>, query: string): T[] {
  if (!query.trim()) return rows;
  return rows.filter((row) => matchesAccountQuery(config.searchText(row), query));
}

/**
 * 稳定排序 + 可选置顶。
 *
 * 稳定性靠「记住原索引」实现：键相等时回落到 `index` 差（与改造前
 * `left.index - right.index` 同一口径）。这样切排序键不会让相等项乱跳。
 */
export function sortAccounts<T>(rows: T[], key: AccountSortKey, config: AccountListConfig<T>): T[] {
  const compare = config.comparators[key];
  const base = compare
    ? rows
        .map((row, index) => ({ row, index }))
        .sort((left, right) => {
          const diff = compare(left.row, right.row);
          return diff !== 0 ? diff : left.index - right.index;
        })
        .map(({ row }) => row)
    : [...rows];
  return config.pinFirst ? config.pinFirst(base) : base;
}

/**
 * 分桶。`none` 返回单桶（`key = null`）。
 *
 * 桶内保持入参顺序（调用方通常先 `sortAccounts` 再 `groupAccounts`）；
 * 桶之间的顺序按**首次出现**排列（Map 的插入序），因此「先出现的状态排前面」。
 */
export function groupAccounts<T>(
  rows: T[],
  group: AccountGroupKey,
  config: AccountListConfig<T>,
): AccountBucket<T>[] {
  if (group === "none") return [{ key: null, items: rows }];

  const order: string[] = [];
  const map = new Map<string, T[]>();
  for (const row of rows) {
    const bucketKey = config.bucketKey(row, group) ?? "";
    let items = map.get(bucketKey);
    if (!items) {
      items = [];
      map.set(bucketKey, items);
      order.push(bucketKey);
    }
    items.push(row);
  }
  return order.map((bucketKey) => ({
    key: bucketKey === "" ? null : bucketKey,
    items: map.get(bucketKey) ?? [],
  }));
}

/**
 * 是否有任何筛选生效（用于「真没有数据」vs「被筛没了」的判据）。
 *
 * ⚠️ 分桶（`group`）本身**不移除**任何行，因此「被筛没」的充分判据仍是
 * `query` 非空；本函数把 `group` 一并计入，是因为「清除筛选」出口要**同时**复位两者。
 */
export function hasActiveFilter(state: { query: string; group: AccountGroupKey }): boolean {
  return state.query.trim().length > 0 || state.group !== "none";
}
