//! 模型能力声明（**图片输入**）—— 出口 ① 与「管理面模型表」共用的**唯一解析点**。
//!
//! ## 为什么需要它（issue #101）
//!
//! 用户在客户端里接的是本网关，客户端**只认自己那份模型注册表**：模型条目没勾「支持图片输入」
//! 时，它在**发请求前**就把图片挡掉了 —— 请求根本没到网关。要让它自动判断，网关必须把
//! 「这个模型吃不吃图」写在客户端读得到的地方（`GET /v1/models`）。
//!
//! 三条产品线的现状**完全不同**，所以需要一层统一解析，而不是各自拼字段：
//!
//! | 产品线 | 上游是否声明 |
//! |:--|:--|
//! | WorkBuddy | ✅ 目录里有 `supportsImages`（且 `disabledMultimodal` 是否决项） |
//! | Trae | ❌ 客户端清单只有 name / 上下文窗口 |
//! | Qoder | ❌ 官方清单没有这个字段 |
//!
//! ⇒ 上游声明优先，用户覆盖次之，两边都没有就**不下发该字段**（`None`）。
//!
//! ## ★ 三态：`None` ≠ `false`（本模块最重要的一条）
//!
//! - `Some(true)` ⇒ 下发 `supports_images: true`；
//! - `Some(false)` ⇒ 下发 `supports_images: false`（客户端据此隐藏上传入口）；
//! - `None` ⇒ **字段整个不出现**。
//!
//! 「上游没说」与「上游说不行」是两件事。把它们压成 `false` 会让 Trae / Qoder 的全部模型
//! 凭空多出一句「不支持图片」的断言 —— 那是**网关编的**，而它并没有这个信息。
//! 沿用既有哲学（§3.4 规则 1「无数据不渲染」）：没数据就不下发，客户端回落到自己的配置。
//!
//! ## ★ 为什么 `supports_images` 之外还要下发 `architecture`（OpenRouter 兼容）
//!
//! `supports_images` 是**本仓自定义**字段，不在 OpenAI 规范里 —— 实测主流客户端
//! （含报障用的 DeepSeek Harness）**不读它**。OpenRouter 生态的客户端读的是
//! `architecture.input_modalities`。两套都写，能自动识别的客户端才多起来；
//! 只写一套等于把「自动识别」寄托在客户端恰好认识我们的私有字段上。
//!
//! ## ★ 声明必须与转发同源（硬约束）
//!
//! `supports_images` 描述的是**模型能力**，不是「本网关一定会转发」。数据面现状：
//! - `POST /v1/chat/completions`（OpenAI 兼容）：图片 parts **原样透传**
//!   （`outbound::sanitize` 只改 `text` part）；
//! - `POST /v1/messages`（Anthropic）：默认**丢弃**，仅 `BUDDY_SWITCH_ANTHROPIC_FORWARD_IMAGES=1`
//!   才转发，丢弃时会在文本里写明「已移除 N 张图片」（`protocol::anthropic`）。
//!
//! 两者不矛盾，但**改其中一边必须回头看另一边**：若哪天数据面彻底不支持图片，
//! 这里继续声明 `true` 就是撒谎，比不声明更坏（客户端发图 ⇒ 模型答非所问）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::product::Scope;

/// 图片输入能力的**来源**（前端据此显示「来自上游 / 你手动指定」，并提供「还原上游」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageCapSource {
    /// 上游目录声明（WB 的 `supportsImages`）—— 权威，用户没动过。
    Upstream,
    /// 用户在「模型」页手动指定（上游没声明、或声明错了）。
    Override,
    /// **内置表**声明（上游没这个字段，但该模型已确认能吃图 —— issue #142）。
    ///
    /// 与 `Upstream` 分开是为了**不撒谎**：界面上「来自上游」和「网关内置表说的」
    /// 是两件事，前者权威、后者可被用户覆盖。
    Known,
}

