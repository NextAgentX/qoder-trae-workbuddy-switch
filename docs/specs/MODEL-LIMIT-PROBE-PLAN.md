# 账号池 × 模型 限流探测 —— 完整实施方案 + 交互方案

> 状态：**现行（P1–P3 已实现；§5.9 = 账号页联动，2026-10-22 落地）**。初稿日期：2026-10-16。
> 需求原话：「出一套方案可以测试账号池中每个账号的模型列表的模型是否被限流」
> 已拍板：**① 被动表 + 主动探测都要　② 三个平台全做　③ 探测结果回填池冷却**
> 依据：**本仓源码实测**（行号可核实）；未核实处显式标注。

---

## 0. 一句话结论 + 必须知道的一个坑

> ✅ **交互原型已可点**：`docs/prototype/buddy-switch-prototype.html` → 网关页 → 模型 → **限流探测**。
> 先玩一遍，再决定做不做（`node docs/prototype/verify-model-probe.mjs` 会真跑一遍并出截图）。详见 §5.8。

**做法**：对「账号 × 模型」矩阵逐格发一次**最小 chat 请求**，用**三平台各自既有的错误分类函数**判读结果，
矩阵落盘 + **折回模型列表 + 卡内矩阵**呈现；结果可选择**回填**进账号池冷却。

**★ 坑（先看这个再读下去）**：**Trae 池目前没有「模型级冷却」这个概念** ——
`TraePoolEntry` 只有账号级 `cooldown_until` / `cooldown_reason`，`apply_error(uid, kind, reason)` **连 model 参数都没有**
（`gateway/src/trae/pool.rs:381`）。而 WB（`entry.rs:385 cooldown_model`）与 Qoder（`qoder/pool.rs:364 model_cooldowns`）都有。

⇒ **「三平台全做 + 回填」在 Trae 上会遇到语义冲突**：Trae 探测到「模型 M 被限流」却只能回填**账号级**冷却，
会把该账号的**其它模型一起打死**（过度冷却）。两条出路，见 §4.5.4，**需要你选**。

---

## 1. 目标与口径

### 1.1 「账号的模型列表」＝ 每账号一份

| 平台 | 清单来源 | 粒度 |
|:--|:--|:--|
| WB | `upstream.fetch_models(region, acc)`（`core/upstream.rs:596`） | **按账号**（上游可能对不同账号下发不同清单） |
| Qoder | `qoder::api::fetch_model_list(region, input)`（`qoder/api.rs:1433`）；CLI 侧缓存 `cli_models_cache_dir(region, uid)`（`qoder/paths.rs:136`） | 目录按区域；**CLI 缓存按 uid** |
| Trae | `trae::model_list::read_client_model_list(variant)`（`trae/model_list.rs:105`） | **按变体**（读客户端 `state.vscdb` 里上游下发的缓存，**所有账号共用**） |

⇒ 矩阵的**列集合**：WB/Qoder 取「该账号自己拉的清单」的并集（缺的格 = 该账号无此模型）；Trae 全行同列。

### 1.2 「被限流」＝ 严格三档，**其余一律不叫限流**

