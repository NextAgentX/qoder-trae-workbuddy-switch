import { Switch } from "@/components/ui/switch";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

interface RegionBarProps {
  /**
   * 是否**合并**两个区域查看。
   *
   * ⚠️ 这是**布尔**而不是 `RegionFilter` —— 见下方说明：区域不在这里选。
   */
  value: boolean;
  onChange: (next: boolean) => void;
  /** 重新取数中：控件保持可点，仅禁用并给出无障碍提示。 */
  disabled?: boolean;
  className?: string;
}

/**
 * 合并态开关（**视图参数**，不是作用域）。
 *
 * ## 为什么从「三选一」变成「一个开关」
 *
 * 改造前这里渲染 `cn / global / all` 三个 Tab。但 **区域已经上移到侧栏**
 * （`ScopeSwitch`，P2）—— 页面里再放一个区域选择器，就有了**两个「看起来能切」的控件**，
 * 用户不确定哪个生效（§3.1 的纪律）。
 *
 * ## 为什么 `all` 留下
 *
 * 因为它**不是作用域**。判据（VOCABULARY 第 1 节）：
 *
 * > 换一个值，会不会导致「取的是**另一份数据**」？会 ⇒ 作用域；不会 ⇒ 视图参数。
 *
 * `cn → global` 换的是**数据源**（作用域，归侧栏）；
 * `单区 → all` 换的是**同一份数据的呈现方式**（把两个区域合起来看）⇒ **视图参数**，留在页内。
 *
 * 这也与 `useScope` 的分工一致：它只管 `cn` / `global`，**不管 `all`**。
 */
export function RegionBar({ value, onChange, disabled, className }: RegionBarProps) {
  const t = useT();
  return (
    <label
      className={cn(
        "flex min-w-0 items-center gap-2 text-xs text-muted-foreground",
        disabled && "opacity-60",
        className,
      )}
    >
      <Switch
        checked={value}
        onCheckedChange={onChange}
        disabled={disabled}
        aria-label={t("shared.region.combine.aria")}
      />
      <span className="truncate">{t("shared.region.combine.label")}</span>
    </label>
  );
}
