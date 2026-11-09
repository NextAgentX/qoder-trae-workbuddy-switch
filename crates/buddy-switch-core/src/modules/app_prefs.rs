//! 应用级偏好（L0）—— **只放「宿主自己也要读」的那一类**。
//!
//! ## 边界：偏好存哪，看**谁需要读它**
//!
//! | 偏好 | 读者 | 载体 |
//! |:---|:---|:---|
//! | 主题 / 语言 / 作用域快捷键 / 自动检查更新 | 只有渲染层 | `localStorage`（见 `lib/theme.ts` 等） |
//! | **关闭窗口时是否隐藏到托盘** | **宿主（Rust）** | 本模块的 `app_prefs.json` |
//! | **账号授权是否在应用内完成** | **宿主（建授权窗）+ 渲染层（兜底按钮）** | 本模块的 `app_prefs.json` |
//! | **首次启动的风险确认时间** | 只有渲染层 | 本模块的 `app_prefs.json`（**刻意的例外**，见下） |
//!
//! 最后一行是本模块**唯一**偏离「只放宿主也读的偏好」这条界线的项：`legal_consent_at`
//! 只有渲染层读，但它不能放 `localStorage` —— 用户清一次站点数据就等于抹掉「已确认风险」
//! 的证据，产品会重新弹窗、作者手里什么都不剩。所以它跟 `close_to_tray` 一起进
//! `app_prefs.json`（store 根下、可审计、跨版本与清缓存存续），代价只是随
//! `get_app_prefs` / `set_app_prefs` 多走一遍命令。
//!
//! 「关闭窗口」这件事发生在 `WindowEvent::CloseRequested` 里，那一刻**没有渲染层参与**
//! —— `prevent_close()` 必须在 Rust 侧当场决定，晚了窗口已经没了。所以这一项不能像
//! 主题那样留在 `localStorage`（宿主读不到它）。
//!
//! ⚠️ **别把纯前端偏好搬进来**：每加一项都要多一对命令 + 四处接线（`commands.rs` /
//! `generate_handler!` / `api.ts` / 门禁豁免表），而收益是零 —— 渲染层读 `localStorage`
//! 是同步的，走命令反而要处理「首帧还没读回来」。
//!
//! ## 与 `github_config.json` 的关系
//!
//! 两者都是 store 根下的小 JSON，但**归属不同域**：更新源配置归 `update` 模块
//! （含 owner/repo/proxy，是网络配置），本模块归「本应用的运行方式」。合并只会让
//! 两个域的归一化规则互相牵连。

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::modules::config::{atomic_write, store_dir};

/// 偏好文件（与 `github_config.json` 同级，都在 store 根下）。
pub fn app_prefs_file() -> PathBuf {
    store_dir().join("app_prefs.json")
}

/// 默认值。
///
/// `close_to_tray = true` 是**改设置之前就有的行为**（`tray.rs` 里
/// `CloseRequested` 一律 `prevent_close` + `hide`）—— 默认值必须与它逐字一致，
/// 否则升级后所有老用户的「点关闭」行为会**静默改变**。
///
/// `watch_agent_sessions = false`：P15 的外部 Agent 会话监听**默认关**。
/// 开启它要**往别的客户端写 hook 配置**，属于必须由用户显式同意的动作
/// （`P15-SPIKE-RESULTS §6 D41`）—— 默认开等于替用户改了别人的配置。
///
/// `legal_consent_at = 0`：**0 表示尚未确认**首次启动的风险提示（epoch 毫秒，非零即已确认）。
/// 默认必须是 0 —— 把默认值写成「已同意」等于替用户签字，与免责声明的本意正相反。
///
/// `auth_in_app = true`：账号授权**默认在应用内完成**（内嵌窗口载授权页），不跳出系统浏览器。
/// 默认开的理由：授权页是自有域名（无第三方 OAuth 的 embedded-webview 限制），内嵌路径
/// 少一次应用切换、且授权窗口的登录态留在本进程里，后续查账号数据不必反复登录。
/// ★ 它只是**默认路径**，不是硬约束：任何一步失败（窗口建不起来 / 用户点「用浏览器打开」）
/// 都必须能退回系统浏览器，否则一次建窗失败就等于「登不了账号」。
pub fn default_app_prefs() -> Value {
    json!({
        "close_to_tray": true,
        "watch_agent_sessions": false,
        "legal_consent_at": 0,
        "auth_in_app": true
    })
}

