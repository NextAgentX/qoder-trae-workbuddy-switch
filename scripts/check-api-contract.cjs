#!/usr/bin/env node
"use strict";

// API 契约 / 演示数据一致性护栏（零依赖，仅用 Node 标准库）。
//
// 本项目的双通道适配层有若干处「纯字符串」必须逐字对齐，且全部没有编译期保护：
//   ① src/lib/api.ts                       —— call("<cmd>") 的 cmd 字符串
//   ② src/lib/api.ts                       —— ROUTES 表的键名与 { method, path }
//   ③ crates/buddy-switch-server/src/api.rs   —— 路由表的 path + method（跨语言，编译器无法互校）
//   ④ src-tauri/src/lib.rs                 —— invoke_handler 登记的命令名
//   ⑤ src/lib/screenshot-demo.ts           —— screenshotDemoResponse 的 switch case 分支
//                                             （须覆盖 DEMO_READ_COMMANDS 的每个只读命令）
//
// 任一处漂移，症状都是「构建全绿，但某端静默失效」（桌面端 command not found /
// webui 抛「暂不支持」/ method 写反打到错的 handler / 演示模式运行时报缺只读数据）。
// 本脚本在构建前做这些一致性校验，发现差异立即以非零退出码失败，并打印具体差异。
//
// 有意不做的一项：webui POST body 形状（`{ config: … }` vs 扁平）。后端对二者都接受
// （`body.get("config").unwrap_or(&body)`），且 body 形状在源码里没有机器可读的声明，
// 无法用稳健的静态规则校验——强行用脆弱正则只会降低整条护栏的可信度，故不做。
//
//   ⑥ src-tauri/src/commands.rs  —— 桌面端命令的**参数名**（见第 7 条校验）
//
// 第 7 条只覆盖一个**可机检**的子集：签名里除 Tauri 注入参数外**只剩一个 `Value` 参数**
// 的命令。Tauri 按参数名从 invoke 载荷里取值，前端平铺传参就会报
// `missing required key <参数名>`（真踩过：`trae_switch_account` 平铺传 `userId/launch/…`，
// 报 `missing required key options`，而 webui 通道因为 `body.get("options").unwrap_or(body)`
// 能容忍平铺 —— 于是「浏览器里好用、桌面端点不动」）。这类命令的参数名与形状是**单一、
// 机器可读**的，因此可以硬校验；其余多参数命令仍不做（那才需要脆弱的正则）。
//
// 用法：node scripts/check-api-contract.cjs   （npm run check:api）

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const API_TS = path.join(ROOT, "src", "lib", "api.ts");
const SCREENSHOT_DEMO_TS = path.join(ROOT, "src", "lib", "screenshot-demo.ts");
const SERVER_RS = path.join(ROOT, "crates", "buddy-switch-server", "src", "api.rs");
const TAURI_LIB_RS = path.join(ROOT, "src-tauri", "src", "lib.rs");
const TAURI_COMMANDS_RS = path.join(ROOT, "src-tauri", "src", "commands.rs");
// 作用域解析的**唯一事实来源**（core）—— 第 9 条反例扫描的对象。
const CORE_PRODUCT_RS = path.join(
  ROOT,
  "crates",
  "buddy-switch-core",
  "src",
  "modules",
  "product.rs",
);

// 第 7 条校验的豁免表。留空是**有意**的：目前所有「单 Value 参数」命令都能用对象
// 字面量调用，豁免项一旦出现，说明出现了需要人工判断的形态，应在评审时说明理由。
const SINGLE_VALUE_SHAPE_EXEMPT = new Set([]);

// webui 未提供、仅桌面端可用的命令。它们「有意」没有 ROUTES 路由条目——豁免的依据是
// 「无 ROUTES 路由」，而不是「没守卫」：这些命令的 wrapper **内部**都自带
// `demoModeEnabled` / `isWebui()` 早退守卫（见 api.ts 的 openPermissionSettings /
// checkAuthPermission / revealAppInFinder / relaunchApp / get|setLaunchAtLoginEnabled），
// 因此「webui 不可达」由函数自己保证，不依赖调用方自律。
// 除此之外的任何 call("<cmd>") 都必须有 ROUTES 条目、且必须已在桌面端登记。
const ROUTE_EXEMPT_COMMANDS = new Set([
  "open_permission_settings",
  "check_auth_permission",
  "reveal_app_in_finder",
  "relaunch_app",
  "get_launch_at_login_enabled",
  "set_launch_at_login_enabled",
]);

