//! 模型目录：内置兜底目录（CN / Global 两份）+ 目录缓存 store。
//!
//! 对照参考实现 `catalog.ts` + `catalog-store.ts`。
//!
//! 降级链：本次实时（Live）→ 上次成功缓存（Cached）→ 内置兜底（Builtin），
//! 由 [`CatalogSource`] 标注来源。两版的目录缓存**按 region 隔离**（同一模型 id
//! 在两版可能倍率/窗口都不同），落盘到 `~/.buddy-switch/gateway_models.<region>.json`。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde_json::Value;

use crate::modules::config::{now_ms, store_dir};
use crate::modules::net::{self, TransportErrorKind};
use crate::modules::region::{catalog_cache_file, Region};
use crate::modules::upstream::{parse_model_catalog, UpstreamClient, UpstreamErrorKind, UpstreamFailure};

/// 目录来源标注（P0-4 / L1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogSource {
    /// 本次实时拉取成功。
    Live,
    /// 上次成功缓存的目录。
    Cached,
    /// 内置兜底目录。
    Builtin,
}

/// 一个可用的模型条目。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogModel {
    /// 模型 id。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 上下文窗口。
    pub context_window: u64,
    /// 最大输出 token。
    pub max_tokens: u64,
    /// 是否支持图片输入。
    pub supports_images: bool,
    /// 积分倍率字符串（如 `"x0.79"`），缺省为 None。
    pub credits: Option<String>,
    /// 促销徽标。
    pub badges: Vec<String>,
    /// 当前是否免费。
    pub free: bool,
}

/// 一次目录快照。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogSnapshot {
    /// 所属 region。
    pub region: Region,
    /// 来源。
    pub source: CatalogSource,
    /// 拉取成功时间（毫秒）；Builtin 为 None。
    pub fetched_at: Option<i64>,
    /// 模型列表。
    pub models: Vec<CatalogModel>,
    /// 降级说明（如「上游接口可能已变更」）。
    pub note: Option<String>,
}

/// 落盘格式。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CachedCatalog {
    region: Region,
    fetched_at: i64,
    models: Vec<CatalogModel>,
}

/// ★ 契约（P10 §1.3 规则 2）：**非 live 快照的 `note` 恒非空**。
///
/// 三个常量就是「为什么不是实时」的**后端**落点 —— 前端 `describeCatalogSource` 另有
/// i18n 兜底，二者独立：后端给不出具体原因时前端仍能兜底，后端给得出时原因更具体。
///
/// ⚠️ 所有非 live 快照（`cached` / `builtin`）都**必须**经 [`with_reason`] 产出。
/// 改回 `note: None` 会让来源条退回「只有一个灰徽标，用户分不清是『加载中』还是『拉不到』」
/// （这正是 `load_cached` 此前把 `note` 置 `None` 留下的缺口）。
const CACHED_NOTE: &str = "展示的是上次成功识别的缓存目录（本次未能连上上游）";
/// 「内置」来源的兜底原因（从未成功识别过上游）。
const BUILTIN_NOTE: &str = "尚未成功识别过上游目录，展示的是内置兜底目录";
/// 上游返回 200 但清单为空 —— 空清单不可用，不能写 live。
const EMPTY_CATALOG_NOTE: &str = "上游未返回任何模型（清单为空）";

/// 目录「过期」阈值：24 小时（P10 §1.6 触发表）。
///
/// `fetched_at` 超过它 ⇒ 网关启动时后台补识别一次。
pub const STALE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// ★ 不变量出口：把快照标成**非 live** 来源时，**必须**带上原因。
///
/// 这是「非实时必有原因」的后端唯一落点 —— 直接拼 `note: None` 会让契约静默失效。
fn with_reason(mut snapshot: CatalogSnapshot, reason: &str) -> CatalogSnapshot {
    debug_assert!(
        !reason.trim().is_empty(),
        "非 live 快照的原因不得为空（否则用户只剩一个灰徽标可猜）"
    );
    snapshot.note = Some(reason.to_string());
    snapshot
}

