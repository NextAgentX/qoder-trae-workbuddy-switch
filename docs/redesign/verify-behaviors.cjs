/* 原型「行为」验收套件 —— 与 verify-demo.cjs 分工：
 *   verify-demo.cjs      = 契约套件（骨架、导航、搜索/IME、能力开关、命令面板…）
 *   verify-behaviors.cjs = 行为套件（本文件：数据自洽、真实聚合、错误态、部分失败、
 *                          向导会话树、能力门控、账号池排他、按作用域取值…）
 *
 * 立场：**不信任实现者的自述**。所有断言都直接打开页面读真实 DOM 与真实数据对象，
 * 不看任何汇报文字。
 *
 * 依赖 jsdom（装在隔离工作区，不要往项目里装）：
 *   export NODE_PATH="<managed node workspace>/node_modules"
 *   node docs/redesign/verify-behaviors.cjs
 *
 * 变异模式（证明断言不是恒真的空转）：
 *   node docs/redesign/verify-behaviors.cjs --mutate=merge|snapshot|selectall
 *   变异只写 os.tmpdir() 里的**副本**，交付目录下的原文件永不被写。
 *   变异模式下**全绿即为失败**（说明断言打不红）。
 */
const path = require("path");
const fs = require("fs");
const os = require("os");
const { JSDOM } = require("jsdom");

const BASE = __dirname;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];

/* ─────────── 变异注入 ─────────── */
const MUT = (process.argv.find((a) => a.startsWith("--mutate")) || "").replace("--mutate", "").replace(/^=/, "");
const MUTATIONS = {
  merge: {
    desc: "把 merge 退回假开关（只改标签、不换数据源）",
    file: "demo-app.js",
    from: "if (!state.merge) return USAGE[state.scope];",
    to: "return USAGE[state.scope]; /* MUTATED: merge 假开关 */",
  },
  snapshot: {
    desc: "去掉快照的能力门控（WorkBuddy 也会出现快照）",
    file: "demo-app.js",
    from: '${p.capabilities.includes("snapshot") ? snapshotsHtml(a) : ""}',
    to: "${snapshotsHtml(a)} /* MUTATED: 忽略能力开关 */",
  },
  selectall: {
    desc: "退回 D1 缺陷（首屏全选按钮语义错，需点两次）",
    file: "demo-app.js",
    from: 'if (state.dest === "accounts") renderAccountList();',
    to: "/* MUTATED: 不再同步全选按钮 */",
  },
  permretry: {
    desc: "退回 perm-retry 死按钮（处理器读错参数位：a1 → a2）",
    file: "demo-app.js",
    /* 锚**处理器**而不是渲染端：原缺陷的成因是「渲染只带一参、处理器却读 a2」，
       往渲染端加尾参不会让 a1 失效 ⇒ 变异不成立。 */
    from: "const a = curScope().accounts.find((x) => x.id === a1);",
    to: "const a = curScope().accounts.find((x) => x.id === a2); /* MUTATED: 读错参数位 */",
  },
  progbind: {
    desc: "退回单绑定模型（切一个程序位会清掉本账号其他程序位）",
    file: "demo-app.js",
    from: "      acc.programs[a2] = true;\n      curScope().accounts.forEach((o) => { if (o.id !== acc.id) o.programs[a2] = false; });",
    to: "      p.programs.forEach((x) => { acc.programs[x.id] = x.id === a2; }); /* MUTATED: 单绑定 */",
  },
  packs: {
    desc: "退回资源包不去重（同一账号出现重名包）",
    file: "demo-data.js",
    from: 'const names = _pick(["新手礼包", "月度订阅", "活动赠送", "团队共享", "年度包"], n, rnd);',
    to: 'const names = Array.from({ length: n }, () => ["新手礼包", "月度订阅", "活动赠送", "团队共享", "年度包"][rnd() % 5]); /* MUTATED: 不去重 */',
  },
  chartsegs: {
    desc: "堆叠柱只画第一个模型（图表偷懒，与数据打架）",
    file: "demo-app.js",
    from: "      const segs = series.map((s, k) => {",
    to: "      const segs = series.slice(0, 1).map((s, k) => { /* MUTATED: 只画一段 */",
  },
  mergeext: {
    desc: "合并只影响 days、不聚合图表数据（回到「切了开关只有一处变」）",
    file: "demo-app.js",
    from: "return { days, models, sessions, credits, ext: mergeExt(ids) };",
    to: "return { days, models, sessions, credits, ext: USAGE[state.scope].ext }; /* MUTATED */",
  },
  baselinen0: {
    desc: "余额曲线退回锚 0（高位小幅波动被压成直线）",
    file: "demo-app.js",
    from: '${areaChart(bal, 170, `积分余额变化曲线，${scopeName}，${rangeName}`, { anchorZero: false })}',
    to: '${areaChart(bal, 170, `积分余额变化曲线，${scopeName}，${rangeName}`)}',
  },
  /* ── 全局备份与恢复（§3.14）── */
  bkcreds: {
    desc: "凭据开关默认打开（退回不安全默认）",
    file: "demo-app.js",
    from: "creds: false,",
    to: "creds: true, /* MUTATED: 凭据默认开 */",
  },
  bkest: {
    desc: "备份预估大小写死（不随勾选变化，退化成装饰数字）",
    file: "demo-app.js",
    from: "if (p.accounts) { accounts += s.accounts; bytes += s.accounts * BACKUP_BYTES.account; }",
    to: "if (p.accounts) { /* MUTATED: 预估写死 */ }",
  },
  bkpreview: {
    desc: "点「选择备份文件」跳过预览、直接落盘",
    file: "demo-app.js",
    from: 'case "bk-import": stop(); state.backup.preview = true; state.backup.result = null; renderContent(); break;',
    to: 'case "bk-import": stop(); state.backup.preview = false; state.backup.result = null; renderContent(); break; /* MUTATED: 跳过预览 */',
  },
  /* ── 会话工作台（§3.15）── */
  sessfilter: {
    desc: "状态筛选渲染全部选项（退回「点了永远是空」的假选项）",
    file: "demo-app.js",
    from: "    .filter(([v]) => v === \"all\" || present.has(v));",
    to: "    ;/* MUTATED: 不过滤假选项 */",
  },
  sesssync: {
    desc: "分叉组的按钮退回「同步到全部副本」（静默覆盖，不再要求先选保留哪一份）",
    file: "demo-app.js",
    /* 打**按钮那一行**：第一次写成替换整个三元表达式，把外层模板字符串提前闭合 ⇒
       语法错误而非行为缺陷（跑出「3 passed / 3 failed」才发现）。 */
    from: '"sess-unify|${g.key}"',
    to: '"sess-sync|${g.key}"<!-- MUTATED -->',
  },
  sessbadge: {
    desc: "侧栏会话徽章用「总组数」而不是「待同步数」（失去提醒意义）",
    file: "demo-app.js",
    from: "sessions: sessionPendingCount() ? { n: sessionPendingCount(), warn: true } : null,",
    to: "sessions: { n: SESSION_GROUPS.length, warn: false }, /* MUTATED: 徽章用总数 */",
  },
  /* ── 网关接入与模型识别（§3.17）── */
  catnote: {
    desc: "来源条不显示回落原因（只剩一个灰徽标，用户不知道是加载中还是拉不到）",
    file: "demo-app.js",
    /* 只改条件、不动结构：替换整段会把外层模板字符串提前闭合 ⇒ 语法错误而非行为缺陷。 */
    from: "${c.note ? ",
    to: "${false ? /* MUTATED: 不显示原因 */ ",
  },
  catlabel: {
    desc: "把「内置」兜底清单伪装成「实时」（用户照着清单调用会扑空）",
    file: "demo-app.js",
    from: 'builtin: { label: "内置", cls: "bad",',
    to: 'builtin: { label: "实时", cls: "ok", /* MUTATED: 伪装 */',
  },
  catscan: {
    desc: "点「重新识别」不进入识别中态（按钮可重复点、无反馈）",
    file: "demo-app.js",
    from: "      state.gwScanning = true;",
    to: "      state.gwScanning = false; /* MUTATED: 不进入识别中 */",
  },
  /* ── 模型禁用与接入说明（§3.18）── */
  mdserved: {
    desc: "暴露计数忽略「用户禁用」（禁用变成只改 UI 的假开关）",
    file: "demo-app.js",
    from: "const served = g.models.filter((m) => m.available && !m.disabled).length;",
    to: "const served = g.models.filter((m) => m.available).length; /* MUTATED: 假开关 */",
  },
  mdauth: {
    desc: "Anthropic 的 curl 用错认证头（退回 Bearer）",
    file: "demo-app.js",
    from: '-H "x-api-key:',
    to: '-H "Authorization: Bearer /* MUTATED */',
  },
  mdtool: {
    desc: "接入说明的工具 Tab 失效（永远显示通用片段）",
    file: "demo-app.js",
    from: "  const tool = tools.some(([id]) => id === state.gwTool) ? state.gwTool : tools[0][0];",
    to: '  const tool = "generic"; /* MUTATED: 工具 Tab 失效 */',
  },
  /* ── 桌面悬浮窗与脱敏（§3.16 / §3.3）── */
  cpwrite: {
    desc: "悬浮窗里出现写操作（把「回到会话」换成「切换账号」，违反只读）",
    file: "demo-app.js",
    from: 'data-act="cp-jump|${c.id}"',
    to: 'data-act="switch|${c.id}" /* MUTATED: 写操作混进只读浮层 */',
  },
  cpblur: {
    desc: "悬浮窗标题不再模糊（截图 / 投屏会泄露完整会话标题）",
    file: "demo.css",
    from: "filter: blur(3.2px);",
    to: "/* MUTATED: 不模糊 */",
  },
  cpmask: {
    desc: "账号卡直接显示完整邮箱（脱敏失效）",
    file: "demo-app.js",
    from: '<span>${esc(maskEmail(a.email) || a.remark || "—")}</span>',
    to: '<span>${esc(a.email || a.remark || "—")}</span> /* MUTATED: 不脱敏 */',
  },
  /* ── 会话能力的平台边界（§3.15）── */
  navcap: {
    desc: "侧栏忽略平台能力（Trae 也渲染「会话」入口）",
    file: "demo-app.js",
    from: "  const dests = DESTINATIONS.filter((d) => !d.cap || p.capabilities.includes(d.cap));",
    to: "  const dests = DESTINATIONS; /* MUTATED: 忽略平台能力 */",
  },
  sesscap: {
    desc: "会话页忽略能力检查（Trae 也列出 WorkBuddy 的会话，让人以为复制过来了）",
    file: "demo-app.js",
    from: '  if (!curPlatform().capabilities.includes("sessionCopy")) {',
    to: "  if (false) { /* MUTATED: 忽略能力检查 */",
  },
  tabdegen: {
    desc: "客户端只有 1 项时也渲染 Tab（单选项的「切换器」是假控件）",
    file: "demo-app.js",
    from: "SESSION_CLIENTS.length > 1",
    to: "SESSION_CLIENTS.length >= 1 /* MUTATED: 不退化 */",
  },
  plannedmix: {
    desc: "把「计划新增」当成「本机没装」处理（引导用户去装一个本仓还不支持的客户端）",
    file: "demo-app.js",
    from: "          const title = pg.planned",
    to: "          const title = false /* MUTATED: 不区分 planned */",
  },
  /* ── 请求代理（§3.19 / P12）── */
  proxydef: {
    desc: "请求代理默认开启（违反 D29「默认关」）",
    file: "demo-app.js",
    from: "  proxyOn: false,",
    to: "  proxyOn: true, /* MUTATED: 违反默认关 */",
  },
  proxyhosts: {
    desc: "抹掉「不改 hosts / 不做 DNS / 不做透明代理」的声明（把劫持说成「网络优化」）",
    file: "demo-app.js",
    from: "          <b>绝不劫持</b>：不改 hosts、不做 DNS 劫持、不做透明代理 ——",
    to: "          <b>网络优化</b>：自动接管系统解析以提升速度 ——",
  },
  proxyall: {
    desc: "白名单外的域名也代理（违反「只代理白名单域」）",
    file: "demo-app.js",
    from: '<tr><td class="muted">其它全部域名</td><td class="muted">—</td><td><span class="badge">直连</span></td></tr>',
    to: '<tr><td class="muted">其它全部域名</td><td class="muted">—</td><td><span class="badge">也代理</span></td></tr>',
  },
};
if (MUT) {
  if (!MUTATIONS[MUT]) { console.error("未知变异：" + MUT + "（可选 " + Object.keys(MUTATIONS).join(" / ") + "）"); process.exit(2); }
}
let DIR = BASE;
if (MUT) {
  const m = MUTATIONS[MUT];
  DIR = fs.mkdtempSync(path.join(os.tmpdir(), "bsdemo-b-"));
  for (const f of fs.readdirSync(BASE)) if (fs.statSync(path.join(BASE, f)).isFile()) fs.copyFileSync(path.join(BASE, f), path.join(DIR, f));
  const p = path.join(DIR, m.file);
  const src = fs.readFileSync(p, "utf8");
  /* ★ 锚点比较必须与**换行符无关**：本仓文件是 CRLF，而锚点字符串里的 \n 是 LF，
     直接 includes 会**静默不匹配** ⇒ 变异没被注入 ⇒ 却因别的原因变红 ⇒ 假验证。
     （2026-10-01 真踩到：`progbind` 因此失效过一次，靠「锚点不匹配必须退出」这条才暴露。）
     修法：两侧都归一成 LF 再比，写回时还原原文件的换行风格。 */
  const eol = src.includes("\r\n") ? "\r\n" : "\n";
  const norm = src.replace(/\r\n/g, "\n");
  if (!norm.includes(m.from)) { console.error("变异注入失败：锚点不匹配 → " + m.from); process.exit(2); }
  fs.writeFileSync(p, norm.replace(m.from, m.to).replace(/\n/g, eol));
  console.log(`【变异注入】${m.desc}\n`);
}
const check = (name, fn) => {
  try { const r = fn(); results.push({ name, ok: r !== false, detail: r === true || r === undefined ? "" : String(r) }); }
  catch (e) { results.push({ name, ok: false, detail: e.message }); }
};

