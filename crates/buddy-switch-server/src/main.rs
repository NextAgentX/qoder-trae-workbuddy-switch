//! buddy-switch CLI：npm 安装形态的入口（展示名 `Buddy Switch`）。
//!
//! ```bash
//! buddy-switch              # 启动本地服务 + 打开浏览器 webui
//! buddy-switch serve        # 只起服务不开浏览器（--port / --host / --no-open）
//! buddy-switch status       # 终端输出当前账号
//! buddy-switch version      # 版本号
//! ```
//!
//! ## 监听地址（容器 / 反代）
//!
//! 默认只绑 `127.0.0.1:57890`。容器里从外部访问不到回环地址，因此地址与端口都可以覆盖：
//!
//! | 优先级 | 地址 | 端口 |
//! |:---|:---|:---|
//! | 1（最高） | `--host <addr>` | `--port <n>` |
//! | 2 | 环境变量 `BUDDY_SWITCH_HOST` | 环境变量 `BUDDY_SWITCH_PORT` |
//! | 3 | `127.0.0.1` | `57890` |
//!
//! ⚠️ 监听地址**不是回环**时会打印一条警告：webui 能读写账号库与网关 Key，
//! 对外暴露前请读 `docs/guides/DOCKER.md` 的安全段。数据落点由 `BUDDY_SWITCH_HOME` 决定，
//! 与本组变量无关。

mod api;
// 管理面的来源校验（Host / Origin / Content-Type）。**必须在 `api::router()` 之前装载策略。**
mod api_guard;
mod gateway_host;
// Qoder 网关（第三个平台，默认 57895）：独立端口，不与另三个 merge（axum 同路径 merge 会 panic）。
mod qoder_gateway_host;
mod trae_gateway_host;
// 统一网关（第三个产品线，默认 57892）：跨产品线的单一入口 —— 独立端口，不与另两个 merge。
mod unified_gateway_host;

use serde_json::json;

use buddy_switch_core::modules::background::{TaskKind, TaskSpec, TASKS};
use buddy_switch_core::modules::product::Product;
use buddy_switch_core::modules::{
    account, auth_file, backup, config, process, region::Region, rotate, runtime_log, schedule,
    scheduler, token_daily, trae, update,
};

fn default_port() -> u16 {
    57890
}

/// 监听地址覆盖用的环境变量名（容器 / 反代场景）。
///
/// 容器里 `127.0.0.1` 从外部访问不到，必须能从环境变量指定 `0.0.0.0`。
/// 与 `BUDDY_SWITCH_HOME` 同一层：**只改监听地址**，不改变任何数据落点。
const BUDDY_SWITCH_HOST_ENV: &str = "BUDDY_SWITCH_HOST";

/// 监听端口覆盖用的环境变量名（优先级低于 CLI `--port`）。
const BUDDY_SWITCH_PORT_ENV: &str = "BUDDY_SWITCH_PORT";

/// 解析监听地址：`--host <addr>` > `BUDDY_SWITCH_HOST` > `127.0.0.1`。
///
/// 取 env 值作**入参**而非直接读 `std::env`：测试可以并行跑，不会互相踩环境变量
/// （本仓的既有约定：可并行单测的纯决策函数一律显式入参）。
fn resolve_host(args: &[String], env: Option<&str>) -> String {
    if let Some(index) = args.iter().position(|arg| arg == "--host") {
        if let Some(host) = args
            .get(index + 1)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            return host.to_string();
        }
    }
    env.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("127.0.0.1")
        .to_string()
}

/// 解析监听端口：`--port <n>` > `BUDDY_SWITCH_PORT` > [`default_port`]。
///
/// 非法值（非数字 / 超出 `u16`）**静默回落**下一优先级：这与 `--port` 的既有行为一致
/// （启动不该因为一个错字而失败），且启动日志里会打印最终生效的地址，便于发现。
fn resolve_port(args: &[String], env: Option<&str>) -> u16 {
    if let Some(index) = args.iter().position(|arg| arg == "--port") {
        if let Some(port) = args
            .get(index + 1)
            .and_then(|value| value.parse::<u16>().ok())
        {
            return port;
        }
    }
    env.and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or_else(default_port)
}

/// 该监听地址是否**只对本机可见**。
///
/// 用于决定要不要打印那条「已对外监听」的警告。判据刻意保守（只认几个明确的回环写法）：
/// 把 `0.0.0.0` / 局域网 IP / 域名误判成回环 ⇒ 警告不出现，用户不知道 webui 已对外。
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// 「给浏览器的地址」：监听地址是回环就用它，否则换成 `127.0.0.1`。
///
/// `--host 0.0.0.0` 时 `http://0.0.0.0:57890` 打不开（Windows 上还会被当成
/// 不可路由地址），而且 `Host: 0.0.0.0` 会被管理面的来源校验拒绝。
/// 端口保留用户给的那个 —— 对外监听与本地打开用的是同一个端口。
fn browser_addr(host: &str, port: u16) -> String {
    if is_loopback_host(host) {
        format!("{host}:{port}")
    } else {
        format!("127.0.0.1:{port}")
    }
}

