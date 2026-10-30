// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod gateway;
// Qoder 网关（第三个平台，默认 57895）：独立端口，不与另三个 merge（axum 同路径 merge 会 panic）。
mod qoder_gateway;
mod trae_gateway;
// 统一网关（第三个产品线，默认 57892）：跨产品线的单一入口 —— 独立端口，不与另三个 merge。
// ⚠️ 端口分工上是**第四个监听**（WB 57891 / Trae 57894 / Qoder 57895 / 统一 57892），
//    因此 setup 里必须有**四段** apply spawn —— 漏一段只在「保存配置」时才起监听，
//    重启后静默断服（issue #52）。
#[cfg(desktop)]
mod companion;
// Qoder 网页端登录窗 + 会话 Cookie 收割（issue #111）。
// ⚠️ **不加 `#[cfg(desktop)]`**：命令名写进了下面无条件编译的 `generate_handler!`，
//    加了 cfg 会让非桌面目标的构建直接找不到模块。真正的「仅桌面端可用」判定在
//    **能力层**（headless 宿主走 server 路由，返回结构化拒绝），不靠这里裁剪。
mod qoder_web;
#[cfg(desktop)]
mod tray;
mod unified_gateway;

use buddy_switch_core::modules;
use buddy_switch_core::modules::background::{TaskKind, TaskSpec, TASKS};
use buddy_switch_core::modules::product::Product;
use buddy_switch_core::modules::runtime_log;
use std::time::Duration;
use tauri::Manager;

/// 宿主诊断输出的**唯一出口**：写运行日志，并在 **debug** 构建同时打 stderr。
///
/// ## 为什么不能直接 `eprintln!`
///
/// 本宿主是 `windows_subsystem = "windows"` 的 release GUI（见 `main.rs`）：**没有控制台**，
/// `eprintln!` 在用户机器上写进虚无。于是「网关起不来」「切号失败」这类问题只有一句界面
/// toast，用户拿不到任何可粘贴给维护者的东西。改走运行日志后，设置页「日志」分区直接可读。
///
/// debug 构建仍打 stderr：`tauri dev` 下开发者在终端就能看到，不必去翻文件。
fn host_log(level: runtime_log::Level, scope: &str, message: impl AsRef<str>) {
    let message = message.as_ref();
    runtime_log::log(level, scope, message);
    #[cfg(debug_assertions)]
    eprintln!("[{scope}] {message}");
}

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

/// 网关用量「今日」快照的记录周期（毫秒）—— 与 core
/// [`buddy_switch_core::modules::background::GATEWAY_USAGE_RECORD_INTERVAL`] 同值。
///
/// 取 2 分钟（对齐 `token_daily::CLIENT_SCAN_TTL_MS`）：账号卡片的「今日 Token」
/// gateway 桶是仪表盘读数，不需要实时，但绝不能长时间不动；而每轮只读一份受
/// `log_keep` 封顶的日志 + 账号库，代价极低。
const GATEWAY_USAGE_RECORD_INTERVAL_MS: u64 = 2 * 60 * 1000;

