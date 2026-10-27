/* ==========================================================================
   Buddy Switch 原型 — 模拟数据
   形状刻意贴近真实后端契约（account / gateway / usage），方便你对照现有字段。
   ========================================================================== */

/* ─────────── 图标 ─────────── */
const MARK = {
  workbuddy: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.6 20.5 12 12 21.4 3.5 12Z"/><path d="M12 7.6 16.6 12 12 16.4 7.4 12Z"/></svg>`,
  trae: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.8 21.2 20H2.8Z"/><path d="M12 9.6 16.4 17H7.6Z"/></svg>`,
  qoder: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.6"/><circle cx="12" cy="12" r="4.2"/><path d="m15.3 15.3 4.1 4.1"/></svg>`,
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
  external: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6M20 4l-9 9"/><path d="M18 14v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2h5"/></svg>`,
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
  qoder: {
    id: "qoder",
    label: "Qoder",
    mark: MARK.qoder,
    accent: "oklch(0.68 0.15 245)",
    regions: [
      { id: "cn", label: "国内版", short: "CN" },
      { id: "global", label: "国际版", short: "Global" },
    ],
    /* 程序位 = **能承载「当前登录」的官方客户端**。Qoder 有两处官方凭据存储：
         ① 桌面 App —— `%APPDATA%\com.qoder[.cn].app.stable\auth.v1.dat`
            （Chromium v10 布局，AES-256-GCM，密钥经 DPAPI 解出）
         ② Qoder CLI —— `~/.qoder[.cn]/.auth/user`（AES-128-CBC，key=iv=machine_id[:16]）
       两者各是一个程序位。⚠️ 与 WorkBuddy 一样，**程序位名不含区域** ——
       区域由作用域（qoder-cn / qoder-global）承担，写进名字会让「国内版/国际版」
       看起来像两个客户端。 */
    programs: [
      { id: "qoder_app", label: "Qoder 桌面版", short: "App", installed: true },
      { id: "qoder_cli", label: "Qoder CLI", short: "CLI", installed: true },
    ],
    /* Qoder 的能力集：签到（每日 100 Credits 由网关代领）、积分/额度、
       OAuth 设备授权（PKCE S256）、API 网关（OpenAI + Anthropic 双协议）。
       ⚠️ 刻意**没有** sessionCopy（会话复制只有 WorkBuddy 桌面有），
       也**没有** travel（自动旅行是 WorkBuddy 独有）、autoRotate、snapshot、group。 */
    capabilities: ["checkin", "credits", "oauth", "gateway"],
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

  /* ① 先定每个包的**总量**（最后一包吃余数 ⇒ 合计恒等于 creditsTotal）。 */
  const packs = [];
  let totLeft = creditsTotal;
  for (let i = 0; i < n; i++) {
    const tot = i === n - 1 ? Math.max(0, totLeft) : Math.round(creditsTotal / n);
    totLeft -= tot;
    packs.push({ name: names[i], total: tot, remaining: tot });
  }

  /* ② 再把「已用掉的量」随机摊到各包上 —— 之前每包都写 `credits / n`，
        结果一屏四行全是「310 / 500 · 已用 190」，进度条一模一样，
        既不像真实发放（有的包没动、有的用掉大半），也让设计评审看不出条形图的差异。
        ⚠️ 摊完的合计必须**精确**等于 `credits`（否则「积分」指标块与包明细对不上）。 */
  let used = creditsTotal - Math.min(credits, creditsTotal);
  const order = packs.map((_, i) => i).sort(() => rnd() - 0.5);
  for (const i of order) {
    if (used <= 0) break;
    const p = packs[i];
    /* 每包用掉 20%~90%（而不是固定 90%）：额度紧的账号（如 120/2000）否则会变成
       「每包都恰好用 450」的一排相同进度条 —— 那正是这版要修掉的观感。 */
    const take = Math.min(Math.floor(p.total * (0.2 + rnd() * 0.7)), used);
    p.remaining = p.total - take;
    used -= take;
  }
  /* ③ 还有没摊完的（都被上限挡住时）继续往后摊，允许把包用光 ——
        ⚠️ 必须摊完：`sum(remaining)` 要**精确**等于 `credits`，否则指标块与包明细对不上
        （曾经把余量一次性压到最后一包，被 clamp 吃掉几百，合计就漂了）。 */
  for (const p of packs) {
    if (used <= 0) break;
    const take = Math.min(p.remaining, used);
    p.remaining -= take;
    used -= take;
  }

  /* ④ 到期：`expireAt`（毫秒）是真实后端 `CreditResource.expireAt` 的对应字段 ——
        抽屉的「积分 / 资源包」要显示「到期 10-24 12:00」，只有天数不够。
        抖动 ±6.5h 让到期时刻看起来像真的发放时间，而不是每包都「今天此刻 + N 天」。 */
  for (let i = 0; i < packs.length; i++) {
    const p = packs[i];
    /* 第一包约一半概率落在 7 天内 ⇒ 演示里能看到「临期」的橙色条与卡片徽章；
       其余包 2~61 天，保证「未临期」也始终有样本（颜色断言需要两种都在）。 */
    const days = i === 0 && rnd() < 0.55 ? 2 + (rnd() % 6) : 2 + (rnd() % 60);
    const jitterMs = ((rnd() % 780) - 390) * 60000;
    p.expireAt = Date.now() + days * 86400000 + jitterMs;
    p.expireInDays = Math.max(1, Math.ceil((p.expireAt - Date.now()) / 86400000));
  }
  return packs;
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
    credits: 1200, creditsTotal: 2000,
    checkedIn: true, expired: false, needsRelogin: false, priority: false,
    programs: {}, lastUsed: "刚刚", lastUsedTs: Date.now(),
    todayTokens: 0, totalTokens: 0, status: "ok",
    snapshots: [],
  }, o || {});
  a.sessionTree = a.sessionTree || makeSessionTree(id);
  a.packs = a.packs || makePacks(id, a.credits, a.creditsTotal);
  /* 「临期积分 / 最近到期」**从资源包派生**，不再手写在账号字面量里。
     真实后端的 `expiringSoonRemaining` 也是这么算的；手写会与包明细打架 ——
     a2 就出现过「指标块说无临期积分、积分页却摆着一个 10-20 到期的包」。
       · expiring      = 7 天内到期的包里还剩多少（卡片徽章 + 指标块用）
       · expireInDays  = 这些包里最早到期的天数（没有临期包时退回全部包里最近的） */
  const _now = Date.now();
  const _soon = a.packs.filter((p) => p.remaining > 0 && p.expireAt && p.expireAt - _now <= 7 * 86400000);
  a.expiring = _soon.reduce((sum, p) => sum + p.remaining, 0);
  const _live = a.packs.filter((p) => p.remaining > 0 && p.expireAt);
  a.expireInDays = _soon.length
    ? Math.min(..._soon.map((p) => Math.ceil((p.expireAt - _now) / 86400000)))
    : (_live.length ? Math.min(..._live.map((p) => Math.ceil((p.expireAt - _now) / 86400000))) : null);
  a.checkinHistory = a.checkinHistory || makeCheckinHistory(id);
  return a;
}

