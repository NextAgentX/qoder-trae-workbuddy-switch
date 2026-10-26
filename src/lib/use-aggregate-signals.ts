/**
 * 聚合页的**信号层**（issue #50 · D4.1 / D4.2）。
 *
 * ## 这一层补的是「卡片上的四项数据」
 *
 * 账号列表本身（`accounts_list` / `get_trae_accounts` / `get_qoder_accounts`）只给
 * **账号是什么**，不给「它今天怎么样」。卡片上另外四项各有各的取数路径：
 *
 * | 信号 | WorkBuddy | Trae | Qoder |
 * |:--|:--|:--|:--|
 * | 今日 Token | `get_account_token_today` | `get_trae_account_token_today` | 无此概念 |
 * | 今日签到 | `get_checkin_status_map` | `get_trae_checkin_status_map` | `qoder_checkin_status_map` |
 * | 程序位状态 | 4 条 status 命令 | `get_trae_variants` + 登录态 | `qoder_program_status` |
 * | 积分 | store 的 `creditMap` | `get_trae_accounts` 自带 | `qoder_credits` |
 *
 * ## 关键纪律
 *
 * ① **逐作用域取，不逐账号取**。每条命令都是「一次给一批」的（`status_map` 收一串 id、
 *    `token_today` 一次给整区）。逐账号调用会变成 N×N 次上游查询 —— 账号页里已有明文禁止。
 * ② **缺席 ≠ 0**。`token_today` 的 `accounts` 里没有某账号 = 「今日无记录 / 未知」，
 *    与「有一行全 0」是两件事。Map 的 `get` 返回 `undefined` 即缺席。
 * ③ **部分失败不拖垮其余**。Qoder 的签到通道常是「未开放」，那是**事实**不是错误；
 *    某一类命令挂了只影响那一类信号。
 * ④ **不额外取程序位状态**除非真的要用：程序位状态是「本机装了哪些客户端」，
 *    与账号列表无关 —— 但没有它，卡片页脚那排胶囊**画不出来**
 *    （`QoderAccountCard` 的 `programs` 不传 ⇒ 整行不渲染，不是画一排灰胶囊）。
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import * as api from "@/lib/api";
import type { AccountTokenTodayEntry, Region } from "@/lib/types";
import type { QoderProgramStatus, QoderCreditsRow } from "@/lib/api";
import type {
  TraeAccount,
  TraeCreditPackage,
  TraeRegionId,
  TraeVariantStatus,
} from "@/lib/trae-types";
import {
  loadTraeVariantLogins,
  loadTraeVariantStatuses,
  type TraeVariantLogins,
} from "@/lib/trae-variant-status";
import { traeProgramsFor } from "@/lib/trae-programs";
import type { TraeProgram } from "@/components/trae-account-card";
import { AGGREGATE_SOURCES, type AggregateAccount } from "@/lib/aggregate-accounts";
import { scopeParts, type Scope } from "@/lib/scope";

/** 一个作用域的信号集合。每一项都可能「没取到」（`undefined` = 未知 / 通道未开放）。 */
export interface ScopeSignals {
  /** 今日 Token：`uid → 明细`（**缺席 = 未知，不是 0**）。 */
  todayTokens: Map<string, AccountTokenTodayEntry>;
  /** 网关部分是否被裁（渲染成 `≥N` 的下界）。 */
  todayTokensTruncated: boolean;
  /** 今日签到：`账号键 → 是否已签`（`null` = 未知 / 通道未开放）。 */
  checkin: Map<string, boolean | null>;
  /**
   * Trae 程序位的**探测结果**（跨区域一次探测，逐区域过滤）。
   *
   * ⚠️ 这里存的是 `statuses` / `logins` 本身，**不是**算好的程序位数组 ——
   * 因为「这个账号是不是该程序位的当前账号」是**逐账号**判的（同一区域里，
   * A 是 Work 的当前账号、B 是 Code 的当前账号）。按账号算 ⇒ 放在页面侧调
   * `traeProgramsFor(account, …)`，与 Trae 账号页**共用同一份判定**。
   */
  traeStatuses: TraeVariantStatus[];
  traeLogins: TraeVariantLogins;
  /** Qoder 程序位（按区域）。 */
  qoderPrograms: QoderProgramStatus[];
  /** Qoder 积分（`uid → 行`）。 */
  qoderCredits: Map<string, QoderCreditsRow>;
  /**
   * Trae 的**逐包积分**（`userId → 包数组`）。
   *
   * ★ issue #89 · C2：账号视图（`list_account_views_for`）只带聚合积分，包粒度明细在
   * `get_trae_credits` 的 `packages[uid]` 里。渠道页把它**合并进 account 对象**再交给卡片；
   * 聚合页此前直接用账号对象自带的 `creditPackages`（未刷新过积分时为 `null`）
   * ⇒ 指标行的 `/Y` 与「N 积分 M 天内过期」chip 在聚合页常缺。
   *
   * 每个作用域**一次**调用（不是逐账号），符合本层的取数纪律 ①。
   */
  traeCredits: Map<string, TraeCreditPackage[]>;
}

