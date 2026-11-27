# 独立外部平台 15 页面逐入口对齐记录

> 状态：已完成（2026-11-26）· 对应实施计划 Task 11 的核对矩阵
>
> 关联：[设计](../specs/2026-11-25-external-platforms-design.md) · [实施计划](2026-11-25-independent-external-platforms.md) · [外部积分来源](../../specs/EXTERNAL-CREDITS.md)

## 核对口径

三个 provider（doubao / loomy / raccoon）× 五个入口（overview / accounts / usage / gateway / settings）
共用同一组页面（`src/pages/ExternalProviderPage.tsx` 导出五个目的地组件，
账号页复用 `src/components/external-credit-sources.tsx`），因此**结构对齐只需一次实现、逐平台验证数据隔离**。

视觉基准为 `docs/prototype/index.html` 加载的 `demo-app.js` / `demo.css`（见设计文档「原型逐入口对齐规格」表）。

## 静态核验（已跑通）

| 检查 | 结果 |
|:--|:--|
| `npx tsc --noEmit` | 通过 |
| `node scripts/check-scope-registry.cjs` | 338 通过 / 0 失败（含 9 作用域、CN-only、词条键位） |
| `node scripts/check-api-contract.cjs` | 通过（routes=300，含 12 条新命令路由） |
| `node scripts/check-store-selectors.cjs` / `check-hooks-order.cjs` | 通过 |
| `npm run test:unit` | 7 文件 / **61** 用例通过（含 5 条 v1→v2 可见性迁移回归） |
| `cargo test -p buddy-switch-core` | 1934 通过（含前后端能力枚举跨语言护栏） |
| `cargo test -p buddy-switch-gateway` | 906 通过 |
| `cargo check -p buddy-switch-server` / `--lib`（`src-tauri`） | 通过 |
| `npx vite build` | 通过 |
| `git diff --check` | 通过 |

## 逐入口矩阵（3 × 5 = 15）

| # | 平台 | 入口 | URL | 路由直达 | 页头/作用域 | 子导航选中 | 加载态 | 空/错误态 | 窄屏 | 数据隔离 |
|---:|:--|:--|:--|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| 1 | 豆包 | 概览 | `/s/doubao-cn/overview` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 2 | 豆包 | 账号 | `/s/doubao-cn/accounts` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 3 | 豆包 | 用量 | `/s/doubao-cn/usage` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 4 | 豆包 | 网关 | `/s/doubao-cn/gateway` | ✅ | ✅ | ✅ overview/models/integration | ✅ Skeleton | ✅ | ✅ | ✅ |
| 5 | 豆包 | 设置 | `/s/doubao-cn/settings` | ✅ | ✅ | ✅ refresh/data | — | ✅ | ✅ | ✅ |
| 6 | Loomy | 概览 | `/s/loomy-cn/overview` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 7 | Loomy | 账号 | `/s/loomy-cn/accounts` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 8 | Loomy | 用量 | `/s/loomy-cn/usage` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 9 | Loomy | 网关 | `/s/loomy-cn/gateway` | ✅ | ✅ | ✅ | ✅ Skeleton | ✅ | ✅ | ✅ |
| 10 | Loomy | 设置 | `/s/loomy-cn/settings` | ✅ | ✅ | ✅ | — | ✅ | ✅ | ✅ |
| 11 | 小浣熊 | 概览 | `/s/raccoon-cn/overview` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 12 | 小浣熊 | 账号 | `/s/raccoon-cn/accounts` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 13 | 小浣熊 | 用量 | `/s/raccoon-cn/usage` | ✅ | ✅ | — | ✅ Skeleton | ✅ | ✅ | ✅ |
| 14 | 小浣熊 | 网关 | `/s/raccoon-cn/gateway` | ✅ | ✅ | ✅ | ✅ Skeleton | ✅ | ✅ | ✅ |
| 15 | 小浣熊 | 设置 | `/s/raccoon-cn/settings` | ✅ | ✅ | ✅ | — | ✅ | ✅ | ✅ |

- **不存在的入口**：三家均不显示「会话」（能力门控 `sessionCopy` 只有 WorkBuddy 有；
  `/s/<provider>-cn/sessions` 直达时渲染明确的不支持空态，而不是 WorkBuddy 会话页）。
- **不存在的 global 作用域**：三家 CN-only，切换器与 URL 无法构造 `doubao-global` 等组合。

## 本轮核对中发现并修复的问题

### 第一轮（结构/门禁）

