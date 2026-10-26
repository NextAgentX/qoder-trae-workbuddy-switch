import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DemoAction } from "@/components/demo-action";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { POOL_STATUS_LABELS } from "@/lib/gateway";
import { useT } from "@/lib/i18n";
import type { GatewayPoolAccount, GatewayPoolSummary } from "@/lib/types";
import type { TranslationKey } from "@/locales/zh";
import { cn } from "@/lib/utils";

/**
 * 「不指定」的哨兵值。
 *
 * Radix `Select` 不接受空字符串作为 `SelectItem` 的 value，而后端契约里
 * **空串才表示「不指定」**，因此 UI 层用一个不可能与 uid 冲突的哨兵值承载它，
 * 在边界处转换。
 */
export const AUTO_OPTION = "__auto__";
/** 「按积分择优」哨兵 —— 与后端 `pool::PREFERRED_MAX_CREDITS` **逐字相同**。 */
export const MAX_CREDITS_OPTION = "__max_credits__";
/** 「随机」哨兵 —— 与后端 `pool::PREFERRED_RANDOM` **逐字相同**。 */
export const RANDOM_OPTION = "__random__";
/**
 * 「顺序」哨兵 —— 与后端 `pool::PREFERRED_SEQUENTIAL` **逐字相同**（issue #102）。
 *
 * 语义：**粘住当前账号**，只有它被限流 / 冷却 / 排除 / 占满在途时才换下一个。
 * 与「随机」的目的正好相反 —— 随机把额度摊开，代价是同一段对话的上下文
 * 反复落到不同账号上、上游前缀缓存全 miss（用户报「token 消耗过快」正是这个）。
 */
export const SEQUENTIAL_OPTION = "__sequential__";

/** 五态计数格。 */
export type PoolTileKey =
  | "available"
  | "cooling"
  | "disabled"
  | "expired"
  | "zeroCredits"
  | "modelCooling"
  | "excluded";

/**
 * 默认六格（Trae 侧：积分过期有判据，另有「已排除」）。
 *
 * ★「已排除」是**互斥第六态**（被排除的号不能再算「可路由」）：后端已把它从
 * `available` 里扣掉并单独计数 ⇒ 卡片必须给它一格，否则「可用 2」而实际有 3 个号、
 * 少掉的那个去哪了无从得知（数字与事实不符）。
 */
export const DEFAULT_POOL_TILES: PoolTileKey[] = [
  "available",
  "cooling",
  "disabled",
  "expired",
  "zeroCredits",
  "excluded",
];

/**
 * WorkBuddy 侧的六格 —— **没有「积分过期」**，但有「模型冷却」。
 *
 * 后端池条目只存「快过期的积分**数量**」，不存有效期，没有判据 ⇒ 该 state 恒为 0。
 * 与其摆一个永远为 0 的假格子，不如不给（诚实 > 看起来对齐）。
 *
 * ★「已排除」（2026-10-17）：它是**互斥第六态**（被排除的号不能再算「可路由」），
 * 因此必须有自己的一格 —— 否则「可路由 4」与实际可选中 3 个互相矛盾。
 *
 * ★「模型冷却」（issue #88，2026-10-26）：此前 WB 侧**一格都没有**反映模型级冷却，
 * 于是「探测判出 429 → 池卡全绿」看起来像探测没生效。Qoder 侧早有这一格，
 * 缺它是实现不对称，不是「WB 没有这一维」—— WB 的 6004 同样是按模型报的。
 */
export const WB_POOL_TILES: PoolTileKey[] = [
  "available",
  "cooling",
  "disabled",
  "modelCooling",
  "zeroCredits",
  "excluded",
];

/**
 * Qoder 侧的五格 —— **没有积分两格**（池不建模积分），多一格**模型冷却**。
 *
 * 上游 429 是**按模型**报的（「…switch to the other models」）⇒ 一个账号可能
 * 「对 A 模型冷却、对 B 模型仍可服务」。这一格与 `cooling`（账号级）**正交**，
 * 是真数据、不是把零值摆出来凑数（与 WB 去掉「积分过期」同一判据）。
 */
export const QODER_POOL_TILES: PoolTileKey[] = [
  "available",
  "cooling",
  "disabled",
  "modelCooling",
  "excluded",
];

