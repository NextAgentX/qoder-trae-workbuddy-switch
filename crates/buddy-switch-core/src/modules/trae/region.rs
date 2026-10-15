//! Trae 的两个**正交维度**：区域（region）与程序（program）。
//!
//! ## 为什么需要这一层（现状缺陷）
//!
//! 改造前只有一根轴（[`super::variant::TraeVariant`]），两个取值
//! `TraeWork` / `Trae` **都是国内构建**，于是：
//!
//! - `data_dir_names` 把国内与国外的候选名混在同一张表里
//!   （`["TRAE SOLO CN", "TRAE SOLO"]`、`["Trae CN", "Trae"]`），并按顺序取
//!   **第一个存在的**目录 ⇒ 装了国际版也永远探测不到，用户在界面上看不见、管不了；
//! - 「一条程序」与「一个账号体系」被混为一谈：国内的两条程序其实共用
//!   `api.trae.cn` 同一套账号，却各自维护一份账号库。
//!
//! ## 两根轴各自负责什么
//!
//! | 轴 | 取值 | 决定 |
//! |:---|:---|:---|
//! | [`TraeRegion`] | `Cn` / `Global` | **持久化**：账号库 / 登录态快照 / 设备标识 / 冷却 / 端点。即「账号属于哪套账号体系」 |
//! | [`TraeProgram`] | `TraeWork` / `TraeCode` | **执行**：把登录态写进哪个客户端目录、启动哪个 exe |
//!
//! 关键是**持久化轴是区域而不是程序**：CN 与国际是两套互不相通的账号体系
//! （JWT 不通用，端点不同），而同一区域内的两条程序共用同一套账号 ——
//! 同一个 Trae 账号可以分别启用在国内的 TraeWork 与 TraeCode 上。
//!
//! ## 程序位（program instance）与其官方名
//!
//! 取值来自**两处权威来源**（本机 2026-09-21 实测），不是我们起的名字：
//!
//! - 客户端 `<安装根>\resources\app\product.json` 的 `nameAlias` / `packageType`；
//! - 系统注册表 `Uninstall` 项的 `DisplayName`（用户在「应用和功能」里看到的名字）。
//!
//! | 区域 | 程序 | userData 目录 | exe / 进程 | `nameAlias` | `packageType` | 注册表显示名 |
//! |:---|:---|:---|:---|:---|:---|:---|
//! | CN | TraeWork | `TRAE SOLO CN` | `TRAE SOLO CN.exe` | `TraeWork CN` | `SOLO_CN` | TraeWork CN (User) |
//! | CN | TraeCode | `Trae CN` | `Trae CN.exe` | `TraeCode CN` | `TRAE_CN` | TraeCode CN (User) |
//! | CN | **Trae**（合并版） | `Trae CN` | `Trae CN.exe` | `TraeCode CN` | `TRAE_CN` | TraeCode CN (User) |
//! | Global | TraeWork | `TRAE SOLO` | `TRAE SOLO.exe` | `TraeWork` | `SOLO_I18N` | TraeWork (User) |
//! | Global | TraeCode | `Trae` | `Trae.exe` | `TraeCode`（待实测） | `TRAE_I18N`（推断，见下） | 本机未安装 |
//! | Global | **Trae**（合并版） | `Trae` | `Trae.exe` | `TraeCode`（待实测） | `TRAE_I18N`（推断，见下） | 本机未安装 |
//!
//! ⚠️ **候选名必须互不重叠**：`TRAE SOLO CN` 与 `TRAE SOLO` 只差一个后缀，
//! `Trae CN` 与 `Trae` 是包含关系。用「包含匹配」判程序位会互相抢
//! （出现「装了 A、读了 B 的目录」），因此 [`ProgramSpec`] 的候选名一律
//! **精确逐字比对**，不重叠由单测钉住。
//!
//! ### 历史重叠：`Trae`（合并版）× `TraeCode`（issue #20）—— 两格均已退役
//!
//! 2026-10-13 客户端把 TraeWork 与 TraeCode 合并成**一个**客户端，落点就是
//! `Trae CN` / `Trae` —— 与旧的 **TraeCode** 逐字同名（本机 `migration-job.json`
//! 实证：`sourceAppName = "TRAE SOLO CN"`、`targetDataPath = …\Trae CN`）。
//! 也就是说 `Trae` 与 `TraeCode` **物理上是同一个客户端**。
//!
//! issue #20 曾把两者都列为程序位（刻意共享候选名）；**2026-10-15 按用户决策
//! 全部撤下**：界面只保留**合并版**一条程序位（见 [`programs_of`]）——
//! 「一个客户端摆两个按钮」没有信息量，只会让人疑惑点哪个。
//! 遗留 `TraeCode` 的**数据**（快照目录 `profiles_trae_cn/`、绑定了该变体的
//! Key、记账）一律**原样保留**：变体表（[`super::variant`]）与
//! `program_spec(region, TraeCode)` 查询都还在，旧版本程序按原布局读写不受影响。
//!
//! ## 程序位收敛（2026-10-15，用户决策）
//!
//! 程序位表收敛为**每区域一个**：合并版客户端（国内 `Trae CN` / 国际 `Trae`）。
//! TraeWork（`TRAE SOLO CN`）与遗留 TraeCode 两格从界面退役 —— 它们的**数据**
//! （快照目录、账号库记录、记账文件）不迁移、不删除：
//!
//! - TraeWork 的快照在 `profiles/`（默认变体目录），userData 在 `%APPDATA%\TRAE SOLO CN`
//!   —— 新版本只是不再在界面上提供这一格，文件原样在原处；
//! - 遗留 TraeCode 的快照在 `profiles_trae_cn/`，既有迁移（`region_migrate`）会把
//!   可搬的搬进合并版槽位，搬不动的（客户端级目录名相同）天然兼容；
//! - **旧版本的本项目**按原布局读写全部这些文件，不受本次收敛影响（布局零变化）。
//!
//! 变体轴（[`super::variant::TraeVariant`] 的 6 个取值）**保持不变** —— 变体决定
//! 快照目录 / Key 归属 / 日志分档，退役的只是「程序位」这一层界面入口。
//!
//! ## 程序位是否都真的接上了（2026-09-28 issue #3；2026-10-13 issue #20）
//!
//! 历史：本模块曾有 4 个 [`ProgramSpec`]，第 4 个（Global × TraeCode）一度接不上
//! （`platform::variant_for_program` 返回 `None`，界面上永远是「未检测到」）；
//! issue #3 修复后 6 个组合（3 程序 × 2 区域）都映射到
//! [`super::variant::TraeVariant`] 的一个取值。2026-10-15 程序位表收敛后，
//! **界面程序位**只剩每区域一个（合并版），但全部 6 个组合的映射
//! （[`program_spec`] / [`super::platform::variant_for_program`]）**仍然保留** ——
//! 旧数据与旧版本程序的兼容面一点没少。
//!
//! ⚠️ 唯一**推断**（非实测）的是 `TraeCode` 国际版的 `packageType`：本机未装该客户端。
//! 它只被 [`super::variant::OAuthLine::from_package_type`] 消费，而那个分派
//! **只区分 `SOLO_*` 与非 `SOLO_*`** ⇒ 任何非 `SOLO_*` 取值都落 TRAE 线（IDE 该用的
//! 那条钥匙），所以推断错也**不会**把请求打到错的域或换错钥匙。
//!
//! ## 展示名：**国内版带 ` CN` 后缀、国际版不带后缀**
//!
//! ★ **六个名字全部取自客户端自己的字符串**，逐个可核（`product.json` 的
//! `nameShort` / `nameAlias`）：
//!
//! | 程序位 | 国内版 | 来源 | 国际版 | 来源 |
//! |:---|:---|:---|:---|:---|
//! | TraeWork | `TraeWork CN` | `TRAE SOLO CN` 的 `nameAlias`（实测，见 [`super::variant`] 模块头） | `TraeWork` | 国际 TraeWork 客户端的 `nameAlias`（**本机实测**） |
//! | TraeCode | `TraeCode CN` | `Trae CN` 客户端的 `nameAlias`（**本机实测**） | `TraeCode` | 按「国际不带后缀」规则**推断**（本机未装国际 TraeCode） |
//! | Trae（合并版） | `Trae CN` | 合并客户端的 `nameShort`（**本机实测**） | `Trae` | **用户确认**（2026-10-13） |
//!
//! ⚠️ 只有「国际 TraeCode」那一格是**推断**（去掉 ` CN` 后缀），因为它没有可读的
//! 客户端；其余五格都有实测字段或用户确认。推断依据是国际 TraeWork 的
//! `nameAlias` 实测为 `TraeWork`（不带后缀）⇒ 同族规则。
//!
//! ⚠️ 更正记录（2026-10-13，用户更正）：此前的约定是「**国际版带 `AI` 后缀**」，
//! 那是**错的** —— 它凭空造了 `Trae AI` / `TraeWork AI` 这类客户端根本不用的名字，
//! 而且 `Trae AI` 会同时指国际 TraeCode 与国际合并版（撞名）。
//! 六个名字**两两不同**有两条护栏（本模块的 `在役程序位的展示名两两不同` +
//! 跨语言门禁 `check-scope-registry.cjs`，后者还要求与前端兜底表逐字一致）。
//! 2026-10-15 收敛后本模块只**在役** 2 个程序位名；全部 6 个变体名仍由
//! [`super::variant::TraeVariant::display_name`] 保留（供旧数据/旧版本兼容）。
//!
//! ## 区域名：**客户端自己的产品名**（用户 2026-10-13 定）
//!
//! [`TraeRegion::display_name`] 返回 **`Trae CN`**（国内）/ **`Trae`**（国际）——
//! 逐字取客户端在该区域的 `nameShort`（本机 `product.json` 实测）。
//!
//! ⚠️ **它与合并版程序位的展示名逐字相同**（`Trae CN` / `Trae`）—— 客户端的产品名
//! 本就同时指这两件事，用户明确选择如此。它们分属**区域轴**与**程序位轴**：
//! ★ 别把它们并进「六个展示名两两不同」那条护栏，★ 也别去重。
//!
//! ⚠️ 它的**已知成本**：文本层面再也无法用 `Trae CN` 当「程序位名」的标记 ——
//! `oauth` 的「成功页不得出现程序位名」断言因此收窄到 `TraeWork CN` / `TraeCode CN`
//! （见该用例的说明）。要恢复强度只能让区域名与程序位名不同。
//!
//! ⚠️ **组合**标签（作用域芯片 = 平台 + 区域词，见前端 `scope-chip.tsx`）用的是
//! **跨平台**的区域**词**（`shared.region.version.*`，WorkBuddy 也读它）⇒
//! 本值塞进那个组合会得到「Trae Trae CN」。Trae 侧因此另有独立词条
//! （前端 `trae.region.*`），本函数只服务 Trae 自己的文案。
//!
//! ## 与 [`super::variant`] 的关系（过渡期）
//!
//! 区域端点**不在这里另立一份**：本模块的 [`region_endpoints`] 直接读
//! [`super::variant::variant_spec`] 里那张已实测/已更正的端点表，
//! 避免同一事实出现两个来源。等调用点全部迁到区域轴后，端点表本身也会搬进来。

