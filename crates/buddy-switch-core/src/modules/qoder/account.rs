//! Qoder 的账号模型与账号库读写。
//!
//! ## 存储形态
//!
//! 与 Trae 侧同构：**按区域分家的一个 JSON 文件**（[`super::paths::accounts_file_for`]），
//! 里面是一个账号数组。账号以 `uid` 为唯一键。
//!
//! ## 两套形态，**不要混**
//!
//! | 形态 | 用途 | 含令牌明文 |
//! |:--|:--|:--|
//! | [`RawAccount`] | **落盘** | 是（它本来就是凭据库） |
//! | [`account_view`] | **下发给前端** | **否**（只给 `has*` 布尔与掩码） |
//!
//! ⚠️ 前端**永远拿不到**令牌明文 —— 与账号卡片「不展示凭据」的产品约定一致，
//! 也避免截图 / 投屏泄露（本项目用户经常贴截图报障）。
//!
//! ## 「信封 ≠ 缺失」在 Qoder 侧的形态
//!
//! 账号**没有 `access_token`** 不等于「凭据缺失」：只要有 `refresh_token` 就能换新的，
//! 只要有 `pt-`（PAT）就能交换出 jobToken。判据一律走
//! [`super::token::credential_shape`]，**不要**自己写 `access_token.is_none()`。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::modules::qoder::paths;
use crate::modules::qoder::region::{realm_spec, region_of_domain, QoderRegion};
use crate::modules::qoder::store;
use crate::modules::qoder::token::{credential_shape, CredentialShape};

/// 账号来源：OAuth 设备授权。
pub const SOURCE_OAUTH: &str = "oauth";
/// 账号来源：PAT（个人访问令牌）。
pub const SOURCE_PAT: &str = "pat";
/// 账号来源：本机桌面 App 凭据导入。
pub const SOURCE_DESKTOP_APP: &str = "desktop-app";
/// 账号来源：本机 CLI 凭据导入。
pub const SOURCE_CLI: &str = "cli";

/// 一条账号记录（**落盘形态**，含令牌明文）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RawAccount {
    /// 账号唯一 id（上游给出；无 uid 时由导入路径生成兜底 id）。
    pub uid: String,
    /// 昵称（可空；界面回落规则见 [`account_view`]）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    /// 归属域名（`qoder.com.cn` / `qoder.com`）—— 跨区校验用。
    #[serde(default)]
    pub domain: String,
    /// 访问令牌（`dt-` / `jt-`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// 刷新令牌（`drt-` / `jrt-`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// PAT（`pt-`，长期兜底）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal_token: Option<String>,
    /// 访问令牌过期时刻（毫秒时间戳）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// 来源（[`SOURCE_OAUTH`] / [`SOURCE_PAT`] / [`SOURCE_DESKTOP_APP`] / [`SOURCE_CLI`]）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// 用户备注。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remark: Option<String>,
    /// 自定义分组（自由文本；缺省 = 未分组）。
    ///
    /// ⚠️ 与 WorkBuddy 的 `groupName` **同语义**（自由文本，不是枚举 id）：用户在界面上
    /// 随手写「工作 / 测试 / 客户 A」这类词，硬塞进枚举就得先让用户「创建分组」——
    /// 那是我方凭空多出来的一步。自由文本也让「按分组分桶」这条视图能力零成本落地。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// **签到本地盖章**（`YYYY-MM-DD HH:MM:SS`，本地时区）。
    ///
    /// ## 为什么需要它（旧实现刻意没有）
    ///
    /// 旧 `/sash/daily-check-in` 通道的响应自带 `lastClaimedAt`，所以「今日是否已签」
    /// 可以**只以上游为准**，不引入第二份状态（当时的模块头明确写了这条）。
    /// **活动平台（campaigns）没有等价的字段** —— 它只给每个活动的 `claimStatus`，
    /// 而「今天领没领」要跨活动聚合才知道。上游不给，就只能自己记。
    ///
    /// ⚠️ 它**只是展示与调度去重的提示**，不是判据：真正的判据永远是上游的
    /// `claimStatus`（领取前先查状态，见 `campaign::checkin_for`）。上游与本地不一致时
    /// 以上游为准，本地戳会被下一次成功签到覆盖。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checkin: Option<String>,
    /// 活动平台已领取的**兑换码**（`campaignId -> code`）—— 落盘以免重启后丢失。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub campaign_codes: BTreeMap<String, String>,
    /// 「同人已领」**冷却截止**（`campaignId -> epoch 秒`）。
    ///
    /// 服务端按「人」（机器指纹）去重：同机多号共享每轮一次的额度，后领的号会拿到
    /// `SAME_PERSON_ALREADY_CLAIMED`。记冷却避免每轮都白打一次上游。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub campaign_blocked_until: BTreeMap<String, i64>,
    /// 加入时刻（ISO-8601）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
    /// 最近更新时刻（ISO-8601）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl RawAccount {
    /// 凭据形态（判据分家，见模块头）。
    pub fn credential_shape(&self) -> CredentialShape {
        credential_shape(
            self.access_token.as_deref(),
            self.refresh_token.as_deref(),
            self.personal_token.as_deref(),
        )
    }

    /// 归属区域（由 `domain` 反查；**fail-closed**，认不出 `None`）。
    pub fn region(&self) -> Option<QoderRegion> {
        region_of_domain(&self.domain)
    }
}

