//! 独立第三方消费账号网关（豆包、Loomy、小浣熊）。
//!
//! 每个 provider 使用自己的 listener、API key 与凭据池，不接入 WorkBuddy/Trae/Qoder
//! 账号池。服务只监听本机回环地址；上游域名固定在代码中，避免凭据转发到用户指定地址。

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use buddy_switch_core::modules::{config, external_credits};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};

const LOOMY_CHAT: &str = "https://loomyad.xunfei.cn/api/v1/chat/completions";
const LOOMY_VERSION: &str = "0.9.38";
const RACCOON_CHAT: &str = "https://xiaohuanxiong.com/api/web/llm/v2/chat/completions";
const TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Doubao,
    Loomy,
    Raccoon,
}

impl Provider {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "doubao" => Ok(Self::Doubao),
            "loomy" => Ok(Self::Loomy),
            "raccoon" => Ok(Self::Raccoon),
            _ => Err("不支持的第三方网关".into()),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Doubao => "doubao",
            Self::Loomy => "loomy",
            Self::Raccoon => "raccoon",
        }
    }
    pub fn port(self) -> u16 {
        match self {
            Self::Doubao => 57896,
            Self::Loomy => 57897,
            Self::Raccoon => 57898,
        }
    }
}

#[derive(Clone)]
struct StateData {
    provider: Provider,
    api_key: Arc<String>,
    client: reqwest::Client,
    cursor: Arc<AtomicUsize>,
}

fn key_path(provider: Provider) -> PathBuf {
    config::store_dir().join(format!("external_gateway_{}.key", provider.key()))
}

/// Return an existing local key or create one with private directory permissions.
pub fn api_key(provider: Provider) -> Result<String, String> {
    let path = key_path(provider);
    if let Ok(key) = std::fs::read_to_string(&path) {
        let key = key.trim();
        if key.starts_with("sk-ext-") {
            return Ok(key.to_string());
        }
    }
    config::ensure_private_dir(&config::store_dir())
        .map_err(|e| format!("创建网关配置目录失败：{e}"))?;
    let key = format!(
        "sk-ext-{}-{}",
        provider.key(),
        uuid::Uuid::new_v4().simple()
    );
    config::atomic_write(&path, &key).map_err(|e| format!("保存网关 API Key 失败：{e}"))?;
    Ok(key)
}

pub async fn models_snapshot(provider: Provider) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("创建本地网关客户端失败：{error}"))?;
    let url = format!("http://127.0.0.1:{}/v1/models", provider.port());
    let response = client
        .get(url)
        .bearer_auth(api_key(provider)?)
        .send()
        .await
        .map_err(|_| "该平台网关当前未运行".to_string())?;
    if !response.status().is_success() {
        return Err(format!("读取模型清单失败（HTTP {}）", response.status()));
    }
    response
        .json()
        .await
        .map_err(|_| "上游模型清单格式无效".to_string())
}

pub fn router(provider: Provider, key: String) -> Result<Router, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("创建代理客户端失败：{e}"))?;
    let state = StateData {
        provider,
        api_key: Arc::new(key),
        client,
        cursor: Arc::new(AtomicUsize::new(0)),
    };
    Ok(Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/messages", post(chat))
        .route("/v1/responses", post(chat))
        .route("/v1/images/generations", post(generate_images))
        .route("/v1/audio/generations", post(generate_music))
        .route("/v1/video/generations", post(generate_videos))
        .route("/v1/usage", get(request_usage))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(state))
}

async fn models(State(state): State<StateData>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return auth_error();
    }
    let accounts = match external_credits::gateway_accounts(state.provider.key()) {
        Ok(accounts) => accounts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":error,"type":"server_error"}})),
            )
                .into_response()
        }
    };
    let Some(account) = accounts.first() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"message":"当前平台没有已添加的账号凭据","type":"server_error"}})),
        )
            .into_response();
    };
    if state.provider == Provider::Loomy {
        let response = state
            .client
            .get("https://loomyad.xunfei.cn/api/v1/models")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", account.credential),
            )
            .header("token", &account.credential)
            .header("traceparent", new_traceparent())
            .header("loomy-version", LOOMY_VERSION)
            .send()
            .await;
        return match response {
            Ok(response) if response.status().is_success() => {
                let status = StatusCode::from_u16(response.status().as_u16())
                    .unwrap_or(StatusCode::BAD_GATEWAY);
                let content_type = response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .cloned()
                    .unwrap_or(HeaderValue::from_static("application/json"));
                let stream = response
                    .bytes_stream()
                    .map(|chunk| chunk.map_err(std::io::Error::other));
                let mut result = Response::new(Body::from_stream(stream));
                *result.status_mut() = status;
                result
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, content_type);
                result
            }
            _ => upstream_error(),
        };
    }
    if state.provider == Provider::Raccoon {
        let token = account
            .credential
            .strip_prefix("Bearer ")
            .unwrap_or(&account.credential);
        let response = state
            .client
            .get("https://xiaohuanxiong.com/api/web/llm/v2/model_catalog")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::ORIGIN, "https://xiaohuanxiong.com")
            .header(header::REFERER, "https://xiaohuanxiong.com/")
            .header(header::USER_AGENT, "office-raccoon/1.0.0")
            .header("x-client-platform", "desktop-windows")
            .header("x-client-version", "v1.0.0")
            .header("x-raccoon-language", "zh")
            .send()
            .await;
        if let Ok(response) = response {
            if let Ok(body) = response.json::<Value>().await {
                let mut ids = Vec::new();
                collect_raccoon_models(&body, &mut ids);
                ids.sort();
                ids.dedup();
                if !ids.is_empty() {
                    return Json(json!({"object":"list","data":ids.iter().map(|id| json!({"id":id,"object":"model","owned_by":"raccoon"})).collect::<Vec<_>>()})).into_response();
                }
            }
        }
        return upstream_error();
    }
    // Doubao2API exposes its model list from the local desktop Preferences cache;
    // the credit-only account credential does not include that client-side cache.
    let ids = [
        "auto",
        "doubao-2.1-lite",
        "doubao-2.1-pro",
        "deepseek-v4-flash",
        "kimi-k3.1-pro",
        "kimi-k3.1-pro-fast",
        "kimi-k3",
        "glm-5.3",
        "glm-5.3-flash",
        "deepseek-v4-pro",
        "doubao",
        "doubao-think",
        "doubao-expert",
        "doubao-image",
        "doubao-music",
        "doubao-video",
    ];
    Json(json!({"object":"list","data":ids.iter().map(|id| json!({"id":id,"object":"model","owned_by":"doubao"})).collect::<Vec<_>>()})).into_response()
}

async fn generate_images(
    State(state): State<StateData>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return auth_error();
    }
    if state.provider != Provider::Doubao {
        return unsupported("当前平台未提供图像生成接口");
    }
    let prompt = payload
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if prompt.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"prompt is required","type":"invalid_request_error"}})),
        )
            .into_response();
    }
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("doubao-image");
    if model != "doubao-image" {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"图像生成仅支持 doubao-image","type":"invalid_request_error"}}))).into_response();
    }
    if payload
        .get("n")
        .and_then(Value::as_u64)
        .is_some_and(|n| n != 1)
    {
        return unsupported("豆包图像生成当前每次仅支持生成一张图片");
    }
    let ratio = payload
        .get("ratio")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("size")
                .and_then(Value::as_str)
                .map(|size| match size {
                    "1792x1024" => "16:9",
                    "1024x1792" => "9:16",
                    "1024x768" => "4:3",
                    "768x1024" => "3:4",
                    _ => "1:1",
                })
        })
        .unwrap_or("1:1");
    if !matches!(ratio, "1:1" | "16:9" | "9:16" | "4:3" | "3:4") {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"ratio must be 1:1, 16:9, 9:16, 4:3, or 3:4","type":"invalid_request_error"}}))).into_response();
    }
    let accounts = match external_credits::gateway_accounts(state.provider.key()) {
        Ok(accounts) => accounts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":error,"type":"server_error"}})),
            )
                .into_response()
        }
    };
    let Some(account) = accounts.first() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"message":"当前平台没有已添加的账号凭据","type":"server_error"}})),
        )
            .into_response();
    };
    match doubao_generate_image(&state, &account.credential, prompt, ratio).await {
        Ok(images) if !images.is_empty() => {
            save_usage_record(
                Provider::Doubao,
                Protocol::OpenAi,
                "doubao-image",
                StatusCode::OK,
                None,
                None,
            );
            Json(json!({
                "created": chrono::Utc::now().timestamp(),
                "data": images.iter().map(|image| json!({"url":image["url"]})).collect::<Vec<_>>()
            }))
            .into_response()
        }
        Ok(_) | Err(_) => {
            save_usage_record(
                Provider::Doubao,
                Protocol::OpenAi,
                "doubao-image",
                StatusCode::BAD_GATEWAY,
                None,
                None,
            );
            upstream_error()
        }
    }
}

