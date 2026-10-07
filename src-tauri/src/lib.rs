// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod gateway;
mod trae_gateway;
#[cfg(desktop)]
mod companion;
#[cfg(desktop)]
mod tray;

use std::time::Duration;
use tauri::Manager;
use buddy_switch_core::modules;
use buddy_switch_core::modules::background::{TaskKind, TaskSpec, TASKS};
use buddy_switch_core::modules::product::Product;

const SCREENSHOT_DEMO_ENV: &str = "BUDDY_SWITCH_SCREENSHOT_DEMO";

/// WorkBuddy 账号池状态落盘周期（毫秒）。与 `buddy-switch-server` 同值。
///
/// 取值理由：`Pool::flush_if_dirty` 只在**有改动**时才真正写盘，所以这里定的是
/// 「最坏情况下最多丢多少治理状态」。30 秒足够短（冷却 / 熔断的时效是分钟级，
/// 丢 30 秒不会造成错误决策），又远长于一次请求。
const POOL_PERSIST_INTERVAL_MS: u64 = 30_000;

/// Trae 积分刷新周期（毫秒）。与 WorkBuddy 余额刷新的缺省间隔（30 分钟）一致：
/// 同一管理面不该有两种节奏。
const TRAE_CREDITS_REFRESH_INTERVAL_MS: u64 = 30 * 60 * 1000;

/// 定时备份的**检查**周期（毫秒）。
///
/// 取 1 小时：`backup::auto_backup()` 当日已备份时**幂等直接返回**（不重复读全量），
/// 故检查本身开销极低；小时级足以在跨日后尽快产生当日备份。
const AUTO_BACKUP_CHECK_INTERVAL_MS: u64 = 60 * 60 * 1000;

pub(crate) fn is_screenshot_demo() -> bool {
    std::env::var(SCREENSHOT_DEMO_ENV).as_deref() == Ok("1")
}

/// 启动全部后台任务（以 core 的 [`TASKS`] 为**唯一事实来源**）。
///
/// ⚠️ 回归背景（本层存在的原因）：此前 `spawn_background_loops` 直接写死四个循环
/// （签到 30 分钟 / 旅行 30 分钟 / 领取 15 分钟 / 保活每天一次），**完全没有排程**，
/// 于是设置页「定时任务排程」的六类开关与六份小时表在桌面端全部空转——「活跃地图」
/// 从未执行过一次。
///
/// 改造后「有哪些后台任务」是**数据**（core [`TASKS`]，两宿主共用），本函数只负责
/// **按表启动**；「怎么启动」按宿主运行时决定（Tauri 用 `tauri::async_runtime::spawn`）。
fn spawn_background_loops() {
    for spec in TASKS {
        spawn_background_task(*spec);
    }
}