const EMPTY: ScopeSignals = {
  todayTokens: new Map(),
  todayTokensTruncated: false,
  checkin: new Map(),
  traeStatuses: [],
  traeLogins: {},
  qoderPrograms: [],
  qoderCredits: new Map(),
  traeCredits: new Map(),
};

/** 账号在某个作用域里的主键（各平台不同：WB `id` / Trae `userId` / Qoder `uid`）。 */
function primaryKey(account: AggregateAccount): string {
  if (account.platform === "workbuddy") return account.data.id;
  if (account.platform === "trae") return account.data.userId;
  return account.data.uid;
}

/** 今日 Token 的归属键：**uid**（不是账号 id）—— 三个平台的今日 Token 都按 uid 聚合。 */
function tokenKey(account: AggregateAccount): string | null {
  if (account.platform === "workbuddy") return account.data.uid;
  /* Trae 的 `todayTokens` 用账号自己的 userId 聚合（见 `TraeAccountsPage`）。 */
  if (account.platform === "trae") return account.data.userId;
  return null;
}

export interface AggregateSignalsState {
  /** `作用域 → 信号`。取不到的项是空集合（卡片据此不渲染那一格）。 */
  byScope: Map<Scope, ScopeSignals>;
  /** 整体是否还在首屏加载。 */
  loading: boolean;
  /** 重取全部信号（卡片「刷新」四项之后调用）。 */
  reload: () => Promise<void>;
}

/**
 * 拉取 6 个作用域的信号。
 *
 * ⚠️ **不轮询**：这些是「今天怎么样」的快照，聚合页是**手动**刷新型界面
 * （顶栏 ⟳ / 卡片「更多 → 刷新」）。加轮询会让 6 个作用域 × 4 类信号一直打后端。
 */
