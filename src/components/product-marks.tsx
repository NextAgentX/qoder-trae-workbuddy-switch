import { cn } from "@/lib/utils";
import workbuddyIcon from "@/assets/workbuddy-official-icon.png";
import codebuddyCnIdeIcon from "@/assets/codebuddy-cn-ide-icon.png";
import traeIcon from "@/assets/trae.png";
import traeworkIcon from "@/assets/traework.png";
import qoderIcon from "@/assets/qoder.png";
import doubaoIcon from "@/assets/doubao-official-icon.png";
import loomyIcon from "@/assets/loomy-official-icon.png";
import raccoonIcon from "@/assets/raccoon-official-icon.png";

interface MarkProps {
  size?: number;
  className?: string;
}

/**
 * WorkBuddy 官方应用图标（从 WorkBuddy.app 的 icon.icns 提取）。
 * 与 CodeBuddy IDE 图标同为标准 macOS app icon 风格（约 10% 透明边距），
 * 放大 118% 居中裁掉透明圈后与 CodeBuddy 系列图标视觉一致。
 */
export function WorkBuddyMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%]", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={workbuddyIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

export function CodeBuddyMark({ size = 32, className }: MarkProps) {
  const icon = Math.max(10, Math.round(size));
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-[22%] border border-white/10 bg-zinc-950 text-zinc-50 shadow-sm",
        className,
      )}
      style={{ width: size, height: size, fontSize: icon }}
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="size-[1em]"
      >
        <path d="M4.4 7.4 10.4 12 4.4 16.6" />
        <path d="M13 16.6h7" />
      </svg>
    </span>
  );
}

/**
 * CodeBuddy IDE（桌面客户端）官方应用图标。
 * 源图四周自带约 9% 透明边距：正方形图 + object-cover 不会触发任何缩放，
 * 必须先把图放大到 122% 再居中裁剪，才能把透明圈裁掉并与 WorkBuddy 的
 * 全幅 logo 达到同样的视觉大小（裁剪仅落在透明边距上，几乎不伤画面）。
 */
export function CodeBuddyCnIdeMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={codebuddyCnIdeIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[122%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

/**
 * Trae 客户端官方应用图标（深色底版本）。
 *
 * 源图已归一化为与 WorkBuddy 官方图标相同的参数：
 * 圆角方块内容占画布 80.4%、四周约 9.8% 透明边距、严格居中，
 * 因此复用 `WorkBuddyMark` 的同一手法 —— 放大 118% 居中裁掉透明圈，
 * 再由容器 `rounded-[22%]` 裁出圆角，保证同排产品图标视觉体量一致。
 */
export function TraeMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%]", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={traeIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

/**
 * Qoder 官方图标（黑色「Q」字标 + 自带浅色底）。
 *
 * 源图为官方 logo 原图，已按 `WorkBuddyMark` / `TraeMark` 的**同一参数**归一化：
 * 内容占画布 **80.4%**、严格居中、底色取原图自带色 ⇒ 直接复用
 * 「放大 118% 居中 + 容器 `rounded-[22%]` 收口」的同款手法，
 * 保证同排产品图标视觉体量一致。
 *
 * ⚠️ 曾经这里是一枚**手绘圆环字形**（本仓当时没有官方资产，形状只是原型的
 * 抽象占位符）—— 2026-10-15 换用官方 logo 后**不要**回退到自绘字形：
 * 作用域选择器 / 页头 chip 上的图标会被用户拿来和官方客户端比对。
 */
export function QoderMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%]", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={qoderIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

export function StatusDot({ on, className }: { on: boolean; className?: string }) {
  return (
    <span
      aria-hidden
      className={cn("size-1.5 shrink-0 rounded-full", on ? "bg-primary" : "bg-muted-foreground/35", className)}
    />
  );
}

/**
 * 产品线变体标识（与 Rust 侧 `TraeVariant::as_str()` 逐字一致）。
 *
 * 刻意**不复用** `TraeVariantId` 的导入：本文件是纯展示组件，
 * 不依赖任何业务类型模块，避免「改一个类型定义要连带动画图标组件」。
 */
type TraeVariantKey =
  | "trae_work"
  | "trae_cn"
  | "trae_code"
  // 合并后的客户端（issue #20）：图标与 `trae_code` 同一个（同一家族）。
  | "trae"
  | "trae_merged"
  | "global"
  | "global_trae_code"
  | "global_trae_merged"
  | "cn";

