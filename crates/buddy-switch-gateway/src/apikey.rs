//! API Key 存储：**仅存 sha256 哈希 + 前缀**，明文仅在创建时返回一次。
//!
//! 落盘 `~/.buddy-switch/gateway_keys.json`。为支持「宿主合并路由」与「独立监听」
//! 两份实例共享同一批 Key，本存储**每次操作都读盘**（无进程内缓存），从而
//! 创建即可见、吊销即时生效。
//!
//! ## 每把 Key 的两项策略（issue #76，2026-10-22）
//!
//! - **用量配额**（[`QuotaMode`] / [`QuotaUnit`] / [`QuotaPeriod`] + `quota_limit`）：
//!   达到上限后**只拒绝这把 Key**的新请求（429），不影响其它 Key；
//! - **可访问账号**（[`ScopeMode`] + `scope_accounts` / `scope_groups`）：
//!   白名单是**硬约束** —— 在选号的候选过滤阶段剔除名单外账号，
//!   **全部不可用时明确报错，不回退全池**（回退 = 白名单形同虚设）。
//!
//! 判定逻辑是**纯函数**（[`quota_verdict`] / [`account_allowed`]）：
//! 单元测试不需要起监听、不需要真实请求，直接构造记录即可覆盖全部边界。

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::Region;

// ─────────────────── 密钥策略（issue #76）───────────────────

/// 配额模式：不限额 / 限额。**默认不限额** —— 与改造前行为一致，
/// 老台账零迁移（`#[serde(default)]`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QuotaMode {
    #[default]
    Unlimited,
    Limited,
}

/// 计量单位。**请求数**不是凑数选项：Trae 的业务代理（签到 / 领取等接口）
/// 没有 token、只有请求数；统一网关的一把 Key 还能同时访问 LLM 与业务命名空间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QuotaUnit {
    #[default]
    Token,
    Request,
}

/// 重置周期。`None` = 不重置（累计口径）；其余三项按**本地时区**判周期键
/// （`daily:2026-10-22` / `weekly:2026-W43` / `monthly:2026-10`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QuotaPeriod {
    #[default]
    None,
    Daily,
    Weekly,
    Monthly,
}

impl QuotaPeriod {
    /// 周期键（用于「上一周期 ⇒ 计数归零」的判定）。
    ///
    /// `None` 返回 `None` ⇒ 计数**不清空**（累计口径）。
    /// 其余按本地时区取 `now_ms()` 对应的日 / 周 / 月。
    pub fn period_key(self, now_ms: i64) -> Option<String> {
        match self {
            QuotaPeriod::None => None,
            QuotaPeriod::Daily => Some(format!("daily:{}", local_date(now_ms))),
            QuotaPeriod::Weekly => Some(format!("weekly:{}", local_iso_week(now_ms))),
            QuotaPeriod::Monthly => Some(format!("monthly:{}", &local_date(now_ms)[..7])),
        }
    }
}

/// 账号范围：全部 / 指定白名单。**默认全部**（老台账零迁移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ScopeMode {
    #[default]
    All,
    Selected,
}

