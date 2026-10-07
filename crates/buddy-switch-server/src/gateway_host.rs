//! webui / CLI 形态的 WorkBuddy 网关宿主（P5 §2.4 泛化）。
//!
//! - [`shared_state`] 提供一份进程内共享的 [`GatewayState`]，供 `api.rs` 的
//!   `merge(gateway::router(state))` 与网关管理路由复用（配置 / Key / 日志一致）。
//! - [`apply`] / [`status`] 管理独立监听的生命周期（默认 `127.0.0.1:57891`）。
//!
//! 生命周期逻辑（启动 / 停止 / 状态）由网关 crate 的泛型
//! [`GatewayHost<WbGateway>`](buddy_switch_gateway::GatewayHost) 提供；本模块只负责
//! **进程内单例**与 **WorkBuddy 产品线的接线**（路径所有权见 [`buddy_switch_gateway::WbGateway`]）。
//!
//! ## ⚠️ 为什么不把 Trae 网关 merge 进本模块
//!
//! `buddy_switch_gateway::trae::router()` 也挂 `/v1/models` 与 `/v1/chat/completions`，
//! 与 WorkBuddy 网关路径**逐字相同**。axum 的 `merge` 遇到同路径会直接 **panic**
//! （**运行期崩**），故 Trae 网关只能走**自己的端口**（7864）——
//! 见 [`crate::trae_gateway_host`]。泛化**不改变**端口分离。

use std::sync::OnceLock;

use tokio::sync::Mutex;

use buddy_switch_gateway::{GatewayHost, GatewayProduct, GatewayState, WbGateway};

static HOST: OnceLock<Mutex<GatewayHost<WbGateway>>> = OnceLock::new();

/// 进程内共享的网关状态（惰性初始化，读一次配置）。
///
/// 独立监听与 `api.rs` 的管理面复用**同一份**状态，因此监听 57891 上的真实请求日志与
/// `/v1/models` 目录刷新结果都能在管理页看到。
pub fn shared_state() -> GatewayState {
    <WbGateway as GatewayProduct>::shared_state()
}

fn host() -> &'static Mutex<GatewayHost<WbGateway>> {
    HOST.get_or_init(|| Mutex::new(GatewayHost::new()))
}

/// 按当前配置应用（启动 / 重启）独立监听。
///
/// 以共享状态内的配置为准（管理面保存配置时会同步写回该状态）；配置为 disabled
/// 等价于停止。
pub async fn apply() -> Result<Option<String>, String> {
    host().lock().await.start().await
}

/// 查询运行状态（运行中, 监听地址）。
pub async fn status() -> (bool, Option<String>) {
    let host = host().lock().await;
    (host.is_running(), host.addr())
}
