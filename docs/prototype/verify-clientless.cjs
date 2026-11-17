/* 原型验收：无客户端形态（issue #62）
 *
 * 验的是**行为**，不是文案自述：直接打开原型读真实 DOM，断言
 *   ① 无客户端 + 0 账号 ⇒ 引导卡（三条可用路径 + 能力矩阵），不再是「还没有账号」；
 *   ② 无客户端 + 有账号 ⇒ 细条（说清降级项），引导卡消失；
 *   ③ 添加账号抽屉里「从本机导入」**不静默失败**，点了给出替代路径；
 *   ④ 授权弹窗：链接可复制、倒计时在走、超时态禁止「已完成」、Trae 有端口转发块；
 *   ⑤ 网关页在无客户端时明确「网关可用」，不是空白警告。
 *
 * 依赖 jsdom（装在隔离工作区，不要往项目里装）：
 *   NODE_PATH="<managed node workspace>/node_modules" node docs/prototype/verify-clientless.cjs
 *
 * 变异模式（证明断言不是恒真的空转）：
 *   node docs/prototype/verify-clientless.cjs --mutate=banner|trae|gate
 *   变异只写 os.tmpdir() 里的副本，交付目录下的原文件永不被写。
 *   变异模式下**全绿即为失败**（说明断言打不红）。
 */
const path = require("path");
const fs = require("fs");
const os = require("os");
const { JSDOM } = require("jsdom");

const BASE = __dirname;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];

/* ─────────── 阶段打点（诊断用，零成本） ───────────
 *
 * 为什么需要它：2026-11-17 这个套件在 CI 上**偶发**挂死（linux 180s /
 * mac 600s 被 SIGKILL），而本机复现不了（Node 22/24 都是 2 秒通过）。
 * 父进程 `check-prototype-behaviors.cjs` 超时时会把子进程**已产生的输出**
 * 原样转出来 —— 所以只要这里打点，下次 CI 一复现，日志里就能直接看到
 * 「卡在第几步」，不必再靠推理。
 *
 * 打点写 **stderr**：stdout 会被父进程在成功时丢弃（只在失败时转出），
 * 而 stderr 永远会被转出 —— 挂死时它就是唯一的信息来源。
 * 用 `process.stderr.write` 而不是 console.error：后者在管道里同样有缓冲，
 * 而 write 是同步落盘的（对 TTY/管道都是同步）。
 */
const T0 = Date.now();
const trace = (msg) => {
  try {
    process.stderr.write(`[clientless +${String(Date.now() - T0).padStart(6)}ms] ${msg}\n`);
  } catch {
    /* 打点本身失败绝不能影响验收本身。 */
  }
};
trace("script start");

/* ─────────── 变异注入 ─────────── */
const MUT = (process.argv.find((a) => a.startsWith("--mutate")) || "").replace("--mutate", "").replace(/^=/, "");
const MUTATIONS = {
  banner: {
    desc: "把引导卡退回旧的「还没有账号」空态",
    file: "demo-app.js",
    from: "if (isClientless(s)) return clientlessEmptyHtml();",
    to: "/* MUTATED: 无客户端也走旧空态 */",
  },
  trae: {
    desc: "去掉 Trae 的端口转发块（假装轮询就够）",
    file: "demo-app.js",
    from: "${trae ? tunnelHtml() : `",
    to: "${false ? tunnelHtml() : `",
  },
  gate: {
    desc: "网关页不再声明「网关可用」",
    file: "demo-app.js",
    from: '${isClientless(curScope()) ? srvStripHtml("gateway") : ""}',
    to: '${"" /* MUTATED: 网关页不提示 */}',
  },
};

function prepareCopy() {
  if (!MUT) return BASE;
  const mut = MUTATIONS[MUT];
  if (!mut) throw new Error(`未知变异：${MUT}（可选 ${Object.keys(MUTATIONS).join("/")}）`);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bs-proto-"));
  for (const f of ["index.html", "demo-app.js", "demo-data.js", "demo.css"]) {
    fs.copyFileSync(path.join(BASE, f), path.join(dir, f));
  }
  const target = path.join(dir, mut.file);
  const code = fs.readFileSync(target, "utf8");
  if (!code.includes(mut.from)) throw new Error(`变异锚点没命中：${mut.from}`);
  fs.writeFileSync(target, code.replace(mut.from, mut.to), "utf8");
  console.log(`\n⚠️  变异模式：${mut.desc}\n`);
  return dir;
}