/**
 * 第 8 条：「参数化命令」清单（`*_for(scope, …)` 风格，与 `docs/redesign/P5-ARCH.md` §2.3
 * 命令表**同源**）。对集合内每个命令断言「四处都带了 scope」：
 * ① `commands.rs` 该 fn 签名含 `scope`；② `api.ts` 的 `call("<cmd>", {…})` 顶层键含 `scope`；
 * ③ `api.rs` 的 handler 从 query/body 读到 `scope`。
 *
 * ## 为什么要有它（防的是「参数化只做一半」）
 *
 * 命令签名带了 `scope`、但前端没传 / 服务端没读 ⇒ 桌面端与 webui **行为分叉**，
 * 而**构建全绿**（两端各自都能编译）。这是最难在评审里发现的一类漂移。
 *
 * ⚠️ **本集合在 T03（命令面参数化）之前是空集**：门禁规则先就位，等 T03 填命令名。
 * 刻意**不**为了「让规则有事可做」去提前参数化任何命令 —— 那是 T03 的范围。
 */
const SCOPE_PARAMETERIZED = new Set([
  // T03 在此登记参数化命令名，例："accounts_list", "account_checkin", …
  /* ✅ 2026-10-04：以下三条已参数化 —— 前端 `api.ts` 改调并显式传 `scope`，
     后端 `commands.rs` 签名含 `scope` 参数。
     ⚠️ **登记即激活第 8 条**：此后任一侧漏改（前端不传 scope / 后端签名没有 scope）都会红。
     这正是要防的「参数化只做一半」—— 桌面端与 webui 会分叉，而构建全绿。 */
  "accounts_list",
  "account_delete",
  "account_remark",
  /* ✅ 2026-10-04（续）：账号导出 / 导入家族 —— **区域纯**（只按区域读写账号库，不触碰
     客户端 / 程序位）且两平台**同构**（WB `export_accounts_for(region, ids)` vs Trae
     `export_accounts_for(variant, ids)`），故可安全折叠到 `scope`。
     ⚠️ 名字**不**沿用 §2.3 的 `export_accounts` / `export_accounts_to_path` / `import_accounts`
     —— 那三个与 **WorkBuddy 旧命令同名**，与 §2.4「旧命令保留为别名」冲突（`generate_handler!`
     不允许重名）。旧命令保留为别名（见 `api.ts` ALIASES），参数化命令取不冲突新名。 */
  "accounts_export",
  "accounts_export_to_path",
  "accounts_import",
  /* ✅ 2026-10-04（续）：积分 —— **区域纯**（只按区域读账号库 / 积分缓存，不触碰
     客户端 / 程序位）且两平台**同构**（WB `get_credit_expiry` 单账号 vs Trae
     `get_trae_credits` 区域总览），故可折叠到 `scope`。形状按 `scope.product()` 分叉；
     `accountId` 仅 WB 分支使用（Trae 分支忽略）。旧命令保留为别名（见 api.ts ALIASES）。 */
  "credits",
  /* ✅ 2026-10-04（续）：签到状态 —— **区域纯**（只按区域读账号库 / 签到缓存，不触碰
     客户端 / 程序位）且两平台**同构**（WB `get_checkin_status` 单账号 vs Trae
     `get_trae_checkin_status` 区域摘要），故可折叠到 `scope`。形状按 `scope.product()` 分叉；
     `accountId` 仅 WB 分支使用（Trae 分支忽略）。旧命令保留为别名（见 api.ts ALIASES）。
     ⚠️ 参数化命令只对齐**桌面端单账号**契约：webui 的旧路由 `GET /api/checkin/status` 是
     **批量整端点**（`getCheckinStatusMap` 依赖它），该差异刻意保留。 */
  "checkin_status",
  /* ✅ 2026-10-04（续）：从本机客户端导入账号 —— **区域纯**（读**该区域客户端**登录态、
     写**该区域**账号库；客户端由 `scope` 决定，不用 detected_variant 猜测）且两平台同构
     （WB `import_local` vs Trae `trae_import_local_account`）。名字**不**沿用 §2.3 的
     `import_local` —— 与 **WorkBuddy 旧命令同名**（`generate_handler!` 不允许重名），
     故取不冲突新名 `accounts_import_local`（与 `accounts_import` 同族）。旧命令保留为别名。 */
  "accounts_import_local",
  /* ✅ 2026-10-04（续）：打开客户端数据目录 —— 两平台**同形**（都收 scope、返回 `{ok,path}`
     或结构化 `Unsupported`）。判为区域纯的依据：**只读**客户端数据目录的**路径**并交文件管理器
     打开，**不写**程序位状态（区别于会关/重启进程的 `switch_account`）。旧命令保留为别名。 */
  "open_data_dir",
]);

/**
 * 第 9 条：「scope 解析必须 fail-closed」——**只**扫作用域解析点（core `product.rs`）。
 *
 * ## 为什么只扫这一个文件（不误伤旧 `region` 兼容解析）
 *
 * 旧 `region` 参数（两宿主的 `parse_region` / core `parse_region_filter`）是**合法**的
 * 宽容入口（老前端不带该参数等价于 CN），其 `unwrap_or(Region::Cn)` 语义**保留**。
 * fail-closed 只约束**新的 `scope` 参数路径**。把旧入口一起扫进来会立刻假红，
 * 于是整条护栏被绕过 —— 那比没有护栏更坏。故第 9 条**只**锚在 `Scope` 的唯一解析点上。
 */