/// 把上游失败映射为**可读、具体**的原因（✅ P10 #5：超时 / 网络 / 鉴权…）。
///
/// 取代此前笼统的 `DEGRADE_NOTE`（「上游接口可能已变更…」）—— 那句话回答不了
/// 「**为什么**这次不是实时」，用户看到它只能猜。粒度：
///
/// - **传输层失败**（`status == 0`）按 [`net`] 的分类细化：连接超时 / 无法连接 / 读体失败；
/// - **有状态码的失败**按 [`UpstreamErrorKind`]：鉴权 / 额度 / 限流 / 未找到 / 服务端 / 客户端。
pub fn failure_reason(failure: &UpstreamFailure) -> String {
    if failure.status == 0 {
        // `net::describe_transport_error` 的文案以「种类：」开头，据此细化传输层原因。
        let message = &failure.message;
        return if message.contains(net::transport_error_label(TransportErrorKind::Timeout)) {
            "上游未响应（连接超时）".to_string()
        } else if message.contains(net::transport_error_label(TransportErrorKind::Connect)) {
            "网络错误（无法连接上游）".to_string()
        } else if message.contains(net::transport_error_label(TransportErrorKind::BodyOrDecode)) {
            "网络错误（上游响应读取失败）".to_string()
        } else {
            "网络错误（请求失败）".to_string()
        };
    }
    match failure.kind {
        UpstreamErrorKind::HardCredit => "鉴权 / 额度失败（积分不足）".to_string(),
        UpstreamErrorKind::SoftRate => "上游限流（请求过于频繁）".to_string(),
        UpstreamErrorKind::SessionDead => "鉴权失败（登录态已失效）".to_string(),
        UpstreamErrorKind::NotFound => "上游接口未找到（接口可能已变更）".to_string(),
        UpstreamErrorKind::Server => "上游服务端错误".to_string(),
        UpstreamErrorKind::Client => "上游拒绝了请求".to_string(),
    }
}

/// 快照是否「待确认」（P10 §3.4 / §1.6）：**从未成功识别**（`fetched_at == None`，即内置），
/// 或 `fetched_at` 距 `now_ms` 已超过 [`STALE_AFTER_MS`]。
///
/// 供「网关启动补识别」判定哪些 region 需要后台探测 —— 避免每次启动都无脑打上游。
pub fn is_stale(snapshot: &CatalogSnapshot, now_ms: i64) -> bool {
    match snapshot.fetched_at {
        None => true,
        Some(fetched_at) => now_ms.saturating_sub(fetched_at) >= STALE_AFTER_MS,
    }
}