| 问题 | 位置 | 影响 |
|:--|:--|:--|
| 聚合入口副标题用 `SCOPES.length`（9），账号总数只数 6 | `src/components/shell/scope-selector.tsx` | 显示「跨 9 个作用域」与实际统计口径不符 |
| 聚合需处理数把 3 个外部作用域的 `attentionCount=null` 当缺失，整条降级为不显示 | 同上 | 6 个 core 作用域的需处理数被外部平台拖没 |
| 模型回退目标列出 `doubao-cn` 等外部作用域 | `src/components/gateway/model-fallback-sheet.tsx` | 可选的跨平台回退目标在网关侧不存在 |
| 聚合筛选面板遍历 `SCOPES` | `src/components/aggregate/aggregate-filter-panel.tsx` | 多出 3 个永远为空的筛选分组 |
| 聚合导出分组遍历 `SCOPES` | `src/components/aggregate/aggregate-export-button.tsx` | 同上 |
| 网关状态加载态用「转圈 + 文字」（§3.6 禁止） | `src/pages/ExternalProviderPage.tsx` | 页面级加载态门禁变红 |
| 切换平台时页面组件被复用，保留上一家的账号/网关/用量 | `src/App.tsx` + `ExternalProviderPage.tsx` | 切到 Loomy 会先闪出豆包的余额 |
| 内置版本说明落后于应用版本 | `src/data/release-notes.json` | scope 门禁「打包内置版本说明」变红 |

### 第一轮审查（八路代码审查）

| 问题 | 位置 | 影响 |
|:--|:--|:--|
| `loadAll()` 仍遍历 9 个作用域拉 `get_disabled_models`，Rust 侧只认 6 个 | `src/stores/gateway.ts` | **每次进网关卡都常驻一条「未知的作用域」红条** |
| 跨作用域切账号把三家外部平台列成来源 | `src/components/switch-account-dialog.tsx` | 死行 + 套用 WB CN 账号名 |
| 概览「其他作用域」列出外部平台 | `src/pages/OverviewPage.tsx` | 永远显示「未检测到」 |
| 作用域可见性 v1→v2 迁移：坏旧值只剩外部平台、且从不回写 | `src/lib/scope-visibility.ts` | 老用户升级后看不到自己的 WB 账号；每次启动重跑迁移 |
| 侧栏运行圆点丢掉 WorkBuddy 跨区域取或 | `src/App.tsx` | `wb-global` 显示未运行（客户端其实在跑） |
| 豆包非流式请求收到 SSE 原文 | `crates/buddy-switch-gateway/src/external.rs` | `"stream": false` 的 SDK 解析失败 |
| 豆包路径 `client_stream=false` 时进入 `unreachable!` | 同上 | panic |
| 小浣熊 Cookie 同时含两个 token 时丢掉 refresh token | `crates/buddy-switch-core/src/modules/external_credits.rs` | 登录态过期后无法自动刷新 |
| 刷新与增删账号并发时后写覆盖先写 | 同上 | 期间添加/删除的账号静默丢失 |
| `providerFromPath` 静默回落 `doubao` | `src/pages/ExternalProviderPage.tsx` | 第 4 个平台会显示豆包的账号与余额 |
| `platformOfScope` 三元链把新平台落到 `"qoder"` | `src/lib/aggregate-accounts.ts` | 潜在错标签（当前无调用方） |
| 顶栏刷新：三家全部失败仍报「已刷新」 | `src/lib/use-scope-status.ts` | 与页面告警自相矛盾 |
| 网关状态胶囊用硬编码 emerald，未用 `success` 语义色 / `Badge` | `src/pages/ExternalProviderPage.tsx` | 违反主题令牌与组件复用约定，暗色模式错色 |
| 采样去重 O(accounts × samples)、无变化也回写 | `src/lib/external-platforms.ts` | 每次挂载/刷新都整库序列化 |
| 概览/用量日聚合逐行重扫重排（O(n² log n)） | `src/pages/ExternalProviderPage.tsx` | 500 行 × 5000 条卡渲染 |
| 顶栏作用域状态一次 IPC 变三次同文件磁盘读 | `src/lib/use-scope-status.ts` | 每次挂载三次冗余读盘 |

**未修（记录在案）**：`UsagePage` 的外部平台 tab 表为空（当前路由不可达，属潜在）；
`scope-shortcuts` 上限 9 已用满（第 10 个作用域会一次性关掉所有人的 ⌘1–⌘9）；
`external.rs` 的 API Key 明文存放、非常量时间比较（与主网关 `apikey.rs` 的哈希+策略实现不同）；
Rust 侧每次代理请求重新读账号文件、豆包重试时整包 clone payload。

## WorkBuddy UI 逐页逐功能对照（2026-11-27）

对照基准：WorkBuddy 五个入口的**实际实现** + 原型 `demo.css` 的 `.card-head` / `.split` / `.grid.g3` 规格。

### 本轮统一掉的差异

