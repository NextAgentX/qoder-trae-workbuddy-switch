//! Qoder 上游协议客户端 —— **两个通道**：
//!
//! | 通道 | 端点族 | 认证 |
//! |:--|:--|:--|
//! | **COSY 推理通道** | `/algo/api/v2/service/pro/sse/agent_chat_generation`（SSE）、`/algo/api/v2/model/list` | COSY 签名头（见 [`cosy`]） |
//! | **openapi 账号通道** | `/api/v1/deviceToken/refresh`、`/api/v1/jobToken/*`、`/sash/api/v1/me/daily-check-in/*`、`/api/v2/quota/usage`、`/api/v2/user/plan` | `Authorization: Bearer <access_token>` |
//!
//! ## 协议要点（都是实测形态，别凭直觉改）
//!
//! - **请求体过 [`cosy::qoder_encode`] 编码**后发送；**签名与 body 必须一致**
//!   （模型清单这种 GET 也要**带 body** `qoder_encode("{}")`，裸 GET 会 403）。
//! - 聊天响应是**信封 SSE**：`data:{"body":"<内层 OpenAI chunk>","statusCodeValue":200}`，
//!   结束帧 `data:{"body":"[DONE]"}`，`event:finish*` 是计时元数据（忽略）。
//!   解包见 [`unwrap_envelope_line`]，噪声字段清洗见 [`clean_chunk`]。
//! - 请求体以官方 baseprompt 为骨架（[`base_template`]），每次覆写 id / 模型配置 /
//!   会话 / 工具 / 参数 / business。
//! - 刷新通道是**普通 JSON POST**（不签名、不带 Bearer）；`expires_in` 单位是**毫秒**。
//! - 签到通道在 `/sash` 而非 `/api`；**国际版没有该接口**（404/405/410）——
//!   那要如实报「本区域未开放」，不是静默失败。
//!
//! ## 移植口径：**刻意未移植**的机制（每条都写明理由，别当遗漏）
//!
//! 1. **出站指纹清洗**（把客户端自称 "Claude Code"/"Codex CLI" 的提示词改写成中性文本）：
//!    防御性脱敏，涉及多张正则表；未移植期间客户端自带的 system 会**原样上行**。
//! 2. **历史工具结果的暴露面削减**（长历史首尾保留 + 中间省略）：缓解措施而非协议刚需。
//! 3. **每模型思考档位支持表**（把不支持的档位映射到「最近的合法档位」）：
//!    需要一手实测每款模型的档位集合；当前**只透传官方词表内的取值**（见 [`canonical_effort`]），
//!    其余丢弃（上游对不认识的档位是「忽略」而非报错，丢弃不会形成错误请求）。
//! 4. **`validate_public_http_url` 类 SSRF 守卫**：本模块的 URL 全部由区域常量拼出，
//!    没有任何用户输入进 URL 的路径。
//!
//! ## 已移植的回声治理（issue #46 / #66，别当同一条）
//!
//! | 形态 | 处理 | 入口 |
//! |:--|:--|:--|
//! | `marker + 合法 JSON 数组` | 还原成结构化 `tool_calls` | [`parse_leaked_tool_calls`] |
//! | `marker + 数组 + DSML 闭标签残片` | 同上（**收尾容错**） | [`parse_leaked_tool_calls_with_trailing`] |
//! | `marker + 截断/损坏的数组` | **吞掉正文**（绝不把内部标记交给用户） | [`leaked_partial_droppable`] |
//! | `[工具结果…]` 回声块 | **吞掉正文**（不还原：文本化历史没有 `tool_call_id` 可配对） | [`tool_echo_droppable`] |
//! | `<｜｜DSML｜｜ calls>` 调用块 | 还原成结构化 `tool_calls`，标签从正文剥掉 | [`parse_dsml_tool_calls`] |
//!
//! 后两类回读的「散文切分」在流式层（`gateway::qoder::sse`）做 —— 单帧里散文与标记
//! 可能同框，所以切分必须在**帧**这一层。
//!
//! ## 真机探针
//!
//! 本机无 Qoder 账号 ⇒ 各端点带 `#[ignore]` 探针（设 `QODER_LIVE_ACCESS_TOKEN` /
//! `QODER_LIVE_UID` 后可跑）。**没有真机证据前，不要声称某端点已验证**。

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::cosy::{self, CosyAccountInput};
use super::region::{self, QoderRegion};

/// 工具结果在**文本化历史**里的前缀（逐字节对齐；削减/回读都锚在它上面）。
pub const TOOL_RESULT_MARKER: &str = "[工具结果";
/// 模型自造的「工具结果结束」闭标记（网关只写开标记；这个只用于判回声是否自洽）。
pub const TOOL_RESULT_CLOSE: &str = "[工具结果结束]";
/// 助手工具调用的文本化标记（见 [`flatten_messages`] 的文本模式）。
/// 历史工具结果削减：单侧保留字符数的默认值（与参考实现一致）。
pub const TOOL_RESULT_KEEP_DEFAULT: usize = 2000;
/// 削完至少要省这么多字符才值得加省略标记（否则标记本身比省下的还长）。
const TOOL_RESULT_SHRINK_MARGIN: usize = 200;
/// 默认保护最后 1 条工具结果（当前轮那条）。
const TOOL_RESULT_KEEP_TAIL_DEFAULT: usize = 1;
/// 历史工具结果被省略时的标记（**固定串**：带字数会打断上游前缀缓存）。
const TOOL_RESULT_OMITTED_MARKER: &str =
    "\n[[buddy-switch: 已省略中间内容；需要完整内容请重新执行该命令]]\n";

pub const LEAK_MARKER: &str = "[assistant 请求调用工具]";
/// 同一个标记的**尖括号写法**：模型复述时会把 `[ ]` 改写成 `< >`（真机见过）。
/// 两种都必须认，否则「换个括号」就绕过整条回声治理。
pub const LEAK_MARKER_ALT: &str = "<assistant 请求调用工具>";
/// 上游给 delta 填充的空噪声字段（值为空时剥掉，避免客户端读到空壳字段）。
const NOISE_KEYS: [&str; 3] = ["extra_fields", "refusal", "reasoning_content"];
/// 结构化工具有效历史的作用域白名单（只在 CN 出口、且模型 key 前缀命中时启用）。
///
/// ⚠️ 匹配的是**模型 key**，不是 `display_name`。官方清单里 Qwen 族的 key 形如
/// `qfmodel` / `qmodel*` / `q37fmodel`（`Qwen3.8-Flash` 只是展示名），GLM 族形如
/// `gmodel` / `gfmodel` / `gm51model`（`GLM-5.3` 同理）。
///
/// ★ 2026-10-18（issue #46 的 B 项）：旧名单只有 `qwen` / `glm` / `gm4` / `gm5`
/// ⇒ **一个真实 key 都不命中** ⇒ CN 全量模型退化成文本模式，历史里的工具调用被序列化
/// 成标记文本塞进正文，模型回声后直接泄漏到用户界面（即 A 项的诱因）。
/// 刻意**不纳入** Kimi / MiniMax 族（本仓无 CN 清单依据，见
/// [`DEEPSEEK_MODEL_KEYS`] 的对照说明）；DeepSeek 族由 [`is_deepseek_key`] 单独判定。
const STRUCTURED_TOOL_HISTORY_WHITELIST: [&str; 9] = [
    "qmodel",  // qmodel / qmodel_38max …
    "qfmodel", // qfmodel / qfmodel_…
    "q37",     // q37fmodel
    "qwen",    // 兼容以 `qwen` 开头的 key（展示名形态）
    "gmodel",  // gmodel / gmodel_…
    "gfmodel", // gfmodel
    "gm5",     // gm51model（已被此前缀覆盖）
    "gm4",     // gm4model
    "glm",     // 兼容以 `glm` 开头的 key
];
/// DeepSeek 族的模型 key（CN 出口）。
///
/// 两处用途，**判定必须同源**（都走 [`is_deepseek_key`]）：
///
/// 1. **历史多轮一致性**：`reasoning_content` 要随历史回传（`build_chat_body` 的
///    `keep_reasoning`）；
/// 2. **结构化工具历史**：★ 2026-10-21（issue #72）真机取证 —— DeepSeek Flash 在**文本
///    模式**下会把网关序列化的伪格式（`[assistant 请求调用工具]` + 紧凑 JSON、`[工具结果
///    (name)]`）**照抄进正文**，再混上客户端 system 教的 `<｜｜DSML｜｜ calls>` 语法 ⇒
///    客户端看到的「上下文乱码 / 特殊字符」正是这坨内部标记，且工具一次都没执行。
///    报障者用 `QD_STRUCTURED_TOOL_HISTORY=on` 强制结构化后症状消失 ⇒ 上游**接受**
///    结构化历史。旧代码把 `dmodel` / `dfmodel` 排除在白名单外（且用反向断言钉死），
///    前提（「未验证，宁可不启用」）被真机推翻，故改为**默认启用**。
///
/// ⚠️ 前缀匹配而非整段相等：容忍 `_suffix` 变体（与白名单的族前缀同一口径），
/// 免得「结构化开了、reasoning 没回传」这种半开状态把上游打成 400。
///
/// ★ `deepseek` 这一项来自**参考实现**（`reference/qoder2api-hub-main/qoder_proxy.py`
/// 的 `STRUCTURED_TOOL_HISTORY_WHITELIST`）：它的白名单是
/// `("qwen","glm","gm4","gm5","deepseek","dfmodel")`，注释写明
/// 「DeepSeek 的**上游 key 是 `dfmodel`**（显示名 DeepSeek-V4-Flash），只写 "deepseek"
/// 匹配不到 ⇒ auto 一直是文本化 ⇒ 模型照抄伪协议」—— 与 issue #72 是同一条病。
/// 本仓保留 `deepseek` 这一项是为了覆盖「key 就叫 deepseek*」的其它形态；
/// 展示名匹配仍**刻意不做**（本仓一律只认 key：Qwen 的 key 是 `q37fmodel`、
/// 展示名是 `Qwen3.8-Flash`，混用两套判据迟早误伤）。
const DEEPSEEK_MODEL_KEYS: [&str; 3] = ["dmodel", "dfmodel", "deepseek"];

/// key 是否属于 DeepSeek 族（**唯一**判定入口）。
fn is_deepseek_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    !key.is_empty()
        && DEEPSEEK_MODEL_KEYS
            .iter()
            .any(|prefix| key.starts_with(prefix))
}
/// 模型清单的**签名路径**（不含 query）。
///
/// ⚠️ 签名只覆盖 `urlparse(raw_url).path` —— **query 不参与签名**。拿带
/// `?Encode=1` 的整串去签，服务端按无 query 的 path 复算 ⇒ md5 不等 ⇒ 403。
pub const PATH_MODELS_SIGN: &str = "/algo/api/v2/model/list";
/// 模型清单**请求路径**（带 `Encode=1`；GET 也**必须带签名 body**）。
pub const PATH_MODELS_ENCODED: &str = "/algo/api/v2/model/list?Encode=1";

// ---------------------------------------------------------------------------
// 请求骨架（官方 baseprompt）
// ---------------------------------------------------------------------------

/// 官方请求骨架（`baseprompt.json`）。
///
/// ⚠️ 与官方原件的**两处精简**（都有依据，别还原）：
/// - `tools` 置空：模板自带的 14 个桌面端工具（Bash/Edit/…）**永不随请求发出**
///   （每请求都被客户端工具或 `[]` 覆盖；泄漏给普通客户端是明确要避免的形态）。
/// - `messages` 只留 system：两条空 user 轮次在任何路径上都不存活。
const BASE_TEMPLATE_JSON: &str = include_str!("baseprompt.json");

/// 解析一次、进程内复用的骨架。
pub fn base_template() -> Result<&'static Value, String> {
    static TEMPLATE: OnceLock<Result<Value, String>> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| {
            serde_json::from_str::<Value>(BASE_TEMPLATE_JSON)
                .map_err(|error| format!("解析 Qoder 请求骨架失败: {error}"))
        })
        .as_ref()
        .map_err(|error| error.clone())
}

/// 模型元数据（`model_config` 的取值来源；来自模型清单条目）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelMeta {
    pub key: String,
    pub display_name: String,
    pub is_vl: bool,
    pub is_reasoning: bool,
    pub max_input_tokens: i64,
}