| 状态 | WB 判据（`pool/mod.rs:359 classify_event`） | Qoder 判据（`qoder/pool.rs:62`） | Trae 判据（`trae/pool.rs:81/96`） |
|:--|:--|:--|:--|
| `rate_limited_model`（**模型级限流**） | `SoftRate{model_scoped:true}`（429 + `6004`/`IsModelRateLimit`） | `QoderErrKind::RateLimited`（429，上游本来就按模型报） | `SoftRate`（429 / `4008` / 含 `quota`/`exceeded`/`rate`） |
| `rate_limited_account`（账号级限流） | `SoftRate{model_scoped:false}` | —（Qoder 无此细分，一律按模型） | —（同上，无法细分） |
| `model_blocked`（模型封禁） | `ModelBlocked`（`11102`） | —（Qoder 无此码） | — |
| `no_credits` | `HardCredit`（402 / 余额文案） | `Transient`？→ 需按 body 细分（未核实） | `PlanLimit`（`1005` / 含 `plan`） |
| `auth_dead` | `SessionDead`（`12153`）/**`AuthRejected`（401/403，issue #100）** | `AuthDead`（401/403） | `SessionDead`（401，永久禁用） |
| `model_unavailable` | `NotFound`（404） | `ClientError` | `NotFound`（404） |
| `upstream_error` | `Server` / `Client` | `Transient`（5xx） | `Server` / `Client`（5xx/4xx 区间） |
| `upstream_queued`（**上游排队**，2026-10-22 新增） | —（WB 无此形态） | `QoderErrKind::Queued`（`10605` / `isQueued`，裹在 **403 信封**里） | —（Trae 侧并入 `rate_limited_model`） |
| `transport_error` | `status == 0` | —（连接失败） | —（连接失败） |
| `ok` | 2xx | 2xx | 2xx |
| `skipped` | 信封凭据 / 未登录 / 不在库 | 无 access_token（PAT 不可推理） | 无 jwt / 无 device_id |

> ★★ **Qoder 的 2xx ≠ 可用**（2026-10-22 真机取证）：上游的**排队**信号是
> **HTTP 200 + SSE 信封里的错误帧**（`statusCodeValue:403` + `10605/isQueued`）。
> 只看 HTTP 状态 ⇒ 排队被报成「全部可用（上游 200）」（假阴性），而真实流量拿到的是排队错误。
> ⇒ Qoder 探测**必须读一次流**（`qoder::sse::first_upstream_error`）取第一帧错误，
> 判档用**信封里的状态码 + 错误体**（`classify_http_with_detail`），不看 HTTP 状态。
>
> ★ `upstream_queued` **独立成档**（不并入限流三档，也不并入 `auth_dead`）：
> 上游明说 `modelKey` + `serviceAvailable:false` + `retryAfterSeconds` ⇒ 等 30 秒就好，
> 不是被限流、也不是凭据失效。回填语义 = `Other`（**不写冷却**），与中继侧
> `pool::apply_error` 对 `Queued` 的处理一致（既不冷却也不标死）。
>
> ★ 账本状态码同步：客户端拿到的是排队错误帧（`code:429`）时，**账本也记 429**，
> 不记 502 —— 否则日志把「上游在排队、稍后重试」说成「网关挂了」。

> ★★ **红线**：只有前三行能叫「限流」。把 402（没积分）报成「限流」会让用户去傻等解锁 ——
> `client_limits.rs` 已经踩过这个坑（「上游 6004 文案的 `reset at <T>` 是**配额窗口重置**，不是解锁时刻，实测 6/6 判错」）。
> **主动探测的价值就是消除这个歧义：结果 = 此刻的实证。**
>
> ★ 同理：**排队**（`upstream_queued`）也不许并进「限流」—— 排队等 30 秒就好，
> 报成限流会让用户去傻等解锁；也不许并进 `auth_dead` —— 账号好着。

### 1.3 与既有能力的边界（**不重造轮子**）

| 已有 | 位置 | 覆盖 | 缺口（本方案补的） |
|:--|:--|:--|:--|
| 客户端限流台账 | `core/client_limits.rs` + `src/components/client-limits-card.tsx` | 账号×模型×恢复时刻（**日志反推**） | 没跑过的模型 = **不在台账 ≠ 没被限流** |
| WB 池模型级冷却 | `pool.snapshot()[].rate_limited_models`（`pool/mod.rs:1038`） | 经网关真实流量打出来的冷却 | 同上：没打过 = 未知 |
| Qoder 池模型冷却 | `qoder::pool::status_list()[].modelCooldowns`（`qoder/pool.rs:392`） | 同上 | 同上 |
| Trae | —（**无模型级视图**） | — | 全无 |

⇒ 本方案 = **① 把三张被动表在 UI 上拼成一张底色矩阵** + **② 主动探测把「未知」变成实判**。

---

## 2. 架构

```
                        ┌──────────────── 被动（0 请求）────────────────┐
  pool.snapshot()       │ WB rate_limited_models                        │
  qoder status_list()   │ Qoder modelCooldowns                          ├─► 底色矩阵
  client_limits         │ 客户端台账（账号×模型×恢复时刻）                  │
                        └───────────────────────────────────────────────┘
                        ┌──────────────── 主动（逐格探测）──────────────┐
  [scope] ─► ① 展开矩阵  │ 账号(池) × 模型(每账号清单)                     │
           ─► ② 逐格探测  │ 单账号·单次·零重试 · 直连上游（绕开换号轮转）      ├─► 实测矩阵
           ─► ③ 判读     │ 各平台既有 classify_*（零新判据）               │
                        └───────────────────────────────────────────────┘
                                     │
                                     ▼ ④ 回填（可选开关，默认开）
                        pool.adopt_probe(...)  ──► 池冷却（供真实流量避让）
```

**两条红线（全程不得破）**：

1. **探测绝不复用「会换号」的中继** —— WB 的 `routes/relay.rs`、Qoder 的 `chat_for_region` 的 `max_rotate` 循环、
   Trae 的 `chat_for_variant` 同类循环，**都会自动换号**。用它们探测，结果会**归因到别的账号**上，矩阵立刻变假数据。
   ⇒ 探测必须**单账号、单次、零重试**，直接调各平台的上游出站函数。
2. **探测本身不写池治理状态** —— 除了 §4.5 的**显式回填**那一步（一个统一出口），探测路径上绝不调
   `apply_upstream_error` / `apply_error`。否则探测自己把号打成冷却 = **自我实现的限流**，还会把退避越探越长。

---

## 3. 后端实施方案

### 3.1 新模块：`crates/buddy-switch-gateway/src/model_probe.rs`

```
pub enum ProbeScope { Wb(Region), Qoder(QoderRegion), Trae(Traevariant) }

pub struct ProbeJob {
    pub scope: ProbeScope,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub total: usize,          // 总格数
    pub done: usize,           // 已探格数
    pub current: Option<String>, // 「账号 / 模型」当前探测点（进度文案）
    pub phase: ProbePhase,      // Idle | Running | Done | Cancelled | Failed
    pub cells: Vec<ProbeCell>,
    pub truncated: bool,
    pub adopted: usize,         // 回填了几条
    pub error: Option<String>,
}

pub struct ProbeCell {
    pub uid, pub nickname, pub model,
    pub state: ProbeState,      // §1.2 的枚举
    pub http_status: u16,
    pub reset_at_ms: Option<i64>,
    pub reason: String,         // 上游原文（截断）
    pub latency_ms: i64,
    pub list_source: &'static str, // client-cache | upstream | catalog-fallback
}
```

**作业状态放哪**：**两宿主各一个 `static PROBE_JOB: Mutex<Option<ProbeJob>>`**，
与既有 `SWITCH_PROGRESS: Mutex<Option<String>>` **同款**（`server/api.rs:76`、`src-tauri/commands.rs:127`）。
一次只允许一个探测在跑（第二次调用返回「已有探测进行中」）。

### 3.2 三平台适配（一个 trait 三实现，把差异收敛在一处）

```
trait ProbeTarget {
    fn accounts(&self) -> Vec<ProbeAccount>;              // 从各自的池派生
    async fn list_models(&self, acc) -> (Vec<String>, &'static str);
    async fn probe_once(&self, acc, model) -> RawOutcome;  // 直连上游，单账号单次
    fn classify(outcome) -> ProbeState;                    // 各平台既有 classify_*
}
```

| | WB 实现 | Qoder 实现 | Trae 实现 |
|:--|:--|:--|:--|
| `accounts()` | `pool.sync_accounts` 后取该 realm 的 uid | `pools[region].sync_for(region)` 后取条目 | `TraePool::for_variant(v).sync_for(v)` 条目 |
| `list_models` | `upstream.fetch_models(region, acc)` | `qoder::api::fetch_model_list` + CLI 缓存 | `read_client_model_list(variant)` |
| `probe_once` | ①`outbound::prepare_outbound_body` 构造最小体 ②`upstream.chat_stream_with(region, acc, body, None, Some(&extra))`，`extra` = `session_headers::conversation_headers` + `device_headers(uid)` | ①`api::build_chat_body` → `cosy::qoder_encode` ②`cosy::shared_cache().get(&CosyAccountInput{..})` 取签名 session ③`session.headers(&encoded, PATH_CHAT_SSE, Some(&meta.key), true)` ④ POST `{host}{PATH_CHAT_SSE}` | ①`payload::prepare_llm_chat_body(src, variant, model, uid, device_id, machine_id)` ② POST `{TRAE_LLM_CHAT_PATH}`（带 jwt/device 头） |
| `classify` | `pool::classify_event(status, body)` | `qoder::pool::classify_http(status)`（Qoder 无 body 判据） | `trae::pool::classify_http(status)` → `classify_solo(code,msg)` |

★ **探测请求的公共约定**：
- body 最小化：`{ "model": M, "stream": true, "max_tokens": 1, "messages": [{"role":"user","content": PROBE_TEXT}] }`，
  `PROBE_TEXT` = 固定中性短句（如 `"ping"`），避免触发上游内容审核。
- **2xx ⇒ 立刻丢弃响应体**（不聚合、不落账本、不写日志），只留 `latency_ms`。
  非 2xx ⇒ 读体（≤4KB，与 `ERROR_BODY_LIMIT` 一致）交 `classify_*`。
- **信封凭据预拦**（WB `account::is_envelope`；Qoder 无 `access_token`；Trae 无 jwt）⇒ `skipped`，**绝不发空 Bearer**。

### 3.3 调度：并发 / 节流 / 取消 / 上限

| 项 | 取值 | 理由 |
|:--|:--|:--|
| 账号内 | **串行**（同账号的多个模型一次一个） | 连发会叠加上游频率限制，把好号探成 429 |
| 跨账号并发 | 默认 **2**（配置项 `probe_concurrency`） | 保守起步 |
| 请求间隔 | 每格之间 ≥ `probe_gap_ms`（默认 150ms） | 复用 `min_pick_gap_ms`（默认 100）的思路 |
| 重试 | **0 次** | 重试只会放大限流，并让「第几次」的退避语义混乱 |
| 格数上限 | 默认 **400**（超出 `truncated: true`） | 20 账号 × 20 模型 |
| 取消 | `AtomicBool` 取消位，逐格检查；用户关面板/切 scope 即停 | 400 格可能要几分钟 |
| 超时 | 单格 30s；整作业 10min | 防挂死 |

★ **探测自身 429 是信号不是失败**：如实记成 `rate_limited_*`，**绝不自动重试**。

### 3.4 命令面（★ 走「作业 + 轮询」，与 `switch_progress` 同款，零新基建）

**依据**：本仓**没有** EventSource/SSE 前端消费（`grep EventSource src/` 零命中），
但已有成熟的**长任务 + 进度轮询**先例：`switch_progress`（`server/api.rs:1780` / `src-tauri/commands.rs:1347`，前端 `switch-account-dialog.tsx:191` 轮询）。
本方案**照抄这个形状**，不引入新传输。

| 命令 | 桌面（`src-tauri/commands.rs`） | 服务端（`server/api.rs`） | 返回 |
|:--|:--|:--|:--|
| 启动探测 | `probe_models_start(scope, uids?, models?, adopt)` | `POST /api/{wb\|qoder\|trae}/models/probe` | `{ started, total }` |
| 查进度 | `probe_models_status()` | `GET /api/{...}/models/probe/status` | §3.1 的 `ProbeJob`（含 `cells`） |
| 取消 | `probe_models_cancel()` | `POST /api/{...}/models/probe/cancel` | `{ cancelled }` |
| 上次结果（可选） | — | `GET /api/{...}/models/probe/last` | 落盘的最近一次报告 |

**★ 硬约束**：
- 命令**同步四处**：`ROUTES` + 前端 `call()` / `generate_handler![]` / server `.route(...)` / 命令本体。
- 跑门禁 `scripts/check-api-contract.cjs`（新命令必须登记）。
- **一区一 Key ⇒ 命令必须参数化**，计入 `check-scope-registry.cjs` 的 `SCOPE_PARAMETERIZED` 计数。
- 桌面端可**顺带 `emit("model-probe-progress", ...)`** 事件（与 `switch-progress` / `trae-checkin-progress` 同款），
  但前端**以轮询为准**（事件只是加速）。
- ★ **不新增周期性后台任务**。一次性、用户触发即可 ——
  「后台任务登记表」两宿主各一份、已出过四次漏登记事故（`core/background.rs` + `server/main.rs` + `src-tauri/lib.rs`）。
  若将来要「定时探测」，那是**另一个决定**，必须同时改两宿主注册表。

### 3.5 落盘

- `~/.buddy-switch/model-probe.<scope>.json`（原子写，复用 `core::config::atomic_write`）。
- **TTL 默认 10 分钟**（探测是「此刻」的快照，过期标 `stale`；前端灰显）。
- 与 `gateway_disabled_models.json` / `gateway_strategy.json` 同目录（`region::gateway_config_file().with_file_name(...)`）。

---

## 4.5 回填（★ 本方案最危险的一节，请细看）

### 4.5.1 为什么危险

探测是**我们主动打**的请求。如果「探到 429 就回填冷却」，会出三件事：

1. **自我造限流**：我们为探 N 个模型连发 N 个请求，本身就可能把账号推到 429；回填 = 用自己造成的限流惩罚自己。
2. **退避被污染**：WB 的 `soft_streak` / Trae 的 `cooldown_of` 都是「第几次被罚」的指数退避；
   探测次数混进去会让退避越来越长。
3. **Trae 语义冲突**：Trae 只有账号级冷却 ⇒ 模型级限流回填会**误伤同账号其它模型**。

### 4.5.2 因此回填必须走**新出口**，且**不推进退避指数**

**不复用** `Pool::apply_upstream_error`（WB，`pool/mod.rs:784`，内部 `note_error`+`maybe_trip_breaker`+推进 `soft_streak`）
与 `QoderPool::apply_error`（`qoder/pool.rs:353`，内部推 EMA）。

**新增** `adopt_probe_result(...)`（每平台一份），语义 = **「按事实置冷却，但不动历史/退避/健康度」**：

| 探测到的状态 | 回填动作 | 后端调用 |
|:--|:--|:--|
| `rate_limited_model` | 模型级冷却（`reset_at_ms` 来自 WB `classify_event` 解析；无则 `now + 60s`） | WB `entry.cooldown_model(...)`；Qoder `entry.model_cooldowns.insert(model, now+60_000)` |
| `model_blocked` | 模型级退避（6h→12h→24h） | WB `entry.block_model_backoff(...)`（**`hits` 由真实封禁次数决定，探测不 +1** → 需拆出「置冷却但不加 hits」的路径） |
| `rate_limited_account` | 账号级软冷却（用上游墙钟，封顶 `soft_rate_max`） | WB `cooldown_soft_until(...)`（**不**走 `cooldown_soft_backoff`，不推进 `soft_streak`） |
| `no_credits` | 账号级硬冷却至次日 04:00 | WB `cooldown_until_tomorrow_4am(...)` |
| `auth_dead` | **建议不回填**（只标状态）。理由：探测用错凭据/上游抽风会**永久禁用**好号（Trae 的 `SessionDead` 是 `-1` 永久） | — |
| `ok` | ★ **清掉该模型的冷却**（2026-10-16 用户拍板）—— 探到可用就该恢复，别让它一直挂着 | WB：从 `model_cooldowns` 删掉该 `model`；Qoder：从 `model_cooldowns` 删掉该 `model` |
| 其它 | 不回填 | — |

★ 每平台都需要**新增**这样一个方法（WB `Pool`、Qoder `QoderPool`；Trae 见 §4.5.3）。
**必须配一条单测**：回填后 `soft_streak` / `fails` / `retry_count` / `success_ema` / `error_ema` **逐字段不变**（变异：改成推进 ⇒ 红）。

### 4.5.3 回填时机（**别边探边填**）

- 探测**全程只读**；结束后**一次性汇总回填**（`adopted` 计数写进报告）。
- 这样「中途取消」不会留下半套污染。
- 回填开关：`adopt: bool`（入参），UI 默认**开**（用户已选），可关。

### 4.5.4 Trae 的取舍（**需你拍板**）

Trae 现在没有模型级冷却。三选一：

| 选项 | 做法 | 代价 | 评价 |
|:--|:--|:--|:--|
| **T1（推荐）** | **Trae 只探测、不回填** | 0 | 最安全。Trae 的模型级限流只呈现给用户看，不影响路由 |
| T2 | 先给 Trae 补一套**模型级冷却**（`TraePoolEntry.model_cooldowns` + `apply_error(model)` + `cooldown_of` + **落盘格式** + 签到页展示 + `credits::save_cooldown_for` 兼容） | 中（要动落盘契约与签到页共用文件） | 三平台语义齐平，但改动面明显变大 |
| T3 | Trae 回填**账号级**冷却 | 小 | ❌ **不建议**：模型级限流会把该账号其它模型一起打死 |

> 我的建议：**一期 Trae 采 T1**；T2 作为独立改造单独立项（它本身就值得做，但别塞进探测方案里）。

---

## 5. 交互方案（UI/UX）

### 5.1 入口与信息架构 —— ★ **一张卡片，不是弹层**

**形态：网关页 → 「模型」分区里的一张普通 `.card`**（折叠 ⇄ 展开），**不是模态弹层**。

> ★★ 2026-10-16 改版理由（第一版做成弹层，用户原话「和狗屎一样」）：
> 1. **矩阵天生要宽度** —— 塞进 720px 的弹层必然挤成一团（行列汇总 + 图例根本没地方放）；
> 2. **多余的壳** —— 「对话框里再套一张表」多一层层级，用户在网关页做的判断被遮罩切断；
> 3. **选择控件本来就该在页面里** —— 没必要悬空到遮罩上再收回来。
> ⇒ 现在：卡内切「选择 / 进行中 / 结果」三态；宽度吃满 `split-main`（1440 下约 950px）。

```
模型分区
[ 来源条：实时 / 已保存 / 内置 · 上次识别 xx:xx · 原因 ]                        [ 重新识别 ]
┌ 限流探测  [账号 × 模型 实探]                        4 账号 · 4 模型可测   [ 展开 ▾ ] ┐   ← 折叠态：一行
└──────────────────────────────────────────────────────────────────────────────────────┘
[ 模型表格 ]
```

### 5.2 卡内三态

★★ **两条用户口径（2026-10-16 拍板）**：

1. **不替用户选模型** —— 模型**默认空选**，「开始探测」置灰并提示「请先选择要测试的模型」。
   全选等于默认把整个清单打一遍：又慢又费积分，而用户本来只想知道**某几个**模型的情况。
   （账号相反：**默认全选** —— 要回答的就是「每个账号」。）
2. **`ok` 会清掉该模型已有的冷却**；回填**按模型**增删（见 §4.5）。

```
┌ 限流探测  [账号 × 模型 实探]                        4 账号 · 4 模型可测   [ 收起 ▴ ] ┐
│                                                                                     │
│  ① 要测哪些模型                                       已选 2/4      [ 全选 ]        │
│  ┌────────────────────┬────────────────────┬────────────────────┬────────────────┐  │
│  │ ☑ claude-sonnet-4.5│ ☑ claude-opus-4.1  │ ☐ gpt-5-codex      │ ☐ gemini-2.5-pro│  │ ← 默认**都不选**
│  └────────────────────┴────────────────────┴────────────────────┴────────────────┘  │
│                                                                                     │
│  ② 在这些账号上测                                     已选 4/4      [ 清空 ]        │
│  ┌────────────────────┬────────────────────┬────────────────────┬────────────────┐  │
│  │ ☑ jackey           │ ☑ team-shared      │ ☑ qa-bot           │ ☑ design-lab    │  │ ← 默认全选
│  └────────────────────┴────────────────────┴────────────────────┴────────────────┘  │
│                                                                                     │
│  ┌───────────────────────────────────────────────────────────────────────┐          │
│  │ 把结果回填账号池                                          [ ● ON ]    │          │
│  │ 可用 ⇒ **清掉**该模型的冷却；限流／封禁 ⇒ **置**该模型的冷却。          │          │
│  │ 只动冷却，不推进退避计数。                                             │          │
│  └───────────────────────────────────────────────────────────────────────┘          │
│  ───────────────────────────────────────────────────────────────────────────────    │
│  4 账号 × 2 模型 = **8** 格 · 消耗极少积分                          [ ✦ 开始探测 ]   │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

> ★ 选择控件用**复选框行**（`.pick` + `.pick-grid`，`minmax(196px,1fr)` 自适应列数），
> **不用 chip 云**：chip 云没有左对齐基线，20+ 个模型会糊成一团（第一版就是那样）。

**进行中**（同一个卡内切换）：

```
┌ 限流探测  [账号 × 模型 实探]                        上次 15:42 · 回填 +1/-0  [ 收起 ▴ ] ┐
│  ▶ 5 / 8                                        正在探测：jackey · gpt-5-codex       │
│  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  │
│  ● 模型限流 1   ● 账号限流 1   ● 积分不足 1   ● 可用 4                                │ ← **一行**统计
│  账号内串行 · 跨账号并发 2 · 格间 150ms · 零重试。探测自身 429 是信号不是失败。         │
│  ───────────────────────────────────────────────────────────────────────────────    │
│  结束后自动回填池冷却                                              [ 取消剩余 ]        │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

> ★ 统计是**一行**（`.pstats`），不是一屏 tile —— 第一版 12 个大格子才是「乱」的主因；
> 行列汇总本来就在矩阵的边缘，不需要再复述一遍。

**完成** ⇒ 卡内直接出矩阵（§5.3）。

### 5.3 结果矩阵（**卡内**，吃满 `split-main`）

```
┌ 限流探测  [账号 × 模型 实探]                        上次 15:42 · 回填 +1/-0  [ 收起 ▴ ] ┐
│  ✓ 共 8 格   已完成 · 回填 置 1 / 清 0                                              │
│  图例  ● 可用  ● 模型限流  ● 账号限流  ● 积分不足  ● 跳过  ┄ 未探测  ●被动已知        │
├────────────┬───────────────────────────────────────────────────────────────────────┤
│ 账号 \ 模型 │ claude-sonnet-4.5  claude-opus-4.1  gpt-5-codex  …         限流数       │
├────────────┼───────────────────────────────────────────────────────────────────────┤
│ 张小明      │  ✅   ✅   🟠        ✅        ✅              1           │
│ 李四        │  ✅   ✅   ✅        ✅        🔴              1           │
│ 王五        │  ✅   ✅   ❓        ❓        ✅              0           │
│ 赵六        │  ⏭   ⏭   ⏭        ⏭        ⏭               —           │
├────────────┼───────────────────────────────────────────────────────────┤
│ 限流账号数   │  0    0    1         0         1                           │
└────────────┴───────────────────────────────────────────────────────────┘
   🟠 hover → tooltip：glm-5.3 · 模型限流 · HTTP 429 · 6004 · 重置 2026-10-17 14:18:44
   ⏭ hover → tooltip：未探测（该账号登录态失效 / 无凭据）
```

**交互细则**：
- **单元格颜色 + 图标双编码**（不能只靠颜色，色盲可用；也满足无障碍）。
- **hover 高亮整行 + 整列**；点击单元格 → 右侧详情气泡（状态 / HTTP / 上游原文 / `resetAt` / 耗时）。
- **行列边缘有汇总**（每账号限流数、每模型限流账号数），可点表头排序（默认按「限流数」降序）。
- **被动底色**：未探测过的格显示**灰色 `❓未知`**；池/台账里已有记录的格显示对应色 + 角标
  `真实流量` / `客户端日志`，与探测结果**视觉区分**（角标 + 浅色底）。
- **空态**：全部 `✅` ⇒ 「全部可用」；全部 `⏭` ⇒ 走「去登录/导入账号」指引（复用 `NoCredentialCause` 的分档文案）。
- **取消/失败**：面板保留已探到的格，标 `部分结果`，并给「重探」。
- **回填反馈**：完成条上显示「已回填 3 条冷却」+ 可展开看明细。
- ★ **明示**（恒定可见）：「本探测会向每个账号各发一次最小请求，消耗极少积分」——不能让用户以为这是纯本地操作。

### 5.4 ★ 结果**折回模型列表**（模型表加一列状态）

用户口径（2026-10-16）：「**模型列表也需要更新状态**」。

探测结果不能只活在卡片里 —— 「模型」分区那张**模型表**才是用户看模型的地方，
所以每行加一格 **「限流（探测）」**，直接回答「这个模型在我的账号里被限流了几个」：

| 该模型的状态 | 单元格 | 色 |
|:--|:--|:--|
| 本次实测过，0 个账号限流 | `全部可用` | ok |
| 本次实测过，部分限流 | `N/M 限流`（M = 实测覆盖的账号数） | warn |
| 本次实测过，全被限流 | `M/M 限流` | bad |
| 只在真实流量／客户端台账里见过 | `被动 N` | 默认 |
| 没探过、也没被动记录 | `—`（灰） | muted |
| 该模型这个区域不可用 | `—`（灰） | muted |

- **点徽章 = 下钻**：展开卡片并把范围**收窄到这一个模型**（done 态直接摊开矩阵；
  setup 态则只勾中它，等用户点「开始探测」）。⇒ 模型表与矩阵**双向可达**。
- ★ 与卡片读**同一份数据**（`p.cells` / `p.passive`），**不另存一份** —— 两处各自维护必然漂移。
- ⚠️ 真实实现里模型身份是 **`(作用域, 模型 id)`**（同名模型跨区域各有一条，见
  `MODEL-IDENTITY-ARCH.md`）；原型按 `name` 取（「模型」分区里清单已按 name 去重），
  落 React 时这条列必须按 scope 收窄。

### 5.5 状态机

```
 idle ──开始──► running ──完成──► done ──(TTL 10min)──► stale
                  │
                  ├──取消──► cancelled（保留部分结果）
                  └──失败──► failed（给原因 + 重试）
```

> ★ `done` 态下**改动选择**（下钻/加选模型）不会回到 `running`：矩阵按当前选择过滤，
> 头部的「共 N 格」**随选择收缩**（下钻后写「共 2 格」，不能还挂着上次的 6）。

### 5.6 文案与 i18n

- zh + en **双写**（词条按平台域分：`wbApi` / `qoderApi` / `traeApi`，或共用 `shared.gateway.probe.*`）。
- 术语：**探测结果是「限流」，池内治理动作是「冷却」** —— 两者别混用（池卡上已有「模型冷却」一词，指治理态）。
- 关键文案必须**说明「所以我能怎么办」**：
  - 🟠「模型被限流：换其它模型可立即使用，或等上游重置（14:18）」
  - 🔴「账号被限流：该账号所有模型暂不可用」
  - 🚫「模型被上游封禁：换模型；反复封禁会逐次加长」
  - 💰「积分不足：去「账号」页看余额」
  - 🔑「登录态失效：重新登录或导入」
  - ❓「未探测：点「开始探测」或缩小范围后重试」

### 5.7 无障碍与门禁约束（**改 UI 前先读**）

- **优先用 shadcn/Radix 组合**（`components.json` 已配）；缺组件就补到 `src/components/ui/`，
  不写原生交互快捷（**禁** `details/summary`）。见 `AGENTS.md`「UI 组件策略」。
- 刷新反馈统一 `useRefreshAction`（吸底 500ms + 三件套）；
  **mounted/alive 守卫的 effect 必须在 setup 里重置标志**（StrictMode 演练否则按钮永远转）。
- 矩阵单元格必须**可键盘聚焦**（tab 顺序、`aria-label` 带完整状态文案）。
- 三平台共用组件（矩阵）抽到 `src/components/gateway/` 下，差异用 props 传（与 `account-pool-card` 的
  `QODER_POOL_TILES` 同款做法）。
- 展示字段过 `displayText`（防脏值白屏，issue #2）。

### 5.8 ★ 原型（**已实现，可点**）

本方案的交互**已经在原型里跑起来了**，先体验再决定做不做：

| 产物 | 位置 |
|:--|:--|
| 可点原型（双击即看，单文件） | `docs/prototype/buddy-switch-prototype.html` → 网关页 → 模型 → **限流探测** |
| 拆分版（真实实现的对照片） | `index.html` + `demo-app.js` + `demo-data.js` + `demo.css` |
| 实测（43 条断言 + 截图） | `node docs/prototype/verify-model-probe.mjs` → `logs/model-probe/*.png` |

**原型里能试到的**（都对应上面某一节）：

- **卡片就在模型分区里**（默认折叠成一行，点「展开」）—— **不是弹层**（第一版形态被否）；
- 三个作用域（WB / Qoder / Trae）都能开 —— **三平台同形**；
- **① 先选模型**（默认空选，按钮置灰并提示）**② 再选账号**（默认全选）—— 复选框网格，各自带计数与「全选/清空」；
  选中后页脚直接算出「**N 账号 × M 模型 = X 格**」；
- 开始 → **进度条 + 一行实时统计**（模型限流 / 账号限流 / 积分不足 / 登录失效…）；
- **取消剩余** → 保留部分结果，标「已取消剩余」；
- 结束 → **矩阵**（账号 × 模型，色块 + 悬停详情）、行列汇总、图例；页脚「导出 CSV / 重探」；
- ★ **结果折回模型表**：每行一格「限流（探测）」（`全部可用` / `N/M 限流` / `被动 N` / `—`），
  **点徽章下钻**到矩阵（只留该模型）；
- **被动底色**与本次实测视觉区分（内描边 = 被动已知；虚线灰 = 尚未探测）；
- **回填**（默认开）→ 切到「账号池」能看到 **「冷却 *模型名*」** 徽标（**按模型记**）；
  `ok` 的格会把该模型的冷却**清掉**；
- 亮 / 暗两种主题；换作用域自动清结果。

**原型刻意演示的七条语义**（全部写成断言，改坏即红）：

1. ★ **不是弹层** —— 卡片不在 `.overlay` 里（第一版做成弹层被否，这条防回退）；
2. 只有「模型限流／账号限流／模型封禁」算限流 —— **402 积分不足独立成格**；
3. **默认不替用户选模型**（空选 + 按钮禁用 + 明确提示），账号默认全选；
4. **回填只认本次实测的格**（第一版把被动底色也回填了，被实测抓出来 → 已修）；
5. **`ok` 清掉该模型的冷却**，且冷却**按模型**记（徽标要点名模型，不能只有一个数字）；
6. ★ **结果折回模型表**：卡头的「N 模型可测」与网格同源（曾「卡头 5 个、网格 4 行」）、
   点徽章下钻后「共 N 格」随选择收缩（不能挂着上次的总数）；
7. 换作用域**必须清空**探测结果（否则拿上个作用域的格冒充）。

> ⚠️ 原型是**假数据 + 定时器**（确定性生成，刷新不乱跳）。真实实现是
> **作业 + 轮询**（§3.4），交互形状一致、数据源换成真上游。

---

### 5.9 ★★ 与账号页「客户端限流」的联动（2026-10-22 落地）

需求原话：「账号中的客户端限流要和网关中的模型限流探测关联起来，数据要同步。账号中的客户端限流也可以加入限流探测的入口。」

**两块界面回答同一个问题的两种证据**：台账是**日志反推**（哪一格报过 429、何时恢复），探测是**真发一次最小请求**的实判。此前各自成篇（网关卡单向把台账里 `active` 的格当「被动底色」），现在按 `(uid, model)` 对齐。

| 事项 | 结论（改之前先读） |
|:--|:--|
| **同一份数据（三层）** | ①**本轮作业** `probe_status`（单槽、进度/相位）；②前端合并索引 `probeCells`（`useProbeCells(scope)`，`refreshProbeStatus` 收到快照后**按 `(uid, model)` 合并** —— 单槽每次新作业从空 `cells` 开始，不合并的话「逐行实测」探第二行就把第一行冲掉）；③**落盘台账** `probe_ledger`（`probe_results.json`，跨重启、按作用域隔离）。两处界面读的永远是 ②，② 由 ①③ 合并而成。 |
| **合并只发生在读侧** | 不改后端作业语义（网卡「本次 N 格」仍按 `probe.cells`），也**不擦除**：某次作业没包含的格不会消失（否则探一格清空一堆）。后端在 `finish` 里也**同款合并**进台账（只合并不擦除），故重启后 `probe_ledger` 读回来的就是「这些格最近一次怎么样」。 |
| **必须按 scope 收窄** | `probe_status` 后端**不按作用域过滤**（全局单槽）⇒ 前端 `raw.scope === scope` 才认。少了它，wb-cn 的格会画到 qoder-cn 的矩阵上。 |
| **入口一（卡头）** | 账号页台账卡「实测 N 格」= 台账里「仍受限 / 状态未知」的 **`(uid, model)` 去重组合**（已恢复的不测）。传的是 `probe_start` 的 **`cells` 显式清单**（与 `uids × models` 的笛卡尔积互斥）⇒ 3 条台账就是 **3 格**；若走两根轴会变成 6 格，白烧配额。 |
| **入口二（行尾）** | 每行「实测」= `cells:[{uid, model}]`，恒 1 格。关联不到账号/模型的行（join 失败 / playground）**置灰 + `title` 说明原因**，不是静默不可点。 |
| **入口三（矩阵格）** | 网关卡矩阵单元格详情里「实测/重测这一格」，同口径（单格、不回填）。 |
| **两处入口都 `adopt: false`** | 账号页是**查证**，不是治理 —— 绝不悄悄改池冷却。要回填走网关卡的选择态（那里有明示代价 + 逐条明细）。网关卡头部因此显示「上次 … · **未回填**」而不是「置 0 / 清 0」。 |
| **冲突只并列** | 台账徽章**照旧**；实测结论作为第二结论并列。台账仍受限 + 实测可用 ⇒ 行内写「实测可用，台账仍受限（历史证据，不改写）」；台账已恢复 + 实测受限 ⇒ 「台账已恢复，但此刻实测受限」。谁对不归 UI 裁定。 |
| **实测独有不进台账** | 实测查出限流、但台账没有记录的格单列在卡尾（`probeOnlyLimits`）。台账的语义是「客户端日志里出现过」，塞进去会污染它、也让「条数」变黑箱。 |
| **只有三档算限流** | `LIMIT_STATES` = `rate_limited_model` / `rate_limited_account` / `model_blocked`；`no_credits`（402）在行内显式写「**不是限流**：额度用尽，等解锁没用」。`lib/limit-link.ts` 的 `LIMIT_STATES` 被门禁钉住（混进 402 即红）。 |
| **作业位单例** | 任意入口在跑时，另一侧入口变进度 + 取消（后端拒绝第二个作业时如实 toast，并顺手对齐一次快照）。 |
| **跨重启** | 作业槽是进程内的，所以另有 `probe_ledger`（`GET /api/gateway/probe/ledger?scope=`，落盘 `~/.buddy-switch/probe_results.json`）。两张卡进入时都拉一次并 merge 进 `probeCells`；作业收尾时也会回读一次对齐。**空 `cells` = 没探过**，不是「全可用」。 |
| **`cells` 与两根轴互斥** | 后端 `prepare` 先判 `cells`：有则忽略 `uids` / `models`，只探点名的格（去重保序、超 `MAX_CELLS` 直接报错而不是截断 —— 用户点名的格被悄悄丢掉更糟）。网卡「开始探测」仍走两根轴（那里用户显式勾了账号与模型，笛卡尔积正是他要的）。 |

产物：

| 产物 | 位置 |
|:--|:--|
| 设计稿（可点，左右并排看「同步」） | `docs/prototype/limit-link-proto.html` |
| 行为校验（34 条 + 截图） | `node docs/prototype/verify-limit-link.mjs` → `logs/limit-link/*.png` |
| 纯函数口径 | `src/lib/limit-link.ts`（`mergeProbeCells` / `probeTargets` / `limitConflict` / `probeOnlyLimits` / `tallyCells`） |
| 后端 | `model_probe.rs`（`ProbeStartCell` / `dedup_pairs` / `LedgerFile` / `ledger_snapshot` / `record_ledger`）+ 两宿主 `probe_ledger` 命令 + server `GET /api/gateway/probe/ledger` |
| 单测 | `explicit_pairs_are_trimmed_and_deduped` · `ledger_merges_without_erasing_and_keeps_scopes_apart` · `ledger_round_trips_through_disk` · 接线护栏 `both_hosts_wire_every_probe_exit` |
| 门禁 | `check:scope` 的「★ 客户端限流 × 限流探测：两侧读**同一份**实测结论」 |

---

## 6. 分期实施（逐期可独立验收）

### P1 · WorkBuddy（全链路：被动 + 探测 + 回填 + UI）
1. 后端：`model_probe.rs`（ProbeTarget trait + WB 实现 + 调度 + 取消）
2. 后端：`Pool::adopt_probe_result(...)`（**按模型增删**：`ok` ⇒ 清该模型冷却、限流/封禁 ⇒ 置该模型冷却；不推进退避）
3. 后端：三命令 × 两宿主 + 门禁登记
4. 前端：**模型分区内的 `ModelProbeCard`**（折叠 ⇄ 展开，卡内三态；**不是弹层**）+ store 动作 + i18n 双写
5. 被动底色：把 `pool.snapshot().rate_limited_models` 合进矩阵 **+ 折回模型表新增「限流（探测）」列**
6. 测试：`probe_state_of` 单测 + 变异；回填「不动退避」单测；e2e（mock 上游 429/402/200）；接线护栏
7. 门禁：`tsc` / `check:api` / `check:scope` / `check:store-selectors`

### P2 · Qoder
1. 复用 `ProbeTarget`，实现 Qoder 分支（`build_chat_body` + `qoder_encode` + `cosy::shared_cache`）
2. `QoderPool::adopt_probe_result(...)`（`model_cooldowns` 置位，不推 EMA）
3. **一区一 Key ⇒ 命令参数化**（`region`），跑 `check-scope-registry.cjs`
4. 前端：Qoder 页接同一组件（列集合 = 区域清单）

### P3 · Trae
1. **先定 §4.5.4 的 T1/T2/T3**（我建议 T1）
2. 复用 `ProbeTarget`，实现 Trae 分支（`prepare_llm_chat_body` + `TRAE_LLM_CHAT_PATH`）
3. T1 时**不做回填**；T2 时先补 Trae 模型级冷却（独立立项）
4. 前端：Trae 页接同一组件（全行同列）

---

## 7. 验证与门禁

| 层 | 内容 | 可证伪 |
|:--|:--|:--|
| 纯函数 | 三平台 `probe_state_of`：全枚举覆盖 | 把 `HardCredit` 归进「限流」⇒ 红；把 WB `model_scoped:false` 判成模型级 ⇒ 红 |
| 回填 | 回填后 `soft_streak`/`fails`/`retry_count`/`success_ema`/`error_ema` **逐字段不变** | 改成走 `apply_upstream_error` ⇒ 红 |
| 隔离 | 探测账号 A 不影响账号 B 的池状态；探测**不产生**任何冷却（未开回填时） | 探测路径误调 `apply_*` ⇒ 红 |
| e2e | mock 上游构造 200 / 429(6004) / 429(无标记) / 402 / 11102；断言矩阵单元格 | Qoder 已有 `state.upstream_override` 挂载点（`qoder/routes.rs:364`）；**WB/Trae 需确认是否有等价挂载点（未核实）** |
| 接线 | 两宿主注册点存在性 | 照搬 `catalog_probe::both_hosts_wire_the_catalog_probe` 写法 |
| 门禁 | `tsc` · `check:api` · `check:scope` · `check:store-selectors` · `check:agent-sources`（若动来源表） | — |

---

## 8. 风险与红线（改动时逐条对照）

1. **探测不复用会换号的中继**（否则矩阵归因错）。
2. **探测不写池治理状态**（除 §4.5 的显式回填出口）。
3. **回填不推进退避指数 / 不动健康度**（`soft_streak`/`fails`/`retry_count`/EMA 逐字段不变）。
4. **Trae 无模型级冷却** ⇒ 回填会过度冷却（§4.5.4）。
5. **探测是真实上游请求**：消耗额度、留痕；`ok` 会**清掉该模型的冷却**（2026-10-16 用户拍板，见 §4.5.2）。
6. **信封/无凭据预先拦截** ⇒ `skipped`，不发空 Bearer。
7. **安全红线 F**：只用该 scope 自己的凭据。
8. **规模上限 + 可中断**；单格超时 30s、整作业 10min。
9. **不新增周期性后台任务**（要加就必须同步两宿主注册表）。
10. **命令四处同步 + 三门禁**（api-contract / scope-registry / store-selectors）。

---

## 9. 待你确认（确认完我才动手）

**已由你拍板（2026-10-16）**：

| # | 已定 | 说明 |
|:--|:--|:--|
| ✅ | 被动表 + 主动探测**都要** | 底色合成一张表 + 逐格实探 |
| ✅ | **三个平台全做** | P1 WB → P2 Qoder → P3 Trae |
| ✅ | 结果**回填**池冷却 | 但面板上留开关 |
| ✅ | **Q3：`ok` 要清掉该模型的冷却** | 探到可用就该恢复；回填按**模型**增删 |
| ✅ | 冷却**必须标记到具体模型** | 徽标点名模型（不是只有一个数字） |
| ✅ | **默认不替用户选模型** | 模型默认空选 + 按钮置灰 + 提示；账号默认全选 |
| ✅ | 交互要**干净、不啰嗦** | 两步式面板（① 选模型 ② 选账号），已在原型落地 |

**仍待确认**：

| # | 问题 | 我的建议 |
|:--|:--|:--|
| Q1 | **Trae 回填怎么办**（T1 只探测不回填 / T2 先补模型级冷却 / T3 账号级回填） | **T1** |
| Q2 | 回填是**自动**还是**完成后再点一次「确认回填」**？ | 自动（完成即回填），但面板上留开关 |
| Q4 | `auth_dead` 是否回填（Trae 会永久禁用）？ | **不回填**，只标状态 |
| Q5 | 落盘 TTL（默认 10 分钟）+ 是否要「导出 CSV」？ | 10 分钟 + 要导出 |
| Q6 | 作业进度用**轮询**（照 `switch_progress`）还是**SSE**？ | **轮询**（零新基建，本仓无 EventSource 先例） |
| Q7 | 一期是否**只做 WB**（P1 先行验收）？ | 是 |

### 9.1 安全与合规（2026-10-16 安全审计后补，**接线时必须逐条落实**）

| # | 约束 | 为什么 |
|:--|:--|:--|
| S1 | UI**恒定明示**：「将向每个账号各发一次最小请求，消耗极少积分、**会在上游留痕**，可能触发风控」 | 探测是真实上游请求，不是本地操作；用户有权知道代价 |
| S2 | `reason`（上游原文）前端**一律纯文本渲染**（禁 `dangerouslySetInnerHTML`） | 它是不可信输入：控制字符 / ANSI / 标记都可能出现 |
| S3 | 探测**落审计日志**（谁在何时探了哪些 uid × model、回填了什么） | A09：无留痕就无法追责与复盘 |
| S4 | `uids` / `models` 入参**必须有上限**（已实现 `MAX_AXIS = 200`） | IPC 入参不可信，巨列表耗内存 |
| S5 | 回填**必须**置 `dirty`（已实现） | 否则周期落盘 `flush_if_dirty` 不写盘 ⇒ 回填静默丢失 |
| S6 | 回填的 `realm` **必须校验**（已实现） | 池按 `uid` 建键、`realm` 只是属性 ⇒ 不校验可能跨作用域写错条目 |
| S7 | 冷却**必须有下限**（已实现 `MIN_PROBE_COOLDOWN_MS = 60s`） | `cooldown_model(.., 0, ..)` 落 `now+1ms` = 等于没冷却 |
| S8 | ⚠️ **账号级回填会清空模型级表**（`cooldown_soft_until` 既有语义） | 用户得知道「一个账号级限流会把该账号的模型冷却全清掉」——UI 需说明 |

### 9.2 一期**主动偏离**方案的两处（待用户确认）

| 项 | 方案 §3.3 原设计 | 一期实现 | 理由 |
|:--|:--|:--|:--|
| 并发 | 跨账号并发 2 | **账号串行** | 串行不会自己把账号推成 429；代价是整轮更慢。并发留到有真实耗时数据再定 |
| 模型轴 | 逐账号 `fetch_models` 取并集 | 未指定时回落**区域目录** | 逐账号拉清单要 N 次额外上游往返，收益不抵成本（前端总会显式传 `models`） |

---

## 附：本方案复用清单（逐条可核实）

| 复用物 | 位置 |
|:--|:--|
| WB 限流判据（唯一） | `gateway/src/pool/mod.rs:359 classify_event` |
| WB 池模型级冷却 | `gateway/src/pool/entry.rs:385 cooldown_model` / `:439 block_model_backoff` / `:185 healthy_for_model` |
| WB 被动视图（已导出） | `gateway/src/pool/mod.rs:1038 rate_limited_models` → `routes/status.rs:28` |
| WB 回填（**不复用**，只参照语义） | `gateway/src/pool/mod.rs:784 apply_upstream_error` |
| WB 出站 | `core/upstream.rs:466 chat_stream_with` / `:596 fetch_models` |
| WB 出站改写管线 | `gateway/src/outbound/mod.rs:60 prepare_outbound_body` |
| WB 会话/设备头 | `gateway/src/routes/relay.rs:204-207` |
| Qoder 池模型冷却 + 回填 | `gateway/src/qoder/pool.rs:90 model_cooldowns` / `:353 apply_error(.., model)` / `:392 status_list` |
| Qoder 出站 | `gateway/src/qoder/routes.rs:302-389`（`build_chat_body` + `qoder_encode` + `session.headers` + `PATH_CHAT_SSE`） |
| Qoder 清单 | `core/qoder/api.rs:1433 fetch_model_list` / `:95 ModelMeta` / `qoder/paths.rs:136 cli_models_cache_dir` |
| Trae 池（**无模型级**） | `gateway/src/trae/pool.rs:128 TraePoolEntry` / `:381 apply_error` / `:704 cooldown_of` |
| Trae 出站 | `gateway/src/trae/payload.rs:291 prepare_llm_chat_body` / `trae/routes.rs:490+` |
| Trae 清单 | `core/trae/model_list.rs:105 ClientModelList`（客户端 `state.vscdb`，按变体） |
| Trae 错误分类 | `gateway/src/trae/pool.rs:81 classify_http` / `:96 classify_solo` |
| 客户端只读台账 | `core/client_limits.rs` + `src/components/client-limits-card.tsx` |
| 长任务 + 轮询先例 | `server/api.rs:76/1780` · `src-tauri/commands.rs:127/1347` · `src/components/switch-account-dialog.tsx:191` |
| 后台任务登记表 | `core/background.rs:100 TaskKind` + `server/main.rs` + `src-tauri/lib.rs` |
| 门禁 | `scripts/check-api-contract.cjs` · `check-scope-registry.cjs` · `check-store-selectors.cjs` |
| 共用池卡 | `src/components/gateway/account-pool-card.tsx`（`PoolTileKey` / `QODER_POOL_TILES`） |
