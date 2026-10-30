# 开发指南

## 环境要求

Node.js ≥ 20、Rust stable、macOS（或 Windows/Linux）。

## 开发命令

```bash
npm install
npm run tauri dev        # 开发模式
npm run build:app        # 构建 debug .app（含前端资源补丁）
npm run build:app:release  # 构建 release .app + 签名更新包
```

## 本地核验的两个坑（都踩过，各浪费过一轮排查）

### 1. 本机代理会接管 `127.0.0.1` 的请求 —— curl 必须显式绕开

若环境里有 `http_proxy` / `HTTP_PROXY`（开发机实测是 `http://127.0.0.1:51093`），
`curl http://127.0.0.1:<port>/...` **会被代理接管**，拿到的是代理的
`502 upstream connect failed`（或别的服务的 HTML）—— 看起来像「本地服务根本没起来」，
于是会去查一个并不存在的问题。

⇒ **核验本地端口一律 `curl --noproxy '*' ...`**。这条对 hook 接收端 / 网关 / webui 一视同仁。

> 顺带一个 shell 坑：`C="curl -s --noproxy *"` 这样的**变量拼接**在 `$C` 展开时会做
> glob —— `*` 会变成**当前目录的全部文件名**，于是 curl 把一堆文件名当 URL 去请求。
> 要写成函数：`c() { curl -s --noproxy '*' "$@"; }`。

### 2. `npm run build:fast` 可能被批量删除守卫拦下（受限环境）

vite 构建前会清空 `dist/assets`。当目录里的文件数**超过守卫阈值（50）**时，构建在
`prepare-out-dir` 阶段直接失败：

```
[vite:prepare-out-dir] [safe-delete][SAFE_DELETE_BULK_CONFIRM_REQUIRED] {"count":52,"threshold":50,...}
```

**这不是代码问题**，也不代表前端构建坏了。绕法：先把 `dist` **改名**（不是删除）
挪走再构建：

```bash
mv dist dist.old.$$ && npm run build:fast
```

## 发布新版本

签名密钥（自动更新用）存放于 `~/.buddy-switch/buddy-switch-updater.key`，构建脚本通过
`TAURI_SIGNING_PRIVATE_KEY` 注入。发布新版本时：

0. ★ **打 tag 之前**先 `sh scripts/stamp-version.sh --tag v<版本>` **再** `npm run notes:json`，
   把 `src/data/release-notes.json` 一起提交。
   为什么顺序不能反：内置说明里的版本号是**离线时的兜底文案**（在线时读 Release 资产里的
   `release-notes.json`），它比应用版本旧就会在断网时显示上一版内容 —— `pr-check` 有一条
   门禁专门钉这件事（「打包内置的版本说明必须与当前版本一致」）。
   本次 v2026.10.300953 就是先 tag 后补，导致该门禁在 main 的 push 上红了一次。
1. `npm run build:app:release` 生成 `.app.tar.gz` + `.sig`（Windows NSIS 构建会额外生成当前版本的 `*_x64-setup.exe` + `.exe.sig`）。CI 会先清掉 `target/**/release/bundle`，避免 cargo cache 把旧安装包带进 Release。
2. macOS：`UPDATE_OS=macos UPDATE_ARCH=aarch64 sh scripts/gen-update-json.sh` 生成 `latest-macos-aarch64.json`；Intel 用 `UPDATE_ARCH=x86_64`
3. Windows：`UPDATE_OS=windows UPDATE_ARCH=x86_64 sh scripts/gen-update-json.sh` 生成 `latest-windows-x86_64.json`
4. Linux：`UPDATE_OS=linux UPDATE_ARCH=x86_64 sh scripts/gen-update-json.sh` 生成 `latest-linux-x86_64.json`。
   只有 **AppImage** 能自更新（`deb` 归系统包管理器，无法原地升级）⇒ 认的是
   `*_amd64.AppImage` + 同名 `.sig`（**不是 tar.gz**：tauri-bundler ≥2.10 把 AppImage 当
   v2 自包含更新目标，updater 直接下载 AppImage 本体替换自身）；
   清单同时写 `linux-x86_64-appimage` 与 `linux-x86_64` 两个平台键（前者是新版客户端的首选候选，
   后者兜住回退路径）。
5. `python3 scripts/merge-update-manifests.py <产物目录>` 合并为 `latest.json`，并把 Windows 平台项写入 `latest-macos-x86_64.json`（兼容已安装的 Windows 客户端）
6. 将安装包、签名更新包、`latest*.json` 一并上传到 GitHub Release

### npm 版（webui）发布

