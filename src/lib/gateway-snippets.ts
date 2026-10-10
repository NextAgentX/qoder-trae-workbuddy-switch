/**
 * 网关接入片段（**P3 合并 → P11 两段式「协议 × 工具」**）。
 *
 * ## ★ 为什么升级为「协议 Tab × 工具 Tab」（P11 §1.7）
 *
 * 协议（OpenAI 兼容 / Anthropic 兼容）是**端点属性**，不是模型属性 —— 同一个模型两种协议
 * 都支持。而**端点路径**与**认证头**由协议决定：
 *
 * | | OpenAI 兼容 | Anthropic 兼容 |
 * |:--|:--|:--|
 * | 端点 | `POST /v1/chat/completions` | `POST /v1/messages` |
 * | 认证头 | `Authorization: Bearer` | `x-api-key` |
 *
 * 把协议做成**第一段 Tab**，认证头就**不可能给错**：给错等于让用户配不通（R7）。
 * 只留「一个工具 Tab」时，协议差异无处安放，正是改造前的缺陷。
 *
 * ## 片段里的「键名」是契约，保持英文硬编码
 *
 * `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `base_url` / `env_key` / `baseURL` 这些键名
 * 是**外部工具读取的契约**，翻译了会粘贴失败。`t(...)` 只负责**自然语言字段标签**。
 *
 * ## generic 只给通用参数，**不编造配置文件片段**（P11 §1.8 / §3.10 第 1 条）
 *
 * 未确认的客户端一律「协议 + 通用参数 + 一句以其官方文档为准」。编错的配置比没有说明更糟：
 * 用户会以为网关坏了。因此 generic 片段里**不出现** `config.toml` / `settings.json` 之类的
 * 具体文件名（断言会逐字检查）。
 *
 * ## 为什么函数仍叫 `gatewaySnippet`（而不是设计里的 `accessSnippet`）
 *
 * `P11-ARCH.md` §3.5 把它命名为 `accessSnippet`，但 `scripts/check-scope-registry.cjs` 的
 * P3 护栏要求两个接入指引都**逐字**调用 `gatewaySnippet(` —— 改名即变红。故保留此名、
 * **只扩展签名**（新增 `protocol`）。这是与设计的**有意偏差**，已在 T03 报告中说明。
 *
 * ## P3 合并的由来（保留：它是本模块存在的理由）
 *
 * 改造前 `components/gateway/integration-guide.tsx` 与 `trae-integration-guide.tsx`
 * **各写了一份** `TOOLS` / `ToolKey` / `snippetFor`，逐字重复 —— 而且**已经漂移出三个缺陷**：
 *
 * | # | WB 版 | 合并后 |
 * |:--|:---|:---|
 * | 1 | `cline` 的 `openAiModelId` **硬编码 `"GLM-5.3"`** | **`model` 参数** |
 * | 2 | `continue` 的 `model` **硬编码 `GLM-5.3`** | **`model` 参数** |
 * | 3 | `claude-code` 片段**缺 `ANTHROPIC_MODEL`** | **补上**（P11 继续保留） |
 *
 * 第 3 条：`ANTHROPIC_MODEL` 必须显式给 —— 模型名与 Claude 的默认模型名不同，不设的话客户端会
 * 请求一个网关上不存在的模型；报错发生在**客户端侧**，用户看到的只是「模型不可用」，
 * **很难联想到是这里少了一行**。
 *
 * ⚠️ **`model` 是必传字段而不是可选**：可选就会有人不传、然后退回硬编码 —— **那正是要修的缺陷**。
 */

import type { Translate } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";

/** 协议维度（P11 §3.18.2）：决定**端点路径**与**认证头**。 */
export type GatewayProtocolKey = "openai" | "anthropic";

/** 协议元数据。`endpoint` / `authHeader` 是**英文契约，不翻译**。 */
export interface GatewayProtocol {
  key: GatewayProtocolKey;
  /** 已 i18n 的展示标签键。 */
  labelKey: TranslationKey;
  /** 端点（英文契约）。 */
  endpoint: string;
  /** 认证头名（英文契约）。 */
  authHeader: string;
}

