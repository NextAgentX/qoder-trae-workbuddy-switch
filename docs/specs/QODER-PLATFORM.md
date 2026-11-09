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

### 3.1 macOS 与 Windows 是**两套**密钥来源（issue #81，2026-09-23 真机取证）

| | Windows | macOS |
|:--|:--|:--|
| 桌面 userData 目录 | `%APPDATA%\com.qoder[.cn].app.stable\` | `~/Library/Application Support/com.qoder[.cn].app.stable\`（**叶子名与 Windows 逐字相同** ⇒ `dirs::config_dir()` 直接可用，无需按平台分叉） |
| 主密钥来源 | 同目录 `Local State` 的 `os_crypt.encrypted_key`（DPAPI 解出） | **登录钥匙串**：macOS 的 `Local State` 实测只有 `{"uninstall_metrics":…}`，**没有 `os_crypt`** |
| 钥匙串条目 | — | 国内版 `svce="Qoder CN App Safe Storage"` / `acct="Qoder CN App Key"`；国际版 `svce="Qoder Safe Storage"` / `acct="Qoder Key"`（`security dump-keychain` 实测） |
| blob 算法 | `v10` + 12B 随机 nonce + AES-256-GCM | `v10` + AES-128-CBC，key = `PBKDF2-HMAC-SHA1(口令, salt="saltysalt", iter=1003, len=16)`，**IV 固定 16×0x20 不写进文件**，PKCS7 |
| CLI 凭据 | `~/.qoder[.cn]/.auth/user`（加密或明文） | 同左；实测 `~/.qoder-cn/.auth/` **只有 `machine_id`、没有 `user`** ⇒ CN 的 CLI 不落盘凭据，登录态由桌面端注入 |

★ **服务名按区域不同**（国内版 Electron 应用名是 `Qoder CN App`，国际版是 `Qoder`）⇒
收进 `region::RealmSpec.macos_keychain_service` 表字段，读写两侧（`profile` 导入 /
`switch` 切换）同表取值。曾经两侧都硬编码国际版的 `"Qoder Safe Storage"`，国内版在
macOS 上永远查不到钥匙串条目 ⇒ 解密失败 ⇒ 「桌面端检测不到 / 切换不了账号」（issue #81）。

> 读钥匙串会触发系统授权弹窗（一次性，用户点「始终允许」后不再打扰）；
> `security` 调用必须带超时（本仓 `vscode_cn_inject` 已按 10s 上限执行）。

---

## 4. 能力清单

| 能力 | 有 | 说明 |
|:---|:---:|:---|
| 每日签到领积分 | ✅ | 走**活动平台**：`GET /sash/api/v1/me/campaigns` → 筛 `CLAIM_BENEFIT` + `CLAIMABLE` → 逐个 `POST …/{id}/claim`，累加 Credits；已领 ⇒ 幂等「今日已领」。旧 `/sash/daily-check-in` 只在**整区没有活动平台**（404/405/410）时兜底。三档语义不变：**没有积分类活动 ⇒ 未开放（`ok:false` + `disabled`）**，不谎报成功。★ **上游实例在 10:00（UTC+8）切换，窗口 10:00 → 次日 09:59**（2026-11-09 真机取证：`startAt`=10:00、`endAt`=次日 09:59）；活动列表**依赖 `cosy-machine*` 原生头**——不带 `runtime-info` 身份时上游直接返回空列表（`showCampaign:false`），带了才有「每天领 100 Credits」 |
| 积分 / 额度查询 | ✅ | quota/usage + 套餐快照 |
| Pro 福利包 | ❌ | **未实现**：`PATH_PRO_CLAIM` 只有常量定义、零调用点（issue #58 第 1 阶段一并落地） |
| OAuth 设备授权 | ✅ | **PKCE S256**；两区 URL 参数不同（国内带 `redirect_uri`，国际不带） |
| PAT 导入 | ✅ | 前缀 `pt-`，导入后自动交换 jobToken |
| API 网关 | ✅ | OpenAI 兼容（Chat Completions / Responses）+ **Anthropic 兼容**（`/v1/messages`） |
| 定时调度 | ✅ | `ScheduleTask::{QoderCheckin, QoderKeepalive}`：逐账号**顺序**执行（并发会踩风控）。判据与 Trae 两条同源 —— 签到默认**关**（替用户去领东西，opt-in），保活默认**开**（维持用户交出来的 `refresh_token`）；入口在 Qoder 设置页「计划任务」。★ 签到默认时点 **10:05 / 21:00**（不是整点 09:00：09:00 落在上一个实例窗口内，必然 `CLAIMED` ⇒ 领不到）；老配置仍排 09:00 时由**窗口感知补跑**兜住（`qoder::checkin::retry_after_ms` → 下一个 10:05 补跑一次，`scheduler::schedule_loop` 消费）。每轮写台账 `qoder/checkin_logs.<region>.json` + 一条 `[qoder-checkin]` 运行日志，设置页「计划任务」显示最近一次结果 |
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
   （2026-10-21 修正：**截断的回声**不再是 fail-open —— 见 §7.3。）
3. **合成形状对齐 Anthropic 侧读法**（*实现时读 `anthropic::push_tool_call` 发现的协议约束，非评审要求*）：`[anthropic::push_tool_call]` 读
   `delta.tool_calls[i].index` 且 `unwrap_or(0)` ⇒ 合成帧必须逐个带 `index`；
   `finish_reason` 必须落在上游 usage 帧**之前**，否则 Anthropic 侧补不出 usage。

> **评审指引（归属披露）**：`#46` 的 A/B/C/D 四项修复分处
> `5f6c52a`（A 守卫）/ `7fca77d`（B 白名单）/ `803c727`（C 用量页）/
> `de932d7`（D 签到后端与共用组件）。⚠️ **D 的页面层主体**
> （`runCheckin` 的三档分流 + `skippedNames` 透传，改 `QoderAccountsPage.tsx`）
> 与两条 i18n 词条落在并行会话的 `cdb54b2` —— 评审 D 时请连同该 commit 一起看，
> 不要只翻 `de932d7`。

