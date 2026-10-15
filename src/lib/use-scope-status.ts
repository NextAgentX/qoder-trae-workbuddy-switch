/**
 * 四个作用域的**环境状态汇总** —— 顶栏作用域选择器的唯一取数来源。
 *
 * ## 为什么要有这一层（而不是各组件自己拼）
 *
 * 顶栏的触发按钮与作用域菜单都要展示**全部 4 个作用域**的同一组事实
 * （`已登录 xxx / 未登录 / 未检测到 · N 个账号`，外加运行点）。两处各拼一遍
 * 迟早漂移成「触发按钮说 8 个账号、菜单里说 7 个」—— 那正是本项目反复要消灭的那类不一致。
 *
 * ## 四条事实分别从哪来
 *
 * - **WorkBuddy 侧**（`wb-cn` / `wb-global`）：直接读 `useAccountsStore` 的
 *   当前切片（`status` / `status.global`）。它由既有的 `useWorkbuddyStatusRefresh`
 *   与账号页维护，本 hook **不额外取数**。
 * - **Trae 侧**（`trae-cn` / `trae-global`）：一次聚合快照 ——
 *   安装 / 运行态取自 `trae-variant-status` 的同一次探测（`loadTraeVariantStatuses`，
 *   即侧栏运行点的数据源），「当前登录」与「账号数」在其上各取一次。
 *   快照走 `useCachedResource` + **本 hook 自己的 60 秒轮询**（见 [`TRAE_SCOPE_STATUS_POLL_MS`]）。
 *
 * ## 为什么 Trae 侧要在 WorkBuddy 作用域下也探测
 *
 * 作用域菜单要并排展示 4 个作用域 ⇒ Trae 的状态必须可得，否则菜单里两条 Trae
 * 永远是「未检测到」。这与旧的「不在 Trae 分区就不探测」的取舍不同：那时只有
 * 侧栏一颗圆点，现在是一张跨作用域的状态表 —— 信息的范围变了，取数范围随之改变。
 */

import { useEffect, useMemo } from "react";

import * as api from "@/lib/api";
import type { AccountMeta, AppStatus } from "@/lib/types";
import type { TraeRegionId, TraeVariantStatus } from "@/lib/trae-types";
import type { Scope } from "@/lib/scope";
import {
  findRegionStatus,
  loadTraeVariantLogins,
  loadTraeVariantStatuses,
  type TraeVariantLogins,
} from "@/lib/trae-variant-status";
import { useCachedResource } from "@/lib/use-cached-resource";
import { useDocumentVisible } from "@/lib/use-document-visible";
import { useAccountsStore } from "@/stores/accounts";
import { identityDisplayName } from "@/components/account-card";

/** 单个作用域的环境状态。 */
export interface ScopeStatus {
  /** 该作用域的客户端是否已安装。 */
  installed: boolean;
  /** 该作用域的客户端是否在运行。 */
  running: boolean;
  /** 当前登录账号展示名；未登录 / 读不到时为 `null`。 */
  loggedInName: string | null;
  /** 账号库里的账号数。 */
  accountCount: number;
  /**
   * 「需处理」账号数（凭据已过期 / 需重新登录）—— 原型 `scope-menu` 的
   * `· N 个需处理`（`buddy-switch-prototype.html:3889`）。
   *
   * ⚠️ **`null` = 未知，不是「0」**：Trae 侧的聚合快照只取账号**总数**
   * （逐账号的过期 / 重登事实要再拉一次账号列表）。不知道就说不知道 ——
   * 菜单里**不渲染**该段，而不是用「0 个需处理」暗示「那边没事」。
   */
  attentionCount: number | null;
}

/** 四个作用域的状态表。 */
export type ScopeStatusMap = Record<Scope, ScopeStatus>;

/** 尚未取到数据时的占位（诚实：不确定就不说已安装 / 已登录）。 */
const UNKNOWN_STATUS: ScopeStatus = {
  installed: false,
  running: false,
  loggedInName: null,
  accountCount: 0,
  attentionCount: null,
};

