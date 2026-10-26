# 账号池选号：限流不换号 Bug 修复 + 「排除账号」功能 —— 完整方案

> 状态：**方案（待拍板，未写码）**。日期：2026-10-17。
> 需求原话：「不指定账号时账号池怎么选号？实际发现一个 bug：一个账号的某个模型被限流后，其他账号正常，
> 请求却一直失败，不是该切到没被限制的账号吗？另外要能排除账号池中某些账号。」
> 依据：**本仓源码实测**（行号可核实）。未核实处显式标注「待验证」。
>
> **第二轮（同日）**：追加 8 条优化（§8），并把整套交互做进了原型 ——
> `docs/prototype/buddy-switch-prototype.html` → 网关 → **账号池**
> （验证：`node docs/prototype/verify-pool-exclude.cjs`）。
>
> ## ✅ 第四轮：已按「全部 ①（推荐项）」落地（2026-10-17）
>
> 门禁：`cargo test --workspace --lib` **2015 通过 / 0 失败**；`cargo fmt --check` 通过；
> `npm run build`（tsc + 298 项契约检查 + vite）通过。**未提交**（工作区还有别的改动）。
>
> | 项 | 状态 | 落点 |
> |:--|:--|:--|
> | P0-1 冷却口径（60s 下限 + 只信结构化墙钟 + 成功即清 + 账号级封顶 15min） | ✅ | `pool/mod.rs`（`MIN_MODEL_COOLDOWN_MS`、`classify_event`、`apply_upstream_error`）、`pool/entry.rs`（`note_success`） |
> | P0-2 池非空不回落 `AccountSelector` | ✅ | `routes/relay.rs::select_account` |
> | P1-1 失败带 `attempts[]` | ✅ | `error.rs::UpstreamAttempt` + `openai_body`/`anthropic_body` |
> | §9.2 重试矩阵 + 模型级失败 2 次即停 + 每轮重取 `now_ms` | ✅ | `routes/relay.rs`（`should_rotate` / `is_model_level` / `MODEL_FAILURE_LIMIT`） |
> | O3 在途满排队 2s | ✅ | `relay.rs::acquire_with_wait` + `PoolConfig::acquire_timeout_ms` |
> | §4 排除账号（存储/选号/状态/端点/前端） | ✅ | `pool/exclusions.rs`（新）、`pool/pick.rs`、`routes/pool_admin.rs`（新）、`commands.rs`、`server/api.rs`、`account-pool-card.tsx` |
> | 随机档 `AccountStrategy::Random` | ✅（仅 WB 的 relay 生效） | `account_strategy.rs`、`pool/pick.rs::PickMode` |
> | 顺序档 `AccountStrategy::Sequential`（issue #102） | ✅（三平台共用；见 §10） | `pool/pick.rs::PickMode::Sequential`、`account_strategy.rs`、`routes/relay.rs`、三个网关页 |
> | O5 排除时按 uid 清粘性 | ✅ | `sticky.rs::purge_uid` + `pool_admin::set` |
> | 前端：排除按钮 / 已排除格 / 模型冷却徽标 | ✅（仅 WB 页） | `account-pool-card.tsx`、`ApiServicePage.tsx` |
>
> | O1 跨请求模型闸门 + 半开探测（含探针 120s 租约） | ✅ | `pool/gate.rs`（新）、`routes/relay.rs` |
> | P0-4 在途租约自愈 | ✅ | `pool/entry.rs::in_flight_now`、`pool/mod.rs::acquire` |
> | **P0-3 200 信封错误（非流式）** | ✅ | `protocol/error_sniffer.rs`（新）+ `chat`/`messages`/`responses` 三条路由 + `relay::note_envelope_failure` |
> | **P0-3 流内信封错误（流式）** | ✅ | `UsageTap::with_error_sink` + `relay::EnvelopeErrorSink`（旁听、不改字节、spawn 落治理）+ 末端残帧也判 |
> | **O6 模型 × 账号维度 EMA** | ✅ | `PoolEntry.model_health` + `weight_of_model`（样本 <3 回落账号级） |
> | **Trae / Qoder 策略枚举** | ✅ | `pool::parse_preferred`（哨兵值 `__auto__` / `__max_credits__` / `__random__`）+ 两平台 `pick` 内部解析 + 两页池卡下拉三档 |
>
> **全部 12 条拍板项 + §8/§9 推荐项 + O6 + 三平台策略枚举均已落地。**

> **第三轮（同日）**：① 去掉「本次请求的模型」选择器（模型属请求，不属网关设置）；
> ② 新增 **随机** 策略；③ 把「失败即换号」抽成**与策略无关的统一链路**（§9），
> 并加「模拟请求」把整条链路逐步演示出来。
> 当前门禁：**21 条断言 + 9 个变异**（`verify-pool-exclude.cjs`）。拍板前**先点一遍原型**。

---

## 0. 一句话结论

**选号内核本身是对的（模型级冷却会被正确跳过），坏在「冷却时长」与「兜底/流内错误」这两处旁路：**

1. **模型级 429 的冷却被写成 `now + 1ms`**（等于没冷却）——所以那个号在「这个模型」上从来没被真正排除，
   每次请求的第一跳都会重新撞它一次。代码里已经自己标注了这是已知缺陷（`pool/mod.rs:236`）。
2. **池给不出账号时回落 `AccountSelector`**，会把「池已判定不可用」的账号（桌面端当前登录那个）再交回来，
   这条路径上**换号根本没发生**（`routes/relay.rs:404`）。
3. **WB 侧 2xx + 流内/信封错误完全不做治理**，还会把粘性绑到这个坏号上 ⇒ 同一会话后续每轮都钉住它。

三条叠起来，症状恰好是「池里别的号明明可用，请求却一直失败，且看不出它在换号」。

---

## 1. 现状：不指定账号时，到底怎么选

### 1.1 全链路（WorkBuddy 网关；Trae / Qoder 结构同构）

```
请求 → relay::relay                          routes/relay.rs:124
  ① sync_accounts(账号库, realm)              增量 upsert，不删（保留治理历史）
  ② sticky_key = region|model|conversation_id 客户端没给会话 id ⇒ 整段跳过粘性
  ③ for attempt in 0..max_rotate（默认 3）    state.rs:108
       ├─ 读粘性 → preferred_uid
       ├─ select_account()                    relay.rs:364
       │    偏好 = Pinned（显式指定）> 粘性
       │    pool.pick_account(preferred) → pick_with_preference()
       │    偏好不可用 ⇒ 自动回落通用选号
       │    池给不出 ⇒ 回落 AccountSelector（current/pinned/max_credits）★旁路
       ├─ acquire(uid)  在途租约             relay.rs:196
       ├─ 出站（LLM / 业务）                  relay.rs:204
       ├─ release(uid)                       relay.rs:239
       └─ Ok  ⇒ note_success + 绑粘性 + 返回
          Err ⇒ classify_event + apply_upstream_error + tried.insert + 解绑粘性 + 继续
```

