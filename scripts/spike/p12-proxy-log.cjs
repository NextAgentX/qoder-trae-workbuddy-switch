#!/usr/bin/env node
/**
 * P12 spike · H1 —— 「只记录 CONNECT」的最小 HTTP 代理。
 *
 * ## 这个脚本是干什么的（以及**不**干什么）
 *
 * 它只回答一个问题（H1）：
 *
 * > 把客户端指向一个 HTTP 代理后，代理日志里**是否出现目标业务域的 CONNECT**？
 *
 * 因此它刻意**只做一件事**：把 `CONNECT host:port` 记下来，然后**原样 TCP 盲转发**
 * （不解密、不改包、不落正文）。据此判断客户端**是否真的走代理**。
 *
 * **★ 它不装 CA、不做 TLS 中间人、不解密任何流量** —— 那是 H2 的事，且必须由**用户
 * 手动**把自签 CA 装进系统信任库（设计决策 #3：应用不得自动提权）。本脚本与 CA 无关。
 *
 * ## 为什么是 Node（无第三方依赖）
 *
 * 只用 `node:http` / `node:net` / `node:fs` —— 不引入 `node-forge` 之类会让人以为
 * 「是不是顺便把证书也做了」的依赖。**看到这个文件的人应当立刻确信它没有解密能力**。
 *
 * ## 安全边界（与设计 §1.6 三条硬边界一致）
 *
 * 1. **只监听回环地址**（默认 `127.0.0.1`）—— 不对外暴露，不劫持任何全局设置；
 * 2. **不写任何系统/客户端配置** —— 是否把客户端指过来，由用户手动决定；
 * 3. **不落盘请求正文** —— 只记 `CONNECT host:port` 与明文 HTTP 的 method/路径。
 *
 * ## 用法
 *
 * ```bash
 * # 起代理（前台，Ctrl-C 退出并打印按域汇总）
 * node scripts/spike/p12-proxy-log.cjs
 *
 * # 换端口 / 换日志位置
 * P12_PROXY_PORT=8888 P12_PROXY_LOG=/tmp/p12.log node scripts/spike/p12-proxy-log.cjs
 *
 * # 自检（离线，不需要真客户端、不需要外网、不需要 CA）：证明「代理会记录 CONNECT」
 * node scripts/spike/p12-proxy-log.cjs --selftest
 * ```
 *
 * 环境变量：
 * - `P12_PROXY_PORT`  监听端口（默认 `18787`）
 * - `P12_PROXY_HOST`  监听地址（默认 `127.0.0.1`，**不要**改成 `0.0.0.0`）
 * - `P12_PROXY_LOG`   日志文件路径（默认 `<本目录>/p12-proxy-connect.log`）
 *
 * ⚠️ **spike-only 工具**：`scripts/spike/*` 不得进打包产物（设计决策 #4）。
 */

"use strict";

const http = require("node:http");
const net = require("node:net");
const fs = require("node:fs");
const path = require("node:path");

/** 默认监听端口。挑一个不常见的高位端口，避免和 7864（网关）等既有端口冲突。 */
const DEFAULT_PORT = 18787;
/** 默认只绑回环 —— 绝不对外监听。 */
const DEFAULT_HOST = "127.0.0.1";
/** 默认日志落点（与脚本同目录；`.gitignore` 已忽略 `*.log`）。 */
const DEFAULT_LOG = path.join(__dirname, "p12-proxy-connect.log");

/** 当前时间的 ISO 串，用于日志前缀。 */
function stamp() {
  return new Date().toISOString();
}

/**
 * 把 `host:port` 拆开；缺端口时回落 `fallbackPort`（CONNECT 通常省略 443）。
 *
 * @param {string} authority `CONNECT` 行里的 `host:port`（IPv6 形如 `[::1]:443`）。
 * @param {number} fallbackPort 缺省端口。
 * @returns {{ host: string, port: number }}
 */
function splitHostPort(authority, fallbackPort) {
  const raw = String(authority || "").trim();
  if (raw.startsWith("[")) {
    // IPv6 字面量：[::1]:443
    const end = raw.indexOf("]");
    const host = raw.slice(1, end === -1 ? raw.length : end);
    const rest = end === -1 ? "" : raw.slice(end + 1);
    const port = rest.startsWith(":") ? Number(rest.slice(1)) : NaN;
    return { host, port: Number.isFinite(port) ? port : fallbackPort };
  }
  const idx = raw.lastIndexOf(":");
  if (idx === -1) {
    return { host: raw, port: fallbackPort };
  }
  const host = raw.slice(0, idx);
  const port = Number(raw.slice(idx + 1));
  return { host, port: Number.isFinite(port) ? port : fallbackPort };
}

/**
 * 代理日志器：既写 stdout（人看），也追加到文件（留证据），并维护按域计数。
 */