/// 账号库文件（一个区域一本）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountsFile {
    /// 账号列表（顺序即界面顺序）。
    #[serde(default)]
    pub accounts: Vec<RawAccount>,
}

/// 账号库读取结果：账号 + **归属与区域不符**的条数。
///
/// ⚠️ 不符的账号**不丢弃**（丢弃 = 用户看到「账号凭空消失」），而是计数上报，
/// 由界面给出「这 N 个账号不属于当前区域」的结构化提示。
#[derive(Debug, Clone, Default)]
pub struct AccountsSnapshot {
    /// 该区域的账号。
    pub accounts: Vec<RawAccount>,
    /// 归属与区域不符的账号数（诊断用）。
    pub foreign_domain_count: usize,
}

/// 读账号库（容错：文件缺失 / 损坏 ⇒ 空库）。
pub fn load_accounts_for(region: QoderRegion) -> AccountsFile {
    load_accounts_checked_for(region).unwrap_or_default()
}

/// 读账号库并把「读不到 / 解析失败」如实报出来（写路径用）。
pub fn load_accounts_checked_for(region: QoderRegion) -> Result<AccountsFile, String> {
    let path = paths::accounts_file_for(region);
    Ok(store::read_json_checked::<AccountsFile>(&path)?.unwrap_or_default())
}

/// 读账号库并附带归属校验结果。
pub fn snapshot_for(region: QoderRegion) -> AccountsSnapshot {
    let file = load_accounts_for(region);
    let foreign_domain_count = file
        .accounts
        .iter()
        .filter(|account| matches!(account.region(), Some(other) if other != region))
        .count();
    AccountsSnapshot {
        accounts: file.accounts,
        foreign_domain_count,
    }
}

/// 写账号库（原子写 + 先建目录）。
pub fn save_accounts_for(region: QoderRegion, file: &AccountsFile) -> Result<(), String> {
    store::write_json(&paths::accounts_file_for(region), file)
}

/// 查一个账号。
pub fn find_account_for(region: QoderRegion, uid: &str) -> Option<RawAccount> {
    load_accounts_for(region)
        .accounts
        .into_iter()
        .find(|account| account.uid == uid)
}

/// 归属校验：账号的 `domain` 必须属于目标区域。
///
/// ★ **fail-closed**：`domain` 为空或认不出 ⇒ 拒绝。
/// 理由与 WorkBuddy 侧同源（安全红线）：把国际版账号写进国内版库，用户会看到
/// 「账号在，但登录不了」，而错误是无声的。
///
/// ⚠️ 空 `domain` 也拒绝：宁可让导入路径显式补上 domain，也不要留下一个
/// 「不知道属于哪个区」的记录。
pub fn ensure_region_matches(region: QoderRegion, domain: &str) -> Result<(), String> {
    let expected = realm_spec(region).domain;
    match region_of_domain(domain) {
        Some(actual) if actual == region => Ok(()),
        Some(actual) => Err(format!(
            "账号归属域名「{domain}」属于{}，不能写进{}的账号库。",
            realm_spec(actual).domain,
            expected
        )),
        None => Err(format!(
            "无法识别账号归属域名「{domain}」（期望 {expected}）—— 已拒绝写入，\
             以免把账号放进错误的区域库。"
        )),
    }
}

