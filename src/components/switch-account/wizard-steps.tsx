/**
 * 迁移向导的**步骤条**（P4 T01）。
 *
 * ## 为什么单独一个组件
 *
 * 「切换账号 = 抽屉内三步向导（目标 → 迁移内容 → 确认）」是 §3.5 的形态；步骤条要能
 * **一眼看出当前在第几步、还剩几步**。它是纯展示（只吃 `step`），因此独立成一个
 * 组件 —— 三个步骤页共享同一个 `step` 状态（由宿主 `switch-account-dialog` 持有），
 * 步骤条自己不持状态，避免「两个地方各自记进度」而漂移。
 *
 * ## 无障碍
 *
 * 用 `<ol>` 承载（有序步骤），容器给 `aria-label`；当前及已完成的步骤用**文字 + 数字**
 * 双重表达（不靠颜色单独承载语义）—— 色觉障碍用户也能读出进度。
 */

import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** 向导步骤：1 = 目标与来源，2 = 迁移内容，3 = 确认。 */
export type WizardStep = 1 | 2 | 3;

/** 全部步骤（顺序即展示顺序）。 */
const WIZARD_STEPS: readonly WizardStep[] = [1, 2, 3];

export function WizardSteps({ step }: { step: WizardStep }) {
  const t = useT();
  const labels: Record<WizardStep, string> = {
    1: t("wbAccounts.wizard.stepTarget"),
    2: t("wbAccounts.wizard.stepContent"),
    3: t("wbAccounts.wizard.stepConfirm"),
  };

  return (
    <ol className="flex items-center gap-1.5" aria-label={t("wbAccounts.wizard.stepsAria")}>
      {WIZARD_STEPS.map((n, index) => {
        const done = step >= n;
        return (
          <li key={n} className="flex min-w-0 flex-1 items-center gap-1.5">
            {index > 0 ? <span className="h-px flex-1 bg-border" aria-hidden="true" /> : null}
            <span
              className={cn(
                "flex min-w-0 items-center gap-1.5 text-xs",
                done ? "text-foreground" : "text-muted-foreground",
              )}
              aria-current={step === n ? "step" : undefined}
            >
              <span
                className={cn(
                  "flex size-4 shrink-0 items-center justify-center rounded-full border text-[10px] font-medium",
                  done
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-border text-muted-foreground",
                )}
                aria-hidden="true"
              >
                {n}
              </span>
              <span className="truncate">{labels[n]}</span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}
