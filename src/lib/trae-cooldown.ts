/**
 * Trae 冷却态 → 「用户看到什么、该做什么」的**唯一映射**（issue #108 §P1-A）。
 *
 * ## 为什么需要这一层
 *
 * 同一份 `cooldownType`，改造前全仓三个界面给出三种说法：
 *
 * | 界面 | 对 `SessionDead` 的表述 | 隐含动作 |
 * |:--|:--|:--|
 * | 卡片（`trae-account-card`） | destructive「**需重新登录**」 | 去重新登录 |
 * | 表格（`TraeAccountsPage` 状态列） | warning「**冷却中**」 | 等它自己好 |
 * | 聚合页（`aggregate-accounts`） | **完全不看** `cooldownType` | 视为正常 |
 *
 * 三个界面各自从原始字段现推语义，于是「每加一个界面就多一种说法」，而没有一处
 * 负责回答「这个状态下用户到底该做什么」。
 *
 * ## 口径（与后端 `credits::classify_error` / `trae::pool::cooldown_of` 对齐）
 *
 * | `type` | 含义 | 下一步 |
 * |:--|:--|:--|
 * | `SessionDead` | 上游**真的**作废了会话（`12153`） | 重新登录；凭据换新后标记会自动消失 |
 * | `AuthRejected` | 401/403 但**没有**作废标记（token 过期，可自愈） | 等保活刷新，60 秒后重试 |
 * | `PlanLimit` | 套餐额度用尽 | 等额度重置或换号 |
 * | `SoftRate` / `Server` / `Client` / `BusinessError` / `NotFound` | 限流 / 上游抖动 | 等冷却结束 |
 */

import type { TranslationKey } from "@/locales/zh";

/** 徽标色调：`destructive` = 需要人工介入，`warning` = 会自己好。 */
export type TraeCooldownTone = "destructive" | "warning";

export interface TraeCooldownDisplay {
  /** 徽标词条 key（三个界面共用，不再各自写文案）。 */
  labelKey: TranslationKey;
  tone: TraeCooldownTone;
  /** 该状态的**真实出口**（tooltip 用）。 */
  hintKey: TranslationKey;
}

/**
 * 冷却态 → 展示口径；`null`/空串 ⇒ `null`（没有冷却，不渲染任何徽标）。
 *
 * ⚠️ 不要在这里判断「有没有到期」：到期与否由调用方按墙钟比，本函数只做语义映射。
 */
export function describeTraeCooldown(
  type: string | null | undefined,
): TraeCooldownDisplay | null {
  if (!type) return null;
  switch (type) {
    case "SessionDead":
      return {
        labelKey: "trae.comp.card.chip.needRelogin",
        tone: "destructive",
        hintKey: "trae.comp.card.cooldown.hint.sessionDead",
      };
    case "AuthRejected":
      return {
        labelKey: "trae.comp.card.chip.authRejected",
        tone: "warning",
        hintKey: "trae.comp.card.cooldown.hint.authRejected",
      };
    case "PlanLimit":
      return {
        labelKey: "trae.comp.card.chip.cooling",
        tone: "warning",
        hintKey: "trae.comp.card.cooldown.hint.planLimit",
      };
    default:
      return {
        labelKey: "trae.comp.card.chip.cooling",
        tone: "warning",
        hintKey: "trae.comp.card.cooldown.hint.transient",
      };
  }
}

/**
 * 「这条判定有多旧」——把 11 小时前的 401 与刚刚的失败区分开。
 *
 * 改造前卡片直接把 `cooldownReason` 原文渲染成一行红字，用户会读成「刚才又失败了」，
 * 于是反复重登、反复重启（正是 issue #108 的报障原文）。
 *
 * `since` 是**秒**（后端 `cooldownSince` 与 `cooldownUntil` 同单位）。
 * 返回 `null` 表示拿不到时刻（旧数据）——调用方不要补一个假值。
 */
export function cooldownAgeKey(
  since: number | null | undefined,
  now: number = Date.now(),
): { key: TranslationKey; minutes: number } | null {
  if (typeof since !== "number" || since <= 0) return null;
  const minutes = Math.max(0, Math.floor((now - since * 1000) / 60_000));
  if (minutes < 1) return { key: "trae.comp.card.cooldown.age.justNow", minutes };
  if (minutes < 60) return { key: "trae.comp.card.cooldown.age.minutes", minutes };
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return { key: "trae.comp.card.cooldown.age.hours", minutes: hours };
  return { key: "trae.comp.card.cooldown.age.days", minutes: Math.floor(hours / 24) };
}

/**
 * 该冷却是否**需要人动手**（`SessionDead`）——聚合页/侧栏的「待处理」用它。
 *
 * 其余档位会自己到期，把它们算进「待处理」会让那个数字永远不为 0，用户很快学会无视它。
 */
export function cooldownNeedsAttention(type: string | null | undefined): boolean {
  return type === "SessionDead";
}