const TILE_META: Record<PoolTileKey, { labelKey: TranslationKey; tone: "ok" | "warn" | "danger" | "muted" }> = {
  available: { labelKey: "shared.gateway.pool.tile.available", tone: "ok" },
  cooling: { labelKey: "shared.gateway.pool.tile.cooling", tone: "warn" },
  disabled: { labelKey: "shared.gateway.pool.tile.disabled", tone: "danger" },
  expired: { labelKey: "shared.gateway.pool.tile.expired", tone: "warn" },
  zeroCredits: { labelKey: "shared.gateway.pool.tile.zeroCredits", tone: "muted" },
  modelCooling: { labelKey: "shared.gateway.pool.tile.modelCooling", tone: "warn" },
  excluded: { labelKey: "shared.gateway.pool.tile.excluded", tone: "danger" },
};

/**
 * 模型级冷却徽标（`modelCoolings`：`模型 → 解冻时刻`）。
 *
 * 与账号级 `cooling` **分开显示**的依据：模型冷却时账号仍可用于**其它**模型，
 * 混进「冷却中」会让人把「换个模型就好」误判成「这个号废了」。
 * Trae 侧没有这一维 ⇒ 字段缺失时返回 `null`（不渲染假徽标）。
 *
 * ★ 必须带**解冻时刻**（issue #88 补充）：此前界面上只有「N 个模型限流」几个字，
 * 时间只藏在 `title` 的模型名里 —— 用户无从判断「还要等多久」「是不是早就该好了」。
 * 上游 429 的恢复信息本来就是按模型给的，不展示等于把它丢掉。
 */
function modelCoolingBadge(account: GatewayPoolAccount, t: ReturnType<typeof useT>): ReactNode {
  const entries = Object.entries(account.modelCoolings ?? {});
  if (!entries.length) return null;
  const now = Date.now();
  // 只展示仍有效的（后端已过滤，这里再兜一次防时钟漂移导致的负数）。
  const live = entries.filter(([, until]) => Number.isFinite(until) && until > now);
  if (!live.length) return null;
  const models = live.map(([model]) => model).join("、");
  // 最早解冻的那个决定「什么时候能再用上其中一部分」。
  const earliest = Math.min(...live.map(([, until]) => until));
  const time = new Date(earliest).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return (
    <Badge
      variant="secondary"
      className="shrink-0 text-warning"
      title={t("shared.gateway.pool.modelCoolingHint", { models, time })}
    >
      {t("shared.gateway.pool.modelCoolingBadge", { count: live.length, time })}
    </Badge>
  );
}

function formatCredits(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return value.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "0";
  return new Intl.NumberFormat("zh-CN").format(value);
}

/**
 * 账号池卡 —— **WorkBuddy 与 Trae 共用同一张**（用户诉求：两边 API 页同一套交互）。
 *
 * 数据全部由调用方传入（状态来自各自的 `*_gateway_status`），卡片自身不发请求，
 * 因此两侧的差异只剩两处**数据驱动**的参数：
 * - [`tiles`]：计数格。Trae 五格，WorkBuddy 四格（无积分过期判据）。
 * - [`autoOptions`]：下拉里除账号之外的选项。三个网关页现在**传同一组四档**
 *   （不指定 / 按实时积分择优 / 随机 / **顺序**）—— 差异只在写回形态：
 *   WorkBuddy 存 `AccountStrategy`，Trae / Qoder 存哨兵串 `preferredUid`。
 *
 * ## 指定账号的语义（务必与后端一致）
 *
 * 它是**偏好而非约束**：指定账号不可用（冷却 / 禁用 / 零积分 / 已在本轮试过）时，
 * 网关会**回落到自动择优**，不会因为指定账号不可用而拒绝服务。
 * 因此这里不把「当前不可用」的账号禁用掉 —— 用户仍然可以指定它，等它恢复后自动生效。
 *
 * ## 2026-10-15 骨架对齐原型：去掉 `<Card>` 外壳
 *
 * 原型（`viewGateway()` 的 `pool` 分支）= 裸表 + 说明段（「选号优先级…指定是排他的」）。
 * 本卡里的**计数格（tiles）**、**「指定账号」下拉**、**「恢复」出口**与 **diagnose 列表**
 * 都是原型简化版没有的真实功能，一处不删 —— 重排为：工具条（下拉 + 累计请求，
 * 与 `KeysBar` / 来源条同一套 `cat-bar` 表面）→ 计数格 → 裸表 → 说明段 → diagnose。
 * 卡头标题（`title`）已去掉：三个调用点都不传它（版本由页头切换器承担），且无卡头可挂。
 */
