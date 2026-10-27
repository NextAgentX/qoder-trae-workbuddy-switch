//! Trae 积分：剩余额度查询、每日快照、签到明细与错误冷却。
//!
//! ## 为什么必须 `no_proxy()`
//!
//! 第 C 组的本地 MITM 代理会把**系统代理**改成 `127.0.0.1:<port>`。若积分/签到请求沿用
//! 系统代理，就会走进自己的代理进程：
//!
//! - 代理进程崩溃/被关后端口变「死端口」，所有请求立刻 `connection refused`；
//! - 即使代理活着，请求也会被自己 MITM，日志里出现自我递归的噪声。
//!
//! 参考实现在 Python 侧用 `NO_PROXY=*` + 显式空 `ProxyHandler` 解决同一问题。
//! 这里对应地为本模块的所有请求使用 [`trae_http_client`]（`no_proxy()`），
//! **不要**改用 `config::http_request`——后者会继承环境代理设置。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::modules::trae::device::DeviceEntry;
use crate::modules::trae::jwt;
use crate::modules::trae::paths;
use crate::modules::trae::store;
use crate::modules::trae::variant::TraeVariant;
use crate::modules::trae::{TRAE_APP_VERSION, TRAE_ENTITLEMENT_PATH};

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

/// 全局唯一的上游 HTTP 客户端。
///
/// 关键配置：
/// - `no_proxy()`：见模块头注释，绝不能走本地代理。
/// - 120s 总超时：签到是串行的多账号循环，单账号超时必须显著小于整体容忍时间。
static TRAE_HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

/// 取 Trae 上游 HTTP 客户端。
pub fn trae_http_client() -> &'static reqwest::Client {
    TRAE_HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build trae http client")
    })
}

// ---------------------------------------------------------------------------
// 上游请求头的固定取值
// ---------------------------------------------------------------------------
//
// ## 出处：抓包复刻，不是从客户端源码抄的
//
// 下面这些取值来自**参考实现（Python）抓取的真实流量**。核对过客户端本体：
// `x-market-client-id` / `x-market-user-id` / `x-lscbd-aid` 以及
// `VSCode 1.107.1 (TRAE SOLO CN)` 这一整串，在 Trae 客户端安装目录里
// **一个字面量都搜不到**（5141 个文件全扫过，含 `.exe` 与 `.asar`；
// `TRAE SOLO CN.exe` 里出现的 `x-lgw-req-sdk-type` 只是 TTNet 网络库的
// 内部默认头名，与本模块无关）。也就是说**无法从客户端源码侧核对它们的正确性**，
// 只能以「线上实测能用」为准。
//
// ## 因此：这些值是线上契约，不要"顺手改成看起来更对的"
//
// `TRAE_UA_PRODUCT` 只写了 `TRAE SOLO CN` 一条产品线，而本机实测
// `Trae CN` 与 `TRAE SOLO CN` 是**可同机并存**的两条产品线（见 `platform` 模块）。
// 直觉上会想把产品名改成"按探测到的产品动态生成"，但**没有证据支持**：
//
// - 服务端是否校验 UA 里的产品名，无从得知（UA 未被任何本地日志记录）；
// - 真机日志只证明**两条产品线都调用同一个接口**
//   （`api.trae.cn/trae/api/v2/pay/ide_user_ent_usage`，均返回 200），
//   并不证明 UA 可以随便换。
//
// 所以在拿到「服务端对 UA 不敏感」的实测证据之前，**一律保持现状**。
// 想改的话，先按 `references` 里的抓包方式验证 Trae CN 的真实 UA，再来动这里。
// 下面的回归测试会把取值钉死，改了它就会红 —— 那是提醒，不是障碍。

/// UA 括号里的产品名。
const TRAE_UA_PRODUCT: &str = "TRAE SOLO CN";

/// UA 里的 VSCode 版本号。真机 `resources/app/package.json` 的 `version` 就是它。
const TRAE_VSCODE_VERSION: &str = "1.107.1";

/// `x-lscbd-aid`（活动/埋点侧的应用标识）。
const TRAE_MARKET_AID: &str = "787976";

/// `x-lgw-req-sdk-type`。
const TRAE_SDK_TYPE: &str = "3";

/// `package-type`。`stable_cn` 是 CN 稳定版通道。
const TRAE_PACKAGE_TYPE: &str = "stable_cn";

/// `x-user-region`。Trae 是产品级 CN 通道，不随 WorkBuddy 的 `Region` 变。
const TRAE_USER_REGION: &str = "CN";

/// 完整 UA：`VSCode <版本> (<产品名>)`。
fn user_agent() -> String {
    format!("VSCode {TRAE_VSCODE_VERSION} ({TRAE_UA_PRODUCT})")
}

/// `x-market-client-id`：只有 `VSCode <版本>`，**不带**产品名。
fn market_client_id() -> String {
    format!("VSCode {TRAE_VSCODE_VERSION}")
}

/// 构造签到/状态/积分接口共用的请求头。
///
/// 每个账号使用**自己的**设备标识与 session，这是「一账号一设备」隔离的核心：
/// 沿用同一套 `x-device-id` 会让上游把所有账号关联到同一设备，签到风控随之触发。
///
/// 刻意**不发送 `accept-encoding`**：本 crate 的 reqwest 未启用 gzip feature，
/// 若声明 `gzip, deflate` 而不解压，响应会变成不可解析的二进制。交给 reqwest
/// 自行协商（identity）即可。
///
/// 各固定取值的出处与"为什么不能随手改"见上方常量区注释。
pub fn build_headers(jwt_value: &str, device: &DeviceEntry) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("accept".into(), "*/*".into());
    headers.insert("accept-language".into(), "zh-CN".into());
    headers.insert("authorization".into(), jwt::authorization_header(jwt_value));
    headers.insert("content-type".into(), "application/json".into());
    headers.insert("user-agent".into(), user_agent());
    headers.insert("x-market-client-id".into(), market_client_id());
    headers.insert(
        "x-market-user-id".into(),
        device.market_user_id.clone().unwrap_or_default(),
    );
    headers.insert("x-user-region".into(), TRAE_USER_REGION.into());
    headers.insert("x-device-id".into(), device.device_id.clone());
    headers.insert("x-lgw-req-sdk-type".into(), TRAE_SDK_TYPE.into());
    headers.insert("package-type".into(), TRAE_PACKAGE_TYPE.into());
    headers.insert("x-request-id".into(), uuid::Uuid::new_v4().to_string());
    headers.insert("x-lscbd-aid".into(), TRAE_MARKET_AID.into());
    headers.insert("x-lscbd-platform".into(), platform_tag().into());
    headers.insert("app-version".into(), TRAE_APP_VERSION.into());
    headers.insert("x-tt-trace-id".into(), trace_id());
    headers.insert(
        "vscode-sessionid".into(),
        device.session_id.clone().unwrap_or_default(),
    );
    headers.insert("sec-fetch-dest".into(), "empty".into());
    headers.insert("sec-fetch-mode".into(), "no-cors".into());
    headers.insert("sec-fetch-site".into(), "none".into());
    headers
}

/// `x-lscbd-platform` 取值。参考实现写死 `windows`；这里按真实平台上报，
/// 使 Linux/macOS 上的请求不自称 Windows（减少上游的跨平台特征矛盾）。
fn platform_tag() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// 生成 `x-tt-trace-id`：`00-<16hex>-01`。
fn trace_id() -> String {
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    format!("00-{}-01", &uuid[..16])
}

/// 对 Trae 上游发一个 POST，返回 `(HTTP 状态码, 响应体文本)`。
///
/// 约定与参考实现一致：
/// - 网络层失败返回 `status = 0`，`body` 为错误描述；
/// - HTTP 错误码（4xx/5xx）**不**转成 `Err`，因为签到需要按状态码分类错误
///   （401 → 会话失效、429 → 限频），调用方必须拿到原始状态码。
///
/// # ⚠️ 未验证的线索：请求体可能是空的（`{}`）
///
/// 这里对所有路径都发 `{}`（沿用参考实现）。但**真实 Trae 客户端的请求体不是空的** ——
/// 从客户端自身的缓存里翻出了它自动生成的 API SDK（`%APPDATA%\TRAE SOLO CN\Cache\Cache_Data\f_*`），
/// 里面写着：
///
/// ```js
/// GetIdeUserEntUsageV2(e, t) {
///   let r = e || {},
///       a = this.genBaseURL("/trae/api/v2/pay/ide_user_ent_usage"),
///       i = {require_usage: r.require_usage, req_source: r.req_source,
///            full_data: r.full_data, Request: r.Request};
///   return this.request({url: a, method: "POST", data: i}, t)
/// }
/// ```
///
/// 而且同一份缓存里有个**真实调用点**，默认参数是
/// `{require_usage: true, full_data: true}`，`queryFn` 里也恒为 `full_data: true`：
///
/// ```js
/// function tp(e) {
///   let t = arguments.length > 1 && void 0 !== arguments[1]
///       ? arguments[1] : {require_usage: !0, full_data: !0};
///   return e({url: "/trae/api/v2/pay/user_current_entitlement_list",
///             method: "POST", data: t})
/// }
/// ```
///
/// **也就是说真实客户端总是要求 `full_data: true`，而本模块一个字段都不发。**
/// 若服务端把 `full_data` 默认成 `false`，响应里就不会有完整的
/// `user_entitlement_pack_list`，[`calc_remaining_credits`] 会直接报
/// 「响应中缺少 user_entitlement_pack_list」。
///
/// **为什么明知可疑却还没改**：本机切换器的 Trae 账号库是空的
/// （`checkin_accounts.json` 不存在），**这条路径从未对真实上游跑通过**，
/// 因此"发 `{}` 能用"这个前提本身就没人验证过。同时我也**没有实测证据**
/// 证明 `full_data: true` 才正确。改线上协议不能靠推理。
///
/// **下次有可用账号时的第一步**：先按原样打一次，看响应有没有
/// `user_entitlement_pack_list`；若没有，再改成
/// `{"require_usage": true, "full_data": true}` 复测。别凭猜直接改。
pub async fn post_json(path: &str, jwt_value: &str, device: &DeviceEntry) -> (u16, String) {
    post_json_for(TraeVariant::default(), path, jwt_value, device).await
}

