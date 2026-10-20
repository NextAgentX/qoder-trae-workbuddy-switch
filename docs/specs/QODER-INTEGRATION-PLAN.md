# Qoder 集成 — 模块 / 接口 / 依赖梳理

> 状态：**实施中**。本文件是「先梳理、再落地」的梳理产物，随实现推进逐项勾选。
> 事实依据：`docs/specs/QODER-PLATFORM.md`（平台事实与建模决策）。
> 原型基准：`docs/prototype/buddy-switch-prototype.html`（+ 拆分形态）。
>
> ⚠️ 本文件只写**本仓要做什么**与**Qoder 协议的事实**；不写任何外部实现的出处。

---

## 0. 一句话方案

Qoder = **1 个 `Product` + 2 个 `Scope`（`qoder-cn` / `qoder-global`）+ 2 个程序位（桌面版 / CLI）**，
能力 `["checkin","credits","oauth","gateway"]`。分五层落地，**自下而上**，每层独立可验。

---

## 1. 依赖顺序（不可颠倒）

```
L0 core 平台基础        Product::Qoder + Scope::{QoderCn,QoderGlobal} + QODER_SPEC + QoderRegion
   │                    ⇒ 统一网关命名空间**自动**多出 2 条（零手写）
   ▼
L1 core 平台模块        modules/qoder/{region,paths,account,device,refresh,oauth,checkin,credits,profile,platform}
   │                    ⇒ 账号库 / 令牌 / 签到 / 积分 / 本机导入，纯逻辑 + 单测
   ▼
L2 gateway 产品线       gateway/src/qoder/{mod,routes,pool,apikey,payload,sse} + unified 分派分支
   ▼
L3 两处宿主接线         server(api.rs + main.rs) · tauri(commands.rs + lib.rs)
   ▼
L4 前端                 registry/scope → api.ts → 页面/组件 → 词表
   ▼
L5 门禁同步 + 全量验证   check-scope-registry.cjs / check-api-contract.cjs
```

**为什么必须自下而上**：L0 是跨语言护栏（`product_spec_matches_frontend_registry_slug_and_capabilities`）
的一端，前端改了后端不改就 `cargo test` 变红；L1 的账号库是 L2 选号池的数据源；L2 的
`*_for_region` 是 L3 命令体的唯一实现。

---

## 2. 逐层清单

### L0 · core 平台基础（`crates/buddy-switch-core/src/modules/`）

| 文件 | 改动 | 备注 |
|:--|:--|:--|
| `qoder/region.rs`（**新建**） | `QoderRegion { Cn, Global }` + `RealmSpec` 表（域名 / 开放接口 / 网关 / 客户端 id / 本地目录） | 仿 `trae/region.rs` 的表驱动 |
| `qoder/mod.rs`（**新建**） | 模块清单 + 上游常量 | |
| `product.rs` | `Product::Qoder`（`slug="qoder"`）；`Scope::{QoderCn,QoderGlobal}`；`Scope::parse/as_str/product/all`；`qoder_region()`；`scope_of_qoder_region` / `require_qoder_region`；`QODER_SCOPES` / `QODER_CAPABILITIES` / `QODER_SPEC`；`product_spec` 分支；`ScopeError::message` 合法取值列表 | ★ `Scope::parse` 保持 **fail-closed** |
| `gateway_namespace.rs` | `RelayTarget::Qoder(QoderRegion)`；`relay_target_of` 改**穷举**；`scope_label_key` 加分支；表规模断言 8 → **10** | 派生层本身零手写 |

**L0 完成判据**：`cargo test -p buddy-switch-core` 全绿（含跨语言护栏——此时前端**还没改**，
所以护栏会红 ⇒ **L0 与 L4-registry 必须同批**，见 §4 的「同批约束」）。

### L1 · core 平台模块（`modules/qoder/`）

