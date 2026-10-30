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
/// 账号故障自动切换（issue #129）：流中途失败 → 客户端可重试错误的**唯一**形状出口。
pub mod failover;
pub mod key_usage;
pub mod logging;
/// 模型能力声明（图片输入，issue #101）：三条产品线出口 ① 与管理面共用的唯一解析点。
pub mod model_capability;
/// 模型降级链（主模型明确失败时改用备选模型重发一次；见模块文档）。
pub mod model_fallback;
/// 账号池 × 模型限流探测（一次性作业；见模块文档）。
pub mod model_probe;
pub mod model_route;
pub mod outbound;
pub mod pool;
pub mod protocol;
// E1 · 代理 KPI 的真实计数（今日代理请求 / 今日换号；落盘，跨重启不丢）。
pub mod proxy_kpi;
/// Qoder 的 OpenAI / Anthropic 兼容网关（**平行第三套实现**，独立端口 57895）。
pub mod qoder;
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
// 统一网关（第三个产品线）：把两个产品线的模型合成一个入口。
// ⚠️ 它是**独立端口**上的**第三个** `GatewayProduct` —— 永不与另两个 router merge
// （axum 同路径 merge 会运行期 panic）。详见模块头与 `docs/specs/UNIFIED-GATEWAY-ARCH.md`。
pub mod unified;

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

/// 健康检查的**两个等价路径**：canonical [`HEALTH_CANONICAL_PATH`]，另一个是兼容别名。
///
/// ## 为什么改名为 `/health` 又保留 `/healthz`
///
/// `-z` 后缀是 k8s / Google 的探针命名习惯（用来和「名为 health 的资源」区分）；本仓四个
/// 网关都是**固定路由的独立端口**，不存在这种冲突 ⇒ 这个后缀换不到任何东西，只换来
/// 「为什么不是 health」这一个反复被问的问题。故 canonical 改为 `/health`。
///
/// 但 `/healthz` 已经随 WB / Qoder / 统一入口发出去过，用户的探活脚本里可能写死它 ——
/// 直接删属于没必要的破坏。**两个都留、只宣传 `/health`**：文档、示例、验收脚本一律用
/// `/health`；`/healthz` 作为兼容别名长期保留（成本是一条路由，收益是不打断任何人）。
///
/// ⚠️ 不要在别处再手写这两个路径：挂载点唯一（[`health_routes`]），
/// 否则「都支持」迟早退化成「四条线有的挂了一个、有的挂了两个」。
pub const HEALTH_CANONICAL_PATH: &str = "/health";

/// 健康检查的兼容别名（旧路径，见 [`HEALTH_CANONICAL_PATH`] 的说明）。
pub const HEALTH_LEGACY_PATH: &str = "/healthz";

/// 健康检查的全部路径（canonical 在前）。
pub const HEALTH_PATHS: [&str; 2] = [HEALTH_CANONICAL_PATH, HEALTH_LEGACY_PATH];

/// 把**同一个**健康检查处理器挂到 [`HEALTH_PATHS`] 的每一个路径上。
///
/// 单一挂载点：四个网关都经本函数取路由 ⇒ 「两个路径等价」是结构保证，不靠自觉。
/// 泛型只为让各网关的 state 类型（`GatewayState` / `TraeGatewayState` / …）各自成立。
pub fn health_routes<S, H, T>(handler: H) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
    H: axum::handler::Handler<T, S> + Clone,
    T: 'static,
{
    use axum::routing::get;
    let mut router = axum::Router::new();
    for path in HEALTH_PATHS {
        router = router.route(path, get(handler.clone()));
    }
    router
}

