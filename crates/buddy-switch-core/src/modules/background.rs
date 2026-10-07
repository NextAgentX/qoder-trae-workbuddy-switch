//! 后台任务注册表（P5 §2.2）—— **「有哪些后台任务」的唯一事实来源**。
//!
//! ## 为什么必须有这一层（本仓已出过三次真实事故）
//!
//! 三次事故的共同形态是「**调用点静默消失**」——函数存在、类型全对、**构建全绿、
//! 测试全绿，但生产上没人调用**：
//!
//! | 事故 | 静默后果 |
//! |:---|:---|
//! | `Pool::set_credits` 无生产调用点 | 池里 `credits` 恒为 0 ⇒ 四因子加权静默退化为两因子 |
//! | `Pool::flush_if_dirty` 无生产调用点 | 冷却 / 熔断 / 成功率 EMA 只在内存里，重启即清零 |
//! | 自动备份未登记 | 生产上永不产生自动备份，而用户以为「出事时有备份」 |
//!
//! 对策（**登记即执行**）：把「有哪些后台任务」做成**数据**（[`TASKS`]），两个宿主
//! （`src-tauri` 与 `buddy-switch-server`）**严格按本表**启动；新增「写状态 / 周期性」
//! 出口**必须先在本表登记**，否则不会被任何宿主启动。
//!
//! ## 二维化：`(Product, TaskKind)`
//!
//! 改造前两宿主各有一份**扁平**枚举，且**产品名进了枚举名**（`TraeCreditsRefresh`），
//! 于是「任务种类」与「产品线」两个正交维度被混在一起。本模块把它拆成二维：
//! [`TaskSpec::product`] × [`TaskSpec::kind`]。
//!
//! ## 落地边界（P5 §2.2）
//!
//! - **表本体**（「有哪些任务」）在本模块（core，两宿主共用）；
//! - **`spawn` 分支**（「怎么启动」）仍在**各宿主**里 —— 启动方式与宿主运行时绑定
//!   （Tauri 用 `tauri::async_runtime::spawn`，server 用 `tokio::spawn`）。
//!
//! 这**不破坏**「两侧宿主都要登记」的既有约定：本表是唯一来源，**两个宿主的 spawn
//! 派发都必须覆盖本表出现的每一个 [`TaskKind`]**。该约定由**两道**护栏共同守住：
//! 1. **编译期**：两宿主的 `match spec.kind` **穷尽**（无通配分支）⇒ 新增一类
//!    [`TaskKind`] 会让**两个宿主都编译失败**；
//! 2. **运行期**：`tests::both_hosts_register_every_task_kind` 直接读**两个宿主的源码**，
//!    断言各自的 `spawn` 派发都有一条对应分支（防「构建全绿但某侧没派发」）。

use std::time::Duration;

use crate::modules::product::Product;
use crate::modules::schedule::ScheduleTask;

/// WorkBuddy 账号池治理状态落盘周期。
///
/// 取值理由：`Pool::flush_if_dirty` 只在**有改动**时才真正写盘，所以这里定的是
/// 「最坏情况下最多丢多少治理状态」。30 秒足够短（冷却 / 熔断的时效是分钟级，
/// 丢 30 秒不会造成错误决策），又远长于一次请求。
pub const POOL_PERSIST_INTERVAL: Duration = Duration::from_secs(30);

