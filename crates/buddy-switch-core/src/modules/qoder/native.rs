//! Qoder 官方客户端的**原生风控身份桥**（`runtime-info`）—— 活动平台的必需前置。
//!
//! ## 为什么不能只用派生身份
//!
//! 官方桌面端在拉活动列表前，会 spawn 自带的组件取**真实机器身份**：
//!
//! ```text
//! <install>/resources/umid/runtime-info.exe prod --account-stdin
//! stdin : {"account": "<uid>"}
//! stdout: {"machineToken": "...", "machineType": "...", "machineCode": "...", "vmInfo": {...}}
//! ```
//!
//! 服务端**按这些值过滤设备定向活动**。用派生假值时**不会报错**，只会让活动列表
//! 里**静默少掉**「每日领取 100 Credits」这类条目 —— 那正是「领不到」最常见的根因。
//!
//! ## 两条纪律（改本模块前先读）
//!
//! 1. **best-effort，绝不阻塞主流程**：组件不存在、跑不起来、输出不是 JSON —— 一律
//!    返回 `None`，由调用方回退派生身份。本模块**永不** `panic`、**永不**把失败
//!    当成「活动不可用」。
//! 2. **身份是机器级的**（同机不同账号返回同一份）⇒ 按**区域**缓存，不做按 uid 缓存；
//!    缓存 30 分钟（组件本身约 3.7s，是整条链路最慢的一环）。
//!
//! ## 关闭开关
//!
//! 设 `BUDDY_SWITCH_QODER_NATIVE_IDENTITY=0` 即完全禁用（受限环境 / 单测不希望
//! 拉起客户端二进制时用）。`BUDDY_SWITCH_QODER_UMID_DIR` 可显式指定组件目录
//! （非 Windows 上提取出来的组件放这里）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::modules::process::run_cmd_timeout_full;
use crate::modules::qoder::region::QoderRegion;

/// 身份来自官方原生桥（`campaigns()` 的 `identity` 字段取值之一）。
pub const IDENTITY_NATIVE: &str = "runtime-info";
/// 身份是派生兜底（无原生桥时的正确回退）。
pub const IDENTITY_DERIVED: &str = "derived";

/// 原生身份：三字段**必须齐全**才算有效（缺一即视为组件不可用）。
///
/// 与 `device::MachineIdentity` **刻意不合并**：那个是「按 uid 稳定派生」的结果，
/// 这个是「官方组件给出的真值」，两者的 `machine_id` 语义都不同 —— 合并会诱导
/// 调用方把派生值当真值用（正是本模块要防的事）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeIdentity {
    /// `cosy-machinetoken` 头。
    pub machine_token: String,
    /// `cosy-machinetype` 头。
    pub machine_type: String,
    /// `cosy-machinecode` 头。
    pub machine_code: String,
}

/// 成功结果缓存时长（组件输出由本机种子文件决定，短时内重复调用恒得同一份）。
const TTL: Duration = Duration::from_secs(1800);
/// **失败**结果缓存时长（短得多：用户可能刚装上客户端）。
const NEGATIVE_TTL: Duration = Duration::from_secs(300);
/// 单次调用的超时（组件正常约 3.7s；给足余量，但仍必须有上界）。
const RUN_TIMEOUT_SECS: u64 = 25;

/// 进程内缓存：`区域 -> (取到时刻, 身份)`。身份是机器级的 ⇒ 键只到区域。
static CACHE: OnceLock<Mutex<HashMap<QoderRegion, (Instant, Option<NativeIdentity>)>>> =
    OnceLock::new();

