/**
 * 模型清单「来源条」（P10 §3.2）—— **两平台共用的壳**。
 *
 * ## 为什么抽共用壳，而不是两平台各写一份
 *
 * WB（`model-list.tsx`）与 Trae（`trae-model-list.tsx`）的来源条**结构完全相同**
 * （徽标 + 上次识别时间 + 原因 + 「接入后自动识别」提示 + 重新识别按钮），差异**全部是数据**：
 * 来源词汇、时间字段、计数来源、按钮文案。沿用 P4 的分层结论：**能合并的合并**。
 *
 * ## ★ 硬约束（可证伪）
 *
 * - **壳内禁止出现平台 / 来源分支**（`platform === …` / `source === "client-cache"`）：
 *   来源词汇一律由调用方经 `view.badgeLabel` 与按钮文案 props 注入。出现分支即说明
 *   合并过度，应退回「两份来源条 + 抽文案片段」（P10 §1.2 反向约束）。
 * - **`scanning` 是「按钮操作中」态**（§3.6）⇒ 用 `<Loader2 className="animate-spin">`
 *   + 文案（`scanningLabel`）+ `disabled`（防重复点击）。**不是**区块加载态，
 *   **不得**换成 `<Skeleton>` —— 那会把按钮变成骨架屏。
 * - **原因非空契约**：`view.reason` 非空时渲染为琥珀色小字；`live` 无原因时不渲染。
 * - **无数据不渲染**（§3.4 规则 1）：`view.identifiedCount === 0` 时不摆「已识别 0 个模型」。
 * - **计数只说「已识别」，禁止「可用」**（§3.17.3 陷阱 3 / D24）：HTTP 200 ≠ 模型可用。
 *
 * ⚠️ 时间 / 计数 / 提示 / 「来源」标签走**共享词条**（`shared.gateway.sourceBar.*`），
 *    故两平台同形；徽标与按钮文案由调用方注入（WB 三级 / Trae「客户端缓存 / 未读取」）。
 *
 * ## 为什么自定义组件（AGENTS.md 要求记录理由）
 *
 * 它不是交互原语，而是**纯展示的组合**（徽标 + 两行文字 + 一个按钮），由项目现有
 * `Badge` / `Button` 原语与 `cn()` 拼成，不涉及焦点管理 / 键盘交互 —— 不需要 Radix 层。
 * 唯一的交互（重新识别按钮）已复用 `Button` 并包在 `DemoAction` 里（与其余网关控件同口径）。
 */

import { Loader2, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DemoAction } from "@/components/demo-action";
import { formatRelativeTime, type CatalogSourceView } from "@/lib/catalog-source";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface CatalogSourceBarProps {
  /** 归一后的来源视图（§3.1）—— 壳只按它渲染。 */
  view: CatalogSourceView;
  /** 是否正在识别（驱动按钮「识别中…」+ disabled）。 */
  scanning: boolean;
  /** 点「重新识别」。 */
  onRescan: () => void;
  /**
   * 重新识别按钮的**空闲文案**（i18n 后）。
   *
   * ⚠️ 是 props 而非常量：WB 是「重新识别」（探测上游），Trae 是「重新读取」
   *    （读客户端本地缓存，**不探测上游** —— 统一措辞会撒谎，见 P10 待明确 #4）。
   */
  rescanLabel: string;
  /** 重新识别按钮的**进行中文案**（i18n 后）。 */
  scanningLabel: string;
  /** 可选：上次识别时间的绝对时间（悬停 `title`）。 */
  identifiedAtTitle?: string;
  className?: string;
}

export function CatalogSourceBar({
  view,
  scanning,
  onRescan,
  rescanLabel,
  scanningLabel,
  identifiedAtTitle,
  className,
}: CatalogSourceBarProps) {
  const t = useT();
  const relative = formatRelativeTime(view.identifiedAt, t);

  return (
    // `cat-bar` 是**稳定的验收钩子**（与 `trae-model-list` 的 `data-model` 同口径）：
    // CDP / 手工回归据此定位来源条，不靠 className 漂移。
    <div className={cn("cat-bar mb-3", className)}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span className="flex items-center gap-1.5">
          {t("shared.gateway.sourceBar.sourceLabel")}
          <Badge variant={view.badgeVariant} className="rounded-md">
            {view.badgeLabel}
          </Badge>
        </span>
        <span title={identifiedAtTitle}>
          {relative
            ? t("shared.gateway.sourceBar.identifiedAt", { time: relative })
            : t("shared.gateway.sourceBar.neverIdentified")}
        </span>
        {view.identifiedCount > 0 && (
          <span>{t("shared.gateway.sourceBar.count", { n: view.identifiedCount })}</span>
        )}
        <span className="ml-auto flex flex-wrap items-center gap-2">
          <span>{t("shared.gateway.sourceBar.autoHint")}</span>
          <DemoAction>
            <Button
              variant="ghost"
              size="sm"
              data-act="gw-scan"
              onClick={() => onRescan()}
              disabled={scanning}
            >
              {scanning ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              {scanning ? scanningLabel : rescanLabel}
            </Button>
          </DemoAction>
        </span>
      </div>
      {view.reason && <p className="mt-1 text-xs text-amber-600">{view.reason}</p>}
    </div>
  );
}
