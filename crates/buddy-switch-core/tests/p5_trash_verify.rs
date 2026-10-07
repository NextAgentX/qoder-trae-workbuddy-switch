//! P5 §2.7「软删除 + 回收站」的**独立验证**（独立集成测试，只走公开 API）。
//!
//! ## 为什么另起一个测试二进制（而不是复用 `account.rs` 里的单测）
//!
//! `account.rs` 的内联单测与被测函数**同文件**：它们能看见私有项，也共享同一个
//! `#[cfg(test)]` 编译单元。独立验证的价值在于**换一个观察点**——本文件只依赖
//! `buddy_switch_core` 的 **public** 接口（`pub fn` / `pub struct`），因此：
//!
//! - 若某个行为其实依赖私有实现细节（而非公开契约），本文件会**编译失败 / 断言失败**；
//! - 与内联单测相互印证：两者同时绿，才说明「契约层」与「实现层」都成立。
//!
//! ## 隔离方式
//!
//! 与 `switch_region_binding.rs` 同款：全部 fixture 写入临时目录，`BUDDY_SWITCH_HOME`
//! 指向它；用一把进程内 `Mutex` 串行化所有会改环境变量的用例，结束时恢复并清理。
//!
//! ## 覆盖（对应派单的 5 条核心 + 3 条边界）
//!
//! 1. 软删除 → 恢复**往返无损**（逐字段深比较，含凭据 / 备注 / 任意扩展字段）；
//! 2. `purge` 是**唯一不可逆步**（`sweep` 只清**过期**的，不清新鲜的）；
//! 3. 留存策略：**30 天** + 上限 **200 条**（先到先出）；
//! 4. 按 **uid** 删除不再空转（`id ≠ uid`，软删 / 恢复 / purge 三条路径都验）；
//! 5. 边界：**trash 文件损坏**（非法 JSON）⇒ 可理解行为（不崩溃 / 不清空 live）；
//! 6. 边界：**未知 scope**（`trae-glboal`）⇒ 必须 `Err`（fail-closed）。

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use buddy_switch_core::modules::account::{
    self, clear_trash_for, load_trash_for, purge_account_for, restore_account_for,
    sweep_trash_entries, trash_account_for, TrashEntry, TRASH_MAX_ENTRIES, TRASH_RETENTION_MS,
};
use buddy_switch_core::modules::config::BUDDY_SWITCH_HOME_ENV;
use buddy_switch_core::modules::product::{
    require_wb_region, resolve_scope, resolve_scope_param, scope_of_wb_region, Scope,
};
use buddy_switch_core::modules::region::{accounts_file_for, accounts_trash_file_for, Region};

/// 串行化所有会修改 `BUDDY_SWITCH_HOME` 的用例（该变量是进程级全局状态）。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "buddy-switch-p5-trash-{label}-{}-{nanos}",
        std::process::id()
    ))
}

/// 隔离 home 守卫：设置 `BUDDY_SWITCH_HOME`、持有 [`ENV_LOCK`]，Drop 时恢复环境变量
/// 并删除临时目录（字段按声明顺序析构 ⇒ 恢复环境变量期间仍持锁）。
struct IsolatedHome {
    _lock: MutexGuard<'static, ()>,
    previous: Option<String>,
    dir: PathBuf,
}

impl IsolatedHome {
    fn new(label: &str) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var(BUDDY_SWITCH_HOME_ENV).ok();
        let dir = unique_temp_dir(label);
        fs::create_dir_all(&dir).expect("create isolated home");
        std::env::set_var(BUDDY_SWITCH_HOME_ENV, &dir);
        Self {
            _lock: lock,
            previous,
            dir,
        }
    }

}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(BUDDY_SWITCH_HOME_ENV, value),
            None => std::env::remove_var(BUDDY_SWITCH_HOME_ENV),
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(1_000_000)
}

/// 一份**字段齐全**的账号记录（凭据 / 备注 / 扩展字段都有），用于证明「原样保留」。
fn rich_account(id: &str, uid: &str) -> Value {
    json!({
        "id": id,
        "uid": uid,
        "nickname": "测试用户",
        "email": "user@example.com",
        "remark": "备注-甲",
        "access_token": "AT-SECRET",
        "refresh_token": "RT-SECRET",
        "token_type": "Bearer",
        "domain": "www.codebuddy.cn",
        "created_at": 1_700_000_000_000i64,
        // 任意扩展字段：软删除必须**原样保留**（TrashEntry.account 是不透明 Value）。
        "envelope": { "cipher": "deadbeef", "version": 2 },
        "tags": ["a", "b"]
    })
}