/// 为某一类任务起一个独立排程循环。
///
/// 排程语义（重读配置 → 算 `next_fire` → sleep 到点 → 派发）全部在
/// [`scheduler::schedule_loop`] 里，**桌面端与服务端共用同一份**：此前它只写在本文件，
/// 桌面端另有一套写死周期的循环，两边行为漂移且桌面端的排程配置完全不生效。
fn spawn_scheduled_task(task: schedule::ScheduleTask) {
    tokio::spawn(scheduler::schedule_loop(task));
}

// 「有哪些后台任务」已二维化到 core（`buddy_switch_core::modules::background::TASKS`）：
// 每个条目是 `(product, kind)` 二维坐标。本文件此前持有一份**扁平**枚举 `BackgroundTask`
// （且产品名进了枚举名，如 `TraeCreditsRefresh`），把「任务种类」与「产品线」两个正交维度
// 混在一起；现改为直接消费 core 的 TASKS —— 表是**数据**（两宿主共用），宿主只负责
// **怎么启动**（见下方 `spawn_background_task`）。

/// 账号池状态落盘周期（毫秒）。
///
/// 取值理由：[`buddy_switch_gateway::pool::Pool::flush_if_dirty`] 只在**有改动**时
/// 才真正写盘，所以这里定的是「最坏情况下最多丢多少治理状态」。30 秒足够短
/// （冷却 / 熔断的时效是分钟级，丢 30 秒不会造成错误决策），又远长于一次请求，
/// 不会给请求路径加负担。
const POOL_PERSIST_INTERVAL_MS: u64 = 30_000;

/// Trae 积分刷新周期（毫秒）。
///
/// 与 WorkBuddy 余额刷新的缺省间隔（30 分钟）保持一致：同一管理面不该有两种节奏。
const TRAE_CREDITS_REFRESH_INTERVAL_MS: u64 = 30 * 60 * 1000;

/// 定时备份的检查周期（毫秒）。
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