use super::variant::{variant_spec, EndpointSet, TraeVariant};

/// 账号体系所在区域。
///
/// 它是**持久化轴**：账号库文件后缀、登录态快照目录、设备标识、冷却账本、
/// 以及全部联网端点都由它决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraeRegion {
    /// 国内版（`api.trae.cn` / `api.trae.com.cn`，账号体系与国内主体一致）。
    Cn,
    /// 国际版（客户端 `packageType = SOLO_I18N`，发布者 SPRING (SG) PTE. LTD）。
    Global,
}

impl TraeRegion {
    /// 稳定标识（文件名后缀、序列化、前端传参）。
    pub fn as_str(self) -> &'static str {
        match self {
            TraeRegion::Cn => "cn",
            TraeRegion::Global => "global",
        }
    }

    /// Trae 侧的**区域名**（用户 2026-10-13 定：用客户端自己的产品名）。
    ///
    /// 取客户端在该区域的 `nameShort`（本机 `product.json` 实测）：
    /// 国内版客户端叫 **`Trae CN`**、国际版叫 **`Trae`**（见 [`super::variant`] 模块头）。
    ///
    /// ## ⚠️ 已知的**刻意重名**（用户选择，不是事故）
    ///
    /// 这两个名字与**合并版程序位的展示名**逐字相同（`Trae CN` / `Trae`）——
    /// 因为客户端的产品名本来就同时指这两件事。它们分属两根轴
    /// （**区域** vs **程序位**），因此：
    ///
    /// - ★ **不要**把它们并进「六个展示名两两不同」那条护栏 —— 那条只管程序位轴；
    /// - ⚠️ 判据层面有真实代价：`oauth` 的「成功页不得出现程序位名」那条断言
    ///   原先用 `Trae CN` 当标记，现在它同时是区域名 ⇒ 只能用 `TraeWork CN` /
    ///   `TraeCode CN` 当标记（见该用例的说明）。**这是本命名的已知成本。**
    ///
    /// ## ⚠️ 它**只用于 Trae 侧**
    ///
    /// 前端的**组合**标签（作用域芯片「平台 + 区域」，见 `scope-chip.tsx`）用的是
    /// **跨平台**的区域词（`shared.region.version.*`，WorkBuddy 也读它）。
    /// 把本值塞进那个组合会得到「Trae Trae CN」——
    /// 因此 Trae 侧另有一对独立词条（`trae.region.*`），本函数只服务 Trae 自己的文案。
    pub fn display_name(self) -> &'static str {
        match self {
            TraeRegion::Cn => "Trae CN",
            TraeRegion::Global => "Trae",
        }
    }

    /// 从字符串解析区域，**宽容**并兼容历史取值。
    ///
    /// ⚠️ 历史值 `trae_work` / `trae_cn` **都映射到 [`TraeRegion::Cn`]**：
    /// 它们是改造前的「产品线」标识，而那两个产品线**都是国内构建**。
    /// 老前端/老链接带着这两个值进来时必须落到国内版，**不能**落到国际版 ——
    /// 否则会在国际版里读一个空账号库，用户看到的是「账号凭空消失」。
    pub fn parse(s: &str) -> Option<TraeRegion> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cn" | "china" | "domestic" | "trae_work" | "trae_cn" | "traework" | "trae" => {
                Some(TraeRegion::Cn)
            }
            "global" | "intl" | "international" | "sg" | "ai" => Some(TraeRegion::Global),
            _ => None,
        }
    }

    /// 全部区域（遍历用，顺序稳定）。
    pub fn all() -> [TraeRegion; 2] {
        [TraeRegion::Cn, TraeRegion::Global]
    }
}