/// 归一化：只认已知键；**按默认值的类型**决定受理哪种输入（布尔键只认布尔、
/// 时间戳键只认非负整数），否则回落默认值；未知键丢弃。
///
/// 归一化而不是原样透传，理由与 `normalize_switch_config` 相同：配置文件是用户
/// 可手改的，一个 `"false"`（字符串）如果被当成真值传给 `if`，就会出现
/// 「设置里显示已关闭、实际仍然隐藏到托盘」这种**两边都以为自己对**的状态。
/// 时间戳同理：`"1760000000000"` 这种字符串不认，宁可回落 0（= 未确认，再问一次）
/// 也不能凭空认定用户已经同意过风险提示。
fn normalize_app_prefs(input: &Value) -> Value {
    let defaults = default_app_prefs();
    let mut merged = defaults.clone();
    if let Some(obj) = input.as_object() {
        for (key, default) in defaults.as_object().unwrap() {
            let accepted = match default {
                Value::Bool(_) => obj.get(key).and_then(Value::as_bool).map(Value::Bool),
                Value::Number(_) => obj.get(key).and_then(Value::as_u64).map(|n| json!(n)),
                _ => None,
            };
            if let Some(value) = accepted {
                merged[key.as_str()] = value;
            }
        }
    }
    merged
}

/// 读取偏好；文件缺失 / 损坏 / 字段非法一律回落默认值（**永不失败**）。
pub fn load_app_prefs() -> Value {
    let f = app_prefs_file();
    if let Ok(text) = std::fs::read_to_string(&f) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            return normalize_app_prefs(&value);
        }
    }
    default_app_prefs()
}

/// 保存偏好，返回**归一化后的回读值**（调用方按回读值渲染，不假装写入成功）。
pub fn save_app_prefs(prefs: &Value) -> std::io::Result<Value> {
    let merged = normalize_app_prefs(prefs);
    std::fs::create_dir_all(store_dir())?;
    atomic_write(
        &app_prefs_file(),
        &serde_json::to_string_pretty(&merged).unwrap_or_default(),
    )?;
    Ok(merged)
}

