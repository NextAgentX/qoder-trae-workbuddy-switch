# cc-switch 融合专题 · 设计系统规范（沿用 + 扩展）

> 基底 = `docs/prototype/demo.css`（1928 行，OKLCh / Rhea 主题）。**本规范不新增任何设计资产，只定义「新语义如何落在既有令牌上」。** 数值取自 demo.css 实测。

## 1. 令牌沿用声明

**零新增颜色。** 新页面出现新色值即视为缺陷。

| 语义 | 令牌 | 落地写法 |
|---|---|---|
| 接入正常 / 写入成功 | `--success` | `.badge.ok`（16% 底 + 纯色字） |
| 漂移 / 半开 / 降级 / 临期 | `--warning` | `.badge.warn`（18% 底） |
| 外部占用 / 失败 / 删除行 | `--destructive` | `.badge.bad`（16% 底） |
| 未接入 / 未安装 / 直连（中性） | `--muted-fg` on `--muted` | `.badge` 默认态，**不带颜色** |
| 当前生效 / 选中 / 主操作 / 进行中 | `--primary` | 交互态专用 |
| 正文 / 数字主体 | `--foreground` | `11.9:1` @card |
| 次级说明 / 标签 | `--muted-fg` | `5.6:1` @card（AA 通过） |

**强度分层（不引入新色）**：软底一律 `color-mix(in oklab, var(--x) 16%, transparent)`（warning 用 18%，与既有 `.badge` 一致），文字一律纯色 `var(--x)`。禁止用 `opacity` 造第三档——透明会与 hover 态混淆。
**⚠️ 色相冲突防线**：`--primary`(162.48) 与 `--success`(155) 几乎同色。**同一张卡片内不得同时用 primary 表「健康」、success 表「状态」**——primary 只给可点/选中，success 只给状态。这是本专题最易做错处。**light 校正（勿抄 dark 方案）**：light 下四色徽标**全部不合格**——实算 `success` 3.07 / `warning` 3.05 / `destructive` 3.57 / `primary` 2.02（文字纯色压 16% 软底）。必须**双调**：文字往 `foreground` 压深 + 软底加深。实测最小混比：`destructive` 压 20%、`success`/`warning` 压 30%、`primary` 压 **50%**（它最烂，浅薄荷几乎贴白底）。实现期须逐色 DevTools 实测，禁止沿用 dark 数值。**算对比度必须标注分母令牌**：`--card`(L .279) 与 `--card-2`(L .245) 不同，同一前景可得两个比值；高彩度色（如 destructive C .191）**不可用 Y≈L³ 近似**（会高估约 10%）；`color-mix()` 的产物**必须在 OKLab 内插值**再转 sRGB，按通道近似会再偏 ±0.3。三者叠加足以跨越 0.5 的余量——**最终判定只认 DevTools 取色器**。

## 2. 新增语义组件（6 个）

### 2.1 `client-card` — 8 款客户端矩阵卡
**铁律：5 态高度完全一致，`min-height:120px` 写死，状态只换徽标与右下按钮。**

```html
<button class="client-card" data-state="drift">
  <div class="cc-row1"><span class="cc-ico">⌘</span><b>Claude Code</b><i>v2.0.14</i><span class="badge warn">漂移</span></div>
  <code class="cc-url">http://127.0.0.1:8788/claude</code>
  <div class="cc-row3"><span class="cc-meta">Proxy-A → 「生产·主」 · 今日 1,284</span><button class="btn xs">看差异</button></div>
</button>
```
- 栅格 `grid-template-rows:30px 18px 26px; row-gap:8px; padding:14px 16px` → 30+18+26+16+28 = 118 + 2 border = 120px。取消行显隐，**永远渲染 3 行**。
- `.cc-ico` 30×30 / radius 9px（同 `.opt .ico`）；`.cc-url` 等宽 11px `--muted-fg`，`white-space:pre; overflow:hidden; text-overflow:ellipsis`（吸取 `.code-line` 教训：Windows 反斜杠会断行）。
- 网格 `repeat(auto-fill, minmax(300px,1fr)); gap:12px`；`<1280px` 退 3 列，`<900px` 退 2 列，`<620px` 单列。hover 沿用 `.acct`（`--border-strong` + `shadow-md`）；`focus-visible: outline 2px var(--ring); offset 2px`。
- 5 态映射（只改 row1 徽标 + row3 按钮，row2 永不空）：