class ProxyLogger {
  /**
   * @param {string} logPath 日志文件路径。
   * @param {boolean} [silent] 为 `true` 时只写文件、不写 stdout（自检用）。
   */
  constructor(logPath, silent = false) {
    this.logPath = logPath;
    this.silent = silent;
    /** @type {Map<string, number>} host:port -> 次数 */
    this.hosts = new Map();
    this.connectCount = 0;
    this.plainCount = 0;
  }

  /** 追加一行（带时间戳）。写文件失败不影响代理运行（spike 工具，尽力而为）。 */
  append(line) {
    const rec = `[${stamp()}] ${line}`;
    if (!this.silent) {
      process.stdout.write(rec + "\n");
    }
    try {
      fs.appendFileSync(this.logPath, rec + "\n");
    } catch {
      /* 磁盘/权限问题不阻塞代理：日志是证据，不是运行前提 */
    }
  }

  /** 记录一次 CONNECT。 */
  connect(host, port) {
    const key = `${host}:${port}`;
    this.connectCount += 1;
    this.hosts.set(key, (this.hosts.get(key) || 0) + 1);
    this.append(`CONNECT ${key}`);
  }

  /** 记录一次明文 HTTP 请求（非 CONNECT）。 */
  plain(method, target, hostHeader) {
    this.plainCount += 1;
    this.append(`HTTP ${method} ${target} (Host: ${hostHeader || "-"})`);
  }

  /** 返回按域计数的汇总文本（退出时打印）。 */
  summary() {
    const lines = [
      "",
      "================ P12 H1 代理汇总 ================",
      `CONNECT 次数 : ${this.connectCount}`,
      `明文 HTTP 次数: ${this.plainCount}`,
      "按目标域统计（host:port -> 次数）:",
    ];
    if (this.hosts.size === 0) {
      lines.push("  （没有任何 CONNECT —— 客户端可能没走代理）");
    } else {
      const sorted = [...this.hosts.entries()].sort((a, b) => b[1] - a[1]);
      for (const [key, n] of sorted) {
        lines.push(`  ${key}  x${n}`);
      }
    }
    lines.push("================================================");
    return lines.join("\n");
  }
}

/**
 * 创建一个「只记录 CONNECT」的最小代理服务（未 `listen`）。
 *
 * 导出成函数是为了让 `--selftest` 能在**同一进程内**起服务并自测，
 * 而不必 spawn 子进程去猜端口。
 *
 * @param {{ logger: ProxyLogger }} opts
 * @returns {import("node:http").Server}
 */
function createProxyServer(opts) {
  const { logger } = opts;

  const server = http.createServer();

  // --- 明文 HTTP（非 CONNECT）：记录后原样转发 ---------------------------------
  server.on("request", (req, res) => {
    logger.plain(req.method, req.url, req.headers.host);
    let target;
    try {
      target = req.url.startsWith("http")
        ? new URL(req.url)
        : new URL(`http://${req.headers.host}${req.url}`);
    } catch {
      res.writeHead(400, { "content-type": "text/plain" });
      res.end("p12-proxy: 无法解析目标 URL");
      return;
    }
    const upstream = http.request(
      {
        host: target.hostname,
        port: target.port || 80,
        method: req.method,
        path: target.pathname + target.search,
        headers: req.headers,
      },
      (upstreamRes) => {
        res.writeHead(upstreamRes.statusCode || 502, upstreamRes.headers);
        upstreamRes.pipe(res);
      },
    );
    upstream.on("error", (err) => {
      logger.append(`  ! 明文转发失败 ${target.host}: ${err.message}`);
      if (!res.headersSent) {
        res.writeHead(502, { "content-type": "text/plain" });
      }
      res.end(`p12-proxy: ${err.message}`);
    });
    req.pipe(upstream);
  });

  // --- CONNECT：**只记录，然后盲转发**（不解密）--------------------------------
  server.on("connect", (req, clientSocket, head) => {
    const { host, port } = splitHostPort(req.url, 443);
    logger.connect(host, port);

    const upstream = net.connect(port, host, () => {
      // 告诉客户端隧道已建立；此后两端字节**原样互传**，本进程看不到明文。
      clientSocket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      if (head && head.length) {
        upstream.write(head);
      }
      upstream.pipe(clientSocket);
      clientSocket.pipe(upstream);
    });

    upstream.on("error", (err) => {
      logger.append(`  ! 上游连接失败 ${host}:${port}: ${err.message}`);
      if (!clientSocket.destroyed) {
        clientSocket.end();
      }
    });
    clientSocket.on("error", () => {
      upstream.destroy();
    });
    clientSocket.on("close", () => {
      upstream.destroy();
    });
  });

  return server;
}

/**
 * 离线自检：证明「代理确实会把 CONNECT 记进日志」。
 *
 * 全程**不需要真客户端、不需要外网、不需要 CA**：
 * 起一个本地 TCP 假目标 → 通过代理发一条**裸 CONNECT** → 断言拿到
 * `200 Connection Established` 且日志里出现该 CONNECT。
 *
 * @returns {Promise<number>} 进程退出码（0 = 通过）。
 */