/// 账号池**余额 / 积分**刷新周期。
///
/// 与 WorkBuddy 余额刷新的缺省间隔（30 分钟）一致：同一管理面不该有两种节奏。
/// ⚠️ WorkBuddy 侧实际等待时长由 `credits_refresh::next_refresh_wait_ms` 动态给出
/// （池为空 ⇒ 短探测），此常量只是**标称值**（见 [`TaskSpec::interval`]）。
pub const CREDITS_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// 定时备份的**检查**周期。
///
/// 取 1 小时：`backup::auto_backup()` 当日已备份时**幂等直接返回**（不重复读全量），
/// 故检查本身开销极低；小时级足以在跨日后尽快产生当日备份。
pub const AUTO_BACKUP_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// 自动轮换的**检查**周期。
///
/// 真正的轮换间隔由 `auto_rotate_config.check_interval_minutes` 决定，本值只是
/// 「多久看一眼配置」的心跳。
pub const AUTO_ROTATE_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// 后台任务的**种类**（L2）—— 与产品线（[`TaskSpec::product`]）**正交**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// 启动补跑：整理历史签到日志 + 签到核验 / 旅行派出领取 / 保活各跑一轮
    /// （**受各自排程开关约束**，关掉的任务一次都不跑）。
    StartupMaintenance,
    /// 自动轮换（按 `auto_rotate_config` 间隔；CodeBuddy CLI 为 CN 专有）。
    AutoRotate,
    /// 账号池**余额 / 积分刷新**（真实读数回填选号权重）。
    ///
    /// 二维化后**两产品共用此 kind**，由 [`TaskSpec::product`] 区分实现：
    /// - `(Workbuddy, CreditsRefresh)`：WorkBuddy 账号池余额刷新；
    /// - `(Trae, CreditsRefresh)`：Trae 两条国内产品线的积分刷新。
    ///
    /// 两者的**实现完全不同**（前者走 `gateway::shared_state` 的池，后者走
    /// `trae::credits` 的区域账号库），这正是「产品线」与「任务种类」是两个维度
    /// 而非一个扁平枚举的实证。
    CreditsRefresh,
    /// 账号池**治理状态落盘**（冷却 / 熔断 / 成功率 EMA / 余额读数）。
    ///
    /// 缺了它这些状态只在内存里，每次重启从零开始 —— 例如刚判定 `SessionDead`
    /// 的账号重启后立刻又被选中、再撞一次同样的墙。
    PoolPersist,
    /// **定时全局备份**（D17）：每日一份、保留 7 份、写 `backup_dir()`。
    ///
    /// **默认开**、但可在设置里关掉 —— 关闭后本任务不再产生新备份。
    AutoBackup,
    /// **网关启动补识别**（P10 §1.6 / T03 · ✅ #7：要登记）。
    ///
    /// 启动时对每个「待确认」region（内置，或 `fetched_at` 超过 24h）**各探测一次**，
    /// 把清单从「内置 / 陈旧」补成「实时」。
    ///
    /// ## 为什么是**一次性**任务（不是 `loop`）
    ///
    /// 判据（`catalog_probe::stale_regions`）只在**启动这一刻**有意义：补一次即可。
    /// 之后由「接入即识别」（`create_api_key` 后的探测）与手动「重新识别」维持新鲜度 ——
    /// 无脑循环探测只会白打上游。
    ///
    /// ## 为什么 `create_api_key` 后的那次探测**不登记**在本表
    ///
    /// 本表是**启动期**任务清单（两宿主的 `spawn_background_loops` 各跑一遍）。
    /// `create_api_key` 的探测是**命令触发**的（用户新建 Key 才跑），与「启动」无关：
    /// 登记进本表会让它**每次启动都跑**。它的「不被静默丢弃」由调用点本身守住
    /// （`commands::create_api_key` 里显式 `spawn(probe_catalog(...))`）—— 不是漏登记。
    ///
    /// ⚠️ **两宿主必须同步登记**（登记即执行 / P5 R9）：缺了它启动时不会补识别
    /// 「内置 / 陈旧」的清单，用户只能看到「内置」来源条。
    GatewayCatalogProbe,
    /// 六 / 七类积分定时任务之一（按 `schedule_config` 的小时表，各自独立排程）。
    Scheduled(ScheduleTask),
}

impl TaskKind {
    /// **种类级**稳定标识（供日志 / 诊断）：`startup_maintenance` / `credits_refresh` / …
    ///
    /// [`TaskKind::Scheduled`] 拼上定时任务标识（`scheduled:checkin`）。
    ///
    /// ⚠️ 本函数的返回值在**整张表**上**不保证**互不相同：[`TaskKind::CreditsRefresh`] 是
    /// **两个产品共用的同一 kind**（二维化的本意）⇒ 裸 kind 标签会重复（`credits_refresh`
    /// 出现两次）。**任务级**唯一标识请用 [`TaskSpec::label`]（`产品:种类`）。
    pub fn label(self) -> String {
        match self {
            TaskKind::StartupMaintenance => "startup_maintenance".to_string(),
            TaskKind::AutoRotate => "auto_rotate".to_string(),
            TaskKind::CreditsRefresh => "credits_refresh".to_string(),
            TaskKind::PoolPersist => "pool_persist".to_string(),
            TaskKind::AutoBackup => "auto_backup".to_string(),
            TaskKind::GatewayCatalogProbe => "gateway_catalog_probe".to_string(),
            TaskKind::Scheduled(task) => format!("scheduled:{}", task.as_str()),
        }
    }
}