主包与平台分包统一在 **`@nextagentx` scope** 下：主包 `@nextagentx/buddy-switch`，
平台包 `@nextagentx/buddy-switch-<platform>-<arch>`（**目录名不带 scope**，
仍是 `npm/platform/buddy-switch-<tag>`，`build.yml` 的 `PKG_DIR` 与生成脚本都按目录名拼路径）。
安装后的**命令名仍是 `buddy-switch`**（`bin` 字段决定，与包名无关）。

1. 编译 server 二进制并上传 GitHub Release（`.github/workflows/build.yml` 自动执行）
2. 先 `sh scripts/gen-platform-packages.sh <版本>` 生成 5 个平台包，把对应二进制放进各包 `bin/` 后逐个 `npm publish --access public`
3. `cd npm && npm publish --access public`（主包，`postinstall` 从已安装的平台包复制二进制）

> ⚠️ **发布前提**：npm 账号必须拥有 **`nextagentx` 这个 scope**（用户名即为 `nextagentx`，
> 或在该账号下创建同名 organization）。scope 不属于自己时 `npm publish` 会 403，
> 且 `@nextagentx/*` 是别人无法代持的命名空间。
>
> 另外：`buddy-switch`（不带 scope）这个包名**在 npm 上已被他人占用**，所以不能再退回无 scope 命名；
> 本项目此前的发布用的是 `workbuddy-switch`。
> scoped 包必须带 `--access public`，否则会以私有包发布（私有包需要付费账号）。
>
> `npm/package.json` 的 `optionalDependencies` 列了 5 个平台（darwin-arm64 / darwin-x64 /
> win32-x64 / linux-x64 / linux-arm64），与 CI 矩阵**一一对应**，改动任一侧都要同步另一侧
> （`stamp-version.mjs` 会把它们的版本号统一重写，但**不会**替你补漏掉的平台）。

#### linux-arm64 是「CLI only」

CI 矩阵里 `linux-arm64` 的 `bundles` 为空 ⇒ 跳过 `Build desktop app`，`update_arch` 为空 ⇒
跳过 updater 清单。**它只产出 CLI 裸二进制**（npm 平台包 + Release 资产），**没有桌面 App**。

原因：桌面 App 走 Tauri，需要 webkit2gtk；在 x64 runner 上交叉编 arm64 版要开 dpkg 多架构并装
`libwebkit2gtk-4.1-dev:arm64`，代价高且易碎。而 CLI 是纯 Rust（reqwest 用 rustls、rusqlite 走
`bundled`），交叉编译很干净，只需：

- `apt install gcc-aarch64-linux-gnu`
- job 级 env：`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc`
- job 级 env：`CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc`
  （**不能省**：`libsqlite3-sys` 用 `cc` 现场编译 SQLite 的 C amalgamation，
  不指定交叉 C 编译器就会拿 host gcc 产出 x86_64 目标文件，链接期报 `file in wrong format`）

⚠️ 若以后要给 linux-arm64 加桌面 App，改的不止是 matrix：`Build desktop app` 的
`if: matrix.bundles != ''` 会放行，随后 webkit2gtk 的 arm64 依赖必须真的装上。

#### npm 通道开关：`PUBLISH_NPM`（**当前关闭**）

CI 的两个发布 job（`publish-platform` / `publish-main`）与「是否把 CLI 裸二进制留在 Release 里」
都由**同一个仓库变量**控制：Settings → Secrets and variables → Actions → **Variables** → `PUBLISH_NPM`。

| `PUBLISH_NPM` | npm job | Release 里的 `buddy-switch-*` 裸二进制 |
| --- | --- | --- |
| 未配置 / 非 `true`（**当前**） | skipped（不红，不再需要 `NPM_TOKEN`） | **保留** —— webui 形态唯一的下载渠道 |
| `true` | 正常运行（需要 `NPM_TOKEN`） | 剔除（已随平台包发到 npm，Release 只留桌面 App） |

**为什么要有这个开关**：npmjs 账号注册/访问受限期间拿不到 token，此前每次 tag 都会红 4 个
`npm platform *` job、`publish-main` 被 skip，而 Release 与安装包本身一直是 success ——
那是噪音不是事故，却会让人误判「这次发版挂了」。恢复发布时**只改变量**，不用改 workflow。

**关闭期间的 webui 分发方式**：从 GitHub Releases 下载对应平台的裸二进制直接跑
（`buddy-switch` / `buddy-switch serve --port 57890` / `status` / `version`）。
macOS 未签名会触发 Gatekeeper，先 `xattr -d com.apple.quarantine <路径>`；Windows 会有 SmartScreen 提示，选「仍要运行」。

