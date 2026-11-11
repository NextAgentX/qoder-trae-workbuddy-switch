import { useEffect, useMemo, useState } from "react";
import { Server } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { EmptyState } from "@/components/ui/empty-state";
import { CatalogSourceBar } from "@/components/gateway/catalog-source-bar";
import { ContextSourceCard } from "@/components/gateway/context-source-card";
import { ModelTable } from "@/components/gateway/model-table";
import { CreditsCell, EffortCell } from "@/components/gateway/model-ability";
import { useImageCapColumn } from "@/components/gateway/model-image-column";
import { focusProbeModel, PROBE_LIMIT_STATES } from "@/components/gateway/model-probe-card";
import * as api from "@/lib/api";
import { describeCatalogSource, formatAbsoluteTime } from "@/lib/catalog-source";
import { poolOf } from "@/lib/gateway";
import { useT } from "@/lib/i18n";
import { scopeOf } from "@/lib/scope";
import { servedCount } from "@/lib/types";
import type { GatewayModelRow, Region } from "@/lib/types";
import { cn } from "@/lib/utils";
import { EMPTY_DISABLED_SET, useGatewayStore, useProbeCells, useProbePassive } from "@/stores/gateway";

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
 *
 * ## 2026-10-15 骨架对齐原型：卡头 → 来源条工具条 + 裸表 + 说明段
 *
 * 原型（`viewGateway()` 的 `models` 分支）= `catalogBar(来源条)` + 裸表 + 说明段，
 * **没有卡头**（「模型清单」标题与左栏分区导航重复，已去掉）。
 * `CatalogSourceBar` 本身就是原型 `<div class="cat-bar">` 的复刻，形态不变；
 * 表下的计数（`shared.gateway.models.count`）与「两个出口」说明（`twoExits`）即原型的
 * 说明段，保持在表后**恒渲染**（见下方 `total > 0` 守卫的 ★ 说明）。
 */