/**
 * Trae 产品线图标（按变体区分）。
 *
 * ## 两条产品线各用官方图标
 *
 * `nameAlias` 显示 `TRAE SOLO CN` 自称 `TraeWork CN`、`Trae CN` 自称
 * `TraeCode CN`，两条产品线现在各有一张官方图标（已按 WorkBuddy 参数
 * 归一化：内容占画布 80.4%、四周 9.8% 透明边距、严格居中）：
 *
 * - `trae_work`：`traework.png`（官方浅色底图标）
 * - `trae_cn`  ：`trae.png`（官方深色底图标）
 *
 * 两者靠**图标本身的底色**区分（浅底 / 深底），**不再叠 `CN` 角标**：
 * 角标在 15px 的按钮里只有 4~5px 高，糊成一团反而像噪点；而调用方
 * （账号卡片按钮、产品线切换器、范围条）都在图标旁写着产品线名称，
 * 角标属于重复信息。
 *
 * 渲染手法与 `WorkBuddyMark` / `TraeMark` 完全一致（118% 放大裁透明圈）。
 */
export function TraeVariantMark({
  variant,
  size = 32,
  className,
}: MarkProps & { variant: TraeVariantKey }) {
  return (
    <span
      aria-hidden
      className={cn(
        "relative inline-flex shrink-0 overflow-hidden rounded-[22%]",
        className,
      )}
      style={{ width: size, height: size }}
    >
      {/* `global`（国际版 TraeWork）与 `trae_work`（国内版）同属 TraeWork 家族 ⇒ 同一个图标。
          `trae` / `trae_merged` / `trae_code` / `global_trae_merged` 同属 Trae 家族
          ⇒ 都走 `traeIcon`（落 else 分支）。 */}
      <img
        src={variant === "trae_work" || variant === "global" ? traeworkIcon : traeIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

/* ─────────────── 外部积分平台（豆包 / Loomy / 商汤小浣熊） ───────────────
 *
 * 三个官方图标均已按 WorkBuddy / Trae / Qoder 的**同一组参数**归一化：
 * 画布 256×256 透明、内容占画布 80.4%、严格居中
 * （脚本：`scripts/normalize-platform-logos.mjs`；源图取自各自官网公开的
 * favicon / apple-touch-icon）。因此渲染手法逐字复用 —— 放大 118% 居中裁掉
 * 透明圈，再由容器 `rounded-[22%]` 收口，与同排产品图标视觉体量一致。
 *
 * ⚠️ 小浣熊的源图是**横版 lockup**（178×111），归一化时按长边缩放到 80.4%
 * 并居中，因此它在方形容器里上下留白更多 —— 这是保持原图比例的正确结果，
 * 不要为了「撑满」去拉伸它。
 */

export function DoubaoMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%]", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={doubaoIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

export function LoomyMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%]", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={loomyIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

export function RaccoonMark({ size = 32, className }: MarkProps) {
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%]", className)}
      style={{ width: size, height: size }}
    >
      <img
        src={raccoonIcon}
        alt=""
        className="absolute left-1/2 top-1/2 size-[118%] max-w-none -translate-x-1/2 -translate-y-1/2 object-cover"
      />
    </span>
  );
}

/**
 * 聚合入口标识（四宫格拼合）。
 *
 * ## 为什么是四宫格
 *
 * 聚合页的语义是「把多个平台的作用域收进一处看」—— 四宫格正好是「多合为一」的
 * 通用图形隐喻：三格分别放 WorkBuddy / Trae / Qoder 的**官方图标**（沿用各自的
 * 归一化资产，不另画一遍），第四格放一个品牌色圆点，表示「还有更多」。
 *
 * ## 为什么不用 lucide 图标拼
 *
 * `LayoutGrid` 之类的线性图标与旁边四枚**彩色实心**产品图标并排会显得轻飘，
 * 用户也读不出「这是四个平台」。这里直接复用真实产品图标，语义自解释。
 *
 * ## 几何
 *
 * 与产品图标同尺寸同圆角（`rounded-[22%]`），内部 2×2 栅格、格子间距为尺寸的 6%，
 * 每格自身带 18% 圆角 —— 视觉密度与 WorkBuddy / Trae / Qoder 的单个图标相当。
 */
export function AggregateMark({ size = 32, className }: MarkProps) {
  const gap = Math.max(1, Math.round(size * 0.06));
  const cell = (size - gap) / 2;
  return (
    <span
      aria-hidden
      className={cn("relative inline-flex shrink-0 overflow-hidden rounded-[22%] bg-muted/60", className)}
      style={{ width: size, height: size, padding: gap, display: "grid", gap, gridTemplateColumns: "1fr 1fr", gridTemplateRows: "1fr 1fr" }}
    >
      <WorkBuddyMark size={cell} className="!rounded-[18%]" />
      <TraeMark size={cell} className="!rounded-[18%]" />
      <QoderMark size={cell} className="!rounded-[18%]" />
      <span className="grid place-items-center rounded-[18%] bg-primary/15">
        <span className="rounded-full bg-primary" style={{ width: cell * 0.34, height: cell * 0.34 }} />
      </span>
    </span>
  );
}
