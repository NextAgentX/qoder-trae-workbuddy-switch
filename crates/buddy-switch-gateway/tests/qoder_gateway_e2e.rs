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
use buddy_switch_core::modules::qoder::cosy;
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
    /// 首次返回指定 HTTP 错误，之后正常；回放限流与排队的不同信封。
    RejectOnce(StatusCode, &'static str, Arc<AtomicUsize>),
    /// HTTP 200 + **非信封错误帧**（issue #46 的真机形态：`event:error` + Jackson
    /// 堆栈 + `{"success":false,"msgCode":500}`）——验证「不再静默成空补全」。
    NonEnvelopeError,
    /// 第一次 POST 返回 **400（参数错）**，之后正常 —— 验证**自动降级**
    /// （issue #72：结构化历史被拒 ⇒ 同账号用文本模式重发一次）。
    /// 同时把每次的**解码后**请求体记下来，供断言「第二发确实是文本模式」。
    RejectFirst(Arc<Mutex<Vec<String>>>),
    /// 每次都返回 **400** —— 验证「降级结论**不**在第一次 400 就落记忆」
    /// （文本模式也 400 ⇒ 400 与结构化无关，记了就是误判）。
    Always400,
    /// 正常响应，但把每次的**解码后**请求体记下来（验证出站清洗用）。
    Record(Arc<Mutex<Vec<String>>>),
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
    body: axum::body::Bytes,
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
    if let Behavior::RejectOnce(status, detail, counter) = &behavior {
        if counter.fetch_add(1, Ordering::SeqCst) == 0 {
            return Response::builder()
                .status(*status)
                .body(Body::from(*detail))
                .expect("构造 mock 响应");
        }
    }
    /* ★ 自动降级（issue #72）：第一次回 400（参数错），之后正常；并把解码后的
    请求体记下来 —— 断言「第二发确实是文本模式」需要看到**请求体**，不是响应。 */
    if let Behavior::RejectFirst(seen) = &behavior {
        let plain = cosy::qoder_decode(&String::from_utf8_lossy(&body)).unwrap_or_default();
        let text = String::from_utf8_lossy(&plain).to_string();
        let mut recorded = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        recorded.push(text);
        let first = recorded.len() == 1;
        drop(recorded);
        if first {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "text/plain")
                .body(Body::from("tool_calls are not supported"))
                .expect("构造 mock 响应");
        }
    }
    if matches!(behavior, Behavior::Always400) {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "text/plain")
            .body(Body::from("always rejected"))
            .expect("构造 mock 响应");
    }
    if let Behavior::Record(seen) = &behavior {
        let plain = cosy::qoder_decode(&String::from_utf8_lossy(&body)).unwrap_or_default();
        seen.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(String::from_utf8_lossy(&plain).to_string());
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
        // 已在上面单独处理（第一次回 400，之后放行 / 一直 400 / 只记录）。
        Behavior::RejectFirst(_)
        | Behavior::Always400
        | Behavior::Record(_)
        | Behavior::RejectOnce(..) => false,
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

/// issue #72：**自动降级**。结构化历史被上游按参数错（400）拒 ⇒ 网关用**同一账号**、
/// 文本模式重发一次并成功 —— 用户拿到的是正常回答，而不是一轮 400。
///
/// 可证伪：把 `routes.rs` 里的 `degrade::should_degrade` 分支删掉 ⇒ 本用例收到 400。
#[tokio::test]
async fn structured_tool_history_degrades_to_text_after_a_400() {
    let _guard = guard();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (state, key) = gateway_for(
        "degrade",
        &[("q1", "dt-1")],
        Behavior::RejectFirst(seen.clone()),
    )
    .await;
    // 白名单模型（`qwen` 前缀）+ 齐备的 tool_call id ⇒ 第一发必然走结构化。
    let request = json!({
        "model": "qwen3.8-max",
        "messages": [
            {"role": "user", "content": "看下目录"},
            {"role": "assistant", "content": "", "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "read", "arguments": "{}"},
            }]},
            {"role": "tool", "tool_call_id": "c1", "name": "read", "content": "hi"},
        ],
    })
    .to_string();
    let response = router(state)
        .oneshot(chat_request(Some(&key), &request))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK, "降级后应拿到正常回答");
    let text = body_text(response).await;
    assert!(text.contains("来自 q1 的回答"), "{text}");

    let bodies = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
    assert_eq!(bodies.len(), 2, "应发两次（结构化 + 文本模式重发）");
    assert!(
        bodies[0].contains("\"tool_calls\""),
        "第一发应是结构化：{}",
        bodies[0]
    );
    assert!(
        bodies[1].contains("assistant 请求调用工具"),
        "第二发应退成文本模式：{}",
        bodies[1]
    );
    assert!(
        !bodies[1].contains("\"tool_calls\""),
        "第二发不得再带结构化字段：{}",
        bodies[1]
    );
    // 结论坐实后才落记忆：这个 (区域, 模型) 之后直接走文本模式。
    assert!(
        buddy_switch_gateway::qoder::degrade::is_unsupported(QoderRegion::Cn, "qwen3.8-max"),
        "降级成功后必须记住该模型不支持结构化"
    );
}

