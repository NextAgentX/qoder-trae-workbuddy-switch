//! 「接入即识别」与「网关启动补识别」的**唯一探测出口**（P10 §1.6 / T03）。
//!
//! ## 为什么要有这个模块
//!
//! 改造前探测**只有手动**一条路（`refresh_gateway_models` / `api_gateway_refresh_models`），
//! 于是「刚绑好凭据」到「看到模型清单」之间隔着一整段空白：用户不点「重新识别」就永远
//! 看到「内置 / 已保存」，却不知道自己其实已经接好了（P10 §1.1「接入即识别未接线」）。
//!
//! 本模块把探测收敛成**一个**入口，让两处触发共用同一份逻辑、同一套降级语义：
//!
//! 1. **接入即识别**（✅ #6：绑定凭据成功 = 创建 API Key）—— `create_api_key` 成功后
//!    fire-and-forget 调 [`probe_region`]；
//! 2. **网关启动补识别** —— 清单「待确认」（内置，或 `fetched_at` 超过 24h）时调
//!    [`probe_stale_regions`]。
//!
//! ## 三条硬约束
//!
//! - **不新造探测逻辑**：一律复用 [`CatalogStore::refresh`]，失败时由它按
//!   cached → builtin 降级并写入**具体原因**（[`buddy_switch_core::modules::catalog::failure_reason`]）。
//! - **失败不阻断接入**（§3.17.1 规则 3）：调用方 fire-and-forget，探测失败**静默**。
//! - **安全边界（§3.17.5 / §2.5）**：探测**只用该 region 自己的凭据**
//!   （`state.strategy_for(region)` + [`AccountSelector::select`]，后者已按 region 校验）；
//!   `CatalogStore` 已按 region 隔离落盘 ⇒ **不可能**跨 region 读缓存。

use buddy_switch_core::modules::catalog::{is_stale, CatalogSnapshot, CatalogStore};
use buddy_switch_core::modules::region::Region;

use crate::account_strategy::AccountSelector;
use crate::error::GatewayError;
use crate::state::GatewayState;
use crate::timeutil;

/// 探测某 region 的模型目录（**唯一的探测入口**）。
///
/// - 成功 ⇒ 写 live（并落盘 `gateway_models.<region>.json`）；
/// - 上游失败 ⇒ 由 [`CatalogStore::refresh`] 按 cached → builtin 降级，**原因写进 `note`**；
/// - 账号选择失败（池空 / 策略无命中 / 区域不符）⇒ 返回 [`GatewayError`]，
///   **不写脏数据**（调用方决定是静默忽略还是回传错误）。
///
/// ⚠️ 调用方负责 **fire-and-forget**（探测失败不得阻断接入）。
pub async fn probe_region(
    state: &GatewayState,
    region: Region,
) -> Result<CatalogSnapshot, GatewayError> {
    let strategy = state.strategy_for(region).await;
    // 安全边界 F：`AccountSelector::select` 已按 region 校验凭据（区域不符直接拒），
    // 因此本探测**不可能**用别的 region 的凭据去写本 region 的清单。
    let account = AccountSelector.select(region, &strategy).await?;
    Ok(state.catalogs.refresh(region, &state.upstream, &account).await)
}

/// 需要「启动补识别」的 region（**纯函数**，只读 store，便于单测）。
///
/// 判据 = [`is_stale`]：内置（从未成功识别）或 `fetched_at` 超过 24h。
pub fn stale_regions(store: &CatalogStore, now_ms: i64) -> Vec<Region> {
    Region::all()
        .into_iter()
        .filter(|region| is_stale(&store.current(*region), now_ms))
        .collect()
}

/// [`stale_regions`] 的宿主入口：按当前时刻判定哪些 region 待确认。
pub fn regions_needing_probe(state: &GatewayState) -> Vec<Region> {
    stale_regions(&state.catalogs, timeutil::now_ms())
}

