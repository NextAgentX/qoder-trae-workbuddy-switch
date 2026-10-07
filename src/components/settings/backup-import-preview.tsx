/**
 * 导入预览卡（P7 T03）—— **导入的安全阀**（§1.7）。
 *
 * ## 它存在的理由
 *
 * 导入是**不可逆写**。用户点「开始导入」之前，必须能看清**这次会改什么**：
 * 逐作用域的「新增 / 覆盖 / 跳过 / 冲突」计数、将被覆盖的配置项、以及
 * 「不含凭据 ⇒ 需重新登录 N 个账号」。
 *
 * 只做后端不做入口 = 用户零收益 + 假承诺风险（P5 回收站踩过的坑）⇒ 预览必须是**可见的**
 * 一层，而不是一句「导入完成」。
 *
 * ## 冲突策略只给三种（§7 #5）
 *
 * `keepLocal`（默认，D16）/ `overwrite` / `keepBoth`。后端枚举里还有一个 `Ask`
 * （逐条询问），但**不渲染入口** —— 逐条 UI 成本高，且「有入口但没接线」本身就是假承诺。
 */

import { AlertTriangle, Loader2 } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { displayText } from "@/lib/display-text";
import { useT, type Translate } from "@/lib/i18n";
import { regionDescriptor } from "@/lib/region";
import { parseScope, scopeParts } from "@/lib/scope";
import type { BackupConflictPolicy, ImportPreview } from "@/lib/types";
import type { TranslationKey } from "@/locales/zh";

/** 冲突策略选项（**只有三种**，§7 #5）。 */
const CONFLICT_OPTIONS: { value: BackupConflictPolicy; labelKey: TranslationKey }[] = [
  { value: "keepLocal", labelKey: "backup.import.conflict.keepLocal" },
  { value: "overwrite", labelKey: "backup.import.conflict.overwrite" },
  { value: "keepBoth", labelKey: "backup.import.conflict.keepBoth" },
];

/**
 * 作用域展示名（`WorkBuddy · 国内版`）。
 *
 * ⚠️ 必须**归一**：预览行的 `scope` 声明为 `string`，但运行时可能拿到任意 JSON
 * ⇒ 对象当 React 子节点会**卸载整棵树**（issue #2）。`displayText` 把非字符串收成
 * `null`，未知作用域退回原始文本（不猜、不回落默认作用域）。
 */
export function backupScopeLabel(scope: string, t: Translate): string {
  const raw = displayText(scope);
  const parsed = parseScope(raw);
  if (!parsed) return raw ?? "";
  const parts = scopeParts(parsed);
  const product = t(parts.product === "workbuddy" ? "product.workbuddy" : "product.trae");
  return `${product} · ${regionDescriptor(parts.region).versionLabel}`;
}

/** 导出时间（ISO 串）→ `YYYY-MM-DD`；拿不到就给 `—`。 */
function exportDay(value: string): string {
  const raw = displayText(value);
  return raw && raw.length >= 10 ? raw.slice(0, 10) : "—";
}

interface Props {
  preview: ImportPreview;
  conflict: BackupConflictPolicy;
  onConflictChange: (policy: BackupConflictPolicy) => void;
  onCancel: () => void;
  onRun: () => void;
  /** 导入进行中（按钮内 `<Loader2>`，**不是**区块加载态）。 */
  pending: boolean;
}

/** 导入预览卡。 */
export function BackupImportPreview({
  preview,
  conflict,
  onConflictChange,
  onCancel,
  onRun,
  pending,
}: Props) {
  const t = useT();
  const rows = Array.isArray(preview.scopes) ? preview.scopes : [];
  const configs = Array.isArray(preview.configOverwrites) ? preview.configOverwrites : [];
  const configItems = configs
    .map((item) => displayText(item))
    .filter((item): item is string => item !== null && item.length > 0);

  return (
    <div className="space-y-3 px-4 py-3 sm:px-5">
      {/* 契约版本头：kind / formatVersion / exportedAt / 含不含凭据 */}
      <div className="space-y-1">
        <p className="text-[13px] font-medium">
          {t("backup.import.preview.head", { kind: "buddy-switch-backup" })}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("backup.import.preview.meta", {
            version: preview.formatVersion,
            exportedAt: exportDay(preview.exportedAt),
            credentials: preview.includesCredentials
              ? t("backup.import.preview.credentialsIncluded")
              : t("backup.import.preview.credentialsExcluded"),
          })}
        </p>
      </div>

      {/* ★ 安全阀：逐作用域「会改什么」 */}
      <div className="divide-y divide-border/50 rounded-md border border-border/60">
        {rows.length === 0 ? (
          <p className="px-3 py-3 text-xs text-muted-foreground">
            {t("backup.import.preview.noScopes")}
          </p>
        ) : (
          rows.map((row, index) => {
            const label = backupScopeLabel(row.scope, t);
            const rejected = row.rejected === true;
            return (
              <div
                key={`${displayText(row.scope) ?? "scope"}-${index}`}
                className="flex flex-wrap items-center justify-between gap-2 px-3 py-2"
              >
                <span className="min-w-0 truncate text-[13px] font-medium">{label}</span>
                {rejected ? (
                  <span className="text-xs text-destructive">
                    {t("backup.import.preview.rejected")}
                  </span>
                ) : (
                  <span className="text-xs tabular-nums text-muted-foreground">
                    {t("backup.import.preview.counts", {
                      added: row.added ?? 0,
                      overwritten: row.overwritten ?? 0,
                      skipped: row.skipped ?? 0,
                      conflicts: row.conflicts ?? 0,
                    })}
                  </span>
                )}
              </div>
            );
          })
        )}
      </div>

      {configItems.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {t("backup.import.preview.configs", {
            count: configItems.length,
            items: configItems.join(" / "),
          })}
        </p>
      )}

      {/* 不含凭据 ⇒ 必须显式告知「需重新登录」（§1.7 断言） */}
      {!preview.includesCredentials && (
        <Alert variant="warning">
          <AlertTriangle />
          <AlertDescription>
            {t("backup.import.preview.relogin", { count: preview.needRelogin ?? 0 })}
          </AlertDescription>
        </Alert>
      )}

      {/* 冲突策略：三选项分段控件 */}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0">
          <div className="text-[13px] font-medium">{t("backup.import.conflict.label")}</div>
          <p className="text-xs leading-4 text-muted-foreground/75">
            {t("backup.import.conflict.desc")}
          </p>
        </div>
        <div
          role="radiogroup"
          aria-label={t("backup.import.conflict.label")}
          className="flex shrink-0 flex-wrap gap-1"
        >
          {CONFLICT_OPTIONS.map((option) => {
            const on = conflict === option.value;
            return (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={on}
                onClick={() => onConflictChange(option.value)}
                className={
                  "cursor-pointer rounded-md border px-2.5 py-1 text-xs transition-colors " +
                  (on
                    ? "border-primary bg-primary/10 font-medium text-foreground"
                    : "border-border text-muted-foreground hover:bg-foreground/[0.04]")
                }
              >
                {t(option.labelKey)}
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" onClick={onCancel} disabled={pending}>
          {t("backup.import.cancel")}
        </Button>
        <Button size="sm" onClick={onRun} disabled={pending}>
          {pending ? <Loader2 className="animate-spin" /> : null}
          {t("backup.import.run")}
        </Button>
      </div>
    </div>
  );
}
