import { useEffect, useMemo, useState } from "react";
import { CloudDownload, Cpu, Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { CatalogSourceBar } from "@/components/gateway/catalog-source-bar";
import { ContextSourceCard } from "@/components/gateway/context-source-card";
import { ModelTable } from "@/components/gateway/model-table";
import { CreditsCell, EffortCell } from "@/components/gateway/model-ability";
import { useImageCapColumn } from "@/components/gateway/model-image-column";
import { DemoAction } from "@/components/demo-action";
import { TraeVariantMark } from "@/components/product-marks";
import * as api from "@/lib/api";
import { formatAbsoluteTime, type CatalogSourceView } from "@/lib/catalog-source";
import { displayText } from "@/lib/display-text";
import { useT } from "@/lib/i18n";
import { scopeOf } from "@/lib/scope";
import { servedCount } from "@/lib/types";
import type { GatewayModelRow, Region } from "@/lib/types";
import { cn } from "@/lib/utils";
import type { TraeClientModel, TraeModelSource, TraeVariantId } from "@/lib/trae-types";
import { useRefreshAction } from "@/lib/use-refresh-action";
import { EMPTY_DISABLED_SET, useGatewayStore } from "@/stores/gateway";

/**
 * Trae 模型清单 —— 读**客户端（上游下发）**的清单缓存。
 *
 * ## 为什么现在**有**刷新按钮（推翻了上一版的裁定）
 *
 * 上一版这里是静态常量（`buddy-switch-gateway` 的 `payload::MODEL_NAMES`），
 * 「刷新」永远不可能改变结果，因此当时判定为假控件、刻意不加按钮，只留一行说明。
 *
 * issue #4 之后数据源换成了**客户端 `state.vscdb` 里的上游清单缓存**
 * （见 core 的 `trae::model_list`）：客户端刷新过模型列表之后，这里重新读一次
 * **真的**会拿到新清单。按钮不再是假控件，因此恢复。
 *
 * ## ★ 来源条与 WB **共用壳**，但来源词汇**不统一**（P10 §1.2 / 待明确 #3）
 *
 * WB 的「来源」是**上游探测**（实时 / 已保存 / 内置）；Trae 的「来源」是**读客户端本地缓存**
 * （客户端缓存 / 未读取）—— 两者机制不同。若硬把 Trae 也改成三级，UI 会**撒谎**
 * （说「实时」其实是本地缓存）。因此只共用壳 `CatalogSourceBar`，词汇由本组件注入。
 * 按钮也因此叫「重新读取」（重读本地缓存），**不叫**「重新识别」（那会暗示探测上游）。
 *
 * ## 为什么卡上要有一个**程序位**选择器（issue #4 的后续报障）
 *
 * 清单是**客户端级**的，而一个区域下有两条程序位（TraeWork / TraeCode），
 * 两个客户端的上游清单**完全不同**（function 分组、模型集合都不一样）。
 * 上一版这里只读区域主程序（TraeWork）那一份，于是 TraeCode 专有的模型
 * （实测 `glm-5.3-flash`）**在界面上永远看不到** —— 用户报的就是这一条。
 *
 * 因此选择器是**取数据的一部分**，不是装饰：切到哪条程序位就读哪份缓存
 * （数据由宿主页按 `sources` 一次性取好，切换不发请求、不闪骨架）。
 *
 * ## 网关对外清单**按 API Key 的归属程序位**取（2026-09-30 起）
 *
 * `/v1/models` 与请求体的 `function` 都由 Key 的程序位决定，**两条程序位的清单不同**
 * （TraeWork → `solo_work_lite`，TraeCode → `chat_v3`；上游按 function 做白名单，
 * 2026-09-30 实测）。因此卡上展示的「网关对外 N 个」是**当前选中程序位**那一份 ——
 * 也就是「拿这个程序位的 Key 连本网关，外部客户端会看到什么」。
 *
 * ## 空态不是错误
 *
 * 客户端没启动过 / 没登录 / 还没拉过清单时，后端返回 `source = "missing"` 与
 * 一句可读的 `note`。这里按空态呈现，**不报错** —— 「少一张清单」远好过整页红。
 *
 * ## P11：分组 chips → **表格** + 禁用 / 启用 + 计数 + 「两个出口」说明
 *
 * - **列集 = 模型 / 上游状态 / 网关暴露**（Trae **渲染「上游状态」列** —— 它有 `servable`
 *   判据，该列有信息量；WB 无此判据，故不渲染，**决策 #3**）。**无归属列**（#1）、**无协议列**（#2）。
 * - **四组合可区分**：上游状态列只看 `available`（= ∈ 网关对外清单），网关暴露列只看 `disabled`。
 * - **扁平化 + 去重**：同一模型会在多个 function 分组里重复出现（实测 `solo_work_lite` 与
 *   `solo_work_remote` 内容完全相同）⇒ 表格按模型 id 去重，避免同一个模型出现两行、两个切换按钮。
 * - **计数 = `available && !disabled`**（`servedCount`）—— 与出口 ① **同源**。
 * - **「两个出口」说明必须出现**：禁用的模型不会出现在 `/v1/models`，**直接调用也会被拒绝**。
 *
 * ⚠️ 禁用偏好按**作用域**键控（2026-10-15 从 region 升级）：同区域两条程序位**共用**
 * 同一份禁用集合（同一作用域），跨平台不再串键。`region` 由宿主页传入（不是从 `sources` 猜），
 * 本组件把它升成 `trae-*` 作用域。
 *
 * ## 2026-10-15 骨架对齐原型：卡头 → 工具条 + 来源条 + 裸表 + 说明段
 *
 * 原型（`viewGateway()` 的 `models` 分支）= `catalogBar(来源条)` + 裸表 + 说明段，
 * 没有卡头（「Trae 模型清单」标题与左栏分区导航重复，已去掉）。
 * Trae 比原型多出的**程序位选择器 + 摘要**（卡头旧内容，属取数参数，见上方「为什么卡上
 * 要有一个程序位选择器」）改为一条与 `cat-bar` 同形的**工具条** —— 它不是卡头，是
 * 该分区的一处工具区（与来源条、`KeysBar` 同一套表面）。表下的计数与「两个出口」
 * 说明即原型说明段，保持不变。
 */
export function TraeModelList({
  sources,
  region,
  defaultModel,
  refreshing,
  onRefresh,
  maxMode,
  onToggleMaxMode,
  onFetchUpstream,
  fetchingUpstream = false,
  className,
}: {
  /**
   * 本区域**各程序位**的客户端清单（顺序 = 主程序在前）。
   *
   * 宿主页在**同一次**快照里把各程序位都取好（本地 SQLite 读，成本可忽略），
   * 因此切换程序位是纯前端行为，不需要新命令、也不会出现两份互不一致的快照。
   */
  sources: TraeModelSource[];
  /** 本区域的禁用偏好来源（region 由宿主页传入；本组件升成 `trae-*` 作用域后读写）。 */
  region: Region;
  /** 网关配置里的默认模型（仅用于摘要文案）。 */
  defaultModel: string;
  refreshing: boolean;
  onRefresh: () => void | Promise<void>;
  /** Max Mode 是否开启（与网关配置同源）。 */
  maxMode: boolean;
  onToggleMaxMode: (checked: boolean) => void;
  /**
   * ★ issue #173（**opt-in**）：用账号池凭据直连上游抓一次清单。
   *
   * 只在**客户端缓存读不到**的空态里出现 —— 那不是「少一个按钮」，而是把
   * 「headless 部署永远拿不到清单」这条死路打开：每次点击都是一次带账号 JWT 的
   * 上游请求，所以必须由用户显式触发，绝不自动跑。
   */
  onFetchUpstream?: (variant: TraeVariantId) => void | Promise<void>;
  fetchingUpstream?: boolean;
  className?: string;
}) {
  const t = useT();
  /** 「重新读取」的进行中态（点击后立刻转圈/禁用，覆盖 `refreshing` 之外的即时反馈）。 */
  const { busy, run } = useRefreshAction();

  /**
   * 用户手动选中的程序位。
   *
   * **不落 URL、不落 store**：它是本卡片的取数参数，属于「操作处的 L3 参数」
   * （与 IA 提案里 `Trae 程序位` 那一行的载体一致），不是页面级状态。
   *
   * 选中值不在 `sources` 里时（例如切了区域）自动回落 `sources[0]`（区域主程序），
   * 因此**不需要** effect 去同步/清理 —— 少一处会在切换瞬间闪一下的中间态。
   */
  const [picked, setPicked] = useState<TraeVariantId | null>(null);
  /** 正在切换禁用状态的行 id（驱动该行按钮的 `<Loader2>` + `disabled`）。 */
  const [pendingId, setPendingId] = useState<string | null>(null);
  const active = sources.find((source) => source.variant === picked) ?? sources[0] ?? null;
  const data = active?.data ?? null;

  /** 本区域的**作用域**（禁用偏好按作用域键控；同区域两条程序位共用一个作用域）。 */
  const scope = scopeOf("trae", region);
  const disabledSet = useGatewayStore((s) => s.disabled[scope]) ?? EMPTY_DISABLED_SET;
  const loadDisabled = useGatewayStore((s) => s.loadDisabled);
  const setModelDisabled = useGatewayStore((s) => s.setModelDisabled);

  /* 进入 / 切作用域时读一次该作用域的禁用偏好。 */
  useEffect(() => {
    void loadDisabled(scope);
  }, [loadDisabled, scope]);

  /** 当前程序位的**网关对外清单** id 集合（见 `TraeModelSource.gatewayNames`）。 */
  const servableNames = useMemo(
    () => (active?.gatewayNames ? new Set(active.gatewayNames) : null),
    [active],
  );

  /* 行 = 客户端全分组清单（去重）+ 上游事实（available）+ 用户偏好（disabled）。
     顺带产出 `ability`：id → 该模型在**本程序位**那份条目里的能力字段（档位 / 倍率）。

     ★ 为什么分组要**按「服务组优先」排序**：同一模型会在多个分组重复出现，而
       `reasoning_effort_config` / `consumption_rate` 是**按 (function, 模型) 下发**的
       —— 实测同一台机器上 `solo_work_lite` 22 条只有 1 条带档位，`chat_v3` 却有 15 条。
       若按字典序去重（改造前），用户看到的档位可能来自**另一个程序位**的分组，显示出来
       就是「看得见、用不了」—— 本仓最忌讳的那类假信息。服务组排最前 ⇒ 去重时它的数据胜出。 */
  const { rows, ability } = useMemo(() => {
    const groups = data?.groups ?? [];
    const isServed = (models: TraeClientModel[]): boolean => {
      if (!servableNames) return false;
      const names = new Set(models.filter((model) => !model.isBypass).map((model) => model.name));
      if (names.size !== servableNames.size) return false;
      for (const name of servableNames) if (!names.has(name)) return false;
      return true;
    };
    // 稳定排序：服务组在前，其余保持后端给的顺序（不改展示序）。
    const ordered = groups
      .map((group, index) => ({ group, index, served: isServed(group.models) ? 0 : 1 }))
      .sort((a, b) => a.served - b.served || a.index - b.index)
      .map((item) => item.group);

    const seen = new Set<string>();
    const out: GatewayModelRow[] = [];
    const byId = new Map<string, TraeClientModel>();
    for (const group of ordered) {
      for (const model of group.models) {
        // 白屏防线：`name` / `displayName` 运行时可能是对象，过 `displayText` 收敛。
        const id = displayText(model.name) ?? "";
        if (!id || seen.has(id)) continue;
        seen.add(id);
        byId.set(id, model);
        out.push({
          id,
          name: displayText(model.displayName) ?? id,
          scope,
          // 上游状态：∈ 网关对外清单即可服务；清单未取到（null）时不谎称「不可用」。
          available: servableNames ? servableNames.has(model.name) : true,
          disabled: disabledSet.has(id),
          // 本地按开关选档：点开关立刻变，不等后端重取。
          context_window: (() => {
            const windows = active?.gatewayContexts?.[model.name];
            if (!windows) return null;
            return maxMode ? (windows.max ?? windows.dev) : windows.dev;
          })(),
        });
      }
    }
    return { rows: out, ability: byId };
  }, [data, servableNames, active, disabledSet, scope, maxMode]);

  const { served, total } = servedCount(rows);
  /* ★ issue #173：直连上游抓到的清单也是「有数据」—— 此前只认 `client-cache`，
     于是 headless 部署里明明抓到了清单，界面仍显示空态（数据被界面判死）。 */
  const upstream = data?.source === "upstream";
  const loaded = (data?.source === "client-cache" || upstream) && total > 0;
  const gatewayCount = active?.gatewayNames?.length ?? null;

  const labels = {
    colModel: t("shared.gateway.models.colModel"),
    colContext: t("shared.gateway.models.colContext"),
    colUpstream: t("shared.gateway.models.colUpstream"),
    colExposed: t("shared.gateway.models.colExposed"),
    upstreamAvailable: t("shared.gateway.models.upstreamAvailable"),
    upstreamUnavailable: t("shared.gateway.models.upstreamUnavailable"),
    exposed: t("shared.gateway.models.exposed"),
    disabled: t("shared.gateway.models.disabled"),
    disable: t("shared.gateway.models.disable"),
    enable: t("shared.gateway.models.enable"),
    copyId: t("shared.gateway.models.copyId"),
    idCopied: t("shared.gateway.models.idCopied"),
  };

  /**
   * 来源视图（契约与 WB 共用，词汇不同）。
   *
   * - `kind` 只驱动**徽标语义色**（本地缓存 = 中性，未读取 = 警示）；
   * - `reason` 恒非空（契约）：后端 `note` 优先，缺失时按「客户端缓存 / 未读取」兜底。
   */
  const view: CatalogSourceView = {
    kind: upstream ? "live" : loaded ? "cached" : "builtin",
    badgeLabel: t(
      upstream
        ? "trae.gateway.models.sourceUpstream"
        : loaded
          ? "trae.gateway.models.sourceCache"
          : "trae.gateway.models.sourceMissing",
    ),
    badgeVariant: upstream ? "success" : loaded ? "secondary" : "warning",
    identifiedAt: data?.readAt ?? null,
    reason: upstream
      ? null
      : (displayText(data?.note) ??
        t(loaded ? "trae.gateway.models.reasonCache" : "trae.gateway.models.reasonMissing")),
    identifiedCount: total,
  };

  /** 切换某行的禁用状态；失败不改内存集合（先落库再更新）⇒ 行不会「假装切了」。 */
  async function onToggleDisabled(row: GatewayModelRow, disabled: boolean) {
    setPendingId(row.id);
    try {
      await setModelDisabled(scope, row.id, disabled);
      toast.success(
        t(
          disabled
            ? "shared.gateway.models.disabledToast"
            : "shared.gateway.models.enabledToast",
          { name: row.name },
        ),
      );
    } catch (e) {
      toast.error(t("shared.gateway.models.toggleFail"), { description: api.asError(e) });
    } finally {
      setPendingId(null);
    }
  }

  /* 图片输入列（issue #101）：Trae 的客户端清单**没有**多模态字段 ⇒ `entryOf` 恒 `undefined`，
     于是这一列只有「本页手动指定 / 未声明」两种状态 —— 如实反映「网关确实不知道」。
     要让它自动识别，只能等上游清单开始下发能力字段（或 WB 那条目录路径）。 */
  const imageColumn = useImageCapColumn({
    scope,
    entryOf: () => undefined,
  });

  return (
    <div className={cn(className)}>
      {/* 与 WB / Qoder 同一位置、同一形态：模型页顶部一个上下文开关（Trae 叫 Max Mode）。 */}
      <ContextSourceCard
        checked={maxMode}
        onChange={onToggleMaxMode}
        title={t("trae.stats.api.gateway.maxMode")}
        description={t("trae.stats.api.gateway.maxModeDesc")}
      />

      {/* 工具条（无卡头）：程序位选择是**取数参数**（见头部文档「为什么卡上要有一个程序位选择器」），
          摘要随行 —— 与来源条 / `KeysBar` 同一套 `cat-bar` 表面（`bg-muted` + 圆角 + 10px/12px 内距）。 */}
      <div className="mb-3 flex flex-wrap items-center gap-2.5 rounded-[10px] bg-muted px-3 py-2.5">
        {sources.length > 1 && (
          <div
            className="inline-flex items-center gap-1 rounded-xl border border-border bg-muted/40 p-1"
            role="tablist"
            aria-label={t("trae.gateway.models.program")}
          >
            {sources.map((source) => {
              const selected = source.variant === active?.variant;
              return (
                <button
                  key={source.variant}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  onClick={() => setPicked(source.variant)}
                  title={t("trae.gateway.models.programTip", { label: source.label })}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-xs outline-none transition-colors",
                    "focus-visible:ring-2 focus-visible:ring-ring/50",
                    selected
                      ? "bg-background font-medium text-foreground shadow-sm"
                      : "text-muted-foreground hover:bg-background/60 hover:text-foreground",
                  )}
                >
                  <TraeVariantMark variant={source.variant} size={15} />
                  <span>{source.label}</span>
                </button>
              );
            })}
          </div>
        )}
        <span className="text-xs text-muted-foreground">
          {t("trae.gateway.models.summary", { count: total, model: defaultModel })}
        </span>
      </div>

      <p className="mb-3 text-xs text-muted-foreground">{t("trae.gateway.models.note")}</p>

      <CatalogSourceBar
        view={view}
        scanning={refreshing}
        onRescan={onRefresh}
        rescanLabel={t("trae.gateway.models.refresh")}
        scanningLabel={t("trae.gateway.models.scanning")}
        identifiedAtTitle={view.identifiedAt ? formatAbsoluteTime(view.identifiedAt) : undefined}
      />

      {/* 读的是**哪个客户端**的缓存：同机两条程序位各有一份，必须写出来，
          否则用户看到数字对不上时无从判断「这是哪条线的清单」。 */}
      {active && (
        <div className="mb-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
          <span title={data?.dataDir ?? undefined}>
            {t("trae.gateway.models.readFrom", { label: active.label })}
          </span>
          {gatewayCount !== null && (
            <span>
              {active
                ? t("trae.gateway.models.gatewayCountFor", {
                    count: gatewayCount,
                    program: active.label,
                  })
                : t("trae.gateway.models.gatewayCount", { count: gatewayCount })}
            </span>
          )}
        </div>
      )}

      {!loaded ? (
        <EmptyState icon={Cpu}
          title={t("trae.gateway.models.empty")}
          description={t("trae.gateway.models.emptyDesc")}
          action={
            <>
              <DemoAction>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={refreshing || busy || fetchingUpstream}
                  onClick={() => void run(() => onRefresh())}
                >
                  {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                  {t("trae.gateway.models.refresh")}
                </Button>
              </DemoAction>
              {/* ★ issue #173：客户端缓存读不到时的**第二条来源**（headless / Docker 常见）。
                  明说「用账号凭据请求上游」，因为它确实会打一次上游 —— 不藏成本。 */}
              {onFetchUpstream && active ? (
                <DemoAction>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={refreshing || busy || fetchingUpstream}
                    title={t("trae.gateway.models.fetchUpstreamHint")}
                    onClick={() => void onFetchUpstream(active.variant)}
                  >
                    {fetchingUpstream ? <Loader2 className="animate-spin" /> : <CloudDownload />}
                    {fetchingUpstream
                      ? t("trae.gateway.models.fetchUpstreamBusy")
                      : t("trae.gateway.models.fetchUpstream")}
                  </Button>
                </DemoAction>
              ) : null}
            </>
          }
          className="py-4"
        />
      ) : (
        <>
          {/* Trae **渲染「上游状态」列**（有 `servable` 判据）⇒ showUpstream={true}。 */}
          <ModelTable
            rows={rows}
            pendingId={pendingId}
            onToggleDisabled={(row, disabled) => void onToggleDisabled(row, disabled)}
            showUpstream={true}
            labels={labels}
            imageColumn={imageColumn}
            /* ★ issue #65：档位 / 倍率都来自**客户端清单**（网关不参与解析）。
               `thinking === "unknown"` 必须显示成「未下发」—— TraeWork 的
               `solo_work_lite` 实测 22 条只有 1 条带档位，写成「不支持」就是撒谎。 */
            effortColumn={{
              header: t("shared.gateway.models.colEfforts"),
              render: (row) => {
                const model = ability.get(row.id);
                const thinking = model?.thinking ?? "unknown";
                return (
                  <EffortCell
                    efforts={model?.efforts ?? []}
                    defaultEffort={model?.defaultEffort ?? null}
                    unavailable={
                      thinking === "unsupported"
                        ? "unsupported"
                        : thinking === "unknown"
                          ? "unknown"
                          : "undeclared"
                    }
                  />
                );
              },
            }}
            creditsColumn={{
              header: t("shared.gateway.models.colCredits"),
              render: (row) => <CreditsCell rate={ability.get(row.id)?.creditsRate ?? null} />,
            }}
          />
          {/* 计数 = available && !disabled，与出口 ① 同源。
              ★ 守卫是 `total > 0`（**有清单就显示**），不是 `served > 0` ——
              `served === 0` 不是「无数据」，而是「**全部被禁用**」这一真实且正需被看见的状态。
              （WB 侧 `model-list.tsx` 已按同一判据改过；Trae 侧此前漏跟进，两平台
              在「全部禁用」时一个显示计数、一个不显示。） */}
          {total > 0 && (
            <p className="mt-3 text-xs text-muted-foreground" data-act="gw-model-count">
              {t("shared.gateway.models.count", { served, total })}
            </p>
          )}
          {/* ★ 两个出口同时生效的说明：否则用户不知道禁用真的生效了（会以为是假开关）。 */}
          <p className="mt-1 text-xs text-muted-foreground" data-act="gw-two-exits">
            {t("shared.gateway.models.twoExits")}
          </p>
        </>
      )}
    </div>
  );
}
