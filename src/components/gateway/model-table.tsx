/**
 * 模型表格（P11 §1.6）—— WorkBuddy 与 Trae **共用**的表格壳。
 *
 * ## 为什么抽共用壳，而不是两个模型页各写一份
 *
 * 改造前 `model-list.tsx` 把模型渲染成 `flex-wrap` 的弹性格（chips），Trae 侧另写了一份
 * 同构的分组 chips —— **都没有状态列、没有禁用按钮、没有计数**。P11 要给两边都加上
 * 「禁用 / 启用」这一整套交互，若各写一份，四组合的判据（下面那两列）会各漂一份。
 * 沿用 P4 / P10 的分层结论：**能合并的合并，差异走数据**。
 *
 * ## ★ 四组合可区分（P11 的核心）
 *
 * `available`（上游事实）与 `disabled`（用户偏好）是**两个正交维度**，UI 必须能表达全部四种：
 *
 * | available | disabled | 上游状态列（仅 Trae） | 网关暴露列 |
 * |:--:|:--:|:---|:---|
 * | ✓ | ✗ | 可用 | 已暴露 + [禁用] |
 * | ✓ | ✓ | 可用 | 已禁用 + [启用] |
 * | ✗ | ✗ | 不可用 | 已暴露 |
 * | ✗ | ✓ | 不可用 | 已禁用 |
 *
 * ⇒ **两列独立驱动**：上游状态列只看 `row.available`，网关暴露列只看 `row.disabled`。
 * **不得**把两者压成一个字段（压了之后「上游不可用」与「你自己禁用了」就分不清）。
 *
 * ## 为什么列集里**没有**「归属」列、「协议」列
 *
 * - **无归属列**（决策 #1）：UI 只渲染**当前 region**（页面持有唯一 region）⇒ 全表同一 scope，
 *   归属列是常量、纯冗余；
 * - **无协议列**（决策 #2）：协议是**端点属性**、不是模型属性（同一模型两种协议都支持）⇒ 无数据支撑。
 *
 * ## 为什么「上游状态」列由 `showUpstream` 开关（而不是恒显）
 *
 * WorkBuddy 的 `available` **恒为 `true`**（在清单里即可用，无「非可服务」概念）⇒ 该列永远是
 * 同一个值，就是噪音（§3.4 规则 1「无数据不渲染」/ **决策 #3**）。Trae 有 `servable` 判据，
 * 该列有信息量，故 Trae 传 `showUpstream=true`。
 *
 * ## 硬约束（可证伪）
 *
 * - **每行只一个可点区**：禁用 / 启用按钮。**无行点击、无勾选**（与 `AccountTable` 不同，
 *   故**不复用**它 —— 后者强制勾选 + 打开详情）。
 * - **标灰仍可见**（D26）：`disabled` 行加 `row-off`（`opacity-60` + 虚边框），**仍渲染** ——
 *   彻底隐藏会让用户以为清单里没有它。
 * - **展示字段一律过 `displayText`**（白屏防线，issue #2）：`name` / `id` 运行时可能是对象，
 *   当 React 子节点渲染会卸载整棵树。
 * - **共用壳内禁止平台判断**：差异一律走 `rows`（数据）+ `showUpstream`（开关）+ `labels`（文案）。
 * - **切换按钮是「按钮操作中」态**（§3.6）：`<Loader2>` 在按钮内 + `disabled` 防重复点击，
 *   **不是**区块加载态（不用 `<Skeleton>`）。
 *
 * ## 为什么自定义组件（AGENTS.md 要求记录理由）
 *
 * 它是**纯展示的组合**（表头 + 行 + 徽标 + 一个按钮），由项目现有 `Badge` / `Button` 原语与
 * `cn()` 拼成，不涉及焦点管理 / 键盘交互，因此不需要 Radix 层。唯一的交互（切换按钮）已复用
 * `Button` 并包在 `DemoAction` 里（与其余网关控件同口径）。
 */