const SCOPE_RESOLUTION_FILES = [CORE_PRODUCT_RS];

/** scope 解析点**禁止**出现的 fail-open 回落（命中即红）。 */
const SCOPE_FAIL_OPEN_PATTERNS = [
  /unwrap_or\s*\(\s*Region::Cn\s*\)/,
  /unwrap_or\s*\(\s*Region::[A-Za-z]+\s*\)/,
  /unwrap_or\s*\(\s*Scope::[A-Za-z]+\s*\)/,
  /unwrap_or_default\s*\(\s*\)/,
  /unwrap_or\s*\(\s*DEFAULT_SCOPE\s*\)/,
  /\?\?\s*DEFAULT_SCOPE/,
];

/**
 * 第 10 条：全局探测**不得进任何写路径**（PLAN §2.4 / P5-ARCH §前置 1）。
 *
 * `platform::detected_variant()` / `platform::detect_data_dir()` 是**无入参的全局探测**
 * （挑本机最近活跃的客户端），只适合环境自检与展示。若它们出现在宿主的命令 / handler 里，
 * 一个「按本机活跃客户端推断作用域」的写操作就会把数据写进**错误的库**（别名转发尤其危险：
 * 旧命令必须**硬编码自己的 scope**，不接受「由调用方决定」或「按本机推断」）。
 *
 * 判据只锚**裸调用**（`detected_variant(` / `detect_data_dir(`）—— `detect_data_dir_for(v)`
 * 是**按变体取目录**的写侧来源，语义不同，不在此列。
 */
const HOST_COMMAND_FILES = [TAURI_COMMANDS_RS, SERVER_RS];

/** 读文件并去掉可能存在的 UTF-8 BOM。 */
function read(file) {
  return fs.readFileSync(file, "utf8").replace(/^\uFEFF/, "");
}

/**
 * 去掉行注释与块注释。
 *
 * 必要性（真实踩过）：注释里写 `call("<cmd>")` 这类示例会把扫描器骗过去，
 * 报出「未登记的命令 <cmd>」，而真正的调用点全都没问题——护栏于是从
 * 「保护构建」变成「制造噪声」，最后被开发者绕过。
 * 用 `(^|[^:])` 排除 `://`，避免把 URL 里的 `//` 当成行注释起点。
 */
function stripComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/[^\n]*/g, "$1");
}

/**
 * 从 openIndex（指向 "{"/"("/"["）开始，返回与之配对的括号**内部**文本。
 *
 * 只对同一种括号计数，足以覆盖本项目里的对象字面量与函数实参（不含嵌套异种括号）。
 */
function sliceBalanced(text, openIndex) {
  const open = text[openIndex];
  const close = open === "{" ? "}" : open === "(" ? ")" : open === "[" ? "]" : null;
  if (!close) throw new Error(`sliceBalanced: 起始字符不是括号 (${JSON.stringify(open)})`);
  let depth = 0;
  for (let i = openIndex; i < text.length; i += 1) {
    const ch = text[i];
    if (ch === open) {
      depth += 1;
    } else if (ch === close) {
      depth -= 1;
      if (depth === 0) return text.slice(openIndex + 1, i);
    }
  }
  throw new Error("sliceBalanced: 括号未闭合");
}

/**
 * `call` 调用点的匹配前缀：`call` + **可选的泛型实参** + `(` + 空白。
 *
 * 为什么放行泛型（真实踩过，2026-09-23）：`call<AppStatus>("get_status", …)` 是
 * 完全正常的 TS 写法 —— 返回类型无法从实参推断时（例如后面接
 * `.then(normalizeAppStatus)`）**必须**显式写出来。原来的 `call\s*\(` 不认泛型，
 * 于是这两条调用点直接扫不到，护栏反过来报「ROUTES["get_status"] 是死路由」。
 *
 * 这里的取舍：护栏的职责是「每个 `call` 的命令名都要有路由」，**不是**限制代码
 * 怎么写（检查脚本不得左右代码设计）。放行泛型不会削弱它 —— 只是让本该被扫到的
 * 调用点重新进入扫描范围；`[^(]*?` 顺带兼容 `Record<string, number>` 这类嵌套 `<>`。
 */
const CALL_PREFIX = String.raw`(?<![\w.$])call\s*(?:<[^(]*?>\s*)?\(\s*`;

/** 从 api.ts 的 ROUTES 表提取 { cmd, method, path } 列表。 */
function extractRoutes(apiTs) {
  const anchor = apiTs.indexOf("const ROUTES");
  if (anchor < 0) throw new Error("api.ts 中找不到 `const ROUTES`");
  const openIndex = apiTs.indexOf("{", anchor);
  if (openIndex < 0) throw new Error("api.ts 中找不到 ROUTES 对象字面量");
  const block = sliceBalanced(apiTs, openIndex);
  const re =
    /([A-Za-z0-9_]+)\s*:\s*\{\s*method\s*:\s*"([A-Za-z]+)"\s*,\s*path\s*:\s*"([^"]+)"\s*\}/g;
  const routes = [];
  let m;
  while ((m = re.exec(block)) !== null) {
    routes.push({ cmd: m[1], method: m[2].toUpperCase(), path: m[3] });
  }
  if (routes.length === 0) throw new Error("未能从 ROUTES 解析出任何路由条目");
  return routes;
}

