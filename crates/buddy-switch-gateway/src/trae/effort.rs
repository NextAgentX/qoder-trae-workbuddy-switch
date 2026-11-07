//! Trae 思考档位（`reasoning_effort`）的映射与注入 —— issue #127。
//!
//! ## 问题
//!
//! OpenAI 兼容客户端把档位放在**顶层** `reasoning_effort`（`low` / `medium` / `high`…），
//! 而 Trae 的档位词表是**模型自己声明的**、而且是另一套名字（实测出现过
//! `light` / `high` / `extra_high`）。网关此前对 `reasoning_effort` **完全不碰**：
//! 不校验、不转换、原样透传 ⇒ 上游按未知字段忽略，等于「配了不生效」。
//!
//! ## 为什么默认**不注入**（与本仓 `forward_images_enabled` 同一条纪律）
//!
//! 上游收不收 `model_info.reasoning_effort_level`、收错了是静默忽略还是 `4001`，
//! **没有实测证据**。可参考的线索只有两条：
//!
//! 1. Trae 原生客户端把档位放在 `model_info.reasoning_effort_level`（issue #127 的
//!    `renderer.log` 实测，取值 `extra_high`）；
//! 2. 第三方实现把档位放在 `custom_model.reasoning_effort`。
//!
//! 两条都不是「我们这条链路」的实测。贸然默认开启，一旦上游严格校验就是**把
//! 「配了不生效」换成「整条请求 4001」** —— 那更糟。所以：
//!
//! ```text
//! BUDDY_SWITCH_TRAE_REASONING_EFFORT=1   # 1 / on / true / yes
//! ```
//!
//! 打开后**只做加法**：在请求体里补 `model_info.reasoning_effort_level`，
//! 顶层 `reasoning_effort` 原样留着（上游忽略它，删了反而少一条对照线索）。
//!
//! ## 词表从哪来
//!
//! 用**客户端清单里该模型自己声明的 `efforts`**（`state.vscdb` 的
//! `reasoning_effort_config.options`，见 core 的 `trae::model_list`）：那是上游亲口说的
//! 「这个模型支持哪些档」。声明为空（`—` 那些模型）⇒ **不映射也不注入** ——
//! 没声明就没有正确答案，编一个值上去只是换一种猜。
//!
//! ## 强度序
//!
//! [`trae_rank`] 把两套名字并到同一条强度轴上（`light`≈`low`、`extra_high`≈`xhigh`）。
//! 这些别名来自**实际观测到的取值**，不是一份规格文档 —— 新增取值时加一行即可，
//! 认不出的取值一律返回 `None`（不处理），绝不当成 0 或最高档。
//!
//! ## 开启后的成本
//!
//! 每个带顶层 `reasoning_effort` 的请求会读一次客户端清单（`state.vscdb`），与
//! `/v1/models` 的成本同级。只在这条开关打开时才发生 —— 默认关时零额外 IO。

use serde_json::{Map, Value};

use buddy_switch_core::modules::trae::model_list::read_client_model_list;
use buddy_switch_core::modules::trae::variant::TraeVariant;

/// 打开注入的环境变量（默认关，理由见模块头）。
pub const ENV_INJECT: &str = "BUDDY_SWITCH_TRAE_REASONING_EFFORT";

/// 是否把档位上送给上游。
///
/// 布尔解析与 `BUDDY_SWITCH_ANTHROPIC_FORWARD_IMAGES` / `BUDDY_SWITCH_QODER_KEEP_TOPLEVEL_IMAGES`
/// 同形（`1` / `on` / `true` / `yes`）。
pub fn injection_enabled() -> bool {
    std::env::var(ENV_INJECT)
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// 档位名 → 强度序（0 最弱）。认不出 ⇒ `None`（**不猜**）。
///
/// 与 `crate::outbound::effort::effort_rank` 是**两条独立的轴**：那条给 WorkBuddy 的
/// DeepSeek 静态表用（`off < minimal < low < medium < high < xhigh < max`），
/// 这条多了 Trae 侧观测到的 `light` / `extra_high` 两个别名。刻意不合并 ——
/// 两边的词表来源不同（一个是规格，一个是观测），合并会让任何一边的新取值污染另一边。
pub fn trae_rank(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "off" => Some(0),
        "minimal" => Some(1),
        // ★ `light` 是 Trae 清单里的实际取值，功能上等价于「低档」。
        "light" | "low" => Some(2),
        "medium" => Some(3),
        "high" => Some(4),
        // ★ `extra_high` 同上（issue #127 的 renderer.log 实测值）。
        "extra_high" | "xhigh" => Some(5),
        "max" => Some(6),
        _ => None,
    }
}

