//! Trae 侧**端到端**：冷却记录必须能自证「判的是哪一份凭据」（issue #108 §P0-A）。
//!
//! # 为什么必须有这一条
//!
//! 报障现场自相矛盾：卡片红字「需重新登录」+ 一行 11 小时前的 401 原文，而**同一张卡片**
//! 上 JWT 剩余 336 小时、积分也刷得出来。用户重新登录、重启应用都无效 —— 因为那条
//! `SessionDead`（`until = 9_999_999_999`）**没有任何失效路径**：六个「凭据变新」的出口
//! 只写账号库，从不碰冷却表。
//!
//! 修法不是「在那六个出口各清一次冷却」（WB 侧已论证「靠观测账号库变化去推断事件」
//! 必然误判），而是让冷却记录**自带判据**：写入时记下当时的 JWT `exp`，读时与当前
//! `exp` 比对，不一致即自动失效。
//!
//! ## 为什么单测挡不住
//!
//! `matches_credential` 的单测只证明「这个函数算得对」，证明不了**接线**：
//! ① `apply_error` 真的把 `exp` 写进去了；② `sync_for` 真的按指纹过滤了；
//! ③ 视图（`account_view`）与池（`sync_for`）用的是**同一个**判据 —— 第二点尤其关键，
//! 判据分家会出现最坏的形态：界面说「可用」，网关永远不选它（且没有任何报错）。
//!
//! 本文件把「冷却文件 → 池 `disabled`」与「冷却文件 → 视图 `cooldownType`」两条链
//! 各跑一遍，用的是**生产写入函数**（`save_cooldown_with_credential_for`），
//! 因此写入侧与读取侧的字段名漂移也会被抓住。
//!
//! # 环境隔离
//!
//! 与 `trae_pool_revive.rs` 同套路：`BUDDY_SWITCH_HOME` 指向**已存在**的临时目录
//! （该变量要求目录存在，否则静默回落真实 home ⇒ 断言会读到真机数据）。

use std::collections::HashMap;

use buddy_switch_core::modules::trae::account::{self, AccountsFile, RawAccount};
use buddy_switch_core::modules::trae::credits;
use buddy_switch_core::modules::trae::variant::TraeVariant;
use buddy_switch_gateway::trae::pool::TraePool;

/// base64url（无填充）—— 只为构造一个**真的能被 `jwt::parse` 解出 `exp`** 的令牌。
///
/// 不引依赖：本文件只需要这一个方向，且输入永远是 ASCII 的 JSON。
fn b64url(input: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[n as usize & 63] as char);
        }
    }
    out
}

/// 构造一个 `exp` 可指定的 Trae JWT（签名段是假的 —— 本地解析**不校验签名**）。
fn jwt_with_exp(uid: &str, exp: i64) -> String {
    let payload = format!("{{\"data\":{{\"id\":\"{uid}\"}},\"exp\":{exp}}}");
    format!("header.{}.sig", b64url(&payload))
}

fn isolated_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "buddy-switch-trae-fingerprint-{tag}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).expect("临时 home 应可创建");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    dir
}

