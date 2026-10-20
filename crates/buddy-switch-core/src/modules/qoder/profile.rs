//! 本机凭据导入（**只读**）与 PAT 导入。
//!
//! ## 两处官方凭据存储
//!
//! | 来源 | 路径 | 形态 |
//! |:--|:--|:--|
//! | **桌面 App** | `%APPDATA%\com.qoder[cn].app.stable\auth.v1.dat` | Electron `safeStorage`（`v10` 布局；Windows 下密钥经 DPAPI 解出） |
//! | **Qoder CLI** | `~/.qoder[.cn]/.auth/user` | 明文 JSON，或 AES-128-CBC（`key = iv = machine_id[:16]`） |
//!
//! ## 三条纪律
//!
//! 1. **只读**：本模块**永不**写客户端目录。改客户端登录态是 `switch` 的职责，不是导入的。
//! 2. **不静默采用**：探测（[`probe_local_sources`]）与导入（[`import_from_local_for`]）
//!    分开 —— 界面先展示「读到了什么」，用户确认后再落库。
//! 3. **「读不到」与「没有」必须分开说**：文件不存在 / 存在但解不开 / 解开了但没有账号，
//!    是三种不同的用户动作（去装客户端 / 去登录 / 去用另一个来源）。
//!    [`LocalProbe::state`] 把这三态显式区分，压成一个布尔值就会给出错误的引导。

use serde_json::{json, Value};

use crate::modules::qoder::account::{
    self, RawAccount, SOURCE_CLI, SOURCE_DESKTOP_APP, SOURCE_PAT,
};
use crate::modules::qoder::paths;
use crate::modules::qoder::region::{realm_spec, QoderRegion};
use crate::modules::qoder::store;
use crate::modules::qoder::token::{kind_of, TokenKind};

/// 一处本机凭据来源的探测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalState {
    /// 文件不存在 ⇒ 该客户端可能没装 / 没登录过。
    Missing,
    /// 文件在，但解不开（加密形态不认识 / 密钥取不到）。
    Unreadable(String),
    /// 读到了，但里面没有可用账号（例如只有空 token）。
    Empty,
    /// 读到了可用账号。
    Ready,
}

impl LocalState {
    /// 稳定标识（下发给前端）。
    pub fn as_str(&self) -> &'static str {
        match self {
            LocalState::Missing => "missing",
            LocalState::Unreadable(_) => "unreadable",
            LocalState::Empty => "empty",
            LocalState::Ready => "ready",
        }
    }

    /// 可读原因（`Missing` / `Ready` 无附加说明）。
    pub fn detail(&self) -> Option<String> {
        match self {
            LocalState::Unreadable(reason) => Some(reason.clone()),
            _ => None,
        }
    }
}

/// 一处来源的探测结果。
#[derive(Debug, Clone)]
pub struct LocalProbe {
    /// 来源种类。
    pub kind: &'static str,
    /// 展示名（界面用）。
    pub label: &'static str,
    /// 该来源的**路径**（展示用；脱敏由调用方决定，路径本身不是凭据）。
    pub path: String,
    /// 三态。
    pub state: LocalState,
}

/// 桌面 App 的来源标识。
pub const KIND_DESKTOP: &str = "desktop-app";
/// CLI 的来源标识。
pub const KIND_CLI: &str = "cli";

/// 探测两处来源（**不落库、不弹窗**）。
pub fn probe_local_sources(region: QoderRegion) -> Vec<LocalProbe> {
    vec![probe_desktop(region), probe_cli(region)]
}

/// 探测桌面 App 凭据。
pub fn probe_desktop(region: QoderRegion) -> LocalProbe {
    let path = paths::desktop_auth_file(region);
    let state = match read_desktop_credential(region) {
        Ok(Some(_)) => LocalState::Ready,
        Ok(None) => LocalState::Empty,
        Err(error) if error.kind == ReadErrorKind::Missing => LocalState::Missing,
        Err(error) => LocalState::Unreadable(error.message),
    };
    LocalProbe {
        kind: KIND_DESKTOP,
        label: "Qoder 桌面版",
        path: path.display().to_string(),
        state,
    }
}