impl ModelMeta {
    /// 从模型清单条目（[`parse_model_list`] 的行）取元数据；缺字段用官方默认值兜底。
    pub fn from_catalog_entry(entry: &Value) -> Self {
        let key = entry
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let display_name = entry
            .get("display_name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&key)
            .to_string();
        Self {
            key,
            display_name,
            is_vl: entry.get("is_vl").and_then(Value::as_bool).unwrap_or(false),
            is_reasoning: entry
                .get("is_reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            max_input_tokens: entry
                .get("max_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(180_000),
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 消息处理
// ---------------------------------------------------------------------------

/// OpenAI 的 `developer` 角色 → `system`（上游只认识 system/user/assistant）。
pub fn normalize_roles(messages: Option<&Vec<Value>>) -> Vec<Value> {
    let mut out = Vec::new();
    for message in messages.into_iter().flatten() {
        let Some(object) = message.as_object() else {
            continue;
        };
        let mut item = object.clone();
        if object.get("role").and_then(Value::as_str) == Some("developer") {
            item.insert("role".to_string(), Value::String("system".to_string()));
        }
        out.push(Value::Object(item));
    }
    out
}

/// content 拆解结果。
pub struct FlattenedContent {
    /// 纯文本（多段文本以 `\n` 连接；与旧行为一致）。
    pub text: String,
    /// 图片 URL（`data:` URL 原样保留）。
    pub images: Vec<String>,
    /// **有图时**才保留的多模态 parts（`[{type:"text"},{type:"image_url"}]`）；
    /// 无图时为 `None`（此时上游收纯文本字符串即可）。
    ///
    /// ★ 为何必须保留：上游**只读** message content 里的结构化 `image_url` parts，
    /// **不读**顶层 `image_urls` / `chat_context.imageUrls`（实测：填了顶层字段
    /// 模型仍回「我没有收到图片」；改成 parts 后才正确识别图像内容）。
    /// 所以带图消息的 content **不能**压成字符串。
    pub parts: Option<Vec<Value>>,
}

impl FlattenedContent {
    fn empty() -> Self {
        Self {
            text: String::new(),
            images: Vec::new(),
            parts: None,
        }
    }
}

/// 把 OpenAI content（字符串 / parts 列表 / 单个 part 对象）拆成
/// 纯文本 + 图片 + **保留形态的 parts**。
///
/// ★ 单 part 形态（`content` 直接给一个 part 对象而非列表）按**单元素列表**处理，
/// 绝不 `str()` —— 否则图像被字面量文本化（模型收到一段
/// `{"type":"image_url",...}` 字符串），图像从未真正到达模型。
pub fn flatten_content(content: Option<&Value>) -> FlattenedContent {
    match content {
        None | Some(Value::Null) => FlattenedContent::empty(),
        Some(Value::String(text)) => FlattenedContent {
            text: text.clone(),
            images: Vec::new(),
            parts: None,
        },
        Some(Value::Array(parts)) => flatten_parts(parts),
        Some(value @ Value::Object(_)) => flatten_parts(std::slice::from_ref(value)),
        Some(other) => FlattenedContent {
            text: other.to_string(),
            images: Vec::new(),
            parts: None,
        },
    }
}

/// 逐 part 拆解：文本进 `texts`，图片进 `images`；**有图时**同时保留 parts。
fn flatten_parts(parts: &[Value]) -> FlattenedContent {
    let mut texts: Vec<String> = Vec::new();
    let mut images: Vec<String> = Vec::new();
    let mut structured: Vec<Value> = Vec::new();
    for piece in parts {
        match piece {
            Value::String(text) => {
                texts.push(text.clone());
                structured.push(serde_json::json!({ "type": "text", "text": text }));
            }
            Value::Object(object) => {
                let piece_type = object.get("type").and_then(Value::as_str).unwrap_or("");
                if matches!(
                    piece_type,
                    "text" | "input_text" | "output_text" | "summary_text"
                ) {
                    if let Some(text) = object.get("text").and_then(Value::as_str) {
                        texts.push(text.to_string());
                        structured.push(serde_json::json!({ "type": "text", "text": text }));
                    }
                } else if matches!(piece_type, "image_url" | "image" | "input_image")
                    || object.contains_key("image_url")
                {
                    if let Some(url) = image_url_of(object) {
                        images.push(url.clone());
                        // ★ `detail` 必须跟着走：它是 OpenAI 的图片精度档位
                        //   （`low` / `high` / `auto`），丢了对高精度场景就是**掉画质**
                        //   ——分辨率与 token 消耗都由它决定。归一化只该改**形态**，
                        //   不该顺手摘掉语义字段。
                        let mut image = serde_json::json!({ "url": url });
                        if let Some(detail) = image_detail_of(object) {
                            image["detail"] = Value::String(detail);
                        }
                        structured
                            .push(serde_json::json!({ "type": "image_url", "image_url": image }));
                    }
                }
            }
            _ => {}
        }
    }
    let text = texts
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    // 有图才保留 parts（无图时上游收纯文本，保留数组只是噪音）。
    let parts = if images.is_empty() {
        None
    } else {
        Some(structured)
    };
    FlattenedContent {
        text,
        images,
        parts,
    }
}

/// 从一个图片 part 里取出 URL（兼容 `image_url` 为字符串 / 对象两种写法、
/// `data` + `mimeType` 的裸 base64 形态，以及 Anthropic 风格的 `source`）。
fn image_url_of(object: &serde_json::Map<String, Value>) -> Option<String> {
    let mut url = object
        .get("image_url")
        .or_else(|| object.get("url"))
        .and_then(|value| {
            if let Some(inner) = value.get("url") {
                inner.as_str()
            } else {
                value.as_str()
            }
        })
        .map(str::to_string);
    if url.is_none() {
        if let Some(data) = object.get("data").and_then(Value::as_str) {
            let mime = object
                .get("mimeType")
                .or_else(|| object.get("mime_type"))
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            url = Some(format!("data:{mime};base64,{data}"));
        }
    }
    // Anthropic 风格：`{"type":"image","source":{"type":"base64","media_type":…,"data":…}}`
    // 或 `{"type":"image","source":{"type":"url","url":…}}`。
    if url.is_none() {
        if let Some(source) = object.get("source") {
            if let Some(link) = source.get("url").and_then(Value::as_str) {
                url = Some(link.to_string());
            } else if let Some(data) = source.get("data").and_then(Value::as_str) {
                let mime = source
                    .get("media_type")
                    .or_else(|| source.get("mediaType"))
                    .and_then(Value::as_str)
                    .unwrap_or("image/png");
                url = Some(format!("data:{mime};base64,{data}"));
            }
        }
    }
    url
}

/// 取图片精度档位（OpenAI `image_url.detail`）。
///
/// 两种位置都认：嵌在 `image_url` 对象里（chat 形态），或与 `image_url` 平级
/// （Responses 的 `input_image` 把 `detail` 放在 part 顶层）。Anthropic 的
/// `source` 形态没有这个字段 ⇒ `None`（别给它编一个默认值）。
fn image_detail_of(object: &serde_json::Map<String, Value>) -> Option<String> {
    object
        .get("image_url")
        .and_then(|value| value.get("detail"))
        .and_then(Value::as_str)
        .or_else(|| object.get("detail").and_then(Value::as_str))
        .map(str::to_string)
        .filter(|detail| !detail.is_empty())
}

/// 把 OpenAI content（字符串或 parts 列表）压成纯文本；图片单独收集。
///
/// ★ 图片**同时**保留在 [`flatten_content`] 的 `parts` 里 —— 上游只读
/// content 里的结构化 parts，见 [`FlattenedContent::parts`]。
pub fn flatten_content_text(content: Option<&Value>) -> (String, Vec<String>) {
    let flat = flatten_content(content);
    (flat.text, flat.images)
}

/// 扁平化结果：`(首个 system 文本, 历史消息, 图片 URL 列表)`。
pub struct FlattenedMessages {
    pub system_text: Option<String>,
    pub messages: Vec<Value>,
    pub images: Vec<String>,
}

/// 把客户端会话压平成上游可接受的 `{role, content}` 序列。
///
/// - 首个 system/developer 抽为 `system_text`（模板 system 的替换源）；
/// - `tool` 结果：`structured=true` 保留 `{role:"tool", tool_call_id, content}`
///   （**content 用空字符串而非 null**——null 会让上游丢掉配对的 assistant 消息）；
///   否则降级为 user 文本（`[工具结果 (name)]\n` 前缀）。
/// - assistant 的 `tool_calls`：structured 原样透传（arguments 统一成 JSON 字符串）；
///   否则序列化为 `<LEAK_MARKER>\n<JSON 数组>` 塞进 content。
/// - assistant 的 `reasoning_content` 仅在 `keep_reasoning=true`（DeepSeek 族）时保留。
///   历史工具结果**单侧**保留字符数（0 = 关闭削减）。
///
/// 为什么要它：长历史里的工具输出（构建日志 / 文件内容 / 测试回显）占掉的 token
/// 常常比正文还多；而且**文本化历史里的工具结果是回声的主要来源**（模型照着复述）。
/// 参考实现（qoder2api-hub `_shrink_tool_result`）默认单侧 2000 字符。
///
/// 环境变量 `BUDDY_SWITCH_QODER_TOOL_KEEP`：
/// - 正整数 ⇒ 单侧保留字符数；
/// - `0` / `off` / `none` / `false` / `no` / `disable` ⇒ **关闭**（回退全文回灌）；
/// - 未设置或非法 ⇒ 默认 [`TOOL_RESULT_KEEP_DEFAULT`]。
pub fn parse_tool_keep(raw: Option<&str>) -> usize {
    match raw.map(|text| text.trim().to_ascii_lowercase()) {
        None => TOOL_RESULT_KEEP_DEFAULT,
        Some(text) if text.is_empty() => TOOL_RESULT_KEEP_DEFAULT,
        Some(text) => {
            if matches!(
                text.as_str(),
                "0" | "off" | "none" | "false" | "no" | "disable" | "disabled"
            ) {
                return 0;
            }
            text.parse::<usize>().unwrap_or(TOOL_RESULT_KEEP_DEFAULT)
        }
    }
}

/// 实际生效的单侧保留字符数（每次调用都读环境变量 ⇒ 运行期可调）。
fn tool_result_keep() -> usize {
    parse_tool_keep(
        std::env::var("BUDDY_SWITCH_QODER_TOOL_KEEP")
            .ok()
            .as_deref(),
    )
}

/// 保护**最后 N 条**工具结果不削减（默认 1 = 当前轮那条）。
///
/// ★ 为什么不用「当前轮」判据：那会随请求形态滑动 —— 用户再发一句话后，**所有**
/// 历史工具结果会从「全文」变成「省略版」，同一条消息在不同轮次序列化成不同字节，
/// 上游前缀缓存从第一条被削处整段失效（参考实现 issue #48 实测命中率 95%→8%）。
/// 「只保护最后 N 条」时，被改写的永远是**同一条**（滑出保护窗的那条），中断点只有一处。
fn tool_result_keep_tail() -> usize {
    std::env::var("BUDDY_SWITCH_QODER_TOOL_KEEP_TAIL")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(TOOL_RESULT_KEEP_TAIL_DEFAULT)
}

/// 历史工具结果：首尾各留 `keep` 字符，中间换成**固定串**省略标记。
///
/// - 本来就不长（`<= 2*keep + margin`）⇒ 原样；
/// - **完整 JSON** 给双倍预算（截断 JSON 会失去可解析性）；
/// - 切点尽量落在换行边界，避免切出半行；
/// - 标记是**固定串**（不带省略字数）：字数会让同一条历史在不同轮次产生不同字节，
///   打断上游前缀缓存。
pub fn shrink_tool_result(text: &str, keep: usize) -> String {
    if keep == 0 || text.len() <= keep * 2 + TOOL_RESULT_SHRINK_MARGIN {
        return text.to_string();
    }
    if is_complete_json(text) && text.len() <= keep * 4 {
        return text.to_string();
    }
    let head = &text[..floor_char_boundary(text, keep)];
    let head = match head.rfind('\n') {
        Some(at) if at > head.len() / 2 => &head[..at],
        _ => head,
    };
    let tail_start = floor_char_boundary(text, text.len().saturating_sub(keep));
    let tail = &text[tail_start..];
    let tail = match tail.find('\n') {
        Some(at) if at < tail.len() / 2 => &tail[at + 1..],
        _ => tail,
    };
    format!("{head}{TOOL_RESULT_OMITTED_MARKER}{tail}")
}

/// 往下取到最近的 UTF-8 字符边界（`text[..limit]` 不能切半个汉字）。
fn floor_char_boundary(text: &str, limit: usize) -> usize {
    if limit >= text.len() {
        return text.len();
    }
    let mut index = limit;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// `text` 是不是一个**完整可解析**的 JSON 对象 / 数组（决定给不给双倍预算）。
fn is_complete_json(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let first = trimmed.as_bytes()[0];
    if first != b'{' && first != b'[' {
        return false;
    }
    serde_json::from_str::<Value>(trimmed).is_ok()
}

pub fn flatten_messages(
    messages: &[Value],
    keep_reasoning: bool,
    structured: bool,
) -> FlattenedMessages {
    let mut system_text: Option<String> = None;
    let mut flat: Vec<Value> = Vec::new();
    let mut images: Vec<String> = Vec::new();

    /* 历史工具结果削减（见 [`shrink_tool_result`]）：**只削更早的**，最后
    `keep_tail` 条原样保留（当前轮那条模型正在用）。 */
    let keep = tool_result_keep();
    let keep_tail = tool_result_keep_tail();
    let tool_total = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .count();
    let protected_from = tool_total.saturating_sub(keep_tail);
    let mut tool_seen = 0usize;

    for message in messages {
        let Some(object) = message.as_object() else {
            continue;
        };
        let role = object.get("role").and_then(Value::as_str).unwrap_or("");
        let flat_content = flatten_content(object.get("content"));
        let text = flat_content.text.clone();
        images.extend(flat_content.images.iter().cloned());
        // ★ 带图消息的 content **保留 parts 形态**（上游只读 content 里的
        // 结构化 `image_url` parts）。无图时仍是纯文本字符串。
        let mut content_value = serde_json::Value::String(text.clone());
        if let Some(parts) = flat_content.parts.clone() {
            content_value = serde_json::Value::Array(parts);
        }

        if matches!(role, "system" | "developer") {
            if system_text.is_none() && !text.is_empty() {
                system_text = Some(text);
            }
            continue;
        }

        if role == "tool" {
            let name = object.get("name").and_then(Value::as_str).unwrap_or("");
            let body = if keep == 0 || tool_seen >= protected_from {
                text
            } else {
                shrink_tool_result(&text, keep)
            };
            tool_seen += 1;
            if structured {
                flat.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": object.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
                    "content": body,
                }));
            } else {
                let suffix = if name.is_empty() {
                    String::new()
                } else {
                    format!(" ({name})")
                };
                flat.push(serde_json::json!({
                    "role": "user",
                    "content": format!("{TOOL_RESULT_MARKER}{suffix}]\n{body}"),
                }));
            }
            continue;
        }

        if role == "assistant" {
            let tool_calls = object.get("tool_calls").and_then(Value::as_array);
            if let Some(calls) = tool_calls.filter(|calls| !calls.is_empty()) {
                if structured {
                    let mut normalized = Vec::new();
                    for call in calls {
                        let call = call.as_object().cloned().unwrap_or_default();
                        let function = call
                            .get("function")
                            .and_then(Value::as_object)
                            .cloned()
                            .unwrap_or_default();
                        let arguments = match function.get("arguments") {
                            Some(Value::String(text)) => text.clone(),
                            Some(other @ (Value::Object(_) | Value::Array(_))) => {
                                serde_json::to_string(other).unwrap_or_default()
                            }
                            None | Some(Value::Null) => String::new(),
                            Some(other) => other.to_string(),
                        };
                        normalized.push(serde_json::json!({
                            "id": call.get("id").and_then(Value::as_str).unwrap_or(""),
                            "type": call.get("type").and_then(Value::as_str).unwrap_or("function"),
                            "function": {
                                "name": function.get("name").and_then(Value::as_str).unwrap_or(""),
                                "arguments": arguments,
                            },
                        }));
                    }
                    let mut item = Map::new();
                    item.insert("role".into(), Value::String("assistant".into()));
                    item.insert("content".into(), Value::String(text));
                    item.insert("tool_calls".into(), Value::Array(normalized));
                    if keep_reasoning {
                        if let Some(reasoning) = object.get("reasoning_content") {
                            item.insert(
                                "reasoning_content".into(),
                                Value::String(reasoning.as_str().unwrap_or("").to_string()),
                            );
                        }
                    }
                    flat.push(Value::Object(item));
                    continue;
                }
                // 文本模式：marker + 紧凑 JSON 数组（逐字对齐）。
                let rendered: Vec<Value> = calls
                    .iter()
                    .filter_map(|call| {
                        let call = call.as_object()?;
                        let function = call.get("function").and_then(Value::as_object)?;
                        let arguments = match function.get("arguments") {
                            Some(Value::String(text)) => text.clone(),
                            Some(other @ (Value::Object(_) | Value::Array(_))) => {
                                serde_json::to_string(other).unwrap_or_default()
                            }
                            None | Some(Value::Null) => String::new(),
                            Some(other) => other.to_string(),
                        };
                        Some(serde_json::json!({
                            "name": function.get("name").and_then(Value::as_str).unwrap_or(""),
                            "arguments": arguments,
                        }))
                    })
                    .collect();
                let echo = serde_json::to_string(&rendered).unwrap_or_else(|_| "[]".to_string());
                let content = if text.is_empty() {
                    format!("{LEAK_MARKER}\n{echo}")
                } else {
                    format!("{text}\n{LEAK_MARKER}\n{echo}")
                };
                flat.push(serde_json::json!({ "role": "assistant", "content": content }));
                continue;
            }

            let mut item = Map::new();
            item.insert("role".into(), Value::String("assistant".into()));
            item.insert("content".into(), content_value);
            if keep_reasoning {
                if let Some(reasoning) = object.get("reasoning_content") {
                    item.insert(
                        "reasoning_content".into(),
                        Value::String(reasoning.as_str().unwrap_or("").to_string()),
                    );
                }
            }
            flat.push(Value::Object(item));
            continue;
        }

        // user 与其它角色原样保留（★ 带图时 content 是 parts 数组）。
        flat.push(serde_json::json!({ "role": role, "content": content_value }));
    }

    FlattenedMessages {
        system_text,
        messages: flat,
        images,
    }
}

/// 是否保留顶层 `image_urls` / `chat_context.imageUrls`（**默认否**）。
///
/// 上游只读 message content 里的结构化 `image_url` parts；顶层两份是冗余，
/// 且 base64 图片在两处各存一份会让出站体积翻倍（见 `build_chat_body` 注释）。
/// 排查时置 `BUDDY_SWITCH_QODER_KEEP_TOPLEVEL_IMAGES=1` 恢复旧行为。
fn keep_toplevel_images() -> bool {
    std::env::var("BUDDY_SWITCH_QODER_KEEP_TOPLEVEL_IMAGES")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// 从一个 message 的 `content` 里取**纯文本**（字符串直取；parts 数组取
/// `text` part 拼接）。带图消息的 content 是 parts 数组 ⇒ 不能用 `as_str()`。
pub fn content_text_of(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) == Some("text") {
                        part.get("text").and_then(Value::as_str).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(text)
        }
        _ => None,
    }
}

/// 出站工具统一**嵌套** `tools[].function` 对象（上游对平铺形态报 400）。
///
/// - 已嵌套（`{type, function:{…}}`）⇒ 规整内部字段；
/// - 平铺 `{type:"function", name, …}` ⇒ 包成嵌套；
/// - 其余形态（`custom` 等上游不认的）⇒ **丢弃**（不发畸形请求）。
pub fn normalize_tools(tools: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for tool in tools {
        let Some(object) = tool.as_object() else {
            continue;
        };
        let mut function = Map::new();
        if let Some(inner) = object.get("function").and_then(Value::as_object) {
            let name = inner.get("name").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            function.insert("name".into(), Value::String(name.to_string()));
            if let Some(description) = inner.get("description").and_then(Value::as_str) {
                function.insert("description".into(), Value::String(description.to_string()));
            }
            function.insert(
                "parameters".into(),
                inner
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
            );
        } else {
            let tool_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("function");
            let name = object.get("name").and_then(Value::as_str).unwrap_or("");
            if tool_type != "function" || name.is_empty() {
                continue;
            }
            function.insert("name".into(), Value::String(name.to_string()));
            if let Some(description) = object.get("description").and_then(Value::as_str) {
                function.insert("description".into(), Value::String(description.to_string()));
            }
            function.insert(
                "parameters".into(),
                object
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
            );
        }
        out.push(serde_json::json!({ "type": "function", "function": function }));
    }
    out
}

// ---------------------------------------------------------------------------
// 思考档位
// ---------------------------------------------------------------------------

/// 请求侧的**原始**思考档位（提取顺序：`reasoning_effort` → `reasoning.effort` →
/// `thinking.effort`/`thinking.level`；不做任何归一化）。
pub fn extract_requested_effort(payload: &Value) -> Option<String> {
    let object = payload.as_object()?;
    if let Some(value) = object.get("reasoning_effort").and_then(Value::as_str) {
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    if let Some(value) = object
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
    {
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    for key in ["effort", "level"] {
        if let Some(value) = object
            .get("thinking")
            .and_then(|thinking| thinking.get(key))
            .and_then(Value::as_str)
        {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// 官方档位词表内的取值**原样透传**；关闭类取值统一成 `"none"`；其余**丢弃**。
///
/// （每模型的合法档位集合是实测数据，未移植——见模块头「刻意未移植」第 4 条。）
pub fn canonical_effort(effort: &str) -> Option<&'static str> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" | "disable" | "false" | "0" | "no" => Some("none"),
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        _ => None,
    }
}

/// 每条 `tool` 有 `tool_call_id`、每个 `assistant.tool_calls` 条目有 `id` —— 结构化的前提。
///
/// 只查 id 非空，**不查配对**：孤儿（assistant 声明了调用、却没有 tool 消息回答）由
/// **出站清洗**处理（[`cleanup_orphan_tool_history`]，与 WB / Trae 侧同款），
/// 文本模式则天然宽容。在这里提前判配对属于「猜哪些请求会 400」，
/// 与降级机制的设计哲学冲突。
fn tool_ids_ok(messages: &[Value]) -> bool {
    for message in messages {
        let Some(object) = message.as_object() else {
            continue;
        };
        match object.get("role").and_then(Value::as_str) {
            Some("tool") => {
                let id = object
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if id.trim().is_empty() {
                    return false;
                }
            }
            Some("assistant") => {
                if let Some(calls) = object.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                        if id.trim().is_empty() {
                            return false;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    true
}

/// 清理**孤儿**工具调用（与 WB / Trae 侧同款语义，issue #72）。
///
/// ## 为什么 Qoder 侧也需要
///
/// WB / Trae 的出站整形里一直有这一步（`buddy-switch-gateway::outbound` 的
/// `cleanup_orphan_tool_calls`），Qoder 侧移植时**只搬了「id 是否齐备」的检查**，
/// 没搬清洗 —— 于是 Qoder 的结构化请求可能带着「assistant 声明了调用、却没有 tool
/// 消息回答」的畸形历史发出去（上游多半判 `tool_call_sequence_broken` → 400）。
/// 清洗比「退化成文本模式」好：它**保住**了配对完整的那部分结构化历史，而退化会把
/// 整段历史变成伪格式（模型照抄 ⇒ issue #72 的乱码）。
///
/// ## 规则（与 WB / Trae 侧**逐条一致**）
///
/// - 保留集 = `assistant.tool_calls[].id` ∩ `role=="tool"` 的 `tool_call_id`；
/// - `assistant` 消息只要**批内任一** `id` 不在保留集 → **整批**删除 `tool_calls`
///   （保留该消息的正文）；
/// - `role=="tool"` 且 `tool_call_id` 不在**存活批次**集合内 → **整条消息删除**。
///
/// ★ 「存活批次」而非「保留集」是与参考实现的**刻意分歧**（勿按「对齐参考实现」改回）：
/// 批次 `[A,B]` 只有 `A` 有结果时，保留集 = `{A}`，但该批次因 `B` 是孤儿而整批被删
/// ——此时若按保留集放行，`A` 的 `role:"tool"` 结果就会悬空，上游照样判协议错误。
/// 删除与放行必须以**同一套 id** 为准。
///
/// 无工具流量时不做任何改动。
pub fn cleanup_orphan_tool_history(messages: &mut Vec<Value>) {
    use std::collections::HashSet;

    let mut call_ids: HashSet<String> = HashSet::new();
    let mut result_ids: HashSet<String> = HashSet::new();
    for message in messages.iter() {
        let Some(wrapped) = message.as_object() else {
            continue;
        };
        if wrapped.get("role").and_then(Value::as_str) == Some("tool") {
            if let Some(id) = wrapped.get("tool_call_id").and_then(Value::as_str) {
                result_ids.insert(id.to_string());
            }
        }
        if let Some(calls) = wrapped.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    call_ids.insert(id.to_string());
                }
            }
        }
    }
    if call_ids.is_empty() && result_ids.is_empty() {
        return;
    }

    let keep: HashSet<String> = call_ids.intersection(&result_ids).cloned().collect();

    // 第一趟：裁决每个 assistant 批次，并记录真正存活下来的 tool_call id。
    let mut surviving: HashSet<String> = HashSet::new();
    for message in messages.iter_mut() {
        let Some(wrapped) = message.as_object_mut() else {
            continue;
        };
        let batch: Vec<String> = wrapped
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .map(|call| {
                        call.get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        if batch.is_empty() {
            continue;
        }
        if batch.iter().any(|id| !keep.contains(id)) {
            wrapped.remove("tool_calls");
        } else {
            surviving.extend(batch);
        }
    }

    // 第二趟：工具结果只认存活批次。
    messages.retain(|message| {
        let Some(wrapped) = message.as_object() else {
            return true;
        };
        if wrapped.get("role").and_then(Value::as_str) != Some("tool") {
            return true;
        }
        let id = wrapped
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        surviving.contains(id)
    });
}

/// 本次请求是否走结构化工具历史。
///
/// 与官方客户端的调用姿势对齐：**CN 出口 + 白名单模型前缀（含 DeepSeek 族）+ id 齐备**
/// 才启用；环境变量 `QD_STRUCTURED_TOOL_HISTORY=off/on` 可显式覆盖（`auto` 为默认）。
/// 任何情况下 id 不齐备都回退文本化（fail-safe：宁可少用，不发畸形请求）。
pub fn structured_tool_history_enabled(
    model_key: &str,
    region: QoderRegion,
    messages: &[Value],
) -> bool {
    if !tool_ids_ok(messages) {
        return false;
    }
    let mode = std::env::var("QD_STRUCTURED_TOOL_HISTORY")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "off" => return false,
        "1" | "on" | "true" | "yes" | "enable" | "enabled" | "force" => return true,
        _ => {}
    }
    if region != QoderRegion::Cn {
        return false;
    }
    /* ★ issue #72：DeepSeek 族默认启用（真机取证见 [`DEEPSEEK_MODEL_KEYS`]）。
    判据放在白名单**之前**且只认 CN 出口 —— `dmodel` / `dfmodel` 是 CN key，
    国际版（`smodel` / `cmodel`）不在本表，别顺手扩成「所有 key」。 */
    if is_deepseek_key(model_key) {
        return true;
    }
    let key = model_key.trim().to_ascii_lowercase();
    !key.is_empty()
        && STRUCTURED_TOOL_HISTORY_WHITELIST
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

// ---------------------------------------------------------------------------
// 请求体构造（骨架 + 覆写）
// ---------------------------------------------------------------------------

/// 本次请求**会**不会走结构化工具历史（网关侧判定用的公开入口）。
///
/// 与 [`build_chat_body`] 内部走**同一个**判据（[`structured_tool_history_enabled`]），
/// 不另写一套 —— 否则网关以为走了结构化、实际发的是文本，降级判定就会错位。
/// 网关拿到这个值后可以：① 传给 [`build_chat_body_with_structured`]；
/// ② 在结构化请求被 400 拒时决定要不要降级重试。
pub fn structured_tool_history_for(payload: &Value, meta: &ModelMeta, region: QoderRegion) -> bool {
    let messages = normalize_roles(payload.get("messages").and_then(Value::as_array));
    structured_tool_history_enabled(&meta.key, region, &messages)
}

/// 由客户端请求构造 `agent_chat_generation` 上游请求体（随后过 [`cosy::qoder_encode`]）。
///
/// `region` 参与「结构化工具历史」的 auto 判据（只在 CN 出口启用的白名单规则）。
/// 上游 `parameters.max_tokens` 的硬范围（实测：超限/零/负一律 400
/// `invalid_parameter_error`：`Range of max_tokens should be [1, 131072]`）。
///
/// 客户端侧常按模型清单的 `context_length`（最大档开关下是 1M）反推输出预算，
/// 会要出 100W+ 的值；网关**必须**在出站前钳制，否则整条请求被打回。
pub const MAX_TOKENS_LIMIT: i64 = 131_072;

pub fn build_chat_body(
    payload: &Value,
    meta: &ModelMeta,
    region: QoderRegion,
    user_type: &str,
) -> Result<Value, String> {
    let structured = structured_tool_history_for(payload, meta, region);
    build_chat_body_with_structured(payload, meta, user_type, structured)
}

/// 同上，但**显式指定**结构化 / 文本模式（自动降级用：重发时必须强制文本）。
///
/// 刻意**不收 `region`**：区域与环境变量的判据都在 [`structured_tool_history_for`] 里，
/// 这里只执行调用方的决定。`structured == true` 仍要过 [`tool_ids_ok`] —— id 齐备是硬前提，
/// 强制结构化也不能绕过它，否则「降级」会退化成「发畸形请求」。
pub fn build_chat_body_with_structured(
    payload: &Value,
    meta: &ModelMeta,
    user_type: &str,
    structured: bool,
) -> Result<Value, String> {
    let template = base_template()?;
    let mut body = template.clone();

    let mut messages = normalize_roles(payload.get("messages").and_then(Value::as_array));
    // ★ 与 `structured_tool_history_enabled` 同一个判据入口：DeepSeek 族「结构化开、
    // reasoning 却没回传」是半开状态，上游可能直接 400（见 `DEEPSEEK_MODEL_KEYS`）。
    let keep_reasoning = is_deepseek_key(&meta.key);
    // 调用方给的选择只能**收窄**，不能放宽：id 不齐备时强制结构化也是畸形请求。
    let structured = structured && tool_ids_ok(&messages);
    /* ★ 出站清洗（与 WB / Trae 侧同款，issue #72）：结构化历史里的孤儿（assistant
    声明了调用、却没有 tool 消息回答）会被上游判协议错误（400
    `tool_call_sequence_broken`）。清洗比「退化成文本模式」好 —— 它保住配对完整的那
    部分结构化历史；退化成文本模式会把整段历史变成伪格式，模型照抄就是乱码。 */
    if structured {
        cleanup_orphan_tool_history(&mut messages);
    }
    let flattened = flatten_messages(&messages, keep_reasoning, structured);

    // system：客户端给了就用客户端的；没给就保留模板自带的官方系统提示词。
    if let Some(system_text) = &flattened.system_text {
        body["messages"] = serde_json::json!([{ "role": "system", "content": system_text }]);
    }
    if let Some(array) = body.get_mut("messages").and_then(Value::as_array_mut) {
        array.extend(flattened.messages.iter().cloned());
    }

    // 最新用户提示词（chat_context 高亮 + business 会话名）。
    // ★ 带图消息的 content 是 parts 数组 ⇒ 文本要从 text part 里取。
    let mut prompt = String::new();
    for message in flattened.messages.iter().rev() {
        if message.get("role").and_then(Value::as_str) == Some("user") {
            if let Some(content) = message.get("content").and_then(content_text_of) {
                if !content.is_empty() {
                    prompt = content;
                    break;
                }
            }
        }
    }

    let request_id = Uuid::new_v4().to_string();
    body["request_id"] = Value::String(request_id.clone());
    body["chat_record_id"] = Value::String(request_id);
    body["request_set_id"] = Value::String(Uuid::new_v4().to_string());
    body["session_id"] = Value::String(Uuid::new_v4().to_string());
    body["stream"] = Value::Bool(true);
    body["agent_id"] = Value::String("agent_common".to_string());
    body["aliyun_user_type"] = Value::String(if user_type.is_empty() {
        cosy::DEFAULT_USER_TYPE.to_string()
    } else {
        user_type.to_string()
    });

    // model_config：以清单元数据为准，动态 key。
    let model_config = serde_json::json!({
        "key": meta.key,
        "display_name": meta.display_name,
        "model": "",
        "format": "openai",
        "is_vl": meta.is_vl,
        "is_reasoning": meta.is_reasoning,
        "api_key": "",
        "url": "",
        "source": "system",
        "max_input_tokens": meta.max_input_tokens,
    });
    body["model_config"] = model_config.clone();

    // chat_context：高亮文本 + 模型配置副本 + 图片。
    {
        let context = body
            .get_mut("chat_context")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "请求骨架缺 chat_context".to_string())?;
        if let Some(text) = context.get_mut("text").and_then(Value::as_object_mut) {
            text.insert("text".into(), Value::String(prompt.clone()));
        }
        if let Some(extra) = context.get_mut("extra").and_then(Value::as_object_mut) {
            if let Some(original) = extra
                .get_mut("originalContent")
                .and_then(Value::as_object_mut)
            {
                original.insert("text".into(), Value::String(prompt.clone()));
            }
            extra.insert("modelConfig".into(), model_config);
        }
        // ★ 顶层 `image_urls` / `chat_context.imageUrls` **默认不填**。
        //
        // 上游**只读** message content 里的结构化 `image_url` parts（实测：
        // 填了顶层字段模型仍回「我没有收到图片」；改成 parts 后才正确识别
        // 图像内容）。顶层两份是纯冗余 —— 而 base64 图片在两处各存一份会让
        // 出站体积翻倍（10 张图 15MB → 48MB），撞上游 30MB 请求体上限被 RST，
        // 还会把账号池打到熔断。
        //
        // 排查时可用 `BUDDY_SWITCH_QODER_KEEP_TOPLEVEL_IMAGES=1` 恢复旧行为。
        if !flattened.images.is_empty() && keep_toplevel_images() {
            context.insert(
                "imageUrls".into(),
                serde_json::json!(flattened.images.clone()),
            );
            body["image_urls"] = serde_json::json!(flattened.images.clone());
        }
    }

    // parameters：max_tokens（客户端没给就保留骨架默认）+ 思考档位。
    {
        let parameters = body
            .get_mut("parameters")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "请求骨架缺 parameters".to_string())?;
        let max_tokens = payload
            .get("max_tokens")
            .or_else(|| payload.get("max_completion_tokens"))
            .and_then(Value::as_i64)
            // ★ 上游范围 [1, MAX_TOKENS_LIMIT]：客户端要 100W（常见：按
            // `context_length` 反推输出预算）会被上游 400 打回，钳制后再放行。
            .map(|tokens| tokens.clamp(1, MAX_TOKENS_LIMIT));
        if let Some(max_tokens) = max_tokens {
            parameters.insert("max_tokens".into(), Value::from(max_tokens));
        }
        if let Some(effort) = extract_requested_effort(payload) {
            if let Some(canonical) = canonical_effort(&effort) {
                parameters.insert(
                    "reasoning_effort".into(),
                    Value::String(canonical.to_string()),
                );
            }
        }
    }

    // tools：客户端给了用客户端的（统一嵌套），否则置空（泄漏桌面工具是明确要避免的形态）。
    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| normalize_tools(tools))
        .unwrap_or_default();
    body["tools"] = Value::Array(tools);

    // business 会话卡片。
    {
        let business = body
            .get_mut("business")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "请求骨架缺 business".to_string())?;
        business.insert("id".into(), Value::String(Uuid::new_v4().to_string()));
        business.insert("begin_at".into(), Value::from(now_millis()));
        let name = if prompt.is_empty() {
            "chat".to_string()
        } else {
            prompt.chars().take(30).collect()
        };
        business.insert("name".into(), Value::String(name));
        business
            .entry("product".to_string())
            .or_insert_with(|| Value::String("cli".to_string()));
        business
            .entry("type".to_string())
            .or_insert_with(|| Value::String("agent".to_string()));
        business
            .entry("stage".to_string())
            .or_insert_with(|| Value::String("start".to_string()));
    }

    Ok(body)
}

// ---------------------------------------------------------------------------
// 信封 SSE 解包
// ---------------------------------------------------------------------------

/// 去掉 `data:` 前缀（多前缀也处理；`:` 注释行与空行返回空串）。
pub fn strip_data_prefix(line: &str) -> String {
    let mut text = line.trim();
    if text.is_empty() || text.starts_with(':') {
        return String::new();
    }
    while let Some(rest) = text.strip_prefix("data:") {
        text = rest.trim();
    }
    if text.is_empty() || text.starts_with(':') {
        return String::new();
    }
    text.to_string()
}

/// 剥掉上游给 delta 填充的空噪声字段；整个 delta 空且无 `finish_reason` ⇒ 空串（跳过）。
pub fn clean_chunk(raw: &str) -> String {
    let Ok(mut object) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };
    let Some(choices) = object.get_mut("choices").and_then(Value::as_array_mut) else {
        return raw.to_string();
    };
    let mut changed = false;
    let mut empty_choices = 0usize;
    let total = choices.len();
    for choice in choices.iter_mut() {
        let Some(choice_object) = choice.as_object_mut() else {
            continue;
        };
        // `finish_reason` 判断先做完（在可变借 `delta` 之前），避免借用冲突。
        let finish = choice_object
            .get("finish_reason")
            .map(|value| !value.is_null())
            .unwrap_or(false);
        let mut delta_is_empty = false;
        if let Some(delta) = choice_object
            .get_mut("delta")
            .and_then(Value::as_object_mut)
        {
            if let Some(function_call) = delta.get("function_call") {
                let empty = match function_call.as_object() {
                    Some(object) => !object.contains_key("name"),
                    None => function_call.is_null(),
                };
                if empty {
                    delta.remove("function_call");
                    changed = true;
                }
            }
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                if tool_calls.is_empty() {
                    delta.remove("tool_calls");
                    changed = true;
                }
            }
            for key in NOISE_KEYS {
                if let Some(value) = delta.get(key) {
                    if !value_truthy(value) {
                        delta.remove(key);
                        changed = true;
                    }
                }
            }
            delta_is_empty = delta.is_empty();
        }
        if delta_is_empty && !finish {
            empty_choices += 1;
        }
    }
    if total > 0 && empty_choices == total {
        return String::new();
    }
    if changed {
        serde_json::to_string(&object).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    }
}

fn value_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(false),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// 一行上游 SSE 的解包结果。
#[derive(Debug, Clone, PartialEq)]
pub enum ChatFrame {
    /// 注释 / 空行 / 无内容帧 —— 跳过。
    Skip,
    /// 流结束（`body == "[DONE]"`）。
    Done,
    /// 一个内层 OpenAI chunk（已清洗；JSON 字符串）。
    Chunk(String),
    /// 上游报错帧（`statusCodeValue != 200`），`detail` 为错误详情。
    UpstreamError { status: u16, detail: String },
}

/// 解包一行信封 SSE（见模块头的帧形态）。
pub fn unwrap_envelope_line(line: &str) -> ChatFrame {
    // ★ 先过「非信封错误形态」守卫：上游有一类失败是 **HTTP 200 + 非信封错误帧**
    //   （真机取证 2026-10-17，Qoder CN：`event:error` + Jackson 堆栈 +
    //   `{"success":false,"msgCode":500,…}`，三行都没有 `body`/`statusCodeValue`）。
    //   旧实现逐行 Skip ⇒ 客户端拿到一个**静默的空补全**（0 token），排查方向被带偏。
    if let Some(frame) = unwrap_non_envelope_error(line.trim()) {
        return frame;
    }
    let data = strip_data_prefix(line);
    if data.is_empty() {
        return ChatFrame::Skip;
    }
    let Ok(outer) = serde_json::from_str::<Value>(&data) else {
        return ChatFrame::Skip;
    };
    let Some(outer) = outer.as_object() else {
        return ChatFrame::Skip;
    };
    let status = outer
        .get("statusCodeValue")
        .and_then(Value::as_i64)
        .or_else(|| {
            outer
                .get("statusCodeValue")
                .and_then(Value::as_str)
                .and_then(|text| text.parse::<i64>().ok())
        });
    if let Some(status) = status {
        if status != 200 {
            let detail = match outer.get("body") {
                Some(Value::String(text)) => text.chars().take(400).collect(),
                Some(other) => other.to_string().chars().take(400).collect(),
                None => String::new(),
            };
            return ChatFrame::UpstreamError {
                status: status as u16,
                detail,
            };
        }
    }
    let Some(Value::String(body)) = outer.get("body") else {
        return ChatFrame::Skip;
    };
    if body == "[DONE]" {
        return ChatFrame::Done;
    }
    let cleaned = clean_chunk(body);
    if cleaned.is_empty() {
        return ChatFrame::Skip;
    }
    ChatFrame::Chunk(cleaned)
}

/// 非信封形态的错误帧识别（保守白名单：只认**明确**的错误信号）。
///
/// 判据顺序与「宁可漏判也不误杀正常帧」的纪律一致：
/// 1. `event:` 行只有含 `error` 才算错误（`event:finish` 一类计时元数据仍走 Skip）；
/// 2. JSON 对象必须**没有信封字段**（`body` / `statusCodeValue` 都没有）才可能被判错
///    —— 有信封字段的行走 [`unwrap_envelope_line`] 的原路径（含非 200 报错）；
/// 3. 错误信号 = `success:false` / 非 0 `msgCode` / 非 0 `code` / 非空 `error` /
///    Java 异常转储（`stackTrace` / `localizedMessage` / `exception`），
///    与 WorkBuddy 侧 `error_sniffer` 同一套信号词。
fn unwrap_non_envelope_error(text: &str) -> Option<ChatFrame> {
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix("event:") {
        if rest.trim().starts_with("error") {
            return Some(ChatFrame::UpstreamError {
                status: 502,
                detail: "上游以 event:error 结束本流（非信封错误帧）".to_string(),
            });
        }
        return None;
    }
    let data = strip_data_prefix(text);
    if data.is_empty() {
        return None;
    }
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&data) else {
        return None;
    };
    if object.contains_key("body") || object.contains_key("statusCodeValue") {
        return None;
    }
    let exception_dump = object.contains_key("stackTrace")
        || object.contains_key("localizedMessage")
        || object.contains_key("exceptionClass");
    // ★ `code` / `msgCode` 只认「非 0 且不在 2xx」：`{"code":200,"message":"ok"}` 这类
    //   「业务成功」信封很常见，把它判成错误就等于把正常帧变成上游错误。
    let code = error_code(&object, "msgCode").or_else(|| error_code(&object, "code"));
    let has_error_signal = exception_dump
        || matches!(object.get("success"), Some(Value::Bool(false)))
        || code.is_some()
        || object
            .get("error")
            .map(|value| match value {
                Value::Null => false,
                Value::Object(map) => !map.is_empty(),
                Value::String(text) => !text.trim().is_empty(),
                _ => true,
            })
            .unwrap_or(false);
    if !has_error_signal {
        return None;
    }
    let status = code
        .filter(|value| (100..600).contains(value))
        .unwrap_or(502);
    let detail = ["message", "msgInfo", "msg", "localizedMessage", "error"]
        .iter()
        .find_map(|key| object.get(*key).and_then(nonempty_str))
        .map(|text| text.chars().take(400).collect())
        .unwrap_or_else(|| data.chars().take(400).collect());
    Some(ChatFrame::UpstreamError {
        status: status as u16,
        detail,
    })
}

/// 取一个非空字符串（`error` 键在异常转储里可能是对象，此时不参与取词）。
fn nonempty_str(value: &Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// 取「业务错误码」：`0`（无错误）与 **2xx**（业务成功信封，如 `{"code":200}`）不算错误。
fn error_code(object: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    let value = object.get(key)?;
    let number = value.as_i64().or_else(|| {
        value
            .as_str()
            .and_then(|text| text.trim().parse::<i64>().ok())
    })?;
    if number == 0 || (200..300).contains(&number) {
        return None;
    }
    Some(number)
}

// ---------------------------------------------------------------------------
// 泄漏回声还原（严格整段识别）
// ---------------------------------------------------------------------------

/// 去掉开头的回声标记（`[assistant 请求调用工具]` 或尖括号写法）。
fn strip_leak_marker(text: &str) -> Option<&str> {
    text.strip_prefix(LEAK_MARKER)
        .or_else(|| text.strip_prefix(LEAK_MARKER_ALT))
}

/// 文本是否以**任一**回声标记开头（含只写了一半的前缀）。
pub fn starts_with_leak_marker(text: &str) -> bool {
    [LEAK_MARKER, LEAK_MARKER_ALT]
        .iter()
        .any(|marker| marker.starts_with(text) || text.starts_with(marker))
}

/// 把模型复述的「marker + JSON 数组」还原成结构化 `tool_calls`。
///
/// **严格守卫**（避免把「讨论该标记的普通回复」误判为工具调用）：
/// 整段 trim 后必须恰好是 marker + JSON 数组（可带 ``` 围栏）；数组非空；
/// 每项 name 非空、arguments 为合法 JSON；声明了工具时全部命中声明集合。
///
/// 返回 `(tool_calls, clean_text)`；识别成功时 `clean_text` 为空串。
pub fn parse_leaked_tool_calls(
    text: &str,
    allowed_names: Option<&HashSet<String>>,
) -> (Option<Vec<Value>>, String) {
    let mut t = text.trim().to_string();
    if t.starts_with("```") && t.ends_with("```") && t.len() > 6 {
        let mut inner = t[3..t.len() - 3].to_string();
        if inner.len() >= 4 && inner[..4].eq_ignore_ascii_case("json") {
            inner = inner[4..].to_string();
        }
        t = inner.trim().to_string();
    }
    let Some(rest) = strip_leak_marker(&t) else {
        return (None, text.to_string());
    };
    let rest = rest.trim();
    if !(rest.starts_with('[') && rest.ends_with(']')) {
        return (None, text.to_string());
    }
    let Ok(array) = serde_json::from_str::<Value>(rest) else {
        return (None, text.to_string());
    };
    let Some(items) = array.as_array().filter(|items| !items.is_empty()) else {
        return (None, text.to_string());
    };
    match calls_from_leak_array(items, allowed_names) {
        Some(calls) => (Some(calls), String::new()),
        None => (None, text.to_string()),
    }
}

/// 把回声数组的每一项转成结构化调用（严格校验；任一项不合格 ⇒ `None`）。
fn calls_from_leak_array(
    items: &[Value],
    allowed_names: Option<&HashSet<String>>,
) -> Option<Vec<Value>> {
    let mut calls = Vec::new();
    for item in items {
        let object = item.as_object()?;
        let name = object.get("name").and_then(Value::as_str)?;
        if name.trim().is_empty() {
            return None;
        }
        let arguments = match object.get("arguments") {
            Some(Value::String(text)) => text.clone(),
            Some(other @ (Value::Object(_) | Value::Array(_))) => {
                serde_json::to_string(other).unwrap_or_default()
            }
            _ => return None,
        };
        if !arguments.trim().is_empty() && serde_json::from_str::<Value>(&arguments).is_err() {
            return None;
        }
        // 工具会按「声明集合」校验名字：未声明的名字**不认**（宁可不还原）。
        if let Some(allowed) = allowed_names {
            if !allowed.contains(name.trim()) {
                return None;
            }
        }
        calls.push(serde_json::json!({
            "id": format!("call_{}", Uuid::new_v4().simple()),
            "type": "function",
            "function": { "name": name.trim(), "arguments": arguments },
        }));
    }
    Some(calls)
}

/// 严格整段识别**失败**时的收尾容错：`marker + JSON 数组 + 非正文尾巴`。
///
/// 典型形态（issue #66 的真机样本）：模型把两种协议格式**混着**复述，正文先是
/// 网关的调用回声 JSON，后面又跟了一串 DSML 闭标签。严格判据要求「整段恰好是
/// 数组」，于是整段还原失败 ⇒ 正文原样透传 ⇒ 用户看到一坨标记，工具一次都没执行。
///
/// 判据（只放窄一点点，避免误伤散文）：
/// - 必须以 [`LEAK_MARKER`] 开头；
/// - 数组之后**只允许**以 `<`（标签残片）或 ``` 开头 —— 也就是「不是人话」；
/// - 数组本身仍走 [`calls_from_leak_array`] 的严格校验（含声明集合）。
///
/// 返回 `(calls, clean_text)`；`clean_text` 是数组之前的散文（通常为空）。
pub fn parse_leaked_tool_calls_with_trailing(
    text: &str,
    allowed_names: Option<&HashSet<String>>,
) -> (Option<Vec<Value>>, String) {
    let t = text.trim();
    let Some(after_marker) = strip_leak_marker(t) else {
        return (None, text.to_string());
    };
    let Some(bracket) = after_marker.find('[') else {
        return (None, text.to_string());
    };
    let body = &after_marker[bracket..];
    // 只从尾部往前试（数组一定在尾巴之前结束），最多试 64 个候选，避免病态输入打满 CPU。
    let mut attempts = 0;
    for (position, ch) in body.char_indices().rev() {
        if ch != ']' {
            continue;
        }
        attempts += 1;
        if attempts > 64 {
            break;
        }
        let candidate = &body[..=position];
        let Ok(array) = serde_json::from_str::<Value>(candidate) else {
            continue;
        };
        let Some(items) = array.as_array().filter(|items| !items.is_empty()) else {
            continue;
        };
        let Some(calls) = calls_from_leak_array(items, allowed_names) else {
            continue;
        };
        let tail = body[position + 1..].trim();
        if !(tail.is_empty() || tail.starts_with('<') || tail.starts_with("```")) {
            return (None, text.to_string());
        }
        return (Some(calls), String::new());
    }
    (None, text.to_string())
}

/// 正文相对「泄漏回声」的位置（[`parse_leaked_tool_calls`] 与流式守卫共用同一套前缀规则）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeakState {
    /// 还处在围栏 / 标记前缀区（可能只写了一半）—— 仍可能是回声。
    Prefix,
    /// 已越过标记，携带其后的正文（可能带 ``` 收尾围栏）。
    Body(String),
    /// 显然不是回声。
    NotLeak,
}

/// 把「标记之前的围栏前缀 / 标记之后的正文」解析出来。
///
/// 为什么单独抽一个：流式守卫要判断「这段缓冲**还可能**是回声吗」（Prefix 就得继续等），
/// 而整段判定只关心 Body。两处若各写一套前缀规则，必然分裂成两种结论。
pub fn leak_body_offset(text: &str) -> LeakState {
    let t = text.trim_start();
    if t.is_empty()
        || [LEAK_MARKER, LEAK_MARKER_ALT]
            .iter()
            .any(|m| m.starts_with(t))
    {
        return LeakState::Prefix;
    }
    if let Some(body) = strip_leak_marker(t) {
        return LeakState::Body(body.to_string());
    }
    if !t.starts_with('`') {
        return LeakState::NotLeak;
    }
    if t.len() < 3 {
        return if "```".starts_with(t) {
            LeakState::Prefix
        } else {
            LeakState::NotLeak
        };
    }
    let Some(rest) = t.strip_prefix("```") else {
        return LeakState::NotLeak;
    };
    let after_fence = rest.trim_start();
    let low = after_fence.to_ascii_lowercase();
    let after = if low.starts_with("json") {
        after_fence[4..].trim_start()
    } else if starts_with_leak_marker(&low) {
        // 无语言标记的 ``` 围栏
        after_fence
    } else if "json".starts_with(&low) {
        // 语言标记还没打完（j / js / json）
        return LeakState::Prefix;
    } else {
        return LeakState::NotLeak;
    };
    if [LEAK_MARKER, LEAK_MARKER_ALT]
        .iter()
        .any(|m| m.starts_with(after))
    {
        return LeakState::Prefix;
    }
    if let Some(body) = strip_leak_marker(after) {
        return LeakState::Body(body.to_string());
    }
    LeakState::NotLeak
}

/// 回声里出现的工具名是否**全部**在本次声明集合里。
///
/// 判据收紧的原因（参考实现 #72）：只按「本次声明了 tools」就 fail-closed 太宽 ——
/// 模型复述**未声明**的工具时我们本来也还原不了，把那种回合判失败会误伤正常回答。
/// 返回 `false` 也用于「文本里没有名字」的情形，由调用方单独处理。
pub fn echo_names_declared(text: &str, allowed: &HashSet<String>) -> bool {
    let mut names: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find("\"name\"") {
        let after = rest[position + 6..].trim_start();
        let Some(after) = after.strip_prefix(':') else {
            rest = after;
            continue;
        };
        let after = after.trim_start();
        let Some(after) = after.strip_prefix('"') else {
            rest = after;
            continue;
        };
        let Some(end) = after.find('"') else {
            break;
        };
        names.push(&after[..end]);
        rest = &after[end..];
    }
    !names.is_empty() && names.iter().all(|name| allowed.contains(*name))
}

/// 回声**被截断**（JSON 不完整 / 非法）时是否该整段吞掉。
///
/// 三个条件同时成立才吞（缺一不可）：
/// 1. trim 后以 [`LEAK_MARKER`] 开头（正常散文几乎不会这样开头）；
/// 2. 本次请求**声明了 tools**（与还原路径同一前置条件）；
/// 3. 标记之后（剥掉围栏）仍是 JSON 数组字面量的**前缀** —— 即「本该是数组，只是没写完」。
///
/// 完整且合法的数组**不吞**：那种情况还原失败另有原因（工具名未声明 / arguments 非法），
/// 保持既有 fail-open 语义。只有「数组本身没写完 / 非法」才吞。
///
/// ★ 这是 issue #66 的直接对策：模型把历史工具调用复述到正文、又被上游截断后，
///   既还原不成结构化调用，也不能把网关内部协议文本（标记 + 半截 JSON）交给终端用户。
pub fn leaked_partial_droppable(text: &str, allowed_names: Option<&HashSet<String>>) -> bool {
    if text.trim().is_empty() || allowed_names.map(HashSet::is_empty).unwrap_or(true) {
        return false;
    }
    let t = text.trim();
    if t == LEAK_MARKER || t == LEAK_MARKER_ALT {
        // 只有标记、连数组都没开始写：判据 3 的退化情形，同样是回声。
        return true;
    }
    // 有工具名且含**未声明**的名字 ⇒ 这条判据认不了（保持透传，参考实现 #72）。
    let allowed = allowed_names.expect("上面已过滤空集合");
    if t.contains("\"name\"") && !echo_names_declared(t, allowed) {
        return false;
    }
    let LeakState::Body(body) = leak_body_offset(t) else {
        return false;
    };
    let mut rest = body.trim();
    if let Some(stripped) = rest.strip_suffix("```") {
        rest = stripped.trim();
    }
    // 标记之后不是 JSON 数组 ⇒ 是散文（可能是正常讨论），不吞。
    if !rest.starts_with('[') {
        return false;
    }
    // 完整且合法的数组不吞：那种情况还原失败另有原因（工具名未声明 / arguments
    // 非法），保持既有 fail-open 语义；只有「数组没写完 / 损坏」才吞。
    serde_json::from_str::<Value>(rest).is_err()
}

/// 拆出「[工具结果<name>]」开标记行之后的部分；不是合法开标记 ⇒ `None`。
fn tool_echo_body(text: &str) -> Option<&str> {
    let t = text.trim();
    let rest = t.strip_prefix(TOOL_RESULT_MARKER)?;
    let close = rest.find(']')?;
    if rest[..close].contains('\n') {
        return None; // 标记行未闭合 / 换行后才有 `]`：非本网关形态
    }
    Some(&rest[close + 1..])
}

/// 正文是「[工具结果…]」回声块时是否该整段吞掉（与 [`leaked_partial_droppable`] 同形态）。
///
/// 同机纪律：只有**声明了 tools** 才吞（与回声回读同一前置，避免误杀正常讨论）；
/// 形态自洽即可 —— 出现模型自造的闭标记，或标记之后（忽略空白）以 JSON / 标签起始
/// 字符开头（含被截断、永不闭合的块）。
///
/// 不还原成 tool 消息：还原需要把回声与历史上的 `tool_call_id` 配对，而文本化历史
/// 并未保留 call_id ⇒ 本轮只做「不把内部协议文本交给终端用户」。
pub fn tool_echo_droppable(text: &str, allowed_names: Option<&HashSet<String>>) -> bool {
    if allowed_names.map(HashSet::is_empty).unwrap_or(true) {
        return false;
    }
    let Some(body) = tool_echo_body(text) else {
        return false;
    };
    if body.contains(TOOL_RESULT_CLOSE) {
        return true;
    }
    let rest = body.trim_start();
    match rest.chars().next() {
        None => true, // 只有开标记：退化为「纯标记」
        Some(first) => matches!(first, '{' | '[' | '"' | '<'),
    }
}

// ---------------------------------------------------------------------------
// DSML 工具调用回退解析
// ---------------------------------------------------------------------------

/// DSML 标签里唯一可辨的「名字」部分（`<｜｜DSML｜｜ calls>` / `invoke` / `parameter`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DsmlTag {
    Calls { closing: bool },
    Invoke { closing: bool },
    Parameter { closing: bool },
    Other,
}

/// 解析一个 `<…>` 标签：`(标签内文, 种类, name 属性)`。
fn classify_tag(inner: &str) -> (DsmlTag, Option<String>) {
    let trimmed = inner.trim();
    let closing = trimmed.starts_with('/');
    let body = trimmed.trim_start_matches('/').trim();
    /* ★ 标签名可能**紧贴**哨兵：真机实测模型写成 `<｜｜DSML｜｜calls>`（中间没有空格），
    按「整词等于 calls」判会全部落空 ⇒ 整块还原失败 ⇒ 正文被吞（用户看到空回答）。
    因此按「词尾」匹配：`｜｜DSML｜｜calls` / `｜｜DSML｜｜ calls` / `calls` 都认。 */
    let mut kind = DsmlTag::Other;
    for token in body.split_whitespace() {
        let word = token.trim_end_matches('>').to_ascii_lowercase();
        // 属性 token（`name="calls"`）不带引号收尾时不该被当成标签名。
        if word.contains('=') {
            continue;
        }
        if word.ends_with("parameter") {
            kind = DsmlTag::Parameter { closing };
            break;
        }
        if word.ends_with("invoke") {
            kind = DsmlTag::Invoke { closing };
            break;
        }
        if word.ends_with("calls") {
            kind = DsmlTag::Calls { closing };
            break;
        }
    }
    // `name="x"` / `name='x'`：只认引号包裹（无引号的值不是官方形态，不猜）。
    let name = body.find("name=").and_then(|at| {
        let rest = &body[at + "name=".len()..];
        let mut chars = rest.chars();
        let quote = chars.next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let rest = chars.as_str();
        let end = rest.find(quote)?;
        Some(rest[..end].to_string())
    });
    (kind, name)
}

/// 找下一个**真正的标签**：`(起始字节, 结束字节, 标签内文)`。
///
/// 值里出现的裸 `<`（`if (a < b)`、`a<b>c`）会造出「假标签」：从 `<` 到最近的 `>`
/// 之间若还有 `<`，说明这不是一个标签，跳过继续找。否则参数值会被**截断在假标签处**。
fn next_tag(text: &str, from: usize) -> Option<(usize, usize, &str)> {
    let mut cursor = from;
    loop {
        let start = cursor + text[cursor..].find('<')?;
        let rest = &text[start + 1..];
        let end_rel = rest.find('>')?;
        if rest[..end_rel].contains('<') {
            cursor = start + 1;
            continue;
        }
        let end = start + 1 + end_rel + 1;
        return Some((start, end, &text[start + 1..end - 1]));
    }
}

/// 从 `from` 起找**指定种类**的标签（跳过其它标签与假标签）。
fn find_tag(text: &str, from: usize, want: DsmlTag) -> Option<(usize, usize)> {
    let mut cursor = from;
    while let Some((start, end, inner)) = next_tag(text, cursor) {
        if classify_tag(inner).0 == want {
            return Some((start, end));
        }
        cursor = end;
    }
    None
}

/// 解析一段 DSML 调用块内的 `invoke` 列表（参数值里允许出现裸 `<`）。
fn parse_dsml_invokes(block: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some((_start, end, inner)) = next_tag(block, cursor) {
        cursor = end;
        let (kind, name) = classify_tag(inner);
        if kind != (DsmlTag::Invoke { closing: false }) {
            continue;
        }
        let Some(name) = name else { continue };
        // 本 invoke 的边界：配对的 `</…invoke>`（缺失 ⇒ 剩余全部，截断形态也尽量取值）。
        let (body_end, next_cursor) = match find_tag(block, end, DsmlTag::Invoke { closing: true })
        {
            Some((close_start, close_end)) => (close_start, close_end),
            None => (block.len(), block.len()),
        };
        let body = &block[end..body_end];
        let mut params: Vec<(String, String)> = Vec::new();
        let mut inner_cursor = 0;
        while let Some((_p_start, p_end, p_inner)) = next_tag(body, inner_cursor) {
            inner_cursor = p_end;
            let (p_kind, p_name) = classify_tag(p_inner);
            if p_kind != (DsmlTag::Parameter { closing: false }) {
                continue;
            }
            let Some(p_name) = p_name else { continue };
            let (value_end, after) =
                match find_tag(body, p_end, DsmlTag::Parameter { closing: true }) {
                    Some((value_end, close_end)) => (value_end, close_end),
                    None => (body.len(), body.len()),
                };
            params.push((p_name, body[p_end..value_end].trim().to_string()));
            inner_cursor = after;
        }
        out.push((name, params));
        cursor = next_cursor;
    }
    out
}

/// 把模型写出的 **DSML 工具调用块**还原成结构化 `tool_calls`。
///
/// 为什么需要：客户端（CodeBuddy / WorkBuddy 等）在自己的 system 里教模型用
/// `<｜｜DSML｜｜ calls>` 语法发起工具调用；Qoder 上游的文本模式下模型会**照学**，
/// 把调用写成 DSML 标签塞进正文。网关若不还原，客户端收到的是一坨标签文本 ——
/// 工具不会执行，任务在本该调工具的位置断掉（issue #66 的「一个任务能断好多次」）。
///
/// 判据：必须找到**成对**的 `calls` 标签块且块内至少一个 `invoke name=…`；
/// 名字为空 ⇒ 不认（宁可透传也不造出一个空名字的调用）。
/// `allowed_names` **刻意不参与**过滤：模型有时会调用客户端没声明但确实存在的工具名，
/// 与参考实现一致——还原失败才回退透传。
///
/// 返回 `(tool_calls, clean_text)`；成功时 `clean_text` 是**剥掉调用块**后的正文。
pub fn parse_dsml_tool_calls(text: &str) -> (Option<Vec<Value>>, String) {
    if text.is_empty() || !text.contains("DSML") {
        return (None, text.to_string());
    }
    // 找第一个 calls 开标签，再找其后第一个 calls 闭标签。
    let mut cursor = 0;
    let mut block: Option<(usize, usize, usize)> = None; // (开标签起点, 闭标签终点, 内容起点)
    while let Some((start, end, inner)) = next_tag(text, cursor) {
        cursor = end;
        let (kind, _) = classify_tag(inner);
        // ⚠️ 只关心「calls 开标签」这一种，其余一律忽略 ⇒ `if let` 足够
        // （`match` 单分支写法会被 clippy::single_match 拦下）。
        if let DsmlTag::Calls { closing: false } = kind {
            if let Some((_close_start, close_end)) =
                find_tag(text, end, DsmlTag::Calls { closing: true })
            {
                block = Some((start, close_end, end));
                break;
            }
        }
    }
    let Some((start, end, content_start)) = block else {
        return (None, text.to_string());
    };
    let invokes = parse_dsml_invokes(&text[content_start..end]);
    if invokes.is_empty() {
        return (None, text.to_string());
    }
    let mut calls = Vec::new();
    for (name, params) in invokes {
        let mut arguments = Map::new();
        for (key, value) in params {
            arguments.insert(key, Value::String(value));
        }
        let arguments =
            serde_json::to_string(&Value::Object(arguments)).unwrap_or_else(|_| "{}".into());
        calls.push(serde_json::json!({
            "id": format!("call_{}", Uuid::new_v4().simple()),
            "type": "function",
            "function": { "name": name.trim(), "arguments": arguments },
        }));
    }
    let head = text[..start].trim();
    let tail = text[end..].trim();
    let clean = match (head.is_empty(), tail.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tail.to_string(),
        (false, true) => head.to_string(),
        (false, false) => format!("{head} {tail}"),
    };
    (Some(calls), clean)
}

// ---------------------------------------------------------------------------
// 模型清单
// ---------------------------------------------------------------------------

/// 解析模型清单响应：`{"chat":[{key, display_name, …}]}` →
/// **官方条目全字段原样透传**（不裁剪 enable/strategies/…），仅补 `id`/`name` 兼容键。
pub fn parse_model_list(payload: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(chat) = payload.get("chat").and_then(Value::as_array) else {
        return out;
    };
    for entry in chat {
        let Some(object) = entry.as_object() else {
            continue;
        };
        let Some(key) = object.get("key").and_then(Value::as_str) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        let mut row = object.clone();
        let display = object
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_string();
        row.insert("id".into(), Value::String(key.to_string()));
        row.entry("name".to_string())
            .or_insert_with(|| Value::String(display.clone()));
        row.entry("display_name".to_string())
            .or_insert_with(|| Value::String(display));
        out.push(Value::Object(row));
    }
    out
}

// ---------------------------------------------------------------------------
// openapi 通道：刷新 / 签到 / 额度 / 套餐
// ---------------------------------------------------------------------------

/// 刷新族（按**令牌前缀**路由，见 `token.rs`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshKind {
    /// `drt-` → `/api/v1/deviceToken/refresh`
    Device,
    /// `jrt-` → `/api/v1/jobToken/refresh`
    Job,
}

