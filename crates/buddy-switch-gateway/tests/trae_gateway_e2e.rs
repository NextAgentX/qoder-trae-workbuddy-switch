// 测试串行锁（`guard()` / `ENV_LOCK`）会被**故意**持有到用例结束：它防的是多个用例
// 同时改同一个进程级状态（环境变量 / 全局配置）。`#[tokio::test]` 默认 current-thread，
// 不存在「持锁跨线程唤醒」的风险；clippy::await_holding_lock 在这里是误报。
#![allow(clippy::await_holding_lock)]
//! Trae 网关**端到端**测试：真实 HTTP 出站 + 真实路由 + 真实 SSE 转换。
//!
//! # 为什么必须有这一层
//!
//! 网关里有三块逻辑，单测覆盖不到、编译也不会报：
//!
//! 1. **鉴权中间件是否真的挂在路由上**——`bearer_auth` 单测全绿，但若 `router()`
//!    忘了挂 layer，端口一开就是**任何人可白嫖额度**。这类「零护栏接线」只能端到端发现。
//! 2. **换号重试是否真的换**——`pool.pick` 的单测证明「能给下一个账号」，
//!    但证明不了 `stream_chat` / `aggregate_chat` 的循环真的会在失败后再挑一次。
//! 3. **SOLO SSE → OpenAI SSE 的转换在真实 HTTP 流下是否成立**——解析器单测喂的是
//!    构造好的分片，而真实 `reqwest` 响应体的分片边界不由我们决定。
//!
//! 本机没有真实 Trae 账号（`~/.buddy-switch/trae/checkin_accounts.json` 不存在），
//! 无法联调真实上游。因此这里用**本地 mock 上游**顶替：出站地址经
//! [`buddy_switch_gateway::trae::TraeGatewayState::upstream`] 覆盖到 `127.0.0.1`，
//! 其余链路（选号 / 冷却写回 / 请求体改写 / 响应转换）全部走生产代码。
//!
//! # 环境隔离
//!
//! 与 `body_limit.rs` 同一套路：`BUDDY_SWITCH_HOME` 指向已存在的临时目录，
//! 同进程内用一把互斥锁串行化（env 是进程级全局状态）。

use std::sync::Mutex;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::routing::post;
use tower::ServiceExt;