| 项 | WorkBuddy | 外部平台（改前） | 处理 |
|:--|:--|:--|:--|
| 页头下边距 | `header.mb-5` | 容器 `gap-5` 叠加 | 容器去掉 `gap-5`，只留页头 `mb-5` |
| 页头刷新按钮 | `size="default"` | `size="sm"` | 改为 `default` |
| 卡头内边距 | `px-5 py-3` | `px-4 py-[13px]` / `py-3.5` | 7 处统一为 `px-5 py-3` + `border-border/60` |
| 卡头标题 | `text-sm font-semibold` | `text-[13.5px] font-semibold` | 7 处统一为 `text-sm` |
| 卡体/行内边距 | `px-5` | `px-4` | 内容区与行统一为 `px-5` |
| 概览统计块 | 共享 `StatTiles` + `hint` / `title` / `tone` 语义 | 四个裸数字 | 补齐三项语义（低余额/错误账号 → `warn`，网关运行 → `up`） |
| 概览分栏 | `.split`（gap 22 / 900px 转纵向） | `lg:` 断点 + 固定侧栏宽 | 改用 `[data-split-layout]`，与网关/设置同源 |
| 用量 Tab 节奏 | Tab `mb-4` → 参数条 → 内容 | 参数条嵌在 `Tabs` 内、`mt-3` 叠加 | 参数条移出、统一 `mb-4` |
| 账号页骨架 | 共享 `AccountWorkbench` | 自己拼 Card + 手写列表 | 锁定平台时改走 `AccountWorkbench`（骨架、两种空态、分组头、批量条、慢加载同源） |
| 账号页卡片栅格 | `.grid.g3` 1/2/3 列 | 固定 2 列 | 由 `AccountWorkbench` 提供 1/2/3 列 |
| 账号页表格视图 | `AccountTable` + 列定义 | 只有卡片 | 补「账号 / 状态 / 余额」三列，表格开关可用 |
| 账号页空态 | 「真没有」与「被筛没」两种出口 | 只有一种 | 两种空态分开，筛没态带「清除筛选」 |
| 设置页作用域说明条 | `rounded-[10px] bg-muted px-3 py-2` + `Info` | `Card` 包一段小字 | 改为同一说明条 |

### 第二轮（用户指出：添加账号 / 网关未对齐）

| 项 | WorkBuddy | 外部平台（改前） | 处理 |
|:--|:--|:--|:--|
| 添加账号 | 工具栏「+ 添加账号」→ **右侧抽屉**选方式（`AddAccountDrawer`） | 直接弹**居中 Dialog + 表单** | 改为同一抽屉：`ways={[]}` + 一张「粘贴凭据」卡，选中后再开表单 |
| 网关骨架 | `GatewayShell`：左侧「类型 + 子导航」栏 + 右侧内容区 | `SideSubnav` 与 `Tabs` **两套导航并存** | 改用 `GatewayShell`，移除 `Tabs` |
| 网关事实行 | 端口 / 绑定地址 / 账号池 / 模型数 / 最近失败原因 | 只有地址与 Key | 补齐端口、绑定、账号池计数、模型数、账号池隔离说明、最近失败原因 |
| 网关请求记录 | 概览内含「请求日志」 | 只有用量页有 | 网关子导航新增「请求记录」分区 |
| 网关动作 | 复制地址/Key、打开数据目录、刷新 | 只有刷新模型 | 补「打开数据目录」与请求记录刷新 |

### 添加账号抽屉：逐项对照

| 抽屉部位 | WorkBuddy | 外部平台 | 状态 |
|:--|:--|:--|:--|
| 容器 | 右侧 Sheet（`AddAccountDrawer`） | 同 | ✅ 一致 |
| 标题 / 作用域副标题 | 「添加账号」+「当前作用域：**名称**。选择一种方式添加账号。」 | 同 | ✅ 一致 |
| 添加方式卡 | OAuth 登录 / 从本机导入 / 导入备份 | 「粘贴 {provider} 凭据」 | ✅ 形态一致，卡片内容按能力不同 |
| 未装客户端提示 | 有 | 不适用（无客户端概念） | ✅ |
| 「更多操作」分区标题 | 有 | 有 | ✅ 一致 |
| └ 开关行形态 | 图标块 + 文字 + 右侧 `Switch` | 同 | ✅ 一致 |
| └ 动作行形态 | 图标块 + 文字 + 右侧箭头 | 同 | ✅ 一致 |
| 更多操作内容 | 自动签到、自动旅行（开关）；导出账号、刷新积分（动作） | 自动刷新余额、低余额提醒（开关）；刷新余额（动作） | ✅ 形态一致；具体项按平台真实能力 |

**外部平台刻意没有的项**（渲染即死控件）：

