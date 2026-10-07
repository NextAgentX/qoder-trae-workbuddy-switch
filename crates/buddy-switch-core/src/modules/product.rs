//! 产品线（`Product`）与作用域（`Scope`）—— P5 的**唯一事实来源**。
//!
//! ## 为什么需要这一层
//!
//! 两个宿主（`src-tauri` 与 `buddy-switch-server`）各自散落着 `parse_region` /
//! `parse_trae_variant`，它们是**宽容解析**（缺失即回落到默认）。参数化命令面（P5 T03）
//! 会引入 `scope` 参数，若沿用宽容解析，一个拼错的作用域会被**静默**当成另一个作用域：
//!
//! ```text
//! accounts_list("trae-glboal")  →  静默读写 trae-cn 的账号库
//! ```
//!
//! 用户看到的是「账号凭空消失」，而**构建全绿、日志无痕**。所以 `scope` 的解析必须
//! **fail-closed**：未知 / 空 / 缺一律 `Err`（[`Scope::parse`]），**绝不 default**。
//!
//! ## 与既有 `Region` / `TraeRegion` 的关系
//!
//! [`Scope`] 是「**平台 × 区域**」的二维笛卡尔积（L3），由 [`Product`]（L1）与
//! 各自的区域轴组合而成：
//!
//! | 平台 | 区域轴 | 作用域 |
//! |:---|:---|:---|
//! | WorkBuddy | [`Region`]（cn / global） | `wb-cn` / `wb-global` |
//! | Trae | [`TraeRegion`]（cn / global） | `trae-cn` / `trae-global` |
//!
//! ⚠️ **路径派生唯一规则不破**（P5 §前置 4）：作用域字符串 → 解析成枚举 → 查表派生路径，
//! **中间任何一步都不许用原始字符串**。本模块只负责「字符串 → 枚举」这一步；
//! 路径由 `region::accounts_file_for` / `trae::paths::*_for_region` 等**枚举入口**派生。
//! 任何把 `scope` 原始串拼进 `PathBuf` 的写法都是把隔离根基拆掉。
//!
//! ## `Region::parse` 的 `Option` 语义为何保留
//!
//! [`Region::parse`]（core `region.rs`）与两宿主的 `parse_region` 是**旧 `region` 参数**的
//! 兼容入口 —— 老前端不带该参数是**合法**的（等价于 CN），因此它们的 `Option` /
//! `unwrap_or(Cn)` 语义**不在这里改**。fail-closed 只约束**新的 `scope` 参数路径**
//! （[`Scope::parse`] / [`resolve_scope`] / [`resolve_scope_opt`]）。两者的边界见
//! `scripts/check-api-contract.cjs` 第 9 条：它**只**扫本文件的 fail-open 反例，
//! 不误伤旧的 `region` 兼容解析。

use crate::modules::capability::Capability;
use crate::modules::region::Region;
use crate::modules::trae::region::TraeRegion;

/// 一条产品线（L1）。
///
/// `Workbuddy` 与 `Trae` 是「新增平台」时的注册点之一 —— 新增平台 = 这里加一个取值 +
/// `Scope` 加对应作用域 + `ProductSpec` 加一条（T02）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Product {
    /// WorkBuddy（国内版 / 国际版两套账号体系）。
    Workbuddy,
    /// Trae（国内版 / 国际版两套账号体系）。
    Trae,
}

impl Product {
    /// scope 短标识 —— 与前端 `PLATFORM_SLUG`（`registry.ts`）**逐字对齐**：`"wb"` / `"trae"`。
    ///
    /// ⚠️ 改这里必须同步改前端注册表，否则 `/s/{scope}/...` 路由与后端作用域会漂移。
    pub fn slug(self) -> &'static str {
        match self {
            Product::Workbuddy => "wb",
            Product::Trae => "trae",
        }
    }

    /// 稳定的完整标识：`"workbuddy"` / `"trae"`。
    pub fn as_str(self) -> &'static str {
        match self {
            Product::Workbuddy => "workbuddy",
            Product::Trae => "trae",
        }
    }

    /// 从字符串解析平台（**宽容**，供诊断 / 旧值兼容用；`scope` 路径请用 [`Scope::parse`]）。
    ///
    /// 接受完整标识与短标识（`workbuddy` / `wb` / `trae`），大小写不敏感。
    pub fn parse(s: &str) -> Option<Product> {
        match s.trim().to_ascii_lowercase().as_str() {
            "workbuddy" | "wb" => Some(Product::Workbuddy),
            "trae" => Some(Product::Trae),
            _ => None,
        }
    }

    /// 全部平台，供遍历用（顺序即 UI 展示顺序）。
    pub fn all() -> [Product; 2] {
        [Product::Workbuddy, Product::Trae]
    }
}

