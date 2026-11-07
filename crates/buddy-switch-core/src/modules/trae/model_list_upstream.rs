//! 直连上游取 Trae 模型清单（issue #173）。
//!
//! ## 为什么需要第二条来源
//!
//! [`super::model_list`] 读的是客户端 `state.vscdb` —— 零网络、零鉴权，但它有个
//! 结构性前提：**客户端得先启动并登录过**。headless / Docker 部署（issue #62）里没有
//! 客户端，清单永远是 `missing`，只能退回内置兜底表（模型一多就过时）。
//!
//! issue #173 的取证证明存在一条**普通 HTTPS POST**：
//!
//! ```text
//! POST https://api5-normal.mchost.guru/api/ide/v1/batch_get_detail_param
//! body: {"functions":["chat_v3"]}
//! → HTTP 200，function_configs[0].config_info_list 共 59 条（38 条对用户可见）
//! ```
//!
//! 报错极具误导性：缺请求头时回 `{"error_code":2001,"error_message":"Config detail for
//! function: chat_v3 not found"}` —— 说的是「function 不存在」，而不是「缺头」。
//! 本模块的文件头把这条写下来，免得下一个人再走一遍。
//!
//! ## 三条口径红线
//!
//! 1. **opt-in**：这是**用账号 JWT 打 IDE 接口**。本仓对「替用户花上游成本」的默认
//!    一律是关（对照 `credit-refresh-pref` 的「打上游默认关」）。⇒ 只有用户**显式点**
//!    才发请求（见 [`refresh`]），不做后台轮询、不在渲染路径上打网络。
//! 2. **失败静默回落**：拿不到就回到客户端缓存 / 内置兜底，**不把「上游没回」当成
//!    「没有模型」**。缓存只是**优先**，不是唯一来源。
//! 3. **不冒充能力**：`batch_get_detail_param` 的可见条目里 `is_vl` **实测全为空**
//!    （issue #173）⇒ 图片能力**只能**来自客户端缓存或内置表。本模块**不得**用它推
//!    `multimodal`，否则就是拿「没声明」当「不支持」。同理 `display_config.max_mode`
//!    是 Max Mode 的支持声明，不是图片能力。
//!
//! ## 版本闸门（会过期的东西）
//!
//! `x-ide-version` / `x-ide-version-code` 是有**闸门作用**的：旧值会回
//! `4001 config item is empty`。它们来自抓包时的客户端版本（`3.4.4` / `20260924`），
//! 客户端升级后本路径**会失效** —— 失效时静默回落到客户端缓存，不会影响任何已有功能。
//! 更新方式：取一份新的客户端包，看 `resources/app/package.json` 里的版本与
//! `x-ide-version-code` 对应的构建号，再改这两个常量（有回归测试钉死取值，改了会红）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::modules::trae::account;
use crate::modules::trae::device;
use crate::modules::trae::jwt;
use crate::modules::trae::model_list::{ClientModel, ThinkingSupport};
use crate::modules::trae::paths;
use crate::modules::trae::variant::TraeVariant;
use crate::modules::trae::{store, TRAE_OAUTH_APP_ID};

/// 清单来源：直连上游。界面按它选文案（与 [`super::model_list::SOURCE_CLIENT_CACHE`] 区分）。
pub const SOURCE_UPSTREAM: &str = "upstream";

/// 上游 IDE 服务基址（**仅国内版实测可用**）。
///
/// ⚠️ 国际版（`api.trae.ai` 那条线）**没有实测证据**能走同一个路径 —— 实测过的只有
/// CN 抓包。因此 [`is_supported`] 只对国内版放行，国际版回落客户端缓存并给出原因，
/// **不猜一个域名**（猜错就是把账号 JWT 发给一个陌生主机）。
const IDE_BASE_CN: &str = "https://api5-normal.mchost.guru";

/// 取 function 配置明细的路径（逐字照抄抓包）。
const DETAIL_PARAM_PATH: &str = "/api/ide/v1/batch_get_detail_param";

/// 缓存文件名（放在 `~/.buddy-switch/trae/` 下，按变体分家）。
const CACHE_FILE: &str = "model_list_upstream.json";

