import { useEffect, useMemo, useState } from "react";
import { Server } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { CatalogSourceBar } from "@/components/gateway/catalog-source-bar";
import { ModelTable } from "@/components/gateway/model-table";
import * as api from "@/lib/api";
import { describeCatalogSource, formatAbsoluteTime } from "@/lib/catalog-source";
import { useT } from "@/lib/i18n";
import { scopeOf } from "@/lib/scope";
import { servedCount } from "@/lib/types";
import type { GatewayModelRow, Region } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useGatewayStore } from "@/stores/gateway";

/**
 * 模型列表：来源条（实时 / 已保存 / 内置 + 上次识别 + 原因）+ 重新识别 + **模型表格**（P0-4 / P0-12 / P1-7 / P10 / P11）。
 *
 * ## 为什么来源条搬进共用壳 `CatalogSourceBar`
 *
 * 原来这里把徽标 + `updatedAt` + `note` 直接摊在 JSX 里，而 Trae 侧另写了一份同构的。
 * P10 §1.2 判定「结构完全相同、差异全是数据」⇒ 收敛到共用壳，本组件只负责**取数 + 组装 view**。
 *
 * ## 为什么按钮文案是「重新识别」而不是「刷新」
 *
 * 这个动作是**探测上游 `/v1/models`**（`refreshModels` → `catalogs.refresh`），不是重渲染。
 * 叫「刷新」会让用户以为只是重画一遍；叫「重新识别」才说明它在真的去拉清单。
 * 进行中用 `<Loader2>`（**按钮操作中**态，§3.6）—— 不是区块加载态，故**不用** `<Skeleton>`。
 *
 * ## 版本由页面传入（受控），组件内**不再自带版本选择器**
 *
 * 改造前这里有个内部 `Tabs`，与页面上的接入地址 / 账号池 / 接入指引各自为政：
 * 在这张卡里切到国际版，页面其他部分还停在国内版 —— 同一个「当前版本」在四个地方
 * 各说各话。现在与 Trae 侧一致：**页面持有唯一版本，本组件只负责渲染**。
 *
 * ## P11：chips → 表格 + 禁用 / 启用 + 计数 + 「两个出口」说明
 *
 * - **列集 = 模型 / 网关暴露**（WB **不渲染「上游状态」列** —— `available` 恒 `true`，
 *   恒同一个值就是噪音，§3.4 规则 1 / **决策 #3**）；**无归属列**（#1）、**无协议列**（#2）。
 * - `available` 在本侧**恒 `true`**（在清单里即可用）；`disabled` 来自后端偏好存储。
 * - **计数 = `available && !disabled`**（`servedCount`）—— 与出口 ①（`/v1/models` 过滤）**同源**：
 *   禁用后 −1、启用后恢复。
 * - **「两个出口」说明必须出现**：禁用的模型不会出现在 `/v1/models`，**直接调用也会被拒绝**
 *   并说明原因 —— 两个出口同时生效，否则禁用就是假开关。
 * - 切换按钮是**按钮操作中**态（`<Loader2>` + `disabled`）；展示字段过 `displayText`（白屏防线）。
 */
export function ModelList({ region, className }: { region: Region; className?: string }) {
  const t = useT();
  const [refreshing, setRefreshing] = useState(false);
  /** 正在切换禁用状态的行 id（驱动该行按钮的 `<Loader2>` + `disabled`）。 */
  const [pendingId, setPendingId] = useState<string | null>(null);
  const snapshot = useGatewayStore((s) => s.models[region]);
  const refreshModels = useGatewayStore((s) => s.refreshModels);
  const disabledSet = useGatewayStore((s) => s.disabled[region]);
  const loadDisabled = useGatewayStore((s) => s.loadDisabled);
  const setModelDisabled = useGatewayStore((s) => s.setModelDisabled);

  /* 进入 / 切 region 时读一次该 region 的禁用偏好（页面级 `loadAll` 也会拉，幂等）。 */
  useEffect(() => {
    void loadDisabled(region);
  }, [loadDisabled, region]);

  /* 来源视图由**快照**归一而来（契约在 `lib/catalog-source`）：`kind !== "live"` ⇒ 原因非空。 */
  const view = describeCatalogSource(snapshot, t);

  /* 行 = 上游清单（WB 侧 `available` 恒 true）+ 用户偏好（`disabled`）。 */
  const rows = useMemo<GatewayModelRow[]>(
    () =>
      (snapshot?.models ?? []).map((model) => ({
        id: model.id,
        name: model.name,
        scope: scopeOf("workbuddy", region),
        // WB 无「非可服务」概念：在清单里即可用（决策 #3）。
        available: true,
        disabled: disabledSet.has(model.id),
      })),
    [snapshot, disabledSet, region],
  );
  const { served, total } = servedCount(rows);

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

  async function onRescan() {
    setRefreshing(true);
    try {
      await refreshModels(region);
      toast.success(t("wbStats.gateway.refreshed"));
    } catch (e) {
      toast.error(t("wbStats.gateway.refreshFail"), { description: api.asError(e) });
    } finally {
      setRefreshing(false);
    }
  }

  /**
   * 切换某行的禁用状态。
   *
   * 失败时**不改内存集合**（`setModelDisabled` 先落库再更新）⇒ 行不会「假装切了」。
   * 错误经 `api.asError` 本地化后展示：后端对硬调用返回 403 / `type_name="model_disabled"`，
   * 文案已说明「被禁用」并给出路（T01），前端原样透出即可。
   */
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
        {/* 版本不在这里显示也不在这里切：页头切换器是唯一入口（与 Trae 侧同口径）。 */}
        <span className="text-sm font-semibold">{t("wbStats.gateway.modelList")}</span>
      </div>

      <div className="px-5 py-4">
        <CatalogSourceBar
          view={view}
          scanning={refreshing}
          onRescan={() => void onRescan()}
          rescanLabel={t("wbStats.gateway.rescan")}
          scanningLabel={t("wbStats.gateway.scanning")}
          identifiedAtTitle={view.identifiedAt ? formatAbsoluteTime(view.identifiedAt) : undefined}
        />
        {!snapshot || snapshot.models.length === 0 ? (
          <EmptyState
            icon={Server}
            title={t("wbStats.gateway.noModels")}
            description={t("wbStats.gateway.noModelsDesc")}
            action={
              <Button variant="outline" size="sm" onClick={() => void onRescan()} disabled={refreshing}>
                {t("wbStats.gateway.rescan")}
              </Button>
            }
          />
        ) : (
          <>
            {/* WB 不渲染「上游状态」列（恒可用，决策 #3）⇒ showUpstream={false}。 */}
            <ModelTable
              rows={rows}
              pendingId={pendingId}
              onToggleDisabled={(row, disabled) => void onToggleDisabled(row, disabled)}
              showUpstream={false}
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
