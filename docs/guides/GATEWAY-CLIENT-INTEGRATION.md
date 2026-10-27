# 网关接入指南：客户端接入 + 上游扩展

> 适用版本：`Buddy Switch` 当前主线（网关 crate `buddy-switch-gateway`）。
> 本文只写**能核实**的内容。凡是本仓没有证据、只能靠外部文档的，一律显式标注「以官方文档为准 / 未在本仓验证」，**不编造配置片段** —— 编错的配置比没有说明更糟：用户会以为网关坏了。

---

## 0. 先分清两个方向，别搞混

「接入」在这份文档里有两个完全不同的含义，工作量差两个数量级：

| | 方向 A：客户端接入网关 | 方向 B：上游扩展 |
|:---|:---|:---|
| 干什么 | 让本机其它 AI 工具用上网关的额度 | 给网关**增加新的额度来源** |
| 手段 | **改配置文件 / 环境变量**，不改代码 | **改本仓代码**，一次完整工程改造 |
| 举例 | Codex、Claude Code、Hermes、dsh、Trae IDE、CodeBuddy CLI 指向网关 | 把 Codex/ChatGPT 账号、Claude 账号接成第三个上游 |
| 谁适合 | 所有人 | 要发版的人 |
| 本文位置 | 第 1–3 章 | 第 4 章 |

**一句话**：A 是「填表」，B 是「加产品线」。别把 B 当成 A 来承诺。

---

## 1. 网关能力矩阵（事实）

三个产品分区（WorkBuddy / Trae / Qoder）**彼此完全独立**：独立端口、独立 Key 库、独立账号池、独立配置文件。**Key 不能跨区使用**。

自 2026-10-14 起还有**第四个入口「统一网关」**（跨作用域、默认关），见 §1.3 —— 它的定位不是替代前三个，而是「一个 provider 用遍全部平台」。下表第三列是它；Qoder 分区见 §1.4（只列差异，其余逐条同构）。

> ★ 2026-10-17 **端口统一到 `5789x` 一段**：界面 57890 · WB 网关 57891 · 统一入口 57892 · hook 57893 · **Trae 网关 57894** · **Qoder 网关 57895**（另 8899 是 Trae 登录代理，与网关无关）。Trae / Qoder 原来是 `7864` / `7865`，与 WB 的 `57891` 混在两段里，看不出规律。
> 只改**默认值**：配置里写着旧端口的保持不变（`trae/api_gateway.json` / `qoder/gateway_config.json` 的 `port` 优先）。

| 项 | WorkBuddy 分区 | Trae 分区 | 统一入口（第三个入口） |
|:---|:---|:---|:---|
| 额度来源（上游） | WorkBuddy 账号库（CN / Global 两套） | Trae 账号库（4 个程序位） | **两者都借**：按模型名的命名空间前缀分派 |
| 默认监听端口 | `57891` | `57894` | `57892` |
| 根地址（不含 `/v1`） | `http://127.0.0.1:57891` | `http://127.0.0.1:57894` | `http://127.0.0.1:57892` |
| OpenAI 基址（含 `/v1`） | `http://127.0.0.1:57891/v1` | `http://127.0.0.1:57894/v1` | `http://127.0.0.1:57892/v1` |
| `GET /v1/models` | ✅ | ✅ | ✅（聚合四段命名空间，id 带前缀） |
| `POST /v1/chat/completions` | ✅ | ✅ | ✅ |
| `POST /v1/messages`（Anthropic） | ✅ | ✅ | ✅ |
| `POST /v1/responses`（Responses API） | ✅ | ✅（2026-10-22 起） | ✅（三个命名空间都可用） |
| 健康检查（免鉴权） | `GET /health`（兼容 `/healthz`） | `GET /health`（兼容 `/healthz`） | `GET /health`（兼容 `/healthz`） |
| `GET /status`（需鉴权） | ✅ | ✅ | ✅ |
| 业务转发 `POST /biz/*path` | ✅（WB 独有） | ❌ | ✅（命名空间来自 Key 的 `biz_namespace`） |
| **认证头** | `x-api-key` **优先**，其次 `Authorization: Bearer` | **只认 `Authorization: Bearer`** | **两者都收**（同 WB 规则） |
| API Key 前缀 | `sk-wb-` | `sk-trae-` | `sk-uni-` （Qoder 分区：`sk-qoder-`，见 §1.4） |
| Key 落盘 | `~/.buddy-switch/gateway_keys.json` | `~/.buddy-switch/trae/api_gateway_keys.json` | `~/.buddy-switch/gateway_unified_keys.json` |
| 网关配置落盘 | `~/.buddy-switch/gateway_config.json` | `~/.buddy-switch/trae/api_gateway.json` | `~/.buddy-switch/gateway_unified.json` |
| 请求体上限 | 默认 8 MB（`max_body_mb`） | 同（`maxBodyMb`） | 同（`max_body_mb`） |

### 1.1 三条必须记住的硬事实

1. **`/v1/responses` 三条产品线都有**（Trae 于 2026-10-22 补齐），`wire_api = "responses"` 处处可用。
   Trae 与 Qoder 侧都走**协议翻译**：入站 Responses 翻成 chat，出站把上游 chat SSE 转成 Responses 事件流（非流式转 Responses 对象）。WB 侧上游本就是 chat 方言，同样翻一层。
   ⚠️ 曾经的「Trae 没有 ⇒ 501」已随补齐移除；若你按旧文档把 Codex 配成 `wire_api = "chat"`，仍然能用，只是拿不到 Responses 的 `output`/`output_text` 形状。
2. **两边的认证头规则不一样。**
   WB 侧与统一入口 `x-api-key` 和 `Authorization: Bearer` 都收；Trae 侧**只收 `Authorization: Bearer`**（Qoder 侧同样只认 Bearer）。
   ⇒ Claude Code 接 Trae / Qoder 网关时**必须用 `ANTHROPIC_AUTH_TOKEN`**（走 Bearer）；用 `ANTHROPIC_API_KEY`（走 `x-api-key`）会拿到 401。
3. **端口不能合并。**
   四套网关挂的路由**逐字重叠**（`/v1/models`、`/v1/chat/completions`），axum 同路径 merge 会**运行期 panic**。所以永远是四个端口、四个 Key 库、四份配置。

### 1.2 额度来源与区域