/// 缓存新鲜期：6 小时。
///
/// 上游清单一天也变不了几次，但**过期不等于不能用** —— 超过这个时长后
/// [`super::model_list`] 会**优先读客户端缓存**（更新的那份），只有当客户端也读不到时
/// 才拿这份过期的顶上（并在 `note` 里注明是哪天抓的）。
const CACHE_TTL_MS: i64 = 6 * 3600 * 1000;

/// 抓包时的 IDE 版本（**闸门**，见模块头）。
const IDE_VERSION: &str = "3.4.4";
/// 抓包时的 IDE 版本码（**闸门**，见模块头）。
const IDE_VERSION_CODE: &str = "20260924";

/// 只请求 `chat_v3` —— 实测返回 59 条，已含界面需要的全部分组。
const FUNCTIONS: [&str; 1] = ["chat_v3"];

/// 不是「模型」的配置项：摘要是节点功能、fast_apply 是补丁生成、标题生成与输入优化同理。
///
/// 上游把它们和模型放在同一个 `config_info_list` 里（同一份 function 配置），
/// 不过滤就会在模型下拉里出现 `summary` 这种**根本不是模型**的条目。
const INTERNAL_CONFIGS: [&str; 4] = [
    "summary",
    "fast_apply",
    "title_generation",
    "input_optimization",
];

/// 该变体是否支持直连（当前只有国内版有实测证据）。
pub fn is_supported(variant: TraeVariant) -> bool {
    matches!(
        variant.region(),
        crate::modules::trae::region::TraeRegion::Cn
    )
}

/// 缓存文件路径（按变体分家：两条产品线的清单本来就不同）。
pub fn cache_file_for(variant: TraeVariant) -> PathBuf {
    paths::trae_dir().join(format!(
        "{}_{}.json",
        CACHE_FILE.trim_end_matches(".json"),
        variant.as_str()
    ))
}

/// 落盘的缓存：**原文照存**，读的时候再解析。
///
/// 为什么不存解析结果：`ClientModel` 的字段会随版本增删，存成结构体后旧缓存会在
/// 新版本里变成「字段全 None」的僵尸数据；存原文则解析规则一改就自动生效。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheFile {
    #[serde(default)]
    fetched_at_ms: i64,
    #[serde(default)]
    response: Value,
}

/// 记下这次的原文与时刻。写失败不算错误（缓存是加速手段，不是数据）。
pub fn write_cache(variant: TraeVariant, response: &Value) -> Result<(), String> {
    let path = cache_file_for(variant);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    store::write_json(
        &path,
        &CacheFile {
            fetched_at_ms: crate::modules::config::now_ms(),
            response: response.clone(),
        },
    )
}

/// 读缓存的原文与抓取时刻；没有缓存 / 文件损坏 ⇒ `None`（**不是错误**）。
pub fn read_cache(variant: TraeVariant) -> Option<(i64, Value)> {
    let cache: CacheFile = store::read_json(&cache_file_for(variant));
    if cache.fetched_at_ms <= 0 || cache.response.is_null() {
        return None;
    }
    Some((cache.fetched_at_ms, cache.response))
}

/// 新鲜的解析结果；没有缓存或已过期 ⇒ `None`。
pub fn fresh_models(variant: TraeVariant, now_ms: i64) -> Option<Vec<ClientModel>> {
    let (fetched_at, response) = read_cache(variant)?;
    if now_ms.saturating_sub(fetched_at) > CACHE_TTL_MS {
        return None;
    }
    let models = parse_detail_params(&response);
    if models.is_empty() {
        return None;
    }
    Some(models)
}

/// 过期的解析结果（客户端也读不到时的兜底），`None` = 从没抓过。
pub fn stale_models(variant: TraeVariant) -> Option<(i64, Vec<ClientModel>)> {
    let (fetched_at, response) = read_cache(variant)?;
    let models = parse_detail_params(&response);
    if models.is_empty() {
        return None;
    }
    Some((fetched_at, models))
}

/* ══════════════════════ 解析（纯函数）══════════════════════ */

