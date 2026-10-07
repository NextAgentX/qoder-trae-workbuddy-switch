/**
 * 第 3 步：**确认页（如实汇总）**（P4 T04）。
 *
 * ## 「如实」的含义（§1.5 / §3.5 不许假承诺）
 *
 * 确认页的数字**必须等于实际会执行的操作**：会话数 = 本次将复制的会话数，
 * 目录数 = **被选中会话覆盖的目录数**（`dirsSelected`）。两者都读
 * `@/lib/session-tree` 的同一份口径 —— 不许出现「展示 N 个、实际只迁一部分」。
 *
 * ## 为什么要「不可逆」提示（决策 #4）
 *
 * 迁移会**写入目标账号的数据**（合并记忆 / 连接器，复制会话），这是**不可逆**的
 * ⇒ §3.5 要求不可逆操作前让用户看清。第 3 步因此在「影响」之外**再加一层**不可逆提示。
 *
 * ## 影响文案（§3.15.3）
 *
 * WorkBuddy 桌面是「需重启」客户端 ⇒ 如实写「客户端将被重启，未保存的编辑器内容可能丢失」。
 * 这里**不写**「自动重开」—— 是否重开由统一重启确认框（`RestartConfirmDialog`）决定，
 * 本页只负责说明**影响**。
 */

import type { ReactNode } from "react";
import { TriangleAlert } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { useT } from "@/lib/i18n";
import type { TreeSelectionStats } from "@/lib/session-tree";
import type { AccountMeta } from "@/lib/types";

export interface MigrationSummaryProps {
  /** 目标账号。 */
  target: AccountMeta;
  /** 目标账号的脱敏身份（邮箱 / uid / id，见 `accountIdentity`）。 */
  targetIdentity: string;
  /** 来源账号展示名；未知时为 `null`（回落「当前登录账号」）。 */
  sourceName: string | null;
  /** 来源作用域展示名（`产品 · 区域`）。 */
  sourceScopeLabel: string;
  /** 目标程序位展示名。 */
  programLabel: string;
  /** 选中统计（`selected` / `dirsSelected` 供「已选 N 个会话 / M 个目录」）。 */
  stats: TreeSelectionStats;
  migrate: { sessions: boolean; memory: boolean; connectors: boolean };
}

/** 一行「标签 → 值」。 */
function SummaryRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex gap-3">
      <dt className="w-16 shrink-0 text-muted-foreground">{label}</dt>
      <dd className="min-w-0 flex-1 break-words">{children}</dd>
    </div>
  );
}

export function MigrationSummary({
  target,
  targetIdentity,
  sourceName,
  sourceScopeLabel,
  programLabel,
  stats,
  migrate,
}: MigrationSummaryProps) {
  const t = useT();
  const targetName =
    target.nickname || target.email || target.uid || t("wbAccounts.common.unknownAccount");

  /* 迁移内容清单：会话用「已选 N 个会话 / M 个目录」，未选会话时写「未选择任何会话」。 */
  const parts: string[] = [];
  if (migrate.sessions) {
    parts.push(
      stats.selected > 0
        ? t("wbAccounts.wizard.confirmSessions", { n: stats.selected, d: stats.dirsSelected })
        : t("wbAccounts.wizard.confirmNoSessions"),
    );
  }
  if (migrate.memory) parts.push(t("wbAccounts.wizard.confirmMemory"));
  if (migrate.connectors) parts.push(t("wbAccounts.wizard.confirmConnectors"));
  const content = parts.length
    ? parts.join(t("wbAccounts.wizard.contentSeparator"))
    : t("wbAccounts.wizard.confirmNone");

  const source = `${sourceName ?? t("wbAccounts.wizard.sourceCurrent")} · ${sourceScopeLabel}`;

  return (
    <div className="space-y-4">
      <dl className="space-y-2 text-sm">
        <SummaryRow label={t("wbAccounts.wizard.confirmTarget")}>
          <span className="font-medium">{targetName}</span>{" "}
          <span className="text-muted-foreground">{targetIdentity}</span>
        </SummaryRow>
        <SummaryRow label={t("wbAccounts.wizard.confirmSource")}>{source}</SummaryRow>
        <SummaryRow label={t("wbAccounts.wizard.confirmProgram")}>{programLabel}</SummaryRow>
        <SummaryRow label={t("wbAccounts.wizard.confirmContent")}>{content}</SummaryRow>
        <SummaryRow label={t("wbAccounts.wizard.impact")}>
          {t("wbAccounts.wizard.impactText")}
        </SummaryRow>
      </dl>

      <Alert variant="warning">
        <TriangleAlert aria-hidden="true" />
        <AlertDescription>{t("wbAccounts.wizard.irreversible")}</AlertDescription>
      </Alert>
    </div>
  );
}