| WorkBuddy 项 | 不做的原因 |
|:--|:--|
| OAuth 登录 | 三家都没有可用的授权端点 |
| 从本机导入 | 没有客户端会话可读，凭据本来就是手动录入 |
| 导入备份 | 外部账号不在备份覆盖的 6 个 core 作用域内 |
| 自动签到 / 自动旅行 | 外部平台没有签到或旅行能力（产品明确不做签到） |
| 导出账号 | 没有对应的导出命令 |

### 已知的残留差异（不改，需你确认是否接受）

- **数值设置仍在设置页**：抽屉里的两个开关只能开/关，间隔分钟数与阈值数字在「设置 → 余额刷新」。
  WorkBuddy 的两枚开关是纯布尔值，所以同页没有数字项；外部平台是「开关 + 数值」两段，
  数值放抽屉会把设置页的 `SettingsFieldRow` 重复一遍。

### 平台标识（logo）

三家外部平台原先共用一枚通用 `Coins` 图标（无法区分平台）。现改为各自官方标识：

| 平台 | 源（官网公开资产） | 处理 |
|:--|:--|:--|
| 豆包 | `lf-flow-web-cdn.doubao.com/.../doubao_avatar_red_favicon.png`（76×76） | 归一化到 256×256，内容占 80.4% |
| Loomy | `loomy.xunfei.cn/icon.png`（1024×1024） | 同上 |
| 商汤小浣熊 | `sta-volc.xiaohuanxiong.com/.../brand/apple-touch-icon.png`（180×180，**横版 lockup** 178×111） | 同上，按长边缩放居中，保留原比例不拉伸 |

归一化参数与既有 `WorkBuddy` / `Trae` / `Qoder` 图标**完全一致**（画布 256×256 透明、内容占 80.4%、严格居中），
渲染统一用「放大 118% 居中裁透明圈 + 容器 `rounded-[22%]`」。脚本：`scripts/normalize-platform-logos.mjs`。

**聚合标识**：新增 `AggregateMark` —— 四宫格拼合，三格分别是 WorkBuddy / Trae / Qoder 的官方图标，
第四格是品牌色圆点（表示「还有更多」）。替换了原顶栏与作用域菜单里的 `LayoutGrid` 线性图标：
线性图标与旁边四枚彩色实心产品图标并排会显轻飘，也读不出「四个平台」。

> ⚠️ 三家图标来自官网公开的 favicon / apple-touch-icon，用于标识对应平台；
> 若后续要对外分发，建议再确认各家的品牌使用条款。

### 判断为「本就一致」、无需改动

- **设置/网关子导航**：`SettingsSubnav` 只是 `SideSubnav` 的薄包装（差异仅 `aria-label`），外部平台直接用 `SideSubnav` 在版式上完全一致。
- **网关页骨架**：外部平台同样使用 `[data-split-layout]` + 左侧子导航 + 右侧内容，与 WorkBuddy `GatewayShell` 同构。
- **状态表达**：WorkBuddy 网关用「圆点 + `text-success`」，外部平台用 `Badge variant="success"`；两者都走语义令牌，属同一套状态语言的不同粒度。

### 仍存在、且**有意保留**的差异

| 项 | 原因 |
|:--|:--|
| 概览统计块字段不同 | `OverviewStats` 绑定 WorkBuddy 的 token/积分/网关指标，外部平台是账号/余额/网关卡状态/更新时间；只能共用 `StatTiles` 原语 |
| 账号页无「回收站」「批量签到」「每日签到结果条」 | 外部平台没有客户端账号的软删除与签到能力，渲染即为死控件 |
| 账号页无「今日 Token」列 | 该数据来自客户端 token 统计，外部平台不采集 |
| 用量页三视图是「余额趋势 / 请求账本 / 本机采样」 | 外部平台没有客户端 token 统计与会话，对应列不存在 |
| 设置页分区只有「余额刷新 / 本机数据」 | 其余分组（外观、权限、更新、网络、备份）是应用级，不属于单个外部平台 |

### 验证

`tsc --noEmit`、`check-scope-registry`（338）、`check-hooks-order`、`check-prototype-behaviors`（13 套件）、`test:unit`（61）、`vite build` 全部通过。

## 仍需人工确认

- **视觉细节**：上表的 ✅ 为代码路径与结构核验（路由、组件、状态分支、断点类名），
  **不是浏览器截图比对**。逐像素间距/颜色如与原型有出入，按设计文档的原型对齐表再校。
- **真实上游联通**：豆包 / Loomy / 小浣熊的余额与模型请求依赖用户自己的消费端凭据，
  未在自动化中跑真实账号。
- **Windows 安装包**：本轮未执行 `npm run bundle:nsis`。需要发版产物时单独跑，
  并记录 `.exe` 路径与 SHA-256。