- WB 网关：Key 绑定 **region**（`cn` / `global`）。同一把 Key 只能路由到该域的账号。
- Trae 网关：Key 绑定 **variant**（`TraeWork CN` / `TraeWork` / `TraeCode CN` / `TraeCode`），决定用哪个程序位的账号池。
- 模型名前缀 `cn:` / `global:` **默认关闭**（`allow_model_region_prefix = false`）。关闭时同域前缀会被剥离，跨域前缀返回**可诊断的 400** —— 这是有意的安全取舍：Key 的域与账号库强校验，客户端不能自行越域。
- ⚠️ **这套 `cn:` / `global:` 前缀属于 WB 独立端口，与统一入口的命名空间是两回事**，两套前缀不跨入口混用。

### 1.3 第三个入口：统一网关（可选，默认关）

**它解决什么**：两个既有网关的模型 id **不含来源** —— 同一个 `deepseek-v4.1-flash` 在 WB 国内 / WB 国际 / Trae 国内 / Trae 国际是四条不同上游、不同倍率，而客户端按 id 去重 ⇒ 只能存在一条、会互相覆盖。统一入口让 **id 自己携带来源**。

**怎么用**：

1. 网关页 →「统一入口」分区 → 打开「启用统一网关」（默认关，端口 `57892`）。
2. 在「统一密钥」卡创建一把 Key —— **一把 Key 可同时授权多个命名空间**（如 `wb-cn` + `trae-cn`）。
3. 客户端里 Base URL 填 `http://127.0.0.1:57892/v1`，认证头用 `Authorization: Bearer sk-uni-…`。
4. 模型名必须带前缀：`wb-cn:deepseek-v4.1-flash`、`trae-global:…`。**不带前缀一律 400**，错误体里给出可用前缀清单。

| 命名空间 | 默认 | 说明 |
|:---|:---|:---|
| `wb-cn` / `wb-global` | ✅ 开 | WorkBuddy 国内版 / 国际版 |
| `trae-cn` / `trae-global` | ✅ 开 | Trae **合并版**客户端（`Trae CN` / `Trae`） |
| `qoder-cn` / `qoder-global` | ✅ 开 | Qoder 国内版 / 国际版 |

> 命名空间**派生自作用域注册表**（`Scope::all()`）：共 6 条，全部默认启用。Trae 的两条老程序线（`traework-*` / `traecode-*`）已随「客户端合并」清空扩展层，不再出现在表里。

**三条硬边界**：

1. **解析只做整段精确匹配，禁止前缀匹配** —— `trae` / `traework` / `traecode` 互为前缀，任何 `starts_with` 式推断都会把请求发给另一套账号池。
2. **跨作用域**：统一入口把三条产品线合成一个入口，因此**页面上的作用域选择器对它不生效**（卡 1 顶部有明确说明）。
3. **它只是「借用」三条产品线的账号池**：统一入口不自己持有账号池；关掉统一入口不影响 57891 / 57894 / 57895 的任何行为。

> 架构与逐条决策见 [`specs/UNIFIED-GATEWAY-ARCH.md`](../specs/UNIFIED-GATEWAY-ARCH.md)，交互规格见 [`specs/UNIFIED-GATEWAY-UI.md`](../specs/UNIFIED-GATEWAY-UI.md)。

### 1.4 Qoder 分区（2026-10-16 新增）

| 项 | 值 |
|:---|:---|
| 默认监听端口 | `57895`（原 `7865`；只改默认值） |
| OpenAI 基址 | `http://127.0.0.1:57895/v1` |
| Anthropic 基址 | `http://127.0.0.1:57895`（`/v1/messages`） |
| API Key 前缀 | `sk-qoder-` + 32 位 hex（与 `sk-wb-` / `sk-trae-` / `sk-uni-` 同族；原 `qd-` 前缀的老 Key **照常可用**，校验只比 sha256） |
| 认证头 | **只认 `Authorization: Bearer`**（同 Trae） |
| 区域归属 | **由 Key 的归属区域决定**（`cn` / `global`），一个监听同时服务两区 |
| Key / 配置落盘 | `~/.buddy-switch/qoder/gateway_keys.json` / `gateway_config.json` |
| 协议 | OpenAI（`/v1/models`、`/v1/chat/completions`、`/v1/responses`<sup>†</sup>）+ Anthropic（`/v1/messages`） |

<sup>†</sup> Trae / Qoder 上游只有 chat SSE ⇒ `/v1/responses` 由网关做协议翻译（入站翻成 chat、出站转 Responses 事件流 / 对象），
Codex 的 `wire_api = "responses"` 可直接用。翻译边界同 WB（见 §3.1「坑」）。

---

## 2. 通用前置（所有客户端都要做）

### 2.1 启动网关

桌面端：侧栏 →「网关」→「概览」→ 打开网关开关。
webui/CLI：`buddy-switch serve` 后同样在「网关」页打开开关。

> 开关默认是**关**的。没开 = 端口上没人监听 = 所有客户端连接被拒。
> 统一入口（§1.3）是**另一个独立开关**，在网关页的「统一入口」分区里。

### 2.2 创建 API Key

网关页 →「密钥」→ 创建，选择归属区域/程序位。（统一入口的 Key 在「统一入口」分区的「统一密钥」卡里创建。）

- 明文 `sk-wb-<32 hex>` / `sk-trae-<32 hex>` / `sk-uni-<32 hex>` **只在创建时显示一次**；库里只存 `sha256` + 前缀 4 位 + 尾 4 位。
- 丢了就**吊销重建**，找不回来。（若只是误吊销，可以直接**恢复**；恢复后判据是**行为**——`verify` 重新通过，不是看字段被清。）
- 列表里显示成 `sk-wb-a1b2••••••••9f3c` —— 中间那串省略号**不是** Key 的一部分，**别往配置里粘**。

### 2.3 记下该填哪个地址（这是最常见的坑）

| 客户端类型 | 该填什么 | 值 |
|:---|:---|:---|
| **OpenAI 系**（Codex、opencode、Trae 自定义模型、CodeBuddy `models.json`） | **含 `/v1` 的基址** | `http://127.0.0.1:57891/v1` |
| **Anthropic 系**（Claude Code、CodeBuddy `CODEBUDDY_BASE_URL`、dsh `anthropic-messages`） | **根地址，不带 `/v1`** | `http://127.0.0.1:57891` |
| **统一入口**（两系都行） | 同上规则，端口换成 `57892` | `http://127.0.0.1:57892/v1`（OpenAI 系）/ `http://127.0.0.1:57892`（Anthropic 系） |