async fn doubao_generate_image(
    state: &StateData,
    cookie: &str,
    prompt: &str,
    ratio: &str,
) -> Result<Vec<Value>, String> {
    let csrf = refresh_doubao_csrf(&state.client, cookie).await;
    let message = json!({
        "content": serde_json::to_string(&json!({"text":prompt,"ratio":ratio})).map_err(|_| "invalid prompt")?,
        "content_type":2009, "attachments":[], "references":[],
        "skill":{"skill_type":3,"skill_type_no_default":3,"skill_id":"3","skill_id_no_default":"3"}
    });
    let body = json!({
        "messages":[message],
        "completion_option":{
            "is_regen":false,"with_suggest":true,"need_create_conversation":true,"launch_stage":1,
            "is_replace":false,"is_delete":false,"is_ai_playground":false,"memory_type":2,
            "message_from":0,"use_deep_think":false,"use_auto_cot":false,"resend_for_regen":false,
            "enable_commerce_credit":false,"action_bar_skill_id":3
        },
        "evaluate_option":{"web_ab_params":""},
        "local_conversation_id":uuid::Uuid::new_v4().to_string(),
        "local_message_id":uuid::Uuid::new_v4().to_string()
    });
    let mut request = state
        .client
        .post(doubao_url("/samantha/chat/completion")?)
        .header(header::COOKIE, cookie)
        .header(header::ACCEPT, "text/event-stream")
        .header(header::ORIGIN, "https://www.doubao.com")
        .header(header::REFERER, "https://www.doubao.com/chat/")
        .header("agw-js-conv", "str")
        .json(&body);
    if let Some(csrf) = csrf {
        request = request.header("x-tt-passport-csrf-token", csrf);
    }
    let response = request.send().await.map_err(|_| "图像生成请求失败")?;
    if !response.status().is_success() {
        return Err("豆包图像生成服务拒绝请求".into());
    }
    let raw = response.text().await.map_err(|_| "图像生成响应读取失败")?;
    let mut images = Vec::new();
    for event in raw.replace("\r\n", "\n").split("\n\n") {
        let Some(data) = event.lines().find_map(|line| line.strip_prefix("data:")) else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        if envelope.get("event_type").and_then(Value::as_i64) == Some(2005) {
            return Err("豆包图像生成失败".into());
        }
        if envelope.get("event_type").and_then(Value::as_i64) != Some(2001) {
            continue;
        }
        let event_data = envelope.get("event_data").cloned().unwrap_or(Value::Null);
        let event_data = if let Some(text) = event_data.as_str() {
            serde_json::from_str::<Value>(text).unwrap_or(Value::Null)
        } else {
            event_data
        };
        let message = event_data.get("message").cloned().unwrap_or(Value::Null);
        if message.get("content_type").and_then(Value::as_i64) != Some(2010) {
            continue;
        }
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        let content = if let Some(text) = content.as_str() {
            serde_json::from_str::<Value>(text).unwrap_or(Value::Null)
        } else {
            content
        };
        let Some(rows) = content.get("data").and_then(Value::as_array) else {
            continue;
        };
        for row in rows {
            let url = row
                .pointer("/image_ori/url")
                .and_then(Value::as_str)
                .or_else(|| row.pointer("/image_raw/url").and_then(Value::as_str));
            if let Some(url) = url.filter(|url| url.starts_with("https://")) {
                images.push(json!({"url":url}));
            }
        }
    }
    Ok(images)
}

async fn generate_music(
    State(state): State<StateData>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return auth_error();
    }
    if state.provider != Provider::Doubao {
        return unsupported("当前平台未提供音频生成接口");
    }
    let Some(prompt) = payload
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"prompt is required","type":"invalid_request_error"}})),
        )
            .into_response();
    };
    let accounts = match external_credits::gateway_accounts(state.provider.key()) {
        Ok(accounts) => accounts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":error,"type":"server_error"}})),
            )
                .into_response()
        }
    };
    let Some(account) = accounts.first() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"message":"当前平台没有已添加的账号凭据","type":"server_error"}})),
        )
            .into_response();
    };
    match doubao_generate_music(&state, &account.credential, prompt, &payload).await {
        Ok(tracks) if !tracks.is_empty() => {
            save_usage_record(
                Provider::Doubao,
                Protocol::OpenAi,
                "doubao-music",
                StatusCode::OK,
                None,
                None,
            );
            Json(json!({"created":chrono::Utc::now().timestamp(),"data":tracks})).into_response()
        }
        _ => {
            save_usage_record(
                Provider::Doubao,
                Protocol::OpenAi,
                "doubao-music",
                StatusCode::BAD_GATEWAY,
                None,
                None,
            );
            upstream_error()
        }
    }
}

async fn generate_videos(
    State(state): State<StateData>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return auth_error();
    }
    if state.provider != Provider::Doubao {
        return unsupported("当前平台未提供视频生成接口");
    }
    let Some(prompt) = payload
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"prompt is required","type":"invalid_request_error"}})),
        )
            .into_response();
    };
    let ratio = payload
        .get("ratio")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("size")
                .and_then(Value::as_str)
                .map(|size| match size {
                    "1792x1024" => "16:9",
                    "1024x1792" => "9:16",
                    "1024x768" => "4:3",
                    "768x1024" => "3:4",
                    _ => "1:1",
                })
        })
        .unwrap_or("16:9");
    if !matches!(ratio, "1:1" | "16:9" | "9:16" | "4:3" | "3:4") {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"ratio must be 1:1, 16:9, 9:16, 4:3, or 3:4","type":"invalid_request_error"}}))).into_response();
    }
    let accounts = match external_credits::gateway_accounts(state.provider.key()) {
        Ok(accounts) => accounts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":error,"type":"server_error"}})),
            )
                .into_response()
        }
    };
    let Some(account) = accounts.first() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"message":"当前平台没有已添加的账号凭据","type":"server_error"}})),
        )
            .into_response();
    };
    match doubao_generate_video(&state, &account.credential, prompt, ratio).await {
        Ok(videos) if !videos.is_empty() => {
            save_usage_record(
                Provider::Doubao,
                Protocol::OpenAi,
                "doubao-video",
                StatusCode::OK,
                None,
                None,
            );
            Json(json!({"created":chrono::Utc::now().timestamp(),"data":videos})).into_response()
        }
        _ => {
            save_usage_record(
                Provider::Doubao,
                Protocol::OpenAi,
                "doubao-video",
                StatusCode::BAD_GATEWAY,
                None,
                None,
            );
            upstream_error()
        }
    }
}

fn doubao_sse_events(raw: &str) -> std::vec::IntoIter<Value> {
    raw.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            event
                .lines()
                .find_map(|line| line.strip_prefix("data:"))
                .and_then(|data| serde_json::from_str(data.trim()).ok())
        })
        .collect::<Vec<_>>()
        .into_iter()
}

fn doubao_event_data(event: &Value) -> Value {
    let value = event.get("event_data").cloned().unwrap_or(Value::Null);
    value
        .as_str()
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(value)
}

