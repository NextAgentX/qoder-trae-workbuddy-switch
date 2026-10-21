// 测试串行锁（`guard()` / `ENV_LOCK`）会被**故意**持有到用例结束：它防的是多个用例
// 同时改同一个进程级状态（环境变量 / 全局配置）。`#[tokio::test]` 默认 current-thread，
// 不存在「持锁跨线程唤醒」的风险；clippy::await_holding_lock 在这里是误报。
#![allow(clippy::await_holding_lock)]
//! Qoder 网关**端到端**测试：真实 HTTP 出站 + 真实路由 + 真实 SSE 转换。
//!
//! # 为什么必须有这一层
//!
//! 与 `trae_gateway_e2e.rs` 同一逻辑，网关里有三块单测覆盖不到的接线：
//! ① 鉴权中间件是否真的挂在路由上（漏挂 = 任何人可白嫖额度）；
//! ② 换号重试是否真的换（单测只证明「能给下一个」，证明不了循环真的会再挑一次）；
//! ③ 信封 SSE → 下游 SSE 在**真实 reqwest 分片**下是否成立。
//!
//! 本机没有真实 Qoder 账号，无法联调真实上游 ⇒ 用**本地 mock 上游**顶替：
//! 出站经 [`QoderGatewayState::with_upstream`] 覆盖到 `127.0.0.1`，其余链路
//! （COSY 签名 / 选号 / 冷却写回 / 请求体构造 / 响应转换）全部走生产代码。
//!
//! # 环境隔离
//!
//! `BUDDY_SWITCH_HOME` 指向已存在的临时目录（store 落在 `<home>/.buddy-switch/qoder/`），
//! 同进程内一把互斥锁串行化（env 是进程级全局状态）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use serde_json::{json, Value};
use tower::ServiceExt;

