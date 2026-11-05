#!/usr/bin/env node
/**
 * `src/lib/credit-packs.ts` 的**真实断言**（issue #144 复核 §5「门禁从文本断言升级为行为断言」）。
 *
 * ## 为什么不用文本断言
 *
 * 旧门禁对聚合详情只做字符串匹配（`check-scope-registry.cjs:9164`「文件里有没有某个
 * `account.platform === ...`」）。它能证明「三个分支都在」，**证明不了**「到期时间没被
 * 当成毫秒」—— 而 #144 的回归正是这样漏过去的：代码结构完全正确，只有量纲错了。
 *
 * 所以这里照抄 `check-key-usage.mjs` 的做法：用 esbuild 把**生产代码本身**转译成 ESM、
 * 经 `data:` URL 导入，跑真断言。不复制逻辑（复制的那份必然与生产漂移）。
 *
 * ## 钉住四条
 *
 * 1. **秒 → 毫秒**：`secondsToMs` 之后必须等于 2026-11-21 02:40（+08:00），而不是 1970
 *    （#144 的指纹）。注意断言的是**固定 fixture 的换算结果**，不读系统时钟 —— 与运行日历无关；
 * 2. **`remaining <= 0` / `total <= 0` 剔除**（#143 / #144 共用的判据）；
 * 3. **排序**：已过期垫底 → 未过期按到期升序 → `null`（永久）垫在未过期的最后；
 * 4. **`ratio` 是 0..100**（抽屉直接拼 `%`，给 0..1 就是 1/100 宽的进度条）。
 *
 * 另加两条「用法」断言（这类必须看源码）：Trae 的秒值只能经 `secondsToMs` 归一，
 * 不许再出现就地 `* 1000`；四个调用方都必须走 `toDetailPacks`。
 *
 * 用法：`node scripts/check-credit-packs.mjs`（已挂进 `npm run check:scope`）。
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function loadTs(relative) {
  const source = readFileSync(path.join(root, relative), "utf8");
  // ★ Windows：绝对路径必须转 `file://` URL，否则 ESM loader 把 `D:` 当成协议。
  const { transform } = await import(
    pathToFileURL(path.join(root, "node_modules/esbuild/lib/main.js")).href
  );
  const { code } = await transform(source, { loader: "ts", format: "esm" });
  return import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
}

const { normalizePacks, toDetailPacks, packIsExpired, packRatio, secondsToMs } = await loadTs(
  "src/lib/credit-packs.ts",
);
/* 临期窗口的镜像常量（#144 复核 §3.3）：与后端 `EXPIRING_SOON_SECS` 必须同值。 */
const { TRAE_EXPIRING_SOON_MS } = await loadTs("src/lib/credit-expiry.ts");

/** 递归列出 `src/**` 的 ts/tsx（用法断言的扫描面）。 */
function walkTs(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const abs = path.join(dir, entry);
    if (statSync(abs).isDirectory()) walkTs(abs, out);
    else if (/\.(ts|tsx)$/.test(entry)) out.push(abs);
  }
  return out;
}

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

/* 判定基准时刻：2026-11-03T00:00:00+08:00。用固定值而不是 `Date.now()`，
   否则「已过期 / 临期」的断言会随日历漂移。 */
const NOW = Date.UTC(2026, 10, 2, 16, 0, 0);
const DAY = 86_400_000;
/** 2026-11-21 02:40 +08:00，即截图里那批「近期到期」的量级（Unix **秒**）。 */
const SEC_2026_11_21 = 1795200000;

const pack = (over = {}) => ({
  key: "k",
  name: "pack",
  total: 100,
  remaining: 100,
  used: 0,
  expireAtMs: NOW + 10 * DAY,
  ...over,
});

check("★ 秒 → 毫秒：Trae 的 Unix 秒必须换算成 2026-11-21 02:40（+08:00），不是 1970（#144 回归）", () => {
  const ms = secondsToMs(SEC_2026_11_21);
  eq(ms, SEC_2026_11_21 * 1000, "secondsToMs 的倍率");
  /* 钉住绝对值：输入是常量、不读 `Date.now()`，所以这条断言与运行年份无关
     （2027 年跑同样是这个时刻，不是「必须在 2026 年」）。 */
  eq(new Date(ms).toISOString(), "2026-11-20T18:40:00.000Z", "换算后的绝对时刻（= 2026-11-21 02:40 +08:00）");
  /* 反证：直接当毫秒解释会落到 1970-01-22 02:40（+08:00，即截图里那串时间的来源）——
     把错误方向也钉住，避免有人「顺手」改回裸值。 */
  eq(new Date(SEC_2026_11_21).toISOString(), "1970-01-21T18:40:00.000Z", "未换算时的时刻（这是错误方向，仅作对照）");
  eq(secondsToMs(null), null, "null 必须原样透传（永久有效 ≠ 0）");
});