impl ImageCapSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Override => "override",
            Self::Known => "known",
        }
    }
}

/// 解析结果：能力值 + 它的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCapability {
    pub supports: bool,
    pub source: ImageCapSource,
}

/// 按**作用域**的图片能力覆盖（用户偏好）。
///
/// 形状 `HashMap<作用域, HashMap<模型 id, bool>>`：**缺失的条目 = 跟随上游**（不是 `false`）。
/// 这一点决定了「还原上游」只需删键，不需要第三态枚举。
///
/// 与 `disabled_models` **同构**（同样按作用域键控、同样进程内唯一实例），
/// 但**刻意不合并成一个 store**：禁用是「能不能调」，能力是「怎么调」，两者的
/// 默认值方向相反（禁用默认全开、能力默认跟随上游），混在一起会让「缺省」有两种解释。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImageCaps {
    /// 键是作用域 id（[`Scope::as_str`] 的字面量）。
    ///
    /// ⚠️ 用 `String` 而非 `Scope` 作键：`Scope` 是枚举，未知键会让整份反序列化失败
    /// ⇒ `load()` 回落空表 ⇒ 一个未来新增的作用域会**清空全部**用户指定（与
    /// `disabled_models` 同一个脚枪，那边已经踩过）。
    scopes: HashMap<String, HashMap<String, bool>>,
}

impl ImageCaps {
    /// 从 `~/.buddy-switch/gateway_model_image_caps.json` 读取；缺失 / 损坏回落空表。
    pub fn load() -> Self {
        let file = file();
        std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<ImageCaps>(&text).ok())
            .unwrap_or_default()
    }

    /// 原子写回。
    pub fn save(&self) -> Result<(), String> {
        let file = file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        core_config::atomic_write(&file, &content).map_err(|error| error.to_string())
    }

    /// 某作用域的覆盖表（只读；缺失即空表）。
    pub fn get(&self, scope: Scope) -> &HashMap<String, bool> {
        static EMPTY: OnceLock<HashMap<String, bool>> = OnceLock::new();
        self.scopes
            .get(scope.as_str())
            .unwrap_or_else(|| EMPTY.get_or_init(HashMap::new))
    }

    /// 指定 / 清除（`supports = None` ⇒ 删键 = 跟随上游）；返回是否**发生了变化**（幂等）。
    ///
    /// 空白 `model_id` 视为无效输入，直接返回 `false`（不写脏数据）。
    pub fn set(&mut self, scope: Scope, model_id: &str, supports: Option<bool>) -> bool {
        let id = model_id.trim();
        if id.is_empty() {
            return false;
        }
        let table = self.scopes.entry(scope.as_str().to_string()).or_default();
        match supports {
            Some(value) => {
                // 大小写敏感地**先清同名异写**：`GLM-5.3` 与 `glm-5.3` 是同一个模型，
                // 留着两条会在「还原上游」后残留一条反方向的旧覆盖（判断大小写不敏感，见 `lookup`）。
                let stale: Vec<String> = table
                    .keys()
                    .filter(|key| key.eq_ignore_ascii_case(id) && key.as_str() != id)
                    .cloned()
                    .collect();
                for key in stale {
                    table.remove(&key);
                }
                table.insert(id.to_string(), value) != Some(value)
            }
            None => {
                let stale: Vec<String> = table
                    .keys()
                    .filter(|key| key.eq_ignore_ascii_case(id))
                    .cloned()
                    .collect();
                let changed = !stale.is_empty();
                for key in stale {
                    table.remove(&key);
                }
                changed
            }
        }
    }

    /// 大小写不敏感查找（上游对模型名不敏感，`trae::payload::model_config` 也先 `to_lowercase`）。
    ///
    /// 与 `disabled_models::is_disabled_in` 同一理由：精确匹配会让 `GLM-5.3` 绕过 `glm-5.3`
    /// 的覆盖 ⇒ 同一个模型两副面孔。
    fn lookup(&self, scope: Scope, model_id: &str) -> Option<bool> {
        let id = model_id.trim();
        if id.is_empty() {
            return None;
        }
        self.get(scope)
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(id))
            .map(|(_, value)| *value)
    }
}