| 文件 | 职责 | 关键接口 |
|:--|:--|:--|
| `region.rs` | 双区常量表 | `QoderRegion::{Cn,Global}`、`realm_spec(region) -> &RealmSpec`、`all()` |
| `paths.rs` | 全部路径派生（**只从枚举派生**，原始 scope 串永不进 `PathBuf`） | `accounts_file_for(region)`、`desktop_auth_file_for(region)`、`cli_auth_file_for(region)`、`models_cache_dir_for(region, uid)` |
| `account.rs` | 账号模型 + 账号库读写 | `RawAccount`（`uid/nickname/domain/access_token/refresh_token/personal_token/expires_at/source/credits/plan`）、`load_accounts_for_region`、`save_accounts_for_region`、`account_view`（camelCase 线上形态） |
| `device.rs` | 稳定设备指纹派生 | `derive_id(uid, salt)`、`machine_identity(uid) -> MachineIdentity{ machine_id, machine_token, machine_type, session_id }` |
| `token.rs` | 令牌前缀语义 | `TokenKind::{DeviceAccess(dt-),JobAccess(jt-),Personal(pt-),DeviceRefresh(drt-),JobRefresh(jrt-)}`、`kind_of(token) -> Option<TokenKind>` |
| `refresh.rs` | 按前缀路由的刷新 | `refresh_account_for(region, account)`、`ensure_fresh_for(region, uid)`、`run_keepalive_cycle_for(region)` |
| `oauth.rs` | PKCE S256 设备流 | `login_start_for(region)`、`login_poll(login_id)`、`login_cancel`；两区 URL 参数差异在 `RealmSpec` |
| `checkin.rs` | 每日签到（legacy 通道，上游已关停 · #58） | `claim_decision(&CheckinStatus)`、`claim_for(region, uid)`、`status_for(region)`、`status_map_for(region, uids)`；纯函数出口 `already_claim_response` / `disabled_claim_response` |
| `credits.rs` | 额度 / 套餐 | `fetch_credits_for(region, uid)`、`fetch_plan_for(region, uid)` |
| `profile.rs` | 本机导入（**两处**官方凭据存储） | `import_from_local_for(region)`、`import_from_pat_for(region, pat)` |
| `platform.rs` | 客户端探测 / 能力诊断 | `detect_clients_for(region)`、`capabilities()` |
| `store.rs` | 落盘（原子写） | 仿既有 `atomic_write` 惯例 |

**加密（L1 内）**

| 来源 | 形态 | 实现要点 |
|:--|:--|:--|
| 桌面 App `auth.v1.dat` | Chromium `v10` + **AES-256-GCM** | 密钥 = `Local State` 的 `os_crypt.encrypted_key` 剥 `DPAPI` 前缀后经 **DPAPI** 解出。★ 本仓已有同款实现（`modules/vscode_cn_inject.rs` 的 `dpapi_decrypt` + v10 布局），**复用同一模式**，不另起一套 |
| CLI `~/.qoder[.cn]/.auth/user` | **AES-128-CBC**，`key = iv = machine_id[:16]` | 需自带 AES-128-CBC（本仓已有 AES 原语可复用）；兼容以 `{` 开头的明文 |

### L2 · gateway 产品线（`crates/buddy-switch-gateway/src/`）

| 文件 | 改动 | 备注 |
|:--|:--|:--|
| `qoder/mod.rs`（新建） | `QoderGateway` + `impl GatewayProduct` + `OWNED_PATHS` + 进程内单例 | 照 `trae/mod.rs` |
| `qoder/routes.rs`（新建） | `/health`（兼容 `/healthz`）`/status` `/v1/models` `/v1/chat/completions` `/v1/messages`；鉴权 `Authorization: Bearer` | ★ `/v1/responses` 明确 **501**（Qoder 无等价协议） |
| `qoder/pool.rs`（新建） | `QoderPoolEntry` + `impl PoolEntryLike` + `qoder_pick_policy()` | 照 `trae/pool.rs` |
| `qoder/apikey.rs`（新建） | 密钥库（`qd-` 前缀）+ 三态动作 | 照 `trae/apikey.rs` |
| `qoder/payload.rs` / `qoder/sse.rs`（新建） | 请求体构造 / SSE 解析 | |
| `unified/routes.rs` | `relay_llm` 的 match 加 `RelayTarget::Qoder(region)` 分支 | 无第二份转发实现 |
| `unified/routes.rs` | `collect_namespace_models` 加 Qoder 分支 | |
| `lib.rs` | `pub mod qoder;` + 路径重叠护栏单测 | ★ **绝不 merge router**（独立端口） |

