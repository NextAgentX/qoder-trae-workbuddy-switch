//! `GET /v1/models`：按 Key 绑定 region 返回模型列表 + 来源标注（P0-4）。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::json;

use crate::disabled_models::is_disabled;
use crate::routes::{authenticate, json_response, openai_error};
use crate::state::GatewayState;
use buddy_switch_core::modules::catalog::{CatalogModel, CatalogSnapshot};
use buddy_switch_core::modules::product::{scope_of_wb_region, Scope};

/// 一条 WB 模型在 OpenAI `/v1/models` 里的形状（**唯一产出点**：独立网关与统一网关共用）。
///
/// ★ `context_length` 必须与 Trae 侧同名字段对齐 —— OpenAI 兼容客户端读的是它；
/// `context_window` 保留给老客户端与管理面（WB 目录原本就用这个名字）。
///
/// ★ 图片能力（issue #101）走 [`crate::model_capability::apply`]：上游 `supportsImages`
/// 优先、用户在「模型」页的覆盖次之、两边都没有就**不下发**该字段。这里**只传上游原值**，
/// 合并判断只有一处（`model_capability`），别在此处再写一遍 `if override`。
///
/// ★ 思考档位（issue #127）与**管理面**（[`snapshot_view`]）同源同值：上游目录声明的
/// `reasoning.efforts` 优先、内置静态表次之，`efforts_source` 标明是哪一份。
/// 客户端据此才知道「这个模型能传哪些 `reasoning_effort`」—— 改造前只有管理面有，
/// 出口 ① 一条都不给，客户端只能猜（猜错的档位会被 [`crate::outbound::effort`] 静默降级）。
///
/// ⚠️ 两边都没声明时**不写这三个字段**（不是写空数组）：`[]` 会被读成「该模型不支持思考」，
/// 而真相是「上游没下发、内置表也没有」—— 与 `supports_images` 的取舍一致，不编事实。
pub fn model_entry(model: &CatalogModel, max_context: bool, scope: Scope) -> serde_json::Value {
    // 开关决定声明「上限档」还是「默认档」（上游两档：如 300k 默认 / 1M 上限）。
    let window = if max_context {
        model.context_window
    } else {
        model.context_window_default.unwrap_or(model.context_window)
    };
    let mut value = json!({
        "id": model.id,
        "object": "model",
        "created": 0,
        "owned_by": "workbuddy",
        "context_length": window,
        // 两档原值：前端据此本地即时切换，不必等重新取数。
        "context_length_default": model.context_window_default,
        "context_length_max": model.context_window,
        "context_window": window,
        "max_tokens": model.max_tokens,
        // 原文（上游怎么写就怎么给）+ 数字（同一份解析的唯一出口）。
        "credits": model.credits,
        "credits_rate": model.credits_rate(),
        "badges": model.badges,
        // 判零用 `is_free()`（认 `x0.00 credits`），不读落盘的 `free` 快照。
        "free": model.is_free(),
    });
    if let Some(object) = value.as_object_mut() {
        crate::model_capability::apply(object, scope, &model.id, Some(model.supports_images));
        // 思考档位（issue #127）：与 `snapshot_view` 共用 `ModelAbility::resolve`，
        // 否则「管理面显示的档位」与「网关下发的档位」迟早漂移。
        if let Some(region) = scope.wb_region() {
            let ability =
                ModelAbility::resolve(model, crate::outbound::effort::static_effort_cap(region));
            if !ability.efforts.is_empty() {
                object.insert(
                    "reasoning_efforts".into(),
                    serde_json::json!(ability.efforts),
                );
                object.insert(
                    "default_effort".into(),
                    serde_json::json!(ability.default_effort),
                );
                object.insert("efforts_source".into(), serde_json::json!(ability.source));
            }
        }
    }
    value
}