function check(name, fn) {
  trace(`check start: ${name}`);
  try {
    const detail = fn();
    results.push({ ok: true, name, detail });
    console.log(`  ok   ${name}${detail ? "  → " + detail : ""}`);
  } catch (e) {
    results.push({ ok: false, name, error: e.message });
    console.log(` FAIL  ${name}  → ${e.message}`);
  }
  trace(`check end:   ${name}`);
}

/** 单文件版（`buddy-switch-prototype.html`）—— 与 4 文件源是**双向分叉**的冻结副本，
 *  改一处必须两处同改，所以这里也验一遍，防「只改了一边」。 */
async function bootSingle(query) {
  trace(`bootSingle start query=${query}`);
  const html = fs.readFileSync(path.join(BASE, "buddy-switch-prototype.html"), "utf8")
    .replace('<link rel="stylesheet" href="demo.css" />', "");
  const dom = new JSDOM(html, {
    runScripts: "dangerously",
    url: `https://proto.local/buddy-switch-prototype.html?${query}`,
    pretendToBeVisual: true,
  });
  trace("bootSingle JSDOM constructed, sleeping");
  await sleep(80);
  trace("bootSingle ready");
  return dom;
}

async function boot(dir, query) {
  const html = fs.readFileSync(path.join(dir, "index.html"), "utf8")
    .replace(/<script src="demo-data.js"><\/script>/, `<script>${fs.readFileSync(path.join(dir, "demo-data.js"), "utf8")}</script>`)
    .replace(/<script src="demo-app.js"><\/script>/, `<script>${fs.readFileSync(path.join(dir, "demo-app.js"), "utf8")}</script>`)
    .replace('<link rel="stylesheet" href="demo.css" />', "");
  const dom = new JSDOM(html, {
    runScripts: "dangerously",
    url: `https://proto.local/index.html?${query}`,
    pretendToBeVisual: true,
  });
  trace(`boot JSDOM constructed query=${query}, sleeping`);
  await sleep(60);
  trace("boot ready");
  return dom;
}

const bAct = (doc, sel, act) => {
  const el = doc.querySelector(`${sel}[data-act="${act}"]`);
  if (!el) throw new Error(`找不到 ${sel}[data-act="${act}"]`);
  el.click();
  return el;
};