**⚠️ 需新设计的唯一一处**：`disabled_models.rs` 现在按 `Region`（cn/global）分家，
Qoder 的 cn/global 会与 WB/Trae **共用同一把键**。方案：把键从 `Region` 升为 `Scope`（或
`(Product, Region)`），并补迁移/兼容读。**这是 L2 的独立子任务，先出方案再改。**

### L3 · 两处宿主

| 文件 | 改动 |
|:--|:--|
| `buddy-switch-server/src/qoder_gateway_host.rs`（新建） | 照 `trae_gateway_host.rs` |
| `buddy-switch-server/src/api.rs` | `/api/qoder/*` 路由 + handler（照 `api.rs` 的 Trae 段） |
| `buddy-switch-server/src/main.rs` | 网关启动 + `spawn_background_task` 派发分支 |
| `src-tauri/src/qoder_gateway.rs`（新建） | 照 `trae_gateway.rs` |
| `src-tauri/src/commands.rs` | `#[tauri::command] qoder_*` 命令本体 |
| `src-tauri/src/lib.rs` | `generate_handler!` + `spawn_background_task` 派发分支 |

★ **两处各一份，改一处必 grep 另一处**。★ `spawn_background_task` 的 `match` **穷尽无通配**
⇒ core 新增 `TaskKind` 会让两宿主**编译失败**（这正是「登记即执行」的强制机制）。

### L4 · 前端

| 文件 | 改动 | 类型 |
|:--|:--|:--|
| `src/lib/platforms/registry.ts` | `Product` 加 `"qoder"`；`PLATFORM_DESCRIPTORS` 加一条；`PRODUCTS`；`PlatformSlug`/`PLATFORM_SLUG`/`SLUG_TO_PRODUCT` | 🔧 改一处数据 |
| `src/lib/scope.ts` | `SCOPES` 4 → 6 | 🔧 |
| `src/components/product-marks.tsx` | 加 `QoderMark` | 🔧 |
| `src/components/scope-chip.tsx` | 二元分派 → **数据驱动**（否则 Qoder 显示 Trae 图标） | 🔧 |
| `src/App.tsx` | `SCOPED_ROUTES` 加 qoder 一行；4 处平台分支扩为三分支 | ⚠️ 主要工作量 |
| `src/pages/Qoder*.tsx`（6 个） | 照 Trae 页复制改数据 | 📋 |
| `src/components/qoder-*.tsx`、`gateway/qoder-*.tsx`（8 个） | 照 Trae 组件复制改数据 | 📋 |
| `src/lib/qoder-*.ts`（4~5 个） | 照 `trae-*.ts` 复制改数据 | 📋 |
| `src/locales/domains/qoder*.{zh,en}.ts` + `shell.*` 的 `product.qoder` | 新词表域 | 📋 |
| `src/lib/api.ts` | `qoder*` 命令段 + `ROUTES` + `DEMO_READ_COMMANDS` | 📋 |
| `src/lib/screenshot-demo.ts` | 只读 Qoder 命令补 `case`（门禁第 6 条） | 📋 |

**store：不需要新增**（照 Trae 用 `useCachedResource`；`useBackupStore` 按 `Scope` 自动覆盖）。

### L5 · 门禁同步

| 门禁 | 必改点 |
|:--|:--|
| `scripts/check-scope-registry.cjs` | 「4 个作用域」硬编码 → 6；`SCOPED_ROUTES` 每 dest 出现 2 次 → 3 次；`FOUR_SCOPES` → 6；命令面板作用域数 |
| `scripts/check-api-contract.cjs` | `SCOPE_PARAMETERIZED` 登记 Qoder 参数化命令；确认 fail-closed 正向断言仍命中 |
| `crates/.../product.rs` | 跨语言护栏自动覆盖（须与前端同批改） |

---

## 3. 原型 ↔ 代码 对应关系