fn read_accounts(region: Region) -> Vec<Value> {
    account::load_accounts_for(region)
}

// ---------------------------------------------------------------------------
// 核心 1：软删除 → 恢复**往返无损**（逐字段深比较）
// ---------------------------------------------------------------------------

/// 删除一个账号 ⇒ 从 live 消失、出现在 trash；恢复 ⇒ 回到 live 且**逐字段**无损。
///
/// 断言方式是 `restored == original`（`serde_json::Value` 的深比较）——比「抽查几个
/// 字段」更强：任何一个字段（含凭据 / 备注 / 任意扩展字段）丢失或改动都会变红。
#[test]
fn soft_delete_then_restore_is_lossless_for_every_field() {
    let home = IsolatedHome::new("roundtrip");
    let original = rich_account("acc-1", "u-1");
    account::save_accounts_for(Region::Cn, &[original.clone()]).expect("seed");

    // 前置：live 有 1 条、trash 为空。
    assert_eq!(read_accounts(Region::Cn), vec![original.clone()]);
    assert!(load_trash_for(Region::Cn).is_empty());

    // 删除 ⇒ live 消失、trash 出现。
    trash_account_for(Region::Cn, "acc-1").expect("soft delete");
    assert!(read_accounts(Region::Cn).is_empty(), "软删除后 live 必须清空");
    let trash = load_trash_for(Region::Cn);
    assert_eq!(trash.len(), 1, "软删除后回收站必须恰好 1 条");
    assert_eq!(
        trash[0].account, original,
        "回收站里的记录必须与 live 原记录逐字节一致"
    );
    // 元数据：到期时间 = 删除时间 + 30 天。
    assert_eq!(trash[0].expires_at, trash[0].deleted_at + TRASH_RETENTION_MS);
    assert_eq!(trash[0].origin_scope, Scope::WbCn);

    // 恢复 ⇒ 回到 live 且**逐字段**无损。
    restore_account_for(Region::Cn, "acc-1").expect("restore");
    let live = read_accounts(Region::Cn);
    assert_eq!(live.len(), 1, "恢复后 live 必须恰好 1 条");
    assert_eq!(
        live[0], original,
        "恢复后的记录必须与删除前**逐字段**一致（凭据 / 备注 / 扩展字段都不能丢）"
    );
    assert!(load_trash_for(Region::Cn).is_empty(), "恢复后回收站必须清空");

    // 守恒：live 数量 + trash 数量 在往返后回到起点（不丢、不重）。
    assert_eq!(read_accounts(Region::Cn).len() + load_trash_for(Region::Cn).len(), 1);

    drop(home);
}

/// live 已存在同 key 时恢复必须 `Err`（**不覆盖**，否则会丢现有账号的数据）。
#[test]
fn restore_refuses_when_live_already_has_the_key() {
    let home = IsolatedHome::new("restore-conflict");
    account::save_accounts_for(Region::Cn, &[rich_account("acc-1", "u-1")]).expect("seed");

    trash_account_for(Region::Cn, "acc-1").expect("soft delete");
    // 用另一份同 key（但内容不同）的记录占住 live。
    let occupant = json!({ "id": "acc-1", "uid": "u-1", "nickname": "占位", "access_token": "NEW" });
    account::save_accounts_for(Region::Cn, &[occupant.clone()]).expect("occupy");

    let err = restore_account_for(Region::Cn, "acc-1").expect_err("必须拒绝覆盖式恢复");
    assert!(err.contains("已存在"), "错误文案应说明冲突，got: {err}");
    // 拒绝后 live 与 trash 都不得被改动。
    assert_eq!(read_accounts(Region::Cn), vec![occupant]);
    assert_eq!(load_trash_for(Region::Cn).len(), 1, "拒绝恢复不得消费回收站条目");

    drop(home);
}

// ---------------------------------------------------------------------------
// 核心 2：purge 是唯一不可逆步
// ---------------------------------------------------------------------------