use buddy_switch_core::modules::trae::variant::TraeVariant;
use buddy_switch_gateway::trae::apikey::TraeApiKeyStore;
use buddy_switch_gateway::trae::{
    ensure_api_key, router, status_view, TraeGatewayConfig, TraeGatewayState,
};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` 的用例。
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 容忍锁中毒：某个用例断言失败并持锁 panic 后，其余用例仍应各自报告真实结果，
/// 而不是在取锁处集体失败（"一处失败、满屏失败"会掩盖真正的缺陷）。
fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 把 `BUDDY_SWITCH_HOME` 指向一个已存在的临时目录，并写入一份账号库。
///
/// 注意层级：`BUDDY_SWITCH_HOME` 是**主目录**，store 在其下的 `.buddy-switch/`，
/// 因此 Trae 数据落在 `<home>/.buddy-switch/trae/`。少写一层 `.buddy-switch`
/// 会静默地读到空账号库（症状是每个请求都 503「无可用账号」）。
fn isolated_home(tag: &str, accounts: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "trae-gw-e2e-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let trae_dir = dir.join(".buddy-switch").join("trae");
    std::fs::create_dir_all(&trae_dir).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    write_accounts_for(&dir, TraeVariant::default(), accounts);
    dir
}

/// 往隔离 home 写入**指定变体**的账号库（默认变体沿用无后缀的旧文件名）。
///
/// 用于验证「池按变体分家」：Trae CN 的账号写进 `checkin_accounts.trae_cn.json`，
/// 与 Work 的 `checkin_accounts.json` 各自独立。
fn write_accounts_for(home: &std::path::Path, variant: TraeVariant, accounts: &[(&str, &str)]) {
    // ⚠️ 账号库自 2026-09-21 起按**区域**分家：国内两个产品线（`TraeWork`/`Trae`）
    // **共用** `checkin_accounts.json`，国际版用 `checkin_accounts.global.json`。
    //
    // 这里必须写**区域文件**。曾经按变体名拼 `checkin_accounts.trae_cn.json` ——
    // 那是分家前的命名，翻转后已无人读取，于是池读不到账号、请求一律 503
    // （症状看起来像「网关坏了」，实则种子写错了地方）。
    use buddy_switch_core::modules::trae::region::TraeRegion;
    let name = match variant.region() {
        TraeRegion::Cn => "checkin_accounts.json".to_string(),
        TraeRegion::Global => "checkin_accounts.global.json".to_string(),
    };
    let file = home.join(".buddy-switch").join("trae").join(name);
    let list: Vec<serde_json::Value> = accounts
        .iter()
        .map(|(uid, jwt)| {
            serde_json::json!({ "name": format!("账号 {uid}"), "UserID": uid, "jwt": jwt })
        })
        .collect();
    let body = serde_json::json!({ "accounts": list });
    std::fs::write(file, serde_json::to_string_pretty(&body).unwrap()).expect("写入账号库");
}

/// 在隔离 home 里创建一把归属指定变体的 Key，返回一次性明文。
fn create_key(home: &std::path::Path, variant: TraeVariant) -> String {
    let store = TraeApiKeyStore::new(
        home.join(".buddy-switch")
            .join("trae")
            .join("api_gateway_keys.json"),
    );
    let (_record, plaintext) = store.create(format!("e2e {}", variant.as_str()), variant);
    plaintext
}

/// 读取冷却文件（用于断言错误是否被写回共享状态）。
fn cooldowns(home: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(
        home.join(".buddy-switch")
            .join("trae")
            .join("account_cooldowns.json"),
    )
    .unwrap_or_else(|_| "{}".into());
    serde_json::from_str(&text).unwrap_or(serde_json::json!({}))
}

// ---------------------------------------------------------------------------
// mock 上游
// ---------------------------------------------------------------------------

/// mock 上游的行为。
#[derive(Clone)]
enum Behavior {
    /// 正常返回一段 SOLO 事件流，正文回显 `x-ide-token`（便于断言"这次用的是哪个账号"）。
    Normal,
    /// 连接期直接 401，**响应体里没有**会话作废标记 ⇒ 浅冷却（可自愈）。
    Http401,
    /// 连接期直接 401，响应体带 `12153` ⇒ 真·会话作废（永久禁用）。
    Http401SessionDead,
    /// 连接期直接 429（限流）。
    Http429,
    /// 200 + 流内 `event:error`（套餐额度用尽）。
    StreamError,
    /// 只对指定 jwt 返回 401，其余正常——用于验证换号重试。
    FailForJwt(String),
}

/// mock 上游处理器。
async fn mock_upstream(State(behavior): State<Behavior>, headers: HeaderMap) -> Response {
    let jwt = headers
        .get("x-ide-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();

    let plain = |status: StatusCode, text: &str| {
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .body(Body::from(text.to_string()))
            .expect("构造 mock 响应")
    };

    match behavior {
        Behavior::Http401 => plain(StatusCode::UNAUTHORIZED, "unauthorized"),
        Behavior::Http401SessionDead => plain(
            StatusCode::UNAUTHORIZED,
            r#"{"code":12153,"message":"Offline user session not found"}"#,
        ),
        Behavior::Http429 => plain(StatusCode::TOO_MANY_REQUESTS, "rate limited"),
        Behavior::FailForJwt(target) if jwt == target => {
            plain(StatusCode::UNAUTHORIZED, "session dead")
        }
        Behavior::StreamError => {
            let body = "id:1\nevent:error\ndata:{\"code\":1005,\"message\":\"plan limit\"}\n\n";
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from(body.to_string()))
                .expect("构造 mock 响应")
        }
        _ => {
            // 真实上游的报文形状：`id:N` + `event:` + `data:`，事件之间空行分隔。
            // 正文回显 jwt，这样"最终用的是哪个账号"可以从响应内容直接断言。
            let body = format!(
                "id:1\nevent:output\ndata:{{\"response\":\"{jwt}\"}}\n\n\
                 id:2\nevent:token_usage\ndata:{{\"prompt_tokens\":11,\"completion_tokens\":22,\"total_tokens\":33}}\n\n\
                 id:3\nevent:done\ndata:{{\"finish_reason\":\"stop\"}}\n\n"
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from(body))
                .expect("构造 mock 响应")
        }
    }
}

/// 启动 mock 上游，返回 `http://127.0.0.1:<port>`。
async fn spawn_mock(behavior: Behavior) -> String {
    let app = axum::Router::new()
        .route("/api/agent/v3/llm_utils_chat", post(mock_upstream))
        .with_state(behavior);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定 mock 上游");
    let addr = listener.local_addr().expect("取 mock 地址");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

// ---------------------------------------------------------------------------
// 请求构造与驱动
// ---------------------------------------------------------------------------

fn chat_request(key: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json");
    if let Some(key) = key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("构造请求")
}

/// 组装「已指向 mock 上游」的网关状态，并创建一把默认归属（TraeWork）的 Key。
async fn gateway_for(behavior: Behavior) -> (TraeGatewayState, String) {
    let base = spawn_mock(behavior).await;
    let mut state = TraeGatewayState::new(TraeGatewayConfig::default());
    state.upstream = base;
    let key = ensure_api_key(&state.key_store)
        .expect("隔离 home 无 Key，ensure_api_key 应生成一把并返回明文");
    (state, key)
}

async fn body_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读取响应体");
    String::from_utf8_lossy(&bytes).to_string()
}