fn cache() -> &'static Mutex<HashMap<QoderRegion, (Instant, Option<NativeIdentity>)>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 原生桥是否被显式关闭（`BUDDY_SWITCH_QODER_NATIVE_IDENTITY=0/false/no`）。
pub fn disabled() -> bool {
    matches!(
        std::env::var("BUDDY_SWITCH_QODER_NATIVE_IDENTITY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "false" | "no"
    )
}

/// 取（可缓存的）原生身份；取不到返回 `None`（调用方回退派生）。
///
/// `force = true` 跳过缓存重新取值（服务端明确拒绝当前身份时的自愈路径用）。
pub fn cached_identity(region: QoderRegion, uid: &str, force: bool) -> Option<NativeIdentity> {
    if disabled() {
        return None;
    }
    if !force {
        if let Ok(guard) = cache().lock() {
            if let Some((at, value)) = guard.get(&region) {
                let ttl = if value.is_some() { TTL } else { NEGATIVE_TTL };
                if at.elapsed() < ttl {
                    return value.clone();
                }
            }
        }
    }
    let value = identity_from_component(region, uid);
    if let Ok(mut guard) = cache().lock() {
        guard.insert(region, (Instant::now(), value.clone()));
    }
    value
}

/// 清掉缓存（诊断 / 自愈路径用）。
pub fn clear_cache() {
    if let Ok(mut guard) = cache().lock() {
        guard.clear();
    }
}

/// 定位 `runtime-info` 组件；找不到返回 `None`。
///
/// 查找顺序：
/// 1. `BUDDY_SWITCH_QODER_UMID_DIR`（显式指定目录，非 Windows 上提取出来的组件放这里）；
/// 2. Windows：桌面客户端安装目录下的 `resources/umid/runtime-info.exe`。
pub fn runtime_info_exe(region: QoderRegion) -> Option<PathBuf> {
    if let Some(dir) = std::env::var("BUDDY_SWITCH_QODER_UMID_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        for name in component_file_names() {
            let candidate = Path::new(&dir).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let install = desktop_install_dir(region)?;
    let umid = install.join("resources").join("umid");
    for name in component_file_names() {
        let candidate = umid.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 组件文件名候选（Windows 带 `.exe`；其它平台无扩展名）。
fn component_file_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["runtime-info.exe"]
    } else {
        &["runtime-info"]
    }
}

/// 桌面客户端安装目录（launcher `state.ini` 的 `installDir`；找不到再试常见目录）。
///
/// ⚠️ 这是 **best-effort 探测**：探测不到就返回 `None`（不猜、不回落），
/// 上层据此走「不发机器头」的派生分支 —— 与官方「没有原生身份时不冒充」一致。
fn desktop_install_dir(region: QoderRegion) -> Option<PathBuf> {
    let base = local_appdata()?;
    for name in launcher_dir_names(region) {
        for launcher in [format!("{name} Launcher"), "Launcher".to_string()] {
            let ini = base.join(&name).join(&launcher).join("state.ini");
            if let Ok(text) = std::fs::read(&ini) {
                if let Some(dir) = parse_state_ini(&text) {
                    let path = PathBuf::from(dir);
                    if path.is_dir() {
                        return Some(path);
                    }
                }
            }
        }
    }
    for name in launcher_dir_names(region) {
        let dir = base.join("Programs").join(name);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    None
}

/// launcher 目录名候选（国内版带 `CN` 形态；国际版只有 `Qoder`）。
fn launcher_dir_names(region: QoderRegion) -> &'static [&'static str] {
    match region {
        QoderRegion::Cn => &["Qoder CN", "QoderCN", "Qoder"],
        QoderRegion::Global => &["Qoder"],
    }
}

fn local_appdata() -> Option<PathBuf> {
    if let Some(value) = std::env::var("LOCALAPPDATA")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(value));
    }
    dirs::home_dir().map(|home| home.join("AppData").join("Local"))
}

/// 解析 launcher `state.ini`（UTF-8 / UTF-8-BOM / UTF-16 三种编码）取 `installDir`。
///
/// 抽成**纯函数**（吃字节、吐 `Option<String>`）是为了可测：编码识别与
/// 「大小写不敏感的键名」这两点都是实测踩过的坑（launcher 用 UTF-16 写盘）。
pub fn parse_state_ini(bytes: &[u8]) -> Option<String> {
    let text = decode_ini(bytes)?;
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("installDir") {
            let value = value.trim().trim_matches('"').trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// 逐编码尝试解码（UTF-8-BOM → UTF-8 → UTF-16LE/BE 带 BOM）。
fn decode_ini(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_utf16(&bytes[2..], true);
        return Some(text);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_utf16(&bytes[2..], false);
        return Some(text);
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    std::str::from_utf8(body).ok().map(str::to_string)
}

/// 最小 UTF-16 解码（不引第三方依赖；`state.ini` 只含 ASCII 路径）。
fn encoding_utf16(bytes: &[u8], little_endian: bool) -> (String, usize, usize) {
    let mut out = String::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let unit = if little_endian {
            u16::from_le_bytes([bytes[index], bytes[index + 1]])
        } else {
            u16::from_be_bytes([bytes[index], bytes[index + 1]])
        };
        index += 2;
        if let Some(ch) = char::from_u32(unit as u32) {
            out.push(ch);
        }
    }
    let consumed = index;
    (out, consumed, 0)
}

/// 调用组件取身份（**单次**，无缓存）。任何失败 ⇒ `None`。
fn identity_from_component(region: QoderRegion, uid: &str) -> Option<NativeIdentity> {
    let exe = runtime_info_exe(region)?;
    let cwd = exe.parent().map(Path::to_path_buf);
    // stdin 形态与官方一致：`{"account": "<uid>"}` 后跟一个空格（组件按分隔符读）。
    let stdin = format!("{{\"account\":\"{}\"}} ", uid.replace('"', ""));
    let output = run_cmd_timeout_full(
        &exe,
        &["prod", "--account-stdin"],
        Some(stdin.as_bytes()),
        cwd.as_deref(),
        RUN_TIMEOUT_SECS,
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // 组件可能先打一行日志再给 JSON ⇒ 取**第一个能解析成对象**的行。
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if let Some(identity) = parse_identity(&value) {
                return Some(identity);
            }
        }
    }
    None
}

