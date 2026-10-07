# P12 spike 操作指引（请求代理与无缝切号 · 技术可行性验证）

> **这不是实现**，是 **spike（做不做得成）**。本目录只有**验证工具**，没有、也**不该有**
> 转发层 / 面板等半成品代码（设计 `P12-ARCH.md` §1.5：作废 = 不实现）。
>
> 结论写在 [`docs/redesign/P12-SPIKE-RESULTS.md`](../../docs/redesign/P12-SPIKE-RESULTS.md)。
>
> ⚠️ **`scripts/spike/*` 不得进打包产物**（设计决策 #4）。打包只消费 `dist/`（前端）与
> 编译出的二进制，`scripts/` 天然不在其中；`scripts/package-windows.ps1` 里加了护栏显式核对。

---

## 0. 分工总览：**哪些能自动跑、哪些必须你手动做**

| 步骤 | 假设 | 能否自动跑 | 为什么 |
|:---|:---|:---|:---|
| **H0** | 客户端**业务域**是否可配自定义 base URL | ✅ **能**（只读配置） | 读 WB / Trae 的配置文件与设置项即可判断；**绝不改** |
| **H1** | 客户端会**主动走 HTTP 代理**吗 | ⚠️ **一半** | 工具（起代理 + 记 CONNECT）能自动跑并自检；**把真客户端指过来**要你手动 |
| **H2** | 客户端**信任自签 CA、不报证书错误**吗（★ 真正 go/no-go） | ⚠️ **半自动**（见 §6） | **装 CA 必须你执行**（系统级改动，决策 #3）；但**起中间人 / 重放 / 判定是否报证书错误 / 打印卸载命令**都已自动 ⇒ 人工只剩**一条命令** |
| **H3** | 凭据可否**按请求替换** | ⚠️ **半自动**（见 §6） | **抓包 / 替换 / 判定上游是否 200** 已自动；**启动客户端 + 点一次业务操作**必须你手动（需要真实流量） |
| **H4** | 转发层**不改写 header 原始大小写** | ⚠️ **取决于有没有样本** | 本仓**没有任何已抓的业务请求样本** ⇒ 只能靠 H2 阶段顺带抓 |
| **H5** | 业务请求可**按方法+路径分类读/写** | ✅ **能**（离线，读本仓逆向成果） | 见 `P12-SPIKE-RESULTS.md` H5 一节 |

> **判据顺序不可颠倒**：**H0 最便宜、排最前**（只翻配置项，且**直接判死 C**）；
> **H1 先于 H2**（先确认客户端认代理，再装 CA —— 否则 CA 白装还得清理）。

---

## 1. H0 —— 只读客户端配置（可自动，**只读，绝不改**）

**目标**：判断客户端的**业务域**（不是只有 LLM 域）是否存在可配自定义 base URL 的项。

**动作**：**读**下列位置（本机实测路径，`$APPDATA` = `%APPDATA%`，`$HOME` = `%USERPROFILE%`）：

### WorkBuddy（Electron）
- 安装目录 `resources/app.asar.unpacked/cli/product.json`
  —— 关注 `endpoint` / `stagingEndpoint` / `officialEndpoints` / `productConfigPathEnv` / `productConfigEnv`
- 安装目录 `resources/app.asar`（桌面主进程也读同一套 env）
- 用户数据 `$HOME/.workbuddy/app/app-config.json`（本机仅 `{"locale":"zh-CN"}`）

### Trae（VSCode 系）
- 安装目录 `resources/app/product.json` —— 关注 `bootConfig`（业务域**硬编码**在此）
- 用户设置 `$APPDATA/Trae CN/User/settings.json` —— 只有 `trae.network.proxy.*`
- `$HOME/.trae/argv.json`（VSCode 永久启动参数，仅少数受支持项）

**命令**（示例，纯只读）：

```bash
node -e "const p=require('$APPDATA/Trae CN/User/settings.json');console.log(Object.keys(p))"
grep -o '"trae\.[a-zA-Z0-9._]*"' "<Trae安装>/resources/app/out/main.js" | sort -u
```

**结论**：见 `P12-SPIKE-RESULTS.md` 的 H0 一节（含读到的**配置键名 / 文件路径**证据）。

**还原**：无（本步只读，不改任何东西）。

---

## 2. H1 —— 最小代理只记录 CONNECT（工具可自动；指客户端要手动）

### 2.1 自检（离线，证明「代理会记录 CONNECT」）—— **可自动**

```bash
node scripts/spike/p12-proxy-log.cjs --selftest
```

