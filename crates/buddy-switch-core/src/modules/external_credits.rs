//! 用户主动添加的第三方积分来源（豆包、Loomy、商汤小浣熊）。
//!
//! 这些服务不是 WorkBuddy Switch 的可切换客户端平台；本模块只保管用户提供的会话，
//! 查询余额并把脱敏结果呈现在「用量 → 积分」。凭据文件沿用本仓 owner-only 原子写入。

use chrono::Utc;
use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

use super::config::{atomic_write, ensure_private_dir, store_dir};

const FILE_NAME: &str = "external_credit_accounts.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const DOUBAO_DETAIL: &str = "https://www.doubao.com/alice/ug/campaign/v3/detail/";
const LOOMY_RECORDS: &str =
    "https://loomyad.xunfei.cn/api/v1/points/records?pageNo=1&pageSize=1&recordType=all";
const RACCOON_BALANCE: &str = "https://xiaohuanxiong.com/api/web/points/v1/balance";
const RACCOON_REFRESH: &str = "https://xiaohuanxiong.com/api/web/auth/v1/refresh";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    provider: String,
    name: String,
    #[serde(default)]
    credential: String,
    #[serde(default)]
    refresh_credential: String,
    #[serde(default)]
    remaining: Option<f64>,
    #[serde(default)]
    daily_remaining: Option<f64>,
    #[serde(default)]
    updated_at: Option<i64>,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    accounts: Vec<Account>,
}

/// Gateway-only credential view. This type is deliberately not used in API
/// responses: credentials stay inside the local gateway process.
#[derive(Debug, Clone)]
pub struct GatewayAccount {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub credential: String,
    pub refresh_credential: String,
}

/// Read credentials for one provider for the local gateway router.
///
/// This is separate from `list_accounts_for`, whose serialized response is
/// explicitly credential-free.
pub fn gateway_accounts(provider: &str) -> Result<Vec<GatewayAccount>, String> {
    provider_label(provider).ok_or("暂不支持该积分来源")?;
    let store = load_store()?;
    Ok(store.accounts.into_iter()
        .filter(|account| account.provider == provider)
        .map(|account| GatewayAccount {
            id: account.id,
            provider: account.provider,
            name: account.name,
            credential: account.credential,
            refresh_credential: account.refresh_credential,
        })
        .collect())
}

fn file_path() -> PathBuf {
    store_dir().join(FILE_NAME)
}

/// 串行化「读盘 → 改 → 写盘」。`refresh_provider` 会在两步之间 await 上游十几秒，
/// 期间页面仍可添加/删除账号；没有这把锁，后写的一方会静默覆盖前一方。
fn store_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