async fn submit_doubao_media(
    state: &StateData,
    cookie: &str,
    content_type: i64,
    skill_id: i64,
    content: Value,
) -> Result<String, String> {
    let csrf = refresh_doubao_csrf(&state.client, cookie).await;
    let message = json!({"content":serde_json::to_string(&content).map_err(|_| "生成参数无效")?,"content_type":content_type,"attachments":[],"references":[],"skill":{"skill_type":skill_id,"skill_type_no_default":skill_id,"skill_id":skill_id.to_string(),"skill_id_no_default":skill_id.to_string()}});
    let body = json!({"messages":[message],"completion_option":{"is_regen":false,"with_suggest":true,"need_create_conversation":true,"launch_stage":1,"is_replace":false,"is_delete":false,"is_ai_playground":false,"memory_type":2,"message_from":0,"use_deep_think":false,"use_auto_cot":false,"resend_for_regen":false,"enable_commerce_credit":false,"action_bar_skill_id":skill_id},"evaluate_option":{"web_ab_params":""},"local_conversation_id":uuid::Uuid::new_v4().to_string(),"local_message_id":uuid::Uuid::new_v4().to_string()});
    let mut request = state
        .client
        .post(doubao_url("/samantha/chat/completion")?)
        .header(header::COOKIE, cookie)
        .header(header::ACCEPT, "text/event-stream")
        .header(header::ORIGIN, "https://www.doubao.com")
        .header(header::REFERER, "https://www.doubao.com/chat/")
        .header("agw-js-conv", "str")
        .json(&body);
    if let Some(csrf) = csrf {
        request = request.header("x-tt-passport-csrf-token", csrf);
    }
    let response = request.send().await.map_err(|_| "豆包媒体生成请求失败")?;
    if !response.status().is_success() {
        return Err("豆包媒体生成服务拒绝请求".into());
    }
    response
        .text()
        .await
        .map_err(|_| "豆包媒体生成响应读取失败".into())
}

async fn doubao_generate_music(
    state: &StateData,
    cookie: &str,
    prompt: &str,
    payload: &Value,
) -> Result<Vec<Value>, String> {
    let mut content = json!({"text":prompt});
    for name in [
        "lyric",
        "genre",
        "mood",
        "gender",
        "theme",
        "generation_type",
    ] {
        if let Some(value) = payload.get(name).and_then(Value::as_str) {
            content[name] = json!(value);
        }
    }
    let raw = submit_doubao_media(state, cookie, 2005, 9, content).await?;
    let mut final_content = None;
    for event in doubao_sse_events(&raw) {
        if event.get("event_type").and_then(Value::as_i64) == Some(2005) {
            return Err("豆包音乐生成失败".into());
        }
        if event.get("event_type").and_then(Value::as_i64) != Some(2001) {
            continue;
        }
        let data = doubao_event_data(&event);
        let message = data.get("message").cloned().unwrap_or(Value::Null);
        if !matches!(
            message.get("content_type").and_then(Value::as_i64),
            Some(2006 | 2004)
        ) {
            continue;
        }
        let value = message.get("content").cloned().unwrap_or(Value::Null);
        let parsed = value
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(value);
        if data
            .get("is_finish")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || final_content.is_none()
        {
            final_content = Some(parsed);
        }
    }
    let tasks = final_content
        .and_then(|v| v.get("tasks").cloned())
        .unwrap_or(Value::Null);
    let rows: Vec<Value> = match tasks {
        Value::Array(rows) => rows,
        Value::Object(map) => map.into_values().collect(),
        _ => Vec::new(),
    };
    Ok(rows.into_iter().filter_map(|task| {
        let vm = task.get("video_model").cloned().unwrap_or(Value::Null);
        let vm = vm.as_str().and_then(|text| serde_json::from_str::<Value>(text).ok()).unwrap_or(vm);
        let duration = vm.get("video_duration").cloned().unwrap_or(json!(0));
        let audio_url = vm.get("video_list").and_then(Value::as_object).and_then(|list| list.values().find_map(|entry| entry.get("main_url").and_then(Value::as_str).and_then(|encoded| BASE64.decode(encoded).ok()).and_then(|bytes| String::from_utf8(bytes).ok()))).unwrap_or_default();
        (!audio_url.is_empty()).then(|| json!({"audio_url":audio_url,"title":task.get("title"),"duration":duration,"lyrics":task.get("lyrics"),"cover_url":task.pointer("/cover/image_ori/url")}))
    }).collect())
}

async fn doubao_generate_video(
    state: &StateData,
    cookie: &str,
    prompt: &str,
    ratio: &str,
) -> Result<Vec<Value>, String> {
    let raw = submit_doubao_media(
        state,
        cookie,
        2020,
        17,
        json!({"text":prompt,"ratio":ratio}),
    )
    .await?;
    let task_id = doubao_sse_events(&raw).find_map(|event| {
        if event.get("event_type").and_then(Value::as_i64) != Some(2001) {
            return None;
        }
        doubao_event_data(&event)
            .pointer("/fin_reason/async_task/id")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let raw = if let Some(task_id) = task_id {
        let url = doubao_url("/samantha/chat/async/stream")?;
        let csrf = refresh_doubao_csrf(&state.client, cookie).await;
        let mut request = state
            .client
            .post(url)
            .header(header::COOKIE, cookie)
            .header(header::ACCEPT, "text/event-stream")
            .header(header::ORIGIN, "https://www.doubao.com")
            .header(header::REFERER, "https://www.doubao.com/chat/")
            .json(&json!({"task_id":task_id,"event_id":0}));
        if let Some(csrf) = csrf {
            request = request.header("x-tt-passport-csrf-token", csrf);
        }
        let response = request
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .map_err(|_| "豆包视频任务查询失败")?;
        if !response.status().is_success() {
            return Err("豆包视频任务查询被拒绝".into());
        }
        response
            .text()
            .await
            .map_err(|_| "豆包视频结果读取失败".to_string())?
    } else {
        raw
    };
    let mut videos = Vec::new();
    for event in doubao_sse_events(&raw) {
        if event.get("event_type").and_then(Value::as_i64) == Some(2005) {
            return Err("豆包视频生成失败".into());
        }
        if event.get("event_type").and_then(Value::as_i64) != Some(2001) {
            continue;
        }
        let data = doubao_event_data(&event);
        let message = data.get("message").cloned().unwrap_or(Value::Null);
        if message.get("content_type").and_then(Value::as_i64) != Some(2021) {
            continue;
        }
        let value = message.get("content").cloned().unwrap_or(Value::Null);
        let content = value
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or(value);
        let rows = content
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| vec![content]);
        for item in rows {
            let vm = item.get("video_model").cloned().unwrap_or(Value::Null);
            let vm = vm
                .as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .unwrap_or(vm);
            let video_url = item
                .get("video_url")
                .or_else(|| item.get("url"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    vm.get("video_list")
                        .and_then(Value::as_object)
                        .and_then(|list| {
                            list.values().find_map(|v| {
                                v.get("main_url")
                                    .and_then(Value::as_str)
                                    .and_then(|encoded| BASE64.decode(encoded).ok())
                                    .and_then(|bytes| String::from_utf8(bytes).ok())
                            })
                        })
                });
            if let Some(url) = video_url.filter(|url| url.starts_with("https://")) {
                videos.push(json!({"url":url,"cover_url":item.get("cover_url").or_else(|| item.pointer("/cover/url")),"width":item.get("width"),"height":item.get("height"),"duration":item.get("duration")}));
            }
        }
    }
    Ok(videos)
}

fn collect_raccoon_models(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let id = map
                .get("name")
                .or_else(|| map.get("model_name"))
                .or_else(|| map.get("id"))
                .and_then(Value::as_str);
            if let Some(id) = id.filter(|id| !id.trim().is_empty()) {
                ids.push(id.trim().to_string());
            }
            for child in map.values() {
                collect_raccoon_models(child, ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_raccoon_models(item, ids);
            }
        }
        _ => {}
    }
}