### 1.2 选号算法（`pool/pick.rs`，三平台共用一份）

`pick()`（`pick.rs:205`）五步：

| 步 | 逻辑 | 位置 |
|:--|:--|:--|
| 1 过滤 | 非 `tried` ∧ `entry_usable`（`healthy_for_request(now, model)` + 未占满在途）∧ realm 匹配 | `pick.rs:220` |
| 2 兜底 | 无候选 ⇒ `pick_earliest_expiry`：只挑「**有账号级冷却**且最早解冻」的（硬冷却除外） | `pick.rs:232 / 414` |
| 3 权重 | `1 + 积分占比×10 + 快过期占比×expiring_weight + 到期紧迫度×w + 闲置补偿(0.5/h,封顶5) + 成功率EMA×3` | `pick.rs:105` |
| 4 分层 | 成本硬过滤：`Free > Unknown > Paid`（保证无观测号仍有机会被观测） | `pick.rs:259` |
| 5 抽签 | 单价升序→权重降序 → Top-5 短名单 → 防惊群（100ms 内不重复）→ **加权抽签**（WB 随机；Trae/Qoder `deterministic=true` 取第一） | `pick.rs:282-321` |

**关键点**：模型级限流只影响第 1 步（`healthy_for_request` 看 `model_cooldowns`）。
也就是说 —— **只要冷却真的存在，选号一定会跳过它**。这就是为什么根因不在算法，而在「冷却没被写进去」。

### 1.3 失败治理（`Pool::apply_upstream_error`，`pool/mod.rs:834`）

| 事件 | 处置 | 冷却位置 |
|:--|:--|:--|
| 402 / 余额文案 | 硬冷却至次日 04:00 | 账号级 |
| 429 + `6004`/`IsModelRateLimit` | **模型级**冷却（看墙钟，封顶 `soft_rate_max`=2h） | `model_cooldowns` |
| 429 无模型标记 | 账号级软冷却（墙钟 or 指数退避） | `until_ms` |
| `11102` | 模型封禁退避 6h→12h→24h | `model_cooldowns` |
| 404 | 固定 60s | 账号级 |
| 5xx | 熔断 30m→1h→2h→4h→6h（3 次触发） | `breaker_until` |
| `12153` | 三振禁用（终态，需人工恢复） | `disabled` |

---

## 2. Bug 诊断：5 个真实缺陷（按「能否解释症状」排序）

### D1 ★ 模型级 429 的冷却退化为 1ms，等于没有冷却（最直接）

```rust
// pool/mod.rs:868  真实流量这条路径
entry.cooldown_model(now, model, reset_at.unwrap_or(0), soft_rate_max, "6004 model rate limit");

// pool/entry.rs:417
let until = if reset_at_ms > cap { cap }
            else if reset_at_ms > now_ms { reset_at_ms }
            else { now_ms.saturating_add(1) };   // ← 上游没给墙钟 ⇒ 冷 1 毫秒
```

- 上游 `6004` 的响应体**通常没有 `resetTime` 这类结构化字段**（有也多是文案里的配额窗口，见 D2）⇒ `reset_at = None` ⇒ `now+1ms`。
- 后果：该账号在「这个模型」上**从未被真正排除**。下一个请求是新的 `tried` 集合，重新抽签，
  它照样是权重最高那个（积分最多 / 免费层 / 闲置最久）⇒ **每个请求的第一跳都撞它一次**。
- `max_rotate=3` 时单请求还能靠重试救回来，但：
  - 并发/批量请求（agent 一轮发 N 个）时，N 个请求**同时**第一跳都撞它；
  - 可用账号只剩 1 个、或其它号在途占满时，3 次重试全撞同一堵墙 ⇒ 用户看到的就是「一直失败，别的号明明可用」。
- **代码已自认**：`pool/mod.rs:236-239`「既有问题…对上游没给墙钟的模型级 429 同样落 `now + 1ms`（等价于不冷却）…须单独立项」。

### D2 ★ 把 6004 文案里的 `reset at <T>` 当解锁时刻 ⇒ 另一个方向的错判（过度冷却）

- `parse_reset_at_ms`（`pool/mod.rs:324`）会解析 `… reset at 2026-09-16 16:34:24 UTC+8`，
  `cooldown_soft_until` 直接拿它当解冻时刻，封顶 `soft_rate_max = 2h`。
- 但 `client_limits.rs:29-37` 已**实测**：那是**配额窗口重置时刻，不是解锁时刻**
  （17:20 报「reset at 次日 09:04」，17:36 就恢复；「实测 6/6 全错」）。
- 后果：一次 429 可能把账号（账号级）或模型（模型级）冷 2 小时，而实际十几分钟就恢复。
  **同一份数据，账号级冷 2h、模型级冷 1ms** —— 两个极端都在同一处代码里，这本身就是缺陷信号。

### D3 ★ 池给不出账号时，回落 `AccountSelector` 会把坏号交回来（换号在这条路上不存在）

```rust
// routes/relay.rs:404
if let Some(uid) = picked_uid {
    if let Some(account_value) = account::find_account_for(region, &uid) { return Some(account_value); }
}
// ↓ 池给不出 ⇒ 回落策略
match selector.select(region, &strategy).await { ... }   // 默认 Current = 读桌面端认证文件
```

- 该回落**只判 `tried`**，不看模型级冷却、不看熔断、不看在途 —— 它绕过了池的全部治理。
- 触发链：A 被模型限流（真冷却）∧ B 在途占满/禁用 ⇒ 池返回 `None` ⇒ 回落把 A 交回来 ⇒ 再 429 ⇒
  A 进 `tried` ⇒ 下一轮回落还是 A ⇒ `None` ⇒ 直接返回错误。**三次重试全打在同一堵墙上。**
- 这正是「正常不是该切换到没被限制的账号吗」的正面回答：**在这条兜底路径上它不会切。**

### D4 ★ WB 侧 2xx + 流内/信封错误完全不做治理，还会绑粘性

```rust
// routes/relay.rs:244
UpstreamChatResult::Ok(ok) => {
    pool.note_success(...);                       // 记成功
    state.sticky.write().await.bind(key, &uid);   // ★ 把粘性绑到它身上
    return Ok(RelayOutcome::Ok { response: ok.response, ... });  // 原样透传（含错误帧）
}
```