/// 按注册表条目派生对应的后台循环。
///
/// `match spec.kind` **穷尽**（无通配分支）：core 新增一类 [`TaskKind`] 会让本函数
/// **编译失败**，从而强制本宿主补上派发分支（登记即执行 / P5 R9 的**编译期**护栏）。
/// **运行期**护栏见 core `tests::both_hosts_register_every_task_kind`（直接读本文件源码）。
///
/// ⚠️ 产品线维度：`CreditsRefresh` 被**两个产品共用**（二维化的本意），故在该分支内再按
/// [`TaskSpec::product`] 分派 —— 两者实现**完全不同**（WorkBuddy 走 `gateway::shared_state`
/// 的池，Trae 走 `trae::credits` 的区域账号库）。
fn spawn_background_task(spec: TaskSpec) {
    match spec.kind {
        // 启动补跑：排程小时表之外的「今天该做但还没做」的一次性动作。
        // 语义与服务端 `buddy-switch-server` 完全一致（共用 core::scheduler）。
        TaskKind::StartupMaintenance => {
            tauri::async_runtime::spawn(async move {
                modules::scheduler::run_startup_maintenance().await;
            });
        }
        // 自动轮换（CodeBuddy CLI）：按配置间隔执行。
        TaskKind::AutoRotate => {
            tauri::async_runtime::spawn(async move {
                let mut last_rotate_at: i64 = 0;
                loop {
                    let rotate_cfg = modules::config::load_auto_rotate_config();
                    if rotate_cfg.get("enabled").and_then(|v| v.as_bool()) == Some(true) {
                        let interval_minutes = rotate_cfg
                            .get("check_interval_minutes")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(5)
                            .max(1);
                        let now = modules::config::now_ms();
                        if now - last_rotate_at >= interval_minutes * 60_000 {
                            last_rotate_at = now;
                            let _ = modules::rotate::run_rotate_cycle().await;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            });
        }
        // 账号池余额 / 积分刷新：**独立**循环，首次启动先跑一次。按**产品线**分派实现。
        TaskKind::CreditsRefresh => match spec.product {
            // WorkBuddy 账号池余额刷新（真实读数回填选号权重）。
            //
            // 为什么必须是独立后台循环：余额是慢变数据，在请求路径上同步拉余额会直接拉高
            // 每次请求的延迟（见 `buddy_switch_gateway::credits_refresh` 的文档）。
            Product::Workbuddy => {
                tauri::async_runtime::spawn(async move {
                    let state = gateway::shared_state();
                    loop {
                        // 启动即刷一次（循环首轮）：补齐上次进程遗留的「从未取过余额」账号。
                        let _ =
                            buddy_switch_gateway::credits_refresh::refresh_once(&state).await;
                        // ★ 与 server 侧**同一判据**（`next_refresh_wait_ms`）：
                        //   池为空 ⇒ 短探测，否则「刚导入账号」要等满一整个周期才看得到积分。
                        let wait_ms = {
                            let pool = state.pool.read().await;
                            buddy_switch_gateway::credits_refresh::next_refresh_wait_ms(&pool)
                        };
                        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                    }
                });
            }
            // Trae 积分刷新（两条国内产品线，顺带自动解冻已恢复的账号）。
            Product::Trae => {
                tauri::async_runtime::spawn(async move {
                    // 启动即刷一次：补齐上次进程遗留的陈旧读数。
                    refresh_trae_credits().await;
                    loop {
                        tokio::time::sleep(Duration::from_millis(TRAE_CREDITS_REFRESH_INTERVAL_MS))
                            .await;
                        refresh_trae_credits().await;
                    }
                });
            }
        },
        // WorkBuddy 账号池治理状态落盘（只在有改动时真正写盘）。
        TaskKind::PoolPersist => {
            tauri::async_runtime::spawn(async move {
                let state = gateway::shared_state();
                loop {
                    tokio::time::sleep(Duration::from_millis(POOL_PERSIST_INTERVAL_MS)).await;
                    state.persist_pool().await;
                }
            });
        }
        // 定时全局备份（D17）：**默认开**，但可在设置里关掉（§7 #7）——关闭后不再产生
        // 新备份（已产生的保留）。`auto_backup()` 当日已备份则幂等返回，检查开销极低。
        TaskKind::AutoBackup => {
            tauri::async_runtime::spawn(async move {
                loop {
                    if modules::config::is_auto_backup_enabled() {
                        let _ = modules::backup::auto_backup();
                    }
                    tokio::time::sleep(Duration::from_millis(AUTO_BACKUP_CHECK_INTERVAL_MS)).await;
                }
            });
        }
        // ★ 网关启动补识别（P10 §1.6 / ✅ #7）：**一次性**——启动时把「内置 / 陈旧」的清单
        // 补成实时，之后交给「接入即识别」与手动「重新识别」。失败静默（探测不得让启动失败）。
        TaskKind::GatewayCatalogProbe => {
            tauri::async_runtime::spawn(async move {
                let state = gateway::shared_state();
                let _ = buddy_switch_gateway::catalog_probe::probe_stale_regions(&state).await;
            });
        }
        // 六类定时任务各自独立排程：每类一个循环，按自己的小时表 sleep 到点。
        // 排程语义在 core::scheduler 里，桌面端与服务端共用，**不在此处另写一份**。
        TaskKind::Scheduled(task) => {
            tauri::async_runtime::spawn(modules::scheduler::schedule_loop(task));
        }
    }
}

/// 刷新 Trae **两条国内产品线**的全部账号积分（顺带自动解冻已恢复的账号）。
///
/// 只刷 `TraeVariant::all()`（= `TraeWork` / `Trae`）：网关的 Key 只归属这两条线
/// （见 `buddy_switch_gateway::trae::apikey` 的 `variant`），刷国际版属于无效功。
///
/// 失败不中断：`refresh_all_remaining_for` 内部已对**单账号**失败做了记录与跳过，
/// 这里再兜一层是为了「一个变体整体报错不影响另一个变体」。
async fn refresh_trae_credits() {
    for variant in modules::trae::variant::TraeVariant::all() {
        let _ = modules::trae::credits::refresh_all_remaining_for(variant).await;
    }
}

/// 应用级窗口事件转发：主窗口（托盘）与悬浮窗（companion）各按自己的 label 短路。
///
/// 两个处理器都注册在**同一条** `on_window_event` 上 —— Tauri 的 Builder 只接受一个
/// 全局窗口事件回调，因此这里做一次分发，而不是让其中一方去「顺带」处理另一方的事件。
#[cfg(desktop)]
fn on_window_event<R: tauri::Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    tray::on_window_event(window, event);
    companion::on_window_event(window, event);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init());

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![tray::SILENT_STARTUP_ARG]),
        ));
        builder = builder.on_window_event(on_window_event);
    }

    let app = builder
        .setup(|app| {
            #[cfg(desktop)]
            {
                tray::setup(app)?;
                // 主窗口由配置创建为不可见；在事件循环呈现前决定本次启动是否静默。
                // 仅系统自启（精确 `--hidden` 参数）进入静默托盘，普通启动立即显示主窗口。
                tray::setup_startup_visibility(
                    app.handle(),
                    tray::is_silent_startup(std::env::args()),
                );
            }
            // 网关运行时句柄（管理命令依赖它查询/切换独立监听）。
            app.manage(gateway::GatewayRuntime::new());
            // Trae 网关运行时句柄（与上面那份**互不相干**：配置 / Key / 日志全独立）。
            app.manage(trae_gateway::TraeGatewayRuntime::new());
            // README 截图模式只渲染前端虚构数据，禁止读取账号后执行签到、轮换或保活。
            if !is_screenshot_demo() {
                spawn_background_loops();
                // 按配置启动 API 网关独立监听（默认关闭；默认 127.0.0.1:57891）。
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = handle.state::<gateway::GatewayRuntime>();
                    match runtime.apply().await {
                        Ok(Some(addr)) => eprintln!("[gateway] 已启动: http://{addr}"),
                        Ok(None) => {}
                        Err(error) => eprintln!("[gateway] 启动失败: {error}"),
                    }
                });
                // 按配置启动 Trae 网关独立监听（默认关闭；默认 127.0.0.1:7864）。
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = handle.state::<trae_gateway::TraeGatewayRuntime>();
                    match runtime.apply().await {
                        Ok(Some(addr)) => eprintln!("[trae-gateway] 已启动: http://{addr}"),
                        Ok(None) => {}
                        Err(error) => eprintln!("[trae-gateway] 启动失败: {error}"),
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_accounts,
            // P5 T03 · 参数化命令（`scope` 决定平台与区域；旧命令保留为别名）。
            commands::accounts_list,
            commands::account_remark,
            commands::account_delete,
            commands::accounts_export,
            commands::accounts_export_to_path,
            commands::accounts_import,
            commands::accounts_import_local,
            commands::credits,
            commands::checkin_status,
            commands::open_data_dir,
            // P5 §2.7 · 回收站 4 命令（**全新命令**，非参数化、无别名；带 scope 且 fail-closed）。
            commands::account_trash_list,
            commands::account_restore,
            commands::account_purge,
            commands::account_trash_clear,
            commands::get_codebuddy_cli_status,
            commands::install_codebuddy_cli_helper,
            commands::switch_codebuddy_cli_account,
            commands::get_codebuddy_cn_ide_status,
            commands::switch_codebuddy_cn_ide_account,
            commands::detect_codebuddy_cn_ide_account,
            commands::delete_account,
            commands::oauth_start,
            commands::oauth_status,
            commands::import_local,
            commands::export_accounts,
            commands::export_accounts_to_path,
            commands::preview_import_accounts,
            commands::import_accounts,
            // P7 T01 · 全局备份导出（跨 4 作用域；凭据按 CREDENTIAL_KEYS 剥离）。
            commands::export_backup,
            // P7 T02 · 全局备份导入（预览 / 导入 / 自动备份 / 恢复 / 配置）。
            commands::preview_import_backup,
            commands::import_backup,
            commands::list_auto_backups,
            commands::restore_auto_backup,
            commands::get_auto_backup_config,
            commands::save_auto_backup_config,
            commands::switch_account,
            commands::switch_progress,
            commands::list_sessions,
            // P8 T02 · 只读：copy ledger 连通成「会话副本组」（两宿主同步）。
            commands::list_session_replicas,
            commands::session_fingerprints,
            commands::sync_session_replicas,
            commands::copy_sessions,
            commands::migrate_account_data,
            commands::set_account_remark,
            commands::get_switch_config,
            commands::save_switch_config,
            commands::open_permission_settings,
            commands::check_auth_permission,
            commands::reveal_app_in_finder,
            commands::open_accounts_dir,
            commands::get_checkin_status,
            commands::get_credit_expiry,
            commands::get_credit_statistics,
            commands::get_token_statistics,
            commands::checkin,
            commands::checkin_all,
            commands::get_auto_checkin_config,
            commands::save_auto_checkin_config,
            commands::get_checkin_logs,
            commands::get_travel_status,
            commands::get_auto_travel_config,
            commands::save_auto_travel_config,
            commands::get_schedule_config,
            commands::save_schedule_config,
            commands::run_schedule_task,
            commands::refresh_account_token,
            commands::get_auto_rotate_config,
            commands::save_auto_rotate_config,
            commands::rotate_status,
            commands::run_rotate,
            commands::get_rotate_logs,
            commands::get_github_config,
            commands::save_github_config,
            commands::check_update,
            commands::relaunch_app,
            commands::get_launch_at_login_enabled,
            commands::set_launch_at_login_enabled,
            commands::get_gateway_config,
            commands::save_gateway_config,
            commands::proxy_status,
            commands::proxy_enable,
            commands::proxy_disable,
            commands::gateway_status,
            commands::open_workbuddy_data_dir,
            commands::list_api_keys,
            commands::create_api_key,
            commands::revoke_api_key,
            commands::delete_api_key,
            commands::get_gateway_models,
            commands::refresh_gateway_models,
            commands::get_account_strategy,
            commands::save_account_strategy,
            commands::get_disabled_models,
            commands::set_model_disabled,
            commands::get_gateway_logs,
            commands::clear_gateway_logs,
            // ---- Trae 模块（与 server 的 /api/trae/* 路由一一对应）----
            commands::get_trae_env,
            commands::get_trae_variants,
            commands::get_trae_capabilities,
            commands::get_trae_accounts,
            commands::get_trae_checkin_status,
            commands::get_trae_credits,
            commands::get_trae_token_statistics,
            commands::get_trae_logs,
            commands::get_trae_profiles,
            commands::get_trae_settings,
            commands::save_trae_settings,
            commands::trae_add_account,
            commands::trae_update_account,
            commands::trae_set_account_remark,
            commands::trae_delete_account,
            commands::trae_import_local_account,
            commands::trae_oauth_start,
            commands::trae_oauth_status,
            commands::trae_oauth_cancel,
            commands::trae_export_accounts,
            commands::trae_export_accounts_to_path,
            commands::trae_preview_import_accounts,
            commands::trae_import_accounts,
            commands::trae_group_op,
            commands::trae_checkin,
            commands::trae_refresh_credits,
            commands::trae_refresh_jwt,
            commands::trae_clear_cooldown,
            commands::trae_switch_account,
            commands::trae_merge_legacy_regions,
            commands::trae_save_login,
            commands::trae_backup_profile,
            commands::trae_restore_profile,
            commands::trae_delete_profile,
            commands::trae_reset_device,
            // Trae API 网关（管理面）——与上面 WorkBuddy 网关的一组命令平行。
            commands::get_trae_gateway_config,
            commands::save_trae_gateway_config,
            commands::trae_gateway_status,
            commands::get_trae_gateway_models,
            commands::get_trae_client_models,
            // 多 Key 管理（含归属产品线）+ 打开数据目录（替代旧的单 Key regenerate）。
            commands::list_trae_api_keys,
            commands::create_trae_api_key,
            commands::revoke_trae_api_key,
            commands::delete_trae_api_key,
            commands::open_trae_data_dir,
            commands::trae_launch_client,
            commands::get_trae_gateway_logs,
            commands::clear_trae_gateway_logs,
            // P9 T02 · 桌面悬浮窗**只读数据层**（命令面只有读 / 显隐 / 聚焦，无写操作）。
            commands::companion_status,
            commands::companion_set_visible,
            commands::companion_toggle,
            commands::companion_jump,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| {
        #[cfg(desktop)]
        tray::on_run_event(event);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_switch_core::modules::background::has_task;

    /// 护栏：core [`TASKS`] 必须登记**全部**定时任务 + 关键后台任务（「登记即执行」）。
    ///
    /// 回归背景：排程循环此前只存在于 `buddy-switch-server` 二进制，桌面端只有四个写死
    /// 周期的循环，于是设置页「定时任务排程」的全部控件在桌面端空转——最典型的是
    /// 「活跃地图」从未执行，用户在官网对照连登热力图发现始终没点亮。
    ///
    /// 表本体已移到 core（两宿主共用），本用例保证**桌面端依赖的那份表**不丢任务。
    /// 桌面端 `spawn` 派发是否覆盖每个 [`TaskKind`]，由 core
    /// `tests::both_hosts_register_every_task_kind`（直接读本文件源码）守住。
    ///
    /// 可证伪性：从 core [`TASKS`] 移除任一 `Scheduled(...)`（或关键任务）会让本用例变红。
    #[test]
    fn background_tasks_cover_all_scheduled_tasks() {
        for task in modules::schedule::ScheduleTask::all() {
            assert!(
                TASKS.iter().any(|spec| spec.kind == TaskKind::Scheduled(task)),
                "后台任务表缺少定时任务「{}」——它在此进程里永远不会执行",
                task.as_str()
            );
        }
        assert!(
            has_task(Product::Workbuddy, TaskKind::StartupMaintenance),
            "注册表缺少启动补跑任务"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::AutoRotate),
            "注册表缺少自动轮换任务"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::CreditsRefresh),
            "注册表缺少账号池余额刷新 —— 缺了它池里的 credits 恒为 0，\
             「四因子加权」会静默退化为两因子（服务端早已有该任务，桌面端此前漏了）"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::PoolPersist),
            "注册表缺少账号池落盘 —— 缺了它冷却 / 熔断 / 成功率 EMA 只在内存里，重启即清零"
        );
        assert!(
            has_task(Product::Trae, TaskKind::CreditsRefresh),
            "注册表缺少 Trae 积分刷新 —— 缺了它纯用 API（不开界面）时积分读数永远陈旧：\
             耗尽积分的账号仍被选中、尚有余额的账号被误判为零积分"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::AutoBackup),
            "注册表缺少定时全局备份 —— 缺了它生产上永不产生自动备份（P5 R9「登记即执行」）"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::GatewayCatalogProbe),
            "注册表缺少网关启动补识别（P10 T03 / ✅ #7）—— 缺了它「内置 / 陈旧」的模型清单\
             永远不会在启动时被补成实时，用户只能看到「内置」来源条"
        );
    }
}
