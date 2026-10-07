//! 全局备份的**分区表**（表驱动）：一个可备份内容类型 = 一行。
//!
//! ## 为什么是表驱动（§1.11 / §4.2）
//!
//! 导出面板的分区勾选、导入的逐分区合并都从**同一张表**读，因此：
//!
//! - **新增可备份内容类型 = 加一行**（不用改 UI、不用改导出/导入编排）；
//! - 导出与导入**不会各写一份**（那正是本仓反复踩过的「两套实现漂移」）。
//!
//! 表本体只有一份（[`backup_sections`]），`backup.rs` 的导出组装与（T02 的）导入
//! 合并都遍历它。
//!
//! ## 分区与作用域的关系
//!
//! 每个分区都按 [`Scope`] 读（`read(scope)`）：账号 / 配置 / 网关都各自落在某个
//! 作用域的库或文件里。WorkBuddy 与 Trae 的差异**收敛在本文件的 read/merge 实现里**，
//! 不散落到导出/导入编排 —— 编排只认「遍历 SCOPES × 遍历分区表」。

use serde_json::{json, Value};

use crate::modules::account;
use crate::modules::config;
use crate::modules::export_import::{
    merge_import_record, ConflictPolicy, MergeOutcome, TokenPolicy,
};
use crate::modules::product::{require_trae_region, require_wb_region, Product, Scope};
use crate::modules::region::catalog_cache_file;
use crate::modules::trae;

/// 单个分区合并后的计数（供导入预览 / 结果逐 scope 汇总）。
///
/// 与前端 `ImportPreviewScopeRow`（`{ scope, added, overwritten, skipped, conflicts }`）
/// 同源：`conflicts` 是「命中 uid 且策略需用户裁决」的数量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SectionCounts {
    /// 新增（追加为新记录）。
    pub added: usize,
    /// 覆盖（命中 uid 且覆盖本地）。
    pub overwritten: usize,
    /// 跳过（命中 uid 且保留本地；或按策略不计入）。
    pub skipped: usize,
    /// 冲突（命中 uid，需用户裁决；本期批量策略下恒 0）。
    pub conflicts: usize,
}

/// 一个可备份分区：id / 怎么读 / 怎么合并 / 怎么回写。
///
/// - `read`：导出时按作用域读出该分区的 JSON（缺失 / 损坏时给安全空值，**不报错**）；
/// - `merge`：导入时把导入文件里的该分区合并进本地（策略由 [`ConflictPolicy`] 决定）；
/// - `write`：把合并后的结果**回写**本地存储（T02 导入需要；导出不调用它）。
///
/// ⚠️ 三个函数指针都是**唯一实现**：导出、导入都从这里取，**禁止**在编排里另写一份。
///
/// ## 为什么 `merge` 与 `write` 分开（而不是让 `merge` 直接落盘）
///
/// `merge` 收的是**内存里的 `Value`**（导出、导入、以及导入**预览**都用它）：预览要求
/// 「**只读、不写盘**」（§1.7 的安全阀），若 `merge` 自己落盘，预览就会真的改数据。
/// 因此把「算」与「写」拆开：预览只 `merge` 到本地数据的一份**副本**上、丢弃；导入才
/// 在 `merge` 之后调 `write` 落盘。
pub struct BackupSectionSpec {
    /// 分区 id，同时也是 envelope 里该分区的键名：`"accounts" | "config" | "gateway"`。
    pub id: &'static str,
    /// 读出该作用域下本分区的数据。
    pub read: fn(scope: Scope) -> Result<Value, String>,
    /// 把导入数据合并进本地数据（**纯内存**），返回计数。
    pub merge: fn(local: &mut Value, incoming: &Value, policy: ConflictPolicy) -> SectionCounts,
    /// 把合并后的分区数据**回写**本地存储。
    pub write: fn(scope: Scope, merged: &Value) -> Result<(), String>,
}

/// ★ 分区表本体（**唯一**）：导出面板与导入合并都从它读。
///
/// 顺序即 UI 展示顺序与 envelope 内的键顺序：`accounts` / `config` / `gateway`。
pub fn backup_sections() -> &'static [BackupSectionSpec] {
    &[
        BackupSectionSpec {
            id: "accounts",
            read: read_accounts,
            merge: merge_accounts,
            write: write_accounts,
        },
        BackupSectionSpec {
            id: "config",
            read: read_config,
            merge: merge_object_section,
            write: write_config,
        },
        BackupSectionSpec {
            id: "gateway",
            read: read_gateway,
            merge: merge_object_section,
            write: write_gateway,
        },
    ]
}