/// 把上游响应解析成客户端清单同形的 [`ClientModel`]。
///
/// ## 只填**能证明**的字段
///
/// 上游条目的形状（issue #173 的实测）：
/// `config_name / config_source / config_switch / context_window_tokens / display_config /
/// display_contact_config / extra_config / is_default / is_invisible_to_user /
/// model_detail_list / reasoning_effort_config / usage`。
///
/// 与 `state.vscdb` 那份清单相比，**没有** `multimodal` / `is_vl` / `credits_rate`。
/// ⇒ 这些字段一律留默认值（`false` / `None`），界面按「未下发」呈现，
/// 而不是拿人形默认值冒充上游声明。
pub fn parse_detail_params(response: &Value) -> Vec<ClientModel> {
    let Some(configs) = response
        .get("function_configs")
        .and_then(Value::as_array)
        .and_then(|configs| configs.first())
        .and_then(|first| first.get("config_info_list"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut models: Vec<ClientModel> = Vec::new();
    for config in configs {
        if let Some(model) = model_of(config) {
            models.push(model);
        }
    }
    // 上游顺序即优先级；仅去掉重名（同一 config_name 重复出现时**保留第一条**）。
    let mut seen = BTreeMap::new();
    models.retain(|model| seen.insert(model.name.clone(), ()).is_none());
    models
}

/// 单条配置 → 模型；不是模型（不可见 / 内部功能 / 空名）时返回 `None`。
fn model_of(config: &Value) -> Option<ClientModel> {
    if config.get("is_invisible_to_user").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let name = config
        .get("config_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return None;
    }
    // `custom_model_*` 是用户自建路由（`provider` 非空），网关发给上游无效（见 `ClientModel::is_bypass`）。
    if name.to_ascii_lowercase().starts_with("custom_model") {
        return None;
    }
    if INTERNAL_CONFIGS.contains(&name) {
        return None;
    }

    let display = config.get("display_config");
    let display_name = display
        .and_then(|value| value.get("display_name"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(name)
        .to_string();

    // 档位词表：实测形如 `{"options":["light","high"],"default_level":"high"}`。
    // 字段名拼错就退化成「未下发」，不会伪造出「支持」。
    let effort_config = config.get("reasoning_effort_config");
    let efforts: Vec<String> = effort_config
        .and_then(|value| value.get("options"))
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let default_effort = effort_config
        .and_then(|value| value.get("default_level"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty());
    let thinking = if efforts.is_empty() {
        // 没下发 ≠ 不支持（与 `model_list` 的同一纪律）。
        ThinkingSupport::Unknown
    } else {
        ThinkingSupport::Supported
    };

    Some(ClientModel {
        name: name.to_string(),
        display_name,
        // 上游这份条目没有 `model_type`；不猜 `chat_model`。
        model_type: String::new(),
        // ★ 红线：这份响应里没有图片能力声明（`is_vl` 实测全空），一律 false = 未声明。
        multimodal: false,
        is_default: config
            .get("is_default")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        // 名字来自上游下发的配置 ⇒ 对本工具而言就是「官方内置」。
        is_preset: true,
        is_new: false,
        is_beta: false,
        is_bypass: false,
        context_window: config.get("context_window_tokens").and_then(Value::as_i64),
        context_window_max: None,
        prompt_max_tokens: None,
        credits_rate: None,
        efforts,
        default_effort,
        thinking,
    })
}

/* ══════════════════════ 请求（opt-in）══════════════════════ */

/// 用**账号池里的第一条有效 JWT** 打一次上游并落盘缓存。
///
/// ## 只由用户的显式动作触发
///
/// 调用方只能是「刷新清单」这类用户点击路径（见管理面模型卡的「从上游拉取」）。
/// **不要**把它挂到渲染路径或定时器上：每次调用都是一次带账号凭据的上游请求。
///
/// ## 失败原因要能自证
///
/// 返回的 `Err` 里必须带上「哪一步失败」（没有账号 / 没有设备 / HTTP 状态 / 解析），
/// 否则用户只会看到「刷新失败」四个字 —— 那正是 issue #173 抱怨的报错质量。
pub async fn refresh(variant: TraeVariant) -> Result<Vec<ClientModel>, String> {
    if !is_supported(variant) {
        return Err("该区域暂无直连取清单的实测证据，请先启动客户端一次".to_string());
    }

    let accounts = account::load_accounts_for(variant);
    let account = accounts
        .accounts
        .iter()
        .find(|record| !jwt::normalize(&record.jwt).is_empty())
        .ok_or_else(|| "账号池里没有可用凭据（先登录一个 Trae 账号）".to_string())?;
    let uid = jwt::user_id_of(&account.jwt).unwrap_or_default();
    let device_entry = device::ensure_for_variant(variant, &uid)
        .map_err(|error| format!("读不到本机设备标识: {error}"))?;
    let machine_id = device::oauth_login_machine_for(variant).machine_id;

    let headers = build_headers(&account.jwt, &device_entry.device_id, &machine_id);
    let url = format!("{IDE_BASE_CN}{DETAIL_PARAM_PATH}");
    let body = serde_json::json!({ "functions": FUNCTIONS });

    let mut request = crate::modules::trae::credits::trae_http_client()
        .post(&url)
        .json(&body);
    for (key, value) in headers {
        request = request.header(key, value);
    }

    let response = request.send().await.map_err(|error| {
        // 与 credits 侧同一套传输错误归类（超时 / 连不上 / 响应体），不另拼一套说法。
        let app = crate::modules::net::transport_error(&error);
        format!("连接上游失败: {}", app.text())
    })?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!("上游返回 HTTP {status}: {}", truncate(&text, 200)));
    }
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|_| format!("上游返回非 JSON: {}", truncate(&text, 200)))?;
    // 业务错误码：2001 = 「function 找不到」，抓包证明它在**缺请求头**时也会出现。
    if let Some(code) = parsed.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let message = parsed
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(format!(
                "上游拒绝（{code}）：{message} —— 若看到「function not found」，实际多是请求头不全或 x-ide-version 过期"
            ));
        }
    }

    let models = parse_detail_params(&parsed);
    if models.is_empty() {
        return Err("上游返回了 0 个可见模型（响应结构可能变了）".to_string());
    }
    // 落盘失败不影响本次结果：缓存是加速手段。
    let _ = write_cache(variant, &parsed);
    Ok(models)
}