async fn chat(State(state): State<StateData>, headers: HeaderMap, request: Request) -> Response {
    if !authorized(&state, &headers) {
        return auth_error();
    }
    let endpoint = request.uri().path().to_string();
    let bytes =
        match axum::body::to_bytes(request.into_body(), 16 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"message":"无法读取请求体","type":"invalid_request_error"}})),
            )
                .into_response(),
        };
    let payload: Value =
        match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => return (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({"error":{"message":"请求体必须是 JSON","type":"invalid_request_error"}}),
                ),
            )
                .into_response(),
        };
    let protocol = match endpoint.as_str() {
        "/v1/messages" => Protocol::Anthropic,
        "/v1/responses" => Protocol::Responses,
        _ => Protocol::OpenAi,
    };
    let client_stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let upstream_payload = match protocol {
        Protocol::OpenAi => payload.clone(),
        Protocol::Anthropic => {
            crate::protocol::anthropic::to_upstream_request_with_images(&payload)
        }
        Protocol::Responses => crate::protocol::responses::to_upstream_request(&payload),
    };
    let mut upstream_payload = upstream_payload;
    upstream_payload["stream"] = json!(true);
    upstream_payload["stream_options"] = json!({"include_usage": true});
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if model.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"model is required","type":"invalid_request_error"}})),
        )
            .into_response();
    }
    let accounts = match external_credits::gateway_accounts(state.provider.key()) {
        Ok(accounts) => accounts,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":error,"type":"server_error"}})),
            )
                .into_response()
        }
    };
    if accounts.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"message":"当前平台没有已添加的账号凭据","type":"server_error"}})),
        )
            .into_response();
    }
    let index = state.cursor.fetch_add(1, Ordering::Relaxed) % accounts.len();
    let ordered_accounts = accounts
        .iter()
        .cycle()
        .skip(index)
        .take(accounts.len())
        .collect::<Vec<_>>();
    if state.provider == Provider::Doubao {
        upstream_payload.as_object_mut().map(|object| {
            object.remove("stream_options");
        });
        if protocol != Protocol::OpenAi {
            // Doubao's private web endpoint is text-only and does not expose the
            // stable tool-call semantics required by Anthropic/Responses.
            if upstream_payload.get("tools").is_some() {
                return unsupported(
                    "豆包上游当前不支持工具调用，Anthropic/Responses 请求无法安全转换",
                );
            }
        }
        let mut last = upstream_error();
        for account in ordered_accounts {
            let response = doubao_chat(&state, &account.credential, upstream_payload.clone()).await;
            if response.status().is_success() || !retryable_status(response.status()) {
                /* ⚠️ OpenAI 协议也要走 `adapt_doubao_response`：上游被强制 `stream=true`，
                    直接透传会让 `"stream": false` 的调用方收到 SSE 原文。
                    适配器对 OpenAI 的非流式分支会把增量聚合成一个 `chat.completion` 对象。 */
                return adapt_doubao_response(response, protocol, &model, client_stream, &payload)
                    .await;
            }
            last = response;
        }
        return tap_axum_response(last, state.provider, protocol, &model);
    }
    let url = match state.provider {
        Provider::Loomy => LOOMY_CHAT,
        Provider::Raccoon => RACCOON_CHAT,
        Provider::Doubao => unreachable!(),
    };
    let mut response_result = Err(());
    for account in ordered_accounts {
        let mut upstream = state
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "text/event-stream");
        match state.provider {
            Provider::Loomy => {
                upstream = upstream
                    .header("token", &account.credential)
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", account.credential),
                    )
                    .header("loomy-version", LOOMY_VERSION)
                    .header("x-loomy-request-purpose", "chat.message")
                    .header("traceparent", new_traceparent())
            }
            Provider::Raccoon => {
                let token = account
                    .credential
                    .strip_prefix("Bearer ")
                    .unwrap_or(&account.credential);
                upstream = upstream
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::ORIGIN, "https://xiaohuanxiong.com")
                    .header(header::REFERER, "https://xiaohuanxiong.com/")
                    .header(header::USER_AGENT, "office-raccoon/1.0.0")
                    .header("x-client-platform", "desktop-windows")
                    .header("x-client-version", "v1.0.0")
                    .header("x-raccoon-language", "zh");
            }
            Provider::Doubao => unreachable!(),
        }
        // The exact same converted payload is retried on another account only
        // before a successful upstream response starts streaming.
        match upstream.json(&upstream_payload).send().await {
            Ok(response) if retryable_status(response.status()) => {
                response_result = Ok(response);
            }
            Ok(response) => {
                return adapt_response(
                    response,
                    state.provider,
                    protocol,
                    &model,
                    client_stream,
                    &payload,
                )
                .await
            }
            Err(_) => response_result = Err(()),
        }
    }
    let response = match response_result {
        Ok(response) => response,
        Err(_) => {
            save_usage_record(
                state.provider,
                protocol,
                &model,
                StatusCode::BAD_GATEWAY,
                None,
                None,
            );
            return upstream_error();
        }
    };
    adapt_response(
        response,
        state.provider,
        protocol,
        &model,
        client_stream,
        &payload,
    )
    .await
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Protocol {
    OpenAi,
    Anthropic,
    Responses,
}

async fn adapt_doubao_response(
    response: Response,
    protocol: Protocol,
    model: &str,
    client_stream: bool,
    original: &Value,
) -> Response {
    let status = response.status();
    if !status.is_success() {
        return response;
    }
    if client_stream {
        // The Doubao adapter already emits OpenAI SSE. Pass its live byte stream
        // directly into the shared protocol decoders so Claude/Codex receive
        // deltas as soon as Doubao sends them.
        let (mut parts, body) = response.into_parts();
        let input = record_stream(
            body.into_data_stream(),
            Provider::Doubao,
            protocol,
            model,
            status,
        );
        let output = match protocol {
            Protocol::Anthropic => {
                Body::from_stream(crate::protocol::anthropic::AnthropicSseStream::new(
                    Box::pin(input),
                    model.to_string(),
                ))
            }
            Protocol::Responses => {
                Body::from_stream(crate::protocol::responses::ResponsesSseStream::new(
                    Box::pin(input),
                    model.to_string(),
                    std::sync::Arc::new(crate::protocol::responses::custom_tool_names(original)),
                ))
            }
            Protocol::OpenAi => Body::from_stream(input),
        };
        parts.headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        parts
            .headers
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        return Response::from_parts(parts, output);
    }
    let bytes = match axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return upstream_error(),
    };
    let raw = String::from_utf8_lossy(&bytes).to_string();
    let sse = if raw.trim_start().starts_with("data:") {
        raw
    } else {
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            return upstream_error();
        };
        let acc = crate::protocol::openai::CompletionAccumulator::from_response(&value);
        format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({
                "id": acc.id.unwrap_or_else(|| format!("chatcmpl_{}", uuid::Uuid::new_v4().simple())),
                "object":"chat.completion.chunk", "created":chrono::Utc::now().timestamp(),
                "model":acc.model.unwrap_or_else(|| model.to_string()),
                "choices":[{"index":0,"delta":{"role":"assistant","content":acc.content},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":acc.prompt_tokens.unwrap_or(0),"completion_tokens":acc.completion_tokens.unwrap_or(0)}
            })
        )
    };
    if !client_stream {
        let acc = crate::protocol::openai::CompletionAccumulator::from_sse(&sse);
        save_usage_record_values(
            Provider::Doubao,
            &protocol_name(protocol),
            model,
            status,
            acc.prompt_tokens,
            acc.completion_tokens,
        );
        let body = match protocol {
            Protocol::Anthropic => crate::protocol::anthropic::aggregate_message(&sse, model),
            Protocol::Responses => crate::protocol::responses::to_response_object(
                &acc,
                model,
                &crate::protocol::responses::custom_tool_names(original),
            ),
            Protocol::OpenAi => openai_response(&acc, model),
        };
        return Json(body).into_response();
    }
    unreachable!("client_stream returned above")
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status == StatusCode::PAYMENT_REQUIRED
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

