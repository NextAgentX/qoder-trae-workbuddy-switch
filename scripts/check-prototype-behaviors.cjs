#!/usr/bin/env node
/**
 * 跑一遍**原型行为校验套件**（`docs/prototype/verify-*.cjs`）。
 *
 * ## 为什么需要这个入口
 *
 * 这套断言是原型的「112 条行为契约」，但长期**跑不起来**：它们依赖 `jsdom`，
 * 而 `jsdom` 既没进 `devDependencies`、CI 也没跑过一次。后果是
 * `demo-app.js` 里一个**同名函数重复声明**（SyntaxError ⇒ 整份脚本不执行、
 * 原型在浏览器里只剩静态骨架）活到了 main —— 因为门禁只对原型做**文本匹配**。
 * 于是「有校验」和「校验真的在跑」是两件事，这里把它合成一件：一条命令、一个退出码。
 *
 * ## 边界（刻意不纳入）
 *
 * `verify-limit-link.mjs` / `verify-model-probe.mjs` 走 **CDP + 真浏览器**，
 * 需要先起 dev server，CI 里没有那套装备 ⇒ 不进本脚本，避免造一条**永远红**的门禁
 * （永远红 = 所有人都学会忽略它 = 等于没有）。
 *
 * ## 用法
 *
 * ```bash
 * npm run check:proto            # 全跑
 * npm run check:proto -- pool    # 只跑名字含 "pool" 的（本地迭代用）
 * ```
 */
const { readdirSync } = require("node:fs");
const { spawnSync } = require("node:child_process");
const path = require("node:path");

/* ★ 前置：Node 版本。`jsdom 30` 依赖 `undici 8`，后者 `engines` 要求 **Node ≥ 22.19** ——
   版本不够时 12 个套件会同时崩在 `undici/lib/interceptor/decompress.js`（`reading 'bind'`），
   看起来像「断言全错」，实际是运行时太老。这里提前用一句话说清，别再让人去翻栈。 */
const NODE_MIN = [22, 19, 0];
const current = process.versions.node.split(".").map((part) => Number(part));
const tooOld =
  current[0] < NODE_MIN[0] ||
  (current[0] === NODE_MIN[0] &&
    (current[1] < NODE_MIN[1] ||
      (current[1] === NODE_MIN[1] && current[2] < NODE_MIN[2])));
if (tooOld) {
  console.error(
    `[check-proto] Node ${process.versions.node} 太旧：jsdom 30 → undici 8 要求 Node ≥ ${NODE_MIN.join(".")}。\n` +
      `              升级本机 Node（或 CI 的 node-version）后重跑；这是运行时问题，不是断言失败。`,
  );
  process.exit(1);
}

/* ★ 每个套件的**硬上限**。`spawnSync` 默认无限等待：任何套件挂住
   （死循环 / 未清理的 `setInterval` 让进程不退出 / 等网络）都会把整条
   `npm run build` 拖成「无输出卡死」—— 2026-11-09 实测卡了 17 分钟，
   而单独跑该脚本 1 分钟就全绿（**偶发**，最难受的那种）。
   上限取实测最慢套件（`verify-behaviors` ≈ 25s）的 7 倍，CI 慢机器也够；
   要临时放宽用 `CHECK_PROTO_TIMEOUT_MS=...`。 */
const SUITE_TIMEOUT_MS = Number(process.env.CHECK_PROTO_TIMEOUT_MS || 180_000);
/* 超时值本身也要兜：写错成字符串/0 会让 `spawnSync` 报 TypeError（看着像脚本坏了）。 */
if (!Number.isFinite(SUITE_TIMEOUT_MS) || SUITE_TIMEOUT_MS <= 0) {
  console.error(`[check-proto] CHECK_PROTO_TIMEOUT_MS 不是正数：${process.env.CHECK_PROTO_TIMEOUT_MS}`);
  process.exit(1);
}

const DIR = path.join(__dirname, "..", "docs", "prototype");
const filter = process.argv.slice(2).filter((arg) => !arg.startsWith("-"))[0] || "";

const files = readdirSync(DIR)
  .filter((name) => /^verify-.*\.cjs$/.test(name))
  .filter((name) => (filter ? name.includes(filter) : true))
  .sort();

if (files.length === 0) {
  console.error(`[check-proto] 没找到校验脚本（filter="${filter}"）`);
  process.exit(1);
}

const failed = [];
for (const name of files) {
  const started = Date.now();
  const run = spawnSync(process.execPath, [path.join(DIR, name)], {
    encoding: "utf8",
    // 断言脚本本身会打印逐条结果；只在失败时把它们的输出转出来，
    // 否则 12 个套件 × 几十条会把日志淹没（有用信息反而看不见）。
    stdio: ["ignore", "pipe", "pipe"],
    timeout: SUITE_TIMEOUT_MS,
    // 超时后必须**真的杀掉**：只发 SIGTERM 时，卡在同步循环里的 JS 没有机会处理它。
    killSignal: "SIGKILL",
  });
  const ms = Date.now() - started;
  /* 超时后 `status` 为 `null`、`signal` 为 `SIGKILL`，只看退出码会把超时当成普通失败
     （甚至 `null === 0` 为 false 就只报 `exit=null`，看不出发生了什么）。 */
  const timedOut = Boolean(run.error && run.error.code === "ETIMEDOUT");
  const ok = !timedOut && run.status === 0;
  // 汇总行形如 `22 passed / 0 failed`、`18 passed, 0 failed`；没有则只给退出码。
  const summary = timedOut
    ? `timeout > ${SUITE_TIMEOUT_MS / 1000}s`
    : (run.stdout || "")
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => /passed|通过/.test(line))
        .pop();
  console.log(
    `${ok ? "  ok  " : "  FAIL"} ${name.padEnd(30)} ${summary ?? `exit=${run.status}`}  (${ms}ms)`,
  );
  if (!ok) {
    failed.push({ name, run, timedOut });
  }
}

if (failed.length) {
  console.error(`\n[check-proto] ${failed.length}/${files.length} 个套件失败：\n`);
  for (const { name, run, timedOut } of failed) {
    console.error(`──────── ${name} ────────`);
    if (timedOut) {
      /* 超时**没有断言输出**可转，所以必须给一句人话 + 排查方向，否则下一个人
         只能看到一堆空白。 */
      console.error(
        `该套件超过 ${SUITE_TIMEOUT_MS / 1000}s 未退出（已被 SIGKILL）。\n` +
          `  · 常见原因：setInterval / setTimeout 未清理 ⇒ 事件循环不空、进程不退出；\n` +
          `  · 也可能是死循环，或等了某个不会回来的 IO。\n` +
          `  · 单独跑它看看：node docs/prototype/${name}\n` +
          `  · 确认是慢机器而非挂住，可临时放宽：CHECK_PROTO_TIMEOUT_MS=600000 npm run check:proto`,
      );
      continue;
    }
    const text = `${run.stdout || ""}${run.stderr || ""}`;
    // 只印失败行与其后几行上下文：断言脚本的失败信息本身就带预期/实际。
    const lines = text.split("\n");
    const keep = new Set();
    lines.forEach((line, index) => {
      if (/FAIL|Error|失败/.test(line)) {
        for (let k = index; k < Math.min(index + 6, lines.length); k += 1) keep.add(k);
      }
    });
    if (keep.size === 0) {
      console.error(text.slice(-2000));
    } else {
      [...keep]
        .sort((a, b) => a - b)
        .forEach((index) => console.error(lines[index]));
    }
    console.error("");
  }
  process.exit(1);
}

console.log(`\n[check-proto] ${files.length} 个套件全部通过`);
