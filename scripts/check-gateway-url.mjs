#!/usr/bin/env node
/**
 * `src/lib/gateway-url.ts` 的**真实断言**（issue #165 门禁）。
 *
 * ## 为什么不用文本断言
 *
 * #165 的回归形状是「结构全对、只有拼串少一层 `/v1`」—— 文本断言（「文件里有没有
 * `baseUrl`」）看不出来。所以这里照抄 `check-credit-packs.mjs`：用 esbuild 把**生产代码**
 * 转译成 ESM、经 `data:` URL 导入，跑真断言；再补几条只能看源码的「用法」断言。
 *
 * ## 钉住五条
 *
 * 1. **`0.0.0.0` 归一回环**：监听通配地址不能当目标地址（局域网模式复制出去必须可用）；
 * 2. **`/v1` 只加一次**：后端 `base_url` 是根地址、兜底分支带 `/v1`，收敛后不得出现 `/v1/v1`；
 * 3. **两档语义分开**：根地址（Anthropic）与 OpenAI 基址（含 `/v1`）不得互相冒充；
 * 4. **端口缺失不伪造**：不许产出 `http://127.0.0.1:/v1` 这种照抄必失败的串；
 * 5. **全仓唯一入口**：`src/**` 里不许再手拼 `http://${host}:${port}`（只有本模块可以）。
 *
 * 用法：`node scripts/check-gateway-url.mjs`（已挂进 `npm run check:scope`）。
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function loadTs(relative) {
  const source = readFileSync(path.join(root, relative), "utf8");
  // ★ Windows：绝对路径必须转 `file://` URL，否则 ESM loader 把 `D:` 当成协议。
  const { transform } = await import(
    pathToFileURL(path.join(root, "node_modules/esbuild/lib/main.js")).href
  );
  const { code } = await transform(source, { loader: "ts", format: "esm" });
  return import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
}

const {
  GATEWAY_LOOPBACK,
  GATEWAY_LAN,
  gatewayDisplayHost,
  gatewayRootUrl,
  gatewayOpenAiBaseUrl,
  gatewayOpenAiBaseUrlFrom,
  stripV1Suffix,
} = await loadTs("src/lib/gateway-url.ts");

let failed = 0;
let passed = 0;
function check(name, fn) {
  try {
    const detail = fn();
    passed += 1;
    console.log(`  ✓ ${name}${detail ? ` — ${detail}` : ""}`);
  } catch (error) {
    failed += 1;
    console.error(`  ✗ ${name}\n      ${error instanceof Error ? error.message : String(error)}`);
  }
}
function eq(actual, expected, label) {
  if (actual !== expected) throw new Error(`${label}：实际 ${JSON.stringify(actual)}，应 ${JSON.stringify(expected)}`);
}

console.log("\n#165 · 网关对外地址（gateway-url）");

check("0.0.0.0 归一回环（监听地址不能当目标地址）", () => {
  eq(GATEWAY_LAN, "0.0.0.0", "LAN 常量");
  eq(GATEWAY_LOOPBACK, "127.0.0.1", "回环常量");
  eq(gatewayDisplayHost("0.0.0.0"), "127.0.0.1", "通配地址");
  eq(gatewayDisplayHost("  0.0.0.0  "), "127.0.0.1", "带空白的通配地址");
  eq(gatewayDisplayHost(""), "127.0.0.1", "空值");
  eq(gatewayDisplayHost(null), "127.0.0.1", "null");
  eq(gatewayDisplayHost("192.168.1.7"), "192.168.1.7", "具体地址保持原样");
  return "0.0.0.0 → 127.0.0.1";
});

check("两档语义分开：根地址不带 /v1、OpenAI 基址带 /v1", () => {
  eq(gatewayRootUrl("127.0.0.1", 57892), "http://127.0.0.1:57892", "根地址");
  eq(gatewayOpenAiBaseUrl("127.0.0.1", 57892), "http://127.0.0.1:57892/v1", "OpenAI 基址");
  if (gatewayRootUrl("127.0.0.1", 57892).includes("/v1")) throw new Error("根地址混入了 `/v1`");
  return "root=http://127.0.0.1:57892 · openai=…/v1";
});

check("局域网监听时接入地址仍是可用地址（不是 0.0.0.0）", () => {
  eq(gatewayOpenAiBaseUrl("0.0.0.0", 57892), "http://127.0.0.1:57892/v1", "局域网 + OpenAI 基址");
  return "0.0.0.0 → http://127.0.0.1:57892/v1";
});

check("/v1 只加一次：后端根地址与已带 /v1 的串都收敛成同一形态", () => {
  const fromRoot = gatewayOpenAiBaseUrlFrom("http://127.0.0.1:57891", "127.0.0.1", 57891);
  const fromV1 = gatewayOpenAiBaseUrlFrom("http://127.0.0.1:57891/v1", "127.0.0.1", 57891);
  const fromSlash = gatewayOpenAiBaseUrlFrom("http://127.0.0.1:57891/v1/", "127.0.0.1", 57891);
  eq(fromRoot, "http://127.0.0.1:57891/v1", "后端根地址（真实形态）");
  eq(fromV1, "http://127.0.0.1:57891/v1", "已带 /v1");
  eq(fromSlash, "http://127.0.0.1:57891/v1", "带尾斜杠");
  for (const value of [fromRoot, fromV1, fromSlash]) {
    if (value.includes("/v1/v1")) throw new Error(`拼出了 /v1/v1：${value}`);
  }
  return "两条分支同形态";
});

check("后端无 baseUrl 时走自拼分支（同样带 /v1 且归一回环）", () => {
  eq(gatewayOpenAiBaseUrlFrom(null, "0.0.0.0", 57892), "http://127.0.0.1:57892/v1", "null baseUrl");
  eq(gatewayOpenAiBaseUrlFrom("   ", "127.0.0.1", 57892), "http://127.0.0.1:57892/v1", "空白 baseUrl");
  return "兜底分支与主路径同形态";
});

check("端口缺失不伪造 `host:/v1`", () => {
  eq(gatewayRootUrl("127.0.0.1", null), "http://127.0.0.1", "无端口根地址");
  eq(gatewayRootUrl("127.0.0.1", undefined), "http://127.0.0.1", "undefined 端口");
  const url = gatewayOpenAiBaseUrl("127.0.0.1", null);
  // 空端口会拼成 `http://host:/v1`（注意 `://` 本身不是空端口，判据要盯 `:<slash>`）。
  if (/:[^/]*:(\/|$)/.test(url.replace(/^http:\/\//, ""))) {
    throw new Error(`端口缺失时拼出了 ${url}（照抄必失败）`);
  }
  return "无端口 → http://127.0.0.1";
});

check("stripV1Suffix 幂等且只削一层", () => {
  eq(stripV1Suffix("http://h:1/v1"), "http://h:1", "削 /v1");
  eq(stripV1Suffix("http://h:1/v1/"), "http://h:1", "削 /v1/");
  eq(stripV1Suffix("http://h:1"), "http://h:1", "根地址不变");
  eq(stripV1Suffix(stripV1Suffix("http://h:1/v1")), "http://h:1", "幂等");
  return "幂等";
});

/* ── 用法断言（只能看源码）────────────────────────────────────────────── */

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const abs = path.join(dir, entry);
    if (statSync(abs).isDirectory()) walk(abs, out);
    else if (/\.(ts|tsx)$/.test(entry)) out.push(abs);
  }
  return out;
}

