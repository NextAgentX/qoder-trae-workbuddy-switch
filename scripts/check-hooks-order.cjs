#!/usr/bin/env node
/**
 * 门禁：**早退之后不许再调 hook**（React #300 的唯一成因）。
 *
 * ## 为什么需要它
 *
 * issue #131：`TokenStatsPage` 的 `TrendView` 在 `if (filtered) return <ViewEmpty/>` **之后**
 * 还有 `const cumulative = useMemo(...)`。`filtered` 是「有历史数据、但当前窗口没有」，
 * 于是从「有数据」切到「今天无数据」时 hook 数量变化 ⇒ React 抛
 * `#300 Rendered more hooks than during the previous render` ⇒ 整页白屏。
 *
 * 这类 bug 的共同形状是「**hook 调用出现在同函数内的 `return` / `throw` 之后**」，
 * 而 `tsc` 不管、运行时只在**特定数据组合**下炸（本地开发和演示模式都不一定复现）。
 * 所以做成静态门禁：宁可多扫一遍 AST，也别等用户点出来。
 *
 * ## 判据（刻意只认最明确的一种形状）
 *
 * 对每个**函数体是块**的函数：按语句顺序扫，一旦遇到「自身含 `return` / `throw`」的
 * 顶层语句（`return`、`throw`、`if (...) { return ... }`、含它们的 `for`/`switch` 不算——
 * 循环里的 return 不算早退），后续顶层语句里若出现 hook 调用即报错。
 *
 * - 只认函数体的**顶层语句**：`useMemo(() => { ... })` 里的箭头是另一个函数，单独扫。
 * - hook 的判据是「被调者名字以 `use` + 大写字母开头」（`useT` / `useMemo` / `useState`）；
 *   组件名同样是大写开头但**不是** `use` 前缀，不会误判。
 * - 顶层 `const a = cond ? x : y` 之类不构成早退，不管。
 *
 * 用法：`node scripts/check-hooks-order.cjs`（`npm run build` 已挂）。有问题时退出码 1。
 */
const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const ROOT = path.resolve(__dirname, "..");
const SRC = path.join(ROOT, "src");
const EXTS = new Set([".ts", ".tsx"]);

/** 递归收集要扫的文件（跳过 `.d.ts`）。 */
function collect(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
      collect(full, out);
    } else if (EXTS.has(path.extname(entry.name)) && !entry.name.endsWith(".d.ts")) {
      out.push(full);
    }
  }
  return out;
}

/** `useFoo` / `useState`；`user`、`UserCard` 不算。 */
function isHookName(name) {
  return /^use[A-Z0-9]/.test(name);
}

/** 该表达式是不是一次 hook 调用（`useX(...)` / `React.useX(...)` / `ns.useX(...)`）。 */
function hookCalleeName(node) {
  if (!ts.isCallExpression(node)) return null;
  const callee = node.expression;
  if (ts.isIdentifier(callee)) return isHookName(callee.text) ? callee.text : null;
  if (ts.isPropertyAccessExpression(callee) && isHookName(callee.name.text)) return callee.name.text;
  return null;
}

/**
 * 在**不进入嵌套函数**的前提下找第一处 hook 调用。
 * 进入嵌套函数会把 `useMemo(() => { if (x) return; useY() })` 里那种「另一个函数的顺序」
 * 算到外层头上，属于误报来源。
 */
function findHookCall(node) {
  let found = null;
  const walk = (child) => {
    if (found) return;
    if (
      ts.isFunctionDeclaration(child) ||
      ts.isFunctionExpression(child) ||
      ts.isArrowFunction(child) ||
      ts.isMethodDeclaration(child) ||
      ts.isClassDeclaration(child)
    ) {
      return; // 嵌套函数由它自己那一轮负责
    }
    const name = hookCalleeName(child);
    if (name) {
      found = { name, node: child };
      return;
    }
    ts.forEachChild(child, walk);
  };
  walk(node);
  return found;
}

/** 该语句自身（不进嵌套函数）是否含 `return` / `throw` ⇒ 构成早退。 */
function hasOwnExit(node) {
  let exit = false;
  const walk = (child) => {
    if (exit) return;
    if (
      ts.isFunctionDeclaration(child) ||
      ts.isFunctionExpression(child) ||
      ts.isArrowFunction(child) ||
      ts.isMethodDeclaration(child)
    ) {
      return;
    }
    if (ts.isReturnStatement(child) || ts.isThrowStatement(child)) {
      exit = true;
      return;
    }
    ts.forEachChild(child, walk);
  };
  walk(node);
  return exit;
}

const problems = [];

function scanFunction(fn, source, rel) {
  const body = fn.body;
  if (!body || !ts.isBlock(body)) return;
  let earlyExitAt = null;
  for (const statement of body.statements) {
    const hook = findHookCall(statement);
    if (earlyExitAt && hook) {
      const { line } = source.getLineAndCharacterOfPosition(hook.node.getStart(source));
      const exitLine = source.getLineAndCharacterOfPosition(earlyExitAt.getStart(source)).line + 1;
      problems.push(
        `${rel}:${line + 1}  ${hook.name}() 出现在早退之后（第 ${exitLine} 行已 return/throw）` +
          ` ⇒ hook 数量会随条件变化，运行时抛 React #300`,
      );
      continue;
    }
    if (!earlyExitAt && hasOwnExit(statement)) earlyExitAt = statement;
  }
}

function scanFile(file) {
  const text = fs.readFileSync(file, "utf8");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const rel = path.relative(ROOT, file).replace(/\\/g, "/");
  const visit = (node) => {
    if (
      ts.isFunctionDeclaration(node) ||
      ts.isFunctionExpression(node) ||
      ts.isArrowFunction(node) ||
      ts.isMethodDeclaration(node)
    ) {
      scanFunction(node, source, rel);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
}

const files = collect(SRC);
for (const file of files) scanFile(file);

if (problems.length) {
  console.error("hooks 顺序校验失败（早退之后不得调用 hook）：\n");
  for (const line of problems) console.error("  " + line);
  console.error(`\n共 ${problems.length} 处。修法：把 hook 调用上移到早退之前（不是把早退下移，` +
    `因为早退之后可能还有其它 hook）。`);
  process.exit(1);
}

console.log(`hooks 顺序校验通过：${files.length} 个文件，0 处早退后调用 hook`);