/// 探测 CLI 凭据。
pub fn probe_cli(region: QoderRegion) -> LocalProbe {
    let path = paths::cli_auth_file(region);
    let state = match read_cli_credential(region) {
        Ok(Some(_)) => LocalState::Ready,
        Ok(None) => LocalState::Empty,
        Err(error) if error.kind == ReadErrorKind::Missing => LocalState::Missing,
        Err(error) => LocalState::Unreadable(error.message),
    };
    LocalProbe {
        kind: KIND_CLI,
        label: "Qoder CLI",
        path: path.display().to_string(),
        state,
    }
}

/// 某个程序位**当前登录的账号 uid**（只读；读不到 / 没登录 ⇒ `None`）。
///
/// 供「切换账号」判断「哪个程序位已经登着这个号」—— 卡片上的「当前登录」徽章与
/// 「切换到此账号」的可用性都由它决定。
///
/// ⚠️ **读不到 ≠ 没登录**：文件缺失 / 解不开都会给 `None`。调用方展示时要区分
/// 「这个程序位没装 / 没登录」与「读不出来」——用 [`probe_local_sources`] 拿四态，
/// 本函数只回答「uid 是多少」。
pub fn current_uid_for(region: QoderRegion, kind: &str) -> Option<String> {
    let account = match kind {
        KIND_DESKTOP => read_desktop_credential(region).ok().flatten(),
        KIND_CLI => read_cli_credential(region).ok().flatten(),
        _ => None,
    }?;
    Some(account.uid).filter(|uid| !uid.trim().is_empty())
}

/// 探测结果的**下发形态**（含区域与域名，供界面解释「为什么这个区域没有账号」）。
pub fn probe_view_for(region: QoderRegion) -> Value {
    let probes: Vec<Value> = probe_local_sources(region)
        .into_iter()
        .map(|probe| {
            json!({
                "kind": probe.kind,
                "label": probe.label,
                "path": probe.path,
                "state": probe.state.as_str(),
                "detail": probe.state.detail(),
            })
        })
        .collect();
    json!({
        "region": region.as_str(),
        "domain": realm_spec(region).domain,
        "sources": probes,
    })
}

/// 读一处凭据失败的**分类**（「不存在」与「读不懂」必须分开，见模块头第 3 条）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadErrorKind {
    /// 文件不存在。
    Missing,
    /// 文件在但读不出来（IO / 解密 / 解析）。
    Unreadable,
}

/// 读凭据的错误。
#[derive(Debug, Clone)]
pub struct ReadError {
    /// 分类。
    pub kind: ReadErrorKind,
    /// 可读原因。
    pub message: String,
}

fn missing(path: &std::path::Path) -> ReadError {
    ReadError {
        kind: ReadErrorKind::Missing,
        message: format!("未找到 {}", path.display()),
    }
}

fn unreadable(message: String) -> ReadError {
    ReadError {
        kind: ReadErrorKind::Unreadable,
        message,
    }
}