| state | 徽标 | row2 | row3 按钮 |
|---|---|---|---|
| `gw` 网关接管 | `.badge ok`「网关接管」 | 本地网关 URL | `.btn sm`「打开档案」 |
| `direct` 直连 | `.badge`「直连」 | 上游真实 URL | `.btn ghost sm`「改回接管」 |
| `off` 未接入 | `.badge`「未接入」 | 上游真实 URL | `.btn primary sm`「接入」 |
| `absent` 未安装 | `.badge`「未安装」 | 「未检测到 ~/.xxx」 | `.btn ghost sm`「安装指引」 |
| `drift` 漂移 | `.badge warn`「漂移」 | 实际检测到的 URL | `.btn sm`「看差异」 |
| `busy` 外部占用 | `.badge bad`「被外部占用」 | 实际检测到的 URL | `.btn danger sm`「强制接管」 |

### 2.2 `profile-card` — 档案卡
```html
<article class="profile-card is-active">
  <div class="pc-top"><span class="scope-chip">项目 · apollo</span><i>v7</i><span class="grow"></span><button class="btn ghost xs">⋯</button></div>
  <h3>生产·主</h3>
  <code class="pc-prov">Proxy-A · sk-…a91f</code>
  <div class="pc-counts"><span>MCP 4</span><span>Skills 12</span><span>Prompt 2</span></div>
  <div class="pc-foot"><span class="badge ok">当前生效</span><button class="btn sm">切换到此</button></div>
</article>
```
- `min-height:172px`，`padding:16px`，`grid-template-rows:22px auto 18px 18px 32px; gap:10px`。
- 「当前生效」只给两重信号：`is-active` 加 `.acct.is-current::before` 同款左侧 3px `--primary` 竖条 + `.badge ok`。**不要第三处改边框。**
- 非生效卡按钮「应用」；生效卡**不重复状态**，改放一条事实（应用日期，如 `11-08 应用`）——**冗余的解法不是删掉，是拿它装新信息**，只换个说法仍是重复。按钮 `min-width:88px` 防抖。
### 2.3 `diff-row` — 按文件分组的差异行
```html
<div class="diff-file"><code>~/.claude/settings.json</code><span class="badge">3 +1 −1</span></div>
<div class="diff-body">
  <div class="diff-row del"><span class="ln">12</span><i>−</i><code><span class="k">env.ANTHROPIC_BASE_URL</span><span class="v">https://api.anthropic.com</span></code></div>
  <div class="diff-row add"><span class="ln">12</span><i>+</i><code><span class="k">env.ANTHROPIC_BASE_URL</span><span class="v">http://127.0.0.1:8788/claude</span></code></div>
</div>
```
- 行高 22px；`.ln` 宽 30px 右对齐 `tabular-nums` `--muted-fg` 10.5px；`<i>` 宽 12px。
- 底色 add `color-mix(in oklab, var(--success) 10%, transparent)`、del `--destructive 10%`；**左边缘 2px 纯色竖条**（40% 强度）做第二重编码。禁止纯红纯绿（`#0f0`/`#f00`）。
- 截断：`.k` 保头 `flex:0 1 auto; max-width:52%; ellipsis`（键名要读全）；`.v` 保尾 `flex:1; min-width:0; ellipsis`（差异在末端）。`.diff-body` 可 `overflow-x:auto`，行内不折行。
### 2.4 `flow-step` — 链路演示 5 节点
```html
<div class="flow">
  <div class="flow-step s-ok"><span class="fs-n">1</span><b>客户端</b><i>Claude Code</i></div>
  <div class="flow-step s-run"><span class="fs-n">2</span><b>网关</b><i>选号中…</i></div>
  <div class="flow-step s-idle">…</div>
</div>
```
- 容器 `display:grid; grid-template-columns:repeat(5, minmax(0,1fr)); align-items:start`；连接线走 `.flow-step::after`：`position:absolute; top:22px; left:calc(100% - 6px); width:12px; height:1px; background:var(--border-strong)`，末节点 `:last-child::after{display:none}`。
- 节点 `min-width:0; padding:10px 12px; border:1px solid var(--border); border-radius:10px; background:var(--card-2)`，内容全 `ellipsis`。
- 三态：`s-idle` 边框 `--border` + 标题 `--muted-fg`（**不低于 muted-fg，禁止 `opacity<.6`**）；`s-run` 边框 `--primary` + 10% primary 软底 + 10px `spinner`；`s-ok` 边框 `--border-strong` + 标题 `--foreground` + 数字标 `--success`；`s-fail` 边框 `--destructive` + 8% 软底 + 标题 `--destructive`。
- 宽度伸缩：**1280 视口实测 `.page` 内宽 984px（=1280−侧栏 240−padding 28×2），5 列各 177px**——不是早期估算的 1184/226，故**中文标签 ≤4 字、地址一律 ellipsis**；`<1180px` 改 `repeat(3, minmax(0,1fr))` 且第 4 节点 `grid-column:1/3`（此时连接线 `::after` 一并 `display:none`，跨行画线无意义），保持阅读顺序，不做横向滚动。
### 2.5 `breaker-badge` — 熔断三态（**必须与健康徽标不同构**）
```html
<span class="breaker-badge b-half"><i class="bb-dot"></i>熔断 · 半开</span>
```
- 结构差异：`.badge` 是软底药丸；`.breaker-badge` 是**无底色描边小方牌** —— `background:transparent; border:1px solid; border-radius:6px; height:20px; padding:0 7px; font-size:10.5px; font-weight:600` + 前置 5px 圆点。文案永远以「熔断 ·」开头，字面即区分。
- `b-closed` `--success` 静态；`b-half` `--warning` + `.bb-dot` 走 `animation: bbPulse 1.6s ease-in-out infinite`（`box-shadow` 外扩 0→3px、`opacity .5→1`）；`b-open` `--destructive` 静态，右侧追加倒计时 `12s 后试探`（10px、`tabular-nums`）。
- `min-width:96px` 固定，防「闭合」→「半开」抖动；`@media (prefers-reduced-motion: reduce)` 停 `bbPulse`。
- 适用对象（**按对象划界，不按页面划界**）：`breaker-badge` 只标记**熔断状态机本身**（closed/open/half-open，是运行时部件）。账号级的「被排除」是它的**派生结果**，不是状态机 → 账号卡用 `.badge warn` 药丸，文案必须含「熔断」以建立因果（推荐「因熔断已排除」）。形态编码层级、文案编码因果，两者都不许省。
### 2.6 `mock-code` — 只读「将写入」代码块
```html
<figure class="mock-code">
  <figcaption><code>~/.codex/config.toml</code><span class="mock">mock</span></figcaption>
  <pre>base_url = "http://127.0.0.1:8788/codex"</pre>
</figure>
```
- `border:1px solid var(--border); border-radius:10px; background:var(--background); overflow:hidden`（深底 = 只读暗示，与 `.code-line` 一致）。
- `figcaption` 高 32px、`padding:0 12px`、`border-bottom:1px solid var(--border)`，路径 `code` 11px `--muted-fg` 走 ellipsis；`.mock` 角标 `background:color-mix(in oklab, var(--warning) 18%, transparent); color:var(--warning); border-radius:5px; font-size:9.5px; letter-spacing:.06em; text-transform:uppercase; padding:1px 5px`。
- `pre`：`margin:0; padding:12px 14px; overflow-x:auto; white-space:pre; line-height:1.6; font-size:11px; font-family:ui-monospace,"SFMono-Regular",Consolas,monospace`。**长行横向滚动，绝不 wrap**（换行会伪造源码里不存在的结构）。块内不可编辑；需编辑时换「左 mock-code + 右输入」两栏，不做 contenteditable。