/** 协议元数据表（键是联合类型 ⇒ 查表必然命中）。 */
const PROTOCOLS: Record<GatewayProtocolKey, GatewayProtocol> = {
  openai: {
    key: "openai",
    labelKey: "shared.gateway.access.protocol.openai",
    endpoint: "POST /v1/chat/completions",
    authHeader: "Authorization: Bearer",
  },
  anthropic: {
    key: "anthropic",
    labelKey: "shared.gateway.access.protocol.anthropic",
    endpoint: "POST /v1/messages",
    authHeader: "x-api-key",
  },
};

/** 协议 Tab 的渲染顺序（OpenAI 在前，与原型一致）。 */
export const GATEWAY_PROTOCOLS: readonly GatewayProtocol[] = [PROTOCOLS.openai, PROTOCOLS.anthropic];

/** 取协议元数据（键是联合类型 ⇒ 无回落分支）。 */
export function protocolOf(key: GatewayProtocolKey): GatewayProtocol {
  return PROTOCOLS[key];
}

/** 工具定义：品牌名（Codex / opencode / Claude Code）保持英文；只有 generic 的标签走 i18n。 */
interface AccessToolDef {
  key: string;
  /** 固定展示名（品牌名，不翻译）。 */
  label?: string;
  /** 需要 i18n 的展示名（仅 generic）。 */
  labelKey?: TranslationKey;
}

/**
 * 该协议下**已知**的工具（D28：硬编码 + 词条；generic 的标签走 i18n）。
 *
 * ⚠️ 只列**能确认**的客户端；未确认的一律走 `generic`（协议 + 通用参数 + 以其官方文档为准）。
 */
const ACCESS_TOOLS: Record<GatewayProtocolKey, readonly AccessToolDef[]> = {
  openai: [
    { key: "codex", label: "Codex" },
    { key: "opencode", label: "opencode" },
    { key: "generic", labelKey: "shared.gateway.access.tool.genericOpenai" },
  ],
  anthropic: [
    { key: "claude-code", label: "Claude Code" },
    { key: "opencode", label: "opencode" },
    { key: "generic", labelKey: "shared.gateway.access.tool.genericAnthropic" },
  ],
};

/** 该协议下的工具 Tab（已 i18n 的标签）。 */
export function accessTools(
  t: Translate,
  protocol: GatewayProtocolKey,
): { key: string; label: string }[] {
  return ACCESS_TOOLS[protocol].map((tool) => ({
    key: tool.key,
    label: tool.labelKey ? t(tool.labelKey) : (tool.label ?? tool.key),
  }));
}

export interface AccessSnippetInput {
  t: Translate;
  protocol: GatewayProtocolKey;
  /** 工具 key（`codex` / `claude-code` / `opencode` / `generic`）。 */
  tool: string;
  /** 网关的 OpenAI 兼容 base（**含 `/v1`**）。 */
  baseUrl: string;
  /** **根地址**（去掉 `/v1`）—— Anthropic 系客户端会自己拼 `/v1/messages`。 */
  rootUrl: string;
  /** 已脱敏的 Key 前缀，或「请先创建 Key」的占位文案。 */
  key: string;
  /**
   * 示例模型名（**必传**）。
   *
   * ⚠️ 必须是 **`available && !disabled`** 的第一个（别拿一个被禁用的模型当例子）。
   * 必传而不是可选：可选就会有人不传、然后退回硬编码 —— 那正是 P3 修掉的缺陷。
   */
  model: string;
}

/**
 * 生成可粘贴的接入片段（**协议 × 工具**）。
 *
 * 认证头由 `protocol` 决定（Anthropic → `x-api-key`；OpenAI → `Authorization: Bearer`）；
 * `generic` 只给通用参数，**不编造配置文件片段**。
 */
