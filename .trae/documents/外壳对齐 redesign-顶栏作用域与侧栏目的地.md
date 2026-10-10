# 外壳对齐 redesign：顶栏 L1 作用域 + 侧栏 L2 目的地

## Context（为什么做这件事）

用户反馈「当前实现和 redesign 原型完全不一样，我要的就是按 redesign 的新框架和 UI」。

经核查，**根因是外壳框架从未按要求落地**：

- redesign 权威骨架（[index.html](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/index.html#L11-L79)、[PLAN.md](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/PLAN.md#L161-L199) §2.1/§2.2）是「**顶部应用栏(L0/L1) + 侧栏(L2) + 内容区(L3)**」三段式：
  - **L1 作用域**（平台×区域，如 `wb-cn`）压成**顶栏左一的菜单按钮**（[PLAN.md](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/PLAN.md#L453-L497) §3.1）；
  - 侧栏固定「目的地」若干项，**不随产品换血**（[PLAN.md](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/PLAN.md#L499-L522) §3.2）。
- 当前 [App.tsx](file:///d:/_03_WorkBuddy/workbuddy-switch/src/App.tsx#L700-L811) **完全没有顶栏**，把 `ProductSwitch`（产品 Tab）+ `ScopeSwitch`（区域 Tab）塞在侧栏，侧栏随产品换血 —— 与原型观感"完全不一样"。
- 更关键的是，门禁 [check-scope-registry.cjs](file:///d:/_03_WorkBuddy/workbuddy-switch/scripts/check-scope-registry.cjs#L582-L603) 里那条断言**标题写着"P2 顶栏作用域选择器"，内容却断言"在侧栏内、位于 ProductSwitch 之后、限 WorkBuddy"** —— 它把错误的侧栏形态钉死，导致 PLAN §P2「作用域选择器上移顶栏」永远无法通过。

页面级（概览/账号/用量/会话/网关/设置）大多已按原型实现（例：[OverviewPage.tsx](file:///d:/_03_WorkBuddy/workbuddy-switch/src/pages/OverviewPage.tsx) 已实现"待处理在前 + 今日用量摘要"）。**本次先补齐外壳框架**，这是用户最不满、也是观感差异最大的部分。

**预期结果**：应用长出与原型一致的顶栏（作用域下拉 + 运行胶囊 + 搜索 + 刷新 + 主题 + 应用设置），侧栏精简为"品牌 + 目的地 + 版本行"，切作用域在顶栏一处完成、整页跟随。

---

## 目标形态（权威依据）

```
┌──────────────────────────────────────────────────────────────────┐
│ [◆ WorkBuddy 国内版 ▾]  ● 运行中   [🔍 搜索… ⌘K]  [↻] [◐] [⚙]   │ TopBar 56px
├──────────────┬───────────────────────────────────────────────────┤
│ 概览          │  账号管理                                          │
│ 账号    8  ●  │  国内版 · 8 个账号 · 3 个已过期                     │
│ 用量          │  ─────────────────────────────────────────────     │
│ 会话    2  ●  │  [🔍 搜索账号][排序▾][分组▾][卡片|表格][+ 添加]     │
│ 网关    3     │  ┌────────┐ ┌────────┐ ┌────────┐                 │
│ 设置          │  │ 账号卡  │ │ 账号卡  │ │ 账号卡  │                 │
│ ──────────── │                                                    │
│ v2026.x ●  ⧉ ↑ │                                                    │
└──────────────┴───────────────────────────────────────────────────┘
   Sidebar 240px                  Content
```

- 顶栏控件（[index.html](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/index.html#L14-L45)）：`scope-trigger`（产品图标 + 作用域名 + 副标题「当前登录 · N 个账号」）、`run-pill`（运行中/未运行/未检测到）、搜索(⌘K)、刷新、主题、应用设置 ⚙。
- 作用域菜单项（[PLAN.md](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/PLAN.md#L457-L462) §3.1）：4 个作用域并排，各显示「已登录: xxx / 未登录 / 未检测到 · N 个账号」+ 运行点 + ⌘N 序号。
- 侧栏底部（[index.html](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/index.html#L63-L74)）：状态点 + 版本号 + 会话悬浮窗 mini-btn + 检查更新 mini-btn。
- **macOS 红绿灯避让**：顶栏左内边距 ≥ 76px（[PLAN.md](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/PLAN.md#L468-L487) §3.1，`trafficLightPosition{x:16,y:30}`）；Windows/Linux 8–12px。**此问题浏览器/jsdom 抓不到，需真机截图确认**。

---

## 实施方案

### 阶段一：外壳框架（本次重点）

**1. 新增 lib（复用优先）**
- `src/lib/platform.ts`：抽出 `isMacOverlayTitleBar()`，把 [App.tsx:630-631](file:///d:/_03_WorkBuddy/workbuddy-switch/src/App.tsx#L630-L631) 的内联判断收敛一处（`api.isDesktop() && userAgent.includes("Macintosh")`）。
- `src/lib/use-scope-status.ts`：汇总 4 个作用域的 `{installed, running, loggedInName, accountCount}`。WB 侧读 `useAccountsStore`（`status`/`global.status`）；Trae 侧复用 [trae-variant-status.ts](file:///d:/_03_WorkBuddy/workbuddy-switch/src/lib/trae-variant-status.ts) 的 `TRAE_VARIANTS_KEY` 快照。对应 PLAN §2.3 的 `loadScopeStatus()`。
- 作用域跳转 href：复用 [scope-shortcuts.ts](file:///d:/_03_WorkBuddy/workbuddy-switch/src/lib/scope-shortcuts.ts#L75-L80) 的 `scopeShortcutTarget(scope, dest)`，不另造拼路径函数。

**2. 新增 `src/components/shell/`（自底向上）**
| 文件 | 职责 |
|:---|:---|
| `run-pill.tsx` | 纯展示：运行中/未运行/未检测到 |
| `scope-selector.tsx` | 触发按钮（产品图标+作用域名+副标题）+ 下拉（4 作用域状态 + ⌘N 序号），复用 shadcn `dropdown-menu` |
| `app-menu.tsx` | 顶栏 ⚙ 溢出菜单（L0：语言/外观/开机自启/自动更新/关于/捐赠 + 客户端设置/Skill 管理）|
| `top-bar.tsx` | 56px 容器 + 平台左内边距 + `data-tauri-drag-region` + 右侧按钮组 |

**3. L0 入口受控化（小改，不重写业务）**
- 为现有 `AppSettingsEntry`（[app-settings.tsx](file:///d:/_03_WorkBuddy/workbuddy-switch/src/components/app-settings.tsx)）、`ClientSettingsEntry`、`SkillManagerEntry`、`DonateButton` 各加**受控 Dialog 导出**（`open`/`onOpenChange`），由 `app-menu.tsx` 的 `DropdownMenuItem` 触发。保留 Dialog 内容**懒挂载**（性能约束见 `app-settings.tsx` 注释）。
- 主题按钮：按 `theme.ts` 的 `system/light/dark` 三态做选择，**不做二态假开关**（§1.7 纪律）。

**4. 重组 [App.tsx](file:///d:/_03_WorkBuddy/workbuddy-switch/src/App.tsx) `Layout`（:553-813）**
- 结构改纵向：`<div flex-col>` → `TopBar` + `<div flex-1>` → `aside(240px) + main`。
- **删除**：`ProductSwitch`(:735 及定义 :326-403)、`ScopeSwitch`(:738 及定义 :297-324)、侧栏「搜索或跳转」按钮(:742-752)、侧栏底部 L0 区块(:783-791)。
- **保留不动**：`scopedNav`(:605-618，**必须留在 App.tsx**，门禁断言其在 App.tsx)、`CommandPalette`、⌘K、⌘1..N 快捷键与 `isTypingTarget`、`FirstRunHint`、`Suspense/Outlet`、`/s/{scope}/{dest}` 与旧路由并存。
- `AppFooter`(:468-551) 精简为原型 `.ver-row`（状态点 + 版本号 + `CompanionToggle` mini-btn + 检查更新 mini-btn）。
- 移除 `hasUnifiedTitleBar` 的 `pt-20`(:712) 与 main 的 `pt-16`(:796)；让位改由顶栏承担。

**5. 状态流转**
- 唯一真值仍是 URL。切作用域 = `writeLastScope(...)` + `navigate(scopeShortcutTarget(next, currentDest ?? "accounts"))`。
- `⌘/Ctrl+1..N` 已由 `planScopeShortcuts` 动态生成，仅在菜单里补序号提示，**不新增快捷键**。
- Trae 的 `?line=`：区域（cn/global）本就属作用域；让 [use-trae-variant.ts](file:///d:/_03_WorkBuddy/workbuddy-switch/src/lib/use-trae-variant.ts) **路径 scope 优先、回落 `?line=`**（桥接，保留旧链接兼容），避免出现第二个真值。**程序位（TraeWork/TraeCode）是 L3 页内参数，不进 L1**，留在账号卡片/页面变体切换器。

**6. i18n**（`shell.zh.ts` ↔ `shell.en.ts` 必须成对）
- 新增：`topbar.scope.aria / .menuLabel`、`topbar.scope.sub.loggedIn|loggedOut|missing`（模板 "{name} · {n} 个账号"）、`topbar.run.running|stopped|missing`、`topbar.search`、`topbar.refresh`、`topbar.theme`、`topbar.appMenu`。
- 复用：`palette.entry`、`appSettings.*`、`nav.*`、`sidebar.version`。

**7. 同步门禁（关键）**
- 重写 [check-scope-registry.cjs:582-603](file:///d:/_03_WorkBuddy/workbuddy-switch/scripts/check-scope-registry.cjs#L582-L603)：判据从「侧栏内、ProductSwitch 之后、限 WorkBuddy」改为「存在 `ScopeSelector`、渲染于 `TopBar`、覆盖全部 `SCOPES`、**不出现在页面内**」。
- 保住 [check-scope-registry.cjs:2673-2683](file:///d:/_03_WorkBuddy/workbuddy-switch/scripts/check-scope-registry.cjs#L2673-L2683) 对 `App.tsx` 的 `palette.entry` 子串断言 → 让 `App.tsx` 以 `<TopBar searchLabel={t("palette.entry")} />` 传入。
- 其余断言（`scopedNav` 留在 App.tsx、§P1 三页走 `useScope`、面板/回收站无 ProductSwitch）天然通过。

### 阶段二：逐页校准（外壳完成、验证通过后）

- 顺序（[PLAN.md](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/PLAN.md#L1726-L1727) §7 决策 2）：**用量 → 网关 → 设置 → 账号**。
- 方法：浏览器打开 [index.html](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/index.html) 作视觉基准，对照 [demo-app.js](file:///d:/_03_WorkBuddy/workbuddy-switch/docs/redesign/demo-app.js) 的 `viewOverview`(:210-313)/`viewAccounts`(:567-607)/`viewUsage`(:794-838) 逐项比对。
- 重点：概览「其他作用域」卡（复用 `use-scope-status`）、账号页 `scope-chip` + 工具栏、页头作用域副标题。

---

## 验证方法

**不跑 `tauri dev/build`**（用户正在使用桌面客户端，避免打断其登录态）。在浏览器/demo 模式迭代：

```powershell
npx tsc --noEmit        # 键类型 / zh-en 词条对齐
npm run check:scope     # 227 条断言（含本次改写的 :582-603）
npm run check:api
npm run build           # tsc + 全部门禁 + vite build
npm run dev:demo        # VITE_DEMO_MODE=1 只读 fixture，安全
npm run build:demo      # → dist-demo（HashRouter，深链带 #）
```

- 视觉基准：直接打开 `docs/redesign/index.html`（离线可用）。
- 深链形如 `#/s/wb-cn/accounts`（[App.tsx:816](file:///d:/_03_WorkBuddy/workbuddy-switch/src/App.tsx#L816) 在 `pagesDemoHostingEnabled` 下用 `HashRouter`）。
- **macOS 红绿灯避让**必须真机截图确认（浏览器/jsdom 不可见）。

---

## 风险

- 移除 `ProductSwitch`/`ScopeSwitch` 会让 [check-scope-registry.cjs:582-603](file:///d:/_03_WorkBuddy/workbuddy-switch/scripts/check-scope-registry.cjs#L582-L603) 变红 → 必须同步重写该断言（见阶段一第 7 步），否则 `npm run build` 挂。
- `use-trae-variant` 改读路径 scope 时，须保留 `?line=` 旧值宽容规则，并回归 Trae 各页。
- 本方案在 L0 入口归顶上**偏离** PLAN §7 决策 3 的推荐项（推荐留侧栏），但**与原型 index.html 一致**，故以原型为准并记录此偏离。