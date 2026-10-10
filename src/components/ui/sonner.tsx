import {
  AlertTriangle,
  CheckCircle2,
  Info,
  Loader2,
  XCircle,
} from "lucide-react";
import { Toaster as SonnerToaster, type ToasterProps } from "sonner";

export function Toaster(props: ToasterProps) {
  return (
    <SonnerToaster
      position="bottom-right"
      closeButton
      gap={8}
      visibleToasts={4}
      icons={{
        success: <CheckCircle2 className="size-4 text-brand" />,
        info: <Info className="size-4 text-brand" />,
        warning: <AlertTriangle className="size-4 text-warning" />,
        error: <XCircle className="size-4 text-destructive" />,
        loading: <Loader2 className="size-4 animate-spin text-brand" />,
      }}
      toastOptions={{
        classNames: {
          toast:
            "!gap-2.5 !rounded-lg !border-border !bg-popover !px-3 !py-2.5 !text-popover-foreground !shadow-overlay",
          content: "!gap-0.5",
          title: "!text-[13px] !font-medium !leading-5",
          // `!whitespace-pre-line`：诊断类错误是**多行**文案（说清产品线 / 缺失来源 /
          // 下一步动作，见 `trae::profile::diagnose_missing_credential`）。默认
          // `white-space: normal` 会把换行折叠成空格，整段挤成一坨，反而读不出重点。
          description: "!text-xs !leading-5 !text-muted-foreground !whitespace-pre-line",
          icon: "!mr-0 !self-start !pt-0.5",
          closeButton:
            "!size-5 !border-border !bg-popover !text-muted-foreground !shadow-sm hover:!bg-accent hover:!text-foreground focus-visible:!ring-2 focus-visible:!ring-brand/30",
          actionButton: "!h-7 !rounded-md !bg-brand !px-2.5 !text-xs !text-brand-foreground",
          cancelButton: "!h-7 !rounded-md !bg-muted !px-2.5 !text-xs !text-muted-foreground",
        },
      }}
      {...props}
    />
  );
}