fn lock_store() -> std::sync::MutexGuard<'static, ()> {
    store_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn load_store() -> Result<Store, String> {
    let path = file_path();
    if !path.exists() {
        return Ok(Store::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("读取第三方积分账号失败：{error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("解析第三方积分账号失败：{error}"))
}

fn save_store(store: &Store) -> Result<(), String> {
    ensure_private_dir(&store_dir()).map_err(|error| format!("创建积分数据目录失败：{error}"))?;
    let text = serde_json::to_string_pretty(store)
        .map_err(|error| format!("序列化第三方积分账号失败：{error}"))?;
    atomic_write(&file_path(), &text).map_err(|error| format!("保存第三方积分账号失败：{error}"))
}

fn provider_label(provider: &str) -> Option<&'static str> {
    match provider {
        "doubao" => Some("豆包"),
        "loomy" => Some("Loomy"),
        "raccoon" => Some("商汤小浣熊"),
        _ => None,
    }
}

/// 返回不包含任何凭据的账号列表与最后一次余额快照。
pub fn list_accounts() -> Result<Value, String> {
    list_accounts_for(None)
}

pub fn list_accounts_for(provider: Option<&str>) -> Result<Value, String> {
    if let Some(provider) = provider {
        provider_label(provider).ok_or("暂不支持该积分来源")?;
    }
    let store = load_store()?;
    Ok(json!({
        "accounts": store.accounts.iter()
            .filter(|account| provider.is_none_or(|provider| account.provider == provider))
            .map(|account| json!({
            "id": account.id,
            "provider": account.provider,
            "name": account.name,
            "remaining": account.remaining,
            "dailyRemaining": account.daily_remaining,
            "unit": match account.provider.as_str() { "doubao" => "额度", _ => "积分" },
            "updatedAt": account.updated_at,
            "error": account.last_error,
        })).collect::<Vec<_>>(),
    }))
}

/// 添加一条第三方账号。`credential` 是 Doubao Cookie、Loomy session 或小浣熊 token/Cookie；
/// 小浣熊可选提供旧版 refresh token。账号原文不会从查询接口返回。
pub fn add_account(
    provider: &str,
    name: &str,
    credential: &str,
    refresh_credential: Option<&str>,
) -> Result<Value, String> {
    let provider = provider.trim().to_ascii_lowercase();
    let label = provider_label(&provider).ok_or("暂不支持该积分来源")?;
    let credential = credential.trim();
    let mut credential = credential.to_string();
    let mut refresh_credential = refresh_credential.unwrap_or_default().trim().to_string();
    if provider == "doubao" {
        credential = strip_header_prefix(&credential, "cookie").to_string();
    }
    if provider == "raccoon" {
        /* 整段 Cookie 里两个键都可能出现，独立各取一次 —— 只取第一个命中会把
           `raccoon_refresh_token` 丢掉。 */
        let token = raccoon_cookie_pair(&credential, "xhx_token").map(|(_, value)| value.to_string());
        let refresh = raccoon_cookie_pair(&credential, "raccoon_refresh_token").map(|(_, value)| value.to_string());
        if token.is_some() || refresh.is_some() {
            match token {
                Some(value) => credential = value,
                None => credential.clear(),
            }
            if refresh_credential.is_empty() {
                if let Some(value) = refresh {
                    refresh_credential = value;
                }
            }
        } else if let Some(token) = credential.strip_prefix("Bearer ") {
            credential = token.trim().to_string();
        }
    }
    if credential.is_empty() && !(provider == "raccoon" && !refresh_credential.is_empty()) {
        return Err("凭据不能为空".into());
    }
    let _guard = lock_store();
    let mut store = load_store()?;
    let id = uuid::Uuid::new_v4().to_string();
    let account = Account {
        id: id.clone(),
        provider,
        name: if name.trim().is_empty() {
            label.to_string()
        } else {
            name.trim().to_string()
        },
        credential,
        refresh_credential,
        remaining: None,
        daily_remaining: None,
        updated_at: None,
        last_error: None,
    };
    store.accounts.push(account);
    save_store(&store)?;
    Ok(json!({ "id": id }))
}

pub fn remove_account(account_id: &str) -> Result<Value, String> {
    remove_account_for(account_id, None)
}

pub fn remove_account_for(account_id: &str, provider: Option<&str>) -> Result<Value, String> {
    if let Some(provider) = provider {
        provider_label(provider).ok_or("暂不支持该积分来源")?;
    }
    let _guard = lock_store();
    let mut store = load_store()?;
    let before = store.accounts.len();
    store.accounts.retain(|account| {
        account.id != account_id || provider.is_some_and(|provider| account.provider != provider)
    });
    if before == store.accounts.len() {
        return Err("第三方积分账号不存在".into());
    }
    save_store(&store)?;
    Ok(json!({ "ok": true }))
}

/// 逐个刷新，单账号失败时保留其他来源结果，并显式携带错误而不伪造 0。
pub async fn refresh_all() -> Result<Value, String> {
    refresh_provider(None).await
}

pub async fn refresh_provider(provider: Option<&str>) -> Result<Value, String> {
    if let Some(provider) = provider {
        provider_label(provider).ok_or("暂不支持该积分来源")?;
    }
    let mut store = load_store()?;
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("创建积分查询客户端失败：{error}"))?;
    let mut rows = Vec::with_capacity(store.accounts.len());
    for account in &mut store.accounts {
        if provider.is_some_and(|provider| account.provider != provider) {
            continue;
        }
        let result = match account.provider.as_str() {
            "doubao" => fetch_doubao(&client, account).await,
            "loomy" => fetch_loomy(&client, account).await,
            "raccoon" => fetch_raccoon(&client, account).await,
            _ => Err("未知积分来源".to_string()),
        };
        match result {
            Ok(balance) => {
                account.remaining = Some(balance.remaining);
                account.daily_remaining = balance.daily_remaining;
                account.updated_at = Some(Utc::now().timestamp_millis());
                account.last_error = None;
                rows.push(json!({
                    "id": account.id,
                    "provider": account.provider,
                    "name": account.name,
                    "ok": true,
                    "remaining": balance.remaining,
                    "dailyRemaining": balance.daily_remaining,
                    "unit": balance.unit,
                    "updatedAt": account.updated_at,
                }));
            }
            Err(error) => {
                account.last_error = Some(error.clone());
                rows.push(json!({
                    "id": account.id,
                    "provider": account.provider,
                    "name": account.name,
                    "ok": false,
                    "remaining": account.remaining,
                    "dailyRemaining": account.daily_remaining,
                    "updatedAt": account.updated_at,
                    "error": error,
                }));
            }
        }
    }
    /* 刷新期间要逐个 await 上游（每个账号最长 15 秒），期间用户可能已经添加或删除了账号。
       所以这里不写回本函数的快照，而是**重新读盘、只按 id 合并本轮的余额字段** ——
       否则一次刷新会静默撤销期间发生的增删。 */
    /* 锁只包「读盘 → 合并 → 写盘」这一小段：上游 await 若也持锁，一次刷新会把
       添加/删除请求卡住十几秒。 */
    let latest = {
        let _guard = lock_store();
        let mut latest = load_store()?;
        for account in &mut latest.accounts {
            if provider.is_some_and(|provider| account.provider != provider) {
                continue;
            }
            if let Some(refreshed) = store.accounts.iter().find(|candidate| candidate.id == account.id) {
                account.remaining = refreshed.remaining;
                account.daily_remaining = refreshed.daily_remaining;
                account.updated_at = refreshed.updated_at;
                account.last_error = refreshed.last_error.clone();
            }
        }
        save_store(&latest)?;
        latest
    };
    drop(latest);
    Ok(json!({ "accounts": rows }))
}

struct Balance {
    remaining: f64,
    daily_remaining: Option<f64>,
    unit: &'static str,
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn code_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

async fn body_json(response: reqwest::Response, provider: &str) -> Result<Value, String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("{provider} 响应读取失败：{error}"))?;
    let body: Value = serde_json::from_str(&text)
        .map_err(|_| format!("{provider} 返回非 JSON 响应（HTTP {status}）"))?;
    if !status.is_success() {
        return Err(format!("{provider} 请求失败（HTTP {status}）"));
    }
    Ok(body)
}