/// 本地时区的 `YYYY-MM-DD`（不引第三方日期库：网关只需要日 / 周 / 月三档键，
/// 用 `now_ms` 手算足够，且避免为一个格式多拖一个依赖）。
fn local_date(now_ms: i64) -> String {
    let days = now_ms.div_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// 本地时区的 ISO 周键 `YYYY-Www`（周一为一周之始；1 月 1 日可能落在 W01 之前的那一周）。
fn local_iso_week(now_ms: i64) -> String {
    let days = now_ms.div_euclid(86_400_000);
    // 1970-01-01 是周四 ⇒ ISO 周一位移 3 天到周一。
    let weekday = (days + 3).rem_euclid(7); // 0 = 周一
    let thursday = days - weekday + 3; // 所在周的周四决定年份与周数
    let (year, _, _) = civil_from_days(thursday);
    let jan1 = days_from_civil(year, 1, 1);
    let week = (thursday - jan1) / 7 + 1;
    format!("{year:04}-W{week:02}")
}

/// Howard Hinnant 的 `days_from_civil` 逆运算（日 ↔ 年月日），无依赖、可测。
fn civil_from_days(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_from_civil(year: i64, month: u64, day: u64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (if month > 2 { month - 3 } else { month + 9 }) as u64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

/// 配额判定结果。`Exceeded` 带上四个字段 ⇒ 调用方能直接拼出「含单位与周期」的
/// 429 文案，不必再回去查记录（文案与判定同源，不会说错单位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaVerdict {
    /// 未超限（含「不限额」这一档）。
    Ok,
    Exceeded {
        used: u64,
        limit: u64,
        unit: QuotaUnit,
        period: QuotaPeriod,
    },
}

/// 纯函数：这把 Key 在「已用 `used`」时是否超限。
///
/// ★ 不限额 ⇒ 恒 `Ok`（改造前行为）；限额但 `limit == 0` ⇒ 也 `Ok`
///   （0 = 关闭该条，与护栏的「0 = 关闭」同口径，见 `qoder-quota-guard`）。
/// ★ `used >= limit` 即超限（**等于**也算）：上限是「最多用到这么多」。
pub fn quota_verdict(record: &ApiKeyRecord, used: u64) -> QuotaVerdict {
    if record.quota_mode != QuotaMode::Limited || record.quota_limit == 0 {
        return QuotaVerdict::Ok;
    }
    if used >= record.quota_limit {
        return QuotaVerdict::Exceeded {
            used,
            limit: record.quota_limit,
            unit: record.quota_unit,
            period: record.quota_period,
        };
    }
    QuotaVerdict::Ok
}

/// 纯函数：**策略版**配额判定（中继层持有的是 [`KeyPolicy`] 而非整条记录）。
///
/// 与 [`quota_verdict`] 同判据，只是数据来源不同：一个读台账记录，一个读
/// 请求带的策略快照。两处必须走同一套规则 —— 复制一份就是漂移的开始。
pub fn quota_verdict_for_policy(policy: &KeyPolicy, used: u64) -> QuotaVerdict {
    if policy.quota_mode != QuotaMode::Limited || policy.quota_limit == 0 {
        return QuotaVerdict::Ok;
    }
    if used >= policy.quota_limit {
        return QuotaVerdict::Exceeded {
            used,
            limit: policy.quota_limit,
            unit: policy.quota_unit,
            period: policy.quota_period,
        };
    }
    QuotaVerdict::Ok
}

/// 白名单判定的**唯一实现**（安全审计 P1-7）。
///
/// `All` ⇒ 放行（不改选号行为）。`Selected` ⇒ 账号 uid 或账号所属分组
/// **任一命中**即放行（名单是并集）。
///
/// ## 为什么抽成 `scope_*` 参数而不是只留 `account_allowed`
///
/// 改造前这条判定有 **4 份实现**：`account_allowed`（有测试、**零生产调用**）与
/// `relay` / `trae::routes` / `qoder::routes` 三份手写的 `allowed_uids_for`（生产在跑、
/// 零测试）。要改「uid ∪ 分组」的语义时，改那份有测试的会全绿，三份活的原封不动 ⇒
/// **三个平台行为分叉而构建全绿**。
///
/// 现在判定只此一处，`account_allowed` / `policy_allows` / `allowed_uids_for_policy`
/// 都是它的薄包装。三产品的分组**数据源**仍各不相同（WB 是账号记录上的 `groupName`、
/// Trae 是独立 membership 文件、Qoder 是账号记录上的 `group`），差异只留在「怎么取
/// `(uid, group)`」，判定不再可能分家。
pub fn scope_allows(
    scope_mode: ScopeMode,
    scope_accounts: &[String],
    scope_groups: &[String],
    account_uid: &str,
    account_group: Option<&str>,
) -> bool {
    if scope_mode != ScopeMode::Selected {
        return true;
    }
    if scope_accounts.iter().any(|uid| uid == account_uid) {
        return true;
    }
    match account_group {
        Some(group) if !group.is_empty() => scope_groups.iter().any(|g| g == group),
        _ => false,
    }
}

/// 按**Key 记录**判定（`account_allowed` 的原签名）。
pub fn account_allowed(
    record: &ApiKeyRecord,
    account_uid: &str,
    account_group: Option<&str>,
) -> bool {
    scope_allows(
        record.scope_mode,
        &record.scope_accounts,
        &record.scope_groups,
        account_uid,
        account_group,
    )
}

/// 按**Key 策略**判定（中继路径只拿得到 `KeyPolicy`）。
pub fn policy_allows(policy: &KeyPolicy, account_uid: &str, account_group: Option<&str>) -> bool {
    scope_allows(
        policy.scope_mode,
        &policy.scope_accounts,
        &policy.scope_groups,
        account_uid,
        account_group,
    )
}

/// 把 Key 策略的账号范围**展开**成 uid 集合（`None` = 全部账号，不过滤）。
///
/// `accounts` 由调用方从各自的产品线账号库取 `(uid, 分组)`；分组字段三处不同是
/// **刻意的**（见 [`scope_allows`] 的说明），判定不因此分家。
///
/// ★ 展开**只增不删**：`scope_accounts` 里显式写进去的 uid 即使账号库里已经没有，
/// 也要保留 —— 用户改完策略发现「少了一个号」是无从解释的。
pub fn allowed_uids_for_policy<I>(policy: &KeyPolicy, accounts: I) -> Option<HashSet<String>>
where
    I: IntoIterator<Item = (String, Option<String>)>,
{
    if policy.scope_mode != ScopeMode::Selected {
        return None;
    }
    let mut allowed: HashSet<String> = policy.scope_accounts.iter().cloned().collect();
    for (uid, group) in accounts {
        if scope_allows(
            policy.scope_mode,
            &policy.scope_accounts,
            &policy.scope_groups,
            &uid,
            group.as_deref(),
        ) {
            allowed.insert(uid);
        }
    }
    Some(allowed)
}

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
    /* ── 策略（issue #76）—— 全部 `#[serde(default)]` ⇒ **老台账零迁移**：
    缺字段按各自 `Default`（不限额 / token / 不重置 / 全部账号）读入，
    读出来就是「改造前的行为」，不会把老 Key 变成限额或空名单。 ── */
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
    /// 允许访问的**分组** id（与 `scope_accounts` 取并集）——
    /// 组内换号时这把 Key 的策略不用改（三条产品线都已有分组）。
    #[serde(default)]
    pub scope_groups: Vec<String>,
}

impl ApiKeyRecord {
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
            /* 策略七件套（issue #76）。★ 只放**策略**、不放「已用」——
               用量是动态账本（`key_usage`），让 `list` 每次多读一个文件不划算，
               由独立命令 `key_usage` 取（见规范 §6.2 的实现偏差说明）。 */
            "quotaMode": self.quota_mode,
            "quotaLimit": self.quota_limit,
            "quotaUnit": self.quota_unit,
            "quotaPeriod": self.quota_period,
            "scopeMode": self.scope_mode,
            "scopeAccounts": self.scope_accounts,
            "scopeGroups": self.scope_groups,
        })
    }
}

