# 三平台合规风控设置实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 WorkBuddy、Trae、Qoder 建立共享、可在设置中调整的请求限流与平台隔离的异常暂停规则，并移除按账号伪装设备身份/分流代理的实现。

**Architecture:** 在 `buddy-switch-core` 增加共享的设置与持久化账号暂停状态；三套 gateway 各自在账号选择和出站路径执行共享参数，但风险状态仍按平台/区域隔离，不共享账号池。设置入口位于「设置 → 通用」，Tauri 与本地 HTTP 命令保持同形。

**Tech Stack:** Rust workspace、Axum、Tauri 2、React/TypeScript、Vitest、现有 JSON 原子写入与平台账号池。

**Spec:** `docs/specs/RISK-CONTROLS-DESIGN.md`

## Global Constraints

- 请求参数由三平台共享；账号暂停/冷却状态按平台 + 区域 + 账号分别保存，读取和写入不得跨作用域。
- 上游明确认证拒绝、封禁、限流或额度不足时不得自动换号接力。
- 有不确定副作用的 POST 不自动重放；请求体只可在具备幂等保证时重试。
- 不生成/覆盖 UID 派生设备身份，不按账号配置代理出口。
- 旧配置文件与客户端身份文件不得被静默删除或覆盖。
- UI 必须显示参数单位、范围、当前生效值和保存错误。
- 不新增依赖；兼容现有配置文件缺字段的升级路径。

## Review Focus

1. 账号暂停/冷却状态在区域或平台间串区 — 测试 `risk_state_is_isolated_by_product_and_region`。
2. 并发请求同时穿过速率窗口 — 测试 `rate_limit_is_atomic_under_concurrent_requests`。
3. 进程重启清除封禁/冷却 — 测试 `account_pause_survives_reload_and_requires_manual_recovery`。
4. 429/封禁后 relay 继续换账号 — 每个平台测试 `rate_limit_does_not_rotate_accounts` / `account_block_does_not_rotate_accounts`。
5. 结果不确定的 POST 被重复提交 — 测试 `uncertain_post_is_not_replayed`，并覆盖 WorkBuddy refresh/check-in/travel、Trae refresh、Qoder refresh。

---

## 文件结构

- Create `crates/buddy-switch-core/src/modules/risk_control.rs`: 共享配置值、作用域键、速率窗口、账号暂停/冷却的原子持久化 API。
- Modify `crates/buddy-switch-core/src/modules/mod.rs`: 导出风险策略模块。
- Modify `crates/buddy-switch-gateway/src/state.rs`, `src/trae/mod.rs`, `src/qoder/mod.rs`: 三平台读取共享请求参数，并由各平台分别持久化风险状态。
- Modify `crates/buddy-switch-gateway/src/routes/relay.rs`, `src/trae/routes.rs`, `src/qoder/routes.rs` 与对应 pool 模块：选择账号前检查策略，遇到受保护错误时持久化状态并立即结束当前账号尝试。
- Modify WorkBuddy/Trae/Qoder 的账号认证 API 调用模块：统一登记 rate permit，并对不确定副作用的 POST 禁止自动重放。
- Modify `crates/buddy-switch-server/src/api.rs` 与 `src-tauri/src/commands.rs`: 共享设置保存、运行时热应用与状态读取保持一致。
- Create `src/components/gateway/risk-controls-form.tsx`: 受控的跨平台共享表单，只处理输入、校验提示和提交回调。
- Modify `src/pages/SettingsPage.tsx`: 在「设置 → 通用」读写三平台共享设置。
- Modify `src/lib/types.ts`, `src/lib/trae-types.ts`, `src/lib/trae-gateway.ts`, `src/lib/qoder-gateway.ts`, `src/lib/gateway.ts`, `src/lib/api.ts`, locale domains and screenshot demo: 同步类型、归一化、演示响应和中英文词条。
- Modify `docs/specs/RISK-ACCOUNT-ISOLATION.md` and `docs/README.md`: 替换账号关联规避表述，索引现行合规风控规则。
- Retire WorkBuddy-only account proxy and UID-derived device identity changes in the current worktree; preserve the single-account auth snapshot and leave existing local files untouched.

## Implementation Tasks

### Task 1: Shared risk policy and durable account state

**Files:**
- Create: `crates/buddy-switch-core/src/modules/risk_control.rs`
- Modify: `crates/buddy-switch-core/src/modules/mod.rs`
- Test: module unit tests in `risk_control.rs`

**Interfaces:**
- `RiskControlSettings`: `enabled: bool`, `requests_per_minute: u32`, `max_in_flight_per_account: u32`, `max_in_flight_global: u32`, `min_account_gap_ms: u64`, `max_transient_attempts: u32`, `fallback_cooldown_secs: u64`.
- `RiskScope`: product string + region string; validated against fixed allowed values by caller.
- `AccountRiskState`: `Active`, `Cooling { until_ms, reason }`, `Paused { reason }`.
- `begin_request(scope, uid, settings, idempotent) -> Result<RiskPermit, RiskDecision>` and `record_upstream_outcome(scope, uid, outcome, reset_at_ms)`; account state saved atomically under platform-scoped storage.
- `recover_account(scope, uid)`: explicit manual recovery only; no automatic removal of a persistent `Paused` state.

- [x] **Step 1: Write tests** for config normalization, malformed state fail-closed behavior, independent product/region keys, atomic concurrent rate admission, exact cooldown expiry, persistent pause across store reload, and explicit-only recovery.
- [x] **Step 2: Run** `cargo test -p buddy-switch-core risk_control --lib`.
- [x] **Step 3: Implement** the typed settings, decision result, synchronized limiter and atomic JSON state persistence; use existing `atomic_write` and existing home/store path helpers.
- [x] **Step 4: Run** the focused test, then `cargo test -p buddy-switch-core --lib` (1653 passed).

