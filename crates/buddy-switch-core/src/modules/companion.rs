//! 桌面悬浮窗（P9）的**只读**数据模型 + provider。
//!
//! ## 为什么是独立的只读模型，而不是复用账号 store
//!
//! 账号数据是**按 region 分家**的账号 / 积分域（`modules::account`）；悬浮窗展示的是
//! **跨来源的「会话 / 任务状态」**（运行中 / 待确认 / 已完成 / 失败），**不是账号列表**，
//! 也**不属于任何单一 scope**（`PLAN §3.16`：层级不属于 L0–L3）。两者数据域不同，
//! 硬塞进账号模型会把「会话状态」伪装成「账号数据」。
//!
//! ## 本期数据源范围（P9 ✅ #2：只做「已有数据源」的来源）
//!
//! provider **只**接由**现有**进度状态（宿主侧的 `switch-progress` 等）能推导出的
//! 「进行中操作」。跨来源客户端（Codex / CodeBuddy 等）与 P8 会话工作台**不在本期** ——
//! provider **预留**与 P8 同源的接口形状（本模块的 `LiveOperation` → `CompanionItem`
//! 映射就是那个可替换点）但**不依赖**它。某来源无数据源 ⇒ 按 §3.4 规则 1「无数据不渲染」，
//! **不渲染该来源**（不造一个永远空着的分组）。
//!
//! ## fail-closed，不造假
//!
//! 没有任何可推导的进行中操作 ⇒ 返回**结构化空态**（`items: []` + `source: "unavailable"`），
//! 由前端渲染空态。**绝不用演示数据填充**。

use serde::{Deserialize, Serialize};

/// 数据来源状态：provider 就绪且至少有一条可推导项时为 [`SOURCE_LIVE`]，
/// 否则为 [`SOURCE_UNAVAILABLE`]（结构化空态）。与前端 `CompanionSnapshot.source` 同源。
pub const SOURCE_LIVE: &str = "live";
/// 见 [`SOURCE_LIVE`]。
pub const SOURCE_UNAVAILABLE: &str = "unavailable";

/// 会话 / 任务四态（`PLAN §3.6` 状态语言对齐）。
///
/// 序列化为 **lowercase**（`"running"` / `"waiting"` / `"done"` / `"failed"`）——
/// 前端**完全按该字段**渲染 class / 文案 / 徽标，**不做业务判断**（P9 §1.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompanionState {
    /// 运行中。
    Running,
    /// 待确认。
    Waiting,
    /// 已完成。
    Done,
    /// 失败。
    Failed,
}

/// 悬浮窗的一行（只读）。
///
/// ⚠️ `title` 是**敏感字段**：前端**默认不进 DOM**（未悬停 / 未聚焦渲染占位符，
/// 悬停 / `:focus-visible` 才展开），见 P9 §1.6 / ✅ #3。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionItem {
    /// 稳定键（聚焦 / 去重用）。
    pub id: String,
    /// 来源客户端名（本期：有数据源的来源）。
    pub source: String,
    /// 会话 / 任务标题（★ 默认不进 DOM，见结构体文档）。
    pub title: String,
    /// 四态之一。
    pub state: CompanionState,
    /// 相对时间 / 提示（如「12 分钟前」「等待确认」）。
    pub hint: String,
}

/// 悬浮窗快照（只读）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionSnapshot {
    /// 行列表；无数据源时为空。
    pub items: Vec<CompanionItem>,
    /// [`SOURCE_LIVE`] / [`SOURCE_UNAVAILABLE`]。
    pub source: String,
    /// 快照生成时间（ms）。
    pub updated_at: i64,
}

/// provider 输入：一个「进行中操作」——由宿主侧**现有**状态推导（只读）。
///
/// 这是与 P8 会话工作台**同源的预留接口**：P8 就绪时，只需把它的「会话状态」也映射成
/// `LiveOperation` 塞进 [`snapshot_from_operations`]，本模块与前端**无需改动**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveOperation {
    /// 稳定键（聚焦 / 去重用）。
    pub id: String,
    /// 来源客户端名。
    pub source: String,
    /// 会话 / 任务标题。
    pub title: String,
    /// 相对时间 / 提示。
    pub hint: String,
    /// 四态之一。
    pub state: CompanionState,
}

