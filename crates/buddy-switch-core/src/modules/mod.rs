pub mod account;
pub mod activity;
// P15 · 外部 Agent 会话监听：`hook 事件 → 会话状态` 的纯状态机 —— 悬浮窗的**第二个数据源**
// （第一个是 `companion.rs` 消费的自家切换进度）。**零周期任务**：过期靠读取时惰性判定。
pub mod agent_watch;
// P15 · hook 的本地接收端（只绑 127.0.0.1）+ 客户端 hook 调用的 `hook` 子命令。
pub mod agent_hook_rx;
// P15 · 往客户端 `settings.json` 装 / 卸本工具的 hook（复用 `client_settings` 的
// 备份 / 进程守卫 / 格式保持，**不新造写入路径**）。
pub mod agent_hook_install;
// L0 应用级偏好里**宿主也要读**的那一类（如「关闭窗口是否隐藏到托盘」）——
// 纯渲染层的偏好留在 `localStorage`，别搬进来（见模块头）。
pub mod app_prefs;
pub mod at_rest;
pub mod auth_file;
pub mod backup;
pub mod backup_sections;
// P5 §2.2 · 后台任务注册表（`TaskKind` + `TaskSpec` + `TASKS`）—— 两宿主共用的唯一事实来源。
pub mod background;
// P12 T03 · 业务转发的**路径白名单**（本地路由）—— 放在 core 是因为**两侧都要用**：
// `gateway` 拿它做 404 判定，`proxy.rs` 拿它暴露给面板（**避免两侧各写一份**）。
pub mod biz_allowlist;
pub mod cache;
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
pub mod credit_usage;
pub mod credits;
pub mod cst;
pub mod error_code;
pub mod export_import;
pub mod identity;
pub mod proxy_kpi_view;
pub mod reconcile;
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
// Qoder（第三个产品线）：双区账号管理。与 WorkBuddy / Trae 平行，**不复用**彼此的区域表。
pub mod qoder;
pub mod refresh;
pub mod region;
// 统一网关的**模型命名空间**（`wb-cn:` / `trae-global:` 前缀）——
// 派生层由 `Scope::all()` 生成 ⇒ 新增平台时命名空间自动出现；详见模块头。
pub mod gateway_namespace;
// Trae / Qoder 的**网关逐日账本**（用量页「账本」视图的数据源）。
// 与 `token_daily` 同源同语义，但它只记网关一本（这两个平台没有可解析的客户端落盘）。
pub mod gateway_ledger;
pub mod rotate;
// 本工具**自身**的运行日志（排查问题用）：按日纯文本 + 有界滚动 + 出站脱敏。
// ⚠️ 与「网关请求日志」（JSON、面向统计）和「Trae 客户端日志」是三件不同的事，见模块头。
pub mod runtime_log;
pub mod schedule;
pub mod scheduler;
pub mod school;
pub mod session;
pub mod session_bundle;
// 追加型 jsonl 的尾部窗口读取（token_daily / client_limits 共用的唯一实现）。
pub(crate) mod jsonl_tail;
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
pub mod vscode_cn_inject;
pub mod vscode_ext;
/// VS Code 插件 / CodeBuddy IDE 的**会话仓**（P14 S4a · 只读半）。
pub mod vscode_session;
