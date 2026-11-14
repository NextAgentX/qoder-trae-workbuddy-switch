# Qoder Model Scoped Rate Limit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Qoder accounts selectable across models when a Qoder model receives an upstream 429.

**Architecture:** Keep Qoder's existing model cooldown in `QoderPool`. The shared risk store continues to enforce request rate/concurrency limits and persistent account pauses, while Qoder gateway routing ignores account-wide `Cooling` states. Do not write a second account-wide cooldown for Qoder gateway 429 responses.

**Tech Stack:** Rust, Tokio, serde; existing Qoder gateway and shared risk-control modules.

**Spec:** `docs/superpowers/specs/2026-11-14-qoder-model-scoped-rate-limit-design.md`

## Global Constraints

- Qoder `429` cooldown remains model scoped in `QoderPool`.
- Authentication rejection remains a persistent account-level pause.
- Shared account rate, concurrency, and minimum-gap controls remain active when enabled.
- WorkBuddy and Trae account-cooling behavior remains unchanged.
- Preserve existing uncommitted changes in Qoder and Trae E2E files.

## Review Focus

- A legacy Qoder `Cooling` record must not exclude the account across all models; implement in Qoder's `account_blocked` selection path.
- An enabled shared limiter must still enforce request rate/concurrency when Qoder has a legacy `Cooling` record; implement in Qoder admission handling.
- Qoder `Paused` states must continue to block routing; keep this behavior in both preselection and admission.
- WorkBuddy and Trae must continue treating `Cooling` as blocked; keep the generic helper's behavior unchanged for those products.
- Qoder gateway `429` must not create a new account-wide `Cooling`; remove the redundant Qoder relay write.

## File Map

- Modify `crates/buddy-switch-core/src/modules/risk_control.rs`: expose admission that bypasses only persistent Cooling while preserving Pause and rate controls.
- Modify `crates/buddy-switch-gateway/src/risk_controls.rs`: use Qoder-specific account-state behavior, keeping other products unchanged.
- Modify `crates/buddy-switch-gateway/src/qoder/routes.rs`: stop writing shared account-level cooldown on Qoder gateway 429; retain the model-level pool update.
- Modify `docs/specs/RISK-CONTROLS-DESIGN.md`: document the Qoder model-scoped 429 behavior.

## Global Verification Constraint

Task constraint: do not add or run tests unless the user asks. Review the final source diff and confirm the edits are limited to the files above; report this limitation clearly.

---

### Task 1: Keep Qoder 429 Cooling Model Scoped

**Files:**
- Modify: `crates/buddy-switch-core/src/modules/risk_control.rs`
- Modify: `crates/buddy-switch-gateway/src/risk_controls.rs`
- Modify: `crates/buddy-switch-gateway/src/qoder/routes.rs`
- Modify: `docs/specs/RISK-CONTROLS-DESIGN.md`

**Interfaces:**
- Add a `RiskStore` admission method (or internal shared implementation) that bypasses only an existing `Cooling` state but still rejects `Paused` and enforces every configured rate/concurrency/gap limit.
- Make `risk_controls::admit_request` use that admission policy for product `qoder`; preserve existing admission for `workbuddy` and `trae`.
- Make `risk_controls::account_blocked` return blocked for Qoder `Paused` only, while preserving existing behavior for other products.
- Remove the Qoder relay's `record_rate_limit` call; retain `QoderPool::apply_error(... RateLimited ...)` as the 60-second model cooldown.

- [x] **Step 1: Update the risk-control specification**

In the Qoder gateway section, state that upstream 429 is model scoped and is handled by the Qoder pool; shared persistent account-level `Cooling` must not make the account unavailable to every Qoder model. Keep account-level `Paused` semantics for authentication rejection.

- [x] **Step 2: Implement a cooling-aware admission path**

In `RiskStore`, factor admission or add a method so Qoder can ignore only persisted `Cooling` while `Paused` remains a rejection and the configured request-rate, in-flight, and account-gap checks remain identical. Route Qoder through this method in `risk_controls::admit_request`; leave WorkBuddy and Trae on the existing method.

- [x] **Step 3: Use model-level cooldown for Qoder relay 429**

Remove the Qoder relay call that writes `RiskOutcome::RateLimited` to the shared account risk store. Keep `pool.apply_error` unchanged so the current model remains cooled for 60 seconds. Keep Qoder authentication rejection writes to `Paused` unchanged.

- [x] **Step 4: Review the final diff**

Inspect `git diff` and `git status --short`. Confirm only the four planned files plus this spec and plan changed, and leave pre-existing user modifications in the two E2E files intact. `cargo fmt --check`, `cargo check -p buddy-switch-core -p buddy-switch-gateway`, and `git diff --check` pass. Tests were not added or run.
