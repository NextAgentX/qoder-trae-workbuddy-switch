//! Trae 多 API Key 存储：**sha256 哈希 + 前缀 + 归属程序位 + 可空明文**。
//! 明文**不随创建落盘**：用户点「保存到本软件」后由 `save_plaintext` 写入；
//! 「自行保存」的 Key 与旧记录一样无明文（列表不显示眼睛）。
//!
//! 落盘 `~/.buddy-switch/trae/api_gateway_keys.json`（见 [`paths::api_gateway_keys_file`]）。
//! 与 WorkBuddy 的 [`crate::apikey::ApiKeyStore`] 同构（每次操作都读盘、无进程内缓存，
//! 使「独立监听」与「宿主合并路由」两份实例看到同一批 Key），差异只有一处：
//! 把 `region: Region` 换成了 `variant: TraeVariant`。
//!
//! ⚠️ 该 `variant` 自 2026-09-30 起是**程序位**（区域 × 程序，4 个取值），不再是
//! 「产品线」也不是「区域」：它同时决定 `/v1/models` 列哪份客户端清单与请求体带哪个
//! `function`（见 [`super::routes::TraeKeyVariant`]）。旧值 `cn` / `global` 仍可读，
//! 经 `TraeVariant::parse` 落**区域主程序**（TraeWork），老 Key 行为不变。
//!
//! ## 两条**零回归**前提（改动这里前务必先读）
//!
//! 1. **旧文件升级**：本文件缺失 / 为空 / 解析失败时，[`TraeApiKeyStore::load`] 会回落
//!    读取 `settings.json` 的 `apiKey`（字符串），合成一条 legacy 记录
//!    （`id = "legacy"`，`name = "旧版 Key（升级迁移）"`）。
//!    **`variant` 必须是 [`TraeVariant::default`]（= TraeWork）**：升级前的网关只从
//!    TraeWork 池选号（`account::entries()` = `entries_for(TraeWork)`），legacy Key 归
//!    TraeWork 才能让升级前后路由到同一个池、行为逐字节一致。若归到别的变体，
//!    老用户升级后所有调用会撞上「另一个池为空」。
//! 2. **不改写用户文件**：兼容读 `settings.json` 的 `apiKey`，但**绝不**删除或改写它
//!    （[`super::super::trae::settings`] 的 `apiKey` 字段保留为兼容读）。
//!    惰性物化（create / revoke / delete 时连同 legacy 一并落盘）保证「新建一把 Key」
//!    不会让旧 Key 失效。
//!
//! ## 序列化口径（前端能读对归属列的前提）
//!
//! `TraeVariant` 枚举**派生的** serde 是 `#[serde(rename_all = "lowercase")]`，会得到
//! `"traework"` / `"traecn"`，**不匹配**前端 `TraeVariantId = "trae_work" | "trae_cn"`。
//! 因此记录里的 `variant` 用 [`TraeVariant::as_str`]（→ `"trae_work"` / `"trae_cn"`）
//! 序列化、用 [`TraeVariant::parse`] 反序列化 —— **绝不**用派生 Serialize。
//! 未知值反序列化回落到 [`TraeVariant::default`]，保持宽容失败方向。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use buddy_switch_core::modules::config as core_config;

/* ★ issue #76：策略类型**直接复用** WorkBuddy 侧那一套（`crate::apikey`）。
两条产品线的 Key 策略语义完全相同（配额单位/周期、账号白名单），各定义一份
必然漂移 —— 前端也只有一套 `KeyPolicy` 类型。 */
use crate::apikey::{
    key_taken_message, resolve_key_plaintext, KeyPolicy, QuotaMode, QuotaPeriod, QuotaUnit,
    ScopeMode,
};
use buddy_switch_core::modules::trae::{settings as trae_settings, variant::TraeVariant};

use crate::apikey::{constant_time_eq, sha256_hex};

/// legacy 合成记录的固定 id（升级迁移的旧 Key 只可能有一条）。
const LEGACY_ID: &str = "legacy";

/// 序列化 `TraeVariant` 为 `as_str()`（`"trae_work"` / `"trae_cn"`）。
///
/// 供 `#[serde(serialize_with = "variant_as_str")]` 使用；**不要**改用派生实现。
fn variant_as_str<S>(variant: &TraeVariant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(variant.as_str())
}

/// 反序列化 `variant` 键：值缺失时由 `#[serde(default)]` 兜底，未知值回落默认变体。
fn variant_from_str<'de, D>(deserializer: D) -> Result<TraeVariant, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(TraeVariant::parse(&raw).unwrap_or_default())
}