### 7.3 回声治理的**四种形态**与 DSML 回退（issue #66，2026-10-21）

`#46` 只覆盖「整段合法回声」这一种形态。真机样本（issue #66）里还有三种，任何一种
落到客户端都是「任务在最该调工具的地方断掉、界面显示一坨内部标记」：

| 形态 | 处理 | 唯一实现 |
|:--|:--|:--|
| `marker + 合法 JSON 数组` | 还原成结构化 `tool_calls` | `api::parse_leaked_tool_calls` |
| `marker + 数组 + DSML 闭标签残片` | 同上（**收尾容错**：数组之后的尾巴只允许 `<` / ``` —— 也就是「不是人话」） | `api::parse_leaked_tool_calls_with_trailing` |
| `marker + 截断/损坏的数组` | **吞掉正文**，只保留上游 `finish_reason` | `api::leaked_partial_droppable` |
| `[工具结果…]` 回声块 | **吞掉正文**（文本化历史没有 `tool_call_id`，还原不了） | `api::tool_echo_droppable` |
| `<｜｜DSML｜｜ calls>` 调用块 | 还原成结构化 `tool_calls`，标签从正文剥掉 | `api::parse_dsml_tool_calls` |

★ **为什么客户端会看到 DSML**：客户端的 system 教模型用 DSML 语法发起工具调用；
Qoder 上游跑文本模式时模型会**照学**，把调用写成标签塞进正文。这条与「回声」是
**两个不同的问题**（一个是复述历史，一个是本轮真的在调工具），不能互相顶替。

三条纪律：

1. **散文与标记同帧时先切分**：`LeakGuardStream` 在帧内定位标记起点，标记之前的
   散文**立即发出**（不拖首字延迟），标记及其后进扣帧流程。只判「整帧以标记开头」
   会漏掉「散文 + 标记」这种最常见的形态。
2. **DSML 守卫排在泄漏守卫之后**（`QoderOpenAiStream → LeakGuard → DsmlGuard → UsageTap`）：
   前者合成的帧已是结构化 `tool_calls`，后者据此判定「不再合成第二组」；顺序反了会
   各还原一遍，客户端会把同一组工具执行两次。
3. **还原不了就吞掉，绝不透传内部标记**：这是 `#46` 那条「fail-open」的**唯一例外**，
   前提是「本次请求声明了 tools」（未声明 tools 时整段只可能是正常讨论，必须透传）。