/// `purge` 从 trash 真删（trash 里也没了）；`sweep` 只清**过期**条目 —— 新鲜的 purge 之外
/// 没有任何路径会永久删除。
#[test]
fn purge_is_the_only_irreversible_step_and_sweep_only_removes_expired() {
    let home = IsolatedHome::new("purge-only");
    account::save_accounts_for(
        Region::Cn,
        &[rich_account("a1", "u1"), rich_account("a2", "u2")],
    )
    .expect("seed");

    trash_account_for(Region::Cn, "a1").expect("soft delete a1");
    trash_account_for(Region::Cn, "a2").expect("soft delete a2");
    assert_eq!(load_trash_for(Region::Cn).len(), 2);

    // ★ 关键反例：用**当前时间** sweep —— 两条都是新鲜的，**一条都不能被清掉**。
    //   （若 sweep 被写成「无条件删最旧」或误用 0 作为 now，这里会变红。）
    let swept = account::sweep_trash_for(Region::Cn, now_ms());
    assert_eq!(swept, 0, "sweep 不得清掉未过期的条目");
    assert_eq!(load_trash_for(Region::Cn).len(), 2, "未过期条目必须原样留存");

    // purge 之后 trash 里也没了 —— 这才是唯一不可逆步。
    purge_account_for(Region::Cn, "a1").expect("purge");
    let trash = load_trash_for(Region::Cn);
    assert_eq!(trash.len(), 1, "purge 后回收站应只剩 1 条");
    assert_eq!(trash[0].account["id"], "a2");
    // purge 一个不存在的 key ⇒ 如实报错（不是静默成功）。
    assert!(purge_account_for(Region::Cn, "u1").is_err());

    // purge 不影响 live（live 早已在软删除时清空），也不得复活任何账号。
    assert!(read_accounts(Region::Cn).is_empty());

    drop(home);
}

// ---------------------------------------------------------------------------
// 核心 3：留存策略（30 天 + 上限 200，先到先出）
// ---------------------------------------------------------------------------

/// 过期条目被清、未过期条目保留。
#[test]
fn sweep_removes_expired_and_keeps_fresh() {
    let now = now_ms();
    let mut entries = vec![
        TrashEntry {
            account: rich_account("old", "u-old"),
            deleted_at: now - TRASH_RETENTION_MS - 10,
            expires_at: now - 10,
            origin_scope: Scope::WbCn,
        },
        TrashEntry {
            account: rich_account("fresh", "u-fresh"),
            deleted_at: now,
            expires_at: now + TRASH_RETENTION_MS,
            origin_scope: Scope::WbCn,
        },
    ];
    assert_eq!(sweep_trash_entries(&mut entries, now), 1, "只应清掉 1 条过期");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].account["id"], "fresh", "留下的必须是未过期的那条");

    // 边界：恰好等于 now 的 expires_at 视为**已过期**（`> now` 才保留）。
    let mut boundary = vec![TrashEntry {
        account: rich_account("edge", "u-edge"),
        deleted_at: now,
        expires_at: now,
        origin_scope: Scope::WbCn,
    }];
    assert_eq!(sweep_trash_entries(&mut boundary, now), 1, "expires_at == now 应过期");
}

/// 构造 201 条 ⇒ 应只留 200（裁掉最旧的 1 条，先到先出）。
#[test]
fn sweep_enforces_200_entry_cap_oldest_first() {
    let now = now_ms();
    // deleted_at = 0..200（升序），expires_at 全部未过期。
    let mut many: Vec<TrashEntry> = (0..TRASH_MAX_ENTRIES + 1)
        .map(|i| TrashEntry {
            account: rich_account(&format!("k{i}"), &format!("u{i}")),
            deleted_at: i as i64,
            expires_at: now + TRASH_RETENTION_MS,
            origin_scope: Scope::WbCn,
        })
        .collect();
    assert_eq!(many.len(), 201);

    let removed = sweep_trash_entries(&mut many, now);
    assert_eq!(removed, 1, "超上限 1 条应被裁掉");
    assert_eq!(many.len(), TRASH_MAX_ENTRIES, "上限必须是 200");
    // 最旧的 k0（deleted_at=0）被裁；新的最旧项是 k1。
    assert_eq!(many[0].account["id"], "k1", "先到先出：应裁掉最旧的");
    assert_eq!(
        many[many.len() - 1].account["id"],
        "k200",
        "最新的必须保留"
    );
}

