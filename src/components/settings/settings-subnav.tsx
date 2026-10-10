/**
 * 设置页 **9 项子导航**（P7 §3.9）—— 数据驱动。
 *
 * ## 为什么数据驱动
 *
 * 9 项的名字 / 图标 / 顺序只在这里声明一次，页面按 `active` 渲染对应分区。
 * 加一项 = 加一行（§4.2 同一思路），不需要在页面里再写一遍 `if`。
 *
 * ## 9 项从哪来（§3.9）
 *
 * 设置的真实内容是 **WB 7 组 + Trae 10 组**，6 项装不下，逐项核对后收敛为 9 项：
 * `通用 / 账号与登录 / 签到与积分 / 网关 / 计划任务 / 客户端 / 权限与诊断 / 日志 / 备份与恢复`。
 * 其中「备份与恢复」（第 9 项）是 P7 新增。
 *
 * ## 为什么不做成「可折叠 + 记住位置」
 *
 * 子导航回答的是「设置里有哪几类」，不是「我在哪一步」；URL 尾段才是位置真值
 * （PLAN §3.9 的层级表）。本组件只负责**切换视图**，把选中态交给调用方
 * （`active` / `onSelect`），不自持状态 —— 否则「页面刷新后回到第一项」这类
 * 问题会在两个地方各修一遍。
 */

import {
  CalendarCheck,
  Clock,
  DatabaseBackup,
  Monitor,
  ScrollText,
  Server,
  Settings2,
  ShieldCheck,
  UserRound,
} from "lucide-react";

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
  | "diagnostics"
  | "logs"
  | "backup";

export type SettingsSubnavItem = SideSubnavItem<SettingsSubnavId>;

/** 唯一的一份子导航表（顺序即展示顺序）。 */
export const SETTINGS_SUBNAV: readonly SettingsSubnavItem[] = [
  { id: "general", labelKey: "settings.subnav.general", icon: Settings2 },
  { id: "account", labelKey: "settings.subnav.account", icon: UserRound },
  { id: "checkin", labelKey: "settings.subnav.checkin", icon: CalendarCheck },
  { id: "gateway", labelKey: "settings.subnav.gateway", icon: Server },
  { id: "schedule", labelKey: "settings.subnav.schedule", icon: Clock },
  { id: "client", labelKey: "settings.subnav.client", icon: Monitor },
  { id: "diagnostics", labelKey: "settings.subnav.diagnostics", icon: ShieldCheck },
  { id: "logs", labelKey: "settings.subnav.logs", icon: ScrollText },
  { id: "backup", labelKey: "settings.subnav.backup", icon: DatabaseBackup },
];

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