async fn adapt_response(
    upstream: reqwest::Response,
    provider: Provider,
    protocol: Protocol,
    model: &str,
    client_stream: bool,
    original_request: &Value,
) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let protocol_content_type = match protocol {
        Protocol::OpenAi => "text/event-stream",
        Protocol::Anthropic | Protocol::Responses => "text/event-stream",
    };
    if !status.is_success() {
        let content_type = upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .cloned()
            .unwrap_or(HeaderValue::from_static("application/json"));
        let body_stream = record_stream(upstream.bytes_stream(), provider, protocol, model, status);
        let mut response = Response::new(Body::from_stream(body_stream));
        *response.status_mut() = status;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
        return response;
    }
    if client_stream {
        let stream = record_stream(upstream.bytes_stream(), provider, protocol, model, status);
        let mut response = match protocol {
            Protocol::OpenAi => Response::new(Body::from_stream(stream)),
            Protocol::Anthropic => {
                let mapped = crate::protocol::anthropic::AnthropicSseStream::new(
                    Box::pin(stream),
                    model.to_string(),
                );
                Response::new(Body::from_stream(mapped))
            }
            Protocol::Responses => {
                let custom = std::sync::Arc::new(crate::protocol::responses::custom_tool_names(
                    original_request,
                ));
                let mapped = crate::protocol::responses::ResponsesSseStream::new(
                    Box::pin(stream),
                    model.to_string(),
                    custom,
                );
                Response::new(Body::from_stream(mapped))
            }
        };
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(protocol_content_type),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        return response;
    }
    let text = match upstream.text().await {
        Ok(text) => text,
        Err(_) => return upstream_error(),
    };
    let accumulator = crate::protocol::openai::CompletionAccumulator::from_sse(&text);
    save_usage_record(
        provider,
        protocol,
        model,
        status,
        accumulator.prompt_tokens,
        accumulator.completion_tokens,
    );
    let body = match protocol {
        Protocol::Anthropic => crate::protocol::anthropic::aggregate_message(&text, model),
        Protocol::Responses => crate::protocol::responses::to_response_object(
            &accumulator,
            model,
            &crate::protocol::responses::custom_tool_names(original_request),
        ),
        Protocol::OpenAi => openai_response(&accumulator, model),
    };
    Json(body).into_response()
}

fn record_stream<S, E>(
    stream: S,
    provider: Provider,
    protocol: Protocol,
    model: &str,
    status: StatusCode,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send
where
    S: futures_util::Stream<Item = Result<axum::body::Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let provider = provider;
    let model = model.to_string();
    let protocol = protocol_name(protocol);
    async_stream::stream! {
        let mut stream = Box::pin(stream);
        let mut captured = Vec::new();
        let mut complete = true;
        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    if captured.len() < 2 * 1024 * 1024 {
                        let remaining = 2 * 1024 * 1024 - captured.len();
                        captured.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                    }
                    yield Ok(bytes);
                }
                Err(error) => { complete = false; yield Err(std::io::Error::other(error.to_string())); }
            }
        }
        let text = String::from_utf8_lossy(&captured);
        let acc = crate::protocol::openai::CompletionAccumulator::from_sse(&text);
        if complete { save_usage_record_values(provider, &protocol, &model, status, acc.prompt_tokens, acc.completion_tokens); }
    }
}

fn protocol_name(protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAi => "openai",
        Protocol::Anthropic => "anthropic",
        Protocol::Responses => "responses",
    }
    .into()
}

fn usage_path() -> PathBuf {
    config::store_dir().join("external_gateway_usage.json")
}
fn usage_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn save_usage_record(
    provider: Provider,
    protocol: Protocol,
    model: &str,
    status: StatusCode,
    input: Option<u64>,
    output: Option<u64>,
) {
    save_usage_record_values(
        provider,
        &protocol_name(protocol),
        model,
        status,
        input,
        output,
    )
}

fn save_usage_record_values(
    provider: Provider,
    protocol: &str,
    model: &str,
    status: StatusCode,
    input: Option<u64>,
    output: Option<u64>,
) {
    let _guard = usage_lock().lock().unwrap_or_else(|e| e.into_inner());
    let path = usage_path();
    let mut records = std::fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Vec<Value>>(&data).ok())
        .unwrap_or_default();
    records.push(json!({
        "id": uuid::Uuid::new_v4().to_string(), "provider": provider.key(), "protocol": protocol,
        "model": model, "status": status.as_u16(), "inputTokens": input, "outputTokens": output,
        "totalTokens": input.zip(output).map(|(a,b)| a + b), "completedAt": chrono::Utc::now().timestamp_millis()
    }));
    if records.len() > 500 {
        records.drain(0..records.len() - 500);
    }
    if let Ok(text) = serde_json::to_string(&records) {
        let _ = config::ensure_private_dir(&config::store_dir());
        let _ = config::atomic_write(&path, &text);
    }
}

async fn request_usage(State(state): State<StateData>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return auth_error();
    }
    match usage_snapshot(state.provider) {
        Ok(value) => Json(value).into_response(),
        Err(_) => upstream_error(),
    }
}

/// Read locally persisted request records for one provider. Used by the desktop
/// and server control planes so their UIs do not need cross-origin browser fetches.
pub fn usage_snapshot(provider: Provider) -> Result<Value, String> {
    let records = std::fs::read(usage_path())
        .ok()
        .and_then(|data| serde_json::from_slice::<Vec<Value>>(&data).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.get("provider").and_then(Value::as_str) == Some(provider.key()))
        .collect::<Vec<_>>();
    Ok(json!({"object":"list","data":records}))
}

fn openai_response(
    acc: &crate::protocol::openai::CompletionAccumulator,
    fallback_model: &str,
) -> Value {
    let tool_calls = acc
        .tool_calls
        .iter()
        .enumerate()
        .map(|(i, call)| {
            json!({
                "index": i, "id": call.id, "type": "function",
                "function": {"name": call.name, "arguments": call.arguments}
            })
        })
        .collect::<Vec<_>>();
    let mut message = json!({"role":"assistant", "content": if acc.content.is_empty() { Value::Null } else { json!(acc.content) }});
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }
    let mut body = json!({
        "id": acc.id.clone().unwrap_or_else(|| format!("chatcmpl_{}", uuid::Uuid::new_v4().simple())),
        "object":"chat.completion", "created":acc.created.unwrap_or_else(|| chrono::Utc::now().timestamp()),
        "model":acc.model.as_deref().unwrap_or(fallback_model),
        "choices":[{"index":0,"message":message,"finish_reason":acc.finish_reason.as_deref().unwrap_or("stop")}]
    });
    if let (Some(input), Some(output)) = (acc.prompt_tokens, acc.completion_tokens) {
        body["usage"] =
            json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output});
    }
    body
}

fn tap_axum_response(
    response: Response,
    provider: Provider,
    protocol: Protocol,
    model: &str,
) -> Response {
    let (parts, body) = response.into_parts();
    let status = parts.status;
    let stream = record_stream(body.into_data_stream(), provider, protocol, model, status);
    Response::from_parts(parts, Body::from_stream(stream))
}

fn new_traceparent() -> String {
    format!(
        "00-{}-{}-01",
        uuid::Uuid::new_v4().simple(),
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    )
}

