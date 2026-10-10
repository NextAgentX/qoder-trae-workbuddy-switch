/**
 * 文案域：**请求代理（P12 · C 路）** —— English（键的权威之一）。
 *
 * 与 `proxy.zh.ts` **逐键对齐**（只留被引用的键）。
 * 归属文件：`src/components/gateway/proxy-panel.tsx`。
 */
export const en = {
  "proxy.title": "Request proxy",
  "proxy.state.off": "Off",
  /** ⚠️「Set up as guided」而非「Running」—— 应用**不校验**是否生效。 */
  "proxy.state.on": "Set up as guided",
  "proxy.state.unverified": "(the app does not verify it is in effect)",
  "proxy.toggle.enable": "Turn on",
  "proxy.toggle.disable": "Turn off",
  "proxy.restore": "Restore",

  // ★ E2: closed-state explanation. Without it, "Off" tells the user nothing about
  //   whether it is a product default or something they have to configure themselves.
  "proxy.offNotice.title": "Request proxy is not enabled",
  "proxy.offNotice.default": "The request proxy is off by default; no existing request is affected.",
  "proxy.offNotice.howTo":
    "To turn it on, configure a proxy in the client yourself, or point its base URL at the local gateway address.",

  // ★ E1: KPI row in the enabled state. "Persisted bodies" is always 0 — a promise
  //   (request bodies are never written to disk), not a measured number.
  "proxy.kpi.requests": "Proxied today",
  "proxy.kpi.switches": "Account switches today",
  "proxy.kpi.bodies": "Persisted bodies",
  "proxy.kpi.bodiesZero": "0 (never persisted)",

  "proxy.risk.title": "Before you turn it on",
  "proxy.risk.traffic": "Traffic goes through the local gateway",
  "proxy.risk.noBody": "Request bodies are not written to disk",

  "proxy.noHijack.title": "What we will NOT do",
  "proxy.noHijack.never": "Never hijack",
  "proxy.noHijack.hosts": "No hosts changes",
  "proxy.noHijack.dns": "No DNS hijacking",
  "proxy.noHijack.transparent": "No transparent proxy",

  "proxy.mode.label": "Interception",
  /** C 路 = base URL redirect —— **not** a MITM. */
  "proxy.mode.cRedirect": "Base URL redirect (route C)",
  "proxy.gateway.label": "Gateway address",

  "proxy.domains.title": "Allowlist",
  "proxy.domains.colDomain": "Domain",
  "proxy.domains.colAction": "Handling",
  "proxy.domains.actionProxied": "Via gateway",
  "proxy.domains.fallback": "All other domains",
  "proxy.domains.actionDirect": "Direct",

  // P12 T03: business paths the gateway actually forwards (**diagnostic**, not a user toggle).
  "proxy.bizPaths.title": "Business paths forwarded",
  "proxy.bizPaths.hint": "Only these paths go through the gateway; everything else is not forwarded (404).",

  "proxy.guide.title": "How to turn it on (one manual step)",
  "proxy.guide.why":
    "The client reads its proxy from its own settings file (not the system proxy, not an environment variable). The app cannot change it for you (and should not touch your settings).",
  "proxy.guide.envLabel": "Set in the client's settings",
  "proxy.guide.restart": "Restart the client for it to take effect.",
  "proxy.guide.copy": "Copy",
  "proxy.guide.copied": "Copied",
} as const;
