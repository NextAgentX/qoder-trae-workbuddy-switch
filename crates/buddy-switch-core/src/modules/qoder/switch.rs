//! Qoder 的「切换账号」—— 把指定账号的凭据**写回**官方客户端的登录态。
//!
//! 与 [`super::profile`] 的「只读导入」互为反面：导入把客户端凭据读进账号库，
//! 切换把账号库里的凭据写回客户端。**两个程序位**（桌面版 / CLI）× **两个区域**
//! （`qoder-cn` / `qoder-global`）共四处登录态，全部由本模块负责写。
//!
//! ## 四条硬纪律
//!
//! ### 1. 桌面版必须「读原明文 → 只改字段 → 写回」，**不能从账号记录重建**
//!
//! `auth.v1.dat` 解密后的明文里有一批**账号库里没有**的字段：`refreshTokenExpiresAt`、
//! `user.email`、`user.phone`、`user.avatarUrl`，以及将来客户端升级后新增的字段。
//! 账号库的落盘结构（[`RawAccount`]）装不下它们（`expires_at` 是 `i64`，而明文里
//! `expiresAt` 是 RFC3339 字符串 ⇒ 导入时就已丢）。**若从 `RawAccount` 重建明文，
//! 这些字段会全部消失，客户端很可能把整份登录态判为无效并清掉** —— 用户会看到
//! 「切了个寂寞，还退登了」。
//!
//! ⇒ 因此本模块只做**原地改写**：把现有明文解析成 JSON 对象，只覆盖
//! `token` / `refreshToken` / `expiresAt` / `user.id` / `user.name`，其余键**原样保留**。
//!
//! ### 2. 客户端在跑时**拒绝写**（进程守卫）
//!
//! Qoder 客户端（桌面版）会把登录态读进内存，退出时按内存态**整体回写**。
//! 在它运行时写文件，用户一关客户端就前功尽弃（甚至写坏）。因此写前必须探测
//! [`program_running`]；判据沿用 [`crate::modules::process::is_image_running`] 的
//! 既有语义：**查询失败视为「在运行」**（保守拒绝，让用户重试一次，而不是赌一把）。
//!
//! ### 3. 写后必须**解密校验**，失败必须**回滚**
//!
//! 写文件本身可能「看起来成功、内容却不对」（加密参数漂移、磁盘/杀软介入…）。
//! 因此写完立刻**读回 → 解密 → 比对 `token`**；一旦不一致，把**写前备份**原样写回，
//! 并把「已还原」如实报出来。没有这一步，用户会被留在「既没换成、还回不去」的坏状态。
//!
//! ### 4. GCM 用**随机 nonce** ⇒ 幂等性只能在**明文层**断言
//!
//! Windows 下 `v10` 密文 = `[前缀 3B][随机 nonce 12B][AES-256-GCM 密文]`。同一个明文
//! 加密两次，**密文必然不同**（nonce 每次重生成）。所以「写两次结果一致」这种断言
//! 只能落在**解密后的明文 JSON** 上，绝不能比密文字节。本模块的单测据此组织。
//!
//! ## 两处登录态
//!
//! | 程序位 | 路径 | 形态 |
//! |:--|:--|:--|
//! | 桌面版 | [`paths::desktop_auth_file`] | Electron safeStorage（`v10`；Windows 下密钥取自同目录 `Local State` 经 DPAPI 解出；macOS 下口令在登录钥匙串，服务名按区域查表） |
//! | CLI | [`paths::cli_auth_file`] | 明文 JSON，或 AES-128-CBC（`key = iv = machine_id[:16]`，标准 Base64） |
//!
//! CLI 侧**跟随既有形态**写回：原文件是明文就写明文、是密文就写密文、文件不存在则按
//! 密文新建（这是官方客户端的默认形态）。这样客户端读到的永远是它自己写的形态。

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::modules::qoder::account::{self, RawAccount};
use crate::modules::qoder::paths;
use crate::modules::qoder::region::{realm_spec, QoderRegion};
use crate::modules::qoder::store;

/// Linux `secret-tool` 的应用名候选（按顺序尝试）。同上：必须与 `profile.rs` 一致。
const LINUX_SECRET_TOOL_NAMES: &[&str] = &["qoder", "Qoder"];

/// 桌面版登录态的展示名（与 [`super::profile`] 的来源标签逐字一致）。
const DESKTOP_LABEL: &str = "Qoder 桌面版";

/// Qoder 的**程序位**（两个）。
///
/// ⚠️ 区域（[`QoderRegion`]）是**另一个正交轴**，不进这个枚举：程序位决定「写哪一处
/// 登录态」，区域决定「写国内版还是国际版的那一处」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QoderProgram {
    /// 桌面版（`%APPDATA%\com.qoder[cn].app.stable\auth.v1.dat`）。
    Desktop,
    /// CLI（`~/.qoder[.cn]/.auth/user`）。
    Cli,
}

impl QoderProgram {
    /// 稳定标识（前端传参 / 返回值）。
    /// 稳定标识 = **凭据来源 id**，与 [`super::profile::KIND_DESKTOP`] / [`super::profile::KIND_CLI`]
    /// **逐字相同**。
    ///
    /// ★★ 这不是巧合，是**必须**：程序位与「凭据来源」在本平台是**同一件事**（一个程序位
    /// 对应一处官方登录态）。曾经这里用 `"desktop"` 而来源用 `"desktop-app"` —— 两套 id
    /// 让 `program_status_for` 按 `"desktop"` 去查来源表**永远落空**，真机表现为
    /// 「桌面版明明登着，界面却显示未检测到」（单测测不到，因为两边各自都自洽）。
    /// ⇒ **一个事实只许有一个 id**。
    pub fn as_str(self) -> &'static str {
        match self {
            QoderProgram::Desktop => super::profile::KIND_DESKTOP,
            QoderProgram::Cli => super::profile::KIND_CLI,
        }
    }

    /// 从字符串解析程序位，**fail-closed**：认不出就 `None`，不回落默认值。
    ///
    /// 回落会让「前端传了个拼错的程序位」被静默当成桌面版写下去 —— 写错登录态比
    /// 拒绝更糟，因此宁可返回 `None` 让上层报错。
    pub fn parse(s: &str) -> Option<Self> {
        // fail-closed：只认**来源 id**，不接受 `"desktop"` 之类的「看起来像」的写法 ——
        // 宽容解析会让两套 id 悄悄复活（见 [`QoderProgram::as_str`] 的教训）。
        match s.trim().to_ascii_lowercase().as_str() {
            x if x == super::profile::KIND_DESKTOP => Some(QoderProgram::Desktop),
            x if x == super::profile::KIND_CLI => Some(QoderProgram::Cli),
            _ => None,
        }
    }

    /// 展示名 —— 与 [`super::profile`] 的既有来源标签**逐字一致**。
    pub fn label(self) -> &'static str {
        match self {
            QoderProgram::Desktop => "Qoder 桌面版",
            QoderProgram::Cli => "Qoder CLI",
        }
    }

    /// 全部程序位（遍历用，顺序稳定）。
    pub fn all() -> [QoderProgram; 2] {
        [QoderProgram::Desktop, QoderProgram::Cli]
    }
}

