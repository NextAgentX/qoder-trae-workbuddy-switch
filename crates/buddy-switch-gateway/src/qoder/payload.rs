//! Qoder 请求整形 / 模型清单（**薄层**）—— 协议实现在
//! [`buddy_switch_core::modules::qoder::api`]，这里只做网关特有的取数选择：
//! 默认模型回落、粘性键、模型清单响应形状、清单元数据查找。

use std::collections::{BTreeSet, HashSet};

use serde_json::{json, Value};

use buddy_switch_core::modules::qoder::region::QoderRegion;

/// 本次请求要用的模型名：显式 `model` → 配置默认 →（调用方保证非空）。
pub fn model_of(parsed: &Value, default_model: &str) -> String {
    if let Some(model) = parsed.get("model").and_then(Value::as_str) {
        if !model.is_empty() {
            return model.to_string();
        }
    }
    default_model.to_string()
}

/// 是否流式（OpenAI 语义：缺省 `false`；上游**恒**走流式，由聚合层本地折叠）。
pub fn wants_stream(parsed: &Value) -> bool {
    parsed
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 会话粘性键（`metadata.conversation_id` 优先，回落 `metadata.session_id`；
/// Responses 客户端（Codex 等）没有 `metadata`，用 `prompt_cache_key`
/// —— 它本身就是「同一段会话复用同一前缀」的语义，与粘性同目的；
/// 都没有 ⇒ `None`，调用方整段跳过粘性——绝不退化成轮级键）。
pub fn sticky_key_of(parsed: &Value) -> Option<String> {
    if let Some(metadata) = parsed.get("metadata") {
        for key in ["conversation_id", "session_id"] {
            if let Some(value) = metadata.get(key).and_then(Value::as_str) {
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    parsed
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Responses 请求（`POST /v1/responses`）→ 上游 chat 形态。
///
/// 与 WB 那条线**共用** [`crate::protocol::responses::to_upstream_request`]
/// （`input` / `instructions` / `tools` / `tool_choice` / `max_output_tokens`
/// 的翻译只有一份）；这里只补 Qoder 上游额外认的**思考档位**：
/// Responses 把它放在 `reasoning.effort` 里，而 `build_chat_body` 读的是
/// `payload.reasoning.effort` / `reasoning_effort` —— 不搬过来就等于用户设了
/// `model_reasoning_effort = "high"` 却拿到默认档。
pub fn responses_to_chat(parsed: &Value) -> Value {
    let mut payload = crate::protocol::responses::to_upstream_request(parsed);
    if let Some(reasoning) = parsed.get("reasoning") {
        if let Some(object) = payload.as_object_mut() {
            object.insert("reasoning".to_string(), reasoning.clone());
        }
    }
    payload
}

/// 本次请求声明的工具名集合（供泄漏回声回读的守卫；无 tools ⇒ `None`）。
pub fn allowed_tool_names(parsed: &Value) -> Option<HashSet<String>> {
    let tools = parsed.get("tools").and_then(Value::as_array)?;
    let mut names = HashSet::new();
    for tool in tools {
        let name = tool
            .get("function")
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .or_else(|| tool.get("name").and_then(Value::as_str));
        if let Some(name) = name {
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        }
    }
    if names.is_empty() {
        None
    } else {
        Some(names)
    }
}

/// `/v1/models` 响应：清单元数据 + **禁用过滤**（`disabled` 是该区域作用域的集合）。
pub fn models_response(catalog: &[Value], disabled: &BTreeSet<String>, max_context: bool) -> Value {
    let data: Vec<Value> = catalog
        .iter()
        .filter(|entry| {
            let id = entry
                .get("id")
                .or_else(|| entry.get("key"))
                .and_then(Value::as_str)
                .unwrap_or("");
            !crate::disabled_models::is_disabled_in(disabled, id)
        })
        .map(|entry| {
            let id = entry
                .get("id")
                .or_else(|| entry.get("key"))
                .and_then(Value::as_str)
                .unwrap_or("");
            json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": "qoder",
                "display_name": entry.get("display_name").cloned().unwrap_or(Value::String(id.to_string())),
                "is_reasoning": entry.get("is_reasoning").cloned().unwrap_or(Value::Bool(false)),
                "max_input_tokens": entry.get("max_input_tokens").cloned(),
                // 与 WB / Trae 对齐：OpenAI 兼容客户端读 `context_length`。
                // 开关开 ⇒ 上游最大档（如 1M）；关 ⇒ 默认档（如 200K）。
                "context_length": selected_window(entry, max_context),
                // 两档原值（前端本地即时切换用）。
                "context_length_default": context_windows(entry).0,
                "context_length_max": context_windows(entry).1,
            })
        })
        .collect();
    json!({ "object": "list", "data": data })
}

/// 管理面模型清单（**未过滤**禁用 + 逐条 `disabled` 标记 + 来源三级）—— 两宿主共用的**唯一**形状。
///
/// ## 与 [`models_response`]（网关出口 ①）的分工
///
/// - 出口 ①：**过滤掉**被禁用的模型（外部客户端就该看不到它们）；
/// - 管理面：**全都给**（否则用户会以为禁用过的模型从上游消失了），逐条标 `disabled`，
///   由前端据此渲染「已禁用 + 启用」那一行 —— 「网关真正暴露的条数」= 全量 − 已禁用，
///   与出口 ① **同源同判据**（`is_disabled_in`）。
///
/// ⚠️ 形状由本函数**唯一产出**（`MEMORY.md §二`：两条通道不得各自拼装 JSON ——
/// `copy_sessions` 事故的直接对策：webui 与桌面端各自拼形状，一处改了另一处静默漂移）。
pub fn admin_models_response(
    region: QoderRegion,
    catalog: &super::CatalogView,
    disabled: &BTreeSet<String>,
    max_context: bool,
) -> Value {
    let data: Vec<Value> = catalog
        .models
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .or_else(|| entry.get("key"))
                .and_then(Value::as_str)
                .unwrap_or("");
            json!({
                "id": id,
                "object": "model",
                "owned_by": "qoder",
                "display_name": entry
                    .get("display_name")
                    .cloned()
                    .unwrap_or_else(|| Value::String(id.to_string())),
                "is_reasoning": entry
                    .get("is_reasoning")
                    .cloned()
                    .unwrap_or(Value::Bool(false)),
                "max_input_tokens": entry.get("max_input_tokens").cloned(),
                "context_length": selected_window(entry, max_context),
                "context_length_default": context_windows(entry).0,
                "context_length_max": context_windows(entry).1,
                "disabled": crate::disabled_models::is_disabled_in(disabled, id),
            })
        })
        .collect();
    json!({
        "object": "list",
        "region": region.as_str(),
        "source": catalog.source,
        "fetchedAt": catalog.fetched_at,
        "note": catalog.note,
        "data": data,
    })
}

/// 从 `context_config` 里取 (默认窗口, 最大窗口)。
///
/// 上游形态：`{"200K":{"token_count":200000,"is_default":true},"400K":{...},"1M":{...}}`
/// —— 默认档与上限档是**两个数**；没有 `context_config` 时回落 `max_input_tokens`（单档）。
pub fn context_windows(entry: &Value) -> (Option<i64>, Option<i64>) {
    let mut default = None;
    let mut max = None;
    if let Some(config) = entry.get("context_config").and_then(Value::as_object) {
        for window in config.values() {
            let Some(tokens) = window.get("token_count").and_then(Value::as_i64) else {
                continue;
            };
            if tokens <= 0 {
                continue;
            }
            max = Some(max.map_or(tokens, |current: i64| current.max(tokens)));
            if window.get("is_default").and_then(Value::as_bool) == Some(true) {
                default = Some(tokens);
            }
        }
    }
    if max.is_none() {
        max = entry.get("max_input_tokens").and_then(Value::as_i64);
    }
    (default.filter(|value| Some(*value) < max), max)
}

/// 按开关选要用的窗口（默认取上限）。
pub fn selected_window(entry: &Value, max_context: bool) -> Option<i64> {
    let (default, max) = context_windows(entry);
    if max_context {
        max.or(default)
    } else {
        default.or(max)
    }
}

/// 从清单里找某模型的元数据（大小写不敏感 —— 上游对模型名大小写不敏感）。
///
/// 找不到 ⇒ 合成一份**中性元数据**（`key = 用户传的名字`）：不虚构
/// `is_reasoning` / 上下文长度，上游收到不认识的 key 会自己报错 ——
/// 比本地猜一个「像真的」的值诚实。
pub fn meta_for(
    catalog: &[Value],
    model: &str,
    max_context: bool,
) -> buddy_switch_core::modules::qoder::api::ModelMeta {
    for entry in catalog {
        let key = entry
            .get("key")
            .or_else(|| entry.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if key.eq_ignore_ascii_case(model) {
            let mut meta =
                buddy_switch_core::modules::qoder::api::ModelMeta::from_catalog_entry(entry);
            // ★ 请求里的窗口跟着开关走：开 = 上限档，关 = 默认档。
            //   上游 `max_input_tokens` 是**单值**，所以这里直接覆盖成选中的窗口。
            if let Some(window) = selected_window(entry, max_context) {
                meta.max_input_tokens = window;
            }
            return meta;
        }
    }
    buddy_switch_core::modules::qoder::api::ModelMeta {
        key: model.to_string(),
        display_name: model.to_string(),
        is_vl: false,
        is_reasoning: false,
        max_input_tokens: 180_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 出口①必须带 `context_length`（OpenAI 兼容客户端读这个字段），
    /// 与 Trae / WB 三平台同名同义；`max_input_tokens` 保留为上游原字段。
    #[test]
    fn models_response_declares_openai_context_length() {
        let catalog = vec![json!({
            "key": "Qwen3.8-Max",
            "display_name": "Qwen3.8-Max",
            "max_input_tokens": 200_000,
        })];
        let body = models_response(&catalog, &BTreeSet::new(), true);
        assert_eq!(body["data"][0]["context_length"], 200_000);
        assert_eq!(body["data"][0]["max_input_tokens"], 200_000);
    }

    /// ★ 多窗口：开关开 ⇒ 声明 `context_config` 的最大档（1M），关 ⇒ `is_default` 档（200K）；
    /// 请求里的 `model_config.max_input_tokens` 同源。
    #[test]
    fn context_switch_picks_max_or_default_window() {
        let catalog = vec![json!({
            "key": "qmodel_38max",
            "display_name": "Qwen3.8-Max",
            "max_input_tokens": 180_000,
            "context_config": {
                "200K": {"token_count": 200_000, "is_default": true},
                "400K": {"token_count": 400_000},
                "1M": {"token_count": 1_000_000}
            }
        })];

        let max = models_response(&catalog, &BTreeSet::new(), true);
        assert_eq!(max["data"][0]["context_length"], 1_000_000);
        assert_eq!(max["data"][0]["context_length_default"], 200_000);
        assert_eq!(max["data"][0]["context_length_max"], 1_000_000);
        let lower = models_response(&catalog, &BTreeSet::new(), false);
        assert_eq!(lower["data"][0]["context_length"], 200_000);

        assert_eq!(
            meta_for(&catalog, "qmodel_38max", true).max_input_tokens,
            1_000_000
        );
        assert_eq!(
            meta_for(&catalog, "qmodel_38max", false).max_input_tokens,
            200_000
        );
    }

    /// 没有 `context_config` 的老条目 ⇒ 用 `max_input_tokens` 单档，开关不改变结果。
    #[test]
    fn context_switch_handles_single_window_entries() {
        let catalog = vec![json!({"key": "legacy", "max_input_tokens": 128_000})];
        assert_eq!(selected_window(&catalog[0], true), Some(128_000));
        assert_eq!(selected_window(&catalog[0], false), Some(128_000));
    }

    #[test]
    fn model_defaulting_and_stream_flag() {
        assert_eq!(model_of(&json!({"model": "ultimate"}), "x"), "ultimate");
        assert_eq!(model_of(&json!({}), "fallback"), "fallback");
        assert_eq!(model_of(&json!({"model": ""}), "fallback"), "fallback");
        assert!(!wants_stream(&json!({})));
        assert!(wants_stream(&json!({"stream": true})));
    }

    #[test]
    fn sticky_key_requires_metadata_ids() {
        assert_eq!(
            sticky_key_of(&json!({"metadata": {"conversation_id": "c-1"}})),
            Some("c-1".into())
        );
        assert_eq!(
            sticky_key_of(&json!({"metadata": {"session_id": "s-1"}})),
            Some("s-1".into())
        );
        // Responses 客户端（Codex）没有 `metadata`，靠 `prompt_cache_key` 粘同一段会话。
        assert_eq!(
            sticky_key_of(&json!({"prompt_cache_key": "p-1"})),
            Some("p-1".into())
        );
        // `metadata` 优先（同一请求两处都给时以显式会话 id 为准）。
        assert_eq!(
            sticky_key_of(&json!({
                "metadata": {"conversation_id": "c-1"},
                "prompt_cache_key": "p-1",
            })),
            Some("c-1".into())
        );
        assert_eq!(sticky_key_of(&json!({})), None);
        assert_eq!(sticky_key_of(&json!({"metadata": {}})), None);
        assert_eq!(sticky_key_of(&json!({"prompt_cache_key": ""})), None);
    }

    /// Responses → chat：复用 `protocol::responses` 的那份翻译，并按 Qoder 上游
    /// 的读法补上思考档位（`reasoning.effort`）。
    #[test]
    fn responses_to_chat_keeps_translation_and_reasoning_effort() {
        let parsed = json!({
            "model": "ultimate",
            "instructions": "be terse",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
            ],
            "tools": [{"type": "function", "name": "pwsh", "description": "d"}],
            "reasoning": {"effort": "high", "summary": "auto"},
            "max_output_tokens": 256,
        });
        let payload = responses_to_chat(&parsed);
        assert_eq!(payload["model"], "ultimate");
        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][0]["content"], "be terse");
        assert_eq!(payload["messages"][1]["content"], "hi");
        assert_eq!(payload["tools"][0]["function"]["name"], "pwsh");
        assert_eq!(payload["max_tokens"], 256);
        // Qoder 的 `build_chat_body` 读 `reasoning.effort` 定档位。
        assert_eq!(payload["reasoning"]["effort"], "high");
        // 翻译结果必须能过 `build_chat_body` 的思考档位解析。
        assert_eq!(
            buddy_switch_core::modules::qoder::api::extract_requested_effort(&payload),
            Some("high".into())
        );
        // 无 `reasoning` 时不凭空插一个空对象。
        assert!(responses_to_chat(&json!({"model": "m", "input": "hi"}))
            .get("reasoning")
            .is_none());
    }

    /// 禁用过滤走**唯一判断点**的 `is_disabled_in`（大小写不敏感）。
    #[test]
    fn models_response_filters_disabled() {
        let catalog = vec![
            json!({"id": "Ultimate", "display_name": "Ultimate"}),
            json!({"id": "Qwen3.8-Max", "display_name": "Qwen3.8-Max"}),
        ];
        let mut disabled = BTreeSet::new();
        disabled.insert("ultimate".to_string());
        let response = models_response(&catalog, &disabled, true);
        let ids: Vec<&str> = response["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["Qwen3.8-Max"], "被禁模型必须消失（大小写不敏感）");
    }

    #[test]
    fn meta_lookup_is_case_insensitive_with_neutral_fallback() {
        let catalog = vec![
            json!({"key": "qwen3.8-max", "display_name": "Qwen3.8-Max", "is_reasoning": true}),
        ];
        let meta = meta_for(&catalog, "QWEN3.8-MAX", true);
        assert_eq!(meta.key, "qwen3.8-max");
        assert!(meta.is_reasoning);
        // 未知名 ⇒ 中性合成（不猜 is_reasoning）。
        let unknown = meta_for(&catalog, "mystery-model", true);
        assert_eq!(unknown.key, "mystery-model");
        assert!(!unknown.is_reasoning);
    }

    #[test]
    fn allowed_tool_names_reads_both_shapes() {
        let parsed = json!({"tools": [
            {"type": "function", "function": {"name": "a"}},
            {"type": "function", "name": "b"},
        ]});
        let names = allowed_tool_names(&parsed).unwrap();
        assert!(names.contains("a") && names.contains("b"));
        assert!(allowed_tool_names(&json!({})).is_none());
    }

    /// 管理面清单：**未过滤**（禁用的仍在，但带标记）+ 来源三级原样透出。
    ///
    /// 变异：把 `admin_models_response` 改成走 [`models_response`] 的过滤 ⇒
    /// 第一条断言红（禁用行消失，用户会以为它从上游没了）。
    #[test]
    fn admin_models_response_keeps_disabled_rows_but_marks_them() {
        let catalog = super::super::CatalogView {
            models: vec![
                json!({"id": "Ultimate", "display_name": "Ultimate", "is_reasoning": true}),
                json!({"key": "glm-5.1", "display_name": "GLM-5.1"}),
            ],
            source: "cached",
            fetched_at: Some(1_700_000_000_000),
            note: Some("本次拉取失败".to_string()),
        };
        let mut disabled = BTreeSet::new();
        disabled.insert("ultimate".to_string());

        let body = admin_models_response(QoderRegion::Global, &catalog, &disabled, true);
        assert_eq!(body["region"], "global");
        assert_eq!(body["source"], "cached");
        assert_eq!(body["fetchedAt"], 1_700_000_000_000i64);
        assert_eq!(body["note"], "本次拉取失败");
        let rows = body["data"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "禁用的行**必须留着**（全量清单）");
        assert_eq!(rows[0]["id"], "Ultimate");
        assert_eq!(
            rows[0]["disabled"], true,
            "大小写不敏感（`ultimate` ↔ `Ultimate`）"
        );
        assert_eq!(rows[0]["is_reasoning"], true);
        assert_eq!(rows[1]["id"], "glm-5.1", "无 `id` 时回落 `key`");
        assert_eq!(rows[1]["disabled"], false);
    }
}