★ **两条真机才暴露的形态**（2026-10-21 实测，务必保留在判据里）：

- 标签名会**紧贴哨兵**：`<｜｜DSML｜｜calls>` / `<｜｜DSML｜｜parameter name="command">`
  （中间没有空格）。按「整词等于 calls」判会全部落空 ⇒ 整块被当成截断 DSML 吞掉
  ⇒ 用户看到**空回答**。判据必须按**词尾**匹配。
- 收尾帧常带 `"content":""`：空串必须当「无正文」，否则绕过收尾改写，
  合成调用后 `finish_reason` 仍是 `stop`（客户端当普通回答、不执行工具）。
- **未声明 tools 的请求一律原样透传**（`DsmlGuardStream::new` 收 `allowed_tools`）：
  那时合成出来的调用客户端根本执行不了，比透传更糟。

合成调用后 `finish_reason` 必须改写成 `tool_calls`（Anthropic 侧据此映射 `tool_use`）；
上游没发收尾帧时补一帧，避免客户端悬挂。

签到侧的同类缺陷（同一 issue）：**一次 404 不足以断言「本区域没有活动平台」**。单次
404 会让签到回落旧通道，旧通道对国内版恒返回 `DISABLED` ⇒ 界面「无法签到」，而真相
往往只是这一发请求没打到接口。`campaign::fetch_campaigns_with_identity` 因此在
判「整区不可用」之前**原样重发一次**（`transient_404_is_retried_before_concluding_the_region_is_unavailable`）。
读不到签到状态时，账号抽屉必须给出**原因**（上游错误 / 无机器身份 / 本区域无活动），
不能像从前那样让整个「签到」页签静默消失。

### 7.4 与参考实现的功能对等（2026-10-21）

以 `qoder2api-hub` 为参照补齐的能力（每条都有测试或真机证据）：

