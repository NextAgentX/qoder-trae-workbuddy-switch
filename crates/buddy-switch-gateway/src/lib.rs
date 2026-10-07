//! `buddy-switch-gateway`：WorkBuddy 模型额度的 OpenAI / Anthropic 兼容网关。
//!
//! 本 crate 是网关本体，落在独立 crate（不污染 `buddy-switch-core` 的依赖面），
//! 可被两种宿主同时复用：
//! - `buddy-switch-server`：webui/CLI 形态，`api.rs` 内 `merge(router(state))`，
//!   并可用 [`spawn_listener`] 起独立监听（默认 `127.0.0.1:57891`）。
//! - `src-tauri`：桌面宿主，`gateway.rs` 持有 [`GatewayHandle`]。
//!
//! 对外契约（已冻结，不得更改签名）：
//! ```ignore
//! pub fn router(state: GatewayState) -> axum::Router;
//! pub async fn spawn_listener(config: GatewayConfig) -> anyhow::Result<GatewayHandle>;
//! ```
//!
//! 关键约束：
//! - `router()` **不设 fallback**，避免与宿主 merge 时的 fallback 冲突 panic。
//! - SSE 透传用 `Body::from_stream`；客户端断开即 drop stream → abort 上游。
//! - 上游流中途异常时**补发 `data: [DONE]`**，绝不悬挂连接。

pub mod account_strategy;
pub mod apikey;
/// 「接入即识别」与「网关启动补识别」的**唯一探测出口**（P10 §1.6 / T03）。
pub mod catalog_probe;
pub mod credits_refresh;
/// 模型禁用偏好（P11）：两个出口（`/v1/models` + 调用）共用的唯一判断点。
pub mod disabled_models;
pub mod error;
pub mod logging;
pub mod model_route;
pub mod outbound;
pub mod pool;
pub mod protocol;
pub mod rng;
pub mod routes;
pub mod session_headers;
pub mod state;
pub mod sticky;
pub mod timeutil;
/// Trae 的 OpenAI 兼容网关（**平行第二套实现**，与上面的 WorkBuddy 网关互不复用）。
///
/// 独立模块而非 `GatewayState` 的一个分支：两者归属域、凭据形态、上游协议、
/// 响应格式、路由挂载方式全不相同，详见 [`trae`] 模块文档。
pub mod trae;

pub use account_strategy::{AccountSelector, AccountStrategy};
pub use apikey::{ApiKeyRecord, ApiKeyStore};
pub use disabled_models::{is_disabled, is_disabled_in, DisabledModels};
pub use error::GatewayError;
pub use outbound::{OutboundMeta, OutboundOptions};
pub use rng::Pcg32;
pub use state::{
    body_limit_bytes, sync_pool_with_accounts, GatewayConfig, GatewayState, GatewayStatusView,
    DEFAULT_MAX_BODY_MB,
};
pub use sticky::StickyTable;

use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;

use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// 组装网关对外路由（**不含 fallback**）。
///
/// 挂载：`GET /healthz`、`GET /v1/models`、`GET /status`、
/// `POST /v1/chat/completions`、`POST /v1/messages`。
/// 鉴权在各自 handler 内完成（Key 绑定 region）；`/healthz` 免鉴权。
pub fn router(state: GatewayState) -> axum::Router {
    use axum::routing::{get, post};
    // 请求体上限：axum 的 `DefaultBodyLimit` **默认只有 2MB**，大上下文（长代码文件、
    // 长对话）会撞上一个裸 413。它在 **router 构建期**固化，因此改配置需重启网关
    // （见 `GatewayConfig::max_body_mb`）。
    let body_limit = state.body_limit_bytes;
    axum::Router::new()
        .route("/healthz", get(routes::health::handler))
        .route("/v1/models", get(routes::models::handler))
        .route("/status", get(routes::status::handler))
        .route("/v1/chat/completions", post(routes::chat::handler))
        .route("/v1/messages", post(routes::messages::handler))
        // P12 T03 · **业务请求转发**（本地路由）：白名单内的业务路径走 `relay` 的治理层
        // （选号 / 在途 / 冷却 / 熔断 / 换号），**不另写轮换逻辑**。白名单外一律 404。
        .route("/biz/{*path}", post(routes::biz::handler))
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .with_state(state)
    // 注意：不设 fallback，避免与宿主 merge 时的 fallback 冲突
}

