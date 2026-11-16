# 统一网关斜杠模型 ID 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一网关的模型清单使用 `{namespace}/{model_id}`，同时继续接受旧 `{namespace}:{model_id}` 请求。

**Architecture:** 在 core 命名空间工具集中维护分隔符规则：生成清单 ID 时使用 `/`，解析请求时兼容 `/` 与 `:`。统一网关的 OpenAI 和 Anthropic 请求共享 namespace guard，因此两类 API 路由行为一致；上游始终只收到裸模型 ID。

**Tech Stack:** Rust、Axum、serde_json；Markdown 文档。

**Spec:** `docs/superpowers/specs/2026-11-16-unified-gateway-slash-model-ids-design.md`

## Global Constraints

- 命名空间只允许注册表 ID 整段精确匹配，禁止前缀匹配或模糊推断。
- 仅统一网关清单输出改成斜杠；独立网关保持原样。
- 旧冒号格式请求继续通过同一启用、授权与 relay 检查。
- 分隔符只切开命名空间与模型 ID；模型 ID 剩余内容原样透传。
- 不新增测试代码或运行测试命令；若既有测试断言因输出格式改变而过时，只更新其预期值。

## Review Focus

- 冒号旧请求：必须仍路由到同一命名空间，且上游模型名不含命名空间。
- 斜杠新请求：只按第一个分隔符切分，模型 ID 内部的后续斜杠保持不变。
- 未知命名空间：维持现有拒绝行为，不得误路由。
- 空模型 ID：斜杠和冒号两种写法均由既有 guard 拒绝。
- OpenAI 与 Anthropic 请求：两者都经过统一解析与授权链路。

## File Map

- `crates/buddy-switch-core/src/modules/gateway_namespace.rs`：统一分隔、解析、生成命名空间模型 ID。
- `crates/buddy-switch-gateway/src/unified/routes.rs`：清单输出文案；清单 ID 由 core 的 compose 生成；已有请求 guard 复用 core split。
- `crates/buddy-switch-core/src/modules/gateway_namespace.rs` 和 `crates/buddy-switch-gateway/src/unified/routes.rs` 内既有测试：仅在其预期因新格式而过时的断言处同步更新，不新增测试、也不运行测试。
- `docs/specs/UNIFIED-GATEWAY-ARCH.md`、`docs/guides/GATEWAY-CLIENT-INTEGRATION.md`、`README.md`：同步斜杠新格式与冒号兼容说明。

---

### Task 1: 统一命名空间 ID 的生成与兼容解析

**Files:**
- Modify: `crates/buddy-switch-core/src/modules/gateway_namespace.rs`

**Interfaces:**
- `split(model: &str) -> Split<'_>` 继续返回已识别命名空间及裸模型 ID；接受第一个 `/` 或 `:` 分隔符。
- `compose(namespace: &str, model: &str) -> String` 改为输出 `{namespace}/{model}`。

- [x] 让解析器按最早出现的 `/` 或 `:` 切分，只在前段精确命中注册表时剥离前缀；未知前段仍保留完整模型字符串。
- [x] 将 compose 输出切换为斜杠；同步更新同文件里已有的格式预期断言和注释，不新增测试或运行测试。

### Task 2: 统一网关清单与请求路由

**Files:**
- Modify: `crates/buddy-switch-gateway/src/unified/routes.rs`

**Interfaces:**
- `build_models_response` 继续调用 core compose，返回的模型 `id` 因而改为斜杠格式。
- `guard_namespace` 继续调用 core split；OpenAI Chat/Responses 与 Anthropic Messages 共用同一判定。

- [x] 更新 `/v1/models` 处理器和 builder 的注释，说明新斜杠清单格式。
- [x] 更新现有清单断言以匹配斜杠输出；保留旧冒号请求兼容的既有调用路径，不新增或运行测试。

### Task 3: 同步现行文档

**Files:**
- Modify: `docs/specs/UNIFIED-GATEWAY-ARCH.md`
- Modify: `docs/guides/GATEWAY-CLIENT-INTEGRATION.md`
- Modify: `README.md`

- [x] 将统一网关示例及清单规则改为 `{namespace}/{model_id}`。
- [x] 明确冒号形式仍可作为已有客户端配置的兼容请求格式；OpenAI/Anthropic 的 API 协议本身不规定该分隔符。
- [x] 确认独立网关的冒号/区域前缀示例未被误改。
