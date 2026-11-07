//! Trae 客户端**模型清单**（issue #4）：读客户端 `state.vscdb` 里**上游下发**的清单。
//!
//! ## 为什么读客户端缓存，而不是自己请求上游
//!
//! Trae 的模型清单由**服务端下发**：客户端 `ModelService` 通过 native RPC
//! （`method = "model_list_by_function"`）拉取，再落盘到 `state.vscdb` 的
//! `<uid><分隔符>AI.agent.model.model_list_map`。上游**没有**「外部凭 token 就能列模型」
//! 的简单 HTTP 端点 —— 那条路要复刻 native 的 `wrapRequestWithCredential`
//! （service 名 / method 名 / 凭据包装 / 可能的签名），成本与不确定性都高一个量级。
//!
//! 因此本模块复用客户端**已经完成**的刷新结果：
//!
//! - 零网络、零鉴权、零逆向；
//! - 新鲜度 = 客户端最后一次刷新（启动 / 权益变化 / 清单卡片上的「重试」）。
//!
//! ## 键名形状：**不解析分隔符**
//!
//! 键是 `<uid><分隔符>AI.agent.model.model_list_map`，而分隔符**按产品线不同**
//! （本机实测：Trae Work = `:`、TraeCode CN = `_`）。本模块一律用
//! `LIKE '%<后缀>'` 匹配、**不硬编码分隔符** —— 这样未实测的国际版变体同样成立，
//! 也不会因为猜错分隔符而**静默**读不到（读不到会被误判成「客户端没拉过清单」）。
//!
//! ## 目录与 uid **同源**
//!
//! 目录走 [`icube::login_state_dir_for`]、uid 走 [`profile::client_login_uid_for`]，
//! 二者都落在「客户端此刻真正在用的那个 userData 目录」上。读不到登录态目录时再按
//! 活跃度补试其余候选目录（**只读**，不建目录）。
//!
//! ## 逐字段容错
//!
//! 上游字段会随版本增删，且**任何脏值都可能让前端整棵树卸载**（本仓踩过：展示字段
//! 未归一时 React 白屏）。故本模块不用 `#[derive(Deserialize)]` 严格解析，而是一个
//! 字段一个字段地取，取不到就回落成 `null` / 空串 / `false`。

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::variant::TraeVariant;
use crate::modules::config::now_ms;

/// 客户端 storage 里模型清单的键后缀（客户端 `ModelService` 的 storage key 常量）。
pub const MODEL_LIST_KEY_SUFFIX: &str = "AI.agent.model.model_list_map";

/// 读库时的等待窗口（客户端可能正在写同一个库，见 [`read_key_from_db`]）。
const BUSY_TIMEOUT_MS: u64 = 1_000;

/// 读到了客户端缓存。
pub const SOURCE_CLIENT_CACHE: &str = "client-cache";
/// 没读到（未启动过 / 未登录 / 还没拉过清单）。
pub const SOURCE_MISSING: &str = "missing";

