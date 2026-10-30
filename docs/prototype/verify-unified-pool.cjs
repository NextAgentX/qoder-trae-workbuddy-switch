/* 统一网关交互的实测（2026-10-15 第五轮 · 对齐版）。
 *
 * ## 被测的交互形态
 *
 * 统一网关 = **网关页里的第二个类型**（左栏顶部切类型，下面切该类型的分区）：
 *   - 独立网关：概览 / 密钥 / 模型 / 账号池 / 请求代理 / 请求日志
 *   - 统一网关：概览 / 命名空间 / 密钥
 *
 * ## 本轮重点：**骨架逐块对齐独立网关**
 *
 *   - 概览 = 卡「服务状态」(**全是 `.srow` 行**) + 卡「接入说明」（与独立网关的 `quickAccessHtml` 同形）；
 *   - 其余分区 = `cat-bar` 工具条 + 裸表(`.tbl-wrap`) + `<p class="tiny muted">` 说明段；
 *   - **不许再自造容器**（`.uni-note` / `.uni-sec` / `.uni-sum` 已删，本文件有守卫）；
 *   - 密钥工具条 `keysBar` 是**两个类型共用**的构件；
 *   - 接入说明的协议 Tab 复用 `state.gwApi`，与独立网关**是同一个状态**。
 *
 * 与既有门禁同一立场：只跑单文件；变异只写 `os.tmpdir()` 副本；变异模式下**全绿即失败**。
 *
 * 用法：
 *   NODE_PATH=<managed node workspace>/node_modules node docs/prototype/verify-unified-pool.cjs
 *   ... --mutate=<名>   期望变红
 */
const path = require("path");
const fs = require("fs");
const os = require("os");
const { JSDOM, VirtualConsole } = require("jsdom");

const BASE = __dirname;
const results = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const MUT = (process.argv.find((a) => a.startsWith("--mutate")) || "").replace("--mutate", "").replace(/^=/, "");
const MUTATE = Boolean(MUT);

const MUTATIONS = {
  poolcount: {
    desc: "账号池数字写死（不再从 GATEWAY 的池里数出来）",
    from: '  return { target, total: pool.length, available: count("可用"), low: pool.filter((x) => x.low && !x.excluded).length, excluded };',
    to: '  return { target, total: pool.length, available: 9, low: pool.filter((x) => x.low && !x.excluded).length, excluded }; /* MUTATED */',
  },
  noaccount: {
    desc: "删掉「池里一个账号都没有」这一档（空池被当成有账号）",
    from: '  if (p.total === 0) return { cls: "bad", label: "无账号", hint: `上游池 ${p.target} 里一个账号都没有` };',
    to: "",
  },
  jumpaway: {
    desc: "「看池」退回跳走（丢失跨平台视野）",
    from: '      if (UNI.poolOpen.has(a1)) UNI.poolOpen.delete(a1); else UNI.poolOpen.add(a1);\n      renderContent(); break;',
    to: '      state.scope = a1 === "wb-cn" ? "wb-cn" : "qoder-cn"; state.dest = "gateway"; state.gwSub = "pool"; renderAll(); break; /* MUTATED */',
  },
  verdict: {
    desc: "结论徽标永远说「可用」（不再看池）",
    from: '  const dead = on.filter((x) => uniPoolOf(x.id).available === 0);',
    to: '  const dead = []; /* MUTATED */',
  },
  bindconfirm: {
    desc: "开放局域网去掉危险确认（一键生效）",
    from: '  if (next === UNI.bind) return;\n  const toLan = next === "0.0.0.0";\n  openConfirm({',
    to: '  if (next === UNI.bind) return;\n  const toLan = next === "0.0.0.0";\n  if (true) { UNI.bind = next; renderContent(); return; } /* MUTATED */\n  openConfirm({',
  },
  typeisolation: {
    desc: "切类型不重置分区（统一网关停在独立网关的分区上）",
    from: '      state.gwType = a1;\n      state.gwSub = GW_SECTIONS[a1][0][0];\n      renderContent(); break;',
    to: '      state.gwType = a1; renderContent(); break; /* MUTATED */',
  },
  owncontainer: {
    desc: "统一网关的概览退回自造容器（`.uni-sum` 摘要条）",
    from: '    <div class="srow"><div class="lab"><b>接入地址</b><span class="mono">${base}</span></div>\n      <button class="btn sm" data-act="copy|${base}">${ICON.copy}复制</button></div>',
    to: '    <div class="uni-sum"><span class="tiny muted">${base}</span><button class="btn xs" data-act="copy|${base}">复制</button></div>',
  },
};

