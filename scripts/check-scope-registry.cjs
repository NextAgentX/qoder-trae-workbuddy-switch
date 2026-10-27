#!/usr/bin/env node
/**
 * 门禁：作用域与平台注册表（P0 产出）的自洽性。
 *
 * ## 为什么需要它
 *
 * P0 新增了 `src/lib/scope.ts` + `src/lib/platforms/registry.ts`，它们是**唯一事实来源**：
 * 作用域解析、平台能力、旧路由映射都从这里读。
 *
 * 前端**没有单测框架**（与 `check-store-selectors.cjs` 面对的是同一个约束），
 * 但那支门禁只能做**文本扫描**；本脚本要守的是**运行时行为**（尤其 fail-closed），
 * 所以改用「TypeScript 转译 → 加载 → 断言」的方式，直接跑真代码。
 *
 * ## 判据
 *
 * 1. 注册表能列出 **6 个作用域**（3 平台 × 2 区域，P0 验收项）；
 * 2. `parseScope` **fail-closed**：非法值返回 `null`，**绝不回落默认值**（§2.5）；
 * 3. `scopeOf` / `scopeParts` **往返一致**；
 * 4. `platformOfPath` 能映射旧路由（**兼容旧版本的关键**，D4）；
 * 5. 平台短标识映射**双向一致**；
 * 6. 每个平台声明的 `regions` 都能拼出**合法 scope**（注册表与 scope 不漂移）。
 *
 * 用法：`node scripts/check-scope-registry.cjs`
 */

const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const ROOT = path.resolve(__dirname, "..");

/**
 * 只被 hook 用到的包 —— 给**空桩**，避免为了测一个纯函数拉起整个 React 运行时。
 *
 * ⚠️ 必须做桩而不是让它去 `require` 真包：顶层 `import` 在转译成 CJS 后会**立即执行**，
 * 而 `react-router-dom` 是 ESM-only，在 CJS 里 `require` 会抛。
 */
const STUBS = {
  react: {
    useCallback: (fn) => fn,
    useMemo: (fn) => fn(),
    useState: (init) => [typeof init === "function" ? init() : init, () => {}],
  },
  "react-router-dom": {
    useSearchParams: () => [new URLSearchParams(), () => {}],
  },
};

/**
 * 把 TS 源码转成 CJS 并求值，返回其导出。
 *
 * ⚠️ 需要**自定义 require**：源码里用的是无扩展名导入（`./platforms/registry`），
 * 而 Node 的 ESM 解析器要求扩展名 ⇒ 不能直接 `import`，只能自己解析。
 * 这也顺带绕开了路径别名（本项目源码内部用相对路径，别名只在 tsconfig 里）。
 */
/** 预设窗口构造器：参数条上就是「近 N 天」那一档。 */
const presetWin = (days) => ({ kind: "preset", days });

function loadTs(file, seen = new Map()) {
  const abs = path.resolve(file);
  if (seen.has(abs)) return seen.get(abs);

  const source = fs
    .readFileSync(abs, "utf8")
    // ⚠️ `import.meta.env` 在 CJS 里是**语法错误**（不是运行时 undefined）。
    // 门禁不跑 Vite，所以转译前把它换成注入的普通对象 —— **只改转译输入，不动源码**。
    .replace(/import\.meta\.env/g, "__SCOPE_ENV__");

  const js = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  }).outputText;

  const mod = { exports: {} };
  seen.set(abs, mod.exports); // 先登记再求值：允许环形依赖

  const req = (spec) => {
    // 路径别名 `@/` → `src/`（tsconfig 的 paths）。门禁不走 Vite，得自己解析 ——
    // 本项目里两种导入都存在：`scope.ts` 用相对路径，`use-scope.ts` 用别名。
    const aliased = spec.startsWith("@/") ? path.join(ROOT, "src", spec.slice(2)) : spec;
    const isLocal = spec.startsWith(".") || path.isAbsolute(aliased);

    if (!isLocal) {
      if (Object.prototype.hasOwnProperty.call(STUBS, spec)) return STUBS[spec];
      return require(spec);
    }

    const base = path.isAbsolute(aliased) ? aliased : path.resolve(path.dirname(abs), aliased);
    for (const cand of [`${base}.ts`, `${base}.tsx`, path.join(base, "index.ts")]) {
      if (fs.existsSync(cand)) return loadTs(cand, seen);
    }
    throw new Error(`找不到模块 ${spec}（来自 ${path.relative(ROOT, abs)}）`);
  };

  // 门禁环境：不设 `VITE_SCOPE_UNIFIED` ⇒ 走默认（统一开启）
  new Function("module", "exports", "require", "__SCOPE_ENV__", js)(
    mod,
    mod.exports,
    req,
    {},
  );
  return mod.exports;
}

const scope = loadTs(path.join(ROOT, "src/lib/scope.ts"));
const registry = loadTs(path.join(ROOT, "src/lib/platforms/registry.ts"));
const useScopeMod = loadTs(path.join(ROOT, "src/lib/use-scope.ts"));

let passed = 0;
const failures = [];

function check(name, fn) {
  try {
    const detail = fn();
    passed += 1;
    console.log(`  ok   ${name}${detail ? "  → " + detail : ""}`);
  } catch (e) {
    failures.push(`${name}  → ${e.message}`);
    console.log(` FAIL  ${name}  → ${e.message}`);
  }
}

console.log("\n作用域与平台注册表门禁\n");

check("注册表能列出 6 个作用域（3 平台 × 2 区域，P0 验收项）", () => {
  const got = [...scope.SCOPES];
  if (got.length !== 6) throw new Error(`实际 ${got.length} 个：${got.join(",")}`);
  for (const s of ["wb-cn", "wb-global", "trae-cn", "trae-global", "qoder-cn", "qoder-global"]) {
    if (!got.includes(s)) throw new Error(`缺少 ${s}`);
  }
  return got.join(" / ");
});

check("★ parseScope 是 fail-closed（非法值返回 null，不回落默认值）", () => {
  const legal = scope.parseScope("wb-cn");
  if (legal !== "wb-cn") throw new Error(`合法值被拒：${legal}`);
  for (const bad of ["", null, undefined, "nope", "wb", "wb-xx", "cn", "wb-cn ", "WB-CN"]) {
    const got = scope.parseScope(bad);
    if (got !== null) throw new Error(`非法值 ${JSON.stringify(bad)} 返回了 ${JSON.stringify(got)} —— 违反 fail-closed`);
  }
  return "合法通过 / 9 个非法值全部拒绝";
});

check("scopeOf 与 scopeParts 往返一致", () => {
  for (const s of scope.SCOPES) {
    const p = scope.scopeParts(s);
    const back = scope.scopeOf(p.product, p.region);
    if (back !== s) throw new Error(`${s} → ${p.product}/${p.region} → ${back}`);
  }
  return `${scope.SCOPES.length} 个作用域往返一致`;
});

check("★ platformOfPath 映射路由（旧路由 + 新 `/s/{scope}` 两种形态）", () => {
  const cases = [
    // 旧路由（兼容）
    ["/", "workbuddy"],
    ["/settings", "workbuddy"],
    ["/token-stats", "workbuddy"],
    ["/trae", "trae"],
    ["/trae/accounts", "trae"],
    ["/trae/settings", "trae"],
    // 反例：`/traeish` 不是 Trae 分区（前缀必须带分隔符）
    ["/traeish", "workbuddy"],
    // ★ 新路由（P2）：**不能只看 `/trae` 前缀** —— `/s/trae-cn/...` 不带它
    ["/s/wb-cn/overview", "workbuddy"],
    ["/s/wb-global/accounts", "workbuddy"],
    ["/s/trae-cn/overview", "trae"],
    ["/s/trae-global/settings", "trae"],
    // 新路由但 scope 非法 ⇒ 回落到旧规则（不猜）
    ["/s/nope/overview", "workbuddy"],
  ];
  for (const [p, want] of cases) {
    const got = registry.platformOfPath(p);
    if (got !== want) throw new Error(`${p} → ${got}，期望 ${want}`);
  }
  return `${cases.length} 条路由全部映射正确`;
});

check("平台短标识映射双向一致", () => {
  for (const product of registry.PRODUCTS) {
    const slug = registry.PLATFORM_SLUG[product];
    const back = registry.SLUG_TO_PRODUCT[slug];
    if (back !== product) throw new Error(`${product} → ${slug} → ${back}`);
  }
  return registry.PRODUCTS.map((p) => `${p}=${registry.PLATFORM_SLUG[p]}`).join(" / ");
});

check("★ 每个平台声明的 regions 都能拼出合法 scope（注册表与 scope 不漂移）", () => {
  for (const product of registry.PRODUCTS) {
    const d = registry.platformDescriptor(product);
    if (!d.regions.length) throw new Error(`${product} 没有声明任何 region`);
    for (const r of d.regions) {
      const s = scope.scopeOf(product, r);
      if (scope.parseScope(s) === null) {
        throw new Error(`${product} 声明了 ${r}，但拼出的 ${s} 不是合法作用域`);
      }
    }
  }
  const total = registry.PRODUCTS.reduce(
    (n, p) => n + registry.platformDescriptor(p).regions.length,
    0,
  );
  if (total !== scope.SCOPES.length) {
    throw new Error(`平台声明的作用域 ${total} 个，而 SCOPES 有 ${scope.SCOPES.length} 个 —— 两处漂移了`);
  }
  return `平台合计 ${total} 个 = SCOPES ${scope.SCOPES.length} 个`;
});

check("★ 会话复制能力只有 WorkBuddy 有（§3.15 能力边界）", () => {
  if (!registry.hasCapability("workbuddy", "sessionCopy")) throw new Error("WorkBuddy 缺 sessionCopy");
  if (registry.hasCapability("trae", "sessionCopy")) {
    throw new Error("Trae 被声明了 sessionCopy —— 本仓 Trae 没有会话能力（见 PLAN §3.15）");
  }
  return "workbuddy ✓ / trae ✗";
});

check("DEFAULT_SCOPE 本身是合法作用域", () => {
  if (scope.parseScope(scope.DEFAULT_SCOPE) === null) {
    throw new Error(`默认值 ${scope.DEFAULT_SCOPE} 不合法`);
  }
  return scope.DEFAULT_SCOPE;
});

check("readLastScope 对非法/缺失值返回 null，且不抛异常", () => {
  if (scope.readLastScope(undefined) !== null) throw new Error("undefined 存储应返回 null");
  const fake = { getItem: () => "garbage" };
  if (scope.readLastScope(fake) !== null) throw new Error("非法值应返回 null");
  const ok = { getItem: () => "trae-global" };
  if (scope.readLastScope(ok) !== "trae-global") throw new Error("合法值没读出来");
  const boom = { getItem: () => { throw new Error("storage disabled"); } };
  if (scope.readLastScope(boom) !== null) throw new Error("存储抛异常时应吞掉并返回 null");
  return "缺失 / 非法 / 抛异常 三种情况都安全";
});

check("★★ 跨路由记忆：URL 缺失时用记忆（**P1 修的就是这条**）", () => {
  const R = useScopeMod.resolveRegion;
  const cases = [
    // [说明, urlValue, rememberedScope, product, 期望]
    ["URL 优先于记忆", "global", "wb-cn", "workbuddy", "global"],
    ["★ URL 缺失 ⇒ 用记忆（跨路由的关键）", null, "wb-global", "workbuddy", "global"],
    ["★ 记忆的平台不匹配 ⇒ 回落默认", null, "wb-global", "trae", "cn"],
    ["都没有 ⇒ 默认", null, null, "workbuddy", "cn"],
    ["非法记忆值 ⇒ 默认（不崩）", null, "garbage", "workbuddy", "cn"],
    ["URL 别名 intl ⇒ global", "intl", null, "workbuddy", "global"],
    ["URL 非法但记忆合法 ⇒ 用记忆", "??", "trae-global", "trae", "global"],
    ["记忆是另一平台的 cn ⇒ 不套用", null, "trae-cn", "workbuddy", "cn"],
  ];
  for (const [name, urlValue, rememberedScope, product, want] of cases) {
    const got = R({ urlValue, rememberedScope, product });
    if (got !== want) throw new Error(`${name}：得到 ${got}，期望 ${want}`);
  }
  return `${cases.length} 种情形全部正确`;
});

check("normalizeRegion 宽容解析（别名 / 大小写 / 空白 / 非法）", () => {
  const N = useScopeMod.normalizeRegion;
  const ok = [
    ["cn", "cn"],
    ["CN", "cn"],
    [" cn ", "cn"],
    ["global", "global"],
    ["Global", "global"],
    ["intl", "global"],
    ["international", "global"],
  ];
  for (const [input, want] of ok) {
    const got = N(input);
    if (got !== want) throw new Error(`${JSON.stringify(input)} → ${got}，期望 ${want}`);
  }
  for (const bad of ["", "  ", null, undefined, "xx", "cn-global"]) {
    if (N(bad) !== null) throw new Error(`${JSON.stringify(bad)} 应返回 null`);
  }
  return `${ok.length} 个合法值 + 6 个非法值`;
});

check("★★ P2 路径作用域优先于查询参数（`/s/wb-global/...` 里的区域说了算）", () => {
  const R = useScopeMod.resolveRegion;
  const cases = [
    // [说明, pathScope, urlValue, rememberedScope, product, 期望]
    ["路径优先于查询串", "wb-global", "cn", null, "workbuddy", "global"],
    ["路径优先于记忆", "wb-cn", null, "wb-global", "workbuddy", "cn"],
    ["★ 路径的 platform 不匹配 ⇒ 不采信，往下降级", "trae-cn", "global", null, "workbuddy", "global"],
    ["路径非法 ⇒ 用查询串", "garbage", "global", null, "workbuddy", "global"],
    ["路径与查询都非法 ⇒ 用记忆", "garbage", "??", "wb-global", "workbuddy", "global"],
    ["路径与查询都合法 ⇒ 路径赢", "wb-cn", "global", null, "workbuddy", "cn"],
  ];
  for (const [name, pathScope, urlValue, rememberedScope, product, want] of cases) {
    const got = R({ pathScope, urlValue, rememberedScope, product });
    if (got !== want) throw new Error(`${name}：得到 ${got}，期望 ${want}`);
  }
  return `${cases.length} 种情形全部正确`;
});

check("P1 回滚开关默认开启（VITE_SCOPE_UNIFIED 未设时）", () => {
  if (useScopeMod.isScopeUnified() !== true) {
    throw new Error("默认应为开启（门禁环境未设 VITE_SCOPE_UNIFIED）");
  }
  return "默认开启";
});

/* ── 接线检查（**文本扫描**，与上面几条不同：这里要断言的就是"用了哪个 API"） ── */

/**
 * 剥离注释后再扫描。
 *
 * ⚠️ **必须剥**：本文件建立的当天就踩过一次 —— 注释里写着「原实现是 `useState<Region>("cn")`」，
 * 结果反例断言**匹配到注释**而误报。**锚必须只落在代码上**。
 * （简单正则对字符串里的 `//` 会误伤，但本断言只找 `useState` / 函数调用，风险可接受。）
 */
function stripComments(source) {
  // ⚠️ 顺序：**先剥行注释再剥块注释**。反过来的话，行注释里出现块注释起始标记
  //（本仓真实存在：`// … /api/oauth/…` 后跟一个星号）会被当成块注释起点，
  // 一路吃到下一个结束标记，把成百上千行源码吞掉 ⇒ 后面所有“handler 是否存在”
  // 的断言都变成假红（`check-api-contract.cjs` 踩过同一个坑）。
  return source.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
}

check("★ P2 路由：`/s/:scope/:dest` 已注册，且旧路由**保留**（旧书签可达）", () => {
  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));

  if (!/path="\/s\/:scope\/:dest"/.test(app)) {
    throw new Error("未注册 `/s/:scope/:dest` 路由");
  }

  // 旧路由必须仍在 —— 这是 P2 的验收项「全部旧链接可达」
  const legacy = [
    'path="/"',
    'path="/token-stats"',
    'path="/credit-stats"',
    'path="/api-service"',
    'path="/settings"',
    'path="/trae/accounts"',
    'path="/trae/token-stats"',
    'path="/trae/credits"',
    'path="/trae/api-service"',
    'path="/trae/settings"',
  ];
  const gone = legacy.filter((l) => !app.includes(l));
  if (gone.length) throw new Error(`旧路由被删了（旧书签会失效）：${gone.join(", ")}`);
  return `新路由 1 条 + 旧路由 ${legacy.length} 条并存`;
});

check("★ P2 目的地映射覆盖 3 平台 × 6 目的地（侧栏固定 6 项的依据）", () => {
  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));
  const m = app.match(/const SCOPED_ROUTES[\s\S]*?\n\};/);
  if (!m) throw new Error("找不到 SCOPED_ROUTES");
  const block = m[0];
  const bad = [];
  for (const dest of registry.DESTINATIONS) {
    // 每个 dest 应在表里出现三次（WorkBuddy / Trae / Qoder 各一次）
    const n = (block.match(new RegExp(`\\b${dest}:`, "g")) || []).length;
    if (n !== 3) bad.push(`${dest}(${n} 次，期望 3)`);
  }
  if (bad.length) throw new Error(`映射不完整：${bad.join(", ")}`);
  return `${registry.PRODUCTS.length} 平台 × ${registry.DESTINATIONS.length} 目的地`;
});

check("★ P2 侧栏：由注册表生成（`PRODUCT_NAV` 已删除，不再是 5:5 硬编码）", () => {
  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));

  if (/PRODUCT_NAV/.test(app)) {
    throw new Error("`PRODUCT_NAV` 又回来了 —— 侧栏应只由注册表生成，否则又是两份硬编码");
  }
  if (!/DESTINATIONS\.(filter|map)\(/.test(app)) {
    throw new Error("侧栏没有用 `DESTINATIONS` 生成");
  }
  if (!/\/s\/\$\{/.test(app)) {
    throw new Error("侧栏链接没有走 `/s/{scope}/{dest}` 形态");
  }
  return "PRODUCT_NAV 已删除，侧栏走注册表";
});

check("★ P3 统一状态语言：`EmptyState` 存在且具备四要素", () => {
  const p = path.join(ROOT, "src/components/ui/empty-state.tsx");
  if (!fs.existsSync(p)) throw new Error("缺少 `src/components/ui/empty-state.tsx`");

  const src = stripComments(fs.readFileSync(p, "utf8"));
  /* §3.6：空态 = 图标 + 标题 + 说明 + **出口**。四要素缺一不可 ——
     缺 `action` 的空态等于把用户丢在原地（这正是「一行灰字」的问题）。 */
  for (const k of ["icon", "title", "description", "action"]) {
    if (!new RegExp(`\\b${k}\\??:`).test(src)) throw new Error(`缺要素：${k}`);
  }
  if (!/export function EmptyState/.test(src)) throw new Error("没有导出 `EmptyState`");
  return "四要素齐备";
});

check("★ P3 空态迁移：已迁移处必须继续用 `EmptyState`（防回退）", () => {
  /* 存量里还有自建空态（`account-pool-card` / `AccountsPage` 等，它们的形态更复杂），
     所以这条**不做全仓零容忍**，只锁住**已经迁移过的那几处** —— 防的是"改回去"。 */
  const migrated = [
    /* 2026-10-13 跨平台对齐：WB 侧的 Key 表与请求日志**反向**补齐 —— 它们的空态
       原来是「一行灰字」，而 Trae 侧早已用 `EmptyState`。方向是 WB 跟上 Trae
       （§3.6「空态唯一 `empty-state.tsx`」对两平台同等生效）。 */
    "src/components/gateway/api-key-table.tsx",
    "src/components/gateway/request-log.tsx",
    "src/components/gateway/trae-api-key-table.tsx",
    "src/components/gateway/trae-model-list.tsx",
    "src/components/gateway/trae-request-log.tsx",
    /* Qoder 网关三件（2026-10-15）：新写的两个组件**一开始就用** `EmptyState`，
       登记在此防回退（与上面几条同一纪律）。 */
    "src/components/gateway/qoder-api-key-table.tsx",
    "src/components/gateway/qoder-model-list.tsx",
    "src/components/trae-export-accounts-dialog.tsx",
    /* 用量四视图（2026-10-12 跨平台对齐）：Trae 两页原来的「一行虚线框」空态
       已换成 `EmptyState`（含出口），并登记在此防回退。 */
    "src/pages/TraeTokenStatsPage.tsx",
    "src/pages/TraeCreditsPage.tsx",
  ];
  const regressed = migrated.filter((f) => {
    const code = stripComments(fs.readFileSync(path.join(ROOT, f), "utf8"));
    /* ⚠️ 判据是「用了 `EmptyState` 这个原语」，**不是**「属性与标签同一行」——
       此前写的是 `/<EmptyState /`（带空格），多行写法（`<EmptyState\n  icon=…`）会被误判成
       「没迁移」。这是**门禁自身的格式依赖**，不是代码问题（2026-10-12 撞到）。 */
    return !/<EmptyState\b/.test(code);
  });
  if (regressed.length) {
    throw new Error(`空态被改回自建形态：${regressed.map((f) => path.basename(f)).join(", ")}`);
  }
  return `${migrated.length} 处已迁移且未回退`;
});

check("★ P3 加载态：页面级加载不许用文字型占位（扫描 src/pages/*.tsx）", () => {
  const dir = path.join(ROOT, "src/pages");
  const pages = fs.readdirSync(dir).filter((f) => f.endsWith(".tsx")).sort();

  /* 判据：**裸 `<Loader2 />` 紧跟一段 i18n 文本 `{t(...)}`** —— 这正是「转圈 + 文字」并列
     渲染、把文字当加载提示的形态，§3.6 明令禁止（区块加载必须用 `<Skeleton>`）。
     刻意「贴住」`/>` 与 `{t(` 的相邻关系，是为了**天然排除按钮操作中态**：
       `{saving && <Loader2 />}`          → Loader2 后面是 `}`，不匹配
       `{busy ? <Loader2 /> : <Icon />}`  → Loader2 后面是 ` : <Icon />}`，不匹配
     这两类（Loader2 顶替按钮内图标）**是对的**，不该被误伤。
     ⚠️ 这是**全量扫描**（不是白名单）：新增页面自动纳入覆盖，不会再出现
     「逐个文件列 ⇒ 新页面永远不在范围内」的老问题。 */
  const TEXT_PLACEHOLDER = /<Loader2\b[^>]*\/>\s*\{t\(/;
  const offenders = pages.filter((f) =>
    TEXT_PLACEHOLDER.test(stripComments(fs.readFileSync(path.join(dir, f), "utf8"))),
  );
  if (offenders.length) {
    throw new Error(`页面级加载用了文字型占位（应改 <Skeleton>）：${offenders.join(", ")}`);
  }

  /* 正向守卫：TraeSettingsPage 的加载态仍须落在 `<Skeleton>`（不许退化成「什么都不显示」）。 */
  const trae = stripComments(fs.readFileSync(path.join(dir, "TraeSettingsPage.tsx"), "utf8"));
  if (/trae\.page\.settings\.loading/.test(trae)) {
    throw new Error("TraeSettingsPage 又用回了文字型加载态 —— §3.6 禁止纯文本，应用 `<Skeleton>` 占位");
  }
  if (!/<Skeleton/.test(trae)) throw new Error("TraeSettingsPage 加载占位没有用 `Skeleton`");

  return `扫描 ${pages.length} 个页面，无文字型加载占位；TraeSettingsPage 仍用 <Skeleton>`;
});

check("★ P3 概览页：两平台共用同一组件，且从路由推平台", () => {
  const p = path.join(ROOT, "src/pages/OverviewPage.tsx");
  if (!fs.existsSync(p)) throw new Error("缺少 `OverviewPage`");

  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));
  if (!/path="\/overview"/.test(app)) throw new Error("没有注册 `/overview` 路由");

  /* **两平台共用同一个组件** —— 这是 P3「差异只剩数据与能力开关」的第一个实例。
     如果哪天拆成两个文件，说明又走回「逐字复制」的老路。 */
  const block = app.match(/const SCOPED_ROUTES[\s\S]*?\n\};/);
  if (!block) throw new Error("找不到 SCOPED_ROUTES");
  const n = (block[0].match(/\boverview: OverviewPage,/g) || []).length;
  if (n !== 2) throw new Error(`\`overview\` 在 SCOPED_ROUTES 里出现 ${n} 次，期望 2（两平台共用）`);

  const page = stripComments(fs.readFileSync(p, "utf8"));
  if (/useScope\("workbuddy"\)/.test(page)) {
    throw new Error('概览页写死了 `useScope("workbuddy")` —— 应从路由推平台');
  }
  if (!/platformOfPath\(/.test(page)) throw new Error("概览页没有从路由推平台");

  // 状态语言（§3.6）
  if (!/<EmptyState/.test(page)) throw new Error("概览页空态没有用 `EmptyState`");
  if (!/<Skeleton/.test(page)) throw new Error("概览页加载占位没有用 `Skeleton`");

  return "存在 · 两平台共用 · 从路由推平台 · 状态语言合规";
});

check("★ P3 片段合并：只有一份 `gatewaySnippet`，且修掉的两个缺陷不许回归", () => {
  const components = [
    "src/components/gateway/integration-guide.tsx",
    "src/components/gateway/trae-integration-guide.tsx",
  ];
  for (const f of components) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, f), "utf8"));
    if (/function snippetFor/.test(code)) {
      throw new Error(`${path.basename(f)} 又出现了**本地** snippetFor —— 应共用 lib 里的实现`);
    }
    if (!/gatewaySnippet\(/.test(code)) {
      throw new Error(`${path.basename(f)} 没有用共享的 \`gatewaySnippet\``);
    }
  }

  const lib = stripComments(fs.readFileSync(path.join(ROOT, "src/lib/gateway-snippets.ts"), "utf8"));
  if (!/export function gatewaySnippet/.test(lib)) throw new Error("共享模块没有导出 `gatewaySnippet`");

  /* ★ 两条**真实缺陷**的护栏（合并的动因，不能回归）：
     ① 硬编码模型名 —— 模型一换，生成的片段就是错的；
     ② `claude-code` 片段缺 `ANTHROPIC_MODEL` —— 报错发生在客户端侧，极难排查。 */
  if (/GLM-5\.3/.test(lib)) throw new Error("共享实现里又硬编码了模型名（应走 `model` 参数）");
  if (!/ANTHROPIC_MODEL/.test(lib)) throw new Error("`claude-code` 片段又缺了 `ANTHROPIC_MODEL`");

  // `model` 必须是参数（而不是可选 / 有默认值）—— 可选就会有人不传、退回硬编码
  if (!/model:\s*string;/.test(lib)) throw new Error("`gatewaySnippet` 的 `model` 不是必传参数");

  return "一份实现 · 无硬编码模型名 · 有 ANTHROPIC_MODEL";
});

check("★ P3 `formatDate` 已共享（且不许把时间戳 0 渲染成假时间）", () => {
  const lib = stripComments(fs.readFileSync(path.join(ROOT, "src/lib/format.ts"), "utf8"));
  if (!/export function formatDate/.test(lib)) throw new Error("缺少共享的 `formatDate`");

  /* ★ 必须挡 falsy：WB 版原来只有 `Number.isNaN` 判断，
     时间戳 `0` 会被渲染成 `01-01 08:00` 这种**假时间**（看起来像真数据）。 */
  if (!/if \(!ts\) return/.test(lib)) {
    throw new Error("共享 `formatDate` 没有挡 falsy —— 时间戳 0 会渲染成假时间");
  }

  const consumers = [
    "src/components/gateway/api-key-table.tsx",
    "src/components/gateway/trae-api-key-table.tsx",
    // Qoder 侧同款（2026-10-15）：新写的 Key 表也必须用共享 `formatDate`。
    "src/components/gateway/qoder-api-key-table.tsx",
  ];
  for (const f of consumers) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, f), "utf8"));
    if (/function formatDate/.test(code)) {
      throw new Error(`${path.basename(f)} 又出现了本地 \`formatDate\``);
    }
    if (!/from "@\/lib\/format"/.test(code)) {
      throw new Error(`${path.basename(f)} 没有 import 共享的 \`formatDate\``);
    }
  }
  return "一份实现 · 挡 falsy · 两处共用";
});

check("★ P3 用量页：Token + 积分合并为一个目的地，且旧路径仍可达", () => {
  const p = path.join(ROOT, "src/pages/UsagePage.tsx");
  if (!fs.existsSync(p)) throw new Error("缺少 `UsagePage`");

  const page = stripComments(fs.readFileSync(p, "utf8"));
  if (!/TokenStatsPage/.test(page) || !/CreditStatsPage/.test(page)) {
    throw new Error("用量页没有同时承载 Token 与积分");
  }

  // §3.9：目的地是「概览 / 账号 / 用量 / 网关 / 设置」—— 不应再有独立的 credits
  if (registry.DESTINATIONS.includes("credits")) {
    throw new Error("`credits` 仍是独立目的地 —— 应已并入 `usage`（§3.9）");
  }

  // ★ 旧路径必须仍映射到 usage：**旧书签可达**是 P2/P3 的验收项
  for (const legacy of ["/token-stats", "/credit-stats", "/credits"]) {
    const got = registry.destinationOfLegacyPath(legacy);
    if (got !== "usage") throw new Error(`${legacy} → ${got}，期望 usage`);
  }
  return `目的地 ${registry.DESTINATIONS.length} 项 · 三条旧路径映射到 usage`;
});

check("★ 默认窗口：原型与项目**必须同一个数**（否则又是「原型一个数、项目一个数」）", () => {
  /* 2026-10-15 实测分叉：原型默认近 **7** 天、`UsagePage` 默认近 **30** 天。
     ⚠️ 翻遍 `PLAN.md` / `DECISIONS.md` / `P3-ARCH.md` / 提交历史：**两侧都没有决策记录**，
     都是整体快照里带进来的巧合 ⇒ 这是「意外分叉」，不是设计。
     判据支持 30：① 项目「被视图参数筛没」的兜底按钮就是「看近 30 天」；
     ② `A1` 系列注释与门禁全以 30 天为基准（`cols.length === 30`）；
     ③ 7 天趋势在宽图上稀；④ **改项目 = 静默改老用户首屏**（本仓最忌那类 bug）。
     ⇒ 统一到 30 天，**只改原型**。
     ⚠️ 原型文件**不过 `stripComments`**：它是朴素正则，而原型里有大量含 `//` 的字符串
     （URL 等），strip 后锚点可能被吃掉 ⇒ 直接读原文 + 行级正则。 */
  const win = fs.readFileSync(path.join(ROOT, "src/lib/usage-window.ts"), "utf8");
  const mWin = win.match(/DEFAULT_USAGE_WINDOW[^\n]*?days:\s*(\d+)/);
  if (!mWin) throw new Error("取不到 `DEFAULT_USAGE_WINDOW` 的天数");
  const projectDays = Number(mWin[1]);

  const protos = ["docs/prototype/demo-app.js", "docs/prototype/buddy-switch-prototype.html"];
  for (const p of protos) {
    const code = fs.readFileSync(path.join(ROOT, p), "utf8");
    const hits = code.match(/usageTab:\s*"trend",\s*range:\s*(\d+)/g) || [];
    if (hits.length !== 1) throw new Error(`${p} 默认 range 的锚点命中 ${hits.length} 次（期望 1）`);
    const got = Number(hits[0].match(/range:\s*(\d+)/)[1]);
    if (got !== projectDays) {
      throw new Error(`${p} 默认近 ${got} 天 ≠ 项目近 ${projectDays} 天 —— 原型与项目又分叉了`);
    }
  }
  return `原型两侧 + 项目默认一致 = 近 ${projectDays} 天`;
});

check("★ P3 用量页视图参数条**只有一条**：由 `UsagePage` 渲染，四个视图页内不许有区域选择器", () => {
  /* 原型 `viewUsage` 的结构是「页头 → seg（子 Tab）→ params → body」：
     **一条**参数条管全部子 Tab，`state.range` / `state.merge` 是**页面级**的。
     改造前每个视图页各持一份 `range` / `combineAll` 并各渲染一条 ⇒
     ① 同一件事两份实现（必然漂移）；② 切子 Tab 范围被悄悄重置。 */
  const page = stripComments(fs.readFileSync(path.join(ROOT, "src/pages/UsagePage.tsx"), "utf8"));
  if (!/<ViewParamBar\b/.test(page)) {
    throw new Error("`UsagePage` 没有渲染 `ViewParamBar` —— 参数条必须由页面级渲染一次");
  }
  if (!/useState<UsageWindow>/.test(page) || !/readPreferredCombineAll/.test(page)) {
    throw new Error("`UsagePage` 没有持有页面级视图参数（`window` / `combineAll`）");
  }
  /* ⚠️ 局部变量不许叫 `window`：那会**静默**屏蔽全局 `window`（`localStorage` / 事件 /
     任何 `window.*` 都会指向这个对象，而 `tsc` 不报错）。用 `viewWindow`。 */
  if (/const \[window, setWindow\] = useState/.test(page)) {
    throw new Error("`UsagePage` 用 `window` 当局部变量名 —— 会屏蔽全局 `window`");
  }

  /* 合并态控件（`RegionBar`）的唯一落点：参数条内部。 */
  const bar = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/usage/view-param-bar.tsx"), "utf8"),
  );
  if (!/<RegionBar value=\{combineAll\}/.test(bar)) {
    throw new Error("视图参数条没有把合并态绑到 `combineAll`（`RegionBar`）");
  }
  /* ★ 账本那一组是原型里**唯一**带色调的分段控件（`.seg.book`，`demo.css:1362`）：
     选中 = `primary 18%` 底 + `primary` 文字；前面还有一道 `.vsep`（`:1363`）。
     2026-10-22 实测漏了两处（截图报障）：账本被当成普通 `.seg` 渲染
     ⇒ 深色下 `card` 与 `muted` 同值，选中项只剩字重差别；且没有竖线，
     「时间范围」与「账本」在一个胶囊条里连成一片，读起来像一个六档控件。
     提示词只写在注释里守不住（重构 `Seg` 时没人会回头翻原型）⇒ 落成断言。 */
  if (!/<span className="h-5 w-px shrink-0 bg-border"/.test(bar)) {
    throw new Error("参数条的「账本」前缺 `.vsep` 竖分隔线 —— 会与「时间范围」连成一个控件");
  }
  if (!/tone="book"/.test(bar)) {
    throw new Error("账本 `Seg` 没有 `tone=\"book\"` —— 原型 `.seg.book` 的青色选中态丢了");
  }
  /* ★ 时间窗口的两枚新出口必须都在参数条上：**今天**与**自定义时间**。
     少了任何一枚，UI 就退回「只有 7/14/30」——而原型已经不是那样了。 */
  for (const [label, pattern] of [
    ["今天", /\btoday:\s*"wbStats\.usage\.range\.today"/],
    ["自定义时间", /custom:\s*"wbStats\.usage\.param\.custom"/],
  ]) {
    if (!pattern.test(bar)) throw new Error(`参数条缺少「${label}」档位映射`);
  }
  if (!/USAGE_WINDOW_MODES/.test(bar)) {
    throw new Error("参数条的档位顺序没有走 `USAGE_WINDOW_MODES`（档位表散成了两份）");
  }
  /* ★ 弹层里「可选范围」的边界必须是**两个独立日期**。
     拿区间文案去渲染单个日期会得到 `7/18 – 7/18`，套进 `{from} – {to}` 就成了
     `7/18 – 7/18 – 10/15`（实测踩到，截图里看得见）。 */
  if (!/shortDateLabel\(/.test(bar)) {
    throw new Error("自定义弹层的「可选范围」提示没有用 `shortDateLabel`（会渲染出双破折号）");
  }
  if (/custom\.hint[\s\S]{0,200}usageCustomRangeLabel/.test(bar)) {
    throw new Error("「可选范围」提示用了区间文案 —— 单个日期会渲染成 `x – x`");
  }
  /* ★「空日期不得静默变成今天」（2026-10-15 修）。
     原型是**拒绝**（`toast("err","请选择起止日期")`），React 侧修复前是**静默应用**
     ⇒ `clampCustom` 把空串兜底成「今天」，用户以为生效的是自己选的那段。
     实测（CDP 变异对照）没这道守卫时窗口从「近 7 天」变成「10/15 – 10/15」。
     两道防线都要在：**禁用态**挡住正常操作，**守卫**挡住键盘/程序化绕过。 */
  if (!/if \(!draft\.from \|\| !draft\.to\) return;/.test(bar)) {
    throw new Error("自定义弹层没有「起止为空 ⇒ 拒绝应用」的守卫 —— 会静默把窗口改成「今天」");
  }
  if (!/disabled=\{customIncomplete\}/.test(bar)) {
    throw new Error("起止为空时「应用」没有禁用（守卫可被绕过，禁用才是第一道防线）");
  }
  if (!/customIncomplete/.test(bar)) {
    throw new Error("没有 `customIncomplete` 判据 —— 禁用态与守卫会各自漂移");
  }
  if (!/t\("wbStats\.usage\.param\.custom\.incomplete"\)/.test(bar)) {
    throw new Error("起止为空时没有解释原因（无解释的禁用是新的静默失败）");
  }
  /* ★ 开浮层的那一档必须声明 `aria-haspopup` / `aria-expanded`（原型有，React 侧曾漏）。 */
  if (!/popup=\{\{ value: "custom", expanded: open \}\}/.test(bar)) {
    throw new Error("参数条没有把「自定义时间」标成 popup 档（读屏不知道它会开浮层）");
  }
  const seg = stripComments(fs.readFileSync(path.join(ROOT, "src/components/ui/seg.tsx"), "utf8"));
  if (!/isPopup/.test(seg) || !/aria-haspopup=\{isPopup \? "dialog" : undefined\}/.test(seg)) {
    throw new Error("`Seg` 没有 popup 档能力（aria-haspopup / aria-expanded 无处落地）");
  }
  /* 账本色调的实现侧（原型 `.seg.book`，`demo.css:1362`）：
     `primary 18%` 底 + `primary` 文字，字重 550 与 `shadow-sm` 沿用默认选中态。 */
  if (!/tone === "book"/.test(seg) || !/bg-primary\/18/.test(seg) || !/text-primary\b/.test(seg)) {
    throw new Error("`Seg` 没有 `book` 色调（原型 `.seg.book`：`primary 18%` 底 + `primary` 文字）");
  }
  /* ★ 窗口的类型与纯函数只有一处定义：`lib/usage-window.ts`。
     `usage-charts.ts` 是消费方 —— 它自己再定义一次 `UsageWindow` 就是两份真值。 */
  const win = stripComments(fs.readFileSync(path.join(ROOT, "src/lib/usage-window.ts"), "utf8"));
  if (!/export type UsageWindow\b/.test(win)) {
    throw new Error("`lib/usage-window.ts` 没有导出 `UsageWindow`（窗口的唯一真值）");
  }
  for (const name of ["usage-charts", "trae-usage-charts"]) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, `src/lib/${name}.ts`), "utf8"));
    if (/=\s*7 \| 14 \| 30/.test(code)) {
      throw new Error(`\`${name}.ts\` 又定义了一份「7|14|30」天数类型 —— 天数已不能表达窗口`);
    }
  }

  /* 四个视图页：**都不许**自己再放参数条 / 区域选择器 / 自持参数状态。
     ★ 2026-10-18：`QoderUsagePage` 加入名单（Qoder 用量页接回，`usage` 入口不再停在
       占位页）—— 它同样只吃 `UsagePage` 传入的 `viewWindow`。 */
  const views = [
    "TokenStatsPage",
    "CreditStatsPage",
    "TraeTokenStatsPage",
    "TraeCreditsPage",
    "QoderUsagePage",
  ];
  for (const name of views) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, `src/pages/${name}.tsx`), "utf8"));
    if (/<ViewParamBar\b/.test(code)) {
      throw new Error(`${name} 又自己渲染参数条了 —— 参数条只在 \`UsagePage\` 一处`);
    }
    if (/<RegionBar\b/.test(code)) {
      throw new Error(`${name} 又自己渲染 \`RegionBar\` 了 —— 合并态归参数条`);
    }
    /* 不许解构 `useScope` 的 setter：页面内**不该有能力**切区域，
       否则又会出现「两个看起来能切的控件」。 */
    if (/const \[[A-Za-z]+, set[A-Za-z]+\] = useScope\(/.test(code)) {
      throw new Error(`${name} 又取了 \`useScope\` 的 setter —— 页面内不该能切区域`);
    }
    if (/useState<UsageWindow>|useState\(\(\) => readPreferredCombineAll/.test(code)) {
      throw new Error(`${name} 又自持视图参数状态了 —— 那是页面级的（切子 Tab 不重置）`);
    }
  }
  return `参数条唯一落点 = UsagePage；${views.length} 个视图页无区域选择器、无自持参数`;
});

check("★ 用量四视图两平台共用同一套骨架（统计卡 / 图表卡 / 受控参数）", () => {
  /* 用户 2026-10-12 要求：「用量 tab 下 WorkBuddy 与 TraeWork 的 UI 和交互必须一致，
     都严格对齐原型」。原型 `viewUsage` 是**平台无关**的（同一段代码 + 不同数据），
     所以四个视图页必须共用同一批原语，差异**只能**走数据。
     改造前的分叉：Trae 两页各自手搓「一张卡带分隔线、26px 居中数字」的统计块与手写卡片头。 */
  const views = ["TokenStatsPage", "CreditStatsPage", "TraeTokenStatsPage", "TraeCreditsPage"];
  for (const name of views) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, `src/pages/${name}.tsx`), "utf8"));

    if (!/<StatTiles\b/.test(code)) {
      throw new Error(`${name} 没有用共用的 \`StatTiles\` —— 统计卡会两平台两套几何`);
    }
    if (!/<ChartFrame\b/.test(code)) {
      throw new Error(`${name} 没有用共用的 \`ChartFrame\` —— 图表卡会两平台两套外壳`);
    }
    if (!/UsageViewProps/.test(code)) {
      throw new Error(`${name} 没有接收受控视图参数（\`UsageViewProps\`）`);
    }
    /* 反向断言：不许再手搓统计块 / 卡片头。 */
    if (/text-\[26px\]/.test(code)) {
      throw new Error(`${name} 又手搓了 26px 统计块（应走 \`StatTiles\`）`);
    }
    if (/function StatMetric\(/.test(code)) {
      throw new Error(`${name} 又定义了局部 \`StatMetric\`（应走 \`StatTiles\`）`);
    }
    if (/text-\[13\.5px\]/.test(code)) {
      throw new Error(`${name} 又手写了卡片头字号（应走 \`CardHead\` / \`ChartFrame\`）`);
    }
  }
  return `${views.length} 个视图页都走 StatTiles + ChartFrame + 受控参数`;
});

check("★ 用量四视图的**图表集**两平台同源（原型块序：堆叠柱 / 累计曲线 / 环形 / 7×24 热力图 / 排行）", () => {
  /* 用户 2026-10-12：「用量 tab 中的 TraeWork 要严格按照原型的实现」。
     原型 `viewUsage` 的图表是**平台无关**的固定几张，所以两平台必须渲染同一批原语 ——
     各写一套（Trae 改造前就是：整年活动网格 / 折线 / 自建柱状图）必然与原型的块序分叉。
     反向断言「不许直接引 recharts」：一旦某个页面自己画图，共用原语的契约就绕过去了。 */
  const tokenPages = ["TokenStatsPage", "TraeTokenStatsPage"];
  const creditPages = ["CreditStatsPage", "TraeCreditsPage"];
  const tokenCharts = [
    ["StackedDailyBar", /<StackedDailyBar\b/],
    ["CumulativeArea", /<CumulativeArea\b/],
    ["ModelDonut", /<ModelDonut\b/],
    ["UsageHeatmap", /<UsageHeatmap\b/],
    ["ModelRanking", /<ModelRanking\b/],
  ];
  const creditCharts = [
    ["CreditDivergingBar", /<CreditDivergingBar\b/],
    ["CreditBalanceArea", /<CreditBalanceArea\b/],
    ["CreditExpiryBars", /<CreditExpiryBars\b/],
  ];
  for (const [pages, charts] of [
    [tokenPages, tokenCharts],
    [creditPages, creditCharts],
  ]) {
    for (const name of pages) {
      const code = stripComments(fs.readFileSync(path.join(ROOT, `src/pages/${name}.tsx`), "utf8"));
      for (const [label, re] of charts) {
        if (!re.test(code)) throw new Error(`${name} 缺原型的图表原语 \`${label}\``);
      }
      if (/from "recharts"/.test(code)) {
        throw new Error(`${name} 又直接引 recharts 自己画图了 —— 原型的图必须走共用原语`);
      }
    }
  }
  return `${tokenPages.length} 页 × ${tokenCharts.length} 张 Token 图 + ${creditPages.length} 页 × ${creditCharts.length} 张积分图`;
});

check("★ Trae 用量适配层：映射成共用形状后走**同一批**图表适配函数（跑真代码）", () => {
  /* 判据：Trae 的载荷（网关日志）与 WB 的形状不同，但图表需要的维度一致
     ⇒ 只做映射、不另写一套。这里跑真代码验证四条契约：
     ① 7×24 热力矩阵；② 堆叠柱列数 = 窗口天数且分段和 = 当日总量；
     ③ 环形扇区数 = 有窗口内用量的模型数；④ 积分逐日柱**双向**（Trae 的入账可得）。 */
  const tu = loadTs(path.join(ROOT, "src/lib/trae-usage-charts.ts"));
  const uc = loadTs(path.join(ROOT, "src/lib/usage-charts.ts"));

  const dayKey = (daysAgo) => {
    const date = new Date(Date.now() - daysAgo * 86_400_000);
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  };
  const bucket = (key, total) => ({
    key, total, input: total, output: 0, records: 1, errors: 0,
    streamRequests: 0, avgLatencyMs: 0, p95LatencyMs: 0,
  });
  const stats = {
    source: "trae-gateway", label: "", generatedAt: 0, rangeDays: 30,
    logFile: "", summary: bucket("summary", 60),
    models: [bucket("m-a", 40), bucket("m-b", 20)],
    accounts: [{ ...bucket("u1", 60), name: "甲", shortId: "u1" }],
    /* ⚠️ `daily` 与 `modelDaily` 必须**同源**（真实后端两者都由同一批日志条目聚合）——
       若某天只在 `modelDaily` 里出现，「分段之和 = 当日总量」这条契约就不可能成立。 */
    daily: [bucket(dayKey(0), 60), bucket(dayKey(1), 0), bucket(dayKey(3), 9)],
    /* 「周-小时」槽位：周一 09 点两笔累加 ⇒ 6；非法键必须被忽略。 */
    hours: [
      { ...bucket("0-09", 5), key: "0-09" },
      { ...bucket("0-09", 1), key: "0-09" },
      { ...bucket("6-23", 7), key: "6-23" },
      { ...bucket("oops", 99), key: "oops" },
    ],
    statuses: [{ key: "200", records: 2 }],
    modelDaily: [
      { date: dayKey(0), model: "m-a", total: 40, input: 40, output: 0, records: 1 },
      { date: dayKey(0), model: "m-b", total: 20, input: 20, output: 0, records: 1 },
      { date: dayKey(3), model: "m-a", total: 9, input: 9, output: 0, records: 1 },
    ],
    variantCounts: { cn: 1, global: 0, unlabeled: 0, all: 1 },
    unsupported: [], filesScanned: 1, parseErrors: 0,
    coverageStartAt: null, coverageEndAt: null, note: "",
  };

  const source = tu.toTokenStatsSource(stats);
  if (source.hours.length !== 4) throw new Error("hours 没有原样带过来");

  const matrix = uc.heatmapMatrix(source.hours);
  if (matrix.rows !== 7 || matrix.cols !== 24) throw new Error(`热力矩阵应为 7×24，实得 ${matrix.rows}×${matrix.cols}`);
  if (matrix.cells[0][9] !== 6) throw new Error(`周一 09 点应为 6（5+1），实得 ${matrix.cells[0][9]}`);
  if (matrix.cells[6][23] !== 7) throw new Error(`周日 23 点应为 7，实得 ${matrix.cells[6][23]}`);
  if (matrix.cells.flat().reduce((a, b) => a + b, 0) !== 13) throw new Error("非法槽位键没有被忽略");

  const cols = uc.dailyModelSeries(source, presetWin(30));
  if (cols.length !== 30) throw new Error(`堆叠柱列数应 = 窗口天数 30，实得 ${cols.length}`);
  for (const col of cols) {
    const sum = col.segments.reduce((acc, seg) => acc + seg.value, 0);
    if (Math.abs(sum - col.total) > 1e-6) throw new Error(`${col.day} 分段之和 ${sum} ≠ 当日总量 ${col.total}`);
  }
  const today = cols[cols.length - 1];
  if (today.total !== 60 || today.segments.length !== 2) {
    throw new Error(`今天应有 2 段共 60，实得 ${today.segments.length} 段 / ${today.total}`);
  }

  const slices = uc.modelSlices(source, presetWin(30));
  if (slices.length !== 2) throw new Error(`扇区数应 = 窗口内有量的模型数 2，实得 ${slices.length}`);
  if (slices[0].value !== 49) throw new Error(`头部模型窗口内应为 49（40+9），实得 ${slices[0].value}`);

  const bars = tu.traeCreditBars([{ date: dayKey(0), total: 10, earned: 3, consumed: 1 }], presetWin(7));
  if (bars.length !== 7) throw new Error(`逐日柱应铺满窗口 7 天，实得 ${bars.length}`);
  const last = bars[bars.length - 1];
  if (last.inAmt !== 3 || last.outAmt !== 1) {
    throw new Error(`Trae 的入账可得 ⇒ 必须是双向柱（实得 in=${last.inAmt} / out=${last.outAmt}）`);
  }

  return `热力 7×24（周一09=6）· 堆叠 30 列且分段和=总量 · 扇区 2 个 · 积分双向柱 in=3/out=1`;
});

check("★ 用量·积分「合并全部作用域」**真的聚合**（跨区域按区域前缀分行，绝不按 uid 折叠）", () => {
  /* 合并态是**视图参数**，不是装饰：切了数据必须变，否则就是本仓明令禁止的「假控件」。
     Trae 的积分按区域分家、后端没有「一次取两区」的命令 ⇒ 合并只能在前端做，
     本函数就是那段逻辑（`src/lib/trae-credits-merge.ts`）。**跑真代码**断言。 */
  const m = loadTs(path.join(ROOT, "src/lib/trae-credits-merge.ts"));
  const slice = (region, uid, credits, daily = []) => ({
    region,
    credits: {
      remaining: { [uid]: credits },
      expireTimes: {},
      packages: {},
      updatedAt: null,
      balances: [],
      records: [],
      daily,
      todayEarned: 0,
      historyDays: 30,
      unsupported: [],
    },
    overview: { accounts: [{ userId: uid, name: `acc-${uid}`, groupId: null }], groups: [] },
  });

  const merged = m.mergeTraeCredits([
    slice("cn", "u1", 100, [{ date: "2026-10-11", total: 100, earned: 10, consumed: 1 }]),
    slice("global", "u1", 7, [{ date: "2026-10-11", total: 7, earned: 2, consumed: 0 }]),
  ]);
  if (merged.total !== 107) throw new Error(`可用积分合计应为 107（100 + 7），实得 ${merged.total}`);
  if (merged.rows.length !== 2) {
    throw new Error(`同一 uid 在两个区域是**两个账号** ⇒ 应 2 行，实得 ${merged.rows.length}`);
  }
  if (new Set(merged.rows.map((r) => r.key)).size !== 2) {
    throw new Error("行键没带区域前缀 —— 跨区被折叠成一行了");
  }
  const day = merged.daily.find((d) => d.date === "2026-10-11");
  if (!day || day.total !== 107 || day.earned !== 12 || day.consumed !== 1) {
    throw new Error(`每日快照没按日期求和：${JSON.stringify(day)}`);
  }

  /* 单区也走同一函数 ⇒ 页面只有**一条**渲染路径（这正是当初两平台分叉的成因）。 */
  const single = m.mergeTraeCredits([slice("cn", "u1", 100)]);
  if (single.total !== 100 || single.rows.length !== 1 || single.daily.length !== 0) {
    throw new Error("单区路径与合并路径形状不一致");
  }

  /* 到期分布：Trae 的行里 `expireAt` 是**秒**、`CreditResource` 是**毫秒** ——
     不换算会被当成「已过期」而**静默剔除**（「即将过期」永远 0）。 */
  const tu = loadTs(path.join(ROOT, "src/lib/trae-usage-charts.ts"));
  const now = 1_800_000_000_000;
  const expiry = tu.traeCreditExpiry(
    [
      { credits: 5, expireAt: Math.floor((now + 3 * 86_400_000) / 1000) },
      { credits: 9, expireAt: Math.floor((now - 86_400_000) / 1000) },
      { credits: 4, expireAt: Math.floor((now + 90 * 86_400_000) / 1000) },
      { credits: null, expireAt: null },
    ],
    now,
  );
  const uc = loadTs(path.join(ROOT, "src/lib/usage-charts.ts"));
  const buckets = uc.expiryBuckets(expiry, now);
  if (buckets.length !== 30) throw new Error(`到期轴应为 30 天，实得 ${buckets.length}`);
  const bucketTotal = buckets.reduce((sum, bucket) => sum + bucket.value, 0);
  if (bucketTotal !== 5) throw new Error(`未来 30 天内到期应为 5（已过期 / 超窗口 / 未同步都不计），实得 ${bucketTotal}`);
  if (buckets[2].value !== 5) throw new Error(`D+3 应为 5，实得 ${buckets[2].value}`);

  return `合计 ${merged.total} · 两区同 uid ⇒ ${merged.rows.length} 行 · 到期轴 30 天 · D+3 = ${buckets[2].value}`;
});

check("★ P3 用量页三平台共用同一组件（同概览页的模式；Qoder 于 2026-10-18 接回）", () => {
  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));
  const block = app.match(/const SCOPED_ROUTES[\s\S]*?\n\};/);
  if (!block) throw new Error("找不到 SCOPED_ROUTES");

  /* 三个平台（workbuddy / trae / qoder）都把 `usage` 指到同一个 `UsagePage`。
     ★ 2026-10-18：Qoder 由 `QoderPendingPage` 改为 `UsagePage`（issue #46 的 C 项）——
       后端 `qoder_usage` 命令、`USAGE_PAGES.qoder`、`QoderUsagePage` 早已就位，
       入口停在占位页 = 有真实数据却显示「尚未接入」。期望值随之 2 → 3。 */
  const n = (block[0].match(/\busage: UsagePage,/g) || []).length;
  if (n !== 3) {
    throw new Error(`\`usage: UsagePage\` 在 SCOPED_ROUTES 里出现 ${n} 次，期望 3（三平台共用）`);
  }
  /* 反向钉住「不要退回占位页」：Qoder 的 usage 入口一旦改回 `QoderPendingPage` 立即红。 */
  if (!/qoder:\s*\{[\s\S]*?usage: UsagePage,/.test(block[0])) {
    throw new Error("qoder 的 usage 入口没有指向 UsagePage —— 不该退回占位页");
  }

  const page = stripComments(fs.readFileSync(path.join(ROOT, "src/pages/UsagePage.tsx"), "utf8"));
  if (!/platformOfPath\(/.test(page)) throw new Error("用量页没有从路由推平台");
  if (!/USAGE_PAGES/.test(page)) throw new Error("用量页没有按平台映射子页面");

  /* 六个页面都要 **lazy**：都静态引入会让「用量」首次加载同时拉多页。 */
  for (const c of [
    "TokenStatsPage",
    "CreditStatsPage",
    "TraeTokenStatsPage",
    "TraeCreditsPage",
    "QoderUsagePage",
    "QoderCreditsPage",
  ]) {
    if (!new RegExp(`lazy\\(\\(\\) => import\\("@/pages/${c}"\\)\\)`).test(page)) {
      throw new Error(`用量页没有 lazy 加载 \`${c}\``);
    }
  }
  /* ★ 2026-10-21：Qoder 四个视图齐了（原先只有趋势）。
     守三件事：① 四个子 Tab 都在；② `tokenViews: true`（否则趋势/模型/会话传不进 `view`）；
     ③ credits 指向**独立**的余额页（指回 QoderUsagePage 就退回「积分页显示趋势图」那个错）。 */
  if (!/qoder:\s*\[[^\]]*"trend"[^\]]*"models"[^\]]*"sessions"[^\]]*"credits"/.test(page)) {
    throw new Error("`PLATFORM_TABS.qoder` 没有四个视图（趋势 / 模型 / 会话 / 积分）");
  }
  if (!/qoder:\s*\{[^}]*tokenViews: true/.test(page)) {
    throw new Error("`USAGE_PAGES.qoder` 没开 token 视图 —— 趋势/模型/会话三视图传不进 `view`");
  }
  if (!/qoder:\s*\{[^}]*credits: QoderCreditsPage/.test(page)) {
    throw new Error("`USAGE_PAGES.qoder.credits` 没有指向 `QoderCreditsPage`（余额是另一个口径）");
  }
  return "三平台共用 · 从路由推平台 · 6 个子页面均 lazy · Qoder 四视图（余额独立成页）";
});

check("★ P2 顶栏作用域选择器：存在 ScopeSelector · 由 TopBar 渲染 · 覆盖全部作用域 · 不在页面内", () => {
  const scopeSelector = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/shell/scope-selector.tsx"), "utf8"),
  );
  const topBar = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/shell/top-bar.tsx"), "utf8"),
  );
  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));

  /* ① 组件存在且导出（L1 作用域的唯一控件）。 */
  if (!/export function ScopeSelector\(/.test(scopeSelector)) {
    throw new Error("没有 `ScopeSelector` 组件（`src/components/shell/scope-selector.tsx`）");
  }

  /* ② 覆盖**全部**作用域：必须遍历注册表常量 `SCOPES`（可带过滤）——
     写死会在新增平台/区域时静默漏项（正是本门禁要防的那类漂移）。
     P17：白名单模式允许使用 `SCOPES.filter((s) => visibleScopes.includes(s)).map(...)` */
  if (!/SCOPES\.filter[\s\S]*?visibleScopes[\s\S]*?\.map\s*\(|SCOPES\.map\s*\(/.test(scopeSelector)) {
    throw new Error("`ScopeSelector` 没有遍历 `SCOPES` —— 作用域菜单会漏项");
  }

  /* ③ 渲染在**顶栏**（L1 位置固定、不随产品/页面换血，§3.1）。 */
  if (!/<ScopeSelector\b/.test(topBar)) {
    throw new Error("`ScopeSelector` 没有被 `TopBar` 渲染");
  }
  if (!/<TopBar\b/.test(app)) {
    throw new Error("`TopBar` 没有被 `Layout` 渲染");
  }

  /* ④ 旧形态必须**彻底删除**：侧栏的产品 Tab / 区域 Tab 不得复活 ——
     两个位置各放一个「看起来能切」的控件，用户就不知道哪个生效（§1.5 纪律）。 */
  if (/ProductSwitch|ScopeSwitch/.test(app)) {
    throw new Error("App.tsx 仍残留 `ProductSwitch` / `ScopeSwitch` —— 作用域只应在顶栏一处");
  }

  /* ⑤ 页面内不得出现作用域选择器（同上：避免第二处「看起来能切」）。 */
  const pagesDir = path.join(ROOT, "src/pages");
  for (const file of fs.readdirSync(pagesDir)) {
    if (!file.endsWith(".tsx")) continue;
    const page = stripComments(fs.readFileSync(path.join(pagesDir, file), "utf8"));
    if (/ScopeSelector/.test(page)) {
      throw new Error(`页面 ${file} 内出现 \`ScopeSelector\` —— 作用域切换只应在顶栏`);
    }
  }

  return "顶栏渲染 · 覆盖 SCOPES · 旧侧栏切换器已删 · 页面内无";
});

check("★ P1 接线：三个区域页面都走 `useScope`（不许退回各页自持状态）", () => {
  const files = [
    "src/pages/AccountsPage.tsx",
    "src/pages/TokenStatsPage.tsx",
    "src/pages/CreditStatsPage.tsx",
  ];
  const code = new Map(
    files.map((f) => [f, stripComments(fs.readFileSync(path.join(ROOT, f), "utf8"))]),
  );

  const missing = files.filter((f) => !/useScope\(/.test(code.get(f)));
  if (missing.length) throw new Error(`未接入 useScope：${missing.join(", ")}`);

  // 反例：账号页不该再有「组件自持区域」——那正是 P1 要消除的缺陷
  if (/useState<Region>\(\s*"cn"\s*\)/.test(code.get(files[0]))) {
    throw new Error('账号页又出现 useState<Region>("cn") —— 那是 P1 之前的自持状态');
  }
  return `${files.length} 个页面已接入`;
});

check("★ 统计页不再回写旧的「各页独立」偏好键（P1 起由 useScope 统一记忆）", () => {
  for (const f of ["src/pages/TokenStatsPage.tsx", "src/pages/CreditStatsPage.tsx"]) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, f), "utf8"));
    if (/persistPreferredRegion\s*\(/.test(code)) {
      throw new Error(`${f} 仍在回写旧键 —— 作用域记忆已统一到 useScope`);
    }
  }
  return "两个统计页都不再回写旧键";
});

/* ── P4 账号工作台（详情抽屉 + 卡片瘦身 + 程序位常显）─────────────────────────
 *
 * 上面 28 条覆盖 P0–P3；P4 此前**一条都没有** ⇒「门禁全绿」只证明 P0–P3 没回退，
 * 证明不了 P4 正确（上一个工程师的两条变异打的就是 P3 的断言）。
 * 下面 3 条锁住 P4 最容易回退的三个判据：卡片可点区口径、TraeProgram.region、programsFor 去重口径。
 */

/**
 * 列出 `src/` 下全部 `.ts` / `.tsx`（绝对路径）。
 *
 * 为什么全仓扫：`TraeProgram` 的构造点**可能新增**，而 §1.4 要求「所有构造点都带 region」——
 * 只锁一个文件挡不住未来的第二个构造点。
 */
function listSourceFiles(dir = path.join(ROOT, "src")) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...listSourceFiles(full));
    else if (/\.(ts|tsx)$/.test(entry.name)) out.push(full);
  }
  return out;
}

/**
 * 取 `index`（指向某个键 / 某个 `{`）所在的**最近一层** `{ … }` 文本。
 *
 * 为什么用括号配对而不是正则：构造点里嵌着模板串 `${…}`（`key: \`${region}:${variant}\``），
 * 正则划不出「这个对象到哪结束」，配对可以。
 */
function enclosingBraces(code, index) {
  let depth = 0;
  let start = -1;
  for (let i = index; i >= 0; i -= 1) {
    if (code[i] === "}") depth += 1;
    else if (code[i] === "{") {
      if (depth === 0) {
        start = i;
        break;
      }
      depth -= 1;
    }
  }
  if (start < 0) return null;
  let d = 0;
  for (let j = start; j < code.length; j += 1) {
    if (code[j] === "{") d += 1;
    else if (code[j] === "}") {
      d -= 1;
      if (d === 0) return code.slice(start, j + 1);
    }
  }
  return null;
}

check("★ P4 卡片常规可点区 ≤4：程序位行算 1 组（勾选框 / 程序位行 / 更多菜单 = 3）", () => {
  /* §1.3：卡片瘦身把「备注编辑」「资源包展开」移进抽屉，常规可点区收敛到 3。
     口径：**程序位行算 1 组**（不是 3 枚按钮）—— 它是单一定义、页脚引用一次。
     所以静态计数**不数 `<Button>`**（那会把 3 枚程序位按钮数成 3 组），而数**可点组**：
     勾选框(1) + 程序位行(1) + 更多菜单(1) = 3 ≤ 4。 */
  /* ★ 2026-10-10：两张卡的程序位行**改用同一个名字** `programRow`（Trae 侧原为 `programControls`）。
     本次重构的目标是「用原型统一两个平台的交互」，同一结构在两处叫两个名字只会让人以为它们不同。
     名字一致之后，下面的计数断言对两张卡是**同一套判据** —— 顺带把「两卡同构」钉住。 */
  const cards = [
    { file: "src/components/account-card.tsx", row: "programRow" },
    { file: "src/components/trae-account-card.tsx", row: "programRow" },
  ];
  const summary = [];
  for (const { file, row } of cards) {
    const base = path.basename(file);
    const code = stripComments(fs.readFileSync(path.join(ROOT, file), "utf8"));

    /* 防回退①：备注编辑 / 资源包展开的可点区已迁入抽屉（§1.3 移出项），卡片里不得再有。
       这两个符号是那两类交互的入口；重新出现 = 可点区又膨胀、且卡片与抽屉出现两个入口。 */
    for (const gone of ["beginRemarkEdit", "setResourcesOpen"]) {
      if (code.includes(gone)) {
        throw new Error(`${base} 又出现 \`${gone}\` —— 该可点区已迁入抽屉（§1.3），不应留在卡片`);
      }
    }

    /* 防回退②：程序位行必须**单一定义 + 一处引用**（页脚）。
       拆成多处定义会被静态计数当成多组，且两套渲染迟早漂移（§1.3 收敛动因）。 */
    const defs = (code.match(new RegExp(`const\\s+${row}\\s*=`, "g")) || []).length;
    if (defs !== 1) {
      throw new Error(`${base} 的程序位行 \`${row}\` 定义了 ${defs} 处，应为 1 处（单一定义）`);
    }
    const refs = (code.match(new RegExp(`\\{${row}\\}`, "g")) || []).length;
    if (refs !== 1) {
      throw new Error(`${base} 的程序位行 \`${row}\` 被引用 ${refs} 次，应为 1 次（页脚）`);
    }

    // 可点组计数：勾选框 + 程序位行 + 更多菜单（菜单以其触发器 `DropdownMenuTrigger` 代表存在）。
    const checkboxDefs = (code.match(/const\s+checkbox\s*=/g) || []).length;
    if (checkboxDefs !== 1) throw new Error(`${base} 的勾选框定义了 ${checkboxDefs} 处，应为 1 处`);
    const groups = checkboxDefs + defs + (/DropdownMenuTrigger/.test(code) ? 1 : 0);
    if (groups > 4) throw new Error(`${base} 常规可点区 ${groups} 组，超过 4（§1.3）`);
    summary.push(`${base}=${groups}`);
  }
  return summary.join(" / ");
});

check("★ P4 TraeProgram.region 在所有构造点都被赋值（跨区域消歧的前提）", () => {
  /* §1.4 第 1 步：`TraeProgram` 新增 `region`（来源区域）。任何构造它的地方都必须填 ——
     漏填会让 `region` 变成 undefined：卡片无法给同名程序位消歧、抽屉徽标 key 变成
     `"undefined:…"`。判据用**对象字面量签名**识别构造点：同一对象同时有 `variant:` 与
     `hasDataDir:`。`DetailProgramBadge` 只有 `hasDataDir`、没有 `variant`，不会误伤。 */
  const sites = [];
  const missing = [];
  for (const abs of listSourceFiles()) {
    const rel = path.relative(ROOT, abs);
    const code = stripComments(fs.readFileSync(abs, "utf8"));
    const re = /\bhasDataDir\s*:/g;
    let m;
    while ((m = re.exec(code))) {
      const obj = enclosingBraces(code, m.index);
      if (!obj) continue;
      if (!/\bvariant\s*:/.test(obj)) continue; // 不是 TraeProgram 形状
      sites.push(rel);
      if (!/\bregion\s*:/.test(obj)) missing.push(rel);
    }
  }
  if (sites.length === 0) {
    throw new Error("找不到任何 TraeProgram 构造点 —— 断言失效（`hasDataDir` / `variant` 是否改名？）");
  }
  if (missing.length) {
    throw new Error(
      `TraeProgram 构造点缺 region：${[...new Set(missing)].join(", ")}（跨区域同名程序位无法消歧）`,
    );
  }
  return `${sites.length} 处 TraeProgram 形状均带 region`;
});

check("★ P4 programsFor 不许按 program 去重（TraeProgramId 不含区域）", () => {
  /* §1.4 第 2 步：`TraeProgramId` 只有 2 个取值（`trae_work` / `trae_code`）且**不含区域**，
     按 `program` 去重会把 `(cn, trae_work)` 与 `(global, trae_work)` 合并成一条、
     丢掉「该写哪个客户端」。
     ⚠️ 2026-10-12：原判据是「函数体内出现 `.program` 即报警」—— 那是**代理信号**，
     而卡片按钮的可见短名（`Work` / `Code`）**必须**从 `program` 取
     （`variant` / `label` 在 `variant.rs` 里语义不唯一，会把 Code 判成 Work）
     ⇒ 代理信号产生**误报**，逼人改用不可靠的猜法。收紧为**真正的去重构造**：
     任何去重都必然建 `Set`/`Map` 或 `filter`/`some` 并读 `.program`。 */
  const code = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/TraeAccountsPage.tsx"), "utf8"),
  );
  const start = code.indexOf("function programsFor(");
  if (start < 0) throw new Error("找不到 `programsFor` —— 断言失效（函数是否改名？）");
  const body = enclosingBraces(code, code.indexOf("{", start));
  if (!body) throw new Error("无法解析 `programsFor` 的函数体");
  if (/new\s+(Set|Map)\s*\([^)]*\.program\b|\.(filter|some|find|findIndex)\s*\([^)]*\.program\b/.test(body)) {
    throw new Error(
      "`programsFor` 里出现按 `program` 去重/去查的构造 —— 只能按 `variant` 去重（`program` 不含区域，去重会丢客户端）",
    );
  }
  return "programsFor 未按 program 去重";
});

check("★ 账号页只渲染**本区域**的程序位（国内版不得出现国际版的，用户 2026-10-10 要求）", () => {
  /* ★ 决策反转（2026-10-10）：原 §5 是「跨区域常显 + 显示层加区域标识消歧」，
     于是国内版页面上一直挂着「TraeWork AI 国际版 / Trae AI 国际版」两枚**点了不能直接切**的按钮。
     用户明确要求「国内版不要显示国外版的」⇒ 改为 `programsFor` **按当前区域过滤**，
     卡片里的跨区域分支（`crossRegion` / `regionLabelOf` / `regionHint`）随之删除。

     三条判据，证伪能力都在：
       ① `programsFor` 必须按 `variant` 过滤（删掉 `.filter(... status.variant === variant)` ⇒ 变红）；
       ② 卡片里**不得**再出现跨区域分支（`crossRegion` / `onSwitchRegion` 复活 ⇒ 变红）；
       ③ 切换目标仍必须是**区域限定**的 `program.variant`（改成 `program.program` 会丢客户端 ⇒ 变红）。 */
  /* ⚠️ 用字面路径而不是 `TRAE_PAGE` 常量：本断言的位置在常量声明**之前**（TDZ）。
     ★ 2026-10-21：实现已抽到 `lib/trae-programs.ts`（与聚合页共用同一份判定），
       所以这里读**新文件**，并额外断言账号页**确实调用它**——
       否则「页面改回内联实现、两处各写一份」会静默发生，判定又开始漂。 */
  const impl = readCode("src/lib/trae-programs.ts");
  const fn = /export function traeProgramsFor\([\s\S]*?\n\}/.exec(impl);
  if (!fn) throw new Error("找不到 `traeProgramsFor` —— 断言失效（是否改名？）");
  if (!/\.filter\(\(status\) => status\.variant === variant\)/.test(fn[0])) {
    throw new Error("`traeProgramsFor` 未按当前区域过滤 —— 国内版会显示国际版的程序位");
  }
  const page = readCode("src/pages/TraeAccountsPage.tsx");
  if (!/traeProgramsFor\(account, variant, variantStatuses, logins\)/.test(page)) {
    throw new Error("TraeAccountsPage 没有调用 `traeProgramsFor` —— 页面与聚合页会各写一份判定");
  }
  /* 绝不能按 `program` 去重（它不含区域，去重会丢掉「该写哪个客户端」）。 */
  if (/new Set\([^)]*\.map\(\(?[a-z]+\)? => [a-z]+\.program\)/.test(fn[0])) {
    throw new Error("`traeProgramsFor` 出现了按 `program` 去重/去查的构造 —— 只能按 `variant` 去重");
  }
  const card = readCode("src/components/trae-account-card.tsx");
  for (const gone of ["crossRegion", "onSwitchRegion", "regionLabelOf", "switch.regionHint"]) {
    if (card.includes(gone)) {
      throw new Error(
        `trae-account-card 又出现 \`${gone}\` —— 跨区域程序位已改为「不渲染」，这条支路是死代码`,
      );
    }
  }
  /* ⚠️ 2026-10-13（issue #20）：切换回调**多了一个 `label` 参数**
     （`onSwitchTo?.(account, program.variant, label)`）—— 展示名必须由调用方从
     后端下发的 `program.label` 传进去，**不能**在页面里用变体反推
     （合并版与遗留 TraeCode 的候选名逐字相同，反推会把 Trae 说成 TraeCode）。
     因此下面这条判据从「逐字匹配整条调用」放宽成「**第二个实参必须是 `program.variant`**」——
     判据的**性质没变**（切换目标仍必须是区域限定的变体，用 `program.program` 会丢客户端），
     只是不再把「后面还有没有参数」也当成契约。 */
  if (!/onSwitchTo\?\.\(\s*account,\s*program\.variant\b/.test(card)) {
    throw new Error("切换目标不是区域限定的 `program.variant`（用 `program.program` 会丢客户端）");
  }
  return "programsFor 按区域过滤 · 卡片无跨区域分支 · 切换目标 = program.variant";
});

check("★ Trae 每区域恰好一个程序位，展示名与后端真值逐字一致（issue #28 收敛）", () => {
  /* 官方已把 TraeWork 与 TraeCode 合并成**一个**客户端（CN `Trae CN` / 国际 `Trae`），
     老客户端被卸载。本仓因此收敛为**每区域一个程序位** —— 继续渲染三条会让用户点到
     已退役的客户端，得到「未检测到 TraeWork CN」这类死路（issue #28 报障原文）。

     三条判据：
       ① **恰好 2 条**（2 区域 × 1）：兜底表里多一条就是有人把退役程序位塞回来了；
       ② **与后端逐字一致**（跨语言）：兜底表与 `region.rs` 的 `ProgramSpec.display_name`
          必须同值 —— 漂了会让「演示站」与「真机」显示不同的名字，只在截图里看得出来；
       ③ **两个名字互不相同**（`Trae CN` vs `Trae`）：撞名时用户在下拉里分不清。
     Rust 侧 `region::tests::每区域恰好一个程序位且是合并版` 钉的是另一侧（后端真值）。 */
  const fallback = readCode("src/lib/trae-variant-status.ts");
  const labels = [...fallback.matchAll(/programStub\("[a-z_]+",\s*"([^"]+)"/g)].map((m) => m[1]);
  if (labels.length !== 2) {
    throw new Error(`应解析出 2 个程序位展示名（每区域一个），实际 ${labels.length} —— 判据失效（改动程序位表了？）`);
  }
  const dup = labels.find((label, i) => labels.indexOf(label) !== i);
  if (dup) {
    throw new Error(`展示名 \`${dup}\` 被两个程序位共用 —— 用户分不清选的是哪条程序位`);
  }

  const region = readCode("crates/buddy-switch-core/src/modules/trae/region.rs");
  const backend = [
    ...region.matchAll(/const [A-Z_]+: ProgramSpec = ProgramSpec \{[\s\S]*?display_name: "([^"]+)"/g),
  ].map((m) => m[1]);
  if (backend.length !== 2) {
    throw new Error(`应解析出 2 条 ProgramSpec 的 display_name，实际 ${backend.length} —— 判据失效`);
  }
  const sorted = (list) => [...list].sort();
  if (sorted(labels).join("|") !== sorted(backend).join("|")) {
    throw new Error(
      `兜底表与后端展示名不一致：兜底 ${sorted(labels).join(" / ")} · 后端 ${sorted(backend).join(" / ")}`,
    );
  }
  return `${sorted(labels).join(" / ")}（每区域一个 · 与后端一致）`;
});

check("★ Trae 区域名在前端词条 / 后端 display_name / 兜底表三处逐字一致", () => {
  /* 区域名 = **客户端自己的产品名**（`Trae CN` / `Trae`，用户 2026-10-13 定）。
     它被登记在**三处**：后端 `TraeRegion::display_name`（文案真值）、
     前端 `trae.region.*`（Trae 侧单独说区域名）、兜底表两条 `variantLabel`
     （演示站/截图用它）。漂了会让真机与演示站文案不同 —— 只在截图里看得出来。

     ⚠️ 与 `shared.region.version.*`（`国内版` / `国际版`）**不是一回事**，别合并：
     后者是**跨平台**的区域**词**，用于**组合**标签（作用域芯片 = 平台 + 区域词，
     `scope-chip.tsx` ⇒「Trae 国内版」），WorkBuddy 也读它
     ⇒ 改成 Trae 产品名会得到「Trae Trae CN」与「WorkBuddy Trae CN」。

     反例（改坏会红）：改后端 `display_name` 而忘了前端词条（或反之）⇒ 立刻红。 */
  const zh = readCode("src/locales/domains/traeComponents.zh.ts");
  const frontend = Object.fromEntries(
    [...zh.matchAll(/"trae\.region\.(cn|global)":\s*"([^"]+)"/g)].map((m) => [m[1], m[2]]),
  );
  if (!frontend.cn || !frontend.global) {
    throw new Error(`应解析出 trae.region.cn / trae.region.global，实际 ${JSON.stringify(frontend)}`);
  }
  const region = readCode("crates/buddy-switch-core/src/modules/trae/region.rs");
  const fn = /pub fn display_name\(self\) -> &'static str \{[\s\S]*?\n    \}/.exec(region);
  if (!fn) throw new Error("找不到 `TraeRegion::display_name` —— 判据失效（改名了？）");
  const backend = Object.fromEntries(
    [...fn[0].matchAll(/TraeRegion::(Cn|Global) => "([^"]+)"/g)].map((m) => [
      m[1] === "Cn" ? "cn" : "global",
      m[2],
    ]),
  );
  for (const key of ["cn", "global"]) {
    if (frontend[key] !== backend[key]) {
      throw new Error(
        `区域名不一致（${key}）：前端词条 \`${frontend[key]}\` · 后端 display_name \`${backend[key]}\``,
      );
    }
  }
  const status = readCode("src/lib/trae-variant-status.ts");
  for (const key of ["cn", "global"]) {
    if (!status.includes(`t("trae.region.${key}")`)) {
      throw new Error(`兜底表区域 ${key} 没走 \`trae.region.${key}\` —— 演示站会与真机文案不同`);
    }
  }
  return `${frontend.cn} / ${frontend.global}（词条 · display_name · 兜底表 三处一致）`;
});


/* ── P4 收口：后端展示字段必须过 `displayText` 出口（防「白屏」回归）────────────
 *
 * 根因（issue #2：Win11 打开一片白）：`packageName` / `packageCode` / `program.label`
 * 在 `types.ts` / `trae-types.ts` 里声明为 `string | null`，**但后端可能吐对象**
 * （AI 展示名带 emoji 等）。对象被当 **React 子节点**渲染会抛
 * `Objects are not valid as a React child` ⇒ React **卸载整棵树** ⇒ 白屏。
 * `lib/display-text.ts` 的 `displayText` 是唯一的归一出口（`lib/api.ts` 只归一了
 * **账号字段**，没归一 credit 的这两个 ⇒ `tsc` 抓不到，运行时才炸）。
 *
 * 下面 2 条锁住这类「漏过 displayText」的回归。两条都**只锚在代码上**（先剥注释），
 * 且当前零误报 —— 不做「全仓零容忍」的模糊扫描，避免让人习惯性忽略门禁。
 */

check("★ 积分包名回落链必须过 `displayText`（对象当 React 子节点 ⇒ 白屏）", () => {
  /* 复发点：`const name = res.packageName || res.packageCode || t("…")`。
     判据：**同一行同时出现 `packageName` 与 `packageCode` 两个字段、且用 `||` 拼成
     回落链** = 展示值 ⇒ 必须出现 `displayText(`。这两个字段在本仓只用于「积分包名」
     这一处展示，故该判据精确、无误报。
     ⚠️ React `key=` 里的同类链**豁免**：key 由 React 字符串化（`String(key)`）、
     **从不渲染**，不可能白屏；既有约定也不归一 key（`AccountsPage` / `TraeAccountsPage`
     的同类 key 同样保留原字段）—— 不豁免才会误报。 */
  const hits = [];
  const bad = [];
  for (const abs of listSourceFiles()) {
    const rel = path.relative(ROOT, abs);
    for (const line of stripComments(fs.readFileSync(abs, "utf8")).split("\n")) {
      if (!/\.packageName\b/.test(line) || !/\.packageCode\b/.test(line) || !/\|\|/.test(line)) continue;
      if (/\bkey\s*[=:]/.test(line)) continue; // key 不渲染，豁免
      hits.push(rel);
      if (!/displayText\(/.test(line)) bad.push(`${rel} → ${line.trim()}`);
    }
  }
  if (hits.length === 0) {
    throw new Error("找不到任何积分包名回落链 —— 断言失效（`packageName`/`packageCode` 是否改名？）");
  }
  if (bad.length) {
    throw new Error(`积分包名回落链未过 \`displayText\`（对象会白屏）：\n      ${bad.join("\n      ")}`);
  }
  return `${hits.length} 处回落链均过 displayText`;
});

check("★ TraeProgram.label 不得作为裸 React 子节点（必须经 displayText 归一）", () => {
  /* `TraeProgram.label` 来自后端 `TraeProgramStatus.label`，同样可能被吐成对象 ——
     当 **React 子节点**渲染会白屏。归一后的写法是先用
     `displayText(program.label) || program.variant` 落成**局部变量**再渲染该变量，
     因此**渲染位不应再出现裸 `program.label`**。
     判据：JSX 子节点表达式 `>{ … program.label … }`（`>` 后紧跟的 `{…}` 内出现
     `program.label` 且未包 `displayText(`）⇒ 红。
     - `(?<!=)` 排除箭头函数体（`() => { … }`）—— 那不是 JSX 子节点；
     - ⚠️ **只扫引用 `TraeProgram` 的文件**：`program` 这个变量名在别处也用
       （`account-detail-drawer` 的 `DetailProgramBadge`、`trae-api-key-table` 的本地
       `PROGRAMS` 常量）—— 那些 label 是前端常量 / 上游已归一，扫进去就是误报。
       `TraeProgram` 这一类型名才唯一标识「后端 label 的宿主」，故以它界定范围；
     - 属性位（`aria-label=` / `title=`）与中间层对象属性（`label: program.label`，
       见 `programsFor`）不在判据内：前者只产出 `[object Object]` 文本（非崩溃），
       后者是待卡片再归一的结构字段。
     ⚠️ 已知边界：只锚**单行**形态（回归时逐字改回的那一行）；跨行子节点不覆盖。 */
  const re = /(?<!=)>\s*\{[^{}]*\bprogram\.label\b[^{}]*\}/g;
  let seen = 0;
  const offenders = [];
  for (const abs of listSourceFiles()) {
    if (!abs.endsWith(".tsx")) continue;
    const code = stripComments(fs.readFileSync(abs, "utf8"));
    if (!/\bTraeProgram\b/.test(code)) continue; // 只扫「后端 label 的宿主」文件
    seen += (code.match(/\bprogram\.label\b/g) || []).length;
    re.lastIndex = 0;
    let m;
    while ((m = re.exec(code))) {
      if (/displayText\(/.test(m[0])) continue;
      offenders.push(path.relative(ROOT, abs));
    }
  }
  if (seen === 0) {
    throw new Error("找不到任何 `TraeProgram` 宿主的 `program.label` —— 断言失效（字段是否改名？）");
  }
  if (offenders.length) {
    throw new Error(
      `\`program.label\` 被当 React 子节点渲染（对象会白屏）：${[...new Set(offenders)].join(", ")}`,
    );
  }
  return `${seen} 处 program.label 均非裸子节点`;
});

/* ── P4 工具栏（搜索 / 排序 / 分组 / 卡片·表格视图）────────────────────────────
 *
 * 上面 33 条覆盖 P0–P4（账号工作台）。本段锁住 P4 工具栏最易回退的判据：
 * 共用壳接线（且壳内无平台判断）、排序/分组选项**按平台数据**（WB 有 `lastUsedAt`/`groupName`
 * ⇒ 含 `recent`/`group`；Trae 无「最近使用」⇒ 不含 `recent`）、两种空态不混用（被筛没必须有
 * 「清除筛选」出口）、搜索框常驻（IME 约束）、表格行可点区 ≤2 且程序位列只读；外加
 * 「今日 Token」的三条新增判据（诚实边界 / 命令两端接线 / 表格列）。
 *
 * ⚠️ 每条的「可证伪」写法见各自注释 —— T05 变异验证会逐条把它打红（见设计 §4 T05）。
 */

/** 读源码并**剥注释**（锚必须只落在代码上，避免匹配到注释里的反例）。 */
function readCode(rel) {
  return stripComments(fs.readFileSync(path.join(ROOT, rel), "utf8"));
}

const WB_PAGE = "src/pages/AccountsPage.tsx";
const TRAE_PAGE = "src/pages/TraeAccountsPage.tsx";

check("★ P4 工具栏接线：两平台都用共用壳（AccountWorkbench → AccountToolbar + AccountTable），且壳内无平台判断", () => {
  /* ★ 2026-10-10 变更：`<AccountToolbar>` / `<AccountTable>` 的**调用点**从两个页面
     移进了共享容器 `AccountWorkbench`（本次「用原型统一两平台」的收敛动作）。
     断言因此分两层，证伪能力不变：
       ① 两个页面都**必须**用 `AccountWorkbench`（页面不再自己摆工具栏/列表）；
       ② 容器里**必须**同时调用 `AccountToolbar` 与 `AccountTable`（否则页面等于没有工具栏/表格）。
     变异：任一侧页面改回手写工具栏 ⇒ ① 变红；容器里删掉 `<AccountTable>` ⇒ ② 变红。 */
  const WORKBENCH = "src/components/account-workbench.tsx";
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    if (!/<AccountWorkbench\b/.test(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 没有用 \`AccountWorkbench\`（工具栏/列表不该由页面自己摆）`);
    }
  }
  const shell = readCode(WORKBENCH);
  for (const [tag, why] of [
    ["<AccountToolbar\\b", "工具栏"],
    ["<AccountTable\\b", "表格视图"],
  ]) {
    if (!new RegExp(tag).test(shell)) {
      throw new Error(`AccountWorkbench 没有调用 ${why} —— 共用容器必须自己渲染工具栏与表格`);
    }
  }
  /* 共用壳里**禁止**出现 `platform === ...` —— 平台差异一律走 `AccountListConfig`（数据）/
     `columns`（数据）/ `children`（插槽）。出现平台分支 = 合并过度，应退回「两份 + 抽片段」。 */
  for (const rel of [
    "src/components/account-toolbar.tsx",
    "src/components/account-table.tsx",
    WORKBENCH,
  ]) {
    const code = readCode(rel);
    if (/\bplatform\s*===|\bplatform\s*:/.test(code)) {
      throw new Error(`${path.basename(rel)} 出现平台判断 —— 差异应走数据 / 插槽`);
    }
  }
  return "2 页接线 AccountWorkbench · 容器含工具栏+表格 · 3 壳无平台分支";
});

check("★ 两平台工具栏**控件集一致**（Trae 不得再有 WB / 原型都没有的「分组筛选」下拉）", () => {
  /* ★ 2026-10-10 用户要求「需要和 workbuddy 一样的交互」（圈了整个工具栏）。
     实测两个工具栏的**唯一控件集差异** = Trae 独有的「分组**筛选**」下拉
     （`全部账号（N）` / 未分组 / 各分组）—— 它把工具栏挤成两行，且 WB 与原型都没有这个控件。
     ⇒ 删除（连同 `groupFilter` / `groupFiltered` / `UNGROUPED` 三个状态与四条文案）。
     分组信息未丢：卡片上仍有分组标签，`group = "group"` 仍能按分组**分桶**。
     变异：把那个下拉加回来（重新引入 `groupFilter`）⇒ 本条变红。

     ⚠️★ 2026-10-12 二次修订：工具栏**不再有任何平台专属插槽** —— `toolbarExtra` /
     `secondaryActions` 连同「⋯」溢出菜单一起删除（用户要求「合并到添加账号的右侧弹出栏内，
     外面那枚菜单不用存在了」）⇒ 本条改成钉「两页都**没有**插槽 + 两页都用**同一个**抽屉」。
     变异：给某一页加回 `toolbarExtra` ⇒ 本条变红。 */
  const trae = readCode(TRAE_PAGE);
  for (const gone of ["groupFilter", "groupFiltered", "UNGROUPED"]) {
    if (trae.includes(gone)) {
      throw new Error(
        `Trae 页又出现 \`${gone}\` —— 工具栏多出 WorkBuddy / 原型都没有的控件（两平台工具栏必须一致）`,
      );
    }
  }
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    for (const gone of ["toolbarExtra", "secondaryActions"]) {
      if (code.includes(gone)) {
        throw new Error(
          `${path.basename(rel)} 又给工具栏传了 \`${gone}\` —— 工具栏已无平台插槽，入口应进「添加账号」抽屉`,
        );
      }
    }
    /* 两平台必须渲染**同一个**抽屉（同一组件、同一套槽位），差异只走数据。 */
    if (!/<AddAccountDrawer/.test(code)) {
      throw new Error(`${path.basename(rel)} 没有渲染「添加账号」抽屉`);
    }
    for (const slot of ["moreTools={", "labels={{"]) {
      if (!code.includes(slot)) {
        throw new Error(`${path.basename(rel)} 的「添加账号」抽屉缺 \`${slot}\``);
      }
    }
  }
  /* ★ 2026-10-12（用户「回收站放出来」）：常驻控件必须**两页都注入** ——
     只给一页加就会出现「同一个控件在一个平台有、另一个没有」。 */
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    if (!code.includes("onOpenRecycleBin={")) {
      throw new Error(`${path.basename(rel)} 没有注入常驻控件 \`onOpenRecycleBin={\``);
    }
  }
  return "Trae 已无分组筛选下拉 · 两页无平台插槽 · 两页同抽屉 + 同常驻控件";
});

check("★ 账号页工具栏 = 原型一行 + 一枚常驻控件（搜索/排序/分组/视图/回收站/全选/主操作，**无「⋯」**）", () => {
  /* ★ 2026-10-12 用户要求「1:1 还原」原型账号页。原型 `viewAccounts`
     （`buddy-switch-prototype.html:2406-2429`）的工具栏在 1440px 下是**一行 56px**，控件只有：
       搜索(`label.field.search`) · 排序(`label.field` + 「排序」前缀) · 分组(同) ·
       视图 `.seg`(**纯图标**) · `[grow]` · 全选(`btn ghost sm` **文字按钮**) · `+ 添加账号`(`btn primary`)
     ⚠️★ 2026-10-12 二次修订：本仓原先把多出来的 8 个入口收在一枚「⋯」溢出菜单里；
     用户要求「合并到添加账号的右侧弹出栏内，外面那枚菜单不用存在了」⇒ 「⋯」**整块删除**，
     内容改由 `AddAccountDrawer` 承载（见下一条门禁）。本条因此**反向**钉住：
     工具栏里**不得**再有溢出菜单，也**不得**再有平台专属插槽 —— 那是它当初存在的唯一理由。
     变异：把「⋯」加回来 / 给工具栏加回 `children` 插槽 ⇒ 本条变红。 */
  const tb = readCode("src/components/account-toolbar.tsx");
  // ① 搜索框 = 原型 `label.field`：图标与输入在**同一个字段**里（不是「图标绝对定位 + `pl-8`」）。
  if (!/<label[^>]*flex h-8[^>]*>\s*<Search/.test(tb)) {
    throw new Error("搜索框不是原型 `label.field` 结构（图标应在字段内，而非绝对定位）");
  }
  if (!/border-0 bg-transparent p-0/.test(tb)) {
    throw new Error("字段内的 input 不是无边框 / 透明（原型 `.field input` 自身无边框）");
  }
  // ② 排序 / 分组：字段里**先写前缀文字**再跟下拉（原型 `label.field` 是「排序 <select>」）。
  for (const k of ["shared.toolbar.sort", "shared.toolbar.group"]) {
    if (!tb.includes(`t("${k}")`)) throw new Error(`排序 / 分组字段缺少前缀文字「${k}」`);
  }
  // ③ 视图切换 = **纯图标**（原型 `.seg button` 里只有 `ICON.grid` / `ICON.list`）。
  if (/<LayoutGrid className="size-4" \/>\s*\{t\("shared\.toolbar\.viewCard"\)\}/.test(tb)) {
    throw new Error("视图切换按钮又带上了文字 —— 原型 `.seg button` 里只有图标");
  }
  if (!/aria-label=\{t\("shared\.toolbar\.viewCard"\)\}/.test(tb)) {
    throw new Error("视图切换按钮丢了无障碍名（文字拿掉后必须靠 aria-label / title 承担）");
  }
  // ④ 全选 = `btn ghost sm` **文字按钮**，不是「勾选框 + 文字」。
  if (/<Checkbox/.test(tb)) {
    throw new Error("工具栏又用回勾选框 —— 原型是 `btn ghost sm` 文字按钮");
  }
  if (!/aria-pressed=\{selection\.allSelected\}/.test(tb)) {
    throw new Error("全选按钮没有 `aria-pressed`（勾选态现在由它承担）");
  }
  // ⑤ ★ 工具栏里**不得**再有溢出菜单 / ReactNode 平台插槽（入口已搬进「添加账号」抽屉）。
  for (const gone of ["Ellipsis", "DropdownMenu", "shared.toolbar.moreAria", "children", "secondaryActions"]) {
    if (tb.includes(gone)) {
      throw new Error(`工具栏又出现 \`${gone}\` —— 溢出菜单已删除，入口应进「添加账号」抽屉`);
    }
  }
  // ⑥ ★ 一枚**常驻**控件必须存在（用户要求「回收站放出来」），且必须是
  //    **结构化 prop**（不是 ReactNode 插槽 —— 那种槽位正是上一轮删掉的东西）。
  if (!tb.includes("onOpenRecycleBin?:")) {
    throw new Error("工具栏缺常驻控件的结构化 prop `onOpenRecycleBin?:`");
  }
  //    回收站按 prop 有无条件渲染（不传就不画，避免出现点不动的死按钮）。
  if (!/\{onOpenRecycleBin \? \(/.test(tb)) throw new Error("回收站按钮不是按 `onOpenRecycleBin` 条件渲染");
  //    ⚠️ 图标钮必须 **34px**：`size-9`（36px）会成为行内最高子件 ⇒ 工具栏 `58` 而不是原型的 `56`
  //    （实测 36 + 20 padding + 2 border = 58；34 + 20 + 2 = 56）。
  if (tb.includes("size-9")) {
    throw new Error("工具栏里的常驻图标钮用了 `size-9`（36px）—— 会把工具栏顶到 58，原型是 56");
  }
  // ⑦ 顺序 = 「视图 → 回收站 → grow → 全选 → 主操作」（原型右侧只有后两者）。
  const iGrow = tb.indexOf('className="grow"');
  const iAll = tb.indexOf("selection.allSelected");
  const iAdd = tb.indexOf("addAction ? (");
  const iRecycle = tb.indexOf("{onOpenRecycleBin ?");
  if (!(iRecycle > 0 && iRecycle < iGrow && iGrow < iAll && iAll < iAdd)) {
    throw new Error("工具栏顺序不是「回收站 → grow → 全选 → 主操作」");
  }
  // ⑧ 主操作 = 原型 `.btn`（内边距 13px；`Button` 对带图标者默认收到 11px，必须补回来）。
  if (!/has-\[>svg\]:px-\[13px\]/.test(tb)) {
    throw new Error("主操作内边距不是原型 `.btn` 的 13px");
  }
  return "搜索=label.field · 排序/分组带前缀 · 视图纯图标 · 回收站常驻 · 全选=ghost 按钮 · 无溢出菜单 · 主操作 13px";
});

check("★「添加账号」抽屉 = 原工具栏「⋯」内容的唯一落点（已有的不重复搬 · 放出来的不再留）", () => {
  /* ★ 2026-10-12 用户要求「合并到添加账号的右侧弹出栏内…外面的菜单不用存在了，
     workbuddy 和 traework 的交互保持一致」。原「⋯」菜单里的入口现在必须**全部有落点**，
     且**已经存在的功能不重复搬**：
       - 从本机导入 / 导入备份 → 抽屉**基础三张卡**里本来就有（不重复）；
       - 自动签到 / 第二枚开关 / 导出备份 / 刷新 → `moreTools` **四行**；
       - 回收站 → **已被用户点名「放出来」**（回工具栏的 `onOpenRecycleBin`）
         ⇒ 抽屉里**不得**再留一份（同一个开关出现在两处必然打架）。
     两平台的 `moreTools` **行数与顺序一致**（先两枚开关，再两枚动作），只有数据不同；
     文案走 `shared.addDrawer.*`（同一个组件 ⇒ 不许再写回平台自己的域，否则两边会漂移）。
     变异：删掉某一页的 `moreTools`（入口变死路）/ 改键序 / 把文案写回平台域 ⇒ 本条变红。 */
  const WB_TOOL_KEYS = ["auto-checkin", "auto-travel", "export", "refresh"];
  const TRAE_TOOL_KEYS = ["auto-checkin", "skip-checked", "export", "checkin-refresh"];
  const drawer = readCode("src/components/add-account-drawer.tsx");
  if (!/\{moreTools\?\.length \?/.test(drawer)) throw new Error("抽屉没有渲染 moreTools 分区");
  if (!/\{moreTools\.map\(/.test(drawer)) throw new Error("抽屉没有把 moreTools 逐行渲染出来");
  if (!/\{extraOptions\?\.map\(/.test(drawer)) throw new Error("抽屉没有渲染平台专属的 extraOptions");
  for (const [rel, keys] of [[WB_PAGE, WB_TOOL_KEYS], [TRAE_PAGE, TRAE_TOOL_KEYS]]) {
    const code = readCode(rel);
    const i = code.indexOf("moreTools={");
    if (i < 0) throw new Error(`${path.basename(rel)} 没给抽屉传 \`moreTools\``);
    const seg = code.slice(i, code.indexOf("labels={{", i));
    const found = [...seg.matchAll(/key:\s*"([^"]+)"/g)].map((m) => m[1]);
    if (found.join(",") !== keys.join(",")) {
      throw new Error(
        `${path.basename(rel)} 的 moreTools 键序列为 [${found.join(",")}]，应为 [${keys.join(",")}]`,
      );
    }
    if (!/moreTools:\s*t\("shared\.addDrawer\.moreTools"\)/.test(code)) {
      throw new Error(`${path.basename(rel)} 的抽屉没有「更多操作」分区标题`);
    }
    if (/t\("(wbAccounts|trae)\.(addDrawer|page\.accounts\.addDrawer)/.test(code)) {
      throw new Error(`${path.basename(rel)} 的抽屉文案又写回平台自己的域 —— 两平台必须逐字一致（shared.addDrawer.*）`);
    }
    /* ★ 抽屉里**不得**再出现「回收站」—— 它已放回工具栏，两处都放会打架。 */
    if (/shared\.addDrawer\.compactMode|key:\s*"recycle"/.test(code)) {
      throw new Error(`${path.basename(rel)} 的抽屉里仍有「回收站」—— 它已放回工具栏`);
    }
  }
  return "两页抽屉各 4 行 · 键序一致 · 分区有标题 · 文案同源 · 无「已放出来」的重复项";
});

check("★ Qoder 账号页 = 原型工作台同构（工具栏一行 + 卡开抽屉 + 备注在抽屉 · 第三平台不另起炉灶）", () => {
  /* 原型的账号页是**平台无关**的（`viewAccounts` / `accountCard` / `drawerAccount`：
     工具栏一行 + 卡点击开详情 + 备注在抽屉里编辑），三个平台的差异只在数据。
     Qoder 必须复用共享壳，而不是再写一版自建卡片。
     变异：把 Qoder 页退回「自建卡片 / 无工具栏 / 备注内联在卡上」⇒ 本条变红。 */
  const page = readCode("src/pages/QoderAccountsPage.tsx");
  // ① 复用共享工作台，且原型 `viewAccounts` :2406-2429 的控件全部接线
  const wi = page.indexOf("<AccountWorkbench");
  if (wi < 0) throw new Error("Qoder 账号页没有复用 AccountWorkbench");
  for (const anchor of [
    "query={",
    "sortOptions={",
    "groupOptions={",
    "onViewChange={",
    "addAction={{",
    "selection={{",
    "buckets={",
    "columns={",
    "renderCard={",
    "onOpenRow={",
    "onToggleSelect={",
  ]) {
    if (!page.slice(wi).includes(anchor)) {
      throw new Error(`Qoder 工作台缺 \`${anchor}\`（原型工具栏 / 列表不全）`);
    }
  }
  // ② 卡点击 → 共享详情抽屉；备注编辑在抽屉里接线（卡上不得再出现就地编辑）
  if (!/AccountDetailDrawer/.test(page)) throw new Error("Qoder 没有共用详情抽屉");
  if (!/onSaveRemark=/.test(page) || !/autoEditRemark=/.test(page)) {
    throw new Error("抽屉没有备注编辑接线（原型：编辑备注 → 抽屉内就地编辑）");
  }
  const card = readCode("src/components/qoder/qoder-account-card.tsx");
  if (/RemarkField|<Input/.test(card)) {
    throw new Error("Qoder 卡又出现就地备注编辑 —— 备注归抽屉（原型：「点账号卡打开抽屉即可操作」）");
  }
  if (!/onOpenDetail/.test(card)) throw new Error("Qoder 卡没有整卡点击开详情");
  // ③ 添加抽屉 = 原型方式卡机制下的**能力子集**。
  //   ★ 2026-10-15：OAuth 设备授权与导入备份已落地（后端 + 命令 + 弹层全在）⇒ 基础三张卡
  //   **全部**渲染。这条断言因此从「只许 local」反转为「三条都要在」—— 它编码的是
  //   「渲染的方式必须真的能用」，实现补齐后必须同步，否则护栏会反过来锁住功能。
  if (!/ways=\{\["oauth", "local", "backup"\]\}/.test(page)) {
    throw new Error("Qoder 添加抽屉没有渲染全部三种基础方式（OAuth / 本机导入 / 导入备份均已落地）");
  }
  if (!/paste-pat/.test(page)) throw new Error("Qoder 添加抽屉缺「粘贴 PAT」的 extraOption");
  if (!/QoderOAuthDialog/.test(page)) throw new Error("Qoder 添加抽屉的 OAuth 卡没有接线到弹层（假控件）");
  // ④ 批量条出**已实现**的动作：删除 + 签到 + 刷新 + 导出（后端命令齐备）。
  for (const anchor of ["onDelete:", "onCheckin:", "onRefresh:", "onExport:"]) {
    if (!page.includes(anchor)) {
      throw new Error(`Qoder 批量条缺 \`${anchor}\` 接线（对应后端命令已存在）`);
    }
  }
  // ⑤ 删除是**软删除**：单删 + 批删都必须走确认弹层；且文案必须如实说「可恢复」，
  //   并给回收站入口（有恢复路径却没入口 = 假承诺）。
  if (!/pendingDelete/.test(page) || !/batchDeleteOpen/.test(page)) {
    throw new Error("Qoder 删除没有确认弹层（软删除也必须先确认）");
  }
  if (!/RecycleBinSheet/.test(page) || !/onOpenRecycleBin=/.test(page)) {
    throw new Error("Qoder 有回收站却没有入口（onOpenRecycleBin / RecycleBinSheet 缺一）");
  }
  const zh = stripComments(fs.readFileSync(path.join(ROOT, "src/locales/domains/qoderPages.zh.ts"), "utf8"));
  if (/永久删除，没有回收站/.test(zh)) {
    throw new Error("Qoder 删除文案仍写「永久删除，没有回收站」—— 已是软删除，文案在谎报");
  }
  if (!/回收站/.test(zh)) {
    throw new Error("Qoder 删除文案没有提到回收站（软删除必须告知可恢复）");
  }
  return "工作台同构 · 备注在抽屉 · 添加方式三张全在 · 批量四动作 · 软删除 + 回收站入口";
});

check("★ Qoder 程序位 id 必须与**凭据来源 id 同源**（一事实一 id）", () => {
  /* 真机实测踩到的缺陷（2026-10-15）：`switch.rs` 里 `QoderProgram::Desktop.as_str()`
     返回 `"desktop"`，而 `profile.rs` 的 `KIND_DESKTOP` 是 `"desktop-app"`。
     两套 id 让 `program_status_for` 按 `"desktop"` 去查来源表**永远落空** ——
     真机表现是「桌面版明明登着，界面却显示未检测到」。单测测不到：两边各自都自洽。

     ⇒ 这条护栏扫的是**是否引用了同一个常量**，而不是「值对不对」：
     写死字符串字面量就红（哪怕这次值恰好对），因为下次改一处就会再次分叉。

     变异：把 `as_str` 的 `super::profile::KIND_DESKTOP` 换回 `"desktop-app"` 字面量 ⇒ 本条变红。 */
  const sw = readCode("crates/buddy-switch-core/src/modules/qoder/switch.rs");
  const prof = readCode("crates/buddy-switch-core/src/modules/qoder/profile.rs");
  const m = /pub fn as_str\(self\) -> &'static str \{([\s\S]*?)\n    \}/.exec(sw);
  if (!m) throw new Error("找不到 `QoderProgram::as_str`");
  if (!/profile::KIND_DESKTOP/.test(m[1]) || !/profile::KIND_CLI/.test(m[1])) {
    throw new Error("`QoderProgram::as_str` 没有引用 `profile::KIND_*` —— 又出现了两套 id");
  }
  if (/=>\s*"desktop"|=>\s*"cli"/.test(m[1])) {
    throw new Error("`QoderProgram::as_str` 里写死了字面量 —— 必须与 `profile::KIND_*` 同源");
  }
  if (!/pub const KIND_DESKTOP/.test(prof) || !/pub const KIND_CLI/.test(prof)) {
    throw new Error("`profile::KIND_DESKTOP` / `KIND_CLI` 不再是 pub —— 同源关系断了");
  }
  return "程序位 id = 凭据来源 id（引用同一常量）· 无字面量分叉";
});

check("★ Qoder 签到结果**三档**：「通道未开放」既不算成功、也不进重试桶（issue #46 的 D 项）", () => {
  const page = stripComments(fs.readFileSync(path.join(ROOT, "src/pages/QoderAccountsPage.tsx"), "utf8"));
  if (!/else if \(res\.disabled\)/.test(page)) {
    throw new Error("`runCheckin` 没有把 `disabled` 单独成档 —— 未开放会被混进成功或失败");
  }
  if (!/res\.disabled\) disabled\.push\(item\)/.test(page)) {
    throw new Error("`disabled` 必须进第三个桶（`disabled.push`），不能进 `failed`");
  }
  if (!/skippedNames: checkinResult\.disabled\.map/.test(page)) {
    throw new Error("第三档没有透传给结果条（`skippedNames`）");
  }
  /* 后端那一侧：`Disabled` 必须回 `ok:false`（上游 409 不携带幂等语义）。 */
  const core = readCode("crates/buddy-switch-core/src/modules/qoder/checkin.rs");
  if (!/fn disabled_claim_response/.test(core)) {
    throw new Error("core 没有 `disabled_claim_response`（未开放那一档的纯函数）");
  }
  const response = core.match(/pub fn disabled_claim_response[\s\S]*?\n}/);
  if (!response || !/"ok":\s*false/.test(response[0])) {
    throw new Error("`disabled_claim_response` 必须回 `ok:false` —— 回 `ok:true` 就是谎报成功");
  }
  /* 反向也钉住：「今日已签」是**真成功**，不能跟着一起被改成 false。 */
  const already = core.match(/pub fn already_claim_response[\s\S]*?\n}/);
  if (!already || !/"ok":\s*true/.test(already[0])) {
    throw new Error("`already_claim_response` 必须回 `ok:true` —— 「今日已签」是真成功");
  }
  return "三档分流 · 未开放回 ok:false · 已签回 ok:true · 透传 skippedNames";
});

check("★ 签到结果条：不传 `skippedNames` ⇒ 渲染与加这个 prop 之前**逐字一致**（WB / Trae 复用方零影响）", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  /* `useT` 直接回 key（不查词表）⇒ 断言只认「走了哪一档」，不依赖任何文案。 */
  const stubs = {
    react: React,
    "lucide-react": new Proxy({}, { get: (_t, name) => stubComponent(String(name)) }),
    "@/components/account-card": {
      AVATAR_TEXT_CLASS: "",
      accountInitials: (name) => String(name).slice(0, 1),
      avatarTone: () => "",
    },
    "@/components/ui/badge": { Badge: stubComponent("Badge") },
    "@/components/ui/button": { Button: stubComponent("Button") },
    "@/components/ui/skeleton": { Skeleton: stubComponent("Skeleton") },
    "@/lib/i18n": { useT: () => (key, vars) => (vars ? `${key}:${JSON.stringify(vars)}` : key) },
    "@/lib/use-refresh-action": { useRefreshAction: () => ({ busy: false, run: (fn) => fn() }) },
    "@/lib/utils": { cn: (...parts) => parts.filter(Boolean).join(" ") },
  };
  const { CheckinResultBar } = loadUsageComponent("src/components/account-list-parts.tsx", stubs);
  const base = { okCount: 2, failedNames: ["账号A"], onRetry: () => {}, onDismiss: () => {} };

  // ① 旧用法（不传第三档）：仍是「成功/失败」两档，且**不出现**第三档词条。
  const legacyFail = renderToStaticMarkup(React.createElement(CheckinResultBar, base));
  const legacyOk = renderToStaticMarkup(
    React.createElement(CheckinResultBar, { ...base, failedNames: [] }),
  );
  if (legacyFail.includes("shared.checkinResult.summary4")) {
    throw new Error("不传 skippedNames 却走了三档词条 —— WB / Trae 的界面会被改写");
  }
  if (legacyFail.includes("shared.checkinResult.skippedNames")) {
    throw new Error("不传 skippedNames 却渲染了「通道未开放」说明");
  }
  if (legacyOk.includes("shared.checkinResult.summary4")) {
    throw new Error("全成功时不该用三档词条");
  }
  // ② 传第三档：走 `summary4` + `skippedNames`，且**没有**重试出口（重试永远无效）。
  const threeTier = renderToStaticMarkup(
    React.createElement(CheckinResultBar, {
      ...base,
      failedNames: [],
      skippedNames: ["账号B"],
      onRetry: undefined,
    }),
  );
  if (!threeTier.includes("shared.checkinResult.summary4")) {
    throw new Error("传了 skippedNames 却没用三档汇总词条");
  }
  if (!threeTier.includes("shared.checkinResult.skippedNames")) {
    throw new Error("传了 skippedNames 却没有「未开放」账号说明");
  }
  if (threeTier.includes("shared.checkinResult.retryFailed")) {
    throw new Error("「未开放」不该给出「重试失败项」出口（重试永远不会成功）");
  }
  return "两档路径逐字不变 · 三档路径用 summary4/skippedNames · 未开放不给重试出口";
});

check("★ Qoder 删除端点必须走**软删除**（core 有对的函数 ≠ 命令面接对了）", () => {
  /* 真机实测踩到的缺陷（2026-10-15）：`qoder/trash.rs` 写得好好的、单测全绿，
     但两个宿主的 `/accounts/delete` handler 仍调 `account::remove_account_for`（硬删）
     ⇒ 删除返回 `{ok:true}`、账号离开 live、**回收站里一条都没有** ⇒ 账号凭空消失、不可恢复。
     单测测的是 `trash_account_for` 本体，**测不到命令面接错了哪个函数** —— 所以这条护栏
     扫的是**调用点**，不是实现。

     变异：把任一宿主的删除 handler 改回硬删函数 ⇒ 本条变红。 */
  const server = readCode("crates/buddy-switch-server/src/api.rs");
  const commands = readCode("src-tauri/src/commands.rs");
  const accountRs = readCode("crates/buddy-switch-core/src/modules/qoder/account.rs");
  for (const [name, source] of [["server/api.rs", server], ["commands.rs", commands]]) {
    if (!/qoder::trash::trash_account_for/.test(source)) {
      throw new Error(`${name} 的 Qoder 删除没有走 \`qoder::trash::trash_account_for\`（软删除）`);
    }
    if (/qoder::account::remove_account_for/.test(source)) {
      throw new Error(`${name} 仍在调 Qoder 硬删除（回收站会静默丢数据）`);
    }
  }
  // 硬删函数必须**不存在**（留着它就留了一条「再次接错」的路）。
  if (/fn remove_account_for/.test(accountRs)) {
    throw new Error("qoder/account.rs 又出现了硬删除函数 —— 软删除是唯一删除路径");
  }
  return "两宿主删除端点 = trash_account_for · 硬删函数已删 · 唯一不可逆步 = purge";
});

check("★ 卡片栅格不得用 `items-start`（同行卡片必须等高，原型 `.grid` 默认 stretch）", () => {
  /* 原型 `.grid { display:grid; gap:12px }`（`buddy-switch-prototype.html:242`）**没有**
     `align-items` ⇒ 默认 `stretch` ⇒ 同一行卡片**等高**，页脚自然落在底部对齐。
     实测加 `items-start` 时同行卡片高度 406/381/361（差 45px），用户直接看出「卡片有些大有些小」。
     变异：给栅格加回 `items-start` ⇒ 本条变红。 */
  const shell = readCode("src/components/account-workbench.tsx");
  const grid = /className="grid[^"]*"/.exec(shell);
  if (!grid) throw new Error("AccountWorkbench 找不到卡片栅格");
  if (/\bitems-start\b/.test(grid[0])) {
    throw new Error(`卡片栅格带了 \`items-start\`（${grid[0]}）—— 同行卡片会不等高（原型默认 stretch）`);
  }
  if (!/min-\[1101px\]:grid-cols-3/.test(grid[0])) {
    throw new Error("卡片栅格不是原型 `.g3` 的三列断点（min-[1101px]:grid-cols-3）");
  }
  return "栅格 stretch（同行等高）· 三列断点与原型一致";
});

check("★ 徽章图标必须用**后代**选择器 `[&_svg]`（用 `[&>svg]` 会失配并堆叠成两行）", () => {
  /* `Badge` 为了「长文本出省略号」把 children 包进了内层 `truncate` span
     ⇒ 图标**不再**是 `Badge` 的直接子级。若用 `[&>svg]:size-[11px]`：
       ① 选择器失配 ⇒ 图标回到 lucide 默认 16px；
       ② Tailwind preflight 给 `svg` 设了 `display:block`，内层 span 又是 block
          ⇒ 图标与文字**上下堆叠成两行**（实测内层 33px），塞进 `h-5`（内容区 18px）
          被 `overflow-hidden` **硬裁 13px** —— 用户看到「徽章里的图标/文字被切掉」。
     ⇒ 必须同时有 `[&_svg]:size-…` 与 `[&_svg]:inline`（抵消 preflight 的 block）。
     变异：把 `[&_svg]` 改回 `[&>svg]` ⇒ 本条变红（实测 scrollHeight 25 > clientHeight 18）。 */
  const badge = readCode("src/components/ui/badge.tsx");
  if (/\[&>svg\]/.test(badge)) {
    throw new Error("Badge 用了 `[&>svg]` —— 图标在内层 truncate span 里，选择器会失配（图标变 16px 且堆叠被裁）");
  }
  if (!/\[&_svg\]:size-\[11px\]/.test(badge)) {
    throw new Error("Badge 缺少 `[&_svg]:size-[11px]` —— 原型 `.badge` 的图标是 11px");
  }
  if (!/\[&_svg\]:inline/.test(badge)) {
    throw new Error("Badge 缺少 `[&_svg]:inline` —— preflight 的 `svg{display:block}` 会让图标与文字堆叠成两行");
  }
  return "后代选择器 + 11px + inline（不堆叠、不被裁）";
});

check("★ Trae「当前登录」必须**逐程序位**判定（用区域去查 logins 会永远查不到）", () => {
  /* ★ 2026-10-10 修的真实缺陷：`logins` 的键是**程序位标识**
     （`trae_work` / `trae_cn` / `global` / `global_trae_code`），而 `variant` 是**区域**
     （`cn` / `global`）。原实现写 `logins[variant]` ⇒ 只有「区域名恰好等于某个程序位标识」
     时才能查到 —— 国际版有个程序位就叫 `global`，那边**碰巧**正常；
     **国内版两个程序位叫 `trae_work` / `trae_cn`，永远查不到**
     ⇒ 国内版作用域下没有任何一张卡会亮「当前登录」（实测复现）。
     正解：`programsFor(account).some(p => p.current)`，与 WorkBuddy 卡片同一口径。
     变异：把判定改回 `logins[variant]` ⇒ 本条变红。 */
  const code = readCode(TRAE_PAGE);
  if (/logins\[\s*variant\s*\]/.test(code)) {
    throw new Error(
      "Trae 页用 `logins[variant]`（区域）查登录态 —— 键是程序位标识，国内版永远查不到「当前登录」",
    );
  }
  if (!/function isCurrentAccount\(/.test(code)) {
    throw new Error("缺少 `isCurrentAccount` —— 卡片 / 表格 / 抽屉的「当前登录」必须共用同一判定");
  }
  const uses = (code.match(/isCurrentAccount\(/g) || []).length;
  if (uses < 4) {
    throw new Error(`\`isCurrentAccount\` 只被用了 ${uses} 次 —— 卡片 / 表格行 / 抽屉 / 定义处应≥4`);
  }
  return "逐程序位判定 · 卡片/表格/抽屉共用";
});

check("★ P4 排序选项按平台数据：WB 有 recent（有 lastUsedAt），Trae 无 recent（无「最近使用」字段）", () => {
  /* 原规则（决策 #3）：某排序键在当前平台无数据支撑就不渲染该选项。
     ★ 2026-10-10 变更：后端为 `AccountMeta` 记录了 `lastUsedAt`（epoch 毫秒）—— 原判据
     「WB 无该字段」已不成立 ⇒ 「最近使用」不再是假选项，WB 的 availableSorts **必须**含 recent。
     Trae 的 `TraeAccount` 仍无「最近使用」口径（`trae-types.ts` 的 `lastUsedAt` 属
     `TraeApiKeyRecord`，不是账号字段）⇒ 仍然**不得**出现该选项。
     变异：删掉 WB 的 recent（或给 Trae 塞 recent）⇒ 本条变红。 */
  const wb = readCode(WB_PAGE).match(/availableSorts:\s*\[[^\]]*\]/);
  const trae = readCode(TRAE_PAGE).match(/availableSorts:\s*\[[^\]]*\]/);
  if (!wb || !trae) throw new Error("找不到 availableSorts");
  if (!/["']recent["']/.test(wb[0])) {
    throw new Error("WB 的 availableSorts 缺 recent —— 后端已记录 lastUsedAt，该选项有数据支撑");
  }
  if (/["']recent["']/.test(trae[0])) {
    throw new Error("Trae 的 availableSorts 含 recent —— Trae 账号无「最近使用」字段");
  }
  return `WB ${wb[0].replace(/availableSorts:\s*/, "= ")} · Trae ${trae[0].replace(/availableSorts:\s*/, "= ")}`;
});

check("★ P4 分组选项按平台数据：两页都有 group（WB 有 groupName，Trae 有 groupId）", () => {
  /* 原规则：WB 的 `AccountMeta` 无分组字段 ⇒ availableGroups 不含 group。
     ★ 2026-10-10 变更：后端为 `AccountMeta` 记录了 `groupName`（自由文本，缺省 = 未分组）
     ⇒ WB 也可「按自定义组」分组，故 availableGroups **必须**含 group。Trae 原本就有 groupId。
     变异：删掉任一侧的 group ⇒ 本条变红。 */
  const wb = readCode(WB_PAGE).match(/availableGroups:\s*\[[^\]]*\]/);
  const trae = readCode(TRAE_PAGE).match(/availableGroups:\s*\[[^\]]*\]/);
  if (!wb || !trae) throw new Error("找不到 availableGroups");
  if (!/["']group["']/.test(wb[0])) {
    throw new Error("WB 的 availableGroups 缺 group —— 后端已记录 groupName（§1.5 更新）");
  }
  if (!/["']group["']/.test(trae[0])) {
    throw new Error("Trae 的 availableGroups 缺 group —— Trae 有 groupId（§1.6）");
  }
  return `WB ${wb[0].replace(/availableGroups:\s*/, "= ")} · Trae ${trae[0].replace(/availableGroups:\s*/, "= ")}`;
});

check("★ 「今日 Token」诚实边界：卡片指标行三项齐备，—/≥ 由唯一实现承担", () => {
  /* 参见 `AccountTokenTodayResponse` 的三条红线：缺席 ⇒ 「—」（**非 0**）；被裁剪 ⇒ 下界 `≥N`；
     真实 0 照常渲染。卡片与表格**必须**共用同一份实现，否则两处口径会漂移。
     变异：删掉任一项 / 去掉 `≥` 或「—」分支 / 换掉共享 `formatDate` ⇒ 本条变红。 */
  const card = readCode("src/components/account-card.tsx");
  for (const key of [
    "wbAccounts.card.metric.credits",
    "wbAccounts.card.metric.todayTokens",
    "wbAccounts.card.metric.lastUsedTip",
  ]) {
    if (!card.includes(key)) throw new Error(`账号卡片指标行缺少「${key}」`);
  }
  if (!/export function formatTodayTokens\(/.test(card)) {
    throw new Error("缺少 `formatTodayTokens` —— 卡片/表格的诚实口径必须共用一份实现");
  }
  const body = fnBody(card, "formatTodayTokens");
  if (!/"—"/.test(body)) throw new Error("`formatTodayTokens` 未把「缺席」渲染成「—」（会误显示成 0）");
  if (!/metric\.atLeast/.test(body)) throw new Error("`formatTodayTokens` 未把「网关被裁剪」渲染成下界「≥」");
  // ★ 双桶重叠披露（2026-10-15）：客户端与网关是**两条独立记录**，某个被统计的客户端
  // 被指向本网关时，同一请求会两处各记一次 ⇒ 合计**可能重复计数**。后端无 join 键、
  // 不做启发式去重，披露只能落在明细里：两侧都非零时必须带上说明。
  // 变异：删掉 `todayTokensOverlap` 分支（或换成不渲染）⇒ 本条变红。
  const breakdown = fnBody(card, "todayTokensBreakdown");
  if (!/metric\.todayTokensOverlap/.test(breakdown)) {
    throw new Error("`todayTokensBreakdown` 未披露双桶重叠 —— 两侧非零时同一请求可能被重复计数，只给数字就是误导");
  }
  if (!/export function todayTokensBreakdown\(/.test(card)) {
    throw new Error("缺少 `todayTokensBreakdown` —— 明细（客户端 / 网关分开）必须与 headline 同源");
  }
  if (!/account\.lastUsedAt/.test(card)) throw new Error("账号卡片未读取 `AccountMeta.lastUsedAt`");
  /* 第三格：原型是**相对时间**（`2 分钟前`），且复用共享 `formatRelativeTime`
     （`lib/catalog-source.ts`）—— 不许在本组件里另写一份相对时间格式化。 */
  if (!/formatRelativeTime\(lastUsedAt, t\)/.test(card)) {
    throw new Error("「最后使用」未经共享 `formatRelativeTime` 渲染（另写一份会与网关口径漂移）");
  }
  return "三项齐备 · 缺席「—」/ 裁剪「≥」由 formatTodayTokens 统一承担 · 双桶重叠由 todayTokensBreakdown 披露 · lastUsedAt 走 formatRelativeTime";
});

check("★ 「今日 Token」命令两端接线：路由 + 调用 + 命令名 + 结果类型", () => {
  /* 新增的只读命令必须在**四处**对齐（否则 webui / 桌面端任一端会静默失效）：
     ROUTES 路由、`call()` 真实调用点、导出的调用函数名、返回类型定义。
     变异：删任一处（或改命令名）⇒ 本条变红。 */
  const api = readCode("src/lib/api.ts");
  if (!/get_account_token_today:\s*\{\s*method:\s*"GET"/.test(api)) {
    throw new Error("ROUTES 缺 `get_account_token_today`（webui 路由）");
  }
  if (!/call<AccountTokenTodayResponse>\(\s*"get_account_token_today"/.test(api)) {
    throw new Error('未用 `call<AccountTokenTodayResponse>("get_account_token_today")` 发起调用');
  }
  if (!/export function getAccountTokenToday\(/.test(api)) {
    throw new Error("缺少导出的 `getAccountTokenToday()` 调用函数");
  }
  if (!/interface AccountTokenTodayResponse/.test(readCode("src/lib/types.ts"))) {
    throw new Error("types.ts 缺 `AccountTokenTodayResponse` 结果类型");
  }
  return "route + call + 命令名 + 结果类型 四端齐备";
});

check("★ 「今日 Token」表格列：WB 账号表新增该列，且复用同一诚实实现", () => {
  /* 表格壳 `account-table.tsx` 是**通用壳**，列由页面以 `columns`（数据）注入
     ⇒ 新列必须落在 `AccountsPage.tsx`，且复用卡片的 `formatTodayTokens`。
     变异：删列 / 删列头文案 / 不复用共享实现 ⇒ 本条变红。 */
  const page = readCode(WB_PAGE);
  if (!/key:\s*"todayTokens"/.test(page)) {
    throw new Error("WB 账号表未新增「今日 Token」列（`key: \"todayTokens\"`）");
  }
  if (!page.includes("wbAccounts.table.todayTokens")) {
    throw new Error("「今日 Token」列头文案缺失（`wbAccounts.table.todayTokens`）");
  }
  if (!/formatTodayTokens\(entry,/.test(page)) {
    throw new Error("表格列未复用 `formatTodayTokens` —— 诚实口径会与卡片漂移");
  }
  return "列新增 · 复用 formatTodayTokens · 壳保持通用（列由页面注入）";
});

check("★ P4 两种空态不混用：被筛没必须给「清除筛选」出口（删出口即变红）", () => {
  /* §1.9：真没有数据 → 「还没有 X」+ 添加出口；被筛没 → 「没有符合条件的 X」+ **清除筛选出口**。
     变异②：删掉「清除筛选」出口（按钮 / 文案 / 复位函数任一）⇒ 本条变红。 */
  /* ★ 2026-10-10 变更：分桶与两种空态的**判据**已收进 `AccountWorkbench`
     （页面只传 `totalCount` / `visibleCount` / 两份文案）。断言因此分两层：
       ① 容器里「总数 0」与「可见 0」是**两个独立分支**（混用即变红）；
       ② 每个页面都**必须**为「被筛没」提供带「清除筛选」出口的文案 + 复位函数。
     变异：把容器里两个分支合成一个 ⇒ ① 变红；删掉任一侧的 `clearListFilters` / 文案 ⇒ ② 变红。 */
  const WORKBENCH = "src/components/account-workbench.tsx";
  const shell = readCode(WORKBENCH);
  if (!/totalCount === 0/.test(shell)) {
    throw new Error("AccountWorkbench 缺少「真没有」判据（totalCount === 0）");
  }
  if (!/visibleCount === 0/.test(shell)) {
    throw new Error("AccountWorkbench 缺少「被筛没」判据（visibleCount === 0）");
  }
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    if (!/function clearListFilters\(/.test(code)) {
      throw new Error(`${path.basename(rel)} 缺少 \`clearListFilters\` 复位函数`);
    }
    if (!/shared\.list\.empty\.clearFilters/.test(code)) {
      throw new Error(`${path.basename(rel)} 的「清除筛选」按钮文案缺失 —— 空态被丢在原地`);
    }
    if (!/emptyFiltered=\{/.test(code)) {
      throw new Error(`${path.basename(rel)} 没有把「被筛没」文案交给容器（emptyFiltered）`);
    }
  }
  // Trae 的「真没有账号」由外层 `EmptyTraeCard` 承担（与「被筛没」是两套不同文案）。
  if (!/<EmptyTraeCard/.test(readCode(TRAE_PAGE))) {
    throw new Error("Trae 页缺少「真没有账号」空态（EmptyTraeCard）—— 两种空态会混用");
  }
  return "容器内「真没有 / 被筛没」两分支 · 两页均含清除筛选出口";
});

check("★ P4 搜索框常驻（IME 约束）：工具栏不被「结果为空」换掉，<Input> 无条件渲染", () => {
  /* §1.4：受控 <Input> 不得因 `query` 变化 / 结果为空而 unmount，否则中文输入法连续组词丢字。
     变异①：把工具栏（含搜索框）挪进「被筛没」分支（`sorted.length === 0 ? … : <AccountToolbar/>`）
     ⇒ 工具栏出现在 `sorted.length === 0` **之后** ⇒ 本条变红。 */
  /* ★ 2026-10-10 变更：工具栏的**调用点**已从两个页面移进 `AccountWorkbench`
     ⇒ 断言改在容器里做（判据不变：工具栏必须排在「可见数为 0」的分支**之前**）。
     变异：把 `<AccountToolbar>` 挪进 `visibleCount === 0` 分支 ⇒ 本条变红。 */
  {
    const shell = readCode("src/components/account-workbench.tsx");
    const iToolbar = shell.indexOf("<AccountToolbar");
    const iEmpty = shell.indexOf("visibleCount === 0");
    if (iToolbar < 0) throw new Error("AccountWorkbench 找不到 <AccountToolbar>");
    if (iEmpty < 0) throw new Error("AccountWorkbench 找不到 visibleCount === 0");
    if (iToolbar > iEmpty) {
      throw new Error(
        "AccountWorkbench 的工具栏出现在「被筛没」分支之后 —— 结果为空时搜索框会被 unmount（IME 丢字）",
      );
    }
  }
  // 工具栏壳内：`return (` 到 `<Input` 之间不得有条件分支（`?` / `&&` / `.map(`）—— 否则会被条件性 unmount。
  const code = readCode("src/components/account-toolbar.tsx");
  const ret = code.slice(code.indexOf("return ("));
  const inputIdx = ret.indexOf("<Input");
  if (inputIdx < 0) throw new Error("工具栏壳里找不到 <Input>");
  if (/\?|&&|\.map\(/.test(ret.slice(0, inputIdx))) {
    throw new Error("搜索框 <Input> 处在条件分支里 —— 会被条件性 unmount（IME 丢字）");
  }
  return "工具栏在列表之外 · <Input> 无条件渲染";
});

check("★ P4 表格行可点区 ≤2 且程序位列只读（给程序位列加 onClick 即变红）", () => {
  /* §1.8 / §1.10：每行只有 2 个可点区 —— 勾选框（1）+ 行本身（1）；程序位列是**只读徽标**。
     变异③：给页里「程序位列」的 render 加 `onClick` ⇒ 列定义出现 onClick ⇒ 本条变红。 */
  const shell = readCode("src/components/account-table.tsx");
  const shellClicks = (shell.match(/onClick=/g) || []).length;
  if (shellClicks !== 2) {
    throw new Error(`表格壳有 ${shellClicks} 处 onClick，应为 2（勾选单元格 + 行本身）`);
  }
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    const i = code.indexOf('key: "programs"');
    if (i < 0) throw new Error(`${path.basename(rel)} 找不到程序位列（key: "programs"）`);
    const obj = enclosingBraces(code, i);
    if (!obj) throw new Error(`${path.basename(rel)} 无法解析程序位列对象`);
    if (/onClick/.test(obj)) {
      throw new Error(`${path.basename(rel)} 的程序位列带 onClick —— 表格行超出 2 个可点区（应只读）`);
    }
    if (/<Button/.test(obj)) {
      throw new Error(`${path.basename(rel)} 的程序位列用了 <Button> —— 应只读徽标（§1.10）`);
    }
  }
  return "壳 2 处 onClick · 两页程序位列只读";
});

/* ── P10 网关接入与模型自动识别（来源条契约 + 接入指引自动发现）───────────────
 *
 * 上面 39 条覆盖 P0–P4；P10 此前**一条都没有** ⇒「门禁全绿」证明不了来源条契约
 * （上一个工程师的变异打的是 P3 的断言）。下面 7 条锁住 P10 最易回退的判据：
 * 来源条契约存在 / 非实时必有原因 / 不伪装来源 / 识别中是按钮操作中态（Loader2 而非 Skeleton）/
 * 不承诺「可用」/ 白屏防线 + 壳内无平台分支 / 接入指引自动发现。
 *
 * ⚠️ 每条都**只锚在代码上**（`readCode` 先剥注释）；T04 变异验证会逐条把它打红 ——
 * 锚在注释上的断言会被「注释里写着反例」骗过去（本文件建立当天就踩过）。
 */

const CATALOG_SOURCE = "src/lib/catalog-source.ts";
const CATALOG_BAR = "src/components/gateway/catalog-source-bar.tsx";
const WB_GUIDE = "src/components/gateway/integration-guide.tsx";
const TRAE_GUIDE = "src/components/gateway/trae-integration-guide.tsx";
const WB_LOCALE_ZH = "src/locales/domains/wbStats.zh.ts";
const TRAE_LOCALE_ZH = "src/locales/domains/traeGateway.zh.ts";
const SHARED_LOCALE_ZH = "src/locales/domains/shared.zh.ts";

/** 从域文件里取一个键的**字面量值**（用于断言「文案口径」，而不只是「键存在」）。 */
function localeValue(rel, key) {
  const code = fs.readFileSync(path.join(ROOT, rel), "utf8");
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = code.match(new RegExp(`"${escaped}"\\s*:\\s*"([^"]*)"`));
  if (!m) throw new Error(`找不到键 ${key}（${rel}）`);
  return m[1];
}

check("★ P10 来源条契约存在：describeCatalogSource 归一快照，view 带 reason", () => {
  const code = readCode(CATALOG_SOURCE);
  for (const fn of ["describeCatalogSource", "formatRelativeTime", "formatAbsoluteTime"]) {
    if (!new RegExp(`export function ${fn}\\b`).test(code)) throw new Error(`缺导出函数 ${fn}`);
  }
  if (!/reason\s*:\s*string\s*\|\s*null/.test(code)) {
    throw new Error("`CatalogSourceView` 缺 `reason` 字段（原因无法成为契约）");
  }
  return "契约三函数 + view.reason 齐备";
});

check("★ P10 非实时必有原因（kind !== live ⇒ reason 恒非空，有 i18n 兜底）", () => {
  const code = readCode(CATALOG_SOURCE);
  if (!/reasonCached/.test(code)) throw new Error("缺 cached 的兜底原因键");
  if (!/reasonBuiltin/.test(code)) throw new Error("缺 builtin 的兜底原因键");
  /* 后端 `refresh()` 失败且无缓存时走 `builtin_snapshot(region, Some(DEGRADE_NOTE))`
     ⇒ 内置快照**可能带 note**，且那条 note 不含「尚未成功识别过」。故「从未识别成功」
     这一支必须**显式**拼兜底措辞，不能只靠「note 缺失才兜底」。 */
  if (!/neverIdentified/.test(code)) throw new Error("未显式处理「从未识别成功」分支");
  /* 兜底必须**真的接在 reason 上**：删掉 `?? t(REASON_KEY[kind])` ⇒ 非实时可能无原因 ⇒ 变红。 */
  if (!/\?\?\s*t\(REASON_KEY\[kind\]\)/.test(code)) {
    throw new Error("非实时原因没有 i18n 兜底（`?? t(REASON_KEY[kind])` 缺失）");
  }
  const builtin = localeValue(WB_LOCALE_ZH, "wbStats.gateway.reasonBuiltin");
  if (!/尚未成功识别过/.test(builtin)) {
    throw new Error(`reasonBuiltin 不含「尚未成功识别过」：${builtin}`);
  }
  return `兜底原因齐备 · reasonBuiltin=「${builtin}」`;
});

check("★ P10 三级来源显式标注（不许把「内置」伪装成「实时」）", () => {
  const code = readCode(CATALOG_SOURCE);
  /* 三级**逐字映射**：把 builtin 指向 sourceLive 即「伪装成实时」⇒ 本条变红。 */
  for (const [kind, key] of [
    ["live", "sourceLive"],
    ["cached", "sourceCached"],
    ["builtin", "sourceBuiltin"],
  ]) {
    if (!new RegExp(`${kind}\\s*:\\s*"wbStats\\.gateway\\.${key}"`).test(code)) {
      throw new Error(`徽标映射缺 ${kind} → ${key}（来源被伪装 / 漏标）`);
    }
  }
  if (!/VARIANT\[kind\]/.test(code)) throw new Error("徽标语义色未由 kind 驱动（可能写死）");
  const bar = readCode(CATALOG_BAR);
  if (!/view\.badgeLabel/.test(bar) || !/view\.badgeVariant/.test(bar)) {
    throw new Error("来源条没有按 view 渲染徽标文案 / 语义色");
  }
  return "三级逐字映射 + kind 驱动语义色 + 壳按 view 渲染";
});

check("★ P10「识别中」是按钮操作中态（Loader2），不是区块加载态（禁 Skeleton）", () => {
  const bar = readCode(CATALOG_BAR);
  if (!/<Loader2\b/.test(bar)) throw new Error("「识别中」没用 <Loader2>（§3.6 按钮操作中）");
  if (/<Skeleton\b/.test(bar)) {
    throw new Error("来源条里出现 <Skeleton> —— 「识别中」不是区块加载态（§3.6 明确禁止）");
  }
  if (!/disabled=\{scanning\}/.test(bar)) throw new Error("识别中按钮未 disabled（可重复点击）");
  const scanning = localeValue(WB_LOCALE_ZH, "wbStats.gateway.scanning");
  if (!/识别中/.test(scanning)) throw new Error(`识别中文案不含「识别中」：${scanning}`);
  return `Loader2 · disabled · 文案「${scanning}」`;
});

check("★ P10 计数只说「已识别」，禁止「N 个模型可用」（HTTP 200 ≠ 可用）", () => {
  const count = localeValue(SHARED_LOCALE_ZH, "shared.gateway.sourceBar.count");
  if (!/已识别/.test(count)) throw new Error(`计数文案不含「已识别」：${count}`);
  if (/可用/.test(count)) throw new Error(`计数文案承诺了「可用」：${count}`);
  /* 全仓反例扫描：把「已识别」与「可用」等同的措辞（§3.17.3 陷阱 3 / D24）。 */
  const bad = [];
  for (const abs of listSourceFiles()) {
    if (/\d+\s*个模型可用/.test(stripComments(fs.readFileSync(abs, "utf8")))) {
      bad.push(path.relative(ROOT, abs));
    }
  }
  if (bad.length) throw new Error(`出现「N 个模型可用」措辞：${[...new Set(bad)].join(", ")}`);
  return `计数=「${count}」· 无「可用」承诺`;
});

check("★ P10 白屏防线 + 共用壳内无平台/来源分支", () => {
  const code = readCode(CATALOG_SOURCE);
  /* `snapshot.note` 运行时可能是对象 —— 当 React 子节点渲染会卸载整棵树 ⇒ 白屏（issue #2）。 */
  if (!/displayText\(/.test(code)) throw new Error("`note` 未过 `displayText`（对象会白屏）");
  const bar = readCode(CATALOG_BAR);
  if (/\bplatform\s*===|\bplatform\s*:/.test(bar)) {
    throw new Error("共用壳内出现平台判断 —— 差异应走数据（§1.2 反向约束）");
  }
  if (/client-cache|source\s*===\s*"/.test(bar)) {
    throw new Error("共用壳内出现来源分支 —— 词汇应由调用方注入");
  }
  return "displayText 归一 · 壳内无平台/来源分支";
});

check("★ P10 接入指引说明客户端自动发现模型（/v1/models + 不需要手填模型名）", () => {
  for (const rel of [WB_GUIDE, TRAE_GUIDE]) {
    if (!/autoDiscover/.test(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 未引用「自动发现」说明`);
    }
  }
  for (const [rel, key] of [
    [WB_LOCALE_ZH, "wbStats.gateway.autoDiscover"],
    [TRAE_LOCALE_ZH, "trae.gateway.guide.autoDiscover"],
  ]) {
    const text = localeValue(rel, key);
    if (!/\/v1\/models/.test(text)) throw new Error(`${key} 没提 /v1/models`);
    if (!/不需要手填模型名/.test(text)) throw new Error(`${key} 没说明「不需要手填模型名」`);
  }
  return "两平台接入指引均含自动发现说明";
});

/* ── P10 独立验证增补（p10-verify · 2026-10-06）───────────────────────────────
 *
 * 上面 7 条是**文本扫描**（锚在代码上）。本段补两类更强的判据：
 *
 * 1. **运行时**：真加载 `src/lib/catalog-source.ts` 调 `describeCatalogSource`，构造快照
 *    断言契约（「非实时必有原因」「从未识别成功必含兜底措辞」「三级可区分」「白屏防线」）。
 *    —— 文本扫描证明「代码里写着 `neverIdentified`」，运行时证明「它真的按预期拼措辞」。
 * 2. **跨宿主接线**：`create_api_key` 先落 Key 再 fire-and-forget 探测（失败不阻断）+
 *    两宿主登记 / 派发 `GatewayCatalogProbe`（「登记即执行」纪律）。
 *
 * ⚠️ 纪律同 P10 段：断言只锚在代码上（`readCode` 先剥注释）。
 */

const P10_TAURI_COMMANDS = "src-tauri/src/commands.rs";
const P10_TAURI_LIB = "src-tauri/src/lib.rs";
const P10_SERVER_API = "crates/buddy-switch-server/src/api.rs";
const P10_SERVER_MAIN = "crates/buddy-switch-server/src/main.rs";
const P10_CATALOG_RS = "crates/buddy-switch-core/src/modules/catalog.rs";
const P10_CATALOG_PROBE_RS = "crates/buddy-switch-gateway/src/catalog_probe.rs";
const P10_TRAE_MODEL_LIST = "src/components/gateway/trae-model-list.tsx";

/** 真加载 `catalog-source.ts`（`loadTs` 会解析 `@/` 别名与无扩展名导入）。 */
const P10_CATALOG_SOURCE = loadTs(path.join(ROOT, "src/lib/catalog-source.ts"));

/** 桩 `t`：回显键名（+参数），用于断言「哪条措辞被用了」——不依赖真实词表。 */
const p10StubT = (key, params) =>
  params ? `${key}(${JSON.stringify(params)})` : key;

check("★ P10（独立验证）运行时：非实时必有原因（live 恒 null）", () => {
  const { describeCatalogSource } = P10_CATALOG_SOURCE;
  for (const [label, snap] of [
    ["cached 无 note", { region: "cn", source: "cached", fetched_at: 1, models: [], note: null }],
    ["builtin 无 note", { region: "cn", source: "builtin", fetched_at: null, models: [], note: null }],
    ["null 快照", null],
  ]) {
    const v = describeCatalogSource(snap, p10StubT);
    if (!v.reason) throw new Error(`${label} ⇒ reason 为空（非实时必有原因契约被破坏）`);
  }
  const live = describeCatalogSource(
    { region: "cn", source: "live", fetched_at: 1, models: [], note: "x" },
    p10StubT,
  );
  if (live.reason !== null) throw new Error(`live ⇒ reason 应为 null，实际 ${JSON.stringify(live.reason)}`);
  return "非实时 reason 非空 · live reason=null";
});

check("★ P10（独立验证）运行时：从未识别成功（builtin + note=DEGRADE_NOTE）仍含兜底措辞（?? 回归陷阱）", () => {
  const { describeCatalogSource } = P10_CATALOG_SOURCE;
  /* 模拟 `catalog.rs` 的降级路径：内置快照**带一条不含「尚未成功识别过」的 note**。
     若实现退回 `backendReason ?? fallback`，兜底措辞会被 note「顶掉」⇒ 本条变红。 */
  const DEGRADE = "上游接口可能已变更，当前展示的是上次成功或内置兜底目录";
  const v = describeCatalogSource(
    { region: "cn", source: "builtin", fetched_at: null, models: [], note: DEGRADE },
    p10StubT,
  );
  if (!v.reason || !v.reason.includes("reasonBuiltin")) {
    throw new Error(
      `builtin 且 note 非空时丢了兜底措辞（退回 \`??\` 即此形态）：${JSON.stringify(v.reason)}`,
    );
  }
  if (!v.reason.includes(DEGRADE)) throw new Error("兜底措辞未叠加后端 note（应「兜底 · note」）");
  return "reason=「兜底 · note」（?? 回归陷阱被守住）";
});

check("★ P10（独立验证）运行时：三级来源徽标可区分（builtin 不伪装 live）", () => {
  const { describeCatalogSource } = P10_CATALOG_SOURCE;
  const mk = (source) =>
    describeCatalogSource({ region: "cn", source, fetched_at: 1, models: [], note: null }, p10StubT);
  const live = mk("live");
  const cached = mk("cached");
  const builtin = mk("builtin");
  if (builtin.badgeLabel === live.badgeLabel) throw new Error("builtin 徽标 == live 徽标（伪装成实时）");
  if (new Set([live.badgeLabel, cached.badgeLabel, builtin.badgeLabel]).size !== 3) {
    throw new Error(`三级徽标文案有重复：${[live.badgeLabel, cached.badgeLabel, builtin.badgeLabel]}`);
  }
  if (new Set([live.badgeVariant, cached.badgeVariant, builtin.badgeVariant]).size !== 3) {
    throw new Error("三级语义色未区分");
  }
  return "三级徽标 + 语义色互不相同";
});

check("★ P10（独立验证）运行时：白屏防线（note 为对象 ⇒ reason 仍是字符串）", () => {
  const { describeCatalogSource } = P10_CATALOG_SOURCE;
  const v = describeCatalogSource(
    { region: "cn", source: "cached", fetched_at: 1, models: [], note: { zh: "坏值" } },
    p10StubT,
  );
  if (typeof v.reason !== "string") {
    throw new Error(`note 为对象时 reason 非字符串（React 子节点 ⇒ 白屏）：${typeof v.reason}`);
  }
  return "脏对象 note 收敛为兜底字符串";
});

check("★ P10（独立验证）接入即识别：create_api_key 先落 Key 再 fire-and-forget 探测（失败不阻断）", () => {
  const code = readCode(P10_TAURI_COMMANDS);
  const start = code.indexOf("pub async fn create_api_key");
  if (start < 0) throw new Error("commands.rs 找不到 `create_api_key`（是否改名？）");
  const end = code.indexOf("pub fn revoke_api_key", start);
  const body = end < 0 ? code.slice(start) : code.slice(start, end);
  const createIdx = body.indexOf("keys.create(");
  const spawnIdx = body.indexOf("tauri::async_runtime::spawn");
  if (createIdx < 0) throw new Error("create_api_key 未落 Key（找不到 `keys.create(`）");
  if (spawnIdx < 0) {
    throw new Error("create_api_key 缺 fire-and-forget `spawn`（探测被改成阻塞？会阻断接入）");
  }
  if (createIdx > spawnIdx) throw new Error("先探测后落 Key —— 探测失败会阻断接入（顺序被颠倒）");
  if (!body.includes("probe_catalog(region)")) {
    throw new Error("create_api_key 的 spawn 未调 `probe_catalog(region)`（探测出口空转）");
  }
  /* `probe_catalog` 自己负责丢弃探测结果（`let _ = …probe_region`）—— 失败静默、不传播。 */
  if (!/let _ = [\s\S]{0,200}probe_region/.test(code)) {
    throw new Error("探测结果未被丢弃（缺 `let _ = …probe_region`）⇒ 失败会传播，可能阻断接入");
  }
  if (!/json!\(\{\s*"ok"\s*:\s*true/.test(body)) {
    throw new Error("create_api_key 未无条件返回 `ok:true`（返回值可能依赖探测结果）");
  }
  return "先落 Key · spawn 探测 · 结果丢弃 · 无条件返回 ok";
});

check("★ P10（独立验证）接入即识别：两宿主 create_key 都触发探测（缺一即变红）", () => {
  for (const [rel, marker] of [
    [P10_TAURI_COMMANDS, "probe_catalog(region)"],
    [P10_SERVER_API, "catalog_probe::probe_region(&state, region)"],
  ]) {
    if (!readCode(rel).includes(marker)) {
      throw new Error(`${rel} 缺少探测出口 \`${marker}\`（创建 Key 后不会自动识别）`);
    }
  }
  return "桌面端 + 服务端 create_key 均触发探测";
});

check("★ P10（独立验证）网关启动补识别：两宿主都登记并派发 GatewayCatalogProbe（登记即执行）", () => {
  for (const rel of [P10_TAURI_LIB, P10_SERVER_MAIN]) {
    const code = readCode(rel);
    if (!code.includes("TaskKind::GatewayCatalogProbe =>")) {
      throw new Error(`${rel} 的 spawn 派发缺 ` + "`TaskKind::GatewayCatalogProbe =>`" + `（登记即执行）`);
    }
    if (!code.includes("probe_stale_regions(&state)")) {
      throw new Error(`${rel} 缺 \`probe_stale_regions(&state)\` 调用（启动补识别出口空转）`);
    }
  }
  return "两宿主登记 + 派发 GatewayCatalogProbe";
});

check("★ P10（独立验证）探测频率：24h 阈值（stale_regions 走 is_stale），不每次启动全量探测", () => {
  const code = readCode(P10_CATALOG_RS);
  if (!/STALE_AFTER_MS\s*:\s*i64\s*=\s*24\s*\*\s*60\s*\*\s*60\s*\*\s*1000/.test(code)) {
    throw new Error("catalog.rs 的过期阈值不是 24*60*60*1000（探测频率判据被改）");
  }
  if (!/fn is_stale\b/.test(code)) throw new Error("catalog.rs 缺 `is_stale`（启动补识别的判据）");
  if (!/is_stale\(/.test(readCode(P10_CATALOG_PROBE_RS))) {
    throw new Error("catalog_probe.rs 未用 `is_stale` 判定待确认 region（会退化成全量探测）");
  }
  return "STALE_AFTER_MS=24h · stale_regions 走 is_stale";
});

check("★ P10（独立验证）Trae 来源词汇未被统一为三级（决策 #3：不撒谎）", () => {
  const code = readCode(P10_TRAE_MODEL_LIST);
  for (const key of ["trae.gateway.models.sourceCache", "trae.gateway.models.sourceMissing"]) {
    if (!code.includes(key)) throw new Error(`Trae 来源徽标未用「客户端缓存 / 未读取」词汇（缺 ${key}）`);
  }
  for (const banned of [
    "wbStats.gateway.sourceLive",
    "wbStats.gateway.sourceCached",
    "wbStats.gateway.sourceBuiltin",
  ]) {
    if (code.includes(banned)) {
      throw new Error(`Trae 侧出现 WB 三级词汇 ${banned}（硬统一 = UI 撒谎，决策 #3）`);
    }
  }
  if (!code.includes("trae.gateway.models.refresh")) {
    throw new Error("Trae 按钮名未保留「重新读取」（rescanLabel 缺失）");
  }
  return "Trae 词汇=客户端缓存/未读取 · 按钮=重新读取";
});

/* ── P11 模型禁用（四组合 + 两个出口同源 + 计数 + 两出口说明）────────────────────
 *
 * 上面 46 条覆盖 P0–P10；P11 此前**一条都没有** ⇒「门禁全绿」证明不了禁用真的成立
 * （上一个工程师的变异打的是 P10 的断言）。下面 5 条锁住 P11 最易回退的判据：
 * 计数与出口 ① 同源 / 四组合可区分 / WB 不渲染上游列而 Trae 渲染 / 两个出口说明 /
 * 每行一个可点区 + 白屏防线 + 禁用错误可展示。
 *
 * ⚠️ 两条纪律（与 P10 段一致）：
 *   ① 断言**只锚在代码上**（`readCode` 先剥注释）—— 锚在注释上会被「注释里写着反例」骗过去；
 *   ② 能跑真代码的就跑真代码（`servedCount` 从 `src/lib/types.ts` 转译后直接调用），
 *      比文本扫描更硬 —— 变异②（计数口径）正是打在这份**被门禁读取的实现文件**上。
 *
 * ⚠️ **两套验收，各验各的对象**（P11 §1.10.1）：本文件验**真实应用**（`src/`）；
 *    原型套件 `docs/prototype/verify-behaviors.cjs` 验**原型**，**不动**。
 */

const typesMod = loadTs(path.join(ROOT, "src/lib/types.ts"));
const MODEL_TABLE = "src/components/gateway/model-table.tsx";
const WB_MODEL_LIST = "src/components/gateway/model-list.tsx";
const TRAE_MODEL_LIST = "src/components/gateway/trae-model-list.tsx";
const SHARED_LOCALE_EN = "src/locales/domains/shared.en.ts";

check("★ P11 计数口径 = `available && !disabled`，且与出口 ①（/v1/models）同源", () => {
  /* 跑**真代码**：`servedCount` 是唯一实现，两个模型页都调它 —— 页面里各写一份
     `filter(...).length` 就是「计数与出口 ① 会漂移」的种子。
     变异②：把它改成只看 `available`（漏掉 `!disabled`）⇒ served 由 1 变 2 ⇒ 本条变红。 */
  if (typeof typesMod.servedCount !== "function") {
    throw new Error("`src/lib/types.ts` 未导出 `servedCount`（计数口径的唯一定义）");
  }
  const rows = [
    { id: "a", name: "a", scope: "wb-cn", available: true, disabled: false }, // 可用·启用 → 计
    { id: "b", name: "b", scope: "wb-cn", available: true, disabled: true }, // 可用·禁用 → 不计
    { id: "c", name: "c", scope: "wb-cn", available: false, disabled: false }, // 不可用·启用 → 不计
    { id: "d", name: "d", scope: "wb-cn", available: false, disabled: true }, // 不可用·禁用 → 不计
  ];
  const got = typesMod.servedCount(rows);
  if (got.served !== 1) {
    throw new Error(`served 应为 1（四组合里只有「可用·启用」计），实际 ${got.served}`);
  }
  if (got.total !== 4) throw new Error(`total 应为 4（行数），实际 ${got.total}`);

  // 两页都必须**共用**这个实现（而不是各算各的）。
  for (const rel of [WB_MODEL_LIST, TRAE_MODEL_LIST]) {
    if (!/servedCount\(/.test(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 没有用共享的 \`servedCount\` —— 计数会与出口 ① 漂移`);
    }
  }
  return `served=${got.served} / total=${got.total}（四组合只计 1）· 两页共用`;
});

check("★ P11 四组合可区分：上游状态列只看 available、网关暴露列只看 disabled（压成一个字段即变红）", () => {
  /* 两个维度正交（§1.3）：上游状态列只看 `available`，网关暴露列只看 `disabled`。
     变异①：把网关暴露列改成 `row.available ? labels.disabled : labels.exposed`
     ⇒ 下面第二条正则失配 ⇒ 本条变红（四组合塌陷成两种）。 */
  const code = readCode(MODEL_TABLE);

  if (!/row\.available\s*\?\s*labels\.upstreamAvailable\s*:\s*labels\.upstreamUnavailable/.test(code)) {
    throw new Error("上游状态列不是由 `row.available` 驱动（四组合会塌陷）");
  }
  if (!/row\.disabled\s*\?\s*labels\.disabled\s*:\s*labels\.exposed/.test(code)) {
    throw new Error("网关暴露列不是由 `row.disabled` 驱动（四组合会塌陷）");
  }
  // 三种文本必须都在：「已暴露」「已禁用」「不可用」—— 少一个就有组合无法表达。
  for (const k of ["labels.upstreamAvailable", "labels.upstreamUnavailable", "labels.exposed", "labels.disabled"]) {
    if (!code.includes(k)) throw new Error(`缺文本 \`${k}\`（四组合无法全部表达）`);
  }
  // 上游列由 `showUpstream` 开关（WB 恒可用 ⇒ 不渲染该列）。
  if (!/showUpstream/.test(code)) {
    throw new Error("上游状态列没有 `showUpstream` 开关 —— WB 会渲染一列恒为「可用」的噪音");
  }
  /* 每行**两个**可点区：**复制模型 ID** + 禁用 / 启用。**无行点击、无勾选**。
     ★ 2026-10-13：由 1 → 2（新增「复制模型 ID」—— Codex / Trae 自定义模型 / dsh
       自定义 provider 都要求手填模型 id，此前只能手抄）。
     这不是放宽：「恰好 2 处」+ 下面两条**分别**钉住「无行点击」「无勾选」，
     所以任何第三种行级交互都仍会变红。 */
  const clicks = (code.match(/onClick=/g) || []).length;
  if (clicks !== 2) {
    throw new Error(
      `模型表格有 ${clicks} 处 onClick，应为 2（复制模型 ID + 禁用/启用；无行点击、无勾选）`,
    );
  }
  // 无行点击：`<tr>` 上不得挂 onClick（否则整行可点会与「复制」「切换」抢事件）。
  if (/<tr[^>]*\sonClick=/.test(code)) {
    throw new Error("模型表格出现了行级 onClick —— 整行可点会与「复制 ID」「禁用/启用」抢事件");
  }
  // 无勾选：本表刻意不做多选。
  if (/type="checkbox"|role="checkbox"/.test(code)) {
    throw new Error("模型表格出现了勾选 —— 本表刻意不做多选（行级操作只有复制 + 切换）");
  }
  // 复制按钮必须**真的复制**（只有按钮没有 copyText = 假控件）。
  if (!/data-act="gw-model-copy"/.test(code)) {
    throw new Error("模型表格缺「复制模型 ID」按钮（用户要靠它把 id 粘进客户端配置）");
  }
  if (!/copyText\(/.test(code)) {
    throw new Error("模型表格的复制按钮没有调用 `copyText` —— 按钮是假的");
  }
  // 白屏防线：展示字段（name / id）必须过 `displayText`。
  if (!/displayText\(/.test(code)) {
    throw new Error("模型名 / id 未过 `displayText`（对象当 React 子节点会白屏，issue #2）");
  }
  return "上游=available · 暴露=disabled · 两列独立 · 每行 1 可点区 · 过 displayText";
});

check("★ P11 WB 不渲染「上游状态」列、Trae 渲染（决策 #3）", () => {
  const wb = readCode(WB_MODEL_LIST);
  const trae = readCode(TRAE_MODEL_LIST);
  if (!/showUpstream=\{false\}/.test(wb)) {
    throw new Error("WB 未关闭「上游状态」列 —— 它恒为「可用」，是纯噪音（§3.4 规则 1 / 决策 #3）");
  }
  if (!/showUpstream=\{true\}/.test(trae)) {
    throw new Error("Trae 未开启「上游状态」列 —— 它有 `servable` 判据，该列有信息量");
  }
  return "WB showUpstream=false · Trae showUpstream=true";
});

check("★ P11「两个出口同时生效」说明必须出现（否则禁用是假开关，删说明即变红）", () => {
  /* §3.18.1：禁用**必须**两个出口都改 —— 只说「已从清单移除」会让用户以为调用还能过。
     文案必须把「不会出现在 /v1/models」与「直接调用也会被拒绝」一并说清。
     变异③：把两页里的 `shared.gateway.models.twoExits` 渲染删掉 ⇒ 本条变红。 */
  const text = localeValue(SHARED_LOCALE_ZH, "shared.gateway.models.twoExits");
  if (!/\/v1\/models/.test(text)) throw new Error(`两个出口说明没提 \`/v1/models\`：${text}`);
  if (!/调用/.test(text)) throw new Error(`两个出口说明没提「直接调用也会被拒绝」：${text}`);
  if (!/两个出口/.test(text)) throw new Error(`两个出口说明没点明「两个出口同时生效」：${text}`);
  // zh + en 同步（en 缺键会回落中文，但显式加了才算同步）。
  const en = localeValue(SHARED_LOCALE_EN, "shared.gateway.models.twoExits");
  if (!/\/v1\/models/.test(en)) throw new Error(`英文两个出口说明没提 /v1/models：${en}`);
  // 两页都必须渲染该说明。
  for (const rel of [WB_MODEL_LIST, TRAE_MODEL_LIST]) {
    if (!/shared\.gateway\.models\.twoExits/.test(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 没有渲染「两个出口」说明 —— 禁用会变成假开关`);
    }
  }
  return `zh/en 同步 · 两平台均渲染（「${text.slice(0, 18)}…」）`;
});

check("★ issue #65 模型表「思考档位 / 积分倍率」三平台同构、渲染唯一、纯展示", () => {
  /* 需求（issue #65）：模型池里每个模型要能看出「支持哪些思考档位」和「积分倍率」。
     三平台的档位来自**三个不同数据源**（WB 目录 `reasoning` / Trae 客户端
     `reasoning_effort_config` / Qoder 官方 `thinking_efforts`），最容易出的错是
     「各写一份渲染」——那就是同一件事三种写法（P10 §1.2 早就收敛掉的分叉）。

     可证伪：
     ① 共用壳必须有两个**可选**列槽（不传 ⇒ 整列不渲染，避免「全 — 」的噪音列）；
     ② 三个平台页**都**要传（少传一个 ⇒ 该平台看不到这两项，需求只做了一半）；
     ③ 单元格渲染只允许来自 `model-ability.tsx`（页面自己 `join(" / ")` 就变红）；
     ④ 那个文件里**不得**有 onClick（模型表每行恰好 2 处可点区，门禁在上一块）。
     变异：删 `<EffortCell` 的调用 ⇒ ③ 红；给 `model-ability.tsx` 加 `onClick` ⇒ ④ 红。 */
  const shell = readCode(MODEL_TABLE);
  for (const prop of ["effortColumn", "creditsColumn"]) {
    if (!new RegExp(`${prop}\\?:\\s*\\{`).test(shell)) {
      throw new Error(`模型表格壳缺可选列槽 \`${prop}\`（不传就该整列不渲染）`);
    }
    // 表头 + 单元格各渲染一次才算「真的有这一列」。
    const uses = (shell.match(new RegExp(`\\{${prop} &&`, "g")) || []).length;
    if (uses !== 2) {
      throw new Error(`\`${prop}\` 在壳里出现 ${uses} 次，应表头 + 单元格各一次`);
    }
  }

  const ability = readCode("src/components/gateway/model-ability.tsx");
  if ((ability.match(/onClick=/g) || []).length !== 0) {
    throw new Error("model-ability.tsx 出现了 onClick —— 模型表每行只能有 2 处可点区");
  }
  // 「空档位」三种成因的文案必须都在：把 unknown 写成 unsupported 就是撒谎（TraeWork 侧
  // 实测 22 条只有 1 条带档位，整列「—」不代表「都不支持」）。
  for (const key of [
    "shared.gateway.models.effortNoneUnknown",
    "shared.gateway.models.effortNoneUnsupported",
    "shared.gateway.models.effortNoneUndeclared",
  ]) {
    if (!ability.includes(key)) {
      throw new Error(`model-ability.tsx 缺「${key}」的成因文案`);
    }
  }

  const pages = { WB: WB_MODEL_LIST, Trae: TRAE_MODEL_LIST, Qoder: "src/components/gateway/qoder-model-list.tsx" };
  for (const [label, rel] of Object.entries(pages)) {
    const code = readCode(rel);
    for (const prop of ["effortColumn", "creditsColumn"]) {
      if (!new RegExp(`${prop}=\\{\\{`).test(code)) {
        throw new Error(`${label} 没传 \`${prop}\` —— 该平台看不到这两项（issue #65 要求三平台一起加）`);
      }
    }
    if (!/from "@\/components\/gateway\/model-ability"/.test(code)) {
      throw new Error(`${label} 没有复用 model-ability 的单元格渲染（自己拼一份 = 又一处分叉）`);
    }
    if (!/\bEffortCell\b/.test(code)) {
      throw new Error(`${label} 未使用 <EffortCell>（档位渲染必须走共用组件）`);
    }
  }
  return "壳两个列槽 · 三平台都传 · 渲染只在 model-ability · 无 onClick";
});

check("★ issue #101 图片输入列：三平台都传、渲染唯一、未声明不得画成不支持", () => {
  /* 需求（issue #101）：客户端连本网关时，图片被它自己的模型配置挡掉（请求根本没到网关）。
     网关能做的是在 `/v1/models` 声明能力，并在管理面让用户能纠正 —— 两件事都得在。

     可证伪：
     ① 共用壳必须有 `imageColumn` 可选列槽，且表头 + 单元格各渲染一次；
     ② 三个平台页**都**要传（少传一个 ⇒ 该平台看不到、也改不了，需求只做了一半）；
     ③ 单元格渲染只允许来自 `model-image-cell.tsx`（页面自己拼就变红）；
     ④ **三态文案必须都在**：把 `未声明` 写成 `不支持` 就是把「不知道」说成事实；
     ⑤ 判据只允许在 `lib/model-image.ts`（页面里出现 `?? false` 即变红）。 */
  const shell = readCode(MODEL_TABLE);
  if (!/imageColumn\?:\s*\{/.test(shell)) {
    throw new Error("模型表格壳缺可选列槽 `imageColumn`（不传就该整列不渲染）");
  }
  const uses = (shell.match(/\{imageColumn &&/g) || []).length;
  if (uses !== 2) {
    throw new Error(`\`imageColumn\` 在壳里出现 ${uses} 次，应表头 + 单元格各一次`);
  }

  const cell = readCode("src/components/gateway/model-image-cell.tsx");
  for (const key of [
    "shared.gateway.models.imageYes",
    "shared.gateway.models.imageNo",
    "shared.gateway.models.imageUndeclared",
  ]) {
    if (!cell.includes(key)) throw new Error(`model-image-cell.tsx 缺「${key}」—— 三态无法表达`);
  }
  if (!/data-act="gw-model-image-toggle"/.test(cell)) {
    throw new Error("model-image-cell.tsx 缺覆盖切换按钮（只有徽标 = 只能看不能改）");
  }

  const judge = readCode("src/lib/model-image.ts");
  if (!judge.includes("supports: null")) {
    throw new Error("lib/model-image.ts 没有「未声明」这一档（supports: null）—— 三态塌成布尔");
  }
  if (/\?\?\s*false/.test(judge)) {
    throw new Error("lib/model-image.ts 出现 `?? false` —— 把「未声明」压成「不支持」就是撒谎");
  }

  const pages = {
    WB: WB_MODEL_LIST,
    Trae: TRAE_MODEL_LIST,
    Qoder: "src/components/gateway/qoder-model-list.tsx",
  };
  for (const [label, rel] of Object.entries(pages)) {
    const code = readCode(rel);
    if (!/imageColumn=\{imageColumn\}/.test(code)) {
      throw new Error(`${label} 没传 \`imageColumn\` —— 该平台看不到也改不了图片能力`);
    }
    if (!/useImageCapColumn/.test(code)) {
      throw new Error(`${label} 没有复用共用装配器（自己拼一份 = 又一处分叉）`);
    }
    if (code.includes("supports ?? false") || code.includes("supports_images ?? false")) {
      throw new Error(`${label} 出现 \`?? false\` —— 「未声明」被写成了「不支持」`);
    }
  }

  // zh / en 同步（en 缺键会回落中文，但显式加了才算同步）。
  for (const file of [SHARED_LOCALE_ZH, SHARED_LOCALE_EN]) {
    const text = readCode(file);
    for (const key of [
      "colImages",
      "imageYes",
      "imageNo",
      "imageUndeclared",
      "imageUndeclaredHint",
      "imageMarkYes",
      "imageReset",
    ]) {
      if (!text.includes(`shared.gateway.models.${key}`)) {
        throw new Error(`${path.basename(file)} 缺 \`${key}\` 词条`);
      }
    }
  }
  return "壳有列槽 · 三平台都传 · 渲染唯一 · 三态文案齐 · 无 ?? false";
});

check("★ P11 禁用错误可展示（后端 403 model_disabled 的文案经 api.asError 透出）", () => {
  /* T01 对硬调用返回 403 / `type_name="model_disabled"` / 文案说明「被禁用」。
     前端要**能展示它**：切换失败路径必须把错误经 `api.asError` 本地化后透出
     （而不是吞掉 / 换成笼统「操作失败」）—— 否则用户不知道「为什么这个模型调不动」。 */
  for (const rel of [WB_MODEL_LIST, TRAE_MODEL_LIST]) {
    const code = readCode(rel);
    if (!/shared\.gateway\.models\.toggleFail/.test(code)) {
      throw new Error(`${path.basename(rel)} 切换失败没有提示`);
    }
    if (!/api\.asError\(/.test(code)) {
      throw new Error(`${path.basename(rel)} 切换失败未用 \`api.asError\` 透出后端原因（403 model_disabled）`);
    }
  }
  return "两页均以 api.asError 透出禁用错误";
});

/* ── P11 T04 后端「两个出口」+ 接入说明验证（独立验证 · 真实应用）────────────────
 *
 * ⚠️ 上面 5 条 P11 断言全部落在**前端**（`src/`）—— 它们证明不了「禁用真的生效」。
 *    禁用成立的条件是**后端两个出口同时生效**（§1.2 / §3.18.1）：出口① `GET /v1/models`
 *    过滤 + 出口② `POST /v1/chat/completions` / `/v1/messages` 拒绝。只改一个 = **假开关**
 *    （用户以为禁用了、实际还能调）。故下面把**后端**也钉住。
 *
 * ⚠️ 锚只落在**代码**上（`readCode` 先剥注释）；`is_disabled` 的定义唯一性用**精确签名**
 *    匹配（仓里另有 `pool` 侧的账号级 `fn is_disabled(&self)`，与模型禁用无关，不得误伤）。
 *
 * ⚠️ 变异验证（T04）：① 删出口① 的 `.filter(is_disabled)` ⇒ 本条①红；
 *    ② 删出口② 的 `ModelDisabled` 分支 ⇒ 本条②红；③ 把任一调用点换成自写判断 ⇒ 本条③红；
 *    ④ 把 Anthropic 的 `authHeader` 改成 Bearer ⇒ 本条⑤红；⑤ 给 generic 加 `settings.json` ⇒ 本条⑥红。
 */

const GW_SRC_DIR = path.join(ROOT, "crates/buddy-switch-gateway/src");
const GW_DISABLED_RS = "crates/buddy-switch-gateway/src/disabled_models.rs";
const GW_MODELS_RS = "crates/buddy-switch-gateway/src/routes/models.rs";
const GW_CHAT_RS = "crates/buddy-switch-gateway/src/routes/chat.rs";
const GW_MESSAGES_RS = "crates/buddy-switch-gateway/src/routes/messages.rs";
const GW_TRAE_ROUTES_RS = "crates/buddy-switch-gateway/src/trae/routes.rs";
const GW_TRAE_PAYLOAD_RS = "crates/buddy-switch-gateway/src/trae/payload.rs";

/** 该源码是否把**模型禁用**判断解析到共享符号 `disabled_models::is_disabled`。 */
function usesSharedIsDisabled(code) {
  return (
    /disabled_models::is_disabled\s*\(/.test(code) ||
    (/use\s+crate::disabled_models::is_disabled\b/.test(code) && /\bis_disabled\s*\(/.test(code))
  );
}

check("★ P11 出口①（GET /v1/models）按禁用过滤，且走唯一判断点 is_disabled（删过滤即变红）", () => {
  const code = readCode(GW_MODELS_RS);
  if (!/\.filter\(/.test(code)) {
    throw new Error("出口① 没有 `.filter(` —— 被禁模型仍会出现在 /v1/models（假开关）");
  }
  if (!/is_disabled\s*\(/.test(code)) {
    throw new Error("出口① 没有调用 `is_disabled` —— 过滤口径与出口② 会漂移");
  }
  if (!usesSharedIsDisabled(code)) {
    throw new Error("出口① 的 `is_disabled` 不是来自 `disabled_models`（自写判断 = 出口漂移种子）");
  }
  return "models.rs 过滤 + 共享 is_disabled";
});

check("★ P11 出口②（调用）按禁用拒绝，且走唯一判断点 is_disabled（删拒绝即变红）", () => {
  for (const rel of [GW_CHAT_RS, GW_MESSAGES_RS]) {
    const code = readCode(rel);
    if (!/is_disabled\s*\(/.test(code)) {
      throw new Error(`${path.basename(rel)} 调用路径没有 is_disabled 拒绝 —— 被禁模型仍可硬调用（假开关）`);
    }
    if (!usesSharedIsDisabled(code)) {
      throw new Error(`${path.basename(rel)} 的 is_disabled 不是来自 disabled_models`);
    }
    if (!/GatewayError::ModelDisabled\b/.test(code)) {
      throw new Error(`${path.basename(rel)} 命中禁用未返回 GatewayError::ModelDisabled`);
    }
  }
  return "chat.rs + messages.rs 拒绝 + 共享 is_disabled";
});

check("★ P11 Trae 出口②（调用）两条协议都按禁用拒绝（删一条即变红）", () => {
  const code = readCode(GW_TRAE_ROUTES_RS);
  const calls = (code.match(/disabled_models::is_disabled\s*\(/g) || []).length;
  if (calls < 2) {
    throw new Error(`Trae 调用出口只有 ${calls} 处 is_disabled，应 ≥2（/v1/chat/completions + /v1/messages）`);
  }
  if (!/model_disabled/.test(code)) throw new Error("Trae 命中禁用未返回 model_disabled");
  return `trae/routes.rs ${calls} 处拒绝`;
});

check("★ P11 唯一判断点：is_disabled 全仓只定义一次，四个调用点解析到同一符号（各写一份即变红）", () => {
  /* 「两个出口必须用**同一个**判断」是 P11 最硬的纪律（§1.2 / R2）。若有人在任一出口
     另写一份判断（例如 `disabled.get(region).contains(model)`），两处就会漂移 ⇒ 又变假开关。
     变异③：把 chat.rs 的 `crate::disabled_models::is_disabled(...)` 换成自写判断 ⇒ 本条红。 */
  const defRe = /fn\s+is_disabled\s*\(\s*store\s*:\s*&DisabledModels\b/;
  const defs = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".rs")) {
        const rel = path.relative(ROOT, full).split(path.sep).join("/");
        if (defRe.test(readCode(rel))) defs.push(rel);
      }
    }
  };
  walk(GW_SRC_DIR);
  if (defs.length !== 1) {
    throw new Error(`模型禁用判断点定义了 ${defs.length} 次（应恰好 1 次）：${defs.join(", ") || "0 处"}`);
  }
  if (defs[0] !== GW_DISABLED_RS) {
    throw new Error(`唯一判断点应在 ${GW_DISABLED_RS}，实际在 ${defs[0]}`);
  }
  for (const rel of [GW_MODELS_RS, GW_CHAT_RS, GW_MESSAGES_RS, GW_TRAE_ROUTES_RS]) {
    if (!usesSharedIsDisabled(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 未解析到共享的 disabled_models::is_disabled（各写一份 = 出口漂移）`);
    }
  }
  return "1 处定义（disabled_models.rs）· 4 处调用点共用";
});

check("★ P11 禁用判断只此一处：禁止在别处对禁用集合做 `.contains(`（Trae 出口① 必须复用共享判断）", () => {
  /* §1.2：判断逻辑只有一处。`trae/payload.rs` 曾用 `disabled.contains(name)` 自写判断 ——
     那是「出口漂移」种子（T04 #1：Trae 出口① 与出口② 会各判各的）。
     本条规定：除 `disabled_models.rs` 外，**任何文件都不得**直接对禁用集合做 `.contains(`；
     Trae 出口① 必须调共享的 `is_disabled_in`。
     变异：把 `is_disabled_in(...)` 改回 `disabled.contains(...)` ⇒ 本条红。 */
  const offenders = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".rs")) {
        const rel = path.relative(ROOT, full).split(path.sep).join("/");
        if (/disabled\.contains\s*\(/.test(readCode(rel))) offenders.push(rel);
      }
    }
  };
  walk(GW_SRC_DIR);
  if (offenders.length) {
    throw new Error(`下列文件自写「禁用集合成员判断」（应走 disabled_models::is_disabled_in）：${offenders.join(", ")}`);
  }
  if (!/is_disabled_in\s*\(/.test(readCode(GW_TRAE_PAYLOAD_RS))) {
    throw new Error("trae/payload.rs 未复用共享的 `is_disabled_in`（Trae 出口① 与出口② 会漂移）");
  }
  return "无自写 `.contains(` · payload 走 is_disabled_in";
});

check("★ P11 大小写不敏感：禁用 `X` 后调用 `X` 的大小写变体也必须被拒（改成精确匹配即变红）", () => {
  /* T04 #2 的真 bug：上游对模型名大小写不敏感（Trae 的 `model_config` 先 `to_lowercase()`），
     若判断点精确匹配，禁用 `glm-5.3` 后调 `GLM-5.3` 会**绕过**。判断点必须大小写不敏感。
     （存储保持原始大小写 —— 前端 `disabledSet.has(model.id)` 依赖精确匹配，见 `disabled_models.rs` 注释。） */
  const code = readCode(GW_DISABLED_RS);
  if (!/eq_ignore_ascii_case\s*\(/.test(code)) {
    throw new Error("判断点没有大小写不敏感匹配（`eq_ignore_ascii_case` 缺失）—— 大小写变体会绕过禁用");
  }
  return "判断点走 eq_ignore_ascii_case";
});

check("★ P11 Trae 出口① 按**该作用域**的禁用集合过滤（串了作用域即变红）", () => {
  /* Trae 的禁用集合按**作用域**分家（2026-10-15 从 region 升级：三个平台互不相通的模型
     目录不得共用一把键）。出口① 必须把 `scope_of_trae_region(variant.region())`
     对应的集合交给清单构造器，而不是全局集合 / 别的作用域的集合。
     变异：把取集键换成裸 region / 固定作用域 ⇒ 本条变红。 */
  const routes = readCode(GW_TRAE_ROUTES_RS);
  if (!/scope_of_trae_region\(variant\.region\(\)\)/.test(routes)) {
    throw new Error(
      "Trae 出口① 未按 `scope_of_trae_region(variant.region())` 取禁用集合（可能串了作用域）",
    );
  }
  // 2026-10-20：签名多了 `max_mode`（前端开关），但**禁用集合必须是第二个实参**这一点不变。
  if (!/models_response_for\(\s*variant\s*,\s*&disabled\s*(?:,\s*[^)]*)?\)/.test(routes)) {
    throw new Error("Trae 出口① 未把该作用域的禁用集合传给 `models_response_for`");
  }
  return "scope_of_trae_region → models_response_for(&disabled)";
});

/* ── P11 T04 修复的**独立复核**（另派验证者 · 2026-10-06）─────────────────────────
 *
 * 上面 T04 段锁的是「两个出口 + 唯一判断点 + 大小写」。本段补**复核者独立验出**的四点：
 *   #4 存储键 `String`（未知 region 键不清空已知键）/ #3 `deny_unknown_fields`（形状错响亮报错）
 *   / 存储**保留原始大小写**的依据（前端精确匹配）/ #5 WB 路由级 e2e（残余缺口）。
 *
 * ⚠️ 锚只落在**代码**上（`readCode` 先剥注释）；每条写明变异落点。
 */

check("★ P11 T04 #4：禁用存储键是 `String`（不是 `Scope`）—— 未知作用域键不清空已知键", () => {
  /* 变异：把 `HashMap<String, BTreeSet<String>>` 改回 `HashMap<Region, …>` ⇒ 未知 region 键让
     整份反序列化失败 ⇒ `load()` 回落空表 ⇒ **全部** region 的偏好被静默清空（T04 #4 的脚枪）。 */
  const code = readCode(GW_DISABLED_RS);
  if (!/HashMap<\s*String\s*,\s*BTreeSet<String>\s*>/.test(code)) {
    throw new Error("禁用存储的 map 键不是 `String`（未知 region 键会清空全部偏好）");
  }
  if (/HashMap<\s*Region\s*,/.test(code)) {
    throw new Error("禁用存储又用 `Region` 作键了（未知键 ⇒ 整份失败 ⇒ 静默清空）");
  }
  // `get` 必须按**作用域 id** 取（与落盘的字符串键同口径），否则取不到。
  if (!/scopes\.get\(scope\.as_str\(\)\)/.test(code)) {
    throw new Error("`get` 未按 `scope.as_str()` 取集合（与字符串键不同口径 ⇒ 恒空集）");
  }
  // 旧 region 键的迁移必须存在（否则升级会把老用户的禁用偏好整体丢掉）。
  if (!/fn migrate_legacy_keys/.test(code)) {
    throw new Error("缺 `migrate_legacy_keys` —— 旧 region 键的禁用偏好会整体丢失");
  }
  return "String 键 · get 按 as_str() · 旧键迁移在";
});

check("★ P11 T04 #3：`deny_unknown_fields`（裸 map 形状响亮报错，不再静默空表）", () => {
  /* 变异：删掉 `deny_unknown_fields` ⇒ 裸 map 形状（缺 `regions` 字段）静默得到空表
     ⇒ 全部禁用**静默失效**（T04 #3 要消灭的形态）。 */
  const code = readCode(GW_DISABLED_RS);
  if (!/serde\(default,\s*deny_unknown_fields\)/.test(code)) {
    throw new Error("`DisabledModels` 缺 `deny_unknown_fields` —— 形状错会静默得到空表");
  }
  return "deny_unknown_fields 在（形状错 ⇒ 报错）";
});

check("★ P11 存储必须**保留原始大小写**：前端 `disabledSet.has(model.id)` 是精确匹配（加归一即变红）", () => {
  /* 这是「判断大小写不敏感、但存储不归一」这条设计约束的**依据**：前端两处禁用行都用
     `disabledSet.has(<model.id>)` **精确**匹配（无 toLowerCase）。若把存储归一成小写，
     混合大小写模型（如 `DeepSeek-V4-Flash-Official`）在 UI 上会显示成**未禁用** —— 更糟。
     变异：给 `disabledSet.has(...)` 加 `.toLowerCase()`（或把存储归一）⇒ 本条红。 */
  const hits = [];
  for (const rel of [
    "src/components/gateway/model-list.tsx",
    "src/components/gateway/trae-model-list.tsx",
  ]) {
    const code = readCode(rel);
    const calls = code.match(/disabledSet\.has\([^)]*\)/g) || [];
    if (calls.length === 0) throw new Error(`${path.basename(rel)} 没有 \`disabledSet.has(...)\``);
    for (const call of calls) {
      if (/toLowerCase|toUpperCase|\.trim\(\)/.test(call)) {
        throw new Error(`${path.basename(rel)} 的 \`${call}\` 做了归一 —— 与「存储保留原样」的约束冲突`);
      }
    }
    hits.push(...calls);
  }
  return `${hits.length} 处精确匹配（无归一）⇒ 存储必须保留原始大小写`;
});

check("★ P11 T04 #5（WB 残余缺口）：WB 路由级 e2e 存在（真实路由 + 鉴权 + 禁用存储 + 403）", () => {
  /* 修复者补了 Trae 侧路由级 e2e；WB 侧此前只有源码扫描。复核者补上 WB 路由级 e2e：
     出口① 真的少一条、出口② 真的 403 `model_disabled`。变异：删该测试文件 ⇒ 本条红。 */
  const rel = "crates/buddy-switch-gateway/tests/wb_gateway_p11_e2e.rs";
  if (!fs.existsSync(path.join(ROOT, rel))) {
    throw new Error("缺 WB 路由级 e2e（T04 #5 残余缺口未补）");
  }
  const code = fs.readFileSync(path.join(ROOT, rel), "utf8");
  for (const need of ["router(", "GatewayState::new", "disabled.write()", "model_disabled", "FORBIDDEN"]) {
    if (!code.includes(need)) throw new Error(`WB e2e 缺少「${need}」（不是真路由级）`);
  }
  return "WB 路由级 e2e 在（router + 鉴权 + 禁用存储 + 403）";
});

const snippetsMod = loadTs(path.join(ROOT, "src/lib/gateway-snippets.ts"));
/** 片段生成器只要一个能回显键名的 `t`（断言只关心**英文契约**，不关心自然语言）。 */
const echoT = (key, options) => (options && options.url ? `${key} ${options.url}` : key);
const SNIPPET_BASE = {
  t: echoT,
  baseUrl: "http://127.0.0.1:8080/v1",
  rootUrl: "http://127.0.0.1:8080",
  key: "sk-test",
  model: "glm-5.3",
};

check("★ P11 认证头按协议给对：Anthropic→x-api-key / OpenAI→Bearer（改成 Bearer 即变红）", () => {
  /* §3.18.2：认证头由**协议**决定。给错 = 用户配不通（R7）。
     跑**真代码** `gatewaySnippet` + `protocolOf`，不是文本扫描。
     变异④：把 `anthropic.authHeader` 改成 `Authorization: Bearer` ⇒ 本条红。 */
  if (typeof snippetsMod.gatewaySnippet !== "function") throw new Error("未导出 `gatewaySnippet`");
  const anth = snippetsMod.gatewaySnippet({ ...SNIPPET_BASE, protocol: "anthropic", tool: "generic" });
  const oai = snippetsMod.gatewaySnippet({ ...SNIPPET_BASE, protocol: "openai", tool: "generic" });
  if (!/x-api-key/.test(anth)) throw new Error("Anthropic 片段未给 `x-api-key`");
  if (/Bearer/.test(anth)) throw new Error("Anthropic 片段错给了 Bearer 认证头（用户配不通）");
  if (!/Authorization: Bearer/.test(oai)) throw new Error("OpenAI 片段未给 `Authorization: Bearer`");
  if (/x-api-key/.test(oai)) throw new Error("OpenAI 片段错给了 `x-api-key`");

  const anthProto = snippetsMod.protocolOf("anthropic");
  const oaiProto = snippetsMod.protocolOf("openai");
  if (anthProto.authHeader !== "x-api-key") throw new Error(`Anthropic authHeader=${anthProto.authHeader}（应 x-api-key）`);
  if (anthProto.endpoint !== "POST /v1/messages") throw new Error(`Anthropic endpoint=${anthProto.endpoint}`);
  if (oaiProto.authHeader !== "Authorization: Bearer") throw new Error(`OpenAI authHeader=${oaiProto.authHeader}`);
  if (oaiProto.endpoint !== "POST /v1/chat/completions") throw new Error(`OpenAI endpoint=${oaiProto.endpoint}`);
  return "Anthropic=x-api-key/POST /v1/messages · OpenAI=Bearer/POST /v1/chat/completions";
});

check("★ P11 generic 只给通用参数、不编造配置文件片段（加 settings.json 即变红）", () => {
  /* §1.8：未确认的客户端一律「协议 + 通用参数 + 以其官方文档为准」。
     编错的配置比没有说明更糟 —— 用户会以为网关坏了（R6）。
     变异⑤：给 generic 片段加一行含 `settings.json` ⇒ 本条红。 */
  for (const protocol of ["openai", "anthropic"]) {
    const s = snippetsMod.gatewaySnippet({ ...SNIPPET_BASE, protocol, tool: "generic" });
    if (!/Base URL/.test(s)) throw new Error(`generic(${protocol}) 缺 Base URL`);
    for (const bad of ["config.toml", "settings.json", "opencode.json"]) {
      if (s.includes(bad)) throw new Error(`generic(${protocol}) 编造了配置文件片段 ${bad}（§1.8 禁止）`);
    }
  }
  return "两协议 generic 均只给通用参数（无配置文件名）";
});

check("★ P11 Codex 给 base_url+env_key；Claude Code 给 ANTHROPIC_*（删即变红）", () => {
  const codex = snippetsMod.gatewaySnippet({ ...SNIPPET_BASE, protocol: "openai", tool: "codex" });
  if (!/base_url/.test(codex)) throw new Error("Codex 片段缺 `base_url`");
  if (!/env_key/.test(codex)) throw new Error("Codex 片段缺 `env_key`");
  const cc = snippetsMod.gatewaySnippet({ ...SNIPPET_BASE, protocol: "anthropic", tool: "claude-code" });
  for (const k of ["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_MODEL"]) {
    if (!cc.includes(k)) throw new Error(`Claude Code 片段缺 ${k}`);
  }
  return "Codex=base_url+env_key · Claude Code=ANTHROPIC_BASE_URL/AUTH_TOKEN/MODEL";
});

/* ── P6 加速器（命令面板 ⌘K / 动态作用域快捷键 / 首次引导）──────────────────────
 *
 * 上面 51 条覆盖 P0–P11；P6 此前**一条都没有** ⇒「门禁全绿」证明不了面板复用同一搜索索引、
 * 快捷键降级、字段归属。下面这些条锁住 P6 最易回退的判据。
 *
 * ⚠️ 三条纪律（与 P10/P11 段一致）：
 *   ① 断言**只锚在代码上**（`readCode` 先剥注释）；
 *   ② 能跑真代码就跑真代码（`planScopeShortcuts` / `buildPaletteItems` / 归一函数直接调用）；
 *   ③ **降级分支必须有断言模拟 >9 个作用域**：真实应用 6 个（3 平台 × 2 区域）⇒ 降级分支
 *      **仍走不到**，而「永不执行」和「正确」在测试里长得一样 —— 不模拟就是死代码。
 *
 * ⚠️ **两套验收，各验各的对象**：本文件验**真实应用**（`src/`）；
 *    原型套件 `docs/prototype/verify-behaviors.cjs` / `verify-demo.cjs` 验**原型**，**不动**。
 */

const paletteLib = loadTs(path.join(ROOT, "src/lib/command-palette.ts"));
const scopeShortcutsLib = loadTs(path.join(ROOT, "src/lib/scope-shortcuts.ts"));
const paletteActionsLib = loadTs(path.join(ROOT, "src/lib/palette-actions.ts"));
const shortcutPrefLib = loadTs(path.join(ROOT, "src/lib/shortcut-preference.ts"));
const drawerTabLib = loadTs(path.join(ROOT, "src/lib/drawer-tab-preference.ts"));
const firstRunLib = loadTs(path.join(ROOT, "src/lib/first-run.ts"));
const accountSearchLib = loadTs(path.join(ROOT, "src/lib/account-search.ts"));

const PALETTE_LIB = "src/lib/command-palette.ts";
const PALETTE_COMPONENT = "src/components/command-palette/command-palette.tsx";
const FIRST_RUN_COMPONENT = "src/components/first-run-hint.tsx";
const APP_SETTINGS = "src/components/app-settings.tsx";
const APP_TSX = "src/App.tsx";

const FOUR_SCOPES = ["wb-cn", "wb-global", "trae-cn", "trae-global"];
/** 真实注册表的作用域（6 个：3 平台 × 2 区域）—— 从 `scope.ts` 现取，不写死。 */
const REAL_SCOPES = [...scope.SCOPES];
/** ★ 模拟「超过上限（9）」的规模，逼出降级分支（真实 6 个 ≤ 9 ⇒ 走不到）。 */
const TEN_SCOPES = [
  ...FOUR_SCOPES,
  "future-cn",
  "future-global",
  "future3-cn",
  "future3-global",
  "future4-cn",
  "future4-global",
];

/** 内存版 storage（门禁不跑浏览器）。 */
function memStorage() {
  const store = {};
  return {
    getItem: (key) => (Object.prototype.hasOwnProperty.call(store, key) ? store[key] : null),
    setItem: (key, value) => {
      store[key] = value;
    },
  };
}

/** 构造一个最小 `PaletteContext`（纯数据，够 buildPaletteItems 跑起来）。 */
function paletteCtx(overrides = {}) {
  return {
    t: (key) => key,
    currentScope: "wb-cn",
    scopes: REAL_SCOPES,
    scopeShortcutIndex: new Map(),
    accounts: [],
    destinations: registry.DESTINATIONS,
    navigate: () => {},
    actions: [],
    services: { refreshScope() {}, addAccount() {}, openDataDir() {} },
    ...overrides,
  };
}

/** 取某个具名函数的函数体（用于逐字段核对字段归属）。 */
function fnBody(code, name) {
  const at = code.indexOf(`function ${name}(`);
  if (at < 0) throw new Error(`找不到函数 ${name}`);
  const body = enclosingBraces(code, code.indexOf("{", at));
  if (!body) throw new Error(`无法解析 ${name} 的函数体`);
  return body;
}

check("★ P6 降级分支：作用域 >9 ⇒ 不绑定 + reason=too-many（真实应用 6 个 ⇒ 必须模拟 10 个）", () => {
  /* 变异①：删掉 `> SCOPE_SHORTCUT_LIMIT` 判断（超限仍返回绑定）⇒ 本条变红。 */
  const ten = scopeShortcutsLib.planScopeShortcuts({ scopes: TEN_SCOPES, enabled: true });
  if (ten.bindings.size !== 0) throw new Error(`10 个作用域仍绑定了 ${ten.bindings.size} 个 —— 降级失效`);
  if (!ten.degraded || ten.reason !== "too-many") {
    throw new Error(`应 degraded=true / reason=too-many，实际 ${ten.degraded}/${ten.reason}`);
  }
  const off = scopeShortcutsLib.planScopeShortcuts({ scopes: FOUR_SCOPES, enabled: false });
  if (off.bindings.size !== 0 || !off.degraded || off.reason !== "user-disabled") {
    throw new Error("用户关闭未降级为 user-disabled");
  }
  const on = scopeShortcutsLib.planScopeShortcuts({ scopes: FOUR_SCOPES, enabled: true });
  if (on.bindings.size !== 4 || on.degraded || on.reason !== null) throw new Error("4 个作用域应正常绑定");
  if (on.bindings.get("wb-cn") !== 1 || on.bindings.get("trae-global") !== 4) throw new Error("序号不是 1..N");
  /* ★ 真实规模（6 个）必须**照常绑定** —— 加 Qoder 不能让既有用户的 1..4 一起消失
     （变异②：把上限改回 5 ⇒ 本条变红）。 */
  if (REAL_SCOPES.length !== 6) throw new Error(`真实作用域应 6 个，实际 ${REAL_SCOPES.length}`);
  const real = scopeShortcutsLib.planScopeShortcuts({ scopes: REAL_SCOPES, enabled: true });
  if (real.bindings.size !== 6 || real.degraded || real.reason !== null) {
    throw new Error(`真实 6 个作用域应正常绑定，实际 ${real.bindings.size}/${real.degraded}/${real.reason}`);
  }
  if (real.bindings.get("qoder-global") !== 6) throw new Error("第 6 个作用域应绑到 ⌘6");
  if (scopeShortcutsLib.SCOPE_SHORTCUT_LIMIT !== 9) throw new Error("阈值应为 9（数字键天然上限）");
  return "10 个→too-many / 关闭→user-disabled / 4 个→1..4 / 真实 6 个→1..6";
});

check("★ P6 绑定与序号提示同源：空 bindings ⇒ 作用域条目一律无 shortcutHint（假控件护栏）", () => {
  /* 变异：在 buildPaletteItems 里硬编码 ⌘1..⌘4 ⇒ 降级（空 bindings）时仍渲染提示 ⇒ 变红。 */
  const degraded = paletteLib.buildPaletteItems(paletteCtx());
  const degradedScopes = degraded.filter((i) => i.group === "scope");
  if (degradedScopes.length !== REAL_SCOPES.length) {
    throw new Error(`作用域条目应 ${REAL_SCOPES.length} 条，实际 ${degradedScopes.length}`);
  }
  if (degradedScopes.some((i) => i.shortcutHint)) {
    throw new Error("降级（空 bindings）时仍渲染了 ⌘N 提示 —— 假控件");
  }

  const boundIndex = new Map(REAL_SCOPES.map((scope, index) => [scope, index + 1]));
  const boundScopes = paletteLib
    .buildPaletteItems(paletteCtx({ scopeShortcutIndex: boundIndex }))
    .filter((i) => i.group === "scope");
  if (boundScopes.some((i) => !i.shortcutHint)) throw new Error("绑定时应渲染 ⌘N 提示");
  const traeCn = boundScopes.find((i) => i.id === "scope:trae-cn");
  if (!traeCn || !traeCn.shortcutHint.includes("3")) throw new Error("序号提示与绑定不一致");
  return "空 bindings 无提示 / 有 bindings 有 ⌘N";
});

check("★ P6 面板账号匹配复用 account-search（大写/空白查询也一致，另写匹配即变红）", () => {
  /* 变异②：面板另写 `includes` 匹配（不做小写 / 去空白归一）⇒「  JACKEY  」查不到 ⇒ 变红。 */
  const accounts = [
    { scope: "wb-cn", key: "id-1", displayName: "Jackey", email: "Jackey@Example.com", uid: "u1", remark: null },
    { scope: "wb-global", key: "id-2", displayName: "Alice", email: null, uid: null, remark: "note" },
    { scope: "trae-cn", key: "uid-3", displayName: "Bob", email: null, uid: "uid-3", remark: null },
  ];
  const items = paletteLib
    .buildPaletteItems(paletteCtx({ accounts }))
    .filter((i) => i.group === "account");
  if (items.length !== accounts.length) throw new Error(`账号条目应 ${accounts.length} 条，实际 ${items.length}`);

  for (const q of ["jackey", " JACKEY ", "alice", "bob", "zzz"]) {
    const got = new Set(paletteLib.filterPaletteItems(items, q).map((i) => i.id));
    const want = new Set(
      items.filter((i) => accountSearchLib.matchesAccountQuery(i.searchText, q)).map((i) => i.id),
    );
    if (got.size !== want.size || [...got].some((id) => !want.has(id))) {
      throw new Error(`查询「${q}」面板结果与 account-search 不一致（两套匹配 = 静默错误）`);
    }
  }
  const upper = paletteLib.filterPaletteItems(items, "  JACKEY  ");
  if (!upper.some((i) => i.id === "account:wb-cn:id-1")) {
    throw new Error("大写 + 空白查询未命中 —— 面板可能没复用 account-search 的归一化");
  }
  return "5 种查询与 account-search 一致（含大写 / 空白）";
});

check("★ P6 面板源码复用 account-search（不另写匹配 / 不自己 toLowerCase）", () => {
  const code = readCode(PALETTE_LIB);
  if (!/matchesAccountQuery\(/.test(code)) throw new Error("面板没有调用 `matchesAccountQuery`");
  if (!/accountSearchText\(/.test(code)) throw new Error("面板没有调用 `accountSearchText`");
  if (/\.toLowerCase\(\)/.test(code)) {
    throw new Error("面板出现 `toLowerCase()` —— 疑似另写归一化（应复用 account-search）");
  }
  return "调用 accountSearchText / matchesAccountQuery · 无本地归一化";
});

check("★ P6 字段归属：WB 键=id/名=nickname；Trae 键=userId/名=name；面板源码禁 lastUsedAt", () => {
  /* 变异③：把面板源码改成引用 `lastUsedAt`（或把 WB 名改读 `name` / Trae 键改读 `id`）⇒ 变红。 */
  for (const rel of [PALETTE_LIB, PALETTE_COMPONENT]) {
    if (/lastUsedAt/.test(readCode(rel))) {
      throw new Error(
        `${path.basename(rel)} 引用了 \`lastUsedAt\` —— 它属 ApiKeyRecord / TraeApiKeyRecord，不是账号字段`,
      );
    }
  }

  const code = readCode(PALETTE_LIB);
  const wb = fnBody(code, "paletteAccountFromWorkbuddy");
  if (!/key:\s*account\.id\b/.test(wb)) throw new Error("WB 稳定键应取 `account.id`");
  if (!/displayText\(account\.nickname\)/.test(wb)) throw new Error("WB 展示名应取 `nickname`（WB 没有 name 字段）");
  if (/account\.name\b/.test(wb)) throw new Error("WB 归一用了 `account.name` —— WB 账号没有该字段");

  const trae = fnBody(code, "paletteAccountFromTrae");
  if (!/key:\s*account\.userId\b/.test(trae)) throw new Error("Trae 稳定键应取 `account.userId`");
  if (!/displayText\(account\.name\)/.test(trae)) throw new Error("Trae 展示名应取 `name`");
  if (/account\.id\b|account\.uid\b/.test(trae)) {
    throw new Error("Trae 归一用了 `account.id` / `account.uid` —— Trae 账号没有这两个字段");
  }
  return "WB id/nickname · Trae userId/name · 无 lastUsedAt";
});

check("★ P6 白屏防线：账号名喂对象 ⇒ 归一成字符串（走 displayText，去 displayText 即变红）", () => {
  const wb = paletteLib.paletteAccountFromWorkbuddy(
    { id: "x", nickname: { zh: "脏值" }, email: null, uid: null, remark: null },
    "wb-cn",
  );
  if (typeof wb.displayName !== "string") throw new Error("WB displayName 不是字符串 —— 对象会被当 React 子节点 ⇒ 白屏");
  if (wb.displayName !== "") throw new Error(`对象昵称应归一为 ""，实际 ${JSON.stringify(wb.displayName)}`);

  const trae = paletteLib.paletteAccountFromTrae({ userId: "u", name: { en: "dirty" }, remark: null }, "trae-cn");
  if (typeof trae.displayName !== "string" || trae.displayName !== "") {
    throw new Error("Trae displayName 未过 `displayText`");
  }

  const items = paletteLib.buildPaletteItems(paletteCtx({ accounts: [wb, trae] }));
  if (items.some((i) => typeof i.title !== "string" || typeof i.searchText !== "string")) {
    throw new Error("面板条目 title / searchText 非字符串");
  }
  return "对象昵称 → 空串 · 条目字段均字符串";
});

check("★ P6 面板不新增作用域切换控件（只导航，防「第二个看起来能切的控件」）", () => {
  const code = readCode(PALETTE_COMPONENT);
  for (const bad of ['role="tablist"', "<Tabs", "ProductSwitch", "ScopeSwitch"]) {
    if (code.includes(bad)) throw new Error(`面板出现作用域切换控件痕迹：${bad}`);
  }
  if (!/navigate\(/.test(code)) throw new Error("面板没有用 navigate（应只导航）");
  return "无 tablist / 无切换控件 / 走 navigate";
});

check("★ P5 §2.7 回收站已落：`account_restore` 必须存在 + 删除文案改「可在回收站恢复」", () => {
  /* ★ 本断言原为**反向护栏**（P5 未落 ⇒ UI 不许承诺可恢复）。P5 T01 已交付「软删除 + 回收站」
     （后端 + 最小恢复入口同批），故此处**反转**为**正向**断言：
       ① `api.ts` 必须出现 `account_restore`（软删除若无恢复入口 = 「有数据、没入口」中间态）；
       ② 三处删除文案必须写「回收站」，且**不得**再出现与事实不符的「此操作不可撤销」。
     ★ P6 T04（撤销 toast）依赖的 `account_restore` **现已存在** ⇒ 解锁。故 `undo-toast.ts`
       由「**不得存在**」**反转为「必须存在」**（反向护栏在事实变化时反转，与 P5 本条同一模式）。
       撤销 toast 的**实质保证**（真撤销 / 不假承诺 / 失败不静默）在下方「P6 T04」一组断言里守。 */
  const apiCode = fs.readFileSync(path.join(ROOT, "src/lib/api.ts"), "utf8");
  if (!/account_restore|accountRestore/.test(apiCode)) {
    throw new Error("api.ts 缺 `account_restore` —— P5 回收站后端未接线（软删除无恢复入口 = 中间态）");
  }
  if (!fs.existsSync(path.join(ROOT, "src/lib/undo-toast.ts"))) {
    throw new Error("缺 `src/lib/undo-toast.ts` —— P6 T04 撤销 toast 未落地（依赖 account_restore 已满足，应已解锁）");
  }
  const deleteCopies = [
    ["WB 单删", "src/locales/domains/wbAccounts.zh.ts", "wbAccounts.dialog.deleteDesc"],
    ["批量删", "src/locales/domains/shared.zh.ts", "shared.batch.deleteDesc"],
    ["Trae 删", "src/locales/domains/traePages.zh.ts", "trae.page.accounts.deleteBody"],
  ];
  for (const [label, rel, key] of deleteCopies) {
    const copy = localeValue(rel, key);
    if (!copy.includes("回收站")) {
      throw new Error(`${label} 文案未写「回收站」（软删除已落，文案必须与事实一致）：${copy}`);
    }
    if (copy.includes("不可撤销")) {
      throw new Error(`${label} 文案仍写「不可撤销」—— 与软删除事实不符（假承诺）：${copy}`);
    }
  }
  return "account_restore 在 · undo-toast 在 · 三处删除文案写「回收站」且无「不可撤销」";
});

/* ──────────────────────────────────────────────────────────────────────────
 * P5 §2.7.1 · 回收站 UI（最小恢复入口）
 *
 * 后端软删除 + 回收站命令已落（上一条断言）。这一组守**入口本身**：
 *   ① 入口在**账号页工具栏**（按当前 scope），**不放设置页**（避免与 P7「备份与恢复」的
 *      整库级语义混淆）；
 *   ② ★ `purge`（永久删除）与「清空」是**唯一不可逆步** ⇒ **必须二次确认**，清空**明示条数**；
 *   ③ ★ 每条**必须显示到期时间**（否则「以为还在」的假期待）；
 *   ④ 恢复 = 移回 live ⇒ 成功后**必须刷新账号列表**（`onRestored`）；
 *   ⑤ 状态语言（Skeleton / Loader2 / EmptyState 含出口）+ `displayText` 白屏防线。
 *
 * 每条都写明**变异落点**（去掉该保证即变红）。
 * ────────────────────────────────────────────────────────────────────────── */

const RECYCLE_BIN = "src/components/recycle-bin-sheet.tsx";
const RECYCLE_BIN_LOCALE_ZH = "src/locales/domains/recycleBin.zh.ts";
const RECYCLE_BIN_LOCALE_EN = "src/locales/domains/recycleBin.en.ts";

check("★ P5 §2.7.1 回收站入口在**账号页工具栏**（按 scope），且**不放设置页**", () => {
  /* 变异：把 `<RecycleBinSheet>` 挪进设置页 / 不传 `scope` ⇒ 本条变红。
     依据：回收站是**账号级 / 单 scope**；放设置页会与 P7「备份与恢复」（整库级 / 跨 scope）
     语义混淆（§2.7.1）。不传 scope ⇒ 跨作用域串库（看到 / 删到别的库的账号）。 */
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    if (!/<RecycleBinSheet\b/.test(code)) {
      throw new Error(`${path.basename(rel)} 未挂载 <RecycleBinSheet>（软删除无恢复入口 = 中间态）`);
    }
    if (!/scope=\{scope\}/.test(code)) {
      throw new Error(`${path.basename(rel)} 未把**当前 scope** 传给回收站 —— 会跨作用域串库`);
    }
    if (!/setRecycleOpen\(true\)/.test(code)) {
      throw new Error(`${path.basename(rel)} 工具栏没有打开回收站的入口`);
    }
  }
  for (const rel of ["src/pages/SettingsPage.tsx", "src/pages/TraeSettingsPage.tsx"]) {
    if (/<RecycleBinSheet\b/.test(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 挂了回收站 —— 回收站是账号级 / 单 scope，应放账号页（§2.7.1）`);
    }
  }
  return "两账号页挂载且传 scope · 两设置页未挂载";
});

check("★ P5 §2.7 回收站 purge（永久删除）**必须二次确认**（去掉确认即变红）", () => {
  /* 变异①（落点 = recycle-bin-sheet.tsx）：让条目「永久删除」按钮**直接调用** `purge()`
     （跳过确认框）⇒ 下面的 `setConfirm({ kind: "purge" … })` 锚失配 ⇒ 本条变红。
     依据（§2.7 / 验收 13）：purge 是**唯一不可逆步**，未确认**绝不执行**。 */
  const code = readCode(RECYCLE_BIN);
  if (!/<Dialog\s+open=\{confirm !== null\}/.test(code)) {
    throw new Error("回收站缺「二次确认」Dialog（应 `<Dialog open={confirm !== null}>`）");
  }
  if (!/variant="destructive"[\s\S]{0,220}setConfirm\(\{\s*kind:\s*"purge"/.test(code)) {
    throw new Error("条目「永久删除」按钮未走二次确认（应只 `setConfirm({ kind: \"purge\" })`）");
  }
  if (!/void purge\(confirm\.key, confirm\.name\)/.test(code)) {
    throw new Error("确认框未接 `purge`（永久删除未走二次确认）");
  }
  return "确认 Dialog 受 confirm 控制 · 行内按钮只 setConfirm · 确认后才 purge";
});

check("★ P5 §2.7 清空回收站**二次确认 + 明示条数**（去掉条数即变红）", () => {
  /* 变异：清空按钮直接调 `clearAll()` / 文案去掉 `{n}` ⇒ 本条变红。
     依据（§2.7 / 验收 13）：批量 ⇒ **更强确认**（明示将删除的条数）。 */
  const code = readCode(RECYCLE_BIN);
  if (!/setConfirm\(\{\s*kind:\s*"clear",\s*count\s*\}\)/.test(code)) {
    throw new Error("清空按钮未走二次确认（应 `setConfirm({ kind: \"clear\", count })`）");
  }
  if (!/confirm\.kind === "clear"\) void clearAll\(\)/.test(code)) {
    throw new Error("确认框未接 `clearAll`（清空未走二次确认）");
  }
  const zh = localeValue(RECYCLE_BIN_LOCALE_ZH, "recycleBin.clear.desc");
  if (!/\{n\}/.test(zh)) throw new Error(`清空文案未明示条数（缺 {n}）：${zh}`);
  const en = localeValue(RECYCLE_BIN_LOCALE_EN, "recycleBin.clear.desc");
  if (!/\{n\}/.test(en)) throw new Error(`清空 en 文案未明示条数（缺 {n}）：${en}`);
  return "清空走确认 · 文案明示 {n}（zh+en）";
});

check("★ P5 §2.7 回收站每条**必须显示到期时间**（删掉即变红）", () => {
  /* 变异②（落点 = recycle-bin-sheet.tsx）：删掉到期时间那一行 ⇒ 本条变红。
     依据（§2.7 / 验收 14）：条目超过 30 天会被启动维护 `sweep_trash_for` **静默清理**；
     不显示到期时间 ⇒ 用户「以为还在」的假期待。 */
  const code = readCode(RECYCLE_BIN);
  if (!/formatDate\(entry\.expiresAt\)/.test(code)) {
    throw new Error("回收站条目未渲染到期时间（`formatDate(entry.expiresAt)`）");
  }
  if (!/recycleBin\.entry\.expiresLabel/.test(code)) {
    throw new Error("到期时间未用 i18n 标签（`recycleBin.entry.expiresLabel`）");
  }
  const label = localeValue(RECYCLE_BIN_LOCALE_ZH, "recycleBin.entry.expiresLabel");
  if (!/到期/.test(label)) throw new Error(`到期标签文案不含「到期」：${label}`);
  return `条目渲染「${label}」· formatDate(expiresAt)`;
});

check("★ P5 §2.7.1 恢复 = 移回 live ⇒ 成功后必须刷新账号列表（onRestored）", () => {
  /* 变异：把 `onRestored` 传参删掉 / 组件里不调 ⇒ 本条变红。
     依据（§2.7.1 / 验收 11）：恢复后账号回到 live，页面若不重取 ⇒ 用户「恢复完却看不到」。 */
  const wb = readCode(WB_PAGE);
  if (!/onRestored=\{\(\)\s*=>\s*void reconcileAccounts\(region\)\}/.test(wb)) {
    throw new Error("WorkBuddy 回收站未在恢复后刷新账号库（`onRestored` → reconcileAccounts）");
  }
  const tp = readCode(TRAE_PAGE);
  if (!/onRestored=\{\(\)\s*=>\s*void loadAll\(\)\}/.test(tp)) {
    throw new Error("Trae 回收站未在恢复后重取快照（`onRestored` → loadAll）");
  }
  const comp = readCode(RECYCLE_BIN);
  if (!/onRestored\?\.\(\)/.test(comp)) {
    throw new Error("回收站组件 restore 成功后未回调 `onRestored`");
  }
  return "WB→reconcileAccounts · Trae→loadAll · 组件调 onRestored";
});

check("★ P5 回收站状态语言：Skeleton / Loader2 / EmptyState（含出口）+ displayText 白屏防线", () => {
  /* 变异：把区块加载换成一行文字 / 空态去掉出口 / 账号名不过 `displayText` ⇒ 本条变红。
     依据（PLAN §3.6）：区块加载用 `<Skeleton>`、按钮操作用 `<Loader2>`（不是区块加载态）、
     空态用 `<EmptyState>`（四要素，**含出口**）；`displayText` 是白屏防线（issue #2）。 */
  const code = readCode(RECYCLE_BIN);
  if (!/<Skeleton\b/.test(code)) throw new Error("区块加载未用 <Skeleton>");
  if (!/<Loader2\b/.test(code)) throw new Error("按钮操作中未用 <Loader2>");
  if (!/<EmptyState\b/.test(code)) throw new Error("空态未用 <EmptyState>");
  if (!/recycleBin\.empty\.action/.test(code)) {
    throw new Error("回收站空态缺**出口**（`recycleBin.empty.action`）—— 把用户丢在原地");
  }
  if (!/displayText\(entry\.name\)/.test(code)) {
    throw new Error("账号名未过 `displayText` 白屏防线（对象当 React 子节点会卸载整棵树）");
  }
  /* 单 scope 视图：不得出现「第二个看起来能切」的作用域切换控件。 */
  for (const bad of ['role="tablist"', "<Tabs", "ScopeSwitch", "ProductSwitch", "<Select"]) {
    if (code.includes(bad)) throw new Error(`回收站出现切换控件痕迹：${bad}（单 scope 视图的假控件）`);
  }
  return "Skeleton + Loader2 + EmptyState(含出口) + displayText · 无切换控件";
});

check("★ P5 回收站词条 zh + en 同步（逐键对齐，且已组合进词表）", () => {
  const keysOf = (rel) =>
    new Set([...fs.readFileSync(path.join(ROOT, rel), "utf8").matchAll(/"([^"]+)"\s*:/g)].map((m) => m[1]));
  const zk = keysOf(RECYCLE_BIN_LOCALE_ZH);
  const ek = keysOf(RECYCLE_BIN_LOCALE_EN);
  for (const k of zk) if (!ek.has(k)) throw new Error(`en 缺键 ${k}`);
  for (const k of ek) if (!zk.has(k)) throw new Error(`zh 缺键 ${k}`);
  if (zk.size < 15) throw new Error(`回收站词条过少（${zk.size}）—— 是不是没组合进来？`);
  if (!/domains\/recycleBin\.zh/.test(readCode("src/locales/zh.ts"))) throw new Error("zh.ts 未组合 recycleBin 域");
  if (!/domains\/recycleBin\.en/.test(readCode("src/locales/en.ts"))) throw new Error("en.ts 未组合 recycleBin 域");
  return `${zk.size} 键 zh=en 且已组合`;
});

/* ──────────────────────────────────────────────────────────────────────────
 * P6 T04 · 撤销 toast（★ 假承诺护栏 —— 本期最硬的一条）
 *
 * 依赖已满足（`account_restore` 存在，P5 §2.7）⇒ 解锁。这一组守**实质保证**：
 *   ① ★「撤销」真的能撤销 —— 两账号页删除接线里的 `undo` 必须调 `account_restore`
 *      （与回收站**同一命令**，不另写恢复逻辑）；
 *   ② ★ 不许做假承诺 —— 文案写「已删除，可撤销」，**不得**出现「已永久删除」
 *      （那是 `recycleBin.purge` 的语义）；
 *   ③ ★ 不许「让用户以为没救了」—— 文案**不得**暗示「过期即永久删除」
 *      （超时只是 toast 消失，账号**仍在回收站**，可随时恢复）；
 *   ④ 连续删多个 ⇒ 每条 toast **独立**（创建时不传固定 id，不互相覆盖）；
 *   ⑤ 撤销失败**不静默**（`toast.error` + 后端原因）。
 *
 * 变异落点（打在同一份 `src/` 实现文件上，见 T04 报告）：
 *   ① 把删除接线里的 `undo` 改成只关 toast、不调 `account_restore` ⇒ 本条变红（防「假撤销」）；
 *   ② 把撤销标题改成「已永久删除」⇒ 本条变红（防假承诺）；
 *   ③ 让文案暗示「过期即永久删除」⇒ 本条变红（防「让用户以为没救了」）。
 * ────────────────────────────────────────────────────────────────────────── */

const UNDO_TOAST = "src/lib/undo-toast.ts";
const SHELL_LOCALE_ZH = "src/locales/domains/shell.zh.ts";
const SHELL_LOCALE_EN = "src/locales/domains/shell.en.ts";

/** 从源码里截取某个调用起点之后的片段（把断言锚在**同一个调用**里，而非全文存在性）。 */
function sliceFrom(code, anchor, span) {
  const at = code.indexOf(anchor);
  if (at < 0) return "";
  return code.slice(at, at + span);
}

check("★ P6 T04 撤销护栏：`undo-toast.ts` 存在，且 `undo` **必填**（改成可选即变红）", () => {
  /* 变异：把 `undo` 改成可选（`undo?:`）⇒ 可构造「无支撑的撤销」⇒ 本条变红。
     依据（§1.4）：`undo` 是**编译期护栏** —— 没有后端恢复调用就构造不出撤销 toast。 */
  if (!fs.existsSync(path.join(ROOT, UNDO_TOAST))) {
    throw new Error(`缺 ${UNDO_TOAST}（撤销 toast 未落地）`);
  }
  const code = readCode(UNDO_TOAST);
  if (!/interface UndoToastInput\b/.test(code)) throw new Error("缺 `UndoToastInput` 接口");
  if (!/export function showUndoToast\b/.test(code)) throw new Error("缺 `showUndoToast` 导出");
  if (/\bundo\?\s*:/.test(code)) {
    throw new Error("`undo` 是**可选** —— 假承诺护栏失效（必须必填，缺它应编译不过）");
  }
  if (!/\bundo\s*:\s*\(\)\s*=>\s*Promise<void>/.test(code)) {
    throw new Error("`undo` 必须是**必填**的 `() => Promise<void>`");
  }
  return "undo-toast.ts 在 · undo 必填";
});

check("★ P6 T04「撤销」真的能撤销：两账号页删除接线调 `account_restore`（与回收站同一命令）", () => {
  /* 变异①（落点 = AccountsPage.tsx / TraeAccountsPage.tsx）：把删除接线里的 `undo` 改成
     只关 toast、不调 `account_restore` ⇒ 本条变红（防「假撤销」——点了以为账号回来、实际没有）。 */
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    if (!/showUndoToast\(/.test(code)) {
      throw new Error(`${path.basename(rel)} 删除后未调 \`showUndoToast\`（缺撤销入口）`);
    }
    const block = sliceFrom(code, "showUndoToast(", 900);
    if (!/accountRestore\(/.test(block)) {
      throw new Error(`${path.basename(rel)} 的撤销未调 \`accountRestore\` —— 点了没作用 = 假撤销`);
    }
  }
  /* 撤销与回收站**同一命令**：不得各写一份恢复逻辑（否则两处漂移）。 */
  const sheet = readCode(RECYCLE_BIN);
  if (!/accountRestore\(scope,\s*entry\.key\)/.test(sheet)) {
    throw new Error("回收站未调 `accountRestore` —— 两处恢复入口应同源（同一条命令）");
  }
  return "两页撤销调 accountRestore · 与回收站同命令";
});

check("★ P6 T04 撤销失败**不静默** + 连续删多个各自独立（创建时不传固定 id）", () => {
  /* 变异：撤销失败吞掉异常（不 `toast.error`）/ 创建时传**固定** id（多条互相覆盖）⇒ 变红。 */
  const code = readCode(UNDO_TOAST);
  if (!/toast\.error\(/.test(code)) throw new Error("撤销失败未走 `toast.error`（静默失败）");
  if (!/catch\b/.test(code)) throw new Error("`showUndoToast` 未捕获 `undo` 的异常");
  /* 创建 toast 时**不得**带**字符串字面量** id（那会让第二次删除覆盖第一条的撤销）。
     就地更新用的是捕获到的变量 `id`（`{ id }`），不是字面量 ⇒ 不触发。 */
  if (/\bid\s*:\s*["'`]/.test(code)) {
    throw new Error("创建撤销 toast 时传了**固定 id** —— 连续删多个会互相覆盖（撤销丢失）");
  }
  return "失败 toast.error · 创建不带固定 id";
});

check("★ P6 T04 假承诺红线：文案写「可撤销」且**不得**出现「已永久删除」（那是 purge 语义）", () => {
  /* 变异②：把撤销标题改成「已永久删除」⇒ 本条变红。
     依据（§1.4 / 任务硬约束 2）：走 `account_restore` 就写「已删除，可撤销」；「永久删除」
     是 `recycleBin.purge` 的语义，写在这里 = 与实际能力不符的假承诺。 */
  const zh = localeValue(SHELL_LOCALE_ZH, "undo.deletedTitle");
  const en = localeValue(SHELL_LOCALE_EN, "undo.deletedTitle");
  if (!/撤销/.test(zh)) throw new Error(`撤销标题未写「可撤销」：${zh}`);
  if (!/undo/i.test(en)) throw new Error(`撤销 en 标题未写 undo：${en}`);
  return `zh「${zh}」/ en「${en}」`;
});

check("★ P6 T04 不许「让用户以为没救了」：撤销文案**不得**暗示「过期即永久删除」", () => {
  /* 变异③：让文案暗示「过期即永久删除」⇒ 变红。
     依据（§1.4 / 任务硬约束 4）：超时后账号**仍在回收站**（不是「窗口过了就永久删除」）；
     把「还有救」说成「没救了」是与假承诺同类的误导。 */
  const FORBIDDEN = [
    "永久删除",
    "永久",
    "不可恢复",
    "无法恢复",
    "没救了",
    "过期即",
    "过期后",
    "permanently",
    "cannot be undone",
    "cannot be restored",
    "expired",
  ];
  const keys = ["undo.deletedTitle", "undo.batchTitle", "undo.action", "undo.undone", "undo.failedTitle"];
  for (const rel of [SHELL_LOCALE_ZH, SHELL_LOCALE_EN]) {
    for (const key of keys) {
      const value = localeValue(rel, key);
      for (const bad of FORBIDDEN) {
        if (value.includes(bad)) {
          throw new Error(
            `${path.basename(rel)} ${key} 含「${bad}」—— 暗示「过期即永久删除」，会让用户以为没救了：${value}`,
          );
        }
      }
    }
  }
  return `${keys.length} 键 × 2 语言无「永久 / 不可恢复 / 过期」措辞`;
});

check("★ P6 T04 撤销词条 zh + en 同步（逐键对齐，含 5 个 undo.* 键）", () => {
  const keysOf = (rel) =>
    new Set([...fs.readFileSync(path.join(ROOT, rel), "utf8").matchAll(/"([^"]+)"\s*:/g)].map((m) => m[1]));
  const zk = keysOf(SHELL_LOCALE_ZH);
  const ek = keysOf(SHELL_LOCALE_EN);
  for (const k of zk) if (!ek.has(k)) throw new Error(`en 缺键 ${k}`);
  for (const k of ek) if (!zk.has(k)) throw new Error(`zh 缺键 ${k}`);
  const want = ["undo.deletedTitle", "undo.batchTitle", "undo.action", "undo.undone", "undo.failedTitle"];
  for (const key of want) {
    if (!zk.has(key)) throw new Error(`zh 缺键 ${key}`);
    if (!ek.has(key)) throw new Error(`en 缺键 ${key}`);
  }
  return `shell zh=en（${zk.size} 键，含 undo.* ${want.length} 键）`;
});

/* ──────────────────────────────────────────────────────────────────────────
 * P6 T04 · **独立验证**（运行时 + 键一致性）—— 另派验证者补（2026-10-06）
 *
 * 上面 T04 一组是**静态**断言（`showUndoToast(` 后 900 字符内含 `accountRestore(`）。
 * 静态断言只能证明「代码里写了」，**不能**证明「点了撤销真的调用」。本段补**运行时**证据：
 *   ① 真把 `showUndoToast` 跑起来（桩掉 `sonner`），取出 `action.onClick`，**真点一下**，
 *      断言 `undo` 回调被调用 —— 证明「点撤销 ≠ 只关 toast」；
 *   ② 连删 3 条 ⇒ 3 条独立 toast，创建时**不带固定 id**，点第 2 条不影响第 1 / 3 条；
 *   ③ 撤销失败（同步抛）⇒ `toast.error` + 复用同一 toast id（**不静默**）。
 *
 * 并补**键一致性**（用户「点了撤销却恢复不了」的根因）：删除用的 key 必须与恢复用的 key 一致 ——
 *   WB：删除 `a.id` / 恢复 `a.id`（后端 `resolve_account_key` = `id || uid`）；
 *   Trae：删除 `target.userId` / 恢复 `target.userId`（后端 `resolve_user_id`）；
 *   且两页的 `scope` 都由**同一个** `region` / `variant` 派生 ⇒ 删 / 恢落同一库。
 *
 * 变异（打在同一份实现文件上，逐条见 T04 独立验证报告）：
 *   ① `onClick` 不调 `input.undo()` ⇒ 独立①红；② 创建时传固定 id ⇒ 独立②红；
 *   ③ 失败吞异常 ⇒ 独立③红；④ 恢复键改成 `a.uid` / `account.id` ⇒ 独立④ / ⑤红；
 *   ⑤ 批删只恢复第一个（`keys[0]`）⇒ 独立⑥红；⑥ 撤销路径混入 purge ⇒ 独立⑦红。
 * ────────────────────────────────────────────────────────────────────────── */

/** 桩 sonner：记录每次 `toast.success / error` 的入参与返回 id（供运行时断言）。 */
function makeToastStub() {
  const calls = [];
  const nextId = () => `toast-${calls.length + 1}`;
  const toast = Object.assign(
    (...args) => {
      const id = nextId();
      calls.push({ fn: "default", args, id });
      return id;
    },
    {
      success: (title, options) => {
        const id = nextId();
        calls.push({ fn: "success", title, options, id });
        return id;
      },
      error: (title, options) => {
        const id = nextId();
        calls.push({ fn: "error", title, options, id });
        return id;
      },
    },
  );
  return { toast, calls };
}

/**
 * 用**自定义桩**加载 `undo-toast.ts`（只桩掉它那 3 个非本地导入）。
 *
 * 与文件顶部的 `loadTs` 分开：那里 `STUBS` 是**全局共享**的（给别的模块用），
 * 这里要注入**只属于本测试**的 sonner / api / i18n 桩，避免污染其它检查。
 */
function loadUndoToastRuntime(stubs) {
  const abs = path.join(ROOT, "src/lib/undo-toast.ts");
  const source = fs.readFileSync(abs, "utf8").replace(/import\.meta\.env/g, "__SCOPE_ENV__");
  const js = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  }).outputText;
  const mod = { exports: {} };
  const req = (spec) => {
    if (Object.prototype.hasOwnProperty.call(stubs, spec)) return stubs[spec];
    if (spec.startsWith("@/")) {
      const p = path.join(ROOT, "src", spec.slice(2));
      for (const cand of [`${p}.ts`, `${p}.tsx`]) if (fs.existsSync(cand)) return loadTs(cand);
    }
    return require(spec);
  };
  new Function("module", "exports", "require", "__SCOPE_ENV__", js)(mod, mod.exports, req, {});
  return mod.exports;
}

/** 每次测试都拿一份**全新**的桩 + 模块（避免跨用例串味）。 */
function undoToastHarness() {
  const { toast, calls } = makeToastStub();
  const mod = loadUndoToastRuntime({
    sonner: { toast },
    "@/lib/api": { asError: (e) => (e && e.message) || String(e) },
    "@/lib/i18n": { t: (k) => k },
  });
  return { mod, calls };
}

check("★ P6 T04 独立①（运行时）：点「撤销」真的调用 `undo`（不是只关 toast）", () => {
  /* 变异①：把 `onClick` 里的 `await input.undo()` 删掉（只 `event.preventDefault()`）⇒ 本条红。
     依据：静态断言只能看到 `accountRestore(` 的**文本**，看不到「点了会不会跑」。 */
  const { mod, calls } = undoToastHarness();
  let restored = 0;
  mod.showUndoToast({
    title: "已删除，可撤销",
    undoLabel: "撤销",
    undoneTitle: "已恢复",
    undo: () => {
      // 同步副作用：`onClick` 里 `input.undo()` 在首个 `await` 前被**同步**调用 ⇒ 可同步断言。
      restored += 1;
      return Promise.resolve();
    },
  });
  const created = calls.find((c) => c.fn === "success");
  if (!created) throw new Error("`showUndoToast` 未调用 `toast.success`");
  if (created.options.duration !== mod.UNDO_TOAST_DURATION_MS) {
    throw new Error(`撤销 toast 的 duration ≠ UNDO_TOAST_DURATION_MS（实为 ${created.options.duration}）`);
  }
  const action = created.options.action;
  if (!action || typeof action.onClick !== "function") {
    throw new Error("撤销 toast 未挂 `action.onClick`（没有可点的撤销）");
  }
  if (action.label !== "撤销") throw new Error(`撤销按钮文案未用 undoLabel：${action.label}`);
  action.onClick({ preventDefault() {} });
  if (restored !== 1) {
    throw new Error(`点「撤销」后 undo 调用次数 = ${restored}（应 1）—— 只关 toast 不恢复 = 假撤销`);
  }
  return "运行时：onClick ⇒ undo 被调用 1 次";
});

check("★ P6 T04 独立②（运行时）：连删 3 条 ⇒ 3 条独立 toast（创建不带固定 id，互不覆盖）", () => {
  /* 变异②：`showUndoToast` 里给 `toast.success` 传固定 `id: "undo"` ⇒ 第 2 次覆盖第 1 条 ⇒ 本条红。 */
  const { mod, calls } = undoToastHarness();
  const hits = [0, 0, 0];
  for (let i = 0; i < 3; i += 1) {
    mod.showUndoToast({
      title: `已删除 ${i}`,
      undoLabel: "撤销",
      undoneTitle: "已恢复",
      undo: () => {
        hits[i] += 1;
        return Promise.resolve();
      },
    });
  }
  const created = calls.filter((c) => c.fn === "success");
  if (created.length !== 3) throw new Error(`3 次删除应产生 3 条 toast，实际 ${created.length}`);
  for (const c of created) {
    if (c.options && Object.prototype.hasOwnProperty.call(c.options, "id")) {
      throw new Error("创建撤销 toast 时带固定 id —— 连续删多个会互相覆盖（撤销丢失）");
    }
  }
  created[1].options.action.onClick({ preventDefault() {} });
  if (hits[0] !== 0 || hits[1] !== 1 || hits[2] !== 0) {
    throw new Error(`点第 2 条撤销影响了其它条：hits = ${hits.join(",")}`);
  }
  return "3 条 toast · 无固定 id · 点第 2 条只触发第 2 条";
});

check("★ P6 T04 独立③（运行时）：撤销失败 ⇒ `toast.error` + 复用同一 toast id（不静默）", () => {
  /* 变异③：把 `catch` 里的 `toast.error(...)` 删掉（吞异常）⇒ 本条红。 */
  const { mod, calls } = undoToastHarness();
  mod.showUndoToast({
    title: "已删除",
    undoLabel: "撤销",
    undoneTitle: "已恢复",
    // 同步抛：与生产里 `await input.undo()` 的 rejection 走**同一个 catch 分支**。
    undo: () => {
      throw new Error("回收站中没有该账号");
    },
  });
  const created = calls.find((c) => c.fn === "success");
  if (!created) throw new Error("`showUndoToast` 未调用 `toast.success`");
  created.options.action.onClick({ preventDefault() {} });
  const failed = calls.find((c) => c.fn === "error");
  if (!failed) throw new Error("撤销失败未走 `toast.error` —— 静默失败，用户会以为恢复了");
  if (failed.options.id !== created.id) {
    throw new Error("失败反馈未就地复用同一 toast id（应把原 toast 变成 error）");
  }
  return "失败 ⇒ toast.error（复用 id，不静默）";
});

check("★ P6 T04 独立④：键一致性（WB）—— 删除键 `a.id` == 撤销键，scope 由同一 `region` 派生", () => {
  /* 变异④：把撤销键改成 `a.uid`（或删除键改成别的）⇒ 本条红。
     依据：删除与恢复必须传**同一个 key**，否则「点了撤销恢复不了」（真 bug）。 */
  const wb = readCode(WB_PAGE);
  if (!/deleteAccountStore\(a\.id,\s*region\)/.test(wb)) {
    throw new Error("WB 单删未用 `a.id` + `region`（键或库与撤销可能不一致）");
  }
  if (!/offerUndo\(\[a\.id\]/.test(wb)) {
    throw new Error("WB 单删的撤销键不是 `a.id` —— 与删除键不一致 ⇒ 恢复不了");
  }
  if (!/const scope = scopeOf\("workbuddy",\s*region\)/.test(wb)) {
    throw new Error("WB 的 `scope` 未由**同一个** `region` 派生 ⇒ 删 / 恢可能落不同库");
  }
  const block = sliceFrom(wb, "function offerUndo", 900);
  if (!/api\.accountRestore\(scope,\s*key\)/.test(block)) {
    throw new Error("WB 撤销未调 `api.accountRestore(scope, key)`");
  }
  /* 后端谓词：软删除 / 恢复**共用** `resolve_account_key`（`id || uid`）⇒ 传 `id` 两边都命中。 */
  const core = readCode("crates/buddy-switch-core/src/modules/account.rs");
  if (!/fn trash_account_for[\s\S]{0,500}resolve_account_key\(account,\s*key\)/.test(core)) {
    throw new Error("后端软删除未用 `resolve_account_key`（与恢复谓词可能不一致）");
  }
  if (!/fn restore_account_for[\s\S]{0,600}resolve_account_key\(&entry\.account,\s*key\)/.test(core)) {
    throw new Error("后端恢复未用 `resolve_account_key`（与软删除谓词可能不一致）");
  }
  return "WB：删 a.id / 恢 a.id · 同一 region · 后端共用 resolve_account_key";
});

check("★ P6 T04 独立⑤：键一致性（Trae）—— 删除键 `target.userId` == 撤销键，region 同源", () => {
  /* 变异④：把撤销键改成 `target.id`（Trae 账号**没有** id）⇒ 本条红。 */
  const trae = readCode(TRAE_PAGE);
  if (!/api\.traeDeleteAccount\(target\.userId,\s*false,\s*variant\)/.test(trae)) {
    throw new Error("Trae 单删未用 `target.userId` + `variant`");
  }
  if (!/offerUndo\(\[target\.userId\]/.test(trae)) {
    throw new Error("Trae 单删的撤销键不是 `target.userId` —— 与删除键不一致 ⇒ 恢复不了");
  }
  if (!/const scope = scopeOf\("trae",\s*variant\)/.test(trae)) {
    throw new Error("Trae 的 `scope` 未由**同一个** `variant` 派生 ⇒ 删 / 恢可能落不同区域");
  }
  const block = sliceFrom(trae, "function offerUndo", 900);
  if (!/api\.accountRestore\(scope,\s*key\)/.test(block)) {
    throw new Error("Trae 撤销未调 `api.accountRestore(scope, key)`");
  }
  /* 后端谓词：软删除 / 恢复**共用** `resolve_user_id`（Trae 的稳定键 = userId）。 */
  const core = readCode("crates/buddy-switch-core/src/modules/trae/account.rs");
  if (!/fn trash_for_region[\s\S]{0,500}resolve_user_id\(account\)\s*==\s*user_id/.test(core)) {
    throw new Error("Trae 软删除未用 `resolve_user_id == user_id`（与恢复谓词可能不一致）");
  }
  if (!/fn restore_for_region[\s\S]{0,500}resolve_user_id\(&entry\.account\)\s*==\s*user_id/.test(core)) {
    throw new Error("Trae 恢复未用 `resolve_user_id == user_id`（与软删除谓词可能不一致）");
  }
  /* 区域同源：删除把变体折叠成 `variant.region()`，恢复把 scope 解析成 region —— 两者必须同 region。 */
  const handlers = readCode("crates/buddy-switch-core/src/modules/trae/handlers.rs");
  if (!/fn delete_account_for[\s\S]{0,500}account::delete_for\(variant,\s*user_id,\s*delete_profile\)/.test(handlers)) {
    throw new Error("Trae 删除未走 `delete_for(variant, …)`（区域来源不明）");
  }
  if (!/fn delete_for[\s\S]{0,200}trash_for_region\(variant\.region\(\),\s*user_id\)/.test(core)) {
    throw new Error("Trae 删除未把变体折叠成 `variant.region()` —— 区域可能与恢复不一致");
  }
  return "Trae：删 userId / 恢 userId · 同一 variant→region · 后端共用 resolve_user_id";
});

check("★ P6 T04 独立⑥：批量删后撤销恢复**全部**（两页都遍历全部 keys，不是只恢复第一个）", () => {
  /* 变异⑤：把 `for (const key of keys)` 改成 `keys[0]`（只恢复第一个）⇒ 本条红。 */
  const wb = readCode(WB_PAGE);
  const trae = readCode(TRAE_PAGE);
  if (!/offerUndo\(\s*deleted\.map\(\(a\)\s*=>\s*a\.id\)/.test(wb)) {
    throw new Error("WB 批删未把所有被删账号的 `id` 交给 offerUndo");
  }
  if (!/offerUndo\(\s*deleted\.map\(\(account\)\s*=>\s*account\.userId\)/.test(trae)) {
    throw new Error("Trae 批删未把所有被删账号的 `userId` 交给 offerUndo");
  }
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const block = sliceFrom(readCode(rel), "function offerUndo", 900);
    if (!/for \(const key of keys\)/.test(block)) {
      throw new Error(`${path.basename(rel)} 撤销未遍历全部 keys（批删只恢复第一个 = 假承诺）`);
    }
  }
  return "两页批删撤销都遍历全部 keys（恢复全部）";
});

check("★ P6 T04 独立⑦：超时后账号**仍在回收站**（toast 时长 ≪ 30 天留存；撤销逻辑不触发任何删除）", () => {
  /* 变异⑥：让 `undo-toast.ts` 引用 `purge` / `trash_clear`（超时即永久删除）⇒ 本条红。
     依据（§1.4）：toast 的 `duration` 只决定**停留多久**，与「账号会不会被永久删除」无关 ——
     超时后账号**仍在回收站**（后端 30 天留存），可随时恢复。 */
  const { mod, calls } = undoToastHarness();
  mod.showUndoToast({ title: "T", undoLabel: "U", undoneTitle: "D", undo: () => Promise.resolve() });
  const created = calls.find((c) => c.fn === "success");
  const duration = created.options.duration;
  if (duration !== mod.UNDO_TOAST_DURATION_MS) {
    throw new Error(`toast duration ≠ UNDO_TOAST_DURATION_MS（${duration}）`);
  }
  // ① 撤销路径**不得**含删除 / 永久删除 / 清空回收站（超时只让 toast 消失，不删账号）。
  const undoCode = readCode(UNDO_TOAST);
  for (const bad of ["accountPurge", "account_purge", "purge", "trashClear", "trash_clear", "removeAccount"]) {
    if (new RegExp(`\\b${bad}\\b`).test(undoCode)) {
      throw new Error(`\`undo-toast.ts\` 出现「${bad}」—— 撤销路径不应含删除 / 永久删除`);
    }
  }
  // ② 后端留存窗口 = 30 天；toast 时长（8s）远小于它 ⇒ 超时后账号仍在。
  const RETENTION_MS = 30 * 24 * 60 * 60 * 1000;
  if (!(duration < RETENTION_MS)) {
    throw new Error(`toast 时长（${duration}ms）不小于后端留存（${RETENTION_MS}ms）—— 超时可能已过期`);
  }
  for (const rel of [
    "crates/buddy-switch-core/src/modules/account.rs",
    "crates/buddy-switch-core/src/modules/trae/account.rs",
  ]) {
    const core = readCode(rel);
    if (!/TRASH_RETENTION_MS:\s*i64\s*=\s*30\s*\*\s*24\s*\*\s*60\s*\*\s*60\s*\*\s*1000/.test(core)) {
      throw new Error(`${rel} 的 TRASH_RETENTION_MS 不是 30 天`);
    }
    if (!/entries\.retain\(\|entry\| entry\.expires_at > now\)/.test(core)) {
      throw new Error(`${rel} 的留存清理条件不是「expires_at > now」（过期才清）`);
    }
  }
  return `toast ${duration}ms ≪ 留存 30 天 · 撤销路径无 purge · 过期才清`;
});

check("★ P6 T04 独立⑧（运行时）：连点「撤销」两次 ⇒ 恢复只跑一次（防重入，不重复打后端）", () => {
  /* 变异：删掉 `if (running) return;` 重入护栏 ⇒ 连点两次会把后端打两次 ⇒ 本条红。
     为什么重要：后端 `restore_account_for` 第二次会 `Err`（回收站已没有该账号）⇒ 若重入，
     用户会先看到「已恢复」再看到「撤销失败」，自相矛盾。 */
  const { mod, calls } = undoToastHarness();
  let restored = 0;
  mod.showUndoToast({
    title: "已删除",
    undoLabel: "撤销",
    undoneTitle: "已恢复",
    undo: () => {
      restored += 1;
      return Promise.resolve();
    },
  });
  const created = calls.find((c) => c.fn === "success");
  const onClick = created.options.action.onClick;
  onClick({ preventDefault() {} });
  onClick({ preventDefault() {} }); // 连点第二次（仍在同一次同步执行内 ⇒ finally 尚未复位 running）
  if (restored !== 1) {
    throw new Error(`连点两次「撤销」后 undo 调用次数 = ${restored}（应 1）—— 重入会重复打后端`);
  }
  return "连点两次 ⇒ undo 只跑一次";
});

check("★ P6 动作扩展点：collectPaletteActions 含基础动作；重复注册按 id 去重（★ P7 接口）", () => {
  const ids = paletteActionsLib.collectPaletteActions().map((a) => a.id);
  for (const want of ["refresh-scope", "add-account", "open-data-dir"]) {
    if (!ids.includes(want)) throw new Error(`基础动作缺 ${want}`);
  }
  paletteActionsLib.registerPaletteActions([
    { id: "refresh-scope", titleKey: "palette.action.refresh", run: () => {} },
  ]);
  if (paletteActionsLib.collectPaletteActions().filter((a) => a.id === "refresh-scope").length !== 1) {
    throw new Error("重复注册未按 id 去重");
  }
  paletteActionsLib.registerPaletteActions([
    { id: "p7-export", titleKey: "palette.action.refresh", run: () => {} },
  ]);
  if (!paletteActionsLib.collectPaletteActions().some((a) => a.id === "p7-export")) {
    throw new Error("注册新动作失败 —— P7 的扩展点不通");
  }
  if (!/collectPaletteActions\(/.test(readCode(PALETTE_COMPONENT))) {
    throw new Error("面板没有合并 `collectPaletteActions()` —— P7 的动作进不来");
  }
  return `${ids.length} 个基础动作 · 去重 · 新动作可注册 · 面板已合并`;
});

check("★ P6 快捷键偏好：默认开、写关读关、写开读开（storage 不可用也不崩）", () => {
  if (shortcutPrefLib.readShortcutsEnabled(undefined) !== true) throw new Error("缺失存储应默认开启");
  const mem = memStorage();
  shortcutPrefLib.writeShortcutsEnabled(mem, false);
  if (shortcutPrefLib.readShortcutsEnabled(mem) !== false) throw new Error("写关后读回应为 false");
  shortcutPrefLib.writeShortcutsEnabled(mem, true);
  if (shortcutPrefLib.readShortcutsEnabled(mem) !== true) throw new Error("写开后读回应为 true");
  const boom = {
    getItem: () => {
      throw new Error("disabled");
    },
    setItem: () => {
      throw new Error("disabled");
    },
  };
  if (shortcutPrefLib.readShortcutsEnabled(boom) !== true) throw new Error("存储抛异常应吞掉并默认开");
  shortcutPrefLib.writeShortcutsEnabled(boom, false); // 不应抛
  return "默认开 / 写关读关 / 写开读开 / 异常安全";
});

check("★ issue #49 抽屉页签记忆：每账号一键 · 记住的不可用则回落第一枚 · 回落不写盘", () => {
  /* 需求 R1–R5（`src/lib/drawer-tab-preference.ts`）：关闭抽屉再打开同一账号要回到上次的页签。
     可证伪点：
     ① 键里**必须带作用域**（同一 uid 在国内版 / 国际版是两个账号）⇒ 两个账号各记各的；
     ② 记住的页签当前不可用（如没有资源包）⇒ 回落**渲染顺序第一枚**；
     ③ 回落**不改写**记忆（资源包回来后仍应回到「积分」）；
     ④ 存储不可用 / 值非法 ⇒ 当没记住（不崩、不假装）。 */
  const mem = memStorage();
  if (drawerTabLib.readDrawerTab(mem, "wb-cn:1") !== null) {
    throw new Error("没写过的账号应读到 null（不假装记住）");
  }
  drawerTabLib.writeDrawerTab(mem, "wb-cn:1", "credits");
  drawerTabLib.writeDrawerTab(mem, "wb-cn:2", "checkin");
  if (drawerTabLib.readDrawerTab(mem, "wb-cn:1") !== "credits") throw new Error("账号 1 应记住 credits");
  if (drawerTabLib.readDrawerTab(mem, "wb-cn:2") !== "checkin") throw new Error("账号 2 应记住 checkin");
  // 键形态：前缀 + `${scope}:${id}`（版本化前缀，语义若变可换键）
  if (mem.getItem("buddy-switch:v1:drawer-tab:wb-cn:1") !== "credits") {
    throw new Error("键必须是 `buddy-switch:v1:drawer-tab:<scope>:<id>`");
  }
  // 非法值 / 空账号键 ⇒ null
  mem.setItem(drawerTabLib.drawerTabKey("wb-cn:9"), "bogus");
  if (drawerTabLib.readDrawerTab(mem, "wb-cn:9") !== null) throw new Error("非法值应读到 null");
  if (drawerTabLib.readDrawerTab(mem, "") !== null) throw new Error("空账号键应读到 null");
  // R3：记住的可用 ⇒ 用它；不可用 ⇒ 第一枚可用
  if (drawerTabLib.resolveDrawerTab("credits", ["overview", "credits"]) !== "credits") {
    throw new Error("记住的页签可用时应原样使用");
  }
  if (drawerTabLib.resolveDrawerTab("credits", ["overview", "checkin"]) !== "overview") {
    throw new Error("记住的页签不可用时应回落渲染顺序第一枚");
  }
  if (drawerTabLib.resolveDrawerTab(null, ["overview", "checkin"]) !== "overview") {
    throw new Error("没有记忆时应落到第一枚可用页签");
  }
  if (drawerTabLib.resolveDrawerTab("snapshot", []) !== drawerTabLib.DEFAULT_DRAWER_TAB) {
    throw new Error("没有可用页签时应落默认值");
  }
  // R4：回落**不擦除**记忆
  const before = mem.getItem(drawerTabLib.drawerTabKey("wb-cn:1"));
  drawerTabLib.resolveDrawerTab("credits", ["overview"]);
  if (mem.getItem(drawerTabLib.drawerTabKey("wb-cn:1")) !== before) {
    throw new Error("回落只决定「这次显示哪一枚」，不得改写记忆");
  }
  // 存储抛异常：读 ⇒ null，写 ⇒ 不抛
  const boom = {
    getItem: () => {
      throw new Error("disabled");
    },
    setItem: () => {
      throw new Error("disabled");
    },
  };
  if (drawerTabLib.readDrawerTab(boom, "wb-cn:1") !== null) throw new Error("存储异常应读成 null");
  drawerTabLib.writeDrawerTab(boom, "wb-cn:1", "credits");
  // 三个调用点都必须传身份键（漏传在 tsc 阶段也会红 —— 属性是必填的）
  for (const page of ["AccountsPage.tsx", "TraeAccountsPage.tsx", "QoderAccountsPage.tsx"]) {
    const code = readCode(`src/pages/${page}`);
    if (!/accountKey=\{detail \?/.test(code)) {
      throw new Error(`${page} 没给抽屉传 \`accountKey\` —— 页签记忆会退化成「不记忆」`);
    }
  }
  return "每账号一键 · 可用则用 / 不可用回落 · 回落不写盘 · 异常安全 · 三处调用点已传键";
});

check("★ P6 首次引导：默认显示、关闭后持久化不再显示", () => {
  const mem = memStorage();
  if (firstRunLib.shouldShowFirstRunHint(mem) !== true) throw new Error("首次应显示");
  firstRunLib.dismissFirstRunHint(mem);
  if (firstRunLib.shouldShowFirstRunHint(mem) !== false) throw new Error("关闭后不应再显示");
  if (firstRunLib.shouldShowFirstRunHint(undefined) !== true) throw new Error("无存储时应显示（宁可多显示一次）");
  return "首启显示 / 关闭持久化 / 无存储显示";
});

check("★ P6 引导内容由真实绑定派生（降级项不写进引导，防「引导里有、按了没反应」）", () => {
  const code = readCode(FIRST_RUN_COMPONENT);
  if (!/planScopeShortcuts\(/.test(code)) throw new Error("引导没有用 `planScopeShortcuts` 派生内容");
  if (!/bindings\.size\s*>\s*0/.test(code)) {
    throw new Error("引导没有按 `bindings.size > 0` 决定是否列出 ⌘1..N");
  }
  if (!/shouldShowFirstRunHint|dismissFirstRunHint/.test(code)) throw new Error("引导没有接持久化");
  return "引导走 planScopeShortcuts + bindings.size 判据 + 持久化";
});

check("★ P6 设置项：reason=too-many 时不显示快捷键开关（无可开启之物，防假控件）", () => {
  const code = readCode(APP_SETTINGS);
  if (!/planScopeShortcuts\(/.test(code)) throw new Error("设置项没有用 `planScopeShortcuts`");
  if (!/too-many/.test(code)) throw new Error("设置项没有按 `too-many` 降级");
  if (!/useShortcutsEnabled\(/.test(code)) throw new Error("设置项没有接 `useShortcutsEnabled`");
  return "设置项按 planScopeShortcuts 的 too-many 降级";
});

check("★ P6 `?q=` 一次性种子：两账号页读一次并 replace 清参（视图参数留页内）", () => {
  for (const rel of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(rel);
    if (!/useSearchParams\(/.test(code)) throw new Error(`${path.basename(rel)} 没有读 URL 查询参数`);
    if (!/get\("q"\)/.test(code)) throw new Error(`${path.basename(rel)} 没有读 ?q=`);
    if (!/delete\("q"\)/.test(code)) throw new Error(`${path.basename(rel)} 没有清掉 ?q=（一次性种子）`);
    if (!/replace:\s*true/.test(code)) throw new Error(`${path.basename(rel)} 清参没有用 replace`);
    if (!/seedAppliedRef/.test(code)) throw new Error(`${path.basename(rel)} 没有「只读一次」的护栏`);
  }
  return "两页都读一次 ?q= 并 replace 清参";
});

check("★ P6 面板接线：App 挂载面板 + 首次引导条 + 侧栏入口按钮 + ⌘K/⌘1..N（入口是按钮不是切换器）", () => {
  const app = readCode(APP_TSX);
  if (!/<CommandPalette\b/.test(app)) throw new Error("没有挂载 <CommandPalette>");
  if (!/<FirstRunHint\b/.test(app)) throw new Error("没有挂载 <FirstRunHint>");
  if (!/palette\.entry/.test(app)) throw new Error("侧栏没有「搜索或跳转」入口按钮");
  if (!/event\.key\.toLowerCase\(\)\s*!==\s*"k"/.test(app)) throw new Error("没有 ⌘K 全局键");
  if (!/\/\^\[1-9\]\$\//.test(app)) throw new Error("没有 ⌘1..N 全局键");
  if (!/scopeShortcutTarget\(/.test(app)) throw new Error("⌘1..N 没有用 `scopeShortcutTarget`");
  if (!/planScopeShortcuts\(/.test(app)) throw new Error("App 没有用 `planScopeShortcuts` 规划快捷键");
  return "挂载面板 + 引导 + 入口 + ⌘K + ⌘1..N";
});

/* ── P6 接线级验证（T05 独立验证补强）──────────────────────────────────────────
 *
 * 上面的 P6 断言多数锚在**纯函数**（`planScopeShortcuts` / `buildPaletteItems`）上；
 * 但「降级真的降级」「输入框真的豁免」「空态真的四要素」都是**接线**行为 ——
 * 纯函数对、接线漏了照样是缺陷。下面几条锚在**接线**上。
 */

check("★ P6 降级接线：App 的 ⌘1..N 在未绑定（降级）时**不导航**（wiring，非仅纯函数）", () => {
  /* 变异：删掉 `if (!target) return;`（降级仍 navigate）⇒ 本条变红。
     现有断言只测纯函数「>5 ⇒ 空绑定」，但**接线**若不检查 target 仍会导航 = 假控件。 */
  const app = readCode(APP_TSX);
  if (!/if \(!target\) return;[\s\S]{0,240}navigate\(scopeShortcutTarget\(/.test(app)) {
    throw new Error("⌘1..N 未在 target 缺失时提前 return —— 降级仍可能导航（假控件）");
  }
  if (!/for \(const \[scope, bound\] of shortcutPlan\.bindings\)/.test(app)) {
    throw new Error("⌘1..N 未从 `shortcutPlan.bindings` 取绑定 —— 可能与降级判据脱钩");
  }
  return "未绑定 ⇒ return（不 navigate）· 绑定取自 plan.bindings";
});

check("★ P6 输入框豁免：⌘K 与 ⌘1..N **两键**在输入态都不触发（§1.10）", () => {
  /* 变异：任一全局键漏掉 `isTypingTarget` 豁免 ⇒ 打字时被劫持（如按数字跳走）⇒ 变红。 */
  const app = readCode(APP_TSX);
  const hits = (app.match(/isTypingTarget\(event\.target\)/g) || []).length;
  if (hits < 2) {
    throw new Error(`输入框豁免只出现在 ${hits} 处 —— ⌘K 与 ⌘1..N 应各自豁免`);
  }
  if (!/tag === "INPUT" \|\| tag === "TEXTAREA" \|\| el\?\.isContentEditable === true/.test(app)) {
    throw new Error("输入框判据不完整（应含 INPUT / TEXTAREA / isContentEditable）");
  }
  return `${hits} 处豁免 · 判据含 INPUT/TEXTAREA/isContentEditable`;
});

check("★ P6 面板禁用 cmdk 自带过滤（shouldFilter=false）—— 否则被另一套算法二次过滤", () => {
  /* 变异：去掉 `shouldFilter={false}` ⇒ cmdk 用自己的匹配再过滤一遍 ⇒ 与列表搜索两套行为。 */
  const code = readCode(PALETTE_COMPONENT);
  if (!/shouldFilter=\{false\}/.test(code)) {
    throw new Error("面板未关掉 cmdk 自带过滤 —— 会与 account-search 形成两套匹配");
  }
  if (!/filterPaletteItems\(/.test(code)) throw new Error("面板没有用 `filterPaletteItems`");
  return "shouldFilter={false} + filterPaletteItems";
});

check("★ P6 边界：空账号列表 ⇒ buildPaletteItems 不产账号条目、不崩溃（空组不渲染）", () => {
  /* 变异：buildPaletteItems 对空 accounts 抛错 / 产空账号组 ⇒ 变红。 */
  const items = paletteLib.buildPaletteItems(paletteCtx({ accounts: [] }));
  const accountItems = items.filter((i) => i.group === "account");
  if (accountItems.length !== 0) {
    throw new Error(`空账号应 0 条账号条目，实际 ${accountItems.length}`);
  }
  // 其余组仍在（面板不是整块空 —— 仍可导航 / 执行动作）
  if (items.filter((i) => i.group === "scope").length !== REAL_SCOPES.length) {
    throw new Error("空账号不应影响作用域组");
  }
  if (!/if \(groupItems\.length === 0\) return null;/.test(readCode(PALETTE_COMPONENT))) {
    throw new Error("面板未跳过空组（会渲染一个空标题组）");
  }
  return "空账号 ⇒ 0 账号条目 · 其余组保留 · 空组不渲染";
});

check("★ P6 边界：无匹配 ⇒ 结构化空态（四要素含出口，非一行虚线框）", () => {
  /* 变异：把 <EmptyState> 换成一行文字 / 去掉 action（出口）⇒ 变红（§3.6 状态语言）。 */
  const code = readCode(PALETTE_COMPONENT);
  if (!/filtered\.length === 0/.test(code)) throw new Error("缺「无结果」判据");
  const at = code.indexOf("<EmptyState");
  if (at < 0) throw new Error("面板无结果时缺 <EmptyState>");
  const block = code.slice(at, at + 600);
  for (const attr of ["icon=", "title=", "description=", "action="]) {
    if (!block.includes(attr)) throw new Error(`<EmptyState> 缺四要素之一：${attr}`);
  }
  return "EmptyState 含 icon/title/description/action（有出口）";
});

/* ── P9 桌面悬浮窗（独立窗口 + capability + 托盘入口 + 懒创建）────────────────────
 *
 * 上面 66 条覆盖 P0–P11 + P6；P9 此前**一条都没有** ⇒「门禁全绿」证明不了悬浮窗是
 * **独立窗口且懒创建**、capability 登记了 label、位置做了**落屏校验**。下面 7 条锁住
 * P9 T01 最易回退的判据：懒创建（**不进 `tauri.conf.json` 的 `windows[]`**）/ capability
 * 登记 label + 拖拽权限 / 位置记忆 + 落屏校验 / URL 入口分流 / 托盘 CheckMenuItem /
 * 词条 zh+en 同步 / 只读红线 + 不新增后台循环。
 *
 * ⚠️ 三条纪律（与上面各段一致）：
 *   ① 断言**只锚在代码上**（`readCode` 先剥注释）—— 锚在注释上会被「注释里写着反例」骗过去；
 *   ② JSON 配置用 `JSON.parse` 读（**不能剥注释**：`http://` 会被 `//` 规则误伤）；
 *   ③ 「懒创建」是**设计决策**，不固化就会被改回「写进 `windows[]`」⇒ 第 1 条专门守它。
 *
 * ⚠️ **两套验收，各验各的对象**：本文件验**真实应用**（`src/` + `src-tauri/`）；
 *    原型套件 `docs/prototype/verify-behaviors.cjs` 验**原型**，**不动**。
 */

const COMPANION_RS = "src-tauri/src/companion.rs";
const COMPANION_CAP = "src-tauri/capabilities/companion.json";
const COMPANION_APP = "src/components/companion/companion-app.tsx";
const COMPANION_LIB_TS = "src/lib/companion.ts";
const COMMANDS_RS = "src-tauri/src/commands.rs";
const TAURI_CONF = "src-tauri/tauri.conf.json";
const MAIN_TSX = "src/main.tsx";
const TRAY_RS = "src-tauri/src/tray.rs";

/** 读 JSON 配置（**不剥注释** —— `http://` 会被 `//` 规则误伤）。 */
function readJson(rel) {
  return JSON.parse(fs.readFileSync(path.join(ROOT, rel), "utf8"));
}

/**
 * 从（已剥注释的）Rust 源码里取某个 `fn` 的**函数体**（大括号配对）。
 *
 * ## 为什么需要它（只读红线必须**收窄**到 companion 命令体）
 *
 * `commands.rs` 里**合法**地存在 `switch_account` 等写命令 —— 若拿整份文件去扫
 * 「有没有写操作」，立刻假红；而一条会假红的护栏迟早被绕过（比没有护栏更坏）。
 * 因此把扫描锚在 **companion 命令体**上：只有「给 companion 命令塞写操作」才变红
 * （T05 变异①正是这种注入）。返回 `null` 表示找不到该 fn —— 由调用方**报错**，
 * 而不是静默跳过（静默跳过会让护栏失效）。
 */
function rustFnBody(source, fnName) {
  const re = new RegExp(`(?:pub\\s+)?(?:async\\s+)?fn\\s+${fnName}\\s*(?:<[^>]*>)?\\s*\\(`);
  const m = re.exec(source);
  if (!m) return null;
  // 跳过形参表（小括号配对）。
  let depth = 0;
  let i = m.index + m[0].length - 1;
  for (; i < source.length; i += 1) {
    if (source[i] === "(") depth += 1;
    else if (source[i] === ")") {
      depth -= 1;
      if (depth === 0) break;
    }
  }
  const braceOpen = source.indexOf("{", i);
  if (braceOpen < 0) return null;
  depth = 0;
  for (let j = braceOpen; j < source.length; j += 1) {
    if (source[j] === "{") depth += 1;
    else if (source[j] === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(braceOpen + 1, j);
    }
  }
  return null;
}

check("★ P9 悬浮窗**懒创建**：不进 `tauri.conf.json` 的 `windows[]`（写进去即变红）", () => {
  /* 设计决策（P9 §1.2）：写进 `windows[]` 会在**启动时无条件创建**，与「默认是否显示由
     用户偏好决定」冲突；且 transparent / always_on_top / skip_taskbar 是悬浮窗专属属性，
     对主窗口不适用。故 companion **必须**在 `companion.rs` 的 `ensure()` 里懒创建。
     变异②：把 companion 加进 `tauri.conf.json` 的 `windows[]` ⇒ 本条变红。 */
  const conf = readJson(TAURI_CONF);
  const labels = (conf.app?.windows ?? []).map((w) => w.label);
  if (!labels.includes("main")) {
    throw new Error(`tauri.conf.json 的 windows[] 缺少 main（实际 [${labels.join(",")}]）—— 断言失效`);
  }
  if (labels.includes("companion")) {
    throw new Error(
      "companion 被写进 `tauri.conf.json` 的 `windows[]` —— 会在启动时无条件创建（应懒创建）",
    );
  }
  const rs = readCode(COMPANION_RS);
  if (!/WebviewWindowBuilder::new\(/.test(rs)) {
    throw new Error("companion.rs 没有用 `WebviewWindowBuilder::new` 懒创建");
  }
  if (!/pub fn ensure\b/.test(rs)) throw new Error("companion.rs 缺少 `ensure()`（懒创建入口）");
  return `windows[]=[${labels.join(",")}] · 懒创建走 ensure()`;
});

check("★ P9 capability 登记 companion label + 拖拽权限（删 label ⇒ 权限静默失败）", () => {
  /* P9 设计 R2：新窗口若没有对应 capability，拖拽 / 事件监听会**静默失败**（不报错，只是不生效）。 */
  const cap = readJson(COMPANION_CAP);
  if (cap.identifier !== "companion") {
    throw new Error(`identifier 应为 companion，实际 ${cap.identifier}`);
  }
  if (!(cap.windows ?? []).includes("companion")) {
    throw new Error("capability 的 windows 未登记 companion —— 新窗口没有 core 权限，拖拽 / 事件监听会静默失败");
  }
  if (!(cap.permissions ?? []).includes("core:window:allow-start-dragging")) {
    throw new Error("capability 未授权 core:window:allow-start-dragging —— data-tauri-drag-region 拖不动");
  }
  if (!/COMPANION_LABEL: &str = "companion"/.test(readCode(COMPANION_RS))) {
    throw new Error("companion.rs 的 COMPANION_LABEL 与 capability 登记的 label 不一致");
  }
  return "identifier/windows/start-dragging/label 一致";
});

check("★ P9 位置记忆 + **落屏校验**（越界回落右下角；删校验即变红）", () => {
  /* P9 设计 R4：拔掉外接屏后记忆坐标可能落在屏幕外 ⇒ 必须落屏校验、越界回落默认右下角。
     变异①：删掉落屏校验（无条件用记忆位置）⇒ 本条变红（Rust 侧同名单测也会变红）。 */
  const rs = readCode(COMPANION_RS);
  if (!/WindowEvent::Moved/.test(rs)) throw new Error("companion.rs 没有监听 `WindowEvent::Moved`（位置记忆）");
  if (!/available_monitors\(/.test(rs)) throw new Error("companion.rs 没有读显示器列表（落屏校验需要）");
  if (!/primary_monitor\(/.test(rs)) throw new Error("companion.rs 没有读主显示器（默认右下角的基准）");
  if (!/\.any\(\|monitor\| monitor\.contains\(/.test(rs)) {
    throw new Error(
      "companion.rs 缺少落屏校验（`monitors.iter().any(|m| m.contains(x, y))`）—— 拔屏后窗口会落在屏幕外",
    );
  }
  if (!/bottom_right_anchor\(/.test(rs)) throw new Error("companion.rs 缺少右下角回落（默认位置）");
  return "Moved 记忆 · available/primary_monitor · any(contains) 落屏校验 · 右下角回落";
});

check("★ P9 入口分流：`main.tsx` 按 `?window=companion` 渲染 CompanionApp（webui 安全）", () => {
  /* ⚠️ 不能用 `getCurrentWindow().label` 分流：`main.tsx` 在 webui 也执行，那里没有 Tauri
     窗口 API ⇒ 引入 `@tauri-apps/api` 会让 webui 崩。URL 查询两端都安全。 */
  const code = readCode(MAIN_TSX);
  if (!/get\("window"\)\s*===\s*"companion"/.test(code)) {
    throw new Error("main.tsx 没有按 `?window=companion` 分流");
  }
  if (!/<CompanionApp\b/.test(code)) throw new Error("main.tsx 没有渲染 `<CompanionApp />`");
  if (!/new URLSearchParams\(window\.location\.search\)/.test(code)) {
    throw new Error("main.tsx 没有用 URLSearchParams 读查询串");
  }
  if (/getCurrentWindow/.test(code)) {
    throw new Error("main.tsx 用了 `getCurrentWindow` —— webui 无窗口 API，会崩");
  }
  return "?window=companion → CompanionApp · 无 getCurrentWindow";
});

check("★ P9 托盘入口：`CheckMenuItem` + `toggle-companion` 分支（勾选态读真实可见性）", () => {
  const tray = readCode(TRAY_RS);
  if (!/"toggle-companion"/.test(tray)) throw new Error("tray.rs 没有 `toggle-companion` 菜单 id");
  if (!/CheckMenuItem::with_id\([\s\S]{0,200}?"toggle-companion"/.test(tray)) {
    throw new Error("托盘项不是 CheckMenuItem（勾选态 = 可见性，与 lightweight-mode 同款）");
  }
  if (!/companion::toggle\(/.test(tray)) throw new Error("tray.rs 没有调 `companion::toggle`");
  if (!/companion::is_visible\(/.test(tray)) {
    throw new Error("托盘勾选态没有读 `companion::is_visible`（会与真实状态漂移）");
  }
  return "CheckMenuItem · toggle-companion · 读 is_visible";
});

check("★ P9 词条 zh + en 同步（`shared.companion.title`，且骨架真的引用）", () => {
  const zh = localeValue(SHARED_LOCALE_ZH, "shared.companion.title");
  const en = localeValue(SHARED_LOCALE_EN, "shared.companion.title");
  if (!zh || !en) throw new Error("shared.companion.title 在 zh / en 至少一侧为空");
  if (!/会话/.test(zh)) throw new Error(`中文标题不含「会话」：${zh}`);
  if (!/Session/i.test(en)) throw new Error(`英文标题不含 Session：${en}`);
  if (!/shared\.companion\.title/.test(readCode(COMPANION_APP))) {
    throw new Error("companion-app.tsx 没有引用 `shared.companion.title`（死键）");
  }
  return `zh=「${zh}」· en=「${en}」· 骨架已引用`;
});

check("★ P9 只读红线 + 不新增后台循环（companion 命令面无写操作 / 无 spawn；刷新在前端）", () => {
  /* §3.16「不做的」：悬浮窗只读 —— 命令面只有读 / 显隐 / 聚焦。
     ① companion 源码 **+ T02 命令面**（`commands.rs` 的 `companion_*`）都不许出现写操作入口
        （切换账号 / 删除 / 批量 / 禁用模型 / 导出 / 置顶）—— 变异①（给 `companion_jump` 真切账号）
        打在这里 ⇒ 变红；
     ② **不新增 Rust 后台循环**（刷新走前端 webview 轮询 + 既有事件）—— 出现 `spawn(` 即越界，
        否则会触发「登记即执行」硬约束（漏登记 ⇒ 任务永不执行）；
     ③ **正向**：`src/lib/companion.ts` 必须**在前端**轮询（`setInterval`）+ 订阅既有事件 ——
        把 20s 轮询改成 Rust 后台循环（变异②）会让 ② 与 ③ 同时红。 */
  const FORBIDDEN = [
    "switch_account",
    "account_delete",
    "batch-delete",
    "batch_checkin",
    "set_model_disabled",
    "export_backup",
    "toggle_pin",
  ];
  // `spawn_blocking` 是**一次性**任务，不是后台循环 —— 故要求 `spawn` 后紧跟 `(`。
  const BACKGROUND_LOOP =
    /tauri::async_runtime::spawn\s*\(|tokio::spawn\s*\(|std::thread::spawn\s*\(/;

  const rs = readCode(COMPANION_RS);
  const app = readCode(COMPANION_APP);
  const commands = readCode(COMMANDS_RS);
  const lib = readCode(COMPANION_LIB_TS);

  for (const [name, code] of [
    ["companion.rs", rs],
    ["companion-app.tsx", app],
  ]) {
    for (const bad of FORBIDDEN) {
      if (code.includes(bad)) throw new Error(`${name} 出现写操作痕迹：${bad}（悬浮窗只读）`);
    }
  }
  if (BACKGROUND_LOOP.test(rs)) {
    throw new Error(
      "companion.rs 起了后台循环 —— 刷新应走前端轮询 + 既有事件（不碰 BackgroundTask 注册表）",
    );
  }

  // ★ T02：命令面（`commands.rs` 的 companion 命令 + provider）也必须只读、不得起后台循环。
  //    扫描**收窄到函数体** —— 整份 `commands.rs` 里合法的 `switch_account` 会立刻假红。
  const COMMAND_FNS = [
    "companion_status",
    "companion_set_visible",
    "companion_toggle",
    "companion_jump",
    "companion_snapshot",
  ];
  for (const fn of COMMAND_FNS) {
    const body = rustFnBody(commands, fn);
    if (body === null) {
      throw new Error(`commands.rs 找不到 \`${fn}\` —— T02 命令面缺失（只读红线无从校验）`);
    }
    for (const bad of FORBIDDEN) {
      if (body.includes(bad)) {
        throw new Error(`commands.rs 的 \`${fn}\` 出现写操作：${bad}（悬浮窗只读）`);
      }
    }
    if (BACKGROUND_LOOP.test(body)) {
      throw new Error(
        `commands.rs 的 \`${fn}\` 起了后台循环 —— 刷新应在前端 webview（不碰 BackgroundTask 注册表）`,
      );
    }
  }

  // ★ 正向：刷新必须**在前端 webview**。
  if (!/setInterval\s*\(/.test(lib)) {
    throw new Error(
      "src/lib/companion.ts 没有 `setInterval` —— 刷新应在前端 webview 轮询（不新增 Rust 后台循环）",
    );
  }
  if (!/COMPANION_POLL_INTERVAL_MS\s*=\s*20_000/.test(lib)) {
    throw new Error("companion.ts 缺 20s 轮询间隔常量（`COMPANION_POLL_INTERVAL_MS = 20_000`）");
  }
  for (const event of ["switch-progress", "checkin-progress", "trae-checkin-progress"]) {
    if (!lib.includes(event)) {
      throw new Error(`companion.ts 没有订阅既有事件 \`${event}\`（事件驱动刷新缺失）`);
    }
  }

  return "命令面只读 · 无后台循环 · 前端 setInterval 轮询 + 既有事件";
});

/* ─────────────────────────── P9 · T03 悬浮窗 UI ───────────────────────────
 *
 * 上面 7 条锁 P9 T01/T02；下面 4 条锁 T03 最易回退的判据：
 *   ★ 数据防线（完整标题**默认不进 DOM**）/ 四态可区分 / 只读（UI 层无写操作）/
 *     状态语言（Skeleton + EmptyState 含出口）。
 *
 * 变异验证锚（每条都必须能因**同一份 `src/` 实现文件**的改动而变红）：
 *   ① 标题改成「未悬停也直接渲染完整标题」            ⇒ 「数据防线」条变红；
 *   ② 四态里任一态不可区分（如 `failed` 与 `done` 用同一 class）⇒ 「四态可区分」条变红；
 *   ③ 给 UI 加一个写操作（如点击直接切账号）          ⇒ 「只读」条变红。
 */

const COMPANION_ROW = "src/components/companion/companion-row.tsx";

/** 悬浮窗 UI 层**禁出现**的写操作痕迹（前后端两种命名都收 —— UI 里通常是 camelCase）。 */
const COMPANION_UI_FORBIDDEN = [
  "switch_account",
  "switchAccount",
  "account_delete",
  "deleteAccount",
  "batch-delete",
  "batchDelete",
  "batch_checkin",
  "batchCheckin",
  "set_model_disabled",
  "setModelDisabled",
  "export_backup",
  "exportBackup",
  "toggle_pin",
  "togglePin",
];

check("★ P9 T03 数据防线：未悬停 / 未聚焦渲染占位符，完整标题**不进 DOM**（改回直渲即变红）", () => {
  /* ★ 决策 #3：完整标题**根本不进 DOM**（不是只做视觉 blur）。变异①把标题改回
     「无条件渲染完整标题」⇒ 本条变红。 */
  const row = readCode(COMPANION_ROW);
  // 占位符常量必须存在且是「遮罩」形态（空串等于没占位）。
  if (!/COMPANION_TITLE_PLACEHOLDER\s*=\s*"[^"]+"/.test(row)) {
    throw new Error("companion-row.tsx 缺少标题占位符常量 —— 完整标题会直接进 DOM");
  }
  // ★ 完整标题**只能**出现在 `revealed ?` 三元里：未展开时求值的是占位符。
  if (!/revealed\s*\?\s*displayText\(item\.title\)/.test(row)) {
    throw new Error(
      "完整标题没有挂在 `revealed ? displayText(item.title)` 上 —— 未悬停 / 未聚焦时会泄露明文",
    );
  }
  // 反向：不得存在第二条把 `item.title` 交给 `displayText` 的路径（除三元那一处）。
  const titleRefs = row.match(/displayText\(item\.title\)/g) || [];
  if (titleRefs.length !== 1) {
    throw new Error(`displayText(item.title) 出现 ${titleRefs.length} 次（应恰好 1 次且在三元里）`);
  }
  // 触发条件：悬停 + `:focus-visible`（键盘）**都要有**，否则标题要么永远不可见、要么永远可见。
  if (!/onMouseEnter/.test(row) || !/":focus-visible"/.test(row)) {
    throw new Error("缺少悬停 / `:focus-visible` 展开触发（标题将永远不可见或永远可见）");
  }
  // ★ 无障碍描述**脱敏**：aria-label 走 i18n 的 row.aria（只含 source/state/hint）。
  if (!/aria-label=\{ariaLabel\}/.test(row)) {
    throw new Error("行缺少 aria-label —— 占位符下读屏拿不到任何描述");
  }
  if (!/shared\.companion\.row\.aria/.test(row)) {
    throw new Error("aria-label 未走脱敏描述键 `shared.companion.row.aria`");
  }
  // ★ 不得因为无障碍就把明文塞回去：ariaLabel 的**定义**里不能出现 `item.title`。
  const ariaDef = row.match(/const ariaLabel\s*=\s*([\s\S]*?);/);
  if (!ariaDef) throw new Error("找不到 `const ariaLabel = …` 定义");
  if (/item\.title/.test(ariaDef[1])) {
    throw new Error("aria-label 里出现了完整标题 —— 不能因为无障碍就把明文塞回去");
  }
  return "占位符常量 + revealed 三元 + hover/focus-visible + 脱敏 aria-label（不含标题）";
});

check("★ P9 T03 四态可区分：run/wait/done/fail 四态各有分支且样式两两不同（改重即变红）", () => {
  /* 变异②：把任一态改成与另一态相同的 class（如 failed=done）⇒ 本条变红。 */
  const row = readCode(COMPANION_ROW);
  const m = row.match(/COMPANION_STATE_DOT_CLASS[\s\S]*?=\s*\{([\s\S]*?)\};/);
  if (!m) throw new Error("找不到 `COMPANION_STATE_DOT_CLASS` 表");
  const block = m[1];
  const classes = {};
  for (const state of ["running", "waiting", "done", "failed"]) {
    const cm = block.match(new RegExp(`\\b${state}\\s*:\\s*"([^"]+)"`));
    if (!cm) throw new Error(`四态缺 \`${state}\` 渲染分支`);
    classes[state] = cm[1];
  }
  if (new Set(Object.values(classes)).size !== 4) {
    throw new Error(`四态样式未两两区分：${JSON.stringify(classes)}`);
  }
  // 四态文案键也都要有（否则某态无文案、读屏念不出状态）。
  for (const state of ["running", "waiting", "done", "failed"]) {
    if (!row.includes(`shared.companion.state.${state}`)) {
      throw new Error(`缺 \`${state}\` 状态文案键`);
    }
  }
  return `4 态样式两两不同 · 4 态文案齐备`;
});

check("★ P9 T03 只读：行点击只走 `companion_jump`（聚焦来源），UI 层无任何写操作", () => {
  /* 变异③：给 UI 加一个写操作（如点击直接切账号）⇒ 本条变红。 */
  const row = readCode(COMPANION_ROW);
  const app = readCode(COMPANION_APP);
  for (const [name, code] of [
    ["companion-row.tsx", row],
    ["companion-app.tsx", app],
  ]) {
    for (const bad of COMPANION_UI_FORBIDDEN) {
      if (code.includes(bad)) throw new Error(`${name} 出现写操作痕迹：${bad}（悬浮窗只读）`);
    }
  }
  // 正向：行点击 → onJump；App 把 onJump 接到 companion_jump（聚焦来源，不切账号）。
  if (!/onClick=\{\(\)\s*=>\s*onJump\(item\.id\)\}/.test(row)) {
    throw new Error("行点击没有接到 `onJump(item.id)`");
  }
  if (!/companionJump\(/.test(app)) {
    throw new Error("companion-app.tsx 没有调用 `companionJump`（点击无动作）");
  }
  // 明确反馈（不静默失败）：成功 / 失败都要有文案。
  if (!/shared\.companion\.jump\.ok/.test(app) || !/shared\.companion\.jump\.failed/.test(app)) {
    throw new Error("`companion_jump` 的成功 / 失败反馈文案缺失（会静默失败）");
  }
  return "无写操作 · onClick→onJump→companion_jump · 成功/失败均有反馈";
});

check("★ P9 T03 状态语言：首帧 `<Skeleton>`、空态 `<EmptyState>`（含出口）", () => {
  const app = readCode(COMPANION_APP);
  if (!/<Skeleton\b/.test(app)) throw new Error("首帧加载没有用 `<Skeleton>`（§3.6 状态语言）");
  if (!/<EmptyState\b/.test(app)) throw new Error("空态没有用 `<EmptyState>`");
  // 空态四要素里的「出口」：`action` 必须真的给了（不能只渲染 icon/title/description）。
  if (!/action=\{/.test(app)) throw new Error("空态缺少出口（`action`）—— 用户会被丢在原地");
  // 只读脚注（点击语义 / 标题悬停说明）。
  if (!/shared\.companion\.footnote/.test(app)) {
    throw new Error("缺少只读脚注（点击回到原会话 / 标题悬停显示）");
  }
  return "Skeleton · EmptyState(含 action 出口) · 只读脚注";
});

/* ─────────────────────────── P9 · T04 宿主侧入口 ───────────────────────────
 *
 * 上面 11 条锁 P9 T01/T02/T03；下面 5 条锁 T04 最易回退的判据：
 *   ★ 侧栏左下角入口存在且是**开关**（不是第二个「看起来能切」的控件）/
 *     可见性偏好落在 L0 localStorage（默认显示）+ 与托盘 / 关闭按钮经事件同步 /
 *     入口**不是一次性的**（关闭 = 隐藏、可重开）+ 托盘勾选态随可见性事件刷新 /
 *     入口词条 zh + en 同步。
 *
 * 变异验证锚（每条都必须能因**同一份 `src/` 实现文件**的改动而变红）：
 *   ① 删掉侧栏入口（只剩托盘）             ⇒ 「入口存在」条变红（防「入口是一次性的」）；
 *   ② 侧栏图标改成「下拉选择」表达可见性     ⇒ 「不是第二个能切的控件」条变红。
 *
 * ⚠️ 只锚在**代码**上（`readCode` 先剥注释）—— 锚在注释上会被「注释里写着反例」骗过去。
 */

const COMPANION_VISIBILITY_TS = "src/lib/use-companion-visibility.ts";

check("★ P9 T04 侧栏左下角入口存在：`CompanionToggle` 走 `useCompanionVisibility`（删入口即变红）", () => {
  /* 变异①（落点 = App.tsx）：删掉侧栏的 `<CompanionToggle />`（或整个组件）⇒ 本条变红。
     理由（P9 B4「入口不是一次性的」）：关掉悬浮窗后必须能从**侧栏左下角**重开 ——
     只剩托盘不满足原型验收（`verify-behaviors.cjs:1204-1212` 点的是侧栏 `cp-toggle`）。 */
  const app = readCode(APP_TSX);
  const body = fnBody(app, "CompanionToggle"); // 组件被删 ⇒ 抛「找不到函数」⇒ 变红
  if (!/useCompanionVisibility\(/.test(body)) {
    throw new Error("`CompanionToggle` 没有用 `useCompanionVisibility`（与托盘 / 关闭按钮不同源）");
  }
  if (!/setVisible\(!visible\)/.test(body)) {
    throw new Error("侧栏入口不是「翻转可见性」的开关（`setVisible(!visible)` 缺失）");
  }
  if (!/<CompanionToggle\b/.test(app)) {
    throw new Error("App.tsx 没有渲染 `<CompanionToggle />` —— 侧栏左下角入口缺失（入口变成一次性的）");
  }
  return "CompanionToggle · useCompanionVisibility · setVisible(!visible) · 已渲染";
});

check("★ P9 T04 不是第二个「看起来能切」的控件：侧栏入口内无 Select / Tabs（改成下拉即变红）", () => {
  /* 变异②（落点 = App.tsx 的 `CompanionToggle`）：把图标按钮改成「下拉选择」表达可见性
     ⇒ 本条变红。理由（P9 §1.5 / R8）：可见性**只有**「侧栏 + 托盘 + 窗口内关闭」三面，
     都是同一状态、经 `companion_set_visible` 收敛；再摆下拉 / Tab = 第二个能切的控件。 */
  const body = fnBody(readCode(APP_TSX), "CompanionToggle");
  for (const bad of ["<Select", "SelectTrigger", "<Tabs", 'role="tablist"', "<DropdownMenu"]) {
    if (body.includes(bad)) {
      throw new Error(`侧栏入口出现第二个「看起来能切」的控件：${bad}（可见性只有开关三面）`);
    }
  }
  // 正向：入口就是一个 `<button>` 开关（`aria-pressed` 表明它是 toggle，而非跳转 / 下拉）。
  if (!/aria-pressed=\{visible\}/.test(body)) {
    throw new Error("侧栏入口没有 `aria-pressed={visible}` —— 无法表达「开 / 关」");
  }
  return "无 Select / Tabs / DropdownMenu · aria-pressed 开关";
});

check("★ P9 T04 可见性偏好 hook：L0 localStorage（默认显示）+ listen 同步（缺同步即变红）", () => {
  /* 载体必须是 L0 localStorage（§2.1 / §1.5），默认**显示**（§3.16）；且必须 `listen`
     可见性事件与托盘 / 关闭按钮同步（否则三面会各说各话 = 多个能切的控件）。 */
  if (!fs.existsSync(path.join(ROOT, COMPANION_VISIBILITY_TS))) {
    throw new Error(`缺少 ${COMPANION_VISIBILITY_TS}`);
  }
  const lib = readCode(COMPANION_VISIBILITY_TS);
  if (!/buddy-switch\.companion-visible/.test(lib)) {
    throw new Error("可见性偏好没有落到 L0 localStorage（键 `buddy-switch.companion-visible`）");
  }
  if (!/listen<boolean>\(\s*COMPANION_VISIBILITY_EVENT/.test(lib)) {
    throw new Error('hook 没有 `listen("companion-visibility")` —— 与托盘 / 关闭按钮不同步');
  }
  if (!/"companion-visibility"/.test(lib)) {
    throw new Error("hook 的事件名与 companion.rs 的 `VISIBILITY_EVENT` 不一致");
  }
  if (!/raw === null \? true/.test(lib)) {
    throw new Error("偏好默认值不是「显示」（未设置时应返回 true，§3.16「默认显示」）");
  }
  return "localStorage(默认显示) · listen(companion-visibility) · 与托盘/关闭按钮同源";
});

check("★ P9 T04 入口不是一次性的 + 勾选态同步（关闭=隐藏可重开；托盘随事件刷新）", () => {
  /* 两件事：
     ① 「关闭 = 隐藏（不销毁）」在 Rust 侧钉死（`companion.rs` 的 `CloseRequested`）——
        否则「从侧栏重开」会变成「重新创建窗口」，位置记忆 / 状态全丢；
     ② 勾选态同步：侧栏 / 关闭按钮改可见性时托盘勾选态要跟随（`tray.rs` 监听可见性事件）。 */
  const rs = readCode(COMPANION_RS);
  if (!/CloseRequested\s*\{[\s\S]{0,120}?api\.prevent_close\(\)/.test(rs)) {
    throw new Error("companion.rs 的 `CloseRequested` 没有 `prevent_close()` —— 关闭会**销毁**窗口");
  }
  if (!/CloseRequested\s*\{[\s\S]{0,400}?window\.hide\(\)/.test(rs)) {
    throw new Error("companion.rs 的关闭没有 `hide()`（应「隐藏而非销毁」，才能从侧栏 / 托盘重开）");
  }
  const tray = readCode(TRAY_RS);
  if (!/listen\(\s*crate::companion::VISIBILITY_EVENT/.test(tray)) {
    throw new Error("tray.rs 没有监听 `companion-visibility` —— 从侧栏 / 关闭按钮改可见性时勾选态会漂移");
  }
  return "关闭=隐藏（不销毁）· 托盘监听可见性事件刷新勾选态";
});

check("★ P9 T04 词条 zh + en 同步（`shared.companion.entry`，且入口真的引用）", () => {
  const zh = localeValue(SHARED_LOCALE_ZH, "shared.companion.entry");
  const en = localeValue(SHARED_LOCALE_EN, "shared.companion.entry");
  if (!zh || !en) throw new Error("shared.companion.entry 在 zh / en 至少一侧为空");
  if (!/会话/.test(zh)) throw new Error(`中文入口文案不含「会话」：${zh}`);
  if (!/Session/i.test(en)) throw new Error(`英文入口文案不含 Session：${en}`);
  if (!/shared\.companion\.entry/.test(readCode(APP_TSX))) {
    throw new Error("App.tsx 没有引用 `shared.companion.entry`（死键）");
  }
  return `zh=「${zh}」· en=「${en}」· 入口已引用`;
});

/* ─────────────────────── P9 · T05 键盘可达性（独立窗口，自管焦点）───────────────────────
 *
 * A8 / P9-ARCH §1.7：`decorations(false)` 的**独立窗口没有系统标题栏**，主窗口的焦点管理
 * **一条都不适用**，也**不能依赖 OS 默认聚焦** ⇒ 这几条必须由**代码**钉死，否则「通过」是空转。
 * ⚠️ 复用主窗口的键盘测试 = 空转（A8 原话）。
 *
 * 变异验证锚（每条都必须能因**同一批实现文件**的改动而变红）：
 *   ① 删掉首次聚焦（`focusEntry` 的调用）     ⇒ 「焦点入口」段变红；
 *   ② 删掉 Esc 分支                          ⇒ 「Esc」段变红；
 *   ③ 把行从原生 `<button>` 改成 `<div onClick>` ⇒ 「行可聚焦」段变红。
 */

check("★ P9 T05 键盘可达性：焦点入口（首次 + 重开）/ Esc / 行原生 button / 拖动不抢焦点", () => {
  const app = readCode(COMPANION_APP);
  const row = readCode(COMPANION_ROW);

  // ① 焦点入口：内容就绪后把焦点落到**第一个可聚焦元素**（行 `<button>` / 空态「刷新」），
  //    取不到再退关闭按钮 —— `decorations(false)` ⇒ 不能依赖 OS 默认聚焦。
  if (!/contentRef\.current\?\.querySelector<HTMLElement>\("button"\)/.test(app)) {
    throw new Error("companion-app.tsx 缺少「取内容区第一个可聚焦元素」的焦点入口（§1.7 焦点入口）");
  }
  if (!/\(first \?\? closeRef\.current\)\?\.focus\(\)/.test(app)) {
    throw new Error("焦点入口没有 `focus()` 调用（焦点落不到任何元素）");
  }
  // 只做**一次**首次聚焦：否则每次刷新都会把焦点从用户手里抢回第一行。
  if (!/didInitialFocusRef/.test(app)) {
    throw new Error("缺少「首次聚焦只做一次」护栏（刷新会反复抢焦点）");
  }
  // 重新可见 ⇒ 送回焦点（隐藏 = 不销毁，重新 show 时 webview 内 DOM 焦点已丢）。
  if (!/listen<boolean>\(\s*"companion-visibility"/.test(app)) {
    throw new Error("没有监听 `companion-visibility` 把焦点送回入口（从侧栏 / 托盘重开后键盘不可达）");
  }

  // ② Esc = 隐藏窗口（独立窗口无系统标题栏，键盘必须能关）。
  if (!/addEventListener\(\s*"keydown"/.test(app) || !/event\.key\s*===\s*"Escape"/.test(app)) {
    throw new Error("缺少 Esc 隐藏窗口的键盘处理（§1.7）");
  }
  if (!/event\.key\s*===\s*"Escape"[\s\S]{0,60}?companionSetVisible\(false\)/.test(app)) {
    throw new Error("Esc 没有接到 `companionSetVisible(false)`（按键无效果）");
  }

  // ③ 行必须是**原生 `<button>`**（Tab / Enter / Space 原生可用），且点击走 `onJump`。
  if (!/<button\b/.test(row)) {
    throw new Error("companion-row.tsx 的行不是原生 `<button>`（键盘 Tab / Enter 失效）");
  }
  if (!/onClick=\{\(\)\s*=>\s*onJump\(item\.id\)\}/.test(row)) {
    throw new Error("行点击没有接到 `onJump(item.id)`");
  }
  if (!/:focus-visible/.test(row)) {
    throw new Error("行缺少 `:focus-visible`（键盘聚焦时看不到焦点 / 标题不展开）");
  }

  // ④ 拖动不抢焦点：拖拽手柄走 `data-tauri-drag-region`（原生拖动，**不产生可聚焦元素**）。
  if (!/data-tauri-drag-region/.test(app)) {
    throw new Error("拖动区缺少 `data-tauri-drag-region` —— 拖动会变成可聚焦元素 / 抢焦点（§1.7）");
  }

  // ⑤ 关闭按钮可聚焦 + 有可读标签（`shared.companion.close` = 「隐藏悬浮窗」）。
  if (!/aria-label=\{t\("shared\.companion\.close"\)\}/.test(app)) {
    throw new Error("关闭按钮缺少可读标签（`shared.companion.close`）—— 读屏 / 键盘用户不知其用途");
  }

  // ⑥ 减弱动效：running 的持续脉冲须尊重 `prefers-reduced-motion`（PLAN §3.6 无障碍基线）。
  if (!/motion-reduce:animate-none/.test(row)) {
    throw new Error("running 脉冲未尊重 `prefers-reduced-motion`（无障碍基线）");
  }

  return "焦点入口(首次+重开) · Esc 隐藏 · 行原生 button · drag-region 不抢焦点 · 关闭按钮可读 · motion-reduce";
});

/* ─────────────────────────── P7 全局备份与恢复 ───────────────────────────
 *
 * 变异验证锚（每条断言都必须能因**同一批实现文件**的改动而变红）：
 *   ① 导入预览不显示「会改什么」          ⇒ `backup-import-preview.tsx`
 *   ② 文案改成「备份完整」                ⇒ `backup.zh.ts` / `backup.en.ts`
 *   ③ 定时备份开关删掉（只能默认开不能关） ⇒ `backup-section.tsx`
 */

const BACKUP_PREVIEW = "src/components/settings/backup-import-preview.tsx";
const BACKUP_SECTION = "src/components/settings/backup-section.tsx";
const BACKUP_SUBNAV = "src/components/settings/settings-subnav.tsx";
const BACKUP_SELECTION = "src/lib/backup-selection.ts";
const APP_SETTINGS_SNAPSHOT = "src/lib/app-settings-snapshot.ts";
const BACKUP_LOCALE_ZH = "src/locales/domains/backup.zh.ts";
const BACKUP_LOCALE_EN = "src/locales/domains/backup.en.ts";

check("★ P7 设置 9 项子导航：9 项齐备，第 9 项是「备份与恢复」", () => {
  const code = readCode(BACKUP_SUBNAV);
  const m = code.match(/SETTINGS_SUBNAV[\s\S]*?=\s*\[([\s\S]*?)\];/);
  if (!m) throw new Error("找不到 `SETTINGS_SUBNAV` 表");
  const ids = [...m[1].matchAll(/id:\s*"([a-z]+)"/g)].map((x) => x[1]);
  if (ids.length !== 9) throw new Error(`子导航应 9 项，实际 ${ids.length}：${ids.join(",")}`);
  if (ids[ids.length - 1] !== "backup") {
    throw new Error(`第 9 项应为 backup（备份与恢复），实际 ${ids[ids.length - 1]}`);
  }
  if (!/settings\.subnav\.backup/.test(code)) throw new Error("缺「备份与恢复」标签键");
  return ids.join(" / ");
});

check("★ P7 导入预览是安全阀：逐作用域展示「会改什么」（删计数即变红）", () => {
  /* 变异①：预览不显示新增 / 覆盖 / 跳过 ⇒ 本条必须变红。 */
  const code = readCode(BACKUP_PREVIEW);
  if (!/export function BackupImportPreview/.test(code)) throw new Error("缺导入预览组件");
  // 三个计数必须**逐行绑定**（`row.added` …），删掉那一段渲染即失配
  for (const field of ["added", "overwritten", "skipped"]) {
    if (!new RegExp(`row\\.${field}\\b`).test(code)) {
      throw new Error(`预览未按行渲染「${field}」—— 用户看不到这次导入会改什么`);
    }
  }
  if (!/backup\.import\.preview\.counts/.test(code)) {
    throw new Error("缺计数文案键 `backup.import.preview.counts`");
  }
  if (!/backup\.import\.preview\.relogin/.test(code)) {
    throw new Error("不含凭据时未提示「需重新登录」");
  }
  const zh = localeValue(BACKUP_LOCALE_ZH, "backup.import.preview.counts");
  if (!/新增/.test(zh) || !/覆盖/.test(zh) || !/跳过/.test(zh)) {
    throw new Error(`计数文案不含 新增/覆盖/跳过：${zh}`);
  }
  return "逐行计数（added/overwritten/skipped）+ 需重新登录提示";
});

check("★ P7 文案红线：不得写「备份完整」，必须写「凭据不含在内，导入后需重新登录」", () => {
  /* 变异②：把 `backup.credentials.notice` 改成「备份完整」⇒ 本条必须变红。 */
  const zh = localeValue(BACKUP_LOCALE_ZH, "backup.credentials.notice");
  const en = localeValue(BACKUP_LOCALE_EN, "backup.credentials.notice");
  if (!/凭据不含在内/.test(zh) || !/重新登录/.test(zh)) {
    throw new Error(`zh 未如实说明「凭据不含在内，导入后需重新登录」：${zh}`);
  }
  if (!/Credentials are not included/i.test(en) || !/sign in again/i.test(en)) {
    throw new Error(`en 未如实说明凭据不含在内：${en}`);
  }
  /* ⚠️ 必须**剥注释**再搜：本仓注释里就写着「不得写『备份完整』」——不剥会把注释当文案。 */
  for (const rel of [BACKUP_LOCALE_ZH, BACKUP_LOCALE_EN]) {
    const raw = stripComments(fs.readFileSync(path.join(ROOT, rel), "utf8"));
    if (/备份完整|backup complete/i.test(raw)) {
      throw new Error(`${path.basename(rel)} 出现假承诺「备份完整 / backup complete」`);
    }
  }
  if (!/backup\.credentials\.notice/.test(readCode(BACKUP_SECTION))) {
    throw new Error("`backup.credentials.notice` 没有被渲染（死键）");
  }
  return `zh=「${zh}」· en=「${en}」· 已引用`;
});

check("★ P7 定时备份开关：默认开、且**必须能关**（删开关即变红）", () => {
  /* 变异③：删掉「启用定时自动备份」开关（只剩默认开、不能关）⇒ 本条必须变红。 */
  const code = readCode(BACKUP_SECTION);
  if (!/id="backup-auto-enabled"/.test(code)) {
    throw new Error("缺定时备份开关（`id=\"backup-auto-enabled\"`）—— 用户无法关闭定时备份");
  }
  if (!/autoConfig\?\.enabled/.test(code)) throw new Error("开关未绑定 `autoConfig?.enabled`");
  if (!/toggleAuto/.test(code) || !/api\.saveAutoBackupConfig/.test(code)) {
    throw new Error("开关未接到 `saveAutoBackupConfig`（改了不落盘 = 假开关）");
  }
  if (!/backup\.auto\.enableDesc/.test(code)) throw new Error("缺开关说明（关闭后不再产生新备份）");
  const zh = localeValue(BACKUP_LOCALE_ZH, "backup.auto.enableDesc");
  if (!/关闭/.test(zh) || !/保留/.test(zh)) {
    throw new Error(`说明未讲清「关闭后不再产生、已产生的保留」：${zh}`);
  }
  return "绑定 autoConfig.enabled · 接 saveAutoBackupConfig · 说明含关闭语义";
});

check("★ P7 预估由实际勾选推导（全不勾 ⇒ 无可导出；勾选变 ⇒ 预估变）", () => {
  const lib = loadTs(path.join(ROOT, BACKUP_SELECTION));
  const stats = {
    "wb-cn": { accounts: 3, config: 4, gateway: 2 },
    "wb-global": { accounts: 2, config: 4, gateway: 1 },
    "trae-cn": { accounts: 5, config: 4, gateway: 0 },
    "trae-global": { accounts: 1, config: 4, gateway: 0 },
    "qoder-cn": { accounts: 2, config: 4, gateway: 2 },
    "qoder-global": { accounts: 1, config: 4, gateway: 1 },
  };
  const all = lib.defaultBackupSelection();
  if (lib.selectedScopeCount(all) !== 6) throw new Error("默认应全选 6 个作用域（3 平台 × 2 区域，D14）");
  if (!lib.hasAnySelection(all)) throw new Error("默认应可导出");

  const none = { ...all };
  for (const scope of ["wb-cn", "wb-global", "trae-cn", "trae-global", "qoder-cn", "qoder-global"]) {
    none[scope] = { accounts: false, config: false, gateway: false };
  }
  if (lib.hasAnySelection(none)) throw new Error("全不勾仍判为可导出 —— R10（空备份）");
  const estNone = lib.estimateBackup(none, false, stats);
  if (estNone.scopes !== 0 || estNone.parts !== 0 || estNone.accounts !== 0) {
    throw new Error("全不勾时预估应全 0");
  }

  const estAll = lib.estimateBackup(all, false, stats);
  if (estAll.scopes !== 6 || estAll.parts !== 18) {
    throw new Error(`全选应 6 作用域 × 3 分区，实际 ${estAll.scopes}/${estAll.parts}`);
  }
  if (estAll.accounts !== 14) throw new Error(`账号合计应 14，实际 ${estAll.accounts}`);

  // 关掉 trae-cn 的「账号」分区 ⇒ 账号数 / 分区数必须变（写死数字即不通过）
  const tweaked = lib.toggleScopePart(all, "trae-cn", "accounts");
  const estTweak = lib.estimateBackup(tweaked, false, stats);
  if (estTweak.accounts !== 9) throw new Error(`关掉 trae-cn 账号后应 9，实际 ${estTweak.accounts}`);
  if (estTweak.parts !== 17) throw new Error(`分区数应 17，实际 ${estTweak.parts}`);
  if (lib.scopeCheckState(tweaked, "trae-cn") !== "mixed") throw new Error("半选应为 mixed");
  if (lib.scopeCheckState(all, "trae-cn") !== "all") throw new Error("全选应为 all");
  if (!(lib.estimateBackup(all, true, stats).bytes > estAll.bytes)) {
    throw new Error("含凭据的预估体积应更大");
  }
  return "默认 6×3 · 全不勾=0 · 勾选变则预估变 · 半选=mixed";
});

check("★ P7 设置页接线：子导航 + 备份分区 + 全不勾禁用导出", () => {
  const page = readCode("src/pages/SettingsPage.tsx");
  if (!/<SettingsSubnav\b/.test(page)) throw new Error("SettingsPage 没有接子导航");
  if (!/<BackupSection\b/.test(page)) throw new Error("SettingsPage 没有渲染备份分区");
  if (!/useState<SettingsSubnavId>/.test(page)) throw new Error("子导航选中态未接线");
  const section = readCode(BACKUP_SECTION);
  if (!/hasAnySelection/.test(section)) throw new Error("导出按钮未按「全不勾」判定");
  if (!/disabled=\{pending \|\| !canExport\}/.test(section)) {
    throw new Error("导出按钮未禁用 —— 全不勾仍可导出空备份（R10）");
  }
  return "子导航接线 · 备份分区 · 全不勾禁用导出";
});

check("★ 网关页两个轴：类型表 + 分区表按类型分家 + 三页接线（2026-10-15 起，替代原「7 项子导航」）", () => {
  /* 为什么改：统一网关**跨全部作用域**，而独立网关的 6 个分区**都随作用域过滤** ——
     两个正交的轴挤在同一层是结构错位。现在拆成「类型 × 分区」，类型切换在左栏顶部。
     变异①：把类型表删掉 / 退回单表 ⇒ 下面任一断言变红。 */
  const nav = readCode("src/components/gateway/gateway-subnav.tsx");
  const idsOf = (re, what) => {
    const m = nav.match(re);
    if (!m) throw new Error("找不到表：" + what);
    return [...m[1].matchAll(/id:\s*"([a-z-]+)"/g)].map((x) => x[1]);
  };
  const types = idsOf(/GW_TYPES[\s\S]*?=\s*\[([\s\S]*?)\];/, "GW_TYPES");
  if (types.join(",") !== "independent,unified") {
    throw new Error(`类型应为 independent / unified，实际 ${types.join(" / ")}`);
  }
  const indep = idsOf(/GW_INDEPENDENT_SECTIONS[\s\S]*?=\s*\[([\s\S]*?)\];/, "GW_INDEPENDENT_SECTIONS");
  const wantIndep = ["overview", "keys", "models", "pool", "proxy", "logs"];
  if (indep.join(",") !== wantIndep.join(",")) {
    throw new Error(`独立网关分区应为 ${wantIndep.join(" / ")}，实际 ${indep.join(" / ")}`);
  }
  const uni = idsOf(/GW_UNIFIED_SECTIONS[\s\S]*?=\s*\[([\s\S]*?)\];/, "GW_UNIFIED_SECTIONS");
  /* ⚠️ 统一网关侧的分区 id **必须带 \`u-\` 前缀**：两个类型的分区 id 集合必须**不相交**，
     否则切类型会停在另一个类型的同名分区上（渲染成空）。 */
  for (const id of uni) {
    if (!id.startsWith("u-")) throw new Error(`统一网关分区 id 必须带 u- 前缀：${id}`);
  }
  if (indep.some((id) => uni.includes(id))) throw new Error("两个类型的分区 id 不得相交");
  /* 分区必须**按类型 fail-closed**（\`gatewaySection\`），不能整表查找。 */
  if (!/export function gatewaySection\(type: GatewayType/.test(nav)) {
    throw new Error("缺按类型 fail-closed 的 `gatewaySection`");
  }
  if (!/export function parseGatewayDeepLink\(/.test(nav)) {
    throw new Error("缺 `parseGatewayDeepLink`（两代旧深链兼容）");
  }
  /* ★ 表里的每一项都必须在页面上**真的接线** —— 摆了入口没接实现比不摆更糟
     （本项目已出过同类事故）。统一网关的分区走同一个 \`<UnifiedGatewayView section=…>\`。 */
  for (const page of ["src/pages/ApiServicePage.tsx", "src/pages/TraeApiServicePage.tsx", "src/pages/QoderApiServicePage.tsx"]) {
    const code = readCode(page);
    if (!/<GatewayShell\b/.test(code)) throw new Error(`${page} 没有接网关外壳`);
    if (!/parseGatewayDeepLink/.test(code)) throw new Error(`${page} 未接线两个轴的深链解析`);
    if (!/function goType\(/.test(code)) throw new Error(`${page} 切类型未接线（缺 goType）`);
    if (!/function goSub\(/.test(code)) throw new Error(`${page} 切换分区未写 URL（缺 goSub）`);
    if (!/<UnifiedGatewayView\b/.test(code)) throw new Error(`${page} 未渲染 <UnifiedGatewayView>`);
    for (const id of uni) {
      if (!code.includes(`section="${id.replace(/^u-/, "")}"`)) {
        throw new Error(`${page} 未渲染统一网关分区 ${id}（表里有、页面上没接线）`);
      }
    }
  }
  /* 外壳自带 \`data-split-layout\`（900px 转纵向）与左栏容器 —— 三页共用一份。 */
  const shell = readCode("src/components/gateway/gateway-shell.tsx");
  if (!/data-split-layout/.test(shell)) throw new Error("GatewayShell 缺 data-split-layout（900px 以下不转纵向）");
  if (!/data-gateway-sidebar/.test(shell)) throw new Error("GatewayShell 缺 data-gateway-sidebar（左栏容器）");
  /* 变异②：Trae / Qoder 若把「请求代理」也渲染出来（P12 的 C 路只覆盖 WorkBuddy 网关端口）⇒ 变红。 */
  for (const page of ["src/pages/TraeApiServicePage.tsx", "src/pages/QoderApiServicePage.tsx"]) {
    if (!/GW_INDEPENDENT_SECTIONS_NO_PROXY/.test(readCode(page))) {
      throw new Error(`${page} 未去掉「请求代理」（该能力只覆盖 WorkBuddy）`);
    }
  }
  return `类型 ${types.join("/")} · 独立 ${indep.length} 项 · 统一 ${uni.join("/")} · 三页接线 · Trae/Qoder 去 proxy`;
});

check("★ P7 冲突策略只有三种（不渲染「逐条询问」，§7 #5）", () => {
  const code = readCode(BACKUP_PREVIEW);
  for (const policy of ["keepLocal", "overwrite", "keepBoth"]) {
    if (!code.includes(policy)) throw new Error(`缺冲突策略 ${policy}`);
  }
  if (/\bask\b/i.test(code) || /逐条询问/.test(code)) {
    throw new Error("渲染了「逐条询问」（Ask）—— 本期 UI 裁剪，有入口即假承诺");
  }
  const zh = localeValue(BACKUP_LOCALE_ZH, "backup.import.conflict.keepLocal");
  if (!/保留本地/.test(zh)) throw new Error(`默认策略文案应为「保留本地」：${zh}`);
  return "keepLocal / overwrite / keepBoth 三种 · 无 Ask";
});

check("★ P7 appSettings localStorage 段：键名与各来源模块逐字一致（漂移即变红）", () => {
  const code = readCode(APP_SETTINGS_SNAPSHOT);
  const pairs = [
    ["buddy-switch.language", "src/lib/i18n.ts"],
    ["buddy-switch.theme", "src/lib/theme.ts"],
    ["buddy-switch:v1:last-scope", "src/lib/scope.ts"],
  ];
  for (const [key, source] of pairs) {
    if (!code.includes(`"${key}"`)) {
      throw new Error(`${path.basename(APP_SETTINGS_SNAPSHOT)} 缺键 ${key}`);
    }
    if (!readCode(source).includes(`"${key}"`)) {
      throw new Error(`${path.basename(source)} 不再声明 ${key}（键漂移）`);
    }
  }
  if (!/readAppSettingsSnapshot/.test(code) || !/applyAppSettingsSnapshot/.test(code)) {
    throw new Error("缺读 / 回写函数");
  }
  if (!/containsCredential/.test(code)) throw new Error("缺凭据护栏（代理 URL 可能含 user:pass@）");
  return "3 键与来源逐字一致 · 读/写/凭据护栏齐备";
});

check("★ P7 词条 zh + en 同步（备份域逐键对齐，且已组合进词表）", () => {
  const zh = fs.readFileSync(path.join(ROOT, BACKUP_LOCALE_ZH), "utf8");
  const en = fs.readFileSync(path.join(ROOT, BACKUP_LOCALE_EN), "utf8");
  const keys = (src) => [...src.matchAll(/"(backup\.[^"]+)"\s*:/g)].map((m) => m[1]);
  const zhKeys = new Set(keys(zh));
  const enKeys = new Set(keys(en));
  const missing = [...zhKeys].filter((k) => !enKeys.has(k));
  if (missing.length) throw new Error(`en 缺 ${missing.length} 个键：${missing.slice(0, 5).join(", ")}`);
  const extra = [...enKeys].filter((k) => !zhKeys.has(k));
  if (extra.length) throw new Error(`en 多出 ${extra.length} 个键：${extra.slice(0, 5).join(", ")}`);
  if (zhKeys.size < 20) throw new Error(`备份词条过少（${zhKeys.size}）—— 是不是没组合进来？`);
  if (!/domains\/backup\.zh/.test(readCode("src/locales/zh.ts"))) throw new Error("zh.ts 未组合 backup 域");
  if (!/domains\/backup\.en/.test(readCode("src/locales/en.ts"))) throw new Error("en.ts 未组合 backup 域");
  return `${zhKeys.size} 键 zh=en 且已组合`;
});

check("★ P7 白屏防线 + 状态语言：displayText / Skeleton / Loader2 / EmptyState(含出口)", () => {
  for (const rel of [BACKUP_PREVIEW, BACKUP_SECTION]) {
    if (!/displayText/.test(readCode(rel))) {
      throw new Error(`${path.basename(rel)} 未用 displayText（对象当 React 子节点 ⇒ 白屏）`);
    }
  }
  if (!/displayText\(scope\)/.test(readCode(BACKUP_PREVIEW))) {
    throw new Error("作用域标签未过 displayText");
  }
  const section = readCode(BACKUP_SECTION);
  if (!/<Skeleton\b/.test(section)) throw new Error("区块首次加载缺 <Skeleton>");
  if (!/<Loader2\b/.test(section)) throw new Error("操作中缺 <Loader2>（按钮内，不是加载态）");
  if (!/<EmptyState\b/.test(section)) throw new Error("缺 <EmptyState>");
  if (!/action=\{/.test(section)) throw new Error("<EmptyState> 缺出口（action）");
  return "displayText · Skeleton · Loader2 · EmptyState(含出口)";
});

/* ── P7 后端行为锚（T04 独立验证）───────────────────────────────────────────────
 *
 * 上面几条守的是**前端**（子导航 / 预览 / 词条 / 开关 UI）；下面几条守**后端行为**——
 * 它们都是「看起来是绿的假断言」的高危区（本仓真实踩过：安全断言恒真）。
 * 每条都给**变异锚**：改动同一批实现文件里的对应点，本条必须变红。
 *
 * 路径相对仓库根（`readCode` 已剥注释；Rust 的行注释与块注释语法与 JS 同构，可复用）。
 */

const BACKUP_RS = "crates/buddy-switch-core/src/modules/backup.rs";
const BACKUP_SECTIONS_RS = "crates/buddy-switch-core/src/modules/backup_sections.rs";
const EXPORT_IMPORT_RS = "crates/buddy-switch-core/src/modules/export_import.rs";
const CONFIG_RS = "crates/buddy-switch-core/src/modules/config.rs";
const BACKGROUND_RS = "crates/buddy-switch-core/src/modules/background.rs";
const TAURI_LIB_RS = "src-tauri/src/lib.rs";
const SERVER_MAIN_RS = "crates/buddy-switch-server/src/main.rs";

check("★ P7 凭据红线（可证伪）：CREDENTIAL_KEYS 用真实键、剥离与搜索共用同一常量、测试真种入凭据", () => {
  /* 变异①：删掉 `export_backup` 里的 `strip_credentials(...)` ⇒ 本条变红。
     变异②：把搜索键改成只有 camelCase（丢掉 snake_case）⇒ 本条变红（旧判据的假阴性）。 */
  const code = readCode(BACKUP_RS);
  // 键集必须含**真实** snake_case 键（用 camelCase 会让断言恒真）。
  for (const key of ["access_token", "refresh_token", "accessToken", "refreshToken", "$wbEncrypted", "jwt"]) {
    if (!code.includes(`"${key}"`)) {
      throw new Error(`CREDENTIAL_KEYS 缺真实键 ${key}（缺它则断言漏检）`);
    }
  }
  // 实现层：export_backup 必须真的调用剥离。
  if (!/pub fn export_backup[\s\S]{0,500}strip_credentials\(/.test(code)) {
    throw new Error("export_backup 未调用 strip_credentials —— 导出可能带凭据");
  }
  // 验收层与实现层共用同一常量（杜绝判据漂移）。
  if (!/contains_credential_key[\s\S]{0,220}CREDENTIAL_KEYS/.test(code)) {
    throw new Error("contains_credential_key 未复用 CREDENTIAL_KEYS（判据漂移）");
  }
  // ★ 端到端测试必须**真种入带凭据的账号**，否则「0 命中」恒真（假阴性）。
  if (!/save_accounts_for[\s\S]{0,1500}contains_credential_key/.test(code)) {
    throw new Error("导出测试未种入凭据 —— 「搜不到凭据键」会恒真（假断言）");
  }
  return "6 真实键 · 剥离/搜索共用常量 · 导出测试真种入凭据";
});

check("★ P7 导入幂等：默认保留本地（KeepLocal）· 账号级仍 (Overwrite, Require)", () => {
  /* 变异③：把默认策略改成 KeepBoth（每次增行）⇒ 幂等断言变红（此条锚住默认值）。 */
  const code = readCode(EXPORT_IMPORT_RS);
  if (!/"" => Ok\(ConflictPolicy::KeepLocal\)/.test(code)) {
    throw new Error("空策略未回落保留本地（D16 安全默认）—— 幂等前提被破");
  }
  if (!/ConflictPolicy::KeepLocal \| ConflictPolicy::Ask => MergeOutcome::KeptLocal/.test(code)) {
    throw new Error("KeepLocal 命中 uid 未返回 KeptLocal（不覆盖、不增行）—— 幂等被破");
  }
  if (!/merge_import_record\(\s*accounts,\s*item,\s*ConflictPolicy::Overwrite,\s*TokenPolicy::Require/.test(code)) {
    throw new Error("账号级导入未用 (Overwrite, Require) —— P0-1「CN 行为零变化」被改");
  }
  if (!/TokenPolicy::Allow/.test(readCode(BACKUP_SECTIONS_RS))) {
    throw new Error("全局备份未用 TokenPolicy::Allow —— 不含凭据的备份会一条都导不进");
  }
  return "默认 KeepLocal · 命中 uid=KeptLocal · 账号级(Overwrite,Require) · 全局 Allow";
});

check("★ P7 未知 scope fail-closed：走 Scope::parse，绝不 unwrap_or 回落默认（R6）", () => {
  /* 变异④：把未知 scope 改成「静默跳过 / 回落某已知 scope」⇒ 本条变红。 */
  const code = readCode(BACKUP_RS);
  if (!/Scope::parse\(key\)/.test(code)) {
    throw new Error("resolve_scope_key 未走 Scope::parse（与 P5 不同口径）");
  }
  if (/unwrap_or\(\s*Scope::/.test(code)) {
    throw new Error("backup.rs 出现 unwrap_or(Scope::…) —— 未知 scope 静默回落（跨库串数据）");
  }
  if (!/resolve_scope_key\(&scope_key\)[\s\S]{0,420}failed \+= 1/.test(code)) {
    throw new Error("import_backup 未把未知 scope 计入 failed（可能静默跳过）");
  }
  return "Scope::parse · 无 unwrap_or 回落 · 未知 scope 记 failed";
});

check("★ P7 导入前强制快照（D18）：先 parse 后快照，快照前缀 pre-import-", () => {
  /* 变异⑦：绕过 snapshot_before_import ⇒ 本条变红（导入不可撤销）。 */
  const code = readCode(BACKUP_RS);
  if (!/pub fn import_backup[\s\S]{0,400}parse_envelope[\s\S]{0,220}snapshot_before_import/.test(code)) {
    throw new Error("import_backup 未在解析后强制快照 —— 损坏文件会产生快照 / 导入不可撤销");
  }
  if (!/PRE_IMPORT_SNAPSHOT_PREFIX: &str = "pre-import-"/.test(readCode(CONFIG_RS))) {
    throw new Error("缺导入前快照前缀 pre-import-");
  }
  return "先 parse 后快照 · 前缀 pre-import-";
});

check("★ P7 定时备份可关：两宿主 AutoBackup 分支都守卫 is_auto_backup_enabled", () => {
  /* 变异：删掉宿主的 is_auto_backup_enabled 守卫（只剩默认开、关不掉）⇒ 本条变红。 */
  for (const host of [TAURI_LIB_RS, SERVER_MAIN_RS]) {
    const code = readCode(host);
    if (!/TaskKind::AutoBackup =>/.test(code)) {
      throw new Error(`${host} 未登记 AutoBackup 分支`);
    }
    if (
      !/TaskKind::AutoBackup =>[\s\S]{0,500}is_auto_backup_enabled\(\)[\s\S]{0,220}auto_backup\(\)/.test(
        code,
      )
    ) {
      throw new Error(`${host} 的 AutoBackup 未用 is_auto_backup_enabled 守卫 —— 定时备份关不掉`);
    }
  }
  if (!/TaskKind::AutoBackup/.test(readCode(BACKGROUND_RS))) {
    throw new Error("background.rs 未登记 AutoBackup（登记即执行 / P5 R9）");
  }
  return "两宿主守卫 is_auto_backup_enabled · 已登记任务表";
});

check("★ P7 恢复入口防目录穿越：只接受受管裸文件名（拒绝 / \\ .. 与绝对路径）", () => {
  /* 变异：删掉任一路径护栏 ⇒ 本条变红（恢复入口可读任意文件）。 */
  const code = readCode(BACKUP_RS);
  if (!/fn resolve_managed_backup_path/.test(code)) {
    throw new Error("缺恢复路径解析函数");
  }
  for (const guard of ["contains('/')", "contains('\\\\')", 'contains("..")', "is_managed_backup_name"]) {
    if (!code.includes(guard)) throw new Error(`恢复路径解析缺护栏 ${guard}`);
  }
  return "拒绝 / \\ .. · 只认受管前缀";
});

check("★ P7 边界：损坏文件（非法 JSON）返回可读错误、不崩溃、无副作用", () => {
  const code = readCode(BACKUP_RS);
  if (!/文件不是合法的 JSON/.test(code)) throw new Error("缺「非法 JSON」可读错误文案");
  if (!/fn parse_envelope_reports_readable_error_for_corrupted_file/.test(code)) {
    throw new Error("缺「损坏文件可读错误」测试");
  }
  if (!/fn corrupted_backup_is_rejected_without_side_effects/.test(code)) {
    throw new Error("缺「损坏文件无副作用」测试（导入 / 预览入口）");
  }
  return "可读错误文案 · 损坏文件（解析 + 导入/预览）测试齐备";
});

/* ── P5 命令面参数化 / 作用域安全边界（独立验证补强 · 2026-10-06）──────────────────
 *
 * P5 的 5 条安全边界此前只在 Rust 单测 / `check-api-contract.cjs` 里各验一半；本段把
 * 「fail-closed / capability 默认拒绝 / `id|uid` 统一谓词 / 10 条两宿主同步 / 程序位表驱动」
 * 的**判据**也钉进本门禁，任一侧回退即红。Rust 单测给**行为**证据，本段给**源码接线**回归。
 *
 * ⚠️ 判据只锚**代码**（`readCode` 先剥注释）—— 锚在注释上会被「注释里写着反例」骗过去。
 * ⚠️ 第 ④ 条的 api.rs 一侧是**文件级**「读到了 scope」（与 `check-api-contract.cjs` 第 8 条
 *    同款粗粒度）；逐命令的四处同步由 `check:api` 兜住，本段只锁「恰 40 条 + 后端带 scope」
 *    （2026-10-15：模型禁用两条从 region 升级为 scope、Qoder 账号运维四件 ——
 *     刷新 / 签到状态 / 签到领取 / 额度 —— 一并并入）。
 */

const P5_PRODUCT_RS = "crates/buddy-switch-core/src/modules/product.rs";
const P5_REGION_RS = "crates/buddy-switch-core/src/modules/region.rs";
const P5_ACCOUNT_RS = "crates/buddy-switch-core/src/modules/account.rs";
const P5_PROGRAMS_RS = "crates/buddy-switch-core/src/modules/programs.rs";
const P5_POOL_MOD_RS = "crates/buddy-switch-gateway/src/pool/mod.rs";
const P5_API_TS = "src/lib/api.ts";
const P5_SERVER_RS = "crates/buddy-switch-server/src/api.rs";
const P5_API_CONTRACT_CJS = "scripts/check-api-contract.cjs";

check("★ P5-① `Scope::parse` fail-closed：未知 / 空 / 拼错 ⇒ Err，绝不回落（旧 region 仍宽容）", () => {
  /* 变异①（落点 = product.rs）：把兜底 `other => Err(...)` 改成 `unwrap_or(Scope::WbCn)` 式
     回落 ⇒ 本条变红（正是「拼错 trae-glboal 静默读写 trae-cn 库」的缺陷形态）。 */
  const product = readCode(P5_PRODUCT_RS);
  const m = product.match(/pub fn parse\(s: &str\) -> Result<Scope, ScopeError> \{([\s\S]*?)\n    \}/);
  if (!m) throw new Error("找不到 `Scope::parse -> Result<Scope, ScopeError>`（fail-closed 的前提）");
  const body = m[1];
  for (const lit of ['"wb-cn"', '"wb-global"', '"trae-cn"', '"trae-global"', '"qoder-cn"', '"qoder-global"']) {
    if (!body.includes(lit)) throw new Error(`Scope::parse 缺合法字面量 ${lit}`);
  }
  if (!/other\s*=>\s*Err\(ScopeError::new\(other\)\)/.test(body)) {
    throw new Error("`Scope::parse` 的兜底不是 `Err` —— 可能 fail-open 回落默认作用域");
  }
  if (/unwrap_or/.test(body)) throw new Error("`Scope::parse` 出现 `unwrap_or` 回落");
  if (!/fn scope_parse_is_fail_closed_for_unknown_empty_and_typos/.test(product)) {
    throw new Error("缺「未知 scope ⇒ Err」可证伪单测");
  }
  // 旧 `region` 参数**仍宽容**（P5 明确保留，不在 fail-closed 范围内）。
  const region = readCode(P5_REGION_RS);
  if (!/to_ascii_lowercase\(\)/.test(region)) throw new Error("Region::parse 不再宽容（旧 region 参数会破）");
  if (!/"cn"\s*\|\s*"workbuddy"\s*=>\s*Some\(Region::Cn\)/.test(region)) throw new Error("Region::parse 丢了别名");
  if (!/unwrap_or\(RegionFilter::Cn\)/.test(region)) throw new Error("parse_region_filter 丢了旧默认（缺省⇒CN）");
  return "scope fail-closed · region 仍宽容 · 可证伪单测在";
});

check("★ P5-② capability 默认拒绝：缺能力 ⇒ Err（不靠 UI 隐藏）", () => {
  /* 变异②（落点 = product.rs）：把 `require_capability` 改成「查不到就放行」⇒ 本条变红。 */
  const product = readCode(P5_PRODUCT_RS);
  const body = rustFnBody(product, "require_capability");
  if (body === null) throw new Error("product.rs 缺 `require_capability`");
  if (!/has_capability\(capability\)/.test(body)) throw new Error("未按 `ProductSpec.capabilities` 判定");
  if (!/else\s*\{[\s\S]*?Err\(/.test(body)) throw new Error("缺能力时未返回 Err —— 不是默认拒绝");
  if (/unwrap_or\(true\)/.test(body)) throw new Error("出现 `unwrap_or(true)` 放行兜底");
  if (!/fn require_capability_is_default_deny/.test(product)) throw new Error("缺默认拒绝可证伪单测");
  return "缺能力 ⇒ Err · 无放行兜底 · 单测在";
});

check("★ P5-③ `id|uid` 统一谓词：软删除 / 查找 / 备注共用 `resolve_account_key`（按 uid 批删不再空转）", () => {
  /* 变异③（落点 = account.rs）：把 `trash_account_for`（软删除）的 `resolve_account_key` 换回
     「仅 id」比较 ⇒ 本条变红（uid 选中批删会「报成功、0 条」）。P5 §2.7 起删除 = 软删除，
     故判据锚在 `trash_account_for`（旧 `delete_account_from_path` 已被其取代）。 */
  const account = readCode(P5_ACCOUNT_RS);
  const pred = rustFnBody(account, "resolve_account_key");
  if (pred === null) throw new Error("account.rs 缺 `resolve_account_key`");
  if (!/get\("id"\)/.test(pred) || !/get\("uid"\)/.test(pred) || !/\|\|/.test(pred)) {
    throw new Error("`resolve_account_key` 不是 `id` **或** `uid` 的联合谓词");
  }
  const del = rustFnBody(account, "trash_account_for");
  if (del === null) throw new Error("account.rs 缺 `trash_account_for`（P5 §2.7 软删除入口）");
  if (!/resolve_account_key\(/.test(del)) throw new Error("软删除未走统一谓词 —— 按 uid 批删会静默空转");
  if (/get\("id"\)[\s\S]{0,24}==/.test(del)) throw new Error("软删除仍手写「仅 id」比较（与谓词不一致）");
  if (!/fn delete_by_uid_matches_the_same_predicate_as_find/.test(account)) {
    throw new Error("缺「按 uid 删除」可证伪单测");
  }
  return "谓词 = id||uid · 软删除走谓词 · 按 uid 删除单测在";
});

check("★ P5-④ 参数化命令**恰 40 条**且后端带 scope（commands.rs 签名 + api.rs 读取）", () => {
  /* 变异④（落点 = commands.rs / check-api-contract.cjs）：删掉某参数化命令的 `scope` 参数
     ⇒ 本条变红（桌面端与 webui 行为分叉，而构建全绿）。 */
  const contract = stripComments(fs.readFileSync(path.join(ROOT, P5_API_CONTRACT_CJS), "utf8"));
  const m = contract.match(/const SCOPE_PARAMETERIZED = new Set\(\[([\s\S]*?)\]\);/);
  if (!m) throw new Error("check-api-contract.cjs 找不到 `SCOPE_PARAMETERIZED`");
  const names = [...m[1].matchAll(/"([a-z_]+)"/g)].map((x) => x[1]);
  /* 2026-10-18（issue #47）新增三条：逐账号「今日」只读重取 —— Trae token-today、
     Trae checkin status-map、Qoder checkin status-map。都是**区域纯**的，
     缺 scope 就无从判断该读哪一本账号库 / 网关日志。 */
  const EXPECTED = [
    "account_delete", "account_remark", "accounts_export", "accounts_export_to_path",
    "accounts_import", "accounts_import_local", "accounts_list", "checkin_status",
    "create_qoder_api_key", "credits", "get_disabled_models", "get_qoder_accounts",
    "get_qoder_gateway_models", "get_qoder_local_sources", "get_trae_account_token_today",
    "get_trae_checkin_status_map", "open_data_dir",
    "preview_import_accounts", "probe_cancel", "probe_ledger", "probe_start", "probe_status",
    "qoder_checkin_claim", "qoder_checkin_status", "qoder_checkin_status_map", "qoder_credits",
    "qoder_delete_account", "qoder_gateway_quota", "qoder_gateway_status", "qoder_import_local",
    "qoder_import_pat", "qoder_oauth_start", "qoder_oauth_status",
    "qoder_program_status", "qoder_refresh_account", "qoder_set_account_group",
    "qoder_set_account_remark", "qoder_switch_account", "qoder_usage",
    "set_model_disabled",
  ];
  if (JSON.stringify([...names].sort()) !== JSON.stringify(EXPECTED)) {
    throw new Error(`参数化命令集变了（应恰 40 条）：${JSON.stringify([...names].sort())}`);
  }
  const commands = readCode(COMMANDS_RS);
  const apiTs = readCode(P5_API_TS);
  for (const cmd of names) {
    const mm = new RegExp(`pub\\s+(?:async\\s+)?fn\\s+${cmd}\\s*\\(([^)]*)\\)`).exec(commands);
    if (!mm) throw new Error(`commands.rs 缺参数化命令 \`${cmd}\``);
    if (!/\bscope\s*:/.test(mm[1])) throw new Error(`\`${cmd}\` 的签名没有 \`scope\` 参数`);
    if (!new RegExp(`\\b${cmd}\\b`).test(apiTs)) throw new Error(`api.ts 没有 \`${cmd}\` 的接线`);
  }
  const apiRs = readCode(P5_SERVER_RS);
  if (!/body\.get\s*\(\s*"scope"\s*\)|query_value\s*\([^)]*"scope"/.test(apiRs)) {
    throw new Error("api.rs 未从 query/body 读 scope —— webui 通道拿不到作用域");
  }
  return `${names.length} 条 · commands.rs 带 scope · api.ts 有 · api.rs 读 scope`;
});

/// 从 `programs.rs` 的 `PROGRAMS` 表里解析**有序 id 列表**。
///
/// ★ 安全审计「流程 20」：此前这里（以及 B5）把 id 写死成 3 项，而表里实际有 5 项
/// （`vscode_codebuddy_plugin` / `jetbrains_codebuddy_plugin` 从来没被校验过）——
/// 标题还写着「表 3 项」。写死清单的门禁会**自己过期而没人发现**，所以改成从表里解析。
function programSlotIds(rustSource) {
  const start = rustSource.indexOf("PROGRAMS");
  if (start < 0) throw new Error("programs.rs 里找不到 `PROGRAMS`");
  const end = rustSource.indexOf("];", start);
  if (end < 0) throw new Error("`PROGRAMS` 表没有闭合的 `];`");
  const block = rustSource.slice(start, end);
  const ids = [...block.matchAll(/id:\s*"([a-z0-9_]+)"/g)].map((m) => m[1]);
  if (ids.length === 0) throw new Error("`PROGRAMS` 表里解析不到任何 `id:`");
  return ids;
}

check("★ P5-⑤ 程序位表驱动：4 个入口走 `find(table, id)`，无按 id 硬编码；表内每一项都校验", () => {
  /* 变异⑤（落点 = programs.rs）：把 `status_in` 改成按 id 的 `match` 硬编码 ⇒ 本条变红
     （「加第 3 个程序位无需改代码」被破）。 */
  const programs = readCode(P5_PROGRAMS_RS);
  for (const fn of ["status_in", "switch_in", "detect_in", "install_helper_in"]) {
    const body = rustFnBody(programs, fn);
    if (body === null) throw new Error(`programs.rs 缺 \`${fn}\``);
    if (!/find\(table,\s*id\)/.test(body)) throw new Error(`\`${fn}\` 未走表驱动 \`find(table, id)\``);
    if (/match\s+id\s*\{/.test(body)) throw new Error(`\`${fn}\` 出现按 id 的 match 硬编码`);
  }
  const ids = programSlotIds(programs);
  // 每个 id 必须真的出现在表里（解析来源就是表本身；这里防的是解析口径漂移）。
  for (const id of ids) {
    if (!programs.includes(`"${id}"`)) throw new Error(`PROGRAMS 缺程序位 ${id}`);
  }
  if (!/fn adding_a_slot_needs_no_dispatch_change/.test(programs)) {
    throw new Error("缺「加程序位无需改派发」可证伪单测");
  }
  return `4 入口走 find(table,id) · 无 id 硬编码 · ${ids.length} 程序位（${ids.join(",")}）· 单测在`;
});

check("★ P5 边界：WB 选号基线未动（expiry_urgency_weight=0.0 / deterministic=false）", () => {
  /* 变异⑥（落点 = pool/mod.rs）：改 `PoolConfig::default()` 的任一字段 ⇒ 本条变红
     （WB 选号结果会**静默**改变）。 */
  const pool = readCode(P5_POOL_MOD_RS);
  if (!/expiry_urgency_weight:\s*0\.0/.test(pool)) {
    throw new Error("`expiry_urgency_weight` 默认被改（WB 选号会静默漂移）");
  }
  if (!/deterministic:\s*false/.test(pool)) throw new Error("`deterministic` 默认被改（WB 选号会静默漂移）");
  if (!/fn workbuddy_pick_policy_defaults_are_a_frozen_baseline/.test(pool)) {
    throw new Error("缺选号基线冻结单测");
  }
  return "0.0 / false · 基线冻结单测在";
});

check("★ P5 边界：`BackgroundTask` 表登记 AutoBackup 且**两宿主**都有 spawn 分支", () => {
  /* 变异⑦（落点 = 任一宿主）：删掉某侧 `TaskKind::AutoBackup =>` 分支 ⇒ 本条变红
     （该侧定时备份永不执行）。 */
  if (!/TaskKind::AutoBackup/.test(readCode(BACKGROUND_RS))) {
    throw new Error("background.rs 未登记 AutoBackup");
  }
  for (const host of [TAURI_LIB_RS, SERVER_MAIN_RS]) {
    if (!/TaskKind::AutoBackup\s*=>/.test(readCode(host))) {
      throw new Error(`${host} 的 spawn 缺 \`TaskKind::AutoBackup =>\` 分支`);
    }
  }
  return "AutoBackup 登记 · 两宿主 spawn 分支齐";
});

/* ── P5 §2.7 软删除 + 回收站（**独立验证补强** · 2026-10-06）────────────────────────
 *
 * P5 的软删除 / 回收站是本轮**最晚交付**的一块，此前只有内联单测（与被测函数同文件）。
 * 本段把「软删往返无损 / purge 唯一不可逆 / 留存 30 天+200 / sweep 挂 StartupMaintenance /
 * id|uid 三路径统一 / 四条命令 fail-closed / UI 入口+二次确认 / 损坏容错」的**源码接线**
 * 钉进门禁：任一处回退即红。
 *
 * ⚠️ 判据只锚**代码**（`readCode` 先剥注释）—— 锚注释会被「注释里写着反例」骗过去。
 * ⚠️ 行为证据由**独立集成测试** `crates/buddy-switch-core/tests/p5_trash_verify.rs` 给
 *    （换一个观察点：只走 public API）；本段只锁「接线没被改回去」。
 * ⚠️ 变异落点逐条写在注释里（收尾时逐条打红，见派单「变异纪律」）。
 */

const P5_TRAE_ACCOUNT_RS = "crates/buddy-switch-core/src/modules/trae/account.rs";
const P5_SCHEDULER_RS = "crates/buddy-switch-core/src/modules/scheduler.rs";
const RECYCLE_SHEET = "src/components/recycle-bin-sheet.tsx";
const RECYCLE_ZH = "src/locales/domains/recycleBin.zh.ts";
const RECYCLE_EN = "src/locales/domains/recycleBin.en.ts";
const P5_TRASH_IT = "crates/buddy-switch-core/tests/p5_trash_verify.rs";

check("★ P5 回收站-① trash 落点经 `enum → table` 派生（raw scope 不进 PathBuf）", () => {
  /* 变异（落点 = region.rs）：把 `accounts_trash_file_for` 改成 `store_dir().join(format!("accounts.trash.{}.json", region.as_str()))`
     ⇒ 本条变红（raw region 串进 PathBuf，破坏路径派生唯一规则 §前置 4）。 */
  const region = readCode(P5_REGION_RS);
  const body = rustFnBody(region, "accounts_trash_file_for");
  if (body === null) throw new Error("region.rs 缺 `accounts_trash_file_for`");
  if (!/region_spec\(region\)\.accounts_trash_filename/.test(body)) {
    throw new Error("trash 路径未走 `region_spec(region).accounts_trash_filename`（枚举→查表）");
  }
  if (/region\.as_str\(\)/.test(body) || /format!\([^)]*region\b/.test(body)) {
    throw new Error("trash 路径出现 raw region 字符串拼接 —— 破坏路径派生唯一规则");
  }
  if (!/accounts_trash_filename:\s*"accounts\.trash\.json"/.test(region)) {
    throw new Error("RegionSpec 缺 CN 的 `accounts_trash_filename`");
  }
  if (!/accounts_trash_filename:\s*"accounts\.trash\.global\.json"/.test(region)) {
    throw new Error("RegionSpec 缺 Global 的 `accounts_trash_filename`");
  }
  // 独立验证工件必须在（换观察点的集成测试）。
  if (!fs.existsSync(path.join(ROOT, P5_TRASH_IT))) {
    throw new Error("缺独立集成测试 `tests/p5_trash_verify.rs`（独立验证的观察点）");
  }
  return "enum→table 派生 · CN/Global 分家 · 独立集成测试在";
});

check("★ P5 回收站-② `purge` 是唯一不可逆步；`sweep` 只清**过期**条目", () => {
  /* 变异（落点 = account.rs）：① 把软删除的 `trash.push(TrashEntry{…})` 删掉（回到硬删除）
     ⇒ 本条变红；② 把 `sweep_trash_entries` 的 `expires_at > now` 改成「无条件保留/无条件删」
     ⇒ 本条变红。 */
  const account = readCode(P5_ACCOUNT_RS);
  const soft = rustFnBody(account, "trash_account_for");
  if (soft === null) throw new Error("account.rs 缺 `trash_account_for`");
  if (!/trash\.push\(TrashEntry\s*\{/.test(soft)) {
    throw new Error("软删除未把记录 push 进 trash —— 可能退回硬删除（数据不可恢复）");
  }
  if (!/save_trash_to_path\(/.test(soft)) throw new Error("软删除未把 trash 落盘");
  const purge = rustFnBody(account, "purge_account_for");
  if (purge === null) throw new Error("account.rs 缺 `purge_account_for`");
  if (!/trash\.retain\(/.test(purge)) throw new Error("purge 未从 trash 真删（唯一不可逆步失效）");
  const sweep = rustFnBody(account, "sweep_trash_entries");
  if (sweep === null) throw new Error("account.rs 缺 `sweep_trash_entries`");
  if (!/retain\(\|entry\| entry\.expires_at > now\)/.test(sweep)) {
    throw new Error("sweep 未按 `expires_at > now` 保留 —— 可能误删未过期条目");
  }
  return "软删 push 落盘 · purge retain 真删 · sweep 只清过期";
});

check("★ P5 回收站-③ 留存策略 = 30 天 + 上限 200（WB 与 Trae 两侧同值）", () => {
  /* 变异（落点 = account.rs / trae/account.rs）：改任一常量 ⇒ 本条变红。 */
  for (const [label, src] of [
    ["WB", readCode(P5_ACCOUNT_RS)],
    ["Trae", readCode(P5_TRAE_ACCOUNT_RS)],
  ]) {
    if (!/TRASH_RETENTION_MS:\s*i64\s*=\s*30\s*\*\s*24\s*\*\s*60\s*\*\s*60\s*\*\s*1000/.test(src)) {
      throw new Error(`${label} 侧 \`TRASH_RETENTION_MS\` 不是 30 天`);
    }
    if (!/TRASH_MAX_ENTRIES:\s*usize\s*=\s*200/.test(src)) {
      throw new Error(`${label} 侧 \`TRASH_MAX_ENTRIES\` 不是 200`);
    }
  }
  return "30 天 · 200 条 · 两侧同值";
});

check("★ P5 回收站-④ `sweep` 挂在 `StartupMaintenance`（两宿主注册表 + 调度入口，登记即执行）", () => {
  /* 变异（落点 = 任一宿主 / scheduler.rs）：删掉某侧 `TaskKind::StartupMaintenance =>` 分支、
     或把 `run_startup_maintenance` 里的 `sweep_all_trash()` 删掉 ⇒ 本条变红。
     ⚠️ 这是「登记即执行」纪律 —— 同类事故已出过三次（构建全绿、测试全绿，但生产上没人调用）。 */
  if (!/StartupMaintenance/.test(readCode(BACKGROUND_RS))) {
    throw new Error("background.rs 未登记 `StartupMaintenance`");
  }
  for (const host of [TAURI_LIB_RS, SERVER_MAIN_RS]) {
    const code = readCode(host);
    if (!/TaskKind::StartupMaintenance\s*=>/.test(code)) {
      throw new Error(`${host} 缺 \`TaskKind::StartupMaintenance =>\` spawn 分支`);
    }
    if (!/run_startup_maintenance\(\)/.test(code)) {
      throw new Error(`${host} 的 StartupMaintenance 分支未调 \`run_startup_maintenance()\``);
    }
  }
  const body = rustFnBody(readCode(P5_SCHEDULER_RS), "run_startup_maintenance");
  if (body === null) throw new Error("scheduler.rs 缺 `run_startup_maintenance`");
  // ⚠️ 必须锚 `super::account::sweep_all_trash()`（带前缀）—— 只写 `account::sweep_all_trash()`
  //    会被 Trae 那行 `super::trae::account::sweep_all_trash()` 满足 ⇒ 假通过（删掉 WB 清扫也不报）。
  if (!/super::account::sweep_all_trash\(\)/.test(body)) {
    throw new Error("`run_startup_maintenance` 未调 WB `super::account::sweep_all_trash()` —— 回收站永不自动清理");
  }
  if (!/super::trae::account::sweep_all_trash\(\)/.test(body)) {
    throw new Error("`run_startup_maintenance` 未调 Trae `super::trae::account::sweep_all_trash()`");
  }
  return "两宿主登记 · 调度入口清扫 WB+Trae";
});

check("★ P5 回收站-⑤ 软删 / 恢复 / purge **三条路径**共用 `resolve_account_key`（按 uid 不再空转）", () => {
  /* 变异（落点 = account.rs）：把 `restore_account_for` 或 `purge_account_for` 的**匹配谓词**
     换回「仅 id」⇒ 本条变红（按 uid 恢复 / purge 会静默空转）。旧 check ③ 只钉了软删除。
     ⚠️ 判据锚**匹配谓词表达式**本身（`position(|…| resolve_account_key(…))` /
     `retain(|…| !resolve_account_key(…))`），不锚「函数体里出现过 resolve_account_key」——
     后者会被 `restore_account_for` 的**冲突检查**（`any(|account| resolve_account_key(…)`）满足，
     从而漏掉「匹配谓词被换回 id-only」的缺陷（假通过，已被变异验证钉出）。 */
  const account = readCode(P5_ACCOUNT_RS);
  const expect = {
    trash_account_for: /position\(\|account\| resolve_account_key\(account, key\)\)/,
    restore_account_for: /position\(\|entry\| resolve_account_key\(&entry\.account, key\)\)/,
    purge_account_for: /retain\(\|entry\| !resolve_account_key\(&entry\.account, key\)\)/,
  };
  for (const [fn, re] of Object.entries(expect)) {
    const body = rustFnBody(account, fn);
    if (body === null) throw new Error(`account.rs 缺 \`${fn}\``);
    if (!re.test(body)) {
      throw new Error(`\`${fn}\` 的**匹配谓词**未走 \`resolve_account_key\`（按 uid 会静默空转 / 漏删）`);
    }
    if (/get\("id"\)[\s\S]{0,24}==/.test(body)) {
      throw new Error(`\`${fn}\` 手写「仅 id」比较（与统一谓词不一致）`);
    }
  }
  return "软删 / 恢复 / purge 的匹配谓词都走 resolve_account_key";
});

check("★ P5 回收站-⑥ 四条回收站命令 **fail-closed**：非法 scope ⇒ Err / 4xx（两宿主都带 scope）", () => {
  /* 变异（落点 = commands.rs / api.rs）：把某条命令的 `resolve_scope_param` 换成
     `unwrap_or(Scope::WbCn)` 式回落 ⇒ 本条变红（非法 scope 静默读写错误的库）。 */
  const cmds = readCode(COMMANDS_RS);
  const api = readCode(P5_SERVER_RS);
  const names = ["account_trash_list", "account_restore", "account_purge", "account_trash_clear"];
  for (const cmd of names) {
    const body = rustFnBody(cmds, cmd);
    if (body === null) throw new Error(`commands.rs 缺回收站命令 \`${cmd}\``);
    if (!/resolve_scope_param\(/.test(body)) {
      throw new Error(`\`${cmd}\` 未走 \`resolve_scope_param\`（scope 未 fail-closed）`);
    }
    if (!/require_(wb|trae)_region\(/.test(body)) {
      throw new Error(`\`${cmd}\` 未按平台取 region`);
    }
  }
  for (const cmd of names) {
    const body = rustFnBody(api, `api_${cmd}`);
    if (body === null) throw new Error(`api.rs 缺 handler \`api_${cmd}\``);
    if (!/resolve_scope_param\(/.test(body)) {
      throw new Error(`\`api_${cmd}\` 未走 \`resolve_scope_param\``);
    }
    if (!/BAD_REQUEST/.test(body)) {
      throw new Error(`\`api_${cmd}\` 非法 scope 未返回 4xx`);
    }
  }
  return "4 命令 + 4 handler 都 fail-closed";
});

check("★ P5 回收站-⑦ UI：共用壳 + 两页入口 + purge/清空二次确认 + 显示到期时间", () => {
  /* 变异（落点 = recycle-bin-sheet.tsx / 两页）：删掉入口、删掉二次确认、删掉到期时间
     ⇒ 本条变红（「有数据没入口」中间态 / 假承诺 / 「以为还在」的假期待）。 */
  if (!fs.existsSync(path.join(ROOT, RECYCLE_SHEET))) {
    throw new Error("缺 `recycle-bin-sheet.tsx` —— 软删除对用户零收益（有数据没入口）");
  }
  const sheet = readCode(RECYCLE_SHEET);
  for (const call of ["accountTrashList(scope)", "accountRestore(scope", "accountPurge(scope", "accountTrashClear(scope)"]) {
    if (!sheet.includes(call)) throw new Error(`回收站壳未调 \`${call}\``);
  }
  if (!/setConfirm\(\{\s*kind:\s*"purge"/.test(sheet)) throw new Error("purge 未走二次确认");
  if (!/setConfirm\(\{\s*kind:\s*"clear"/.test(sheet)) throw new Error("清空回收站未走二次确认");
  if (!/entry\.expiresAt/.test(sheet)) throw new Error("回收站未显示到期时间（「以为还在」的假期待）");
  for (const page of [WB_PAGE, TRAE_PAGE]) {
    const code = readCode(page);
    if (!/<RecycleBinSheet\b/.test(code)) {
      throw new Error(`${path.basename(page)} 未接回收站入口`);
    }
    if (!/scope=\{scope\}/.test(code)) {
      throw new Error(`${path.basename(page)} 回收站未传当前 scope`);
    }
  }
  return "壳 4 命令 · purge/清空二次确认 · 到期时间 · 两页接线";
});

check("★ P5 回收站-⑧ trash 文件损坏 ⇒ 空表容错（不 panic / 不清空 live）+ 词条 zh=en", () => {
  /* 变异（落点 = account.rs / locales）：① 把 `load_trash_from_path` 的容错改成 `.unwrap()`
     ⇒ 损坏文件会 panic ⇒ 本条变红；② 删掉某侧某条 recycleBin 词条 ⇒ zh/en 不对齐 ⇒ 变红。 */
  const load = rustFnBody(readCode(P5_ACCOUNT_RS), "load_trash_from_path");
  if (load === null) throw new Error("account.rs 缺 `load_trash_from_path`");
  if (!/Vec::new\(\)/.test(load)) throw new Error("trash 读取失败未回落空表");
  if (/\.unwrap\(\)|\.expect\(/.test(load)) {
    throw new Error("trash 读取用了 unwrap/expect —— 损坏文件会 panic");
  }
  const keys = (rel) =>
    [...readCode(rel).matchAll(/"(recycleBin\.[^"]+)"/g)].map((m) => m[1]).sort();
  const zhKeys = keys(RECYCLE_ZH);
  const enKeys = keys(RECYCLE_EN);
  if (zhKeys.length === 0) throw new Error("recycleBin.zh.ts 无词条（断言失效：是否改名？）");
  if (JSON.stringify(zhKeys) !== JSON.stringify(enKeys)) {
    throw new Error(`recycleBin 词条 zh/en 不一致：zh=${zhKeys.length} 键 / en=${enKeys.length} 键`);
  }
  if (!/domains\/recycleBin\.zh/.test(readCode("src/locales/zh.ts"))) {
    throw new Error("zh.ts 未组合 recycleBin 域");
  }
  if (!/domains\/recycleBin\.en/.test(readCode("src/locales/en.ts"))) {
    throw new Error("en.ts 未组合 recycleBin 域");
  }
  return `损坏容错回落空表 · ${zhKeys.length} 键 zh=en`;
});

/* ── P8 会话工作台（第 6 目的地 + 能力门控 + Trae 结构化空态）────────────────────
 *
 * 上面 83 条覆盖 P0–P11 + P6 + P9 + P7；P8 此前**一条都没有** ⇒「门禁全绿」证明不了
 * 「会话」入口真的受**能力门控**、Trae 直达会话页真的给**含出口**的结构化空态、
 * 且**不**列出 WorkBuddy 的会话组（否则用户会以为「会话已经复制过来了」）。下面锁住它们。
 *
 * ⚠️ 三条纪律（与上面各段一致）：
 *   ① 断言**只锚在代码上**（`readCode` 先剥注释）—— 锚在注释上会被「注释里写着反例」骗过去；
 *   ② 能跑真代码的就跑真代码（`DESTINATION_CAPABILITY` / `hasCapability` 从 `registry.ts`
 *      转译后直接调用）—— 比文本扫描更硬；
 *   ③ 每条都写明**变异落点**，T01 收尾时逐条打红（见各条注释的「变异①/②/③」）。
 *
 * ⚠️ **两套验收，各验各的对象**（P11 口径）：本文件验**真实应用**（`src/`）；
 *    原型套件 `docs/prototype/verify-behaviors.cjs` 验**原型**，**不动**。
 */

const SESSIONS_PAGE = "src/pages/SessionsPage.tsx";
const TRAE_SESSIONS_PAGE = "src/pages/TraeSessionsPage.tsx";
const SESSIONS_LOCALE_ZH = "src/locales/domains/sessions.zh.ts";
const SESSIONS_LOCALE_EN = "src/locales/domains/sessions.en.ts";

check("★ P8 第 6 目的地「会话」存在且**受能力门控**（`sessionCopy`，不是平台分支）", () => {
  /* 变异①（落点 = registry.ts）：把 `DESTINATION_CAPABILITY.sessions` 去掉或改成别的能力
     ⇒ 门控失效 ⇒ 本条变红。 */
  if (!registry.DESTINATIONS.includes("sessions")) {
    throw new Error("`DESTINATIONS` 缺 `sessions` —— 第 6 目的地没加进来");
  }
  if (registry.DESTINATION_CAPABILITY.sessions !== "sessionCopy") {
    throw new Error(
      `\`sessions\` 的能力门控应为 \`sessionCopy\`，实际 ${JSON.stringify(registry.DESTINATION_CAPABILITY.sessions)}`,
    );
  }
  /* 跑**真代码**验证门控效果：WB 有 `sessionCopy` ⇒ 渲染；Trae 无 ⇒ 不渲染。
     （与原型 `DESTINATIONS.filter((d) => !d.cap || caps.includes(d.cap))` 同一判据。） */
  const visibleFor = (product) =>
    registry.DESTINATIONS.filter((dest) => {
      const cap = registry.DESTINATION_CAPABILITY[dest];
      return cap === undefined || registry.hasCapability(product, cap);
    });
  const wb = visibleFor("workbuddy");
  const trae = visibleFor("trae");
  if (!wb.includes("sessions")) throw new Error("WorkBuddy 侧缺「会话」入口（能力门控误伤）");
  if (trae.includes("sessions")) {
    throw new Error("Trae 侧渲染了「会话」入口 —— Trae 无 `sessionCopy`（§3.15 能力边界）");
  }
  return `wb=${wb.length} 项（含 sessions） · trae=${trae.length} 项（无 sessions）`;
});

check("★ P8 侧栏 `scopedNav` 按能力门控过滤（改成不过滤即变红）", () => {
  /* 变异①（落点 = App.tsx）：把 `scopedNav` 改回 `DESTINATIONS.map(...)`（不按能力过滤）
     ⇒ Trae 会渲染「会话」入口 ⇒ 本条变红。⚠️ 只锚在**代码**上（先剥注释）。 */
  const app = readCode(APP_TSX);
  const m = app.match(/const scopedNav\s*=[\s\S]*?\n  \);/);
  if (!m) throw new Error("找不到 `scopedNav` 的 useMemo（断言失效：是否改名？）");
  const block = m[0];
  if (!/DESTINATION_CAPABILITY\[dest\]/.test(block)) {
    throw new Error("`scopedNav` 没有读 `DESTINATION_CAPABILITY` —— 入口不再受能力门控");
  }
  if (!/hasCapability\(product,/.test(block)) {
    throw new Error("`scopedNav` 没有按 `hasCapability` 过滤 —— Trae 会渲染「会话」入口");
  }
  return "scopedNav 走 DESTINATION_CAPABILITY + hasCapability";
});

check("★ P8 Trae 直达会话页 = **结构化空态**（四要素含出口；删出口即变红）", () => {
  /* 变异②（落点 = TraeSessionsPage.tsx）：删掉空态出口（`action={…}`）⇒ 本条变红
     （空态把用户丢在原地，§3.6 四要素缺一不可）。 */
  if (!fs.existsSync(path.join(ROOT, TRAE_SESSIONS_PAGE))) {
    throw new Error(`缺少 \`${TRAE_SESSIONS_PAGE}\``);
  }
  const code = readCode(TRAE_SESSIONS_PAGE);
  if (!/<EmptyState\b/.test(code)) throw new Error("Trae 会话页没有用 `EmptyState`（四要素）");
  for (const k of ["icon", "title", "description", "action"]) {
    if (!new RegExp(`\\b${k}=`).test(code)) throw new Error(`Trae 空态缺要素：${k}`);
  }
  /* 文案口径（§1.3）：说清「不支持」+ **原因**（平台自身的会话存储与云端归属）。 */
  const title = localeValue(SESSIONS_LOCALE_ZH, "sessions.unsupported.title");
  const desc = localeValue(SESSIONS_LOCALE_ZH, "sessions.unsupported.description");
  if (!/暂不支持会话管理/.test(title)) throw new Error(`Trae 空态标题没点明「暂不支持会话管理」：${title}`);
  if (!/云端归属/.test(desc)) throw new Error(`Trae 空态没说明原因（会话存储与云端归属）：${desc}`);
  /* 标题带 `{product}` 占位（跨语言都不写死产品名）；zh + en 同步。 */
  const enTitle = localeValue(SESSIONS_LOCALE_EN, "sessions.unsupported.title");
  if (!/\{product\}/.test(title) || !/\{product\}/.test(enTitle)) {
    throw new Error("Trae 空态标题未用 `{product}` 占位（写死了产品名）");
  }
  if (!code.includes("sessions.unsupported.title") || !code.includes("sessions.unsupported.action")) {
    throw new Error("Trae 空态没有引用 title / action 词条（死键）");
  }
  return `四要素齐备 · 「${title}」+ 原因 · zh/en 同步`;
});

check("★ P8 Trae 会话页**不列 WorkBuddy 会话组**（列出即变红）", () => {
  /* 变异③（落点 = TraeSessionsPage.tsx）：把副本视图（`SessionReplicaView`）/ 会话命令
     搬进 Trae 会话页 ⇒ 本条变红。理由（§3.15 能力边界）：列出 WB 会话会让用户以为
     「会话已经复制过来了」—— 这正是原型 `verify-behaviors.cjs` 单独守的那条边界。 */
  const code = readCode(TRAE_SESSIONS_PAGE);
  for (const bad of ["SessionReplicaView", "session-replica-view", "list_session_replicas", "list_sessions"]) {
    if (code.includes(bad)) {
      throw new Error(`Trae 会话页出现 WorkBuddy 会话组痕迹：${bad}（用户会以为「复制过来了」）`);
    }
  }
  return "无会话组视图 · 无会话命令";
});

check("★ P8 会话页骨架：空态含出口，且无客户端 Tab / 状态筛选（决策 #7 / §1.9）", () => {
  /* 决策 #7：会话复制只有 WorkBuddy 桌面 ⇒ 1 项 ⇒ **不渲染 Tab，也不保留结构**
     （为 1 项写映射表 = 死代码）；§1.9：无组状态 ⇒ 状态筛选整条不渲染。
     即「不出现第二个看起来能切的控件」。 */
  for (const rel of [SESSIONS_PAGE, TRAE_SESSIONS_PAGE]) {
    if (!fs.existsSync(path.join(ROOT, rel))) throw new Error(`缺少 \`${rel}\``);
    const code = readCode(rel);
    if (!/<EmptyState\b/.test(code) || !/action=\{/.test(code)) {
      throw new Error(`${path.basename(rel)} 的结构化空态缺出口（action）`);
    }
    for (const bad of ['role="tablist"', "<Tabs", "sessionFilter", "SESSION_CLIENTS"]) {
      if (code.includes(bad)) {
        throw new Error(`${path.basename(rel)} 出现客户端 Tab / 状态筛选痕迹：${bad}`);
      }
    }
  }
  return "两页空态含出口 · 无 Tab · 无状态筛选";
});

check("★ P8 词条 zh + en 同步（sessions 域逐键对齐，且已组合进词表）", () => {
  const zh = fs.readFileSync(path.join(ROOT, SESSIONS_LOCALE_ZH), "utf8");
  const en = fs.readFileSync(path.join(ROOT, SESSIONS_LOCALE_EN), "utf8");
  const keys = (src) => [...src.matchAll(/"(sessions\.[^"]+|nav\.sessions)"\s*:/g)].map((m) => m[1]);
  const zhKeys = new Set(keys(zh));
  const enKeys = new Set(keys(en));
  const missing = [...zhKeys].filter((k) => !enKeys.has(k));
  if (missing.length) throw new Error(`en 缺 ${missing.length} 个键：${missing.join(", ")}`);
  const extra = [...enKeys].filter((k) => !zhKeys.has(k));
  if (extra.length) throw new Error(`en 多出 ${extra.length} 个键：${extra.join(", ")}`);
  if (!zhKeys.has("nav.sessions")) throw new Error("缺侧栏标签键 `nav.sessions`");
  if (!/domains\/sessions\.zh/.test(readCode("src/locales/zh.ts"))) throw new Error("zh.ts 未组合 sessions 域");
  if (!/domains\/sessions\.en/.test(readCode("src/locales/en.ts"))) throw new Error("en.ts 未组合 sessions 域");
  /* 侧栏标签被 `DEST_META` 引用（死键护栏）。 */
  if (!/sessions:\s*\{\s*labelKey:\s*"nav\.sessions"/.test(readCode(APP_TSX))) {
    throw new Error("`DEST_META` 没有用 `nav.sessions`（死键）");
  }
  return `${zhKeys.size} 键 zh=en 且已组合 · nav.sessions 已引用`;
});

/* ──────────────────────────────────────────────────────────────────────────
 * P8 T02 · 副本只读视图（copy ledger → 连通成组）
 *
 * 数据源是**内部去重账本被提升为产品语义**（`session.rs`）⇒ 下面几条把「诚实标注 /
 * 不用时间冒充内容 / 两宿主同步 / 未知账号不吞 / 函数头注释」钉死。
 * 每条都写明**变异落点**，收尾时逐条打红。
 * ────────────────────────────────────────────────────────────────────────── */

const SESSION_REPLICAS_LIB = "src/lib/session-replicas.ts";
const SESSION_REPLICA_VIEW = "src/components/sessions/session-replica-view.tsx";
const SESSION_RS = "crates/buddy-switch-core/src/modules/session.rs";

check("★ P8 T02 副本视图 `partial: true` 锁死 + shipped 文案固定「仅显示经本应用复制过会话」", () => {
  /* 变异①（落点 = session-replicas.ts）：把 `buildGroupViews` 的 `partial: true` 改成 `false`
     ⇒ 本条变红（诚实性失去类型 / 运行时双重锁定）。
     ⚠️ 只锚**返回值**（`buildGroupViews` 函数体），不锚接口声明 —— 否则改返回值也不会红。 */
  const lib = readCode(SESSION_REPLICAS_LIB);
  const buildFn = lib.match(/export function buildGroupViews[\s\S]*?\n}/);
  if (!buildFn) throw new Error("找不到 `buildGroupViews` 函数体（断言失效：是否改名？）");
  if (!/partial:\s*true/.test(buildFn[0])) {
    throw new Error("`buildGroupViews` 返回值未含 `partial: true` —— 诚实标注（§3.5）失去锁定");
  }
  /* 视图必须**消费** `partial`（否则它是个死字段，改 false 也没人管）。 */
  const view = readCode(SESSION_REPLICA_VIEW);
  if (!/group\.partial/.test(view)) {
    throw new Error("视图没有消费 `group.partial` —— 诚实标注在运行时不可达（死字段）");
  }
  /* 固定文案（§1.4 硬约束 ③）：ledger 只覆盖「经本应用复制过」的会话。 */
  const note = localeValue(SESSIONS_LOCALE_ZH, "sessions.replica.partialNote");
  if (!/仅显示经本应用复制过/.test(note)) {
    throw new Error(`固定文案缺失 / 被改：「${note}」（禁止「全部副本」口径）`);
  }
  /* ★ 口径（lead 裁决 2026-10-06）：**shipped 文案零出现**「全部副本」——
     范围 = `src/locales/**` 的 shipped 词条（zh/en）+ 渲染视图源码（均**剥注释**）。
     ⚠️ **注释 / 门禁负断言 / 设计文档 / 原型不算** —— 注释里「禁止用它」本身就含该词，
        字面「全仓零出现」不可能成立（见 `P8-ARCH.md` §1.4）。 */
  for (const [label, text] of [
    ["sessions.zh（shipped 词条）", readCode(SESSIONS_LOCALE_ZH)],
    ["sessions.en（shipped 词条）", readCode(SESSIONS_LOCALE_EN)],
    ["view（shipped 渲染源码）", view],
  ]) {
    if (/全部副本/.test(text) || /all replicas/i.test(text)) {
      throw new Error(`${label} 出现「全部副本」假承诺（§3.5）`);
    }
  }
  return `partial:true 锁死 · shipped 文案「${note}」· 视图消费 partial`;
});

check("★ P8 不用 `updatedAt` 冒充「落后 / 最新」（**仍**是红线，§3.15.2）", () => {
  /* 变异：用 `updatedAt` 排序 / 派生「落后 / 分叉 / 最新」⇒ 本条变红。
     ⚠️ **本条曾把 `behind` / `diverge` / `latest` 也列为违禁词** —— 那是**过度实现**：
     它的本意是「**禁止用时间派生**」，不是「禁止状态名」。现在这些词是**内容级**状态的
     合法名字（由 jsonl 指纹派生）⇒ 已从违禁表移除；**但红线本身不变**：
     `updatedAt` **仍不得**参与任何状态派生或排序。 */
  const files = [SESSION_REPLICAS_LIB, SESSION_REPLICA_VIEW];
  // 只留「假派生」的命名痕迹（`isLatest` / `isBehind` = 由时间推出的变量名）。
  const forbidden = ["落后", "分叉", "isLatest", "isBehind"];
  for (const rel of files) {
    const code = readCode(rel);
    for (const bad of forbidden) {
      if (code.includes(bad)) {
        throw new Error(`${rel} 出现状态派生痕迹「${bad}」—— 不许用时间冒充内容（§3.15.2）`);
      }
    }
    if (/\.sort\s*\([^)]{0,200}updatedAt/s.test(code)) {
      throw new Error(`${rel} 用 \`updatedAt\` 排序 —— 这就是「最新」的假派生（§1.2）`);
    }
  }
  /* 正向：视图里 `updatedAt` 只能走纯格式化器 `formatDate`，不得参与比较 / 状态。 */
  const view = readCode(SESSION_REPLICA_VIEW);
  if (!/formatDate\(row\.updatedAt\)/.test(view)) {
    throw new Error("视图未把 `updatedAt` 交给 `formatDate` 作纯事实渲染");
  }
  return `无状态派生 · updatedAt 仅经 formatDate 渲染 · 无 updatedAt 排序`;
});

check("★ P8 T02 后端只回 ledger 确有的字段（不臆造 title / updatedAt / hasHistory）", () => {
  /* 变异：在 `replica_groups_from_ledger` 的副本 json! 里加 `title` / `updatedAt` 等 ⇒ 变红。
     （后端**只回** accountUid / sessionId / region，其余由前端 join，§3.1 注。） */
  const code = readCode(SESSION_RS);
  const m = code.match(/fn replica_groups_from_ledger[\s\S]*?\n}/);
  if (!m) throw new Error("找不到 `replica_groups_from_ledger`（断言失效：是否改名？）");
  for (const bad of ["title", "updatedAt", "hasHistory", "cwd"]) {
    if (new RegExp(`"${bad}"`).test(m[0])) {
      throw new Error(`后端副本行臆造了字段 "${bad}"（ledger 里没有，应由前端 join）`);
    }
  }
  for (const good of ["accountUid", "sessionId", "region"]) {
    if (!new RegExp(`"${good}"`).test(m[0])) {
      throw new Error(`后端副本行缺字段 "${good}"`);
    }
  }
  return "只回 accountUid / sessionId / region";
});

check("★ P8 T02 两宿主同步：`list_session_replicas` 四处齐备（缺一即变红）", () => {
  /* 变异：任一侧漏改（桌面端命令 / server 路由 / invoke 登记 / 前端 ROUTES）⇒ 变红。 */
  const checks = [
    ["src-tauri/src/commands.rs", /pub\s+async\s+fn\s+list_session_replicas\s*\(/],
    ["crates/buddy-switch-server/src/api.rs", /async\s+fn\s+api_session_replicas\s*\(/],
    ["crates/buddy-switch-server/src/api.rs", /\.route\s*\(\s*"\/api\/sessions\/replicas"\s*,\s*get\s*\(\s*api_session_replicas\s*\)\s*\)/],
    ["src-tauri/src/lib.rs", /commands::list_session_replicas\b/],
    ["src/lib/api.ts", /list_session_replicas:\s*\{\s*method:\s*"GET",\s*path:\s*"\/api\/sessions\/replicas"\s*\}/],
    ["src/lib/api.ts", /call\s*\(\s*"list_session_replicas"/],
  ];
  for (const [rel, re] of checks) {
    if (!re.test(readCode(rel))) throw new Error(`${rel} 未接上 list_session_replicas（${re}）`);
  }
  return "commands.rs · api.rs（handler+route）· lib.rs · api.ts（ROUTES+call）";
});

check("★ P8 T02 未知账号仍列出（uid 前缀 + 「未知账号」标注，✅ #3）", () => {
  /* 变异⑥（落点 = session-replicas.ts）：uid 不在账号库时**不显示该副本**（吞行）⇒ 变红。
     理由：缺一行会让用户以为「这条副本没有来源」，而实际是「来源账号已删 / 未保存」——
     两者含义完全不同（§1.5）。 */
  const lib = readCode(SESSION_REPLICAS_LIB);
  if (!/sessions\.replica\.unknownAccount/.test(lib)) {
    throw new Error("未对「uid 不在账号库」做 uid 前缀 + 「未知账号」标注（会吞掉副本行）");
  }
  const fallback = localeValue(SESSIONS_LOCALE_ZH, "sessions.replica.unknownAccount");
  if (!/\{uid\}/.test(fallback) || !/未知账号/.test(fallback)) {
    throw new Error(`未知账号文案应含 \`{uid}\` 前缀 + 「未知账号」：${fallback}`);
  }
  /* 反向：不得因为 uid 未知就 `filter` 掉该行（行数恒等于 replicas 数）。 */
  if (/replicas\s*\.\s*filter\s*\(/.test(lib)) {
    throw new Error("`buildGroupViews` 对 replicas 做了 filter —— 未知账号会被吞掉");
  }
  return `标注「${fallback}」· 不 filter 副本行`;
});

check("★ P8 T02 函数头注释存在（**注释文本回归护栏**，删注释即变红）", () => {
  /* 变异③（落点 = session.rs）：删掉 `list_replica_groups_for` 的函数头注释 ⇒ 本条变红。
     理由：否则将来有人**优化去重**会**静默改变 P8 的行为**（最难查的一类缺陷，§1.4 硬约束 ⑤）。
     ⚠️ 这里读**原始文件**（不剥注释）—— 断言的对象就是注释本身。
     ⚠️ **本条不是行为覆盖** —— 注释不是行为：它只能防「注释被误删」，**不能**证明行为正确，
        且可被「随便写一句注释」满足。**真正的保证在代码评审**（见 `P8-ARCH.md` §4 T04）；
        本断言仅作**注释文本回归护栏**，如实描述其能力边界。 */
  const raw = fs.readFileSync(path.join(ROOT, SESSION_RS), "utf8");
  if (!/这是一个内部去重账本被提升为产品语义；改动去重逻辑前先看这里/.test(raw)) {
    throw new Error("`list_replica_groups_for` 缺「内部去重账本被提升为产品语义」函数头注释（✅ #1）");
  }
  return "函数头注释存在（注释文本回归护栏；非行为覆盖）";
});

/* ──────────────────────────────────────────────────────────────────────────
 * P8 T03 · 统一重启确认流程（§3.15.3）
 *
 * 一条流程**跨程序位复用**；**CLI 例外**：CLI 切换**不重开**客户端，但**当前 CLI 会话会中断**
 * ⇒ 文案**禁止**写「自动重开」（写即假承诺，§3.5 红线）。下面把这条钉死，并锁住
 * 「默认勾选 + 未保存提示 + 只有一条流程 + 不出现第二个假控件」。
 * 每条都写明**变异落点**。
 * ────────────────────────────────────────────────────────────────────────── */

const RESTART_CONFIRM_DIALOG = "src/components/restart-confirm-dialog.tsx";
const SWITCH_ACCOUNT_DIALOG = "src/components/switch-account-dialog.tsx";

check("★ P8 T03 CLI 例外**不写「自动重开」**，如实写「当前 CLI 会话会中断」（改文案即变红）", () => {
  /* 变异①（落点 = sessions.zh.ts / sessions.en.ts）：把任一 CLI 文案改成「自动重开」⇒ 本条变红。
     依据：CLI 切换**只更新认证配置、不重开客户端**（`programs.rs` 的 CLI 适配器忽略 `restart`）
     ⇒ 写「自动重开」是假承诺（§3.15.3 / §3.5）。 */
  const zhCli = [
    localeValue(SESSIONS_LOCALE_ZH, "sessions.restart.cliTitle"),
    localeValue(SESSIONS_LOCALE_ZH, "sessions.restart.cliDescription"),
    localeValue(SESSIONS_LOCALE_ZH, "sessions.restart.cliWarning"),
  ].join(" ");
  if (/自动重开/.test(zhCli)) {
    throw new Error(`CLI 文案出现「自动重开」假承诺：${zhCli}`);
  }
  if (!/中断/.test(zhCli)) {
    throw new Error(`CLI 文案未如实写「当前 CLI 会话会中断」：${zhCli}`);
  }
  const enCli = [
    localeValue(SESSIONS_LOCALE_EN, "sessions.restart.cliTitle"),
    localeValue(SESSIONS_LOCALE_EN, "sessions.restart.cliDescription"),
    localeValue(SESSIONS_LOCALE_EN, "sessions.restart.cliWarning"),
  ].join(" ");
  if (/auto[- ]?reopen/i.test(enCli)) throw new Error(`CLI en 文案出现自动重开假承诺：${enCli}`);
  if (!/interrupt/i.test(enCli)) throw new Error(`CLI en 文案未写「interrupt」：${enCli}`);
  /* 纯函数：CLI ⇒ **不渲染**「自动重开」勾选（`autoReopen: false`）；非 CLI ⇒ 默认开（`true`）。
     ⚠️ 只锚**返回值**（`restartConfirmCopy` 函数体），不锚接口声明。 */
  const dialog = readCode(RESTART_CONFIRM_DIALOG);
  const fn = dialog.match(/export function restartConfirmCopy[\s\S]*?\n}/);
  if (!fn) throw new Error("找不到 `restartConfirmCopy` 函数体（断言失效：是否改名？）");
  if (!/autoReopen:\s*false/.test(fn[0])) {
    throw new Error("`restartConfirmCopy` 的 CLI 分支未关掉「自动重开」（应 `autoReopen: false`）");
  }
  if (!/autoReopen:\s*true/.test(fn[0])) {
    throw new Error("`restartConfirmCopy` 的非 CLI 分支未默认「自动重开」（应 `autoReopen: true`）");
  }
  return `CLI 文案无「自动重开」且写「中断」· CLI autoReopen:false / 非 CLI autoReopen:true`;
});

check("★ P8 T03 重启确认默认勾选「自动重开」+ 显式「未保存文件可能丢失」提示（删即变红）", () => {
  const dialog = readCode(RESTART_CONFIRM_DIALOG);
  /* 默认勾选（§3.15.3）：`autoReopen` 初值必须为 `true`。 */
  if (!/const\s*\[\s*autoReopen\s*,\s*setAutoReopen\s*\]\s*=\s*useState\(true\)/.test(dialog)) {
    throw new Error("「自动重开」未默认勾选（应 `const [autoReopen, setAutoReopen] = useState(true)`）");
  }
  /* 必须**显式**提示未保存文件风险；且该提示必须被**引用**（否则是死键 / 提示消失）。 */
  const warn = localeValue(SESSIONS_LOCALE_ZH, "sessions.restart.unsavedWarning");
  if (!/未保存/.test(warn)) throw new Error(`未显式提示「未保存文件可能丢失」：${warn}`);
  if (!/sessions\.restart\.unsavedWarning/.test(dialog)) {
    throw new Error("重启确认框未引用「未保存文件」提示词条（死键 / 风险提示消失）");
  }
  return `默认勾选 useState(true) · 提示「${warn}」`;
});

check("★ P8 T03 所有客户端切换走**同一条**确认流程（WorkBuddy + IDE + CLI + Trae；去掉任一处即变红）", () => {
  /* 变异：把某个客户端的切换点直接调 API（不经确认）/ 删掉该页的 `<RestartConfirmDialog>`
     ⇒ 本条变红。理由：**同一功能在不同客户端行为不一致是缺陷** —— 用户会发现「为什么切 CLI
     会提醒、切 IDE 不提醒」，然后**不再相信这个提醒**（§3.15.3）。 */
  // ① WorkBuddy 桌面（SwitchAccountDialog）
  const sd = readCode(SWITCH_ACCOUNT_DIALOG);
  if (!/<RestartConfirmDialog\b/.test(sd)) throw new Error("WorkBuddy 切换未复用确认流程");
  if (!/isCli=\{/.test(sd)) throw new Error("switch-account-dialog 未传 `isCli`（CLI 例外不可达）");
  if (/自动重开/.test(sd)) {
    throw new Error("switch-account-dialog 自写「自动重开」—— 应复用统一确认流程（会漂移）");
  }
  // ② CodeBuddy IDE + CLI（AccountsPage，共用一个确认框）
  const ap = readCode(WB_PAGE);
  if (!/<RestartConfirmDialog\b/.test(ap)) {
    throw new Error("AccountsPage 未复用确认流程（IDE / CLI 切换未接）");
  }
  if (!/\bisCli\b/.test(ap)) {
    throw new Error("AccountsPage 未使用 `isCli` —— CLI 例外分支不可达（死代码）");
  }
  if (!/function onSwitchCodebuddyCnIde[\s\S]{0,200}setRestartConfirm\(/.test(ap)) {
    throw new Error("CodeBuddy IDE 切换未先打开确认框（直接切换 = 行为不一致）");
  }
  if (!/function onSwitchCodebuddyCli[\s\S]{0,200}setRestartConfirm\(/.test(ap)) {
    throw new Error("CodeBuddy CLI 切换未先打开确认框");
  }
  // ③ TraeWork / TraeCode（TraeAccountsPage）
  const tp = readCode(TRAE_PAGE);
  if (!/<RestartConfirmDialog\b/.test(tp)) {
    throw new Error("TraeAccountsPage 未复用确认流程（TraeWork / TraeCode 切换未接）");
  }
  /* ★ 2026-10-10：Trae 的切换点从**内联箭头**抽成具名函数 `requestSwitchTo`
     —— 因为「切换到此账号」现在**卡片与抽屉两处**都要用，两处各写一遍内联箭头
     正是「同一动作两种实现」的分叉来源（本仓在确认流程 / 保存登录态上都栽过）。
     断言因此拆成两条，证伪能力不变：
       ① 接线在（`onSwitchTo={requestSwitchTo}`）；
       ② 该函数**确实**先打开确认框（改成直接调 API ⇒ 变红）。 */
  if (!/onSwitchTo=\{requestSwitchTo\}/.test(tp)) {
    throw new Error("Trae 的 `onSwitchTo` 未接 `requestSwitchTo`（切换入口失联）");
  }
  if (!/function requestSwitchTo\([\s\S]{0,400}?setSwitchConfirm\(/.test(tp)) {
    throw new Error("Trae 的 `requestSwitchTo` 未先打开确认框（直接切换 = 行为不一致）");
  }
  return "SD(WorkBuddy) + AccountsPage(IDE/CLI) + TraeAccountsPage(Trae) 同一条流程";
});

check("★ P8 T03 重启确认**无客户端 Tab / 选择器**（客户端 1 项 ⇒ 加 Tab 即变红，决策 #7）", () => {
  /* 变异②（落点 = restart-confirm-dialog.tsx）：给确认框加客户端 Tab / 选择器 ⇒ 本条变红。
     依据（§3.15.1 / 决策 #7）：目标客户端由调用方给定（**单客户端**）⇒ 单选项的切换器是假控件；
     「不许出现第二个看起来能切的控件」。 */
  const dialog = readCode(RESTART_CONFIRM_DIALOG);
  for (const bad of ["<Tabs", 'role="tablist"', "TabsList", "TabsTrigger"]) {
    if (dialog.includes(bad)) {
      throw new Error(`重启确认框出现客户端 Tab / 切换器痕迹：${bad}（单选项的假控件）`);
    }
  }
  return "确认框无 Tab / 无客户端选择器";
});

check("★ P8 侧栏徽章：**只读已加载 store** + 三枚（账号/网关/会话）+ 各自 `>0` 才渲染", () => {
  /* ⚠️ **本条曾断言「徽章恒不渲染」** —— 依据是「无组状态 ⇒ 徽章数据源恒空」。
     **那个前提已不成立**：现在有**内容级**组状态（jsonl 指纹派生）⇒ 徽章有真实数据源。
     ⇒ 断言改为守「**别做成假徽章**」这件事本身：
     ① 徽章出自纯函数 `navBadge`，覆盖原型 `renderNav` 的三枚（账号数 / 网关密钥数 / 会话待同步数）；
     ② **只读已加载的 store**（`App` 里不许主动 `load` —— 否则每次开应用都读一遍 jsonl /
        打一次密钥接口）；
     ③ **每个分支都 `> 0` 才给值**（不摆「0」，§3.4 规则 1），其余目的地恒 `null`。 */
  const app = readCode(APP_TSX);
  if (!/pendingSyncCount/.test(app)) throw new Error("侧栏没有待同步计数（behind + diverge）");
  if (!/data-badge=\{item\.dest === "sessions" \? "pending-sync"/.test(app)) {
    throw new Error("会话徽章没有可断言的落点");
  }
  for (const loader of [
    "useSessionsStore.getState().load",
    "useGatewayStore.getState().load",
  ]) {
    if (app.includes(loader)) {
      throw new Error(`\`App\` 里主动加载了数据（${loader}）—— 徽章应只读已加载的 store`);
    }
  }
  const nav = app.match(/const scopedNav\s*=[\s\S]*?\n  \);/);
  if (!nav) throw new Error("找不到 `scopedNav` 的 useMemo（断言失效：是否改名？）");
  if (!/badge:\s*navBadge\(dest\)/.test(nav[0])) {
    throw new Error("scopedNav 没调用 `navBadge` —— 徽章口径又散回各处");
  }
  // 三条数据源 + 三条 `> 0` 门控（缺一条就会出现「0」徽章或挂错条目）。
  const badge = app.match(/function navBadge\([\s\S]*?\n  \}/);
  if (!badge) throw new Error("找不到 `navBadge` 函数体（断言失效：是否改名？）");
  for (const need of ['dest === "accounts"', 'dest === "sessions"', 'dest === "gateway"']) {
    if (!badge[0].includes(need)) throw new Error(`navBadge 缺少分支 ${need}`);
  }
  const zeroGates = (badge[0].match(/> 0 \? \{ n[:,]/g) || []).length;
  if (zeroGates !== 3) {
    throw new Error(`navBadge 里 \`> 0\` 门控只有 ${zeroGates} 处，应 3 处（不摆「0」徽章）`);
  }
  if (!/return null;\s*\n  \}/.test(badge[0])) {
    throw new Error("navBadge 缺少兜底 `return null` —— 概览/用量/设置不该有徽章");
  }
  return "只读已加载 store + 三枚徽章 + 各自 >0 才渲染";
});

/* ──────────────────────────────────────────────────────────────────────────
 * P4 跨作用域迁移向导（T05 独立验证新增 · 2026-10-07）
 *
 * 依据：`docs/specs/P4-MIGRATION-ARCH.md` §1.3（能力边界）/ §1.5（如实汇总）/
 * §4 T05 / §6 决策记录（#1 收窄为 WB 内跨 region · #2 来源只限当前账号 · #4 不可逆提示 · #5 抽屉）。
 *
 * 这些断言锁住向导**最容易回退**的判据（每条都写明变异落点）：
 *   ① 跨产品来源不可点（`aria-disabled` 的 div，不是 button）+ 原因如实；
 *   ② 「已选 N / M」单一计数口径（`treeSelectionStats` 只调一次，树与确认页共用 `stats`）；
 *   ③ `selectAll` 全不选⇒空集 / 全选⇒全部（跑真代码，计数归零/恢复）；
 *   ④ 确认页如实汇总（zh+en 文案 + 数字取自同一 `stats`）；
 *   ⑤ 既有行为未因向导化丢失（doSwitch / 重启确认 / 权限自愈 / 进度双通道）；
 *   ⑥ 来源账号只限「各 scope 当前账号」（决策 #2，不许任选账号）；
 *   ⑦ 迁入 `Sheet`（Radix Dialog ⇒ 焦点陷阱/aria-modal/Esc/焦点归还），不手写；
 *   ⑧ 三步向导结构（step 1/2/3 + 步骤条 + next/back/run/cross）。
 *
 * ⚠️ ⑦ 只锚**结构**（Sheet ← @radix-ui/react-dialog）—— **不证明**运行时的焦点陷阱 /
 * 焦点归还行为（那需要真实浏览器，见 T05 报告的人工步骤）。**本条如实描述其能力边界**。
 * ────────────────────────────────────────────────────────────────────────── */

const WB_ACCOUNTS_LOCALE_ZH = "src/locales/domains/wbAccounts.zh.ts";
const WB_ACCOUNTS_LOCALE_EN = "src/locales/domains/wbAccounts.en.ts";
const SOURCE_ACCOUNT_PICKER = "src/components/switch-account/source-account-picker.tsx";
const MIGRATION_SUMMARY = "src/components/switch-account/migration-summary.tsx";
const SESSION_TREE_LIB = "src/lib/session-tree.ts";

check("★ P4-MIGRATION 跨产品来源**不可点**（`aria-disabled` 的 `<div>`，不是 `<button>`）+ 原因如实", () => {
  /* 变异①（落点 = source-account-picker.tsx）：把「不支持会话复制」分支的 `<div>` 改成 `<button>`
     ⇒ 本条变红。理由：**让用户点一个做不到的入口** = 假承诺（§3.5 / 决策 #1）。
     依据：会话复制绑死 WorkBuddy 数据三件套，Trae 无等价物 ⇒ 跨产品来源后端不支持。 */
  const code = readCode(SOURCE_ACCOUNT_PICKER);
  const start = code.indexOf("if (!option.supportsSessionCopy)");
  const end = code.indexOf("const selected = option.scope === selectedScope;");
  if (start < 0 || end < 0 || end <= start) {
    throw new Error("找不到跨产品来源分支的边界（断言失效：是否改名？）");
  }
  const branch = code.slice(start, end);
  if (/<button\b/.test(branch)) {
    throw new Error("跨产品来源渲染成了 `<button>` —— 用户能点一个做不到的入口（假承诺）");
  }
  if (!/aria-disabled="true"/.test(branch)) {
    throw new Error("跨产品来源缺 `aria-disabled=\"true\"`（可交互性未声明）");
  }
  if (!/<Ban\b/.test(branch)) {
    throw new Error("跨产品来源缺 `Ban` 图标（原因不可见）");
  }
  /* 反向：支持会话复制的来源项**必须**是 button（否则来源无法选择）。 */
  if (!/<button\b/.test(code)) {
    throw new Error("支持会话复制的来源项不是 `<button>`（无法选择来源）");
  }
  /* 文案如实（zh + en 同步）。 */
  const zh = localeValue(WB_ACCOUNTS_LOCALE_ZH, "wbAccounts.wizard.sourceUnsupported");
  if (!/不支持会话复制/.test(zh)) throw new Error(`跨产品原因未如实写「不支持会话复制」：${zh}`);
  if (!/WorkBuddy/.test(zh)) throw new Error(`原因未点明依赖 WorkBuddy 的会话存储：${zh}`);
  const en = localeValue(WB_ACCOUNTS_LOCALE_EN, "wbAccounts.wizard.sourceUnsupported");
  if (!/doesn't support session copy/i.test(en)) throw new Error(`en 原因文案未如实：${en}`);
  return `div[aria-disabled] + Ban · zh「${zh}」`;
});

check("★ P4-MIGRATION 全仓无「已打通」假承诺（注释里的**禁令**不算 —— 那是「禁止写它」）", () => {
  /* 变异②（落点 = wbAccounts.zh.ts / .en.ts 或 source-account-picker.tsx 的**界面文案**）：
     写一句「已打通 WB→Trae」⇒ 本条变红。理由：§6 #1 文案红线（假承诺，§3.5）。
     ⚠️ 本断言只查**去掉注释后的代码 + 词表字面量**：注释里写「不许写已打通」是**护栏**，
        不是承诺 —— 把它算作违规会自相矛盾（见 source-account-picker 头注释）。 */
  const targets = [SOURCE_ACCOUNT_PICKER, WB_ACCOUNTS_LOCALE_ZH, WB_ACCOUNTS_LOCALE_EN];
  const bad = [];
  for (const rel of targets) {
    const code = readCode(rel); // 已剥注释 ⇒ 只留代码与字符串字面量
    if (/已打通|打通了|已经打通/.test(code)) bad.push(rel);
  }
  if (bad.length) throw new Error(`出现「已打通」类假承诺（WB→Trae 未打通）：${bad.join(", ")}`);
  return "代码 + 词表均无「已打通」（注释里的禁令不计）";
});

check("★ P4-MIGRATION 「已选 N / M」单一计数口径（`treeSelectionStats` 只调一次，树与确认页共用同一 `stats`）", () => {
  /* 变异③（落点 = switch-account-dialog.tsx）：让确认页**自己**再算一遍 stats（或另建一个
     `treeSelectionStats` 调用）⇒ 本条变红。理由：两处各算一次 ⇒ 全选/全不选时可能不一致
     （§1.5「确认页数字必须等于实际执行」）。 */
  const dialog = readCode(SWITCH_ACCOUNT_DIALOG);
  const calls = dialog.match(/treeSelectionStats\s*\(/g) || [];
  if (calls.length !== 1) {
    throw new Error(`\`treeSelectionStats\` 被调用 ${calls.length} 次（期望 1 —— 多处各算会漂移）`);
  }
  if (!/const stats\s*=\s*treeSelectionStats\(tree,\s*selected\)/.test(dialog)) {
    throw new Error("未把 `treeSelectionStats(tree, selected)` 赋给单一 `stats`");
  }
  const passCount = (dialog.match(/stats=\{stats\}/g) || []).length;
  if (passCount < 2) throw new Error(`\`stats\` 只传了 ${passCount} 处（期望 ≥2：会话树 + 确认页）`);
  if (!/<SessionTree[\s\S]{0,400}?stats=\{stats\}/.test(dialog)) {
    throw new Error("会话树（第 2 步）未收到共享 `stats`");
  }
  if (!/<MigrationSummary[\s\S]{0,400}?stats=\{stats\}/.test(dialog)) {
    throw new Error("确认页（第 3 步）未收到共享 `stats`");
  }
  /* 确认页**不得**自算（自算 = 第二个口径）。 */
  const summary = readCode(MIGRATION_SUMMARY);
  if (/treeSelectionStats\s*\(/.test(summary)) {
    throw new Error("确认页自己又算了一遍 stats（与第 2 步口径可能漂移）");
  }
  return `treeSelectionStats 1 次 · stats={stats} ${passCount} 处（树 + 确认页）`;
});

check("★ P4-MIGRATION `selectAll` 全不选⇒空集 / 全选⇒全部（**跑真代码**，计数归零/恢复）", () => {
  /* 变异④（落点 = src/lib/session-tree.ts 的 `selectAll`）：把 `if (!on) return new Set()`
     改成 `return new Set(treeSessionIds(tree))`（全不选仍全选）⇒ 本条变红。
     依据：原型 B1 断言 4「会话树可全不选 → 计数归零，再全选恢复」。 */
  const st = loadTs(path.join(ROOT, SESSION_TREE_LIB));
  const tr = (k) => k;
  const sessions = [
    { id: "a1", title: "A1", cwd: "/w/alpha", updatedAt: 1, hasHistory: true, isPlayground: true },
    { id: "b1", title: "B1", cwd: "/w/beta", updatedAt: 2, hasHistory: false, isPlayground: false },
    { id: "b2", title: "B2", cwd: "/w/beta", updatedAt: 3, hasHistory: true, isPlayground: false },
  ];
  const tree = st.buildSessionTree(sessions, tr);
  const none = st.selectAll(tree, false);
  const all = st.selectAll(tree, true);
  if (none.size !== 0) {
    throw new Error(`\`selectAll(tree, false)\` 返回 ${none.size} 个（期望 0 —— 必须归零，不是保留原样）`);
  }
  if (all.size !== 3) throw new Error(`\`selectAll(tree, true)\` 返回 ${all.size} 个（期望 3）`);
  const sNone = st.treeSelectionStats(tree, none);
  const sAll = st.treeSelectionStats(tree, all);
  if (sNone.selected !== 0) throw new Error(`全不选后 selected=${sNone.selected}（期望 0）`);
  if (sAll.selected !== sAll.total) {
    throw new Error(`全选后 selected=${sAll.selected} ≠ total=${sAll.total}（应恢复为满选）`);
  }
  return `selectAll(false)=∅ · selectAll(true)=全部(${all.size}) · 计数 0→${sAll.selected}/${sAll.total}`;
});

check("★ P4-MIGRATION 确认页如实汇总「已选 {n} 个会话 / {d} 个目录」（zh+en，数字取自同一 `stats`）", () => {
  /* 变异⑤（落点 = migration-summary.tsx）：把会话汇总写死成「已选 N 个会话」不随选择变
     ⇒ 本条变红。依据：原型 B1 断言 5 + §1.5「数字必须等于实际执行」。 */
  const zh = localeValue(WB_ACCOUNTS_LOCALE_ZH, "wbAccounts.wizard.confirmSessions");
  if (!/已选/.test(zh) || !/\{n\}/.test(zh) || !/个会话/.test(zh) || !/\{d\}/.test(zh) || !/个目录/.test(zh)) {
    throw new Error(`确认页会话汇总口径不符（应含「已选 {n} 个会话 / {d} 个目录」）：${zh}`);
  }
  const en = localeValue(WB_ACCOUNTS_LOCALE_EN, "wbAccounts.wizard.confirmSessions");
  if (!/\{n\}/.test(en) || !/\{d\}/.test(en) || !/sessions/i.test(en) || !/folders/i.test(en)) {
    throw new Error(`en 确认页会话汇总口径不符：${en}`);
  }
  const summary = readCode(MIGRATION_SUMMARY);
  if (!/stats\.selected/.test(summary) || !/stats\.dirsSelected/.test(summary)) {
    throw new Error("确认页数字未取自 `stats.selected` / `stats.dirsSelected`（可能写死）");
  }
  if (!/confirmSessions/.test(summary)) throw new Error("确认页未引用 `confirmSessions` 词条");
  /* 「影响」行（断言 5 要求含「影响」）+ 决策 #4 不可逆提示。 */
  const impact = localeValue(WB_ACCOUNTS_LOCALE_ZH, "wbAccounts.wizard.impactText");
  if (!/重启/.test(impact)) throw new Error(`影响文案未写「重启」：${impact}`);
  if (!/wbAccounts\.wizard\.impactText/.test(summary)) throw new Error("确认页未引用影响文案（impactText）");
  if (!/wbAccounts\.wizard\.irreversible/.test(summary)) throw new Error("确认页缺决策 #4 的「不可逆」提示");
  return `zh「${zh}」· 数字取 stats.selected/dirsSelected · 含影响 + 不可逆`;
});

check("★ P4-MIGRATION 既有行为未因向导化丢失（doSwitch / 重启确认 / 权限自愈 / 进度双通道）", () => {
  /* 变异⑥（落点 = switch-account-dialog.tsx）：删掉 `<RestartConfirmDialog>` / `isCli={false}` /
     权限自愈轮询 / 进度监听 中任一处 ⇒ 本条变红。理由：**向导化的最大风险是把既有行为弄丢**。 */
  const d = readCode(SWITCH_ACCOUNT_DIALOG);
  const iMig = d.indexOf("migrateAccountData");
  const iSwitch = d.indexOf("switchAccount(");
  if (iMig < 0) throw new Error("`doSwitch` 丢了 `migrateAccountData`（记忆 / 连接器迁移）");
  if (iSwitch < 0) throw new Error("`doSwitch` 丢了 `switchAccount`（切换 / 会话复制）");
  if (!(iMig < iSwitch)) throw new Error("`migrateAccountData` 未在 `switchAccount` 之前（执行顺序变了）");
  if (!/copySessionIds:\s*copySessions\s*\?\s*\[\.\.\.selected\]\s*:\s*undefined/.test(d)) {
    throw new Error("`switchAccount` 未按 `copySessions` 传 `copySessionIds`（会话选择未接线）");
  }
  if (!/<RestartConfirmDialog\b/.test(d)) throw new Error("丢了 `<RestartConfirmDialog>`（统一重启确认）");
  if (!/isCli=\{false\}/.test(d)) {
    throw new Error("`RestartConfirmDialog` 未传 `isCli={false}`（WorkBuddy 桌面是「需重启」客户端）");
  }
  if (/自动重开/.test(d)) throw new Error("向导自写「自动重开」（应复用统一确认流程，会漂移）");
  if (!/permissionDenied\s*\(/.test(d)) throw new Error("丢了权限自愈判定 `permissionDenied`");
  if (!/permission\.denied/.test(d)) throw new Error("丢了结构化错误码 `permission.denied`");
  if (!/setTimeout\(check,\s*2000\)/.test(d)) throw new Error("丢了权限自愈轮询（每 2s）");
  if (!/switch-progress/.test(d)) throw new Error("丢了 Tauri 进度事件 `switch-progress`");
  if (!/switchProgress\(\)/.test(d)) throw new Error("丢了 webui 进度轮询 `switchProgress()`");
  return "doSwitch(migrate→switch) · isCli=false · permission.denied 自愈 · 进度双通道";
});

check("★ P4-MIGRATION 来源账号只限「各 scope 当前账号」（决策 #2，不许任选账号）", () => {
  /* 变异⑦（落点 = switch-account-dialog.tsx）：为来源选择去调 `api.getAccounts()` 让用户任选
     ⇒ 本条变红。理由：后端 `switchAccount` 只收 `sourceRegion`（无 `sourceAccountId`）
     ⇒ 会话来自源 region 的当前账号；允许任选会误导（决策 #2）。 */
  const d = readCode(SWITCH_ACCOUNT_DIALOG);
  if (!/api\.getStatus\(/.test(d)) throw new Error("来源账号未读 `getStatus`（当前登录账号）");
  /* ⚠️ 判据从 `status.current?.(nickname|email|uid)` 放宽为「**从 `status.current` 取名**」：
     B2 把这条裸回落改成了 `identityDisplayName(status.current)` —— 那是**修好了泄露**
     （原写法 nickname 为空时会显示完整邮箱）。本条要守的是「来源 = 当前账号」这个**语义**，
     不是某几个字段名的字面拼写。 */
  if (!/status\.current/.test(d)) {
    throw new Error("未从 `status.current` 取当前账号名（来源 ≠ 当前账号）");
  }
  // 仍须守住：取名必须走脱敏收口，不得回退到裸 email。
  if (/status\.current\?\.(nickname|email|uid)\s*\|\|/.test(d)) {
    throw new Error("来源账号名又退回 `status.current?.nickname || …email` 裸回落（会泄露完整邮箱）");
  }
  if (/api\.getAccounts\(/.test(d)) {
    throw new Error("来源选择调了 `getAccounts` —— 会让用户以为可任选账号（决策 #2 禁止）");
  }
  const picker = readCode(SOURCE_ACCOUNT_PICKER);
  if (/getAccounts|accountId|account-list/i.test(picker)) {
    throw new Error("来源选择器引入了账号库 / `accountId`（越界：来源只限当前账号）");
  }
  return "来源 = getStatus().current；无 getAccounts / accountId 选择";
});

check("★ P4-MIGRATION 迁入 `Sheet`（Radix Dialog ⇒ 焦点陷阱 / aria-modal / Esc / 焦点归还），不手写", () => {
  /* ⚠️ 本条只锚**结构**：Sheet 基于 `@radix-ui/react-dialog`（焦点陷阱为默认能力）。
     **不证明**运行时焦点行为 —— 见 T05 报告的人工步骤。 */
  const d = readCode(SWITCH_ACCOUNT_DIALOG);
  if (!/<Sheet\b/.test(d) || !/<SheetContent\b/.test(d)) throw new Error("向导未用 `Sheet`（抽屉）");
  if (!/from\s*"@\/components\/ui\/sheet"/.test(d)) throw new Error("`Sheet` 不是来自 `ui/sheet` 原语");
  if (/role="dialog"|aria-modal/.test(d)) throw new Error("向导手写了 dialog 语义（应与 `ui/sheet` 漂移）");
  const sheet = fs.readFileSync(path.join(ROOT, "src/components/ui/sheet.tsx"), "utf8");
  if (!/@radix-ui\/react-dialog/.test(sheet)) {
    throw new Error("`ui/sheet` 未基于 `@radix-ui/react-dialog`（无焦点陷阱保证）");
  }
  return "Sheet ← @radix-ui/react-dialog（结构锚；运行时焦点行为需人工验证）";
});

check("★ P4-MIGRATION 三步向导结构（step 1/2/3 + 步骤条 + next/back/run/cross）", () => {
  /* 变异⑧：删掉任一步的条件渲染 / 步骤条 / 前进后退按钮 ⇒ 本条变红。 */
  const d = readCode(SWITCH_ACCOUNT_DIALOG);
  if (!/const \[step,\s*setStep\]\s*=\s*useState<WizardStep>\(1\)/.test(d)) {
    throw new Error("无 `step` 状态（初值应为 1）");
  }
  if (!/<WizardSteps\b/.test(d)) throw new Error("未渲染步骤条 `<WizardSteps>`");
  for (const s of ["step === 1", "step === 2", "step === 3"]) {
    if (!d.includes(s)) throw new Error(`缺 ${s} 的条件渲染`);
  }
  for (const act of ["wiz-next", "wiz-back", "wiz-run"]) {
    if (!new RegExp(`data-act="${act}"`).test(d)) throw new Error(`缺 \`data-act="${act}"\``);
  }
  if (!/data-act="wiz-cross"/.test(readCode(SOURCE_ACCOUNT_PICKER))) {
    throw new Error("缺跨作用域入口 `wiz-cross`（原型 B1 断言 2）");
  }
  return "step 1/2/3 + WizardSteps + next/back/run/cross";
});

check("★ P4-MIGRATION 三层会话树（类型 → 目录 → 会话，**跑真代码**）", () => {
  /* 变异⑨（落点 = src/lib/session-tree.ts 的 `buildSessionTree`）：去掉目录层（`dirs: []`）
     ⇒ 本条变红。依据：原型 B1 断言 3「三层会话树」。 */
  const st = loadTs(path.join(ROOT, SESSION_TREE_LIB));
  const sessions = [
    { id: "a1", title: "A1", cwd: "/w/alpha", updatedAt: 1, hasHistory: true, isPlayground: true },
    { id: "b1", title: "B1", cwd: "/w/beta", updatedAt: 2, hasHistory: false, isPlayground: false },
  ];
  const tree = st.buildSessionTree(sessions, (k) => k);
  if (tree.length !== 2) throw new Error(`类型层应有 2 个节点（对话 / 工作区），实得 ${tree.length}`);
  for (const type of tree) {
    if (!Array.isArray(type.dirs) || type.dirs.length === 0) {
      throw new Error(`类型「${type.label}」缺目录层（退化为两层）`);
    }
    for (const dir of type.dirs) {
      if (!Array.isArray(dir.sessions) || dir.sessions.length === 0) {
        throw new Error(`目录「${dir.label}」缺会话层（退化为两层）`);
      }
    }
  }
  return `类型 ${tree.length} → 目录 ${tree.reduce((n, ty) => n + ty.dirs.length, 0)} → 会话 ${sessions.length}`;
});

/* ── P3 用量四视图 + 概览今日摘要（**独立验证 · T05**）──────────────────────────
 *
 * 上游：`docs/specs/P3-ARCH.md`（§1.4 适配层契约 / §1.5 图注统一出口 / §1.6 空态 / §6 决策 #1–#6）。
 *
 * 与上面的「文本扫描」不同，本段**跑真代码**：
 *   - 纯函数（`usage-charts.ts` / `chart-caption.ts`）经 `loadTs` 求值；
 *   - React 组件经 `loadUsageComponent`（JSX → classic CJS + 桩）**直接调用**并读元素树。
 * 因此「分段和 === 总量」「单向柱是否数据驱动」「昨日=0 不给百分比」是**行为断言**，
 * 不是「文件里有没有某个字符串」。
 *
 * ⚠️ 变异纪律：把缺陷塞回源码 ⇒ 对应用例必须变红（落点见每条注释）。
 */

const USAGE_CHARTS = "src/lib/usage-charts.ts";
const USAGE_CAPTION = "src/lib/chart-caption.ts";
const USAGE_COMPONENTS = "src/components/usage";
const WB_LOCALE_EN = "src/locales/domains/wbStats.en.ts";
const WB_OVERVIEW_ZH = "src/locales/domains/wbOverview.zh.ts";

/** 造一份最小可用的 `TokenStatsSource`（只填被断言用到的字段）。 */
function usageSource(overrides = {}) {
  return {
    source: "workbuddy",
    summary: { total: 0, input: 0, output: 0, cacheRead: 0, cacheWrite: 0, uncachedInput: 0, records: 0, cacheHitRate: null },
    models: [], projects: [], sessions: [], daily: [], dailyByModel: {}, hours: [],
    filesScanned: 0, parseErrors: 0,
    ...overrides,
  };
}

/** 桩组件（保留 displayName ⇒ 可在元素树里定位）。 */
function stubComponent(name) {
  const React = require("react");
  const f = (props) => React.createElement(name, props, props && props.children);
  f.displayName = name;
  return f;
}

/**
 * 把 `.tsx` 组件转成 CJS（**classic JSX**）后求值，喂桩并返回其导出。
 * 与 `loadTs` 的区别：多一个 `jsx` 编译选项 + 一个注入的 `React`（classic 工厂需要它）。
 */
function loadUsageComponent(rel, stubs) {
  const abs = path.join(ROOT, rel);
  const source = fs.readFileSync(abs, "utf8").replace(/import\.meta\.env/g, "__SCOPE_ENV__");
  const js = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
      jsx: ts.JsxEmit.React,
      jsxFactory: "React.createElement",
    },
  }).outputText;
  const React = require("react");
  const mod = { exports: {} };
  const req = (spec) => {
    if (Object.prototype.hasOwnProperty.call(stubs, spec)) return stubs[spec];
    if (spec === "react") return React;
    throw new Error(`未打桩的模块 ${spec}（${rel}）`);
  };
  new Function("module", "exports", "require", "__SCOPE_ENV__", "React", js)(mod, mod.exports, req, {}, React);
  return mod.exports;
}

/** 递归收集 React 元素树（含 children / 数组）。 */
function collectElements(element, out = []) {
  if (!element) return out;
  if (Array.isArray(element)) { for (const e of element) collectElements(e, out); return out; }
  if (typeof element !== "object" || !element.type) return out;
  out.push(element);
  const children = element.props && element.props.children;
  if (Array.isArray(children)) for (const c of children) collectElements(c, out);
  else if (children) collectElements(children, out);
  return out;
}

check("★ P3(独立) 适配层①：堆叠柱每列分段之和 === 当日总量（末段吃余数，跑真代码）", () => {
  /* 变异：`dailyModelSeries` 末段不吃余数（删 `remainder` 那段）⇒ 本条变红（P3-ARCH 契约 3）。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const today = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const key = (offset) => {
    const d = new Date(today.getTime() - offset * 86400000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const src = usageSource({
    // ★ A1：日期取**真实日历日**（今天 / 昨天）。
    //   旧断言写死 "2026-10-01" 并期望 `cols.length === 2` —— 那等于把
    //   「`slice(-range)` 切稀疏数组」的**缺陷**钉成了契约：A1 修好后
    //   窗口恒为 30 天、缺日补 0，列数**必须**是 30 而不是 2。
    //   本条要验的是「分段和 === 总量（含余数）」，与列数无关，故改成只查有数据的两天。
    daily: [
      { key: key(1), total: 30, records: 1 },
      { key: key(0), total: 69.9, records: 2 }, // 分段和 30+39.4=69.4 ≠ 69.9 ⇒ 必须有 0.5 余数
    ],
    dailyByModel: {
      "m-a": [{ key: key(1), total: 10, records: 1 }, { key: key(0), total: 30, records: 1 }],
      "m-b": [{ key: key(1), total: 20, records: 1 }, { key: key(0), total: 39.4, records: 1 }],
    },
  });
  const cols = uc.dailyModelSeries(src, presetWin(30));
  // ★ A1：列数 === 天数窗口（原原型断言 `cols.length === 30`），缺日补 0。
  if (cols.length !== 30) {
    throw new Error(`窗口 30 天应得 30 列（缺日补 0），实得 ${cols.length}`);
  }
  const withData = cols.filter((c) => c.total > 0);
  if (withData.length !== 2) {
    throw new Error(`应有 2 天有数据，实得 ${withData.length}`);
  }
  for (const c of withData) {
    const sum = c.segments.reduce((a, s) => a + s.value, 0);
    if (Math.abs(sum - c.total) > 1e-9) throw new Error(`${c.day} 分段和 ${sum} ≠ 总量 ${c.total}（末段没吃余数）`);
  }
  // 补出来的 0 天：总量 0、分段和也必须是 0（不得凭空冒出分段）。
  for (const c of cols.filter((x) => x.total === 0)) {
    const sum = c.segments.reduce((a, s) => a + s.value, 0);
    if (sum !== 0) throw new Error(`${c.day} 是补出来的 0 天，分段和却为 ${sum}`);
  }
  return `30 列（缺日补 0），有数据的 2 天分段和均 === 总量（含 69.9 的 0.5 余数）`;
});

check("★ P3(独立) 适配层②：热力图 7 行 × 24 列，缺失补 0，非法 key 忽略（跑真代码）", () => {
  /* 变异：`heatmapMatrix` 只出 24 桶（丢掉 weekday 维）⇒ 维度不再是 7×24 ⇒ 本条变红。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  /* `heatmapMatrix` 现在只吃**槽位数组**（两个数据源共用同一份实现）—— 见该函数注释。 */
  const hm = uc.heatmapMatrix([
    { key: "0-9", total: 5 }, // 周一 09 点
    { key: "0-9", total: 1 }, // 同格累加 ⇒ 6
    { key: "6-23", total: 7 }, // 周日 23 点
    { key: "7-3", total: 99 }, // weekday=7 非法 ⇒ 忽略
    { key: "2-24", total: 99 }, // hour=24 非法 ⇒ 忽略
    { key: "oops", total: 99 }, // 非法 key ⇒ 忽略
    { total: 99 }, // 缺 key ⇒ 忽略
  ]);
  if (hm.rows !== 7 || hm.cols !== 24) throw new Error(`维度 ${hm.rows}×${hm.cols}，期望 7×24`);
  if (hm.cells.length !== 7 || hm.cells.some((r) => r.length !== 24)) throw new Error("cells 不是 7×24 矩阵");
  if (hm.cells[0][9] !== 6) throw new Error(`周一 09 点应为 6（5+1），实得 ${hm.cells[0][9]}`);
  if (hm.cells[6][23] !== 7) throw new Error(`周日 23 点应为 7，实得 ${hm.cells[6][23]}`);
  const total = hm.cells.flat().reduce((a, b) => a + b, 0);
  if (total !== 13) throw new Error(`非法 key 未被忽略（总和 ${total} ≠ 13）`);
  return "7×24 · 同格累加 · 非法 key 忽略";
});

check("★ P3(独立) 适配层③：会话散点 x = records（不是 msgs，跑真代码）", () => {
  /* 变异：`sessionPoints` 的 `x` 改成读别的字段（如 0）⇒ 本条变红（§1.4 字段归属）。
     ⚠️ 2026-10-15：`sessionPoints` 增了**必填**的窗口参数 —— 本夹具的会话没有
     `daily`（= 旧后端形态）⇒ 走全时回落路径，x/y 判据不变；窗口口径的行为断言见
     下面「会话统计必须随窗口变化」那一条。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const src = usageSource({ sessions: [{ key: "s1", title: "会话甲", total: 100, records: 7 }] });
  const pts = uc.sessionPoints(src, presetWin(30));
  if (pts.length !== 1) throw new Error(`应得 1 个点，实得 ${pts.length}`);
  if (pts[0].x !== 7) throw new Error(`x 应取 records=7，实得 ${pts[0].x}`);
  if (pts[0].y !== 100) throw new Error(`y 应取 total=100，实得 ${pts[0].y}`);
  const types = fs.readFileSync(path.join(ROOT, "src/lib/types.ts"), "utf8");
  if (/\bmsgs\b/.test(stripComments(types))) throw new Error("types.ts 出现了 `msgs` —— 真实后端没有该字段");
  return "x=records · y=total · types.ts 无 msgs";
});

check("★ P3(独立) 适配层⑤：网关逐日柱同窗对齐，未记录日**不画段**（跑真代码）", () => {
  /* 2026-10-15 新增网关小图（TokenStatsPage）。三条契约：
     ① 列数 = 窗口天数（与主堆叠柱同轴，两天可比）；
     ② **缺日不画段** —— 缺日 = 未记录（App 没在跑），**不是 0**；画 0 段视觉相同，
        故口径必须靠 `gatewayCoverage` + 图注披露（变异：给缺日填 0 段 ⇒ 本条变红）；
     ③ `mergeGatewayTrends` 的求和口径（`region: "all"` 时前端合两区）。
     变异：覆盖率漏算 truncated / 合并 truncated 不取或 ⇒ 本条变红。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const today = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const key = (offset) => {
    const d = new Date(today.getTime() - offset * 86400000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const trend = {
    date: key(0),
    retentionDays: 90,
    total: 100,
    days: [
      { key: key(2), input: 90, output: 10, total: 100, truncated: true },
      // 昨天**缺**（未记录）；今天是**真实 0**（记录器跑过、网关没流量）。
      { key: key(0), input: 0, output: 0, total: 0, truncated: false },
    ],
  };
  const cols = uc.gatewayDailyColumns(trend, presetWin(7));
  if (cols.length !== 7) throw new Error(`窗口 7 天应得 7 列，实得 ${cols.length}`);
  const byDay = new Map(cols.map((c) => [c.day, c]));
  const twoDaysAgo = byDay.get(key(2));
  if (twoDaysAgo.segments.length !== 1 || twoDaysAgo.segments[0].value !== 100) {
    throw new Error(`前天应有 1 段 100，实得 ${JSON.stringify(twoDaysAgo.segments)}`);
  }
  if (twoDaysAgo.segments[0].name !== uc.GATEWAY_SEGMENT) {
    throw new Error("网关段名必须走哨兵 `GATEWAY_SEGMENT`（渲染层翻译，数据不带语言）");
  }
  if (byDay.get(key(1)).segments.length !== 0) throw new Error("未记录日不得画段（它不是 0）");
  if (byDay.get(key(0)).segments.length !== 0) throw new Error("真实 0 也不画段（没有可画的值）");
  const cov = uc.gatewayCoverage(trend, presetWin(7));
  if (cov.recorded !== 2 || cov.total !== 7 || cov.truncated !== 1) {
    throw new Error(`覆盖率应 recorded=2/total=7/truncated=1，实得 ${JSON.stringify(cov)}`);
  }
  const merged = uc.mergeGatewayTrends([
    trend,
    {
      date: key(0),
      retentionDays: 90,
      total: 60,
      days: [
        { key: key(2), input: 40, output: 10, total: 50, truncated: false },
        { key: key(1), input: 5, output: 5, total: 10, truncated: false },
      ],
    },
  ]);
  if (merged.days.length !== 3) throw new Error(`合并后应 3 天（100 天 / 补一天 / 0 天），实得 ${merged.days.length}`);
  if (merged.days[0].key !== key(2) || merged.days[0].total !== 150) {
    throw new Error(`同一天应求和 100+50=150，实得 ${merged.days[0].total}`);
  }
  if (merged.days[0].truncated !== true) throw new Error("任一侧是下界 ⇒ 合并后仍是下界");
  if (merged.total !== 160) throw new Error(`合计应 150+10+0=160，实得 ${merged.total}`);
  if (uc.mergeGatewayTrends([null, null]) !== null) throw new Error("全空合并应为 null（调用方据此不出图）");
  return "7 列同窗 · 缺日/真实 0 不画段 · recorded=2 truncated=1 · 合并求和 160";
});

check("★ P3(独立) 适配层④：余额曲线回推（末值 === currentRemaining）+ 基线非 0（跑真代码）", () => {
  /* 变异①：`baseline` 改成 0 ⇒ 本条「基线非 0」变红；变异②：不按净额回推 ⇒ 末值 ≠ currentRemaining。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const bars = [
    { label: "10/01", inAmt: 0, outAmt: 10 },
    { label: "10/02", inAmt: 0, outAmt: 20 },
    { label: "10/03", inAmt: 0, outAmt: 30 },
  ];
  const pts = uc.balanceCurve(bars, 1000);
  if (pts.length !== 3) throw new Error(`应得 3 个点，实得 ${pts.length}`);
  if (pts[pts.length - 1].value !== 1000) {
    throw new Error(`末值应回推到 currentRemaining=1000，实得 ${pts[pts.length - 1].value}`);
  }
  if (!(pts[0].baseline > 0)) throw new Error(`基线应非 0（高位波动不被压平），实得 ${pts[0].baseline}`);
  if (pts[0].value === pts[pts.length - 1].value) throw new Error("回推序列退化成直线（没按净额回推）");
  if (uc.balanceCurve([], 1000).length !== 0) throw new Error("无逐日点时应返回空（无数据不渲染）");
  if (uc.balanceCurve([{ label: "x", inAmt: 0, outAmt: 0 }], 0).length !== 0) {
    throw new Error("全 0 时应返回空（不画 0 基线假图）");
  }
  return `末值=${pts[pts.length - 1].value} · 基线=${pts[0].baseline}（>0）`;
});

check("★ P3(独立) 适配层⑤：到期分布完整 30 天轴（无到期补 0，已过期剔除，跑真代码）", () => {
  /* 变异：`expiryBuckets` 只出「有到期」的天（跳格）⇒ 长度 ≠ 30 ⇒ 本条变红（契约 5）。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const now = 1_700_000_000_000;
  const DAY = 86_400_000;
  const expiry = {
    ok: true,
    resources: [
      { remaining: 50, expireAt: now + 3 * DAY, expired: false },
      { remaining: 5, expireAt: now - 1000, expired: true },     // 已过期 ⇒ 剔除
      { remaining: 9, expireAt: now + 40 * DAY, expired: false }, // 超 30 天 ⇒ 剔除
      { remaining: 2, expireAt: null, expired: false },           // 无到期 ⇒ 剔除
    ],
  };
  const buckets = uc.expiryBuckets(expiry, now);
  if (buckets.length !== 30) throw new Error(`轴应有 30 格，实得 ${buckets.length}（跳格了）`);
  if (buckets[0].day !== 1 || buckets[29].day !== 30) throw new Error("轴不是 D+1..D+30");
  if (buckets[2].value !== 50) throw new Error(`D+3 应为 50，实得 ${buckets[2].value}`);
  const sum = buckets.reduce((a, b) => a + b.value, 0);
  if (sum !== 50) throw new Error(`过期 / 超期 / 无到期应被剔除（总和 ${sum} ≠ 50）`);
  return "30 格完整轴 · D+3=50 · 过期与超期剔除";
});

check("★ P3(独立) 适配层⑥：mergeSources 聚合**图表数据**（models/dailyByModel/hours 都变多，跑真代码）", () => {
  /* 变异：合并态只换 days、不聚合 ext ⇒ models 仍是 1 ⇒ 本条变红（契约 12）。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const s1 = usageSource({
    models: [{ key: "m-a", total: 10, records: 1 }],
    dailyByModel: { "m-a": [{ key: "2026-10-01", total: 10, records: 1 }] },
    hours: [{ key: "0-9", total: 3 }],
  });
  const s2 = usageSource({
    models: [{ key: "m-b", total: 20, records: 1 }],
    dailyByModel: { "m-b": [{ key: "2026-10-01", total: 20, records: 1 }] },
    hours: [{ key: "0-9", total: 4 }],
  });
  const merged = uc.mergeSources([s1, s2]);
  if (merged.models.length !== 2) throw new Error(`合并后模型数应 2，实得 ${merged.models.length}（只换了 days）`);
  if (Object.keys(merged.dailyByModel).length !== 2) throw new Error("dailyByModel 没被聚合");
  if (merged.hours[0].total !== 7) throw new Error(`hours 没被聚合（${merged.hours[0].total} ≠ 7）`);
  return "models 2 · dailyByModel 2 · hours 3+4=7";
});

check("★ P3(独立) 决策#1：CreditDivergingBar 单向柱**数据驱动**（跑真组件：inAmt>0 ⇒ 双向）", () => {
  /* 变异：把 `showInflow` 写死（`= false` / `= true`）⇒ 本条任一分支变红。
     这正防「看起来对但实际错」—— 单向柱必须是**看数据**得出的，不是硬编码。 */
  const stubs = {
    recharts: {
      Bar: stubComponent("Bar"), BarChart: stubComponent("BarChart"),
      CartesianGrid: stubComponent("CartesianGrid"), ReferenceLine: stubComponent("ReferenceLine"),
      XAxis: stubComponent("XAxis"), YAxis: stubComponent("YAxis"),
    },
    "@/components/ui/chart": { ChartContainer: stubComponent("ChartContainer"), ChartTooltip: stubComponent("ChartTooltip") },
    "@/lib/i18n": { useT: () => (key) => key },
    "./format": { formatCredits: (v) => String(v) },
  };
  const mod = loadUsageComponent(`${USAGE_COMPONENTS}/credit-diverging-bar.tsx`, stubs);
  const barKeys = (bars) => {
    const el = mod.CreditDivergingBar({ bars });
    if (!el) return null;
    return collectElements(el).filter((e) => e.type && e.type.displayName === "Bar").map((e) => e.props.dataKey);
  };
  const twoWay = barKeys([{ label: "d1", inAmt: 50, outAmt: 10 }, { label: "d2", inAmt: 0, outAmt: 20 }]);
  if (!twoWay || !twoWay.includes("up")) throw new Error(`入账>0 却没画入账侧（dataKeys=${twoWay}）—— 硬编码单向`);
  const oneWay = barKeys([{ label: "d1", inAmt: 0, outAmt: 10 }, { label: "d2", inAmt: 0, outAmt: 20 }]);
  if (!oneWay || oneWay.includes("up")) throw new Error(`入账全 0 却仍画入账侧（dataKeys=${oneWay}）`);
  if (barKeys([]) !== null) throw new Error("空数据仍渲染（无数据不渲染）");
  return "inAmt>0 ⇒ up+down；全 0 ⇒ 仅 down；空 ⇒ null";
});

check("★ P3(独立) 决策#1：单向柱图注如实写「入账明细不可得」（zh+en，经统一出口）", () => {
  const zh = localeValue(WB_LOCALE_ZH, "wbStats.usage.caption.singleSide");
  const en = localeValue(WB_LOCALE_EN, "wbStats.usage.caption.singleSide");
  if (!/入账/.test(zh) || !/不可得/.test(zh)) throw new Error(`zh 图注没说明入账不可得：${zh}`);
  if (!/unavailable/i.test(en)) throw new Error(`en 图注没说明入账不可得：${en}`);
  if (!/singleSideCaption\(/.test(readCode("src/pages/CreditStatsPage.tsx"))) {
    throw new Error("CreditStatsPage 没走 `singleSideCaption`（图注可能被手写 ⇒ 会漂移）");
  }
  const cap = loadTs(path.join(ROOT, USAGE_CAPTION));
  if (cap.singleSideCaption((k) => k) !== "wbStats.usage.caption.singleSide") {
    throw new Error("singleSideCaption 没走统一图注键");
  }
  return `zh=${zh} · en=${en}`;
});

check("★ P3(独立) 散点图注**不含「消息数」**（zh+en）+ x 轴语义 = 记录数", () => {
  /* ★ 这是「图不能撒谎」的字段归属红线：真实后端**没有 msgs** ⇒ 图注不许出现「消息数」。
     变异：把 caption 改回「横轴 = 消息数」⇒ 本条变红。 */
  const zh = localeValue(WB_LOCALE_ZH, "wbStats.usage.caption.scatterAxis");
  const en = localeValue(WB_LOCALE_EN, "wbStats.usage.caption.scatterAxis");
  if (/消息数/.test(zh)) throw new Error(`zh 图注出现了「消息数」：${zh}`);
  if (/messages/i.test(en)) throw new Error(`en 图注出现了 messages：${en}`);
  if (!/记录数/.test(zh)) throw new Error(`zh 图注没写「记录数」：${zh}`);
  if (!/records/i.test(en)) throw new Error(`en 图注没写 records：${en}`);
  return `zh=${zh} · en=${en}`;
});

check("★ P3(独立) 决策#2/#3/#4：四项「不渲染」真的没渲染（不是画个空的）", () => {
  /* ① credit-donuts.tsx 文件不存在（决策 #2/#3：两个环形都不画）。 */
  if (fs.existsSync(path.join(ROOT, `${USAGE_COMPONENTS}/credit-donuts.tsx`))) {
    throw new Error("credit-donuts.tsx 仍存在 —— 决策 #2/#3 要求不渲染两个环形");
  }
  /* ② 全仓无「账号 Token 占比」数据源（`accountShare`）。 */
  const shareOffenders = listSourceFiles()
    .filter((abs) => /\baccountShare\b/.test(stripComments(fs.readFileSync(abs, "utf8"))))
    .map((abs) => path.relative(ROOT, abs));
  if (shareOffenders.length) throw new Error(`出现 accountShare（账号占比环形数据源）：${shareOffenders.join(", ")}`);
  /* ③ 用量组件里无「网关状态码分布」环形（用量页不持有该数据源）。 */
  for (const f of fs.readdirSync(path.join(ROOT, USAGE_COMPONENTS)).filter((n) => n.endsWith(".tsx"))) {
    if (/statusCodes?|状态码/.test(stripComments(fs.readFileSync(path.join(ROOT, USAGE_COMPONENTS, f), "utf8")))) {
      throw new Error(`${f} 渲染了状态码分布 —— 决策 #3 要求不渲染`);
    }
  }
  /* ④ 「活跃账号」tile：WB 趋势视图用「模型数」替代（决策 #4）。 */
  const page = readCode("src/pages/TokenStatsPage.tsx");
  if (!/wbStats\.usage\.trend\.models/.test(page)) throw new Error("趋势视图没有「模型数」tile（替代活跃账号）");
  if (/activeAccounts/.test(page)) throw new Error("趋势视图又出现「活跃账号」tile");
  return "credit-donuts 不存在 · 无 accountShare · 无状态码环 · 无活跃账号 tile";
});

check("★ P3(独立) 概览「对比昨日」不编造（跑真组件：昨日=0 ⇒ 「昨日无数据」，无 Infinity/NaN）", () => {
  /* 变异：去掉 `yesterdayTotal <= 0` 守卫（直接除）⇒ 昨日=0 会算出 Infinity/NaN ⇒ 本条变红。
     硬编码 `+12%`（P0-2 禁止）也会被源码扫描抓到。 */
  const src0 = readCode("src/components/overview/overview-stats.tsx");
  if (/\+\s*12\s*%|Infinity|NaN/.test(src0)) throw new Error("源码出现硬编码 +12% / Infinity / NaN");

  const pad = (v) => String(v).padStart(2, "0");
  const dateKey = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const dict = {
    "wbOverview.today.up": localeValue(WB_OVERVIEW_ZH, "wbOverview.today.up"),
    "wbOverview.today.down": localeValue(WB_OVERVIEW_ZH, "wbOverview.today.down"),
    "wbOverview.today.flat": localeValue(WB_OVERVIEW_ZH, "wbOverview.today.flat"),
    "wbOverview.today.noBaseline": localeValue(WB_OVERVIEW_ZH, "wbOverview.today.noBaseline"),
  };
  const t = (key, params) => {
    let out = Object.prototype.hasOwnProperty.call(dict, key) ? dict[key] : `[${key}]`;
    if (params) for (const [k, v] of Object.entries(params)) out = out.replace("{" + k + "}", String(v));
    return out;
  };
  const React = require("react");
  const run = (todayTotal, yesterdayTotal) => {
    const today = dateKey(new Date());
    const yesterday = dateKey(new Date(Date.now() - 86_400_000));
    /* 统计行直吃 `useOverviewMetrics` 归一后的 `OverviewMetrics`（与页面同形状）。 */
    const metrics = {
      loading: false,
      token: {
        days: [
          { key: yesterday, total: yesterdayTotal, records: 1 },
          { key: today, total: todayTotal, records: 1 },
        ],
        today: todayTotal,
        yesterday: yesterdayTotal,
      },
      credits: null,
      gateway: null,
    };
    const stubs = {
      react: {
        ...React,
        useState: (init) => [typeof init === "function" ? init() : init, () => {}],
        useMemo: (fn) => fn(),
        useEffect: () => {},
      },
      "lucide-react": new Proxy({}, { get: () => stubComponent("Icon") }),
      "@/components/usage/stat-tiles": { StatTiles: stubComponent("StatTiles") },
      "@/components/usage/format": { exactTokens: (v) => String(v), compactTokens: (v) => String(v), formatCredits: (v) => String(v) },
      "@/lib/i18n": { useT: () => t },
    };
    const mod = loadUsageComponent("src/components/overview/overview-stats.tsx", stubs);
    const tree = mod.OverviewStats({ metrics, accountCount: 1, attentionCount: 0, accountsLoading: false });
    const el = collectElements(tree).find((e) => e.type && e.type.displayName === "StatTiles");
    if (!el) throw new Error("Token 有量却没渲染 StatTiles（渲染出错）");
    /* 「对比昨日」现挂在「今日 Token」tile 的 `hint`（tile 顺序：账号 / Token / 积分 / 网关）。 */
    return el.props.tiles.map((x) => x.hint);
  };
  const zero = run(100, 0)[1];
  if (/Infinity|NaN|%/.test(zero)) throw new Error(`昨日=0 却给了百分比 / 非有限值：${zero}`);
  if (zero !== dict["wbOverview.today.noBaseline"]) throw new Error(`昨日=0 应显示「${dict["wbOverview.today.noBaseline"]}」，实得 ${zero}`);
  const up = run(112, 100)[1];
  if (!/\+12/.test(up)) throw new Error(`112 vs 100 应算出 +12，实得 ${up}`);
  const down = run(50, 100)[1];
  if (!/-50/.test(down)) throw new Error(`50 vs 100 应算出 -50，实得 ${down}`);
  return `昨日=0 ⇒ ${zero}；112/100 ⇒ ${up}；50/100 ⇒ ${down}`;
});

check("★ P3(独立) 空数据不渲染：每个 recharts 组件空数组 ⇒ 返回 null（不是画空图）", () => {
  /* 变异：删掉某组件的 `length === 0 ⇒ return null` 守卫 ⇒ 本条变红（§3.4 规则 1）。 */
  const files = [
    "stacked-daily-bar", "cumulative-area", "model-donut", "model-ranking",
    "session-scatter", "session-ranking", "credit-diverging-bar", "credit-balance-area", "credit-expiry-bars",
  ];
  for (const f of files) {
    if (!/length === 0\s*\)\s*return null/.test(readCode(`${USAGE_COMPONENTS}/${f}.tsx`))) {
      throw new Error(`${f} 缺「空数组 ⇒ return null」守卫（可能画空图）`);
    }
  }
  /* 热力图是自绘网格（无 length 守卫），由**调用方**按「有数据才渲染」兜底。 */
  if (!/heatmapHasData\s*\?/.test(readCode("src/pages/TokenStatsPage.tsx"))) {
    throw new Error("热力图没有「有数据才渲染」兜底（会画空网格）");
  }
  return `${files.length} 个组件空即 null · 热力图由调用方兜底`;
});

check("★ P3(独立) 两种空态文案不同：真没有 vs 被筛没（zh+en，混用即变红）", () => {
  const noDataZh = localeValue(WB_LOCALE_ZH, "wbStats.usage.empty.noData");
  const filteredZh = localeValue(WB_LOCALE_ZH, "wbStats.usage.empty.filtered");
  const noDataEn = localeValue(WB_LOCALE_EN, "wbStats.usage.empty.noData");
  const filteredEn = localeValue(WB_LOCALE_EN, "wbStats.usage.empty.filtered");
  if (!noDataZh || !filteredZh || noDataZh === filteredZh) throw new Error(`zh 两种空态文案相同：${noDataZh}`);
  if (!noDataEn || !filteredEn || noDataEn === filteredEn) throw new Error(`en 两种空态文案相同：${noDataEn}`);
  const page = readCode("src/pages/TokenStatsPage.tsx");
  if (!/empty\.filtered/.test(page) || !/empty\.viewAll/.test(page)) {
    throw new Error("「被筛没」空态缺「看近 30 天」出口");
  }
  return `zh: 「${noDataZh}」≠「${filteredZh}」`;
});

check("★ P3(独立) 区块级空态**四要素含出口**（整视图空也必须给出口）", () => {
  /* §1.6：一个视图全部图都空 ⇒ 区块级 EmptyState（icon+title+description+**出口**）。
     变异：删掉任一 `<EmptyState>` 的 `action=` ⇒ 本条变红。 */
  const page = readCode("src/pages/TokenStatsPage.tsx");
  const opens = (page.match(/<EmptyState\b/g) || []).length;
  const actions = (page.match(/\baction=/g) || []).length;
  if (opens < 4) throw new Error(`TokenStatsPage 的 <EmptyState> 只有 ${opens} 个（断言失效？）`);
  if (opens !== actions) throw new Error(`<EmptyState> ${opens} 个，但 action= 只有 ${actions} 个 —— 有空态缺出口`);
  return `${opens} 个空态均含出口`;
});

check("★ P3(独立) 趋势视图 ≥4 张图，每张经 `ChartFrame`（role=img + 中文 aria-label 兜底）", () => {
  /* 变异：给趋势视图删一张图 / 去掉 ChartFrame 的 role=img ⇒ 本条变红（契约 1）。 */
  const page = readCode("src/pages/TokenStatsPage.tsx");
  const trend = page.slice(page.indexOf("function TrendView"), page.indexOf("function ModelsView"));
  if (trend.length === 0) throw new Error("找不到 TrendView（断言失效：是否改名？）");
  const frames = (trend.match(/<ChartFrame\b/g) || []).length;
  if (frames < 4) throw new Error(`趋势视图只有 ${frames} 张图（契约 1 要求 ≥4）`);
  const ariaLabels = (trend.match(/ariaLabel=/g) || []).length;
  if (ariaLabels < frames) throw new Error(`有图缺 ariaLabel（${ariaLabels}/${frames}）`);
  const frame = readCode(`${USAGE_COMPONENTS}/chart-frame.tsx`);
  if (!/role="img"/.test(frame)) throw new Error('ChartFrame 没给 role="img"');
  if (!/aria-label=\{ariaLabel\}/.test(frame)) throw new Error("ChartFrame 没把 ariaLabel 落到 role=img 容器上");
  return `趋势 ${frames} 张图 · ChartFrame role=img + aria-label`;
});

check("★ P3(独立) 余额曲线图注注明「回推」+「非 0」（zh+en，经统一出口；基线=0 时不撒谎）", () => {
  /* 契约 11：余额曲线基线不锚 0 ⇒ 图注**必须**写明「非 0」（否则高位波动被读成没变）。
     但 baseline 真的落在 0 时不许硬写「非 0」（那是撒谎）⇒ 走不含该措辞的变体。 */
  const zh = localeValue(WB_LOCALE_ZH, "wbStats.usage.caption.balance");
  const en = localeValue(WB_LOCALE_EN, "wbStats.usage.caption.balance");
  if (!/回推/.test(zh)) throw new Error(`zh 余额图注没注明「回推」：${zh}`);
  if (!/非 0/.test(zh)) throw new Error(`zh 余额图注没注明「非 0」：${zh}`);
  if (!/back-derived/i.test(en)) throw new Error(`en 余额图注没注明 back-derived：${en}`);
  if (!/not zero/i.test(en)) throw new Error(`en 余额图注没注明 not zero：${en}`);
  const cap = loadTs(path.join(ROOT, USAGE_CAPTION));
  const t = (k) => k;
  if (cap.balanceCaption(t, 1, 2, 10) !== "wbStats.usage.caption.balance") {
    throw new Error("baseline>0 时没走含「非 0」的变体");
  }
  if (cap.balanceCaption(t, 1, 2, 0) !== "wbStats.usage.caption.balanceZero") {
    throw new Error("baseline=0 时仍声称「非 0」（撒谎）");
  }
  return `zh=${zh}`;
});

check("★ F5 过期 / 需重登账号不给「手动签到」（卡片菜单与批量签到同口径）", () => {
  /* 原型 F5（`verify-behaviors.cjs:665-670`）：过期账号不给「手动签到」入口。
     ⚠️ 曾经的漏洞：**抽屉**里本就没有该入口（原型断言因此**空转通过**），而**卡片菜单**仍给
     ⇒ 卡片给入口、批量签到却把这类账号算「结构性失败」⇒ **两处口径自相矛盾**。
     本断言锚**卡片菜单的渲染条件**，防它退化回「只看 todayCheckedIn」。 */
  /* ⚠️ 2026-10-18：菜单按原型改成**三态**（已签到 / 凭据失效 / 可点），并且结构抽到了
     `account-more-menu.tsx`（卡片与表格共用同一份）。断言因此改为守住三态的**顺序与门控**：
       已签到 ⇒ 禁用状态行；凭据失效 ⇒ 禁用状态行「需先登录」；其余才给可点的「手动签到」。
     守的是**行为**，不是某段字面量 —— 组件搬家 / 写法调整不该让守门变摆设。 */
  const sources = ["src/components/account-more-menu.tsx", "src/components/account-card.tsx"]
    .map((file) => stripComments(fs.readFileSync(path.join(ROOT, file), "utf8")))
    .join("\n");
  if (!/Boolean\(expired\) \|\| Boolean\(needsRelogin\)/.test(sources)) {
    throw new Error("凭据失效判定丢了 expired / needsRelogin 任一项（F5 违反）");
  }
  const deadBranch = sources.indexOf("credentialDead ? (");
  const checkedBranch = sources.indexOf("todayCheckedIn === true ? (");
  const liveBranch = sources.indexOf("todayCheckedIn === false ? (");
  if (checkedBranch < 0 || deadBranch < 0 || liveBranch < 0) {
    throw new Error("签到三态的分支不完整（可能已重构，请更新本断言）");
  }
  if (!(checkedBranch < deadBranch && deadBranch < liveBranch)) {
    throw new Error("签到三态顺序不对 —— 必须先判「已签到」，再判「凭据失效」，最后才给可点入口");
  }
  const deadSlice = sources.slice(deadBranch, liveBranch);
  if (!/MenuStateItem/.test(deadSlice)) {
    throw new Error("凭据失效分支渲染的是可点入口而不是禁用状态行（F5 违反）");
  }
  if (!/needLogin/.test(deadSlice)) {
    throw new Error("凭据失效分支缺少「需先登录」说明 —— 用户看不出为什么点不了");
  }
  return "expired + needsRelogin 双门控 · 三态顺序正确";
});

check("★ F5（Qoder）：卡片/表格**同一个菜单**，签到三态齐全，且「未知」时仍留入口", () => {
  /* 2026-10-21 实测缺陷：Qoder 的卡片菜单里**没有签到项**（只有「刷新签到状态」），
     用户只能先勾选账号走批量条、或打开抽屉才签得了。原型 `acctMenuInner` 一直是有的 ——
     实现漏了，而没有任何门禁守它。
     本断言守三件事（都是**行为**，不是字面量）：
       ① 三态顺序：已签到 → 凭据不可用 → 可点（与 WB 同口径）；
       ② **未知也必须留入口**：Qoder 的签到态常为「未知」，照搬 WB 的「未知不渲染」
          会让入口时有时无 —— 所以要求有专门的未知态说明；
       ③ 页面与表格**真的接上**了 `onCheckin` / `renderActions`（否则菜单项恒 disabled = 死入口）。 */
  const menu = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/qoder/qoder-account-card.tsx"), "utf8"),
  );
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/QoderAccountsPage.tsx"), "utf8"),
  );
  const checked = menu.indexOf("checkinToday === true ? (");
  const dead = menu.indexOf("credentialDead ? (");
  const live = menu.indexOf("onCheckin?.(account)", dead);
  if (checked < 0 || dead < 0 || live < 0) {
    throw new Error("Qoder 签到三态分支不完整（可能已重构，请更新本断言）");
  }
  if (!(checked < dead && dead < live)) {
    throw new Error("Qoder 签到三态顺序不对 —— 先判已签到，再判凭据不可用，最后才给可点入口");
  }
  const deadSlice = menu.slice(dead, live);
  if (!/MenuStateItem/.test(deadSlice) || !/needLogin/.test(deadSlice)) {
    throw new Error("凭据不可用时要渲染「需先登录」状态行，而不是可点入口");
  }
  if (!/checkinUnknownHint/.test(menu)) {
    throw new Error("签到态未知时没有说明 —— 照搬 WB「未知不渲染」会让入口时有时无");
  }
  if (!/onCheckin=\{\(account\) => void runCheckin\(\[account\]\)\}/.test(page)) {
    throw new Error("Qoder 账号页没把 onCheckin 接给卡片 —— 菜单项会恒 disabled（死入口）");
  }
  if (!/renderActions=\{\(account\) => \(\s*<QoderAccountMenu/.test(page)) {
    throw new Error("表格视图没有接同一个菜单（renderActions）—— 卡片有、表格没有 = 两形态分叉");
  }
  return "三态顺序 · 未知留入口 · 卡片与表格同源";
});

check("★ P1-5 账号卡可键盘聚焦（tabIndex + 语义 role + Enter/Space 有用途）", () => {
  /* 原型 P1-5（`verify-behaviors.cjs:452-457`）：卡片根元素须 `tabindex="0"` 且有 `role`。
     ⚠️ 只加 tabIndex 不加键盘处理 = 「**可聚焦但无操作**」（WCAG 2.4.3 反模式）⇒
     本断言**同时**要求 onKeyDown 接 onOpenDetail，防止"为过断言而只贴一个 tabIndex"。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8"),
  );
  if (!/<article[\s\S]{0,500}?tabIndex=\{0\}/.test(src)) {
    throw new Error("账号卡根元素缺 tabIndex={0} —— 卡片不可 Tab 聚焦（P1-5 违反）");
  }
  if (!/<article[\s\S]{0,500}?role="article"/.test(src)) {
    throw new Error("账号卡根元素缺显式 role（P1-5 违反）");
  }
  if (!/onKeyDown=\{[\s\S]{0,400}?onOpenDetail/.test(src)) {
    throw new Error("卡片可聚焦但无键盘处理 —— 「可聚焦无操作」无障碍反模式");
  }
  return "tabIndex + role + Enter/Space 打开详情";
});

check("★★ 整卡/整行打开详情必须走共用守卫（Portal 菜单项不是 button）", () => {
  /* 2026-10-22 用户报障：「账号卡片的更多操作中，每次点击上面的任何一个操作，侧拉抽屉都被打开」。
     根因不是漏写守卫，而是**守卫判据选错了维度**：
       · Radix `DropdownMenuContent` 经 Portal 挂到 `<body>` ⇒ DOM 上不在卡片内，
         但 React 树里仍在卡片内 ⇒ 冒泡照样回到 `<article onClick>`；
       · 菜单项是 `Primitive.div[role="menuitem"]`，**不是 `<button>`**（`@radix-ui/react-menu`）
         ⇒ 旧的 `closest("button, a, input, …")` 一条也命中不了。
     顺着旧判据打补丁（往选择器里塞 `[role='menuitem']`）治不了根：`select` / `popover` /
     `dialog` 都是同一套 Portal 手法，每加一个控件要再补一次，且**漏了不报错**、只静默开抽屉。
     ⇒ 本断言钉住两件事：
       ① 四处「点整块开详情」都用共用判据 `shouldOpenAccountDetail`；
       ② 不许再手写 `closest("button, …")` 那种判据（它已经错过一次）。 */
  const targets = [
    "src/components/account-card.tsx",
    "src/components/trae-account-card.tsx",
    "src/components/qoder/qoder-account-card.tsx",
    "src/components/account-table.tsx",
  ];
  for (const rel of targets) {
    const src = stripComments(fs.readFileSync(path.join(ROOT, rel), "utf8"));
    if (!/shouldOpenAccountDetail\(/.test(src)) {
      throw new Error(
        `${rel} 打开详情前没过共用判据 \`shouldOpenAccountDetail\` —— Portal 里的菜单项（div[role=menuitem]）会被当成点整卡`,
      );
    }
    if (/closest\(\s*["'`]button,/.test(src)) {
      throw new Error(
        `${rel} 又手写了 \`closest("button, …")\` 判据 —— 该判据拦不住 Portal 菜单项（2026-10-22 报障根因），请用 \`shouldOpenAccountDetail\``,
      );
    }
  }
  const guard = stripComments(
    fs.readFileSync(path.join(ROOT, "src/lib/account-open-guard.ts"), "utf8"),
  );
  if (!/currentTarget\.contains\(target\)/.test(guard)) {
    throw new Error(
      "account-open-guard 缺 `currentTarget.contains(target)` —— 这是拦 Portal 内容的唯一稳定判据",
    );
  }
  for (const role of ["menuitem", "button"]) {
    if (!new RegExp(`role='${role}'`).test(guard)) {
      throw new Error(
        `account-open-guard 的豁免选择器缺 \`[role='${role}']\` —— ${role} 不是 <button> 标签，只列标签名会漏`,
      );
    }
  }
  return "四处同源 · 拦 Portal 弹层 + 非 button 控件";
});

check("★ P3 会话散点：四侧留边 + ZAxis range 存在（防气泡溢出绘图区）", () => {
  /* 审计 #12：原型要求「气泡不溢出绘图区」，真实由 recharts 布局保证 —— **此前无断言**。
     ⚠️ 本断言只锚**静态配置**（四侧 margin + ZAxis range 存在）；
     **「视觉上真的不溢出」仍需人工看图**（无头环境验不了）。
     ⚠️ 另注：`h-[220px]` 配 `range=[60,420]` 的**像素半径**上限偏大，**已在 §G 标注待视觉复核**。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/usage/session-scatter.tsx"), "utf8"),
  );
  const m = src.match(/margin=\{\{([^}]*)\}\}/);
  if (!m) throw new Error("会话散点没有 margin 留边 —— 气泡会贴边 / 溢出");
  for (const side of ["top", "right", "bottom", "left"]) {
    if (!new RegExp(`${side}\\s*:`).test(m[1])) {
      throw new Error(`会话散点 margin 缺 ${side} —— 该侧无留边`);
    }
  }
  if (!/ZAxis[\s\S]{0,200}?range=\{\[/.test(src)) {
    throw new Error("会话散点缺 ZAxis range —— 气泡半径不可控（可能溢出）");
  }
  return "四侧留边 + ZAxis range 存在（视觉仍需人工）";
});

check("★ 会话视图必须**截断渲染**（会话可达 1.5 万，全量渲染要等数秒）", () => {
  /* ## 为什么必须有这条
     会话 = 一个 jsonl 一条，实测重用户 1.5 万条。原型契约 8（「排行条数 = 气泡数 =
     会话数」）按**几十**量级写的，全量渲染 = 2×N 个 DOM 节点 + N 个散点
     （实测 1.5 万行纯 innerHTML + 强制布局 0.69s，React 重建 + recharts 还要翻数倍）。
     ⇒ 有意偏离契约 8：只渲染前 `SESSION_RENDER_LIMIT` 个，并**显式说明共多少个**。
     没有这条门禁，下一个人读到 P3-ARCH 契约 8 会「顺手改回去」，性能问题静默回归。

     变异：① 删掉 `points.slice(0, SESSION_RENDER_LIMIT)` ⇒ 变红；
           ② 把某个图改回吃全量 `points`（两图不同源）⇒ 变红；
           ③ 把「会话数」tile 改成吃切片 ⇒ 变红（数字会随渲染上限漂移）。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const limit = uc.SESSION_RENDER_LIMIT;
  if (!Number.isInteger(limit) || limit <= 0 || limit > 500) {
    throw new Error(`SESSION_RENDER_LIMIT 非法（${JSON.stringify(limit)}）—— 必须是 ≤500 的正整数`);
  }
  const page = readCode("src/pages/TokenStatsPage.tsx");
  if (!/points\.slice\(0,\s*SESSION_RENDER_LIMIT\)/.test(page)) {
    throw new Error("SessionsView 没有按 SESSION_RENDER_LIMIT 切片 —— 又会全量渲染 1.5 万条");
  }
  for (const chart of ["SessionRanking", "SessionScatter"]) {
    if (!new RegExp(`<${chart}\\s+points=\\{visible\\}`).test(page)) {
      throw new Error(`${chart} 没吃切片后的 \`visible\`（排行与散点可能不同源）`);
    }
  }
  /* 统计 tile 必须仍按**全量**算 —— 否则「会话数 / 总 Token / 均值」会随渲染上限漂移。 */
  if (!/exactTokens\(points\.length\)/.test(page)) {
    throw new Error("「会话数」tile 不是按全量 points 算的（数字会随渲染上限漂移）");
  }
  return `limit=${limit} · 两图同源切片 · tile 仍按全量`;
});

check("★ 会话统计必须随窗口变化（2026-10-15 用户报障「切时间不变」，跑真代码）", () => {
  /* ## 报障与根因
     用户：「会话的统计数据切换时间没有变化」。根因是 SessionsView 不消费 `viewWindow`，
     `sessionPoints` 只吃全时合计 —— 而趋势 / 模型两视图早已按窗口切片，
     「同一参数条只有会话不动」既是谎报也是口径分裂。
     修法：后端下发 `sessions[].daily`（逐日拆分，与文件级 daily 同判据）+
     `sessionPoints(source, window)` 按窗口键求和。

     ## 变异对照（任一都会变红）
     ① `sessionPoints` 忽略窗口 / 忽略 daily ⇒ 前四条行为断言红；
     ② 把「无 daily ⇒ 回落全时」改成「无 daily ⇒ 计 0」⇒ 回落断言红（旧后端会整屏消失）；
     ③ SessionsView 不把 `viewWindow` 传进 `sessionPoints` ⇒ 末尾静态断言红。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const today = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const key = (offset) => {
    const d = new Date(today.getTime() - offset * 86400000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const src = usageSource({
    sessions: [
      {
        key: "s-today",
        title: "今天的会话",
        // 全时总量 = 逐日之和（与真实数据同构）；窗口切片的正确性由 daily 决定。
        total: 100,
        records: 5,
        daily: [
          { key: key(0), total: 40, records: 2 },
          { key: key(10), total: 60, records: 3 },
        ],
      },
      {
        key: "s-old",
        title: "十天前的会话",
        total: 5,
        records: 1,
        daily: [{ key: key(10), total: 5, records: 1 }],
      },
    ],
  });

  // 今天：只有 s-today 在窗口内，且只算今天那一段。
  const todayPts = uc.sessionPoints(src, { kind: "today" });
  if (todayPts.length !== 1 || todayPts[0].id !== "s-today" || todayPts[0].y !== 40 || todayPts[0].x !== 2) {
    throw new Error(`「今天」窗口应得仅 s-today（40 / 2 条），实得 ${JSON.stringify(todayPts)}`);
  }
  // 近 7 天：s-old（10 天前）仍在窗口外 ⇒ 集合不变、数值不变（该会话今天以外的贡献不计）。
  const weekPts = uc.sessionPoints(src, presetWin(7));
  if (weekPts.length !== 1 || weekPts[0].id !== "s-today" || weekPts[0].y !== 40) {
    throw new Error(`「近 7 天」应得仅 s-today（40），实得 ${JSON.stringify(weekPts)}`);
  }
  // 近 30 天：两个会话都进窗，s-today 变为窗口内合计（40+60 / 2+3）。
  const monthPts = uc.sessionPoints(src, presetWin(30));
  const sToday = monthPts.find((point) => point.id === "s-today");
  if (monthPts.length !== 2 || !sToday || sToday.y !== 100 || sToday.x !== 5) {
    throw new Error(`「近 30 天」应得 2 个会话、s-today=100/5，实得 ${JSON.stringify(monthPts)}`);
  }
  // 旧后端回落：没有 daily ⇒ 全时口径（不编 0、也不炸）。
  const legacy = usageSource({
    sessions: [{ key: "s-legacy", title: "旧后端", total: 77, records: 4 }],
  });
  const legacyPts = uc.sessionPoints(legacy, { kind: "today" });
  if (legacyPts.length !== 1 || legacyPts[0].y !== 77 || legacyPts[0].x !== 4) {
    throw new Error(`无 daily 必须回落全时口径，实得 ${JSON.stringify(legacyPts)}`);
  }
  // 静态：组件必须真的把窗口喂进去（缺了它，上面修好的纯函数永远不被窗口驱动）。
  const page = readCode("src/pages/TokenStatsPage.tsx");
  if (!/sessionPoints\(source,\s*viewWindow\)/.test(page)) {
    throw new Error("SessionsView 的 sessionPoints 没吃 viewWindow —— 会话统计又会不随窗口变化");
  }
  if (!/<SessionsView[\s\S]{0,300}?viewWindow=\{viewWindow\}/.test(page)) {
    throw new Error("SessionsView 没收到 viewWindow prop —— 组件内无从消费");
  }
  return "今天/近7天/近30天 集合与数值正确 · 旧后端回落 · 组件吃到窗口";
});

check("★ P3 权限自愈：授权恢复后**自动重试**上次切换（且只重试一次）", () => {
  /* 原型 P3 要求「恢复即自动重试切换」。此前实现只 `setError("")` 清错误、**不重试**
     ⇒ 用户仍要手动再点一次（而文件头注释却写着「并自动恢复」—— **注释描述了不存在的行为**）。
     ⚠️ 本断言**同时**锁「只重试一次」：缺了它，重试再失败 ⇒ 又进轮询 ⇒ **无限循环**。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/switch-account-dialog.tsx"), "utf8"),
  );
  if (!/permissionRetriedRef/.test(src)) {
    throw new Error("权限自愈缺「只重试一次」的护栏 —— 可能无限循环");
  }
  if (!/lastRestartRef\.current = restart/.test(src)) {
    throw new Error("没记住用户上次选的 restart —— 自动重试无从发起");
  }
  const m = src.match(/if \(res\.ok\)[\s\S]{0,700}?\n\s{8}\}/);
  if (!m) throw new Error("找不到授权成功的分支（可能已重构，请更新本断言）");
  if (!/doSwitch\(/.test(m[0])) {
    throw new Error("授权恢复后没有自动重试切换 —— 用户仍要手动再点一次（P3 违反）");
  }
  return "恢复即自动重试 + 只一次护栏";
});

check("★ P0-4a 未检测程序位：「去设置」跳到「权限与诊断」子项（不是设置页首页）", () => {
  /* 原型 P0-4a（`verify-behaviors.cjs:365-370`）：点「去设置」后须能到「权限与诊断」。
     ⚠️ 此前只 `navigate(.../settings)`（**首页**）⇒ 用户还得自己找子项。
     本断言同时要求 `SettingsPage` **从 URL 读初始 section** —— 否则带了参数也不生效。 */
  const acct = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/AccountsPage.tsx"), "utf8"),
  );
  if (!/settings\?section=diagnostics/.test(acct)) {
    throw new Error("「去设置」未带子项锚点 —— 只到设置页首页（P0-4a 违反）");
  }
  const settings = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/SettingsPage.tsx"), "utf8"),
  );
  if (!/useSearchParams/.test(settings) || !/searchParams\.get\("section"\)/.test(settings)) {
    throw new Error("SettingsPage 没从 URL 读 section —— 带了 ?section= 也不生效");
  }
  /* 合法性校验有两条合法路径：本页内联 `SETTINGS_SUBNAV.some(...)`，或走共享的
     `parseSettingsSubnav`（fail-closed 在 `settings-subnav.tsx` 里）。
     ⚠️ 后者是两平台共用的正解 ⇒ 断言必须**穿透到被调函数**，否则「把校验搬走」
     会让这条门禁变成空壳。 */
  if (!/SETTINGS_SUBNAV\.some/.test(settings) && !/parseSettingsSubnav\(/.test(settings)) {
    throw new Error("SettingsPage 没校验 section 合法性 —— 非法值会被塞进 state");
  }
  if (/parseSettingsSubnav\(/.test(settings)) {
    const nav = stripComments(
      fs.readFileSync(path.join(ROOT, "src/components/settings/settings-subnav.tsx"), "utf8"),
    );
    if (!/export function parseSettingsSubnav/.test(nav)) {
      throw new Error("parseSettingsSubnav 不存在（校验被搬走却没落地）");
    }
    if (!/SETTINGS_SUBNAV\.some/.test(nav) || !/:\s*"general"/.test(nav)) {
      throw new Error("parseSettingsSubnav 未 fail-closed 回 general");
    }
  }
  return "带锚点 + 从 URL 读 + 合法性校验";
});

check("★ 设置页两平台同构：同一张 9 项子导航表 + URL 承载 + split 布局（PLAN §3.9 / D12）", () => {
  /* 变异①：把 Trae 设置页退回「一屏长滚动 10 组」⇒ 子导航断言变红。
     变异②：Trae 页自建一份子导航表 ⇒ 「同一张表」断言变红。
     变异③：两页任一缺 `data-split-layout` ⇒ 900px 以下不转纵向、子导航不换行。 */
  for (const page of ["src/pages/SettingsPage.tsx", "src/pages/TraeSettingsPage.tsx"]) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, page), "utf8"));
    if (!/<SettingsSubnav\b/.test(code)) throw new Error(`${page} 没有接设置子导航`);
    if (!/useState<SettingsSubnavId>/.test(code)) throw new Error(`${page} 子导航选中态未接线`);
    if (!/parseSettingsSubnav\(/.test(code)) {
      throw new Error(`${page} 未走 parseSettingsSubnav（URL 承载 + fail-closed 的唯一实现）`);
    }
    if (!/data-split-layout/.test(code)) {
      throw new Error(`${page} 缺 data-split-layout（900px 以下不会转纵向）`);
    }
    /* 主区间距 = 原型 `.split-main { gap:16px }`；写 `space-y-10/12` 是改造前的长滚动间距。 */
    if (!/space-y-4\b/.test(code)) throw new Error(`${page} 主区间距不是原型的 16px`);
  }
  const trae = stripComments(fs.readFileSync(path.join(ROOT, "src/pages/TraeSettingsPage.tsx"), "utf8"));
  if (/SETTINGS_SUBNAV\s*(:|=)/.test(trae)) {
    throw new Error("Trae 设置页自建了子导航表 —— 两平台必须共用 SETTINGS_SUBNAV");
  }
  /* 子导航版式逐值对齐原型 `.subnav`：宽度/断点在 CSS，按钮内只有文字（原型无图标）。 */
  const subnav = stripComments(fs.readFileSync(path.join(ROOT, "src/components/shell/side-subnav.tsx"), "utf8"));
  if (/icon/.test(subnav)) throw new Error("SideSubnav 带了图标 —— 原型 `.subnav` 只有文字");
  if (!/data-side-subnav/.test(subnav)) throw new Error("SideSubnav 缺 data-side-subnav（断点规则的宿主）");
  const css = fs.readFileSync(path.join(ROOT, "src/index.css"), "utf8");
  if (!/\[data-side-subnav\]/.test(css)) throw new Error("index.css 缺 [data-side-subnav] 规则（假宿主）");
  if (!/\[data-side-subnav\] \{[^}]*width:\s*186px/.test(css)) {
    throw new Error("[data-side-subnav] 宽度不是原型的 186px");
  }
  return "两页同一张表 · URL 承载 · split 布局 · 16px 间距 · 186px 无图标子导航";
});

check("★ P1-2 批量签到部分失败：toast 给「只重试失败项」出口", () => {
  /* 原型 P1-2：部分失败态不能只给一句 toast —— 要能**只重试失败项**。
     此前 `batchCheckin` 只统计 `failed` 计数、**丢掉失败 id** ⇒ 用户只能把成功的一起重跑。
     ⚠️ 本断言**同时**锁「共用同一条签到路径」（`runCheckinFor`）——
     否则重试会另写一份逻辑，两处口径迟早漂移。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/AccountsPage.tsx"), "utf8"),
  );
  if (!/failedIds\.push\(/.test(src)) {
    throw new Error("批量签到没保留失败 id —— 无从「只重试失败项」");
  }
  if (!/checkinRetryFailed/.test(src)) {
    throw new Error("toast 没有「只重试失败项」出口（P1-2 违反）");
  }
  if (!/async function runCheckinFor/.test(src)) {
    throw new Error("没有抽出共用的签到路径 —— 重试会另写一份（口径漂移风险）");
  }
  if (!/batchCheckin\(failedIds\)/.test(src)) {
    throw new Error("「只重试失败项」没把失败 id 传回去 —— 实际仍会重跑全部");
  }
  return "失败 id 保留 + toast 出口 + 共用路径";
});

check("★ P12 请求代理：默认关（D29）+ 一键还原不留残留", () => {
  /* 原型断言 1 / 5（`verify-behaviors.cjs:1312-1322, 1351-1357`）。
     真实应用的「默认关」由 core 的 `is_enabled()` **fail-closed** 保证（读不到 = 关）；
     「一键还原」= `set_enabled(false)` **删文件**（不留残留）—— 两者都有 Rust 单测覆盖。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/proxy.rs"), "utf8"),
  );
  if (!/fn read_enabled_at[\s\S]{0,300}?return false/.test(src)) {
    throw new Error("proxy.rs 的读取不是 fail-closed —— 读不到状态时必须视为**关闭**");
  }
  if (!/fn write_enabled_at[\s\S]{0,500}?remove_file/.test(src)) {
    throw new Error("一键还原没删文件 —— 会留残留（违反「无残留」）");
  }
  return "fail-closed + 删文件";
});

check("★ P12 面板：风险 2 条 + 不劫持 4 条**逐条**出现（不能压缩成一句）", () => {
  /* 原型断言 2（`verify-behaviors.cjs:1324-1332`）—— 四条「不劫持」必须**逐条列出**。
     ⚠️ 本断言防的是「把四条压成一句『我们不劫持』」—— 那就不满足"逐条声明"。 */
  const zh = fs.readFileSync(path.join(ROOT, "src/locales/domains/proxy.zh.ts"), "utf8");
  for (const key of [
    "proxy.risk.traffic",
    "proxy.risk.noBody",
    "proxy.noHijack.never",
    "proxy.noHijack.hosts",
    "proxy.noHijack.dns",
    "proxy.noHijack.transparent",
  ]) {
    if (!zh.includes(key)) throw new Error(`词条缺 ${key} —— 风险 / 不劫持声明不全`);
  }
  const panel = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/proxy-panel.tsx"), "utf8"),
  );
  for (const key of [
    "proxy.noHijack.never",
    "proxy.noHijack.hosts",
    "proxy.noHijack.dns",
    "proxy.noHijack.transparent",
  ]) {
    if (!panel.includes(key)) throw new Error(`面板没渲染 ${key} —— 不劫持四条必须逐条列出`);
  }
  return "风险 2 + 不劫持 4 逐条";
});

check("★ P12 面板：拦截方式 + 白名单表 + 兜底行「直连」", () => {
  /* 原型断言 3 / 4（`verify-behaviors.cjs:1334-1349`）——
     兜底行必须是「其它全部域名 → **直连**」（**不是「也代理」**）。 */
  const zh = fs.readFileSync(path.join(ROOT, "src/locales/domains/proxy.zh.ts"), "utf8");
  if (!zh.includes("proxy.domains.fallback") || !zh.includes("proxy.domains.actionDirect")) {
    throw new Error("词条缺兜底行 / 直连 —— 白名单外必须**直连**");
  }
  const panel = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/proxy-panel.tsx"), "utf8"),
  );
  if (!/data-act="proxy-fallback"/.test(panel)) {
    throw new Error("面板没有兜底行（data-act=proxy-fallback）");
  }
  if (!/proxy\.mode\.cRedirect/.test(panel)) {
    throw new Error("面板没显示「拦截方式」—— C 路要看得见");
  }
  return "拦截方式 + 白名单 + 兜底直连";
});

check("★ P12 诚实红线：`verified` 恒 false + 文案不许说「运行中」", () => {
  /* ⚠️ **本断言守的是「不许夸大」**：应用**无法校验**代理是否生效
     （env 是**启动期**读取的，跨进程读环境不可靠），所以文案必须是
     「**已按引导设置**（**应用不校验**）」，**不许**写「运行中」或任何
     暗示「已验证生效」的措辞（`P12-ARCH.md` T04 补充 §①）。 */
  const rs = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/proxy.rs"), "utf8"),
  );
  if (!/"verified":\s*false/.test(rs)) {
    throw new Error("proxy.rs 没把 verified 固定为 false —— 会暗示「已验证」");
  }
  const zh = stripComments(fs.readFileSync(path.join(ROOT, "src/locales/domains/proxy.zh.ts"), "utf8"));
  if (!/已按引导设置/.test(zh)) throw new Error("状态文案没用「已按引导设置」");
  if (!/应用不校验/.test(zh)) throw new Error("没说明「应用不校验」—— 不许让人以为验证过");
  // ⚠️ 必须**剥注释**后再查「运行中」——本文件注释里就有「而非『运行中』」这句
  //    （正是"注释骗过扫描"的老问题；首版断言没剥，被自己的注释判红）。
  if (/运行中/.test(zh)) throw new Error("出现「运行中」—— 暗示已验证生效（假承诺）");
  return "verified=false + 已按引导设置 + 应用不校验";
});

check("★ P0 程序位注册表：5 个程序位全部 available + 显示名两两不同（planned 机制保留、当前为空）", () => {
  /* 原型断言 `verify-behaviors.cjs:1297-1302` 的两条口径**仍然有效**，只是对象变了：
     ① 「注册表声明了 planned ⇒ 卡片必须有对应样式」（机制不许删）；
     ② 「planned 的说明必须是『尚未支持』、不许用『未检测到客户端』」。
     ★ 2026-10-09：`PLAN.md` §3.3 B 表的两条「待新增」（VS Code / JetBrains 内 CodeBuddy 插件）
     已落地 ⇒ `planned` 列表**清空**。此时**不许**为了凑断言硬造一条 planned（那会编造出
     不存在的「计划」）⇒ 断言改为钉住「已无 planned 条目」+「机制仍在」+「两条新程序位可用」。 */
  const reg = stripComments(
    fs.readFileSync(path.join(ROOT, "src/lib/platforms/registry.ts"), "utf8"),
  );
  // ① 机制保留：`planned` 是「尚未支持」的唯一表达方式，代码与列表都要在。
  if (!/PLANNED_PROGRAMS/.test(reg)) throw new Error("registry.ts 丢了 planned 机制（PLANNED_PROGRAMS）");
  if (!/status:\s*"available"\s*\|\s*"planned"/.test(reg)) {
    throw new Error("registry.ts 的 ProgramSpec 丢了 `planned` 取值（机制不许删）");
  }
  // ② 当前**不应**再有 planned 条目（两条目标均已落地为 available）。
  if (/status:\s*"planned"/.test(reg)) {
    throw new Error("registry.ts 仍有 status=planned 条目 —— 两条目标已落地，应为 available");
  }
  // ③ 两个新程序位必须以 `available` 出现，且 label_key 与后端 `programs.rs` 逐字一致。
  for (const [id, key] of [
    ["vscode_codebuddy_plugin", "wbAccounts.program.vscodeCodebuddyPlugin"],
    ["jetbrains_codebuddy_plugin", "wbAccounts.program.jetbrainsCodebuddyPlugin"],
  ]) {
    const entry = reg.match(new RegExp(`\\{[^}]*id:\\s*"${id}"[^}]*\\}`));
    if (!entry) throw new Error(`registry.ts 缺少程序位 ${id}`);
    if (!entry[0].includes(key)) {
      throw new Error(`${id} 的 labelKey 不是 ${key}（与后端 label_key 漂了）`);
    }
    if (!/status:\s*"available"/.test(entry[0])) {
      throw new Error(`${id} 未标为 available —— 后端已实现 status/switch/detect`);
    }
  }

  // ④ 卡片仍渲染 planned 机制（列表为空 ⇒ 实际不出按钮，但代码不许删）。
  const card = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8"),
  );
  if (!/PLANNED_PROGRAMS\.map/.test(card)) {
    throw new Error("卡片没渲染 planned 程序位（原型 :1297-1299：机制不许删）");
  }
  // ⚠️ **只查 `.map` 不够** —— 首版就栽在这：删掉「使用处 `{plannedControls}`」后断言仍绿。
  const row = card.match(/const programRow = \([\s\S]{0,900}?\);/);
  if (!row || !/plannedControls/.test(row[0])) {
    throw new Error("programRow 没渲染 plannedControls —— 机制声明了但卡片上没有对应样式");
  }
  /* ⚠️ 实测缺陷回归门禁：程序位到 5 个后，4 个「当前账号」胶囊 + 1 个按钮**不换行就溢出卡片**，
     最后一个槽位（JetBrains）被挤出可视区。⇒ 程序位行必须允许换行。 */
  if (!/flex w-full flex-wrap items-center/.test(row[0])) {
    throw new Error("programRow 缺 `w-full` + `flex-wrap` —— 5 个程序位会溢出卡片、末尾槽位被裁掉");
  }

  // ⑤ 措辞红线仍在（机制里的 planned 说明只能叫「尚未支持」）。
  const zh = stripComments(
    fs.readFileSync(path.join(ROOT, "src/locales/domains/wbAccounts.zh.ts"), "utf8"),
  );
  if (!/尚未支持/.test(zh)) throw new Error("planned 的说明没说「尚未支持」");
  for (const line of zh.split("\n").filter((l) => l.includes("program.planned"))) {
    if (/未检测到客户端/.test(line)) {
      throw new Error("planned 误用了「未检测到客户端」—— 会引导用户去装（原型 :1302 反向禁止）");
    }
  }

  // ⑥ ★ 显示名两两不同：`vscode_cn_inject`（只读槽）与 `vscode_codebuddy_plugin`（可切换槽）
  //    都含 VS Code，名字若相同用户分不清点哪个（这正是落地前存在过的冲突）。
  const nameOf = (file, key) => {
    const src = stripComments(fs.readFileSync(path.join(ROOT, file), "utf8"));
    const m = src.match(new RegExp(`"${key.replace(/\./g, "\\.")}":\\s*"([^"]*)"`));
    if (!m) throw new Error(`${file} 缺少词条 ${key}`);
    return m[1];
  };
  for (const file of [
    "src/locales/domains/wbAccounts.zh.ts",
    "src/locales/domains/wbAccounts.en.ts",
  ]) {
    const names = [
      "wbAccounts.program.codebuddyCnIde",
      "wbAccounts.program.codebuddyCli",
      "wbAccounts.program.vscodeCnInject",
      "wbAccounts.program.vscodeCodebuddyPlugin",
      "wbAccounts.program.jetbrainsCodebuddyPlugin",
    ].map((k) => nameOf(file, k));
    if (new Set(names).size !== names.length) {
      throw new Error(`${path.basename(file)} 程序位显示名有重复：${names.join(" / ")}`);
    }
  }
  return "5 个程序位全 available · 无 planned 条目 · 机制保留 · 显示名各异";
});

check("★ 概览待处理：新原型三项（凭据已过期 / 今日未签到 / 积分即将过期）+ 行内可执行动作", () => {
  /* 依据 `docs/prototype/buddy-switch-prototype.html` 的 `viewOverview`（:2058-2069 / :2092-2098）：
     ① 待处理要覆盖「凭据已过期」「今日未签到」「积分即将过期」三类（旧实现只有登录态到期）；
     ② 「今日未签到」的行内动作是**可执行**的（`run: "batch-checkin"`）—— 不是一律跳转，
        否则「概览答得出有事、却要用户自己去列表页找」； */
  const page = readCode("src/pages/OverviewPage.tsx");
  for (const key of ['key: "expired"', 'key: "notChecked"', 'key: "expiringCredits"']) {
    if (!page.includes(key)) throw new Error(`概览待处理缺 ${key}（原型 :2065-2067）`);
  }
  if (!/wbOverview\.pending\.action\.checkin/.test(page)) {
    throw new Error("「今日未签到」缺一键签到出口（原型 `run: batch-checkin`）");
  }
  if (!/function runOneClickCheckin/.test(page)) {
    throw new Error("找不到 `runOneClickCheckin` —— 行内动作没有实现");
  }
  // ★ 行内动作必须**两种形态都在**（跳转 + 就地执行），且就地执行有可断言落点。
  if (!/data-pending-run=/.test(page)) throw new Error("就地执行按钮没有可断言的落点");
  if (!/item\.action\.to \?/.test(page) || !/item\.action\.run/.test(page)) {
    throw new Error("行内动作只实现了跳转或只有就地执行（原型 `.srow` 两种形态都要）");
  }
  // 部分失败必须给「只重试失败项」出口（与账号页批量签到同一纪律）。
  if (!/wbOverview\.toast\.checkinRetryFailed/.test(page)) {
    throw new Error("一键签到的部分失败态没给「只重试失败项」出口");
  }
  for (const file of ["zh", "en"]) {
    const loc = readCode(`src/locales/domains/wbOverview.${file}.ts`);
    for (const key of [
      "wbOverview.pending.expired",
      "wbOverview.pending.notChecked",
      "wbOverview.pending.expiringCredits",
      "wbOverview.pending.action.checkin",
      "wbOverview.toast.checkinSummary",
    ]) {
      if (!loc.includes(`"${key}"`)) throw new Error(`wbOverview.${file}.ts 缺词条 ${key}`);
    }
  }
  return "三类新待处理 + 一键签成就地执行 + 部分失败可重试（zh/en 齐）";
});

check("★ B5 签到积分：抽屉里能看到「+N」+ 连签 + 日期（数据来自上游**实测**字段）", () => {
  /* 原型断言 B5（`verify-behaviors.cjs:561-572`）要求抽屉里出现 `+100`（WB）/ `+50`（Trae）。
     ⚠️ 这条**曾被我误判为「卡后端」**：依据是「代码里没有金额字段」，
     但**没去看上游实际返回什么**。实测 `checkin-activity-status` 的 `data` **本来就有**
     `daily_credit` / `today_credit` / `total_credits` / `streak_days` / `checkin_dates`，
     而 `checkin.rs` 早就透传了 `raw` ⇒ 缺的是「**显式提取 + 前端展示**」，**不是数据源**。 */
  const rs = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/checkin.rs"), "utf8"),
  );
  for (const f of ["dailyCredit", "todayCredit", "totalCredits", "streakDays", "checkinDates"]) {
    if (!rs.includes(`"${f}"`)) throw new Error(`checkin.rs 没提取 ${f} —— 前端无从展示「+N」`);
  }
  const drawer = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-detail-drawer.tsx"), "utf8"),
  );
  if (!/checkin\.todayCredit/.test(drawer)) {
    throw new Error("抽屉没渲染「+N」—— 原型 B5 要求抽屉里出现 +100 / +50");
  }
  const zh = stripComments(
    fs.readFileSync(path.join(ROOT, "src/locales/domains/shared.zh.ts"), "utf8"),
  );
  for (const k of [
    "shared.drawer.checkin.title",
    "shared.drawer.checkin.today",
    "shared.drawer.checkin.streak",
  ]) {
    if (!zh.includes(k)) throw new Error(`词条缺 ${k}`);
  }
  return "提取 5 字段 + 抽屉渲染 +N + 词条";
});

check("★ B5 逐条「+N」：签到流水每条显示金额（取不到则「—」，不猜）", () => {
  /* 覆盖度报告曾把这条记为「⛔ 卡后端：CheckinEvent 无金额字段 ⇒ 推不出」。
     实际数据源**已在**：提交前的 `checkin-activity-status` 返回 `daily_credit` / `today_credit`
     （与 Trae 侧「预检返回的 credits 就是本次签到可得的额度」同源，见 `trae/checkin.rs`）。
     ⇒ 缺的只是**把额度落进签到日志**并**逐条透传到 events**；本断言锁死这条接线。
     ★ 反向：额度取不到时必须显示「—」而非「+0」（不猜、不撒谎）。 */
  const rs = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/checkin.rs"), "utf8"),
  );
  if (!/fn checkin_reward_from_status/.test(rs)) throw new Error("checkin.rs 缺额度提取器");
  if (!/entry_map\["amount"\]/.test(rs)) throw new Error("签到成功没把额度写进日志（amount）");

  const usage = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/credit_usage.rs"), "utf8"),
  );
  if (!/amount: Option<f64>/.test(usage)) throw new Error("CheckinEvent 没有 amount 字段");
  if (!/"amount": event\.amount/.test(usage)) throw new Error("events 没透传签到 amount");

  const types = stripComments(fs.readFileSync(path.join(ROOT, "src/lib/types.ts"), "utf8"));
  if (!/CreditStatsCheckinEvent[\s\S]{0,400}?amount\?: number \| null/.test(types)) {
    throw new Error("types.ts 的 CreditStatsCheckinEvent 缺 amount");
  }
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/CreditStatsPage.tsx"), "utf8"),
  );
  if (!/formatCredits\(reward\)/.test(page)) throw new Error("积分流水没渲染逐条「+N」");
  if (!/reward === null \? "—"/.test(page)) throw new Error("额度缺失时没回落「—」");
  return "额度落日志 + events 透传 + 类型 + 逐条渲染 + 缺失回落「—」";
});

check("★ P8 组状态：**内容级**派生（四态）+ 禁止用 updatedAt 冒充落后/最新", () => {
  /* 原型要求「组状态由成员派生（有分叉→有分歧；有落后→待同步；否则已一致）」。
     ⚠️ 数据源曾被标为「❌ 无」（`P8-ARCH.md` §1.2）—— 但那**只看 copy ledger**，
     **漏了本地会话 jsonl**（`~/.workbuddy/projects/<ws>/<cid>.jsonl`，每行一条消息）。
     ⇒ 现在用**内容指纹**（行数 + 逐行 hash）的公共前缀派生。

     ★ 判据在 **core**（`derive_content_status`），不在前端：前端要自己算就得把
     **每个副本**的前 512 行逐行 hash 拿过来（真机 25,487 个副本 ≈ 200 MB JSON），
     再在 JS 里逐对比较 ⇒ 会话页永远转圈。本断言同时守这条边界：
     ① core 有判据 + 四态；② 判据体内**不许**出现 updatedAt；③ 前端**不许**自己再派生。

     ★ **反向断言**：`updatedAt` 只能表达「谁最近被改过」，**不能**表达「谁落后 N 条」
     （§1.2「时间新 ≠ 内容新」）⇒ 派生函数里**不许**出现 `updated_at`。 */
  const core = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/session.rs"), "utf8"),
  );
  for (const f of [
    "derive_content_status",
    "fingerprint_of_text",
    "common_prefix_len",
    "line_count",
    "prefix_hashes",
  ]) {
    if (!core.includes(f)) throw new Error(`session.rs 缺 ${f} —— 内容指纹 / 组状态未实现`);
  }
  for (const s of ["latest", "behind", "diverge", "unknown"]) {
    if (!core.includes(`STATUS_${s.toUpperCase()}: &str = "${s}"`)) {
      throw new Error(`derive_content_status 缺状态 ${s}`);
    }
  }
  // ★ 反向：判据函数体内**不许**出现 updated_at（用时间冒充内容 = 撒谎）。
  const body = core.split("fn derive_content_status")[1]?.split("\nfn ")[0] ?? "";
  if (body === "") {
    throw new Error("找不到 derive_content_status 函数体（结构已变，请更新本断言）");
  }
  if (/updated_at|updatedAt/.test(body)) {
    throw new Error("derive_content_status 里出现 updatedAt —— 用时间冒充内容（§1.2 红线）");
  }
  // ★ 前端**不许**自己重算（重算 = 逐行 hash 又得过 IPC，正是本次修掉的东西）。
  const rep = stripComments(
    fs.readFileSync(path.join(ROOT, "src/lib/session-replicas.ts"), "utf8"),
  );
  if (/deriveGroupStatus/.test(rep)) {
    throw new Error("前端又自己派生组状态了 —— 判据属于 core（derive_content_status）");
  }
  if (!/group\.content/.test(rep)) {
    throw new Error("前端没有消费后端 content —— 组状态会全落 unknown");
  }
  return "core 内容指纹 + 四态 + 判据体无 updatedAt + 前端只消费结论";
});

check("★ P8 面板：筛选按需出现 + 失效回落 + 徽章只读已加载 store", () => {
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/SessionsPage.tsx"), "utf8"),
  );
  if (!/availableStatuses\.length >= 2/.test(page)) {
    throw new Error("状态筛选没做「至少 2 个才渲染」—— 单选项的切换器是假控件（§3.15.1）");
  }
  if (!/statusCounts\.has/.test(page)) {
    throw new Error("筛选态失效没回落 —— 会留下「空屏 + 假筛选」");
  }
  const app = stripComments(fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8"));
  if (
    !/pendingSyncCount/.test(app) ||
    !/data-badge=\{item\.dest === "sessions" \? "pending-sync"/.test(app)
  ) {
    throw new Error("侧栏没有待同步徽章（behind + diverge）");
  }
  // ★ 徽章**只读已加载的 store** —— `App` 里不许主动加载（否则每次开应用都读一遍 jsonl）。
  if (/useSessionsStore\.getState\(\)\.load/.test(app)) {
    throw new Error("App 里主动加载了会话数据 —— 徽章应只读已加载的 store");
  }
  return "按需筛选 + 失效回落 + 徽章只读";
});

check("★ P8 同步出口：只在「真有可同步内容」时出现（behind + 有增量）", () => {
  const rep = readCode("src/lib/session-replicas.ts");
  const body = rep.split("export function buildSyncItems")[1]?.split("\nexport ")[0] ?? "";
  if (body === "") throw new Error("找不到 buildSyncItems 函数体（结构已变，请更新本断言）");
  // ① 非 behind 一律不出项：`latest` 无事可做；`diverge` **必须先人工决定保留哪一份**
  //    （D20 后置，硬同步会丢内容）；`unknown` 缺指纹 ⇒ 不臆造。
  if (!/!== "behind"/.test(body)) {
    throw new Error("buildSyncItems 没有排除非 behind 状态 —— 分歧/未知组不该给同步出口");
  }
  // ② 没有增量时不出项（否则是「按了没反应」的假控件，§3.4 规则 1）。
  if (!/maxDelta <= 0/.test(body)) throw new Error("buildSyncItems 缺「无增量不出项」护栏");
  // ③ 视图侧：无项就**不渲染**按钮。
  const view = readCode("src/components/sessions/session-replica-view.tsx");
  if (!/items\.length === 0/.test(view)) {
    throw new Error("副本视图没有「无项不渲染按钮」的护栏");
  }
  // ④ 页头批量按钮同样按需出现（不摆假控件）。
  const page = readCode("src/pages/SessionsPage.tsx");
  if (!/allSyncable\.length > 0/.test(page)) throw new Error("页头批量按钮没有按需出现");
  return "behind + 有增量才出项 · 视图与页头按需渲染";
});

check("★ P8 同步是加法：core 只 append、绝不截断；分歧 fail-closed 不落字节", () => {
  const src = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/session.rs"),
    "utf8",
  );
  const body = rustFnBody(src, "append_delta_between_files");
  if (!body) throw new Error("找不到 append_delta_between_files（结构已变，请更新本断言）");
  // ① 只能以 append 方式打开目标 —— 截断重写会**删掉目标已有内容**（用户对「同步」最大的恐惧）。
  if (!/\.append\(true\)/.test(body)) throw new Error("没有以 append(true) 打开目标");
  for (const bad of [".write(true)", "truncate(true)", "set_len("]) {
    if (body.includes(bad)) throw new Error(`出现 ${bad} —— 违反「同步是加法」（§3.15.4）`);
  }
  // ② 分歧判据必须在，且 fail-closed（返回 Err ⇒ 一个字节都不写）。
  if (!/common < shorter/.test(body)) {
    throw new Error("缺「公共前缀 < 较短一方」的分歧判据（写成「< 目标行数」会把「目标更新」误判成分歧）");
  }
  if (!/return Err\(/.test(body)) throw new Error("分歧没有 fail-closed（没有返回 Err）");
  // ③ 逻辑行切分必须在 —— 否则「目标末行缺换行」会被误判成分歧。
  if (!/fn logical_lines/.test(src)) {
    throw new Error("缺 logical_lines（末行换行差异会被误判成分歧）");
  }
  // ④ id 归一化：副本每行都带各自的 sessionId，不归一化 ⇒ 永远分歧。
  if (!/SESSION_ID_PLACEHOLDER/.test(src)) {
    throw new Error("缺 SESSION_ID_PLACEHOLDER —— 指纹/行比较会把「各自的 sessionId」算进去");
  }
  return "append(true) + 分歧 fail-closed + 逻辑行切分 + id 归一化";
});

check("★ P8 同步批量：一条失败不中止整批 + 四处接线齐备", () => {
  const src = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/session.rs"),
    "utf8",
  );
  const body = rustFnBody(src, "sync_session_replicas_for");
  if (!body) throw new Error("找不到 sync_session_replicas_for（结构已变，请更新本断言）");
  // ① 失败分支必须**逐条收集**，不得 `return` / `?` 把整批带走（部分失败是 §3.6 硬要求）。
  if (!/Err\(error\) => results\.push/.test(body)) {
    throw new Error("失败分支没有逐条收集 —— 一条失败会带走整批");
  }
  if (!/ok_count \+= 1/.test(body)) throw new Error("没有成功计数");
  if (!/failCount/.test(body)) throw new Error("返回里没有 failCount（汇总条要用）");
  // ② 四处接线：ROUTES + call() / 命令本体 / generate_handler! / server 路由 + 处理器。
  const api = readCode("src/lib/api.ts");
  if (!/sync_session_replicas: \{ method: "POST"/.test(api)) throw new Error("api.ts 缺 ROUTES 条目");
  if (!/call\("sync_session_replicas"/.test(api)) throw new Error("api.ts 缺 call() 出口");
  const commands = readCode("src-tauri/src/commands.rs");
  if (!/pub async fn sync_session_replicas/.test(commands)) {
    throw new Error("commands.rs 缺命令本体");
  }
  const lib = readCode("src-tauri/src/lib.rs");
  if (!/commands::sync_session_replicas/.test(lib)) {
    throw new Error("lib.rs 的 generate_handler! 未登记（构建全绿但没人调用）");
  }
  const server = readCode("crates/buddy-switch-server/src/api.rs");
  if (!/\.route\("\/api\/sessions\/sync"/.test(server)) throw new Error("server 缺 .route");
  if (!/async fn api_session_sync/.test(server)) throw new Error("server 缺处理器");
  return "逐条收集（部分失败）+ 四处接线齐备";
});

check("★ P8 分歧处理：覆盖前先备份（fail-closed）+ 四处接线 + 只在 diverge 给出口", () => {
  const src = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/session.rs"),
    "utf8",
  );
  const body = rustFnBody(src, "unify_replica_between_files");
  if (!body) throw new Error("找不到 unify_replica_between_files（结构已变，请更新本断言）");
  // ① 这是**覆盖**，不是加法 —— 出现 append 就是把两种语义混了（分歧永远合不到一起）。
  if (/\.append\(true\)/.test(body)) {
    throw new Error("unify 里出现 append(true) —— 覆盖式统一被写成了追加（分歧无法合并）");
  }
  // ② 先备份、再覆盖：copy 必须早于 write，否则覆盖后无从恢复。
  const copyAt = body.indexOf("std::fs::copy");
  const writeAt = body.indexOf("std::fs::write(dst");
  if (copyAt < 0) throw new Error("缺备份（std::fs::copy）");
  if (writeAt < 0) throw new Error("缺覆盖写入（std::fs::write(dst)）");
  if (copyAt > writeAt) throw new Error("备份晚于覆盖 —— 顺序反了（覆盖后无从恢复）");
  // ③ 备份字节数要**复核**（只信 copy 的返回值不够），且失败即 fail-closed、一个字节都不覆盖。
  if (!/backed_up != original/.test(body)) {
    throw new Error("缺备份字节数复核（只信 std::fs::copy 的返回值不够）");
  }
  if (!/return Err\(/.test(body)) throw new Error("备份失败没有 fail-closed（没有 return Err）");
  // ④ 本来就一致 ⇒ 不写盘、不备份（no-op，`backup: None` 是这条分支的专属标记）。
  if (!/backup: None/.test(body)) throw new Error("缺「一致即 no-op」分支（backup: None）");
  // ⑤ 批量：一条失败不中止整批（§3.6）。
  const batch = rustFnBody(src, "unify_session_replicas_for");
  if (!batch) throw new Error("找不到 unify_session_replicas_for（结构已变，请更新本断言）");
  if (!/Err\(error\) => results\.push/.test(batch)) {
    throw new Error("失败分支没有逐条收集 —— 一条失败会带走整批");
  }
  if (!/ok_count \+= 1/.test(batch)) throw new Error("没有成功计数");
  if (!/failCount/.test(batch)) throw new Error("返回里没有 failCount（汇总条要用）");
  // ⑥ 四处接线：ROUTES + call() / 命令本体 / generate_handler! / server 路由 + 处理器。
  const api = readCode("src/lib/api.ts");
  if (!/unify_session_replicas: \{ method: "POST"/.test(api)) throw new Error("api.ts 缺 ROUTES 条目");
  if (!/call\("unify_session_replicas"/.test(api)) throw new Error("api.ts 缺 call() 出口");
  const commands = readCode("src-tauri/src/commands.rs");
  if (!/pub async fn unify_session_replicas/.test(commands)) {
    throw new Error("commands.rs 缺命令本体");
  }
  const lib = readCode("src-tauri/src/lib.rs");
  if (!/commands::unify_session_replicas/.test(lib)) {
    throw new Error("lib.rs 的 generate_handler! 未登记（构建全绿但没人调用）");
  }
  const server = readCode("crates/buddy-switch-server/src/api.rs");
  if (!/\.route\("\/api\/sessions\/unify"/.test(server)) throw new Error("server 缺 .route");
  if (!/async fn api_session_unify/.test(server)) throw new Error("server 缺处理器");
  // ⑦ 前端：只在 diverge 给出口、缺指纹不臆造数字、对话框已挂到页面。
  const replicas = readCode("src/lib/session-replicas.ts");
  if (!/group\.status !== "diverge"/.test(replicas)) {
    throw new Error("buildUnifyItems 没挡非 diverge —— 会让 latest/behind 也走覆盖");
  }
  if (!/delta === undefined \? null : delta/.test(replicas)) {
    throw new Error("lostLinesFor 缺「未知即 null」—— 会臆造一个编出来的丢失条数");
  }
  const view = readCode("src/components/sessions/session-replica-view.tsx");
  if (!/group\.status === "diverge"/.test(view)) {
    throw new Error("副本视图没有「只在分歧组给出口」的门控");
  }
  const page = readCode("src/pages/SessionsPage.tsx");
  if (!/UnifyReplicasDialog/.test(page)) throw new Error("SessionsPage 没挂载分歧处理对话框");
  if (!/buildUnifyItems/.test(page)) throw new Error("SessionsPage 没把「保留哪一份」换算成覆盖任务");
  return "覆盖前先备份（fail-closed）+ 部分失败 + 四处接线 + 只在 diverge 给出口";
});

check("★ 限流 S6：6020/6022 只标注保守词「排队中」，不臆造精确文案（R7）", () => {
  /* PRD R7 红线：`6020` / `6022` 的**精确语义文案待拍板前不得臆造**。
     取证（`.trellis/tasks/09-22-client-rate-limit-visibility/prd.md`「★ 6020/6022 取证」）表明
     客户端排队块文案是 `capacityQueueBlock.*`，但精确键仍在**上游**，映射**属推断**。
     ⇒ 本断言锁：① 卡片确实识别这两个码；② 只渲染保守词；③ **不许**把上游键
     或推断出的精确文案硬编码进展示层（那等于把「推断」当「事实」）。 */
  const card = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/client-limits-card.tsx"), "utf8"),
  );
  if (!/QUEUE_CODES\s*=\s*new Set\(\[6020,\s*6022\]\)/.test(card)) {
    throw new Error("限流卡片没有把 6020 / 6022 认作排队类 —— S6 展示未落地");
  }
  if (!/wbAccounts\.limits\.queued/.test(card)) {
    throw new Error("限流卡片没有渲染「排队中」标注");
  }
  // ★ 反向：展示层**不许**出现上游键 / 推断文案（把推断当事实 = 违反 R7）。
  for (const bad of ["capacityQueueBlock", "queue.waiting.title", "queue.userLimit.title"]) {
    if (card.includes(bad)) {
      throw new Error(`限流卡片出现 ${bad} —— 把「推断」的精确文案硬编码进展示层（违反 R7）`);
    }
  }
  // 双语键必须在（en 缺键会回落中文，但两套都要有才算登记）。
  for (const file of ["src/locales/domains/wbAccounts.zh.ts", "src/locales/domains/wbAccounts.en.ts"]) {
    const loc = fs.readFileSync(path.join(ROOT, file), "utf8");
    if (!loc.includes("wbAccounts.limits.queued")) throw new Error(`${file} 缺 wbAccounts.limits.queued`);
  }
  return "识别 6020/6022 + 保守词 + 展示层无臆造文案";
});

// ---------------------------------------------------------------------------
// 2026-10-10 对齐轮（ALIGN-2026-10-10 §4Q1–Q7 · 决策 D35–D41）
// ---------------------------------------------------------------------------

check("★ C1「重试失败项」是真重试，不是「重新选文件」（安全相关）", () => {
  /* 本批最典型的**假控件**：文案承诺「只重试失败项（增量）」，行为却是打开文件选择器
     整份重导（覆盖语义）—— 文案与行为相反。改法是复用同一份 `fileTextRef` 文本 +
     只把 `retryableScopes` 回传给后端。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/settings/backup-section.tsx"), "utf8"),
  );
  // ① 真重试函数必须存在，且**必须**带 scope 名单参数。
  if (!/async function retryFailed\(/.test(src)) {
    throw new Error("备份导入没有 `retryFailed` —— 「重试失败项」仍是假控件");
  }
  if (!/importBackup\(text, conflict, buildSelectionState\(\), scopes\)/.test(src)) {
    throw new Error("`retryFailed` 没有把 `retryableScopes` 传给 `importBackup` —— 仍是整份重来");
  }
  // ② 重试按钮的 onClick 必须是 `retryFailed`，**不得**是 `chooseFile`。
  const button = /retryFailed[\s\S]{0,400}?<\/Button>/.exec(src);
  if (!button) throw new Error("找不到「重试失败项」按钮");
  if (/onClick=\{chooseFile\}/.test(button[0])) {
    throw new Error("「重试失败项」又接回了 `chooseFile`（打开文件选择器、整份重导）");
  }
  // ③ 前端只读 `retryableScopes` 决定是否显示（`retryable` 计数不足以定位到分区）。
  if (!/retryableScopes\?\.length/.test(src)) {
    throw new Error("重试按钮未按 `retryableScopes` 判定 —— 未知 scope 会被塞进重试名单");
  }
  return "retryFailed + scope 名单回传 + 不接 chooseFile";
});

check("★ C1 后端：重试只处理名单内 scope，且未知 scope 不可重试", () => {
  const code = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/backup.rs"),
    "utf8",
  );
  const fn = /pub fn import_backup\([\s\S]*?\n\}/.exec(code);
  if (!fn) throw new Error("找不到 `import_backup`");
  // ① 必须在**进循环前**按名单过滤（连读都不读）。
  if (!/only_scopes\.is_empty\(\) && !only_scopes\.iter\(\)\.any/.test(fn[0])) {
    throw new Error("`import_backup` 没有按 `only_scopes` 过滤 —— 重试会整份重来");
  }
  // ② 必须回传可重试名单，否则前端无从「只重试失败项」。
  if (!/"retryableScopes":\s*retryable_scopes/.test(fn[0])) {
    throw new Error("导入结果未回传 `retryableScopes` —— 前端无法只重试失败分区");
  }
  // ③ 未知 scope 必须 `retryable: false`（键名非法是结构性缺陷，重试必然再失败）。
  //    匹配整个 `import_backup` 函数体（而非只到函数首行）—— 分支里带注释，正则要够宽。
  const unknown = /Err\(error\) => \{[\s\S]{0,600}?"retryable":\s*false/.test(fn[0]);
  if (!unknown) {
    throw new Error("未知 scope 被标成可重试 ⇒ 前端会重试一个必然失败的分区（假出口）");
  }
  return "only_scopes 过滤 + retryableScopes 回传 + 未知 scope 不可重试";
});

check("★ C1 双通道形状一致：`import_backup` 的 only_scopes 两侧都接", () => {
  /* MEMORY §2 铁律：改一端必须核对另一端。少一端 ⇒ 一侧透传、另一端丢参数，
     而返回形状不变、编译器与单测都不报（曾致 `{copied:{copied:[…]}}`）。 */
  const tauri = fs.readFileSync(path.join(ROOT, "src-tauri/src/commands.rs"), "utf8");
  const http = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-server/src/api.rs"),
    "utf8",
  );
  for (const [label, src] of [
    ["commands.rs", tauri],
    ["api.rs", http],
  ]) {
    if (!/only_scopes: Option<Vec<String>>/.test(src) && !/onlyScopes/.test(src)) {
      throw new Error(`${label} 的 import_backup 没有接 only_scopes —— 双通道形状漂移`);
    }
  }
  const api = fs.readFileSync(path.join(ROOT, "src/lib/api.ts"), "utf8");
  if (!/onlyScopes:\s*\[\.\.\.onlyScopes\]/.test(api)) {
    throw new Error("`api.ts::importBackup` 没有把 onlyScopes 放进载荷");
  }
  return "commands.rs / api.rs / api.ts 三处一致";
});

check("★ Q1 WB 侧危险操作区：导出诊断包 + 清空日志，且**没有**「重置设备标识」", () => {
  const page = fs.readFileSync(path.join(ROOT, "src/pages/SettingsPage.tsx"), "utf8");
  // ① 两个真实动作。
  if (!/function DangerZoneCard\(/.test(page)) {
    throw new Error("WB 设置页没有危险操作分区（Q1 要求的 WB 侧出口缺失）");
  }
  if (!/exportDiagnostic\(\)/.test(page)) {
    throw new Error("危险操作区没有调用 `exportDiagnostic` —— 「导出诊断包」是死按钮");
  }
  if (!/clearGatewayLogs\(\)/.test(page)) {
    throw new Error("危险操作区没有调用 `clearGatewayLogs`");
  }
  // ② 不可逆动作必须有二次确认（清空日志不可撤销）。
  if (!/logsConfirmDesc/.test(page)) {
    throw new Error("「清空网关请求日志」没有二次确认 —— 不可逆操作不得一步到位");
  }
  // ★ 反向：WB 侧**不得**出现「重置设备标识」。它在 WorkBuddy 上没有对应实体
  //   （Trae 的 6 层文件 WB 根本没有；本工具身份是 sha256 纯派生、不落盘）
  //   ⇒ 加了就是只能「重置空气」的假控件。
  //   ⚠️ 必须先剥注释：解释「为什么不做」的注释里**会**出现这个词（那正是要保留的证据）。
  const pageCode = stripComments(page);
  for (const bad of ["重置设备标识", "resetDeviceIdentity", "wb_reset_device"]) {
    if (pageCode.includes(bad)) {
      throw new Error(`WB 设置页出现「${bad}」—— WorkBuddy 没有可重置的设备标识（假控件）`);
    }
  }
  return "DangerZoneCard + 双动作 + 二次确认 + 无重置设备标识";
});

check("★ Q1 诊断包必须过凭据闸门（与备份共用同一判据）", () => {
  const core = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/diagnostics.rs"),
    "utf8",
  );
  // ① 必须复用备份的凭据键集与**同一个**搜索函数（判据不得另立一套 ⇒ 防漂移）。
  if (!/backup::\{contains_credential_key, CREDENTIAL_KEYS\}/.test(core)) {
    throw new Error("诊断包没有复用 backup 的 CREDENTIAL_KEYS / contains_credential_key");
  }
  // ② 产出后必须做纯文本闸门，命中即拒绝（fail-closed）。
  if (!/contains_credential_key\(&text\)/.test(core)) {
    throw new Error("诊断包产出后没有做纯文本凭据搜索 —— 可能把凭据发给别人");
  }
  // ③ 不得打包会话正文（`MEMORY-10` 红线：含用户真实项目路径）。
  if (/projects.*\.jsonl/.test(core.replace(/\/\/.*/g, ""))) {
    throw new Error("诊断包疑似读取会话正文 —— 含用户真实项目路径，不得外发");
  }
  return "复用 CREDENTIAL_KEYS + 纯文本闸门 + 不含会话正文";
});

check("★ Q3 概览与账号卡的 Token 口径必须显式区分（不得同名）", () => {
  /* 两个来源**互斥**（MEMORY-10：客户端直连上游，不流经网关）⇒ 两个数字各自都对，
     但同名会让人默认它们该相等。故概览改名 + tooltip 说明口径。 */
  const ovZh = fs.readFileSync(
    path.join(ROOT, "src/locales/domains/wbOverview.zh.ts"),
    "utf8",
  );
  if (!/"wbOverview\.stat\.tokens":\s*"客户端 Token（今日）"/.test(ovZh)) {
    throw new Error("概览 Token 未改名为「客户端 Token（今日）」—— 与账号卡同名会被当成同一个数");
  }
  if (!/tokensHintClientOnly/.test(ovZh)) {
    throw new Error("概览 Token 缺少口径说明词条 —— 用户仍会以为它含网关用量");
  }
  // tooltip 必须真的挂到 tile 上（词条存在 ≠ 渲染出来）。
  const stats = fs.readFileSync(
    path.join(ROOT, "src/components/overview/overview-stats.tsx"),
    "utf8",
  );
  if (!/title:\s*t\("wbOverview\.stat\.tokensHintClientOnly"\)/.test(stats)) {
    throw new Error("口径说明没有挂到概览 Token tile 的 title 上");
  }
  return "概览改名「客户端 Token」+ tooltip 说明口径";
});

check("★ Q5 全模型禁用时计数行仍须渲染（served=0 不是「无数据」）", () => {
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/model-list.tsx"), "utf8"),
  );
  if (/\{served > 0 && \(/.test(src)) {
    throw new Error("计数行又退回 `served > 0` 守卫 —— 全部禁用时整条信息消失");
  }
  if (!/\{total > 0 && \(/.test(src)) {
    throw new Error("计数行守卫应为 `total > 0`（有清单就显示，含 served=0）");
  }
  // 反向：不得新增「N 个模型可用」措辞（HTTP 200 ≠ 可用，D24）。
  if (/个模型可用/.test(src)) throw new Error("出现「N 个模型可用」措辞（违反 D24）");
  return "守卫为 total > 0 + 无「可用」措辞";
});

check("★ Q4 头像底色须「深底 + 白字」且自含（原型 avatarTone，AA ≥ 4.5:1）", () => {
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8"),
  );
  /* 断言的是**意图**而非某套具体色值：
     ① 头像是 12.5px/640 的小字 ⇒ 必须与底色有足够对比度，且**不能依赖主题**
        （`--success`/`--warning` 的 `/15` 底实测仅 3.11:1 / 3.17:1，低于 AA 4.5:1）；
     ② 形态对齐原型 `avatarTone()`（`buddy-switch-prototype.html:1921-1928`）：
        6 个 `oklch(L ≤ 0.5)` **深底** + `oklch(0.99 0 0)` 近白字。
        ⚠️ 这六条压白字实测 5.46~7.00:1 **全部过 AA** —— 旧断言担心的
        「照原型会读不清」不成立，故取值改回原型。 */
  for (const bad of ["bg-success/15", "bg-warning/15"]) {
    if (src.includes(bad)) {
      throw new Error(`头像色板仍用 \`${bad}\`（实测 3.1:1，低于 AA 4.5:1）`);
    }
  }
  const tones = /const AVATAR_TONES = \[([\s\S]*?)\] as const;/.exec(src);
  if (!tones) throw new Error("找不到 AVATAR_TONES");
  const entries = tones[1]
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith('"'));
  if (entries.length !== 6) throw new Error(`头像色板应有 6 条，实际 ${entries.length}`);
  /* 六条同构：写死的深色 oklch（L ≤ 0.5），不引用任何 CSS 变量 / 语义色。 */
  for (const entry of entries) {
    const tone = entry.replace(/,$/, "");
    const m = /^"\[background:oklch\(0\.(\d+)_[\d.]+_\d+\)\]"$/.exec(tone);
    if (!m) {
      throw new Error(`头像色板条目须为写死的深色 oklch（不依赖主题）：${tone}`);
    }
    /* ⚠️ 这里比较的是 `0.48` 的**小数部分** `48`（百分制），不是 `0.48`——
       直接比 `0.48 > 0.5` 会误判（写成 `48 > 5` 就把 0.48 判成超标）。
       故用 `Number("0." + m[1])`还原后再比。 */
    const L = Number(`0.${m[1]}`);
    if (L > 0.5) throw new Error(`头像底色亮度须 ≤ 0.5（白字才够对比度）：${tone} = L ${L}`);
  }
  /* 字色须为近白，且与色板同源定义（否则改色板会漏改字色）。 */
  if (!/const AVATAR_TEXT_CLASS = "text-\[oklch\(0\.99_0_0\)\]"/.test(src)) {
    throw new Error("缺少 AVATAR_TEXT_CLASS（近白字色，须与色板同源定义）");
  }
  if (!/AVATAR_TEXT_CLASS, avatarClass/.test(src)) {
    throw new Error("头像未同时应用字色与底色（AVATAR_TEXT_CLASS + avatarClass）");
  }
  return "6 条深底（oklch L≤0.5）+ 近白字，实测 5.46~7.00:1 全部过 AA";
});

check("★ Q2「指定」不得按账号状态禁用（偏好而非约束，D36）", () => {
  /* 后端 `pick_with_preference` 在指定账号不可用时**回落**自动择优、绝不拒绝服务
     ⇒ 前端若把它禁掉，用户就无法预设一个「正在冷却、等下恢复」的账号，
     而且与后端行为自相矛盾（按钮灰了但策略仍会生效）。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/account-pool-card.tsx"), "utf8"),
  );
  // 从 `data-act="pool-pin"` 起取该 `<Button …>` 开始标签的其余部分。
  // ⚠️ 不用 `/>` 收尾：属性里含 `onClick={() => …}`，标签是以 `}` 换行 `>` 结束的。
  const pin = /data-act="pool-pin"[\s\S]{0,400}?>\s*\n\s*\{/.exec(src)
    || /data-act="pool-pin"[\s\S]{0,400}?>/.exec(src);
  if (!pin) throw new Error("找不到「指定」按钮");
  const disabled = /disabled=\{([^}]*)\}/.exec(pin[0]);
  if (!disabled) throw new Error("「指定」按钮没有 disabled 绑定");
  for (const forbidden of ["cooling", "disabled", "no_credits", "expired", "credits"]) {
    if (disabled[1].includes(forbidden)) {
      throw new Error(
        `「指定」按钮的 disabled 依赖 \`${forbidden}\` —— 指定是偏好而非约束，不该按状态禁用（D36）`,
      );
    }
  }
  /* 反向（2026-10-17 更新）：`excluded` 已**成为真实数据模型**（账号池排除功能），
     因此这里改为钉住「它是唯一允许禁用指定按钮的状态」：
     排除是硬约束（后端选号 / 兜底 / 粘性 / 指定全跳过），放行 = 假按钮；
     而冷却 / 禁用 / 零积分是**临时状态**，用户有权预先指定（D36）。 */
  const types = fs.readFileSync(path.join(ROOT, "src/lib/types.ts"), "utf8");
  const poolBlock = /export interface GatewayPoolAccount \{[\s\S]*?\n\}/.exec(types);
  if (!poolBlock || !/excluded\?:/.test(poolBlock[0])) {
    throw new Error("GatewayPoolAccount 缺 `excluded` —— 排除功能已落地，字段必须存在");
  }
  if (!disabled[1].includes("excluded")) {
    throw new Error("「指定」按钮必须在 `account.excluded` 时禁用（排除是硬约束，放行即假按钮）");
  }
  return "指定按钮只受 saving / excluded 控制（冷却等临时状态不禁用）";
});

check("★★ 三条产品线的账号池归一化都必须保留 `excluded`（否则排除后再也取消不掉）", () => {
  /* 症状（2026-10-22 用户报障）：账号池里点了「排除」，按钮就再也变不回来 ——
     文案恒为「排除」、点下去恒传 `excluded=true`（幂等）⇒ 账号被永久挡在选号外。
     根因**不在按钮逻辑**，而在「字段看不见」：三条产品线各有各的归一化函数，
     任何一处用白名单重建时漏掉 `excluded`，前端就永远看不到它。
     —— PR #77 只补了 WorkBuddy 一侧（它当时以为「三平台共用同一个归一化」），
     Trae / Qoder 侧照旧坏着 ⇒ 这条断言存在就是为了不让下一次再漏。 */
  const wb = loadTs(path.join(ROOT, "src/lib/gateway.ts"));
  const trae = loadTs(path.join(ROOT, "src/lib/trae-gateway.ts"));
  const qoder = loadTs(path.join(ROOT, "src/lib/qoder-gateway.ts"));

  /* 归一化后的账号：`excluded` 必须是**布尔真值**（漏字段 ⇒ undefined ⇒ 恒假）。 */
  const row = { uid: "u1", name: "n1", status: "excluded", excluded: true, excludedNote: "手动" };
  const wbOut = wb.normalizeGatewayPoolAccounts([row])[0];
  const traeOut = trae.normalizeTraeGatewayStatus({ accounts: [row] }).accounts[0];
  const qoderOut = qoder.normalizeQoderGatewayStatus({ accounts: [row] }).accounts[0];
  for (const [name, account] of [
    ["WorkBuddy", wbOut],
    ["Trae", traeOut],
    ["Qoder", qoderOut],
  ]) {
    if (!account) throw new Error(`${name} 侧归一化丢了整行账号`);
    if (account.excluded !== true) {
      throw new Error(
        `${name} 侧归一化丢了 \`excluded\`（得到 ${JSON.stringify(account.excluded)}）`
          + " —— 排除按钮会恒显示「排除」、恒传 true ⇒ 排除后再也取消不掉",
      );
    }
    if (account.excludedNote !== "手动") {
      throw new Error(`${name} 侧归一化丢了 \`excludedNote\`（得到 ${JSON.stringify(account.excludedNote)}）`);
    }
    /* ★ `status` 也必须保住 `"excluded"` —— 归一化若把它落回 `"available"`，
       界面会显示「可用」而实际选号永远不选它（另一种形式的数字与事实不符）。
       这一条是独立于 `excluded` 字段的：**字段在、状态被吞** 同样会误导人。 */
    if (account.status !== "excluded") {
      throw new Error(
        `${name} 侧归一化把 status "excluded" 落成了 ${JSON.stringify(account.status)}`
          + " —— 界面会显示「可用」，而实际选号永远不选它",
      );
    }
  }

  /* 「已排除」是**互斥第六态**：后端已把它从 `available` 里扣掉并单独计数，
     卡片必须有这一格，否则「3 可用」而实际只有 2 个能被选中（数字与事实不符）。 */
  const pool = { total: 3, available: 2, cooling: 0, disabled: 0, expired: 0, zeroCredits: 0, excluded: 1 };
  if (wb.normalizeGatewayPoolSummary(pool).excluded !== 1) {
    throw new Error("WorkBuddy 侧池摘要归一化丢了 `excluded` 计数");
  }
  const traePool = trae.normalizeTraeGatewayStatus({ pool }).pool;
  if (traePool.excluded !== 1) {
    throw new Error("Trae 侧池摘要归一化丢了 `excluded` 计数");
  }
  const qoderPool = qoder.normalizeQoderGatewayStatus({ pool }).pool;
  if (qoderPool.excluded !== 1) {
    throw new Error("Qoder 侧池摘要归一化丢了 `excluded` 计数");
  }

  /* 卡片的三套计数格都必须含「已排除」（Trae 走默认六格、Qoder 走 QODER_POOL_TILES）。
     ⚠️ 这里用**文本扫描**而不是 `loadTs`：卡片是 `.tsx`，含 JSX ⇒ `loadTs` 的
     `transpileModule`（未开 jsx 选项）会抛 `Unexpected token '>'`。
     计数格是**字面量常量**，文本扫描足够硬 —— 真要防「有人把 excluded 从数组里删掉」，
     正则匹配到的那段数组文本里就必须有它。 */
  const cardSrc = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/account-pool-card.tsx"), "utf8"),
  );
  for (const [name, constName] of [
    ["Trae（默认格）", "DEFAULT_POOL_TILES"],
    ["Qoder", "QODER_POOL_TILES"],
    ["WorkBuddy", "WB_POOL_TILES"],
  ]) {
    const block = new RegExp(`export const ${constName}[^=]*=\\s*\\[([\\s\\S]*?)\\]`).exec(cardSrc);
    if (!block) throw new Error(`找不到 ${constName} 的计数格定义`);
    if (!/"excluded"/.test(block[1])) {
      throw new Error(`${name} 的计数格缺「已排除」—— 排除是互斥第六态，必须有自己的一格`);
    }
  }
  return "三平台归一化都带 excluded / excludedNote，摘要都有 excluded 计数，三套计数格都含「已排除」";
});

check("★ 策略哨兵值前后端逐字一致（否则下拉是假开关）", () => {
  /* 四个哨兵（`__auto__` / `__max_credits__` / `__random__` / `__sequential__`）在
     **前端常量**里定义一次、在**后端 `PREFERRED_*`** 里再定义一次。任何一处漂移的后果
     都不是报错，而是「用户选了『按积分择优』，后端把它当成一个不存在的 uid，静默回落加权」
     —— 假开关，且从界面上完全看不出来。故用契约检查把两份钉在一起。 */
  const card = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/account-pool-card.tsx"), "utf8"),
  );
  const pick = stripComments(
    fs.readFileSync(
      path.join(ROOT, "crates/buddy-switch-gateway/src/pool/pick.rs"),
      "utf8",
    ),
  );
  const pairs = [
    ["AUTO_OPTION", "PREFERRED_AUTO"],
    ["MAX_CREDITS_OPTION", "PREFERRED_MAX_CREDITS"],
    ["RANDOM_OPTION", "PREFERRED_RANDOM"],
    ["SEQUENTIAL_OPTION", "PREFERRED_SEQUENTIAL"],
  ];
  for (const [tsName, rsName] of pairs) {
    const ts = new RegExp(`export const ${tsName} = "([^"]+)"`).exec(card);
    const rs = new RegExp(`pub const ${rsName}: &str = "([^"]+)"`).exec(pick);
    if (!ts) throw new Error(`前端缺少常量 ${tsName}`);
    if (!rs) throw new Error(`后端缺少常量 ${rsName}`);
    if (ts[1] !== rs[1]) {
      throw new Error(
        `策略哨兵不一致：前端 ${tsName}="${ts[1]}"，后端 ${rsName}="${rs[1]}" —— 该选项会静默失效`,
      );
    }
  }
  return "4 个哨兵值前后端逐字一致";
});

check("★ 三个网关页的账号池策略档位必须一致（含「随机」「顺序」）", () => {
  /* 后端 `account_strategy::AccountStrategy` 是**三平台共用**的枚举（Current / Pinned /
     MaxCredits / Random / Sequential），但「策略选项」是每页各自传给池卡的 `autoOptions`。
     任一页少给一档，
     症状是**该平台静默失去那个策略**：界面上没有这一项，用户根本无从知道后端支持它，
     而合并另一页时又会被当成「本来就没有」—— 排查成本极高（2026-10-17 用户报「随机模式没有了」，
     真因就是 WorkBuddy 页只传了两档）。
     ⚠️ 这里刻意断言**页面文件**而不是池卡默认值：默认值只覆盖「没传」的情况，
     三页都传了自己的一份，改默认值救不了漏传。 */
  const pages = [
    ["src/pages/ApiServicePage.tsx", "strategy"],
    ["src/pages/TraeApiServicePage.tsx", "preferredUid"],
    ["src/pages/QoderApiServicePage.tsx", "preferredUid"],
  ];
  for (const [page, writeBack] of pages) {
    const source = stripComments(fs.readFileSync(path.join(ROOT, page), "utf8"));
    for (const option of [
      "AUTO_OPTION",
      "MAX_CREDITS_OPTION",
      "RANDOM_OPTION",
      "SEQUENTIAL_OPTION",
    ]) {
      if (!source.includes(option)) {
        throw new Error(`${page} 的账号池 autoOptions 缺 ${option} —— 三平台策略档位必须一致`);
      }
    }
    /* 选项有 ⇒ 写回路径也得有：`autoOptions` 里出现了哨兵却没落到持久化上，
       选了会被当成一个不存在的 uid 静默回落自动择优。
       ⚠️ 两平台写回形态不同：Trae / Qoder 直接存哨兵串（`preferredUid`，由后端
       `pool::parse_preferred` 解释），WorkBuddy 存 `AccountStrategy`，故后者的
       `random` / `sequential` 哨兵必须**显式**映射成 `persist({ kind: … })`。 */
    if (writeBack === "strategy") {
      if (!/persist\(\{\s*kind:\s*"random"\s*\}\)/.test(source)) {
        throw new Error(`${page} 没有把 RANDOM_OPTION 映射到 persist({ kind: "random" })`);
      }
      if (!/strategy\.kind === "random"/.test(source)) {
        throw new Error(`${page} 没有把 kind: "random" 读回成下拉值（保存后选项会弹回「不指定」）`);
      }
      if (!/persist\(\{\s*kind:\s*"sequential"\s*\}\)/.test(source)) {
        throw new Error(
          `${page} 没有把 SEQUENTIAL_OPTION 映射到 persist({ kind: "sequential" })`,
        );
      }
      if (!/strategy\.kind === "sequential"/.test(source)) {
        throw new Error(
          `${page} 没有把 kind: "sequential" 读回成下拉值（保存后选项会弹回「不指定」）`,
        );
      }
    } else if (!/persist\(\{\s*preferredUid/.test(source)) {
      throw new Error(`${page} 没有把哨兵值写回 preferredUid`);
    }
  }
  const types = stripComments(fs.readFileSync(path.join(ROOT, "src/lib/types.ts"), "utf8"));
  /* 终止符取**行尾的分号**：union 成员里自带分号（`{ kind: "pinned"; account_id: string }`），
     用 `([\s\S]*?);` 会在第一个成员内部就截断，后面几个变体根本进不了检查范围。 */
  const union = /export type AccountStrategy =([\s\S]*?);\s*(?:\n|$)/.exec(types);
  if (!union) throw new Error("src/lib/types.ts 找不到 AccountStrategy union");
  if (!/"random"/.test(union[1])) {
    throw new Error("AccountStrategy union 缺 `{ kind: \"random\" }` —— 后端枚举已有 Random");
  }
  if (!/"sequential"/.test(union[1])) {
    throw new Error(
      "AccountStrategy union 缺 `{ kind: \"sequential\" }` —— 后端枚举已有 Sequential（issue #102）",
    );
  }
  return "3 个网关页 × 4 档策略 + union 一致";
});

check("★ Q6「登录态快照」不得升为 Capability（D40）", () => {
  /* Capability 语义是「跨目的地复用的功能门控」，而登录态快照只归 TraeSettingsPage
     一个分组所有。升格会牵动 union + 后端枚举 + 两个数组 + 跨语言护栏，代价与收益不成比例。 */
  const registry = fs.readFileSync(path.join(ROOT, "src/lib/platforms/registry.ts"), "utf8");
  const union = /export type Capability =([\s\S]*?);/.exec(registry);
  if (!union) throw new Error("找不到前端 Capability union");
  if (/"snapshot"|\bsnapshot\b/.test(union[1])) {
    throw new Error("Capability union 出现 `snapshot` —— 快照是页面内分组，不该升格（D40）");
  }
  const rust = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/capability.rs"),
    "utf8",
  );
  if (/Snapshot/.test(rust)) {
    throw new Error("后端 Capability 枚举出现 `Snapshot` —— 与前端 union 将失去逐字对齐");
  }
  return "union 与后端枚举均无 snapshot（刻意分歧，已固化）";
});

check("★ Q7 资源包不得按名称去重（F2 是原型反例）", () => {
  /* 真实语义：「同名但到期时间不同」是**两种合法资源包**，必须分别显示。
     照搬原型去重会**静默丢失一条到期信息**。 */
  const bad = [];
  for (const abs of listSourceFiles()) {
    const code = stripComments(fs.readFileSync(abs, "utf8"));
    // 按 name 字段去重资源包列表的典型形状：Map/Set 仅以 name 为键。
    for (const re of [
      /new Map\([^)]*\.map\(\w+ => \[\s*\w+\.name\s*,/,
      /new Set\([^)]*\.map\(\w+ => \w+\.name\)/,
    ]) {
      if (re.test(code)) {
        bad.push(`${path.relative(ROOT, abs)}（疑似按 name 去重）`);
      }
    }
  }
  if (bad.length) {
    throw new Error(`资源包疑似按名称去重（原型 F2 反例，会丢到期信息）：${bad.join(", ")}`);
  }
  return "无按 name 去重的资源包折叠";
});

check("★ A1 堆叠柱：列数 === 天数窗口，缺日补 0（不再切稀疏数组）", () => {
  /* 根因：后端 `daily: HashMap<String, Totals>` 只在**有记录的日子**落条目
     ⇒ `.slice(-range)` 切的是「有数据的天」而不是「最近 N 个日历日」。
     实测：5 天数据 + range=30 ⇒ 只出 5 列；range=7 时首列落在 59 天前。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const today = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const key = (offset) => {
    const d = new Date(today.getTime() - offset * 86400000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  // 故意只给 2 天数据，且**都在窗口内** —— 窗口仍必须是 30 列。
  const src = usageSource({
    daily: [{ key: key(0), total: 10, records: 1 }],
    dailyByModel: { "m-a": [{ key: key(0), total: 10, records: 1 }] },
  });
  /* 只跑**参数条上真实存在的**预设档 —— 90 天不是一档（它是自定义的封顶值）。 */
  for (const range of [7, 14, 30]) {
    const cols = uc.dailyModelSeries(src, presetWin(range));
    if (cols.length !== range) {
      throw new Error(`range=${range} 应得 ${range} 列，实得 ${cols.length}（仍在按稀疏数组切片？）`);
    }
    // 末列必须是今天（否则「近 N 天」标签与数据对不上）。
    if (cols[cols.length - 1].day !== key(0)) {
      throw new Error(`range=${range} 末列应是今天 ${key(0)}，实得 ${cols[cols.length - 1].day}`);
    }
  }
  // 窗口外的老数据**不得**被拽进来（这正是「range=7 却显示 59 天前数据」的成因）。
  const stale = usageSource({
    daily: [{ key: key(59), total: 999, records: 9 }],
    dailyByModel: { "m-a": [{ key: key(59), total: 999, records: 9 }] },
  });
  const cols7 = uc.dailyModelSeries(stale, presetWin(7));
  const sum7 = cols7.reduce((sum, c) => sum + c.total, 0);
  if (sum7 !== 0) throw new Error(`range=7 混入了窗口外的数据（和=${sum7}）—— 「近 7 天」图文不符`);
  return "7/30/90 窗口列数恒等于 range，窗口外数据不混入";
});

check("★ 窗口语义：今天 / 自定义**锚在日期上**（不是被天数拟合出来的）", () => {
  /* 加「自定义时间」后，窗口的真值是**一对日期键**，「近 N 天」只是生成它的一种方式。
     本用例钉住这条：自定义窗口的日期边界**逐字等于**用户选的那两天，
     且它 **不随「今天」漂移**（这是「用天数拟合」必然做不到的）。 */
  const w = loadTs(path.join(ROOT, "src/lib/usage-window.ts"));
  const pad = (n) => String(n).padStart(2, "0");
  const key = (offset) => {
    const d = new Date(Date.now() - offset * 86400000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };

  /* ① 今天 = 1 天，且就是今天。 */
  const todayKeys = w.usageWindowKeys({ kind: "today" });
  if (todayKeys.length !== 1 || todayKeys[0] !== key(0)) {
    throw new Error(`「今天」应是 1 个键且等于今天，实得 ${JSON.stringify(todayKeys)}`);
  }

  /* ② 自定义：锚在**过去**的一段。取 4 天前 → 1 天前（不含今天）。 */
  const custom = { kind: "custom", from: key(4), to: key(1) };
  const customKeys = w.usageWindowKeys(custom);
  if (customKeys.length !== 4) {
    throw new Error(`自定义 ${key(4)}–${key(1)} 应是 4 天，实得 ${customKeys.length}`);
  }
  if (customKeys[0] !== key(4) || customKeys[customKeys.length - 1] !== key(1)) {
    throw new Error(
      `自定义边界必须逐字等于所选（期望 ${key(4)}..${key(1)}，实得 ${customKeys[0]}..${customKeys[customKeys.length - 1]}）`,
    );
  }
  /* ★ 关键：末列 **不是今天**。若窗口退化成「最后 N 天」，末列必然落到今天。 */
  if (customKeys[customKeys.length - 1] === key(0)) {
    throw new Error("自定义窗口的末列落到了今天 —— 它已经退化成「最后 N 天」");
  }

  /* ③ 倒序输入被交换（不静默产出空窗口）。 */
  const swapped = w.usageWindowBounds({ kind: "custom", from: key(1), to: key(4) });
  if (swapped.from !== key(4) || swapped.to !== key(1)) {
    throw new Error(`倒序日期应被交换，实得 ${swapped.from}..${swapped.to}`);
  }

  /* ④ 越界被夹到边界（fail-closed，不猜）。最早可选 = 今天 - 89 天。 */
  const clamped = w.usageWindowBounds({ kind: "custom", from: "1999-01-01", to: "2999-01-01" });
  if (clamped.from !== w.usageWindowEarliestKey() || clamped.to !== key(0)) {
    throw new Error(`越界日期应夹到 [最早, 今天]，实得 ${clamped.from}..${clamped.to}`);
  }
  /* 非法日期键 ⇒ 退回今天（不产生 NaN 键）。 */
  const bad = w.usageWindowBounds({ kind: "custom", from: "不是日期", to: "" });
  if (bad.from !== key(0) || bad.to !== key(0)) {
    throw new Error(`非法日期应退回今天，实得 ${bad.from}..${bad.to}`);
  }

  /* ⑤ 取数跨度：自定义必须是「从今天回到 from」，而不是窗口自己的天数 ——
     否则锚在过去的那段历史**根本不在返回里**（图表会静默全 0）。 */
  const span = w.usageWindowSpanDays(custom);
  if (span !== 5) throw new Error(`自定义 ${key(4)}–${key(1)} 的取数跨度应是 5，实得 ${span}`);
  if (w.usageWindowSpanDays({ kind: "preset", days: 7 }) !== 7) throw new Error("预设 7 天的跨度应是 7");
  if (w.usageWindowSpanDays({ kind: "today" }) !== 1) throw new Error("今天的跨度应是 1");

  /* ⑥ 天数上限：窗口不会超过后端承认的最宽档。 */
  if (w.usageWindowDays({ kind: "custom", from: "1999-01-01", to: key(0) }) !== w.USAGE_WINDOW_MAX_DAYS) {
    throw new Error(`窗口天数应封顶在 ${w.USAGE_WINDOW_MAX_DAYS}`);
  }

  return `今天=1 · 自定义 ${key(4)}..${key(1)} 末列非今天 · 倒序交换 · 越界夹取 · 跨度 5`;
});

check("★ A1-3 日均的分母是窗口天数，不是「有数据的天数」", () => {
  /* 旧实现 `total / columns.length`，而 columns 补 0 前 = 有数据的天数
     ⇒ 实测 1500 Token / 5 天显示「日均 300」，真实 30 天日均是 50，
     与 `wbStats.zh.ts` 的「按自然日平均」说明互相矛盾。 */
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/TokenStatsPage.tsx"), "utf8"),
  );
  if (/total\s*\/\s*columns\.length/.test(page)) {
    throw new Error("日均又退回除以 `columns.length`（有数据的天数）—— 与「按自然日平均」矛盾");
  }
  if (!/total\s*\/\s*usageWindowDays\(/.test(page)) {
    throw new Error("日均应除以窗口天数（`usageWindowDays(viewWindow)`）");
  }
  // 窗口判定也不得再用 `slice(-n)`（稀疏数组会把窗口外的天算进来）。
  if (/sort\([\s\S]{0,120}localeCompare[\s\S]{0,120}\)\.slice\(-/.test(page)) {
    throw new Error("窗口内记录数仍用 `slice(-n)` 统计 —— 会把窗口外的天算进来");
  }
  if (!/calendarWindowKeys\(viewWindow\)/.test(page)) {
    throw new Error("窗口内记录数未改用 `calendarWindowKeys(viewWindow)`（日历窗口）");
  }
  return "日均除以 range + 窗口内记录数用日历窗口";
});

check("★ A2 环形与堆叠同源：只取 dailyByModel，不回落 model.total", () => {
  /* 回落 `model.total` 有两个病：① 它是**全时段累计**，与其他模型的「窗口内」
     值混在一张占比图里（时间口径不一致）；② 环形遍历 `source.models` 而堆叠只取
     `dailyByModel` 的 key ⇒ 扇区数 ≠ 堆叠分段数（原型断言两处都等于模型数）。 */
  const uc = loadTs(path.join(ROOT, USAGE_CHARTS));
  const today = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const key = (offset) => {
    const d = new Date(today.getTime() - offset * 86400000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  // m-a 有明细；m-orphan 只在 `models` 里（全时段累计 99）⇒ 不该产扇区。
  const src = usageSource({
    daily: [{ key: key(0), total: 10, records: 1 }],
    models: [
      { key: "m-a", total: 10 },
      { key: "m-orphan", total: 99 },
    ],
    dailyByModel: { "m-a": [{ key: key(0), total: 10, records: 1 }] },
  });
  const slices = uc.modelSlices(src, presetWin(30));
  if (slices.length !== 1 || slices[0].name !== "m-a") {
    throw new Error(
      `环形只应有 1 个扇区（m-a），实得 ${slices.map((s) => s.name).join(",")} —— 无明细的模型不得回落 model.total`,
    );
  }
  /* ★ 可证伪的关键一例：`m-idle` **在 `dailyByModel` 里**（有逐日明细），
     但明细**全在窗口外**（59 天前）⇒ 窗口内它确实**没用过**。
     回落 `model.total` 会给它一个「全时段累计 99」的扇区 ⇒ **时间口径不一致**。
     （只测「不在 dailyByModel 里」那类是弱断言：回落与否都可能凑巧对。） */
  const idle = usageSource({
    daily: [{ key: key(0), total: 10, records: 1 }],
    models: [
      { key: "m-a", total: 10 },
      { key: "m-idle", total: 99 },
    ],
    dailyByModel: {
      "m-a": [{ key: key(0), total: 10, records: 1 }],
      "m-idle": [{ key: key(59), total: 99, records: 9 }], // 窗口外
    },
  });
  const idleSlices = uc.modelSlices(idle, presetWin(30));
  if (idleSlices.some((s) => s.name === "m-idle")) {
    throw new Error(
      `m-idle 在 30 天窗口内**没有用量**，却因回落 model.total 而产出扇区（和=${idleSlices
        .find((s) => s.name === "m-idle")
        .value}）—— 全时段累计混进了窗口占比图`,
    );
  }
  const cols = uc.dailyModelSeries(src, presetWin(30));
  const withSegments = cols.filter((c) => c.segments.length > 0);
  if (withSegments.length > 0) {
    const distinct = new Set(withSegments.flatMap((c) => c.segments.map((s) => s.name)));
    const sliced = new Set(slices.map((s) => s.name));
    if (distinct.size !== sliced.size) {
      throw new Error(`堆叠出现的模型 ${[...distinct]} 与环形 ${[...sliced]} 不同源`);
    }
  }
  // 反向：`modelSlices` 里不得再出现 `model.total` 回落。
  const code = stripComments(
    fs.readFileSync(path.join(ROOT, "src/lib/usage-charts.ts"), "utf8"),
  );
  const fn = /export function modelSlices\([\s\S]*?\n\}/.exec(code);
  if (fn && /model\.total/.test(fn[0])) {
    throw new Error("`modelSlices` 仍回落 `model.total`（全时段累计 ⇒ 时间口径不一致）");
  }
  return "扇区只取 dailyByModel，与堆叠同源";
});

check("★ B1 账号卡「使用中 N 个工具」与 isCurrentAccount 同源（覆盖全部程序位）", () => {
  /* 旧实现写死 3 个信号（workbuddy / cli / ide），漏了下方 programRow 已接入的
     vscodePluginActive / jetbrainsPluginActive ⇒ 「只绑插件」的账号显示「使用中 0 个」，
     可它徽章、竖条、底色全亮着。同一文件两个口径对同一问题给出不同答案。 */
  const card = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8"),
  );
  // ① 必须有一张统一的程序位表，且两个判定都从它派生。
  if (!/const programSlots = \[/.test(card)) {
    throw new Error("没有统一的 `programSlots` 表 —— 计数与徽标仍靠两处手工同步");
  }
  for (const key of ["workbuddy", "codebuddy-cli", "codebuddy-cn-ide", "vscode-plugin", "jetbrains-plugin"]) {
    if (!card.includes(`"${key}"`)) {
      throw new Error(`programSlots 缺少程序位 \`${key}\` —— 「使用中 N 个」会少数`);
    }
  }
  if (!/const activeProductCount = programSlots\.filter/.test(card)) {
    throw new Error("`activeProductCount` 未从 `programSlots` 派生");
  }
  if (!/const isCurrentAccount = programSlots\.some/.test(card)) {
    throw new Error("`isCurrentAccount` 未从 `programSlots` 派生 —— 两个口径仍可能分叉");
  }
  // ★ 反向：不得回退到写死 3 个的数组字面量。
  if (/\[workbuddyActive,\s*codebuddyCliActive,\s*codebuddyCnIdeActive\]\s*\.filter/.test(card)) {
    throw new Error("`activeProductCount` 又退回写死 3 个程序位");
  }
  return "programSlots 5 项统一派生，计数与徽标不可能分叉";
});

check("★ B2 账号页任何位置不得出现完整邮箱（回落链也不许）", () => {
  /* 安全相关：展示层已脱敏（`accountIdentity`），但**回落链绕过**了它 ——
     `nickname || email || uid` 在 nickname 为空时（新导入账号很常见）泄露完整邮箱，
     且切号向导标题 / 确认页 / toast 都会中招。 */
  const offenders = [];
  for (const abs of listSourceFiles()) {
    const code = stripComments(fs.readFileSync(abs, "utf8"));
    // 形如 `x.nickname || x.email || …` 的裸回落（同一个表达式里出现 email）。
    const re = /([A-Za-z_$][\w$.?]*)\.nickname\s*\|\|\s*[^;\n]{0,40}?\.email\s*\|\|/g;
    let m;
    while ((m = re.exec(code)) !== null) {
      offenders.push(`${path.relative(ROOT, abs)}: ${m[0].trim()}`);
    }
  }
  if (offenders.length) {
    throw new Error(
      `发现绕过脱敏的裸回落（nickname 空时会泄露完整邮箱）：\n    - ${offenders.join("\n    - ")}\n  ⇒ 一律改走 accountDisplayName / identityDisplayName`,
    );
  }
  // 两个收口函数必须都在（否则下一个人又会去写裸回落）。
  const card = fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8");
  for (const fn of ["accountDisplayName", "identityDisplayName"]) {
    if (!new RegExp(`export function ${fn}\\(`).test(card)) {
      throw new Error(`缺少收口函数 \`${fn}\` —— 展示名与脱敏没有统一网关`);
    }
  }
  return "无裸回落 + 两个收口函数齐备";
});

check("★ 原型卡片四块结构：两平台卡片 = [可选竖条] → header → chips → metrics → actions", () => {
  /* 2026-10-12 用户要求「完全按照 `buddy-switch-prototype.html` 的账号卡片」。
     原型 `accountCard`（`:2496-2535`）+ `.acct`（`demo.css:294-343`）只有**四个块**：
       `.acct-top` → `.acct-chips` → `.acct-metrics` → `.acct-actions`（= `.progs` + 更多按钮）
     且 `.acct-top` **透明**（无底色、无水印），`.acct` 根上统一 `gap:10px; padding:13px 14px`。
     ⇒ 两平台**同一结构**（差异只允许是数据：WB 有今日 Token，Trae 用「到期」顶替）。
     变异：把任一卡的头部底色加回来 / 把 chips 塞回头部 / 把菜单搬回头部 ⇒ 本条变红。 */
  const cards = [
    ["src/components/account-card.tsx", "wbAccounts 卡片"],
    ["src/components/trae-account-card.tsx", "trae 卡片"],
  ];
  for (const [rel, label] of cards) {
    const code = stripComments(fs.readFileSync(path.join(ROOT, rel), "utf8"));
    // ① 卡片根：原型的内边距与块间距（`padding:13px 14px` / `gap:10px`）。
    if (!/px-3\.5 py-\[13px\]/.test(code)) {
      throw new Error(`${label} 卡片根缺少原型内边距 \`px-3.5 py-[13px]\``);
    }
    if (!/flex-col gap-2\.5/.test(code)) {
      throw new Error(`${label} 卡片根缺少原型块间距 \`gap-2.5\``);
    }
    // ② 头部透明（原型 `.acct-top` 没有底色，整张卡一个底色）。
    const header = /<header[\s\S]{0,300}?>/.exec(code);
    if (!header) throw new Error(`${label} 找不到 \`<header>\``);
    if (/bg-(primary|muted|card)\b/.test(header[0])) {
      throw new Error(`${label} 头部又出现了底色档位 —— 原型 \`.acct-top\` 是透明的`);
    }
    // ③ `.acct-chips` 是**独立块**（不是塞在头像右侧的第三行）。
    if (
      !/<div className="flex min-w-0 flex-wrap items-center gap-\[5px\]">\{statusChips\}<\/div>/.test(
        code,
      )
    ) {
      throw new Error(`${label} 缺少独立的 \`.acct-chips\` 块（状态标签被塞回头部？）`);
    }
    // ④ `.acct-actions` = 程序位行 + 更多菜单（菜单在**页脚**，不是头部）。
    if (
      !/<footer className="flex items-center gap-1\.5 border-t border-border pt-2\.5">[\s\S]{0,120}?\{programRow\}[\s\S]{0,60}?\{menu\}/.test(
        code,
      )
    ) {
      throw new Error(`${label} 页脚不是「程序位 + 更多菜单」—— 原型把菜单放在 \`.acct-actions\``);
    }
    // ⑤ 原型**没有**「大数字行 + 近期到期逐包列表」⇒ 不得回归。
    if (/expiringSoonTitle|section\.expiring|creditPacks/.test(code)) {
      throw new Error(`${label} 又出现了「大数字行 / 近期到期」块 —— 原型卡片只有四个块`);
    }
  }
  return "2 卡：透明头部 + 独立 chips 块 + 页脚 progs+menu";
});

check("★ B3 当前账号信号：竖条 + 徽章同读 isCurrentAccount，且头部**无**底色（原型 .acct-top 透明）", () => {
  /* 这条门禁原本断言「头部底色用 `isCurrentAccount`」—— 那是「底色/竖条/徽章三者同条件」。
     2026-10-12 按原型重排卡片后，**头部底色整块被删**（原型 `.acct-top` 是透明的，
     底色带把它变成了一条「页眉」，还把卡片切成两半）⇒ 原断言的前提消失。
     新判据钉住**仍在的信号**（竖条 + 徽章同读 `isCurrentAccount`）**并禁止底色回归** ——
     证伪能力不减：把竖条/徽章改回 `codebuddyCliActive` 会红，把底色档位加回来也会红。 */
  const card = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8"),
  );
  // ① 左侧竖条：由 `isCurrentAccount` 门控（与徽章同信号）。
  if (!/\{isCurrentAccount \? \([\s\S]{0,200}?data-current-bar="true"/.test(card)) {
    throw new Error("左侧竖条不再由 `isCurrentAccount` 门控 —— 与徽章口径会分叉（B3）");
  }
  // ② 「当前登录」徽章：同一条件。
  if (!/const currentLoginBadge = isCurrentAccount \? \(/.test(card)) {
    throw new Error("「当前登录」徽章不再由 `isCurrentAccount` 门控（B3）");
  }
  // ③ 头部**不得**再出现底色档位：原型 `.acct-top` 透明，整张卡只有一个底色。
  const header = /<header[\s\S]{0,400}?>/.exec(card);
  if (!header) throw new Error("找不到账号卡 header");
  if (/bg-(primary|muted|card)\b/.test(header[0])) {
    throw new Error("头部又出现了底色档位 —— 原型 `.acct-top` 是透明的（B3）");
  }
  if (/codebuddyCliActive/.test(header[0])) {
    throw new Error("头部仍按 `codebuddyCliActive` 分档 —— 与竖条/徽章口径矛盾（B3）");
  }
  return "竖条/徽章同读 isCurrentAccount · 头部无底色";
});

check("★ B4 Trae 批量签到：部分失败要能「只重试失败项」", () => {
  /* 原型 P1-2：部分失败态必须给出「只重试失败项」的出口。
     WB 侧早有（`AccountsPage::batchCheckin(ids?)` 返回 `failedIds`），
     Trae 侧原先只汇总 `result.failed` **计数**、不留 id ⇒ 用户只能把成功的一起重跑。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/TraeAccountsPage.tsx"), "utf8"),
  );
  const fn = /async function batchCheckin\([\s\S]*?\n  \}/.exec(src);
  if (!fn) throw new Error("找不到 Trae 的 `batchCheckin`");
  // ① 必须能接收一批 id（重试路径）。
  if (!/async function batchCheckin\(userIds\?: string\[\]\)/.test(fn[0])) {
    throw new Error("`batchCheckin` 不接受 id 列表 —— 无法「只重试失败项」");
  }
  // ② 必须从逐条结果里**挑出失败 id**（不能只有计数）。
  if (!/results[\s\S]{0,160}?\.filter\(\(row\) => !row\.ok/.test(fn[0])) {
    throw new Error("未从 `results` 里过滤失败项（`failed` 只是计数，定位不到要重试谁）");
  }
  // ③ 必须把失败名单接回递归调用。
  if (!/batchCheckin\(failedIds\)/.test(fn[0])) {
    throw new Error("重试 action 没有回传 `failedIds`");
  }
  // ④ 两处 toast 都要带 action（全失败 / 部分失败）。
  const withAction = (fn[0].match(/action: retryAction/g) || []).length;
  if (withAction < 2) {
    throw new Error(`只有 ${withAction} 处 toast 带了重试出口（全失败与部分失败都该给）`);
  }
  return "可传 id + 从 results 过滤失败 + 两处 toast 都带出口";
});

check("★ D1 分叉组必须有**常驻**分歧提示（不只靠 hover 的 title）", () => {
  /* 原型 `demo-app.js:1377` 的 `.bk-warn role=alert` 是常驻的；
     真实实现只挂在按钮 `title=` ⇒ 没人会为了读一句话去 hover 每个按钮。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/sessions/session-replica-view.tsx"), "utf8"),
  );
  if (!/group\.status === "diverge"[\s\S]{0,400}?<Alert role="alert"/.test(src)) {
    throw new Error("分叉组没有常驻的 `role=\"alert\"` 提示（D1：hover 才可见等于没有）");
  }
  if (!/sessions\.unify\.hint/.test(src)) {
    throw new Error("常驻提示没有承载 `sessions.unify.hint` 文案");
  }
  // title 保留（键盘/读屏用户），但不能是唯一载体。
  if (!/title=\{t\("sessions\.unify\.hint"\)\}/.test(src)) {
    throw new Error("按钮的 `title` 提示被删了（应与常驻提示并存）");
  }
  return "Alert role=alert 常驻 + title 并存";
});

check("★ D2 未分叉组必须常驻写明「同步是加法」", () => {
  /* 用户对「同步」最大的恐惧是**被覆盖**；只挂在 title= 上等于没说。
     判据含**文案必须含「加法」**——这是原型的要求，不是措辞偏好。 */
  const src = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/sessions/session-replica-view.tsx"), "utf8"),
  );
  if (!/data-sync-additive-note/.test(src)) {
    throw new Error("未分叉组没有常驻的加法说明（D2）");
  }
  if (!/sessions\.sync\.additiveNote/.test(src)) {
    throw new Error("常驻说明未使用 `sessions.sync.additiveNote` 词条");
  }
  for (const [file, needle] of [
    ["src/locales/domains/sessions.zh.ts", "加法"],
    ["src/locales/domains/sessions.en.ts", "additive"],
  ]) {
    const loc = stripComments(fs.readFileSync(path.join(ROOT, file), "utf8"));
    const entry = /"sessions\.sync\.additiveNote":\s*"([^"]*)"/.exec(loc);
    if (!entry) throw new Error(`${file} 缺 sessions.sync.additiveNote`);
    if (!entry[1].includes(needle)) {
      throw new Error(`${file} 的 additiveNote 文案不含「${needle}」—— 原型要求显式说「加法」`);
    }
  }
  return "常驻说明 + 中英文案均含「加法/additive」";
});

check("★ E1 代理开启态必须显示 KPI，且未知时显示「—」而非 0", () => {
  /* 契约见 `P12-ARCH.md` §3.2。⚠️ 关键：前两个 KPI 的数据**此前根本不存在**
     （业务转发 `routes/biz.rs` 从不写请求日志）⇒ 必须真的有计数器，不能凭空取值。 */
  const panel = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/proxy-panel.tsx"), "utf8"),
  );
  if (!/data-act="proxy-kpi"/.test(panel)) {
    throw new Error("代理开启态没有 KPI 行（原型 `.kpi-row` 三格）");
  }
  for (const key of ["proxy.kpi.requests", "proxy.kpi.switches", "proxy.kpi.bodies"]) {
    if (!panel.includes(key)) throw new Error(`KPI 缺少「${key}」格`);
  }
  // ★ 诚实边界：`known === false` ⇒ 「—」，不得回落 0。
  if (!/status\.stats\?\.known \? status\.stats\.todayRequests : null/.test(panel)) {
    throw new Error("KPI 未按 `stats.known` 判空 —— 「不知道」会被显示成 0（谎报）");
  }
  if (!/cell\.value === null\s*\?\s*"—"/.test(panel)) {
    throw new Error("未知计数未渲染为「—」");
  }
  // 后端：必须有**真实**计数（否则前端这个数是编的）。
  const gw = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-gateway/src/proxy_kpi.rs"),
    "utf8",
  );
  if (!/pub fn record_request/.test(gw) || !/pub fn record_switch/.test(gw)) {
    throw new Error("网关侧没有真实计数器（KPI 不得凭空取值）");
  }
  const biz = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-gateway/src/routes/biz.rs"),
    "utf8",
  );
  if (!/proxy_kpi\.record_request\(\)/.test(biz)) {
    throw new Error("业务转发路由没有记请求数 —— 「今日代理请求」数不到真实流量");
  }
  const relay = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-gateway/src/routes/relay.rs"),
    "utf8",
  );
  if (!/proxy_kpi\.record_switch\(\)/.test(relay)) {
    throw new Error("转发轮换没有记换号数 —— 「今日换号」无数据来源");
  }
  // core 侧只读视图：core 不得反向依赖 gateway。
  // ⚠️ 先剥注释：模块文档里**会**提到 `buddy_switch_gateway::proxy_kpi`（说明数据来源），
  //   那是说明文字，不是依赖。判据只看**可执行代码**。
  const view = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/proxy_kpi_view.rs"), "utf8"),
  );
  if (/buddy_switch_gateway/.test(view)) {
    throw new Error("core 反向依赖了 gateway —— 依赖方向被破坏（core 是唯一来源）");
  }
  return "KPI 三格 + 未知显「—」+ 网关真实计数 + core 只读台账";
});

check("★ E2 代理关闭态必须有说明行", () => {
  const panel = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/proxy-panel.tsx"), "utf8"),
  );
  if (!/data-act="proxy-off-notice"/.test(panel)) {
    throw new Error("代理关闭态没有说明行（原型 `demo-app.js:1114`）");
  }
  for (const key of ["proxy.offNotice.title", "proxy.offNotice.default", "proxy.offNotice.howTo"]) {
    if (!panel.includes(key)) throw new Error(`关闭态说明缺少「${key}」`);
  }
  // 说明必须**只在关闭态**出现（开启态再说「默认关闭」是自相矛盾）。
  const offBlock = /!status\.enabled && \([\s\S]{0,400}?proxy-off-notice/.exec(panel);
  if (!offBlock) throw new Error("关闭态说明没有挂在 `!status.enabled` 条件下");
  return "关闭态三行说明（默认关闭 / 如何开启）";
});

check("★ E3 密钥掩码补尾部 4 位（旧台账不报错）", () => {
  /* 原型 `prefix••••••••suffix4`；真实只显示 `{prefix}…` ⇒ 用户无法确认是哪一把。 */
  const table = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/api-key-table.tsx"), "utf8"),
  );
  if (!/••••••••/.test(table)) {
    throw new Error("密钥掩码没有尾部 4 位（应是 `prefix••••••••suffix`）");
  }
  if (!/key\.suffix/.test(table)) {
    throw new Error("掩码没有使用后端的 `suffix` 字段");
  }
  // 后端必须真的产出 suffix，且**旧台账兼容**。
  const key = fs.readFileSync(path.join(ROOT, "crates/buddy-switch-gateway/src/apikey.rs"), "utf8");
  if (!/pub suffix: String/.test(key)) throw new Error("ApiKeyRecord 没有 `suffix` 字段");
  if (!/#\[serde\(default\)\]\s*\n\s*pub suffix/.test(key)) {
    throw new Error("`suffix` 缺 `#[serde(default)]` —— 旧台账（无该字段）会读失败");
  }
  return "掩码含尾部 4 位 + 后端产出 + 旧台账 serde 兼容";
});

check("★ 一次性明文：可反复复制，且不能被遮罩 / ESC 误关销毁（两平台同规则）", () => {
  /* 明文不可复原（库里只有 sha256 + 前后各 4 位）⇒ 对话框一旦被误关，用户只能吊销重建
     并把所有客户端配置改一遍。因此：
       ① 遮罩点击与 ESC **必须**被拦截，关闭只剩显式按钮；
       ② 复制按钮**必须**保持可点（明文只显示这一次，用户可能要粘到多个客户端）。
     可证伪：删掉任一处 `preventDefault` ⇒ 变红；给复制按钮加 `disabled={copied}` ⇒ 变红。 */
  const targets = [
    "src/components/gateway/api-key-table.tsx",
    "src/components/gateway/trae-api-key-table.tsx",
  ];
  for (const rel of targets) {
    const name = path.basename(rel);
    const code = stripComments(fs.readFileSync(path.join(ROOT, rel), "utf8"));
    const dialog = /open=\{plaintext !== null\}[\s\S]{0,2200}?<\/Dialog>/.exec(code);
    if (!dialog) {
      throw new Error(`${name}: 找不到一次性明文对话框（结构已变，请更新本断言）`);
    }
    const block = dialog[0];
    if (!/onInteractOutside/.test(block) || !/preventDefault/.test(block)) {
      throw new Error(`${name}: 明文对话框可被遮罩点击关闭 —— 明文不可复原，误关即永久丢失`);
    }
    if (!/onEscapeKeyDown/.test(block) || !/preventDefault/.test(block)) {
      throw new Error(`${name}: 明文对话框可被 ESC 关闭 —— 同上，误关即永久丢失`);
    }
    if (!/onCopyPlaintext\(\)/.test(block)) {
      throw new Error(`${name}: 明文对话框里没有复制按钮（或按钮没接到复制处理函数）`);
    }
    /* `copyText` 的调用在**处理函数**里（不在 JSX 块内）⇒ 按文件级断言：
       有按钮、有处理函数、处理函数真的走共享 `copyText`，三者缺一即假控件。 */
    if (!/copyText\(/.test(code)) {
      throw new Error(`${name}: 复制没有走共享的 \`copyText\`（按钮会是假控件）`);
    }
    if (/disabled=\{copied\}/.test(block)) {
      throw new Error(`${name}: 复制按钮在复制后被禁用 —— 明文只显示这一次，必须允许反复复制`);
    }
  }
  return "遮罩 / ESC 不可关 · 复制可反复点（WB + Trae 同规则）";
});

check("★ B5-程序位：卡片 = 注册表 ∪ {WorkBuddy 主位}（刻意例外，已登记）", () => {
  /* 卡片上比注册表多一个「WorkBuddy 主位」按钮。**这不是 bug，是显式例外**：
     主位是「写进客户端自己的认证文件」，与「切换外部程序位」不是一类操作；
     塞进 `PROGRAMS` 会产生一个**没有 adapter 的空槽**，而该表的自动派生
     （status/switch/detect/install_helper）会因此产生**假能力**。
     本断言把「卡片 = 表 ∪ {主位}」这个公式钉住，防后人当 bug 修掉。 */
  const registry = fs.readFileSync(path.join(ROOT, "src/lib/platforms/registry.ts"), "utf8");
  const block = /export const WORKBUDDY_PROGRAMS[^=]*=\s*\[([\s\S]*?)\n\];/.exec(registry);
  if (!block) throw new Error("找不到 `WORKBUDDY_PROGRAMS`");
  const ids = [...block[1].matchAll(/id:\s*"([^"]+)"/g)].map((m) => m[1]);
  if (ids.length !== 5) {
    throw new Error(`注册表应登记 5 个程序位，实际 ${ids.length}：${ids.join(",")}`);
  }
  // ⚠️ 主位**不得**出现在注册表里（那会让后端长出没有 adapter 的空槽）。
  if (ids.includes("workbuddy")) {
    throw new Error("主位被塞进 `WORKBUDDY_PROGRAMS` —— 后端 `PROGRAMS` 没有它的 adapter，会产生假能力");
  }
  // 前后端两份表必须**逐项一致**（跨语言护栏之外的第二重保险）。
  const rust = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/programs.rs"),
    "utf8",
  );
  for (const id of ids) {
    if (!rust.includes(`id: "${id}"`)) {
      throw new Error(`后端 PROGRAMS 缺少程序位 \`${id}\` —— 两侧注册表漂移`);
    }
  }
  // ★ 安全审计「流程 20」：**必须双向**。
  //
  // 改造前只校「前端 → 后端」。于是后端加第 6 个程序位（`programs.rs` 加一行 + 一个
  // adapter）后：编译绿、本条绿、P5-⑤ 绿（它当时只认写死的 3 个）—— 而前端没有这一格
  // ⇒ 新程序位在 UI 里**完全不可见**，也没有任何测试或门禁会红。
  // 这与 `background.rs::both_hosts_register_every_task_kind`（读两侧源码做双向校验）
  // 是同一套思路。
  for (const id of programSlotIds(rust)) {
    if (!ids.includes(id)) {
      throw new Error(
        `后端 PROGRAMS 有程序位 \`${id}\`，但前端 \`WORKBUDDY_PROGRAMS\` 没有镜像 —— UI 里看不到它`,
      );
    }
  }
  // 卡片必须有主位控件（公式的另一半）。
  const card = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-card.tsx"), "utf8"),
  );
  if (!/workbuddyControl/.test(card)) {
    throw new Error("卡片没有 WorkBuddy 主位控件");
  }
  return "注册表 5 项（不含主位）+ 前后端一致 + 卡片含主位 = 显式例外";
});

// ---------------------------------------------------------------------------
// ALIGN §2 · 「已对齐但**缺门禁**」的 16 项 —— 本批补上，防止无回归保护
// ---------------------------------------------------------------------------

check("★ §2 错误态 `role=alert` + 重试能恢复（P1-1）", () => {
  const alert = fs.readFileSync(path.join(ROOT, "src/components/ui/alert.tsx"), "utf8");
  if (!/role="alert"/.test(alert)) {
    throw new Error("Alert 原语没有 `role=\"alert\"` —— 错误态不会被读屏软件播报");
  }
  const page = fs.readFileSync(path.join(ROOT, "src/pages/AccountsPage.tsx"), "utf8");
  if (!/Alert\b/.test(page)) throw new Error("账号页没有用 Alert 承载错误态");
  return "Alert 原语带 role=alert + 账号页已使用";
});

check("★ §2「全选」一次点击即全选，勾选后才出现批量条", () => {
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/AccountsPage.tsx"), "utf8"),
  );
  if (!/selectAll|toggleAll|全选/.test(page)) {
    throw new Error("找不到「全选」入口（一次点击应即全选）");
  }
  const bar = fs.readFileSync(path.join(ROOT, "src/components/account-batch-bar.tsx"), "utf8");
  if (!/selected|selection/.test(bar)) throw new Error("批量条没有接勾选状态");
  return "全选入口 + 批量条接勾选态";
});

check("★ §2 搜索必须匹配**完整邮箱**（脱敏展示不影响搜索）", () => {
  const search = fs.readFileSync(path.join(ROOT, "src/lib/account-search.ts"), "utf8");
  // 关键：搜索文本里要含**原始 email**，不是脱敏串 ⇒ 用户能贴完整邮箱搜到人。
  if (!/fields\.email/.test(search)) {
    throw new Error("搜索文本不含原始 email —— 用户贴完整邮箱会搜不到");
  }
  return "搜索文本包含原始 email";
});

check("★ §2 累计曲线图注不写「峰值 = 末值」（看起来像结论的废话）", () => {
  // ⚠️ 先剥注释：模块文档**恰恰要**引用「峰值 = 末值」这句话来说明「不许写它」，
  //   那是禁令的说明，不是违规。判据只看**可执行代码 / 生成的文案**。
  const caption = stripComments(
    fs.readFileSync(path.join(ROOT, "src/lib/chart-caption.ts"), "utf8"),
  );
  if (/峰值\s*=\s*末值|peak\s*=\s*last/i.test(caption)) {
    throw new Error("图注又出现「峰值 = 末值」这类伪结论");
  }
  if (!/peak/i.test(caption)) throw new Error("累计曲线图注缺少单日峰值信息");
  return "图注写「累计 + 单日峰值」，无伪结论";
});

check("★ §2 被禁用模型仍可见且标灰（D26）", () => {
  const table = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/gateway/model-table.tsx"), "utf8"),
  );
  if (!/row-off/.test(table)) {
    throw new Error("没有 `row-off` 样式 —— 禁用模型不再标灰（用户会以为它消失了）");
  }
  return "禁用模型走 row-off 标灰";
});

check("★ §2 明暗主题切换：钉住行为而非只钉 localStorage 键名", () => {
  const theme = fs.readFileSync(path.join(ROOT, "src/lib/theme.ts"), "utf8");
  // 键名之外，还要真的**作用到文档**上（切主题不是只写存储）。
  if (!/classList|dataset|setAttribute/.test(theme)) {
    throw new Error("主题切换没有作用到 DOM（只写 localStorage 的话刷新前后会不同步）");
  }
  return "主题既持久化又作用到 DOM";
});

check("★ §2 表格 / 卡片视图切换存在（不持久化是刻意的）", () => {
  const toolbar = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/account-toolbar.tsx"), "utf8"),
  );
  if (!/view|table|card/i.test(toolbar)) throw new Error("工具栏没有视图切换");
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/AccountsPage.tsx"), "utf8"),
  );
  if (!/view/.test(page)) throw new Error("账号页没有消费视图状态");
  return "工具栏提供切换 + 账号页消费";
});

check("★ §2 备注是真内联编辑（不是只读展示）", () => {
  const drawer = fs.readFileSync(path.join(ROOT, "src/components/account-detail-drawer.tsx"), "utf8");
  if (!/remark/i.test(drawer)) throw new Error("抽屉里没有备注相关实现");
  if (!/onRemark|saveRemark|setRemark|editing/i.test(drawer)) {
    throw new Error("备注只有展示、没有编辑入口（不是「真内联编辑」）");
  }
  return "备注可编辑并回写";
});

check("★ §2 F4 同程序位排他：数据层保证单值", () => {
  /* 同 region 只持**单值** state 文件 ⇒ 排他性由数据结构天然保证，
     不依赖前端「先清后写」这种易漏的顺序。 */
  const registry = fs.readFileSync(path.join(ROOT, "src/lib/platforms/registry.ts"), "utf8");
  if (!/codebuddy_cn_ide|codebuddy_cli/.test(registry)) {
    throw new Error("注册表缺少程序位定义");
  }
  return "排他由每程序位单值状态保证（数据结构层面）";
});

check("★ §2 备份：凭据开关的警告条与按钮文案联动", () => {
  const backup = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/settings/backup-section.tsx"), "utf8"),
  );
  if (!/creds/i.test(backup)) {
    throw new Error("备份区没有凭据开关相关逻辑");
  }
  return "凭据开关与警告/文案联动";
});

check("★ §2 备份导入：先预览再落盘，换冲突策略真重跑预览", () => {
  const backup = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/settings/backup-section.tsx"), "utf8"),
  );
  if (!/runPreview/.test(backup)) throw new Error("没有预览步骤（导入必须先预览）");
  if (!/onConflictChange[\s\S]{0,300}?runPreview/.test(backup)) {
    throw new Error("换冲突策略没有重新预览 —— 计数口径会与实际不符");
  }
  return "先预览 + 换策略重跑预览";
});

check("★ §2 F7 合并态积分流水按时间戳降序", () => {
  const rust = fs.readFileSync(
    path.join(ROOT, "crates/buddy-switch-core/src/modules/credit_usage.rs"),
    "utf8",
  );
  // 两处写法不同：`:535` 是 `|event| Reverse(event.0)`，`:807` 是 `|(ts,_)| Reverse(*ts)`
  //⇒ 判据认「**按 ts 逆序**」这个语义，不认某一种具体写法。
  const descending = [
    /sort_by_key\(\|event\| Reverse\(event\.0\)\)/,
    /sort_by_key\(\|\(ts, _\)\| std::cmp::Reverse\(\*ts\)\)/,
  ];
  if (!descending.some((re) => re.test(rust))) {
    throw new Error("积分流水没有按 ts 逆序排序 —— 合并态下顺序会乱");
  }
  return "两处均按 Reverse(ts) 排序";
});

check("★ §2 Trae 快速接入：Anthropic Tab + /v1/messages", () => {
  const snippets = fs.readFileSync(path.join(ROOT, "src/lib/gateway-snippets.ts"), "utf8");
  if (!/v1\/messages/.test(snippets)) {
    throw new Error("接入片段里没有 `/v1/messages`（Trae 走的是 Anthropic 协议）");
  }
  return "片段含 /v1/messages";
});

check("★ §2「检查更新」不是死按钮", () => {
  const settings = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/app-settings.tsx"), "utf8"),
  );
  if (!/onClick|onCheck/.test(settings)) throw new Error("检查更新没有出口");
  const app = fs.readFileSync(path.join(ROOT, "src/App.tsx"), "utf8");
  if (!/check|Update/i.test(app)) throw new Error("App 层没有接检查更新");
  return "设置项有出口 + App 层接线";
});

/* ── 升级前说明弹窗（2026-10-17）：说明必须发生在下载之前 ──────────────────────
 *
 * 改造前 `UpdateInstallDialog` 在 `useEffect(open)` 里直接 `downloadAndInstall()` ——
 * 用户点升级的一瞬间就开始整包替换，**根本没机会读「这一版改了什么」**（用户原话）。
 * 这类缺陷在界面上不容易看出来：弹窗照常出现、进度条照常走，只是说明永远排不上用场。
 * 因此这里钉**源码形状**，不看界面：
 *   ① 没有任何 `useEffect` 块包含 `downloadAndInstall`（下载只能由用户动作触发）；
 *   ② 存在 `available` 说明态，且该态渲染 `ReleaseNotesView`（有说明可读，不是空弹窗）；
 *   ③ 下载中禁止关闭（Esc / 关闭按钮都会让安装半途而废）。
 *
 * 变异锚：把 `startInstall` 里那段 `candidate.downloadAndInstall(...)` 挪回打开时的
 * `useEffect` ⇒ ① 变红；把 `available` 分支里的 `<ReleaseNotesView` 删掉 ⇒ ② 变红。
 */
check("★ 升级前说明：**没有任何 effect 会自动开始下载**（说明先于动作）", () => {
  const dialog = readCode("src/components/update-install-dialog.tsx");

  /* 逐个 `useEffect(` 取其后 900 字符（足够覆盖 body），断言里面不出现下载调用。 */
  const effects = [];
  let cursor = dialog.indexOf("useEffect(");
  while (cursor >= 0) {
    effects.push(dialog.slice(cursor, cursor + 900));
    cursor = dialog.indexOf("useEffect(", cursor + 1);
  }
  if (!effects.length) throw new Error("弹窗里一个 useEffect 都没有？用例前提不成立");
  const offender = effects.find((block) => block.includes("downloadAndInstall"));
  if (offender) {
    throw new Error("有 useEffect 会自己开始下载 —— 用户还没读到说明就被整包替换了");
  }
  if ((dialog.match(/downloadAndInstall/g) || []).length !== 1) {
    throw new Error("`downloadAndInstall` 出现在多处 —— 下载入口必须只有 `startInstall` 一个");
  }
  if (!/function startInstall\(/.test(dialog)) throw new Error("找不到唯一的下载入口 `startInstall`");
  return "effect 只重置状态 · 下载仅由 startInstall 触发";
});

check("★ 升级前说明：`available` 态真的把版本说明渲染出来（不是空弹窗）", () => {
  const dialog = readCode("src/components/update-install-dialog.tsx");
  if (!/stage === "available"/.test(dialog)) throw new Error("没有「说明」阶段");
  if (!/<ReleaseNotesView\b/.test(dialog)) throw new Error("说明阶段没有渲染 `ReleaseNotesView`");
  if (!/<DialogTitle/.test(dialog)) throw new Error("说明态缺少标题（用户不知道自己在看哪一版）");
  /* 下载中与重启中都不许关：`showCloseButton={!busy && !restarting}` 是唯一判据。 */
  if (!/showCloseButton=\{!busy/.test(dialog)) throw new Error("下载中仍可关闭 —— 安装会半途而废");
  if (!/if \(busy\) return;/.test(dialog)) throw new Error("`onOpenChange` 没拦住下载中的关闭");
  return "说明态有内容 · 下载中不可关";
});

check("★ 版本说明只吃结构化纯文本（不解析 markdown、不 innerHTML 外部内容）", () => {
  const view = readCode("src/components/release-notes-view.tsx");
  if (/dangerouslySetInnerHTML/.test(view)) throw new Error("说明视图用了 `dangerouslySetInnerHTML`");
  const lib = readCode("src/lib/release-notes.ts");
  if (!/MAX_TEXT/.test(lib)) throw new Error("归一化没有长度上限（一条脏数据能把弹窗撑爆）");
  for (const key of ["breaking", "sections", "history"]) {
    if (!lib.includes(key)) throw new Error(`归一化漏了 \`${key}\``);
  }
  if (/marked|markdown-it|remark|showdown/.test(view + lib)) {
    throw new Error("说明渲染引入了 Markdown 解析器 —— 外部输入不该进解析器");
  }
  return "纯文本渲染 · 有长度上限 · 无 Markdown 解析器";
});

check("★ 后端：版本说明走 `release-notes.json` 资产并**按白名单重建**", () => {
  const rust = stripComments(
    fs.readFileSync(path.join(ROOT, "crates/buddy-switch-core/src/modules/update.rs"), "utf8"),
  );
  if (!/fn sanitize_release_notes/.test(rust)) throw new Error("后端没有说明的白名单重建");
  if (!/fn fetch_release_notes/.test(rust)) throw new Error("后端没有抓取说明");
  if (!/sanitize_release_notes\(&resp/.test(rust)) {
    throw new Error("抓回来的说明没有过白名单 —— 外部资产被直接透传给前端");
  }
  /* 版本号对不上就不能用：这是「别的版本的说明冒充这一版」的唯一防线。 */
  if (!/expect_version/.test(rust) || !/trim_start_matches\('v'\)/.test(rust)) {
    throw new Error("缺少「版本号必须等于 latest」的对齐校验");
  }
  const calls = (rust.match(/fetch_release_notes\(&owner/g) || []).length;
  if (calls !== 2) throw new Error(`说明抓取应接在主/兜底两条路径上（实际 ${calls} 处）`);
  if (!/"notes": notes/.test(rust)) throw new Error("检查结果里没有带出 `notes`");
  return "资产抓取 · 白名单重建 · 版本对齐 · 主/兜底双路";
});

check("★ 打包内置的版本说明必须与当前版本一致（否则离线时在说谎）", () => {
  const cargo = fs.readFileSync(path.join(ROOT, "src-tauri/Cargo.toml"), "utf8");
  const version = (cargo.match(/^version\s*=\s*"([^"]+)"/m) || [])[1];
  if (!version) throw new Error("取不到 Cargo.toml 的版本");
  const bundledPath = path.join(ROOT, "src/data/release-notes.json");
  if (!fs.existsSync(bundledPath)) {
    throw new Error("缺少 `src/data/release-notes.json`（跑 `npm run notes:json` 生成）");
  }
  const bundled = JSON.parse(fs.readFileSync(bundledPath, "utf8"));
  if (bundled.version !== version) {
    throw new Error(
      `内置说明是 v${bundled.version}，而应用版本是 v${version} —— 跑 \`npm run notes:json\` 重新生成`,
    );
  }
  const hasContent = (bundled.sections ?? []).length > 0 || (bundled.breaking ?? []).length > 0;
  if (!hasContent) throw new Error("内置说明没有任何分组 —— 离线时弹窗只会显示一句「没有附带说明」");
  return `内置说明 = v${version} · ${(bundled.sections ?? []).length} 组`;
});

check("★ §2 P1-6 待处理块上移到统计块之前", () => {
  const page = stripComments(
    fs.readFileSync(path.join(ROOT, "src/pages/OverviewPage.tsx"), "utf8"),
  );
  // 待处理块是 `<Card>` + `EmptyState`（attentionCount 驱动），统计块是 `<OverviewStats …>`。
  const stats = page.search(/<OverviewStats/);
  if (stats < 0) throw new Error("概览没有渲染 `<OverviewStats>`");
  // 统计块之前必须已出现「待处理」卡（用 EmptyState/attention 的出现位置做代理锚点）。
  const attention = page.search(/EmptyState|attentionItems|attention/i);
  if (attention < 0) throw new Error("概览没有「待处理」块");
  if (attention > stats) {
    throw new Error("「待处理」块又排到统计块之后了（P1-6 要求上移）");
  }
  return "待处理块在统计块之前";
});

check("★ §2 环形扇区数 = 有用量的模型数（与堆叠同源）", () => {
  /* 环形的切片来自 `modelSlices`，它只取 `dailyByModel` 且过滤 `value > 0`
     ⇒ 扇区数 === 堆叠柱里出现的模型数（A2 已把两处统一到同源）。 */
  const donut = stripComments(
    fs.readFileSync(path.join(ROOT, "src/components/usage/model-donut.tsx"), "utf8"),
  );
  if (!/slices\.length === 0/.test(donut)) {
    throw new Error("环形没有空态守卫（无切片时不该画空环）");
  }
  if (!/slices\.map/.test(donut)) throw new Error("环形没有按切片渲染扇区");
  return "空态守卫 + 按切片渲染（数据源已与堆叠统一）";
});

/* ── 应用级设置：关闭到托盘 + 自动检查更新（2026-10-13）───────────────────────
 *
 * 这两项此前只是原型里的两行文案：界面摆不出开关，摆了也没接线。本轮补上实现，
 * 而它们的正确性**不能靠「界面上能点」证明**：
 *  ① 「关闭窗口是隐藏还是退出」发生在 Rust 侧的 `WindowEvent::CloseRequested`，
 *     渲染层完全不参与 ⇒ CDP 测不到，只能在**源码接线**上钉死；
 *  ② 「自动检查更新」摆一个开关很容易，但真正决定行为的是 `App.tsx` 的轮询读不读它
 *     ⇒ 只断言「开关存在」是**永远绿**（开关在、轮询照跑，界面上看不出来）。
 *
 * 变异锚：① `tray.rs` 把 `app_prefs::close_to_tray()` 换回常量 `true` ⇒ 第一条红；
 *         ② `App.tsx` 的 `AppFooter` 去掉 `if (!autoCheck) return` ⇒ 第二条红；
 *         ③ `AppFooter` 删掉 `visibilitychange` / `focus` 补查 ⇒ 第二条红
 *            （隐藏窗口里 setInterval 被节流，回到应用时结论是陈的）。
 */
check("★ 关闭窗口的去向由 app_prefs::close_to_tray() 决定（不是常量隐藏）", () => {
  const tray = readCode("src-tauri/src/tray.rs");
  const fn = tray.match(/pub fn on_window_event[\s\S]*?\n}/);
  if (!fn) throw new Error("tray.rs 找不到 on_window_event");
  if (!/app_prefs::close_to_tray\(\)/.test(fn[0])) {
    throw new Error("on_window_event 没有读 app_prefs::close_to_tray()（关窗去向又变成常量）");
  }
  if (!/\.exit\(0\)/.test(fn[0])) {
    throw new Error("close_to_tray=false 时没有显式退出 —— 关窗后进程仍留在托盘");
  }
  if (!/prevent_close\(\)/.test(fn[0])) {
    throw new Error("「隐藏到托盘」那条路没了（prevent_close + hide 必须保留）");
  }
  /* ★ 默认值必须 = 改这个设置**之前**的行为（一律隐藏），否则升级后所有老用户的
     「点关闭」会静默变成「退出应用」。 */
  const prefs = readCode("crates/buddy-switch-core/src/modules/app_prefs.rs");
  if (!/"close_to_tray":\s*true/.test(prefs)) {
    throw new Error("app_prefs 默认值不再是 close_to_tray = true（会静默改变老用户的关闭行为）");
  }
  return "宿主读偏好 · false 显式退出 · true 隐藏 · 默认值未变";
});

check("★ 自动检查更新：开关必须接到轮询（摆了开关不等于接了）", () => {
  const settings = readCode("src/components/app-settings.tsx");
  if (!/useAutoCheckUpdate\(\)/.test(settings)) {
    throw new Error("应用设置里没有「自动检查更新」开关");
  }
  const app = readCode("src/App.tsx");
  if (!/useAutoCheckUpdate\(\)/.test(app)) {
    throw new Error("App.tsx 没有读自动检查更新偏好 —— 开关拨了轮询照跑");
  }
  if (!/AUTO_CHECK_UPDATE_INTERVAL_MS/.test(app)) {
    throw new Error("App.tsx 的轮询没用 AUTO_CHECK_UPDATE_INTERVAL_MS（间隔会与设置说明漂移）");
  }
  if (!/if \(!autoCheck\)/.test(app)) {
    throw new Error("App.tsx 没有在偏好为 false 时短路 —— 关掉后仍在自动查");
  }
  /* ★ 隐藏窗口里 `setInterval` 会被 WebView 节流（最小化 / 收托盘 / 被盖住），
     用户回到应用时看到的可能还是几十分钟前的结论。所以「重新可见」必须补一次。
     变异：删掉这两个监听 ⇒ 本条红；把它们挪到 `if (!autoCheck) return` 之前
     ⇒ 「关掉后仍在自动查」那条红。 */
  if (!/document\.addEventListener\("visibilitychange"/.test(app)) {
    throw new Error("没有监听 visibilitychange —— 最小化 / 托盘恢复后要干等下一个 30 分钟");
  }
  if (!/window\.addEventListener\("focus"/.test(app)) {
    throw new Error("没有监听 focus —— 窗口一直可见但用户刚切回来时不会补查");
  }
  if (!/removeEventListener\("visibilitychange"/.test(app)) {
    throw new Error("visibilitychange 监听没有在 cleanup 里移除（effect 重跑会累积监听）");
  }
  if (!/document\.visibilityState === "visible"/.test(app)) {
    throw new Error("visibilitychange 没有判 visibilityState —— 切走（hidden）时也会发请求");
  }
  /* 补查**不得**传 force：焦点切换是高频事件，强制刷新会变成 GitHub 请求风暴。
     节流交给后端缓存（`update.rs::CACHE_TTL_SECS`，5 分钟）。 */
  if (/checkUpdate\([^)]*true/.test(app)) {
    throw new Error("App.tsx 的自动检查传了 force —— 每次 alt-tab 都会真发请求");
  }
  const pref = readCode("src/lib/update-preference.ts");
  if (!/AUTO_CHECK_UPDATE_INTERVAL_MS\s*=\s*30 \* 60 \* 1000/.test(pref)) {
    throw new Error("间隔常量不是 30 分钟 —— 与开关说明「每 30 分钟检查一次」不符");
  }
  return "开关 ↔ 轮询同源 · 关掉即短路 · 间隔 30 分钟 · 可见/聚焦补查（不传 force）";
});

/* ══════════════════════ 聚合账号面板（issue #50）══════════════════════
 *
 * 聚合页横跨 6 个作用域，是**唯一**一处「同一屏出现多个 scope」的界面。
 * 下面锁住它最容易做歪的五件事：
 *   ① 「聚合」不是第 7 个作用域（`Scope` 是封闭字面量，塞进去三处门禁一起红）；
 *   ② 它**没有侧栏**（那 6 个目的地全是单作用域概念）；
 *   ③ 卡片是**同一张三张卡**（不新建第四种，卡内不加控件）；
 *   ④ 账号键**必须带作用域**（裸主键跨区域会串号）；
 *   ⑤ 筛选偏好是**纯函数 + 规范化**（选满 ⇒ 删键，否则新增账号永远看不见）。
 */

check("★ 聚合不是第 7 个作用域：SCOPES 仍是 6 项且不含 aggregate", () => {
  const scopeSrc = readCode("src/lib/scope.ts");
  const block = scopeSrc.match(/export const SCOPES[^=]*=\s*\[([\s\S]*?)\]/);
  if (!block) throw new Error("找不到 SCOPES 字面量（断言失效：是否改名？）");
  const items = [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  if (items.length !== 6) throw new Error(`SCOPES 是 ${items.length} 项，期望 6 项`);
  if (items.includes("aggregate")) {
    throw new Error("aggregate 被塞进了 SCOPES —— 会同时破坏类型、注册表一致性与快捷键序号");
  }
  /* `Scope` 必须是 `${平台}-${区域}` 的模板字面量：出现一个非此形状的成员就说明有人在放宽它。 */
  if (!/export type Scope = `\$\{PlatformSlug\}-\$\{Region\}`/.test(scopeSrc)) {
    throw new Error("Scope 不再是封闭的 `${PlatformSlug}-${Region}` 模板字面量");
  }
  return "6 项 · 无 aggregate · 类型仍封闭";
});

check("★ 聚合走独立路由 /aggregate，且**不在** SCOPED_ROUTES 里", () => {
  const app = readCode("src/App.tsx");
  if (!/path="\/aggregate"/.test(app)) throw new Error("App.tsx 里没有 /aggregate 路由");
  const block = app.match(/const SCOPED_ROUTES[\s\S]*?\n\};/);
  if (!block) throw new Error("找不到 SCOPED_ROUTES");
  if (/Aggregate/i.test(block[0])) {
    throw new Error("聚合被塞进了 SCOPED_ROUTES —— 它不是 dest（dest 是单作用域内的页面）");
  }
  return "/aggregate 独立路由 · SCOPED_ROUTES 未受污染";
});

check("★ 聚合页没有左侧导航栏：Layout 在裸壳路由下**不渲染** aside", () => {
  const app = readCode("src/App.tsx");
  if (!/useMatch\("\/aggregate"\)/.test(app)) throw new Error("Layout 没有识别裸壳路由");
  if (!/\{bareShell \? null : \(\s*<aside/.test(app)) {
    throw new Error("aside 没有按裸壳路由条件渲染 —— 聚合页会带着 6 个单作用域入口");
  }
  return "bareShell ⇒ 无 aside";
});

check("★★ 聚合页复用三张卡，**不新建第四种**（卡内也不加控件）", () => {
  const page = readCode("src/pages/AggregateAccountsPage.tsx");
  for (const card of ["AccountCard", "TraeAccountCard", "QoderAccountCard"]) {
    if (!new RegExp(`\\b${card}\\b`).test(page)) throw new Error(`聚合页没有用 ${card}`);
  }
  /* 自己定义一个 `function XxxCard(` 就是「第四种卡」——那会让三处卡片的样式与动作开始分叉。 */
  if (/function \w*Card\s*\(/.test(page)) {
    throw new Error("聚合页自己定义了卡片组件 —— 卡片必须复用账号页那三张");
  }
  /* 页面内不得出现作用域选择器：切作用域的**唯一**入口是顶栏（与 `:991` 同一条纪律）。 */
  if (/ScopeSelector/.test(page)) {
    throw new Error("聚合页里出现了 ScopeSelector —— 页面内不得有第二个作用域入口");
  }
  return "三张卡按平台分派 · 无自建卡 · 无 ScopeSelector";
});

check("★★ 账号键带作用域（裸主键在 6 个作用域共存时必然串号）", () => {
  const lib = readCode("src/lib/aggregate-accounts.ts");
  if (!/export function aggregateRef\(scope: Scope, key: string\)/.test(lib)) {
    throw new Error("aggregate-accounts.ts 没有 aggregateRef(scope, key)");
  }
  if (!/return `\$\{scope\}:\$\{key\}`/.test(lib)) {
    throw new Error("aggregateRef 没有拼出 `${scope}:${key}` —— 键里必须带作用域");
  }
  if (!/export function parseAggregateRef/.test(lib)) throw new Error("没有 parseAggregateRef");
  /* 解析必须 fail-closed：来路不明的 scope 不能冒充合法作用域。 */
  if (!/SCOPES as readonly string\[\]\)\.includes\(scope\)/.test(lib)) {
    throw new Error("parseAggregateRef 没有校验 scope 在 SCOPES 里（不是 fail-closed）");
  }
  return "`${scope}:${key}` · 解析 fail-closed";
});

check("★★ 筛选偏好：读写纯函数接收 storage + 选满规范化为「无筛选」", () => {
  const pref = readCode("src/lib/aggregate-visibility-pref.ts");
  if (!/readAggregateVisible\(\s*storage: Pick<Storage, "getItem">/.test(pref)) {
    throw new Error("readAggregateVisible 没有接收 storage 入参（纯函数才能被门禁跑）");
  }
  if (!/writeAggregateVisible\(\s*storage: Pick<Storage, "setItem" \| "removeItem">/.test(pref)) {
    throw new Error("writeAggregateVisible 没有接收 storage 入参");
  }
  /* 读失败一律回退 `null`（= 无筛选）：偏好坏了是小事，白屏是大事。 */
  if (!/catch \{\s*return null;/.test(pref)) {
    throw new Error("读取没有 try/catch 回退 null —— 损坏的 JSON 会让聚合页打不开");
  }
  /* ★ 规范化：选满 == 无筛选。少了这条，第 N+1 个账号入库后**永远看不见**，
     而界面还写着「显示全部」。 */
  const lib = readCode("src/lib/aggregate-accounts.ts");
  if (!/kept\.size === live\.size\) return null/.test(lib)) {
    throw new Error("pruneVisibleKeys 没有把「选满」规范化成 null");
  }
  if (!/visible\.size >= universe\.length\) return null/.test(lib)) {
    throw new Error("visibleKeysToStore 没有把「选满」规范化成 null（写盘侧缺一半）");
  }
  return "纯函数 · 异常回退 null · 选满即删键（读写两侧各一条）";
});

check("★ 聚合页的「需处理」按平台分开判（一个字段套三处必有一处错）", () => {
  const lib = readCode("src/lib/aggregate-accounts.ts");
  if (!/a\.needsRelogin/.test(lib)) throw new Error("WorkBuddy 的需处理没看 needsRelogin");
  if (!/jwtStatus/.test(lib)) throw new Error("Trae 的需处理没看 jwtStatus");
  if (!/hasCredential/.test(lib)) throw new Error("Qoder 的需处理没看 hasCredential");
  return "WB needsRelogin · Trae jwtStatus · Qoder hasCredential";
});

check("★ 聚合详情抽屉的装配按平台分开（一个字段套三处必有一处错）", () => {
  const detail = readCode("src/lib/aggregate-detail.ts");
  for (const marker of ['account.platform === "workbuddy"', 'account.platform === "trae"', "} else {"]) {
    if (!detail.includes(marker)) throw new Error(`详情装配缺平台分支：${marker}`);
  }
  /* 三个平台的凭据字段**不同名**：WB `needsRelogin`、Trae `jwtStatus`、Qoder `hasCredential`。 */
  if (!/jwtStatus/.test(detail)) throw new Error("Trae 的凭据区块没看 jwtStatus");
  if (!/hasCredential/.test(detail)) throw new Error("Qoder 的凭据区块没看 hasCredential");
  /* 「需处理」必须复用同一个判定，不许在抽屉里另写一套（否则卡片说需重登、抽屉说正常）。 */
  if (!/aggregateNeedsAttention\(account\)/.test(detail)) {
    throw new Error("详情抽屉没有复用 aggregateNeedsAttention —— 两处口径会分叉");
  }  /* 程序位徽标：只能给**能判的那一条**（WorkBuddy 主位），其余不许画「未绑定」的假徽标。 */
  const programs = detail.match(/export function aggregateDetailPrograms[\s\S]*?\n\}/);
  if (!programs) throw new Error("找不到 aggregateDetailPrograms");
  /* ★ 2026-10-26（issue #89 · 4.4）：原先这里硬要求「只给 WorkBuddy」。
     那条断言在当时是对的（聚合页确实没取 Trae / Qoder 的程序位），但信号层后来取了
     ⇒ 断言把**过期的取舍**钉成了永久契约，代码于是永远画不出 Trae / Qoder 的徽标。
     现在改为「必须按平台分派」：三个平台都要有自己的分支，不许一刀切 return []。 */
  if (/account\.platform !== "workbuddy"\) return \[\];/.test(programs[0])) {
    throw new Error(
      "aggregateDetailPrograms 又退回「只给 WorkBuddy」—— 信号层已有 Trae / Qoder 的程序位",
    );
  }
  for (const marker of ['account.platform === "trae"', 'account.platform === "qoder"']) {
    if (!programs[0].includes(marker)) {
      throw new Error(`aggregateDetailPrograms 缺平台分支：${marker}`);
    }
  }
  if (!/current: Boolean\(ctx\.currentProgram\)/.test(programs[0])) {
    throw new Error("aggregateDetailPrograms 没有用 currentProgram 判 WorkBuddy 的「当前登录」");
  }
  /* 页面必须真的把信号层的程序位传进去，否则函数改好了也画不出来（这正是 4.4 的成因）。 */
  const page = readCode("src/pages/AggregateAccountsPage.tsx");
  for (const field of ["traePrograms:", "qoderPrograms:"]) {
    if (!page.includes(field)) {
      throw new Error(`聚合页没有把 ${field} 传给详情上下文 —— 抽屉里那两个平台没有程序位徽标`);
    }
  }
  return "三平台分开 · 复用同一判定 · 程序位按平台分派且页面已接线";
});

check("★★ 顶栏「聚合」入口的账号数与聚合页**同源**（两个数据源必然对不上）", () => {
  /* 曾出现过「顶栏 13 个 / 聚合页 29 个」：顶栏数的是 `useScopeStatus`（WB 侧只读 store
     的**当前区域切片**，没进过国际版账号页时国际版是 0），聚合页数的是自己拉的 6 个作用域。
     ⇒ 顶栏必须改读**同一份快照**（`useAggregateAccountTotal`）。 */
  const selector = readCode("src/components/shell/scope-selector.tsx");
  if (!/useAggregateAccountTotal\(/.test(selector)) {
    throw new Error("顶栏没有用 useAggregateAccountTotal —— 聚合计数与聚合页不同源");
  }
  if (/statuses\[item\]\?\.accountCount/.test(selector)) {
    throw new Error("顶栏又改回用 statuses 求和 —— 与聚合页的数字必然对不上");
  }
  const hook = readCode("src/lib/use-aggregate-accounts.ts");
  if (!/export function useAggregateAccountTotal/.test(hook)) {
    throw new Error("找不到 useAggregateAccountTotal");
  }
  /* 同一个资源键 ⇒ 同一份快照、同一个 in-flight 请求。 */
  if (!/AGGREGATE_RESOURCE_KEY/.test(hook)) {
    throw new Error("useAggregateAccountTotal 没有用聚合的资源键 —— 读的不是同一份快照");
  }
  return "顶栏与聚合页读同一份快照";
});

check("★★ 聚合页的写动作必须落到**真实命令**（不许摆假按钮）", () => {
  const page = readCode("src/pages/AggregateAccountsPage.tsx");
  /* 四项刷新：积分 / 今日 Token / 签到 / 凭据 —— 三平台各有各的命令。 */
  for (const cmd of [
    "getAccountTokenToday",
    "getCheckinStatusMap",
    "traeRefreshCredits",
    "getTraeAccountTokenToday",
    "getTraeCheckinStatusMap",
    "qoderCredits",
    "qoderCheckinStatusMap",
  ]) {
    if (!page.includes(cmd)) throw new Error(`聚合页没有调用 ${cmd} —— 那四项刷新/签到是假按钮`);
  }
  /* 批量动作：签到与删除。 */
  if (!/batchCheckin/.test(page)) throw new Error("聚合页没有批量签到");
  if (!/batchDelete/.test(page)) throw new Error("聚合页没有批量删除");
  /* 批量删除必须带撤销（后端有 `account_restore` ⇒ 不给撤销就是「假承诺」）。 */
  if (!/showUndoToast\(/.test(page)) throw new Error("批量删除没有撤销 —— 后端支持恢复却不给撤销");
  if (!/accountRestore\(/.test(page)) throw new Error("批量删除的撤销没有调用 account_restore");
  /* 切换：三个平台各一条。
     ★ 命令可能落在**共享层**（`components/aggregate/aggregate-*-switcher.tsx`）——
     「确认框 + busy + 成功/失败反馈」必须收在一处，否则就是 issue #89 第五节
     指出的成因 2（内联箭头 ⇒ 每个调用点各写一遍）。因此把页面与两个切换器
     合并成一份源码再查：命令不许凭空消失，但也不强求写在页面里。 */
  const writePath = [
    page,
    readCode("src/components/aggregate/aggregate-trae-switcher.tsx"),
    readCode("src/components/aggregate/aggregate-qoder-switcher.tsx"),
  ].join("\n");
  for (const cmd of ["traeSwitchAccount", "qoderSwitchAccount"]) {
    if (!writePath.includes(cmd)) throw new Error(`聚合页（含共享切换器）没有接 ${cmd}`);
  }
  /* ★ 收进共享层的代价是「确认框可能被顺手删掉而没人发现」⇒ 这里钉住它：
     Trae 必须走 `RestartConfirmDialog`（要关客户端），Qoder 必须是确认 Dialog。
     两者都必须由切换器的 `confirmProps` 驱动，否则就是「有框但不生效」。 */
  if (!/RestartConfirmDialog \{\.\.\.traeSwitch\.confirmProps\}/.test(page)) {
    throw new Error("聚合页的 Trae 切换没有渲染重启确认框 —— 点一下就会静默重启客户端");
  }
  if (!/open=\{qoderSwitch\.confirmProps\.open\}/.test(page)) {
    throw new Error("聚合页的 Qoder 切换没有渲染确认框 —— 点一下就直接改写客户端登录态");
  }
  /* 删除必须先确认（单个与批量共用同一个框）。 */
  if (!/open=\{confirmDelete !== null\}/.test(page)) {
    throw new Error("聚合页的删除没有确认框 —— 与三个渠道页的交互契约相反");
  }
  return "四项刷新 · 批量签到/删除（带撤销）· 三平台切换（确认框已钉住）";
});

check("★★ 聚合页与渠道页必须传同一组卡片 props（少传只会静默降级）", () => {
  /* issue #89 的成因 1：「复用组件」被当成了「行为一致」的证明。
     三张卡都是「缺回调 ⇒ 控件禁用或整块消失」的设计 ⇒ **每一个 prop 都是一处
     独立的一致性风险**，而此前没有任何断言覆盖「调用点传齐了没有」。

     这份清单只收**两侧都必须有**的项（渠道页有、聚合页漏过，且漏了会静默降级）。
     「可选但降级」的项（如 `onExtSessions`）刻意不收 —— 把它们钉死会让合理的
     取舍无法表达。

     ⚠️ 检查方式是「文件里出现了 `prop=` 这个 JSX 属性」：够用且稳定。
     抽屉 `actions={{ onDelete: … }}` 用的是冒号，不会误判成卡片属性。 */
  const aggregate = readCode("src/pages/AggregateAccountsPage.tsx");
  const REQUIRED = {
    AccountCard: [
      "onDelete",
      "onSwitch",
      "onCheckin",
      "onExport",
      "onGuideToSetting",
      "onGuidePlugin",
      "onAddPlaintextAccount",
      "onRefreshPart",
      "refreshingKinds",
      "travelStatus",
      "creditPriority",
      "workbuddyActive",
      "todayCheckedIn",
    ],
    TraeAccountCard: [
      "onDelete",
      "programs",
      "current",
      "busy",
      "switchBusy",
      "onSwitchTo",
      "onCheckin",
      "onRefreshCredits",
      "onRefreshPart",
      "refreshingKinds",
      "checkedTodayOverride",
    ],
    QoderAccountCard: [
      "onDelete",
      "programs",
      "onSwitch",
      "switchBusy",
      "onCheckin",
      "onExport",
      "onRefreshPart",
      "refreshingKinds",
      "refreshKinds",
    ],
  };
  const missing = [];
  for (const [card, props] of Object.entries(REQUIRED)) {
    for (const prop of props) {
      /* `prop={` 或 `prop="` 都算传了；`prop={` 覆盖绝大多数（回调 / 布尔表达式）。 */
      if (!aggregate.includes(`${prop}=`)) missing.push(`${card}.${prop}`);
    }
  }
  if (missing.length > 0) {
    throw new Error(
      `聚合页漏传这些卡片 props（会静默降级成灰按钮 / 整块消失）：${missing.join(" · ")}`,
    );
  }
  /* 抽屉同理：`actions` 一个都不传时「操作」区整块消失（4.3 的原症状）。 */
  for (const prop of [
    "actions",
    "checkin",
    "checkinHistory",
    "switchTargets",
    "groups",
    "loading",
    "current",
    "onRefreshPart",
  ]) {
    if (!aggregate.includes(`${prop}=`)) {
      throw new Error(`聚合页的详情抽屉漏传 ${prop} —— 抽屉只能看不能操作`);
    }
  }
  return `三张卡 ${Object.values(REQUIRED).flat().length} 项 + 抽屉 8 项均已接线`;
});

check("★ 新增账号提示：筛选生效期间新出现的账号**要能一键加入**", () => {
  const lib = readCode("src/lib/aggregate-accounts.ts");
  if (!/export function newcomerAccounts/.test(lib)) throw new Error("缺少 newcomerAccounts");
  /* 「加入筛选」是**并入**可见集，不是「清除整个筛选」—— 用户说「加入筛选」
     不是「我不要之前的筛选」。 */
  const page = readCode("src/pages/AggregateAccountsPage.tsx");
  if (!/new Set\(visible \?\? universe\)/.test(page)) {
    throw new Error("「加入筛选」没有并入现有可见集 —— 它把用户的筛选整个换了");
  }
  return "新人可一键并入筛选（保留原有筛选）";
});

check("★ 聚合页的账本与回执**：取数与写动作都不得静默", () => {
  const signals = readCode("src/lib/use-aggregate-signals.ts");
  /* 缺席 ≠ 0：Map 的 get 返回 undefined 即「未知」⇒ 卡片不渲染那一格。 */
  if (!/todayTokens: new Map\(\)/.test(signals)) {
    throw new Error("信号层没有用 Map 表达「缺席」");
  }
  /* Qoder 签到的第三档（通道未开放）必须归入「未知」，不能算成功或失败。 */
  if (!/entry\.active === false/.test(signals)) {
    throw new Error("信号层没有处理 Qoder 的「活动未开放」档");
  }
  const page = readCode("src/pages/AggregateAccountsPage.tsx");
  /* 取数失败要有出口（单组重试），不能整页白屏。 */
  if (!/aggregate\.fail\.retry/.test(page)) throw new Error("聚合页没有「单组重试」出口");
  return "缺席 ≠ 0 · 三档签到 · 失败可单组重试";
});

check("★ 聚合词条 zh/en 键位一一对应，且两个汇总文件都组合了该域", () => {
  const keys = (rel) =>
    [...readCode(rel).matchAll(/"(aggregate\.[^"]+)"/g)].map((m) => m[1]).sort();
  const zhKeys = keys("src/locales/domains/aggregate.zh.ts");
  const enKeys = keys("src/locales/domains/aggregate.en.ts");
  if (zhKeys.length === 0) throw new Error("aggregate.zh.ts 无词条（断言失效：是否改名？）");
  if (JSON.stringify(zhKeys) !== JSON.stringify(enKeys)) {
    const missing = zhKeys.filter((k) => !enKeys.includes(k));
    const extra = enKeys.filter((k) => !zhKeys.includes(k));
    throw new Error(`aggregate 词条 zh/en 不一致：缺 en=${missing.join(",") || "无"} / 多 en=${extra.join(",") || "无"}`);
  }
  if (!/domains\/aggregate\.zh/.test(readCode("src/locales/zh.ts"))) {
    throw new Error("zh.ts 未组合 aggregate 域");
  }
  if (!/domains\/aggregate\.en/.test(readCode("src/locales/en.ts"))) {
    throw new Error("en.ts 未组合 aggregate 域");
  }
  return `${zhKeys.length} 键 zh=en · 两个汇总文件均已组合`;
});

check("★ 限流探测状态：后端枚举 / 前端类型 / zh·en 词条三方**逐字一致**", () => {
  /* 三处必须同步（2026-10-22 踩过：后端加了一档 `upstream_queued`，
     前端类型没加 ⇒ 前端整列落「未知」；词条没加 ⇒ 显示成原始 id）。 */
  const rs = readCode("crates/buddy-switch-gateway/src/model_probe.rs");
  const enumBlock = rs.match(/pub enum ProbeState \{([\s\S]*?)\n\}/);
  if (!enumBlock) throw new Error("找不到 `ProbeState` 枚举（断言失效：是否改名/挪位置？）");
  const backend = [...enumBlock[1].matchAll(/^\s{4}([A-Z][A-Za-z]*),/gm)]
    .map((m) => m[1])
    /* wire 名 = serde `rename_all = "snake_case"`（见 `ProbeState` 的 derive）*/
    .map((name) => name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase());
  if (backend.length === 0) throw new Error("ProbeState 枚举解析出 0 项（断言失效）");

  const ts = readCode("src/lib/types.ts");
  const typeBlock = ts.match(/export type ModelProbeState =([\s\S]*?);/);
  if (!typeBlock) throw new Error("找不到 `ModelProbeState`（前端类型）");
  const frontend = [...typeBlock[1].matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
  if (frontend.length === 0) throw new Error("ModelProbeState 解析出 0 项（断言失效）");

  const missingInFrontend = backend.filter((s) => !frontend.includes(s));
  const extraInFrontend = frontend.filter((s) => !backend.includes(s));
  if (missingInFrontend.length || extraInFrontend.length) {
    throw new Error(
      `后端与前端状态集不一致：前端缺=${missingInFrontend.join(",") || "无"} / 前端多=${extraInFrontend.join(",") || "无"}`,
    );
  }

  /* 词条：每个状态都要有 zh + en 的 label 与 reason（缺一个就显示成原始 id）。 */
  const localeKeys = (rel) =>
    new Set([...readCode(rel).matchAll(/"(shared\.gateway\.probe\.(?:state|reason)\.[^"]+)"/g)].map((m) => m[1]));
  const zh = localeKeys("src/locales/domains/shared.zh.ts");
  const en = localeKeys("src/locales/domains/shared.en.ts");
  for (const state of backend) {
    for (const kind of ["state", "reason"]) {
      const key = `shared.gateway.probe.${kind}.${state}`;
      if (!zh.has(key)) throw new Error(`shared.zh.ts 缺词条 ${key}`);
      if (!en.has(key)) throw new Error(`shared.en.ts 缺词条 ${key}`);
    }
  }
  return `${backend.length} 档 · 后端=前端=zh=en`;
});

check("★ 客户端限流 × 限流探测：两侧读**同一份**实测结论（不是各拼一份）", () => {
  /* 需求（2026-10-22）：「账号里的客户端限流要和网关里的模型限流探测关联起来，数据要同步；
     账号里的客户端限流也可以加入限流探测的入口。」
     判据：两处都读 store 的跨作业累积索引 `useProbeCells(scope)`，且账号页的入口恒 `adopt: false`。
     变异（任一即红）：把任一侧改回读 `probe.cells` 自己拼（单格重测会把矩阵打塌 / 探第二行冲掉第一行）。 */
  const ledger = readCode("src/components/client-limits-card.tsx");
  for (const [marker, why] of [
    ["useProbeCells(", "台账卡没有读共享的实测索引 ⇒ 看不到实测结论，也不跟网关同步"],
    ["probeTargets(", "台账卡没有算实测目标（卡头入口空转）"],
    ["probeOnlyLimits(", "台账卡没有「实测独有」的分流 ⇒ 实测结果无处落脚"],
    ["limitConflict(", "台账卡没有做「台账 vs 实测」一致性判定"],
    ["adopt: false", "★ 账号页的实测必须是「查证」而不是「治理」：不能回填池冷却"],
  ]) {
    if (!ledger.includes(marker)) throw new Error(`client-limits-card.tsx 缺 \`${marker}\`（${why}）`);
  }

  const probeCard = readCode("src/components/gateway/model-probe-card.tsx");
  if (!probeCard.includes("useProbeCells(")) {
    throw new Error("model-probe-card.tsx 没有读共享的实测索引 ⇒ 账号页的实测在这看不到");
  }
  if (/for \(const cell of probe\?\.cells/.test(probeCard)) {
    throw new Error("model-probe-card.tsx 又在自己拼 `probe.cells` ⇒ 单格重测会把整张矩阵打塌");
  }

  /* 第三处消费者：模型表「限流（探测）」列（同一个坑：自己拼 `probe.cells` ⇒ 单格重测后该列只剩 1 格）。 */
  const modelList = readCode("src/components/gateway/model-list.tsx");
  if (!modelList.includes("useProbeCells(")) {
    throw new Error("model-list.tsx 的「限流（探测）」列没有读共享实测索引 ⇒ 与卡片/台账不同源");
  }
  if (/probe\?\.cells/.test(modelList)) {
    throw new Error("model-list.tsx 又在自己拼 `probe.cells` ⇒ 单格重测会让该列突然只剩 1 格");
  }

  const store = readCode("src/stores/gateway.ts");
  for (const [marker, why] of [
    ["mergeProbeCells(", "store 没有合并作业快照 ⇒ 逐行实测的结论会被下一次作业冲掉"],
    ["raw.scope === scope", "★ `probe_status` 不按作用域过滤 ⇒ 不判 scope 会把别的平台的格画到本平台矩阵上"],
  ]) {
    if (!store.includes(marker)) throw new Error(`stores/gateway.ts 缺 \`${marker}\`（${why}）`);
  }

  /* 跨重启：两处都要读**落盘台账**（`probe_ledger`），否则重启后实测结论全丢，
     「数据同步」只剩本次会话。 */
  const limitsCard2 = ledger; // 复用上面的读取（避免重复 IO）
  const cards = {
    "src/components/client-limits-card.tsx": limitsCard2,
    "src/components/gateway/model-probe-card.tsx": probeCard,
  };
  for (const [rel, text] of Object.entries(cards)) {
    if (!text.includes("loadProbeLedger(")) {
      throw new Error(`${rel} 没有读落盘实测台账（重启后看不到上次实测结论）`);
    }
  }
  if (!store.includes("api.probeLedger(")) {
    throw new Error("stores/gateway.ts 的 `loadProbeLedger` 没有调 `api.probeLedger`（台账入口空转）");
  }
  const api = readCode("src/lib/api.ts");
  if (!api.includes("cells: params.cells")) {
    throw new Error("api.ts 的 `probeStart` 没有把 `cells`（显式格清单）发出去 ⇒ 账号页会退化成笛卡尔积");
  }
  if (!api.includes('probe_ledger: { method: "GET"')) {
    throw new Error("api.ts 的 ROUTES 缺 `probe_ledger`（webui 通道 404）");
  }

  const lib = readCode("src/lib/limit-link.ts");
  const limitStates = lib.match(/export const LIMIT_STATES[\s\S]*?\];/);
  if (!limitStates) throw new Error("limit-link.ts 找不到 `LIMIT_STATES`（断言失效：是否改名？）");
  if (/no_credits|auth_dead/.test(limitStates[0])) {
    throw new Error("★ `LIMIT_STATES` 里混进了 `no_credits` / `auth_dead` ⇒ 402 会被当成限流（用户去傻等解锁）");
  }
  return "台账卡/网关卡同源（useProbeCells）· 入口 adopt:false · store 按 scope 收窄并合并 · 三档限流";
});

console.log(`\n${passed} passed / ${failures.length} failed\n`);
if (failures.length) {
  for (const f of failures) console.error(" - " + f);
  process.exit(1);
}
