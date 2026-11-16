# 模型降级链（Model Fallback）设计与实施方案

> 状态：**现行 · P0 已实现**（后端降级循环 + IPC + 卡片与抽屉均已接通；P1/P2 见 §10.4）
> 适用：WorkBuddy / Trae / Qoder 三条独立网关 + 统一网关
> 关联：`docs/specs/P5-ARCH.md`（网关架构）、`crates/buddy-switch-gateway/src/pool/gate.rs`（模型闸门）

---

## 0. 一句话方案

在**换号重试循环的外层**再包一层「模型尝试循环」：主模型按现有逻辑跑完换号仍失败、且失败原因属于**可降级白名单**时，改写请求体的 `model` 字段、换一个同作用域的备用模型再跑一遍；最多 2 跳，每请求每模型只试一次。配置独立落盘、热生效，UI 落在「模型」分区的一张卡片里。

---

## 1. 现状与缺口（已逐条核实）

| 事实 | 位置 | 对方案的含义 |
|:--|:--|:--|
| 网关**没有「选模型」这一步**：模型名只做前缀剥离与禁用判定，随后原样进 body | `routes/chat.rs:110-151`、`model_route.rs:96` | 降级必须新增一层模型名改写，不是在现有选择逻辑里加分支 |
| 换号重试循环在 `relay_inner`，`tries = config.max_rotate`，单账号失败即拉黑 | `routes/relay.rs:354`、`:272` | 降级循环要在它**外面**，否则与账号维度耦合 |
| 模型闸门命中即 `break`，**直接失败、不换模型** | `routes/relay.rs:362-413` | 闸门是降级最重要的触发源；命中时应**零成本跳到下一跳** |
| 计入闸门的判据极窄：只有 6004 与 11102 | `routes/relay.rs:856 is_model_level` | 降级判据不能直接复用 `is_model_level`，需要更宽的白名单 |
| 全仓**无**任何模型级 fallback / alias 实现；`MODEL-IDENTITY-ARCH.md` 设计过 `model_alias.rs` 但未落地 | 文档存在、代码不存在 | 从零新增，无历史包袱，但也无现成抽象可复用 |
| 超时与 5xx **无法区分**（统一 `status=0, kind=Server`） | `core/upstream.rs:439`、`net.rs:62 is_timeout` | 「超时降级」需要先补一个判据位，属独立小改动 |
| `Pool.config` 在 `GatewayState::new` 时固化，`save_gateway_config` 只写 `state.config` | `state.rs:412`、`commands.rs:2627` | **降级配置绝不能放 `pool`**，否则改完必须重启网关才生效 |
| 已有 `disabled_models` 是「独立文件 + 进程内唯一 `Arc` + 按 scope 分家」的成熟范式 | `disabled_models.rs` | 降级配置直接照抄这套形状，风险最低 |
| `rewrite_body_model(raw, bare_model)` 已是「把 body 顶层 model 改成指定名」的现成实现 | `model_route.rs:96` | 降级改名**复用它**，不新写 JSON 操作 |
| `relay()` 在唯一出口归还探针票 `release_probe(&model, &tag)` | `routes/relay.rs:247-261` | ★ 跨 hop 时 model 变了，**归还必须按 hop 的模型逐个做**，否则该模型半开态会卡满 120s 租约 |

**结论**：这是一个纯增量功能，不需要改现有治理语义；风险集中在「循环嵌套后的资源归还」与「降级判据的边界」两处。

---

## 2. 六个核心决策

### D1 · 降级循环放在换号循环外层，不是里面

```
请求 → [hop 0: 主模型 → 换号循环 A→B→C] → 失败? → [hop 1: 备选1 → 换号循环] → …
```

**理由**：换号是「同一个模型换一个账号」，降级是「换一个模型」。两者正交，嵌套顺序决定了成本：

- 若把降级塞进换号循环内部（「这个号失败就换个模型试试」），会出现「A 号 + glm 失败 → 换 deepseek 再试 A 号」——同一个账号被反复使用，且 `tried` 集合语义被污染。
- 放在外层则天然成立：hop 0 用尽账号仍失败 ⇒ 判定为模型问题 ⇒ 换模型 ⇒ **重置 `tried`**（不同模型可用账号不同）⇒ 重新选号。

**代价**：`relay_inner` 要拆成 `relay_once`（原逻辑，收 `model` + `body` 两个参数）+ 新的 `relay`（调度 hop）。这是纯机械重构，不改变任何治理语义。

### D2 · 配置独立文件，不进 `GatewayConfig`，更不进 `pool`

新建 `~/.buddy-switch/gateway_model_fallback.json`，进程内唯一 `Arc<RwLock<>>`，与 `disabled_models.rs` 完全同构。

**理由**（三条，任何一条都足以否掉另外两种放法）：

1. `GatewayConfig` 的键集被 `gateway_config_serializes_with_snake_case_keys` 精确钉死，加字段要改契约测试；且它是**运行配置**，而降级链是**用户偏好**（同 §2.7 的判定）。
2. 放进 `pool` ⇒ 继承「改完要重启网关」的坑（见 §1 第 7 行）。用户点保存后不生效、也没有任何提示，这是最难自查的一类缺陷。
3. 三个网关 + 命令面各自持一份内存副本 ⇒ 「保存写到了 A、B 读到陈旧副本」的假开关问题（`disabled_models.rs` 模块文档已就此论证过一次）。

**收益**：保存即生效，零重启。

### D3 · 只在**同作用域**内降级

备选模型只能从当前 scope（`wb-cn` / `trae-global` / `qoder-cn` …）的模型目录里选。

**理由**：作用域决定三件事——账号池、上游 wire 协议、计费口径。跨作用域降级等于「把这个请求改道到另一个产品的网关」，那是**统一网关命名空间**的职责（`{客户端名}-{区域}` 前缀），不是降级链的职责。把两件事混在一起，会让「我的 Key 明明绑的是 WB 国内版，为什么账单出现在 Trae」变成无解的问题。

**跨作用域作为 P2 立项**，前置条件是统一网关已经能稳定做命名空间分派；届时实现方式是「降级目标写成 `trae-cn/glm-5.3` 形式的命名空间串」，由统一网关接管分派，单产品网关不实现。

### D4 · 触发判据是**白名单**，不是「失败就降级」

默认只认三类模型级失败（闸门暂停 / 6004 / 11102），5xx 与超时需用户显式勾选。完整表见 §4。

