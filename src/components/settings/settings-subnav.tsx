/**
 * 设置页 **10 项子导航**（P7 §3.9 + issue #109）—— 数据驱动。
 *
 * ## 为什么数据驱动
 *
 * 各项的名字 / 图标 / 顺序只在这里声明一次，页面按 `active` 渲染对应分区。
 * 加一项 = 加一行（§4.2 同一思路），不需要在页面里再写一遍 `if`。
 *
 * ## 9 项从哪来（§3.9）
 *
 * 设置的真实内容是 **WB 7 组 + Trae 10 组**，6 项装不下，逐项核对后收敛为 9 项；
 * issue #109 又加了第 10 项「CodeBuddy CLI」：
 * `通用 / 账号与登录 / 签到与积分 / 网关 / 计划任务 / 客户端 / CodeBuddy CLI /
 *  权限与诊断 / 日志 / 备份与恢复`。
 * 其中「备份与恢复」是 P7 新增（**必须保持最后一项**），「CodeBuddy CLI」是 WorkBuddy 侧独有。
 *
 * ## 为什么不做成「可折叠 + 记住位置」
 *
 * 子导航回答的是「设置里有哪几类」，不是「我在哪一步」；URL 尾段才是位置真值
 * （PLAN §3.9 的层级表）。本组件只负责**切换视图**，把选中态交给调用方
 * （`active` / `onSelect`），不自持状态 —— 否则「页面刷新后回到第一项」这类
 * 问题会在两个地方各修一遍。
 *
 * ## ★ 一份表，两个产品线共用
 *
 * 原型只有一个 `viewSettings`（`buddy-switch-prototype.html:3230`），靠
 * `r.cap && p.capabilities.includes(r.cap)` 做平台差异 ⇒ **子导航表本身没有平台分支**。
 * 本仓照此办理：WorkBuddy 设置页与 Trae 设置页**接同一个 `SETTINGS_SUBNAV`**，
 * 各自渲染自己的内容（§3.9 归位表：Trae 的 10 组已全部落进这 9 项）。
 * 若某天真出现「无处安放」的项，再按 D12 走平台专属子项，而不是在这里写 `if`。
 */

import { SideSubnav, type SideSubnavItem } from "@/components/shell/side-subnav";
import { useT } from "@/lib/i18n";

/** 9 个设置分区的标识。 */
export type SettingsSubnavId =
  | "general"
  | "account"
  | "checkin"
  | "gateway"
  | "schedule"
  | "client"
  | "network"
  | "cli"
  | "diagnostics"
  | "logs"
  | "backup";

export type SettingsSubnavItem = SideSubnavItem<SettingsSubnavId>;

/** 唯一的一份子导航表（顺序即展示顺序）。 */
export const SETTINGS_SUBNAV: readonly SettingsSubnavItem[] = [
  { id: "general", labelKey: "settings.subnav.general" },
  { id: "account", labelKey: "settings.subnav.account" },
  { id: "checkin", labelKey: "settings.subnav.checkin" },
  { id: "gateway", labelKey: "settings.subnav.gateway" },
  { id: "schedule", labelKey: "settings.subnav.schedule" },
  { id: "client", labelKey: "settings.subnav.client" },
  /* issue #111 收尾：Qoder 逐包取数的出站代理（按区域）。目前只有 Qoder 有内容，
     WB / Trae 设置页仍按自己的 id 收窄，不会渲染这一项。 */
  { id: "network", labelKey: "settings.subnav.network" },
  /* issue #109：CodeBuddy CLI 的**接入方式**（二选一）。放这里而不是账号卡：
     它是全局一次性配置（CLI 的凭据只有一份 `~/.codebuddy/settings.json`），
     而账号卡上的 `cliControl` 是「按账号切默认账号」，另一层次。
     ⚠️ 只有 WorkBuddy 侧有这一项 —— Trae / Qoder 设置页用 `items` 收窄排除它。 */
  { id: "cli", labelKey: "settings.subnav.cli" },
  { id: "diagnostics", labelKey: "settings.subnav.diagnostics" },
  { id: "logs", labelKey: "settings.subnav.logs" },
  { id: "backup", labelKey: "settings.subnav.backup" },
];

/** 从 URL 段解析设置分区（非法值 fail-closed 回 `general`，不把任意串塞进 state）。 */
export function parseSettingsSubnav(raw: string | null): SettingsSubnavId {
  return SETTINGS_SUBNAV.some((item) => item.id === raw) ? (raw as SettingsSubnavId) : "general";
}

interface Props {
  active: SettingsSubnavId;
  onSelect: (id: SettingsSubnavId) => void;
  /** 覆盖表（默认用 `SETTINGS_SUBNAV`）；Trae 设置页若复用可传自己的表。 */
  items?: readonly SettingsSubnavItem[];
  className?: string;
}

/**
 * 设置子导航（版式由 `SideSubnav` 统一提供，这里只钉住自己的**表**与**无障碍名**）。
 */
export function SettingsSubnav({ active, onSelect, items = SETTINGS_SUBNAV, className }: Props) {
  const t = useT();
  return (
    <SideSubnav
      active={active}
      items={items}
      onSelect={onSelect}
      ariaLabel={t("settings.subnav.aria")}
      className={className}
    />
  );
}
