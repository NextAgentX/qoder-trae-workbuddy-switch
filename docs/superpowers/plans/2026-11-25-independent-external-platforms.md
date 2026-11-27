# 独立接入豆包、Loomy、小浣熊实施计划

> 状态：已完成 · 2026-11-26。15 页面逐入口核对见 [对齐记录](2026-11-25-external-platforms-alignment.md)。
>
> **For agentic workers:** Native execution in this session. Complete each task in order and keep the active goal intact.

**Goal:** 为豆包、Loomy、商汤小浣熊注册独立 CN 平台入口，并让每个平台拥有概览、账号、用量、网关、设置五个独立入口。

**Architecture:** 扩展前端平台注册表与 CN 路由；第三方账号按 provider 强过滤；使用一个 provider-aware 页面组实现五个目的地，真实读取已有余额。外部平台不扩充 core 客户端 / 网关 Scope，避免走错账号库或网关中继。保留现有 WorkBuddy 用量页的外部积分入口作为兼容。

**Tech Stack:** React / TypeScript、Tauri、Axum、buddy-switch-core、Serde。

**Spec:** docs/superpowers/specs/2026-11-25-external-platforms-design.md

## Global Constraints

- 新平台只有 CN 路由，不能生成 global 页面。
- provider 专用 API 拒绝未知 provider，列表/刷新/删除只对指定 provider 生效。
- 凭据不得出现在查询响应、日志或错误文本中。
- 外部余额按平台分别显示，不跨服务相加。
- 保留现有三平台和未提交工作树改动。
- 不把未实现的 API 转发显示为已启用。

## Review Focus

- 非法 provider/scope 组合：命令返回错误，不访问其他平台账号。
- 一个 JSON store 内混存三家账号：页面和删除/刷新只作用于选择的平台。
- 平台缺少 global scope：切换器和 URL 不能构造 global 页面。
- 上游鉴权失败或余额字段缺失：保留最后成功值，并展示错误。
- 网关暂不支持的上游：页面显示实际状态，不出现可误用的启用开关。

## File Structure

- Keep the core `Product` / `Scope` registry unchanged; external providers use the dedicated credit provider API.
- Modify external account provider filtering: crates/buddy-switch-core/src/modules/external_credits.rs.
- Modify frontend platform and route registries: src/lib/platforms/registry.ts, src/lib/scope.ts, src/App.tsx.
- Add provider-scoped overview/accounts/usage/gateway/settings views under src/pages/external-platforms/ and shared components under src/components/external-platforms/.
- Modify app scope status, gateway/usage adapter types, locale domains, and scope/API contract scripts where exhaustive registrations require it.
- Preserve and update docs/specs/EXTERNAL-CREDITS.md, docs/superpowers/specs/2026-11-25-external-platforms-design.md, and docs/README.md.

## Tasks

### Task 1: Register three independent CN platform routes

Update frontend Product/PlatformSlug/Scope registries and app navigation; leave Rust core scopes limited to WorkBuddy, Trae, and Qoder. Add localized labels. Register each provider with only its applicable capabilities and hide sessions. Update compiler-driven frontend product maps and the scope registry guard for three extra CN-only platform routes.

**Check:** TypeScript and Rust compile; scope guard recognizes exactly doubao-cn, loomy-cn, raccoon-cn in addition to existing scopes.

### Task 2: Make external credit commands provider-scoped

Add explicit provider-filtered list, refresh and remove APIs in external_credits. Route all new provider pages through those APIs, reject mismatched/unknown providers, keep credential-free responses, and preserve existing JSON records and legacy all-provider APIs.

**Check:** API contract check sees every command route; compile both Tauri and server hosts.

### Task 3: Add five provider-scoped pages

Create provider-aware overview, account, usage, gateway and settings views. Overview summarizes only the selected provider. Accounts lists and edits only that provider. Usage shows that provider's real balance and source status. Gateway and settings have independent routes and display only implemented capabilities. Reuse existing shadcn components and locale keys.

**Check:** Every provider/destination pair has a route mapping; no mapping falls back to an existing product page.

### Task 4: Connect shell navigation and scope status

Register all three products in the switcher, scope selector, command palette, visible-scope settings, app shell, badges and deep links. Do not show sessions or global regions for these products. Add external account counts to scope status without probing unrelated desktop clients.

**Check:** All three CN scopes can be selected; all five destinations are visible and produce stable provider-specific URLs.

### Task 5: Reconcile docs and perform static verification

Update external credit and platform docs plus docs/README.md. Run rustfmt on new Rust files, cargo check for core/server/Tauri, TypeScript no-emit, API contract check, scope registry check, and git diff --check. Do not run test suites unless requested.

