/**
 * 「图片输入」单元格（issue #101）—— 三个平台模型页共用的**唯一渲染点**。
 *
 * ## 为什么是徽标 + 图标按钮，而不是下拉三态
 *
 * 模型表每行已经有一个「禁用 / 启用」按钮，交互语言是**徽标说明状态 + 按钮改状态**。
 * 图片能力再多一个下拉框会让同一行出现两种交互范式，而且 Radix Select 在表格行内
 * 还会带来焦点管理上的额外麻烦。所以：
 *
 * - **徽标**：`支持` / `不支持` / `未声明`（三态，见 `lib/model-image.ts`）；
 * - **切换按钮**：点一下取反（`未声明` ⇒ 标记为支持）；
 * - **还原按钮**：只在「当前值来自本页覆盖」时出现 —— 清掉覆盖、回到上游声明。
 *
 * ## 硬约束
 *
 * - **不得把「未声明」画成「不支持」**：两者的客户端行为不同（前者回落客户端配置，
 *   后者隐藏上传入口），文案与配色都必须分开。`imageUndeclared` 用中性 `outline`，
 *   `imageNo` 用 `secondary` —— 都没有警示色，因为「不知道」不是错误。
 * - **本文件是 `model-ability.tsx` 的兄弟而不是它的成员**：那边被门禁钉死「零 `onClick`」
 *   （纯展示），能力列要带交互，混进去会让那条断言失效，也会让「纯展示」的语义变糊。
 * - 写操作在演示站由 `DemoAction` 统一拦截（与「禁用 / 启用」同款）。
 */

import { Image, ImageOff, Loader2, RotateCcw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DemoAction } from "@/components/demo-action";
import { useT } from "@/lib/i18n";
import type { ImageCapView } from "@/lib/model-image";

export interface ImageCapCellProps {
  cap: ImageCapView;
  /** 该行正在写（驱动 `<Loader2>` + 禁用按钮，防重复点击）。 */
  busy?: boolean;
  /** 写覆盖：`true` / `false` = 手动指定，`null` = 还原上游声明。 */
  onSet: (next: boolean | null) => void;
}

export function ImageCapCell({ cap, busy = false, onSet }: ImageCapCellProps) {
  const t = useT();
  // 三态 → 徽标。`null` 必须有自己的文案（不是 `false` 的文案）。
  const badge =
    cap.supports === true
      ? { variant: "success" as const, label: t("shared.gateway.models.imageYes") }
      : cap.supports === false
        ? { variant: "secondary" as const, label: t("shared.gateway.models.imageNo") }
        : { variant: "outline" as const, label: t("shared.gateway.models.imageUndeclared") };

  // 来源说明：用户看得到「这是上游说的」还是「这是我自己标的」。
  const sourceHint =
    cap.source === "override"
      ? t("shared.gateway.models.imageSourceOverride")
      : cap.source === "upstream"
        ? t("shared.gateway.models.imageSourceUpstream")
        : cap.source === "known"
          ? t("shared.gateway.models.imageSourceKnown")
          : t("shared.gateway.models.imageUndeclaredHint");

  // 下一个值：`未声明` / `不支持` ⇒ 标记为支持；`支持` ⇒ 标记为不支持。
  const next = cap.supports !== true;
  const toggleLabel = next
    ? t("shared.gateway.models.imageMarkYes")
    : t("shared.gateway.models.imageMarkNo");

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {/* 来源说明用**原生 `title`**，不用 Radix Tooltip：徽标本身不可聚焦，包一层 Tooltip
          要么拿不到焦点（键盘用户永远看不到），要么给每行加一个 tab 停靠点（一张表几十行）。
          原生 title 两者都不占，与 `model-list.tsx` 里探测列的做法一致。 */}
      <Badge variant={badge.variant} className="rounded-md" title={sourceHint}>
        {badge.label}
      </Badge>
      <DemoAction>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-6 shrink-0"
          data-act="gw-model-image-toggle"
          aria-label={toggleLabel}
          title={toggleLabel}
          disabled={busy}
          onClick={() => onSet(next)}
        >
          {busy ? <Loader2 className="animate-spin" /> : next ? <Image /> : <ImageOff />}
        </Button>
      </DemoAction>
      {/* 还原只对「本页覆盖」有意义：没有覆盖时它是个空操作，不该出现在行里。 */}
      {cap.source === "override" && (
        <DemoAction>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6 shrink-0"
            data-act="gw-model-image-reset"
            aria-label={t("shared.gateway.models.imageReset")}
            title={t("shared.gateway.models.imageReset")}
            disabled={busy}
            onClick={() => onSet(null)}
          >
            <RotateCcw />
          </Button>
        </DemoAction>
      )}
    </div>
  );
}
