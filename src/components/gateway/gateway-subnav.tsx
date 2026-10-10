/**
 * 网关工作台**左侧子导航**（PLAN §3.9）—— 数据驱动。
 *
 * ## 为什么需要它
 *
 * 改造前「API 服务」页是 **6 张卡纵向堆叠**（`PLAN.md:125`）：找不到「还有哪几类」，
 * 想改一个端口要滚过密钥表 / 模型清单 / 请求日志。原型 `viewGateway`
 * （`demo-app.js:1160`）给出的是**左侧子导航 + 主区**：
 * `概览 / 密钥 / 模型 / 账号池 / 请求代理 / 请求日志`。
 *
 * ## 一份表，两个产品线共用
 *
 * WorkBuddy 与 Trae 的网关是**两套后端**（上游、端口、Key 模型都不同），
 * 但**分类完全相同** ⇒ 表只声明一次，两页各自渲染自己的内容。
 * 词条落在 `shell` 域（跨产品线的应用级导航）。
 *
 * ⚠️ Trae **没有**「请求代理」（P12 的 C 路只覆盖 WorkBuddy 网关端口），
 * 故 Trae 页**不渲染**该项 —— 缺能力就不画空壳（§3.4 规则 1），
 * 由页面按 `capabilities` 过滤，而不是在这里写死平台判断。
 */

import { Boxes, Gauge, KeyRound, Layers, Route, ScrollText } from "lucide-react";

import { SideSubnav, type SideSubnavItem } from "@/components/shell/side-subnav";
import { useT } from "@/lib/i18n";

/** 网关工作台的 6 个分区。 */
export type GatewaySubnavId = "overview" | "keys" | "models" | "pool" | "proxy" | "logs";

/** 唯一的一份子导航表（顺序即展示顺序）。 */
export const GATEWAY_SUBNAV: readonly SideSubnavItem<GatewaySubnavId>[] = [
  { id: "overview", labelKey: "gateway.subnav.overview", icon: Gauge },
  { id: "keys", labelKey: "gateway.subnav.keys", icon: KeyRound },
  { id: "models", labelKey: "gateway.subnav.models", icon: Boxes },
  { id: "pool", labelKey: "gateway.subnav.pool", icon: Layers },
  { id: "proxy", labelKey: "gateway.subnav.proxy", icon: Route },
  { id: "logs", labelKey: "gateway.subnav.logs", icon: ScrollText },
];

/** 从 URL 段解析子导航（非法值 fail-closed 回 `overview`，不把任意串塞进 state）。 */
export function parseGatewaySubnav(raw: string | null): GatewaySubnavId {
  return GATEWAY_SUBNAV.some((item) => item.id === raw)
    ? (raw as GatewaySubnavId)
    : "overview";
}

interface Props {
  active: GatewaySubnavId;
  onSelect: (id: GatewaySubnavId) => void;
  /** 覆盖表（默认全 6 项）；Trae 页传去掉「请求代理」的表。 */
  items?: readonly SideSubnavItem<GatewaySubnavId>[];
  className?: string;
}

/** 网关子导航（版式由 `SideSubnav` 统一提供）。 */
export function GatewaySubnav({ active, onSelect, items = GATEWAY_SUBNAV, className }: Props) {
  const t = useT();
  return (
    <SideSubnav
      active={active}
      items={items}
      onSelect={onSelect}
      ariaLabel={t("gateway.subnav.aria")}
      className={className}
    />
  );
}