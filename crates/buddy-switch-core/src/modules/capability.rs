//! 平台能力：受限能力的统一说明形状 + 「在文件管理器中打开目录」。
//!
//! ## 为什么 [`Unsupported`] 从 `trae::platform` 搬到这里
//!
//! 它原本只服务 Trae（平台能力矩阵）。WorkBuddy 侧的「打开数据目录」需要**同一个形状** ——
//! 前端才能用同一段分支渲染「当前平台不支持」而不是「加载失败」。
//! 与其再拼一遍 JSON（字段名迟早漂移：`supportedOn` vs `supported_on`，前端静默拿不到值），
//! 不如把它放到通用位置：`trae::platform` 以 `pub use` 转发，**Trae 侧调用点零改动**。
//!
//! ⚠️ 与 `trae::handlers::unsupported_note` 的分工（那里刻意保留第二处，理由见其文档）：
//! 本结构的字段是 `&'static str`，表达**平台能力矩阵**；载荷内运行期拼装的置灰卡走那个。

use serde_json::{json, Value};

use crate::modules::region::{self, Region};
use crate::modules::session;

/// 平台能力（PLAN §2.7）—— **平台固有，用户不可改**（P5 §2.5 第 (2) 条）。
///
/// ## 与「用户偏好」正交
///
/// - **能力**（本枚举）：平台**有没有**这个功能。用户改不了。由**命令分发层**校验
///   （`product::require_capability`，**默认拒绝**）。
/// - **偏好**（`preferences.*`）：用户**要不要看**这个入口。纯前端，**不影响数据**。
///
/// 典型混淆：用户关掉「CodeBuddy CLI」入口 ≠ 该能力消失 —— 走 URL 或直接调命令仍应工作。
///
/// ## 为什么必须在**命令分发层**校验（而不是靠 UI 隐藏）
///
/// Tauri 自定义命令对 webview **全可达**：UI 不渲染按钮 ≠ 命令调不到。Trae 侧既有的
/// `trae/platform.rs::capabilities()` 只是**诊断用的 unsupported 列表**，**不是门控**。
/// 故每个涉及能力的命令入口都要 `require_capability(product, Capability::X)?`，
/// 缺能力即 `Err`（见 `product::require_capability`）。
///
/// ⚠️ **取值与前端 `src/lib/platforms/registry.ts` 的 `Capability` 联合类型逐字对齐**
/// （`serde(rename_all = "camelCase")` 产出 `autoRotate` / `sessionCopy`）。
/// 两侧漂移由 `product.rs` 的跨语言护栏测试
/// （`product_spec_capabilities_match_frontend_registry`）兜住。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// 签到。
    Checkin,
    /// 自动旅行（**WorkBuddy 独有**）。
    Travel,
    /// 积分 / 资源包。
    Credits,
    /// 扫码登录。
    Oauth,
    /// API 网关。
    Gateway,
    /// 自动轮换（**WorkBuddy 独有**）。
    AutoRotate,
    /// 会话复制（**只有 WorkBuddy 桌面**，见 PLAN §3.15）。
    SessionCopy,
}

impl Capability {
    /// 稳定标识 —— 与前端 `Capability` 字面量**逐字一致**。
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Checkin => "checkin",
            Capability::Travel => "travel",
            Capability::Credits => "credits",
            Capability::Oauth => "oauth",
            Capability::Gateway => "gateway",
            Capability::AutoRotate => "autoRotate",
            Capability::SessionCopy => "sessionCopy",
        }
    }

    /// 从字符串解析能力；未知返回 `None`（供诊断 / 注册表对照用）。
    pub fn parse(s: &str) -> Option<Capability> {
        Capability::all().into_iter().find(|c| c.as_str() == s)
    }

    /// 全部能力，供遍历用。
    pub fn all() -> [Capability; 7] {
        [
            Capability::Checkin,
            Capability::Travel,
            Capability::Credits,
            Capability::Oauth,
            Capability::Gateway,
            Capability::AutoRotate,
            Capability::SessionCopy,
        ]
    }
}

/// 平台受限能力：说明「哪个能力、在哪些平台可用、当前平台为什么不行」。
///
/// 字段刻意带上 `supported_on` 与 `reason`：前端可直接渲染成一条可操作的提示，
/// 而不是让用户面对「不支持」三个字去猜。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// 能力标识，例 `machine_guid_reset`。
    pub capability: &'static str,
    /// 能力的人类可读名称。
    pub label: &'static str,
    /// 该能力可用的平台。
    pub supported_on: &'static str,
    /// 当前平台不可用的具体原因。
    pub reason: String,
}