/** 提取 api.ts 中所有裸 call("<cmd>") 的 cmd（排除 httpCall 等与函数定义）。 */
function extractCallCommands(apiTs) {
  // 先剥注释：注释中的示例调用不构成契约（见 stripComments 的说明）。
  const source = stripComments(apiTs);
  const re = new RegExp(`${CALL_PREFIX}"([^"]+)"`, "g");
  const cmds = new Set();
  let m;
  while ((m = re.exec(source)) !== null) cmds.add(m[1]);
  if (cmds.size === 0) throw new Error("未能从 api.ts 解析出任何 call(\"<cmd>\")");
  return cmds;
}

/** 提取 api.ts 中 DEMO_READ_COMMANDS 集合包含的只读命令名。 */
function extractDemoReadCommands(apiTs) {
  const anchor = apiTs.indexOf("DEMO_READ_COMMANDS");
  if (anchor < 0) throw new Error("api.ts 中找不到 DEMO_READ_COMMANDS");
  const openIndex = apiTs.indexOf("[", anchor);
  if (openIndex < 0) throw new Error("DEMO_READ_COMMANDS 不是数组字面量");
  const block = sliceBalanced(apiTs, openIndex);
  const cmds = new Set();
  const re = /"([^"]+)"/g;
  let m;
  while ((m = re.exec(block)) !== null) cmds.add(m[1]);
  if (cmds.size === 0) throw new Error("未能从 DEMO_READ_COMMANDS 解析出任何命令");
  return cmds;
}

/** 提取 screenshot-demo.ts 中 screenshotDemoResponse 的 switch case 分支命令名。 */
function extractDemoCaseCommands(screenshotTs) {
  const anchor = screenshotTs.indexOf("export function screenshotDemoResponse");
  if (anchor < 0) throw new Error("screenshot-demo.ts 中找不到 screenshotDemoResponse");
  const openIndex = screenshotTs.indexOf("{", anchor);
  if (openIndex < 0) throw new Error("screenshotDemoResponse 函数体未找到");
  const body = sliceBalanced(screenshotTs, openIndex);
  const cmds = new Set();
  const re = /case\s+"([^"]+)"/g;
  let m;
  while ((m = re.exec(body)) !== null) cmds.add(m[1]);
  if (cmds.size === 0) throw new Error("screenshotDemoResponse 中解析不到任何 case 分支");
  return cmds;
}

/** 从 server api.rs 提取 path → 支持的 method 集合。 */
function extractServerRoutes(serverRs) {
  const map = new Map();
  const routeRe = /\.route\s*\(/g;
  let m;
  while ((m = routeRe.exec(serverRs)) !== null) {
    const openIndex = m.index + m[0].length - 1;
    const inner = sliceBalanced(serverRs, openIndex);
    const pathMatch = inner.match(/"([^"]+)"/);
    if (!pathMatch) continue;
    const routePath = pathMatch[1];
    const methods = map.get(routePath) || new Set();
    const methodRe = /\b(get|post|put|patch|delete|head|options)\s*\(/g;
    let mm;
    while ((mm = methodRe.exec(inner)) !== null) methods.add(mm[1].toUpperCase());
    map.set(routePath, methods);
  }
  if (map.size === 0) throw new Error("未能从 server api.rs 解析出任何 .route(...)");
  return map;
}

/** 从 src-tauri lib.rs 的 generate_handler![...] 提取登记的命令名。 */
function extractInvokeCommands(libRs) {
  const block = libRs.match(/generate_handler!\s*\[([\s\S]*?)\]/);
  if (!block) throw new Error("lib.rs 中找不到 generate_handler![...]");
  const cmds = new Set();
  const re = /commands::([A-Za-z0-9_]+)/g;
  let m;
  while ((m = re.exec(block[1])) !== null) cmds.add(m[1]);
  if (cmds.size === 0) throw new Error("lib.rs invoke_handler 中解析不到命令名");
  return cmds;
}

/**
 * 按**顶层**逗号切分（跳过字符串、跳过 `{}`/`[]`/`()` 内部）。
 *
 * 用于解析 Rust 形参表与 TS 对象字面量的顶层条目——嵌套里的逗号不能当分隔符。
 */
function splitTopLevel(text) {
  const parts = [];
  let depth = 0;
  let current = "";
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '"' || ch === "'" || ch === "`") {
      const quote = ch;
      current += ch;
      i += 1;
      while (i < text.length) {
        current += text[i];
        if (text[i] === "\\") {
          current += text[i + 1] ?? "";
          i += 2;
          continue;
        }
        if (text[i] === quote) {
          i += 1;
          break;
        }
        i += 1;
      }
      continue;
    }
    if (ch === "{" || ch === "[" || ch === "(") depth += 1;
    if (ch === "}" || ch === "]" || ch === ")") depth -= 1;
    if (ch === "," && depth === 0) {
      parts.push(current);
      current = "";
      i += 1;
      continue;
    }
    current += ch;
    i += 1;
  }
  if (current.trim() !== "") parts.push(current);
  return parts;
}