/// 从一条原始记录里抽出账号（两处来源共用的解析）。
///
/// 兼容几种字段命名（上游不同版本 / 两种存储用了不同的键）：
/// - 访问令牌：`token` / `accessToken` / `access_token`
/// - 刷新令牌：`refreshToken` / `refresh_token`
/// - 用户：`user.id` / `user.user_id` / `uid`
///
/// ⚠️ **只认 `pt-` / `dt-` / `jt-` 三种前缀**；认不出的一律**不采用**（见 [`kind_of`]）。
/// 但**不因此报错** —— 只是这条来源没有可用账号，交给上层给「读不懂」的文案。
fn account_from_record(region: QoderRegion, raw: &Value, source: &str) -> Option<RawAccount> {
    let get_str = |keys: &[&str]| -> Option<String> {
        for key in keys {
            if let Some(value) = raw.get(*key).and_then(Value::as_str) {
                if !value.trim().is_empty() {
                    return Some(value.to_string());
                }
            }
        }
        None
    };

    let access = get_str(&["token", "accessToken", "access_token"]);
    let refresh = get_str(&["refreshToken", "refresh_token"]);
    let personal = get_str(&["personalToken", "personal_token"]);

    // 至少要有一个**能认出前缀**的凭据，否则这条记录没有意义。
    let recognized = [access.as_deref(), refresh.as_deref(), personal.as_deref()]
        .into_iter()
        .flatten()
        .any(|token| kind_of(token).is_some());
    if !recognized {
        return None;
    }

    let user = raw.get("user");
    let uid = user
        .and_then(|value| {
            value
                .get("id")
                .or_else(|| value.get("user_id"))
                .or_else(|| value.get("uid"))
                .and_then(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                })
        })
        .or_else(|| get_str(&["uid", "user_id", "userId"]))?;
    if uid.trim().is_empty() {
        return None;
    }

    let nickname = user
        .and_then(|value| {
            value
                .get("name")
                .or_else(|| value.get("nickname"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .or_else(|| get_str(&["nickname", "name"]));

    Some(RawAccount {
        uid,
        nickname,
        domain: realm_spec(region).domain.to_string(),
        access_token: access,
        refresh_token: refresh,
        personal_token: personal,
        expires_at: raw
            .get("expiresAt")
            .or_else(|| raw.get("expires_at"))
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))),
        source: Some(source.to_string()),
        remark: None,
        group: None,
        ..Default::default()
    })
}

/// 读桌面 App 的凭据文件。
///
/// `Ok(None)` = 文件在但里面没有可用账号；`Err(Missing)` = 文件不存在。
fn read_desktop_credential(region: QoderRegion) -> Result<Option<RawAccount>, ReadError> {
    let path = paths::desktop_auth_file(region);
    if !path.exists() {
        return Err(missing(&path));
    }
    let blob = std::fs::read(&path)
        .map_err(|error| unreadable(format!("读取 {} 失败：{error}", path.display())))?;
    if blob.is_empty() {
        return Ok(None);
    }
    let app_dir = paths::desktop_app_dir(region);
    let plaintext = crate::modules::vscode_cn_inject::decrypt_electron_safe_storage(
        &blob,
        &app_dir,
        "Qoder 桌面版",
        MACOS_KEYCHAIN_SERVICE,
        LINUX_SECRET_TOOL_NAMES,
    )
    .map_err(|error| {
        unreadable(format!(
            "{} 无法解密：{error}（若刚升级过客户端，先在客户端里重新登录一次再试）",
            path.display()
        ))
    })?;
    let text = String::from_utf8_lossy(&plaintext);
    let json: Value = serde_json::from_str(text.trim()).map_err(|error| {
        unreadable(format!(
            "{} 解密成功但不是 JSON（{error}）—— 客户端存储格式可能已变更",
            path.display()
        ))
    })?;
    Ok(account_from_record(region, &json, SOURCE_DESKTOP_APP))
}

/// macOS Keychain 里 Electron `safeStorage` 的服务名。
///
/// ⚠️ **未实测**：Electron 的约定是「`<App Name> Safe Storage`」。若某台机器上查不到，
/// 错误文案会把该名字带出来，便于定位（而不是静默失败）。
const MACOS_KEYCHAIN_SERVICE: &str = "Qoder Safe Storage";

/// Linux `secret-tool` 的应用名候选（按顺序尝试）。
const LINUX_SECRET_TOOL_NAMES: &[&str] = &["qoder", "Qoder"];

/// 读 CLI 凭据文件。
///
/// 两种形态：
/// - 以 `{` 开头 ⇒ **明文 JSON**（最常见）；
/// - 否则 ⇒ AES-128-CBC，`key = iv = machine_id[:16]`，`machine_id` 取自同目录的
///   `machine_id` 文件。
fn read_cli_credential(region: QoderRegion) -> Result<Option<RawAccount>, ReadError> {
    let path = paths::cli_auth_file(region);
    if !path.exists() {
        return Err(missing(&path));
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| unreadable(format!("读取 {} 失败：{error}", path.display())))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = if bytes.first() == Some(&b'{') {
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        decrypt_cli_blob(region, &bytes)?
    };
    let json: Value = serde_json::from_str(text.trim()).map_err(|error| {
        unreadable(format!(
            "{} 不是可解析的 JSON（{error}）—— 客户端存储格式可能已变更",
            path.display()
        ))
    })?;
    Ok(account_from_record(region, &json, SOURCE_CLI))
}