/// CN 内置兜底目录（对照参考实现 `FALLBACK_WORKBUDDY_MODELS`，16 条）。
///
/// 说明：条目含 `String` / `Vec` 字段，无法在 `const` 上下文中构造（会触发 E0015），
/// 故改用 [`LazyLock`] 惰性构造；对外仍以 `&'static [CatalogModel]` 只读切片暴露，
/// 数据与原先逐字节一致。
pub static FALLBACK_CN_MODELS: LazyLock<Vec<CatalogModel>> = LazyLock::new(|| {
    vec![
        CatalogModel { id: "auto".into(), name: "Auto".into(), context_window: 168_000, max_tokens: 32_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "hy4-preview".into(), name: "Hy4 preview".into(), context_window: 1_000_000, max_tokens: 64_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "hy3".into(), name: "Hy3".into(), context_window: 192_000, max_tokens: 64_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "hy3-x".into(), name: "Hy3".into(), context_window: 192_000, max_tokens: 64_000, supports_images: true, credits: Some("x0.05".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "deepseek-v4.1-flash".into(), name: "Deepseek-V4.1-Flash".into(), context_window: 1_000_000, max_tokens: 128_000, supports_images: true, credits: Some("x0.03 credits".into()), badges: vec!["独家优惠".into()], free: false },
        CatalogModel { id: "glm-5.3".into(), name: "GLM-5.3".into(), context_window: 1_000_000, max_tokens: 48_000, supports_images: true, credits: Some("x0.79".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "glm-5.3-flash".into(), name: "GLM-5.3-Flash".into(), context_window: 1_000_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.06".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "glm-5.2".into(), name: "GLM-5.2".into(), context_window: 1_000_000, max_tokens: 48_000, supports_images: true, credits: Some("x0.79 credits".into()), badges: vec!["夜间折扣".into()], free: false },
        CatalogModel { id: "glm-5.1".into(), name: "GLM-5.1".into(), context_window: 200_000, max_tokens: 48_000, supports_images: false, credits: Some("x0.79 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "glm-5v-turbo".into(), name: "GLM-5v-Turbo".into(), context_window: 200_000, max_tokens: 64_000, supports_images: true, credits: Some("x0.71 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "kimi-k3-1".into(), name: "Kimi-K3".into(), context_window: 1_000_000, max_tokens: 32_000, supports_images: true, credits: Some("x1.62 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "kimi-k2.8-preview".into(), name: "Kimi-K2.8-Preview".into(), context_window: 1_000_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.77 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "kimi-k2.7".into(), name: "Kimi-K2.7-Code".into(), context_window: 256_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.57 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "kimi-k2.6".into(), name: "Kimi-K2.6".into(), context_window: 256_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.52 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "minimax-m3".into(), name: "MiniMax-M3".into(), context_window: 512_000, max_tokens: 128_000, supports_images: true, credits: Some("x0.25 credits".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "deepseek-v4-pro".into(), name: "Deepseek-V4-Pro".into(), context_window: 1_000_000, max_tokens: 50_000, supports_images: true, credits: Some("x0.51 credits".into()), badges: Vec::new(), free: false },
    ]
});

/// Global 内置兜底目录（对照参考实现 `FALLBACK_WORKBUDDY_AI_MODELS`，20 条）。
///
/// 同 CN：条目含 `String` / `Vec` 字段，改用 [`LazyLock`] 惰性构造。
pub static FALLBACK_GLOBAL_MODELS: LazyLock<Vec<CatalogModel>> = LazyLock::new(|| {
    vec![
        CatalogModel { id: "default-model".into(), name: "Auto".into(), context_window: 176_000, max_tokens: 24_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "fast-model".into(), name: "Fast".into(), context_window: 200_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.34".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "balanced-model".into(), name: "Balanced".into(), context_window: 256_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.59".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "primary-model".into(), name: "Primary".into(), context_window: 272_000, max_tokens: 72_000, supports_images: true, credits: Some("x3.31".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "deep-model".into(), name: "Deep".into(), context_window: 176_000, max_tokens: 24_000, supports_images: true, credits: Some("x3.33".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "hy4-preview-f".into(), name: "Hy4 preview".into(), context_window: 300_000, max_tokens: 64_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "hy3".into(), name: "Hy3".into(), context_window: 192_000, max_tokens: 64_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "deepseek-v4.1-flash".into(), name: "Deepseek-V4.1-Flash".into(), context_window: 300_000, max_tokens: 128_000, supports_images: true, credits: None, badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-6-astra".into(), name: "GPT-6-Astra".into(), context_window: 400_000, max_tokens: 128_000, supports_images: true, credits: Some("x6.67".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-5.6-sol".into(), name: "GPT-5.6-Sol".into(), context_window: 1_000_000, max_tokens: 128_000, supports_images: true, credits: Some("x3.47".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-5.6-terra".into(), name: "GPT-5.6-Terra".into(), context_window: 1_000_000, max_tokens: 128_000, supports_images: true, credits: Some("x1.39".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-5.6-luna".into(), name: "GPT-5.6-Luna".into(), context_window: 1_000_000, max_tokens: 128_000, supports_images: true, credits: Some("x0.14".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-5.5".into(), name: "GPT-5.5".into(), context_window: 1_000_000, max_tokens: 128_000, supports_images: true, credits: Some("x3.31".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-5.4".into(), name: "GPT-5.4".into(), context_window: 272_000, max_tokens: 72_000, supports_images: true, credits: Some("x1.65".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gpt-5.3-codex".into(), name: "GPT-5.3-Codex".into(), context_window: 272_000, max_tokens: 72_000, supports_images: true, credits: Some("x1.25".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "gemini-3.5-flash".into(), name: "Gemini-3.5-Flash".into(), context_window: 1_000_000, max_tokens: 65_536, supports_images: true, credits: Some("x0.99".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "glm-5.3".into(), name: "GLM-5.3".into(), context_window: 1_000_000, max_tokens: 48_000, supports_images: true, credits: Some("x0.79".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "glm-5.2".into(), name: "GLM-5.2".into(), context_window: 1_000_000, max_tokens: 48_000, supports_images: true, credits: Some("x0.79".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "kimi-k3".into(), name: "Kimi-K3".into(), context_window: 1_000_000, max_tokens: 32_000, supports_images: true, credits: Some("x1.62".into()), badges: Vec::new(), free: false },
        CatalogModel { id: "kimi-k2.6".into(), name: "Kimi-K2.6".into(), context_window: 256_000, max_tokens: 32_000, supports_images: true, credits: Some("x0.52".into()), badges: Vec::new(), free: false },
    ]
});

/// 目录缓存 store：每 region 一个快照 + 落盘。
pub struct CatalogStore {
    cache_dir: PathBuf,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// 本次进程内实时成功的快照。
    live: HashMap<Region, CatalogSnapshot>,
    /// 从磁盘加载并缓存的快照。
    cached: HashMap<Region, CatalogSnapshot>,
    /// 已尝试从磁盘加载过的 region（避免重复 IO）。
    loaded: HashMap<Region, bool>,
}

impl Default for CatalogStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CatalogStore {
    /// 使用默认缓存目录 `~/.buddy-switch`。
    pub fn new() -> Self {
        Self::with_cache_dir(store_dir())
    }

    /// 使用指定缓存目录（便于测试）。
    pub fn with_cache_dir(dir: PathBuf) -> Self {
        Self {
            cache_dir: dir,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn cache_file(&self, region: Region) -> PathBuf {
        // 默认目录与 `region::catalog_cache_file` 一致；自定义目录时改写文件名。
        if self.cache_dir == store_dir() {
            catalog_cache_file(region)
        } else {
            self.cache_dir
                .join(format!("gateway_models.{}.json", region.as_str()))
        }
    }

    /// 内置兜底目录（按 region 返回对应的 `&'static` 只读切片）。
    pub fn fallback_models(region: Region) -> &'static [CatalogModel] {
        match region {
            Region::Cn => FALLBACK_CN_MODELS.as_slice(),
            Region::Global => FALLBACK_GLOBAL_MODELS.as_slice(),
        }
    }

    /// 当前该 region 的目录快照：live → cached → builtin。
    pub fn current(&self, region: Region) -> CatalogSnapshot {
        if let Some(snapshot) = self.inner.lock().unwrap().live.get(&region) {
            return snapshot.clone();
        }
        if let Some(snapshot) = self.load_cached(region) {
            return snapshot;
        }
        Self::builtin_snapshot(region, BUILTIN_NOTE)
    }

    /// 手动写入实时快照（供网关手动刷新 / 单测种子）。
    pub fn set_live(&self, region: Region, models: Vec<CatalogModel>) {
        let snapshot = CatalogSnapshot {
            region,
            source: CatalogSource::Live,
            fetched_at: Some(now_ms()),
            models,
            note: None,
        };
        self.inner.lock().unwrap().live.insert(region, snapshot);
    }

    /// 实时刷新：成功写缓存并返回 Live；失败按 cached → builtin 降级。
    pub async fn refresh(
        &self,
        region: Region,
        client: &UpstreamClient,
        acc: &Value,
    ) -> CatalogSnapshot {
        match client.fetch_models(region, acc).await {
            Ok(models) if !models.is_empty() => {
                let snapshot = CatalogSnapshot {
                    region,
                    source: CatalogSource::Live,
                    fetched_at: Some(now_ms()),
                    models,
                    note: None,
                };
                self.inner
                    .lock()
                    .unwrap()
                    .live
                    .insert(region, snapshot.clone());
                self.save_cached(&snapshot);
                snapshot
            }
            // 空清单（上游 200 但未给模型）与 `Err` 都算「本次识别失败」——
            // 空清单不可用，写 live 会让用户看到「实时 + 0 个模型」。
            failed => {
                let reason = match &failed {
                    Err(failure) => failure_reason(failure),
                    Ok(_) => EMPTY_CATALOG_NOTE.to_string(),
                };
                if let Some(cached) = self.load_cached(region) {
                    return with_reason(cached, &reason);
                }
                Self::builtin_snapshot(region, &reason)
            }
        }
    }

    fn builtin_snapshot(region: Region, reason: &str) -> CatalogSnapshot {
        with_reason(
            CatalogSnapshot {
                region,
                source: CatalogSource::Builtin,
                fetched_at: None,
                models: Self::fallback_models(region).to_vec(),
                note: None,
            },
            reason,
        )
    }

    fn load_cached(&self, region: Region) -> Option<CatalogSnapshot> {
        {
            let inner = self.inner.lock().unwrap();
            if inner.loaded.get(&region).copied().unwrap_or(false) {
                return inner.cached.get(&region).cloned();
            }
        }
        let snapshot = self.read_cached_file(region);
        let mut inner = self.inner.lock().unwrap();
        inner.loaded.insert(region, true);
        if let Some(snapshot) = &snapshot {
            inner.cached.insert(region, snapshot.clone());
        }
        snapshot
    }

    fn read_cached_file(&self, region: Region) -> Option<CatalogSnapshot> {
        let path = self.cache_file(region);
        let text = std::fs::read_to_string(&path).ok()?;
        let cached: CachedCatalog = serde_json::from_str(&text).ok()?;
        if cached.models.is_empty() {
            return None;
        }
        Some(with_reason(
            CatalogSnapshot {
                region,
                source: CatalogSource::Cached,
                fetched_at: Some(cached.fetched_at),
                models: cached.models,
                note: None,
            },
            CACHED_NOTE,
        ))
    }

    fn save_cached(&self, snapshot: &CatalogSnapshot) {
        if let Some(parent) = self.cache_file(snapshot.region).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let cached = CachedCatalog {
            region: snapshot.region,
            fetched_at: snapshot.fetched_at.unwrap_or_else(now_ms),
            models: snapshot.models.clone(),
        };
        let content = serde_json::to_string_pretty(&cached).unwrap_or_default();
        let _ = crate::modules::config::atomic_write(&self.cache_file(snapshot.region), &content);
    }
}

/// 便捷构造一个失败分类（供测试 / 降级路径复用）。
pub fn catalog_failure(status: u16, kind: UpstreamErrorKind, message: impl Into<String>) -> UpstreamFailure {
    UpstreamFailure {
        status,
        kind,
        message: message.into(),
    }
}

/// 解析目录（转调 [`parse_model_catalog`]），供需要直接解析的场景使用。
pub fn parse_catalog(data: &Value, international: bool) -> Result<Vec<CatalogModel>, UpstreamFailure> {
    parse_model_catalog(data, international)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-catalog-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 把内置兜底目录钉死：条目数与 id 序列硬编码，独立于兜底数据源本身。
    ///
    /// 背景：兜底降级链用例里 `builtin.models.len() == FALLBACK_CN_MODELS.len()`
    /// 两端同源（都走同一个 `LazyLock`），误删/误改兜底数据也照样通过。此处以
    /// 参考实现 `catalog.ts` 的 `FALLBACK_WORKBUDDY_MODELS` /
    /// `FALLBACK_WORKBUDDY_AI_MODELS` 为准做字面量校验。
    #[test]
    fn fallback_catalog_matches_hardcoded_contract() {
        const CN_IDS: [&str; 16] = [
            "auto",
            "hy4-preview",
            "hy3",
            "hy3-x",
            "deepseek-v4.1-flash",
            "glm-5.3",
            "glm-5.3-flash",
            "glm-5.2",
            "glm-5.1",
            "glm-5v-turbo",
            "kimi-k3-1",
            "kimi-k2.8-preview",
            "kimi-k2.7",
            "kimi-k2.6",
            "minimax-m3",
            "deepseek-v4-pro",
        ];
        const GLOBAL_IDS: [&str; 20] = [
            "default-model",
            "fast-model",
            "balanced-model",
            "primary-model",
            "deep-model",
            "hy4-preview-f",
            "hy3",
            "deepseek-v4.1-flash",
            "gpt-6-astra",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.3-codex",
            "gemini-3.5-flash",
            "glm-5.3",
            "glm-5.2",
            "kimi-k3",
            "kimi-k2.6",
        ];

        let cn = CatalogStore::fallback_models(Region::Cn);
        let global = CatalogStore::fallback_models(Region::Global);

        assert_eq!(FALLBACK_CN_MODELS.len(), 16);
        assert_eq!(FALLBACK_GLOBAL_MODELS.len(), 20);
        assert_eq!(cn[0].id, "auto");
        assert_eq!(global[0].id, "default-model");

        assert_eq!(
            cn.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            CN_IDS,
            "CN 兜底目录变更需同步更新本断言"
        );
        assert_eq!(
            global.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            GLOBAL_IDS,
            "Global 兜底目录变更需同步更新本断言"
        );

        assert_ne!(cn, global, "两个 region 的兜底目录不得互相复用");
    }

    #[test]
    fn degradation_chain_live_then_cached_then_builtin() {
        let dir = temp_dir();

        // 1) 无 live、无缓存 → Builtin。
        let store = CatalogStore::with_cache_dir(dir.clone());
        let builtin = store.current(Region::Cn);
        assert_eq!(builtin.source, CatalogSource::Builtin);
        // 硬编码 16：与兜底数据源比较会两端同源，失去回归价值。
        assert_eq!(builtin.models.len(), 16);

        // 2) 写入缓存文件后 → Cached（新 store 无 live）。
        let cached_models = vec![CatalogModel {
            id: "cached-model".into(),
            name: "Cached".into(),
            context_window: 1,
            max_tokens: 1,
            supports_images: false,
            credits: None,
            badges: Vec::new(),
            free: false,
        }];
        let cached_doc = json!({
            "region": "cn",
            "fetched_at": 42,
            "models": cached_models,
        });
        std::fs::write(
            dir.join("gateway_models.cn.json"),
            serde_json::to_string(&cached_doc).unwrap(),
        )
        .unwrap();
        let store2 = CatalogStore::with_cache_dir(dir.clone());
        let cached = store2.current(Region::Cn);
        assert_eq!(cached.source, CatalogSource::Cached);
        assert_eq!(cached.models[0].id, "cached-model");
        assert_eq!(cached.fetched_at, Some(42));

        // 3) 写入 live 后 → Live 优先。
        store2.set_live(
            Region::Cn,
            vec![CatalogModel {
                id: "live-model".into(),
                name: "Live".into(),
                context_window: 2,
                max_tokens: 2,
                supports_images: true,
                credits: Some("x0.10".into()),
                badges: Vec::new(),
                free: false,
            }],
        );
        let live = store2.current(Region::Cn);
        assert_eq!(live.source, CatalogSource::Live);
        assert_eq!(live.models[0].id, "live-model");

        // region 隔离：Global 仍为 Builtin。
        assert_eq!(store2.current(Region::Global).source, CatalogSource::Builtin);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_cache_is_ignored_and_falls_back_to_builtin() {
        let dir = temp_dir();
        std::fs::write(dir.join("gateway_models.global.json"), "not-json").unwrap();
        let store = CatalogStore::with_cache_dir(dir.clone());
        assert_eq!(store.current(Region::Global).source, CatalogSource::Builtin);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn sample_model(id: &str) -> CatalogModel {
        CatalogModel {
            id: id.into(),
            name: id.into(),
            context_window: 1,
            max_tokens: 1,
            supports_images: false,
            credits: None,
            badges: Vec::new(),
            free: false,
        }
    }

    fn has_reason(snapshot: &CatalogSnapshot) -> bool {
        snapshot
            .note
            .as_deref()
            .map(|note| !note.trim().is_empty())
            .unwrap_or(false)
    }

    /// ★ 契约（P10 §1.3 规则 2）：**非 live 快照的 `note` 恒非空**。
    ///
    /// ## 为什么这条断言必须存在
    ///
    /// 「非实时必有原因」此前只是**前端**的兜底（`describeCatalogSource`），后端
    /// `load_cached` 会把 `note` 置 `None` ⇒ 一份「已保存」快照在**后端**就没有原因。
    /// 本用例把契约钉在**后端**：非 live ⇒ note 非空。
    ///
    /// ## 可证伪（T03 变异验证 ②）
    ///
    /// 把 `read_cached_file` 的 `with_reason(…, CACHED_NOTE)` 改回 `note: None`
    /// （或删掉 `current()` 内置兜底的 `BUILTIN_NOTE`），本用例立刻变红。
    #[test]
    fn non_live_snapshot_always_carries_a_reason() {
        let dir = temp_dir();

        // ① 无 live、无缓存 → 内置：必须带原因。
        let store = CatalogStore::with_cache_dir(dir.clone());
        let builtin = store.current(Region::Cn);
        assert_eq!(builtin.source, CatalogSource::Builtin);
        assert!(
            has_reason(&builtin),
            "内置快照必须带原因（否则用户只剩一个灰徽标），实际 note={:?}",
            builtin.note
        );

        // ② 有缓存 → 已保存：必须带原因（这正是 `load_cached` 此前置 `None` 的缺口）。
        let cached_doc = json!({
            "region": "cn",
            "fetched_at": 42,
            "models": [sample_model("cached-model")],
        });
        std::fs::write(
            dir.join("gateway_models.cn.json"),
            serde_json::to_string(&cached_doc).unwrap(),
        )
        .unwrap();
        let cached = CatalogStore::with_cache_dir(dir.clone()).current(Region::Cn);
        assert_eq!(cached.source, CatalogSource::Cached);
        assert!(
            has_reason(&cached),
            "已保存快照必须带原因，实际 note={:?}",
            cached.note
        );

        // ③ live 允许无原因（「实时」不需要解释「为什么不是实时」）。
        let live_store = CatalogStore::with_cache_dir(dir.clone());
        live_store.set_live(Region::Global, vec![sample_model("live-model")]);
        let live = live_store.current(Region::Global);
        assert_eq!(live.source, CatalogSource::Live);
        assert_eq!(live.note, None, "live 不应带回落原因");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ✅ #5：失败原因**具体**（超时 / 网络 / 鉴权…），不是笼统一句。
    ///
    /// 可证伪：把 `failure_reason` 改回返回 `DEGRADE_NOTE` 一类的常量 ⇒ 下面的
    /// 「含具体词」「互不相同」断言全红。
    #[test]
    fn failure_reason_maps_failures_to_concrete_reasons() {
        // 传输层失败（status == 0）：按 `net` 的分类标签细化。
        let timeout = catalog_failure(
            0,
            UpstreamErrorKind::Server,
            net::compose_transport_message(TransportErrorKind::Timeout, "error sending request"),
        );
        assert!(
            failure_reason(&timeout).contains("超时"),
            "连接超时应映射为「超时」，实际：{}",
            failure_reason(&timeout)
        );

        let connect = catalog_failure(
            0,
            UpstreamErrorKind::Server,
            net::compose_transport_message(TransportErrorKind::Connect, "tcp connect error"),
        );
        assert!(
            failure_reason(&connect).contains("网络"),
            "无法连接应映射为「网络错误」，实际：{}",
            failure_reason(&connect)
        );

        // 有状态码：按 `kind` 分类。
        let cases = [
            (401, UpstreamErrorKind::SessionDead, "鉴权"),
            (402, UpstreamErrorKind::HardCredit, "积分"),
            (429, UpstreamErrorKind::SoftRate, "限流"),
            (404, UpstreamErrorKind::NotFound, "未找到"),
            (502, UpstreamErrorKind::Server, "服务端"),
            (400, UpstreamErrorKind::Client, "拒绝"),
        ];
        for (status, kind, want) in cases {
            let reason = failure_reason(&catalog_failure(status, kind, "boom"));
            assert!(
                reason.contains(want),
                "status={status} 期望含「{want}」，实际：{reason}"
            );
        }

        // 六种分类的原因必须**互不相同** —— 笼统一句话回答不了「为什么不是实时」。
        let mut unique = std::collections::HashSet::new();
        for kind in [
            UpstreamErrorKind::HardCredit,
            UpstreamErrorKind::SoftRate,
            UpstreamErrorKind::SessionDead,
            UpstreamErrorKind::NotFound,
            UpstreamErrorKind::Server,
            UpstreamErrorKind::Client,
        ] {
            assert!(
                unique.insert(failure_reason(&catalog_failure(500, kind, ""))),
                "两种分类给出了同一条原因：{kind:?}"
            );
        }
    }

    /// `is_stale`：从未识别（`fetched_at == None`）与 `fetched_at` 超过 24h 都算「待确认」。
    #[test]
    fn is_stale_covers_builtin_and_expired_live() {
        let now = 1_700_000_000_000_i64;
        let mut snapshot = CatalogSnapshot {
            region: Region::Cn,
            source: CatalogSource::Builtin,
            fetched_at: None,
            models: Vec::new(),
            note: Some(BUILTIN_NOTE.to_string()),
        };
        assert!(is_stale(&snapshot, now), "从未成功识别（内置）必为待确认");

        snapshot.source = CatalogSource::Live;
        snapshot.fetched_at = Some(now - STALE_AFTER_MS + 1);
        assert!(!is_stale(&snapshot, now), "24h 内的 live 不待确认");

        snapshot.fetched_at = Some(now - STALE_AFTER_MS);
        assert!(is_stale(&snapshot, now), "恰好 24h 即待确认");

        snapshot.fetched_at = Some(now - STALE_AFTER_MS - 1);
        assert!(is_stale(&snapshot, now), "超过 24h 的 live 待确认");
    }
}
