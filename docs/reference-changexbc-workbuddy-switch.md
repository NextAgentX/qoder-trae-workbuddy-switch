# 外部参考：changexbc/workbuddy-switch（同产品的**兄弟实现**）

> 调研日期：2026-10-09 ｜ 对象：`https://github.com/changexbc/workbuddy-switch`（MIT，Tauri + React + Rust）
> 目的：判断这个项目对 **buddy-switch**（本仓，`NextAgentX/trae-workbuddy-switch`）有什么可参考的。
> 结论口径：**它不是上游也不是下游，而是同一产品的另一份独立实现** —— 两边**功能高度重叠**，
> 真正的差集**很小且集中**；最值钱的是它已经落地、而本仓 PLAN 只写到一半的**两处体验机制**，
> 以及一个**独立成组件的「外部 Agent 监听」**。
> 配套方案：`docs/redesign/P13-REFERENCE-PARITY.md`（可纳入功能的可行方案）。
> 取证方式见 §5：**有网络限制，部分大文件只确认存在、未读内容** —— 本文严格区分
> 「代码里看到」「README 声称」「未取证」。

---

## 0. 结论先行（四条）

1. **两仓同源不同实现，不是上下级。** 本仓是 `NextAgentX/trae-workbuddy-switch`（PolyForm-Noncommercial，
   2026-09-22 起）；参考是 `changexbc/workbuddy-switch`（MIT）。两边都在 2026-09/10 高频发版
   （参考 npm 最新 `0.1.60`，发布于 2026-10-09），**功能表逐条对得上**（账号管理 / 会话复制 / 积分到期 /
   积分统计 / Token 统计 / CodeBuddy CLI / 自动轮换 / 自动更新 / 权限检测 / npm + webui + Pages 只读演示）。

2. **★ 真正的差集只有三块**（详见 §2 对照矩阵）：
   - **①「用户偏好」两层开关**：参考有 `src/lib/nav-prefs.ts`（侧栏目的地可见性）与
     `src/lib/supported-tools.ts`（逐客户端入口开关，JetBrains 默认关），**本仓两条都没有** ——
     而 `PLAN.md §2.7 / §3.2 / §3.3 规则 3` 正好把这两条**写成了目标**。⇒ 「PLAN 已写、未落地」，
     参考提供了一份**可对照的落地实现**。
   - **②会话能力覆盖到 CodeBuddy IDE / VS Code 插件**：参考有**三套会话命名空间**
     （WorkBuddy / VS Code 插件 / CodeBuddy IDE，见其 `modules/mod.rs`），本仓只有 WorkBuddy 一套
     （`crates/buddy-switch-core/src/modules/mod.rs` 无 `vscode_session*` / `codebuddy_ide_session*`）。
   - **③外部 AI Agent 监听（Agent Companion）**：参考把 `changexbc/agent-companion`（MIT）
     **内联成一个 Tauri 插件 + sidecar 运行时**，监听 Codex / WorkBuddy / CodeBuddy IDE / VS Code 插件 / Codeg
     的会话状态（hooks + webhook）。本仓 P9 悬浮窗**只**展示自家切换/签到操作推导出的条目
     （`crates/buddy-switch-core/src/modules/companion.rs:10-16` 自陈「跨来源客户端与 P8 不在本期」）。

3. **本仓在若干处反而更领先，不该往回学**：内容指纹（`session.rs:390` `SessionFingerprint` +
   `:410` `SESSION_ID_PLACEHOLDER` 防「假分歧」）、fail-closed 的增量同步（`:538` 有分歧则**一个字节都不写**）、
   覆盖式分歧处理 + 备份字节数复核（`:764-776`）、客户端限流台账（只读 SQLite）、
   Skill 管理、客户端设置托管、P12 本地路由。**参考的判据疑似「记录条数级」，本仓是「逐行 hash 级」
   —— 这是语义差异，不能混用**（详见 §3.2）。

4. **参考的架构不宜整体照搬，但两处工程做法值得抄**：
   **①「偏好开关」的语义与实现**（关闭入口 = 不渲染 + 跳过轮询，URL 直达仍可用，数据不受影响）；
   **②「内联第三方组件」的版本固定机制**（`vendor/<name>/` + `<name>.version.json`（revision +
   archiveSha256 + interfaceVersion）+ `git archive` 取快照 + 宿主/组件兼容边界由 `interfaceVersion` 表达）。