Anthropic 系客户端**自己会拼 `/v1/messages`**；给它带 `/v1` 的地址会拼成 `/v1/v1/messages` → 404。

> ⚠️ 界面上的「Base URL」一行显示的是**根地址**（后端 `base_url` 字段就是 `http://host:port`）。照抄进 OpenAI 系客户端会缺 `/v1` —— 见附录 B。

### 2.4 模型名不用手填（但也不能瞎填）

`GET /v1/models` 会返回该 Key 所属域/程序位下**可用且未被禁用**的模型清单。多数客户端（Codex 除外）会自动发现。

- **Codex、Trae 自定义模型、CodeBuddy `models.json` 需要手填模型 ID** —— 从 `/v1/models` 里复制，别猜。
- `/v1/models` 返回 200 **不等于**该模型此刻一定能用（上游可能限流）。清单只代表「网关愿意暴露」。
- **统一入口的模型名必须带命名空间前缀**（`wb-cn:…`）—— 不带前缀一律 400，不做兜底猜测。见 §1.3。
- **Trae 的上下文上限（Max Mode）逐模型不同，且不需要特殊模型名**：网关从 Trae 官方模型目录
  读取每个模型的 `dev`（`__dev` 通道实际窗口）与 `max`（Max Mode 上限）；`max > dev` 的模型在请求时
  注入 `is_max_mode`，`/v1/models` 直接声明它**自己的** `max`。
  - 不是统一 1M：实测 `glm-5.3` 200k→1M、`mimo-v2.6-pro` 200k→981504、`Doubao-Seed-2.1-Turbo` 上限仍是 256k；
    清单里 `max_mode: true` 表示这条已开 Max。
  - 目录拉不到（无账号 / 网络）⇒ 按客户端清单的默认窗口声明，**不猜数字**。
  - 基名即可（`glm-5.3`、统一入口 `trae-cn:glm-5.3`），不用加后缀、不用另选一条。

### 2.5 流式

两个协议都支持 SSE。网关对上游**强制 `stream: true`**，客户端要非流式时由网关聚合后再返回。

### 2.6 60 秒自检（先跑这个，再配客户端）

WorkBuddy 分区：

```bash
export BUDDY_SWITCH_KEY="sk-wb-在这里粘你的key"

# 1) 模型清单（OpenAI 风格）
curl -s http://127.0.0.1:57891/v1/models \
  -H "Authorization: Bearer $BUDDY_SWITCH_KEY"

# 2) OpenAI 端点
curl -s http://127.0.0.1:57891/v1/chat/completions \
  -H "Authorization: Bearer $BUDDY_SWITCH_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"<从清单里复制>","messages":[{"role":"user","content":"hi"}]}'

# 3) Anthropic 端点（注意认证头不同）
curl -s http://127.0.0.1:57891/v1/messages \
  -H "x-api-key: $BUDDY_SWITCH_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"<从清单里复制>","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'
```

Trae 分区：把端口换成 `57894`，且**认证头统一用 `Authorization: Bearer`**（`x-api-key` 会被 401）。

### 2.7 错误对照表

| 症状 | 真实原因 | 怎么办 |
|:---|:---|:---|
| **`400` + `"code":"11-128"`**（`request illegal` / 「这次调用没通过渠道校验」） | 上游渠道校验要求**会话头三件套**齐全（`X-Conversation-ID` / `X-Conversation-Request-ID` / `X-Conversation-Message-ID`）；旧版网关在客户端没传 `X-Conversation-ID` 时会漏发 | 升级到 **`v2026.10.171833`+** 后重启网关。**与额度、登录态无关** —— 重登 / 换号 / 改客户端配置都没用 |
| `401 缺少凭据` | 客户端没发认证头 | 检查 Key 环境变量是否真的存在（新开终端 / 重启客户端） |
| `401 API Key 无效或已吊销` | Key 错、被吊销、或跨区使用 | 重新创建；确认 Key 属于当前分区 |
| `401` + Trae 网关 + Claude Code | 用了 `ANTHROPIC_API_KEY`（发 `x-api-key`） | 改用 `ANTHROPIC_AUTH_TOKEN` |
| **`401` + `"type":"no_credential"`**，且文案里带「N 个已禁用 / 冷却中 / 余额为 0 / 被你排除 / 在途已满」 | **号在库里，但此刻一个都用不了** —— 不是没登录，重新登录/切换账号救不了 | 按文案里的计数去「网关 → 账号池」看逐账号原因。禁用是终态（需余额刷新成功或人工恢复）；排除点「取消排除」；「在途已满」会自己好 |
| **`503` + `"type":"pool_busy"`** | 号**都健康**、凭据与余额正常，只是**在途并发占满**（旧版本这里错报成 401，会把人骗去重新登录） | 响应头已带 `Retry-After: 2`，等 2 秒重试即可。长期出现说明并发确实高：调高「每账号在途上限」或增加账号 |
| **`402`** | 上游余额不足（网关按硬冷却处理，通常冷到次日 04:00） | 换号由网关自动处理；**全都余额不足**时才会把这个 402 透出来 —— 该去充值/换号了 |
| `404` | 路径拼错，或 Codex 对 Trae 网关用了 `responses` | 见 §2.3；Codex 接 Trae 必须 `wire_api = "chat"` |
| `413` | 请求体超过 `max_body_mb`（默认 8MB） | 调大后**重启网关**（该值在 router 构建期固化，不热生效） |
| `429` | 上游限流 / Key 限流 | 换号由网关自动处理；持续 429 看「请求日志」与「客户端限流台账」 |
| `502` | 上游错误 / 账号池无可用户 | 看「API 服务」页的诊断行（逐账号说明为什么不能路由） |
| 连不上 | 网关开关没开 / 端口被占 | 看「API 服务」页的运行状态与实际监听地址 |

> **排障先看 `error.attempts`**：失败响应里有一份「这一轮换过哪些号、每个为什么不行」的轨迹
> （`account` / `uid` / `status` / `reason`）。有它就不用翻日志猜网关到底换号了没有；
> 空数组表示压根没打到上游（例如被模型闸门挡住）。
>
> **`Retry-After` 只出现在 `pool_busy` 上**：429 / 402 的真正解冻时刻在上游响应里，本仓拿不到
> 可信值，宁可让客户端自己指数退避，也不编一个假的秒数。
>
> **`11-128` 是版本问题，不是配置问题**：`092c5e3` 起 chat 出站恒发上面那三件套
> （`session_headers::resolve_conversation_id` 阶梯到按 `session_key` 稳定派生，不再留空），
> 国际版 `wb-global` 与国内版走同一条出站路径 —— WB 老入口 `57891` 与统一网关 `57892` 都吃这个修复。
> 仍报 400 就先看「设置 → 关于」的版本号，别去翻 Key 或重登账号。

