//! WorkBuddy **路由级**端到端：模型闸门暂停必须回 **503**，绝不能回 401。
//!
//! # 为什么必须有这一条（issue #106）
//!
//! 报障现场：客户端报「本轮运行失败 API 密钥无效」，而网关日志里那一行是
//! **19 ms、账号列为空** —— 一个字节都没发出去。根因是闸门短路只 `break` 不设
//! `last`，于是落到 `pool_unavailable_error` 的兜底，而那条兜底只有两个出口
//! （`pool_busy` 503 / `no_credential` 401），`pool_busy` 的判据（可用号全在途满）
//! 在空池 / 全冷却时**不成立** ⇒ 一律 401。
//!
//! 客户端的固定契约是：401/403 ⇒ `AUTH` ⇒ **不可重试** ⇒ 整轮会话终止。
//! 于是「这个模型此刻被限流」被翻译成「你的凭据坏了」，用户去重新登录，永远修不好。
//!
//! ## 为什么单测挡不住
//!
//! `GatewayError::ModelGatePaused.status() == 503` 的断言只能证明「这个类型映射对了」，
//! 证明不了「中继真的走到它」—— 改造前的缺陷恰恰是**没走到**（落了兜底）。
//! 只有把真实路由跑起来、真的一次闸门短路，才能钉住这条接线。
//!
//! # 环境隔离
//!
//! 与 `wb_gateway_p11_e2e.rs` 同套路：`BUDDY_SWITCH_HOME` 指向已存在的临时目录，
//! 同进程内用一把互斥锁串行化（env 是进程级全局状态）。