check("全仓唯一入口：src/** 不许再手拼 http://${host}:${port}", () => {
  const offenders = [];
  for (const file of walk(path.join(root, "src"))) {
    if (path.basename(file) === "gateway-url.ts") continue;
    const source = readFileSync(file, "utf8");
    // 只看代码行：注释里写「禁止手拼 `http://${host}:${port}`」不算违规。
    const code = source
      .split("\n")
      .filter((line) => !/^\s*(\*|\/\/|\/\*)/.test(line))
      .join("\n");
    if (/`http:\/\/\$\{/.test(code)) offenders.push(path.relative(root, file));
  }
  if (offenders.length > 0) {
    throw new Error(`这些文件仍在手拼网关地址（应改用 gateway-url.ts）：${offenders.join("、")}`);
  }
  return "无手拼";
});

check("统一网关页：概览行复制的是含 /v1 的串，说明卡的 Anthropic 侧是根地址", () => {
  const rel = "src/components/gateway/unified-gateway-view.tsx";
  const source = readFileSync(path.join(root, rel), "utf8");
  if (!source.includes("gatewayRootUrl(")) throw new Error("未使用 `gatewayRootUrl`（可能又手拼了）");
  if (!/const baseUrl = `\$\{rootUrl\}\/v1`/.test(source)) {
    throw new Error("`baseUrl` 不是「rootUrl + /v1」—— 概览行会再次给出 404 地址");
  }
  if (!source.includes("copyText(baseUrl")) throw new Error("复制出口没用 `baseUrl`（少了 `/v1`）");
  if (!source.includes("{ openai: baseUrl, anthropic: rootUrl }")) {
    throw new Error("接入说明的 openai / anthropic 取值错位（应分别为 baseUrl / rootUrl）");
  }
  if (/\$\{baseUrl\}\/v1/.test(source)) throw new Error("curl 样例又用 `baseUrl` 拼 `/v1`（会得到 /v1/v1）");
  return "复制 = baseUrl（含 /v1）· 说明卡 openai=baseUrl / anthropic=rootUrl";
});

check("三个渠道页都经 gateway-url 派生，不再各自手拼", () => {
  for (const rel of [
    "src/pages/ApiServicePage.tsx",
    "src/pages/TraeApiServicePage.tsx",
    "src/pages/QoderApiServicePage.tsx",
  ]) {
    const source = readFileSync(path.join(root, rel), "utf8");
    const viaHelper =
      source.includes("resolveGatewayBaseUrl(") || source.includes("gatewayOpenAiBaseUrl(");
    if (!viaHelper) throw new Error(`${rel} 未使用统一派生入口`);
  }
  return "三页均经 gateway-url";
});

console.log(`\n${failed === 0 ? "✅" : "❌"} gateway-url：${passed} 通过 / ${failed} 失败\n`);
if (failed > 0) process.exit(1);
