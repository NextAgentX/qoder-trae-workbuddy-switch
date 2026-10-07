#!/usr/bin/env node
/**
 * P12 spike · H2/H3 **半自动** harness
 *
 * ## 它解决什么
 *
 * `P12-ARCH.md` 把 H2/H3 定为「**必须用户手动**」，原因是：
 *   - H2 要**装自签 CA 进系统信任库**（决策 #3：应用**不得自动提权**）
 *   - H3 要**真实业务流量**（签到 / 积分 / 旅行）
 *
 * 本 harness **不改变这个边界**（仍然不自动装 CA、不自动跑客户端），
 * 而是把「人工」压缩到最小：**一条命令 + 点一次业务操作**，其余全自动。
 *
 * ## 它**不做**什么（刻意的安全边界）
 *
 * - **不装 / 不卸 CA** —— 只**打印**命令，由用户执行（系统级改动必须用户知情）
 * - **不启动客户端** —— 只提示用户去点
 * - **不落请求 / 响应正文**（沿用 `p12-mitm-addon.py` 的默认：只记元数据）
 *
 * ## 用法
 *
 *   node scripts/spike/p12-h2h3-harness.mjs ca      # ① 打印 CA 路径 + 装/卸命令
 *   node scripts/spike/p12-h2h3-harness.mjs h2      # ② 起中间人（H2：看是否报证书错误）
 *   node scripts/spike/p12-h2h3-harness.mjs h3      # ③ 起中间人 + 替换凭据（H3：看上游 200）
 *   node scripts/spike/p12-h2h3-harness.mjs judge   # ④ 读 log，给结论
 *
 * ⚠️ **spike-only**：`scripts/spike/*` 不得进打包产物（设计决策 #4）。
 */

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";

const ROOT = resolve(import.meta.dirname, "../..");
const SPIKE = join(ROOT, "scripts", "spike");
const LOG = join(SPIKE, "p12-mitm.log");
const CAPTURE = join(SPIKE, "p12-capture");
const ADDON = join(SPIKE, "p12-mitm-addon.py");
const PORT = Number(process.env.P12_MITM_PORT || 18888);

/** mitmproxy 的 venv（本仓 spike 专用，不污染系统）。 */
const MITM_PY =
  process.env.P12_MITM_PY ||
  "C:/Users/Jackey/.workbuddy-ai/binaries/python/envs/p12spike/Scripts/python.exe";

/** CA 文件（mitmproxy 首次运行会生成到 `~/.mitmproxy/`）。 */
const CA_DIR = join(homedir(), ".mitmproxy");
const CA_PEM = join(CA_DIR, "mitmproxy-ca-cert.pem");
const CA_CER = join(CA_DIR, "mitmproxy-ca-cert.cer");

/** 域白名单（与 README / P12-SPIKE-RESULTS 一致）。 */
const DOMAINS = [
  "www.workbuddy.cn",
  "www.codebuddy.cn",
  "copilot.tencent.com",
  "api.trae.cn",
  "trae-api-cn.mchost.guru",
].join(",");

function log(msg) {
  process.stdout.write(`${msg}\n`);
}

function die(msg, code = 1) {
  process.stderr.write(`${msg}\n`);
  process.exit(code);
}

// ---------------------------------------------------------------------------
// ① ca —— 只打印命令，绝不执行（系统级改动必须用户知情）
// ---------------------------------------------------------------------------

function cmdCa() {
  log("═".repeat(72));
  log("H2 前置 · 安装 mitmproxy 自签 CA（**这一步需要你执行**）");
  log("═".repeat(72));
  log("");
  if (!existsSync(CA_PEM) && !existsSync(CA_CER)) {
    log("⚠️ CA 尚未生成。请先跑一次 `… h2`（起中间人时会自动生成），再回来执行本步。");
    log(`   预期路径：${CA_PEM}`);
    log("");
    return;
  }
  const cer = existsSync(CA_CER) ? CA_CER : CA_PEM;
  log(`CA 文件：${cer}`);
  log("");
  log("【安装】Windows —— 注意 `-user Root` 是**当前用户**信任库，**不需要管理员**：");
  log("");
  log(`    certutil -addstore -user Root "${cer}"`);
  log("");
  log("  若要装进**机器级**信任库（需管理员，影响所有用户）——不推荐：");
  log(`    certutil -addstore Root "${cer}"      # 需提权`);
  log("");
  log("【卸载 · 一键还原】跑完 spike **务必**执行：");
  log("");
  log(`    certutil -delstore -user Root "${cer}"`);
  log("");
  log("【验证已装】");
  log("");
  log(`    certutil -user -store Root | findstr /i mitmproxy`);
  log("");
  log("═".repeat(72));
  log("装完后：把客户端 HTTP/HTTPS 代理指向 127.0.0.1:" + PORT + "，再跑 `… h2`");
  log("═".repeat(72));
}

