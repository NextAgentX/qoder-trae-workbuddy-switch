/* 账号池：选号解释 + 排除账号 + 模型级暂停 / 半开试探（2026-10-17）。
 *
 * ## 为什么这些断言值得写
 *
 * 本轮改动的核心不是「多了一个按钮」，而是**让选号结论可见**：
 *   1. 五态计数**互斥且穷尽**（排除算独立一态，绝不能再混进「可用」）；
 *   2. 「本次请求会选中谁 / 为什么选不出」是**从池数据派生**的，不是写死的文案
 *      —— 改一个开关、排除一个号，结论必须跟着变；
 *   3. 模型级暂停 → 半开试探 → 恢复，是一条**可点通**的闭环（不是三张静态截图）。
 *
 * ⇒ 断言必须打在「结论随状态变化」上。只断言「按钮存在」等于什么都没测。
 *
 * 与既有门禁同一立场：只跑单文件；变异只写 `os.tmpdir()` 副本；变异模式下**全绿即失败**。
 *
 * 用法：
 *   NODE_PATH=<repo>/.tmp-verify/node_modules node docs/prototype/verify-pool-exclude.cjs
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
  excludedignored: {
    desc: "排除不影响选号（跳过判据不认 excluded）",
    from: '  if (x.excluded) return { code: "excluded", text: "已排除（用户设置）" };',
    to: '  /* MUTATED */',
  },
  countincludes: {
    desc: "把已排除的号算进「可用」（数字与事实不符）",
    from: "    if (x.excluded) c.excluded += 1;\n    else if (x.state === \"冷却\") c.cooling += 1;",
    to: "    if (x.state === \"冷却\") c.cooling += 1; /* MUTATED */",
  },
  probekeeps: {
    desc: "半开试探成功但不清该模型的冷却（表里还在冷却，结论却说恢复）",
    from: "        if (x.modelCool) x.modelCool = x.modelCool.filter((c) => c.model !== gate.model);",
    to: "        /* MUTATED */",
  },
  payalways: {
    desc: "收费兜底开关失效（默认就用收费号）",
    from: "  if (bestTier === 2 && !payFallback) {",
    to: "  if (false) {",
  },
  stickystays: {
    desc: "排除时不解除粘性绑定（粘性徽标挂在已排除的行上）",
    from: "        const wasSticky = Boolean(row.sticky);\n        row.sticky = false;",
    to: "        const wasSticky = Boolean(row.sticky); /* MUTATED */",
  },
  gateignored: {
    desc: "模型级暂停不短路（每个请求仍逐个账号撞墙）",
    from: "  if (gate && gate.model === model && gate.phase === \"paused\") {",
    to: "  if (false) {",
  },
  maxcreditsignored: {
    desc: "「按实时积分择优」失效（仍按权重选）",
    from: "  const ordered = layer.slice().sort((a, b) => (mode === \"max_credits\" ? b.credits - a.credits : b.weight - a.weight));",
    to: "  const ordered = layer.slice().sort((a, b) => b.weight - a.weight); /* MUTATED */",
  },
  randomignored: {
    desc: "「随机」档失效（退回按权重选）",
    from: "  if (mode === \"random\") {",
    to: "  if (false) {",
  },
  noretry: {
    desc: "失败后不换号（第一跳失败就整轮结束）",
    from: "      continue;\n    }\n    steps.push({ kind: \"ok\", account: acc.account",
    to: "      break; /* MUTATED */\n    }\n    steps.push({ kind: \"ok\", account: acc.account",
  },
};