/// ★★ 凭据换新 ⇒ 池**与**视图都必须放行；`exp` 未变 ⇒ 两者都必须继续拦。
///
/// 变异验证（任一条都会让本用例变红）：
/// - 去掉 `sync_for` 里的 `filter(|entry| entry.matches_credential(..))` ⇒ 第 ①②段红；
/// - 去掉 `account_view` 里的指纹判断 ⇒ 第 ①段红（视图仍显示 SessionDead）；
/// - 把指纹改成 access token 原文（本例用「同 exp 的另一个令牌」模拟）⇒ 第 ③段红。
#[test]
fn session_dead_cooldown_only_applies_to_the_credential_it_judged() {
    let home = isolated_home("dead");
    let variant = TraeVariant::default();
    let uid = "trae-user-fingerprint";
    let old_exp = 1_700_000_000_i64;
    let new_exp = 1_800_000_000_i64;

    // 铺账号：当前凭据是**新**令牌（exp = new_exp）。
    let file = AccountsFile {
        accounts: vec![RawAccount {
            name: "测试账号".to_string(),
            user_id: Some(uid.to_string()),
            jwt: jwt_with_exp(uid, new_exp),
            refresh_token: None,
            added_at: None,
            updated_at: None,
            remark: None,
        }],
        device_bindings: HashMap::new(),
    };
    account::save_accounts_for_region(variant.region(), &file).expect("账号库应可写");

    // 用**生产写入函数**写一条「旧凭据的」永久失效判定。
    credits::save_cooldown_with_credential_for(
        variant,
        uid,
        "SessionDead",
        -1,
        "账号「A」上游失败（HTTP 401）：{\"code\":1001}",
        Some(old_exp),
    );

    // ① 池：凭据已换新 ⇒ **不得**派生为禁用（否则界面可用、网关永远不选它）。
    let mut pool = TraePool::for_variant(variant);
    assert_eq!(pool.sync_for(variant), 1, "账号库应有 1 条（jwt 非空）");
    assert!(
        !pool.entries()[0].disabled,
        "凭据换新后不得再 `disabled` —— 这正是「重新登录也修不好」的那个终态"
    );
    assert!(
        pool.entries()[0].healthy(buddy_switch_gateway::trae::now_secs()),
        "凭据换新后必须真的可路由"
    );
    assert_eq!(
        pool.summary(buddy_switch_gateway::trae::now_secs())
            .available,
        1,
        "池卡计数格必须显示 1 个可用（而不是 1 个已禁用）"
    );

    // ①′ 视图：同一条链的另一个出口，必须给出一致的结论。
    let views = account::list_account_views_for(variant);
    let view = views
        .iter()
        .find(|view| view["userId"] == serde_json::json!(uid))
        .expect("应有该账号的视图")
        .clone();
    assert!(
        view["cooldownType"].is_null(),
        "凭据换新后视图不得再显示 SessionDead（报障原文就是这句「需重新登录」）：{view}"
    );
    assert!(
        view["cooldownUntil"].is_null() && view["cooldownSince"].is_null(),
        "失效的判定三个字段必须一起消失，不能留半截：{view}"
    );

    // ② 反向对照：把指纹改成**当前** exp（等价于「判定就是针对这份凭据的」）⇒ 必须拦。
    credits::save_cooldown_with_credential_for(
        variant,
        uid,
        "SessionDead",
        -1,
        "12153",
        Some(new_exp),
    );
    let mut pool = TraePool::for_variant(variant);
    pool.sync_for(variant);
    assert!(
        pool.entries()[0].disabled,
        "指纹与当前凭据一致时，永久判定必须继续生效（否则「会话作废」变成可自愈）"
    );
    let views = account::list_account_views_for(variant);
    let view = views
        .iter()
        .find(|view| view["userId"] == serde_json::json!(uid))
        .expect("应有该账号的视图")
        .clone();
    assert_eq!(
        view["cooldownType"],
        serde_json::json!("SessionDead"),
        "视图与池必须同源：池拦着，视图就不能说正常"
    );
    assert!(
        view["cooldownSince"].is_number(),
        "判定时刻必须下发（前端要显示「11 小时前」而不是让人以为刚失败）：{view}"
    );

    // ③ 旧数据（没有指纹）⇒ 保守照旧生效，不误放行。
    credits::clear_cooldown_for(variant, uid).expect("清冷却应成功");
    credits::save_cooldown_for(variant, uid, "SessionDead", -1, "12153 旧记录");
    let mut pool = TraePool::for_variant(variant);
    pool.sync_for(variant);
    assert!(
        pool.entries()[0].disabled,
        "无指纹的旧记录必须照旧生效（拿不到证据时不做放行）"
    );

    let _ = std::fs::remove_dir_all(&home);
}