// ---------------------------------------------------------------------------
// ② h2 / ③ h3 —— 起中间人（前台），自动带上 addon 与白名单
// ---------------------------------------------------------------------------

function cmdProxy(mode) {
  if (!existsSync(ADDON)) die(`找不到 addon：${ADDON}`);
  if (!existsSync(MITM_PY)) {
    die(
      `找不到 mitmproxy venv：${MITM_PY}\n` +
        `（可用 P12_MITM_PY 环境变量指定其它 python；或先装：\n` +
        `   <python> -m venv … && …/Scripts/pip install mitmproxy）`,
    );
  }
  const env = {
    ...process.env,
    P12_MITM_DOMAINS: process.env.P12_MITM_DOMAINS || DOMAINS,
    P12_MITM_LOG: LOG,
    P12_MITM_CAPTURE_DIR: CAPTURE,
  };
  if (mode === "h3") {
    if (!process.env.P12_MITM_REPLACE_AUTH) {
      log("⚠️ H3 需要 `P12_MITM_REPLACE_AUTH`（要替换成的凭据值）。");
      log("   例：P12_MITM_REPLACE_AUTH='Bearer eyJ…' node … h3");
      log("   未设置 ⇒ 本次只抓包、不替换（等同于 H2 的抓包能力）。");
      log("");
    } else {
      env.P12_MITM_REPLACE_AUTH = process.env.P12_MITM_REPLACE_AUTH;
      log("✓ 已启用凭据替换（命中白名单域时改写 Authorization）。");
    }
  }
  log("═".repeat(72));
  log(`P12 spike · ${mode.toUpperCase()} —— 中间人启动中`);
  log("═".repeat(72));
  log(`端口：127.0.0.1:${PORT}    白名单域：${DOMAINS}`);
  log(`日志：${LOG}`);
  log(`结构化记录：${CAPTURE}`);
  log("");
  log("【你现在要做的】");
  log(`  1) 把客户端（WorkBuddy / Trae）的 HTTP+HTTPS 代理指向 127.0.0.1:${PORT}`);
  log("  2) 在客户端里**点一次业务操作**（签到 / 积分 / 旅行 任选）");
  log("  3) 观察客户端是否**弹证书错误**：");
  log("       · 不弹、业务正常 ⇒ H2 **通过**");
  log("       · 弹证书错误        ⇒ H2 **失败**（= 证书固定 ⇒ A 路出局）");
  log("  4) 回到本终端按 Ctrl+C 停止，再跑 `… judge`");
  log("");
  log("⚠️ 跑完记得：客户端代理改回「无」+ 执行 `… ca` 里的**卸载命令**");
  log("═".repeat(72));

  const child = spawn(MITM_PY, ["-m", "mitmproxy", "-s", ADDON, "-p", String(PORT)], {
    cwd: ROOT,
    env,
    stdio: "inherit",
  });
  child.on("exit", (code) => log(`\n中间人已退出（code=${code}）。下一步：node … judge`));
}

// ---------------------------------------------------------------------------
// ④ judge —— 读 log，按 P12-ARCH 的判据给结论
// ---------------------------------------------------------------------------

/** addon 的日志行格式（见 `p12-mitm-addon.py`）：
 *   `REQ <method> <host><path> casing_preserved=<bool>`
 *   `RES <host><path> status=<n> error=<e>`
 *   `ERR <host><path> error=<e>`
 *   `    AUTH 已替换（原长度 N -> 新长度 M）`
 */
