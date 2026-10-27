//! 上游客户端：chat 流式 / token 刷新 / 目录 / 积分。
//!
//! 对照参考实现 `upstream.ts`，逐字段移植其 wire 行为：
//! - 强制 `stream: true`（上游不接受非流式）；
//! - `role: "developer"` → `role: "system"`（上游拒 developer，400/11128）；
//! - `tool_choice` 对象形态扁平化为字符串，`none` 时删除 `tool_choice` 与
//!   `tools`/`functions`；
//! - 国际版附加：messages 首条若非 `system`，**前置**一条最小 system 消息；
//! - chat 请求**绝不携带 refresh token**（`X-Refresh-Token` 仅出现在刷新端点）。

use std::collections::HashMap;

use serde_json::{json, Map, Value};
use tokio::sync::watch;

use crate::modules::account;
use crate::modules::catalog::{parse_credits_rate, CatalogModel, CatalogReasoning};
use crate::modules::identity;
use crate::modules::net;
use crate::modules::region::{region_spec, CatalogUa, Region};

/// 共享 CLI 形态 UA（刷新 / CN 目录使用）。
pub const CLIENT_UA: &str = "CLI/2.63.2 CodeBuddy/2.63.2";

/// 错误响应体读取上限（字节）。
const ERROR_BODY_LIMIT: usize = 4096;

/// 国际版前置的最小 system 提示词（只为满足网关前置条件，不引导模型）。
pub const INTERNATIONAL_SYSTEM_PROMPT: &str = "You are a helpful assistant.";

/// 上游失败分类（对照参考实现 `classifyUpstreamError`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamErrorKind {
    /// 硬性积分不足 → 402。
    HardCredit,
    /// 软限流 → 429。
    SoftRate,
    /// 会话失效 → 401。
    SessionDead,
    /// 未找到（模型不存在等）→ 502。
    NotFound,
    /// 服务端错误 → 502。
    Server,
    /// 其他客户端错误 → 400。
    Client,
}

impl UpstreamErrorKind {
    /// 稳定的 snake_case 标识（用于错误体 `type`/`code`）。
    pub fn as_str(self) -> &'static str {
        match self {
            UpstreamErrorKind::HardCredit => "hard_credit",
            UpstreamErrorKind::SoftRate => "soft_rate",
            UpstreamErrorKind::SessionDead => "session_dead",
            UpstreamErrorKind::NotFound => "not_found",
            UpstreamErrorKind::Server => "server",
            UpstreamErrorKind::Client => "client",
        }
    }
}

/// 硬性积分不足标记（ASCII 小写 + 原样中文）。
const HARD_CREDIT_MARKERS: &[&str] = &[
    "insufficient credit",
    "no credit",
    "credit exhausted",
    "credits exhausted",
    "out of credit",
    "quota exceeded",
    "quota exhaust",
    "payment required",
    "credit not enough",
    "not enough credit",
    "积分不足",
    "额度不足",
    "余额不足",
    "积分用完",
    "额度用尽",
    "没有积分",
];

/// 会话失效标记。
/// 会话被上游**作废**的标记（出现在响应体里）。
///
/// ★ 跨产品线**唯一来源**：WorkBuddy（`gateway::pool::classify_event`）与 Trae
/// （`gateway::trae::pool::classify_http_with_body` / `classify_solo`）都必须读这一份。
///
/// issue #108 的教训：Trae 侧此前**完全不识别** `12153`，于是「真正的会话作废」
/// 与「access token 过期」共用同一个判据（裸 401）⇒ 只能二选一，怎么选都错。
/// 必须先能识别作废，才有资格把裸 401 降级为可自愈的浅冷却。
pub const SESSION_DEAD_MARKERS: &[&str] = &["Offline user session not found", "12153"];

/// 上游失败分类（严格按参考实现的判定顺序）。
pub fn classify_upstream_error(status: u16, body: &str) -> UpstreamErrorKind {
    if status == 402 {
        return UpstreamErrorKind::HardCredit;
    }
    let lower = body.to_lowercase();
    for marker in HARD_CREDIT_MARKERS {
        if lower.contains(&marker.to_lowercase()) || body.contains(marker) {
            return UpstreamErrorKind::HardCredit;
        }
    }
    for marker in SESSION_DEAD_MARKERS {
        if body.contains(marker) {
            return UpstreamErrorKind::SessionDead;
        }
    }
    if status == 429 {
        return UpstreamErrorKind::SoftRate;
    }
    if status == 404 {
        return UpstreamErrorKind::NotFound;
    }
    if status >= 500 {
        return UpstreamErrorKind::Server;
    }
    if status >= 400 {
        return UpstreamErrorKind::Client;
    }
    UpstreamErrorKind::Client
}

/// 请求体规范化：强制 `stream:true`；`developer`→`system`；`tool_choice` 对象→字符串。
///
/// 非 JSON 或非对象输入原样返回（对照参考实现）。
pub fn prepare_chat_body(source: &str) -> String {
    let Ok(mut body) = serde_json::from_str::<Value>(source) else {
        return source.to_string();
    };
    let Some(obj) = body.as_object_mut() else {
        return source.to_string();
    };
    obj.insert("stream".to_string(), json!(true));
    normalize_developer_role(obj);
    normalize_tool_choice(obj);
    serde_json::to_string(&body).unwrap_or_else(|_| source.to_string())
}

/// `role: "developer"` → `role: "system"`。
fn normalize_developer_role(obj: &mut Map<String, Value>) {
    let Some(Value::Array(messages)) = obj.get_mut("messages") else {
        return;
    };
    for message in messages.iter_mut() {
        if let Some(wrapped) = message.as_object_mut() {
            if wrapped.get("role").and_then(Value::as_str) == Some("developer") {
                wrapped.insert("role".to_string(), json!("system"));
            }
        }
    }
}