/// 客户端（登录态）用量「今日」快照的记录周期（毫秒）—— 与 core
/// [`buddy_switch_core::modules::background::CLIENT_USAGE_RECORD_INTERVAL`] 同值。
///
/// 取 2 分钟（对齐 `token_daily::CLIENT_SCAN_TTL_MS`）：卡片「今日 Token」的 client 列
/// 是仪表盘读数，不需要实时，但绝不能长时间不动；扫描本身受 mtime 预筛 + 尾部窗口
/// + TTL 闸门三重代价控制（稳态 ≈ 0.7 s）。
const CLIENT_USAGE_RECORD_INTERVAL_MS: u64 = 2 * 60 * 1000;

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
                        let _ = buddy_switch_gateway::credits_refresh::refresh_once(&state).await;
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
            /* Qoder：**尚未有余额刷新循环**（额度要调上游，属网关层，未落地）。
            ⚠️ 与 server 侧同款：core 的 `TASKS` 表里没有任何 Qoder 任务 ⇒ 本分支当前
            不可达；一旦登记了，这行警告会出现在日志里，而不是悄悄空转。 */
            Product::Qoder => {
                host_log(
                    runtime_log::Level::Warn,
                    "background",
                    "Qoder 的 CreditsRefresh 已登记但**尚无实现** —— \
                     请补刷新循环，或从 core 的 TASKS 表里移除该条目。",
                );
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
        // ★ 网关用量「今日」快照（2026-10-15 新增，缺陷修复，与服务端同语义）：周期把
        // 网关日志聚合进 `token_daily` 的 gateway 桶并落盘。
        //
        // 为什么必须有它：`record_gateway_today` 此前唯一的调用点是「打开账号页」的
        // 命令（`get_account_token_today`）⇒ ① 不进账号页，gateway 桶一直是旧值；
        // ② `token_daily` 保留 90 天，但历史只覆盖「当天 App 被打开过」的日子
        //    （实测大片空洞）⇒ 网关用量永远成不了趋势。
        //
        // 覆盖**两个 region**：`gateway_logs.json` 是单文件、行内带 `region`，
        // 只记一个 region 会让另一个的历史永远空着。
        TaskKind::GatewayUsageRecord => match spec.product {
            // WorkBuddy：`token_daily` 的 gateway 桶（按 region 分文件）。
            //
            // 覆盖**两个 region**：`gateway_logs.json` 是单文件、行内带 `region`，
            // 只记一个 region 会让另一个的历史永远空着。
            Product::Workbuddy => {
                tauri::async_runtime::spawn(async move {
                    loop {
                        for region in modules::region::Region::all() {
                            let _ = modules::token_daily::record_gateway_today(region);
                        }
                        tokio::time::sleep(Duration::from_millis(GATEWAY_USAGE_RECORD_INTERVAL_MS))
                            .await;
                    }
                });
            }
            // Trae / Qoder：`gateway_ledger`（按 scope 分文件，含 unlabeled 一档）。
            //
            // ⚠️ 必须一次记**全部** scope：日志是单文件、行内带归属（Trae 看 `variant`、
            // Qoder 看 `region`），只记当前作用域会让切区域后的账本整段是空的。
            Product::Trae => {
                tauri::async_runtime::spawn(async move {
                    loop {
                        modules::gateway_ledger::record_all(
                            modules::gateway_ledger::LedgerProduct::Trae,
                        );
                        tokio::time::sleep(Duration::from_millis(GATEWAY_USAGE_RECORD_INTERVAL_MS))
                            .await;
                    }
                });
            }
            Product::Qoder => {
                tauri::async_runtime::spawn(async move {
                    loop {
                        modules::gateway_ledger::record_all(
                            modules::gateway_ledger::LedgerProduct::Qoder,
                        );
                        tokio::time::sleep(Duration::from_millis(GATEWAY_USAGE_RECORD_INTERVAL_MS))
                            .await;
                    }
                });
            }
        },
        // ★ 客户端（登录态）用量「今日」快照（2026-10-15 用户报障修复，与服务端同语义）：
        // 周期把客户端会话日志的**今日**用量聚合进 `token_daily` 的 client 桶并落盘。
        //
        // 为什么必须有它：`record_client_today` 此前唯一的调用点是「打开账号页」的命令
        // （`get_account_token_today`）⇒ 不进账号页，卡片上的 client 列一直是旧值
        // （用户报障：「client 数字不动」），而同卡片的 gateway 桶已被上一条周期刷新
        // ⇒ 两个桶一新一旧，用户看到自相矛盾的数字。
        //
        // `force = false`：走 `CLIENT_SCAN_TTL_MS` 闸门 —— 本循环周期与闸门同值，
        // 挡掉的只是「刚被账号页扫过」的重复轮次，不挡新鲜度。
        //
        // 覆盖**两个 region**：客户端语料按区域分目录（CN = `.workbuddy/projects`，
        // Global = `.workbuddy-ai/projects`），只扫一个另一个永远是旧值。
        //
        // ⚠️ 与网关那条**刻意分成两个循环**：失败面不同（读盘扫 jsonl vs 读一份日志），
        // 合并后客户端扫描 panic 会把网关记录一起带停。
        TaskKind::ClientUsageRecord => {
            tauri::async_runtime::spawn(async move {
                loop {
                    for region in modules::region::Region::all() {
                        let _ = modules::token_daily::record_client_today(region, false);
                    }
                    tokio::time::sleep(Duration::from_millis(CLIENT_USAGE_RECORD_INTERVAL_MS))
                        .await;
                }
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

/// Linux / WebKitGTK 渲染兜底（issue #80）。
///
/// ## 为什么需要它
///
/// WebKitGTK 在 **Wayland** 会话下走 DMA-BUF + 加速合成。实测在某些驱动组合
/// （NVIDIA 专有驱动、分数缩放、部分 Mesa 版本）上表现为：**已有图层照常绘制，
/// 新创建的合成层画不出来**。对应到本工具就是「点『创建密钥』→ 弹窗确实进了 DOM、
/// `body` 也被 Radix 锁成 `pointer-events:none`，但屏幕上什么都没多出来，整窗点不动」
/// —— 与 issue #80 的复现描述逐字吻合（Wayland / 屏幕没有对话框 / 点了没有任何变化 /
/// 日志干净）。这也是为什么它跟「慢 IO 占住 IPC 线程」无关：那条路会留下 `[ipc-slow]`
/// 记录，而这里一次 IPC 都没发生。
///
/// ## 为什么默认开
///
/// 关掉 DMA-BUF 渲染器的代价是合成路径变慢（本工具以表单为主，无感）；
/// 收益是弹窗 / 浮层**一定能画出来**。画不出来是致命的，帧率不是。
///
/// ## 覆盖方式
///
/// - 自己设了 `WEBKIT_DISABLE_DMABUF_RENDERER` ⇒ 完全交给用户，本函数不动；
/// - `BUDDY_SWITCH_KEEP_DMABUF=1` ⇒ 保留 DMA-BUF（想对比/排障时用）。
///
/// 只在 Wayland 下设，X11 没有这个问题，不要无谓地拖慢它。
#[cfg(target_os = "linux")]
fn apply_linux_render_workarounds() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }
    if std::env::var("BUDDY_SWITCH_KEEP_DMABUF")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return;
    }
    let wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if wayland {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

/// 读一个环境变量用于启动日志：未设置写 `unset`。
///
/// **不折叠空串**（`GDK_BACKEND=` 是「显式设成空」）—— 排障时要能区分
/// 「用户没设」与「用户设了但值是空 / 被别的启动方式覆盖成空」。
#[cfg(target_os = "linux")]
fn linux_env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "unset".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ★ 必须在 `tauri::Builder` 起来之前 —— WebKitGTK 在进程启动早期读这些环境变量。
    #[cfg(target_os = "linux")]
    apply_linux_render_workarounds();

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
            // ★ 运行日志：装 panic hook + 写启动横幅（版本 / 平台 / 数据目录 / 级别）。
            //   放在最前面 —— 它要成为日志文件里的**第一条**记录，之后所有诊断都排在它后面；
            //   晚一步调用就会丢掉「tray 初始化阶段」可能出现的错误。
            runtime_log::init("Buddy Switch 桌面端");
            // ★ 图形栈诊断（issue #80）：报告「点了没反应」时，第一件要确认的事是
            //   「会话类型 + WebKit 合成路径」，而此前日志里一个字都没有 ——
            //   用户贴出来的日志总是「很干净」，排查只能靠猜。
            #[cfg(target_os = "linux")]
            {
                // ★ 这一段是**唯一**能自证「App 实际看到的环境」的地方（issue #80 第二轮）。
                //   用户在 shell 里 `export GDK_BACKEND=x11` / `WEBKIT_DISABLE_*`，与进程真正
                //   继承到的值**可能对不上**（.desktop 启动、systemd user unit、AppImage 包装
                //   脚本各有一套环境）⇒ 报告里写「已经设了」，日志却说不清设没设到进程里。
                //   所以：只记**进程真实读到**的值，缺失一律写 `unset`（与空串区分）。
                runtime_log::info(
                    "startup",
                    &format!(
                        "图形栈 · session={} · WAYLAND_DISPLAY={} · desktop={} · gdk_backend={} · \
                         WEBKIT_DISABLE_DMABUF_RENDERER={} · WEBKIT_DISABLE_COMPOSITING_MODE={} · \
                         GDK_SCALE={} · GDK_DPI_SCALE={}",
                        linux_env("XDG_SESSION_TYPE"),
                        linux_env("WAYLAND_DISPLAY"),
                        linux_env("XDG_CURRENT_DESKTOP"),
                        linux_env("GDK_BACKEND"),
                        linux_env("WEBKIT_DISABLE_DMABUF_RENDERER"),
                        linux_env("WEBKIT_DISABLE_COMPOSITING_MODE"),
                        linux_env("GDK_SCALE"),
                        linux_env("GDK_DPI_SCALE"),
                    ),
                );
            }
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
            // Qoder 网关运行时句柄（同样独立；**一个监听服务两个区域**）。
            app.manage(qoder_gateway::QoderGatewayRuntime::new());
            app.manage(unified_gateway::UnifiedGatewayRuntime::new());
            // README 截图模式只渲染前端虚构数据，禁止读取账号后执行签到、轮换或保活。
            if !is_screenshot_demo() {
                spawn_background_loops();
                // P15 · 外部 Agent hook 的本地接收端：只绑 `127.0.0.1`。
                //
                // ★ **它不是后台任务**：`serve()` 是事件驱动的常驻 accept，没有周期动作
                //   ⇒ 不进 `background.rs` 的 `TASKS` 注册表（`P15-SPIKE-RESULTS §2 Q2`）。
                // ★ 绑定失败（端口被占 / 沙箱限制）**不阻断启动**：这是可选能力，
                //   宿主照常可用；用户从哪里看得到？——设置页的监听开关会显示连接不上。
                tauri::async_runtime::spawn(async {
                    if let Err(error) = buddy_switch_core::modules::agent_hook_rx::serve().await {
                        host_log(runtime_log::Level::Error, "agent-hook", error.to_string());
                    }
                });
                // 按配置启动 API 网关独立监听（默认关闭；默认 127.0.0.1:57891）。
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = handle.state::<gateway::GatewayRuntime>();
                    match runtime.apply().await {
                        Ok(Some(addr)) => host_log(
                            runtime_log::Level::Info,
                            "gateway",
                            format!("已启动: http://{addr}"),
                        ),
                        Ok(None) => {}
                        Err(error) => host_log(
                            runtime_log::Level::Error,
                            "gateway",
                            format!("启动失败: {error}"),
                        ),
                    }
                });
                // 按配置启动 Trae 网关独立监听（默认关闭；默认 127.0.0.1:57894）。
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = handle.state::<trae_gateway::TraeGatewayRuntime>();
                    match runtime.apply().await {
                        Ok(Some(addr)) => host_log(
                            runtime_log::Level::Info,
                            "trae-gateway",
                            format!("已启动: http://{addr}"),
                        ),
                        Ok(None) => {}
                        Err(error) => host_log(
                            runtime_log::Level::Error,
                            "trae-gateway",
                            format!("启动失败: {error}"),
                        ),
                    }
                });
                // 按配置启动 Qoder 网关独立监听（默认关闭；默认 127.0.0.1:57895）。
                // ⚠️ 一个监听、两个区域（区域由请求所用 API Key 的归属决定）。
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = handle.state::<qoder_gateway::QoderGatewayRuntime>();
                    match runtime.apply().await {
                        Ok(Some(addr)) => host_log(
                            runtime_log::Level::Info,
                            "qoder-gateway",
                            format!("已启动: http://{addr}"),
                        ),
                        Ok(None) => {}
                        Err(error) => host_log(
                            runtime_log::Level::Error,
                            "qoder-gateway",
                            format!("启动失败: {error}"),
                        ),
                    }
                });
                // 按配置启动**统一网关**独立监听（默认关闭；默认 127.0.0.1:57892）。
                // ⚠️ 跨产品线单一入口：第四个监听，不与另三个 merge（同路径 merge 会 panic）。
                // ★ 少了这一段，`gateway_unified.json` 里的 `enabled: true` 只在
                //   `save_unified_gateway_config` 那一刻被应用 —— 重启后端口上没有监听，
                //   客户端只会「一直生成中」，网关侧一条日志都没有（issue #52）。
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = handle.state::<unified_gateway::UnifiedGatewayRuntime>();
                    match runtime.apply().await {
                        Ok(Some(addr)) => host_log(
                            runtime_log::Level::Info,
                            "unified-gateway",
                            format!("已启动: http://{addr}"),
                        ),
                        Ok(None) => {}
                        Err(error) => host_log(
                            runtime_log::Level::Error,
                            "unified-gateway",
                            format!("启动失败: {error}"),
                        ),
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
            commands::accounts_paste_credential,
            commands::credits,
            commands::checkin_status,
            commands::open_data_dir,
            // 缓存清理（「清除缓存」）：`cache_scan` 只读量体积，`cache_clear` 按类别删。
            commands::cache_scan,
            commands::cache_clear,
            // P5 §2.7 · 回收站 4 命令（**全新命令**，非参数化、无别名；带 scope 且 fail-closed）。
            commands::account_trash_list,
            commands::account_restore,
            commands::account_purge,
            commands::account_trash_clear,
            commands::get_codebuddy_cli_status,
            commands::install_codebuddy_cli_helper,
            commands::switch_codebuddy_cli_account,
            commands::codebuddy_cli_enable_gateway,
            commands::codebuddy_cli_disable_gateway,
            commands::get_codebuddy_cn_ide_status,
            commands::switch_codebuddy_cn_ide_account,
            commands::detect_codebuddy_cn_ide_account,
            commands::get_vscode_codebuddy_plugin_status,
            commands::switch_vscode_codebuddy_plugin_account,
            commands::detect_vscode_codebuddy_plugin_account,
            commands::get_jetbrains_codebuddy_plugin_status,
            commands::switch_jetbrains_codebuddy_plugin_account,
            commands::detect_jetbrains_codebuddy_plugin_account,
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
            // Q1 · 诊断包导出（只读；凭据闸门在 core 内，命中即拒绝发出）。
            commands::export_diagnostic,
            commands::switch_account,
            commands::switch_progress,
            commands::list_sessions,
            // P8 T02 · 只读：copy ledger 连通成「会话副本组」（两宿主同步；含内容状态 `content`）。
            commands::list_session_replicas,
            commands::sync_session_replicas,
            // P8 · **分歧处理（覆盖式统一）**：目标分歧内容会被丢弃 ⇒ 前端二次确认（两宿主同步）。
            commands::unify_session_replicas,
            commands::copy_sessions,
            // P14 S4a · VS Code 插件 / CodeBuddy IDE 的**会话仓**（只读枚举 + 加法复制；两宿主同步）。
            commands::list_vscode_sessions,
            commands::copy_vscode_sessions,
            // P14 S4b · 记录级 diff（只读）+ 增量追加（**只有 appendable 才真写**）。
            commands::diff_vscode_sessions,
            commands::sync_vscode_sessions,
            // 客户端限流可视化（**只读**观测：账号 × 模型 × 恢复时刻；两宿主同步）。
            commands::client_limits,
            commands::migrate_account_data,
            commands::set_account_remark,
            commands::set_account_group,
            commands::get_switch_config,
            commands::save_switch_config,
            commands::open_permission_settings,
            commands::check_auth_permission,
            commands::reveal_app_in_finder,
            commands::open_accounts_dir,
            // Skill 管理（cc-switch 式）：清单 + 启停 + 跨区搬迁 + 本地导入（两宿主同步）。
            commands::list_skills,
            commands::set_skill_enabled,
            commands::copy_skills_cross,
            commands::import_skill,
            commands::open_skills_dir,
            // 上游客户端 settings.json 统一托管（读 · 编辑 · 备份 · 恢复 · 差异比对，两宿主同步）。
            commands::list_client_settings_slots,
            commands::read_client_settings,
            commands::patch_client_settings,
            commands::list_client_settings_backups,
            commands::restore_client_settings_backup,
            commands::diff_client_settings,
            commands::get_checkin_status,
            commands::get_credit_expiry,
            commands::get_credit_statistics,
            commands::get_token_statistics,
            commands::get_account_token_today,
            // 网关逐日用量趋势（只读 `token_daily` 90 天落库历史；两宿主同步）。
            commands::get_gateway_token_trend,
            commands::get_gateway_usage_dim,
            // Trae / Qoder 的网关账本（用量页「账本」视图，issue #63 的平台补齐）。
            commands::get_trae_gateway_usage_dim,
            commands::get_qoder_gateway_usage_dim,
            // Trae 客户端账本（导入用户自己导出的明文库，不内置解密）。
            commands::probe_trae_client_db,
            commands::unlock_trae_client_db,
            commands::get_trae_client_token_stats,
            commands::get_trae_client_db_config,
            commands::save_trae_client_db_path,
            commands::get_usage_reconcile,
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
            commands::get_app_prefs,
            commands::set_app_prefs,
            // 运行日志（本工具自身，排查问题用）：读取 / 清空 / 打开目录 / 慢命令上报。
            commands::runtime_logs,
            commands::clear_runtime_logs,
            commands::open_runtime_logs_dir,
            // 前端 invoke 计时超阈值时的诊断上报（issue #80：卡死类报障需要「哪个命令慢」）。
            commands::report_slow_call,
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
            commands::restore_api_key,
            commands::delete_api_key,
            // ★ issue #76：密钥策略（用量配额 + 可访问账号）与按密钥用量。
            commands::set_key_policy,
            commands::key_usage,
            commands::revive_gateway_account,
            commands::get_unified_gateway_config,
            commands::save_unified_gateway_config,
            commands::get_unified_gateway_models,
            commands::unified_gateway_pool_overview,
            commands::list_unified_api_keys,
            commands::create_unified_api_key,
            commands::revoke_unified_api_key,
            // ★ issue #76：统一网关密钥策略与按密钥用量。
            commands::set_unified_key_policy,
            commands::unified_key_usage,
            commands::unified_key_usage_all,
            commands::restore_unified_api_key,
            commands::delete_unified_api_key,
            commands::get_gateway_models,
            commands::refresh_gateway_models,
            commands::gateway_pool_exclusions,
            commands::set_trae_gateway_pool_exclusion,
            commands::set_qoder_gateway_pool_exclusion,
            commands::set_gateway_pool_exclusion,
            commands::get_account_strategy,
            commands::save_account_strategy,
            commands::get_disabled_models,
            commands::set_model_disabled,
            // 模型降级链（P13）：读 / 存 / 预演。
            commands::get_model_fallback,
            commands::save_model_fallback,
            commands::preview_model_fallback,
            commands::get_fallback_stats,
            // 图片能力声明 / 覆盖（issue #101）：三条产品线 `/v1/models` 与管理面共用一份存储。
            commands::get_model_image_caps,
            commands::set_model_image_cap,
            commands::get_gateway_logs,
            commands::clear_gateway_logs,
            commands::probe_start,
            commands::probe_status,
            commands::probe_cancel,
            commands::probe_ledger,
            // ---- Trae 模块（与 server 的 /api/trae/* 路由一一对应）----
            commands::get_trae_env,
            commands::get_trae_variants,
            commands::get_trae_capabilities,
            commands::get_trae_accounts,
            commands::get_trae_checkin_status,
            // issue #47：逐账号「今日」只读重取（与 server 的同名路由一一对应）。
            commands::get_trae_checkin_status_map,
            commands::get_trae_account_token_today,
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
            commands::trae_oauth_submit_callback,
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
            // Qoder（第三个平台）—— 6 条命令，全部参数化 `scope`。
            // ⚠️ 与 `crates/buddy-switch-server/src/api.rs` 的路由表**一一对应**，
            // 由 `scripts/check-api-contract.cjs` 校验。
            commands::get_qoder_accounts,
            commands::get_qoder_local_sources,
            commands::qoder_import_local,
            commands::qoder_import_pat,
            commands::qoder_set_account_remark,
            commands::qoder_delete_account,
            commands::qoder_refresh_account,
            commands::qoder_checkin_status,
            // issue #47：逐账号「今日签到状态」只读重取（与 server 的同名路由一一对应）。
            commands::qoder_checkin_status_map,
            commands::qoder_checkin_claim,
            commands::qoder_pro_pack_status,
            commands::qoder_pro_pack_claim,
            commands::qoder_credits,
            commands::qoder_oauth_start,
            commands::qoder_oauth_status,
            commands::qoder_program_status,
            commands::qoder_switch_account,
            commands::qoder_set_account_group,
            commands::qoder_usage,
            // issue #111 · 网页端逐资源包明细：登录窗 + 会话收割 + 取证探针。
            // ⚠️ 仅桌面端（`Webview` 只存在于 Tauri 宿主）；server 侧同名路由返回结构化拒绝。
            commands::qoder_web_login_open,
            commands::qoder_web_login_close,
            commands::qoder_web_credit_packs_probe,
            commands::qoder_web_credit_packs,
            commands::qoder_web_session_status,
            commands::qoder_web_window_cookies,
            commands::qoder_web_proxy_get,
            commands::qoder_web_proxy_set,
            commands::qoder_web_session_logout,
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
            // ★ issue #76：Trae 密钥策略与按密钥用量（与 WB 侧同形）。
            commands::set_trae_key_policy,
            commands::trae_key_usage,
            commands::restore_trae_api_key,
            commands::delete_trae_api_key,
            commands::open_trae_data_dir,
            commands::trae_launch_client,
            commands::get_trae_gateway_logs,
            commands::clear_trae_gateway_logs,
            // Qoder API 网关（管理面）—— 与上面两套网关命令平行（**一个监听两个区域**）。
            commands::get_qoder_gateway_config,
            commands::save_qoder_gateway_config,
            commands::qoder_gateway_status,
            commands::get_qoder_gateway_models,
            commands::qoder_gateway_quota,
            commands::list_qoder_api_keys,
            commands::create_qoder_api_key,
            commands::revoke_qoder_api_key,
            // ★ issue #76：Qoder 密钥策略与按密钥用量（与 WB / Trae 侧同形）。
            commands::set_qoder_key_policy,
            commands::qoder_key_usage,
            commands::restore_qoder_api_key,
            commands::delete_qoder_api_key,
            commands::get_qoder_gateway_logs,
            commands::clear_qoder_gateway_logs,
            // P9 T02 · 桌面悬浮窗**只读数据层**（命令面只有读 / 显隐 / 聚焦，无写操作）。
            commands::companion_status,
            commands::companion_set_visible,
            commands::companion_toggle,
            commands::companion_jump,
            // P15 · 外部 Agent 会话监听：hook 安装状态（只读）+ 开关（写客户端配置）。
            commands::get_agent_hook_status,
            commands::set_agent_hook_enabled,
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
                TASKS
                    .iter()
                    .any(|spec| spec.kind == TaskKind::Scheduled(task)),
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
        // ★ 网关用量「今日」快照（2026-10-15 缺陷修复）：不登记 ⇒ 桌面端不会周期记录
        // gateway 桶 ⇒ `token_daily` 的 90 天历史只有「当天打开过账号页」的日子，
        // 网关用量成不了趋势。
        assert!(
            has_task(Product::Workbuddy, TaskKind::GatewayUsageRecord),
            "注册表缺少网关用量今日快照 —— 缺了它 record_gateway_today 只剩账号页一个\
             调用点，token_daily 的 90 天历史大片空洞、账号卡片的 gateway 桶长时间不刷新"
        );
        // ★ 客户端（登录态）用量「今日」快照（2026-10-15 用户报障修复）：不登记 ⇒ 桌面端
        // 不周期扫客户端语料 ⇒ 卡片「今日 Token」的 client 列只在打开账号页时才刷新，
        // 而同卡片的 gateway 桶已被周期刷新（用户看到的「client 数字不动」）。
        assert!(
            has_task(Product::Workbuddy, TaskKind::ClientUsageRecord),
            "注册表缺少客户端用量今日快照 —— 缺了它 record_client_today 只剩账号页一个\
             调用点，账号卡片「今日 Token」的 client 列长时间不刷新"
        );
    }

    /// ★★ 护栏：**四个**网关在 `setup` 里都必须被 `apply()` 拉起（issue #52）。
    ///
    /// 回归背景：统一网关只 `app.manage(...)`、没有对应的 `spawn + apply()` ——
    /// 于是配置里 `enabled: true` 只在「保存配置」那一刻生效，重启应用后 57892
    /// 上没有监听，客户端表现为「一直生成中」，网关侧连一条日志都没有。
    ///
    /// 为什么是**源码级**断言而不是起一个 app：`setup` 里需要 `tauri::Builder` +
    /// 事件循环，单测起不来。四个运行时的 `state::<...>()` 取用 + `.apply()` 只在这里
    /// 出现一次，标记足够精确（`commands.rs` 里是 `app.state::<...>()`，不会被误命中）。
    ///
    /// 可证伪：删掉任一段 `spawn + apply`（或只留 `manage`）⇒ 本用例立刻变红。
    #[test]
    fn every_gateway_is_started_from_setup() {
        // ★ 读**运行期**源码并截掉测试段：`include_str!` 会把本用例自己也算进去，
        //   那几条标记字面量就会自己满足断言（护栏空转）。
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs");
        let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {path}: {e}"));
        let setup = source
            .split("#[cfg(test)]")
            .next()
            .expect("文件必有一段非测试部分");
        for (label, runtime) in [
            ("WorkBuddy 57891", "gateway::GatewayRuntime"),
            ("Trae 57894", "trae_gateway::TraeGatewayRuntime"),
            ("Qoder 57895", "qoder_gateway::QoderGatewayRuntime"),
            ("统一网关 57892", "unified_gateway::UnifiedGatewayRuntime"),
        ] {
            let marker = format!("handle.state::<{runtime}>()");
            let at = setup.find(&marker).unwrap_or_else(|| {
                panic!("setup 里没有取用 {label} 的运行时 —— 该网关不会随应用启动监听")
            });
            // `apply` 必须紧跟在取用之后（几行内），确保它真的被用于启动而不是弃之不用。
            assert!(
                setup[at..]
                    .lines()
                    .take(4)
                    .any(|line| line.contains(".apply()")),
                "{label} 取到运行时后没有调用 `.apply()` —— 开关打开也不会起监听"
            );
        }

        // 另一侧宿主（`buddy-switch-server`）同样必须启动四个 —— 它与桌面端**各持一份**
        // 启动序列（刻意不去重），任何一侧漏一段都是同一个缺陷。
        let server_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/buddy-switch-server/src/main.rs"
        );
        let server = std::fs::read_to_string(server_path)
            .unwrap_or_else(|e| panic!("读不到 {server_path}: {e}"));
        let server_main = server
            .split("#[cfg(test)]")
            .next()
            .expect("文件必有一段非测试部分");
        for host in [
            "gateway_host::apply()",
            "trae_gateway_host::apply()",
            "qoder_gateway_host::apply()",
            "unified_gateway_host::apply()",
        ] {
            assert!(
                server_main.contains(host),
                "buddy-switch-server 的启动序列缺少 `{host}` —— 该网关在 webui 侧不会随进程启动"
            );
        }
    }
}