/// 把请求档位映射到该模型**自己声明**的档位。
///
/// 规则（与 `outbound::effort::adjust_effort` 同一条降级思路）：
/// 1. 已在声明列表里（大小写不敏感）⇒ 返回**声明里的写法**（大小写以声明为准）；
/// 2. 否则取「不高于请求档的声明档中最高的那个」；
/// 3. 所有声明档都更高 ⇒ 取最低那个（floored）；
/// 4. 声明为空 / 请求档认不出 ⇒ `None`（不改写、不注入）。
pub fn map_effort(requested: &str, declared: &[String]) -> Option<String> {
    let requested_trimmed = requested.trim();
    if requested_trimmed.is_empty() || declared.is_empty() {
        return None;
    }
    if let Some(exact) = declared
        .iter()
        .find(|effort| effort.eq_ignore_ascii_case(requested_trimmed))
    {
        return Some(exact.clone());
    }
    let requested_rank = trae_rank(requested_trimmed)?;

    let mut at_or_below: Option<(u8, &String)> = None;
    for effort in declared {
        let Some(rank) = trae_rank(effort) else {
            continue;
        };
        if rank <= requested_rank && at_or_below.map(|(cur, _)| rank > cur).unwrap_or(true) {
            at_or_below = Some((rank, effort));
        }
    }
    if let Some((_, effort)) = at_or_below {
        return Some(effort.clone());
    }
    // 全部高于请求档 ⇒ 退最低（与参考实现 `floored` 同义）。
    declared
        .iter()
        .filter_map(|effort| trae_rank(effort).map(|rank| (rank, effort)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, effort)| effort.clone())
}

/// 该模型在客户端清单里声明的档位（第一个非空声明的分组为准）。
///
/// 为什么取「第一个非空」而不是合并全部分组：同一模型会在多个 function 分组里出现，
/// 而档位是**按 (function, 模型) 下发**的 —— 合并会把别的程序位的档位混进来，
/// 变成「看得见、用不了」（与 `servable_entries` 的处理同源）。
pub fn declared_efforts(variant: TraeVariant, model: &str) -> Vec<String> {
    let list = read_client_model_list(variant);
    for group in &list.groups {
        for entry in &group.models {
            if entry.name == model && !entry.efforts.is_empty() {
                return entry.efforts.clone();
            }
        }
    }
    Vec::new()
}

/// 按开关把档位注入请求体（**只加不减**）。
///
/// 返回实际注入的档位（未注入时为 `None`），供调用方打一行诊断日志。
///
/// 注入位置是 `model_info.reasoning_effort_level`（原生客户端的字段位置）。
/// **已有 `model_info` 对象时合并进去**，不整块替换 —— 未来若上游要求更多
/// `model_info` 子字段，这里不能把它们冲掉。
pub fn inject_effort_level(
    object: &mut Map<String, Value>,
    variant: TraeVariant,
    model: &str,
) -> Option<String> {
    if !injection_enabled() {
        return None;
    }
    let declared = declared_efforts(variant, model);
    inject_effort_level_with(object, &declared, true)
}

/// [`inject_effort_level`] 的纯函数内核（`enabled` 显式传入 ⇒ 可单测）。
pub fn inject_effort_level_with(
    object: &mut Map<String, Value>,
    declared: &[String],
    enabled: bool,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let requested = object.get("reasoning_effort").and_then(Value::as_str)?;
    let chosen = map_effort(requested, declared)?;

    let slot = object
        .entry("model_info")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(info) = slot.as_object_mut() else {
        // `model_info` 被客户端塞成了非对象（脏数据）：**不覆盖**，也不猜该放哪。
        return None;
    };
    info.insert(
        "reasoning_effort_level".to_string(),
        Value::String(chosen.clone()),
    );
    Some(chosen)
}