let DIR = BASE;
if (MUTATE) {
  if (!MUTATIONS[MUT]) {
    console.error("未知变异：" + MUT + "（可选：" + Object.keys(MUTATIONS).join(" / ") + "）");
    process.exit(2);
  }
  const m = MUTATIONS[MUT];
  DIR = fs.mkdtempSync(path.join(os.tmpdir(), "bsunipool-"));
  const p = path.join(DIR, "buddy-switch-prototype.html");
  const src = fs.readFileSync(path.join(BASE, "buddy-switch-prototype.html"), "utf8");
  const eol = src.includes("\r\n") ? "\r\n" : "\n";
  const norm = src.replace(/\r\n/g, "\n");
  if (!norm.includes(m.from)) {
    console.error("变异注入失败：锚点不匹配 → " + m.from.slice(0, 70));
    process.exit(2);
  }
  fs.writeFileSync(p, norm.replace(m.from, m.to).replace(/\n/g, eol));
  console.log(`【变异注入】${m.desc}\n`);
}

const FILE = path.join(DIR, "buddy-switch-prototype.html");

function check(name, fn) {
  try {
    const r = fn();
    results.push({ name, ok: r !== false, detail: r === true || r === undefined ? "" : String(r) });
  } catch (e) {
    results.push({ name, ok: false, detail: e.message });
  }
}

async function boot(query) {
  const errors = [];
  const vc = new VirtualConsole();
  vc.on("jsdomError", (e) => errors.push(String(e && e.message ? e.message : e)));
  const dom = await JSDOM.fromFile(FILE, {
    runScripts: "dangerously",
    resources: "usable",
    pretendToBeVisual: true,
    virtualConsole: vc,
    url: "file://" + FILE.replace(/\\/g, "/") + (query ? "?" + query : ""),
  });
  await new Promise((r) => dom.window.addEventListener("load", r));
  await sleep(140);
  dom.__errors = errors;
  return dom;
}

const flat = (s) => String(s || "").replace(/\s+/g, " ").trim();
const click = (win, el) => el.dispatchEvent(new win.MouseEvent("click", { bubbles: true }));
const segLabels = (d) => [...d.querySelectorAll(".gw-type-seg button")].map((b) => b.textContent.trim());
const activeSeg = (d) => flat((d.querySelector(".gw-type-seg button.on") || {}).textContent);
const subLabels = (d) => [...d.querySelectorAll(".side-col .subnav button")].map((b) => b.textContent.trim());
const cardHeads = (d) => [...d.querySelectorAll(".split-main .card-head h3")].map((h) => flat(h.textContent));
const srowLabels = (d) => [...d.querySelectorAll(".split-main .srow .lab b")].map((b) => flat(b.textContent));
const clickSub = (win, d, id) =>
  click(win, [...d.querySelectorAll("[data-act^='gw-sub|']")].find((b) => b.dataset.act === "gw-sub|" + id));
/** 取某命名空间那一行（表里第一格是 `<code>` 里的 id）。 */
function rowOf(doc, ns) {
  return [...doc.querySelectorAll("table.tbl tbody tr")].find((tr) => {
    const c = tr.querySelector("td code b");
    return c && c.textContent.trim() === ns;
  });
}

