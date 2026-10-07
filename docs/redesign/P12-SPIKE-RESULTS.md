# P12 spike 结论：请求代理与无缝切号（**技术可行性验证**）

> **状态**：spike 进行中（2026-10-05）· **本文件是 T01 的产出**，随验证推进更新。
> **性质**：**这是「做不做得成」的结论，不是实现**。A / C 都出局 ⇒ 整个 P12 作废、**不写半成品代码**（设计 `P12-ARCH.md` §1.5）。
> **上游**：`P12-ARCH.md`（spike 设计 · H0–H5 假设 + 作废判据）；`DECISIONS.md` D29–D34；`PLAN.md` §3.19。
> **工具**：`scripts/spike/p12-proxy-log.cjs`（H1 最小代理）· `scripts/spike/p12-mitm-addon.py`（H2/H3/H4）· `scripts/spike/README.md`（操作 + 还原）。
>
> ⚠️ **诚实纪律**：**结论可能是「不可行」，那是合格产出。本文件不弱化作废判据、不把「C 覆盖 LLM」说成「P12 达成」。**
> ⚠️ **本文件只写「自动化部分」的结论**；H2 / H3 及 H1 的「真客户端」半段**必须用户手动执行**，已逐条标注。

---

## 0. 一句话结论

**C 路可行（H0 通过：WorkBuddy 业务域**有**覆盖通道）；A 路未判定（H1 真客户端半段 + H2 + H3 需用户手动）。**
⇒ **P12 不判死**。但有一处**与设计预期相反**的发现需要如实指出：

> 设计预期「C 大概率覆盖不到业务请求」（`P12-ARCH.md` §1.1 / D34）——**在 WorkBuddy 侧不成立**：
> WorkBuddy 的业务域基址（`endpoint`）**是可覆盖的**（见 H0）。**Trae 侧则成立**（业务域硬编码，C 只能覆盖 LLM）。

⇒ 按设计决策 #1 的语义，**C 记「部分通过」**（Trae 只覆盖 LLM，与标题目标有落差）；
但 **WorkBuddy 侧 C 可能覆盖业务请求**（**待运行时 mock 确认**，见 H0 的「★ 未做」）。

---

## 1. 验证环境（本机实测的客户端安装与数据目录）

> 证据来源：`find` / `cat` 只读探测（**未改任何客户端配置**）。

| 客户端 | 安装目录 | 用户数据 / 设置 | 关键配置载体 |
|:---|:---|:---|:---|
| **WorkBuddy**（Electron） | `D:\Program Files\WorkBuddy`（`WorkBuddy.exe` + `resources/app.asar`，`appVersion 5.6.2`） | `%APPDATA%\WorkBuddy`（空）、`%USERPROFILE%\.workbuddy\app\app-config.json`（`{"locale":"zh-CN"}`） | `resources/app.asar`、`resources/app.asar.unpacked/cli/product.json` |
| **Trae**（VSCode 系，CN） | `D:\Programs\Trae CN`（`Trae CN.exe`，`product.json` 内 `appVersion 3.3.104`） | `%APPDATA%\Trae CN\User\settings.json`、`%USERPROFILE%\.trae\argv.json` | `resources/app/product.json`（`bootConfig`）、`out/main.js`（设置项） |
| （CodeBuddy CN IDE） | 未装（`%APPDATA%\CodeBuddy CN\User\globalStorage` 为空目录） | — | — |

---

## 2. H0–H5 逐条判定

### H0 —— 客户端**业务域**可配自定义 base URL 吗 ⇒ **C 路可用吗**

| 项 | 内容 |
|:---|:---|
| **statement** | 客户端业务域（非仅 LLM 域）存在可指向自定义 base URL 的配置项 |
| **how** | **读** WorkBuddy / Trae 的配置文件与设置项（只读）；**绝不改** |
| **pass** | 业务域**可配** |
| **failCriterion** | 业务域**不可配**（配置项只覆盖 LLM 域） |
| **failImpact** | `out-c`（按设计 #1：C 记「部分通过」，C 不判死、仍做，但 P12 不算达成标题目标） |
| **verdict** | **pass（WorkBuddy） / fail（Trae）** —— 见下 |
| **evidence** | 见「H0 证据」 |