| 能力 | 落点 | 关键取舍 |
|:--|:--|:--|
| Responses API（Codex） | `protocol/responses.rs` + 两条 Responses 路由 | 协议翻译复用同一条中继（选号/换号/记账一份） |
| **freeform（custom）工具** | 同上 + `custom_tool_names` | `apply_patch` 出站降级成单 `input` function；入站历史还原；出站还原成 `custom_tool_call`。**不发 `custom_tool_call_input.delta`**（上游给的是 JSON 片段，按规范会被拼进原文；原文只在 `.done` 给） |
| 模型清单计费与能力 | `payload::admin_models_response` + 模型页「价格 / 思考」列 | `price_factor` / `is_free` / `promotion` / `thinking_efforts` 原样带上；官方没给就渲染「—」 |
| 排队信号（HTTP 级） | `pool::classify_http_with_detail` | 401/403 里的 `10605` / `isQueued` ⇒ `Queued`：不冷却、不标 dead、不推 EMA；真失效词优先 |
| 排队信号（信封级） | `sse::process_line` + `aggregate_relay` | 200 + `statusCodeValue:403` ⇒ 429 `upstream_queued`，文案说清「不是账号问题」 |
| 风控遥测 | `/health` 的 `risk.{queued,rateLimited,rejected}` | 进程级累计、只读、不落盘；只在分类处记一次 |
| 历史工具结果削减 | `api::shrink_tool_result` + `flatten_messages` | 单侧默认 2000 字符（`BUDDY_SWITCH_QODER_TOOL_KEEP` 可调，`off` 关闭）；保护**最后 1 条**（`…_KEEP_TAIL`）；标记固定串 ⇒ 不打碎前缀缓存 |
| 单号在途闸门 | `pool::acquire` + `InflightGuard` | **默认关**（`BUDDY_SWITCH_QODER_MAX_INFLIGHT`，0 = 关）；名额抢不到就**跳过该号**而不是排队等；`Arc<AtomicUsize>` + Drop 归还，客户端断开也不会泄漏 |
| **限额护栏（四条）** | `qoder::quota` + 网关页「限额护栏」卡 | G1 余额地板 / G2 当日 token / **G3 当日 credit** / G4 单模型当日 token；**默认全关**、**未知一律放行**；计数来自 `record_request` 单一漏斗（不从环形日志推导），落盘 `qoder/gateway_usage.{region}.json`，每天 10:00（UTC+8）重置。G3 的 credit 取自上游 `usage.credits`（真机确认逐帧都给，浮点） |
| 最小选号间隔（pacing） | `pool::parse_min_pick_gap` → `PickPolicy::min_pick_gap_ms` | **默认关**（`BUDDY_SWITCH_QODER_MIN_INTERVAL_MS`）；软约束：短名单剔除「刚用过」的号，全员都在间隔内则 LRU 兜底 —— **不会拒服务** |
| **402 余额停放** | `classify_http(402)` + `quota::park_exhausted` | 402 ≠ 客户端参数错：换号 + 停到下一个重置窗口（**不是**几分钟的短冷却，那只会再撞一次）；余额视图看到 `remaining > 0` 才解封 |

**刻意不做**（附理由，别当遗漏）：

1. **Pro 升级包** 已实现（`campaign::pro_pack_status` / `claim_pro_pack` + 抽屉入口）：
   端点 404/403/410 ⇒ `available=false`，前端**不渲染入口**（真机本机 CN 账号就是 404，
   活动已下线），领取也拒绝并把原因说清 —— 不会出现「点了没反应」的死按钮。
2. **G3 当日 credit 上限**：**已实现**。原先以为「上游不给按请求的 credit 成本」，
   真机复验时发现 `usage.credits` 每帧都有（如 `0.348604025`）—— 于是计数直接取自
   记账漏斗，与 token 同源，不需要客户端日志那一路（那是 IDE 自己的用量，口径不同）。
3. **最小间隔（pacing）**：**已实现为可选**（默认关）。参考实现按账号 + 客户端 IP
   统计节奏；我们只做「同账号两发之间的最小间隔」，且用选号内核现成的**软约束**
   （`PickPolicy::min_pick_gap_ms`：剔除刚用过的，不够人就 LRU 兜底）—— 不引入
   「等一会儿再发」的阻塞语义，也就不会把客户端超时拖成「上游很慢」。
4. 历史工具结果削减**已实现**（上表）；需要「一条都不削」时设
   `BUDDY_SWITCH_QODER_TOOL_KEEP=off`。
5. 四条限额护栏**已实现**（上表）：阈值在网关页「账号池 → 限额护栏」里配，
   逐账号当日用量（token / credit）与拦截原因同页可见（判据与选号**共用同一个函数**，
   面板说「已拦」就是真会拦）。

### 7.5 已知边界（不是遗漏，别照着猜做）

| 边界 | 现状 | 为什么现在不做 |
|:--|:--|:--|
| **团队 / 企业账号的「共享额度池」** | 不建模：G1 与余额显示都用**账号自身套餐**的 `remaining` | 参考实现按 `credits.shared` / `remain_effective` 判（团队套餐用满但共享池还有钱时不能停号）。本机只有个人账号，上游对团队账号的字段名与语义**无从验证**；照着猜字段名写解析，线上只会是一段永不执行的死代码 —— 宁可留一句诚实的边界。真机拿到团队账号响应后再补（届时改 `api::parse_quota` 一处即可，护栏不用动）。 |
| 限流探测（`ModelProbeCard`） | **不走**限额护栏 | 探测是诊断动作（一个极小的请求），被日限拦下会让「模型到底通不通」永远测不出来；它也不计入 G2/G4 的日用量。 |
| 402 停放的**跨重启** | 落在 `gateway_usage.{region}.json` 的 `parked`，重启后仍生效 | 已做；这里只提醒「清掉这个文件 = 也清掉停放」。 |