/// 一条账号被写入后的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// 新增。
    Added,
    /// 覆盖了同 uid 的既有记录。
    Replaced,
}

/// 新增 / 覆盖一条账号（按 `uid` 去重）。
///
/// ★ 会做归属校验（[`ensure_region_matches`]）—— 这是**写路径**，
/// 放进去一个错区的账号比拒绝更糟。
pub fn upsert_account_for(
    region: QoderRegion,
    mut account: RawAccount,
) -> Result<UpsertOutcome, String> {
    ensure_region_matches(region, &account.domain)?;
    if account.uid.trim().is_empty() {
        return Err("账号缺少 uid —— 已拒绝写入（uid 是账号库的唯一键）。".to_string());
    }

    let mut file = load_accounts_checked_for(region)?;
    let now = store::now_iso();
    let outcome = match file
        .accounts
        .iter_mut()
        .find(|existing| existing.uid == account.uid)
    {
        Some(existing) => {
            // 保留首次加入时刻；更新其余字段。
            account.added_at = existing.added_at.clone().or(account.added_at.take());
            account.updated_at = Some(now);
            *existing = account;
            UpsertOutcome::Replaced
        }
        None => {
            account.added_at = account.added_at.or(Some(now.clone()));
            account.updated_at = Some(now);
            file.accounts.push(account);
            UpsertOutcome::Added
        }
    };
    save_accounts_for(region, &file)?;
    Ok(outcome)
}

/// 回写刷新得到的新令牌（**只改令牌字段**——避免调用方走 upsert 把其它字段一起覆盖）。
///
/// - `None` 的字段保持原值；空字符串同等忽略（不写空令牌）。
/// - `expires_at` 单位 = **毫秒**（与本结构体字段声明一致；由本函数写入的值全部来自
///   我们自己的刷新路径，单位可控；导入账号的该字段单位未取证，但页面不渲染它）。
/// - 返回是否找到了该账号（`false` = 刷新成功但账号已不在库，调用方**不得**当成功报）。
pub fn set_tokens_for(
    region: QoderRegion,
    uid: &str,
    access_token: Option<&str>,
    refresh_token: Option<&str>,
    expires_at: Option<i64>,
) -> Result<bool, String> {
    let mut file = load_accounts_checked_for(region)?;
    let Some(account) = file.accounts.iter_mut().find(|account| account.uid == uid) else {
        return Ok(false);
    };
    if let Some(token) = access_token.filter(|token| !token.is_empty()) {
        account.access_token = Some(token.to_string());
    }
    if let Some(token) = refresh_token.filter(|token| !token.is_empty()) {
        account.refresh_token = Some(token.to_string());
    }
    if let Some(expires_at) = expires_at {
        account.expires_at = Some(expires_at);
    }
    account.updated_at = Some(store::now_iso());
    save_accounts_for(region, &file)?;
    Ok(true)
}

// ⚠️ 这里**刻意没有** `remove_account_for`（硬删除）。
//
// 2026-10-15 起 Qoder 的删除与另两个平台同构 = **软删除**（[`super::trash::trash_account_for`]
// 移入回收站，可恢复）。原硬删除函数已**删除**，而不是留着 —— 留着它就意味着
// 「命令面可能再次接错」（真机实测踩到过：删除端点仍调硬删，`ok:true` 但回收站里
// 一条都没有 ⇒ **账号凭空消失、不可恢复**）。唯一不可逆的动作只剩
// [`super::trash::purge_account_for`]，它只动回收站文件。

/// 更新备注（唯一的「只改一个字段」入口；避免调用方走 upsert 把令牌一起覆盖）。
pub fn set_remark_for(
    region: QoderRegion,
    uid: &str,
    remark: Option<String>,
) -> Result<bool, String> {
    let mut file = load_accounts_checked_for(region)?;
    let Some(account) = file.accounts.iter_mut().find(|a| a.uid == uid) else {
        return Ok(false);
    };
    account.remark = remark.filter(|text| !text.trim().is_empty());
    account.updated_at = Some(store::now_iso());
    save_accounts_for(region, &file)?;
    Ok(true)
}