---

## 3. 客户端接入（逐个）

下面每个客户端都给：**结论 → 要改哪个文件 → 完整片段 → 怎么验证 → 坑**。

> 通用纪律：**改完必须重启客户端**（除 CodeBuddy `models.json` 支持热重载）。环境变量在进程启动时读取。

### 3.1 Codex（OpenAI Codex CLI）

**结论**：改 `~/.codex/config.toml`，加一个自定义 `model_provider`。WB 与 Qoder 网关都能用 `wire_api = "responses"`；Trae 网关只能用 `wire_api = "chat"`。

**WorkBuddy 网关（推荐 `responses`）**

```toml
# ~/.codex/config.toml
model = "<从 /v1/models 复制的模型 id>"
model_provider = "buddy_switch"

[model_providers.buddy_switch]
name = "Buddy Switch"
base_url = "http://127.0.0.1:57891/v1"
env_key = "BUDDY_SWITCH_KEY"
wire_api = "responses"
```

**Qoder 网关（`responses` 亦可；网关内翻译成 chat）**

```toml
model = "<从 http://127.0.0.1:57895/v1/models 复制的模型 id>"
model_provider = "buddy_switch_qoder"

[model_providers.buddy_switch_qoder]
name = "Buddy Switch (Qoder)"
base_url = "http://127.0.0.1:57895/v1"
env_key = "BUDDY_SWITCH_KEY"
wire_api = "responses"
```

**Trae 网关（必须 `chat`）**

```toml
model = "<从 /v1/models 复制的模型 id>"
model_provider = "buddy_switch_trae"

[model_providers.buddy_switch_trae]
name = "Buddy Switch (Trae)"
base_url = "http://127.0.0.1:57894/v1"
env_key = "BUDDY_SWITCH_KEY"
wire_api = "chat"
```

Key 放环境变量，**不要写进 TOML**：

```bash
# macOS / Linux：写进 ~/.zshrc 或 ~/.bashrc
export BUDDY_SWITCH_KEY="sk-wb-..."
```
```powershell
# Windows（新开终端生效）
setx BUDDY_SWITCH_KEY "sk-wb-..."
```

**验证**：`codex exec "只回复 ok"`（或交互式里发一句话）。

**坑**

- `env_key` 是**变量名**，不是 Key 本身。填成 Key 会变成「拿 Key 当变量名去找」→ 认证失败。
- `base_url` **必须带 `/v1`**：Codex 会自己拼 `/responses` 或 `/chat/completions`。
- **Responses 支持的是一个子集**（网关侧 `protocol/responses.rs` 已写清边界）：
  - 支持：`input`（字符串 / items 数组）、`instructions`、扁平 `tools`、`tool_choice`、`temperature` / `top_p` / `max_output_tokens` / `parallel_tool_calls`。
  - **忽略而非报错**：`previous_response_id`、`store`、`include`、`background`、`stream_options`、文件 / 音频输入。
  - 其中 `previous_response_id` 要特别注意：本网关**无服务端会话状态**，依赖它做续写的调用会**静默丢上下文**。Codex 默认 `store: false`、每轮发全量历史，因此正常对话不受影响；但如果你在别处显式用了服务端会话续写，请改用 `wire_api = "chat"`。
- `reasoning` 项在 **WB 路径**会被丢弃（上游不接受）；**Qoder 路径**会翻成上游的思考档位 ⇒ `model_reasoning_effort` 生效。

### 3.2 Claude Code

**结论**：三个环境变量。`ANTHROPIC_BASE_URL` 给**根地址**（不带 `/v1`）。

```bash
# macOS / Linux：~/.zshrc 或 ~/.bashrc
export ANTHROPIC_BASE_URL="http://127.0.0.1:57891"
export ANTHROPIC_AUTH_TOKEN="sk-wb-..."
export ANTHROPIC_MODEL="<从 /v1/models 复制的模型 id>"
```

```powershell
# Windows
setx ANTHROPIC_BASE_URL "http://127.0.0.1:57891"
setx ANTHROPIC_AUTH_TOKEN "sk-wb-..."
setx ANTHROPIC_MODEL "<模型 id>"
```

也可以写进 `~/.claude/settings.json` 的 `env` 段（等价，且不污染全局环境）：

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:57891",
    "ANTHROPIC_AUTH_TOKEN": "sk-wb-...",
    "ANTHROPIC_MODEL": "<模型 id>"
  }
}
```

**接 Trae 网关**时：`ANTHROPIC_BASE_URL="http://127.0.0.1:57894"`，并且**必须**用 `ANTHROPIC_AUTH_TOKEN` —— Trae 侧只认 `Authorization: Bearer`，`ANTHROPIC_API_KEY` 会走 `x-api-key` 而被 401。

**验证**：`claude -p "只回复 ok"`。

**坑**

- **`ANTHROPIC_MODEL` 必须显式给。** 不给的话客户端会请求它自己的默认模型名，网关上不存在 —— 报错发生在**客户端侧**，用户看到的只是「模型不可用」，很难联想到是这里少了一行。
- 别给 `ANTHROPIC_BASE_URL` 加 `/v1`（会变成 `/v1/v1/messages`）。
- 同时设了 `ANTHROPIC_API_KEY` 和 `ANTHROPIC_AUTH_TOKEN` 时，行为取决于客户端版本；**只留 `ANTHROPIC_AUTH_TOKEN`** 最稳。

### 3.3 Hermes Agent

**结论**：Hermes 支持「自定义端点（任何 OpenAI 兼容 API）」。**优先用官方向导**，不要手编未核实的键名。

**首选路径（官方向导）**

```bash
hermes model      # 交互式：选「自定义端点」→ 填 Base URL + API Key
```

填的值：

- Base URL：`http://127.0.0.1:57891/v1`（**OpenAI 兼容，含 `/v1`**）
- API Key：`sk-wb-...`
- 模型：从 `/v1/models` 复制（向导可能问上下文窗口大小，按模型实际值填）

**手改配置（结构以向导写出的为准）**