impl Default for TraeRegion {
    /// 默认国内版：与改造前的默认变体（`trae_work`）落在**同一个账号库**上，
    /// 因此「不传 region」对老调用点的语义就是「行为与升级前一模一样」。
    fn default() -> Self {
        TraeRegion::Cn
    }
}

/// Trae 的两条程序线。
///
/// 它是**执行轴**：只决定「登录态写进哪个客户端目录、启动哪个 exe」，
/// 不参与账号归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraeProgram {
    /// TraeWork 程序线（客户端 `TRAE SOLO*`，官方别名 `TraeWork*`）。
    ///
    /// 自 2026-10-13 起客户端已把它**并入** [`TraeProgram::Trae`]；本取值只服务
    /// **未升级的老版本**（本机 `D:\Programs\TRAE SOLO CN\` 已被卸载、只剩 debug.log）。
    TraeWork,
    /// Trae 代码编辑器程序线（客户端 `Trae CN` / `Trae`，官方别名 `TraeCode*`）。
    ///
    /// ## 它与 [`TraeProgram::Trae`] 是**两条不同的程序位**，不是别名关系
    ///
    /// 用户确认的表述：**`TraeCode` 是 `TraeCode`，`Trae` 是 `Trae`（Work 与 Code 的
    /// 合并版）**。两者在界面上是两枚独立按钮、两个独立标识（`trae_code` / `trae`），
    /// 各有各的语义。
    ///
    /// ⚠️ 它们**候选名逐字相同**（`Trae CN` / `Trae`）纯属客户端的历史选择 ——
    /// 合并版复用了 TraeCode 的目录名与 exe 名。因此在**已合并**的机器上两条程序位
    /// 探测到的是同一个物理客户端；而在**未合并**的机器上，本取值就是那条遗留 IDE 客户端。
    ///
    /// ## ★ 但两者有**各自独立**的变体与快照目录（issue #20 收尾）
    ///
    /// 变体是快照目录 / Key 归属 / 网关日志分档的唯一派生来源。两条程序位共用变体时，
    /// 它们的**登录态快照会挤进同一个目录**（用户报障原文：「Trae 与 TraeCode 目前仍
    /// 共用同一个快照目录」）。因此合并版有自己的标识：`trae_merged`（CN）/
    /// `global_trae_merged`（国际），快照目录相应为 `profiles_trae_merged` /
    /// `profiles_global_trae_merged`。
    /// ⇒ `变体 ↔ (区域, 程序位)` 是**双射**（见 `client_settings::variant_for_program` 的单测）。
    TraeCode,
    /// **合并后的单一客户端**（issue #20）：CN `Trae CN` / 国际 `Trae`。
    ///
    /// 它是 TraeWork 与 TraeCode **两条产品线合并的产物**，与 [`TraeProgram::TraeCode`]
    /// 是两条不同的程序位（见该取值的说明）。
    ///
    /// ## 依据（本机实测，不是推断）
    ///
    /// 客户端 2026-10-13 把 TraeWork 与 TraeCode 合并成一条产品线，并把旧数据迁进
    /// 合并后的客户端。证据是 `%APPDATA%\Trae CN\migration-job.json`（逐字）：
    ///
    /// ```json
    /// { "sourceDataPath": "…\\AppData\\Roaming\\TRAE SOLO CN",
    ///   "sourceInstallPath": "D:\\Programs\\TRAE SOLO CN",
    ///   "sourceAppName": "TRAE SOLO CN",
    ///   "targetDataPath": "…\\AppData\\Roaming\\Trae CN",
    ///   "removeApp": true, "removeData": false, "status": "completed" }
    /// ```
    ///
    /// 即：`TRAE SOLO CN`（TraeWork）被卸载、数据并入 `Trae CN`。
    ///
    /// ## ⚠️ 它与 [`TraeProgram::TraeCode`] 候选名逐字相同 —— 这是客户端的历史选择
    ///
    /// 合并后的客户端复用了 TraeCode 的目录名 / exe 名（`Trae CN` / `Trae`），
    /// 因此两者**无法用名字区分**。但它们是**两条不同的程序位**（用户确认：
    /// 「TraeCode 是 TraeCode，Trae 是 Trae（Work 与 Code 的合并版）」）——
    /// 界面上是两枚独立按钮、两个独立标识，只是共享同一份持久化身份与快照目录
    /// （那由「同一台机器上它们是同一个物理客户端」这一事实决定）。
    /// 本取值是**面向用户的正确名字**：客户端现在就叫 `Trae`，不再叫 `TraeCode`。
    Trae,
}

