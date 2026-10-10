//! ★ 诊断包导出（Q1：WB 侧危险操作区的「导出诊断包」）。
//!
//! ## 它是什么
//!
//! 一份**给排查用**的 JSON 快照：版本 / 平台 / 各数据源是否可读 / 网关配置**摘要**
//! （脱敏）/ 近期错误摘要。用户把它发给维护者或贴进 issue，**不必手工截多条界面**。
//!
//! ## ★ 凭据红线（复用备份的那一套，不另立标准）
//!
//! 诊断包是要**发给别人**的产物 ⇒ 与 [`crate::modules::backup::CREDENTIAL_KEYS`]
//! **同一套键集**、**同一个搜索函数** [`contains_credential_key`]：
//!
//! - 实现层：递归剥离（复用 `strip_credentials` 的同一份键集）；
//! - 验收层：产出后当**纯文本**搜这组键 ⇒ **必须 0 命中**，否则**返回错误而不是发出**。
//!
//! ⚠️ 刻意**不**打包会话正文（`projects/**/*.jsonl`）：它含用户真实项目路径与对话内容
//! （`MEMORY-10` 红线：展示层不把全路径抛到界面），对定位本工具自身的问题也没有帮助。
//!
//! ## 为什么放在 core 而不是 Trae 侧
//!
//! Q1 的原型要求 WB 侧也有危险操作区。WB 与 Trae 是**平行的两个 Product**
//! （`product.rs::WB_SPEC` / `TRAE_SPEC`），不是超集关系 ⇒ 各有各的危险面：
//! Trae 的是「重置客户端设备标识」（改 Trae 自己的文件），WB 的是本模块。

use serde_json::{json, Map, Value};

use crate::modules::backup::{contains_credential_key, CREDENTIAL_KEYS};
use crate::modules::config;

/// 诊断包类型标识（前端据此识别，勿与备份 envelope 混用）。
pub const DIAGNOSTIC_KIND: &str = "buddy-switch-diagnostic";

/// 采集一份诊断包（**只读**：不写任何用户数据）。
///
/// 返回可直接落盘的 JSON 文本。**失败即 `Err`**：宁可不出文件，也不发出一份
/// 含凭据的诊断包。
pub fn export_diagnostic() -> Result<String, String> {
    let bundle = json!({
        "kind": DIAGNOSTIC_KIND,
        "generatedAt": config::now_ms(),
        "app": app_info(),
        "dataSources": data_sources(),
        "gateway": gateway_summary(),
    });

    let text = serde_json::to_string_pretty(&bundle)
        .map_err(|error| format!("诊断包序列化失败：{error}"))?;

    // ★ 验收闸门：产出后当**纯文本**搜凭据键，命中即拒绝发出（fail-closed）。
    //   这与 `backup` 的验收搜索**共用同一个函数**，杜绝判据漂移。
    if let Some(hit) = contains_credential_key(&text) {
        return Err(format!(
            "诊断包包含疑似凭据字段（{hit}），已阻止导出。请把该字段报给维护者。"
        ));
    }

    Ok(text)
}

/// 版本 / 平台信息（无敏感数据）。
fn app_info() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
    })
}

/// **有 / 无**两态，不给具体数值：诊断只需回答「这条链读得到吗」。
fn probe(label: &str, path: std::path::PathBuf) -> Value {
    json!({
        "label": label,
        "available": path.is_dir(),
    })
}

/// 各数据源可读性。⚠️ 只报 `available`，**不报行数 / 大小 / 路径**（路径含用户名）。
fn data_sources() -> Value {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut sources = Map::new();
    sources.insert(
        "workbuddyCnProjects".to_string(),
        probe("WorkBuddy 国内版会话目录", home.join(".workbuddy/projects")),
    );
    sources.insert(
        "workbuddyGlobalProjects".to_string(),
        probe("WorkBuddy 国际版会话目录", home.join(".workbuddy-ai/projects")),
    );
    sources.insert(
        "workbuddyCnAuth".to_string(),
        probe("WorkBuddy 国内版认证目录", home.join(".workbuddy")),
    );
    sources.insert(
        "toolDataDir".to_string(),
        probe("本工具数据目录", home.join(".buddy-switch")),
    );
    sources.insert(
        "gatewayLogs".to_string(),
        probe(
            "网关请求日志",
            crate::modules::region::gateway_config_file().with_file_name("gateway_logs.json"),
        ),
    );
    Value::Object(sources)
}