/// 降级结论**不**在第一次 400 就落记忆：文本模式也 400 ⇒ 400 与结构化无关
/// （上游整体坏了）。此时记住「结构化不被接受」是**误判** —— 会让该模型长时间
/// 退回文本模式，正是 issue #72 要修的乱码。
#[tokio::test]
async fn degrade_memo_is_not_written_when_the_text_retry_also_fails() {
    let _guard = guard();
    let (state, key) = gateway_for("degrade-fail", &[("q1", "dt-1")], Behavior::Always400).await;
    let request = json!({
        "model": "qwen-always-400",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "", "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "read", "arguments": "{}"},
            }]},
            {"role": "tool", "tool_call_id": "c1", "name": "read", "content": "hi"},
        ],
    })
    .to_string();
    let response = router(state)
        .oneshot(chat_request(Some(&key), &request))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        !buddy_switch_gateway::qoder::degrade::is_unsupported(QoderRegion::Cn, "qwen-always-400"),
        "重发也失败 ⇒ 400 与结构化无关，不该落降级记忆"
    );
}

/// 出站清洗（与 WB / Trae 侧同款）：客户端历史里的**孤儿**（assistant 声明了调用、
/// 没有 tool 消息回答；或 tool 消息没有对应的调用）必须在**发出去之前**清干净 ——
/// 上游对这种请求判协议错误（400 `tool_call_sequence_broken`）。
///
/// 可证伪：把 core 里的 `cleanup_orphan_tool_history` 调用去掉 ⇒ 本用例的
/// 「c2 / c9 不该出现」两条断言红。
#[tokio::test]
async fn orphan_tool_history_is_cleaned_before_reaching_upstream() {
    let _guard = guard();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (state, key) =
        gateway_for("clean", &[("q1", "dt-1")], Behavior::Record(seen.clone())).await;
    let request = json!({
        "model": "qwen3.8-max",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "", "tool_calls": [{
                "id": "c1", "type": "function",
                "function": {"name": "read", "arguments": "{}"},
            }]},
            {"role": "tool", "tool_call_id": "c1", "name": "read", "content": "结果"},
            {"role": "assistant", "content": "", "tool_calls": [{
                "id": "c2", "type": "function",
                "function": {"name": "read", "arguments": "{}"},
            }]},
            {"role": "tool", "tool_call_id": "c9", "name": "read", "content": "悬空"},
        ],
    })
    .to_string();
    let response = router(state)
        .oneshot(chat_request(Some(&key), &request))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::OK);
    let bodies = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
    assert_eq!(bodies.len(), 1, "应只发一次（没有降级）");
    let body = &bodies[0];
    assert!(body.contains("\"c1\""), "配对完整的调用必须保留：{body}");
    assert!(!body.contains("\"c2\""), "没有回答的调用必须清掉：{body}");
    // ★ 必须带引号精确匹配：裸 `c9` 会撞上随机 `chat_record_id` 里的
    // "c9" 子串（本用例因此偶发红，属测试自身的缺陷）。
    assert!(!body.contains("\"c9\""), "悬空结果必须清掉：{body}");
}

/// 429 是终态：首个账号被限流后必须直接失败，不得由第二个账号接力。
#[tokio::test]
async fn chat_does_not_rotate_on_429() {
    let _guard = guard();
    let counter = Arc::new(AtomicUsize::new(0));
    let (state, key) = gateway_for(
        "rate-limit",
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
    assert_eq!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "429 必须原样返回，不得换号重试"
    );
    let text = body_text(response).await;
    assert!(text.contains("429"), "{text}");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "只允许首个账号发一次请求"
    );
    // 429 的账号被写回**模型级**冷却（池状态可见）；另一个账号不得被调用。
    let pools = state.pools.lock().await;
    let pool = pools.get(&QoderRegion::Cn).expect("池存在");
    let cooling = pool
        .entries()
        .filter(|entry| !entry.model_cooldowns.is_empty())
        .count();
    assert_eq!(cooling, 1, "恰有一个账号被记入模型级冷却");
}

