import { Skeleton } from "@/components/ui/skeleton";

/**
 * 路由懒加载期间的占位（`Suspense` 的 `fallback`）。
 *
 * ## 为什么放在 `Layout` 的 `<Outlet />` 位置而不是包住 `<Routes>`
 *
 * 包住整个 `<Routes>` 会让**侧栏也一起被替换掉** —— 每次切页侧栏闪一下，
 * 看起来像整页重载。放在 `<Outlet />` 里，外壳（侧栏、产品切换、状态圆点）保持不动，
 * 只有内容区显示骨架，切页体验与「数据还在加载」的页面内骨架一致。
 *
 * ## 为什么用骨架而不是转圈
 *
 * 各页面自己加载数据时用的就是 `Skeleton`（见 `TokenStatsPage` / `TraeCreditsPage` 等）。
 * 若这里放一个居中转圈，用户会看到「转圈 → 骨架」两次跳变；用同一种骨架语言只跳一次。
 *
 * 容器样式对齐页面外壳（`mx-auto max-w-[1240px] px-7 pt-[26px] pb-[60px]`），避免加载完成时整块内容位移。
 */
export function PageFallback() {
  return (
    <div
      className="mx-auto w-full max-w-[1240px] px-7 pt-[26px] pb-[60px]"
      // 无障碍：告诉读屏软件这里正在加载，而不是一片沉默的空白。
      aria-busy="true"
      aria-live="polite"
      data-testid="page-fallback"
    >
      <Skeleton className="h-7 w-44" />
      <Skeleton className="mt-2 h-4 w-72" />
      <div className="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {[0, 1, 2, 3, 4, 5].map((key) => (
          <Skeleton key={key} className="h-36 rounded-2xl" />
        ))}
      </div>
    </div>
  );
}
