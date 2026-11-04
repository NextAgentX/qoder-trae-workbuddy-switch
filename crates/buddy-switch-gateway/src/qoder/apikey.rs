//! Qoder 网关的多 API Key 存储（`sk-qoder-` 前缀 + **归属区域** + 三态动作）。
//!
//! 与 Trae 侧（`trae/apikey.rs`）**同款结构**：`sha256` + 可空明文（用户选择「保存到本软件」才落盘）；
//! `prefix` / `suffix` 供掩码展示（`prefix••••••••suffix`，**不含省略号**）；三态动作
//! **吊销 → 恢复 → 删除**，且**已吊销的行必须同时给「恢复 + 删除」两个出口**
//! （两平台同形的产品纪律 —— 只给删除会让「误吊销」只能靠删号重来）。
//!
//! ## 前缀为什么是 `sk-qoder-`（2026-10-17 改）
//!
//! 四条产品线的 Key 前缀是**同一族**，看一眼就知道该配哪个地址：
//! `sk-wb-`（WorkBuddy）· `sk-trae-`（Trae）· **`sk-qoder-`（Qoder）** · `sk-uni-`（统一网关）。
//! 原先 Qoder 用短前缀 `qd-`，与另外三条不成族——用户手里一把 `qd-…` 的 Key
//! 无从判断它属于哪个网关，AI 工具的「按前缀认提供商」规则也匹配不上。
//!
//! ⚠️ **兼容**：校验只比对 `sha256(完整明文)`，**不看前缀** ⇒ 老 `qd-…` Key
//! 升级后照常可用（改名前创建的记录原样留在磁盘上）；只有**新创建**的 Key 用新前缀。
//!
//! ## 与 Trae 侧的**两处刻意差异**
//!
//! 1. **绑定 `region` 而不是程序位**：Qoder 没有程序位轴（一个区域一本账号库、一套端点），
//!    网关按区域选池 ⇒ Key 归属就是区域。
//! 2. **没有 legacy 回落**：Qoder 网关是新增产品线，不存在「设置里的旧 apiKey」可迁 ——
//!    不照搬 Trae 的 `settings.api_key` 合成回落（那会把别人的历史包袱搬进来）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use buddy_switch_core::modules::qoder::region::QoderRegion;

/* ★ issue #76：策略类型**直接复用** WorkBuddy 侧那一套（`crate::apikey`）。
三条产品线的 Key 策略语义完全相同，各定义一份必然漂移。 */
use crate::apikey::{
    constant_time_eq, key_taken_message, resolve_key_plaintext, KeyPolicy, QuotaMode, QuotaPeriod,
    QuotaUnit, ScopeMode,
};

/// 新 Key 的前缀字面量（创建时 `sk-qoder-<secret 前 4 位>`）。
///
/// 与 `sk-wb-` / `sk-trae-` / `sk-uni-` 同族；改这里只影响**新建**的 Key。
pub const KEY_PREFIX: &str = "sk-qoder-";

/// 明文 Key 的密钥段长度（32 位十六进制，与另三条产品线一致）。
const SECRET_HEX_LEN: usize = 32;

/// 区域 ↔ 字符串（磁盘 / 线上形状 `"cn"` / `"global"`；与 `QoderRegion::as_str` 一致）。
fn region_as_str<S: serde::Serializer>(
    region: &QoderRegion,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(region.as_str())
}

fn region_from_str<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<QoderRegion, D::Error> {
    let raw = String::deserialize(deserializer)?;
    Ok(QoderRegion::parse(&raw).unwrap_or_default())
}

