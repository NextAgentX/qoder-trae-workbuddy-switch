#!/usr/bin/env node
/**
 * `gatewayCacheHit` / `dimTotals` 的**真实断言**（issue #154）。
 *
 * ## 为什么这条门禁值得存在
 *
 * 命中率是**一个除法**，而它已经被同一个错误伤过一次：协议层把命中量算了**两遍**
 * （`input_tokens` 抄上游 `prompt_tokens` 又单列 `cache_read_input_tokens`），
 * 现场表现为「缓存命中率恒为 50%，而网关日志是 99%」（issue #154）。
 * 那一处修在 Rust 的协议出口；本文件守的是**账本层的另一半**：
 *
 * 1. **分母只能是输入侧**（`input`），不是 `total` —— 拿 input+output 当分母，
 *    命中率会随输出变长系统性下跌，而输出长度与缓存毫无关系；
 * 2. **`cached` 不参与任何求和**（它是 `input` 的子集）—— 一旦被加进 `total`，
 *    同一个错误就从协议层搬进账本层，页面自己翻倍，客户端都不需要参与；
 * 3. **`null`（没观测到）≠ `0`（确实没命中）** —— 两者混同会让用户去查一个
 *    并不存在的缓存问题，而「上游压根没报这个字段」才是多数情况。
 *
 * ## 为什么是脚本而不是 vitest
 *
 * 项目没有前端测试框架（质量靠 `check:*` 门禁）。这里用已有的 esbuild 把
 * `src/lib/usage-ledger.ts` 现场转译成 ESM、经 `data:` URL 导入，跑真断言 ——
 * 测的是**生产代码本身**，不是复制一份逻辑（复制的那份必然与生产漂移）。
 *
 * 用法：`node scripts/check-cache-hit.mjs`（已挂进 `npm run check:scope`）。
 */

import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// ★ Windows：绝对路径必须转 `file://` URL，否则 ESM loader 把 `D:` 当成协议。
const { build } = await import(pathToFileURL(path.join(root, "node_modules/esbuild/lib/main.js")).href);
/* 必须 **bundle** 而不是单纯 transform：`usage-ledger.ts` 里有 `@/lib/usage-window`
这种路径别名，`data:` URL 里的裸模块说明符无法解析（`ERR_UNSUPPORTED_RESOLVE_REQUEST`）。
bundle + alias 把它内联进来，跑的是同一份生产代码。 */
const built = await build({
  entryPoints: [path.join(root, "src/lib/usage-ledger.ts")],
  bundle: true,
  format: "esm",
  write: false,
  platform: "neutral",
  alias: { "@": path.join(root, "src") },
});
const code = built.outputFiles[0].text;
const mod = await import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
const { gatewayCacheHit, dimTotals, modelKey } = mod;