import { Ban, Loader2, RotateCcw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DemoAction } from "@/components/demo-action";
import { displayText } from "@/lib/display-text";
import { cn } from "@/lib/utils";
import type { GatewayModelRow } from "@/lib/types";

/** 已 i18n 的列头 / 状态文案 / 按钮标签（**无 `colScope`** —— 无归属列，决策 #1）。 */
export interface ModelTableLabels {
  colModel: string;
  colUpstream: string;
  colExposed: string;
  upstreamAvailable: string;
  upstreamUnavailable: string;
  exposed: string;
  disabled: string;
  disable: string;
  enable: string;
}

export interface ModelTableProps {
  rows: GatewayModelRow[];
  /** 正在切换的行 id（驱动按钮 `<Loader2>` + `disabled`）。 */
  pendingId: string | null;
  /** 点「禁用 / 启用」。`disabled` 是**目标态**（`true` = 禁用）。 */
  onToggleDisabled: (row: GatewayModelRow, disabled: boolean) => void;
  /** 是否渲染「上游状态」列 —— WB `false`（恒可用，§3.4 规则 1 / 决策 #3），Trae `true`。 */
  showUpstream: boolean;
  labels: ModelTableLabels;
}

export function ModelTable({
  rows,
  pendingId,
  onToggleDisabled,
  showUpstream,
  labels,
}: ModelTableProps) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-sm" data-act="gw-model-table">
        <thead>
          <tr className="border-b border-border/60 text-left text-xs text-muted-foreground">
            <th scope="col" className="px-2 py-2 font-medium">
              {labels.colModel}
            </th>
            {showUpstream && (
              <th scope="col" className="px-2 py-2 font-medium">
                {labels.colUpstream}
              </th>
            )}
            <th scope="col" className="px-2 py-2 font-medium">
              {labels.colExposed}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            // 白屏防线：`name` / `id` 运行时可能是对象，一律过 `displayText` 收敛成字符串。
            const id = displayText(row.id) ?? "";
            const name = displayText(row.name) ?? id;
            const pending = pendingId === row.id;
            return (
              <tr
                key={row.id}
                // 验收钩子（与 trae-model-list 的 `data-model` 同口径）：CDP / 断言据此
                // 逐个判「这一行可不可用 / 有没有被禁用」，不必靠 className / 文案反查。
                data-model={id}
                data-available={row.available ? "yes" : "no"}
                data-disabled={row.disabled ? "yes" : "no"}
                className={cn(
                  "border-b border-border/40 last:border-0",
                  // D26：被禁用**可见但标灰**（不彻底隐藏）。
                  row.disabled && "row-off border-dashed opacity-60",
                )}
              >
                <td className="px-2 py-2">
                  <span className="font-medium">{name}</span>
                  {name !== id && (
                    <code className="ml-1.5 font-mono text-[11px] text-muted-foreground">{id}</code>
                  )}
                </td>
                {showUpstream && (
                  <td className="px-2 py-2">
                    {/* 上游状态列**只看 `row.available`**。 */}
                    <Badge variant={row.available ? "success" : "warning"} className="rounded-md">
                      {row.available ? labels.upstreamAvailable : labels.upstreamUnavailable}
                    </Badge>
                  </td>
                )}
                <td className="px-2 py-2">
                  <div className="flex flex-wrap items-center gap-2">
                    {/* 网关暴露列**只看 `row.disabled`**。 */}
                    <Badge variant={row.disabled ? "secondary" : "success"} className="rounded-md">
                      {row.disabled ? labels.disabled : labels.exposed}
                    </Badge>
                    <DemoAction>
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 px-2"
                        data-act="gw-model-toggle"
                        disabled={pending}
                        onClick={() => onToggleDisabled(row, !row.disabled)}
                      >
                        {pending ? (
                          <Loader2 className="animate-spin" />
                        ) : row.disabled ? (
                          <RotateCcw />
                        ) : (
                          <Ban />
                        )}
                        {row.disabled ? labels.enable : labels.disable}
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
  );
}
