//! WorkBuddy 网关 P11 模型禁用：**路由级**端到端（补 T04 的残余缺口）。
//!
//! # 为什么补这一条
//!
//! 修复者加了 **Trae 侧**的路由级 e2e（`trae_gateway_e2e.rs::p11_disabled_model_*`），
//! 但 **WB 侧**只有源码结构扫描 —— 证明不了「WB 的 `/v1/models` 过滤真的接到**本 region**
//! 的禁用集合」「WB 的硬调用真的回 403 `model_disabled`」。若过滤接到错的 region / 错的
//! 集合，源码断言仍会绿。本文件用**真实路由 + 真实鉴权 + 真实禁用存储**把 WB 侧补上，
//! 与 Trae 侧对称。
//!
//! # 环境隔离
//!
//! 与 `body_limit.rs` / `trae_gateway_e2e.rs` 同套路：`BUDDY_SWITCH_HOME` 指向已存在的
//! 临时目录，同进程内用一把互斥锁串行化（env 是进程级全局状态）。
//!
//! WB 的 `/v1/models` 在隔离 home 下读不到实时目录 ⇒ 回落**内置兜底清单**
//! （`FALLBACK_CN_MODELS` / `FALLBACK_GLOBAL_MODELS`），足够做「少一条」的断言。

use std::sync::Mutex;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use buddy_switch_core::modules::catalog::{FALLBACK_CN_MODELS, FALLBACK_GLOBAL_MODELS};
use buddy_switch_core::modules::region::Region;
use buddy_switch_gateway::{router, GatewayConfig, GatewayState};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` 的用例。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn isolated_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wb-gw-p11-e2e-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    dir
}

fn models_request(key: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/v1/models")
        .header("authorization", format!("Bearer {key}"))
        .body(Body::empty())
        .expect("构造请求")
}

fn chat_request(key: &str, model: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {key}"))
        .body(Body::from(
            serde_json::json!({ "model": model, "messages": [] }).to_string(),
        ))
        .expect("构造请求")
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读取响应体");
    String::from_utf8_lossy(&bytes).to_string()
}

async fn models_ids(app: &axum::Router, key: &str) -> Vec<String> {
    let response = app
        .clone()
        .oneshot(models_request(key))
        .await
        .expect("请求应完成");
    assert_eq!(response.status(), StatusCode::OK, "/v1/models 应 200");
    let value: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("响应应为 JSON");
    value["data"]
        .as_array()
        .expect("data 应为数组")
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect()
}

/// ★★ WB **路由级**端到端：出口① 真的少一条、出口② 真的回 403 `model_disabled`。
#[tokio::test]
async fn wb_disabled_model_is_hidden_from_models_and_rejected_on_call() {
    let _guard = guard();
    let home = isolated_home("disable");
    let state = GatewayState::new(GatewayConfig::default());
    let (_record, key) = state.keys.create("e2e-wb".into(), Region::Cn);
    let app = router(state.clone());

    // 目标模型必须在**内置兜底清单**里，否则「少一条」无从谈起。
    let target = "glm-5.3";
    assert!(
        FALLBACK_CN_MODELS.iter().any(|m| m.id == target),
        "断言前提：目标模型必须在内置兜底清单里"
    );

    // 基线：未禁用 ⇒ 出现在 /v1/models。
    let before = models_ids(&app, &key).await;
    assert!(
        before.iter().any(|id| id == target),
        "未禁用时应出现在 /v1/models：{before:?}"
    );

    // 禁用该模型（写进**进程内唯一**的禁用存储 —— 与两个出口读的是同一份）。
    {
        let mut lock = state.disabled.write().await;
        lock.set(Region::Cn, target, true);
    }

    // 出口①：真的少一条。
    let after = models_ids(&app, &key).await;
    assert!(
        !after.iter().any(|id| id == target),
        "禁用后不应出现在 /v1/models：{after:?}"
    );
    assert_eq!(after.len() + 1, before.len(), "禁用后应正好少一条");

    // 出口②：硬调用真的回 403 `model_disabled`（在触达上游之前就拒）。
    let response = app
        .clone()
        .oneshot(chat_request(&key, target))
        .await
        .expect("请求应完成");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "被禁模型的调用应 403（而不是放行 / 笼统 4xx）"
    );
    let text = body_text(response).await;
    assert!(text.contains("model_disabled"), "错误类别应为 `model_disabled`：{text}");
    assert!(text.contains("被禁用"), "文案必须说明「被禁用」：{text}");

    // ★ 大小写变体也必须被拒（T04 #2 的绕过链在 WB 路由级也堵上）。
    let response = app
        .clone()
        .oneshot(chat_request(&key, &target.to_uppercase()))
        .await
        .expect("请求应完成");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "大写变体的调用也必须 403（否则禁用被绕过）"
    );

    // 清理：禁用存储是**进程级全局**（同进程其余用例会读到它）。
    {
        let mut lock = state.disabled.write().await;
        lock.set(Region::Cn, target, false);
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// ★ 路由级 region 隔离：在 **Global** 禁用某模型，**CN** 的 `/v1/models` 不受影响。
///
/// 堵住「过滤接到全局集合而非本 region 集合」的接线缺陷 —— 源码扫描抓不到。
#[tokio::test]
async fn wb_disabled_in_one_region_does_not_hide_the_other() {
    let _guard = guard();
    let home = isolated_home("region-iso");
    let state = GatewayState::new(GatewayConfig::default());
    let (_cn_rec, cn_key) = state.keys.create("e2e-wb-cn".into(), Region::Cn);
    let (_gl_rec, global_key) = state.keys.create("e2e-wb-global".into(), Region::Global);
    let app = router(state.clone());

    // CN 与 Global 的内置清单都含 `glm-5.3`（前提）。
    let target = "glm-5.3";
    assert!(FALLBACK_CN_MODELS.iter().any(|m| m.id == target));
    assert!(FALLBACK_GLOBAL_MODELS.iter().any(|m| m.id == target));

    // 只在 Global 禁用。
    {
        let mut lock = state.disabled.write().await;
        lock.set(Region::Global, target, true);
    }

    // Global 少一条。
    let global_ids = models_ids(&app, &global_key).await;
    assert!(
        !global_ids.iter().any(|id| id == target),
        "Global 禁用后 Global 清单应少一条：{global_ids:?}"
    );
    // CN **不受影响**（region 隔离）。
    let cn_ids = models_ids(&app, &cn_key).await;
    assert!(
        cn_ids.iter().any(|id| id == target),
        "Global 的禁用不得让 CN 的模型消失（region 隔离）：{cn_ids:?}"
    );

    // 清理。
    {
        let mut lock = state.disabled.write().await;
        lock.set(Region::Global, target, false);
    }
    let _ = std::fs::remove_dir_all(&home);
}