async function boot(query) {
  const dom = await JSDOM.fromFile(path.join(DIR, "index.html"), {
    runScripts: "dangerously", resources: "usable", pretendToBeVisual: true,
    url: "file://" + path.join(DIR, "index.html").replace(/\\/g, "/") + (query ? "?" + query : ""),
  });
  await new Promise((r) => dom.window.addEventListener("load", r));
  await sleep(80);
  return dom;
}

(async () => {
  try {
  /* ── 数据层：P0-2 今日 Token 自洽 + 死字段已删 ── */
  const dom0 = await boot("dest=overview&hint=0");
  const ev = (expr) => dom0.window.eval(expr);

  check("P0-2 各作用域 days 末值 = 账号 todayTokens 合计", () => {
    const expect = { "wb-cn": 128700, "wb-global": 18400, "trae-cn": 96200, "trae-global": 0 };
    const bad = [];
    for (const id of Object.keys(expect)) {
      const last = ev(`USAGE[${JSON.stringify(id)}].days[USAGE[${JSON.stringify(id)}].days.length-1].value`);
      const sum = ev(`SCOPES[${JSON.stringify(id)}].accounts.reduce((s,a)=>s+a.todayTokens,0)`);
      if (last !== expect[id]) bad.push(`${id}: days末值=${last} 期望${expect[id]}`);
      if (sum !== expect[id]) bad.push(`${id}: 账号合计=${sum} 期望${expect[id]}`);
    }
    if (bad.length) throw new Error(bad.join(" | "));
    return true;
  });

  check("P0-2 scope 级 todayTokens 死字段已删除", () => {
    const alive = ["wb-cn", "wb-global", "trae-cn", "trae-global"].filter((id) => ev(`SCOPES[${JSON.stringify(id)}].todayTokens !== undefined`));
    if (alive.length) throw new Error("仍存在：" + alive.join(","));
    return true;
  });

  check("概览「今日 Token」= days 末值（不再打架）", () => {
    const tile = [...dom0.window.document.querySelectorAll(".stat")].find((n) => n.textContent.includes("今日 Token"));
    if (!tile) throw new Error("找不到今日 Token 统计块");
    const shown = tile.querySelector(".v").textContent.trim();
    const expect = ev("USAGE[SCOPE_ID].days[USAGE[SCOPE_ID].days.length-1].value".replace(/SCOPE_ID/g, "state.scope"));
    const fmtTok = (n) => (n >= 1e6 ? (n / 1e6).toFixed(1) + "M" : n >= 1e3 ? (n / 1e3).toFixed(1) + "K" : String(n || 0));
    if (shown !== fmtTok(expect)) throw new Error(`显示 ${shown}，期望 ${fmtTok(expect)}`);
    return true;
  });

  check("「对比昨日」不再是硬编码 +12%", () => {
    const html = dom0.window.document.querySelector("#content").innerHTML;
    if (html.includes("+12%")) throw new Error("仍是硬编码 +12%");
    return true;
  });

  /* ── P0-1 merge 真实聚合 ── */
  const dom1 = await boot("dest=usage&usage=trend&range=7&hint=0");
  const w = dom1.window;
  const totalOf = (d) => {
    const t = [...d.querySelectorAll(".stat")].find((n) => n.textContent.includes("区间总 Token"));
    return t ? t.querySelector(".v").textContent.trim() : null;
  };
  const single = totalOf(w.document);
  check("P0-1 合并前：区间总 Token 存在", () => single || (() => { throw new Error("读不到区间总 Token"); })());

  w.document.querySelector('[data-act="merge|1"]').click();
  await sleep(60);
  const merged = totalOf(w.document);
  check("P0-1 点「合并全部作用域」后数字真的变了", () => {
    if (merged === single) throw new Error(`数字没变：${single} → ${merged}（假开关未修好）`);
    return `${single} → ${merged}`;
  });
  check("P0-1 合并值 = 各作用域逐日求和（精确校验）", () => {
    const fmtTok = (n) => (n >= 1e6 ? (n / 1e6).toFixed(1) + "M" : n >= 1e3 ? (n / 1e3).toFixed(1) + "K" : String(n || 0));
    const expect = ev.call(null, "0") === undefined ? null : (() => {
      const ids = ["wb-cn", "wb-global", "trae-cn", "trae-global"];
      let sum = 0;
      for (const id of ids) {
        const days = w.eval(`USAGE[${JSON.stringify(id)}].days`);
        for (const d of days.slice(-7)) sum += d.value;
      }
      return sum;
    })();
    if (merged !== fmtTok(expect)) throw new Error(`显示 ${merged}，期望 ${fmtTok(expect)}`);
    return true;
  });
  check("P0-1 合并态下模型 Tab 也跟着变（不只是趋势图）", () => {
    const before = w.document.querySelector("#content").innerHTML.length;
    w.document.querySelector('[data-act="usage-tab|models"]').click();
    const html = w.document.querySelector("#content").innerHTML;
    if (!html.includes("合并全部作用域") && !html.includes("合并")) throw new Error("模型 Tab 没有合并口径提示");
    return true;
  });

  /* ── P0-3 死按钮 ── */
  const dom2 = await boot("hint=0");
  const d2 = dom2.window.document;
  check("P0-3「检查更新」不再是死按钮（打开更新抽屉）", () => {
    const b = d2.querySelector('[data-act="check-update"]');
    if (!b) throw new Error("没有 data-act=check-update");
    b.click();
    const drawer = d2.querySelector("#drawer");
    const toast = d2.querySelector("#toasts").textContent.trim();
    if (!drawer.classList.contains("open") && !toast) throw new Error("点了没有任何反馈");
    if (!drawer.classList.contains("open")) throw new Error("没有打开更新抽屉");
    const body = d2.querySelector("#drawer-body").textContent;
    if (!/v\d{4}\.\d+\.\d+/.test(body)) throw new Error("抽屉里没有版本号");
    return d2.querySelector("#drawer-title").textContent;
  });

  /* ── P0-4 假承诺 ── */
  const dom3 = await boot("dest=accounts&hint=0");
  const d3 = dom3.window.document;
  d3.querySelector('[data-act="acct-prog|a3|cli"]').click();
  await sleep(40);
  check("P0-4a 未检测程序位：给出真实可点的「去设置」入口", () => {
    const toast = d3.querySelector("#toasts");
    const btn = toast.querySelector('[data-act]');
    if (!btn) throw new Error("toast 里没有可点入口，仍是空口承诺");
    btn.click();
    if (!d3.querySelector("#content").textContent.includes("权限与诊断")) throw new Error("点了没跳到权限与诊断");
    return "已跳转";
  });

  const dom4 = await boot("dest=accounts&open=account:a1&hint=0");
  const d4 = dom4.window.document;
  check("P0-4b 备注是真的内联编辑（不是 toast 桩）", () => {
    const btn = d4.querySelector('[data-act^="act|remark"]');
    if (!btn) throw new Error("抽屉里没有编辑备注入口");
    btn.click();
    const body = d4.querySelector("#drawer-body");
    if (!body.querySelector("input")) throw new Error("点了没有出现输入框");
    return true;
  });

  /* ── P0-5 proto-note ── */
  check("P0-5 设计说明有独立的 .proto-note 样式承载", () => {
    const css = require("fs").readFileSync(path.join(DIR, "demo.css"), "utf8");
    if (!/\.proto-note\s*\{/.test(css)) throw new Error("demo.css 里没有 .proto-note");
    if (!css.includes("原型说明")) throw new Error(".proto-note 没有「原型说明」前缀");
    return true;
  });

  /* ── P1-1 错误态 ── */
  const dom5 = await boot("dest=gateway&err=gateway&hint=0");
  check("P1-1 错误态：role=alert + 重试按钮", () => {
    const el = dom5.window.document.querySelector('[role="alert"]');
    if (!el) throw new Error("没有 role=alert");
    if (!el.textContent.includes("重试")) throw new Error("没有重试入口");
    return true;
  });
  check("P1-1 重试能恢复（错误态可消除）", () => {
    const d = dom5.window.document;
    const btn = [...d.querySelectorAll("[data-act]")].find((n) => n.textContent.includes("重试"));
    if (!btn) throw new Error("找不到重试按钮");
    btn.click();
    if (d.querySelector('[role="alert"]')) throw new Error("重试后错误态仍在");
    return true;
  });

  /* ── ★ 独立发现的缺陷：首屏「全选」按钮需点两次 ── */
  const domSA = await boot("dest=accounts&hint=0");
  const dSA = domSA.window.document;
  const saAct0 = dSA.querySelector("#select-all-btn").dataset.act;
  check("★「全选」按钮首屏 data-act 应为 select-all|1（否则第一次点击是空操作）", () => {
    if (saAct0 !== "select-all|1") throw new Error(`首屏是 ${saAct0}，点它只会清空已空的选择 = 空操作`);
    return true;
  });
  check("★「全选」一次点击即选中全部", () => {
    dSA.querySelector("#select-all-btn").click();
    const n = dSA.querySelectorAll("#acct-list .check.on").length;
    if (n === 0) throw new Error("点了一次仍一个都没选中");
    return n + " 个";
  });

  /* ── P1-2 部分失败 ──
     注意：D1 修好后「全选」**一次点击**即选中全部。旧脚本靠点两次绕开 bug，
     现在点两次会变成「全选→取消」，所以这里必须只点一次。 */
  const dom6 = await boot("dest=accounts&hint=0");
  const d6 = dom6.window.document;
  d6.querySelector("#select-all-btn").click();
  await sleep(40);
  const batchBtn = d6.querySelector('[data-act="batch-checkin"]');
  check("勾选后出现批量条", () => batchBtn || (() => { throw new Error("没有 batch-checkin 按钮，批量条未出现"); })());
  batchBtn.click();
  await sleep(60);
  check("P1-2 批量签到呈现部分失败（不是永远 100% 成功）", () => {
    const t = d6.querySelector("#content").textContent;
    if (!t.includes("失败")) throw new Error("没有失败态：批量操作仍显示全部成功");
    return true;
  });

  /* ── P1-3 权限引导 ── */
  const dom7 = await boot("dest=accounts&open=perm:a1&hint=0");
  check("P1-3 权限错误升级引导（三步 + 轮询自检）", () => {
    const t = dom7.window.document.querySelector("#drawer-body").textContent;
    if (!t.includes("自动检测") && !t.includes("检测中")) throw new Error("没有轮询自检指示");
    return true;
  });

  /* ── P1-5 键盘可达 ── */
  const dom8 = await boot("dest=accounts&hint=0");
  check("P1-5 账号卡可键盘聚焦", () => {
    const card = dom8.window.document.querySelector(".acct");
    if (card.getAttribute("tabindex") !== "0") throw new Error("没有 tabindex=0");
    if (!card.getAttribute("role")) throw new Error("没有 role");
    return true;
  });

  /* ── P1-6/7/8 视觉 ── */
  check("P1-6 待处理上移到统计块之前", () => {
    const html = dom0.window.document.querySelector("#content").innerHTML;
    if (html.indexOf("待处理") > html.indexOf("账号总数")) throw new Error("待处理仍在统计块之后");
    return true;
  });
  check("P1-7 头像底色亮度 L ≤ 0.5", () => {
    const src = require("fs").readFileSync(path.join(DIR, "demo-app.js"), "utf8");
    const tones = src.match(/avatarTone[\s\S]{0,400}?\]/)[0].match(/oklch\(([\d.]+)/g) || [];
    const bad = tones.filter((t) => parseFloat(t.replace("oklch(", "")) > 0.5);
    if (bad.length) throw new Error("仍有过亮的 tone：" + bad.join(","));
    return true;
  });
  check("P1-8 当前账号不再三重信号", () => {
    const css = require("fs").readFileSync(path.join(DIR, "demo.css"), "utf8");
    const block = (css.match(/\.acct\.is-current[^{]*\{[^}]*\}/g) || []).join("\n");
    if (/border-color\s*:/.test(block)) throw new Error("仍保留 border-color 信号");
    return true;
  });

  /* ══════════ 第二轮（B1~B5）独立核实 ══════════ */

  /* B1 · 向导：来源账号 + 三层会话树 */
  const domW = await boot("dest=accounts&open=wizard:a1&hint=0");
  const dW = domW.window.document;
  check("B1 第 1 步有「来源账号」选择（不再隐含当前账号）", () => {
    const t = dW.querySelector("#drawer-body").textContent;
    if (!t.includes("从哪里迁移")) throw new Error("第 1 步没有来源选择：" + t.slice(0, 80));
    return true;
  });
  check("B1 跨作用域入口存在（打通 WB→Trae 迁移）", () => {
    if (!dW.querySelector('[data-act="wiz-cross"]')) throw new Error("没有跨作用域入口");
    return true;
  });
  dW.querySelector('[data-act="wiz-next"]').click();
  check("B1 第 2 步是三层的会话树 + 「已选 N / M」计数", () => {
    const t = dW.querySelector("#drawer-body").textContent;
    if (!/已选 \d+ \/ \d+/.test(t)) throw new Error("没有「已选 N / M」计数");
    if (!t.includes("对话") && !t.includes("工作区")) throw new Error("没有类型层");
    return true;
  });
  check("B1 会话树可全不选 → 计数归零，再全选恢复", () => {
    /* ⚠️ 每次点击后必须**重新查询**：drawerWizard 会整块重渲染 #drawer-body，
       先前拿到的节点会脱离文档，而监听器委托在 document 上 ⇒ 对脱离节点 click 静默无效。 */
    const q = (sel) => dW.querySelector(sel);
    if (!q('[data-act="wiz-tree-all|0"]') || !q('[data-act="wiz-tree-all|1"]')) throw new Error("没有全选/全不选控件");
    q('[data-act="wiz-tree-all|0"]').click();
    let m = q("#drawer-body").textContent.match(/已选 (\d+) \/ (\d+)/);
    if (!m) throw new Error("读不到计数");
    if (Number(m[1]) !== 0) throw new Error(`全不选后仍是 ${m[1]}`);
    q('[data-act="wiz-tree-all|1"]').click();
    m = q("#drawer-body").textContent.match(/已选 (\d+) \/ (\d+)/);
    if (Number(m[1]) !== Number(m[2])) throw new Error(`全选后 ${m[1]} != ${m[2]}`);
    return `全不选 0 → 全选 ${m[2]}`;
  });
  dW.querySelector('[data-act="wiz-next"]').click();
  check("B1 确认页如实汇总「已选 N 个会话 / M 个目录」", () => {
    const t = dW.querySelector("#drawer-body").textContent;
    if (!/已选 \d+ 个会话/.test(t)) throw new Error("确认页没有如实汇总：" + t.slice(0, 120));
    if (!t.includes("影响")) throw new Error("确认页缺少「影响」说明");
    return true;
  });

  /* B2 · 登录态快照 —— 重点验「能力开关驱动」而非平台硬编码 */
  const domT = await boot("scope=trae-cn&dest=accounts&open=account:t1&hint=0");
  const domB = await boot("dest=accounts&open=account:a1&hint=0");
  check("B2 Trae 有「登录态快照」（capability=snapshot）", () => {
    if (!domT.window.document.querySelector("#drawer-body").textContent.includes("快照")) throw new Error("Trae 抽屉里没有快照");
    return true;
  });
  check("B2 WorkBuddy 无「登录态快照」（能力开关真的在生效）", () => {
    if (domB.window.document.querySelector("#drawer-body").textContent.includes("快照")) throw new Error("WB 不该有快照 —— 说明是平台硬编码而非能力开关");
    return true;
  });

  /* B3 · 账号池指定账号（排他） */
  const domP = await boot("dest=gateway&gw=pool&hint=0");
  const dP = domP.window.document;
  check("B3 账号池可「指定账号」且指定是排他的", () => {
    const pins = [...dP.querySelectorAll('[data-act^="pin|"]')];
    if (pins.length < 2) throw new Error("找不到两个以上的指定按钮");
    pins[0].click(); pins[1].click();
    const marked = dP.querySelectorAll("#content .badge").length && (dP.querySelector("#content").textContent.match(/已指定/g) || []).length;
    if (marked !== 1) throw new Error(`「已指定」出现 ${marked} 次，应恰好 1 次（排他）`);
    return true;
  });

  /* B4 · Anthropic 兼容接入指引 */
  check("B4 快速接入有 Anthropic 兼容 Tab 且给 /v1/messages", () => {
    const d = domP.window.document;
    d.querySelector('[data-act="nav|gateway"]').click();
    d.querySelector('[data-act="gw-sub|overview"]').click();
    const tab = d.querySelector('[data-act="gw-api|anthropic"]');
    if (!tab) throw new Error("没有 Anthropic Tab");
    tab.click();
    const t = d.querySelector("#content").textContent;
    if (!t.includes("/v1/messages")) throw new Error("没有 /v1/messages 示例");
    if (!t.includes("x-api-key") && !t.includes("anthropic-version")) throw new Error("缺少 Anthropic 专有请求头");
    return true;
  });

  /* B5 · 签到积分按作用域取（WB=100 / Trae=50） */
  check("B5 签到积分按作用域取值（Trae=50，不是硬编码 100）", () => {
    const t = domT.window.document.querySelector("#drawer-body").textContent;
    if (!t.includes("+50")) throw new Error("Trae 抽屉里没有 +50 签到记录");
    if (/\+\s*100/.test(t) && !t.includes("+50")) throw new Error("疑似硬编码 100");
    return true;
  });
  check("B5 WorkBuddy 签到积分 = +100", () => {
    const t = domB.window.document.querySelector("#drawer-body").textContent;
    if (!t.includes("+100")) throw new Error("WB 抽屉里没有 +100 签到记录");
    return true;
  });

  /* ══════════ 第三轮（F1~F9）独立核实 ══════════ */

  /* F4 · 程序位绑定模型：一个**程序位**同时只有一个账号；一个**账号**可绑 0..N 个程序位。
     这条断言是**方向性**的——单绑定模型也会「看起来正常」，所以必须断言
     「本账号的其他程序位**没有**被清掉」，而不只是断言「目标位被设上了」。 */
  const domM = await boot("dest=accounts&hint=0");
  const mw = domM.window;
  check("F4 初始态：a1 同时绑定 workbuddy 与 ide（真实应用支持「使用中 N 个工具」）", () => {
    const p = mw.eval("SCOPES['wb-cn'].accounts.find(a=>a.id==='a1').programs");
    if (!p.workbuddy || !p.ide) throw new Error("初始态不是双绑定：" + JSON.stringify(p));
    return true;
  });
  check("F4 把 IDE 切到 a3：a3.ide 置真，**a1.workbuddy 必须仍然为真**", () => {
    mw.document.querySelector('[data-act="acct-prog|a3|ide"]').click();
    const p1 = mw.eval("SCOPES['wb-cn'].accounts.find(a=>a.id==='a1').programs");
    const p3 = mw.eval("SCOPES['wb-cn'].accounts.find(a=>a.id==='a3').programs");
    if (!p3.ide) throw new Error("a3.ide 没被置真");
    if (!p1.workbuddy) throw new Error("★ a1.workbuddy 被清掉了 ⇒ 退回单绑定模型（把 IDE 切到 X 不该让 X 从 WorkBuddy 登出）");
    if (p1.ide) throw new Error("a1.ide 应被清掉（同一程序位只能有一个账号）");
    return "a1.workbuddy 保留，a1.ide 让出";
  });
  check("F4 同一程序位在其他账号上的绑定被清空", () => {
    const others = mw.eval("SCOPES['wb-cn'].accounts.filter(a=>a.id!=='a3').map(a=>a.programs.ide)");
    if (others.some(Boolean)) throw new Error("其他账号仍绑着 ide：" + JSON.stringify(others));
    return true;
  });
  /* ⚠️ 必须用**全新页面**查这枚 chip：上面的切换已经把 a1 的 ide 让给了 a3，
     此时已无任何账号绑 ≥2 个程序位。在同一个 DOM 上断言 = 断言一个不成立的前提。 */
  const domC = await boot("dest=accounts&hint=0");
  check("F4 卡片显示「使用中 N 个工具」（多绑定对用户可见）", () => {
    const t = domC.window.document.querySelector("#content").textContent;
    if (!/使用中 \d+ 个工具/.test(t)) throw new Error("没有「使用中 N 个工具」chip ⇒ 多绑定不可见");
    const n = t.match(/使用中 (\d+) 个工具/)[1];
    if (Number(n) < 2) throw new Error("N 应 ≥ 2，实际 " + n);
    return "使用中 " + n + " 个工具";
  });

  /* F1 · perm-retry —— 评审批评我「只查文案没点按钮」的补课。
     自愈是 3 轮 × 2000ms，必须真等；等待本身也验证了「2 秒轮询自愈」不是空话。
     ⚠️ 用 **a3** 而不是 a1：a1 本来就绑着 workbuddy，断言「切换后仍绑着」是恒真的。
        a3 初始未绑任何程序位 ⇒ 「切换后绑上了」才是一条可证伪的断言。 */
  const domR = await boot("dest=accounts&open=perm:a3&hint=0");
  const rw = domR.window;
  check("F1 权限引导：轮询自检文案存在且按钮初始禁用", () => {
    const btn = rw.document.querySelector("#perm-retry");
    if (!btn) throw new Error("没有 perm-retry 按钮");
    if (!btn.disabled) throw new Error("按钮初始就可用 ⇒ 没有体现「等自检通过」");
    if (!rw.document.querySelector("#perm-status").textContent.includes("自检")) throw new Error("没有自检文案");
    if (rw.eval("SCOPES['wb-cn'].accounts.find(a=>a.id==='a3').programs.workbuddy")) throw new Error("a3 初始就绑着 workbuddy ⇒ 断言前提不成立");
    return true;
  });
  await sleep(6600);
  check("F1 自检 3 轮后按钮自动可用（真的会自愈）", () => {
    const btn = rw.document.querySelector("#perm-retry");
    if (btn.disabled) throw new Error("等了 6.6s 按钮仍禁用 ⇒ 自愈没发生");
    if (!rw.document.querySelector("#perm-status").textContent.includes("已检测到管理员权限")) throw new Error("状态文案没变成已检测到");
    return true;
  });
  check("F1 ★ 点「重试切换」真的切换了程序位（不是只关抽屉）", () => {
    rw.document.querySelector("#perm-retry").click();
    const toasts = rw.document.querySelector("#toasts").textContent;
    if (!toasts.includes("已把")) throw new Error("点击后没有任何成功反馈 ⇒ 死按钮：" + JSON.stringify(toasts.slice(0, 60)));
    const bound = rw.eval("SCOPES['wb-cn'].accounts.find(a=>a.id==='a3').programs.workbuddy");
    if (!bound) throw new Error("a3 的程序位没有被真的切换（点击只关了抽屉）");
    return true;
  });

  /* F2 · 资源包名不重复 */
  check("F2 同一账号内资源包名不重复", () => {
    const dup = mw.eval(`(() => {
      const bad = [];
      for (const sid of Object.keys(SCOPES)) for (const a of SCOPES[sid].accounts) {
        if (!a.packs) continue;
        const names = a.packs.map(p => p.name);
        if (new Set(names).size !== names.length) bad.push(sid + '/' + a.name + ' = ' + names.join(','));
      }
      return bad;
    })()`);
    if (dup.length) throw new Error("仍有重名：" + dup.slice(0, 3).join(" | "));
    return true;
  });

  /* F3 · 目录名与会话名不共用字符串 */
  check("F3 目录名与会话标题不是同一批字符串", () => {
    const overlap = mw.eval(`(() => {
      const dirs = new Set(SESSION_SEEDS.dirs);
      return SESSION_SEEDS.sessions.filter(s => dirs.has(s));
    })()`);
    if (overlap.length) throw new Error("重叠：" + overlap.slice(0, 3).join(" / "));
    return true;
  });

  /* F5 · 过期/需重登账号不给「手动签到」 */
  const domX = await boot("dest=accounts&open=account:a5&hint=0");
  check("F5 过期账号抽屉里没有「手动签到」入口", () => {
    const t = domX.window.document.querySelector("#drawer-body").textContent;
    if (t.includes("手动签到")) throw new Error("过期账号仍提供手动签到 ⇒ 与批量签到的「结构性失败」口径矛盾");
    return true;
  });

  /* F6 · 已排除账号不可指定 */
  const domQ = await boot("dest=gateway&gw=pool&hint=0");
  check("F6 已排除账号的「指定」按钮不可用", () => {
    const btns = [...domQ.window.document.querySelectorAll("#content button")].filter((b) => b.textContent.trim() === "指定");
    if (!btns.length) throw new Error("找不到任何「指定」按钮");
    if (!btns.some((b) => b.disabled)) throw new Error("已排除行（design-lab）的指定按钮仍可用");
    return true;
  });

  /* F7 · 合并态积分流水按真实时间戳排序 */
  check("F7 合并态积分流水按时间戳降序（不是中文字符串序）", () => {
    const ok = mw.eval(`(() => {
      const ids = Object.keys(SCOPES);
      const all = [];
      for (const id of ids) for (const c of (USAGE[id].credits || [])) all.push(c);
      if (all.some(c => typeof c.ts !== 'number')) return 'MISSING_TS';
      const sorted = [...all].sort((a, b) => b.ts - a.ts);
      for (let i = 1; i < sorted.length; i++) if (sorted[i].ts > sorted[i-1].ts) return 'BAD_ORDER';
      return 'OK';
    })()`);
    if (ok === "MISSING_TS") throw new Error("credits 条目缺少 ts 字段，无法按时间排序");
    if (ok === "BAD_ORDER") throw new Error("排序不是时间序");
    return true;
  });

  /* F8 · 设置页危险按钮不再是死按钮 */
  const domS = await boot("dest=settings&set=diagnostics&hint=0");
  check("F8 设置页危险按钮有事件绑定（不再是死按钮）", () => {
    const d = domS.window.document;
    const btns = [...d.querySelectorAll("#content button")].filter((b) => ["重置", "导出"].includes(b.textContent.trim()));
    if (!btns.length) throw new Error("找不到危险操作按钮");
    const dead = btns.filter((b) => !b.dataset.act);
    if (dead.length) throw new Error(`仍有 ${dead.length} 个无 data-act 的死按钮`);
    return true;
  });

  /* ══════════ 用量工作台图表 ══════════ */

  const domT2 = await boot("dest=usage&usage=trend&range=30&hint=0");
  const dT = domT2.window.document;
  const q = (root, sel) => [...root.querySelectorAll(sel)];

  check("用量·趋势：至少 4 张图，且每张都有非空 aria-label", () => {
    const imgs = q(dT, '[role="img"]');
    if (imgs.length < 4) throw new Error("只有 " + imgs.length + " 张图");
    const noLabel = imgs.filter((n) => !(n.getAttribute("aria-label") || "").trim());
    if (noLabel.length) throw new Error(noLabel.length + " 张图没有 aria-label（看不了图的人拿不到信息）");
    return imgs.length + " 张";
  });
  check("用量·趋势：堆叠柱的列数 = 天数，且每列分段数 = 模型数", () => {
    const cols = q(dT, ".cbar .ccol");
    if (cols.length !== 30) throw new Error("列数 " + cols.length + "，期望 30");
    const models = domT2.window.eval("USAGE[state.scope].ext.modelDays.length");
    const segsPerCol = cols.map((c) => c.querySelectorAll("i").length);
    const bad = segsPerCol.filter((n) => n !== models);
    if (bad.length) throw new Error(`每列分段数应为 ${models}，实际有 ${[...new Set(bad)].join("/")}`);
    return `${cols.length} 列 × ${models} 段`;
  });
  check("用量·趋势：堆叠柱各分段高度之和 ≈ 当日总量（图不能与数据打架）", () => {
    const cols = q(dT, ".cbar .ccol");
    const maxDay = domT2.window.eval("Math.max(...USAGE[state.scope].days.slice(-30).map(d=>d.value))");
    const sumLastCol = [...cols[cols.length - 1].querySelectorAll("i")]
      .reduce((s, i) => s + parseFloat(i.style.height || "0"), 0);
    const expect = domT2.window.eval("USAGE[state.scope].days[USAGE[state.scope].days.length-1].value") / maxDay * 100;
    if (Math.abs(sumLastCol - expect) > 1.5) throw new Error(`末列分段和 ${sumLastCol.toFixed(1)}% vs 期望 ${expect.toFixed(1)}%`);
    return sumLastCol.toFixed(1) + "%";
  });
  check("用量·趋势：热力图是 7 行 × 24 列", () => {
    const rows = q(dT, ".cheat-row");
    if (rows.length !== 7) throw new Error("行数 " + rows.length);
    const bad = rows.map((r) => r.querySelectorAll(".cells i").length).filter((n) => n !== 24);
    if (bad.length) throw new Error("列数不是 24：" + [...new Set(bad)].join("/"));
    return "7×24";
  });
  check("用量·趋势：环形图扇区数 = 模型数", () => {
    const arcs = q(dT, ".cdonut circle").length;
    const models = domT2.window.eval("USAGE[state.scope].ext.modelDays.length");
    if (arcs !== models) throw new Error(`扇区 ${arcs} ≠ 模型 ${models}`);
    return arcs + " 段";
  });
  check("用量·趋势：累计曲线图注不再写「峰值 = 末值」这种废话", () => {
    const cap = dT.querySelector(".carea figcaption").textContent;
    if (!cap.includes("累计")) throw new Error("图注不符：" + cap);
    return cap.trim();
  });

  const domM2 = await boot("dest=usage&usage=models&range=30&hint=0");
  const dM = domM2.window.document;
  check("用量·模型：环形 + 每行一条迷你趋势线", () => {
    const models = domM2.window.eval("USAGE[state.scope].ext.modelDays.length");
    const bars = q(dM, ".hbar").length;
    if (bars !== models) throw new Error(`排行条 ${bars} ≠ 模型 ${models}`);
    const sparks = q(dM, ".hbar .cspark").length;
    if (sparks !== models) throw new Error(`迷你趋势线 ${sparks} ≠ 模型 ${models}`);
    if (!q(dM, ".cdonut circle").length) throw new Error("没有环形图");
    return `${bars} 行 · ${sparks} 条趋势线`;
  });

  const domS2 = await boot("dest=usage&usage=sessions&range=30&hint=0");
  const dS = domS2.window.document;
  check("用量·会话：气泡数 = 会话数，排行条数 = 会话数", () => {
    const n = domS2.window.eval("USAGE[state.scope].sessions.length");
    const bubs = q(dS, ".cscatter .bub").length;
    const bars = q(dS, ".hbar").length;
    if (bubs !== n) throw new Error(`气泡 ${bubs} ≠ 会话 ${n}`);
    if (bars !== n) throw new Error(`排行条 ${bars} ≠ 会话 ${n}`);
    return `${n} 个会话 · ${bubs} 个气泡`;
  });
  check("用量·会话：气泡不溢出绘图区（半径已留边）", () => {
    const bubs = q(dS, ".cscatter .bub");
    const over = bubs.filter((b) => parseFloat(b.style.left) > 93 || parseFloat(b.style.bottom) > 93);
    if (over.length) throw new Error(over.length + " 个气泡越界");
    return true;
  });

  const domC2 = await boot("dest=usage&usage=credits&range=30&hint=0");
  const dC = domC2.window.document;
  check("用量·积分：双向柱 + 余额曲线 + 到期时间轴 + 两个环形", () => {
    const div = q(dC, ".cdiv .dcol").length;
    if (div !== 30) throw new Error("双向柱列数 " + div);
    const upDown = q(dC, ".cdiv .up").length && q(dC, ".cdiv .dn").length;
    if (!upDown) throw new Error("双向柱缺少入账或出账侧");
    const areas = q(dC, ".carea").length;
    if (areas < 1) throw new Error("没有余额曲线");
    const bars = q(dC, ".bars .bar").length;
    if (bars !== 30) throw new Error("到期分布应为完整 30 天时间轴，实际 " + bars + " 根");
    const donuts = q(dC, ".cdonut").length;
    if (donuts !== 2) throw new Error("环形图应有 2 个（账号占比 + 状态码），实际 " + donuts);
    return `双向柱 30 · 曲线 ${areas} · 到期 ${bars} · 环形 ${donuts}`;
  });
  check("用量·积分：余额曲线基线不锚 0（否则高位小幅波动会被压成直线）", () => {
    const cap = dC.querySelector(".carea figcaption").textContent;
    if (!cap.includes("非 0")) throw new Error("图注没说明基线不是 0：" + cap);
    return cap.trim();
  });

  /* 合并态必须把**图表数据**也一起聚合，不能只有 days 变 */
  check("用量·合并：模型扇区数真的变多（mergeExt 聚合了图表数据）", () => {
    const before = q(dT, ".cdonut circle").length;
    dT.querySelector('[data-act="merge|1"]').click();
    const after = q(dT, ".cdonut circle").length;
    if (after <= before) throw new Error(`合并前 ${before} 段 → 合并后 ${after} 段，图表数据没聚合`);
    return `${before} → ${after} 段`;
  });

  /* ══════════ 全局备份与恢复（PLAN §3.14） ══════════ */
  const domBK = await boot("dest=settings&set=backup&hint=0");
  const dB = domBK.window.document;
  const bq = (s) => dB.querySelector(s);
  const bByAct = (act) => [...dB.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const flat = (s) => String(s || "").replace(/\s+/g, " ").trim();

  check("备份页：设置子导航含「备份与恢复」，共 9 项", () => {
    const items = [...dB.querySelectorAll(".subnav button")].map((b) => b.textContent.trim());
    if (items.length !== 9) throw new Error(`子导航 ${items.length} 项，期望 9 项`);
    if (!items.includes("备份与恢复")) throw new Error("没有「备份与恢复」：" + items.join("/"));
    return items.join(" / ");
  });

  check("备份页：默认勾选全部 4 个作用域（跨作用域一次导出）", () => {
    const rows = [...dB.querySelectorAll(".bk-export-list .bk-row")];
    if (rows.length !== 4) throw new Error(`${rows.length} 行，期望 4 行`);
    const off = rows.filter((r) => !r.querySelector(".check").classList.contains("on"));
    if (off.length) throw new Error(`${off.length} 个作用域默认未勾选`);
    return "4/4 默认全选";
  });

  check("备份页：默认不含凭据（安全默认），预估行如实声明", () => {
    const sw = bByAct("bk-creds");
    if (!sw) throw new Error("找不到凭据开关");
    if (sw.getAttribute("aria-checked") !== "false") throw new Error("凭据开关默认应为关");
    const est = flat(bq(".bk-est").textContent);
    if (!est.includes("不含凭据")) throw new Error("预估行没声明「不含凭据」：" + est);
    return est;
  });

  check("★ 打开凭据开关：出现风险警告 + 主按钮文案随之变化", () => {
    bByAct("bk-creds").click();
    const warn = bq(".bk-warn");
    if (!warn) throw new Error("打开凭据后没有出现警告条");
    if (!/明文凭据/.test(warn.textContent)) throw new Error("警告条没说清风险：" + warn.textContent);
    const btn = bByAct("bk-export");
    if (!/含凭据/.test(btn.textContent)) throw new Error("主按钮文案没跟着变：" + btn.textContent);
    return flat(btn.textContent);
  });

  check("★ 预估大小随勾选实时变化（不是写死的装饰数字）", () => {
    const before = flat(bq(".bk-est").textContent);
    bByAct("bk-part|wb-global|accounts").click();
    const after = flat(bq(".bk-est").textContent);
    if (after === before) throw new Error("取消勾选后预估没变 ⇒ 数字是写死的");
    return `${before} → ${after}`;
  });

  check("★ 三态勾选：取消一个分区后行头 aria-checked=mixed（半选语义）", () => {
    const row = [...dB.querySelectorAll(".bk-export-list .bk-row")][1];   // wb-global
    const head = row.querySelector(".check");
    if (head.getAttribute("aria-checked") !== "mixed") {
      throw new Error(`行头 aria-checked = ${head.getAttribute("aria-checked")}，期望 mixed`);
    }
    if (!head.classList.contains("mixed")) throw new Error("缺少 .mixed 视觉态");
    return "aria-checked=mixed";
  });

  check("关掉凭据开关：警告条消失，预估行回到「不含凭据」", () => {
    bByAct("bk-creds").click();
    if (bq(".bk-warn")) throw new Error("关闭凭据后警告条仍在");
    const est = flat(bq(".bk-est").textContent);
    if (!est.includes("不含凭据")) throw new Error("预估行没回到「不含凭据」：" + est);
    return est;
  });

  check("导入：先预览再落盘（点「选择备份文件」不会直接写入）", () => {
    bByAct("bk-import").click();
    const head = bq(".bk-pv-head");
    if (!head) throw new Error("点了「选择备份文件」没有出现预览");
    if (!/formatVersion/.test(head.textContent)) throw new Error("预览没显示契约版本：" + head.textContent);
    const rows = [...dB.querySelectorAll(".bk-pv-list .bk-row")];
    if (rows.length !== 2) throw new Error(`预览 ${rows.length} 行，期望 2 行`);
    return flat(head.textContent);
  });

  check("导入预览：不含凭据时明确提示「需重新登录」", () => {
    const warn = bq(".bk-warn");
    if (!warn || !/重新登录/.test(warn.textContent)) throw new Error("没有提示需重新登录");
    return flat(warn.textContent);
  });

  check("冲突策略可切换，且切换真的生效", () => {
    const before = flat(bq(".seg button.on").textContent);
    bByAct("bk-conflict|overwrite").click();
    const after = flat(bq(".seg button.on").textContent);
    if (after === before) throw new Error("切换冲突策略后选中项没变");
    return `${before} → ${after}`;
  });

  check("★ 导入结果是部分失败态（失败计数 + 重试按钮），不是一句「完成」", () => {
    bByAct("bk-run").click();
    const est = bq(".bk-res .bk-est");
    if (!est) throw new Error("导入后没有结果区");
    if (!/失败/.test(est.textContent)) throw new Error("结果里没有失败计数：" + est.textContent);
    if (!bByAct("bk-retry")) throw new Error("没有「重试失败项」按钮");
    return flat(est.textContent);
  });

  /* 独立 boot 一个干净页面：验证「一个都不勾」时不能导出空备份 */
  const domBK2 = await boot("dest=settings&set=backup&hint=0");
  const dB2 = domBK2.window.document;
  const bByAct2 = (act) => [...dB2.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  check("★ 全部取消勾选后，导出按钮被禁用（不能导出空备份）", () => {
    for (const s of ["wb-cn", "wb-global", "trae-cn", "trae-global"]) bByAct2("bk-scope|" + s).click();
    const btn = bByAct2("bk-export");
    if (!btn.hasAttribute("disabled")) throw new Error("没有任何勾选，导出按钮仍可点");
    return "disabled";
  });

  /* ══════════ 会话工作台（PLAN §3.15） ══════════ */
  const domSG = await boot("dest=sessions&hint=0");
  const dSG = domSG.window.document;
  const sq = (s) => dSG.querySelector(s);
  const sByAct = (act) => [...dSG.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const sFlat = (s) => String(s || "").replace(/\s+/g, " ").trim();

  check("★ 客户端只有 1 个时不渲染 Tab（单选项的「切换器」是假控件）", () => {
    if (dSG.querySelector(".cli-tabs")) throw new Error("只有 1 个客户端却渲染了 Tab");
    if (dSG.querySelectorAll(".cli-tab").length) throw new Error("存在 Tab 按钮");
    /* 对照：清单确实只有 1 项 —— 排除「清单是空的」这种假通过。
       ⚠️ `SESSION_CLIENTS` 是 `const`，**不会挂到 `window`**（只有 `function`/`var` 会）⇒ 必须用 eval。 */
    const n = domSG.window.eval("SESSION_CLIENTS.length");
    if (n !== 1) throw new Error(`客户端清单 ${n} 项，期望 1`);
    return "退化：无 Tab（客户端 1 项）";
  });

  check("★ 状态筛选选项按需出现：数据里没有的状态不渲染（不出现假选项）", () => {
    const opts = [...dSG.querySelectorAll(".sess-filter-seg button")].map((b) => b.textContent.trim());
    /* 演示数据只有「待同步」与「有分歧」两种状态 ⇒ 选项 = 全部 + 2 = 3 */
    if (opts.length !== 3) {
      throw new Error(`渲染了 ${opts.length} 个选项（${opts.join("/")}），期望 3 —— 多出的是「点了永远是空」的假选项`);
    }
    if (opts.includes("已一致")) throw new Error("数据里没有「已一致」的组，却渲染了该选项");
    if (!opts.includes("有分歧")) throw new Error("有分叉组，却没渲染「有分歧」");
    return opts.join(" / ");
  });

  /* 注：「结构化空态」的断言已合并到下面 Trae 的那条（原版依赖的「空客户端」是演示数据里
     一个不存在的客户端，已随 v2.6 的客户端清单修正一并删除）。 */

  check("★ 会话组状态由成员派生（有分叉→有分歧；有落后→待同步；否则已一致）", () => {
    const f = domSG.window.sessionGroupStatus;
    if (typeof f !== "function") throw new Error("sessionGroupStatus 未定义");
    const d = f({ members: [{ state: "latest" }, { state: "diverge" }] });
    const b = f({ members: [{ state: "latest" }, { state: "behind" }] });
    const l = f({ members: [{ state: "latest" }] });
    if (d !== "diverge" || b !== "behind" || l !== "latest") throw new Error(`派生错误：${d}/${b}/${l}`);
    return `${d} / ${b} / ${l}`;
  });

  check("会话关联图：标出共同旧版，且每个副本给出相对旧版的差异", () => {
    sByAct("sess-open|sg1").click();
    const base = sq(".sg-base");
    if (!base || !/共同旧版/.test(base.textContent)) throw new Error("关联图没标出共同旧版");
    const members = [...dSG.querySelectorAll(".sgm")];
    if (members.length !== 2) throw new Error(`${members.length} 个副本，期望 2`);
    const deltas = members.map((m) => sFlat(m.querySelector(".sgm-delta").textContent));
    if (!deltas.some((d) => /落后/.test(d))) throw new Error("副本没显示「落后 N 条」：" + deltas.join("/"));
    return `共同旧版 + ${deltas.join(" / ")}`;
  });

  check("★ 分叉的组：给分歧提示，且只能先「选择保留哪一份」——不许直接同步", () => {
    sByAct("sess-open|sg3").click();                   // sg3 含 diverge 成员
    const warn = sq(".drawer .bk-warn");
    if (!warn || !/分歧/.test(warn.textContent)) throw new Error("分叉组没有分歧提示");
    if (!sByAct("sess-unify|sg3")) throw new Error("分叉组没有「选择保留哪一份」入口");
    if (sByAct("sess-sync|sg3")) throw new Error("分叉组不该直接给「同步到全部副本」（会静默覆盖）");
    return "有分歧提示 + 只能先选保留哪一份";
  });

  check("★ 未分叉的组：文案必须写明「同步是加法」（用户最怕被覆盖）", () => {
    sByAct("sess-open|sg1").click();
    const txt = sFlat(sq(".drawer").textContent);
    if (!/加法/.test(txt)) throw new Error("没说明同步是加法");
    if (!sByAct("sess-sync|sg1")) throw new Error("未分叉组没有同步按钮");
    return "有加法说明 + 同步按钮";
  });

  check("侧栏「会话」徽章 = 待同步会话组数（落后 + 分叉）", () => {
    const item = [...dSG.querySelectorAll("#nav .nav-item")].find((b) => /会话/.test(b.textContent));
    if (!item) throw new Error("侧栏没有会话项");
    const badge = item.querySelector(".nav-badge");
    if (!badge) throw new Error("会话项没有徽章");
    const pending = domSG.window.sessionPendingCount();
    if (Number(badge.textContent) !== pending) throw new Error(`徽章 ${badge.textContent} ≠ 待同步 ${pending}`);
    return `${badge.textContent}（待同步）`;
  });

  check("会话页：状态筛选真的收敛结果（不是假开关）", () => {
    const all = dSG.querySelectorAll(".sg-card").length;
    sByAct("sess-filter|diverge").click();
    const only = dSG.querySelectorAll(".sg-card").length;
    if (!(only > 0 && only < all)) throw new Error(`筛选前 ${all} → 筛选后 ${only}，没收敛`);
    return `${all} → ${only}`;
  });

  /* ══════════ 网关接入与模型自动识别（PLAN §3.17） ══════════ */
  const domGW = await boot("dest=gateway&gw=models&hint=0");
  const dGW = domGW.window.document;
  const gwq = (s) => dGW.querySelector(s);
  const gwByAct = (act) => [...dGW.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const gwFlat = (s) => String(s || "").replace(/\s+/g, " ").trim();
  /* check 的回调是同步的 ⇒ 需要多态的页面必须**先 boot 好**（await 不能写在 check 里）。 */
  const domGWG = await boot("dest=gateway&gw=models&scope=wb-global&hint=0");    // 已保存
  const domGWT = await boot("dest=gateway&gw=models&scope=trae-global&hint=0");  // 内置
  const domGWO = await boot("dest=gateway&hint=0");                              // 概览（接入指引）

  check("模型页：显式标注清单来源 + 上次识别时间（不伪装）", () => {
    const bar = gwq(".cat-bar");
    if (!bar) throw new Error("模型页没有来源条");
    const txt = gwFlat(bar.textContent);
    if (!/实时/.test(txt)) throw new Error("默认作用域应显示「实时」：" + txt);
    if (!/上次识别/.test(txt)) throw new Error("没有显示上次识别时间：" + txt);
    return txt;
  });

  check("★ 来源条说明「接入后自动识别」（不是只靠手动刷新）", () => {
    const txt = gwFlat(gwq(".cat-bar").textContent);
    if (!/接入后自动识别/.test(txt)) throw new Error("没说明接入后自动识别：" + txt);
    return "已说明接入后自动识别";
  });

  check("★ 来源非「实时」时必须给出原因（不能只给一个灰徽标让用户猜）", () => {
    const bar = domGWG.window.document.querySelector(".cat-bar");
    const txt = String(bar.textContent).replace(/\s+/g, " ").trim();
    if (!/已保存/.test(txt)) throw new Error("wb-global 应显示「已保存」：" + txt);
    if (!/超时/.test(txt)) throw new Error("没有给出回落原因：" + txt);
    return txt;
  });

  check("★ 从未识别成功时显示「内置」+ 原因（而非静默给一份清单）", () => {
    const bar = domGWT.window.document.querySelector(".cat-bar");
    const txt = String(bar.textContent).replace(/\s+/g, " ").trim();
    if (!/内置/.test(txt)) throw new Error("trae-global 应显示「内置」：" + txt);
    if (!/尚未成功识别过/.test(txt)) throw new Error("没有说明尚未识别过：" + txt);
    return txt;
  });

  check("★ 重新识别：先进入「识别中」并禁用按钮（防重复点击），非阻塞", () => {
    const btn = gwByAct("gw-scan");
    if (!btn) throw new Error("没有「重新识别」按钮");
    btn.click();
    const after = gwByAct("gw-scan");
    if (!after.hasAttribute("disabled")) throw new Error("识别中按钮应禁用");
    if (!/识别中/.test(after.textContent)) throw new Error("没进入「识别中」态：" + after.textContent);
    return gwFlat(after.textContent);
  });

  check("★ 文案不承诺「N 个模型可用」（HTTP 200 ≠ 模型可用）", () => {
    const txt = gwFlat(dGW.querySelector(".split-main").textContent);
    if (/\d+ 个模型可用/.test(txt)) throw new Error("不该整体承诺「可用」：" + txt);
    return "只声明「已识别」，未承诺可用性";
  });

  check("★ 接入指引说明客户端会自动发现模型（不用手填模型名）", () => {
    const txt = String(domGWO.window.document.querySelector(".split-main").textContent).replace(/\s+/g, " ").trim();
    if (!/\/v1\/models/.test(txt)) throw new Error("接入指引没提 /v1/models");
    if (!/不需要手填模型名/.test(txt)) throw new Error("没说明不需要手填模型名");
    return "接入指引含自动发现说明";
  });

  /* ══════════ 模型禁用与接入说明（PLAN §3.18） ══════════ */
  const domMD = await boot("dest=gateway&gw=models&hint=0");
  const dMD = domMD.window.document;
  const mdByAct = (act) => [...dMD.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const mdFlat = (s) => String(s || "").replace(/\s+/g, " ").trim();
  const domGA = await boot("dest=gateway&api=anthropic&hint=0");   // Anthropic 协议
  const domGO = await boot("dest=gateway&api=openai&hint=0");      // OpenAI 协议
  /* 暴露计数读成函数 + 记初始值：不硬编码数字（否则改一条演示数据就要改断言）。 */
  const mdServed = () =>
    Number(mdFlat(dMD.querySelector(".split-main").textContent).match(/当前暴露 (\d+) \/ (\d+)/)[1]);
  const mdInitial = mdServed();

  check("模型页：禁用与「上游可用性」是两个独立维度，三种展示可区分", () => {
    const rows = [...dMD.querySelectorAll(".tbl tbody tr")];
    if (rows.length !== 6) throw new Error(`${rows.length} 行，期望 6`);
    const txt = mdFlat(dMD.querySelector(".split-main").textContent);
    for (const want of ["已暴露", "已禁用", "不可用"]) {
      if (!txt.includes(want)) throw new Error("缺少状态「" + want + "」");
    }
    return "已暴露 / 已禁用 / 上游不可用 可区分";
  });

  check("★ 禁用模型：暴露计数随之下降（不是假开关）", () => {
    const before = mdFlat(dMD.querySelector(".split-main").textContent).match(/当前暴露 (\d+) \/ (\d+)/);
    if (!before) throw new Error("没有暴露计数");
    mdByAct("model-disable|claude-sonnet-4.5|国内版").click();
    const after = mdFlat(dMD.querySelector(".split-main").textContent).match(/当前暴露 (\d+) \/ (\d+)/);
    if (Number(after[1]) !== Number(before[1]) - 1) {
      throw new Error(`禁用后暴露数没减：${before[1]} → ${after[1]}`);
    }
    return `${before[1]} → ${after[1]}`;
  });

  check("★ 禁用可逆：再次启用后计数恢复", () => {
    mdByAct("model-enable|claude-sonnet-4.5|国内版").click();
    if (mdServed() !== mdInitial) throw new Error(`启用后应恢复到 ${mdInitial}，实际 ${mdServed()}`);
    return `恢复到 ${mdInitial}`;
  });

  check("★ 被禁用的模型仍可见（标灰），不是彻底消失", () => {
    const row = [...dMD.querySelectorAll(".tbl tbody tr")].find((r) => /claude-opus-4\.1/.test(r.textContent));
    if (!row) throw new Error("被禁用的模型整行消失了 —— 用户会以为清单里没有它");
    if (!row.classList.contains("row-off")) throw new Error("被禁用的行没有标灰");
    return "可见 + 标灰";
  });

  check("★ 页面说明「两个出口同时生效」（否则禁用就是假开关）", () => {
    const txt = mdFlat(dMD.querySelector(".split-main").textContent);
    if (!/不会出现在/.test(txt) || !/直接调用也会被拒绝/.test(txt)) {
      throw new Error("没有说明两个出口：" + txt);
    }
    return "已说明 /v1/models 隐藏 + 直接调用被拒";
  });

  check("★ 接入说明按协议给对认证头（Anthropic→x-api-key / OpenAI→Bearer）", () => {
    const a = mdFlat(domGA.window.document.querySelector(".split-main").textContent);
    if (!/x-api-key/.test(a)) throw new Error("Anthropic 页没给 x-api-key");
    if (!/\/v1\/messages/.test(a)) throw new Error("Anthropic 页没提 /v1/messages");
    const o = mdFlat(domGO.window.document.querySelector(".split-main").textContent);
    if (!/Authorization: Bearer/.test(o)) throw new Error("OpenAI 页没给 Bearer");
    if (!/\/v1\/chat\/completions/.test(o)) throw new Error("OpenAI 页没提 /v1/chat/completions");
    return "Anthropic→x-api-key / OpenAI→Bearer";
  });

  check("接入说明：Codex 给了 model_provider 的 base_url 与 env_key", () => {
    const pre = domGO.window.document.querySelectorAll(".split-main pre")[0];
    const txt = String(pre.textContent).replace(/\s+/g, " ").trim();
    if (!/base_url/.test(txt) || !/env_key/.test(txt)) throw new Error("Codex 配置不完整：" + txt);
    return "base_url + env_key";
  });

  check("★ 通用工具页不编造配置（不出现具体配置文件片段）", () => {
    const d = domGA.window.document;
    const btn = [...d.querySelectorAll("[data-act]")].find((el) => el.dataset.act === "gw-tool|generic");
    if (!btn) throw new Error("没有 generic 工具 Tab");
    btn.click();
    const pre = d.querySelectorAll(".split-main pre")[0];
    const txt = String(pre.textContent).replace(/\s+/g, " ").trim();
    if (/config\.toml|settings\.json|opencode\.json/.test(txt)) {
      throw new Error("通用页混入了具体配置文件片段：" + txt);
    }
    if (!/Base URL/.test(txt)) throw new Error("通用页没给通用参数：" + txt);
    return "只给通用参数，未编造配置文件";
  });

  /* ══════════ 桌面悬浮窗与邮箱脱敏（§3.16 / §3.3，D21/D22 已拍板） ══════════ */
  const domCP = await boot("hint=0");
  const dCP = domCP.window.document;
  const cpByAct = (act) => [...dCP.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const cpFlat = (s) => String(s || "").replace(/\s+/g, " ").trim();
  const domEM = await boot("dest=accounts&hint=0");

  check("★ 悬浮窗：默认显示，四态齐备（运行中/待确认/已完成/失败）", () => {
    const cp = dCP.querySelector(".companion");
    if (!cp) throw new Error("没有悬浮窗");
    const rows = [...dCP.querySelectorAll(".cp-row")];
    if (rows.length !== 4) throw new Error(`${rows.length} 条，期望 4`);
    const dots = [...dCP.querySelectorAll(".cp-dot")].map((d) => d.className.replace(/^cp-dot\s*/, ""));
    for (const want of ["run", "wait", "done", "fail"]) {
      if (!dots.includes(want)) throw new Error("缺少状态 " + want + "：" + dots.join("/"));
    }
    return dots.join(" / ");
  });

  check("★ 悬浮窗是只读的：不含任何写操作（§3.16 明确「不做的」）", () => {
    const acts = [...new Set([...dCP.querySelectorAll(".companion [data-act]")]
      .map((el) => el.dataset.act.split("|")[0]))];
    const forbidden = ["switch", "delete", "batch-delete", "batch-checkin", "model-disable", "bk-export", "acct-prog", "pin"];
    const bad = acts.filter((a) => forbidden.includes(a));
    if (bad.length) throw new Error("悬浮窗里出现了写操作：" + bad.join("/"));
    return "仅 " + acts.join(" / ");
  });

  check("★ 悬浮窗标题默认模糊（防截图泄露），悬停才展开", () => {
    /* ★ 必须读 `DIR`（当前被测目录）而不是 `BASE`（原目录）：
       变异注入发生在 `DIR`，读 `BASE` 会让断言在变异下**恒真** ⇒ 假验证。
       （2026-10-03 真踩到：`cpblur` 因此空转，跑出「102 passed / 0 failed」。） */
    const css = fs.readFileSync(path.join(DIR, "demo.css"), "utf8");
    if (!/\.cp-title\s*\{[^}]*filter:\s*blur/.test(css)) throw new Error(".cp-title 没有默认 blur 规则");
    if (!/\.cp-row:hover\s+\.cp-title[^{]*\{[^}]*filter:\s*none/.test(css)) throw new Error("没有悬停展开规则");
    return "默认 blur + 悬停 none";
  });

  check("★ 悬浮窗可关闭，且能从侧栏左下角重新打开（入口不是一次性的）", () => {
    cpByAct("cp-close").click();
    if (dCP.querySelector(".companion")) throw new Error("点了关闭但悬浮窗还在");
    const toggle = cpByAct("cp-toggle");
    if (!toggle) throw new Error("侧栏没有悬浮窗入口");
    toggle.click();
    if (!dCP.querySelector(".companion")) throw new Error("从侧栏入口打不开");
    return "关闭 → 侧栏入口 → 重新打开";
  });

  check("悬浮窗：点击条目回到原会话（有明确反馈）", () => {
    cpByAct("cp-jump|cp1").click();
    const t = dCP.querySelector("#toasts");
    if (!t || !/已回到/.test(t.textContent)) throw new Error("点击后没有反馈");
    return cpFlat(t.textContent);
  });

  check("★ 邮箱默认脱敏：账号页不出现完整邮箱，只出现脱敏形态", () => {
    const txt = String(domEM.window.document.querySelector("#content").textContent);
    if (/jackey@example\.com/.test(txt)) throw new Error("账号页出现了完整邮箱");
    if (!/j\*+@example\.com/.test(txt)) throw new Error("没有看到脱敏形态");
    return "本地段已打码";
  });

  check("★ 搜索仍按完整邮箱匹配（脱敏只作用于展示，不作用于数据）", () => {
    const src = fs.readFileSync(path.join(DIR, "demo-app.js"), "utf8");   // ★ 同上：读 DIR
    if (/\[a\.name,\s*maskEmail\(a\.email\)/.test(src)) {
      throw new Error("搜索把展示用的脱敏值当成了数据 —— 会导致搜不到");
    }
    if (!/\[a\.name,\s*a\.email,\s*a\.remark/.test(src)) throw new Error("搜索没有用完整 email");
    return "搜索用原值、展示用脱敏值";
  });

  /* ══════════ 会话能力的平台边界（§3.15 能力边界，2026-10-03 逐文件核实） ══════════ */
  const domTC = await boot("dest=sessions&scope=trae-cn&hint=0");   // Trae 无会话能力
  const dTC = domTC.window.document;
  const domWC = await boot("scope=wb-cn&hint=0");                   // WorkBuddy 有
  const dWC = domWC.window.document;

  check("★ 会话能力只有 WorkBuddy 有：Trae 侧栏不渲染「会话」入口", () => {
    const labels = [...dTC.querySelectorAll("#nav .nav-label")].map((el) => el.textContent.trim());
    if (labels.includes("会话")) throw new Error("Trae 侧栏出现了「会话」入口：" + labels.join("/"));
    if (labels.length !== 5) throw new Error(`Trae 侧栏 ${labels.length} 项，期望 5`);
    return labels.join(" / ");
  });

  check("WorkBuddy 侧栏有「会话」入口（对照，排除「两边都没有」的假通过）", () => {
    const labels = [...dWC.querySelectorAll("#nav .nav-label")].map((el) => el.textContent.trim());
    if (!labels.includes("会话")) throw new Error("WorkBuddy 侧栏缺少「会话」：" + labels.join("/"));
    return labels.join(" / ");
  });

  check("★ Trae 经 URL 直达会话页 → **结构化**空态（图标+标题+说明+出口）+ 说明原因", () => {
    const e = dTC.querySelector(".empty");
    if (!e) throw new Error("没有空态");
    if (!e.querySelector(".ico")) throw new Error("空态缺图标");
    if (!e.querySelector("b")) throw new Error("空态缺标题");
    if (!e.querySelector("p")) throw new Error("空态缺说明");
    const txt = String(e.textContent).replace(/\s+/g, " ").trim();
    if (!/暂不支持会话管理/.test(txt)) throw new Error("没说清是「不支持」：" + txt);
    if (!/云端归属/.test(txt)) throw new Error("没说明原因（依赖平台自身的会话存储与云端归属）：" + txt);
    return txt;
  });

  check("★ Trae 会话页不列出 WorkBuddy 的会话组（否则用户以为「复制过来了」）", () => {
    if (dTC.querySelectorAll(".sg-card").length) throw new Error("Trae 会话页出现了会话组卡片");
    if (dTC.querySelector(".cli-tabs")) throw new Error("Trae 会话页出现了客户端 Tab");
    return "无会话组、无客户端 Tab";
  });

  /* ══════════ 账号卡片的程序位（§3.3 / 附录 D：客户端清单） ══════════ */
  const domAC = await boot("dest=accounts&hint=0");
  const dAC = domAC.window.document;

  check("★ 账号卡片的程序位与注册表一致（数量 + 逐个 id 对齐）", () => {
    const cards = [...dAC.querySelectorAll(".acct")];
    if (!cards.length) throw new Error("没有账号卡片");
    const declared = domAC.window.eval("PLATFORMS.workbuddy.programs.map((p) => p.id)");
    /* **逐卡片**核对（`/accounts` 下有多张卡 —— 第一次写成"总数"被 8 张 × N 掩盖过） */
    for (const [i, c] of cards.entries()) {
      const ids = [...c.querySelectorAll(".progs .prog")].map((b) => (b.dataset.act || "").split("|")[2]);
      if (ids.length !== declared.length) {
        throw new Error(`第 ${i + 1} 张卡片 ${ids.length} 个程序位，注册表 ${declared.length} 个`);
      }
      if (ids.join(",") !== declared.join(",")) {
        throw new Error(`第 ${i + 1} 张卡片程序位与注册表不符：${ids.join("/")} vs ${declared.join("/")}`);
      }
    }
    return `${cards.length} 张卡片 × ${declared.join("/")}`;
  });

  check("★ 「计划新增」与「本机没装」在 UI 上可区分（措辞不同）", () => {
    const btns = [...dAC.querySelectorAll(".progs .prog")];
    const planned = btns.find((b) => b.classList.contains("planned"));
    if (!planned) throw new Error("注册表声明了 planned 的程序位，卡片上却没有对应样式");
    const t = planned.getAttribute("title") || "";
    if (!/尚未支持/.test(t)) throw new Error("planned 的 tooltip 没说明「尚未支持」：" + t);
    /* 反面：不许用「未检测到客户端」的措辞 —— 那会引导用户去装，而装了也没用 */
    if (/未检测到客户端/.test(t)) throw new Error("planned 误用了「未检测到客户端」的措辞");
    return t;
  });

  /* ══════════ 请求代理（§3.19 / P12，D29 已拍板：默认关 + 绝不劫持） ══════════ */
  const domPX = await boot("dest=gateway&gw=proxy&hint=0");
  const dPX = domPX.window.document;
  const pxByAct = (act) => [...dPX.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const pxFlat = (s) => String(s || "").replace(/\s+/g, " ").trim();
  const pxMain = () => pxFlat(dPX.querySelector(".split-main").textContent);

  check("★ 请求代理：**默认关闭**（D29 拍板）", () => {
    const sw = pxByAct("proxy-toggle");
    if (!sw) throw new Error("没有代理开关");
    if (sw.classList.contains("on")) throw new Error("默认是开启的 —— 违反「默认关」");
    if (!/已关闭/.test(pxMain())) throw new Error("没有「已关闭」状态");
    return "默认关";
  });

  check("★ 关闭态必须**同时**说明「风险」与「不劫持」三条（不能只写一句）", () => {
    const txt = pxMain();
    if (!/流量会经过本机网关/.test(txt)) throw new Error("没说明流量会过网关");
    if (!/不落盘请求正文/.test(txt)) throw new Error("没说明不落盘正文");
    for (const bad of ["绝不劫持", "不改 hosts", "不做 DNS 劫持", "不做透明代理"]) {
      if (!txt.includes(bad)) throw new Error("「不劫持」声明缺一条：" + bad);
    }
    return "风险 + 不劫持三条都在";
  });

  check("★ 开启后：显示拦截方式、白名单与 KPI", () => {
    pxByAct("proxy-toggle").click();
    const main = dPX.querySelector(".split-main");
    const txt = pxFlat(main.textContent);
    if (!/运行中/.test(txt)) throw new Error("没有运行中状态");
    if (!/拦截方式/.test(txt)) throw new Error("没显示拦截方式（D34 的 C 先行要看得见）");
    const want = domPX.window.eval("PROXY.domains.length") + 1;   // +1 = 兜底行
    const rows = [...main.querySelectorAll(".tbl tbody tr")];
    if (rows.length !== want) throw new Error(`白名单 ${rows.length} 行，期望 ${want}`);
    return txt.slice(0, 48);
  });

  check("★ 白名单外**直连**（不是「也代理」）", () => {
    const rows = [...dPX.querySelectorAll(".split-main .tbl tbody tr")];
    const txt = pxFlat(rows[rows.length - 1].textContent);
    if (!/其它全部域名/.test(txt)) throw new Error("缺兜底行：" + txt);
    if (!/直连/.test(txt)) throw new Error("兜底行不是「直连」：" + txt);
    return txt;
  });

  check("★ 一键还原：回到关闭态（且不留残留）", () => {
    pxByAct("proxy-restore").click();
    const sw = pxByAct("proxy-toggle");
    if (sw.classList.contains("on")) throw new Error("还原后开关仍是开的");
    if (!/已关闭/.test(pxMain())) throw new Error("没回到关闭态");
    return "已还原";
  });

  /* ── 报告 ── */
  } catch (e) {
    results.push({ name: "核实序列完整执行", ok: false, detail: e.message });
  }
  const pass = results.filter((r) => r.ok).length;
  const fail = results.length - pass;
  for (const r of results) console.log(`${r.ok ? "  ok  " : " FAIL "} ${r.name}${r.detail && !r.ok ? "  → " + r.detail : ""}`);
  console.log(`\n${pass} passed / ${fail} failed`);

  if (MUT) {
    console.log(fail > 0
      ? "\n变异测试通过：断言在缺陷下确实变红。"
      : "\n变异测试失败：断言空转 —— 注入缺陷后仍全绿。");
    process.exit(fail > 0 ? 0 : 1);
  }
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => { console.error("核实脚本自身异常：", e); process.exit(3); });