async fn fetch_doubao(client: &Client, account: &Account) -> Result<Balance, String> {
    let mut url = reqwest::Url::parse(DOUBAO_DETAIL).map_err(|error| error.to_string())?;
    for (key, value) in [
        ("version_code", "20800"),
        ("language", "zh"),
        ("device_platform", "web"),
        ("doubao_device_platform", "web"),
        ("aid", "497858"),
        ("real_aid", "497858"),
        ("pkg_type", "release_version"),
        ("pc_version", "2.32.6"),
        ("doubao_pc_version", "5.72.66"),
        ("samantha_web", "1"),
        ("web_platform", "desktop"),
        ("use-olympus-account", "1"),
        ("web_tab_id", "0"),
        ("device_id", ""),
        ("region", ""),
        ("sys_region", ""),
    ] {
        url.query_pairs_mut().append_pair(key, value);
    }
    let response = client.post(url)
        .header(header::COOKIE, &account.credential)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, "https://www.doubao.com/pc/agent-explore")
        .header(header::ORIGIN, "https://www.doubao.com")
        .header(header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) DoubaoDesktop/2.32.6 Chrome/147.0.7727.149 Safari/537.36")
        .json(&json!({"campaign_group_code":"pc_incentive_task_mvp_v3"}))
        .send().await.map_err(|error| format!("豆包请求失败：{error}"))?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err("豆包登录凭据已失效，请重新添加 Cookie".into());
    }
    let body = body_json(response, "豆包").await?;
    let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 710012001 {
        return Err("豆包登录凭据已失效，请重新添加 Cookie".into());
    }
    if code != 0 {
        return Err(format!("豆包接口返回异常（code {code}）"));
    }
    let remaining = number(body.pointer("/data/progress/total_amount"))
        .ok_or("豆包响应缺少额度字段，未更新余额")?;
    Ok(Balance {
        remaining,
        daily_remaining: None,
        unit: "额度",
    })
}