impl Unsupported {
    /// 构造一条受限能力说明。
    pub fn new(
        capability: &'static str,
        label: &'static str,
        supported_on: &'static str,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            capability,
            label,
            supported_on,
            reason: reason.into(),
        }
    }

    /// 线上形态（camelCase）。
    pub fn to_json(&self) -> Value {
        json!({
            "capability": self.capability,
            "label": self.label,
            "supportedOn": self.supported_on,
            "reason": self.reason,
        })
    }
}

/// 打开 **WorkBuddy 客户端**的数据目录（宿主命令 `open_workbuddy_data_dir` 的唯一实现）。
///
/// 目录走 [`session::session_data_dir`]（CN `.workbuddy` / Global `.workbuddy-ai`），
/// 与「凭据从哪来」「会话正文在哪」这些问题同源 —— 用户点这个按钮就是要去核对它们。
///
/// ## 两条边界（与 Trae 侧 `trae::handlers::open_data_dir` 同策）
///
/// - **非 Windows → 结构化 `Unsupported`**，不是「假成功」也不是裸 `Err`：
///   「在文件管理器中打开」按 Windows 语义实现（`explorer`），其他平台未对齐，
///   如实声明做不到，前端据此渲染可操作提示。
/// - **目录不存在 → 可读 `Err`**（提示先启动一次客户端），而不是打开一个空路径
///   让 `explorer` 弹「找不到」。
pub fn open_workbuddy_data_dir(region: Region) -> Result<Value, String> {
    if !cfg!(windows) {
        return Ok(Unsupported::new(
            "open_workbuddy_data_dir",
            "打开 WorkBuddy 数据目录",
            "Windows",
            "WorkBuddy 数据目录定位与文件管理器打开按 Windows 语义实现，当前平台未提供等价方式。",
        )
        .to_json());
    }

    let dir = session::session_data_dir(region);
    if !dir.is_dir() {
        // 不再写成「未找到「{region}」的 WorkBuddy 数据目录」——`region_display(Cn)` 就是
        // "WorkBuddy"，那样会读成「WorkBuddy 的 WorkBuddy 数据目录」（实测文案）。
        return Err(format!(
            "未找到 {} 的数据目录（{}）：请先启动一次该客户端。",
            region::region_display(region),
            dir.display()
        ));
    }

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|error| format!("打开目录失败: {error}"))?;
    }

    Ok(json!({
        "ok": true,
        "path": dir.to_string_lossy(),
        "region": region.as_str(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_serializes_with_camel_case_keys() {
        let value = Unsupported::new("open_workbuddy_data_dir", "打开目录", "Windows", "原因").to_json();
        assert_eq!(value["capability"], json!("open_workbuddy_data_dir"));
        assert_eq!(value["supportedOn"], json!("Windows"));
        assert_eq!(value["reason"], json!("原因"));
        // 前端按 camelCase 读 ⇒ 不得出现 snake_case 键。
        assert!(value.get("supported_on").is_none());
    }

    /// 非 Windows 必须返回**结构化 Unsupported**（带四个字段），而不是裸错误。
    #[test]
    fn open_workbuddy_data_dir_is_structured_unsupported_off_windows() {
        if cfg!(windows) {
            return;
        }
        let value = open_workbuddy_data_dir(Region::Cn).expect("非 Windows 走 Unsupported 分支");
        assert_eq!(value["capability"], json!("open_workbuddy_data_dir"));
        assert!(value["reason"].as_str().unwrap_or("").contains("Windows"));
        assert!(value.get("ok").is_none(), "做不到就不能带 ok:true");
    }

    /// `Capability::as_str` 的 7 个取值与前端 `registry.ts` 的联合类型**逐字一致**，
    /// 且互不重复、可往返解析。
    #[test]
    fn capability_as_str_matches_frontend_union_and_round_trips() {
        let expected = [
            "checkin",
            "travel",
            "credits",
            "oauth",
            "gateway",
            "autoRotate",
            "sessionCopy",
        ];
        let got: Vec<&str> = Capability::all().iter().map(|c| c.as_str()).collect();
        assert_eq!(got, expected, "Capability 取值必须与前端 Capability 联合类型逐字对齐");

        // 互不重复（重复会让「按名解析」产生歧义）。
        let mut sorted = got.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len(), "Capability 取值不得重复");

        // 往返。
        for cap in Capability::all() {
            assert_eq!(Capability::parse(cap.as_str()), Some(cap));
        }
        assert_eq!(Capability::parse("nope"), None);
    }
}
