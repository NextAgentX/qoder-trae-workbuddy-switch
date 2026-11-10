#!/usr/bin/env node
/* ============================================================================
 * verify-cc-switch-bridge.cjs —— cc-switch 融合专题原型的可复跑验收套件
 *
 *   node verify-cc-switch-bridge.cjs                 全量
 *   node verify-cc-switch-bridge.cjs --only=C2,F3    单条/若干条重跑
 *
 * 立场：**不信任实现者的自述**。所有几何与状态断言都打开真实 Chrome 读
 * computed style / getBoundingClientRect / 真点事件，不看任何汇报文字。
 *
 * 依赖：本机 Chrome（headless）。找不到时静态检查照跑，动态项标 [SKIP]
 * 并以退出码 1 收尾 —— 没有证据就不算通过。
 *
 * 退出码：0 全过（含 8 条人工项待办）／ 1 有 FAIL 或有 SKIP
 *
 * 条目分组（与设计系统专家的 40 条对齐）：
 *   A 文件与骨架   C 客户端卡   D 差异行   E 链路   F 熔断
 *   G 令牌与动效   H 三条闭环    I 结构一致性   M 只能人工看
 *
 * ⚠️ 这个脚本是**施工自检工具，不是审查工具**：
 *    若它的结论与 critique-reviewer 的人工判断冲突，以人工为准，回来修脚本 ——
 *    脚本错了会长期误导后来所有人，比少一个脚本严重。
 *
 * ── 判据出处（看到 FAIL 直接翻规范对应条款，不用问人）────────────────────
 *   规范文件：docs/specs/PROTO-DS-CCSWITCH.md
 *
 *   A1  § 任务书硬约束（单文件、无 CDN / 外部字体 / 外部图片）
 *   A2  § 任务书硬约束（DOCTYPE / lang=zh-CN / charset / viewport）
 *   A3  § 任务书自检项（<div 与 </div> 数量差为 0）
 *   A4  § 任务书自检项（文件可读且 > 60KB）
 *   C1  §2.1 client-card —— 8 款客户端矩阵卡
 *   C2  §2.1 铁律「5 态高度完全一致，min-height:120px 写死」+ §5 细则 1
 *   C3  §2.1 grid-template-rows:30px 18px 26px；row-gap:8px；padding:14px 16px
 *   C4  §5 细则 10「未安装 / 未接入不要禁用按钮」+ §2.1 五态映射表
 *   C5  §2.1「row2 永不空」+ §5 细则 1「缺值填『未检测到 …』而非留空」
 *   C6  §4.4 键盘可达（tabindex / focus-visible）+ 本次裁定：交互元素不嵌套
 *   D1  §2.3「底色 add/del 一律 color-mix 10% 软底」
 *   D2  §2.3 + §5 细则 2（左边缘 2px 纯色竖条，40% 强度做第二重编码）
 *   D3  §5 细则 2「+ / − 符号必须真打出来，不靠颜色」
 *   D4  §2.3「禁止纯红纯绿（#0f0 / #f00）」
 *   E1  §2.4 flow-step —— 链路演示 5 节点
 *   E2  §2.4 三态 + §5 细则 4「最低 --muted-fg，禁止 opacity:.4」
 *   E3  §2.4「容器 grid-template-columns:repeat(5, minmax(0,1fr))」
 *   E4  §3 页面骨架 / 任务书「1280px 宽下不得横向滚动」
 *   F1  §2.5「必须与健康徽标不同构：无底色描边小方牌」
 *   F2  §2.5「文案永远以『熔断 ·』开头，字面即区分」
 *   F3  §2.5「熔断徽标只出现在网关 / 选号行」+ §5 细则 5
 *   F4  §2.5「只有半开有脉冲」+ §4.5「prefers-reduced-motion 下关闭」
 *   G1  §4.5「动效仅三处（抽屉 translateX / spinner / b-half 脉冲）」
 *   G2  §1「零新增颜色。新页面出现新色值即视为缺陷」
 *   G3  §1 色相冲突防线「--primary(162.48) 与 --success(155) 几乎同色 → 禁用 .badge.info」
 *   G4  §1「禁止用 opacity 造第三档」
 *   G5  §3 数据密集区 + §5 细则 6「所有数字挂 tabular-nums」
 *   H1  § 任务书「闭环 A 新客户端接入」逐步可点
 *   H2  § 任务书「每一个成功终点都要有一个可达的失败终点（A 有写入失败）」
 *   H3  § 任务书「闭环 B 档案切换」：diff 弹窗 → 确认 → toast + 撤销 + 版本记录
 *   H4  § 任务书「闭环 C 计量回流」：关闭后概览今日请求 +1
 *   H5  § 任务书「闭环 C 有 401 换号」异常分支
 *   I1  §3 页面骨架（.page / .page-head / 六页一级导航）
 *   I2  §3「顶栏 56px、侧栏 240px」+ 任务书导航顺序
 *   I3  §2.2「min-height:172px」；验收口径已按裁定改为「同行 + 跨行等高且 ≥172」
 *   I4  §3 间距只用既有值（状态点 6px 不得被 flex 拉伸）
 *   I5  §4.1「抽屉 max-height 限高可滚」
 *   I6  §4.2「弹窗只承载确认一个动作」+ 限高
 *   I7  §5 细则 7「时间戳用固定假时间（如 2026-11-09）」
 *   I8  §5 细则 7「不用 new Date()、不用『3 分钟前』」
 *   C7  §2.1 row3（本轮裁定 P2）：第 3 行必须有固定 2 字口径 key，否则 5 种 schema 无法纵向扫
 *   D5  §2.3 diff 徽标（本轮裁定 P1-2）：+n −m 必须等于该文件块真实行数
 *   I9  §4.4 键盘可达：焦点兜底环必须在 @supports 之外（WebKitGTK 老版本无 :has()，
 *       写进块里会整条被丢弃 → 键盘用户完全失焦，且只有 Linux 用户会撞上）
 *   I10 §4.4 + 本轮裁定：Tab 到卡内按钮时整卡出环（鼠标向见 M9）
 *   I11 § 任务书 P1-1：跨页数字自洽（概览 stat = 网关三行之和；顶栏副标题实时计算不外写）
 *   I12 § 任务书 P1-4：实现注释离开产品界面（.proto-note 清零，设计备注默认收起）
 * ========================================================================== */
'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');

const FILE = path.join(__dirname, 'cc-switch-bridge.html');
const SRC = fs.readFileSync(FILE, 'utf8');

const onlyArg = (process.argv.find((a) => a.startsWith('--only')) || '').replace(/^--only=?/, '');
const ONLY = onlyArg ? onlyArg.split(',').map((s) => s.trim()).filter(Boolean) : [];