async fn fetch_loomy(client: &Client, account: &Account) -> Result<Balance, String> {
    let response = client
        .get(LOOMY_RECORDS)
        .header("token", &account.credential)
        .send()
        .await
        .map_err(|error| format!("Loomy 请求失败：{error}"))?;
    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err("Loomy 登录态已失效，请重新添加 session".into());
    }
    let body = body_json(response, "Loomy").await?;
    let code = body.get("code").map(code_string).unwrap_or_default();
    if code == "100002" || code == "020002" {
        return Err("Loomy 登录态已失效，请重新添加 session".into());
    }
    if !matches!(code.as_str(), "000000" | "0") {
        return Err(format!("Loomy 接口返回异常（code {code}）"));
    }
    let data = body
        .pointer("/data/data")
        .or_else(|| body.get("data"))
        .unwrap_or(&body);
    let permanent = number(data.get("balance")).ok_or("Loomy 响应缺少余额字段，未更新积分")?;
    let daily = number(data.get("dailyBalance"));
    let remaining =
        number(data.get("availableBalance")).unwrap_or(permanent + daily.unwrap_or(0.0));
    Ok(Balance {
        remaining,
        daily_remaining: daily,
        unit: "积分",
    })
}

async fn fetch_raccoon(client: &Client, account: &mut Account) -> Result<Balance, String> {
    let token = raccoon_token(&account.credential).unwrap_or(account.credential.as_str());
    let request = |token: &str| {
        client
            .get(RACCOON_BALANCE)
            .header(header::ACCEPT, "application/json")
            .header(header::ORIGIN, "https://xiaohuanxiong.com")
            .header(header::REFERER, "https://xiaohuanxiong.com/work/home")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::COOKIE, format!("xhx_token={token}"))
    };
    let mut response = if token.is_empty() {
        None
    } else {
        Some(
            request(token)
                .send()
                .await
                .map_err(|error| format!("小浣熊请求失败：{error}"))?,
        )
    };
    if response.as_ref().map_or(true, |response| {
        response.status() == StatusCode::UNAUTHORIZED
    }) && !account.refresh_credential.is_empty()
    {
        let refresh = client
            .post(RACCOON_REFRESH)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&json!({"refresh_token": account.refresh_credential}))
            .send()
            .await
            .map_err(|error| format!("小浣熊续期失败：{error}"))?;
        let payload = body_json(refresh, "小浣熊续期").await?;
        let data = payload.get("data").unwrap_or(&payload);
        let access = data
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or("");
        if access.is_empty() {
            return Err("小浣熊续期响应缺少 access token，请重新添加登录凭据".into());
        }
        account.credential = access.to_string();
        if let Some(next) = data.get("refresh_token").and_then(Value::as_str) {
            if !next.is_empty() {
                account.refresh_credential = next.to_string();
            }
        }
        response = Some(
            request(&account.credential)
                .send()
                .await
                .map_err(|error| format!("小浣熊请求失败：{error}"))?,
        );
    }
    let response = response.ok_or("小浣熊缺少访问凭据，请重新添加 token")?;
    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err("小浣熊登录态已失效，请重新添加 token".into());
    }
    let body = body_json(response, "小浣熊").await?;
    let code = body
        .get("code")
        .map(code_string)
        .unwrap_or_else(|| "0".to_string());
    if code == "200001" || code == "200003" {
        return Err("小浣熊登录态已失效，请重新添加 token".into());
    }
    if code != "0" {
        return Err(format!("小浣熊接口返回异常（code {code}）"));
    }
    let data = body.get("data").unwrap_or(&body);
    let remaining =
        number(data.get("available_points")).ok_or("小浣熊响应缺少可用积分字段，未更新余额")?;
    Ok(Balance {
        remaining,
        daily_remaining: None,
        unit: "积分",
    })
}

fn raccoon_token(raw: &str) -> Option<&str> {
    raccoon_cookie_pair(raw, "xhx_token").map(|(_, value)| value)
}

/// 从整段 Cookie 里按名字取一个值。`raccoon_cookie_value` 只返回 `find_map` 的**第一个**
/// 命中，所以粘贴同时含 `xhx_token` 和 `raccoon_refresh_token` 的 Cookie 时，
/// 旧实现会把 refresh token 丢掉（登录态过期后无法自动刷新）。
fn raccoon_cookie_pair<'a>(raw: &'a str, wanted: &str) -> Option<(&'a str, &'a str)> {
    let cookie = strip_header_prefix(raw, "cookie");
    cookie.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        let key = key.trim();
        key.eq_ignore_ascii_case(wanted).then_some((key, value.trim()))
    })
}

fn strip_header_prefix<'a>(raw: &'a str, header_name: &str) -> &'a str {
    let raw = raw.trim();
    raw.split_once(':')
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case(header_name))
        .map(|(_, value)| value.trim())
        .unwrap_or(raw)
}