/// 按**产品线变体**派生的 [`post_json`]。
///
/// 端点主机随变体查表（见 [`super::variant`]），路径仍由调用方给。
/// 默认变体 [`TraeVariant::default`] 即 Trae Work，与 [`post_json`] 等价。
///
/// ## 为什么端点必须走变体而不是常量（取证纪律）
///
/// 实测两条 CN 产品线（`TRAE SOLO CN` / `Trae CN`）的 `product.json` 里
/// `*.trae.normal` **逐字相同** —— 即**产品线不改变端点，region 才改变端点**。
/// 因此本函数只负责「取到该变体登记的那套端点」；变体之间当前取值相同是
/// **实测结论**而不是巧合，[`super::variant`] 里有回归测试钉死。
/// 将来若要给某条产品线单独换域，必须**先取到该产品线自己的实测证据**，
/// 再改那张表 —— 不要在调用点写 `if variant == …`。
pub async fn post_json_for(
    variant: TraeVariant,
    path: &str,
    jwt_value: &str,
    device: &DeviceEntry,
) -> (u16, String) {
    let base = super::endpoints_for(variant).account_base;
    let url = format!("{base}{path}");
    let mut request = trae_http_client().post(&url).body("{}");
    for (key, value) in build_headers(jwt_value, device) {
        request = request.header(key, value);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            (status, text)
        }
        // 带码版：文本与 `describe_transport_error` 逐字节相同，另带 `net.transport.*` 码，
        // 供前端按当前语言重渲染「刷新积分失败」这类提示。
        Err(error) => (0, crate::modules::net::transport_error(&error).to_wire()),
    }
}

/// POST 并解析 JSON。网络失败或非 JSON 响应时返回 `Err`。
pub async fn post_json_parsed(
    path: &str,
    jwt_value: &str,
    device: &DeviceEntry,
) -> Result<(u16, Value), String> {
    post_json_parsed_for(TraeVariant::default(), path, jwt_value, device).await
}

/// 按变体派生的 [`post_json_parsed`]，见 [`post_json_for`] 的取证纪律说明。
pub async fn post_json_parsed_for(
    variant: TraeVariant,
    path: &str,
    jwt_value: &str,
    device: &DeviceEntry,
) -> Result<(u16, Value), String> {
    let (status, body) = post_json_for(variant, path, jwt_value, device).await;
    if status == 0 {
        return Err(body);
    }
    let parsed = serde_json::from_str::<Value>(&body)
        .map_err(|_| format!("HTTP {status}: 非 JSON 响应: {}", truncate(&body, 200)))?;
    Ok((status, parsed))
}

/// 截断长文本用于错误文案。
fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 一条签到积分明细。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreditRecord {
    pub date: String,
    pub user_id: String,
    pub credits: i64,
    #[serde(default)]
    pub delta: i64,
}

/// 签到积分明细文件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreditsFile {
    #[serde(default)]
    pub records: Vec<CreditRecord>,
}

/// 每日积分快照（趋势图数据源）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreditsDailySnapshot {
    pub date: String,
    pub total: f64,
    pub earned: f64,
    pub consumed: f64,
}

/// 每日积分快照文件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreditsDailyFile {
    #[serde(default)]
    pub snapshots: Vec<CreditsDailySnapshot>,
}

/// 单个积分包（资源包）的明细。
///
/// 源：`user_entitlement_pack_list` 里**逐包**解析而来（改造前只算聚合总额与最早到期，
/// 把逐包明细丢弃了）。落进 `remaining_credits.json` 的 `packages` 字段，
/// 经 `credits_overview_for` 透出，供账号卡渲染「积分包进度条」。
///
/// 线上/磁盘形状为 camelCase（`packageCode` / `packageName` / `expireAt` /
/// `expiringSoon`），前端类型 `TraeCreditPackage` 逐字对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditPackage {
    /// 包编码（上游字段缺失时为 `None`）。
    pub package_code: Option<String>,
    /// 包名（上游字段缺失时为 `None`）。
    pub package_name: Option<String>,
    /// 包总额度（`credits_limit`）。
    pub total: f64,
    /// 剩余额度（`credits_limit - usage.credits_amount`，下限 0）。
    pub remaining: f64,
    /// 已用额度（`usage.credits_amount`，无 usage 视为 0）。
    pub used: f64,
    /// 到期时间（Unix 秒；无则 `None`）。
    pub expire_at: Option<i64>,
    /// 是否已过期（`expire_at` 存在且 `< now`）。
    pub expired: bool,
    /// 是否**临期**（未过期且 `expire_at - now <= ` [`EXPIRING_SOON_SECS`]）。
    pub expiring_soon: bool,
}

/// 「临期」的判定窗口（秒）—— **14 天**。
///
/// ## 为什么从 7 天放宽到 14 天（issue #83 · 3）
///
/// 7 天作为「再不用就浪费了」的告警太紧：多数人一周才打开一次这个工具，
/// 7 天窗口常常在用户下一次打开之前就滑过去了 —— 徽标从来没亮过，积分就过期了。
///
/// 为什么不是 30 天：大部分积分包的有效期本身就在 30 天内，30 天窗口会让**几乎所有**
/// 包都显示「临期」⇒ 这个信号失去区分度，用户会学会无视它。14 天是「两次使用间隔
/// 的两倍左右」，既能在真正浪费前提醒，又不至于天天红。
///
/// ⚠️ 这是**产品口径**，不是技术常量：改它等于改文案的承诺
/// （`trae.comp.card.chip.expiring` 写的是「N 天内过期」，由本窗口决定 N 的上界）。
const EXPIRING_SOON_SECS: i64 = 14 * 24 * 3600;