### 在线演示（GitHub Pages）部署前提

`.github/workflows/pages.yml` 在每次 push 到 `main` 时构建只读演示并部署。
**首次部署前必须先手动启用 Pages**：仓库 Settings → Pages → Source 选 **GitHub Actions**。

未启用时该工作流会**恰好失败在 `Configure Pages` 这一步**（前面的 `npm ci` 与
`npm run build:demo` 都是通过的，容易误判成构建坏了），并且此后每次 push 都会留一条红的 run。

> **不要试图用 `enablement: true` 绕过这一步**：`actions/configure-pages` 的文档明确要求该选项
> 使用 `GITHUB_TOKEN` **以外**的 token（PAT 的 `repo` scope，或 GitHub App 的
> `administration:write` + `pages:write`），加了不但仍然失败，还会平白引入一个密钥依赖。

## 目录结构

### 根目录：哪些是入口、哪些是产物

根目录只放**必须在这一层**的东西（构建工具按约定找它们）；其余一律进子目录。

| 条目 | 是什么 | 能不能动 |
| --- | --- | --- |
| `build-quick.cmd` | **双击入口**：日常打包（默认 `fast` 跳过 tsc，`full` 走全套门禁）。内部靠 `%~dp0` 定位 `scripts\` 与 `deliverables\` | ⚠️ 只能留在根；位置与文件名是使用习惯的一部分 |
| `build-exe.cmd` | **双击入口**：完整打包（= `scripts/package-windows.ps1`），产物进 `deliverables\` | 同上 |
| `index.html` `vite.config.ts` `tsconfig*.json` `components.json` | 前端构建入口与工具配置，工具链按固定路径找它们 | 不能动 |
| `package.json` `package-lock.json` / `Cargo.toml` `Cargo.lock` `rustfmt.toml` | 两套依赖清单与格式化配置 | 不能动 |
| `Dockerfile` `docker-compose.yml` `.dockerignore` `docker/` | 服务端容器形态；`docker/entrypoint.sh` 被 Dockerfile 引用 | 构建上下文必须在根 |
| `README.md` / `README.en.md`（中英两版）`CONTRIBUTING*.md` `LICENSE` `AGENTS.md` | 面向使用者与协作者的门面；`AGENTS.md` 另含 UI / 提交 / 文档规范 | 不能动 |
| `src/` `src-tauri/` `crates/` `npm/` `scripts/` `docs/` `public/` | 源码、宿主、核心库、npm 包、工具脚本、文档、Web 静态资源 | — |
| `node_modules/` `target/` `dist/` `deliverables/` | 依赖与构建产物（**全部被 `.gitignore` 忽略**） | 可随时删，重跑 `npm ci` / 构建即回来 |

> 想往根目录加文件前先问一句：**构建工具或使用习惯是否非得在根？** 不是的话放进 `scripts/`（工具）、`docs/`（文档）、`public/`（前端静态资源）。

### 源码目录

```
src-tauri/
  src/
    commands.rs      # Tauri command 薄包装（对应 Python 版 HTTP API）
    lib.rs           # 宿主装配：托盘、窗口、后台任务登记表、三份网关宿主
    gateway.rs / trae_gateway.rs / unified_gateway.rs   # 三个网关的宿主（各占一个端口）
crates/
  buddy-switch-core/    # 核心逻辑：account/auth_file/oauth/process/switch/session/checkin/refresh/update/config
                        # 另有 product.rs（Product/Scope 唯一事实来源）、gateway_namespace.rs（统一入口命名空间）、
                        # trae/（变体、区域、OAuth、快照迁移）、agent_watch.rs（外部会话监听状态机）等
  buddy-switch-gateway/ # OpenAI / Anthropic 兼容网关 —— **三套产品线**：WB / Trae / 统一入口，
                        # 共用 GatewayHost 与 pool/（账号池、选号策略 entry_like.rs）
  buddy-switch-server/  # HTTP server + CLI：axum API + rust-embed 前端；三个网关宿主各一份
src/                 # 前端：components/pages/lib（api.ts 双通道：Tauri invoke / HTTP fetch）
npm/                 # npm 包：package.json + bin + scripts/install.js
docs/
  README.md          # ★ 文档地图（唯一入口）：按生命周期分区，新文档放哪看这里
  guides/            # 上手类：本文件 / DOCKER.md / GATEWAY-CLIENT-INTEGRATION.md
  specs/             # 现行架构与决策（PLAN / P*-ARCH / DECISIONS / VOCABULARY）—— 改行为就同步改
  history/           # 只读历史：审计、探针、外部调研、旧方案
  prototype/         # 可点原型 + JSDOM 校验套件（check:scope 会直接读这里）
  releases/          # 每版发布说明（gen-release-notes.mjs 约定路径）
  images/            # README 截图（浅/深两套）+ brand/（图标源图，scripts/gen-icons.mjs 默认读它）