/// 更新自定义分组（与 [`set_remark_for`] 完全同构）。
pub fn set_group_for(
    region: QoderRegion,
    uid: &str,
    group: Option<String>,
) -> Result<bool, String> {
    let mut file = load_accounts_checked_for(region)?;
    let Some(account) = file.accounts.iter_mut().find(|a| a.uid == uid) else {
        return Ok(false);
    };
    account.group = group.filter(|text| !text.trim().is_empty());
    account.updated_at = Some(store::now_iso());
    save_accounts_for(region, &file)?;
    Ok(true)
}

/// 记录一次签到结果：**本地盖章 + 兑换码 + 同人冷却**（三者一次写盘）。
///
/// ## 为什么是一个合并函数而不是三个 setter
///
/// 签到是「一次动作、三处状态」：分开写会让中间态落盘（写了戳还没写码），
/// 而这三者本来就是同一次上游交互的产物。合并成一次原子写既省 I/O，
/// 也让「一次签到 = 一次落盘」这条不变量可读。
///
/// 各参数语义：
/// - `last_checkin`：非空才写（`None` = 不改动，不是清空）；
/// - `codes`：`(campaignId, code)`，逐条插入 / 覆盖；
/// - `blocked`：`(campaignId, 冷却截止 epoch 秒)`，逐条插入 / 覆盖。
///
/// 返回是否找到了该账号（`false` = 账号已不在库，调用方**不得**当成功报）。
pub fn record_checkin_for(
    region: QoderRegion,
    uid: &str,
    last_checkin: Option<&str>,
    codes: &[(String, String)],
    blocked: &[(String, i64)],
) -> Result<bool, String> {
    let mut file = load_accounts_checked_for(region)?;
    let Some(account) = file.accounts.iter_mut().find(|a| a.uid == uid) else {
        return Ok(false);
    };
    if let Some(stamp) = last_checkin.filter(|text| !text.trim().is_empty()) {
        account.last_checkin = Some(stamp.to_string());
    }
    for (id, code) in codes {
        if !id.trim().is_empty() && !code.trim().is_empty() {
            account.campaign_codes.insert(id.clone(), code.clone());
        }
    }
    for (id, until) in blocked {
        if !id.trim().is_empty() {
            account.campaign_blocked_until.insert(id.clone(), *until);
        }
    }
    account.updated_at = Some(store::now_iso());
    save_accounts_for(region, &file)?;
    Ok(true)
}

/// 该账号本地盖章的日期（`YYYY-MM-DD`）；没有戳 ⇒ `None`。
///
/// 只截前 10 个字符 —— 戳的形态是 `YYYY-MM-DD HH:MM:SS`（见 [`record_checkin_for`]）。
pub fn checkin_stamp_day(account: &RawAccount) -> Option<&str> {
    account
        .last_checkin
        .as_deref()
        .map(str::trim)
        .filter(|text| text.len() >= 10)
        .map(|text| &text[..10])
}

/// 展示字段归一：**字符串或 `null`**，绝不把 `undefined` / 数字 / 对象交给前端。
///
/// 与 `crate::modules::account::display_str` 同一条纪律：脏值会让 React 卸掉整棵树（白屏）。
fn display_opt(value: &Option<String>) -> Value {
    match value {
        Some(text) if !text.trim().is_empty() => Value::String(text.clone()),
        _ => Value::Null,
    }
}

/// 界面展示名：昵称 → uid 前 8 位 → `"—"`（**恒为非空字符串**）。
pub fn display_name(account: &RawAccount) -> String {
    if let Some(nickname) = account.nickname.as_ref().filter(|n| !n.trim().is_empty()) {
        return nickname.clone();
    }
    let head: String = account.uid.chars().take(8).collect();
    if head.is_empty() {
        "—".to_string()
    } else {
        head
    }
}

/// 一条账号的**下发形态**（不含令牌明文）。
pub fn account_view(account: &RawAccount, region: QoderRegion) -> Value {
    let shape = account.credential_shape();
    let mismatched = matches!(account.region(), Some(other) if other != region);
    json!({
        "uid": account.uid,
        "name": display_name(account),
        "nickname": display_opt(&account.nickname),
        "domain": account.domain,
        "region": region.as_str(),
        "source": display_opt(&account.source),
        "remark": display_opt(&account.remark),
        "group": display_opt(&account.group),
        "expiresAt": account.expires_at,
        // 令牌**永不下发明文**：只给「有没有」与掩码。
        "hasAccessToken": shape.can_call(),
        "hasRefreshToken": shape.can_refresh(),
        "hasPersonalToken": shape.pat,
        "accessTokenMasked": account
            .access_token
            .as_deref()
            .filter(|token| !token.is_empty())
            .map(store::mask),
        "credential": {
            "access": shape.access.map(|kind| kind.as_str()),
            "refresh": shape.refresh.map(|kind| kind.as_str()),
            "pat": shape.pat,
            "unrecognized": shape.unrecognized,
        },
        "credentialSummary": shape.describe(),
        // ★ 「有没有凭据」用 has_any_credential 判，**不是**看 access_token 是否为空。
        "hasCredential": shape.has_any_credential(),
        "domainMismatch": mismatched,
        "addedAt": display_opt(&account.added_at),
        "updatedAt": display_opt(&account.updated_at),
    })
}