---

## 1. 参考项目速览

| 项 | 值 | 依据 |
|:---|:---|:---|
| 仓库 | `changexbc/workbuddy-switch` | README |
| 许可 | **MIT**（`Copyright (c) 2026 wb-switch`） | `LICENSE` 全文；GitHub API `spdx_id: MIT` |
| 技术栈 | Tauri + React + Rust（`crates/wb-switch-core` / `crates/wb-switch-server`） | `Cargo.toml`、目录树 |
| 形态 | 桌面 App（macOS/Windows/Linux）+ npm/webui（`workbuddy-switch`，`workbuddy-switch status`）+ GitHub Pages 只读演示 | README、`npm/`、`index.html`、`.github/workflows/pages.yml` |
| 发版节奏 | npm `0.1.0`（2026-08-16）→ `0.1.60`（2026-10-09）：**54 天里 50+ 个版本**（版本号有跳号，如缺 `0.1.15` / `0.1.19` / `0.1.32` / `0.1.38` / `0.1.41` / `0.1.48` / `0.1.49`） | npm registry `time` 字段 |
| 核心模块 | 38 个：`account` `auth_file` `checkin` `codebuddy_cli` `codebuddy_cn_ide` `codebuddy_ide` `codebuddy_ide_session` `codebuddy_ide_session_sync` `config` `credit_usage` `credits` `error_log` `export_import` `jetbrains` `limits` `linux_keyring` `notifications` `oauth` `official_usage` `process` `rate_limit_events` `rate_limit_hook` `refresh` `rotate` `session` `session_backup` `session_groups` `session_link` `switch` `token_stats` `travel` `update` `variant` `vscode_cn_inject` `vscode_ext` `vscode_session` `vscode_session_link` `vscode_session_sync` | `crates/wb-switch-core/src/modules/mod.rs` |
| 内联第三方 | `vendor/agent-companion/`（`changexbc/agent-companion`，MIT，独立 App + 独立 Releases）；`vendor-patched/indexmap`（打了补丁的 crate 副本） | 目录树 |

> ⚠️ **可复现性边界**：本文**没有跑过**参考项目的任何一行代码，也没有读过它 400 KB+ 的大模块内部
> （见 §5）。所有结论按「他人实现记录」引用，**不得当成本仓可依赖的事实**。

---

## 2. 能力对照矩阵（参考 README 功能表 × 本仓代码）

> 判据：**用户可感知的结果一致**即可，不要求 DOM/文件结构一致（沿用 `COVERAGE-REPORT.md §0.1` 口径）。
> 本仓证据以当前工作区代码为准（2026-10-09）。

