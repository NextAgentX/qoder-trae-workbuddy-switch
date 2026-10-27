//! 统一网关的**端到端**验收：真的起监听、走真实 TCP、打真实 HTTP 请求。
//!
//! ## 为什么必须是 e2e 而不是「再写几个单测」
//!
//! 本仓踩过的坑：路由在不在册、通配符语法对不对，**编译期完全测不出来**
//! （`{*path}` 写成 0.8 语法 ⇒ router 构建期 panic；handler 写了但没人调 ⇒ 路径 404）。
//! 所以这份测试**只用真实请求**判定行为，不读源码字符串。
//!
//! ## 隔离：**由夹具自己完成**（不需要运行者记得设环境变量）
//!
//! 统一网关**借用**两个产品线的进程内单例（目录缓存 / 禁用集）—— 那些是**只读**的；
//! 但 `WbGateway::shared_state()` 初始化时可能创建 store 目录，而 `relay` 每个请求都会
//! 读真实账号库。⇒ [`isolate_home`] 在**第一个请求之前**把 `BUDDY_SWITCH_HOME` 指向一个
//! **恒空的临时目录**。
//!
//! ⚠️ **不要改回「跑之前请自行设 `BUDDY_SWITCH_HOME`」**：那种「记得设」的前提会让裸跑
//! `cargo test` 变红（本机有账号 ⇒ 请求真的打到上游），而 CI 上又会因为机器恰好没登录
//! 而**假绿**。2026-10-14 实测踩到：`config_changes_apply_to_the_running_listener`
//! 常年红，而真因根本不是它名字说的那件事。
//!
//! ## 覆盖边界（**刻意不测什么**）
//!
//! 覆盖：鉴权 / 命名空间四道把关 / 三个 LLM 端点的挂载与错误形状 /
//! `/v1/responses` + Trae 的**设计内 501** / 业务转发缺命名空间 / 未知路径无 fallback /
//! `/v1/models` 的聚合结构。
//!
//! **不测**：走到真实上游的那一跳。它需要真实账号凭据与真实网络，
//! 且会写冷却 / 账本 / 请求日志 —— 不属于可重复的测试范畴（那部分靠两个既有产品线的
//! e2e（自带隔离状态与假上游）覆盖）。

use std::collections::BTreeSet;

use axum::http::StatusCode;
use buddy_switch_gateway::unified::{
    router, spawn_listener_with_state, UnifiedApiKeyStore, UnifiedGatewayConfig,
    UnifiedGatewayState,
};
use serde_json::Value;

/// 进程级隔离的 `BUDDY_SWITCH_HOME`（**只设一次**，指向一个恒空的临时目录）。
///
/// # 为什么必须有它（2026-10-14 实测：一条用例因它常年红）
///
/// 网关读账号库 / 认证文件走 `core_config::home_dir()`，那是**进程级 env**。夹具原先
/// 只隔离了 `keys.json`，于是用例的结论取决于**跑测试的人本机有没有登录过**：
/// `config_changes_apply_to_the_running_listener` 在本机（有可用 CN 账号）上，请求
/// **真的打到了上游**并拿回「模型 x 不存在」= 400，撞上「不应是 400」的断言。
///
/// ⇒ 隔离必须**自证**，不能靠运行者记得设变量（见文件头的 ⚠️）。
///
/// # 为什么只设一次（而不是每个用例各设一次）
///
/// `cargo test` 默认多线程，用例间反复改同一个进程级变量是竞态源（本仓既有约定：
/// 需要改它的用例必须串行化）。一次性设成恒空目录 ⇒ 值恒定、无竞态，且所有用例
/// 都看不到真实账号。临时目录不回收（`temp_dir` 下的空目录，无副作用）。
///
/// # 为什么必须在**第一个请求之前**调
///
/// `WbGateway::shared_state()` 是**进程单例**，首次被请求触发时初始化；而池的
/// `sync_accounts` **只增不删** ⇒ 一旦有请求在隔离前同步过真实账号，后续用例即使
/// home 空了，也会从池里选到那个账号。故 [`start_gateway_with_state`] 一进来就调。
fn isolate_home() {
    static HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-unified-e2e-home-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("隔离 home 应可创建");
        std::env::set_var("BUDDY_SWITCH_HOME", &dir);
        dir
    });
}

