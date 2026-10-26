/**
 * 聚合页的 **Trae 程序位切换**（issue #89 · 2.1）。
 *
 * ## 为什么必须有这一层
 *
 * Trae 的切换是「**先关客户端、再写登录态、再重开**」。渠道页把这件事当硬纪律：
 * `requestSwitchTo` 只把目标放进 state，真正的动作在 `RestartConfirmDialog` 的
 * `onConfirm` 里执行（`TraeAccountsPage.tsx` 的 `doSwitchTo`）。聚合页此前直接在
 * `renderCard` 里写 `void api.traeSwitchAccount({ userId, variant })` ——
 * **`api.traeSwitchAccount` 的 `launch` 默认是 `true`** ⇒ 用户点一下页脚胶囊，
 * 客户端被静默重启，而全程没有一句确认。
 *
 * 两条路径的**实际效果**相同（确认框里不勾选时 `autoReopen` 也是 `true`），
 * 差别只在于「有没有告知用户这一下会关掉客户端」—— 而这正是 issue #50 R2
 * 「同一组件、同一套动作」里被漏掉的那一半。
 *
 * ## 与 `aggregate-wb-switcher` 同构
 *
 * 确认框、busy 态、成功/失败 toast 都收在这一层，页面只调 `request(...)`。
 * 在 `renderCard` 里内联这些逻辑，等于把「确认 + busy + 反馈」再写一遍 ——
 * 那正是 issue #89 第五节指出的成因 2。
 */

import { useCallback, useState } from "react";
import { toast } from "sonner";

import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { aggregateDisplayName, type AggregateAccount } from "@/lib/aggregate-accounts";
import type { TraeVariantId } from "@/lib/trae-types";

export interface AggregateTraeSwitcher {
  /** 请求切换（**先弹确认框**，不直接执行）。 */
  request: (account: AggregateAccount, variant: TraeVariantId, label: string) => void;
  /**
   * 正在执行的动作 key，形状与卡片约定一致：`switch-<userId>@<变体>`。
   *
   * 卡片的 `busy` 派生出「切换中」文案与菜单项的 `disabled`
   * （`trae-account-card.tsx` 的 `switchingVariant` / `saving` / `thawing` …）——
   * 键形状对不上就**不转圈、不互斥、可重复点**，重复点会真的发第二次请求。
   */
  busy: string | null;
  /** 任一账号正在切换中 ⇒ 锁住所有切换按钮（与渠道页 `switchBusy` 同义）。 */
  switchBusy: boolean;
  /** 确认框 props（页面渲染 `RestartConfirmDialog`）。 */
  confirmProps: {
    open: boolean;
    clientLabel: string;
    isCli: false;
    onConfirm: (restart: boolean) => void;
    onOpenChange: (open: boolean) => void;
  };
}

interface Pending {
  account: AggregateAccount;
  variant: TraeVariantId;
  label: string;
}

/**
 * @param onSwitched 切换成功后的回调（页面据此重取账号列表与程序位信号）。
 */
export function useAggregateTraeSwitcher(onSwitched?: () => void): AggregateTraeSwitcher {
  const t = useT();
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const request = useCallback((account: AggregateAccount, variant: TraeVariantId, label: string) => {
    setPending({ account, variant, label });
  }, []);

  const onConfirm = useCallback(
    (restart: boolean) => {
      const target = pending;
      setPending(null);
      if (!target || target.account.platform !== "trae") return;
      const userId = target.account.data.userId;
      const key = `switch-${userId}@${target.variant}`;
      setBusy(key);
      const toastId = toast.loading(t("aggregate.toast.switching"), {
        description: `${aggregateDisplayName(target.account)} · ${target.label}`,
      });
      void (async () => {
        try {
          /* `launch` 就是确认框里的「自动重开」—— **必须显式传**，
             否则 `api.traeSwitchAccount` 会走它自己的默认 `true`，
             用户在框里取消勾选就成了假承诺（§3.5）。 */
          await api.traeSwitchAccount({
            userId,
            variant: target.variant,
            launch: restart,
          });
          toast.success(t("wbAccounts.toast.configUpdated"), {
            id: toastId,
            description: `${aggregateDisplayName(target.account)} · ${target.label}`,
          });
          onSwitched?.();
        } catch (error) {
          toast.error(api.asError(error), { id: toastId });
        } finally {
          setBusy(null);
        }
      })();
    },
    [onSwitched, pending, t],
  );

  return {
    request,
    busy,
    switchBusy: busy !== null,
    confirmProps: {
      open: pending !== null,
      clientLabel: pending?.label ?? "",
      isCli: false,
      onConfirm,
      onOpenChange: (open) => {
        if (!open) setPending(null);
      },
    },
  };
}