/// 单个模型条目。
///
/// 只保留界面需要、且能**逐字段容错**的字段；`name` 之外的字段一律有回落值。
///
/// 序列化成 camelCase —— 与 Trae 分区既有的前端类型（`TraeEnvStatus` 等）同风格，
/// 前端不需要为这一处单独做字段名转换。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientModel {
    /// 模型标识（上游 `name`，如 `deepseek-v4.1-flash`）。空名条目会被丢弃。
    pub name: String,
    /// 展示名（上游 `display_name`）；取不到时回落 `name`。
    pub display_name: String,
    /// `reasoning_model` / `chat_model` …；取不到为 `""`。
    pub model_type: String,
    pub multimodal: bool,
    pub is_default: bool,
    /// 是否 TRAE 官方内置（`is_preset`）。
    pub is_preset: bool,
    pub is_new: bool,
    pub is_beta: bool,
    /// 是否**第三方 / 自定义路由**条目（上游 `provider` 非空，或 `custom_model_id` 有值）。
    ///
    /// 网关发给上游的是 `config_name` / `model_name` 这套字段，对这类条目**无效**：
    /// 本机实测 `openrouter//stealth/ox-alpha`（`provider = "openrouter"`，
    /// `custom_model_id = "2445846658"`）在 `solo_work_lite` 下被上游
    /// `4001 param is invalid` 拒（见 `buddy-switch-gateway` 的 `probe_work_list_names`）。
    ///
    /// ⇒ 网关侧 [`buddy_switch_gateway::trae::payload`] 的对外清单会**过滤掉**它们；
    /// 界面按「在不在对外清单里」逐个打「网关不提供」标记，两处口径一致。
    pub is_bypass: bool,
    /// 默认上下文窗口（`context_window_size.default`），取不到为 `None`。
    pub context_window: Option<i64>,
    /// 上限上下文窗口（`context_window_size.max`），取不到为 `None`。
    ///
    /// 上游给 Max Mode 声明的上限（如 `1000000`）；实测客户端缓存里**常为 `null`**，
    /// 因此消费方（Trae 网关的 `/v1/models`）必须有兜底，不能只依赖本字段。
    pub context_window_max: Option<i64>,
    /// 单次回复上限（`prompt_max_tokens`），取不到为 `None`。
    pub prompt_max_tokens: Option<i64>,
    /// 积分倍率（`features.consumption_rate.data.rate`，如 `0.2` = 基准价的 0.2 倍）。
    ///
    /// ★ issue #65：上游一直下发这个字段，此前解析时被丢弃 ⇒ 界面查「这个模型多少积分」
    /// 只能回客户端看。`None` = 该条目没下发（实测 `assistant` 分组整组都没有）。
    pub credits_rate: Option<f64>,
    /// 支持的思考档位（`reasoning_effort_config.options`，保序照抄上游词表）。
    ///
    /// ★ 空数组有**两种成因**，界面文案必须区分（见 `efforts_supported`）：
    /// - `support_thinking == false`：该模型确实不支持选档；
    /// - 整个 `reasoning_effort_config` 缺失：这条程序位**没下发**档位信息。
    ///
    /// 混为一谈会让 TraeWork 侧（实测 `solo_work_lite` 22 条里只有 1 条带档位）看着像
    /// 「全都不支持思考」，而真相是「这个程序位的清单里没这项能力」。
    pub efforts: Vec<String>,
    /// 默认档位（`reasoning_effort_config.default_level`）；取不到为 `None`。
    pub default_effort: Option<String>,
    /// 上游对「思考档位」的声明状态（见 [`ThinkingSupport`]）。
    ///
    /// ★ 必须与 `efforts` 分开放：`efforts` 为空有**两种**成因（明确不支持 / 根本没下发），
    /// 混为一谈会让 TraeWork 侧（实测 `solo_work_lite` 22 条只有 1 条带档位）看着像
    /// 「全都不支持思考」，而真相是「这条程序位的清单里没这项能力」。
    pub thinking: ThinkingSupport,
}

/// 思考档位的声明状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingSupport {
    /// 上游声明支持，可选档位见 `efforts`（实测全非空）。
    Supported,
    /// 上游明确不支持（`support_thinking: false` 且没有 options）。
    Unsupported,
    /// 客户端清单里**没有**这项字段 —— 不是「不支持」，是「不知道」，界面必须显示未下发。
    Unknown,
}

/// 一个 function 分组（`solo_work_lite` / `solo_coder` / `builder` …）。
///
/// 分组名**按产品线不同**（Trae Work 与 TraeCode 的取值集合完全不同），
/// 故本模块原样透出、不做映射 —— 由调用方决定展示哪几组。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientModelGroup {
    pub function: String,
    pub models: Vec<ClientModel>,
}

/// 读取结果（界面直接消费，**不抛错**：读不到是正常状态）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientModelList {
    /// 变体标识（[`TraeVariant::as_str`]）。
    pub variant: String,
    /// 变体展示名（[`TraeVariant::display_name`]）。
    pub variant_label: String,
    /// [`SOURCE_CLIENT_CACHE`] 或 [`SOURCE_MISSING`]。
    pub source: String,
    /// 本次读取时间（毫秒时间戳）。
    pub read_at: i64,
    /// 读到的 `state.vscdb` 所在 userData 目录。
    pub data_dir: Option<String>,
    /// 命中的缓存键里的 uid（`<uid><分隔符>…` 的前缀）。
    pub uid: Option<String>,
    pub groups: Vec<ClientModelGroup>,
    /// 读不到时说明原因；正常时也可能带「检测到多份缓存」这类提示。
    pub note: Option<String>,
}

impl ClientModelList {
    /// 读不到时的统一出口（`source = missing`）。
    fn missing(variant: TraeVariant, data_dir: Option<PathBuf>, note: impl Into<String>) -> Self {
        Self {
            variant: variant.as_str().to_string(),
            variant_label: variant.display_name().to_string(),
            source: SOURCE_MISSING.to_string(),
            read_at: now_ms(),
            data_dir: data_dir.map(|dir| dir.to_string_lossy().to_string()),
            uid: None,
            groups: Vec::new(),
            note: Some(note.into()),
        }
    }

    /// 直连上游的来源出口（issue #173）：`source = upstream`。
    ///
    /// **分组名固定 `chat_v3`**：直连路径只请求了这一个 function（实测它已含界面需要的
    /// 全部可见模型），而上游响应里没有「本地客户端那套程序位分组」。编一个看起来像
    /// `solo_work_lite` 的分组名就是造假 —— 界面按原样显示这个上游 function 名。
    fn from_upstream(
        variant: TraeVariant,
        models: Vec<ClientModel>,
        read_at: i64,
        note: String,
    ) -> Self {
        Self {
            variant: variant.as_str().to_string(),
            variant_label: variant.display_name().to_string(),
            source: super::model_list_upstream::SOURCE_UPSTREAM.to_string(),
            read_at,
            data_dir: None,
            uid: None,
            groups: vec![ClientModelGroup {
                function: "chat_v3".to_string(),
                models,
            }],
            note: Some(note),
        }
    }
}