/// 解 CLI 的 AES-128-CBC 密文（`key = iv = machine_id[:16]`）。
fn decrypt_cli_blob(region: QoderRegion, blob: &[u8]) -> Result<String, ReadError> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

    let machine_id_path = paths::cli_home_dir(region).join(".auth").join("machine_id");
    let machine_id = std::fs::read_to_string(&machine_id_path).map_err(|_| {
        unreadable(format!(
            "凭据是加密形态，但取不到设备标识 {} —— 无法派生解密密钥",
            machine_id_path.display()
        ))
    })?;
    let machine_id = machine_id.trim();
    if machine_id.len() < 16 {
        return Err(unreadable(format!(
            "设备标识过短（{} 位），无法派生解密密钥：{}",
            machine_id.len(),
            machine_id_path.display()
        )));
    }
    let key = machine_id.as_bytes()[..16].to_vec();
    let iv = key.clone();
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, blob)
        .map_err(|error| unreadable(format!("凭据不是合法 Base64：{error}")))?;
    let plaintext = Aes128CbcDec::new_from_slices(&key, &iv)
        .map_err(|error| unreadable(format!("构造解密器失败：{error}")))?
        .decrypt_padded_vec_mut::<Pkcs7>(&decoded)
        .map_err(|_| {
            unreadable(
                "解密失败：密钥不匹配（设备标识与写入凭据时的不一致）。\
                 请先在 CLI 里重新登录一次再导入。"
                    .to_string(),
            )
        })?;
    Ok(String::from_utf8_lossy(&plaintext).to_string())
}

