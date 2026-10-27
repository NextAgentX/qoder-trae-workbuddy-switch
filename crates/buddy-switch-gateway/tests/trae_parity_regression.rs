// 测试串行锁（`guard()` / `ENV_LOCK`）会被**故意**持有到用例结束：它防的是多个用例
// 同时改同一个进程级状态（环境变量 / 全局配置）。`#[tokio::test]` 默认 current-thread，
// 不存在「持锁跨线程唤醒」的风险；clippy::await_holding_lock 在这里是误报。
#![allow(clippy::await_holding_lock)]
//! 独立回归护栏：**证伪**「Trae 对齐 WorkBuddy」本轮改造中的高风险点。
//!
//! 本文件由 QA 新增（不改任何产品源码），与 `trae_gateway_e2e.rs` 平行、互不依赖，
//! 用同一套「隔离 home + 本地 mock 上游」手法覆盖**现有测试没钉住**的三条关键链路：
//!
//! 1. **R1 的物化后路由**：`trae_gateway_e2e.rs` 只证明「legacy Key 用 Work 账号能通」，
//!    但没有「先新建一把 CN Key（触发 legacy 物化落盘）之后，旧 Key 仍走 Work 池」这一步。
//!    这正是 R1 最危险的时序：一旦物化把 legacy 归属写错，老用户升级 + 新建 Key 后就全量失败。
//! 2. **两池互不串扰（R8④）**：设计稿明确要求「改一条线的冷却不动另一条」，
//!    但 e2e 只断言了 `status_view` 取对池，没有断言「一条线冷却不污染另一条」。
//! 3. **归属键的双处形态**：落盘（磁盘文件内容）+ 上线（`list_response` 的 JSON）
//!    都必须是 `trae_work`/`trae_cn` 下划线形态，绝不能是派生 serde 的 `traework`。
//!
//! 每条断言都先问过「不修这个 bug 它会红吗」：
//! - 若 legacy 归属被写成 Trae → 测试 1 的第二段（走 Work 池拿 jwt-a）会红；
//! - 若 `sync_for` 取错变体数据 / 两池共用一份 → 测试 2、3 会红；
//! - 若有人把 `variant` 换回 `#[serde(rename_all = "lowercase")]` 派生实现 → 测试 4 会红。

use std::sync::Mutex;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::routing::post;
use tower::ServiceExt;