#### H0 证据（逐条可复核）

**WorkBuddy —— 业务域 `endpoint` 可覆盖**（⇒ C 路对 WB **可能**覆盖业务请求）：

1. `resources/app.asar.unpacked/cli/product.json`（**shipped**）明确声明了业务域与**覆盖通道**：
   ```jsonc
   "endpoint": "https://www.workbuddy.cn",          // ← 业务/账号域基址
   "stagingEndpoint": "https://staging.workbuddy.cn",
   "officialEndpoints": ["https://www.workbuddy.cn", "https://copilot.tencent.com", ...],
   "productConfigPathEnv": "ACC_PRODUCT_CONFIG_PATH",              // ← 用「配置文件路径」覆盖
   "productConfigEnv": ["ACC_PRODUCT_CONFIG_V3", "ACC_PRODUCT_CONFIG_V2", "ACC_PRODUCT_CONFIG"]  // ← 用「内联 JSON」覆盖
   ```
2. `resources/app.asar`（**桌面主进程**）里存在读取逻辑（非仅 CLI）：
   - `resolveNetworkEnvironmentFromProductConfigEnv(config)` —— 读 `config.productConfigEnv` 里的键，逐键取 `process.env[key]` 并 `JSON.parse`；
   - `resolveAgentCliProductEnv({...})` —— `resolvedProductConfigPathEnv = productConfigPathEnv ?? process.env["ACC_PRODUCT_CONFIG_PATH"] ?? ...`，并处理 `process.env.ACC_PRODUCT_CONFIG_V3`；
   - `MUTUALLY_EXCLUSIVE_ENV_GROUPS = [["ACC_PRODUCT_CONFIG_PATH", "ACC_PRODUCT_CONFIG_V3"]]`。
3. CLI 侧请求基址解析（`resources/app.asar.unpacked/cli/dist/codebuddy-headless.js`）：
   ```
   es = ei.baseURL || ea.endpoint || this.productManager.getEndpoint() || void 0
   ```
   ⇒ 请求基址 = `请求自带 baseURL` || `product config.endpoint` || `productManager.getEndpoint()`；
   而 `endpoint` 来自**可被 `ACC_PRODUCT_CONFIG*` 覆盖的 product config**。
4. 本仓逆向成果印证业务域就是这几个：`crates/buddy-switch-core/src/modules/config.rs:15`
   `WORKBUDDY_API_ENDPOINT = "https://www.codebuddy.cn"`；`credits.rs:20` `WORKBUDDY_WEB_ENDPOINT = "https://www.workbuddy.cn"`；
   `region.rs:181-182` `chat_base = "https://copilot.tencent.com"` / `billing_base = "https://www.codebuddy.cn"`。

**Trae —— 业务域硬编码，**无**用户可配项**：

1. `resources/app/product.json` 的 `bootConfig` 把业务域**逐条硬编码**（按区域键 `normal`）：
   ```
   iCube.normal        = "https://api.trae.com.cn"
   account.trae.normal = "https://api.trae.cn"
   agent.trae.normal   = "https://trae-api-cn.mchost.guru"
   ug.trae.normal      = "https://api.trae.cn"
   remote.trae.normal  = "https://trae-api-cn.mchost.guru"
   frontier.normal     = "wss://frontier.zijieapi.com/ws/v2"
   ```
2. 用户设置里**只有网络代理与证书**，**没有**业务域基址项（`out/main.js` / `out/nls.keys.json` 全量列出）：
   `trae.network.proxy.mode` / `.effectiveMode` / `.manual.proxy` / `.manual.noProxy`；
   `trae.network.certificate.builtin` / `.system`；`trae.preview.networkUtils.*`（仅预览域自定义头）。
3. `%APPDATA%\Trae CN\User\settings.json`（本机实际内容）：
   ```json
   { "trae.network.proxy.effectiveMode": "system", "AI.agent.v2.browserChromeEnabled": "auto" }
   ```
   `%USERPROFILE%\.trae\argv.json` 是 VSCode 永久启动参数（仅少数受支持项，无业务域项）。