/// 刷新结果。
#[derive(Debug, Clone, PartialEq)]
pub struct TokenRefresh {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// 过期时刻（unix 秒）。`expires_in` 单位是**毫秒**；两个字段都缺时按**令牌家族**兜底
    /// （`jrt-` 24 小时 / `drt-` 30 天，见 [`FALLBACK_EXPIRY_JOB_SECS`]）。
    pub expires_at: i64,
}

/// 解析刷新响应（`kind` 决定 token 字段名）。
pub fn parse_refresh_response(
    kind: RefreshKind,
    data: &Value,
    previous_refresh: &str,
) -> TokenRefresh {
    let access_token = match kind {
        RefreshKind::Device => data
            .get("token")
            .or_else(|| data.get("device_token"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        RefreshKind::Job => data
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    };
    let refresh_token = data
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if previous_refresh.is_empty() {
                None
            } else {
                Some(previous_refresh.to_string())
            }
        });
    TokenRefresh {
        access_token,
        refresh_token,
        // ★ issue #130 §5.3：兜底寿命按**令牌家族**取 —— `jrt-` 约 24 小时，
        //   `drt-` 约 30 天。共用 30 天会把 24 小时寿命的令牌标成「还有一个月」，
        //   于是保活永远跳过它，直到它 401。
        expires_at: token_expiry_with_fallback(
            data,
            match kind {
                RefreshKind::Job => FALLBACK_EXPIRY_JOB_SECS,
                RefreshKind::Device => FALLBACK_EXPIRY_DEFAULT_SECS,
            },
        ),
    }
}

