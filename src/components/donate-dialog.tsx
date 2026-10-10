import { ExternalLink } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { GITHUB_REPOSITORY_URL, openReleaseUrl } from "@/lib/update";
import donateAlipay from "@/assets/donate-alipay.jpg";
import donateWechat from "@/assets/donate-wechat.png";

/**
 * 打赏渠道。收款码随前端产物一起打包（`src/assets/`，由 Vite 处理 base 路径），
 * 因此桌面端与 webui、以及 GitHub Pages 演示都能正常显示，不依赖外部图床。
 */
const DONATE_CHANNELS: { key: string; nameKey: TranslationKey; image: string }[] = [
  { key: "wechat", nameKey: "wbSettings.donate.wechat", image: donateWechat },
  { key: "alipay", nameKey: "wbSettings.donate.alipay", image: donateAlipay },
];

/**
 * 打赏入口（顶栏 ⚙ 菜单项驱动）：点击后弹出收款码。
 *
 * ## 为什么做成弹窗而不是直接铺开两张码
 *
 * 收款码是竖版海报（约 744×1080），铺开会把内容挤出视野；而打赏是**低频**操作，
 * 不值得长期占据固定高度。
 *
 * ## 受控形态
 *
 * `open` / `onOpenChange` 由 `app-menu.tsx` 的菜单项驱动；本组件**不自带触发按钮**
 * （与 `AppSettingsDialog` / `SkillManagerDialog` / `ClientSettingsDialog` 同构）。
 */
export function DonateDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const t = useT();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("wbSettings.donate.title")}</DialogTitle>
          <DialogDescription>{t("wbSettings.donate.desc")}</DialogDescription>
        </DialogHeader>
        {/* 两张收款码的长宽比并不相同（1121×1527 / 1080×1620），若各自按宽度铺满，
            高度会差出十几像素、下面的渠道名对不齐。这里给**统一高度的容器 + object-contain**：
            图按自身比例缩放到容器内，多出来的部分留白，两张码的视觉高度与说明位置因此一致。
            **不要用 `object-cover`** —— 那会把二维码裁掉边缘，扫不出来。 */}
        <div className="grid grid-cols-2 gap-4">
          {DONATE_CHANNELS.map((channel) => (
            <figure key={channel.key} className="flex flex-col gap-2">
              <div className="flex h-[320px] items-center justify-center overflow-hidden rounded-lg border border-border bg-muted/30">
                <img
                  src={channel.image}
                  alt={t("wbSettings.donate.qrAlt", { name: t(channel.nameKey) })}
                  className="h-full w-full object-contain"
                />
              </div>
              <figcaption className="text-center text-sm font-medium">{t(channel.nameKey)}</figcaption>
            </figure>
          ))}
        </div>
        <DialogFooter className="sm:justify-between">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => void openReleaseUrl(GITHUB_REPOSITORY_URL)}
          >
            <ExternalLink className="size-4" aria-hidden="true" />
            {t("wbSettings.donate.repoHome")}
          </Button>
          <DialogClose asChild>
            <Button type="button" variant="outline" size="sm">
              {t("wbSettings.common.close")}
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