/// 构造请求头。
///
/// ## 哪些是账号/客户端相关的
///
/// `x-ide-token`（**不是** `Authorization` —— 这是 issue #173 最大的坑）/
/// `x-device-id` / `x-machine-id` / `x-app-id` / `x-ide-version*`。
/// 其余是固定值：照抄抓包，**不按主机平台改写** —— 「看起来更合理」的改动没有实测支撑，
/// 而失败时调用方已经会回落客户端缓存，代价只是这条 opt-in 路径不可用。
fn build_headers(jwt_value: &str, device_id: &str, machine_id: &str) -> Vec<(String, String)> {
    let token = jwt::normalize(jwt_value).to_string();
    vec![
        ("accept".into(), "*/*".into()),
        ("content-type".into(), "application/json".into()),
        ("request-traffic-type".into(), "prod".into()),
        ("user-agent".into(), "TraeClient/TTNet".into()),
        ("x-agent-host-type".into(), "0".into()),
        ("x-app-id".into(), TRAE_OAUTH_APP_ID.into()),
        ("x-app-version".into(), "default".into()),
        ("x-app-version-code".into(), IDE_VERSION_CODE.into()),
        ("x-bridge-transport".into(), "aha".into()),
        // 每次请求唯一：上游用它做链路去重，复用同值会被判重复请求。
        (
            "x-custom-trace-id".into(),
            uuid::Uuid::new_v4().simple().to_string(),
        ),
        ("x-device-brand".into(), "MacBookPro18,3".into()),
        ("x-device-cpu".into(), "Apple".into()),
        ("x-device-id".into(), device_id.to_string()),
        ("x-device-type".into(), "mac".into()),
        ("x-ide-token".into(), token),
        ("x-ide-version".into(), IDE_VERSION.into()),
        ("x-ide-version-code".into(), IDE_VERSION_CODE.into()),
        ("x-ide-version-type".into(), "stable".into()),
        ("x-lgw-req-sdk-type".into(), "3".into()),
        ("x-machine-id".into(), machine_id.to_string()),
        ("x-os-version".into(), "macOS 27.0.1".into()),
        ("package-type".into(), "stable_cn".into()),
        (
            "x-request-id".into(),
            format!("req_{}", uuid::Uuid::new_v4().simple()),
        ),
        ("x-lscbd-aid".into(), "787976".into()),
        ("x-lscbd-platform".into(), "mac".into()),
        ("X-SS-DP".into(), "787976".into()),
    ]
}

fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// issue #173 的真实形状（截取）：可见条目 + 隐藏条目 + 内部功能 + 自定义路由。
    fn fixture() -> Value {
        serde_json::json!({
            "function_configs": [{
                "function": "chat_v3",
                "config_info_list": [
                    {
                        "config_name": "deepseek-v4.1-flash",
                        "context_window_tokens": 200000,
                        "is_default": true,
                        "is_invisible_to_user": false,
                        "display_config": { "display_name": "DeepSeek V4.1 Flash" },
                        "reasoning_effort_config": { "options": ["low", "high", "max"], "default_level": "high" }
                    },
                    {
                        "config_name": "glm-5.3",
                        "is_invisible_to_user": false,
                        "reasoning_effort_config": { "options": [] }
                    },
                    { "config_name": "hidden-model", "is_invisible_to_user": true },
                    { "config_name": "custom_model_2445846658", "is_invisible_to_user": false },
                    { "config_name": "summary", "is_invisible_to_user": false },
                    { "config_name": "", "is_invisible_to_user": false },
                    { "config_name": "deepseek-v4.1-flash", "is_invisible_to_user": false }
                ]
            }]
        })
    }

    #[test]
    fn parse_keeps_only_user_visible_models() {
        let models = parse_detail_params(&fixture());
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["deepseek-v4.1-flash", "glm-5.3"],
            "隐藏条目 / 内部功能 / 自定义路由 / 空名都必须剔除，重名只留第一条"
        );
    }

    #[test]
    fn parse_maps_only_provable_fields() {
        let models = parse_detail_params(&fixture());
        let flash = &models[0];
        assert_eq!(flash.display_name, "DeepSeek V4.1 Flash");
        assert_eq!(flash.context_window, Some(200000));
        assert!(flash.is_default);
        assert_eq!(flash.efforts, vec!["low", "high", "max"]);
        assert_eq!(flash.default_effort.as_deref(), Some("high"));
        assert_eq!(flash.thinking, ThinkingSupport::Supported);
        // ★ 红线：这份响应没有图片能力声明 ⇒ 不得写 true，也不得据「没字段」判不支持。
        assert!(!flash.multimodal, "没有 is_vl 时不得声明支持图片");

        let glm = &models[1];
        // `options: []` = 下发了但为空 ⇒ 仍按「未下发」处理（不是「明确不支持」）。
        assert_eq!(glm.thinking, ThinkingSupport::Unknown);
        assert!(
            glm.display_name == "glm-5.3",
            "缺 display_name 时回落 config_name"
        );
    }

    #[test]
    fn parse_tolerates_malformed_responses() {
        assert!(parse_detail_params(&Value::Null).is_empty());
        assert!(parse_detail_params(&serde_json::json!({})).is_empty());
        assert!(parse_detail_params(&serde_json::json!({ "function_configs": [] })).is_empty());
        assert!(parse_detail_params(&serde_json::json!({
            "function_configs": [{ "config_info_list": "x" }]
        }))
        .is_empty());
    }

    #[test]
    fn only_cn_is_supported() {
        use crate::modules::trae::variant::TraeVariant;
        assert!(is_supported(TraeVariant::TraeMerged));
        assert!(
            !is_supported(TraeVariant::Global),
            "国际版没有实测证据，不得凭空放行（那是把账号 JWT 发给未经验证的主机）"
        );
    }

    #[test]
    fn headers_put_the_token_in_x_ide_token_not_authorization() {
        let headers = build_headers("Cloud-IDE-JWT abc.def.ghi", "123", "mach");
        let map: std::collections::HashMap<_, _> = headers.into_iter().collect();
        assert_eq!(
            map.get("x-ide-token").map(String::as_str),
            Some("abc.def.ghi")
        );
        assert!(
            !map.contains_key("authorization"),
            "issue #173：JWT 放在 x-ide-token 里，放 Authorization 会被回 2001"
        );
        assert_eq!(map.get("x-machine-id").map(String::as_str), Some("mach"));
        assert_eq!(map.get("x-device-id").map(String::as_str), Some("123"));
    }

    #[test]
    fn version_constants_are_pinned() {
        // 改了这两个常量必须是有意的（客户端升级后重新抓包），不是顺手美化。
        assert_eq!(IDE_VERSION, "3.4.4");
        assert_eq!(IDE_VERSION_CODE, "20260924");
    }
}
