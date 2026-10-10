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
 * 合并态切换（**视图参数**，不是作用域）—— `.seg` 双按钮形态（同原型 `viewUsage` 的 params 段）。
 *
 * ## 为什么这里是「仅本作用域 / 合并全部作用域」两个按钮，而不是一个开关
 *
 * 原型 `viewUsage` 的合并控件就是一个 `.seg`：
 * `<button>仅本作用域</button><button>合并全部作用域</button>`。
 * 用两段式分段控件把「当前是哪一档」直接显出来，比「开关 + 标签」更一目了然。
 *
 * ## 为什么区域不在这里选
 *
 * 区域（`cn` ↔ `global`）**已经上移到侧栏**（`ScopeSwitch`，P2）—— 页面里再放一个区域选择器，
 * 就有了**两个「看起来能切」的控件**，用户不确定哪个生效（§3.1 的纪律）。
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
  const options = [
    { merge: false, label: t("shared.region.combine.only") },
    { merge: true, label: t("shared.region.combine.all") },
  ];
  return (
    <div
      className={cn("flex gap-1 rounded-lg bg-muted p-1", disabled && "opacity-60", className)}
      role="group"
      aria-label={t("shared.region.combine.aria")}
    >
      {options.map((option) => (
        <button
          key={option.merge ? "all" : "only"}
          type="button"
          className={cn(
            "cursor-pointer rounded-md px-2.5 py-1.5 text-xs transition-colors",
            value === option.merge
              ? "bg-background font-medium text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground",
            disabled && "cursor-not-allowed",
          )}
          onClick={() => onChange(option.merge)}
          disabled={disabled}
          aria-pressed={value === option.merge}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
