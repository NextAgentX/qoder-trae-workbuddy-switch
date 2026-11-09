import { useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Command, CommandGroup, CommandInput, CommandItem } from "@/components/ui/command";
import { CommandEmpty, CommandList } from "cmdk";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * 「默认思考档」选择器（Qoder / WorkBuddy 共用）。
 *
 * ## 为什么是独立组件
 *
 * `ModelTable` 的 `effortColumn.render(row)` 是在**渲染期被调用**的普通函数，
 * 不是 React 组件 —— 在里面直接 `useState` 违反 Hooks 规则
 * （运行时 `Invalid hook call` / 状态错乱，而 TS 查不出来）。所以选择器必须
 * 有自己的组件边界，`render` 只负责把数据传进来。
 *
 * ## 语义：这是「默认值」，不是「强制值」
 *
 * 客户端自己**没传** `reasoning_effort` 时，网关补上这里选的档；
 * 客户端显式传了就**尊重客户端** —— 否则换个客户端就会莫名被降档。
 * 注入点在各自产品线的出站构体处（Qoder: `qoder/routes.rs`；WB: `outbound/mod.rs`）。
 *
 * ## 为什么 `value` 可能不在 `efforts` 里
 *
 * 静态档位表与上游声明会随版本变化：用户昨天选的档，今天可能已不被该模型支持。
 * 此时仍**原样显示**用户的选择（不静默改写），后端注入时再按官方词表校验丢弃 ——
 * 「显示的和实际发的不是一回事」比「显示一个过时值」更难查。
 */
export function ModelEffortSelect({
  modelId,
  efforts,
  value,
  onChange,
}: {
  /** 模型标识（写进 `model_efforts` 的键）。 */
  modelId: string;
  /** 该模型支持的档位（保序由调用方决定，这里会按当前值排序）。 */
  efforts: string[];
  /** 当前已配置的默认档；`null` = 未配置（回落静态表 / 上游默认）。 */
  value: string | null;
  /** 选中回调；传 `null` 表示清除配置。 */
  onChange: (modelKey: string, effort: string | null) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  // 当前值排最前：一打开就能看到「我现在选的是哪个」。
  const sorted = [...efforts].sort((a, b) => {
    if (a === value) return -1;
    if (b === value) return 1;
    return a.localeCompare(b);
  });

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="h-7 w-full justify-between text-xs"
          // 模型行本身有「复制 id / 禁用」两处点击区，这里必须阻止冒泡，
          // 否则点下拉会连带触发行的其它行为。
          onClick={(event) => event.stopPropagation()}
        >
          {value || t("shared.gateway.models.effortDefault", { effort: sorted[0] })}
          <ChevronDown className="ml-2 h-3 w-3 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[200px] p-0" align="start">
        <Command>
          <CommandInput placeholder={t("shared.gateway.models.selectEffort")} />
          <CommandList>
            <CommandEmpty>{t("shared.gateway.models.noEffort")}</CommandEmpty>
            <CommandGroup>
              {sorted.map((effort) => (
                <CommandItem
                  key={effort}
                  value={effort}
                  onSelect={() => {
                    onChange(modelId, effort);
                    setOpen(false);
                    toast.success(t("shared.gateway.models.effortSaved"));
                  }}
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4",
                      value === effort ? "opacity-100" : "opacity-0",
                    )}
                  />
                  {effort}
                  {value === effort && (
                    <span className="ml-auto text-xs text-muted-foreground">
                      {t("shared.gateway.models.current")}
                    </span>
                  )}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
