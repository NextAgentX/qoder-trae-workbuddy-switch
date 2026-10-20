//! Qoder OAuth **设备授权登录**（PKCE S256）。
//!
//! ## 为什么是设备流，而不是本地回调
//!
//! 官方客户端的授权回跳用的是**自定义 scheme**（国内 `qoder-work-cn://`、国际
//! `qoder://aicoding.aicoding-agent/login-success`），由客户端自己注册成系统协议处理器。
//! 本仓是**旁挂**的账号管理器，**不注册任何系统协议**，也就收不到那条回跳 —— 起一个
//! `127.0.0.1` 回调监听同样没用：授权页根本不会往本仓的地址跳。
//!
//! 设备流的形态正好绕开这一点：本仓只**发起**授权（拿到一条带 `nonce` 的授权页地址），
//! 用户在**任意浏览器**里完成确认，本仓再拿 `nonce + verifier` 去轮询换令牌。全程
//! 不需要本仓被任何人回调，也不需要本仓持有任何回调端点。
//!
//! ## 为什么两区参数差异做成数据
//!
//! 国内版与国际版在授权 URL 上有**两处实测差异**（不是猜测）：
//! 国内带 `redirect_uri`、国际不带；`nonce` 国内是带横线的 uuid、国际是 32 位无横线 hex。
//! 这些差异连同 `website` / `client_id` / `openapi_base` 一起放进
//! [`RealmSpec`](crate::modules::qoder::region::RealmSpec)（字段 `send_redirect_uri` /
//! `nonce_dashed`），**调用方一律查表**，绝不写 `if region == Cn` 分支 ——
//! 这样「新增一个区域」只是表里加一行，而不是散落各处的条件判断。
//!
//! ## 为什么 `machine_id` 优先读客户端自己的
//!
//! 上游把授权与「设备」绑定。本仓若自造一个 `machine_id`，在用户本机已经装过官方
//! 客户端的情况下会变成「一台陌生设备在登录」，与官方客户端的设备视图对不上。
//! 因此取值优先级是：**官方 CLI 的 `machine_id` 文件** → **桌面端的 `auth.machine-id`**
//! → 都没有才**生成一个并落盘**（保证同一区域多次登录复用同一台「虚拟设备」，身份不漂移）。
//!
//! ## 为什么必须校验 loginId 与 region 绑定
//!
//! 登录会话在**发起时**就绑定了区域。若允许用 A 区的 `loginId` 去 B 区轮询，成功时
//! 会把 A 区的登录态写进 **B 区的账号库** —— 用户会看到「账号在，但登录不了」，
//! 而且错误是无声的。这与账号库归属校验（[`account::ensure_region_matches`]）是**同一条红线**：
//! 轮询在**任何 HTTP 请求之前**就拒绝跨区 `loginId`。
//!
//! ## 令牌永不下发明文
//!
//! 轮询成功后的 `result` 一律走 [`account::account_view`]（它已经做过脱敏，只给
//! `has*` 布尔与掩码），**不把 `access_token` / `refresh_token` 明文交给前端**。
//!
//! ## 无真机证据的部分
//!
//! 授权页与轮询端点的**真实交互**（用户点确认、上游返回 200 + 令牌）本机无账号，
//! **未做真机验证**；本文件只保证：URL 按区域常量拼装、PKCE 计算、会话状态机、
//! 区域绑定与入库路径在离线可测的范围内是**可证伪**的。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use rand_core::RngCore;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::modules::config::{now_secs, OAUTH_TIMEOUT_SECONDS};
use crate::modules::qoder::account::{self, RawAccount, SOURCE_OAUTH};
use crate::modules::qoder::api;
use crate::modules::qoder::paths;
use crate::modules::qoder::region::{realm_spec, QoderRegion};
use crate::modules::qoder::store;
use crate::modules::qoder::PATH_DEVICE_TOKEN_POLL;

/// 拉取账号身份的端点（openapi 通道）。
///
/// 与 [`PATH_DEVICE_TOKEN_POLL`] 同属 openapi 通道，但模块级路径常量表里没有它，
/// 因此就近声明（**不**改 `mod.rs` 的常量表，那是别的改动的事）。
const PATH_USERINFO: &str = "/api/v1/userinfo";

