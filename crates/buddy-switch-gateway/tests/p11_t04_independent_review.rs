//! P11 T04 修复的**独立复核**（另派验证者，不复用修复者自己的用例）。
//!
//! 修复者已在 `p11_disabled_models_edges.rs` 里放了回归护栏；本文件专门验**他没测的**四件事：
//!
//! 1. `deny_unknown_fields` 的**向后兼容**副作用：未知**顶层字段**（不是未知 region）现在
//!    会不会让**整份**反序列化失败 ⇒ `load()` 回落空表 ⇒ 全部禁用偏好**静默清空**？
//! 2. `HashMap<String,…>` 的**未知 region 键**：被保留后会不会「存着但永远查不到」？
//! 3. `disabled_models` 文件**被外部改坏**（非法 JSON）⇒ 是**可理解回落**还是崩溃？
//! 4. region 隔离在**序列化往返**下是否仍成立（同模型在 cn / global 各禁一次）。
//!
//! ⚠️ 本文件**只加断言、不改产品源码**。所有断言锚在**真实代码**上（`DisabledModels` /
//!    `is_disabled` / `load`），不是文本扫描。

use std::sync::Mutex;

use buddy_switch_core::modules::region::Region;
use buddy_switch_gateway::{is_disabled, is_disabled_in, DisabledModels};

/// 串行化所有触碰 `BUDDY_SWITCH_HOME` 的用例（env 是进程级全局状态）。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 把 `BUDDY_SWITCH_HOME` 指向一个已存在的空临时目录（`load()` 才能读到「无文件」）。
fn isolated_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wb-p11-review-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建隔离目录");
    std::env::set_var("BUDDY_SWITCH_HOME", &dir);
    dir
}

/// ★ 复核 ①（`deny_unknown_fields` 的向后兼容副作用）。
///
/// 修复 #3 用 `deny_unknown_fields` 让「裸 map 形状」**响亮报错**。但同一属性也会让
/// **任何未知顶层字段**（例：未来版本新增的 `schemaVersion`，或用户手加的注记）整份报错。
/// 由于 `load()` 把**任何**解析错误吞成 `default()`，其运行时后果是**全部 region 的禁用
/// 偏好被静默清空** —— 这与修复 #3 想消灭的「静默失效」是**同一族**。
///
/// 本用例**记录**该行为（不判对错）：断言「未知顶层字段 ⇒ 整份 Err」，并演示
/// `load()` 等价路径（`unwrap_or_default`）会得到空表。
#[test]
fn unknown_top_level_field_rejects_the_whole_file() {
    // 有合法 regions + 一个**无关**的顶层字段。
    let text = r#"{"regions":{"cn":["glm-5.3"]},"schemaVersion":2}"#;
    let parsed = serde_json::from_str::<DisabledModels>(text);
    assert!(
        parsed.is_err(),
        "当前实现下，未知顶层字段会让整份反序列化失败（deny_unknown_fields 的副作用）"
    );

    // `load()` 的等价路径：解析失败 ⇒ 回落空表 ⇒ 已禁用的模型**全部复活**。
    let loaded = serde_json::from_str::<DisabledModels>(text).unwrap_or_default();
    assert!(
        !is_disabled(&loaded, Region::Cn, "glm-5.3"),
        "文件里明明禁用了 glm-5.3，但一个无关顶层字段就让偏好**静默丢失**"
    );
}

/// ★ 复核 ②（未知 region 键：保留但惰性）。
///
/// 键改 `String` 后未知 region（例 `"mars"` / 手改的 `"wb-cn"`）**不再清空已知键**（这是
/// 修复 #4 的目标，已由 `fixed_unknown_region_key_does_not_wipe_known_regions` 覆盖）。
/// 本用例进一步钉住**另一面**：未知键会被**原样保留**（写回仍在），但 `is_disabled`
/// 只按 `Region`（`cn`/`global`）取集合 ⇒ 该条目**永远不会被命中**（惰性，不放大也不误伤）。
#[test]
fn unknown_region_key_is_preserved_but_inert() {
    let text = r#"{"regions":{"cn":["glm-5.3"],"mars":["x"]}}"#;
    let store: DisabledModels = serde_json::from_str(text).expect("未知 region 键不得让整份失败");

    // 已知键照常命中。
    assert!(is_disabled(&store, Region::Cn, "glm-5.3"), "已知键不受未知键影响");

    // 未知键的条目**不被**任何 Region 命中（`Region` 枚举只产 cn / global）。
    assert!(!is_disabled(&store, Region::Cn, "x"), "未知 region 的条目不得串进 cn");
    assert!(!is_disabled(&store, Region::Global, "x"), "未知 region 的条目不得串进 global");

    // 未知键**被保留**（序列化写回后仍在）—— 不是被丢弃，故无「静默丢数据」。
    let round = serde_json::to_string(&store).expect("序列化");
    assert!(round.contains("\"mars\""), "未知 region 键应原样保留（写回仍在）：{round}");
    assert!(round.contains("\"cn\""), "{round}");
}

