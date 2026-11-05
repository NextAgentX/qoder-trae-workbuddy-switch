#!/usr/bin/env node
/**
 * 生成 GitHub Release 的「版本变化内容」正文。
 *
 * 为什么自己生成、而**不用** `softprops/action-gh-release` 的
 * `generate_release_notes: true`：GitHub 那套自动生成只从 **Pull Request** 里取
 * 变更（标题、标签、贡献者）。本仓的提交是**直接推到 main**（不用 PR），实测那种
 * 情况下自动生成的结果只有一行 `**Full Changelog**: ...compare/...` —— 也就是
 * 「没有变化内容」。而本仓的提交**恰好**严格遵循 Conventional Commits（见
 * `AGENTS.md` → Git Commit Language），所以直接按前缀分组就是一份可用的中文变更说明。
 *
 * 输入：两个 ref 之间的提交（默认「上一个 tag」→「本次 tag」）。
 * 输出：Markdown 正文，写到 `--out`（默认 `release-notes.md`）。
 *
 * usage:
 *   node scripts/gen-release-notes.mjs --tag v2026.9.221126
 *   node scripts/gen-release-notes.mjs --tag v2026.9.221126 --from v2026.9.211636
 *   node scripts/gen-release-notes.mjs --tag v2026.9.221126 --repo NextAgentX/qoder-trae-workbuddy-switch
 *   node scripts/gen-release-notes.mjs --help
 *
 * ## 几处刻意选择（改动前先读）
 *
 * 1. **只取提交首行，不取正文。** 本仓的提交正文动辄几十行、且含大量内部实现细节与
 *    ⚠️ 警戒（那是写给维护者的）。Release 正文面向安装用户，一行一项即可。
 * 2. **不排除 merge 提交之外的东西，但排除 merge 本身**（`--no-merges`）：
 *    本仓是线性 history + squash 式提交；merge 提交的标题（`Merge pull request #N`）
 *    对用户没有信息量。
 * 3. **生成失败不阻断发布。** 变更说明是**附加信息**：缺失是「降级」，而发布失败是
 *    「事故」。因此本脚本任何异常都会退化为一段兜底说明 + `::warning::` 注解，
 *    退出码仍为 0。**不要**改成 `exit 1` —— 那会把「说明没生成出来」放大成
 *    「二进制发不出去」。
 * 4. **人工润色优先。** 若存在 `docs/releases/<tag>.md`（或去掉 `v` 前缀的同名文件），
 *    则**整篇采用**该文件、不做任何自动拼装 —— 需要人工措辞时用它，不要让脚本去猜。
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CUSTOM_NOTES_DIR = resolve(REPO_ROOT, "docs/releases");

/** 分组顺序即正文顺序：用户最关心的在前，维护性的在后。 */
const GROUPS = [
  { title: "🚀 新功能", types: ["feat"] },
  { title: "🐛 问题修复", types: ["fix"] },
  { title: "⚡ 性能优化", types: ["perf"] },
  { title: "♻️ 重构与内部调整", types: ["refactor"] },
  { title: "📝 文档", types: ["docs"] },
  { title: "🔧 构建与 CI", types: ["build", "ci"] },
  { title: "🧹 其他维护", types: ["chore", "style", "test", "revert"] },
];
const BREAKING_TITLE = "⚠️ 破坏性变更";
const FALLBACK_TITLE = "其他变更";

/** `type(scope)!: 描述`（容忍中文全角冒号，本仓曾混用过）。 */
const CONVENTIONAL = /^([A-Za-z]+)(?:\(([^)]*)\))?(!)?\s*[:：]\s*(.*)$/;

