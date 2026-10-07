//! P11 T04 边界验证：禁用**判断点**的边界行为（真实代码，不是文本扫描）。
//!
//! # 为什么单独一个集成测试文件
//!
//! 「禁用」的两个出口都依赖同一份存储与同一个判断点（`is_disabled`）。文本扫描能证明
//! 「调用了它」，但证明不了「大小写变体命中吗」「存储里多一个未知 region 键会怎样」这类
//! **边界行为**。这些只能跑真代码。
//!
//! ⚠️ 本文件**只加断言**，不改产品源码。其中 `fixed_*` 用例锁住 T04 修复后的**正确行为**
//!    （大小写不敏感 / 未知 region 键不再清空 / 裸 map 形状被拒）—— 这些是**回归护栏**。

use buddy_switch_core::modules::region::Region;
use buddy_switch_gateway::{is_disabled, is_disabled_in, DisabledModels};

/// 边界①：空 / 空白 model id **永远不会**被判为禁用（`set` 拒绝写入，`is_disabled` 恒 false）。
#[test]
fn blank_and_empty_ids_are_never_disabled() {
    let mut store = DisabledModels::default();
    assert!(!store.set(Region::Cn, "", true), "空 id 不得写入");
    assert!(!store.set(Region::Cn, "   ", true), "空白 id 不得写入");
    assert!(!is_disabled(&store, Region::Cn, ""), "空 id 永远不命中");
    assert!(!is_disabled(&store, Region::Cn, "   "), "空白 id 永远不命中");
    assert!(store.get(Region::Cn).is_empty(), "空 / 空白 id 不得污染存储");
}

/// ★ 边界②（**回归护栏 · T04 #2**）：判断点**大小写不敏感**。
///
/// 上游对模型名大小写不敏感（Trae 的 `model_config` 先 `to_lowercase()` 归一），
/// 故禁用 `glm-5.3` 后，调用 `GLM-5.3` / `Glm-5.3` 都必须**被判为禁用** ——
/// 否则就是「假开关」（用户以为禁用了、实际还能调）。
#[test]
fn matching_is_case_insensitive() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "glm-5.3", true);
    assert!(is_disabled(&store, Region::Cn, "glm-5.3"), "精确名命中");
    assert!(is_disabled(&store, Region::Cn, "GLM-5.3"), "大写变体也必须命中（否则禁用被绕过）");
    assert!(is_disabled(&store, Region::Cn, "Glm-5.3"), "混合大小写也必须命中");
    // 存储保持**原始**大小写：前端 `disabledSet.has(model.id)` 依赖精确匹配。
    assert!(
        store.get(Region::Cn).contains("glm-5.3"),
        "存储不得被归一成小写（否则 UI 对不上混合大小写的 id）"
    );
}

/// ★ 边界②-回归：**报告里那个具体绕过链**（禁用 `claude-sonnet` → 调 `Claude-Sonnet`）。
#[test]
fn disabling_one_case_blocks_the_other_case() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "claude-sonnet", true);
    assert!(
        is_disabled(&store, Region::Cn, "Claude-Sonnet"),
        "禁用 claude-sonnet 后 Claude-Sonnet 也必须被拒（T04 #2 的绕过链）"
    );
    // 未禁用的大小写变体不得误伤。
    assert!(!is_disabled(&store, Region::Cn, "claude-opus"));
}

/// 判断点对前后空白**归一**（`trim`），与 `set` 的写入口径一致。
#[test]
fn surrounding_whitespace_is_trimmed_on_both_sides() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "  glm-5.3  ", true);
    assert!(is_disabled(&store, Region::Cn, "glm-5.3"), "写入 trim 后应命中无空白查询");
    assert!(is_disabled(&store, Region::Cn, " glm-5.3 "), "查询侧也 trim");
}

/// 边界④（**回归护栏 · T04 #4**）：**正确形状**下，未知 region 键**不影响已知键**。
///
/// 改造前：`Region` 作 map 键，未知键（未来新增 region / 手改的 `wb-cn`）让**整份**反序列化
/// 失败 ⇒ `load()` 回落**空表** ⇒ 所有 region 的偏好被静默清空。
/// 现在键是字符串 ⇒ 未知键被**原样保留**、已知键照常命中。
#[test]
fn fixed_unknown_region_key_does_not_wipe_known_regions() {
    let text = r#"{"regions":{"cn":["glm-5.3"],"wb-cn":["x"]}}"#;
    let store: DisabledModels = serde_json::from_str(text).expect("未知 region 键不得让整份反序列化失败");
    assert!(
        is_disabled(&store, Region::Cn, "glm-5.3"),
        "未知键不得清空合法的 cn 偏好"
    );
    assert!(store.get(Region::Global).is_empty(), "未出现的 region 仍是空集");
}

/// 边界③（**回归护栏 · T04 #3**）：**裸 map 形状**（`{"cn":[…]}`）被**拒绝**（响亮报错）。
///
/// 改造前：未知字段被 serde 默认忽略 ⇒ 「看起来对」的裸 map 文件**反序列化成功但得到空表**
/// ⇒ 全部禁用**静默失效**。现在 `deny_unknown_fields` ⇒ 形状错就报错（可诊断）。
#[test]
fn fixed_bare_map_shape_is_rejected_loudly() {
    let bare = r#"{"cn":["glm-5.3"],"global":["kimi-k2.6"]}"#;
    assert!(
        serde_json::from_str::<DisabledModels>(bare).is_err(),
        "裸 map 形状（缺 `regions` 字段）必须报错，而不是静默得到空表"
    );
}

/// 对照：正确形状 + 两个已知 region 键能正常往返。
#[test]
fn known_region_keys_round_trip() {
    let text = r#"{"regions":{"cn":["glm-5.3"],"global":["kimi-k2.6"]}}"#;
    let store: DisabledModels = serde_json::from_str(text).expect("正确形状应能反序列化");
    assert!(is_disabled(&store, Region::Cn, "glm-5.3"));
    assert!(is_disabled(&store, Region::Global, "kimi-k2.6"));
    assert!(!is_disabled(&store, Region::Global, "glm-5.3"), "region 之间不得串味");
}

/// 边界⑤：region 相互独立（禁 CN 的 m 不影响 Global 的 m）。
#[test]
fn regions_are_independent() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "m", true);
    assert!(is_disabled(&store, Region::Cn, "m"));
    assert!(!is_disabled(&store, Region::Global, "m"));
}

/// `is_disabled_in`（唯一判断点的核心）单独可测，且与 `is_disabled` 同口径。
#[test]
fn is_disabled_in_matches_the_store_api() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "GLM-5.3", true);
    assert!(is_disabled_in(store.get(Region::Cn), "glm-5.3"), "核心函数同样大小写不敏感");
    assert!(!is_disabled_in(store.get(Region::Global), "glm-5.3"), "另一 region 的空集不命中");
}