/// 某个程序位在某个区域的**进程映像名**（写前守卫用）。
///
/// 桌面版国内版实测映像名是 `Qoder CN.exe`；国际版按客户端命名推测为 `Qoder.exe`。
/// CLI 的映像名**未实测**（不同安装方式可能是 `qoder.exe` / 包在 `node.exe` 里），
/// 这里按「国内带 `-cn`」的既有目录命名习惯取值；取错的方向是「守卫可能漏判」，
/// 而守卫的兜底语义（[`crate::modules::process::is_image_running`] 查询失败即视为在跑）
/// 保证了最坏情况也只是多拒绝一次，不会误放行。
fn program_image(region: QoderRegion, program: QoderProgram) -> &'static str {
    match (program, region) {
        (QoderProgram::Desktop, QoderRegion::Cn) => "Qoder CN.exe",
        (QoderProgram::Desktop, QoderRegion::Global) => "Qoder.exe",
        (QoderProgram::Cli, QoderRegion::Cn) => "qoder-cn.exe",
        (QoderProgram::Cli, QoderRegion::Global) => "qoder.exe",
    }
}

/// 某个程序位当前是否在运行（写回前的守卫用）。
///
/// ★ **fail-closed**：判据委托给 [`crate::modules::process::is_image_running`]，
/// 其语义是「查询失败视为在运行」。守卫的唯一目的是避免「客户端退出时用内存态覆盖
/// 我们刚写的登录态」，误判成「在运行」的代价只是用户重试一次，误判成「已退出」的
/// 代价是白写一遍甚至写坏。
pub fn program_running(region: QoderRegion, program: QoderProgram) -> bool {
    program_running_with(region, program, crate::modules::process::is_image_running)
}

/// [`program_running`] 的可注入版本。
///
/// ## 为什么做成闭包参数
///
/// 真实判据要调用 `tasklist` / `pgrep`，单测里**无法**在不真的启动客户端的前提下
/// 让它返回「在运行」。把「映像名 → 是否在跑」这一步抽成闭包后，用例可以注入
/// `|_| true` / `|_| false` 并把「守卫真的拒绝了」这条分支钉死；同时还能捕获
/// 闭包收到的映像名，断言守卫查的是**正确的进程名**（否则守卫形同虚设）。
fn program_running_with<F>(region: QoderRegion, program: QoderProgram, mut probe: F) -> bool
where
    F: FnMut(&str) -> bool,
{
    probe(program_image(region, program))
}

/// 切换：把 `uid` 这条账号的凭据**写回**指定程序位的登录态。
///
/// 返回 `{ ok, program, region, uid, backupPath, written }`：
/// - `backupPath`：写前备份的路径（无既有文件时为 `null`）；
/// - `written`：`{ path, form, tokenMasked }` —— 写入目标、形态与**掩码**令牌。
///
/// ★ **返回值不含任何令牌明文**：需要展示时只给 [`store::mask`] 的掩码
/// （本项目用户经常贴截图报障）。
///
/// ## 顺序（不可调换）
///
/// ① 进程守卫 → ② 查账号 → ③ 区域 fail-closed → ④ 写回（含备份）→ ⑤ 写后校验 →
/// ⑥ 不一致则回滚。守卫必须在写之前：见模块头第 2 条。
pub fn switch_account_for(
    region: QoderRegion,
    program: QoderProgram,
    uid: &str,
) -> Result<Value, String> {
    switch_account_for_with(
        region,
        program,
        uid,
        crate::modules::process::is_image_running,
    )
}

/// [`switch_account_for`] 的可注入版本（单测用，注入进程探测避免依赖真机状态）。
fn switch_account_for_with<F>(
    region: QoderRegion,
    program: QoderProgram,
    uid: &str,
    probe: F,
) -> Result<Value, String>
where
    F: FnMut(&str) -> bool,
{
    let uid = uid.trim();
    if uid.is_empty() {
        return Err("缺少账号 uid —— 无法确定要切换哪个账号。".to_string());
    }

    // ① 进程守卫（fail-closed）。
    if program_running_with(region, program, probe) {
        return Err(format!(
            "{} 正在运行，写回的登录态会被它退出时的内存态覆盖。请先退出 {} 客户端，再切换账号。",
            program.label(),
            program.label()
        ));
    }

    // ② 查账号。
    let account = account::find_account_for(region, uid)
        .ok_or_else(|| format!("未找到账号 {uid}（区域 {}）。", region.as_str()))?;

    // ③ 区域 fail-closed：账号的归属域名必须属于目标区域。
    account::ensure_region_matches(region, &account.domain)?;

    // 写回必须有访问令牌：只写个空 token 会让客户端直接退登。
    if account
        .access_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        return Err(format!(
            "账号 {uid} 没有访问令牌（token），无法写回登录态（仅刷新令牌不足以让客户端登录）。"
        ));
    }

    let outcome = match program {
        QoderProgram::Desktop => {
            let app_dir = paths::desktop_app_dir(region);
            // ★ macOS 钥匙串服务名**按区域查表**（issue #81）：国内版是
            // `Qoder CN App Safe Storage`、国际版才是 `Qoder Safe Storage`
            // （真机取证 2026-09-23，`security dump-keychain`）。读写两侧
            // （本模块与 `super::profile`）从同一张表取 —— 用不同服务名会各自拿到
            // 不同的密钥，表现为「切换后客户端退登」。
            let macos_service = realm_spec(region).macos_keychain_service;
            let encode = move |plain: &[u8]| {
                crate::modules::vscode_cn_inject::encrypt_electron_safe_storage(
                    plain,
                    &app_dir,
                    DESKTOP_LABEL,
                    macos_service,
                    LINUX_SECRET_TOOL_NAMES,
                )
            };
            let app_dir = paths::desktop_app_dir(region);
            let decode = move |blob: &[u8]| {
                crate::modules::vscode_cn_inject::decrypt_electron_safe_storage(
                    blob,
                    &app_dir,
                    DESKTOP_LABEL,
                    macos_service,
                    LINUX_SECRET_TOOL_NAMES,
                )
            };
            switch_desktop(region, &account, encode, decode)?
        }
        QoderProgram::Cli => switch_cli(region, &account)?,
    };

    Ok(json!({
        "ok": true,
        "program": program.as_str(),
        "region": region.as_str(),
        "uid": uid,
        "backupPath": outcome.backup_path,
        "written": outcome.written,
    }))
}

/// 一次写回的结果（内部形态）。
struct SwitchWriteOutcome {
    /// 下发用的 `written` 字段（不含令牌明文）。
    written: Value,
    /// 写前备份路径（字符串形态，便于直接下发）。
    backup_path: Option<String>,
}

// ---------------------------------------------------------------------------
// 桌面版写回
// ---------------------------------------------------------------------------