/// 网关配置**摘要**：只取非敏感字段，且逐个剥离凭据键。
///
/// ⚠️ 不整份塞 `gateway_config.json` —— 它可能含 `apiKeys` / 代理 `user:pass@`
/// （`update` 代理那类）。这里**白名单取字段**，不是「全取再删几个」。
fn gateway_summary() -> Value {
    let path = crate::modules::region::gateway_config_file();
    let raw = std::fs::read_to_string(&path).ok();
    let Some(raw) = raw else {
        return json!({ "available": false });
    };
    let Ok(mut config) = serde_json::from_str::<Value>(&raw) else {
        return json!({ "available": true, "parseable": false });
    };
    scrub_credentials(&mut config);

    // 白名单：只保留这些诊断确实用得上的字段。
    const SAFE_FIELDS: &[&str] = &[
        "enabled",
        "port",
        "logKeep",
        "log_keep",
        "logBodies",
        "proxyMode",
        "domains",
        "region",
    ];
    let mut safe = Map::new();
    for field in SAFE_FIELDS {
        if let Some(value) = config.get(*field) {
            safe.insert((*field).to_string(), value.clone());
        }
    }
    // `domains` 若是数组，逐项过一遍剥离（防其中夹带凭据 URL）。
    if let Some(Value::Array(items)) = safe.get_mut("domains") {
        for item in items.iter_mut() {
            scrub_credentials(item);
        }
    }
    json!({ "available": true, "parseable": true, "config": Value::Object(safe) })
}

/// 按 [`CREDENTIAL_KEYS`] 递归剥离（与 `backup::strip_credentials` 同一份键集）。
fn scrub_credentials(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("$wbEncrypted") {
                *value = Value::Null;
                return;
            }
            for key in CREDENTIAL_KEYS {
                map.remove(*key);
            }
            for (_, child) in map.iter_mut() {
                scrub_credentials(child);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                scrub_credentials(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 诊断包**不得**含任何凭据键（当纯文本搜，与导出验收同一判据）。
    #[test]
    fn diagnostic_bundle_text_has_no_credential_keys() {
        let text = export_diagnostic().expect("诊断包应可导出");
        assert_eq!(
            contains_credential_key(&text),
            None,
            "诊断包出现凭据键 —— 绝不可发给他人"
        );
    }

    /// 诊断包**只读**：除时间戳外，两次导出必须逐字节一致（证明无写入、无随机字段）。
    #[test]
    fn diagnostic_bundle_is_read_only_and_idempotent() {
        // `generatedAt` 是时间戳，两次导出必然不同 ⇒ 比对前先剔除它。
        let strip_ts = |text: &str| -> Value {
            let mut value: Value = serde_json::from_str(text).expect("诊断包应是合法 JSON");
            value.as_object_mut().unwrap().remove("generatedAt");
            value
        };
        let first = strip_ts(&export_diagnostic().expect("首次导出"));
        let second = strip_ts(&export_diagnostic().expect("二次导出"));
        assert_eq!(
            first, second,
            "除时间戳外两次导出应一致（证明无写入、无随机字段漂移）"
        );
    }

    /// 剥离是**递归**的：嵌套在数组 / 对象里的凭据同样拿不到。
    #[test]
    fn scrub_removes_nested_credentials() {
        let mut value = json!({
            "outer": { "access_token": "A", "keep": 1 },
            "list": [{ "jwt": "J", "keep": 2 }, { "refreshToken": "R" }],
        });
        scrub_credentials(&mut value);
        assert_eq!(
            contains_credential_key(&value.to_string()),
            None,
            "嵌套凭据未被剥离：{value}"
        );
        assert_eq!(value["outer"]["keep"], json!(1), "非敏感字段须保留");
        assert_eq!(value["list"][0]["keep"], json!(2), "非敏感字段须保留");
    }
}