/// 单条 Qoder API Key。**含归属区域**；`plaintext` 仅在用户选择「保存到本软件」后有值，旧记录为 `None`。
///
/// 磁盘 / 线上形状为 camelCase（`createdAt` / `revokedAt` / `lastUsedAt`），
/// 前端类型逐字对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QoderApiKeyRecord {
    pub id: String,
    pub name: String,
    /// 归属区域；缺省 = [`QoderRegion::default`]（国内版）。
    #[serde(
        default,
        serialize_with = "region_as_str",
        deserialize_with = "region_from_str"
    )]
    pub region: QoderRegion,
    /// 前缀（脱敏展示用），如 `sk-qoder-a1b2`。⚠️ **不含省略号**。
    pub prefix: String,
    /// 尾部 4 位（如 `9f3c`）。列表显示 `prefix••••••••suffix`。
    #[serde(default)]
    pub suffix: String,
    /// `sha256(hex)` of 完整明文 Key。
    pub hash: String,
    /// 完整明文；旧记录缺字段时为 `None`。
    #[serde(default)]
    pub plaintext: Option<String>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    #[serde(default)]
    pub last_used_at: Option<i64>,
    /* ── 策略（issue #76）—— 与 WB / Trae 侧同形同义，全 `#[serde(default)]`
    ⇒ 老台账零迁移（读出来 = 改造前行为：不限额 + 全部账号）。 ── */
    #[serde(default)]
    pub quota_mode: QuotaMode,
    #[serde(default)]
    pub quota_limit: u64,
    #[serde(default)]
    pub quota_unit: QuotaUnit,
    #[serde(default)]
    pub quota_period: QuotaPeriod,
    #[serde(default)]
    pub scope_mode: ScopeMode,
    #[serde(default)]
    pub scope_accounts: Vec<String>,
    #[serde(default)]
    pub scope_groups: Vec<String>,
}

impl QoderApiKeyRecord {
    /// 列表视图（**不含 hash**，含可空 `plaintext`）。
    pub fn masked(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "region": self.region.as_str(),
            "prefix": self.prefix,
            "suffix": self.suffix,
            "plaintext": self.plaintext,
            "createdAt": self.created_at,
            "revokedAt": self.revoked_at,
            "revoked": self.revoked_at.is_some(),
            "lastUsedAt": self.last_used_at,
            /* 策略七件套（issue #76）—— 与 WB / Trae 侧同形，前端共用一套构件。 */
            "quotaMode": self.quota_mode,
            "quotaLimit": self.quota_limit,
            "quotaUnit": self.quota_unit,
            "quotaPeriod": self.quota_period,
            "scopeMode": self.scope_mode,
            "scopeAccounts": self.scope_accounts,
            "scopeGroups": self.scope_groups,
        })
    }

    /// 是否已吊销。
    /// 取出策略部分（中继层只认策略，不认 hash / 明文）。
    pub fn policy(&self) -> KeyPolicy {
        KeyPolicy {
            quota_mode: self.quota_mode,
            quota_limit: self.quota_limit,
            quota_unit: self.quota_unit,
            quota_period: self.quota_period,
            scope_mode: self.scope_mode,
            scope_accounts: self.scope_accounts.clone(),
            scope_groups: self.scope_groups.clone(),
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }
}

/// `sha256` 十六进制小写。
fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn random_id() -> String {
    // 新 Key 的 id 与 secret 都来自 UUID（v4）：id 不泄密，secret 是 32 位十六进制。
    uuid::Uuid::new_v4().simple().to_string()
}

/// 读写 `qoder/gateway_keys.json` 的多 Key 存储。
///
/// **无进程内缓存**：每次操作都读盘 —— 独立监听与宿主路由两份实例创建即可见、
/// 吊销即时生效（与 Trae / WB 侧同一纪律）。
pub struct QoderApiKeyStore {
    path: PathBuf,
}

impl QoderApiKeyStore {
    /// 新建存储（不读取文件；每次操作实时读盘）。
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// 存储文件路径（供诊断/展示）。
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// 读取 Key 列表（文件缺失 / 损坏 ⇒ 空表）。
    pub fn load(&self) -> Vec<QoderApiKeyRecord> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str::<Vec<QoderApiKeyRecord>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn save(&self, records: &[QoderApiKeyRecord]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(records).map_err(|error| error.to_string())?;
        buddy_switch_core::modules::config::atomic_write(&self.path, &text)
            .map_err(|error| error.to_string())
    }

    /// 生成 Key：明文 `sk-qoder-` + 32 位 hex；返回 `(记录, 明文)`。保留两参签名 ⇒ 存量调用点零改动。
    pub fn create(
        &self,
        name: String,
        region: QoderRegion,
    ) -> Result<(QoderApiKeyRecord, String), String> {
        self.create_with_key(name, region, None)
    }

