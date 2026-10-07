/* 原型验证：用 jsdom 真跑一遍交互，断言不是「看起来对」。
 * 用法：
 *   NODE_PATH=<managed workspace>/node_modules node docs/redesign/verify-demo.cjs [demoDir]
 *   ... --mutate   注入一个变异（去掉启动渲染），**期望测试变红**，用于证明断言不空转。
 */
const path = require("path");
const fs = require("fs");
const os = require("os");
const { JSDOM } = require("jsdom");

const BASE = __dirname;
/* 变异注入：把缺陷塞回去，**期望测试变红**。全绿就说明断言在空转。 */
const MUT = (process.argv.find((a) => a.startsWith("--mutate")) || "").replace("--mutate", "").replace(/^=/, "");
const MUTATE = Boolean(MUT);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const MUTATIONS = {
  boot: {
    desc: "去掉启动渲染（模拟白屏）",
    file: "demo-app.js",
    apply: (src) => {
      const out = src.replace(/\nbootFromUrl\(\);/,
        "\n/* MUTATED: 启动渲染被移除 */\ndocument.querySelector('#content').innerHTML = '';");
      if (out === src) throw new Error("未匹配到启动 bootFromUrl()");
      return out;
    },
  },
  cap: {
    desc: "忽略能力开关（模拟平台硬编码分支，让 Trae 也出现「自动旅行」）",
    file: "demo-app.js",
    apply: (src) => {
      const from = "const rows = g.rows.filter((r) => !r.cap || p.capabilities.includes(r.cap));";
      const to = "const rows = g.rows.filter(() => true); /* MUTATED: 忽略能力开关 */";
      if (!src.includes(from)) throw new Error("未匹配到能力过滤行");
      return src.replace(from, to);
    },
  },
  ime: {
    desc: "搜索时整页重绘（模拟打断中文输入法）",
    file: "demo-app.js",
    apply: (src) => {
      const from = 'if (t.id === "acct-q") { state.q = t.value; renderAccountList(); return; }';
      const to = 'if (t.id === "acct-q") { state.q = t.value; renderContent(); return; } /* MUTATED */';
      if (!src.includes(from)) throw new Error("未匹配到搜索处理行");
      return src.replace(from, to);
    },
  },
};

let dir = BASE;
if (MUTATE) {
  if (!MUTATIONS[MUT]) { console.error("未知变异：" + MUT + "（可选 " + Object.keys(MUTATIONS).join(" / ") + "）"); process.exit(2); }
  dir = fs.mkdtempSync(path.join(os.tmpdir(), "bsdemo-"));
  for (const f of fs.readdirSync(BASE)) fs.copyFileSync(path.join(BASE, f), path.join(dir, f));
  const m = MUTATIONS[MUT];
  const p = path.join(dir, m.file);
  /* 锚点统一在 LF 归一化后的文本上做匹配，写回时还原原换行风格 —— 与 verify-behaviors.cjs 一致。
     本仓文件是 CRLF，若拿含 `\n` 的字面量直接 includes 会静默不匹配 ⇒ 假验证。 */
  const src = fs.readFileSync(p, "utf8");
  const eol = src.includes("\r\n") ? "\r\n" : "\n";
  fs.writeFileSync(p, m.apply(src.replace(/\r\n/g, "\n")).replace(/\n/g, eol));
  console.log(`【变异注入】${m.desc}\n`);
}

const results = [];
function check(name, fn) {
  try { const r = fn(); results.push({ name, ok: r !== false, detail: r === true || r === undefined ? "" : String(r) }); }
  catch (e) { results.push({ name, ok: false, detail: e.message }); }
}