**理由**：降级的本质是「换一个模型可能成功」。判据宽一格，就会把「调用方参数写错了」翻译成「另一个模型也不认识这个参数」——用户拿到的错误更陌生、且白烧一次配额。宁可少降，不可乱降。

### D5 · 每次请求都从主模型开始，**不做粘性降级**

请求 A 降级到 glm 成功后，请求 B 仍然先试主模型。

**理由**：粘性降级会让「我以为在用 claude，其实一直在用 glm」变成长期静默状态，而这正是降级功能最需要避免的认知错位。而**回切成本本来就是零**——主模型失败时闸门已经把它标记为暂停，暂停期内的后续请求在 hop 0 的闸门判定处直接短路、一次上游请求都不发。闸门到期后自然回到主模型。**闸门 + 降级天然协同，不需要额外的回切机制**。

### D6 · 对客户端透明，但必须可观测

降级成功时返回**目标模型**的正常响应，同时带两个响应头：

```
X-Buddy-Fallback-From: claude-sonnet-4.5
X-Buddy-Fallback-To: glm-5.3
X-Buddy-Fallback-Reason: model_gate
```

**理由**：OpenAI 协议的响应体里 `model` 字段会变成 `glm-5.3`。如果不同时给出「原来是什么、为什么换」，用户看到模型名变了只会以为网关串号了。这两个头是**唯一**能让客户端（和用户）把「降级」与「故障」区分开的信息。

失败时错误体新增可选 `fallback` 对象（见 §7），不破坏 `attempts` 的四项约定。

---

## 3. 数据模型

`~/.buddy-switch/gateway_model_fallback.json`：

```jsonc
{
  "version": 1,
  "scopes": {
    "wb-cn": {
      "enabled": true,
      // 主模型 → 有序备选列表。键用主模型名（大小写敏感存储、判断不敏感）
      "chains": {
        "claude-sonnet-4.5": {
          "targets": ["glm-5.3", "deepseek-v4.1-flash"],
          "triggers": ["gate", "model_rate", "model_blocked"],
          "enabled": true
        },
        "qwen-max": {
          "targets": ["claude-sonnet-4.5"],
          "triggers": ["gate"],
          "enabled": false
        }
      }
    },
    "trae-cn": { "enabled": false, "chains": {} }
  }
}
```

设计要点：

- **键是 `String`（scope id）而不是 `Scope` 枚举**——`Scope` 是枚举，未知键会让整份反序列化失败、回落空表、清空全部配置（`disabled_models.rs` 踩过的脚枪）。
- `deny_unknown_fields` 关闭（`#[serde(default)]` 但**不** deny），未知字段原样保留，兼容未来加字段。
- `targets` 上限 **2**（`MAX_TARGETS = 2`），且**不开放**给用户调。理由：每跳都要走完一整轮换号循环（`max_rotate` 默认 64，实际通常 1-3 次），延迟是线性叠加的；第 3 跳的实际成功率极低而成本确定。上限 2 与「总跳数 ≤ 3（主 + 2）」是同一条约束的两面，写死在一个常量里，避免出现「配了 3 个但只用前 2 个」的死配置。
- **不做「别名/映射表」**：`chains` 是「失败时怎么办」的策略，不是「A 等价于 B」的路由表。混在一起会让 `GET /v1/models` 的清单语义变模糊。