/// 宿主专用：窗口被要求关闭时，该隐藏到托盘（true）还是真正退出（false）。
///
/// 每次读盘而不是进程内缓存：这个函数只在一件事（点关闭）上被调用，
/// 读一个小 JSON 的成本可以忽略；缓存反而会引入「改了设置但没生效」的窗口期。
pub fn close_to_tray() -> bool {
    load_app_prefs()
        .get("close_to_tray")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// 宿主专用：是否开启外部 Agent 会话监听（即「给客户端装 hook」）。
///
/// 与 [`close_to_tray`] 同款：每次读盘、不缓存 —— 它只在「装 / 卸 hook」这个
/// 用户显式动作上被读，读一个小 JSON 的成本可以忽略。
pub fn watch_agent_sessions() -> bool {
    load_app_prefs()
        .get("watch_agent_sessions")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 账号授权是否默认在**应用内**完成（内嵌授权窗）而不是跳系统浏览器。
///
/// 读者是宿主（`src-tauri` 的授权窗命令）与渲染层（登录对话框的兜底按钮），两边都读
/// 同一份文件 ⇒ 不会出现「设置里显示内嵌、实际跳了浏览器」。脏值一律回落默认 `true`。
pub fn auth_in_app() -> bool {
    load_app_prefs()
        .get("auth_in_app")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认值**只含已知键**，且每一项的默认值都是刻意选的（改它 = 改行为）。
    #[test]
    fn 默认值只含已知键() {
        let d = default_app_prefs();
        assert_eq!(
            d["close_to_tray"],
            json!(true),
            "老行为，必须与升级前逐字一致"
        );
        assert_eq!(
            d["watch_agent_sessions"],
            json!(false),
            "要写别的客户端配置的动作，必须默认关"
        );
        assert_eq!(
            d["legal_consent_at"],
            json!(0),
            "0 = 未确认风险提示；默认写成已同意等于替用户签字"
        );
        assert_eq!(
            d["auth_in_app"],
            json!(true),
            "账号授权默认在应用内完成；失败路径由调用方退回系统浏览器"
        );
        assert_eq!(
            d.as_object().unwrap().len(),
            4,
            "每加一个键都要同步这条断言"
        );
    }

    /// `auth_in_app()` 真值表：默认**开**（应用内授权），脏值回落开。
    ///
    /// 方向与 `watch_agent_sessions` 相反：这里默认开不会造成外部副作用（只是换个
    /// 承载方式，且失败可退回浏览器）；默认关反而会让「新增的默认体验」对老用户无效。
    #[test]
    fn auth_in_app_真值表() {
        let cases: [(Value, bool); 5] = [
            (json!({ "auth_in_app": true }), true),
            (json!({ "auth_in_app": false }), false),
            (json!({ "auth_in_app": "false" }), true), // 字符串不是布尔 ⇒ 默认（开）
            (json!({}), true),                         // 缺字段 ⇒ 默认（开）
            (json!([]), true),                         // 非对象 ⇒ 默认（开）
        ];
        for (input, want) in cases {
            let got = normalize_app_prefs(&input)
                .get("auth_in_app")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            assert_eq!(got, want, "输入 {input} 应得 {want}");
        }
    }

    #[test]
    fn 归一化只认已知键且非布尔回落默认() {
        // 已知键 + 合法布尔 ⇒ 采纳
        let ok = normalize_app_prefs(&json!({ "close_to_tray": false }));
        assert_eq!(ok["close_to_tray"], json!(false));
        // 字符串 "false" 不是布尔 ⇒ **回落 true**（不能把假值当真值用）
        let stringy = normalize_app_prefs(&json!({ "close_to_tray": "false" }));
        assert_eq!(stringy["close_to_tray"], json!(true));
        // 未知键丢弃（防止把别的模块的配置误存进来）
        let extra = normalize_app_prefs(&json!({ "close_to_tray": false, "theme": "dark" }));
        assert_eq!(extra.as_object().unwrap().len(), 4, "只认已知的四个键");
        assert!(extra.get("theme").is_none());
        // 非对象输入 ⇒ 全默认
        assert_eq!(normalize_app_prefs(&json!("nope")), default_app_prefs());
        assert_eq!(normalize_app_prefs(&json!(null)), default_app_prefs());
    }

    /// 真值表：`close_to_tray()` 是宿主的**唯一**判据，错一格就是「点了关闭没反应」
    /// 或「点了关闭整个应用没了」。逐格断言，不靠默认值蒙。
    #[test]
    fn close_to_tray_真值表() {
        let cases: [(Value, bool); 5] = [
            (json!({ "close_to_tray": true }), true),
            (json!({ "close_to_tray": false }), false),
            (json!({ "close_to_tray": "no" }), true), // 脏值 ⇒ 默认（隐藏）
            (json!({}), true),                        // 缺字段 ⇒ 默认（隐藏）
            (json!([]), true),                        // 非对象 ⇒ 默认（隐藏）
        ];
        for (input, want) in cases {
            let got = normalize_app_prefs(&input)
                .get("close_to_tray")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            assert_eq!(got, want, "输入 {input} 应得 {want}");
        }
    }

    /// `watch_agent_sessions()` 真值表：默认**关**，脏值一律回落到关。
    ///
    /// 这条比 `close_to_tray` 更要紧：它是「要不要去动**别人客户端的**配置文件」的判据，
    /// 任何一格错成 `true` 都意味着**用户没同意就被写了 hook**。
    ///
    /// 可证伪性：把 `unwrap_or(false)` 改成 `unwrap_or(true)` ⇒ 后三格变红。
    #[test]
    fn watch_agent_sessions_真值表() {
        let cases: [(Value, bool); 5] = [
            (json!({ "watch_agent_sessions": true }), true),
            (json!({ "watch_agent_sessions": false }), false),
            (json!({ "watch_agent_sessions": "true" }), false), // 字符串不是布尔 ⇒ 关
            (json!({}), false),                                 // 缺字段 ⇒ 关
            (json!([]), false),                                 // 非对象 ⇒ 关
        ];
        for (input, want) in cases {
            let got = normalize_app_prefs(&input)
                .get("watch_agent_sessions")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            assert_eq!(got, want, "输入 {input} 应得 {want}");
        }
    }

    /// `legal_consent_at` 真值表：只认**非负整数**（epoch 毫秒）。
    ///
    /// 字符串 / 负数 / 小数 / 缺字段一律回落 0（= 未确认）。方向必须是「宁可再问一次」：
    /// 任何一格错成「当作已同意」，都等于用户没勾选就被认定同意了风险提示。
    ///
    /// 可证伪性：把 `Value::Number(_)` 分支也接 `as_bool`（或直接透传原值）⇒ 后四格变红。
    #[test]
    fn legal_consent_at_真值表() {
        let cases: [(Value, u64); 6] = [
            (
                json!({ "legal_consent_at": 1_760_000_000_000u64 }),
                1_760_000_000_000,
            ),
            (json!({ "legal_consent_at": 1 }), 1),
            (json!({ "legal_consent_at": "1760000000000" }), 0), // 字符串 ⇒ 未确认
            (json!({ "legal_consent_at": -1 }), 0),              // 负数 ⇒ 未确认
            (json!({ "legal_consent_at": 1.5 }), 0),             // 小数 ⇒ 未确认
            (json!({}), 0),                                      // 缺字段 ⇒ 未确认
        ];
        for (input, want) in cases {
            let got = normalize_app_prefs(&input)
                .get("legal_consent_at")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            assert_eq!(got, want, "输入 {input} 应得 {want}");
        }
    }
}
