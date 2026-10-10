import { useState } from "react";
import { Blocks, Coffee, FileJson2, Monitor, Moon, Settings, SlidersHorizontal, Sun } from "lucide-react";

import { AppSettingsDialog } from "@/components/app-settings";
import { ClientSettingsDialog } from "@/components/client-settings";
import { DonateDialog } from "@/components/donate-dialog";
import { SkillManagerDialog } from "@/components/skill-manager";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useT } from "@/lib/i18n";
import { getThemePreference, setThemePreference, type ThemePreference } from "@/lib/theme";
import { cn } from "@/lib/utils";

/**
 * 顶栏应用级入口（L0）—— 齿轮溢出菜单。
 *
 * ## 为什么把四个弹层收在一枚菜单里（而不是顶栏排四颗按钮）
 *
 * 这是**低频**操作（通用设置 / 客户端设置 / Skill 管理 / 捐赠），顶栏的空间要留给
 * 高频的作用域与搜索。收进溢出菜单后，顶栏固定为「作用域 + 运行态 + 搜索 + 刷新 +
 * 主题 + 设置」，与原型 `index.html:14-45` 一致。
 *
 * ## 为什么这里只做「驱动」，不重写业务
 *
 * 四个弹层各是一整域实现（`AppSettingsDialog` / `ClientSettingsDialog` /
 * `SkillManagerDialog` / `DonateDialog`），本组件只持有它们的 `open` 状态。
 * 弹层内容**懒挂载**（Radix Dialog 默认卸载）⇒ 不开就不取数，与原先侧栏入口同性能。
 */
export function AppMenu() {
  const t = useT();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [clientOpen, setClientOpen] = useState(false);
  const [skillsOpen, setSkillsOpen] = useState(false);
  const [donateOpen, setDonateOpen] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={t("topbar.appMenu")}
            title={t("topbar.appMenu")}
            className={iconButtonClass}
          >
            <Settings className="size-[17px]" aria-hidden="true" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-52">
          <DropdownMenuItem onSelect={() => setSettingsOpen(true)}>
            <SlidersHorizontal aria-hidden="true" />
            {t("appSettings.entry")}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setClientOpen(true)}>
            <FileJson2 aria-hidden="true" />
            {t("clientSettings.entry")}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setSkillsOpen(true)}>
            <Blocks aria-hidden="true" />
            {t("skillManager.entry")}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setDonateOpen(true)}>
            <Coffee aria-hidden="true" />
            {t("wbSettings.donate.trigger")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      {/* 四个受控弹层：内容懒挂载，关闭即卸载。 */}
      <AppSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
      <ClientSettingsDialog open={clientOpen} onOpenChange={setClientOpen} />
      <SkillManagerDialog open={skillsOpen} onOpenChange={setSkillsOpen} />
      <DonateDialog open={donateOpen} onOpenChange={setDonateOpen} />
    </>
  );
}

/** 三态选项 → 图标（`system` 跟随系统用显示器）。 */
const THEME_META: Record<ThemePreference, { icon: typeof Sun; labelKey: "appSettings.theme.system" | "appSettings.theme.light" | "appSettings.theme.dark" }> = {
  system: { icon: Monitor, labelKey: "appSettings.theme.system" },
  light: { icon: Sun, labelKey: "appSettings.theme.light" },
  dark: { icon: Moon, labelKey: "appSettings.theme.dark" },
};

/**
 * 顶栏主题菜单 —— **三态选择**（系统 / 浅色 / 深色）。
 *
 * ⚠️ **不做二态假开关**：`theme.ts` 的偏好本就三态（`system` 是独立选项，
 * 不是「跟随系统时把按钮按当前系统值显示」）。压成明/暗二态会让「跟随系统」
 * 这一态在界面上无处可表达，用户也关不掉它。
 */
export function ThemeMenu() {
  const t = useT();
  const [preference, setPreference] = useState<ThemePreference>(getThemePreference);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={t("topbar.theme")}
          title={t("topbar.theme")}
          className={iconButtonClass}
        >
          <Sun className="size-[17px]" aria-hidden="true" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-40">
        {(Object.keys(THEME_META) as ThemePreference[]).map((key) => {
          const { icon: Icon, labelKey } = THEME_META[key];
          return (
            <DropdownMenuItem
              key={key}
              onSelect={() => {
                setThemePreference(key);
                setPreference(key);
              }}
              className={cn(key === preference && "bg-accent")}
            >
              <Icon aria-hidden="true" />
              {t(labelKey)}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** 顶栏图标按钮样式（与原型 `.icon-btn` 对齐：34px 方角圆角 + hover 底色）。 */
export const iconButtonClass = cn(
  /* 尺寸对齐原型 `.icon-btn`（`buddy-switch-prototype.html:143-147`）：34×34 / 圆角 10px。 */
  "grid size-[34px] shrink-0 place-items-center rounded-[10px] text-muted-foreground outline-none transition-colors",
  "hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/40",
);