/// 通过**真实文件**验证 `sweep_trash_for` 落盘：过期条目被清、新鲜条目留存。
#[test]
fn sweep_trash_for_persists_to_disk() {
    let home = IsolatedHome::new("sweep-persist");
    let trash_path = accounts_trash_file_for(Region::Cn);
    let now = now_ms();
    let seed = vec![
        TrashEntry {
            account: rich_account("old", "u-old"),
            deleted_at: now - TRASH_RETENTION_MS - 10,
            expires_at: now - 10,
            origin_scope: Scope::WbCn,
        },
        TrashEntry {
            account: rich_account("fresh", "u-fresh"),
            deleted_at: now,
            expires_at: now + TRASH_RETENTION_MS,
            origin_scope: Scope::WbCn,
        },
    ];
    fs::create_dir_all(trash_path.parent().expect("parent")).expect("mkdir");
    fs::write(&trash_path, serde_json::to_string(&seed).unwrap()).expect("write trash");

    assert_eq!(account::sweep_trash_for(Region::Cn, now), 1);
    // 重新从盘读：必须只剩 fresh。
    let after = load_trash_for(Region::Cn);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].account["id"], "fresh");

    drop(home);
}

// ---------------------------------------------------------------------------
// 核心 4：按 uid 删除不再空转（id ≠ uid，三条路径都验）
// ---------------------------------------------------------------------------

/// `id ≠ uid` 时，**按 uid** 软删 / 恢复 / purge 都必须真的命中（不能「移 0 条却报成功」）。
#[test]
fn delete_restore_purge_by_uid_all_hit_when_id_differs() {
    let home = IsolatedHome::new("by-uid");
    // 关键：id 与 uid **不同**，且查找用的 key 是 uid。
    let original = rich_account("id-AAA", "uid-BBB");
    account::save_accounts_for(Region::Cn, &[original.clone()]).expect("seed");

    // ① 软删除：按 uid。
    trash_account_for(Region::Cn, "uid-BBB").expect("按 uid 软删除必须成功");
    assert!(read_accounts(Region::Cn).is_empty(), "按 uid 软删除必须真的移出 live");
    let trash = load_trash_for(Region::Cn);
    assert_eq!(trash.len(), 1, "按 uid 软删除必须真的移入 1 条（不是 0 条）");
    assert_eq!(trash[0].account["uid"], "uid-BBB");

    // ② 恢复：按 uid。
    restore_account_for(Region::Cn, "uid-BBB").expect("按 uid 恢复必须成功");
    assert_eq!(read_accounts(Region::Cn), vec![original], "按 uid 恢复必须原样回到 live");
    assert!(load_trash_for(Region::Cn).is_empty());

    // ③ purge：先按 uid 再删一次，然后按 uid purge。
    trash_account_for(Region::Cn, "uid-BBB").expect("按 uid 再删");
    assert_eq!(load_trash_for(Region::Cn).len(), 1);
    purge_account_for(Region::Cn, "uid-BBB").expect("按 uid purge 必须成功");
    assert!(load_trash_for(Region::Cn).is_empty(), "按 uid purge 必须真的清掉");

    // 对照：删一个**不存在**的 key ⇒ 必须报错（不是静默成功）。
    assert!(trash_account_for(Region::Cn, "uid-NOPE").is_err());

    drop(home);
}

// ---------------------------------------------------------------------------
// 边界 1：trash 文件损坏（非法 JSON）⇒ 可理解行为（不崩溃 / 不清空 live）
// ---------------------------------------------------------------------------

/// trash 文件是非法 JSON 时：读取返回空表（不 panic），**绝不**连带清空 live 账号库。
#[test]
fn corrupted_trash_file_is_tolerated_without_touching_live() {
    let home = IsolatedHome::new("corrupt-trash");
    let live = vec![rich_account("acc-1", "u-1")];
    account::save_accounts_for(Region::Cn, &live).expect("seed");

    // 写入非法 JSON。
    let trash_path = accounts_trash_file_for(Region::Cn);
    fs::create_dir_all(trash_path.parent().expect("parent")).expect("mkdir");
    fs::write(&trash_path, "{ this is not valid json ][").expect("write corrupt");

    // 读取必须容错（返回空表，不 panic）。
    assert!(
        load_trash_for(Region::Cn).is_empty(),
        "损坏的 trash 文件应被当作空表（可理解行为），而不是崩溃"
    );
    // ★ 关键：live 账号库必须**原封不动**（损坏 trash ≠ 清空 live）。
    assert_eq!(read_accounts(Region::Cn), live, "损坏 trash 不得波及 live 账号库");

    // 损坏后再做一次软删除：应产生一份**合法**的 trash（自愈），且 live 正确减少。
    trash_account_for(Region::Cn, "acc-1").expect("软删除仍应可用");
    assert!(read_accounts(Region::Cn).is_empty(), "软删除应正常移出 live");
    let trash = load_trash_for(Region::Cn);
    assert_eq!(trash.len(), 1, "软删除应写出一份合法的 trash");
    assert_eq!(trash[0].account["id"], "acc-1");

    // 清空返回条数正确。
    assert_eq!(clear_trash_for(Region::Cn), 1);
    assert!(load_trash_for(Region::Cn).is_empty());

    drop(home);
}