function git(args) {
  try {
    return execFileSync("git", args, {
      cwd: REPO_ROOT,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 64 * 1024 * 1024,
    }).trim();
  } catch (error) {
    // 只保留 stderr 的第一行：`execFileSync` 默认把**整条命令**（含 `%x1f` 这类
    // 控制字符）拼进 message，而下面要把这个原因写进 Release 正文与 GitHub 注解 ——
    // 注解是单行语义，多行内容会被截断或糊成一行。
    const stderr = error && error.stderr ? String(error.stderr).trim() : "";
    const first = stderr.split("\n").map((line) => line.trim()).filter(Boolean)[0];
    const detail =
      first || (error instanceof Error ? error.message.split("\n")[0] : String(error));
    throw new Error(`git ${args[0]} 失败：${detail}`);
  }
}

function gitOrEmpty(args) {
  try {
    return git(args);
  } catch {
    return "";
  }
}

function parseArgs(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") {
      opts.help = true;
      continue;
    }
    if (!arg.startsWith("--")) throw new Error(`无法识别的参数：${arg}`);
    const [flag, inline] = arg.slice(2).split("=");
    const key = flag.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    const value = inline !== undefined ? inline : argv[i + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new Error(`参数 --${flag} 缺少取值`);
    }
    if (inline === undefined) i += 1;
    opts[key] = value;
  }
  return opts;
}

