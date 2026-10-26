/**
 * 聚合页的**详情抽屉装配**（issue #50 · D4.2）。
 *
 * ## 为什么单独一层
 *
 * `AccountDetailDrawer` 是**零平台字符串**的完全可复用抽象（props 里没有一个
 * "WorkBuddy" / "Trae" 字样）。三个平台要喂给它的 `stats` / `sections` / `packs`
 * 形状相同、来源完全不同 —— 这段映射放在页面里会让页面涨到 800 行，且**无法被门禁断言**。
 * 抽成纯函数后：页面只负责挂回调；门禁可以断言「三个平台各自的装配都在」。
 *
 * ## 三条口径
 *
 * ① **缺数据不渲染那一项**，而不是渲染「0」或「—」。积分没刷新过 ⇒ 不给「积分」那一格；
 *    今日 Token 未知 ⇒ 不给那一格。多一格「—」只是噪声，还会让人以为「确实是 0」。
 * ② 「需处理」的判定**复用 `aggregateNeedsAttention`**，不在这里另写一遍 ——
 *    否则会出现「卡片说需重新登录、抽屉说正常」这种自相矛盾。
 * ③ 分组是**自由文本**（WB / Qoder）还是 **groupId**（Trae）必须分开：Trae 侧
 *    页面**不注入** `onSaveGroup`，抽屉就不会渲染那一块。
 *
 * ## 程序位徽标（issue #89 · 4.4 已补齐 Trae / Qoder）
 *
 * 「当前登录」在三个平台上的数据源都不是账号对象本身：
 * WB 靠 `program_status`（页面用一组 props 传进卡片）、Trae 靠 `trae_program_status`、
 * Qoder 靠 `qoder_program_status` —— 都是**按区域**的独立命令。
 *
 * ⚠️ 这里曾长期 `return []`（「聚合页没取，所以不给」），但**那句话早就过期了**：
 * 信号层后来取了 Trae / Qoder 的程序位（`ScopeSignals.traeStatuses` / `qoderPrograms`），
 * 只是页面没把它传进本函数。注释描述的是正确做法，代码没照做 —— 这类漂移最难自查，
 * 因为读注释会以为已经处理了。现在按平台分派：**取了就必须画**。
 */

import type { CreditExpiry } from "@/lib/types";
import type { QoderProgramStatus } from "@/lib/api";
import type { TraeProgram } from "@/components/trae-account-card";
import type {
  DetailPack,
  DetailProgramBadge,
  DetailRow,
  DetailSection,
  DetailStat,
} from "@/components/account-detail-drawer";
import { formatAbsoluteTime } from "@/lib/catalog-source";
import { displayText } from "@/lib/display-text";
import { formatCredits } from "@/components/usage/format";
import type { Translate } from "@/lib/i18n";
import { aggregateNeedsAttention, type AggregateAccount } from "@/lib/aggregate-accounts";

/** 抽屉装配需要的外部事实（页面已经有的，不为了打开抽屉再发一轮请求）。 */
export interface AggregateDetailContext {
  t: Translate;
  /** WorkBuddy 的积分快照（store `creditMap` 的对应 region）。 */
  credit?: CreditExpiry;
  /** Trae / Qoder 的积分（卡片上那个「剩余 / 总额」）。 */
  credits?: { remaining: number | null; total: number | null } | null;
  /** 今日 Token（缺席 = 未知，**不当 0**）。 */
  todayTokens?: number | null;
  /** 今日是否已签到（`null` / 缺席 = 未知）。 */
  todayCheckedIn?: boolean | null;
  /**
   * WorkBuddy **主程序位**当前登着的是不是这个账号。
   *
   * ⚠️ 只有主位能判：另外四个程序位（CLI / IDE / VS Code / JetBrains）的当前账号
   * 来自各自的 `*_status` 命令（按区域各一条），聚合页没拉 ⇒ 那一栏不画徽标，
   * 而不是画一排「未绑定」。
   */
  currentProgram?: boolean;
  /**
   * Trae 的程序位（`traeProgramsOf(signals, item)` 的结果，**跨区域已消歧**）。
   *
   * 页面已经从信号层算好了 —— 这里只做形状转换，不再发请求。
   */
  traePrograms?: TraeProgram[];
  /** Qoder 的程序位（信号层 `qoderPrograms`，按区域一份）。 */
  qoderPrograms?: QoderProgramStatus[];
}