// ---------------------------------------------------------------------------
// 边界 2：未知 scope ⇒ Err（fail-closed）
// ---------------------------------------------------------------------------

/// `resolve_scope` / `resolve_scope_param` 对未知 / 空 / 缺 / 大小写错误一律 `Err`，
/// **绝不**回落默认作用域（否则会静默读写错误的库）。
#[test]
fn unknown_scope_is_rejected_fail_closed() {
    // 合法值。
    assert_eq!(resolve_scope("wb-cn").unwrap(), Scope::WbCn);
    assert_eq!(resolve_scope("wb-global").unwrap(), Scope::WbGlobal);
    assert_eq!(resolve_scope("trae-cn").unwrap(), Scope::TraeCn);
    assert_eq!(resolve_scope("trae-global").unwrap(), Scope::TraeGlobal);

    // 拼错（派单点名的 `trae-glboal`）⇒ Err。
    assert!(resolve_scope("trae-glboal").is_err(), "拼错的 scope 必须 Err");
    // 空 / 未知。
    assert!(resolve_scope("").is_err(), "空 scope 必须 Err");
    assert!(resolve_scope("wb").is_err(), "不完整 scope 必须 Err");
    // 大小写不宽容（fail-closed 的判据：不做折叠）。
    assert!(resolve_scope("WB-CN").is_err(), "scope 不做大小写折叠");

    // 命令层入口：None（缺）/ 空 ⇒ Err，且错误文案可读（列出合法取值）。
    let err = resolve_scope_param(None).expect_err("缺 scope 必须 Err");
    assert!(err.contains("wb-cn"), "错误文案应列出合法取值，got: {err}");
    assert!(resolve_scope_param(Some("")).is_err());
    assert!(resolve_scope_param(Some("trae-glboal")).is_err());
    assert_eq!(resolve_scope_param(Some("wb-global")).unwrap(), Scope::WbGlobal);

    // 平台分支守卫：用 Trae 作用域走 WB 分支 ⇒ Err（反之亦然）。
    assert!(require_wb_region(Scope::TraeCn).is_err());
    assert!(require_wb_region(Scope::TraeGlobal).is_err());
    assert_eq!(require_wb_region(Scope::WbCn).unwrap(), Region::Cn);

    // region → scope 派生穷举且正确（回收站冗余记 origin_scope 的依据）。
    assert_eq!(scope_of_wb_region(Region::Cn), Scope::WbCn);
    assert_eq!(scope_of_wb_region(Region::Global), Scope::WbGlobal);
}

/// 回收站按 scope / region 隔离：CN 的 trash 与 Global 的 trash 是两个**不同文件**。
#[test]
fn trash_is_isolated_per_region() {
    let home = IsolatedHome::new("region-isolation");

    // 路径隔离：live 与 trash 四个路径两两不同。
    let cn_trash = accounts_trash_file_for(Region::Cn);
    let global_trash = accounts_trash_file_for(Region::Global);
    assert_ne!(cn_trash, global_trash, "CN / Global 回收站必须是不同文件");
    assert_ne!(cn_trash, accounts_file_for(Region::Cn), "trash 不得与 live 同名");
    assert_ne!(global_trash, accounts_file_for(Region::Global));

    // 数据隔离：往 CN 删账号，Global 的回收站不得出现它。
    account::save_accounts_for(Region::Cn, &[rich_account("cn-1", "u-cn")]).expect("seed cn");
    trash_account_for(Region::Cn, "cn-1").expect("soft delete cn");
    assert_eq!(load_trash_for(Region::Cn).len(), 1);
    assert!(
        load_trash_for(Region::Global).is_empty(),
        "CN 的删除不得出现在 Global 的回收站里"
    );

    drop(home);
}