/// 覆盖文件路径：`~/.buddy-switch/gateway_model_image_caps.json`（与 `gateway_config.json` 同目录）。
pub fn file() -> PathBuf {
    buddy_switch_core::modules::region::gateway_config_file()
        .with_file_name("gateway_model_image_caps.json")
}

/// 进程内**唯一**的覆盖存储：三个网关（各自 `/v1/models`）+ 统一网关 + 命令面共用一份。
///
/// 与 `disabled_models::shared()` 同一理由 —— 各持一份内存副本必然出现「命令写到 A、
/// 网关读 B」的假开关。
///
/// ## ★ 为什么用 **std** 锁（而不是 tokio 的 `RwLock`）
///
/// 读取点在**同步**的清单构造器里（`routes::models::model_entry` /
/// `trae::payload::models_response_from` / `qoder::payload::models_response`），而这些构造器
/// 会在**两种上下文**被调用：Trae 侧在 `spawn_blocking` 里（那里 `blocking_read` 可用、
/// `.await` 不可用），统一网关在 **async** 上下文里直接调同一个函数（那里 `blocking_read`
/// 会 **panic**）。tokio 锁在这两处**必有一处炸**，因此这里只保护一张内存小表、用 std 锁：
/// 临界区是一次 `HashMap` 查找（纳秒级），**绝不在持锁时做 IO** —— 落盘由 [`set_and_save`]
/// 在锁外完成（调用方负责放进 `spawn_blocking`）。
static SHARED: OnceLock<RwLock<ImageCaps>> = OnceLock::new();

/// 取共享存储（首次调用时从磁盘加载）。
pub fn shared() -> &'static RwLock<ImageCaps> {
    SHARED.get_or_init(|| RwLock::new(ImageCaps::load()))
}

/// 某作用域的覆盖表快照（克隆一份，供管理面命令序列化用）。
pub fn overrides_of(scope: Scope) -> HashMap<String, bool> {
    shared()
        .read()
        .map(|guard| guard.get(scope).clone())
        .unwrap_or_default()
}

/// **唯一解析点**（全局版）：上游声明 > 用户覆盖；两边都没有 ⇒ `None`（不下发字段）。
///
/// 读锁中毒时回落「只有上游声明」—— 宁可少一个用户覆盖，也不要让整张清单 500。
pub fn resolve_effective(
    scope: Scope,
    model_id: &str,
    upstream: Option<bool>,
) -> Option<ImageCapability> {
    let override_value = shared()
        .read()
        .ok()
        .and_then(|guard| guard.lookup(scope, model_id));
    if let Some(supports) = override_value {
        return Some(ImageCapability {
            supports,
            source: ImageCapSource::Override,
        });
    }
    upstream
        .map(|supports| ImageCapability {
            supports,
            source: ImageCapSource::Upstream,
        })
        // ★ issue #142：上游**不声明**时用内置表兜底（只放已确认能吃图的模型）。
        .or_else(|| known_multimodal_capability(model_id))
}

/// 写入一条覆盖并**落盘**（`supports = None` ⇒ 删键 = 跟随上游）；返回是否发生了变化。
///
/// ⚠️ 这是**阻塞**调用（含磁盘写）：命令面必须放进 `spawn_blocking`（本仓既有纪律：
/// 同步 IO 不得压在 IPC / 事件循环线程上）。持锁只到内存改完为止，落盘在锁外。
pub fn set_and_save(scope: Scope, model_id: &str, supports: Option<bool>) -> Result<bool, String> {
    let snapshot = {
        let mut guard = shared()
            .write()
            .map_err(|_| "图片能力存储已损坏（锁中毒）".to_string())?;
        let changed = guard.set(scope, model_id, supports);
        if !changed {
            return Ok(false);
        }
        guard.clone()
    };
    snapshot.save()?;
    Ok(true)
}