| 参考功能 | 本仓状态 | 本仓证据 | 差在哪 |
|:---|:--:|:---|:---|
| 账号管理（OAuth / 导入导出 / 删除） | ✅ | `src/components/oauth-login-dialog.tsx`、`export-accounts-dialog.tsx`、`import-accounts-dialog.tsx` | — |
| 账号切换（含进度） | ✅ | `crates/buddy-switch-core/src/modules/switch.rs`、`src-tauri/src/commands.rs` | — |
| 会话复制（WorkBuddy 桌面） | ✅ | `session.rs:1127` `copy_session_to_user_cross`、`:1772` `copy_sessions_for_switch_cross` | — |
| **会话复制（CodeBuddy IDE / VS Code 插件）** | ❌ | `modules/mod.rs`（66 行）**无** `codebuddy_ide_session*` / `vscode_session*`；`codebuddy_cn_ide.rs` / `vscode_ext.rs` / `jetbrains.rs` 都只做**账号切换** | **整块缺**：参考有三套会话命名空间，本仓只有一套 |
| **关联会话（跨账号副本状态 + 增量同步 + 分歧处理）** | ✅（WorkBuddy 单客户端） | `session.rs:412` `session_fingerprint_for`、`:505` `append_delta_between_files`、`:723` `unify_replica_between_files`、`src/pages/SessionsPage.tsx`、`src/components/sessions/session-replica-view.tsx:183` | 功能对齐；**机制不同**（见 §3.2） |
| **关联会话的「按客户端集中查看」** | ❌ | `src/pages/SessionsPage.tsx:32-34, 297` **刻意**不渲染客户端 Tab（「只有 1 项 ⇒ 假控件」，§3.15.1） | 客户端只有 1 个 ⇒ 参考的 3 个 section 在本仓无处安放 |
| **「新增关联会话」/ 解除关联** | ❌ | `src/lib/api.ts` 只有 `list_session_replicas` / `sync_session_replicas` / `unify_session_replicas`，**无** links preview / register / unlink | 本仓的关联是**账本副产物**，不是可编辑对象 |
| 积分到期查询（7 天高亮 + 紧迫排序） | ✅ | `README.md:102`、`src/pages/AccountsPage.tsx` | — |
| 积分统计 / Token 统计 | ✅ | `README.md:103-104`、`src/pages/UsagePage.tsx`、`TokenStatsPage.tsx`、`CreditStatsPage.tsx` | — |
| CodeBuddy CLI 切换 | ✅ | `codebuddy_cli.rs` | — |
| CodeBuddy IDE 切换 | ✅ | `codebuddy_cn_ide.rs` | — |
| **VS Code CodeBuddy 插件切换** | ✅ | `modules/vscode_ext.rs`、`vscode_cn_inject.rs`；`src/lib/platforms/registry.ts:112-114` 两条 WB 程序位**均已 `available`**（2026-10-09 落地） | 本仓**已对齐**（参考 README 有，本仓代码有、README 未写，见 §4 文档回写项） |
| **JetBrains IDE 插件切换** | ✅ | `modules/jetbrains.rs`（`options/secret-storage.xml`）；`registry.ts:115` | 本仓**已对齐**；差别只在**默认可见性**（参考 JetBrains **默认关**，本仓无开关） |
| 插件会话复制 | ❌ | 同「会话复制（IDE / VS Code）」行 | 整块缺 |
| 自动轮换 | ✅ | `rotate.rs` | — |
| 自动更新（签名校验） | ✅ | `src/lib/update.ts`、`src-tauri/src/update_service.rs`、`scripts/merge-update-manifests.py` | — |
| **会话悬浮窗（Agent Companion：Codex / WorkBuddy / CodeBuddy / Codeg）** | ⚠️ **形态对、数据源完全不同** | 本仓有 P9 独立窗口（`src-tauri/src/companion.rs:210` `index.html?window=companion`）；但数据只来自自家 `switch-progress` / `checkin-progress`（`src/lib/companion.ts:76-80`），`companion.rs:10-16` 明写跨来源客户端**不在本期** | 参考监听**外部 Agent 会话**（hooks/webhook）；本仓只显示自己的进行中操作 |
| 权限检测（macOS） | ✅ | `src/pages/SettingsPage.tsx` `PermissionCheckCard` | — |
| **设置 →「支持工具」逐客户端开关** | ❌ | `src/components/settings/settings-subnav.tsx:54-62` 共 9 项，**无**「支持工具」；全仓 grep `supported-tools` / `tools.<id>` **0 命中** | 参考 `src/lib/supported-tools.ts`（5 个 ToolId + `defaultEnabled`，JetBrains 默认 `false`） |
| **侧栏入口可见性偏好** | ❌ | 全仓 grep `nav-prefs` / 目的地可见性 **0 命中**；`App.tsx:184` `DEST_META` 是**静态表**，无偏好层 | 参考 `src/lib/nav-prefs.ts`（`wb-switch.nav.sessions`，关掉＝不渲染入口、URL 直达仍可用） |
| npm / webui 版本 + `status` 子命令 | ✅ | `npm/bin/buddy-switch.js`、`crates/buddy-switch-server/src/main.rs:295`（`status` / `version` / `serve`）；本仓 npm 包**尚未发布**（`npm/README.md:8`） | 本仓功能齐、**渠道未开** |
| GitHub Pages 只读演示 | ✅ | `.github/workflows/pages.yml`、`npm run build:demo` | — |
| 运行日志查看 | ✅ | `TraeSettingsPage.tsx`（app / checkin / switcher）；WB 侧为**空态 + 出口**（`settings.logs.emptyBody`） | ⚠️ **口径冲突**：参考称会解析 **WorkBuddy 运行日志**做沙箱审批判定；本仓文案写「WorkBuddy 无独立运行日志」⇒ **待核实**（见 §5） |
| 系统通知 | ⚠️ 只有 1 处 | `src-tauri/src/tray.rs:398` `notify_checkin`（签到完成）；`tauri-plugin-notification` 已装（`src-tauri/Cargo.toml:38`） | 参考有 `crates/.../notifications.rs`（9.7 KB）与 `src/lib/notify.ts`（1.7 KB）—— **内容未取证** |
| 错误日志 / 错误上报 | ⚠️ | 本仓有 `src/components/error-boundary.tsx`、`modules/error_code.rs`（错误码映射） | 参考有 `modules/error_log.rs`（11 KB）+ `src/lib/error-report.ts`（4.7 KB）—— **内容未取证** |
| 会话级备份 | ❌ | 本仓 `modules/backup.rs` 是**设置/账号** envelope（P7）；会话备份只作为 `unify` 的副作用（`session.rs:764`） | 参考有 `modules/session_backup.rs`（54 KB）—— **面未取证** |
| 客户端限流 | ✅（机制不同） | 本仓 `modules/client_limits.rs`：以 `SQLITE_OPEN_READ_ONLY` 只读客户端库，汇总「账号 × 模型 × 恢复时刻」 | 参考有 `rate_limit_hook.rs`（59 KB）+ `rate_limit_events.rs`（77 KB）⇒ 疑似 **hook 注入式采集** —— **未取证**，不宜照搬 |
| 定时任务排程（六类） | ✅ | `schedule.rs`、`scheduler.rs`、`README.md:108` | — |
| TraeWork 分区（签到 / 快照 / 设备 / 环境探测） | ✅ | `modules/trae/**`、`TraeSettingsPage.tsx` | 参考**没有 Trae 分区** ⇒ 这是本仓独有覆盖 |