- 主配置：`~/.hermes/config.yaml` —— `model:` 段（`default: <provider>/<model>`、`temperature`、`max_tokens`）
- 密钥：`~/.hermes/.env` —— 环境变量形式
- CLI：`hermes config set model.provider ...` / `hermes config set model.name ...`
- 项目级覆盖：`<项目根>/.hermes/config.yaml` 会覆盖全局配置

> ⚠️ **诚实边界**：`base_url` 在 `config.yaml` 里的**确切嵌套位置随版本变化**，本仓未逐版核实。因此：**用 `hermes model` 向导写一次，再去看它写成了什么** —— 那才是你这个版本的权威答案。手编键名请以 Hermes 官方文档为准。

**验证**：`hermes` 启动后发一句「只回复 ok」，或 `hermes doctor`。

**坑**

- Hermes 的「自定义端点」走 **OpenAI 兼容**（`/v1/chat/completions`），不要填根地址。
- 改配置后部分项需重启 Hermes / 其网关（`hermes gateway restart`）。

### 3.4 dsh（DeepSeek Harness）

**结论**：走「设置 → 模型 → Add a custom provider」。Provider ID 是**永久**的，想改名只能新建再删旧。

**UI 路径（推荐）**

1. `dsh web` 打开 Web UI（默认 `http://127.0.0.1:3080`）
2. **设置 → 模型 → Add a custom provider**
3. 填：
   - Provider ID：小写，如 `buddy-switch`（**创建后不可改名**）
   - 显示名称：`Buddy Switch`
   - Base URL：`http://127.0.0.1:57891/v1`
   - API 协议：`openai-completions`（接 WB / Trae 网关也可选 `openai-responses`；三条产品线现在都支持 `/v1/responses`）
   - 模型：点 **Fetch available models**（走 `GET /models`）→ 勾选保存
   - 输入类型：展开**自定义设置 → 模型选项**，要收图的模型勾上**图片**（新建模型默认只勾文本；要取消文本必须先勾图片，至少保留一种）
   - 凭据：直接在 Web UI 填（落到 `$DSH_HOME/.credentials.yaml`，只回传脱敏描述符），或填 `apiKeyEnv` 指向一个环境变量名

**手改 `$DSH_HOME/profiles/<profile>/cordis.patch.yml`**

配置**不在 `settings.yaml`**（DSH 里没有这个文件）：自定义 provider 落在当前 profile 的
`cordis.patch.yml`，`dsh web` 启动时 `<profile>` 是 `web`、桌面版是 `desktop`；
密钥单独存在 `$DSH_HOME/.credentials.yaml`。浏览器与服务器同机时，用设置页头部的
**Open configuration file** 直接打开这份文件。

patch 是**列表**，每项 `- id: <插件>` + `config:`，providers 挂在 `llm-pi-ai` 的 `config` 下：

```yaml
- id: llm-pi-ai
  config:
    providers:
      buddy-switch:
        displayName: BuddySwitch
        apiKeyEnv: BUDDY_SWITCH_API_KEY  # 这是「环境变量名」，不是密钥本身
        api: openai-completions          # openai-completions | openai-responses | anthropic-messages
        baseURL: http://127.0.0.1:57891/v1
        models:
          - id: <从 /v1/models 复制的模型 id>
```

⚠️ **patch 里的 override 会整段替换该项的 `config`**：手改时把同文件里其它 provider 与字段一并保留，否则会被清掉。

接 Trae 网关的 Anthropic 协议写法：

```yaml
      api: anthropic-messages
      baseURL: http://127.0.0.1:57894
```

**兼容项（只在报错时才加）**

某些 OpenAI 兼容网关不接受推理模型的 `developer` 角色，或只接受 `max_tokens`：

```yaml
      compat:
        supportsDeveloperRole: false
        maxTokensField: max_tokens
```

**视觉模型要显式声明**（手工录入的模型默认按纯文本处理）

DSH 判断一个模型能不能收图**只看配置里声明的输入模态**，不看网关实际能力。没声明就在
**提示词准入阶段**被拦下 —— 请求根本到不了网关，所以「同一模型走官方 API 能发图、走网关不行」
通常不是网关的问题，而是 DSH 侧没把这项能力声明出来。`input` 只对该模型生效：

```yaml
        models:
          - id: <模型 id>
            input: [text, image]
```

全部手填模型都收图时，在**路由**上写一次兜底，别逐条重复：

```yaml
        defaultInput: [text, image]
```

- 优先级：条目 `input`（非空）> 已装目录 > 路由 `defaultInput`（默认 `[text]`）。
- **收回纯文本要写 `input: [text]`**；`input: []` 等价于没写，会继续继承 `defaultInput` 的图片能力。
- 内置 provider 没有显式 `models` 列表时，用 `modelOverrides` 按模型 id 写 `input`。
- 未知模态值会被拒；除模型自身的 `input` 外，每个列表至少要写一种模态。
- **模态声明不做校验**：给实际不收图的模型错开图片，会一直失败在提供方那一侧。

**验证**：保存后用**新会话**发一句「只回复 custom provider connected，不要修改文件，也不要运行命令」，再发一张图。

**坑**

- `apiKeyEnv` 引用的是**环境变量名**；该变量必须在 **dsh 启动前**就存在，改完要重启 dsh。
- 模型配置**下一次请求生效**；已发过请求的旧会话保留日志里记录的模型与图片 ⇒ 换模型、补模态后都要**新建会话**验证。
- **`api` 保持 `openai-completions`**：它发的是 `{"type":"image_url","image_url":{"url":"data:image/png;base64,…"}}`，
  换成 `openai-responses` 则是 `input_image` + `detail`，多数 OpenAI 兼容网关不接受。
- Fetch 返回 401 = 凭据问题，手动填模型**绕不过**认证。
- 自定义 provider 的**模型名必须手填**（不像内置目录有清单）。

### 3.5 WorkBuddy 侧客户端

先说清一件事：**WorkBuddy 桌面端本身不是网关的消费方，而是网关的额度来源。** 网关把 WorkBuddy 账号的额度以兼容接口对外提供，桌面端不参与这条链路。

真正能接网关的是 WorkBuddy 产品族里的**开发者客户端**：

#### 3.5.1 CodeBuddy CLI / WorkBuddy（共用 `~/.codebuddy/` 配置体系）

**方式一：Anthropic 兼容（推荐，只改环境变量）**

```bash
export CODEBUDDY_BASE_URL="http://127.0.0.1:57891"   # Anthropic 兼容根地址
export CODEBUDDY_API_KEY="sk-wb-..."
export CODEBUDDY_MODEL="<从 /v1/models 复制的模型 id>"
# 可选
export CODEBUDDY_BIG_SLOW_MODEL="<模型 id>"
export CODEBUDDY_SMALL_FAST_MODEL="<模型 id>"
```

