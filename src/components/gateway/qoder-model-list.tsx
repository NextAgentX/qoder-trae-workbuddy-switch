import { useEffect, useMemo, useState } from "react";
import { Cpu, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { CatalogSourceBar } from "@/components/gateway/catalog-source-bar";
import { ContextSourceCard } from "@/components/gateway/context-source-card";
import { CreditsCell, EffortCell } from "@/components/gateway/model-ability";
import { useImageCapColumn } from "@/components/gateway/model-image-column";
import { ModelEffortSelect } from "@/components/gateway/model-effort-select";
import { ModelTable } from "@/components/gateway/model-table";
import { DemoAction } from "@/components/demo-action";
import * as api from "@/lib/api";
import { formatAbsoluteTime, type CatalogSourceView } from "@/lib/catalog-source";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { QoderGatewayCatalog } from "@/lib/qoder-gateway";
import { scopeOf } from "@/lib/scope";
import { servedCount } from "@/lib/types";
import type { GatewayModelRow, Region } from "@/lib/types";
import { EMPTY_DISABLED_SET, useGatewayStore } from "@/stores/gateway";

/**
 * Qoder 模型清单 —— 上游清单 + **禁用的两个出口**（P11）。
 *
 * ## 数据源与另两条产品线的差别
 *
 * - WB / Trae：清单来自**本机缓存**（探测快照 / 客户端 `state.vscdb`）⇒ 来源条说「实时 / 已保存 / 内置」；
 * - Qoder：**没有本机缓存**，清单只能由网关**向官方拉**（要过 COSY 签名 + 账号凭据）
 *   ⇒ 来源三级变成「实时（上次拉取成功）/ 上次成功（本次失败，用缓存）/ 暂无清单（拉不到且无缓存）」。
 *   三者都**必须带原因**（`unavailable` / `cached` 两支后端已给 `note`，这里再兜一层 i18n，
 *   否则会出现「只有一个灰徽标」的猜谜态）。
 *
 * ## 列集与 WB 一致：**不渲染「上游状态」列**
 *
 * Qoder 的 `available` 与 WB 同理「在清单里即可服务」——恒为同一个值就是噪音
 * （§3.4 规则 1 / 决策 #3）。**上游事实**（∈ 清单）与**用户偏好**（`disabled`）
 * 仍是正交两维，只是前者恒真 ⇒ 由「网关暴露」列单独承载偏好。
 *
 * ## 未过滤清单 + 逐条 `disabled`
 *
 * 后端管理面给的是**全量**清单（禁用的也在），因此：
 * - 行渲染用的 `disabled` 取**store**（与网关出口 ① 同源，且切换后立即可见）；
 * - 计数 `served = total − 已禁用`（`servedCount`，与出口 ① 同源）。
 *
 * ## 2026-10-15 骨架对齐原型：卡头 → 来源条工具条 + 裸表 + 说明段
 *
 * 原型（`viewGateway()` 的 `models` 分支）= `catalogBar(来源条)` + 裸表 + 说明段，
 * **没有卡头**（「Qoder 模型清单」标题与左栏分区导航重复，已去掉）。
 * 表下的计数与「两个出口」说明即原型说明段，保持不变。
 */

export function QoderModelList({
  catalog,
  region,
  refreshing,
  onRefresh,
  maxContext,
  onToggleMaxContext,
  modelEfforts = {},
  onModelEffortChange,
  className,
}: {
  catalog: QoderGatewayCatalog;
  /** 本区域的禁用偏好来源（组件升成 `qoder-*` 作用域后读写）。 */
  region: Region;
  refreshing: boolean;
  onRefresh: () => void;
  /** 是否声明上游**最大**上下文窗口（默认 true）。 */
  maxContext: boolean;
  onToggleMaxContext: (checked: boolean) => void;
  /** 按模型的默认思考档：{ modelKey: effort } */
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  modelEfforts?: Record<string, string>;
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  onModelEffortChange: (modelKey: string, effort: string | null) => void;
  className?: string;
}) {
  const t = useT();
  /** 正在切换禁用状态的行 id（驱动该行按钮的 `<Loader2>` + `disabled`）。 */
  const [pendingId, setPendingId] = useState<string | null>(null);

  /** 本区域的**作用域**（禁用偏好按作用域键控；三平台互不相通）。 */
  const scope = scopeOf("qoder", region);
  const disabledSet = useGatewayStore((s) => s.disabled[scope]) ?? EMPTY_DISABLED_SET;
  const loadDisabled = useGatewayStore((s) => s.loadDisabled);
  const setModelDisabled = useGatewayStore((s) => s.setModelDisabled);

  /* 进入 / 切作用域时读一次该作用域的禁用偏好。 */
  useEffect(() => {
    void loadDisabled(scope);
  }, [loadDisabled, scope]);

  /* 行 = 上游全量清单 + 用户偏好（`disabled`）。 */
  const rows = useMemo<GatewayModelRow[]>(
    () =>
      catalog.rows.map((model) => ({
        id: model.id,
        name: model.name,
        scope,
        // Qoder 无「非可服务」概念：在清单里即可用（与 WB 同一判据，决策 #3）。
        available: true,
        disabled: disabledSet.has(model.id),
        // 本地按开关选档：点开关立即变，不等重新取数。
        context_window: maxContext
          ? (model.contextMax ?? model.maxInputTokens)
          : (model.contextDefault ?? model.maxInputTokens),
      })),
    [catalog, disabledSet, scope, maxContext],
  );
  const { served, total } = servedCount(rows);

  /** 行内要读「价格 / 思考档位」，按 id 回查官方清单（`GatewayModelRow` 不带这些字段）。 */
  const catalogById = useMemo(
    () => new Map(catalog.rows.map((model) => [model.id, model])),
    [catalog.rows],
  );

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
   * 来源视图（契约与 WB / Trae 共用，词汇不同）。
   *
   * ★ 原因**恒非空**（契约）：后端 `note` 优先，缺失时按来源档位兜底。
   */
  const view: CatalogSourceView = {
    kind: catalog.source,
    badgeLabel: t(
      catalog.source === "live"
        ? "qoder.gateway.models.sourceLive"
        : catalog.source === "cached"
          ? "qoder.gateway.models.sourceCached"
          : "qoder.gateway.models.sourceUnavailable",
    ),
    badgeVariant:
      catalog.source === "live" ? "success" : catalog.source === "cached" ? "secondary" : "warning",
    identifiedAt: catalog.fetchedAt,
    reason:
      catalog.source === "live"
        ? null
        : (catalog.note ??
          t(
            catalog.source === "cached"
              ? "qoder.gateway.models.reasonCached"
              : "qoder.gateway.models.reasonUnavailable",
          )),
    identifiedCount: catalog.rows.length,
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

  /* 图片输入列（issue #101）：Qoder 官方清单**没有**多模态字段 ⇒ `entryOf` 恒 `undefined`，
     这一列只有「本页手动指定 / 未声明」两种状态。别在这里读管理面视图里那个
     `supports_images`：它本来就是由覆盖表算出来的，绕一圈只会多一处可能漂移的判断。 */
  const imageColumn = useImageCapColumn({
    scope,
    entryOf: () => undefined,
  });

  return (
    <div className={cn(className)}>
      {/* 无卡头 —— 分区名由左栏导航承担（与 WB / Trae 侧同口径）。 */}
      <p className="mb-3 text-xs text-muted-foreground">{t("qoder.gateway.models.note")}</p>

      <ContextSourceCard
        checked={maxContext}
        onChange={(checked) => onToggleMaxContext(checked)}
      />

      <CatalogSourceBar
        view={view}
        scanning={refreshing}
        onRescan={onRefresh}
        rescanLabel={t("qoder.gateway.models.refresh")}
        scanningLabel={t("qoder.gateway.models.scanning")}
        identifiedAtTitle={view.identifiedAt ? formatAbsoluteTime(view.identifiedAt) : undefined}
      />

      {total === 0 ? (
        <EmptyState
          icon={Cpu}
          title={t("qoder.gateway.models.empty")}
          description={t("qoder.gateway.models.emptyDesc")}
          action={
            <DemoAction>
              <Button variant="outline" size="sm" onClick={() => onRefresh()} disabled={refreshing}>
                <RefreshCw />
                {t("qoder.gateway.models.refresh")}
              </Button>
            </DemoAction>
          }
          className="py-4"
        />
      ) : (
        <>
          {/* Qoder **不渲染「上游状态」列**（在清单里即可用，决策 #3）⇒ showUpstream={false}。 */}
          <ModelTable
            rows={rows}
            pendingId={pendingId}
            onToggleDisabled={(row, disabled) => void onToggleDisabled(row, disabled)}
            showUpstream={false}
            labels={labels}
            imageColumn={imageColumn}
            /* ★ issue #65：官方清单里的计费与能力事实（免费 / 峰谷价 / 思考档位）。
               与 WB / Trae **同一套两列**（`model-ability.tsx`），不再挤在一个「价格 / 思考」
               单元格里 —— 倍率是单值、档位是集合，混排既扫不出来也没法各自说明来源。
               错峰活动徽标仍挂在倍率格内（它是「这一刻多少钱」的一部分）。 */
            effortColumn={{
              header: t("shared.gateway.models.colEfforts"),
              render: (row) => {
                const model = catalogById.get(row.id);
                const efforts = model?.efforts ?? [];
                if (efforts.length === 0) {
                  return (
                    <EffortCell
                      efforts={[]}
                      defaultEffort={null}
                      unavailable={model?.isReasoning ? "unknown" : "unsupported"}
                    />
                  );
                }
                return (
                  <ModelEffortSelect
                    modelId={row.id}
                    efforts={efforts}
                    value={modelEfforts[row.id] ?? null}
                    onChange={onModelEffortChange}
                  />
                );
              },
            }}
            creditsColumn={{
              header: t("shared.gateway.models.colCredits"),
              render: (row) => {
                const model = catalogById.get(row.id);
                return (
                  <CreditsCell
                    rate={model?.priceFactor ?? null}
                    free={model?.isFree ?? false}
                    extra={
                      model?.promotionBadge ? (
                        <Badge
                          variant="warning"
                          className="px-1.5 py-0 text-[10px] leading-4"
                          title={
                            model.promotionWindow
                              ? t("qoder.gateway.models.promotionWindow", {
                                  range: model.promotionWindow,
                                })
                              : undefined
                          }
                        >
                          {model.promotionBadge}
                        </Badge>
                      ) : null
                    }
                  />
                );
              },
            }}
          />
          {/* 计数 = available && !disabled，与出口 ① 同源。
              ★ 守卫是 `total > 0`（**有清单就显示**）—— `served === 0` 不是「无数据」，
              而是「**全部被禁用**」这一真实且正需被看见的状态。 */}
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
