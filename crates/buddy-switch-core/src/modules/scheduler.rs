//! 定时任务的**运行时**调度：派发、按点循环、启动补跑。
//!
//! 与 [`super::schedule`] 的分工：`schedule` 只管配置与「下一次几点触发」这类**纯函数**
//! （可在无 tokio 运行时的单测里验证）；本模块管**真的去跑**。
//!
//! ⚠️ 历史缺陷（本模块存在的原因）：排程循环此前**只写在 `buddy-switch-server` 二进制里**，
//! 而桌面端（`src-tauri`）的 `spawn_background_loops` 是四个**写死周期**的循环（签到 30 分钟 /
//! 旅行 30 分钟 / 领取 15 分钟 / 保活每天一次）。于是设置页「定时任务排程」的 13 个控件
//! （六类开关 + 六份小时表 + 活跃上报次数）在桌面端**全部空转**——最典型的是「活跃地图」
//! 从未执行，用户在官网对照连登热力图发现始终没点亮。
//!
//! 现在桌面端与服务端**共用这一份**实现：两端只负责 `spawn`，排程语义只有一份，不会漂移。

use serde_json::{json, Value};
use std::time::Duration;

/// Trae 分区（第二条产品线）的定时签到。**与上面的 `checkin` 是两套**：
/// 前者签 WorkBuddy 的账号库，它签 Trae 的区域账号库，两者数据、端点、凭据全不相通。
///
/// 刻意走 `trae::handlers` 而不是 `trae::checkin`：签到选项的解析链
/// 「请求参数 > 用户设置 > 内置默认」在 handlers 里单点承担，绕过去就会让设置页的
/// 「重试次数 / 跳过已签 / 跳过过期」对自动签到失效（本仓库已踩过三次的假控件）。
use super::trae::handlers as trae_handlers;
use super::{
    activity, cat, checkin, config, qoder, refresh,
    region::Region,
    schedule::{self, ScheduleConfig, ScheduleTask},
    school, travel,
};

/// 任务被禁用（`hours` 为空）时重查配置的间隔：1 分钟。
///
/// 禁用态不能退出循环（否则用户后来打开开关就再也不会生效），只需低频重查。
const DISABLED_RECHECK_MS: u64 = 60_000;

/// 哪些任务在**进程启动时补跑一轮**。
///
/// 只补「今天该做但还没做」的几类：签到核验（WorkBuddy 与 Trae 各一）、Trae JWT 续期、
/// 旅行派出+领取、保活。活跃上报 / 开学季 / 夜猫子不在其中——它们是「每天至多一次」的
/// 积分动作，交给排程小时表即可，启动就跑会让「我明明设了 10 点」的语义失效。
///
/// Trae 签到**在其中**：排程定在 9 点、而用户 10 点才开这个应用是常态，
/// 不补跑就会变成「打开时已过点，今天整天不签」——那等于「自动签到」对多数人不生效。
/// 它有自己的开关且**默认关闭**（见 `schedule::default_schedule`），
/// 因此补跑不会让任何人被突如其来的外部请求影响。
///
/// Trae JWT 续期**也在其中**（2026-10-14 新增）：它是「凭据到期前必须有人换」的动作，
/// 而到期时刻由**上游签发时决定**、与我们的小时表无关 —— 应用恰好没开在那两个整点是
/// 常态，不补跑就等于「续期看运气」。它只对**真的快到期**的账号发请求
/// （`account::needs_jwt_refresh`），没有到期账号时这一轮只是读一遍账号库。
const STARTUP_TASKS: [ScheduleTask; 7] = [
    ScheduleTask::Checkin,
    ScheduleTask::TraeCheckin,
    ScheduleTask::TraeJwtRefresh,
    ScheduleTask::Travel,
    ScheduleTask::Keepalive,
    // Qoder 两条与 Trae 两条**同判据**：签到默认关（opt-in）、保活默认开。
    ScheduleTask::QoderCheckin,
    ScheduleTask::QoderKeepalive,
];

/// 启动补跑的任务清单（**纯函数**）：受各自排程开关约束，关掉的任务一次都不跑。
///
/// 为什么必须显式按开关过滤：桌面端此前的启动补跑是**无条件**的，于是「关闭某类任务」
/// 的开关在桌面端形同虚设（关了照样跑）。抽成纯函数后可用单测钉住这一点。
pub fn startup_tasks(cfg: &ScheduleConfig) -> Vec<ScheduleTask> {
    STARTUP_TASKS
        .into_iter()
        .filter(|task| task.enabled(cfg))
        .collect()
}