- WB 上游在部分场景用 **HTTP 200 + `{"code":6004,...}` 信封** 或 **SSE `event:error`** 报限流。
  Trae（`trae/routes.rs:14` 的表格 + `record_stream_error`）与 Qoder（`qoder/routes.rs:653` 的
  `aggregated.upstream_error`）都有专门处理，**WB 这一层是缺的**。
- 后果：该账号**不会被冷却**，粘性还**被绑到它身上** ⇒ 同一会话后续每一轮都优先落到这个号上，
  **一直失败且换号完全没发生**。这是唯一能确定性解释「一直失败 + 完全没换号」的路径。
- 流式请求尤其严重：响应头已发出，客户端拿到的是流内错误帧，网关侧零治理。

### D5 ★ 在途租约可能泄漏 ⇒ 账号永久「在途占满」（结构性）

- `acquire`（`relay.rs:196`）与 `release`（`relay.rs:239`）是**手写配对、无 RAII**。
- 客户端断开 / 请求 future 被 drop（上游挂住后客户端取消）时，`release` 不执行 ⇒ `in_flight` 永久 +1。
- 达 `max_in_flight`（CN 3 / Global 2）后 `entry_usable` 恒 false ⇒ 该号**再也选不中**；
  而 `status_of` / `status_list` 里它仍显示 `available`（前端池卡没有 in_flight 列）。
- **与用户描述「池里账号看起来正常、请求却一直失败」高度吻合**，且只能从 `/status` 的
  `in_flight_full` 字段看出来（`pool/mod.rs:1294`）。

### D6 ★ 「按实时积分择优」在聊天选号里几乎不生效（**假控件**）

- UI（`AccountPoolCard` 的「指定账号」下拉）把它作为与「不指定 / 指定某号」并列的第三档，
  文案承诺「**每次请求查询**」。
- 但后端只有一处把策略读进**选号偏好**，且只认 `Pinned`：

  ```rust
  // routes/relay.rs:383
  let preferred = match &strategy {
      AccountStrategy::Pinned { account_id } if !account_id.is_empty() => Some(account_id.as_str()),
      _ => sticky_uid,          // ← MaxCredits 落到这里，等于没参与选号
  };
  ```
- `MaxCredits` 只在**兜底**里被执行（`relay.rs:415` 的 `AccountSelector.select`），
  而兜底的前提是「池给不出账号」—— 池非空时永远走不到。
- ⇒ 实际效果：**选了这个模式，聊天请求仍然按池的权重抽签走**。它真正生效的地方是
  `catalog_probe`（用积分最高的账号去拉 `/v1/models` 清单），与「每次请求选号」是两回事。
- 这是本仓明确反对的形态（参考 `dual_port` 的教训：「有字段、有开关、没有读取方」）。
  **要么把它提升为一等策略，要么把文案改成「拉模型清单时生效」**（见 §5 决策 10）。

### 2.1 先做一次「确诊」，别盲改

现有数据足够区分是哪一条在生效，建议按顺序看：

| 检查 | 判读 |
|:--|:--|
| `GET /status` → `accounts[].modelCoolings` | 若该账号该模型**不在表里**（或 until 只比 now 大几毫秒）⇒ **D1 命中** |
| `GET /status` → `accounts[].in_flight` / 汇总 `in_flight_full` | 非 0 且长期不降 ⇒ **D5 命中**（在途泄漏） |
| `GET /status` → `diagnose[]` | 若显示「可用」但你请求失败 ⇒ 看下一条 |
| 请求日志 `lastError` + 客户端收到的 body | 是 `429/6004` 还是「no_credential / 上游拒绝」⇒ 区分 D3（no_credential）与 D4（透传错误帧） |
| 手动把该号在客户端切走 / 临时禁用 | 若立刻恢复 ⇒ 确认「请求一直落到它身上」（D1 或 D4） |

> ⚠️ 若日志里能拿到「一次请求尝试了哪几个账号」，直接就能定位。当前失败响应只带**最后一条**上游错误，
> 所以 P1 里有一条「失败响应带上本轮尝试明细」。

---

## 3. 修复方案（P0 必修 / P1 观测 / P2 收口）

### P0-1 冷却口径收敛：模型级有下限，且不再采信文案墙钟

**改法**

1. `pool/mod.rs` 新增常量（与 Qoder/探测回填同值，避免三处漂移）：
   ```rust
   pub const MIN_MODEL_COOLDOWN_MS: i64 = 60_000;   // 60s
   ```
2. `parse_reset_at_ms` 拆成 `from_json` / `from_text` 两个出口（保留现状作为 `or_else` 组合，
   但**冷却只用 `from_json`**）：
   - 结构化字段（`resetTime` 等）→ 可信，可用作冷却墙钟；
   - 文案兜底（`… UTC+8`）→ **只用于展示/诊断**，不进冷却（依据 `client_limits.rs` 的 6/6 实测）。
3. `apply_upstream_error` 的模型级分支：
   ```rust
   let reset = reset_at.filter(|v| *v > now_ms + MIN_MODEL_COOLDOWN_MS)
                       .unwrap_or(now_ms + MIN_MODEL_COOLDOWN_MS);
   entry.cooldown_model(now_ms, model, reset, config.soft_rate_max_ms, "6004 model rate limit");
   ```
4. `PoolEntry::note_success` 增加「该模型成功 ⇒ 清该模型的冷却」：
   ```rust
   self.model_cooldowns.remove(model);   // 现在明确不清（entry.rs:257），改为清
   ```
   理由：成功是最强的反证（与 `client_limits` 的「反证优先」同一哲学）。
5. 账号级 429 的 `soft_rate_max_ms` 从 2h 收到 **15min**（新增 `rate_window_reset_max_ms`，
   与「模型封禁 24h / 熔断 6h」分档，别再用同一个 2h 兜所有）。

**效果**：D1（1ms）与 D2（2h）两个极端都收敛到「60s ~ 15min，成功即恢复」。

**风险**：模型被封禁（`11102`）的场景**不受影响**（走 `block_model_backoff`，6h→24h 保持）。
账号级冷却变短会让「真·额度耗尽」的号多撞几次 429 —— 但 402 走硬冷却，不受此影响。

### P0-2 兜底顺序重排：池非空时**只信池**，不再回落 `AccountSelector`

**改法**（`routes/relay.rs:364 select_account`）

```
池非空 → pick_account()：有结果就用；无结果 ⇒ 直接返回 NoCredential{ cause: PoolExhausted{...} }
池为空 → 回落 AccountSelector（此时才真的可能是「没登录/没导入」）
```