(async () => {
  /* ════════ A · 独立网关（本轮也被改了两处） ════════ */
  const domA = await boot("scope=wb-cn&dest=gateway&hint=0");
  const dA = domA.window.document;

  check("零异常加载", () => {
    if (domA.__errors.length) throw new Error(domA.__errors.slice(0, 2).join(" | "));
    return true;
  });

  check("侧栏 6 项；类型控件 2 项且默认独立网关；子导航 6 项", () => {
    const nav = [...dA.querySelectorAll("#nav .nav-label")].map((n) => n.textContent.trim());
    if (nav.length !== 6 || nav.includes("统一网关")) throw new Error("侧栏：" + nav.join("/"));
    if (segLabels(dA).join("/") !== "独立网关/统一网关") throw new Error("类型控件：" + segLabels(dA).join("/"));
    if (activeSeg(dA) !== "独立网关") throw new Error("默认选中：" + activeSeg(dA));
    const subs = subLabels(dA);
    if (subs.length !== 6) throw new Error("子导航：" + subs.join("/"));
    return nav.join("/") + " · " + subs.join("/");
  });

  check("★ 独立网关的密钥分区也有工具条（有密钥时也能创建 —— 此前只有空态才有出口）", () => {
    clickSub(domA.window, dA, "keys");
    const bar = dA.querySelector(".split-main .cat-bar");
    if (!bar) throw new Error("密钥分区没有 cat-bar 工具条");
    const t = flat(bar.textContent);
    if (!t.includes("共 ")) throw new Error("工具条没有计数：" + t);
    if (!dA.querySelector('[data-act="new-key"]')) throw new Error("工具条没有创建密钥出口");
    return t.slice(0, 60);
  });

  check("★ 独立网关概览的「统一网关」入口行能真的切过去（不再是死链）", () => {
    clickSub(domA.window, dA, "overview");
    const btn = [...dA.querySelectorAll("[data-act^='gw-type|']")].find((b) => b.dataset.act === "gw-type|unified");
    if (!btn) throw new Error("找不到入口按钮");
    click(domA.window, btn);
    if (domA.window.eval("state.gwType") !== "unified") throw new Error("没切过去：" + domA.window.eval("state.gwType"));
    return "gw-type|unified → 统一网关";
  });

  check("★ 全局 `.seg` 的**布局**没被改写（它被用量页 / 接入方式 / 会话筛选共用）", () => {
    /* 曾经把「网关类型」也叫 `.seg` ⇒ 那个**全局**分段控件被改成 grid，用量页 tabs、
       接入方式选择器、会话筛选全变了形。这条守卫钉住「不许再犯」。

       ⚠️ 判据从「数规则条数 == 4」改成「钉布局 + 给总数一个上限」：
       数条数会被**合法的增量**逼着改数字（`.seg button:disabled` 两条、`.seg.book` /
       `.seg.wide` 两个变体后加的都是正当需求），一旦为了过门禁改数字，这条守卫就等于
       自己废掉了 —— 它再也拦不住真正的覆盖（把 `.seg {` 改成 grid 同样会让数字变）。
       现在钉的是**事故本体**：基础规则必须仍是 `inline-flex`，不许出现 grid。 */
    const src = fs.readFileSync(FILE, "utf8");
    const base = (src.match(/^\.seg\s*\{([^}]*)\}/m) || [])[1] || "";
    if (!base.includes("inline-flex")) {
      throw new Error("`.seg` 基础规则不再是 inline-flex：" + base.trim());
    }
    if (base.includes("grid")) throw new Error("`.seg` 被改成 grid（正是那次事故）");
    /* 纯 `.seg` 规则（`.seg` / `.seg button…`）的总数上限：变体 `.seg.xxx` 不计入。
       加规则不是罪；无节制加才是 —— 超过上限请先想清楚是不是该用变体。 */
    const n = (src.match(/^\.seg(?![.\w-])/gm) || []).length;
    if (n > 6) {
      throw new Error("纯 `.seg` 规则数 = " + n + "（上限 6）—— 新增的布局覆盖请改成变体 `.seg.xxx`");
    }
    const own = (src.match(/^\.gw-type-seg/gm) || []).length;
    if (own !== 4) throw new Error("`.gw-type-seg` 规则数 = " + own + "，期望 4");
    return "inline-flex 未变 · 纯 .seg " + n + "/6 · .gw-type-seg 4";
  });

  /* ════════ B · 统一网关类型 ════════ */
  const domB = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dB = domB.window.document;
  const winB = domB.window;

  check("统一网关类型：子导航 = 概览 / 命名空间 / 密钥 / 账号池（接入说明已并回概览）", () => {
    if (activeSeg(dB) !== "统一网关") throw new Error("选中：" + activeSeg(dB));
    const subs = subLabels(dB);
    if (subs.join("/") !== "概览/命名空间/密钥/账号池") throw new Error("子导航：" + subs.join("/"));
    return subs.join(" / ");
  });

  check("统一网关页头**不挂** scope-chip，改挂「跨全部作用域」徽标", () => {
    if (dB.querySelector(".page-head .scope-chip")) throw new Error("跨作用域页仍挂着 scope-chip（会误导）");
    const badge = dB.querySelector(".page-head .badge");
    if (!badge || !flat(badge.textContent).includes("跨全部作用域")) throw new Error("徽标：" + flat((badge || {}).textContent));
    return flat(badge.textContent);
  });

  check("★ 概览 = 两张卡「服务状态」+「接入说明」（与独立网关的概览同形）", () => {
    const heads = cardHeads(dB);
    if (heads.join("/") !== "服务状态/接入说明") throw new Error("概览的卡：" + heads.join("/"));
    return heads.join(" / ");
  });

  check("★ 概览的配置项**全是 `.srow` 行**（不再用自造摘要条）", () => {
    const rows = srowLabels(dB);
    for (const want of ["启用统一网关", "接入地址", "监听地址", "端口", "命名空间", "账号池", "裸模型名"]) {
      if (!rows.includes(want)) throw new Error("缺 `.srow` 行「" + want + "」，实际：" + rows.join("/"));
    }
    return rows.join(" / ");
  });

  check("★ 全页**没有自造容器**（`.uni-note` / `.uni-sec` / `.uni-sum` / `.uni-select` 全为 0）", () => {
    for (const cls of ["uni-note", "uni-sec", "uni-sum", "uni-select"]) {
      const n = dB.querySelectorAll("." + cls).length;
      if (n) throw new Error("仍有 " + n + " 个 ." + cls);
    }
    const src = fs.readFileSync(FILE, "utf8");
    const css = (src.match(/^\.uni-(note|sec|sum|select)[ {]/gm) || []).length;
    if (css) throw new Error("CSS 里还留着 " + css + " 条自造容器规则");
    return "DOM 0 个 · CSS 0 条";
  });

  check("概览分区有结论徽标 = 「部分不可用」（阳性对照：默认启用集里确实有 1 段空池）", () => {
    const ids = JSON.parse(
      winB.eval("JSON.stringify(UNI_NS.filter(x=>UNI.namespaces.has(x.id)&&uniPoolOf(x.id).available===0).map(x=>x.id))"),
    );
    /* ★ 阳性对照：默认启用集是 6 条派生层，其中**只有 trae-global 的池是空的**
       （4 条扩展程序线默认不启用）。这个数必须是 1，否则断言在别的数据下会假绿。 */
    if (ids.length !== 1 || ids[0] !== "trae-global") throw new Error("阳性对照失败：" + ids.join(","));
    const badge = dB.querySelector(".split-main .card-head .badge");
    const label = flat(badge.textContent);
    if (!label.includes("部分不可用") || !label.includes("1")) throw new Error("结论徽标：「" + label + "」");
    return label + " ← " + ids.join(",");
  });

  check("概览分区：监听地址是 select.inp（既有类），选局域网要两步确认且取消会还原", () => {
    const sel = dB.querySelector("select[data-uni-bind]");
    if (!sel) throw new Error("监听地址不是 <select>");
    if (!sel.classList.contains("inp")) throw new Error("没有复用既有的 .inp 类：" + sel.className);
    const before = winB.eval("UNI.bind");
    sel.value = "0.0.0.0";
    sel.dispatchEvent(new winB.Event("change", { bubbles: true }));
    if (!dB.querySelector("#confirm-overlay").classList.contains("open")) throw new Error("开放局域网没有弹危险确认");
    if (winB.eval("UNI.bind") !== before) throw new Error("确认前就已生效");
    click(winB, dB.querySelector('[data-act="confirm-cancel"]'));
    const sel2 = dB.querySelector("select[data-uni-bind]");
    if (sel2.value !== before) throw new Error("取消后下拉仍显示 " + sel2.value + "，没还原");
    return "select.inp + 确认 + 取消还原（" + before + "）";
  });

  check("概览分区：端口提示列全既有端口（界面/WB/统一/Trae/Qoder/hook）", () => {
    const t = flat(dB.querySelector("#content").textContent);
    for (const p of ["57890", "57891", "57892", "57893", "57894", "57895"]) {
      if (!t.includes(p)) throw new Error("端口提示漏了 " + p);
    }
    return "57890 / 57891 / 57892 / 57893 / 57894 / 57895";
  });

  /* ── 命名空间分区 ── */
  clickSub(winB, dB, "u-ns");
  check("★ 命名空间分区 = cat-bar 工具条 + 裸表 + 说明段（与独立网关的「模型」分区同形）", () => {
    const bar = dB.querySelector(".split-main .cat-bar");
    if (!bar) throw new Error("没有 cat-bar 工具条");
    if (!flat(bar.textContent).includes("已启用")) throw new Error("工具条内容不对：" + flat(bar.textContent));
    if (!dB.querySelector(".split-main > .tbl-wrap")) throw new Error("表不是裸表（应有 .tbl-wrap）");
    const note = [...dB.querySelectorAll(".split-main > p.tiny.muted")].map((p) => flat(p.textContent));
    if (!note.length) throw new Error("没有说明段");
    return flat(bar.textContent).slice(0, 50) + " · 裸表 · 说明段";
  });

  check("命名空间表有「账号池」与「可用性」两列", () => {
    const th = [...dB.querySelectorAll("table.tbl thead th")].map((e) => e.textContent.trim());
    for (const want of ["账号池", "可用性"]) {
      if (!th.includes(want)) throw new Error("缺列「" + want + "」：" + th.join("/"));
    }
    return "账号池 / 可用性 均在";
  });

  check("wb-cn 行的池数字 = 从 GATEWAY 池里数出来的值（不写死数字）", () => {
    /* ★ 期望值**从页面里的原始池数据现算**，不再写死「2 可用 / 共 4」：
       写死数字的断言与它自己要守的性质（「不许把数字写死」）正好相反 ——
       演示数据一改，它就会报一个与被测行为无关的红（2026-10-17 实测踩到）。
       仍然有效：这里按「排除项不算可用」重算一遍，与 `uniPoolOf` 的结果比对，
       两边任一侧漂移都会红。 */
    const raw = JSON.parse(winB.eval('JSON.stringify(GATEWAY["wb-cn"].pool)'));
    const expectAvailable = raw.filter((x) => !x.excluded && x.state === "可用").length;
    const e = JSON.parse(winB.eval("JSON.stringify(uniPoolOf('wb-cn'))"));
    if (e.available !== expectAvailable || e.total !== raw.length) {
      throw new Error(`池数字与原始池数据不符：${JSON.stringify(e)} vs 期望 ${expectAvailable}/${raw.length}`);
    }
    const tr = rowOf(dB, "wb-cn");
    if (!tr) throw new Error("表里找不到 wb-cn 行");
    const cell = flat(tr.children[4].textContent);
    if (!cell.includes(`${expectAvailable} 可用`) || !cell.includes(`共 ${raw.length} 个`)) {
      throw new Error("账号池格不对：" + cell);
    }
    return cell;
  });

  check("trae-global 行 = 「无账号」+「去加账号」出口", () => {
    const e = JSON.parse(winB.eval("JSON.stringify(uniPoolOf('trae-global'))"));
    if (e.total !== 0) throw new Error("阳性对照失败：trae-global 池不是空的");
    const tr = rowOf(dB, "trae-global");
    const cell = flat(tr.children[4].textContent);
    const health = flat(tr.children[5].textContent);
    if (!cell.includes("0 可用") || !cell.includes("去加账号")) throw new Error("空池格不对：" + cell);
    if (health !== "无账号") throw new Error("可用性不是「无账号」而是「" + health + "」");
    return cell + " → " + health;
  });

  check("勾选区每项都显示客户端名", () => {
    const labels = [...dB.querySelectorAll(".uni-ns")];
    const missing = labels.filter((l) => !flat((l.querySelector(".uni-ns-name") || {}).textContent));
    if (missing.length) throw new Error(missing.length + " 项没有客户端名");
    return labels.length + " 项都有";
  });

  check("点「看池」就地展开：不切作用域、不换分区，页内出现池明细表", () => {
    const btn = [...dB.querySelectorAll("[data-act^='uni-pool-toggle|']")].find((b) => b.dataset.act === "uni-pool-toggle|wb-cn");
    if (!btn) throw new Error("找不到 wb-cn 的「看池」按钮");
    if (btn.getAttribute("aria-expanded") !== "false") throw new Error("初始 aria-expanded 不是 false");
    click(winB, btn);
    if (winB.eval("state.gwType") !== "unified") throw new Error("看池把网关类型切走了");
    if (winB.eval("state.gwSub") !== "u-ns") throw new Error("看池把分区切走了：" + winB.eval("state.gwSub"));
    if (winB.eval("state.scope") !== "wb-cn") throw new Error("看池把作用域切走了");
    if (!dB.querySelector(".uni-pool-detail") || !dB.querySelector(".uni-pool-tbl")) throw new Error("没有出现池明细");
    const btn2 = [...dB.querySelectorAll("[data-act^='uni-pool-toggle|']")].find((b) => b.dataset.act === "uni-pool-toggle|wb-cn");
    if (btn2.getAttribute("aria-expanded") !== "true") throw new Error("展开后 aria-expanded 不是 true");
    return "就地展开 · aria-expanded 同步 · 类型/分区/作用域都没变";
  });

  check("次要出口「去 xxx 的网关页」切到独立网关类型 + 该作用域的 pool 分区", () => {
    const btn = [...dB.querySelectorAll("[data-act^='uni-pool-go|']")].find((b) => b.dataset.act === "uni-pool-go|trae-global");
    if (!btn) throw new Error("找不到深链按钮");
    click(winB, btn);
    if (winB.eval("state.scope") !== "trae-global") throw new Error("作用域没切过去：" + winB.eval("state.scope"));
    if (winB.eval("state.gwType") !== "independent") throw new Error("没切回独立网关类型：" + winB.eval("state.gwType"));
    if (winB.eval("state.gwSub") !== "pool") throw new Error("没落到 pool 分区：" + winB.eval("state.gwSub"));
    return "trae-global / independent / pool";
  });

  /* ── 账号池分区 ── */
  const domP = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dP = domP.window.document;
  const winP = domP.window;
  clickSub(winP, dP, "u-pool");

  check("★ 账号池分区 = cat-bar + 按池分组的裸表 + 说明段（与独立网关的账号池分区同形）", () => {
    const bar = dP.querySelector(".split-main .cat-bar");
    if (!bar) throw new Error("没有 cat-bar 工具条");
    if (!dP.querySelector(".split-main > .tbl-wrap")) throw new Error("表不是裸表");
    if (![...dP.querySelectorAll(".split-main > p.tiny.muted")].length) throw new Error("没有说明段");
    return flat(bar.textContent).slice(0, 60);
  });

  check("★ 分组 = 已启用命名空间**去重后**的池（阳性对照：6 条派生层 → 6 个池，每条一个）", () => {
    const g = JSON.parse(
      winP.eval(
        "JSON.stringify([...new Set(UNI_NS.filter(x=>UNI.namespaces.has(x.id)).map(x=>uniPoolOf(x.id).target))])",
      ),
    );
    /* ★ 阳性对照：默认启用集是 6 条派生层，**每条映射到自己的池**（wb-cn / wb-global /
       trae-cn / trae-global / qoder-cn / qoder-global）⇒ 去重后仍是 6。
       ⚠️ 若有人把 `traework-*` 也默认启用，去重后仍应是 6（它们复用 Trae 的两个池）——
       这个数写死成 6 是**刻意的**：它同时钉住「扩展层复用 Trae 池」这条映射。 */
    if (g.length !== 6) throw new Error("阳性对照失败：去重后池数 = " + g.length + "（" + g.join(",") + "）");
    const grp = [...dP.querySelectorAll("tr.uni-grp")].map((tr) => tr.querySelector("code").textContent.trim());
    if (grp.join("/") !== g.join("/")) throw new Error("表里分组 = " + grp.join("/") + "，期望 " + g.join("/"));
    return grp.join(" / ");
  });

  check("★ 账号行**逐条数出来**（行数 = 各池 pool 长度之和，不另写一套）", () => {
    const expect = winP.eval(
      "['wb-cn','wb-global','trae-cn','trae-global','qoder-cn','qoder-global'].reduce((n,s)=>n+((GATEWAY[s]||{}).pool||[]).length,0)",
    );
    const rows = [...dP.querySelectorAll("table.tbl tbody tr")].filter(
      (tr) => !tr.classList.contains("uni-grp") && tr.querySelector("td b"),
    );
    if (rows.length !== expect) throw new Error("账号行 " + rows.length + " 行，期望 " + expect);
    return rows.length + " 行（= 各池之和）";
  });

  check("空池分组有可诊断文案 + 「去该平台管理」出口（阳性对照：trae-global 池确实为空）", () => {
    if (JSON.parse(winP.eval("JSON.stringify(uniPoolOf('trae-global'))")).total !== 0) {
      throw new Error("阳性对照失败：trae-global 池不是空的");
    }
    const trs = [...dP.querySelectorAll("table.tbl tbody tr")];
    const i = trs.findIndex((tr) => tr.classList.contains("uni-grp") && flat(tr.textContent).includes("trae-global"));
    if (i < 0) throw new Error("找不到 trae-global 分组");
    const next = flat(trs[i + 1].textContent);
    if (!next.includes("一个账号都没有")) throw new Error("空池没有可诊断文案：" + next);
    if (!dP.querySelector('[data-act="uni-pool-go|trae-global"]')) throw new Error("空池分组没有「去该平台管理」出口");
    return "trae-global → 文案 + 出口";
  });

  const domQ = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dQ = domQ.window.document;
  check("★ 概览的「账号池」行深链到账号池分区（不是命名空间分区）", () => {
    const row = [...dQ.querySelectorAll(".split-main .srow")].find((r) => flat(r.querySelector(".lab b").textContent) === "账号池");
    if (!row) throw new Error("概览没有「账号池」行");
    click(domQ.window, row.querySelector("[data-act]"));
    if (domQ.window.eval("state.gwSub") !== "u-pool") throw new Error("没落到账号池分区：" + domQ.window.eval("state.gwSub"));
    return "gw-sub|u-pool";
  });

  /* ── 密钥分区 ── */
  const domC = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dC = domC.window.document;
  clickSub(domC.window, dC, "u-keys");
  check("★ 密钥分区 = cat-bar 工具条 + 裸表 + 说明段（与独立网关同形）", () => {
    const bar = dC.querySelector(".split-main .cat-bar");
    if (!bar) throw new Error("没有工具条");
    if (!dC.querySelector('[data-act="uni-key-new"]')) throw new Error("工具条没有创建密钥出口");
    if (!dC.querySelector(".split-main > .tbl-wrap")) throw new Error("表不是裸表");
    if (![...dC.querySelectorAll(".split-main > p.tiny.muted")].length) throw new Error("没有说明段");
    return flat(bar.textContent).slice(0, 50) + " · 裸表 · 说明段";
  });

  /* ── 接入说明与独立网关共用协议 Tab ── */
  const domD = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dD = domD.window.document;
  check("★ 接入说明的协议 Tab 与独立网关**共用同一个状态**（切过去再切回来不跳回默认）", () => {
    const anthropic = [...dD.querySelectorAll("[data-act^='gw-api|']")].find((b) => b.dataset.act === "gw-api|anthropic");
    if (!anthropic) throw new Error("接入说明没有协议 Tab");
    click(domD.window, anthropic);
    if (domD.window.eval("state.gwApi") !== "anthropic") throw new Error("gwApi 没变：" + domD.window.eval("state.gwApi"));
    const t = flat(dD.querySelector("#content").textContent);
    if (!t.includes("/v1/messages")) throw new Error("没换成 Anthropic 的代码样例");
    /* 切到独立网关类型 —— 协议 Tab 必须**还是** Anthropic（同一个 state） */
    click(domD.window, [...dD.querySelectorAll("[data-act^='gw-type|']")].find((b) => b.dataset.act === "gw-type|independent"));
    if (domD.window.eval("state.gwApi") !== "anthropic") throw new Error("切类型后 gwApi 被重置了");
    const active = flat((dD.querySelector(".split-main .card-head .seg button.on") || {}).textContent);
    if (active !== "Anthropic 兼容") throw new Error("独立网关的协议 Tab 不是 Anthropic 而是「" + active + "」");
    return "unified → independent 保持 Anthropic";
  });

  /* ── 未启用态 ── */
  const domE = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dE = domE.window.document;
  check("关掉开关 ⇒ 概览出现「启用后生效」提示，结论徽标变「未启用」", () => {
    click(domE.window, dE.querySelector('[data-act="uni-toggle"]'));
    const t = flat(dE.querySelector("#content").textContent);
    if (!t.includes("启用后生效")) throw new Error("没有「启用后生效」提示");
    const badge = flat(dE.querySelector(".split-main .card-head .badge").textContent);
    if (badge !== "未启用") throw new Error("结论徽标是「" + badge + "」");
    return "未启用 + 提示到位";
  });

  /* ── 切类型重置分区 ── */
  const domF = await boot("scope=wb-cn&dest=gateway&type=unified&hint=0");
  const dF = domF.window.document;
  check("从统一网关切回独立网关：分区重置为「概览」，且页头换回 scope-chip", () => {
    click(domF.window, [...dF.querySelectorAll("[data-act^='gw-type|']")].find((b) => b.dataset.act === "gw-type|independent"));
    if (domF.window.eval("state.gwSub") !== "overview") throw new Error("分区没重置：" + domF.window.eval("state.gwSub"));
    if (!dF.querySelector(".page-head .scope-chip")) throw new Error("独立网关页头缺 scope-chip");
    return "gwSub=overview + scope-chip 回归";
  });

  /* ── 旧深链兼容（两代） ── */
  const domG = await boot("scope=wb-cn&dest=gateway&gw=unified&hint=0");
  check("旧深链 ① ?dest=gateway&gw=unified ⇒ 统一网关类型", () => {
    if (domG.window.eval("state.gwType") !== "unified") throw new Error("gwType = " + domG.window.eval("state.gwType"));
    return "兼容";
  });

  const domH = await boot("scope=wb-cn&dest=unified&hint=0");
  check("旧深链 ② ?dest=unified ⇒ 归位到网关页的统一网关类型", () => {
    if (domH.window.eval("state.dest") !== "gateway") throw new Error("dest = " + domH.window.eval("state.dest"));
    if (domH.window.eval("state.gwType") !== "unified") throw new Error("gwType = " + domH.window.eval("state.gwType"));
    return "兼容";
  });

  /* ── 深链：`gw` 直接给统一网关的分区 id ── */
  const domR = await boot("scope=wb-cn&dest=gateway&type=unified&gw=u-pool&hint=0");
  check("★ 深链 ?type=unified&gw=u-pool 直接落到账号池分区（不被前置分支写成 u-ov）", () => {
    if (domR.window.eval("state.gwSub") !== "u-pool") throw new Error("gwSub = " + domR.window.eval("state.gwSub"));
    if (domR.window.eval("state.gwType") !== "unified") throw new Error("gwType = " + domR.window.eval("state.gwType"));
    if (!flat(domR.window.document.querySelector("#content").textContent).includes("个池")) throw new Error("没渲染账号池分区");
    return "u-pool";
  });

  /* ── 失败态深链 ── */
  const domI = await boot("scope=wb-cn&dest=gateway&type=unified&err=unified&hint=0");
  check("深链 ?err=unified 显示最近失败原因，且点名命名空间", () => {
    const t = flat(domI.window.document.querySelector("#content").textContent);
    if (!t.includes("最近一次请求失败")) throw new Error("没显示失败块");
    if (!t.includes("trae-global")) throw new Error("失败原因没点名命名空间");
    return "失败块 + 点名命名空间";
  });

  /* ── 汇总 ── */
  const failed = results.filter((r) => !r.ok);
  for (const r of results) {
    console.log(`  ${r.ok ? "ok  " : "FAIL"} ${r.name}${r.detail ? "  → " + r.detail : ""}`);
  }
  console.log(`\n${results.length - failed.length} passed / ${failed.length} failed`);
  if (MUTATE) {
    console.log(failed.length ? "变异生效：断言确实打红了 ✓" : "★ 变异未打红 —— 断言在空转，视为失败");
    process.exit(failed.length ? 0 : 1);
  }
  process.exit(failed.length ? 1 : 0);
})();