export function gatewaySnippet({
  t,
  protocol,
  tool,
  baseUrl,
  rootUrl,
  key,
  model,
}: AccessSnippetInput): string {
  // Anthropic 系客户端自己拼 `/v1/messages` ⇒ 给**根地址**；OpenAI 系给含 `/v1` 的 base。
  const base = protocol === "anthropic" ? rootUrl : baseUrl;
  const authHeader = protocolOf(protocol).authHeader;

  switch (tool) {
    case "claude-code":
      // `ANTHROPIC_MODEL` 必须显式给（见文件头 P3 第 3 条缺陷）—— 不能因为「自动发现」就省掉。
      return [
        `export ANTHROPIC_BASE_URL=${rootUrl}`,
        `export ANTHROPIC_AUTH_TOKEN=${key}`,
        `export ANTHROPIC_MODEL=${model}`,
      ].join("\n");

    case "codex":
      // Codex 的 `model_provider`：`base_url` 指向本网关（OpenAI 兼容），`env_key` 指明 Key 的环境变量名。
      return [
        `# ${t("shared.gateway.access.codex.title")}`,
        `base_url = "${baseUrl}"`,
        `env_key  = "BUDDY_SWITCH_KEY"    # ${t("shared.gateway.access.codex.envKeyNote")}`,
        `# ${t("shared.gateway.access.codex.docNote")}`,
      ].join("\n");

    case "opencode":
      return [
        `# ${t("shared.gateway.access.opencode.title")}`,
        `baseURL = "${base}"`,
        `apiKey  = "${key}"`,
        `# ${t("shared.gateway.access.opencode.docNote")}`,
      ].join("\n");

    default:
      // generic：**只给通用参数**（Base URL + Key + 认证头 + 自动发现），
      // **绝不**编造具体配置文件片段（§1.8）。`Base URL` / `API Key` 是通用参数名（英文契约）。
      return [
        `# ${t(
          protocol === "anthropic"
            ? "shared.gateway.access.tool.genericAnthropic"
            : "shared.gateway.access.tool.genericOpenai",
        )}`,
        `Base URL : ${base}`,
        `API Key  : ${key}   (${t("shared.gateway.access.authHeaderLabel")}: ${authHeader})`,
        `${t("shared.gateway.snippet.model")} : ${t("shared.gateway.access.generic.modelHint", {
          url: `${rootUrl}/v1/models`,
        })}`,
      ].join("\n");
  }
}

export interface AccessCurlInput {
  protocol: GatewayProtocolKey;
  /** **根地址**（去掉 `/v1`）—— 端点路径取自 `protocolOf(protocol).endpoint`（**含** `/v1`）。 */
  rootUrl: string;
  /** 示例模型名（与接入片段同一取值）。 */
  model: string;
}

/**
 * 「原始请求（调试用）」curl 片段（原型 `buddy-switch-prototype.html:2903-2910` / `:2929-2930`）。
 *
 * 认证头由**协议**决定，与 `gatewaySnippet` **同源**（复用 `protocolOf(protocol).authHeader`）：
 * Anthropic → `x-api-key`；OpenAI → `Authorization: Bearer`。Key 用环境变量占位而不是脱敏前缀 ——
 * 那串带省略号（`sk-wb-…`），粘进终端只会报错。
 */
export function gatewayCurl({ protocol, rootUrl, model }: AccessCurlInput): string {
  const proto = protocolOf(protocol);
  const url = `${rootUrl}${proto.endpoint.replace(/^POST\s+/, "")}`;
  if (protocol === "anthropic") {
    return [
      `curl ${url} \\`,
      `  -H "${proto.authHeader}: $BUDDY_SWITCH_KEY" \\`,
      `  -H "anthropic-version: 2023-06-01" \\`,
      `  -d '{"model":"${model}","max_tokens":256,"messages":[{"role":"user","content":"hi"}]}'`,
    ].join("\n");
  }
  return [
    `curl ${url} \\`,
    `  -H "${proto.authHeader}: $BUDDY_SWITCH_KEY" \\`,
    `  -d '{"model":"${model}","messages":[{"role":"user","content":"hi"}]}'`,
  ].join("\n");
}