use buddy_switch_core::modules::product::scope_of_qoder_region;
use buddy_switch_core::modules::qoder::region::QoderRegion;
use buddy_switch_gateway::qoder::apikey::QoderApiKeyStore;
use buddy_switch_gateway::qoder::{router, QoderGatewayConfig, QoderGatewayState};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` / 全局禁用存储的用例。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 把 `BUDDY_SWITCH_HOME` 指向隔离临时目录，并写入国内版账号库。
///
/// ⚠️ 层级：store 在 `<home>/.buddy-switch/` 下 —— 少写一层会**静默**读到空账号库
/// （症状：每个请求 503「没有可用账号」）。
fn isolated_home(tag: &str, accounts: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qoder-gw-e2e-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let qoder_dir = dir.join(".buddy-switch").join("qoder");
    std::fs::create_dir_all(&qoder_dir).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    let list: Vec<Value> = accounts
        .iter()
        .map(|(uid, token)| {
            json!({
                "uid": uid,
                "nickname": format!("账号 {uid}"),
                "domain": "qoder.com.cn",
                "accessToken": token,
                "refreshToken": format!("drt-{uid}"),
            })
        })
        .collect();
    std::fs::write(
        qoder_dir.join("accounts.cn.json"),
        serde_json::to_string_pretty(&json!({ "accounts": list })).unwrap(),
    )
    .expect("写入账号库");
    dir
}

fn create_key(home: &std::path::Path) -> String {
    let store = QoderApiKeyStore::new(
        home.join(".buddy-switch")
            .join("qoder")
            .join("gateway_keys.json"),
    );
    let (_record, plaintext) = store
        .create("e2e".into(), QoderRegion::Cn)
        .expect("创建测试 Key");
    plaintext
}

// ---------------------------------------------------------------------------
// mock 上游
// ---------------------------------------------------------------------------

/// mock 上游的行为。
#[derive(Clone)]
enum Behavior {
    /// 正常：回显 `cosy-user`（谁在调用）+ usage 帧 + `[DONE]`。
    Normal,
    /// 前 `n` 次 POST 返回 429，之后正常（**换号重试**验证）。
    FailFirstTimes(Arc<AtomicUsize>),
    /// 一直 429（**轮换耗尽**验证）。
    Always429,
    /// HTTP 200 + **非信封错误帧**（issue #46 的真机形态：`event:error` + Jackson
    /// 堆栈 + `{"success":false,"msgCode":500}`）——验证「不再静默成空补全」。
    NonEnvelopeError,
}

/// 真机取证（2026-10-17，Qoder CN）里「编码体 + 漏 `?Encode=1`」时上游回的三行。
///
/// 注意状态是 **200**：上游解析失败也照样先发响应头，错误只出现在帧里 ——
/// 这正是旧实现「HTTP 200 + 空回答 + 0 token」的来源。
const NON_ENVELOPE_ERROR_FRAMES: &str = concat!(
    "event:error\n",
    "data:{\"stackTrace\":[{\"className\":\"com.fasterxml.jackson.core.JsonParser\",\"methodName\":\"_constructError\"}],\"localizedMessage\":\"Unrecognized token 'dwmxdL': was expecting (JSON String, Number, …)\"}\n",
    "{\"success\":false,\"traceId\":\"t-1\",\"msgCode\":500,\"msgInfo\":\"Internal Server Error\"}\n",
);

/// 构造一帧信封 SSE（`body` 为内层 OpenAI chunk 的 JSON 字符串）。
fn envelope(body: &Value) -> String {
    format!(
        "data:{}\n",
        json!({ "body": body.to_string(), "statusCodeValue": 200 })
    )
}

async fn mock_chat(
    State(behavior): State<Behavior>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    let request_count = match &behavior {
        Behavior::FailFirstTimes(counter) => counter.fetch_add(1, Ordering::SeqCst),
        _ => 0,
    };
    // ★ 真机契约（issue #46）：请求体是 `qoder_encode` 过的自定义 base64，上游只按
    //   query `?Encode=1` 决定「先解码再按 JSON 解析」还是「直接按 JSON 解析」。
    //   漏了这个 query ⇒ 上游把 base64 当 JSON 读失败 ⇒ **HTTP 200 + 非信封错误帧**。
    //   mock 照这个行为回放：URL 参数被漏掉时本文件所有 chat 用例直接红，
    //   而不是像修复前那样「mock 一律回正常信封 ⇒ 永远绿」。
    let encoded_query = uri
        .query()
        .map(|query| query.split('&').any(|pair| pair == "Encode=1"))
        .unwrap_or(false);
    if !encoded_query {
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(NON_ENVELOPE_ERROR_FRAMES))
            .expect("构造 mock 响应");
    }
    if matches!(behavior, Behavior::NonEnvelopeError) {
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(NON_ENVELOPE_ERROR_FRAMES))
            .expect("构造 mock 响应");
    }
    let should_fail = match &behavior {
        // `counter` 在上一段已经 `fetch_add` 过了，这里只需**匹配到变体**，
        // 不读它的值 ⇒ 用 `_` 占位。写成 `counter` 会触发
        // `unused_variables` 警告，而 CI 的 `-D warnings`（见 pr-check.yml）
        // 会把它升级成编译错误。
        Behavior::FailFirstTimes(_) => {
            // 第一次调用（返回值为 0）失败；之后放行。
            request_count == 0
        }
        Behavior::Always429 => true,
        Behavior::Normal | Behavior::NonEnvelopeError => false,
    };
    if should_fail {
        return Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("content-type", "text/plain")
            .body(Body::from("rate limited"))
            .expect("构造 mock 响应");
    }
    let user = headers
        .get("cosy-user")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    // 校验 COSY 头确实到了（缺了就返回错误帧 —— 签名头是这条链路的存在意义）。
    let authorized = headers.contains_key("authorization")
        && headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("Bearer COSY."))
            .unwrap_or(false)
        && headers.contains_key("cosy-key")
        && headers.contains_key("cosy-machineid");
    if !authorized {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("missing cosy headers"))
            .expect("构造 mock 响应");
    }
    let frames = format!(
        "{}{}{}{}",
        envelope(
            &json!({"id": "c1", "created": 1, "choices": [{"delta": {"content": format!("来自 {user}")}}]})
        ),
        envelope(&json!({"choices": [{"delta": {"content": " 的回答"}}]})),
        envelope(
            &json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 7, "completion_tokens": 9, "total_tokens": 16}})
        ),
        "data:{\"body\":\"[DONE]\"}\n",
    );
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .body(Body::from(frames))
        .expect("构造 mock 响应")
}

async fn mock_models(headers: HeaderMap) -> Response {
    // 清单接口同样要过 COSY 头（裸 GET 会 403）。
    let ok = headers.contains_key("authorization") && headers.contains_key("cosy-key");
    if !ok {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("missing cosy headers"))
            .expect("构造 mock 响应");
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"chat": [{"key": "ultimate", "display_name": "Ultimate"}, {"key": "qwen3.8-max", "display_name": "Qwen3.8-Max", "is_reasoning": true}]})
                .to_string(),
        ))
        .expect("构造 mock 响应")
}

/// 启动 mock 上游，返回 `http://127.0.0.1:<port>`。
async fn spawn_mock(behavior: Behavior) -> String {
    let app = axum::Router::new()
        .route(
            "/algo/api/v2/service/pro/sse/agent_chat_generation",
            post(mock_chat),
        )
        .route("/algo/api/v2/model/list", get(mock_models))
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

async fn body_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读取响应体");
    String::from_utf8_lossy(&bytes).to_string()
}