export function useAggregateSignals(accounts: readonly AggregateAccount[]): AggregateSignalsState {
  const [byScope, setByScope] = useState<Map<Scope, ScopeSignals>>(new Map());
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);

  /** 按作用域分组（一次分组，三处取数共用）。 */
  const grouped = useMemo(() => {
    const map = new Map<Scope, AggregateAccount[]>();
    for (const account of accounts) {
      const bucket = map.get(account.scope);
      if (bucket) bucket.push(account);
      else map.set(account.scope, [account]);
    }
    return map;
  }, [accounts]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    /**
     * Trae 的程序位状态是**跨区域**的一次探测（`get_trae_variants` 一次给全部区域），
     * 但卡片要的是「**本区域**的程序位」⇒ 先取一次全量，再按区域过滤
     * （`traeProgramsFor` 内部已按 variant 过滤）。
     *
     * ⚠️ **登录态取不到时不能把程序位探测结果一起丢掉**：`loadTraeVariantLogins`
     * 读的是客户端的登录态快照（读不到很常见 —— 客户端没启动过），
     * 而程序位「装了没有 / 能不能写」是**另一件事**。两者一起清掉 ⇒
     * 卡片页脚那排程序位胶囊**整行消失**，用户以为「没装客户端」。
     * ⇒ 登录态失败只丢登录态（`current` 判定退化为「都不是当前账号」）。
     */
    const traeGlobal = loadTraeVariantStatuses()
      .then(async (statuses) => ({
        statuses,
        logins: await loadTraeVariantLogins(statuses).catch(() => ({} as TraeVariantLogins)),
      }))
      .catch(() => ({ statuses: [] as TraeVariantStatus[], logins: {} as TraeVariantLogins }));

    void Promise.all(
      AGGREGATE_SOURCES.map(async (source) => {
        const items = grouped.get(source.scope) ?? [];
        const signals: ScopeSignals = { ...EMPTY };

        /* ── 今日 Token ── */
        if (source.platform === "workbuddy") {
          try {
            const response = await api.getAccountTokenToday(source.region as Region);
            signals.todayTokens = new Map(response.accounts.map((entry) => [entry.uid, entry]));
            signals.todayTokensTruncated = response.gatewayTruncated;
          } catch {
            /* 取不到 ⇒ 卡片那一格不渲染（不是 0） */
          }
        } else if (source.platform === "trae") {
          try {
            const response = await api.getTraeAccountTokenToday(source.variant);
            signals.todayTokens = new Map(response.accounts.map((entry) => [entry.uid, entry]));
            signals.todayTokensTruncated = response.gatewayTruncated;
          } catch {
            /* 同上 */
          }
        }

        /* ── 今日签到 ── */
        try {
          if (source.platform === "workbuddy") {
            const ids = items.map((item) => primaryKey(item));
            const map = await api.getCheckinStatusMap(ids, source.region as Region);
            signals.checkin = new Map(
              items.map((item) => [primaryKey(item), map[primaryKey(item)]?.todayCheckedIn ?? null]),
            );
          } else if (source.platform === "trae") {
            const ids = items.map((item) => primaryKey(item));
            const map = await api.getTraeCheckinStatusMap(source.variant, ids);
            signals.checkin = new Map(
              items.map((item) => [primaryKey(item), map[primaryKey(item)]?.todayCheckedIn ?? null]),
            );
          } else {
            const ids = items.map((item) => primaryKey(item));
            const map = await api.qoderCheckinStatusMap(source.scope, ids);
            /* ⚠️ Qoder 的签到是**三档**：`active === false` ⇒ 该区域未开放活动。
               那一档**既不算成功也不算失败** —— 卡片的签到项据此显示「通道未开放」。 */
            signals.checkin = new Map(
              items.map((item) => {
                const entry = map[primaryKey(item)];
                if (!entry) return [primaryKey(item), null] as const;
                if (entry.active === false) return [primaryKey(item), null] as const;
                return [primaryKey(item), entry.todayCheckedIn] as const;
              }),
            );
          }
        } catch {
          /* 签到状态读不到 ⇒ 全部未知（`null`） */
        }

        /* ── 程序位 ── */
        if (source.platform === "trae") {
          try {
            const { statuses, logins } = await traeGlobal;
            /* 存**探测结果**而非算好的数组：`current` 是逐账号判的（见 `ScopeSignals` 注释）。 */
            signals.traeStatuses = statuses;
            signals.traeLogins = logins;
          } catch {
            /* 程序位取不到 ⇒ 卡片不渲染程序位行（`programs` 不传） */
          }
        } else if (source.platform === "qoder") {
          try {
            const view = await api.qoderProgramStatus(source.scope);
            signals.qoderPrograms = view.programs;
          } catch {
            /* 同上 */
          }
        }

        /* ── Trae 逐包积分（issue #89 · C2）── */
        if (source.platform === "trae") {
          try {
            const overview = await api.getTraeCredits(source.variant);
            signals.traeCredits = new Map(Object.entries(overview.packages ?? {}));
          } catch {
            /* 取不到 ⇒ 卡片不渲染逐包明细（不是画一条 0） */
          }
        }

        /* ── Qoder 积分 ── */
        if (source.platform === "qoder") {
          try {
            const view = await api.qoderCredits(source.scope);
            signals.qoderCredits = new Map(view.accounts.map((row) => [row.uid, row]));
          } catch {
            /* 取不到 ⇒ 卡片不渲染积分指标 */
          }
        }

        return [source.scope, signals] as const;
      }),
    ).then((entries) => {
      if (cancelled) return;
      setByScope(new Map(entries));
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [grouped, nonce]);

  const reload = useCallback(async () => {
    setNonce((value) => value + 1);
  }, []);

  return { byScope, loading, reload };
}

/** 取某账号的今日 Token 条目（**缺席 = 未知**，调用方据此不渲染那一格）。 */
export function todayTokensOf(
  signals: ScopeSignals | undefined,
  account: AggregateAccount,
): AccountTokenTodayEntry | undefined {
  const key = tokenKey(account);
  if (!key) return undefined;
  return signals?.todayTokens.get(key);
}

/** 取某账号的今日签到（`null` = 未知 / 通道未开放）。 */
export function checkinTodayOf(
  signals: ScopeSignals | undefined,
  account: AggregateAccount,
): boolean | null {
  if (!signals) return null;
  return signals.checkin.get(primaryKey(account)) ?? null;
}

/** 取某账号的 Trae 程序位（**按账号判 `current`**，与 Trae 账号页共用 `traeProgramsFor`）。 */
export function traeProgramsOf(
  signals: ScopeSignals | undefined,
  account: AggregateAccount,
): TraeProgram[] {
  if (account.platform !== "trae" || !signals) return [];
  /* ⚠️ 这里传的是**区域 id**（`cn` / `global`），不是账号库变体 id
     （`trae_merged` / `global_trae_merged`）。
     `TraeVariantStatus.variant` 是**区域**（`cn`/`global`），而 `program.variant`
     才是账号库变体 —— 两层含义不同，传错会过滤后全空（卡片页脚整行消失）。
     账号库变体（`trae_merged`）是**命令参数**（删除 / 签到 / 切换用），别与这里混。 */
  const region = scopeParts(account.scope).region as TraeRegionId;
  return traeProgramsFor(account.data as TraeAccount, region, signals.traeStatuses, signals.traeLogins);
}