| 原型（`demo-data.js`） | 代码落点 |
|:--|:--|
| `MARK.qoder`（圆环+内环+短尾） | `src/components/product-marks.tsx::QoderMark` |
| `PLATFORMS.qoder.accent = oklch(0.68 0.15 245)` | 暂无平台专属令牌（与 WB/Trae 一致，靠图标区分） |
| `PLATFORMS.qoder.programs = [App, CLI]` | `modules/qoder/platform.rs` 的程序位表 + 前端 `QODER_PROGRAMS` |
| `PLATFORMS.qoder.capabilities` | `QODER_CAPABILITIES`（后端）+ `registry.ts`（前端），**两侧逐字对齐** |
| `SCOPES["qoder-cn"/"qoder-global"]` | `Scope::QoderCn/QoderGlobal` + 前端 `SCOPES` |
| 账号字段（credits/expiring/checkedIn/programs/status） | `qoder::account::AccountView` |
| `USAGE[...]`（trend/models/sessions/credits） | `qoder::credits` + `token_stats` 的适配层 |
| `GATEWAY[...]`（keys/models/pool/logs/catalog） | `gateway::qoder::{apikey,pool,routes}` |
| 备份页 6 个作用域 | `backup_sections`（按 `Scope::all()` 自动） |
| 统一入口 6 个派生命名空间 | `gateway_namespace`（由 `Scope::all()` **自动**生成） |

---

## 4. 硬约束（违反必出事）

1. **同批约束**：`product.rs` 与 `src/lib/platforms/registry.ts` 必须**同一批改完**
   —— 跨语言护栏会互相钉死，只改一边必然 `cargo test` 红。
2. `Scope::parse` **fail-closed**，未知 / 空 / 拼错一律 `Err`；测试名
   `scope_parse_is_fail_closed_for_unknown_empty_and_typos` **不得改名**（门禁按名找它）。
   ★ 且输入集必须含**「比基准长一截」**的形状（`wb-cn-x` / `qoder-cn-x`）——
   否则前缀实现与精确实现在现有输入集上等价，断言恒绿。
3. 命名空间解析**整段精确匹配**，禁前缀。
4. **绝不 merge router**：Qoder 网关**独立端口**，`OWNED_PATHS` 与既有平台重叠。
5. 路径派生**只从枚举查表**，原始 scope 串永不进 `PathBuf`。
6. 凭据类：**「信封 ≠ 缺失」**——判据分家（刷新看 `refresh_token`，可用性看 `access_token`）。
7. 新增「写状态 / 周期性」出口 ⇒ **先 grep 两处宿主的注册表**。
8. `reference/` 不进仓库（已加 `.gitignore`）。

---

## 5. 验证计划

| 层 | 手段 |
|:--|:--|
| L0 | `cargo test -p buddy-switch-core`（含跨语言护栏 + 命名空间派生护栏） |
| L1 | 各模块内联单测（账号库往返 / 令牌前缀分家 / 指纹稳定性 / 签到幂等 / 双区 URL 差异） |
| L2 | `cargo test -p buddy-switch-gateway` + `tests/qoder_gateway_e2e.rs`（真发请求，非字符串扫描） |
| L3 | `npm run check:api`；两宿主的 `background_tasks_cover_*` |
| L4 | `tsc` + `npm run check:scope` + `npm run build` |
| 端到端 | 无真实 Qoder 账号 ⇒ 用合成夹具 + 变异验证（注入缺陷证明断言非恒真） |

★ **不做**：live 探针（本机无 Qoder 客户端与账号）；那些留 `#[ignore]` 的真机探针位。

---

## 6. 进度（2026-10-15 实测）

> 本轮交付的是**账号管理垂直切片 + 诚实空态**：凡数据源未落地的页面/端点，一律给
> **可诊断的结构化说明**（不借别平台的数据、不画空壳、不假装已接入）。逐项：