const dash = "—";

/**
 * 平台程序位徽标。
 *
 * ★ **只给 WorkBuddy 主位一条**：主位的「当前登录」能从 `AppStatus.current` 判
 * （`isWorkbuddyCurrent`），而另外四个（CLI / IDE / VS Code / JetBrains）要各自的
 * `*_status` 命令（按区域各一条），聚合页没拉。
 * ⇒ 只给能判的那一条，**不画一排「未绑定」的假徽标**（那是「没数据」伪装成「没装」）。
 *
 * Trae / Qoder 的程序位状态在信号层里（`ScopeSignals.traeStatuses` / `qoderPrograms`），
 * 由页面按账号装配后传入。
 */
export function aggregateDetailPrograms(
  account: AggregateAccount,
  ctx: AggregateDetailContext,
): DetailProgramBadge[] {
  const { t } = ctx;

  /* Trae：`TraeProgram` 与 `DetailProgramBadge` 字段几乎一一对应
     （`installed` / `hasDataDir` / `current` 都是后端给的事实）。
     `key` 必须带区域 —— 两个区域可能各有一个同名程序位。 */
  if (account.platform === "trae") {
    return (ctx.traePrograms ?? []).map((program) => ({
      key: `${program.region}:${program.program}`,
      label: program.label,
      current: program.current,
      installed: program.installed,
      hasDataDir: program.hasDataDir,
    }));
  }

  /* Qoder：后端只给四态字符串 + `currentUid`，映射到徽标的两个布尔。
     `ready` / `empty` 都说明**装了且目录在**（差别是登录态），`unreadable` 是目录在但读不了，
     其余按未装处理 —— 与卡片上那枚胶囊的 `ready = state === "ready" && !running` 不冲突：
     那是「此刻能不能切」，这里是「装了没有」。 */
  if (account.platform === "qoder") {
    const uid = displayText(account.data.uid) ?? "";
    return (ctx.qoderPrograms ?? []).map((program) => ({
      key: program.id,
      label: program.label,
      current: program.currentUid !== null && program.currentUid === uid,
      installed: program.state !== "missing",
      hasDataDir: program.state === "ready" || program.state === "empty",
    }));
  }

  /* ⚠️ 只给**主位**（WorkBuddy 客户端本身）：另外四个程序位（CLI / IDE / VS Code /
     JetBrains）的当前账号来自各自的 `*_status` 命令，聚合页没拉 ——
     画一排「未绑定」是撒谎（那是「没数据」伪装成「没装」）。
     ⚠️ 主位**不在** `WORKBUDDY_PROGRAMS` 表里（那张表是「可切换的外部程序位」，
     主位是「把账号写进客户端自己的认证文件」，语义不同）。 */
  return [
    {
      key: "workbuddy",
      label: t("product.workbuddy"),
      current: Boolean(ctx.currentProgram),
      installed: true,
      hasDataDir: true,
    },
  ];
}

/** 顶部指标块。**只给有真值的项**（缺数据不渲染那一格）。 */
export function aggregateDetailStats(
  account: AggregateAccount,
  ctx: AggregateDetailContext,
): DetailStat[] {
  const { t } = ctx;
  const stats: DetailStat[] = [];

  if (account.platform === "workbuddy") {
    const credit = ctx.credit;
    if (credit?.ok && typeof credit.totalRemaining === "number") {
      const total =
        credit.resources && credit.resources.length > 0
          ? credit.resources.reduce((sum, item) => sum + (Number.isFinite(item.total) ? item.total : 0), 0)
          : null;
      stats.push({
        key: "credits",
        label: t("wbAccounts.card.metric.credits"),
        valueText: formatCredits(credit.totalRemaining),
        subText: total === null ? undefined : `/ ${formatCredits(total)}`,
      });
    }
  } else if (ctx.credits && ctx.credits.remaining != null) {
    stats.push({
      key: "credits",
      label: t("wbAccounts.card.metric.credits"),
      valueText: formatCredits(ctx.credits.remaining),
      subText: ctx.credits.total == null ? undefined : `/ ${formatCredits(ctx.credits.total)}`,
    });
  }

  if (ctx.todayTokens != null) {
    stats.push({
      key: "todayTokens",
      label: t("wbAccounts.card.metric.todayTokens"),
      valueText: compactTokens(ctx.todayTokens),
    });
  }

  if (ctx.todayCheckedIn != null) {
    stats.push({
      key: "checkin",
      label: t("aggregate.detail.checkin"),
      valueText: ctx.todayCheckedIn ? t("aggregate.detail.checkedIn") : t("aggregate.detail.notCheckedIn"),
      tone: ctx.todayCheckedIn ? undefined : "warn",
    });
  }

  const lastUsed = lastUsedText(account);
  if (lastUsed) {
    stats.push({ key: "lastUsed", label: t("aggregate.detail.lastUsed"), valueText: lastUsed });
  }

  return stats;
}

