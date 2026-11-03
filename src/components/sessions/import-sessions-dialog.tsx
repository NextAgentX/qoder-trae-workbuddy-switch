/**
 * issue #87 **导入会话**的确认对话框。
 *
 * ## 为什么导入必须二次确认，而导出不用
 *
 * 导出**只读**（只写用户选的那个文件），点错了重来即可；导入**写用户数据**
 * （往 `projects/` 加正文、往 `sessions` 表加行）⇒ 必须先让用户看见「这个包里有什么」。
 *
 * ## 文案里必须说清的两件事
 *
 * 1. **只新增，不覆盖**：每条都生成新 id，不会改动任何现有会话 —— 用户最怕的是
 *    「导入把我的历史冲掉了」；
 * 2. **同一份包导两次就是两份**：不做静默去重。宁可说在前面，也不要事后解释。
 *
 * 包里带的会话可能没有索引行（`hasRow: false`）—— 那种情况客户端列表里**看不到**，
 * 只能靠正文存在。这一点也在下面如实标出，不让用户以为「导入成功但东西没了」。
 */

import { FileDown, TriangleAlert } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useT } from "@/lib/i18n";
import type { SessionBundlePreview } from "@/lib/types";

export interface ImportSessionsDialogProps {
  /** 包预览；`null` = 关闭（由页面控制）。 */
  preview: SessionBundlePreview | null;
  /** 导入进行中（按钮进「操作进行中」态，防重复提交）。 */
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  /** 用户确认后回调（页面负责调 `importSessionsBundle` 并 toast 结果）。 */
  onConfirm: () => void;
}

export function ImportSessionsDialog({
  preview,
  busy,
  onOpenChange,
  onConfirm,
}: ImportSessionsDialogProps) {
  const t = useT();
  const open = preview !== null;
  const count = preview?.count ?? 0;
  const shown = preview?.sessions.slice(0, 5) ?? [];
  // 包里没带索引行的条数：这些导入后**客户端列表里看不到**，必须提前说。
  const withoutRow = (preview?.sessions ?? []).filter((item) => !item.hasRow).length;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[520px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <FileDown className="size-4" aria-hidden="true" />
            {t("sessions.bundle.importTitle")}
          </DialogTitle>
          <DialogDescription>{t("sessions.bundle.importDesc", { n: count })}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-2.5">
          {shown.length > 0 ? (
            <ul className="flex flex-col gap-1.5">
              {shown.map((item, index) => (
                <li
                  key={`${item.sourceCid ?? "?"}-${index}`}
                  className="flex items-center gap-2 rounded-[10px] bg-muted px-3 py-2 text-[12.5px]"
                >
                  <span className="min-w-0 flex-1 truncate">{item.title ?? "（无标题）"}</span>
                  {item.hasRow ? null : (
                    <Badge variant="outline" className="shrink-0 rounded-md">
                      {t("sessions.bundle.noRow")}
                    </Badge>
                  )}
                </li>
              ))}
              {count > shown.length ? (
                <li className="px-3 text-[12px] text-muted-foreground">
                  {t("sessions.bundle.more", { n: count - shown.length })}
                </li>
              ) : null}
            </ul>
          ) : null}

          {withoutRow > 0 ? (
            <div className="flex items-start gap-2 rounded-[10px] border-l-[3px] bg-muted px-3 py-2 text-[12px] text-muted-foreground">
              <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
              <span>{t("sessions.bundle.noRowHint", { n: withoutRow })}</span>
            </div>
          ) : null}
        </div>

        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={() => onOpenChange(false)}>
            {t("sessions.bundle.cancel")}
          </Button>
          <Button disabled={busy || count === 0} onClick={onConfirm}>
            {busy ? t("sessions.bundle.importing") : t("sessions.bundle.confirm", { n: count })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