/** `owner/repo`：CI 里有 GITHUB_REPOSITORY，本机则回落到 origin 远端。 */
function resolveRepo(opts) {
  if (opts.repo) return opts.repo.replace(/^https?:\/\/github\.com\//, "").replace(/\.git$/, "");
  if (process.env.GITHUB_REPOSITORY) return process.env.GITHUB_REPOSITORY;
  const url = gitOrEmpty(["remote", "get-url", "origin"]);
  const match = url.match(/github\.com[:/]([^/\s]+)\/([^/\s]+?)(?:\.git)?$/);
  return match ? `${match[1]}/${match[2]}` : "";
}

/**
 * 上一个 tag。优先 `git describe`（选**距离最近**的可达 tag）。
 * 它的失败是正常分支而不是异常：当前 tag 之前的提交若从未打过 tag
 * （本仓目前就只有 1 个 tag），`describe` 会 `fatal: No tags can describe`，
 * 此时回落到「所有可达 tag 里日期最新的那个」，仍无则视为首次发布。
 */
function previousTag(tag) {
  if (!tag) return "";
  const attempts = [
    ["describe", "--tags", "--abbrev=0", "--match", "v*", `${tag}^`],
    ["tag", "--list", "v*", "--merged", `${tag}^`, "--sort=-creatordate"],
  ];
  for (const args of attempts) {
    const first = gitOrEmpty(args).split("\n").map((line) => line.trim()).filter(Boolean)[0];
    if (first) return first;
  }
  return "";
}

function readCommits(range) {
  // ⚠️ 这里**故意用会抛错的 `git()`**，而不是 `gitOrEmpty`：range 里的 ref 写错
  // （tag 名打错、`fetch-depth: 1` 导致历史不完整）时，`git log` 会 exit 128，
  // 而吞掉错误就只剩「0 条提交」—— 正文会安静地变成「本次发布不含用户可见的
  // 功能变更」。那是**看着正常、实际错误**的结果。宁可走 main 的兜底分支，
  // 让 `::warning::` 把真实错因带出来。
  //
  // 合法的空区间（两个 ref 之间确实没有提交）`git log` 会 exit 0、输出为空 ——
  // 不需要额外区分，0 条提交本身就是那个分支。
  //
  // `%x1f` 分隔字段、`%x1e` 分隔记录：提交正文含换行与任意标点，
  // 只有非打印控制字符能安全做分隔（用 `|` / 空行分隔都会被正文内容撞上）。
  const raw = git(["log", "--no-merges", "--pretty=format:%H%x1f%s%x1f%b%x1e", range]);
  return raw
    .split("\x1e")
    .map((record) => record.replace(/^[\r\n]+/, ""))
    .filter(Boolean)
    .map((record) => {
      const [hash, subject = "", body = ""] = record.split("\x1f");
      return { hash: hash.trim(), subject: subject.trim(), body };
    });
}

function classify(commit) {
  const match = commit.subject.match(CONVENTIONAL);
  const breaking = /(^|\n)BREAKING[ -]CHANGE\s*:/.test(commit.body);
  if (!match) {
    return { hash: commit.hash, group: FALLBACK_TITLE, scope: "", text: commit.subject, breaking };
  }
  const [, type, scope = "", bang = "", text = ""] = match;
  const lowerType = type.toLowerCase();
  const group = GROUPS.find((candidate) => candidate.types.includes(lowerType));
  return {
    hash: commit.hash,
    group: group ? group.title : FALLBACK_TITLE,
    scope: scope.trim(),
    text: text.trim() || commit.subject,
    // 两种破坏性写法都认：type!: 与正文里的 BREAKING CHANGE:
    breaking: Boolean(bang) || breaking,
  };
}

function formatEntry(entry, repo) {
  const scope = entry.scope ? `**${entry.scope}**：` : "";
  const link = repo
    ? `（[${entry.hash.slice(0, 7)}](https://github.com/${repo}/commit/${entry.hash})）`
    : `（${entry.hash.slice(0, 7)}）`;
  return `- ${scope}${entry.text}${link}`;
}

function customNotes(tag) {
  if (!tag) return null;
  for (const name of [tag, tag.replace(/^v/, "")]) {
    const path = resolve(CUSTOM_NOTES_DIR, `${name}.md`);
    if (existsSync(path)) {
      /* ⚠️ 必须去掉**开头**的 HTML 注释块：那是写给维护者的编辑提示
         （「本文件存在时整篇采用、续行会被丢弃」），不是给安装用户看的。
         以前直接 `readFileSync().trim()` ⇒ 这段提示原样出现在 Release 页面上
         （2026-11-05 的 v2026.11.51155 就发出去过一版，事后 `gh release edit` 才修掉）。
         只剥开头：正文中间偶尔会有注释（如隐藏旧条目），那些保持原样。 */
      const body = stripLeadingHtmlComments(readFileSync(path, "utf8")).trim();
      if (body) return { path, body };
    }
  }
  return null;
}

/** 去掉文本**开头**连续出现的 `<!-- ... -->` 块（含块之间的空行）。 */
function stripLeadingHtmlComments(text) {
  let out = text.replace(/^\uFEFF/, "");
  for (;;) {
    const match = out.match(/^\s*<!--[\s\S]*?-->\s*/);
    if (!match) return out;
    out = out.slice(match[0].length);
  }
}

function render({ tag, prev, commits, repo }) {
  const groups = new Map();
  for (const commit of commits) {
    const entry = classify(commit);
    const key = entry.breaking ? BREAKING_TITLE : entry.group;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(entry);
  }

  const orderedTitles = [
    BREAKING_TITLE,
    ...GROUPS.map((group) => group.title),
    FALLBACK_TITLE,
  ].filter((title) => groups.has(title));

  const scopeLabel = prev ? `\`${prev}\` → \`${tag}\`` : `首个带 tag 的版本 \`${tag}\``;
  const header = [
    `<!-- 本正文由 scripts/gen-release-notes.mjs 自动生成（分组依据：提交的 Conventional Commits 前缀）。`,
    `     需要人工措辞时，新建 docs/releases/${tag || "vX.Y.Z"}.md 即可整篇覆盖，无需改脚本。 -->`,
    "",
    `> 对比范围：${scopeLabel} · 共 ${commits.length} 项变更`,
    "",
  ];

  const body = orderedTitles.length
    ? orderedTitles.flatMap((title) => [
        `### ${title}`,
        "",
        ...groups.get(title).map((entry) => formatEntry(entry, repo)),
        "",
      ])
    : ["本次发布不含用户可见的功能变更（仅版本号与构建产物更新）。", ""];

  const footer = [];
  // 对比链接用 GitHub 的**三点**形式 `A...B`（其文档形式，必然可用）；本仓历史是
  // 线性的，它与上面 `git log A..B`（两点）取到的提交集合**完全一致**。
  // 若将来改成带合并的分支流（历史分叉），两者会差出「只在 A 上、不在 B 上」的
  // 提交 —— 届时这里要重新验证，别默认仍然等价。
  if (repo && prev) {
    footer.push(
      "",
      "---",
      "",
      `**完整变更对比**：https://github.com/${repo}/compare/${prev}...${tag}`,
    );
  } else if (repo) {
    footer.push("", "---", "", `**提交历史**：https://github.com/${repo}/commits/${tag}`);
  }
  footer.push(
    "**安装包**：Windows（NSIS）· macOS（DMG，Apple Silicon / Intel）· Linux（deb / AppImage）；桌面端也可在应用内直接更新。",
    "**许可**：PolyForm Noncommercial 1.0.0 —— 源码公开（source-available）· 非商业使用。",
    "",
  );

  return [...header, ...body, ...footer].join("\n");
}

function fallbackBody({ tag, prev, repo, reason }) {
  const lines = [
    "<!-- 由 scripts/gen-release-notes.mjs 兜底生成：变更说明本次未能自动生成。 -->",
    "",
    "本次发布未附带自动生成的变更清单。",
    "",
    reason ? `> 生成失败原因：${reason}` : "",
  ].filter((line) => line !== "");
  if (repo) {
    const link = prev
      ? `https://github.com/${repo}/compare/${prev}...${tag}`
      : `https://github.com/${repo}/commits/${tag}`;
    lines.push("", "---", "", `**完整变更**：${link}`);
  }
  return `${lines.join("\n")}\n`;
}

/**
 * 本次发布的 tag。
 *
 * 优先级：`--tag` > 形如 `v2026.10.171833` 的 `GITHUB_REF_NAME` > **`src-tauri/Cargo.toml`
 * 的当前版本**。
 *
 * 为什么要读 Cargo：本地跑（`npm run notes:json`）时没有 tag，而 `GITHUB_REF_NAME`
 * 在非 tag 触发的 CI 里是**分支名**（`main`）—— 直接采信会产出一个 `version: "main"`
 * 的垃圾说明。版本载体只有 Cargo / package.json，它们已经被 `stamp-version` 统一过。
 */
function resolveTag(opts) {
  if (opts.tag) return opts.tag;
  const fromEnv = process.env.GITHUB_REF_NAME || "";
  if (/^v?\d{4}\.\d{1,2}\.\d{2}\d{4}$/.test(fromEnv.trim())) return fromEnv.trim();
  const cargo = resolve(REPO_ROOT, "src-tauri/Cargo.toml");
  if (existsSync(cargo)) {
    const match = readFileSync(cargo, "utf8").match(/^version\s*=\s*"([^"]+)"/m);
    if (match) return `v${match[1]}`;
  }
  return "";
}