use std::sync::Mutex;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use buddy_switch_core::modules::region::Region;
use buddy_switch_gateway::{router, GatewayConfig, GatewayState};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` 的用例。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn isolated_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wb-gw-gate-e2e-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    dir
}

fn chat_request(key: &str, model: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {key}"))
        .body(Body::from(
            serde_json::json!({
                "model": model,
                "stream": false,
                "messages": [{ "role": "user", "content": "hi" }],
            })
            .to_string(),
        ))
        .expect("构造请求")
}

/// ★★ 「模型闸门暂停」的对外状态码必须是 503，且带 `Retry-After`；**不是 401**。
///
/// 变异验证：
/// - 把 `relay.rs` 里闸门分支的 `last = Some(...)` 去掉（回到改造前只 `break`）⇒ 本用例红
///   （状态码变 401、`type` 变 `no_credential`）；
/// - 把 `ModelGatePaused` 的 `status()` 改回 401 ⇒ 本用例红。
#[tokio::test]
async fn paused_model_gate_returns_503_with_retry_after_instead_of_401() {
    let _guard = guard();
    let home = isolated_home("pause");
    let state = GatewayState::new(GatewayConfig::default());
    let (_record, key) = state.keys.create("e2e-gate".into(), Region::Cn);
    let app = router(state.clone());

    let model = "glm-5.3";
    let now_ms = buddy_switch_gateway::timeutil::now_ms();

    // 把闸门真的压下去：默认 `gate_threshold = 2` ⇒ 连记两次模型级失败即暂停。
    {
        let mut pool = state.pool.write().await;
        assert!(!pool.note_model_failure(model, now_ms), "第 1 次不该开闸");
        assert!(pool.note_model_failure(model, now_ms), "第 2 次必须开闸");
    }

    // 前提自检：此刻闸门确实挡着（否则下面的 503 可能来自别的原因，断言就假了）。
    {
        let mut pool = state.pool.write().await;
        let block = pool
            .admit_model(model, now_ms, "req-probe")
            .expect("暂停中必须被挡住（前提自检）");
        assert!(block.remain_ms > 0, "剩余时长必须为正");
        assert_eq!(
            block.failures, 2,
            "`failures` 是**窗口内失败次数**（不是账号数）—— 空池连记两次就是 2"
        );
    }

    let response = app
        .clone()
        .oneshot(chat_request(&key, model))
        .await
        .expect("请求应完成");
    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("读取响应体");
    let text = String::from_utf8_lossy(&bytes).to_string();

    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "闸门暂停绝不能是 401 —— 客户端会判 AUTH、不可重试、整轮会话终止：{text}"
    );
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "闸门暂停应是 503（临时不可用，可重试）：{text}"
    );
    assert!(
        text.contains("model_gate_paused"),
        "错误类别必须是 `model_gate_paused`（不是 `no_credential`）：{text}"
    );
    assert!(text.contains(model), "文案必须点名被暂停的模型：{text}");
    assert!(
        text.contains("与你的凭据无关"),
        "必须明确否定「凭据坏了」这个误判（用户正是被这句带偏的）：{text}"
    );
    assert!(
        retry_after.is_some(),
        "必须带 `Retry-After`（闸门的 until_ms 是本进程算出的确定值，不违反「不臆造数字」）：{text}"
    );
    // 诊断信息不能丢：闸门短路那条 attempt 是本链路唯一能自查的线索。
    assert!(
        text.contains("attempts"),
        "错误体必须带 attempts（改造前在 no_credential 出口被整表丢掉）：{text}"
    );

    // 反向对照：**别的模型不受影响**（闸门按模型分家，否则一个模型被罚就全站停摆）。
    {
        let mut pool = state.pool.write().await;
        assert!(
            pool.admit_model("deepseek-v4-flash", now_ms, "req-probe")
                .is_none(),
            "另一个模型不得被同一道闸门挡住"
        );
    }

    let _ = std::fs::remove_dir_all(&home);
}

/// ★★ 安全审计 P1-3：半开探针票必须**有归还路径**。
///
/// ## 缺陷形态
///
/// 领票的请求可能**根本没得到模型级结论**（这里就是「池里一个号都没有」）：
/// 它不会调 `note_model_success` / `note_model_failure`，于是票一直挂到
/// `PROBE_LEASE_MS`（120 s）租约到期 —— 期间**所有**请求都被判成「名额被占用」⇒
/// 整模型 503，而报错还说是「别的请求」占了名额。
///
/// ## 这条用例钉什么
///
/// 请求返回后，**下一个请求必须能立刻领到票**（而不是等 120 秒）。
/// 判据用 `admit_model` 的返回值：能领到票 ⇒ `None`；票被遗弃 ⇒ `Some(block)`。
///
/// 变异验证：把 `relay()` 外层那句 `release_probe` 删掉 ⇒ 本条红。
#[tokio::test]
async fn abandoned_probe_is_returned_when_the_request_has_no_model_verdict() {
    let _guard = guard();
    let home = isolated_home("probe-release");
    // 把退避压到 1ms，好让「暂停到期 → 半开」在测试里立刻发生。
    let state = GatewayState::new(GatewayConfig {
        pool: buddy_switch_gateway::pool::PoolConfig {
            gate_threshold: 2,
            gate_base_ms: 1,
            gate_max_ms: 2,
            ..Default::default()
        },
        ..Default::default()
    });
    let (_record, key) = state.keys.create("e2e-probe".into(), Region::Cn);
    let app = router(state.clone());

    let model = "glm-5.3";
    let now_ms = buddy_switch_gateway::timeutil::now_ms();
    {
        let mut pool = state.pool.write().await;
        pool.note_model_failure(model, now_ms);
        pool.note_model_failure(model, now_ms);
    }
    // 等闸门到期（1ms 退避）。
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // 池里没有任何账号 ⇒ 这个请求领票后必然「选不出号」，拿不到模型级结论。
    let response = app
        .clone()
        .oneshot(chat_request(&key, model))
        .await
        .expect("请求应完成");
    assert!(
        !response.status().is_success(),
        "空池不可能成功，这条用例的前提是「请求带着票失败了」"
    );

    // 核心断言：票已归还 ⇒ 下一个请求能立刻领到票。
    let later = buddy_switch_gateway::timeutil::now_ms();
    let mut pool = state.pool.write().await;
    assert!(
        pool.admit_model(model, later, "req-next").is_none(),
        "上一个请求没有模型级结论 ⇒ 票必须归还，否则整模型要被白锁 120 秒"
    );

    drop(pool);
    let _ = std::fs::remove_dir_all(&home);
}
