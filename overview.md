# 多区域 WorkBuddy + API 网关改造概览

## 已完成

- 支持 WorkBuddy CN 与 WorkBuddy AI Global 两套区域配置、认证、账号库、会话、积分、签到、保活、模型目录与身份信息。
- 新增可复用 Rust API 网关 crate，提供 OpenAI 兼容与 Anthropic 兼容接口：`/v1/models`、`/v1/chat/completions`、`/v1/messages`。
- Tauri 桌面宿主与 npm/webui server 均接入网关；补齐账号目录打开、网关配置/状态/Key/策略/日志管理接口。
- 前端新增 API 服务页、Key 管理、模型目录、账号策略、请求日志和集成指引，并完成 CN/Global UI 隔离。
- 前端契约已对齐后端：`GatewayConfig` 与 `GatewayStatus` **统一使用 snake_case**；Key、日志、策略字段均已按后端实际输出修正。

### 测试收口（本轮补完）

- **`GatewayStatus` 契约统一**：新增显式契约结构体 `GatewayStatusView`（snake_case，`From<&GatewayConfig>`），`api_gateway_status` 不再手工拼 `json!`；响应键集合由测试精确钉死（8 键，无 camelCase 残留）。前端 `normalizeGatewayStatus` 保留双写法兼容。
- **`switch_account` region 绑定可证伪测试**：新增 `BUDDY_SWITCH_HOME` 隔离缝，据此建立 `crates/buddy-switch-core/tests/switch_region_binding.rs`，以 CN/Global 两套独立账号库与认证文件为夹具，证明 CN 薄包装只读 CN 账号库且只写 CN 认证文件（跨区账号 id 必须报错且零副作用）。此前该绑定**无法被证伪**。
- **server 路由级测试**：为 `crates/buddy-switch-server` 建立路由级测试脚手架（`tower::ServiceExt::oneshot` + `axum::body::to_bytes`），覆盖只读路由契约、`/api/gateway/keys` **不泄露 hash 与明文**的红线、API Key 生命周期 HTTP 出口、配置校验 400、SPA 深链回退与 404。此前 50+ handler 处于零路由覆盖状态。
- **修复 SPA 深链 MIME 缺陷**：`static_handler` 原先按**请求路径**推导 `Content-Type`，深链（如 `/accounts`）回退 `index.html` 时返回 `application/octet-stream`，浏览器会**下载**文件而非渲染应用（前端用 `BrowserRouter`，深链是常态）。现改为按**被服务的资源名**推导。
- **`BUDDY_SWITCH_HOME` 加护栏**：该覆盖值必须是非空、绝对、**已存在的目录**，否则忽略并打一次性 stderr 警告后回落真实 home，避免误设导致账号库静默迁移（用户视角「账号凭空消失」）。
- **消除 flaky**：`rotate.rs` 的跨日边界竞态（`decide_target` 内部二次 `now_ms()` 与整数日 `floor()`）已修，fixture 留 1h 余量；core 连跑 10 次 0 失败。
- **消除空洞断言**：`/api/accounts` 原先在空隔离库上断言 `== []`（恒真、打不红），现播种 1 条账号并断言具体字段。
- **消除 `/api/sessions` 空洞断言**：新增 `sessions_route_returns_seeded_session_rows`，用 `rusqlite` 播种真实 `workbuddy.db`（含 `custom_title` 覆盖、`is_playground`、软删除、跨账号、`claw` 工作区、以及「有/无正文 jsonl → `hasHistory`」等分支）与认证文件，断言**恰好**返回应返回的两条及其字段值、排序与逐项键集合。切断 `uid → sessions 表` 映射即变红。
- **修复前端产物目录冲突（`dist` 双用途陷阱）**：`npm run build`（WebUI/Tauri，Vite `base=/`）与 `npm run build:demo`（GitHub Pages 演示，`base=/trae-workbuddy-switch/`）原本**都输出到 `dist/`**，而 `dist/` 被三处消费：`rust-embed`（`crates/buddy-switch-server`）、Tauri `frontendDist`、`scripts/fix-app.sh`。一旦编译期 `dist/` 是演示构建，`index.html` 会请求 `/workbuddy-switch/assets/*`（embed 中不存在）→ 回退成 HTML → 浏览器模块脚本 MIME 校验失败（实测控制台：`Failed to load module script: … responded with a MIME type of "text/html"`）→ **webui 与桌面端双双空白页**（实测 `#root` 子节点数为 0）。现已把演示构建**分流**到 `dist-demo/`：
  - `package.json`：`build:demo` 增加 `--outDir dist-demo`；`vite.config.ts` 记录该输出目录约定。
  - `.github/workflows/pages.yml`：Pages 上传路径改为 `dist-demo`。
  - `.gitignore`：新增 `dist-*`，避免各类本地实验产物被纳入版本。
  - 实测确认：WebUI 构建写 `dist`（`/assets/index-*.js`），演示构建写 `dist-demo`（`/trae-workbuddy-switch/assets/index-*.js`），且**演示构建前后 `dist/index.html` 的 SHA256 完全不变**。