/// 组装网关对外路由（**不含 fallback**）。
///
/// 挂载：`GET /health`（兼容 `/healthz`，见 [`HEALTH_PATHS`]）、`GET /v1/models`、
/// `GET /status`、`POST /v1/chat/completions`、`POST /v1/messages`、
/// `POST /v1/responses`。
/// 鉴权在各自 handler 内完成（Key 绑定 region）；健康检查免鉴权。
pub fn router(state: GatewayState) -> axum::Router {
    use axum::routing::{get, post};
    // 请求体上限：axum 的 `DefaultBodyLimit` **默认只有 2MB**，大上下文（长代码文件、
    // 长对话）会撞上一个裸 413。它在 **router 构建期**固化，因此改配置需重启网关
    // （见 `GatewayConfig::max_body_mb`）。
    let body_limit = state.body_limit_bytes;
    axum::Router::new()
        .merge(health_routes(routes::health::handler))
        .route("/v1/models", get(routes::models::handler))
        .route("/status", get(routes::status::handler))
        .route("/v1/chat/completions", post(routes::chat::handler))
        .route("/v1/messages", post(routes::messages::handler))
        // Responses API（issue #21）：与 chat/messages 共用同一条中继，只差协议层。
        .route("/v1/responses", post(routes::responses::handler))
        // P12 T03 · **业务请求转发**（本地路由）：白名单内的业务路径走 `relay` 的治理层
        // （选号 / 在途 / 冷却 / 熔断 / 换号），**不另写轮换逻辑**。白名单外一律 404。
        //
        // ⚠️ 通配符**必须**用 axum 0.7 的 `*path`，**不是** 0.8 的 `{*path}`：
        //   0.7 的 path-router 会把 `{*path}` 判为非法 catch-all，在**构建期 panic**
        //   （报错信息 "catch-all parameters are only allowed at the end of a route"
        //   具有误导性，真因是花括号不被识别）⇒ 整个网关起不来，且 `cargo check` 全绿。
        // ★ 业务转发**同时接 GET 与 POST**：白名单按前缀收，同前缀下 GET-only 与
        //   POST-only 混在一起（`/activity/growth/streak` 是 GET、`/lottery/draw` 是 POST）。
        //   只挂 POST 会让 GET-only 的路径经网关报 405 / 上游 404（真实环境实测）。
        .route(
            "/biz/*path",
            get(routes::biz::handler).post(routes::biz::handler),
        )
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
        anyhow::anyhow!(
            "无法监听 {addr}：端口可能被占用或不可用（{error}）。请在设置中改用其他端口。"
        )
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
/// （WorkBuddy 57891 / Trae 57894）—— 泛化**不改变端口分离**。
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
/// **路径所有权**：`GET /health` + `/healthz`（见 [`HEALTH_PATHS`]）、`GET /v1/models`、
/// `GET /status`、`POST /v1/chat/completions`、`POST /v1/messages`、
/// `POST /v1/responses`（见 [`router`]）。
pub struct WbGateway;

/// WorkBuddy 网关的进程内共享状态（惰性初始化，读一次配置）。
static WB_SHARED_STATE: OnceLock<GatewayState> = OnceLock::new();

impl GatewayProduct for WbGateway {
    type State = GatewayState;

    const OWNED_PATHS: &'static [&'static str] = &[
        "/health",
        "/healthz",
        "/v1/models",
        "/status",
        "/v1/chat/completions",
        "/v1/messages",
        // issue #21：OpenAI Responses API —— 与 chat/messages 共用中继，路径独立。
        "/v1/responses",
        // P12 T03 · 业务转发 —— **WB 独有**（Trae 业务域不可配，见 spike 的 H0）。
        // ⚠️ 写**精确的 axum 路径**（本表是「路径所有权」声明，不是前缀表）。
        // ⚠️ 通配符用 axum 0.7 语法 `*path`（**不是** 0.8 的 `{*path}`）。
        "/biz/*path",
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

    /// ★★ 第三个产品线（统一网关）**也**与两者重叠 ⇒ 它同样**必须**独立端口。
    ///
    /// 与 [`two_products_own_overlapping_paths`] 互补：那条钉「wb 与 trae 重叠」，
    /// 本条钉「统一网关也与两者重叠」—— 三个 `OWNED_PATHS` **两两相交**，
    /// ⇒ **任意两个 router merge 都会运行期 panic**，故必须三个端口。
    ///
    /// 可证伪：若哪天统一网关改成挂在自己的前缀下（不再重叠），本用例变红 ——
    /// 那是提醒「可以复核是否还需要第三个端口」，不是缺陷。
    #[test]
    fn unified_gateway_also_overlaps_both_products() {
        let unified: BTreeSet<&str> = crate::unified::UnifiedGateway::OWNED_PATHS
            .iter()
            .copied()
            .collect();
        let wb: BTreeSet<&str> = WbGateway::OWNED_PATHS.iter().copied().collect();
        let trae: BTreeSet<&str> = trae::TraeGateway::OWNED_PATHS.iter().copied().collect();

        assert!(
            !unified.intersection(&wb).collect::<Vec<_>>().is_empty(),
            "统一网关与 WB 网关必须存在路径重叠（否则端口分离的前提已变）"
        );
        assert!(
            !unified.intersection(&trae).collect::<Vec<_>>().is_empty(),
            "统一网关与 Trae 网关必须存在路径重叠"
        );
        // 重叠点必须含 `/v1/models` —— 它正是「同名模型能并列」的对外出口
        assert!(
            unified.contains(&"/v1/models"),
            "统一网关必须拥有 /v1/models（聚合清单是它的核心职责）"
        );
        // 三个产品线各占一个端口
        assert_eq!(
            [
                WbGateway::OWNED_PATHS.len(),
                trae::TraeGateway::OWNED_PATHS.len(),
                crate::unified::UnifiedGateway::OWNED_PATHS.len(),
            ]
            .iter()
            .filter(|len| **len > 0)
            .count(),
            3,
            "三条产品线的路径所有权表都不该为空"
        );
    }

    /// P12 T03：业务转发路由**必须**在册，且**真的挂在 `router()` 里**。
    ///
    /// 可证伪：删掉 `router()` 里的 `.route("/biz/*path", …)` ⇒ 本用例变红。
    /// ⚠️ **只看 `router()` 的函数体** —— 若直接扫整个文件，本测试里那行字符串会让断言
    /// **恒真**（典型的假断言：把被测逻辑删掉它也绿）。
    ///
    /// ★★ 本用例**曾经放过一个致命 bug**：路由一度写成 axum **0.8** 的 `/biz/{*path}`，
    /// 而本仓锁的是 **axum 0.7** ⇒ `router()` 在**构建期panic**，网关完全起不来，
    /// 但 `cargo check` 全绿、上面那条字符串断言也全绿（它只查「字符串在不在」，
    /// 管不了「axum 认不认这个语法」）。
    /// ⇒ 断言必须**升级为真的构建 router**（下面 [`router_builds_and_biz_route_matches`]
    /// 承担），字符串检查只保留为「声明与实现不脱节」的辅助。
    #[test]
    fn business_route_is_owned_and_mounted() {
        assert!(
            WbGateway::OWNED_PATHS.contains(&"/biz/*path"),
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
        // ⚠️ 断言**实现无关**：只要求「确实挂了 /biz/*path 且绑到 biz::handler」。
        //   原先写死了 `post(...)`，于是给业务路由**加 GET** 时这条守卫会误报
        //   —— 而「加 GET」正是修掉「GET-only 白名单路径不可达」所必需的改动。
        //   「挂载是否正确」由下一条**真的构建 router + 真的发请求**的用例守（那条才是硬判据）。
        assert!(
            body.contains(r#"/biz/*path"#) && body.contains("routes::biz::handler"),
            "router() 里没挂业务转发路由 —— handler 写了但没人调（本项目已出过三次同类事故）"
        );
    }

    /// issue #21：Responses 路由**必须**在册，且**真的挂在 `router()` 里**。
    ///
    /// 可证伪：删掉 `router()` 里的 `.route("/v1/responses", …)` ⇒ 本用例变红。
    /// 与 [`business_route_is_owned_and_mounted`] 同款：只扫 `router()` 函数体，
    /// 避免本文件其它字符串（含文档注释）让断言**恒真**。
    #[test]
    fn responses_route_is_owned_and_mounted() {
        assert!(
            WbGateway::OWNED_PATHS.contains(&"/v1/responses"),
            "Responses 路径未声明所有权"
        );
        let src = include_str!("lib.rs");
        let body = src
            .split("pub fn router(state: GatewayState) -> axum::Router {")
            .nth(1)
            .and_then(|rest| rest.split("/// 已启动的网关监听句柄").next())
            .expect("找不到 router 函数体（结构已变，请更新本断言）");
        assert!(
            body.contains(r#".route("/v1/responses", post(routes::responses::handler))"#),
            "router() 里没挂 Responses 路由 —— handler 写了但没人调（本项目已出过三次同类事故）"
        );
    }

    /// ★★ **axum 路由语法护栏**：`router()` 必须**真的能构建出来**，且业务路由**真的能匹配**。
    ///
    /// ## 为什么必须有这条（血的教训）
    ///
    /// `cargo check` / `cargo build` **全绿**、`OWNED_PATHS` 声明齐全、字符串断言也全绿，
    /// 但axum **0.7** 不认 0.8 的 `{*path}` 语法 ⇒ `router()` **构建期 panic**
    /// ⇒ **网关根本起不来**（用户侧表现为「网关功能完全无法运行」）。
    /// **编译期完全测不出来**，只有「真的去构建 / 真的发一个请求」才能暴露。
    ///
    /// 可证伪：把 `router()` 里的 `/biz/*path` 改回 `/biz/{*path}` ⇒ 本用例 panic 变红。
    #[tokio::test]
    async fn router_builds_and_biz_route_matches() {
        // 隔离 home：避免构建状态时读写真实`~/.buddy-switch/`。
        let home =
            std::env::temp_dir().join(format!("buddy-switch-router-syntax-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var(
            buddy_switch_core::modules::config::BUDDY_SWITCH_HOME_ENV,
            &home,
        );

        // ① 构建 router：语法非法会在这里 panic（这正是原 bug 的触发点）。
        let state = GatewayState::new(GatewayConfig::default());
        let app = router(state);

        // ② 真打一条业务请求：通配符没真正生效（404 / 405）也要红。
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/biz/v1/user/checkin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status() != axum::http::StatusCode::NOT_FOUND
                && response.status() != axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "/biz/*path 未真正匹配业务请求（得到 {}）—— 通配符语法可能又写错了",
            response.status()
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// 四个网关的路径所有权都**非空**，且**都声明了两个健康检查路径**。
    ///
    /// 回归背景：改名 `/healthz` → `/health` 时最省事的做法是「只改路由、不改声明」，
    /// 或只改三条线里的两条 ⇒ 文档写着两个等价、实际某条线 404。本用例把
    /// 「都支持」钉在每个 `OWNED_PATHS` 上（真正的挂载由
    /// [`health_endpoints_answer_on_every_gateway`] 真发请求守）。
    #[test]
    fn every_product_declares_both_health_paths() {
        for (label, paths) in [
            ("WorkBuddy", WbGateway::OWNED_PATHS),
            ("Trae", trae::TraeGateway::OWNED_PATHS),
            // Qoder 与统一网关的 `OWNED_PATHS` 在各自模块里，平级模块可直接读。
            ("Qoder", crate::qoder::QoderGateway::OWNED_PATHS),
            ("统一网关", crate::unified::UnifiedGateway::OWNED_PATHS),
        ] {
            assert!(!paths.is_empty(), "{label} 的路径所有权不得为空");
            for path in HEALTH_PATHS {
                assert!(
                    paths.contains(&path),
                    "{label} 的 OWNED_PATHS 缺少健康检查路径 `{path}` —— \
                     声明与实现脱节（四个网关必须都支持 `{HEALTH_CANONICAL_PATH}` 与兼容别名）"
                );
            }
        }
    }

    /// ★★ 真发请求：`/health` 与 `/healthz` 都必须 200，且返回体**逐字相同**。
    ///
    /// 为什么不能只查 `OWNED_PATHS`：那张表是声明，axum 里没挂照样 404。
    /// 为什么要求「逐字相同」：两个路径必须是**同一个处理器**；各写一份迟早分家
    /// （这正是本次统一要消灭的那类缺陷）。
    #[tokio::test]
    async fn health_endpoints_answer_on_every_gateway() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        // 隔离 home：`GatewayState::new` 会读配置与目录。
        let home = std::env::temp_dir().join(format!("buddy-switch-health-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var(
            buddy_switch_core::modules::config::BUDDY_SWITCH_HOME_ENV,
            &home,
        );

        // ★ 必须复用**同一份** state：返回体里带 `startedAt`，两次新建 state 会差几毫秒，
        //   断言「逐字相同」就会因为时间戳而不成立（不是路由分家）。
        let state = GatewayState::new(GatewayConfig::default());
        let mut bodies = Vec::new();
        for path in HEALTH_PATHS {
            let app = router(state.clone());
            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::OK,
                "WB 网关的 `{path}` 没有 200 —— 健康检查路径只改了声明、没挂路由？"
            );
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            bodies.push(bytes.to_vec());
        }
        assert_eq!(
            bodies[0], bodies[1],
            "`{HEALTH_CANONICAL_PATH}` 与 `{HEALTH_LEGACY_PATH}` 的返回体必须逐字相同 \
             —— 两个路径共用同一个处理器，不能各写一份"
        );

        std::fs::remove_dir_all(&home).ok();
    }
}
