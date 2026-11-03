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
import type { QoderProgramStatus, QoderWebCreditPacks } from "@/lib/api";
import type { TraeProgram } from "@/components/trae-account-card";
import type {
  DetailPack,
  DetailProgramBadge,
  DetailRow,
  DetailSection,
  DetailStat,
} from "@/components/account-detail-drawer";
import { formatAbsoluteTime } from "@/lib/catalog-source";
import { secondsToMs, toDetailPacks, type CreditPackTexts } from "@/lib/credit-packs";
import { packState } from "@/lib/qoder-packs";
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
  /**
   * Qoder 的逐资源包明细（issue #111）。
   *
   * 与其它字段不同，这一份**必须现拉**（网页会话链路，只有桌面端拿得到）⇒
   * 页面在打开抽屉时按需取，取不到就是 `undefined`/`available:false`，
   * 此时抽屉不渲染资源包区块（不是渲染空包）。
   */
  qoderPacks?: QoderWebCreditPacks | null;
}

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
 * ★ **issue #144 复核 §5（L1 + L2）之后**：过滤（`remaining > 0`）、排序（已过期垫底 →
 * 到期升序 → 永久垫后）、`ratio`（0..100）全部交给 `credit-packs.ts`，本函数只剩
 * 「取数据 + 字段改名 + 量纲归一 + 文案」。此前这四件事在这里和三个渠道页各写一遍，
 * 于是同一批包在四个页面上排在不同位置、`ratio` 在一处漏乘 100、Trae 的秒被当毫秒。
 *
 * ★ **Trae 的 `expireAt` 是 Unix 秒**（后端直接透传上游 `expire_time`），
 * `* 1000` 只允许出现在 Trae 分支的这一行 —— 这是 #144 那个「到期列全是 1970」的根因。
 *
 * ⚠️ Qoder **有**包的概念（issue #111 已接入网页端逐资源包明细），但数据来自
 * **网页会话**链路（仅桌面端 + 需登录一次）⇒ 拿不到时返回空数组，抽屉不渲染该区块。
 * 与「该账号没有包」在界面上靠「有没有区块」区分不了，真正的引导放在 Qoder 账号页。
 */
export function aggregateDetailPacks(
  account: AggregateAccount,
  ctx: AggregateDetailContext,
): DetailPack[] {
  const { t } = ctx;

  if (account.platform === "workbuddy") {
    const resources = ctx.credit?.ok ? (ctx.credit.resources ?? []) : [];
    return toDetailPacks(
      resources.map((item) => ({
        key: `${item.packageCode ?? "pack"}-${item.expireAt ?? "none"}`,
        name:
          displayText(item.packageName) ||
          displayText(item.packageCode) ||
          t("aggregate.detail.packFallbackName"),
        total: item.total,
        remaining: item.remaining,
        used: item.used,
        /* WB 是**毫秒**（后端 `parse_timestamp_ms`）。 */
        expireAtMs: item.expireAt,
        expired: item.expired,
        expiringSoon: item.expiringSoon,
      })),
      AGGREGATE_PACK_TEXTS(t),
    );
  }

  if (account.platform === "trae") {
    /* ★ issue #144 复核 §4.1：「账号 + 其信号」的合并**在页面里做一次**
       （`AggregateAccountsPage` 的 `detailAccount`），所以这里读的字段与 Trae 渠道页
       **同名同义**（`account.creditPackages`）。此前这里多传一个 `ctx.traePacks` 侧信道，
       而账号视图本身不含该键 ⇒ 两条路径对「这个账号有哪些包」各答一次。 */
    const packs = account.data.creditPackages ?? [];
    return toDetailPacks(
      packs.map((item) => ({
        key: `${item.packageCode ?? "pack"}-${item.expireAt ?? "none"}`,
        name: displayText(item.packageName) || t("aggregate.detail.packFallbackName"),
        total: item.total,
        remaining: item.remaining,
        used: item.used,
        /* ★ 唯一允许出现 `* 1000` 的地方：Trae 上游给的是**秒**。 */
        expireAtMs: secondsToMs(item.expireAt),
        expired: item.expired,
        expiringSoon: item.expiringSoon,
      })),
      AGGREGATE_PACK_TEXTS(t),
    );
  }

  if (account.platform === "qoder") {
    const packs = ctx.qoderPacks;
    if (!packs?.available) return [];
    const now = Date.now();
    return toDetailPacks(
      packs.packs.map((pack, index) => {
        const state = packState(pack, now);
        return {
          key: pack.id ?? `qoder-pack-${index}`,
          name: qoderPackSourceName(t, pack.source),
          total: pack.limit,
          remaining: pack.remaining,
          used: pack.used,
          /* Qoder 是**毫秒**（`expiresAtMs`）。 */
          expireAtMs: pack.expiresAtMs ?? null,
          expired: state === "expired",
          /* Qoder 上游没有临期标记 ⇒ 用 `packState` 的窗口（14 天，与卡片浮层同源）。 */
          expiringSoon: state === "expiring",
        };
      }),
      AGGREGATE_PACK_TEXTS(t),
      now,
    );
  }

  return [];
}

/**
 * 聚合页抽屉的**文案层**。
 *
 * ⚠️ 刻意**不复用**渠道页的词条域（`wbAccounts.*` / `trae.comp.*` / `qoder.page.*`）：
 * 文案域不同，混用会串（既有判断，见 `qoderPackSourceName`）。共用的是逻辑，不是文案。
 */
function AGGREGATE_PACK_TEXTS(t: Translate): CreditPackTexts {
  return {
    remainingText: formatCredits,
    totalText: formatCredits,
    /* ★「已用」用**共用**词条（issue #144 复核 §3.4）：它不属于任何平台的词条域。
       此前聚合页写裸数字、渠道页写「已用 13.99」—— 同一个抽屉同一列两种说法。 */
    usedText: (value) => t("shared.credit.packUsed", { n: formatCredits(value) }),
    expiryText: (expireAtMs, expired) =>
      expired
        ? t("aggregate.detail.packExpired")
        : expireAtMs !== null
          ? formatAbsoluteTime(expireAtMs)
          : t("aggregate.detail.packForever"),
  };
}

/** 上游 `source` → 聚合页文案（**不复用** Qoder 页的词条：文案域不同，混用会串）。 */
function qoderPackSourceName(t: Translate, source: string | undefined): string {
  switch (source) {
    case "PLAN":
      return t("aggregate.detail.qoderPackPlan");
    case "RESOURCE_PACKAGE_SOURCE_BONUS":
      return t("aggregate.detail.qoderPackBonus");
    case "RESOURCE_PACKAGE_SOURCE_DEDICATED":
    case "DEDICATED":
      return t("aggregate.detail.qoderPackDedicated");
    default:
      return t("aggregate.detail.packFallbackName");
  }
}