/// 启动全部后台任务（以 core 的 [`TASKS`] 为**唯一事实来源**）。
///
/// ⚠️ **登记即执行**——不要在本函数之外直接 `tokio::spawn` 后台循环；新增「写状态 /
/// 周期性」出口必须先在 core [`TASKS`] 登记，否则两个宿主都不会启动它。
///
/// 「有哪些后台任务」是**数据**（core [`TASKS`]，两宿主共用），本函数只负责**按表启动**；
/// 「怎么启动」按宿主运行时决定（server 用 `tokio::spawn`）。
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
/// [`TaskSpec::product`] 分派 —— 两者实现**完全不同**（WorkBuddy 走
/// `gateway_host::shared_state` 的池，Trae 走 `trae::credits` 的区域账号库）。
fn spawn_background_task(spec: TaskSpec) {
    match spec.kind {
        // 启动：整理历史签到日志 + 补跑一轮（签到核验 / 旅行派出领取 / 保活），
        // 各类是否补跑受自己的排程开关约束。与桌面端共用 core::scheduler 的同一份语义。
        TaskKind::StartupMaintenance => {
            tokio::spawn(async move {
                scheduler::run_startup_maintenance().await;
            });
        }
        // 自动轮换：按配置间隔执行。
        TaskKind::AutoRotate => {
            tokio::spawn(async move {
                let mut last_cycle_at: i64 = 0;
                loop {
                    let cfg = config::load_auto_rotate_config();
                    if cfg.get("enabled").and_then(|v| v.as_bool()) == Some(true) {
                        let interval_minutes = cfg
                            .get("check_interval_minutes")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(5)
                            .max(1);
                        let now = config::now_ms();
                        if now - last_cycle_at >= interval_minutes * 60_000 {
                            last_cycle_at = now;
                            let _ = rotate::run_rotate_cycle().await;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            });
        }
        // 账号池余额 / 积分刷新：**独立**循环（不与其它循环合并成一个 tick），首次启动先跑
        // 一次，之后周期刷新。按**产品线**分派实现。
        TaskKind::CreditsRefresh => match spec.product {
            // WorkBuddy 账号池余额刷新（真实余额回填选号权重）。
            //
            // 为什么必须是独立后台循环：余额是慢变数据，在请求路径上同步拉余额会直接拉高
            // 每次请求的延迟。刷新语义见 `buddy_switch_gateway::credits_refresh`。
            Product::Workbuddy => {
                tokio::spawn(async move {
                    let state = gateway_host::shared_state();
                    loop {
                        // 启动即刷一次（循环首轮）：补齐上次进程遗留的「从未取过余额」账号。
                        let _ = buddy_switch_gateway::credits_refresh::refresh_once(&state).await;
                        // ★ 等待时长由 gateway 侧统一给（两侧宿主共用同一判据）：
                        //   池为空 ⇒ 短探测，否则「刚导入账号」要等满一整个周期才看得到积分。
                        let wait_ms = {
                            let pool = state.pool.read().await;
                            buddy_switch_gateway::credits_refresh::next_refresh_wait_ms(&pool)
                        };
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    }
                });
            }
            // Trae 积分刷新（两条国内产品线）：与 WorkBuddy 余额刷新同为独立循环。
            Product::Trae => {
                tokio::spawn(async move {
                    // 启动即刷一次：补齐上次进程遗留的陈旧读数。
                    refresh_trae_credits().await;
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            TRAE_CREDITS_REFRESH_INTERVAL_MS,
                        ))
                        .await;
                        refresh_trae_credits().await;
                    }
                });
            }
            /* Qoder：**尚未有余额刷新循环**（额度要调上游，属网关层，未落地）。
            ⚠️ 这里**不是**「静默吞掉」：core 的 `TASKS` 表里没有任何 Qoder 任务
            ⇒ 本分支当前不可达。一旦有人往 `TASKS` 里加了 Qoder 的 `CreditsRefresh`，
            这行警告会立刻出现在日志里，而不是让它悄悄空转。 */
            Product::Qoder => {
                eprintln!(
                    "[background] Qoder 的 CreditsRefresh 已登记但**尚无实现** —— \
                     请补刷新循环，或从 core 的 TASKS 表里移除该条目。"
                );
            }
        },
        // 账号池治理状态落盘：**周期**刷新，且只在有改动时写（`flush_if_dirty`）。
        //
        // ★ 为什么必须有它：`Pool::flush_if_dirty` 此前**没有任何生产调用点** ——
        //   唯一的包装 `GatewayState::persist_pool` 自身零调用者，而 `Pool::load`
        //   读的那个文件因此**永远是旧的**。后果不是崩溃而是**静默遗忘**：
        //   冷却 / 熔断 / 成功率 EMA / 余额读数 / `credits_refreshed_ms`
        //   全部只在内存里，每次重启从零开始（例如刚判定 SessionDead 的账号
        //   重启后立刻又被选中、再撞一次同样的墙）。
        //
        //   这与 core 后台任务模块文档里记的 `set_credits` 事故是**同一类**：
        //   「函数存在、类型全对、构建全绿、测试全绿，但生产上没人调用」。
        //   登记进 core TASKS 后，core 与宿主两侧的护栏都守住它不被删除。
        TaskKind::PoolPersist => {
            tokio::spawn(async move {
                let state = gateway_host::shared_state();
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(POOL_PERSIST_INTERVAL_MS))
                        .await;
                    state.persist_pool().await;
                }
            });
        }
        // 定时全局备份（D17）：**默认开**，但可在设置里关掉（§7 #7）——关闭后不再产生
        // 新备份（已产生的保留）。`auto_backup()` 当日已备份则幂等返回，检查开销极低。
        TaskKind::AutoBackup => {
            tokio::spawn(async move {
                loop {
                    if config::is_auto_backup_enabled() {
                        let _ = backup::auto_backup();
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(
                        AUTO_BACKUP_CHECK_INTERVAL_MS,
                    ))
                    .await;
                }
            });
        }
        // ★ 网关启动补识别（P10 §1.6 / ✅ #7）：**一次性**——启动时把「内置 / 陈旧」的清单
        // 补成实时，之后交给「接入即识别」与手动「重新识别」。失败静默（探测不得让启动失败）。
        TaskKind::GatewayCatalogProbe => {
            tokio::spawn(async move {
                let state = gateway_host::shared_state();
                let _ = buddy_switch_gateway::catalog_probe::probe_stale_regions(&state).await;
            });
        }
        // ★ 网关用量「今日」快照（2026-10-15 新增，缺陷修复）：周期把网关日志聚合进
        // `token_daily` 的 gateway 桶并落盘。
        //
        // 为什么必须有它：`record_gateway_today` 此前唯一的调用点是「打开账号页」的
        // 命令（`api_account_token_today`）⇒ ① 不进账号页，gateway 桶一直是旧值；
        // ② `token_daily` 保留 90 天，但历史只覆盖「当天 App 被打开过」的日子
        //    （实测大片空洞）⇒ 网关用量永远成不了趋势。
        //
        // 覆盖**两个 region**：`gateway_logs.json` 是单文件、行内带 `region`，
        // 只记一个 region 会让另一个的历史永远空着。
        TaskKind::GatewayUsageRecord => {
            tokio::spawn(async move {
                loop {
                    for region in Region::all() {
                        let _ = token_daily::record_gateway_today(region);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(
                        GATEWAY_USAGE_RECORD_INTERVAL_MS,
                    ))
                    .await;
                }
            });
        }
        // ★ 客户端（登录态）用量「今日」快照（2026-10-15 用户报障修复）：周期把客户端
        // 会话日志的**今日**用量聚合进 `token_daily` 的 client 桶并落盘。
        //
        // 为什么必须有它：`record_client_today` 此前唯一的调用点是「打开账号页」的命令
        // （`api_account_token_today`）⇒ 不进账号页，卡片上的 client 列一直是旧值
        // （用户报障：「client 数字不动」）。**服务端常驻进程从不打开账号页**，这条在
        // 服务端尤其致命；桌面端同形（见 `src-tauri/src/lib.rs` 同分支）。
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
            tokio::spawn(async move {
                loop {
                    for region in Region::all() {
                        let _ = token_daily::record_client_today(region, false);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(
                        CLIENT_USAGE_RECORD_INTERVAL_MS,
                    ))
                    .await;
                }
            });
        }
        // 六类积分任务各自独立排程。
        TaskKind::Scheduled(task) => spawn_scheduled_task(task),
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
    for variant in trae::variant::TraeVariant::all() {
        let _ = trae::credits::refresh_all_remaining_for(variant).await;
    }
}

