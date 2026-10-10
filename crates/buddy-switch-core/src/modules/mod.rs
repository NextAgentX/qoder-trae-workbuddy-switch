pub mod account;
pub mod activity;
pub mod at_rest;
pub mod auth_file;
pub mod backup;
pub mod backup_sections;
// P5 §2.2 · 后台任务注册表（`TaskKind` + `TaskSpec` + `TASKS`）—— 两宿主共用的唯一事实来源。
pub mod background;
// P12 T03 · 业务转发的**路径白名单**（本地路由）—— 放在 core 是因为**两侧都要用**：
// `gateway` 拿它做 404 判定，`proxy.rs` 拿它暴露给面板（**避免两侧各写一份**）。
pub mod biz_allowlist;
pub mod capability;
pub mod cat;
pub mod catalog;
pub mod checkin;
// 客户端限流可视化（账号 × 模型 × 恢复时刻）—— **纯只读**三表 join。
pub mod client_limits;
// 上游客户端 settings.json 统一托管（读 · 编辑 · 备份 · 恢复 · 差异比对）——
// 覆盖 WorkBuddy/Trae 的 CN/Global 全部程序位，**外科式文本编辑**保留原排版。
pub mod client_settings;
pub mod codebuddy_cli;
pub mod codebuddy_cn_ide;
// P9 · 桌面悬浮窗的**只读**数据模型 + provider（跨来源的会话 / 任务状态）。
pub mod companion;
pub mod config;
pub mod connector;
// Q1 · 诊断包导出（只读、复用备份的凭据剥离键集；不含会话正文与路径）。
pub mod diagnostics;
// E1 · 代理 KPI 台账的只读视图（core 只读 gateway 写的台账，不引入反向依赖）。
pub mod proxy_kpi_view;
pub mod credit_usage;
pub mod credits;
pub mod cst;
pub mod error_code;
pub mod export_import;
pub mod identity;
// JetBrains IDE（IDEA / PyCharm）内 CodeBuddy 插件账号切换（secret-storage.xml）。
pub mod jetbrains;
pub mod memory;
pub mod migrate;
pub mod net;
pub mod oauth;
pub mod official_usage;
pub mod process;
pub mod product;
// P5 §2.5 · WorkBuddy 程序位表（`ProgramSpec` + `ProgramAdapter` trait）—— 表驱动 + 统一操作面。
pub mod programs;
// P12 · C 路（base URL 重定向）的**本地状态** —— 只记标记，**不改任何系统设置**
// （T02 定的是**引导式**：应用只显示要设的 env，由用户手动设；见 `P12-ARCH.md`）。
pub mod proxy;
pub mod refresh;
pub mod region;
pub mod rotate;
pub mod schedule;
pub mod scheduler;
pub mod school;
pub mod session;
// Skill 管理（cc-switch 式）：清单 + 启停 + 跨区搬迁 + 本地导入 —— 目录即 SSOT。
pub mod skill;
pub mod switch;
// 每日 Token 记录（账号 × 来源 × 本地日）—— 持久化「今日 Token」，供 UI 展示。
pub mod token_daily;
pub mod token_stats;
pub mod trae;
pub mod travel;
pub mod update;
pub mod upstream;
// VS Code 内 CodeBuddy 扩展（tencent-cloud.coding-copilot）账号切换。
pub mod vscode_ext;
pub mod vscode_cn_inject;