/// 从 JSON 里读一个整数 —— **同时接受浮点**。
///
/// ★★ 这条不是「顺手兼容」，是**真机取证后的必修项**：上游的额度响应里
/// `remaining` / `total` / `used` 都是 **浮点**（实测 `{"remaining":99.0,"total":100.0}`），
/// 而 `Value::as_i64()` 对 `99.0` 返回 `None`。只认整数的写法会让**有余额的账号显示成
/// 0 / 0** —— 界面上那是「这个号没积分了」，而它其实还有 99 —— **谎报**，比报错坏得多。
///
/// 取整用 `round`（额度是 credits 的整数语义；上游给 `.0` 是序列化形态，不是真小数）。
/// 非数值（字符串 / null / 对象）仍返回 `None`，由调用方决定是 0 还是「未知」。
pub(crate) fn as_num(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number.round() as i64))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<f64>().ok())
                .map(|n| n.round() as i64)
        })
}

/// 兜底寿命（秒）：`jrt-`（Job 家族）的 access token 实测约 **24 小时**，
/// 其余（`drt-` 设备令牌 / OAuth 登录）约 **30 天**。
///
/// ## 为什么必须按家族分开（issue #130 §5.3）
///
/// 改造前两者共用「30 天」这一个兜底。若某次 `jrt-` 刷新响应里既没有 `expires_in`
/// 也没有 `expires_at`，到期时刻会被写成「30 天后」，而**真实寿命只有 24 小时** ⇒
/// 此后 `needs_keepalive` 一直判「还剩 29 天，不用刷」，保活对这个号**彻底失效**，
/// 直到它 401（再被判 `AuthDead`，只能人工恢复）。
///
/// 这条与窗口大小无关：窗口调到 48 小时也救不了，因为**判据输入本身是错的**。
pub const FALLBACK_EXPIRY_JOB_SECS: i64 = 24 * 3_600;