/**
 * Trae 聚合快照的缓存键。
 *
 * ⚠️ **与 `TRAE_VARIANTS_KEY` 分开**：后者只装 `get_trae_variants` 的探测结果
 * （侧栏运行点直接复用），本快照在其上**再加**两次登录态 / 账号数请求。
 * 合成一个键会让「只想读运行点」的调用方被迫承担那两次请求。
 */
const TRAE_SCOPE_STATUS_KEY = "trae:scope-status";

/**
 * 快照的新鲜窗口：**50 秒**。
 *
 * 它是 [`TRAE_SCOPE_STATUS_POLL_MS`] 的**减数**，两个值必须满足「窗口 < 间隔」。
 */
const TRAE_SCOPE_STATUS_FRESH_MS = 50 * 1000;

/**
 * Trae 侧快照的轮询间隔：**60 秒**（= 窗口 50 秒 + 10 秒余量），与侧栏那颗运行点同档期
 * （`App.tsx` 的 `useTraeVariantRunning` 也是 60 秒）。
 *
 * ## ★ 为什么是「派生」而不是另写一个 `60 * 1000`
 *
 * 窗口与间隔**相等**时，tick 会因为「距上次写入还差几十毫秒不到一个窗口」被 `load`
 * 的窗口闸门跳过（`at` 写在请求**结束**之后，所以 tick 那一刻的年龄总是略小于间隔）
 * ⇒ 实际节奏退化成**两分钟一次**，而且命中与否由定时器抖动决定。
 * 那正是「看着有定时器、数据却不更新」的假修复。写成派生值后这条约束由**类型之外的
 * 算术**保证：改窗口不会悄悄把轮询打回两倍周期。
 *
 * ## 为什么只在可见时轮询
 *
 * 一次快照 = 进程探测（`tasklist` 子进程）+ 两次账号列表。窗口被隐藏时用户看不见
 * 任何变化，代价却照付。
 */
const TRAE_SCOPE_STATUS_POLL_MS = TRAE_SCOPE_STATUS_FRESH_MS + 10 * 1000;

interface TraeScopeSnapshot {
  cn: ScopeStatus;
  global: ScopeStatus;
}

/**
 * 取一个区域下「当前登录」的展示名：区域有两条程序位（TraeWork / TraeCode），
 * 取**第一条有登录态**的 —— 与原型 `currentAccount()` 的口径一致。
 */
function regionLoginName(
  statuses: TraeVariantStatus[],
  logins: TraeVariantLogins,
  region: TraeRegionId,
): string | null {
  for (const program of findRegionStatus(statuses, region)?.programs ?? []) {
    const login = logins[program.variant];
    if (login) return login.name;
  }
  return null;
}

async function loadTraeScopeSnapshot(): Promise<TraeScopeSnapshot> {
  const statuses = await loadTraeVariantStatuses();
  // 三次请求互不依赖：登录态按程序位、账号数按区域（国内取任一条国内程序位即可；
  // 2026-10-15 收敛后用合并版 `trae_merged`，账号库是区域级的，取哪条程序位等价）。
  const [logins, cnAccounts, globalAccounts] = await Promise.all([
    loadTraeVariantLogins(statuses),
    api.getTraeAccounts("trae_merged").catch(() => null),
    api.getTraeAccounts("global").catch(() => null),
  ]);

  const build = (region: TraeRegionId, accountCount: number): ScopeStatus => {
    const entry = findRegionStatus(statuses, region);
    return {
      installed: entry?.installed ?? false,
      running: entry?.running ?? false,
      loggedInName: regionLoginName(statuses, logins, region),
      accountCount,
      // 本次只取了总数 ⇒ 「需处理」**未知**（不拿 0 冒充，见 `ScopeStatus.attentionCount`）。
      attentionCount: null,
    };
  };

  return {
    cn: build("cn", cnAccounts?.total ?? 0),
    global: build("global", globalAccounts?.total ?? 0),
  };
}