| 层 | 状态 | 说明 |
|:--|:--|:--|
| **L0** 平台基础 | ✅ 完成 | `Product::Qoder` + `Scope::{QoderCn,QoderGlobal}` + `QODER_SPEC` + `QoderRegion`；统一网关命名空间**自动**多出 2 条（表 8 → 10）；跨语言护栏（`product_spec` ↔ 前端 registry）绿 |
| **L1** core 平台模块 | ◐ **账号链路完成** | `qoder/{region,token,paths,account,device,store,profile}` 已落地（8 文件，40+ 单测）。**未做**：`refresh` / `oauth` / `checkin` / `credits` / `platform`（都要调上游，属网关期） |
| **L2** gateway 产品线 | ⛔ **未接入**（刻意） | 统一网关的 `RelayTarget::Qoder` 给**可诊断 501**（`namespace_product_line_not_wired`）——不静默借用 WB 的池；模型清单给 `source: "unavailable"` + note。**不 merge router**（独立端口）待做 |
| **L3** 两处宿主 | ◐ **账号命令完成** | 6 条命令 × 四处同步（ROUTES / `call()` / `generate_handler!` / `.route` + 命令本体），`check:api` 实测 `参数化=16` 全绿；两宿主未接入的旧端点对 Qoder 作用域**拒绝且可诊断**（`qoder_scope_not_applicable`）；`spawn_background_task` 穷尽分支已补（当前不可达） |
| **L4** 前端 | ◐ **4 个目的地真实 + 2 个诚实空态** | **账号页 = 原型工作台同构**（2026-10-15 二轮，用户要求「严格按 `buddy-switch-prototype.html` 1:1」）：复用 `AccountWorkbench` / `AccountToolbar`（搜索·排序·分组·视图·全选·`+ 添加账号`）/ `AccountDetailDrawer`（**备注在抽屉里编辑**，卡上不再堆按钮）/ `AddAccountDrawer`（`ways` 收窄到已落地子集：从本机导入 + 粘贴 PAT；OAuth / 备份不摆空卡）；**批量条只出删除**（缺的动作不渲染）；**硬删除必确认**（弹层明说「没有回收站」）；卡片四块同形（头像 / 状态槽「凭据可用」/ 来源徽章 / 凭据摘要 + 掩码）、表卡双视图、`/` 聚焦搜索、`?q=` 一次性种子。**真实**（其余）：概览（账号数 + 客户端四态 + 域名 + `—` 表未知）、设置（子导航收窄到有内容的分区）、平台图标（`QoderMark` 字标 + `Record<Product,…>` 查找表）。**诚实空态**：用量 / 网关。**待做**：网关三卡（keys/pool/logs）、程序位胶囊（等切换能力落地） |
| **L5** 门禁同步 | ✅ 完成 | `check:api` / `check:scope` / `check:docker` / `check:store-selectors` / `check:npm-platforms` / `check:agent-sources` 全跑过；`check:scope` 的「4 作用域」→ 6、「每 dest 2 次」→ 3、「恰 10 条参数化」→ 16、备份 4×3 → 6×3 已同步 |
| 附加 | ✅ | 作用域快捷键上限 **5 → 9**（PLAN §3.1 v2.8 修订）——否则 6 个作用域会把既有用户的 `⌘1..⌘4` 一起关掉 |

**验证基线（2026-10-15，含二轮交互校准）**：`cargo check --workspace` 绿；`check:api` 绿（routes 189 / call 179 / server 173 / invoke 197 / 参数化 16）；
`check:scope` **295 passed / 0 failed**（含新增的「Qoder 账号页 = 原型工作台同构」门禁，已做两次变异验证：注入未实现动作 / 混入未落地 `ways` ⇒ 均变红，sha256 还原）；
`tsc` 绿；`cargo test -p buddy-switch-gateway` **482/0**。

**真端到端（webui 通道，可复跑）**：隔离 `BUDDY_SWITCH_HOME` 起 `target/debug/buddy-switch.exe` ⇒
`node _verify-qoder-e2e.mjs http://127.0.0.1:<port>`（20 条断言：双区各自读库 / 非法与缺 scope 400 fail-closed（文案列 6 个合法值）/
通用端点拒 Qoder 且未误伤 WB / PAT 导入→备注→删除 / **下发不含令牌明文** / CLI 路径跟随隔离 home / **双区互不相通**）。

**真实 UI 截图**：`logs/qoder-integration/*.png`（6 作用域菜单 + Qoder 圆环图标 + `⌘1..⌘6`、**账号页**（工具栏一行 + 卡四块）、
**详情抽屉**（账号信息 + 备注行）、**抽屉内备注编辑态**、**添加账号抽屉**（方式子集两张卡）、**批量条**、**硬删除确认**、**表格视图**、
概览页、设置页、诚实未接入页、WorkBuddy 分区无回归）—— demo 构建（`VITE_PAGES_DEMO=1` ⇒ HashRouter、base `/<仓库名>/`）+ 无头 Chrome 拍摄。