/// 已启动的网关监听句柄。
///
/// Drop 该句柄（不调用 [`GatewayHandle::shutdown`]）会触发优雅停机：
/// 内部 oneshot sender 被丢弃 → 优雅关闭 future 完成。
pub struct GatewayHandle {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl GatewayHandle {
    /// 由监听方直接构造句柄。
    ///
    /// 字段是私有的，因此**平级模块**（如 `trae::spawn_listener`）无法用结构体字面量
    /// 构造。与其把字段放宽成 `pub(crate)`（那会让「谁都能改 addr」成为可能），
    /// 不如只开一个受控构造器：语义与 [`spawn_listener_with_state`] 内部那处完全一致。
    pub fn new(
        addr: SocketAddr,
        shutdown: oneshot::Sender<()>,
        join: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self {
            addr,
            shutdown: Some(shutdown),
            join: Some(join),
        }
    }

    /// 实际监听地址（端口为 0 时返回系统分配的端口）。
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// 优雅停机：通知 serve 循环退出并等待任务结束。
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

/// 按配置启动独立监听（**自建一份** [`GatewayState`]）。
///
/// 向后兼容的旧签名：内部转调 [`spawn_listener_with_state`]。宿主若希望独立监听与
/// 管理面共享同一份状态（请求日志 / Key / 目录缓存），应改用 [`spawn_listener_with_state`]。
///
/// - 默认只监听回环地址；非回环地址需要 `allow_non_loopback = true`。
/// - 端口占用/地址非法时返回**可读错误**（不是 panic）。
pub async fn spawn_listener(config: GatewayConfig) -> anyhow::Result<GatewayHandle> {
    spawn_listener_with_state(GatewayState::new(config)).await
}

/// 用**调用方提供的** [`GatewayState`] 启动独立监听。
///
/// 关键：`state` 是可 `Arc` 共享的句柄（配置 / Key 库 / 目录缓存 / 请求日志均为 `Arc`），
/// 因此独立监听与宿主管理面**看到的是同一份**请求日志与模型目录刷新结果——否则
/// 独立监听会把请求写进它自己的日志，管理页恒为空。
///
/// 监听地址 / 端口 / 是否允许非回环均取自 `state` 内的配置快照。
pub async fn spawn_listener_with_state(state: GatewayState) -> anyhow::Result<GatewayHandle> {
    let config = state.config_snapshot().await;
    let bind_addr = config.bind_addr.clone();
    let port = config.port;

    let ip: IpAddr = bind_addr
        .parse()
        .map_err(|_| anyhow::anyhow!("网关监听地址无效：{bind_addr}（应为 IPv4/IPv6 字面量）"))?;

    if !ip.is_loopback() && !config.allow_non_loopback {
        return Err(anyhow::anyhow!(
            "已拒绝监听非回环地址 {bind_addr}：局域网内任何设备都可消耗你的额度，\
             请确认可信网络后，在设置中显式开启『允许局域网访问』再重试"
        ));
    }

    let addr = SocketAddr::new(ip, port);
    let listener = TcpListener::bind(addr).await.map_err(|error| {
        anyhow::anyhow!("无法监听 {addr}：端口可能被占用或不可用（{error}）。请在设置中改用其他端口。")
    })?;
    let local = listener.local_addr().unwrap_or(addr);

    let app = router(state);

    let (tx, rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });

    Ok(GatewayHandle {
        addr: local,
        shutdown: Some(tx),
        join: Some(join),
    })
}

// ---------------------------------------------------------------------------
// 网关宿主泛化（P5 §2.4）—— `GatewayProduct` trait + 泛型 `GatewayHost<P>`
// ---------------------------------------------------------------------------

/// 一条网关产品线 —— **路径所有权**声明 + 生命周期接入点（P5 §2.4）。
///
/// ## ⚠️ 为什么**不能**把两个平台的 router 合并成一个结构体
///
/// axum 对**同一路径** `merge` 两个 router 会 **panic**（**运行期崩，不是编译期**）。
/// 而 WorkBuddy 网关（[`router`]）与 Trae 网关（[`trae::router`]）都挂
/// `/v1/models` 与 `/v1/chat/completions`，路径**逐字相同** ⇒ 合并即在启动时 panic。
///
/// 故本 trait 的 [`GatewayProduct::router`] 由**各平台 impl 各自声明自己拥有的路径**
/// （[`GatewayProduct::OWNED_PATHS`]）；泛型层（[`GatewayHost`]）**只管理生命周期**，
/// **绝不**持有 / merge 任何 router。两个平台各自**独立端口**
/// （WorkBuddy 57891 / Trae 7864）—— 泛化**不改变端口分离**。
///
/// 该约束的可证伪性由 `tests::two_products_own_overlapping_paths` 守住：
/// 两平台的 [`GatewayProduct::OWNED_PATHS`] **必须**存在交集（否则本约束的前提已变，
/// 应复核是否还需要端口分离）。
pub trait GatewayProduct: Send + Sync + 'static {
    /// 该平台的运行状态（可 `Arc` 共享）。
    type State: Clone + Send + Sync + 'static;

