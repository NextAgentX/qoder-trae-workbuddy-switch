/**
 * 网关对外地址的**唯一派生入口**（issue #165）。
 *
 * ## 为什么单独成文件（而不是塞进 `lib/gateway.ts`）
 *
 * `lib/gateway.ts` 依赖 `@/lib/i18n` 与类型模块；门禁 `scripts/check-gateway-url.mjs`
 * 要用 esbuild 直接跑**生产代码**做断言（与 `check-credit-packs` 同款），这里保持零依赖。
 *
 * ## 两档语义，全仓只此一份
 *
 * - `gatewayRootUrl` → 根地址 `http://host:port`（**不含** `/v1`）：Anthropic 系客户端要它；
 * - `gatewayOpenAiBaseUrl` → 根地址 + `/v1`：OpenAI 系客户端要它。
 *
 * 历史教训（#165）：变量名叫 `baseUrl`、实际是根地址，展示与复制却按「OpenAI 基址」的期待在用，
 * 于是同一屏上「接入地址」写 `…:57892`、「接入说明」写 `…:57892/v1`，照抄前者必然 404。
 * 现在语义由**函数名**钉死，四个网关页一律调用，禁止再手拼 `http://${host}:${port}`。
 *
 * ★ `0.0.0.0` 是**监听**通配地址，不是可路由的目标地址 ⇒ 展示 / 复制一律归一回环。
 */

/** 回环地址（展示与复制的默认主机）。 */
export const GATEWAY_LOOPBACK = "127.0.0.1";

/** 监听通配地址：只能监听，不能当目标地址。 */
export const GATEWAY_LAN = "0.0.0.0";

/** 归一「给用户看」的主机：空值 / 通配地址 → 回环。 */
export function gatewayDisplayHost(rawHost?: string | null): string {
  const host = rawHost?.trim();
  if (!host || host === GATEWAY_LAN) return GATEWAY_LOOPBACK;
  return host;
}

/** 根地址（**不含** `/v1`）。端口缺失时只给 `http://host`，由调用方决定显隐。 */
export function gatewayRootUrl(rawHost?: string | null, port?: number | string | null): string {
  const host = gatewayDisplayHost(rawHost);
  const value = typeof port === "number" ? String(port) : (port ?? "").trim();
  return value ? `http://${host}:${value}` : `http://${host}`;
}

/** OpenAI 系 Base URL（**含** `/v1`）—— 概览行显示并复制的就是它。 */
export function gatewayOpenAiBaseUrl(
  rawHost?: string | null,
  port?: number | string | null,
): string {
  return `${gatewayRootUrl(rawHost, port)}/v1`;
}

/**
 * 把任意形态（根 / 已带 `/v1`）收敛成根地址。
 *
 * ★ 后端 `GatewayStatusView.base_url` 是**根地址**，而 `resolveGatewayBaseUrl` 的兜底分支
 * 历史上返回带 `/v1` 的串 —— 两条分支形态不同正是 #165 的结构性来源。收敛一次再拼，
 * 就不可能拼出 `/v1/v1`。
 */
export function stripV1Suffix(url: string): string {
  return url.replace(/\/+$/, "").replace(/\/v1$/, "");
}

/**
 * 收敛「后端 `base_url` 优先 / 按监听地址自拼」两条分支，**恒定**返回 OpenAI 基址（含 `/v1`）。
 *
 * 这就是 #165 的结构性根因所在：两条分支历史上返回不同形态（后端是根地址、兜底带 `/v1`），
 * 消费方按「带 `/v1`」用 ⇒ 主路径少一层 ⇒ 404。这里先收敛再拼，形态只有一种。
 */
export function gatewayOpenAiBaseUrlFrom(
  rawBaseUrl: string | null | undefined,
  rawHost?: string | null,
  port?: number | string | null,
): string {
  const raw = rawBaseUrl?.trim();
  if (raw) return `${stripV1Suffix(raw)}/v1`;
  return gatewayOpenAiBaseUrl(rawHost, port);
}
