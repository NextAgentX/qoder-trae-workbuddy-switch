import { toast } from "sonner";

import { asError } from "@/lib/api";
import { t } from "@/lib/i18n";

/**
 * 撤销 toast —— P6 §1.4 / §3.4 的「假承诺」护栏落地。
 *
 * ## 为什么 `undo` 是**必填**（本文件存在的全部理由）
 *
 * PLAN §3.5 红线：**后端没有软删除支撑之前，UI 不许出现「已删除 · 撤销」**。
 * 一个「撤销」按钮如果点了什么也不做（或只关掉 toast），比**没有**撤销更糟 ——
 * 用户会以为账号回来了，实际没有（假承诺）。
 *
 * 所以这里把「后端恢复调用」做成**必填参数**：没有 `undo` 就**构造不出**一个撤销 toast，
 * 于是「无支撑的撤销」在**编译期**就写不出来（`tsc` 变红），而不是靠 review 记得。
 * 唯一能合法调用的前提 = 后端确实有恢复命令（P5 §2.7 的 `account_restore(scope, key)`）。
 *
 * ## 两条边界（都会「让用户以为没救了」，故文案必须诚实）
 *
 * 1. **撤销失败**（账号已被别处恢复、或已过期被启动维护 `sweep_trash_for` 清掉）：
 *    必须给**可理解的反馈**（`toast.error` + 后端原因），**绝不静默** —— 静默会让用户
 *    以为撤销成功。
 * 2. **超时**：这里只让 toast 消失，**不删除任何东西** —— 账号仍在回收站，可随时从
 *    回收站恢复。故文案**不得**暗示「过期即永久删除」（那会把「还有救」说成「没救了」，
 *    与假承诺同类的误导）。
 *
 * ## 连续删多个：每条 toast **独立**
 *
 * 创建时**不传固定 `id`** ⇒ sonner 为每条生成各自的 id ⇒ 删除 A、再删除 B 时两条撤销
 * 互不覆盖。点某条的「撤销」只影响它自己那条（`undo` 是它自己的闭包）。
 */

/**
 * 撤销窗口时长（ms）。**仅决定 toast 停留多久**，与「账号会不会被永久删除」无关 ——
 * 超时后账号仍在回收站（见文件头「超时」）。
 */
export const UNDO_TOAST_DURATION_MS = 8000;

export interface UndoToastInput {
  /** 主标题（已 i18n），如「已删除，可撤销」。 */
  title: string;
  /** 副标题（已 i18n，通常是被删账号的展示名）。 */
  description?: string;
  /** 「撤销」按钮文案（已 i18n）。 */
  undoLabel: string;
  /** 撤销成功后的 toast 文案（已 i18n）。 */
  undoneTitle: string;
  /** ★ 后端恢复调用。**必填** —— 没有它就不该有「撤销」。 */
  undo: () => Promise<void>;
}

/**
 * 弹一条带「撤销」的**成功** toast。
 *
 * 点「撤销」就地执行 `input.undo()`：成功 → 原 toast 变为「已恢复」；失败 → 变为
 * 一条 error（含后端原因）。全程不静默。
 */
export function showUndoToast(input: UndoToastInput): void {
  // 防重入：连点「撤销」只跑一次恢复（按钮不会因此重复打后端）。
  let running = false;
  // ★ 创建时**不传 id** ⇒ 每次删除各自一条，连续删多个互不覆盖。
  const toastId = toast.success(input.title, {
    description: input.description,
    duration: UNDO_TOAST_DURATION_MS,
    action: {
      label: input.undoLabel,
      onClick: (event) => {
        // 保持本条 toast 可见（不立即消失），等恢复结果**就地**更新 —— 否则「点一下没反应」。
        event.preventDefault();
        if (running) return;
        running = true;
        void (async () => {
          try {
            await input.undo();
            toast.success(input.undoneTitle, { id: toastId, description: input.description });
          } catch (error) {
            // ★ 不静默：账号可能已被别处恢复 / 已过期被清理 ⇒ 必须给出可理解的反馈。
            toast.error(t("undo.failedTitle"), { id: toastId, description: asError(error) });
          } finally {
            running = false;
          }
        })();
      },
    },
  });
}