#### ★ H0 的「未做」部分（**必须用户手动**）

设计 H0 的「**跑**：改配置指向本地 mock server；**看**：业务请求是否真的打到 mock」——**本次未做**（不在工程师可自动化范围，且需启动真实客户端）。
⇒ 结论强度：**「配置层证据充分」**，但「**改道是否真的生效**」**待运行时确认**。

---

### H1 —— 客户端会**主动走 HTTP 代理**吗（A 路前半）

| 项 | 内容 |
|:---|:---|
| **statement** | 客户端会主动把请求交给 HTTP 代理（出现目标业务域 CONNECT） |
| **how** | 起只记录 CONNECT 的最小代理（不装 CA）；把客户端指过去；看日志是否出现目标域 CONNECT |
| **pass** | 看到目标域 CONNECT |
| **failCriterion** | 看不到目标域 CONNECT ⇒ **A 出局** |
| **failImpact** | `out-a` |
| **verdict** | **pending**（**工具侧已证**；**真客户端侧未验，需手动**） |
| **evidence** | `scripts/spike/p12-proxy-log.cjs` + 下方日志 |

**工具侧（已自动，证据可复现）**：

- 离线自检：`node scripts/spike/p12-proxy-log.cjs --selftest` ⇒ `[selftest] PASS —— 代理会记录 CONNECT，且隧道盲转发通路正常。`
- 真实域 CONNECT 演示（本机实测，经代理 `curl -x http://127.0.0.1:18899 <业务域>`）：
  ```
  [..] CONNECT www.workbuddy.cn:443
  [..] CONNECT trae-api-cn.mchost.guru:443
  [..] CONNECT copilot.tencent.com:443
  ```
  ⇒ **代理确实会把目标业务域记成 CONNECT**（代理侧行为已被证明）。

**真客户端侧（★ 未验，需手动）**：尚未把**真 WorkBuddy / Trae 客户端**指向该代理。
⇒ H1 的**最终判定取决于「真客户端是否主动走代理」**，该步见 `scripts/spike/README.md` §2.3。

> 说明：Trae 有 `trae.network.proxy.mode = manual`（可配 HTTP 代理）；WorkBuddy 是 Chromium，
> 可走系统代理或 `--proxy-server`。**两者都「理论上可指向代理」**，但**是否真的把业务请求交给代理**仍需实测。

---

### H1 补充核实（2026-10-07 · **静态代码分析**，未跑真客户端）

> 本轮**没跑真实客户端**，而是**读了 WB 主进程代码**（`app.asar`）——
> **结论足以判定 H1 的方向**，但"配置生效后真的走代理"仍需实测。

**证据**（`app.asar` 里搜到的原文片段）：

```js
// ① manual 模式：显式代理（含本地直连规则）
session.setProxy({ proxyRules: normalizeElectronProxyRules(r),
                   proxyBypassRules: "localhost,127.0.0.1,::1" })
// ② 三种模式
e.mode === "manual" && e.url ? setProxy({ proxyRules }) :
e.mode === "none"            ? setProxy({ mode: "direct" }) :
                               setProxy({ mode: "system" })
// ③ 读 env
{ HTTP_PROXY: process.env.HTTP_PROXY, HTTPS_PROXY: process.env.HTTPS_PROXY, NO_PROXY: ... }
// ④ 启动时应用（**同时含 TLS**）
this.persistedProxyMode = applyProxyAndTlsBootstrap({ ... })
```

**⇒ 结论（H1 修正为 ✅ 通过 —— 静态证据）**：

1. **WB 支持显式代理**（`manual` 模式 + `proxyRules`）⇒ 「客户端会走 HTTP 代理」**成立** ✓
2. **它读 `HTTPS_PROXY` env** ⇒ 有 **env 启动** 与 **持久化配置** 两条路 ✓
3. **★ `proxyBypassRules` 含 `127.0.0.1`** ⇒ **本地网关直连、不走代理** ⇒ **不会死循环** ✓
4. **存在 `proxySettings` / `proxyMode` / `proxyUrl` / `proxyDetection` / `proxyConflict` 等键**
   ⇒ **有完整的代理设置面 + 冲突检测** ✓