/// 单一账号持续 429 ⇒ 仍直接返回 429（不是被包装成 503）。
#[tokio::test]
async fn chat_rate_limit_is_429_with_reason() {
    let _guard = guard();
    let (state, key) = gateway_for("exhausted", &[("q1", "dt-1")], Behavior::Always429).await;
    let response = router(state)
        .oneshot(chat_request(
            Some(&key),
            r#"{"model": "ultimate", "messages": []}"#,
        ))
        .await
        .expect("请求");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let text = body_text(response).await;
    assert!(text.contains("429"), "{text}");
}

/// #204：同模型的第二次请求被本地冷却挡住，仍应是可重试的 429。
#[tokio::test]
async fn model_cooldown_followup_is_429_with_retry_after() {
    let _guard = guard();
    // 嵌套 RESOURCE_LIMIT 的排队标记确实可被前 300 字符截掉；HTTP 429 本来
    // 就不查标记，截断前后都归 RateLimited，不能把它误报成 Queued 冷却。
    const NESTED_QUEUE: &str = concat!(
        "{\"code\":\"RESOURCE_LIMIT\",\"message\":\"",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "\",\"detail\":{\"code\":10605,\"isQueued\":true}}"
    );
    let clipped: String = NESTED_QUEUE.chars().take(300).collect();
    use buddy_switch_gateway::qoder::pool::{classify_http_with_detail, QoderErrKind};
    assert!(!clipped.contains("isQueued") && !clipped.contains("10605"));
    assert_eq!(
        classify_http_with_detail(429, NESTED_QUEUE),
        QoderErrKind::RateLimited
    );
    assert_eq!(
        classify_http_with_detail(429, &clipped),
        QoderErrKind::RateLimited
    );
    // 401/403 则受截断影响，但会误判成 AuthDead，不是 RateLimited。
    assert_eq!(
        classify_http_with_detail(403, NESTED_QUEUE),
        QoderErrKind::Queued
    );
    assert_eq!(
        classify_http_with_detail(403, &clipped),
        QoderErrKind::AuthDead
    );
    for detail in [
        r#"{"code":"RESOURCE_LIMIT"}"#,
        r#"{"isQueued":true}"#,
        r#"{"code":10605}"#,
        NESTED_QUEUE,
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (state, key) = gateway_for(
            "cooldown-followup",
            &[("q1", "dt-1")],
            Behavior::RejectOnce(StatusCode::TOO_MANY_REQUESTS, detail, calls.clone()),
        )
        .await;
        let app = router(state.clone());
        let request = r#"{"model":"ultimate","messages":[]}"#;
        let initial = app
            .clone()
            .oneshot(chat_request(Some(&key), request))
            .await
            .unwrap();
        assert_eq!(initial.status(), StatusCode::TOO_MANY_REQUESTS);
        let initial_retry: u64 = initial
            .headers()
            .get("retry-after")
            .expect("首个模型限流响应也必须给出 Retry-After")
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=60).contains(&initial_retry));
        let _ = body_text(initial).await;

        let followup = app
            .clone()
            .oneshot(chat_request(Some(&key), request))
            .await
            .unwrap();
        assert_eq!(followup.status(), StatusCode::TOO_MANY_REQUESTS, "{detail}");
        let retry_after: u64 = followup.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=60).contains(&retry_after), "{retry_after}");
        let text = body_text(followup).await;
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], "rate_limit_error", "{text}");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("HTTP 429") && message.contains("ultimate"),
            "{text}"
        );
        assert!(
            message.contains("冷却") && !message.contains("导入"),
            "{text}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "冷却中不得再次请求上游");

        // 相同账号的另一模型仍可调用，不能把模型冷却升级成账号故障。
        let other = app
            .oneshot(chat_request(
                Some(&key),
                r#"{"model":"qwen3.8-max","messages":[]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(other.status(), StatusCode::OK);
        let _ = body_text(other).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

/// 所有协议（包括 stream 请求）共享冷却出口，等待时间取最早解冻并向上取整。
#[tokio::test]
async fn model_cooldown_retry_after_uses_earliest_account_for_all_protocols() {
    let _guard = guard();
    for (path, request, anthropic) in [
        (
            "/v1/chat/completions",
            json!({"model":"ultimate","messages":[],"stream":true}),
            false,
        ),
        (
            "/v1/messages",
            json!({"model":"ultimate","messages":[],"max_tokens":16,"stream":true}),
            true,
        ),
        (
            "/v1/responses",
            json!({"model":"ultimate","input":"hi","stream":true}),
            false,
        ),
    ] {
        let (state, key) = gateway_for(
            "cooldown-protocol",
            &[("q1", "dt-1"), ("q2", "dt-2")],
            Behavior::Normal,
        )
        .await;
        let now = buddy_switch_core::modules::config::now_ms();
        {
            let mut pools = state.pools.lock().await;
            let pool = pools.entry(QoderRegion::Cn).or_default();
            pool.sync_for(QoderRegion::Cn);
            for (uid, delay) in [("q1", 60_500), ("q2", 120_500)] {
                pool.adopt_probe_result(
                    uid,
                    "ultimate",
                    &buddy_switch_gateway::pool::ProbeOutcome::ModelRateLimited {
                        reset_at_ms: Some(now + delay),
                        upstream_reset_at_ms: None,
                    },
                    now,
                );
            }
            assert_eq!(
                pool.model_cooldown_retry_after("ultimate", &Default::default(), None, now),
                Some(61)
            );
            assert_eq!(
                pool.model_cooldown_retry_after(
                    "ultimate",
                    &Default::default(),
                    None,
                    now + 60_500
                ),
                None,
                "至少一个账号已解冻，就不能再归因为全冷却"
            );
        }
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", format!("Bearer {key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "{path}");
        let retry: i64 = response.headers()["retry-after"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        // 60.5s 的余量避免将测试绑定到单个墙钟毫秒；仍能抓到错误地取最晚解冻。
        assert!((60..=61).contains(&retry), "{path}: {retry}");
        let text = body_text(response).await;
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            body["error"][if anthropic { "type" } else { "code" }],
            "rate_limit_error",
            "{text}"
        );
        assert_eq!(
            state.log.list().last().unwrap()["status"],
            429,
            "日志应与响应一致"
        );
    }
}

/// 只根据本区域、Key 名单和硬排除后的账号判冷却；不能被无关账号影响。
#[tokio::test]
async fn model_cooldown_diagnosis_respects_account_scope_and_other_failures() {
    use buddy_switch_gateway::qoder::pool::QoderErrKind;
    let _guard = guard();
    for (case, selected, status) in [
        ("selected-cold", "q1", StatusCode::TOO_MANY_REQUESTS),
        ("selected-healthy", "q2", StatusCode::OK),
        (
            "selected-missing",
            "missing",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        ("selected-pat", "pat", StatusCode::SERVICE_UNAVAILABLE),
        ("auth-dead", "q1", StatusCode::SERVICE_UNAVAILABLE),
        ("excluded", "q1", StatusCode::SERVICE_UNAVAILABLE),
        ("paused", "q1", StatusCode::SERVICE_UNAVAILABLE),
        ("mixed-upstream", "", StatusCode::BAD_GATEWAY),
        ("mixed-transport", "", StatusCode::SERVICE_UNAVAILABLE),
        ("mixed-quota", "", StatusCode::TOO_MANY_REQUESTS),
        ("other-region", "q1", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let (state, key) = gateway_for(
            case,
            &[("q1", "dt-1"), ("q2", "dt-2"), ("pat", "")],
            if case == "mixed-upstream" {
                Behavior::RejectOnce(
                    StatusCode::BAD_GATEWAY,
                    "upstream unavailable",
                    Arc::new(AtomicUsize::new(0)),
                )
            } else {
                Behavior::Normal
            },
        )
        .await;
        let state = if case == "mixed-transport" {
            // 非法 URL 使真实 reqwest 立即报传输错误，无需等待外部网络超时。
            state.with_upstream("http://[".into())
        } else {
            state
        };
        if !selected.is_empty() {
            let record = state.key_store.verify(&key).unwrap();
            state
                .key_store
                .update_policy(
                    &record.id,
                    buddy_switch_gateway::apikey::KeyPolicy {
                        scope_mode: buddy_switch_gateway::apikey::ScopeMode::Selected,
                        scope_accounts: vec![selected.into()],
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        {
            let mut pools = state.pools.lock().await;
            let pool = pools.entry(QoderRegion::Cn).or_default();
            pool.sync_for(QoderRegion::Cn);
            pool.apply_error(
                "q1",
                QoderErrKind::RateLimited,
                "RESOURCE_LIMIT",
                "ultimate",
            );
            if case == "auth-dead" {
                pool.apply_error("q1", QoderErrKind::AuthDead, "token expired", "ultimate");
            }
        }
        if case == "excluded" {
            buddy_switch_gateway::pool::exclusions::set_at(
                &buddy_switch_gateway::pool::exclusions::file_path(),
                "qoder:cn",
                "q1",
                true,
                "test",
                1,
            )
            .unwrap();
        }
        // 真实认证失败同时写入池的 AuthDead 和持久化 Paused；复现完整状态。
        if matches!(case, "paused" | "auth-dead") {
            use buddy_switch_core::modules::risk_control::{RiskOutcome, RiskScope};
            state
                .risk_store
                .as_ref()
                .unwrap()
                .record_outcome_at(
                    &RiskScope::new("qoder", "cn").unwrap(),
                    "q1",
                    RiskOutcome::Banned("test".into()),
                    None,
                    1,
                    60,
                )
                .unwrap();
        }
        if case == "mixed-quota" {
            buddy_switch_gateway::qoder::quota::park_exhausted(QoderRegion::Cn, "q2", "test");
        }
        if case == "other-region" {
            // 国际区 Key 不得借用国内区的账号或冷却诊断。
            let (_, global_key) = state
                .key_store
                .create("global".into(), QoderRegion::Global)
                .unwrap();
            let response = router(state)
                .oneshot(chat_request(
                    Some(&global_key),
                    r#"{"model":"ultimate","messages":[]}"#,
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                status,
                "{case}: {}",
                body_text(response).await
            );
            continue;
        }
        let response = router(state)
            .oneshot(chat_request(
                Some(&key),
                r#"{"model":"ultimate","messages":[]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{case}");
        if case != "selected-cold" {
            assert!(!response.headers().contains_key("retry-after"), "{case}");
        }
        let text = body_text(response).await;
        if case == "mixed-quota" {
            buddy_switch_gateway::qoder::quota::release_park(QoderRegion::Cn, "q2");
        }
        if status == StatusCode::SERVICE_UNAVAILABLE {
            assert!(text.contains("no_available_account"), "{case}: {text}");
        } else if case == "mixed-quota" {
            assert!(text.contains("quota_exceeded"), "{text}");
        }
    }
}

/// 401/403 排队只留痕，不写模型冷却；后续请求必须仍能调用上游。
#[tokio::test]
async fn queued_followup_remains_callable_without_cooldown() {
    let _guard = guard();
    for (status, detail, initial_status) in [
        (StatusCode::UNAUTHORIZED, r#"{"code":10605,"isQueued":true}"#, StatusCode::UNAUTHORIZED),
        (StatusCode::FORBIDDEN, r#"{"code":10605,"isQueued":true}"#, StatusCode::FORBIDDEN),
        (StatusCode::OK, "data:{\"body\":\"{\\\"code\\\":10605,\\\"isQueued\\\":true}\",\"statusCodeValue\":403}\n", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (state, key) = gateway_for("queued-followup", &[("q1", "dt-1")],
            Behavior::RejectOnce(status, detail, calls.clone())).await;
        let app = router(state.clone());
        let request = r#"{"model":"ultimate","messages":[]}"#;
        let initial = app.clone().oneshot(chat_request(Some(&key), request)).await.unwrap();
        assert_eq!(initial.status(), initial_status);
        let _ = body_text(initial).await;
        let pools = state.pools.lock().await;
        let entry = pools[&QoderRegion::Cn].entries().next().unwrap();
        assert!(!entry.auth_dead && entry.model_cooldowns.is_empty());
        drop(pools);
        let followup = app.oneshot(chat_request(Some(&key), request)).await.unwrap();
        assert_eq!(followup.status(), StatusCode::OK);
        let _ = body_text(followup).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

/// #204：队列标记在第 300 字符之后，也不能把健康账号标死并持久化暂停。
#[tokio::test]
async fn long_queued_403_is_classified_before_detail_truncation() {
    let _guard = guard();
    const DETAIL: &str = concat!(
        "{\"code\":\"RESOURCE_LIMIT\",\"message\":\"",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "\",\"detail\":{\"code\":10605,\"isQueued\":true}}"
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let (state, key) = gateway_for(
        "long-queued-403",
        &[("q1", "dt-1")],
        Behavior::RejectOnce(StatusCode::FORBIDDEN, DETAIL, calls.clone()),
    )
    .await;
    let app = router(state.clone());
    let request = r#"{"model":"ultimate","messages":[]}"#;
    let initial = app
        .clone()
        .oneshot(chat_request(Some(&key), request))
        .await
        .unwrap();
    assert_eq!(
        initial.status(),
        StatusCode::FORBIDDEN,
        "保留直接 HTTP 状态语义"
    );
    assert!(!initial.headers().contains_key("retry-after"));
    let text = body_text(initial).await;
    let body: Value = serde_json::from_str(&text).unwrap();
    let shown = body["error"]["message"]
        .as_str()
        .unwrap()
        .strip_prefix("上游拒绝（HTTP 403）：")
        .unwrap();
    assert_eq!(shown.chars().count(), 300, "展示详情仍限 300 字符");
    assert!(!shown.contains("isQueued") && !shown.contains("10605"));
    {
        let pools = state.pools.lock().await;
        let entry = pools[&QoderRegion::Cn].entries().next().unwrap();
        assert!(!entry.auth_dead, "迟到的排队标记不能被截断成 AuthDead");
        assert_eq!(entry.cooldown_until, 0);
        assert!(entry.model_cooldowns.is_empty());
        assert!(entry.last_error.as_deref().unwrap().starts_with("queued: "));
    }
    assert!(
        !buddy_switch_gateway::risk_controls::account_blocked(
            &state.risk_store,
            "qoder",
            "cn",
            "q1",
            buddy_switch_core::modules::config::now_ms().max(0) as u64,
        ),
        "排队不得持久化暂停账号"
    );
    let logs = state.log.list();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["status"], 403);
    assert_eq!(
        logs[0]["error"].as_str().unwrap(),
        shown,
        "日志只保留截断详情"
    );

    let followup = app
        .oneshot(chat_request(Some(&key), request))
        .await
        .unwrap();
    assert_eq!(
        followup.status(),
        StatusCode::OK,
        "后续请求不得变成 no_available_account"
    );
    let _ = body_text(followup).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
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

/// 业务会话聚合（默认开）：同一会话的两轮请求复用同一个 `business`
/// `id / begin_at / name`；不同会话使用独立的 `business` 锚点。
#[tokio::test]
async fn business_anchor_is_reused_across_turns_of_the_same_conversation() {
    let _guard = guard();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (state, key) = gateway_for(
        "business",
        &[("q1", "dt-1")],
        Behavior::Record(seen.clone()),
    )
    .await;

    // 同一会话的第二轮：历史前缀不变，只追加 assistant + 新 user。
    let first = json!({
        "model": "qwen3.8-max",
        "messages": [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "round one"},
        ],
    })
    .to_string();
    let second = json!({
        "model": "qwen3.8-max",
        "messages": [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "round one"},
            {"role": "assistant", "content": "answer"},
            {"role": "user", "content": "round two"},
        ],
    })
    .to_string();
    let other = json!({
        "model": "qwen3.8-max",
        "messages": [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "another conversation"},
        ],
    })
    .to_string();

    for body in [&first, &second, &other] {
        let response = router(state.clone())
            .oneshot(chat_request(Some(&key), body))
            .await
            .expect("请求");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = body_text(response).await;
    }

    let bodies = seen
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(bodies.len(), 3, "三次请求都应到达上游");
    let business = |index: usize| -> Value {
        serde_json::from_str::<Value>(&bodies[index])
            .expect("上游请求体是 JSON")
            .get("business")
            .cloned()
            .expect("请求体必须带 business")
    };
    let first_turn = business(0);
    let second_turn = business(1);
    let other_conversation = business(2);

    assert_eq!(
        first_turn["id"], second_turn["id"],
        "同一会话必须复用 business.id：{first_turn} vs {second_turn}"
    );
    assert_eq!(
        first_turn["begin_at"], second_turn["begin_at"],
        "begin_at 必须复用首次锚点"
    );
    assert_eq!(
        first_turn["name"], second_turn["name"],
        "name 必须复用首次锚点"
    );
    assert_ne!(
        first_turn["id"], other_conversation["id"],
        "不同会话不得共用 business.id"
    );
}