check("remaining <= 0 / total <= 0 不列（#143 / #144 共用判据）", () => {
  const rows = normalizePacks(
    [
      pack({ key: "used-up", remaining: 0 }),
      pack({ key: "zero-limit", total: 0, remaining: 0 }),
      pack({ key: "ok", remaining: 5 }),
    ],
    NOW,
  );
  eq(
    rows.map((row) => row.key),
    ["ok"],
    "过滤后的 key",
  );
});

check("排序：已过期垫底 → 到期升序 → null（永久）垫在未过期最后", () => {
  const rows = normalizePacks(
    [
      pack({ key: "expired", expireAtMs: NOW - DAY, expired: true }),
      pack({ key: "forever", expireAtMs: null }),
      pack({ key: "late", expireAtMs: NOW + 30 * DAY }),
      pack({ key: "soon", expireAtMs: NOW + 2 * DAY }),
    ],
    NOW,
  );
  eq(
    rows.map((row) => row.key),
    ["soon", "late", "forever", "expired"],
    "排序结果",
  );
});

check("expired 取「后端标记 或 到期时刻已过」的并集（不只信后端）", () => {
  if (!packIsExpired(pack({ expireAtMs: NOW - 1 }), NOW)) {
    throw new Error("到期时刻已过却没判过期 —— 界面会把过期包标成临期");
  }
  if (!packIsExpired(pack({ expired: true, expireAtMs: NOW + DAY }), NOW)) {
    throw new Error("后端说过期却不认 —— 后端口径与前端显示会打架");
  }
  if (packIsExpired(pack({ expireAtMs: null }), NOW)) {
    throw new Error("永久有效被判成过期");
  }
});

check("expiringSoon 只认后端标记（缺字段 = 未知，不推测窗口）", () => {
  const rows = normalizePacks(
    [
      pack({ key: "no-flag", expiringSoon: undefined }),
      pack({ key: "flagged", expiringSoon: true }),
      pack({ key: "expired-flagged", expiringSoon: true, expired: true }),
    ],
    NOW,
  );
  const byKey = Object.fromEntries(rows.map((row) => [row.key, row.expiringSoon]));
  eq(byKey, { "no-flag": false, flagged: true, "expired-flagged": false }, "临期标记");
});

check("★ ratio 是 0..100 且已夹紧（给 0..1 就是 1/100 宽的进度条）", () => {
  eq(packRatio(86.01, 100), 86.01, "正常值");
  eq(packRatio(200, 100), 100, "超额夹紧到 100");
  eq(packRatio(0, 0), 0, "分母为 0");
  eq(packRatio(-5, 100), 0, "负值夹紧到 0");
  const rows = normalizePacks([pack({ remaining: 86.01, total: 100 })], NOW);
  eq(rows[0].ratio, 86.01, "normalizePacks 里的 ratio");
});

check("toDetailPacks 把**毫秒**交给 expiryText（不是原始秒）", () => {
  const seen = [];
  toDetailPacks(
    [pack({ key: "t", expireAtMs: secondsToMs(SEC_2026_11_21) })],
    {
      remainingText: String,
      totalText: String,
      usedText: String,
      expiryText: (ms, expired) => {
        seen.push({ ms, expired });
        return "x";
      },
    },
    NOW,
  );
  eq(seen, [{ ms: SEC_2026_11_21 * 1000, expired: false }], "expiryText 的入参");
});

/* ── 用法断言：这类只能看源码（「谁在调」不是纯函数行为）────────────────── */

check("Trae 的秒值只经 secondsToMs 归一，不许再就地 `* 1000`", () => {
  for (const file of ["src/lib/aggregate-detail.ts", "src/pages/TraeAccountsPage.tsx"]) {
    const code = readFileSync(path.join(root, file), "utf8");
    if (!code.includes("secondsToMs(")) {
      throw new Error(`${file} 没有用 secondsToMs 归一 Trae 的秒口径`);
    }
    if (/expireAt\s*\*\s*1000/.test(code)) {
      throw new Error(`${file} 里还有就地 \`expireAt * 1000\` —— 下一次漏乘就是再一个 1970`);
    }
  }
});

