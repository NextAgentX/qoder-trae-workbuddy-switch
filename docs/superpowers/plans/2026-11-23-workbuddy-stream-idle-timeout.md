# WorkBuddy 流式转发 idle 超时实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** WorkBuddy CN 的流式响应在上游字节静默 60 秒后显式失败并结束，不再等到约 300 秒的底层读超时。

**Architecture:** 在流式字节转发与 `UsageTap` 之间增加 idle 计时包装，只在 WorkBuddy CN streaming chat completions 路径启用。超时写入现有流失败信号并由 SSE 出口发送错误帧与 `[DONE]`；请求日志记录 idle 时长和转发字节数。

**Tech Stack:** Rust、Tokio、futures-util、Axum、reqwest。

**Spec:** `docs/superpowers/specs/2026-11-23-workbuddy-stream-idle-timeout-design.md`

## Global Constraints

- “在每个上游字节到达时重置 idle 计时；默认 60 秒。”
- “提供 `BUDDY_SWITCH_STREAM_IDLE_TIMEOUT_SECS` 环境变量覆盖；设为 `0` 时禁用。”
- “不在已发送响应头、已开始输出的 SSE 流中切换账号重放；不改变非流式请求的超时策略。”
- “保留现有 usage 采集、错误治理和 `[DONE]` 终止规则；idle 超时必须按流失败记录，不得静默结束成‘成功’。”

## Review Focus

- 预取首块必须仍原样透传，且 idle 计时从流式下游开始读取后工作。
- 后续任一上游字节都重置计时；按字节而非 token 内容判断活跃度。
- 超时值 `0` 关闭 idle 闸门，未配置或非法值回退到 60 秒。
- `auto_failover` 关闭时 idle 超时仍要作为失败明确结束，不能只补 `[DONE]`。
- 正常 EOF 和上游传输错误保留原有语义；计数不包含网关生成的错误帧。

---

### Task 1: 可配置的 WorkBuddy CN 流中 idle 计时

**Files:**
- Modify: `crates/buddy-switch-gateway/src/routes/relay.rs`
- Modify: `crates/buddy-switch-gateway/src/routes/chat.rs`
- Modify: `crates/buddy-switch-gateway/src/protocol/usage_tap.rs`

**Interfaces:**
- `relay.rs` 提供环境变量名 `BUDDY_SWITCH_STREAM_IDLE_TIMEOUT_SECS`、默认值 60 秒和解析函数；解析函数返回 `Duration::ZERO` 表示禁用。
- 流包装输入 `Stream<Item = Result<Bytes, reqwest::Error>>`，输出 `Stream<Item = Result<Bytes, std::io::Error>>`；普通 reqwest 错误映射为 `Other`，idle 超时映射为 `TimedOut`，供 `UsageTap` 区分治理事件。
- 仅 `Region::Cn` 且 `stream: true` 的 chat completions 接入包装；不影响 WorkBuddy Global、Responses、Anthropic 或非流式请求。

- [x] **Step 1: 在 `relay.rs` 定义常量及解析函数**：缺省或无法解析时返回 60 秒，`0` 返回 `Duration::ZERO`。
- [x] **Step 2: 实现 idle stream 包装**：每次成功收到上游字节后重置计时；reqwest 错误映射为 `std::io::ErrorKind::Other`，超时映射为 `TimedOut` 并终止，不重试上游。
- [x] **Step 3: 在 `chat.rs` WorkBuddy CN 流式分支接入包装**：保持链路次序为上游字节流 → idle 包装 → `UsageTap` → reasoning/SSE 出口；禁用值直接透传原流。
- [x] **Step 4: 在 `UsageTap` 失败路径保留 idle 诊断**：把 idle 归类为 `UpstreamEvent::Timeout`，记录超时毫秒数与已透传的上游字节数；日志字段不覆盖已有 usage 或请求耗时字段。
- [x] **Step 5: 静态复核**：检查三种配置值、仅 CN streaming chat 路径受影响、错误帧后恰好一个 `[DONE]`，并审阅最终 diff 与 `git status`。

### Task 2: 让 idle 超时在关闭 failover 时仍显式结束

**Files:**
- Modify: `crates/buddy-switch-gateway/src/failover.rs`
- Modify: `crates/buddy-switch-gateway/src/protocol/usage_tap.rs`
- Modify: `crates/buddy-switch-gateway/src/protocol/openai.rs` (contract comments only)

**Interfaces:**
- `StreamFailure` 保留现有 `auto_failover` 对普通流错误的开关行为；idle 超时提供专用的强制失败记录路径。
- `SsePassthrough` 对 idle timeout 产生 OpenAI 兼容错误帧与 `[DONE]`，即使普通 auto-failover 关闭；其他事件保持原有开关语义。

- [x] **Step 1: 增加专用于 idle timeout 的强制失败记录**：状态码和错误类型沿用 `failover::client_error(UpstreamEvent::Timeout, …)`。
- [x] **Step 2: 复用既有 SSE 收尾分支**：确认 `SsePassthrough::schedule_tail` 会读取强制记录的 timeout 信号并发送错误帧与 `[DONE]`；更新契约注释，其他失败严格保持既有行为。
- [x] **Step 3: 静态复核流失败信号被一次消费**：确认错误帧不重复、`[DONE]` 不重复，并确认 usage sink 在流结束时仍正常收口。

### Task 3: 使用说明与契约索引

**Files:**
- Modify: `docs/specs/UNIFIED-GATEWAY-ARCH.md`
- Modify: `docs/guides/GATEWAY-CLIENT-INTEGRATION.md`
- Modify: `docs/README.md`

- [x] **Step 1: 记录流中 idle 超时契约**：说明默认 60 秒、环境变量名、`0` 禁用、仅 WorkBuddy CN 流式 Chat Completions 生效，以及超时错误帧语义。
- [x] **Step 2: 更新文档索引状态**：设计标记为已实现，计划标记为已完成。
- [x] **Step 3: 静态复核文档**：确认没有把响应头已提交后的 SSE 错误写成 HTTP 502，也没有宣称会在同一流中换号重放。