/// 一条后台任务：`(产品线, 种类)` 二维坐标 + 标称周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSpec {
    /// 该任务服务的产品线。
    pub product: Product,
    /// 任务种类。
    pub kind: TaskKind,
    /// **标称**周期，仅作诊断 / 文档用途（启动分支按各自语义决定实际等待）：
    ///
    /// - `Duration::ZERO` = 「由配置 / 小时表 / 动态判据驱动，无固定周期」
    ///   （启动补跑、WorkBuddy 余额刷新、六类定时任务）；
    /// - 非零 = 固定（或检查）周期（落盘 / Trae 积分刷新 / 备份 / 轮换检查）。
    pub interval: Duration,
}

impl TaskSpec {
    /// **任务级**稳定标识（`产品:种类`，例 `wb:credits_refresh` / `trae:credits_refresh`）。
    ///
    /// ## 为什么标识必须带产品（而不是只有 kind）
    ///
    /// 一个后台任务的**身份**是 `(产品, 种类)` 这个**二维坐标**，而不是单独的种类：
    /// [`TaskKind::CreditsRefresh`] 被**两个产品共用**（二维化的本意）——
    /// 只按 kind 打标签会让「WorkBuddy 余额刷新」与「Trae 积分刷新」**同名**，
    /// 日志 / 诊断里无从区分，而两者的实现与数据源**完全不同**
    /// （前者走 `gateway::shared_state` 的池，后者走 `trae::credits` 的区域账号库）。
    ///
    /// 故本函数的返回值在**整张表**上**互不相同**（由 `task_labels_are_stable_and_distinct` 守住）。
    pub fn label(&self) -> String {
        format!("{}:{}", self.product.slug(), self.kind.label())
    }
}

/// ★ **单一事实来源**：两个宿主共用的后台任务清单。
///
/// ⚠️ **登记即执行**：新增「写状态 / 周期性」出口必须在此登记，否则不会被任何宿主启动。
///
/// 六 / 七类定时任务由 [`TaskKind::Scheduled`] **逐条**登记：`schedule.rs` 的宏新增一类
/// 定时任务后，必须同步在此补一行，否则两宿主的
/// `background_tasks_cover_all_scheduled_tasks` 会变红。
pub const TASKS: &[TaskSpec] = &[
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::StartupMaintenance,
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::AutoRotate,
        interval: AUTO_ROTATE_CHECK_INTERVAL,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::CreditsRefresh,
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::PoolPersist,
        interval: POOL_PERSIST_INTERVAL,
    },
    TaskSpec {
        product: Product::Trae,
        kind: TaskKind::CreditsRefresh,
        interval: CREDITS_REFRESH_INTERVAL,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::AutoBackup,
        interval: AUTO_BACKUP_CHECK_INTERVAL,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::GatewayCatalogProbe,
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::Scheduled(ScheduleTask::Checkin),
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::Scheduled(ScheduleTask::Travel),
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::Scheduled(ScheduleTask::Activity),
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::Scheduled(ScheduleTask::Keepalive),
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::Scheduled(ScheduleTask::School),
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Workbuddy,
        kind: TaskKind::Scheduled(ScheduleTask::Cat),
        interval: Duration::ZERO,
    },
    TaskSpec {
        product: Product::Trae,
        kind: TaskKind::Scheduled(ScheduleTask::TraeCheckin),
        interval: Duration::ZERO,
    },
];

/// 是否存在「该产品线的该种类」任务。
pub fn has_task(product: Product, kind: TaskKind) -> bool {
    TASKS
        .iter()
        .any(|spec| spec.product == product && spec.kind == kind)
}