impl TraeProgram {
    /// 稳定标识。
    pub fn as_str(self) -> &'static str {
        match self {
            TraeProgram::TraeWork => "trae_work",
            TraeProgram::TraeCode => "trae_code",
            TraeProgram::Trae => "trae",
        }
    }

    /// 从字符串解析程序线（宽容，接受客户端 exe / 目录名的常见写法）。
    pub fn parse(s: &str) -> Option<TraeProgram> {
        let lowered = s.trim().to_ascii_lowercase();
        let lowered = lowered.strip_suffix(".exe").unwrap_or(&lowered);
        if lowered.contains("solo") || lowered.contains("work") {
            return Some(TraeProgram::TraeWork);
        }
        // `code` 必须**先于**裸 `trae` 判定：`trae_code` 是遗留 IDE 程序位的标识，
        // 而裸 `trae` 自 issue #20 起指合并后的客户端 —— 两者是**不同槽位**
        // （虽然物理上同一个客户端），顺序写反会让 `trae_code` 落进 `Trae`。
        if lowered.contains("code") {
            return Some(TraeProgram::TraeCode);
        }
        // 裸 `trae`（含 `trae cn` / `trae cn.exe`）＝ 合并后的客户端。
        // 客户端自己的目录名/ exe 名就是这个，故它是最常见的输入。
        if lowered == "trae" || lowered == "trae cn" {
            return Some(TraeProgram::Trae);
        }
        // 其余含 `trae` 的历史写法（`trae_cn` / `global_trae_code` 已在上面的
        // `code` 分支处理）一律落遗留 TraeCode，**不猜**成合并版。
        if lowered.contains("trae") {
            return Some(TraeProgram::TraeCode);
        }
        None
    }

    /// 全部程序线（遍历用，顺序稳定：遗留两条在前，合并版在后）。
    pub fn all() -> [TraeProgram; 3] {
        [
            TraeProgram::TraeWork,
            TraeProgram::TraeCode,
            TraeProgram::Trae,
        ]
    }
}

/// 一个**程序位**（区域 × 程序）的全部差异。
#[derive(Debug, Clone, Copy)]
pub struct ProgramSpec {
    /// 所属区域。
    pub region: TraeRegion,
    /// 所属程序线。
    pub program: TraeProgram,
    /// 界面展示名（**国内版带 ` CN` 后缀、国际版不带后缀**），见模块头「展示名」。
    pub display_name: &'static str,
    /// 客户端 `product.json` 的 `nameAlias`（官方自称，用于诊断与核对）。
    pub name_alias: &'static str,
    /// 客户端 `product.json` 的 `packageType`；未知时为 `None`（**不猜**）。
    pub package_type: Option<&'static str>,
    /// userData 目录名候选（`%APPDATA%\<名字>`）。**精确匹配，不做包含匹配**。
    pub data_dir_names: &'static [&'static str],
    /// 可执行文件名候选（精确匹配）。
    pub exe_names: &'static [&'static str],
    /// 进程名候选（Windows 精简名，不带 `.exe`）。
    pub proc_names: &'static [&'static str],
}

/// ⛔ **已从程序位表退役**（2026-10-15，用户决策：界面只保留合并版客户端）。
///
/// 仅 [`program_spec`] 的兼容查询还能取到它 —— 绑定了 TraeWork 的旧数据
/// （快照在 `profiles/`、userData 在 `%APPDATA%\TRAE SOLO CN`）原样保留、
/// 不迁移不删除；旧版本的本项目按原布局照常读写。
const CN_TRAE_WORK: ProgramSpec = ProgramSpec {
    region: TraeRegion::Cn,
    program: TraeProgram::TraeWork,
    display_name: "TraeWork CN",
    name_alias: "TraeWork CN",
    package_type: Some("SOLO_CN"),
    data_dir_names: &["TRAE SOLO CN"],
    exe_names: &["TRAE SOLO CN.exe"],
    proc_names: &["TRAE SOLO CN"],
};