let DIR = BASE;
if (MUTATE) {
  if (!MUTATIONS[MUT]) {
    console.error("未知变异：" + MUT + "（可选：" + Object.keys(MUTATIONS).join(" / ") + "）");
    process.exit(2);
  }
  const m = MUTATIONS[MUT];
  DIR = fs.mkdtempSync(path.join(os.tmpdir(), "bspool-"));
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

const click = (win, el) => el.dispatchEvent(new win.MouseEvent("click", { bubbles: true }));
const flat = (s) => String(s || "").replace(/\s+/g, " ").trim();
const act = (d, name) => [...d.querySelectorAll("[data-act]")].find((b) => b.dataset.act === name);
const text = (dom) => flat(dom.window.document.querySelector("#content").textContent);

/* 池分区里「已排除」那一格（kpi 第二行文字）：从 `.kpi-row` 里按标签取数，不猜顺序。 */
function tile(d, label) {
  const box = [...d.querySelectorAll("#content .kpi-row > div")].find(
    (x) => flat(x.querySelector("span").textContent) === label
  );
  if (!box) throw new Error("没有「" + label + "」计数格");
  return Number(flat(box.querySelector("b").textContent));
}

/* 选号结论那一行（`.pool-explain` 的第一段）。 */
function verdictLine(d) {
  const box = d.querySelector("#content .pool-explain");
  if (!box) throw new Error("没有选号解释块");
  return flat(box.textContent);
}

/* 「这一轮请求的换号链路」块（点「模拟请求」后出现）。它和选号结论**共用** `.pool-explain` 样式，
   所以不能按位置取，必须按内容认。 */
function chainText(d) {
  const box = [...d.querySelectorAll("#content .pool-explain")].find((x) => flat(x.textContent).includes("换号链路"));
  return box ? flat(box.textContent) : "";
}

(async () => {
  /* ── ① 五态计数互斥且穷尽 ── */
  const domA = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  const dA = domA.window.document;
  check("五态计数：可用 4 · 已排除 1，五格相加 = 条目数 5", () => {
    if (domA.__errors.length) throw new Error("jsdom 报错：" + domA.__errors[0]);
    const sum = ["可用", "冷却", "禁用", "零积分", "已排除"].reduce((n, k) => n + tile(dA, k), 0);
    if (sum !== 5) throw new Error("五格相加 = " + sum + "，应为 5");
    if (tile(dA, "可用") !== 4) throw new Error("可用 = " + tile(dA, "可用") + "（已排除的号不能算进可用）");
    if (tile(dA, "已排除") !== 1) throw new Error("已排除 = " + tile(dA, "已排除"));
    return "4 + 0 + 0 + 0 + 1 = 5";
  });

  check("已排除的行：整行变暗 + 徽标 + 「指定」按钮禁用（硬约束在 UI 上先挡住）", () => {
    const row = [...dA.querySelectorAll("#content table.tbl tbody tr")].find((r) => flat(r.textContent).includes("design-lab"));
    if (!row) throw new Error("找不到 design-lab 行");
    if (!row.classList.contains("row-off")) throw new Error("行没有 row-off");
    if (!flat(row.textContent).includes("已排除")) throw new Error("没有「已排除」徽标");
    const pin = [...row.querySelectorAll("button")].find((b) => flat(b.textContent) === "指定");
    if (!pin || !pin.disabled) throw new Error("「指定」按钮没有禁用");
    return "row-off + 徽标 + 指定禁用";
  });

  /* ── ② 模型级暂停短路（默认状态） ── */
  check("默认 gpt-5-codex：模型级暂停 ⇒ 结论是「选不出账号」，且点名暂停原因", () => {
    const t = verdictLine(dA);
    if (!t.includes("选不出账号")) throw new Error("结论不是「选不出」：" + t);
    if (!t.includes("整体暂停")) throw new Error("没点名模型级暂停：" + t);
    return t.slice(0, 60);
  });

  check("跳过清单三条原因各就各位：模型冷却 / 在途满 / 已排除", () => {
    const t = verdictLine(dA);
    if (!t.includes("team-shared") || !t.includes("模型冷却")) throw new Error("缺「模型冷却」那条");
    if (!t.includes("backup-01") || !t.includes("在途")) throw new Error("缺「在途满」那条");
    if (!t.includes("design-lab") || !t.includes("已排除")) throw new Error("缺「已排除」那条");
    return "3 条跳过原因";
  });

  /* ── ③ 半开试探 → 恢复 ── */
  const domB = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  const dB = domB.window.document;
  check("点「立即试探」⇒ 暂停条变「已恢复」，结论立刻变成「会选中 jackey」", () => {
    click(domB.window, act(dB, "pool-probe|gpt-5-codex"));
    const t = text(domB);
    if (!t.includes("已恢复")) throw new Error("暂停条没变「已恢复」");
    const v = verdictLine(dB);
    if (!v.includes("会选中 jackey")) throw new Error("结论没变成命中：" + v);
    return v.slice(0, 50);
  });

  check("恢复后该模型的冷却被清掉（表里不再挂「模型冷却」徽标）", () => {
    const t = text(domB);
    if (t.includes("gpt-5-codex · ")) throw new Error("表里仍挂着 gpt-5-codex 的冷却");
    return "冷却已清";
  });

  check("点「重置」⇒ 回到暂停态（原型可反复演示，不必刷新）", () => {
    click(domB.window, act(dB, "pool-reset"));
    if (!text(domB).includes("整体暂停")) throw new Error("没有回到暂停态");
    return "已重置";
  });

  /* ── ④ 换号重试：与策略无关的统一链路 ──
     用户诉求原话：「不管什么模式，请求失败需要有重试机制，识别到账号或模型被限制就换其他账号」。
     这里钉三件事：① 闸门暂停时**不浪费任何一跳**；② 失败后**确实换了另一个号并成功**；
     ③ 试过的号在下一跳被排除（不是把同一堵墙撞 3 次）。 */
  const domC = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  const dC = domC.window.document;
  check("闸门暂停时点「模拟请求」⇒ 一步都不发，直接返回「整体暂停」", () => {
    click(domC.window, act(dC, "pool-simulate"));
    const c = chainText(dC);
    if (!c.includes("停")) throw new Error("链路没有「停」这一步：" + c);
    if (!c.includes("整体暂停")) throw new Error("没说明是模型闸门：" + c);
    if (c.includes("429")) throw new Error("暂停时不该再发请求（却出现了 429）");
    return c.slice(0, 46);
  });

  check("★ 试探恢复后点「模拟请求」⇒ 第 1 跳 429 → 写冷却 → 换号 → 第 2 跳成功", () => {
    click(domC.window, act(dC, "pool-probe|gpt-5-codex"));
    click(domC.window, act(dC, "pool-simulate"));
    const c = chainText(dC);
    if (!c.includes("jackey") || !c.includes("429")) throw new Error("第 1 跳没有按预期失败：" + c);
    if (!c.includes("写模型冷却")) throw new Error("失败后没写冷却：" + c);
    if (!c.includes("换号")) throw new Error("没有「换号」这个动作：" + c);
    if (!c.includes("team-shared") || !c.includes("200 OK")) throw new Error("没有换到另一个号成功：" + c);
    return c.slice(0, 90);
  });

  check("★ 失败的那一跳真的被冷却了：表里 jackey 出现模型冷却，且再模拟时第一跳直接换人", () => {
    const row = [...dC.querySelectorAll("#content table.tbl tbody tr")].find((r) => flat(r.textContent).includes("jackey"));
    if (!row || !flat(row.textContent).includes("gpt-5-codex")) throw new Error("jackey 没被写冷却");
    click(domC.window, act(dC, "pool-simulate"));
    const c = chainText(dC);
    if (c.includes("jackey")) throw new Error("第二轮仍从 jackey 开始（已冷却的号应被跳过）：" + c);
    if (!c.includes("team-shared")) throw new Error("第二轮没直接命中 team-shared：" + c);
    return "第 2 轮跳过 jackey";
  });

  /* ── ⑤ 排除：影响结论 / 清粘性 / 与指定互斥 ── */
  const domD = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  const dD = domD.window.document;
  check("先指定 jackey 再排除它 ⇒ 指定被自动取消（两条规则不能同时成立）", () => {
    click(domD.window, act(dD, "pin|jackey"));
    if (domD.window.eval("state.pins[state.scope]") !== "jackey") throw new Error("前置：指定失败");
    click(domD.window, act(dD, "pool-exclude|jackey"));
    if (domD.window.eval("state.pins[state.scope]")) throw new Error("排除后指定没被取消");
    /* toast 在 `#toasts` 里，不在 `#content` —— 断言要打到真正承载文案的容器上。 */
    const t = flat(dD.querySelector("#toasts").textContent);
    if (!t.includes("已排除 jackey")) throw new Error("没有提示「已排除」：" + t);
    if (!t.includes("取消指定")) throw new Error("提示没说「已同时取消指定」：" + t);
    return "指定已取消 + 提示到位";
  });

  check("排除时按 uid 清粘性：jackey 的「粘性」徽标消失（否则每请求白跑一跳）", () => {
    const row = [...dD.querySelectorAll("#content table.tbl tbody tr")].find((r) => flat(r.textContent).includes("jackey"));
    if (!row) throw new Error("找不到 jackey 行");
    if (flat(row.textContent).includes("粘性")) throw new Error("粘性徽标还在");
    return "粘性已清";
  });

  check("排除后计数联动：可用 3 · 已排除 2", () => {
    if (tile(dD, "可用") !== 3) throw new Error("可用 = " + tile(dD, "可用"));
    if (tile(dD, "已排除") !== 2) throw new Error("已排除 = " + tile(dD, "已排除"));
    return "3 / 2";
  });

  check("取消排除 ⇒ 恢复参与选号（可用回到 4）", () => {
    click(domD.window, act(dD, "pool-exclude|jackey"));
    if (tile(dD, "可用") !== 4) throw new Error("可用 = " + tile(dD, "可用"));
    return "已恢复";
  });

  /* ── ⑤b 三档策略：不指定 / 按实时积分择优 / 指定账号 ──
     ★ 这三档在真实后端是**同一个枚举** `AccountStrategy{Current, Pinned, MaxCredits}`，
       所以它们必须待在**同一个下拉**里、互斥。上一轮重构工具条时把整个下拉删掉了
       （只留下行内「指定」按钮）⇒ `max_credits` 这一档在原型里消失，属于回归。
     用 trae-cn 做这条断言：jackey 与 code-lab **权重并列 100、积分 880 / 1320**
       ⇒ auto 取先出现者 jackey，max_credits 必须改选 code-lab，差别才可测。 */
  const domG = await boot("scope=trae-cn&dest=gateway&gw=pool&hint=0");
  const dG = domG.window.document;
  check("「指定账号」下拉在，且四档齐全（不指定 / 积分择优 / 随机 / 账号列表）", () => {
    const sel = dG.querySelector("[data-gw-pin]");
    if (!sel) throw new Error("没有「指定账号」下拉（被工具条重构删掉了）");
    const labels = [...sel.options].map((o) => flat(o.textContent));
    if (!labels.includes("不指定（自动择优）")) throw new Error("缺「不指定」：" + labels.join(" / "));
    if (!labels.some((x) => x.startsWith("按实时积分择优"))) throw new Error("缺「按实时积分择优」：" + labels.join(" / "));
    if (!labels.some((x) => x.startsWith("随机"))) throw new Error("缺「随机」档：" + labels.join(" / "));
    if (!labels.includes("jackey")) throw new Error("缺账号项：" + labels.join(" / "));
    return labels.join(" | ");
  });

  check("切到「按实时积分择优」⇒ 结论改按积分（权重并列时积分高的 code-lab 胜出）", () => {
    if (!verdictLine(dG).includes("会选中 jackey")) throw new Error("前置：auto 没选中 jackey → " + verdictLine(dG));
    const sel = dG.querySelector("[data-gw-pin]");
    sel.value = "__max_credits__";
    sel.dispatchEvent(new domG.window.Event("change", { bubbles: true }));
    const v = verdictLine(dG);
    if (!v.includes("会选中 code-lab")) throw new Error("没按积分择优：" + v);
    if (!v.includes("按实时积分择优")) throw new Error("结论没说明当前模式：" + v);
    return "jackey → code-lab（1320 积分）";
  });

  check("★ 切到「随机」⇒ 结论说明是均匀抽签，且命中者必在候选里（不看权重）", () => {
    const sel = dG.querySelector("[data-gw-pin]");
    sel.value = "__random__";
    sel.dispatchEvent(new domG.window.Event("change", { bubbles: true }));
    if (domG.window.eval("state.gwMode[state.scope]") !== "random") throw new Error("策略没切成 random");
    const v = verdictLine(dG);
    if (!v.includes("随机模式")) throw new Error("结论没说明是随机模式：" + v);
    if (!/会选中 (jackey|code-lab|solo-test)/.test(v)) throw new Error("命中者不在候选里：" + v);
    return v.slice(0, 60);
  });

  check("行内「指定」是策略的第四个取值：指定后下拉同步，且不再是积分/随机档", () => {
    click(domG.window, act(dG, "pin|jackey"));
    const sel = dG.querySelector("[data-gw-pin]");
    if (sel.value !== "jackey") throw new Error("下拉没同步到 jackey，实际 " + sel.value);
    if (domG.window.eval("state.gwMode[state.scope]") !== "pinned") throw new Error("模式不是 pinned");
    return "sel=jackey · mode=pinned";
  });

  const domH = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  check("已排除的账号在下拉里置灰（不能「指定一个永不使用的号」）", () => {
    const sel = domH.window.document.querySelector("[data-gw-pin]");
    const opt = [...sel.options].find((o) => flat(o.textContent).startsWith("design-lab"));
    if (!opt) throw new Error("下拉里没有 design-lab 这一项");
    if (!opt.disabled) throw new Error("已排除的账号没置灰");
    if (!flat(opt.textContent).includes("已排除")) throw new Error("没标注「已排除」");
    return "design-lab（已排除）disabled";
  });

  /* ── ⑥ 收费兜底开关 ── */
  const domE = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  const dE = domE.window.document;
  check("只剩收费号时默认拒绝（不偷偷花积分），开关打开后才命中", () => {
    /* 先「立即试探」清掉模型冷却与闸门（模型下拉已删，不能再靠换模型绕过）。 */
    click(domE.window, act(dE, "pool-probe|gpt-5-codex"));
    /* 把两个免费号都排掉 ⇒ 候选只剩 qa-bot（paid）与 backup-01（paid，在途满） */
    click(domE.window, act(dE, "pool-exclude|jackey"));
    click(domE.window, act(dE, "pool-exclude|team-shared"));
    let v = verdictLine(dE);
    /* ★ 断言必须钉住**分支**而不是关键词：命中档的文案里也有「收费号」三个字，
       只查关键词的话「兜底失效」这个变异根本打不红（实测过）。 */
    if (!v.includes("选不出账号")) throw new Error("没有走「拒绝」这一档：" + v);
    if (!v.includes("收费号")) throw new Error("没说明「只剩收费号」：" + v);
    if (domE.window.eval("state.gwPayFallback[state.scope]")) throw new Error("开关默认不是关的");
    click(domE.window, act(dE, "pool-payfallback"));
    v = verdictLine(dE);
    if (v.includes("选不出账号")) throw new Error("开了兜底仍然拒绝：" + v);
    if (!v.includes("会选中 qa-bot")) throw new Error("开了兜底也没命中收费号：" + v);
    return "拒绝 → 打开 → qa-bot";
  });

  /* ── ⑥b 账号故障时自动切换（issue #129） ──
     ★ 钉住的是**存储粒度**：真实实现是三份 config 各一个同名布尔（按产品线），
     不是按 scope。写成按 scope 存的话，切到国际版开关会自己变回关 —— 那个 bug 在
     真机上根本不存在（纯属演示造出来的），反过来会误导下一个改这块的人。
     ⚠️ 这里刻意不切 scope 再断言（那要调 `renderContent()`，依赖它挂在 window 上）：
     直接钉「键是产品线、不是 scope」这个结构事实，等价且更稳。 */
  const domAF = await boot("scope=wb-cn&dest=gateway&gw=pool&hint=0");
  const dAF = domAF.window.document;
  check("账号故障时自动切换：默认关，且状态按产品线存（不是按 scope）", () => {
    const sw = dAF.querySelector('[data-act="pool-autofailover"]');
    if (!sw) throw new Error("工具条里没有这个开关");
    if (sw.getAttribute("aria-checked") !== "false") throw new Error("默认不是关");
    if (domAF.window.eval("!!state.gwAutoFailover.workbuddy")) throw new Error("状态默认不是关的");
    click(domAF.window, act(dAF, "pool-autofailover"));
    if (domAF.window.eval("state.gwAutoFailover.workbuddy") !== true) {
      throw new Error("点了没生效");
    }
    if (domAF.window.eval("Object.keys(state.gwAutoFailover).some((k) => k.includes('-'))")) {
      throw new Error("按 scope 存的（应落在产品线：WB 两个版本共用一份）");
    }
    return "默认关 → 打开 → key=workbuddy";
  });

  /* ── ⑦ 统一网关的池摘要仍与池数据同源 ── */
  const domF = await boot("scope=wb-cn&dest=gateway&type=unified&gw=u-pool&hint=0");
  check("统一网关的池明细：已排除不再算进「可用」（摘要条与池数据同源）", () => {
    const t = text(domF);
    if (!t.includes("4 可用")) throw new Error("摘要没有「4 可用」：" + t.slice(0, 120));
    if (!t.includes("1 已排除")) throw new Error("摘要没有「1 已排除」");
    return "4 可用 · 1 已排除";
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
