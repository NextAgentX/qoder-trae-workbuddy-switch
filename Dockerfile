# syntax=docker/dockerfile:1.7
#
# Buddy Switch 服务端镜像（webui + API 网关，headless）。
#
# ## 这个镜像是什么 / 不是什么
#
# **是**：`buddy-switch` 服务端 —— 本地 webui（账号池、签到、积分、网关管理）与
# 三个 API 网关入口（WorkBuddy / Trae / 统一）。它自带账号库与网关 Key，
# 数据落在 `BUDDY_SWITCH_HOME`（默认 `/data`）。
#
# **不是**：桌面客户端。切换客户端登录态、导入本机账号、重启客户端、托盘、悬浮窗
# 这些能力依赖**同机的图形客户端**，容器里一律不可用（webui 上会显示为不可用/无数据）。
# 换句话说：镜像里能用的是「多账号 API 网关 + 管理面板」，不是「桌面切换器」。
#
# ## 构建顺序（不可颠倒）
#
# `crates/buddy-switch-server/src/api.rs` 用 `#[folder = "../../dist/"]` 把前端
# **编译期**嵌进二进制。因此必须先产出 `dist/` 再编 Rust —— 顺序反了会嵌进一个空目录，
# 表现为「容器起来了、页面 404」，而构建**全绿**。本文件用两个阶段强制这个顺序：
# `.dockerignore` 排除了宿主机的 `dist/`，阶段 2 只接受阶段 1 的产物。
#
# ## 构建
#
#   docker build -t buddy-switch:local .
#
# ## 运行
#
#   docker compose up -d          # 见 docker-compose.yml（默认只发布到 127.0.0.1）
#
# 详细说明与安全须知见 `docs/guides/DOCKER.md`。

# =============================================================================
# 阶段 1 · 前端：产出 dist/
# =============================================================================
FROM node:22-bookworm-slim AS frontend

WORKDIR /src

# 先只拷清单再装依赖：源码改动不会让 node_modules 那层失效。
# `npm ci` 而非 `npm install` —— 严格按 package-lock.json 装，构建可复现。
COPY package.json package-lock.json ./
RUN npm ci

# 再拷源码。
# ⚠️ `npm run build`（不是 `build:fast`）：它带 `tsc` 类型检查与**全部**契约门禁
# （api 契约 / docker / store 选择器 / 作用域注册表 / npm 平台表 / hook 来源 / Pages base）。
# 镜像与发布走同一条路径，否则容器里跑的可能是一份类型就不通过的前端。
#
# ★ 这里**没有** Chrome：原型的浏览器依赖动态检查（`verify-cc-switch-bridge.cjs`）
#   在无浏览器时只「跳过」而不判失败（`REQUIRE_BROWSER_CHECKS` 未置）——
#   那是环境事实，不该把镜像构建拖垮。2026-11-10 实测：该套件以退出码 1 结束 ⇒
#   `npm run build` 失败 ⇒ **镜像发不出去**，而失败原因与镜像内容毫无关系。
#   「动态检查必须真的执行」这条硬要求落在 pr-check（ubuntu runner 自带 Chrome，
#   那里置 `REQUIRE_BROWSER_CHECKS=1`），不在镜像构建里。
COPY . .
RUN npm run build

# =============================================================================
# 阶段 2 · Rust：编 buddy-switch-server（只编它，不编 Tauri 桌面壳）
# =============================================================================
FROM rust:1.98-bookworm AS builder

WORKDIR /src

# 全量拷源码（.dockerignore 已排除 target/ 与 dist/）。
# 刻意**不做**「先拷清单编依赖」的分层优化：本工作区含 4 个 crate 且相互依赖，
# 造桩（stub src/）的写法很脆（bin crate 要 main.rs、lib crate 要 lib.rs，
# 漏一个就 E0583），换来的只是重复构建时的提速。正确性优先。
COPY . .

# ★ 前端产物从阶段 1 取，**不用**宿主机上的 dist/ —— 那是「构建顺序颠倒」的入口。
COPY --from=frontend /src/dist ./dist

# 只编服务端：`src-tauri`（Tauri 桌面壳）在 Linux 上要 webkit2gtk 等系统库，
# 编它既没必要也会让镜像构建失败。cargo 仍会解析整个工作区（能过），只是不编译它。
RUN cargo build --release -p buddy-switch-server \
    && strip target/release/buddy-switch

# =============================================================================
# 阶段 3 · 运行时
# =============================================================================
FROM debian:bookworm-slim AS runtime

# ca-certificates 是**必需**的：全部上游都是 HTTPS，缺了它所有出站请求都会以
# 「证书校验失败」告终（表现为网关全 502，而容器日志里看不出是缺 CA）。
# curl 只服务 compose 的 healthcheck（slim 镜像里没有 wget/curl）。
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# 非 root 运行。
RUN useradd --create-home --uid 10001 --shell /usr/sbin/nologin buddy

COPY --from=builder /src/target/release/buddy-switch /usr/local/bin/buddy-switch
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# 数据目录。CLI 对 `BUDDY_SWITCH_HOME` 是 **fail-closed** 的：指向不存在或非绝对的
# 目录会**拒绝启动**（防的是「以为在隔离目录、实际写到真实数据上」）。
# entrypoint 负责 `mkdir -p`，这里先把目录建好并交给 buddy，让**命名卷**首次创建时
# 能继承正确的属主；**bind mount** 需要使用者自己 chown（见 docs/guides/DOCKER.md）。
RUN mkdir -p /data && chown -R buddy:buddy /data
VOLUME ["/data"]

USER buddy
WORKDIR /data

# 容器内必须绑 0.0.0.0，否则宿主访问不到（CLI 默认 127.0.0.1）。
ENV BUDDY_SWITCH_HOME=/data \
    BUDDY_SWITCH_HOST=0.0.0.0 \
    BUDDY_SWITCH_PORT=57890

EXPOSE 57890

# `--no-open`：容器里没有浏览器，不加它每次启动都会白跑一次 `xdg-open`。
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["serve", "--no-open"]