```

## 端口与文件速查

| 端口 | 归属 | 说明 |
| --- | --- | --- |
| `57890` | webui 界面本体 | `buddy-switch serve --port` 可改 |
| `57891` | WorkBuddy 网关 | 默认关；`~/.buddy-switch/gateway_config.json` |
| `57892` | **统一入口**（跨产品线） | 默认关；`~/.buddy-switch/gateway_unified.json` + `gateway_unified_keys.json` |
| `57893` | 客户端会话监听的 hook 接收端 | 需在「设置 → 客户端」显式开启 |
| `57894` | Trae 网关 | 默认关（原 `7864`） |
| `57895` | Qoder 网关 | 默认关（原 `7865`） |
| `8899` | Trae 本地 MITM 代理 | 登录态捕获依赖它（**不是网关**） |

> ★ 2026-10-17 起四条网关统一落在 `5789x` 一段（57891 / 57892 / 57894 / 57895），不再出现「WB 是 5789x、Trae/Qoder 是 78xx」的混编。**只改默认值**：配置文件里写着旧端口的保持原端口。

四个网关的 `/v1/models` 与 `/v1/chat/completions` **逐字重叠** ⇒ 不能 merge 进同一个 Router（axum 同路径 merge 是**运行期 panic**，有护栏测试 `two_products_own_overlapping_paths` 钉住）。所以永远是四个端口、四套 Key 库、四份配置。

## 排查问题：先看运行日志

本工具**自己**的运行日志在 `~/.buddy-switch/logs/runtime-YYYY-MM-DD.log`（按日纯文本），
桌面端「设置 → 日志」与 webui 同一路径可直接查看（级别 / 日期 / 关键字筛选）。

| 想调什么 | 怎么调 |
| --- | --- |
| 日志级别 | 环境变量 `BUDDY_SWITCH_LOG_LEVEL`（`error` / `warn` / `info` / `debug`，缺省 `info`），**改后要重启** |
| 日志上限 | 代码常量 `runtime_log::KEEP_DAYS`(7) / `MAX_PART_BYTES`(4 MiB) / `MAX_PARTS_PER_DAY`(4) |
| 想看更细 | 设为 `debug` 重启；`debug` 及以上会写「选号明细」这类逐条记录 |

> ⚠️ **别和另外两份日志搞混**：`gateway_logs.json` 是客户端经网关的**请求日志**（JSON、面向统计，在网关页看）；
> 上游客户端自己的日志在各自数据目录里。三者用途不同，刻意不合并。
>
> ⚠️ 桌面宿主是 `windows_subsystem = "windows"` 的 **release GUI（没有控制台）**，
> 所以 `eprintln!` 在用户机器上等于写进虚无 —— **诊断输出一律走 `runtime_log`**，
> 新增宿主级日志请用 `src-tauri/src/lib.rs` 的 `host_log(...)`（debug 构建会同时打 stderr）。

## 门禁（改完必跑）

```bash
npm run check:api              # 前端 API 契约 ↔ 后端一致（routes / call / server / invoke 四个面）
npm run check:store-selectors  # zustand 选择器
npm run check:scope            # 作用域注册表 + 原型行为对照
npm run check:npm-platforms    # npm 平台包与 CI 矩阵一致
npx tsc --noEmit               # 前端类型
```

`npm run build` 会按顺序把前四个门禁串起来再 `vite build`。**改任何命令面（新增/改名）必须跑 `check:api`** —— 新增一条命令要同步四处：`src/lib/api.ts` 的 `ROUTES` + `call()`、`src-tauri/src/lib.rs` 的 `generate_handler!`、`src-tauri/src/commands.rs`、`crates/buddy-switch-server/src/api.rs` 的 `.route`。

> Rust 侧：`cargo check --workspace` 能跑且包含 `src-tauri`；但 `cargo test --workspace` **必失败**（`src-tauri` 在测试配置下编不过），测试一律用 `-p` 排除它。cargo 必须串行（`-j 1`）并建议 `CARGO_INCREMENTAL=0`，否则并发写同一 target 会坏增量缓存并触发 rustc ICE。

## 隐私注意事项

- 仓库不提交本地数据（accounts.json、认证文件、密钥、token 由 `.gitignore` 排除）
- 发布前用 `git grep` 扫描 token 模式（`ghp_`/`npm_`/`gho_` 等）