/// 策略更新入参（管理面命令的载荷）。
///
/// ★ **不含** `id`：id 从路径/参数单独给（与 `revoke(id)` 同形）——
///   把 id 混进策略体里，「改策略」和「换 id」就分不开了。
/// ★ 字段全为**值类型**（非 Option）：调用方给的就是「完整策略」，
///   不是「补丁」。缺字段由前端的抽屉保证（两个 radio 必选其一），
///   后端不猜「用户是不是想只改一半」—— 半更新的语义在 UI 上更容易出错。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPolicy {
    pub quota_mode: QuotaMode,
    pub quota_limit: u64,
    pub quota_unit: QuotaUnit,
    pub quota_period: QuotaPeriod,
    pub scope_mode: ScopeMode,
    pub scope_accounts: Vec<String>,
    pub scope_groups: Vec<String>,
}

/// **跨产品线共享的「调用者」抽象**（issue #76）。
///
/// ## 为什么需要它
///
/// 四条产品线的 Key 台账是**四套类型**（`ApiKeyRecord` / `TraeApiKeyRecord` /
/// `QoderApiKeyRecord` / `UnifiedApiKeyRecord`），但它们对中继的意义完全一样：
/// 「谁在调用（`key_id`）+ 允许他做什么（`policy`）+ 用量记到哪个账本（`usage`）」。
///
/// 中继（`chat_for_region` / `chat_for_variant`）如果按具体 Key 类型收参，
/// **统一网关就无法把请求带配额地转发下去** —— 它的 Key 是第四种类型。
/// 这正是 issue #76 里「统一网关只能做请求数口径」那个伪边界的成因：
/// 不是语义不成立，是**参数类型绑死了产品线**。
///
/// 有了 `KeyActor`：
/// - 三条产品线的 handler 用各自 Key 构造它；
/// - 统一网关用**自己的** Key 构造它，照样把配额 / 白名单 / 记账带进目标产品线的中继；
/// - 白名单展开时用 `actor.policy`，账号库按**目标产品线**读 —— uid 空间问题自然消解。
///
/// `usage` 是 `Arc`：中继可能跨 `await` / 进 `tokio::spawn`，克隆指针即可。
#[derive(Clone)]
pub struct KeyActor {
    /// 密钥 id（记账与日志的 join 键）。
    pub key_id: String,
    /// 策略（配额 + 账号范围）。**判定与展开都只看它**，不看 Key 台账。
    pub policy: KeyPolicy,
    /// 用量账本（**调用方所在产品线的**那份）。
    pub usage: std::sync::Arc<crate::key_usage::KeyUsageStore>,
    /// ★ 这个 Key **属于哪套台账**（`"wb"` / `"trae"` / `"qoder"` / `"unified"`）。
    ///
    /// 为什么必须有：统一网关把请求转发给产品线中继时，产品线的请求日志记下的
    /// `keyId` 是**网关那把 Key 的 id** —— 与产品线自己的 Key id 混在同一个字段里。
    /// 没有这个标签，产品线的「按密钥看用量」会把网关的 Key 当成一把「已删除的本地 Key」
    /// （显示成短 id），用户永远对不上。
    pub scope: &'static str,
}