/// 管理面用的目录快照视图：把每个模型的 `context_window` 按开关换成**生效窗口**，
/// 并补上 `context_length`（与网关出口 ① 同源同值）。
///
/// 没有这一步，管理面会一直显示目录原始字段（上限档），开关在界面上看起来「没反应」。
///
/// ## 思考档位：上游声明 > 内置静态表 > 无（issue #65）
///
/// 上游目录的 `reasoning.supportedEfforts` 只在**部分模型**上出现（实测同一份 CN 目录
/// 里 `hy3` / `gpt-5.5` 有，`fast-model` / `kimi-k3` 只给了 `effort`）。缺声明时回落
/// `outbound::effort::static_effort_cap` 的内置表 —— 但**必须标明来源**：
/// 静态表与官方客户端不一致时，用户会以为是网关读错了。`efforts_source` 就是那条标签
/// （`"upstream"` / `"static"` / `null`）。
///
/// ⚠️ 原始 `reasoning` 块**不从本视图下发**：同一个事实给两份表示（原文 + 解析结果）
/// 迟早漂移，界面只认解析后的三个字段。
pub fn snapshot_view(snapshot: &CatalogSnapshot, max_context: bool) -> serde_json::Value {
    let table = crate::outbound::effort::static_effort_cap(snapshot.region);
    let mut snapshot = snapshot.clone();
    // 先记上限档，再把 `context_window` 换成生效档 —— 前端要两档原值才能本地即时切换。
    let maxes: Vec<u64> = snapshot.models.iter().map(|m| m.context_window).collect();
    let abilities: Vec<ModelAbility> = snapshot
        .models
        .iter()
        .map(|model| ModelAbility::resolve(model, table))
        .collect();
    for model in &mut snapshot.models {
        model.context_window = model.effective_window(max_context);
        // 老缓存可能带着按旧判据算的 `free`（`x0.00 credits` 判成收费）⇒ 出口重算一次。
        model.refresh_derived();
    }
    let mut value = serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null);
    if let Some(rows) = value.get_mut("models").and_then(|v| v.as_array_mut()) {
        // ★ 图片能力（issue #101）：与出口 ① 走**同一个**解析点 —— 管理面显示的能力必须
        //   就是网关真正下发的那一份，否则用户改了覆盖、界面点头、客户端却看不到。
        //   行里此刻的 `supports_images` 是 `CatalogModel` 序列化出来的**上游原值**，
        //   先读出来当 `upstream`，再由 `apply` 用覆盖压过它。
        let scope = scope_of_wb_region(snapshot.region);
        for ((row, max), ability) in rows.iter_mut().zip(maxes).zip(abilities) {
            if let Some(object) = row.as_object_mut() {
                object.insert("context_length_max".into(), serde_json::json!(max));
                object.remove("reasoning");
                object.insert(
                    "credits_rate".into(),
                    serde_json::json!(ability.credits_rate),
                );
                object.insert(
                    "reasoning_efforts".into(),
                    serde_json::json!(ability.efforts),
                );
                object.insert(
                    "default_effort".into(),
                    serde_json::json!(ability.default_effort),
                );
                object.insert("efforts_source".into(), serde_json::json!(ability.source));
                let id = object
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let upstream = object.get("supports_images").and_then(|v| v.as_bool());
                crate::model_capability::apply(object, scope, &id, upstream);
            }
        }
    }
    value
}

/// 一条模型的「能力面」（思考档位 + 积分倍率），供管理面渲染。
struct ModelAbility {
    efforts: Vec<String>,
    default_effort: Option<String>,
    /// `"upstream"`（上游目录声明）/ `"static"`（内置档位表）/ `None`（两边都没有）。
    source: Option<&'static str>,
    credits_rate: Option<f64>,
}