/// 本表登记的全部定时任务种类（按声明顺序，去重）。
pub fn scheduled_tasks() -> Vec<ScheduleTask> {
    let mut out: Vec<ScheduleTask> = Vec::new();
    for spec in TASKS {
        if let TaskKind::Scheduled(task) = spec.kind {
            if !out.contains(&task) {
                out.push(task);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两个宿主的源码（**跨文件护栏**的读取对象）。
    ///
    /// ⚠️ 路径相对 **core 的 `CARGO_MANIFEST_DIR`**（`crates/buddy-switch-core`）：
    /// `../..` = 仓库根。与 `product.rs` 读取前端 `registry.ts` 同款做法。
    const TAURI_LIB_RS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-tauri/src/lib.rs");
    const SERVER_MAIN_RS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/buddy-switch-server/src/main.rs"
    );

    fn read_source(path: &str) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("读不到 {path}: {error}"))
    }

    /// 某一 [`TaskKind`] 在两个宿主的 `spawn` 派发里应有的**分支标记**。
    ///
    /// 标记带 `=>`（或 `(`）而**不是**裸类型名：宿主的**测试**里也会出现
    /// `TaskKind::AutoBackup`（作为 `has_task` 的实参），若标记不带箭头，删掉 spawn
    /// 分支后测试自身的引用仍会「满足」断言 ⇒ 护栏在空转。带箭头可精确锚定**分支**。
    fn spawn_arm_marker(kind: TaskKind) -> &'static str {
        match kind {
            TaskKind::StartupMaintenance => "TaskKind::StartupMaintenance =>",
            TaskKind::AutoRotate => "TaskKind::AutoRotate =>",
            TaskKind::CreditsRefresh => "TaskKind::CreditsRefresh =>",
            TaskKind::PoolPersist => "TaskKind::PoolPersist =>",
            TaskKind::AutoBackup => "TaskKind::AutoBackup =>",
            TaskKind::GatewayCatalogProbe => "TaskKind::GatewayCatalogProbe =>",
            TaskKind::Scheduled(_) => "TaskKind::Scheduled(",
        }
    }

    /// 表必须覆盖**全部**定时任务（宏 `define_schedule_tasks!` 是定时任务集合的唯一来源）。
    ///
    /// 可证伪：从 [`TASKS`] 删掉任一 `Scheduled(...)` 会让本用例变红 —— 该任务在**两个
    /// 宿主里**都不会被启动（排程控件空转，典型症状是「活跃地图」从未点亮）。
    #[test]
    fn tasks_cover_all_scheduled_tasks() {
        let scheduled = scheduled_tasks();
        for task in ScheduleTask::all() {
            assert!(
                scheduled.contains(&task),
                "TASKS 缺少定时任务「{}」——它在任何宿主里都不会执行（登记即执行）",
                task.as_str()
            );
        }
        assert_eq!(
            scheduled.len(),
            ScheduleTask::all().len(),
            "TASKS 的定时任务登记数必须与 ScheduleTask::all() 一致（不得漏登 / 重复）"
        );
    }

    /// 三次真实事故对应的任务**必须**登记在表里。
    ///
    /// 可证伪：删掉任一条 ⇒ 对应用例断言变红。
    #[test]
    fn tasks_include_every_key_background_task() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::CreditsRefresh),
            "缺 WorkBuddy 账号池余额刷新 ⇒ 池里 credits 恒为 0，四因子加权静默退化为两因子"
        );
        assert!(
            has_task(Product::Trae, TaskKind::CreditsRefresh),
            "缺 Trae 积分刷新 ⇒ 纯用 API 时积分读数永远陈旧"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::PoolPersist),
            "缺账号池治理状态落盘 ⇒ 冷却 / 熔断 / 成功率 EMA 只在内存里、重启清零"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::AutoBackup),
            "缺定时全局备份 ⇒ 生产上永不产生自动备份（P5 R9「登记即执行」）"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::GatewayCatalogProbe),
            "缺网关启动补识别（P10 T03 / ✅ #7）⇒ 「内置 / 陈旧」的模型清单永远不会在启动时\
             被补成实时，用户只能看到「内置」来源条"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::StartupMaintenance),
            "缺启动补跑"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::AutoRotate),
            "缺自动轮换"
        );
    }

    /// 二维化落地：`CreditsRefresh` 是**同一 kind 的两个产品**（不是两个扁平变体）。
    #[test]
    fn credits_refresh_is_one_kind_across_two_products() {
        let wb = TASKS
            .iter()
            .find(|s| s.product == Product::Workbuddy && s.kind == TaskKind::CreditsRefresh)
            .expect("WorkBuddy 余额刷新必须在表里");
        let trae = TASKS
            .iter()
            .find(|s| s.product == Product::Trae && s.kind == TaskKind::CreditsRefresh)
            .expect("Trae 积分刷新必须在表里");
        // 同一 kind，两个 product ⇒ 正是二维坐标。
        assert_eq!(wb.kind, trae.kind, "两者必须共用同一 TaskKind（二维化）");
        assert_ne!(wb.product, trae.product);
    }

    /// **任务级**标识（`产品:种类`）必须稳定且**互不相同**。
    ///
    /// 回归背景：`TaskKind::CreditsRefresh` 被**两个产品共用**（二维化的本意）——
    /// 若按**裸 kind** 打标签，「WorkBuddy 余额刷新」与「Trae 积分刷新」会**同名**
    /// （都是 `credits_refresh`），日志 / 诊断里无从区分，而两者的实现与数据源**完全不同**。
    /// 故任务级标识取 `(product, kind)` 二维坐标（[`TaskSpec::label`]）。
    ///
    /// 可证伪（**变异验证 ②**）：把 [`TaskSpec::label`] 改回只返回 `self.kind.label()`
    /// ⇒ 两个 `credits_refresh` 撞名 ⇒ 本用例变红。
    #[test]
    fn task_labels_are_stable_and_distinct() {
        let labels: Vec<String> = TASKS.iter().map(|spec| spec.label()).collect();
        for label in &labels {
            assert!(!label.is_empty(), "任务标识不得为空");
        }
        let mut sorted = labels.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), labels.len(), "任务标识必须互不相同：{labels:?}");
        // 二维坐标：同一 kind 的两个产品必须产生**不同**的任务级标签。
        assert!(labels.iter().any(|l| l == "wb:credits_refresh"), "{labels:?}");
        assert!(labels.iter().any(|l| l == "trae:credits_refresh"), "{labels:?}");
        // 定时任务的标识带上类别后缀，便于日志区分。
        assert!(labels.iter().any(|l| l == "wb:scheduled:checkin"), "{labels:?}");
        assert!(
            labels.iter().any(|l| l == "trae:scheduled:trae_checkin"),
            "{labels:?}"
        );
    }

    /// ★★ **两侧登记**护栏（跨文件）—— P5 R9「登记即执行」的机器守卫。
    ///
    /// 表本体移到 core 后，唯一能防住「某侧 spawn 漏了一类任务」的，就是本用例：它直接读
    /// **两个宿主的源码**（`src-tauri/src/lib.rs` 与 `buddy-switch-server/src/main.rs`），
    /// 断言各自的 `spawn` 派发都覆盖了 [`TASKS`] 出现的**每一个** [`TaskKind`]。
    ///
    /// 为什么能读出「某侧漏了」：两个宿主**各持一份 `spawn` 分支**（启动方式与运行时绑定，
    /// 刻意不去重）。只改一侧 ⇒ 该侧源码里对应的分支标记消失 ⇒ 本用例变红。
    ///
    /// ⚠️ 本用例在 **core crate** 里跑，**不编译** `src-tauri`：即便某一侧的 `match`
    /// 因删分支而变得不穷尽（编译期也会红），`cargo test -p buddy-switch-core` 仍能运行，
    /// 从而给出**可读的断言失败**而不是一句编译错误。
    ///
    /// 可证伪（**变异验证 ①**）：从任一侧删掉 `TaskKind::AutoBackup` 的 `spawn` 分支，
    /// 本用例立即变红。
    #[test]
    fn both_hosts_register_every_task_kind() {
        for host in [TAURI_LIB_RS, SERVER_MAIN_RS] {
            let source = read_source(host);
            for spec in TASKS {
                let marker = spawn_arm_marker(spec.kind);
                assert!(
                    source.contains(marker),
                    "宿主 {host} 的 spawn 派发缺少分支 `{marker}` —— \
                     该任务在此宿主里**永远不会执行**（登记即执行 / P5 R9）"
                );
            }
        }
    }

    /// 反向护栏：`spawn_arm_marker` 必须覆盖 `TaskKind` 的**全部**取值。
    ///
    /// 可证伪：新增一个 [`TaskKind`] 变体却忘了给 `spawn_arm_marker` 加分支 ⇒ 本 `match`
    /// 不穷尽 ⇒ 编译失败（把「护栏自身漏项」也变成编译期强制）。
    #[test]
    fn spawn_arm_marker_covers_every_kind() {
        for kind in [
            TaskKind::StartupMaintenance,
            TaskKind::AutoRotate,
            TaskKind::CreditsRefresh,
            TaskKind::PoolPersist,
            TaskKind::AutoBackup,
            TaskKind::GatewayCatalogProbe,
            TaskKind::Scheduled(ScheduleTask::Checkin),
        ] {
            assert!(!spawn_arm_marker(kind).is_empty(), "{kind:?}");
        }
    }
}