### Task 2: WorkBuddy request enforcement

**Files:**
- Modify: `crates/buddy-switch-gateway/src/state.rs`, `pool/mod.rs`, `pool/pick.rs`, `routes/relay.rs`
- Modify: WorkBuddy account-authenticated helpers in `crates/buddy-switch-core/src/modules/{refresh,credits,checkin,travel,catalog,upstream}.rs`
- Test: those Rust modules and gateway relay unit tests

**Interfaces:**
- WorkBuddy reads the shared `RiskControlSettings`; CN and Global risk state remain separate.
- Relay checks account pause/cooldown before selection, records upstream status and reset metadata, and stops the request on 401/403, ban, 429/model rate limit, or hard credit.
- Account-authenticated client helpers call the shared risk gate; operations with non-idempotent POST semantics opt out of automatic replay.

- [x] **Step 1: Write failing tests** for shared settings defaults, out-of-range rejection, a blocked account being skipped without selecting a replacement, 429 returning without rotation, and POST transport uncertainty not issuing a second send.
- [x] **Step 2: Run** `cargo test -p buddy-switch-gateway --lib risk_control` and focused core tests.
- [x] **Step 3: Implement** WorkBuddy relay/client integration. Remove proxy egress and UID-derived device identity writes from code while keeping auth snapshot behavior; do not delete existing user files.
- [x] **Step 4: Run** `cargo test -p buddy-switch-gateway --lib` (866 passed, 2 ignored) and `cargo test -p buddy-switch-core --lib` (1653 passed).

### Task 3: Trae request enforcement

**Files:**
- Modify: `crates/buddy-switch-gateway/src/trae/mod.rs`, `src/trae/pool.rs`, `src/trae/routes.rs`
- Modify: `crates/buddy-switch-core/src/modules/trae/{account,credits,handlers,model_list_upstream}.rs`
- Test: Trae pool, route and account request tests

**Interfaces:**
- Trae reads the shared `RiskControlSettings`; CN and Global risk state remain separate.
- Trae program variants in one region share that region's risk state.

- [x] **Step 1: Write failing tests** for region isolation, invalid settings, persistent pause, no account rotation after 429/auth rejection, and no replay of uncertain refresh POST.
- [x] **Step 2: Run** focused Trae Rust tests.
- [x] **Step 3: Implement** shared settings consumption and relay + authenticated background request integration, including Trae model directory refresh.
- [x] **Step 4: Run** Trae-focused Core tests; full Core coverage passed.

### Task 4: Qoder request enforcement

**Files:**
- Modify: `crates/buddy-switch-gateway/src/qoder/mod.rs`, `src/qoder/pool.rs`, `src/qoder/routes.rs`
- Modify: `crates/buddy-switch-core/src/modules/qoder/{api,campaign,account}.rs`
- Test: Qoder pool, route and API tests

**Interfaces:**
- Qoder reads the shared settings; CN/Global risk state remains distinct.
- Existing daily quota, credit reserve, per-model cooldown and manually disabled states retain their current independent meanings.

- [x] **Step 1: Write failing tests** for region isolation, invalid settings, persistent pause, no account rotation after 429/auth rejection, and no replay of uncertain token-refresh POST.
- [x] **Step 2: Run** focused Qoder Rust tests.
- [x] **Step 3: Implement** shared settings consumption and relay + authenticated background request integration, including campaigns/claims and web usage admission.
- [x] **Step 4: Run** Qoder-focused Core tests; full Core coverage passed.

### Task 5: Shared settings UI and command wiring

**Files:**
- Create: `src/components/gateway/risk-controls-form.tsx`
- Modify: `src/pages/SettingsPage.tsx`, `src/lib/risk-controls.ts`, `src/lib/types.ts`, `src/lib/api.ts`, screenshot demo, locale domain files, server API and Tauri commands.
- Test: frontend unit tests for settings normalization; API contract gate.

- [x] **Step 1: Write frontend tests** for settings normalization and fail-closed defaulting.
- [x] **Step 2: Run** `npm run test:unit -- risk-controls` and `npm run check:api`.
- [x] **Step 3: Implement** one shared settings form in `Settings → General`; wire both Tauri and HTTP save/load endpoints and demo responses.
- [x] **Step 4: Run** `npx tsc --noEmit`, `npm run test:unit` (43/43), and `npm run check:api`.

### Task 6: Documentation, compatibility and full acceptance

**Files:**
- Modify: `docs/specs/RISK-ACCOUNT-ISOLATION.md`, `docs/specs/RISK-CONTROLS-DESIGN.md`, `docs/README.md`
- Verify: Rust crates, TypeScript, frontend and repository gates

- [x] **Step 1: Document** field units, bounds, default behavior, state recovery, legacy config behavior, and the fact that these controls do not guarantee platform account status.
- [x] **Step 2: Remove** inaccurate claim that account-specific proxy/device identity prevents association; document that old `account_egress.json` is inert and not deleted.
- [x] **Step 3: Run** `cargo test -p buddy-switch-core --lib` (1653 passed), `cargo test -p buddy-switch-gateway --lib` (866 passed), `cargo check --workspace --all-targets --locked`, `npm run build` and `npm run test:unit` (43 passed).
- [x] **Step 4: Inspect** `git diff --check`, the final diff, and `git status`; no unrelated pre-existing work was reset or removed.