    /// 自定义密钥的**创建前查重**（与 WB / Trae 侧同一规则）：`sha256(明文)` 不得与台账里
    /// 任何一条记录相同；已吊销的记录也算占用 —— 正确动作是 [`Self::restore`]。
    pub fn ensure_custom_key_available(&self, custom_key: Option<&str>) -> Result<(), String> {
        let Some(raw) = custom_key else {
            return Ok(());
        };
        let plaintext = raw.trim();
        if plaintext.is_empty() {
            return Ok(());
        }
        let hash = sha256_hex(plaintext);
        match self.load().iter().find(|record| record.hash == hash) {
            Some(existing) => Err(key_taken_message(
                &existing.name,
                existing.revoked_at.is_some(),
            )),
            None => Ok(()),
        }
    }

    /// 生成 / 接收 Key：`custom_key` 非空 ⇒ 原样使用；为空 ⇒ 按原规则 `sk-qoder-` + 32 位 hex 生成。
    ///
    /// 自定义密钥**先查重**（[`Self::ensure_custom_key_available`]）：同 hash 只允许一条记录，
    /// 否则吊销一条、另一条仍能通过 `verify`，吊销语义就烂了。
    ///
    /// ★ 2026-11-04：**创建时不落盘明文**（`plaintext: None`），
    ///   改由用户点「保存到本软件」时走 [`Self::save_plaintext`]。
    pub fn create_with_key(
        &self,
        name: String,
        region: QoderRegion,
        custom_key: Option<String>,
    ) -> Result<(QoderApiKeyRecord, String), String> {
        self.ensure_custom_key_available(custom_key.as_deref())?;
        let (secret, prefix, suffix) = resolve_key_plaintext(
            custom_key,
            || format!("{KEY_PREFIX}{}", &random_id()[..SECRET_HEX_LEN]),
            KEY_PREFIX,
        );
        let record = QoderApiKeyRecord {
            id: random_id(),
            name: if name.trim().is_empty() {
                "未命名 Key".to_string()
            } else {
                name.trim().to_string()
            },
            region,
            prefix,
            suffix,
            hash: sha256_hex(&secret),
            plaintext: None,
            created_at: now_ms(),
            revoked_at: None,
            last_used_at: None,
            // 策略默认值：不限额 + 全部账号 = 改造前行为。
            quota_mode: QuotaMode::Unlimited,
            quota_limit: 0,
            quota_unit: QuotaUnit::Token,
            quota_period: QuotaPeriod::None,
            scope_mode: ScopeMode::All,
            scope_accounts: Vec::new(),
            scope_groups: Vec::new(),
        };
        let mut records = self.load();
        records.push(record.clone());
        self.save(&records)?;
        Ok((record, secret))
    }

    /// 把「本次创建返回的明文」写入台账（用户点「保存到本软件」才走这里）。
    ///
    /// 与 WB / Trae 侧同一语义：id 必须存在、`sha256(明文)` 必须等于 `hash`、已有明文幂等不覆盖。
    pub fn save_plaintext(&self, id: &str, plaintext: &str) -> Result<(), String> {
        let plaintext = plaintext.trim();
        if plaintext.is_empty() {
            return Err("明文为空".to_string());
        }
        let digest = sha256_hex(plaintext);
        let mut records = self.load();
        let mut found = false;
        for record in records.iter_mut() {
            if record.id != id {
                continue;
            }
            found = true;
            if record.plaintext.is_some() {
                return Ok(());
            }
            // 与另三条产品线同一写法：常量时间比较（`verify` 也用它）。
            if !constant_time_eq(record.hash.as_bytes(), digest.as_bytes()) {
                return Err("明文与该密钥不匹配".to_string());
            }
            record.plaintext = Some(plaintext.to_string());
            break;
        }
        if !found {
            return Err("API Key 不存在".to_string());
        }
        self.save(&records)
    }

    /// 校验一把明文 Key：命中且未吊销 ⇒ 记录，并**顺手 touch** `lastUsedAt`。
    pub fn verify(&self, presented: &str) -> Option<QoderApiKeyRecord> {
        let trimmed = presented.trim();
        if trimmed.is_empty() {
            return None;
        }
        let hash = sha256_hex(trimmed);
        let records = self.load();
        let record = records
            .iter()
            .find(|record| record.hash == hash && !record.is_revoked())?;
        self.touch(&record.id);
        Some(record.clone())
    }

