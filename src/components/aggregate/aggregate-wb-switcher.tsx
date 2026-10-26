/**
 * 聚合页的 **WorkBuddy 程序位切换**（issue #50）。
 *
 * ## 为什么单独一层
 *
 * 账号页（`AccountsPage`）里四个程序位各有一段切换实现（CLI / IDE / VS Code /
 * JetBrains），且**都要先走统一的重启确认框**（IDE 系要重启、CLI 会话会中断）。
 * 聚合页要能点同一排胶囊 ⇒ 这套流程得有一份**共享实现**，否则两处会漂：
 * 最容易漂的是「确认框的默认勾选」与「取消勾选后不得说『并重启』」——
 * 那是假承诺（§3.5），漂了就会出现「说重启了其实没重启」。
 *
 * ## 四条程序的差别（**不是同一个命令**）
 *
 * | 程序位 | 命令 | 要重启？ | 特殊文案 |
 * |:--|:--|:--|:--|
 * | CLI | `switch_codebuddy_cli_account` | ❌（会话会中断） | 不渲染「自动重开」勾选 |
 * | IDE | `switch_codebuddy_cn_ide_account` | ✅ | — |
 * | VS Code | `switch_vscode_codebuddy_plugin_account` | ✅ | — |
 * | JetBrains | `switch_jetbrains_codebuddy_plugin_account` | ✅ | — |
 *
 * ⚠️ 「需重启」的三个**默认勾选「自动重开」**，且**必须**显式提示未保存文件风险。
 */

import { useCallback, useState } from "react";
import { toast } from "sonner";

import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { aggregateDisplayName, type AggregateAccount } from "@/lib/aggregate-accounts";

/** 四个程序位。 */
export type WbProgramSlot = "cli" | "ide" | "vscode" | "jetbrains";

export interface AggregateWbSwitcher {
  /** 请求切换（**先弹确认框**，不直接执行）。 */
  request: (slot: WbProgramSlot, account: AggregateAccount) => void;
  /** 正在切换中的账号 id（用于把胶囊标成「切换中」）。 */
  busyAccountId: string | null;
  /**
   * 某个程序位**是否有账号正在切换**（锁住该位的所有按钮）。
   *
   * ★ issue #89 · B4：渠道页四个 `*Busy` 都是「该程序位任一账号切换中就锁全部」
   * （`codebuddyCliSwitchingId !== null`），不是逐账号 —— 因为切换是「关客户端 + 写登录态」，
   * 并发点第二个会在前一个还没落盘时覆盖它。聚合页此前只传了 `codebuddyCliBusy`
   * 且用的是**逐账号**语义 ⇒ 既不锁其它卡，也没锁另外三个程序位。
   */
  slotBusy: (slot: WbProgramSlot) => boolean;
  /** 某账号在某个程序位上正在切换（转圈 + `aria-busy`）。 */
  slotLoading: (slot: WbProgramSlot, accountId: string) => boolean;
  /** 确认框的 props（页面渲染 `RestartConfirmDialog`）。 */
  confirmProps: {
    open: boolean;
    clientLabel: string;
    isCli: boolean;
    onConfirm: (restart: boolean) => void;
    onOpenChange: (open: boolean) => void;
  };
}

/**
 * 四个程序位的切换（聚合页用）。
 *
 * @param onSwitched 切换成功后的回调（页面据此重取程序位状态与账号列表）。
 */
export function useAggregateWbSwitcher(onSwitched?: () => void): AggregateWbSwitcher {
  const t = useT();
  const [pending, setPending] = useState<{ slot: WbProgramSlot; account: AggregateAccount } | null>(null);
  /* 记**槽位 + 账号**：只有槽位才能回答「锁哪一位」，只有账号才能回答「给谁转圈」。 */
  const [busy, setBusy] = useState<{ slot: WbProgramSlot; accountId: string } | null>(null);

  const request = useCallback((slot: WbProgramSlot, account: AggregateAccount) => {
    setPending({ slot, account });
  }, []);

  const clientLabel = useCallback(
    (slot: WbProgramSlot) =>
      slot === "cli"
        ? t("wbAccounts.program.codebuddyCli")
        : slot === "ide"
          ? t("wbAccounts.program.codebuddyCnIde")
          : slot === "vscode"
            ? t("wbAccounts.program.vscodeCodebuddyPlugin")
            : t("wbAccounts.program.jetbrainsCodebuddyPlugin"),
    [t],
  );

  const onConfirm = useCallback(
    (restart: boolean) => {
      const target = pending;
      setPending(null);
      if (!target || target.account.platform !== "workbuddy") return;
      const id = target.account.data.id;
      setBusy({ slot: target.slot, accountId: id });
      const label = aggregateDisplayName(target.account);
      const toastId = toast.loading(
        target.slot === "cli"
          ? t("wbAccounts.toast.switchCliLoading")
          : target.slot === "ide"
            ? t("wbAccounts.toast.switchIdeLoading")
            : t("aggregate.toast.switching"),
        { description: label },
      );
      void (async () => {
        try {
          if (target.slot === "cli") {
            await api.switchCodebuddyCliAccount(id);
          } else if (target.slot === "ide") {
            await api.switchCodebuddyCnIdeAccount(id, restart);
          } else if (target.slot === "vscode") {
            await api.switchVscodeCodebuddyPluginAccount(id, restart);
          } else {
            await api.switchJetbrainsCodebuddyPluginAccount(id, restart);
          }
          toast.success(t("wbAccounts.toast.configUpdated"), { id: toastId, description: label });
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

  const slotBusy = useCallback((slot: WbProgramSlot) => busy?.slot === slot, [busy]);
  const slotLoading = useCallback(
    (slot: WbProgramSlot, accountId: string) =>
      busy?.slot === slot && busy.accountId === accountId,
    [busy],
  );

  return {
    request,
    busyAccountId: busy?.accountId ?? null,
    slotBusy,
    slotLoading,
    confirmProps: {
      open: pending !== null,
      clientLabel: clientLabel(pending?.slot ?? "ide"),
      isCli: pending?.slot === "cli",
      onConfirm,
      onOpenChange: (open) => {
        if (!open) setPending(null);
      },
    },
  };
}