/// 状态快照的 `current` 段：**纯函数形态**（显式入参，测试用，不碰磁盘）。
///
/// 与 core `account::library_display_name_in` 同款拆分理由：**单测不该依赖进程级
/// `BUDDY_SWITCH_HOME`**。进程级 env 是全局状态，同一测试二进制内并行跑用例时，
/// 一个改 home 的用例会把其它用例一起带红（本仓已踩过多次）。
///
/// ⚠️ 展示字段一律归一（[`account::display_str`] / [`account::current_nickname_for`]）：
/// 认证文件里 `nickname` 可能是对象（脏值）或 WorkBuddy 5.6 的加密信封，裸透传会把
/// 对象当 React 子节点渲染。这是上游 PR #75 的**第 3 处同构透传点** ——
/// 本仓 `api.rs` / `commands.rs` 早已归一，CLI 这一处此前漏了
/// （PR #75 里对应的提交 `4b000ac` 就是「补上漏掉的那一处」）。
fn snapshot_from_root(root: &serde_json::Value) -> serde_json::Value {
    let acct = root.get("account").cloned().unwrap_or_else(|| json!({}));
    json!({
        "uid": account::display_str(&acct, "uid"),
        "nickname": account::current_nickname_for(Region::Cn, &acct),
        "email": account::display_str(&acct, "email"),
    })
}

/// 状态快照的 `current` 段（从认证文件读；无认证文件返回 `None`）。
fn current_account_snapshot() -> Option<serde_json::Value> {
    Some(snapshot_from_root(&auth_file::read_auth_file()?))
}

fn print_status() {
    let current = current_account_snapshot();
    let running = process::is_workbuddy_running();
    println!("Buddy Switch v{}", update::APP_VERSION);
    println!("WorkBuddy 运行中: {}", if running { "是" } else { "否" });
    match current {
        Some(c) => {
            let name = c
                .get("nickname")
                .and_then(|v| v.as_str())
                .or_else(|| c.get("email").and_then(|v| v.as_str()))
                .unwrap_or("未知");
            println!("当前账号: {name}");
        }
        None => println!("当前账号: 未登录"),
    }
    println!("账号数: {}", account::load_accounts().len());
}

/// 启动前护栏：`BUDDY_SWITCH_HOME` **被设置但不合法**时**拒绝启动**（fail-closed）。
///
/// 为什么不是「警告后回落真实 home」：那正是 2026-09-30 那次事故的形态 ——
/// 调用方以为自己指向隔离目录，实际把账号 / 网关 Key / 签到与积分记录写到了**真实数据**上，
/// 而唯一的提示是一行可能被 `| head` 吞掉的 stderr 警告。
/// 未设置该变量时行为完全不变（正常使用真实 home）。判定与文案都在
/// [`buddy_switch_core::modules::config::env_home_override_refusal`]（纯函数，可单测）。
fn refuse_when_home_override_is_invalid(cmd: &str) {
    let value = std::env::var(buddy_switch_core::modules::config::BUDDY_SWITCH_HOME_ENV).ok();
    if let Some(problem) = home_override_problem(cmd, value.as_deref()) {
        eprintln!("[buddy-switch] 拒绝启动：{problem}");
        std::process::exit(2);
    }
}

/// [`refuse_when_home_override_is_invalid`] 的**纯决策**部分（入参即取值，可并行单测）。
///
/// `None` = 放行。两种放行情形：
/// 1. **不碰 store** 的命令（版本号查询、`hook` 转发）不该被环境变量拦住 ——
///    `hook` 只把 stdin 转给本地接收端，读不到任何账号 / 会话数据；
/// 2. 环境变量未设置 / 空白 / 合法。
fn home_override_problem(cmd: &str, value: Option<&str>) -> Option<String> {
    if matches!(cmd, "version" | "--version" | "-V" | "hook") {
        return None;
    }
    buddy_switch_core::modules::config::home_override_refusal(value)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("serve");
    refuse_when_home_override_is_invalid(cmd);
    match cmd {
        "status" => print_status(),
        "version" | "--version" | "-V" => {
            println!("Buddy Switch {}", env!("CARGO_PKG_VERSION"));
        }
        // P15 · 客户端 hook 调用的入口（与桌面宿主 `src-tauri/src/main.rs` 同一份实现）。
        // 走 core 的 `try_run_hook_cli` ⇒ 两个宿主的 hook 行为不可能分叉。
        "hook" => std::process::exit(
            buddy_switch_core::modules::agent_hook_rx::try_run_hook_cli(&args).unwrap_or(0),
        ),
        _ => serve(&args).await,
    }
}