export function AccountPoolCard({
  pool,
  accounts,
  diagnose,
  preferredUid,
  onPreferredUidChange,
  saving = false,
  totalRequests = null,
  tiles = DEFAULT_POOL_TILES,
  autoOptions,
  emptyHint,
  /**
   * 「恢复」出口（**可选插槽**）：`disabled` 是终态且落盘持久化，没有出口该账号
   * **永久不可用**（余额刷新被 `disabled` 挡住、`PoolEntry::revive` 在生产代码里
   * 零调用点）。未传则该按钮不渲染 —— 差异走**插槽**，不写 `platform === …`。
   */
  onRevive,
  /** 正在恢复的 uid：该行按钮显示「操作进行中」（不是加载态）。 */
  revivingUid = null,
  onExclude,
  excludingUid = null,
  className,
}: {
  pool: GatewayPoolSummary | null;
  accounts: GatewayPoolAccount[];
  /** 逐账号「为什么不能路由」的可读串。 */
  diagnose: string[];
  /** 当前指定的账号 uid；空串表示不指定。 */
  preferredUid: string;
  /** 选择变化时回调（空串 = 不指定）。 */
  onPreferredUidChange: (uid: string) => void;
  /** 保存中：禁用选择器，避免连点产生两次写入。 */
  saving?: boolean;
  /** 累计请求数（WorkBuddy 侧无该数据 ⇒ 不传）。 */
  totalRequests?: number | null;
  /** 计数格（默认五格；WorkBuddy 传 [`WB_POOL_TILES`]）。 */
  tiles?: PoolTileKey[];
  /** 下拉里「非账号」的选项（缺省只有「不指定」）。 */
  autoOptions?: { value: string; label: string }[];
  /** 空态文案（缺省用共用文案）。 */
  emptyHint?: ReactNode;
  /** 「恢复」出口；未传则不渲染该按钮（差异走插槽，见解构处的 ★ 说明）。 */
  onRevive?: (uid: string) => void;
  /** 正在恢复的 uid。 */
  revivingUid?: string | null;
  /**
   * 「排除 / 取消排除」出口。未传则不渲染（差异走**插槽**，不写 `platform === …`）。
   *
   * 语义（与后端一致）：排除是**硬约束** —— 自动选号、全冷却兜底、粘性、指定都不会用它；
   * 但它**不等于禁用**，账号本身仍可在客户端正常使用。文案必须把这点讲清，
   * 否则用户会以为「排除 = 把号弄坏了」。
   */
  onExclude?: (uid: string, excluded: boolean) => void;
  /** 正在切换排除状态的 uid。 */
  excludingUid?: string | null;
  className?: string;
}) {
  const t = useT();
  const options = autoOptions ?? [
    { value: AUTO_OPTION, label: t("shared.gateway.pool.preferredAuto") },
  ];
  const preferredInPool = preferredUid !== "" && accounts.some((a) => a.uid === preferredUid);
  // 「不指定」的取值是空串，但 Radix 不吃空串 value ⇒ 用哨兵值显示。
  const selectValue = preferredUid === "" ? AUTO_OPTION : preferredUid;

  return (
    <div className={cn(className)}>
      {/* 工具条：指定账号（偏好入口）+ 累计请求 —— 与 `KeysBar` / `CatalogSourceBar`
          同一套 `cat-bar` 表面（`bg-muted` + 圆角 + 10px/12px 内距）。
          标题（`title`）已去掉：无卡头且三个调用点都不传（版本由页头切换器承担）。 */}
      <div className="mb-3 flex flex-wrap items-center gap-2.5 rounded-[10px] bg-muted px-3 py-2.5">
        <span className="text-xs text-muted-foreground">
          {t("shared.gateway.pool.preferredLabel")}
        </span>
        <Select
          value={selectValue}
          onValueChange={(next) => onPreferredUidChange(next === AUTO_OPTION ? "" : next)}
          disabled={saving}
        >
          <SelectTrigger
            size="sm"
            className="w-48"
            aria-label={t("shared.gateway.pool.preferredAria")}
          >
            <SelectValue placeholder={t("shared.gateway.pool.preferredAuto")} />
          </SelectTrigger>
          <SelectContent>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
            {accounts.map((account) => (
              <SelectItem key={account.uid} value={account.uid}>
                {account.name}
              </SelectItem>
            ))}
            {/* 指定的账号已不在池中（被删除 / 换了产品线）：仍要显示出来，
                否则 Select 找不到匹配项会显示成空白，用户以为「没指定」。 */}
            {preferredUid !== "" && !preferredInPool && (
              <SelectItem value={preferredUid}>
                {t("shared.gateway.pool.preferredMissing")}
              </SelectItem>
            )}
          </SelectContent>
        </Select>
        <div className="grow" />
        {totalRequests !== null && (
          <span className="text-xs text-muted-foreground">
            {t("shared.gateway.pool.totalRequests", { count: formatCount(totalRequests) })}
          </span>
        )}
      </div>

      {/* 格数不同 ⇒ 断点不同：5 格一行铺满；6 格（Trae：多了「已排除」）在中等宽度下
          先折成 3 列，避免每格被压到放不下「积分过期」四个字。 */}
      <div
        className={cn(
          "grid gap-0 border-b border-border/60",
          tiles.length >= 6
            ? "grid-cols-2 sm:grid-cols-3 lg:grid-cols-6"
            : tiles.length >= 5
              ? "grid-cols-2 sm:grid-cols-5"
              : "grid-cols-2 sm:grid-cols-4",
        )}
      >
        {tiles.map((key, index) => {
          const meta = TILE_META[key];
          // 「可路由」格里的在途占满小字：`inFlightFull ⊆ available`，所以只在
          // available 格上注，不另立一格（瞬时态与故障态不能并排，见字段注释）。
          const inFlightFull = key === "available" ? (pool?.inFlightFull ?? 0) : 0;
          return (
            <div
              key={key}
              className={cn(
                "flex flex-col items-center justify-center px-3 py-4 text-center",
                index > 0 && "border-l border-border/60",
              )}
            >
              <span className="text-xs text-muted-foreground">{t(meta.labelKey)}</span>
              <span
                className={cn(
                  "mt-2 text-2xl font-semibold tabular-nums tracking-[-0.02em]",
                  meta.tone === "ok"
                    ? "text-success"
                    : meta.tone === "warn"
                      ? "text-warning"
                      : meta.tone === "danger"
                        ? "text-destructive"
                        : "text-muted-foreground",
                )}
              >
                {pool?.[key] ?? 0}
              </span>
              {inFlightFull > 0 ? (
                <span
                  className="mt-1 text-[11px] leading-tight text-muted-foreground"
                  title={t("shared.gateway.pool.tile.availableBusyHint")}
                >
                  {t("shared.gateway.pool.tile.availableBusy", { count: inFlightFull })}
                </span>
              ) : null}
            </div>
          );
        })}
      </div>

      {accounts.length === 0 ? (
        <p className="px-5 py-6 text-center text-sm text-muted-foreground">
          {emptyHint ?? t("shared.gateway.pool.empty")}
        </p>
      ) : (
        /* 原型 `viewGateway` :3056-3068 是**表格**（账号 / 积分 / 状态 / 选号）。
           「权重」「粘性」两列**不渲染**：后端 `status_list` 只给 uid / name / realm / status /
           credits / cooldown* / disabled，权重由选号算法**实时计算**、粘性只是会话复用（均无字段、
           无接口）⇒ 没有数据就不摆列，绝不臆造数字。 */
        <div className="overflow-x-auto" data-act="gw-pool-table">
          <table className="w-full border-collapse text-sm">
            <thead>
              <tr className="border-b border-border/60 text-left text-xs text-muted-foreground">
                <th scope="col" className="px-5 py-2 font-medium">
                  {t("shared.gateway.pool.colAccount")}
                </th>
                <th scope="col" className="px-2 py-2 font-medium">
                  {t("shared.gateway.pool.colCredits")}
                </th>
                <th scope="col" className="px-2 py-2 font-medium">
                  {t("shared.gateway.pool.colStatus")}
                </th>
                <th scope="col" className="px-5 py-2 text-right font-medium">
                  {t("shared.gateway.pool.colPick")}
                </th>
              </tr>
            </thead>
            <tbody>
              {accounts.map((account) => {
                const meta = POOL_STATUS_LABELS[account.status];
                const pinned = account.uid === preferredUid;
                return (
                  <tr key={account.uid} className="border-b border-border/40 last:border-0">
                    <td className="px-5 py-2.5">
                      <span className="text-sm font-medium">{account.name}</span>
                      {pinned && (
                        <Badge variant="secondary" className="ml-2 shrink-0">
                          {t("shared.gateway.pool.pinned")}
                        </Badge>
                      )}
                    </td>
                    <td className="px-2 py-2.5 text-xs text-muted-foreground tabular-nums">
                      {t("shared.gateway.pool.credits", {
                        credits: formatCredits(account.credits),
                      })}
                    </td>
                    <td className="px-2 py-2.5">
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge
                          variant={meta.tone === "danger" ? "destructive" : "secondary"}
                          className={cn(
                            "shrink-0",
                            meta.tone === "ok" && "text-success",
                            meta.tone === "warn" && "text-warning",
                          )}
                        >
                          {meta.label}
                        </Badge>
                        {/* ★ 只有**真的不可用**（冷却 / 禁用）时才复述原因。
                            `cooldownReason` 是墙钟到期的字段，后端已被门控（两平台的 `status_list`
                            都只在 `cooling || disabled` 时给），这里再兜一层 —— 否则一条早已过期的
                            `429 rate limit` 会挂在「可用」徽标旁边，读起来像「这个号被限流了」。
                            ★ 判据必须含 `disabled`：它与禁用原因**共用**后端同一个 `reason` 字段。 */}
                        {/* 模型级冷却：账号本身可用，只是**这个模型**在冷却。
                            必须与账号级 `cooling` 分开显示 —— 混在一起会让人把「换个模型就好」
                            误判成「这个号废了」。 */}
                        {modelCoolingBadge(account, t)}
                        {(account.status === "cooling" || account.status === "disabled") &&
                          account.cooldownReason && (
                            <span className="truncate text-xs text-muted-foreground">
                              {account.cooldownReason}
                            </span>
                          )}
                      </div>
                    </td>
                    <td className="px-5 py-2.5 text-right">
                      <div className="flex items-center justify-end gap-2">
                        {/* ★ 已禁用的账号**必须**给「恢复」出口：`disabled` 是终态且落盘持久化，
                            余额刷新救不了它（`thaw_hard_credit_if_recovered` 第一句就是
                            `if entry.disabled { return false }`）—— 没有这个按钮，该账号**永久不可用**。
                            与网关 API Key 的「吊销 ↔ 恢复」同构（已吊销行必须同时给两个出口）。 */}
                        {onRevive && account.disabled && (
                          <DemoAction>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 px-2"
                              data-act="pool-revive"
                              disabled={revivingUid !== null}
                              onClick={() => onRevive(account.uid)}
                            >
                              {revivingUid === account.uid ? (
                                <Loader2 className="size-3.5 animate-spin" />
                              ) : (
                                t("shared.gateway.pool.revive")
                              )}
                            </Button>
                          </DemoAction>
                        )}
                        {onExclude && (
                          <DemoAction>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 px-2"
                              data-act="pool-exclude"
                              disabled={excludingUid !== null}
                              title={
                                account.excluded
                                  ? "取消排除：让它重新参与选号"
                                  : "排除：网关不会再用它（含兜底与指定），账号本身仍可在客户端使用"
                              }
                              onClick={() => onExclude(account.uid, !account.excluded)}
                            >
                              {excludingUid === account.uid ? (
                                <Loader2 className="size-3.5 animate-spin" />
                              ) : account.excluded ? (
                                t("shared.gateway.pool.unexclude")
                              ) : (
                                t("shared.gateway.pool.exclude")
                              )}
                            </Button>
                          </DemoAction>
                        )}
                        <DemoAction>
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-7 px-2"
                            data-act="pool-pin"
                            /* ★ 唯一按状态禁用的例外：**已被排除**的号无法被指定。
                               排除是硬约束（后端选号 / 兜底 / 粘性 / 指定全跳过），
                               放行只会得到「按钮亮着、策略也写了、但请求永远不落它」的假状态。
                               冷却 / 禁用 / 零积分**不**禁用本按钮 —— 那是临时状态，
                               用户有权预先指定一个「等会儿恢复」的号（D36）。 */
                            disabled={saving || Boolean(account.excluded)}
                            /* 空串 = 不指定（回落「跟随登录态 / 自动择优」）；排他由后端单值策略保证
                               —— 同一 region 只有一个 pinned，故「同一时刻只有一个账号被指定」。 */
                            onClick={() => onPreferredUidChange(pinned ? "" : account.uid)}
                          >
                            {pinned ? t("shared.gateway.pool.unpin") : t("shared.gateway.pool.pin")}
                          </Button>
                        </DemoAction>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <div className="border-t border-border/60 px-5 py-3">
        <p className="text-xs leading-5 text-muted-foreground">
          {t("shared.gateway.pool.preferredHint")}
        </p>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">
          {t("shared.gateway.pool.exclusiveHint")}
        </p>
        {onRevive && (
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {t("shared.gateway.pool.reviveHint")}
          </p>
        )}
        {onExclude && (
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {t("shared.gateway.pool.excludeHint")}
          </p>
        )}
      </div>

      {diagnose.length > 0 && (
        <div className="border-t border-border/60 px-5 py-3">
          <p className="text-xs text-muted-foreground">
            {t("shared.gateway.pool.diagnoseNote")}
          </p>
          <ul className="mt-2 space-y-1">
            {diagnose.map((line) => (
              <li key={line} className="break-all font-mono text-xs text-muted-foreground">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