/// ⛔ **已从程序位表退役**（2026-10-15）：与 [`CN_TRAE`] 物理上是同一个客户端
/// （逐字段相同），界面上「一个客户端摆两个按钮」没有意义。其快照目录
/// `profiles_trae_cn/` 原样保留（可搬的已由 `region_migrate` 搬进合并版槽位）。
const CN_TRAE_CODE: ProgramSpec = ProgramSpec {
    region: TraeRegion::Cn,
    program: TraeProgram::TraeCode,
    display_name: "TraeCode CN",
    name_alias: "TraeCode CN",
    package_type: Some("TRAE_CN"),
    data_dir_names: &["Trae CN"],
    exe_names: &["Trae CN.exe"],
    proc_names: &["Trae CN"],
};

/// **合并后的 CN 客户端**（issue #20）：客户端名就是 `Trae CN`。
///
/// 与 [`CN_TRAE_CODE`] **逐字段相同**（同名、同目录、同 exe、同 `packageType`）——
/// 因为合并后的客户端**就是**那一个。差异只在**展示名**（客户端现在叫 `Trae`）
/// 与 `program` 标识（`trae`）。候选名重叠已在模块头登记。
const CN_TRAE: ProgramSpec = ProgramSpec {
    region: TraeRegion::Cn,
    program: TraeProgram::Trae,
    display_name: "Trae CN",
    // 客户端 `product.json` 的 `nameAlias` 实测仍是 `TraeCode CN`
    // （合并版 `D:\Programs\Trae CN\resources\app\product.json`，本机 2026-10-13 读取）
    // —— 上游没改这个字段，我们不替它改。
    name_alias: "TraeCode CN",
    package_type: Some("TRAE_CN"),
    data_dir_names: &["Trae CN"],
    exe_names: &["Trae CN.exe"],
    proc_names: &["Trae CN"],
};

/// ⛔ **已从程序位表退役**（2026-10-15，用户决策）：与 [`CN_TRAE_WORK`] 同批，
/// 界面只保留合并版客户端；数据原样保留、不迁移不删除。
const GLOBAL_TRAE_WORK: ProgramSpec = ProgramSpec {
    region: TraeRegion::Global,
    program: TraeProgram::TraeWork,
    display_name: "TraeWork",
    name_alias: "TraeWork",
    package_type: Some("SOLO_I18N"),
    data_dir_names: &["TRAE SOLO"],
    exe_names: &["TRAE SOLO.exe"],
    proc_names: &["TRAE SOLO"],
};

/// ⛔ **已从程序位表退役**（2026-10-15）：与 [`GLOBAL_TRAE`] 物理上是同一个客户端，
/// 与 [`CN_TRAE_CODE`] 同批退役；数据原样保留、不迁移不删除。
///
/// 国际版 TraeCode：**本机未安装**，`nameAlias` / `packageType` 无从实测，
/// 因此 `package_type` 留 `None`、别名按同族命名规则推断并在此显式标注「待实测」。
/// 猜测只影响诊断文案；候选名（目录 / exe / 进程）是客户端通用命名，风险可控。
///
/// ⚠️ 它与 [`super::variant::GLOBAL_TRAE_CODE_SPEC`] 的 `package_type` **不是同一件事**：
/// 那边的字段是 `&'static str`（授权页分派必须要一个具体值），填的是同一推断值
/// `TRAE_I18N`；本字段是**诊断用**、允许诚实留空。两处都写了「未实测」，
/// 谁先拿到真实客户端就一起更正。
const GLOBAL_TRAE_CODE: ProgramSpec = ProgramSpec {
    region: TraeRegion::Global,
    program: TraeProgram::TraeCode,
    // 国际版**不带后缀**（用户 2026-10-13 更正的规则）；保留家族名 `TraeCode` ——
    // 简称 `Trae` 会与国际合并版撞名（Key 归属下拉里两个同名选项）。
    //
    // ⚠️ 本格是**六个展示名里唯一的推断值**：本机未装国际 TraeCode，读不到它的
    // `nameAlias`，故按同族实测规律去掉 ` CN` 后缀（国际 TraeWork 的 `nameAlias`
    // 实测为 `TraeWork`，不带后缀）。拿到真实客户端后应核对并更正本行。
    display_name: "TraeCode",
    name_alias: "TraeCode（待实测）",
    package_type: None,
    data_dir_names: &["Trae"],
    exe_names: &["Trae.exe"],
    proc_names: &["Trae"],
};

/// **合并后的国际客户端**（issue #20）：客户端名 `Trae` / `Trae.exe`。
///
/// ★ **候选名由用户确认**（2026-10-13）：「国际版 `Trae` / `Trae.exe`、
/// 国内版 `Trae CN` / `Trae CN.exe`」—— 国内那份另有 `product.json` 实测佐证。
///
/// 与 [`GLOBAL_TRAE_CODE`] 候选名逐字段相同（客户端合并时复用了目录名与 exe 名），
/// 但**展示名不同**：本取值是合并版（客户端 `nameShort` = `Trae`）⇒ `Trae`；
/// 而 [`GLOBAL_TRAE_CODE`] 保留家族名 ⇒ `TraeCode`。
/// （规则：**国际版不带后缀**；国内版才带 ` CN`。）
const GLOBAL_TRAE: ProgramSpec = ProgramSpec {
    region: TraeRegion::Global,
    program: TraeProgram::Trae,
    display_name: "Trae",
    name_alias: "TraeCode（待实测）",
    package_type: None,
    data_dir_names: &["Trae"],
    exe_names: &["Trae.exe"],
    proc_names: &["Trae"],
};

/// 界面上**在役**的全部程序位（2026-10-15 收敛后：每区域 1 个 = 合并版客户端，共 2 条）。
///
/// 退役的 4 条（两条 TraeWork + 两条遗留 TraeCode）**不在此表**，但仍可经
/// [`program_spec`] 查到（兼容面见模块头「程序位收敛」）。
///
/// 历史：收敛前本表 6 条，其中 `Trae` 与 `TraeCode` 两条**候选名逐字相同**
/// （同一个客户端）⇒ 摊平成候选表时必须按序去重。收敛后重叠不再存在于本表
/// （候选名重叠只保留在 [`super::variant`] 的变体表里，供旧数据兼容）。
pub fn all_program_specs() -> [&'static ProgramSpec; 2] {
    [&CN_TRAE, &GLOBAL_TRAE]
}