const SCOPES = {
  "wb-cn": {
    id: "wb-cn", platform: "workbuddy", region: "cn",
    running: true, installed: true, version: "2.14.3",
    todayCredits: 46, gatewayRequests: 312, gatewayKeys: 5, checkinReward: 100,
    /* ★ 客户端是否接入本机网关（P12 请求代理）。决定「账本」视图的对账结论：
       true ⇒ 客户端 ⊆ 网关（合计 = 网关 + 直连客户端）；false ⇒ 两本账独立（合计 = 相加 − 重叠）。 */
    proxy: true,
    accounts: [
      makeAccount("a1", "jackey", "jackey@example.com", {
        remark: "主号 · 长期", credits: 1240, creditsTotal: 2000, group: "主力",
        programs: { workbuddy: true, ide: true, cli: false }, priority: true,
        lastUsed: "2 分钟前", todayTokens: 42100, totalTokens: 862400,
      }),
      makeAccount("a2", "team-shared", "shared@example.com", {
        remark: "团队共用", credits: 860, creditsTotal: 1000, group: "主力",
        programs: { workbuddy: false, ide: true, cli: false },
        lastUsed: "1 小时前", todayTokens: 28400, totalTokens: 512000,
      }),
      makeAccount("a3", "backup-01", "backup01@example.com", {
        remark: "备用", credits: 1580, creditsTotal: 2000, expireInDays: 44, group: "备用",
        programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "3 天前", todayTokens: 0, totalTokens: 96400,
      }),
      makeAccount("a4", "qa-bot", "qa@example.com", {
        remark: "自动化测试", credits: 120, creditsTotal: 2000, group: "自动化",
        checkedIn: false, programs: { workbuddy: false, ide: true, cli: false },
        lastUsed: "5 分钟前", todayTokens: 31200, totalTokens: 288100,
      }),
      makeAccount("a5", "design-lab", "design@example.com", {
        remark: "", credits: 430, creditsTotal: 1000, expireInDays: 12,
        checkedIn: false, expired: true, programs: { workbuddy: false, ide: false, cli: false },
        lastUsed: "8 天前", todayTokens: 0, totalTokens: 41200, status: "expired",
      }),
      makeAccount("a6", "ops-02", "ops02@example.com", {
        remark: "运维", credits: 990, creditsTotal: 2000, expireInDays: 27, group: "备用",
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
    /* 国际版这台没开请求代理 ⇒ 两本账独立（合计 = 相加 − 重叠），用来对照 wb-cn 的子集关系。 */
    proxy: false,
    accounts: [
      makeAccount("g1", "jackey-global", "jackey@global.example.com", {
        remark: "国际版主号", credits: 720, creditsTotal: 1000,
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
        remark: "TraeWork 主号", credits: 880, creditsTotal: 1500,
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
        remark: "TRAE SOLO 测试", credits: 90, creditsTotal: 500,
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
  "qoder-cn": {
    id: "qoder-cn", platform: "qoder", region: "cn",
    running: true, installed: true, version: "1.1.64",
    todayCredits: 52, gatewayRequests: 184, gatewayKeys: 2, checkinReward: 100,
    accounts: [
      makeAccount("q1", "jackey", "jackey@qoder.cn", {
        remark: "主号 · 长期", credits: 1180, creditsTotal: 2000,
        programs: { qoder_app: true, qoder_cli: true }, priority: true, group: "常用",
        lastUsed: "3 分钟前", todayTokens: 38400, totalTokens: 512000,
      }),
      makeAccount("q2", "team-qoder", "shared@qoder.cn", {
        remark: "团队共用", credits: 620, creditsTotal: 1000, expireInDays: 19,
        programs: { qoder_app: false, qoder_cli: true }, group: "团队",
        lastUsed: "1 小时前", todayTokens: 12600, totalTokens: 208000,
      }),
      makeAccount("q3", "trial-01", "trial@qoder.cn", {
        remark: "新人试用", credits: 80, creditsTotal: 500,
        checkedIn: false, programs: { qoder_app: false, qoder_cli: false }, group: "测试",
        lastUsed: "20 分钟前", todayTokens: 5200, totalTokens: 34800,
      }),
      makeAccount("q4", "legacy-acc", "legacy@qoder.cn", {
        remark: "旧凭据", credits: 320, creditsTotal: 1000, expireInDays: 12,
        checkedIn: false, expired: true, programs: { qoder_app: false, qoder_cli: false },
        group: "闲置", lastUsed: "9 天前", todayTokens: 0, totalTokens: 76200, status: "expired",
      }),
      makeAccount("q5", "spare-02", "spare@qoder.cn", {
        remark: "备用", credits: 1520, creditsTotal: 2000, expireInDays: 41,
        needsRelogin: true, programs: { qoder_app: false, qoder_cli: false },
        group: "闲置", lastUsed: "13 天前", todayTokens: 0, totalTokens: 91300, status: "relogin",
      }),
    ],
  },
  "qoder-global": {
    id: "qoder-global", platform: "qoder", region: "global",
    running: false, installed: true, version: "1.1.64",
    todayCredits: 18, gatewayRequests: 21, gatewayKeys: 1, checkinReward: 100,
    accounts: [
      makeAccount("qg1", "jackey-global", "jackey@qoder.com", {
        remark: "国际版主号", credits: 900, creditsTotal: 1500,
        programs: { qoder_app: true, qoder_cli: false }, priority: true,
        lastUsed: "2 小时前", todayTokens: 21800, totalTokens: 176400,
      }),
      makeAccount("qg2", "intl-spare", "intl@qoder.com", {
        remark: "", credits: 240, creditsTotal: 1000, expireInDays: 22,
        checkedIn: false, programs: { qoder_app: false, qoder_cli: false },
        lastUsed: "5 天前", todayTokens: 3400, totalTokens: 41200,
      }),
    ],
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
/* ─────────── 网关账本（issue #63）───────────
   设计依据：`docs/specs/USAGE-GATEWAY-UI-REDESIGN.md`。
   三条硬约束，改这里之前先读：
   ① **按模型 / 按账号 / 按端点三者的合计必须都等于网关总量**（否则页面上的「按模型合计 = 网关卡」当场破功）；
   ② **对账结果必须与 `proxy` 一致**：`proxy:true` ⇒ 客户端 ⊆ 网关（匹配率 ~92%，判 subset）；
      `proxy:false` ⇒ 两本账独立（匹配率 ~6%，判 independent）。这是这个设计要证明的那件事；
   ③ 积分同理：余额口径（consumed）**恒大于**请求口径（byRequest），差额是「未归因」，
      绝不能被摊到任何来源上。 */
function buildLedger(scopeId, u, modelDays) {
  const scope = SCOPES[scopeId];
  const proxied = Boolean(scope.proxy);

  const clientDays = u.days.map((d) => d.value);
  const clientTokens = clientDays.reduce((a, b) => a + b, 0);
  const gwFactor = proxied ? 5.6 : 1.4;

  /* 网关逐日：按客户端的形状放大，末位吃余数保证合计一致 */
  const gwRaw = clientDays.map((v, i) => v * gwFactor * (0.82 + ((i * 31) % 40) / 100));
  const gwRawSum = gwRaw.reduce((a, b) => a + b, 0) || 1;
  const gatewayTokens = Math.round(clientTokens * gwFactor);
  let accGw = 0;
  const gatewayDays = gwRaw.map((v, i) => {
    const out = i === gwRaw.length - 1 ? gatewayTokens - accGw : Math.round((v / gwRawSum) * gatewayTokens);
    accGw += out;
    return Math.max(0, out);
  });

  /* 网关模型：客户端模型放大 + 一个「仅网关」的 auto（真实网关里 auto 路由会出现） */
  const rawModels = u.models.length
    ? u.models.map((m, i) => ({ name: m.name, w: m.tokens * (i === 0 ? 1.3 : 0.62) }))
    : [{ name: "claude-sonnet-4.5", w: 1 }];
  rawModels.push({ name: "auto", w: gatewayTokens * 0.004 });
  const modelW = rawModels.reduce((a, m) => a + m.w, 0) || 1;
  let accM = 0;
  const gatewayModels = rawModels.map((m, i) => {
    const out = i === rawModels.length - 1
      ? gatewayTokens - accM
      : Math.round((m.w / modelW) * gatewayTokens);
    accM += out;
    const req = Math.max(1, Math.round(out / 148000));
    return { name: m.name, tokens: Math.max(0, out), req };
  });

  /* 网关的**逐日 × 模型**拆分（「模型」tab 的每日按模型堆叠图要用）。
     与 modelDays 同一手法：每天按权重摊，末位吃余数 ⇒ 每列之和 === 当日网关总量。 */
  const gwModelDays = gatewayModels.map((m) => ({ name: m.name, values: [] }));
  gatewayDays.forEach((dayTotal, i) => {
    const w = gatewayModels.map((m, k) => m.tokens * (0.8 + ((i * 29 + k * 47) % 40) / 100));
    const wsum = w.reduce((a, b) => a + b, 0) || 1;
    let acc = 0;
    gatewayModels.forEach((m, k) => {
      const v = k === gatewayModels.length - 1 ? Math.max(0, dayTotal - acc) : Math.round((dayTotal * w[k]) / wsum);
      acc += v;
      gwModelDays[k].values.push(Math.max(0, v));
    });
  });

  /* 网关账号：取本作用域前 3 个账号按 5:3:2 摊 */
  const accNames = (scope.accounts || []).slice(0, 3).map((a) => a.name);
  const shares = [0.52, 0.34, 0.14];
  let accA = 0;
  const gatewayAccounts = (accNames.length ? accNames : ["unknown"]).map((name, i, arr) => {
    const out = i === arr.length - 1 ? gatewayTokens - accA : Math.round(gatewayTokens * (shares[i] ?? 0.1));
    accA += out;
    return { name, tokens: Math.max(0, out), req: Math.max(1, Math.round(out / 148000)) };
  });

  /* 端点：chat 96% / messages 2% / responses 2% */
  const gatewayRows = Math.max(1, Math.round(gatewayTokens / 148000));
  const epChat = Math.round(gatewayRows * 0.96);
  const epMsg = Math.round(gatewayRows * 0.02);
  const gatewayEndpoints = [
    { name: "/v1/chat/completions", req: epChat },
    { name: "/v1/messages", req: epMsg },
    { name: "/v1/responses", req: Math.max(0, gatewayRows - epChat - epMsg) },
  ];

  /* 对账 */
  const clientRows = Math.max(1, Math.round(clientTokens / 42000));
  const matchedRatio = proxied ? 0.92 : 0.06;
  const matched = Math.round(clientRows * matchedRatio);
  const matchedTokens = Math.round(clientTokens * matchedRatio);
  const verdict = proxied ? "subset" : "independent";
  const failed = Math.max(1, Math.round(gatewayRows * 0.04));

  /* 积分：两个口径（余额口径 = 请求口径 + 未归因） */
  const creditOf = (tokens) => +(tokens / 34000).toFixed(1);
  const clientCredit = creditOf(clientTokens);
  const gatewayCredit = creditOf(gatewayTokens);
  const overlapCredit = +(clientCredit * matchedRatio).toFixed(1);
  const byRequest = +(clientCredit + gatewayCredit - overlapCredit).toFixed(1);
  const gap = +(byRequest * 0.06).toFixed(1);
  const consumed = +(byRequest + gap).toFixed(1);

  /* 客户端模型用量**必须取 modelDays 的逐日合计**：`u.models[].tokens` 是示意量，
     不等于 days 的合计 —— 拿它算占比会得出「五个模型加起来只有 34%」。 */
  const clientModelTotals = (modelDays || []).map((m) => ({
    name: m.name,
    tokens: m.values.reduce((a, b) => a + b, 0),
  }));
  const creditShare = (list, total, totalCredit) =>
    list.map((m) => ({
      name: m.name,
      v: +(totalCredit * (m.tokens / (total || 1))).toFixed(1),
      /* 客户端模型没有请求数字段（modelDays 只存逐日 Token）⇒ 按均值折算，别写成 0。 */
      req: m.req ?? Math.max(1, Math.round((m.tokens || 0) / 42000)),
    }));
  const dailyCredit = (values, totalTokens, totalCredit) =>
    values.map((v) => +(totalCredit * (v / (totalTokens || 1))).toFixed(1));

  return {
    proxied, verdict,
    client: { tokens: clientTokens, requests: clientRows, days: clientDays },
    gateway: {
      tokens: gatewayTokens, requests: gatewayRows, failed,
      days: gatewayDays, models: gatewayModels, modelDays: gwModelDays,
      accounts: gatewayAccounts, endpoints: gatewayEndpoints,
      coverage: `${u.days.length}/${u.days.length} 天`,
    },
    recon: { clientRows, gatewayRows, matched, matchedTokens, ratio: matchedRatio },
    credits: {
      client: clientCredit, gateway: gatewayCredit, overlap: overlapCredit,
      byRequest, consumed, gap,
      clientModels: creditShare(clientModelTotals, clientTokens, clientCredit),
      gatewayModels: creditShare(gatewayModels, gatewayTokens, gatewayCredit),
      dailyClient: dailyCredit(clientDays, clientTokens, clientCredit),
      dailyGateway: dailyCredit(gatewayDays, gatewayTokens, gatewayCredit),
    },
  };
}

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

  /* ★ 积分两个口径必须**同一个量级**：余额口径取 `creditDaily.outAmt` 的合计（权威），
     请求口径按 token 折算出来，两者若各写一套数，差额会大到像 bug（实测 57%）。
     ⇒ 按比例把请求口径缩放到「余额口径 / 1.06」，让差额稳定在 ~6%（= 未归因）。 */
  const ledger = buildLedger(scopeId, u, modelDays);
  const consumedOut = creditDaily.reduce((a, d) => a + d.outAmt, 0);
  const target = consumedOut / 1.06;
  const k = ledger.credits.byRequest ? target / ledger.credits.byRequest : 1;
  for (const key of ["client", "gateway", "overlap", "byRequest"]) {
    ledger.credits[key] = +(ledger.credits[key] * k).toFixed(1);
  }
  for (const list of [ledger.credits.clientModels, ledger.credits.gatewayModels]) {
    for (const m of list) m.v = +(m.v * k).toFixed(1);
  }
  ledger.credits.dailyClient = ledger.credits.dailyClient.map((v) => +(v * k).toFixed(1));
  ledger.credits.dailyGateway = ledger.credits.dailyGateway.map((v) => +(v * k).toFixed(1));
  ledger.credits.consumed = +consumedOut.toFixed(1);
  ledger.credits.gap = +Math.max(0, consumedOut - ledger.credits.byRequest).toFixed(1);

  return { modelDays, hourly, creditDaily, balance, expiry, accountShare, statusCodes, scatter, curTotal, ledger };
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
  "qoder-cn": {
    days: series(30, 54000, 11, 56200),
    models: [
      { name: "Qwen3.8-Max", tokens: 240000, pct: 43 },
      { name: "GLM-5.1", tokens: 138000, pct: 25 },
      { name: "Kimi-K3", tokens: 96000, pct: 17 },
      { name: "MiniMax-M3", tokens: 52000, pct: 9 },
      { name: "其他", tokens: 33000, pct: 6 },
    ],
    sessions: [
      { title: "Qoder 多账号轮询", account: "jackey", tokens: 64200, msgs: 38, when: "今天 09:40" },
      { title: "COSY 签名链路排查", account: "jackey", tokens: 41800, msgs: 26, when: "今天 08:55" },
      { title: "模型清单三源优先级", account: "team-qoder", tokens: 28700, msgs: 17, when: "昨天 20:12" },
      { title: "试用号额度探测", account: "trial-01", tokens: 12400, msgs: 9, when: "昨天 15:30" },
    ],
    credits: [
      credit("今天 10:12", 28, "jackey", -52, "对话消耗"),
      credit("今天 08:00", 175, "jackey", +100, "每日签到"),
      credit("昨天 22:35", 700, "team-qoder", -96, "对话消耗"),
      credit("昨天 08:00", 1620, "jackey", +100, "每日签到"),
      credit("昨天 08:00", 1621, "team-qoder", +100, "每日签到"),
    ],
  },
  "qoder-global": {
    days: series(30, 20000, 13, 25200),
    models: [
      { name: "Qwen3.8-Max", tokens: 96000, pct: 40 },
      { name: "Ultimate", tokens: 62000, pct: 26 },
      { name: "DeepSeek-V4-Pro", tokens: 48000, pct: 20 },
      { name: "其他", tokens: 34000, pct: 14 },
    ],
    sessions: [
      { title: "国际版端点故障切换", account: "jackey-global", tokens: 21800, msgs: 13, when: "今天 11:05" },
      { title: "Ultimate 模型评测", account: "jackey-global", tokens: 9600, msgs: 7, when: "2 天前" },
    ],
    credits: [
      credit("今天 09:30", 105, "jackey-global", -18, "对话消耗"),
      credit("昨天 08:00", 1610, "jackey-global", +100, "每日签到"),
    ],
  },
};

/* ─────────── 网关数据 ─────────── */
const GATEWAY = {
  "wb-cn": {
    enabled: true, port: 57890, baseUrl: "http://127.0.0.1:57890",
    /* ★ `revoked: true` = 已吊销。已吊销的密钥**仍留在表里**，且行内必须同时给出
       「恢复」与「删除」两条出口 —— 吊销只是置标记（明文未变），恢复不需要重建 Key、
       也不需要改任何客户端配置；只留「删除」等于逼用户为一次误吊销删掉重建、
       再把所有客户端配置改一遍。 */
    keys: [
      { id: "k1", name: "默认 Key", masked: "bs_live_9f3a••••••••••••7c21", scope: "国内版", created: "2026-09-12", lastUsed: "2 分钟前", requests: 1842 },
      { id: "k2", name: "CI 自动化", masked: "bs_live_4d18••••••••••••a0e9", scope: "国内版", created: "2026-09-20", lastUsed: "1 小时前", requests: 613, revoked: true },
      { id: "k3", name: "临时调试", masked: "bs_live_c702••••••••••••31bf", scope: "国际版", created: "2026-09-28", lastUsed: "3 天前", requests: 47 },
      /* 「接近上限」那档（86%）：三档用量状态在默认视图里都能看到。 */
      { id: "k10", name: "外包同学", masked: "bs_live_d913••••••••••••52af", scope: "国内版", created: "2026-10-06", lastUsed: "18 分钟前", requests: 956 },
      /* 「不限额」那档：与三档限额同在默认视图（不限 / 正常 / 接近 / 用尽 一览齐全）。 */
      { id: "k11", name: "自用无限额", masked: "bs_live_f4a7••••••••••••6c03", scope: "国内版", created: "2026-10-11", lastUsed: "1 小时前", requests: 3120 },
    ],
    /* available = 上游事实（能不能调）；disabled = 用户偏好（主动拿掉）。
       两者**正交**，四种组合都合法 —— UI 必须能区分「已禁用」与「上游不可用」。 */
    models: [
      { name: "claude-sonnet-4.5", provider: "anthropic", scope: "国内版", available: true, context: 200000, maxWindow: 200000, efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "high", credits: 1.33 },
      { name: "claude-opus-4.1", provider: "anthropic", scope: "国内版", available: true, disabled: true, context: 200000, maxWindow: 200000, efforts: ["low", "medium", "high"], defaultEffort: "high", credits: 3.31 },
      { name: "gpt-5-codex", provider: "openai", scope: "国内版", available: true, context: 400000, maxWindow: 1000000, efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "high", credits: 0.79 },
      { name: "gemini-2.5-pro", provider: "openai", scope: "国内版", available: true, context: 300000, maxWindow: 1000000, efforts: ["low", "medium", "high"], defaultEffort: "medium", credits: 0.34 },
      { name: "claude-sonnet-4.5", provider: "anthropic", scope: "国际版", available: true, context: 200000, maxWindow: 200000, efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "high", credits: 1.33 },
      { name: "gpt-5-codex", provider: "openai", scope: "国际版", available: false, disabled: true, context: 400000, maxWindow: 1000000, efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "high", credits: 0.79 },
    ],
    /* 模型清单的来源（§3.17）：实时 / 已保存 / 内置 —— 三级都必须显式标注，
       不许把「内置」伪装成「实时」（用户照着清单调用会扑空）。 */
    catalog: { source: "live", fetchedAt: "12 分钟前", fetchedAbs: "2026-10-03 12:38", note: null },
    /* ★ 五态互斥（可用 / 冷却 / 禁用 / 零积分 / **已排除**）—— 排除从 `state` 里**独立出来**
       成为 `excluded` 标记：它是**用户意图**，与账号健康正交（被排除 ≠ 账号坏了）。
       `tier` 成本分层（free 0 / unknown 1 / paid 2）、`inFlight/maxInFlight` 在途租约、
       `modelCool` 模型级冷却、`willFail` 演示用的失败注入 —— 全部由 poolSection 派生。 */
    pool: [
      { account: "jackey", credits: 1240, weight: 100, state: "可用", sticky: true, tier: "free", inFlight: 1, maxInFlight: 3, willFail: "429" },
      { account: "team-shared", credits: 860, weight: 80, state: "可用", sticky: false, tier: "free", inFlight: 0, maxInFlight: 3,
        modelCool: [{ model: "gpt-5-codex", remain: 192 }] },
      { account: "qa-bot", credits: 120, weight: 20, state: "可用", low: true, sticky: false, tier: "paid", inFlight: 0, maxInFlight: 3,
        modelCool: [{ model: "gpt-5-codex", remain: 48 }] },
      { account: "backup-01", credits: 480, weight: 30, state: "可用", sticky: false, tier: "paid", inFlight: 3, maxInFlight: 3 },
      { account: "design-lab", credits: 320, weight: 0, state: "可用", sticky: false, tier: "free", inFlight: 0, maxInFlight: 3,
        excluded: true, excludedNote: "留给客户端自己用" },
    ],
    /* 模型级暂停：同一模型在 ≥2 个账号上先后被限流 ⇒ 该模型整体暂停，
       到期后只放**一个**请求做半开试探 —— 否则每个请求都要把每个账号各撞一遍。 */
    modelGate: { model: "gpt-5-codex", accounts: 2, remain: 42, phase: "paused" },
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
    models: [{ name: "claude-sonnet-4.5", provider: "anthropic", scope: "国际版", available: true, efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "high", credits: 1.33 }],
    catalog: { source: "cached", fetchedAt: "昨天 21:04", fetchedAbs: "2026-10-02 21:04", note: "本次探测超时（8s），使用上次成功的结果" },
    pool: [{ account: "jackey-global", credits: 720, weight: 100, state: "可用", sticky: true, tier: "free", inFlight: 0, maxInFlight: 2 }],
    logs: [{ time: "09:12:30", model: "claude-sonnet-4.5", account: "jackey-global", status: 200, ms: 1120, tokens: 640 }],
  },
  "trae-cn": {
    enabled: true, port: 57892, baseUrl: "http://127.0.0.1:57892",
    keys: [
      { id: "k5", name: "TraeWork Key", masked: "ts_live_77bb••••••••••••2f4a", scope: "国内版", created: "2026-09-18", lastUsed: "5 分钟前", requests: 892 },
      /* 已吊销（同 wb-cn 的 k2）：两平台各留一条，确认「恢复 + 删除」在两边同形。 */
      { id: "k6", name: "TraeCode Key", masked: "ts_live_e310••••••••••••9c77", scope: "国内版", created: "2026-09-26", lastUsed: "40 分钟前", requests: 254, revoked: true },
    ],
    /* 用**真实模型名**与**真实目录数值**（2026-10-19 `batch_get_detail_param` 实测）：
       `context` = `__dev` 通道实际窗口，`maxWindow` = Max Mode 上限（不存在为 null）。
       ★ 上限不是统一 1M：mimo 是 981504、Doubao-Turbo 与默认相同。 */
    models: [
      { name: "glm-5.3", provider: "trae", scope: "TraeWork", available: true, context: 200000, maxWindow: 1000000, efforts: ["light", "high", "extra_high"], defaultEffort: "high", credits: 0.83 },
      { name: "qwen3.8-max", provider: "trae", scope: "TraeWork", available: true, context: 200000, maxWindow: 1000000, efforts: ["light", "high"], defaultEffort: "high", credits: 0.2 },
      { name: "Doubao-Seed-Evolving", provider: "trae", scope: "TraeWork", available: true, context: 256000, maxWindow: 1000000, credits: 0.8 },
      { name: "Doubao-Seed-2.1-Turbo", provider: "trae", scope: "TraeWork", available: true, context: 256000, maxWindow: 256000, efforts: ["light", "high"], defaultEffort: "high", credits: 0.2 },
      { name: "mimo-v2.6-pro", provider: "trae", scope: "TraeCode", available: true, context: 200000, maxWindow: 981504, efforts: ["light", "high"], defaultEffort: "high", credits: 0.06 },
      { name: "kimi-k3", provider: "trae", scope: "TraeCode", available: true, context: 200000, maxWindow: 1000000, credits: 0.06 },
      /* 目录里没有这个模型 ⇒ 数字未知，清单只按客户端默认窗口声明。 */
      { name: "kimi-k2.6", provider: "trae", scope: "TraeCode", available: true, context: 200000, maxWindow: null, credits: 0.06 },
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
  "qoder-cn": {
    enabled: true, port: 57894, baseUrl: "http://127.0.0.1:57894",
    keys: [
      { id: "k7", name: "Qoder 默认 Key", masked: "qd_live_5b2e••••••••••••c14f", scope: "国内版", created: "2026-09-22", lastUsed: "3 分钟前", requests: 1176 },
      /* 已吊销（与 wb-cn 的 k2、trae-cn 的 k6 同形）：三个平台各留一条，
         确认「恢复 + 删除」这条三态动作在**每个**平台上都同形。 */
      { id: "k8", name: "Qoder CLI", masked: "qd_live_9a07••••••••••••4e83", scope: "国内版", created: "2026-10-02", lastUsed: "2 小时前", requests: 214, revoked: true },
    ],
    models: [
      { name: "Qwen3.8-Max", provider: "qoder", scope: "国内版", available: true, context: 200000, maxWindow: 1000000, efforts: ["low", "medium", "xhigh"], credits: 0.5 },
      { name: "GLM-5.1", provider: "qoder", scope: "国内版", available: true, context: 200000, maxWindow: 1000000, efforts: ["low", "medium"], credits: 0.3 },
      { name: "Kimi-K3", provider: "qoder", scope: "国内版", available: true, disabled: true, context: 200000, maxWindow: 1000000, credits: 0.1 },
      { name: "MiniMax-M3", provider: "qoder", scope: "国内版", available: true, context: 200000, maxWindow: 400000, credits: 0 },
      /* 区域独占模型（国际版独有）也列出来 —— 与 WB 网关页的跨区域清单同形，
         用来暴露「Key 出口与模型区域不匹配」这个真实存在的错配。 */
      { name: "Ultimate", provider: "qoder", scope: "国际版", available: true, efforts: ["low", "medium", "high"], defaultEffort: "high", credits: 0 },
    ],
    catalog: { source: "live", fetchedAt: "8 分钟前", fetchedAbs: "2026-10-14 16:35", note: null },
    pool: [
      { account: "jackey", credits: 1180, weight: 100, state: "可用", sticky: true },
      { account: "team-qoder", credits: 620, weight: 70, state: "可用", sticky: false },
      { account: "trial-01", credits: 80, weight: 15, state: "余额偏低", sticky: false },
      { account: "legacy-acc", credits: 0, weight: 0, state: "已排除", sticky: false },
    ],
    /* ★ 限额护栏（Qoder 独有；`quota` 缺席 = 该作用域没有护栏卡）。
       四个账号**故意各占一态**：正常 / 当日 token 已满 / 触到余额地板 / 余额耗尽停放
       —— 截图能同时看到四种徽标，也顺带证明「拦下必须给原因」这条不是空话。
       数值口径与后端 `quota_view` 同形（`tokens` / `credits` / `blocked` …）。 */
    quota: {
      date: "2026-10-21",
      limits: { reserveCredits: 200, dailyTokenLimit: 500000, dailyCreditLimit: 50, modelDailyTokenLimit: 0 },
      accounts: [
        { uid: "jackey", name: "jackey", tokens: 128430, credits: 18.42,
          perModel: [{ model: "Qwen3.8-Max", tokens: 96120 }, { model: "GLM-5.1", tokens: 32310 }],
          blocked: null, blockedReason: null, creditsRemaining: 1180 },
        { uid: "team-qoder", name: "team-qoder", tokens: 500000, credits: 62.5,
          perModel: [{ model: "Qwen3.8-Max", tokens: 500000 }],
          blocked: "daily_tokens", blockedReason: "已达当日 token 上限（500000 / 500000）", creditsRemaining: 620 },
        { uid: "trial-01", name: "trial-01", tokens: 42800, credits: 7.15,
          perModel: [{ model: "Qwen3.8-Max", tokens: 42800 }],
          blocked: "reserve", blockedReason: "已触发余额地板：剩余 80 ≤ 保留 200，本账号暂停使用", creditsRemaining: 80 },
        { uid: "legacy-acc", name: "legacy-acc", tokens: 3260, credits: 0.62,
          perModel: [{ model: "GLM-5.1", tokens: 3260 }],
          blocked: "parked", blockedReason: "余额耗尽停放中（明天 10:00 后自动恢复）：HTTP 402 余额不足",
          creditsRemaining: 0, parkedUntil: "明天 10:00" },
      ],
    },
    logs: [
      { time: "10:41:07", model: "Qwen3.8-Max", account: "jackey", status: 200, ms: 764, tokens: 1120 },
      { time: "10:39:52", model: "GLM-5.1", account: "team-qoder", status: 200, ms: 588, tokens: 486 },
      { time: "10:38:20", model: "Qwen3.8-Max", account: "jackey", status: 200, ms: 1310, tokens: 2874 },
      { time: "10:36:44", model: "MiniMax-M3", account: "trial-01", status: 429, ms: 96, tokens: 0 },
      { time: "10:35:11", model: "GLM-5.1", account: "jackey", status: 200, ms: 702, tokens: 934 },
    ],
  },
  "qoder-global": {
    enabled: false, port: 57895, baseUrl: "http://127.0.0.1:57895",
    keys: [{ id: "k9", name: "Qoder 国际版 Key", masked: "qd_live_3f61••••••••••••b7d2", scope: "国际版", created: "2026-09-30", lastUsed: "昨天", requests: 21 }],
    models: [
      { name: "Ultimate", provider: "qoder", scope: "国际版", available: true, efforts: ["low", "medium", "high"], defaultEffort: "high", credits: 0 },
      { name: "Qwen3.8-Max", provider: "qoder", scope: "国际版", available: true, efforts: ["low", "medium", "xhigh"], credits: 0.5 },
    ],
    catalog: { source: "cached", fetchedAt: "昨天 22:10", fetchedAbs: "2026-10-13 22:10", note: "本次探测超时（8s），使用上次成功的结果" },
    pool: [
      { account: "jackey-global", credits: 900, weight: 100, state: "可用", sticky: true },
      { account: "intl-spare", credits: 240, weight: 30, state: "余额偏低", sticky: false },
    ],
    /* 国际版：护栏默认关（三条都是 0）—— 用来演示「未启用」这一档文案，
       与国内版「已开启 + 有账号被拦」形成对照。 */
    quota: {
      date: "2026-10-21",
      limits: { reserveCredits: 0, dailyTokenLimit: 0, dailyCreditLimit: 0, modelDailyTokenLimit: 0 },
      accounts: [
        { uid: "jackey-global", name: "jackey-global", tokens: 41200, credits: 6.8,
          perModel: [{ model: "Ultimate", tokens: 41200 }], blocked: null, blockedReason: null, creditsRemaining: 900 },
        { uid: "intl-spare", name: "intl-spare", tokens: 0, credits: 0,
          perModel: [], blocked: null, blockedReason: null, creditsRemaining: 240 },
      ],
    },
    logs: [
      { time: "11:05:33", model: "Ultimate", account: "jackey-global", status: 200, ms: 918, tokens: 1416 },
      { time: "11:02:07", model: "Qwen3.8-Max", account: "jackey-global", status: 200, ms: 655, tokens: 522 },
    ],
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

/* ★★ P14 S4a · 插件侧会话仓（VS Code 插件 / CodeBuddy IDE）—— **不进上面这张表**。
 *
 * 上面那句「将来有第二个客户端时**零改动**地出现」**不成立**，这条注释在此更正：
 * 插件侧确实有会话复制能力了（S4a），但它与桌面版的模型**根本不同** ——
 *   · 桌面版：跨账号**副本组**（有账本、可双向同步、有「谁落后/谁分叉」）；
 *   · 插件侧：**一次性单向复制**，副本**恒取新 id** ⇒ 构不成可同步的副本组。
 * ⇒ 把它塞进 `SESSION_CLIENTS` 只会让会话页多出一个**永远空的组列表**（假客户端 Tab）。
 * 正确入口在**账号页**（对齐参考实现：插件/IDE 的会话复制随账号切换触发）。
 *
 * 数据形状对齐 `list_vscode_sessions` 的返回（`ExtSessionsResponse`）。
 * ⚠️ 三种状态**必须分开**，文案不同 —— 压成一个「没有会话」就是在撒谎：
 *   `ok`      = 有会话（`skipped > 0` ⇒ 有工作区索引读不出来，要**如实标注**）
 *   `empty`   = 装了客户端，但这个账号没有会话
 *   `missing` = **未检测到客户端数据目录**（`dataRoot: null`）
 */
const EXT_SESSIONS = {
  ok: {
    sourceUid: "jackey",
    dataRoot: "C:\\Users\\jackey\\AppData\\Local\\CodeBuddyExtension\\Data",
    skipped: 1,
    /* 按 `updatedAt` **降序**（最近更新的排前面）。 */
    sessions: [
      { id: "8f2a41c7", workspaceHash: "a91f…", title: "重构账号切换流程", updated: "12 分钟前", updatedAt: 1760000000000, type: "craft", hasHistory: true },
      { id: "3d70be55", workspaceHash: "a91f…", title: "修 issue #3 命名后缀", updated: "2 小时前", updatedAt: 1760000000000, type: "craft", hasHistory: true },
      { id: "c0149a72", workspaceHash: "77b2…", title: "会话副本的幂等性", updated: "昨天", updatedAt: 1760000000000, type: "craft", hasHistory: false },
    ],
  },
  empty: {
    sourceUid: "jackey",
    dataRoot: "C:\\Users\\jackey\\AppData\\Local\\CodeBuddyExtension\\Data",
    skipped: 0,
    sessions: [],
  },
  missing: { sourceUid: "jackey", dataRoot: null, skipped: 0, sessions: [] },
};

/* 复制目标候选 = **本区域里除源账号以外的全部账号**（由 `extSessionCopyHtml` 从
   `curScope().accounts` 现算，**不写死列表** —— 写死会让「多了一个账号但选不到」这种
   缺陷看不见）。跨区域**不**给候选：插件侧没有跨版本数据通道。 */

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
    { host: "qoder.com.cn", on: true, note: "Qoder CN" },
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
      /* `foot: "jitter"` = 本组底部挂「随机抖动预览」（由 demo-app.js 渲染）。
         预览必须与开关同屏，否则「打开就抖动」这件事在原型里看不见。 */
      { title: "自动签到", foot: "jitter", rows: [
        { label: "启用自动签到", desc: "每天定时为全部账号领取签到积分", type: "switch", value: true },
        { label: "执行时刻", desc: "每天在此时刻触发（本地时间）", type: "time", value: "08:00", bind: "checkin-time" },
        /* 新增：随机抖动（issue #27 的「另一支」）。默认**关** —— 不改变任何既有行为。 */
        { label: "随机抖动", desc: "打开后，每次触发在设定时刻前后随机偏移，避免每天都精确落在同一分钟", type: "jitter" },
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
    /* ★ issue #109：CodeBuddy CLI 的**接入方式**（二选一）。
       放设置页而不是账号卡：CLI 的凭据只有一份（`~/.codebuddy/settings.json`），
       这是**全局一次性配置**；账号卡上的 `cliControl` 是「按账号切默认账号」，另一层次。
       渲染走 `viewCliIntegration()`（`kind: "cli"`），因为它是分段器 + 条件行，
       不是 `rows` 能表达的静态行。 */
    id: "cli", label: "CodeBuddy CLI", kind: "cli",
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
/* 作用域展示名（平台名 + 区域名）—— **唯一定义在数据层**。
   ⚠️ 不能定义在 `demo-app.js`：`BACKUP_SECTIONS` 在本文件**加载时**就要取值，
   那时 `demo-app.js` 里的 `scopeLabel` 还不存在（两个 <script> 的顶层 const
   共享同一个全局词法环境，但求值顺序仍是数据层在前）。
   ⚠️ 也不许再写一份：`SCOPES[id]` 上**没有** `label` 字段，曾经写成
   `SCOPES[id].label` ⇒ 每行都渲染成空串（连 `aria-label` 都退化成「全选 」）。 */
const scopeDisplayName = (id) => {
  const s = SCOPES[id];
  const p = PLATFORMS[s.platform];
  return `${p.label} ${p.regions.find((r) => r.id === s.region).label}`;
};

const BACKUP_SECTIONS = Object.keys(SCOPES).map((id) => ({
  id,
  label: scopeDisplayName(id),
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

/* ─────────── 应用更新信息（升级前说明弹窗的唯一数据源）───────────
   侧栏版本行的「检查更新」与设置里的「检查更新」都读它：
     hasUpdate = false → toast「已是最新版本 v<current>」
     hasUpdate = true  → 打开**升级前说明弹窗**：先说清这一版改了什么，再由用户点「立即升级」

   ★ 形状即契约（`docs/specs/UPDATE-NOTES-ARCH.md`）：
     `breaking` / `sections` / `older` 三段都是**结构化纯文本**，前端不做 markdown 解析、
     不把 release body 直接 innerHTML —— 版本说明来自发布资产，属于外部输入。
   ★ 内容取自真实发布说明（`docs/releases/v2026.10.171833.md`），评审时看到的是真数据。
     把 `hasUpdate` 改成 false 可演示「已是最新」分支；`?err=update` 可演示失败分支。 */
const APP_UPDATE = {
  current: "2026.10.161110",
  hasUpdate: true,
  latest: "2026.10.171833",
  size: "27.4 MB",
  publishedAt: "2026-10-17T10:33:00Z",
  releaseUrl: "https://github.com/NextAgentX/qoder-trae-workbuddy-switch/releases/tag/v2026.10.171833",
  /* 升级前必读：会**改变既有行为**的项单独置顶（端口 / 数据格式 / 需要手动跟进的步骤）。
     空数组 = 不渲染该区块（不写「本版无必读项」这种恒显噪声）。 */
  breaking: [
    {
      title: "网关端口统一到 5789x",
      desc: "界面 57890 · WorkBuddy 网关 57891 · 统一入口 57892 · hook 57893 · Trae 网关 57894 · Qoder 网关 57895（Trae / Qoder 原为 7864 / 7865）。只改默认值：在 trae/api_gateway.json / qoder/gateway_config.json 里显式写过 port 的不受影响，客户端也不用动。",
    },
  ],
  /* 本版更新：按主题分组，组内每条是「一句话说清结果」，不是提交记录。 */
  sections: [
    {
      title: "网关",
      items: [
        "在途额度泄漏根治：请求被取消（客户端断开 / 超时）时额度一定归还，不再把整个池卡成「0 个可用」",
        "并发占满改回 503 pool_busy（带 Retry-After: 2），不再用误导性的 401",
        "排不上号时逐项摆出原因：已禁用 / 冷却中 / 余额为 0 / 被排除 / 在途已满，失败响应带 error.attempts",
        "首帧即错误信封时改为换号重试，不再白烧一次请求；上游 CRLF 换行的 SSE 也能正确切帧",
      ],
    },
    {
      title: "账号池",
      items: [
        "支持排除账号（自动选号 / 兜底 / 粘性 / 指定全部跳过），被排除的账号可一键恢复",
        "模型级限流冷却：同一账号「对 A 模型冷却、对 B 模型仍可服务」",
        "「可路由」格里标出「其中 N 个在途已满」—— 可用 ≠ 此刻跑得动",
      ],
    },
    {
      title: "定时任务与其它",
      items: [
        "定时任务时点支持分钟粒度，并可开启随机抖动（避免整点扎堆触发）",
        "Trae：对话复用稳定会话 ID（跨轮不再每次新开）＋ 转发抓取诊断旁路（默认关闭）",
        "macOS：修复 at-rest 加密信封账号解锁失败（可执行体名改读 Info.plist）",
        "README 改为图优先，并新增 6 种语言",
      ],
    },
  ],
  /* 更早版本：只在「你落后不止一版」时有意义，弹窗里默认折叠。
     真实实现由后端按 `current → latest` 区间回填，前端不自己拼。 */
  older: [
    {
      version: "2026.10.161110",
      publishedAt: "2026-10-16T03:10:00Z",
      items: [
        "统一网关总览标出「在途已满」，与账号池卡片同一口径",
        "账号池选号按模型限流状态过滤，冷却中的账号不再被派单",
        "演示夹具（desktop demo fixtures）与真实接口返回对齐",
      ],
    },
  ],
};

/* ─────────── 统一挂载派生用量数据 ───────────
   放在文件末尾：此时 USAGE / SCOPES / GATEWAY 全部已定义。 */
for (const _id of Object.keys(USAGE)) USAGE[_id].ext = buildUsageExtras(_id);

/* ─────────── 密钥策略（issue #76）───────────
   **与 `GATEWAY[...].keys` 分开放**：策略是「另一张表」，不是密钥记录上的一堆字段。
   真实实现里也是 `ApiKeyRecord` 旁边多一张策略/用量表（见 `specs/KEY-PERMISSION-UI-DESIGN.md`）。

   `quota.mode` = unlimited | limited；`unit` = token | request（业务代理没有 token）；
   `period` = none | daily | weekly | monthly（自然月对齐成本归属）。
   `scope.mode` = all | selected；`scope.accounts` 是**账号 id**（可按账号自带的
   `group` 整组选取 —— 分组是账号页已有的同一份数据，不另造）。 */
const KEY_POLICY = {
  k1: { quota: { mode: "limited", limit: 10000000, unit: "token", period: "monthly", used: 6200000 }, scope: { mode: "all" } },
  k2: { quota: { mode: "unlimited" }, scope: { mode: "all" } },
  k3: { quota: { mode: "limited", limit: 1000000, unit: "token", period: "none", used: 1000000 }, scope: { mode: "selected", accounts: ["a1"] } },
  k4: { quota: { mode: "unlimited" }, scope: { mode: "all" } },
  k5: { quota: { mode: "limited", limit: 50000, unit: "request", period: "daily", used: 43000 }, scope: { mode: "selected", accounts: ["t1", "t2"] } },
  k6: { quota: { mode: "unlimited" }, scope: { mode: "all" } },
  k7: { quota: { mode: "limited", limit: 20000000, unit: "token", period: "monthly", used: 2000000 }, scope: { mode: "all" } },
  k8: { quota: { mode: "unlimited" }, scope: { mode: "all" } },
  k9: { quota: { mode: "unlimited" }, scope: { mode: "all" } },
  k10: { quota: { mode: "limited", limit: 10000000, unit: "token", period: "monthly", used: 8600000 }, scope: { mode: "selected", accounts: ["a1", "a2", "a3"] } },
  /* 不限额：与改造前行为一致（消耗多少都放行）。 */
  k11: { quota: { mode: "unlimited" }, scope: { mode: "all" } },
};
const KEY_PERIOD_TEXT = { none: "不重置（累计）", daily: "每日 00:00 重置", weekly: "每周一 00:00 重置", monthly: "自然月结算" };
const KEY_PERIOD_SHORT = { none: "累计", daily: "每日", weekly: "每周", monthly: "自然月" };