- `NoCredentialCause` 增一档可诊断信息：`{ total, excluded, model_limited, in_flight_full, disabled, cooling }`
  —— 让用户一眼看到「3 个号里 1 个该模型被限流、1 个在途占满」而不是笼统「没有可用账号」。
- 判据**复用** `pick::entry_usable` / `PoolEntry::rejection`（不新写第二套，遵守本仓纪律）。

**风险**：`Current` 策略在「池里有号但都不可用」时不再兜底 —— 这正是我们要的（兜底会把坏号交回来）。
文案需要同步改（`error.rs` 的 `no_credential` 档）。

### P0-3 流内/信封错误治理（WB 侧补齐）

**非流式**（响应头未发出，**可以换号**）
- `chat.rs` 的 `CompletionAccumulator::from_sse(&text)` 之后加一步：若 `text` 不是 SSE 且是
  `{"code":!=0}` / `{"error":...}` 信封 ⇒ 走 `classify_event(映射出的 status, body)` +
  `apply_upstream_error` + `tried.insert` ⇒ **重新进入 relay 换号**（需要把 relay 的循环拆一层，
  或在 `chat_for_region` 里加「信封错误 ⇒ 再调一次 relay」的有限重试）。

**流式**（响应头已发出，**只能记冷却 + 解绑粘性**）
- 在 `UsageTap` 链上加一个 `ErrorSniffer`（只旁观帧，不改写字节），流结束时若发现限流类错误：
  `apply_upstream_error(uid, model, event)` + `sticky.unbind(key)`。
- 与既有 `StreamingLogSink` 并列成 `StreamingGovernanceSink`，复用 `stream_sinks()` 的组装点
  （`relay.rs:620`）—— 三条 WB 路由自动都生效，不会「修两条漏一条」。

**为什么流式不换号**：首帧到达时 `200 + text/event-stream` 已发给客户端，换号等于换一个响应体，
协议上做不到（要做得等首帧再决定，代价是首字节延迟 + 全链路改造）。**这一条需要你确认**（见 §5 决策 4）。

### P0-4 在途租约自愈（修 D5）

**改法**（推荐「租约带到期时刻」，不引入 Drop 语义）

```rust
// PoolEntry
in_flight: Vec<i64>,          // 每个在途请求的到期时刻（now + 600s）
fn in_flight(&self, now: i64) -> i64 { self.in_flight.iter().filter(|t| **t > now).count() }
```
- `acquire(uid, now)`：`prune_expired(now)` 后 push(now + LEASE_TTL)；
- `release(uid, now)`：prune 后 pop 最早一个（幂等）；
- `entry_usable` 用 `in_flight(now)` 判定 ⇒ **泄漏的租约 10 分钟后自动失效**，账号自愈。

**为什么不用 RAII**：`relay` 里 `acquire` 与 `release` 跨 await 点，Drop guard 需要在
`tokio::sync::RwLock` 上做 `try_write`（拿不到锁就跳过 = 还是泄漏）。带到期时刻的方案无锁、自愈、可测。

### P1-1 可观测（让「下次再出问题」5 分钟内定位）

1. `status_list` / `accounts_view` 每行补 `inFlight`（前端池卡加一列或 hover 提示）。
2. `PoolSummary` 补 `in_flight_full`（正交计数，与 Qoder 的 `model_cooling` 同一处理）。
3. **失败响应体**带上本轮尝试明细：
   ```json
   { "error": { "message": "...", "attempts": [
       {"account":"A","reason":"429 model rate limit (6004)"},
       {"account":"B","reason":"in_flight full"} ] } }
   ```
4. 请求日志补 `attempts` 字段（`log_keep` 内可见）。

### P1-2 让「模型级限流」在池卡上可见

WB 池卡现在只有「可用 / 冷却 / 禁用 / 零积分」四格，而 Qoder 有 `modelCooling` 一格。
WB 已有 `modelCoolings` 字段（`pool/mod.rs:1146`），建议池卡同样加「模型冷却」格 —— 
否则用户永远看不出「这个号只是这个模型被限流」。

### P2 选号决策收口为可测函数

把 `(policy, excluded, tried, realm, model)` 收成 `PickContext`，`pick` 系列只接一个结构体。
好处：**排除账号（§4）只需加一个字段**，不会出现「五个调用点各传一遍、漏一个」。

---

## 4. 新功能：排除账号池中的某些账号

### 4.1 语义（**需拍板**，见 §5 决策 1）

| 方案 | 含义 | 影响面 |
|:--|:--|:--|
| **A 硬排除（推荐）** | 自动选号、全冷却兜底、粘性、**显式指定**都不使用它 | 语义唯一、可预测；指定被排除账号时 UI 直接拦 |
| B 软排除 | 只排除自动选号；显式指定仍可用 | 多一套优先级规则，用户容易困惑「我排除了怎么还在用」 |

**推荐 A**，并在 UI 上把「排除」和「禁用」的差别写清楚：
> 排除 = 网关**永不**使用该账号（含兜底与指定）；账号本身仍可在桌面端 / 客户端正常使用。

### 4.2 数据模型与存储

**单文件、按 scope 分组**（`~/.buddy-switch/gateway_pool_exclusions.json`，与 `gateway_config.json` 同目录）：

```json
{
  "version": 1,
  "scopes": {
    "wb:cn":      { "u_abc": { "note": "留给客户端自己用", "at": 1760000000000 } },
    "wb:global":  {},
    "trae:work":  { "u_xyz": { "note": "", "at": 1760000000000 } },
    "trae:cn":    {},
    "qoder:cn":   {},
    "qoder:global": {}
  }
}
```

- **为什么单文件而不是三份配置**：排除是「用户对这个号的意图」，与产品线无关；
  一个文件 = 一个读写入口 = 三平台行为不可能漂移（本仓反复强调「唯一实现」）。
- **为什么按 scope**：账号库本就按 region / 变体分家，同一 uid 不会跨 scope 复用。
- 写：`core_config::atomic_write`（原子）；读失败 ⇒ **空集 + 记一条诊断**（fail-open，
  绝不因为配置文件坏了就停止服务，但 UI 顶部要提示「排除表读取失败，已按未排除运行」）。

### 4.3 选号层落地（**一个谓词，四个出口**）

```
pool/pick.rs
  struct PickContext<'a> { policy, excluded: &'a HashSet<String> }
  fn pick(...)                  ← 过滤条件与 tried 同级（pick.rs:220 同一处）
  fn pick_with_preference(...)  ← 偏好可用性也要过 excluded（pick.rs:361）
  fn pick_earliest_expiry(...)  ← 兜底同样排除（pick.rs:414）
  fn is_usable(...)             ← 粘性/指定共用
```