5. **★ `applyProxyAndTlsBootstrap`** ⇒ **启动时同时处理 TLS** ⇒ **H2 有戏**（**待实测**）

**⚠️ 仍未验**：**配置生效后是否真的走代理**（需真客户端 + 点一次业务操作）。
**⚠️ 另注**：本轮实测「WB 不跟随**系统代理**」（改系统代理 12 秒无流量）——
与本结论**不矛盾**：它有**自己的**代理配置面，**不依赖系统代理**。

---

### ★ H1/H2 的**追加发现**（2026-10-07 · 自动化尝试后）

**尝试过程**：装 CA（`-user Root`，**确认不需管理员**）→ 起中间人 → 改代理 → 重启 WB。
**结果**：**WB 零流量**（两次尝试）。但**过程本身产出了三条关键信息**：

| # | 发现 | 证据 | 对实现的意义 |
|:--|:---|:---|:---|
| **1** | **WB 的代理配置在 `~/.workbuddy/settings.json` 的 `http.proxy`** | 代码里的 IPC：`proxySettings:getSettings` / `saveSettings` / `testConnection`；且读 `e["http.proxy"]`（**VSCode 风格**） | **T02 可以「直接写这个键」**（备份 + 还原），**不必只做「引导」** |
| **2** | **它还有 `http.proxySupport`** | 同段代码：`typeof e["http.proxySupport"] === "s…"` | 可能**要同时设**该键才生效（**未验证**） |
| **3** | **改系统代理 / 注入 `HTTPS_PROXY` env 都无效** | 两次实测：改系统代理 12s 零流量；`HTTPS_PROXY` + 重启 WB 12s 零流量 | ⇒ **它只认自己的 `settings.json`** —— env 那段代码是「**读出来给 UI 显示**」，**不是「用它设代理」** |

**⚠️ 但 H2 本身仍未验**：**WB 启动后不发业务请求** ⇒ 中间人零流量 ⇒ **无法观察「是否报证书错误」**。
**⇒ H2 需要一个「能触发业务请求的操作」**（打开某页面 / 点签到）—— **这是人工环节，无法自动化**。

**⚠️ 且 `http.proxy` 是否真的生效也未验**（注入了但没观察到流量；可能「要配合 `http.proxySupport`」，也可能「WB 启动即无请求」）。
**⇒ 不基于推测改设计**：T02 **保持引导式**，但**引导内容改为「在客户端设置里填 `http.proxy`」**（**比「设 env」更贴近事实**）。

**⇒ 全部改动已还原**（`settings.json` 逐字节还原、CA 已卸、中间人已停、WB 已重启）。
**⚠️ 过程中我曾误改「系统代理」**（不知用户本就有 `127.0.0.1:7897`）—— **已立即还原**，并记入教训。

---

### H2 —— 客户端信任自签 CA、不报证书错误（★ 真正的 go/no-go）

| 项 | 内容 |
|:---|:---|
| **statement** | 装自签 CA 后，客户端做 TLS 中间人不报证书错误（= 无证书固定） |
| **how** | **手动装**自签 CA（D32）→ mitmproxy/自研中间人 → 重放 H1 |
| **pass** | **不报证书错误**且业务正常 |
| **failCriterion** | **报证书错误**（= 证书固定）⇒ **A 出局**（硬阻塞） |
| **failImpact** | `out-a` |
| **verdict** | **pending —— ⚠️ 需用户手动执行** |
| **evidence** | 工具：`scripts/spike/p12-mitm-addon.py`；步骤：`scripts/spike/README.md` §3 |

**为什么工程师单独做不到**：装 CA 到系统信任库（Windows「受信任的根证书颁发机构」/ macOS 系统钥匙串）是**系统级改动、需管理员权限**；设计决策 #3 明确**应用不得自动提权**，须用户手动操作。