/// 由一组 `user_entitlement_pack_list` 元素解析逐包明细（纯函数，便于单测）。
///
/// 只统计 `entitlement_base_info.quota.credits_limit` 存在的包（与聚合口径一致）：
/// 无额度上限的包不参与剩余量统计（可能是无限制资源）。
pub fn parse_credit_packages(packs: &[Value], now_ts: i64) -> Vec<CreditPackage> {
    let mut packages = Vec::new();
    for pack in packs {
        let base = pack.get("entitlement_base_info");
        let limit = base
            .and_then(|info| info.get("quota"))
            .and_then(|quota| quota.get("credits_limit"))
            .and_then(Value::as_f64);
        let Some(limit) = limit else {
            continue;
        };
        let used = pack
            .get("usage")
            .and_then(|usage| usage.get("credits_amount"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let remaining = (limit - used).max(0.0);
        let expire_at = pack.get("expire_time").and_then(Value::as_i64);
        let expired = expire_at
            .map(|expire| expire > 0 && expire < now_ts)
            .unwrap_or(false);
        let expiring_soon = !expired
            && expire_at
                .map(|expire| expire - now_ts <= EXPIRING_SOON_SECS)
                .unwrap_or(false);
        let package_name = base
            .and_then(|info| {
                info.get("package_name")
                    .or_else(|| info.get("name"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty());
        let package_code = base
            .and_then(|info| {
                info.get("package_code")
                    .or_else(|| info.get("package_type"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty());
        packages.push(CreditPackage {
            package_code,
            package_name,
            total: crate::modules::trae::credits::round2(limit),
            remaining: crate::modules::trae::credits::round2(remaining),
            used: crate::modules::trae::credits::round2(used),
            expire_at,
            expired,
            expiring_soon,
        });
    }
    packages
}

/// 剩余积分与到期时间缓存。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemainingCreditsFile {
    #[serde(default)]
    pub credits: HashMap<String, f64>,
    #[serde(default)]
    pub expire_times: HashMap<String, i64>,
    /// 逐账号的积分包明细（`uid -> [CreditPackage…]`）。
    ///
    /// `#[serde(default)]`：旧 `remaining_credits.json`（无此字段）读出为空 map，
    /// **不 panic**——这是升级零回归的必要条件（R4）。
    #[serde(default)]
    pub packages: HashMap<String, Vec<CreditPackage>>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// 单个账号的冷却状态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CooldownEntry {
    /// 错误类型（`SessionDead` / `SoftRate` / `Server` / …）。
    #[serde(rename = "type", default)]
    pub error_type: String,
    /// 冷却截止（Unix 秒）；`SessionDead` 用 9999999999 表示永久。
    #[serde(default)]
    pub until: i64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub error_count: i32,
    /// 判定时那份 JWT 的 `exp`（Unix 秒）。**凭据指纹**（issue #108 §P0-A）。
    ///
    /// ## 为什么需要它
    ///
    /// 冷却表只记了「什么错、多久」，**没记「判的是哪一份凭据」**。于是当凭据被换掉
    /// （OAuth 重新登录 / JWT 续期 / 手动粘贴），那条「会话已死」的判定无法自证失效 ——
    /// 用户看到的是「重新登录后仍显示需重新登录」，而且重启多少次都一样。
    ///
    /// ## 为什么取 `exp` 而不是 access token 原文
    ///
    /// 用 token 原文当指纹等于「每次保活刷新都把所有被禁用的号洗白」，三振机制直接被取消
    /// （WorkBuddy 侧 `pool/mod.rs` 已就此论证过）。`exp` 是**上游签发**的量：真正换到
    /// 新令牌时它才变；刷新失败时 token 根本不会被写回，因此不会误放行。
    ///
    /// ## 兼容性
    ///
    /// 旧文件缺这个键 ⇒ `None` ⇒ 按「无指纹」处理，**照旧生效**（保守方向，不会误放行）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_exp: Option<i64>,
    /// 这条判定**写入的时刻**（Unix 秒，issue #108 §P1-A）。
    ///
    /// 前端此前直接把 `reason` 原文渲染成一行红字，用户会把它读成「刚才又失败了」——
    /// 而事实可能是「11 小时前失败过一次」。带上时刻才能说「11 小时前」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judged_at: Option<i64>,
}

impl CooldownEntry {
    /// 这条判定是否仍然针对**当前**这份凭据。
    ///
    /// - 记录里没有指纹（旧数据 / 无 JWT）⇒ `true`（照旧生效，保守）；
    /// - 当前 JWT 解析不出 `exp` ⇒ `true`（拿不到证据时不做放行）；
    /// - 两者都有 ⇒ 相等才生效。
    pub fn matches_credential(&self, current_exp: Option<i64>) -> bool {
        match (self.credential_exp, current_exp) {
            (Some(recorded), Some(current)) => recorded == current,
            _ => true,
        }
    }
}

/// 冷却状态文件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CooldownsFile {
    #[serde(default)]
    pub cooldowns: HashMap<String, CooldownEntry>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// 最近一次签到摘要。
///
/// **序列化为 camelCase**：该结构既落盘（`checkin_summary.json`）又直接进入
/// `get_trae_checkin_status` 的响应体，两种情况都是本模块自己写、自己读，
/// 不存在与外部工具交换的兼容负担，因此统一用线上命名，避免「同一个对象
/// 在磁盘上是 `total_ok`、在响应里是 `totalOk`」的双形态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinSummary {
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub results: Vec<Value>,
    #[serde(default)]
    pub total_ok: i32,
    #[serde(default)]
    pub already: i32,
    #[serde(default)]
    pub failed: i32,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// 今日已签到台账（跨运行累积，按 `userId`）。
///
/// **为什么必须是独立的一份文件**：`CheckinSummary` 记录的是「**最近一次运行**
/// 处理了哪些账号」，每轮整体覆盖。而「今日已签到」是**跨运行累积、按账号**的事实
/// —— 打开 `skip_checked_in` 时，一轮只会处理「本轮还没签过的账号」，
/// 用摘要兼作台账会让上一轮签过的账号在下一轮丢掉标记（徽章显示「未签到」，
/// 且下一轮又把它重新探测一遍）。
///
/// 键必须是 `userId` 而不是显示名：显示名可重复、可修改，用它做键会让两个同名账号
/// 互相冒充（曾经的实现正是按 `name` 匹配摘要）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckinLedger {
    /// 台账归属日（本地时区 `YYYY-MM-DD`）。
    #[serde(default)]
    pub date: String,
    /// 该日已签到的 `userId`（`claim_ok` 与 `skip_already` 都计入）。
    #[serde(default)]
    pub checked_in: Vec<String>,
}

// ---------------------------------------------------------------------------
// 读写
// ---------------------------------------------------------------------------

/// 读取剩余积分缓存（默认变体，兼容壳）。
pub fn load_remaining() -> RemainingCreditsFile {
    load_remaining_for(TraeVariant::default())
}

/// 读取剩余积分缓存（按变体分家）。
pub fn load_remaining_for(variant: TraeVariant) -> RemainingCreditsFile {
    store::read_json(&paths::remaining_credits_file_for(variant))
}

/// 写入剩余积分缓存（默认变体，兼容壳）。
pub fn save_remaining(remaining: &RemainingCreditsFile) -> Result<(), String> {
    save_remaining_for(TraeVariant::default(), remaining)
}

/// 写入剩余积分缓存（按变体分家）。
pub fn save_remaining_for(
    variant: TraeVariant,
    remaining: &RemainingCreditsFile,
) -> Result<(), String> {
    store::write_json(&paths::remaining_credits_file_for(variant), remaining)
}

/// 读取签到积分明细（默认变体，兼容壳）。
pub fn load_history() -> CreditsFile {
    load_history_for(TraeVariant::default())
}

/// 读取签到积分明细（按变体分家）。
pub fn load_history_for(variant: TraeVariant) -> CreditsFile {
    store::read_json(&paths::credits_history_file_for(variant))
}

/// 读取每日快照（默认变体，兼容壳）。
pub fn load_daily() -> CreditsDailyFile {
    load_daily_for(TraeVariant::default())
}

/// 读取每日快照（按变体分家）。
pub fn load_daily_for(variant: TraeVariant) -> CreditsDailyFile {
    store::read_json(&paths::credits_daily_file_for(variant))
}

/// 读取冷却状态（默认变体，兼容壳）。
pub fn load_cooldowns() -> CooldownsFile {
    load_cooldowns_for(TraeVariant::default())
}

/// 读取冷却状态（按变体分家）。
pub fn load_cooldowns_for(variant: TraeVariant) -> CooldownsFile {
    store::read_json(&paths::cooldowns_file_for(variant))
}

/// 读取签到摘要（默认变体，兼容壳）。
pub fn load_summary() -> CheckinSummary {
    load_summary_for(TraeVariant::default())
}

/// 读取签到摘要（按变体分家）。
pub fn load_summary_for(variant: TraeVariant) -> CheckinSummary {
    store::read_json(&paths::checkin_summary_file_for(variant))
}

/// 写入签到摘要（默认变体，兼容壳）。
pub fn save_summary(summary: &CheckinSummary) -> Result<(), String> {
    save_summary_for(TraeVariant::default(), summary)
}

/// 写入签到摘要（按变体分家）。
pub fn save_summary_for(variant: TraeVariant, summary: &CheckinSummary) -> Result<(), String> {
    store::write_json(&paths::checkin_summary_file_for(variant), summary)
}

/// 读取今日已签到台账（默认变体，兼容壳）。
pub fn load_ledger() -> CheckinLedger {
    load_ledger_for(TraeVariant::default())
}

/// 读取今日已签到台账（按变体分家）。
///
/// **跨日即视为空**：台账只对当天有意义，日期不符时返回空台账。读路径**不写盘**
/// （不产生副作用），下一次 [`mark_checked_in_for`] 会以新日期重建。
/// 判据与 `list_account_views_for` 里「摘要必须是今天的」一致，都用 [`store::today`]。
pub fn load_ledger_for(variant: TraeVariant) -> CheckinLedger {
    let ledger: CheckinLedger = store::read_json(&paths::checkin_ledger_file_for(variant));
    if ledger.date == store::today() {
        ledger
    } else {
        CheckinLedger::default()
    }
}

/// 记一笔「今日已签到」（默认变体，兼容壳）。
pub fn mark_checked_in(user_id: &str) -> Result<(), String> {
    mark_checked_in_for(TraeVariant::default(), user_id)
}

/// 记一笔「今日已签到」（按变体分家）。
///
/// 幂等（同账号重复标记不产生重复项）。**每次调用都重读整份台账**，而不是由调用方
/// 持一份集合在收尾时统一落盘：这样「一轮签了 3 个账号」与「分 3 次调用」落盘结果
/// 一致，中途失败时已成功的那几笔不会一起丢。
pub fn mark_checked_in_for(variant: TraeVariant, user_id: &str) -> Result<(), String> {
    let mut ledger = load_ledger_for(variant);
    ledger.date = store::today();
    if !ledger.checked_in.iter().any(|uid| uid == user_id) {
        ledger.checked_in.push(user_id.to_string());
    }
    store::write_json(&paths::checkin_ledger_file_for(variant), &ledger)
}

/// 「今日已签到」的 `userId` 集合 = **台账 ∪ 当日积分明细**。
///
/// 台账是主来源（见 [`CheckinLedger`]）；并上 [`load_history_for`] 里的当日记录，理由有两条：
///
/// 1. **升级零回归**：台账是后加的文件，用户升级当天「今天已经签过」的账号不在台账里，
///    只读台账会让它们的徽章在下一轮签到前错误地显示「未签到」——正是本次要修的那个症状；
/// 2. **兜底**：进程若在 claim 成功之后、落账之前被杀，明细里那条记录仍能证明签过。
///
/// 之所以敢用明细当依据：它**只增不改**，且只有**成功**路径会写
/// （`claim_ok` 写 `delta=credits`、`skip_already` 写 `delta=0`，见 `checkin::run_checkin`），
/// 因此「今天有记录」等价于「今天签过」。失败路径只写冷却，不写明细。
pub fn checked_in_today_for(variant: TraeVariant) -> std::collections::HashSet<String> {
    let today = store::today();
    let mut checked: std::collections::HashSet<String> =
        load_ledger_for(variant).checked_in.into_iter().collect();
    for record in load_history_for(variant).records {
        if record.date == today {
            checked.insert(record.user_id);
        }
    }
    checked
}

/// 追加一条签到积分明细（默认变体，兼容壳）。
pub fn append_history(user_id: &str, credits: i64, delta: i64) -> Result<(), String> {
    append_history_for(TraeVariant::default(), user_id, credits, delta)
}

/// 追加一条签到积分明细（按变体分家），并裁剪到
/// [`crate::modules::trae::TRAE_HISTORY_KEEP_DAYS`] 天内。
pub fn append_history_for(
    variant: TraeVariant,
    user_id: &str,
    credits: i64,
    delta: i64,
) -> Result<(), String> {
    let mut file = load_history_for(variant);
    file.records.push(CreditRecord {
        date: store::today(),
        user_id: user_id.to_string(),
        credits,
        delta,
    });
    let cutoff = (chrono::Local::now()
        - chrono::Duration::days(crate::modules::trae::TRAE_HISTORY_KEEP_DAYS))
    .format("%Y-%m-%d")
    .to_string();
    file.records.retain(|record| record.date >= cutoff);
    store::write_json(&paths::credits_history_file_for(variant), &file)
}

/// 记录/更新当日积分快照。
///
/// - `total` = 所有账号剩余积分之和
/// - `earned` = 今日签到获得（明细里的 `delta` 之和）+ 购买获得（`non_checkin_earned`）
/// - `consumed` = `|total - earned - 昨日 total|`
///
/// 当日快照已存在时**整条更新**而不是跳过：首次记录往往发生在「签到后但积分尚未
/// 刷新」的时刻，若只写一次，`earned` / `consumed` 会永久停留在错误值。
pub fn record_daily_snapshot(non_checkin_earned: f64) {
    record_daily_snapshot_for(TraeVariant::default(), non_checkin_earned)
}

/// 记录/更新当日积分快照（按变体分家）。
///
/// **必须整条链路同变体**：剩余积分、积分明细、每日快照三个文件都按变体分家，
/// 混用会让「今日获得」与「昨日结余」来自两条产品线，算出无意义的消耗值。
pub fn record_daily_snapshot_for(variant: TraeVariant, non_checkin_earned: f64) {
    let today = store::today();
    let remaining = load_remaining_for(variant);
    let total = round2(remaining.credits.values().sum::<f64>());

    let today_checkin_earned: f64 = load_history_for(variant)
        .records
        .iter()
        .filter(|record| record.date == today)
        .map(|record| record.delta as f64)
        .sum();
    let earned = round2(today_checkin_earned + non_checkin_earned);

    let mut file = load_daily_for(variant);
    let yesterday_total = file
        .snapshots
        .iter()
        .rfind(|snapshot| snapshot.date < today)
        .map(|snapshot| snapshot.total)
        .unwrap_or(0.0);
    let consumed = round2((total - earned - yesterday_total).abs());

    match file
        .snapshots
        .iter_mut()
        .find(|snapshot| snapshot.date == today)
    {
        Some(existing) => {
            existing.total = total;
            existing.earned = earned;
            existing.consumed = consumed;
        }
        None => file.snapshots.push(CreditsDailySnapshot {
            date: today,
            total,
            earned,
            consumed,
        }),
    }

    let cutoff = (chrono::Local::now()
        - chrono::Duration::days(crate::modules::trae::TRAE_HISTORY_KEEP_DAYS))
    .format("%Y-%m-%d")
    .to_string();
    file.snapshots.retain(|snapshot| snapshot.date >= cutoff);

    let _ = store::write_json(&paths::credits_daily_file_for(variant), &file);
}

/// 四舍五入到 2 位小数（与参考实现一致，避免浮点尾差在 UI 上显示成 12.340000001）。
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------------
// 剩余积分
// ---------------------------------------------------------------------------

/// 查询某账号的剩余积分。
///
/// 返回 `(剩余积分, 最早过期时间戳, 今日购买获得积分, 逐包明细)`。
///
/// 计算规则（与参考实现一致）：
/// - 只统计 `entitlement_base_info.quota.credits_limit` 存在的资源包；
/// - 每包剩余 = `credits_limit - usage.credits_amount`（无 usage 视为已用 0），**下限为 0**；
/// - 「最早过期」由 [`earliest_live_expire`] 计算：只在**还有剩余**、且 `expire_time > now`
///   的包里取（已过期、已用完的包都不影响紧迫度排序）；
/// - 「今日购买获得」只计 `start_time` 落在**北京时区**今日、且 `charge_amount > 0` 的包
///   ——签到获得的包 `charge_amount = 0`，因此天然不会被误计为购买；
/// - 逐包明细（第 4 个返回值）由 [`parse_credit_packages`] 统一解析，供账号卡进度条。
pub async fn calc_remaining_credits(
    jwt_value: &str,
) -> Result<(f64, Option<i64>, f64, Vec<CreditPackage>), String> {
    let device = device_for_jwt(jwt_value)?;
    let (_status, body) = post_json_parsed(TRAE_ENTITLEMENT_PATH, jwt_value, &device).await?;

    let packs = body
        .get("user_entitlement_pack_list")
        .and_then(|value| value.as_array())
        .ok_or("响应中缺少 user_entitlement_pack_list")?;

    // 固定 UTC+8：上游时间戳按北京时间切日，若用本地时区，
    // 身处非 +08 时区的用户会算错「今日购买」。
    let cst = chrono::FixedOffset::east_opt(8 * 3600).expect("valid offset");
    let now_ts = chrono::Utc::now().timestamp();
    let today_start = chrono::Utc::now()
        .with_timezone(&cst)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .ok_or("时间计算失败")?
        .and_local_timezone(cst)
        .single()
        .ok_or("时区映射失败")?
        .timestamp();
    let today_end = today_start + 86_400;

    let mut total = 0.0_f64;
    let mut purchased_today = 0.0_f64;

    for pack in packs {
        let base = pack.get("entitlement_base_info");
        let limit = base
            .and_then(|info| info.get("quota"))
            .and_then(|quota| quota.get("credits_limit"))
            .and_then(|value| value.as_f64());
        let Some(limit) = limit else {
            // 无额度上限的包不参与剩余量统计（可能是无限制资源）。
            continue;
        };

        let used = pack
            .get("usage")
            .and_then(|usage| usage.get("credits_amount"))
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        total += (limit - used).max(0.0);

        let charge_amount = base
            .and_then(|info| info.get("charge_amount"))
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if charge_amount > 0 {
            if let Some(start_time) = base
                .and_then(|info| info.get("start_time"))
                .and_then(|value| value.as_i64())
            {
                if start_time >= today_start && start_time < today_end {
                    purchased_today += limit;
                }
            }
        }
    }

    Ok((
        round2(total),
        earliest_live_expire(packs, now_ts),
        round2(purchased_today),
        parse_credit_packages(packs, now_ts),
    ))
}

/// 「最早到期」：只在**还有剩余**、且 `expire_time > now` 的包里取（纯函数，便于单测）。
///
/// ## ★ 为什么必须要求「还有剩余」
///
/// 已经用完的包（剩余 0）只要 `expire_time` 还没到，就仍然带着一个「到期时刻」。
/// 若把它算进去，这个账号会被**伪装成快过期**：徽标显示它的到期日、选号据
/// [`expiry_urgency`](crate::modules::trae) 把它排到最前 —— 而它真正还有余额的积分
/// 可能几十天后才到期，真正快过期的账号反而永远轮不到（issue #7）。
///
/// 判据在两侧本就不该分家：WorkBuddy 的快过期口径是「把快过期包的**剩余**加总」
/// （见 `buddy-switch-gateway` 的 `credits_refresh`），空包天然贡献 0；Trae 侧只看
/// 到期时刻，必须自己排除空包，否则同一条语义在两条产品线上行为相反。
///
/// 已过期的包同样不参与：它们的额度已经作废，不构成紧迫度。
pub fn earliest_live_expire(packs: &[Value], now_ts: i64) -> Option<i64> {
    let mut earliest: Option<i64> = None;
    for pack in packs {
        let limit = pack
            .get("entitlement_base_info")
            .and_then(|info| info.get("quota"))
            .and_then(|quota| quota.get("credits_limit"))
            .and_then(Value::as_f64);
        // 无额度上限的包不参与（与聚合口径一致）。
        let Some(limit) = limit else {
            continue;
        };
        let used = pack
            .get("usage")
            .and_then(|usage| usage.get("credits_amount"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        // ★ 已用完的包不得参与「最早到期」。
        if (limit - used).max(0.0) <= 0.0 {
            continue;
        }
        if let Some(expire) = pack.get("expire_time").and_then(Value::as_i64) {
            if expire > now_ts {
                earliest = Some(match earliest {
                    Some(current) => current.min(expire),
                    None => expire,
                });
            }
        }
    }
    earliest
}

/// 由 JWT 解析 uid 并取（必要时创建）设备标识（默认变体，兼容壳）。
pub fn device_for_jwt(jwt_value: &str) -> Result<DeviceEntry, String> {
    device_for_jwt_for(TraeVariant::default(), jwt_value)
}

/// 由 JWT 解析 uid 并取（必要时创建）设备标识（按变体分家）。
///
/// 设备映射按变体分家，因此查询设备也必须带上变体，否则会拿到另一条产品线的设备身份。
pub fn device_for_jwt_for(variant: TraeVariant, jwt_value: &str) -> Result<DeviceEntry, String> {
    let uid = jwt::user_id_of(jwt_value).ok_or("无法从 JWT 解析 user id")?;
    crate::modules::trae::device::ensure_for_variant(variant, &uid)
}

/// 刷新单个账号的剩余积分缓存，返回剩余积分（默认变体，兼容壳）。
pub async fn refresh_remaining_for(user_id: &str) -> Result<f64, String> {
    refresh_remaining_for_variant(TraeVariant::default(), user_id).await
}

/// 刷新单个账号的剩余积分缓存（按变体分家），返回剩余积分。
pub async fn refresh_remaining_for_variant(
    variant: TraeVariant,
    user_id: &str,
) -> Result<f64, String> {
    let account = crate::modules::trae::account::find_for(variant, user_id).ok_or("账号不存在")?;
    let (credits, expire_at, _purchased, packages) = calc_remaining_credits(&account.jwt).await?;
    let mut remaining = load_remaining_for(variant);
    remaining.credits.insert(user_id.to_string(), credits);
    if let Some(expire_at) = expire_at {
        remaining
            .expire_times
            .insert(user_id.to_string(), expire_at);
    }
    remaining.packages.insert(user_id.to_string(), packages);
    remaining.updated_at = Some(store::now_iso());
    save_remaining_for(variant, &remaining)?;
    Ok(credits)
}

/// 批量刷新所有账号的剩余积分，返回成功账号数（默认变体，兼容壳）。
pub async fn refresh_all_remaining() -> usize {
    refresh_all_remaining_for(TraeVariant::default()).await
}

/// 批量刷新**该变体**所有账号的剩余积分（按变体分家），返回成功账号数。
///
/// 顺带执行**自动解冻**：某账号查询成功且剩余积分 > 0 时清除冷却（**含 `SessionDead`**）。
///
/// 判据是「这份凭据刚刚成功访问了上游」—— 一个独立的**正向事实**，不是对
/// 「账号库变没变」的推断（后者已被 WorkBuddy 侧论证为不可靠）。
/// issue #108 §P0-C：排除 `SessionDead` 会让永久记录没有任何自愈路径。
pub async fn refresh_all_remaining_for(variant: TraeVariant) -> usize {
    let accounts = crate::modules::trae::account::entries_for(variant);
    let mut remaining = load_remaining_for(variant);
    let mut cooldowns = load_cooldowns_for(variant);
    let mut succeeded = 0usize;
    let mut thawed = 0usize;
    let mut purchased_today = 0.0_f64;

    for (uid, account) in &accounts {
        match calc_remaining_credits(&account.jwt).await {
            Ok((credits, expire_at, purchased, packages)) => {
                remaining.credits.insert(uid.clone(), credits);
                if let Some(expire_at) = expire_at {
                    remaining.expire_times.insert(uid.clone(), expire_at);
                }
                remaining.packages.insert(uid.clone(), packages);
                purchased_today += purchased;
                succeeded += 1;

                // ★★ issue #108 §P0-C：**取积分成功本身就是会话可用的证据**。
                //
                // 改造前这里显式排除 `SessionDead`，理由写的是「会话彻底失效，积分再正常
                // 也不代表会话可用」。这个理由在**凭据未更新**时也站不住：`calc_remaining_credits`
                // 用的是同一个 JWT 打上游，它能成功就证明这份凭据有效。
                //
                // 后果是闭环死锁：`SessionDead` ⇒ `disabled` ⇒ 不路由（`note_success` 断掉）
                // ⇒ 签到跳过（`clear_cooldown_for` 轮不到）⇒ 只剩人工「解除冷却」一条路。
                // 现场证据：积分文件 08:07 仍在刷新，冷却文件停在 10-26 19:40 一动不动。
                let thawable = should_thaw(credits, cooldowns.cooldowns.get(uid));
                if thawable {
                    cooldowns.cooldowns.remove(uid);
                    thawed += 1;
                    store::append_log(
                        &paths::app_log_file(),
                        &format!(
                            "自动解冻账号 {uid} [{}]: 剩余积分 {credits}，冷却已清除",
                            variant.display_name()
                        ),
                    );
                }
            }
            Err(error) => {
                store::append_log(
                    &paths::app_log_file(),
                    &format!(
                        "获取剩余积分失败 [{} / {}]: {error}",
                        variant.display_name(),
                        account.name
                    ),
                );
            }
        }
    }

    remaining.updated_at = Some(store::now_iso());
    let _ = save_remaining_for(variant, &remaining);
    record_daily_snapshot_for(variant, purchased_today);

    if thawed > 0 {
        cooldowns.updated_at = Some(store::now_iso());
        let _ = store::write_json(&paths::cooldowns_file_for(variant), &cooldowns);
    }
    succeeded
}

/// 自动解冻的判据（**唯一实现**，issue #108 §P0-C）。
///
/// 语义：**这份凭据刚刚成功访问了上游**（取积分成功）⇒ 会话可用 ⇒ 冷却应当清除，
/// `SessionDead` 也不例外。
///
/// 改造前这里显式排除 `SessionDead`，理由是「积分正常不代表会话可用」。该理由在
/// 凭据未更新时也站不住 —— `calc_remaining_credits` 用同一个 JWT 打上游，它能成功
/// 就证明这份凭据有效。而排除的后果是**闭环死锁**：
/// `SessionDead` ⇒ `disabled` ⇒ 不路由（`note_success` 断掉）⇒ 签到跳过
/// （`clear_cooldown_for` 轮不到）⇒ 只剩人工「解除冷却」一条路，重启也没用。
pub(crate) fn should_thaw(credits: f64, entry: Option<&CooldownEntry>) -> bool {
    credits > 0.0
        && entry
            .map(|entry| !entry.error_type.is_empty())
            .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// 冷却
// ---------------------------------------------------------------------------

/// 按 HTTP 状态码与业务码分类错误，返回 `(错误类型, 冷却秒数)`。
///
/// `冷却秒数` 语义：`-1` = 永久、`0` = 不冷却（仅计数）、`>0` = 冷却秒数。
///
/// 分类依据是参考实现在长期使用中总结的经验值，不要随意调小。
///
/// ★ issue #108 §P0-B：**先看 body 里的会话作废标记，再按状态码归类**。
///
/// - 带 `12153` / `Offline user session not found` ⇒ `SessionDead`（`-1` 永久）；
/// - 裸 401/403 ⇒ `AuthRejected`（60 秒，可自愈，**不禁用**）。
///
/// 改造前 401 一律永久，把「access token 过期」这种保活刷新一轮就好、可自愈的问题
/// 打成了终态 —— 用户看到的是「号没坏却再也用不了」。
pub fn classify_error(http_status: u16, code: Option<i64>, body: &str) -> (&'static str, i64) {
    // 200 + 业务码 1005：套餐额度限制，半天内不会有变化。
    if http_status == 200 && code == Some(1005) {
        return ("PlanLimit", 43_200);
    }
    // 作废标记优先于状态码：上游也可能用 200/400 返回它。
    if crate::modules::upstream::SESSION_DEAD_MARKERS
        .iter()
        .any(|marker| body.contains(marker))
    {
        return ("SessionDead", -1);
    }
    match http_status {
        429 => ("SoftRate", 60),
        401 | 403 => ("AuthRejected", 60),
        404 => ("NotFound", 60),
        500..=599 => ("Server", 600),
        400..=499 => ("Client", 600),
        _ => {
            if code.is_some_and(|code| code != 0) {
                ("BusinessError", 300)
            } else {
                ("Unknown", 0)
            }
        }
    }
}

/// 写入/更新账号冷却状态（默认变体，兼容壳）。
pub fn save_cooldown(user_id: &str, error_type: &str, cooldown_seconds: i64, reason: &str) {
    save_cooldown_for(
        TraeVariant::default(),
        user_id,
        error_type,
        cooldown_seconds,
        reason,
    )
}

/// 写入/更新账号冷却状态（按变体分家）。
///
/// `cooldown_seconds` 的三种特殊语义与 [`classify_error`] 一致。
/// `Server` / `Client` 类型有「连续 3 次才真正冷却」的宽容期：瞬时抖动不该立刻封账号，
/// 前两次只累加 `error_count` 且 `until = 0`（即不冷却）。
///
/// **必须分家**：冷却表的 `error_count` 是累积状态，两条产品线共用会让
/// 一条线的失败计数把另一条线的账号「顶」进冷却。
pub fn save_cooldown_for(
    variant: TraeVariant,
    user_id: &str,
    error_type: &str,
    cooldown_seconds: i64,
    reason: &str,
) {
    save_cooldown_with_credential_for(variant, user_id, error_type, cooldown_seconds, reason, None);
}

/// 写入冷却，并记下「判定时那份凭据的 `exp`」作为**指纹**（issue #108 §P0-A）。
///
/// `credential_exp` 为 `None` 时与 [`save_cooldown_for`] 完全等价（不写指纹 ⇒
/// 该条判定永远不会因凭据更新而自动失效，即旧行为）。
///
/// 传 `Some(exp)` 后，`account_view` 会在渲染时比对当前 JWT 的 `exp`：
/// 不一致 ⇒ 这条判定针对的是**上一份**凭据，自动失效。于是「重新登录 / 续期 /
/// 手动粘贴」这些只写账号库、不碰冷却表的出口，**无需改动即可自愈**。
pub fn save_cooldown_with_credential_for(
    variant: TraeVariant,
    user_id: &str,
    error_type: &str,
    cooldown_seconds: i64,
    reason: &str,
    credential_exp: Option<i64>,
) {
    let mut file = load_cooldowns_for(variant);
    let now = chrono::Local::now().timestamp();

    if cooldown_seconds == 0 {
        // 0 = 不冷却：清掉旧记录，避免残留的 error_count 影响下次判定。
        file.cooldowns.remove(user_id);
        file.updated_at = Some(store::now_iso());
        let _ = store::write_json(&paths::cooldowns_file_for(variant), &file);
        return;
    }

    let (until, error_count) = if cooldown_seconds == -1 {
        (9_999_999_999_i64, 0)
    } else if matches!(error_type, "Server" | "Client") {
        let count = file
            .cooldowns
            .get(user_id)
            .map(|entry| entry.error_count)
            .unwrap_or(0)
            + 1;
        if count < 3 {
            (0, count)
        } else {
            (now + cooldown_seconds, 0)
        }
    } else {
        (now + cooldown_seconds, 0)
    };

    file.cooldowns.insert(
        user_id.to_string(),
        CooldownEntry {
            error_type: error_type.to_string(),
            until,
            reason: reason.to_string(),
            error_count,
            credential_exp,
            judged_at: Some(now),
        },
    );
    file.updated_at = Some(store::now_iso());
    let _ = store::write_json(&paths::cooldowns_file_for(variant), &file);
}

/// 清除某账号的冷却（默认变体，兼容壳）。
pub fn clear_cooldown(user_id: &str) -> Result<(), String> {
    clear_cooldown_for(TraeVariant::default(), user_id)
}

/// 清除某账号的冷却（按变体分家；手动或签到成功后调用）。
pub fn clear_cooldown_for(variant: TraeVariant, user_id: &str) -> Result<(), String> {
    let mut file = load_cooldowns_for(variant);
    if file.cooldowns.remove(user_id).is_some() {
        file.updated_at = Some(store::now_iso());
        store::write_json(&paths::cooldowns_file_for(variant), &file)?;
    }
    Ok(())
}

/// 清除**该变体**全部账号的冷却（默认变体，兼容壳）。
pub fn clear_all_cooldowns() -> Result<usize, String> {
    clear_all_cooldowns_for(TraeVariant::default())
}

/// 清除**该变体**全部账号的冷却（按变体分家），返回被清除的条数。
///
/// 用途：该变体所有账号同时被冷却导致大面积失败时的一键恢复。
/// 范围限定在变体内 —— 「清空 Trae Work 的冷却」不该顺手解冻 Trae CN 的账号。
pub fn clear_all_cooldowns_for(variant: TraeVariant) -> Result<usize, String> {
    let mut file = load_cooldowns_for(variant);
    let count = file.cooldowns.len();
    if count > 0 {
        file.cooldowns.clear();
        file.updated_at = Some(store::now_iso());
        store::write_json(&paths::cooldowns_file_for(variant), &file)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::trae::device::DeviceEntry;

    fn sample_device() -> DeviceEntry {
        DeviceEntry {
            device_id: "123456789012345".into(),
            market_user_id: Some("11111111-2222-4333-8444-555555555555".into()),
            session_id: Some("ab".repeat(32)),
            created: None,
            gen: 2,
        }
    }

    /// 签到/积分请求失败时**必须**走全仓统一的传输层文案出口
    /// （`net::describe_transport_error`），否则又会退化成只剩一行
    /// 「error sending request for url (…)」、看不出原因的提示。
    /// 该出口的行为由 `modules/net.rs` 的单测钉住，这里不再重复。
    #[test]
    fn parse_credit_packages_extracts_per_package_details() {
        let now = 1_000_000_i64;
        let packs = vec![
            serde_json::json!({
                "entitlement_base_info": {
                    "package_name": "月卡",
                    "package_code": "month",
                    "quota": { "credits_limit": 100.0 }
                },
                "usage": { "credits_amount": 30.0 },
                "expire_time": now + 3 * 24 * 3600,
            }),
            serde_json::json!({
                "entitlement_base_info": {
                    "name": "年卡",
                    "quota": { "credits_limit": 500.0 }
                },
                "usage": { "credits_amount": 600.0 },
                "expire_time": now - 10,
            }),
            // 无额度上限的包：不参与剩余量统计，也不应出现在明细里。
            serde_json::json!({ "entitlement_base_info": { "quota": {} } }),
        ];
        let packages = parse_credit_packages(&packs, now);
        assert_eq!(packages.len(), 2, "无额度上限的包应被跳过");

        assert_eq!(packages[0].package_name.as_deref(), Some("月卡"));
        assert_eq!(packages[0].package_code.as_deref(), Some("month"));
        assert_eq!(packages[0].total, 100.0);
        assert_eq!(packages[0].remaining, 70.0);
        assert_eq!(packages[0].used, 30.0);
        assert!(packages[0].expiring_soon, "3 天内到期应为 expiringSoon");
        assert!(!packages[0].expired);

        // 已用超过额度 → remaining 下限 0；已过期 → expired 且不再 expiringSoon。
        assert_eq!(packages[1].remaining, 0.0);
        assert!(packages[1].expired);
        assert!(!packages[1].expiring_soon);
    }

    /// ★ 临期窗口的**边界**（issue #83 · 3）：14 天以内算临期，超过不算。
    ///
    /// 这条用例的价值不在于「验证一个常量」，而在于把**产品口径**钉住：
    /// 窗口从 7 天放宽到 14 天是刻意的（7 天常常在用户下次打开之前就滑过去了）。
    /// 变异：把 `EXPIRING_SOON_SECS` 改回 7 天 ⇒ 13 天那条变红；改成 30 天 ⇒ 15 天那条变红。
    #[test]
    fn expiring_soon_window_is_fourteen_days() {
        let now = 1_000_000_i64;
        let pack = |days: i64| {
            serde_json::json!({
                "entitlement_base_info": { "quota": { "credits_limit": 10.0 } },
                "expire_time": now + days * 24 * 3600,
            })
        };
        let near = parse_credit_packages(&[pack(13)], now);
        assert!(near[0].expiring_soon, "13 天后到期应算临期（窗口 14 天）");
        let far = parse_credit_packages(&[pack(15)], now);
        assert!(!far[0].expiring_soon, "15 天后到期不应算临期（窗口 14 天）");
    }

    /// ★★ 护栏（issue #7）：「最早到期」**不得**由已用完的包决定。
    ///
    /// 现场：某账号真正还有余额的包 10/30 才到期，但它有一条**已用尽**的包
    /// 09/30 到期（徽标因此显示「09/30 到期」，而卡片列表里第一条是 10/30 ——
    /// 列表只显示还有剩余的包）。旧口径把这条空包算进「最早到期」，于是该账号的
    /// 到期紧迫度被顶到 ≈1.0，恒压过真正今天到期的账号 ⇒「老是消耗那个几十天后
    /// 才到期的账号」（issue #7）。
    #[test]
    fn earliest_live_expire_ignores_fully_consumed_pack() {
        let now = 1_000_000_i64;
        let dead_but_soon = serde_json::json!({
            "entitlement_base_info": { "quota": { "credits_limit": 500.0 } },
            "usage": { "credits_amount": 500.0 }, // 剩余 0
            "expire_time": now + 100,             // 但到期时刻很近
        });
        let live_but_late = serde_json::json!({
            "entitlement_base_info": { "quota": { "credits_limit": 4_000.0 } },
            "usage": { "credits_amount": 6.21 },
            "expire_time": now + 30 * 86_400,
        });

        let packs = vec![dead_but_soon, live_but_late];
        assert_eq!(
            earliest_live_expire(&packs, now),
            Some(now + 30 * 86_400),
            "空包（剩余 0）不得把账号伪装成快过期"
        );
    }

    /// 已过期的包不参与：额度已作废，不构成紧迫度（既有语义，一并钉住）。
    #[test]
    fn earliest_live_expire_ignores_expired_pack() {
        let now = 1_000_000_i64;
        let expired = serde_json::json!({
            "entitlement_base_info": { "quota": { "credits_limit": 100.0 } },
            "usage": { "credits_amount": 0.0 },
            "expire_time": now - 1,
        });
        let live = serde_json::json!({
            "entitlement_base_info": { "quota": { "credits_limit": 100.0 } },
            "usage": { "credits_amount": 0.0 },
            "expire_time": now + 5_000,
        });
        assert_eq!(
            earliest_live_expire(&[expired, live], now),
            Some(now + 5_000),
            "已过期的包不得参与"
        );
    }

    /// 多个活包取最早；无额度上限 / 无到期时间 / 全空包的组合返回 `None`
    /// （`None` 在选号里对应紧迫度 0，即「无到期信息」——中性降级，不是惩罚）。
    #[test]
    fn earliest_live_expire_takes_min_among_live_and_none_when_no_evidence() {
        let now = 1_000_000_i64;
        let pack = |limit: f64, used: f64, expire: Option<i64>| {
            let mut value = serde_json::json!({
                "entitlement_base_info": { "quota": { "credits_limit": limit } },
                "usage": { "credits_amount": used },
            });
            if let Some(expire) = expire {
                value["expire_time"] = serde_json::json!(expire);
            }
            value
        };

        let live = vec![
            pack(100.0, 0.0, Some(now + 9_000)),
            pack(100.0, 0.0, Some(now + 3_000)),
            pack(100.0, 0.0, None),
        ];
        assert_eq!(earliest_live_expire(&live, now), Some(now + 3_000));

        // 无额度上限的包（可能是无限制资源）不参与统计。
        let unlimited = vec![serde_json::json!({ "entitlement_base_info": { "quota": {} } })];
        assert_eq!(earliest_live_expire(&unlimited, now), None);

        // 全部用完 ⇒ 无紧迫度证据。
        let all_dead = vec![pack(100.0, 100.0, Some(now + 100))];
        assert_eq!(earliest_live_expire(&all_dead, now), None);
    }

    #[test]
    fn old_remaining_file_without_packages_deserializes() {
        // R4：旧 remaining_credits.json（无 packages 键）必须能读，且 packages 回落为空。
        let old = r#"{"credits":{"u1":12.5},"expire_times":{"u1":1700000000},"updated_at":"2026-01-01T00:00:00Z"}"#;
        let parsed: RemainingCreditsFile =
            serde_json::from_str(old).expect("旧结构必须可反序列化（R4：升级不得 panic）");
        assert_eq!(parsed.credits.get("u1"), Some(&12.5));
        assert!(parsed.packages.is_empty(), "缺失的 packages 应回落为空 map");
    }

    #[test]
    fn credit_package_serializes_camel_case() {
        let package = CreditPackage {
            package_code: Some("month".into()),
            package_name: Some("月卡".into()),
            total: 100.0,
            remaining: 70.0,
            used: 30.0,
            expire_at: Some(1),
            expired: false,
            expiring_soon: true,
        };
        let value = serde_json::to_value(&package).expect("序列化");
        for key in [
            "packageCode",
            "packageName",
            "total",
            "remaining",
            "used",
            "expireAt",
            "expired",
            "expiringSoon",
        ] {
            assert!(value.get(key).is_some(), "缺少 camelCase 键 {key}");
        }
        assert!(value.get("expire_at").is_none(), "不得出现 snake_case 键");
    }

    #[test]
    fn headers_carry_per_account_device_identity() {
        let headers = build_headers("abc", &sample_device());
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Cloud-IDE-JWT abc")
        );
        assert_eq!(
            headers.get("x-device-id").map(String::as_str),
            Some("123456789012345")
        );
        assert_eq!(
            headers.get("vscode-sessionid").map(String::as_str),
            Some("ab".repeat(32).as_str())
        );
        assert_eq!(
            headers.get("x-market-user-id").map(String::as_str),
            Some("11111111-2222-4333-8444-555555555555")
        );
        // 绝不能声明 accept-encoding：本 crate 的 reqwest 未启用 gzip，无法解压。
        assert!(
            !headers.contains_key("accept-encoding"),
            "声明 gzip 会导致响应体无法解析"
        );
        // 已带前缀的 JWT 不应被二次加前缀
        let headers = build_headers("Cloud-IDE-JWT abc", &sample_device());
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Cloud-IDE-JWT abc")
        );
    }

    #[test]
    fn headers_tolerate_device_without_optional_fields() {
        let device = DeviceEntry {
            device_id: "d".into(),
            market_user_id: None,
            session_id: None,
            created: None,
            gen: 2,
        };
        let headers = build_headers("t", &device);
        // 缺失可选字段时下发空串而不是 panic
        assert_eq!(
            headers.get("x-market-user-id").map(String::as_str),
            Some("")
        );
        assert_eq!(
            headers.get("vscode-sessionid").map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn trace_id_has_expected_shape() {
        let trace = trace_id();
        assert!(trace.starts_with("00-"), "{trace}");
        assert!(trace.ends_with("-01"), "{trace}");
        assert_eq!(trace.len(), 3 + 16 + 3, "{trace}");
        let middle = &trace[3..19];
        assert!(middle.chars().all(|c| c.is_ascii_hexdigit()), "{trace}");
    }

    #[test]
    fn classify_error_maps_known_cases() {
        assert_eq!(classify_error(200, Some(1005), ""), ("PlanLimit", 43_200));
        assert_eq!(classify_error(429, None, ""), ("SoftRate", 60));
        // ★ issue #108：裸 401 是浅冷却（可自愈）；带作废标记才是永久。
        assert_eq!(classify_error(401, None, ""), ("AuthRejected", 60));
        assert_eq!(classify_error(403, None, ""), ("AuthRejected", 60));
        assert_eq!(
            classify_error(401, None, r#"{"code":12153}"#),
            ("SessionDead", -1)
        );
        assert_eq!(
            classify_error(200, None, "Offline user session not found"),
            ("SessionDead", -1),
            "作废标记优先于状态码（上游可能用 200 返回业务错误）"
        );
        assert_eq!(classify_error(404, None, ""), ("NotFound", 60));
        assert_eq!(classify_error(503, None, ""), ("Server", 600));
        assert_eq!(classify_error(400, None, ""), ("Client", 600));
        assert_eq!(classify_error(200, Some(7), ""), ("BusinessError", 300));
        assert_eq!(classify_error(200, Some(0), ""), ("Unknown", 0));
        assert_eq!(classify_error(200, None, ""), ("Unknown", 0));
    }

    #[test]
    fn classify_error_plan_limit_only_on_200() {
        // 同样 code=1005 但 HTTP 状态非 200 时不能误判为套餐限制。
        assert_eq!(classify_error(500, Some(1005), ""), ("Server", 600));
    }

    /// ★★ issue #108 §P0-C：「积分拉到了」必须成为**解冻证据**（含 `SessionDead`）。
    ///
    /// 变异：把 `should_thaw` 改回「排除 SessionDead」⇒ 第三条断言红。
    #[test]
    fn thaw_accepts_session_dead_because_a_successful_fetch_proves_the_session() {
        let dead = CooldownEntry {
            error_type: "SessionDead".into(),
            until: 9_999_999_999,
            reason: "401".into(),
            error_count: 0,
            credential_exp: None,
            judged_at: None,
        };
        assert!(
            should_thaw(1.0, Some(&dead)),
            "取积分成功 ⇒ 凭据有效 ⇒ 永久冷却必须能自愈"
        );
        // 反向对照：积分 0 不是「会话可用」的证据（可能只是额度用完），不解冻。
        assert!(!should_thaw(0.0, Some(&dead)));
        // 无冷却记录 ⇒ 无事可做。
        assert!(!should_thaw(1.0, None));
        // 空类型是「占位记录」，不清（与 `until > now && !error_type.is_empty()` 同口径）。
        let empty = CooldownEntry {
            error_type: String::new(),
            ..dead.clone()
        };
        assert!(!should_thaw(1.0, Some(&empty)));
    }

    /// ★★ issue #108 §P0-A：凭据指纹必须**双向**成立。
    ///
    /// - `exp` 变了 ⇒ 判定失效（重新登录 / 续期后自动恢复）；
    /// - `exp` 未变 ⇒ 判定继续生效（否则保活刷新会洗白所有禁用，三振机制被取消）。
    #[test]
    fn credential_fingerprint_only_invalidates_when_exp_actually_changed() {
        let entry = CooldownEntry {
            error_type: "SessionDead".into(),
            until: 9_999_999_999,
            reason: "401".into(),
            error_count: 0,
            credential_exp: Some(1_700_000_000),
            judged_at: None,
        };
        assert!(
            !entry.matches_credential(Some(1_800_000_000)),
            "换了新令牌（exp 变）⇒ 旧判定必须失效"
        );
        assert!(
            entry.matches_credential(Some(1_700_000_000)),
            "exp 未变（只换了 access token 原文）⇒ 判定必须继续生效"
        );
        // 拿不到证据时保守：不放行。
        assert!(entry.matches_credential(None));
        // 旧数据没有指纹 ⇒ 照旧生效（不会误放行）。
        let legacy = CooldownEntry {
            credential_exp: None,
            ..entry.clone()
        };
        assert!(legacy.matches_credential(Some(1_800_000_000)));
    }

    #[test]
    fn session_dead_cooldown_is_effectively_permanent() {
        // **带作废标记**的 401：until 必须远大于任何现实时间，否则会话失效会被反复重试。
        // ★ issue #108：判据从「状态码 401」改成「body 里有 12153」—— 裸 401 已降级为
        //   60 秒浅冷却（见 `classify_error_separates_session_dead_from_plain_401`）。
        let now = chrono::Local::now().timestamp();
        let (kind, seconds) = classify_error(401, None, r#"{"code":12153}"#);
        assert_eq!(kind, "SessionDead");
        assert_eq!(seconds, -1);
        assert!(9_999_999_999_i64 > now + 100 * 365 * 86_400);

        // 反向对照：裸 401 **不得**是永久（否则可自愈的 token 过期被打成终态）。
        let (kind, seconds) = classify_error(401, None, "");
        assert_eq!(kind, "AuthRejected");
        assert_eq!(seconds, 60, "裸 401 必须是浅冷却，且不禁用");
    }

    #[test]
    fn daily_snapshot_earned_and_consumed_are_rounded() {
        assert_eq!(round2(12.345), 12.35);
        assert_eq!(round2(12.344), 12.34);
        assert_eq!(round2(0.0), 0.0);
        // 浮点尾差不应外泄到 UI
        assert_eq!(round2(0.1 + 0.2), 0.3);
    }

    #[test]
    fn summary_defaults_are_zero_not_null() {
        let summary = CheckinSummary::default();
        assert_eq!(summary.total_ok, 0);
        assert_eq!(summary.already, 0);
        assert_eq!(summary.failed, 0);
        assert!(summary.results.is_empty());
    }

    #[test]
    fn cooldown_entry_uses_type_key_in_json() {
        // 持久化键名是 `type`，与参考实现一致（不是 error_type）。
        let entry = CooldownEntry {
            error_type: "Server".into(),
            until: 1,
            reason: "r".into(),
            error_count: 2,
            credential_exp: None,
            judged_at: None,
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value.get("type").unwrap().as_str(), Some("Server"));
        assert!(value.get("error_type").is_none());
        // 反序列化也要认 `type`
        let parsed: CooldownEntry =
            serde_json::from_value(serde_json::json!({"type":"Server","until":5})).unwrap();
        assert_eq!(parsed.error_type, "Server");
        assert_eq!(parsed.until, 5);
    }

    #[test]
    fn truncate_limits_by_chars_not_bytes() {
        // 中文按字符截断，不能按字节（会切出半个 UTF-8 序列）。
        assert_eq!(truncate("中文测试文本", 2), "中文");
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn history_retention_days_is_positive() {
        // 保留天数为 0 或负数会让明细被立即清空。常量断言放到编译期（`const _`）：
        // 运行时 `assert!` 对常量是恒真，测不出任何东西，clippy 的 assertions_on_constants 正是这个意思。
        const _: () = assert!(crate::modules::trae::TRAE_HISTORY_KEEP_DAYS > 0);
    }

    /// 钉死上游请求头的固定取值。
    ///
    /// 这些值来自抓包复刻（见常量区注释），**无法从客户端源码侧核对**，
    /// 所以一旦改动就可能静默地把签到/积分打挂 —— 而失败表现只是
    /// 「接口报错」或「额度查不到」，很难联想到是 UA 被改了。
    /// 本测试就是那道拦截：改值必须是有意为之，并同步更新这里的期望。
    #[test]
    fn upstream_headers_are_pinned_to_captured_values() {
        assert_eq!(user_agent(), "VSCode 1.107.1 (TRAE SOLO CN)");
        assert_eq!(market_client_id(), "VSCode 1.107.1");

        let headers = build_headers("abc", &sample_device());
        for (name, expected) in [
            ("user-agent", "VSCode 1.107.1 (TRAE SOLO CN)"),
            ("x-market-client-id", "VSCode 1.107.1"),
            ("x-lscbd-aid", "787976"),
            ("x-lgw-req-sdk-type", "3"),
            ("package-type", "stable_cn"),
            ("x-user-region", "CN"),
        ] {
            assert_eq!(
                headers.get(name).map(String::as_str),
                Some(expected),
                "请求头 {name} 的取值被改动了 —— 它来自抓包复刻，改动前请先确认服务端行为"
            );
        }
    }

    /// `x-market-client-id` **不带**产品名，`user-agent` **带** —— 两者不能混用。
    ///
    /// 这是最容易"顺手统一"的一处：看到 UA 里有 `(TRAE SOLO CN)` 就以为
    /// client-id 也该带上，结果两个头的形态都变。
    #[test]
    fn market_client_id_omits_product_while_user_agent_keeps_it() {
        assert!(user_agent().contains('('));
        assert!(!market_client_id().contains('('));
        assert_eq!(
            market_client_id().trim_start_matches("VSCode "),
            TRAE_VSCODE_VERSION
        );
    }

    /// 台账：跨运行累积、幂等、跨日重置。
    ///
    /// 「跨运行累积」是本文件存在的**唯一理由**（摘要每轮被覆盖，见 `CheckinLedger`
    /// 的文档）；「跨日重置」防止昨天的已签到被当成今天。
    #[test]
    fn checkin_ledger_accumulates_across_runs_and_resets_across_days() {
        let _env = crate::modules::trae::test_support::TempEnv::with_device_fixture();
        let variant = TraeVariant::default();

        mark_checked_in_for(variant, "u1").unwrap();
        mark_checked_in_for(variant, "u2").unwrap();
        // 幂等：同一账号在同一轮里被标记两次（如 claim 成功后又补记）不产生重复项。
        mark_checked_in_for(variant, "u1").unwrap();
        assert_eq!(
            load_ledger_for(variant).checked_in,
            vec!["u1".to_string(), "u2".to_string()],
            "台账必须跨调用累积且去重"
        );

        // 跨日：把落盘日期改成过去 ⇒ 读出来是空台账，且**读路径不写盘**。
        let path = paths::checkin_ledger_file_for(variant);
        let mut stale: CheckinLedger = store::read_json(&path);
        stale.date = "2000-01-01".to_string();
        store::write_json(&path, &stale).unwrap();
        assert!(
            load_ledger_for(variant).checked_in.is_empty(),
            "过期台账不得参与判定"
        );
        let on_disk: CheckinLedger = store::read_json(&path);
        assert_eq!(on_disk.date, "2000-01-01", "读路径不得顺手清盘");

        // 再标记一次：以今天重建，且**不含**昨天的条目。
        mark_checked_in_for(variant, "u3").unwrap();
        let rebuilt = load_ledger_for(variant);
        assert_eq!(rebuilt.date, store::today());
        assert_eq!(rebuilt.checked_in, vec!["u3".to_string()]);
    }

    /// `checked_in_today_for` 必须把**当日积分明细**并进来 —— 升级当天台账还是空的，
    /// 只读台账会让「今天已经签过」的账号错误地显示「未签到」（正是本次要修的症状）。
    #[test]
    fn checked_in_today_unions_ledger_with_today_history() {
        let _env = crate::modules::trae::test_support::TempEnv::with_device_fixture();
        let variant = TraeVariant::default();

        // 只有明细（模拟升级前签过、台账还没建立）。
        append_history_for(variant, "u-history", 150, 150).unwrap();
        // 只有台账（模拟 claim 成功但预检没给出额度、明细因此没写）。
        mark_checked_in_for(variant, "u-ledger").unwrap();
        // 昨天的明细不得计入今天。
        let mut history = load_history_for(variant);
        history.records.push(CreditRecord {
            date: "2000-01-01".into(),
            user_id: "u-yesterday".into(),
            credits: 100,
            delta: 100,
        });
        store::write_json(&paths::credits_history_file_for(variant), &history).unwrap();

        let checked = checked_in_today_for(variant);
        assert!(
            checked.contains("u-history"),
            "当日明细里的账号必须算已签到（升级当天台账为空）"
        );
        assert!(checked.contains("u-ledger"), "台账里的账号必须算已签到");
        assert!(!checked.contains("u-yesterday"), "昨天的明细不得算进今天");
    }
}