/// **唯一解析点**：上游声明 > 用户覆盖；两边都没有 ⇒ `None`（不下发字段）。
///
/// `upstream`：WB 目录给的 `supports_images`；Trae / Qoder 传 `None`。
pub fn resolve(
    store: &ImageCaps,
    scope: Scope,
    model_id: &str,
    upstream: Option<bool>,
) -> Option<ImageCapability> {
    if let Some(supports) = store.lookup(scope, model_id) {
        return Some(ImageCapability {
            supports,
            source: ImageCapSource::Override,
        });
    }
    upstream
        .map(|supports| ImageCapability {
            supports,
            source: ImageCapSource::Upstream,
        })
        .or_else(|| known_multimodal_capability(model_id))
}

/// 内置表里**已确认能吃图**的模型（上游不声明时的兜底）。
///
/// ## 为什么必须有（issue #142）
///
/// Trae / Qoder 的模型清单**没有能力字段** ⇒ 调用方传 `upstream = None` ⇒ 按三态规则
/// 「不下发」⇒ 客户端据此隐藏图片上传入口。于是 `deepseek-v4.1-flash` 明明能吃图，
/// 用户在 Trae / Qoder 里却连上传按钮都看不到。
///
/// 这跟「上游说不支持」是两件事：我们只是**没有信息**。对**确认过**的模型给一个静态声明，
/// 比让客户端一律回落到「不支持」更接近事实。
///
/// ## 纪律（照 `docs/specs/DECISIONS.md` 的「不编事实」）
///
/// - **只放确认过的模型**；不确定就继续 `None` —— 宁可少声明，不可编能力。
/// - 只声明 `true`：内置表**永远不说 false**（说 false 会把用户的上传入口关掉，
///   而「没实测到」不等于「不支持」）。
/// - 用户覆盖仍然优先（`resolve` 的第一支），所以表写错了用户能自己改回来。
fn known_multimodal_capability(model_id: &str) -> Option<ImageCapability> {
    if !is_known_multimodal(model_id) {
        return None;
    }
    Some(ImageCapability {
        supports: true,
        source: ImageCapSource::Known,
    })
}

/// 模型 id 是否在内置的多模态表里。
///
/// 归一化：去掉 `provider/` 前缀、转小写后**子串**匹配 —— 同一模型在三条产品线上的
/// id 带不同前缀/后缀（`deepseek-v4.1-flash` / `deepseek/deepseek-v4.1-flash` /
/// `deepseek-v4.1-flash-thinking`），逐字比对必然漏。
fn is_known_multimodal(model_id: &str) -> bool {
    const KNOWN: &[&str] = &["deepseek-v4.1-flash"];
    let lowered = model_id.trim().to_ascii_lowercase();
    let bare = lowered.rsplit('/').next().unwrap_or(&lowered);
    KNOWN.iter().any(|known| bare.contains(known))
}

/// 把能力写进一条模型条目（**三个出口 + 两个管理面视图的唯一写入点**）。
///
/// - `Some(cap)` ⇒ `supports_images` + `image_input_source` + OpenRouter 兼容的 `architecture`；
/// - `None` ⇒ 把可能已存在的 `supports_images` **删掉**（上游没声明就不许有残留断言）。
///
/// 返回解析结果，调用方一般不用，测试要用。
pub fn apply(
    object: &mut Map<String, Value>,
    scope: Scope,
    model_id: &str,
    upstream: Option<bool>,
) -> Option<ImageCapability> {
    let capability = resolve_effective(scope, model_id, upstream);
    write_capability(object, capability)
}