/// 在隔离 home 里建状态（指向 mock 上游）并创建一把 Key。
async fn gateway_for(
    tag: &str,
    accounts: &[(&str, &str)],
    behavior: Behavior,
) -> (QoderGatewayState, String) {
    let base = spawn_mock(behavior).await;
    let home = isolated_home(tag, accounts);
    let key = create_key(&home);
    let state = QoderGatewayState::new(QoderGatewayConfig::default()).with_upstream(base);
    (state, key)
}

/// 禁用存储的 RAII 复位（断言失败 panic 时也不把污染留在全局）。
///
/// ⚠️ 复位必须走**独立线程 + 独立 runtime**：Drop 可能发生在 async 上下文的
/// unwind 里，而 `RwLock::blocking_write` 在 async 上下文里会 panic、`block_on`
/// 也不能嵌套在 runtime 内。
struct ResetDisabled(&'static str);
impl Drop for ResetDisabled {
    fn drop(&mut self) {
        let model = self.0.to_string();
        let handle = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("复位的临时 runtime");
            runtime.block_on(async move {
                let store = buddy_switch_gateway::disabled_models::shared();
                let mut guard = store.write().await;
                guard.set(scope_of_qoder_region(QoderRegion::Cn), &model, false);
            });
        });
        let _ = handle.join();
    }
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_paths_need_no_authorization() {
    let _guard = guard();
    // 两个路径都必须免鉴权且 200（canonical `/health` + 兼容别名 `/healthz`）。
    for path in buddy_switch_gateway::HEALTH_PATHS {
        let (state, _key) = gateway_for("health", &[("q1", "dt-1")], Behavior::Normal).await;
        let response = router(state)
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .expect("请求");
        assert_eq!(response.status(), StatusCode::OK, "{path} 必须免鉴权且 200");
    }
}