/// 该变体「客户端此刻在用的」userData 目录候选（去重，**只读、不建目录**）。
///
/// 顺序：先「装着登录态的那个目录」（与 uid 同源），再按活跃度补上其余存在候选。
/// 后者是兜底：登录态信封读不出来（例如客户端换了账号体系）时，清单本身可能还在。
fn candidate_dirs(variant: TraeVariant) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = super::icube::login_state_dir_for(variant) {
        dirs.push(dir);
    }
    for dir in super::platform::data_dirs_by_activity_for(variant) {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// `state.vscdb` 在该 userData 目录下的候选相对路径（按优先级）。
fn state_db_candidates(root: &Path) -> [PathBuf; 2] {
    [
        root.join("User").join("globalStorage").join("state.vscdb"),
        root.join("globalStorage").join("state.vscdb"),
    ]
}

/// 读取该变体的客户端模型清单（**永不失败**，读不到时 `source = missing`）。
///
/// ## 三档来源（优先级即顺序）
///
/// 1. **直连上游缓存**（issue #173，`source = upstream`）：新鲜期内优先 —— 它是用户
///    **显式点过刷新**才有的，比客户端缓存更新；
/// 2. **客户端 `state.vscdb`**（`source = client-cache`）：随客户端启动/刷新更新；
/// 3. **过期的直连缓存**：前两者都没有时的兜底（headless 部署的常见形态），
///    文案里必须写明「抓取于何时」，否则用户会以为是当天的。
pub fn read_client_model_list(variant: TraeVariant) -> ClientModelList {
    let now = now_ms();
    if let Some(models) = super::model_list_upstream::fresh_models(variant, now) {
        return ClientModelList::from_upstream(
            variant,
            models,
            now,
            "清单来自直连上游（不是客户端缓存）".to_string(),
        );
    }

    let dirs = candidate_dirs(variant);
    if dirs.is_empty() {
        // headless 部署的典型形态：没有客户端目录，但用户点过一次「从上游拉取」。
        if let Some(list) = stale_upstream_models(variant, now) {
            return list;
        }
        return ClientModelList::missing(
            variant,
            None,
            format!(
                "未找到【{}】的客户端数据目录：请先启动一次该客户端",
                variant.display_name()
            ),
        );
    }

    // uid 只用于**在多份缓存之间选对那一份**；拿不到不影响读取（见下方 note）。
    let prefer_uid = super::profile::client_login_uid_for(variant);

    let mut last_dir: Option<PathBuf> = None;
    // 单个目录读失败（库损坏 / 权限 / 锁）**不该立刻放弃** —— 别的候选目录可能有能用的库。
    let mut last_error: Option<(PathBuf, String)> = None;
    for dir in &dirs {
        if !state_db_candidates(dir)
            .into_iter()
            .any(|path| path.is_file())
        {
            continue;
        }
        last_dir = Some(dir.clone());
        match read_dir_model_list(dir, variant, prefer_uid.as_deref()) {
            Ok(Some(list)) => return list,
            // 该目录没有清单（键不存在 / 解析不出分组）⇒ 继续试下一个候选。
            Ok(None) => continue,
            Err(error) => {
                last_error = Some((dir.clone(), error));
                continue;
            }
        }
    }

    // 全试完了：有「真的读失败」就报它，否则报「还没缓存」—— 后者是可操作的那条。
    // ★ issue #173：两条出口都先看一眼**过期的**直连缓存 —— 比「什么都没有」有用，
    //   但文案必须写明抓取时间（把陈旧数据说成当天的是谎报）。
    if let Some(list) = stale_upstream_models(variant, now) {
        return list;
    }
    if let Some((dir, error)) = last_error {
        return ClientModelList::missing(
            variant,
            Some(dir),
            format!("读取客户端模型清单失败：{error}"),
        );
    }
    ClientModelList::missing(
        variant,
        last_dir,
        format!(
            "【{}】客户端还没有模型清单缓存：启动一次客户端并登录后即可读取",
            variant.display_name()
        ),
    )
}

/// 过期的直连缓存 → 清单（带「抓取于 N 小时前」的说明）；没有缓存 ⇒ `None`。
fn stale_upstream_models(variant: TraeVariant, now: i64) -> Option<ClientModelList> {
    let (fetched_at, models) = super::model_list_upstream::stale_models(variant)?;
    let hours = now.saturating_sub(fetched_at) / 3_600_000;
    let age = if hours <= 0 {
        "不到 1 小时前".to_string()
    } else {
        format!("约 {hours} 小时前")
    };
    Some(ClientModelList::from_upstream(
        variant,
        models,
        now,
        format!("客户端缓存不可用，展示的是 {age}从上游直连抓取的清单"),
    ))
}

/// 从**指定** userData 目录读清单。
///
/// `Ok(None)` 表示「这个目录里没有可用清单」（没库 / 没键 / 键解析不出分组）——
/// 与 `Err` 的区别是前者属于正常状态（继续试下一个候选目录），后者是真故障。
///
/// 单独抽出来是为了让单测能直接喂一个临时目录，**不必改进程环境变量**
/// （那正是本仓最容易写出假绿的地方，见 `test_support` 的模块文档）。
fn read_dir_model_list(
    dir: &Path,
    variant: TraeVariant,
    prefer_uid: Option<&str>,
) -> Result<Option<ClientModelList>, String> {
    let Some(db) = state_db_candidates(dir)
        .into_iter()
        .find(|path| path.is_file())
    else {
        return Ok(None);
    };
    let Some(hit) = read_key_from_db(&db, prefer_uid)? else {
        return Ok(None);
    };
    let groups = parse_groups(&hit.value);
    if groups.is_empty() {
        return Ok(None);
    }

    let note = if hit.key_count > 1 {
        Some(format!(
            "客户端里存有 {} 份模型清单缓存，展示的是 uid={} 那一份",
            hit.key_count,
            hit.uid.as_deref().unwrap_or("未知")
        ))
    } else {
        None
    };

    Ok(Some(ClientModelList {
        variant: variant.as_str().to_string(),
        variant_label: variant.display_name().to_string(),
        source: SOURCE_CLIENT_CACHE.to_string(),
        read_at: now_ms(),
        data_dir: Some(dir.to_string_lossy().to_string()),
        uid: hit.uid,
        groups,
        note,
    }))
}

/// 一次命中的结果。
struct KeyHit {
    /// 键名里的 uid 前缀（解析不出为 `None`）。
    uid: Option<String>,
    /// 命中的键总数（> 1 表示客户端里存了多份缓存）。
    key_count: usize,
    value: String,
}

/// 从 `state.vscdb` 读模型清单键。
///
/// ## 为什么用 `substr` 而不是 `LIKE`
///
/// 键后缀是 `AI.agent.model.model_list_map` —— 里面的 `_` 在 **`LIKE` 里是通配符**
/// （匹配任意单字符），于是 `LIKE '%AI.agent.model.model_list_map'` 也会命中
/// `AI.agent.model.modelXlistYmap` 这类键，进而由 [`uid_from_key`] 解析出**错误的 uid**。
/// `substr(key, -length(?1)) = ?1` 是**字面**比较，没有模式语义 —— 这才是「以该后缀结尾」。
///
/// 多条命中时优先 `prefer_uid` 对应的那条，否则取第一条。
fn read_key_from_db(db: &Path, prefer_uid: Option<&str>) -> Result<Option<KeyHit>, String> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| format!("打开 state.vscdb 失败: {error}"))?;

    // 客户端**可能正在写**这个库（它自己的模型列表刷新），此时读会撞上写锁并立刻
    // 返回 `SQLITE_BUSY`。给一个等待窗口，别把「正好撞上」当成「读不到」。
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))
        .map_err(|error| format!("设置 busy_timeout 失败: {error}"))?;

    let mut statement = conn
        .prepare("SELECT key, value FROM ItemTable WHERE substr(key, -length(?1)) = ?1")
        .map_err(|error| format!("查询模型清单失败: {error}"))?;
    let rows = statement
        .query_map([MODEL_LIST_KEY_SUFFIX], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("查询模型清单失败: {error}"))?;

    let mut hits: Vec<(String, String)> = Vec::new();
    for row in rows {
        match row {
            Ok(pair) => hits.push(pair),
            // 单行读失败（例如 value 不是 TEXT）不致命：跳过即可，别让一条脏行毁掉整次读取。
            Err(_) => continue,
        }
    }
    if hits.is_empty() {
        return Ok(None);
    }

    let key_count = hits.len();
    let chosen = match prefer_uid {
        Some(uid) => hits
            .iter()
            .find(|(key, _)| key_belongs_to_uid(key, uid))
            .or_else(|| hits.first()),
        None => hits.first(),
    };
    let Some((key, value)) = chosen else {
        return Ok(None);
    };

    Ok(Some(KeyHit {
        uid: uid_from_key(key),
        key_count,
        value: value.clone(),
    }))
}