- `Pool::pick_account(..., excluded)`：从 `state.exclusions` 取当前 scope 的集合（进程内缓存 + 写时失效）。
- `TraePool::pick` / `QoderPool::pick`：在 `sync_for` 里读同一份文件（与冷却同源），
  避免「读侧读了、选号没读」这类半成品。
- `AccountSelector` 回落路径（P0-2 之后基本只剩「池为空」）**也过同一个谓词**。

### 4.4 状态面（前端契约）

| 位置 | 改动 |
|:--|:--|
| `PoolSummary` | 加 `excluded: usize` —— **归入互斥五态（成为第六态），不是正交计数** |
| `status_list` / `accounts_view` | 每行加 `excluded: bool`、`excludedNote?: string` |
| `diagnose` | 输出「已排除（用户设置）」；**判定顺序：排除 > 禁用 > 冷却 > 零积分**（用户意图最该先说明） |
| `no_credential` | 新增 cause 档 `AllExcluded { total, excluded }`，文案给「去账号池取消排除」而不是「去登录」 |

> ★ **2026-10-17 修正（原型验证后改口径）**：初稿把 `excluded` 设计成**正交计数**（像 Qoder 的
> `model_cooling`）。做原型时发现这与「硬排除」语义冲突：若排除项仍算在 `available` 里，
> 卡片会说「4 可用」，而实际只有 3 个能被选中 —— **数字与事实不符**。
> ⇒ 排除必须从 `available` 里扣掉。既然扣掉了，它就已经是**互斥态**，那就老老实实做成第六态：
> `available + cooling + disabled + zero_credits + excluded == total`。
> 正交的只有 `model_cooling` 与 `in_flight_full`（它们不改变五态，只是叠加标记）。

### 4.5 API（命令层 + server 层**双面同步**，本仓既有先例）

```
GET  /api/gateway/pool/exclusions?scope=wb:cn
     → { scope, items: [{ uid, name, note, at, inStore: true }] }
POST /api/gateway/pool/exclusions
     { scope, uid, excluded: bool, note?: string } → 返回最新列表
```

- 三平台**共用一份命令**（`src-tauri/src/commands.rs` 一处 + `crates/buddy-switch-server/src/api.rs` 一处），
  scope 参数决定落到哪一段 —— 不写三份。
- `inStore` 用于把「已删除账号的排除项」标出来（见 §5 决策 5）。

### 4.6 UI（`src/components/gateway/account-pool-card.tsx`）

1. **行内按钮**：与现有「指定 / 取消指定」「恢复」同一按钮组，加「排除 / 取消排除」。
2. **行样式**：排除后整行降透明度 + `Badge`「已排除」（与「已指定」徽标同级）。
3. **计数格**：加一格「已排除」（`tiles` 参数已有扩展位，`PoolTileKey` 加一项即可）。
4. **「指定账号」下拉**：被排除的账号**置灰并标注**（避免出现「指定了一个永不使用的号」这种自相矛盾状态）。
5. **冲突提示**：把当前 pinned 的号排除时，弹确认「将同时取消指定」。
6. **说明文案**（`shared.gateway.pool.excludeHint`）：
   > 排除后网关**不会**用它（包括兜底与指定），但它仍可在桌面端正常使用。适合「留额度给客户端自己用」这类场景。

### 4.7 测试清单

| 层 | 用例 |
|:--|:--|
| `pick` | ① 排除项不出现在候选；② 排除项不被 `pick_earliest_expiry` 兜底；③ 排除项不会被偏好/粘性选中；④ 取消排除后立刻恢复可选 |
| 三平台 | 各一条「排除后不再被选中」（WB 池 / Trae 池 / Qoder 池） |
| 契约 | `status_list` 的 `excluded` 字段；`summary.excluded` 与五态计数正交（`available+cooling+disabled+zero==total` 不变） |
| 端点 | GET/POST 往返 + 并发写（原子写不产生半截文件） |
| 回归 | 排除表缺失 / 损坏 ⇒ 空集 + 服务照常（fail-open） |

---

## 5. 需要你拍板的 12 件事

| # | 决策点 | 选项 | 我的建议 |
|:--|:--|:--|:--|
| 1 | 排除语义 | A 硬排除（含指定）/ B 只排除自动选号 | **A**（语义唯一，UI 上把「排除 ≠ 禁用」写清楚） |
| 2 | 429 冷却口径 | ① 不采信文案 `reset at`，模型级固定 60s + 成功即清；② 保留文案墙钟但加 60s 下限 | **①**（`client_limits` 实测 6/6 判错，留着就是拿错数据当真相） |
| 3 | 池非空但全不可用时 | ① 取消 `AccountSelector` 回落，直接报错 + 诊断；② 保留回落（现状） | **①**（回落会把已知坏号交回来，是本 bug 的 D3） |
| 4 | 流式流内错误 | ① 只记冷却 + 解绑粘性（不换号）；② 等首帧再决定是否换号（首字节延迟↑） | **①**（② 要改全链路，收益不抵成本） |
| 5 | 排除项生命周期 | ① 账号删除后保留（重新导入仍排除）；② 账号删除即清理 | **① + UI 标「账号已删除」+ 一键清理** |
| 6 | 落地范围 | ① 三平台全做；② 先 WB，Trae/Qoder 下一批 | **①**（`pick` 内核共用，加一个参数三处都受益，分批反而要改两遍） |
| 7 | 收费号兜底（O2） | ① 默认**关**（宁可报错也不偷偷花积分）；② 默认开（沿用现状，静默用收费号） | **①**（花积分必须可见；开关放池卡工具条） |
| 8 | 模型闸门（O1） | ① 做（阈值 2 个账号 / 窗口 10 分钟 / 暂停 60s 起翻倍封顶 15 分钟）；② 不做，只靠单账号冷却 | **①**（这是「3 次重试全撞一堵墙」的唯一根治） |
| 9 | 在途排队（O3） | ① 排队 2s 再换号；② 维持「满了立刻换号」 | **①**（并发场景不把「忙」误报成「不可用」） |
| 10 | 「按实时积分择优」到底是啥（D6） | ① **提升为一等策略**：候选内按积分降序（仍受排除/冷却/在途约束），与指定互斥；② 维持只做兜底，把文案改成「拉模型清单时生效」 | **①**（否则它就是个假控件；改动很小：`select_account` 里多传一个 `prefer` 给 `pick`，按积分排序替代权重抽签） |
| 11 | **随机档**是否进枚举（§9.3） | ① 加 `AccountStrategy::Random`（serde 标签 `"random"`，老配置不受影响）；② 不做，随机就是「自动择优」 | **①**（自动择优的加权抽签是**有偏**的；用户要的「平均分摊」只有均匀抽签能做到） |
| 12 | 重试矩阵是否按失败类型收敛（§9.2） | ① 按矩阵：参数错不重试、404 只换 1 次、模型闸门立即停；② 维持「一律重试 `max_rotate` 次」 | **①**（现在 400 也会白撞 3 个号，白烧配额；且「模型整体限流」时 3 次重试全打在同一个模型上） |