/// 单条 Trae API Key。**含归属产品线**；`plaintext` 仅在用户选择「保存到本软件」后有值，旧记录为 `None`。
///
/// 线上/磁盘形状为 camelCase（`createdAt` / `revokedAt` / `lastUsedAt`），
/// 前端类型 `TraeApiKeyRecord` 逐字对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraeApiKeyRecord {
    pub id: String,
    pub name: String,
    /// 归属产品线；缺省 = [`TraeVariant::default`]（TraeWork）。
    ///
    /// 序列化用 [`TraeVariant::as_str`] 得 `"trae_work"` / `"trae_cn"`（前端同形）。
    #[serde(
        default,
        serialize_with = "variant_as_str",
        deserialize_with = "variant_from_str"
    )]
    pub variant: TraeVariant,
    /// 前缀（脱敏展示用），如 `sk-trae-a1b2`（新 Key）或 `sk-trae-0123`（legacy）。
    ///
    /// ⚠️ **不含省略号**：展示层统一按 `prefix••••••••suffix` 拼接（见
    /// [`legacy_prefix`] 的说明）—— 这里若自带 `…` 会拼出双省略号。
    pub prefix: String,
    /// ★ 尾部 4 位（如 `9f3c`）。列表显示 `prefix••••••••suffix`。
    ///
    /// 与 WorkBuddy 侧**同形**（`apikey.rs` 的 `ApiKeyRecord::suffix`，E3）：
    /// 只靠前缀，用户无法确认「到底是哪一把」（同时建两把时前缀可能只差一两位）。
    /// 4 位 + 前缀 4 位远不足以还原 32 位 secret ⇒ 不构成泄露。
    ///
    /// 旧台账 / 短 legacy Key 无此值 ⇒ 空串 ⇒ 展示层**省略尾部**（不报错、不补造）。
    #[serde(default)]
    pub suffix: String,
    /// `sha256(hex)` of 完整明文 Key。
    pub hash: String,
    /// 完整明文；旧记录 / legacy 缺字段时为 `None`。
    #[serde(default)]
    pub plaintext: Option<String>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    #[serde(default)]
    pub last_used_at: Option<i64>,
    /* ── 策略（issue #76）—— 与 WorkBuddy 侧**同一组枚举**（`crate::apikey`），
    全 `#[serde(default)]` ⇒ 老台账零迁移（读出来 = 改造前行为：不限额 + 全部账号）。 ── */
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
    /// 允许访问的账号 uid（`scope_mode == Selected` 时有效）。
    #[serde(default)]
    pub scope_accounts: Vec<String>,
    /// 允许访问的**分组** id（与 `scope_accounts` 取并集）。
    #[serde(default)]
    pub scope_groups: Vec<String>,
}

impl TraeApiKeyRecord {
    /// 列表视图（**不含 hash**，含可空 `plaintext`）。
    pub fn masked(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "variant": self.variant.as_str(),
            "prefix": self.prefix,
            "suffix": self.suffix,
            "plaintext": self.plaintext,
            "createdAt": self.created_at,
            "revokedAt": self.revoked_at,
            "revoked": self.revoked_at.is_some(),
            "lastUsedAt": self.last_used_at,
            /* 策略七件套（issue #76）—— 与 WB 侧同形，前端共用一套构件。 */
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
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

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
}

/// legacy Key 的**展示前缀**：前 12 个字符（不足则全部）。
///
/// ## 为什么不直接用 `super::mask_api_key`
///
/// 那个函数返回的是**一整个脱敏串**（`前12位…末4位`），而展示层统一按
/// `prefix••••••••suffix` 拼接 —— 两处各带一个省略号会拼出
/// `sk-trae-0123…cdef••••••••cdef`。拆成前缀 / 后缀两段之后，legacy 记录与
/// 新 Key 走**同一条**展示规则（`trae-api-key-table.tsx`），信息量不丢。
///
/// 按 `chars` 切而不是字节：legacy Key 由用户从 `settings.json` 手填，
/// 未必是纯 ASCII（字节切片会在多字节边界 panic）。
fn legacy_prefix(key: &str) -> String {
    key.trim().chars().take(12).collect()
}

/// legacy Key 的**展示后缀**：末 4 个字符。
///
/// 与 `super::mask_api_key` 的短串分支一致：长度 ≤ 12 时返回空串
/// （此时前缀已覆盖整把 Key，再给后缀等于把整串拼两遍）。
fn legacy_suffix(key: &str) -> String {
    let trimmed = key.trim();
    let total = trimmed.chars().count();
    if total <= 12 {
        return String::new();
    }
    trimmed.chars().skip(total - 4).collect()
}

/// 读写 `api_gateway_keys.json` 的多 Key 存储。
///
/// **无进程内缓存**：每次操作都读盘，因此两份实例（独立监听 / 宿主路由）创建即可见、
/// 吊销即时生效。
pub struct TraeApiKeyStore {
    path: PathBuf,
}

impl TraeApiKeyStore {
    /// 新建存储（不读取文件；每次操作实时读盘）。
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// 存储文件路径（供诊断/展示）。
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// 从 `settings.json` 的 `apiKey` 合成 legacy 记录；为空则 `None`。
    ///
    /// `variant` 固定 [`TraeVariant::default`]（TraeWork）——见模块级「零回归前提 1」。
    fn legacy_record() -> Option<TraeApiKeyRecord> {
        let key = trae_settings::load().api_key;
        let trimmed = key.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(TraeApiKeyRecord {
            id: LEGACY_ID.to_string(),
            name: "旧版 Key（升级迁移）".to_string(),
            variant: TraeVariant::default(),
            prefix: legacy_prefix(trimmed),
            suffix: legacy_suffix(trimmed),
            hash: sha256_hex(trimmed),
            // legacy 属「旧记录」：不把 settings.json 里的明文复制进新台账 ⇒ 眼睛不显示。
            plaintext: None,
            created_at: 0,
            revoked_at: None,
            last_used_at: None,
            // legacy 合成的 Key 没有策略 ⇒ 默认「不限额 + 全部账号」（改造前行为）。
            quota_mode: QuotaMode::Unlimited,
            quota_limit: 0,
            quota_unit: QuotaUnit::Token,
            quota_period: QuotaPeriod::None,
            scope_mode: ScopeMode::All,
            scope_accounts: Vec::new(),
            scope_groups: Vec::new(),
        })
    }

