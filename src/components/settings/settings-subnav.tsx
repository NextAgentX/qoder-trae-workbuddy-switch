/**
 * 设置页子导航 —— 数据驱动。
 *
 * ## 这里只放**产品设置**
 *
 * 应用框架级设置（语言 / 外观 / 快捷键 / 启动与托盘 / 授权 / 更新 / 作用域可见性 /
 * 积分自动刷新）的唯一入口是顶栏 ⚙「应用设置」抽屉（`shell/app-settings-drawer.tsx`）。
 * 此前设置页还有一项「通用」渲染**同一份** `AppSettingsGroups` ⇒ 同一个开关在两条
 * 路径上各出现一次，用户分不清「改哪个才对」。该分区已删除。
 *
 * 「网关」也同理：网关开关 / 监听 / 端口 / 请求日志在网关工作台
 *（`gateway/gateway-subnav.tsx`）里有完整分区，设置页不再重复一份。
 *
 * ⇒ 设置页现在的职责只有一句话：**某个产品 / 作用域自己的设置**。
 *
 * ## 一项在各平台的可见性
 *
 * `SETTINGS_SUBNAV` 声明**全集与顺序**；每个产品用一组白名单收窄
 *（`WB_SETTINGS_SECTIONS` / `TRAE_SETTINGS_SECTIONS` / `QODER_SETTINGS_SECTIONS`），
 * **不复制表** —— 复制即漂移的种子。白名单只表达「这个产品有没有内容」，不改变顺序。
 *
 * ⚠️ `network` 只有 Qoder 有内容（出站代理按区域）。WB / Trae 若仍列出它，
 * 点进去就是一片空白 —— 那正是白名单要挡的东西。
 *
 * ## 为什么不做成「可折叠 + 记住位置」
 *
 * 子导航回答的是「设置里有哪几类」，不是「我在哪一步」；URL 尾段才是位置真值
 *（PLAN §3.9 的层级表）。本组件只负责**切换视图**，把选中态交给调用方
 *（`active` / `onSelect`），不自持状态 —— 否则「页面刷新后回到第一项」这类
 * 问题会在两个地方各修一遍。
 */

import { SideSubnav, type SideSubnavItem } from "@/components/shell/side-subnav";
import { useT } from "@/lib/i18n";

/** 设置分区的标识（全部是产品设置；应用框架设置不在设置页）。 */
export type SettingsSubnavId =
  | "account"
  | "checkin"
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
  { id: "account", labelKey: "settings.subnav.account" },
  { id: "checkin", labelKey: "settings.subnav.checkin" },
  { id: "schedule", labelKey: "settings.subnav.schedule" },
  { id: "client", labelKey: "settings.subnav.client" },
  /* Qoder 逐包取数的出站代理（按区域）。目前只有 Qoder 有内容。 */
  { id: "network", labelKey: "settings.subnav.network" },
  /* CodeBuddy CLI 的**接入方式**（二选一）。放这里而不是账号卡：
     它是全局一次性配置（CLI 的凭据只有一份 `~/.codebuddy/settings.json`），
     而账号卡上的 `cliControl` 是「按账号切默认账号」，另一层次。 */
  { id: "cli", labelKey: "settings.subnav.cli" },
  { id: "diagnostics", labelKey: "settings.subnav.diagnostics" },
  { id: "logs", labelKey: "settings.subnav.logs" },
  { id: "backup", labelKey: "settings.subnav.backup" },
];

/** WorkBuddy：除 Qoder 专属的「网络」外全有内容。 */
export const WB_SETTINGS_SECTIONS = SETTINGS_SUBNAV.filter((item) => item.id !== "network");

/** Trae：「网络与代理」并进「客户端」（PLAN §3.9），且没有 CodeBuddy CLI。 */
export const TRAE_SETTINGS_SECTIONS = SETTINGS_SUBNAV.filter(
  (item) => item.id !== "network" && item.id !== "cli",
);

/**
 * Qoder：五处有真内容 —— 账号与登录、计划任务（签到 / 令牌保活）、
 * 客户端检测、网络（出站代理）、备份与恢复。其余项在 Qoder 侧**没有设置项**，
 * 摆上去就是假控件（点了只有一句「尚未接入」，比不摆更糟）。
 * 对应功能落地后，把它的 id 加进这里即可。
 */
const QODER_SECTION_IDS: readonly SettingsSubnavId[] = [
  "account",
  "client",
  "network",
  "schedule",
  "backup",
];

export const QODER_SETTINGS_SECTIONS = SETTINGS_SUBNAV.filter((item) =>
  QODER_SECTION_IDS.includes(item.id),
);

/**
 * 每个设置分区的**作用域归属**（issue #122）。
 *
 * 设置页始终带着作用域徽标，但里面的项目分两类：一类改了对所有作用域生效，一类只影响
 * 当前作用域。界面此前不区分，用户的原话是「看起来不清楚哪些是改了影响所有作用域的」。
 *
 * ## 判据（可复核，不是拍脑袋）
 *
 * 看该项的**读写命令签名**里有没有 `scope` / `region`：
 *
 * | 分区 | 归属 | 依据 |
 * |:--|:--|:--|
 * | account | 作用域 | 账号库按产品 + 区域分家（每个作用域一份） |
 * | checkin | 全局 | `save_auto_checkin_config(config)` —— **无 scope 参数** |
 * | schedule | 全局 | `save_schedule_config(config)` —— 无 scope 参数 |
 * | client | 作用域 | 客户端探测按作用域（探测命令带 region） |
 * | network | 作用域 | Qoder 出站代理**按区域** |
 * | cli | 全局 | CodeBuddy CLI 凭据只有一份（`~/.codebuddy/settings.json`） |
 * | diagnostics | 全局 | 诊断 / 权限是本机能力，与作用域无关 |
 * | logs | 全局 | 运行日志按本机存一份 |
 * | backup | 全局 | `save_auto_backup_config(config)` —— 无 scope 参数 |
 *
 * ★ 改这张表之前先按同一判据复核命令签名：**别凭界面观感归类** ——
 * 「页面上带着作用域徽标」不等于「这项按作用域存」。
 */
export const SETTINGS_SCOPE: Record<SettingsSubnavId, "global" | "scoped"> = {
  account: "scoped",
  checkin: "global",
  schedule: "global",
  client: "scoped",
  network: "scoped",
  cli: "global",
  diagnostics: "global",
  logs: "global",
  backup: "global",
};

/**
 * 非法 / 缺失 `?section=` 时的落点。
 *
 * 不用字号更小的「第一项」字面量：调用方（三个设置页 + 深链解析）都要能引用它，
 * 否则改一次默认分区就要改三处。
 */
export const DEFAULT_SETTINGS_SUBNAV: SettingsSubnavId = "account";

/** 从 URL 段解析设置分区（非法值 fail-closed 回 {@link DEFAULT_SETTINGS_SUBNAV}，不把任意串塞进 state）。 */
export function parseSettingsSubnav(raw: string | null): SettingsSubnavId {
  return SETTINGS_SUBNAV.some((item) => item.id === raw)
    ? (raw as SettingsSubnavId)
    : DEFAULT_SETTINGS_SUBNAV;
}

interface Props {
  active: SettingsSubnavId;
  onSelect: (id: SettingsSubnavId) => void;
  /** 覆盖表（默认用 `SETTINGS_SUBNAV`）；各产品传自己的白名单。 */
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