---

## 6. 落地顺序与改动面

| 阶段 | 内容 | 改动文件（预估） | 风险 |
|:--|:--|:--|:--|
| **S1** | P0-1 冷却口径 + P0-2 兜底重排 + **§9.2 重试矩阵** + P1-1 失败明细 | `pool/mod.rs`、`pool/entry.rs`、`routes/relay.rs`、`error.rs` | 中：改变真实流量的冷却时长与重试次数，需盯一轮实测 |
| **S2** | P0-4 在途租约自愈 + O3 在途排队 + P1-2 池卡「模型冷却」格 | `pool/entry.rs`、`pool/mod.rs`、前端池卡 | 低（同一改动面，一起做最划算） |
| **S3** | P0-3 流内/信封错误治理 | `routes/chat.rs`、`routes/relay.rs`、新增 `protocol/error_sniffer.rs`、`protocol/usage_tap.rs` | 中高：三条路由共用链，要一次改全 |
| **S4** | O1 模型闸门 + 半开探测（含 `model_probe` 复用） | 新增 `pool/gate.rs`、`routes/relay.rs`、`model_probe.rs` | 中：新增一道选号前短路 |
| **S5** | §4 排除账号（后端：存储 + pick + 三平台 + API 双面）+ O5 清粘性 + **随机档** | `pool/pick.rs`、三个 pool、`commands.rs`、`server/api.rs`、新增 `pool/exclusions.rs`、`sticky.rs`、`account_strategy.rs` | 中 |
| **S6** | §4 排除账号（前端 + 文案 + 三平台联调）+ O2 收费兜底开关 | `account-pool-card.tsx`、三个 Page、`locales/*`、`lib/types.ts` | 低 |
| **S7** | O6 模型维度 EMA / O7 文件缓存 / O8 `now_ms`（可选，P2） | `pool/entry.rs`、`account.rs`、三个 pool | 低（纯内部质量） |

> 建议 S1 先单独上，因为它**不需要任何 UI**，且能直接验证「限流后是否换号」是否恢复
> （用 `modelCoolings` 字段是否出现 60s 以上的 until 来判定）。
> S2 与 S3 可以并行（改动面不重叠）；S4 依赖 S1 的冷却口径；S5 依赖 S1 的 `pick` 参数化。

---

## 7. 附：这次**不改**的东西（避免顺手扩大范围）

- 加权抽签 / 短名单 / 防惊群的算法本体（行为正确，改了要重跑全部选号用例）。
- Trae 侧「没有模型级冷却」这件事（属 `MODEL-LIMIT-PROBE-PLAN.md` §0 的已知缺口，
  要等探测方案落地时一并解决；本次只在 Trae 侧加「排除」）。
- `11102` 模型封禁的 6h→24h 退避（语义正确）。
- 会话粘性的键设计（`region|model|conversation_id` 是对的，模型进键正好让「模型级限流」不被粘性抵消）。

---

## 8. 追加优化（第二轮：做完原型后新增）

> 原型：`docs/prototype/buddy-switch-prototype.html` → 网关 → **账号池**。
> 验证：`node docs/prototype/verify-pool-exclude.cjs`（14 条 + 6 个变异，全绿）。
> 下表标「原型可见」的，打开页面点一遍就能判断要不要做。

### O1 模型级熔断 + 半开探测（**原型可见**，建议做）

**问题**：模型级冷却只是「单账号 × 单模型」的标记。真实高频场景是**同一模型在多个账号上一起被限流**
（上游限流常按时段/账号组生效）。此时每个请求都会把每个账号各撞一遍 —— `max_rotate=3`
正好把 3 次重试全烧在同一堵墙上，最后只回一句「没有可用账号」。

**做法**：新增进程内「模型闸门」：

```rust
struct ModelGate { model: String, accounts: usize, until_ms: i64, phase: GatePhase }
enum GatePhase { Paused, HalfOpen, Closed }
```

- 同一模型在 ≥`gate_threshold`（建议 2）个账号上、`gate_window_ms`（建议 10 分钟）内被限流 ⇒ `Paused`；
  暂停时长 `min(60s × 2^(n-2), 15min)`；
- 到期 ⇒ `HalfOpen`：**只放 1 个请求**（single-flight）去试探；成功 `Closed`（清空计数），失败退避加倍；
- 选号**之前**短路：`Paused` 时直接返回可诊断错误（「`gpt-5-codex` 已被 2 个账号限流，整体暂停 42s」）。

**收益**：把「N 个请求 × M 个账号」的上游浪费压成「1 个探测请求」；错误信息从「没有可用账号」
变成能指导行动的一句话。**代价**：一份进程内状态（不持久化）+ 选号前多一道判断。

### O2 收费号兜底开关（**原型可见**，建议做，默认关）

**问题**：`cost_tier` 分层是「免费 > 无观测 > 收费」，但免费号全不可用时，当前实现会**静默**切到收费号
—— 用户完全不知道自己积分被花了。

**做法**：`allow_paid_fallback`（默认 `false`）。关闭时若候选只剩 Paid 层 ⇒
`NoCredentialCause::PaidOnly`，文案「要么打开开关，要么等免费号恢复」。

**代价**：默认关会让一部分「本来能成功」的请求变成失败 ⇒ 开关必须放在池卡显眼位置（原型已放）。
这是**安全默认 vs 成功率**的取舍，需要拍板（见 §5 决策 7）。

### O3 在途满改为「有界排队」（建议做）

**问题**：`acquire` 失败 ⇒ 立即 `tried` + 换号 ⇒ 无号 ⇒ 直接报错。客户端并发稍高（并行 tool calls）
就表现为「网关拒绝服务」，而账号只是**此刻**忙。

**做法**：`acquire_timeout_ms`（默认 2000，0 = 关闭）。等一个租约释放再发，超时才换号；
等待用 `tokio::sync::Notify`（不占 CPU），池上记 `waiting` 计数供观测。