check("四个装配点都走共用 toDetailPacks（不许再各写一份过滤/排序/比例）", () => {
  for (const file of [
    "src/pages/AccountsPage.tsx",
    "src/pages/TraeAccountsPage.tsx",
    "src/pages/QoderAccountsPage.tsx",
    "src/lib/aggregate-detail.ts",
  ]) {
    const code = readFileSync(path.join(root, file), "utf8");
    if (!code.includes("toDetailPacks(")) {
      throw new Error(`${file} 没有走 toDetailPacks —— 又回到了四份实现`);
    }
  }
});

/* ── 「打上游」的自动刷新必须 opt-in（issue #133 复核 §3）────────────────
 *
 * 这条只能看源码：它是「谁在调、传了什么」而不是纯函数行为。失败模式很具体 ——
 * 新增一个打上游的视图时忘了接开关，于是它默认就开始每 10 分钟花一次上游成本，
 * 而界面文案还写着「自动刷新已关闭」。
 */
check("★ 打上游的三处自动刷新都接到 opt-in 开关（新增视图不许漏接）", () => {
  const upstreamViews = [
    "src/pages/CreditStatsPage.tsx",
    "src/pages/QoderCreditsPage.tsx",
    "src/pages/QoderAccountsPage.tsx",
  ];
  for (const file of upstreamViews) {
    const code = readFileSync(path.join(root, file), "utf8");
    if (!code.includes("useCreditRefreshPref(")) {
      throw new Error(`${file} 会打上游却没有读 opt-in 开关 —— 默认就开始花钱`);
    }
    if (!/useAutoRefreshWhenVisible\([\s\S]{0,400}?upstreamAutoRefresh,\s*\)/.test(code)) {
      throw new Error(`${file} 读了开关却没把它传给 useAutoRefreshWhenVisible 的第 4 参`);
    }
  }
  /* 只读本地快照的两页**不该**受这个开关约束（零上游成本，关掉只损失新鲜度）。 */
  for (const file of ["src/pages/TraeCreditsPage.tsx", "src/pages/TraeAccountsPage.tsx"]) {
    const code = readFileSync(path.join(root, file), "utf8");
    if (code.includes("useCreditRefreshPref(")) {
      throw new Error(`${file} 只读本地快照，却被 opt-in 开关拦住 —— 关掉后它不会自己更新`);
    }
  }
  /* 全局的余额轮询同样打上游 ⇒ 必须读同一个 store（不是自己存一份 state）。 */
  const hook = readFileSync(path.join(root, "src/lib/use-credit-auto-refresh.ts"), "utf8");
  if (!hook.includes("useCreditRefreshPref(")) {
    throw new Error("useCreditAutoRefresh 没有读 opt-in 开关 —— 卡片余额仍在自动打上游");
  }
});

/* ── 临期窗口：只认后端标记，缺标记时用**镜像常量**（#144 复核 §3.3）────────
 *
 * 这条失败模式很隐蔽：同一个包在卡片上说「临期」、点开抽屉却不说 —— 因为两处用了
 * 不同窗口（前端写死 7 天 vs 后端 14 天）。#144 复核当时数出五处实现，其中三处是
 * 前端自设窗口；现在收敛成「后端标记 + 一个镜像常量」。
 */
check("★ 临期窗口只剩两档：后端标记，或镜像后端的 14 天常量（不许再写死 7 天）", () => {
  eq(TRAE_EXPIRING_SOON_MS, 14 * DAY, "TRAE_EXPIRING_SOON_MS 必须镜像后端 EXPIRING_SOON_SECS");
  const offenders = [];
  for (const file of walkTs(path.join(root, "src"))) {
    const code = readFileSync(file, "utf8")
      .split("\n")
      .filter((line) => !/^\s*(\*|\/\/|\/\*)/.test(line))
      .join("\n");
    /* 只抓「7 天窗口」这一种写法：`7 * 86_400_000` / `7 * 86400000` / `7 * 24 * 3600`。 */
    if (/\b7\s*\*\s*(?:86_?400_?000|24\s*\*\s*3600)/.test(code)) {
      offenders.push(path.relative(root, file));
    }
  }
  if (offenders.length > 0) {
    throw new Error(
      `这些文件仍在前端自设 7 天临期窗口（应用后端标记或 TRAE_EXPIRING_SOON_MS）：${offenders.join("、")}`,
    );
  }
});

if (failed > 0) {
  console.error(`\ncredit-packs 门禁：${passed} 通过 / ${failed} 失败`);
  process.exit(1);
}
console.log(
  `credit-packs 门禁：${passed} 项通过（秒→毫秒 · 过滤 · 排序 · ratio · 临期标记 · 调用点 · opt-in 接线 · 临期窗口）`,
);