## 3. 排版与密度

- 字号阶梯（demo.css 实有值，**不得新增**）：`9.5 / 10 / 10.5 / 11 / 11.5 / 12 / 12.5 / 13 / 13.5(正文) / 14 / 15 / 16 / 20 / 28`。
- **数据密集区**（表格、客户端卡、diff、链路）：主值 12.5px `--foreground`、行高 1.35；次级标签 11.5px `--muted-fg`；徽标/角标 10.5px 为下限。所有数字挂 `.num`（`font-variant-numeric: tabular-nums`）防列宽跳动。
- **说明文案区**（页副标题、抽屉提示、原型注释）：12.5px / 1.6 `--muted-fg`；**整句说明禁用 11px 以下**；解释性文字沿用 `.proto-note`（左 3px 竖线 + 弱化色）与产品文案分离。
- 间距只用既有值：网格 `gap:12px`、段落堆叠 `gap:16px`、卡内边距 16px（紧凑卡 14px）、区块间 20px。圆角：容器/卡 `var(--radius)`(12px)、输入/小按钮/图标底 8px、badge 6px、药丸 `999px`，同屏同级容器不混用 12 与 10。页面骨架固定：`.page{max-width:1240px; padding:26px 28px 60px}`、顶栏 56px、侧栏 240px（折叠 64px）。

