/**
 * 聚合页的 **Qoder 程序位切换**（issue #89 · 2.2）。
 *
 * ## 为什么必须有这一层
 *
 * Qoder 的切换是「**写客户端登录态**」—— 与 WorkBuddy 的切换同级，渠道页因此
 * 必须先弹确认（`QoderAccountsPage` 的注释：「写客户端登录态，与 WB 的切换同级
 * ⇒ 必须先确认」）。聚合页此前直接在 `renderCard` 里
 * `void api.qoderSwitchAccount(...)` ⇒ 点一下页脚胶囊立刻改写登录态，没有确认。
 *
 * ## 与 Trae 那层的差别
 *
 * Qoder **没有「重启客户端」这一档**（不涉及 Electron 单实例重启），所以确认框是
 * 普通 `Dialog`（确认 / 取消），不是 `RestartConfirmDialog`（后者会多一个
 * 「自动重开」勾选 —— 摆一个对 Qoder 无效的勾选就是假承诺）。
 *
 * ## 客户端在跑 ⇒ 确认框换成「关闭并继续」（不换弹窗、不丢上下文）
 *
 * 进程守卫会拒绝「客户端在跑时写回」，但**点确认时才现查**（`programs` 是加载快照，
 * 用户可能在那之后才打开客户端）。查到在跑就**不写、不报错**，把同一个确认框的正文换成
 * 「它正在运行」并给出「关闭并继续」—— 与渠道页 `QoderAccountsPage` 同一行为
 * （#89 的教训：聚合页复用卡片 ≠ 行为一致，少传一个 prop 只会静默降级）。
 */

import { useCallback, useState } from "react";
import { toast } from "sonner";

import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { aggregateDisplayName, type AggregateAccount } from "@/lib/aggregate-accounts";
import type { QoderProgramStatus } from "@/lib/api";

export interface AggregateQoderSwitcher {
  /** 请求切换（**先弹确认框**，不直接执行）。 */
  request: (account: AggregateAccount, program: QoderProgramStatus) => void;
  /** 正在切换中的账号 uid（用于禁用该卡的程序位按钮）。 */
  busyUid: string | null;
  /** 任一账号正在切换中 ⇒ 锁住所有切换按钮。 */
  switchBusy: boolean;
  /** 确认框 props（页面渲染普通 `Dialog`）。 */
  confirmProps: {
    open: boolean;
    /** 账号展示名（确认框正文要能指名道姓）。 */
    name: string;
    /** 目标程序位名（取后端下发的 `label`，不由区域反推）。 */
    programLabel: string;
    /** 目标客户端此刻在运行 ⇒ 正文换成「正在运行」，主按钮换成「关闭并继续」。 */
    running: boolean;
    onConfirm: () => void;
    /** 先关闭客户端、再写回（只在 `running` 为真时有意义）。 */
    onCloseAndSwitch: () => void;
    onOpenChange: (open: boolean) => void;
  };
}

interface Pending {
  account: AggregateAccount;
  program: QoderProgramStatus;
}

/** @param onSwitched 切换成功后的回调（页面据此重取账号列表与程序位信号）。 */
export function useAggregateQoderSwitcher(onSwitched?: () => void): AggregateQoderSwitcher {
  const t = useT();
  const [pending, setPending] = useState<Pending | null>(null);
  const [busyUid, setBusyUid] = useState<string | null>(null);
  /** 现查结果：目标客户端确实在跑（确认框据此刻画「关闭并继续」）。 */
  const [running, setRunning] = useState(false);

  const request = useCallback((account: AggregateAccount, program: QoderProgramStatus) => {
    setRunning(false);
    setPending({ account, program });
  }, []);

  /**
   * 真正执行切换。
   *
   * `closeFirst = false` 时先**现查**一次进程状态：查到在跑 ⇒ 撤掉 loading toast、
   * 保留确认框并翻到「关闭并继续」，**一次写操作都不做**（写下去也会被后端守卫拒绝，
   * 只会给用户一条没有出路的错误 toast）。
   */
  const execute = useCallback(
    async (target: Pending, closeFirst: boolean) => {
      /* 收窄到 Qoder（`data.uid` 只有 Qoder 账号有）；调用点已判过一次，这里再判一次
         是为了让类型系统也认这件事。 */
      if (target.account.platform !== "qoder") return;
      const scope = target.account.scope;
      const uid = target.account.data.uid;
      const description = `${aggregateDisplayName(target.account)} · ${target.program.label}`;
      setBusyUid(uid);
      const toastId = toast.loading(t("aggregate.toast.switching"), { description });
      try {
        if (!closeFirst) {
          const fresh = await api.qoderProgramStatus(scope);
          const current = fresh.programs.find((item) => item.id === target.program.id);
          if (current?.running) {
            toast.dismiss(toastId);
            setPending({ account: target.account, program: current });
            setRunning(true);
            return;
          }
        }
        await api.qoderSwitchAccount(scope, target.program.id, uid, closeFirst);
        toast.success(t("wbAccounts.toast.configUpdated"), { id: toastId, description });
        setPending(null);
        setRunning(false);
        onSwitched?.();
      } catch (error) {
        toast.error(api.asError(error), { id: toastId });
      } finally {
        setBusyUid(null);
      }
    },
    [onSwitched, t],
  );

  const onConfirm = useCallback(() => {
    if (!pending || pending.account.platform !== "qoder") return;
    void execute(pending, false);
  }, [execute, pending]);

  const onCloseAndSwitch = useCallback(() => {
    if (!pending) return;
    void execute(pending, true);
  }, [execute, pending]);

  return {
    request,
    busyUid,
    switchBusy: busyUid !== null,
    confirmProps: {
      open: pending !== null,
      name: pending ? aggregateDisplayName(pending.account) : "",
      programLabel: pending?.program.label ?? "",
      running,
      onConfirm,
      onCloseAndSwitch,
      onOpenChange: (open) => {
        if (!open) {
          setPending(null);
          setRunning(false);
        }
      },
    },
  };
}
