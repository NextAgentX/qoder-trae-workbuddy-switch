//! 桌面宿主（Tauri）的 **Trae** 网关接入（P5 §2.4 泛化）：独立监听句柄 + 进程内共享状态。
//!
//! ## 为什么不复用 [`crate::gateway`]
//!
//! 两者持有的**状态类型**完全不同：WorkBuddy 侧是 `GatewayState`（Key 库 / region /
//! 目录缓存），Trae 侧是 `TraeGatewayState`（**按产品线分家的账号池** / **多 Key 库
//! （含归属变体）** / 独立日志文件）。共用一个 `OnceLock` 只会得到「两个 Option 字段里
//! 总有一个是 None」的结构。
//!
//! 泛化后两者**共用**网关 crate 的泛型 [`GatewayHost`](buddy_switch_gateway::GatewayHost)，
//! 只是产品线参数不同（[`WbGateway`](buddy_switch_gateway::WbGateway) vs
//! [`TraeGateway`](buddy_switch_gateway::trae::TraeGateway)）—— 这正是「改 Trae 网关
//! 不会碰到 WorkBuddy 网关」与「生命周期不重复写两遍」的同时达成。
//!
//! ⚠️ 两个平台各持一个 router、各自端口（Trae 7864 / WorkBuddy 57891），**绝不 merge**
//! （axum 同路径 merge 会 panic）。这正是 `buddy_switch_gateway::trae` 的整体设计取向。

use std::sync::Mutex;

use buddy_switch_gateway::trae::{TraeGateway, TraeGatewayState};
use buddy_switch_gateway::{GatewayHost, GatewayProduct};

/// 进程内共享的 Trae 网关状态（惰性初始化，读一次配置）。
///
/// 独立监听与管理命令复用**同一份**状态，因此「API 服务」页看到的请求日志、
/// 账号池与真实打进来的请求完全一致。
pub fn shared_state() -> TraeGatewayState {
    <TraeGateway as GatewayProduct>::shared_state()
}

/// 独立监听生命周期（Tauri managed state）。
///
/// 锁策略与 [`crate::gateway::GatewayRuntime`] 同款（`std::sync::Mutex` + 锁外 `await`，
/// 因 `trae_gateway_status` 需同步读运行态）。
pub struct TraeGatewayRuntime {
    host: Mutex<GatewayHost<TraeGateway>>,
}

impl Default for TraeGatewayRuntime {
    fn default() -> Self {
        Self {
            host: Mutex::new(GatewayHost::new()),
        }
    }
}

impl TraeGatewayRuntime {
    /// 新建（未启动）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按当前配置启动 / 重启独立监听。返回实际监听地址；未启用时返回 `None`。
    pub async fn apply(&self) -> Result<Option<String>, String> {
        let mut host = self.take_host();
        let result = host.start().await;
        self.put_host(host);
        result
    }

    /// 是否运行中。
    pub fn is_running(&self) -> bool {
        self.host.lock().unwrap().is_running()
    }

    /// 运行中的监听地址。
    pub fn addr(&self) -> Option<String> {
        self.host.lock().unwrap().addr()
    }

    /// 取出宿主（在**锁外** `await` 用；[`GatewayHost`] 实现了 `Default`）。
    fn take_host(&self) -> GatewayHost<TraeGateway> {
        std::mem::take(&mut *self.host.lock().unwrap())
    }

    /// 放回宿主（与 [`TraeGatewayRuntime::take_host`] 配对）。
    fn put_host(&self, host: GatewayHost<TraeGateway>) {
        *self.host.lock().unwrap() = host;
    }
}