**与 H2 相关的离线线索**（仅供手动步骤参考，**不构成 H2 判定**）：
- Trae 有证书库选择项 `trae.network.certificate.system` / `.builtin`（`out/nls.keys.json`）——**Trae 可选「系统证书库」**，意味着它**可能**信任系统装的 CA（但**不等于**无证书固定，仍须实测）。
- 本仓 `crates/buddy-switch-core/src/modules/trae/icube.rs:382` 附近涉及 `device_proof` 的 PKCS#8 私钥（P-256），与 H3 相关。

---

### H3 —— 凭据可**按请求替换**

| 项 | 内容 |
|:---|:---|
| **statement** | 拦一个真实业务请求，替换凭据后上游仍接受（200） |
| **how** | 中间人拦真实请求：WB 换 `Authorization: Bearer <token>`；Trae 换 token 后**重做 `device_proof`** 签名 |
| **pass** | 上游**接受** |
| **failCriterion** | 上游仍**拒绝** ⇒ 该平台剔除（`platform-drop`） |
| **failImpact** | `platform-drop`（WB 可换、Trae 不可换 = **可接受结果**） |
| **verdict** | **pending —— ⚠️ 需用户手动执行（需真实业务流量）** |
| **evidence** | 工具：`scripts/spike/p12-mitm-addon.py`（`P12_MITM_REPLACE_AUTH`）；步骤：`README.md` §4 |

**为什么工程师单独做不到**：mock 只能验转发层，**验不了上游是否接受换号后的凭据** ⇒ 必须跑真实客户端产生真实业务流量（签到/积分/旅行）。

**★ Trae 侧状态 = 「需额外打通」（非「已实现」）**（设计决策 #6）：
Trae 换号需重做 `device_proof`（P-256 ECDSA / P1363，`crates/buddy-switch-core/src/modules/trae/icube.rs:844`），
而该私钥（`icube.rs:382` PKCS#8）注释说**只为 refresh 路径服务** ⇒ **代理换号可能拿不到私钥** ⇒
**Trae 的凭据替换本 spike 工具做不到**。**WB 可换、Trae 不可换是合格结果**。

---

### H4 —— 转发层**不改写 header 原始大小写**

| 项 | 内容 |
|:---|:---|
| **statement** | 转发一个带 `anthropic-version` 的请求，上游收到的 header 大小写与原始一致 |
| **how** | **读已抓样本**（header 大小写） |
| **pass** | 大小写**保持** |
| **failCriterion** | 大小写被改写（`Anthropic-Version`）⇒ **不作废 P12**，只决定实现选型 |
| **failImpact** | `no-abort` |
| **verdict** | **pending**（**本仓无已抓样本** ⇒ 只能等 H2 阶段顺带抓） |
| **evidence** | `p12-mitm-addon.py` 记录 `client_header_names(raw)`；本文件 §4「看起来能做但其实做不了」第 4 条 |

**离线观察（不构成判定）**：
- 本仓**没有任何已抓的业务请求样本**（`find` 全仓 `*.har` / `*capture*` / `*.flow` 无命中）。
  ⇒ 「**读已抓样本**离线判定 H4」这一步**当前无样本可读**。
- Trae 走 **HTTP/2**（`%APPDATA%\Trae CN\ahanet\server.json` 的 `ttnet_h2_enabled: 1`，且 `hpack_optimization_*` 配置齐全）。
  **HTTP/2 / HPACK 强制 header 名小写** ⇒ 在 h2 路径上「客户端发出的原始大小写」**本就被协议抹平**，
  H4「保持大小写」的语义在 h2 上**部分不成立**（转发层只需**不主动改写**；但 h2↔h1 转换本身会小写）。

---

### H5 —— 业务请求可**按方法+路径分类读 / 写**

| 项 | 内容 |
|:---|:---|
| **statement** | 业务请求可按「方法 + 路径」可靠区分读 / 写 |
| **how** | **读**抓到的业务请求样本，列方法+路径白名单 |
| **pass** | 能**可靠区分**读写 |
| **failCriterion** | 无法区分 ⇒ **不作废**，但**换号重试整体禁用**（fail-closed） |
| **failImpact** | `no-abort`（#5：宁可少一个功能，不可写错） |
| **verdict** | **pass（离线，依据本仓逆向成果）** |
| **evidence** | `crates/buddy-switch-core/src/modules/cat.rs` / `credits.rs` / `official_usage.rs` |