---

## 3. 值得借鉴的四个机制

### 3.1 ★★ 「用户偏好」两层开关（最值得抄，也最便宜）

**参考实现**（两文件各约 2–3.5 KB，语义写得比本仓 PLAN 更直白）：

| 文件 | 语义（逐字摘自其 doc-comment） |
|:---|:---|
| `src/lib/nav-prefs.ts` | 「关闭『关联会话』后，左侧导航不再渲染该入口；**页面本身仍可经 URL 直达，会话数据、关联关系与账号库都不受影响，重新打开即恢复。**」键 `wb-switch.nav.sessions`，值 `"1"` / `"0"`，缺省为开 |
| `src/lib/supported-tools.ts` | 「关闭某工具 = 该工具的入口（账号卡片按钮、页顶状态徽标）不渲染，**相关状态轮询也跳过**；**不影响账号库与其它工具**，重新打开即恢复。」键 `wb-switch.tools.<id>`；5 个 `ToolId`：`workbuddy` / `codebuddyIde` / `codebuddyCli` / `vscodeExt` / `jetbrains`，**`jetbrains` 的 `defaultEnabled: false`**（灰度） |

两者都做了**同页变更广播**（`storage` 事件只在其它标签页触发 ⇒ 自建 `*:changed` 事件），
并且 `useSyncExternalStore` / `useState + effect` 两种写法都在。

**为什么对本仓直接有用**：`PLAN.md §2.7` 已经把「能力 vs 偏好」讲成**硬区分**，
`§3.2` 四条规则里的第 2 条（「关闭某项 = 不渲染入口；URL 直达仍然可用」）与参考**逐字同义**，
`§3.3` 规则 3 又要求「客户端可被用户关闭」。**本仓缺的是实现，不是设计。**
参考这份实现正好可以当**对照物**验证本仓的设计（尤其是「关掉入口要不要停轮询」这条 ——
本仓 `PLAN §2.7` 的三个「必须避免的错误」之 3 正好回答：**轮询可跳，写状态的任务不能停**）。

> ⚠️ **不能直接照抄的地方**：参考的 `ToolId` 把 **WorkBuddy 桌面**也算作一个「工具」；
> 本仓的词汇表把「WorkBuddy 桌面」当作**平台自身的默认程序位**（`VOCABULARY.md §2`），
> 而 `WorkBuddy 程序位表`（`registry.ts:109-116`）里的 5 项是
> `codebuddy_cn_ide` / `codebuddy_cli` / `vscode_cn_inject` / `vscode_codebuddy_plugin` / `jetbrains_codebuddy_plugin`。
> ⇒ 移植时应做**「程序位可见性偏好」**，并对「WorkBuddy 桌面」这个特例单独定规则（见方案 D35）。