async fn doubao_chat(state: &StateData, cookie: &str, payload: Value) -> Response {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("doubao")
        .to_string();
    let Some((model_key, think_level, agent_mode)) = doubao_model_config(&model) else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":format!("不支持的豆包模型：{model}",),"type":"invalid_request_error"}}))).into_response();
    };
    let messages = match payload.get("messages").and_then(Value::as_array) {
        Some(messages) if !messages.is_empty() => messages,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"messages must be a non-empty array","type":"invalid_request_error"}}))).into_response(),
    };
    let csrf = refresh_doubao_csrf(&state.client, cookie).await;
    let mut prompt = String::new();
    let mut image_attachments = Vec::<Value>::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "tool" || message.get("tool_calls").is_some() {
            return unsupported("豆包网页适配暂不支持工具调用消息");
        }
        let content = match message.get("content") {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Array(parts)) => {
                let mut text = String::new();
                for part in parts {
                    match part.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(value) = part.get("text").and_then(Value::as_str) {
                                text.push_str(value);
                            }
                        }
                        Some("image_url") => {
                            let Some(image_url) =
                                part.pointer("/image_url/url").and_then(Value::as_str)
                            else {
                                return unsupported("豆包图片输入需要 image_url.url");
                            };
                            let (mime, image) = if let Some((mime, encoded)) =
                                parse_image_data_url(image_url)
                            {
                                let image = match BASE64.decode(encoded) {
                                    Ok(image) if image.len() <= 8 * 1024 * 1024 => image,
                                    Ok(_) => return unsupported("单张豆包图片不能超过 8 MiB"),
                                    Err(_) => {
                                        return unsupported("图片 data URL 的 base64 内容无效")
                                    }
                                };
                                (mime, image)
                            } else {
                                match download_remote_image(image_url).await {
                                    Ok(image) => image,
                                    Err(RemoteImageError::Invalid(message)) => return (
                                        StatusCode::BAD_REQUEST,
                                        Json(json!({"error":{"message":message,"type":"invalid_request_error"}})),
                                    ).into_response(),
                                    Err(RemoteImageError::Fetch(message)) => return (
                                        StatusCode::BAD_GATEWAY,
                                        Json(json!({"error":{"message":message,"type":"upstream_error"}})),
                                    ).into_response(),
                                }
                            };
                            match upload_doubao_image(state, cookie, csrf.as_deref(), mime, image)
                                .await
                            {
                                Ok(image) => image_attachments.push(image),
                                Err(error) => return (
                                    StatusCode::BAD_GATEWAY,
                                    Json(
                                        json!({"error":{"message":error,"type":"upstream_error"}}),
                                    ),
                                )
                                    .into_response(),
                            }
                        }
                        _ => return unsupported("豆包网页适配目前只接受文本和图片消息"),
                    }
                }
                text
            }
            _ => return unsupported("豆包网页适配目前只接受文本消息"),
        };
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        if role == "system" || role == "assistant" {
            prompt.push_str(if role == "system" {
                "系统："
            } else {
                "助手："
            });
        }
        prompt.push_str(&content);
    }
    if let Some(object) = payload.as_object() {
        if let Some(name) = object.keys().find(|name| {
            !matches!(
                name.as_str(),
                "model"
                    | "messages"
                    | "stream"
                    | "max_tokens"
                    | "temperature"
                    | "top_p"
                    | "stop"
                    | "frequency_penalty"
                    | "presence_penalty"
            )
        }) {
            return unsupported(&format!("豆包网页适配暂不支持请求参数 `{name}`"));
        }
    }
    let wants_stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let url = match doubao_url("/chat/completion") {
        Ok(url) => url,
        Err(_) => return upstream_error(),
    };
    let mut content_block = vec![
        json!({"block_type":10000,"block_id":uuid::Uuid::new_v4().to_string(),"parent_id":"","meta_info":[],"extra_info":{},"append_fields":[],"is_finish":true,"patch_type":2,"content":{"text_block":{"text":prompt,"icon_url":"","icon_url_dark":"","summary":""},"pc_event_block":""}}),
    ];
    if !image_attachments.is_empty() {
        let attachments = image_attachments.iter().map(|image| json!({
            "type":1,
            "identifier":uuid::Uuid::new_v4().to_string(),
            "image":{
                "name":image["name"],
                "uri":image["uri"],
                "image_ori":{"url":image["cdnUrl"],"width":64,"height":64,"format":image["format"],"url_formats":{}}
            },
            "size":0,"parse_state":1,"review_state":1,"upload_status":1,"progress":100,"src":""
        })).collect::<Vec<_>>();
        content_block.push(json!({"block_type":10052,"block_id":uuid::Uuid::new_v4().to_string(),"parent_id":"","meta_info":[],"append_fields":[],"content":{"attachment_block":{"attachments":attachments},"pc_event_block":""}}));
    }
    let mut option = json!({"send_message_scene":"","create_time_ms":0,"collect_id":"","is_audio":false,"answer_with_suggest":true,"tts_switch":false,"need_deep_think":think_level,"click_clear_context":false,"is_regen":false,"unique_key":uuid::Uuid::new_v4().to_string(),"start_seq":0,"need_create_conversation":true,"conversation_init_option":{"need_ack_conversation":true},"action_bar_skill_id":0,"sse_recv_event_options":{"support_chunk_delta":true}});
    if let Some(model_key) = model_key {
        option["agent_mode"] = json!(agent_mode);
        option["model_config"] = json!({"model_item_key":model_key});
        option["aggregate_params"] = json!({"model_item_key":model_key});
        option["conversation_init_ext"] =
            json!({"model_item_key":model_key,"reasoning_effort":"4","mode_id":"3"});
    }
    let body = json!({
      "client_meta":{"local_conversation_id":format!("local_{}",uuid::Uuid::new_v4().simple()),"conversation_id":"0","bot_id":"7338286299411103781","last_section_id":"","last_message_index":0},
      "messages":[{"local_message_id":uuid::Uuid::new_v4().to_string(),"content_block":content_block,"message_status":0}],
      "option":option,
      "chat_ability":{},"ext":{"use_deep_think":think_level.to_string(),"use_submit_pipeline":"1","commerce_credit_config_enable":"0","sub_conv_firstmet_type":"1"}
    });
    let mut req = state.client.post(url).header(header::COOKIE,cookie).header(header::CONTENT_TYPE,"application/json").header(header::ACCEPT,"text/event-stream").header(header::ORIGIN,"https://www.doubao.com").header(header::REFERER,"https://www.doubao.com/chat/").header(header::USER_AGENT,"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/147.0.0.0 Safari/537.36").header("agw-js-conv","str").json(&body);
    if let Some(csrf) = csrf {
        req = req.header("x-tt-passport-csrf-token", csrf);
    }
    let upstream = match req.send().await {
        Ok(response) => response,
        Err(_) => return upstream_error(),
    };
    if !upstream.status().is_success() {
        return (StatusCode::BAD_GATEWAY, Json(json!({"error":{"message":format!("豆包上游拒绝请求（HTTP {}）", upstream.status()),"type":"upstream_error"}}))).into_response();
    }
    let stream = upstream.bytes_stream();
    if wants_stream {
        let mapped = async_stream::stream! {
            let mut pending = String::new();
            let id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
            let mut thinking_count = 0usize;
            futures_util::pin_mut!(stream);
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else { break };
                pending.push_str(&String::from_utf8_lossy(&chunk));
                pending = pending.replace("\r\n", "\n");
                while let Some(end) = pending.find("\n\n") {
                    let event = pending.drain(..end + 2).collect::<String>();
                    let data = event.lines().find_map(|line| line.strip_prefix("data: "));
                    let Some(data) = data else { continue };
                    let Ok(value) = serde_json::from_str::<Value>(data) else { continue };
                    for (text, is_thinking) in doubao_event_chunks(&value, &mut thinking_count) {
                        let delta = if is_thinking { json!({"reasoning_content":text}) } else { json!({"content":text}) };
                        let chunk = json!({"id":id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
                        yield Ok::<_, std::io::Error>(format!("data: {}\n\n", chunk).into_bytes());
                    }
                    if event.contains("SSE_REPLY_END") || event.contains("event: SSE_REPLY_END") {
                        let chunk = json!({"id":id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
                        yield Ok(format!("data: {}\n\n",chunk).into_bytes());
                        yield Ok(b"data: [DONE]\n\n".to_vec());
                        return;
                    }
                }
            }
            yield Ok(b"data: [DONE]\n\n".to_vec());
        };
        let mut response = Response::new(Body::from_stream(mapped));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        return response;
    }
    let mut pending = String::new();
    let mut answer = String::new();
    let mut reasoning = String::new();
    let mut thinking_count = 0usize;
    futures_util::pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        if let Ok(chunk) = chunk {
            pending.push_str(&String::from_utf8_lossy(&chunk));
            pending = pending.replace("\r\n", "\n");
            while let Some(end) = pending.find("\n\n") {
                let event = pending.drain(..end + 2).collect::<String>();
                if let Some(data) = event.lines().find_map(|line| line.strip_prefix("data: ")) {
                    if let Ok(value) = serde_json::from_str::<Value>(data) {
                        for (text, is_thinking) in doubao_event_chunks(&value, &mut thinking_count)
                        {
                            if is_thinking {
                                reasoning.push_str(&text);
                            } else {
                                answer.push_str(&text);
                            }
                        }
                    }
                }
            }
        }
    }
    Json(json!({"id":format!("chatcmpl-{}",uuid::Uuid::new_v4().simple()),"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"message":{"role":"assistant","content":answer,"reasoning_content":reasoning},"finish_reason":"stop"}]})).into_response()
}

fn parse_image_data_url(value: &str) -> Option<(&'static str, &str)> {
    let value = value.strip_prefix("data:")?;
    let (mime, encoded) = value.split_once(";base64,")?;
    let mime = match mime.to_ascii_lowercase().as_str() {
        "image/png" => "image/png",
        "image/jpeg" | "image/jpg" => "image/jpeg",
        "image/webp" => "image/webp",
        _ => return None,
    };
    (!encoded.is_empty()).then_some((mime, encoded))
}

enum RemoteImageError {
    Invalid(&'static str),
    Fetch(&'static str),
}

async fn download_remote_image(value: &str) -> Result<(&'static str, Vec<u8>), RemoteImageError> {
    const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
    let url =
        reqwest::Url::parse(value).map_err(|_| RemoteImageError::Invalid("图片 URL 格式无效"))?;
    let host = url
        .host_str()
        .ok_or(RemoteImageError::Invalid("图片 URL 必须包含主机名"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || host.parse::<IpAddr>().is_ok()
    {
        return Err(RemoteImageError::Invalid(
            "远程图片只允许使用 HTTPS 域名 URL（默认端口）",
        ));
    }

    // Resolve once, reject any non-public answer, then pin reqwest to the checked
    // addresses. This prevents redirects and DNS rebinding from reaching local nets.
    let addresses = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| RemoteImageError::Fetch("远程图片域名解析失败"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(RemoteImageError::Fetch("远程图片域名没有可用地址"));
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(RemoteImageError::Invalid(
            "远程图片 URL 解析到了非公网地址，已拒绝访问",
        ));
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| RemoteImageError::Fetch("创建远程图片客户端失败"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| RemoteImageError::Fetch("读取远程图片失败"))?;
    if !response.status().is_success() {
        return Err(RemoteImageError::Fetch("远程图片服务器返回错误状态"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
    {
        return Err(RemoteImageError::Invalid("单张豆包图片不能超过 8 MiB"));
    }
    let mime = match response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("image/png") => "image/png",
        Some("image/jpeg") => "image/jpeg",
        Some("image/webp") => "image/webp",
        _ => {
            return Err(RemoteImageError::Invalid(
                "远程图片只接受 PNG、JPEG 或 WebP 格式",
            ))
        }
    };
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RemoteImageError::Fetch("远程图片读取中断"))?;
        if body.len().saturating_add(chunk.len()) > MAX_IMAGE_BYTES {
            return Err(RemoteImageError::Invalid("单张豆包图片不能超过 8 MiB"));
        }
        body.extend_from_slice(&chunk);
    }
    let valid_signature = match mime {
        "image/png" => body.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => body.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => body.len() >= 12 && &body[0..4] == b"RIFF" && &body[8..12] == b"WEBP",
        _ => false,
    };
    if !valid_signature {
        return Err(RemoteImageError::Invalid("远程图片格式与内容类型不匹配"));
    }
    Ok((mime, body))
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_unspecified()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // Only global-unicast 2000::/3, excluding documentation, protocol
            // assignment, and 6to4 ranges that can embed non-public IPv4 targets.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] == 0x0db8 || segments[1] <= 0x01ff)
                    || segments[0] == 0x2002)
        }
    }
}

fn doubao_url(path: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(&format!("https://www.doubao.com{path}"))
        .map_err(|error| format!("豆包接口地址无效：{error}"))?;
    for (name, value) in [
        ("aid", "1044603"),
        ("real_aid", "1044603"),
        ("device_platform", "web"),
        ("doubao_device_platform", "desktop"),
        ("web_platform", "desktop"),
        ("language", "zh"),
        ("region", "CN"),
        ("sys_region", "CN"),
        ("pkg_type", "release_version"),
        ("version_code", "20800"),
        ("pc_version", "2.36.15"),
        ("doubao_pc_version", "2.36.15"),
        ("samantha_web", "1"),
        ("use-olympus-account", "1"),
        ("runtime", "web"),
    ] {
        url.query_pairs_mut().append_pair(name, value);
    }
    Ok(url)
}

async fn upload_doubao_image(
    state: &StateData,
    cookie: &str,
    csrf: Option<&str>,
    mime: &str,
    bytes: Vec<u8>,
) -> Result<Value, String> {
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => return Err("豆包图片格式不支持".into()),
    };
    let filename = format!("image-{}.{}", uuid::Uuid::new_v4().simple(), extension);
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.clone())
        .mime_str(mime)
        .map_err(|_| "豆包图片上传类型无效".to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("data", part)
        .text("file_type", extension.to_string());
    let mut request = state
        .client
        .post(doubao_url("/samantha/pages/upload_image")?)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "https://www.doubao.com")
        .header(header::REFERER, "https://www.doubao.com/chat/")
        .header(header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/147.0.0.0 Safari/537.36")
        .multipart(form);
    if let Some(csrf) = csrf {
        request = request.header("x-tt-passport-csrf-token", csrf);
    }
    let uploaded: Value = request
        .send()
        .await
        .map_err(|_| "豆包图片上传连接失败".to_string())?
        .json()
        .await
        .map_err(|_| "豆包图片上传响应无效".to_string())?;
    if uploaded.get("code").and_then(Value::as_i64) != Some(0) {
        return Err("豆包图片上传未成功，请确认账号 Cookie 有效".into());
    }
    let uri = uploaded
        .pointer("/data/uri")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "豆包图片上传未返回文件标识".to_string())?;
    let mut request = state
        .client
        .post(doubao_url("/alice/message/get_file_url")?)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "https://www.doubao.com")
        .header(header::REFERER, "https://www.doubao.com/chat/")
        .json(&json!({"uris":[uri],"type":"image","format":extension,"expire_second":3600}));
    if let Some(csrf) = csrf {
        request = request.header("x-tt-passport-csrf-token", csrf);
    }
    let urls: Value = request
        .send()
        .await
        .map_err(|_| "读取豆包图片地址失败".to_string())?
        .json()
        .await
        .map_err(|_| "豆包图片地址响应无效".to_string())?;
    if urls.get("code").and_then(Value::as_i64) != Some(0) {
        return Err("豆包未能生成图片预览地址".into());
    }
    let file = urls
        .pointer("/data/file_urls/0")
        .ok_or_else(|| "豆包未返回图片预览地址".to_string())?;
    let file_uri = file
        .get("uri")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "豆包图片响应缺少文件标识".to_string())?;
    let cdn_url = file
        .get("main_url")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("https://"))
        .ok_or_else(|| "豆包图片响应缺少安全的 HTTPS 地址".to_string())?;
    Ok(json!({"uri":file_uri,"cdnUrl":cdn_url,"name":filename,"format":extension}))
}

fn doubao_model_config(model: &str) -> Option<(Option<&'static str>, i32, i32)> {
    Some(match model {
        "doubao" | "doubao-fast" => (None, 0, 0),
        "doubao-think" | "doubao-reason" | "doubao-thinking" => (None, 1, 0),
        "doubao-expert" | "doubao-max" | "doubao-deep" => (None, 3, 0),
        "auto" | "doubao-auto" => (Some("9"), 1, 1),
        "doubao-2.1-lite" | "doubao-lite" => (Some("seed-lite-7b"), 1, 1),
        "doubao-2.1-pro" | "doubao-pro" => (Some("5"), 1, 1),
        "deepseek-v4-flash" => (Some("person-deepseek-v4-1-flash-261017"), 1, 1),
        "kimi-k3.1-pro" => (Some("person-kimi-k3-1-pro-261111"), 3, 1),
        "kimi-k3.1-pro-fast" => (Some("person-kimi-k3-1-pro-highspeed-261111"), 1, 1),
        "kimi-k3" => (Some("person-kimi-k3-261111"), 1, 1),
        "glm-5.3" => (Some("person-glm-5-3-261111"), 1, 1),
        "glm-5.3-flash" => (Some("person-glm-5-3-flash-261111"), 1, 1),
        "deepseek-v4-pro" => (Some("person-deepseek-v4-pro-ga-260813"), 3, 1),
        _ => return None,
    })
}

fn doubao_event_chunks(event: &Value, thinking_count: &mut usize) -> Vec<(String, bool)> {
    if let Some(text) = event.get("text").and_then(Value::as_str) {
        return vec![(text.to_string(), *thinking_count == 1)];
    }
    fn visit(value: &Value, thinking_count: &mut usize, output: &mut Vec<(String, bool)>) {
        match value {
            Value::Object(map) => {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    output.push((text.to_string(), *thinking_count == 1));
                    return;
                }
                if map.get("block_type").and_then(Value::as_i64) == Some(10040) {
                    *thinking_count += 1;
                    return;
                }
                if map.get("block_type").and_then(Value::as_i64) == Some(10000) {
                    if let Some(text) = value
                        .pointer("/content/text_block/text")
                        .and_then(Value::as_str)
                    {
                        output.push((text.to_string(), *thinking_count == 1));
                    }
                    return;
                }
                if let Some(raw) = map.get("content").and_then(Value::as_str) {
                    if let Ok(content) = serde_json::from_str::<Value>(raw) {
                        visit(&content, thinking_count, output);
                    }
                } else {
                    for (key, child) in map {
                        if key != "text_block" {
                            visit(child, thinking_count, output);
                        }
                    }
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, thinking_count, output);
                }
            }
            _ => {}
        }
    }
    let mut output = Vec::new();
    visit(event, thinking_count, &mut output);
    output
}

