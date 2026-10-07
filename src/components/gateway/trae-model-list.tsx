import { useEffect, useMemo, useState } from "react";
import { Cpu, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { CatalogSourceBar } from "@/components/gateway/catalog-source-bar";
import { ModelTable } from "@/components/gateway/model-table";
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
import type { TraeModelSource, TraeVariantId } from "@/lib/trae-types";
import { useGatewayStore } from "@/stores/gateway";

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
 * ⚠️ 禁用偏好按 **region** 键控（**决策 #4**）：同区域两条程序位**共用**同一份禁用集合，
 * 因此 `region` 由宿主页传入（不是从 `sources` 猜）。
 */
export function TraeModelList({
  sources,
  region,
  defaultModel,
  refreshing,
  onRefresh,
  className,
}: {
  /**
   * 本区域**各程序位**的客户端清单（顺序 = 主程序在前）。
   *
   * 宿主页在**同一次**快照里把各程序位都取好（本地 SQLite 读，成本可忽略），
   * 因此切换程序位是纯前端行为，不需要新命令、也不会出现两份互不一致的快照。
   */
  sources: TraeModelSource[];
  /** 本区域的禁用偏好键（**决策 #4**：region 粒度，两条程序位共用）。 */
  region: Region;
  /** 网关配置里的默认模型（仅用于摘要文案）。 */
  defaultModel: string;
  refreshing: boolean;
  onRefresh: () => void;
  className?: string;
}) {
  const t = useT();

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

  const disabledSet = useGatewayStore((s) => s.disabled[region]);
  const loadDisabled = useGatewayStore((s) => s.loadDisabled);
  const setModelDisabled = useGatewayStore((s) => s.setModelDisabled);

  /* 进入 / 切 region 时读一次该 region 的禁用偏好。 */
  useEffect(() => {
    void loadDisabled(region);
  }, [loadDisabled, region]);

  /** 当前程序位的**网关对外清单** id 集合（见 `TraeModelSource.gatewayNames`）。 */
  const servableNames = useMemo(
    () => (active?.gatewayNames ? new Set(active.gatewayNames) : null),
    [active],
  );

  /* 行 = 客户端全分组清单（去重）+ 上游事实（available）+ 用户偏好（disabled）。 */
  const rows = useMemo<GatewayModelRow[]>(() => {
    const groups = data?.groups ?? [];
    const seen = new Set<string>();
    const out: GatewayModelRow[] = [];
    for (const group of groups) {
      for (const model of group.models) {
        // 白屏防线：`name` / `displayName` 运行时可能是对象，过 `displayText` 收敛。
        const id = displayText(model.name) ?? "";
        if (!id || seen.has(id)) continue;
        seen.add(id);
        out.push({
          id,
          name: displayText(model.displayName) ?? id,
          scope: scopeOf("trae", region),
          // 上游状态：∈ 网关对外清单即可服务；清单未取到（null）时不谎称「不可用」。
          available: servableNames ? servableNames.has(model.name) : true,
          disabled: disabledSet.has(id),
        });
      }
    }
    return out;
  }, [data, servableNames, disabledSet, region]);

  const { served, total } = servedCount(rows);
  const loaded = data?.source === "client-cache" && total > 0;
  const gatewayCount = active?.gatewayNames?.length ?? null;

  const labels = {
    colModel: t("shared.gateway.models.colModel"),
    colUpstream: t("shared.gateway.models.colUpstream"),
    colExposed: t("shared.gateway.models.colExposed"),
    upstreamAvailable: t("shared.gateway.models.upstreamAvailable"),
    upstreamUnavailable: t("shared.gateway.models.upstreamUnavailable"),
    exposed: t("shared.gateway.models.exposed"),
    disabled: t("shared.gateway.models.disabled"),
    disable: t("shared.gateway.models.disable"),
    enable: t("shared.gateway.models.enable"),
  };

  /**
   * 来源视图（契约与 WB 共用，词汇不同）。
   *
   * - `kind` 只驱动**徽标语义色**（本地缓存 = 中性，未读取 = 警示）；
   * - `reason` 恒非空（契约）：后端 `note` 优先，缺失时按「客户端缓存 / 未读取」兜底。
   */
  const view: CatalogSourceView = {
    kind: loaded ? "cached" : "builtin",
    badgeLabel: t(loaded ? "trae.gateway.models.sourceCache" : "trae.gateway.models.sourceMissing"),
    badgeVariant: loaded ? "secondary" : "warning",
    identifiedAt: data?.readAt ?? null,
    reason:
      displayText(data?.note) ??
      t(loaded ? "trae.gateway.models.reasonCache" : "trae.gateway.models.reasonMissing"),
    identifiedCount: total,
  };

  /** 切换某行的禁用状态；失败不改内存集合（先落库再更新）⇒ 行不会「假装切了」。 */
  async function onToggleDisabled(row: GatewayModelRow, disabled: boolean) {
    setPendingId(row.id);
    try {
      await setModelDisabled(region, row.id, disabled);
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

  return (
    <Card className={cn("gap-0 py-0", className)}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <Cpu className="size-4 stroke-[1.75]" />
          {t("trae.gateway.models.title")}
        </div>
        <div className="flex flex-wrap items-center gap-3">
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
      </div>

      <div className="px-5 py-4">
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
              <DemoAction>
                <Button variant="outline" size="sm" onClick={() => onRefresh()} disabled={refreshing}>
                  <RefreshCw />
                  {t("trae.gateway.models.refresh")}
                </Button>
              </DemoAction>
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
            />
            {/* 计数 = available && !disabled，与出口 ① 同源；无暴露项时不渲染（§3.4 规则 1）。 */}
            {served > 0 && (
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
    </Card>
  );
}