/// 桌面版写回：解密现有明文 → 原地改字段 → 重新加密 → 备份 → 写入 → 校验 → 失败回滚。
///
/// `encode` / `decode` 是 Electron safeStorage 的加解密对；生产用
/// [`crate::modules::vscode_cn_inject`] 的实现，单测可注入「可逆但非确定性」的替身
/// （见模块头第 4 条：GCM 随机 nonce ⇒ 密文每次不同，只能比明文）。
fn switch_desktop<Enc, Dec>(
    region: QoderRegion,
    account: &RawAccount,
    encode: Enc,
    decode: Dec,
) -> Result<SwitchWriteOutcome, String>
where
    Enc: Fn(&[u8]) -> Result<Vec<u8>, String>,
    Dec: Fn(&[u8]) -> Result<Vec<u8>, String>,
{
    let auth_path = paths::desktop_auth_file(region);
    if !auth_path.exists() {
        return Err(format!(
            "未找到 Qoder 桌面版登录态文件 {}。请先打开 Qoder 桌面版并登录一次，再切换账号。",
            auth_path.display()
        ));
    }

    // 读原明文（**保留未知字段**的前提：必须先拿到现有的完整明文）。
    let existing = std::fs::read(&auth_path)
        .map_err(|error| format!("读取 {} 失败：{error}", auth_path.display()))?;
    let plaintext = decode(&existing).map_err(|error| {
        // ★ issue #153：把**密文形态**带进报错 —— 「格式变了」与「密钥不配套」是两种成因，
        //   不带这一句时两者在用户报告里长得一模一样（都只是「无法解密」）。
        format!(
            "无法解密现有登录态 {}（密文 {}）：{error}（若刚升级过客户端，请先在客户端里重新登录一次再试{}）",
            auth_path.display(),
            crate::modules::vscode_cn_inject::describe_ciphertext(&existing),
            crate::modules::vscode_cn_inject::STALE_KEY_HINT
        )
    })?;
    let mut json: Value = serde_json::from_slice(&plaintext).map_err(|error| {
        format!("现有登录态解密成功但不是 JSON（{error}）—— 客户端存储格式可能已变更。")
    })?;

    // 原地改字段（只动已知键，其余一律保留）。
    apply_desktop_overrides(&mut json, account)?;
    let new_plaintext =
        serde_json::to_vec(&json).map_err(|error| format!("序列化登录态失败：{error}"))?;
    let encrypted = encode(&new_plaintext)?;

    // 写前备份（失败回滚的底本）。
    let backup = backup_auth_file_for_switch(region, QoderProgram::Desktop, &auth_path);

    write_bytes(&auth_path, &encrypted)?;

    // 写后校验：读回 → 解密 → 比对 token。
    let expected = token_of(&json).unwrap_or_default();
    let actual = read_desktop_token(&auth_path, &decode);
    verify_or_rollback(&auth_path, backup.as_deref(), &expected, actual)?;

    Ok(SwitchWriteOutcome {
        written: json!({
            "path": auth_path.display().to_string(),
            "form": "electron-safe-storage",
            "tokenMasked": store::mask(&expected),
        }),
        backup_path: backup.map(|path| path.display().to_string()),
    })
}