/// 派发一类定时任务并返回它的执行结果。
///
/// 返回值供「立即执行」这类手动入口展示：各业务模块自身的返回原样内嵌，便于定位到具体
/// region / 账号（例如活跃上报会带回每个账号的 `reported` 与 `streakDays`）。
///
/// CN / Global 的 region 隔离在此收敛：签到 / 旅行 / 开学季 / 夜猫子只有 CN 有对应上游
/// （Global 侧由各模块自行返回结构化 `unsupported`，独立降级）；活跃上报与保活按 region
/// 各自执行。
pub async fn run_scheduled_task(task: ScheduleTask) -> Value {
    match task {
        ScheduleTask::Checkin => {
            let mut regions = Vec::new();
            for region in Region::all() {
                regions.push(
                    checkin::run_checkin_cycle_for(
                        region,
                        checkin::CheckinCycleMode::PeriodicRecovery,
                    )
                    .await,
                );
            }
            json!({ "task": task.as_str(), "regions": regions })
        }
        // 一趟派出 + 一趟领奖闭环。
        ScheduleTask::Travel => {
            let dispatched = travel::run_travel_cycle().await;
            let claimed = travel::run_travel_claim_cycle().await;
            json!({ "task": task.as_str(), "dispatched": dispatched, "claimed": claimed })
        }
        ScheduleTask::Activity => {
            let mut regions = Vec::new();
            for region in Region::all() {
                regions.push(activity::run_activity_cycle_for(region).await);
            }
            json!({ "task": task.as_str(), "regions": regions })
        }
        ScheduleTask::Keepalive => {
            let mut regions = Vec::new();
            for region in Region::all() {
                regions.push(refresh::run_keepalive_cycle_for(region).await);
            }
            json!({ "task": task.as_str(), "regions": regions })
        }
        ScheduleTask::School => {
            json!({ "task": task.as_str(), "result": school::run_school_cycle_for(Region::Cn).await })
        }
        ScheduleTask::Cat => {
            json!({ "task": task.as_str(), "result": cat::run_cat_cycle_for(Region::Cn).await })
        }
        // Trae 的账号体系与 WorkBuddy **没有 region 交集**（`Region` 只有 CN/Global，
        // 而 Trae 的区域是它自己那本账号库的轴），因此这里不套 `Region::all()` 循环：
        // 遍历哪个区域由 `trae::handlers::run_scheduled_checkin` 自己决定（两本库各签一轮）。
        ScheduleTask::TraeCheckin => {
            json!({ "task": task.as_str(), "result": trae_handlers::run_scheduled_checkin().await })
        }
        // 同上：Trae 的账号库按**变体**分家（不是 `Region`），遍历哪几条由
        // `run_jwt_refresh_cycle` 自己决定（`all_specs()` 全 6 条，含国际版与 TraeCode）。
        ScheduleTask::TraeJwtRefresh => {
            json!({ "task": task.as_str(), "result": trae_handlers::run_jwt_refresh_cycle().await })
        }
        // Qoder 的账号库同样按它**自己的区域轴**分家（`QoderRegion`，不是 `Region`）——
        // 两者恰好都是 cn/global，但语义不同（`Region` 是 WorkBuddy 的轴）。
        // 显式遍历 `QoderRegion::all()` 而不是复用 `Region::all()`：一旦哪天真加了
        // 第三个区域，这里不会静默少签一个。
        ScheduleTask::QoderCheckin => {
            let mut regions = Vec::new();
            for region in qoder::region::QoderRegion::all() {
                regions.push(qoder::checkin::run_cycle_for(region).await);
            }
            json!({ "task": task.as_str(), "regions": regions })
        }
        ScheduleTask::QoderKeepalive => {
            let mut regions = Vec::new();
            for region in qoder::region::QoderRegion::all() {
                regions.push(qoder::refresh::run_keepalive_cycle_for(region).await);
            }
            json!({ "task": task.as_str(), "regions": regions })
        }
    }
}