/// 由现有事件推导出的**只读快照**（P9 §1.3）。
///
/// - 有进行中的操作 ⇒ `source = "live"`，逐条映射为 [`CompanionItem`]；
/// - 没有（provider 无数据源）⇒ 结构化空态：`items: []` + `source = "unavailable"`。
///
/// **纯函数**（无 IO / 无副作用），便于单测钉住「四态可被数据驱动」与「空态不造假」。
pub fn snapshot_from_operations(
    operations: &[LiveOperation],
    updated_at: i64,
) -> CompanionSnapshot {
    if operations.is_empty() {
        return CompanionSnapshot {
            items: Vec::new(),
            source: SOURCE_UNAVAILABLE.to_string(),
            updated_at,
        };
    }
    CompanionSnapshot {
        items: operations
            .iter()
            .map(|operation| CompanionItem {
                id: operation.id.clone(),
                source: operation.source.clone(),
                title: operation.title.clone(),
                state: operation.state,
                hint: operation.hint.clone(),
            })
            .collect(),
        source: SOURCE_LIVE.to_string(),
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation(id: &str, state: CompanionState) -> LiveOperation {
        LiveOperation {
            id: id.to_string(),
            source: "WorkBuddy".to_string(),
            title: format!("任务 {id}"),
            hint: "刚刚".to_string(),
            state,
        }
    }

    /// ★ 无进行中操作 ⇒ **结构化空态**（`items: []` + `source: "unavailable"`），**不造假**。
    ///
    /// 可证伪性：让空输入回落成任何演示数据 / `source: "live"` ⇒ 变红。
    #[test]
    fn empty_operations_yield_structured_empty_state() {
        let snapshot = snapshot_from_operations(&[], 1234);
        assert!(snapshot.items.is_empty(), "无数据源时不得造出任何行");
        assert_eq!(snapshot.source, SOURCE_UNAVAILABLE);
        assert_eq!(snapshot.updated_at, 1234);
    }

    /// ★ 四态**都能被数据驱动**（不是「恰好 4 行」——行数与分布由数据决定）。
    #[test]
    fn all_four_states_are_driven_by_data() {
        let operations = [
            operation("a", CompanionState::Running),
            operation("b", CompanionState::Waiting),
            operation("c", CompanionState::Done),
            operation("d", CompanionState::Failed),
        ];
        let snapshot = snapshot_from_operations(&operations, 42);
        assert_eq!(snapshot.source, SOURCE_LIVE);
        assert_eq!(snapshot.items.len(), 4);
        let states: Vec<CompanionState> = snapshot.items.iter().map(|item| item.state).collect();
        assert_eq!(
            states,
            vec![
                CompanionState::Running,
                CompanionState::Waiting,
                CompanionState::Done,
                CompanionState::Failed,
            ]
        );
    }

    /// 单条也能驱动（行数不固定为 4）——守住「四态齐备 ≠ 恰好 4 行」。
    #[test]
    fn a_single_operation_yields_a_single_row() {
        let snapshot = snapshot_from_operations(&[operation("only", CompanionState::Running)], 7);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].state, CompanionState::Running);
        assert_eq!(snapshot.items[0].id, "only");
    }

    /// 字段**逐字透传**（provider 不臆造、不改写）。
    #[test]
    fn fields_are_mapped_verbatim() {
        let snapshot = snapshot_from_operations(&[operation("x", CompanionState::Waiting)], 99);
        let item = &snapshot.items[0];
        assert_eq!(item.source, "WorkBuddy");
        assert_eq!(item.title, "任务 x");
        assert_eq!(item.hint, "刚刚");
    }

    /// ★ 四态序列化为 **lowercase**（前端按字段渲染 class / 文案的契约）。
    #[test]
    fn state_serializes_lowercase() {
        for (state, expected) in [
            (CompanionState::Running, "\"running\""),
            (CompanionState::Waiting, "\"waiting\""),
            (CompanionState::Done, "\"done\""),
            (CompanionState::Failed, "\"failed\""),
        ] {
            assert_eq!(serde_json::to_string(&state).unwrap(), expected);
        }
    }

    /// 快照整体可序列化（前后端契约）。
    #[test]
    fn snapshot_is_serializable() {
        let snapshot = snapshot_from_operations(&[operation("s", CompanionState::Done)], 5);
        let value: serde_json::Value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["source"], "live");
        assert_eq!(value["updated_at"], 5);
        assert_eq!(value["items"][0]["state"], "done");
    }
}