它会在本机起一个假目标 + 代理，发一条**裸 CONNECT**，断言：
① 拿到 `200 Connection Established`；② 日志里出现该 CONNECT。**不需要真客户端、外网、CA**。

### 2.2 起代理（前台）

```bash
node scripts/spike/p12-proxy-log.cjs
# 或换端口 / 换日志位置：
P12_PROXY_PORT=18888 P12_PROXY_LOG="$PWD/logs/p12-h1.log" node scripts/spike/p12-proxy-log.cjs
```

- 默认只监听 **`127.0.0.1:18787`**（**只回环**，绝不对外）。
- 日志默认落在 `scripts/spike/p12-proxy-connect.log`（该 `*.log` 已被 `.gitignore` 忽略）。
- Ctrl-C 退出时打印**按域汇总**。

### 2.3 把**真客户端**指向该代理 —— **必须你手动**

| 客户端 | 怎么指 | 备注 |
|:---|:---|:---|
| **Trae** | 设置项 `trae.network.proxy.mode = manual`，`trae.network.proxy.manual.proxy = http://127.0.0.1:18787` | 本机 `settings.json` 现有 `trae.network.proxy.effectiveMode = "system"` |
| **WorkBuddy** | 走系统代理，或用 Chromium 启动参数 `--proxy-server=http://127.0.0.1:18787` | 建议改**系统代理**而不是改客户端文件 |

然后在客户端里**点开看积分 / 签到 / 旅行**，回到代理窗口看是否出现目标业务域 CONNECT。

### 2.4 还原 —— **必须做，且由你手动**

1. Trae：把 `trae.network.proxy.mode` 改回 `system`（或删掉 `manual.proxy`）。
2. WorkBuddy：把**系统代理**改回「无 / 直连」。
3. 关掉代理进程（Ctrl-C）。
4. **本步不涉及 CA**，所以无需移除任何证书。

---

## 3. H2 —— 自签 CA + TLS 中间人（★ 真正的 go/no-go）—— **半自动**

### 3.0 ★ 半自动 harness（把人工压到「一条命令」）

```bash
node scripts/spike/p12-h2h3-harness.mjs ca      # ① 打印 CA 路径 + 装/卸命令（**你执行**）
node scripts/spike/p12-h2h3-harness.mjs h2      # ② 起中间人 → 你去客户端点一次业务操作
node scripts/spike/p12-h2h3-harness.mjs judge   # ③ 自动判定（通过 / 失败 / 未完成）
```

**它自动做什么**：起中间人（自动带 addon + 域白名单）· 读日志**自动判定**（识别 TLS/证书类错误 = 证书固定）· 打印装/卸 CA 命令。
**它不做什么**（**刻意的**）：**不装 / 不卸 CA** · **不启动客户端** · **不落正文**。

**⚠️ 关于「提权」**：Windows 有**用户级**根证书库 ⇒ 下面这条**不需要管理员**：

```bash
certutil -addstore -user Root "%USERPROFILE%\.mitmproxy\mitmproxy-ca-cert.cer"   # 装
certutil -delstore -user Root "%USERPROFILE%\.mitmproxy\mitmproxy-ca-cert.cer"   # 卸（**务必**）
```

但**装证书仍是系统级改动** ⇒ 由**你**执行，harness **不代劳**（决策 #3：应用不得自动提权）。

**环境**：mitmproxy 装在隔离 venv `~/.workbuddy-ai/binaries/python/envs/p12spike/`
（harness 用 `P12_MITM_PY` 找它；换机器时重装即可）。

> ⚠️ **装 CA 到系统信任库是系统级改动，需要管理员权限。本工具绝不自动提权**（决策 #3）。
> 若你在这一步装完不放心，**先看 §3.4 的移除步骤**再决定是否继续。

### 3.1 前置

```bash
pip install mitmproxy           # 或 pipx install mitmproxy
```

### 3.2 装 CA（手动，需管理员权限）

1. 先启动一次 `mitmproxy`（见下），它会生成 CA 到 `~/.mitmproxy/`（`mitmproxy-ca-cert.cer` / `.pem`）。
2. **手动**把 `mitmproxy-ca-cert.cer` 装进：
   - **Windows**：`certmgr.msc` →「受信任的根证书颁发机构」→ 导入（**需管理员**）；
     或 `certutil -addstore -user Root "%USERPROFILE%\.mitmproxy\mitmproxy-ca-cert.cer"`（**当前用户**，无需管理员，但仅本用户生效）。
   - **macOS**：钥匙串访问 →「系统」钥匙串 → 导入 → 设为「始终信任」（**需管理员**）。

### 3.3 起中间人并重放 H1