    /// 本平台**拥有的路由路径**（路径所有权的显式声明）。
    ///
    /// ⚠️ 这不是装饰：两平台的路径**逐字重叠**，泛型层据此**拒绝**合并两个 router。
    const OWNED_PATHS: &'static [&'static str];

    /// 该平台的**进程内唯一**共享状态。
    ///
    /// 两宿主（`src-tauri` / `buddy-switch-server`）共用本函数 ⇒ 同一进程内
    /// 「独立监听」与「管理面」看到的是**同一份**请求日志 / Key 库 / 目录缓存。
    fn shared_state() -> Self::State;

    /// 独立监听是否启用（读共享状态的配置快照）。
    fn is_enabled(state: &Self::State) -> impl std::future::Future<Output = bool> + Send;

    /// 组装该平台的路由 —— **路径所有权由本 impl 声明**（见 [`GatewayProduct::OWNED_PATHS`]）。
    fn router(state: Self::State) -> axum::Router;

    /// 用给定状态启动独立监听。
    fn spawn_listener(
        state: Self::State,
    ) -> impl std::future::Future<Output = anyhow::Result<GatewayHandle>> + Send;
}

/// 泛型网关宿主：管理**一个**平台独立监听的生命周期（P5 §2.4）。
///
/// ## 为什么是泛型而不是「一个结构体装两个平台」
///
/// 见 [`GatewayProduct`] 的说明：axum 同路径 `merge` 会 panic，而两平台路径逐字重叠。
/// 故这里**一个 `GatewayHost<P>` 只服务一个平台**，两平台各持一个实例、各自端口。
///
/// ## 与既有 `std` / `tokio` 锁的分工
///
/// - **server 侧**（`buddy-switch-server`）持 `tokio::sync::Mutex`，可直接
///   `lock().await.start().await`（[`GatewayHost::start`] 取 `&mut self`）。
/// - **tauri 侧**持 `std::sync::Mutex`（其 `gateway_status` 是**同步命令**，需同步读
///   `is_running` / `addr`）；为**不跨 `await` 持有 std 锁**，tauri 侧用
///   [`GatewayHost::take_handle`] / [`GatewayHost::set_handle`] 在**锁外** `await`。
pub struct GatewayHost<P: GatewayProduct> {
    handle: Option<GatewayHandle>,
    /// `fn() -> P` 而非 `P`：不因 `P` 的自动 trait（`Send` / `Sync`）而改变本结构的
    /// 自动 trait —— 泛型宿主只承载句柄，不持有产品线实例。
    _product: PhantomData<fn() -> P>,
}

impl<P: GatewayProduct> Default for GatewayHost<P> {
    fn default() -> Self {
        Self {
            handle: None,
            _product: PhantomData,
        }
    }
}

impl<P: GatewayProduct> GatewayHost<P> {
    /// 新建（未启动）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 取走当前句柄（供**持 `std` 锁**的宿主在**锁外** `await` 关闭 —— 见结构体文档）。
    ///
    /// `None` = 当前未运行。
    pub fn take_handle(&mut self) -> Option<GatewayHandle> {
        self.handle.take()
    }

    /// 设置新句柄（与 [`GatewayHost::take_handle`] 配对）。
    pub fn set_handle(&mut self, handle: GatewayHandle) {
        self.handle = Some(handle);
    }

    /// 是否运行中。
    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// 运行中的监听地址。
    pub fn addr(&self) -> Option<String> {
        self.handle.as_ref().map(|handle| handle.addr().to_string())
    }

    /// 按当前配置启动 / 重启独立监听。返回实际监听地址；未启用时返回 `Ok(None)`。
    ///
    /// 先停旧监听（`self.stop().await`）再起新的，故「配置改为 disabled」等价于停止。
    pub async fn start(&mut self) -> Result<Option<String>, String> {
        self.stop().await;
        let state = P::shared_state();
        if !P::is_enabled(&state).await {
            return Ok(None);
        }
        let handle = P::spawn_listener(state)
            .await
            .map_err(|error| error.to_string())?;
        let addr = handle.addr().to_string();
        self.set_handle(handle);
        Ok(Some(addr))
    }

    /// 停止独立监听。
    pub async fn stop(&mut self) {
        if let Some(handle) = self.take_handle() {
            handle.shutdown().await;
        }
    }
}

/// WorkBuddy 网关产品线（[`GatewayProduct`] 的 WB 实现）。
///
/// **路径所有权**：`GET /healthz`、`GET /v1/models`、`GET /status`、
/// `POST /v1/chat/completions`、`POST /v1/messages`（见 [`router`]）。
pub struct WbGateway;

/// WorkBuddy 网关的进程内共享状态（惰性初始化，读一次配置）。
static WB_SHARED_STATE: OnceLock<GatewayState> = OnceLock::new();

impl GatewayProduct for WbGateway {
    type State = GatewayState;