### 7.6 结构化工具历史的**启用面**（含 DeepSeek 族，issue #72，2026-10-21）

`api::structured_tool_history_enabled` 的 auto 判据（未设 `QD_STRUCTURED_TOOL_HISTORY` 时）：

| 条件 | 结论 |
|:--|:--|
| `tool_ids_ok(messages) == false`（`tool` 缺 `tool_call_id` 或 `tool_calls[]` 缺 `id`） | **一律文本模式**（fail-safe，优先于下面所有分支） |
| `QD_STRUCTURED_TOOL_HISTORY=off` | 一律文本模式（逃生阀） |
| `QD_STRUCTURED_TOOL_HISTORY=on` 等 | 一律结构化（**含国际版出口**，仅用于救急） |
| 出口 ≠ CN | 文本模式 |
| key 属 DeepSeek 族（`dmodel` / `dfmodel`，前缀匹配） | **结构化**（issue #72） |
| key 命中 `STRUCTURED_TOOL_HISTORY_WHITELIST`（Qwen / GLM 族前缀） | 结构化 |

### 7.7 结构化历史的**自动降级**（issue #72，2026-10-21）

静态白名单只能**猜**上游接不接受结构化（本仓无真机账号）。所以判错的代价不落在用户身上，
而是落在一次内部重试上 —— `qoder/degrade.rs`：

1. 结构化请求被上游按**参数错**拒（`400` / `422`）；
2. 网关用**同一账号**、文本模式重发一次（参数错跟账号无关，换号只是把同一份坏请求
   喂给另一个号）；**只重发一次**，绝不循环放大；
3. **重发成功** ⇒ 记住 `(区域, 模型)`，TTL 6 小时内该模型**直接走文本模式**，
   不再每次先撞 400。

★ **为什么「重发成功」才落记忆**：文本模式也 400 ⇒ 400 与结构化无关（上游整体坏了）。
此时若记下「结构化不被接受」就是**误判** —— 该模型会长时间退回文本模式，
而文本模式正是 issue #72 要修的乱码形态。宁可多撞一次 400（用户看不到），
也不能把错误结论固化 6 小时。

三条纪律：

- **只对参数类拒绝降级**：401/403（凭据）、429（限流）、5xx（上游故障）与请求体形状无关，
  换文本模式重发只会白烧一次额度。判据是纯函数 `degrade::should_degrade`。
- **降级那一次不记账、不罚账号、不计风控遥测**：不是账号的问题，也不是客户端参数的问题。
  一次请求只能有**一条**请求日志（记最终结果），否则用量与延迟统计都会翻倍。
- **记忆只在进程内**（不落盘）：重启重学的成本是「每条请求多一次 400 + 一次重发」，
  不值得为它引入文件写 + 并发 + 清理。TTL 到期也会重学（上游可能哪天就支持了）。

> 这条机制同样兜住了「孤儿 `tool_calls`」（客户端中断工具调用 / 压缩上下文只裁掉一侧）：
> 上游判 `tool_call_sequence_broken` 也是 400 ⇒ 自动降级。所以 `tool_ids_ok` **刻意只查
> id 非空、不查配对** —— 在这里提前判配对属于「猜哪些请求会 400」，与降级机制的设计哲学冲突。

### 7.8 出站**清洗**（与 WB / Trae 同款，issue #72）

孤儿历史不只靠降级兜底，还在**发出去之前**清干净 —— 与 WB / Trae 侧的
`outbound::cleanup_orphan_tool_calls` **逐条一致**（保留集 / 批次裁决 / 存活批次）：