/// 该键是否属于指定 uid。
///
/// 必须是 `<uid>` **加上一个分隔符**才算命中 —— 只用 `starts_with(uid)` 会让
/// uid `123` 命中 `1234:…`，在多份缓存之间**静默选错**。
fn key_belongs_to_uid(key: &str, uid: &str) -> bool {
    key.strip_prefix(uid)
        .is_some_and(|rest| rest.starts_with(':') || rest.starts_with('_'))
}

/// 从 `<uid><分隔符>AI.agent.model.model_list_map` 里取 uid 前缀。
///
/// 分隔符本身不固定（`:` / `_`），故**按后缀反推**：截掉后缀再剥掉尾部一个分隔符字符。
fn uid_from_key(key: &str) -> Option<String> {
    let prefix = key.strip_suffix(MODEL_LIST_KEY_SUFFIX)?;
    let trimmed = prefix
        .strip_suffix(':')
        .or_else(|| prefix.strip_suffix('_'))
        .unwrap_or(prefix);
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 把 `{ function: [model, …] }` 解析成分组列表。
///
/// 分组按 function 名字典序输出（`serde_json::Map` 的默认序），**不猜上游顺序** ——
/// 展示顺序由调用方决定。空分组与空名条目一律丢弃。
fn parse_groups(raw: &str) -> Vec<ClientModelGroup> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(map) = value.as_object() else {
        return Vec::new();
    };

    let mut groups = Vec::new();
    for (function, models) in map {
        let Some(list) = models.as_array() else {
            continue;
        };
        let models: Vec<ClientModel> = list.iter().filter_map(parse_model).collect();
        if models.is_empty() {
            continue;
        }
        groups.push(ClientModelGroup {
            function: function.clone(),
            models,
        });
    }
    groups
}