/** WorkBuddy 一个区域的切片 → 状态（登录名优先昵称、其次邮箱）。 */
function workbuddyStatus(status: AppStatus | null, accounts: AccountMeta[]): ScopeStatus {
  const now = Date.now();
  return {
    // 后端缺省 `installed` 时按「账号库非空即视为装过」兜底（与账号页口径一致）。
    installed: status?.installed ?? accounts.length > 0,
    running: Boolean(status?.running),
    loggedInName: identityDisplayName(status?.current) || null,
    accountCount: accounts.length,
    // 与侧栏账号徽章的警示判据**同一口径**（原型的 `expiredCount()`）：过期 or 需重登。
    attentionCount: accounts.filter(
      (a) => a.needsRelogin || (a.expiresAt != null && a.expiresAt <= now),
    ).length,
  };
}

/**
 * 四作用域状态表 + Trae 侧的强制刷新入口。
 *
 * `refreshTrae` 供顶栏「刷新当前作用域」在 Trae 作用域下使用；WorkBuddy 侧刷新走
 * 账号 store 自己的命令（见 `Layout` 的刷新实现）。
 *
 * ## ★ Trae 侧为什么要自己带轮询（本 hook 的一处**已修缺陷**）
 *
 * WorkBuddy 侧的两条状态由账号 store 维护，store 自带 60 秒轮询
 * （`useWorkbuddyStatusRefresh`）⇒ 顶栏是活的。
 * Trae 侧只有一份 `useCachedResource` 快照，而 `useCachedResource` **只负责
 * 「挂载时取一次 + 窗口内复用」，它没有定时器** —— 于是 Trae 的三个事实
 * （已安装 / 运行中 / 当前登录）**只有点顶栏 ⟳ 才会更新**：用户看到的正是
 * 「这个不会自动刷新，只能手动刷」。本 hook 因此自己起一个 60 秒轮询。
 *
 * 两个细节都不是随手写的：
 *
 * - 用 `revalidate()` 而不是 `refresh()`：本 hook 有**两个消费者**同时挂载
 *   （顶栏所在的 `Layout` 与概览页），各自建定时器调 `refresh()` 会让每个周期
 *   发两遍请求（一次快照 = 进程探测 + 两次账号列表）。`revalidate` 走 `freshMs`
 *   窗口 ⇒ 后到的那次直接返回，一个周期只有一次真请求。
 * - 只在可见时轮询：见 [`TRAE_SCOPE_STATUS_POLL_MS`]。
 */
export function useScopeStatus(): {
  statuses: ScopeStatusMap;
  refreshTrae: () => Promise<void>;
} {
  const cnStatus = useAccountsStore((s) => s.status);
  const cnAccounts = useAccountsStore((s) => s.accounts);
  const globalStatus = useAccountsStore((s) => s.global.status);
  const globalAccounts = useAccountsStore((s) => s.global.accounts);

  const trae = useCachedResource<TraeScopeSnapshot>(
    TRAE_SCOPE_STATUS_KEY,
    loadTraeScopeSnapshot,
    { freshMs: TRAE_SCOPE_STATUS_FRESH_MS },
  );

  const visible = useDocumentVisible();
  // 取出函数本体再进依赖：`trae` 对象每次渲染都是新的，直接用它会每帧重建定时器。
  const revalidateTrae = trae.revalidate;

  useEffect(() => {
    if (!visible) return;
    const timer = window.setInterval(() => void revalidateTrae(), TRAE_SCOPE_STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, [visible, revalidateTrae]);

  const statuses = useMemo<ScopeStatusMap>(
    () => ({
      "wb-cn": workbuddyStatus(cnStatus, cnAccounts),
      "wb-global": workbuddyStatus(globalStatus, globalAccounts),
      "trae-cn": trae.data?.cn ?? UNKNOWN_STATUS,
      "trae-global": trae.data?.global ?? UNKNOWN_STATUS,
    }),
    [cnStatus, cnAccounts, globalStatus, globalAccounts, trae.data],
  );

  return { statuses, refreshTrae: trae.refresh };
}