#[tokio::test]
async fn chat_without_key_is_401() {
    let _guard = guard();
    let (state, _key) = gateway_for("noauth", &[("q1", "dt-1")], Behavior::Normal).await;
    let response = router(state)
        .oneshot(chat_request(None, r#"{"messages": []}"#))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let text = body_text(response).await;
    assert!(text.contains("Bearer"), "{text}");
}

/// 非流式：聚合出 chat.completion；正文回显调用账号 uid（证明 COSY 头与选号都成立）。
#[tokio::test]
async fn chat_non_stream_aggregates_completion() {
    let _guard = guard();
    let (state, key) = gateway_for("agg", &[("q1", "dt-1")], Behavior::Normal).await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": [{"role": "user", "content": "hi"}]}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).expect("应为 JSON");
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["choices"][0]["message"]["content"], "来自 q1 的回答");
    assert_eq!(body["usage"]["total_tokens"], 16);
}

/// 流式：转发帧 + 必然 [DONE]。
#[tokio::test]
async fn chat_stream_relays_and_closes_with_done() {
    let _guard = guard();
    let (state, key) = gateway_for("stream", &[("q1", "dt-1")], Behavior::Normal).await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "stream": true, "messages": [{"role": "user", "content": "hi"}]}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(text.contains("来自 q1"), "{text}");
    assert!(text.contains(" 的回答"), "{text}");
    assert!(text.trim_end().ends_with("data: [DONE]"), "{text}");
}

/// issue #46：上游「HTTP 200 + 非信封错误帧」必须**浮出来**，不能再是「200 + 空补全」。
///
/// 可证伪：去掉 `api::unwrap_non_envelope_error` 的调用 ⇒ 三行全 Skip ⇒ 本用例
/// 收到 200 + 空 content（正是用户看到的现象）。
#[tokio::test]
async fn chat_non_envelope_error_is_not_a_silent_empty_completion() {
    let _guard = guard();
    let (state, key) = gateway_for("nonenv", &[("q1", "dt-1")], Behavior::NonEnvelopeError).await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": []}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(
        response.status(),
        StatusCode::BAD_GATEWAY,
        "有上游错误就不能回 200 空补全"
    );
    let text = body_text(response).await;
    assert!(text.contains("Internal Server Error"), "{text}");
}

/// 同一形态走流式：错误帧要转发给客户端（不是只补一个 [DONE]），且仍然不悬挂。
#[tokio::test]
async fn chat_stream_surfaces_non_envelope_error_frame() {
    let _guard = guard();
    let (state, key) = gateway_for(
        "nonenvstream",
        &[("q1", "dt-1")],
        Behavior::NonEnvelopeError,
    )
    .await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "stream": true, "messages": []}"#,
        ))
        .await
        .expect("请求");
    // 流式响应头在读到上游帧之前就发了 ⇒ 状态仍是 200，错误只能在帧里。
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(text.contains("\"error\""), "错误帧必须转发给客户端: {text}");
    assert!(text.contains("Internal Server Error"), "{text}");
    assert!(text.trim_end().ends_with("data: [DONE]"), "{text}");
}

/// 换号重试：第一个账号 429 ⇒ 自动换到下一个并成功。
#[tokio::test]
async fn chat_rotates_on_429() {
    let _guard = guard();
    let counter = Arc::new(AtomicUsize::new(0));
    let (state, key) = gateway_for(
        "rotate",
        &[("q1", "dt-1"), ("q2", "dt-2")],
        Behavior::FailFirstTimes(counter.clone()),
    )
    .await;
    let response = router(state.clone())
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": []}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(
        text.contains("来自 q1") || text.contains("来自 q2"),
        "{text}"
    );
    assert!(counter.load(Ordering::SeqCst) >= 2, "必须发生过换号重试");
    // 429 的账号被写回**模型级**冷却（池状态可见）。
    let pools = state.pools.lock().await;
    let pool = pools.get(&QoderRegion::Cn).expect("池存在");
    let cooling = pool
        .entries()
        .filter(|entry| !entry.model_cooldowns.is_empty())
        .count();
    assert_eq!(cooling, 1, "恰有一个账号被记入模型级冷却");
}

/// 轮换耗尽：全部 429 ⇒ 503 + 可读原因（不静默、不悬挂）。
#[tokio::test]
async fn chat_exhausted_rotation_is_503_with_reason() {
    let _guard = guard();
    let (state, key) = gateway_for("exhausted", &[("q1", "dt-1")], Behavior::Always429).await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": []}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let text = body_text(response).await;
    assert!(text.contains("429"), "{text}");
}