### 3.2 ★★ 会话关联的「基线对 + 乐观令牌 + 词表」（机制可借，**口径不可借**）

参考的会话机制有 4 个本仓没有的概念：

| 概念 | 参考做法（代码看到） | 与本地差异 |
|:---|:---|:---|
| **显式关联表 + 基线对** | `session_link::load_store` → `store.groups[].members[] / pair_bases[] / variant`；**复制成功即自动登记**（`register_copied_sessions`），登记失败**不回滚复制**（`linkErrors[]` 上报） | 本仓的关联是 **copy ledger 的连通分量**（`session.rs:1666`），**没有 baseline** ⇒ 「增量」是相对**另一个账号当前内容**，不是相对**上次一致点** |
| **乐观并发令牌** | `previewToken` 是 `SyncSelection` 的必填项；`isActionable = previewToken && (...)` ⇒ **没有 token 就不能点** | 本仓**没有**这一层（同步直接按当前状态执行） |
| **判定词表** | `fastForward` / `diverge` / `ahead` / `identical` / `unknown` → 「可同步 / 双方都有更新 / 目标账号有更新 / 内容一致 / 无法确认」；**模式与可勾选性全部由后端给**，前端只搬 `availableModes[0]` | 本仓词表是 `latest` / `behind` / `diverge` / `unknown`（`src/lib/session-replicas.ts:63`），**语义接近、不逐字相同** |
| **勾选纪律（有测试钉死）** | 「全选**只作**用于 `fastForward` 的组」「`diverge` 默认不勾，需用户显式选择」「不可勾的组即使被勾也不提交」（`session-link-shared.test.ts`） | 本仓有等价纪律（`buildSyncItems` 只对 `behind` 出项），但**没有等价的单测断言** |

> 🚨 **口径红线（最重要的一条）**：参考的判据疑似**「记录条数级」** ——
> 它自己的 UI 提示写着「按会话内容的条数统计，不是对话轮数；**条数相同也不代表内容完全一致**」，
> 而 `recordCount` 只有 `{source, target, baseline}` 三个数。
> 本仓是**「逐行 hash 级」**（`session.rs:390` `line_count` + `prefix_hashes`；
> `:410` 还把本副本自己的 cid 归一化成占位符，**专门修过「假分歧」**，有单测 `:3332` 钉死）。
> ⇒ **两套判据不是同一坐标系，移植时只能选一个**：
> 本仓应当**保留自己的 hash 口径**（更严、且已修过假分歧），只借参考的
> **UI 词表、乐观令牌、勾选纪律**；若照抄它的 `common/extraA/extraB`，会得到一个与
> `deriveGroupStatus` **互相打架**的新数字（同一页两个「共同旧版」）。
> 参考的 `common/extraA/extraB` 精确公式**未取证**（只拿到一组 `{source:4, target:2, baseline:2}`），
> **不要替它编造**。

### 3.3 ★★ 外部 Agent 监听（Agent Companion）：一个**独立组件 + sidecar 运行时**的范本

这是参考与「自己写一个悬浮窗」完全不同的路子，机制清楚（全部代码看到）：