**交互验证（可复跑：`node _verify-qoder-workbench.mjs`，12/12 通过）**：页头原型公式（`{scope} · N 个账号`）/
工具栏一行（搜索·排序·分组·视图×2·全选·添加账号）/ 卡片 2 张（状态槽「凭据可用」+ 来源徽章 + 凭据摘要 + 掩码）/
**卡点击 → 抽屉**（账号信息 / UID / 编辑备注 / 删除账号）/ **抽屉内备注编辑**（输入框出现 → 真实输入 → Enter **走到命令层**，
demo 拦截为预期）/ Esc 关抽屉 / **添加抽屉**（本机导入 + PAT，**无** OAuth / 备份卡；演示构建里工具栏主操作被 `DemoAction`
拦截属平台级设计，脚本经 React props 直调验证接线）/ **批量条只出删除** / **硬删除确认**（「没有回收站」逐字）/
**表格视图四列**（账号·状态·来源·备注）。

**交互验证（截图之外，用 CDP 真实输入事件）**：
- 备注内联编辑：真实点击只读备注 → 输入框出现 → 真实输入 → 真实 Enter → 输入框关闭 + toast「演示模式下不可操作」
  ⇒ **保存链路真的走到命令层**（demo 拦截写操作，真机会打后端）。
- 设置子导航：真实鼠标点「客户端 / 备份与恢复 / 通用」⇒ `active` 高亮与**内容**同步切换（判据用卡片独有文本，不用子导航标签）。
- ⚠️ 两条方法学坑（**写脚本前先读**）：① `new KeyboardEvent(...)` / `el.click()` 这类**合成事件**在 React 18 下不保证被处理
  （实测合成 blur 后输入框仍在 ⇒ 假红）；一律走 `Input.dispatchMouseEvent` / `insertText` / `dispatchKeyEvent`。
  ② headless Chrome 点击后**大块内容可能不重绘**（截图里子导航高亮变了、内容区还是旧的 ⇒ 假绿/误导）
  ⇒ **交互正确性以 DOM 探针为准**，截图只用于静态视图（故本目录**不含**点击后的分区截图）。
- ⚠️ demo 构建走 HashRouter 时 `?section=` **不被读取**（WB / Trae 设置页同样如此，属既有行为；真实应用走 BrowserRouter 正常）。

`cargo test -p buddy-switch-core`：1309 passed / **2 failed** —— 两条 `trae` 用例**依赖真机装有 TraeWork 客户端 exe**
（本机 `D:\Programs\TRAE SOLO[ CN]\` 自 10-13 起只剩 `debug.log` ⇒ 走「未安装」分支；链路文件全部未改动，
**非本轮回归**，建议 trae owner 改成两分支感知，仿 `data_dir_missing_reason_distinguishes_not_installed_from_never_launched`）。

**下一步（按依赖序）**：① `disabled_models` 按 `Scope` 分家（先出方案）→ ② gateway `qoder/` 产品线（routes/pool/apikey/payload/sse）→
③ 两宿主 gateway host + 网关三卡 → ④ `refresh`/`credits`/`checkin` 上游模块 → ⑤ 真实账号端到端验证（真机探针留 `#[ignore]`）。

> 原型 ↔ 代码 的字段级对应见 §3；未接入项的**用户可见话术**统一走各页空态的 `pending.*` 词条（中英齐）。

---

## 7. 进度 · 第三轮（2026-10-15 晚 · **网关产品线 + 网关页落地**）

> 上面的「下一步 ①–④」本轮**全部做完了**；⑤（真实账号端到端）仍缺真机账号，走 mock 上游 e2e。