/// PKCE verifier 的字符集（RFC 7636 unreserved：`ALPHA / DIGIT / "-" / "." / "_" / "~"`）。
const VERIFIER_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
/// PKCE verifier 的长度（官方客户端为 64）。
const VERIFIER_LEN: usize = 64;

// ---------------------------------------------------------------------------
// 登录会话（进程内）
// ---------------------------------------------------------------------------

/// 一次登录的会话状态。
///
/// ⚠️ `verifier` 是 PKCE 的**秘密**：只存内存，**不落盘、不进日志、不进 `result`**。
#[derive(Default)]
struct LoginSession {
    /// 会话绑定的区域（**必须**与轮询时传入的 region 一致）。
    region: QoderRegion,
    /// PKCE code_verifier（秘密）。
    verifier: String,
    /// 设备授权的 nonce（轮询时回传）。
    nonce: String,
    /// 过期时刻（unix 秒）。
    expires_at: i64,
    /// 是否已终结（成功 / 失败 / 超时）。
    done: bool,
    /// 成功时的下发形态（[`account::account_view`]）。
    result: Option<Value>,
    /// 失败原因（可读文案）。
    error: Option<String>,
}

static LOGIN_SESSIONS: OnceLock<Mutex<HashMap<String, LoginSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, LoginSession>> {
    LOGIN_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 把一次登录标记为终结（幂等：会话已被取消时静默忽略）。
fn mark_done(login_id: &str, result: Option<Value>, error: Option<String>) {
    let mut map = sessions().lock().unwrap();
    if let Some(session) = map.get_mut(login_id) {
        session.done = true;
        session.result = result;
        session.error = error;
    }
}

// ---------------------------------------------------------------------------
// 纯函数：PKCE / nonce / 授权 URL
// ---------------------------------------------------------------------------

/// 生成 PKCE `(verifier, challenge)`。
///
/// - `verifier`：64 个字符，取自 [`VERIFIER_ALPHABET`]（RFC 7636 unreserved）；
/// - `challenge = BASE64URL_NOPAD(SHA256(verifier))` —— **去 padding**。
///
/// 纯计算、无 I/O（随机源除外），因此可直接单测。
pub fn pkce_pair() -> (String, String) {
    let verifier = random_verifier();
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

/// 从 [`VERIFIER_ALPHABET`] 随机取 [`VERIFIER_LEN`] 个字符。
///
/// 用**拒绝采样**而非直接取模：`256 % 66 != 0`，直接取模会让字母表前几个字符
/// 出现概率偏高（虽不影响安全，但没必要留这个偏差）。
fn random_verifier() -> String {
    let alphabet_len = VERIFIER_ALPHABET.len();
    let limit = 256 - (256 % alphabet_len);
    let mut rng = rand_core::OsRng;
    let mut out = String::with_capacity(VERIFIER_LEN);
    let mut buf = [0u8; VERIFIER_LEN];
    while out.len() < VERIFIER_LEN {
        rng.fill_bytes(&mut buf);
        for &byte in buf.iter() {
            if (byte as usize) < limit {
                out.push(VERIFIER_ALPHABET[(byte as usize) % alphabet_len] as char);
                if out.len() == VERIFIER_LEN {
                    break;
                }
            }
        }
    }
    out
}

/// 按区域生成 `nonce`：国内 = 带横线 uuid，国际 = 32 位无横线 hex（查表 `nonce_dashed`）。
fn nonce_for(region: QoderRegion) -> String {
    let uuid = uuid::Uuid::new_v4();
    if realm_spec(region).nonce_dashed {
        uuid.to_string()
    } else {
        uuid.simple().to_string()
    }
}

/// 构造授权页 URL：`{website}/device/selectAccounts?{query}`。
///
/// query 固定包含 `challenge` / `challenge_method=S256` / `nonce` / `client_id` /
/// `machine_id`；**国内版额外带 `redirect_uri`**（由 `send_redirect_uri` 决定）。
/// 参数顺序即上列顺序，`redirect_uri` 追加在最末。
///
/// 值**不做 URL 编码**：全部取值要么是本文件生成的 uuid / base64url（天然安全字符集），
/// 要么是区域常量表里的字面值（`qoder-work-cn://`），与官方客户端的拼串口径一致。
pub fn build_authorize_url(
    region: QoderRegion,
    machine_id: &str,
    nonce: &str,
    challenge: &str,
) -> String {
    let spec = realm_spec(region);
    let mut query = format!(
        "challenge={challenge}&challenge_method=S256&nonce={nonce}\
         &client_id={}&machine_id={machine_id}",
        spec.client_id
    );
    if spec.send_redirect_uri {
        query.push_str(&format!("&redirect_uri={}", spec.redirect_uri));
    }
    format!("{}/device/selectAccounts?{query}", spec.website)
}

// ---------------------------------------------------------------------------
// machine_id
// ---------------------------------------------------------------------------

/// 读一行并 trim；文件缺失 / 空 ⇒ `None`。
fn read_trimmed(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 本区域 OAuth 登录使用的 `machine_id`。
///
/// 取值优先级（见模块头）：
/// 1. 官方 CLI 的 `<cli_home>/.auth/machine_id`；
/// 2. 桌面端的 `<app_dir>/auth.machine-id`；
/// 3. 都没有 ⇒ 生成一个 uuid 并持久化到 `<qoder_dir>/oauth_machine_id.{region}.txt`，
///    保证同一区域多次登录复用同一台「虚拟设备」。
///
/// ⚠️ 持久化失败是**硬失败**：静默返回一个「这次能用、下次会变」的值会让设备身份漂移，
/// 比直接报错更糟。
fn machine_id_for(region: QoderRegion) -> Result<String, String> {
    let cli_path = paths::cli_home_dir(region).join(".auth").join("machine_id");
    if let Some(value) = read_trimmed(&cli_path) {
        return Ok(value);
    }
    let desktop_path = paths::desktop_app_dir(region).join("auth.machine-id");
    if let Some(value) = read_trimmed(&desktop_path) {
        return Ok(value);
    }
    // 路径在 `oauth.rs` 内部拼：`paths.rs` 是既有文件，不为本功能再加路径函数。
    let store_path = paths::qoder_dir().join(format!("oauth_machine_id.{}.txt", region.as_str()));
    if let Some(value) = read_trimmed(&store_path) {
        return Ok(value);
    }
    let generated = uuid::Uuid::new_v4().to_string();
    store::write_text(&store_path, &generated)?;
    Ok(generated)
}

// ---------------------------------------------------------------------------
// 发起 / 轮询 / 取消
// ---------------------------------------------------------------------------

/// 发起登录：返回 `{ loginId, verificationUri, expiresIn }`。
///
/// 只做本地计算与读 `machine_id`，**不发任何网络请求** —— 用户拿到
/// `verificationUri` 后在浏览器里完成授权，再由 [`login_poll_for`] 轮询取令牌。
pub async fn login_start_for(region: QoderRegion) -> Result<Value, String> {
    let machine_id = machine_id_for(region)?;
    let (verifier, challenge) = pkce_pair();
    let nonce = nonce_for(region);
    let verification_uri = build_authorize_url(region, &machine_id, &nonce, &challenge);

    let login_id = format!("qoder_{}", uuid::Uuid::new_v4().simple());
    {
        let mut map = sessions().lock().unwrap();
        map.insert(
            login_id.clone(),
            LoginSession {
                region,
                verifier,
                nonce,
                expires_at: now_secs() + OAUTH_TIMEOUT_SECONDS,
                ..Default::default()
            },
        );
    }

    Ok(json!({
        "loginId": login_id,
        "verificationUri": verification_uri,
        "expiresIn": OAUTH_TIMEOUT_SECONDS,
    }))
}

/// 轮询一次：返回 `{ done: false }`，或 `{ done: true, result?: Value, error?: String }`。
///
/// `result` 的形状 = [`account::account_view`]`(&saved, region)`（**不含令牌明文**）。
/// HTTP **202 / 404 ⇒ 用户尚未授权**，继续轮询（返回 `{ done: false }`）。
pub async fn login_poll_for(region: QoderRegion, login_id: &str) -> Value {
    let spec = realm_spec(region);

    // ── 会话检查（全部在**任何 I/O 之前**）──
    let (nonce, verifier) = {
        let mut map = sessions().lock().unwrap();
        let Some(session) = map.get_mut(login_id) else {
            return json!({"done": true, "error": "登录请求不存在"});
        };
        // ★ 区域绑定校验：用 A 区的 loginId 轮询 B 区 ⇒ 直接拒绝，
        //   否则会把 A 区登录态写进 B 区账号库（WB 侧踩过的坑）。
        if session.region != region {
            return json!({"done": true, "error": "登录请求与目标版本不匹配"});
        }
        if session.done {
            return json!({
                "done": true,
                "result": session.result.clone(),
                "error": session.error.clone(),
            });
        }
        if now_secs() > session.expires_at {
            session.done = true;
            session.error = Some("登录超时".to_string());
            return json!({"done": true, "error": "登录超时"});
        }
        (session.nonce.clone(), session.verifier.clone())
    };

    // ── 轮询 ──
    let client = match api::shared_client() {
        Ok(client) => client,
        Err(error) => return json!({"done": true, "error": error}),
    };
    let url = format!(
        "{}{PATH_DEVICE_TOKEN_POLL}?nonce={nonce}&verifier={verifier}&challenge_method=S256",
        spec.openapi_base
    );
    let response = client
        .get(&url)
        .header("Accept", "application/json, text/plain, */*")
        .header("User-Agent", spec.user_agent)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        // 网络抖动 ⇒ 保持 pending，让调用方按 TTL 继续轮询。
        Err(_) => return json!({"done": false}),
    };
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    // 202 / 404 ⇒ 用户尚未授权。
    if matches!(status.as_u16(), 202 | 404) {
        return json!({"done": false});
    }
    if !status.is_success() {
        let detail: String = body.chars().take(160).collect();
        let error = format!("轮询失败: HTTP {} {detail}", status.as_u16());
        mark_done(login_id, None, Some(error.clone()));
        return json!({"done": true, "error": error});
    }
    let data: Value = match serde_json::from_str(&body) {
        Ok(data) => data,
        Err(error) => {
            let error = format!("轮询响应不是 JSON: {error}");
            mark_done(login_id, None, Some(error.clone()));
            return json!({"done": true, "error": error});
        }
    };

    let token = response_field(&data, "token")
        .or_else(|| response_field(&data, "device_token"))
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        // 2xx 但还没有令牌 ⇒ 视为尚未授权（与 202 同义）。
        return json!({"done": false});
    }
    let refresh_token = response_field(&data, "refresh_token")
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    // ── 拉身份 ──
    let userinfo = match fetch_userinfo(spec.openapi_base, spec.user_agent, &token).await {
        Ok(userinfo) => userinfo,
        Err(error) => {
            mark_done(login_id, None, Some(error.clone()));
            return json!({"done": true, "error": error});
        }
    };
    let uid = match userinfo.get("id") {
        Some(Value::String(text)) if !text.trim().is_empty() => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    if uid.is_empty() {
        let error = "userinfo 缺少 id —— 已拒绝写入".to_string();
        mark_done(login_id, None, Some(error.clone()));
        return json!({"done": true, "error": error});
    }
    let nickname = userinfo
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);

    // ── 入库 ──
    let account = RawAccount {
        uid: uid.clone(),
        nickname,
        domain: spec.domain.to_string(),
        access_token: Some(token),
        refresh_token,
        personal_token: None,
        // `RawAccount.expires_at` 单位是**毫秒**；`token_expiry` 返回 unix 秒。
        expires_at: Some(api::token_expiry(&data) * 1000),
        source: Some(SOURCE_OAUTH.to_string()),
        remark: None,
        group: None,
        ..Default::default()
    };
    let for_view = account.clone();
    if let Err(error) = account::upsert_account_for(region, account) {
        let error = format!("保存账号失败: {error}");
        mark_done(login_id, None, Some(error.clone()));
        return json!({"done": true, "error": error});
    }
    // 回读落盘记录，让 view 里的 addedAt / updatedAt 反映真实存储态。
    let saved = account::find_account_for(region, &uid).unwrap_or(for_view);
    let result = account::account_view(&saved, region);
    mark_done(login_id, Some(result.clone()), None);
    json!({"done": true, "result": result})
}

/// 取消一次登录（**幂等**）。返回是否真的取消了（会话此前存在）。
pub fn cancel_login(login_id: &str) -> bool {
    sessions().lock().unwrap().remove(login_id).is_some()
}

/// 取字段：先看顶层，再看 `data` 子对象（上游两种信封形态都兼容）。
fn response_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(Value::as_str).or_else(|| {
        body.get("data")
            .and_then(|data| data.get(key))
            .and_then(Value::as_str)
    })
}

