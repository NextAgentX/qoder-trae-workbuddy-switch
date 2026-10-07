/* ==========================================================================
   Buddy Switch 原型 — 模拟数据
   形状刻意贴近真实后端契约（account / gateway / usage），方便你对照现有字段。
   ========================================================================== */

/* ─────────── 图标 ─────────── */
const MARK = {
  workbuddy: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.6 20.5 12 12 21.4 3.5 12Z"/><path d="M12 7.6 16.6 12 12 16.4 7.4 12Z"/></svg>`,
  trae: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.8 21.2 20H2.8Z"/><path d="M12 9.6 16.4 17H7.6Z"/></svg>`,
};

const ICON = {
  overview: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="9" rx="1.5"/><rect x="14" y="3" width="7" height="5" rx="1.5"/><rect x="14" y="12" width="7" height="9" rx="1.5"/><rect x="3" y="16" width="7" height="5" rx="1.5"/></svg>`,
  accounts: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>`,
  usage: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v18h18"/><path d="m7 15 4-5 4 3 5-7"/></svg>`,
  gateway: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="7" rx="2"/><rect x="2" y="13" width="20" height="7" rx="2"/><path d="M6 7.5h.01M6 16.5h.01"/></svg>`,
  settings: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1"/></svg>`,
  plus: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>`,
  more: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="5" cy="12" r="1.4"/><circle cx="12" cy="12" r="1.4"/><circle cx="19" cy="12" r="1.4"/></svg>`,
  search: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>`,
  check: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="m5 13 4 4L19 7"/></svg>`,
  minus: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M5 12h14"/></svg>`,
  refresh: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-2.64-6.36"/><path d="M21 3v6h-6"/></svg>`,
  copy: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`,
  trash: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>`,
  warn: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4M12 17h.01"/></svg>`,
  clock: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>`,
  spark: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v4M12 17v4M3 12h4M17 12h4"/><path d="m6.3 6.3 2.8 2.8M14.9 14.9l2.8 2.8M6.3 17.7l2.8-2.8M14.9 9.1l2.8-2.8"/></svg>`,
  key: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="7.5" cy="15.5" r="4.5"/><path d="m11 12 8-8 3 3-2 2 2 2-3 3-2-2-2 2"/></svg>`,
  pool: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6"/><path d="M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/></svg>`,
  list: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/></svg>`,
  grid: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>`,
  arrowRight: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>`,
  download: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12M7 11l5 5 5-5M4 21h16"/></svg>`,
  upload: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 21V9M7 13l5-5 5 5M4 3h16"/></svg>`,
  login: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><path d="M10 17l5-5-5-5M15 12H3"/></svg>`,
  shield: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"/></svg>`,
  calendar: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M8 3v4M16 3v4M3 11h18"/></svg>`,
  wand: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 21 9-9M14 4l1 2 2 1-2 1-1 2-1-2-2-1 2-1Z"/><path d="M18 12l.7 1.4 1.4.7-1.4.7-.7 1.4-.7-1.4-1.4-.7 1.4-.7Z"/></svg>`,
  sessions: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="5" r="2.5"/><circle cx="18" cy="12" r="2.5"/><circle cx="6" cy="19" r="2.5"/><path d="M6 7.5v9M8.5 5h4a3 3 0 0 1 3 3v1.5"/></svg>`,
};

/* ─────────── 平台注册表（v2 的核心机制） ───────────
   新增平台 = 这里加一条 + registry 里加一行。页面组件零改动。 */
const PLATFORMS = {
  workbuddy: {
    id: "workbuddy",
    label: "WorkBuddy",
    mark: MARK.workbuddy,
    accent: "oklch(0.696 0.17 162.48)",
    regions: [
      { id: "cn", label: "国内版", short: "CN" },
      { id: "global", label: "国际版", short: "Global" },
    ],
    /* 程序位：账号落到哪个客户端。注意 installed 是「客户端是否存在」，
       与「该账号是否是它的当前登录」是两件事。

       ⚠️ `planned: true` = **本仓尚未支持、计划新增**的客户端（§3.3 B 表）。
       它与 `installed: false` 是**两个不同维度**：
         - `installed: false` = 本仓支持，但本机没装（可引导用户去装）
         - `planned: true`    = 本仓**还不支持**（引导用户去装也没用，只能等实现）
       渲染上必须区分，否则用户会点了没用还以为是自己的问题。 */
    programs: [
      { id: "workbuddy", label: "WorkBuddy", short: "WB", installed: true },
      { id: "ide", label: "CodeBuddy IDE", short: "IDE", installed: true },
      { id: "cli", label: "CodeBuddy CLI", short: "CLI", installed: false },
      { id: "vscode", label: "VS Code 插件", short: "VSC", installed: false, planned: true },
    ],
    /* sessionCopy = 会话复制能力。**只有 WorkBuddy 有** —— 它依赖 WB 特有的数据三件套
       （projects jsonl + workbuddy.db + edge-sync-mapping 云端归属），Trae 侧没有等价物。
       这是**平台固有**能力（§2.7），不是用户开关。 */
    capabilities: ["checkin", "travel", "credits", "oauth", "gateway", "autoRotate", "sessionCopy"],
  },
  trae: {
    id: "trae",
    label: "TraeWork",
    mark: MARK.trae,
    accent: "oklch(0.72 0.14 278)",
    regions: [
      { id: "cn", label: "国内版", short: "CN" },
      { id: "global", label: "国际版", short: "Global" },
    ],
    programs: [
      { id: "trae_work", label: "TraeWork", short: "Work", installed: true },
      { id: "trae_code", label: "TraeCode", short: "Code", installed: true },
    ],
    capabilities: ["checkin", "credits", "oauth", "snapshot", "group", "gateway"],
  },
};

/* 目的地：两个平台共用同一套（这就是 5:5 同构的价值）。
   `cap` = 该项**需要平台具备某个能力**才渲染 —— 「会话」需要 `sessionCopy`，
   而它只有 WorkBuddy 有（§3.15 能力边界）⇒ Trae 作用域下不出现这个入口。 */
const DESTINATIONS = [
  { id: "overview", label: "概览", icon: ICON.overview },
  { id: "accounts", label: "账号", icon: ICON.accounts },
  { id: "usage", label: "用量", icon: ICON.usage },
  { id: "sessions", label: "会话", icon: ICON.sessions, cap: "sessionCopy" },
  { id: "gateway", label: "网关", icon: ICON.gateway },
  { id: "settings", label: "设置", icon: ICON.settings },
];

/* ─────────── 作用域 = 平台 × 区域 ─────────── */
/* ─────────── 会话树 / 资源包 / 签到历史 ───────────
   用生成器保持数据紧凑：每个账号一组三层会话树（类型 → 目录 → 会话），
   一组资源包明细，以及近 7 天签到历史。种子取自账号 id，保证稳定可复现。 */
const SESSION_SEEDS = {
  types: ["对话", "工作区", "claw"],
  /* 目录名「像文件夹」，会话名「像任务标题」——两个池不共用字符串，
     否则会出现「目录『X』里装着会话『X』」。 */
  dirs: ["tauri-cmd-refactor", "spa-deeplink", "gateway-keys", "ci-matrix",
    "account-pool", "snapshot-design", "session-tree", "perm-guide",
    "credit-rotation", "oauth-callback", "tray-autostart", "settings-flags"],
  sessions: ["重构 Tauri 命令面", "修复 SPA 深链 MIME", "网关 Key 生命周期", "CI 四平台矩阵",
    "账号池选号策略", "登录态快照设计", "会话树三层选择", "权限提升引导",
    "积分轮换策略", "OAuth 回调排查", "托盘与自启", "设置页能力开关",
    "深链即唯一真值", "能力开关驱动 UI", "统一错误态", "批量部分失败汇总"],
};
function _rng(seedStr) {
  let h = 0; for (const ch of seedStr) h = (h * 31 + ch.charCodeAt(0)) % 997;
  return () => { h = (h * 1103515245 + 12345) & 0x7fffffff; return h; };
}
function _pick(arr, n, rnd) {
  const used = new Set(), out = [];
  while (out.length < n && used.size < arr.length) { const i = rnd() % arr.length; if (!used.has(i)) { used.add(i); out.push(arr[i]); } }
  return out;
}
function makeSessionTree(seedStr) {
  const rnd = _rng(seedStr);
  return SESSION_SEEDS.types.map((type) => ({
    type,
    dirs: _pick(SESSION_SEEDS.dirs, 2 + (rnd() % 2), rnd).map((name) => ({
      name,
      sessions: _pick(SESSION_SEEDS.sessions, 2 + (rnd() % 3), rnd).map((title) => ({ title, msgs: 6 + (rnd() % 42) })),
    })),
  }));
}
function makePacks(seedStr, credits, creditsTotal) {
  const rnd = _rng(seedStr + "p");
  const n = 2 + (rnd() % 3);
  /* 包名先洗牌再取前 n 个，保证同一账号内不出现重名资源包。 */
  const names = _pick(["新手礼包", "月度订阅", "活动赠送", "团队共享", "年度包"], n, rnd);
  const out = [];
  let remLeft = credits, totLeft = creditsTotal;
  for (let i = 0; i < n; i++) {
    const last = i === n - 1;
    const tot = last ? Math.max(0, totLeft) : Math.round(creditsTotal / n);
    const rem = last ? Math.max(0, remLeft) : Math.min(tot, Math.round(credits / n));
    totLeft -= tot; remLeft -= rem;
    out.push({ name: names[i], remaining: rem, total: tot, expireInDays: 2 + (rnd() % 60) });
  }
  return out;
}
function makeCheckinHistory(seedStr) {
  const rnd = _rng(seedStr + "c");
  const today = new Date();
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(today.getTime() - i * 86400000);
    return { date: `${d.getMonth() + 1}/${d.getDate()}`, ok: rnd() % 7 !== 0 };
  });
}

function makeAccount(id, name, email, o) {
  const a = Object.assign({
    id, name, email, remark: "", group: "默认",
    credits: 1200, creditsTotal: 2000, expiring: 0, expireInDays: 30,
    checkedIn: true, expired: false, needsRelogin: false, priority: false,
    programs: {}, lastUsed: "刚刚", lastUsedTs: Date.now(),
    todayTokens: 0, totalTokens: 0, status: "ok",
    snapshots: [],
  }, o || {});
  a.sessionTree = a.sessionTree || makeSessionTree(id);
  a.packs = a.packs || makePacks(id, a.credits, a.creditsTotal);
  a.checkinHistory = a.checkinHistory || makeCheckinHistory(id);
  return a;
}

const SCOPES = {
  "wb-cn": {
    id: "wb-cn", platform: "workbuddy", region: "cn",
    running: true, installed: true, version: "2.14.3",
    todayCredits: 46, gatewayRequests: 312, gatewayKeys: 3, checkinReward: 100,
    accounts: [
      makeAccount("a1", "jackey", "jackey@example.com", {
        remark: "主号 · 长期", credits: 1240, creditsTotal: 2000, expiring: 320, expireInDays: 3,
        programs: { workbuddy: true, ide: true, cli: false }, priority: true,
        lastUsed: "2 分钟前", todayTokens: 42100, totalTokens: 862400,
      }),
      makeAccount("a2", "team-shared", "shared@example.com", {
        remark: "团队共用", credits: 860, creditsTotal: 1000, expiring: 0, expireInDays: 21,
        programs: { workbuddy: false, ide: true, cli: false },
        lastUsed: "1 小时前", todayTokens: 28400, totalTokens: 512000,
      }),
      makeAccount("a3", "backup-01", "backup01@example.com", {
        remark: "备用", credits: 1580, creditsTotal: 2000, expireInDays: 44,
        programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "3 天前", todayTokens: 0, totalTokens: 96400,
      }),
      makeAccount("a4", "qa-bot", "qa@example.com", {
        remark: "自动化测试", credits: 120, creditsTotal: 2000, expiring: 120, expireInDays: 2,
        checkedIn: false, programs: { workbuddy: false, ide: true, cli: false },
        lastUsed: "5 分钟前", todayTokens: 31200, totalTokens: 288100,
      }),
      makeAccount("a5", "design-lab", "design@example.com", {
        remark: "", credits: 430, creditsTotal: 1000, expireInDays: 12,
        checkedIn: false, expired: true, programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "8 天前", todayTokens: 0, totalTokens: 41200, status: "expired",
      }),
      makeAccount("a6", "ops-02", "ops02@example.com", {
        remark: "运维", credits: 990, creditsTotal: 2000, expireInDays: 27,
        programs: { workbuddy: false, ide: true, cli: false },
        lastUsed: "昨天", todayTokens: 8700, totalTokens: 173000,
      }),
      makeAccount("a7", "spare-global", "spare@example.com", {
        remark: "国际版备用", credits: 2050, creditsTotal: 3000, expireInDays: 58,
        needsRelogin: true, programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "12 天前", todayTokens: 0, totalTokens: 55000, status: "relogin",
      }),
      makeAccount("a8", "demo", "demo@example.com", {
        remark: "演示用", credits: 640, creditsTotal: 1000, expireInDays: 9,
        programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "4 小时前", todayTokens: 18300, totalTokens: 121900,
      }),
    ],
  },
  "wb-global": {
    id: "wb-global", platform: "workbuddy", region: "global",
    running: false, installed: true, version: "2.14.3",
    todayCredits: 7, gatewayRequests: 26, gatewayKeys: 1, checkinReward: 100,
    accounts: [
      makeAccount("g1", "jackey-global", "jackey@global.example.com", {
        remark: "国际版主号", credits: 720, creditsTotal: 1000, expiring: 180, expireInDays: 4,
        programs: { workbuddy: true, ide: false, cli: false }, priority: true,
        lastUsed: "1 天前", todayTokens: 14200, totalTokens: 88300,
      }),
      makeAccount("g2", "travel-acc", "travel@global.example.com", {
        remark: "", credits: 260, creditsTotal: 1000, expireInDays: 18,
        checkedIn: false, programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "6 天前", todayTokens: 4200, totalTokens: 31200,
      }),
    ],
  },
  "trae-cn": {
    id: "trae-cn", platform: "trae", region: "cn",
    running: true, installed: true, version: "1.8.2",
    todayCredits: 31, gatewayRequests: 148, gatewayKeys: 2, checkinReward: 50,
    accounts: [
      makeAccount("t1", "jackey", "jackey@example.com", {
        remark: "TraeWork 主号", credits: 880, creditsTotal: 1500, expiring: 140, expireInDays: 5,
        programs: { trae_work: true, trae_code: true }, priority: true, group: "常用",
        lastUsed: "刚刚", todayTokens: 51400, totalTokens: 402100,
        snapshots: [{ ts: "2026-09-28 14:20", size: "18.4 MB" }, { ts: "2026-09-21 09:05", size: "17.9 MB" }],
      }),
      makeAccount("t2", "code-lab", "codelab@example.com", {
        remark: "TraeCode 专用", credits: 1320, creditsTotal: 1500, expireInDays: 33,
        programs: { trae_work: false, trae_code: true }, group: "常用",
        lastUsed: "20 分钟前", todayTokens: 26800, totalTokens: 296400,
      }),
      makeAccount("t3", "solo-test", "solo@example.com", {
        remark: "TRAE SOLO 测试", credits: 90, creditsTotal: 500, expiring: 90, expireInDays: 1,
        checkedIn: false, programs: { trae_work: false, trae_code: false }, group: "测试",
        lastUsed: "2 天前", todayTokens: 12000, totalTokens: 74800,
      }),
      makeAccount("t4", "idle-01", "idle01@example.com", {
        remark: "", credits: 480, creditsTotal: 500, expireInDays: 15,
        programs: { trae_work: false, trae_code: false }, group: "闲置",
        lastUsed: "11 天前", todayTokens: 0, totalTokens: 22100,
      }),
      makeAccount("t5", "snap-acc", "snap@example.com", {
        remark: "有登录态快照", credits: 700, creditsTotal: 1500, expireInDays: 25,
        programs: { trae_work: false, trae_code: false }, group: "闲置",
        lastUsed: "5 天前", todayTokens: 6000, totalTokens: 130500,
        snapshots: [{ ts: "2026-09-30 10:12", size: "22.1 MB" }, { ts: "2026-09-24 16:40", size: "21.7 MB" }, { ts: "2026-09-15 08:30", size: "20.9 MB" }],
      }),
    ],
  },
  "trae-global": {
    id: "trae-global", platform: "trae", region: "global",
    running: false, installed: false, version: "1.8.2",
    todayCredits: 0, gatewayRequests: 0, gatewayKeys: 0, checkinReward: 50,
    accounts: [],
  },
};

/* ─────────── 用量数据 ─────────── */
/* lastValue：最后一天（今天）的真实值。传入时用它，而不是 base*1.12——
   保证「概览统计块 = 趋势图末柱 = 账号卡今日 Token 合计」三者一致。 */
function series(days, base, seed, lastValue) {
  const out = [];
  const today = new Date();
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(today.getTime() - i * 86400000);
    const wobble = Math.sin((i + seed) * 1.7) * 0.35 + Math.cos((i + seed) * 0.6) * 0.22;
    const v = Math.max(0, Math.round(base * (1 + wobble)));
    out.push({
      label: `${d.getMonth() + 1}/${d.getDate()}`,
      value: i === 0 ? (lastValue == null ? Math.round(base * 1.12) : lastValue) : v,
    });
  }
  return out;
}

/* 积分流水：`when` 只用于显示；`ts` 是真实时间戳（按分钟偏移），排序只看 ts——
   否则「今天 / 昨天 / 3 天前」按中文字符串排会乱序。 */
const _nowTs = Date.now();
function credit(when, minsAgo, account, delta, note) {
  return { when, ts: _nowTs - minsAgo * 60000, account, delta, note };
}

/* ─────────── 用量派生数据 ───────────
   全部**从已有数据推导**（days / models / sessions / accounts / gatewayRequests），
   **绝不手写新数字**——否则又会冒出「同一件事两处数值打架」那类不自洽。
   推导结果挂在 `USAGE[scope].ext`，由文件末尾统一调用。

   一条硬约束：**堆叠柱各分段之和必须精确等于当日总量**（最后一段吃掉舍入余数），
   否则「堆叠柱总高」与「单色柱高度」会对不上，是能被一眼看穿的假数据。 */
function buildUsageExtras(scopeId) {
  const u = USAGE[scopeId];
  const scope = SCOPES[scopeId];

  /* 1. 每个模型的每日序列（堆叠柱用） */
  const weights = u.models.map((m) => m.tokens);
  const modelDays = u.models.map((m) => ({ name: m.name, values: [] }));
  u.days.forEach((d, i) => {
    const w = weights.map((t, k) => t * (0.78 + ((i * 37 + k * 53) % 44) / 100));
    const wsum = w.reduce((a, b) => a + b, 0) || 1;
    let acc = 0;
    u.models.forEach((m, k) => {
      const v = k === u.models.length - 1 ? Math.max(0, d.value - acc) : Math.round((d.value * w[k]) / wsum);
      acc += v;
      modelDays[k].values.push(Math.max(0, v));
    });
  });

  /* 2. 时段热力图：7 天(周一..周日) × 24 小时，取**相对强度 0–100**。
        刻意不声称是绝对 Token，避免与 days 的口径冲突。 */
  const hourly = Array.from({ length: 7 }, (_, wd) =>
    Array.from({ length: 24 }, (_, h) => {
      const weekend = wd >= 5 ? 0.42 : 1;
      const shape = h < 7 ? 0.05 : h < 9 ? 0.42 : h < 12 ? 1 : h < 14 ? 0.66 : h < 19 ? 0.94 : h < 22 ? 0.46 : 0.12;
      return Math.round(shape * weekend * (72 + ((h * 13 + wd * 29) % 28)));
    })
  );

  /* 3. 每日积分收支。入账用**该作用域真实的签到奖励**（WB=100 / Trae=50），别硬编码。 */
  const reward = scope.checkinReward || 100;
  const creditDaily = u.days.map((d, i) => ({
    label: d.label,
    inAmt: i % 7 === 6 ? 0 : reward,      // 周日不签到
    outAmt: Math.round(d.value / 2600),   // 由 Token 量推导消耗
  }));

  /* 4. 积分余额曲线：以「当前余额合计」为**终值**往前回推 ⇒ 末值必然对得上统计块。 */
  const curTotal = scope.accounts.reduce((s, a) => s + a.credits, 0);
  const net = creditDaily.map((c) => c.inAmt - c.outAmt);
  let bal = curTotal - net.reduce((a, b) => a + b, 0);
  const balance = net.map((v, i) => { bal += v; return { label: creditDaily[i].label, value: Math.max(0, bal) }; });

  /* 5. 到期分布：由账号的 expiring / expireInDays 聚合，与「即将过期」统计块**同源**。 */
  const expiry = Array.from({ length: 30 }, (_, i) => ({ day: i + 1, value: 0 }));
  scope.accounts.forEach((a) => {
    if (a.expiring > 0 && a.expireInDays >= 1 && a.expireInDays <= 30) expiry[a.expireInDays - 1].value += a.expiring;
  });

  /* 6. 账号 Token 占比（环形图） */
  const accountShare = scope.accounts
    .filter((a) => a.totalTokens > 0)
    .map((a) => ({ name: a.name, value: a.totalTokens }))
    .sort((x, y) => y.value - x.value);

  /* 7. 网关状态码分布：由 gatewayRequests 推导，与概览的「网关请求」**同源**。 */
  const rq = scope.gatewayRequests || 0;
  const statusCodes = rq
    ? [
        { name: "200 成功", value: Math.round(rq * 0.94) },
        { name: "429 限流", value: Math.round(rq * 0.042) },
        { name: "5xx 上游错误", value: Math.max(1, Math.round(rq * 0.018)) },
      ]
    : [];

  /* 8. 会话散点：x = 消息数，y = Token（气泡大小随 Token） */
  const scatter = u.sessions.map((s) => ({ x: s.msgs, y: s.tokens, label: s.title }));

  return { modelDays, hourly, creditDaily, balance, expiry, accountShare, statusCodes, scatter, curTotal };
}

const USAGE = {
  "wb-cn": {
    days: series(30, 96000, 1, 128700),
    models: [
      { name: "claude-sonnet-4.5", tokens: 412000, pct: 41 },
      { name: "claude-opus-4.1", tokens: 268000, pct: 27 },
      { name: "gpt-5-codex", tokens: 173000, pct: 17 },
      { name: "gemini-2.5-pro", tokens: 96000, pct: 10 },
      { name: "其他", tokens: 51000, pct: 5 },
    ],
    sessions: [
      { title: "重构 Tauri 命令面", account: "jackey", tokens: 84200, msgs: 46, when: "今天 09:12" },
      { title: "修复 SPA 深链 MIME", account: "jackey", tokens: 51300, msgs: 31, when: "今天 08:40" },
      { title: "网关 Key 生命周期", account: "team-shared", tokens: 38700, msgs: 22, when: "昨天 21:05" },
      { title: "CI 四平台矩阵", account: "qa-bot", tokens: 29400, msgs: 18, when: "昨天 17:22" },
      { title: "账号池选号策略", account: "jackey", tokens: 21800, msgs: 14, when: "昨天 11:03" },
    ],
    credits: [
      credit("今天 10:24", 30, "jackey", -46, "对话消耗"),
      credit("今天 08:00", 180, "qa-bot", +100, "每日签到"),
      credit("昨天 22:11", 720, "team-shared", -128, "对话消耗"),
      credit("昨天 08:00", 1600, "jackey", +100, "每日签到"),
      credit("昨天 08:00", 1601, "team-shared", +100, "每日签到"),
    ],
  },
  "wb-global": {
    days: series(30, 12000, 3, 18400),
    models: [
      { name: "claude-sonnet-4.5", tokens: 62000, pct: 52 },
      { name: "gpt-5-codex", tokens: 34000, pct: 29 },
      { name: "其他", tokens: 23000, pct: 19 },
    ],
    sessions: [
      { title: "国际版端点评测", account: "jackey-global", tokens: 18400, msgs: 11, when: "昨天 14:30" },
      { title: "区域端点差异", account: "jackey-global", tokens: 9200, msgs: 7, when: "3 天前" },
    ],
    credits: [
      credit("今天 09:00", 120, "jackey-global", -7, "对话消耗"),
      credit("昨天 08:00", 1620, "jackey-global", +100, "每日签到"),
    ],
  },
  "trae-cn": {
    days: series(30, 74000, 5, 96200),
    models: [
      { name: "chat_v3", tokens: 288000, pct: 46 },
      { name: "solo_work_lite", tokens: 194000, pct: 31 },
      { name: "trae-code-pro", tokens: 121000, pct: 19 },
      { name: "其他", tokens: 25000, pct: 4 },
    ],
    sessions: [
      { title: "TraeCode 会话恢复", account: "code-lab", tokens: 46200, msgs: 28, when: "今天 10:02" },
      { title: "TraeWork 模型清单", account: "jackey", tokens: 33100, msgs: 19, when: "今天 09:15" },
      { title: "SOLO 积分规则", account: "solo-test", tokens: 12000, msgs: 8, when: "2 天前" },
    ],
    credits: [
      credit("今天 10:30", 24, "code-lab", -31, "对话消耗"),
      credit("今天 08:00", 180, "jackey", +50, "每日签到"),
      credit("昨天 22:40", 700, "jackey", -88, "对话消耗"),
    ],
  },
  "trae-global": { days: series(30, 0, 7, 0), models: [], sessions: [], credits: [] },
};

/* ─────────── 网关数据 ─────────── */
const GATEWAY = {
  "wb-cn": {
    enabled: true, port: 57890, baseUrl: "http://127.0.0.1:57890",
    keys: [
      { id: "k1", name: "默认 Key", masked: "bs_live_9f3a••••••••••••7c21", scope: "国内版", created: "2026-09-12", lastUsed: "2 分钟前", requests: 1842 },
      { id: "k2", name: "CI 自动化", masked: "bs_live_4d18••••••••••••a0e9", scope: "国内版", created: "2026-09-20", lastUsed: "1 小时前", requests: 613 },
      { id: "k3", name: "临时调试", masked: "bs_live_c702••••••••••••31bf", scope: "国际版", created: "2026-09-28", lastUsed: "3 天前", requests: 47 },
    ],
    /* available = 上游事实（能不能调）；disabled = 用户偏好（主动拿掉）。
       两者**正交**，四种组合都合法 —— UI 必须能区分「已禁用」与「上游不可用」。 */
    models: [
      { name: "claude-sonnet-4.5", provider: "anthropic", scope: "国内版", available: true },
      { name: "claude-opus-4.1", provider: "anthropic", scope: "国内版", available: true, disabled: true },
      { name: "gpt-5-codex", provider: "openai", scope: "国内版", available: true },
      { name: "gemini-2.5-pro", provider: "openai", scope: "国内版", available: true },
      { name: "claude-sonnet-4.5", provider: "anthropic", scope: "国际版", available: true },
      { name: "gpt-5-codex", provider: "openai", scope: "国际版", available: false, disabled: true },
    ],
    /* 模型清单的来源（§3.17）：实时 / 已保存 / 内置 —— 三级都必须显式标注，
       不许把「内置」伪装成「实时」（用户照着清单调用会扑空）。 */
    catalog: { source: "live", fetchedAt: "12 分钟前", fetchedAbs: "2026-10-03 12:38", note: null },
    pool: [
      { account: "jackey", credits: 1240, weight: 100, state: "可用", sticky: true },
      { account: "team-shared", credits: 860, weight: 80, state: "可用", sticky: false },
      { account: "qa-bot", credits: 120, weight: 20, state: "余额偏低", sticky: false },
      { account: "design-lab", credits: 0, weight: 0, state: "已排除", sticky: false },
    ],
    logs: [
      { time: "10:38:12", model: "claude-sonnet-4.5", account: "jackey", status: 200, ms: 842, tokens: 1284 },
      { time: "10:37:44", model: "gpt-5-codex", account: "team-shared", status: 200, ms: 610, tokens: 402 },
      { time: "10:36:02", model: "claude-opus-4.1", account: "jackey", status: 200, ms: 1420, tokens: 3105 },
      { time: "10:34:51", model: "gpt-5-codex", account: "qa-bot", status: 429, ms: 121, tokens: 0 },
      { time: "10:33:19", model: "claude-sonnet-4.5", account: "jackey", status: 200, ms: 733, tokens: 918 },
      { time: "10:31:07", model: "gemini-2.5-pro", account: "team-shared", status: 200, ms: 980, tokens: 1642 },
    ],
  },
  "wb-global": {
    enabled: false, port: 57891, baseUrl: "http://127.0.0.1:57891",
    keys: [{ id: "k4", name: "国际版 Key", masked: "bs_live_1a55••••••••••••88d0", scope: "国际版", created: "2026-09-25", lastUsed: "昨天", requests: 26 }],
    models: [{ name: "claude-sonnet-4.5", provider: "anthropic", scope: "国际版", available: true }],
    catalog: { source: "cached", fetchedAt: "昨天 21:04", fetchedAbs: "2026-10-02 21:04", note: "本次探测超时（8s），使用上次成功的结果" },
    pool: [{ account: "jackey-global", credits: 720, weight: 100, state: "可用", sticky: true }],
    logs: [{ time: "09:12:30", model: "claude-sonnet-4.5", account: "jackey-global", status: 200, ms: 1120, tokens: 640 }],
  },
  "trae-cn": {
    enabled: true, port: 57892, baseUrl: "http://127.0.0.1:57892",
    keys: [
      { id: "k5", name: "TraeWork Key", masked: "ts_live_77bb••••••••••••2f4a", scope: "国内版", created: "2026-09-18", lastUsed: "5 分钟前", requests: 892 },
      { id: "k6", name: "TraeCode Key", masked: "ts_live_e310••••••••••••9c77", scope: "国内版", created: "2026-09-26", lastUsed: "40 分钟前", requests: 254 },
    ],
    models: [
      { name: "chat_v3", provider: "trae", scope: "TraeCode", available: true },
      { name: "solo_work_lite", provider: "trae", scope: "TraeWork", available: true },
      { name: "trae-code-pro", provider: "trae", scope: "TraeCode", available: true },
    ],
    /* Trae 侧按**程序位**分家下发清单（TraeWork=solo_work_lite / TraeCode=chat_v3），
       所以识别也必须按 scope 分家，不能一份清单打天下。 */
    catalog: { source: "live", fetchedAt: "5 分钟前", fetchedAbs: "2026-10-03 12:45", note: null },
    pool: [
      { account: "jackey", credits: 880, weight: 100, state: "可用", sticky: true },
      { account: "code-lab", credits: 1320, weight: 100, state: "可用", sticky: false },
      { account: "solo-test", credits: 90, weight: 15, state: "余额偏低", sticky: false },
    ],
    logs: [
      { time: "10:39:01", model: "chat_v3", account: "code-lab", status: 200, ms: 512, tokens: 742 },
      { time: "10:37:20", model: "solo_work_lite", account: "jackey", status: 200, ms: 690, tokens: 1103 },
      { time: "10:35:48", model: "chat_v3", account: "jackey", status: 200, ms: 431, tokens: 388 },
      { time: "10:32:12", model: "trae-code-pro", account: "solo-test", status: 402, ms: 88, tokens: 0 },
    ],
  },
  "trae-global": {
    enabled: false, port: 57893, baseUrl: "http://127.0.0.1:57893", keys: [], models: [], pool: [], logs: [],
    /* 从未探测成功过 ⇒ 内置兜底。UI 必须显示「内置」+ 原因 + 重试出口，而不是静默给一份清单。 */
    catalog: { source: "builtin", fetchedAt: null, fetchedAbs: null, note: "尚未成功识别过（上游未响应）" },
  },
};

/* ─────────── 关联会话（v2.3）───────────
   同一份会话被复制到多个账号后，各账号里的副本会各自演化
   ⇒ 需要回答「谁落后 / 谁分叉 / 同步给谁」。
   组状态**由成员派生**，不写死（§3.10 第 1 条）。 */
/* ★ 会话复制**只有 WorkBuddy 桌面**有（v2.6 核实：调用点在 `switch.rs`，即 WB 账号切换路径；
   CodeBuddy IDE / CLI 的切换是各自独立模块，不带会话复制）。
   ⇒ 当前只有 1 个客户端，按 §3.15.1 的退化规则**不渲染客户端 Tab**（单选项的切换器是假控件）。
   保留数组结构，是为了将来有第二个客户端支持时**零改动**地出现。 */
const SESSION_CLIENTS = [
  { id: "workbuddy", label: "WorkBuddy 桌面", mark: "◆" },
];

/* member.state: latest（最新/与共同旧版一致） / behind（落后） / diverge（已分叉） */
const SESSION_GROUPS = [
  {
    key: "sg1", client: "workbuddy", title: "重构账号切换流程",
    updated: "12 分钟前", base: "2026-09-30 14:22",
    members: [
      { account: "jackey", delta: "+3 条", state: "latest" },
      { account: "team-shared", delta: "落后 3 条", state: "behind" },
    ],
  },
  {
    key: "sg2", client: "workbuddy", title: "修 issue #3 命名后缀",
    updated: "2 小时前", base: "2026-09-29 09:10",
    /* 演示数据里**故意不出现「已一致」状态** —— 让「筛选选项按需出现」这条规则可被证伪：
       数据只有 2 种状态 ⇒ 选项应是「全部 + 2」；若渲染成固定的 4 个，就多出假选项。 */
    members: [
      { account: "jackey", delta: "+1 条", state: "latest" },
      { account: "backup-01", delta: "落后 1 条", state: "behind" },
      { account: "team-shared", delta: "落后 1 条", state: "behind" },
    ],
  },
  {
    key: "sg3", client: "workbuddy", title: "网关账号池选号策略",
    updated: "昨天", base: "2026-09-28 17:40",
    members: [
      { account: "jackey", delta: "+5 条", state: "latest" },
      { account: "backup-01", delta: "+2 条（另一分支）", state: "diverge" },
    ],
  },
  {
    key: "sg4", client: "workbuddy", title: "Tauri 打包脚本整理",
    updated: "3 天前", base: "2026-09-26 11:02",
    members: [
      { account: "jackey", delta: "+1 条", state: "latest" },
      { account: "team-shared", delta: "落后 1 条", state: "behind" },
    ],
  },
];

/* 组状态派生：有分叉 → diverge；否则有落后 → behind；否则 latest。 */
function sessionGroupStatus(g) {
  if (g.members.some((m) => m.state === "diverge")) return "diverge";
  if (g.members.some((m) => m.state === "behind")) return "behind";
  return "latest";
}

/* ─────────── 请求代理（§3.19 / P12）───────────
   代理客户端自身的业务请求，由网关按请求选号 ⇒ **无缝切号**（不写文件、不重启客户端）。

   ⚠️ 两条已拍板的硬约束（D29）：
     ① **默认关**；
     ② **绝不劫持** —— 不改 hosts / 不做 DNS 劫持 / 不做透明代理，
        只接受「客户端主动把请求交给网关」。 */
const PROXY = {
  enabled: false,                 // 默认关（D29）
  mode: "C",                      // 当前拦截方式（D34：C 先行）
  modeLabel: "C · 客户端 base URL",
  modeNote: "无需 CA、不碰 TLS；覆盖 LLM 调用（高频）",
  todayRequests: 128,
  switches: 3,                    // 今日换号次数
  domains: [
    { host: "www.codebuddy.cn", on: true, note: "WorkBuddy CN" },
    { host: "www.workbuddy.ai", on: true, note: "WorkBuddy Global" },
    { host: "api.trae.cn", on: true, note: "Trae CN" },
  ],
};

/* ─────────── 桌面悬浮窗（§3.16 / D21 已拍板进本期）───────────
   各来源的会话状态，**只读** —— 浮层不承载任何写操作（§3.16 明确「不做的」）。
   state 与 §3.6 的状态语言对齐：运行中 / 待确认 / 已完成 / 失败。 */
const COMPANION_SOURCES = [
  { id: "cp1", source: "Codex", title: "重构账号切换流程", state: "running", hint: "12 分钟前" },
  { id: "cp2", source: "WorkBuddy", title: "网关账号池选号策略", state: "waiting", hint: "等待确认" },
  { id: "cp3", source: "CodeBuddy", title: "Tauri 打包脚本整理", state: "done", hint: "2 小时前" },
  { id: "cp4", source: "WorkBuddy", title: "修复 issue #3 命名后缀", state: "failed", hint: "昨天" },
];

/* ─────────── 设置 ─────────── */
const SETTINGS = [
  {
    id: "general", label: "通用",
    groups: [
      { title: "外观", rows: [
        { label: "主题", desc: "跟随系统 / 浅色 / 深色", type: "select", value: "跟随系统", options: ["跟随系统", "浅色", "深色"] },
        { label: "紧凑模式", desc: "卡片更小，同屏显示更多账号", type: "switch", value: true },
        { label: "界面语言", desc: "简体中文 / English", type: "select", value: "简体中文", options: ["简体中文", "English"] },
      ]},
      { title: "启动与更新", rows: [
        { label: "开机自启", desc: "系统启动时在后台运行", type: "switch", value: true },
        { label: "自动检查更新", desc: "每 30 分钟检查一次新版本", type: "switch", value: true },
        { label: "最小化到托盘", desc: "关闭窗口时保持后台运行", type: "switch", value: true },
      ]},
    ],
  },
  {
    id: "account", label: "账号与登录",
    groups: [
      { title: "登录行为", rows: [
        { label: "切换时复制会话", desc: "把当前账号的会话一并带到目标账号", type: "switch", value: false },
        { label: "迁移记忆与连接器", desc: "切换账号时同步记忆库与连接器配置", type: "switch", value: false },
        { label: "切换后自动重启客户端", desc: "写入凭据后重启，使新账号立即生效", type: "switch", value: true },
      ]},
      { title: "列表展示", rows: [
        { label: "当前登录账号置顶", desc: "把正在使用的账号排到列表最前", type: "switch", value: true },
        { label: "默认排序", desc: "打开账号页时的排序方式", type: "select", value: "积分优先级", options: ["积分优先级", "名称", "到期时间", "最近使用"] },
      ]},
    ],
  },
  {
    id: "checkin", label: "签到与积分",
    groups: [
      { title: "自动签到", rows: [
        { label: "启用自动签到", desc: "每天定时为全部账号领取签到积分", type: "switch", value: true },
        { label: "执行时刻", desc: "每天在此时刻触发（本地时间）", type: "time", value: "08:00" },
        { label: "跳过今日已签", desc: "已签到的账号不再重复请求", type: "switch", value: true },
      ]},
      { title: "积分与轮换", rows: [
        { label: "启用自动轮换", desc: "积分不足时自动切换到余额充足的账号", type: "switch", value: true },
        { label: "轮换阈值", desc: "剩余积分低于该值时触发轮换", type: "number", value: "200" },
        { label: "紧迫度权重", desc: "0 = 只看余额；越大越优先消耗即将过期的积分", type: "number", value: "0" },
      ]},
      { title: "自动旅行", rows: [
        { label: "启用自动旅行", desc: "通过旅行任务获取额外积分", type: "switch", value: false, cap: "travel" },
      ]},
    ],
  },
  {
    id: "gateway", label: "网关",
    groups: [
      { title: "服务", rows: [
        { label: "启用本地网关", desc: "在 127.0.0.1 上提供 OpenAI / Anthropic 兼容接口", type: "switch", value: true },
        { label: "监听端口", desc: "修改后需要重启网关", type: "number", value: "57890" },
      ]},
      { title: "账号池", rows: [
        { label: "启用账号池", desc: "按积分优先级自动选择上游账号", type: "switch", value: true },
        { label: "粘性会话", desc: "同一会话尽量复用同一账号", type: "switch", value: true },
        { label: "池为空时短探测", desc: "池为空时缩短刷新间隔，尽快填充", type: "switch", value: true },
      ]},
    ],
  },
  {
    id: "schedule", label: "计划任务",
    groups: [
      { title: "已注册任务", rows: [
        { label: "启动维护", desc: "启动时同步账号池与治理状态", type: "static", value: "每次启动" },
        { label: "积分刷新", desc: "刷新各账号剩余积分", type: "static", value: "每 10 分钟" },
        { label: "池状态落盘", desc: "持久化账号池状态", type: "static", value: "每 5 分钟" },
        { label: "自动轮换", desc: "按策略切换上游账号", type: "static", value: "每 2 分钟" },
      ]},
    ],
  },
  {
    id: "client", label: "客户端",
    groups: [
      { title: "客户端检测", rows: [
        { label: "WorkBuddy Desktop", desc: "检测安装路径与版本", type: "static", value: "已检测到 · 2026.10.1" },
        { label: "CodeBuddy CLI", desc: "命令行工具的可用性", type: "static", value: "未检测到" },
        { label: "CodeBuddy CN IDE", desc: "国内版 IDE 的接入状态", type: "static", value: "已检测到" },
      ]},
      { title: "网络与代理", rows: [
        { label: "使用系统代理", desc: "跟随操作系统的代理设置", type: "switch", value: true },
        { label: "自定义代理地址", desc: "留空则使用系统代理", type: "text", value: "" },
        { label: "请求超时", desc: "单次请求的超时秒数", type: "number", value: "30" },
      ]},
      { title: "设备", rows: [
        { label: "设备标识", desc: "用于客户端识别本机", type: "static", value: "已生成" },
      ]},
    ],
  },
  {
    id: "diagnostics", label: "权限与诊断",
    groups: [
      { title: "环境自检", rows: [
        { label: "数据目录", desc: "账号库与配置的存放位置", type: "static", value: "~/.buddy-switch/" },
        { label: "客户端检测", desc: "是否找到已安装的客户端", type: "static", value: "已检测到" },
        { label: "凭据可读性", desc: "当前账号的凭据是否可解密", type: "static", value: "正常" },
      ]},
      { title: "危险操作", rows: [
        { label: "重置设备标识", desc: "重新生成设备 ID，可能影响登录态", type: "button", value: "重置" },
        { label: "导出诊断包", desc: "打包日志与配置（不含凭据）", type: "button", value: "导出" },
      ]},
    ],
  },
  {
    id: "logs", label: "日志",
    groups: [
      { title: "运行日志", rows: [
        { label: "日志级别", desc: "记录的详细程度", type: "select", value: "信息", options: ["错误", "警告", "信息", "调试"] },
        { label: "保留天数", desc: "超过天数的日志自动清理", type: "number", value: "7" },
        { label: "日志目录", desc: "运行日志的存放位置", type: "static", value: "~/.buddy-switch/logs/" },
      ]},
      { title: "操作", rows: [
        { label: "打开日志目录", desc: "在文件管理器中打开", type: "button", value: "打开" },
        { label: "清空日志", desc: "删除全部运行日志", type: "button", value: "清空" },
      ]},
    ],
  },
  /* 备份与恢复：不用通用 rows 表格渲染，走 demo-app.js 的 viewBackup()（kind 标记）。 */
  {
    id: "backup", label: "备份与恢复", kind: "backup",
    groups: [],
  },
];

/* ─────────── 全局备份（PLAN §3.14）───────────
   分区与条目数**全部由已有数据推导**（§3.10 第 1 条：渲染层不写死数字），
   所以「预计大小」会随勾选实时变化，而不是一个装饰性的假数字。 */
const BACKUP_SECTIONS = Object.keys(SCOPES).map((id) => ({
  id,
  label: SCOPES[id].label,
  accounts: SCOPES[id].accounts.length,
  configs: 4,                                  // 签到 / 旅行 / 自动轮换 / 切换行为
  gatewayKeys: SCOPES[id].gatewayKeys || 0,
}));

/* 单条记录的粗略字节数，仅用于估算导出体积（不参与任何业务判断）。 */
const BACKUP_BYTES = { base: 320, account: 620, credential: 900, config: 480, gatewayKey: 180 };

/* 模拟「用户选中的备份文件」——真实实现里由文件解析得到，这里用于演示预览与冲突策略。 */
const BACKUP_FILE_SAMPLE = {
  formatVersion: 1,
  exportedAt: "2026-10-01T14:00:00Z",
  includesCredentials: false,
  scopes: {
    "wb-cn": { added: 3, replaced: 2, skipped: 1, conflict: 0, configs: ["签到", "旅行", "轮换"] },
    "trae-global": { added: 5, replaced: 0, skipped: 0, conflict: 0, configs: [] },
  },
};

/* ─────────── 应用更新信息 ───────────
   侧栏底部「检查更新」由它驱动，避免把版本判断写死在逻辑里：
     hasUpdate = false → 提示「已是最新版本 v<current>」
     hasUpdate = true  → 打开更新说明弹层，展示 notes
   把 hasUpdate 改成 false 即可演示「已是最新」分支。 */
const APP_UPDATE = {
  current: "2026.10.1",
  hasUpdate: true,
  latest: "2026.10.2",
  size: "24.6 MB",
  notes: [
    "账号工作台新增「按状态分组」，可直接筛出未签到 / 凭据过期的账号",
    "修复 SPA 深链在刷新后返回 404 的问题",
    "网关账号池选号更均匀，减少单一账号被耗尽",
  ],
};

/* ─────────── 统一挂载派生用量数据 ───────────
   放在文件末尾：此时 USAGE / SCOPES / GATEWAY 全部已定义。 */
for (const _id of Object.keys(USAGE)) USAGE[_id].ext = buildUsageExtras(_id);