| 层 | 参考实现 | 关键点 |
|:---|:---|:---|
| 组件本体 | `vendor/agent-companion/`（**内联快照**，非 git submodule） | 独立仓库 `changexbc/agent-companion`、**独立 App 与 Releases**、**MIT**、自带更新器（在宿主里**不**接管更新） |
| 版本固定 | `vendor/agent-companion.version.json` = `{revision, archiveSha256, interfaceVersion: 1}` | `interfaceVersion` 是**宿主/组件兼容边界**：改了原生命令或窗口行为 ⇒ **宿主必须发版**（`docs/agent-companion-integration.md` 明写） |
| 取快照 | `node scripts/vendor-agent-companion.mjs <repo> <full-sha>` | 用 `git archive` ⇒ **本地未提交改动不会进快照**；干净 checkout 不需要兄弟仓库 |
| 宿主集成 | `src-tauri/src/companion.rs`（2893 B，**很薄**）：`agent_studio_desktop::Config { assets: "companion/", runtime: <sidecar>, manage_autostart: false, default_enabled: false, show_rail: true }` | **宿主拥有托盘与总开关；组件拥有自己的窗口与监听连接**；`default_enabled: false`；命令只有 `get/set_companion_enabled` + `open_companion_settings`；事件 `agent-studio:enabled` |
| 前端状态 | `src/lib/use-companion-enabled.ts`（`useSyncExternalStore` + `listen("agent-studio:enabled")`） | 与参考「用户偏好」那一层同构 |
| 构建 | `tauri build` 跑 `npm run build:desktop`；把 `dist-embed/` 复制到 `companion/`；**运行时作为 external binary 打包**；跨目标用 `AGENT_COMPANION_TARGET` | **不在运行时下载可执行代码**（显式承诺） |
| 只读演示 | `scripts/prepare-companion-demo.mjs` + `src/components/companion-demo-dialog.tsx` + `src/lib/companion-demo-script.ts` | 把组件的 **web demo 构建**嵌进 Pages 演示，靠 `postMessage` 驱动**虚构快照**；`src/` 与 `dist/` 分离避免 dev 副本混进生产包 |
| 监听来源 | Codex（Desktop/CLI，hooks）、WorkBuddy（CN/global，hooks）、CodeBuddy IDE（CN/global，hooks）、CodeBuddy VS Code 插件（hooks）、**Codeg（webhook）** | 状态：运行中 / 待确认 / 已完成（+ 失败）；**不导入历史会话**；跳回原会话（CodeBuddy 两代**没有**精确跳转） |
| 采集实现 | `collector/lib/` 每来源一个适配器（`workbuddy.js` 11 KB、`codebuddy-ide.js` 9 KB、`codex.js` + `codex-live.js` + `codex-read-state.js`、`codeg.js` 25 KB、`custom.js` 19 KB）+ 内核（`collector.js` 12 KB、`hub.js` 7.9 KB、`tail.js`、`host-process.js`）；`collector/server.js` 是本地 HTTP 服务；`collector/hooks/*.py` 三个 hook 脚本 | ⚠️ **口径冲突**：组件 README 写「**Rust 本地服务**接收 Hooks / Webhook；打包后的应用**无需 Node.js 或 Python**」，但仓库里同时有整套 Node `collector/` ⇒ **打包版与开发版是两条实现**，**哪条进包未取证** |
| 接收端安全（`collector/server.js` 读到） | ① `Host` 必须匹配 `^(127\.0\.0\.1\|localhost)(:\d+)?$`，否则 **403**；② `Origin` 存在且不等于 `http://<host>` ⇒ **403**；③ 请求体**硬上限**（设置 32 KB / 自定义集成 2 MB / hook 64 KB），超限 **413**；④ `Content-Type` 必须是 `application/json`，否则 **415**；⑤ 默认端口 **8849**、只绑 `127.0.0.1`；⑥ 状态经 `/api/state` + `/events`（SSE，**1s 心跳**）下发 | 这是**可直接借用的本地接收端安全基线**（与本仓 `PLAN §2.5` 的 fail-closed 纪律同源） |
| 已知限制（README 自陈） | Codex 权限请求 Hook **在自动审批前**触发 ⇒ 不能据此判定真需人工；WorkBuddy **沙箱审批不发 Hook**，靠解析运行日志（延迟 10–25 s）；结束 Hook 丢失 ⇒ 无法可靠判定完成 | 「诚实边界」写法值得学：**每条都写清「做不到什么」** |

**为什么对本仓重要**：本仓 P9 的 `CompanionSnapshot` 数据模型
（`crates/.../companion.rs:53-75`：`id / source / title / state / hint` + `source: live|unavailable` + `updated_at`）
**已经是通用的**，且 `LiveOperation`（`:82-93`）被显式标注为**可替换点**。
⇒ 参考的外部监听若要纳入本仓，**不需要改前端契约**，只需换 provider —— 这是本仓 P9 当初就预留的口子。
（但「谁来产出 `LiveOperation`」是**新增一个本地接收端**，与本仓 P9 现有纪律
「刷新全在前端 webview、不新增后台循环」（`src/lib/companion.ts:11-16`）**不冲突**：
接收端是**事件驱动**的，不是周期轮询；但仍需按项目硬约束**登记进 `BackgroundTask` 两侧注册表**。）

### 3.4 零散可借的小件

