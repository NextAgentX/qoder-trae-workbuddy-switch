//! API Key 存储：**仅存 sha256 哈希 + 前缀**，明文仅在创建时返回一次。
//!
//! 落盘 `~/.buddy-switch/gateway_keys.json`。为支持「宿主合并路由」与「独立监听」
//! 两份实例共享同一批 Key，本存储**每次操作都读盘**（无进程内缓存），从而
//! 创建即可见、吊销即时生效。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::Region;

/// 单条 API Key 记录（明文不落库）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub region: Region,
    /// 前缀，如 `sk-wb-a1b2`（用于列表脱敏展示）。
    pub prefix: String,
    /// ★ E3：尾部 4 位（如 `9f3c`）—— 与 `prefix` 同为**非敏感**片段，
    /// 列表里显示成 `sk-wb-a1b2••••••••9f3c`，用户才能**确认是哪一把**。
    /// ⚠️ `#[serde(default)]` ⇒ **旧台账没有该字段时按空串处理**（显示时省略尾部，不报错）。
    #[serde(default)]
    pub suffix: String,
    /// `sha256(hex)` of full plaintext key。
    pub hash: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

impl ApiKeyRecord {
    /// 脱敏展示（不含 hash）。
    pub fn masked(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "region": self.region,
            "prefix": self.prefix,
            "suffix": self.suffix,
            "createdAt": self.created_at,
            "revokedAt": self.revoked_at,
            "revoked": self.revoked_at.is_some(),
            "lastUsedAt": self.last_used_at,
        })
    }
}

/// 读写 `gateway_keys.json` 的 Key 存储。
pub struct ApiKeyStore {
    path: PathBuf,
}

impl ApiKeyStore {
    /// 新建存储（不读取文件；每次操作实时读盘）。
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Vec<ApiKeyRecord> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str::<Vec<ApiKeyRecord>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn save(&self, records: &[ApiKeyRecord]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(records).map_err(|error| error.to_string())?;
        core_config::atomic_write(&self.path, &content).map_err(|error| error.to_string())
    }

    /// 生成 Key：明文 `sk-wb-` + 32 位 hex；返回 (记录, 明文)。明文仅此一次返回。
    pub fn create(&self, name: String, region: Region) -> (ApiKeyRecord, String) {
        let secret = uuid::Uuid::new_v4().simple().to_string(); // 32 hex
        let plaintext = format!("sk-wb-{secret}");
        let prefix = format!("sk-wb-{}", &secret[..4]);
        // ★ E3：尾部 4 位（32 hex 的末 4）—— 与 prefix 一样**不可逆推**出整把 key。
        let suffix = secret[secret.len() - 4..].to_string();
        let record = ApiKeyRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            region,
            prefix,
            suffix,
            hash: sha256_hex(&plaintext),
            created_at: core_config::now_ms(),
            revoked_at: None,
            last_used_at: None,
        };
        let mut records = self.load();
        records.push(record.clone());
        if let Err(error) = self.save(&records) {
            eprintln!("[gateway] 保存 API Key 失败: {error}");
        }
        (record, plaintext)
    }

    /// 常量时间比较哈希；返回有效记录，无效/不存在/已吊销返回 `None`。
    pub fn verify(&self, presented: &str) -> Option<ApiKeyRecord> {
        let presented = presented.trim();
        if presented.is_empty() {
            return None;
        }
        let digest = sha256_hex(presented);
        for record in self.load() {
            if constant_time_eq(record.hash.as_bytes(), digest.as_bytes()) {
                if record.revoked_at.is_some() {
                    return None;
                }
                return Some(record);
            }
        }
        None
    }

    /// 更新最近使用时间（60 秒节流，避免每个请求都写盘）。
    pub fn touch(&self, id: &str) {
        let mut records = self.load();
        let now = core_config::now_ms();
        let mut changed = false;
        for record in records.iter_mut() {
            if record.id == id {
                let stale = record
                    .last_used_at
                    .map(|last| now - last > 60_000)
                    .unwrap_or(true);
                if stale {
                    record.last_used_at = Some(now);
                    changed = true;
                }
                break;
            }
        }
        if changed {
            let _ = self.save(&records);
        }
    }

    /// 吊销（置 `revoked_at`，不物理删除）。
    pub fn revoke(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let mut found = false;
        for record in records.iter_mut() {
            if record.id == id {
                record.revoked_at = Some(core_config::now_ms());
                found = true;
                break;
            }
        }
        if !found {
            return Err("API Key 不存在".to_string());
        }
        self.save(&records)
    }

    /// 物理删除（仅允许删除**已吊销**的 Key）。
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let before = records.len();
        let target_revoked = records
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.revoked_at.is_some());
        match target_revoked {
            None => return Err("API Key 不存在".to_string()),
            Some(false) => return Err("请先吊销该 API Key 再删除".to_string()),
            Some(true) => {}
        }
        records.retain(|record| record.id != id);
        debug_assert!(records.len() < before);
        self.save(&records)
    }

    /// 列出全部 Key（含已吊销）。
    pub fn list(&self) -> Vec<ApiKeyRecord> {
        self.load()
    }
}