export function ModelList({ region, className }: { region: Region; className?: string }) {
  const t = useT();
  const [refreshing, setRefreshing] = useState(false);
  /** 正在切换禁用状态的行 id（驱动该行按钮的 `<Loader2>` + `disabled`）。 */
  const [pendingId, setPendingId] = useState<string | null>(null);
  /** 本页所在**作用域**（禁用偏好按作用域键控；`models` 快照仍按 region 键控）。 */
  const scope = scopeOf("workbuddy", region);
  const snapshot = useGatewayStore((s) => s.models[region]);
  const refreshModels = useGatewayStore((s) => s.refreshModels);
  const disabledSet = useGatewayStore((s) => s.disabled[scope]) ?? EMPTY_DISABLED_SET;
  const loadDisabled = useGatewayStore((s) => s.loadDisabled);
  const setModelDisabled = useGatewayStore((s) => s.setModelDisabled);
  const config = useGatewayStore((s) => s.config);
  const saveConfig = useGatewayStore((s) => s.saveConfig);
  const loadModels = useGatewayStore((s) => s.loadModels);
  /**
   * 开关的本地草案值：点击立刻生效（不等保存 / 回读）。
   * 保存或浅读结束后清空，之后以 store 配置为准。
   */
  const [maxContextDraft, setMaxContextDraft] = useState<boolean | null>(null);
  /** 「上游连接方式」开关的本地草案值（同上：点击立刻生效，保存后清空）。 */
  const [upstreamProxyDraft, setUpstreamProxyDraft] = useState<boolean | null>(null);
  /* 限流探测：与探测卡/账号页台账读**同一份**派生数据
     （`useProbeCells(scope)` 实测 + `useProbePassive()` 被动底色）。
     ⚠️ 这里**不许**自己拼 `probe.cells`：后端作业单槽，单格重测会让那一列突然只剩 1 格。 */
  const status = useGatewayStore((s) => s.status);

  /* 进入 / 切作用域时读一次该作用域的禁用偏好（页面级 `loadAll` 也会拉，幂等）。 */
  useEffect(() => {
    void loadDisabled(scope);
  }, [loadDisabled, scope]);

  /* 来源视图由**快照**归一而来（契约在 `lib/catalog-source`）：`kind !== "live"` ⇒ 原因非空。 */
  const view = describeCatalogSource(snapshot, t);

  /* 行 = 上游清单（WB 侧 `available` 恒 true）+ 用户偏好（`disabled`）。 */
  const rows = useMemo<GatewayModelRow[]>(
    () =>
      (snapshot?.models ?? []).map((model) => ({
        id: model.id,
        name: model.name,
        scope,
        // WB 无「非可服务」概念：在清单里即可用（决策 #3）。
        available: true,
        disabled: disabledSet.has(model.id),
        // 本地按开关选档（后端两档原值都在）：点开关立即变，不等重取。
        context_window: (maxContextDraft ?? config.max_context)
          ? (model.context_length_max ?? model.context_window)
          : (model.context_window_default ?? model.context_window),
      })),
    [snapshot, disabledSet, scope, config.max_context, maxContextDraft],
  );
  const { served, total } = servedCount(rows);

  /**
   * 行内要读「思考档位 / 积分倍率」，按 id 回查目录原始条目
   * （`GatewayModelRow` 刻意不带这些字段：它是「上游事实 ∪ 用户偏好」的行模型，
   * 能力面属于目录，不属于行 —— 与 Qoder 侧同一做法）。
   */
  const catalogById = useMemo(
    () => new Map((snapshot?.models ?? []).map((model) => [model.id, model])),
    [snapshot],
  );

  /* 探测的账号轴（= 该 region 的池条目）与实测格索引（uid × 模型 id）。 */
  const accounts = useMemo(() => poolOf(status, region).accounts, [status, region]);
  const probedMap = useProbeCells(scope);
  /* 被动底色（与探测卡**同一份派生值**）：客户端台账 ∪ 池模型级冷却。 */
  const passiveMap = useProbePassive(region);

  /**
   * 某模型的探测汇总：`probed` = 本次实测覆盖的账号数；`limited` = 其中被限流/封禁的
   * （**只有三档算限流**）；`unavailable` = 其中**不可用但不是限流**的（上游排队 / 上游异常 /
   * 登录失效 / 无此模型 / 网络错误）；`passive` = 只在被动底色里见过的账号数，另按**来源**拆出
   * `passiveFlow`（池冷却 = 真实流量）/ `passiveLedger`（客户端台账 = 客户端日志）——
   * 两来源都有时 `source === "flow"` 且 `both` ⇒ 两边都计（§5.3 角标要能区分来源）。
   *
   * ★ `unavailable` 必须**单列**：只有 `limited` 时，探到「上游排队」的模型会显示成
   *   「全部可用」—— 那是撒谎（该模型此刻真的调不通）。
   */
  function modelProbeStatus(modelId: string): {
    probed: number;
    limited: number;
    unavailable: number;
    passive: number;
    passiveFlow: number;
    passiveLedger: number;
  } {
    let probed = 0;
    let limited = 0;
    let unavailable = 0;
    let passive = 0;
    let passiveFlow = 0;
    let passiveLedger = 0;
    for (const account of accounts) {
      const cell = probedMap[account.uid]?.[modelId];
      if (cell) {
        probed += 1;
        if (PROBE_LIMIT_STATES.includes(cell.state)) limited += 1;
        else if (cell.state !== "ok" && cell.state !== "skipped") unavailable += 1;
        continue;
      }
      const info = passiveMap[account.uid]?.[modelId];
      if (info) {
        passive += 1;
        if (info.source === "flow") {
          passiveFlow += 1;
          if (info.both) passiveLedger += 1;
        } else {
          passiveLedger += 1;
        }
      }
    }
    return { probed, limited, unavailable, passive, passiveFlow, passiveLedger };
  }

  /**
   * 「限流（探测）」单元格（原型 `probeModelCellHtml`）：
   * 本次实测过 ⇒ `全部可用` / `N/M 限流` / `N/M 不可用`；只被动见过 ⇒ `被动 N`；没探过 ⇒ `—`。
   * **点徽章 = 下钻**（收窄探测范围到这一个模型，并展开卡片）。
   */
  function renderProbeCell(modelId: string) {
    const { probed, limited, unavailable, passive, passiveFlow, passiveLedger } =
      modelProbeStatus(modelId);
    if (probed > 0) {
      const allOk = limited === 0 && unavailable === 0;
      const variant = allOk
        ? "success"
        : limited === probed
          ? "destructive"
          : "warning";
      const title = allOk
        ? t("shared.gateway.probe.colTooltipAllOk", { total: probed })
        : limited > 0
          ? t("shared.gateway.probe.colTooltipLimited", { total: probed, limited })
          : t("shared.gateway.probe.colTooltipUnavailable", {
              total: probed,
              unavailable,
            });
      return (
        <button
          type="button"
          data-act="gw-model-probe"
          className="inline-flex"
          title={title}
          onClick={() => focusProbeModel(modelId)}
        >
          <Badge variant={variant}>
            {allOk
              ? t("shared.gateway.probe.colAllOk")
              : limited > 0
                ? t("shared.gateway.probe.colLimited", { limited, total: probed })
                : t("shared.gateway.probe.colUnavailable", {
                    unavailable,
                    total: probed,
                  })}
          </Badge>
        </button>
      );
    }
    if (passive > 0) {
      return (
        <button
          type="button"
          data-act="gw-model-probe"
          className="inline-flex"
          title={t("shared.gateway.probe.colTooltipPassive", {
            n: passive,
            flow: passiveFlow,
            ledger: passiveLedger,
          })}
          onClick={() => focusProbeModel(modelId)}
        >
          <Badge variant="secondary">{t("shared.gateway.probe.colPassive", { n: passive })}</Badge>
        </button>
      );
    }
    return (
      <span className="text-xs text-muted-foreground" title={t("shared.gateway.probe.colTooltipNone")}>
        {t("shared.gateway.probe.colNone")}
      </span>
    );
  }

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

  /* 图片输入列（issue #101）：取数 / 三态判据 / 写入 / 提示全在共用装配器里，
     本页只告诉它「目录条目怎么按 id 取」（`catalogById` 就是上面那份回查表）。 */
  const imageColumn = useImageCapColumn({
    scope,
    entryOf: (id) => catalogById.get(id),
  });
  return (
    <div className={cn(className)}>
      {/* 无卡头 —— 分区名由左栏导航承担（与 Trae / Qoder 侧同口径）。
          版本也不在这里显示 / 切换：页头切换器是唯一入口。 */}
      <ContextSourceCard
        checked={maxContextDraft ?? config.max_context}
        onChange={(checked) => {
          setMaxContextDraft(checked);
          void (async () => {
            try {
              await saveConfig({ ...config, max_context: checked });
              // 清单数字按开关选档 ⇒ 保存后浅读一次（不强制打上游）。
              await loadModels(region);
            } finally {
              setMaxContextDraft(null);
            }
          })();
        }}
      />
      {/* 上游连接方式：默认**直连**。
          原因：后端 `reqwest` 不带 `no_proxy()` 时会读 `HTTPS_PROXY`，而该变量在代理
          客户端关闭后常以 User 作用域残留 ⇒ 上游请求被迫走本地代理，表现为
          「只有开着代理才能用」（实测 workbuddy.ai 直连可用、经本地代理被掐断 TLS）。
          复用 `ContextSourceCard`（它已支持标题/描述覆写），不新造组件。 */}
      <ContextSourceCard
        title={t("shared.gateway.models.upstreamProxyTitle")}
        description={t("shared.gateway.models.upstreamProxyDesc")}
        checked={upstreamProxyDraft ?? config.upstream_via_proxy}
        onChange={(checked) => {
          setUpstreamProxyDraft(checked);
          void (async () => {
            try {
              await saveConfig({ ...config, upstream_via_proxy: checked });
            } finally {
              setUpstreamProxyDraft(null);
            }
          })();
        }}
      />
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
            imageColumn={imageColumn}
            /* ★ issue #65：思考档位 / 积分倍率（三平台同构，渲染统一走
               `model-ability.tsx`）。档位空数组在 WB 侧只有一种成因：上游没声明、
               内置档位表也没条目 ⇒ `undeclared`。 */
            effortColumn={{
              header: t("shared.gateway.models.colEfforts"),
              render: (row) => (
                <EffortCell
                  efforts={catalogById.get(row.id)?.reasoning_efforts ?? []}
                  defaultEffort={catalogById.get(row.id)?.default_effort ?? null}
                  source={catalogById.get(row.id)?.efforts_source ?? null}
                  unavailable="undeclared"
                />
              ),
            }}
            creditsColumn={{
              header: t("shared.gateway.models.colCredits"),
              render: (row) => (
                <CreditsCell
                  rate={catalogById.get(row.id)?.credits_rate ?? null}
                  free={catalogById.get(row.id)?.free ?? false}
                />
              ),
            }}
            /* 「限流（探测）」列（P1 §5.4）：**仅 WB 传**；Trae / Qoder 不传 ⇒ 壳里不多出该列与
               任何 click handler（保持「每行 2 处可点区」的门禁断言）。 */
            extraColumn={{
              header: t("shared.gateway.models.colProbe"),
              render: (row) => renderProbeCell(row.id),
            }}
          />
          {/* 计数 = available && !disabled，与出口 ① 同源。
              ★ Q5/D39：守卫是 `total > 0`（**有清单就显示**），不是 `served > 0` ——
              `served === 0` 不是「无数据」，而是「**全部被禁用**」这一真实且正需被看见的状态。
              旧守卫恰好在最该出现的场景下把整条信息藏了起来，且与下一行「两个出口」说明
              **恒渲染**的处理自相矛盾（那里同样认为「禁用生效了必须看得见」）。
              文案沿用既有词条，不新增措辞（避免踩「禁止 N 个模型可用」的反向断言）。 */}
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
