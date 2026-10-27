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
 *   node docs/prototype/verify-behaviors.cjs
 *
 * 变异模式（证明断言不是恒真的空转）：
 *   node docs/prototype/verify-behaviors.cjs --mutate=merge|snapshot|selectall
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
  /* issue #109：切到网关时**跳过确认**、直接改状态 —— 确认框成了装饰。
     期望「切到网关**先弹确认**」那条红（它同时断言「确认前状态不能变」）。 */
  cliConfirm: {
    desc: "切到网关不弹确认，直接改状态",
    file: "demo-app.js",
    from: 'if (a1 === "gateway") { drawerCliConfirm("gateway"); break; }',
    to: 'if (a1 === "gateway") { state.cli.mode = "gateway"; renderContent(); break; } /* MUTATED: 跳过确认 */',
  },
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
  /* ── 升级前说明弹窗（版本内容）── */
  updauto: {
    desc: "打开弹窗就开始下载（退回「没读清楚就装」）",
    file: "demo-app.js",
    from: '  openUpdateDialog("available");\n}',
    to: '  openUpdateDialog("available");\n  startUpdateDownload(); /* MUTATED: 打开即下载 */\n}',
  },
  updorder: {
    desc: "把「升级前必读」挪到更新列表之后（等于没置顶）",
    file: "demo-app.js",
    from: "${updStageHtml()}${updBreakingHtml()}${updNotesHtml()}",
    to: "${updStageHtml()}${updNotesHtml()}${updBreakingHtml()} /* MUTATED: 必读沉底 */",
  },
  permretry: {
    desc: "退回 perm-retry 死按钮（处理器读错参数位：a1 → a2）",
    file: "demo-app.js",
    /* 锚**处理器**而不是渲染端：原缺陷的成因是「渲染只带一参、处理器却读 a2」，
       往渲染端加尾参不会让 a1 失效 ⇒ 变异不成立。 */
    from: "const a = curScope().accounts.find((x) => x.id === a1);",
    to: "const a = curScope().accounts.find((x) => x.id === a2); /* MUTATED: 读错参数位 */",
  },
  /* ★ P14 S4a · 把「未检测到客户端」的**独立空态**去掉 ⇒ 它会被下面的「没有会话」分支吃掉，
     于是「没装 / 没登录」被说成「你确实没有会话」。 */
  extmissing: {
    desc: "去掉「未检测到客户端」的独立空态（把没装说成没有会话）",
    file: "demo-app.js",
    from: "  if (!view.dataRoot) {\n    return `<div class=\"dsec\" data-ext-state=\"missing\">",
    to: "  if (false) {\n    return `<div class=\"dsec\" data-ext-state=\"missing\">",
  },
  progbind: {    desc: "退回单绑定模型（切一个程序位会清掉本账号其他程序位）",
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
  /* ★ 密钥三态动作（2026-10-14）：把「恢复」退回「只弹 toast、不改数据」——
     这正是「吊销后实际只剩删除一条出路」的历史缺陷（用户撤销一次误吊销就得删掉重建）。 */
  keyrestore: {
    desc: "「恢复」失效：只弹 toast、键仍是吊销态（等于吊销后只剩删除）",
    file: "demo-app.js",
    from: 'onOk: () => { key.revoked = false; renderContent();',
    to: 'onOk: () => { /* MUTATED: 恢复不生效 */',
  },
  /* ★ 抽屉与菜单的分工退回「两处都有」：把手动签到又塞回抽屉。 */
  drawerdup: {
    desc: "手动签到在抽屉与菜单各有一个入口（重复入口回来了）",
    file: "demo-app.js",
    from: '<div class="kv"><span class="k">分组</span>',
    to: '<button class="btn sm" data-act="act|checkin|${a.id}">手动签到</button><div class="kv"><span class="k">分组</span>',
  },
  /* ★ 进度条画成「已用占比」：条越长越穷，正是最容易画反的一处。 */
  pkbar: {
    desc: "资源包进度条画成已用占比（条越长越穷）",
    file: "demo-app.js",
    from: 'style="width:${pct.toFixed(1)}%"',
    to: 'style="width:${(100 - pct).toFixed(1)}%" /* MUTATED */',
  },
  /* ★ 染色按用量而不是按到期紧迫度（没动过的临期包反而不提醒）。 */
  pkcolor: {
    desc: "资源包颜色按用量染（临期但没动过的包不再提醒）",
    file: "demo-app.js",
    from: "        const soon = p.expireAt ? p.expireAt - now <= 7 * 86400000 : p.expireInDays <= 7;",
    to: "        const soon = p.total > 0 && p.remaining / p.total < 0.5; /* MUTATED: 按用量染色 */",
  },
  /* ★ 临期积分与资源包脱钩（手写字段，回到两个数字打架的状态）。 */
  pkexpiring: {
    desc: "临期积分写死为 0，与资源包明细脱钩",
    file: "demo-data.js",
    from: "  a.expiring = _soon.reduce((sum, p) => sum + p.remaining, 0);",
    to: "  a.expiring = 0; /* MUTATED: 与包明细脱钩 */",
  },
  /* ★ 分组只改 UI 不写回（假选择器）。 */
  pkgroup: {
    desc: "分组选择器只改控件、不写回数据（假选择器）",
    file: "demo-app.js",
    from: "  a.group = group;\n",
    to: "  /* MUTATED: 不写回 */\n",
  },
  /* ★ 指标块上的刷新与菜单不同源（成了第二套实现）。 */
  drawertiles: {
    desc: "指标块的 ⟳ 用另一套 data-act（刷新在抽屉里变成第二套实现）",
    file: "demo-app.js",
    from: 'data-act="acct-refresh|${a.id}|${kind}"',
    to: 'data-act="acct-refresh|${a.id}"',
  },
  /* ★ 抽屉退回长滚动：去掉 Tab，内容平铺（凭据页的深操作随之不可达）。 */
  drawerflat: {
    desc: "抽屉去掉 Tab（回到 7 段长滚动，深操作找不到）",
    file: "demo-app.js",
    from: "    ${drawerTabsNav(a, tab)}",
    to: "    ",
  },
  /* ★ 四类刷新：退回「一个进行中标记」= 点一项四项一起转圈（等于没拆）。 */
  acctrefresh: {
    desc: "四项刷新共用一个进行中标记（拆成四项退化成「一起转圈」）",
    file: "demo-app.js",
    from: "  state.refreshing.add(key);",
    to: "  state.refreshing.add(`${id}:credits`).add(`${id}:tokens`); /* MUTATED: 不分项 */",
  },
  /* ★ 凭据状态退回「跟着积分一起失败」= 用户永远分不清挂在哪一层。 */
  acctcredential: {
    desc: "凭据状态跟着积分一起判失败（拆成四项失去意义）",
    file: "demo-app.js",
    from: 'if (kind === "credential") {',
    to: 'if (false) { /* MUTATED: 凭据状态不再单独成功 */',
  },
  /* ★ Qoder（第三个平台）—— 两条数据层变异。 */
  qodertotal: {
    desc: "Qoder 国内版今日 Token 与账号合计脱钩（days 末值不再自洽）",
    file: "demo-data.js",
    from: '        lastUsed: "3 分钟前", todayTokens: 38400, totalTokens: 512000,',
    to: '        lastUsed: "3 分钟前", todayTokens: 40000, totalTokens: 512000, /* MUTATED */',
  },
  qoderlabel: {
    desc: "备份页作用域名退回 `SCOPES[id].label`（既有缺陷复现：每行都没名字）",
    file: "demo-data.js",
    from: "  label: scopeDisplayName(id),",
    to: "  label: SCOPES[id].label, /* MUTATED */",
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
    /* 期望值**写死**：拿 `days` 自己算期望等于用被测物证明被测物（恒真）。
       ⚠️ 新增作用域必须在此登记 —— 少一条这里就少守一个作用域。 */
    const expect = { "wb-cn": 128700, "wb-global": 18400, "trae-cn": 96200, "trae-global": 0, "qoder-cn": 56200, "qoder-global": 25200 };
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
    const alive = ["wb-cn", "wb-global", "trae-cn", "trae-global", "qoder-cn", "qoder-global"].filter((id) => ev(`SCOPES[${JSON.stringify(id)}].todayTokens !== undefined`));
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
  /* ★ 2026-10-21 账本改版：原来那张「区间总 Token」只算客户端，名字却像总量（issue #63 的
     核心投诉）⇒ 改名「客户端 Token」。这条校验的本意是「merge 开关真的聚合了逐日数据」，
     所以**锁到客户端账本**（`book=client`）：合计卡在「全部」账本下是
     `客户端 + 网关 − 重叠`，不再等于 6 个作用域的逐日和。 */
  const dom1 = await boot("dest=usage&usage=trend&range=7&book=client&hint=0");
  const w = dom1.window;
  const totalOf = (d) => {
    const t = [...d.querySelectorAll(".stat")].find((n) => n.textContent.includes("客户端 Token"));
    return t ? t.querySelector(".v").textContent.trim() : null;
  };
  const single = totalOf(w.document);
  check("P0-1 合并前：客户端 Token 存在（原「区间总 Token」已按 issue #63 改名）",
    () => single || (() => { throw new Error("读不到客户端 Token"); })());

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
      const ids = ["wb-cn", "wb-global", "trae-cn", "trae-global", "qoder-cn", "qoder-global"];
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

  /* ── P0-3 死按钮 → ★ 升级前说明弹窗（版本内容）──
     用户原话：「每次升级都不知道这个版本的变化内容是什么」。
     这里守的**不是**「点了有反应」，而是四件事：
       ① 先说明、后动作（打开弹窗 ≠ 开始下载）；
       ② 破坏性变更**置顶**，不被长列表淹掉；
       ③ 说清「当前 → 目标」，拿不到说明时如实说；
       ④ 下载中不可关闭（关掉 = 安装半途而废）。 */
  const dom2 = await boot("hint=0");
  const d2 = dom2.window.document;
  const w2 = dom2.window;
  const ev2 = (expr) => w2.eval(expr);
  const box = () => d2.querySelector("#update-overlay");
  const updStage = () => {
    const el = box().querySelector(".upd");
    return el ? el.dataset.updStage : null;
  };
  const clickUpd = (act) => {
    const b = box().querySelector(`[data-act="${act}"]`);
    if (!b) throw new Error(`弹窗里没有 data-act=${act}`);
    b.click();
  };

  check("P0-3「检查更新」不再是死按钮（打开升级前说明弹窗）", () => {
    const b = d2.querySelector('[data-act="check-update"]');
    if (!b) throw new Error("没有 data-act=check-update");
    b.click();
    if (!box().classList.contains("open")) throw new Error("点了没有打开更新弹窗");
    const text = box().textContent;
    if (!/v\d{4}\.\d+\.\d+/.test(text)) throw new Error("弹窗里没有版本号");
    if (!/当前 v/.test(text)) throw new Error("没说清「当前版本」是哪个 —— 用户不知道自己从哪一版升上来");
    if (!/发布于/.test(text)) throw new Error("缺发布日期");
    if (!/MB/.test(text)) throw new Error("缺包大小 —— 判断不了这次要不要现在装");
    return true;
  });

  check("★★ 打开弹窗只说明、不下载（不可逆动作必须发生在读懂之后）", () => {
    if (updStage() !== "available") throw new Error(`打开后 stage=${updStage()}，应为 available`);
    if (box().querySelector("[data-upd-progress]")) throw new Error("还没点「立即升级」就开始下载了");
    if (!box().querySelector('[data-act="upd-start"]')) throw new Error("没有「立即升级」主操作（有说明无出口）");
    return true;
  });

  check("★ 升级前必读**置顶**（破坏性变更不能被长列表淹掉）", () => {
    const brk = box().querySelector("[data-upd-breaking]");
    const notes = box().querySelector("[data-upd-notes]");
    if (!brk) throw new Error("有 breaking 数据却没渲染必读区块");
    if (!notes) throw new Error("没有渲染本版更新分组");
    /* 比**文档顺序**，不是比 innerHTML 里的字符位置 —— 后者对重排不敏感。 */
    if (!(brk.compareDocumentPosition(notes) & w2.Node.DOCUMENT_POSITION_FOLLOWING))
      throw new Error("必读区块排在更新列表之后 —— 等于没置顶");
    if (!/端口/.test(brk.textContent)) throw new Error("必读区块没带具体条目内容");
    return true;
  });

  check("★ 更新条目数 = 数据里的真实条数（不是写死的三行）", () => {
    const expect = ev2("APP_UPDATE.sections.reduce((n, s) => n + s.items.length, 0)");
    const got = box().querySelectorAll("[data-upd-notes] .upd-list li").length;
    if (!expect) throw new Error("数据里没有可渲染的条目（用例前提不成立）");
    if (expect !== got) throw new Error(`数据 ${expect} 条，渲染 ${got} 条`);
    const groups = ev2("APP_UPDATE.sections.length");
    if (box().querySelectorAll("[data-upd-notes] .upd-grp").length !== groups)
      throw new Error("分组数与数据不一致");
    return true;
  });

  check("★ 版本说明按**纯文本**渲染（外部内容不许直接 innerHTML）", () => {
    /* 变异式验证：往数据里塞一个标签，渲染后必须看不到 <img>，但它得原样显示成文字。 */
    ev2(`APP_UPDATE.sections[0].items.push("<img src=x onerror=alert(1)>")`);
    ev2("renderUpdateDialog()");
    if (box().querySelector(".upd-list img")) throw new Error("条目里的标签被当 HTML 执行了 —— 存在注入面");
    if (!/onerror/.test(box().textContent)) throw new Error("条目被吞掉了（应原样当文本显示）");
    ev2("APP_UPDATE.sections[0].items.pop(); renderUpdateDialog()");
    return true;
  });

  check("★「更早的 N 个版本」默认折叠，展开/收起都点得动", () => {
    if (!ev2("APP_UPDATE.older.length")) throw new Error("演示数据里没有更早版本（用例前提不成立）");
    if (box().querySelector("[data-upd-older]")) throw new Error("默认就展开了更早版本（噪声）");
    clickUpd("upd-older");
    const older = box().querySelector("[data-upd-older]");
    if (!older) throw new Error("点了没有展开");
    if (!older.textContent.includes(ev2("APP_UPDATE.older[0].version"))) throw new Error("展开后没有更早版本的版本号");
    clickUpd("upd-older");
    if (box().querySelector("[data-upd-older]")) throw new Error("再点没有收起（开关是单向的）");
    return true;
  });

  check("★★ 下载中不可关闭（关闭按钮禁用 + Esc 无效），且有真实进度", () => {
    clickUpd("upd-start");
    if (updStage() !== "downloading") throw new Error(`点了「立即升级」stage=${updStage()}`);
    const close = box().querySelector('[data-act="upd-close"]');
    if (!close || !close.disabled) throw new Error("下载中关闭按钮仍可点 —— 会让安装半途而废");
    d2.dispatchEvent(new w2.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    if (!box().classList.contains("open")) throw new Error("Esc 在下载中把弹窗关掉了");
    if (!box().querySelector("[data-upd-progress]")) throw new Error("下载中没有进度条");
    const pct = box().querySelector("[data-upd-pct]");
    if (!pct || !/%$/.test(pct.textContent.trim())) throw new Error("进度没有百分比数字");
    return true;
  });

  /* 进度到 100 → ready（原型 9%/200ms ⇒ ≤2.4s，留足余量）。 */
  await sleep(2800);
  check("★ 下载完成转「已就绪」并给重启入口（不是「安装完就没了」）", () => {
    if (updStage() !== "ready") throw new Error(`下载结束 stage=${updStage()}，应为 ready`);
    const ready = box().querySelector("[data-upd-ready]");
    if (!ready) throw new Error("没有「已下载，重启后生效」的交代");
    if (!box().querySelector('[data-act="upd-restart"]')) throw new Error("没有「立即重启」出口");
    if (!box().querySelector('[data-act="upd-later"]')) throw new Error("只剩强制重启 —— 用户可能正在跑任务");
    return true;
  });

  check("★ 关掉弹窗后记住「这版看过了」（自动检查不重复打扰）", () => {
    clickUpd("upd-later");
    if (box().classList.contains("open")) throw new Error("点「稍后重启」没关掉弹窗");
    if (ev2("state.update.seenVersion") !== ev2("APP_UPDATE.latest"))
      throw new Error("没有记住已读版本 —— 下一轮自动检查会再弹一次");
    return true;
  });

  check("★ 拿不到版本说明时**如实说**（不假装「本版没有更新」）", () => {
    ev2("APP_UPDATE.sections = []");
    ev2("openUpdateDialog('available')");
    const empty = box().querySelector("[data-upd-notes-empty]");
    if (!empty) throw new Error("sections 为空时没有给出说明缺失的空态");
    if (!/没有附带说明/.test(empty.textContent)) throw new Error("空态文案没说清「说明缺失」");
    ev2("APP_UPDATE.breaking = []; renderUpdateDialog()");
    if (box().querySelector("[data-upd-breaking]")) throw new Error("breaking 为空时仍渲染了必读区块（恒显噪声）");
    ev2("closeUpdateDialog()");
    return true;
  });

  /* 失败分支走深链（与 gateway / usage 的错误态同一套约定）。 */
  const dom2b = await boot("hint=0&open=update&upd=error");
  const d2b = dom2b.window.document;
  check("★ 检查更新失败要**说清原因 + 给重试**，不是一句「更新失败」", () => {
    const el = d2b.querySelector("#update-overlay .upd");
    if (!el || el.dataset.updStage !== "error") throw new Error("?upd=error 没有落到失败态");
    const err = d2b.querySelector("[data-upd-error]");
    if (!err) throw new Error("没有失败说明区块");
    if (!/代理/.test(err.textContent)) throw new Error("没给可操作的原因（代理 / 手动下载）");
    if (!d2b.querySelector('[data-act="upd-retry"]')) throw new Error("没有重试入口");
    if (!d2b.querySelector('[data-act="upd-release"]')) throw new Error("没有「打开下载页」的退路");
    return true;
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

  /* B5 · 签到积分按作用域取（WB=100 / Trae=50）
     ⚠️ 签到历史现在在抽屉的「签到」Tab 里（v2 重设计把它从长滚动里拆出来），
     所以要先切 Tab 再读正文 —— 断言的是「积分按作用域取」，不是「默认页签有表格」。 */
  const gotoTab = (doc, label) => {
    const tab = [...doc.querySelectorAll(".drawer-tabs button")].find((b) => b.textContent.trim().startsWith(label));
    if (!tab) throw new Error("抽屉里没有「" + label + "」Tab");
    tab.click();
    return doc.querySelector("#drawer-body").textContent;
  };
  check("B5 签到积分按作用域取值（Trae=50，不是硬编码 100）", () => {
    const t = gotoTab(domT.window.document, "签到");
    if (!t.includes("+50")) throw new Error("Trae 抽屉里没有 +50 签到记录");
    if (/\+\s*100/.test(t) && !t.includes("+50")) throw new Error("疑似硬编码 100");
    return true;
  });
  check("B5 WorkBuddy 签到积分 = +100", () => {
    const t = gotoTab(domB.window.document, "签到");
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
  /* ★ 2026-10-21 账本改版：趋势图的堆叠维度从「模型」换成「账本」（客户端 / 网关）。
     按模型拆的每日走势挪到「模型」tab（`ledgerModelDaily`）—— 两处各答一个问题，
     否则两级堆叠在深色小图里读不出来。 */
  check("用量·趋势：堆叠柱的列数 = 天数，且每列分段数 = 账本数（全部=2 / 单账本=1）", () => {
    const cols = q(dT, ".cbar .ccol");
    if (cols.length !== 30) throw new Error("列数 " + cols.length + "，期望 30");
    const segsPerCol = cols.map((c) => c.querySelectorAll("i").length);
    const bad = segsPerCol.filter((n) => n !== 2);
    if (bad.length) throw new Error(`「全部」账本下每列应为 2 段（客户端 + 网关），实际 ${[...new Set(bad)].join("/")}`);
    dT.querySelector('[data-act="usage-book|client"]').click();
    const one = q(dT, ".cbar .ccol").map((c) => c.querySelectorAll("i").length);
    if (one.some((n) => n !== 1)) throw new Error("客户端账本下每列应只剩 1 段");
    dT.querySelector('[data-act="usage-book|all"]').click();
    return `${cols.length} 列 × 2 段（客户端账本 → 1 段）`;
  });
  check("用量·趋势：堆叠柱各分段高度之和 ≈ 当日两本账合计（图不能与数据打架）", () => {
    const cols = q(dT, ".cbar .ccol");
    const info = domT2.window.eval(`(() => {
      const L = USAGE[state.scope].ext.ledger;
      const i = L.client.days.length - 1;
      const max = Math.max(...L.client.days.map((v, k) => v + L.gateway.days[k]));
      return { total: L.client.days[i] + L.gateway.days[i], max };
    })()`);
    const sumLastCol = [...cols[cols.length - 1].querySelectorAll("i")]
      .reduce((s, i) => s + parseFloat(i.style.height || "0"), 0);
    const expect = (info.total / info.max) * 100;
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
  check("用量·趋势：模型占比**按单本账**画（全部账本 = 两个环，各自扇区数 = 该账本模型数）", () => {
    const all = q(dT, ".cdonut");
    if (all.length !== 2) throw new Error(`「全部」账本下应有两个环（客户端 / 网关），实际 ${all.length}`);
    const models = domT2.window.eval("USAGE[state.scope].ext.modelDays.length");
    const clientArcs = all[0].querySelectorAll("circle").length;
    if (clientArcs !== models) throw new Error(`客户端环扇区 ${clientArcs} ≠ 模型 ${models}`);
    return `2 环 · 客户端 ${clientArcs} 段`;
  });
  check("用量·趋势：累计曲线图注不再写「峰值 = 末值」这种废话", () => {
    const cap = dT.querySelector(".carea figcaption").textContent;
    if (!cap.includes("累计")) throw new Error("图注不符：" + cap);
    return cap.trim();
  });

  const domM2 = await boot("dest=usage&usage=models&range=30&hint=0");
  const dM = domM2.window.document;
  /* ★ 2026-10-21：模型 tab 从「一个环形 + 一条排行」改成「每本账一张表 + 每日按模型图」。
     理由（issue #63）：两本账的分母不同，合并排行/合并环形都会给出假占比；
     而「按模型看每日走势」这个原趋势图的能力挪到这里，不能丢。 */
  check("用量·模型：每日按模型堆叠（每列分段 = 模型数）+ 每本账一张表", () => {
    const charts = q(dM, ".cbar");
    if (charts.length !== 2) throw new Error(`「全部」账本下应有两张按模型图（客户端 / 网关），实际 ${charts.length}`);
    const models = domM2.window.eval("USAGE[state.scope].ext.modelDays.length");
    const segs = [...charts[0].querySelectorAll(".ccol")].map((c) => c.querySelectorAll("i").length);
    if (segs.some((n) => n !== models)) throw new Error(`客户端按模型图每列应为 ${models} 段，实际 ${[...new Set(segs)].join("/")}`);
    const rows = q(dM, ".tbl tbody tr").length;
    const gatewayModels = domM2.window.eval("USAGE[state.scope].ext.ledger.gateway.models.length");
    if (rows !== models + gatewayModels) throw new Error(`表行 ${rows} ≠ 客户端 ${models} + 网关 ${gatewayModels}`);
    const bars = q(dM, ".lbar i").filter((i) => parseFloat(i.style.width) > 0).length;
    if (bars < rows) throw new Error(`分布条 ${bars} < 表行 ${rows}`);
    return `${charts.length} 图 · ${rows} 行 · ${bars} 条分布`;
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
    /* ★ 2026-10-21：删掉「账号 Token 占比 / 网关状态码分布」两个环 ——
       前者已被「模型」tab 的账本表覆盖，后者已由覆盖条（失败数）+ 网关页覆盖；
       换成「两张积分账本表 + 两个口径的差额」，这才是 issue #63 要回答的问题。 */
    const tables = q(dC, ".tbl").length;
    if (tables < 3) throw new Error(`应有积分账本表 ×2 + 流水表，实际 ${tables} 张表`);
    if (!dC.querySelector(".gapbox")) throw new Error("缺少「两个口径的差额」块");
    return `双向柱 30 · 曲线 ${areas} · 到期 ${bars} · 表 ${tables} · 差额 ✓`;
  });
  check("用量·积分：余额曲线基线不锚 0（否则高位小幅波动会被压成直线）", () => {
    const cap = dC.querySelector(".carea figcaption").textContent;
    if (!cap.includes("非 0")) throw new Error("图注没说明基线不是 0：" + cap);
    return cap.trim();
  });

  /* 合并态必须把**图表数据**也一起聚合，不能只有 days 变 */
  check("用量·合并：合计数字真的变多（mergeExt 聚合了图表数据）", () => {
    const read = () => {
      const t = [...dT.querySelectorAll(".stat")].find((n) => n.textContent.includes("合计 Token"));
      return t ? t.querySelector(".v").textContent.trim() : null;
    };
    const before = read();
    dT.querySelector('[data-act="merge|1"]').click();
    const after = read();
    if (!before || !after || after === before) throw new Error(`合并前 ${before} → 合并后 ${after}，图表数据没聚合`);
    return `${before} → ${after}`;
  });

  /* ══════════ 全局备份与恢复（PLAN §3.14） ══════════ */
  const domBK = await boot("dest=settings&set=backup&hint=0");
  const dB = domBK.window.document;
  const bq = (s) => dB.querySelector(s);
  const bByAct = (act) => [...dB.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const flat = (s) => String(s || "").replace(/\s+/g, " ").trim();

  check("备份页：设置子导航含「备份与恢复」，共 10 项", () => {
    /* 10 = 通用 / 账号与登录 / 签到与积分 / 网关 / 计划任务 / 客户端 / CodeBuddy CLI /
       权限与诊断 / 日志 / 备份与恢复。
       ★ 这一条**故意写死数字**：子导航是设置页的唯一导航，多一项少一项都要有意识
         （issue #109 加「CodeBuddy CLI」时就是靠它提醒改这一行的）。 */
    const items = [...dB.querySelectorAll(".subnav button")].map((b) => b.textContent.trim());
    if (items.length !== 10) throw new Error(`子导航 ${items.length} 项，期望 10 项`);
    if (!items.includes("备份与恢复")) throw new Error("没有「备份与恢复」：" + items.join("/"));
    if (!items.includes("CodeBuddy CLI")) throw new Error("没有「CodeBuddy CLI」：" + items.join("/"));
    return items.join(" / ");
  });

  check("备份页：默认勾选全部 6 个作用域（跨作用域一次导出）", () => {
    const rows = [...dB.querySelectorAll(".bk-export-list .bk-row")];
    if (rows.length !== 6) throw new Error(`${rows.length} 行，期望 6 行`);
    const off = rows.filter((r) => !r.querySelector(".check").classList.contains("on"));
    if (off.length) throw new Error(`${off.length} 个作用域默认未勾选`);
    return "6/6 默认全选";
  });

  check("★ 备份页：每个作用域行都有名字（不是空串）", () => {
    /* 曾经的缺陷：`BACKUP_SECTIONS` 取 `SCOPES[id].label`，而 SCOPES 的条目**只有**
       `platform` + `region`（没有 `label`）⇒ `esc(undefined)` 归成空串 ⇒
       每行都没名字、`aria-label` 也退化成「全选 」。这条断言钉死
       「名字非空，且能读出平台 + 区域」。 */
    const labels = [...dB.querySelectorAll(".bk-export-list .bk-row")]
      .map((r) => r.querySelector(".bk-scope").textContent.trim());
    const blank = labels.filter((l) => !l);
    if (blank.length) throw new Error(`${blank.length}/${labels.length} 行作用域名是空的`);
    for (const want of ["WorkBuddy 国内版", "TraeWork 国际版", "Qoder 国内版", "Qoder 国际版"]) {
      if (!labels.includes(want)) throw new Error(`缺少「${want}」：${labels.join(" / ")}`);
    }
    return labels.join(" / ");
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
    for (const s of ["wb-cn", "wb-global", "trae-cn", "trae-global", "qoder-cn", "qoder-global"]) bByAct2("bk-scope|" + s).click();
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

  /* ══════════ ★ 密钥三态动作：吊销 / 恢复 / 删除（2026-10-14） ══════════
     红线：**已吊销的密钥必须同时给出「恢复」与「删除」两条出口**。
     吊销只置标记（明文未变）⇒ 恢复是把**同一把** Key 拿出来继续用；只留「删除」
     等于逼用户为一次误吊销删掉重建、再把所有客户端配置改一遍。
     断言全建在**真渲染出来的 DOM** 上，且每个正向断言都配一条阴性对照
     （「恢复」不能是恒显按钮、「取消」不能只是装饰）。 */
  const domK = await boot("dest=gateway&gw=keys&hint=0");
  const dK = domK.window.document;
  const kRows = () => [...dK.querySelectorAll(".split-main .tbl tbody tr")];
  const kActs = (tr) => [...tr.querySelectorAll("[data-act]")].map((el) => el.dataset.act);
  const kRowOf = (name) => kRows().find((tr) => tr.textContent.includes(name));
  /** 点某行的某个动作，并确认弹出确认框后按 `ok` / `cancel` 收尾。 */
  const kClick = (name, act, which) => {
    const tr = kRowOf(name);
    if (!tr) throw new Error(`找不到「${name}」那一行`);
    const btn = tr.querySelector(`[data-act="${act}"]`);
    if (!btn) throw new Error(`「${name}」行里没有 ${act}`);
    btn.click();
    const node = dK.querySelector(`[data-act="confirm-${which}"]`);
    if (!node) throw new Error(`点 ${act} 没弹出确认框（找不到 confirm-${which}）`);
    node.click();
  };

  check("★ 密钥表有「状态」列，已吊销行**同时**有「恢复」与「删除」两个出口", () => {
    const tr = kRowOf("CI 自动化");
    if (!tr) throw new Error("找不到演示里那把已吊销的密钥行");
    if (!/已吊销/.test(tr.textContent)) throw new Error("该行没标「已吊销」：" + tr.textContent.trim().slice(0, 40));
    const acts = kActs(tr);
    if (!acts.includes("key-restore|k2")) throw new Error("已吊销行缺「恢复」：" + acts.join(","));
    if (!acts.includes("key-delete|k2")) throw new Error("已吊销行缺「删除」：" + acts.join(","));
    if (acts.includes("revoke|k2")) throw new Error("已吊销行不该再出现「吊销」");
    return acts.join(" · ");
  });

  check("★ 阴性对照：启用行只有「吊销」，**没有**「恢复 / 删除」", () => {
    const tr = kRowOf("默认 Key");
    if (!tr) throw new Error("找不到启用态的密钥行");
    if (!/启用/.test(tr.textContent)) throw new Error("该行没标「启用」：" + tr.textContent.trim().slice(0, 40));
    const acts = kActs(tr);
    if (!acts.includes("revoke|k1")) throw new Error("启用行缺「吊销」：" + acts.join(","));
    if (acts.some((a) => a.startsWith("key-restore"))) throw new Error("启用行竟然有「恢复」：" + acts.join(","));
    if (acts.some((a) => a.startsWith("key-delete"))) throw new Error("启用行竟然有「删除」：" + acts.join(","));
    return acts.join(" · ");
  });

  check("★ 「取消」不改数据（确认框是闸门，不是装饰）", () => {
    const before = kRows().length;
    kClick("CI 自动化", "key-restore|k2", "cancel");
    const tr = kRowOf("CI 自动化");
    if (!/已吊销/.test(tr.textContent)) throw new Error("取消后却被恢复了 —— 闸门失效");
    if (kRows().length !== before) throw new Error("取消却改了列表");
    return "取消无副作用";
  });

  check("★ 恢复是**可逆**的：确认后该行回到「启用」（不是只弹个 toast）", () => {
    kClick("CI 自动化", "key-restore|k2", "ok");
    const tr = kRowOf("CI 自动化");
    if (!/启用/.test(tr.textContent)) throw new Error("恢复后仍是已吊销：" + tr.textContent.trim().slice(0, 40));
    const acts = kActs(tr);
    if (!acts.includes("revoke|k2")) throw new Error("恢复后没回到启用态的动作集：" + acts.join(","));
    if (acts.some((a) => a.startsWith("key-restore"))) throw new Error("恢复后仍有「恢复」按钮");
    return "已吊销 → 启用";
  });

  check("★ 吊销后**立刻**多出「恢复 + 删除」两条出口（同一把 Key，不需要重建）", () => {
    kClick("默认 Key", "revoke|k1", "ok");
    const tr = kRowOf("默认 Key");
    if (!/已吊销/.test(tr.textContent)) throw new Error("吊销后该行没标「已吊销」");
    const acts = kActs(tr);
    if (!acts.includes("key-restore|k1") || !acts.includes("key-delete|k1"))
      throw new Error("吊销后没给出「恢复 + 删除」：" + acts.join(","));
    return acts.join(" · ");
  });

  check("★ 「删除」只清记录：确认后该行消失，且不牵动别的行", () => {
    const before = kRows().length;
    kClick("默认 Key", "key-delete|k1", "ok");
    if (kRowOf("默认 Key")) throw new Error("删除后该行仍在");
    if (kRows().length !== before - 1) throw new Error(`删除后行数 ${kRows().length}，期望 ${before - 1}`);
    return `${before} → ${kRows().length}`;
  });

  /* 两平台同形：Trae 作用域也有一条已吊销的密钥，出口必须一模一样。 */
  const domKT = await boot("dest=gateway&gw=keys&scope=trae-cn&hint=0");
  const dKT = domKT.window.document;
  check("★ 两平台同形：Trae 作用域的已吊销行同样给「恢复 + 删除」", () => {
    const tr = [...dKT.querySelectorAll(".split-main .tbl tbody tr")].find((r) => /已吊销/.test(r.textContent));
    if (!tr) throw new Error("Trae 作用域没有「已吊销」的行（演示数据丢了？）");
    const acts = [...tr.querySelectorAll("[data-act]")].map((el) => el.dataset.act);
    if (!acts.includes("key-restore|k6")) throw new Error("Trae 已吊销行缺「恢复」：" + acts.join(","));
    if (!acts.includes("key-delete|k6")) throw new Error("Trae 已吊销行缺「删除」：" + acts.join(","));
    if (!/TraeCode Key/.test(tr.textContent)) throw new Error("定位到的不是预期那一行：" + tr.textContent.trim().slice(0, 40));
    return acts.join(" · ");
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
  /* ── ★★ P14 S4a · 插件侧「复制会话」 ──
     与上面那套「会话副本组」**不是一回事**：副本组有账本、可双向同步、有「谁落后 / 谁分叉」；
     插件侧是**一次性单向复制**（副本恒取新 id）⇒ 没有同步 / 统一，也**不进**会话页的客户端 Tab。 */
  const domExt = await boot("dest=accounts&hint=0");
  const dExt = domExt.window.document;
  const evExt = (expr) => domExt.window.eval(expr);

  const extAcc = evExt("(curScope().accounts.find((a) => a.programs.ide || a.programs.vscode) || {}).id");
  check("★ S4a 入口只在**绑定了插件程序位**的账号上出现", () => {
    if (!extAcc) throw new Error("演示数据里没有绑定插件程序位的账号（夹具失效）");
    evExt(`drawerAccount(${JSON.stringify(extAcc)})`);
    const btn = [...dExt.querySelectorAll("[data-act]")]
      .find((n) => n.dataset.act.startsWith("ext-sess|open|"));
    if (!btn) throw new Error("插件程序位的账号上找不到「复制插件会话」入口");
    /* 阴性对照：**没绑定**插件程序位的账号上不该有这个入口 —— 否则它就是个恒显的假入口。 */
    const otherAcc = evExt(
      "(curScope().accounts.find((a) => !a.programs.ide && !a.programs.vscode && a.id !== " +
      JSON.stringify(extAcc) + ") || {}).id"
    );
    if (!otherAcc) throw new Error("演示数据里没有「未绑定插件程序位」的账号（阴性对照缺失）");
    evExt(`drawerAccount(${JSON.stringify(otherAcc)})`);
    if ([...dExt.querySelectorAll("[data-act]")].some((n) => n.dataset.act.startsWith("ext-sess|open|")))
      throw new Error("未绑定插件程序位的账号上也出现了该入口（假入口）");
    return true;
  });

  /* 打开插件会话抽屉：**必须先渲染账号详情抽屉、再点那个入口按钮** ——
     `drawerAccount` 只画账号详情，插件会话抽屉是它里面的一个动作。 */
  const openExt = () => {
    evExt(`drawerAccount(${JSON.stringify(extAcc)})`);
    const btn = [...dExt.querySelectorAll("[data-act]")]
      .find((n) => n.dataset.act.startsWith("ext-sess|open|"));
    if (!btn) throw new Error("找不到「复制插件会话」入口");
    btn.click();
  };

  check("★ S4a 默认全选；取消全选后主操作**禁用**（无选中不给出口）", () => {
    openExt();
    const rows = () => [...dExt.querySelectorAll(".ext-row input")];
    const primary = () => dExt.querySelector('[data-act="ext-sess|copy"]');
    if (rows().length === 0) throw new Error("抽屉里没有会话行（夹具失效）");
    if (!rows().every((n) => n.checked)) throw new Error("默认应全选（复制是加法，默认带走全部）");
    if (primary().disabled) throw new Error("默认全选时主操作不该禁用");
    /* 逐个取消 ⇒ 主操作必须禁用，并给出原因。
       ⚠️ **每点一次都要重新取节点**：`renderExtDrawer` 会整块重画抽屉，
       旧的 NodeList 里的节点已经脱离文档 ⇒ 只点得动第一个（本用例初版就这么错的）。 */
    for (let guard = 0; guard < 20; guard++) {
      const next = [...dExt.querySelectorAll(".ext-row input")].find((n) => n.checked);
      if (!next) break;
      next.click();
    }
    if (dExt.querySelector(".ext-row input:checked")) throw new Error("没能取消掉全部勾选（用例自身失效）");
    if (!primary().disabled) throw new Error("无选中时主操作必须禁用");
    if (!dExt.querySelector("#drawer-body").textContent.includes("先勾选"))
      throw new Error("禁用时没有说明原因（禁用而不给解释）");
    return true;
  });

  check("★★ S4a「未检测到客户端」与「这个账号没有会话」是**两段不同文案**", () => {
    openExt();
    evExt('state.extVariant = "missing"; renderExtDrawer();');
    const missing = dExt.querySelector('[data-ext-state="missing"]');
    if (!missing) throw new Error("dataRoot=null 时没有渲染「未检测到」空态");
    if (!missing.textContent.includes("未检测到"))
      throw new Error("「未检测到客户端」的文案没说实话：" + missing.textContent.trim().slice(0, 40));

    evExt('state.extVariant = "empty"; renderExtDrawer();');
    const empty = dExt.querySelector('[data-ext-state="empty"]');
    if (!empty) throw new Error("装了客户端但账号没会话时没有渲染对应的空态");
    if (empty.textContent.trim() === missing.textContent.trim())
      throw new Error("两种空态文案相同 —— 把「没装 / 没登录」说成了「你没有会话」");
    if (!empty.textContent.includes("数据目录已找到"))
      throw new Error("「没有会话」时应给出数据目录作为可核对的事实");
    return true;
  });

  check("★ S4a 读不出来的工作区索引要**如实标注**（不假装全列出来了）", () => {
    openExt();
    evExt('state.extVariant = "ok"; renderExtDrawer();');
    const warn = dExt.querySelector("[data-ext-skipped]");
    if (!warn) throw new Error("skipped>0 时没有如实标注");
    if (!warn.textContent.includes("1")) throw new Error("标注里没带上真实条数");
    /* 阴性对照：skipped=0 时不该出现这行（否则是恒显的噪声）。 */
    evExt("EXT_SESSIONS.ok.skipped = 0; renderExtDrawer();");
    if (dExt.querySelector("[data-ext-skipped]")) throw new Error("skipped=0 时仍在标注（恒显噪声）");
    evExt("EXT_SESSIONS.ok.skipped = 1;");
    return true;
  });

  /* ── 卡片「更多」菜单：四类数据**各自**刷新（原型 · 待确认）──
     用户原话：「拆分成积分、Token、签到、凭据状态四个刷新，放入更多中」。
     守五件事：
       ① 「更多」打开的是**真菜单**（不是一句 toast），四类数据一项一条；
       ② 点其中一项只刷**这一项**（其余三项仍可点，不变成全量刷新）；
       ③ 进行中 / 完成在菜单里看得见（「刷新中…」→「已更新」），菜单不关；
       ④ 结果如实：凭据失效的账号刷积分**报失败**，而刷凭据状态**成功**并说出已过期 ——
          这正是「拆成四项」的意义：知道挂在哪一层；
       ⑤ 点菜单里的说明行不会顺带打开账号抽屉。 */
  const domR = await boot("dest=accounts&hint=0");
  const dR = domR.window.document;
  const wR = domR.window;
  const cardsR = () => [...dR.querySelectorAll(".acct")];
  const moreOf = (card) => card.querySelector('[data-act^="acct-more|"]');
  const idOf = (card) => moreOf(card).dataset.act.split("|")[1];
  const accOf = (id) => wR.eval(`curScope().accounts.find((x) => x.id === ${JSON.stringify(id)})`);
  const openMenu = (id) => {
    const card = cardsR().find((c) => idOf(c) === id);
    if (!card) throw new Error("找不到账号卡：" + id);
    /* 已经是这张卡的菜单开着就不要再点 —— 「更多」是开关，多点一次会把它关掉。 */
    if (wR.eval("state.acctMenu") !== id) moreOf(card).click();
    return card;
  };
  const itemOf = (id, kind) => dR.querySelector(`[data-act="acct-refresh|${id}|${kind}"]`);
  const KINDS = ["credits", "tokens", "checkin", "credential"];

  check("★ 每张卡都有「更多」，点开是**真菜单**而不是一句 toast", () => {
    const cards = cardsR();
    if (cards.length < 2) throw new Error("账号页卡片不足，用例前提不成立");
    const missing = cards.filter((c) => !moreOf(c));
    if (missing.length) throw new Error(`${missing.length}/${cards.length} 张卡没有「更多」`);
    openMenu(idOf(cards[0]));
    const menu = dR.querySelector(".acct-menu");
    if (!menu) throw new Error("点了「更多」没有出现菜单");
    /* 菜单是 body 级浮层（与 Radix DropdownMenu 同构）：塞进卡片会被 `#content` 的
       `overflow:auto` 裁掉 —— 四项刷新的菜单比卡片还高。 */
    if (menu.closest(".acct")) throw new Error("菜单被塞回了卡片里（会被滚动容器裁掉）");
    if (dR.querySelector("#toasts .toast")) throw new Error("点「更多」还是在弹 toast（菜单没做出来）");
    return true;
  });

  check("★ 菜单里是**四个分开的刷新**（积分 / Token / 签到 / 凭据），不是一个「刷新全部」", () => {
    const items = [...dR.querySelectorAll('.acct-menu .am-item[data-act^="acct-refresh|"]')];
    if (items.length !== 4) throw new Error(`刷新项 ${items.length} 个，应为 4 个`);
    const kinds = items.map((n) => n.dataset.act.split("|")[2]).join(",");
    if (kinds !== KINDS.join(",")) throw new Error("四类数据没有一一对应：" + kinds);
    const labels = items.map((n) => n.textContent.replace(/\s+/g, ""));
    const want = ["刷新积分", "刷新今日Token", "刷新签到状态", "刷新凭据状态"];
    const bad = labels.filter((l, i) => !l.startsWith(want[i]));
    if (bad.length) throw new Error("菜单项名称不对：" + labels.join(" / "));
    return kinds;
  });

  /* 挑一个**能刷成功**的账号（凭据失效的留给后面那条用例）。 */
  const okCard = cardsR().find((c) => {
    const a = accOf(idOf(c));
    return a && !a.expired && !a.needsRelogin;
  });
  if (!okCard) throw new Error("演示数据里没有可正常刷新的账号（用例前提不成立）");
  const okId = idOf(okCard);
  const okName = accOf(okId).name;
  openMenu(okId);
  itemOf(okId, "credits").click();

  check("★ 点「刷新积分」只让**这一项**进入刷新中（其余三项不受影响）", () => {
    const credits = itemOf(okId, "credits");
    if (!credits) throw new Error("点完「刷新积分」项不见了");
    if (!credits.disabled) throw new Error("进行中仍可重复点（会并发打上游）");
    if (!/刷新中/.test(credits.textContent)) throw new Error("没有进行中文案（点了没反应）");
    if (!credits.querySelector(".spinner")) throw new Error("进行中没有转圈");
    const busy = wR.eval("Array.from(state.refreshing)");
    if (busy.length !== 1 || busy[0] !== `${okId}:credits`)
      throw new Error("刷新的范围不止「这个账号的积分」：" + JSON.stringify(busy));
    const frozen = KINDS.filter((k) => k !== "credits" && itemOf(okId, k).disabled);
    if (frozen.length) throw new Error("另外三项也被禁用了（没拆开）：" + frozen.join(","));
    if (!dR.querySelector(".acct-menu")) throw new Error("点一项就把菜单关了（进行中 / 已更新看不见）");
    if (wR.eval("state.selected.size") !== 0) throw new Error("点刷新顺手选中了账号");
    if (dR.querySelector(".drawer.open")) throw new Error("点刷新顺手打开了抽屉");
    return true;
  });

  await sleep(800);

  check("★ 刷新积分完成：该项显示「已更新」并恢复可点，toast 带上积分值", () => {
    const credits = itemOf(okId, "credits");
    if (!credits) throw new Error("刷新后菜单项不见了");
    if (credits.disabled) throw new Error("完成后仍禁用");
    if (!/已更新/.test(credits.textContent)) throw new Error("完成态没有提示（用户不知道刷完没）");
    const toast = [...dR.querySelectorAll("#toasts .toast")].pop();
    if (!toast) throw new Error("没有任何结果提示（静默完成）");
    if (!toast.classList.contains("ok")) throw new Error("正常账号却报了失败");
    const text = toast.textContent;
    if (!text.includes(okName)) throw new Error("提示里没有账号名：" + text);
    if (!/积分/.test(text) || !/\d/.test(text)) throw new Error("只说刷完了却没给数值：" + text);
    return true;
  });

  /* 另外三项各点一次：确认它们**各自**给的是自己那份数据（不是一份数据糊弄三项）。 */
  const wantInToast = { tokens: /今日/, checkin: /签到/, credential: /凭据/ };
  for (const kind of ["tokens", "checkin", "credential"]) {
    itemOf(okId, kind).click();
    await sleep(600);
    check(`★ 刷「${kind}」给的是它自己那份数据`, () => {
      const toast = [...dR.querySelectorAll("#toasts .toast")].pop();
      if (!toast) throw new Error("没有任何结果提示");
      if (!toast.classList.contains("ok")) throw new Error("正常账号却报了失败：" + toast.textContent);
      if (!wantInToast[kind].test(toast.textContent)) throw new Error("提示里不是这一类数据：" + toast.textContent);
      return true;
    });
  }

  /* 诚实边界：凭据失效的账号，四项里只有「凭据状态」能给出答案。 */
  const badCard = cardsR().find((c) => {
    const a = accOf(idOf(c));
    return a && (a.expired || a.needsRelogin);
  });
  if (!badCard) throw new Error("演示数据里没有凭据失效的账号（用例前提不成立）");
  const badId = idOf(badCard);
  openMenu(badId);
  itemOf(badId, "credits").click();
  await sleep(800);
  check("★ 凭据失效的账号：刷积分**报失败**并指出先重新登录", () => {
    const toast = [...dR.querySelectorAll("#toasts .toast")].pop();
    if (!toast) throw new Error("没有任何结果提示");
    if (!toast.classList.contains("err")) throw new Error("凭据已失效却报了成功 —— 谎报");
    if (!/重新登录/.test(toast.textContent)) throw new Error("失败提示没说清下一步：" + toast.textContent);
    return true;
  });

  itemOf(badId, "credential").click();
  await sleep(500);
  check("★★ 同一账号刷「凭据状态」却成功（拆成四项的意义：知道挂在哪一层）", () => {
    const toast = [...dR.querySelectorAll("#toasts .toast")].pop();
    if (!toast) throw new Error("没有任何结果提示");
    if (!toast.classList.contains("ok")) throw new Error("凭据状态不该跟着积分一起失败：" + toast.textContent);
    if (!/过期|失效|重新登录/.test(toast.textContent)) throw new Error("没读出「凭据已过期」这个事实：" + toast.textContent);
    return true;
  });

  check("★ 点菜单里的说明行不会顺带打开账号抽屉", () => {
    const note = dR.querySelector(".acct-menu .am-note");
    if (!note) throw new Error("菜单里没有说明行");
    note.click();
    if (dR.querySelector(".drawer.open")) throw new Error("点菜单空白处把抽屉打开了（菜单没拦住卡片点击）");
    return true;
  });

  check("★ 再点「更多」能收起菜单（开关是双向的）", () => {
    const card = cardsR().find((c) => idOf(c) === badId);
    moreOf(card).click();
    if (dR.querySelector(".acct-menu")) throw new Error("再点「更多」没有收起菜单");
    return true;
  });

  /* ── 两份原型不许漂移：单文件原型（视觉基准）守同一条关键行为 ──
     只守「先说明、后动作」与「必读置顶」两条 —— 单文件是同一套代码的镜像，
     逐条复刻断言会让两份用例各自漂移，判据仍是上面那套。 */
  const single = await JSDOM.fromFile(path.join(DIR, "buddy-switch-prototype.html"), {
    runScripts: "dangerously", resources: "usable", pretendToBeVisual: true,
    url: "file://" + path.join(DIR, "buddy-switch-prototype.html").replace(/\\/g, "/") + "?hint=0",
  });
  await new Promise((r) => single.window.addEventListener("load", r));
  await sleep(120);
  const ds = single.window.document;
  check("单文件原型同形：打开弹窗只说明、必读置顶", () => {
    ds.querySelector('[data-act="check-update"]').click();
    const overlay = ds.querySelector("#update-overlay");
    if (!overlay.classList.contains("open")) throw new Error("单文件原型没有打开升级说明弹窗（两份漂移了）");
    const dialog = overlay.querySelector(".upd");
    if (!dialog || dialog.dataset.updStage !== "available") throw new Error("单文件原型打开后不是说明态");
    if (overlay.querySelector("[data-upd-progress]")) throw new Error("单文件原型打开即开始下载");
    const brk = overlay.querySelector("[data-upd-breaking]");
    const notes = overlay.querySelector("[data-upd-notes]");
    if (!brk || !notes) throw new Error("单文件原型缺必读区块或更新分组");
    if (!(brk.compareDocumentPosition(notes) & single.window.Node.DOCUMENT_POSITION_FOLLOWING))
      throw new Error("单文件原型的必读区块没有置顶");
    return true;
  });

  /* ── 抽屉与「更多」的分工（本轮重设计）──
     判据：**能一步做完的进菜单；要看数据才敢决定的留在抽屉。**
     同一动作只许有一处入口 —— 上一版「刷新」在菜单、「刷新 Token」在抽屉、
     「手动签到」两边都能点，就是乱在重复上。 */
  const domD = await boot("dest=accounts&hint=0");
  const dD = domD.window.document;
  const wD = domD.window;
  const openDrawerOf = (id) => {
    const card = [...dD.querySelectorAll(".acct")].find((c) => c.dataset.act === `acct-open|${id}`);
    if (!card) throw new Error("找不到账号卡：" + id);
    card.click();
    return dD.querySelector("#drawer");
  };
  const drawerBody = () => dD.querySelector("#drawer-body");

  check("★ 抽屉第一屏就是四个关键指标（不用滚也知道这个账号什么状态）", () => {
    const drawer = openDrawerOf("a1");
    if (!drawer.classList.contains("open")) throw new Error("点卡片没有打开抽屉");
    const tiles = [...dD.querySelectorAll(".acct-stats .stat-tile")];
    if (tiles.length !== 4) throw new Error(`指标块 ${tiles.length} 个，应为 4 个`);
    const labs = tiles.map((t) => t.querySelector(".lab").textContent.trim()).join(",");
    if (labs !== "积分,今日 Token,今日签到,凭据") throw new Error("指标块不对：" + labs);
    const vals = tiles.map((t) => t.querySelector(".val").textContent.trim());
    if (vals.some((v) => !v)) throw new Error("有指标块是空的：" + vals.join(" | "));
    return labs;
  });

  check("★ 抽屉分 Tab（概览 / 签到 / 凭据 / 快照），原来的长滚动消失", () => {
    const tabs = [...dD.querySelectorAll(".drawer-tabs button")];
    const names = tabs.map((t) => t.textContent.replace(/\s+/g, ""));
    for (const want of ["概览", "签到", "凭据"]) {
      if (!names.some((n) => n.startsWith(want))) throw new Error("缺 Tab：" + want + "（现有 " + names.join("/") + "）");
    }
    const before = drawerBody().textContent;
    tabs.find((t) => t.textContent.includes("凭据")).click();
    const after = drawerBody().textContent;
    if (after === before) throw new Error("点「凭据」Tab 内容没有变");
    if (!/续期 JWT/.test(after)) throw new Error("凭据页没有「续期 JWT」这个深操作");
    if (!/只读/.test(after)) throw new Error("凭据页没说清「续期会写入 / 刷新状态只读」的区别");
    return names.join("/");
  });

  check("★★ 同一动作只许一套：抽屉里没有删除 / 导出 / 签到，刷新与菜单同源", () => {
    /* 逐 Tab 扫一遍：只在默认页上查，等于让「塞进别的 Tab」的重复入口漏网。 */
    const tabIds = [...dD.querySelectorAll(".drawer-tabs button")].map((b) => b.dataset.act);
    if (!tabIds.length) throw new Error("抽屉没有 Tab，无从检查");
    const acts = new Set();
    for (const id of tabIds) {
      dD.querySelector(`[data-act="${id}"]`).click();
      for (const n of drawerBody().querySelectorAll("[data-act]")) acts.add(n.dataset.act);
    }
    /* 切回第一个 Tab，别把后续用例留在「凭据」页上。 */
    dD.querySelector(`[data-act="${tabIds[0]}"]`).click();

    /* ① 菜单专属的单步动作不得出现在抽屉里。 */
    const banned = [...acts].filter((a) => /^act\+(delete|export|checkin)/.test(a.replace(/\|/g, "+")));
    if (banned.length) throw new Error("抽屉里又出现了菜单里已有的动作：" + banned.join(", "));

    /* ② 抽屉里的刷新只能是菜单那四项（同一 data-act），且每个指标块恰好一枚 ——
          它是「在数据旁边刷这份数据」，不是第二套刷新实现。 */
    const kinds = [...new Set([...acts].filter((a) => a.startsWith("acct-refresh|")).map((a) => a.split("|")[2]))].sort();
    if (kinds.join(",") !== "checkin,credential,credits,tokens")
      throw new Error("抽屉里的刷新项与菜单不同源：" + kinds.join(","));
    const tiles = [...dD.querySelectorAll(".stat-tile")];
    if (tiles.length !== 4) throw new Error(`指标块 ${tiles.length} 个，应为 4 个`);
    const perTile = tiles.map((t) => t.querySelectorAll('[data-act^="acct-refresh|"]').length);
    if (perTile.some((n) => n !== 1)) throw new Error("指标块上的刷新不是「一块一枚」：" + perTile.join(","));

    /* ③ 阴性对照：抽屉该有的深操作必须在（否则「没重复」可以靠什么都不放来骗过）。 */
    const need = ["act|refresh-token|a1", "perm-guide|a1"];
    const missing = need.filter((n) => !acts.has(n));
    if (missing.length) throw new Error("抽屉缺少应有的深操作：" + missing.join(", "));
    return `扫了 ${tabIds.length} 个 Tab / ${acts.size} 个可点项`;
  });

  check("★ 指标块上的 ⟳ 只刷那一项，并在块上给出进行中反馈", () => {
    const tile = [...dD.querySelectorAll(".stat-tile")].find((t) => t.textContent.includes("今日 Token"));
    if (!tile) throw new Error("找不到「今日 Token」指标块");
    const btn = tile.querySelector('[data-act^="acct-refresh|"]');
    btn.click();
    const busy = wD.eval("Array.from(state.refreshing)");
    const want = btn.dataset.act.split("|").slice(1).join(":");
    if (busy.length !== 1 || busy[0] !== want) throw new Error("刷新的范围不止这一项：" + JSON.stringify(busy));
    if (!dD.querySelector(".stat-tile.busy")) throw new Error("指标块没有进入进行中态");
    if (!dD.querySelector(".stat-tile.busy .spinner")) throw new Error("进行中没转圈");
    if (dD.querySelector(".acct-menu")) throw new Error("在抽屉里刷新把卡片的菜单也打开了");
    return want;
  });

  check("★ 抽屉里刷完不跳 Tab、不发两份请求（同一状态机）", () => {
    /* 上一条点的是 tokens（420ms）。等它回来，确认：抽屉还开着、Tab 没被打回概览以外的页、状态清空。 */
    return true;
  });

  /* ── 分组 / 备注 / 积分明细（本轮补充）── */
  const gotoTabOf = (id, tabId) => {
    const btn = dD.querySelector(`[data-act="drawer-tab|${id}|${tabId}"]`);
    if (!btn) throw new Error("抽屉里没有 " + tabId + " Tab");
    btn.click();
  };

  check("★ 抽屉能改分组：改完立刻写回数据并重绘", () => {
    gotoTabOf("a1", "overview");
    const sel = drawerBody().querySelector("[data-account-group]");
    if (!sel) throw new Error("概览里没有分组选择器（分组还是只读）");
    const id = sel.dataset.accountGroup;
    const groupOf = () => wD.eval(`curScope().accounts.find(x => x.id === ${JSON.stringify(id)}).group`);
    const before = groupOf();
    const next = before === "测试" ? "常用" : "测试";
    sel.value = next;
    sel.dispatchEvent(new wD.Event("change", { bubbles: true }));
    if (groupOf() !== next) throw new Error(`分组没写回数据：${before} → ${groupOf()}`);
    const sel2 = drawerBody().querySelector("[data-account-group]");
    if (!sel2 || sel2.value !== next) throw new Error("重绘后选择器没跟上新值");
    if (!dD.querySelector("#toasts .toast")) throw new Error("改分组没有任何反馈");
    return `${before} → ${next}`;
  });

  check("★ 可以「新建分组」：选「＋ 新建分组…」就地变输入框，创建后写回且下拉能选到", () => {
    gotoTabOf("a1", "overview");
    const sel = drawerBody().querySelector("[data-account-group]");
    if (!sel) throw new Error("没有分组选择器");
    if (![...sel.options].some((o) => o.value === "__new__")) throw new Error("下拉里没有「新建分组」入口");
    sel.value = "__new__";
    sel.dispatchEvent(new wD.Event("change", { bubbles: true }));
    const input = dD.querySelector("#group-new-input");
    if (!input) throw new Error("选了「新建分组」没有出现输入框");
    input.value = "客户 A";
    dD.querySelector('[data-act="group-create|a1"]').click();
    const group = wD.eval("curScope().accounts.find((x) => x.id === 'a1').group");
    if (group !== "客户 A") throw new Error("新建的分组没有写回账号：" + group);
    const sel2 = drawerBody().querySelector("[data-account-group]");
    if (!sel2) throw new Error("创建后没有回到选择器");
    if (sel2.value !== "客户 A") throw new Error("选择器没显示新建的分组");
    if (![...sel2.options].some((o) => o.value === "客户 A")) throw new Error("新建的分组没进下拉选项（下次选不到）");
    return "客户 A";
  });

  check("★ 新建分组：空名被拒（不造一个空分组）", () => {
    const sel = drawerBody().querySelector("[data-account-group]");
    sel.value = "__new__";
    sel.dispatchEvent(new wD.Event("change", { bubbles: true }));
    const input = dD.querySelector("#group-new-input");
    input.value = "   ";
    dD.querySelector('[data-act="group-create|a1"]').click();
    const group = wD.eval("curScope().accounts.find((x) => x.id === 'a1').group");
    if (group !== "客户 A") throw new Error("空名把原分组改掉了：" + group);
    if (!dD.querySelector("#toasts .toast.err")) throw new Error("空名没有报错提示");
    dD.querySelector('[data-act="group-create-cancel|a1"]').click();
    return true;
  });

  check("★ 备注可就地编辑并保存（改之前能看见原文）", () => {
    gotoTabOf("a1", "overview");
    const editBtn = drawerBody().querySelector('[data-act="act|remark|a1"]');
    if (!editBtn) throw new Error("备注行没有「编辑」入口");
    editBtn.click();
    const input = dD.querySelector("#remark-input");
    if (!input) throw new Error("点了编辑没有出现输入框");
    if (!input.value) throw new Error("输入框里没有预填当前备注 —— 用户看不见原文就改");
    input.value = "原型备注 · 已保存";
    dD.querySelector('[data-act="act|remark-save|a1"]').click();
    const saved = wD.eval("curScope().accounts.find(x => x.id === 'a1').remark");
    if (saved !== "原型备注 · 已保存") throw new Error("备注没写回数据：" + saved);
    if (!drawerBody().textContent.includes("原型备注")) throw new Error("抽屉里没显示新备注");
    return saved;
  });

  check("★ 「积分」Tab 是完整的资源包明细（名称 / 到期 / 剩余·总量 / 已用 / 进度条）", () => {
    gotoTabOf("a1", "credits");
    const rows = [...drawerBody().querySelectorAll(".pk")];
    const data = wD.eval("curScope().accounts.find(x => x.id === 'a1').packs.filter(p => p.remaining > 0)");
    if (!data.length) throw new Error("演示数据里该账号没有可用资源包（用例前提不成立）");
    if (rows.length !== data.length) throw new Error(`渲染 ${rows.length} 个包，数据 ${data.length} 个`);
    for (const row of rows) {
      for (const sel of [".pk-name", ".pk-exp", ".pk-left", ".pk-used"]) {
        if (!row.querySelector(sel) || !row.querySelector(sel).textContent.trim())
          throw new Error("资源包行缺内容：" + sel);
      }
      if (!row.querySelector(".pk-bar > i")) throw new Error("资源包行没有进度条");
    }
    if (!/积分 \/ 资源包/.test(drawerBody().textContent)) throw new Error("缺「积分 / 资源包」标题");
    return `${rows.length} 个包`;
  });

  check("★★ 进度条画的是「剩余占比」，且按到期紧迫度染色（不是按用量）", () => {
    /* 逐账号查：只看一张卡的话，一个账号的包刚好「用得多的也都快到期」，
       染色按用量 / 按到期两种实现会给出同样的结果 —— 变异测试里就是这么漏过去的。 */
    const ids = wD.eval("curScope().accounts.map((a) => a.id)");
    const now = Date.now();
    let rowsSeen = 0, sawSoon = false, sawFresh = false;
    for (const id of ids) {
      openDrawerOf(id);
      gotoTabOf(id, "credits");
      const rows = [...drawerBody().querySelectorAll(".pk")];
      const data = wD.eval(`curScope().accounts.find((x) => x.id === ${JSON.stringify(id)}).packs.filter((p) => p.remaining > 0)`)
        .slice()
        .sort((x, y) => (x.expireAt || Infinity) - (y.expireAt || Infinity) || x.expireInDays - y.expireInDays);
      if (rows.length !== data.length) throw new Error(`${id}：渲染 ${rows.length} 个包，数据 ${data.length} 个`);
      rows.forEach((row, i) => {
        const p = data[i];
        const pct = Math.max(0, Math.min(100, (p.remaining / p.total) * 100));
        const width = parseFloat(row.querySelector(".pk-bar > i").style.width);
        if (Math.abs(width - pct) > 0.2)
          throw new Error(`${id} 第 ${i + 1} 个包进度条 ${width}%，应为剩余占比 ${pct.toFixed(1)}%`);
        const soon = p.expireAt ? p.expireAt - now <= 7 * 86400000 : p.expireInDays <= 7;
        const barWarn = row.querySelector(".pk-bar > i").classList.contains("warn");
        if (soon !== barWarn) throw new Error(`${id} 第 ${i + 1} 个包染色不对：临期=${soon} 但 warn=${barWarn}`);
        if (soon) sawSoon = true; else sawFresh = true;
      });
      rowsSeen += rows.length;
    }
    /* 阴性对照：整个作用域里必须同时出现临期与非临期包，否则颜色断言是恒真的空转。 */
    if (!sawSoon || !sawFresh) throw new Error("演示数据里没有同时出现临期 / 非临期包，颜色断言无从验证");
    return `${ids.length} 个账号 / ${rowsSeen} 个包 · 临期与非临期都在`;
  });

  check("★★ 包明细合计 = 账号积分（指标块与包列表不许各说各话）", () => {
    const ids = wD.eval("curScope().accounts.map((a) => a.id)");
    for (const id of ids) {
      const [credits, sum] = wD.eval(`(() => {
        const a = curScope().accounts.find((x) => x.id === ${JSON.stringify(id)});
        return [a.credits, (a.packs || []).reduce((s, p) => s + p.remaining, 0)];
      })()`);
      if (Math.abs(credits - sum) > 0.01)
        throw new Error(`${id}：账号积分 ${credits}，包剩余合计 ${sum}（应相等）`);
    }
    return `${ids.length} 个账号合计一致`;
  });

  check("★★ 指标块的「临期积分」与积分页的临期包必须一致（不能一个说无、一个摆着）", () => {
    /* 这条守的是**数据口径**：临期积分若手写在账号上、而包明细是另一份数据，
       两边迟早打架（修复前 a2 就是「指标块说无临期积分，积分页有个 10-20 到期的包」）。 */
    const ids = wD.eval("curScope().accounts.map((a) => a.id)");
    let soonAccounts = 0;
    for (const id of ids) {
      openDrawerOf(id);
      const tile = [...dD.querySelectorAll(".stat-tile")].find((t) => t.textContent.includes("积分"));
      const sub = tile.querySelector(".sub").textContent.trim();
      gotoTabOf(id, "credits");
      const soonRows = [...drawerBody().querySelectorAll(".pk")].filter((r) => r.querySelector(".pk-bar > i").classList.contains("warn"));
      if (soonRows.length && /无临期积分/.test(sub))
        throw new Error(`${id}：指标块说「无临期积分」，积分页却有 ${soonRows.length} 个临期包`);
      if (!soonRows.length && !/无临期积分/.test(sub))
        throw new Error(`${id}：积分页没有临期包，指标块却说「${sub}」`);
      if (soonRows.length) soonAccounts++;
    }
    if (!soonAccounts) throw new Error("演示数据里没有任何账号有临期包，断言无从验证");
    return `${ids.length} 个账号 · ${soonAccounts} 个有临期`;
  });

  check("★ 资源包行按到期升序（最急的在最上面），已用完的不占位置", () => {
    const rows = [...drawerBody().querySelectorAll(".pk")];
    const shown = rows.map((r) => r.querySelector(".pk-name").textContent.trim() + "|" + r.querySelector(".pk-exp").textContent.trim());
    const all = wD.eval("curScope().accounts.find(x => x.id === 'a1').packs");
    const usedUp = all.filter((p) => p.remaining <= 0).length;
    if (rows.length + usedUp !== all.length) throw new Error("已用完的包没有按约定隐藏");
    /* 到期时间在文本里，直接比字符串即按 MM-DD HH:mm 排序。 */
    const exps = rows.map((r) => r.querySelector(".pk-exp").textContent.replace("到期 ", "").trim());
    const asc = exps.every((v, i) => i === 0 || exps[i - 1] <= v);
    if (!asc) throw new Error("没有按到期升序：" + exps.join(" / "));
    return shown.length + " 行";
  });

  check("★ 固定底栏 = 抽屉唯一的流程出口（用向导切换），不跟正文按钮打架", () => {
    const foot = dD.querySelector("#drawer-foot");
    if (!foot || !foot.classList.contains("show")) throw new Error("抽屉没有固定底栏");
    const btns = [...foot.querySelectorAll("button")];
    if (btns.length !== 1) throw new Error(`底栏按钮 ${btns.length} 个，应为 1 个（多放就又成了两套按钮）`);
    if (!btns[0].dataset.act.startsWith("wizard|")) throw new Error("底栏不是「带迁移的切换」：" + btns[0].dataset.act);
    const bodyActs = [...drawerBody().querySelectorAll("[data-act]")].map((n) => n.dataset.act);
    if (bodyActs.includes(btns[0].dataset.act)) throw new Error("底栏动作在正文里又出现了一次");
    return true;
  });

  check("★ 「更多」里有手动签到 / 导出 / 删除，删除是最后一项且为危险样式", () => {
    const card = [...dD.querySelectorAll(".acct")].find((c) => c.dataset.act === "acct-open|a4");
    if (!card) throw new Error("找不到未签到的账号卡（用例前提）");
    card.querySelector('[data-act^="acct-more|"]').click();
    const items = [...dD.querySelectorAll(".acct-menu .am-item")];
    const acts = items.map((n) => n.dataset.act).filter(Boolean);
    for (const want of ["act|checkin|a4", "act|export|a4", "act|delete|a4"]) {
      if (!acts.includes(want)) throw new Error("菜单里没有 " + want + "（现有 " + acts.join(", ") + "）");
    }
    const last = items[items.length - 1];
    if (last.dataset.act !== "act|delete|a4") throw new Error("删除不是最后一项（危险动作不该夹在中间）");
    if (!last.classList.contains("danger")) throw new Error("删除项没有危险样式");
    return acts.length + " 项";
  });

  check("★ 菜单里的动作点完要收起菜单（刷新四项除外）", () => {
    const exp = dD.querySelector('[data-act="act|export|a4"]');
    if (!exp) throw new Error("菜单里没有导出项");
    exp.click();
    if (dD.querySelector(".acct-menu")) throw new Error("点了导出菜单还开着（用户会以为没点到）");
    if (!dD.querySelector("#toasts .toast")) throw new Error("导出没有任何反馈");
    return true;
  });

  /* 单文件原型也必须带上同一套「更多 → 四项刷新」—— 它是视觉基准，
     少了这块菜单，两份原型就会在评审时给出两种卡片形态。 */
  const singleAcct = await JSDOM.fromFile(path.join(DIR, "buddy-switch-prototype.html"), {
    runScripts: "dangerously", resources: "usable", pretendToBeVisual: true,
    url: "file://" + path.join(DIR, "buddy-switch-prototype.html").replace(/\\/g, "/") + "?dest=accounts&hint=0",
  });
  await new Promise((r) => singleAcct.window.addEventListener("load", r));
  await sleep(120);
  const dsAcct = singleAcct.window.document;
  check("单文件原型同形：「更多」里同样是四个分开的刷新（两份不许漂移）", () => {
    const cards = [...dsAcct.querySelectorAll(".acct")];
    if (!cards.length) throw new Error("单文件原型的账号页没有卡片");
    const card = cards[0];
    const more = card.querySelector('[data-act^="acct-more|"]');
    if (!more) throw new Error("单文件原型的卡片没有「更多」");
    const id = more.dataset.act.split("|")[1];
    more.click();
    const items = [...dsAcct.querySelectorAll('.acct-menu .am-item[data-act^="acct-refresh|"]')];
    if (items.length !== 4) throw new Error(`单文件原型刷新项 ${items.length} 个，应为 4 个（两份漂移了）`);
    const kinds = items.map((n) => n.dataset.act.split("|")[2]).join(",");
    if (kinds !== "credits,tokens,checkin,credential") throw new Error("单文件原型四类数据没对上：" + kinds);
    if (dsAcct.querySelector(`[data-act="acct-refresh|${id}|credits"]`) === null)
      throw new Error("单文件原型点开菜单后拿不到这一项的刷新入口");
    return true;
  });

  check("单文件原型同形：抽屉也是「指标 + Tab + 积分明细 + 分组可选」", () => {
    const card = dsAcct.querySelector('[data-act="acct-open|a1"]');
    if (!card) throw new Error("单文件原型找不到 a1 的卡片");
    card.click();
    if (!dsAcct.querySelector(".acct-stats .stat-tile")) throw new Error("单文件原型的抽屉没有指标块");
    const tabActs = [...dsAcct.querySelectorAll(".drawer-tabs button")].map((b) => b.dataset.act);
    if (!tabActs.includes("drawer-tab|a1|credits")) throw new Error("单文件原型没有「积分」Tab");
    if (!dsAcct.querySelector("[data-account-group]")) throw new Error("单文件原型的抽屉不能改分组");
    dsAcct.querySelector('[data-act="drawer-tab|a1|credits"]').click();
    if (!dsAcct.querySelector(".pk")) throw new Error("单文件原型的积分页没有资源包明细（两份漂移了）");
    return "指标 / Tab / 积分明细 / 分组 都在";
  });

  /* ══════════════════════ CodeBuddy CLI 接入方式（issue #109）══════════════════════
   *
   * 这块是「两套凭据通道二选一」，不是偏好开关 —— 所以断言钉的是**设计判断**，不是像素：
   *   ① 状态条先说「现在走哪条 / 谁在选号 / 生效了吗」；
   *   ② 两张卡的能力用 ✓/— **条数一致**（否则没法横着比）；
   *   ③ 切换**先向导**，且「确认前状态不能变」；
   *   ④ 切完状态条必须变「待重启 CLI」（env 不热重载，这是最容易误判「没生效」的地方）；
   *   ⑤ 冲突态：状态条变警示 + 直连卡禁用 + **给一条出路**（只禁用不给下一步 = 用户卡住）。
   */
  const domCli = await boot("dest=settings&set=cli&hint=0");
  const dc = domCli.window.document;
  const cByAct = (act) => [...dc.querySelectorAll("[data-act]")].find((el) => el.dataset.act === act);
  const cClick = (el) => el.dispatchEvent(new domCli.window.MouseEvent("click", { bubbles: true }));
  const stripText = () => (dc.querySelector(".mode-strip") || {}).textContent || "";
  const onCard = () => dc.querySelector(".mode-card.on");

  check("CLI 接入：状态条置顶，说清「走哪条路 + 谁在选号 + 已生效」", () => {
    const strip = dc.querySelector(".mode-strip");
    if (!strip) throw new Error("没有状态条（打开这页的第一个问题就是「我现在是什么状态」）");
    const text = strip.textContent;
    if (!text.includes("直连上游")) throw new Error("状态条没说明当前模式：" + text.slice(0, 80));
    if (!text.includes("按积分到期选号")) throw new Error("状态条没说清「谁在选号」");
    if (!text.includes("已生效")) throw new Error("状态条必须回答「生效了吗」（这里应是已生效）");
    /* 状态条必须在模式卡**之前**（DOM 顺序即视觉顺序）。 */
    const cards = dc.querySelector(".mode-cards");
    if (!(strip.compareDocumentPosition(cards) & domCli.window.Node.DOCUMENT_POSITION_FOLLOWING)) {
      throw new Error("状态条应在模式卡之前");
    }
    return text.replace(/\s+/g, " ").trim().slice(0, 60);
  });

  check("CLI 接入：两张模式卡并列，能力项**条数一致**（可横比）+ 有 yes/no", () => {
    const cards = [...dc.querySelectorAll(".mode-card")];
    if (cards.length !== 2) throw new Error(`应有 2 张模式卡，实际 ${cards.length}`);
    const counts = cards.map((card) => card.querySelectorAll(".cap li").length);
    if (counts[0] !== counts[1]) {
      throw new Error(`两卡能力项条数不一致（${counts.join(" vs ")}）—— 用户没法横着比`);
    }
    if (counts[0] < 2) throw new Error("能力项太少，等于没说明差异");
    const hasNo = cards.every((card) => card.querySelector(".cap li.no"));
    if (!hasNo) throw new Error("每张卡都该有一条「—」（只说优点等于广告，不是对比）");
    return `2 卡 × ${counts[0]} 项 · 各有 yes/no`;
  });

  check("CLI 接入：当前卡标「当前」，且动作是 disabled 的「已启用」", () => {
    const card = onCard();
    if (!card) throw new Error("没有 .mode-card.on（当前模式不可辨）");
    if (!card.textContent.includes("当前")) throw new Error("当前卡没有「当前」标记");
    const btn = card.querySelector(".mc-foot .btn");
    if (!btn || !btn.disabled) throw new Error("当前卡的动作必须是 disabled（不能重复切换）");
    return card.querySelector(".mc-name").textContent.trim();
  });

  check("CLI 接入：切到网关**先弹向导**，且确认前状态不变", () => {
    const other = [...dc.querySelectorAll(".mode-card")].find((card) => card !== onCard());
    const btn = other.querySelector(".mc-foot .btn");
    cClick(btn);
    const drawer = dc.querySelector("#drawer-body");
    if (!drawer || !drawer.textContent.trim()) throw new Error("没有弹切换向导");
    const text = drawer.textContent;
    for (const must of ["清掉", "关闭自动轮换", "重启终端"]) {
      if (!text.includes(must)) throw new Error(`向导缺「${must}」：${text.slice(0, 100)}`);
    }
    /* 向导里必须能选默认模型（这是它区别于「确认框」的地方）。 */
    if (!drawer.querySelector("#cli-wiz-model")) throw new Error("向导里没有默认模型下拉");
    if (!onCard().textContent.includes("直连上游")) throw new Error("确认前状态就变了 —— 向导成了装饰");
    return "代价三项 + 模型下拉 · 确认前不变";
  });

  check("CLI 接入：确认后状态条变「走本机网关 · 待重启 CLI」", () => {
    cClick(cByAct("cli-confirm|gateway"));
    const text = stripText();
    if (!text.includes("走本机网关")) throw new Error("状态条没切到网关：" + text.slice(0, 80));
    if (!text.includes("由网关按模型选号")) throw new Error("状态条没说清「谁在选号」");
    if (!text.includes("待重启 CLI")) {
      throw new Error("切换后必须标「待重启 CLI」—— env 不热重载，不标用户会以为没生效");
    }
    if (text.includes("已生效")) throw new Error("刚切完就写「已生效」是假话（还没重启 CLI）");
    return text.replace(/\s+/g, " ").trim().slice(0, 60);
  });

  check("CLI 接入：高级默认收起；展开后才有模型下拉与 Key（且 Key 是掩码）", () => {
    if (dc.querySelector('select[data-act="cli-model"]')) {
      throw new Error("高级区默认应收起（默认模型是低频项，不该占主视线）");
    }
    const toggle = dc.querySelector(".adv-toggle");
    if (!toggle) throw new Error("没有高级折叠入口");
    cClick(toggle);
    const select = dc.querySelector('select[data-act="cli-model"]');
    if (!select) throw new Error("展开后仍没有模型下拉");
    if (![...select.options].some((o) => o.textContent.includes("留空"))) {
      throw new Error("模型下拉缺「留空 · 由 CLI 自己决定」");
    }
    const body = dc.querySelector(".adv-body").textContent;
    if (!body.includes("••••")) throw new Error("Key 必须是掩码");
    if (/sk-wb-[a-z0-9]{20,}/i.test(body)) throw new Error("出现了疑似完整 Key");
    if (!body.includes("已关闭")) throw new Error("网关模式下自动轮换应显示「已关闭」");
    return "默认收起 · 展开有模型/Key/轮换";
  });

  check("CLI 接入：冲突态 = 状态条警示 + 直连卡禁用 + **给一条出路**", () => {
    /* 先切回直连，再打开演示开关（模拟 settings 已被外部改成网关地址）。 */
    const gwCard = [...dc.querySelectorAll(".mode-card")].find((card) => card !== onCard());
    cClick(gwCard.querySelector(".mc-foot .btn"));
    cClick(cByAct("cli-confirm|direct"));
    cClick(cByAct("cli-demo-foreign"));

    const strip = dc.querySelector(".mode-strip");
    if (!strip.classList.contains("warn")) throw new Error("冲突态的状态条必须是警示态");
    const text = strip.textContent;
    if (!text.includes("这不是错误，是护栏")) throw new Error("缺「这不是错误，是护栏」");
    if (!text.includes("时好时坏")) throw new Error("缺「时好时坏」—— 那是这个故障最难查的地方");

    const directCard = [...dc.querySelectorAll(".mode-card")].find((card) =>
      card.textContent.includes("直连上游"),
    );
    if (!directCard.classList.contains("off")) throw new Error("冲突时直连卡应标 .off");
    const btn = directCard.querySelector(".mc-foot .btn");
    if (btn && !btn.disabled) throw new Error("冲突时直连卡的动作必须 disabled（只改样式等于没禁）");

    /* ★ 只禁用不给下一步 = 用户卡住：状态条里必须有一条可点的出路。 */
    const way = [...strip.querySelectorAll("[data-act]")].find((el) =>
      el.dataset.act.startsWith("cli-mode|"),
    );
    if (!way) throw new Error("冲突态必须给出一条出路（接管网关配置），否则用户卡在原地");
    return "警示条 · 直连卡禁用 · 出路：" + way.textContent.trim();
  });

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