/// 解析单个模型条目；`name` 为空时返回 `None`（丢弃）。
fn parse_model(value: &Value) -> Option<ClientModel> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())?
        .to_string();
    let display_name = value
        .get("display_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| name.clone());

    let (efforts, default_effort, thinking) = parse_thinking(value.get("reasoning_effort_config"));

    Some(ClientModel {
        name,
        display_name,
        model_type: string_field(value, "model_type"),
        multimodal: bool_field(value, "multimodal"),
        is_default: bool_field(value, "is_default"),
        is_preset: bool_field(value, "is_preset"),
        is_new: bool_field(value, "is_new"),
        is_beta: bool_field(value, "is_beta"),
        is_bypass: !string_field(value, "provider").is_empty()
            || value
                .get("custom_model_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.trim().is_empty()),
        context_window: value
            .get("context_window_size")
            .and_then(|size| size.get("default"))
            .and_then(as_i64),
        context_window_max: value
            .get("context_window_size")
            .and_then(|size| size.get("max"))
            .and_then(as_i64),
        prompt_max_tokens: value.get("prompt_max_tokens").and_then(as_i64),
        // ★ issue #65：这两项一直随清单下发，此前被丢弃 ⇒ 界面只能显示名字与窗口。
        //   实测（2026-10-21 Trae CN `state.vscdb`）：
        //   `features.consumption_rate.data.rate` = 0.06～1.83；`assistant` 分组整组没有。
        credits_rate: value
            .get("features")
            .and_then(|features| features.get("consumption_rate"))
            .and_then(|rate| rate.get("data"))
            .and_then(|data| data.get("rate"))
            .and_then(as_f64),
        efforts,
        default_effort,
        thinking,
    })
}

/// 解析 `reasoning_effort_config` → `(可选档位, 默认档, 声明状态)`。
///
/// 三种实测形态（2026-10-21）：
/// - `{"support_thinking": true, "options": ["light","high"], "default_level": "high"}`
/// - `{"support_thinking": false}` —— 明确不支持
/// - 整个字段缺失 —— **未下发**（不等于不支持，见 [`ThinkingSupport`]）
fn parse_thinking(raw: Option<&Value>) -> (Vec<String>, Option<String>, ThinkingSupport) {
    let Some(object) = raw.and_then(Value::as_object) else {
        return (Vec::new(), None, ThinkingSupport::Unknown);
    };
    let efforts: Vec<String> = object
        .get("options")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|effort| !effort.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    // 默认档必须在可选集里（上游若给出集外值，展示它就是误导）。
    let default_effort = object
        .get("default_level")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
        .map(str::to_string)
        .filter(|effort| efforts.iter().any(|item| item == effort));
    let thinking = if !efforts.is_empty() {
        ThinkingSupport::Supported
    } else if object.get("support_thinking").and_then(Value::as_bool) == Some(false) {
        ThinkingSupport::Unsupported
    } else {
        // 声明了「支持」却没给选项 ⇒ 等于没下发，别编档位。
        ThinkingSupport::Unknown
    };
    (efforts, default_effort, thinking)
}

/// 取字符串字段；类型不对或缺失一律回落空串（**绝不让脏值传到展示层**）。
fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

/// 取布尔字段；非布尔一律 `false`。
fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// 取整数字段；接受数字与「数字字符串」，其余为 `None`。
fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// 取浮点字段；接受数字与「数字字符串」（上游个别地方把数字塞进字符串），其余为 `None`。
///
/// ★ 用 `as_f64` 而不是 `as_i64`：`consumption_rate` 是 `0.06` / `1.83` 这类小数，
/// 走整数解析会**静默变 None**（`as_i64` 只认整数），于是整列全是「—」。
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64().filter(|rate| rate.is_finite()),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|rate| rate.is_finite()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个临时 userData 目录，内含只带 `ItemTable` 的最小 `state.vscdb`。
    ///
    /// 刻意**不改进程环境变量**：直接喂目录给 [`read_dir_model_list`]，
    /// 因此这些用例可以并行跑，也不会与别的用例抢 `env_lock`。
    fn temp_user_data(entries: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-model-list-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let db = root.join("User").join("globalStorage").join("state.vscdb");
        std::fs::create_dir_all(db.parent().unwrap()).expect("建 globalStorage");
        let conn = rusqlite::Connection::open(&db).expect("建库");
        conn.execute(
            "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .expect("建表");
        for (key, value) in entries {
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                rusqlite::params![key, value],
            )
            .expect("写键");
        }
        root
    }

    fn cleanup(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
    }

    /// ★ 护栏：键名分隔符**按产品线分家**（Trae Work `:` / TraeCode `_`），两者都要认。
    ///
    /// 这条是「不硬编码分隔符」的落地断言：写死任一个都会让另一半产品线**静默**读不到。
    #[test]
    fn uid_from_key_accepts_both_product_line_separators() {
        assert_eq!(
            uid_from_key("3604620555324748:AI.agent.model.model_list_map").as_deref(),
            Some("3604620555324748"),
            "Trae Work 用冒号分隔"
        );
        assert_eq!(
            uid_from_key("3604620555324748_AI.agent.model.model_list_map").as_deref(),
            Some("3604620555324748"),
            "TraeCode 用下划线分隔"
        );
        // 只有后缀、没有 uid ⇒ 无 uid（而不是把整个键当 uid）。
        assert_eq!(uid_from_key(MODEL_LIST_KEY_SUFFIX), None);
        // 不是本键 ⇒ 不认。
        assert_eq!(uid_from_key("3604:AI.agent.model.other"), None);
    }

    /// ★ 护栏：uid 匹配必须**带上分隔符**，否则 `123` 会命中 `1234:…`。
    #[test]
    fn key_belongs_to_uid_requires_a_separator_boundary() {
        let key = "1234:AI.agent.model.model_list_map";
        assert!(key_belongs_to_uid(key, "1234"));
        assert!(!key_belongs_to_uid(key, "123"), "前缀相同但不是同一个 uid");
        assert!(
            !key_belongs_to_uid(key, "1234:AI"),
            "uid 之后必须紧跟分隔符"
        );
    }

    /// 端到端：两种键名形状都能从真库读出来。
    #[test]
    fn reads_entries_for_both_key_shapes() {
        let payload = r#"{"solo_work_lite":[{"name":"deepseek-v4.1-flash","display_name":"DeepSeek-V4.1-Flash","is_default":true,"multimodal":true,"is_preset":true,"context_window_size":{"max":null,"default":256000}}]}"#;

        for separator in [':', '_'] {
            let key = format!("7000000000000001{separator}{MODEL_LIST_KEY_SUFFIX}");
            let root = temp_user_data(&[(key.as_str(), payload)]);
            let list = read_dir_model_list(&root, TraeVariant::TraeWork, None)
                .expect("读库不该失败")
                .expect("应当读到清单");

            assert_eq!(list.source, SOURCE_CLIENT_CACHE);
            assert_eq!(list.variant, "trae_work");
            assert_eq!(list.uid.as_deref(), Some("7000000000000001"));
            assert_eq!(list.groups.len(), 1);
            assert_eq!(list.groups[0].function, "solo_work_lite");
            let model = &list.groups[0].models[0];
            assert_eq!(model.name, "deepseek-v4.1-flash");
            assert_eq!(model.display_name, "DeepSeek-V4.1-Flash");
            assert_eq!(model.context_window, Some(256000));
            assert!(model.is_default);
            assert!(model.is_preset);

            cleanup(&root);
        }
    }

    /// ★ 护栏：键匹配必须是**字面后缀**，不能被 `_` 的通配符语义骗到。
    ///
    /// 历史实现是 `LIKE '%<后缀>'`，而后缀里的 `_`（`model_list_map`）在 LIKE 里是
    /// **通配符** ⇒ `AI.agent.model.modelXlistYmap` 也会被命中，再由 [`uid_from_key`]
    /// 解析出一个**错误的 uid**。改用 `substr(key, -length(?1)) = ?1` 后不再有模式语义。
    #[test]
    fn key_matching_is_literal_not_pattern() {
        let payload = r#"{"g":[{"name":"real"}]}"#;
        let decoy = r#"{"g":[{"name":"decoy"}]}"#;
        let real_key = format!("u1:{MODEL_LIST_KEY_SUFFIX}");
        // 只把后缀里的两个 `_` 换成别的字符 —— 对 LIKE 而言与真键「等价」。
        let decoy_key = "u2:AI.agent.model.modelXlistYmap";
        let root = temp_user_data(&[(real_key.as_str(), payload), (decoy_key, decoy)]);

        let list = read_dir_model_list(&root, TraeVariant::TraeWork, None)
            .expect("读库不该失败")
            .expect("应当读到清单");
        assert_eq!(
            list.groups[0].models[0].name, "real",
            "只应命中字面后缀的那个键"
        );
        assert_eq!(list.uid.as_deref(), Some("u1"));
        // ★ 真正有鉴别力的断言：**命中条数**。
        // 只断言「第一条是真键」是假护栏 —— LIKE 会把 decoy 一并算进来，
        // 而 decoy 恰好排在后面，第一条仍是真的（变异验证时实测到过这一点）。
        assert!(
            list.note.is_none(),
            "只应命中一个字面后缀的键；LIKE 会把 decoy 也算进来，于是 note 提示「存有 2 份缓存」"
        );

        cleanup(&root);
    }

    /// 多份缓存（切过账号）时按 `prefer_uid` 精确选，并带出条数提示。
    #[test]
    fn prefer_uid_selects_the_matching_cache() {
        let payload_a = r#"{"solo_work_lite":[{"name":"model-a"}]}"#;
        let payload_b = r#"{"solo_work_lite":[{"name":"model-b"}]}"#;
        let key_a = format!("7000000000000001:{MODEL_LIST_KEY_SUFFIX}");
        let key_b = format!("7000000000000002:{MODEL_LIST_KEY_SUFFIX}");
        let root = temp_user_data(&[(key_a.as_str(), payload_a), (key_b.as_str(), payload_b)]);

        let list = read_dir_model_list(&root, TraeVariant::TraeWork, Some("7000000000000002"))
            .expect("读库不该失败")
            .expect("应当读到清单");
        assert_eq!(list.groups[0].models[0].name, "model-b");
        assert_eq!(list.uid.as_deref(), Some("7000000000000002"));
        assert!(
            list.note.is_some(),
            "多份缓存必须给出提示，不能装作只有一份"
        );

        cleanup(&root);
    }

    /// 「读不到」是**正常状态**（`Ok(None)`），不是错误 —— 上层据此继续试别的候选目录。
    #[test]
    fn absent_key_or_db_yields_none_not_error() {
        // 库在、但没有本键。
        let root = temp_user_data(&[("aha.account", "{}")]);
        assert!(read_dir_model_list(&root, TraeVariant::TraeWork, None)
            .expect("不该报错")
            .is_none());
        cleanup(&root);

        // 库都不在。
        let empty = std::env::temp_dir().join(format!(
            "buddy-switch-model-list-empty-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&empty).expect("建临时目录");
        assert!(read_dir_model_list(&empty, TraeVariant::TraeWork, None)
            .expect("不该报错")
            .is_none());
        cleanup(&empty);
    }

    /// 键在、但内容解析不出分组 ⇒ 同样算「没有清单」（宁可显示空态，也不显示半截脏数据）。
    #[test]
    fn unparsable_value_yields_none() {
        let key = format!("7000000000000001:{MODEL_LIST_KEY_SUFFIX}");
        for raw in ["not json", "[]", "{}", r#"{"solo_work_lite":[]}"#] {
            let root = temp_user_data(&[(key.as_str(), raw)]);
            assert!(
                read_dir_model_list(&root, TraeVariant::TraeWork, None)
                    .expect("不该报错")
                    .is_none(),
                "内容 {raw:?} 应当被视为「没有清单」"
            );
            cleanup(&root);
        }
    }

    /// ★★ 护栏：**第三方 / 自定义路由**条目必须被标出来（`provider` 非空或 `custom_model_id` 有值）。
    ///
    /// 网关发给上游的是 `config_name` / `model_name` 这套字段，对这类条目**无效** ——
    /// 实测 `openrouter//stealth/ox-alpha`（`provider = "openrouter"`）在 `solo_work_lite`
    /// 下被上游 `4001` 拒。漏标 ⇒ 它会进对外清单，成为又一个「看得见、调不动」。
    #[test]
    fn parse_model_marks_third_party_routes_as_bypass() {
        let raw = r#"{
            "g": [
                {"name":"openrouter//stealth/ox-alpha","provider":"openrouter","custom_model_id":"2445846658"},
                {"name":"custom-with-id","provider":"","custom_model_id":" 42 "},
                {"name":"blank-provider","provider":"   ","custom_model_id":null},
                {"name":"normal","provider":"","custom_model_id":null}
            ]
        }"#;
        let groups = parse_groups(raw);
        let models = &groups[0].models;
        assert!(models[0].is_bypass, "provider 非空 ⇒ bypass");
        assert!(
            models[1].is_bypass,
            "custom_model_id 非空（含空白）⇒ bypass"
        );
        assert!(!models[2].is_bypass, "provider 全空白 ⇒ 不是 bypass");
        assert!(!models[3].is_bypass, "两者都空 ⇒ 正常条目");
    }

    /// ★ 护栏：脏值一律归一，**绝不让非字符串传到展示层**（本仓踩过 React 白屏）。
    #[test]
    fn parse_groups_normalizes_dirty_entries() {
        let raw = r#"{
            "solo_work_lite": [
                {"name":"good","display_name":123,"is_default":"yes","is_beta":null,
                 "context_window_size":{"default":"1024"},"prompt_max_tokens":"2048"},
                {"name":"   "},
                {"display_name":"no-name"}
            ],
            "broken": "not-an-array",
            "empty": []
        }"#;
        let groups = parse_groups(raw);
        assert_eq!(groups.len(), 1, "只有 solo_work_lite 有有效条目");

        let model = &groups[0].models[0];
        assert_eq!(model.name, "good");
        assert_eq!(
            model.display_name, "good",
            "非字符串 display_name 必须回落 name"
        );
        assert!(!model.is_default, "非布尔 is_default 必须回落 false");
        assert!(!model.is_beta);
        assert_eq!(model.model_type, "", "缺失的字符串字段回落空串");
        assert_eq!(model.context_window, Some(1024), "数字字符串要能解析");
        assert_eq!(model.prompt_max_tokens, Some(2048));

        assert!(parse_groups("not json").is_empty());
        assert!(parse_groups("[1,2,3]").is_empty(), "顶层不是对象 ⇒ 空结果");
    }

    /// ★ issue #65：思考档位 + 积分倍率必须从客户端清单里读出来。
    ///
    /// 三种「档位」形态要能区分（`supported` / `unsupported` / `unknown`）——
    /// 把 `unknown` 混成 `unsupported` 会让 TraeWork 侧（实测 22 条只有 1 条带档位）
    /// 看着像「全都不支持思考」，而真相是这条程序位没下发。
    ///
    /// 变异：`parse_thinking` 的缺失分支返回 `Unsupported` ⇒ 第一条断言红；
    /// 用 `as_i64` 读 `rate` ⇒ `0.2` 变 `None` ⇒ 第二条红。
    #[test]
    fn parse_model_reads_thinking_and_consumption_rate() {
        let raw = r#"{
            "solo_agent": [
                {"name":"with-thinking","reasoning_effort_config":
                    {"support_thinking":true,"options":["light","high"],"default_level":"high"},
                 "features":{"consumption_rate":{"enable":true,"data":{"rate":0.2}}}},
                {"name":"no-thinking","reasoning_effort_config":{"support_thinking":false},
                 "features":{"consumption_rate":{"enable":true,"data":{"rate":0.06}}}},
                {"name":"no-config","features":{"consumption_rate":{"enable":true,"data":{"rate":"0.83"}}}},
                {"name":"no-features"}
            ]
        }"#;
        let groups = parse_groups(raw);
        let models = &groups[0].models;

        assert_eq!(models[0].efforts, vec!["light", "high"]);
        assert_eq!(models[0].default_effort.as_deref(), Some("high"));
        assert_eq!(models[0].thinking, ThinkingSupport::Supported);
        assert_eq!(models[0].credits_rate, Some(0.2));

        assert!(models[1].efforts.is_empty());
        assert!(models[1].default_effort.is_none());
        assert_eq!(
            models[1].thinking,
            ThinkingSupport::Unsupported,
            "support_thinking:false ⇒ 明确不支持"
        );
        assert_eq!(models[1].credits_rate, Some(0.06));

        assert_eq!(
            models[2].thinking,
            ThinkingSupport::Unknown,
            "整块缺失 ⇒ 未下发（不是「不支持」）"
        );
        assert_eq!(models[2].credits_rate, Some(0.83), "数字字符串倍率也要认");

        assert_eq!(models[3].thinking, ThinkingSupport::Unknown);
        assert_eq!(models[3].credits_rate, None, "没下发就是 None，不编");
    }

    /// 默认档不在可选集里 ⇒ 丢弃（否则界面会显示一个传了就被降级的档位）。
    #[test]
    fn parse_model_drops_default_level_outside_options() {
        let raw = r#"{"g":[{"name":"m","reasoning_effort_config":
            {"support_thinking":true,"options":["low"],"default_level":"max"}}]}"#;
        let groups = parse_groups(raw);
        assert!(groups[0].models[0].default_effort.is_none());
    }
}