impl ModelAbility {
    fn resolve(
        model: &CatalogModel,
        table: &std::collections::HashMap<&'static str, crate::outbound::effort::EffortSpec>,
    ) -> Self {
        let upstream = model.reasoning.as_ref().filter(|r| !r.efforts.is_empty());
        let spec = table.get(model.id.trim()).filter(|s| !s.efforts.is_empty());
        let (efforts, source) = match (upstream, spec) {
            (Some(reasoning), _) => (reasoning.efforts.clone(), Some("upstream")),
            (None, Some(spec)) => (spec.efforts.clone(), Some("static")),
            (None, None) => (Vec::new(), None),
        };
        // 默认档只取「确实可选」的那一档：默认值不在可选集里就是上游自相矛盾，
        // 展示它会直接误导（用户照着它传请求会被降级）。
        let default_effort = model
            .reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.default_effort.clone())
            .or_else(|| spec.and_then(|spec| spec.default_effort.clone()))
            .filter(|effort| !efforts.is_empty() && efforts.contains(effort));
        Self {
            efforts,
            default_effort,
            source,
            credits_rate: model.credits_rate(),
        }
    }
}

/// 模型列表处理器。
pub async fn handler(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    let record = match authenticate(&state, &headers) {
        Ok(record) => record,
        Err(error) => return openai_error(error),
    };

    let snapshot = state.catalogs.current(record.region);
    // ★ 出口 ①（P11）：语义是「**可用的**模型清单」——不可服务与被用户禁用的都**不出现**。
    //   WB 侧 `available` 恒 true（无「非可服务」概念，故此处无 no-op 过滤）；
    //   禁用过滤读的是**与出口 ② 同一份** `disabled`（`is_disabled` 是唯一判断点）。
    let disabled = state.disabled.read().await;
    let max_context = state.config.read().await.max_context;
    // 作用域一次算好：禁用过滤与图片能力解析读的是**同一个**作用域（WB 的 region → Scope）。
    let scope = scope_of_wb_region(record.region);
    let data: Vec<serde_json::Value> = snapshot
        .models
        .iter()
        .filter(|model| !is_disabled(&disabled, scope, &model.id))
        .map(|model| model_entry(model, max_context, scope))
        .collect();
    drop(disabled);

    json_response(
        StatusCode::OK,
        json!({
            "object": "list",
            "data": data,
            "wb_region": snapshot.region,
            "wb_source": snapshot.source,
            "wb_fetched_at": snapshot.fetched_at,
            "wb_note": snapshot.note,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_switch_core::modules::catalog::CatalogReasoning;

    /// ★ 出口①必须同时给 `context_length`（OpenAI 客户端读）与 `context_window`（老字段）。
    #[test]
    fn wb_model_entry_declares_openai_context_length() {
        let model = CatalogModel {
            id: "hy4-preview".into(),
            name: "Hy4 preview".into(),
            context_window: 960_000,
            context_window_default: Some(300_000),
            max_tokens: 64_000,
            supports_images: true,
            credits: None,
            badges: vec![],
            free: false,
            reasoning: None,
        };
        let max = model_entry(&model, true, Scope::WbCn);
        assert_eq!(max["context_length"], 960_000, "默认用上限档");
        assert_eq!(max["context_window"], 960_000);

        // 开关关 ⇒ 声明默认档（上游 contextWindow.defaultLength）。
        let lower = model_entry(&model, false, Scope::WbCn);
        assert_eq!(lower["context_length"], 300_000);
        assert_eq!(lower["context_window"], 300_000);
    }

    /// ★ issue #101：出口 ① 必须把**图片能力**写进清单 —— 客户端就是靠它决定要不要发图。
    ///
    /// 变异：删掉 `model_entry` 里的 `model_capability::apply` ⇒ 本条红（客户端永远看不到能力，
    /// 也就永远自己把图片挡掉 —— 正是 issue #101 的现场）。
    #[test]
    fn wb_model_entry_declares_image_capability() {
        let mut model = CatalogModel {
            id: "issue101-image-probe".into(),
            name: "probe".into(),
            context_window: 200_000,
            context_window_default: None,
            max_tokens: 32_000,
            supports_images: true,
            credits: None,
            badges: vec![],
            free: false,
            reasoning: None,
        };
        let entry = model_entry(&model, true, Scope::WbCn);
        assert_eq!(entry["supports_images"], true);
        assert_eq!(entry["image_input_source"], "upstream");
        // OpenRouter 兼容字段：主流客户端读的是它，不读我们的私有 `supports_images`。
        assert_eq!(
            entry["architecture"]["input_modalities"],
            serde_json::json!(["text", "image"])
        );
        assert_eq!(entry["architecture"]["modality"], "text+image->text");

        // 上游明确说不支持 ⇒ 给 `false`（客户端据此隐藏上传入口），**不是**省略。
        model.supports_images = false;
        let entry = model_entry(&model, true, Scope::WbCn);
        assert_eq!(entry["supports_images"], false);
        assert_eq!(
            entry["architecture"]["input_modalities"],
            serde_json::json!(["text"])
        );
    }

    /// 没有默认档的上游（如国内目录只给一个数）⇒ 关开关也回落到上限，不能变 0。
    #[test]
    fn wb_model_entry_falls_back_when_no_default_window() {
        let model = CatalogModel {
            id: "glm-5.3".into(),
            name: "GLM-5.3".into(),
            context_window: 1_000_000,
            context_window_default: None,
            max_tokens: 48_000,
            supports_images: true,
            credits: None,
            badges: vec![],
            free: false,
            reasoning: None,
        };
        assert_eq!(
            model_entry(&model, false, Scope::WbCn)["context_length"],
            1_000_000
        );
    }

    fn model(id: &str, credits: Option<&str>, reasoning: Option<CatalogReasoning>) -> CatalogModel {
        let mut model = CatalogModel {
            id: id.into(),
            name: id.into(),
            context_window: 200_000,
            context_window_default: None,
            max_tokens: 32_000,
            supports_images: false,
            credits: credits.map(str::to_string),
            badges: vec![],
            free: false,
            reasoning,
        };
        model.refresh_derived();
        model
    }

    fn snapshot(models: Vec<CatalogModel>) -> CatalogSnapshot {
        CatalogSnapshot {
            region: buddy_switch_core::modules::region::Region::Cn,
            source: buddy_switch_core::modules::catalog::CatalogSource::Live,
            fetched_at: Some(1),
            models,
            note: None,
        }
    }

    /// ★ issue #127：出口 ①（`/v1/models`）必须下发思考档位，且与管理面同源同值。
    ///
    /// 改造前只有管理面（`snapshot_view`）有 `reasoning_efforts`，客户端拿到的清单里
    /// 一条都没有 —— 用户只能猜档位，猜错的会被 `outbound::effort` 静默降级。
    ///
    /// 变异：删掉 `model_entry` 里那段 `ModelAbility::resolve` ⇒ 前两条断言红。
    #[test]
    fn wb_model_entry_declares_reasoning_efforts() {
        // ① 上游声明优先，`efforts_source = "upstream"`。
        let upstream = model(
            "kimi-k2.8-preview",
            None,
            Some(CatalogReasoning {
                efforts: vec!["low".into(), "high".into(), "max".into()],
                default_effort: Some("high".into()),
                can_disable_thinking: None,
            }),
        );
        let entry = model_entry(&upstream, true, Scope::WbCn);
        assert_eq!(
            entry["reasoning_efforts"],
            serde_json::json!(["low", "high", "max"])
        );
        assert_eq!(entry["default_effort"], "high");
        assert_eq!(entry["efforts_source"], "upstream");

        // ② 上游没声明 ⇒ 回落内置静态表，来源必须标明 `"static"`（否则用户以为网关读错了）。
        let fallback = model_entry(&model("glm-5.3", None, None), true, Scope::WbCn);
        assert!(
            fallback["reasoning_efforts"]
                .as_array()
                .is_some_and(|list| !list.is_empty()),
            "{fallback}"
        );
        assert_eq!(fallback["efforts_source"], "static");

        // ③ 两边都没有 ⇒ **不下发**这三个字段（不是空数组：空数组会被读成「不支持思考」）。
        let unknown = model_entry(&model("not-anywhere", None, None), true, Scope::WbCn);
        assert!(unknown.get("reasoning_efforts").is_none(), "{unknown}");
        assert!(unknown.get("efforts_source").is_none(), "{unknown}");
    }

    /// ★ issue #65：思考档位三档来源必须能区分 —— 上游声明 / 内置静态表 / 都没有。
    ///
    /// 变异：把 `ModelAbility::resolve` 里的 `source` 一律写成 `"static"` ⇒ 第一条断言红；
    /// 把静态表回落删掉 ⇒ 第二条红（`glm-5.3` 在 CN 表里，必须能显示 low/high/max）。
    #[test]
    fn snapshot_view_resolves_efforts_with_source() {
        let view = snapshot_view(
            &snapshot(vec![
                // 上游声明（`kimi-k2.8-preview` 不在任何静态表里）
                model(
                    "kimi-k2.8-preview",
                    Some("x1.65"),
                    Some(CatalogReasoning {
                        efforts: vec!["low".into(), "high".into(), "max".into()],
                        default_effort: Some("high".into()),
                        can_disable_thinking: Some(true),
                    }),
                ),
                // 上游没声明、静态表有
                model("glm-5.3", Some("x0.79"), None),
                // 两边都没有
                model("not-anywhere", None, None),
            ]),
            true,
        );
        let rows = view["models"].as_array().unwrap();

        assert_eq!(rows[0]["efforts_source"], "upstream");
        assert_eq!(
            rows[0]["reasoning_efforts"],
            serde_json::json!(["low", "high", "max"])
        );
        assert_eq!(rows[0]["default_effort"], "high");
        assert_eq!(rows[0]["credits_rate"], 1.65);
        // ★ 原始 reasoning 块不下发（同一事实只留一份表示）
        assert!(rows[0].get("reasoning").is_none());

        assert_eq!(rows[1]["efforts_source"], "static");
        assert_eq!(
            rows[1]["reasoning_efforts"],
            serde_json::json!(["low", "high", "max"]),
            "glm-5.3 的 CN 静态表是 low/high/max"
        );
        assert_eq!(rows[1]["default_effort"], "high");

        assert!(rows[2]["efforts_source"].is_null());
        assert_eq!(rows[2]["reasoning_efforts"], serde_json::json!([]));
        assert!(rows[2]["default_effort"].is_null());
        assert!(rows[2]["credits_rate"].is_null());
    }

    /// ★ issue #65 顺带修的缺陷：`x0.00 credits` 也是免费（旧判据只认 `x0.00`）。
    #[test]
    fn zero_credits_with_suffix_is_free() {
        let with_suffix = model("hy3", Some("x0.00 credits"), None);
        assert_eq!(with_suffix.credits_rate(), Some(0.0));
        assert!(with_suffix.is_free(), "`x0.00 credits` 必须判免费");
        assert_eq!(model_entry(&with_suffix, true, Scope::WbCn)["free"], true);
        assert_eq!(
            model_entry(&with_suffix, true, Scope::WbCn)["credits_rate"],
            0.0
        );

        // 非零带后缀照常给数字（前端不必各自剥后缀）
        let paid = model("fast-model", Some("x0.34 credits"), None);
        assert_eq!(paid.credits_rate(), Some(0.34));
        assert!(!paid.is_free());
    }

    /// 默认档不在可选集里 ⇒ 不展示（上游自相矛盾时宁可不显示，也别误导）。
    #[test]
    fn default_effort_outside_supported_set_is_dropped() {
        let view = snapshot_view(
            &snapshot(vec![model(
                "weird",
                None,
                Some(CatalogReasoning {
                    efforts: vec!["low".into()],
                    default_effort: Some("max".into()),
                    can_disable_thinking: None,
                }),
            )]),
            true,
        );
        assert_eq!(
            view["models"][0]["reasoning_efforts"],
            serde_json::json!(["low"])
        );
        assert!(view["models"][0]["default_effort"].is_null());
    }
}