**代价**：最坏 +2s 延迟。与 P0-4（租约自愈）一起做最划算 —— 同一个改动面。

### O4 选号解释 + 失败明细（**原型可见**，建议做）

**问题**：失败只回最后一条上游错误，用户无法回答「换号到底发生了没有」。

**做法**：① 失败响应带 `attempts[]`（账号 + 拒绝原因 / 上游状态）；
② 池卡直接给「本次请求会选中谁 / 跳过了谁 / 为什么」，由池状态**派生**（原型里这套派生函数
就是真实判据顺序的复刻：排除 > 禁用 > 账号冷却 > 零积分 > 模型冷却 > 在途满 > 成本分层）。

### O5 排除/禁用时按 uid 清粘性（**原型可见**，建议做）

**问题**：粘性表只按 key 解绑。账号被排除/禁用后，指向它的绑定仍在 —— 每个请求都要先
`is_usable` 判一次再解绑（白跑一跳），UI 上「粘性」徽标还挂在已排除的行上，自相矛盾。

**做法**：`sticky.purge_uid(uid)`，在**排除 / 禁用 / 删除账号**三处调用（三处都要，漏一处就漏一种状态）。

### O6 模型 × 账号维度的成功率 EMA（可选，P2）

**问题**：`success_ema` 是**账号级**。一个号在某模型上老失败、别的模型很好，会被整体降权（错杀），反之亦然。

**做法**：EMA 键改 `(uid, model)`（每号最多留 8 个模型，LRU 淘汰），权重里用「该模型」的 EMA，
无记录回落账号级。**代价**：状态变多；持久化要兼容（老数据按账号级读入，写回新结构）。

### O7 账号库 / 冷却文件的 mtime 缓存（性能，P2）

**问题**：WB 每请求 `load_accounts_for`（读盘 + 解析）；Trae / Qoder 每请求 `sync_for` 读 3 个 JSON。
并发上来后是纯浪费（`history/perf-audit-2026-09-24.md` 已记过同类问题）。

**做法**：`(path, mtime, len)` → 解析结果 的进程内缓存（纯 mtime 失效，无 TTL），三平台共用一份。
正确性不变：文件一改，mtime 立刻变。

### O8 `now_ms` 每轮重取（细节，随手改）

`relay` 只在入口取一次 `now_ms`，重试期间冷却到期看不见。3 次重试跨秒级，影响小，
但与「冷却靠墙钟」的语义不一致 —— 每轮重取一行的事，顺手改掉。

### 8.1 原型的实现边界（避免误读）

- **没有「本次请求的模型」选择器**：模型是**请求**的属性，不是网关配置项。做成池卡上的下拉，
  会让人以为「网关有一个全局模型设置」——那是假控件。当前演示请求的模型在计数格下方以只读文字给出。
- **「指定账号」五档策略收在同一个下拉里**（不指定 / 按实时积分择优 / **随机** / **顺序** / 某个账号），与真实
  `AccountPoolCard` 同形 —— 它们是同一个枚举 `AccountStrategy{Current, Pinned, MaxCredits, Random, Sequential}`，
  拆成多个控件会让它们同时成立。原型里 `max_credits` 按「它应该有的样子」实现
  （候选内按积分降序），**当前后端并非如此**（见 D6 / 决策 10）。
- 原型里的「本次请求会选中谁」是**确定性**的（免费层取权重最高者）；真实实现是
  Top-5 短名单内**加权抽签**。原型取确定值只为让结论这一行可读 —— 抽签结果没法用一句话讲清。
- 「**模拟请求**」按当前策略跑一轮真实链路（选号 → 429 → 写冷却 → 换号 → …），并把每一步显示出来；
  `willFail` 是演示用的失败注入（真实实现里会不会 429 由上游决定）。「**重置**」把冷却 / 闸门 / 排除
  复位（策略与开关保持不变）。
- 原型**不模拟**并发（在途是静态数字）、不模拟时间流逝（冷却是静态剩余秒数）。
  真实实现里两者都是活的：冷却到期自动恢复、租约到期自动释放。
- 原型只改了**单文件** `buddy-switch-prototype.html`。多文件原型（`index.html` + `demo-*.js`）
  **暂未同步** —— 等设计确认后与真实实现一次性同步，避免改两遍。

---

## 9. 重试与换号：与策略解耦（第三轮诉求）

> 需求原话：「不指定账号时，不管什么模式，请求失败需要有重试机制；如果识别到账号或者模型被限制了，
> 应该换符合要求的其他账号请求，这样是否更合理」。

**是的，更合理，而且现在的实现只做到了一半。** 结论：换号不该是某个模式的附加行为，
而是**中继层的统一策略** —— 策略只决定**第一跳**选谁，失败之后一律进入同一条链路。

### 9.1 唯一链路（三条路由共用，不新增第二份）

```text
attempt = 0;  tried = {}
while attempt < max_rotate:
    if 模型闸门暂停(model):        return 可诊断错误          # 一个请求都不发
    uid = pick(策略, tried, model, excluded)                  # 策略只影响「这一跳选谁」
    if uid == None:                return 聚合诊断（attempts[]）
    tried += uid
    resp = 出站(uid)
    if 2xx:                        return 成功
    event = classify(resp);        写冷却(uid, model, event)
    if not 值得换号(event):         return 原错误
    attempt += 1
```

### 9.2 失败类型 → 是否换号（决策矩阵）

| 失败 | 换号？ | 冷却动作 | 理由 |
|:--|:--|:--|:--|
| 429 + `6004`（模型级） | **是** | 模型冷却 60s 起（P0-1） | 换到「同模型可用」的号就有救 |
| 429（账号级） | **是** | 账号软冷却（≤15min） | 该号整体被限 |
| 402 / 余额文案 | **是** | 硬冷却至次日 04:00 | 换号能救 |
| 5xx | **是** | 熔断计数 | 上游抖动，换号常能过 |
| `12153` 会话失效 | **是** | 三振禁用 | 该号登录态坏了 |
| **401 / 403（无 `12153`）** | **是** | 浅冷却 60s，**不计数、不禁用**（#100） | 凭据被拒多半是 token 过期 ⇒ 保活刷新可自愈；打终态会让「号没坏却用不了」 |
| 404 | **只换 1 次** | 60s | 可能是账号级模型清单差异；换第 3 次纯浪费 |
| 400 / 422 参数错 | **否** | 无 | 同一份坏参数喂给下一个号 = 白烧配额（现在会白撞 3 次） |
| `11128` 内容审核 | 否（走降级提示词） | 无 | 与账号无关 |
| 流内错误（响应头已发） | **否** | 记冷却 + 解绑粘性 | 协议上无法换（见 P0-3） |