/// 把 OpenAI `tool_choice` 拼写改写为上游字符串形态。
fn normalize_tool_choice(obj: &mut Map<String, Value>) {
    if !obj.contains_key("tool_choice") {
        return;
    }
    let choice = obj.get("tool_choice").cloned().unwrap_or(Value::Null);
    match choice {
        Value::String(text) => {
            if text.trim().eq_ignore_ascii_case("none") {
                obj.remove("tool_choice");
                obj.remove("tools");
                obj.remove("functions");
            }
        }
        Value::Object(wrapped) => {
            let kind = wrapped
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_lowercase();
            match kind.as_str() {
                "none" => {
                    obj.remove("tool_choice");
                    obj.remove("tools");
                    obj.remove("functions");
                }
                "auto" | "required" => {
                    obj.insert("tool_choice".to_string(), json!(kind));
                }
                "function" => {
                    let mut name = wrapped
                        .get("function")
                        .and_then(Value::as_object)
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        if let Some(fallback) = wrapped.get("name").and_then(Value::as_str) {
                            name = fallback.to_string();
                        }
                    }
                    let name = name.trim().to_string();
                    let value = if name.is_empty() {
                        "auto".to_string()
                    } else {
                        name
                    };
                    obj.insert("tool_choice".to_string(), json!(value));
                }
                _ => {
                    obj.remove("tool_choice");
                }
            }
        }
        _ => {
            obj.remove("tool_choice");
        }
    }
}

/// 国际版附加规范化：首条消息非 `system` 时前置最小 system 消息（不合并、不改序）。
pub fn prepare_international_chat_body(source: &str) -> String {
    let prepared = prepare_chat_body(source);
    let Ok(mut body) = serde_json::from_str::<Value>(&prepared) else {
        return prepared;
    };
    let Some(obj) = body.as_object_mut() else {
        return prepared;
    };
    let Some(Value::Array(messages)) = obj.get_mut("messages") else {
        return prepared;
    };
    let first_is_system = messages
        .first()
        .and_then(Value::as_object)
        .and_then(|m| m.get("role"))
        .and_then(Value::as_str)
        == Some("system");
    if first_is_system {
        return prepared;
    }
    messages.insert(
        0,
        json!({ "role": "system", "content": INTERNATIONAL_SYSTEM_PROMPT }),
    );
    serde_json::to_string(&body).unwrap_or(prepared)
}

/// 按 region 规范化 chat 请求体（纯函数，无 I/O、无状态）。
///
/// 分派规则：
/// - [`Region::Cn`]：直传已由调用方规范化的 `body_json`（`to_string` 拷贝）；
/// - [`Region::Global`]：走 [`prepare_international_chat_body`]
///   （内部已含 [`prepare_chat_body`]，幂等）。
///
/// 从 [`UpstreamClient::chat_stream`] 内联的 `match region { ... }` 提取而来，
/// 行为与提取前逐字节一致；提取目的是让该分派可被无 I/O 的单测直接覆盖。
fn chat_body_for(region: Region, body_json: &str) -> String {
    match region {
        Region::Global => prepare_international_chat_body(body_json),
        Region::Cn => body_json.to_string(),
    }
}

/// 上游失败（含状态码与分类）。
#[derive(Debug, Clone)]
pub struct UpstreamFailure {
    /// HTTP 状态码；传输错误为 0。
    pub status: u16,
    /// 分类。
    pub kind: UpstreamErrorKind,
    /// 可读消息（上游响应体片段）。
    pub message: String,
}

/// token 刷新结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshOutcome {
    /// 新 access token。
    pub access_token: String,
    /// 新 refresh token（若上游返回）。
    pub refresh_token: Option<String>,
    /// 相对过期秒数（若上游返回）。
    pub expires_in_sec: Option<i64>,
    /// 域（若上游返回）。
    pub domain: Option<String>,
}

/// 一个积分套餐及余额。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreditAccount {
    /// 套餐名。
    pub package_name: String,
    /// 剩余额度。
    pub remain: f64,
    /// 套餐容量。
    pub size: f64,
}

/// 聚合积分结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Credits {
    /// 总剩余。
    pub total: f64,
    /// 各套餐明细。
    pub accounts: Vec<CreditAccount>,
}

/// chat 成功结果：携带原始流式响应。
pub struct UpstreamChatOk {
    /// 上游 SSE 响应，调用方用 `bytes_stream()` 透传。
    pub response: reqwest::Response,
}

/// chat 结果：成功（流式响应）或分类失败。
pub enum UpstreamChatResult {
    /// 上游 2xx。
    Ok(UpstreamChatOk),
    /// 上游非 2xx 或传输错误。
    Err {
        /// HTTP 状态码；传输错误为 0。
        status: u16,
        /// 分类。
        kind: UpstreamErrorKind,
        /// 响应体片段。
        message: String,
    },
}

/// 上游 HTTP 客户端。一个实例服务整个网关；凭据逐请求传入。
pub struct UpstreamClient {
    http: reqwest::Client,
}

impl UpstreamClient {
    /// 用给定的 reqwest 客户端构造。
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// POST chat 端点；成功返回原始 SSE 响应。
    ///
    /// CN 直传已规范化的 body；Global 走 [`prepare_international_chat_body`]
    /// （内部已含 [`prepare_chat_body`]，幂等）。
    pub async fn chat_stream(
        &self,
        region: Region,
        acc: &Value,
        body_json: &str,
        signal: Option<watch::Receiver<bool>>,
    ) -> UpstreamChatResult {
        self.chat_stream_with(region, acc, body_json, signal, None)
            .await
    }