async function selftest() {
  const tmpLog = path.join(
    require("node:os").tmpdir(),
    `p12-proxy-selftest-${process.pid}.log`,
  );
  try {
    fs.writeFileSync(tmpLog, "");
  } catch {
    /* ignore */
  }

  // 1) 本地「假目标」：接受连接后回显收到的字节，充当被 CONNECT 的目标主机。
  const dummy = net.createServer((sock) => {
    sock.on("data", (buf) => sock.write(buf));
  });
  await new Promise((resolve) => dummy.listen(0, "127.0.0.1", resolve));
  const dummyPort = dummy.address().port;

  // 2) 起代理（静默日志，写临时文件）。
  const logger = new ProxyLogger(tmpLog, true);
  const server = createProxyServer({ logger });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const proxyPort = server.address().port;

  let ok = true;
  const failures = [];

  try {
    // 3) 作为「客户端」向代理发一条裸 CONNECT，指向本地假目标。
    const result = await new Promise((resolve, reject) => {
      const sock = net.connect(proxyPort, "127.0.0.1", () => {
        sock.write(
          `CONNECT 127.0.0.1:${dummyPort} HTTP/1.1\r\n` +
            `Host: 127.0.0.1:${dummyPort}\r\n\r\n`,
        );
      });
      let buf = "";
      let tunneled = false;
      const timer = setTimeout(() => {
        sock.destroy();
        reject(new Error("自检超时（5s 内未拿到 200 Connection Established）"));
      }, 5000);
      sock.on("data", (chunk) => {
        buf += chunk.toString("utf8");
        if (!tunneled && buf.includes("200 Connection Established")) {
          tunneled = true;
          // 隧道已建立：再发一个字节，验证盲转发通路（假目标会回显）。
          sock.write("PING");
          return;
        }
        if (tunneled && buf.includes("PING")) {
          clearTimeout(timer);
          sock.destroy();
          resolve({ established: true, echo: true });
        }
      });
      sock.on("error", (err) => {
        clearTimeout(timer);
        reject(err);
      });
    });

    if (!result.established) {
      ok = false;
      failures.push("未收到 200 Connection Established");
    }

    // 4) 断言日志里记下了这条 CONNECT。
    const logText = fs.readFileSync(tmpLog, "utf8");
    const expect = `CONNECT 127.0.0.1:${dummyPort}`;
    if (!logText.includes(expect)) {
      ok = false;
      failures.push(`日志里没有 "${expect}"`);
    }
    if (logger.connectCount !== 1) {
      ok = false;
      failures.push(`CONNECT 计数应为 1，实际 ${logger.connectCount}`);
    }
  } catch (err) {
    ok = false;
    failures.push(err && err.message ? err.message : String(err));
  } finally {
    await new Promise((resolve) => server.close(resolve));
    await new Promise((resolve) => dummy.close(resolve));
    try {
      fs.unlinkSync(tmpLog);
    } catch {
      /* ignore */
    }
  }

  if (ok) {
    process.stdout.write(
      "[selftest] PASS —— 代理会记录 CONNECT，且隧道盲转发通路正常。\n",
    );
    return 0;
  }
  process.stdout.write(`[selftest] FAIL —— ${failures.join("；")}\n`);
  return 1;
}

/** 前台运行代理（Ctrl-C 退出并打印汇总）。 */
function main() {
  const port = Number(process.env.P12_PROXY_PORT) || DEFAULT_PORT;
  const host = process.env.P12_PROXY_HOST || DEFAULT_HOST;
  const logPath = process.env.P12_PROXY_LOG || DEFAULT_LOG;

  if (host !== "127.0.0.1" && host !== "localhost" && host !== "::1") {
    process.stderr.write(
      `[p12-proxy] 拒绝监听非回环地址 ${host}（spike 工具只允许本机回环）。\n`,
    );
    process.exit(2);
  }

  try {
    fs.mkdirSync(path.dirname(logPath), { recursive: true });
  } catch {
    /* ignore */
  }

  const logger = new ProxyLogger(logPath);
  const server = createProxyServer({ logger });

  server.on("error", (err) => {
    process.stderr.write(`[p12-proxy] 监听失败：${err.message}\n`);
    process.exit(1);
  });

  server.listen(port, host, () => {
    logger.append(`p12-proxy 已启动 http://${host}:${port}（只记录 CONNECT，不解密）`);
    logger.append(`日志文件：${logPath}`);
    logger.append("把客户端 HTTP 代理指向上面地址；本工具不会改动任何系统设置。");
  });

  const shutdown = () => {
    process.stdout.write(logger.summary() + "\n");
    server.close(() => process.exit(0));
    // 兜底：连接挂着时 2s 后强退。
    setTimeout(() => process.exit(0), 2000).unref();
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

module.exports = { createProxyServer, ProxyLogger, splitHostPort };

if (require.main === module) {
  if (process.argv.includes("--selftest")) {
    selftest().then((code) => process.exit(code));
  } else {
    main();
  }
}