async fn serve(args: &[String]) {
    let port = resolve_port(args, std::env::var(BUDDY_SWITCH_PORT_ENV).ok().as_deref());
    let host = resolve_host(args, std::env::var(BUDDY_SWITCH_HOST_ENV).ok().as_deref());

    // ★ 安全审计 P0-1：装载管理面的 Host 白名单（默认只有回环）。
    //   必须在 `api::router()` 之前 —— `router()` 里的中间件会在构造时取策略。
    let allowed_hosts = std::env::var(api_guard::ALLOWED_HOSTS_ENV).ok();
    api_guard::init(allowed_hosts.as_deref());

    let app = api::router();
    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "启动失败: {addr} 被占用或不可用（{e}）。\
                 可用 --port 指定其他端口，或 --host 指定监听地址。"
            );
            std::process::exit(1);
        }
    };

    println!("Buddy Switch v{}", update::APP_VERSION);
    println!("webui: http://{addr}");
    // ★ 对外监听时**必须明说**：webui 能读写账号库与网关 Key（见 docs/guides/DOCKER.md 的安全段），
    // 默认只绑回环正是为了「不声不响地把管理面暴露出去」这件事不发生。
    // 判据用最终生效的地址（不是用户填的原始值），否则 `--host localhost` 这类写法会漏报。
    if !is_loopback_host(&host) {
        println!(
            "⚠️  webui 正对外监听（{host}）—— 它能读写账号库与网关 Key。\
             请确保只在可信网络内暴露，或用反向代理加上鉴权。"
        );
        // ★ 安全审计 P0-1：管理面默认只接受回环 Host。对外监听时**必须**告知怎么放开，
        //   否则远程用户只会看到一个 403，然后以为软件坏了。
        match api_guard::init(allowed_hosts.as_deref()).clone() {
            policy if policy == api_guard::Policy::default() => println!(
                "⚠️  管理面只接受本机访问（Host 必须是 127.0.0.1 / localhost / ::1）。\
                 远程访问请在启动前设置 {}（逗号分隔，可带端口）。",
                api_guard::ALLOWED_HOSTS_ENV
            ),
            _ => println!(
                "管理面允许的额外 Host 来自 {}。",
                api_guard::ALLOWED_HOSTS_ENV
            ),
        }
    }
    println!("按 Ctrl+C 停止服务。");

    // ★ 运行日志：装 panic hook + 写启动横幅（版本 / 平台 / 数据目录 / 级别）。
    //   服务端有控制台，但仍要落盘 —— webui 的「日志」页读的是同一批文件，
    //   而 Docker 部署时控制台输出常常没人留。
    runtime_log::init("Buddy Switch 服务端");
    runtime_log::info("startup", format!("webui 监听 {addr}"));

    let no_open = args.iter().any(|a| a == "--no-open");
    if !no_open {
        // ★ 打开浏览器要用**回环**地址：`--host 0.0.0.0` 时 `http://0.0.0.0:57890`
        //   既不是有效访问方式，也会被管理面的 Host 校验拒掉（见 `api_guard`）。
        open_browser(&browser_addr(&host, port));
    }

    spawn_background_loops();

    // P15 · 外部 Agent hook 接收端（与桌面宿主 `src-tauri/src/lib.rs` 同款）。
    // ★ 只绑 `127.0.0.1:57893`，**事件驱动**（不是周期任务 ⇒ 不进 `background.rs` 的注册表）。
    // ★ 起不来（端口被占 / 桌面版已持有）**不阻断 webui** —— 两者同时跑时只有一处能收，
    //   而「客户端 hook 指向哪一个」由用户最后装的 hook 决定。
    tokio::spawn(async {
        if let Err(error) = buddy_switch_core::modules::agent_hook_rx::serve().await {
            runtime_log::error("agent-hook", error.to_string());
        }
    });

    // 按配置启动 API 网关独立监听（默认关闭；默认 127.0.0.1:57891）。
    // ★ 启动结果同时落**运行日志**：webui 的「日志」页读的是同一批文件，
    //   而控制台输出在 Docker / 后台运行时常常没人留（这正是本功能存在的理由）。
    match gateway_host::apply().await {
        Ok(Some(addr)) => {
            println!("API 网关: http://{addr}");
            runtime_log::info("gateway", format!("已启动: http://{addr}"));
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("[gateway] 启动失败: {error}");
            runtime_log::error("gateway", format!("启动失败: {error}"));
        }
    }

    // 按配置启动 Trae 网关独立监听（默认关闭；默认 127.0.0.1:57894）。
    match trae_gateway_host::apply().await {
        Ok(Some(addr)) => {
            println!("Trae API 网关: http://{addr}");
            runtime_log::info("trae-gateway", format!("已启动: http://{addr}"));
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("[trae-gateway] 启动失败: {error}");
            runtime_log::error("trae-gateway", format!("启动失败: {error}"));
        }
    }

    // 按配置启动 Qoder 网关独立监听（默认关闭；默认 127.0.0.1:57895）。
    // ⚠️ 一个监听、两个区域（区域由请求所用 API Key 的归属决定）。
    match qoder_gateway_host::apply().await {
        Ok(Some(addr)) => {
            println!("Qoder API 网关: http://{addr}");
            runtime_log::info("qoder-gateway", format!("已启动: http://{addr}"));
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("[qoder-gateway] 启动失败: {error}");
            runtime_log::error("qoder-gateway", format!("启动失败: {error}"));
        }
    }

    // 按配置启动**统一网关**独立监听（默认关闭；默认 127.0.0.1:57892）。
    // ⚠️ 起不来**不阻断**另两个入口 —— 三个入口互不牵连（关掉统一入口不影响任何既有客户端）。
    match unified_gateway_host::apply().await {
        Ok(Some(addr)) => {
            println!("统一 API 网关: http://{addr}");
            runtime_log::info("unified-gateway", format!("已启动: http://{addr}"));
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("[unified-gateway] 启动失败: {error}");
            runtime_log::error("unified-gateway", format!("启动失败: {error}"));
        }
    }

    axum::serve(listener, app).await.unwrap();
}