let failed = 0;
let passed = 0;
function check(name, fn) {
  try {
    fn();
    passed += 1;
  } catch (error) {
    failed += 1;
    console.error(`  ✗ ${name}\n    ${error.message}`);
  }
}
function eq(actual, expected, hint) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${hint ? hint + "：" : ""}期望 ${e}，实际 ${a}`);
}
function near(actual, expected, hint) {
  if (actual == null || Math.abs(actual - expected) > 1e-9) {
    throw new Error(`${hint ? hint + "：" : ""}期望 ${expected}，实际 ${actual}`);
  }
}

/** 造一天的维度项（四个数组给同一组值，只填需要的那个）。 */
const entry = (over) => ({
  input: 0,
  output: 0,
  total: 0,
  requests: 1,
  failed: 0,
  credit: 0,
  ...over,
});
const dayOf = (key, over = {}) => ({
  key,
  truncated: false,
  partial: false,
  models: [],
  accounts: [],
  endpoints: [],
  keys: [],
  keysPartial: false,
  ...over,
});
/** 窗口 = 全部日期（与 `usageWindowKeys` 的 `{kind:"preset"}` 等价）。 */
const allWindow = { kind: "all" };

/* ── ① 分母是输入侧，不是 total ───────────────────────────────────── */
check("命中率的分母是 input（不含 output）", () => {
  const dim = {
    days: [
      dayOf("2026-11-05", {
        models: [entry({ model: "m", input: 1000, output: 9000, total: 10000, cached: 900 })],
      }),
    ],
  };
  const hit = gatewayCacheHit(dim, allWindow);
  near(hit.hitRate, 0.9, "900 / 1000 = 90%（拿 total 当分母会算成 9%）");
  eq(hit.input, 1000, "分母是输入侧");
  eq(hit.cached, 900, "分子是命中量");
});

/* ── ② cached 不得被加进 total（同一个 bug 的另一半） ────────────── */
check("dimTotals 的 tokens 不含 cached（cached 是 input 的子集）", () => {
  const dim = {
    days: [
      dayOf("2026-11-05", {
        models: [entry({ model: "m", input: 1000, output: 20, total: 1020, cached: 900 })],
      }),
    ],
  };
  const [row] = dimTotals(dim, allWindow, modelKey);
  eq(row.tokens, 1020, "tokens 只能是 input + output");
  eq(row.input, 1000, "input 单列（命中率的分母）");
  eq(row.cached, 900, "cached 单列（命中率的分子）");
});

/* ── ③ null（没观测到）必须与 0（确实没命中）分开 ────────────────── */
check("窗口内没有 cached 字段 ⇒ observed=false 且 hitRate=null", () => {
  const dim = {
    days: [
      dayOf("2026-11-05", {
        models: [entry({ model: "m", input: 1000, output: 20, total: 1020, cached: null })],
      }),
    ],
  };
  const hit = gatewayCacheHit(dim, allWindow);
  eq(hit.observed, false, "一条命中数据都没有");
  eq(hit.hitRate, null, "没观测到 ⇒ null（不是 0%）");
  eq(hit.input, 1000, "分母仍然给出来（UI 可以显示「输入 1K，命中未观测」）");
});

check("cached 明确为 0 ⇒ observed=true 且 hitRate=0", () => {
  const dim = {
    days: [
      dayOf("2026-11-05", {
        models: [entry({ model: "m", input: 1000, output: 20, total: 1020, cached: 0 })],
      }),
    ],
  };
  const hit = gatewayCacheHit(dim, allWindow);
  eq(hit.observed, true, "上游报了 0，这是真有观测");
  near(hit.hitRate, 0, "确实一次没命中");
});

/* ── ④ 只选一条维度求和（否则同一个请求被算四遍） ────────────────── */
check("四条维度都有数时，命中率不会翻倍（只取 models）", () => {
  const same = [
    entry({ model: "m", account: "a", endpoint: "/e", key: "wb:k", input: 1000, cached: 500, total: 1000 }),
  ];
  const dim = {
    days: [
      dayOf("2026-11-05", {
        /* 同一个请求在四个维度各出现一次 —— 混着加会得到 4000/2000 之类的错值。 */
        models: same,
        accounts: same,
        endpoints: same,
        keys: same,
      }),
    ],
  };
  const hit = gatewayCacheHit(dim, allWindow);
  eq(hit.input, 1000, "只能计一次");
  eq(hit.cached, 500, "只能计一次");
  near(hit.hitRate, 0.5);
});

/* ── ⑤ input 为 0 ⇒ null（不是 0%） ─────────────────────────────── */
check("窗口内 input 为 0 ⇒ hitRate=null", () => {
  const dim = {
    days: [dayOf("2026-11-05", { models: [entry({ model: "m", input: 0, output: 0, total: 0, cached: 0 })] })],
  };
  const hit = gatewayCacheHit(dim, allWindow);
  eq(hit.hitRate, null, "没有可判定的输入 ⇒ null");
  eq(hit.observed, true, "但确实读到了 cached 字段");
});

/* ── ⑥ 窗口过滤：窗外的高/低命中不得混进分母 ─────────────────────── */
check("窗口外的日期不参与命中率", () => {
  const dim = {
    days: [
      /* 方案外的日子里，命中率是 0；方案内是 100%。若不做窗口过滤，结果会是 50%。 */
      dayOf("2026-01-01", { models: [entry({ model: "m", input: 1000, cached: 0, total: 1000 })] }),
      dayOf("2026-11-05", { models: [entry({ model: "m", input: 1000, cached: 1000, total: 1000 })] }),
    ],
  };
  const hit = gatewayCacheHit(dim, { kind: "preset", days: 1 });
  /* 只断言「不是 50%」这一件事：`usageWindowKeys` 的日期推导由它自己的套件覆盖，
     这里钉的是「本函数确实走了窗口过滤」。 */
  if (hit.hitRate != null && Math.abs(hit.hitRate - 0.5) < 1e-9) {
    throw new Error("把窗外那天也算进来了（窗口过滤失效）");
  }
});

/* ── ⑦ dim 为 null（还没取到）时不得崩 ───────────────────────────── */
check("dim=null ⇒ 空结果，不抛异常", () => {
  const hit = gatewayCacheHit(null, allWindow);
  eq(hit.hitRate, null);
  eq(hit.observed, false);
});

console.log(`\n[check-cache-hit] ${passed} passed / ${failed} failed`);
if (failed > 0) process.exit(1);