/// 给 `/v1/models` 的单个条目补思考档位声明（issue #127 的第 1 个诉求）。
///
/// 只写**清单里真的有**的信息：
/// - `thinking`：`supported` / `unsupported` / `unknown`（与 [`crate::trae::payload`] 的
///   `ThinkingSupport` 同义，**未下发 ≠ 不支持**）；
/// - `reasoning_effort`：`{ "options": [...], "default": "high" }`，声明为空时**整个键省略**
///   （不写 `[]`，那会被客户端读成「支持 0 个档位」）。
pub fn annotate_model_entry(
    entry: &mut Value,
    model: &str,
    list: &buddy_switch_core::modules::trae::model_list::ClientModelList,
) {
    let mut found: Option<(&Vec<String>, Option<&String>, &str)> = None;
    for group in &list.groups {
        for item in &group.models {
            if item.name == model {
                found = Some((
                    &item.efforts,
                    item.default_effort.as_ref(),
                    match item.thinking {
                        buddy_switch_core::modules::trae::model_list::ThinkingSupport::Supported => {
                            "supported"
                        }
                        buddy_switch_core::modules::trae::model_list::ThinkingSupport::Unsupported => {
                            "unsupported"
                        }
                        buddy_switch_core::modules::trae::model_list::ThinkingSupport::Unknown => {
                            "unknown"
                        }
                    },
                ));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some((efforts, default_effort, thinking)) = found else {
        return;
    };
    let Some(object) = entry.as_object_mut() else {
        return;
    };
    object.insert("thinking".to_string(), Value::String(thinking.to_string()));
    if !efforts.is_empty() {
        let mut spec = Map::new();
        spec.insert(
            "options".to_string(),
            Value::Array(efforts.iter().cloned().map(Value::String).collect()),
        );
        if let Some(default_effort) = default_effort {
            spec.insert("default".to_string(), Value::String(default_effort.clone()));
        }
        object.insert("reasoning_effort".to_string(), Value::Object(spec));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn exact_match_wins_and_keeps_declared_spelling() {
        // 客户端发 `HIGH`，清单声明 `high` ⇒ 用清单的写法（上游认清单）。
        assert_eq!(
            map_effort("HIGH", &declared(&["light", "high"])),
            Some("high".into())
        );
        assert_eq!(
            map_effort("light", &declared(&["light", "high"])),
            Some("light".into())
        );
    }

    /// ★ 核心：OpenAI 的 `low`/`medium` 落在 `light`，`high` 及以上落在 `high`。
    #[test]
    fn openai_scale_falls_back_to_nearest_declared_level() {
        let options = declared(&["light", "high"]);
        assert_eq!(map_effort("minimal", &options), Some("light".into()));
        assert_eq!(map_effort("low", &options), Some("light".into()));
        assert_eq!(map_effort("medium", &options), Some("light".into()));
        assert_eq!(map_effort("high", &options), Some("high".into()));
        assert_eq!(map_effort("max", &options), Some("high".into()));

        // 三档词表里 `extra_high` 是最高档；`xhigh` 认它。
        let three = declared(&["light", "high", "extra_high"]);
        assert_eq!(map_effort("xhigh", &three), Some("extra_high".into()));
        assert_eq!(map_effort("max", &three), Some("extra_high".into()));
    }

    /// 所有声明档都高于请求档 ⇒ 退最低（floored），不是「不处理」。
    #[test]
    fn request_below_every_declared_level_falls_to_the_lowest() {
        let options = declared(&["high", "extra_high"]);
        assert_eq!(map_effort("off", &options), Some("high".into()));
        assert_eq!(map_effort("minimal", &options), Some("high".into()));
    }

    #[test]
    fn unknown_request_or_empty_declaration_is_left_alone() {
        assert_eq!(map_effort("banana", &declared(&["light", "high"])), None);
        assert_eq!(map_effort("high", &[]), None);
        assert_eq!(map_effort("", &declared(&["high"])), None);
        // 声明里全是认不出的取值 ⇒ 不猜（否则会往上游送一个没人见过的档位）。
        assert_eq!(map_effort("high", &declared(&["weird", "odd"])), None);
    }

    #[test]
    fn trae_rank_merges_aliases_and_rejects_unknown() {
        assert_eq!(trae_rank("light"), trae_rank("low"));
        assert_eq!(trae_rank("extra_high"), trae_rank("xhigh"));
        assert!(trae_rank("light").unwrap() < trae_rank("high").unwrap());
        assert!(trae_rank("high").unwrap() < trae_rank("extra_high").unwrap());
        assert_eq!(trae_rank("turbo"), None);
    }

    fn client_model(
        name: &str,
        efforts: &[&str],
        default: Option<&str>,
    ) -> buddy_switch_core::modules::trae::model_list::ClientModel {
        use buddy_switch_core::modules::trae::model_list::{ClientModel, ThinkingSupport};
        ClientModel {
            name: name.to_string(),
            display_name: name.to_string(),
            model_type: String::new(),
            multimodal: false,
            is_default: false,
            is_preset: true,
            is_new: false,
            is_beta: false,
            is_bypass: false,
            context_window: None,
            context_window_max: None,
            prompt_max_tokens: None,
            credits_rate: None,
            efforts: efforts.iter().map(|value| value.to_string()).collect(),
            default_effort: default.map(str::to_string),
            thinking: if efforts.is_empty() {
                ThinkingSupport::Unknown
            } else {
                ThinkingSupport::Supported
            },
        }
    }

    fn list_of(
        models: Vec<buddy_switch_core::modules::trae::model_list::ClientModel>,
    ) -> buddy_switch_core::modules::trae::model_list::ClientModelList {
        use buddy_switch_core::modules::trae::model_list::{ClientModelGroup, ClientModelList};
        ClientModelList {
            variant: "trae_cn".into(),
            variant_label: "Trae".into(),
            source: "client-cache".into(),
            read_at: 0,
            data_dir: None,
            uid: None,
            groups: vec![ClientModelGroup {
                function: "chat_v3".into(),
                models,
            }],
            note: None,
        }
    }

    /// ★ issue #127 的第 1 个诉求：`/v1/models` 要能看出「这个模型支持哪些档位」。
    #[test]
    fn annotate_exposes_thinking_and_options() {
        let list = list_of(vec![client_model(
            "glm-5.3",
            &["light", "high"],
            Some("high"),
        )]);
        let mut entry = serde_json::json!({ "id": "glm-5.3" });
        annotate_model_entry(&mut entry, "glm-5.3", &list);
        assert_eq!(entry["thinking"], "supported");
        assert_eq!(
            entry["reasoning_effort"]["options"],
            serde_json::json!(["light", "high"])
        );
        assert_eq!(entry["reasoning_effort"]["default"], "high");
    }

    /// 没声明档位 ⇒ 只写 `thinking`，**不写** `reasoning_effort`（`[]` 会被读成「支持 0 档」）。
    #[test]
    fn annotate_omits_options_when_nothing_declared() {
        let list = list_of(vec![client_model("seed-evolving", &[], None)]);
        let mut entry = serde_json::json!({ "id": "seed-evolving" });
        annotate_model_entry(&mut entry, "seed-evolving", &list);
        assert_eq!(
            entry["thinking"], "unknown",
            "未下发必须是 unknown，不是 unsupported"
        );
        assert!(entry.get("reasoning_effort").is_none());
    }

    /// 注入：只加 `model_info.reasoning_effort_level`，顶层字段与已有子键都不动。
    #[test]
    fn injection_adds_one_field_and_keeps_everything_else() {
        let options = declared(&["light", "high"]);
        let mut object: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "model": "glm-5.3",
            "reasoning_effort": "medium",
            "model_info": { "keep_me": true }
        }))
        .unwrap();
        let chosen = inject_effort_level_with(&mut object, &options, true);
        assert_eq!(chosen.as_deref(), Some("light"));
        assert_eq!(object["model_info"]["reasoning_effort_level"], "light");
        assert_eq!(object["model_info"]["keep_me"], true, "已有子键不得被冲掉");
        assert_eq!(object["reasoning_effort"], "medium", "顶层字段原样保留");
    }

    /// 开关关闭 / 没有 reasoning_effort / 声明为空 ⇒ 一个字节都不改。
    #[test]
    fn injection_is_a_noop_when_not_applicable() {
        let options = declared(&["light", "high"]);
        let mut off: Map<String, Value> =
            serde_json::from_value(serde_json::json!({ "reasoning_effort": "high" })).unwrap();
        assert_eq!(inject_effort_level_with(&mut off, &options, false), None);
        assert!(off.get("model_info").is_none());

        let mut no_field: Map<String, Value> =
            serde_json::from_value(serde_json::json!({ "model": "x" })).unwrap();
        assert_eq!(
            inject_effort_level_with(&mut no_field, &options, true),
            None
        );

        let mut empty: Map<String, Value> =
            serde_json::from_value(serde_json::json!({ "reasoning_effort": "high" })).unwrap();
        assert_eq!(inject_effort_level_with(&mut empty, &[], true), None);
    }

    #[test]
    fn injection_is_off_by_default() {
        // 默认关：环境变量未设置时不得改写任何请求体（这是本模块最重要的行为）。
        if std::env::var(ENV_INJECT).is_err() {
            assert!(!injection_enabled());
        }
    }
}