/// 网关启动补识别：对每个「待确认」region 探测一次，返回实际探测的 region 数。
///
/// 失败静默（fire-and-forget 语义）—— 探测只是「顺手把清单变新」，绝不该让启动失败。
pub async fn probe_stale_regions(state: &GatewayState) -> usize {
    let regions = regions_needing_probe(state);
    for region in &regions {
        let _ = probe_region(state, *region).await;
    }
    regions.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use buddy_switch_core::modules::catalog::CatalogModel;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-catalog-probe-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    /// ★ 启动补识别的判据：全新 store（两 region 均内置）⇒ 都要探测；
    /// 刚写入 live 的 region ⇒ 不再探测（避免每次启动都无脑打上游）。
    #[test]
    fn stale_regions_probes_builtin_and_skips_fresh_live() {
        let dir = temp_dir();
        let store = CatalogStore::with_cache_dir(dir.clone());
        let now = timeutil::now_ms();

        // 全新 store：两 region 都是内置（从未成功识别）⇒ 都待确认。
        assert_eq!(
            stale_regions(&store, now),
            Region::all().to_vec(),
            "内置清单必须待确认（否则「接入即识别」的补识别永远不会触发）"
        );

        // 写入新鲜的 live 后，该 region 不再待确认。
        store.set_live(Region::Cn, vec![sample_model("live-model")]);
        assert_eq!(
            stale_regions(&store, now),
            vec![Region::Global],
            "新鲜的 live 不应被反复探测"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 超过 24h 的 live 会重新进入「待确认」（`fetched_at` 判定，与来源无关）。
    #[test]
    fn stale_regions_probes_expired_live() {
        let dir = temp_dir();
        let store = CatalogStore::with_cache_dir(dir.clone());
        store.set_live(Region::Cn, vec![sample_model("live-model")]);

        let fetched_at = store.current(Region::Cn).fetched_at.unwrap();
        // 刚写入 ⇒ 不待确认；把「现在」推到 24h 之后 ⇒ 待确认。
        assert!(!stale_regions(&store, fetched_at).contains(&Region::Cn));
        assert!(stale_regions(&store, fetched_at + 24 * 60 * 60 * 1000).contains(&Region::Cn));

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ **接线护栏（跨文件）**——「接入即识别」与「启动补识别」的出口**必须**存在。
    ///
    /// 本用例直接读**两个宿主**（桌面 `src-tauri` / 服务端 `buddy-switch-server`）的源码，
    /// 断言四处调用点都在。这是本仓对「**写状态出口静默消失**」那一类事故（P5 R9：函数存在、
    /// 类型全对、构建全绿、单测全绿，但**生产上没人调用**）的标准守卫形态 —— 与
    /// `core::background::both_hosts_register_every_task_kind`（读两宿主 spawn 分支）同款做法。
    ///
    /// ## 为什么读源码而不是「跑一遍」
    ///
    /// `create_api_key` 是 **Tauri 命令**，测试环境起不了 Tauri runtime；服务端 handler 会碰
    /// 真实 home。故只能用**调用点标记**锚定接线 —— 与 core 读宿主源码是同一取舍。
    ///
    /// ## 可证伪（T03 变异验证 ①）
    ///
    /// 从任一处删掉探测调用，本用例立即变红：
    /// - 删 `commands.rs` 里 `create_api_key` 的 `probe_catalog(region)`（接入即识别 · 桌面端）；
    /// - 删 `api.rs` 里 `api_gateway_create_key` 的 `catalog_probe::probe_region(&state, region)`（服务端）；
    /// - 删 `lib.rs` / `main.rs` 里 `GatewayCatalogProbe` 分支的 `probe_stale_regions(&state)`（启动补识别）。
    ///
    /// 标记带**实参**（而非裸函数名）：`probe_catalog` 在本仓既是定义（`(region: Region)`）
    /// 又是调用（`(region)`），裸名会在删掉调用点后仍被定义行「满足」⇒ 护栏空转。
    #[test]
    fn both_hosts_wire_the_catalog_probe() {
        const TAURI_COMMANDS_RS: &str =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-tauri/src/commands.rs");
        const TAURI_LIB_RS: &str =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-tauri/src/lib.rs");
        const SERVER_API_RS: &str =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/buddy-switch-server/src/api.rs");
        const SERVER_MAIN_RS: &str =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/buddy-switch-server/src/main.rs");

        let read = |path: &str| {
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("读不到 {path}: {error}"))
        };

        // ① 接入即识别（✅ #6：触发点 = 创建 API Key）：两宿主的 create_api_key 都要触发探测。
        for (file, marker) in [
            (TAURI_COMMANDS_RS, "probe_catalog(region)"),
            (SERVER_API_RS, "catalog_probe::probe_region(&state, region)"),
        ] {
            assert!(
                read(file).contains(marker),
                "{file} 缺少「接入即识别」出口 `{marker}` —— 创建 API Key 后不会后台探测，\
                 用户必须手动点「重新识别」才看得到清单（P10 §1.6 / ✅ #6）"
            );
        }

        // ② 启动补识别：两宿主的 spawn 派发都要调 `probe_stale_regions`。
        for file in [TAURI_LIB_RS, SERVER_MAIN_RS] {
            assert!(
                read(file).contains("probe_stale_regions(&state)"),
                "{file} 缺少「启动补识别」出口 —— 「内置 / 陈旧」的清单不会在启动时被补成实时\
                 （P10 §1.6 / ✅ #7）"
            );
        }
    }
}