/// `sha256` 十六进制小写。
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 常量时间字节比较（长度不同直接 false）。
///
/// `pub(crate)`：Trae 的多 Key 存储（[`crate::trae::apikey`]）复用同一实现，
/// 避免两处各写一份常量时间比较——**鉴权比较的安全属性必须只定义一次**，
/// 否则任何一处被改成短路比较都不会被对方发现。
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_store() -> ApiKeyStore {
        let path = std::env::temp_dir().join(format!(
            "buddy-switch-gateway-keys-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        ApiKeyStore::new(path)
    }

    #[test]
    fn create_returns_plaintext_once_and_stores_only_hash() {
        let store = temp_store();
        let (record, plaintext) = store.create("cursor".to_string(), Region::Cn);

        assert!(plaintext.starts_with("sk-wb-"));
        assert_eq!(plaintext.len(), "sk-wb-".len() + 32);
        assert_eq!(record.prefix, format!("sk-wb-{}", &plaintext["sk-wb-".len().."sk-wb-".len() + 4]));
        assert_eq!(record.hash, sha256_hex(&plaintext));
        assert_ne!(record.hash, plaintext, "不得明文落库");

        let on_disk = std::fs::read_to_string(&store.path).unwrap();
        assert!(!on_disk.contains(&plaintext), "磁盘不得出现明文");
    }

    #[test]
    fn masked_is_an_explicit_non_secret_whitelist() {
        let store = temp_store();
        let (record, plaintext) = store.create("masked".to_string(), Region::Global);
        let masked = record.masked();
        let object = masked.as_object().unwrap();
        let fields: std::collections::BTreeSet<&str> = object.keys().map(String::as_str).collect();
        let expected: std::collections::BTreeSet<&str> = [
            // ★ E3 新增 `suffix`（尾部 4 位）。它是**显式白名单**的一员 ——
            //   本测试的意义正是「掩码视图只能有这些键」，多一个键就要**在这里被批准**，
            //   而不是悄悄混进去。下面两条断言保证它不泄露 hash / 明文。
            "id", "name", "region", "prefix", "suffix", "createdAt", "revokedAt", "revoked",
            "lastUsedAt",
        ]
        .into_iter()
        .collect();
        assert_eq!(fields, expected);
        assert!(!object.contains_key("hash"));
        assert!(!masked.to_string().contains(&record.hash));
        assert!(!masked.to_string().contains(&plaintext));
        // ★ `suffix` 恰好是明文的**末 4 位**（这是设计意图：用户靠它确认是哪一把），
        //   所以「明文以 suffix 结尾」是**必然**的，不是泄露。
        //   真正的边界是：suffix **只有 4 位**，加上 prefix 的 4 位也**远不足以**还原 32 位 secret
        //   ⇒ 掩码视图绝不包含 hash / 明文主体（上面两条断言已在守这条）。
        assert_eq!(record.suffix.len(), 4, "suffix 应恰好 4 位");
        assert!(
            plaintext.ends_with(&record.suffix),
            "suffix 就是明文末 4 位（供用户核对）"
        );
        assert_eq!(
            record.prefix.len(),
            "sk-wb-".len() + 4,
            "prefix 也只暴露 4 位"
        );
        assert_eq!(masked["region"], json!(Region::Global));
        assert_eq!(masked["revoked"], false);
    }

    #[test]
    fn verify_rejects_one_byte_difference_and_wrong_length() {
        let store = temp_store();
        let (_, plaintext) = store.create("verify".to_string(), Region::Cn);
        let mut different = plaintext.clone().into_bytes();
        let last = different.len() - 1;
        different[last] = if different[last] == b'0' { b'1' } else { b'0' };
        assert!(store.verify(&String::from_utf8(different).unwrap()).is_none());
        assert!(store.verify("x").is_none());
    }

    #[test]
    fn verify_accepts_valid_rejects_invalid_and_revoked() {
        let store = temp_store();
        let (record, plaintext) = store.create("k".to_string(), Region::Global);

        assert!(store.verify(&plaintext).is_some(), "有效 Key 应通过");
        assert!(store.verify("sk-wb-deadbeefdeadbeefdeadbeefdeadbeef").is_none(), "无效 Key 应拒绝");

        store.revoke(&record.id).unwrap();
        assert!(store.verify(&plaintext).is_none(), "已吊销 Key 应拒绝");
        assert!(store.list().iter().any(|r| r.id == record.id && r.revoked_at.is_some()));

        // 已吊销可物理删除；未吊销不可删除。
        store.delete(&record.id).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn delete_unrevoked_is_rejected() {
        let store = temp_store();
        let (record, _) = store.create("k".to_string(), Region::Cn);
        assert!(store.delete(&record.id).is_err());
    }

    #[test]
    fn revoke_and_delete_unknown_keys_are_rejected() {
        let store = temp_store();
        assert_eq!(store.revoke("missing"), Err("API Key 不存在".to_string()));
        assert_eq!(store.delete("missing"), Err("API Key 不存在".to_string()));
    }

    #[test]
    fn touch_is_throttled_for_recent_use() {
        let store = temp_store();
        let (record, _) = store.create("touch".to_string(), Region::Cn);
        store.touch(&record.id);
        let first = store.list().into_iter().find(|item| item.id == record.id).unwrap().last_used_at;
        assert!(first.is_some());
        store.touch(&record.id);
        let second = store.list().into_iter().find(|item| item.id == record.id).unwrap().last_used_at;
        assert_eq!(second, first);
    }

    #[test]
    fn two_instances_share_state_via_disk() {
        let path = std::env::temp_dir().join(format!(
            "buddy-switch-gateway-keys-shared-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let a = ApiKeyStore::new(path.clone());
        let b = ApiKeyStore::new(path.clone());
        let (_, plaintext) = a.create("shared".to_string(), Region::Cn);
        assert!(b.verify(&plaintext).is_some(), "另一实例应能看到新建 Key");
    }

    #[test]
    fn sha256_is_stable_lowercase_hex() {
        let digest = sha256_hex("anything");
        assert_eq!(digest.len(), 64);
        assert_eq!(digest, sha256_hex("anything"));
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // 已知向量：sha256("") = e3b0c442...
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