function usage() {
  return [
    "usage: node scripts/gen-release-notes.mjs [--tag <tag>] [--from <ref>] [--out <file>] [--repo <owner/repo>] [--json <file>]",
    "",
    "  --tag   本次发布的 tag（默认 GITHUB_REF_NAME；不是版本形则回落 src-tauri/Cargo.toml 的版本）",
    "  --from  对比起点的 ref（默认自动找上一个 tag；找不到即为首次发布）",
    "  --out   输出文件（默认 release-notes.md；只传 --json 时**不写** Markdown）",
    "  --repo  GitHub 仓库坐标（默认 GITHUB_REPOSITORY，再回落 origin 远端）",
    "  --json  额外产出**结构化版本说明**（应用内「升级前弹窗」的数据源，见 docs/specs/UPDATE-NOTES-ARCH.md）",
  ].join("\n");
}

/* ══════════════════════ 结构化版本说明（--json）══════════════════════
 *
 * 为什么同一份「这一版改了什么」要出两种形态：
 *   - `.md`   → GitHub Release 正文（人读，允许 Markdown）；
 *   - `.json` → 应用内**升级前说明弹窗**（机器读，只能是结构化纯文本）。
 *
 * 前端**不解析 markdown**：版本说明来自发布资产（可被替换的外部输入），
 * 把 Markdown 转成 HTML 再渲染就是给自己开一个注入面。所以分组必须在这里做完。
 *
 * 三个来源，优先级从高到低：
 *   ① `docs/releases/<tag>.json` —— 人工写的结构化说明（要精细控制就用它）；
 *   ② `docs/releases/<tag>.md`   —— 人工措辞的正文，**按标题与列表解析**出分组；
 *   ③ 提交本身（Conventional Commits 前缀分组）—— 与 `.md` 正文同一套分组口径。
 *
 * `history` 由 `docs/releases/*.md` 全量解析而来（新→旧）：应用里「更早的 N 个版本」
 * 就是它，前端按「当前版本 < v ≤ 最新版本」切片（`lib/release-notes.ts` 的 `olderNotes`）。
 */