    /// 记录一次使用（写盘失败**忽略** —— 使用时间戳是统计信息，不能阻断请求）。
    pub fn touch(&self, id: &str) {
        let mut records = self.load();
        if let Some(record) = records.iter_mut().find(|record| record.id == id) {
            record.last_used_at = Some(now_ms());
            let _ = self.save(&records);
        }
    }

    /// 吊销（置 `revoked_at`；已吊销 ⇒ 可读错误）。
    /// 更新策略（配额 + 账号范围）。**不动** id / hash / 吊销态 / region。
    ///
    /// 与 WB / Trae 侧**同一套规则**（复用 `KeyPolicy::normalized`）。
    pub fn update_policy(&self, id: &str, policy: KeyPolicy) -> Result<(), String> {
        let policy = policy.normalized()?;
        let mut records = self.load();
        let mut found = false;
        for record in records.iter_mut() {
            if record.id == id {
                record.quota_mode = policy.quota_mode;
                record.quota_limit = policy.quota_limit;
                record.quota_unit = policy.quota_unit;
                record.quota_period = policy.quota_period;
                record.scope_mode = policy.scope_mode;
                record.scope_accounts = policy.scope_accounts;
                record.scope_groups = policy.scope_groups;
                found = true;
                break;
            }
        }
        if !found {
            return Err("API Key 不存在".to_string());
        }
        self.save(&records)
    }