或写进 `~/.codebuddy/settings.json`：

```json
{
  "env": {
    "CODEBUDDY_BASE_URL": "http://127.0.0.1:57891",
    "CODEBUDDY_API_KEY": "sk-wb-...",
    "CODEBUDDY_MODEL": "<模型 id>"
  }
}
```

> ⚠️ **与本工具的「接入 CLI / 自动轮换」互斥。**
>
> 那两项会往**同一个** `settings.json` 写**上游 token**（`CODEBUDDY_AUTH_TOKEN` /
> `apiKeyHelper`）。你在这里手工写上网关地址后，若还开着自动轮换，**轮换每轮都会把网关
> Key 覆盖成上游 token** ⇒ CLI 把上游 token 发到本机网关 ⇒ 401，而且下一轮又改回去
> ⇒ **时好时坏**，看起来正像「API 密钥无效」。
>
> 两条路选一条：
>
> 1. **用界面切换**（推荐）：**设置 → CodeBuddy CLI** 一键切到「走本机网关」——
>    它会建 Key、写这三个 env、清掉上游凭据，并**关掉自动轮换**；
> 2. **手工写**（本文这段）：写完后请**关掉自动轮换**，并且不要再点「接入 CLI」。
>
> 从 v2026.10.27 起，本工具检测到 CLI 已指向本机网关时**会直接拒绝**写上游 token
> （错误文案会点名命中的地址），自动轮换也会跳过 —— 这是护栏，不是故障。

**方式二：OpenAI 兼容（`~/.codebuddy/models.json`）**

```json
{
  "models": [
    {
      "id": "<从 /v1/models 复制的模型 id>",
      "name": "Buddy Switch",
      "vendor": "OpenAI",
      "url": "http://127.0.0.1:57891/v1/chat/completions",
      "apiKey": "${BUDDY_SWITCH_KEY}",
      "maxInputTokens": 200000,
      "maxOutputTokens": 8192,
      "supportsToolCall": true,
      "supportsImages": false
    }
  ],
  "availableModels": ["<模型 id>"]
}
```

**验证**：`codebuddy --model <模型 id>` 发一句话；或会话内 `/model` 切换。

**坑**

- `models.json` 的 `url` **必须是完整路径、以 `/chat/completions` 结尾**。只填 `http://127.0.0.1:57891/v1` 这种裸基址**不工作** —— 这是该字段最常见的失败原因。
- `${变量名}` 引用不到环境变量时，占位符会**原样保留**并导致请求失败；启动前必须 `export`。
- `models.json` 支持热重载；环境变量不支持，**必须重启终端与 CLI**。
- 项目级 `<项目根>/.codebuddy/models.json` 优先级高于用户级。
- ⚠️ **与本工具的账号切换通道冲突**：本工具的「接入 CodeBuddy CLI」会往同一份 `~/.codebuddy/settings.json` 写 `env.CODEBUDDY_AUTH_TOKEN`（Windows）或 `apiKeyHelper`（macOS/Linux），「自动轮换」也依赖它。**要接网关就不要再开那条通道** —— 两者都在提供凭据，会互相覆盖。

> ⚠️ **诚实边界**：`CODEBUDDY_BASE_URL` 到底是「根地址」还是「含 `/v1` 的基址」，取决于客户端如何拼接 `/v1/messages`。先按根地址填；若报 404，改成 `http://127.0.0.1:57891/v1` 再试。以 CodeBuddy 官方文档为准。

#### 3.5.2 CodeBuddy CN IDE

桌面 IDE（VS Code 系，`www.codebuddy.cn`）。**本仓未核实它是否有自定义端点入口**。若其设置里提供自定义模型 / 自定义 Base URL，按 §3.5.1 的 OpenAI 或 Anthropic 规则填同样的值。

本仓能对它做的只是「客户端设置托管」（改它自己的 `settings.json`），字段语义以官方文档为准。

### 3.6 Trae 侧客户端

Trae IDE 与 SOLO 桌面端**支持自定义模型**（国内版 v3.3.51 起灰度、v4.0 起界面开放更多参数）。

**UI 路径**

- Trae IDE：**设置 → 模型 → 添加模型**（或对话框模型清单 →「添加模型」）
- SOLO 桌面端：左下角头像 → **设置 → 模型 → 添加模型**

> 同时装了 IDE 和 SOLO 时，IDE 的模型配置会**同步过来**。

**要填的值**

| 字段 | WorkBuddy 网关 | Trae 网关 |
|:---|:---|:---|
| 服务商 / API 格式 | `OpenAI` | `OpenAI` |
| 自定义请求地址 | `http://127.0.0.1:57891/v1/chat/completions` | `http://127.0.0.1:57894/v1/chat/completions` |
| 模型 ID | 从 `/v1/models` 复制 | 同左 |
| API Key | `sk-wb-...` | `sk-trae-...` |
| 多模态开关 | 按模型实际能力勾 | 同左 |

> 想走 Anthropic 协议也可以：API 格式选 `Anthropic`，地址填 `http://127.0.0.1:57891/v1/messages`（Trae 网关则 `http://127.0.0.1:57894/v1/messages`）。但**只有 Trae 网关需要 Bearer**，Trae IDE 的 Anthropic 模式发什么头本仓未核实，**优先用 OpenAI 协议**。

**坑（都是社区踩出来的）**

- **「自定义请求地址」是最容易错的一项。** 界面有「完整 URL」开关：
  - 关闭开关 ⇒ 填**基址**：`http://127.0.0.1:57891/v1`
  - 打开开关 ⇒ 填**完整地址**：`http://127.0.0.1:57891/v1/chat/completions`
  两者选错就是 404。**先确认开关状态，再决定填哪个。**
- **模型 ID 必须自己查**（从 `/v1/models` 复制）。填「模型展示名称」不生效 —— 展示名只是给你自己看的标签。
- 上下文窗口**不要往大了填**。填超过模型真实值会导致反复压缩，越聊越傻。
- 进阶配置里的「模型系列」模板、工具调用轮次保持默认即可。

> ⚠️ **诚实边界**：Trae 自定义模型的配置**不在** `settings.json` 里（`settings.json` 管的是编辑器偏好与代理）。本仓只托管 `settings.json`，**不托管自定义模型清单**。界面路径与字段以 Trae 官方为准。