Rust 侧（新文件 `crates/buddy-switch-gateway/src/model_fallback.rs`）：

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelFallback {
    pub version: u32,
    /// 键是作用域 id 字符串（**不是** Scope 枚举，见文档 §3）
    pub scopes: HashMap<String, ScopeFallback>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeFallback {
    pub enabled: bool,
    /// 主模型 → 链。键保持原始大小写（UI 要按原样回显）
    pub chains: HashMap<String, FallbackChain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FallbackChain {
    pub targets: Vec<String>,
    pub triggers: BTreeSet<Trigger>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// 模型闸门暂停（零成本触发）
    Gate,
    /// 模型级限流 6004
    ModelRate,
    /// 模型封禁 11102
    ModelBlocked,
    /// 上游 5xx
    Server,
    /// 传输层超时
    Timeout,
    /// 上游 404（该账号没有这个模型）
    NotFound,
}
```

**唯一判断点**（同 `is_disabled` 的纪律）：`plan_for(&self, scope, model) -> Vec<&str>`，返回 `[主模型] + 有效备选`。**禁止**在别处复制「哪些跳该试」的逻辑。

---

## 4. 触发判据表

| 上游事件 | 现有归类 | 默认降级 | 可勾选 | 判据理由 |
|:--|:--|:--:|:--:|:--|
| 模型闸门暂停 `ModelGatePaused` | 503 | ✅ | 固定 | 已有跨请求证据表明该模型整体不可用，**零成本**（一次上游请求都不发） |
| 模型级限流 `SoftRate{model_scoped:true}`（6004） | 429 | ✅ | 固定 | 上游明说是这个模型的事，换号无用 |
| 模型封禁 `ModelBlocked`（11102） | 4xx | ✅ | 固定 | 短时间不会恢复 |
| 上游 5xx `Server` | 5xx | ❌ | ✅ | 换模型大概率有效，但**故障期间会把流量全推给备选**，可能连锁打挂。默认关，用户自担 |
| 请求超时 | `status=0` | ❌ | ✅ | 需要新判据位（§5.4）。默认关：长任务降级会**重复计费** |
| 上游 404 `NotFound` | 4xx | ❌ | ✅ | 通常意味着「该账号的模型清单里没有它」，换号已试过；跨模型 404 语义不确定 |
| **参数错误 400 / 422** | `Client` | ❌ | **禁止** | 换模型救不了，只会把同一个错误翻译成更陌生的模型名 |
| **内容审核拒绝**（`11-128` / `content_blocked`） | `Client` | ❌ | **禁止** | ★ 见下方红线 |
| **402 余额不足** | `HardCredit` | ❌ | **禁止** | 账号额度问题，换模型同样 402 |
| **401/403 凭据被拒** | `AuthRejected` | ❌ | **禁止** | 账号问题。降级会污染目标模型，且掩盖了「该重新登录」这个真实指引 |
| **Key 配额用尽** `KeyQuotaExceeded` | 429 | ❌ | **禁止** | 按 Key 计量，与模型无关。且它在选号前就被拒，根本进不了循环 |
| **在途占满** `PoolBusy` | 503 | ❌ | **禁止** | 几秒自愈，等待比降级便宜 |

### ★ 红线：内容审核拒绝永不降级

不同模型的审核策略不同，「换个模型再试一次」在效果上等于**帮调用方绕过内容审核**。而且它会造成最难解释的行为：同一段内容，今天被拒、明天通过，取决于当时哪个模型在闸门里。

项目里已有的 `outbound/prompt.rs` 的 `DegradeGate`（换中性提示词重试）是**另一个东西**——那是「同一模型、换一种说法」，不是「换一个审核尺度不同的模型」。两者必须严格分开，命名上也要分开（见 §5.5）。

### 一个必须写进代码注释的边界

`is_model_level`（`relay.rs:856`）**不能**直接复用为降级判据：它只认 6004/11102，是为了「计入闸门」而刻意收窄的。降级判据更宽（还含闸门本身、5xx、超时），两者是不同的问题。新建 `fallback::should_fallback(&GatewayError, &Trigger set) -> Option<Trigger>` 作为唯一判定点，并在注释里写明「与 `is_model_level` 的宽窄差异是刻意的」。

---

## 4.5 预算：按**请求次数**，不按跳数

延迟由请求次数决定，不由跳数决定（3 跳 × 3 个号 = 9 次，与 2 跳 × 5 个号同量级）。

| 常量 | 值 | 含义 |
|:--|:--|:--|
| `TOTAL_REQUEST_BUDGET` | 8 | 一次请求跨**所有跳 × 所有账号轮次**的总上游请求数上限。每跳分到 `min(max_rotate, 剩余预算)`，用完即停（不论链还剩几跳） |
| `MAX_ATTEMPTS_PER_HOP` | 6 | `max_rotate` 的合法上限（1–6） |
| `MAX_TARGETS` | 3 | 链里最多 3 个备选（主 + 3 跳） |

**已知例外**：单次上游超时是 600s（`state.rs`），一次超时就吃满整个预算 ——
刻意不在这里缩短（那会影响所有正常请求）。

## 5. 执行时序

### 5.1 主流程

```
handler（chat.rs / messages.rs / responses.rs）
  └─ 模型名前缀解析 → 禁用判定 → 配额判定      ← 全部保持现状，在降级之前
      └─ relay(state, request)                 ← 改造点
           ├─ plan = fallback::plan_for(scope, origin_model)   // [主, 备1, 备2]
           ├─ for (hop, model) in plan.enumerate():
           │     ├─ hop > 0 时：
           │     │    ├─ 目标禁用？ → skip
           │     │    ├─ 目标闸门暂停？ → skip（不发请求）
           │     │    ├─ 能力不满足请求？ → skip（见 5.3）
           │     │    └─ body = rewrite_body_model(body, model)   ← 复用 model_route.rs:96
           │     ├─ relay_once(...)             // 原 relay_inner，含换号循环
           │     ├─ Ok  → 打响应头，return Ok
           │     └─ Err(f) →
           │          ├─ fallback::should_fallback(&f.error, chain.triggers)
           │          │     ├─ None → return Err(f)          // 不可降级，原样抛出
           │          │     └─ Some(trigger) → 记事件，continue
           │          └─ 已无下一跳 → return Err(f + fallback 摘要)
           └─ finally: 按 hop 归还探针票
```

### 5.2 ★ 探针票必须按 hop 归还

现有代码在 `relay()` 的唯一出口做一次 `pool.release_probe(&model, &tag)`。改造后 `model` 是 hop 变量，**必须移进循环**：

```rust
for (hop, model) in plan.iter().enumerate() {
    let outcome = relay_once(state, &request_for(model), probe_tag).await;
    {
        let mut pool = state.pool.write().await;
        pool.release_probe(model, probe_tag);   // ← 每 hop 都要还
    }
    match outcome { ... }
}
// 验证票是请求级的，仍在最外层还一次
state.pool.write().await.release_verify_tickets(probe_tag);
```

**漏了这一条的后果**：hop 0 的模型领了半开探针票却没还，该模型会在 `PROBE_LEASE_MS = 120_000` 内一直处于「另一个请求正在探路」的 `HalfOpen` 态，后续请求全部被 `blocked` 挡住——表现为「降级配好之后，主模型彻底不可用了」，而且日志里看不出任何错误。这是本方案最容易写错的一处，必须在测试里覆盖。

### 5.3 降级前的能力校验

降级不是「换名字」这么简单：请求体可能带 `tools` / `tool_choice` / 图片 content part / `response_format` / 超长上下文。目标模型不支持时，硬发过去会得到 400，比原始错误更难懂。

```rust
/// 目标模型能否承接这次请求。不支持 ⇒ 跳过这一跳，继续下一跳（不是失败）。
fn target_accepts(scope: Scope, model: &str, body: &Value) -> Result<(), SkipReason>
```

判据来自 `model_capability.rs` 与模型目录（`model-ability.tsx` 展示的同一份数据）：

| 请求特征 | 目标模型缺失时 |
|:--|:--|
| `tools` / `tool_calls` 非空 | skip：`no_tools` |
| 含 `image_url` / `input_image` | skip：`no_vision` |
| `response_format: json_schema` | skip：`no_json_mode` |
| 估算 token > 目标上下文窗口 | skip：`context_too_small` |
| 目标不在该 scope 的模型目录 | skip：`unknown_model`（配置被手工改坏时的兜底） |

**全部跳都被 skip ⇒ 返回原始错误**，并在 `fallback` 摘要里写明每个跳被跳过的原因。用户看到「配了但没生效」时，必须能立刻知道是能力不匹配。

### 5.4 超时判据（独立小改动）

现状：`upstream.rs:439-446` 把传输层失败统一成 `status: 0, kind: Server`，`net.rs:62` 有 `is_timeout()` 但结果被丢弃。

改动：`UpstreamErrorKind` 增加 `Timeout`（或 `Upstream` 错误体加 `is_timeout: bool`），在 `upstream.rs` 的失败分支按 `describe_transport_error` 的结果分类。**这是本方案唯一触碰 `buddy-switch-core` 的改动**，需要单独一个 PR、单独一轮回归。

若想压缩范围：v1 可以先不做超时降级，UI 上把「请求超时」这一项置灰并注明「待支持」。**建议就这么做**——5xx 已经覆盖了大部分瞬时故障，超时的重复计费风险还更高。

### 5.5 与「提示词降级」的关系

| 机制 | 代码标识 | 语义 | 与本方案的关系 |
|:--|:--|:--|:--|
| 提示词降级 | `outbound/prompt.rs` `DegradeGate`、`chat.rs:175` | 同一模型，换中性提示词重试一次 | **互斥**：已在提示词降级窗口内，不再触发模型降级 |
| Qoder 参数降级 | `qoder/degrade.rs:104` | 同一模型，400/422 时退化为纯文本模式 | 在 `relay_once` 内部，与本方案无冲突 |
| 模型降级 | `model_fallback.rs`（本方案） | 换模型重发 | — |

**命名纪律**：本方案一律用 `fallback`，**不得**用 `degrade`。`degrade` 在仓里已有两个含义，再加第三个必然读错。

**叠加保护**：一次请求最多 1 次提示词降级 + 2 次模型降级。两者都触发时，提示词降级在路由层、模型降级在 relay 层，顺序上模型降级先发生（relay 更内层），所以实际是「先换模型，失败后换提示词」——这个顺序要写进注释，否则以后没人能推断出来。

---

## 6. 硬边界（不可降级 / 不可回滚）

1. **已向客户端输出任何字节**（含 SSE 首帧、已发送响应头）⇒ 不可降级。这是物理限制，不是策略选择。降级只能发生在 `peek_first_frame` 之前。
2. **流式请求在首帧之后失败** ⇒ 记冷却、记闸门，但**不重发**。客户端会看到一个正常的流中途断掉——这比「静默换成另一个模型继续输出」诚实。
3. 目标模型被禁用（`disabled_models`）⇒ 不可作为目标。UI 上选择器里直接不出现。
4. 目标模型在闸门暂停中 ⇒ 跳过该跳，不等待。
5. 同一请求内同一模型最多尝试一次（`visited: HashSet<String>`）⇒ 环路不会死循环。
6. 总跳数 ≤ 3（主 + 2）。
7. 降级不改变 `attempts[]` 的对外契约（仍是四项），轨迹放 `fallback` 对象。
8. **不因降级而改写客户端看到的 `model` 之外任何字段**：`usage` 必须是真实的上游用量，不能把两个模型的用量相加。
9. 账本记**实际使用的模型**（`glm-5.3`），不是主模型。否则用量页的模型维度会骗人。
10. 降级成功后**不**解除主模型的闸门/冷却——它确实失败了，治理态要保留。

---

## 7. 对外契约变化（全部向后兼容）

### 成功响应新增响应头

```
X-Buddy-Fallback-From: claude-sonnet-4.5
X-Buddy-Fallback-To: glm-5.3
X-Buddy-Fallback-Reason: model_gate | model_rate | model_blocked | server | timeout | not_found
```

`reason` 用稳定英文标识符（不是中文文案），客户端可编程判断。中文解释只在 UI 与错误体里出现。

### 失败响应新增可选字段

```jsonc
{
  "error": {
    "message": "...",
    "type": "server_error",
    "code": "model_gate_paused"
  },
  "attempts": [ /* 保持现有四项，不改 */ ],
  "fallback": {
    "origin": "claude-sonnet-4.5",
    "tried": ["glm-5.3"],
    "skipped": [{ "model": "deepseek-v4.1-flash", "reason": "no_vision" }],
    "trigger": "model_gate"
  }
}
```

**为什么单开一个对象而不是往 `attempts` 里加字段**：`attempts` 的注释明确写了「只带排查必需的四项，对外契约越小越稳」。加字段会破坏这个约定；单开一个可选对象则零破坏，且语义更清楚（`attempts` = 账号维度，`fallback` = 模型维度）。

---

## 8. UI 方案

**设计已经实现在原型里**，不是另画一张稿：

| 位置 | 内容 |
|:--|:--|
| `docs/prototype/demo-app.js` | `fallbackCard()` / `fbDrawerHtml()` / `fbSimHtml()` / `fbStateOf()` / `fbDiags()` + 7 个 `fb-*` 动作 |
| `docs/prototype/demo.css` | 末尾 `.fb-*` 段（链行 grid、状态点、诊断带、抽屉里的纵向时序） |
| `docs/prototype/demo-data.js` | `GATEWAY["wb-cn"].fallback`（3 条链 + 5 条流水）+ `models[].caps` |
| 看效果 | `npm run proto:single` → 打开单文件版 → `?dest=gateway&gw=models` |

> ★ **原型是产物，不是源**。`buddy-switch-prototype.html` 由 `scripts/gen-prototype-single.cjs`
> 从 `index.html` + `demo.css` + `demo-data.js` + `demo-app.js` 拼成；手改单文件会在下次生成时
> 被覆盖（脚本头部记着 2026-10-21 的真实教训：单文件曾比源落后 300+ 行，改完「看起来两边都改了」）。
> **改设计 = 改那四份源，再 `npm run proto:single`。**
>
> 已验证：`check-scope-registry.cjs` **331 passed / 0 failed**，浏览器控制台零错误。

### 8.1 落点：`models` 分区内的卡片，**不新增左栏分区**

位置：`上下文上限` 策略卡之下、清单来源条（`catalogBar`）之上；React 侧对应
`ApiServicePage.tsx:515-520`，即 `ModelProbeCard` 与 `ModelList` 之间。

**为什么在 `catalogBar` 之前而不是表格之后**：`catalogBar` 说的是「这份清单从哪来」，
必须紧贴表格；而「降级链」和上面的「上下文上限」都是**策略**（怎么用这批模型），同类相邻。

**理由**（三条，前两条是否决项）：

1. 新增分区要改 `gateway-subnav.tsx` 的 `GW_INDEPENDENT_SECTIONS`、三个 ApiServicePage 的接线、以及 `check-scope-registry.cjs:4801` 的 `wantIndep` 数组——**四处联动、一处漏改就 build 红**。而卡片是零门禁成本。
2. Trae / Qoder 用的是 `GW_INDEPENDENT_SECTIONS_NO_PROXY`，加分区还要决定它是否出现在这两处；而卡片自动跟着分区走。
3. 语义上「降级链」和「模型清单」是同一组东西：**清单说有哪些模型，降级链说它们挂了怎么办**。放一起，用户能看着清单直接配。

**代价**：`models` 分区变长。缓解：卡片默认展开（它比下面的长表更需要被看见），
但**只在有链时渲染链行**；空态只占一个 `empty` 块，比一整张表短。

### 8.2 卡片结构

```
┌───────────────────────────────────────────────────────────────────────────┐
│ 模型降级链  [已启用 · 2 条]              同作用域 · 最多 2 跳   [降级流水] │
│ 仅在上游明确失败时触发。已开始向客户端输出的流不会回退，也不会把失败       │
│ 改写成成功 —— 客户端可通过响应头 X-Buddy-Fallback-To 得知实际模型。        │
├───────────────────────────────────────────────────────────────────────────┤
│ [●] 启用降级                                        [+ 新建降级链]        │
│     关掉后配置保留，但一次都不触发                                        │
├───────────────────────────────────────────────────────────────────────────┤
│ ● claude-sonnet-4.5  →  ● gpt-5-codex  ● gemini-2.5-pro    +5xx 停用 ⋯  │
│ ⚠ 第 1 跳 gpt-5-codex 正在闸门暂停中（2 个账号先后失败，剩 42s）。期间该链 │
│   会直接跳到下一跳；若都不行，返回原始错误，不会无限等待。                 │
│ ⚠ gemini-2.5-pro 不支持工具调用，而主模型支持。带这类内容的请求不会降级到 │
│   它 —— 网关会跳过这一跳继续下一个，而不是发出一个必然失败的请求。         │
├───────────────────────────────────────────────────────────────────────────┤
│ ● gemini-2.5-pro  →  ● gpt-5-codex  [+ 备选]                     停用 ⋯ │
│ ⚠ 第 1 跳 gpt-5-codex 正在闸门暂停中（2 个账号先后失败，剩 42s）。……      │
├───────────────────────────────────────────────────────────────────────────┤
│ ● claude-opus-4.1  →  ● claude-sonnet-4.5        [已停用]  启用 ⋯        │
└───────────────────────────────────────────────────────────────────────────┘
```

**关键设计点**（这些是 UI 真正的价值，不是装饰）：

1. **状态点 `●` 是实时健康态，不是静态标记**。绿=可用、黄=冷却/闸门中、灰=禁用/不可用。数据由 `fbStateOf()` 从 `modelGate` / `pool[].modelCool` / `disabled` / `available` **现算**，与账号池那张表读同一份数据。**这一个点回答了用户最关心的问题：「我这条链现在真的能救我吗？」** 写死的绿点在闸门暂停时仍然亮着——那比不显示更糟。
2. **诊断带贴在链行下面，而不是弹 toast**。降级链的问题几乎全是「配置对了但不会生效」（目标也在闸门里、目标能力不匹配、环路），这类问题必须在**配置的那一刻**看得见，而不是等用户来问「为什么没降级」。一条链**可以有多条诊断**（闸门 + 能力不匹配同时成立是常见情形）。
3. **触发标签只标注「超出基线」的部分**。`gate` / `rate` / `blocked` 是默认全开的基线，把它们也画成 tag 会让每行右侧堆满同样的三个词，反而看不出「这条链额外开了 5xx」。所以卡片上只出现 `+5xx` 这种增量，完整清单进 `title`。
4. **空态必须解释「默认关闭」**。降级是会改变行为的开关，空态只写「暂无配置」会让人以为坏了。
5. **不显示「降级成功率」之类的比率**。请求日志是环形缓冲（默认 200 条），繁忙时只覆盖十几分钟，任何按窗口聚合的数字都不能从它算。卡片上的「近 24 小时降级 N 次」必须来自账本新增的计数维度（P1 项）；**P0 只显示最近事件列表，不显示次数** —— 原型里的流水就是按这条做的（表尾明写「不参与任何统计口径」）。
6. **停用的链整行变暗但仍可见**（`.fb-chain.is-off`），与「禁用模型整行变暗」同一套语言：用户要知道它存在、可随时启用。

### 8.3 编辑抽屉（不是弹窗）

用**右侧抽屉**，不是居中弹窗。理由：原型里「改一份带多组的配置」一律走抽屉（Key 权限、
CLI 接入同款），弹窗只留给「确认一个动作」。这里要选模型、排顺序、勾条件、看预演，属前者。
抽屉宽度也比 560px 弹窗更宽裕，纵向时序不用挤。

四个区块（`fbDrawerHtml`）：

1. **主模型**：编辑已有链时**锁定**。理由写进 UI 了：改 `from` 等于换了一条链，会让
   「这条链的历史」和「新链」混在一起 —— 要换就删掉重配。
2. **备选模型序列**：**2 个槽位**（`MAX_TARGETS = 2`），每行 `序号 + select + ↑ ↓`。
   - **不引入 `dnd-kit`**。理由：最多 2 项，上下移按钮完全够用；引入拖拽库要额外处理键盘可达性、触摸、与 Radix focus 管理的冲突，成本远超收益，还会让「同一行里还有 select」的拖拽手势变得模糊。
   - 已禁用 / 上游不可用的模型不出现在选项里（与 `disabled_models` 同源）。
   - 同一模型在清单里可能有「国内版 / 国际版」两条记录 ⇒ **按名字去重**，否则下拉里出现两个一模一样的选项，让人以为是两回事。
3. **触发条件**：5 个可勾选项（`gate` / `rate` / `blocked` / `server` / `timeout`）+ 一个**置灰锁定**的
   「固定不降级」说明块，把 400 / 内容审核 / 402 / Key 配额逐条列出来并说明为什么。
   **让用户看见「什么不会降级」比让他猜更重要。**
4. **模拟路径**：保存前预演，用后端 `preview_model_fallback` 返回真实闸门/冷却态。用**纵向时序**
   而不是横向箭头链——抽屉宽度下横向链必然折行，折完就看不出顺序了（设计稿第一版实测）：

   ```
   1  claude-sonnet-4.5        主模型    闸门暂停 · 剩 42s
   2  gpt-5-codex              备选 1    仍在暂停 → 跳过，不发请求
   3  gemini-2.5-pro           备选 2    可用 → 本次会用它
   ```

   这是整个 UI 里最省客服成本的一块：用户不用先保存再等一次真实失败。

**抽屉里的一个坑**（已踩过）：切触发条件 / 换备选都要重开抽屉来刷新画面（保存按钮可用态、
模拟路径都由 draft 派生），若每次都从已存配置重建 draft，用户刚勾上的条件会被立刻冲掉 ——
那是典型的「点了没反应」。所以 `openFbDrawer()` 里有一句 `same` 判断：**草稿还在且编辑的是
同一条链就保留草稿**。

### 8.4 文案纪律

| 位置 | 必须说清 | 反例（不要写） |
|:--|:--|:--|
| 卡片副标题 | 只在**明确失败**时触发；已输出的流不回退 | 「智能容错」 |
| 开关 | 关掉后**配置保留**但不触发 | 「关闭降级」 |
| 5xx 选项 | 故障期间流量会涌向备选，**可能把备选也打挂** | 「上游异常时自动切换」 |
| 超时选项 | 长任务可能**重复计费** | 「超时自动重试」 |
| 锁定项 | 逐条说明**为什么**不降级 | 「以下情况不支持」 |
| 降级流水 | 只有最近 N 条，不参与统计口径 | 「降级成功率 98%」 |

### 8.5 空态与首次配置

**P0 只做 `新建降级链`**（原型已实现：空态里一个 `empty` 块 + 一个主按钮）。

`从最近失败生成`（读最近失败日志、列出「近期失败过的模型」、点选后预填成链）列为 **P1**：
它依赖「失败日志按模型可检索」这个前提，而请求日志是环形缓冲、没有索引，先在 P0 里验证
降级链本身值不值得用，再决定要不要为它加一层取数。

**不做推荐链**的理由：推荐需要猜「哪个模型等价」，而模型之间没有等价关系（能力、价格、上下文都不同）。猜错一次的代价是用户以为有保护、实际降级到一个不能用的模型。宁可让用户自己选。

---

## 9. 前端实现

### 组件树

```
src/components/gateway/model-fallback-card.tsx      主卡片（~320 行）
  ├─ FallbackChainRow                              单条链 + 状态点 + 诊断带
  ├─ FallbackDrawer                                编辑抽屉（不是 Dialog —— 见 §8.3）
  │    ├─ TargetSlots                              2 槽位 + 上下移
  │    ├─ TriggerChecks                            5 项 + 锁定说明块
  │    └─ FallbackSimulation                       模拟路径（纵向时序）
  └─ FallbackFlowTable                             流水（复用 request-log 的行样式）
```

**状态点与诊断带的数据源**：`live_state` 由后端在 `get_model_fallback` 里随视图一起返回
（`Healthy | Cooling{secs} | Gated{secs} | Disabled`），前端**不自己拼**——
否则卡片上的点和账号池里的点会在两个时刻各自计算，出现「这边绿那边黄」。

### 复用清单（不许自己写）

| 需求 | 用现成的 |
|:--|:--|
| 卡片外壳 / 卡头 | `ui/card` + `CardHead`（`px-5 py-3` + `border-b border-border/60`） |
| 开关 | `ui/switch` |
| 下拉 | `ui/select` |
| 徽章 / tag | `ui/badge` |
| 空态 | `ui/empty-state` |
| 弹窗 | `ui/dialog`（`DialogContent` 已带限高可滚） |
| 提示 | `ui/alert` |
| 折叠 | `ui/button` ghost + Chevron 图标 |

### store

`src/stores/gateway.ts` 增加三个字段与三个 action（**不改现有字段**）：

```ts
fallback: ModelFallbackView | null;
fallbackPreview: FallbackPreview | null;
loadFallback(scope): Promise<void>;
saveFallback(scope, config): Promise<void>;
previewFallback(scope, from, targets): Promise<void>;
```

selector 必须写细粒度（`check-store-selectors.cjs` 会拦）。

### i18n

域 `shared.gateway.fallback.*`（与 `shared.gateway.probe.*` 同级），中英双份：

```ts
"shared.gateway.fallback.title": "模型降级链",
"shared.gateway.fallback.notice": "仅在上游明确失败时触发。已开始向客户端输出的流不会回退……",
"shared.gateway.fallback.trigger.gate": "模型闸门暂停",
"shared.gateway.fallback.locked.why": "参数错误、内容审核拒绝、余额不足、Key 配额用尽固定不降级……",
"shared.gateway.fallback.diag.gateTarget": "第 {hop} 跳 {model} 正在闸门暂停中（剩 {secs}s）……",
```

`zh` 是键的权威；新增键必须同时加 `en`，否则英文界面回落中文。

---

## 10. 实现清单

### 10.1 已完成：真实实现（后端 + 前端）

| 文件 | 改动 |
|:--|:--|
| `crates/buddy-switch-gateway/src/model_fallback.rs` | **新增**：`Trigger` / `FallbackChain` / `ScopeFallback` / `ModelFallback`、`load`/`save`/`shared`、`plan_for`（唯一判断点）、`trigger_for`（白名单判据）、`normalize`、`build_view`、`FallbackEvent` 环形缓冲 + **10 个行为级单测** |
| `crates/buddy-switch-gateway/src/routes/relay.rs` | `relay` 拆成三层：`relay`（归还验证票）→ `relay_hops`（模型维度）→ `relay_once`（账号维度）；`RelayOutcome::Ok` 加 `degrade`；`RelayFailure` 加 `event` / `fallback`；`apply_degrade_headers`；事件记录 |
| `routes/chat.rs` / `messages.rs` / `responses.rs` | 解构 `degrade`、把 `model` shadow 成**实际使用的模型**（账本与日志同口径）、注入响应头 |
| `state.rs` | `GatewayState.fallback`（进程内唯一 `Arc`，与 `disabled` 同一套纪律） |
| `crates/buddy-switch-server/src/api.rs` | 3 个 REST handler（`GET`/`POST /api/gateway/model-fallback`、`POST …/preview`） |
| `src-tauri/src/commands.rs` + `lib.rs` | `get/save/preview_model_fallback` 三个命令 + 注册 |
| `src/lib/types.ts` / `api.ts` / `stores/gateway.ts` | 类型、3 个 API、store 字段与 2 个 action |
| `src/components/gateway/model-fallback-card.tsx` | **新增**卡片（状态点、诊断带、增量触发标签、流水表） |
| `src/components/gateway/model-fallback-sheet.tsx` | **新增**编辑抽屉（主模型锁定、2 槽位、5 触发条件、固定不降级说明、纵向模拟路径） |
| `src/pages/ApiServicePage.tsx` | 接线（**只在 WorkBuddy 页**，见 §10.3） |
| `src/locales/domains/shared.zh.ts` / `.en.ts` | 约 60 个键，中英一一对应 |
| `src/lib/screenshot-demo.ts` | 演示模式快照（模型名与 `demoCatalog` 同源） |
| `crates/buddy-switch-gateway/tests/wb_model_fallback_e2e.rs` | **新增**：2 条路由级 e2e（闸门暂停真的跳到备选 / 不可降级不白跑一跳） |
| `crates/buddy-switch-gateway/src/routes/fallback.rs` | **新增**：`FallbackLoop` 骨架（算 hop 序列 / 改写 body 的 model / 跳过禁用目标 / 注入降级头）—— Trae 与 Qoder 共用 |
| `crates/buddy-switch-gateway/src/trae/routes.rs` | Trae 三个入口（chat / messages / responses）各包一层 hop 循环 |
| `crates/buddy-switch-gateway/src/qoder/routes.rs` | 拆出 `chat_for_region_once`，`chat_for_region` 外层包 hop 循环（三个协议入口共用） |
| `crates/buddy-switch-core/src/modules/upstream.rs` | `UpstreamErrorKind::Timeout`（传输层超时单列一档，对外 504） |

验证：`cargo test -p buddy-switch-gateway --lib` **773 passed**；`cargo check` 三个 crate 全过；`tsc --noEmit` 干净；`npm run build` 通过；`check-scope-registry` 331/0；`check-api-contract` 通过（routes=264）；浏览器控制台零错误。

### 10.2 已完成：原型

| 文件 | 改动 |
|:--|:--|
| `docs/prototype/demo-app.js` | `fallbackCard` / `fbDrawerHtml` / `fbSimHtml` / `fbStateOf` / `fbDiags` + 7 个动作 |
| `docs/prototype/demo.css` | `.fb-*` 段 |
| `docs/prototype/demo-data.js` | `fallback` 配置 + `models[].caps` |
| `docs/prototype/buddy-switch-prototype.html` | 生成产物（`npm run proto:single`） |

### 10.3 实现与设计的偏差（**如实记录**）

| 设计 | 实际 | 原因 |
|:--|:--|:--|
| 能力校验 | **只做图片**：请求带图 + 目标明确不支持 ⇒ 跳过该跳；卡片上也会标出「目标不支持图片输入」 | 目录里只有图片能力（`supports_images` + 用户覆盖）是**有权威声明**的。tools / json_schema / 上下文窗口都没有可靠数据 —— 猜一个阈值去拦会把本来能成功的请求跳掉，比不拦更糟 |
| 超时降级 | **已实现**（第三轮） | `UpstreamErrorKind::Timeout` 由 `reqwest::Error::is_timeout()` 判定（对外 504 / `type: timeout`），`UpstreamEvent::Timeout` 单列一档。冷却处置与 5xx 一致，**降级判据独立** |
| Trae / Qoder 网关 | **前后端都已接**：Trae 三个入口 + Qoder 三个协议入口包了 hop 循环；三个网关页都渲染同一张卡片 | 判定只能看**响应状态码**（5xx ⇒ 服务端故障、429 ⇒ 限流），比 WB 的「按上游事件类型」粗一档；**用户配的 `triggers` 白名单照常生效**。卡片通过 `scope` + `modelOptions` 两个 prop 平台化（Trae 的清单来自客户端 `vscdb`、Qoder 的来自上游实时拉取，都不在 gateway store 里） |
| **同平台跨区域**（`wb-cn` → `wb-global`） | ✅ **已支持** | 换 region 走正常路径即可：协议一致，只是账号池与上游域名不同 |
| **跨平台**（`wb-cn` → `trae-cn`） | ✅ **三个入口都支持**（限深一层） | 走 `routes::cross::relay_on_scope`（同进程内调目标产品线入口，协议都是 OpenAI 形态）。`A → B → A` 的循环由 **`task_local` 限深**挡住（`MAX_CROSS_DEPTH = 1`，超限 502 `fallback_cross_depth_exceeded`），不是靠禁止功能 |
| 请求体没有 `model` 字段 | **不降级**（三条线一致） | 客户端不传 model 时中继用配置默认模型 / 清单第一条，我们**不知道**是哪个 —— 改写 body 是 no-op，硬改会变成「每跳发同一个模型却告诉客户端降到了备选」= 对客户端撒谎 |
| Trae **流式**请求 | 基本不触发降级 | 上游错误多在 SSE 流内，此时响应头已发（200）⇒ 没有「这一跳彻底失败」的信号。非流式会降级（错误体完整、状态码 502）。**这是平台差异，不是 bug**，卡片文案要说清 |
| 失败时返回结构化 `fallback` 对象 | **已实现**：错误体顶层多一个 `fallback` 键（`Upstream` 错误同时追加一句人读文本） | 不能塞进 `GatewayError`（每个变体的状态码与 `type` 都被契约钉着），但**顶层加键是纯增量**。不用「只追加文本」是因为最后一跳可能是 `no_credential`（没有可变 message）—— 那正是最需要轨迹的时候（e2e 测试钉住） |
| 卡片显示「近 24h 降级 N 次」 | **只显示最近 N 条事件** | 环形缓冲不能做窗口统计（§8.2 第 5 点）。事件存在进程内（50 条），不落盘 |
| 统一网关 | **自动继承**（零改动） | `relay_llm` 只做命名空间分派，之后调产品线 `relay` ⇒ WB 的降级对统一网关同样生效 |

### 10.4 待做：P1 / P2

| 项 | 说明 |
|:--|:--|
| 能力校验（tools / json / 上下文） | 图片已做。其余三项要先有可靠的能力声明（目录或静态表），否则宁可放行 |
| 超时判据 | 触碰 `buddy-switch-core` 的 `UpstreamErrorKind`，需单独一轮回归 |
| Trae / Qoder 的**精细判据** | 现在只看状态码（5xx / 429）。要按事件判（封禁 / 超时 / 模型级 vs 账号级限流）就得改 `TraeErrKind` / `QoderErrKind` 的暴露方式 |
| Trae / Qoder 的**降级流水轨迹** | 只记「最终用了哪个备选」，不记「试过哪些」 | 这两条平台的换号轨迹不外露，拿不到完整路径 —— 宁可少记，不要编 |
| ~~账本降级维度~~ | ✅ **已实现**（`token_daily` v5 的 `fallbackDays`）：`record_fallback` **累加**（不是折叠 —— 降级是一次性事件，折叠会漏）+ `fallback_stats` 按窗口汇总 + `get_fallback_stats` 命令（桌面 + WebUI 共用 `stats_payload`）。卡片上显示「近 7 天：降级 N 次（成功 X · 失败 Y）」 |
### 命令契约

```rust
#[tauri::command] async fn get_model_fallback(scope: Scope) -> Result<ModelFallbackView, String>;
#[tauri::command] async fn save_model_fallback(scope: Scope, config: ScopeFallback) -> Result<ModelFallbackView, String>;
#[tauri::command] async fn preview_model_fallback(
    scope: Scope, from: String, targets: Vec<String>,
) -> Result<FallbackPreview, String>;
```

`preview` 是只读纯计算（读闸门/冷却/能力/禁用），不落盘、不发上游请求——**必须保证它零副作用**，否则「点一下预览就把闸门弄乱了」是灾难。

`ModelFallbackView` 在落盘结构之外多带三个**只读派生字段**（不参与持久化）：

```rust
// 实际形状（`model_fallback.rs`）—— 全部字段都是**只读派生**，不参与持久化。
pub struct ScopeFallbackView {
    pub scope: String,
    pub enabled: bool,
    pub chains: Vec<ChainView>,
    pub max_targets: usize,          // 前端据此决定「+ 备选」是否还能点
    pub baseline: Vec<Trigger>,      // 前端据此判断哪些 tag 算「增量」
    pub events: Vec<FallbackEvent>,  // 最近 50 条，**不参与任何统计口径**
}

pub struct ChainView {
    pub from: String,
    pub targets: Vec<TargetView>,    // { model, state: healthy|gated|disabled|unknown, gated_for_secs }
    pub triggers: Vec<Trigger>,
    pub enabled: bool,
}

pub struct FallbackEvent {
    pub at_ms: i64,
    pub from: String,
    pub path: Vec<String>,           // 真正尝试过的备选
    pub reason: String,              // Trigger::as_str（稳定标识符，前端映射回文案）
    pub account: String,
    pub ms: i64,
    pub result: &'static str,        // degraded | failed | blocked
}
```

> 设计稿里设想过的 `last_fallback_at_ms` / `warnings`（NoTools / NoVision / Cycle）**没有实现** ——
> 前者需要账本维度，后者需要能力校验（P1）。见 §10.3。

---

## 11. 门禁与测试

### 必须过的门禁

| 门禁 | 影响 |
|:--|:--|
| `npm run check:api` | 新命令必须同时登记 `DEMO_READ_COMMANDS` 与 `HTTP_ROUTES`（WebUI 模式要走 REST 映射） |
| `npm run check:scope` | 三处 `models` 分区接线必须一致；卡片必传 props 断言 |
| `npm run check:store-selectors` | selector 必须细粒度 |
| `cargo check -p buddy-switch-rust` | 改 `src-tauri/` 后必跑 |
| `cargo fmt -- src-tauri/` | **rustfmt 必须覆盖 `src-tauri/`**（两次因此打红 CI） |
| `cargo test -p buddy-switch-gateway` | 新增测试 |

### 测试（行为级，不是「函数返回 true」）

`model_fallback.rs` 单测：

1. `plan_for` 在 `enabled: false` 时只返回主模型（一条链都不给）。
2. 目标被禁用 ⇒ 不出现在 plan 里。
3. 目标与主模型同名 ⇒ 被剔除（不自环）。
4. 环路 `A→B`、`B→A` ⇒ 单次请求内 `B` 只出现一次。
5. `should_fallback`：402 / 400 / 审核 / `AuthRejected` 全部返回 `None`（**变异测试：把 `Client` 加进白名单，这条必须红**）。
6. 未知 scope 键原样保留、不清空已知 scope（同 `disabled_models` 的脚枪测试）。

`relay.rs` 集成测试：

7. 主模型闸门暂停 + 配了链 ⇒ **只发一次上游请求**（打到备选），`X-Buddy-Fallback-To` 正确。
8. ★ **探针票按 hop 归还**：主模型半开态领票 → 降级 → 断言主模型的 `HalfOpen` 立刻可被下一个请求领票（不是等 120s）。**这条是 §5.2 的守卫。**
9. 全链失败 ⇒ 错误体带 `fallback.tried`，`attempts` 仍只有四项。
10. 能力不匹配（带 tools 请求降级到无 tools 模型）⇒ 该跳被 skip 且**没有发出上游请求**。
11. 流式：首帧已发出后失败 ⇒ 不重发、不降级（客户端拿到中途断流）。
12. 账本记的是目标模型，`usage` 不跨模型相加。

---

## 12. 分期

| 期 | 范围 | 交付判据 |
|:--|:--|:--|
| **P0** | 同作用域、≤2 跳、触发条件固定为闸门/6004/11102、无能力校验、只读流水 | 闸门暂停时自动切到备选；配置热生效；探针票测试绿 |
| **P1** | 能力校验（tools/vision/json/context）、5xx 可勾选、诊断带、模拟预览、账本计数 | 带 tools 的请求不会降到无 tools 模型；卡片显示 24h 次数 |
| **P2** | 超时判据（触碰 `buddy-switch-core`）、跨作用域（经统一网关命名空间）、CSV 导出 | 独立 PR |

P0 刻意不含能力校验，是为了让「循环嵌套 + 探针票归还」这个最高风险点单独验证。**能力校验缺席时，P0 必须把 5xx 保持关闭**——否则降级到无 tools 模型的概率会显著上升。

---

## 13. 已知风险与取舍

| 风险 | 判断 |
|:--|:--|
| **降级掩盖真实故障**：主模型长期失败，用户以为一切正常 | 可接受但必须可观测：响应头 + 流水 + 卡片状态点三处同时暴露。**不做**「降级时弹窗」——那会让自动化调用被打断 |
| **5xx 降级放大故障**：故障期间流量全推备选 | 默认关闭 + 选项文案明示「可能把备选也打挂」。这是把决定权交给用户的正确做法 |
| **重复计费**：超时降级可能让上游已经计费 | 默认关闭超时降级。若 P2 开放，文案必须写明 |
| **配置被手工改坏** | `targets` 里的未知模型 ⇒ 该跳 skip 并报 `unknown_model`；整份文件损坏 ⇒ 回落空表（同 `disabled_models`），不 panic |
| **三条产品线行为漂移** | 配置是网关 crate 级共享、判断点唯一（`plan_for` / `should_fallback`）。**禁止**在 `chat.rs` / `messages.rs` / `responses.rs` 各自加降级分支 |
| **降级后客户端困惑** | 响应头是唯一解。文档 `docs/guides/` 需要补一段「如何判断响应被降级」 |

---

## 14. 落地顺序建议

1. `model_fallback.rs` + 单测（纯逻辑，可独立 review，无运行时影响）
2. `relay.rs` 拆分 `relay_once` + hop 循环 + 探针票归还 + 测试 8（**先写测试 8，再改代码**）
3. 命令层 + `types.ts` + `api.ts`（此时可用 curl 验证，无 UI）
4. 卡片（只读渲染）→ 弹窗（编辑）→ 模拟预览
5. 三处页面接线 + i18n + 门禁
6. 文档：`docs/specs/MODEL-FALLBACK.md`（本文件迁入）+ `docs/guides/` 补「降级判定」