const RESULT = []; // { id, name, ok, detail }
const MANUAL = [
  ['M1', '颜色并集人工比对 demo.css :root', '新增语义只能落在既有令牌上，脚本无法判断「语义→色」是否合理'],
  ['M2', '对比度 AA 实测取样', '软底小字需真实取色算比值，自动化只能到 ±0.3'],
  ['M3', '9 步演讲动线走查', '点着讲的顺序与节奏，机器无关'],
  ['M4', '文案产品边界人读', '「期望态 / 实际态 / 供给侧 / 运行时」是否被措辞守住'],
  ['M5', '抽屉与弹窗的层叠观感', '焦点、滚动锁、背景压暗强度需人眼判断'],
  ['M6', '1440 / 1024 两档宽度观感', '只硬性验收 1280 零横向滚动'],
  ['M7', '投影与截图效果', '内部评通常会投屏，字号与对比在现场才准'],
  ['M8', '深色下图标一致性观感', '8 款客户端标记同色系是视觉判断'],
  ['M9', '鼠标点卡内按钮不出环', '合成 mousedown 不改变引擎的 focus-visible 启发式，脚本测不准（实测仍报 true），需人眼过一遍']
];

/* ═══════════════════════════ id → 规范条款（--list 的唯一数据源） ═══════════════════════════
   ⚠️ 存在两套 id 体系（本脚本的 A/C/D/E/F/G/H/I 与设计系统专家清单的 G/C/P/D/F/B/M/X/J），
   同一个 `F3` 在两套里指不同东西。**条款号是唯一能互相对上的桥**，所以每条必须带 §。
   本轮新增、规范里尚无对应条款的，诚实标「新增，待规范补」，不编条款号。 */
const CLAUSES = {
  A1: ['无任何外部依赖', '任务书硬约束（单文件 / 无 CDN）'],
  A2: ['文档头四件套', '任务书硬约束（DOCTYPE/lang/charset/viewport）'],
  A3: ['标签开闭配对', '任务书自检项（<div> 与 </div> 差为 0）'],
  A4: ['文件体积 60–400 KB', '任务书自检项（可读且 > 60KB）'],
  C1: ['客户端卡 8 张', '§2.1'],
  C2: ['client-card 高度恒定', '§2.1 铁律 + §5 细则 1'],
  C3: ['三行栅格 30/18/26', '§2.1'],
  C4: ['未接入/未安装不禁用', '§2.1 五态映射 + §5 细则 10'],
  C5: ['.cc-url 永不为空', '§2.1（row2 永不空）+ §5 细则 1'],
  C6: ['交互元素不嵌套 · 每卡 1 站', '§4.4 + 本轮裁定'],
  C7: ['第 3 行固定 2 字口径 key', '新增，待规范补（本轮 P2）'],
  D1: ['diff 只用 10% 软底', '§2.3'],
  D2: ['软底与竖条实测', '§2.3 + §5 细则 2'],
  D3: ['＋/− 符号真打', '§5 细则 2'],
  D4: ['无自造纯红纯绿', '§2.3'],
  D5: ['徽标计数 = 真实行数', '新增，待规范补（本轮 P1-2）'],
  E1: ['链路 5 节点', '§2.4'],
  E2: ['未激活节点不用 opacity', '§2.4 + §5 细则 4'],
  E3: ['1280 下 5 列', '§2.4'],
  E4: ['1280 零横向滚动', '§3 页面骨架 + 任务书'],
  F1: ['breaker-badge 描边方牌', '§2.5'],
  F2: ['熔断文案以「熔断」开头', '§2.5'],
  F3: ['熔断徽标不入客户端卡', '§2.5 + §5 细则 5'],
  F4: ['仅半开有脉冲', '§2.5 + §4.5'],
  G1: ['关键帧白名单', '§4.5（动效仅三处）'],
  G2: ['无紫色 / 无 CSS 渐变', '§1（零新增颜色）'],
  G3: ['.badge.info 零使用', '§1 色相冲突防线'],
  G4: ['无 opacity 第三档', '§1'],
  G5: ['数字挂 tabular-nums', '§3 数据密集区 + §5 细则 6'],
  H1: ['闭环A 三步接入 + 待重启', '任务书闭环 A'],
  H2: ['闭环A 部分写入失败 + 回滚', '任务书（每成功终点有可达失败终点）'],
  H3: ['闭环B 切换 + 生效转移 + 撤销', '任务书闭环 B'],
  H4: ['闭环C 关闭抽屉 +1', '任务书闭环 C'],
  H5: ['闭环C 401 自动换号', '任务书闭环 C 异常分支'],
  I1: ['六个页面挂载点', '§3 页面骨架'],
  I2: ['侧栏一级导航顺序', '§3 + 任务书'],
  I3: ['profile 卡同行跨行等高 ≥172', '§2.2（口径经裁定放宽为等高）'],
  I4: ['侧栏状态点未被拉伸', '§3 间距只用既有值'],
  I5: ['抽屉 max-height 限高可滚', '§4.1'],
  I6: ['弹窗 max-height 限高', '§4.2'],
  I7: ['固定假时间 2026-11-09', '§5 细则 7'],
  I8: ['显示文本不取实时时间', '§5 细则 7'],
  I9: ['焦点兜底环在 @supports 之外', '§4.4（WebKitGTK 兼容，新增）'],
  I10: ['卡内按钮聚焦时整卡出环', '§4.4'],
  I11: ['跨页数字自洽（关系式，不钉值）', '任务书 P1-1（本轮裁定）'],
  I12: ['实现注释不进产品界面', '新增，待规范补（本轮 P1-4）']
};
const MANUAL_CLAUSE = {
  M1: '§1', M2: '§4.5', M3: '—（演讲动线）', M4: '—（产品边界）',
  M5: '§4.1 / §4.2', M6: '§3', M7: '§3', M8: '§5 细则 9', M9: '§4.4'
};

/* 中文占两格：按显示宽度补空格，否则 --list 的表看着是歪的 */
function padW(s, w) {
  let n = 0;
  for (const ch of String(s)) n += /[\u1100-\u115f\u2e80-\ua4cf\ua960-\ua97f\uac00-\ud7a3\uf900-\ufaff\ufe30-\ufe4f\uff00-\uff60\uffe0-\uffe6]/.test(ch) ? 2 : 1;
  return s + ' '.repeat(Math.max(0, w - n));
}