(async () => {
  const dom = await JSDOM.fromFile(path.join(dir, "index.html"), {
    runScripts: "dangerously", resources: "usable", pretendToBeVisual: true,
  });
  const { window } = dom;
  const doc = window.document;
  const $ = (s) => doc.querySelector(s);
  const $$ = (s) => [...doc.querySelectorAll(s)];
  const click = (s) => { const n = typeof s === "string" ? $(s) : s; if (!n) throw new Error("找不到元素: " + s); n.click(); };
  const text = (s) => { const n = $(s); return n ? n.textContent.trim() : ""; };

  await new Promise((r) => window.addEventListener("load", r));
  await sleep(60);

  /* ── 骨架 ── */
  check("首屏非空白（#content 有实质内容）", () => {
    const len = $("#content").innerHTML.length;
    if (len < 800) throw new Error("#content 只有 " + len + " 字符，疑似白屏");
    return len + " 字符";
  });
  check("侧栏默认 6 个目的地（含「会话」，可配置）", () => {
    const items = $$("#nav .nav-item");
    if (items.length !== 6) throw new Error("实际 " + items.length + " 项");
    const labels = items.map((el) => el.querySelector(".nav-label").textContent.trim());
    for (const want of ["概览", "账号", "用量", "会话", "网关", "设置"]) {
      if (!labels.includes(want)) throw new Error("缺少「" + want + "」：" + labels.join("/"));
    }
    return labels.join(" / ");
  });
  check("作用域名默认 = WorkBuddy 国内版", () => {
    const t = text("#scope-name");
    if (t !== "WorkBuddy 国内版") throw new Error("实际「" + t + "」");
    return true;
  });
  check("顶栏运行态徽章 = 运行中", () => {
    if (!text("#run-pill").includes("运行中")) throw new Error("实际「" + text("#run-pill") + "」");
    return true;
  });
  check("概览页有「待处理」区块", () => {
    if (!text("#content").includes("待处理")) throw new Error("缺少待处理区块");
    return true;
  });

  /* ── 账号工作台 ── */
  try {
  click('[data-act="nav|accounts"]');
  await sleep(30);
  check("账号页渲染 8 张账号卡", () => {
    const n = $$(".acct").length;
    if (n !== 8) throw new Error("实际 " + n + " 张");
    return true;
  });
  check("账号页有搜索/排序/分组工具栏", () => {
    for (const k of ["#acct-q", '[data-input="sort"]', '[data-input="group"]']) if (!$(k)) throw new Error("缺少 " + k);
    return true;
  });

  /* 搜索：只重绘列表区，输入框 DOM 不换（中文输入法不被打断） */
  const inputBefore = $("#acct-q");
  inputBefore.value = "jackey";
  inputBefore.dispatchEvent(new window.Event("input", { bubbles: true }));
  await sleep(30);
  check("搜索 jackey 收敛到 1 张卡", () => {
    const n = $$(".acct").length;
    if (n !== 1) throw new Error("实际 " + n + " 张");
    return true;
  });
  check("搜索时输入框 DOM 未被替换（保住光标与 IME）", () => {
    if ($("#acct-q") !== inputBefore) throw new Error("输入框被重绘了");
    return true;
  });
  const q2 = $("#acct-q");
  q2.value = "zzz-no-match";
  q2.dispatchEvent(new window.Event("input", { bubbles: true }));
  await sleep(30);
  check("无匹配时给出空态 + 清除搜索", () => {
    const t = text("#content");
    if (!t.includes("没有匹配的账号")) throw new Error("没有空态");
    if (!$('[data-act="clear-q"]')) throw new Error("没有清除搜索按钮");
    return true;
  });
  click('[data-act="clear-q"]');
  await sleep(30);
  check("空态里点清除搜索恢复列表", () => {
    const n = $$(".acct").length;
    if (n !== 8) throw new Error("实际 " + n + " 张");
    return true;
  });

  /* 排序 / 分组 / 视图 */
  const sel = $('[data-input="group"]');
  sel.value = "status";
  sel.dispatchEvent(new window.Event("change", { bubbles: true }));
  await sleep(30);
  check("按状态分组出现分组标题", () => {
    const heads = $$("#acct-list .badge").map((b) => b.textContent.trim());
    if (!heads.some((h) => ["正常", "今日未签到", "凭据已过期", "需重新登录"].includes(h))) throw new Error("没有分组标题：" + heads.join(","));
    return true;
  });
  click('[data-act="view|table"]');
  await sleep(30);
  check("切换到表格视图", () => {
    if (!$("table.tbl")) throw new Error("没有渲染表格");
    return true;
  });
  click('[data-act="view|card"]');
  await sleep(30);

  /* 勾选 + 批量条 */
  click($("#acct-list .check"));
  await sleep(30);
  check("勾选后浮现批量操作条", () => {
    const bar = $(".batch-bar");
    if (!bar) throw new Error("批量条未出现");
    if (!bar.textContent.includes("已选 1 个")) throw new Error("计数不对：" + bar.textContent.trim());
    return true;
  });
  click('[data-act="batch-clear"]');
  await sleep(30);
  check("取消选择后批量条消失", () => {
    if ($(".batch-bar")) throw new Error("批量条仍在");
    return true;
  });

  /* 程序位切换（含未检测到的分支） */
  click('[data-act="acct-prog|a3|ide"]');
  await sleep(30);
  check("可把 IDE 程序位切到 backup-01", () => {
    const t = $$(".toast").map((x) => x.textContent).join(" | ");
    if (!t.includes("CodeBuddy IDE") || !t.includes("backup-01")) throw new Error("toast 内容不符：" + t);
    return true;
  });
  click('[data-act="acct-prog|a3|cli"]');
  await sleep(30);
  check("未检测到的程序位给出解释而非静默失败", () => {
    const t = $$(".toast").map((x) => x.textContent).join(" | ");
    if (!t.includes("未检测到客户端")) throw new Error("toast 内容不符：" + t);
    return true;
  });

  /* 抽屉 */
  click($("#acct-list .acct"));
  await sleep(30);
  check("点击账号卡打开右侧抽屉", () => {
    if (!$("#drawer").classList.contains("open")) throw new Error("抽屉未打开");
    if (!text("#drawer-body").includes("账号信息")) throw new Error("抽屉内容不符");
    return true;
  });
  check("抽屉内可进入三步切换向导", () => {
    click('[data-act^="wizard|"]');
    if (!text("#drawer-body").includes("目标程序位")) throw new Error("向导第一步未渲染");
    return true;
  });
  click('[data-act="wiz-next"]');
  check("向导第二步 = 迁移内容", () => {
    if (!text("#drawer-body").includes("迁移内容")) throw new Error("第二步未渲染");
    return true;
  });
  click('[data-act="wiz-next"]');
  check("向导第三步 = 确认页", () => {
    if (!text("#drawer-body").includes("影响")) throw new Error("确认页未渲染");
    return true;
  });
  click("#drawer-close");
  await sleep(30);

  /* ── 作用域切换 ── */
  click("#scope-trigger");
  await sleep(30);
  check("作用域菜单列出 4 个作用域", () => {
    const n = $$("#scope-menu .scope-item").length;
    if (n !== 4) throw new Error("实际 " + n + " 个");
    return true;
  });
  click('[data-act="scope|trae-global"]');
  await sleep(360);
  check("切到 TraeWork 国际版后顶栏跟随", () => {
    const t = text("#scope-name");
    if (t !== "TraeWork 国际版") throw new Error("实际「" + t + "」");
    return true;
  });
  check("空作用域给出结构化空态而非空白", () => {
    click('[data-act="nav|accounts"]');
    const t = text("#content");
    if (!t.includes("还没有账号")) throw new Error("没有空态：" + t.slice(0, 80));
    return true;
  });

  /* ── 能力开关驱动 UI（扩展性的核心断言） ── */
  click('[data-act="scope|wb-cn"]');
  await sleep(360);
  click('[data-act="nav|settings"]');
  await sleep(30);
  click('[data-act="set-sub|checkin"]');
  await sleep(30);
  const wbHasTravel = text("#content").includes("自动旅行");
  click('[data-act="scope|trae-cn"]');
  await sleep(360);
  click('[data-act="set-sub|checkin"]');
  await sleep(30);
  const traeHasTravel = text("#content").includes("自动旅行");
  check("能力开关驱动 UI：WorkBuddy 有「自动旅行」", () => wbHasTravel || (() => { throw new Error("WorkBuddy 缺失旅行设置"); })());
  check("能力开关驱动 UI：Trae 无「自动旅行」（同一组件、同一路由）", () => {
    if (traeHasTravel) throw new Error("Trae 不应出现旅行设置——说明页面里存在平台硬编码分支");
    return true;
  });

  /* ── 用量 / 网关 ── */
  click('[data-act="nav|usage"]');
  await sleep(30);
  check("用量工作台：Token 与积分合并、含视图参数条", () => {
    const t = text("#content");
    if (!t.includes("视图参数")) throw new Error("缺少视图参数条");
    for (const k of ["趋势", "模型", "会话", "积分"]) if (!t.includes(k)) throw new Error("缺少子 Tab：" + k);
    return true;
  });
  click('[data-act="usage-tab|models"]');
  await sleep(30);
  check("模型用量分布有横向条", () => {
    if (!$$("#content .hbar").length) throw new Error("没有 hbar");
    return true;
  });

  click('[data-act="nav|gateway"]');
  await sleep(30);
  check("网关工作台有 6 项子导航（不再是 6 卡堆叠）", () => {
    const n = $$(".subnav button").length;
    if (n !== 6) throw new Error("实际 " + n + " 项");
    /* v2.7 起第 6 项是「请求代理」（§3.19 / P12） */
    const labels = $$(".subnav button").map((b) => b.textContent.trim());
    if (!labels.includes("请求代理")) throw new Error("子导航缺少「请求代理」：" + labels.join("/"));
    return labels.join("/");
  });
  click('[data-act="gw-sub|keys"]');
  await sleep(30);
  check("密钥表只显示掩码", () => {
    const t = text("#content");
    if (!t.includes("••")) throw new Error("没有掩码");
    if (/bs_live_[0-9a-f]{8}(?!•)/.test(t)) throw new Error("疑似泄露明文密钥");
    return true;
  });
  click('[data-act="gw-sub|logs"]');
  await sleep(30);
  check("请求日志渲染出状态码", () => {
    if (!text("#content").includes("200")) throw new Error("没有状态码");
    return true;
  });

  /* ── 命令面板 ── */
  doc.dispatchEvent(new window.KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
  await sleep(40);
  check("⌘K 打开命令面板", () => {
    if (!$("#palette-overlay").classList.contains("open")) throw new Error("面板未打开");
    return true;
  });
  check("面板包含账号/页面/作用域/动作四类", () => {
    const t = text("#palette-list");
    for (const k of ["页面", "作用域", "账号", "动作"]) if (!t.includes(k)) throw new Error("缺少分组：" + k);
    return true;
  });
  const pi = $("#palette-input");
  pi.value = "jackey";
  pi.dispatchEvent(new window.Event("input", { bubbles: true }));
  await sleep(30);
  check("面板搜索收敛", () => {
    const n = $$("#palette-list .palette-item").length;
    if (n < 1) throw new Error("没有结果");
    if (n > 12) throw new Error("结果过多：" + n);
    return true;
  });
  doc.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await sleep(30);
  check("ESC 关闭命令面板", () => {
    if ($("#palette-overlay").classList.contains("open")) throw new Error("面板未关闭");
    return true;
  });

  /* ── 主题 ── */
  const wasDark = doc.documentElement.classList.contains("dark");
  click("#theme-btn");
  await sleep(30);
  check("明暗主题可切换", () => {
    if (doc.documentElement.classList.contains("dark") === wasDark) throw new Error("class 未变化");
    return true;
  });

  } catch (e) {
    /* 元素缺失/渲染中断也要落成一条失败，而不是让脚本崩掉——否则变异模式下
       只能看到「脚本异常」，读不出是哪条断言守住了。 */
    results.push({ name: "交互序列完整执行", ok: false, detail: e.message });
  }

  /* ── 报告 ── */
  const pass = results.filter((r) => r.ok).length;
  const fail = results.length - pass;
  for (const r of results) console.log(`${r.ok ? "  ok  " : " FAIL "} ${r.name}${r.detail && !r.ok ? "  → " + r.detail : ""}`);
  console.log(`\n${pass} passed / ${fail} failed`);

  if (MUTATE) {
    console.log(fail > 0 ? "\n变异测试通过：断言在缺陷下确实变红。" : "\n变异测试失败：断言空转，注入缺陷后仍全绿。");
    process.exit(fail > 0 ? 0 : 1);
  }
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => { console.error("验证脚本自身异常：", e); process.exit(3); });