/// 没有账号 ⇒ 503 且原因**不是**「全部失败」而是「没有可用账号」。
#[tokio::test]
async fn chat_without_accounts_reports_no_accounts() {
    let _guard = guard();
    let (state, key) = gateway_for("noaccount", &[], Behavior::Normal).await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": []}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let text = body_text(response).await;
    assert!(text.contains("没有可用账号"), "{text}");
}

/// 模型清单：过 COSY 头、带禁用过滤。
#[tokio::test]
async fn models_lists_catalog() {
    let _guard = guard();
    let (state, key) = gateway_for("models", &[("q1", "dt-1")], Behavior::Normal).await;
    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).expect("应为 JSON");
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"ultimate"), "{ids:?}");
    assert!(ids.contains(&"qwen3.8-max"), "{ids:?}");
}

/// 被禁用的模型：硬调用 **403**（出口 ② 与出口 ① 同源）。
#[tokio::test]
async fn disabled_model_is_rejected() {
    let _guard = guard();
    let (state, key) = gateway_for("disabled", &[("q1", "dt-1")], Behavior::Normal).await;
    {
        let store = buddy_switch_gateway::disabled_models::shared();
        let mut guard = store.write().await;
        guard.set(scope_of_qoder_region(QoderRegion::Cn), "ultimate", true);
    }
    let _reset = ResetDisabled("ultimate");
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": []}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let text = body_text(response).await;
    assert!(text.contains("被禁用"), "{text}");
}

/// `/v1/responses` 非流式：入站 Responses → 上游 chat，出站聚合回 **Responses 对象**。
///
/// issue #64：Codex CLI（`wire_api = "responses"`）打的就是这个路径；旧实现直接 501
/// ⇒ 客户端连不上。可证伪：把 `Protocol::Responses` 分支去掉 / 让响应回到 501 ⇒ 红。
#[tokio::test]
async fn responses_non_stream_returns_response_object() {
    let _guard = guard();
    let (state, key) = gateway_for("responses", &[("q1", "dt-1")], Behavior::Normal).await;
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::from(
                    r#"{"model": "ultimate", "instructions": "be terse", "input": "hi"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).expect("应为 JSON");
    assert_eq!(body["object"], "response", "{text}");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output"][0]["type"], "message");
    assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(
        body["output"][0]["content"][0]["text"], "来自 q1 的回答",
        "上游正文必须被翻成 output_text：{text}"
    );
    assert_eq!(body["usage"]["input_tokens"], 7);
    assert_eq!(body["usage"]["output_tokens"], 9);
    assert!(
        body["id"].as_str().unwrap_or_default().starts_with("resp_"),
        "id 需带 resp_ 前缀：{text}"
    );
}

/// `/v1/responses` 流式：转成 **Responses 事件流**（`response.created` … `response.completed`），
/// 且**不补 `data: [DONE]`**（Responses 的流终止信号是 `response.completed`）。
#[tokio::test]
async fn responses_stream_emits_response_events() {
    let _guard = guard();
    let (state, key) = gateway_for("responsesstream", &[("q1", "dt-1")], Behavior::Normal).await;
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::from(
                    r#"{"model": "ultimate", "input": "hi", "stream": true}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let text = body_text(response).await;
    for expected in [
        "response.created",
        "response.output_text.delta",
        "response.output_text.done",
        "response.completed",
    ] {
        assert!(text.contains(expected), "缺事件 `{expected}`：{text}");
    }
    assert!(
        !text.contains("[DONE]"),
        "Responses 流不得以 [DONE] 结尾：{text}"
    );
    assert!(text.contains("\"output_tokens\":9"), "末帧要带用量：{text}");
    // 序号从 0 起严格递增（下游按它归并事件）。
    let first = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find_map(|data| serde_json::from_str::<Value>(data).ok())
        .expect("至少一个事件");
    assert_eq!(first["sequence_number"], 0, "{text}");
}