/// 兜底寿命（秒）：`drt-` / OAuth 家族（约 30 天）。
pub const FALLBACK_EXPIRY_DEFAULT_SECS: i64 = 30 * 86_400;

/// `expires_in`（毫秒）/ `expires_at`（RFC3339）→ unix 秒；都没有按 30 天。
///
/// ⚠️ 不知道令牌家族时**只能**用 30 天兜底。刷新路径请用
/// [`token_expiry_with_fallback`] 并按 [`RefreshKind`] 传对应家族的兜底值
/// （见 [`FALLBACK_EXPIRY_JOB_SECS`]）。
pub fn token_expiry(data: &Value) -> i64 {
    token_expiry_with_fallback(data, FALLBACK_EXPIRY_DEFAULT_SECS)
}

/// 同 [`token_expiry`]，但「两个字段都没有」时用调用方给的家族兜底。
pub fn token_expiry_with_fallback(data: &Value, fallback_secs: i64) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    if let Some(expires_in) = data.get("expires_in").and_then(as_num) {
        return now + expires_in / 1000;
    }
    if let Some(text) = data.get("expires_at").and_then(Value::as_str) {
        if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(
            text.get(..19).unwrap_or(text),
            "%Y-%m-%dT%H:%M:%S",
        ) {
            return datetime.and_utc().timestamp();
        }
    }
    now + fallback_secs
}

/// 签到状态。
#[derive(Debug, Clone, PartialEq)]
pub struct CheckinStatus {
    /// 上游原始状态词（`CLAIMABLE` / `CLAIMED` / `DISABLED` …）。
    pub status: String,
    /// 活动是否活跃（CLAIMABLE / CLAIMED）。
    pub active: bool,
    pub today_checked_in: bool,
    pub streak_days: i64,
    pub total_claim_days: i64,
    pub reward_credits: i64,
    pub total_reward_credits: i64,
    pub next_claim_at: i64,
    pub last_claimed_at: i64,
    pub reward_expires_at: i64,
}

/// 解析签到状态响应；`today` 是本地日期 `YYYY-MM-DD`（由调用方注入，纯函数可测）。
pub fn parse_checkin_status(payload: &Value, today: &str) -> CheckinStatus {
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let last_claimed_at = payload
        .get("lastClaimedAt")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let last_day = if last_claimed_at > 0 {
        chrono::DateTime::from_timestamp(last_claimed_at, 0)
            .map(|datetime| {
                datetime.with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
            })
            .map(|datetime| datetime.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    // ⚠️ 走 [`as_num`] 而不是 `as_i64`：上游把计数写成浮点（`1.0`）时 `as_i64` 会返回 None
    //    ⇒ 连续签到天数 / 累计领取次数全部静默变 0。
    let num = |key: &str| payload.get(key).and_then(as_num).unwrap_or(0);
    CheckinStatus {
        active: status == "CLAIMABLE" || status == "CLAIMED",
        today_checked_in: status == "CLAIMED" && last_day == today,
        streak_days: num("currentStreakDays"),
        total_claim_days: num("totalClaimDays"),
        reward_credits: num("rewardCredits"),
        total_reward_credits: num("totalRewardCredits"),
        next_claim_at: num("nextClaimAt"),
        last_claimed_at,
        reward_expires_at: num("rewardExpiresAt"),
        status,
    }
}

/// 额度视图（userQuota + addOnQuota 的聚合）。
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaView {
    pub remaining: i64,
    pub used: i64,
    pub total: i64,
    pub is_exceeded: bool,
    /// 原始响应透传（页面上「资源包明细」等展示字段的来源）。
    pub raw: Value,
}

/// 解析额度响应。
///
/// **退化守卫**（与官方客户端的判断一致，保守放行）：两个 quota 键都缺失，
/// 或 `isQuotaExceeded=true` 且 remain 与 size 合计全 0（「从没有过额度」）⇒ `Err`
/// —— **拒绝把解析不了的响应写成 0 余额**（0 是谎报，报错是诚实）。
pub fn parse_quota(payload: &Value) -> Result<QuotaView, String> {
    let user_quota = payload.get("userQuota").and_then(Value::as_object);
    let add_on_quota = payload.get("addOnQuota").and_then(Value::as_object);
    if user_quota.is_none() && add_on_quota.is_none() {
        return Err("额度响应缺少 userQuota / addOnQuota（解析不了，拒绝写 0）".to_string());
    }
    // ⚠️ 走 [`as_num`]（同时接受浮点）—— 实测上游给的就是 `99.0 / 100.0 / 1.0`，
    //    用 `as_i64` 会把一份**有余额**的额度解析成 `0 / 0 / 0`（谎报「已用完」）。
    let num = |source: Option<&Map<String, Value>>, key: &str| {
        source
            .and_then(|object| object.get(key))
            .and_then(as_num)
            .unwrap_or(0)
    };
    let remaining = num(user_quota, "remaining") + num(add_on_quota, "remaining");
    let used = num(user_quota, "used") + num(add_on_quota, "used");
    let total = num(user_quota, "total") + num(add_on_quota, "total");
    let is_exceeded = payload
        .get("isQuotaExceeded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if is_exceeded && remaining == 0 && total == 0 {
        return Err("额度响应为退化形态（exceeded 且全 0）—— 拒绝写入".to_string());
    }
    Ok(QuotaView {
        remaining,
        used,
        total,
        is_exceeded,
        raw: payload.clone(),
    })
}

/// openapi 通道的请求头集（逐字对齐官方客户端）。
pub fn openapi_headers(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
) -> Vec<(String, String)> {
    let spec = region::realm_spec(region);
    let request_id = format!(
        "{}-{:06}",
        super::device::derive_id(uid, "req"),
        now_millis() % 1_000_000
    );
    vec![
        ("Content-Type".into(), "application/json".into()),
        ("Accept".into(), "application/json, text/plain, */*".into()),
        ("User-Agent".into(), spec.user_agent.to_string()),
        ("Authorization".into(), format!("Bearer {access_token}")),
        ("X-Request-ID".into(), request_id),
        (
            "X-Machine-ID".into(),
            super::device::derive_id(uid, "machine"),
        ),
        (
            "X-Session-ID".into(),
            super::device::derive_id(uid, "session"),
        ),
        ("Origin".into(), spec.website.to_string()),
        ("Referer".into(), format!("{}/", spec.website)),
    ]
}

// ---------------------------------------------------------------------------
// HTTP 执行层
// ---------------------------------------------------------------------------

/// 进程内复用的 HTTP 客户端（连接池）。
pub fn shared_client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|error| format!("构造 HTTP 客户端失败: {error}"))
        })
        .as_ref()
        .map_err(|error| error.clone())
}

fn apply_headers(
    mut request: reqwest::RequestBuilder,
    headers: &[(String, String)],
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
}

/// 刷新令牌：按 `kind` 路由到 device / job 刷新端点（普通 JSON POST，不签名）。
pub async fn refresh_token(
    region: QoderRegion,
    kind: RefreshKind,
    refresh_token: &str,
) -> Result<TokenRefresh, String> {
    let spec = region::realm_spec(region);
    let path = match kind {
        RefreshKind::Device => super::PATH_DEVICE_TOKEN_REFRESH,
        RefreshKind::Job => super::PATH_JOB_TOKEN_REFRESH,
    };
    let client = shared_client()?;
    let response = apply_headers(
        client
            .post(format!("{}{path}", spec.openapi_base))
            .json(&serde_json::json!({ "refresh_token": refresh_token })),
        &[
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "application/json".into()),
            ("User-Agent".into(), spec.user_agent.to_string()),
        ],
    )
    .timeout(std::time::Duration::from_secs(30))
    .send()
    .await
    .map_err(|error| format!("刷新令牌请求失败: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取刷新响应失败: {error}"))?;
    if !status.is_success() {
        let detail = body.chars().take(160).collect::<String>();
        return Err(format!("刷新令牌失败: HTTP {} {detail}", status.as_u16()));
    }
    let data: Value =
        serde_json::from_str(&body).map_err(|error| format!("刷新响应不是 JSON: {error}"))?;
    Ok(parse_refresh_response(kind, &data, refresh_token))
}

/// PAT → jobToken 交换（`pt-` 的刷新路径：PAT 换 jobToken）。
pub async fn exchange_pat(
    region: QoderRegion,
    personal_token: &str,
) -> Result<TokenRefresh, String> {
    let spec = region::realm_spec(region);
    let client = shared_client()?;
    let response = apply_headers(
        client
            .post(format!(
                "{}{}",
                spec.openapi_base,
                super::PATH_JOB_TOKEN_EXCHANGE
            ))
            .json(&serde_json::json!({ "personal_token": personal_token })),
        &[
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "application/json".into()),
            ("User-Agent".into(), spec.user_agent.to_string()),
        ],
    )
    .timeout(std::time::Duration::from_secs(30))
    .send()
    .await
    .map_err(|error| format!("PAT 交换请求失败: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取交换响应失败: {error}"))?;
    if !status.is_success() {
        let detail = body.chars().take(160).collect::<String>();
        return Err(format!("PAT 交换失败: HTTP {} {detail}", status.as_u16()));
    }
    let data: Value =
        serde_json::from_str(&body).map_err(|error| format!("交换响应不是 JSON: {error}"))?;
    Ok(parse_refresh_response(RefreshKind::Job, &data, ""))
}

/// 签到通道的可用性判定：404 / 405 / 410 ⇒ 本区域未开放该接口。
pub const CHECKIN_REASON_NOT_FOUND: &str = "checkin_endpoint_not_found";

/// 签到状态查询。返回 `Err(CHECKIN_REASON_NOT_FOUND...)` 表示该区域没有此接口。
pub async fn checkin_status(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
) -> Result<CheckinStatus, String> {
    let spec = region::realm_spec(region);
    let client = shared_client()?;
    let response = apply_headers(
        client
            .get(format!(
                "{}{}",
                spec.openapi_base,
                super::PATH_CHECKIN_STATUS
            ))
            .timeout(std::time::Duration::from_secs(15)),
        &openapi_headers(region, uid, access_token),
    )
    .send()
    .await
    .map_err(|error| format!("签到状态请求失败: {error}"))?;
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    if matches!(status_code.as_u16(), 404 | 405 | 410) {
        return Err(format!(
            "{CHECKIN_REASON_NOT_FOUND} (HTTP {})",
            status_code.as_u16()
        ));
    }
    if !status_code.is_success() {
        return Err(format!(
            "签到状态失败: HTTP {} {}",
            status_code.as_u16(),
            body.chars().take(160).collect::<String>()
        ));
    }
    let data: Value =
        serde_json::from_str(&body).map_err(|error| format!("签到状态响应不是 JSON: {error}"))?;
    let today = chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .format("%Y-%m-%d")
        .to_string();
    Ok(parse_checkin_status(&data, &today))
}

/// 签到领取（`POST` 空对象 body；调用前应先查状态确认 `CLAIMABLE`）。
pub async fn checkin_claim(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
) -> Result<Value, String> {
    let spec = region::realm_spec(region);
    let client = shared_client()?;
    let response = apply_headers(
        client
            .post(format!(
                "{}{}",
                spec.openapi_base,
                super::PATH_CHECKIN_CLAIM
            ))
            .json(&serde_json::json!({}))
            .timeout(std::time::Duration::from_secs(15)),
        &openapi_headers(region, uid, access_token),
    )
    .send()
    .await
    .map_err(|error| format!("签到领取请求失败: {error}"))?;
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status_code.is_success() {
        return Err(format!(
            "签到领取失败: HTTP {} {}",
            status_code.as_u16(),
            body.chars().take(160).collect::<String>()
        ));
    }
    serde_json::from_str(&body).map_err(|error| format!("签到响应不是 JSON: {error}"))
}

/// 额度查询（聚合 userQuota + addOnQuota；退化响应返回 `Err`，**不写 0**）。
pub async fn quota_usage(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
) -> Result<QuotaView, String> {
    let spec = region::realm_spec(region);
    let client = shared_client()?;
    let response = apply_headers(
        client
            .get(format!("{}{}", spec.openapi_base, super::PATH_QUOTA_USAGE))
            .timeout(std::time::Duration::from_secs(30)),
        &openapi_headers(region, uid, access_token),
    )
    .send()
    .await
    .map_err(|error| format!("额度查询失败: {error}"))?;
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status_code.is_success() {
        return Err(format!(
            "额度查询失败: HTTP {} {}",
            status_code.as_u16(),
            body.chars().take(160).collect::<String>()
        ));
    }
    let data: Value =
        serde_json::from_str(&body).map_err(|error| format!("额度响应不是 JSON: {error}"))?;
    parse_quota(&data)
}

/// 套餐名（如 `Pro Trial`）。
pub async fn user_plan(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
) -> Result<Value, String> {
    let spec = region::realm_spec(region);
    let client = shared_client()?;
    let response = apply_headers(
        client
            .get(format!("{}{}", spec.openapi_base, super::PATH_USER_PLAN))
            .timeout(std::time::Duration::from_secs(15)),
        &openapi_headers(region, uid, access_token),
    )
    .send()
    .await
    .map_err(|error| format!("套餐查询失败: {error}"))?;
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status_code.is_success() {
        return Err(format!(
            "套餐查询失败: HTTP {} {}",
            status_code.as_u16(),
            body.chars().take(160).collect::<String>()
        ));
    }
    serde_json::from_str(&body).map_err(|error| format!("套餐响应不是 JSON: {error}"))
}

/// 拉取模型清单（COSY 签名；**GET 也带 body** `qoder_encode("{}")`，裸 GET 会 403）。
///
/// 主机按官方的候选顺序尝试（国际版 api1 → api2 → api3；签名只覆盖 path，换主机不影响签名）。
pub async fn fetch_model_list(
    region: QoderRegion,
    input: &CosyAccountInput<'_>,
) -> Result<Vec<Value>, String> {
    fetch_model_list_at(&region::gateway_candidates(region), input).await
}

