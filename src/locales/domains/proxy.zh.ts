/**
 * 文案域：**请求代理（P12 · C 路）** —— 中文（键的权威之一）。
 *
 * 归属文件：`src/components/gateway/proxy-panel.tsx`。
 *
 * ## 为什么单独成域，而不并入 `wbStats`
 *
 * 请求代理有**一整套自己的语义**：默认关 / 风险声明 / 不劫持声明 / 白名单 / 拦截方式 /
 * 一键还原 —— 且**必须如实**（应用**不校验**代理是否生效）。混进 `wbStats` 会让
 * 「哪些键属于代理」不可查。原型断言见 `verify-behaviors.cjs:1306-1357`。
 */
export const zh = {
  "proxy.title": "请求代理",
  "proxy.state.off": "已关闭",
  /** ⚠️ 用「已按引导设置」而非「运行中」—— 应用**不校验**是否生效（见 `proxy.state.unverified`）。 */
  "proxy.state.on": "已按引导设置",
  "proxy.state.unverified": "（应用不校验是否已生效）",
  "proxy.toggle.enable": "开启",
  "proxy.toggle.disable": "关闭",
  "proxy.restore": "一键还原",

  "proxy.risk.title": "开启前请知悉",
  "proxy.risk.traffic": "流量会经过本机网关",
  "proxy.risk.noBody": "不落盘请求正文",

  "proxy.noHijack.title": "我们不会做这些",
  "proxy.noHijack.never": "绝不劫持",
  "proxy.noHijack.hosts": "不改 hosts",
  "proxy.noHijack.dns": "不做 DNS 劫持",
  "proxy.noHijack.transparent": "不做透明代理",

  "proxy.mode.label": "拦截方式",
  /** C 路 = base URL 重定向 —— **不是「中间人」**，不许含糊。 */
  "proxy.mode.cRedirect": "base URL 重定向（C 路）",
  "proxy.gateway.label": "网关地址",

  "proxy.domains.title": "白名单",
  "proxy.domains.colDomain": "域名",
  "proxy.domains.colAction": "处理",
  "proxy.domains.actionProxied": "经网关",
  "proxy.domains.fallback": "其它全部域名",
  "proxy.domains.actionDirect": "直连",

  // P12 T03：网关**实际转发**的业务路径（**诊断信息**，不是给用户选的开关）。
  "proxy.bizPaths.title": "网关转发的业务路径",
  "proxy.bizPaths.hint": "只有这些路径会经网关；其余一律不转发（404）。",

  "proxy.guide.title": "怎么开启（需要你手动设一步）",
  "proxy.guide.why":
    "客户端从自己的设置文件读代理（不是系统代理，也不是环境变量），应用无法替你改（也不该改你的设置）。",
  "proxy.guide.envLabel": "在客户端设置里填",
  "proxy.guide.restart": "设好后需要重启客户端才生效。",
  "proxy.guide.copy": "复制",
  "proxy.guide.copied": "已复制",
} as const;