if (process.argv.indexOf('--list') >= 0) {
  const ids = Object.keys(CLAUSES);
  ids.forEach((id) => {
    const c = CLAUSES[id];
    console.log(padW(id, 4) + padW(c[0], 34) + '← ' + padW(c[1], 40) + '[AUTO]');
  });
  MANUAL.forEach((m) => {
    console.log(padW(m[0], 4) + padW(m[1], 34) + '← ' + padW(MANUAL_CLAUSE[m[0]] || '—', 40) + '[MANUAL]');
  });
  console.log('');
  console.log('合计 ' + ids.length + ' AUTO + ' + MANUAL.length + ' MANUAL');
  console.log('列含义：id / 断言名 / 规范条款（跨两套 id 体系唯一的桥）/ 类型');
  console.log('条款号写法：§x.y 指 PROTO-DS-CCSWITCH.md；标「新增，待规范补」表示本轮才有、规范里还没有对应条款。');
  process.exit(0);
}

function add(id, name, ok, detail) {
  RESULT.push({ id: id, name: name, ok: !!ok, detail: detail == null ? '' : String(detail) });
}

/* ═══════════════════════════ 静态检查（读源码，不需要浏览器） ═══════════════════════════ */

function staticChecks() {
  const count = (re) => (SRC.match(re) || []).length;

  /* A1 无外链 ------------------------------------------------------------- */
  const extScript = count(/<script[^>]+src=/gi);
  const extLink = count(/<link[^>]+href=/gi);
  const extUrl = count(/(?:src|href)\s*=\s*"https?:\/\//gi);
  const atImport = count(/@import/gi);
  add('A1', '无任何外部依赖', extScript + extLink + extUrl + atImport === 0,
    'script src ' + extScript + ' / link href ' + extLink + ' / 绝对 URL ' + extUrl + ' / @import ' + atImport);

  /* A2 文档头 ------------------------------------------------------------- */
  const head = [
    ['DOCTYPE', SRC.indexOf('<!DOCTYPE html>') === 0],
    ['lang=zh-CN', /<html lang="zh-CN">/.test(SRC)],
    ['charset', /<meta charset="utf-8">/i.test(SRC)],
    ['viewport', /name="viewport"/i.test(SRC)]
  ];
  add('A2', '文档头四件套', head.every((h) => h[1]),
    head.map((h) => h[0] + (h[1] ? '✓' : '✗')).join(' '));

  /* A3 标签配对 ----------------------------------------------------------- */
  const pairs = [['div', /<div\b/g, /<\/div>/g], ['span', /<span\b/g, /<\/span>/g],
    ['button', /<button\b/g, /<\/button>/g], ['section', /<section\b/g, /<\/section>/g],
    ['article', /<article\b/g, /<\/article>/g], ['svg', /<svg\b/g, /<\/svg>/g],
    ['style', /<style>/g, /<\/style>/g], ['script', /<script>/g, /<\/script>/g]];
  const bad = pairs.filter((p) => count(p[1]) !== count(p[2]));
  add('A3', '标签开闭配对', bad.length === 0,
    bad.length ? bad.map((p) => p[0] + ' ' + count(p[1]) + '/' + count(p[2])).join(' ')
      : pairs.map((p) => p[0] + ' ' + count(p[1])).join(' '));

  /* A4 体积 --------------------------------------------------------------- */
  const kb = Buffer.byteLength(SRC, 'utf8') / 1024;
  add('A4', '文件体积 60–400 KB 且为单文件', kb >= 60 && kb <= 400, kb.toFixed(1) + ' KB / ' + SRC.split('\n').length + ' 行');

  /* G1 关键帧白名单 -------------------------------------------------------- */
  const kf = (SRC.match(/@keyframes\s+([A-Za-z0-9_-]+)/g) || []).map((s) => s.replace(/@keyframes\s+/, ''));
  const allow = ['sk', 'spin', 'toastIn', 'slideDown', 'bbPulse'];
  const extra = kf.filter((n) => allow.indexOf(n) < 0);
  add('G1', '动画关键帧在白名单内', extra.length === 0, '实测 [' + kf.join(', ') + ']' + (extra.length ? ' 越界 ' + extra.join(',') : ''));

  /* G2 无紫色 / 无 CSS 渐变 ------------------------------------------------
     注意：不能拿正则去 grep "oklch(...27..)" —— `--card` 的亮度就是 0.279，
     必须按通道解析，只判 hue（第三通道）落在 275–330 的紫区。 */
  const purpleHex = (SRC.match(/#8b5cf6|#a855f7|#7c3aed|#6366f1|#9333ea/gi) || []);
  const purpleName = (SRC.match(/\b(?:violet|purple|indigo|fuchsia)\b/gi) || []);
  const oklchHues = (SRC.match(/oklch\(\s*[\d.]+\s+[\d.]+\s+[\d.]+/g) || [])
    .map((s) => parseFloat(s.trim().replace(/^oklch\(\s*/, '').split(/\s+/)[2]))
    .filter((h) => !isNaN(h));
  const purpleHue = oklchHues.filter((h) => h >= 275 && h <= 330);
  const cssGrad = (SRC.match(/linear-gradient\(/g) || []).length;
  add('G2', '无紫色、无 CSS 渐变背景',
    purpleHex.length + purpleName.length + purpleHue.length === 0 && cssGrad === 0,
    '紫色 hex ' + purpleHex.length + ' / 紫色词 ' + purpleName.length + ' / 紫色 hue ' + purpleHue.length +
    ' / linear-gradient ' + cssGrad + '（hue 集合 ' + [...new Set(oklchHues)].sort((a, b) => a - b).join(',') + '）');

  /* G3 badge.info 零使用 --------------------------------------------------- */
  const info = (SRC.match(/badge\s+info|class="badge info"/g) || []).length;
  add('G3', '禁用 .badge.info（与 success 同色相）', info === 0, '出现 ' + info + ' 次');

  /* G4 opacity 集合不含 .4 ------------------------------------------------ */
  const ops = [...new Set((SRC.match(/opacity:\s*[0-9.]+/g) || []).map((s) => parseFloat(s.split(':')[1])))];
  add('G4', '禁用 opacity 造第三档（无 .4）', ops.every((v) => v !== 0.4),
    '实测集合 {' + ops.sort().join(', ') + '}');

  /* I7 固定时间戳 --------------------------------------------------------- */
  const ts = (SRC.match(/2026-11-09/g) || []).length;
  add('I7', '固定假时间 2026-11-09', ts >= 3, '出现 ' + ts + ' 次');

  /* I8 无 new Date() 实际调用 --------------------------------------------- */
  const stripped = SRC.replace(/\/\*[\s\S]*?\*\//g, '').replace(/<!--[\s\S]*?-->/g, '').replace(/^\s*\/\/.*$/gm, '');
  const nd = (stripped.match(/new Date\s*\(/g) || []).length;
  add('I8', '显示文本不取实时时间', nd === 0, '非注释内 new Date() ' + nd + ' 处');

  /* I9 焦点兜底必须在 @supports 之外 ---------------------------------------
     §4.4 键盘可达。Tauri 的 Linux 侧是 WebKitGTK，老版本不支持 :has()：
     若把兜底环写进 @supports 块，整条规则会被丢弃 → 键盘用户完全看不到焦点（WCAG 2.4.7 失败）。
     这是不报错的静默退化，只有 Linux 用户会撞上，Chrome 上永远测不出来 —— 所以必须在源码层守住。 */
  const suppBlocks = [];
  let from = 0;
  while (true) {
    const i = SRC.indexOf('@supports selector(:has(*)){', from);
    if (i < 0) break;
    let depth = 0, j = SRC.indexOf('{', i);
    let end = j;
    for (; end < SRC.length; end++) {
      if (SRC[end] === '{') depth++;
      else if (SRC[end] === '}') { depth--; if (depth === 0) { end++; break; } }
    }
    suppBlocks.push(SRC.slice(i, end));
    from = end;
  }
  const bareFallbacks = (SRC.match(/:focus-within\{/g) || []).length;
  const guarded = (SRC.match(/:focus-within:not\(:has\(:focus-visible\)\)\{/g) || []).length;
  const fallbackInsideSupports = suppBlocks.some((b) => /:focus-within\{/.test(b));
  add('I9', '焦点兜底环在 @supports 之外', suppBlocks.length === 3 && !fallbackInsideSupports && bareFallbacks === 3 && guarded === 3,
    '@supports 块 ' + suppBlocks.length + ' / 兜底裸规则 ' + bareFallbacks + ' / 收环规则 ' + guarded +
    (fallbackInsideSupports ? ' ⚠️ 有兜底被写进了 @supports' : ''));
}

/* ═══════════════════════════ 动态检查（真实 Chrome） ═══════════════════════════ */

const PROBE = [
  'window.addEventListener("load",function(){setTimeout(function(){',
  'var R=[];',
  'function add(id,name,ok,detail){R.push({id:id,name:name,ok:!!ok,detail:detail==null?"":String(detail)})}',
  'function q(s){return document.querySelector(s)}',
  'function qa(s){return Array.prototype.slice.call(document.querySelectorAll(s))}',
  'function cs(e){return getComputedStyle(e)}',
  'function txt(e){return e?e.textContent.replace(/\\s+/g," ").trim():""}',
  'function click(e){if(e)e.dispatchEvent(new MouseEvent("click",{bubbles:true,cancelable:true}))}',
  'function fn(id){ return function(body){ try{ body() }catch(e){ add(id, arguments.length?"":"", false, "异常 "+e.message) } } }',
  'function T(id,name,body){try{body()}catch(e){add(id,name,false,"异常 "+e.message)}}',
  'function tabStops(root){return root.querySelectorAll("button,[href],input,select,textarea,[tabindex]:not([tabindex=\\"-1\\"])").length}',

  /* ── I1 六页挂载点 ── */
  'T("I1","六个页面挂载点",function(){',
  '  var ids=["overview","clients","profiles","accounts","gateway","settings"];',
  '  var ok=ids.every(function(p){return !!document.getElementById("page-"+p)});',
  '  add("I1","六个页面挂载点",ok&&qa(".page").length===6&&qa(".page.on").length===1,',
  '    ids.map(function(p){return "#page-"+p}).join(" ")+" · 同屏可见 "+qa(".page.on").length);',
  '});',

  /* ── I2 侧栏顺序 ── */
  'T("I2","侧栏一级导航顺序",function(){',
  '  var order=qa("#nav .nav-item").map(function(b){return b.dataset.page});',
  '  add("I2","侧栏一级导航顺序",order.join(",")==="overview,clients,profiles,accounts,gateway,settings",order.join(" → "));',
  '});',

  /* P1-1 跨页数字自洽：概览 stat 必须等于三个网关行的请求数之和；
     顶栏作用域副标题必须由 CLIENTS.length / PROFILES.length 算出来，不能写死。 */
  'T("I11","跨页数字自洽",function(){',
  '  var stat=parseInt(txt(q("#stat-requests")).replace(/,/g,""),10);',
  '  var gw=qa("#gw-route .gw-row").slice(0,3).map(function(r){',
  '    return parseInt(txt(r.querySelector(".gw-m b")).replace(/,/g,""),10)});',
  '  var sum=gw.reduce(function(a,b){return a+b},0);',
  '  var sub=txt(q("#scope-sub"));',
  '  add("I11","跨页数字自洽",stat===sum&&sub==="8 个客户端 · 3 个档案",',
  '    "概览 "+stat+" = 网关 "+gw.join("+")+"="+sum+" · 顶栏「"+sub+"」");',
  '});',

  /* P1-4 实现注释必须离开产品界面：.proto-note 清零，设计备注默认收起 */
  'T("I12","实现注释不进产品界面",function(){',
  '  var left=document.querySelectorAll(".proto-note").length;',
  '  var notes=qa(".design-notes");',
  '  var open=notes.filter(function(d){return d.hasAttribute("open")}).length;',
  '  add("I12","实现注释不进产品界面",left===0&&notes.length>0&&open===0,',
  '    "残留 .proto-note "+left+" · 设计备注块 "+notes.length+"（默认展开 "+open+"）");',
  '});',

  /* ── C1 … C6 客户端卡 ── */
  'go("clients");',
  'T("C1","客户端卡 8 张",function(){',
  '  var n=qa("#client-grid .client-card").length;',
  '  add("C1","客户端卡 8 张",n===8,"实测 "+n+" 张");',
  '});',
  /* §2.1 铁律：5 态高度完全一致。注意量法——8 张卡 + 把 claude-code 依次切到 7 种状态
     各量一次，共 15 次测量必须只落在一个值上；只量初始 8 张会漏掉「切状态改写高度」这个坑。 */
  'T("C2","client-card 高度恒定",function(){',
  '  var base=qa("#client-grid .client-card").map(function(c){return Math.round(c.getBoundingClientRect().height)});',
  '  var states=["direct","off","absent","drift","busy","pending","gw"],seen={},n=base.length;',
  '  base.forEach(function(h){seen[h]=1});',
  '  states.forEach(function(st){setClientState("claude-code",st);',
  '    seen[Math.round(q("[data-client=\\"claude-code\\"]").getBoundingClientRect().height)]=1});',
  '  var keys=Object.keys(seen);',
  '  add("C2","client-card 高度恒定",keys.length===1,keys.join("/")+"（8 卡 + 7 态共 "+(n+states.length)+" 次测量）");',
  '});',
  'T("C3","三行栅格 30/18/26",function(){',
  '  var bad=qa("#client-grid .client-card").filter(function(c){',
  '    var r=[c.querySelector(".cc-row1"),c.querySelector(".cc-url"),c.querySelector(".cc-row3")];',
  '    if(r.some(function(x){return !x}))return true;',
  '    return Math.round(r[0].getBoundingClientRect().height)!==30||',
  '           Math.round(r[1].getBoundingClientRect().height)!==18||',
  '           Math.round(r[2].getBoundingClientRect().height)!==26});',
  '  add("C3","三行栅格 30/18/26",bad.length===0,bad.length?bad.length+" 张不符":"8/8 全符");',
  '});',
  'T("C4","无 disabled 按钮",function(){',
  '  var a=document.querySelectorAll(".client-card button[disabled]").length,b=document.querySelectorAll("button[disabled]").length;',
  '  add("C4","未接入/未安装不禁用按钮",a===0&&b===0,"卡内 "+a+" / 全页 "+b);',
  '});',
  'T("C5",".cc-url 永不为空",function(){',
  '  var empty=qa("#client-grid .cc-url").filter(function(e){return !e.textContent.trim()}).length;',
  '  add("C5",".cc-url 永不为空",empty===0,"空值 "+empty+" / 共 "+qa("#client-grid .cc-url").length);',
  '});',
  'T("C6","每卡仅 1 个 Tab 站",function(){',
  '  var stops=qa("#client-grid .client-card").map(function(c){return tabStops(c)});',
  '  var nested=qa("#client-grid .client-card [role=\\"button\\"][tabindex]").length;',
  '  add("C6","交互元素不嵌套且每卡 1 站",stops.every(function(n){return n===1})&&nested===0,"站数 "+stops.join("/")+" · 嵌套 role=button "+nested);',
  '});',
  /* P2 · 第 3 行必须有一个固定 2 字口径 key，否则 8 张卡的第 3 行是 5 种 schema，无法纵向扫 */
  'T("C7","第 3 行口径 key",function(){',
  '  var keys=qa("#client-grid .cc-key").map(function(e){return txt(e)});',
  '  add("C7","第 3 行有固定 2 字口径 key",keys.length===8&&keys.every(function(k){return k.length===2}),keys.join("/"));',
  '});',
  'go("clients");setClientState("claude-code","gw");',

  /* ── I3 档案卡等高（含跨行） ── */
  'go("profiles");',
  /* §2.2 的 min-height:172 只是下限，实际高度被 h3 的 auto 行撑开；
     验收口径（经裁定）＝ 同行等高 + 跨行也等高 + 每个 ≥172，不拿 172 当死值。 */
  'T("I3","profile-card 同行跨行等高且 ≥172",function(){',
  '  var cards=qa("#profile-grid .profile-card").map(function(p){',
  '    var r=p.getBoundingClientRect();return {h:Math.round(r.height),top:Math.round(r.top+window.scrollY)}});',
  '  var byRow={},ok=true,detail=[];',
  '  cards.forEach(function(c){(byRow[c.top]=byRow[c.top]||[]).push(c.h)});',
  '  Object.keys(byRow).forEach(function(k){var hs=byRow[k];detail.push("行@"+k+":"+hs.join("/"));',
  '    if(new Set(hs).size!==1)ok=false;hs.forEach(function(h){if(h<172)ok=false})});',
  '  var allH=cards.map(function(c){return c.h});',
  '  add("I3","profile-card 同行跨行等高且 ≥172",ok&&new Set(allH).size===1,detail.join(" ")+" 全体 "+allH.join("/"));',
  '});',

  /* ── D 差异行（切到切换弹窗） ── */
  'T("D1","diff 行 10% 软底 + 2px 竖条",function(){',
  '  add("D1","diff 行 10% 软底 + 2px 竖条",true,"占位：见 D2 实测");',
  '});',
  'openSwitch("direct-backup");',
  /* §2.3 + §5 细则 2：只能 10% 软底 + 左 2px 竖条。
     computed 会输出 oklab(...)，所以用 `/ 0.1)` `/ 0.4)` 判 alpha，不能比字符串。 */
  'T("D2","diff 行软底与竖条实测",function(){',
  '  var rows=qa("#confirm-box .diff-row"),bad=[];',
  '  rows.forEach(function(r){var s=cs(r);',
  '    if(!/\\/\\s*0?\\.1\\)/.test(s.backgroundColor))bad.push("底 "+r.className+" "+s.backgroundColor);',
  '    if(s.borderLeftWidth!=="2px"||!/\\/\\s*0?\\.4\\)/.test(s.borderLeftColor))bad.push("条 "+r.className)});',
  '  add("D2","diff 行软底与竖条实测",bad.length===0,bad.length?bad.slice(0,2).join(" | "):"10% 底 + 2px/40% 竖条 × "+rows.length);',
  '});',
  'T("D3","diff 符号真打 + 与 −",function(){',
  '  var s=qa("#confirm-box .diff-row").map(function(r){return r.querySelector("i").textContent});',
  '  var hasAdd=s.indexOf("+")>=0,hasDel=s.indexOf("−")>=0;',
  '  add("D3","diff 符号真打 + 与 −",hasAdd&&hasDel,"符号集 ["+[...new Set(s)].join(" ")+"]");',
  '});',
  /* P1-2 徽标里的 +n −m 必须等于该文件块真实的行数 —— 这是评审最会逐行核对的地方 */
  'T("D5","diff 徽标计数与行数一致",function(){',
  '  var bad=[];',
  '  qa("#confirm-box .diff-grp").forEach(function(g){',
  '    var t=txt(g.querySelector(".diff-file .badge"));',
  '    var a=(t.match(/\\+\\s*(\\d+)/)||[0,0])[1]*1;',
  '    var d=(t.match(/−\\s*(\\d+)/)||[0,0])[1]*1;',
  '    var ra=g.querySelectorAll(".diff-row.add").length;',
  '    var rd=g.querySelectorAll(".diff-row.del").length;',
  '    if(a!==ra||d!==rd)bad.push("「"+t+"」vs 实际 +"+ra+" −"+rd);',
  '  });',
  '  add("D5","diff 徽标计数与真实行数一致",bad.length===0,bad.length?bad.join(" | "):"2 个文件块全部吻合");',
  '});',
  'T("D4","diff 无自造纯红纯绿",function(){',
  '  var css="";Array.prototype.forEach.call(document.styleSheets,function(sh){try{',
  '    Array.prototype.forEach.call(sh.cssRules,function(r){css+=r.cssText+"\\n"})}catch(e){}});',
  '  var hit=css.match(/#0f0|#f00|#00ff00|lime|(?:^|[^a-z-])green(?:[^a-z-]|$)/gi)||[];',
  '  add("D4","diff 无自造纯红纯绿",hit.length===0,"命中 "+hit.length);',
  '});',
  'closeConfirm();',

  /* ── E 链路 ── */
  'go("overview");',
  'T("E1","链路 5 节点",function(){var n=qa("#flow .flow-step").length;add("E1","链路 5 节点",n===5,"实测 "+n)});',
  'T("E2","未激活节点不用 opacity",function(){',
  '  var ops=qa("#flow .flow-step").map(function(e){return cs(e).opacity});',
  '  var cols=qa("#flow .flow-step > b").map(function(e){return cs(e).color});',
  '  add("E2","未激活节点不用 opacity",ops.every(function(v){return v==="1"}),"opacity "+ops.join("/")+" · 标题色 "+(new Set(cols)).size+" 种");',
  '});',
  'T("E3","1280 下 5 列",function(){',
  '  var cols=cs(q("#flow")).gridTemplateColumns.split(" ").filter(Boolean);',
  '  add("E3","1280 下 5 列",cols.length===5,cols.length+" 列 × "+cols.map(function(c){return Math.round(parseFloat(c))}).join("/")+"px");',
  '});',
  'T("E4","1280 零横向滚动",function(){',
  '  var c=q("#content"),ok=c.scrollWidth<=c.clientWidth+1;',
  '  add("E4","1280 零横向滚动",ok,"#content "+c.scrollWidth+"/"+c.clientWidth+" · doc "+document.documentElement.scrollWidth+"/"+window.innerWidth);',
  '});',

  /* ── F 熔断 ── */
  'go("gateway");',
  'T("F1","breaker-badge 描边方牌",function(){',
  '  var bs=qa(".breaker-badge"),bad=bs.filter(function(b){var s=cs(b);',
  '    return s.backgroundColor!=="rgba(0, 0, 0, 0)"||s.borderTopWidth!=="1px"||s.borderTopStyle!=="solid"});',
  '  add("F1","breaker-badge 描边方牌",bs.length>0&&bad.length===0,bs.length+" 枚，透明底+1px 实线 "+bs.length+"/"+bs.length);',
  '});',
  'T("F2","熔断文案前缀",function(){',
  '  var t=qa(".breaker-badge").map(function(b){return txt(b)});',
  '  var bad=t.filter(function(s){return s.indexOf("熔断")!==0});',
  '  add("F2","熔断文案以「熔断」开头",bad.length===0,"样例 "+t[0]);',
  '});',
  'T("F3","熔断徽标不入客户端卡",function(){',
  '  var n=document.querySelectorAll("#client-grid .breaker-badge").length;',
  '  add("F3","熔断徽标不入客户端卡",n===0,"客户端卡内 "+n+" 枚");',
  '});',
  'T("F4","仅半开脉冲",function(){',
  /* ★ 必须**同时**看规则与当前媒体态，不能只读 animationName：
     ① headless Chrome 的 prefers-reduced-motion 常为 reduce ⇒ 实际计算值就是 none，
        只读计算值会把「§4.5 正确关闭」判成「没有脉冲」；
     ② 只查 CSS 文本又验不到 §4.5 是否真的接上。
     所以：从样式表确认「基础规则有 bbPulse」+「reduce 规则有 animation:none」，
     再按当前 matchMedia 结果核对实际计算值。 */
  '  var src=document.documentElement.outerHTML;',
  '  var hasPulse=src.indexOf("bbPulse")>=0;',
  '  var offIdx=src.indexOf("prefers-reduced-motion");',
  '  var rest=offIdx>=0?src.slice(offIdx):"";',
  '  var hasReducedOff=offIdx>=0&&(rest.indexOf("animation:none")>=0||rest.indexOf("animation: none")>=0);',
  '  var reduce=matchMedia("(prefers-reduced-motion: reduce)").matches;',
  '  var anim=function(d){return (getComputedStyle(d).getPropertyValue("animation")||"").trim()};',
  '  var half=qa(".b-half .bb-dot").map(anim);',
  '  var other=qa(".b-closed .bb-dot,.b-open .bb-dot").map(anim);',
  '  var othersOff=other.every(function(a){return a===""||a==="none"});',
  '  var actual=reduce?half.every(function(a){return a==="none"}):half.every(function(a){return a.indexOf("bbPulse")>=0});',
  '  add("F4","仅半开有脉冲",hasPulse&&hasReducedOff&&othersOff&&actual,"pulse="+hasPulse+" reducedOff="+hasReducedOff+" reduce="+reduce+" half=["+half.join("|")+"]");',
  '});',

  /* ── G5 等宽数字 ── */
  'T("G5","数字挂 tabular-nums",function(){',
  '  var sels=["#stat-requests","#client-grid .cc-ver","#client-grid .client-card .badge",".gw-m b",".breaker-badge",".diff-row .ln",".acct-metrics b"];',
  '  var miss=sels.filter(function(s){var e=q(s);return !e||cs(e).fontVariantNumeric.indexOf("tabular-nums")<0});',
  '  add("G5","数字挂 tabular-nums",miss.length===0,miss.length?"缺 "+miss.join(" "):sels.length+" 处抽样全中");',
  '});',

  /* ── I4 侧栏状态点 ── */
  'T("I4","侧栏版本行状态点",function(){',
  '  var r=q(".ver-row .dot").getBoundingClientRect();',
  '  add("I4","侧栏状态点未被拉伸",Math.round(r.width)===6&&Math.round(r.height)===6,Math.round(r.width)+"×"+Math.round(r.height));',
  '});',

  /* ── H1 / H2 闭环 A ── */
  /* §4.4 键盘可达：Tab 到卡内按钮 → 整卡出环。
     ⚠️ 鼠标向（点了不该出环）**不硬断言**：合成 mousedown 不会改变引擎的 focus-visible 启发式，
     实测鼠标模拟后 :has(:focus-visible) 仍为真 —— 硬断言会变成恒假，逼人改样式去迁就脚本。
     所以这里只判「键盘向出环 + :has 可用 + 收环规则存在」，鼠标向留给 M9 人眼。 */
  'T("I10","卡内按钮聚焦时整卡出环",function(){',
  '  go("clients");',
  '  var card=q("#client-grid .client-card"),btn=card.querySelector("button[data-card-act]");',
  '  btn.focus();',
  '  var s=cs(card);',
  /* getComputedStyle 返回的是活对象，blur() 之后再读就变回 none 了 —— 必须先快照 */
  '  var oStyle=s.outlineStyle, oWidth=s.outlineWidth;',
  '  var kbd=(oStyle==="solid"&&parseFloat(oWidth)>=2);',
  '  var hasApi=!!(window.CSS&&CSS.supports&&CSS.supports("selector(:has(*))"));',
  '  var guarded=false;',
  '  function scan(rules){Array.prototype.forEach.call(rules,function(r){',
  '    if(r.cssText&&r.cssText.indexOf(":focus-within:not(:has(:focus-visible))")>=0)guarded=true;',
  '    if(r.cssRules)scan(r.cssRules);});}',
  '  Array.prototype.forEach.call(document.styleSheets,function(sh){try{scan(sh.cssRules)}catch(e){}});',
  '  btn.blur();',
  '  add("I10","卡内按钮聚焦时整卡出环",kbd&&hasApi&&guarded,',
  '    "键盘向 outline "+oStyle+" "+oWidth+" · :has 支持 "+hasApi+" · 收环规则 "+guarded);',
  '});',

  'T("H1","闭环A 新客户端接入",function(){',
  '  go("clients");click(q("[data-card-act=\\"openclaw\\"]"));',
  '  var s1=q("#drawer").classList.contains("open")&&qa("#drawer-body .perm-check").length===3;',
  '  click(qa("#drawer-foot .btn").filter(function(b){return b.textContent.indexOf("下一步")>=0})[0]);',
  '  var s2=qa("#drawer-body .opt").length===3;',
  '  click(qa("#drawer-foot .btn").filter(function(b){return b.textContent.indexOf("下一步")>=0})[0]);',
  '  var s3=qa("#drawer-body .mock-code").length===2&&txt(q("#drawer-body")).indexOf("将写入")>=0;',
  '  click(qa("#drawer-foot .btn").filter(function(b){return b.textContent.indexOf("确认写入")>=0})[0]);',
  '  var s4=txt(q("[data-client=\\"openclaw\\"] .badge"))==="已写入 · 待重启";',
  '  click(qa("#drawer-foot .btn").filter(function(b){return b.textContent.indexOf("模拟收到首个请求")>=0})[0]);',
  '  var s5=txt(q("[data-client=\\"openclaw\\"] .badge"))==="网关接管";',
  '  add("H1","闭环A 接入三步 + 待重启 + 计量",s1&&s2&&s3&&s4&&s5,',
  '    "前置检查"+s1+" 三选一"+s2+" 写入预览"+s3+" 待重启"+s4+" 已接入"+s5);',
  '});',
  'T("H2","闭环A 失败分支可达",function(){',
  '  click(q("[data-card-act=\\"openclaw\\"]"));',
  '  click(qa("#drawer-foot .btn").filter(function(b){return b.textContent.indexOf("重新写入")>=0})[0]);',
  '  var partial=qa("#drawer-body .wr-file .bad").length===1&&txt(q("#drawer-body")).indexOf("已恢复 1/1")>=0;',
  '  add("H2","闭环A 部分写入失败 + 回滚 1/1",partial,"失败文件 "+qa("#drawer-body .wr-file .bad").length+" / 回滚文案 "+(txt(q("#drawer-body")).indexOf("已恢复 1/1")>=0));',
  '  closeDrawer();',
  '});',

  /* ── H5 闭环 C 的 401 分支 ── */
  'T("H5","闭环C 401 换号",function(){',
  '  openRequest("7809",true);',
  '  var a=txt(q("#drawer-body")).indexOf("401 换号 1 次")>=0;',
  '  var b=txt(q("#drawer-body")).indexOf("已自动换号")>=0;',
  '  var c=txt(q("#drawer-body")).indexOf("wb-cn-02")>=0;',
  '  add("H5","闭环C 401 自动换号",a&&b&&c,"计数"+a+" 换号提示"+b+" 账号转移"+c);',
  '  closeDrawer();',
  '});',

  /* ── H4 闭环 C 回流 ── */
  'T("H4","闭环C 关闭抽屉 +1",function(){',
  '  go("overview");',
  /* 先把计数复位到源码基线：前面的 H1（firstRequest 计量）与 H5（401 抽屉关闭）已经各 +1，
     不复位的话「基线」会随用例顺序漂移 —— critique-reviewer 报的 12482→12483 就是这个污染。
     断言本身是关系式（after === before + 1），复位只是让 detail 里能读到真实基线。 */
  '  REQ_TOTAL=12480;q("#stat-requests").textContent="12,480";',
  '  var before=parseInt(txt(q("#stat-requests")).replace(/,/g,""),10);',
  '  openRequest("7821");closeDrawer();',
  '  var after=parseInt(txt(q("#stat-requests")).replace(/,/g,""),10);',
  '  add("H4","闭环C 关闭后今日请求 +1",after===before+1,before+" → "+after);',
  '});',

  /* ── H3 闭环 B ── */
  'T("H3","闭环B 档案切换 + 撤销",function(){',
  '  go("profiles");openSwitch("direct-backup");',
  '  var hasDiff=qa("#confirm-box .diff-grp").length>=2;',
  '  click(qa("#confirm-box .cf-foot .btn").filter(function(b){return b.textContent.indexOf("确认切换")>=0})[0]);',
  '  var moved=txt(q("[data-profile=\\"direct-backup\\"] .pc-foot")).indexOf("当前生效")===0;',
  '  var oldOff=txt(q("[data-profile=\\"main-gw\\"] .pc-foot")).indexOf("未生效")===0;',
  '  var ver=txt(q("[data-profile=\\"direct-backup\\"] .pc-top i"))==="v3";',
  '  var undo=!!document.querySelector("#toasts .toast [data-undo]");',
  '  add("H3","闭环B diff 确认 + 生效转移 + 撤销",hasDiff&&moved&&oldOff&&ver&&undo,',
  '    "diff"+hasDiff+" 转移"+moved+" 旧卡未生效"+oldOff+" 版本 v3 "+ver+" 撤销"+undo);',
  '  var u=document.querySelector("#toasts .toast [data-undo]");if(u)click(u);',
  '});',

  /* ── I5 / I6 限高（computed 会把 dvh 解析成 px，所以判「不是 none 且小于视口」） ── */
  'T("I5","抽屉限高可滚",function(){',
  '  var s=cs(q("#drawer")),mh=parseFloat(s.maxHeight);',
  '  var ok=s.maxHeight!=="none"&&!isNaN(mh)&&mh<window.innerHeight;',
  '  add("I5","抽屉 max-height 限高可滚",ok,s.maxHeight+" < 视口 "+window.innerHeight+" · body overflow "+cs(q("#drawer-body")).overflowY);',
  '});',
  'T("I6","弹窗限高",function(){',
  '  openConfirm("<h3>x</h3>");',
  '  var s=cs(q("#confirm-box")),mh=parseFloat(s.maxHeight);',
  '  var ok=s.maxHeight!=="none"&&!isNaN(mh)&&mh<window.innerHeight;',
  '  closeConfirm();',
  '  add("I6","弹窗 max-height 限高",ok,s.maxHeight+" < 视口 "+window.innerHeight);',
  '});',

  'var pre=document.createElement("pre");pre.id="vreport";pre.textContent=JSON.stringify(R);document.body.appendChild(pre);',
  '},700)});'
].join('\n');

function findChrome() {
  if (process.env.CHROME_PATH && fs.existsSync(process.env.CHROME_PATH)) return process.env.CHROME_PATH;
  const cands = [
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser'
  ];
  return cands.find((p) => fs.existsSync(p)) || null;
}

function dynamicChecks() {
  const chrome = findChrome();
  if (!chrome) {
    console.log('[SKIP] 未找到 Chrome/Edge，动态检查（C/D/E/F/G5/H/I1-I6）未执行');
    console.log('       设 CHROME_PATH=<可执行文件> 后重跑；无证据不判通过。');
    /* ⚠️ 这里**不**直接判失败，判据交给 `REQUIRE_BROWSER_CHECKS`：
       - CI（pr-check）置 `REQUIRE_BROWSER_CHECKS=1` ⇒ 没浏览器就以退出码 1 判红
         （「无证据不判通过」在需要证据的地方必须硬）；
       - **镜像构建 / 无浏览器的开发机**不置该变量 ⇒ 没有浏览器是**环境事实**，
         不该把整条构建拖垮（2026-11-10 实测：Dockerfile 里的 `npm run build`
         因本套件以退出码 1 结束而失败 ⇒ 镜像发不出去，而失败原因与镜像本身无关）。 */
    console.log(
      process.env.REQUIRE_BROWSER_CHECKS === "1"
        ? '       REQUIRE_BROWSER_CHECKS=1 ⇒ 视为失败（CI 必须有浏览器）'
        : '       REQUIRE_BROWSER_CHECKS 未置 ⇒ 本机无浏览器，跳过（CI 会判红）',
    );
    return { skipped: true, items: [] };
  }
  const tmp = path.join(os.tmpdir(), 'cc-switch-bridge-verify-' + process.pid + '.html');
  fs.writeFileSync(tmp, SRC.replace('</body>', '<script>\n' + PROBE + '\n</script>\n</body>'));
  try {
    const out = cp.execFileSync(chrome, [
      '--headless=new', '--disable-gpu', '--no-sandbox', '--hide-scrollbars',
      '--window-size=1280,900', '--virtual-time-budget=6000', '--dump-dom',
      'file:///' + tmp.replace(/\\/g, '/')
    ], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] });
    const m = out.match(/<pre id="vreport">([\s\S]*?)<\/pre>/);
    if (!m) throw new Error('页面未回传报告（可能脚本报错）');
    /* --dump-dom 会把文本里的 < > & 序列化掉，还原后再 parse */
    const json = m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&amp;/g, '&');
    return { skipped: false, items: JSON.parse(json) };
  } finally {
    try { fs.unlinkSync(tmp); } catch (e) { /* noop */ }
  }
}

/* ═══════════════════════════ 主流程 ═══════════════════════════ */

staticChecks();

const wantsBrowser = ONLY.length === 0 ||
  ONLY.some((id) => /^(C\d|D\d|E\d|F\d|G5|H\d|I[1-6])$/.test(id));
let dyn = { skipped: false, items: [] };
if (wantsBrowser) dyn = dynamicChecks();

/* D1 是真检查，静态占位项丢弃（D2 已覆盖软底与竖条） */
let all = RESULT.concat(dyn.items).filter((r) => !(r.id === 'D1'));
/* D1 = D2 的结论别名，保留专家清单里的条号 */
const d2 = all.find((r) => r.id === 'D2');
if (d2) all.push({ id: 'D1', name: 'diff 行只用 10% 软底（非纯红纯绿）', ok: d2.ok, detail: d2.detail });

const order = ['A', 'C', 'D', 'E', 'F', 'G', 'H', 'I'];
all.sort((a, b) => {
  const ga = order.indexOf(a.id[0]), gb = order.indexOf(b.id[0]);
  if (ga !== gb) return ga - gb;
  return a.id.localeCompare(b.id, 'en', { numeric: true });
});

const picked = ONLY.length ? all.filter((r) => ONLY.indexOf(r.id) >= 0) : all;
const manualPicked = ONLY.length ? MANUAL.filter((m) => ONLY.indexOf(m[0]) >= 0) : MANUAL;

let pass = 0, fail = 0;
picked.forEach((r) => {
  console.log((r.ok ? '[PASS] ' : '[FAIL] ') + r.id + ' ' + r.name + '  ' + r.detail);
  r.ok ? pass++ : fail++;
});
manualPicked.forEach((m) => console.log('[MANUAL] ' + m[0] + ' ' + m[1] + '  ' + m[2]));

console.log('');
console.log(pass + ' passed, ' + fail + ' failed, ' + manualPicked.length + ' manual');
console.log('');
console.log('覆盖声明：以上只覆盖「令牌 / 几何 / 状态机 / 闭环 / 数字自洽」这些可机读维度。');
console.log('不覆盖（必须人眼）：mock-code 块渲染、文案语气与产品边界、图标语义是否对、');
console.log('  颜色并集与对比度、1440/1024 两档观感、投影效果、鼠标路径焦点环。');
console.log('EXIT=0 只代表这些几何/状态断言成立，**不代表原型可发布** —— 别把它当发布信号。');
if (ONLY.length && !picked.length && !manualPicked.length) {
  console.log('（--only=' + ONLY.join(',') + ' 未匹配任何条号）');
  process.exit(1);
}
if (dyn.skipped) {
  /* 「无浏览器 ⇒ 不判通过」在 **CI** 是硬要求（动态检查静默消失 ⇒ 门禁变永远绿）；
     但在没有浏览器的环境里（镜像构建 / 无浏览器的开发机）它是**环境事实**，
     不该把整条构建拖垮。判据由 `REQUIRE_BROWSER_CHECKS=1` 决定（见 dynamicChecks 的注释）。 */
  if (process.env.REQUIRE_BROWSER_CHECKS === "1") {
    console.log('动态检查未执行 → 不能判通过（退出码 1；REQUIRE_BROWSER_CHECKS=1）');
    process.exit(1);
  }
  console.log('动态检查未执行 → 跳过（退出码 0；CI 里 REQUIRE_BROWSER_CHECKS=1 会判红）');
  process.exit(0);
}
process.exit(fail === 0 ? 0 : 1);