/** 提取对象字面量内部的**顶层键名**（支持 `{ a: 1 }` 与 `{ a }` 简写）。 */
function topLevelKeys(objectBody) {
  return splitTopLevel(objectBody)
    .map((segment) => {
      const withValue = segment.match(/^\s*([A-Za-z_$][\w$]*)\s*:/);
      if (withValue) return withValue[1];
      const shorthand = segment.match(/^\s*([A-Za-z_$][\w$]*)\s*$/);
      return shorthand ? shorthand[1] : null;
    })
    .filter((key) => key !== null);
}

/** 把 snake_case 参数名转成 camelCase（`#[tauri::command(rename_all = "camelCase")]` 用）。 */
function toCamelCase(name) {
  return name.replace(/_([a-z0-9])/g, (_, ch) => ch.toUpperCase());
}

/**
 * 从 commands.rs 提取「除注入参数外只剩一个 `Value` 参数」的命令 → 期望的载荷键名。
 *
 * 返回 `Map<命令名, 期望键名>`。Tauri 注入的参数（`AppHandle` / `State` / `Window` …）
 * 由框架填充，不来自前端，必须先排除，否则每个带 `app` 的命令都会被算成多参数。
 */
function extractSingleValueParamCommands(commandsRs) {
  const source = stripComments(commandsRs);
  const map = new Map();
  const fnRe = /pub\s+(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\(/g;
  let m;
  while ((m = fnRe.exec(source)) !== null) {
    const name = m[1];
    const openIndex = m.index + m[0].length - 1;
    const params = sliceBalanced(source, openIndex);
    const userParams = splitTopLevel(params)
      .map((param) => param.trim())
      .filter((param) => param !== "")
      .map((param) => {
        const colon = param.indexOf(":");
        if (colon < 0) return null;
        return { name: param.slice(0, colon).trim(), type: param.slice(colon + 1).trim() };
      })
      .filter(Boolean)
      .filter((param) => !/AppHandle|State<|Window|WebviewWindow/.test(param.type));
    if (userParams.length !== 1) continue;
    if (!/^(?:serde_json::)?Value$/.test(userParams[0].type)) continue;
    // 取该命令的 `#[tauri::command(...)]` 属性，判断是否需要驼峰化参数名。
    const attribute = source.slice(Math.max(0, m.index - 400), m.index);
    const renameAll = attribute.match(/rename_all\s*=\s*"([^"]+)"\s*\)/);
    const camel = renameAll && renameAll[1] === "camelCase";
    map.set(name, camel ? toCamelCase(userParams[0].name) : userParams[0].name);
  }
  return map;
}

/**
 * 提取 api.ts 中每个 `call("<cmd>", { … })` 的顶层键名。
 *
 * 只登记**对象字面量**调用；实参是变量（`call("x", args)`）时记 `null`，
 * 由调用方决定是报错还是豁免——静默跳过会让护栏悄悄失效。
 */
function extractCallArgKeys(apiTs) {
  const source = stripComments(apiTs);
  const map = new Map();
  const re = new RegExp(`${CALL_PREFIX}"([^"]+)"\\s*(,)?`, "g");
  let m;
  while ((m = re.exec(source)) !== null) {
    const cmd = m[1];
    const afterComma = m.index + m[0].length;
    if (!m[2]) {
      map.set(cmd, []);
      continue;
    }
    let i = afterComma;
    while (i < source.length && /\s/.test(source[i])) i += 1;
    if (source[i] !== "{") {
      map.set(cmd, null);
      continue;
    }
    map.set(cmd, topLevelKeys(sliceBalanced(source, i)));
  }
  return map;
}

/**
 * 从 api.ts 的 `ALIASES` 表提取「旧命令名 → 新命令名」映射（第 3 条据此判定别名）。
 *
 * ## 为什么别名要「声明」而不是「豁免」
 *
 * 命令面参数化后，前端改调新名（`accounts_list`），旧 `ROUTES["get_accounts"]` 就没有
 * 任何 `call()` 引用了 —— 按第 3 条它会被判**死路由**。但旧命令**必须保留**（别名转发 +
 * 可回滚）。与其把旧命令加进一个**无条件静默豁免**集（别名链断了也不报），不如让别名
 * **显式登记**在本表里：ROUTES 条目「活」当且仅当它有 `call()` 引用**或**是本表的键。
 * 再加一条正向约束（见 main 的第 3b 条）：`ALIASES` 的**值**必须自身是活命令，防止
 * 别名指向一条已经死掉的命令。
 */
function extractAliases(apiTs) {
  const anchor = apiTs.indexOf("const ALIASES");
  if (anchor < 0) {
    throw new Error(
      "api.ts 中找不到 `const ALIASES`（第 3 条依赖它区分「别名」与「死路由」）",
    );
  }
  const openIndex = apiTs.indexOf("{", anchor);
  if (openIndex < 0) throw new Error("api.ts 的 ALIASES 不是对象字面量");
  const block = sliceBalanced(apiTs, openIndex);
  // 剥注释：注释里写 `旧名: "新名"` 这类示例会把扫描器骗过去（真实踩过 —— 见 stripComments）。
  const source = stripComments(block);
  const map = new Map();
  const re = /([A-Za-z0-9_]+)\s*:\s*"([^"]+)"/g;
  let m;
  while ((m = re.exec(source)) !== null) map.set(m[1], m[2]);
  return map;
}