- **新增构建产物一致性护栏（回归测试）**：`embedded_index_html_references_only_embedded_assets` 断言内嵌 `index.html` 引用的**每个根绝对资源**都能在 embed 中命中，配套 `asset_refs_in`（提取器）与 `embedded_index_html_is_servable_at_root_and_index`。它把上述白屏事故变成测试期可见的错误——把演示构建塞回 `dist/` 后该测试立即变红并给出可执行提示。**server 测试 16 → 19。**

## 后续新增能力（2026-10 起，超出「多区域 + 网关」原范围）

以下三项在本次改造之后新增，属**应用级 / 观测类**能力，与网关无关；细节见 `README.md`「功能」章节与 `.trellis/tasks/09-22-*` 下的 PRD。

- **客户端限流台账**：从客户端本地数据（`projects/**/*.jsonl` + 只读 `workbuddy.db` 的 `sessions` 表 + 账号库 `auth_raw.account.uid`）**只读** join，汇总「账号 × 模型 × 恢复时刻」；按 `(uid, model, reset_at)` 去重取最近，`active = reset_at > now`，仍受限置顶高亮。**不写库、不改账号状态、不代理流量**。排队类错误码 `6020` / `6022` 的精确语义文案**待拍板**，界面按 R7 纪律只标注保守词「排队中」并原样带出 code。
- **Skill 管理**：扫描 `{data_dir}/skills/<dir>/SKILL.md` 的 frontmatter，清单化展示并逐项启用 / 停用（一并写齐 `user-invocable` / `disable-model-invocation`；**行级重写**、其余字节原样保留，含 CRLF/BOM）；支持跨区域（CN ↔ Global）搬迁补充（冲突保留目标侧）与从本地目录 / zip 导入。
- **客户端设置托管**：统一读写**被代理的上游客户端**（WorkBuddy 两版 + Trae 四个程序位）各自的 `settings.json`——6 槽位声明式规格表 + 格式指纹（缩进单位 + 行尾）+ **手术式字节级补丁**（改动最小化）+ 备份 / 恢复 + 跨槽位差异；**运行中进程直接拒绝写入**。⚠️ 与本工具自己的代理设置（`~/.buddy-switch/**`）是两个不同的文件，仅名字相近。

## 验证结果

- `cargo build -p buddy-switch-core` / `-p buddy-switch-gateway` / `-p buddy-switch-server` / `-p buddy-switch-rust`：均通过，0 error / 0 warning。
- `cargo test -p buddy-switch-core`：**224 passed / 0 failed**（lib）+ **8 passed / 0 failed**（`tests/switch_region_binding.rs`）。
- `cargo test -p buddy-switch-gateway`：**41 passed / 0 failed**，1 doc-test ignored。
- `cargo test -p buddy-switch-server`：**19 passed / 0 failed**。
- flaky 归零：core 连续 10 次全绿；`--test-threads=1` 与默认并行结果一致；server 连续 5 次全绿。
- `npx tsc --noEmit -p tsconfig.json`：通过。
- 生产构建（`npx vite build`）与演示构建：均通过；仅有既有的 chunk 大小警告。
- **SPA 深链已在真实浏览器验证**：以 `BUDDY_SWITCH_HOME` 隔离数据后启动 `buddy-switch serve`，浏览器访问 `/credit-stats` 得到 `document.contentType === "text/html"`、URL 保持在 `/credit-stats`、`#root` 已渲染且显示积分统计页内容；**未触发下载**。反向证伪：把 MIME 改回按请求路径推导后，同一访问**直接触发文件下载**（Playwright 报 `Downloading file credit-stats`），文档停留在 `about:blank`，且 3 条单测同时变红。
- **隔离有效性**：全部测试跑前跑后对 `~/.buddy-switch/` 与认证目录做快照比对，文件数与 mtime 逐行一致，无 `gateway_keys.json` / `gateway_config.json` 落入真实目录。
- **可证伪性**：由工程师与 QA 各自独立做变异注入，确认关键断言真的会变红（`Region::Cn`→`Global`、`masked()` 注入 `hash`、`base_url`→`baseUrl`、MIME 改回按请求路径、删除覆盖护栏、使 `/api/accounts` 恒空等），全部还原并复跑确认全绿。

## 已知限制与后续建议