/// [`apply`] 的纯函数内核（不读全局存储，便于单测注入存储）。
pub fn write_capability(
    object: &mut Map<String, Value>,
    capability: Option<ImageCapability>,
) -> Option<ImageCapability> {
    match capability {
        Some(cap) => {
            object.insert("supports_images".into(), Value::Bool(cap.supports));
            object.insert(
                "image_input_source".into(),
                Value::String(cap.source.as_str().to_string()),
            );
            object.insert("architecture".into(), architecture(cap.supports));
        }
        None => {
            object.remove("supports_images");
            object.insert("image_input_source".into(), Value::Null);
            object.remove("architecture");
        }
    }
    capability
}

/// OpenRouter 形状的能力块（`/api/v1/models` 的 `architecture`）。
///
/// 只声明输入/输出模态：`modality` 是同一事实的字符串写法，很多客户端读它。
fn architecture(supports_images: bool) -> Value {
    let inputs = if supports_images {
        serde_json::json!(["text", "image"])
    } else {
        serde_json::json!(["text"])
    };
    serde_json::json!({
        "modality": if supports_images { "text+image->text" } else { "text->text" },
        "input_modalities": inputs,
        "output_modalities": ["text"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(scope: Scope, id: &str, value: bool) -> ImageCaps {
        let mut store = ImageCaps::default();
        store.set(scope, id, Some(value));
        store
    }

    /// ★ issue #142：上游**不声明**（Trae / Qoder 的常态）时，内置表要给出 `true`。
    ///
    /// 可证伪：把 `resolve` 的 `.or_else(|| known_multimodal_capability(…))` 删掉
    /// ⇒ 第一条断言红（Trae / Qoder 的 `deepseek-v4.1-flash` 又变回「未声明」，
    /// 客户端隐藏上传入口）。
    #[test]
    fn known_multimodal_fills_the_gap_when_upstream_is_silent() {
        let store = ImageCaps::default();
        let known = resolve(&store, Scope::WbCn, "deepseek-v4.1-flash", None)
            .expect("内置表必须认这个模型");
        assert!(known.supports);
        assert_eq!(
            known.source,
            ImageCapSource::Known,
            "来源必须是「内置表」，不能冒充上游声明"
        );
        assert_eq!(ImageCapSource::Known.as_str(), "known");

        // 三条产品线的 id 写法不同：带前缀 / 带后缀都要认。
        assert!(is_known_multimodal("deepseek/deepseek-v4.1-flash"));
        assert!(is_known_multimodal("deepseek-v4.1-flash-thinking"));
        assert!(!is_known_multimodal("deepseek-v4.1"));

        // 上游**明确**说不支持 ⇒ 以上游为准（内置表不覆盖上游）。
        let upstream_no =
            resolve(&store, Scope::WbCn, "deepseek-v4.1-flash", Some(false)).expect("上游声明优先");
        assert!(!upstream_no.supports);
        assert_eq!(upstream_no.source, ImageCapSource::Upstream);

        // 表外模型 ⇒ 仍然 `None`（不许凭空断言）。
        assert!(resolve(&store, Scope::WbCn, "some-text-only-model", None).is_none());
    }

    /// ★ 三态：上游没说 ≠ 不支持 —— `None` 必须**不下发** `supports_images`。
    ///
    /// 变异：把 `None` 分支改成写 `false` ⇒ 第二条断言红（Trae/Qoder 会凭空多出
    /// 「不支持图片」的断言）。
    #[test]
    fn undeclared_emits_no_field() {
        let mut object = Map::new();
        object.insert("id".into(), serde_json::json!("m"));
        // 上游没声明（Trae / Qoder 的常态）
        assert_eq!(write_capability(&mut object, None), None);
        assert!(
            object.get("supports_images").is_none(),
            "未声明不得下发能力断言"
        );
        assert!(object.get("architecture").is_none());
        assert_eq!(object["image_input_source"], Value::Null);
    }

    /// 上游声明优先；用户覆盖压过上游；覆盖清掉后回落上游。
    #[test]
    fn override_beats_upstream_and_can_be_cleared() {
        let mut store = ImageCaps::default();
        let mut object = Map::new();
        let apply = |store: &ImageCaps, object: &mut Map<String, Value>| {
            write_capability(
                object,
                resolve(store, Scope::WbCn, "deepseek-v4.1", Some(true)),
            )
        };

        // ① 上游声明 true，没有覆盖 ⇒ upstream
        let cap = apply(&store, &mut object).unwrap();
        assert!(cap.supports);
        assert_eq!(cap.source, ImageCapSource::Upstream);
        assert_eq!(object["supports_images"], Value::Bool(true));
        assert_eq!(object["image_input_source"], "upstream");
        assert_eq!(
            object["architecture"]["input_modalities"],
            serde_json::json!(["text", "image"])
        );

        // ② 用户改成「不支持」⇒ 覆盖压过上游
        store.set(Scope::WbCn, "deepseek-v4.1", Some(false));
        let cap = apply(&store, &mut object).unwrap();
        assert!(!cap.supports);
        assert_eq!(cap.source, ImageCapSource::Override);
        assert_eq!(object["supports_images"], Value::Bool(false));
        assert_eq!(
            object["architecture"]["input_modalities"],
            serde_json::json!(["text"])
        );

        // ③ 还原上游（删键）⇒ 回到 upstream 声明
        assert!(store.set(Scope::WbCn, "deepseek-v4.1", None));
        let cap = apply(&store, &mut object).unwrap();
        assert_eq!(cap.source, ImageCapSource::Upstream);
        assert!(cap.supports);
    }

    /// 上游没声明时，用户覆盖就是唯一真值（Trae / Qoder 的全部可用路径）。
    #[test]
    fn override_works_without_upstream() {
        let store = store_with(Scope::QoderCn, "Qwen3.8-Max", true);
        let cap = resolve(&store, Scope::QoderCn, "Qwen3.8-Max", None).unwrap();
        assert!(cap.supports);
        assert_eq!(cap.source, ImageCapSource::Override);
    }

    /// 作用域隔离：`wb-cn` 的覆盖不得影响 `trae-cn` / `qoder-cn`。
    #[test]
    fn scopes_are_independent() {
        let store = store_with(Scope::WbCn, "m", true);
        assert!(resolve(&store, Scope::WbCn, "m", None).is_some());
        assert!(resolve(&store, Scope::TraeCn, "m", None).is_none());
        assert!(resolve(&store, Scope::QoderCn, "m", None).is_none());
    }

    /// 大小写不敏感：覆盖写 `glm-5.3`，查询 `GLM-5.3` 必须命中；且**不留下重复键**。
    #[test]
    fn lookup_is_case_insensitive_and_dedupes() {
        let mut store = ImageCaps::default();
        store.set(Scope::TraeCn, "glm-5.3", Some(true));
        assert_eq!(
            resolve(&store, Scope::TraeCn, "GLM-5.3", None).map(|cap| cap.supports),
            Some(true)
        );
        // 换成另一种写法 ⇒ 旧键被清掉（否则「还原上游」会残留反方向覆盖）
        store.set(Scope::TraeCn, "GLM-5.3", Some(false));
        assert_eq!(store.get(Scope::TraeCn).len(), 1);
        assert_eq!(
            resolve(&store, Scope::TraeCn, "glm-5.3", None).map(|cap| cap.supports),
            Some(false)
        );
    }

    /// `set` 幂等：重复写同值 / 重复清空都返回 `false`（命令面据此跳过落盘）。
    #[test]
    fn set_is_idempotent() {
        let mut store = ImageCaps::default();
        assert!(store.set(Scope::WbCn, "m", Some(true)));
        assert!(!store.set(Scope::WbCn, "m", Some(true)));
        assert!(store.set(Scope::WbCn, "m", None));
        assert!(!store.set(Scope::WbCn, "m", None));
        // 空白 id 一律拒绝（不写脏数据）
        assert!(!store.set(Scope::WbCn, "  ", Some(true)));
    }
}