---

## 4. 上游扩展：给网关加一个新的额度来源

### 4.1 先泼冷水

本仓的上游是**硬编码的两条产品线**（WorkBuddy / Trae），**没有插件式 provider 接口**。新增上游 = 一次完整的产品线改造，涉及 core + gateway + server + tauri + 前端 + 门禁。它**不是配置项**。

另外，**Hermes 和 dsh 不构成上游**：它们是**客户端**（消费方），本身不持有可被代理的模型额度。只有「有账号、有额度、有凭证」的东西才能当上游（Codex/ChatGPT 账号、Claude 账号、其它中转站账号等）。

### 4.2 架构分层与落点

| 层 | 职责 | 落点 |
|:---|:---|:---|
| ① 账号层 | 账号库、凭证形态、OAuth/刷新、区域/变体轴 | `core/modules/account.rs`、`auth_file.rs`、`oauth.rs`、`refresh.rs`、`region.rs`、`trae/variant.rs` |
| ② 选号层 | 候选过滤 + 四因子加权 + 成本分层 + 防惊群 + LRU | `gateway/pool/pick.rs`（**算法与产品线无关**）、`pool/entry_like.rs::PoolEntryLike`（**跨产品线契约**）、`pool/entry.rs`（WB 实现） |
| ③ 上游请求层 | wire 行为：强制 stream、`developer`→`system`、区域差异、错误分类 | `core/modules/upstream.rs`、`gateway/outbound/*` |
| ④ 协议层 | 入站协议 ↔ 上游 chat 的双向转换 | `gateway/protocol/{openai,anthropic,responses}.rs` |
| ⑤ 路由/产品线层 | 路由组装、路径所有权、独立监听 | `gateway/lib.rs::GatewayProduct` + `GatewayHost<P>` + `OWNED_PATHS`、`gateway/routes/*` |
| ⑥ 管理面 | 配置/Key/日志/状态 | 四处同步：`server/api.rs`、`src-tauri/commands.rs`、`src-tauri/lib.rs` 的 `generate_handler!`、`src/lib/api.ts` 的 `ROUTES`+`call()` |
| ⑦ 前端 | 页面、注册表、作用域 | `src/lib/scope.ts`、`src/lib/platforms/registry.ts`、`src/pages/*` |
| ⑧ 后台任务 | 周期性/写状态出口 | **两处** `BackgroundTask` 表：`server/main.rs` + `src-tauri/lib.rs` |

### 4.3 新增一条上游的完整清单

按顺序做，每步都能单独验证：

1. **定义轴**：新上游按什么分家（区域？程序位？账号类型？）。照 `region.rs`（WB 的 Region）或 `trae/variant.rs`（Trae 的 `TraeVariant`，6 个变体）的模式，写**全量遍历**的 `all_specs()`，别用 `match` 漏分支。
2. **账号库与凭证**：新的存储文件、信封/加密形态、刷新路径。凡是「取不到就报缺少」的地方，先问是不是**信封**（`is_envelope`）—— 判据分家：签到/积分/网关看 `access_token`，刷新/保活看 `refresh_token`。
3. **实现 `PoolEntryLike`**：新条目类型实现 `pool/entry_like.rs` 的全部方法。
   - ★ **中性降级**：拿不到的数据要表现得像「没有证据」，不是「证据指向坏结果」。无记录 ⇒ `success_ema` / `error_ema` 返回 `0.0`（返回 `1.0` 会让账号**永久选不上**）；无成本观测 ⇒ `CostTier::Unknown`；无在途 ⇒ `0`。
   - ★ **时间单位**：本 trait 全部时间入参是**毫秒**。内部用秒的实现**必须自己换算** —— 混用不报错，只会让冷却与紧迫度**静默错位**。
   - ★ `expiring_credits`（分批到期的绝对值）与 `expiry_urgency`（`[0,1]` 紧迫度）**别互相模拟**：把剩余时间塞进前者会被 `credits` 归一化抹平，语义静默丢失。
4. **定义 `PickPolicy`**：**默认值就是 WorkBuddy 的行为基线**（`expiry_urgency_weight = 0`、`deterministic = false`）。新上游要靠**覆写自己的 policy**（照 `trae_pick_policy()`）来改选号偏好，**绝不改默认值** —— 改默认值会**静默改变 WorkBuddy 的选号行为**。
5. **实现上游 wire**：新上游的端点、认证、流式格式、错误码分类（`UpstreamErrorKind`）。若与现有协议不同，先在 `core/modules/upstream.rs` 里加分支，**别在 gateway 里另写一套轮换逻辑**。
6. **协议转换**：如果上游不是 OpenAI chat 形状，在 `gateway/protocol/` 加转换模块；入站协议尽量复用现有的 `openai` / `anthropic` / `responses`。
7. **新 `GatewayProduct` impl + 独立端口**：照 `WbGateway` / `TraeGateway` 写 `OWNED_PATHS` / `shared_state` / `is_enabled` / `router` / `spawn_listener`。
   - ★ **路径重叠就必须独立端口**：axum 同路径 merge 会在**运行期 panic**（不是编译期）。新上游如果也挂 `/v1/models`、`/v1/chat/completions`（几乎一定），就**必须**新开端口。
8. **管理面四处同步**：`server/api.rs` 加 `.route` + handler、`src-tauri/commands.rs` 加命令、`src-tauri/lib.rs` 的 `generate_handler!` 加名字、`src/lib/api.ts` 的 `ROUTES` + `call()` 加条目。**改完跑 `npm run check:api`**（门禁 `scripts/check-api-contract.cjs`）。
   - ★ 单 `Value` 参数的命令必须传 `{ <参数名>: {…} }`。
9. **前端接线**：`src/lib/platforms/registry.ts` 登记新平台（**映射只在这一处**）；`src/lib/scope.ts` 的 `parseScope` **fail-closed**（非法值返回 `null`，**禁止 `?? DEFAULT_SCOPE`** —— 静默回落会写进错误的库）；跑 `npm run check:scope`。
10. **后台任务登记**：任何「写状态 / 周期性」的新出口，**先 grep 两侧注册表**（`server/main.rs` 与 `src-tauri/lib.rs` 各一份 `BackgroundTask` 表，是唯一事实来源）。同类事故已出过**三次**。
11. **模型目录与探测**：新上游的模型清单要走 `catalog_probe.rs` 的**唯一探测出口**，别新造探测逻辑。探测**只用该域自己的凭据**。
12. **禁用模型偏好**：两个出口（`/v1/models` 与调用路径）必须读**同一份** `disabled_models`（`is_disabled` 是唯一判断点）。