    /// **通用出站**：把一次 POST 送到上游并分类结果。
    ///
    /// ★ **2026-10-07 从 [`Self::chat_stream_with`] 抽出** —— 为了让「本地路由」的
    /// **业务请求转发**（P12 T03）**复用同一条出站路径**，而不是另写一份。
    ///
    /// **参数全部由调用方决定**（URL / 头 / body）⇒ 本函数**不含任何协议假设**。
    /// ⚠️ 唯一约定：**`User-Agent` 由本函数最后写入**（沿用既有 fail-closed 语义 ——
    /// 调用方无法经 `headers` 覆盖 UA）。
    pub async fn forward_with(
        &self,
        url: &str,
        headers: HashMap<String, String>,
        body: String,
        signal: Option<watch::Receiver<bool>>,
        user_agent: &str,
    ) -> UpstreamChatResult {
        // 行为与抽取前**逐字一致**（一律 POST）—— LLM 两条路径都走这里，不给它们引入任何可变性。
        self.forward_with_method(
            url,
            headers,
            body,
            signal,
            user_agent,
            reqwest::Method::POST,
        )
        .await
    }

    /// 同 [`Self::forward_with`]，但**出站方法可指定**。
    ///
    /// ## 为什么需要它（实测，不是推测）
    ///
    /// 业务白名单是**按前缀**收的，而同一个前缀下混着两种方法的端点：
    /// `/activity/growth/streak`、`/activity/growth/heatmap` 只有 **GET**，
    /// 而 `/activity/growth/redeem`、`/lottery/draw` 是 **POST**。
    /// 网关原先一律 POST 出去 ⇒ GET-only 的路径被上游回 `404 page not found`（真实环境实测）。
    ///
    /// ⇒ **业务转发必须透传客户端的原方法**；LLM 路径继续走上面的 POST 包装。
    pub async fn forward_with_method(
        &self,
        url: &str,
        mut headers: HashMap<String, String>,
        body: String,
        signal: Option<watch::Receiver<bool>>,
        user_agent: &str,
        method: reqwest::Method,
    ) -> UpstreamChatResult {
        headers.insert("User-Agent".to_string(), user_agent.to_string());

        let request = self
            .http
            .request(method, url)
            .headers(header_map(&headers))
            .body(body);
        let send = request.send();
        let response = match signal {
            Some(receiver) => {
                tokio::select! {
                    result = send => result,
                    _ = wait_for_cancel(receiver) => {
                        return UpstreamChatResult::Err {
                            status: 0,
                            kind: UpstreamErrorKind::Server,
                            message: "request aborted by caller".to_string(),
                        };
                    }
                }
            }
            None => send.await,
        };

        match response {
            Ok(response) => {
                if response.status().is_success() {
                    UpstreamChatResult::Ok(UpstreamChatOk { response })
                } else {
                    let status = response.status().as_u16();
                    let text = response.text().await.unwrap_or_default();
                    let text = text.chars().take(ERROR_BODY_LIMIT).collect::<String>();
                    let kind = classify_upstream_error(status, &text);
                    UpstreamChatResult::Err {
                        status,
                        kind,
                        message: text,
                    }
                }
            }
            Err(error) => UpstreamChatResult::Err {
                status: 0,
                kind: UpstreamErrorKind::Server,
                // 走统一出口：只有顶层一行「error sending request for url (…)」
                // 是没法排障的，网关日志需要看到具体原因（超时/拒绝/TLS）。
                message: format!("transport error: {}", net::describe_transport_error(&error)),
            },
        }
    }

    /// **业务请求出站**（P12 T03 · 本地路由）：URL = `billing_base + path`，头走 [`account::build_auth_headers`]。
    ///
    /// 与 [`Self::chat_stream_with`] 的差异**只有两处**（**路径基址 + 头构造**）——
    /// 出站与结果分类**完全复用** [`Self::forward_with`]。
    ///
    /// ⚠️ **body 原样透传**：业务请求不是 LLM 协议，**不做任何改写**（不调 `chat_body_for`）。
    pub async fn forward_biz_with(
        &self,
        region: Region,
        acc: &Value,
        path: &str,
        body_json: &str,
        signal: Option<watch::Receiver<bool>>,
        method: reqwest::Method,
    ) -> UpstreamChatResult {
        let user_agent = match self.resolve_chat_ua(region) {
            Ok(ua) => ua,
            Err(message) => {
                return UpstreamChatResult::Err {
                    status: 0,
                    kind: UpstreamErrorKind::Server,
                    message,
                }
            }
        };
        let url = format!("{}{path}", region_spec(region).billing_base);
        let headers = account::build_auth_headers(acc);
        self.forward_with_method(
            &url,
            headers,
            body_json.to_string(),
            signal,
            &user_agent,
            method,
        )
        .await
    }

    /// 同 [`Self::chat_stream`]，但允许调用方注入额外出站头（会话头族 / 派生标识）。
    ///
    /// `extra_headers` 在基础头之后写入、在 `User-Agent` 之前写入——即
    /// **UA 由本函数最终决定**，调用方无法用额外头覆盖它（fail-closed）。
    /// 空值不写入（避免发出 `X-Foo:` 这类空头）。
    pub async fn chat_stream_with(
        &self,
        region: Region,
        acc: &Value,
        body_json: &str,
        signal: Option<watch::Receiver<bool>>,
        extra_headers: Option<&HashMap<String, String>>,
    ) -> UpstreamChatResult {
        let user_agent = match self.resolve_chat_ua(region) {
            Ok(ua) => ua,
            Err(message) => {
                return UpstreamChatResult::Err {
                    status: 0,
                    kind: UpstreamErrorKind::Server,
                    message,
                }
            }
        };

        let url = format!("{}/v2/chat/completions", region_spec(region).chat_base);
        let mut headers = account::build_chat_headers(region, acc);
        if let Some(extra) = extra_headers {
            for (key, value) in extra {
                if !value.is_empty() {
                    headers.insert(key.clone(), value.clone());
                }
            }
        }
        // ★ 出站交给通用实现 —— 行为与抽取前**逐字一致**（仅把「URL/头/body 的构造」留在本函数）。
        self.forward_with(
            &url,
            headers,
            chat_body_for(region, body_json),
            signal,
            &user_agent,
        )
        .await
    }