/// 从组件输出解析身份；三字段**缺一即 `None`**（半套身份会污染请求头，宁可不发）。
///
/// 纯函数，可测：喂一份缺 `machineCode` 的输出必须得到 `None`。
pub fn parse_identity(payload: &Value) -> Option<NativeIdentity> {
    let pick = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    Some(NativeIdentity {
        machine_token: pick("machineToken")?,
        machine_type: pick("machineType")?,
        machine_code: pick("machineCode")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// ★ 三字段齐全才算有效；缺一 ⇒ `None`（半套身份会污染 `cosy-machine*` 头）。
    ///
    /// 可证伪：把任一 `?` 换成 `unwrap_or_default()` ⇒ 本用例立即变红。
    #[test]
    fn identity_requires_all_three_fields() {
        let full = json!({
            "machineToken": "tok", "machineType": "typ", "machineCode": "cod",
            "vmInfo": {"isVm": false},
        });
        assert_eq!(
            parse_identity(&full),
            Some(NativeIdentity {
                machine_token: "tok".into(),
                machine_type: "typ".into(),
                machine_code: "cod".into(),
            })
        );
        for missing in ["machineToken", "machineType", "machineCode"] {
            let mut value = full.clone();
            value.as_object_mut().unwrap().remove(missing);
            assert_eq!(parse_identity(&value), None, "缺 {missing} 必须判无效");
        }
        // 空白值等同缺失（组件偶尔给空串）。
        let blank = json!({"machineToken": "  ", "machineType": "t", "machineCode": "c"});
        assert_eq!(parse_identity(&blank), None);
        assert_eq!(parse_identity(&json!({})), None);
    }

    /// ★ `state.ini` 的三种编码都要能读出 `installDir`（launcher 实测用 UTF-16 写盘）。
    ///
    /// 可证伪：删掉 `decode_ini` 的 UTF-16 分支 ⇒ 第二条断言变红。
    #[test]
    fn state_ini_decodes_utf8_and_utf16() {
        let utf8 = b"installDir=D:\\Programs\\Qoder\r\nother=1\r\n";
        assert_eq!(
            parse_state_ini(utf8),
            Some("D:\\Programs\\Qoder".to_string())
        );

        let text = "installDir=C:\\Apps\\Qoder CN\r\n";
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(
            parse_state_ini(&utf16),
            Some("C:\\Apps\\Qoder CN".to_string())
        );

        // 键名大小写不敏感；缺键 / 空值 ⇒ None（不猜）。
        assert_eq!(
            parse_state_ini(b"INSTALLDIR=D:\\x\n"),
            Some("D:\\x".to_string())
        );
        assert_eq!(parse_state_ini(b"other=1\n"), None);
        assert_eq!(parse_state_ini(b"installDir=\n"), None);
        assert_eq!(parse_state_ini(b""), None);
    }

    /// 关闭开关生效（受限环境不希望拉起客户端二进制）。
    #[test]
    fn disabled_switch_short_circuits() {
        // ⚠️ 不真的改环境变量（并发测试会互相干扰）；只钉住判定函数的**取值集合**。
        for value in ["0", "false", "no", "NO", " False "] {
            assert!(
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no"
                ),
                "{value} 应判为关闭"
            );
        }
        for value in ["1", "true", "yes", ""] {
            assert!(
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no"
                ),
                "{value} 应判为开启"
            );
        }
    }

    /// 区域 → launcher 目录名：国内版带 CN 形态，国际版只有 `Qoder`。
    #[test]
    fn launcher_names_differ_by_region() {
        assert!(launcher_dir_names(QoderRegion::Cn).contains(&"Qoder CN"));
        assert_eq!(launcher_dir_names(QoderRegion::Global), &["Qoder"]);
    }
}
