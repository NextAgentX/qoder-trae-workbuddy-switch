/**
 * 用量图表 · **统一外壳**（P3）。
 *
 * ## 为什么需要它
 *
 * PLAN §3.10 第 4/8/9 条要求每张图都具备：**标题 + 图注 + `role="img"` + 中文 aria-label**
 * （看不了图的人拿到同样的信息），以及**图/表互切**（看到同一份数据）与**导出**。
 * 改造前这些散在 12+ 张图里，各写一遍必然漂移（有的漏 aria-label，有的没有图注）。
 *
 * ## 契约
 *
 * - `role="img"` + `aria-label` 由本壳**统一兜底**（调用方只给文本，不给属性）；
 * - 图注**必须**由 `lib/chart-caption.ts` 产出后传入（本壳不生成、不校验内容，靠约定）；
 * - 图/表互切：给 `tableFallback` 即出现切换器（同一份数据的两种形态，不是第二个「看起来能切」的控件）；
 * - 导出：内置把图区 `<svg>` 序列化为 PNG（recharts 出 svg；纯 DOM 图设 `exportable={false}`）。
 */

import { useRef, useState, type ReactNode } from "react";
import { BarChart3, Download, Table2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface ChartFrameProps {
  /** 已 i18n 的标题。 */
  title: string;
  /** 图注：**必须**由 `lib/chart-caption.ts` 产出。 */
  caption?: string;
  /** 中文 aria-label（看不了图的人拿到同样的信息）。 */
  ariaLabel: string;
  /** 图区（recharts 组件或自绘图）。 */
  children: ReactNode;
  /** 表视图兜底（图/表可互切，看到同一份数据）。 */
  tableFallback?: ReactNode;
  /** 是否提供导出按钮（纯 DOM 图无 `<svg>` 时设 `false`）。 */
  exportable?: boolean;
  className?: string;
}

export function ChartFrame({
  title,
  caption,
  ariaLabel,
  children,
  tableFallback,
  exportable = true,
  className,
}: ChartFrameProps) {
  const t = useT();
  const [mode, setMode] = useState<"chart" | "table">("chart");
  const bodyRef = useRef<HTMLDivElement>(null);

  /** 把图区里的 `<svg>` 序列化后画到 canvas 再导出 PNG（2× 高清）。 */
  function handleExport() {
    const svg = bodyRef.current?.querySelector("svg");
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    const width = Math.max(1, Math.round(rect.width));
    const height = Math.max(1, Math.round(rect.height));
    const clone = svg.cloneNode(true) as SVGSVGElement;
    clone.setAttribute("width", String(width));
    clone.setAttribute("height", String(height));
    const xml = new XMLSerializer().serializeToString(clone);
    const svgUrl = URL.createObjectURL(new Blob([xml], { type: "image/svg+xml;charset=utf-8" }));
    const image = new Image();
    image.onload = () => {
      const scale = 2;
      const canvas = document.createElement("canvas");
      canvas.width = width * scale;
      canvas.height = height * scale;
      const context = canvas.getContext("2d");
      if (context) {
        context.scale(scale, scale);
        context.drawImage(image, 0, 0, width, height);
        canvas.toBlob((png) => {
          if (!png) return;
          const link = document.createElement("a");
          link.href = URL.createObjectURL(png);
          link.download = `${title}.png`;
          link.click();
          URL.revokeObjectURL(link.href);
        });
      }
      URL.revokeObjectURL(svgUrl);
    };
    image.src = svgUrl;
  }

  const showToggle = Boolean(tableFallback);

  return (
    <Card className={cn("min-w-0 gap-0 overflow-hidden rounded-xl py-0 shadow-none", className)}>
      <div className="flex min-w-0 flex-wrap items-start justify-between gap-2 border-b border-border/60 px-4 py-3 sm:px-5">
        <div className="min-w-0">
          <h3 className="text-[13px] font-medium leading-5">{title}</h3>
          {caption ? (
            <p className="mt-0.5 text-[11px] leading-4 text-muted-foreground">{caption}</p>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {showToggle ? (
            <div
              className="flex gap-1 rounded-lg bg-muted p-1"
              role="group"
              aria-label={t("wbStats.usage.chart.viewToggleAria")}
            >
              <button
                type="button"
                className={cn(
                  "cursor-pointer rounded-md p-1.5 transition-colors",
                  mode === "chart"
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
                aria-pressed={mode === "chart"}
                aria-label={t("wbStats.usage.chart.chart")}
                onClick={() => setMode("chart")}
              >
                <BarChart3 className="size-3.5" aria-hidden="true" />
              </button>
              <button
                type="button"
                className={cn(
                  "cursor-pointer rounded-md p-1.5 transition-colors",
                  mode === "table"
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
                aria-pressed={mode === "table"}
                aria-label={t("wbStats.usage.chart.table")}
                onClick={() => setMode("table")}
              >
                <Table2 className="size-3.5" aria-hidden="true" />
              </button>
            </div>
          ) : null}
          {exportable && mode === "chart" ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="size-7 p-0 text-muted-foreground"
              onClick={handleExport}
              aria-label={t("wbStats.usage.chart.export")}
            >
              <Download className="size-3.5" />
            </Button>
          ) : null}
        </div>
      </div>
      <div className="min-w-0 px-4 pt-3 pb-4 sm:px-5">
        {mode === "table" && tableFallback ? (
          tableFallback
        ) : (
          <div ref={bodyRef} role="img" aria-label={ariaLabel} className="min-w-0">
            {children}
          </div>
        )}
      </div>
    </Card>
  );
}