    /// POST 刷新端点；`X-Refresh-Token` 仅在此出现。
    pub async fn refresh_token(
        &self,
        region: Region,
        acc: &Value,
    ) -> Result<RefreshOutcome, String> {
        let refresh_token = account::get_str(acc, "refresh_token").unwrap_or_default();
        if refresh_token.is_empty() {
            return Err("缺少 refresh token，无法刷新".to_string());
        }
        let url = format!(
            "{}/v2/plugin/auth/token/refresh",
            region_spec(region).chat_base
        );
        let mut headers = common_headers(region);
        headers.insert("X-Refresh-Token".to_string(), refresh_token);
        // ⚠️ **此值出处不明，且与外部实测记录冲突 —— 改动前必须先抓包确认。**
        //
        // 本仓约定（见 `modules::trae::mod` 的「契约常量必须与抓包固化值逐字节一致」）：
        // 从抓包/参考实现抄来的字面量都要有出处。这一条没有：
        // `git log -S "X-Auth-Refresh-Source"` 只能追到初始提交
        // （`chore: 初始化 Git 版本管理并纳入项目源码`），此前无版本历史，
        // 也没有任何断言把它钉在某个抓包值上。
        //
        // 外部实测记录（`ithtelab/workbuddy-manager`，`server/services/tencent.py`
        // 的 `refresh_token` 注释）称官方客户端在此头发的是 **`plugin`**：
        // 「`X-Auth-Refresh-Source: plugin` 是官方客户端的刷新渠道标识，
        //   缺了可能被风控当异常来源（上游 D3 实测）」。
        //
        // 该记录**不足以直接改**：它是另一个项目对另一个上游的观测，
        // 且刷新在当前取值下确实能成功（说明服务端不以此为门禁）。
        // 但两端取值不同意味着**必有一端不像官方客户端** —— 需要一次抓包定案。
        // 定案前不要「顺手对齐」成 `plugin`：那是拿未验证的第三方结论
        // 替换另一个未验证的取值，风险对等而收益不明。
        headers.insert("X-Auth-Refresh-Source".to_string(), "workbuddy".to_string());
        if let Some(eid) =
            account::get_str(acc, "enterpriseId").or_else(|| account::get_str(acc, "enterprise_id"))
        {
            headers.insert("X-Enterprise-Id".to_string(), eid);
        }

        let response = self
            .http
            .post(&url)
            .headers(header_map(&headers))
            .json(&json!({}))
            .send()
            .await
            .map_err(|error| net::describe_transport_error(&error))?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let document = parse_envelope(&text)
            .ok_or_else(|| format!("workbuddy upstream returned non-JSON (http {status})"))?;
        let code = document.get("code").and_then(Value::as_i64).unwrap_or(0);
        if !(200..300).contains(&status) || code != 0 {
            let message = document
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let kind = classify_upstream_error(status, &message);
            return Err(format!(
                "workbuddy upstream {} (http {status}): {message}",
                kind.as_str()
            ));
        }
        let data = document
            .get("data")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let access_token = data
            .get("accessToken")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if access_token.is_empty() {
            return Err(
                "workbuddy token refresh returned no accessToken; sign in again in the WorkBuddy app"
                    .to_string(),
            );
        }
        Ok(RefreshOutcome {
            access_token,
            refresh_token: non_empty_string(data.get("refreshToken")),
            expires_in_sec: data
                .get("expiresIn")
                .and_then(Value::as_i64)
                .filter(|value| *value > 0),
            domain: non_empty_string(data.get("domain")),
        })
    }

    /// GET 模型目录。
    ///
    /// CN 走 `/console/enterprises/personal/models`（CLI 同款），Global 走
    /// `/v3/config`（App 形态 UA，**无空格**）。
    pub async fn fetch_models(
        &self,
        region: Region,
        acc: &Value,
    ) -> Result<Vec<CatalogModel>, UpstreamFailure> {
        let spec = region_spec(region);
        let international = matches!(spec.catalog_ua, CatalogUa::App);
        let url = format!("{}{}", spec.chat_base, spec.models_path);
        let user_agent = if international {
            let info = identity::resolve_app_version(region);
            identity::app_user_agent(&info.version).unwrap_or_else(|_| CLIENT_UA.to_string())
        } else {
            CLIENT_UA.to_string()
        };

        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            format!(
                "Bearer {}",
                account::get_str(acc, "access_token").unwrap_or_default()
            ),
        );
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Origin".to_string(), spec.billing_base.to_string());
        headers.insert("Referer".to_string(), format!("{}/", spec.billing_base));
        if international {
            headers.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
            headers.insert("X-Product".to_string(), "SaaS".to_string());
        }
        headers.insert("User-Agent".to_string(), user_agent);