// ---------------------------------------------------------------------------
// 鉴权
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_paths_need_no_authorization() {
    let _guard = guard();
    let home = isolated_home("health", &[("uid-a", "jwt-a")]);
    let (state, _key) = gateway_for(Behavior::Normal).await;

    // canonical `/health` + 兼容别名 `/healthz`：两者都必须免鉴权且 200。
    for path in buddy_switch_gateway::HEALTH_PATHS {
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("路由调用");
        assert_eq!(response.status(), StatusCode::OK, "{path} 必须免鉴权");
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn missing_authorization_is_rejected() {
    let _guard = guard();
    let home = isolated_home("noauth", &[("uid-a", "jwt-a")]);
    let (state, _key) = gateway_for(Behavior::Normal).await;

    let response = router(state)
        .oneshot(chat_request(None, r#"{"model":"m","messages":[]}"#))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "没有 Authorization 头必须 401。若这里返回 200，说明 bearer_auth 没挂在路由上——\
         端口一开就是任何人可白嫖额度"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn wrong_api_key_is_rejected() {
    let _guard = guard();
    let home = isolated_home("wrongkey", &[("uid-a", "jwt-a")]);
    let (state, _key) = gateway_for(Behavior::Normal).await;

    let response = router(state)
        .oneshot(chat_request(
            Some("sk-trae-not-the-key"),
            r#"{"model":"m","messages":[]}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn empty_configured_key_rejects_instead_of_allowing_everyone() {
    let _guard = guard();
    let home = isolated_home("emptykey", &[("uid-a", "jwt-a")]);

    // 多 Key 化后「空 Key 放行」已被彻底移除：**没有创建任何 Key** 时，
    // 任何凭据（含空串）都必须被拒——参考实现「留空即不鉴权」意味着猜到端口就能白嫖。
    let state = TraeGatewayState::new(TraeGatewayConfig::default());
    assert!(
        state.key_store.list().is_empty(),
        "隔离 home 不应有任何 Key（否则本用例验证的不是空配置）"
    );

    let response = router(state)
        .oneshot(chat_request(Some(""), r#"{"model":"m","messages":[]}"#))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "没有任何可用 Key 时必须拒绝，而不是放行所有人"
    );
    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// 流式转换
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streaming_request_is_converted_to_openai_sse() {
    let _guard = guard();
    let home = isolated_home("stream", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Normal).await;

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":true}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.contains("text/event-stream"),
        "流式响应必须是 text/event-stream，实际 {content_type}"
    );

    let text = body_text(response).await;
    // SOLO 事件名绝不能泄漏给客户端。
    for leaked in ["event:output", "event:token_usage", "event:done", "id:1"] {
        assert!(
            !text.contains(leaked),
            "上游私有事件名泄漏到响应里：{leaked}\n{text}"
        );
    }
    // 必须转换成 OpenAI 的 `data: {...}` 分片，并且正文可读。
    assert!(text.contains("data: {"), "缺少 OpenAI data 行\n{text}");
    assert!(
        text.contains("chat.completion.chunk"),
        "缺少 chunk 对象\n{text}"
    );
    assert!(text.contains("jwt-a"), "正文应回显所用账号的 jwt\n{text}");
    // **绝不悬挂连接**：无论上游怎么结束，都必须补发 [DONE]。
    assert!(
        text.trim_end().ends_with("data: [DONE]"),
        "流式响应必须以 data: [DONE] 收尾\n{text}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn non_streaming_request_is_aggregated_locally() {
    let _guard = guard();
    let home = isolated_home("aggregate", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Normal).await;

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("非流式必须返回 JSON");
    assert_eq!(
        parsed["object"], "chat.completion",
        "非流式必须是 chat.completion 而不是 chunk 列表\n{text}"
    );
    assert_eq!(parsed["choices"][0]["message"]["content"], "jwt-a");
    assert_eq!(parsed["choices"][0]["finish_reason"], "stop");
    assert_eq!(parsed["usage"]["prompt_tokens"], 11);
    assert_eq!(parsed["usage"]["completion_tokens"], 22);
    assert_eq!(parsed["usage"]["total_tokens"], 33);
    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// 冷却写回与换号
// ---------------------------------------------------------------------------

/// ★★ issue #108 §P0-B：**裸 401 不再是「会话作废」**。
///
/// 改造前这里断言 `type == "SessionDead"` / `until == 9_999_999_999` —— 那是旧契约，
/// 而且正是报障的根因：access token 过期（保活刷新一轮就好）被打成**永久终态**，
/// 用户重新登录也救不回来（冷却表没人清）。
///
/// 现在裸 401/403 落 `AuthRejected`：**浅冷却 60 秒、不禁用、不计数**，
/// 与 WorkBuddy 侧 `UpstreamEvent::AuthRejected` 同一口径。
///
/// 真正的会话作废由下一条用例（带 `12153`）钉住 —— 两条必须同批存在，
/// 缺一条就会把修复做反。
#[tokio::test]
async fn plain_401_is_a_shallow_cooldown_not_a_permanent_dead_session() {
    let _guard = guard();
    let home = isolated_home("dead", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Http401).await;
    let before = chrono::Utc::now().timestamp();

    let response = router(state.clone())
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#,
        ))
        .await
        .expect("路由调用");

    // 唯一账号 + 上游 401 → 没有可换的账号，对客户端仍报 401（最终错误照旧）。
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 关键断言：错误必须写回**共享的**冷却文件，签到页才会显示同一个状态。
    let file = cooldowns(&home);
    let entry = &file["cooldowns"]["uid-a"];
    assert_eq!(
        entry["type"], "AuthRejected",
        "裸 401 必须归「凭据被拒」而不是「会话作废」\n{file}"
    );
    let until = entry["until"].as_i64().expect("until 应为整数");
    assert_ne!(
        until, 9_999_999_999_i64,
        "裸 401 不得写成永久冷却（否则重登也救不回来）\n{file}"
    );
    assert!(
        until <= before + 120,
        "裸 401 应是浅冷却（≈60s），实际 {until} vs {before}\n{file}"
    );
    // 「不得打成禁用终态」这一面由 `trae_pool_credential_fingerprint.rs` 覆盖
    // （它把冷却文件 → 池 `disabled` 整条链跑通），此处只钉文件语义。
    let _ = std::fs::remove_dir_all(&home);
}

/// ★★ issue #108 §P0-B 的前置条件：**带 `12153` 的 401 必须仍是永久禁用**。
///
/// 这条是「降级裸 401」的安全阀：若识别不出真正的会话作废，降级就等于把死号
/// 反复撞墙。改造前 Trae 侧**完全不识别 `12153`**（只有状态码），本用例在改造前
/// 会因 `classify_http` 拿不到 body 而无法表达 —— 这正是「先补识别、再降级」的顺序要求。
#[tokio::test]
async fn upstream_12153_marks_the_account_session_dead_permanently() {
    let _guard = guard();
    let home = isolated_home("dead-12153", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Http401SessionDead).await;

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#,
        ))
        .await
        .expect("路由调用");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let file = cooldowns(&home);
    let entry = &file["cooldowns"]["uid-a"];
    assert_eq!(
        entry["type"], "SessionDead",
        "带 12153 的 401 必须判会话作废\n{file}"
    );
    assert_eq!(
        entry["until"], 9_999_999_999_i64,
        "会话作废必须是永久冷却（短冷却只会反复撞墙）\n{file}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn upstream_429_cools_the_account_without_disabling_it() {
    let _guard = guard();
    let home = isolated_home("rate", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Http429).await;

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let file = cooldowns(&home);
    let entry = &file["cooldowns"]["uid-a"];
    assert_eq!(entry["type"], "SoftRate", "限流必须记成 SoftRate\n{file}");
    let until = entry["until"].as_i64().unwrap_or(0);
    let now = chrono::Local::now().timestamp();
    // 限流是**临时**冷却，不得像 SessionDead 那样写成永久。
    assert!(
        until > now && until < 9_999_999_999,
        "限流冷却时长不合理: {until}\n{file}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn rotation_retries_with_the_next_account_and_reports_success() {
    let _guard = guard();
    // 账号 A 会被 mock 拒绝，账号 B 正常。选号顺序沿用账号库顺序，因此先试 A。
    let home = isolated_home("rotate", &[("uid-a", "jwt-a"), ("uid-b", "jwt-b")]);
    let (state, key) = gateway_for(Behavior::FailForJwt("jwt-a".into())).await;

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "首个账号失败后必须换号重试并成功，而不是把失败直接透给客户端"
    );
    let text = body_text(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("聚合 JSON");
    assert_eq!(
        parsed["choices"][0]["message"]["content"], "jwt-b",
        "最终必须由第二个账号完成请求\n{text}"
    );

    // A 被标记为「凭据被拒」（裸 401，无 12153）⇒ 浅冷却 + 换号，B 不得被牵连。
    //
    // ★ issue #108：这里原来是 `SessionDead` —— 那意味着「一次 401 就把号打死」。
    // 现在裸 401 只冷 60 秒，但**换号行为不变**（这才是本用例真正要钉的东西）。
    let file = cooldowns(&home);
    assert_eq!(file["cooldowns"]["uid-a"]["type"], "AuthRejected", "{file}");
    assert!(
        file["cooldowns"].get("uid-b").is_none(),
        "成功完成的账号不得被写入冷却\n{file}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn stream_error_event_is_surfaced_and_cools_the_account() {
    let _guard = guard();
    let home = isolated_home("streamerr", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::StreamError).await;

    // 非流式：流内错误在聚合阶段暴露，此时响应头还没发，因此可以整轮重试；
    // 只有一个账号 → 换不动 → 最终返回错误。
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::BAD_GATEWAY,
        "业务错误码 1005（套餐额度）无对应 HTTP 语义，统一报 502"
    );

    // `event:error` 的 1005 必须被识别成 PlanLimit，而不是被当成普通业务码。
    let file = cooldowns(&home);
    assert_eq!(
        file["cooldowns"]["uid-a"]["type"], "PlanLimit",
        "1005 必须归类为 PlanLimit（否则额度耗尽的账号会在 10 分钟后被反复重试）\n{file}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn request_is_rejected_when_no_account_is_available() {
    let _guard = guard();
    // 账号库为空：这是新装用户的真实状态。
    let home = isolated_home("noaccount", &[]);
    let (state, key) = gateway_for(Behavior::Normal).await;

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "没有可用账号时应报 503（服务暂时不可用），而不是 500 或挂起"
    );
    let text = body_text(response).await;
    assert!(
        text.contains("no_healthy_account"),
        "错误码必须是 no_healthy_account，客户端才能区分「没账号」与「上游挂了」\n{text}"
    );
    assert!(
        text.contains("账号"),
        "错误文案必须能指导用户去加账号\n{text}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// T02：池按变体分家（修既有洞）+ 日志变体维度 + 归属选池
// ---------------------------------------------------------------------------

/// 既有洞的**回归护栏**：只装 Trae CN 账号的用户，用**归属 Trae CN** 的 Key
/// 必须能选中 CN 账号。改之前池恒读默认变体（TraeWork），这类用户请求必然失败。
#[tokio::test]
async fn cn_only_accounts_with_cn_key_routes_to_the_cn_pool() {
    let _guard = guard();
    // 只装 Trae CN 账号（写进带 `.trae_cn` 后缀的账号库；Work 账号库为空）。
    let home = isolated_home("cn-only-cn-key", &[]);
    write_accounts_for(&home, TraeVariant::Trae, &[("uid-cn", "jwt-cn")]);
    let base = spawn_mock(Behavior::Normal).await;
    let mut state = TraeGatewayState::new(TraeGatewayConfig::default());
    state.upstream = base;
    let key = create_key(&home, TraeVariant::Trae);

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "归属 Trae CN 的 Key 必须能选中 Trae CN 账号（这正是本轮修掉的既有洞）"
    );
    let text = body_text(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("聚合 JSON");
    assert_eq!(
        parsed["choices"][0]["message"]["content"], "jwt-cn",
        "必须由 Trae CN 账号完成请求\n{text}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// 只有**国内**账号、Key 却归属**国际版** → 国际版池为空 → 503，且文案点名区域
/// 并提示另一个区域有账号（不得是 401/500）。
///
/// ⚠️ 契约在 2026-09-21 变了：国内两条产品线**共用一本账号库**（`TraeRegion::Cn`），
/// 因此「Work Key + 只有 CN 账号 ⇒ Work 池为空」这一前提**不再成立** —— 两个标识
/// 读到的是同一批账号。能造出空池的只剩**跨区域**（国内 / 国际是两套互不相通的
/// 账号体系），那也正是现在唯一有意义的划分。
#[tokio::test]
async fn cn_only_accounts_with_global_key_reports_global_pool_empty() {
    let _guard = guard();
    let home = isolated_home("cn-only-global-key", &[]);
    // 账号写在国内库（`TraeVariant::TraeWork` 的区域就是国内）。
    write_accounts_for(&home, TraeVariant::TraeWork, &[("uid-cn", "jwt-cn")]);
    let base = spawn_mock(Behavior::Normal).await;
    let mut state = TraeGatewayState::new(TraeGatewayConfig::default());
    state.upstream = base;
    let key = create_key(&home, TraeVariant::Global);

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "国际版 Key + 只有国内账号 → 国际版池为空 → 503（不是 401/500）"
    );
    let text = body_text(response).await;
    assert!(text.contains("no_healthy_account"), "{text}");
    assert!(
        text.contains("「Trae」"),
        "文案必须点名选中的区域（Trae）\n{text}"
    );
    assert!(
        text.contains("检测到 Trae CN 有账号"),
        "文案必须提示另一个区域有账号，用户才知道该改用哪个区域的 Key\n{text}"
    );
    assert!(
        !text.contains("账号库为空或全部不可用"),
        "不得复用升级前那句变体无关的旧文案\n{text}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// R8③：只有 Work 账号 + legacy（`settings.apiKey`）Key → 与升级前逐字节一致。
#[tokio::test]
async fn legacy_settings_key_routes_to_work_pool_unchanged() {
    let _guard = guard();
    let home = isolated_home("legacy-work", &[("uid-a", "jwt-a")]);
    let legacy = "sk-trae-11111111111111111111111111111111";
    std::fs::write(
        home.join(".buddy-switch")
            .join("trae")
            .join("settings.json"),
        serde_json::json!({ "apiKey": legacy }).to_string(),
    )
    .expect("写入 legacy settings");

    let base = spawn_mock(Behavior::Normal).await;
    let mut state = TraeGatewayState::new(TraeGatewayConfig::default());
    state.upstream = base;

    // legacy 合成记录的归属必须是**默认变体**（第二条零回归前提）。
    // ★ issue #28：默认变体已改为**合并版**（`TraeMerged`）—— 老默认 `TraeWork`
    //   指向已被官方合并卸载的 `TRAE SOLO CN`，继续拿它当默认就是给流量打给死客户端。
    let record = state
        .key_store
        .verify(legacy)
        .expect("旧 settings.apiKey 必须可用（否则升级即全量 401）");
    assert_eq!(record.variant, TraeVariant::default());

    let response = router(state)
        .oneshot(chat_request(
            Some(legacy),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
        ))
        .await
        .expect("路由调用");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "legacy Key 必须照常选中 Work 账号"
    );
    let text = body_text(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("聚合 JSON");
    assert_eq!(parsed["choices"][0]["message"]["content"], "jwt-a");
    let _ = std::fs::remove_dir_all(&home);
}

/// T02④：请求日志必须带 `variant`（Token 统计的变体范围条数据源）。
#[tokio::test]
async fn request_log_carries_the_variant() {
    let _guard = guard();
    let home = isolated_home("log-variant", &[]);
    write_accounts_for(&home, TraeVariant::Trae, &[("uid-cn", "jwt-cn")]);
    let base = spawn_mock(Behavior::Normal).await;
    let mut state = TraeGatewayState::new(TraeGatewayConfig::default());
    state.upstream = base;
    let key = create_key(&home, TraeVariant::Trae);

    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
        ))
        .await
        .expect("路由调用");
    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_text(response).await;

    let log_text = std::fs::read_to_string(
        home.join(".buddy-switch")
            .join("trae")
            .join("api_gateway_logs.json"),
    )
    .expect("请求日志文件应已落盘");
    let value: serde_json::Value = serde_json::from_str(&log_text).expect("日志为 JSON 数组");
    let entries = value.as_array().expect("日志为数组");
    let entry = entries
        .iter()
        .rev()
        .find(|item| item.get("variant").is_some())
        .expect("日志条目必须带 variant 键");
    assert_eq!(
        entry["variant"],
        serde_json::json!("trae_cn"),
        "variant 必须用 as_str() 的下划线形态\n{log_text}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// T02：`status_view` 按区域取池，且响应键集合不变（护栏测试的端到端佐证）。
///
/// ⚠️ 契约在 2026-09-21 变了：国内两个产品线标识读到**同一本库**（账号已按区域合并），
/// 因此「TraeWork 池为空」不再成立；能区分开的是**区域**（国内 / 国际两套账号体系）。
#[tokio::test]
async fn status_view_selects_the_variant_pool_without_new_keys() {
    let _guard = guard();
    let home = isolated_home("status-variant", &[]);
    write_accounts_for(&home, TraeVariant::TraeWork, &[("uid-cn", "jwt-cn")]);
    let state = TraeGatewayState::new(TraeGatewayConfig::default());

    // 国内区域的两个标识都应看到那 1 个账号（共用一本库）。
    for variant in [TraeVariant::TraeWork, TraeVariant::Trae] {
        let cn = status_view(&state, false, None, "test", variant).await;
        assert_eq!(
            cn["pool"]["total"], 1,
            "{variant:?} 属于国内区域，应看到共用库里的 1 个账号"
        );
        // 键集合不变：不得新增 `variant` 键（前端形状靠键集合钉住）。
        assert!(cn.get("api_key_prefix").is_some());
        assert!(
            cn.get("variant").is_none(),
            "status_view 不得新增 variant 键"
        );
    }
    // 国际版是另一套账号体系 → 空池。
    let global = status_view(&state, false, None, "test", TraeVariant::Global).await;
    assert_eq!(
        global["pool"]["total"], 0,
        "国际版池应为空（两套独立账号体系）"
    );

    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// Anthropic 协议入口（`/v1/messages`）
// ---------------------------------------------------------------------------

/// `/v1/messages` 请求构造。
fn messages_request(key: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json");
    if let Some(key) = key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("构造请求")
}

/// ★ `/v1/messages` **必须**走鉴权。
///
/// 它是新加的路由 —— 若 `router()` 忘了把 layer 挂在它上面，端口一开就是
/// **任何人可白嫖额度**，而这类「零护栏接线」单测抓不到（见本文件头的说明）。
#[tokio::test]
async fn anthropic_endpoint_requires_authorization() {
    let _guard = guard();
    let home = isolated_home("anthropic-auth", &[("uid-a", "jwt-a")]);
    let (state, _key) = gateway_for(Behavior::Normal).await;
    let app = router(state);

    let body = serde_json::json!({"model": "m", "max_tokens": 1, "messages": []}).to_string();
    let response = app
        .oneshot(messages_request(None, &body))
        .await
        .expect("请求应完成");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 错误体必须是 **Anthropic 形状** —— 同一个 Anthropic 客户端要能读懂。
    let text = body_text(response).await;
    let value: serde_json::Value = serde_json::from_str(&text).expect("错误体应是 JSON");
    assert_eq!(value["type"], "error", "错误体形状：{text}");
    assert!(value["error"]["message"].is_string(), "错误体形状：{text}");

    let _ = std::fs::remove_dir_all(&home);
}

/// ★★ 非流式：客户端要 JSON，就必须给 JSON（**不是**事件流）。
///
/// 回归背景（本用例写下之前刚修掉）：`to_upstream_request` 会把上游的 `stream`
/// 强制为 `true`，若拿**转换后**的体判断流式开关，这里会收到 SSE ——
/// 而客户端在等 JSON，且不会报错、只会解析失败。
#[tokio::test]
async fn anthropic_non_streaming_returns_a_message_object() {
    let _guard = guard();
    let home = isolated_home("anthropic-json", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Normal).await;
    let app = router(state);

    // 刻意**不带** `stream`：Anthropic 的默认就是非流式。
    let body = serde_json::json!({
        "model": "m",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "hi"}],
    })
    .to_string();

    let response = app
        .oneshot(messages_request(Some(&key), &body))
        .await
        .expect("请求应完成");
    assert_eq!(response.status(), StatusCode::OK);

    let text = body_text(response).await;
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("非流式必须返回 JSON，实际：{text}"));

    assert_eq!(value["type"], "message", "Anthropic 响应体：{text}");
    assert_eq!(value["role"], "assistant");
    // ★ `msg_` 前缀只应出现**一次**。
    //
    // `message_from_accumulator` 会在上游 id 上**再包一层** `msg_`；若上游 id 自身
    // 也带 `msg_`，就会得到 `msg_msg_xxx` —— 真实请求实测到过这个形态，
    // 而只断言 `starts_with("msg_")` 是抓不住的（双重前缀同样满足）。
    let id = value["id"].as_str().unwrap_or_default();
    assert!(id.starts_with("msg_"), "id 应是 msg_ 前缀：{text}");
    assert_eq!(
        id.matches("msg_").count(),
        1,
        "msg_ 前缀不得重复（上游 chat id 不应自带 msg_）：{id}"
    );
    let content = value["content"].as_array().expect("content 应是数组");
    assert!(!content.is_empty(), "正文不能为空：{text}");
    assert_eq!(content[0]["type"], "text");
    // mock 上游回显 jwt ⇒ 正文非空，证明**整条链路**真的通了（不只是形状对）。
    assert!(
        content[0]["text"].as_str().is_some_and(|t| !t.is_empty()),
        "正文应有内容：{text}"
    );
    assert_eq!(value["usage"]["input_tokens"], 11);
    assert_eq!(value["usage"]["output_tokens"], 22);

    // ★ 日志端点必须记成 `/v1/messages`：否则「Token 统计」页会把 Anthropic 流量
    // 算进 OpenAI 端点 —— 用户看到的分布是错的，而且**不报错**。
    let log_path = home
        .join(".buddy-switch")
        .join("trae")
        .join("api_gateway_logs.json");
    let log_text = std::fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        log_text.contains("/v1/messages"),
        "日志应记录 Anthropic 端点名，实际：{log_text}"
    );
    assert!(
        !log_text.contains("/v1/chat/completions"),
        "Anthropic 请求不得被记成 chat completions：{log_text}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// ★★ 流式：必须产出 **Anthropic 事件序列**（不是 OpenAI 的 data-only 形态）。
#[tokio::test]
async fn anthropic_streaming_emits_anthropic_events() {
    let _guard = guard();
    let home = isolated_home("anthropic-sse", &[("uid-a", "jwt-a")]);
    let (state, key) = gateway_for(Behavior::Normal).await;
    let app = router(state);

    let body = serde_json::json!({
        "model": "m",
        "max_tokens": 100,
        "stream": true,
        "messages": [{"role": "user", "content": "hi"}],
    })
    .to_string();

    let response = app
        .oneshot(messages_request(Some(&key), &body))
        .await
        .expect("请求应完成");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );

    let text = body_text(response).await;
    // 首尾事件必须齐：缺 `message_start` 客户端根本无法开始解析。
    for event in ["message_start", "content_block_delta", "message_stop"] {
        assert!(
            text.contains(&format!("event: {event}")),
            "缺 Anthropic 事件 `{event}`：{text}"
        );
    }
    // 反向断言：绝不能漏出 OpenAI chunk 形态（那说明转换层没接上）。
    assert!(
        !text.contains("chat.completion.chunk"),
        "不应出现 OpenAI chunk（转换层没接上）：{text}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// P11 模型禁用：出口① / 出口② 的**路由级**端到端（T04 #5）
// ---------------------------------------------------------------------------

fn models_request(key: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/v1/models")
        .header("authorization", format!("Bearer {key}"))
        .body(Body::empty())
        .expect("构造请求")
}

/// 取 `/v1/models` 返回的模型 id 列表（走真实路由 + 鉴权中间件）。
async fn models_json(app: &axum::Router, key: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(models_request(key))
        .await
        .expect("请求应完成");
    assert_eq!(response.status(), StatusCode::OK, "/v1/models 应 200");
    serde_json::from_str(&body_text(response).await).expect("响应应为 JSON")
}

async fn models_ids(app: &axum::Router, key: &str) -> Vec<String> {
    models_json(app, key).await["data"]
        .as_array()
        .expect("data 应为数组")
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect()
}

/// ★ Max Mode 的对外表现（issue #53 三版）：清单里**一个模型只有一条记录**，
/// 不再有 `<模型>-max` 影子条目；窗口数字来自官方目录（本用例没有目录缓存 ⇒
/// 回落到客户端清单，只验「不虚报、不产生影子 id」这两条硬约束）。
///
/// 目录驱动的数字口径由 `payload` 的单测钉住（`catalog_decides_per_model_window_...`）。
#[tokio::test]
async fn max_mode_has_no_shadow_ids() {
    let _guard = guard();
    let home = isolated_home("max-mode", &[("uid-mm", "jwt-mm")]);
    let key = create_key(&home, TraeVariant::default());
    let app = router(TraeGatewayState::new(TraeGatewayConfig::default()));

    let body = models_json(&app, &key).await;
    let rows = body["data"].as_array().expect("data 应为数组");
    let ids: Vec<&str> = rows.iter().filter_map(|row| row["id"].as_str()).collect();

    assert!(
        ids.contains(&"glm-5.3"),
        "静态兜底清单必须有 glm-5.3：{ids:?}"
    );
    assert!(
        !ids.iter()
            .any(|id| id.ends_with("-max") && *id != "qwen3.8-max"),
        "不得出现 `-max` 影子条目：{ids:?}"
    );
    let glm = rows.iter().find(|row| row["id"] == "glm-5.3").unwrap();
    // CI 里没有 Trae 客户端库 ⇒ 走静态兜底（窗口 = 兜底常量）；本地有客户端时是 200k。
    // 这里只要求「必须是正数、不能是 null」—— 具体数字由目录/客户端决定，不是本用例的判据。
    assert!(
        glm["context_length"].as_i64().unwrap_or(0) > 0,
        "没有目录时也要给出一个正数窗口，不能是 0/null：{glm:?}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// ★★ P11 **路由级**端到端：被禁用的模型**真的**不在 `/v1/models`，且**真的**被调用拒绝（403）。
///
/// # 为什么必须有这一条（T04 #5）
///
/// 门禁里的 P11 断言是**源码结构**扫描（证明「代码在正确位置调用了 `is_disabled`」），
/// 证明不了「跑起来确实生效」——若把过滤接到错的集合 / 错的 region，源码断言仍会绿。
/// 本条用**真实路由 + 真实鉴权 + 真实禁用存储**证明：出口① **真的少一条**、
/// 出口② **真的回 403 `model_disabled`**。这是「假开关」的最终裁判。
///
/// ⚠️ 禁用存储是**进程级全局**（[`buddy_switch_gateway::disabled_models::shared`]）——
/// 与两个出口读的是同一份。用例结束时复位，且整支文件用 `guard()` 串行化。
#[tokio::test]
async fn p11_disabled_model_is_hidden_from_models_and_rejected_on_call() {
    let _guard = guard();
    let variant = TraeVariant::default();
    let home = isolated_home("p11-disable", &[("uid-p11", "jwt-p11")]);
    let key = create_key(&home, variant);
    let state = TraeGatewayState::new(TraeGatewayConfig::default());
    let app = router(state);

    // 隔离 home 读不到客户端 `state.vscdb` ⇒ `/v1/models` 回落**静态兜底清单**。
    // 目标模型必须在那份清单里，否则「少一条」无从谈起。
    let target = "glm-5.3";
    assert!(
        buddy_switch_gateway::trae::payload::MODEL_NAMES.contains(&target),
        "断言前提：目标模型必须在静态兜底清单里"
    );

    let region = buddy_switch_core::modules::product::scope_of_trae_region(variant.region());
    let store = buddy_switch_gateway::disabled_models::shared();

    // 基线：未禁用 ⇒ 出现在 /v1/models。
    let before = models_ids(&app, &key).await;
    assert!(
        before.iter().any(|id| id == target),
        "未禁用时应出现在 /v1/models：{before:?}"
    );

    // 禁用该模型（写进**进程内唯一**的禁用存储 —— 与两个出口读的是同一份）。
    {
        let mut lock = store.write().await;
        lock.set(region, target, true);
    }

    // 出口①：真的少一条。
    let after = models_ids(&app, &key).await;
    assert!(
        !after.iter().any(|id| id == target),
        "禁用后不应出现在 /v1/models：{after:?}"
    );
    // Max Mode 是**策略**（`is_max_mode` 注入），不再产生影子条目 ⇒ 一个模型一条记录，
    // 禁用后正好少一条。
    assert_eq!(after.len() + 1, before.len(), "禁用后应正好少一条");

    // 出口②：硬调用真的回 403 `model_disabled`（在触达上游之前就拒）。
    let response = app
        .clone()
        .oneshot(chat_request(
            Some(&key),
            &format!(r#"{{"model":"{target}","messages":[]}}"#),
        ))
        .await
        .expect("请求应完成");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "被禁模型的调用应 403（而不是放行 / 笼统 4xx）"
    );
    let text = body_text(response).await;
    assert!(
        text.contains("model_disabled"),
        "错误类别应为 `model_disabled`：{text}"
    );
    assert!(text.contains("被禁用"), "文案必须说明「被禁用」：{text}");

    // ★ 大小写变体也必须被拒（T04 #2 的绕过链在**路由级**也堵上）。
    let response = app
        .clone()
        .oneshot(chat_request(
            Some(&key),
            &format!(r#"{{"model":"{}","messages":[]}}"#, target.to_uppercase()),
        ))
        .await
        .expect("请求应完成");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "大写变体的调用也必须 403（否则禁用被绕过）"
    );

    // 清理：禁用存储是**进程级全局**（同进程其余用例会读到它）。
    {
        let mut lock = store.write().await;
        lock.set(region, target, false);
    }
    let _ = std::fs::remove_dir_all(&home);
}