**离线证据**（读 = `GET`，写 = `POST`，可按方法+路径区分）：

| 业务动作 | 方法 | 路径 | 来源 |
|:---|:---|:---|:---|
| 积分/用量查询（读） | `GET` | `/billing/meter/get-user-resource-summary`、`/billing/meter/get-user-request-usage` | `credits.rs:1187` / `official_usage.rs:28` |
| 任务列表（读） | `GET` | `/v2/activity/growth/tasks` | `cat.rs:29` |
| 领任务（写） | `POST` | `/v2/activity/growth/tasks/accept` | `cat.rs:30` |
| 领取奖励（写） | `POST` | `/activity/growth/tasks/black_cat/claim` | `cat.rs:31`（测试断言 `method == "POST"`，`cat.rs:471`） |

⇒ **读写可可靠区分**（读=`GET`、写=`POST`），且**写操作集中在 `/activity/growth/.../accept|claim` 等少数路径**。
⇒ H5 通过 ⇒ **换号重试不必整体禁用**，但仍按设计纪律：`is_retryable` **默认 false**、**只读可重试、写操作不换号、上限 1**。

---

## 3. 总判定（outcome）

| 假设 | 判定 | 含义 |
|:---|:---|:---|
| **H0** | **pass（WorkBuddy）/ fail（Trae）** | C 可用；WB 业务域**有**覆盖通道，Trae 业务域不可配（C 只覆盖 LLM） |
| **H1** | **pass（静态代码分析）** | WB 有完整代理设置面（`http.proxy` 等）⇒「客户端支持配代理」成立 |
| **H2** | **★ 不再适用（2026-10-07 路线变更）** | 见下 —— **改走「本地路由」后不需要 CA，证书固定问题不存在** |
| **H3** | **降级为参考项** | 新路线靠「改 base URL + 网关持有凭据」，**不是「替换客户端请求里的凭据」** |
| **H4** | **pending**（无样本） | 不作废，只影响选型 |
| **H5** | **pass（离线）** | 不作废，换号重试仍可做（fail-closed） |

**outcome = `C` + `本地路由`**（D34 的 C 先行；**A 路改走本地路由**）

### ★★ 2026-10-07：A 路**改道**（原「TLS 中间人」→「本地路由」）

**参照 cc-switch**（成熟产品）：它的「本地路由」让客户端配置里**只写 `http://127.0.0.1:15721`**，
真正的 provider 存在它自己库里，**本地做转发 + 协议转换** ⇒ **全程明文 http，不需要 CA**。

**⇒ 本仓照此改道**：

| 原路线 | 新路线 |
|:---|:---|
| 客户端配 **HTTP 代理** + **TLS 中间人** | 客户端 **base URL** 指向本地网关（**明文 http**） |
| **要装 CA** ⇒ **被 H2（证书固定）阻塞** | **不需要 CA** ⇒ **H2 不存在** |
| 代理层要解 TLS | **不解 TLS**（明文进、按路径转发） |

**⇒ 三步里前两步已有**：① C 路（改 base URL，**已做**）② `relay.rs`（LLM 转发，**已有**）
⇒ **只缺 ③ 业务请求转发**（T03）。

**⇒ H2 的结论**：**不再是 blocker** —— 它是「中间人路线」特有的问题，**改道后不适用**。
（**H2 的实测仍可作为「若将来要走中间人」的参考**，但**本期不需要**。）

**⇒ H3 也降级**：新路线下「换号」靠「**改 base URL + 网关持有凭据**」，
**不是「替换客户端请求里的凭据」** ⇒ 原 H3 的问题（Trae 的 `device_proof` 签名）**在新路线下不出现**。

---

## 4. ★「看起来能做，但其实做不了」清单

> 逐条列出**容易被误当成「能做」的地方**，避免把不确定说成可行。