impl KeyActor {
    pub fn new(
        key_id: impl Into<String>,
        policy: KeyPolicy,
        usage: std::sync::Arc<crate::key_usage::KeyUsageStore>,
        scope: &'static str,
    ) -> Self {
        Self {
            key_id: key_id.into(),
            policy,
            usage,
            scope,
        }
    }

    /// 配额判定 + 记一笔请求数。返回 `Some(文案)` = 已超限。
    ///
    /// ★ **原子**：走 `KeyUsageStore::check_and_record`（读-判-写同一把锁）。
    ///   原先「`used_for` 判 + `record_request` 记」是两步，并发下会同时通过判定，
    ///   还会因读改写竞争丢计数（详见该方法的文档）。
    ///
    /// ★ 超限时**不记这一笔**：配额是「允许多少次」，不是「尝试多少次」。
    ///   否则用户会看到 101/100 这种越界数字，且被拒的请求会持续抬高分母。
    pub fn check_quota_and_count(&self, now: i64) -> Option<String> {
        let (used, limit, unit, period) =
            self.usage
                .check_and_record(&self.key_id, &self.policy, now)?;
        Some(
            crate::error::GatewayError::KeyQuotaExceeded {
                used,
                limit,
                unit,
                period,
            }
            .message(),
        )
    }

    /// 记 token（**拿不到用量就不记** —— 写 0 样本会让配额晚触发）。
    pub fn record_tokens(&self, total: u64, now: i64) {
        if total == 0 {
            return;
        }
        self.usage
            .record_tokens(&self.key_id, total, self.policy.quota_period, now);
    }
}

impl KeyPolicy {
    /// 入库前的归一：**互相矛盾的下法治「更安全的一边」**。
    ///
    /// - `quota_mode == Unlimited` ⇒ 清空 `quota_limit`（留着旧上限，
    ///   下次切回「限额」时会**突然**拿一个过时的数生效 —— 用户早忘了它）；
    /// - `scope_mode == All` ⇒ 清空白名单（同理，切回「指定」时不该复活旧名单）；
    /// - `scope_mode == Selected` 但名单为空 ⇒ **拒绝**（空白名单 =
    ///   永远选不到号 = 静默失败；UI 上已禁保存，这里再兜一层）。
    pub fn normalized(self) -> Result<Self, String> {
        let mut policy = self;
        if policy.quota_mode == QuotaMode::Unlimited {
            policy.quota_limit = 0;
        }
        if policy.scope_mode == ScopeMode::All {
            policy.scope_accounts.clear();
            policy.scope_groups.clear();
        } else if policy.scope_accounts.is_empty() && policy.scope_groups.is_empty() {
            return Err("指定账号范围时，至少要选一个账号或分组".to_string());
        }
        Ok(policy)
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
            // 策略默认值：不限额 + 全部账号 = **改造前行为**（新建的 Key 一开始没有约束）。
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

    /// 更新策略（配额 + 账号范围）。**不动** id / hash / 吊销态。
    ///
    /// 已吊销的 Key 也允许改策略：恢复后新策略生效（用户常「先改好再恢复」）。
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

    /// 撤销吊销（清 `revoked_at`，Key **重新可用**），不物理删除。
    ///
    /// 与 [`Self::delete`] 同构但方向相反：`delete` 只接受**已吊销**的 Key，
    /// 本方法同样**只接受已吊销**的 Key —— 对一把本来就能用的 Key「恢复」是空操作，
    /// 静默成功会让用户以为「刚才那把被吊销过、现在回来了」，而事实相反 ⇒ 明确报错。
    pub fn restore(&self, id: &str) -> Result<(), String> {
        let mut records = self.load();
        let target_revoked = records
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.revoked_at.is_some());
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
        assert_eq!(
            record.prefix,
            format!("sk-wb-{}", &plaintext["sk-wb-".len().."sk-wb-".len() + 4])
        );
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
            "id",
            "name",
            "region",
            "prefix",
            "suffix",
            "createdAt",
            "revokedAt",
            "revoked",
            "lastUsedAt",
            // ★ issue #76：策略七件套也进掩码视图（前端列表要用它们画用量列与权限 chip）。
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
        assert!(store
            .verify(&String::from_utf8(different).unwrap())
            .is_none());
        assert!(store.verify("x").is_none());
    }