### 9.3 四种策略在换号时的行为（同一段代码，只有「选谁」不同）

| 策略 | 第一跳 | 换号后 |
|:--|:--|:--|
| 自动择优 | 权重 → 短名单 → 加权抽签 | 重新抽签（`tried` 排除） |
| 按实时积分择优 | 积分最高 | 取**次高** |
| **随机** | 候选内**均匀**抽签 | 重新均匀抽（`tried` 排除） |
| **顺序**（#102） | **粘住最近被选中的号**（`used_seq` 降序取第一） | 该号已进 `tried` ⇒ 自动落到「次近使用」的号 |
| 指定账号 | 指定的那个 | **回落自动择优**（偏好不是约束，现有行为） |

共同点：`model` 维度**始终参与过滤** ⇒ 模型级限流不会把请求换到一个同样被限流的号上。

### 9.4 什么时候**停止**换号（这条是「更合理」的关键）

1. **模型闸门**：同模型在 ≥2 个账号上失败 ⇒ 判定为模型级问题，**立即停止**（再换只会烧配额）；
2. `tried` 覆盖全部候选 ⇒ 停止；
3. 属于「不值得换号」的失败类型（9.2 的「否」）⇒ 立即返回；
4. 预算耗尽（`max_rotate`）。

> 现在缺的正是第 1 条：3 次重试会逐个撞在同一堵墙上，最后只回一句「没有可用账号」。

### 9.5 返回什么

失败响应带 `attempts[]`（账号 + 失败原因 + 冷却动作），而不是只给最后一条上游错误。
文案按档分：模型闸门 / 只剩收费号 / 全被排除 / 全冷却 / 参数错（不重试）。

### 9.6 「指定账号」也要换号（用户特别提到的点）

指定是**偏好**：只保证「第一跳优先用它」；它一失败，仍然换号。现有代码已经如此
（`pick_with_preference` 会回落），但 **UI 文案没讲**，用户会以为「指定了就只能用它」。
原型里已把这句话写进提示。

### 9.7 原型怎么演示

工具栏 **「模拟请求」** 按当前策略跑一轮真实链路并把每一步列出来：

```text
第 1 跳 jackey  → 429 6004（gpt-5-codex 模型限流）→ 写模型冷却 60s → 换号
第 2 跳 team-shared → 200 OK（第 2 跳命中）
```

闸门暂停时点它 ⇒ 链路只有一行「停：整体暂停 42s」（**一个请求都不发**）。
「重置」把冷却 / 闸门 / 排除复位，可反复演示。

---

## 10. 顺序档（issue #102，2026-10-26）

> 用户原话：「期望指定账号这里加一个顺序切换，账号限流后自动切换下一个账号。
> 现在随机选号的模式导致缓存复用率极低，token 消耗过快」。

### 10.1 为什么需要第五档

`Random` 每轮都可能换号 ⇒ 同一段对话的上下文落在不同账号上，**上游前缀缓存（KV cache）命中率归零**，
同样的上下文被反复按输入计价。用户体感就是「token 消耗快得离谱」。

`Current`（自动择优）虽然有会话粘性，但粘性键来自客户端的 `conversation_id`：客户端不给（很多
CLI/自建 harness 不给）就整段跳过，退化成每轮加权抽签 —— 与随机同样的后果，只是不那么明显。

因此需要一个**不依赖任何客户端字段**就能稳定复用同一个账号的策略。

### 10.2 语义（与「轮流」的区别）

| 档位 | 每轮选谁 | 效果 |
|:--|:--|:--|
| 自动择优 | 权重 → 短名单 → 加权抽签 | 有偏随机，缓存命中率中等（有粘性时高） |
| 按积分择优 | 积分最高 | 稳定，但按积分而不是按「已在用的号」 |
| 随机 | 候选内均匀抽签 | 每轮换号 ⇒ **缓存命中率归零** |
| **顺序** | **最近被选中的那个** | **一直用它**；它不可用才换下一个 |

「顺序」**不是**轮流（round-robin）：轮流每轮换号，对缓存毫无帮助。本档是
「一个号用到不能用」—— 健康 / 未被限流 / 未被排除 / 未占满在途就一直用它。

顺序从哪来：候选按 `used_seq` **降序**取第一名（`used_seq` 是选号序号，单调递增、与系统时钟无关
⇒ 不受时间回拨影响）；从未用过的号 `used_seq = 0`，并列按 uid 升序。
冷启动取 uid 最小者并一直粘着；它被限流后落回「次近使用」的号；全都没用过就是 uid 序的下一个。

### 10.3 三条硬约束（与其它档一致）

1. **分层仍然在前**：成本分层（免费 > 无观测 > 收费）先过滤，再分策略 —— 顺序档不会「稳定地用收费号」。
2. **排除 / 禁用 / 冷却 / 模型冷却 / 在途满 / `tried` 照样生效**：策略只决定「在合格候选里挑谁」。
3. **不受防惊群（`min_pick_gap_ms`）约束**：那条规则的目的恰恰相反（别让同一个号被连续打死）。
   故本档在分层之后直接返回，不进入短名单 + 防惊群那一段。

### 10.4 一个刻意取舍：冷却恢复后**不抢回**

A 被限流 → 换 B → 60 秒后 A 恢复。此时**仍然留在 B**（B 的 `used_seq` 更大）。
否则每 60 秒抖一次账号，缓存又被清空，等于没修。只有 B 也出问题才回到 A。

### 10.5 落地位置

| 层 | 落点 |
|:--|:--|
| 算法 | `pool/pick.rs::PickMode::Sequential`（分层之后直接返回，`mark_selected` 共用） |
| 哨兵 | `pool::PREFERRED_SEQUENTIAL = "__sequential__"` ↔ 前端 `SEQUENTIAL_OPTION` |
| 策略枚举 | `account_strategy::AccountStrategy::Sequential`（serde 标签 `"sequential"`）；池为空时回落「跟随登录态」 |
| 接线 | `routes/relay.rs::select_account`（`Sequential ⇒ (PickMode::Sequential, None)`，粘性不参与） |
| 前端 | 三个网关页的 `autoOptions` 同一档 + `ApiServicePage` 的 `persist({kind:"sequential"})` 与读回 |
| 门禁 | `check-scope-registry.cjs`：4 个哨兵逐字一致 · 三页 × 4 档 · union 含 `"sequential"` |
| 测试 | `pick.rs`：粘住同一号 / 限流才换且恢复不抢回 / 吃 `tried` 与排除 |
