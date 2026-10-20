# Qoder 平台 — 事实梳理与建模决策

> 用途：把「Qoder 作为第三个平台接进 Buddy Switch」需要的事实与决策固定下来，
> 作为原型评审与后续实现改造的依据。
> 状态：**原型已落地，项目代码未动**（等确认）。
> 边界：本文只写**事实**与**本仓的决策**；不写任何外部参考实现的出处。

---

## 1. 这个平台是什么

阿里出品的 AI 编程 IDE，产品名 **Qoder**，走**双区域**：

| | 国内版 | 国际版 |
|:---|:---|:---|
| 官网 | `qoder.com.cn` | `qoder.com` |
| 开放接口基址 | `https://openapi.qoder.com.cn` | `https://openapi.qoder.sh` |
| 推理网关 | `https://gateway.qoder.com.cn` | `https://api1.qoder.sh`（另有 api2 / api3 故障切换） |
| OAuth 客户端 id | `1c5e33e1-…-acaa81274a5c` | `e883ade2-…-f92ceff5fdcb` |
| 回调 | `qoder-work-cn://` | `qoder://aicoding.aicoding-agent/login-success` |
| CLI 本地目录 | `~/.qoder-cn/` | `~/.qoder/` |
| 桌面 App 目录 | `%APPDATA%\com.qodercn.app.stable\` | `%APPDATA%\com.qoder.app.stable\` |

**结论：双区域是「一个平台内的两个 realm」，不是两个平台。**
⇒ 在本仓建模为**一个 `Product` + 两个 `Scope`**（`qoder-cn` / `qoder-global`），
与 WorkBuddy / Trae 的区域处理方式一致。

---

## 2. 账号模型

一个 Qoder 账号的关键字段（用于推导卡片与抽屉要展示什么）：

| 字段 | 含义 | 备注 |
|:---|:---|:---|
| `uid` | 账号唯一 id | 由上游 `user_id` 给出；无 uid 时按来源生成兜底 id |
| `nickname` | 昵称 | |
| `domain` / `realm` | 归属域名 / 区域 | `cn` 或 `intl` |
| `accessToken` | 访问令牌 | 前缀 `dt-`（设备流）或 `jt-`（jobToken） |
| `refreshToken` | 刷新令牌 | 前缀 `drt-` / `jrt-`，按前缀决定走哪个刷新端点 |
| `personalToken` | PAT（长期兜底） | 前缀 **`pt-`** |
| `expiresAt` | 过期时刻 | |
| `source` | 来源 | `oauth` / `pat` / `desktop-app`（本机导入） |
| `credits` / `plan` | 额度快照 / 套餐名 | |
| `lastCheckin` | 最近签到 | |
| `userType` | 用户类型 | 默认 `personal_professional_trial` |
| `organizationId` / `organizationName` | 组织 | |

**令牌家族（前缀语义）**：`pt-`（PAT，长期）→ 交换得到 `jt-`/`jrt-`；
设备流登录得到 `dt-`/`drt-`。刷新按前缀路由：`drt-` 走 deviceToken 刷新、
`jrt-` 走 jobToken 刷新、都失败则回落 PAT。
⇒ 与本仓既有「凭据类」处理同构：**刷新看 `refresh_token`，可用性看 `access_token`**。

---

## 3. 凭据存储形态（官方两处）

| 来源 | 路径 | 加密 |
|:---|:---|:---|
| **桌面 App** | `%APPDATA%\com.qoder[.cn].app.stable\auth.v1.dat` | Chromium `v10` 布局 + **AES-256-GCM**；密钥来自同目录 `Local State` 的 `os_crypt.encrypted_key`，剥 `DPAPI` 前缀后经 **DPAPI** 解出 32 字节 AES key |
| **Qoder CLI** | `~/.qoder[.cn]/.auth/user[.{profile}]` | **AES-128-CBC**，`key = iv = machine_id 前 16 字符`；兼容以 `{` 开头的明文形态 |

桌面 App 的明文 schema：
`{ schemaVersion, token(dt-), refreshToken(drt-), expiresAt, user:{ id, name, email } }`。

⚠️ **两类都要支持，且要分开报「未检测到」与「读不出」**（同本仓既有的空态二分纪律）。

---

## 4. 能力清单

| 能力 | 有 | 说明 |
|:---|:---:|:---|
| 每日签到领积分 | ⚠️ | **通道未开放**：本仓走的是 legacy `/sash/api/v1/me/daily-check-in/*`，上游已整体关停（claim 恒 409 且不携带幂等语义、status 恒 `DISABLED` + `totalClaimDays=0`）。界面如实报「未开放」并说明要开桌面端网页活动页；**工具内代领未实现**，见 issue #58 |
| 积分 / 额度查询 | ✅ | quota/usage + 套餐快照 |
| Pro 福利包 | ❌ | **未实现**：`PATH_PRO_CLAIM` 只有常量定义、零调用点（issue #58 第 1 阶段一并落地） |
| OAuth 设备授权 | ✅ | **PKCE S256**；两区 URL 参数不同（国内带 `redirect_uri`，国际不带） |
| PAT 导入 | ✅ | 前缀 `pt-`，导入后自动交换 jobToken |
| API 网关 | ✅ | OpenAI 兼容（Chat Completions / Responses）+ **Anthropic 兼容**（`/v1/messages`） |
| 定时调度 | ❌ | **无 Qoder 排程任务**：`ScheduleTask` 枚举里没有 Qoder 变体，`scheduler` 也无对应分支 ⇒ 签到与令牌保活**都不会自动执行**（issue #58 讨论是否补上） |
| 会话复制 | ❌ | **不实现** —— 会话复制依赖客户端自身的会话存储与云端归属，当前只有 WorkBuddy 桌面具备 |
| 自动旅行 | ❌ | WorkBuddy 独有 |

⇒ 原型里的 `capabilities` = `["checkin", "credits", "oauth", "gateway"]`。

---

## 5. 模型清单（三层优先级）

1. **动态接口**（官方此刻为准）> 2. **本机官方客户端目录缓存** > 3. **双区官方快照文件**。

- 清单**以动态源返回的集合为准**：静态快照独有的条目**不外塞**。
- 官方 `enable = false` 的条目**不过滤**，但要附**官方原文的禁用原因**。
- 模型条目可用的属性：官方模型名（客户端唯一需要填的值）、上游 key、别名、
  本地化名、上下文多窗口（如 200K 默认 / 400K / 1M）、思考档位
  （low/medium/high/xhigh/max + 默认标注 + 可关闭）、**峰谷价**
  （谷时时段 22:00–08:00）、`is_free` / `is_new`。

**双区独占**（区域独占模型必须路由到归属出口，错配要拦）：

| | 独占 |
|:---|:---|
| 国内版 | `q37fmodel`、`gm51model` 一类 |
| 国际版 | `ultimate`、`performance`、`efficient`、`smodel`、`cmodel` 一类 |

⇒ 与本仓「**授权页两轴**（域按区域 / 钥匙按产品线）」同源：Key 出口与模型区域
不匹配时必须给**通俗 400**，不能把上游的晦涩拒流原样透出去。

---

## 6. 设备指纹

以账号自身 uid **稳定派生**专属机器标识（machineId / machineToken / machineType /
sessionId）。目的：同一账号长期固定在同一台「虚拟物理设备」上，**多账号之间天然隔离**，
避免被按机器身份关联风控。

⚠️ 派生值是**兜底**：官方桌面端拉活动前会调自带风控桥取**真机器身份**；
用派生假值时活动列表会**静默少条目**（设备定向活动直接不出现）。
⇒ 实现时「真值优先、派生兜底」并**在状态里标注用了哪种**，不要静默降级。

---

## 7. 网关与 API Key

- 多 API Key 并行；**每个 Key 可绑定独立出口**（国内版 / 国际版 / 跟随全局）。
- 面板密码与 API Key **完全独立**（密码以 PBKDF2-SHA256 摘要保存，不存明文）。
- Key 出口与模型区域不匹配 ⇒ 通俗 400。

⇒ 与本仓既有网关三态动作（吊销 / 恢复 / 删除）**同形**，可直接复用既有 UI 与命令。

### 7.1 推理端点必须带 `?Encode=1`（真机取证 2026-10-17）

`agent_chat_generation` 的请求体是 `cosy::qoder_encode` 过的**自定义 base64**；上游只按
URL query `Encode=1` 决定「先解码再按 JSON 解析」还是「直接按 JSON 解析」。

带编码体打**裸路径**时上游**不发 4xx**：它先回 `HTTP 200`，再在流里补三行**非信封错误帧**
（`event:error` / Jackson `JsonParseException` 堆栈 / `{"success":false,"msgCode":500}`），
客户端侧表现为「空回答 + 0 token」，网关日志表现为「200 / 0 token / error:null」。

⇒ 两处纪律（issue #46 的修复即此二条）：

1. **签名路径与发送路径分开**：`PATH_CHAT_SSE`（签名，无 query）/
   `PATH_CHAT_SSE_ENCODED`（发送，带 `Encode=1`），与 `PATH_MODELS_SIGN` /
   `PATH_MODELS_ENCODED` 同一约定；
2. **非信封错误帧不得 Skip**：`api::unwrap_non_envelope_error` 按保守白名单判成
   `ChatFrame::UpstreamError`（只认 `event:error` / `success:false` / 非 0 且非 2xx 的
   `code`·`msgCode` / 非空 `error` / Java 异常转储），正常 delta 与 `event:finish`
   一类计时元数据仍 Skip。

### 7.2 流式链上的**泄漏回声守卫**（issue #46 的 A 项，2026-10-18）

文本模式下历史里的工具调用会被序列化成 `<LEAK_MARKER>` + JSON 数组塞进正文，模型很可能
**原样回声**。非流式 `sse::aggregate` 末尾用 `api::parse_leaked_tool_calls` 还原，而
**流式中继是逐帧透传的** ⇒ 泄漏文本直接打到客户端界面。

⇒ 三条纪律（`sse::LeakGuardStream`）：

1. **接入点固定在 `QoderOpenAiStream` 之内、`UsageTapStream` 之外**：
   - 不改 `QoderOpenAiStream::new` 的签名（8 处调用点零改动）；
   - `aggregate` 内部自建流、**不经过**本层 ⇒ 结构上不可能被还原两次
     （两次会各生成一组随机 id 的 `tool_calls`，客户端会把同一组工具**执行两遍**）。
2. **判错一律 fail-open**：标记写一半 / 超过 64 KiB / 不是合法回声 ⇒ 原样透传。
   唯一例外是**剥正文**：`Recovering` 且上游已给结构化 `tool_calls` 时**不合成第二组**，
   但**仍必须剥掉回声正文** —— 「不双发」与「不泄漏」是两件事，合成不了就整帧放行
   等于把标记原样还给用户。
3. **合成形状对齐 Anthropic 侧读法**（*实现时读 `anthropic::push_tool_call` 发现的协议约束，非评审要求*）：`[anthropic::push_tool_call]` 读
   `delta.tool_calls[i].index` 且 `unwrap_or(0)` ⇒ 合成帧必须逐个带 `index`；
   `finish_reason` 必须落在上游 usage 帧**之前**，否则 Anthropic 侧补不出 usage。

> **评审指引（归属披露）**：`#46` 的 A/B/C/D 四项修复分处
> `5f6c52a`（A 守卫）/ `7fca77d`（B 白名单）/ `803c727`（C 用量页）/
> `de932d7`（D 签到后端与共用组件）。⚠️ **D 的页面层主体**
> （`runCheckin` 的三档分流 + `skippedNames` 透传，改 `QoderAccountsPage.tsx`）
> 与两条 i18n 词条落在并行会话的 `cdb54b2` —— 评审 D 时请连同该 commit 一起看，
> 不要只翻 `de932d7`。

---

## 8. 在本仓的建模决策

| 维度 | 决策 | 理由 |
|:---|:---|:---|
| 平台数 | **+1 个 `Product`**（`qoder`），不是两个 | 双区域是 realm，不是产品 |
| 作用域 | `qoder-cn` / `qoder-global` | 与 `wb-*` / `trae-*` 同构 |
| 程序位 | **2 个**：`Qoder 桌面版` / `Qoder CLI` | 对应官方**两处**凭据存储；程序位名**不含区域**（区域由作用域承担） |
| 能力 | `checkin` / `credits` / `oauth` / `gateway` | 无 sessionCopy / travel / autoRotate / snapshot / group |
| 图标 | 圆环 + 内环 + 右下短尾（几何「Q」） | 与 WB 菱形、Trae 三角同族但可区分 |
| 强调色 | `oklch(0.68 0.15 245)`（蓝） | 与 WB 绿、Trae 紫区分 |

### 待拍板 / 待核实

1. **CN 客户端的展示名**。官方国内版 UA 是 `QoderWork/1.1.64`、国际版是 `Qoder/1.1.64`；
   桌面 App 目录是 `com.qodercn.app.stable`。原型暂按既有惯例写作
   **`Qoder CN` / `Qoder`**（与 `Trae CN` / `Trae`、`WorkBuddy` / `WorkBuddy AI` 同构）。
   若确认国内版客户端正式名是 `QoderWork`，改名只动 `UNI_NS` 一处。
2. **程序位命名**：原型用 `Qoder 桌面版` / `Qoder CLI`。若官方另有正式名（如
   「Qoder IDE」），按官方名改。
3. **签到奖励数值**：按「每日领取 100 Credits」取 `checkinReward = 100`。
4. **是否要做「峰谷价 / 思考档位 / 多上下文窗口」的展示**：原型**未做**（模型表只有
   名称 / 协议 / 归属 / 上游状态 / 网关暴露五列，三平台同构）。要做的话是**三平台一起加**，
   不能只给 Qoder 加 —— 否则就是「同一件事两个做法」。

---

## 9. 原型落点（本次改动）

**拆分形态**（`docs/prototype/index.html` + `demo.css` / `demo-data.js` / `demo-app.js`，门禁跑的就是它）

- `demo-data.js`：`MARK.qoder`、`PLATFORMS.qoder`、`SCOPES["qoder-cn"|"qoder-global"]`、
  `USAGE[...]`、`GATEWAY[...]`、`PROXY.domains` 加 `qoder.com.cn`。
- `demo-app.js`：全局快捷键 `⌘1–4` 放宽到 `⌘1–9`（6 个作用域后 `⌘5`/`⌘6` 原本静默失效）。

**单文件形态**（`docs/prototype/buddy-switch-prototype.html`，双击即看）

- 与拆分形态同一批数据改动。
- 额外：**统一入口**的命名空间表（`UNI_NS` / `UNI_CATALOG` / 默认启用集 /
  Key 授权集 / 报错文案 / 页面提示）补上 `qoder-cn` / `qoder-global`
  —— 它们属于**派生层**（平台 × 区域），真实实现里由 `Scope::all()` 自动生成。

**顺带修掉的既有缺陷**：备份页每个作用域行**都没有名字**
（`BACKUP_SECTIONS` 取 `SCOPES[id].label`，而 `SCOPES` 条目只有 `platform` + `region`
⇒ `esc(undefined)` 归成空串，`aria-label` 也退化成「全选 」）。
修法：把「平台名 + 区域名」的拼装下沉到数据层的 `scopeDisplayName`，
`BACKUP_SECTIONS` 与 `demo-app.js` 的 `scopeLabel` 共用它。

---

## 10. 门禁与验证

| 验证器 | 覆盖 | 结果 |
|:---|:---|:---|
| `verify-demo.cjs` | 拆分形态契约 | **37 / 0** |
| `verify-behaviors.cjs` | 拆分形态行为 | **124 / 0** |
| `verify-qoder-prototype.cjs`（新增） | **单文件形态**（此前无任何自动化覆盖） | **13 / 0** |

被钉死的断言里，原先硬编码「4 个作用域」的四处已同步为 6
（作用域菜单项数、备份分区行数、全不勾选导出、合并口径逐日求和、days 末值表）。
**断言一律写死数字，不拿 `Object.keys(SCOPES).length` 对拍** —— 后者与被测物同源，恒真。

变异验证（注入缺陷 ⇒ 期望变红 ⇒ 还原核 sha256）：

- 拆分形态 4 组：days 末值脱钩 / 备份分区退回硬编码 4 个 / 区域标签错位 / 备份页名字退回空串 —— **全部命中且还原无残留**。
- 单文件形态 3 组：区域标签错位 / 统一入口漏命名空间 / 备份页名字退回空串 —— **全部命中且还原无残留**。

---

## 11. 后续项目代码改造的清单（**等确认后再动**）

1. `crates/buddy-switch-core`：新增 Qoder 模块（账号池、令牌刷新按前缀路由、
   双区常量、签到/额度、模型清单三层、指纹派生）。
2. 新增 `Product::Qoder` + `Scope::QoderCn/QoderGlobal`；`Scope::all()` 自动多出两个作用域
   ⇒ 统一网关命名空间**自动出现**。
3. `src/lib/platforms/registry.ts`：加 `qoder` 描述符 + `PlatformSlug`；
   `src/lib/scope.ts` 的 `SCOPES` 从 4 条变 6 条
   ⇒ **`scripts/check-scope-registry.cjs` 里写死的「4 个作用域」会变红，必须同步**。
4. 网关：Qoder 产品线（配置 / 密钥库 / 状态 / 路由 / OpenAI + Anthropic 双协议）。
5. 两处宿主（`buddy-switch-server/main.rs` + `src-tauri/lib.rs`）的命令注册 +
   `BackgroundTask` 登记（**两侧各一份表，新增周期性出口先 grep 两侧**）。
6. 前端：程序位、凭据导入方式（OAuth / PAT / 本机导入）、模型清单展示。
7. **命名空间护栏**：解析只做整段精确匹配；测试输入必须含「比基准长一截」的形状
   （如 `qoder-cn-x`），否则前缀实现与精确实现在现有输入集上等价、断言恒绿。