    /// 读取 Key 列表。
    ///
    /// 先读 `api_gateway_keys.json`；**若文件缺失 / 为空 / 解析失败**，
    /// 回落 `settings::load().api_key` 合成一条 legacy 记录（`variant = TraeWork`）。
    /// 调用方无法绕过回落——这保证了多 Key 化不会让老用户升级后全量 401。
    pub fn load(&self) -> Vec<TraeApiKeyRecord> {
        let records: Vec<TraeApiKeyRecord> = match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str::<Vec<TraeApiKeyRecord>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        if !records.is_empty() {
            return records;
        }
        match Self::legacy_record() {
            Some(record) => vec![record],
            None => Vec::new(),
        }
    }

    /// 原子写回文件（调用方保证 `records` 已含需要物化的 legacy 记录）。
    fn save(&self, records: &[TraeApiKeyRecord]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(records).map_err(|error| error.to_string())?;
        core_config::atomic_write(&self.path, &content).map_err(|error| error.to_string())
    }

    /// 生成 Key：明文 `sk-trae-` + 32 位 hex；返回 `(记录, 明文)`。
    /// 保留两参签名 ⇒ 存量测试 / e2e 调用点零改动（等价于「不自定义密钥」）。
    pub fn create(&self, name: String, variant: TraeVariant) -> (TraeApiKeyRecord, String) {
        self.create_with_key(name, variant, None)
            // `None` ⇒ 走自动生成，查重必返回 `Ok`。
            .expect("自动生成的 Key 不会与存量重复")
    }

    /// 自定义密钥的**创建前查重**（与 WorkBuddy 侧同一规则）：`sha256(明文)` 不得与
    /// 台账里任何一条记录相同；已吊销的记录也算占用 —— 那是同一把密钥的历史，
    /// 正确动作是 [`Self::restore`]。
    ///
    /// 注意 `load()` 会带出**合成的 legacy 记录**（升级前那把 Key）⇒ 老 Key 无法被
    /// 重复创建，用户得先删掉旧记录。
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

    /// 生成 / 接收 Key；返回 `(记录, 明文)`。
    ///
    /// `custom_key` 非空 ⇒ 原样使用该密钥；为空 ⇒ 按原规则生成。
    /// 写回时先 `load()`（可能含合成的 legacy 记录）再 `save()`，于是 legacy Key
    /// **一并物化落盘** —— 新建 Key 不会让升级前那把 Key 失效。
    ///
    /// 自定义密钥**先查重**（[`Self::ensure_custom_key_available`]）：同 hash 只允许一条记录。
    ///
    /// ★ 2026-11-04：**创建时不落盘明文**（`plaintext: None`），
    ///   改由用户点「保存到本软件」时走 [`Self::save_plaintext`]。
    pub fn create_with_key(
        &self,
        name: String,
        variant: TraeVariant,
        custom_key: Option<String>,
    ) -> Result<(TraeApiKeyRecord, String), String> {
        self.ensure_custom_key_available(custom_key.as_deref())?;
        let (plaintext, prefix, suffix) = resolve_key_plaintext(
            custom_key,
            || format!("sk-trae-{}", uuid::Uuid::new_v4().simple()),
            "sk-trae-",
        );
        let record = TraeApiKeyRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            variant,
            prefix,
            suffix,
            hash: sha256_hex(&plaintext),
            plaintext: None,
            created_at: core_config::now_ms(),
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
        if let Err(error) = self.save(&records) {
            eprintln!("[trae-gateway] 保存 API Key 失败: {error}");
        }
        Ok((record, plaintext))
    }

    /// 把「本次创建返回的明文」写入台账（用户点「保存到本软件」才走这里）。
    ///
    /// 与 WorkBuddy 侧同一规则：id 必须存在、`sha256(明文)` 必须等于 `hash`、已有明文幂等不覆盖。
    /// 写回同样先 `load()` 再 `save()` ⇒ 合成的 legacy 记录会被一并物化（与 `create` 同行为）。
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

    /// 常量时间比较哈希；返回有效记录（携带 `variant`）。无效 / 不存在 / 已吊销返回 `None`。
    pub fn verify(&self, presented: &str) -> Option<TraeApiKeyRecord> {
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

    /// 吊销（置 `revoked_at`，不物理删除）。首次写盘即把 legacy 一并物化。
    /// 更新策略（配额 + 账号范围）。**不动** id / hash / 吊销态 / variant。
    ///
    /// 与 WB 侧 `ApiKeyStore::update_policy` **同一套规则**（含 `KeyPolicy::normalized`
    /// 的矛盾收敛）—— 复用同一个归一函数，不各写一份。
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

    /// 撤销吊销（清 `revoked_at`，Key **重新可用**），不物理删除。
    ///
    /// 与 WorkBuddy 侧 [`crate::apikey::ApiKeyStore::restore`] **同构同语义**：
    /// 只接受**已吊销**的 Key；不存在 / 未吊销一律 `Err`（静默成功会让用户误判状态）。
    ///
    /// ⚠️ 与 `delete` 一样走 `load()`：若台账为空则拿到的是**合成的 legacy 记录**。
    /// legacy 记录 `revoked_at == None` ⇒ 对它调 `restore` 必然落到「未吊销」，**不会**
    /// 顺手把 legacy 物化落盘（`save` 只在通过闸门之后才执行）—— 这是刻意的：
    /// 一次失败的读操作不该改写用户的文件。
    pub fn restore(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let target_revoked = records
            .iter()
            .find(|record| record.id == id)
            .map(TraeApiKeyRecord::is_revoked);
        match target_revoked {
            None => return Err("API Key 不存在".to_string()),
            Some(false) => return Err("该 API Key 未吊销，无需恢复".to_string()),
            Some(true) => {}
        }
        for record in records.iter_mut() {
            if record.id == id {
                record.revoked_at = None;
                break;
            }
        }
        self.save(&records)
    }

    /// 物理删除（仅允许删除**已吊销**的 Key）。
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let target_revoked = records
            .iter()
            .find(|record| record.id == id)
            .map(TraeApiKeyRecord::is_revoked);
        match target_revoked {
            None => return Err("API Key 不存在".to_string()),
            Some(false) => return Err("请先吊销该 API Key 再删除".to_string()),
            Some(true) => {}
        }
        records.retain(|record| record.id != id);
        self.save(&records)
    }

