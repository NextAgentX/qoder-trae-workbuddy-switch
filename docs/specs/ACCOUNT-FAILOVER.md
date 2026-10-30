# 账号故障时自动切换（`auto_failover`）

> 状态：现行（issue #129，2026-10-30 落地）
> 代码入口：[`crates/buddy-switch-gateway/src/failover.rs`](../crates/buddy-switch-gateway/src/failover.rs)
> 关联：[`POOL-PICK-EXCLUDE-PLAN.md`](POOL-PICK-EXCLUDE-PLAN.md) §9.2（换号决策矩阵，已同步本开关）、[`VOCABULARY.md`](VOCABULARY.md) 文末附节（6 个术语）、issue #66 / #106（换号与闸门的上一轮排查）

## 一、这个开关做什么

一个账号在回复中途挂掉时，**不让回复静默截断**：网关把这次失败变成一条
「客户端可重试」的错误，并把该账号写冷却、解绑会话粘性 ⇒ 客户端整步重跑时
必然落到另一个账号，用户看到的是从头完整重写的回复。

字段：`auto_failover`（WB / Trae / Qoder 三份配置各一份，**默认 `false`）。
界面：账号池卡片顶部工具条（三平台共用的 `AccountPoolCard`）。

**为什么不是「静默续写」**：OpenAI 兼容上游没有 assistant 前缀续写能力，
半截 `reasoning_content` / 半截 `tool_calls`（JSON 中间截断）都无法重放。
能成立的等价形态只有「整步重跑」。

## 二、对外错误形状是**协议契约**，不是文案

客户端（DeepSeek Harness 的 `openai-completions` 适配器）**只做文本正则**：
`429` / `rate limit` → `RATE_LIMIT`，`5\d\d` → `SERVER`，`401|403` → `AUTH`（终态）。
`RATE_LIMIT` / `SERVER` 在可重试集合里；`QUOTA` 的判定排在前面且**不可重试**。

唯一映射点：`failover::client_error(event, account, model)`

| 上游事件 | 对外 `status` | 对外 `type` | 客户端归类 | 可重试 |
|:--|:--|:--|:--|:--|
| `SoftRate` / `HardCredit` / `SessionDead` / `AuthRejected` | 429 | `rate_limit_error` | `RATE_LIMIT` | 是 |
| `Server` / `Timeout` / 上游断流 | 502 | `api_error` | `SERVER` | 是 |
| `ModelBlocked` / `NotFound` / `Client` | 400 | `invalid_request_error` | `INVALID_REQUEST` | 否 |

三条硬约束（改文案前必读）：

1. `message` 里**必须**出现等价状态码（`HTTP 429` / `HTTP 502`）；
2. **不许**出现配额类短语（余额 / 配额 / 积分 / `quota` / `insufficient` /
   `out of credit` / `balance` / `exhausted`）—— 它们会把归类抢成不可重试的 `QUOTA`；
3. **不许**把 401/403 形状发到流中途 —— `AUTH` 是一次终态，UI 还会渲染成「密钥无效」。

上游原文（含 `6004` / `12153` / 账号名）**只进日志与账号池**，不进对外文案。
Qoder 侧额外把原文放进错误帧的非标准字段 `error.upstream`（客户端忽略），
给本进程的 `UsageTapStream` 记日志用。

## 三、三协议出口的改造点

| 协议 | 出口 | 断流（`Err`） | 流内错误帧 |
|:--|:--|:--|:--|
| OpenAI | `protocol::openai::SsePassthrough` | 开关开启：先发 `error` 帧再补 `[DONE]` | 由 `UsageTap` 嗅探后写信号槽，出口收尾时发 |
| Anthropic | `protocol::anthropic::AnthropicSseStream` | 开关开启：发 `event: error`，**不补** `message_stop` | `ingest` 逐帧 `sniff_payload`，命中即 `fail()` |
| Responses | `protocol::responses::ResponsesSseStream` | 同上，**不补** `response.completed` | 同上 |

`[DONE]` / `message_stop` / `response.completed` 仍然要发（否则客户端挂死等流结束），
但**错误帧必须在它们之前**：客户端见到 `[DONE]` 就判成功，后面的错误帧会被忽略。

## 四、失败信号为什么是共享槽

链路：`上游 → UsageTap → ReasoningDelta → 协议出口`，三类消费者各只看得见一段：