/**
 * 提取 commands.rs 里某个 `pub fn <name>(…)` 的**形参名**列表（第 8 条用）。
 *
 * 找不到该 fn 返回 `null`（由调用方报错，而不是静默跳过 —— 静默跳过会让规则失效）。
 */
function extractFunctionParams(commandsRs, fnName) {
  const source = stripComments(commandsRs);
  const re = new RegExp(`pub\\s+(?:async\\s+)?fn\\s+${fnName}\\s*\\(`);
  const m = re.exec(source);
  if (!m) return null;
  const openIndex = m.index + m[0].length - 1;
  const params = sliceBalanced(source, openIndex);
  return splitTopLevel(params)
    .map((param) => param.trim())
    .filter((param) => param !== "")
    .map((param) => {
      const colon = param.indexOf(":");
      return colon < 0 ? param : param.slice(0, colon).trim();
    });
}

/** api.rs 是否从 query/body 读到 `scope`（第 8 条的服务端一侧，粗粒度）。 */
function serverReadsScope(serverRs) {
  const source = stripComments(serverRs);
  return /query_value\s*\([^)]*"scope"/.test(source) || /body\.get\s*\(\s*"scope"\s*\)/.test(source);
}

function main() {
  const apiTs = read(API_TS);
  const screenshotTs = read(SCREENSHOT_DEMO_TS);
  const serverRs = read(SERVER_RS);
  const libRs = read(TAURI_LIB_RS);
  const commandsRs = read(TAURI_COMMANDS_RS);

  const routes = extractRoutes(apiTs);
  const callCmds = extractCallCommands(apiTs);
  const callArgKeys = extractCallArgKeys(apiTs);
  const demoReadCmds = extractDemoReadCommands(apiTs);
  const demoCaseCmds = extractDemoCaseCommands(screenshotTs);
  const serverRoutes = extractServerRoutes(serverRs);
  const invokeCmds = extractInvokeCommands(libRs);
  const singleValueCommands = extractSingleValueParamCommands(commandsRs);
  const aliases = extractAliases(apiTs);
  const aliasKeys = new Set(aliases.keys());

  const routeCmds = new Set(routes.map((r) => r.cmd));
  const errors = [];
  const hints = [];

  // 1) 每个 call("<cmd>") 必须已在桌面端 invoke_handler 登记。
  for (const cmd of [...callCmds].sort()) {
    if (!invokeCmds.has(cmd)) {
      errors.push(
        `[desktop] call("${cmd}") 未在 src-tauri/src/lib.rs 的 invoke_handler 登记 → 桌面端会 command not found`,
      );
    }
  }

  // 2) 每个 call("<cmd>")（桌面专属豁免除外）必须有 ROUTES 条目。
  for (const cmd of [...callCmds].sort()) {
    if (!routeCmds.has(cmd) && !ROUTE_EXEMPT_COMMANDS.has(cmd)) {
      errors.push(
        `[webui] call("${cmd}") 缺少 ROUTES 路由条目 → webui 会抛「暂不支持该操作」`,
      );
    }
  }

  // 3) ROUTES 不得有死路由。一个条目「活」当且仅当它有 call("<cmd>") 引用，**或**它是
  //    ALIASES 的键（旧命令保留为别名 ⇒ 旧 ROUTES 条目不是死路由，而是被**声明**的别名）。
  //
  //    为什么改成「有引用 **或** 是别名键」：命令面参数化后前端改调新名，旧命令**必须保留**
  //    （别名转发 + 可回滚）。若仍按「必须有 call() 引用」判，保留旧命令的**第一步就红**，
  //    且红的原因与真实缺陷无关（假失败）。用 ALIASES 表把别名**显式声明**出来，
  //    比无条件豁免更安全 —— 别名链断了（见 3b）仍会报。
  for (const cmd of [...routeCmds].sort()) {
    if (!callCmds.has(cmd) && !aliasKeys.has(cmd)) {
      errors.push(
        `[webui] ROUTES["${cmd}"] 是死路由：没有 call("${cmd}") 使用它，也不是 ALIASES 的键`,
      );
    }
  }

  // 3b) 别名的两条正向约束：键必须有 ROUTES 条目（webui 才能路由旧命令）；值必须自身是
  //     活命令（防止别名指向一条已经死掉的命令 —— 别名链悄悄腐烂）。
  for (const [oldCmd, newCmd] of [...aliases].sort()) {
    if (!routeCmds.has(oldCmd)) {
      errors.push(
        `[alias] ALIASES 的键 "${oldCmd}" 没有 ROUTES 条目 → webui 无法路由旧命令（别名形同虚设）`,
      );
    }
    if (!callCmds.has(newCmd)) {
      errors.push(
        `[alias] ALIASES["${oldCmd}"] 指向 "${newCmd}"，但后者没有任何 call() 引用 → 别名链断了`,
      );
    }
  }

  // 4) 每个 ROUTES 的 cmd 必须已在桌面端 invoke_handler 登记。
  for (const cmd of [...routeCmds].sort()) {
    if (!invokeCmds.has(cmd)) {
      errors.push(
        `[desktop] ROUTES["${cmd}"] 未在 src-tauri/src/lib.rs 的 invoke_handler 登记 → 桌面端会 command not found`,
      );
    }
  }

  // 5) 每个 ROUTES 的 path + method 必须与服务端路由表一致。
  for (const { cmd, method, path: routePath } of routes) {
    const methods = serverRoutes.get(routePath);
    if (!methods) {
      errors.push(
        `[server] ROUTES["${cmd}"] 的 path "${routePath}" 在 crates/buddy-switch-server/src/api.rs 中不存在`,
      );
      continue;
    }
    if (!methods.has(method)) {
      errors.push(
        `[server] ROUTES["${cmd}"] 以 ${method} 访问 "${routePath}"，但服务端该路由仅支持 [${[...methods]
          .sort()
          .join(", ")}]`,
      );
    }
  }

  // 6) 每个演示只读命令都必须有 screenshotDemoResponse 的 case 分支（漏 case 只在运行时才炸）。
  for (const cmd of [...demoReadCmds].sort()) {
    if (!demoCaseCmds.has(cmd)) {
      errors.push(
        `[demo] DEMO_READ_COMMANDS 含 "${cmd}"，但 src/lib/screenshot-demo.ts 的 screenshotDemoResponse 没有对应 case → 演示模式运行时会抛「演示模式缺少只读数据」`,
      );
    }
  }
  // 反向：case 分支不在 DEMO_READ_COMMANDS 中（可能由 call() 之外的路径演示化）—— 提示级，不失败。
  for (const cmd of [...demoCaseCmds].sort()) {
    if (!demoReadCmds.has(cmd)) {
      hints.push(
        `（提示）screenshot-demo 的 case "${cmd}" 不在 DEMO_READ_COMMANDS 中（可能由 call() 之外的路径演示化）`,
      );
    }
  }

  // 7) 「单 Value 参数」命令：前端必须把该参数名作为**唯一**的顶层键传过去。
  //
  // 这类命令的载荷形状在两侧源码里都是单一、机器可读的，所以能硬校验；
  // 校验失败的典型症状是「webui 正常、桌面端报 missing required key」。
  if (singleValueCommands.size === 0) {
    throw new Error(
      "未能从 src-tauri/src/commands.rs 解析出任何「单 Value 参数」命令 —— 第 7 条校验会静默失效",
    );
  }
  for (const [cmd, expectedKey] of [...singleValueCommands].sort()) {
    // 前端没调用的命令不涉及载荷形状（可能是内部 helper，或由其他通道调用）。
    if (!callCmds.has(cmd) || SINGLE_VALUE_SHAPE_EXEMPT.has(cmd)) continue;
    const keys = callArgKeys.get(cmd);
    if (keys === undefined || keys === null) {
      errors.push(
        `[desktop] call("${cmd}") 的实参不是可解析的对象字面量，无法校验参数名` +
          `（命令签名只接受一个 \`${expectedKey}: Value\`）→ 请改用对象字面量 \`{ ${expectedKey}: … }\`，` +
          `或说明理由后加入 SINGLE_VALUE_SHAPE_EXEMPT`,
      );
      continue;
    }
    if (keys.length !== 1 || keys[0] !== expectedKey) {
      errors.push(
        `[desktop] call("${cmd}") 的顶层键为 [${keys.join(", ") || "（无）"}]，` +
          `但命令签名只接受一个 \`${expectedKey}: Value\` 参数 → 桌面端会报 ` +
          `\`missing required key ${expectedKey}\`（webui 通道因后端兼容平铺，可能看不出问题）`,
      );
    }
  }

  // 8) 参数化命令必须**四处**都带 scope：commands.rs 签名 / api.ts 顶层键 / api.rs 读取。
  //    任一缺失 ⇒ 桌面端与 webui 行为分叉，且构建全绿（最难发现的一类漂移）。
  for (const cmd of [...SCOPE_PARAMETERIZED].sort()) {
    const params = extractFunctionParams(commandsRs, cmd);
    if (params === null) {
      errors.push(`[scope] SCOPE_PARAMETERIZED 含 "${cmd}"，但 commands.rs 找不到该命令的 fn`);
    } else if (!params.includes("scope")) {
      errors.push(
        `[scope] 参数化命令 "${cmd}" 的 commands.rs 签名缺 \`scope\` 参数（桌面端拿不到作用域）`,
      );
    }

    const keys = callArgKeys.get(cmd);
    if (keys === undefined) {
      errors.push(`[scope] 参数化命令 "${cmd}" 在 api.ts 没有 \`call("${cmd}", {…})\` 调用点`);
    } else if (keys === null) {
      errors.push(
        `[scope] call("${cmd}") 的实参不是对象字面量，无法校验顶层是否含 \`scope\`（请改用对象字面量）`,
      );
    } else if (!keys.includes("scope")) {
      errors.push(`[scope] call("${cmd}") 的顶层键缺 \`scope\` → webui 通道拿不到作用域`);
    }

    if (!serverReadsScope(serverRs)) {
      errors.push(`[scope] api.rs 的 "${cmd}" handler 未从 query/body 读到 \`scope\``);
    }
  }

  // 9) scope 解析必须 fail-closed：反例扫描（禁 fail-open 回落）+ 正向断言（Result + 单测）。
  for (const file of SCOPE_RESOLUTION_FILES) {
    const code = stripComments(read(file));
    const rel = path.relative(ROOT, file);
    for (const re of SCOPE_FAIL_OPEN_PATTERNS) {
      const hit = re.exec(code);
      if (hit) {
        errors.push(
          `[scope] ${rel} 出现 fail-open 回落 \`${hit[0]}\` —— scope 解析必须 fail-closed，` +
            `未知 / 空 / 缺一律 Err（静默回落会读写**错误的库**）`,
        );
      }
    }
  }
  {
    const productRs = read(CORE_PRODUCT_RS);
    const rel = path.relative(ROOT, CORE_PRODUCT_RS);
    if (
      !/pub\s+fn\s+parse\s*\([^)]*\)\s*->\s*Result<\s*Scope\s*,\s*ScopeError\s*>/.test(
        stripComments(productRs),
      )
    ) {
      errors.push(`[scope] ${rel} 的 Scope::parse 未返回 Result<Scope, ScopeError>（fail-closed 的前提）`);
    }
    if (!/fn\s+scope_parse_is_fail_closed_for_unknown_empty_and_typos\s*\(/.test(productRs)) {
      errors.push(
        `[scope] ${rel} 缺少「未知 scope ⇒ Err」的单测 ` +
          `\`scope_parse_is_fail_closed_for_unknown_empty_and_typos\``,
      );
    }
    if (!/\.is_err\s*\(\s*\)/.test(productRs)) {
      errors.push(
        `[scope] ${rel} 未发现 \`.is_err()\` 断言 —— 「未知 scope ⇒ Err」的错误分支可能不可达（断言在空转）`,
      );
    }
  }

  // 10) 全局探测不得进任何写路径（宿主的命令 / handler 里禁止裸调用）。
  for (const file of HOST_COMMAND_FILES) {
    const code = stripComments(read(file));
    const rel = path.relative(ROOT, file);
    for (const fn of ["detected_variant", "detect_data_dir"]) {
      if (new RegExp(`\\b${fn}\\s*\\(`).test(code)) {
        errors.push(
          `[alias] ${rel} 出现全局探测 \`${fn}(\` —— 禁止进任何写路径（会按本机活跃客户端` +
            `推断作用域，把数据写进错误的库）。写侧来源应用 \`detect_data_dir_for(variant)\`。`,
        );
      }
    }
  }

  if (hints.length > 0) {
    for (const hint of hints) process.stdout.write(`${hint}\n`);
  }

  if (errors.length > 0) {
    process.stderr.write(`API 契约校验失败（共 ${errors.length} 项）：\n`);
    for (const error of errors) process.stderr.write(`  - ${error}\n`);
    process.exit(1);
  }

  process.stdout.write(
    `API 契约校验通过：routes=${routes.length}，call=${callCmds.size}，` +
      `server=${serverRoutes.size}，invoke=${invokeCmds.size}，` +
      `demoRead=${demoReadCmds.size}，demoCase=${demoCaseCmds.size}，` +
      `单Value参数=${singleValueCommands.size}，` +
      `参数化=${SCOPE_PARAMETERIZED.size}，别名=${aliases.size}，` +
      `桌面专属豁免=${ROUTE_EXEMPT_COMMANDS.size}\n`,
  );
}

try {
  main();
} catch (error) {
  process.stderr.write(`API 契约校验无法执行：${error && error.message ? error.message : error}\n`);
  process.exit(1);
}