1. **`ACC_PRODUCT_CONFIG*` 是「启动期环境变量」，不是用户设置文件。**
   ⇒ buddy-switch 需要在**启动 WorkBuddy 时注入 env**；若客户端已由托盘常驻 / 已被启动，
   注入时机与生效范围**需另验**。**「读到一个 env 名」≠「改一处配置就生效」。**
2. **WorkBuddy 只保证 `endpoint` 这一个 base 可覆盖，不能保证「全部业务流量改道」。**
   请求基址解析是 `request.baseURL || config.endpoint || productManager.getEndpoint()` ⇒
   **自带 `baseURL` 的请求**、以及走**别的硬编码域**的请求（如 chat → `copilot.tencent.com`）
   **未必**被 `endpoint` 覆盖。**「业务域可配」≠「所有业务请求都过网关」。**
3. **Trae 业务域硬编码在 `product.json`（shipped），无用户可配项** ⇒
   C 对 Trae **只覆盖 LLM**；**不要把「Trae 能配代理」误当成「Trae 业务域可配」**。
4. **H4 在 Trae 的 HTTP/2 路径上验不出「原始大小写」。**
   TTNet 用 h2（`ttnet_h2_enabled: 1`），HPACK **强制小写** ⇒ 「客户端发出的原始大小写」
   本就被协议抹平。**「能记录 header 大小写」在 h2 上语义不成立** —— 别据此断言「转发层会/不会改写」。
5. **H3 对 Trae 的 `device_proof` 做不到。**
   换号需重做 P-256 签名，而私钥仅 refresh 路径持有（`icube.rs:382`）⇒
   **spike 工具（及将来的代理）可能拿不到私钥** ⇒ Trae 侧**必须**记「需额外打通」，**不许含糊成「已实现」**。
6. **H4 的「读已抓样本」当前无样本可读**（本仓无任何业务请求抓包）⇒ 只能等 H2 阶段顺带抓；
   **「H4 能离线判定」这个前提当前不成立**。

---

## 5. 证据索引（可复核）

| 证据 | 位置 |
|:---|:---|
| H1 最小代理（只记录 CONNECT）+ 自检 | `scripts/spike/p12-proxy-log.cjs`（`--selftest`） |
| H2/H3/H4 中间人 addon | `scripts/spike/p12-mitm-addon.py` |
| 操作 + **每步还原** | `scripts/spike/README.md` |
| H0：WorkBuddy 业务域可覆盖 | `D:\Program Files\WorkBuddy\resources\app.asar.unpacked\cli\product.json`（`endpoint` / `productConfigPathEnv` / `productConfigEnv`）；`resources\app.asar`（`resolveNetworkEnvironmentFromProductConfigEnv` / `resolveAgentCliProductEnv`） |
| H0：Trae 业务域硬编码 | `D:\Programs\Trae CN\resources\app\product.json`（`bootConfig`）；`out\main.js`（`trae.network.*`） |
| H0/H5：本仓业务域与读写方法 | `crates/buddy-switch-core/src/modules/{config,region,credits,official_usage,cat}.rs` |
| H3：Trae `device_proof` 私钥约束 | `crates/buddy-switch-core/src/modules/trae/icube.rs:382,844` |

---

## 6. 下一步（谁来做）

| 步骤 | 负责 | 产出 |
|:---|:---|:---|
| **H1 真客户端半段**（把真客户端指向代理） | **用户手动** | 代理日志（是否出现目标业务域 CONNECT） |
| **H2**（手动装 CA + 中间人重放） | **用户手动**（需管理员权限） | `p12-mitm.log`（是否报证书错误） |
| **H3**（真实业务流量换号） | **用户手动** | 上游状态码（是否 200） |
| **H0 运行时确认**（改 `endpoint` 指向本地 mock，看业务请求是否真打到 mock） | **用户手动** | mock 侧请求日志 |
| **H4**（H2 阶段顺带抓 header 大小写） | 用户手动 + 工具 | `client_header_names(raw)` |
| 把本结论**写回 `PLAN.md` §3.19** | 后续 | 通过写「用哪条路+证据」、失败写「卡在哪一步+现象」 |
