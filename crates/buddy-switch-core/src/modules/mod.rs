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
pub mod codebuddy_cli;
pub mod codebuddy_cn_ide;
// P9 · 桌面悬浮窗的**只读**数据模型 + provider（跨来源的会话 / 任务状态）。
pub mod companion;
pub mod config;
pub mod connector;
pub mod credit_usage;
pub mod credits;
pub mod cst;
pub mod error_code;
pub mod export_import;
pub mod identity;
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
pub mod switch;
pub mod token_stats;
pub mod trae;
pub mod travel;
pub mod update;
pub mod upstream;
pub mod vscode_cn_inject;