/** 嵌进 `.json` 前的清洗：去掉 Markdown 装饰，只留可读文本。 */
function plainText(text) {
  return String(text ?? "")
    /* 行内链接 [文案](url) → 文案（URL 对用户没意义，保留会把一行撑成两行）。 */
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    /* 只去掉**成对**的强调符：单个 `*` / `_` 是文件名字符 ——
       `api_gateway.json`、`pool_busy` 被吃成 `apigateway.json` 是实打实的数据损坏
       （本轮初版就踩了），所以这里绝不碰单个下划线 / 星号。 */
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/** 分组标题去掉 emoji 前缀（`🔧 网关：…` → `网关：…`）：中文界面里 emoji 只是装饰。 */
function stripEmoji(text) {
  return String(text ?? "")
    .replace(/[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{FE0F}]/gu, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * 版本号 → 发布时间。
 *
 * 本仓的版本号**自带本地构建时间**（`2026.10.171833` = 2026-10-17 18:33，北京时间），
 * 所以不查任何外部数据就能给出一个**准确**的发布时间。解析不出来就返回 `undefined`
 * —— 宁可不显示，也不编一个时间。
 */
function publishedAtFromVersion(version) {
  const m = String(version ?? "")
    .trim()
    .replace(/^v/, "")
    .match(/^(\d{4})\.(\d{1,2})\.(\d{2})(\d{2})(\d{2})$/);
  if (!m) return undefined;
  const [, y, mo, d, hh, mm] = m;
  const pad = (v) => String(v).padStart(2, "0");
  return `${y}-${pad(mo)}-${pad(d)}T${hh}:${mm}:00+08:00`;
}

/**
 * 解析人工撰写的发布说明（`docs/releases/<tag>.md`）。
 *
 * 只认两种行，其余（引用、代码块、分隔线之后的内容）一律跳过：
 *   - `## 标题` → 一个分组；标题含「必读 / 破坏性」⇒ 该组进 `breaking`；
 *   - `- 条目`  → 组内一条。
 *
 * 判据：人工稿是**人写给人**的，结构不会比这更复杂；猜多了反而会把正文吃进去。
 *
 * 两处的口径说明：
 *   ① **必读组一条 = 一个变更**：标题（「升级前必读：网关端口统一到 5789x」）就是这一项
 *      的名字，组内 bullet 是它的说明。硬拆成多条会把「一条变更的两个后果」说成两件事。
 *      标题里除了标签没有信息（只剩「升级前必读」）时，才退回按 bullet 拆。
 *   ② 章节标题里 `🔧` 这类 emoji 只做装饰，进 `.json` 前去掉（中文界面里它占宽度不传递信息）。
 */
function parseHumanNotes(markdown) {
  const breakingGroups = [];
  const sections = [];

  /** 一组必读 bullet：整组收成一个条目（见口径 ①）。 */
  const flushBreaking = (group) => {
    const bullets = (group.items ?? []).map((item) => String(item ?? "").trim()).filter(Boolean);
    if (!bullets.length) return;
    if (group.title) {
      /* 多条 bullet 合成一段说明：各自的 `。` 已经在句子里，直接相接即可
         （用「；」连接会拼出「。；」这种标点双打）。 */
      const desc = bullets.map(plainText).filter(Boolean).join("");
      breakingGroups.push(desc ? { title: group.title, desc } : { title: group.title });
      return;
    }
    for (const bullet of bullets) {
      breakingGroups.push(splitLead(bullet));
    }
  };

  let current = null;
  for (const rawLine of String(markdown ?? "").split(/\r?\n/)) {
    const line = rawLine.trim();
    /* 分隔线之后是「安装包 / 许可」这类发布元信息，不是变更内容。 */
    if (/^-{3,}$/.test(line)) break;
    if (!line || line.startsWith("<!--") || line.startsWith(">") || line.startsWith("```")) continue;

    if (line.startsWith("## ")) {
      if (current && current.kind === "breaking") flushBreaking(current);
      current = null;
      const title = stripEmoji(line.slice(3));
      if (!title) continue;
      if (/必读|破坏性/.test(title)) {
        /* 去掉标签本身，只留「这一项叫什么」；标题里的行内代码也要洗掉（`5789x` → 5789x）。 */
        const groupTitle = plainText(
          title
            .replace(/^(升级前必读|破坏性变更|破坏性改动)\s*[：:]?\s*/, "")
            .replace(/[：:]\s*$/, ""),
        );
        current = { kind: "breaking", title: groupTitle, items: [] };
        continue;
      }
      const section = { title: plainText(title).replace(/[：:]\s*$/, ""), items: [] };
      sections.push(section);
      current = { kind: "section", section };
      continue;
    }

    if (!current) continue;
    const bullet = line.match(/^[-*+]\s+(.*)$/);
    if (!bullet) continue;

    if (current.kind === "section") {
      const text = plainText(bullet[1]);
      if (text) current.section.items.push(text);
    } else {
      /* 保留原始标记：`splitLead` 要靠 `**加粗**` 判断哪一段是标题。 */
      current.items.push(bullet[1]);
    }
  }
  if (current && current.kind === "breaking") flushBreaking(current);

  return {
    breaking: breakingGroups.filter((item) => item.title),
    /* 空分组不产出（只有标题没有条目的分组在弹窗里比不显示更困惑）。 */
    sections: sections.filter((section) => section.items.length > 0),
  };
}

/**
 * 一条必读 bullet → `{ title, desc? }`。
 *
 * 三种写法都认：`**标题**：说明` / `标题：说明` / 整句（无冒号就别硬拆）。
 */
function splitLead(rawBullet) {
  const text = plainText(rawBullet);
  if (!text) return { title: "" };
  const bold = String(rawBullet).match(/^\s*\*\*([^*]+)\*\*\s*[：:]?\s*(.*)$/);
  if (bold) {
    const title = plainText(bold[1]);
    const desc = plainText(bold[2]);
    return desc ? { title, desc } : { title };
  }
  const colon = text.match(/^([^：。]{2,30})[：。]\s*(.+)$/);
  if (colon) return { title: colon[1], desc: colon[2] };
  return { title: text };
}

/** 提交 → 结构化分组（`.md` 缺失时的口径；与自动正文同一套 GROUPS）。 */
function notesFromCommits(commits) {
  const groups = new Map();
  const breaking = [];
  for (const commit of commits) {
    const entry = classify(commit);
    if (entry.breaking) {
      breaking.push({ title: entry.scope ? `${entry.scope}：${entry.text}` : entry.text });
      continue;
    }
    const title = stripEmoji(entry.group);
    if (!groups.has(title)) groups.set(title, []);
    groups.get(title).push(entry.scope ? `${entry.scope}：${entry.text}` : entry.text);
  }
  return {
    breaking,
    sections: [...groups.entries()].map(([title, items]) => ({ title, items })),
  };
}

/**
 * 历史版本（新→旧）。来源是 `docs/releases/*.md` 人工稿 —— **只认人工稿**：
 * 自动按提交分组的历史需要完整 git 历史与旧 tag，CI 里拿不到，硬凑只会产出错内容。
 *
 * 只收**比本次版本更早**的：仓库里可能存在已写好、但还没构建的「未来版本」说明
 * （发版期间就是这个状态），把它当历史塞进去会变成「更早的版本」里列着一个更高版本。
 */
function historyFromDocs(currentTag) {
  const dir = CUSTOM_NOTES_DIR;
  if (!existsSync(dir)) return [];
  const current = String(currentTag ?? "").replace(/^v/, "");
  const entries = [];
  for (const file of readdirSync(dir)) {
    const match = file.match(/^v?(.+)\.(md|json)$/);
    if (!match) continue;
    const version = match[1];
    if (!version || version === current) continue;
    if (!/^\d{4}\.\d{1,2}\.\d{2}\d{4}$/.test(version)) continue;
    if (match[2] === "json") continue;
    if (compareVersionStrings(version, current) >= 0) continue;
    const parsed = parseHumanNotes(readFileSync(resolve(dir, file), "utf8"));
    const sections = parsed.sections.length
      ? parsed.sections
      : parsed.breaking.map((item) => ({ title: "升级前必读", items: [item.title] }));
    if (!sections.length) continue;
    entries.push({
      version,
      publishedAt: publishedAtFromVersion(version),
      sections,
    });
  }
  /* 版本号自带时间，同格式下按段数值比较即可 = 时间序。 */
  return entries.sort((a, b) => compareVersionStrings(b.version, a.version));
}

/** 版本号逐段数值比较（与前端 `lib/release-notes.ts::compareVersions` 同一口径）。 */
function compareVersionStrings(a, b) {
  const parse = (value) =>
    String(value ?? "")
      .replace(/^v/, "")
      .split(".")
      .map((part) => Number.parseInt(part, 10) || 0);
  const left = parse(a);
  const right = parse(b);
  for (let i = 0; i < Math.max(left.length, right.length); i += 1) {
    const x = left[i] ?? 0;
    const y = right[i] ?? 0;
    if (x !== y) return x > y ? 1 : -1;
  }
  return 0;
}

function buildReleaseNotes({ tag, commits, customMarkdown }) {
  const version = String(tag ?? "").replace(/^v/, "");
  if (!version) throw new Error("缺少 --tag，无法产出结构化版本说明");

  /* ① 人工结构化稿 > ② 人工正文解析 > ③ 提交分组。 */
  const jsonPath = resolve(CUSTOM_NOTES_DIR, `${version}.json`);
  let notes;
  if (existsSync(jsonPath)) {
    const raw = JSON.parse(readFileSync(jsonPath, "utf8"));
    notes = { breaking: raw.breaking ?? [], sections: raw.sections ?? [] };
  } else if (customMarkdown) {
    notes = parseHumanNotes(customMarkdown);
  } else {
    notes = notesFromCommits(commits ?? []);
  }

  const publishedAt = publishedAtFromVersion(version);
  return {
    version,
    ...(publishedAt ? { publishedAt } : {}),
    ...(notes.breaking.length ? { breaking: notes.breaking } : {}),
    ...(notes.sections.length ? { sections: notes.sections } : {}),
    history: historyFromDocs(version),
  };
}

/**
 * 写 `<--json>` 产物。**失败不阻断发布**（与 `.md` 同一条判据：说明缺失是降级，
 * 发布失败才是事故），但要留 `::warning::` 让人看得见。
 */
function writeNotesJson(opts, source) {
  try {
    const payload = buildReleaseNotes(source);
    if (!payload.sections.length && !payload.breaking.length) {
      console.log("::warning title=结构化版本说明为空::本次没有可用的分组内容（人工稿与提交都没给出）");
    }
    const outPath = resolve(REPO_ROOT, opts.json);
    mkdirSync(dirname(outPath), { recursive: true });
    writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
    /* 计数必须带 `?? []`：`breaking` / `sections` 为空时**键根本不产出**
       （空数组在产物里没有意义），直接 `.length` 会抛「undefined 的 length」——
       本轮初版就因此把「写成功」报成了「生成失败」。 */
    console.log(
      `gen-release-notes: 已写入 ${outPath}（${(payload.sections ?? []).length} 组 / ` +
        `${(payload.breaking ?? []).length} 条必读 / ${(payload.history ?? []).length} 个历史版本）`,
    );
    return 0;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    console.log(`::warning title=结构化版本说明生成失败::${reason}`);
    return 0;
  }
}


function main(argv) {
  const opts = parseArgs(argv);
  if (opts.help) {
    console.log(usage());
    return 0;
  }

  const tag = resolveTag(opts);
  const repo = resolveRepo(opts);
  /* 只传 `--json` 时不写 Markdown：本地生成「内置兜底说明」不该在仓库根目录留一份
     `release-notes.md`（CI 里由 Release 正文那一步显式 `--out`，行为不变）。 */
  const outPath = opts.out ? resolve(REPO_ROOT, opts.out) : opts.json ? null : resolve(REPO_ROOT, "release-notes.md");

  const write = (body) => {
    if (!outPath) return;
    writeFileSync(outPath, body.endsWith("\n") ? body : `${body}\n`, "utf8");
    console.log(`gen-release-notes: 已写入 ${outPath}`);
  };

  // 提到 try 外面：兜底分支也要用它拼 compare 链接，否则「自动找上一个 tag」时
  // 兜底正文会丢掉对比链接（`opts.from` 为空）。
  let prev = opts.from !== undefined ? opts.from : "";

  try {
    const custom = customNotes(tag);
    if (custom) {
      write(custom.body);
      console.log(`gen-release-notes: 使用人工撰写的 ${custom.path}（未自动拼装）`);
      if (opts.json) writeNotesJson(opts, { tag, commits: [], customMarkdown: custom.body });
      return 0;
    }

    prev = opts.from !== undefined ? opts.from : previousTag(tag);
    const range = prev ? `${prev}..${tag || "HEAD"}` : tag || "HEAD";
    const commits = readCommits(range);
    console.log(
      `gen-release-notes: 范围 ${range}，解析出 ${commits.length} 条提交` +
        (prev ? "" : "（无上一个 tag，按首次发布处理）"),
    );
    write(render({ tag, prev, commits, repo }));
    if (opts.json) writeNotesJson(opts, { tag, commits });
    return 0;
  } catch (error) {
    // 见头部注释第 3 条：降级，不阻断发布。
    const reason = error instanceof Error ? error.message : String(error);
    // ⚠️ 兜底写入**必须**自己再包一层。失败点就在 `write` 里时（输出路径不可写 /
    // 磁盘满 / 路径不存在），在 catch 里直接调 `write` 等于**原地再抛一次**，而这次
    // 没有任何人接 ⇒ 变成未捕获异常，`process.exit(main(...))` 根本不会执行，
    // node 以 **1** 退出并打出一整段栈 —— 与头部注释第 3 条「退出码仍为 0」**正好相反**。
    // 更坑的是 `::warning::` 已经先打出去了，CI 日志看起来像「降级成功」，实际那一步是红的。
    try {
      write(fallbackBody({ tag, prev, repo, reason }));
    } catch (fallbackError) {
      // 走到这里说明 `--out` 一定没落地。CI 里 `body_path` 指向不存在的文件，
      // 发布步骤**无论如何**都会失败 ⇒ 此时「响亮失败 + 说清原因」远好过一段栈。
      // 注意：这**不是**第 3 条要避免的那种「把降级放大成事故」—— 降级已经不可能了。
      const detail =
        fallbackError instanceof Error ? fallbackError.message : String(fallbackError);
      console.log(
        `::error title=版本变更说明无法写入::${reason}；兜底写入同样失败：${detail}`,
      );
      return 1;
    }
    console.log(`::warning title=版本变更说明生成失败::${reason}`);
    return 0;
  }
}

process.exit(main(process.argv.slice(2)));