async fn refresh_doubao_csrf(client: &reqwest::Client, cookie: &str) -> Option<String> {
    let url = "https://www.doubao.com/passport/safe/csrf_token/?aid=1044603&device_platform=web&language=zh&region=CN&sys_region=CN&version_code=20800&pc_version=2.36.15&doubao_pc_version=2.36.15";
    let response = client
        .get(url)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "https://www.doubao.com")
        .header(header::REFERER, "https://www.doubao.com/")
        .send()
        .await
        .ok()?;
    response
        .json::<Value>()
        .await
        .ok()?
        .pointer("/data/passport_csrf_token")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn unsupported(message: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"error":{"message":message,"type":"unsupported_request"}})),
    )
        .into_response()
}
fn upstream_error() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error":{"message":"连接上游失败","type":"upstream_error"}})),
    )
        .into_response()
}

fn authorized(state: &StateData, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .is_some_and(|key| key.as_bytes() == state.api_key.as_bytes())
        || headers
            .get("x-api-key")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|key| key.as_bytes() == state.api_key.as_bytes())
}
fn auth_error() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":{"message":"Invalid API key","type":"authentication_error"}})),
    )
        .into_response()
}

pub async fn spawn_listener(
    provider: Provider,
    key: String,
) -> Result<crate::GatewayHandle, String> {
    let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let addr = SocketAddr::new(ip, provider.port());
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|error| format!("无法监听 {addr}：{error}"))?;
    let actual = listener.local_addr().unwrap_or(addr);
    let app = router(provider, key)?;
    let (tx, rx) = oneshot::channel();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    Ok(crate::GatewayHandle::new(actual, tx, join))
}

