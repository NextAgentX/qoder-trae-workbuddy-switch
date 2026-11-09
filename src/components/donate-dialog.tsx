import { ExternalLink, Heart, Star } from "lucide-react";

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
 * 爱发电主页（**账号级**，覆盖作者 GitHub 下的所有项目，不限于 Buddy Switch）。
 *
 * 只此一处使用，因此不做成 `lib/` 常量 —— 与 `GITHUB_*` 不同，它不参与更新检查、
 * 不被任何其它模块读取。
 */
const IFDIAN_URL = "https://ifdian.net/a/NextAgentX";

/**
 * 打赏入口：点击后弹出收款码。
 *
 * **两个入口共用这一个弹层**：顶栏 ⚙ 菜单项（`app-menu.tsx`）与侧栏底部
 * 「版本号上方」的整行按钮（`App.tsx` 的 `SidebarDonate`）。两个入口各自持有
 * 自己的 `open`，因此收款码 UI 只有这一处实现（同款先例见「更新」弹层）。
 *
 * ## 为什么做成弹窗而不是直接铺开两张码
 *
 * 收款码是竖版海报（约 744×1080），铺开会把内容挤出视野；而打赏是**低频**操作，
 * 不值得长期占据固定高度。
 *
 * ## 受控形态
 *
 * `open` / `onOpenChange` 由调用方驱动（顶栏 ⚙「应用设置」抽屉里的入口 / 侧栏底部入口）；
 * 本组件**不自带触发按钮**（与 `SkillManagerDialog` / `ClientSettingsDialog` 同构）。
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
        {/* 两张收款码的像素尺寸不同（微信 425×422 / 支付宝 799×803），若各自按宽度铺满，
            高度会有偏差、下面的渠道名对不齐。这里给**统一高度的容器 + object-contain**：
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
        {/* ★ 爱发电：唯一「不扫码」的通道，做成**条状**而不是第三张码 ——
            它没有收款码图，硬塞进两列 grid 会破等高，且它的主页里本来就带支付入口。
            位置紧跟收款码：这是「已经决定掏钱」的人的第二选择（境外用户往往只有这一条路）。 */}
        <button
          type="button"
          onClick={() => void openReleaseUrl(IFDIAN_URL)}
          className="flex flex-wrap items-center justify-center gap-x-2 gap-y-1 rounded-lg border border-border bg-muted/30 px-3 py-2 text-[12.5px] leading-relaxed text-muted-foreground transition-colors hover:border-primary/40 hover:text-foreground"
        >
          <Heart className="size-3.5 shrink-0 text-rose-500" aria-hidden="true" />
          <span>{t("wbSettings.donate.ifdianLead")}</span>
          <span className="shrink-0 font-medium text-primary">
            {t("wbSettings.donate.ifdianCta")}
          </span>
        </button>
        {/* ★ Star 引导：放在两条打赏入口下方、动作条上方 —— 打开这个弹层的人正处在「愿意支持」
            的意愿峰值，而 Star 对他是**零成本**动作，转化率天然高于打赏。
            链接指向仓库首页（Star 按钮在那里），不直连 `/stargazers`（那是名单，不是动作）。 */}
        <button
          type="button"
          onClick={() => void openReleaseUrl(GITHUB_REPOSITORY_URL)}
          className="flex flex-wrap items-center justify-center gap-x-2 gap-y-1 rounded-lg border border-border bg-muted/30 px-3 py-2 text-[12.5px] leading-relaxed text-muted-foreground transition-colors hover:border-primary/40 hover:text-foreground"
        >
          <Star className="size-3.5 shrink-0 text-amber-500" aria-hidden="true" />
          <span>{t("wbSettings.donate.starLead")}</span>
          <span className="shrink-0 font-medium text-primary">{t("wbSettings.donate.starCta")}</span>
        </button>
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