### 4.4 验收纪律（这部分不做，等于没做完）

- **证明「测试会红」，不是「测试是绿的」**：改回缺陷 ⇒ 护栏真红且逐字复现 ⇒ 还原 ⇒ `sha256` 一致。
- **断言的敌人不是「红」是「永远绿」**：跑在空数据上的断言「0 命中」恒真。**种真实数据 + 阳性对照**，每条断言问自己：删掉被测逻辑它会红吗？
- **别 grep 源码字符串当核验**：路由在不在册要**真的构建 router 并打一条请求**（本仓已因此放过一个「`cargo check` 全绿但网关起不来」的 bug）。
- **新增写盘/写状态出口**：先 grep 生产调用点，再 grep 两侧后台任务注册表。
- **「改文件 → 跑检查 → 还原」一律二进制读写 + `sha256` 复核**：文本写回会 LF↔CRLF 翻转，`core.autocrlf` 让 `git diff` 看不见 ⇒ 静默损坏。

---

## 附录 A：端口与路径速查

| 项 | 值 |
|:---|:---|
| `5789x` 一段的分工 | 57890 界面 · 57891 WB 网关 · 57892 统一入口 · 57893 hook · **57894 Trae 网关** · **57895 Qoder 网关** |
| WorkBuddy 网关 | `127.0.0.1:57891` |
| Trae 网关 | `127.0.0.1:57894`（原 `7864`） |
| Qoder 网关 | `127.0.0.1:57895`（原 `7865`） |
| **统一入口（第四个入口，默认关）** | `127.0.0.1:57892` |
| Trae 本地 MITM 代理（登录态捕获用，**与网关无关**） | `8899` |
| 外部 Agent hook 接收端（P15，**只绑回环**，客户端 hook 往这里 POST） | `127.0.0.1:57893` |
| webui（界面本体） | `127.0.0.1:57890` |
| 本工具数据根 | `~/.buddy-switch/`（可用 `BUDDY_SWITCH_HOME` 覆盖，值必须是**已存在的绝对目录**） |
| WB 账号库 | `~/.buddy-switch/accounts.json` / `accounts.global.json` |
| Trae 账号库 | `~/.buddy-switch/trae/checkin_accounts.json` / `.global.json` |
| Qoder 账号库 | `~/.buddy-switch/qoder/accounts.cn.json` / `accounts.global.json` |
| WB 网关 Key / 配置 | `~/.buddy-switch/gateway_keys.json` / `gateway_config.json` |
| Trae 网关 Key / 配置 | `~/.buddy-switch/trae/api_gateway_keys.json` / `api_gateway.json` |
| Qoder 网关 Key / 配置 | `~/.buddy-switch/qoder/gateway_keys.json` / `gateway_config.json` |
| 统一入口 Key / 配置 | `~/.buddy-switch/gateway_unified_keys.json` / `gateway_unified.json` |

## 附录 B：已知偏差（界面接入指引的 Base URL）

**现象**：「API 服务」页 →「接入指引」里，**OpenAI 协议**的片段（Codex / opencode / 通用）给出的 Base URL **缺 `/v1`**；Anthropic 协议（Claude Code）的片段是对的。

**根因**：后端 `GatewayStatusView.base_url` 的值是 `http://host:port`（**不含 `/v1`**，见 `gateway/state.rs` 的 `GatewayConfig::base_url()` 与其单测）；而前端 `resolveGatewayBaseUrl` 的**兜底分支**返回的是 `http://host:port/v1`。主路径走后端值 ⇒ 拿到不带 `/v1` 的串。接入指引的 `rootUrl = baseUrl.replace(/\/v1\/?$/,"")` 因此没做任何剥离，OpenAI 片段就少了 `/v1`。

**影响**：照抄界面片段配置 Codex / opencode / 通用 OpenAI 客户端 ⇒ 请求打到 `/chat/completions`（少一层 `/v1`）⇒ **404**。Anthropic 系不受影响。

**本文怎么处理**：本文所有片段一律按**正确值**给出（OpenAI 系带 `/v1`，Anthropic 系不带）。

**建议修法**（择一，属代码改动）：
- 让 `GatewayConfig::base_url()` 返回带 `/v1` 的值，并同步 `resolveGatewayBaseUrl` 的兜底分支与 `state.rs` 的单测；或
- 让两个接入指引按协议自行拼 `/v1`，与 `resolveGatewayBaseUrl` 的语义解耦。

> 注意：现有门禁 `scripts/check-scope-registry.cjs` 的夹具用的是 `baseUrl: "http://127.0.0.1:8080/v1"`（**带 `/v1`**），所以这个偏差在门禁下**看不出来** —— 改的时候记得把夹具也一起改成后端真实值，否则修完仍然是绿的。

## 附录 C：安全边界

- 网关默认**只监听 `127.0.0.1`**。改成 `0.0.0.0` 需要二次风险确认 —— 局域网内任何设备都能消耗你的额度。
- API Key **明文只在创建时返回一次**；库里只有 `sha256` + 前缀 4 位 + 尾 4 位，**不可逆推**。列表里的 `••••` 不是 Key。
- Key 与 region / variant **强绑定**，客户端无法自行越域（跨域前缀默认返回 400，而非静默改名）。
- Trae 网关**没有任何可用 Key 时拒绝一切请求**（不像某些实现「留空即不鉴权」）—— 任何人猜到 57894 端口也白嫖不到。
- 本工具的数据全部在 `~/.buddy-switch/` 下，不上传第三方。

---

## 参考

- 仓库内：`README.md`「网关」章节、`docs/history/overvhistory/overview.md`、`src/lib/gateway-snippets.ts`（接入片段的唯一来源）、`crates/buddy-switch-gateway/src/protocol/responses.rs`（Responses 支持子集）、`docs/specs/PLAN.md` §3.18/§3.19/§3.20、`docs/specs/UNIFIED-GATEWAY-ARCH.md`（统一入口的架构与决策）、`docs/specs/UNIFIED-GATEWAY-UI.md`（统一入口的交互规格）。
- 外部客户端配置：一律以各自官方文档为准（Codex `config.toml`、Claude Code 环境变量、Hermes Agent 配置、DeepSeek Harness 模型配置、Trae 自定义模型、CodeBuddy CLI `settings.json` / `models.json`）。
