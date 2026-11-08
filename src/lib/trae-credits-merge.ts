/**
 * Trae 积分 · **跨区域合并**（纯函数，可单测）。
 *
 * ## 为什么需要它
 *
 * WorkBuddy 侧的「用量 · 积分」有一个**合并态**开关（原型 `viewUsage` 的
 * 「仅本作用域 / 合并全部作用域」）。Trae 侧要长得一样、交互一样，就必须真的能合并 ——
 * 否则那枚控件就是**假控件**（切了数据不变），这是本仓明令禁止的。
 *
 * Trae 的积分数据按**区域**分家（`credits_overview_for(TraeVariant)`，
 * `TraeWork`＝国内库 / `Global`＝国际库），后端**没有**「一次取两区」的命令，
 * 所以合并只能在前端做：取两次、再聚合。
 *
 * ## ★ 唯一不能做的事：按 `userId` 合并
 *
 * 同一个 `userId` 在两条区域线是**两个不同账号**（`account.rs` 明写：
 * 同一 uid 在两条线是两个账号）。所以本模块：
 *
 * - 行键 = `<region>:<userId>`（**带区域前缀**），绝不把两区的行按 uid 折叠成一行；
 * - 汇总类数字（合计 / 每日趋势）**求和**，不涉及键；
 * - 不合并 `packages` / `records` 这类以 uid 为键的映射 —— 页面需要它们时按行取该区域的原始切片。
 *
 * ## 单区也走本函数
 *
 * `mergeTraeCredits([单区切片])` 与「不合并」得到同一形状 ⇒ 页面**只有一条代码路径**，
 * 不必为「单区 / 合并」各写一套渲染（那正是两平台当初分叉的成因）。
 */

import type {
  TraeAccountsOverview,
  TraeCreditsOverview,
  TraeRegionId,
  TraeUnsupported,
} from "@/lib/trae-types";

/** 一个区域的积分 + 账号快照（页面按需取 1 或 2 片）。 */
export interface TraeRegionSlice {
  region: TraeRegionId;
  credits: TraeCreditsOverview;
  overview: TraeAccountsOverview;
}

/** 合并后的一行账号积分。 */
export interface TraeCreditsRow {
  /** 行键 = `<region>:<userId>`（**跨区不折叠**，见文件头）。 */
  key: string;
  region: TraeRegionId;
  userId: string;
  name: string;
  groupId: string | null;
  /** 可用积分；两处都取不到 ⇒ `null`（显示「未同步」，**不**当成 0）。 */
  credits: number | null;
  /** 到期时间（**秒**）；无 ⇒ `null`。 */
  expireAt: number | null;
}

/** 每日快照（合并后按日期求和）。 */
export interface TraeCreditsDaily {
  date: string;
  total: number;
  earned: number;
  consumed: number;
}

/**
 * 积分流水的一条（原型「积分流水」表：时间 / 账号 / 变动 / 说明）。
 *
 * `key` 带**区域前缀**：同一 uid 在两条区域线是两个账号，按 uid 折叠会串语义。
 */
export interface TraeCreditsRecord {
  key: string;
  region: TraeRegionId;
  userId: string;
  /** `YYYY-MM-DD`（后端快照按天记，没有更细的时间戳）。 */
  date: string;
  /** 当日余额。 */
  credits: number;
  /** 当日变动（正 = 入账，负 = 消耗）。 */
  delta: number;
}