(async () => {
  const dir = prepareCopy();
  console.log("\n无客户端形态（issue #62）· 原型验收\n");

  /* ① 无客户端 + 0 账号 ⇒ 引导卡 */
  trace("phase 1: clientless + 0 accounts")
  {
    const dom = await boot(dir, "scope=trae-global&dest=accounts&hint=0");
    const d = dom.window.document;
    check("无客户端 + 0 账号 ⇒ 出引导卡，不是「还没有账号」", () => {
      const card = d.querySelector(".srv-card");
      if (!card) throw new Error("没有 .srv-card");
      const txt = card.textContent;
      if (/这个作用域还没有账号/.test(d.querySelector("#acct-list").textContent)) throw new Error("仍然渲染了旧空态");
      for (const k of ["浏览器授权登录", "粘贴授权凭据", "导入备份文件"]) {
        if (!txt.includes(k)) throw new Error(`引导卡缺少路径「${k}」`);
      }
      return "三条路径齐全";
    });
    check("引导卡带能力矩阵（可用 / 需要客户端 两列）", () => {
      const cols = d.querySelectorAll(".cap-matrix .cap-col");
      if (cols.length !== 2) throw new Error(`能力矩阵应有 2 列，实际 ${cols.length}`);
      const ok = cols[0].textContent, off = cols[1].textContent;
      if (!/网关/.test(ok)) throw new Error("「可用」列没提网关 —— 用户会以为网关也没了");
      if (!/程序位切换/.test(off)) throw new Error("「需要客户端」列没提程序位切换");
      return `${cols[0].querySelectorAll("li").length} 可用 / ${cols[1].querySelectorAll("li").length} 降级`;
    });
    check("无客户端时细条**不出现**（0 个账号由引导卡接管）", () => {
      if (d.querySelector(".srv-strip")) throw new Error("同时出现了细条与引导卡");
      return "只有引导卡";
    });

    /* ③ 添加账号抽屉：从本机导入不静默失败 */
    check("点「浏览器授权登录」直接开授权弹窗（不再是 toast 占位）", () => {
      const way = d.querySelector('.srv-way[data-act="add-way|oauth"]');
      if (!way) throw new Error("引导卡里没有「浏览器授权登录」入口");
      way.click();
      const auth = d.querySelector("#auth-overlay");
      if (!auth || !auth.classList.contains("open")) throw new Error("授权弹窗没打开");
      if (!/登录 TraeWork 国际版/.test(auth.textContent)) throw new Error("弹窗标题不对：" + auth.textContent.slice(0, 60));
      return "弹窗已开";
    });
    dom.window.close();
  }

  /* ④ 授权弹窗细节 */
  trace("phase 2: auth dialog details")
  {
    const dom = await boot(dir, "scope=trae-global&dest=accounts&open=auth&hint=0");
    const d = dom.window.document;
    const auth = d.querySelector("#auth-overlay .auth");
    check("授权弹窗有链接 + 二维码 + 三条步骤", () => {
      if (!auth.querySelector(".link-box code")) throw new Error("没有可复制的链接");
      if (!auth.querySelector(".qr-frame svg")) throw new Error("没有二维码位");
      if (auth.querySelectorAll(".auth-steps li").length !== 3) throw new Error("步骤不是 3 条");
      return "链接 / 二维码 / 3 步";
    });
    check("Trae 弹窗把两处硬依赖都说清（设备私钥 + 127.0.0.1:17388）", () => {
      const t = auth.querySelector(".tunnel");
      if (!t) throw new Error("没有 .tunnel —— Trae 远程部署会卡在「认证中」");
      /* ★ 只写端口转发是不够的：换 token 要客户端那把 EC P-256 私钥，
         少了这句用户会以为「扫码就能用」，走到最后一步才失败在 20403。 */
      if (!/设备私钥/.test(t.textContent)) throw new Error("没提设备私钥（会画成「扫码即可」的饼）");
      if (!/20403/.test(t.textContent)) throw new Error("没给失败码，用户没法自查");
      if (!/ssh -L 17388:127\.0\.0\.1:17388/.test(t.textContent)) throw new Error("没给可复制的转发命令");
      return "私钥 + 端口 + 粘贴回调";
    });
    check("倒计时在走（等待态有剩余时间）", () => {
      const left = auth.querySelector("#auth-left");
      if (!left || !/^\d+:\d\d$/.test(left.textContent)) throw new Error("没有倒计时：" + (left && left.textContent));
      return left.textContent;
    });
    dom.window.close();
  }

  /* ④b 超时态 */
  {
    const dom = await boot(dir, "scope=trae-global&dest=accounts&open=auth&auth=expired&hint=0");
    const d = dom.window.document;
    const auth = d.querySelector("#auth-overlay .auth");
    check("超时态：明确说 state 作废，且「已完成」按钮被禁用", () => {
      if (!/已超时/.test(auth.textContent)) throw new Error("没有超时标识");
      const done = auth.querySelector('[data-act="auth-done"]');
      if (!done) throw new Error("没有「已完成」按钮");
      if (!done.disabled) throw new Error("超时后仍可点「已完成」—— 会把作废的 state 当成功");
      return "禁用 + 文案";
    });
    dom.window.close();
  }

  /* ⑤ WB：轮询型，不需要端口转发；且能走完「完成 → 入库 → 细条」 */
  {
    const dom = await boot(dir, "scope=wb-cn&dest=accounts&open=auth&hint=0");
    const d = dom.window.document;
    const auth = d.querySelector("#auth-overlay .auth");
    check("WorkBuddy 弹窗不出现端口转发（轮询型，服务端即可收结果）", () => {
      if (auth.querySelector(".tunnel")) throw new Error("WB 也给了 ssh 隧道 —— 多余且误导");
      if (!/不需要本机回调/.test(auth.textContent)) throw new Error("没说明「不需要本机回调」");
      return "纯轮询";
    });
    dom.window.close();
  }

  /* ② 无客户端 + 有账号 ⇒ 细条 */
  {
    const dom = await boot(dir, "scope=trae-global&dest=accounts&open=auth&hint=0");
    const d = dom.window.document;
    d.querySelector('[data-act="auth-done"]').click();
    await sleep(20);
    d.querySelector('[data-act="auth-complete"]').click();
    await sleep(30);
    check("授权完成后：账号入库 + 弹窗关闭", () => {
      if (d.querySelector("#auth-overlay.open")) throw new Error("弹窗没关");
      const n = dom.window.eval('SCOPES["trae-global"].accounts.length');
      if (n !== 1) throw new Error(`账号库应有 1 个，实际 ${n}`);
      return "1 个账号";
    });
    check("有账号后换成细条，且说清「网关不受影响」", () => {
      const strip = d.querySelector("#srv-slot .srv-strip");
      if (!strip) throw new Error("没有细条");
      if (d.querySelector(".srv-card")) throw new Error("引导卡还在（账号已存在）");
      if (!/账号与网关不受影响/.test(strip.textContent)) throw new Error("细条没点明影响范围");
      return strip.textContent.replace(/\s+/g, " ").slice(0, 40) + "…";
    });
    dom.window.close();
  }

  /* ⑤ 网关页 */
  {
    const dom = await boot(dir, "scope=trae-global&dest=gateway&hint=0");
    const d = dom.window.document;
    check("网关页在无客户端时明确「网关照常可用」", () => {
      const strip = d.querySelector(".srv-strip");
      if (!strip) throw new Error("网关页没有任何说明");
      if (!/网关照常可用/.test(strip.textContent)) throw new Error("说明里没点明网关可用");
      if (!/只依赖账号池里的凭据/.test(strip.textContent)) throw new Error("没说清为什么可用");
      return "已声明";
    });
    check("网关页仍可切开关（不是被降级成只读）", () => {
      const sw = d.querySelector('[data-act="toggle|gw-enabled"]');
      if (!sw) throw new Error("没有启用开关");
      return "开关在";
    });
    dom.window.close();
  }

  /* ③ 抽屉里的「从本机导入」 */
  {
    const dom = await boot(dir, "scope=trae-global&dest=accounts&hint=0");
    const d = dom.window.document;
    d.querySelector('[data-act="add-account"]').click();
    await sleep(20);
    check("添加账号抽屉：四条路并存，从本机导入标注不可用但不置灰", () => {
      const opts = d.querySelectorAll(".opt");
      if (opts.length !== 4) throw new Error(`应有 4 条路径，实际 ${opts.length}`);
      const local = [...opts].find((o) => o.dataset.act === "add-way|local");
      if (!local) throw new Error("没有「从本机导入」");
      if (local.disabled) throw new Error("被禁用 —— 禁用既不给原因也不给替代");
      if (!/本机没有客户端/.test(local.textContent)) throw new Error("没标注原因");
      return "4 条 + 可点";
    });
    check("点「从本机导入」给出替代路径（不是静默失败）", () => {
      const local = d.querySelector('.opt[data-act="add-way|local"]');
      local.click();
      const toast = d.querySelector(".toasts .toast");
      if (!toast) throw new Error("没有任何反馈");
      if (!/浏览器授权登录|粘贴授权凭据/.test(toast.textContent)) throw new Error("反馈里没给替代：" + toast.textContent);
      return "给了两条替代";
    });
    dom.window.close();
  }

  /* 单文件版同形（两份原型不许漂移） */
  {
    const dom = await bootSingle("scope=trae-global&dest=accounts&hint=0");
    const d = dom.window.document;
    check("单文件原型同形：无客户端引导卡 + 三条路径", () => {
      const card = d.querySelector(".srv-card");
      if (!card) throw new Error("单文件版没有 .srv-card（只改了 demo-app.js）");
      const txt = card.textContent;
      for (const k of ["浏览器授权登录", "粘贴授权凭据", "导入备份文件"]) {
        if (!txt.includes(k)) throw new Error(`单文件版缺「${k}」`);
      }
      return "同形";
    });
    check("单文件原型同形：Trae 授权弹窗带端口转发块", () => {
      d.querySelector('.srv-way[data-act="add-way|oauth"]').click();
      const t = d.querySelector("#auth-overlay .tunnel");
      if (!t) throw new Error("单文件版没有端口转发块");
      if (!/ssh -L 17388:127\.0\.0\.1:17388/.test(t.textContent)) throw new Error("单文件版没给转发命令");
      return "同形";
    });
    dom.window.close();
  }

  /* 回归：有客户端的形态不受影响 */
  {
    const dom = await boot(dir, "scope=wb-cn&dest=accounts&hint=0");
    const d = dom.window.document;
    check("回归：装了客户端的账号页不出现任何无客户端 UI", () => {
      if (d.querySelector(".srv-strip") || d.querySelector(".srv-card")) throw new Error("wb-cn 被误判成无客户端");
      if (d.querySelectorAll(".acct").length !== 8) throw new Error("账号卡数量变了");
      return "8 张卡 · 无新增块";
    });
    dom.window.close();
  }

  const failed = results.filter((r) => !r.ok);
  console.log("");
  if (MUT) {
    if (!failed.length) {
      console.log("❌ 变异模式下全绿 —— 断言打不红，等于没验。\n");
      process.exit(1);
    }
    console.log(`✅ 变异被捕获（${failed.length} 条断言变红），断言有效。\n`);
    process.exit(0);
  }
  console.log(failed.length ? `❌ ${failed.length} 条失败 / 共 ${results.length} 条\n` : `✅ 全部通过（${results.length} 条）\n`);
  process.exit(failed.length ? 1 : 0);
})().catch((e) => {
  console.error("验收脚本自身崩了：", e);
  process.exit(2);
});
