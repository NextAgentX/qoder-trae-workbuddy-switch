import { useState } from "react";
import { Check, Copy } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { copyText } from "@/lib/clipboard";
import {
  GATEWAY_PROTOCOLS,
  accessTools,
  gatewayCurl,
  gatewaySnippet,
  protocolOf,
  type GatewayProtocolKey,
} from "@/lib/gateway-snippets";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { ApiKeyRecord, Region } from "@/lib/types";
import { useGatewayStore } from "@/stores/gateway";

/**
 * 接入说明（P11 §1.7）—— **协议 Tab × 工具 Tab** 两段式。
 *
 * ## 为什么从「一个工具 Tab」升级为「协议 × 工具」两段式
 *
 * 协议（OpenAI 兼容 / Anthropic 兼容）是**端点属性**，不是模型属性 —— 同一个模型两种协议
 * 都支持。但**端点路径**与**认证头**由协议决定：Anthropic 用 `x-api-key`、OpenAI 用
 * `Authorization: Bearer`。把协议做成**第一段 Tab**，认证头就**不可能给错**（给错 = 用户配不通）。
 * 只留一个工具 Tab 时，协议差异无处安放 —— 那正是改造前的缺陷。
 *
 * ## 工具清单：只列能确认的客户端
 *
 * `codex` / `claude-code` / `opencode` 是**能确认**的；未确认的一律走 `generic`
 * （协议 + 通用参数 + 以其官方文档为准）—— **不编造配置片段**（编错的配置比没有说明更糟）。
 * 工具清单硬编码在 `lib/gateway-snippets`（D28），本组件只负责渲染与取数。
 *
 * ## 示例模型取 `available && !disabled` 的第一个
 *
 * 别拿一个被禁用的模型当例子（用户照抄会扑空）。WB 的上游清单里即 `available = true`
 * （§1.3），因此这里取**第一个**模型；`disabled` 的过滤在 P11 的 T02 落地后接入
 * （本任务与 T01/T02 无耦合，故不引用尚未存在的 `disabled` 状态）。
 *
 * ## 版本由页面传入（受控）
 *
 * 版本（国内版 / 国际版）在**页头唯一入口**切换，本组件只按传入的版本取对应 Key
 * （`representativeKey`）。
 */

/** 取该 region 第一个启用中的 Key 前缀；无启用中的 Key 时返回 null，由渲染处给出占位提示。 */
function representativeKey(keys: ApiKeyRecord[], region: Region): string | null {
  const active = keys.find((key) => key.region === region && !key.revoked);
  return active ? `${active.prefix}…` : null;
}

export function IntegrationGuide({
  baseUrl,
  region,
  className,
}: {
  baseUrl: string;
  region: Region;
  className?: string;
}) {
  const t = useT();
  const keys = useGatewayStore((s) => s.keys);
  /* 模型名从网关 store 取 —— **修掉了原来硬编码 `GLM-5.3` 的缺陷**
     （模型一换，生成的片段就是错的）。取不到时给**占位**，不编造一个看起来可用的模型名。 */
  const catalog = useGatewayStore((s) => s.models[region]);
  const [protocol, setProtocol] = useState<GatewayProtocolKey>("openai");
  const [tool, setTool] = useState("codex");
  const [copied, setCopied] = useState(false);

  const tools = accessTools(t, protocol);
  /* 切协议后原工具可能不在新协议下（如 `codex` 不在 Anthropic 侧）⇒ 回落该协议的第一个工具。 */
  const activeTool = tools.some((item) => item.key === tool) ? tool : tools[0].key;

  const rootUrl = baseUrl.replace(/\/v1\/?$/, "");
  const key = representativeKey(keys, region) ?? t("wbStats.gateway.noKeyHint");
  const model = catalog?.models[0]?.id ?? t("shared.gateway.snippet.modelPlaceholder");
  const snippet = gatewaySnippet({ t, protocol, tool: activeTool, baseUrl, rootUrl, key, model });
  /* 「原始请求（调试用）」：认证头随协议 Tab 变化（Anthropic → x-api-key；OpenAI → Bearer）。 */
  const curl = gatewayCurl({ protocol, rootUrl, model });

  async function onCopy() {
    await copyText(snippet, t("wbStats.gateway.codeCopied"));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
    <Card className={cn("gap-0 py-0", className)}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
        {/* 版本不在这里显示也不在这里切：页头切换器是唯一入口（与 Trae 侧同口径）。 */}
        <span className="text-sm font-semibold">{t("wbStats.gateway.guide")}</span>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" onClick={() => void onCopy()}>
            {copied ? <Check /> : <Copy />}
            {t("wbStats.gateway.copyCode")}
          </Button>
        </div>
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
          {/* 端点说明：两种协议**都已支持**（§3.18.2，含流式）—— 只说事实，不承诺模型可用。 */}
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
          <p className="mt-1 text-xs text-muted-foreground">{t("wbStats.gateway.streamNote")}</p>
          {/* 自动发现说明（P10 §1.7）：模型清单由客户端从 `/v1/models` 自动读取，
              用户**不需要**在片段里手填模型名 —— 片段里的 `Model:` 只是「契约字段」，
              不是「必须自己查」的东西。只说事实，不承诺「清单里的模型一定可用」（§3.17.3 陷阱 3）。 */}
          <p className="mt-1 text-xs text-muted-foreground">{t("wbStats.gateway.autoDiscover")}</p>
          {/* 「原始请求（调试用）」（原型 `:2929-2930`）：方法 + URL + 协议对应的认证头，可直接粘贴核验。 */}
          <p className="mt-3 text-xs text-muted-foreground">{t("wbStats.gateway.rawRequest")}</p>
          <pre className="mt-1.5 min-w-0 overflow-x-auto rounded-[10px] bg-card-2 p-3 font-mono text-xs leading-6">
            {curl}
          </pre>
        </div>
      </Tabs>
    </Card>
  );
}
