//! Qoder 平台（第三个产品线）—— 阿里 Qoder 的双区账号管理。
//!
//! ## 与 WorkBuddy / Trae 的关系
//!
//! 三者是**平行**的产品线，共用 L0 的 [`Product`](crate::modules::product::Product) /
//! [`Scope`](crate::modules::product::Scope) 抽象，各自实现账号库、令牌、签到、积分与网关。
//! ⚠️ **刻意不复用** WorkBuddy 的 `region::RegionSpec`：Qoder 的区域常量（端点 / 客户端 id /
//! 本地目录）与 WorkBuddy 毫无交集，硬塞进同一张表只会让两边都长出一堆「本平台不适用」的字段。
//! 这与 Trae 侧的处理一致（`trae/region.rs` 自带一套区域轴）。
//!
//! ## 模块地图
//!
//! | 文件 | 职责 |
//! |:--|:--|
//! | [`region`] | 双区域常量表（端点 / 客户端 id / 本地目录 / OAuth 差异） |
//! | [`token`] | 令牌家族与前缀语义（`pt-` / `dt-` / `jrt-` …） |
//! | [`paths`] | 全部路径派生（只从枚举派生，原始 scope 串永不进 `PathBuf`） |
//! | [`account`] | 账号模型 + 账号库读写 |
//! | [`device`] | 稳定设备指纹派生 |
//! | [`cosy`] | COSY 推理签名（自定义 base64 / RSA 包裹 / AES / md5 Bearer） |
//! | [`api`] | 上游协议客户端（COSY 通道聊天/清单 + openapi 通道刷新/签到/额度） |
//! | [`refresh`] | 账号令牌刷新（device / job / PAT 三条路由 + 回写） |
//! | [`checkin`] | 签到状态与领取（`/sash` 通道；国际版未开放如实上报） |
//! | [`credits`] | 额度 / 套餐视图（逐账号 + 区域汇总） |
//! | [`web_usage`] | **网页端**逐资源包明细（只认会话 Cookie，issue #111） |
//! | [`web_session`] | 网页会话（Cookie）的本地缓存（0600、7 天保鲜、按 uid 归属） |
//! | [`store`] | 落盘助手（原子写 + JSON 读写 + 脱敏） |
//!
//! ## 令牌家族（前缀语义）
//!
//! | 前缀 | 含义 | 刷新端点 |
//! |:--|:--|:--|
//! | `pt-` | PAT（个人访问令牌，长期兜底） | 不刷新；用于**交换**出 jobToken |
//! | `dt-` | 设备流访问令牌 | `drt-` 走 deviceToken 刷新 |
//! | `drt-` | 设备流刷新令牌 | — |
//! | `jt-` | jobToken 访问令牌 | `jrt-` 走 jobToken 刷新 |
//! | `jrt-` | jobToken 刷新令牌 | — |
//!
//! ★ **刷新按前缀路由**，不是按区域：拿错端点会得到 401 而不是「静默失败」。

pub mod account;
pub mod api;
pub mod campaign;
pub mod checkin;
pub mod cosy;
pub mod credits;
pub mod device;
pub mod export_import;
pub mod native;
pub mod oauth;
pub mod paths;
pub mod profile;
pub mod refresh;
pub mod region;
pub mod store;
pub mod switch;
pub mod token;
pub mod trash;
pub mod usage;
pub mod web_proxy;
pub mod web_session;
pub mod web_usage;

// ---------------------------------------------------------------------------
// 上游 API 路径（两个区域共用；端点基址在 `region::realm_spec` 里按区查表）
// ---------------------------------------------------------------------------

