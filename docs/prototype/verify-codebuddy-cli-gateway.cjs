/* 原型验证：用 jsdom 真跑一遍交互，断言不是「看起来对」。
 *
 * 用法：
 *   NODE_PATH=<managed workspace>/node_modules node docs/prototype/verify-codebuddy-cli-gateway.cjs
 *   ... --mutate   注入一个变异（删掉 tab 绑定），**期望测试变红**，用于证明断言不空转。
 *
 * ## 为什么这份稿子需要 verify
 *
 * 它要拍的是「二选一开关」，最容易被做错的三件事都不是视觉问题：
 *   ① 切换后**没告诉用户代价**（清 token / 关轮换）—— 文案断言能钉住；
 *   ② 冲突态**没把直连选项禁用** —— 只看截图看不出来（禁用是属性不是样式）；
 *   ③ 五种状态里**有一种点不出来**（tab 没绑） —— 只有真点才暴露。
 */
const path = require("path");
const fs = require("fs");
const { JSDOM } = require("jsdom");

const FILE = path.join(__dirname, "codebuddy-cli-gateway-proto.html");
const MUTATE = process.argv.includes("--mutate");

const failures = [];
function check(name, fn) {
  try {
    fn();
    console.log(`  ok   ${name}`);
  } catch (error) {
    failures.push(`${name} → ${error.message}`);
    console.log(`  FAIL ${name} → ${error.message}`);
  }
}

let html = fs.readFileSync(FILE, "utf8");
if (MUTATE) {
  // 变异：删掉 tab 的 click 绑定（模拟「状态切不过去」）。
  html = html.replace(
    /t\.addEventListener\('click', function \(\) \{ show\(t\.dataset\.state\); \}\);/,
    "/* mutated: click 绑定被删 */",
  );
  if (!html.includes("/* mutated")) throw new Error("变异注入失败：没找到 click 绑定");
}

const dom = new JSDOM(html, { runScripts: "dangerously", pretendToBeVisual: true });
const { document } = dom.window;

/** 可见（未被 .hide 隐藏）的面板。 */
function visiblePanels() {
  return [...document.querySelectorAll("[data-panel]")].filter(
    (panel) => !panel.classList.contains("hide"),
  );
}

check("五种状态面板都在（未接入 / 直连 / 网关 / 冲突 / 切换中）", () => {
  const states = [...document.querySelectorAll("[data-panel]")].map((p) => p.dataset.panel);
  const expected = ["none", "direct", "gateway", "conflict", "busy"];
  for (const state of expected) {
    if (!states.includes(state)) throw new Error(`缺状态 ${state}（实际：${states.join(",")}）`);
  }
});

check("初始只显示「未接入」一个面板", () => {
  const visible = visiblePanels();
  if (visible.length !== 1) throw new Error(`初始应只有 1 个可见，实际 ${visible.length}`);
  if (visible[0].dataset.panel !== "none") {
    throw new Error(`初始应显示 none，实际 ${visible[0].dataset.panel}`);
  }
});

check("点每个 tab 都能切到对应面板（五种状态都可点出来）", () => {
  const tabs = [...document.querySelectorAll("#state-tabs button")];
  if (tabs.length !== 5) throw new Error(`tab 应有 5 个，实际 ${tabs.length}`);
  for (const tab of tabs) {
    tab.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    const visible = visiblePanels();
    if (visible.length !== 1) {
      throw new Error(`点 ${tab.dataset.state} 后可见面板应为 1 个，实际 ${visible.length}`);
    }
    if (visible[0].dataset.panel !== tab.dataset.state) {
      throw new Error(`点 ${tab.dataset.state} 后显示的是 ${visible[0].dataset.panel}`);
    }
  }
});

check("确认框逐条列出代价：清掉上游 token / 关闭自动轮换 / 需重启", () => {
  const text = document.body.textContent;
  for (const must of ["清掉", "关闭自动轮换", "重启终端"]) {
    if (!text.includes(must)) throw new Error(`确认框缺关键代价说明「${must}」`);
  }
});

check("冲突态：直连上游选项被**禁用**（属性，不是样式）", () => {
  const panel = document.querySelector('[data-panel="conflict"]');
  const disabled = [...panel.querySelectorAll(".opt")].filter(
    (opt) => opt.dataset.disabled === "true",
  );
  if (disabled.length !== 1) {
    throw new Error(`冲突态应有且仅有 1 个禁用选项，实际 ${disabled.length}`);
  }
  const button = disabled[0].querySelector("button");
  if (!button || !button.disabled) throw new Error("被禁用的选项里按钮也必须 disabled");
});

check("冲突态把「护栏」说成人话（不是报错口吻）", () => {
  const panel = document.querySelector('[data-panel="conflict"]');
  const text = panel.textContent;
  if (!text.includes("这不是错误，是护栏")) {
    throw new Error("冲突态必须说明「这不是错误，是护栏」——否则用户会当成 bug 报上来");
  }
  if (!text.includes("时好时坏")) {
    throw new Error("必须点明混用的症状是「时好时坏」，那是它最难查的地方");
  }
});

check("网关态展示四项已生效信息（地址 / Key 掩码 / 模型 / 轮换）", () => {
  const panel = document.querySelector('[data-panel="gateway"]');
  const text = panel.textContent;
  for (const must of ["57891", "sk-wb-", "留空", "已关闭"]) {
    if (!text.includes(must)) throw new Error(`网关态缺「${must}」`);
  }
});

check("Key 只显示掩码（原型里也不能出现完整明文）", () => {
  const panel = document.querySelector('[data-panel="gateway"]');
  const text = panel.textContent;
  if (!text.includes("••••")) throw new Error("Key 必须是掩码形态");
  // 真实 Key 形如 sk-wb-xxxx<32 hex>；出现长串十六进制就说明画错了。
  if (/sk-wb-[a-z0-9]{20,}/i.test(text)) throw new Error("出现了疑似完整 Key");
});

check("方案对比里给出「不推荐」的理由（不能只标红不解释）", () => {
  const plans = [...document.querySelectorAll(".plan")];
  if (plans.length !== 3) throw new Error(`应有 3 个候选方案，实际 ${plans.length}`);
  const rejected = plans.find((p) => p.querySelector(".tag.no"));
  if (!rejected) throw new Error("没有标「不推荐」的方案");
  const bad = rejected.querySelectorAll(".bad");
  if (bad.length === 0) throw new Error("「不推荐」的方案必须写出理由（.bad 条目）");
});

check("明确标出「回直连」这条路目前是断的（拍板后要补的缺口）", () => {
  const text = document.body.textContent;
  if (!text.includes("codebuddy_cli_disable_gateway")) {
    throw new Error("必须点名要补的命令，否则拍板后容易漏");
  }
  if (!text.includes("断的")) throw new Error("必须说明「回直连」目前走不通");
});

console.log("");
if (failures.length) {
  console.log(`✗ ${failures.length} 项失败${MUTATE ? "（变异注入下应失败，符合预期）" : ""}：`);
  for (const failure of failures) console.log(`   - ${failure}`);
  process.exit(1);
}
console.log(`✓ 全部通过${MUTATE ? "（变异下仍全绿 ⇒ 断言在空转！）" : ""}`);
if (MUTATE) process.exit(1);