function cmdJudge() {
  log("═".repeat(72));
  log("P12 spike · 判定（读 log）");
  log("═".repeat(72));
  if (!existsSync(LOG)) {
    log(`⚠️ 没有日志：${LOG}`);
    log("   ⇒ 你还没跑过 `… h2`。**H2 = 未完成**（不是通过，也不是失败）。");
    return;
  }
  const lines = readFileSync(LOG, "utf8").split(/\r?\n/).filter(Boolean);
  // ⚠️ addon 每行都带 `[时间戳] ` 前缀（见 p12-mitm-addon.py 的 `_log`）⇒ 必须先剥掉再匹配，
  //    否则 `startsWith("ERR ")` 永远为 false（本 harness 首版就栽在这）。
  const strip = (l) => l.replace(/^\[[^\]]+\]\s*/, "");
  const reqs = lines.filter((l) => /^REQ\s/.test(strip(l)));
  const res = lines.filter((l) => /^RES\s/.test(strip(l)));
  const errs = lines.filter((l) => /^ERR\s/.test(strip(l)));
  const authReplaced = lines.filter((l) => strip(l).includes("AUTH 已替换"));
  const ok = res.filter((l) => /status=2\d\d/.test(l));
  const rejected = res.filter((l) => /status=(401|403)/.test(l));
  // TLS / 证书类错误 = 客户端不信任 CA 的直接证据
  const tlsErrs = errs.filter((l) => /tls|ssl|cert|handshake|alert/i.test(l));

  log("");
  log(`日志行数 ${lines.length} · 目标域请求 ${reqs.length} · 响应 ${res.length}` +
      `（2xx ${ok.length} / 401-403 ${rejected.length}）· 错误 ${errs.length}（TLS 类 ${tlsErrs.length}）` +
      ` · 凭据替换 ${authReplaced.length} 次`);
  log("");
  log("── H2 判定（客户端是否报证书错误）──");
  if (res.length === 0 && errs.length === 0) {
    log("  ⛔ 未完成 —— 没有任何目标域流量。客户端**没走代理**，或没触发业务操作。");
    log("     ⇒ 先确认客户端代理指向 127.0.0.1:" + PORT + "，再点一次业务操作。");
  } else if (tlsErrs.length > 0) {
    log("  ❌ H2 失败 —— 出现 TLS/证书类错误 ⇒ 客户端**不信任自签 CA**（= 证书固定）⇒ **A 路出局**。");
    log(`     证据：${tlsErrs[0].slice(0, 140)}`);
  } else if (ok.length > 0) {
    log("  ✅ H2 通过 —— 有成功响应（2xx）且无 TLS 错误 ⇒ 客户端**接受自签 CA**。");
  } else {
    log("  ⚠️ H2 待定 —— 有流量但无 2xx、也无 TLS 错误（可能 4xx/5xx 或业务侧问题）。");
    log("     ⇒ 需人工看客户端界面：业务功能是否正常？");
  }
  log("");
  log("── H3 判定（凭据可否按请求替换）──");
  if (authReplaced.length === 0) {
    log("  ⛔ 未完成 —— 没有发生凭据替换。");
    log("     ⇒ 若要验 H3，需设 `P12_MITM_REPLACE_AUTH` 后重跑 `… h3`。");
  } else if (rejected.length > 0) {
    log("  ❌ H3 失败 —— 替换后上游返回 401/403 ⇒ 该平台**凭据替换不可行**。");
    log(`     证据：${rejected[0].slice(0, 140)}`);
  } else if (ok.length > 0) {
    log("  ✅ H3 通过 —— 替换凭据后上游返回 2xx ⇒ **可换号**。");
  } else {
    log("  ⚠️ H3 待定 —— 已替换但无 2xx、也无 401-403。");
  }
  log("");
  log("⚠️ 跑完请把客户端代理改回「无」，并执行 `… ca` 里的**卸载命令**移除 CA。");
  log("═".repeat(72));
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

const step = (process.argv[2] || "").toLowerCase();
switch (step) {
  case "ca":
    cmdCa();
    break;
  case "h2":
    cmdProxy("h2");
    break;
  case "h3":
    cmdProxy("h3");
    break;
  case "judge":
    cmdJudge();
    break;
  default:
    log("用法：node scripts/spike/p12-h2h3-harness.mjs <ca|h2|h3|judge>");
    log("");
    log("  ca     打印 CA 路径 + 装/卸命令（**你执行**，用户级信任库不需管理员）");
    log("  h2     起中间人（H2：看客户端是否报证书错误）");
    log("  h3     起中间人 + 替换凭据（H3：看上游是否 200）");
    log("  judge  读 log，给结论");
    process.exit(step ? 1 : 0);
}