**Check:** All listed static checks pass and git status contains no unrelated paths beyond the pre-existing user changes.

### Task 6: Align Overview with the prototype

**Files:** `src/pages/ExternalProviderPage.tsx`, external platform locale domains.

Rebuild the Overview content using the prototype hierarchy: page header and refresh action, pending/attention area, four statistic tiles, then a responsive two-column body with the trend card on the left and provider account/capability summaries on the right. Keep the same composition for all three providers. Use a clearly explained empty state when no historical balance snapshots exist; do not fabricate a trend. Preserve provider-specific errors and low-balance warnings.

**Check:** Open `/s/doubao-cn/overview`, `/s/loomy-cn/overview`, and `/s/raccoon-cn/overview`; verify each has the same hierarchy, provider-isolated values, and no horizontal overflow at narrow width.

### Task 7: Align Accounts with the prototype

**Files:** `src/pages/ExternalProviderPage.tsx`, `src/components/external-credit-sources.tsx`, existing account toolbar/list primitives, external platform locale domains.

Add the prototype account toolbar hierarchy: search, sort, grouping, card/table view selection, select-all, and batch-action region, followed by the account list. Reuse existing account primitives where their data contract fits. Every action must remain provider-scoped. Hide actions that have no supported operation instead of rendering inert controls. Match account row/card density, loading skeletons, empty state, and narrow-width wrapping.

**Check:** For all three account URLs, verify toolbar ordering, card/table switching, provider isolation, add/refresh/delete feedback, confirmation before delete, and responsive wrapping.

### Task 8: Align Usage with the prototype

**Files:** `src/pages/ExternalProviderPage.tsx`, `src/components/usage/view-param-bar.tsx` if its supported controls fit, external platform locale domains.

Match the prototype hierarchy of page header, view tabs, one shared parameter bar, and a content region. Keep filters page-level so switching views does not reset them. Offer only filters supported by the selected provider's request log or local snapshot data. Keep balance, request records, and local samples visibly distinct, including explicit source notes and meaningful empty states.

**Check:** For all three usage URLs, switch each view and confirm the parameter bar remains in the same position and keeps its values; verify empty data is not presented as zero usage and all provider data remains isolated.

### Task 9: Align Gateway with the prototype

**Files:** `src/pages/ExternalProviderPage.tsx`, `src/components/shell/side-subnav.tsx`, external platform locale domains.

Replace the current top tabs with the prototype split layout: left section navigation and right content panel, using the shared responsive split/subnav primitives. Keep a service status panel and connection guide for the overview; provider-supported model and integration sections use the same right-panel card hierarchy. Do not expose unavailable gateway actions as functional controls.

**Check:** For all three gateway URLs, verify selected subnavigation, stable direct navigation, service status, actual model/protocol information, and vertical stacking at the prototype breakpoint.

### Task 10: Align Settings with the prototype

**Files:** `src/pages/ExternalProviderPage.tsx`, `src/components/settings-primitives.tsx`, `src/components/shell/side-subnav.tsx`, external platform locale domains.

Use the prototype split layout with a left settings-section subnav and right grouped setting cards. Keep the provider scope note visible. Only show the real per-provider refresh interval and low-balance threshold controls; retain existing persistence and validation. Match card headers, rows, divider lengths, selected navigation, and responsive stacking.

**Check:** For all three settings URLs, switch sections and verify only the active provider's settings change and persist; confirm layout matches the prototype at desktop and narrow widths.

### Task 11: Verify the 15-page alignment and package the Windows build

**Files:** `docs/superpowers/specs/2026-11-25-external-platforms-design.md`, `docs/README.md` only if the index changes, existing release/package output.

Walk the 3-provider × 5-entry matrix against `docs/prototype/index.html`, checking route deep links, page hierarchy, title/scope, selected navigation, loading/empty/error states, visible action behavior, and narrow-screen layout. Run TypeScript no-emit, `git diff --check`, and the project's Windows installer build. Do not run test suites unless requested. Preserve pre-existing unrelated changes and do not stage or commit shared-index contents.

**Check:** Record a pass/fail row for each of the 15 provider/entry combinations; deliver the generated setup `.exe` with its path and SHA-256. Any failed row remains open and blocks completion.

## Execution Notes

- Implementation order is task 1 → task 2 → task 3 → task 4 → task 5.
- Preserve existing staged/unstaged user changes; never stage or revert unrelated paths.
- Gateway UI must use actual per-provider capability state; do not add fake start/stop controls.
- UI alignment order is task 6 → task 7 → task 8 → task 9 → task 10 → task 11. Shared page composition is implemented once and checked separately against all three providers.