/// 起一个隔离的统一网关（端口 0 = 内核分配），返回 (基址, 明文Key, 临时目录)。
///
/// 密钥库放临时目录；命名空间取默认集（派生层 4 条）。
async fn start_gateway(
    key_namespaces: &[&str],
    biz_namespace: Option<&str>,
    tune: impl FnOnce(&mut UnifiedGatewayConfig),
) -> (
    String,
    String,
    std::path::PathBuf,
    buddy_switch_gateway::GatewayHandle,
) {
    let (base, key, dir, handle, _state) =
        start_gateway_with_state(key_namespaces, biz_namespace, tune).await;
    (base, key, dir, handle)
}

/// 与 [`start_gateway`] 相同，但**额外把监听持有的那份 state 交回来**。
///
/// 统一网关的 `config` 是 `Arc<RwLock<…>>`，`Clone` 廉价且**共享同一个 Arc**
/// ⇒ 外部拿克隆改配置，监听下一跳就读到新值 —— 「配置热改立刻生效」因此**可测**。
///
/// （改造前那条用例以为「无法从测试外部拿到监听持有的 state」，退而断言 `/health`
/// 的命名空间条数，等于**没测**它名字说的那件事。）
async fn start_gateway_with_state(
    key_namespaces: &[&str],
    biz_namespace: Option<&str>,
    tune: impl FnOnce(&mut UnifiedGatewayConfig),
) -> (
    String,
    String,
    std::path::PathBuf,
    buddy_switch_gateway::GatewayHandle,
    UnifiedGatewayState,
) {
    isolate_home();

    let dir = std::env::temp_dir().join(format!(
        "buddy-switch-unified-e2e-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // 端口 0 = 内核分配，避免与并发测试抢端口
    let mut config = UnifiedGatewayConfig {
        port: 0,
        enabled: true,
        ..Default::default()
    };
    tune(&mut config);

    let state =
        UnifiedGatewayState::with_keys(config, UnifiedApiKeyStore::new(dir.join("keys.json")));
    let (record, plaintext) = state
        .keys
        .create(
            "e2e".to_string(),
            key_namespaces.iter().map(|id| id.to_string()).collect(),
            biz_namespace.map(str::to_string),
        )
        .expect("授权集应合法");
    assert!(record.is_active());

    let handle = spawn_listener_with_state(state.clone())
        .await
        .expect("统一网关应能起监听");
    (
        format!("http://{}", handle.addr()),
        plaintext,
        dir,
        handle,
        state,
    )
}

/// 发一条请求并取 (状态码, JSON 体)。
async fn send(
    method: reqwest::Method,
    url: String,
    key: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Value) {
    // ★ 连接层**重试**（安全审计「流程 19」的配套）：这条 e2e 与其它 listener 测试
    //   同时在跑时，偶发出现 `hyper::Error(IncompleteMessage)` —— 服务刚 spawn、
    //   上一轮的随机端口还没完全释放都会这样。
    //
    //   重试只覆盖 `Err`（连不上 / 响应不完整），**不覆盖任何断言**：
    //   真·监听没起来时第 5 次仍然失败并原样报错，真实的回归不会被吞掉。
    let mut last_error = None;
    let mut response = None;
    for attempt in 1..=5u32 {
        let client = reqwest::Client::new();
        let mut retry = client.request(method.clone(), url.clone());
        if let Some(key) = key {
            retry = retry.header("authorization", format!("Bearer {key}"));
        }
        if let Some(body) = body {
            retry = retry
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        match retry.send().await {
            Ok(ok) => {
                response = Some(ok);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(50 * u64::from(attempt))).await;
            }
        }
    }
    let response = response.unwrap_or_else(|| {
        panic!(
            "请求应送达（连不上说明监听没起来）：{:?}",
            last_error.expect("必然有错误")
        )
    });
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
    let json: Value = response.json().await.unwrap_or(Value::Null);
    (status, json)
}

async fn get(base: &str, path: &str, key: Option<&str>) -> (StatusCode, Value) {
    send(reqwest::Method::GET, format!("{base}{path}"), key, None).await
}

async fn post(base: &str, path: &str, key: Option<&str>, body: &str) -> (StatusCode, Value) {
    send(
        reqwest::Method::POST,
        format!("{base}{path}"),
        key,
        Some(body),
    )
    .await
}

/// 所有对外路径在**无凭据**下都必须是 401（不是 404、更不是 200）。
///
/// 这是「路径真的挂上了」的**最强判据**：404 说明没挂、200 说明鉴权漏了。
#[tokio::test]
async fn every_path_is_mounted_and_requires_credentials() {
    let (base, _key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;

    for path in [
        "/status",
        "/v1/models",
        "/v1/chat/completions",
        "/v1/messages",
        "/v1/responses",
        "/biz/anything",
    ] {
        // ★ 必须用**该路径真正支持的方法**探：GET 路由上发 POST 会被 axum 判 405，
        //   那不是「鉴权生效」的证据（405 早于 handler，根本到不了 authenticate）。
        let (status, body) = if path == "/status" || path == "/v1/models" {
            get(&base, path, None).await
        } else {
            post(&base, path, None, "{}").await
        };
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{path} 无凭据应为 401（404 = 没挂上；200 = 鉴权漏了）"
        );
        assert!(
            body["error"]["code"].is_string(),
            "{path} 的错误体应带可诊断的 code：{body}"
        );
    }

    // 未知路径必须 404 —— 证明**没有 fallback**（fallback 会静默吞掉宿主的其它路由）
    let (status, _) = get(&base, "/definitely-not-a-route", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// 两个健康检查路径都免鉴权，且内容能反映配置。
#[tokio::test]
async fn health_paths_are_public_and_reflect_the_config() {
    let (base, _key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;
    let mut bodies = Vec::new();
    for path in buddy_switch_gateway::HEALTH_PATHS {
        let (status, body) = get(&base, path, None).await;
        assert_eq!(status, StatusCode::OK, "{path} 必须免鉴权且 200");
        assert_eq!(body["service"], "unified-gateway");
        assert_eq!(body["running"], true);
        assert_eq!(body["enabled"], true);
        assert_eq!(
            body["namespaces"].as_array().unwrap().len(),
            6,
            "默认启用集应是派生层 6 条（3 平台 × 2 区域；Qoder 于 2026-10-15 加入）"
        );
        bodies.push(body);
    }
    assert_eq!(
        bodies[0], bodies[1],
        "`/health` 与 `/healthz` 必须同源（返回体逐字相同）"
    );
    assert_eq!(bodies[0]["keys"]["total"], 1);

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// `/status` 只列**该 Key 已授权**的命名空间（未授权对它等于不存在）。
#[tokio::test]
async fn status_lists_only_the_authorized_namespaces() {
    let (base, key, dir, handle) = start_gateway(&["wb-cn", "trae-global"], None, |_| {}).await;
    let (status, body) = get(&base, "/status", Some(&key)).await;
    assert_eq!(status, StatusCode::OK);
    let namespaces: Vec<&str> = body["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        namespaces,
        vec!["wb-cn", "trae-global"],
        "应只有被授权的两条，且按命名空间表顺序"
    );
    assert_eq!(body["key"]["name"], "e2e");

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ `/v1/models` 的聚合结构：**逐段**给出 source / fetched_at / note，且非实时必有原因。
///
/// 断言**与数据无关**（不假设用户机器上有几个模型）：只钉结构与契约。
#[tokio::test]
async fn models_aggregates_every_enabled_namespace_with_a_reason_per_segment() {
    let (base, key, dir, handle) = start_gateway(&["wb-cn", "trae-cn"], None, |_| {}).await;
    let (status, body) = get(&base, "/v1/models", Some(&key)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["object"], "list");

    // 只列已授权 ∩ 已启用 ⇒ 恰好这两段（顺序取命名空间表）
    let namespaces = body["namespaces"].as_object().expect("应有逐段来源表");
    // ⚠️ **不断言键序**：JSON 对象的成员顺序在语义上不保证（`serde_json` 默认按字典序存），
    //    断言它等于「表顺序」是在测实现细节。有意义的是**集合相等** + 下面 `data` 的顺序。
    let keys: BTreeSet<&str> = namespaces.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        BTreeSet::from(["wb-cn", "trae-cn"]),
        "应只列已授权 ∩ 已启用的两段"
    );

    for (namespace, meta) in namespaces {
        let source = meta["source"].as_str().unwrap_or_default();
        assert!(
            matches!(source, "live" | "cached" | "builtin"),
            "{namespace} 的来源应是既有词汇表里的值，实际 {source}"
        );
        // ★ 恒非空契约：非实时（或读不到）必须有原因
        if source != "live" {
            assert!(
                meta["note"]
                    .as_str()
                    .map(|note| !note.trim().is_empty())
                    .unwrap_or(false),
                "{namespace} 来源为 {source} 却没有原因（非实时必有原因）"
            );
        }
    }

    // data 里的每条 id 都必须带前缀，且 namespace 字段与 id 前缀一致
    if let Some(data) = body["data"].as_array() {
        for model in data {
            let id = model["id"].as_str().expect("模型应有 id");
            let namespace = model["namespace"].as_str().expect("模型应有 namespace");
            assert!(
                id.starts_with(&format!("{namespace}:")),
                "id `{id}` 的前缀应与 namespace `{namespace}` 一致"
            );
        }
    }

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ 命名空间四道把关：在册 → 非空 → 启用 → 授权（顺序也不许换）。
#[tokio::test]
async fn namespace_guards_reject_in_the_documented_order() {
    // 只授权 `wb-cn` ⇒ 请求 `trae-cn:*` 应 403（而不是 400）
    let (base, key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;

    // 1) 不在册（且不带前缀）⇒ 400 + 列出可用前缀
    let (status, body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"deepseek-v4.1-flash"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "namespace_prefix_required");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("wb-cn:"),
        "应列出可用前缀：{}",
        body["error"]["message"]
    );

    // 2) 前缀后为空 ⇒ 400
    let (status, body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"wb-cn:"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "empty_model_name");

    // 3) 已退役的命名空间 ⇒ **400，先于**授权判定
    //
    // ★ issue #28 删掉了原来这一步（「在册但未启用 ⇒ 400」）：它靠 Trae 的四条扩展层
    //   命名空间（`traework-cn` 等、`default_on: false`）构造，而那些已随程序位退役、
    //   扩展层清空 ⇒ 全表只剩 6 条派生层、全部 `default_on: true`。
    //   **「在册但未启用」这个状态在本仓不再存在**，留着一条恒绿的断言毫无意义。
    //   收敛后的等价风险是「已退役的 id 仍被当成合法命名空间」⇒ 改验它 fail-closed。
    let (status, retired_body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"traework-cn:glm-5.3"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "退役命名空间必须 400");
    assert_ne!(
        retired_body["error"]["code"], "namespace_not_enabled",
        "退役 id 已不在册，不该报「未启用」"
    );
    assert_ne!(
        retired_body["error"]["code"], "namespace_forbidden",
        "退役 id 已不在册，不该走到授权判定（那说明它在册）"
    );

    // 4) 已启用但未授权 ⇒ 403
    let (status, body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"trae-cn:deepseek-v4.1-flash"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "namespace_forbidden");

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ 三个 LLM 端点共用同一份把关 —— 裸名在**任一端点**都被同样地拒绝。
///
/// 可证伪：给某个端点另写一份判断而漏掉某道 ⇒ 本用例在该端点变红。
#[tokio::test]
async fn all_three_llm_endpoints_share_one_guard() {
    let (base, key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;
    for path in ["/v1/chat/completions", "/v1/messages", "/v1/responses"] {
        let (status, body) = post(&base, path, Some(&key), r#"{"model":"glm-5.3"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path} 应拒绝裸模型名");
        assert_eq!(body["error"]["code"], "namespace_prefix_required", "{path}");
    }
    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ `/v1/messages` 的错误体是 **Anthropic 形状**（顶层 `type: "error"`），不是 OpenAI 形状。
#[tokio::test]
async fn messages_uses_the_anthropic_error_shape() {
    let (base, key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;
    let (status, body) = post(
        &base,
        "/v1/messages",
        Some(&key),
        r#"{"model":"glm-5.3","max_tokens":16}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], "error", "Anthropic 形状的顶层判别字段");
    assert_eq!(body["error"]["type"], "invalid_request_error");
    assert_eq!(body["error"]["code"], "namespace_prefix_required");
    assert!(body.get("error").and_then(|e| e.get("message")).is_some());

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ `/v1/responses` + Trae 命名空间 ⇒ **正常分派**（2026-10-22 补齐该端点）。
///
/// 这一条原先钉的是「501 + `endpoint_not_supported_for_namespace`」。补上 Trae 侧的
/// `/v1/responses` 后，同一个请求会走 Trae 网关的同一套中继，失败原因退化为**下游**的
/// （测试环境没有可用账号 ⇒ `no_credential`），而不再可能是「端点不存在」。
///
/// 两条断言分别钉住两件不同的事：
/// 1. 状态码不再是 `NOT_IMPLEMENTED`（端点确实挂上了）；
/// 2. 错误码里不再出现那个已删除的码（分派层没有偷偷保留旧分支）。
///
/// ⚠️ 不要把它放宽成「只要不是 501 就行」然后删掉第 2 条 —— 分派层若对 Trae 重新
/// 加了拦截，这条是唯一会红的用例。
#[tokio::test]
async fn responses_is_served_for_trae_namespaces() {
    let (base, key, dir, handle) = start_gateway(&["trae-cn"], None, |_| {}).await;
    let (status, body) = post(
        &base,
        "/v1/responses",
        Some(&key),
        r#"{"model":"trae-cn:deepseek-v4.1-flash"}"#,
    )
    .await;
    assert_ne!(
        status,
        StatusCode::NOT_IMPLEMENTED,
        "Trae 已支持 /v1/responses：{body}"
    );
    let code = body["error"]["code"].as_str().unwrap_or_default();
    assert_ne!(
        code, "endpoint_not_supported_for_namespace",
        "该错误码已随端点补齐删除：{body}"
    );

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ 业务转发：命名空间来自 **Key 的 `biz_namespace`**，没指定就明确拒绝。
#[tokio::test]
async fn biz_requires_the_key_to_declare_a_namespace() {
    // 未指定 biz_namespace ⇒ 400（不猜、也不隐式取第一个 wb-*）
    let (base, key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;
    let (status, body) = post(&base, "/biz/any/path", Some(&key), "{}").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "biz_namespace_missing");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("WorkBuddy"),
        "应说明只有 WB 合法：{}",
        body["error"]["message"]
    );
    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();

    // 指定了 biz_namespace 且是 WB ⇒ 过掉命名空间三道把关，进入白名单校验
    // （白名单外一律 404 —— 那也是「真的走到了 `biz_for_region`」的证据）
    let (base, key, dir, handle) = start_gateway(&["wb-cn"], Some("wb-cn"), |_| {}).await;
    let (status, _body) = post(&base, "/biz/definitely-not-whitelisted", Some(&key), "{}").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "应已进入既有 `biz_for_region` 的白名单校验（fail-closed 404），而不是卡在命名空间把关"
    );
    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★★ 白名单里的 **GET-only** 路径必须经网关可达。
///
/// ## 这条守的是什么（真实环境实测出来的缺陷）
///
/// 业务白名单是**按前缀**收的，同一个前缀下混着两种方法：
/// `/activity/growth/streak`、`/heatmap` **只有 GET**，而 `/redeem`、`/lottery/draw` 是 POST。
/// 网关原先 `/biz/*path` **只挂 POST** ⇒ 用 POST 打 `streak` 会被上游回
/// `404 page not found`（真实环境实测），白名单声明了却送不到。
///
/// 可证伪：把任一端的路由改回只挂 `post(...)` ⇒ 本用例在「405」上变红。
#[tokio::test]
async fn biz_accepts_get_for_read_only_whitelisted_paths() {
    let (base, key, dir, handle) = start_gateway(&["wb-cn"], Some("wb-cn"), |_| {}).await;

    // ① 白名单外：GET 也必须**到达处理器**（旧实现会在路由层就 405，压根进不到白名单校验）
    let (status, body) = send(
        reqwest::Method::GET,
        format!("{base}/biz/definitely-not-whitelisted"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "GET 也必须进到白名单校验（405 = 路由没接 GET）：{body}"
    );

    // ② 白名单内：GET 必须**过掉白名单**。隔离 store 里没有账号 ⇒ 到不了上游，
    //    但**不该**是 404（白名单拒了）或 405（路由没接 GET）。
    let (status, _body) = send(
        reqwest::Method::GET,
        format!("{base}/biz/activity/growth/streak"),
        Some(&key),
        None,
    )
    .await;
    assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "GET 路由没接上");
    assert_ne!(
        status,
        StatusCode::NOT_FOUND,
        "白名单内的 GET 路径应被放行到中继（GET-only 的 streak 正是这条用例要保的）"
    );

    // ③ 同样要保住 POST 那一半（写操作不能被这次改动弄丢）
    let (status, _body) = send(
        reqwest::Method::POST,
        format!("{base}/biz/v2/billing/meter/daily-checkin"),
        Some(&key),
        Some("{}"),
    )
    .await;
    assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "POST 路由没接上");
    assert_ne!(
        status,
        StatusCode::NOT_FOUND,
        "白名单内的 POST 路径应被放行到中继"
    );

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// ★ 配置改动**立刻生效**：起监听后把某条命名空间停用（热改配置），下一跳就应 400。
///
/// 这条覆盖「管理面保存后独立监听读到新值」这一路径（共享同一份状态）。
///
/// ## 为什么断言 `error.code` 而不只是状态码
///
/// 改造前这里写的是 `assert_ne!(status, 400)`，而 400 有好几种来源：命名空间未启用 /
/// 前缀缺失 / **上游回的 client 错**。本机有可用账号时请求会真的打到上游、拿回
/// 「模型 x 不存在」的 400 ⇒ 用例红得莫名其妙，且从状态码看不出是哪一类。
/// 现在钉住**具体那一类**（`namespace_not_enabled`），语义与用例名一致。
///
/// ## 为什么要有第 ③ 步（改回来）
///
/// 只测「改小 ⇒ 拒绝」的话，把配置在启动期读一次并缓存、或干脆把启用集写死在
/// router 里，都能让 ①② 变绿。第 ③ 步证明读的是**每次请求时的配置**。
#[tokio::test]
async fn config_changes_apply_to_the_running_listener() {
    let (base, key, dir, handle, state) = start_gateway_with_state(&["wb-cn"], None, |_| {}).await;

    // ① 启用中：命名空间把关**放行**，请求停在本地（隔离 home 里没有账号 ⇒ 401）。
    //
    // ★ 断言 `no_credential` 而不是「不是 400」，是**隔离本身的可观测判据**：
    //   若 [`isolate_home`] 被删掉，本机（有可用账号）的请求会真的打到上游、
    //   拿回上游的 `client`/`server` 错误码 ⇒ 这里立刻红。
    let (_, body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"wb-cn:x"}"#,
    )
    .await;
    assert_eq!(
        body["error"]["code"], "no_credential",
        "应停在本地「无可用账号」，说明隔离生效、没打到真实上游：{body}"
    );

    // ② 热改：只留 trae-cn 启用 ⇒ 同一把 Key 打 wb-cn 应**立刻**变成「未启用」
    let mut config = state.config_snapshot().await;
    config.namespaces = vec!["trae-cn".to_string()];
    state.set_config(config).await;

    let (status, body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"wb-cn:x"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未启用必须 400：{body}");
    assert_eq!(
        body["error"]["code"], "namespace_not_enabled",
        "应明确报「未启用」，而不是别的 400：{body}"
    );

    // ③ 再改回来 ⇒ 又放行（证明读的是配置，不是启动期快照）
    let mut config = state.config_snapshot().await;
    config.namespaces = vec!["wb-cn".to_string()];
    state.set_config(config).await;

    let (_, body) = post(
        &base,
        "/v1/chat/completions",
        Some(&key),
        r#"{"model":"wb-cn:x"}"#,
    )
    .await;
    assert_eq!(
        body["error"]["code"], "no_credential",
        "改回来应立刻放行（同样停在本地，不再是被命名空间把关拒绝）：{body}"
    );

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// 端口被占用时**返回可读错误**（而不是 panic）—— 文案要点出「谁占了」。
#[tokio::test]
async fn occupied_port_yields_a_readable_error() {
    let (base, _key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;
    let occupied: u16 = base.rsplit(':').next().unwrap().parse().unwrap();

    let dir2 = std::env::temp_dir().join(format!(
        "buddy-switch-unified-e2e2-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir2).unwrap();
    let config = UnifiedGatewayConfig {
        enabled: true,
        port: occupied, // 故意撞端口
        ..Default::default()
    };
    let state =
        UnifiedGatewayState::with_keys(config, UnifiedApiKeyStore::new(dir2.join("keys.json")));
    let error = match spawn_listener_with_state(state).await {
        Ok(_handle) => panic!("端口已占用时应报错"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("端口"), "错误文案要点出端口问题：{error}");
    assert!(
        error.contains("57891") || error.contains("占用"),
        "文案要给出「谁占了」或候选端口：{error}"
    );

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(dir2).unwrap();
}

/// 声明的路径集合必须与 `router()` 实际挂载一致（防「声明与实现脱节」）。
///
/// 用**真实请求**逐条探：声明过的路径不得是 404。
#[tokio::test]
async fn every_owned_path_is_really_mounted() {
    use buddy_switch_gateway::unified::UnifiedGateway;
    use buddy_switch_gateway::GatewayProduct;

    let (base, _key, dir, handle) = start_gateway(&["wb-cn"], None, |_| {}).await;
    // 每个声明的路径都必须有人接（404 即「声明了却没挂」）
    let mut probed: BTreeSet<&str> = BTreeSet::new();
    for path in UnifiedGateway::OWNED_PATHS {
        let target = path.replace("/*path", "/probe");
        let (status, _) = if buddy_switch_gateway::HEALTH_PATHS.contains(path) {
            get(&base, &target, None).await
        } else {
            post(&base, &target, None, "{}").await
        };
        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "{path} 声明了所有权却拿不到（404）—— handler 写了但没人调"
        );
        probed.insert(path);
    }
    assert_eq!(probed.len(), UnifiedGateway::OWNED_PATHS.len());
    // 8 条 = 健康检查 2 条（canonical + 兼容别名）+ status / models / chat / messages / responses / biz
    assert_eq!(probed.len(), 8, "统一网关应声明 8 条路径");

    handle.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}

/// 用 `router()` 直接构造（不起监听）也能应答 —— 保证宿主的 merge 路径同样可用。
#[tokio::test]
async fn router_can_be_used_without_a_listener() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let dir = std::env::temp_dir().join(format!(
        "buddy-switch-unified-e2e3-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let state = UnifiedGatewayState::with_keys(
        UnifiedGatewayConfig::default(),
        UnifiedApiKeyStore::new(dir.join("keys.json")),
    );
    let response = router(state)
        .oneshot(
            Request::builder()
                .uri(buddy_switch_gateway::HEALTH_CANONICAL_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "router 必须能独立构造并应答（构建期 panic 会让网关完全起不来）"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