- `api::cleanup_orphan_tool_history`（core 侧，Qoder 专用）与
  `buddy-switch-gateway::outbound::cleanup_orphan_tool_calls`（WB / Trae 侧）**同款语义**，
  两侧互指注释 + 各自的测试；
- 保留集 = `assistant.tool_calls[].id` ∩ `role:"tool"` 的 `tool_call_id`；
- assistant 批次**任一** id 是孤儿 ⇒ **整批**删 `tool_calls`（保留正文）；
- `role:"tool"` 的 `tool_call_id` 不在**存活批次** ⇒ **整条**删除。

★ **为什么是「清洗」而不是「退化成文本模式」**：清洗**保住**配对完整的那部分结构化历史；
退化成文本模式会把整段历史变成伪格式（`[assistant 请求调用工具]` + JSON），模型照抄就是
issue #72 的乱码 —— 用一个「看着像乱码的退化」换一个「干净的 400」，不划算。

★ 清洗**只作用于结构化路径**：文本模式的伪格式对畸形配对是宽容的，且它保留的信息量更大
（文本化本就不保留 `tool_call_id`，只能靠标记回读）。「id 完全缺失」的客户端仍走文本模式。

> 移植遗留：本仓 Qoder 侧当初只搬了「id 是否齐备」的检查，**没搬清洗**（WB / Trae 侧搬了）。
> ⇒ 孤儿历史会变成畸形请求发出去。2026-10-21 补齐。

★ **issue #72 的教训**：DeepSeek 原先被排除在白名单外，并用一条**反向断言**钉死
（理由是「本机无账号、未验证」）。真机（DeepSeek Flash + dsh）结果是：文本模式下模型会把
`[assistant 请求调用工具]` + 紧凑 JSON、`[工具结果 (name)]` 这套**网关内部序列化格式照抄进正文**，
再混上客户端 system 教的 `<｜｜DSML｜｜ calls>` 语法 ⇒ 用户看到「上下文乱码 / 特殊字符」，
工具一次也不执行。报障者用 `QD_STRUCTURED_TOOL_HISTORY=on` 强制后症状消失，说明上游**接受**
结构化历史 —— 即旧前提错误。判定入口统一为 `api::is_deepseek_key`（`keep_reasoning` 与
结构化判据**必须同源**，否则会出现「结构化开了、`reasoning_content` 没回传」的半开状态）。

> 反向断言的价值是「别把没依据的 key 捞进来」，但它同时会把**未验证的缺陷**固化成契约。
> 加这类断言时，注释里必须写清「凭什么是错的」和「怎么证伪」，否则下一次只能靠用户报障来推翻。

**未做（需拍板）**：回声守卫的第二道防线在 issue #72 的样本里也没兜住 ——
`parse_leaked_tool_calls_with_trailing` 要求回声数组里的工具名过 `allowed_names` 严格校验，
样本里模型把 `pwsh` 写成了 `pssh` ⇒ 校验失败 ⇒ fail-open 原样透传 ⇒ 标记直接打到界面。
收紧方向是「名字不匹配 ⇒ 吞正文 + 502 让客户端重试」，但那会改变 §7.2 纪律 2 的 fail-open
语义（误判时用户会丢一整轮正文），必须单独拍板，不能顺手改。

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
4. **「峰谷价 / 思考档位 / 多上下文窗口」的展示**（issue #65，2026-10-21 **已落地**）：
   App 侧**三平台一起加**了两列（思考档位 / 积分倍率），见 `P11-ARCH.md` §1.6.1。
   原型（`docs/prototype/`）仍是旧的五列 —— 它本来就多出「协议 / 归属」两列、与 App
   已有差异（决策 #1/#2），本次**不跟随**，避免为了同步把原型的交互套件打红。
   多上下文窗口不做：`max_context` 开关已经覆盖两档，再加一列是重复。

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