/// 作用域 = **平台 × 区域**（L3）。**fail-closed** 的解析载体。
///
/// 四个取值对应前端 `scope.ts` 的 `SCOPES`（`wb-cn` / `wb-global` / `trae-cn` / `trae-global`），
/// 由 `scripts/check-scope-registry.cjs` 在前端侧、本模块在后端侧共同钉住。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Scope {
    /// WorkBuddy 国内版。
    WbCn,
    /// WorkBuddy 国际版。
    WbGlobal,
    /// Trae 国内版。
    TraeCn,
    /// Trae 国际版。
    TraeGlobal,
}

impl Scope {
    /// ★ **fail-closed**：未知 / 空 / 缺一律 `Err`，**绝不 default**（P5 §2.5 第 (1) 条）。
    ///
    /// 判据与前端 `scope.ts::parseScope` **逐字一致**：**不做 trim、不做大小写折叠**，
    /// 只接受下面四个字面量。理由（真实缺陷形态）：拼错 `trae-glboal` 若被静默当成
    /// `trae-cn`，会**读写错误的库** —— 用户看到「账号凭空消失」，而错误是无声的。
    ///
    /// 为什么连「大小写宽容」都不做：宽容解析正是 fail-open 的入口 —— 一旦允许折叠，
    /// 就会有人顺手加上「无法匹配时回落默认」，那等于把这条边界又变回 fail-open。
    /// 需要宽容的是**旧 `region` 参数**（[`Region::parse`]），不是 `scope`。
    pub fn parse(s: &str) -> Result<Scope, ScopeError> {
        match s {
            "wb-cn" => Ok(Scope::WbCn),
            "wb-global" => Ok(Scope::WbGlobal),
            "trae-cn" => Ok(Scope::TraeCn),
            "trae-global" => Ok(Scope::TraeGlobal),
            other => Err(ScopeError::new(other)),
        }
    }

    /// 稳定标识（进 URL 路径段与序列化）：`"wb-cn"` / `"wb-global"` / `"trae-cn"` / `"trae-global"`。
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::WbCn => "wb-cn",
            Scope::WbGlobal => "wb-global",
            Scope::TraeCn => "trae-cn",
            Scope::TraeGlobal => "trae-global",
        }
    }

    /// 该作用域所属平台。
    pub fn product(self) -> Product {
        match self {
            Scope::WbCn | Scope::WbGlobal => Product::Workbuddy,
            Scope::TraeCn | Scope::TraeGlobal => Product::Trae,
        }
    }

    /// WorkBuddy 侧区域（Trae 作用域调用它返回 `None`）。
    pub fn wb_region(self) -> Option<Region> {
        match self {
            Scope::WbCn => Some(Region::Cn),
            Scope::WbGlobal => Some(Region::Global),
            Scope::TraeCn | Scope::TraeGlobal => None,
        }
    }

    /// Trae 侧区域（WorkBuddy 作用域调用它返回 `None`）。
    pub fn trae_region(self) -> Option<TraeRegion> {
        match self {
            Scope::TraeCn => Some(TraeRegion::Cn),
            Scope::TraeGlobal => Some(TraeRegion::Global),
            Scope::WbCn | Scope::WbGlobal => None,
        }
    }

    /// 全部作用域，供遍历用。
    pub fn all() -> [Scope; 4] {
        [Scope::WbCn, Scope::WbGlobal, Scope::TraeCn, Scope::TraeGlobal]
    }

    /// 由平台 + 该平台的区域标识拼出作用域（**只接受两平台各自合法的区域**）。
    ///
    /// 与 [`Scope::parse`] 的分工：本函数用于**已结构化**的入参（前端路由已知 product 与
    /// region）；`parse` 用于**裸字符串**入口，且是 fail-closed 的唯一判据。
    pub fn from_parts(product: Product, region: &str) -> Result<Scope, ScopeError> {
        let raw = format!("{}-{}", product.slug(), region.trim().to_ascii_lowercase());
        Scope::parse(&raw)
    }
}