| 参考文件 | 作用（据文件名 + doc-comment） | 本仓现状 |
|:---|:---|:---|
| `src/lib/use-visible-interval.ts`（2.2 KB） | 页面不可见时停掉轮询（省电） | 本仓有 `src/lib/use-document-visible.ts` ⇒ **疑似已等价**，需对照 |
| `src/lib/use-rotate-deferred-notice.ts`（1.5 KB） | 「轮换被推迟」的一次性提示 | 本仓无 ⇒ 与 `rotate.rs` 的「活跃保护 / 价值过滤」配套，**可能有价值** |
| `src/lib/account-display.ts`（1.6 KB） | 账号显示字段的统一回退链 | 本仓有 `src/lib/display-text.ts` ⇒ 疑似等价（`PLAN §3.3` 要求「所有展示账号名的位置共用同一个函数」） |
| `src/lib/avatar-tone.ts`（538 B） | 头像底色（原型 `P1-7` 的 oklch 亮度判据） | 本仓 `account-card.tsx` 用 Tailwind 色板（`GAP-ANALYSIS §4.2` 判为**不该照搬**） |
| `src/lib/credit-package-names.ts`（2.3 KB）+ `.test.ts` | 资源包名归一（原型 `F2`） | 本仓无（`GAP-ANALYSIS §4.2` 判为**原型演示数据特有**，但参考真的做了 + 有测试 ⇒ **口径可能该翻案**） |
| `src/lib/types.ts`（35 KB）单文件放全部类型 | — | 本仓按域拆 `src/lib/*-types.ts` ⇒ **本仓更好**（`types.ts` 35 KB 会腐坏） |
| `vendor-patched/indexmap` | 内联一个打了补丁的 crate（`Cargo.toml.orig` + `.cargo_vcs_info.json` 都在） | 本仓无 ⇒ **不建议学**（见 §4） |

---

## 4. 不要搬的东西

1. **判据级的口径不要搬。** 会话「分歧 / 共同旧版」用**记录条数**还是**逐行 hash**（见 §3.2）——
   本仓已经选了 hash 且修过假分歧，**不要为了对齐参考而换坐标系**。
2. **不要照抄它的悬浮窗组件替代本仓 P9。** 参考把整个 Agent Companion 内联成插件 + sidecar
   （`dist-embed/` + external binary + `interfaceVersion` 兼容边界）。本仓 P9 是**自家 WebviewWindow**
   （`src-tauri/src/companion.rs`，含键盘可达性门禁与 `capabilities/companion.json`）——
   整体替换会**丢掉本仓的门禁与既定验收**，且引入「两条更新链」（宿主 + 组件）。
   除非确定要**长期跟随上游组件**，否则**只借它的「监听什么 + 怎么收」**。
3. **不要搬 `vendor-patched/`。** 内联一个打过补丁的第三方 crate（`indexmap`）会把
   「上游修复」变成手工维护；本仓既有取向是走正常依赖 + 版本约束。
4. **不要搬它的架构分层。** 参考 `crates/wb-switch-core` 的模块数量少（38 vs 本仓 66），
   大量逻辑塞进超大单文件（`session.rs` 426 KB、`limits.rs` 140 KB、`vscode_session_sync.rs` 125 KB、
   `config.rs` 76 KB）。本仓按域拆分 + 单测密度更高（`session.rs` 内 `#[cfg(test)]` 数千行），
   **不要按它的粒度合并**。
5. **不要抄它的发布细节。** 它 `docs/DEVELOPMENT.md` 把 `TAURI_SIGNING_PRIVATE_KEY` 放在
   `~/.wb-switch/*.key`、由构建脚本注入 —— 与本仓 `.github/workflows/build.yml` 的现行路径不同；
   `docs/reference-workbuddy-manager.md §5.4` 已记录本仓对「签名私钥是否进 CI」的既有判断，
   **改流程要单独拍板**，不要顺手抄。
6. **不要引用它的 README 当事实。** 它 README 说「原生 Rust 服务、无需 Node」，而同仓有整套 Node
   `collector/`（§3.3 表末）—— 与本仓 `docs/reference-workbuddy-manager.md §4.2` 同一条教训：
   **读它的代码，别读它的方案文档当现状**。

---

## 5. ★ 可信度边界（必须与结论一起读）

### 5.1 取证通道限制（本次调研的硬约束）