/// ★ 复核 ③（损坏文件）：非法 JSON ⇒ **可理解回落**（空表），不 panic、不报错。
///
/// 这里走**真实的 `DisabledModels::load()`**（读磁盘），而不是 `from_str(...).unwrap_or_default()`
/// —— 后者证明不了 `load()` 自己的容错接线。
#[test]
fn load_falls_back_to_empty_on_corrupted_file_without_panicking() {
    let _g = guard();
    let home = isolated_home("corrupt");
    let file = buddy_switch_gateway::disabled_models::file();
    std::fs::create_dir_all(file.parent().expect("有父目录")).expect("建目录");

    // 先放一份合法文件 + 禁用一条，确认路径确实被 `load()` 读到（否则「空表」恒真）。
    std::fs::write(&file, r#"{"regions":{"cn":["glm-5.3"]}}"#).expect("写合法文件");
    let good = DisabledModels::load();
    assert!(
        is_disabled(&good, Region::Cn, "glm-5.3"),
        "前提：合法文件必须能被 load() 读到（否则本用例是空断言）"
    );

    // 再把它改坏 ⇒ load() 必须回落空表且**不 panic**。
    std::fs::write(&file, "{ this is not json").expect("写损坏文件");
    let broken = DisabledModels::load();
    assert!(
        !is_disabled(&broken, Region::Cn, "glm-5.3"),
        "损坏文件应回落空表（可理解行为），而不是把上次的值留着或崩溃"
    );
    assert!(broken.get(Region::Cn).is_empty(), "损坏文件 ⇒ 空表");

    let _ = std::fs::remove_dir_all(&home);
}

/// ★ 复核 ④（region 隔离 · 序列化往返）：同一模型在 cn 与 global 各禁一次，互不影响。
///
/// 与「内存里独立」不同，本用例经**序列化 → 反序列化**再验，堵住「只在内存对、落盘串味」。
#[test]
fn same_model_disabled_in_both_regions_stays_isolated_across_round_trip() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "glm-5.3", true);
    // 只在 cn 禁用；global 未动。
    let text = serde_json::to_string(&store).expect("序列化");
    let back: DisabledModels = serde_json::from_str(&text).expect("反序列化");

    assert!(is_disabled(&back, Region::Cn, "glm-5.3"), "cn 的禁用必须往返保留");
    assert!(!is_disabled(&back, Region::Global, "glm-5.3"), "global 不得被 cn 的禁用串味");

    // 再在 global 禁**同一**模型 ⇒ 两者各自独立命中，互不覆盖。
    let mut both = back;
    both.set(Region::Global, "glm-5.3", true);
    let text2 = serde_json::to_string(&both).expect("序列化");
    let back2: DisabledModels = serde_json::from_str(&text2).expect("反序列化");
    assert!(is_disabled(&back2, Region::Cn, "glm-5.3"));
    assert!(is_disabled(&back2, Region::Global, "glm-5.3"));
    // 解除 cn 的禁用，global 仍保持禁用（region 独立清位）。
    let mut only_global = back2;
    only_global.set(Region::Cn, "glm-5.3", false);
    assert!(!is_disabled(&only_global, Region::Cn, "glm-5.3"));
    assert!(is_disabled(&only_global, Region::Global, "glm-5.3"), "清 cn 不得影响 global");
}

/// 复核：`is_disabled_in`（大小写不敏感的核心）对**混合大小写存储**也成立。
///
/// 修复者的用例只覆盖「存小写、查大写」。这里反过来：**存混合大小写、查小写/大写**。
#[test]
fn is_disabled_in_is_case_insensitive_regardless_of_stored_case() {
    let mut store = DisabledModels::default();
    store.set(Region::Cn, "DeepSeek-V4-Flash-Official", true);
    assert!(
        is_disabled_in(store.get(Region::Cn), "deepseek-v4-flash-official"),
        "存混合大小写、查小写也必须命中"
    );
    assert!(
        is_disabled_in(store.get(Region::Cn), "DEEPSEEK-V4-FLASH-OFFICIAL"),
        "存混合大小写、查全大写也必须命中"
    );
    // 存储保持原样（前端 `disabledSet.has(model.id)` 依赖精确匹配）。
    assert!(
        store.get(Region::Cn).contains("DeepSeek-V4-Flash-Official"),
        "存储不得被归一（否则前端精确匹配会对不上）"
    );
}