/// 作用域解析失败的原因（可读，供命令 / HTTP 通道直接渲染）。
///
/// 字段存**原始输入**而不是「回落后的结果」：错误信息要能让用户看见自己传错了什么，
/// 而不是一句无信息的「解析失败」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeError {
    /// 无法解析的原始输入（可能为空串）。
    raw: String,
}

impl ScopeError {
    /// 构造一条作用域解析错误。
    pub fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
        }
    }

    /// 原始输入。
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// 可读文案（中文，与项目其余错误文案一致）。
    ///
    /// 明确列出**合法取值**：用户看到 `trae-glboal` 被拒时，能立刻对照出正确拼写，
    /// 而不是去猜「支持哪些作用域」。
    pub fn message(&self) -> String {
        let shown = if self.raw.is_empty() {
            "（空）".to_string()
        } else {
            self.raw.clone()
        };
        format!(
            "未知的作用域「{}」：合法取值为 wb-cn / wb-global / trae-cn / trae-global。\
             作用域非法时**拒绝操作**（不会回落到任何默认作用域），以免读写错误的账号库。",
            shown
        )
    }
}

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ScopeError {}

/// ★ 作用域解析的**唯一 fail-closed 出口**（命令 / HTTP 通道共用）。
///
/// 与 [`Scope::parse`] 同一判据；单独命名是为了让「这里是作用域解析点」在代码里
/// **可 grep**（门禁第 9 条据此反例扫描），也便于两个宿主统一 `use`。
pub fn resolve_scope(raw: &str) -> Result<Scope, ScopeError> {
    Scope::parse(raw)
}

/// 解析**可选**作用域：`None`（缺）/ 空 / 未知**一律 `Err`**。
///
/// 为什么 `None` 也是 `Err`：`scope` 是参数化命令的**必填**维度，缺它就无从判断该读哪个库。
/// 若在这里回落默认，就等于允许「调用方忘了传 scope」时静默操作 CN 的库 —— 正是要堵的洞。
/// 需要「可选、缺省即 CN」语义的是**旧 `region` 参数**，那条路走 `Region::parse`，不走这里。
pub fn resolve_scope_opt(raw: Option<&str>) -> Result<Scope, ScopeError> {
    match raw {
        Some(value) => resolve_scope(value),
        None => Err(ScopeError::new("")),
    }
}

/// ★ 命令 / HTTP 通道的**统一入口**：把可选的 `scope` 参数解析为 [`Scope`]（fail-closed）。
///
/// 与 [`resolve_scope_opt`] 的分工：那个返回结构化 [`ScopeError`]（供诊断与单测断言
/// `raw()`），本函数把它拍成宿主可直接下发的**可读文案**。两宿主各写一遍
/// `map_err(|e| e.message())` 会漂移（T03 之前 `parse_region` / `parse_trae_variant`
/// 就是两份同款实现）—— 故把这一步也收敛到 core 一处。
pub fn resolve_scope_param(raw: Option<&str>) -> Result<Scope, String> {
    resolve_scope_opt(raw).map_err(|error| error.message())
}

/// 参数化命令的 **WorkBuddy 分支**：取 WB 区域；非 WB 作用域 ⇒ 可读 `Err`。
///
/// 与 [`Scope::wb_region`] 的分工：那个返回 `Option`（正交轴的自然表达），本函数把它
/// 变成**命令层可下发的错误**——把「这个作用域不属于本命令的平台分支」明确报出来，
/// 而不是静默返回 `None` 让调用方自己猜。错误文案带**合法取值**，便于前端直接渲染。
pub fn require_wb_region(scope: Scope) -> Result<Region, String> {
    scope.wb_region().ok_or_else(|| {
        format!(
            "作用域「{}」不是 WorkBuddy 作用域：该命令的 WorkBuddy 分支只接受 wb-cn / wb-global。",
            scope.as_str()
        )
    })
}

/// 参数化命令的 **Trae 分支**：取 Trae 区域；非 Trae 作用域 ⇒ 可读 `Err`。
///
/// ⚠️ 返回的是**区域**（持久化轴），不是程序位：只有**区域纯**的命令
/// （账号列表 / 备注 / 删除 —— 全部按 `variant.region()` 落到同一本区域库）才应使用它。
/// **程序位敏感**的命令（OAuth 授权线 / 打开数据目录 / 启动客户端 / 签到）必须保留
/// `variant` 参数，不得折叠到区域 —— 否则会把登录态写进错误的客户端。
pub fn require_trae_region(scope: Scope) -> Result<TraeRegion, String> {
    scope.trae_region().ok_or_else(|| {
        format!(
            "作用域「{}」不是 Trae 作用域：该命令的 Trae 分支只接受 trae-cn / trae-global。",
            scope.as_str()
        )
    })
}