- 可用通道：`api.github.com`（目录/树/blob 元数据）与 `github.com` HTML；
  再叠加一个可用的 raw 代理 `gh-proxy.com`。
- **`raw.githubusercontent.com` / `cdn.jsdelivr.net` / `cdn.statically.io` / `raw.githack.com`
  在本次调研中全部 `fetch failed`（多次重试）**，`raw.gitmirror.com` 与 `r.jina.ai` DNS 不解析。
- ⇒ 结论：**小文件读到了全文，大文件只确认存在与字节数**。

### 5.2 「代码里看到」（可作证据）

`LICENSE` · `README.md` · `docs/DEVELOPMENT.md` · `docs/agent-companion-integration.md` ·
`vendor/agent-companion/README.md` · `vendor/agent-companion.version.json` · `collector/server.js` ·
`src-tauri/src/companion.rs` · `src/lib/use-companion-enabled.ts` · `src/lib/supported-tools.ts` ·
`src/lib/nav-prefs.ts` · 各目录清单（`contents` API）· 以及由子代理读取的
`mod.rs` / `codebuddy_ide_session_sync.rs` / `session-copy-tab.tsx` / `session-link-shared.tsx` +
`.test.ts` / `session-sync-section.tsx` / `vscode-session-sync-section.tsx` / `session-tree.tsx` /
`session-group-detail.css`。

### 5.3 「未取证」（**不得写成事实**）

| 对象 | 状态 |
|:---|:---|
| `session.rs` 426 KB、`session_link.rs` 132 KB、`session_groups.rs` 111 KB、`vscode_session_sync.rs` 126 KB、`vscode_session.rs` 83 KB、`session_backup.rs` 54 KB、`codebuddy_ide_session.rs` 42 KB | **只确认存在与字节数** |
| `error_log.rs`、`notifications.rs`、`limits.rs`、`rate_limit_hook.rs`、`rate_limit_events.rs`、`linux_keyring.rs` | **只确认存在与字节数** ⇒ 「参考怎么做的」一律**未取证** |
| 参考的 `src/pages/SessionsPage.tsx`（58 KB，客户端 Tab 的落法） | 未读 |
| `session-group-detail.tsx`（33 KB，关联图的左右分列规则） | 未读 |
| `common` / `extraA` / `extraB` 的精确公式；`previewToken` 的生成与失效规则 | **未取证** |
| Agent Companion 打包版是 Node collector 还是 Rust 原生 | **口径冲突，未取证** |
| `vendor/`（除 agent-companion）与 `vendor-patched/` 的许可与来源 | **未核查** |

### 5.4 一处**待核实的事实冲突**：WorkBuddy 有没有可读的运行日志？

- 参考 `vendor/agent-companion/README.md`：「**WorkBuddy 沙箱审批**：沙箱类审批……**不发送 Hook 事件**，
  悬浮栏通过 **WorkBuddy 运行日志**识别……该观察只解析审批行、不读取会话文件」。
- 本仓 `src/locales/domains/shell.zh.ts:182-184`：「本分区**暂无运行日志**……WorkBuddy 分区的运行日志在网关页可见」。
- ⇒ 两处**不可能同时为真**。若要采纳「靠日志判定沙箱审批」这条，**必须先核实本仓是否漏了 WB 的运行日志目录**。
  这是本次调研里**唯一一条会直接影响方案取舍**的事实冲突（方案 §5 决策 D40 会引用它）。

### 5.5 许可结论（可写进方案的那一句）

> 参考仓库 `LICENSE` 为 **MIT**（`Copyright (c) 2026 wb-switch`），
> **会话相关文件未见第三方来源标记**；但仓库内存在 `vendor/` 与 `vendor-patched/` 目录，
> **其内容与许可能力未核查**。
> 本仓自 2026-09-22 起为 **PolyForm-Noncommercial-1.0.0**（`docs/LICENSING.md`）。
> MIT 代码**可以**进入本仓（MIT 允许再许可，条件是**保留其版权与许可声明**），
> 但本仓代码**不能**反向进入 MIT 项目（PolyForm 限制商业使用）。
> ⇒ 任何搬运都必须：① 保留 `Copyright (c) 2026 wb-switch` 与 MIT 原文；
> ② 遵守 `CONTRIBUTING.md`「不要提交来源不明的代码」——**逐文件登记出处**。