#[derive(Default)]
pub struct Runtime {
    handles: Mutex<std::collections::HashMap<String, crate::GatewayHandle>>,
}

impl Runtime {
    pub fn status(&self, provider: Provider) -> Result<Value, String> {
        Ok(
            json!({"provider":provider.key(),"baseUrl":format!("http://127.0.0.1:{}",provider.port()),"apiKey":api_key(provider)?,"running":self.handles.lock().unwrap().contains_key(provider.key()),"port":provider.port()}),
        )
    }
    pub async fn start(&self, provider: Provider) -> Result<Value, String> {
        if self.handles.lock().unwrap().contains_key(provider.key()) {
            return self.status(provider);
        }
        if external_credits::gateway_accounts(provider.key())?.is_empty() {
            return Err("请先在账号入口添加至少一个有效账号".into());
        }
        let handle = spawn_listener(provider, api_key(provider)?).await?;
        self.handles
            .lock()
            .unwrap()
            .insert(provider.key().to_string(), handle);
        self.status(provider)
    }
    pub async fn stop(&self, provider: Provider) -> Result<Value, String> {
        let handle = self.handles.lock().unwrap().remove(provider.key());
        if let Some(handle) = handle {
            handle.shutdown().await;
        }
        self.status(provider)
    }
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(Runtime::default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn provider_gateway_requires_its_own_bearer_key() {
        let app = router(Provider::Raccoon, "local-test-key".into()).unwrap();
        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let accepted = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .header(header::AUTHORIZATION, "Bearer local-test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // No real account store is touched by this test. Reaching the account-pool
        // gate proves the correct Bearer key passed auth; the route then fails closed.
        assert_eq!(accepted.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn doubao_sse_chunks_map_to_openai_content() {
        let mut thinking_count = 0;
        assert_eq!(
            doubao_event_chunks(&json!({"text":"hello"}), &mut thinking_count),
            vec![("hello".into(), false)]
        );
        assert!(doubao_event_chunks(&json!({"patch_op":[{"patch_value":{"content_block":[{"block_type":10040,"content":{"thinking_block":{}}}]}}]}), &mut thinking_count).is_empty());
        assert_eq!(
            doubao_event_chunks(
                &json!({"patch_op":[{"patch_value":{"content_block":[{"block_type":10000,"content":{"text_block":{"text":"reasoning"}}}]}}]}),
                &mut thinking_count
            ),
            vec![("reasoning".into(), true)]
        );
        assert!(doubao_event_chunks(&json!({"patch_op":[{"patch_value":{"content_block":[{"block_type":10040,"content":{"thinking_block":{}}}]}}]}), &mut thinking_count).is_empty());
        assert_eq!(
            doubao_event_chunks(
                &json!({"patch_op":[{"patch_value":{"content_block":[{"block_type":10000,"content":{"text_block":{"text":"answer"}}}]}}]}),
                &mut thinking_count
            ),
            vec![("answer".into(), false)]
        );
    }

    #[test]
    fn doubao_model_ids_map_only_to_verified_upstream_keys() {
        assert_eq!(doubao_model_config("doubao-think"), Some((None, 1, 0)));
        assert_eq!(
            doubao_model_config("deepseek-v4-flash"),
            Some((Some("person-deepseek-v4-1-flash-261017"), 1, 1))
        );
        assert_eq!(doubao_model_config("made-up-model"), None);
    }

    #[test]
    fn raccoon_model_catalog_names_are_extracted_without_duplicates() {
        let mut ids = Vec::new();
        collect_raccoon_models(
            &json!({"data":{"categories":[{"models":[{"name":"sn-glm-5-3"},{"model_name":"sn-kimi-k3"},{"id":"sn-glm-5-3"}]}]}}),
            &mut ids,
        );
        ids.sort();
        ids.dedup();
        assert_eq!(ids, vec!["sn-glm-5-3", "sn-kimi-k3"]);
    }
}