fn open_browser(addr: &str) {
    let url = format!("http://{addr}");
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = std::process::Command::new("cmd");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：开浏览器不闪 cmd 窗
        }
        let _ = c.args(["/C", "start", &url]).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_switch_core::modules::background::has_task;

    /// ★★ 护栏：`BUDDY_SWITCH_HOME` **被设置但不合法**时必须**拒绝启动**。
    ///
    /// 反例（改坏会红）：退回「警告后回落真实 home」⇒ 调用方以为在隔离环境里跑，
    /// 实际把账号 / 网关 Key / 签到与积分记录写到用户真实数据上（2026-09-30 真实事故）。
    #[test]
    fn invalid_home_override_refuses_to_start() {
        // 不存在 / 相对路径 → 拒绝，且理由要点名那个环境变量与修法。
        for bad in ["D:/no/such/dir/buddy-switch-home", "relative-home", "."] {
            let problem = home_override_problem("serve", Some(bad))
                .unwrap_or_else(|| panic!("{bad} 必须被拒绝"));
            assert!(problem.contains("BUDDY_SWITCH_HOME"), "{problem}");
            assert!(problem.contains("真实"), "理由要说清后果：{problem}");
        }

        // 未设置 / 空白 / 合法目录 → 放行（行为与改造前一致）。
        assert!(home_override_problem("serve", None).is_none());
        assert!(home_override_problem("serve", Some("   ")).is_none());
        let dir = std::env::temp_dir();
        assert!(
            home_override_problem("serve", Some(dir.to_str().expect("utf8 temp dir"))).is_none(),
            "已存在的绝对目录必须放行"
        );

        // 纯查询命令不受影响（它不碰任何数据）。
        assert!(home_override_problem("version", Some("D:/no/such/dir")).is_none());
        assert!(home_override_problem("--version", Some("relative")).is_none());
    }

    // -----------------------------------------------------------------------
    // 监听地址 / 端口（容器与反代场景，issue #12）
    // -----------------------------------------------------------------------

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// ★ 安全审计 P0-1：`--host 0.0.0.0` 时给浏览器打开的必须是**回环**地址。
    ///
    /// 反例（改坏会红）：直接 `http://0.0.0.0:57890` —— 打不开，而且
    /// `Host: 0.0.0.0` 会被管理面的来源校验拒成 403，用户会以为软件坏了。
    #[test]
    fn browser_url_uses_loopback_even_when_listening_wide() {
        assert_eq!(browser_addr("127.0.0.1", 57890), "127.0.0.1:57890");
        assert_eq!(browser_addr("localhost", 57890), "localhost:57890");
        // 对外监听：地址换成回环，**端口不变**（否则打开的就不是刚起的那个服务）。
        assert_eq!(browser_addr("0.0.0.0", 57890), "127.0.0.1:57890");
        assert_eq!(browser_addr("192.168.1.5", 1234), "127.0.0.1:1234");
    }

    /// ★ 默认必须仍是**回环**：改坏这条等于「不声不响把 webui 暴露到局域网」。
    #[test]
    fn listen_defaults_to_loopback_and_default_port() {
        assert_eq!(resolve_host(&args(&["serve"]), None), "127.0.0.1");
        assert_eq!(resolve_port(&args(&["serve"]), None), default_port());
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
    }

    /// 优先级：CLI > 环境变量 > 默认值（三条都钉住，防止某一级被悄悄改掉）。
    #[test]
    fn cli_beats_env_beats_default() {
        // 只有 env
        assert_eq!(resolve_host(&args(&["serve"]), Some("0.0.0.0")), "0.0.0.0");
        assert_eq!(resolve_port(&args(&["serve"]), Some("9999")), 9999);
        // CLI 压过 env
        assert_eq!(
            resolve_host(&args(&["serve", "--host", "10.0.0.5"]), Some("0.0.0.0")),
            "10.0.0.5"
        );
        assert_eq!(
            resolve_port(&args(&["serve", "--port", "1234"]), Some("9999")),
            1234
        );
    }

    /// 非法 / 空白值必须**回落下一优先级**而不是 panic 或产生非法地址。
    #[test]
    fn invalid_overrides_fall_back_instead_of_panicking() {
        // 空白 host / 非法 port 一律回落。
        assert_eq!(resolve_host(&args(&["serve"]), Some("   ")), "127.0.0.1");
        assert_eq!(
            resolve_host(&args(&["serve", "--host", "  "]), Some("0.0.0.0")),
            "0.0.0.0"
        );
        assert_eq!(
            resolve_port(&args(&["serve"]), Some("not-a-number")),
            default_port()
        );
        // 超出 u16 的端口同样回落（不能截断成 0 或 panic）。
        assert_eq!(
            resolve_port(&args(&["serve"]), Some("70000")),
            default_port()
        );
        assert_eq!(
            resolve_port(&args(&["serve", "--port", "abc"]), Some("8080")),
            8080,
            "非法 CLI 端口应回落环境变量，而不是直接跳回默认值"
        );
        // `--host` 后面没跟值时不得 panic，也不得把下一个 flag 当地址。
        assert_eq!(resolve_host(&args(&["serve", "--host"]), None), "127.0.0.1");
    }

    /// 对外监听必须被 `is_loopback_host` 判成「非回环」—— 那条警告靠它触发。
    ///
    /// 反例（改坏会红）：把判据写成「以 `127.` 开头」之类，`0.0.0.0` 就会漏判，
    /// 用户以为还在本机、实际整个局域网都能打开管理面。
    #[test]
    fn non_loopback_hosts_are_not_treated_as_local() {
        for host in ["0.0.0.0", "192.168.1.10", "10.0.0.5", "example.com", "[::]"] {
            assert!(
                !is_loopback_host(host),
                "{host} 不是回环地址，必须触发对外监听警告"
            );
        }
    }

    /// 护栏：余额刷新任务**必须**登记在 core 后台任务表（[`TASKS`]）里。
    ///
    /// 回归背景：本次修复的缺陷正是「调用点静默消失」——`Pool::set_credits` 存在、
    /// 类型全对、构建全绿，但生产上无人调用，于是四因子加权静默退化为两因子。
    /// 表本体已移到 core（两宿主共用），本用例守住**服务端依赖的那份表**不丢关键任务。
    /// 服务端 `spawn` 派发是否覆盖每个 [`TaskKind`]，由 core
    /// `tests::both_hosts_register_every_task_kind`（直接读本文件源码）守住。
    #[test]
    fn background_task_registry_includes_credits_refresh() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::CreditsRefresh),
            "后台任务表必须登记 (Workbuddy, CreditsRefresh)（账号池余额刷新）；缺了它生产上\
             余额永远为 0，四因子加权会静默退化为两因子。当前表：{TASKS:?}"
        );
    }

    /// 护栏：**Trae** 积分刷新任务必须登记在 core 后台任务表里。
    ///
    /// 回归背景：Trae 的剩余积分此前只在「签到页」被刷新 —— 纯用 API（不开界面）时
    /// 读数会一直陈旧。选号用 `credits` 做「零积分排除」与「积分多优先」，
    /// 陈旧读数会让**已耗尽积分的账号仍被选中**（白撞一次上游），或**尚有余额的账号
    /// 被误判为零积分而排除**。这与上面那条是**同一类**事故：能力存在、
    /// 类型全对、单测全绿，但生产上没有调用点。
    #[test]
    fn background_task_registry_includes_trae_credits_refresh() {
        assert!(
            has_task(Product::Trae, TaskKind::CreditsRefresh),
            "后台任务表必须登记 (Trae, CreditsRefresh)；缺了它纯用 API 时积分读数永远陈旧。\
             当前表：{TASKS:?}"
        );
    }

    /// 护栏：账号池**落盘**任务必须登记在 core 后台任务表里。
    ///
    /// 回归背景（2026-09-28 实测发现，与上面 `set_credits` 那条是**同一类**事故）：
    /// `Pool::flush_if_dirty` 存在、类型全对、单测全绿，但**生产上没有任何调用点** ——
    /// 唯一的包装 `GatewayState::persist_pool` 自身零调用者。
    /// 而 `Pool::load` 在启动时读的正是它写的那个文件 ⇒ **文件永远是旧的**：
    /// 冷却 / 熔断 / 成功率 EMA / 余额读数 / `credits_refreshed_ms` 每次重启全部归零。
    #[test]
    fn background_task_registry_includes_pool_persist() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::PoolPersist),
            "后台任务表必须登记 (Workbuddy, PoolPersist)（账号池治理状态落盘）；缺了它 \
             `flush_if_dirty` 就没有任何生产调用点，冷却 / 熔断 / 成功率 / 余额读数 \
             全部只在内存里、每次重启静默归零。当前表：{TASKS:?}"
        );
    }

    /// 护栏：**定时全局备份**任务必须登记在 core 后台任务表里（P5 R9「登记即执行」）。
    ///
    /// 回归背景：不登记 ⇒ 生产上永不产生自动备份，而用户以为「出事时有备份」。
    #[test]
    fn background_task_registry_includes_auto_backup() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::AutoBackup),
            "后台任务表必须登记 (Workbuddy, AutoBackup)（定时全局备份）；缺了它生产上永不\
             产生自动备份。当前表：{TASKS:?}"
        );
    }

    /// 护栏：**网关启动补识别**任务必须登记在 core 后台任务表里（P10 T03 / ✅ #7「登记即执行」）。
    ///
    /// 回归背景：不登记 ⇒ 「内置 / 陈旧」的模型清单永远不会在启动时被补成实时，
    /// 用户只能看到「内置」来源条 —— 与「接入即识别」同属「写状态出口静默消失」一类事故。
    #[test]
    fn background_task_registry_includes_gateway_catalog_probe() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::GatewayCatalogProbe),
            "后台任务表必须登记 (Workbuddy, GatewayCatalogProbe)（网关启动补识别）；缺了它\
             启动时不会补识别「内置 / 陈旧」的清单。当前表：{TASKS:?}"
        );
    }

    /// 护栏：**网关用量今日快照**任务必须登记在 core 后台任务表里（2026-10-15 缺陷修复）。
    ///
    /// 回归背景：`record_gateway_today` 此前只有「打开账号页」一个调用点 ⇒
    /// ① 服务端常驻进程从不打开账号页，gateway 桶永远不刷新；② `token_daily` 的
    /// 90 天历史只覆盖「当天打开过账号页」的日子（实测大片空洞）⇒ 网关用量成不了趋势。
    /// 与上面几条同一类事故：能力存在、单测全绿，但生产上没有调用点。
    #[test]
    fn background_task_registry_includes_gateway_usage_record() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::GatewayUsageRecord),
            "后台任务表必须登记 (Workbuddy, GatewayUsageRecord)（网关用量今日快照）；缺了它 \
             record_gateway_today 只剩账号页一个调用点，token_daily 的 90 天历史大片空洞。\
             当前表：{TASKS:?}"
        );
    }

    /// 护栏：**客户端用量今日快照**任务必须登记在 core 后台任务表里（2026-10-15 用户报障修复）。
    ///
    /// 回归背景：`record_client_today` 此前只有「打开账号页」一个调用点 ⇒ 服务端常驻进程
    /// 从不打开账号页，卡片「今日 Token」的 **client 列永远不刷新**（而同卡片的 gateway
    /// 桶已被周期刷新 ⇒ 两个桶一新一旧）。
    #[test]
    fn background_task_registry_includes_client_usage_record() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::ClientUsageRecord),
            "后台任务表必须登记 (Workbuddy, ClientUsageRecord)（客户端用量今日快照）；缺了它 \
             record_client_today 只剩账号页一个调用点，卡片 client 列长时间不刷新。\
             当前表：{TASKS:?}"
        );
    }

    /// 表还须覆盖既有的启动维护 / 自动轮换 / 六类定时任务，防止被误删。
    #[test]
    fn background_task_registry_covers_all_existing_tasks() {
        assert!(
            has_task(Product::Workbuddy, TaskKind::StartupMaintenance),
            "表缺少启动维护任务"
        );
        assert!(
            has_task(Product::Workbuddy, TaskKind::AutoRotate),
            "表缺少自动轮换任务"
        );
        for task in schedule::ScheduleTask::all() {
            assert!(
                TASKS
                    .iter()
                    .any(|spec| spec.kind == TaskKind::Scheduled(task)),
                "表缺少定时任务：{}",
                task.as_str()
            );
        }
    }

    // ⚠️ 本模块**刻意不设**「改 `BUDDY_SWITCH_HOME` 的用例」：`api.rs` 的测试模块
    // 有它自己的 `TEST_LOCK`，两把锁互不相识 ⇒ 同一测试二进制内并行跑时照样互相踩
    // （2026-09-24 实测：本模块一个改 home 的用例把 `api.rs` 的 7 条用例带红）。
    // 凡需要隔离的取值逻辑，一律抽成**纯函数形态**（见 [`snapshot_from_root`]）。

    /// CLI `status` 的展示字段必须**归一**：对象 / 加密信封一律落 `null`。
    ///
    /// 这是上游 PR #75 的**第 3 处同构透传点**（本仓 `api.rs` / `commands.rs` 早已
    /// 归一，只有 CLI 漏了）。回归形态：裸透传会让 `nickname` 变成
    /// `{"$wbEncrypted":1,…}` 对象，终端 / 界面上就是 `[object Object]`。
    ///
    /// 可证伪：把 [`snapshot_from_root`] 改回 `acct.get("nickname")` 裸透传，
    /// 第一条断言（`is_null() || is_string()`）即红。
    #[test]
    fn status_snapshot_normalizes_display_fields() {
        // 脏值 + 加密信封：一律落 null（信封我方解不开，交给终端 / 前端回落）。
        let encrypted = snapshot_from_root(&json!({
            "account": {
                "uid": "uid-cli-probe",
                "nickname": {"$wbEncrypted": 1, "envelope": "x"},
                "email": {"zh": "对象脏值"},
            }
        }));
        for key in ["uid", "nickname", "email"] {
            let value = &encrypted[key];
            assert!(
                value.is_null() || value.is_string(),
                "{key} 必须是字符串或 null，不得是对象：{value}"
            );
        }
        assert_eq!(encrypted["uid"], "uid-cli-probe", "字符串 uid 必须原样保留");

        // 阳性对照：干净数据必须**逐字保留**，数字 uid 转文本。
        // （明文昵称走 `current_nickname_for` 的**早退**分支，不会去读账号库。）
        let clean = snapshot_from_root(&json!({
            "account": {"uid": 12345, "nickname": "小明", "email": "a@b.c"}
        }));
        assert_eq!(clean["uid"], "12345");
        assert_eq!(clean["nickname"], "小明");
        assert_eq!(clean["email"], "a@b.c");

        // 缺 `account` 段也不得 panic（历史 / 半截认证文件）。
        let empty = snapshot_from_root(&json!({}));
        assert!(empty["uid"].is_null());
        assert!(empty["nickname"].is_null());
    }
}