/// 设备授权：发起。
pub const PATH_DEVICE_AUTHORIZE: &str = "/api/v1/deviceToken/authorize";
/// 设备授权：轮询换取令牌。
pub const PATH_DEVICE_TOKEN_POLL: &str = "/api/v1/deviceToken/poll";
/// 设备流令牌刷新。
pub const PATH_DEVICE_TOKEN_REFRESH: &str = "/api/v1/deviceToken/refresh";
/// PAT → jobToken 交换。
pub const PATH_JOB_TOKEN_EXCHANGE: &str = "/api/v1/jobToken/exchange";
/// jobToken 刷新。
pub const PATH_JOB_TOKEN_REFRESH: &str = "/api/v1/jobToken/refresh";
/// 额度 / 用量。
pub const PATH_QUOTA_USAGE: &str = "/api/v2/quota/usage";
/// 套餐信息。
pub const PATH_USER_PLAN: &str = "/api/v2/user/plan";
/// 模型清单（动态源）。
pub const PATH_MODEL_LIST: &str = "/algo/api/v2/model/list";
/// 活动列表（签到 / 福利领取的入口）。
///
/// ★ 这是**当前**官方的签到机制：官方把「每日领取 100 Credits」等限时活动搬到了
/// 活动平台，`/sash/api/v1/me/daily-check-in/*` 只在仍开放时作为兜底（见 [`campaign`]）。
pub const PATH_CAMPAIGNS: &str = "/sash/api/v1/me/campaigns";
/// 签到：状态查询（`/sash` 通道，与 `/api` 通道不同 —— 别拼错）。
pub const PATH_CHECKIN_STATUS: &str = "/sash/api/v1/me/daily-check-in/status";
/// 签到：领取。
pub const PATH_CHECKIN_CLAIM: &str = "/sash/api/v1/me/daily-check-in/claim";
/// Pro 升级奖励**资格查询**（`eligibility → claim` 两步走的第一步）。
pub const PATH_PRO_ELIGIBILITY: &str = "/sash/api/v1/me/pro-upgrade/eligibility";
/// Pro 升级奖励领取。
pub const PATH_PRO_CLAIM: &str = "/sash/api/v1/me/pro-upgrade/claim";
/// 单活动**奖励发放状态**查询（幂等、只读；`{id}` 位置替换 campaign id）。
pub const PATH_CAMPAIGN_REWARD_TEMPLATE: &str = "/sash/api/v1/me/campaigns/{id}/reward";
/// 单活动领取（`{id}` 位置替换 campaign id）。
pub const PATH_CAMPAIGN_CLAIM_TEMPLATE: &str = "/sash/api/v1/me/campaigns/{id}/claim";
/// 推理（SSE）：对话生成。
///
/// ⚠️ 请求体要过 [`cosy::qoder_encode`] 编码、整体走 COSY 签名头 —— 见 `cosy` 模块头。
///
/// ⚠️ **签名路径**（不带 query）；实际**发送**要用 [`PATH_CHAT_SSE_ENCODED`]。
/// 上游按 `?Encode=1` 决定「先解码再按 JSON 解析」还是「直接按 JSON 解析」：
/// 带编码体打这条**裸路径**，上游会把自定义 base64 当 JSON 读 ⇒ HTTP 200 +
/// `event:error` + Jackson `JsonParseException`（真机取证 2026-10-17，Qoder CN）。
/// 与模型清单 [`api::PATH_MODELS_ENCODED`] 同一个约定，别只给清单加、忘了推理。
pub const PATH_CHAT_SSE: &str = "/algo/api/v2/service/pro/sse/agent_chat_generation";
/// 推理（SSE）的**请求路径**：与 [`PATH_CHAT_SSE`] 同一端点，带 `Encode=1`。
///
/// 签名仍然只覆盖 [`PATH_CHAT_SSE`]（query 不参与签名 —— 见 `api` 模块头对
/// `PATH_MODELS_SIGN` 的同款说明）。
pub const PATH_CHAT_SSE_ENCODED: &str =
    "/algo/api/v2/service/pro/sse/agent_chat_generation?Encode=1";

/// 签到活动的奖励数值（「每日领取 100 Credits」）。
///
/// ⚠️ 这是**展示兜底**：真实数值以上游返回为准。写在这里是为了让 UI 在
/// 「尚未拉到活动详情」时也有一个**诚实**的占位，而不是显示 0 或空白。
pub const DAILY_CHECKIN_REWARD: u32 = 100;