| 层 | 状态 | 说明 |
|:--|:--|:--|
| **L0** | ✅ | 无改动 |
| **L1** core | ✅ **补全** | `qoder/` 由 8 文件 → **14 文件**：新增 `cosy`（签名/编码：AES-128-CBC + RSA PKCS#1 v1.5 + 三段轮换 base64 + 5 行 md5 Bearer）、`api`（请求体构造 / 信封解包 / 清单 / openapi 通道：刷新·签到·额度·套餐）、`refresh`、`checkin`、`credits`；`device.rs` 的指纹算法按**官方向量**改成 MD5（原 SHA-256 与真机不符） |
| **L2** gateway | ✅ **接入** | 新 `qoder/` 产品线（`mod/routes/sse/pool/apikey/payload` 6 文件）：**独立监听 7865**（与 WB 57891 / Trae 7864 / 统一 57892 错开，**绝不 merge router**）；`chat_for_region` 一份中继（认证之后与统一入口共用）；`/v1/messages` 走 `protocol::anthropic` 双向转换；`/v1/responses` **501 明确拒绝**；模型级 429 冷却（只冷该模型）；`disabled_models` 键从 `Region` 升级到 **`Scope`**（读时扇出迁移，Qoder 不继承 WB/Trae 的旧键） |
| **L3** 两宿主 | ✅ **网关接线完成** | `buddy-switch-server::qoder_gateway_host` + `src-tauri::qoder_gateway` 各持一个 `GatewayHost<QoderGateway>`；**11 条管理命令 × 四处同步**（config / status / models / keys list·create·revoke·restore·delete / logs / logs clear）；`SCOPE_PARAMETERIZED` 22 → **25**（status / models / create 三条按 `scope` 分家，其余跨区域） |
| **L4** 前端 | ✅ **网关页落地** | `/s/qoder-{cn,global}/gateway` 由「尚未接入」空态换成**真实工作台**（与 WB / Trae **同构**：子导航去掉「请求代理」）：概览（开关 / 监听 / 端口 / Base URL / 概览两行 / 接入指引）×模型（上游实时清单 + 来源三级 + 禁用两出口）×密钥（多 Key + **归属区域** + 吊销/恢复/删除三态）×账号池（共用卡 + `QODER_POOL_TILES` = 可路由/冷却中/会话失效/**模型冷却**）×请求日志×统一入口；统一网关命名空间镜像表补 2 条（`qoder-cn` / `qoder-global`） |
| **L5** 门禁 | ✅ | `check:api` 绿（routes 204 / call 194 / server 186 / invoke 212 / 参数化 **25**）；`check:scope` **295 passed / 0 failed**；`tsc` 0；新组件已登记进「空态 / 共享 `formatDate`」防回退名单 |

**验证基线（第三轮）**：`cargo test -p buddy-switch-gateway` **511/0**（lib，含本轮 +3：池管理面视图的穷尽性、状态视图键集合钉住、管理面清单不过滤禁用）；集成 `qoder_gateway_e2e` 10/10（mock 上游：COSY 头真的到达 / 429 换号写回模型级冷却 / 信封 SSE / 禁用 403 / 501）；`cargo check`（gateway / server / tauri）全绿。

**两个刻意的形状决定（评审时别「顺手统一」）**：
1. **一个监听、两个区域**：Qoder 网关只有一份配置与一个端口，区域由**请求所用 API Key 的归属**决定（`QoderKeyRegion`）⇒ 状态视图里 `enabled/port/running` 是两区域**共用**的事实，**响应体不回显 `region`**（回显会造出「两个网关」的错觉）。
2. **管理面清单 = 未过滤全量 + 逐条 `disabled`**（与 Trae 侧「过滤后清单」不同）：表格要能同时显示「已禁用」的行，否则用户会以为它从上游消失了；「真正暴露的条数」= 全量 − 已禁用，与出口 ① **同源同判据**。形状由 `payload::admin_models_response` **唯一产出**（两宿主不各自拼 JSON）。

**仍未做（诚实清单）**：
- **真实账号端到端**：本机无 Qoder 账号 ⇒ 上游链路（COSY 签名 / 信封 SSE）只有协议级单测 + mock 上游 e2e，**首次真机跑通前不要把「网关可用」当作已有事实**（`api.rs` 里两条 `#[ignore]` 探针位）。
- 用量 / 会话两个目的地仍是诚实空态（数据源不存在）。
- `open_data_dir` 对 Qoder 作用域仍**可诊断拒绝**（未实现该平台的数据目录定位）⇒ 网关页刻意不渲染该按钮。