export interface TraeCreditsMerged {
  rows: TraeCreditsRow[];
  /** 各账号可用积分之和（`credits === null` 的行不计入 —— 见 `unknownCount`）。 */
  total: number;
  /**
   * 额度**未知**的账号数（`credits === null`）。
   *
   * 后端在「额度不可解析」时会清掉缓存（Free / SOLO 账户的权益包里没有 `credits_limit`，
   * issue #186）。这些行不进 `total`，所以 `total` 是**部分和** —— 必须把这个数告诉界面，
   * 否则「总计 0」会被读成「所有账号都没额度」。
   */
  unknownCount: number;
  /** 按日期升序的每日快照（两区同日相加）。 */
  daily: TraeCreditsDaily[];
  /** 积分流水（按日期**降序**，两区合并；行键带区域前缀）。 */
  records: TraeCreditsRecord[];
  /** 最近一次积分缓存更新时间（取最新）。 */
  updatedAt: string | null;
  /** 历史明细保留天数（取最大）。 */
  historyDays: number;
  /** 平台做不到的维度（取首个非空，两区同源同文案）。 */
  unsupported: TraeUnsupported[];
}

export function mergeTraeCredits(slices: TraeRegionSlice[]): TraeCreditsMerged {
  const rows: TraeCreditsRow[] = [];
  const dailyByDate = new Map<string, TraeCreditsDaily>();
  const records: TraeCreditsRecord[] = [];
  let updatedAt: string | null = null;
  let historyDays = 0;
  let unsupported: TraeUnsupported[] = [];

  for (const slice of slices) {
    const { region, credits, overview } = slice;

    for (const account of overview.accounts ?? []) {
      const uid = account.userId;
      rows.push({
        key: `${region}:${uid}`,
        region,
        userId: uid,
        name: account.name,
        groupId: account.groupId,
        /* 权威来源是积分缓存的 `remaining`；缺失时回落账号视图的同名字段（与改造前一致）。 */
        credits: credits.remaining?.[uid] ?? account.remainingCredits ?? account.credits ?? null,
        expireAt: credits.expireTimes?.[uid] ?? account.creditsExpireAt ?? null,
      });
    }

    for (const record of credits.records ?? []) {
      records.push({
        key: `${region}:${record.userId}:${record.date}`,
        region,
        userId: record.userId,
        date: record.date,
        credits: record.credits,
        delta: record.delta,
      });
    }

    for (const snapshot of credits.daily ?? []) {
      const entry = dailyByDate.get(snapshot.date) ?? {
        date: snapshot.date,
        total: 0,
        earned: 0,
        consumed: 0,
      };
      entry.total += snapshot.total ?? 0;
      entry.earned += snapshot.earned ?? 0;
      entry.consumed += snapshot.consumed ?? 0;
      dailyByDate.set(snapshot.date, entry);
    }

    if (credits.updatedAt && (!updatedAt || credits.updatedAt > updatedAt)) {
      updatedAt = credits.updatedAt;
    }
    historyDays = Math.max(historyDays, credits.historyDays ?? 0);
    if (unsupported.length === 0 && (credits.unsupported?.length ?? 0) > 0) {
      unsupported = credits.unsupported;
    }
  }

  /* 排序：积分多的在前，`null`（未同步）沉底 —— 与改造前一致，且**确定性**
     （同分按行键，避免两区数据相同序时顺序抖动）。 */
  rows.sort((left, right) => {
    if (left.credits === null && right.credits === null) return left.key < right.key ? -1 : 1;
    if (left.credits === null) return 1;
    if (right.credits === null) return -1;
    if (right.credits !== left.credits) return right.credits - left.credits;
    return left.key < right.key ? -1 : 1;
  });

  const daily = [...dailyByDate.values()].sort((left, right) =>
    left.date < right.date ? -1 : left.date > right.date ? 1 : 0,
  );

  /* 流水按**日期降序**（原型 `viewUsage` 的积分流水表：最新的在最上）。
     同日按行键定序 ⇒ 顺序**确定性**，两区数据相同也不会抖动。 */
  records.sort((left, right) => {
    if (left.date !== right.date) return left.date < right.date ? 1 : -1;
    return left.key < right.key ? -1 : 1;
  });

  return {
    rows,
    total: rows.reduce((sum, row) => sum + (row.credits ?? 0), 0),
    unknownCount: rows.filter((row) => row.credits === null).length,
    daily,
    records,
    updatedAt,
    historyDays,
    unsupported,
  };
}