function compactTokens(value: number): string {
  if (value >= 1e6) return `${(value / 1e6).toFixed(1)}M`;
  if (value >= 1e3) return `${(value / 1e3).toFixed(1)}K`;
  return String(value);
}

/** ISO 字符串 → `YYYY-MM-DD HH:mm`（解析失败时原样截断，不编一个假时间）。 */
function isoText(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const ts = Date.parse(iso);
  if (Number.isFinite(ts)) return formatAbsoluteTime(ts);
  return iso.slice(0, 16).replace("T", " ");
}

function lastUsedText(account: AggregateAccount): string | null {
  if (account.platform === "workbuddy") {
    const ts = account.data.lastUsedAt;
    return typeof ts === "number" && ts > 0 ? formatAbsoluteTime(ts) : null;
  }
  /* Trae / Qoder 只有 `updatedAt`（ISO 字符串）—— 那是「这条记录最后被写」而不是
     「最后被使用」。文案用「最后使用」会撒谎，所以只在确实有值时才显示，
     并在 `aggregate.detail.lastUsed` 的词条里保持中性（见 locale 注释）。 */
  return isoText(account.data.updatedAt);
}

/** 信息区块：身份 + 凭据。 */
export function aggregateDetailSections(
  account: AggregateAccount,
  ctx: AggregateDetailContext,
): DetailSection[] {
  const { t } = ctx;
  const identity: DetailRow[] = [];
  const credential: DetailRow[] = [];

  if (account.platform === "workbuddy") {
    const a = account.data;
    if (a.email) identity.push({ label: t("aggregate.detail.email"), value: a.email });
    identity.push({ label: "UID", value: a.uid || a.id, mono: true });
    if (a.remark) identity.push({ label: t("aggregate.detail.remark"), value: a.remark });
    if (a.groupName) identity.push({ label: t("aggregate.detail.group"), value: a.groupName });

    credential.push({
      label: t("aggregate.detail.status"),
      value: aggregateNeedsAttention(account)
        ? t("aggregate.detail.credentialRelogin")
        : t("aggregate.detail.credentialOk"),
    });
    if (typeof a.expiresAt === "number" && a.expiresAt > 0) {
      credential.push({ label: t("aggregate.detail.expiresAt"), value: formatAbsoluteTime(a.expiresAt), mono: true });
    }
    if (typeof a.refreshExpiresAt === "number" && a.refreshExpiresAt > 0) {
      credential.push({
        label: t("aggregate.detail.refreshExpiresAt"),
        value: formatAbsoluteTime(a.refreshExpiresAt),
        mono: true,
      });
    }
  } else if (account.platform === "trae") {
    const a = account.data;
    identity.push({ label: "UID", value: a.userId, mono: true });
    if (a.remark) identity.push({ label: t("aggregate.detail.remark"), value: a.remark });
    if (a.deviceIdMasked) identity.push({ label: t("aggregate.detail.device"), value: a.deviceIdMasked, mono: true });

    credential.push({
      label: t("aggregate.detail.jwt"),
      value:
        a.jwtStatus === "ok"
          ? t("aggregate.detail.credentialOk")
          : a.jwtStatus === "warn"
            ? t("aggregate.detail.credentialExpiring")
            : a.jwtStatus === "expired"
              ? t("aggregate.detail.credentialExpired")
              : t("aggregate.detail.unknown"),
    });
    if (typeof a.jwtExpTimestamp === "number" && a.jwtExpTimestamp > 0) {
      credential.push({ label: t("aggregate.detail.expiresAt"), value: formatAbsoluteTime(a.jwtExpTimestamp), mono: true });
    }
    /* ⚠️ `cooldownUntil` 是 **epoch 毫秒**（与 `jwtExpTimestamp` 同口径），
       不是 ISO 字符串 —— 与 `updatedAt` 的形态不同，别混用 `isoText`。 */
    if (typeof a.cooldownUntil === "number" && a.cooldownUntil > 0) {
      credential.push({ label: t("aggregate.detail.cooldown"), value: formatAbsoluteTime(a.cooldownUntil), mono: true });
    }
  } else {
    const a = account.data;
    identity.push({ label: "UID", value: a.uid, mono: true });
    if (a.domain) identity.push({ label: t("aggregate.detail.domain"), value: a.domain });
    if (a.remark) identity.push({ label: t("aggregate.detail.remark"), value: a.remark });
    if (a.group) identity.push({ label: t("aggregate.detail.group"), value: a.group });

    credential.push({
      label: t("aggregate.detail.status"),
      value: a.hasCredential
        ? t("aggregate.detail.credentialOk")
        : t("aggregate.detail.credentialMissing"),
    });
    if (a.credentialSummary) {
      credential.push({ label: t("aggregate.detail.credentialSummary"), value: a.credentialSummary });
    }
    if (typeof a.expiresAt === "number" && a.expiresAt > 0) {
      credential.push({ label: t("aggregate.detail.expiresAt"), value: formatAbsoluteTime(a.expiresAt), mono: true });
    }
  }

  const sections: DetailSection[] = [];
  if (identity.length) {
    sections.push({ key: "identity", title: t("aggregate.detail.sectionIdentity"), rows: identity });
  }
  if (credential.length) {
    sections.push({ key: "credential", title: t("aggregate.detail.sectionCredential"), rows: credential });
  }
  return sections;
}