/// 按 id 取分区（找不到返回 `None`；调用方决定如何处理未知 id）。
pub fn section_by_id(id: &str) -> Option<&'static BackupSectionSpec> {
    backup_sections().iter().find(|spec| spec.id == id)
}

// ---------------------------------------------------------------------------
// 读：账号 / 配置 / 网关
// ---------------------------------------------------------------------------

/// 读账号记录（持久化形态，snake_case）。
///
/// - WorkBuddy：`accounts[.global].json` 的记录数组（`access_token` / `refresh_token`…）；
/// - Trae：`checkin_accounts.json` 的记录数组（`UserID` / `jwt` / `refresh_token`…）。
///
/// 记录字段**必须**是持久化形态（不是线上 camelCase），否则导入端按 uid 去重会取不到键。
fn read_accounts(scope: Scope) -> Result<Value, String> {
    match scope.product() {
        Product::Workbuddy => {
            let region = require_wb_region(scope)?;
            Ok(serde_json::to_value(account::load_accounts_for(region))
                .unwrap_or_else(|_| Value::Array(vec![])))
        }
        Product::Trae => {
            let region = require_trae_region(scope)?;
            let accounts = trae::account::load_accounts_for_region(region).accounts;
            Ok(serde_json::to_value(accounts).unwrap_or_else(|_| Value::Array(vec![])))
        }
    }
}

/// 读配置。
///
/// - WorkBuddy：签到 / 旅行 / 轮换 / 切换四项配置（与 §1.2 的 `config` 子键同源）；
/// - Trae：Trae 侧设置（`trae/settings.json`）。
///
/// ⚠️ 这些配置是**应用级单份**（不按 region 分家），因此同一平台的两个作用域读到的
/// 内容相同 —— 这是既有事实，不做去重（备份按作用域分组是契约要求）。
fn read_config(scope: Scope) -> Result<Value, String> {
    match scope.product() {
        Product::Workbuddy => Ok(json!({
            "checkin": config::load_checkin_config(),
            "travel": config::load_travel_config(),
            "autoRotate": config::load_auto_rotate_config(),
            "switch": config::load_switch_config(),
        })),
        Product::Trae => Ok(serde_json::to_value(trae::settings::load()).unwrap_or(Value::Null)),
    }
}

/// 读网关分区。
///
/// - WorkBuddy：`gateway_config.json` + `gateway_keys.json` + 该 region 的模型目录缓存；
/// - Trae：`trae/api_gateway.json` + `trae/api_gateway_keys.json`（Trae 网关模型清单
///   随客户端刷新，不入备份，给 `null`）。
///
/// 网关文件是**应用级单份**（不按 region 分家），故两版内容相同；模型目录缓存按 region
/// 分家，故读该作用域对应的 region。
fn read_gateway(scope: Scope) -> Result<Value, String> {
    match scope.product() {
        Product::Workbuddy => {
            let region = require_wb_region(scope)?;
            Ok(json!({
                "config": read_json_file(&config::gateway_config_file()),
                "keys": read_json_file(&config::gateway_keys_file()),
                "models": read_json_file(&catalog_cache_file(region)),
            }))
        }
        Product::Trae => Ok(json!({
            "config": read_json_file(&trae::paths::api_gateway_file()),
            "keys": read_json_file(&trae::paths::api_gateway_keys_file()),
            "models": Value::Null,
        })),
    }
}

