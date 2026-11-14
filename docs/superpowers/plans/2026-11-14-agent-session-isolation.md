# Agent 会话上下文隔离实施计划

> **For agentic workers:** 按本计划逐项实施。用户已授权直接实施。

**目标：** 让所有 Responses 客户端在已有产品线、Key、区域隔离之外，再按已提供的客户端会话身份隔离 `previous_response_id` transcript。

**架构：** 在共享 Responses 协议层将状态存储键按 base scope、可选会话指纹和 response ID 组成。已提供身份时严格匹配；续接中身份暂缺时，仅在 response ID 唯一时继承原指纹；首轮无身份的链在首次出现稳定身份时绑定它。WorkBuddy、Qoder、Trae 共用此逻辑。

**技术栈：** Rust、Axum、Serde JSON、现有 `sha2` 依赖。

**状态：** 已完成；`cargo test -p buddy-switch-gateway`、Rust 格式检查和 `git diff --check` 均通过。

**范围：** `crates/buddy-switch-gateway/src/{protocol/responses.rs,session_headers.rs,routes/responses.rs,qoder/routes.rs,trae/routes.rs,unified/routes.rs,routes/cross.rs}`、客户端接入指南及统一网关架构文档。

## 全局约束

- 不读取或记录 prompt 正文、API Key 或原始会话 ID。
- 不改 Completions / Anthropic 的上下文转发语义；它们仍由客户端提交完整历史。
- 不改变已有 API Key、区域、Trae 变体/产品线状态隔离。
- 首轮和续接都缺少客户端会话身份时保持旧行为；续接身份暂缺时只能唯一命中并继承原身份。
- 保留工作区中其他未提交修改，不提交、不改写 Git index。

## 任务

### 任务 1：共享状态身份键

**文件：** `protocol/responses.rs`

- 新增共享会话身份解析器，保留身份来源标签；新增状态 scope 函数，接收已有 scope、已解析会话身份，返回携带 SHA-256 前 128 位指纹的 scope。
- 空/空白身份不附加身份后缀，兼容原行为。
- 指纹只存在于内部状态键，不输出到日志或响应。
- 测试跨会话续接拒绝、ID 暂缺时身份继承，以及无身份链首次绑定。
- 状态命中刷新空闲 TTL，使容量淘汰按最近访问时间工作；输入历史预算提高到 256 KiB。
- 续接状态未命中、容量淘汰、历史裁剪只写不含会话 ID / Key / prompt 正文的摘要日志。

### 任务 2：接入三条 Responses 路由

**文件：** `routes/responses.rs`、`qoder/routes.rs`、`trae/routes.rs`

- WorkBuddy 从 body/header 解析会话身份，并在状态读取、流式状态链、非流式状态写入时共用增强后的 scope。
- Qoder handler 将请求头传到共用中继；仅 Responses 协议读取会话身份，不改变 OpenAI Chat / Anthropic 路径。
- Trae Responses handler 与统一网关分派都传递请求头；使用共享解析器和状态键。
- 统一入口与跨产品线路由同步传递 headers 到 Qoder 中继，避免独立入口与统一入口行为分叉。

### 任务 3：同步行为文档

**文件：** `docs/guides/GATEWAY-CLIENT-INTEGRATION.md`、`docs/specs/UNIFIED-GATEWAY-ARCH.md`

- 说明 Responses 状态按客户端会话身份进一步隔离及各客户端字段来源。
- 明确无稳定身份时的兼容行为与剩余隔离边界。
- 明确 Completions / Anthropic 历史仍由客户端发送。

### 任务 4：回归验证与差异审阅（已完成）

- 执行 `cargo check -p buddy-switch-gateway`。
- 检查差异只包含本计划路径；确认并行工作区改动仍保留。
- Responses 定向测试与 gateway crate 全量测试通过；新用例覆盖跨会话拒绝、身份暂缺时继承、无身份链绑定及标识来源优先级。

## 主要风险

- 携带不同会话 ID 的续接会得到现有的 `previous_response_id not found` 错误；这是阻止串话的预期结果。
- 从首轮到续接均不提供稳定身份时，网关仍无法推断项目归属；需客户端/CCSwitch补充 `x-session-id` 才能获得项目级隔离。