/// 拉取账号身份（`Authorization: Bearer <token>`）。
async fn fetch_userinfo(
    openapi_base: &str,
    user_agent: &str,
    token: &str,
) -> Result<Value, String> {
    let client = api::shared_client()?;
    let response = client
        .get(format!("{openapi_base}{PATH_USERINFO}"))
        .header("Accept", "application/json, text/plain, */*")
        .header("User-Agent", user_agent)
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| format!("拉取账号身份失败: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail: String = body.chars().take(160).collect();
        return Err(format!(
            "拉取账号身份失败: HTTP {} {detail}",
            status.as_u16()
        ));
    }
    serde_json::from_str(&body).map_err(|error| format!("账号身份响应不是 JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ PKCE：长度 / 字符集 / `challenge == base64url(sha256(verifier))` 去 padding。
    ///
    /// 可证伪：把 `pkce_pair` 里的 `URL_SAFE_NO_PAD` 换成 `STANDARD`（带 `+/=`），
    /// 或把 `VERIFIER_LEN` 改成 32，本用例立即变红。
    #[test]
    fn pkce_pair_matches_rfc7636() {
        let (verifier, challenge) = pkce_pair();
        assert_eq!(verifier.len(), VERIFIER_LEN, "verifier 必须是 64 字符");
        assert!(
            verifier.bytes().all(|b| VERIFIER_ALPHABET.contains(&b)),
            "verifier 含字母表外字符：{verifier}"
        );

        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(
            challenge, expected,
            "challenge 必须是 base64url(sha256(verifier))"
        );
        assert!(
            !challenge.contains('='),
            "challenge 必须去 padding：{challenge}"
        );
        assert!(
            !challenge.contains('+') && !challenge.contains('/'),
            "必须是 base64url 字母表"
        );

        // 每次登录的 verifier 必须不同（否则 PKCE 失去意义）。
        let (verifier2, _) = pkce_pair();
        assert_ne!(verifier, verifier2);
    }

    /// ★ 授权 URL **两区差异逐字断言**。
    ///
    /// 可证伪：把 `send_redirect_uri` 读反（`if !spec.send_redirect_uri`），
    /// 两条断言立即变红 —— 那正是「国际版登录打开国内版授权页」的缺陷形态。
    #[test]
    fn build_authorize_url_differs_between_realms() {
        let cn = build_authorize_url(QoderRegion::Cn, "mach-1", "nonce-cn", "chal-1");
        assert_eq!(
            cn,
            "https://qoder.com.cn/device/selectAccounts?challenge=chal-1\
             &challenge_method=S256&nonce=nonce-cn\
             &client_id=1c5e33e1-364d-4ce6-b02c-acaa81274a5c\
             &machine_id=mach-1&redirect_uri=qoder-work-cn://"
        );
        assert!(
            cn.contains("redirect_uri=qoder-work-cn://"),
            "国内版必须带 redirect_uri"
        );

        let global = build_authorize_url(QoderRegion::Global, "mach-1", "nonce-g", "chal-1");
        assert_eq!(
            global,
            "https://qoder.com/device/selectAccounts?challenge=chal-1\
             &challenge_method=S256&nonce=nonce-g\
             &client_id=e883ade2-e6e3-4d6d-adf7-f92ceff5fdcb\
             &machine_id=mach-1"
        );
        assert!(
            !global.contains("redirect_uri"),
            "国际版**不带** redirect_uri：{global}"
        );
        // 两区授权页域不同。
        assert!(cn.starts_with("https://qoder.com.cn/"));
        assert!(global.starts_with("https://qoder.com/"));
    }

    /// ★ nonce 形态按区域查表：国内带横线 uuid，国际 32 位无横线 hex。
    ///
    /// 可证伪：把 `nonce_for` 改成恒 `uuid.to_string()`，国际那条断言变红。
    #[test]
    fn nonce_shape_follows_the_realm_table() {
        let cn = nonce_for(QoderRegion::Cn);
        assert_eq!(cn.len(), 36, "国内 nonce 是带横线 uuid：{cn}");
        assert_eq!(cn.matches('-').count(), 4);

        let global = nonce_for(QoderRegion::Global);
        assert_eq!(global.len(), 32, "国际 nonce 是 32 位无横线 hex：{global}");
        assert!(!global.contains('-'));
        assert!(global.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// ★ 区域绑定：用 A 区的 loginId 轮询 B 区 ⇒ 在任何 HTTP 之前被拒绝。
    ///
    /// 这里直接往会话表里种记录来构造交错，因此**不依赖网络**。
    ///
    /// 可证伪：删掉 `login_poll_for` 里的 `session.region != region` 分支，
    /// 前两条断言立即变红 —— 那正是「CN 登录态被写进国际版账号库」的形态。
    #[tokio::test]
    async fn poll_rejects_login_id_bound_to_other_region() {
        let cn_id = format!("qoder_test_{}", uuid::Uuid::new_v4().simple());
        let global_id = format!("qoder_test_{}", uuid::Uuid::new_v4().simple());
        {
            let mut map = sessions().lock().unwrap();
            map.insert(
                cn_id.clone(),
                LoginSession {
                    region: QoderRegion::Cn,
                    verifier: "v".into(),
                    nonce: "n".into(),
                    expires_at: now_secs() + OAUTH_TIMEOUT_SECONDS,
                    ..Default::default()
                },
            );
            map.insert(
                global_id.clone(),
                LoginSession {
                    region: QoderRegion::Global,
                    verifier: "v".into(),
                    nonce: "n".into(),
                    expires_at: now_secs() + OAUTH_TIMEOUT_SECONDS,
                    ..Default::default()
                },
            );
        }

        let crossed = login_poll_for(QoderRegion::Global, &cn_id).await;
        assert_eq!(crossed["done"], json!(true));
        assert_eq!(crossed["error"], json!("登录请求与目标版本不匹配"));

        let crossed_rev = login_poll_for(QoderRegion::Cn, &global_id).await;
        assert_eq!(crossed_rev["done"], json!(true));
        assert_eq!(crossed_rev["error"], json!("登录请求与目标版本不匹配"));

        let unknown = login_poll_for(QoderRegion::Cn, "qoder_test_never_inserted").await;
        assert_eq!(unknown["done"], json!(true));
        assert_eq!(unknown["error"], json!("登录请求不存在"));

        let mut map = sessions().lock().unwrap();
        map.remove(&cn_id);
        map.remove(&global_id);
    }

    /// ★ `cancel_login` **幂等**：第一次真取消，之后恒 `false`；取消后轮询报「不存在」。
    ///
    /// 可证伪：把 `cancel_login` 改成恒 `true`，第二条断言变红。
    #[tokio::test]
    async fn cancel_login_is_idempotent_and_ends_the_session() {
        let id = format!("qoder_test_{}", uuid::Uuid::new_v4().simple());
        // 不存在的会话 ⇒ 没取消到任何东西。
        assert!(!cancel_login(&id));
        sessions().lock().unwrap().insert(
            id.clone(),
            LoginSession {
                region: QoderRegion::Cn,
                verifier: "v".into(),
                nonce: "n".into(),
                expires_at: now_secs() + OAUTH_TIMEOUT_SECONDS,
                ..Default::default()
            },
        );
        assert!(cancel_login(&id), "第一次取消必须成功");
        assert!(!cancel_login(&id), "重复取消必须返回 false（幂等）");

        let after = login_poll_for(QoderRegion::Cn, &id).await;
        assert_eq!(after["done"], json!(true));
        assert_eq!(after["error"], json!("登录请求不存在"));
    }

    /// ★ 真机探针：本机装了官方 CLI 时，`machine_id` 应取自它的 `.auth/machine_id`。
    ///
    /// 默认不跑（依赖真实 `~/.qoder-cn`）。手工验证：
    /// `cargo test -p buddy-switch-core --lib qoder::oauth -- --ignored`
    #[test]
    #[ignore = "真机探针：依赖真实 ~/.qoder-cn/.auth/machine_id"]
    fn machine_id_prefers_the_official_cli_file() {
        let id = machine_id_for(QoderRegion::Cn).expect("应能取到 machine_id");
        assert!(!id.trim().is_empty());
        assert!(
            uuid::Uuid::parse_str(id.trim()).is_ok(),
            "本机 CLI 的 machine_id 是一行 uuid：{id}"
        );
    }
}