/// 某一类任务的排程循环：**永不返回**，由调用方 `spawn` 到后台。
///
/// 每轮**重新读取配置**并重算自己的 `next_fire`，因此运行期间改动时点表或开关、
/// **下一轮即生效**，无需重启应用。任务被禁用（`times` 为空）时不派发，只按 1 分钟重查。
pub async fn schedule_loop(task: ScheduleTask) {
    loop {
        let cfg = schedule::load_schedule_config();
        let now = config::now_ms();
        // 抖动在这里叠加（`jitter == 0` 时与 `next_fire` 等价）：salt 用任务标识，
        // 于是每个任务各自一条偏移序列 —— 同一时点上的多类任务不会一起被推到同一分钟。
        let at = schedule::next_fire_with_jitter(
            task.times(&cfg),
            task.jitter(&cfg),
            task.as_str(),
            now,
        );
        let wait_ms = match at {
            Some(at) => (at - now).max(0) as u64,
            // 任务被禁用：1 分钟后重查配置，避免无谓空转。
            None => DISABLED_RECHECK_MS,
        };
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        if at.is_none() {
            continue;
        }
        let _ = run_scheduled_task(task).await;
    }
}

/// 启动补跑：进程刚起来时先跑一轮「今天该做但还没做」的事，
/// 避免「排程定在 10 点、但应用 11 点才打开」导致当天整天不跑。
///
/// 每类是否补跑**受它自己的排程开关约束**：关掉的任务一次都不跑。
pub async fn run_startup_maintenance() {
    if let Err(error) = config::compact_checkin_logs() {
        eprintln!("[签到] 历史日志整理失败: {error}");
    }
    // 回收站清扫：**复用本入口**（`TaskKind::StartupMaintenance`），不新增调度器。
    //
    // 回收站条目带 30 天 `expires_at`，另有 200 条上限；两者都需要一个「低频、每进程至少跑一次」
    // 的时机来落地。启动补跑正好满足：它天然在进程起来时执行一次，且不依赖任何排程开关
    // （软删除的过期清理不该被用户的「签到/旅行」开关影响）。
    //
    // WorkBuddy 与 Trae 是**两套互不相通**的账号库与回收站，各自清扫，互不吞噬计数。
    let wb_swept = super::account::sweep_all_trash();
    let trae_swept = super::trae::account::sweep_all_trash();
    if wb_swept + trae_swept > 0 {
        eprintln!("[回收站] 启动清扫过期条目: WorkBuddy {wb_swept} / Trae {trae_swept}");
    }
    let cfg = schedule::load_schedule_config();
    for task in startup_tasks(&cfg) {
        let _ = run_scheduled_task(task).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 启动补跑必须**逐类**受自己的排程开关约束。
    ///
    /// 可证伪性：若把 `startup_tasks` 的开关过滤去掉（回到「无条件补跑」的旧行为），
    /// 第三条断言（全关 → 空）会红；若改成「任一开关关闭就全部不补跑」，
    /// 「关 travel 不影响另外两类」与「关 keepalive 不能带走 Trae 签到」两条会红。
    #[test]
    fn startup_tasks_are_gated_per_task_by_its_own_switch() {
        let cfg = schedule::default_schedule();
        // ⚠️ Trae 签到**默认关闭**（它会对用户没授权过的外部服务发请求），
        // 因此默认状态下它不在补跑清单里。这条同时钉住了「默认不替用户做他没授权的事」。
        // ⚠️ 反之 Trae JWT 续期**默认开启**（`refresh_token` 的唯一用途就是续期，
        // 是用户走网页登录时明确交出来的）⇒ 它必须在默认清单里，否则「自动续期」
        // 又只剩小时表那两次机会（应用没开就整天不续）。
        assert_eq!(
            startup_tasks(&cfg),
            vec![
                ScheduleTask::Checkin,
                ScheduleTask::TraeJwtRefresh,
                ScheduleTask::Travel,
                ScheduleTask::Keepalive,
                ScheduleTask::QoderKeepalive,
            ],
            "默认状态下（两条签到默认关闭、两条保活默认开启）启动补跑应恰好是这五类"
        );

        // 打开 Trae 签到后它**必须**进入补跑清单，否则「排程定在 9 点、10 点才开应用」
        // 会让这个开关一整天都不生效。
        let mut with_trae = cfg.clone();
        with_trae.trae_checkin_enabled = true;
        assert_eq!(
            startup_tasks(&with_trae),
            vec![
                ScheduleTask::Checkin,
                ScheduleTask::TraeCheckin,
                ScheduleTask::TraeJwtRefresh,
                ScheduleTask::Travel,
                ScheduleTask::Keepalive,
                ScheduleTask::QoderKeepalive,
            ],
            "Trae 签到开启后必须进入启动补跑"
        );

        // Qoder 签到同判据：默认不在清单里，打开后必须在。
        let mut with_qoder = cfg.clone();
        with_qoder.qoder_checkin_enabled = true;
        assert_eq!(
            startup_tasks(&with_qoder),
            vec![
                ScheduleTask::Checkin,
                ScheduleTask::TraeJwtRefresh,
                ScheduleTask::Travel,
                ScheduleTask::Keepalive,
                ScheduleTask::QoderCheckin,
                ScheduleTask::QoderKeepalive,
            ],
            "Qoder 签到开启后必须进入启动补跑"
        );

        // 关掉 JWT 续期 ⇒ 它**必须**从补跑清单消失（否则开关形同虚设）。
        let mut no_jwt = cfg.clone();
        no_jwt.trae_jwt_refresh_enabled = false;
        assert_eq!(
            startup_tasks(&no_jwt),
            vec![
                ScheduleTask::Checkin,
                ScheduleTask::Travel,
                ScheduleTask::Keepalive,
                ScheduleTask::QoderKeepalive,
            ],
            "关掉 Trae JWT 续期后它不得再补跑（逐类 gating）"
        );

        let mut all_off = cfg.clone();
        all_off.checkin_enabled = false;
        all_off.travel_enabled = false;
        all_off.keepalive_enabled = false;
        all_off.trae_jwt_refresh_enabled = false;
        all_off.qoder_keepalive_enabled = false;
        assert!(
            startup_tasks(&all_off).is_empty(),
            "全关时启动补跑必须为空（否则开关形同虚设）"
        );

        let mut travel_off = cfg.clone();
        travel_off.travel_enabled = false;
        assert_eq!(
            startup_tasks(&travel_off),
            vec![
                ScheduleTask::Checkin,
                ScheduleTask::TraeJwtRefresh,
                ScheduleTask::Keepalive,
                ScheduleTask::QoderKeepalive,
            ],
            "gating 必须是逐类的：关 travel 不影响另外几类"
        );

        // 逐类 gating 的第二个探针：一条线关闭时**不能**把另一条线一起带走。
        let mut trae_on_keepalive_off = cfg.clone();
        trae_on_keepalive_off.trae_checkin_enabled = true;
        trae_on_keepalive_off.keepalive_enabled = false;
        assert_eq!(
            startup_tasks(&trae_on_keepalive_off),
            vec![
                ScheduleTask::Checkin,
                ScheduleTask::TraeCheckin,
                ScheduleTask::TraeJwtRefresh,
                ScheduleTask::Travel,
                ScheduleTask::QoderKeepalive,
            ],
            "关 keepalive 不能带走 Trae 签到 / Trae JWT 续期"
        );

        // 活跃上报 / 开学季 / 夜猫子**不在**启动补跑里（交给排程小时表）。
        let only_others = schedule::ScheduleConfig {
            checkin_enabled: false,
            travel_enabled: false,
            keepalive_enabled: false,
            trae_jwt_refresh_enabled: false,
            qoder_keepalive_enabled: false,
            ..cfg.clone()
        };
        assert!(startup_tasks(&only_others).is_empty());
    }

    /// 启动补跑的集合必须**恰好**是这七类；新增进来必须有意为之。
    ///
    /// Trae 签到是 2026-09-22 有意加进来的第四类，Trae JWT 续期是 2026-10-14 有意加进来的
    /// 第五类，Qoder 签到 / 保活是 2026-10-19 有意加进来的第六、七类（理由见
    /// [`STARTUP_TASKS`] 的文档），都不是顺手带上的 ——
    /// 这条断言的作用就是让下次有人想加第八类时被迫说明理由。
    #[test]
    fn startup_task_set_is_exactly_the_intended_families() {
        let labels: Vec<&str> = STARTUP_TASKS.iter().map(|t| t.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "checkin",
                "trae_checkin",
                "trae_jwt_refresh",
                "travel",
                "keepalive",
                "qoder_checkin",
                "qoder_keepalive"
            ]
        );
    }
}