/// 同 [`fetch_model_list`]，但由调用方指定候选主机（**测试挂载点**：指向 mock 上游；
/// 生产代码不要绕过 [`fetch_model_list`]）。
pub async fn fetch_model_list_at(
    hosts: &[&str],
    input: &CosyAccountInput<'_>,
) -> Result<Vec<Value>, String> {
    let sign_body = cosy::qoder_encode(b"{}");
    let session = cosy::shared_cache().get(input)?;
    let client = shared_client()?;
    let mut last_error = String::new();
    for host in hosts {
        let raw_url = format!("{host}{PATH_MODELS_ENCODED}");
        // ★ 签名用**无 query** 的 path（见 [`PATH_MODELS_SIGN`]）。
        let headers = session.headers(&sign_body, PATH_MODELS_SIGN, None, false);
        let request = apply_headers(
            client
                .get(&raw_url)
                .body(sign_body.clone())
                .timeout(std::time::Duration::from_secs(15)),
            &headers,
        );
        match request.send().await {
            Ok(response) => {
                let status_code = response.status();
                let body = response.text().await.unwrap_or_default();
                if !status_code.is_success() {
                    last_error = format!(
                        "HTTP {} {}",
                        status_code.as_u16(),
                        body.chars().take(160).collect::<String>()
                    );
                    continue;
                }
                let data: Value = serde_json::from_str(&body)
                    .map_err(|error| format!("模型清单不是 JSON: {error}"))?;
                return Ok(parse_model_list(&data));
            }
            Err(error) => {
                last_error = error.to_string();
            }
        }
    }
    Err(format!(
        "模型清单拉取失败（已试全部候选主机）: {last_error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★★ issue #130 §5.3：兜底寿命必须按**令牌家族**取。
    ///
    /// `jrt-`（Job）家族的 access token 实测约 24 小时；共用 30 天兜底会把
    /// 「24 小时后过期」标成「还有一个月」，于是 `needs_keepalive` 永远判「不用刷」，
    /// 保活对这个号彻底失效，直到它 401（再被判 `AuthDead`，只能人工恢复）。
    ///
    /// 变异：把 `parse_refresh_response` 里按 `kind` 取兜底改回 `token_expiry(data)`
    /// ⇒ 第一条断言红（24h 变成 30 天）。
    #[test]
    fn refresh_fallback_expiry_follows_the_token_family() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        // 两个字段都没有 ⇒ 走兜底。
        let empty = serde_json::json!({ "token": "jt-new" });
        let job = parse_refresh_response(RefreshKind::Job, &empty, "jrt-old");
        let delta = job.expires_at - now;
        assert!(
            (FALLBACK_EXPIRY_JOB_SECS - 5..=FALLBACK_EXPIRY_JOB_SECS + 5).contains(&delta),
            "jrt- 家族兜底必须是 24 小时，实际 {delta}s"
        );
        let device = parse_refresh_response(RefreshKind::Device, &empty, "drt-old");
        let delta = device.expires_at - now;
        assert!(
            (FALLBACK_EXPIRY_DEFAULT_SECS - 5..=FALLBACK_EXPIRY_DEFAULT_SECS + 5).contains(&delta),
            "drt- 家族兜底仍是 30 天，实际 {delta}s"
        );
        // 上游给了 `expires_in` ⇒ 一律以它为准（毫秒），兜底不参与。
        let with_field = serde_json::json!({ "token": "jt-new", "expires_in": 3_600_000 });
        let job = parse_refresh_response(RefreshKind::Job, &with_field, "jrt-old");
        let delta = job.expires_at - now;
        assert!(
            (3_595..=3_605).contains(&delta),
            "`expires_in` 是毫秒，3600000 应折算成 1 小时，实际 {delta}s"
        );
    }

    fn meta() -> ModelMeta {
        ModelMeta {
            key: "ultimate".into(),
            display_name: "Ultimate".into(),
            is_vl: false,
            is_reasoning: true,
            max_input_tokens: 200_000,
        }
    }

    #[test]
    fn base_template_parses_and_is_trimmed() {
        let template = base_template().expect("骨架必须可解析");
        // tools 置空（桌面工具永不随请求发出）。
        assert_eq!(template["tools"], serde_json::json!([]));
        // 只保留 system 一条。
        let messages = template["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"].as_str().unwrap().len() > 1000,
            "官方系统提示词应在"
        );
        // 关键字段在位。
        assert_eq!(template["agent_id"], "agent_common");
        assert_eq!(template["chat_task"], "FREE_INPUT");
        assert_eq!(template["session_type"], "qodercli");
    }

    #[test]
    fn normalize_roles_maps_developer_to_system() {
        let messages = vec![
            serde_json::json!({"role": "developer", "content": "d"}),
            serde_json::json!({"role": "user", "content": "u"}),
        ];
        let out = normalize_roles(Some(&messages));
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[1]["role"], "user");
    }

    #[test]
    fn flatten_content_handles_text_and_images() {
        let (text, images) = flatten_content_text(Some(&serde_json::json!([
            {"type": "text", "text": "看这张"},
            {"type": "image_url", "image_url": {"url": "https://x/1.png"}},
            "plain",
            {"type": "input_image", "data": "AAAA", "mimeType": "image/jpeg"},
        ])));
        assert_eq!(text, "看这张\nplain");
        assert_eq!(images.len(), 2);
        assert_eq!(images[0], "https://x/1.png");
        assert_eq!(images[1], "data:image/jpeg;base64,AAAA");
    }

    /// ★ 带图消息的 content 必须**保留 parts 数组**（上游只读 content 里的
    /// 结构化 `image_url` parts；压成字符串 ⇒ 图像从未到达模型）。
    #[test]
    fn flatten_keeps_multimodal_parts_for_messages_with_images() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "看这张"},
                {"type": "image_url", "image_url": {"url": "https://x/1.png"}},
            ],
        })];
        let flat = flatten_messages(&messages, false, false);
        let content = &flat.messages[0]["content"];
        assert!(
            content.is_array(),
            "带图消息的 content 必须是 parts 数组，实际：{content}"
        );
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "看这张");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "https://x/1.png");
        assert_eq!(flat.images, vec!["https://x/1.png".to_string()]);
    }

    /// ★ `detail` 是**语义**字段（精度档位），归一化只该改形态，不该摘掉它。
    ///
    /// 变异：删掉 `image_detail_of` 的调用 ⇒ 前两条断言红。
    #[test]
    fn flatten_keeps_image_detail_in_both_positions() {
        let flat = flatten_content(Some(&serde_json::json!([
            {"type": "image_url", "image_url": {"url": "https://x/1.png", "detail": "high"}},
            // Responses 的 `input_image`：`detail` 与 `image_url` 平级。
            {"type": "input_image", "image_url": "https://x/2.png", "detail": "low"},
            {"type": "image_url", "image_url": {"url": "https://x/3.png"}},
        ])));
        let parts = flat.parts.expect("有图 ⇒ parts");
        assert_eq!(parts[0]["image_url"]["detail"], "high");
        assert_eq!(parts[1]["image_url"]["detail"], "low");
        assert!(
            parts[2]["image_url"].get("detail").is_none(),
            "客户端没给 detail 就不要编一个：{}",
            parts[2]
        );
    }

    /// 无图消息仍是纯文本（行为不变：上游对纯文本更宽容）。
    #[test]
    fn flatten_keeps_plain_text_for_messages_without_images() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "只有文字"}],
        })];
        let flat = flatten_messages(&messages, false, false);
        assert_eq!(flat.messages[0]["content"], "只有文字");
    }

    /// 单 part 形态（content 直接给一个 part 对象而非列表）：图像必须被收集，
    /// **绝不**被 `str()` 成字面量文本。
    #[test]
    fn flatten_handles_single_part_object_content() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": {"type": "image_url", "image_url": {"url": "https://x/2.png"}},
        })];
        let flat = flatten_messages(&messages, false, false);
        assert_eq!(flat.images, vec!["https://x/2.png".to_string()]);
        let content = &flat.messages[0]["content"];
        assert!(content.is_array(), "单 part 形态也应保留 parts");
        assert_eq!(content[0]["image_url"]["url"], "https://x/2.png");
    }

    /// ★ prompt 提取必须兼容 parts 数组（带图时 content 不再是字符串）。
    #[test]
    fn build_chat_body_prompt_survives_multimodal_content() {
        let payload = serde_json::json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "这是什么颜色"},
                    {"type": "image_url", "image_url": {"url": "https://x/3.png"}},
                ],
            }],
        });
        let body =
            build_chat_body(&payload, &meta(), QoderRegion::Cn, "").expect("请求体必须能构造");
        assert_eq!(body["chat_context"]["text"]["text"], "这是什么颜色");
        // 图片进了 message content 的 parts。
        let messages = body["messages"].as_array().expect("messages 是数组");
        let last = messages.last().expect("至少一条消息");
        assert!(last["content"].is_array());
        assert_eq!(last["content"][1]["image_url"]["url"], "https://x/3.png");
    }

    /// 顶层 `image_urls` / `chat_context.imageUrls` 默认**不填**（上游不读，
    /// 且 base64 双份会让出站体积翻倍）。
    #[test]
    fn build_chat_body_skips_toplevel_image_fields_by_default() {
        let payload = serde_json::json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "看"},
                    {"type": "image_url", "image_url": {"url": "https://x/4.png"}},
                ],
            }],
        });
        let body =
            build_chat_body(&payload, &meta(), QoderRegion::Cn, "").expect("请求体必须能构造");
        // 骨架里本就有 `image_urls: null` 占位 ⇒ 判据是「没有被填成列表」。
        assert!(
            body.get("image_urls")
                .map(|value| value.is_null())
                .unwrap_or(true),
            "顶层 image_urls 默认不填：{:?}",
            body.get("image_urls")
        );
        assert!(
            body["chat_context"]
                .get("imageUrls")
                .map(|value| value.is_null())
                .unwrap_or(true),
            "chat_context.imageUrls 默认不填"
        );
    }

    #[test]
    fn flatten_tool_results_text_and_structured_modes() {
        let messages = vec![serde_json::json!({
            "role": "tool", "tool_call_id": "call_1", "name": "get_weather",
            "content": "晴 25 度",
        })];
        let text_mode = flatten_messages(&messages, false, false);
        assert_eq!(
            text_mode.messages[0]["content"],
            "[工具结果 (get_weather)]\n晴 25 度"
        );
        assert_eq!(text_mode.messages[0]["role"], "user");

        let structured = flatten_messages(&messages, false, true);
        assert_eq!(structured.messages[0]["role"], "tool");
        assert_eq!(structured.messages[0]["tool_call_id"], "call_1");
        // ★ content 是空字符串而非 null（null 会让上游丢掉配对消息）。
        assert_eq!(structured.messages[0]["content"], "晴 25 度");
    }

    #[test]
    fn flatten_assistant_tool_calls_both_modes() {
        let messages = vec![serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_9", "type": "function",
                "function": {"name": "search", "arguments": {"q": "北京"}},
            }],
        })];
        let text_mode = flatten_messages(&messages, false, false);
        let content = text_mode.messages[0]["content"].as_str().unwrap();
        assert!(content.starts_with(LEAK_MARKER), "{content}");
        assert!(content.contains("\"name\":\"search\""), "{content}");
        // 参数被序列化成 JSON 字符串。
        assert!(content.contains("{\\\"q\\\":\\\"北京\\\"}"), "{content}");

        let structured = flatten_messages(&messages, false, true);
        let call = &structured.messages[0]["tool_calls"][0];
        assert_eq!(call["id"], "call_9");
        assert_eq!(call["function"]["arguments"], "{\"q\":\"北京\"}");
    }

    #[test]
    fn normalize_tools_nests_flat_and_drops_unknown() {
        let tools = vec![
            // 平铺 → 嵌套。
            serde_json::json!({"type": "function", "name": "a", "description": "d"}),
            // 已嵌套 → 规整。
            serde_json::json!({"type": "function", "function": {"name": "b", "parameters": {"type": "object"}}}),
            // 未知形态 → 丢弃。
            serde_json::json!({"type": "custom", "name": "c"}),
            // 无名字 → 丢弃。
            serde_json::json!({"type": "function"}),
        ];
        let out = normalize_tools(&tools);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["function"]["name"], "a");
        assert_eq!(out[0]["function"]["parameters"]["type"], "object");
        assert_eq!(out[1]["function"]["name"], "b");
    }

    #[test]
    fn requested_effort_sources_and_canonical_map() {
        assert_eq!(
            extract_requested_effort(&serde_json::json!({"reasoning_effort": "high"})),
            Some("high".into())
        );
        assert_eq!(
            extract_requested_effort(&serde_json::json!({"reasoning": {"effort": "low"}})),
            Some("low".into())
        );
        assert_eq!(
            extract_requested_effort(&serde_json::json!({"thinking": {"level": "max"}})),
            Some("max".into())
        );
        assert_eq!(extract_requested_effort(&serde_json::json!({})), None);
        // 关闭类统一成 none；官方词表外的值丢弃。
        assert_eq!(canonical_effort("off"), Some("none"));
        assert_eq!(canonical_effort("HIGH"), Some("high"));
        assert_eq!(canonical_effort("ultra"), None);
    }

    #[test]
    fn build_chat_body_overwrites_ids_and_keeps_client_system() {
        let payload = serde_json::json!({
            "model": "Ultimate",
            "messages": [
                {"role": "system", "content": "你是测试助手"},
                {"role": "user", "content": "你好"},
            ],
            "max_tokens": 4096,
            "reasoning_effort": "max",
        });
        let body = build_chat_body(
            &payload,
            &meta(),
            QoderRegion::Cn,
            "personal_professional_trial",
        )
        .unwrap();
        assert_eq!(body["stream"], true);
        assert_eq!(body["aliyun_user_type"], "personal_professional_trial");
        // 客户端 system 替换模板 system。
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["content"], "你是测试助手");
        assert_eq!(messages[1]["content"], "你好");
        // id 是 uuid 且 request/chat_record 同源。
        let request_id = body["request_id"].as_str().unwrap();
        assert_eq!(body["chat_record_id"].as_str().unwrap(), request_id);
        assert_ne!(body["session_id"].as_str().unwrap(), request_id);
        assert!(Uuid::parse_str(request_id).is_ok());
        // model_config 来自元数据。
        assert_eq!(body["model_config"]["key"], "ultimate");
        assert_eq!(body["model_config"]["display_name"], "Ultimate");
        assert_eq!(body["model_config"]["is_reasoning"], true);
        assert_eq!(body["model_config"]["max_input_tokens"], 200_000);
        // chat_context 高亮 = 最新用户提示词；business.name 取前 30 字。
        assert_eq!(body["chat_context"]["text"]["text"], "你好");
        assert_eq!(
            body["chat_context"]["extra"]["originalContent"]["text"],
            "你好"
        );
        assert_eq!(body["business"]["name"], "你好");
        // parameters：客户端给的 max_tokens + 归一化档位。
        assert_eq!(body["parameters"]["max_tokens"], 4096);
        assert_eq!(body["parameters"]["reasoning_effort"], "max");
        // tools 置空（客户端没给）。
        assert_eq!(body["tools"], serde_json::json!([]));
    }

    /// ★ max_tokens 出站钳制：上游范围 `[1, MAX_TOKENS_LIMIT]`，超限/零/负
    /// 都会被上游 400（`Range of max_tokens should be [1, 131072]`）。
    ///
    /// 行为级钉子 —— 直接看 [`build_chat_body`] 产物，不只看谓词。
    #[test]
    fn build_chat_body_clamps_max_tokens_into_upstream_range() {
        let meta_ref = meta();
        for (input, expected) in [
            (2_000_000_i64, MAX_TOKENS_LIMIT),
            (MAX_TOKENS_LIMIT + 1, MAX_TOKENS_LIMIT),
            (MAX_TOKENS_LIMIT, MAX_TOKENS_LIMIT),
            (4_096, 4_096),
            (1, 1),
            (0, 1),
            (-5, 1),
        ] {
            let payload = serde_json::json!({
                "model": "ultimate",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": input,
            });
            let body = build_chat_body(&payload, &meta_ref, QoderRegion::Cn, "").unwrap();
            assert_eq!(
                body["parameters"]["max_tokens"], expected,
                "max_tokens={input} 应钳到 {expected}"
            );
        }
        // `max_completion_tokens` 同一条路径（新客户端字段）。
        let payload = serde_json::json!({
            "model": "ultimate",
            "messages": [{"role": "user", "content": "hi"}],
            "max_completion_tokens": 999_999_i64,
        });
        let body = build_chat_body(&payload, &meta_ref, QoderRegion::Cn, "").unwrap();
        assert_eq!(body["parameters"]["max_tokens"], MAX_TOKENS_LIMIT);
    }

    /// ★ issue #72 的**行为级**钉子：DeepSeek 族的工具历史必须进结构化字段，
    /// 请求体里**不能**出现网关的文本化标记。
    ///
    /// 只钉谓词（[`structured_tool_history_enabled`]）不够 —— 那是判据，不是结果。
    /// 这里直接看 [`build_chat_body`] 的产物：`tool_calls` / `role:"tool"` 在，
    /// `[assistant 请求调用工具]` / `[工具结果` 不在。
    ///
    /// 可证伪：把 DeepSeek 分支从 `structured_tool_history_enabled` 删掉 ⇒ 本条红
    /// （两条 `!contains` 会先炸）。
    #[test]
    fn deepseek_tool_history_is_structured_not_text() {
        // 依赖「环境变量未设」的默认 auto 行为 ⇒ 与改环境变量的测试串行。
        let _env = crate::modules::config::env_lock();
        std::env::remove_var("QD_STRUCTURED_TOOL_HISTORY");
        let payload = serde_json::json!({
            "messages": [
                {"role": "user", "content": "看下目录"},
                {"role": "assistant", "content": "", "reasoning_content": "先读文件",
                 "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "read", "arguments": "{\"file_path\":\"a.txt\"}"}
                 }]},
                {"role": "tool", "tool_call_id": "call_1", "name": "read", "content": "hello"},
            ],
        });
        let deepseek = ModelMeta {
            key: "dfmodel".into(),
            display_name: "DeepSeek Flash".into(),
            is_vl: false,
            is_reasoning: true,
            max_input_tokens: 128_000,
        };
        let body = build_chat_body(&payload, &deepseek, QoderRegion::Cn, "").unwrap();
        let messages = body["messages"].as_array().unwrap();
        let raw = serde_json::to_string(messages).unwrap();
        assert!(
            messages
                .iter()
                .any(|message| message["role"] == "tool" && message["tool_call_id"] == "call_1"),
            "工具结果必须保持 `role:\"tool\"` + `tool_call_id`：{raw}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message["tool_calls"].is_array()),
            "助手历史必须保持结构化 `tool_calls`：{raw}"
        );
        assert!(
            !raw.contains(LEAK_MARKER),
            "不得出现文本化调用标记（会诱导模型回声成乱码）：{raw}"
        );
        assert!(
            !raw.contains(TOOL_RESULT_MARKER),
            "工具结果不得退化成 `[工具结果 …]` 文本：{raw}"
        );
        // 同一个族判据的另一半：`reasoning_content` 必须随历史回传
        // （「结构化开、reasoning 不回传」是半开状态，上游可能 400）。
        assert!(
            messages
                .iter()
                .any(|message| message["reasoning_content"] == "先读文件"),
            "DeepSeek 的 reasoning_content 必须回传：{raw}"
        );
    }

    /// ★ 出站清洗（与 WB / Trae 侧 `outbound::cleanup_orphan_tool_calls` **同款语义**）：
    /// 孤儿的 `role:"tool"` 消息删除；含孤儿的 assistant 批次**整批**删 `tool_calls`；
    /// 配对完整的**必须保留**。
    ///
    /// 可证伪：把 `cleanup_orphan_tool_history` 的两趟都注释掉 ⇒ 第二、三条断言红。
    #[test]
    fn orphan_tool_history_is_cleaned_before_sending() {
        let mut messages = vec![
            serde_json::json!({
                "role": "assistant", "content": "第一问",
                "tool_calls": [
                    {"id": "ok", "type": "function", "function": {"name": "a", "arguments": "{}"}},
                ],
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "ok", "content": "结果"}),
            serde_json::json!({
                "role": "assistant", "content": "第二问",
                "tool_calls": [
                    {"id": "orphan", "type": "function", "function": {"name": "b", "arguments": "{}"}},
                ],
            }),
            // 悬空结果：没有对应的 tool_call。
            serde_json::json!({"role": "tool", "tool_call_id": "gone", "content": "残留"}),
        ];
        cleanup_orphan_tool_history(&mut messages);
        let raw = serde_json::to_string(&messages).unwrap();
        assert!(raw.contains("\"ok\""), "配对完整的必须保留：{raw}");
        assert!(!raw.contains("orphan"), "孤儿调用必须被删：{raw}");
        assert!(!raw.contains("\"gone\""), "悬空结果必须被删：{raw}");
        // 批内任一 id 是孤儿 ⇒ 整批删（不是只删那一个）。
        let mut batch = vec![serde_json::json!({
            "role": "assistant", "content": "",
            "tool_calls": [
                {"id": "a1", "type": "function", "function": {"name": "a", "arguments": "{}"}},
                {"id": "a2", "type": "function", "function": {"name": "b", "arguments": "{}"}},
            ],
        })];
        batch.push(serde_json::json!({"role": "tool", "tool_call_id": "a1", "content": "x"}));
        cleanup_orphan_tool_history(&mut batch);
        let raw = serde_json::to_string(&batch).unwrap();
        assert!(!raw.contains("\"a1\""), "含孤儿的批次要整批删：{raw}");
        assert!(!raw.contains("\"a2\""), "含孤儿的批次要整批删：{raw}");
        // 无工具流量 ⇒ 一行不动。
        let mut plain = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let before = serde_json::to_string(&plain).unwrap();
        cleanup_orphan_tool_history(&mut plain);
        assert_eq!(serde_json::to_string(&plain).unwrap(), before);
    }

    /// ★ 结构化请求在**出站前**必须已清干净：孤儿历史不能变成畸形请求发出去。
    ///
    /// 这是「按 WB / Trae 来改」的那一步 —— 清洗，而不是退化成文本模式
    /// （退化成文本模式会把整段历史变成伪格式，模型照抄就是 issue #72 的乱码）。
    #[test]
    fn structured_body_is_cleaned_but_text_mode_is_not() {
        let _env = crate::modules::config::env_lock();
        std::env::remove_var("QD_STRUCTURED_TOOL_HISTORY");
        let payload = serde_json::json!({
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "", "tool_calls": [{
                    "id": "c1", "type": "function",
                    "function": {"name": "read", "arguments": "{}"},
                }]},
                {"role": "tool", "tool_call_id": "c1", "name": "read", "content": "结果"},
                {"role": "assistant", "content": "", "tool_calls": [{
                    "id": "c2", "type": "function",
                    "function": {"name": "read", "arguments": "{}"},
                }]},
                // 悬空结果：对应的 assistant 轮次被裁掉了。
                {"role": "tool", "tool_call_id": "c9", "content": "残留"},
            ],
        });
        let deepseek = ModelMeta {
            key: "dfmodel".into(),
            display_name: "DeepSeek Flash".into(),
            is_vl: false,
            is_reasoning: true,
            max_input_tokens: 128_000,
        };
        // 结构化：清掉悬空结果与孤儿批次，保住配对完整的部分。
        let body = build_chat_body(&payload, &deepseek, QoderRegion::Cn, "").unwrap();
        let raw = serde_json::to_string(body["messages"].as_array().unwrap()).unwrap();
        assert!(raw.contains("\"c1\""), "配对完整的调用必须保留：{raw}");
        assert!(!raw.contains("\"c2\""), "没有回答的调用必须被清掉：{raw}");
        assert!(!raw.contains("c9"), "悬空结果必须被清掉：{raw}");
        // 文本模式：不做清洗（伪格式对畸形配对是宽容的，且保留信息量）。
        // ⚠️ 文本化**不保留** `tool_call_id`（那正是它无法还原调用、只能靠标记回读的原因），
        // 所以断言要看**正文**，不是 id。
        let text = build_chat_body_with_structured(&payload, &deepseek, "", false).unwrap();
        let raw = serde_json::to_string(text["messages"].as_array().unwrap()).unwrap();
        assert!(
            raw.contains("残留"),
            "文本模式不做清洗（保留悬空结果）：{raw}"
        );
    }

    /// 自动降级的**执行面**：显式 `structured = false` 必须真的退成文本模式，
    /// 且 `structured_tool_history_for` 与 `build_chat_body` 同源
    /// （网关拿前者判要不要降级、拿后者决定重发形状 —— 两处判据不同步就会「以为降级了、
    /// 其实又发了一遍结构化」）。
    ///
    /// 可证伪：把 `build_chat_body_with_structured` 里的 `structured`
    /// 改回 `structured_tool_history_enabled(...)` ⇒ 第一条断言红。
    #[test]
    fn explicit_text_override_wins_over_the_model_family() {
        let _env = crate::modules::config::env_lock();
        std::env::remove_var("QD_STRUCTURED_TOOL_HISTORY");
        let payload = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": "", "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "read", "arguments": "{}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1", "name": "read", "content": "hi"},
            ],
        });
        let deepseek = ModelMeta {
            key: "dfmodel".into(),
            display_name: "DeepSeek Flash".into(),
            is_vl: false,
            is_reasoning: true,
            max_input_tokens: 128_000,
        };
        // auto 判据说「结构化」…
        assert!(structured_tool_history_for(
            &payload,
            &deepseek,
            QoderRegion::Cn
        ));
        // …但显式 override 说了算（降级重发走这条）。
        let text = build_chat_body_with_structured(&payload, &deepseek, "", false).unwrap();
        let raw = serde_json::to_string(text["messages"].as_array().unwrap()).unwrap();
        assert!(
            raw.contains(LEAK_MARKER),
            "override 后必须是文本模式：{raw}"
        );
        assert!(
            !raw.contains("\"tool_calls\""),
            "override 后不得再带结构化字段：{raw}"
        );
        // id 不齐备时，强行传 `true` 也不能绕过硬前提（否则「降级」会发畸形请求）。
        let broken = serde_json::json!({"messages": [{"role": "tool", "content": "x"}]});
        let forced = build_chat_body_with_structured(&broken, &deepseek, "", true).unwrap();
        let raw = serde_json::to_string(forced["messages"].as_array().unwrap()).unwrap();
        assert!(
            raw.contains(TOOL_RESULT_MARKER),
            "id 缺失必须回退文本模式：{raw}"
        );
    }

    #[test]
    fn build_chat_body_without_system_keeps_template_prompt() {
        let payload = serde_json::json!({
            "model": "lite",
            "messages": [{"role": "user", "content": "hi"}],
        });
        let body = build_chat_body(&payload, &meta(), QoderRegion::Cn, "").unwrap();
        let messages = body["messages"].as_array().unwrap();
        // 模板 system（官方提示词）被保留。
        assert!(messages[0]["content"].as_str().unwrap().len() > 1000);
        // user_type 空 ⇒ 官方默认。
        assert_eq!(body["aliyun_user_type"], cosy::DEFAULT_USER_TYPE);
        // 客户端没给 max_tokens ⇒ 保留骨架默认 32768。
        assert_eq!(body["parameters"]["max_tokens"], 32768);
    }

    #[test]
    fn envelope_frames_unwrap_correctly() {
        // 正常 chunk。
        let frame = unwrap_envelope_line(
            "data:{\"headers\":{},\"body\":\"{\\\"choices\\\":[{\\\"delta\\\":{\\\"content\\\":\\\"你好\\\"}}]}\",\"statusCodeValue\":200}",
        );
        match frame {
            ChatFrame::Chunk(cleaned) => {
                assert!(cleaned.contains("你好"), "{cleaned}");
            }
            other => panic!("应解出 chunk: {other:?}"),
        }
        // [DONE]。
        assert_eq!(
            unwrap_envelope_line("data:{\"body\":\"[DONE]\"}"),
            ChatFrame::Done
        );
        // 非 200 ⇒ UpstreamError。
        match unwrap_envelope_line("data:{\"body\":\"boom\",\"statusCodeValue\":429}") {
            ChatFrame::UpstreamError { status, detail } => {
                assert_eq!(status, 429);
                assert_eq!(detail, "boom");
            }
            other => panic!("应是错误帧: {other:?}"),
        }
        // 计时元数据 / 注释 / 空行 ⇒ Skip。
        assert_eq!(
            unwrap_envelope_line("event:finish{\"x\":1}"),
            ChatFrame::Skip
        );
        assert_eq!(unwrap_envelope_line(": keep-alive"), ChatFrame::Skip);
        assert_eq!(unwrap_envelope_line(""), ChatFrame::Skip);
    }

    /// ★★ 上游以 **HTTP 200 + 非信封错误帧**收尾时，**不得**静默成空补全。
    ///
    /// 真机取证（2026-10-17，Qoder CN，`agent_chat_generation` 带编码体但**漏了
    /// `?Encode=1`**）：响应共三行 ——
    /// `event:error` / `data:{"stackTrace":[…Jackson JsonParseException…"}` /
    /// `{"success":false,"msgCode":500,…}`，**没有一行**带 `body`/`statusCodeValue`。
    /// 旧实现逐行 Skip ⇒ 客户端拿到「HTTP 200 + 0 token 的空回答」（界面上表现为
    /// 「模型完全不能用」，且没有任何错误线索）。
    ///
    /// 可证伪：删掉 `unwrap_non_envelope_error` 的调用，本用例三行全变 Skip ⇒ 红。
    #[test]
    fn non_envelope_error_frames_are_not_silently_skipped() {
        // ① `event:error` ⇒ 错误帧（不是计时元数据）。
        match unwrap_envelope_line("event:error") {
            ChatFrame::UpstreamError { status, detail } => {
                assert_eq!(status, 502);
                assert!(detail.contains("event:error"), "{detail}");
            }
            other => panic!("event:error 必须是错误帧: {other:?}"),
        }
        // ② `data:` + 堆栈 JSON（无 body / 无 statusCodeValue）⇒ 错误帧，且 detail 取人话字段。
        let stack = "data:{\"stackTrace\":[{\"fileName\":\"JsonParser.java\"}],\"localizedMessage\":\"Unrecognized token 'dwmxdL'\"}";
        match unwrap_envelope_line(stack) {
            ChatFrame::UpstreamError { status, detail } => {
                assert_eq!(status, 502, "没有可用状态字段 ⇒ 502");
                assert!(detail.contains("Unrecognized token"), "{detail}");
            }
            other => panic!("堆栈帧必须是错误帧: {other:?}"),
        }
        // ③ 裸 JSON 信封 `success:false` + `msgCode:500` ⇒ 错误帧，状态取 msgCode。
        let biz = "{\"success\":false,\"traceId\":\"abc\",\"msgCode\":500,\"msgInfo\":\"Internal Server Error\",\"message\":\"Internal Server Error\"}";
        match unwrap_envelope_line(biz) {
            ChatFrame::UpstreamError { status, detail } => {
                assert_eq!(status, 500);
                assert!(detail.contains("Internal Server Error"), "{detail}");
            }
            other => panic!("业务错误信封必须是错误帧: {other:?}"),
        }
    }

    /// 守卫必须**保守**：正常帧与计时元数据一个都不能误判成错误。
    #[test]
    fn normal_and_metadata_frames_never_match_the_error_guard() {
        // 正常 chunk（带 body）仍走原路径。
        assert!(matches!(
            unwrap_envelope_line("data:{\"body\":\"{\\\"choices\\\":[{\\\"delta\\\":{\\\"content\\\":\\\"hi\\\"}}]}\",\"statusCodeValue\":200}"),
            ChatFrame::Chunk(_)
        ));
        assert_eq!(
            unwrap_envelope_line("data:{\"body\":\"[DONE]\"}"),
            ChatFrame::Done
        );
        // `event:finish` 是计时元数据 ⇒ Skip（只有 `event:*error` 才算错误）。
        assert_eq!(
            unwrap_envelope_line("event:finish{\"cost\":12}"),
            ChatFrame::Skip
        );
        // `code:0` / `success:true` / 空 error ⇒ 不是错误。
        assert_eq!(
            unwrap_envelope_line("{\"code\":0,\"data\":{\"ok\":true}}"),
            ChatFrame::Skip
        );
        assert_eq!(
            unwrap_envelope_line("{\"success\":true,\"data\":1}"),
            ChatFrame::Skip
        );
        assert_eq!(
            unwrap_envelope_line("{\"error\":{},\"data\":1}"),
            ChatFrame::Skip
        );
        // `code:200` 是**业务成功**信封（不是「HTTP 200 的上游错误」）—— 2xx 一律不算错误码。
        assert_eq!(
            unwrap_envelope_line("{\"code\":200,\"message\":\"ok\",\"data\":1}"),
            ChatFrame::Skip
        );
        assert_eq!(
            unwrap_envelope_line("{\"msgCode\":\"200\",\"msgInfo\":\"ok\"}"),
            ChatFrame::Skip
        );
        // 非 0 且非 2xx 的业务码（如权益/额度类 `112`）仍然判错，状态沿用业务码。
        match unwrap_envelope_line("{\"code\":112,\"message\":\"no entitlement\"}") {
            ChatFrame::UpstreamError { status, detail } => {
                assert_eq!(status, 112);
                assert!(detail.contains("no entitlement"), "{detail}");
            }
            other => panic!("非 2xx 业务码必须判错: {other:?}"),
        }
        // 普通正文里出现 "error" 字样不误判（这是 delta 帧，带 body）。
        let text = serde_json::json!({"choices": [{"delta": {"content": "the word error here"}}]})
            .to_string();
        let frame = unwrap_envelope_line(&format!(
            "data:{}",
            serde_json::json!({"body": text, "statusCodeValue": 200})
        ));
        assert!(matches!(frame, ChatFrame::Chunk(_)), "{frame:?}");
    }

    #[test]
    fn clean_chunk_strips_noise_and_skips_empty_deltas() {
        // 空噪声字段被剥。
        let cleaned = clean_chunk(
            "{\"choices\":[{\"delta\":{\"content\":\"x\",\"reasoning_content\":\"\",\"refusal\":\"\"}}]}",
        );
        assert!(!cleaned.contains("reasoning_content"), "{cleaned}");
        assert!(!cleaned.contains("refusal"), "{cleaned}");
        assert!(cleaned.contains("\"content\":\"x\""));
        // 整个 delta 空且无 finish_reason ⇒ 空串。
        let empty = clean_chunk("{\"choices\":[{\"delta\":{\"extra_fields\":{}}}]}");
        assert_eq!(empty, "");
        // finish_reason 单独存在 ⇒ 保留。
        let finish = clean_chunk("{\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}");
        assert!(finish.contains("finish_reason"));
    }

    #[test]
    fn leaked_tool_calls_strict_recovery() {
        let text = format!(
            "{LEAK_MARKER}\n[{{\"name\":\"search\",\"arguments\":\"{{\\\"q\\\":\\\"北京\\\"}}\"}}]"
        );
        let (calls, clean) = parse_leaked_tool_calls(&text, None);
        let calls = calls.expect("严格形态必须还原");
        assert_eq!(calls[0]["function"]["name"], "search");
        assert_eq!(clean, "");

        // 尖括号写法（模型复述时会把 `[ ]` 改成 `< >`）同样必须还原。
        let alt = text.replace(LEAK_MARKER, LEAK_MARKER_ALT);
        let (alt_calls, alt_clean) = parse_leaked_tool_calls(&alt, None);
        assert_eq!(
            alt_calls.expect("尖括号标记也要还原")[0]["function"]["name"],
            "search"
        );
        assert_eq!(alt_clean, "");

        // 围栏包裹。
        let fenced = format!("```json\n{text}\n```");
        assert!(parse_leaked_tool_calls(&fenced, None).0.is_some());

        // 声明集合不命中 ⇒ 不还原。
        let allowed: HashSet<String> = ["other".to_string()].into_iter().collect();
        assert!(parse_leaked_tool_calls(&text, Some(&allowed)).0.is_none());

        // 散文 + 标记（非整段）⇒ 不还原（严格守卫）。
        let prose = format!("先说说工具。\n{text}");
        assert!(parse_leaked_tool_calls(&prose, None).0.is_none());
        // 普通正文 ⇒ 不还原。
        assert!(parse_leaked_tool_calls("普通回答", None).0.is_none());
    }

    /// issue #66：截断的回声既还原不了，也**绝不能**透传给终端用户。
    #[test]
    fn truncated_leak_echo_is_droppable_but_valid_prose_is_not() {
        let allowed: HashSet<String> = ["pwsh".to_string()].into_iter().collect();
        // 半截 JSON（上游截断 / 客户端提前收尾）⇒ 吞。
        let truncated = format!("{LEAK_MARKER}\n[{{\"name\":\"pwsh\",\"arguments\":\"{{\\\"comm");
        assert!(leaked_partial_droppable(&truncated, Some(&allowed)));
        assert!(parse_leaked_tool_calls(&truncated, Some(&allowed))
            .0
            .is_none());
        // 只有标记 ⇒ 也是回声。
        assert!(leaked_partial_droppable(LEAK_MARKER, Some(&allowed)));
        // 围栏紧跟着被截断的数组 ⇒ 仍吞。
        let fenced = format!("```json\n{LEAK_MARKER}\n[{{\"name\":\"pwsh\"");
        assert!(leaked_partial_droppable(&fenced, Some(&allowed)));

        // 完整且合法的数组**不吞**（还原失败另有原因，保持既有的 fail-open 透传语义）。
        let valid = format!("{LEAK_MARKER}\n[{{\"name\":\"other\",\"arguments\":\"{{}}\"}}]");
        assert!(!leaked_partial_droppable(&valid, Some(&allowed)));
        // 散文（标记后面不是数组）⇒ 不吞。
        assert!(!leaked_partial_droppable(
            &format!("{LEAK_MARKER}\n下面说说为什么"),
            Some(&allowed)
        ));
        // 未声明 tools ⇒ 不吞（与回读同一前置：避免误杀「讨论该标记」的回复）。
        assert!(!leaked_partial_droppable(&truncated, None));

        // ★ 回声里的名字有**未声明**的 ⇒ 不吞（参考实现 #72：只按「声明了 tools」太宽）。
        let undeclared =
            format!("{LEAK_MARKER}\n[{{\"name\":\"not_declared\",\"arguments\":\"{{\\\"p");
        assert!(!leaked_partial_droppable(&undeclared, Some(&allowed)));
        // 名字全在声明集合里、但数组截断 ⇒ 吞。
        assert!(leaked_partial_droppable(
            &format!("{LEAK_MARKER}\n[{{\"name\":\"pwsh\",\"arguments\":\"{{"),
            Some(&allowed)
        ));
        // 尖括号写法同样认。
        assert!(leaked_partial_droppable(
            &format!("{LEAK_MARKER_ALT}\n[{{\"name\":\"pwsh\""),
            Some(&allowed)
        ));
    }

    /// `echo_names_declared`：全命中才 true；空名字 / 有未声明 / 无 allowed 都 false。
    #[test]
    fn echo_names_must_all_be_declared() {
        let allowed: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        assert!(echo_names_declared(
            r#"[{"name":"a"},{"name":"b"}]"#,
            &allowed
        ));
        assert!(!echo_names_declared(
            r#"[{"name":"a"},{"name":"c"}]"#,
            &allowed
        ));
        assert!(!echo_names_declared("no names here", &allowed));
        assert!(!echo_names_declared(r#"[{"name":"a"}]"#, &HashSet::new()));
    }

    /// issue #66：`[工具结果…]` 回声块同样不能交给用户。
    #[test]
    fn tool_result_echo_is_droppable_only_with_declared_tools() {
        let allowed: HashSet<String> = ["pwsh".to_string()].into_iter().collect();
        let block = format!("{TOOL_RESULT_MARKER} (pwsh)]\n{{\"ok\":true}}");
        assert!(tool_echo_droppable(&block, Some(&allowed)));
        // 自造闭标记 + 散文尾巴 ⇒ 仍吞。
        let closed = format!("{TOOL_RESULT_MARKER} (pwsh)]\n{TOOL_RESULT_CLOSE}\n结果如上");
        assert!(tool_echo_droppable(&closed, Some(&allowed)));
        // 标记后跟自然语言散文 ⇒ 不吞（可能是正常讨论）。
        assert!(!tool_echo_droppable(
            &format!("{TOOL_RESULT_MARKER} 这段是说明文字"),
            Some(&allowed)
        ));
        // 未声明 tools ⇒ 不吞。
        assert!(!tool_echo_droppable(&block, None));
    }

    /// issue #66：模型把调用写成 DSML 标签（客户端 system 教的语法）时必须还原。
    #[test]
    fn dsml_tool_calls_are_recovered_from_text() {
        let text = "先探一下端口。\n<｜｜DSML｜｜ calls>\n<｜｜DSML｜｜ invoke name=\"pwsh\">\n\
                    <｜｜DSML｜｜ parameter name=\"command\">Get-Process</｜｜DSML｜｜ parameter>\n\
                    <｜｜DSML｜｜ parameter name=\"description\">查看进程</｜｜DSML｜｜ parameter>\n\
                    </｜｜DSML｜｜ invoke>\n</｜｜DSML｜｜ calls>";
        let (calls, clean) = parse_dsml_tool_calls(text);
        let calls = calls.expect("完整 DSML 块必须还原");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "pwsh");
        let arguments: Value =
            serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["command"], "Get-Process");
        assert_eq!(arguments["description"], "查看进程");
        assert_eq!(clean, "先探一下端口。", "调用块必须从正文里剥掉");

        // 并行调用。
        let two = "<｜｜DSML｜｜ calls>\
                   <｜｜DSML｜｜ invoke name=\"a\"><｜｜DSML｜｜ parameter name=\"x\">1</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>\
                   <｜｜DSML｜｜ invoke name=\"b\"><｜｜DSML｜｜ parameter name=\"y\">2</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>\
                   </｜｜DSML｜｜ calls>";
        let (calls, clean) = parse_dsml_tool_calls(two);
        let calls = calls.expect("并行调用必须还原");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1]["function"]["name"], "b");
        assert_eq!(clean, "");

        // 没有成对 calls 标签 / 没有 invoke / 没有名字 ⇒ 不认。
        assert!(parse_dsml_tool_calls("<｜｜DSML｜｜ calls> 半截")
            .0
            .is_none());
        assert!(
            parse_dsml_tool_calls("<｜｜DSML｜｜ calls></｜｜DSML｜｜ calls>")
                .0
                .is_none()
        );
        assert!(parse_dsml_tool_calls(
            "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke></｜｜DSML｜｜ invoke></｜｜DSML｜｜ calls>"
        )
        .0
        .is_none());
        // 真机形态：标签名**紧贴**哨兵（`<｜｜DSML｜｜calls>`、中间没有空格）。
        // 可证伪：把 `classify_tag` 的「词尾匹配」改回「整词相等」⇒ 本段拿不到调用。
        let compact = "<｜｜DSML｜｜calls>\n<｜｜DSML｜｜invoke name=\"shell\">\n\
                       <｜｜DSML｜｜parameter name=\"command\">echo hello</｜｜DSML｜｜parameter>\n\
                       </｜｜DSML｜｜invoke>\n</｜｜DSML｜｜calls>";
        let (calls, clean) = parse_dsml_tool_calls(compact);
        let calls = calls.expect("紧凑写法必须还原");
        assert_eq!(calls[0]["function"]["name"], "shell");
        let arguments: Value =
            serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["command"], "echo hello");
        assert_eq!(clean, "");

        // 普通 HTML 标签不误判。
        assert!(parse_dsml_tool_calls("<div class=\"x\">hi</div>")
            .0
            .is_none());

        // 参数值里允许出现裸 `<`（含形如 `a<b>c` 的「假标签」）——不能被截断。
        let tricky = "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"shell\">\
                      <｜｜DSML｜｜ parameter name=\"command\">if (a < b) { echo a<b>c }</｜｜DSML｜｜ parameter>\
                      </｜｜DSML｜｜ invoke></｜｜DSML｜｜ calls>";
        let (calls, _) = parse_dsml_tool_calls(tricky);
        let arguments: Value = serde_json::from_str(
            calls.expect("含裸 `<` 的参数仍应还原")[0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(arguments["command"], "if (a < b) { echo a<b>c }");
    }

    #[test]
    fn model_list_passthrough_with_compat_keys() {
        let payload = serde_json::json!({
            "chat": [
                {"key": "ultimate", "display_name": "Ultimate", "is_reasoning": true, "strategies": [1]},
                {"key": "", "display_name": "缺 key 的行"},
                "not-a-dict",
            ]
        });
        let rows = parse_model_list(&payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], "ultimate");
        assert_eq!(rows[0]["name"], "Ultimate");
        // 原始字段原样透传。
        assert_eq!(rows[0]["strategies"], serde_json::json!([1]));
    }

    #[test]
    fn refresh_response_parsing() {
        // device：token / 缺刷新令牌沿用旧值 / expires_in 毫秒。
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let refresh = parse_refresh_response(
            RefreshKind::Device,
            &serde_json::json!({"token": "dt-new", "expires_in": 86_400_000}),
            "drt-old",
        );
        assert_eq!(refresh.access_token, "dt-new");
        assert_eq!(refresh.refresh_token.as_deref(), Some("drt-old"));
        assert!((refresh.expires_at - (now + 86_400)).abs() <= 5);
        // job：expires_at RFC3339。
        let refresh = parse_refresh_response(
            RefreshKind::Job,
            &serde_json::json!({"token": "jt-new", "expires_at": "2030-01-02T03:04:05Z"}),
            "jrt-old",
        );
        assert_eq!(refresh.expires_at, 1_893_553_445);
    }

    #[test]
    fn checkin_status_parsing() {
        let payload = serde_json::json!({
            "status": "CLAIMED", "lastClaimedAt": 1_789_000_000,
            "currentStreakDays": 3, "totalClaimDays": 12,
            "rewardCredits": 100, "totalRewardCredits": 1200,
            "nextClaimAt": 1_789_086_400, "rewardExpiresAt": 1_790_000_000,
        });
        // 1_789_000_000（UTC+8）= 2026-09-10 左右，用它自己算出来的日期断言。
        let day = chrono::DateTime::from_timestamp(1_789_000_000, 0)
            .unwrap()
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d")
            .to_string();
        let status = parse_checkin_status(&payload, &day);
        assert_eq!(status.status, "CLAIMED");
        assert!(status.active);
        assert!(status.today_checked_in);
        assert_eq!(status.streak_days, 3);
        // 不同「今天」⇒ 不算今日已签。
        let status = parse_checkin_status(&payload, "1999-01-01");
        assert!(!status.today_checked_in);
    }

    #[test]
    fn quota_parsing_and_degenerate_guard() {
        let view = parse_quota(&serde_json::json!({
            "userQuota": {"remaining": 700, "used": 300, "total": 1000},
            "addOnQuota": {"remaining": 50, "used": 10, "total": 60},
        }))
        .unwrap();
        assert_eq!(view.remaining, 750);
        assert_eq!(view.used, 310);
        assert_eq!(view.total, 1060);

        // 两键都缺失 ⇒ 拒绝（不写 0）。
        assert!(parse_quota(&serde_json::json!({"something": 1})).is_err());
        // 退化形态（exceeded 且全 0）⇒ 拒绝。
        assert!(parse_quota(&serde_json::json!({
            "isQuotaExceeded": true,
            "userQuota": {"remaining": 0, "used": 0, "total": 0},
            "addOnQuota": {"remaining": 0, "used": 0, "total": 0},
        }))
        .is_err());
        // 真·耗尽（size>0 而 remain=0）⇒ 放行（0 是真实余额）。
        let exhausted = parse_quota(&serde_json::json!({
            "isQuotaExceeded": true,
            "userQuota": {"remaining": 0, "used": 1000, "total": 1000},
            "addOnQuota": {"remaining": 0, "used": 0, "total": 0},
        }))
        .unwrap();
        assert_eq!(exhausted.remaining, 0);
    }

    /// ★★ 上游把额度写成**浮点** ⇒ 必须解析成真实数值，不能变成 0。
    ///
    /// 这不是构造出来的边界，是**真机取证**（2026-10-15，本机 Qoder CN 账号）：
    /// `GET /api/v2/quota/usage` 实际返回
    /// `{"addOnQuota":{"remaining":99.0,"total":100.0,"used":1.0,"unit":"credits"}}`。
    /// 而 `Value::as_i64()` 对 `99.0` 返回 `None` ⇒ 旧实现把它解析成 `0 / 0 / 0`
    /// ⇒ 界面上「积分 0 / 0」，用户以为号已用完，**而它还有 99** —— 谎报。
    ///
    /// 可证伪：把 `parse_quota` 的 `as_num` 换回 `Value::as_i64`，本用例立即变红
    /// （`left: 0, right: 99`）。
    #[test]
    fn quota_accepts_the_floating_point_shape_the_upstream_actually_returns() {
        let view = parse_quota(&serde_json::json!({
            "addOnQuota": {
                "detailUrl": "https://qoder.com.cn/account/usage",
                "percentage": 0.02,
                "remaining": 99.0,
                "total": 100.0,
                "unit": "credits",
                "used": 1.0,
            },
            "isQuotaExceeded": false,
            "usageType": "credits",
            "userQuota": {"percentage": 0.0, "remaining": 0.0, "total": 0.0, "used": 0.0},
        }))
        .unwrap();
        assert_eq!(view.remaining, 99, "浮点余额必须读出来，不能被当成 0");
        assert_eq!(view.total, 100);
        assert_eq!(view.used, 1);

        // 字符串形态也认（有些上游会把数字序列化成字符串）。
        let as_text = parse_quota(&serde_json::json!({
            "userQuota": {"remaining": "12", "used": "3.0", "total": "15"},
        }))
        .unwrap();
        assert_eq!(
            (as_text.remaining, as_text.used, as_text.total),
            (12, 3, 15)
        );
    }

    #[test]
    fn openapi_headers_carry_auth_and_machine_ids() {
        let headers = openapi_headers(QoderRegion::Cn, "q1", "dt-abc");
        let lookup = |name: &str| {
            headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(lookup("Authorization").as_deref(), Some("Bearer dt-abc"));
        assert_eq!(
            lookup("X-Machine-ID").as_deref(),
            Some("abec85ce8c7ed7ebd0041ee4ca63e701")
        );
        assert!(lookup("X-Request-ID")
            .unwrap()
            .starts_with(&super::super::device::derive_id("q1", "req")));
        assert!(lookup("Origin").unwrap().contains("qoder"));
    }

    #[test]
    fn structured_tool_history_rules() {
        // 本测试会改进程级环境变量 ⇒ 必须与另一条同样读它的测试串行（同 crate 的
        // `config::env_lock`，理由见那里的注释）。
        let _env = crate::modules::config::env_lock();
        let good = vec![
            serde_json::json!({"role": "assistant", "tool_calls": [{"id": "c1"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "content": "x"}),
        ];
        let bad_ids = vec![serde_json::json!({"role": "tool", "content": "x"})];
        // 默认 auto：非 CN ⇒ false；缺 id ⇒ false。
        assert!(!structured_tool_history_enabled(
            "qwen-max",
            QoderRegion::Global,
            &good
        ));
        assert!(!structured_tool_history_enabled(
            "qwen-max",
            QoderRegion::Cn,
            &bad_ids
        ));
        /* ★ issue #72：DeepSeek 族在 CN 出口**默认走结构化**（auto 模式下，未设环境变量）。
        文本模式下 DeepSeek Flash 会照抄 `[assistant 请求调用工具]` 伪格式 + 混进 DSML
        语法 ⇒ 客户端看到「上下文乱码」、工具一次不执行。反向断言（旧行为）是缺陷。 */
        for key in ["dmodel", "dfmodel"] {
            assert!(
                structured_tool_history_enabled(key, QoderRegion::Cn, &good),
                "`{key}` 必须在 CN 出口启用结构化工具历史（issue #72）"
            );
            assert!(
                !structured_tool_history_enabled(key, QoderRegion::Global, &good),
                "`{key}` 是 CN key，国际版出口不得启用"
            );
        }
        // 带后缀的变体同一判据（前缀匹配，见 `is_deepseek_key`）。
        assert!(structured_tool_history_enabled(
            "dfmodel_pro",
            QoderRegion::Cn,
            &good
        ));
        // id 不齐备时，DeepSeek 也照样回退文本化（fail-safe 优先于族判据）。
        assert!(!structured_tool_history_enabled(
            "dfmodel",
            QoderRegion::Cn,
            &bad_ids
        ));
        std::env::set_var("QD_STRUCTURED_TOOL_HISTORY", "on");
        assert!(structured_tool_history_enabled(
            "whatever",
            QoderRegion::Global,
            &good
        ));
        std::env::set_var("QD_STRUCTURED_TOOL_HISTORY", "off");
        assert!(!structured_tool_history_enabled(
            "qwen-max",
            QoderRegion::Cn,
            &good
        ));
        // `off` 是逃生阀：DeepSeek 也必须被关掉（上游若改了契约，用户能自救）。
        assert!(!structured_tool_history_enabled(
            "dfmodel",
            QoderRegion::Cn,
            &good
        ));
        std::env::remove_var("QD_STRUCTURED_TOOL_HISTORY");
        // 默认 auto：CN + 白名单前缀 ⇒ true。
        assert!(structured_tool_history_enabled(
            "qwen3.8-max",
            QoderRegion::Cn,
            &good
        ));
    }

    /// ★ 真实模型 key 必须命中白名单（issue #46 的 B 项的回归钉子）。
    ///
    /// 背景：白名单原本只有 `qwen`/`glm`/`gm4`/`gm5`，而国内版（CN）Qwen 族的 key 是
    /// `q37fmodel`（本仓 `docs/specs/QODER-PLATFORM.md` §5 在册）⇒ **不命中** ⇒ CN 全量
    /// 模型退化成文本模式 ⇒ 工具历史被序列化进正文、模型回声后泄漏到用户界面（A 项）。
    ///
    /// ## 取证来源（不猜）
    ///
    /// - `q37fmodel` / `gm51model`：**本仓文档在册**（`docs/specs/QODER-PLATFORM.md` §5）；
    /// - `qfmodel` / `qmodel*` / `gmodel` / `gfmodel`：issue #46 作者在**真实账号**的 CN
    ///   模型清单里取到的 key 形态（作者 2026-10-18 实测）；本机无 Qoder 账号，
    ///   **未二次复核** —— 真账号复核前不要往这张表里加**没在任一来源出现过**的 key。
    /// - `dmodel` / `dfmodel`：issue #72 报障者真机（DeepSeek Flash + dsh）取证，
    ///   2026-10-21 由「反向排除」改成「默认启用」。
    ///
    /// 可证伪：把常量改回旧名单（4 项）⇒ 第一条断言红；把 DeepSeek 分支删掉 ⇒
    /// ③ 那两条红。
    #[test]
    fn real_qoder_model_keys_hit_the_structured_tool_history_whitelist() {
        // 与 `structured_tool_history_rules` 串行：那条会写进程级环境变量，
        // 并发读到 `on` 会把下面的反向断言全部掩盖成假绿。
        let _env = crate::modules::config::env_lock();
        let good = vec![
            serde_json::json!({"role": "assistant", "tool_calls": [{"id": "c1"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "content": "x"}),
        ];
        // ① 本仓文档在册的两个 key（硬契约）。
        for key in ["q37fmodel", "gm51model"] {
            assert!(
                structured_tool_history_enabled(key, QoderRegion::Cn, &good),
                "`{key}` 必须命中白名单（否则退化成文本模式 → 泄漏回声）"
            );
        }
        // ② 作者真机清单里的族前缀（qwen / glm 族）。
        for key in ["qmodel", "qfmodel", "gmodel", "gfmodel"] {
            assert!(
                structured_tool_history_enabled(key, QoderRegion::Cn, &good),
                "`{key}` 应命中（issue #46 真机清单）"
            );
        }
        // ③ DeepSeek 族（issue #72）：必须启用，且只在 CN。
        for key in ["dmodel", "dfmodel", "deepseek-v4"] {
            assert!(
                structured_tool_history_enabled(key, QoderRegion::Cn, &good),
                "`{key}` 必须启用结构化工具历史（issue #72：文本模式会泄漏伪格式）"
            );
            assert!(!structured_tool_history_enabled(
                key,
                QoderRegion::Global,
                &good
            ));
        }
        // 反向：无依据的族**不能**被捞进来（Kimi / MiniMax 不在本仓 CN 清单；
        // `smodel` / `cmodel` 是**国际版**的 key）。
        for key in ["kimodel", "mmmodel", "smodel", "cmodel"] {
            assert!(
                !structured_tool_history_enabled(key, QoderRegion::Cn, &good),
                "`{key}` 不该命中 CN 结构化工具历史白名单"
            );
        }
        /* ⚠️ region 判据**必须留在白名单之前**（`structured_tool_history_enabled` 开头）：
        国际版出口即便 key 命中也不启用结构化工具历史。删掉这一句 ⇒ 下面这条红。 */
        assert!(!structured_tool_history_enabled(
            "gm51model",
            QoderRegion::Global,
            &good
        ));
    }

    // ── 真机探针（无账号无法验证，刻意保留位） ──

    fn live_account() -> (String, String) {
        (
            std::env::var("QODER_LIVE_UID").expect("需 QODER_LIVE_UID"),
            std::env::var("QODER_LIVE_ACCESS_TOKEN").expect("需 QODER_LIVE_ACCESS_TOKEN"),
        )
    }

    #[tokio::test]
    #[ignore = "真机探针：需 QODER_LIVE_UID / QODER_LIVE_ACCESS_TOKEN（本机无账号）"]
    async fn live_model_list() {
        let (uid, token) = live_account();
        let input = CosyAccountInput {
            uid: &uid,
            nickname: "",
            access_token: &token,
            refresh_token: "",
            user_type: None,
            org_id: None,
            org_name: None,
        };
        let models = fetch_model_list(QoderRegion::Cn, &input).await.unwrap();
        assert!(!models.is_empty());
    }

    #[tokio::test]
    #[ignore = "真机探针：需 QODER_LIVE_UID / QODER_LIVE_ACCESS_TOKEN（本机无账号）"]
    async fn live_quota_and_checkin() {
        let (uid, token) = live_account();
        let quota = quota_usage(QoderRegion::Cn, &uid, &token).await.unwrap();
        println!("remaining={} total={}", quota.remaining, quota.total);
        let _ = checkin_status(QoderRegion::Cn, &uid, &token).await;
    }
    // ---------------------------------------------------------------------
    // 历史工具结果削减（参考实现 `_shrink_tool_result`）
    // ---------------------------------------------------------------------

    #[test]
    fn tool_keep_parses_env_values() {
        assert_eq!(parse_tool_keep(None), TOOL_RESULT_KEEP_DEFAULT);
        assert_eq!(parse_tool_keep(Some("")), TOOL_RESULT_KEEP_DEFAULT);
        assert_eq!(parse_tool_keep(Some(" 500 ")), 500);
        assert_eq!(parse_tool_keep(Some("off")), 0);
        assert_eq!(parse_tool_keep(Some("0")), 0);
        assert_eq!(parse_tool_keep(Some("DISABLE")), 0);
        assert_eq!(parse_tool_keep(Some("abc")), TOOL_RESULT_KEEP_DEFAULT);
    }

    #[test]
    fn shrink_keeps_head_and_tail_with_a_fixed_marker() {
        let body = "A".repeat(5000) + "\n" + &"B".repeat(5000);
        let shrunk = shrink_tool_result(&body, 2000);
        assert!(shrunk.len() < body.len(), "必须真的变短");
        assert!(shrunk.starts_with("AAAA"), "头部要保留");
        assert!(shrunk.ends_with("BBBB"), "尾部要保留");
        assert!(shrunk.contains("已省略中间内容"), "必须有省略标记");
        // 标记是固定串（不带字数）—— 同一份内容两次削出来必须逐字相同。
        assert_eq!(shrunk, shrink_tool_result(&body, 2000));

        // 本来就不长 ⇒ 原样。
        let short = "x".repeat(100);
        assert_eq!(shrink_tool_result(&short, 2000), short);
        // keep=0 ⇒ 关闭。
        assert_eq!(shrink_tool_result(&body, 0), body);
    }

    #[test]
    fn shrink_spares_complete_json_within_double_budget() {
        // 完整 JSON 在 4*keep 预算内不削（截断即失去可解析性）。
        let payload = serde_json::json!({"log": "x".repeat(4000)});
        let text = payload.to_string();
        assert!(
            text.len() > 2000 && text.len() < 8000,
            "前置：长度落在双倍预算内"
        );
        assert_eq!(shrink_tool_result(&text, 2000), text);
    }

    /// 削减只作用于**更早的**工具结果：最后一条（当前轮那条）必须原样保留。
    #[test]
    fn flatten_protects_the_last_tool_result() {
        let big = "L".repeat(6000);
        let messages = vec![
            serde_json::json!({"role": "user", "content": "开始"}),
            serde_json::json!({"role": "assistant", "content": Value::Null, "tool_calls": []}),
            serde_json::json!({"role": "tool", "tool_call_id": "t1", "content": big}),
            serde_json::json!({"role": "assistant", "content": Value::Null, "tool_calls": []}),
            serde_json::json!({"role": "tool", "tool_call_id": "t2", "content": big}),
        ];
        let flat = flatten_messages(&messages, false, true);
        let tool_texts: Vec<&str> = flat
            .messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .filter_map(|message| message["content"].as_str())
            .collect();
        assert_eq!(tool_texts.len(), 2);
        assert!(
            tool_texts[0].contains("已省略中间内容"),
            "更早的工具结果应被削减"
        );
        assert_eq!(tool_texts[1], big, "最后一条（当前轮）必须原样保留");
    }
}
