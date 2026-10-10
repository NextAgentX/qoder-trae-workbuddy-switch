import { TraeVariantMark, WorkBuddyMark } from "@/components/product-marks";
import { useT, type Translate } from "@/lib/i18n";
import { platformDescriptor, type Product } from "@/lib/platforms/registry";
import { regionLabel } from "@/lib/region";
import { scopeParts, type Scope } from "@/lib/scope";
import { cn } from "@/lib/utils";

/**
 * 作用域的展示名：`{平台名} {区域名}`，如「WorkBuddy 国内版」。
 *
 * 与顶栏 `ScopeSelector` 的 `nameOf` **同源**（原型 `scopeLabel()`，`demo-app.js:73-77`）——
 * 两处各拼一遍迟早漂移成「顶栏说国际版、页头说国内版」，那正是本项目反复要消灭的不一致。
 */
export function scopeLabel(t: Translate, scope: Scope): string {
  const { product, region } = scopeParts(scope);
  return `${t(platformDescriptor(product).labelKey)} ${regionLabel(region)}`;
}

/**
 * 平台 → 官方图标。
 *
 * 作用域只到**平台**粒度（区域不换图标）⇒ 不按区域分支。顶栏与页头 `ScopeChip`
 * 共用这一个实现，避免两处各写一份 `product === "workbuddy" ? ... : ...`。
 */
export function ScopeMark({
  product,
  size,
  className,
}: {
  product: Product;
  size: number;
  className?: string;
}) {
  /**
   * 圆角随尺寸取原型值（`buddy-switch-prototype.html:117` 触发按钮 26px/圆角 8；
   * `:455` 菜单项 28px/圆角 9；`:206` 页头 chip 14px/圆角 4）。
   * ⚠️ 不能用比例尺（`rounded-lg` = 12px）—— 26px 的方块配 12px 圆角会「近似圆形」。
   */
  const radius = size <= 14 ? "rounded-[4px]" : size <= 26 ? "rounded-[8px]" : "rounded-[9px]";
  return product === "workbuddy" ? (
    <WorkBuddyMark size={size} className={cn(radius, className)} />
  ) : (
    <TraeVariantMark variant="trae_work" size={size} className={cn(radius, className)} />
  );
}

/**
 * 页头右侧的**只读**作用域标签（原型 `.scope-chip`，`demo.css:182-188`）。
 *
 * ## 为什么是只读标签而不是切换器
 *
 * 区域已并入 L1 作用域，切作用域的**唯一入口**是顶栏 `ScopeSelector`（§3.1）。
 * 页内若再放一个区域切换器，同一件事实就有了两个控件，且它们必须互相同步 ——
 * 那正是改造前「顶栏说 A、页内说 B」的成因。这里只**显示**当前作用域。
 */
export function ScopeChip({ scope, className }: { scope: Scope; className?: string }) {
  const t = useT();
  return (
    <span
      className={cn(
        /* 原型 `.scope-chip`（`:201-207`）：高 22 / 内边距 `0 9px` / gap 6 / 圆角 999 / 字号 11。 */
        "inline-flex h-[22px] shrink-0 items-center gap-1.5 rounded-full bg-muted px-[9px] text-[11px] font-medium text-muted-foreground",
        className,
      )}
    >
      <ScopeMark product={scopeParts(scope).product} size={14} className="rounded-[4px]" />
      {scopeLabel(t, scope)}
    </span>
  );
}