/**
 * 资源包明细。
 *
 * 只有**确实拿到包**时才给：WB 走 `credit.resources`，Trae 走 `creditPackages`，
 * Qoder 没有包的概念（按 credits 计费，只有剩余量）⇒ 空数组，抽屉不渲染该区块。
 */
export function aggregateDetailPacks(
  account: AggregateAccount,
  ctx: AggregateDetailContext,
): DetailPack[] {
  const { t } = ctx;

  const toPack = (input: {
    key: string;
    name: string;
    total: number;
    remaining: number;
    used: number;
    expireAt: number | null;
    expired: boolean;
    expiringSoon: boolean;
  }): DetailPack => ({
    key: input.key,
    name: input.name,
    remainingText: formatCredits(input.remaining),
    totalText: formatCredits(input.total),
    usedText: formatCredits(input.used),
    expiryText: input.expireAt ? formatAbsoluteTime(input.expireAt) : dash,
    expired: input.expired,
    expiringSoon: input.expiringSoon,
    ratio: input.total > 0 ? Math.max(0, Math.min(1, input.remaining / input.total)) : 0,
  });

  if (account.platform === "workbuddy") {
    const resources = ctx.credit?.ok ? (ctx.credit.resources ?? []) : [];
    return resources
      .filter((item) => item.total > 0 || item.remaining > 0)
      .map((item, index) =>
        toPack({
          key: `${item.packageCode ?? "pack"}-${index}`,
          name: displayText(item.packageName) || displayText(item.packageCode) || t("aggregate.detail.packFallbackName"),
          total: item.total,
          remaining: item.remaining,
          used: item.used,
          expireAt: item.expireAt,
          expired: item.expired,
          expiringSoon: item.expiringSoon,
        }),
      );
  }

  if (account.platform === "trae") {
    return (account.data.creditPackages ?? []).map((item, index) =>
      toPack({
        key: `${item.packageCode ?? "pack"}-${index}`,
        name: displayText(item.packageName) || t("aggregate.detail.packFallbackName"),
        total: item.total,
        remaining: item.remaining,
        used: item.used,
        expireAt: item.expireAt,
        expired: item.expired,
        expiringSoon: item.expiringSoon,
      }),
    );
  }

  return [];
}
