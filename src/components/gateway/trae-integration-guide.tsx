import { useState } from "react";
import { Check, Copy } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { copyText } from "@/lib/clipboard";
import {
  GATEWAY_PROTOCOLS,
  accessTools,
  gatewaySnippet,
  protocolOf,
  type GatewayProtocolKey,
} from "@/lib/gateway-snippets";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * Trae 接入说明（P11 §1.7）—— **协议 Tab × 工具 Tab** 两段式，与 WorkBuddy 侧同构。
 *
 * ## 为什么升级为两段式（与 WB 侧同一理由）
 *
 * 协议是**端点属性**（不是模型属性）：Anthropic 用 `x-api-key`、OpenAI 用
 * `Authorization: Bearer`。把协议做成第一段 Tab，认证头就不可能给错。
 *
 * ## 与 WorkBuddy 版的差别：**单条 Base URL**，没有「按版本」双条目
 *
 * WorkBuddy 版顶部有一排 region Tabs（国内版 / 国际版各自一个 Base URL）；
 * Trae 没有 region —— 一个网关、一条上游、一个 Base URL，因此这里只展示一条。
 * 把 `REGIONS.map` 搬过来会造出一个「切了也没差别」的假维度。
 *
 * `keyPrefix` 由页面从网关状态（`apiKeyPrefix`，最近一把未吊销 Key 的前缀）传入；
 * 为空时给出「请先在上方创建 Key」的占位，而不是伪造一段看起来可用的 Key。
 *
 * `model` 由页面传入（Trae 的默认模型）—— 不在这里硬编码模型名（那正是 P3 修掉的缺陷）。
 */

export function TraeIntegrationGuide({
  baseUrl,
  model,
  keyPrefix,
  className,
}: {
  baseUrl: string;
  model: string;
  keyPrefix?: string;
  className?: string;
}) {
  const t = useT();
  const [protocol, setProtocol] = useState<GatewayProtocolKey>("openai");
  const [tool, setTool] = useState("codex");
  const [copied, setCopied] = useState(false);

  const tools = accessTools(t, protocol);
  /* 切协议后原工具可能不在新协议下（如 `codex` 不在 Anthropic 侧）⇒ 回落该协议的第一个工具。 */
  const activeTool = tools.some((item) => item.key === tool) ? tool : tools[0].key;

  // Anthropic 系客户端（Claude Code）自己会拼 `/v1/messages`，因此要给它**根地址**。
  // 与 WorkBuddy 侧的 `integration-guide.tsx` 同算法。
  const rootUrl = baseUrl.replace(/\/v1\/?$/, "");
  const key = keyPrefix ? `${keyPrefix}…` : t("trae.gateway.guide.noKey");
  const snippet = gatewaySnippet({ t, protocol, tool: activeTool, baseUrl, rootUrl, key, model });

  async function onCopy() {
    await copyText(snippet, t("trae.gateway.guide.copied"));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
    <Card className={cn("gap-0 py-0", className)}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
        <span className="text-sm font-semibold">{t("trae.gateway.guide.title")}</span>
        <Button variant="outline" size="sm" onClick={() => void onCopy()}>
          {copied ? <Check /> : <Copy />}
          {t("trae.gateway.guide.copy")}
        </Button>
      </div>

      {/* 第一段：协议 —— 决定端点路径与认证头 */}
      <Tabs value={protocol} onValueChange={(value) => setProtocol(value as GatewayProtocolKey)}>
        <div className="border-b border-border/60 px-5 py-2">
          <TabsList>
            {GATEWAY_PROTOCOLS.map((item) => (
              <TabsTrigger key={item.key} value={item.key} data-act={`gw-api|${item.key}`}>
                {t(item.labelKey)}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>
      </Tabs>

      {/* 第二段：工具 —— 该协议下的已知客户端 */}
      <Tabs value={activeTool} onValueChange={setTool}>
        <div className="px-5 py-2">
          <TabsList className="flex-wrap">
            {tools.map((item) => (
              <TabsTrigger key={item.key} value={item.key} data-act={`gw-tool|${item.key}`}>
                {item.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>
        <div className="px-5 pb-4">
          <pre className="min-w-0 overflow-x-auto rounded-lg border border-border bg-muted/40 p-4 font-mono text-xs leading-6">
            {snippet}
          </pre>
          {/* 端点说明：两种协议**都已支持**（§3.18.2，含流式）。 */}
          <p className="mt-2 text-xs text-muted-foreground">
            {t("shared.gateway.access.endpointNote", {
              openai: protocolOf("openai").endpoint,
              anthropic: protocolOf("anthropic").endpoint,
            })}
          </p>
          {/* 认证头：由**协议**决定（Anthropic → x-api-key；OpenAI → Bearer）。 */}
          <p className="mt-1 text-xs text-muted-foreground">
            {t("shared.gateway.access.authNote", { header: protocolOf(protocol).authHeader })}
          </p>
          <p className="mt-1 text-xs text-muted-foreground">{t("trae.gateway.guide.streamNote")}</p>
          {/* 自动发现说明（P10 §1.7）：与本仓 WB 侧 `integration-guide.tsx` 同构 ——
              外部客户端会从本网关的 `/v1/models` 自动读取清单，不需要手填模型名。 */}
          <p className="mt-1 text-xs text-muted-foreground">
            {t("trae.gateway.guide.autoDiscover")}
          </p>
        </div>
      </Tabs>
    </Card>
  );
}