/// 取某个程序位的描述符。
pub fn program_spec(region: TraeRegion, program: TraeProgram) -> &'static ProgramSpec {
    match (region, program) {
        (TraeRegion::Cn, TraeProgram::TraeWork) => &CN_TRAE_WORK,
        (TraeRegion::Cn, TraeProgram::TraeCode) => &CN_TRAE_CODE,
        (TraeRegion::Cn, TraeProgram::Trae) => &CN_TRAE,
        (TraeRegion::Global, TraeProgram::TraeWork) => &GLOBAL_TRAE_WORK,
        (TraeRegion::Global, TraeProgram::TraeCode) => &GLOBAL_TRAE_CODE,
        (TraeRegion::Global, TraeProgram::Trae) => &GLOBAL_TRAE,
    }
}

/// 某个区域下的**在役**程序位（界面按区域渲染程序控件时遍历它）。
///
/// 2026-10-15 收敛后每区域只有**一条**：合并版客户端。TraeWork 与遗留 TraeCode
/// 已退役（数据原样保留，见 [`all_program_specs`] / 模块头「程序位收敛」）。
pub fn programs_of(region: TraeRegion) -> [&'static ProgramSpec; 1] {
    match region {
        TraeRegion::Cn => [&CN_TRAE],
        TraeRegion::Global => [&GLOBAL_TRAE],
    }
}