    #[test]
    fn verify_accepts_valid_rejects_invalid_and_revoked() {
        let store = temp_store();
        let (record, plaintext) = store.create("k".to_string(), Region::Global);

        assert!(store.verify(&plaintext).is_some(), "有效 Key 应通过");
        assert!(
            store
                .verify("sk-wb-deadbeefdeadbeefdeadbeefdeadbeef")
                .is_none(),
            "无效 Key 应拒绝"
        );

        store.revoke(&record.id).unwrap();
        assert!(store.verify(&plaintext).is_none(), "已吊销 Key 应拒绝");
        assert!(store
            .list()
            .iter()
            .any(|r| r.id == record.id && r.revoked_at.is_some()));

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
    fn restore_revives_revoked_and_rejects_unknown_or_active() {
        let store = temp_store();
        let (record, plaintext) = store.create("restore".to_string(), Region::Cn);

        // 未吊销 ⇒ 无需恢复（**响亮失败**，不静默成功 —— 否则用户会以为刚才那把被吊销过）。
        assert_eq!(
            store.restore(&record.id),
            Err("该 API Key 未吊销，无需恢复".to_string())
        );
        assert_eq!(store.restore("missing"), Err("API Key 不存在".to_string()));

        store.revoke(&record.id).unwrap();
        assert!(store.verify(&plaintext).is_none(), "已吊销 Key 应拒绝");

        store.restore(&record.id).unwrap();
        // ★ 判据锚在**行为**上：恢复的语义是「Key 重新可用」。只断言 `revoked_at.is_none()`
        //   会被「清了字段但鉴权仍拦」的实现骗过（永远绿）。
        assert!(
            store.verify(&plaintext).is_some(),
            "恢复后 Key 必须重新可用"
        );
        let after = store
            .list()
            .into_iter()
            .find(|item| item.id == record.id)
            .expect("恢复不应删除记录");
        assert!(after.revoked_at.is_none());
        // 恢复只清 `revoked_at`：hash 与创建时间不得被牵动（否则等于换了一把 Key）。
        assert_eq!(after.hash, record.hash, "恢复不得改动 hash");
        assert_eq!(after.created_at, record.created_at, "恢复不得改动创建时间");

        // 恢复后仍可再次吊销 → 再删除（两条出口互不排斥）。
        store.revoke(&record.id).unwrap();
        store.delete(&record.id).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn restore_does_not_leak_or_touch_other_keys() {
        let store = temp_store();
        let (first, first_plaintext) = store.create("first".to_string(), Region::Cn);
        let (second, second_plaintext) = store.create("second".to_string(), Region::Global);

        store.revoke(&first.id).unwrap();
        store.revoke(&second.id).unwrap();
        store.restore(&first.id).unwrap();

        // 恢复一把不得连带恢复另一把（错的实现常写成「无条件全清 revoked_at」）。
        assert!(
            store.verify(&first_plaintext).is_some(),
            "被恢复的 Key 应可用"
        );
        assert!(
            store.verify(&second_plaintext).is_none(),
            "另一把必须仍是吊销态"
        );
        let second_after = store
            .list()
            .into_iter()
            .find(|item| item.id == second.id)
            .unwrap();
        assert!(second_after.revoked_at.is_some());
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

    /* ─────────── 策略（issue #76）─────────── */

    /// 构造一条带策略的记录（不落盘，直接测纯函数）。
    fn policy_record() -> ApiKeyRecord {
        ApiKeyRecord {
            id: "k1".into(),
            name: "k".into(),
            region: Region::Cn,
            prefix: "sk-wb-a1b2".into(),
            suffix: "9f3c".into(),
            hash: "x".into(),
            created_at: 0,
            revoked_at: None,
            last_used_at: None,
            quota_mode: QuotaMode::Limited,
            quota_limit: 1000,
            quota_unit: QuotaUnit::Token,
            quota_period: QuotaPeriod::None,
            scope_mode: ScopeMode::All,
            scope_accounts: Vec::new(),
            scope_groups: Vec::new(),
        }
    }

    #[test]
    fn legacy_ledger_without_policy_fields_deserializes_to_defaults() {
        // 老台账：只有前 9 个字段（无策略七件套）。
        // ★ 形态对齐**落盘文件**（`save` 用 `to_string_pretty` ⇒ snake_case）：
        //   camelCase 只在 `masked()`（给前线的线上形态）里出现。混淆两者会让
        //   「老台账零迁移」这条断言测的不是真台账。
        let legacy = serde_json::json!([{
            "id": "old", "name": "old", "region": "cn", "prefix": "sk-wb-a1b2",
            "suffix": "9f3c", "hash": "h", "created_at": 1, "revoked_at": null,
            "last_used_at": null
        }]);
        let records: Vec<ApiKeyRecord> = serde_json::from_value(legacy).unwrap();
        let record = &records[0];
        // 默认值必须正好是「改造前的行为」：不限额 + 全部账号。
        assert_eq!(record.quota_mode, QuotaMode::Unlimited);
        assert_eq!(record.scope_mode, ScopeMode::All);
        assert!(record.scope_accounts.is_empty());
        assert_eq!(
            quota_verdict(record, u64::MAX),
            QuotaVerdict::Ok,
            "老 Key 不得被限额"
        );
        assert!(
            account_allowed(record, "anyone", None),
            "老 Key 不得被白名单挡住"
        );
    }

    #[test]
    fn quota_verdict_covers_unlimited_zero_limit_and_boundary() {
        let mut record = policy_record();
        assert_eq!(quota_verdict(&record, 999), QuotaVerdict::Ok);
        // 等于上限即超限（上限是「最多用到这么多」）。
        assert_eq!(
            quota_verdict(&record, 1000),
            QuotaVerdict::Exceeded {
                used: 1000,
                limit: 1000,
                unit: QuotaUnit::Token,
                period: QuotaPeriod::None
            }
        );
        record.quota_mode = QuotaMode::Unlimited;
        assert_eq!(
            quota_verdict(&record, u64::MAX),
            QuotaVerdict::Ok,
            "不限额恒放行"
        );
        record.quota_mode = QuotaMode::Limited;
        record.quota_limit = 0;
        assert_eq!(
            quota_verdict(&record, u64::MAX),
            QuotaVerdict::Ok,
            "limit=0 = 关闭该条"
        );
    }

    #[test]
    fn account_allowed_unions_accounts_and_groups() {
        let mut record = policy_record();
        record.scope_mode = ScopeMode::Selected;
        record.scope_accounts = vec!["u1".into()];
        record.scope_groups = vec!["主力".into()];
        assert!(account_allowed(&record, "u1", None), "uid 命中即放行");
        assert!(
            account_allowed(&record, "u2", Some("主力")),
            "分组命中即放行"
        );
        assert!(
            !account_allowed(&record, "u2", Some("备用")),
            "名单外不放行"
        );
        assert!(
            !account_allowed(&record, "u2", None),
            "无分组且 uid 不在名单 ⇒ 拒"
        );
        assert!(!account_allowed(&record, "u2", Some("")), "空分组不算命中");
        record.scope_mode = ScopeMode::All;
        assert!(
            account_allowed(&record, "u2", Some("备用")),
            "全部账号 ⇒ 人人放行"
        );
    }

    #[test]
    fn period_key_is_none_for_cumulative_and_local_for_the_rest() {
        assert_eq!(QuotaPeriod::None.period_key(0), None, "不重置没有周期键");
        // 2026-10-22T00:00:00Z = 1781385600000ms（本地时区下的日期键会随时区平移，
        // 这里只断言**形状**与「同日同时一致」）。
        let noon = 1_781_388_000_000i64; // 2026-10-22 12:00 UTC
        let key = QuotaPeriod::Daily.period_key(noon).unwrap();
        assert!(key.starts_with("daily:20"), "日键形状：{key}");
        let weekly = QuotaPeriod::Weekly.period_key(noon).unwrap();
        assert!(
            weekly.starts_with("weekly:20") && weekly.contains("-W"),
            "周键形状：{weekly}"
        );
        let monthly = QuotaPeriod::Monthly.period_key(noon).unwrap();
        assert!(
            monthly.starts_with("monthly:20") && monthly.len() == "monthly:2026-10".len(),
            "月键形状：{monthly}"
        );
    }

    #[test]
    fn update_policy_writes_and_normalizes_contradictions() {
        let store = temp_store();
        let (record, _) = store.create("p".into(), Region::Cn);

        // 空白名单 ⇒ 拒绝（UI 已禁保存，后端再兜一层）。
        let empty = store.update_policy(
            &record.id,
            KeyPolicy {
                quota_mode: QuotaMode::Limited,
                quota_limit: 10,
                quota_unit: QuotaUnit::Token,
                quota_period: QuotaPeriod::Daily,
                scope_mode: ScopeMode::Selected,
                scope_accounts: vec![],
                scope_groups: vec![],
            },
        );
        assert!(empty.is_err(), "空白名单必须拒绝");

        // 正常写入。
        store
            .update_policy(
                &record.id,
                KeyPolicy {
                    quota_mode: QuotaMode::Limited,
                    quota_limit: 500,
                    quota_unit: QuotaUnit::Request,
                    quota_period: QuotaPeriod::Monthly,
                    scope_mode: ScopeMode::Selected,
                    scope_accounts: vec!["u1".into()],
                    scope_groups: vec!["主力".into()],
                },
            )
            .unwrap();
        let after = store
            .list()
            .into_iter()
            .find(|r| r.id == record.id)
            .unwrap();
        assert_eq!(after.quota_limit, 500);
        assert_eq!(after.quota_unit, QuotaUnit::Request);
        assert_eq!(after.quota_period, QuotaPeriod::Monthly);
        assert_eq!(after.scope_accounts, vec!["u1".to_string()]);
        assert_eq!(after.scope_groups, vec!["主力".to_string()]);

        // ★ 矛盾归一：切回「不限额 / 全部账号」时，旧上限与旧名单必须被清掉
        //   （否则下次切回来会「突然」拿过时的数生效）。
        store
            .update_policy(
                &record.id,
                KeyPolicy {
                    quota_mode: QuotaMode::Unlimited,
                    quota_limit: 999, // 矛盾：不限额却带上限
                    quota_unit: QuotaUnit::Token,
                    quota_period: QuotaPeriod::None,
                    scope_mode: ScopeMode::All,
                    scope_accounts: vec!["u1".into()], // 矛盾：全部账号却带名单
                    scope_groups: vec![],
                },
            )
            .unwrap();
        let normalized = store
            .list()
            .into_iter()
            .find(|r| r.id == record.id)
            .unwrap();
        assert_eq!(normalized.quota_limit, 0, "不限额 ⇒ 清空上限");
        assert!(normalized.scope_accounts.is_empty(), "全部账号 ⇒ 清白名单");

        // 不动身份字段。
        assert_eq!(normalized.hash, record.hash, "改策略不得动 hash");
        assert_eq!(
            normalized.created_at, record.created_at,
            "改策略不得动创建时间"
        );
        assert!(normalized.revoked_at.is_none());

        // 不存在的 id ⇒ 响亮失败。
        assert!(store
            .update_policy("missing", KeyPolicy::default())
            .is_err());
    }

    #[test]
    fn masked_carries_policy_fields_and_still_no_hash() {
        let store = temp_store();
        let (record, plaintext) = store.create("m".into(), Region::Cn);
        store
            .update_policy(
                &record.id,
                KeyPolicy {
                    quota_mode: QuotaMode::Limited,
                    quota_limit: 42,
                    quota_unit: QuotaUnit::Request,
                    quota_period: QuotaPeriod::Weekly,
                    scope_mode: ScopeMode::Selected,
                    scope_accounts: vec!["u1".into()],
                    scope_groups: vec![],
                },
            )
            .unwrap();
        let masked = store
            .list()
            .into_iter()
            .find(|r| r.id == record.id)
            .unwrap()
            .masked();
        assert_eq!(masked["quotaMode"], json!("limited"));
        assert_eq!(masked["quotaLimit"], json!(42));
        assert_eq!(masked["quotaUnit"], json!("request"));
        assert_eq!(masked["quotaPeriod"], json!("weekly"));
        assert_eq!(masked["scopeMode"], json!("selected"));
        assert_eq!(masked["scopeAccounts"], json!(["u1"]));
        assert!(!masked.to_string().contains(&plaintext), "掩码不得泄露明文");
    }

    #[test]
    fn sha256_is_stable_lowercase_hex() {
        let digest = sha256_hex("anything");
        assert_eq!(digest.len(), 64);
        assert_eq!(digest, sha256_hex("anything"));
        assert!(digest
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // 已知向量：sha256("") = e3b0c442...
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    // -----------------------------------------------------------------------
    // 安全审计 P1-7：白名单展开必须只有**一份**实现
    // -----------------------------------------------------------------------

    fn scope_policy(accounts: &[&str], groups: &[&str]) -> KeyPolicy {
        KeyPolicy {
            quota_mode: QuotaMode::Unlimited,
            quota_limit: 0,
            quota_unit: QuotaUnit::Request,
            quota_period: QuotaPeriod::None,
            scope_mode: ScopeMode::Selected,
            scope_accounts: accounts.iter().map(|v| v.to_string()).collect(),
            scope_groups: groups.iter().map(|v| v.to_string()).collect(),
        }
    }

    /// 展开 = 「显式 uid ∪ 命中分组里的 uid」，其余一律不在集合里。
    ///
    /// 这条用例的价值在于「三处调用方只剩取 `(uid, group)` 的差异」：
    /// 它们都走 `allowed_uids_for_policy`，所以平台之间不可能再分叉。
    #[test]
    fn allowlist_expansion_is_the_union_of_uids_and_groups() {
        let policy = scope_policy(&["u-explicit"], &["g1"]);
        let accounts = vec![
            ("u-explicit".to_string(), Some("g1".to_string())),
            ("u-by-group".to_string(), Some("g1".to_string())),
            ("u-other-group".to_string(), Some("g2".to_string())),
            ("u-no-group".to_string(), None),
            ("u-empty-group".to_string(), Some(String::new())),
        ];
        let allowed = allowed_uids_for_policy(&policy, accounts).expect("Selected 必须给出集合");
        assert!(allowed.contains("u-explicit"), "{allowed:?}");
        assert!(allowed.contains("u-by-group"), "{allowed:?}");
        assert!(!allowed.contains("u-other-group"), "{allowed:?}");
        assert!(
            !allowed.contains("u-no-group"),
            "没有分组的号不该被分组名单捞进来: {allowed:?}"
        );
        assert!(
            !allowed.contains("u-empty-group"),
            "空字符串分组不是「命中」——否则所有无分组的号都会被放行: {allowed:?}"
        );
    }

    /// ★ 只增不删：显式写进策略的 uid 即使账号库里已经没有了，也必须留在集合里。
    ///
    /// 反例（改坏会红）：把 seed 改成「遍历 accounts 的产物」⇒ 用户会看到「改完策略
    /// 少了一个号」，且毫无解释。
    #[test]
    fn explicit_accounts_survive_a_missing_account_record() {
        let policy = scope_policy(&["u-deleted"], &[]);
        let allowed = allowed_uids_for_policy(&policy, Vec::<(String, Option<String>)>::new())
            .expect("Selected 必须给出集合");
        assert_eq!(
            allowed.into_iter().collect::<Vec<_>>(),
            vec!["u-deleted".to_string()],
            "账号已被删除也不能丢掉显式名单"
        );
    }

    /// `All` ⇒ `None`（不过滤）；这条守住「改策略切回全部」时不会留下一个空集合
    /// 把所有人都挡在外面。
    #[test]
    fn scope_all_means_no_filter_at_all() {
        let mut policy = scope_policy(&["u1"], &[]);
        policy.scope_mode = ScopeMode::All;
        assert!(
            allowed_uids_for_policy(&policy, vec![("u2".to_string(), None)]).is_none(),
            "`All` 必须返回 None（不过滤），而不是一个空集合"
        );
        // 判定侧同口径：`All` 对任何号都放行，哪怕名单里写着别人。
        assert!(policy_allows(&policy, "u2", None));
        assert!(scope_allows(ScopeMode::All, &[], &[], "anything", None));
    }
}