```bash
P12_MITM_DOMAINS="www.workbuddy.cn,www.codebuddy.cn,api.trae.cn,trae-api-cn.mchost.guru,copilot.tencent.com" \
P12_MITM_LOG="scripts/spike/p12-mitm.log" \
P12_MITM_CAPTURE_DIR="scripts/spike/p12-capture" \
mitmproxy -s scripts/spike/p12-mitm-addon.py -p 18888
```

把客户端代理指向 `http://127.0.0.1:18888`（同 §2.3），再次点开业务功能。

**判据**：
- 客户端**不报证书错误** 且业务正常 ⇒ **H2 通过**（A 路可行）；
- 客户端**报证书错误**（或 `p12-mitm.log` 里出现 `ERR ... error=...`）⇒ **H2 失败**（= 证书固定 ⇒ **A 路出局**）。

### 3.4 移除 CA（**必须做，手动**）

- **Windows（当前用户）**：`certutil -delstore -user Root "<CA 指纹或名称>"`
  或 `certmgr.msc` →「受信任的根证书颁发机构」→ 找到 mitmproxy 证书 → 删除。
- **Windows（本机）**：`certutil -delstore Root "<CA 指纹或名称>"`（**需管理员**）。
- **macOS**：钥匙串访问 →「系统」钥匙串 → 找到 mitmproxy 证书 → 删除。
- **核验已移除**：重新打开「受信任的根证书颁发机构」，确认**没有** mitmproxy 条目
  （**绝不静默留一个受信任的自签 CA**）。

---

## 4. H3 —— 凭据按请求替换（**半自动**，需真实流量）

> **半自动**：`P12_MITM_REPLACE_AUTH='Bearer <token>' node scripts/spike/p12-h2h3-harness.mjs h3`
> ⇒ 起中间人并**自动替换**命中域的 `Authorization`；你只需在客户端点一次业务操作，
> 然后 `… judge` **自动判定**上游是 200（可换号）还是 401/403（该平台剔除）。

在 §3 的中间人基础上，先抓一个**真实业务请求**，记录它用的是哪个凭据头：

```bash
# 只记录、不改写（先看清请求长什么样）
P12_MITM_DOMAINS="www.codebuddy.cn" P12_MITM_LOG="scripts/spike/p12-mitm.log" \
mitmproxy -s scripts/spike/p12-mitm-addon.py -p 18888
```

再做**替换**实验（把 Authorization 换成另一个账号的 token，看上游是否 200）：

```bash
P12_MITM_DOMAINS="www.codebuddy.cn" \
P12_MITM_REPLACE_AUTH="Bearer <另一个账号的 access_token>" \
P12_MITM_LOG="scripts/spike/p12-mitm.log" \
mitmproxy -s scripts/spike/p12-mitm-addon.py -p 18888
```

**判据**：上游**接受**（200）⇒ 该平台凭据可换；上游**拒绝** ⇒ 该平台剔除。
⚠️ **Trae 例外**：换号还需重做 `device_proof`（P-256 ECDSA）签名，而私钥仅为 refresh 路径
持有（`crates/buddy-switch-core/src/modules/trae/icube.rs:382`）⇒ **本 addon 做不到**，
Trae 侧状态**如实记为「需额外打通」**（决策 #6）。

---

## 5. H4 / H5 —— 离线分析

- **H4（header 大小写）**：本仓**没有已抓样本** ⇒ 只能靠 §3 的 H2 阶段顺带抓。
  `p12-mitm-addon.py` 会把客户端**实际发出**的 header 名原始大小写记进
  `p12-mitm.log` 的 `client_header_names(raw)=[...]` 行，以及 `p12-capture/flows.jsonl`。
- **H5（读 / 写分类）**：**离线即可判定**，依据本仓逆向成果（`crates/.../cat.rs` /
  `credits.rs`）：读 = `GET`，写 = `POST`（`/activity/growth/tasks/accept|claim`）。
  详见 `P12-SPIKE-RESULTS.md`。

---

## 6. 硬边界（任何步骤都不得越线）

- **★ 绝不劫持 hosts / DNS / 透明代理**（D29 红线）—— 只接受**客户端主动**把请求交给代理。
- **★ 默认关**：任何代理逻辑都不自动启用；客户端是否指过来，由用户**手动**决定。
- **★ 只读配置**：spike 期间**绝不改**用户客户端配置文件（H0 只读）。
- **★ 不落盘正文**：工具默认只记元数据；`P12_MITM_ALLOW_BODY=1` 才会记正文（仅本地深度调试）。
- **★ CA 的装 / 卸都由用户手动完成**：应用不得自动提权（决策 #3）。