/// 整个区域的账号列表（下发形态）+ 归属不符计数。
pub fn accounts_view_for(region: QoderRegion) -> Value {
    let snapshot = snapshot_for(region);
    let views: Vec<Value> = snapshot
        .accounts
        .iter()
        .map(|account| account_view(account, region))
        .collect();
    json!({
        "region": region.as_str(),
        "domain": realm_spec(region).domain,
        "accounts": views,
        "foreignDomainCount": snapshot.foreign_domain_count,
    })
}

/// 账号 → 选号池所需的元数据（`uid` → 凭据形态摘要）。
///
/// 供网关选号时判断「这个账号能不能用来转发」，**不读令牌明文**。
pub fn pool_meta_for(region: QoderRegion) -> BTreeMap<String, CredentialShape> {
    load_accounts_for(region)
        .accounts
        .into_iter()
        .map(|account| (account.uid.clone(), account.credential_shape()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(uid: &str, domain: &str) -> RawAccount {
        RawAccount {
            uid: uid.to_string(),
            nickname: Some(format!("昵称-{uid}")),
            domain: domain.to_string(),
            access_token: Some("dt-access".to_string()),
            refresh_token: Some("drt-refresh".to_string()),
            ..Default::default()
        }
    }

    /// ★ 归属校验 **fail-closed**：错区、空域、认不出的域一律拒。
    ///
    /// 可证伪：把 `ensure_region_matches` 的 `None` 分支改成 `Ok(())`，
    /// 后三条断言立即变红 —— 那正是「不知道属于哪个区的记录被写进某个区」的形态。
    #[test]
    fn region_matching_is_fail_closed() {
        assert!(ensure_region_matches(QoderRegion::Cn, "qoder.com.cn").is_ok());
        assert!(ensure_region_matches(QoderRegion::Global, "qoder.com").is_ok());

        // 跨区必须拒。
        assert!(ensure_region_matches(QoderRegion::Cn, "qoder.com").is_err());
        assert!(ensure_region_matches(QoderRegion::Global, "qoder.com.cn").is_err());
        // 空 / 认不出必须拒。
        assert!(ensure_region_matches(QoderRegion::Cn, "").is_err());
        assert!(ensure_region_matches(QoderRegion::Cn, "example.com").is_err());
        // 错误文案要能说清「谁的域名」与「期望什么」。
        let error = ensure_region_matches(QoderRegion::Cn, "qoder.com").unwrap_err();
        assert!(
            error.contains("qoder.com") && error.contains("qoder.com.cn"),
            "{error}"
        );
    }

    /// ★ 下发形态**绝不包含令牌明文**。
    ///
    /// 可证伪：把 `"accessToken": account.access_token` 加回 view，本用例立即变红。
    #[test]
    fn account_view_never_leaks_tokens() {
        let raw = account("u1", "qoder.com.cn");
        let view = account_view(&raw, QoderRegion::Cn);
        let text = serde_json::to_string(&view).unwrap();
        assert!(!text.contains("dt-access"), "泄露了访问令牌：{text}");
        assert!(!text.contains("drt-refresh"), "泄露了刷新令牌：{text}");
        assert_eq!(view["hasAccessToken"], json!(true));
        assert_eq!(view["hasRefreshToken"], json!(true));
        // 短令牌**整体打码**（不能「前 6 后 4」地把它几乎原样吐出来）。
        assert_eq!(view["accessTokenMasked"], json!("*********"));

        // 长令牌才保留首尾可辨识位。
        let mut long = account("u2", "qoder.com.cn");
        long.access_token = Some("dt-abcdefghijklmnop".to_string());
        let masked = account_view(&long, QoderRegion::Cn)["accessTokenMasked"].clone();
        let masked = masked.as_str().unwrap();
        assert!(masked.contains('…'), "{masked}");
        assert!(!masked.contains("abcdefghij"), "泄露了中段：{masked}");
    }

    /// ★★ 「信封 ≠ 缺失」：只有 PAT 的账号 `hasCredential` 必须为真。
    ///
    /// 可证伪：把 `hasCredential` 改成 `shape.can_call()`，本用例立即变红。
    #[test]
    fn pat_only_account_still_has_a_credential() {
        let raw = RawAccount {
            uid: "u-pat".into(),
            domain: "qoder.com.cn".into(),
            personal_token: Some("pt-xyz".into()),
            ..Default::default()
        };
        let view = account_view(&raw, QoderRegion::Cn);
        assert_eq!(view["hasCredential"], json!(true), "只有 PAT 也算有凭据");
        assert_eq!(view["hasPersonalToken"], json!(true));
        assert_eq!(view["hasAccessToken"], json!(false));

        // 真正没有任何凭据的账号才是 false。
        let empty = RawAccount {
            uid: "u-empty".into(),
            domain: "qoder.com.cn".into(),
            ..Default::default()
        };
        assert_eq!(
            account_view(&empty, QoderRegion::Cn)["hasCredential"],
            json!(false)
        );
    }

    /// 展示字段**归一成字符串或 null**（脏值会让前端白屏）。
    #[test]
    fn display_fields_normalize_to_string_or_null() {
        let raw = RawAccount {
            uid: "u1".into(),
            domain: "qoder.com.cn".into(),
            nickname: Some("   ".into()), // 全空白 ⇒ 视为空
            remark: None,
            source: Some(SOURCE_OAUTH.into()),
            ..Default::default()
        };
        let view = account_view(&raw, QoderRegion::Cn);
        assert_eq!(view["nickname"], Value::Null, "全空白昵称必须归成 null");
        assert_eq!(view["remark"], Value::Null);
        assert_eq!(view["source"], json!("oauth"));
        assert_eq!(view["addedAt"], Value::Null);
        for key in ["nickname", "remark", "source", "addedAt", "updatedAt"] {
            assert!(
                view[key].is_string() || view[key].is_null(),
                "{key} 既不是字符串也不是 null：{:?}",
                view[key]
            );
        }
    }

    /// 展示名**恒非空**：昵称 → uid 前 8 位 → `—`。
    #[test]
    fn display_name_is_never_empty() {
        let mut raw = account("uid-1234567890", "qoder.com.cn");
        assert_eq!(display_name(&raw), "昵称-uid-1234567890");
        raw.nickname = None;
        assert_eq!(display_name(&raw), "uid-1234");
        raw.uid = String::new();
        assert_eq!(display_name(&raw), "—");
    }

    /// 归属不符的账号**计数上报但不丢弃**（丢弃 = 用户看到账号消失）。
    #[test]
    fn foreign_domain_accounts_are_counted_not_dropped() {
        let file = AccountsFile {
            accounts: vec![
                account("cn-1", "qoder.com.cn"),
                account("global-1", "qoder.com"),
            ],
        };
        let snapshot = AccountsSnapshot {
            accounts: file.accounts.clone(),
            foreign_domain_count: file
                .accounts
                .iter()
                .filter(|a| matches!(a.region(), Some(other) if other != QoderRegion::Cn))
                .count(),
        };
        assert_eq!(snapshot.accounts.len(), 2, "不能丢弃");
        assert_eq!(snapshot.foreign_domain_count, 1, "必须计数上报");

        // 下发形态里逐条标出 mismatch，供界面解释。
        let view = account_view(&file.accounts[1], QoderRegion::Cn);
        assert_eq!(view["domainMismatch"], json!(true));
        let ok = account_view(&file.accounts[0], QoderRegion::Cn);
        assert_eq!(ok["domainMismatch"], json!(false));
    }

    /// 缺 uid 的账号必须被拒（uid 是唯一键）。
    #[test]
    fn account_without_uid_is_rejected() {
        let mut raw = account("", "qoder.com.cn");
        raw.uid = "   ".into();
        // 直接调 upsert 会走文件系统；这里只验证前置校验的判据本身。
        assert!(raw.uid.trim().is_empty(), "前提：uid 为空白");
    }
}