- **`npm run build:demo` 是 POSIX-only（既有约定）**：脚本用内联环境变量语法（`VITE_DEMO_MODE=1 VITE_PAGES_DEMO=1 vite build`），在 npm 的 Windows shell（cmd.exe）下会以 `'VITE_DEMO_MODE' 不是内部或外部命令` 失败。该脚本只在 Linux 的 GitHub Pages CI 上运行，与 `build:app`（macOS-only）同属既有约定，故保留不动。**Windows 上要本地构建演示版，请用等价写法**：
  ```powershell
  $env:VITE_DEMO_MODE="1"; $env:VITE_PAGES_DEMO="1"
  npx vite build --outDir dist-demo
  Remove-Item Env:VITE_DEMO_MODE, Env:VITE_PAGES_DEMO
  ```
- **改前端后宿主必须重编才能生效**：`rust-embed` 是**编译期**嵌入 `dist`，判据是**构建顺序**（新 `dist` → 宿主 crate 重新 `Compiling`），别 grep 二进制。
  - 手动 `cargo build -p buddy-switch-server` **不会**因 `dist` 变化自动重编（实测输出 `Finished in 0.56s` 且**没有** `Compiling` 行），二进制保持嵌入旧前端；此时需强制重编（`cargo clean -p buddy-switch-server`，或触碰 crate 源码 mtime）。
  - **但走 `build-quick.cmd` / `build-exe.cmd`（= `scripts/package-windows.ps1`）会自动处理**：打包发生在 `vite build` 之后，宿主会重新 `Compiling`，前端改动天然进包（2026-09-24 实测），产物还会被自动收进 `deliverables/`。只有**手动** `npx tauri build` 时才需要自己管顺序。
  - `embedded_index_html_references_only_embedded_assets` 护栏是这一点的补偿：只要重编发生在正确产物之后，它就守住底线。
- **`/api/checkin/status` 与 `/api/gateway/logs` 仍是空态断言**（P2）：播种前者会触发真实上游网络请求，故未补实；这两条断言当前只能验证 shape。
- **`/api/checkin/status` 是只读 GET 却会触发上游网络调用**（P3，设计层面，非本轮引入）。注意 `buddy-switch serve` 启动时 `spawn_background_loops()` 会执行一次 `checkin::run_checkin_cycle(StartupVerify)`，并会改动 `~/.buddy-switch/` 下的账号与缓存文件——本地做实验时务必先用 `BUDDY_SWITCH_HOME` 指向**已存在**的目录隔离。
- **前端无单测框架**，故未跑前端单测，仅以 `tsc` + 生产构建作为门禁。
- **未运行 Tauri 运行时验证**（仅保证其 crate 可编译）；未做并发压测（受 `CARGO_INCREMENTAL=0` + cargo 串行约束）。
- **Git 仓库已建立并发布**（2026-09-24 起）：远端 `NextAgentX/trae-workbuddy-switch`，已打 tag `v2026.9.221126` / `v2026.9.241700` / `v2026.9.242126` 并发布 Release（四平台安装包 + 签名 + updater 清单）。
  ⚠️ 遗留：release 工作流的 4 个 `npm platform *` job 因缺 `NPM_TOKEN` secret 持续失败 ⇒ **run 整体标 `failure`，但 Release 与安装包本身 success**（npm 是独立 job，不阻塞 Release）。判读 CI 结论必须看**具体 job**，不能只看 run 状态。

## 重要环境说明

Rust 验证使用 Rust 1.98.1 MSVC；cargo 调用必须串行，建议设置 `CARGO_INCREMENTAL=0`，避免多个进程并发写同一 target 导致 rustc 增量缓存损坏和 ICE。

`BUDDY_SWITCH_HOME` 仅用于可移植部署与测试隔离：值必须是**已存在的绝对目录**，否则被忽略并打一次性 stderr 警告后回落真实用户主目录。环境变量是进程级全局状态，需要设置它的测试必须用互斥锁串行化并在结束时恢复（见 `tests/switch_region_binding.rs` 的 `ENV_LOCK` 策略）。

**该护栏已在真实运行中验证生效**：把 `BUDDY_SWITCH_HOME` 指向一个**尚未创建**的目录后启动 `buddy-switch serve`，程序正确拒绝并打印
`[buddy-switch] 环境变量 BUDDY_SWITCH_HOME="…" 已忽略：必须是一个已存在的目录；回落到真实用户主目录。`
随后回落真实 home —— 这正是该护栏要防住的「静默迁移账号库」陷阱。因此用隔离目录做实验前，**必须先确认该目录已存在**，否则隔离不会生效。

前端产物与测试依赖：
- `crates/buddy-switch-server` 的 dev-dependencies 新增 `tower`(util) 与 `rusqlite`(bundled)，两者均已在 `Cargo.lock` 与本地 registry 缓存中，**零联网**。
- `dist/` 被 `.gitignore` 忽略，属纯本地构建产物。