    /// 列出全部 Key（含已吊销）。
    pub fn list(&self) -> Vec<TraeApiKeyRecord> {
        self.load()
    }
}

/// 两条通道共用：`list` 的响应形状（**消除形状漂移**，见 `handlers.rs` 单点构造原则）。
///
/// 返回 `{ "keys": [masked…] }`。
pub fn list_response(store: &TraeApiKeyStore) -> Value {
    let keys: Vec<Value> = store.list().iter().map(TraeApiKeyRecord::masked).collect();
    json!({ "keys": keys })
}

/// 两条通道共用：`create` 的响应形状。保留三参签名供存量调用点使用。
pub fn create_response(store: &TraeApiKeyStore, name: String, variant: TraeVariant) -> Value {
    create_response_with_key(store, name, variant, None)
}

/// 创建成功的响应体（**形状唯一来源**，四条产品线同形）。
fn created_payload(record: &TraeApiKeyRecord, plaintext: String) -> Value {
    json!({ "ok": true, "key": plaintext, "record": record.masked() })
}

/// `create` 的 **`Result` 版**响应：`Err(message)` 交给**调用方**决定通道语义。
///
/// 为什么需要它：HTTP 通道要把「自定义密钥撞车」映射成 `400`（与另三条产品线一致，
/// `WB` / `Qoder` / `Unified` 的 handler 都是 `Err ⇒ json_err(…, BAD_REQUEST)`），
/// 而 IPC 通道没有状态码、只能把错误塞进 body。若本函数直接返回 `Value`，
/// HTTP 侧就只剩「200 + `{ok:false}`」一条路 ⇒ 同族接口出现两种错误码。
///
/// 因此：**HTTP** 用本函数（`Ok ⇒ json_ok`，`Err ⇒ json_err(400)`）；
/// **IPC** 用 [`create_response_with_key`]（错误折进 `{ok:false,error}`）。
/// 失败时**不落盘**（查重在 `create_with_key` 入口）。
pub fn create_result_with_key(
    store: &TraeApiKeyStore,
    name: String,
    variant: TraeVariant,
    custom_key: Option<String>,
) -> Result<Value, String> {
    let (record, plaintext) = store.create_with_key(name, variant, custom_key)?;
    Ok(created_payload(&record, plaintext))
}

/// 两条通道共用：`create` 的响应形状，支持自定义密钥。
///
/// 返回 `{ "ok": true, "key": "<明文>", "record": masked }`；
/// 自定义密钥撞车 ⇒ `{ "ok": false, "error": "…" }`（**不落盘**，前端原样显示 error）。
///
/// ★ 这是 **IPC（桌面端）** 的口径（无 HTTP 状态码可用）。webui 走 HTTP，
///   用 [`create_result_with_key`] 由 handler 映射成 `400`。
pub fn create_response_with_key(
    store: &TraeApiKeyStore,
    name: String,
    variant: TraeVariant,
    custom_key: Option<String>,
) -> Value {
    match create_result_with_key(store, name, variant, custom_key) {
        Ok(payload) => payload,
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /* `mask_api_key` 现在**只**被测试引用（生产路径改用 `legacy_prefix` / `legacy_suffix`
    两段式，见那里的说明）⇒ import 放在测试模块里，避免 lib 构建出现 unused 警告。 */
    use super::super::mask_api_key;
    use std::sync::{Mutex, MutexGuard};

    /// 串行化所有改 `BUDDY_SWITCH_HOME` 的用例（env 是进程级全局状态）。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// 把 home 指向隔离目录，并在 drop 时恢复 env 与清理目录。
    ///
    /// 必须隔离 home —— 否则会读到这台机器上真实的
    /// `~/.buddy-switch/trae/settings.json`，测试结果随环境变化。
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        previous_new: Option<std::ffi::OsString>,
        home: PathBuf,
    }

    impl EnvGuard {
        fn set(home: PathBuf) -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous_new = std::env::var_os("BUDDY_SWITCH_HOME");
            std::env::set_var("BUDDY_SWITCH_HOME", &home);
            Self {
                _lock: lock,
                previous_new,
                home,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous_new.take() {
                Some(value) => std::env::set_var("BUDDY_SWITCH_HOME", value),
                None => std::env::remove_var("BUDDY_SWITCH_HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    /// 建一个隔离 home 并返回 `(guard, store)`；store 指向隔离目录下的
    /// `api_gateway_keys.json`。
    fn isolated(tag: &str) -> (EnvGuard, TraeApiKeyStore) {
        let home = std::env::temp_dir().join(format!(
            "trae-apikey-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".buddy-switch").join("trae")).expect("创建隔离目录");
        let guard = EnvGuard::set(home.clone());
        let store = TraeApiKeyStore::new(
            home.join(".buddy-switch")
                .join("trae")
                .join("api_gateway_keys.json"),
        );
        (guard, store)
    }

    /// 往隔离 home 的 `settings.json` 写入一个 legacy `apiKey`。
    fn write_settings_api_key(home: &std::path::Path, key: &str) {
        let file = home
            .join(".buddy-switch")
            .join("trae")
            .join("settings.json");
        std::fs::write(file, serde_json::json!({ "apiKey": key }).to_string())
            .expect("写入 settings.json");
    }

    #[test]
    fn legacy_settings_key_verifies_and_is_traework() {
        let (guard, store) = isolated("legacy");
        let legacy_key = "sk-trae-0123456789abcdef0123456789abcdef";
        write_settings_api_key(&guard.home, legacy_key);

        // 键库文件尚不存在 —— load() 必须回落 settings.apiKey。
        assert!(!store.path().exists(), "键库文件此时不应存在");
        let record = store
            .verify(legacy_key)
            .expect("旧 settings.apiKey 必须能 verify（否则升级即全量 401）");
        assert_eq!(record.id, "legacy");
        assert_eq!(record.name, "旧版 Key（升级迁移）");
        assert_eq!(
            record.variant,
            TraeVariant::default(),
            // ★ issue #28：默认变体已改为**合并版**（`TraeMerged`）——
            //   老默认 `TraeWork` 指向已被官方合并卸载的客户端。
            "legacy 记录必须归默认变体（升级前网关只从那个池选号）"
        );
        // ★ legacy 的 prefix / suffix 是**分段**的（见 `legacy_prefix` / `legacy_suffix`）：
        // 省略号由展示层统一补，这里若带 `…` 会拼出双省略号。
        assert_eq!(record.prefix, "sk-trae-0123");
        assert_eq!(record.suffix, "cdef");
        // 分段后**信息量不丢**：与完整脱敏串（`mask_api_key`）逐段一致。
        let masked = mask_api_key(legacy_key);
        assert!(
            masked.starts_with(&record.prefix),
            "legacy 前缀必须与脱敏串同源：{masked}"
        );
        assert!(
            masked.ends_with(&record.suffix),
            "legacy 后缀必须与脱敏串同源：{masked}"
        );
        assert_eq!(record.hash, sha256_hex(legacy_key));
    }

    /// ★ legacy 的展示串与「新 Key 的展示规则」逐字等价 —— 两段拼起来就是用户看到的东西。
    ///
    /// ⚠️ 两段各自 `isolated()` 会**死锁**：`EnvGuard` 持 [`ENV_LOCK`]，
    /// 同一线程连续取两次必阻塞（本仓踩过「guard 自锁死」）。故第一段包在块作用域里，
    /// 让 guard 在进入第二段之前 drop。
    #[test]
    fn legacy_mask_uses_the_same_display_rule_as_new_keys() {
        {
            let (guard, store) = isolated("legacy-display");
            let legacy_key = "sk-trae-0123456789abcdef0123456789abcdef";
            write_settings_api_key(&guard.home, legacy_key);

            let record = store.verify(legacy_key).unwrap();
            assert_eq!(
                format!("{}••••••••{}", record.prefix, record.suffix),
                "sk-trae-0123••••••••cdef"
            );
        }

        // 短 legacy Key：前缀已覆盖整串 ⇒ 后缀必须为空（否则等于把整串拼两遍）。
        let (guard, store) = isolated("legacy-short");
        write_settings_api_key(&guard.home, "short");
        let short = store.verify("short").unwrap();
        assert_eq!(short.prefix, "short");
        assert_eq!(short.suffix, "", "≤12 字符的 legacy Key 不该有后缀");
    }

    /// ★ 新 Key 的 `suffix` 恰好 4 位、且**就是明文的末 4 位**（供用户核对）。
    ///
    /// 与 WorkBuddy 侧 `apikey.rs` 的同类断言同形；4 位不足以还原 32 位 secret，
    /// 所以「明文以 suffix 结尾」是**必然**而非泄露。
    #[test]
    fn new_key_suffix_is_the_last_four_of_the_plaintext() {
        let (_guard, store) = isolated("suffix");
        let (record, plaintext) = store.create("suffix".to_string(), TraeVariant::Trae);
        assert_eq!(record.suffix.chars().count(), 4, "suffix 应恰好 4 位");
        assert!(
            plaintext.ends_with(&record.suffix),
            "suffix 就是明文末 4 位（供用户核对）"
        );
        // 脱敏出口必须带上它 —— 否则前端拼不出 `prefix••••••••suffix`。
        assert_eq!(record.masked()["suffix"], json!(record.suffix));
    }

    /// 自定义密钥查重：同 `hash` 只能有一条记录（**已吊销的也算占用**），被拒时不落盘。
    ///
    /// 变异：删掉 `create_with_key` 里的 `ensure_custom_key_available(...)?` ⇒ 本测试红。
    #[test]
    fn duplicate_custom_key_is_rejected() {
        let (_guard, store) = isolated("dup");
        let key = "dup-trae-key-0001";
        let (record, _) = store
            .create_with_key("甲".to_string(), TraeVariant::default(), Some(key.into()))
            .expect("首次创建");

        let error = store
            .create_with_key("乙".to_string(), TraeVariant::default(), Some(key.into()))
            .expect_err("重复的自定义密钥必须被拒");
        assert!(error.contains("甲"), "要点名占用者: {error}");
        assert_eq!(store.load().len(), 1, "被拒时不得落盘");

        // HTTP 通道走 `create_result_with_key`（handler 负责映射 400）⇒ 它必须**同样**拒绝，
        // 否则 webui 会 200 成功但没建出记录。变异：让它改调 `create_response_with_key`
        // 吞掉错误 ⇒ 本断言红。
        let error = create_result_with_key(
            &store,
            "丁".to_string(),
            TraeVariant::default(),
            Some(key.into()),
        )
        .expect_err("Result 版也必须拒绝重复");
        assert!(
            error.contains("甲"),
            "Result 版的提示语要与 store 一致: {error}"
        );
        assert_eq!(store.load().len(), 1);

        store.revoke(&record.id).expect("吊销");
        let error = store
            .create_with_key("丙".to_string(), TraeVariant::default(), Some(key.into()))
            .expect_err("已吊销也占用");
        assert!(error.contains("恢复"), "应提示走恢复: {error}");
        assert_eq!(store.load().len(), 1);
    }

    #[test]
    fn creating_a_new_key_materializes_legacy_and_keeps_it_valid() {
        let (guard, store) = isolated("materialize");
        let legacy_key = "sk-trae-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        write_settings_api_key(&guard.home, legacy_key);

        let (_record, new_plaintext) = store.create("新 Key".to_string(), TraeVariant::Trae);

        // 旧 Key 仍有效（legacy 一并物化落盘）。
        assert!(
            store.verify(legacy_key).is_some(),
            "新建 Key 后旧 Key 不得失效"
        );
        assert!(
            store.verify(&new_plaintext).is_some(),
            "新 Key 必须立即可用"
        );

        // 磁盘上应同时存在 legacy 记录与新记录。
        let on_disk = std::fs::read_to_string(store.path()).unwrap();
        assert!(
            on_disk.contains("legacy"),
            "legacy 记录必须被物化: {on_disk}"
        );
        assert!(!on_disk.contains(legacy_key), "磁盘不得出现明文：{on_disk}");
    }

    #[test]
    fn revoking_legacy_makes_it_invalid() {
        let (guard, store) = isolated("revoke-legacy");
        let legacy_key = "sk-trae-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        write_settings_api_key(&guard.home, legacy_key);
        assert!(store.verify(legacy_key).is_some());

        store.revoke("legacy").expect("吊销 legacy 记录");
        assert!(
            store.verify(legacy_key).is_none(),
            "吊销后旧 Key 必须失效（以文件为准）"
        );
        assert!(store
            .list()
            .iter()
            .any(|record| record.id == "legacy" && record.is_revoked()));
    }

    /// ★ 需求变更（第二次）：**创建时不落盘**，用户点「保存到本软件」才写。
    #[test]
    fn new_key_plaintext_touches_disk_only_after_save() {
        let (_guard, store) = isolated("plaintext");
        let (record, plaintext) = store.create("k".to_string(), TraeVariant::TraeWork);
        let on_disk = std::fs::read_to_string(store.path()).unwrap();
        assert!(
            !on_disk.contains(&plaintext),
            "创建时不得把明文写进台账：{on_disk}"
        );
        assert!(
            plaintext.starts_with("sk-trae-") && plaintext.len() == "sk-trae-".len() + 32,
            "明文形态应为 sk-trae-<32hex>：{plaintext}"
        );

        store.save_plaintext(&record.id, &plaintext).unwrap();
        let on_disk = std::fs::read_to_string(store.path()).unwrap();
        assert!(on_disk.contains(&plaintext), "保存后明文应落盘：{on_disk}");
    }

    #[test]
    fn variant_serializes_to_underscore_ids() {
        let (_guard, store) = isolated("variant-ser");
        let (work, _) = store.create("工作".to_string(), TraeVariant::TraeWork);
        let (cn, _) = store.create("国内".to_string(), TraeVariant::Trae);
        let on_disk = std::fs::read_to_string(store.path()).unwrap();

        assert_eq!(work.variant, TraeVariant::TraeWork);
        assert_eq!(cn.variant, TraeVariant::Trae);
        // 必须是 as_str() 的下划线形态，而不是派生 serde 的 "traework"/"traecn"。
        assert!(on_disk.contains("\"trae_work\""), "{on_disk}");
        assert!(on_disk.contains("\"trae_cn\""), "{on_disk}");
        assert!(
            !on_disk.contains("\"traework\"") && !on_disk.contains("\"traecn\""),
            "不得使用派生 serde 的小写形态：{on_disk}"
        );

        // 读回后 variant 不丢。
        let reloaded = store.list();
        assert_eq!(
            reloaded.iter().find(|r| r.id == work.id).unwrap().variant,
            TraeVariant::TraeWork
        );
        assert_eq!(
            reloaded.iter().find(|r| r.id == cn.id).unwrap().variant,
            TraeVariant::Trae
        );
    }

    #[test]
    fn missing_variant_key_defaults_to_traework() {
        // 老键库文件里没有 variant 键 → 读出即 TraeWork（零回归前提 1）。
        let (guard, store) = isolated("missing-variant");
        let legacy_hash = sha256_hex("sk-trae-cccccccccccccccccccccccccccccccc");
        let raw = serde_json::json!([{
            "id": "old",
            "name": "旧记录",
            "prefix": "sk-trae-cccc",
            "hash": legacy_hash,
            "createdAt": 1,
            "revokedAt": null,
        }]);
        std::fs::write(store.path(), raw.to_string()).unwrap();
        // 隔离目录里没有 settings.json，确保走的是文件而非 legacy 回落。
        let _ = guard.home;
        let record = store
            .verify("sk-trae-cccccccccccccccccccccccccccccccc")
            .unwrap();
        assert_eq!(
            record.variant,
            TraeVariant::default(),
            "缺 variant 字段须回落默认变体"
        );
    }

    #[test]
    fn unknown_variant_falls_back_to_default() {
        let (_guard, store) = isolated("unknown-variant");
        let raw = serde_json::json!([{
            "id": "x",
            "name": "x",
            "variant": "doubao",
            "prefix": "sk-trae-0000",
            "hash": sha256_hex("k"),
            "createdAt": 1,
            "revokedAt": null,
        }]);
        std::fs::write(store.path(), raw.to_string()).unwrap();
        let record = &store.list()[0];
        assert_eq!(record.variant, TraeVariant::default(), "未知变体应回落默认");
    }

    #[test]
    fn verify_rejects_one_byte_and_wrong_length() {
        let (_guard, store) = isolated("verify");
        let (_record, plaintext) = store.create("k".to_string(), TraeVariant::Trae);
        let mut different = plaintext.clone().into_bytes();
        let last = different.len() - 1;
        different[last] = if different[last] == b'0' { b'1' } else { b'0' };
        assert!(store
            .verify(&String::from_utf8(different).unwrap())
            .is_none());
        assert!(store.verify("x").is_none());
        assert!(store.verify("").is_none());
    }

    #[test]
    fn delete_requires_revoke_first() {
        let (_guard, store) = isolated("delete");
        let (record, _) = store.create("k".to_string(), TraeVariant::TraeWork);
        assert!(store.delete(&record.id).is_err(), "未吊销不得删除");
        store.revoke(&record.id).unwrap();
        store.delete(&record.id).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn revoke_and_delete_unknown_are_rejected() {
        let (_guard, store) = isolated("unknown-op");
        assert_eq!(store.revoke("missing"), Err("API Key 不存在".to_string()));
        assert_eq!(store.delete("missing"), Err("API Key 不存在".to_string()));
        assert_eq!(store.restore("missing"), Err("API Key 不存在".to_string()));
    }

    #[test]
    fn restore_revives_revoked_and_rejects_active() {
        let (_guard, store) = isolated("restore");
        let (record, plaintext) = store.create("恢复".to_string(), TraeVariant::Trae);

        // 未吊销 ⇒ 无需恢复（响亮失败，不静默成功）。
        assert_eq!(
            store.restore(&record.id),
            Err("该 API Key 未吊销，无需恢复".to_string())
        );

        store.revoke(&record.id).unwrap();
        assert!(store.verify(&plaintext).is_none(), "已吊销 Key 应拒绝");

        store.restore(&record.id).unwrap();
        // ★ 判据锚在**行为**上：恢复 = Key 重新可用（只看 `revoked_at` 会永远绿）。
        assert!(
            store.verify(&plaintext).is_some(),
            "恢复后 Key 必须重新可用"
        );
        let after = store
            .list()
            .into_iter()
            .find(|item| item.id == record.id)
            .expect("恢复不应删除记录");
        assert!(!after.is_revoked());
        assert_eq!(after.variant, record.variant, "恢复不得改动归属程序位");
        assert_eq!(after.hash, record.hash, "恢复不得改动 hash");
    }

    /// ★ 恢复一把不得连带恢复另一把（错的实现常写成「无条件全清 `revoked_at`」）。
    #[test]
    fn restore_touches_only_the_target_key() {
        let (_guard, store) = isolated("restore-scope");
        let (first, first_plaintext) = store.create("first".to_string(), TraeVariant::Trae);
        let (second, second_plaintext) = store.create("second".to_string(), TraeVariant::TraeWork);
        store.revoke(&first.id).unwrap();
        store.revoke(&second.id).unwrap();

        store.restore(&first.id).unwrap();
        assert!(
            store.verify(&first_plaintext).is_some(),
            "被恢复的 Key 应可用"
        );
        assert!(
            store.verify(&second_plaintext).is_none(),
            "另一把必须仍是吊销态"
        );
    }

    /// ★ legacy **仅在内存里合成**：一次失败的 `restore` 不得把 legacy 顺手物化落盘。
    ///
    /// 与 `revoke` / `delete`（通过闸门后必然 `save`）不同，恢复失败的路径**不能在
    /// 用户磁盘上留下副作用** —— 否则用户只是点错一次，键库文件就被凭空创建。
    #[test]
    fn restore_on_legacy_only_state_does_not_materialize_the_file() {
        let (guard, store) = isolated("restore-legacy-nofile");
        write_settings_api_key(&guard.home, "sk-trae-dddddddddddddddddddddddddddddddd");
        assert!(!store.path().exists(), "键库文件此时不应存在");

        assert_eq!(
            store.restore("legacy"),
            Err("该 API Key 未吊销，无需恢复".to_string())
        );
        assert!(
            !store.path().exists(),
            "失败的恢复不得创建 / 改写用户的键库文件"
        );
    }

    #[test]
    fn legacy_ledger_without_policy_fields_reads_as_before() {
        // 老台账：无策略七件套 ⇒ 默认「不限额 + 全部账号」= 改造前行为。
        // ★ Trae 台账落盘是 **camelCase**（struct 上有 `rename_all = "camelCase"`，
        //   与 WB 侧相反 —— WB 落 snake_case、只在 masked 里转 camelCase）。
        //   用错形态测的就不是真台账。
        let legacy = serde_json::json!([{
            "id": "old", "name": "old", "variant": "trae_work", "prefix": "sk-trae-a1b2",
            "suffix": "9f3c", "hash": "h", "createdAt": 1, "revokedAt": null,
            "lastUsedAt": null
        }]);
        let records: Vec<TraeApiKeyRecord> = serde_json::from_value(legacy).unwrap();
        let record = &records[0];
        assert_eq!(record.quota_mode, QuotaMode::Unlimited);
        assert_eq!(record.scope_mode, ScopeMode::All);
        assert!(record.scope_accounts.is_empty());
        assert_eq!(
            crate::apikey::quota_verdict_for_policy(&record.policy(), u64::MAX),
            crate::apikey::QuotaVerdict::Ok,
            "老 Key 不得被限额"
        );
    }

    #[test]
    fn update_policy_writes_and_normalizes_like_the_wb_side() {
        let (_guard, store) = isolated("policy");
        let (record, _) = store.create("p".to_string(), TraeVariant::default());

        // 空白名单 ⇒ 拒绝（与 WB 侧同一规则）。
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
                    quota_limit: 500,
                    quota_unit: QuotaUnit::Token,
                    quota_period: QuotaPeriod::Monthly,
                    scope_mode: ScopeMode::Selected,
                    scope_accounts: vec!["u1".into()],
                    scope_groups: vec![],
                },
            )
            .unwrap();
        let after = store
            .list()
            .into_iter()
            .find(|r| r.id == record.id)
            .unwrap();
        assert_eq!(after.quota_limit, 500);
        assert_eq!(after.quota_period, QuotaPeriod::Monthly);
        assert_eq!(after.scope_accounts, vec!["u1".to_string()]);
        // 不动身份字段。
        assert_eq!(after.hash, record.hash);
        assert_eq!(after.variant, record.variant);

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
            .find(|r| r.id == record.id)
            .unwrap();
        assert_eq!(normalized.quota_limit, 0);
        assert!(normalized.scope_accounts.is_empty());
    }

    #[test]
    fn masked_is_explicit_non_secret_whitelist() {
        let (_guard, store) = isolated("masked");
        let (record, plaintext) = store.create("masked".to_string(), TraeVariant::Trae);
        let masked = record.masked();
        let object = masked.as_object().unwrap();
        let fields: std::collections::BTreeSet<&str> = object.keys().map(String::as_str).collect();
        let expected: std::collections::BTreeSet<&str> = [
            "id",
            "name",
            "variant",
            "prefix",
            "suffix",
            "plaintext",
            "createdAt",
            "revokedAt",
            "revoked",
            "lastUsedAt",
            // ★ issue #76：策略七件套也进掩码视图（与 WB 侧同款显式白名单）。
            //   前端共用一套 `KeyPolicy` 类型 ⇒ 两边掩码形态必须一致。
            "quotaMode",
            "quotaLimit",
            "quotaUnit",
            "quotaPeriod",
            "scopeMode",
            "scopeAccounts",
            "scopeGroups",
        ]
        .into_iter()
        .collect();
        assert_eq!(fields, expected);
        assert!(!object.contains_key("hash"));
        assert!(!masked.to_string().contains(&record.hash));
        assert!(masked["plaintext"].is_null(), "未保存前不得下发明文");
        store.save_plaintext(&record.id, &plaintext).unwrap();
        let saved = store
            .list()
            .into_iter()
            .find(|entry| entry.id == record.id)
            .unwrap()
            .masked();
        assert_eq!(saved["plaintext"], json!(plaintext));
        assert_eq!(masked["variant"], json!("trae_cn"));
        assert_eq!(masked["revoked"], json!(false));
    }

    #[test]
    fn list_response_and_create_response_shapes_are_pinned() {
        let (_guard, store) = isolated("responses");
        let list = list_response(&store);
        assert!(list.get("keys").unwrap().is_array());

        let created = create_response(&store, "新".to_string(), TraeVariant::TraeWork);
        let object = created.as_object().unwrap();
        let fields: std::collections::BTreeSet<&str> = object.keys().map(String::as_str).collect();
        let expected: std::collections::BTreeSet<&str> =
            ["ok", "key", "record"].into_iter().collect();
        assert_eq!(fields, expected);

        // list_response 必须反映刚创建的 Key。
        let list = list_response(&store);
        assert_eq!(list["keys"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn two_instances_share_state_via_disk() {
        let (_guard, store) = isolated("shared");
        let second = TraeApiKeyStore::new(store.path().to_path_buf());
        let (_record, plaintext) = store.create("shared".to_string(), TraeVariant::TraeWork);
        assert!(
            second.verify(&plaintext).is_some(),
            "另一实例应能看到新建 Key"
        );
    }

    #[test]
    fn touch_is_throttled_for_recent_use() {
        let (_guard, store) = isolated("touch");
        let (record, _) = store.create("touch".to_string(), TraeVariant::TraeWork);
        store.touch(&record.id);
        let first = store
            .list()
            .into_iter()
            .find(|item| item.id == record.id)
            .unwrap()
            .last_used_at;
        assert!(first.is_some());
        store.touch(&record.id);
        let second = store
            .list()
            .into_iter()
            .find(|item| item.id == record.id)
            .unwrap()
            .last_used_at;
        assert_eq!(second, first, "60 秒内不得重复写盘");
    }
}