        let response = self
            .http
            .get(&url)
            .headers(header_map(&headers))
            .send()
            .await
            .map_err(|error| UpstreamFailure {
                status: 0,
                kind: UpstreamErrorKind::Server,
                message: net::describe_transport_error(&error),
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let Some(document) = parse_envelope(&text) else {
            let message = text.chars().take(ERROR_BODY_LIMIT).collect::<String>();
            return Err(UpstreamFailure {
                status,
                kind: classify_upstream_error(status, &message),
                message: format!("workbuddy upstream returned non-JSON (http {status})"),
            });
        };
        let code = document.get("code").and_then(Value::as_i64).unwrap_or(0);
        if !(200..300).contains(&status) || code != 0 {
            let message = document
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            return Err(UpstreamFailure {
                status,
                kind: classify_upstream_error(status, &message),
                message,
            });
        }

        // 两种目录形状：CN 恒有 `{code,msg,data}` 包装；`/v3/config` 可能返回裸文档
        // （顶层直接含 `models` 或 `agents`）。
        let data = match document.get("data") {
            Some(data) if data.is_object() => data.clone(),
            _ => {
                if document.get("models").is_some() || document.get("agents").is_some() {
                    document.clone()
                } else {
                    json!({})
                }
            }
        };
        parse_model_catalog(&data, international)
    }

    /// POST billing 端点，返回聚合剩余积分。
    pub async fn fetch_credits(
        &self,
        region: Region,
        acc: &Value,
    ) -> Result<Credits, UpstreamFailure> {
        let spec = region_spec(region);
        let url = format!("{}/v2/billing/meter/get-user-resource", spec.billing_base);
        let now = chrono::Local::now();
        let begin = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let end = (now + chrono::Duration::days(365 * 101))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let body = json!({
            "PageNumber": 1,
            "PageSize": 100,
            "ProductCode": "p_tcaca",
            "Status": [0, 3],
            "PackageEndTimeRangeBegin": begin,
            "PackageEndTimeRangeEnd": end,
        });

        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            format!(
                "Bearer {}",
                account::get_str(acc, "access_token").unwrap_or_default()
            ),
        );
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        if let Some(uid) = account::get_str(acc, "uid") {
            headers.insert("X-User-Id".to_string(), uid);
        }
        if let Some(eid) =
            account::get_str(acc, "enterpriseId").or_else(|| account::get_str(acc, "enterprise_id"))
        {
            headers.insert("X-Enterprise-Id".to_string(), eid.clone());
            headers.insert("X-Tenant-Id".to_string(), eid);
        }
        if let Some(domain) = account::get_str(acc, "domain") {
            headers.insert("X-Domain".to_string(), domain);
        }

        let response = self
            .http
            .post(&url)
            .headers(header_map(&headers))
            .json(&body)
            .send()
            .await
            .map_err(|error| UpstreamFailure {
                status: 0,
                kind: UpstreamErrorKind::Server,
                message: net::describe_transport_error(&error),
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let Some(document) = parse_envelope(&text) else {
            let message = text.chars().take(ERROR_BODY_LIMIT).collect::<String>();
            return Err(UpstreamFailure {
                status,
                kind: classify_upstream_error(status, &message),
                message: format!("workbuddy upstream returned non-JSON (http {status})"),
            });
        };
        let code = document.get("code").and_then(Value::as_i64).unwrap_or(0);
        if !(200..300).contains(&status) || code != 0 {
            let message = document
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            return Err(UpstreamFailure {
                status,
                kind: classify_upstream_error(status, &message),
                message,
            });
        }

        let wrapper = document
            .get("data")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let data = wrapper
            .get("Response")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let inner = data
            .get("Data")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let raw_accounts = inner
            .get("Accounts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut accounts = Vec::new();
        let mut total = 0.0_f64;
        for raw in raw_accounts {
            let Some(account) = raw.as_object() else {
                continue;
            };
            let number = |key: &str| account.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            let size = number("CycleCapacitySize");
            let cycle_remain = number("CycleCapacityRemain");
            let cycle_used = number("CycleCapacityUsed");
            let capacity_remain = number("CapacityRemain");
            let mut remain = if size > 0.0 || cycle_remain > 0.0 || cycle_used > 0.0 {
                cycle_remain
            } else {
                capacity_remain
            };
            if remain < 0.0 {
                remain = 0.0;
            }
            total += remain;
            accounts.push(CreditAccount {
                package_name: account
                    .get("PackageName")
                    .and_then(Value::as_str)
                    .unwrap_or("(unnamed)")
                    .to_string(),
                remain,
                size: if size > 0.0 {
                    size
                } else {
                    number("CapacitySize")
                },
            });
        }
        Ok(Credits { total, accounts })
    }

    fn resolve_chat_ua(&self, region: Region) -> Result<String, String> {
        let identity = identity::resolve_chat_identity(region);
        identity::chat_user_agent(
            region,
            &identity.client_version,
            identity.cli_version.as_deref(),
        )
        .or_else(|_| {
            identity::chat_user_agent(region, region_spec(region).fallback_app_version, None)
        })
    }
}

/// 共享 CLI 请求头（刷新 / 目录使用）。
fn common_headers(region: Region) -> HashMap<String, String> {
    let origin = region_spec(region).billing_base;
    let mut headers = HashMap::new();
    headers.insert(
        "Accept".to_string(),
        "application/json, text/plain, */*".to_string(),
    );
    headers.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
    headers.insert("Origin".to_string(), origin.to_string());
    headers.insert("Referer".to_string(), format!("{origin}/"));
    headers.insert("User-Agent".to_string(), CLIENT_UA.to_string());
    headers
}

/// 解析上游 JSON envelope（必须是对象）。
fn parse_envelope(text: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(text).ok()?;
    value.is_object().then_some(value)
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// `HashMap<String,String>` → `reqwest::header::HeaderMap`（跳过非法键值）。
fn header_map(headers: &HashMap<String, String>) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (key, value) in headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            map.insert(name, value);
        }
    }
    map
}

/// 等待取消信号为 `true`；发送端被丢弃时永不取消。
async fn wait_for_cancel(mut receiver: watch::Receiver<bool>) {
    loop {
        if *receiver.borrow() {
            return;
        }
        if receiver.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

// ---------------------------------------------------------------------------
// 目录解析（对照参考实现 parseModelCatalog）
// ---------------------------------------------------------------------------

/// 解析目录文档为模型列表。
///
/// 从 `agents` 取 `name == "cli"` 的 `models` 数组作为可用 id 白名单，再按 id 从
/// `models` 匹配；`disabled: true` 跳过；`maxInputTokens`/`maxOutputTokens` 必须为
/// 正数否则跳过。`international` 决定是否解析 `contextWindow.defaultLength`。
pub fn parse_model_catalog(
    data: &Value,
    _international: bool,
) -> Result<Vec<CatalogModel>, UpstreamFailure> {
    let raw_models = data
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let agents = data
        .get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut cli_ids: Option<Vec<String>> = None;
    for agent in &agents {
        if let Some(wrapped) = agent.as_object() {
            if wrapped.get("name").and_then(Value::as_str) == Some("cli") {
                if let Some(models) = wrapped.get("models").and_then(Value::as_array) {
                    cli_ids = Some(
                        models
                            .iter()
                            .filter_map(|id| id.as_str().map(str::to_string))
                            .collect(),
                    );
                    break;
                }
            }
        }
    }
    let Some(cli_ids) = cli_ids else {
        return Err(catalog_error(
            "workbuddy model catalog lists no cli agent models",
        ));
    };
    if cli_ids.is_empty() {
        return Err(catalog_error(
            "workbuddy model catalog lists no cli agent models",
        ));
    }

    let mut by_id: HashMap<String, CatalogModel> = HashMap::new();
    for model in &raw_models {
        let Some(wrapped) = model.as_object() else {
            continue;
        };
        let id = wrapped
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if id.is_empty() || wrapped.get("disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let input = wrapped
            .get("maxInputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output = wrapped
            .get("maxOutputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if input == 0 || output == 0 {
            continue;
        }
        // ★ 上游给两档：`maxInputTokens`/`maxAllowedSize` 是**上限**，
        // `contextWindow.defaultLength` 是**默认**（如 300k 默认 / 1M 上限）。
        // 过去国际版直接拿 defaultLength 当 window ⇒ 清单永远只显示下限，用户看到的 300k 其实不是上限。
        let context_window = wrapped
            .get("maxInputTokens")
            .and_then(Value::as_u64)
            .or_else(|| wrapped.get("maxAllowedSize").and_then(Value::as_u64))
            .filter(|value| *value > 0)
            .unwrap_or(input);
        let default_window = wrapped
            .get("contextWindow")
            .and_then(Value::as_object)
            .and_then(|c| c.get("defaultLength"))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0 && *value < context_window);
        let name = wrapped
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(&id)
            .to_string();
        let supports_images = wrapped.get("supportsImages").and_then(Value::as_bool) == Some(true)
            && wrapped.get("disabledMultimodal").and_then(Value::as_bool) != Some(true);
        let credits = wrapped
            .get("credits")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let badges = parse_badges(wrapped.get("tags"));
        let reasoning = parse_reasoning(wrapped.get("reasoning"));
        // ★ 判零走 `parse_credits_rate`（唯一解析点）：旧实现只认 `x0.00`，遇到上游真给的
        //   `x0.00 credits` 会算成「收费」（issue #65 顺带修）。
        let free = credits
            .as_deref()
            .and_then(parse_credits_rate)
            .map(|rate| rate == 0.0)
            .unwrap_or(false);
        by_id.insert(
            id.clone(),
            CatalogModel {
                id,
                name,
                context_window,
                context_window_default: default_window,
                max_tokens: output,
                supports_images,
                credits,
                badges,
                free,
                reasoning,
            },
        );
    }

    let models: Vec<CatalogModel> = cli_ids
        .into_iter()
        .filter_map(|id| by_id.get(&id).cloned())
        .collect();
    if models.is_empty() {
        return Err(catalog_error(
            "workbuddy model catalog resolved to an empty list",
        ));
    }
    Ok(models)
}

fn catalog_error(message: &str) -> UpstreamFailure {
    UpstreamFailure {
        status: 0,
        kind: UpstreamErrorKind::Server,
        message: message.to_string(),
    }
}

/// 从 `tags` 提取 `badge:<label>[:color]` 的 label。
fn parse_badges(tags: Option<&Value>) -> Vec<String> {
    let mut badges = Vec::new();
    let Some(tags) = tags.and_then(Value::as_array) else {
        return badges;
    };
    for tag in tags {
        let Some(tag) = tag.as_str() else {
            continue;
        };
        let lowered = tag.to_lowercase();
        if !lowered.starts_with("badge:") {
            continue;
        }
        let rest = &tag["badge:".len()..];
        let label = rest.split(':').next().unwrap_or(rest);
        if !label.is_empty() {
            badges.push(label.to_string());
        }
    }
    badges
}

/// 判断积分字符串是否为 `x0.00` 形态（免费）。
/// 解析上游 `reasoning` 块（issue #65）。
///
/// 实测三种形态（2026-10-21 `wb_raw.json`）：
/// - `{"effort": "medium", "summary": "auto"}` —— 只给默认档，**没有** `supportedEfforts`
///   ⇒ 这里返回「档位为空、默认档有值」，由网关侧回落静态表补全可选档位。
/// - `{"defaultEffort": "high", "supportedEfforts": ["low","high","max"], "canDisableThinking": true}`
/// - 整个键缺失 ⇒ `None`（与「声明了但没有档位」不同）。
///
/// 空数组 / 全脏值 ⇒ `None`：上游给了个空表等于没给，别把它当「不支持思考」的结论。
fn parse_reasoning(raw: Option<&Value>) -> Option<CatalogReasoning> {
    let object = raw?.as_object()?;
    let efforts: Vec<String> = object
        .get("supportedEfforts")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|effort| !effort.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let default_effort = object
        .get("defaultEffort")
        // 实测另一支只给 `{"effort": "medium", "summary": "auto"}`（没有 supportedEfforts）
        // —— 那是「默认用哪一档」，语义等价于 `defaultEffort`，别丢。
        .or_else(|| object.get("effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
        .map(str::to_string);
    let can_disable_thinking = object.get("canDisableThinking").and_then(Value::as_bool);
    if efforts.is_empty() && default_effort.is_none() && can_disable_thinking.is_none() {
        return None;
    }
    Some(CatalogReasoning {
        efforts,
        default_effort,
        can_disable_thinking,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_covers_six_kinds() {
        assert_eq!(
            classify_upstream_error(402, ""),
            UpstreamErrorKind::HardCredit
        );
        assert_eq!(
            classify_upstream_error(200, "积分不足，请充值"),
            UpstreamErrorKind::HardCredit
        );
        assert_eq!(
            classify_upstream_error(200, "Offline user session not found"),
            UpstreamErrorKind::SessionDead
        );
        assert_eq!(
            classify_upstream_error(200, "error code 12153"),
            UpstreamErrorKind::SessionDead
        );
        assert_eq!(
            classify_upstream_error(429, ""),
            UpstreamErrorKind::SoftRate
        );
        assert_eq!(
            classify_upstream_error(404, "model not found"),
            UpstreamErrorKind::NotFound
        );
        assert_eq!(
            classify_upstream_error(503, "upstream boom"),
            UpstreamErrorKind::Server
        );
        assert_eq!(
            classify_upstream_error(400, "bad request"),
            UpstreamErrorKind::Client
        );
    }

    #[test]
    fn classify_priority_status_402_before_markers() {
        // 402 优先于 body 标记。
        assert_eq!(
            classify_upstream_error(402, "Offline user session not found"),
            UpstreamErrorKind::HardCredit
        );
    }

    #[test]
    fn prepare_chat_body_forces_stream_developer_and_tool_choice() {
        let source = r#"{
            "model": "glm-5.3",
            "stream": false,
            "messages": [
                {"role": "developer", "content": "sys"},
                {"role": "user", "content": "hi"}
            ],
            "tool_choice": {"type": "function", "function": {"name": "do_it"}}
        }"#;
        let prepared: Value = serde_json::from_str(&prepare_chat_body(source)).unwrap();
        assert_eq!(prepared["stream"], json!(true));
        assert_eq!(prepared["messages"][0]["role"], "system");
        assert_eq!(prepared["messages"][1]["role"], "user");
        assert_eq!(prepared["tool_choice"], json!("do_it"));
    }

    #[test]
    fn prepare_chat_body_none_drops_tools_and_functions() {
        let source = r#"{"tool_choice":"none","tools":[{"x":1}],"functions":[{"y":2}]}"#;
        let prepared: Value = serde_json::from_str(&prepare_chat_body(source)).unwrap();
        assert!(prepared.get("tool_choice").is_none());
        assert!(prepared.get("tools").is_none());
        assert!(prepared.get("functions").is_none());
        assert_eq!(prepared["stream"], json!(true));
    }

    #[test]
    fn prepare_chat_body_flattens_auto_and_required() {
        let auto: Value =
            serde_json::from_str(&prepare_chat_body(r#"{"tool_choice":{"type":"auto"}}"#)).unwrap();
        assert_eq!(auto["tool_choice"], json!("auto"));
        let required: Value =
            serde_json::from_str(&prepare_chat_body(r#"{"tool_choice":{"type":"required"}}"#))
                .unwrap();
        assert_eq!(required["tool_choice"], json!("required"));
    }

    #[test]
    fn prepare_chat_body_passthrough_non_json() {
        assert_eq!(prepare_chat_body("not json"), "not json");
        assert_eq!(prepare_chat_body("[1,2,3]"), "[1,2,3]");
    }

    #[test]
    fn international_prepends_system_without_reordering() {
        let source = r#"{"messages":[{"role":"user","content":"first"},{"role":"assistant","content":"second"}]}"#;
        let prepared: Value =
            serde_json::from_str(&prepare_international_chat_body(source)).unwrap();
        let messages = prepared["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], INTERNATIONAL_SYSTEM_PROMPT);
        assert_eq!(messages[1]["content"], "first");
        assert_eq!(messages[2]["content"], "second");
        assert_eq!(prepared["stream"], json!(true));
    }

    #[test]
    fn international_keeps_existing_leading_system() {
        let source = r#"{"messages":[{"role":"system","content":"keep"}]}"#;
        let prepared: Value =
            serde_json::from_str(&prepare_international_chat_body(source)).unwrap();
        let messages = prepared["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"], "keep");
    }

    #[test]
    fn parse_model_catalog_uses_cli_whitelist_and_skips_invalid() {
        let data = json!({
            "agents": [{"name": "cli", "models": ["a", "b", "disabled-model", "no-window"]}],
            "models": [
                {"id": "a", "name": "A", "maxInputTokens": 100, "maxOutputTokens": 10, "supportsImages": true},
                {"id": "b", "name": "B", "maxInputTokens": 200, "maxOutputTokens": 20, "supportsImages": false},
                {"id": "disabled-model", "maxInputTokens": 1, "maxOutputTokens": 1, "disabled": true},
                {"id": "no-window", "maxInputTokens": 0, "maxOutputTokens": 0},
                {"id": "extra", "maxInputTokens": 5, "maxOutputTokens": 5}
            ]
        });
        let models = parse_model_catalog(&data, false).unwrap();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(models[0].context_window, 100);
        assert!(models[0].supports_images);
        assert!(!models[1].supports_images);
    }

    /// ★ 国际版也要区分两档：`maxInputTokens` 是**上限**、`contextWindow.defaultLength` 是**默认**。
    #[test]
    fn parse_model_catalog_reads_billing_and_both_windows() {
        let data = json!({
            "agents": [{"name": "cli", "models": ["m"]}],
            "models": [{
                "id": "m",
                "name": "M",
                "maxInputTokens": 1000,
                "maxOutputTokens": 100,
                "credits": "x0.00",
                "tags": ["badge:限时免费:#FF0000", "other"],
                "contextWindow": {"defaultLength": 500, "supportedLengths": [500, 1000]}
            }]
        });
        let models = parse_model_catalog(&data, true).unwrap();
        assert_eq!(models[0].context_window, 1000, "上限档取 maxInputTokens");
        assert_eq!(
            models[0].context_window_default,
            Some(500),
            "默认档取 defaultLength"
        );
        assert!(models[0].free);
        assert_eq!(models[0].badges, vec!["限时免费".to_string()]);
    }

    /// ★ issue #65：`x0.00 credits` 也是免费（旧判据只认 `x0.00`，`free` 恒 false），
    /// 且倍率要能解析成数字（同一份目录里三种写法并存）。
    ///
    /// 变异：把 `free` 改回 `is_zero_credits`（只比整数段）⇒ 第一条断言红。
    #[test]
    fn parse_model_catalog_treats_zero_credits_with_suffix_as_free() {
        let data = json!({
            "agents": [{"name": "cli", "models": ["free-a", "free-b", "paid"]}],
            "models": [
                {"id": "free-a", "maxInputTokens": 10, "maxOutputTokens": 10, "credits": "x0.00 credits"},
                {"id": "free-b", "maxInputTokens": 10, "maxOutputTokens": 10, "credits": "0.00"},
                {"id": "paid", "maxInputTokens": 10, "maxOutputTokens": 10, "credits": "x0.34 credits"}
            ]
        });
        let models = parse_model_catalog(&data, false).unwrap();
        assert!(models[0].free, "`x0.00 credits` 必须判免费");
        assert_eq!(models[0].credits_rate(), Some(0.0));
        assert!(models[1].free, "无前缀的 `0.00` 也要认");
        assert!(!models[2].free);
        assert_eq!(models[2].credits_rate(), Some(0.34), "后缀不影响取数");
    }

    /// ★ issue #65：上游 `reasoning` 块解析 —— 声明了档位 / 只给默认档 / 完全缺失。
    #[test]
    fn parse_model_catalog_reads_reasoning_efforts() {
        let data = json!({
            "agents": [{"name": "cli", "models": ["declared", "default-only", "none"]}],
            "models": [
                {"id": "declared", "maxInputTokens": 10, "maxOutputTokens": 10,
                 "reasoning": {"defaultEffort": "high", "canDisableThinking": true,
                               "supportedEfforts": ["low", "high", "max"], "summary": "auto"}},
                {"id": "default-only", "maxInputTokens": 10, "maxOutputTokens": 10,
                 "reasoning": {"effort": "medium", "summary": "auto"}},
                {"id": "none", "maxInputTokens": 10, "maxOutputTokens": 10}
            ]
        });
        let models = parse_model_catalog(&data, false).unwrap();
        let declared = models[0].reasoning.as_ref().expect("声明了 reasoning");
        assert_eq!(declared.efforts, vec!["low", "high", "max"]);
        assert_eq!(declared.default_effort.as_deref(), Some("high"));
        assert_eq!(declared.can_disable_thinking, Some(true));

        let default_only = models[1].reasoning.as_ref().expect("有默认档也算声明");
        assert!(default_only.efforts.is_empty(), "没给 supportedEfforts");
        assert_eq!(
            default_only.default_effort.as_deref(),
            Some("medium"),
            "`effort` 等价于 defaultEffort"
        );

        assert!(
            models[2].reasoning.is_none(),
            "整块缺失 ⇒ None（不是空档位）"
        );
    }

    /// 默认档不小于上限（脏数据）⇒ 不认默认档，避免出现「默认 > 上限」。
    #[test]
    fn parse_model_catalog_ignores_inverted_default_window() {
        let data = json!({
            "agents": [{"name": "cli", "models": ["m"]}],
            "models": [{
                "id": "m", "maxInputTokens": 1000, "maxOutputTokens": 100,
                "contextWindow": {"defaultLength": 2000}
            }]
        });
        let models = parse_model_catalog(&data, true).unwrap();
        assert_eq!(models[0].context_window, 1000);
        assert_eq!(models[0].context_window_default, None);
    }

    #[test]
    fn parse_model_catalog_errors_when_no_cli_agent() {
        let data = json!({"models": [{"id": "a", "maxInputTokens": 1, "maxOutputTokens": 1}]});
        assert!(parse_model_catalog(&data, false).is_err());
    }

    #[test]
    fn chat_body_for_dispatches_cn_as_exact_passthrough() {
        let source = "not-json with preserved whitespace";
        assert_eq!(chat_body_for(Region::Cn, source), source);
    }

    #[test]
    fn chat_body_for_dispatches_global_to_international_normalization() {
        let source = r#"{"messages":[{"role":"user","content":"hi"}],"tools":[{"type":"function"}],"tool_choice":{"type":"auto"}}"#;
        let global = chat_body_for(Region::Global, source);
        assert_eq!(global, prepare_international_chat_body(source));
        assert_ne!(global, source, "Global 不得退化为 CN 原样透传");
        let prepared: Value = serde_json::from_str(&global).unwrap();
        assert_eq!(prepared["messages"][0]["role"], "system");
    }

    #[test]
    fn chat_body_for_global_normalization_is_idempotent() {
        let source = r#"{"messages":[{"role":"user","content":"hi"}]}"#;
        let once = chat_body_for(Region::Global, source);
        assert_eq!(chat_body_for(Region::Global, &once), once);
    }
}