/// 区域对应的端点集合。
///
/// **单一来源**：直接读 [`super::variant::variant_spec`] 的 CN / 国际端点表
/// （国内值已实测跑通、国际值取自国际版客户端自述），本模块不另立一份。
pub fn region_endpoints(region: TraeRegion) -> &'static EndpointSet {
    let spec = variant_spec(TraeVariant::TraeWork);
    match region {
        TraeRegion::Cn => &spec.cn_endpoints,
        TraeRegion::Global => spec
            .global_endpoints
            .as_ref()
            .expect("国际版端点必须在表里登记（见 variant 模块的国际化端点单测）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 区域标识与展示名稳定() {
        assert_eq!(TraeRegion::Cn.as_str(), "cn");
        assert_eq!(TraeRegion::Global.as_str(), "global");
        // ⚠️ 区域名 = **客户端自己的产品名**（用户 2026-10-13 定），
        // 与 `shared.region.version.*`（跨平台区域**词**）分工不同，见 `display_name` 的说明。
        // 反例（改坏会红）：退回「国内版 / 国际版」⇒ 前端 Trae 侧的区域名会与后端漂开。
        assert_eq!(TraeRegion::Cn.display_name(), "Trae CN");
        assert_eq!(TraeRegion::Global.display_name(), "Trae");
        // 默认必须落在与改造前同一份账号库上，否则老用户会看到账号凭空消失。
        assert_eq!(TraeRegion::default(), TraeRegion::Cn);
    }

    /// 历史取值 `trae_work` / `trae_cn` **必须都解析成国内版**：
    /// 它们是改造前的产品线标识，而两个产品线都是国内构建。
    /// 若谁把 `trae_cn` 解析成国际版，老链接会在国际版里读空账号库。
    #[test]
    fn 历史产品线标识一律解析为国内版() {
        for s in ["trae_work", "trae_cn", "CN", "trae", "cn"] {
            assert_eq!(TraeRegion::parse(s), Some(TraeRegion::Cn), "解析失败: {s}");
        }
        for s in ["global", "intl", "international", "SG"] {
            assert_eq!(TraeRegion::parse(s), Some(TraeRegion::Global), "解析失败: {s}");
        }
        assert_eq!(TraeRegion::parse("doubao"), None);
    }

    #[test]
    fn 程序线解析接受客户端命名() {
        assert_eq!(TraeProgram::parse("TRAE SOLO CN.exe"), Some(TraeProgram::TraeWork));
        assert_eq!(TraeProgram::parse("solo"), Some(TraeProgram::TraeWork));
        assert_eq!(TraeProgram::parse("trae_work"), Some(TraeProgram::TraeWork));
        // ⚠️ `Trae CN.exe` 自 issue #20 起落**合并版** `Trae` —— 客户端现在就叫这个名字，
        //    而遗留 TraeCode 与它**同名**（物理上同一个客户端），只能靠标识
        //    `trae_code` 区分。名字层面无法分辨两者，`parse` 取当前（合并版）为准。
        assert_eq!(TraeProgram::parse("Trae CN.exe"), Some(TraeProgram::Trae));
        assert_eq!(TraeProgram::parse("trae_code"), Some(TraeProgram::TraeCode));
        assert_eq!(TraeProgram::parse("doubao"), None);
    }

    /// ★ 裸 `trae` / `trae cn` 必须解析成**合并后的客户端**，而 `trae_code` 仍是遗留槽位
    /// （issue #20）。
    ///
    /// 反例（改坏会红）：把 `contains("trae")` 放在 `contains("code")` 之前 ——
    /// `trae_code` 会落进 `Trae`，于是「切到遗留 IDE 程序位」变成「切到合并版」。
    /// 两者物理上同一个客户端，故**不会报错**，只会让 `program` 标识静默漂移。
    #[test]
    fn 裸名trae解析为合并版而trae_code仍是遗留() {
        for s in ["trae", "Trae", "TRAE", "trae cn", "Trae CN", "Trae CN.exe"] {
            assert_eq!(TraeProgram::parse(s), Some(TraeProgram::Trae), "解析失败: {s}");
        }
        for s in ["trae_code", "trae_code_global", "global_trae_code", "trae_cn", "traecode"] {
            assert_eq!(
                TraeProgram::parse(s),
                Some(TraeProgram::TraeCode),
                "遗留标识必须落 TraeCode，不能被裸名规则抢走: {s}"
            );
        }
        // 三个程序位的标识两两不同 —— `parse` 与 `as_str` 必须互为往返。
        for program in TraeProgram::all() {
            assert_eq!(TraeProgram::parse(program.as_str()), Some(program));
        }
    }

    /// 展示名按用户确认的写法：**国内版带 ` CN` 后缀、国际版不带后缀**。
    ///
    /// ★ 命名规则 = **家族名 +（国内 ? ` CN` : 空）**。家族名取自客户端自己的
    /// `nameAlias`（本机实测：国际 TraeWork 客户端 `nameAlias = "TraeWork"`、
    /// CN 合并客户端 `nameAlias = "TraeCode CN"`），合并版则取 `nameShort`（`Trae CN`）。
    #[test]
    fn 程序位展示名符合约定() {
        use TraeProgram::{Trae, TraeCode, TraeWork};
        use TraeRegion::{Cn, Global};
        assert_eq!(program_spec(Cn, TraeWork).display_name, "TraeWork CN");
        assert_eq!(program_spec(Cn, TraeCode).display_name, "TraeCode CN");
        assert_eq!(program_spec(Global, TraeWork).display_name, "TraeWork");
        assert_eq!(program_spec(Global, TraeCode).display_name, "TraeCode");
        // 合并后的客户端：客户端自己就叫 `Trae CN`（国内）/ `Trae`（国际）。
        assert_eq!(program_spec(Cn, Trae).display_name, "Trae CN");
        assert_eq!(program_spec(Global, Trae).display_name, "Trae");
    }

    /// ★ 合并版客户端的候选名必须与**用户确认的写法**逐字一致（2026-10-13）：
    /// 「**国际版 `Trae` / `Trae.exe`、国内版 `Trae CN` / `Trae CN.exe`**」。
    ///
    /// ## 为什么必须钉住（而不是写在注释里）
    ///
    /// 候选名决定**探测结果**：它一漂，程序位就会指到错的客户端，而且**不报错**。
    /// 反例（改坏会红）：把国内那份写成裸 `Trae`（那是**国际版**客户端的名字）⇒
    /// 只装国际版客户端的机器上，国内程序位被判成「已安装」并指向国际版目录 ——
    /// 这正是 issue #3 修掉过的那一类缺陷（当时的现场是裸名 `Trae` 挂在国内 TraeCode 上）。
    ///
    /// 国内那份另有本机 `product.json` 实测佐证（`nameShort = "Trae CN"`、
    /// `applicationName = "trae-cn"`）；国际那份本机未装客户端，依据就是**用户确认**。
    #[test]
    fn 合并版候选名与用户确认的写法逐字一致() {
        use TraeProgram::Trae;
        use TraeRegion::{Cn, Global};
        for (region, dir_name, exe_name) in [
            (Cn, "Trae CN", "Trae CN.exe"),
            (Global, "Trae", "Trae.exe"),
        ] {
            let spec = program_spec(region, Trae);
            assert_eq!(
                spec.data_dir_names.to_vec(),
                vec![dir_name],
                "{region:?} 合并版的 userData 名漂了（用户确认的是 `{dir_name}`）"
            );
            assert_eq!(
                spec.exe_names.to_vec(),
                vec![exe_name],
                "{region:?} 合并版的 exe 名漂了（用户确认的是 `{exe_name}`）"
            );
            assert_eq!(
                spec.proc_names.to_vec(),
                vec![dir_name],
                "{region:?} 合并版的进程名漂了"
            );
        }
    }

    /// ★★ **六个展示名必须两两不同**（issue #20 收尾）。
    ///
    /// ## 为什么这条非钉不可
    ///
    /// 展示名是**用户唯一能拿来区分程序位的东西**：Key 归属下拉、程序位按钮的
    /// tooltip / `aria-label`、切换 toast 全靠它。两条程序位同名时，
    /// 用户在下拉里看到两个一模一样的选项 —— 选错就是把登录态写进另一条线，
    /// 而且**界面上看不出任何异常**。
    ///
    /// 本仓真实踩过两次：① 国际 TraeCode 曾叫 `Trae AI`，与合并版客户端
    /// （`nameShort` = `Trae`）撞名；② 更正约定后若把国际 TraeCode 简称 `Trae`，
    /// 仍会与合并版撞名 ⇒ 它必须**保留家族名**（`TraeCode`）。
    ///
    /// 反例（改坏会红）：把任意两条 `display_name` 改成同一个值 ⇒ 立刻变红。
    #[test]
    fn 在役程序位的展示名两两不同() {
        let specs = all_program_specs();
        assert_eq!(specs.len(), 2, "前置：应恰有 2 条在役程序位，否则本用例会恒真");
        let mut seen: Vec<(&str, TraeProgram, TraeRegion)> = Vec::new();
        for spec in specs {
            if let Some((name, other_program, other_region)) = seen
                .iter()
                .find(|(name, _, _)| *name == spec.display_name)
            {
                panic!(
                    "展示名 `{name}` 被两条程序位共用：{other_region:?}/{other_program:?} 与 {:?}/{:?} \
                     —— 用户在下拉里分不清，选错会把登录态写进另一条线",
                    spec.region, spec.program
                );
            }
            seen.push((spec.display_name, spec.program, spec.region));
        }
    }

    /// ★★ 合并版程序位（`Trae`）与遗留 TraeCode **必须指向同一个客户端**（issue #20）。
    ///
    /// 依据是 `migration-job.json`（模块头引了原文）：合并把 `TRAE SOLO CN` 的数据
    /// 并进了 `Trae CN`，而 `Trae CN` 正是遗留 TraeCode 的落点。
    ///
    /// 这条断言的意义：三者（候选名 / `packageType` / 展示名归属）一旦分家，
    /// 就会出现「同一台机器上 Trae 与 TraeCode 探测到不同客户端」，
    /// 或「合并版的登录态写进了另一套快照」——两种都不报错，只是数据串了。
    #[test]
    fn 合并版与遗留traecode指向同一客户端() {
        for region in TraeRegion::all() {
            let legacy = program_spec(region, TraeProgram::TraeCode);
            let merged = program_spec(region, TraeProgram::Trae);
            assert_eq!(
                merged.data_dir_names, legacy.data_dir_names,
                "{region:?} 的合并版 userData 候选与遗留 TraeCode 不一致"
            );
            assert_eq!(merged.exe_names, legacy.exe_names, "{region:?} 的 exe 候选不一致");
            assert_eq!(merged.proc_names, legacy.proc_names, "{region:?} 的进程名候选不一致");
            assert_eq!(merged.package_type, legacy.package_type, "{region:?} 的 packageType 不一致");
            // 真正要钉的是**程序位标识**不同 —— 否则前端拿不到区分两者的依据，
            // 「哪一枚按钮点下去写哪个客户端」这件事就没有载体。
            assert_ne!(merged.program, legacy.program);
            assert_ne!(merged.program.as_str(), legacy.program.as_str());
            // 展示名：两条程序位必须**可区分**（`TraeCode CN` vs `Trae CN`）。
            //
            // ⚠️ 更正记录：这段原先只对 CN 断言，理由是「约定说国际版带 `AI` 后缀，
            //    而那条约定给国际 TraeCode 的名字本来就是 `Trae AI`，与合并版同名」。
            //    那个约定是**错的**（2026-10-13 用户更正：国内带 `CN`、国际不带后缀），
            //    国际行现在是 `TraeCode` vs `Trae` ⇒ **两个区域都必须不同**，
            //    断言因此收紧到全部区域。
            assert_ne!(
                merged.display_name, legacy.display_name,
                "{region:?} 的合并版与遗留位展示名相同 —— 界面上会分不清谁是谁"
            );
        }
    }

    /// 程序位的候选名（目录 / exe / 进程）**必须两两互不重叠**。
    ///
    /// 这是本模块存在的理由之一：改造前国内/国外候选名混在一张表里按序取首个，
    /// 用包含关系匹配就会「装了 A、读了 B 的目录」。谁把它们合并回去，这里会红。
    ///
    /// ## 历史白名单已清空（2026-10-15）
    ///
    /// issue #20 时代合并版 `Trae` × 遗留 `TraeCode`（物理上同一个客户端）曾以
    /// 6 项白名单登记在案；程序位表收敛（每区域只留合并版）后两条 TraeCode 格
    /// 退出本表 ⇒ **重叠集合应为空**。候选名重叠仍存在于 [`super::variant`] 的
    /// **变体表**里（6 个变体全保留，供旧数据兼容）—— 那边有自己的护栏
    /// （`variant_of_name` 精确匹配 + 自身单测）。
    #[test]
    fn 程序位候选名互不重叠() {
        let specs = all_program_specs();
        let mut overlaps: Vec<String> = Vec::new();
        for (i, a) in specs.iter().enumerate() {
            for b in specs.iter().skip(i + 1) {
                for (kind, left, right) in [
                    ("userData 目录名", a.data_dir_names, b.data_dir_names),
                    ("exe 名", a.exe_names, b.exe_names),
                    ("进程名", a.proc_names, b.proc_names),
                ] {
                    for name in left {
                        if right.contains(name) {
                            overlaps.push(format!(
                                "{kind}:{name}:{:?}/{:?}:{:?}/{:?}",
                                a.region, a.program, b.region, b.program
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            overlaps.is_empty(),
            "在役程序位的候选名不得重叠（变体表里的历史重叠不在本断言范围）: {overlaps:?}"
        );

        // 反向：CN 与 Global 的同名程序**不能**共用候选名（差一个后缀也算）。
        assert_ne!(
            program_spec(TraeRegion::Cn, TraeProgram::TraeWork).data_dir_names,
            program_spec(TraeRegion::Global, TraeProgram::TraeWork).data_dir_names
        );
    }

    /// `programs_of` 必须给出该区域的**一条**程序位（合并版客户端），
    /// 且归属本区域。2026-10-15 收敛：TraeWork / 遗留 TraeCode 已退役。
    #[test]
    fn 按区域取程序位() {
        for region in TraeRegion::all() {
            let got = programs_of(region);
            assert_eq!(got.len(), 1, "每个区域应只有 1 条在役程序位（合并版客户端）");
            for spec in got {
                assert_eq!(spec.region, region, "程序位归属漂了: {:?}", spec.program);
            }
            // 必须是**合并版** —— 退役的两格若混回来，界面会重新出现
            // 「一个客户端两个按钮」。
            assert!(
                got.iter().any(|s| s.program == TraeProgram::Trae),
                "{region:?} 的程序位不是合并版客户端"
            );
        }
    }

    /// 区域端点是**单一来源**：国内必须等于已实测的 CN 值，国际必须等于
    /// 国际版客户端自述值，且两者主机全不同（混用会把请求打到错的域）。
    #[test]
    fn 区域端点取自既有端点表() {
        let cn = region_endpoints(TraeRegion::Cn);
        assert_eq!(cn.account_base, "https://api.trae.cn");
        assert_eq!(cn.icube_base, "https://api.trae.com.cn");

        let global = region_endpoints(TraeRegion::Global);
        assert_eq!(global.account_base, "https://grow-normal.trae.ai");
        assert_eq!(global.icube_base, "https://icube-normal.trae.ai");
        assert_ne!(global.account_base, cn.account_base);

        // 授权页域（`bootConfig.consoleHost`）同样按区域分家 —— 它不是 API 域，
        // 但漏了分家的后果更隐蔽：国际版用户在**国内**授权页上登录，
        // 页面走完也不回调，症状与「回调端口没监听」几乎一样。
        assert_eq!(cn.console_base, "https://www.trae.cn");
        assert_eq!(global.console_base, "https://www.trae.ai");
        assert_ne!(global.console_base, cn.console_base);
    }
}