## 4. 交互与可访问性约束

1. **抽屉（右侧 460px = `min(460px,94vw)`）**承载接入向导、档案编辑、diff 预览：`.drawer-overlay`(z 64/65) + `.drawer-foot` 固定底栏放唯一主流程按钮；抽屉内不嵌套抽屉。
2. **弹窗只承载「确认一个动作」**（`.overlay` + `.confirm`）：强制接管、删除档案、回滚版本。禁止弹窗放表单。
3. **徽标一律文字 + 颜色双编码**，单独色点不承载语义（色盲 + 截图评审双重需要）。
4. **键盘**：判可见性，不判写法——可聚焦元素必须有 ≥2px `var(--ring)` 的可见替代（`outline` 或 `box-shadow`）。表格行 `tabindex="0"` + `offset:-2px`；**卡片不当 tabindex 容器**（内含真按钮时嵌套交互不合法），改由整卡出环：兜底 `:focus-within` 规则**必须位于 `@supports selector(:has(*))` 块之外**，块内用 `:focus-within:not(:has(:focus-visible))` 收掉鼠标环（**裸用 `:has()` 会让旧 WebKitGTK/Linux 整条规则失效、焦点环彻底消失**，比鼠标误亮严重一档）。`outline:none` 仅当**同时给了等效可见替代且限定作用域**时允许（如 `.inp:focus` 的 ring、`.card:focus-within .btn` 消双环）；全局 `outline:none` 一律不合格。
5. **对比度（dark）**：正文 ≥4.5:1（foreground 11.9 / muted-fg 5.6 均通过）；软底小字见 §1 light 校正。动效仅三处（抽屉 `translateX`、`spinner`、`b-half` 脉冲），全部在 `prefers-reduced-motion` 下关闭；禁止卡片入场动画（8 张同动 = 卡顿）。
6. 触达尺寸：可点元素 ≥24px（`.btn.xs` 24px 为绝对下限），卡片级按钮 ≥28px。

## 5. 给原型构建师的 10 条施工细则

1. **状态切换绝不改变卡片高度**：用固定 `grid-template-rows` + `min-height`，三行永远渲染，缺值填「未检测到 …」而非留空。
2. **diff 行不用纯红纯绿**，只用 `--success`/`--destructive` 的 10% 软底 + 2px 竖条；`+`/`−` 符号必须真打出来，不靠颜色。
3. **`--primary` 与 `--success` 同色相**：左侧竖条（生效）用 primary，徽标（状态）用 success/warn/bad，其余不混。
4. **链路节点未激活也要看得清**：最低 `--muted-fg`，禁止 `opacity:.4`；用边框与底色区分，不用褪色。
5. **熔断徽标不许复用 `.badge`**：它是描边方牌，文案「熔断 · x」，只有半开有脉冲。
6. **所有数字挂 `tabular-nums`**（请求数、版本号、剩余额度、倒计时）。
7. **时间戳用固定假时间**（如 `2026-11-09 14:32`），不用 `new Date()`、不用「3 分钟前」——截图与评审不可复现。**且全站只允许一个「现在」锚点**：表、流、图、抽屉必须同一时刻（**含图表文案里的「峰值出现在 N 点后」这类硬编码**），否则观众会读出「中间几个小时的数据丢了」。
8. **代码块 `white-space:pre`**，Windows 反斜杠路径用 `nowrap` 会折行；长行横向滚动。
9. **8 款客户端图标同色系**（`--muted-fg` / `--primary` 二选一），不许一款一个品牌色——立刻变彩虹拼贴。
10. **未安装 / 未接入不要禁用按钮**：沿用 `.prog.off`（弱化 + 虚线 + 可点给解释），原型里的禁用态等于「坏了」。