/// WorkBuddy 区域 → 作用域（穷举，**无回落**）。
///
/// 回收站条目要冗余记 `origin_scope`（§2.7 的数据结构），而软删除只拿到 **region**；
/// 本函数把区域补成完整作用域。穷举匹配（无 `_ =>`）保证新增区域时**编译失败**，
/// 而不是静默落错作用域。
pub fn scope_of_wb_region(region: Region) -> Scope {
    match region {
        Region::Cn => Scope::WbCn,
        Region::Global => Scope::WbGlobal,
    }
}

/// Trae 区域 → 作用域（穷举，**无回落**）。理由同 [`scope_of_wb_region`]。
pub fn scope_of_trae_region(region: TraeRegion) -> Scope {
    match region {
        TraeRegion::Cn => Scope::TraeCn,
        TraeRegion::Global => Scope::TraeGlobal,
    }
}

// ---------------------------------------------------------------------------
// `ProductSpec` 表（P5 §2.1）—— 平台的**唯一事实来源**
// ---------------------------------------------------------------------------

/// 一个产品线的全部差异（对照 `region::RegionSpec` 的表驱动风格）。
///
/// ## 为什么放 core、且**只写一次**
///
/// 两个宿主（`src-tauri` 与 `buddy-switch-server`）都依赖 core。表数据只在这里写一份，
/// 两宿主只调 [`product_spec`] 查表 —— 这正是「新增平台的**新增命令**注册点收敛到 2 处」
/// 的落点。**任何在宿主里另写一份平台差异（capabilities / slug / 目录名）的做法都是回退。**
///
/// ## 诚实口径（PLAN §2.4.1）
///
/// 本表只收敛「**新增命令**的注册点」；**存量**注册点一个都减不掉（别名转发 ⇒ 旧
/// `generate_handler!` / 旧 `.route` / 旧 ROUTES 条目全保留）。
///
/// ⚠️ 设计 §2.1 还列了一个 `programs` 字段，其数据源（WB 三程序位表）属 **T05**，
/// 且 WB 与 Trae 的程序位是**两个不同的 `ProgramSpec` 类型**（WB 的
/// `programs::ProgramSpec` vs Trae 的 `trae::region::ProgramSpec`，见 §3.5 待明确 #4）。
/// 现在填它只能填错 / 填死代码，故 T02 **不引入 `programs` 字段**，留待 T05 与程序位表
/// 一并定型。
pub struct ProductSpec {
    /// 产品线。
    pub id: Product,
    /// scope 短标识："wb" | "trae"（与前端 `PLATFORM_SLUG` **逐字对齐**）。
    pub slug: &'static str,
    /// 数据目录名：WB 用根（`"."`），Trae 用 `"trae"`（对应 `~/.buddy-switch/<dir_name>/`）。
    pub dir_name: &'static str,
    /// 该平台的作用域（顺序即 UI 展示顺序）。
    pub regions: &'static [Scope],
    /// 平台能力（与前端 `registry.ts` 的 `Capability` **逐字对齐**）。
    pub capabilities: &'static [Capability],
}

impl ProductSpec {
    /// 该平台是否具备某能力。
    pub fn has_capability(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// WorkBuddy 的作用域。
const WB_SCOPES: &[Scope] = &[Scope::WbCn, Scope::WbGlobal];
/// Trae 的作用域。
const TRAE_SCOPES: &[Scope] = &[Scope::TraeCn, Scope::TraeGlobal];

/// WorkBuddy 能力（与 `registry.ts` 的 `workbuddy.capabilities` 逐字对齐）。
const WB_CAPABILITIES: &[Capability] = &[
    Capability::Checkin,
    Capability::Travel,
    Capability::Credits,
    Capability::Oauth,
    Capability::Gateway,
    Capability::AutoRotate,
    Capability::SessionCopy,
];
/// Trae 能力（与 `registry.ts` 的 `trae.capabilities` 逐字对齐）。
const TRAE_CAPABILITIES: &[Capability] = &[
    Capability::Checkin,
    Capability::Credits,
    Capability::Oauth,
    Capability::Gateway,
];

/// WorkBuddy 描述符。
const WB_SPEC: ProductSpec = ProductSpec {
    id: Product::Workbuddy,
    slug: "wb",
    dir_name: ".",
    regions: WB_SCOPES,
    capabilities: WB_CAPABILITIES,
};

/// Trae 描述符。
const TRAE_SPEC: ProductSpec = ProductSpec {
    id: Product::Trae,
    slug: "trae",
    dir_name: "trae",
    regions: TRAE_SCOPES,
    capabilities: TRAE_CAPABILITIES,
};

/// 取产品线的描述符（**两宿主的唯一查表入口**）。
pub fn product_spec(product: Product) -> &'static ProductSpec {
    match product {
        Product::Workbuddy => &WB_SPEC,
        Product::Trae => &TRAE_SPEC,
    }
}

/// 按作用域取描述符（`product_spec(scope.product())`）。
pub fn product_spec_for_scope(scope: Scope) -> &'static ProductSpec {
    product_spec(scope.product())
}

