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
  });
  const ms = Date.now() - started;
  const ok = run.status === 0;
  // 汇总行形如 `22 passed / 0 failed`、`18 passed, 0 failed`；没有则只给退出码。
  const summary = (run.stdout || "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => /passed|通过/.test(line))
    .pop();
  console.log(
    `${ok ? "  ok  " : "  FAIL"} ${name.padEnd(30)} ${summary ?? `exit=${run.status}`}  (${ms}ms)`,
  );
  if (!ok) {
    failed.push({ name, run });
  }
}

if (failed.length) {
  console.error(`\n[check-proto] ${failed.length}/${files.length} 个套件失败：\n`);
  for (const { name, run } of failed) {
    console.error(`──────── ${name} ────────`);
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
