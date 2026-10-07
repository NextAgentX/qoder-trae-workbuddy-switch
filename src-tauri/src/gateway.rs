//! 桌面宿主（Tauri）的 WorkBuddy 网关接入（P5 §2.4 泛化）。
//!
//! - [`shared_state`]：进程内共享的 [`GatewayState`]（配置 / Key / 日志一致）。
//! - [`GatewayRuntime`]：由 `app.manage(...)` 托管，负责独立监听生命周期。
//!   独立监听与命令层复用**同一份** [`shared_state`]（E4），因此桌面端发出到
//!   57891 的真实请求日志 / 目录刷新结果都能在管理页看到。
//!
//! 生命周期逻辑由网关 crate 的泛型
//! [`GatewayHost<WbGateway>`](buddy_switch_gateway::GatewayHost) 提供；本模块只负责
//! Tauri 侧的托管与锁策略（见 [`GatewayRuntime`]）。
//!
//! ⚠️ 与 [`crate::trae_gateway`] **各持一个 `GatewayHost`**、各自端口
//! （WorkBuddy 57891 / Trae 7864）—— **绝不**把两个平台的路由合并（axum 同路径 merge
//! 会 panic，见 [`buddy_switch_gateway::GatewayProduct`]）。

use std::sync::Mutex;

use buddy_switch_gateway::{GatewayHost, GatewayProduct, GatewayState, WbGateway};

/// 进程内共享的网关状态（惰性初始化，读一次配置）。
pub fn shared_state() -> GatewayState {
    <WbGateway as GatewayProduct>::shared_state()
}

/// 独立监听生命周期（Tauri managed state）。
///
/// ## 为什么用 `std::sync::Mutex`（而非 `tokio::sync::Mutex`）
///
/// `gateway_status` 是**同步命令**，需要**同步**读 [`GatewayRuntime::is_running`] /
/// [`GatewayRuntime::addr`]。为**不跨 `await` 持有 std 锁**，`apply` 先把
/// [`GatewayHost`] **整体取出**（`mem::take`），在锁外 `await` 之后再放回。
pub struct GatewayRuntime {
    host: Mutex<GatewayHost<WbGateway>>,
}

impl Default for GatewayRuntime {
    fn default() -> Self {
        Self {
            host: Mutex::new(GatewayHost::new()),
        }
    }
}

impl GatewayRuntime {
    /// 新建（未启动）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按当前配置启动 / 重启独立监听。返回实际监听地址；未启用时返回 `None`。
    ///
    /// **共享状态（E4）**：用 [`shared_state`] 起监听，使独立监听与管理面共享同一份
    /// 请求日志 / Key 库 / 目录缓存（`save_gateway_config` 会把配置同步写回该状态）。
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
    fn take_host(&self) -> GatewayHost<WbGateway> {
        std::mem::take(&mut *self.host.lock().unwrap())
    }

    /// 放回宿主（与 [`GatewayRuntime::take_host`] 配对）。
    fn put_host(&self, host: GatewayHost<WbGateway>) {
        *self.host.lock().unwrap() = host;
    }
}