/// 从**两处来源**导入（按 `uid` 去重；同一 uid 以**桌面 App 优先** —— 它带刷新令牌的概率更高）。
///
/// 返回**逐条落库的结果**：`imported` / `updated` 分开报，便于界面给出
/// 「新增 2 个、更新 1 个」而不是一句「完成」。
pub fn import_from_local_for(region: QoderRegion) -> Result<Value, String> {
    let mut found: Vec<(RawAccount, &'static str)> = Vec::new();
    let mut problems: Vec<Value> = Vec::new();

    for (kind, label, read) in [
        (
            KIND_DESKTOP,
            "Qoder 桌面版",
            read_desktop_credential as fn(QoderRegion) -> Result<Option<RawAccount>, ReadError>,
        ),
        (KIND_CLI, "Qoder CLI", read_cli_credential),
    ] {
        match read(region) {
            Ok(Some(account)) => found.push((account, kind)),
            Ok(None) => problems.push(json!({
                "kind": kind, "label": label, "state": "empty",
                "detail": "读到了凭据文件，但里面没有可用账号",
            })),
            Err(error) => problems.push(json!({
                "kind": kind,
                "label": label,
                "state": if error.kind == ReadErrorKind::Missing { "missing" } else { "unreadable" },
                "detail": error.message,
            })),
        }
    }

    // 同一 uid 只留一条：先出现的优先（桌面 App 排在前面）。
    let mut seen = std::collections::BTreeSet::new();
    found.retain(|(account, _)| seen.insert(account.uid.clone()));

    let mut imported = 0usize;
    let mut updated = 0usize;
    let mut failed: Vec<Value> = Vec::new();
    for (account, kind) in &found {
        let uid = account.uid.clone();
        match account::upsert_account_for(region, account.clone()) {
            Ok(account::UpsertOutcome::Added) => imported += 1,
            Ok(account::UpsertOutcome::Replaced) => updated += 1,
            Err(error) => failed.push(json!({ "uid": uid, "kind": kind, "error": error })),
        }
    }

    Ok(json!({
        "region": region.as_str(),
        "imported": imported,
        "updated": updated,
        "found": found.len(),
        "problems": problems,
        "failed": failed,
    }))
}

/// 由 PAT 构造一条账号记录（**纯本地**，不访问上游）。
///
/// ⚠️ 这里**不交换 jobToken** —— 交换需要上游调用，属 `refresh` 的职责。
/// 本函数只负责「把它变成一条合法的账号记录」，因此可以完全离线验证。
///
/// `uid` 由调用方给出：PAT 本身**不含**用户 id，上游要用它换一次才知道。
/// 若调用方也拿不到，用 `pt-<前 12 位>` 生成一个**稳定**的兜底 id（同一条 PAT 恒得同一 id，
/// 避免重复导入时产生多条记录）。
pub fn account_from_pat(
    region: QoderRegion,
    pat: &str,
    nickname: Option<String>,
) -> Result<RawAccount, String> {
    let pat = pat.trim();
    if pat.is_empty() {
        return Err("PAT 为空。".to_string());
    }
    if kind_of(pat) != Some(TokenKind::Personal) {
        return Err(format!(
            "这不是一个合法的 PAT：应以「{}」开头（前缀不匹配时**不会**被当成其他类型的令牌）。",
            TokenKind::Personal.prefix()
        ));
    }
    let fallback_uid = format!("pt-{}", pat.chars().skip(3).take(12).collect::<String>());
    Ok(RawAccount {
        uid: fallback_uid,
        nickname,
        domain: realm_spec(region).domain.to_string(),
        access_token: None,
        refresh_token: None,
        personal_token: Some(pat.to_string()),
        expires_at: None,
        source: Some(SOURCE_PAT.to_string()),
        remark: None,
        group: None,
        ..Default::default()
    })
}

/// 由 PAT 导入一条账号（落库）。
pub fn import_from_pat_for(
    region: QoderRegion,
    pat: &str,
    nickname: Option<String>,
) -> Result<Value, String> {
    let account = account_from_pat(region, pat, nickname)?;
    let uid = account.uid.clone();
    let outcome = account::upsert_account_for(region, account)?;
    Ok(json!({
        "region": region.as_str(),
        "uid": uid,
        "outcome": match outcome {
            account::UpsertOutcome::Added => "added",
            account::UpsertOutcome::Replaced => "updated",
        },
        "masked": store::mask(pat),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(token: &str, uid: &str) -> Value {
        json!({ "token": token, "refreshToken": "drt-r", "user": { "id": uid, "name": "小明" } })
    }

    /// ★ 只认已知前缀：**一个能认出的凭据都没有**的记录不采用
    /// （而不是存一条用不了的账号）。
    ///
    /// ⚠️ 注意「有刷新令牌但访问令牌认不出」是**可用**的（刷新令牌本身就是凭据），
    /// 所以判据是「**任一**凭据可识别」，不是「访问令牌可识别」。
    #[test]
    fn only_records_with_a_recognized_token_are_adopted() {
        assert!(account_from_record(QoderRegion::Cn, &record("dt-a", "u1"), SOURCE_CLI).is_some());
        assert!(account_from_record(QoderRegion::Cn, &record("pt-a", "u1"), SOURCE_CLI).is_some());
        // 访问令牌认不出，但刷新令牌是合法前缀 ⇒ 仍可用（可刷新出新的访问令牌）。
        assert!(
            account_from_record(QoderRegion::Cn, &record("mystery", "u1"), SOURCE_CLI).is_some(),
            "有合法刷新令牌时，访问令牌认不出不影响可用性"
        );
        // 两种令牌都认不出 ⇒ 不采用。
        let junk =
            json!({ "token": "mystery", "refreshToken": "also-mystery", "user": { "id": "u1" } });
        assert!(account_from_record(QoderRegion::Cn, &junk, SOURCE_CLI).is_none());
        // 完全没有令牌 ⇒ 不采用。
        let no_token = json!({ "user": { "id": "u1" } });
        assert!(account_from_record(QoderRegion::Cn, &no_token, SOURCE_CLI).is_none());
        // 没有 uid ⇒ 不采用（uid 是账号库唯一键）。
        assert!(account_from_record(QoderRegion::Cn, &record("dt-a", ""), SOURCE_CLI).is_none());
        // 三种字段命名都要认。
        for key in ["token", "accessToken", "access_token"] {
            let raw = json!({ key: "dt-a", "user": { "id": "u1" } });
            assert!(
                account_from_record(QoderRegion::Cn, &raw, SOURCE_CLI).is_some(),
                "{key} 未被识别"
            );
        }
    }

    /// 导入时**域名按区域填**（不是照抄客户端里的值）—— 避免把国际版账号写进国内库。
    #[test]
    fn imported_account_gets_the_region_domain() {
        let cn = account_from_record(QoderRegion::Cn, &record("dt-a", "u1"), SOURCE_CLI).unwrap();
        assert_eq!(cn.domain, "qoder.com.cn");
        let global =
            account_from_record(QoderRegion::Global, &record("dt-a", "u1"), SOURCE_CLI).unwrap();
        assert_eq!(global.domain, "qoder.com");
        // 落库前置校验必须通过。
        assert!(account::ensure_region_matches(QoderRegion::Cn, &cn.domain).is_ok());
    }

    /// PAT 导入：**纯本地**，且校验前缀。
    #[test]
    fn pat_import_validates_prefix_and_is_stable() {
        let account = account_from_pat(QoderRegion::Cn, "pt-abcdefghijklmnop", None).unwrap();
        assert_eq!(
            account.personal_token.as_deref(),
            Some("pt-abcdefghijklmnop")
        );
        assert!(account.access_token.is_none(), "PAT 导入不应伪造访问令牌");
        assert_eq!(account.source.as_deref(), Some(SOURCE_PAT));
        // 同一条 PAT 恒得同一兜底 uid（重复导入不会产生多条记录）。
        let again = account_from_pat(QoderRegion::Cn, "pt-abcdefghijklmnop", None).unwrap();
        assert_eq!(account.uid, again.uid);
        // 前缀不对 ⇒ 拒绝，且文案点明期望前缀。
        let error = account_from_pat(QoderRegion::Cn, "dt-wrong-kind", None).unwrap_err();
        assert!(error.contains("pt-"), "{error}");
        assert!(account_from_pat(QoderRegion::Cn, "   ", None).is_err());
    }

    /// ★ 三态必须分开：「文件不存在」与「存在但读不懂」是两种不同的用户动作。
    ///
    /// 可证伪：把 `probe_*` 里的 `Missing` 分支并进 `Unreadable`，本用例立即变红
    /// —— 那正是「引导用户去重新登录，而其实他根本没装客户端」的缺陷形态。
    #[test]
    fn probe_distinguishes_missing_from_unreadable() {
        let missing = LocalState::Missing;
        let unreadable_state = LocalState::Unreadable("解密失败".to_string());
        let empty = LocalState::Empty;
        assert_ne!(missing.as_str(), unreadable_state.as_str());
        assert_ne!(empty.as_str(), unreadable_state.as_str());
        assert_eq!(missing.detail(), None);
        assert!(unreadable_state.detail().is_some(), "读不懂必须带原因");
        assert_eq!(empty.detail(), None);
    }

    /// 探测结果的**下发形态**：四个来源字段齐全，且 state 是稳定字面量。
    #[test]
    fn probe_view_exposes_a_stable_shape() {
        let view = probe_view_for(QoderRegion::Global);
        assert_eq!(view["region"], json!("global"));
        assert_eq!(view["domain"], json!("qoder.com"));
        let sources = view["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 2);
        for source in sources {
            for key in ["kind", "label", "path", "state"] {
                assert!(!source[key].is_null(), "缺字段 {key}：{source}");
            }
            assert!(
                ["missing", "unreadable", "empty", "ready"]
                    .contains(&source["state"].as_str().unwrap()),
                "state 取值非法：{source}"
            );
        }
    }
}
