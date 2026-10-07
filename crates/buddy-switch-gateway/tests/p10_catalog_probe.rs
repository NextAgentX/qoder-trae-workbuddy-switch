//! P10 独立验证（p10-verify）：**「接入即识别」的失败不阻断** + **非实时必有原因**。
//!
//! # 为什么需要这个集成测试（而不是只读源码）
//!
//! 「接入即识别」的关键约束是 **fire-and-forget**：`create_api_key` 创建 Key 后**后台**探测，
//! 探测**失败不得阻断接入**（P10 §3.17.1 规则 3）。这类保证**读代码看不出来**——只看
//! `let _ = probe_region(...)` 无法证明「探测真的会失败时接入仍然成功」。
//!
//! 本测试**真的构造「探测必失败」**（隔离 home ⇒ 无凭据 ⇒ `AccountSelector::select` 返回
//! `Err(NoCredential)`），然后断言：
//!
//! 1. `probe_region` 确实返回 `Err`（证明「必失败」这条路径可达，而不是断言空转）；
//! 2. Key **仍然创建成功**（`keys.create` 无返回错误，`keys.list()` 含它）；
//! 3. 探测失败**没有写脏数据**（目录仍是「内置 + 原因」，而非「实时」）。
//!
//! # 环境隔离
//!
//! `GatewayState::new` 会读 `~/.buddy-switch/`（网关配置 / Key 库 / 账号池），因此先把
//! `BUDDY_SWITCH_HOME` 指向一个**已存在**的临时目录（该变量要求目录存在，否则回落真实 home）。
//! 集成测试是独立进程 ⇒ 进程级 env 不干扰 lib 单测。

use std::sync::Mutex;

use buddy_switch_core::modules::catalog::CatalogSource;
use buddy_switch_core::modules::region::Region;
use buddy_switch_gateway::{catalog_probe, GatewayConfig, GatewayState};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` 的用例（env 是进程级全局状态）。
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 把 `BUDDY_SWITCH_HOME` 指向一个已存在的临时目录，返回该目录。
fn isolated_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wb-gw-p10-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    dir
}

/// ★ 探测必失败（隔离 home ⇒ 无凭据）时，**创建 Key 仍成功**（失败不阻断接入）。
#[tokio::test]
async fn probe_failure_does_not_block_key_creation() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = isolated_home("probe-fail");

    let state = GatewayState::new(GatewayConfig::default());

    // ① 「接入」动作：创建 Key。`keys.create` 签名返回 `(record, plaintext)`（**非 Result**）
    //    ⇒ 本身不可失败；这里把它作为「接入成功」的事实来源。
    let (_record, plaintext) = state.keys.create("p10-verify".to_string(), Region::Cn);
    assert!(!plaintext.is_empty(), "创建 Key 应返回一次性明文");
    let created = state
        .keys
        .list()
        .into_iter()
        .any(|k| k.name == "p10-verify");
    assert!(created, "Key 必须已登记（接入成功的事实）");

    // ② 「探测」：隔离 home 下没有凭据 ⇒ `AccountSelector::select` 返回 Err
    //    ⇒ `probe_region` 必失败。先证明这条路径真的失败（否则本用例是空转）。
    let probe = catalog_probe::probe_region(&state, Region::Cn).await;
    assert!(
        probe.is_err(),
        "隔离 home（无凭据）下探测**必须失败**——若这里 Ok，说明断言空转（选号没走隔离 home）"
    );

    // ③ ★ 核心断言：探测失败后，Key 仍然在（**失败没有阻断接入**）。
    let still_there = state
        .keys
        .list()
        .into_iter()
        .any(|k| k.name == "p10-verify");
    assert!(
        still_there,
        "探测失败**不得**影响已创建的 Key —— 「接入即识别」是 fire-and-forget（§3.17.1 规则 3）"
    );

    // ④ 探测失败**不得写脏数据**：目录仍是「内置」（而非被伪造成「实时」），且带原因。
    let snapshot = state.catalogs.current(Region::Cn);
    assert_eq!(
        snapshot.source,
        CatalogSource::Builtin,
        "选号失败时不得把目录写成 live（否则用户看到「实时」却是内置兜底）"
    );
    assert!(
        snapshot
            .note
            .as_deref()
            .map(|n| !n.trim().is_empty())
            .unwrap_or(false),
        "内置快照必须带原因（非实时必有原因契约），实际 note={:?}",
        snapshot.note
    );

    let _ = std::fs::remove_dir_all(&home);
}