/// 读一个 JSON 文件；缺失 / 不可读 / 非法 JSON 一律给 `Null`（**不报错**）。
///
/// 备份是「尽可能多地带走」，某个分区文件缺失不该让整份导出失败 —— 缺失即空。
fn read_json_file(path: &std::path::Path) -> Value {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// 合并：账号 / 通用对象分区
// ---------------------------------------------------------------------------

/// 合并账号分区：逐条走**唯一合并出口** [`merge_import_record`]（`TokenPolicy::Allow`）。
///
/// ⚠️ **不重写内层**：账号级导入与全局备份共用同一套合并语义，差异只在策略参数。
/// 全局备份用 `Allow`（不含凭据的备份全是无 token 记录，`Require` 会一条都导不进）。
fn merge_accounts(local: &mut Value, incoming: &Value, policy: ConflictPolicy) -> SectionCounts {
    let mut counts = SectionCounts::default();
    let incoming_items = incoming.as_array().cloned().unwrap_or_default();
    let mut accounts = local.as_array().cloned().unwrap_or_default();
    for item in &incoming_items {
        match merge_import_record(&mut accounts, item, policy, TokenPolicy::Allow) {
            MergeOutcome::Appended => counts.added += 1,
            MergeOutcome::Overwritten => counts.overwritten += 1,
            // KeepBoth 会「增行」：计入新增。
            MergeOutcome::KeptBoth => counts.added += 1,
            MergeOutcome::KeptLocal => counts.skipped += 1,
            MergeOutcome::Skipped => counts.skipped += 1,
        }
    }
    *local = Value::Array(accounts);
    counts
}

/// 合并「对象型」分区（配置 / 网关）：按 [`ConflictPolicy`] 逐**顶层键**合并。
///
/// - 本地没有的键 ⇒ 新增（`added`）；
/// - 本地已有的键 ⇒ 依策略：保留本地（`skipped`）/ 覆盖（`overwritten`）。
///
/// 只做**顶层键**合并：配置与网关都是「一组子对象」，逐子对象整体替换即可，不必递归到
/// 叶子（递归会制造「半新半旧」的中间态，反而更难解释）。
fn merge_object_section(
    local: &mut Value,
    incoming: &Value,
    policy: ConflictPolicy,
) -> SectionCounts {
    let mut counts = SectionCounts::default();
    let Some(incoming_map) = incoming.as_object() else {
        return counts;
    };
    let Some(local_map) = local.as_object_mut() else {
        return counts;
    };
    for (key, value) in incoming_map {
        if local_map.contains_key(key) {
            match policy {
                ConflictPolicy::KeepLocal | ConflictPolicy::Ask => counts.skipped += 1,
                ConflictPolicy::Overwrite | ConflictPolicy::KeepBoth => {
                    local_map.insert(key.clone(), value.clone());
                    counts.overwritten += 1;
                }
            }
        } else {
            local_map.insert(key.clone(), value.clone());
            counts.added += 1;
        }
    }
    counts
}

// ---------------------------------------------------------------------------
// 写：账号 / 配置 / 网关（T02 · 导入落盘）
// ---------------------------------------------------------------------------

/// 回写账号分区（合并结果）。
///
/// - WorkBuddy：`accounts[.global].json` 整体回存；
/// - Trae：**必须整体回存 `AccountsFile`**（不得字面重建容器，否则 `device_bindings`
///   被清空 —— 见 `trae::account::AccountsFile` 的不变式 I-1）。故先读回容器、只替换
///   `accounts` 字段。
fn write_accounts(scope: Scope, merged: &Value) -> Result<(), String> {
    let items = merged.as_array().cloned().unwrap_or_default();
    match scope.product() {
        Product::Workbuddy => {
            let region = require_wb_region(scope)?;
            account::save_accounts_for(region, &items)
                .map_err(|error| format!("保存账号库失败：{error}"))
        }
        Product::Trae => {
            let region = require_trae_region(scope)?;
            let mut file = trae::account::load_accounts_for_region(region);
            file.accounts = serde_json::from_value(Value::Array(items))
                .map_err(|error| format!("导入的账号记录形状不合法：{error}"))?;
            trae::account::save_accounts_for_region(region, &file)
        }
    }
}

/// 回写配置分区。
///
/// - WorkBuddy：签到 / 旅行 / 轮换 / 切换四项，各自走其既有 `save_*`（只保留已知字段）；
/// - Trae：整体写入 `trae/settings.json`（反序列化成 `TraeSettings` 再存）。
///
/// 只写**来件里出现过的子项**：备份文件可能只含部分配置，缺的子项保持本地不变
/// （避免用「没带这个键」抹掉用户当前配置）。
fn write_config(scope: Scope, merged: &Value) -> Result<(), String> {
    match scope.product() {
        Product::Workbuddy => {
            if let Some(value) = merged.get("checkin") {
                config::save_checkin_config(value)
                    .map_err(|error| format!("保存签到配置失败：{error}"))?;
            }
            if let Some(value) = merged.get("travel") {
                config::save_travel_config(value)
                    .map_err(|error| format!("保存旅行配置失败：{error}"))?;
            }
            if let Some(value) = merged.get("autoRotate") {
                config::save_auto_rotate_config(value)
                    .map_err(|error| format!("保存轮换配置失败：{error}"))?;
            }
            if let Some(value) = merged.get("switch") {
                config::save_switch_config(value)
                    .map_err(|error| format!("保存切换配置失败：{error}"))?;
            }
            Ok(())
        }
        Product::Trae => {
            let settings: trae::settings::TraeSettings = serde_json::from_value(merged.clone())
                .map_err(|error| format!("Trae 设置形状不合法：{error}"))?;
            trae::settings::save(&settings)
        }
    }
}

/// 回写网关分区。
///
/// - WorkBuddy：`gateway_config.json` + `gateway_keys.json` + 该 region 的模型目录缓存；
/// - Trae：`trae/api_gateway.json` + `trae/api_gateway_keys.json`（Trae 网关模型清单不入备份）。
///
/// ⚠️ 网关 Key 文件只存 `sha256` 哈希 + 前缀（**无明文**，见 `ApiKeyStore`），因此入备份
/// 不构成凭据泄露；`null`（备份里该子项为空）一律**跳过**，不用空值抹掉本地。
fn write_gateway(scope: Scope, merged: &Value) -> Result<(), String> {
    match scope.product() {
        Product::Workbuddy => {
            let region = require_wb_region(scope)?;
            write_json_section(merged.get("config"), &config::gateway_config_file())?;
            write_json_section(merged.get("keys"), &config::gateway_keys_file())?;
            write_json_section(merged.get("models"), &catalog_cache_file(region))?;
            Ok(())
        }
        Product::Trae => {
            write_json_section(merged.get("config"), &trae::paths::api_gateway_file())?;
            write_json_section(merged.get("keys"), &trae::paths::api_gateway_keys_file())?;
            Ok(())
        }
    }
}

/// 把一个 JSON 子项原子写入指定文件；`None` / `null` ⇒ 跳过（不用空值抹掉本地）。
fn write_json_section(value: Option<&Value>, path: &std::path::Path) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    config::atomic_write(path, &text).map_err(|error| format!("写入失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_table_covers_the_three_declared_sections_in_order() {
        let ids: Vec<&str> = backup_sections().iter().map(|spec| spec.id).collect();
        assert_eq!(ids, vec!["accounts", "config", "gateway"]);
        assert!(section_by_id("accounts").is_some());
        assert!(section_by_id("nope").is_none());
    }

    /// `merge_accounts` 用 `TokenPolicy::Allow`：**无 token 的记录也能合入**。
    ///
    /// 可证伪：若这里误用 `Require`（账号级语义），不含凭据的备份会一条都导不进，
    /// 「需重新登录」场景整条失效。
    #[test]
    fn merge_accounts_accepts_records_without_tokens() {
        let mut local = json!([]);
        let incoming = json!([{ "uid": "u1", "nickname": "甲" }]);
        let counts = merge_accounts(&mut local, &incoming, ConflictPolicy::KeepLocal);
        assert_eq!(counts.added, 1);
        assert_eq!(local.as_array().unwrap().len(), 1);
    }

    /// 默认 `KeepLocal`：命中 uid ⇒ 保留本地、不覆盖、不增行（幂等的核心）。
    #[test]
    fn merge_accounts_keep_local_skips_same_uid() {
        let mut local = json!([{ "uid": "u1", "nickname": "本地", "access_token": "t" }]);
        let incoming = json!([{ "uid": "u1", "nickname": "导入" }]);
        let counts = merge_accounts(&mut local, &incoming, ConflictPolicy::KeepLocal);
        assert_eq!(counts.skipped, 1);
        assert_eq!(local.as_array().unwrap().len(), 1);
        assert_eq!(local[0]["nickname"], "本地");
    }

    /// `KeepBoth` 是唯一「增行」策略：命中 uid 也追加为新记录。
    #[test]
    fn merge_accounts_keep_both_appends_new_row() {
        let mut local = json!([{ "uid": "u1", "id": "a1", "nickname": "本地" }]);
        let incoming = json!([{ "uid": "u1", "nickname": "导入" }]);
        let counts = merge_accounts(&mut local, &incoming, ConflictPolicy::KeepBoth);
        assert_eq!(counts.added, 1);
        assert_eq!(local.as_array().unwrap().len(), 2, "KeepBoth 必须增行");
    }

    #[test]
    fn merge_object_section_adds_and_overwrites_per_policy() {
        let mut local = json!({ "checkin": { "enabled": true } });
        let incoming = json!({ "checkin": { "enabled": false }, "travel": { "enabled": true } });

        let counts = merge_object_section(&mut local, &incoming, ConflictPolicy::Overwrite);
        assert_eq!(counts.added, 1, "travel 是新增");
        assert_eq!(counts.overwritten, 1, "checkin 被覆盖");
        assert_eq!(local["checkin"]["enabled"], json!(false));
        assert_eq!(local["travel"]["enabled"], json!(true));

        let mut local = json!({ "checkin": { "enabled": true } });
        let counts = merge_object_section(&mut local, &incoming, ConflictPolicy::KeepLocal);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.added, 1);
        assert_eq!(local["checkin"]["enabled"], json!(true), "保留本地");
    }
}