- `UsageTap`：唯一能看见「这一帧是错误信封」与「上游 `Err`」的地方；
- 协议出口：决定收尾方式；
- `StreamingLogSink`：流结束时落日志（必须知道这次其实是失败）。

`failover::StreamFailure` 是三者共用的极小槽。**嗅探只做一次**（在 `UsageTap`），
否则「出口认为失败、日志认为成功」这种漂移一定会在某次改动后出现。

## 五、关闭态不变量（默认关闭 = 回归基线）

`auto_failover = false` 时，下列行为必须与改造前**逐字一致**。任何一条被破坏，
都说明门控失效 —— 每条都有单测钉住（见第六节）：

1. `SsePassthrough` 断流只补 `data: [DONE]\n\n`，不追加任何错误帧；
2. `AnthropicSseStream` 断流仍 `finalize()`（补 `message_stop`）；
3. `ResponsesSseStream` 断流仍 `finish()`（补 `response.completed`）；
4. `UsageTap` 断流不写失败槽、不上报治理；
5. `StreamFailure::record` 直接丢弃，不留任何状态（连一次写锁都不做）；
6. `QoderOpenAiStream` 断流只补 `[DONE]`；
7. 请求日志 `status` 仍取模板里的 200、`error` 为 `null`。

## 六、五条治理链路（一次都不能省）

「对外保持可用 ≠ 对内保持静默」。开关开启时，每一次被静默处理的失败都要走完：

| 链路 | 落点 |
|:--|:--|
| 账号冷却 / 熔断 / 模型闸门 | `relay::note_envelope_failure` → `Pool::apply_upstream_error` |
| 粘性解绑 | `EnvelopeErrorSink::on_error` → `state.sticky.unbind`（Qoder：`stream_relay` 内） |
| 管理面 `last_error` | 同上 → `state.note_last_error` |
| 请求日志 `status` / `error` | `StreamingLogSink::finish` 读失败槽覆盖 |
| 决策向量 / 降级判据 | 冷却写入即被 `pool::pick` 与 `model_fallback` 读到（无独立落点） |

## 七、可证伪清单（单测索引）

| 断言 | 用例 |
|:--|:--|
| 关闭态收尾字节逐字一致 | `protocol::openai::tests::passthrough_without_failover_keeps_the_legacy_tail_bytes` |
| 开启态「错误帧在前、`[DONE]` 在后」 | `…::passthrough_with_failover_emits_a_retryable_error_before_done` |
| 6004 对外是 429 | `…::passthrough_with_failover_emits_429_for_envelope_errors` |
| 干净流不被加料 | `…::passthrough_with_failover_keeps_a_clean_stream_clean` |
| Anthropic 断流不发 `message_stop` | `protocol::anthropic::tests::broken_stream_with_failover_emits_error_instead_of_message_stop` |
| 私有信封 `{"code":6004}` 被识别 | `…::private_envelope_frame_fails_the_turn` |
| Responses 断流不发 `response.completed` | `protocol::responses::tests::broken_stream_with_failover_emits_error_instead_of_completed` |
| 断流写 502 并上报治理 | `protocol::usage_tap::tests::upstream_break_records_a_server_failure_and_reports_it` |
| 关闭态断流不上报 | `…::upstream_break_is_ignored_when_failover_is_off` |
| 402 只是冷却、不返回「已禁用」 | `pool::tests::apply_error_hard_credit_cools_until_next_4am_and_does_not_disable` |
| 对外文案无配额短语 | `failover::tests::outward_messages_never_mention_quota_words` |

## 八、已知边界

- **`ModelBlocked` 对外 400 ⇒ 客户端不重试**：换号对「该模型被上游封禁」通常也无意义，
  按 issue #129 §3.4 的口径让它落进不可重试集合；
- **首帧前失败**不走本开关：那条路径本来就能真换号（`peek_first_frame`，20s 窗口），
  开关只补「首帧之后」这一段；
- **单账号池**：开关开启时客户端仍会重试并失败，但错误形状变成可重试的 `429`，
  文案含「已尝试其它账号」—— 用户能看出是池里只有一个号，而不是模型坏了；
- **`402` 换号 ≠ `402` 降级**：本开关顺带修掉了「402 上一击就终止换号」的缺陷
  （`Pool::apply_upstream_error` 的 `HardCredit` 分支返回 `false`）。
  这与 [`MODEL-FALLBACK.md`](MODEL-FALLBACK.md) 「402 不降级」**不矛盾**：
  降级是换**模型**（额度问题换模型仍 402），换号是换**账号**（别的号可能有钱）。