/// ★ capability 校验（**默认拒绝**）—— P5 §2.5 第 (2) 条。
///
/// 命令分发层（`src-tauri/src/commands.rs` / `buddy-switch-server/src/api.rs`）的每个
/// 涉及能力的入口，**首行**调用本函数；缺能力即 `Err`。
///
/// ## 为什么必须在这里拦（而不是靠 UI 隐藏按钮）
///
/// Tauri 自定义命令对 webview **全可达**：UI 不渲染按钮 ≠ 命令调不到。Trae 侧既有的
/// `trae/platform.rs::capabilities()` 只是**诊断用的 unsupported 列表**，**不是门控**。
/// 若漏配能力却只在 UI 隐藏，攻击面 / 误调用仍能直达命令 ⇒ 必须**默认拒绝**。
pub fn require_capability(product: Product, capability: Capability) -> Result<(), String> {
    let spec = product_spec(product);
    if spec.has_capability(capability) {
        Ok(())
    } else {
        Err(format!(
            "{} 不具备「{}」能力，已拒绝该操作（capability 默认拒绝：UI 隐藏 ≠ 命令不可调）。",
            spec.id.as_str(),
            capability.as_str(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_slug_matches_frontend_registry() {
        // 与 `src/lib/platforms/registry.ts` 的 `PLATFORM_SLUG` 逐字对齐。
        assert_eq!(Product::Workbuddy.slug(), "wb");
        assert_eq!(Product::Trae.slug(), "trae");
        assert_eq!(Product::Workbuddy.as_str(), "workbuddy");
        assert_eq!(Product::Trae.as_str(), "trae");
        assert_eq!(Product::all().len(), 2);
    }

    #[test]
    fn scope_parse_accepts_the_four_known_scopes() {
        for scope in Scope::all() {
            assert_eq!(Scope::parse(scope.as_str()), Ok(scope), "{}", scope.as_str());
        }
        assert_eq!(Scope::parse("wb-cn"), Ok(Scope::WbCn));
        assert_eq!(Scope::parse("wb-global"), Ok(Scope::WbGlobal));
        assert_eq!(Scope::parse("trae-cn"), Ok(Scope::TraeCn));
        assert_eq!(Scope::parse("trae-global"), Ok(Scope::TraeGlobal));
    }

    /// ★★ **本用例就是门禁第 9 条要找的「未知 scope ⇒ `Err`」证明**（函数名与
    /// `scripts/check-api-contract.cjs` 第 9 条的正向断言**逐字对应**，勿改名）。
    ///
    /// 可证伪：把 `Scope::parse` 改成 `unwrap_or(Scope::WbCn)` 式回落（或加一条
    /// 「无法匹配 ⇒ 默认」），本用例立即变红 —— 那正是「拼错 `trae-glboal` 静默读写
    /// `trae-cn` 的库」的缺陷形态。
    #[test]
    fn scope_parse_is_fail_closed_for_unknown_empty_and_typos() {
        // 未知 / 拼错：必须 Err，**绝不**回落到任何默认作用域。
        for bad in [
            "trae-glboal", // 真实拼错（本模块存在的理由）
            "wb-gloabl",
            "trae",
            "wb",
            "cn",
            "global",
            "workbuddy-cn",
            "trae_global",
            "",
            "   ",
            "wb-cn ",   // 尾随空白：不 trim ⇒ Err（与前端 parseScope 一致）
            " wb-cn",   // 前导空白
            "WB-CN",    // 大写：不折叠 ⇒ Err
            "Trae-Cn",  // 混合大小写
            "wb-cn\n",  // 换行
        ] {
            let got = Scope::parse(bad);
            assert!(
                got.is_err(),
                "非法作用域 {bad:?} 竟然解析成 {got:?} —— 违反 fail-closed（会读写错误的库）"
            );
            // 错误信息必须带上原始输入，便于用户对照正确拼写。
            assert_eq!(got.unwrap_err().raw(), bad);
        }
    }

    #[test]
    fn scope_error_message_lists_legal_values_and_marks_empty() {
        let err = Scope::parse("trae-glboal").unwrap_err();
        let message = err.message();
        assert!(message.contains("trae-glboal"), "{message}");
        assert!(message.contains("wb-cn"), "{message}");
        assert!(message.contains("trae-global"), "{message}");

        // 空输入在文案里要显式标出，而不是留一段空白。
        let empty = Scope::parse("").unwrap_err();
        assert!(empty.message().contains("（空）"), "{}", empty.message());
    }

    #[test]
    fn resolve_scope_delegates_and_resolve_scope_opt_rejects_missing() {
        assert_eq!(resolve_scope("wb-global"), Ok(Scope::WbGlobal));
        assert!(resolve_scope("nope").is_err());

        // ★ 缺（`None`）也是 Err —— scope 是必填维度，缺它无从判断该读哪个库。
        assert!(resolve_scope_opt(None).is_err());
        assert!(resolve_scope_opt(Some("")).is_err());
        assert_eq!(resolve_scope_opt(Some("trae-cn")), Ok(Scope::TraeCn));
    }

    /// `resolve_scope_param`（命令/HTTP 通道统一入口）—— fail-closed，错误文案可读。
    ///
    /// 可证伪：若把它改成 `unwrap_or(Scope::WbCn)` 式回落，第一条断言立即变红。
    #[test]
    fn resolve_scope_param_is_fail_closed_and_carries_readable_message() {
        assert_eq!(resolve_scope_param(Some("trae-global")), Ok(Scope::TraeGlobal));
        // 缺 / 空 / 拼错一律 Err（与 `resolve_scope_opt` 同一判据）。
        for bad in [None, Some(""), Some("trae-glboal"), Some("WB-CN")] {
            let error = resolve_scope_param(bad).unwrap_err();
            assert!(
                error.contains("wb-cn") && error.contains("trae-global"),
                "错误文案必须列出合法取值，实际：{error}"
            );
        }
    }

    /// `require_wb_region` / `require_trae_region` —— 平台分支取区域，跨平台作用域 ⇒ Err。
    ///
    /// 可证伪：若把 `require_wb_region` 改成「非 WB 就回落 `Region::Cn`」，
    /// 第二条断言立即变红 —— 那正是「用 A 平台的作用域去打 B 平台的库」的缺陷形态。
    #[test]
    fn require_region_helpers_reject_cross_platform_scopes() {
        // 正例：本平台作用域取到正确的区域。
        assert_eq!(require_wb_region(Scope::WbCn), Ok(Region::Cn));
        assert_eq!(require_wb_region(Scope::WbGlobal), Ok(Region::Global));
        assert_eq!(require_trae_region(Scope::TraeCn), Ok(TraeRegion::Cn));
        assert_eq!(require_trae_region(Scope::TraeGlobal), Ok(TraeRegion::Global));

        // 反例：跨平台作用域必须 Err（绝不静默落到某平台的默认区域）。
        for scope in [Scope::TraeCn, Scope::TraeGlobal] {
            assert!(require_wb_region(scope).is_err(), "{:?}", scope);
        }
        for scope in [Scope::WbCn, Scope::WbGlobal] {
            assert!(require_trae_region(scope).is_err(), "{:?}", scope);
        }
        // 错误文案可读、且带上原始作用域。
        let error = require_wb_region(Scope::TraeCn).unwrap_err();
        assert!(error.contains("trae-cn"), "{error}");
    }

    /// `scope_of_*_region` 与 [`Scope`] 的两条正交轴**往返一致**（回收站 `origin_scope` 用）。
    ///
    /// 可证伪：把某个区域映射到**另一个平台**的作用域 ⇒ 本用例变红 ——
    /// 那正是「回收站条目记错来源作用域」的形态。
    #[test]
    fn scope_of_region_helpers_round_trip_with_scope_axes() {
        for region in Region::all() {
            let scope = scope_of_wb_region(region);
            assert_eq!(scope.product(), Product::Workbuddy);
            assert_eq!(scope.wb_region(), Some(region));
            assert_eq!(scope.trae_region(), None);
        }
        for region in TraeRegion::all() {
            let scope = scope_of_trae_region(region);
            assert_eq!(scope.product(), Product::Trae);
            assert_eq!(scope.trae_region(), Some(region));
            assert_eq!(scope.wb_region(), None);
        }
        assert_eq!(scope_of_wb_region(Region::Cn), Scope::WbCn);
        assert_eq!(scope_of_wb_region(Region::Global), Scope::WbGlobal);
        assert_eq!(scope_of_trae_region(TraeRegion::Cn), Scope::TraeCn);
        assert_eq!(scope_of_trae_region(TraeRegion::Global), Scope::TraeGlobal);
    }

    #[test]
    fn scope_exposes_product_and_region_axes() {
        assert_eq!(Scope::WbCn.product(), Product::Workbuddy);
        assert_eq!(Scope::WbGlobal.product(), Product::Workbuddy);
        assert_eq!(Scope::TraeCn.product(), Product::Trae);
        assert_eq!(Scope::TraeGlobal.product(), Product::Trae);

        // WB 轴：Trae 作用域取 WB 区域返回 None（正交）。
        assert_eq!(Scope::WbCn.wb_region(), Some(Region::Cn));
        assert_eq!(Scope::WbGlobal.wb_region(), Some(Region::Global));
        assert_eq!(Scope::TraeCn.wb_region(), None);
        assert_eq!(Scope::TraeGlobal.wb_region(), None);

        // Trae 轴：WB 作用域取 Trae 区域返回 None。
        assert_eq!(Scope::TraeCn.trae_region(), Some(TraeRegion::Cn));
        assert_eq!(Scope::TraeGlobal.trae_region(), Some(TraeRegion::Global));
        assert_eq!(Scope::WbCn.trae_region(), None);
        assert_eq!(Scope::WbGlobal.trae_region(), None);
    }

    #[test]
    fn from_parts_round_trips_all_scopes() {
        for scope in Scope::all() {
            let product = scope.product();
            let region = scope
                .wb_region()
                .map(|r| r.as_str())
                .or_else(|| scope.trae_region().map(|r| r.as_str()))
                .expect("每个作用域必有区域");
            assert_eq!(Scope::from_parts(product, region), Ok(scope));
        }
        // 平台与区域不匹配（WB 平台 + 不存在的区域）⇒ Err，不猜。
        assert!(Scope::from_parts(Product::Workbuddy, "sg").is_err());
    }

    // ── ProductSpec 表 + capability 默认拒绝 ──────────────────────────────────

    /// 前端注册表（**跨语言护栏**的读取对象）。
    const REGISTRY_TS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/lib/platforms/registry.ts"
    );

    fn read_registry_ts() -> String {
        std::fs::read_to_string(REGISTRY_TS)
            .unwrap_or_else(|error| panic!("读不到 {REGISTRY_TS}: {error}"))
    }

    /// 从 `registry.ts` 的某平台描述符里提取 `capabilities: [ … ]` 的字面量列表。
    fn ts_capability_list(ts: &str, product_key: &str) -> Vec<String> {
        let anchor = ts
            .find(&format!("{product_key}: {{"))
            .unwrap_or_else(|| panic!("registry.ts 找不到 `{product_key}: {{`"));
        let rest = &ts[anchor..];
        let cap_idx = rest
            .find("capabilities:")
            .unwrap_or_else(|| panic!("{product_key} 描述符缺 capabilities"));
        let after = &rest[cap_idx..];
        let open = after.find('[').expect("capabilities 不是数组字面量");
        let close = after.find(']').expect("capabilities 数组未闭合");
        after[open + 1..close]
            .split(',')
            .map(|item| item.trim().trim_matches('"').to_string())
            .filter(|item| !item.is_empty())
            .collect()
    }

    /// 从 `registry.ts` 的 `PLATFORM_SLUG` 表里取某平台的短标识。
    fn ts_platform_slug(ts: &str, product_key: &str) -> String {
        let anchor = ts
            .find("PLATFORM_SLUG")
            .expect("registry.ts 找不到 PLATFORM_SLUG");
        let block = &ts[anchor..];
        let open = block.find('{').expect("PLATFORM_SLUG 不是对象字面量");
        let close = block.find('}').expect("PLATFORM_SLUG 未闭合");
        let inner = &block[open + 1..close];
        let key_idx = inner
            .find(&format!("{product_key}:"))
            .unwrap_or_else(|| panic!("PLATFORM_SLUG 缺 {product_key}"));
        let after = &inner[key_idx..];
        let q1 = after.find('"').expect("slug 不是字符串字面量");
        let q2 = after[q1 + 1..].find('"').expect("slug 字符串未闭合") + q1 + 1;
        after[q1 + 1..q2].to_string()
    }

    /// ★★ **跨语言护栏**：`ProductSpec` 的 `slug` 与 `capabilities` 必须与前端
    /// `src/lib/platforms/registry.ts` **逐字对齐**（P5 §2.1 / T02 验收）。
    ///
    /// 可证伪：把 `WB_CAPABILITIES` 删掉一项、或把 `slug` 改成 `"workbuddy"`，
    /// 本用例立即变红 —— 那正是「两侧各写一份、悄悄漂移」的缺陷形态。
    #[test]
    fn product_spec_matches_frontend_registry_slug_and_capabilities() {
        let ts = read_registry_ts();
        for product in Product::all() {
            let spec = product_spec(product);
            let key = product.as_str();

            assert_eq!(
                spec.slug,
                ts_platform_slug(&ts, key),
                "{key} 的 ProductSpec.slug 与前端 PLATFORM_SLUG 漂移"
            );

            let want: Vec<String> = spec
                .capabilities
                .iter()
                .map(|c| c.as_str().to_string())
                .collect();
            assert_eq!(
                want,
                ts_capability_list(&ts, key),
                "{key} 的 ProductSpec.capabilities 与前端 registry.ts 漂移（顺序也须一致）"
            );
        }
    }

    /// ★ capability **默认拒绝**：缺能力 ⇒ `Err`（P5 §2.5 第 (2) 条 · 验收 5）。
    ///
    /// 可证伪：若 `require_capability` 改成「查不到就放行」或「回落某个默认能力」，
    /// 本用例的 `is_err()` 断言立即变红。
    #[test]
    fn require_capability_is_default_deny() {
        // Trae 没有 travel / autoRotate / sessionCopy ⇒ 必须 Err。
        for missing in [
            Capability::Travel,
            Capability::AutoRotate,
            Capability::SessionCopy,
        ] {
            assert!(
                require_capability(Product::Trae, missing).is_err(),
                "Trae 缺「{}」却放行了 —— 违反默认拒绝",
                missing.as_str()
            );
        }
        // Trae 有 checkin / credits / oauth / gateway ⇒ Ok。
        for present in [
            Capability::Checkin,
            Capability::Credits,
            Capability::Oauth,
            Capability::Gateway,
        ] {
            assert!(require_capability(Product::Trae, present).is_ok());
        }
        // WorkBuddy 有全部 7 个能力 ⇒ 全部 Ok。
        for cap in Capability::all() {
            assert!(
                require_capability(Product::Workbuddy, cap).is_ok(),
                "WorkBuddy 缺「{}」",
                cap.as_str()
            );
        }
        // 错误信息可读、且带上能力名（前端可直接渲染）。
        let error = require_capability(Product::Trae, Capability::Travel).unwrap_err();
        assert!(error.contains("travel"), "{error}");
        assert!(error.contains("trae"), "{error}");
    }

    /// `product_spec_for_scope` 委派正确 + 目录名与区域表符合实情。
    #[test]
    fn product_spec_for_scope_delegates_and_dir_names_are_correct() {
        for scope in Scope::all() {
            assert_eq!(product_spec_for_scope(scope).id, scope.product());
        }
        // 目录名：WB 用根（"."），Trae 用 "trae"（见 core `trae::paths::trae_dir`）。
        assert_eq!(product_spec(Product::Workbuddy).dir_name, ".");
        assert_eq!(product_spec(Product::Trae).dir_name, "trae");
        // 区域表。
        assert_eq!(
            product_spec(Product::Workbuddy).regions,
            &[Scope::WbCn, Scope::WbGlobal]
        );
        assert_eq!(
            product_spec(Product::Trae).regions,
            &[Scope::TraeCn, Scope::TraeGlobal]
        );
        // 每个平台声明的作用域都能被 `Scope::parse` 往返（表与解析器不漂移）。
        for scope in Scope::all() {
            assert_eq!(Scope::parse(scope.as_str()), Ok(scope));
        }
    }
}