    pub fn revoke(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let record = records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| "找不到该 Key".to_string())?;
        if record.is_revoked() {
            return Err("该 Key 已是吊销状态".to_string());
        }
        record.revoked_at = Some(now_ms());
        self.save(&records)
    }

    /// 恢复吊销（清 `revoked_at`；未吊销 ⇒ 可读错误）。
    pub fn restore(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let record = records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| "找不到该 Key".to_string())?;
        if !record.is_revoked() {
            return Err("该 Key 未被吊销，无需恢复".to_string());
        }
        record.revoked_at = None;
        self.save(&records)
    }

    /// 删除（**仅已吊销可删** —— 未吊销必须先吊销，防误删在用凭据）。
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let records = self.load();
        let record = records
            .iter()
            .find(|record| record.id == id)
            .ok_or_else(|| "找不到该 Key".to_string())?;
        if !record.is_revoked() {
            return Err("只能删除已吊销的 Key：请先吊销，再删除".to_string());
        }
        let remaining: Vec<QoderApiKeyRecord> = records
            .into_iter()
            .filter(|record| record.id != id)
            .collect();
        self.save(&remaining)
    }

    /// 列表（脱敏）。
    pub fn list(&self) -> Vec<Value> {
        self.load().iter().map(QoderApiKeyRecord::masked).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (QoderApiKeyStore, PathBuf) {
        let path = std::env::temp_dir().join(format!("qoder-keys-test-{}.json", random_id()));
        (QoderApiKeyStore::new(path.clone()), path)
    }

    /// 自定义密钥查重：同 `hash` 只能有一条记录（**已吊销的也算占用**），被拒时不落盘。
    ///
    /// 变异：删掉 `create_with_key` 里的 `ensure_custom_key_available(...)?` ⇒ 本测试红。
    #[test]
    fn duplicate_custom_key_is_rejected() {
        let (store, _path) = temp_store();
        let key = "dup-qoder-key-0001";
        let (record, _) = store
            .create_with_key("甲".into(), QoderRegion::Cn, Some(key.into()))
            .expect("首次创建");

        let error = store
            .create_with_key("乙".into(), QoderRegion::Cn, Some(key.into()))
            .expect_err("重复的自定义密钥必须被拒");
        assert!(error.contains("甲"), "要点名占用者: {error}");
        assert_eq!(store.load().len(), 1, "被拒时不得落盘");

        // 吊销不释放 hash：再建一把同值只会得到两条同 hash 记录（吊销语义就废了）。
        store.revoke(&record.id).expect("吊销");
        let error = store
            .create_with_key("丙".into(), QoderRegion::Cn, Some(key.into()))
            .expect_err("已吊销也占用");
        assert!(error.contains("恢复"), "应提示走恢复: {error}");
        assert_eq!(store.load().len(), 1);
    }

    /// 三态动作的合同：吊销 → 恢复 → 删除；未吊销不可删。
    ///
    /// 变异：把 `delete` 的「仅已吊销可删」守卫删掉 ⇒ 本测试红。
    #[test]
    fn key_lifecycle_three_states_and_plaintext_on_disk() {
        let (store, path) = temp_store();
        let (record, secret) = store.create("测试".into(), QoderRegion::Cn).unwrap();
        assert!(secret.starts_with(KEY_PREFIX), "{secret}");
        assert!(record.prefix.starts_with(KEY_PREFIX));

        // ★ 需求变更（第二次）：创建时不落盘，用户点「保存到本软件」才写。
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(!on_disk.contains(&secret), "创建时不得落盘明文");
        store.save_plaintext(&record.id, &secret).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains(&secret), "保存后明文应落盘");

        // 校验通过 + touch 生效。
        assert!(store.verify(&secret).is_some());
        assert!(store.load()[0].last_used_at.is_some());

        // 未吊销不可删。
        assert!(store.delete(&record.id).is_err());
        // 吊销（幂等保护）→ 校验失败。
        store.revoke(&record.id).unwrap();
        assert!(store.revoke(&record.id).is_err());
        assert!(store.verify(&secret).is_none(), "吊销后必须拒绝");
        // 恢复 → 又能用。
        store.restore(&record.id).unwrap();
        assert!(store.verify(&secret).is_some());
        assert!(store.restore(&record.id).is_err(), "未吊销无需恢复");
        // 吊销 → 删除。
        store.revoke(&record.id).unwrap();
        store.delete(&record.id).unwrap();
        assert!(store.load().is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn legacy_ledger_without_policy_fields_reads_as_before() {
        // Qoder 台账落盘是 camelCase（struct 上有 `rename_all`，与 WB 相反）。
        let legacy = serde_json::json!([{
            "id": "old", "name": "old", "region": "cn", "prefix": "sk-qoder-a1b2",
            "suffix": "9f3c", "hash": "h", "createdAt": 1, "revokedAt": null,
            "lastUsedAt": null
        }]);
        let records: Vec<QoderApiKeyRecord> = serde_json::from_value(legacy).unwrap();
        let record = &records[0];
        assert_eq!(record.quota_mode, QuotaMode::Unlimited);
        assert_eq!(record.scope_mode, ScopeMode::All);
        assert_eq!(
            crate::apikey::quota_verdict_for_policy(&record.policy(), u64::MAX),
            crate::apikey::QuotaVerdict::Ok,
            "老 Key 不得被限额"
        );
    }

    #[test]
    fn update_policy_writes_and_normalizes_like_the_other_lines() {
        let (store, path) = temp_store();
        let (record, _) = store.create("p".into(), QoderRegion::Cn).unwrap();

        // 空白名单 ⇒ 拒绝（三条产品线同一规则）。
        assert!(store
            .update_policy(
                &record.id,
                KeyPolicy {
                    quota_mode: QuotaMode::Limited,
                    quota_limit: 10,
                    quota_unit: QuotaUnit::Request,
                    quota_period: QuotaPeriod::Daily,
                    scope_mode: ScopeMode::Selected,
                    scope_accounts: vec![],
                    scope_groups: vec![],
                }
            )
            .is_err());

        store
            .update_policy(
                &record.id,
                KeyPolicy {
                    quota_mode: QuotaMode::Limited,
                    quota_limit: 300,
                    quota_unit: QuotaUnit::Token,
                    quota_period: QuotaPeriod::Weekly,
                    scope_mode: ScopeMode::Selected,
                    scope_accounts: vec!["u1".into()],
                    scope_groups: vec!["g1".into()],
                },
            )
            .unwrap();
        // ★ Qoder 的 `list()` 返回的是**掩码视图**（`Vec<Value>`，camelCase），
        //   不是记录本体 ⇒ 断言按 JSON 键读（与前端看到的形态一致，反而是更好的验收）。
        let after = store
            .list()
            .into_iter()
            .find(|row| row["id"] == serde_json::json!(record.id))
            .unwrap();
        assert_eq!(after["quotaLimit"], serde_json::json!(300));
        assert_eq!(after["quotaPeriod"], serde_json::json!("weekly"));
        assert_eq!(after["scopeGroups"], serde_json::json!(["g1"]));
        assert!(after.get("hash").is_none(), "掩码视图不得带 hash");

        // 矛盾归一：不限额 ⇒ 清上限。
        store
            .update_policy(
                &record.id,
                KeyPolicy {
                    quota_mode: QuotaMode::Unlimited,
                    quota_limit: 999,
                    quota_unit: QuotaUnit::Token,
                    quota_period: QuotaPeriod::None,
                    scope_mode: ScopeMode::All,
                    scope_accounts: vec!["u1".into()],
                    scope_groups: vec![],
                },
            )
            .unwrap();
        let normalized = store
            .list()
            .into_iter()
            .find(|row| row["id"] == serde_json::json!(record.id))
            .unwrap();
        assert_eq!(normalized["quotaLimit"], serde_json::json!(0));
        assert_eq!(normalized["scopeAccounts"], serde_json::json!([]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn list_carries_plaintext_but_never_hash() {
        let (store, path) = temp_store();
        let (record, secret) = store.create("m".into(), QoderRegion::Global).unwrap();
        // 未保存前：键在、值为 null。
        assert!(store.list()[0]["plaintext"].is_null());
        store.save_plaintext(&record.id, &secret).unwrap();
        let list = store.list();
        let text = serde_json::to_string(&list).unwrap();
        assert_eq!(list[0]["plaintext"], serde_json::json!(secret));
        assert!(!text.contains("hash"), "列表不得带 hash 字段");
        assert_eq!(list[0]["region"], "global");
        assert!(list[0]["prefix"].as_str().unwrap().starts_with(KEY_PREFIX));
        let _ = std::fs::remove_file(&path);
    }

    /// 明文形态与另三条产品线**同族**：`sk-qoder-` + 32 位 hex。
    ///
    /// 变异：把 `KEY_PREFIX` 改回 `qd-`，或把 hex 段长度改回 28 ⇒ 本条红。
    #[test]
    fn plaintext_shape_matches_the_other_product_lines() {
        let (store, path) = temp_store();
        let (record, secret) = store.create("shape".into(), QoderRegion::Cn).unwrap();
        assert!(
            secret.starts_with("sk-qoder-"),
            "前缀应为 sk-qoder-：{secret}"
        );
        assert_eq!(
            secret.len(),
            "sk-qoder-".len() + SECRET_HEX_LEN,
            "明文长度应为前缀 + 32 位 hex：{secret}"
        );
        let hex = &secret["sk-qoder-".len()..];
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit()),
            "密钥段必须是十六进制：{hex}"
        );
        assert_eq!(
            record.prefix,
            format!("sk-qoder-{}", &hex[..4]),
            "展示前缀 = 前缀 + 密钥段前 4 位"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// 改前缀**不废老 Key**：校验只比 `sha256(完整明文)`、不看前缀
    /// ⇒ 磁盘上既有的 `qd-…` 记录升级后照常通过。
    #[test]
    fn legacy_qd_keys_still_verify_after_the_prefix_change() {
        let (store, path) = temp_store();
        let legacy = "qd-0123456789abcdef0123456789abcd";
        let record = QoderApiKeyRecord {
            id: "legacy-1".to_string(),
            name: "老 Key".to_string(),
            region: QoderRegion::Cn,
            prefix: "qd-0123".to_string(),
            suffix: "abcd".to_string(),
            hash: sha256_hex(legacy),
            plaintext: None,
            created_at: 0,
            revoked_at: None,
            last_used_at: None,
            // 策略默认值（测试构造点也要给全 —— 缺字段是编译错，不是运行期）。
            quota_mode: QuotaMode::Unlimited,
            quota_limit: 0,
            quota_unit: QuotaUnit::Token,
            quota_period: QuotaPeriod::None,
            scope_mode: ScopeMode::All,
            scope_accounts: Vec::new(),
            scope_groups: Vec::new(),
        };
        let text = serde_json::to_string_pretty(&vec![record]).unwrap();
        std::fs::write(&path, text).unwrap();

        assert!(store.verify(legacy).is_some(), "老的 qd- Key 必须继续可用");
        assert!(store
            .verify("qd-ffffffffffffffffffffffffffffffff")
            .is_none());
        let _ = std::fs::remove_file(&path);
    }
}