use buddy_switch_core::modules::trae::variant::TraeVariant;
use buddy_switch_gateway::trae::apikey::{list_response, TraeApiKeyStore};
use buddy_switch_gateway::trae::{status_view, TraeGatewayConfig, TraeGatewayState};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` 的用例（env 是进程级全局状态）。
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 容忍锁中毒：某条用例断言失败并持锁 panic 后，其余用例仍各自报告真实结果。
fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 把 `BUDDY_SWITCH_HOME` 指向隔离目录（层级：`<home>/.buddy-switch/trae/`）。
fn isolated_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "trae-parity-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".buddy-switch").join("trae")).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    dir
}

/// 往隔离 home 写入**指定变体**的账号库（默认变体沿用无后缀旧文件名）。
fn write_accounts_for(home: &std::path::Path, variant: TraeVariant, accounts: &[(&str, &str)]) {
    // ★ 文件名**必须走生产同源的取值点** `paths::accounts_file_for` —— 它按**区域**
    //   命名（Cn = `checkin_accounts.json`、Global = `checkin_accounts.global.json`）。
    //   原实现在这里手搓「默认变体无后缀、其余加 `.<variant>` 后缀」，那是**合并前**
    //   的产品线命名（`.trae_cn`），生产早已不再写入 ⇒ 默认变体一改就会写错文件名、
    //   用例读到空账号库（症状是 503 `no_healthy_account`，与真实缺陷难以区分）。
    let file = home.join(".buddy-switch").join("trae").join(
        buddy_switch_core::modules::trae::paths::accounts_file_for(variant)
            .file_name()
            .expect("账号库文件名应可取")
            .to_string_lossy()
            .to_string(),
    );
    let list: Vec<serde_json::Value> = accounts
        .iter()
        .map(|(uid, jwt)| serde_json::json!({ "name": format!("账号 {uid}"), "UserID": uid, "jwt": jwt }))
        .collect();
    std::fs::write(
        file,
        serde_json::to_string_pretty(&serde_json::json!({ "accounts": list })).unwrap(),
    )
    .expect("写入账号库");
}

/// 键库文件路径。
fn keys_file(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".buddy-switch")
        .join("trae")
        .join("api_gateway_keys.json")
}

/// 在隔离 home 里创建一把归属指定变体的 Key，返回一次性明文。
fn create_key(home: &std::path::Path, variant: TraeVariant) -> String {
    let store = TraeApiKeyStore::new(keys_file(home));
    let (_record, plaintext) = store.create(format!("k-{}", variant.as_str()), variant);
    plaintext
}

fn write_legacy_settings(home: &std::path::Path, key: &str) {
    let file = home
        .join(".buddy-switch")
        .join("trae")
        .join("settings.json");
    std::fs::write(file, serde_json::json!({ "apiKey": key }).to_string())
        .expect("写入 settings.json");
}

/// mock 上游行为。
#[derive(Clone)]
enum Behavior {
    /// 正常 SOLO 事件流，正文回显 `x-ide-token`（便于断言「这次用的是哪个账号」）。
    Normal,
    /// 只对指定 jwt 返回 401，其余正常。
    FailForJwt(String),
}

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
        Behavior::FailForJwt(target) if jwt == target => {
            plain(StatusCode::UNAUTHORIZED, "session dead")
        }
        _ => {
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

/// 组装指向 mock 上游的网关状态（不自动建 Key；由用例自行 create_key）。
async fn state_with_mock(behavior: Behavior) -> TraeGatewayState {
    let base = spawn_mock(behavior).await;
    let mut state = TraeGatewayState::new(TraeGatewayConfig::default());
    state.upstream = base;
    state
}

fn chat_request(key: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {key}"))
        .body(Body::from(body.to_string()))
        .expect("构造请求")
}

async fn body_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读取响应体");
    String::from_utf8_lossy(&bytes).to_string()
}

const NONSTREAM: &str =
    r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"stream":false}"#;

/// R1 时序护栏：**先新建 CN Key（触发 legacy 物化）之后**，旧 `settings.apiKey`
/// 仍必须能鉴权、且路由到 **Work** 池（拿 jwt-a），而不是被物化到别的产品线。
#[tokio::test]
async fn legacy_key_still_routes_to_work_pool_after_materialization() {
    let _g = guard();
    let home = isolated_home("legacy-materialized");
    write_accounts_for(&home, TraeVariant::default(), &[("uid-a", "jwt-a")]);
    let legacy = "sk-trae-22222222222222222222222222222222";
    write_legacy_settings(&home, legacy);

    let state = state_with_mock(Behavior::Normal).await;

    // 物化前：文件不存在，verify 走 settings 回落。
    assert!(!keys_file(&home).exists(), "键库文件此时不应存在");
    assert_eq!(
        state
            .key_store
            .verify(legacy)
            .expect("物化前旧 Key 应可用")
            .variant,
        TraeVariant::default()
    );

    // 新建一把 CN Key → legacy 被一并物化落盘（本轮惰性物化语义）。
    let cn_plaintext = create_key(&home, TraeVariant::Trae);
    assert!(keys_file(&home).exists(), "create 之后键库文件必须已落盘");

    // 磁盘上的 legacy 记录归属必须仍是 trae_work（物化不得改写归属）。
    let on_disk = std::fs::read_to_string(keys_file(&home)).expect("读键库文件");
    assert!(
        on_disk.contains("\"id\": \"legacy\""),
        "legacy 记录必须被物化: {on_disk}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&on_disk).expect("键库是 JSON 数组");
    let legacy_entry = parsed
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "legacy")
        .expect("物化后的 legacy 记录");
    // ★ issue #28：默认变体已改为**合并版** —— 老默认 `trae_work` 指向已被官方
    //   合并卸载的客户端，物化时若还写它，老用户升级后 Key 会绑到一个死客户端上。
    assert_eq!(
        legacy_entry["variant"], "trae_merged",
        "物化后 legacy 的 variant 必须仍是默认变体（写错归属 → 老用户升级后全量失败）: {on_disk}"
    );

    // 物化后：旧 Key 仍能 verify。
    assert_eq!(
        state
            .key_store
            .verify(legacy)
            .expect("物化后旧 Key 仍必须可用")
            .variant,
        TraeVariant::default()
    );
    // 新 CN Key 立即可用。
    assert!(
        state.key_store.verify(&cn_plaintext).is_some(),
        "新 Key 必须立即可用"
    );

    // 关键：用旧 Key 发请求，必须由 **Work** 账号（jwt-a）完成。
    let response = buddy_switch_gateway::trae::router(state.clone())
        .oneshot(chat_request(legacy, NONSTREAM))
        .await
        .expect("路由调用");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "旧 Key 必须照常选中 Work 账号"
    );
    let text = body_text(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("聚合 JSON");
    assert_eq!(
        parsed["choices"][0]["message"]["content"], "jwt-a",
        "旧 Key 必须走 Work 池（jwt-a），而不是被物化到别的产品线\n{text}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// R8②：两条线各自装账号时，**归属哪条线的 Key 就用哪条线的池**。
/// 顺带证明「两池同时存活」——修复前 `TraePool::sync()` 只读 Work，CN Key 必失败。
/// 键的归属决定用哪个池；**区域之间必须互不越界**。
///
/// ⚠️ 契约在 2026-09-21 变了：账号库按**区域**合并后，国内两个产品线标识
/// （`trae_work` / `trae_cn`）读到的是**同一本**国内库 —— 因此
/// 「Work Key 选 Work 池、CN Key 选 CN 池」不再可区分（两个池内容相同）。
/// 现在唯一有意义的隔离是**区域**：国内 Key 绝不能选到国际版账号，反之亦然。
///
/// 可证伪性：若哪天有人把池重新按产品线拆开（或让 Key 忽略区域），
/// 这里「跨区域取号」的两条断言必红。
#[tokio::test]
async fn region_keys_never_cross_into_the_other_region_pool() {
    let _g = guard();
    let home = isolated_home("two-pools");
    // 国内库 1 个账号（`TraeWork` 的区域即国内）、国际库 1 个账号。
    // ★ issue #28：区域才是隔离维度，程序位已收敛为「每区域一个」。
    write_accounts_for(&home, TraeVariant::TraeMerged, &[("uid-a", "jwt-a")]);
    write_accounts_for(
        &home,
        TraeVariant::GlobalTraeMerged,
        &[("uid-global", "jwt-global")],
    );
    let cn_key = create_key(&home, TraeVariant::TraeMerged);
    let global_key = create_key(&home, TraeVariant::GlobalTraeMerged);
    let state = state_with_mock(Behavior::Normal).await;

    let cn_text = body_text(
        buddy_switch_gateway::trae::router(state.clone())
            .oneshot(chat_request(&cn_key, NONSTREAM))
            .await
            .expect("国内 Key 请求"),
    )
    .await;
    let cn_parsed: serde_json::Value = serde_json::from_str(&cn_text).expect("聚合 JSON");
    assert_eq!(
        cn_parsed["choices"][0]["message"]["content"], "jwt-a",
        "国内 Key 必须由**国内**账号完成请求（不得取到国际版账号）\n{cn_text}"
    );

    let global_text = body_text(
        buddy_switch_gateway::trae::router(state.clone())
            .oneshot(chat_request(&global_key, NONSTREAM))
            .await
            .expect("国际版 Key 请求"),
    )
    .await;
    let global_parsed: serde_json::Value = serde_json::from_str(&global_text).expect("聚合 JSON");
    assert_eq!(
        global_parsed["choices"][0]["message"]["content"], "jwt-global",
        "国际版 Key 必须由**国际版**账号完成请求（不得取到国内账号）\n{global_text}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// R8④：**一个区域的冷却不得污染另一个区域**——两套账号体系的账号库/冷却/积分本就分家。
///
/// ⚠️ 契约在 2026-09-21 变了：冷却文件按**区域**分家（`paths::cooldowns_file_for_region`）：
/// 国内（默认区域）用**无后缀** `account_cooldowns.json`，国际版用
/// `account_cooldowns.global.json`。国内的两个产品线标识**共用**国内那一份 ——
/// 「`Trae` 用 `.trae_cn` 后缀」是合并前的命名，现在已不再写入。
///
/// 可证伪性：若实现把冷却写到区域之外的文件、或让两个区域互相污染，(1)/(2)/(3) 必红。
#[tokio::test]
async fn cooling_one_region_does_not_touch_the_other() {
    let _g = guard();
    let home = isolated_home("pool-isolation");
    // 两套账号体系各一个账号。
    write_accounts_for(&home, TraeVariant::TraeMerged, &[("uid-cn", "jwt-cn")]);
    write_accounts_for(
        &home,
        TraeVariant::GlobalTraeMerged,
        &[("uid-global", "jwt-global")],
    );
    // 只让**国内**账号在上游 401。
    let state = state_with_mock(Behavior::FailForJwt("jwt-cn".into())).await;
    let cn_key = create_key(&home, TraeVariant::TraeMerged);

    let response = buddy_switch_gateway::trae::router(state.clone())
        .oneshot(chat_request(&cn_key, NONSTREAM))
        .await
        .expect("国内请求");
    // 国内池只有一个账号且它 401 → 无可换 → 401。
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 冷却按**区域**分家：两个区域各读自己的文件。
    let read_cooldowns = |file_name: &str| -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(home.join(".buddy-switch").join("trae").join(file_name))
                .unwrap_or_else(|_| "{}".into()),
        )
        .unwrap_or_else(|_| serde_json::json!({}))
    };
    let cn_cooldowns = read_cooldowns("account_cooldowns.json");
    let global_cooldowns = read_cooldowns("account_cooldowns.global.json");

    // (1) 方向不变：国内账号必须被冷却，且落在 **国内** 冷却文件里。
    //
    // ★ issue #108：类型从 `SessionDead` 改为 `AuthRejected` —— 本用例的 mock 返回
    // 的是**裸 401（无 12153）**，按新契约它只是 60 秒浅冷却。本用例要证的是
    // 「冷却落在正确区域的文件里」，与冷却时长/类型无关，故只跟着改口径。
    assert_eq!(
        cn_cooldowns["cooldowns"]["uid-cn"]["type"], "AuthRejected",
        "国内账号必须被冷却并写进国内冷却文件\n{cn_cooldowns}"
    );
    // (2) 国内文件里不得冒出国际版账号。
    assert!(
        cn_cooldowns["cooldowns"].get("uid-global").is_none(),
        "国际版账号不得出现在国内冷却文件\n{cn_cooldowns}"
    );
    // (3) **另一半**：国际版冷却文件里**不得**出现国内账号。
    //     旧版读的是「另一条产品线」的文件却断言「有 uid-cn」，等于把
    //     「冷却错写另一个文件」这一缺陷写死进断言（bug 在时反而变绿）。
    assert!(
        global_cooldowns["cooldowns"].get("uid-cn").is_none(),
        "国内账号的冷却不得污染国际版冷却文件\n{global_cooldowns}"
    );

    // 池摘要：国际版池不受国内冷却影响；国内池该账号已禁用。
    let global = status_view(&state, false, None, "test", TraeVariant::Global).await;
    assert_eq!(global["pool"]["total"], 1);
    assert_eq!(
        global["pool"]["available"], 1,
        "国际版池不得被国内的冷却影响: {global}"
    );
    let cn = status_view(&state, false, None, "test", TraeVariant::TraeWork).await;
    // ★ issue #108：mock 返回的是裸 401（无 `12153`）⇒ 按新契约只落**浅冷却**，
    // 不再把账号打成禁用终态。断言跟着改：`cooling == 1` 且 `disabled == 0`。
    // 本用例真正要证的是「区域之间不互相污染」，`disabled` 的取值只是顺带的口径。
    assert_eq!(
        cn["pool"]["cooling"], 1,
        "国内池该账号应处于冷却中（裸 401 = 浅冷却）: {cn}"
    );
    assert_eq!(
        cn["pool"]["disabled"], 0,
        "裸 401 不得把账号打成禁用终态（否则重登也救不回来）: {cn}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// R4：旧 `remaining_credits.json`（**没有** `packages` 字段）反序列化必须回落空 map、
/// 不 panic，且不得吞掉既有 `credits` / `expire_times`。
///
/// 钉住的缺陷：若有人给 `RemainingCreditsFile` 加上 `deny_unknown_fields`、或漏掉
/// `#[serde(default)]`，老用户升级首启就会整份读失败 → 积分页全空（R4 回归）。
#[test]
fn legacy_remaining_file_without_packages_deserializes_empty() {
    use buddy_switch_core::modules::trae::credits::{load_remaining_for, RemainingCreditsFile};

    let _g = guard();
    let home = isolated_home("legacy-remaining");
    // 旧结构：只有 credits / expire_times / updated_at，没有 packages。
    let legacy = serde_json::json!({
        "credits": { "uid-a": 42.5 },
        "expire_times": { "uid-a": 1_234_567_890i64 },
        "updated_at": "2026-01-01T00:00:00+08:00"
    });
    let file = home
        .join(".buddy-switch")
        .join("trae")
        .join("remaining_credits.json");
    std::fs::write(&file, legacy.to_string()).expect("写旧 remaining 文件");

    // 走真实读盘路径：缺失字段回落空 map，而不是 panic / 整份丢弃。
    let parsed = load_remaining_for(TraeVariant::TraeWork);
    assert!(
        parsed.packages.is_empty(),
        "旧文件缺 packages 必须回落空 map: {parsed:?}"
    );
    assert_eq!(
        parsed.credits.get("uid-a").copied(),
        Some(42.5),
        "既有 credits 不得被 legacy 兼容吞掉"
    );
    assert_eq!(
        parsed.expire_times.get("uid-a").copied(),
        Some(1_234_567_890)
    );

    // 直接反序列化也必须成功（等价于「不得设 deny_unknown_fields」）。
    let direct: RemainingCreditsFile =
        serde_json::from_str(&legacy.to_string()).expect("旧结构必须可反序列化");
    assert!(direct.packages.is_empty());

    let _ = std::fs::remove_dir_all(&home);
}

