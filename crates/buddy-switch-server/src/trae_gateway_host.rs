//! webui / CLI 形态的 **Trae** 网关宿主（P5 §2.4 泛化；默认 `127.0.0.1:7864`）。
//!
//! 与 [`crate::gateway_host`] 平行、**各持一个泛型 [`GatewayHost`]**（原因见
//! `buddy_switch_gateway::trae` 模块文档与 [`buddy_switch_gateway::GatewayProduct`]）。
//!
//! ## 为什么不把 Trae 路由 merge 进 webui 的 API Router
//!
//! `buddy_switch_gateway::trae::router()` 也挂 `/v1/models` 与 `/v1/chat/completions`，
//! 与 WorkBuddy 网关的路径**逐字相同**。axum 的 `merge` 遇到同路径会直接 panic，
//! 所以 Trae 网关只能走**自己的端口**；webui 侧只暴露 `/api/trae/gateway/*` 管理面。
//!
//! 这不是缺陷而是刻意设计：两个网关的上游、凭据、错误语义完全不同，混在一个端口上
//! 还得靠路径前缀区分，客户端配置会变得难以理解。

use std::sync::OnceLock;

use tokio::sync::Mutex;

use buddy_switch_gateway::trae::{TraeGateway, TraeGatewayState};
use buddy_switch_gateway::{GatewayHost, GatewayProduct};

static HOST: OnceLock<Mutex<GatewayHost<TraeGateway>>> = OnceLock::new();

/// 进程内共享的 Trae 网关状态（惰性初始化，读一次配置）。
pub fn shared_state() -> TraeGatewayState {
    <TraeGateway as GatewayProduct>::shared_state()
}

fn host() -> &'static Mutex<GatewayHost<TraeGateway>> {
    HOST.get_or_init(|| Mutex::new(GatewayHost::new()))
}

/// 按当前配置应用（启动 / 重启）独立监听。
pub async fn apply() -> Result<Option<String>, String> {
    host().lock().await.start().await
}

/// 查询运行状态（运行中, 监听地址）。
pub async fn status() -> (bool, Option<String>) {
    let host = host().lock().await;
    (host.is_running(), host.addr())
}