/// 把账号字段覆盖到桌面明文 JSON 上，**保留其它一切字段**（模块头第 1 条）。
///
/// 只动这些键：`token` / `refreshToken` / `expiresAt` / `user.id` / `user.name`。
/// `user` 里的 `email` / `phone` / `avatarUrl` 与 `refreshTokenExpiresAt` 一律保留
/// —— 账号库里没有它们，删掉就等于把客户端登录态打残。
///
/// ⚠️ `expiresAt` 由 `RawAccount.expires_at`（毫秒）**换算成 RFC3339 字符串**：
/// 明文里它是字符串而非数字，直接写数字会让客户端解析失败。
fn apply_desktop_overrides(json: &mut Value, account: &RawAccount) -> Result<(), String> {
    let object = json
        .as_object_mut()
        .ok_or_else(|| "现有登录态不是 JSON 对象 —— 无法原地改字段。".to_string())?;

    if let Some(token) = account.access_token.as_deref().filter(|t| !t.is_empty()) {
        object.insert("token".to_string(), Value::String(token.to_string()));
    }
    if let Some(refresh) = account.refresh_token.as_deref().filter(|t| !t.is_empty()) {
        object.insert(
            "refreshToken".to_string(),
            Value::String(refresh.to_string()),
        );
    }
    if let Some(millis) = account.expires_at {
        if let Some(when) = chrono::DateTime::from_timestamp_millis(millis) {
            object.insert("expiresAt".to_string(), Value::String(when.to_rfc3339()));
        }
    }

    // `user.*`：只改 id / name，**其余子键保留**（email / phone / avatarUrl …）。
    let user = object
        .entry("user".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(user_object) = user.as_object_mut() {
        if !account.uid.is_empty() {
            user_object.insert("id".to_string(), Value::String(account.uid.clone()));
        }
        if let Some(name) = account.nickname.as_deref().filter(|n| !n.is_empty()) {
            user_object.insert("name".to_string(), Value::String(name.to_string()));
        }
    }
    Ok(())
}

/// 读回桌面登录态里的令牌（供写后校验）。
fn read_desktop_token<Dec>(path: &Path, decode: &Dec) -> Result<String, String>
where
    Dec: Fn(&[u8]) -> Result<Vec<u8>, String>,
{
    let blob = std::fs::read(path).map_err(|error| format!("写回后读取失败：{error}"))?;
    let plaintext = decode(&blob).map_err(|error| format!("写回后解密失败：{error}"))?;
    let json: Value =
        serde_json::from_slice(&plaintext).map_err(|error| format!("写回后不是 JSON：{error}"))?;
    token_of(&json).ok_or_else(|| "写回后登录态缺少 token 字段。".to_string())
}

/// 从 JSON 里取访问令牌（兼容 `token` / `accessToken` / `access_token` 三种键名）。
fn token_of(json: &Value) -> Option<String> {
    ["token", "accessToken", "access_token"]
        .iter()
        .find_map(|key| {
            json.get(*key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

// ---------------------------------------------------------------------------
// CLI 写回
// ---------------------------------------------------------------------------

/// CLI 登录态的形态（写回时**跟随既有形态**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliForm {
    /// 明文 JSON（文件以 `{` 开头）。
    Plaintext,
    /// AES-128-CBC 密文（标准 Base64 文本）。
    Encrypted,
}

/// CLI 写回：读现有明文/密文 → 原地改字段 → 按既有形态写回 → 校验 → 失败回滚。
fn switch_cli(region: QoderRegion, account: &RawAccount) -> Result<SwitchWriteOutcome, String> {
    let auth_path = paths::cli_auth_file(region);
    let machine_id_path = paths::cli_home_dir(region).join(".auth").join("machine_id");

    let existing = std::fs::read(&auth_path).ok();
    let (mut json, form) = match existing.as_deref() {
        Some(bytes) if bytes.first() == Some(&b'{') => {
            let json = serde_json::from_slice(bytes).map_err(|error| {
                format!("{} 不是可解析的 JSON（{error}）。", auth_path.display())
            })?;
            (json, CliForm::Plaintext)
        }
        Some(bytes) if !bytes.is_empty() => {
            let key = cli_key(&machine_id_path)?;
            let plaintext = aes_cbc_decrypt(bytes, &key).map_err(|error| {
                format!("无法解密现有 CLI 凭据 {}：{error}", auth_path.display())
            })?;
            let json = serde_json::from_slice(&plaintext).map_err(|error| {
                format!("{} 解密成功但不是 JSON（{error}）。", auth_path.display())
            })?;
            (json, CliForm::Encrypted)
        }
        // 文件缺失 / 为空：按官方客户端的默认形态（密文）新建。
        _ => (Value::Object(Map::new()), CliForm::Encrypted),
    };

    apply_cli_overrides(&mut json, account)?;
    let plaintext =
        serde_json::to_vec(&json).map_err(|error| format!("序列化 CLI 凭据失败：{error}"))?;

    let output = match form {
        CliForm::Plaintext => plaintext,
        CliForm::Encrypted => {
            let key = cli_key(&machine_id_path)?;
            base64_encode(&crate::modules::qoder::cosy::aes_cbc_encrypt(
                &plaintext, &key, &key,
            ))
        }
    };

    let backup = backup_auth_file_for_switch(region, QoderProgram::Cli, &auth_path);
    write_bytes(&auth_path, &output)?;

    let expected = token_of(&json).unwrap_or_default();
    let actual = read_cli_token(&auth_path, &machine_id_path);
    verify_or_rollback(&auth_path, backup.as_deref(), &expected, actual)?;

    Ok(SwitchWriteOutcome {
        written: json!({
            "path": auth_path.display().to_string(),
            "form": match form {
                CliForm::Plaintext => "plaintext",
                CliForm::Encrypted => "aes-128-cbc",
            },
            "tokenMasked": store::mask(&expected),
        }),
        backup_path: backup.map(|path| path.display().to_string()),
    })
}

/// 把账号字段覆盖到 CLI 凭据 JSON 上，**保留其它一切字段**。
///
/// 键名**跟随既有键**（`token` / `accessToken` / `access_token` 三选一，
/// 刷新令牌同理），避免把一个客户端不认识的键写进去。
fn apply_cli_overrides(json: &mut Value, account: &RawAccount) -> Result<(), String> {
    let object = json
        .as_object_mut()
        .ok_or_else(|| "现有 CLI 凭据不是 JSON 对象 —— 无法原地改字段。".to_string())?;

    if let Some(token) = account.access_token.as_deref().filter(|t| !t.is_empty()) {
        insert_at_existing_key(
            object,
            &["token", "accessToken", "access_token"],
            "token",
            token,
        );
    }
    if let Some(refresh) = account.refresh_token.as_deref().filter(|t| !t.is_empty()) {
        insert_at_existing_key(
            object,
            &["refreshToken", "refresh_token"],
            "refreshToken",
            refresh,
        );
    }

    if !account.uid.is_empty() {
        if let Some(user) = object.get_mut("user").and_then(Value::as_object_mut) {
            user.insert("id".to_string(), Value::String(account.uid.clone()));
        } else {
            object.insert("uid".to_string(), Value::String(account.uid.clone()));
        }
    }
    if let Some(name) = account.nickname.as_deref().filter(|n| !n.is_empty()) {
        if let Some(user) = object.get_mut("user").and_then(Value::as_object_mut) {
            user.insert("name".to_string(), Value::String(name.to_string()));
        }
    }
    Ok(())
}

/// 优先写**已存在**的键，否则用兜底键名。
fn insert_at_existing_key(
    object: &mut Map<String, Value>,
    candidates: &[&str],
    fallback: &str,
    value: &str,
) {
    let mut key = fallback;
    for candidate in candidates {
        if object.contains_key(*candidate) {
            key = candidate;
            break;
        }
    }
    object.insert(key.to_string(), Value::String(value.to_string()));
}

/// 读回 CLI 凭据里的令牌（按 `{` 开头判定明文/密文，与读路径同源）。
fn read_cli_token(auth_path: &Path, machine_id_path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(auth_path).map_err(|error| format!("写回后读取失败：{error}"))?;
    let plaintext = if bytes.first() == Some(&b'{') {
        bytes
    } else {
        let key = cli_key(machine_id_path)?;
        aes_cbc_decrypt(&bytes, &key).map_err(|error| format!("写回后解密失败：{error}"))?
    };
    let json: Value =
        serde_json::from_slice(&plaintext).map_err(|error| format!("写回后不是 JSON：{error}"))?;
    token_of(&json).ok_or_else(|| "写回后凭据缺少令牌字段。".to_string())
}

/// 派生 CLI 的 AES-128 密钥：`machine_id[:16]`（不足 16 位即拒绝）。
fn cli_key(machine_id_path: &Path) -> Result<[u8; 16], String> {
    let raw = std::fs::read_to_string(machine_id_path).map_err(|_| {
        format!(
            "取不到设备标识 {} —— 无法派生写回密钥。请先在 Qoder CLI 里登录一次。",
            machine_id_path.display()
        )
    })?;
    let machine_id = raw.trim();
    if machine_id.len() < 16 {
        return Err(format!(
            "设备标识过短（{} 位），无法派生写回密钥：{}",
            machine_id.len(),
            machine_id_path.display()
        ));
    }
    let mut key = [0u8; 16];
    key.copy_from_slice(&machine_id.as_bytes()[..16]);
    Ok(key)
}

/// AES-128-CBC 解密（`key == iv`，标准 Base64，PKCS7）。
///
/// ⚠️ 与 [`super::profile`] 里私有的 `decrypt_cli_blob` 是同一套算法。本模块**不能**
/// 改 `profile.rs`（它是并行的另一处写域），故在此复刻一份；两处的密钥派生
/// （`machine_id[:16]`）必须保持一致，否则写回与导入会互相读不懂。
fn aes_cbc_decrypt(blob: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, String> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

    let decoded = base64_decode(blob).map_err(|error| format!("凭据不是合法 Base64：{error}"))?;
    Aes128CbcDec::new_from_slices(key, key)
        .map_err(|error| format!("构造解密器失败：{error}"))?
        .decrypt_padded_vec_mut::<Pkcs7>(&decoded)
        .map_err(|_| "解密失败：密钥不匹配（设备标识与写入凭据时的不一致）。".to_string())
}

fn base64_encode(bytes: &[u8]) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .encode(bytes)
        .into_bytes()
}

fn base64_decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(bytes)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// 备份 / 写入 / 校验回滚
// ---------------------------------------------------------------------------

/// 写前备份到 [`crate::modules::config::backup_dir`]；返回备份路径。
///
/// 命名沿用本仓既有惯例（`backup_dir()` + 时间戳文件名，见 `auth_file.rs` 的
/// `backup_auth_file_for`），前缀 `qoder-<program>.<region>.` 以免与其它产品的备份重名。
/// 源文件不存在时返回 `None`（没有可备份的东西）。
fn backup_auth_file_for_switch(
    region: QoderRegion,
    program: QoderProgram,
    source: &Path,
) -> Option<PathBuf> {
    if !source.exists() {
        return None;
    }
    let dir = crate::modules::config::backup_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = crate::modules::config::utc_iso();
    let dest = dir.join(format!(
        "qoder-{}.{}.{}.bak",
        program.as_str(),
        region.as_str(),
        stamp
    ));
    std::fs::copy(source, &dest).ok()?;
    Some(dest)
}

/// 写字节（先建父目录，再走原子写）。
///
/// 客户端目录可能尚不存在（CLI 首次写回），而 [`crate::modules::config::atomic_write_bytes`]
/// 只保证「临时文件 + rename」，不建父目录。
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建目录 {} 失败：{error}", parent.display()))?;
    }
    crate::modules::config::atomic_write_bytes(path, bytes)
        .map_err(|error| format!("写入 {} 失败：{error}", path.display()))
}

/// 校验写回结果；不一致时**回滚到备份**并返回「已还原」的可读错误。
///
/// ## 为什么必须回滚（模块头第 3 条）
///
/// 一次「写成功、内容却不对」的切换若没有回滚，用户会被留在「既没换成、还回不去」
/// 的坏状态：客户端退登，而切换前那份好的登录态已经没了。
///
/// `actual` 是写回后**读回并解密**得到的令牌（`Err` 表示读回/解密就失败了）。
fn verify_or_rollback(
    target: &Path,
    backup: Option<&Path>,
    expected_token: &str,
    actual: Result<String, String>,
) -> Result<(), String> {
    match actual {
        Ok(actual) if actual == expected_token => Ok(()),
        Ok(actual) => {
            let reason = format!(
                "写回校验失败：写回后的令牌与预期不一致（读到 {}）。",
                store::mask(&actual)
            );
            Err(rollback_and_report(target, backup, &reason))
        }
        Err(error) => {
            let reason = format!("写回校验失败：{error}");
            Err(rollback_and_report(target, backup, &reason))
        }
    }
}

/// 把备份原样写回目标文件，并拼出带「已还原」字样的错误文案。
fn rollback_and_report(target: &Path, backup: Option<&Path>, reason: &str) -> String {
    let Some(backup) = backup else {
        return format!("{reason} 且没有可用备份，无法自动还原。");
    };
    match std::fs::read(backup) {
        Ok(bytes) => match crate::modules::config::atomic_write_bytes(target, &bytes) {
            Ok(()) => format!("{reason} 已还原为切换前的登录态。"),
            Err(error) => format!(
                "{reason} 且自动还原失败（{error}）—— 请手动从备份 {} 恢复。",
                backup.display()
            ),
        },
        Err(error) => format!(
            "{reason} 且读取备份失败（{error}）—— 请手动从备份 {} 恢复。",
            backup.display()
        ),
    }
}

/// 程序位状态视图 —— 界面用它渲染「程序位胶囊行」与「当前登录」徽章。
///
/// 形状：
/// ```json
/// { "region": "cn",
///   "programs": [ { "id": "desktop-app", "label": "Qoder 桌面版",
///                   "state": "ready|missing|unreadable|empty",
///                   "detail": null, "running": true,
///                   "currentUid": "019efdc6-…" } ] }
/// ```
///
/// ★ 四态（`state`）**必须透传**，不能压成一个布尔：`missing`（没装）/ `unreadable`
/// （装了但解不开）/ `empty`（装了没登录）/ `ready` 是**四种不同的用户动作**，
/// 压成「未检测到」会让用户去重装一个已经装好的客户端。
///
/// ★ `currentUid` 是「该程序位当前登着的账号」，`null` = 读不到。
/// 前端据此把 `currentUid == account.uid` 的那张卡标成「当前登录」——
/// **判断放在前端**（同一个 `currentUid` 要跟列表里每条账号比），后端只给事实。
///
/// ⚠️ `running` 单独给：客户端在跑时**不能**切换（退出时会用内存态覆盖我们写的），
/// 界面要据此把「切换」按钮置灰并说明原因 —— 而不是让用户点了才失败。
pub fn program_status_for(region: QoderRegion) -> Value {
    let probes = super::profile::probe_local_sources(region);
    let programs: Vec<Value> = QoderProgram::all()
        .into_iter()
        .map(|program| {
            let kind = program.as_str();
            let probe = probes.iter().find(|probe| probe.kind == kind);
            json!({
                "id": kind,
                "label": program.label(),
                "state": probe.map(|p| p.state.as_str()).unwrap_or("missing"),
                "detail": probe.and_then(|p| p.state.detail()),
                "running": program_running(region, program),
                "currentUid": super::profile::current_uid_for(region, kind),
            })
        })
        .collect();
    json!({ "region": region.as_str(), "programs": programs })
}

#[cfg(test)]
// 同 `trae/profile.rs`：本模块的 fixture 辅助只被 `#[cfg(windows)]` 用例消费，
// Linux / macOS 上必然「未使用」，而 CI 的 `cargo test` 带 `-D warnings`。
#[cfg_attr(not(windows), allow(dead_code))]
mod tests {
    use super::*;
    use crate::modules::qoder::account::{AccountsFile, SOURCE_DESKTOP_APP};
    use crate::modules::qoder::profile::{self, LocalState};

    /// 隔离后的临时环境：`BUDDY_SWITCH_HOME`（走 [`crate::modules::config::HomeOverrideGuard`]）
    /// 与 `APPDATA`（Windows 上 [`paths::roaming_dir`] 读它）都指向本实例私有目录。
    ///
    /// ## 为什么必须同时隔离两个变量
    ///
    /// 桌面版路径由 `%APPDATA%` 派生、CLI 路径与备份目录由 home 派生；只隔离其中一个
    /// 会让用例读到**真机**的另一半（真机上恰好有数据时「碰巧」通过）。两个变量都改了，
    /// 才能真正断言「写到了哪」。
    ///
    /// ## 为什么 `Drop` 里先还原变量、再删目录
    ///
    /// 变量若继续指向一个已删除的目录，后续用例会吃一次「已忽略」警告并**静默回落真实
    /// home**（那正是把测试数据写进用户真实目录的形态）。
    struct TestEnv {
        /// 只为持有 `env_lock` 与 `BUDDY_SWITCH_HOME` 的还原；不直接读取。
        _home: crate::modules::config::HomeOverrideGuard,
        root: PathBuf,
        previous_appdata: Option<std::ffi::OsString>,
    }

    impl TestEnv {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "buddy-switch-qoder-switch-{}",
                uuid::Uuid::new_v4().simple()
            ));
            let home = root.join("home");
            let appdata = root.join("appdata");
            std::fs::create_dir_all(&home).expect("临时 home 应能创建");
            std::fs::create_dir_all(&appdata).expect("临时 appdata 应能创建");

            let guard = crate::modules::config::HomeOverrideGuard::set(&home);
            let previous_appdata = std::env::var_os("APPDATA");
            std::env::set_var("APPDATA", &appdata);

            Self {
                _home: guard,
                root,
                previous_appdata,
            }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            match self.previous_appdata.take() {
                Some(value) => std::env::set_var("APPDATA", value),
                None => std::env::remove_var("APPDATA"),
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 账号 fixture：桌面版来源，带访问/刷新令牌。
    fn account(uid: &str, access: &str, refresh: &str) -> RawAccount {
        RawAccount {
            uid: uid.to_string(),
            nickname: Some("新昵称".to_string()),
            domain: "qoder.com.cn".to_string(),
            access_token: Some(access.to_string()),
            refresh_token: Some(refresh.to_string()),
            source: Some(SOURCE_DESKTOP_APP.to_string()),
            ..Default::default()
        }
    }

    /// 把账号写进指定区域的账号库（切换要从库里查它）。
    fn store_account(region: QoderRegion, raw: RawAccount) {
        account::save_accounts_for(
            region,
            &AccountsFile {
                accounts: vec![raw],
            },
        )
        .expect("账号库应能写入");
    }

    // ── 进程替身 ──

    /// 桌面版「可逆但非确定性」的编解码替身：密文 = `前缀 + 随机 16B + 明文`。
    ///
    /// 用它模拟真实 `v10` 的**随机 nonce**：同一明文两次加密得到不同密文，因此
    /// 「幂等」只能在**明文层**断言（模块头第 4 条）。
    fn nonce_encode(plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = b"v10".to_vec();
        out.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        out.extend_from_slice(plain);
        Ok(out)
    }

    fn nonce_decode(blob: &[u8]) -> Result<Vec<u8>, String> {
        if blob.len() < 19 {
            return Err("密文过短".to_string());
        }
        Ok(blob[19..].to_vec())
    }

    /// 恒等编解码替身（明文原样进、原样出）。
    fn identity_encode(plain: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plain.to_vec())
    }

    fn identity_decode(blob: &[u8]) -> Result<Vec<u8>, String> {
        Ok(blob.to_vec())
    }

    /// 在临时环境里铺一份桌面登录态明文，返回其路径。
    fn seed_desktop(region: QoderRegion, plaintext: &str) -> PathBuf {
        let path = paths::desktop_auth_file(region);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, plaintext.as_bytes()).unwrap();
        path
    }

    /// 一份「含未知字段」的桌面明文（照实测 schema 写，多塞两个未知键）。
    const DESKTOP_PLAINTEXT: &str = r#"{"schemaVersion":1,"token":"dt-old","refreshToken":"drt-old","expiresAt":"2026-01-01T00:00:00+00:00","refreshTokenExpiresAt":"2026-02-01T00:00:00+00:00","user":{"id":"u1","name":"旧名","email":"a@b.c","phone":"13800000000","avatarUrl":"https://x/y.png"},"unknownField":{"nested":[1,2,3]},"anotherUnknown":"keep-me"}"#;

    // ── QoderProgram ──

    /// 解析 **fail-closed**：认不出的一律 `None`，不回落默认值。
    ///
    /// 可证伪：把 `parse` 的 `_` 分支改成 `Some(QoderProgram::Desktop)`，后四条断言变红
    /// —— 那正是「前端传错程序位却被静默写成桌面版」的形态。
    #[test]
    fn program_parse_is_fail_closed() {
        assert_eq!(
            QoderProgram::parse("desktop-app"),
            Some(QoderProgram::Desktop)
        );
        assert_eq!(QoderProgram::parse("cli"), Some(QoderProgram::Cli));
        // ★ 不接受「看起来像」的旧写法 —— 两套 id 并存正是本条护栏要防的。
        assert_eq!(QoderProgram::parse("desktop"), None);
        assert_eq!(
            QoderProgram::parse("  DESKTOP-APP  "),
            Some(QoderProgram::Desktop),
            "允许首尾空白与大小写"
        );
        for bad in ["", "   ", "gui", "desk", "cl"] {
            assert_eq!(QoderProgram::parse(bad), None, "{bad:?} 不该解析出程序位");
        }
        assert_eq!(QoderProgram::all().len(), 2);
    }

    /// 展示名与 `profile.rs` 的来源标签**逐字一致**（否则界面会把同一个程序位显示成两个名字）。
    ///
    /// 可证伪：把 `label` 的任一支改成别的字面量，本用例变红。
    #[test]
    fn program_labels_match_import_source_labels() {
        assert_eq!(QoderProgram::Desktop.label(), "Qoder 桌面版");
        assert_eq!(QoderProgram::Cli.label(), "Qoder CLI");
        // ★★ 与来源 id **逐字相同**：程序位按这个 id 去查来源表，不同就永远落空
        //    （真机踩过：桌面版明明登着，界面显示「未检测到」）。
        assert_eq!(
            QoderProgram::Desktop.as_str(),
            super::super::profile::KIND_DESKTOP
        );
        assert_eq!(QoderProgram::Cli.as_str(), super::super::profile::KIND_CLI);
        assert_eq!(QoderProgram::Desktop.as_str(), "desktop-app");
    }

    // ── 进程守卫 ──

    /// 守卫**查的是正确的映像名**，且判据由注入闭包决定。
    ///
    /// 可证伪：把 `program_running_with` 里的 `program_image(...)` 换成常量，
    /// 下面的映像名断言变红 —— 那正是「守卫查了个不存在的进程名、形同虚设」的形态。
    #[test]
    fn guard_probes_the_right_image_and_follows_the_probe() {
        let mut seen = String::new();
        let running = program_running_with(QoderRegion::Cn, QoderProgram::Desktop, |image| {
            seen = image.to_string();
            true
        });
        assert!(running, "注入 true ⇒ 视为在运行");
        assert_eq!(seen, "Qoder CN.exe", "国内版桌面版映像名");

        let mut seen = String::new();
        let running = program_running_with(QoderRegion::Global, QoderProgram::Desktop, |image| {
            seen = image.to_string();
            false
        });
        assert!(!running, "注入 false ⇒ 视为未运行");
        assert_eq!(seen, "Qoder.exe", "国际版桌面版映像名");
    }

    /// 客户端在跑 ⇒ **拒绝写**，且错误文案要说清「请先退出 X 客户端」。
    ///
    /// 可证伪：把 `switch_account_for_with` 里的守卫分支删掉，本用例变红
    /// （在客户端运行时写回，会被它退出时的内存态覆盖）。
    #[test]
    fn switch_is_refused_while_the_client_is_running() {
        let _env = TestEnv::new();
        store_account(QoderRegion::Cn, account("u1", "dt-new", "drt-new"));
        seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);

        let error = switch_account_for_with(QoderRegion::Cn, QoderProgram::Desktop, "u1", |_| true)
            .unwrap_err();
        assert!(error.contains("请先退出"), "{error}");
        assert!(error.contains("Qoder 桌面版"), "{error}");

        // 且文件**未被改动**（守卫在写之前生效）。
        let after = std::fs::read_to_string(paths::desktop_auth_file(QoderRegion::Cn)).unwrap();
        assert_eq!(after, DESKTOP_PLAINTEXT);
    }

    // ── 桌面版：保留未知字段（本模块最重要的判据）──

    /// ★★ 桌面写回**保留未知字段**。
    ///
    /// 可证伪：把 `apply_desktop_overrides` 改成「从 `RawAccount` 重建一份 JSON」，
    /// 本用例立刻变红 —— `unknownField` / `anotherUnknown` / `user.email` /
    /// `refreshTokenExpiresAt` 会全部消失，客户端可能据此判登录态无效。
    #[test]
    fn desktop_writeback_preserves_unknown_fields() {
        let _env = TestEnv::new();
        let path = seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);
        let raw = account("u1", "dt-new", "drt-new");

        switch_desktop(QoderRegion::Cn, &raw, identity_encode, identity_decode)
            .expect("写回应成功");

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        // 已知字段被覆盖。
        assert_eq!(after["token"], json!("dt-new"));
        assert_eq!(after["refreshToken"], json!("drt-new"));
        assert_eq!(after["user"]["id"], json!("u1"));
        assert_eq!(after["user"]["name"], json!("新昵称"));

        // 未知 / 账号库装不下的字段**原样保留**。
        assert_eq!(after["unknownField"], json!({"nested":[1,2,3]}));
        assert_eq!(after["anotherUnknown"], json!("keep-me"));
        assert_eq!(after["schemaVersion"], json!(1));
        assert_eq!(after["expiresAt"], json!("2026-01-01T00:00:00+00:00"));
        assert_eq!(
            after["refreshTokenExpiresAt"],
            json!("2026-02-01T00:00:00+00:00")
        );
        assert_eq!(after["user"]["email"], json!("a@b.c"));
        assert_eq!(after["user"]["phone"], json!("13800000000"));
        assert_eq!(after["user"]["avatarUrl"], json!("https://x/y.png"));
    }

    /// `expires_at`（毫秒）必须被换算成 **RFC3339 字符串**再写进 `expiresAt`。
    ///
    /// 可证伪：把 `apply_desktop_overrides` 里的 RFC3339 换算换成直接写数字，
    /// 本用例变红（明文里 `expiresAt` 是字符串，写数字客户端解析不了）。
    #[test]
    fn desktop_expiry_is_written_as_rfc3339_string() {
        let _env = TestEnv::new();
        let path = seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);
        let mut raw = account("u1", "dt-new", "drt-new");
        raw.expires_at = Some(1_800_000_000_000); // 2027-01-15T08:00:00Z

        switch_desktop(QoderRegion::Cn, &raw, identity_encode, identity_decode).unwrap();

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let expires = after["expiresAt"].as_str().expect("expiresAt 必须是字符串");
        assert!(expires.starts_with("2027-01-15"), "{expires}");
        assert!(expires.contains('T') && expires.contains(':'), "{expires}");
    }

    /// ★ 幂等性只能在**明文层**断言：写两次密文不同（随机 nonce），明文逐字段相等。
    ///
    /// 可证伪：把比较换成「密文字节相等」，本用例在第二次写入后变红 ——
    /// GCM 每次用随机 nonce，密文必然不同（模块头第 4 条）。
    #[test]
    fn desktop_writeback_is_idempotent_at_the_plaintext_layer() {
        let _env = TestEnv::new();
        // 用同一个 nonce 编解码器的密文形态铺底（切换会先解码它）。
        let path = paths::desktop_auth_file(QoderRegion::Cn);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, nonce_encode(DESKTOP_PLAINTEXT.as_bytes()).unwrap()).unwrap();
        let raw = account("u1", "dt-new", "drt-new");

        switch_desktop(QoderRegion::Cn, &raw, nonce_encode, nonce_decode).unwrap();
        let first_bytes = std::fs::read(&path).unwrap();
        let first_plain = nonce_decode(&first_bytes).unwrap();

        switch_desktop(QoderRegion::Cn, &raw, nonce_encode, nonce_decode).unwrap();
        let second_bytes = std::fs::read(&path).unwrap();
        let second_plain = nonce_decode(&second_bytes).unwrap();

        assert_ne!(first_bytes, second_bytes, "随机 nonce ⇒ 密文必然不同");
        let first_json: Value = serde_json::from_slice(&first_plain).unwrap();
        let second_json: Value = serde_json::from_slice(&second_plain).unwrap();
        assert_eq!(first_json, second_json, "明文层必须逐字段相等");
    }

    // ── 校验失败 ⇒ 回滚 ──

    /// ★ 校验不一致 ⇒ **把备份原样写回**，错误文案含「已还原」。
    ///
    /// 可证伪：把 `rollback_and_report` 里 `atomic_write_bytes(target, &bytes)` 那一步删掉，
    /// 本用例变红（目标文件仍是「写坏」的内容，用户回不去）。
    #[test]
    fn verify_failure_restores_the_backup() {
        let _env = TestEnv::new();
        let path = seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);

        // 走真实的备份函数，拿到「切换前」的底本。
        let backup = backup_auth_file_for_switch(QoderRegion::Cn, QoderProgram::Desktop, &path)
            .expect("源文件存在 ⇒ 必有备份");

        // 模拟一次「写成功但内容不对」：目标被改成别的字节。
        std::fs::write(&path, b"BROKEN-CONTENT").unwrap();

        let error = verify_or_rollback(&path, Some(&backup), "dt-expected", Ok("dt-actual".into()))
            .unwrap_err();
        assert!(error.contains("已还原"), "{error}");

        // 目标被还原成切换前的原文。
        let restored = std::fs::read_to_string(&path).unwrap();
        assert_eq!(restored, DESKTOP_PLAINTEXT, "必须还原为切换前的登录态");
    }

    /// 读回/解密就失败（`Err`）时同样回滚，而不是把坏文件留在原地。
    #[test]
    fn verify_error_also_rolls_back() {
        let _env = TestEnv::new();
        let path = seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);
        let backup =
            backup_auth_file_for_switch(QoderRegion::Cn, QoderProgram::Desktop, &path).unwrap();
        std::fs::write(&path, b"BROKEN-CONTENT").unwrap();

        let error = verify_or_rollback(
            &path,
            Some(&backup),
            "dt-expected",
            Err("解密失败".to_string()),
        )
        .unwrap_err();
        assert!(error.contains("已还原"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DESKTOP_PLAINTEXT);
    }

    // ── 区域 fail-closed ──

    /// 账号的 `domain` 不属于目标区域 ⇒ 拒绝（用 [`account::ensure_region_matches`]）。
    ///
    /// 可证伪：删掉 `switch_account_for_with` 里的 `ensure_region_matches` 调用，
    /// 本用例变红 —— 那正是「国际版账号被写进国内版登录态」的形态。
    #[test]
    fn region_mismatch_is_rejected() {
        let _env = TestEnv::new();
        let mut foreign = account("u-global", "dt-new", "drt-new");
        foreign.domain = "qoder.com".to_string(); // 国际版域名
        store_account(QoderRegion::Cn, foreign); // 硬塞进国内版库
        seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);

        let error =
            switch_account_for_with(QoderRegion::Cn, QoderProgram::Desktop, "u-global", |_| {
                false
            })
            .unwrap_err();
        assert!(error.contains("qoder.com"), "{error}");
        assert!(error.contains("qoder.com.cn"), "{error}");
    }

    // ── 返回值不含令牌明文 ──

    /// 下发结果**不含任何令牌明文**，只给掩码。
    ///
    /// 走 **CLI** 程序位：它的写回不依赖系统密钥环（桌面版要走 DPAPI / Keychain，
    /// 测试环境无法铺出合法的密钥环条目），因此这条「公开入口返回值」的断言可在
    /// 任意平台稳定跑。
    ///
    /// 可证伪：把 `tokenMasked` 换成原始 token，本用例变红。
    #[test]
    fn switch_result_never_leaks_the_token() {
        let _env = TestEnv::new();
        seed_machine_id(QoderRegion::Cn, "0123456789abcdef");
        store_account(
            QoderRegion::Cn,
            account("u1", "dt-abcdefghijklmnop", "drt-abcdefghijklmnop"),
        );
        let auth = paths::cli_auth_file(QoderRegion::Cn);
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        std::fs::write(&auth, br#"{"token":"dt-old-token","user":{"id":"u1"}}"#).unwrap();

        let result =
            switch_account_for_with(QoderRegion::Cn, QoderProgram::Cli, "u1", |_| false).unwrap();
        let text = serde_json::to_string(&result).unwrap();
        assert!(
            !text.contains("dt-abcdefghijklmnop"),
            "泄露了访问令牌：{text}"
        );
        assert!(
            !text.contains("drt-abcdefghijklmnop"),
            "泄露了刷新令牌：{text}"
        );
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["program"], json!("cli"));
        assert_eq!(result["region"], json!("cn"));
        assert_eq!(result["uid"], json!("u1"));
        assert!(result["written"]["tokenMasked"]
            .as_str()
            .unwrap()
            .contains('…'));
    }

    // ── CLI：明文分支与加密分支 ──

    /// 在临时环境里铺 CLI 的 `machine_id`，返回其路径。
    fn seed_machine_id(region: QoderRegion, mid: &str) -> PathBuf {
        let path = paths::cli_home_dir(region).join(".auth").join("machine_id");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, mid).unwrap();
        path
    }

    /// CLI **明文分支**：原文件是明文 ⇒ 写回仍是明文，且未知字段保留、令牌被更新；
    /// 写出的内容能被 `profile` 的**读路径**读到（`probe_cli` ⇒ `Ready`）。
    #[test]
    fn cli_plaintext_branch_round_trips() {
        let _env = TestEnv::new();
        seed_machine_id(QoderRegion::Cn, "0123456789abcdef");
        store_account(
            QoderRegion::Cn,
            account("u1", "dt-new-token", "drt-new-token"),
        );

        let auth = paths::cli_auth_file(QoderRegion::Cn);
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        std::fs::write(
            &auth,
            br#"{"token":"dt-old-token","refreshToken":"drt-old-token","unknownField":"keep","user":{"id":"u1","email":"a@b.c"}}"#,
        )
        .unwrap();

        switch_account_for_with(QoderRegion::Cn, QoderProgram::Cli, "u1", |_| false).unwrap();

        // 仍是明文（跟随既有形态）。
        let bytes = std::fs::read(&auth).unwrap();
        assert_eq!(bytes.first(), Some(&b'{'), "明文分支应写回明文");
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["token"], json!("dt-new-token"));
        assert_eq!(json["refreshToken"], json!("drt-new-token"));
        assert_eq!(json["unknownField"], json!("keep"), "未知字段必须保留");
        assert_eq!(json["user"]["email"], json!("a@b.c"), "user 子字段必须保留");

        // 用 profile 的读路径验证「客户端读得懂」。
        assert_eq!(profile::probe_cli(QoderRegion::Cn).state, LocalState::Ready);
    }

    /// CLI **加密分支**：原文件是 AES-128-CBC 密文 ⇒ 写回仍是密文，且能被标准
    /// AES-128-CBC+PKCS7（`key = iv = machine_id[:16]`）解出、令牌已更新、未知字段保留。
    #[test]
    fn cli_encrypted_branch_round_trips() {
        let _env = TestEnv::new();
        let mid_path = seed_machine_id(QoderRegion::Cn, "fedcba9876543210");
        store_account(
            QoderRegion::Cn,
            account("u1", "dt-new-token", "drt-new-token"),
        );

        // 用 cosy 的加密器造一份合法密文（与 CLI 写路径同源）。
        let mut key = [0u8; 16];
        key.copy_from_slice(&"fedcba9876543210".as_bytes()[..16]);
        let plain = br#"{"token":"dt-old-token","refreshToken":"drt-old-token","unknownField":"keep","user":{"id":"u1"}}"#;
        let ciphertext = crate::modules::qoder::cosy::aes_cbc_encrypt(plain, &key, &key);
        let auth = paths::cli_auth_file(QoderRegion::Cn);
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        std::fs::write(&auth, base64_encode(&ciphertext)).unwrap();

        switch_account_for_with(QoderRegion::Cn, QoderProgram::Cli, "u1", |_| false).unwrap();

        // 仍是密文（首字节不是 `{`），且能被标准 AES-128-CBC 解出。
        let bytes = std::fs::read(&auth).unwrap();
        assert_ne!(bytes.first(), Some(&b'{'), "加密分支应写回密文");
        let decrypted = aes_cbc_decrypt(&bytes, &key).expect("写出的密文应能标准解密");
        let json: Value = serde_json::from_slice(&decrypted).unwrap();
        assert_eq!(json["token"], json!("dt-new-token"));
        assert_eq!(json["unknownField"], json!("keep"), "未知字段必须保留");

        // 用 profile 的读路径验证（它会用同一个 machine_id 解密）。
        assert_eq!(profile::probe_cli(QoderRegion::Cn).state, LocalState::Ready);
        // machine_id 文件本身不该被改写。
        assert_eq!(
            std::fs::read_to_string(&mid_path).unwrap().trim(),
            "fedcba9876543210"
        );
    }

    /// CLI 凭据是密文但取不到 `machine_id` ⇒ 拒绝，且**不动原文件**。
    #[test]
    fn cli_encrypted_without_machine_id_is_refused() {
        let _env = TestEnv::new();
        store_account(
            QoderRegion::Cn,
            account("u1", "dt-new-token", "drt-new-token"),
        );

        let auth = paths::cli_auth_file(QoderRegion::Cn);
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        std::fs::write(&auth, b"QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=").unwrap();

        let error = switch_account_for_with(QoderRegion::Cn, QoderProgram::Cli, "u1", |_| false)
            .unwrap_err();
        assert!(error.contains("设备标识"), "{error}");
        assert_eq!(
            std::fs::read(&auth).unwrap(),
            b"QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=",
            "拒绝时不得改动原文件"
        );
    }

    /// 没有访问令牌的账号（例如只有 PAT）⇒ 拒绝写回，文案说清原因。
    #[test]
    fn account_without_access_token_is_refused() {
        let _env = TestEnv::new();
        let mut pat_only = account("u-pat", "", "");
        pat_only.access_token = None;
        pat_only.refresh_token = None;
        pat_only.personal_token = Some("pt-xyz".to_string());
        store_account(QoderRegion::Cn, pat_only);
        seed_desktop(QoderRegion::Cn, DESKTOP_PLAINTEXT);

        let error =
            switch_account_for_with(QoderRegion::Cn, QoderProgram::Desktop, "u-pat", |_| false)
                .unwrap_err();
        assert!(error.contains("访问令牌"), "{error}");
    }

    /// 桌面登录态文件不存在 ⇒ 给出「先去客户端登录一次」的可读错误。
    #[test]
    fn missing_desktop_login_state_is_reported() {
        let _env = TestEnv::new();
        store_account(QoderRegion::Cn, account("u1", "dt-new", "drt-new"));

        let error =
            switch_account_for_with(QoderRegion::Cn, QoderProgram::Desktop, "u1", |_| false)
                .unwrap_err();
        assert!(error.contains("登录一次"), "{error}");
    }

    /// 写前备份落在 `backup_dir()` 下，且返回值里给出 `backupPath`（内容 = 切换前原文）。
    ///
    /// 同样走 **CLI**（理由见 `switch_result_never_leaks_the_token`）。
    #[test]
    fn writeback_reports_the_backup_path() {
        let _env = TestEnv::new();
        seed_machine_id(QoderRegion::Cn, "0123456789abcdef");
        store_account(
            QoderRegion::Cn,
            account("u1", "dt-new-token", "drt-new-token"),
        );
        let auth = paths::cli_auth_file(QoderRegion::Cn);
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        let original = br#"{"token":"dt-old-token","user":{"id":"u1"}}"#;
        std::fs::write(&auth, original).unwrap();

        let result =
            switch_account_for_with(QoderRegion::Cn, QoderProgram::Cli, "u1", |_| false).unwrap();
        let backup_path = result["backupPath"].as_str().expect("应有备份路径");
        let backup = PathBuf::from(backup_path);
        assert!(
            backup.starts_with(crate::modules::config::backup_dir()),
            "{backup_path}"
        );
        assert!(backup.exists(), "备份文件应真的存在");
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            original,
            "备份内容 = 切换前的原文"
        );
    }

    /// 空 uid 直接拒绝（uid 是账号库唯一键）。
    #[test]
    fn blank_uid_is_rejected() {
        let _env = TestEnv::new();
        let error =
            switch_account_for_with(QoderRegion::Cn, QoderProgram::Desktop, "   ", |_| false)
                .unwrap_err();
        assert!(error.contains("uid"), "{error}");
    }

    /// 桌面版：真实加解密入口在**缺 `Local State`** 时给出可读错误（而不是 panic 或静默）。
    ///
    /// 只能在 Windows 上跑：那里的 `encrypt_electron_safe_storage` 才会去读 `Local State`。
    /// 造不出一份**合法**的 `Local State`（那需要 `CryptProtectData`，本仓刻意不引入），
    /// 因此这里只钉「错误路径」——它至少证明薄包装确实接到了真实的 DPAPI 取密钥流程。
    #[cfg(windows)]
    #[test]
    fn desktop_encrypt_entry_reports_missing_local_state() {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-qoder-nostate-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let error = crate::modules::vscode_cn_inject::encrypt_electron_safe_storage(
            b"{}",
            &dir,
            DESKTOP_LABEL,
            realm_spec(QoderRegion::Cn).macos_keychain_service,
            LINUX_SECRET_TOOL_NAMES,
        )
        .unwrap_err();
        assert!(error.contains("Local State"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