/// 归属键的**双处形态**：落盘文件与上线响应（`list_response`）都必须是下划线 id。
/// 若有人把 `variant` 换回派生 serde（`#[serde(rename_all = "lowercase")]` → `traework`），
/// 前端「归属产品线」列会读不到值，这条会红。
#[tokio::test]
async fn variant_ids_are_underscore_form_on_disk_and_in_api_response() {
    let _g = guard();
    let home = isolated_home("variant-ids");
    // ★ issue #28：可选变体只剩两个合并版（`trae_merged` / `global_trae_merged`）。
    let cn_key = create_key(&home, TraeVariant::TraeMerged);
    let work_key = create_key(&home, TraeVariant::GlobalTraeMerged);

    let on_disk = std::fs::read_to_string(keys_file(&home)).expect("读键库文件");
    assert!(
        on_disk.contains("\"trae_merged\""),
        "落盘必须是 trae_merged: {on_disk}"
    );
    assert!(
        on_disk.contains("\"global_trae_merged\""),
        "落盘必须是 global_trae_merged: {on_disk}"
    );
    assert!(
        !on_disk.contains("\"traework\"") && !on_disk.contains("\"traecn\""),
        "不得出现派生 serde 的小写形态: {on_disk}"
    );

    // 上线形态：list_response 的每条记录都带下划线形态的 variant，且不含明文/hash。
    let store = TraeApiKeyStore::new(keys_file(&home));
    let response = list_response(&store);
    let keys = response["keys"].as_array().expect("keys 是数组");
    assert_eq!(keys.len(), 2);
    let variants: std::collections::BTreeSet<&str> = keys
        .iter()
        .map(|entry| entry["variant"].as_str().expect("variant 必须是字符串"))
        .collect();
    let expected: std::collections::BTreeSet<&str> =
        ["trae_merged", "global_trae_merged"].into_iter().collect();
    assert_eq!(variants, expected, "上线 variant 集合漂移: {response}");
    // 脱敏红线：响应里不得出现明文 / hash。
    let serialized = response.to_string();
    assert!(
        !serialized.contains(&work_key) && !serialized.contains(&cn_key),
        "不得泄漏明文"
    );
    assert!(
        keys.iter().all(|entry| entry.get("hash").is_none()),
        "不得下发 hash"
    );

    let _ = std::fs::remove_dir_all(&home);
}