    const OWNED_PATHS: &'static [&'static str] = &[
        "/healthz",
        "/v1/models",
        "/status",
        "/v1/chat/completions",
        "/v1/messages",
        // P12 T03 · 业务转发 —— **WB 独有**（Trae 业务域不可配，见 spike 的 H0）。
        // ⚠️ 写**精确的 axum 路径**（本表是「路径所有权」声明，不是前缀表）。
        "/biz/{*path}",
    ];

    fn shared_state() -> GatewayState {
        WB_SHARED_STATE
            .get_or_init(|| GatewayState::new(GatewayConfig::load()))
            .clone()
    }

    fn is_enabled(state: &GatewayState) -> impl std::future::Future<Output = bool> + Send {
        // 克隆一份（`GatewayState` 是 Arc 句柄，克隆极廉价）使返回的 future 不借 `state`。
        let state = state.clone();
        async move { state.config_snapshot().await.enabled }
    }

    fn router(state: GatewayState) -> axum::Router {
        // `self::` 明确指向本模块的自由函数（与 trait 方法同名，不加前缀会歧义）。
        self::router(state)
    }

    fn spawn_listener(
        state: GatewayState,
    ) -> impl std::future::Future<Output = anyhow::Result<GatewayHandle>> + Send {
        self::spawn_listener_with_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// ★★ 护栏：两平台的**路径所有权必须重叠** —— 这正是「不能 merge」的依据。
    ///
    /// 可证伪：若两平台路径不再重叠（例如 Trae 改挂到别的前缀），本用例变红 ⇒ 提醒复核
    /// 「是否还需要端口分离 / 是否已可安全合并」。在重叠存在的前提下，把两个 router
    /// 合并成一个结构体会在**运行期 panic**（axum 同路径 merge）—— 这是**刻意规避**的坑。
    #[test]
    fn two_products_own_overlapping_paths() {
        let wb: BTreeSet<&str> = WbGateway::OWNED_PATHS.iter().copied().collect();
        let trae: BTreeSet<&str> = trae::TraeGateway::OWNED_PATHS.iter().copied().collect();
        let overlap: Vec<&&str> = wb.intersection(&trae).collect();
        assert!(
            !overlap.is_empty(),
            "两平台路径若无重叠，『必须各持 router + 各自端口』的前提已变，请复核 P5 §2.4"
        );
        // 冲突点必须是这两条（axum 会对它们报 overlapping method route）。
        assert!(overlap.contains(&&"/v1/models"), "{overlap:?}");
        assert!(overlap.contains(&&"/v1/chat/completions"), "{overlap:?}");
    }

    /// P12 T03：业务转发路由**必须**在册，且**真的挂在 `router()` 里**。
    ///
    /// 可证伪：删掉 `router()` 里的 `.route("/biz/{*path}", …)` ⇒ 本用例变红。
    /// ⚠️ **只看 `router()` 的函数体** —— 若直接扫整个文件，本测试里那行字符串会让断言
    /// **恒真**（典型的假断言：把被测逻辑删掉它也绿）。
    #[test]
    fn business_route_is_owned_and_mounted() {
        assert!(
            WbGateway::OWNED_PATHS.contains(&"/biz/{*path}"),
            "业务转发路径未声明所有权"
        );
        let src = include_str!("lib.rs");
        // ⚠️ 锚**必须用完整签名**（含大括号）：模块文档注释里也有一行
        //    `//! pub fn router(state: GatewayState) -> axum::Router;`（**分号**结尾），
        //    用 `split("pub fn router(")` 会取到「注释之后到真函数之前」那段 —— **不含路由**，
        //    断言因此**恒红**（首版就栽在这）。
        let body = src
            .split("pub fn router(state: GatewayState) -> axum::Router {")
            .nth(1)
            .and_then(|rest| rest.split("/// 已启动的网关监听句柄").next())
            .expect("找不到 router 函数体（结构已变，请更新本断言）");
        assert!(
            body.contains(r#".route("/biz/{*path}", post(routes::biz::handler))"#),
            "router() 里没挂业务转发路由 —— handler 写了但没人调（本项目已出过三次同类事故）"
        );
    }

    /// 两平台各自的路径所有权**互不相同**（各挂自己的健康检查 / 状态路径），
    /// 且**各自非空**。
    #[test]
    fn each_product_declares_a_nonempty_distinct_path_set() {
        for paths in [WbGateway::OWNED_PATHS, trae::TraeGateway::OWNED_PATHS] {
            assert!(!paths.is_empty(), "路径所有权不得为空");
        }
        // WB 有 `/healthz`，Trae 有 `/health`：各自独有。
        assert!(WbGateway::OWNED_PATHS.contains(&"/healthz"));
        assert!(trae::TraeGateway::OWNED_PATHS.contains(&"/health"));
    }
}
