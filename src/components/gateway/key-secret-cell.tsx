import { useState } from "react";
import { Copy, Eye, EyeOff } from "lucide-react";

import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/clipboard";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * 密钥列：**掩码 + 眼睛**（四条产品线共用）。
 *
 * ## 为什么抽成组件
 *
 * WB / Trae / Qoder / 统一网关四张表此前各写一遍掩码渲染，规则已经漂移过一次。
 * 眼睛的判据（「有没有真实明文」）和显隐交互也必须四边一致 —— 一处一个写法，
 * 用户就会看到「同一种 Key，有的能看、有的不能看」。
 *
 * ## 两个状态
 *
 * - `plaintext` 为 `null` / 空串（旧台账、或改造前创建的 Key）⇒ **只显示掩码、不出眼睛**：
 *   库里从来没有明文，眼睛点了也无从显示，给一个死按钮比不给更糟。
 * - `plaintext` 有值 ⇒ 掩码后跟一枚眼睛，点开换成 `EyeOff`，再点收回掩码。
 *   明文显出来时另给一枚复制按钮（创建面板那枚复制按钮只在建 Key 那一刻有用；
 *   这里的是「以后随时复制」的出口）。
 *
 * ⚠️ **`stopPropagation`**：四张表的行都可以点开明细，眼睛 / 复制按钮必须挡住冒泡，
 * 否则点一次眼睛会顺手把行展开（用户以为误触）。
 */
export function KeySecretCell({
  plaintext,
  prefix,
  suffix,
  className,
}: {
  /** 完整明文；`null` / 空串 ⇒ 不显示眼睛。 */
  plaintext?: string | null;
  /** 可辨识前缀（如 `sk-wb-a1b2`）。 */
  prefix: string;
  /** 尾部 4 位；旧台账为空串 ⇒ 掩码省略尾部。 */
  suffix?: string;
  className?: string;
}) {
  const t = useT();
  const [revealed, setRevealed] = useState(false);
  const secret = typeof plaintext === "string" && plaintext.length > 0 ? plaintext : null;
  const masked = suffix ? `${prefix}••••••••${suffix}` : `${prefix}…`;
  const shown = revealed && secret ? secret : masked;

  return (
    <div className={cn("flex min-w-0 items-center gap-1", className)} data-act="gw-key-secret">
      <code
        className={cn(
          "min-w-0 select-all break-all font-mono text-xs",
          revealed && secret ? "text-foreground" : "text-muted-foreground",
        )}
      >
        {shown}
      </code>
      {secret && (
        <>
          <Button
            variant="ghost"
            size="xs"
            className="shrink-0"
            data-act="gw-key-reveal"
            aria-label={revealed ? t("shared.gateway.keys.hide") : t("shared.gateway.keys.reveal")}
            title={revealed ? t("shared.gateway.keys.hide") : t("shared.gateway.keys.reveal")}
            onClick={(event) => {
              // 行本身是「点开明细」的开关 ⇒ 必须挡住冒泡。
              event.stopPropagation();
              setRevealed((value) => !value);
            }}
          >
            {revealed ? <EyeOff /> : <Eye />}
          </Button>
          {revealed && (
            <Button
              variant="ghost"
              size="xs"
              className="shrink-0"
              data-act="gw-key-reveal-copy"
              aria-label={t("shared.gateway.keys.copy")}
              title={t("shared.gateway.keys.copy")}
              onClick={(event) => {
                event.stopPropagation();
                void copyText(secret);
              }}
            >
              <Copy />
            </Button>
          )}
        </>
      )}
    </div>
  );
}
