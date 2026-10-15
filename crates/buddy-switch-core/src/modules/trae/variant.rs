//! 产品线变体（variant）：把 Trae 各条产品线的全部差异收敛到一张表。
//!
//! ## ⚠️ 弃用状态（P5 §2.6 · 2026-10-05）
//!
//! `TraeVariant` 把「**区域**（国内版 / 国际版）」与「**程序位**（TraeWork / TraeCode）」
//! **压在同一根轴上** —— 这是本仓最要命的命名混淆（VOCABULARY §4：`variant` 双义）。
//! 目标形态是两根正交轴：[`super::region::TraeRegion`]（持久化轴，账号库 / 端点 / 冷却）
//! + [`super::region::TraeProgram`]（执行轴，写哪个客户端 / 启哪个 exe）。
//!
//! **本模块已弃用（deprecated）**：新代码**不得**再依赖 `TraeVariant`；迁移按
//! 「调用点逐段迁到 `region.rs` 双轴 → 全部迁完后删除本模块」推进。
//!
//! ### ★ 删除的前置条件（**当前未满足** ⇒ P5 T05 只标注弃用、**不删除**）
//!
//! 设计规定：必须**在前端 `TraeRegionId` / `TraeProgramId` 完全替代 `TraeVariantId` 之后**
//! 才可删除（否则前端静默读到 `undefined`，**不报错**）。截至 2026-10-05 实测**未满足**：
//!
//! - 前端 `src/` 仍有 **12 个文件、81 处**引用 `TraeVariantId`
//!   （`lib/trae-types.ts` / `lib/trae-variant-status.ts` / `pages/TraeAccountsPage.tsx` /
//!   `pages/TraeSettingsPage.tsx` / 网关组件…）；
//! - 后端 Rust 仍有 **830 处**引用 `TraeVariant`（含 `paths.rs` 里 **18 个** `*_for(variant)` 家族函数）。
//!
//! ⇒ 现在删除 `variant.rs` 或 `paths.rs` 的旧家族会**立刻编译失败 / 运行期静默 `undefined`**。
//! 删除须**另立任务**，先把上述调用点全部迁到双轴。
//!
//! ⚠️ 迁移必须走解析函数，**不要**按字符串字面量硬映射：`TraeVariant` 的四个取值**不是**
//! `TraeRegion × TraeProgram` 的朴素笛卡尔积命名 —— 历史值 `trae_work` / `trae_cn`
//! **都是国内构建**（`TraeRegion::parse` 把两者都归 `Cn`），只有 `global_trae_code` 是国际版 TraeCode。
//! 逐段迁移一律经 [`super::region::TraeRegion::parse`] / [`super::region::TraeProgram::parse`]。
//!
//! ## 为什么需要这一层
//!
//! Trae 有**多条可以同机并存的产品线**（实测本机同时装着 `TRAE SOLO CN` 与
//! `Trae CN`，各有独立的安装目录、userData、进程名）。在引入本模块之前，
//! 这些差异散落在 [`super::platform`]（候选名列表）与 [`super`]（写死的 CN 端点常量）里，
//! 且**只有一个「全候选混在一起」的视角** —— 于是：
//!
//! - 界面上分不清「管理的是哪一条产品线」（自动探测按最近活跃挑，用户只能猜）；
//! - 想按产品线派生端点时没有可用的维度（端点只能写死成 CN 那套）。
//!
//! 本模块给出与 [`crate::modules::region`] 同形的表驱动设计：`TraeVariant` 枚举 +
//! `variant_spec()` 查表。**所有按产品线分家的取值都必须从这里派生**，
//! 不要再在别处散落 `if solo { … } else { … }`。
//!
//! ## 与 `Region` 的关系：正交，不要合并
//!
//! [`crate::modules::region::Region`] 描述 **WorkBuddy 这一产品的两个发行版本**
//! （`workbuddy-desktop.info` / `workbuddy-desktop-ai.info`），它们的认证文件格式、
//! 账号库结构、上游协议形状**完全一致**，差异只是域名/版本号。而 Trae 的产品线差异是
//! **产品级**的：凭据形态、账号库文件名、登录态载体、上游主机族都不同。
//! 把 Trae 塞进 `Region` 会让 `region_spec(All)`、`accounts_file_for(All)` 之类的既有约定
//! 失去定义，且所有 `match region { Cn | Global }` 的调用点都要补分支。
//! 因此两者**并列存在、互不转换**。
//!
//! ## 端点取值的权威来源（重要）
//!
//! 下表里的主机值**不是猜的**，来自客户端自带且随安装包一起分发的
//! `<安装根>\<产品名>\resources\app\product.json`：
//!
//! ```text
//! product.json → bootConfig → <能力>.trae.<regionKey>
//! ```
//!
//! 其中 `normal` 是 CN 构建的默认 region 键（同一份文件里另有 `SG` / `US` / `CN` / `USTTP`）。
//! 实测两条 CN 产品线（`TRAE SOLO CN` / `Trae CN`）的 `*.trae.normal` **逐字相同**，
//! 即：**产品线不改变端点，region 才改变端点**。这一点决定了下面这张表的形状 ——
//! 每个变体记录自己那套 region 键的取值。
//!
//! 同样来自 `product.json` 的身份事实（实测）：
//!
//! | 键 | `TRAE SOLO CN` | `Trae CN` |
//! |:---|:---|:---|
//! | `nameAlias` | **`TraeWork CN`** | `TraeCode CN` |
//! | `packageType` | `SOLO_CN` | `TRAE_CN` |
//! | `runMode` | `solo-lite` | （null） |
//! | `applicationName` | `trae-solo-cn` | `trae-cn` |
//! | `darwinBundleIdentifier` | `cn.trae.solo.app` | `cn.trae.app` |
//!
//! **⇒ `TRAE SOLO CN` 的官方别名就是 `TraeWork CN`**，即它是 Trae Work 产品线的 CN 版；
//! `Trae CN` 才是 IDE 那条线。这就是本模块把两个变体命名为 `TraeWork` / `Trae` 的依据，
//! 也是界面上「Trae Work」这一分区名的出处 —— **不是我们起的名字**。
//!
//! ## 国际化端点：取值来源已更正（2026-09-21）
//!
//! 本表曾把国际版端点记为 `api.trae.ai` / `api.trae.ai` / `grow-normal.trae.ai`，
//! 那三个值是**脱离结构的字符串误摘** —— 它们确实出现在**国内版**客户端的
//! `product.json` 里，但不是 `bootConfig.<能力>.trae.<regionKey>` 的取值，而是
//! 别的能力表（CDN / 市场域）的 `SG`/`US` 键。按结构读一遍就能发现：
//! 国内版本机的 `bootConfig.account.trae.*` 只有 `normal` 一个 CN 值，
//! 而 `grow-normal.trae.ai` 在国际版客户端里是 **account** 基址，被记成了 agent。
//!
//! 现在改用**国际版客户端自己声明的**值（本机已装：注册表 `TraeWork (User)`
//! → `%LOCALAPPDATA%\Programs\TRAE SOLO`，`packageType = SOLO_I18N`），
//! 取自它的 `<安装根>\resources\app\product.json` → `bootConfig.<能力>.trae.normal`：
//!
//! | 能力 | 国内版（`TRAE SOLO CN`） | 国际版（`TRAE SOLO`） |
//! |:---|:---|:---|
//! | `account` | `https://api.trae.cn` | `https://grow-normal.trae.ai` |
//! | `iCube` | `https://api.trae.com.cn` | `https://icube-normal.trae.ai` |
//! | `agent` | `https://trae-api-cn.mchost.guru` | `https://core-normal.trae.ai` |
//! | `ws` | `wss://trae-ws-cn.mchost.guru/custom_model` | `wss://wss-normal.trae.ai/custom_model` |
//! | `consoleHost`（授权页域） | `https://www.trae.cn` | `https://www.trae.ai` |
//!
//! `consoleHost` 那一行**不是 API**：它是「浏览器里打开的那一页」的域，
//! 授权页 = `${consoleHost}/authorization`。它与 `account`/`iCube` 不同主机，
//! 且**必须按区域分家** —— 拿 CN 域给国际版发登录，用户会在错的账号体系的
//! 授权页上登录，页面走完也不回调（见 [`EndpointSet::console_base`]）。
//!
//! ## 仍未验证的部分（勿凭推理"补全"）
//!
//! 上表**主机名**是客户端自述的权威值，但**接口路径与鉴权头是否与国内版同名**
//! 没有任何证据 —— 它从未对真实上游跑通过（本机没有可用的国际版凭据）。
//! 因此国际化联网能力必须**逐项实测**后才能宣称可用，失败方向见
//! [`VariantSpec::endpoints`]：缺失时返回 `None`，**绝不**用 CN 值冒充国际化值。
//! 本模块只负责「端点取哪个值」，不负责「上游是否接受」。

use std::path::PathBuf;

/// Trae 产品线变体。
///
/// 命名依据是客户端 `product.json` 的 `nameAlias`：`TRAE SOLO CN` 自称 `TraeWork CN`，
/// `Trae CN` 自称 `TraeCode CN`。对外展示用 [`TraeVariant::display_name`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraeVariant {
    /// Trae Work 产品线（对应客户端的 `TRAE SOLO` / `TRAE SOLO CN`）。
    ///
    /// ⚠️ 持久化轴翻到区域之后（2026-09-21），本取值在**读写数据**时表示「**国内区域**」
    /// （见 [`TraeVariant::region`]）：`TraeWork` 与 `Trae` 共用国内库，
    /// 差别只在**程序**维度（启动哪个客户端、快照落哪个目录）。
    TraeWork,
    /// Trae IDE 产品线（客户端 `product.json`：`nameShort = Trae CN`、`nameAlias = TraeCode CN`）。
    ///
    /// 语义同 `TraeWork`：**数据层面＝国内区域**，差别只在程序维度。
    Trae,
    /// **国际版区域**（客户端 `packageType = SOLO_I18N`，发布者 SPRING (SG) PTE. LTD）。
    ///
    /// 在持久化轴翻到区域之后新增：它让「区域之间互不污染」这条**真契约**继续可表达
    /// （此前那批断言用的是两个国内产品线，而它们现在共用一本库，断言前提已消失）。
    ///
    /// **刻意暂不列入 [`TraeVariant::all`]**：`all()` 是「两条国内程序位」的旧入口
    /// （见该函数的说明），而本取值属于国际区域。
    Global,
    /// **国际版 TraeCode**（客户端 `Trae` / `Trae.exe`，`nameAlias = TraeCode`）。
    ///
    /// ## 为什么它是最后一个被补上的程序位（issue #3）
    ///
    /// 四个程序位 = 区域 × 程序。前三个早就有了，唯独「国际版 × TraeCode」缺位：
    /// `platform::variant_for_program(Global, TraeCode)` 当时返回 `None`，
    /// 于是国际版页签上的 TraeCode 按钮永远显示「未检测到」——
    /// 用户报的「不支持 TraeCode」正是这一格（issue #3）。
    ///
    /// ## 它同时修掉一个**跨区误判**（比缺位更危险）
    ///
    /// 补位之前，`TRAE_CN_SPEC`（国内 TraeCode）的候选名里混着 `Trae` / `Trae.exe`
    /// —— 那是**国际版**客户端的名字。后果不是报错，而是**静默认错客户端**：
    /// 只装了国际版 TraeCode 的机器上，国内版 TraeCode 的程序位会被判成「已安装」，
    /// 并指向国际版的安装目录与 userData ⇒ 切账号会把登录态写进另一个客户端。
    /// 本取值把 `Trae` / `Trae.exe` 收归自己名下，那条误判随之消失。
    ///
    /// ⚠️ **候选名（目录 / exe / 进程）是客户端通用命名**（`<名> CN` 是国内版、
    /// 裸名是国际版，与 `TRAE SOLO CN` / `TRAE SOLO` 同款规则），风险可控；
    /// 但本机未装国际版 TraeCode，`packageType` 与 `nameAlias` **未实测**，
    /// 故 `package_type` 是显式标注的推断值（见 [`GLOBAL_TRAE_CODE_SPEC`]）。
    #[serde(rename = "global_trae_code")]
    GlobalTraeCode,
    /// **合并后的 CN 客户端**（issue #20）：客户端 `Trae CN`。
    ///
    /// ## 为什么它是一个**独立变体**而不是复用 `Trae`
    ///
    /// 用户确认的表述是「**`TraeCode` 是 `TraeCode`，`Trae` 是 `Trae`（Work 与 Code 的
    /// 合并版）**」—— 两条程序位、两个客户端代次，必须各有各的身份：
    ///
    /// - **登录态快照目录**：变体是快照目录的唯一派生来源（[`super::paths::profiles_dir_for`]
    ///   的 `profiles_<as_str>`）。合并版与遗留 `TraeCode` 复用同一个变体时，
    ///   两条程序位的快照会挤进同一个目录 —— 那正是本次要修的问题。
    /// - **Key 归属 / 网关日志 / Token 统计分档**：都按变体分档。合并后的客户端是
    ///   新的一代，混进 `TraeCode` 那一档会让「哪一代客户端在用」这件事无法区分。
    /// - **双射恢复**：本取值补上后，`变体 ↔ (区域, 程序位)` **重新是一一对应**
    ///   （见 `client_settings::variant_for_program` 的单测）。这一点很值钱：
    ///   变体与程序位不再多对一时，「用变体反推程序位名字」那类缺陷从结构上消失
    ///   （用户已经踩过一次：点「Trae」却提示「TraeCode」）。
    ///
    /// ⚠️ **候选名与 [`TraeVariant::Trae`] 逐字相同**（`Trae CN` / `Trae CN.exe`）——
    /// 客户端合并时复用了 TraeCode 的目录名与 exe 名，这是客户端的既成事实。
    /// 因此 [`super::variant::variant_of_name`] 反查时**必须让本取值优先**
    /// （`all_specs()` 里排在 `TRAE_CN_SPEC` 之前），否则合并后的机器会被标成 `TraeCode`。
    ///
    /// ⚠️ 给**已弃用**的 `TraeVariant` 加取值是**临时妥协**：正确方向仍是
    /// `(TraeRegion, TraeProgram)` 双轴收尾（本模块头「弃用状态」）。但在快照链路
    /// 仍按变体键的当下，这是唯一能让两条程序位拿到**各自快照目录**而不动
    /// 命令面签名的做法。
    TraeMerged,
    /// **合并后的国际客户端**（issue #20）：客户端 `Trae` / `Trae.exe`。
    ///
    /// 与 [`TraeVariant::TraeMerged`] 同款（区域换成国际版；按规则**国际版不带后缀**）。
    /// ★ **候选名由用户确认**（2026-10-13）：「国际版 `Trae` / `Trae.exe`、
    /// 国内版 `Trae CN` / `Trae CN.exe`」⇒ 候选字段不是推断。
    /// `packageType` 与 [`TraeVariant::GlobalTraeCode`] 一样是**未实测的推断值**
    /// （本机未装国际版合并客户端），理由与该取值相同：它只被
    /// [`OAuthLine::from_package_type`] 消费，而那个分派只区分 `SOLO_*` 与非 `SOLO_*`。
    #[serde(rename = "global_trae_merged")]
    GlobalTraeMerged,
}

impl TraeVariant {
    /// 稳定标识（用于文件名、序列化与前端传参）。
    ///
    /// ## `Trae` 的字符串**仍是 `trae_cn`**（刻意不改，别"顺手修"）
    ///
    /// 它已经写进用户的磁盘与既有集成：
    ///
    /// - `api_gateway_keys.json` 里每条 Key 的归属；
    /// - 网关请求日志的 `variant` 字段（Token 统计按它分档）；
    /// - 快照目录名 `profiles_trae_cn`（见 `paths::profiles_dir_for`）。
    ///
    /// 改这个字符串等于**断老数据**，而它本身只是个键名。所以本次只把**标识符**
    /// 正名为 `Trae`（与官方 `nameShort` 一致），字符串保持原样；`parse` 同时接受
    /// `trae_cn` 与 `trae`，两侧都可读。
    pub fn as_str(self) -> &'static str {
        match self {
            TraeVariant::TraeWork => "trae_work",
            TraeVariant::Trae => "trae_cn",
            TraeVariant::Global => "global",
            // 与 [`paths::profiles_dir_for`] 的 `profiles_<as_str>` 规则合成
            // `profiles_global_trae_code` —— 与 `paths::profiles_dir_for_program`
            // 给 `(Global, TraeCode)` 的名字**逐字相同**（有单测钉住）。
            TraeVariant::GlobalTraeCode => "global_trae_code",
            // 合并后的客户端（issue #20）：独立标识 ⇒ 独立快照目录
            // （`profiles_trae_merged` / `profiles_global_trae_merged`）。
            // ⚠️ 刻意**不**用 `trae` —— 那是 `Trae`（遗留 TraeCode）的历史输入别名，
            // 见 [`TraeVariant::parse`]；两者共用字符串会让老数据指向错的一代客户端。
            TraeVariant::TraeMerged => "trae_merged",
            TraeVariant::GlobalTraeMerged => "global_trae_merged",
        }
    }

    /// 本取值对应的**持久化区域**（账号库/签到/积分/冷却/日志按它分家）。
    ///
    /// `TraeWork` 与 `Trae` **都是国内区域** —— 持久化轴翻到区域后，
    /// 二者的差别只剩**程序**维度（启动哪个客户端、快照落哪个目录）。
    pub fn region(self) -> super::region::TraeRegion {
        match self {
            TraeVariant::TraeWork | TraeVariant::Trae => super::region::TraeRegion::Cn,
            TraeVariant::Global | TraeVariant::GlobalTraeCode => super::region::TraeRegion::Global,
            // 合并版与它取代的那条遗留线同区域：CN 合并版落国内，国际合并版落国际。
            TraeVariant::TraeMerged => super::region::TraeRegion::Cn,
            TraeVariant::GlobalTraeMerged => super::region::TraeRegion::Global,
        }
    }

    /// 对外展示名（界面分区标题、日志、错误文案）。
    ///
    /// ★ 六个名字**逐字取客户端自己的字符串**（本机 `product.json` 实测）：
    /// `TraeWork CN` / `TraeCode CN`（两个 CN 客户端的 `nameAlias`）、
    /// `Trae CN`（CN 合并客户端的 `nameShort`）、
    /// `TraeWork`（国际 TraeWork 客户端的 `nameAlias`）、
    /// `TraeCode` / `Trae`（国际版按「不带后缀」规则）。
    /// **一个都不是我们编的** —— 见下面「命名规则」。
    ///
    /// ⚠️ 这里是**程序位**级的名，不是区域名。界面顶部的区域切换用的是
    /// `region::TraeRegion::display_name()`（国内版 / 国际版）。
    /// ## 命名规则（用户确认，2026-10-13 更正）
    ///
    /// **国内版带 ` CN` 后缀、国际版不带后缀**：
    ///
    /// | 程序位 | 国内版 | 国际版 |
    /// |:---|:---|:---|
    /// | TraeWork | `TraeWork CN` | `TraeWork` |
    /// | TraeCode | `TraeCode CN` | `TraeCode` |
    /// | Trae（合并版） | `Trae CN` | `Trae` |
    ///
    /// ★ 六个名字**逐字等于客户端自己的字符串**（本机 `product.json` 实测）：
    /// CN 合并版 `nameShort` = `Trae CN`、`nameAlias` = `TraeCode CN`；
    /// 国际 TraeWork 客户端 `nameAlias` = `TraeWork`。⇒ **一个都不用我们编**。
    ///
    /// ⚠️ 更正前的约定是「国际版带 `AI` 后缀」，那是**错的**（用户 2026-10-13 更正）：
    /// 它凭空造了 `Trae AI` / `TraeWork AI` 这类客户端根本不用的名字，
    /// 而且 `Trae AI` 会同时指国际 TraeCode 与国际合并版 ⇒ 撞名。
    ///
    /// 护栏：`region::tests::在役程序位的展示名两两不同` +
    /// 跨语言门禁（兜底表 vs `region.rs`）。
    pub fn display_name(self) -> &'static str {
        match self {
            TraeVariant::TraeWork => "TraeWork CN",
            // 本取值代表**遗留** TraeCode 程序位（`TraeMerged` 才是合并版）。
            TraeVariant::Trae => "TraeCode CN",
            TraeVariant::Global => "TraeWork",
            TraeVariant::GlobalTraeCode => "TraeCode",
            TraeVariant::TraeMerged => "Trae CN",
            TraeVariant::GlobalTraeMerged => "Trae",
        }
    }

    /// 从字符串解析变体。大小写不敏感，接受若干常见别名。
    ///
    /// 接受 `TRAE SOLO CN` / `TRAE SOLO` 这类**目录名**，因为用户可能直接从
    /// 探测结果（`TraeEnvStatus.dataDir` 的末段）把产品名传回来。
    pub fn parse(s: &str) -> Option<TraeVariant> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trae_work" | "traework" | "trae work" | "work" | "solo" | "trae solo" | "trae solo cn" => {
                Some(TraeVariant::TraeWork)
            }
            // `cn` 是**区域标识**（国内版）⇒ 必须落到该区域的**主程序**（TraeWork）。
            // 它曾作为 Trae 的宽容别名；但区域标识指向主程序才安全 ——
            // 否则「保存登录态」这类**程序级**操作会去读 TraeCode 客户端的 userData。
            "cn" => Some(TraeVariant::TraeWork),
            // `trae_code` 是前端的**程序位**标识（`TraeProgramId`），不带区域。
            // 收它并落**国内** TraeCode：这是唯一「不猜区域」的方向 ——
            // 国内 TraeCode 是既有程序位，而国际版 TraeCode 另有专属标识
            // `global_trae_code`（见下）。落成国际版才会把国内用户的登录态写错客户端。
            "trae_cn" | "traecn" | "trae cn" | "ide" | "trae" | "trae_code" | "traecode" => {
                Some(TraeVariant::Trae)
            }
            // 国际版区域。**不放宽成 `global`/`intl` 之外的词**：区域标识是要落到
            // 文件名与落盘数据上的，认得太宽会让一个拼错的参数静默读到另一套账号库
            // （症状是"账号凭空消失"，极难定位）。
            "global" | "trae_global" | "intl" | "international" => Some(TraeVariant::Global),
            // 国际版 TraeCode（issue #3）。**必须排在 `global` 之后**：
            // `global` 是区域标识、落到该区域的**主程序**（TraeWork），
            // 而本取值是同一区域里的**另一条程序**，两者不可互换 ——
            // 混了会把登录态写进错的客户端。
            //
            // `trae_code_global` / `traecodeglobal` 是同一件事的另两种写法，
            // 一并接受（前端历史值/手写参数都可能出现）。
            "global_trae_code" | "global_traecode" | "trae_code_global" | "traecodeglobal"
            | "trae global" => Some(TraeVariant::GlobalTraeCode),
            // 合并后的客户端（issue #20）。**必须排在 `global_trae_code` 之后**：
            // 两者的标识只差一个词根（`code` / `merged`），顺序写反会把国际版合并客户端
            // 认成国际版 TraeCode —— 症状是快照写进另一条程序位的目录。
            "trae_merged" | "traemerged" | "trae merged" | "trae_app" => {
                Some(TraeVariant::TraeMerged)
            }
            "global_trae_merged" | "globaltraemerged" | "trae_merged_global"
            | "trae app global" => Some(TraeVariant::GlobalTraeMerged),
            // ⚠️ **裸 `trae` 仍归 [`TraeVariant::Trae`]**（遗留 TraeCode），不改：
            // 它是改造前就登记在案的宽容别名，而合并版的**落盘标识**是 `trae_merged`
            // （见 `as_str`）。把裸名改指合并版会让「老参数/老记录」静默换一代客户端。
            _ => None,
        }
    }

    /// **国内区域**的两条程序位，供遍历用。
    ///
    /// ## ⚠️ 它不是「全部变体」（要全部请用 [`all_specs`]）
    ///
    /// 本函数是改造前**单轴（产品线）**时代的遍历入口，只含国内两条线，
    /// 而它被两类**只关心国内**的护栏消费：
    /// `checkin_constants_stay_aligned_with_variant_table`、
    /// `checkin_url_uses_trae_api_base`（都把 CN 端点写死）。
    /// 把两个国际区域取值塞进来会让那些断言对国际端点变红 ——
    /// 那不是缺陷被暴露，而是**遍历口径被换掉了**。
    ///
    /// ⇒ 区域轴上的完整遍历一律走 [`super::region::TraeRegion::all`]（界面/网关）
    /// 或 [`all_specs`]（变体表）；本函数**只**表示「国内两条程序位」。
    pub fn all() -> [TraeVariant; 2] {
        [TraeVariant::TraeWork, TraeVariant::Trae]
    }

    /// 本变体的**授权页产品线**（决定 `auth_from` / `client_id` / `hide_saas_login`）。
    ///
    /// ★ 判定依据**逐字照抄客户端自己的分派**（本机 2026-09-21 读国际版
    /// `out/main.js`）：客户端按 `packageType` 选产品线，而 `packageType`
    /// 正是本表里每个变体已经登记的字段：
    ///
    /// ```js
    /// function Su(t){ return t.packageType===SOLO_CN || t.packageType===SOLO_I18N
    ///                       || t.packageType===SOLO_CN_ENTERPRISE ? "SOLO_Lite" : "TRAE" }
    /// clientId = Pr(t) ? authConfig.SOLO[channel] : authConfig.TRAE[channel]
    /// authFrom = Su(t)==="SOLO_Lite" ? "solo" : "trae"     // 且 solo 时追加 hide_saas_login=true
    /// ```
    ///
    /// ⚠️ **授权页产品线 ≠ 区域**：`SOLO_CN` 与 `SOLO_I18N` 是**两条区域的同一个产品线**，
    /// 因此 `TraeWork`（CN）与 `Global`（国际版 TraeWork）落到**同一条授权页产品线** ——
    /// 本机实测两台客户端的 `iCubeApp.authConfig` 逐字相同，也印证了这一点。
    pub fn oauth_line(self) -> OAuthLine {
        OAuthLine::from_package_type(variant_spec(self).package_type)
    }
}

/// 授权页的**产品线**（客户端 `iCubeApp.authConfig` 的两把钥匙）。
///
/// 它与 [`TraeRegion`](super::region::TraeRegion) **正交**，也与 [`TraeProgram`]
/// 同构但不是同一个东西：`TraeProgram` 是**本项目的执行轴**（写哪个目录、启动哪个 exe），
/// 本枚举是**上游授权页的入参选择器**。二者当前取值一一对应，但依据不同
/// （前者来自 userData/exe 名，后者来自客户端 `packageType` 的分派），
/// 所以**刻意不互相转换** —— 合并会让「授权页参数该按什么分」这个事实被藏起来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OAuthLine {
    /// SOLO 产品线（`packageType` ∈ {`SOLO_CN`, `SOLO_I18N`, `SOLO_CN_ENTERPRISE`}）。
    Solo,
    /// TRAE / IDE 产品线（其余 `packageType`）。
    Trae,
}

impl OAuthLine {
    /// 客户端的 `packageType → 产品线` 分派（**逐字照抄**，含企业版取值）。
    ///
    /// 未登记的取值一律落 [`OAuthLine::Trae`] —— 与客户端 `? :` 的 `else` 分支同向。
    /// 本项目三条变体的 `packageType` 都已登记，故这只是一条与上游对齐的兜底。
    ///
    /// ⚠️ **不是 `const fn`**：`match` 一个 `&str` 在 const 上下文里尚未稳定
    /// （`cannot match on str in constant functions`）。本函数不需要 const，
    /// 真正需要 const 的是 [`OAuthLine::default_client_id`]（`mod.rs` 的常量引用它）。
    pub fn from_package_type(package_type: &str) -> OAuthLine {
        match package_type {
            "SOLO_CN" | "SOLO_I18N" | "SOLO_CN_ENTERPRISE" => OAuthLine::Solo,
            _ => OAuthLine::Trae,
        }
    }

    /// 授权页 `auth_from` 参数（客户端同名字符串）。
    pub const fn auth_from(self) -> &'static str {
        match self {
            OAuthLine::Solo => "solo",
            OAuthLine::Trae => "trae",
        }
    }

    /// 内置 `client_id` 默认值 —— 即客户端 `product.json` 的
    /// `iCubeApp.authConfig.<本产品线>.stable`（本机 2026-09-21 实测；
    /// **两条区域的客户端取值逐字相同**，所以它只按产品线分，不按区域分）。
    ///
    /// ⚠️ 拿另一条线的值去发登录，授权页会停在 billing status 后**不回跳**
    /// （参考实现踩过：拿 SOLO 的值 `en1oxy7wnw8j9n` 去走 IDE 的流程）。
    pub const fn default_client_id(self) -> &'static str {
        match self {
            OAuthLine::Solo => "en1oxy7wnw8j9n",
            OAuthLine::Trae => "ono9krqynydwx5",
        }
    }

    /// 是否追加 `hide_saas_login=true`。
    ///
    /// 客户端原文：`auth_from === "solo" && (url += "&hide_saas_login=true")`
    /// —— 它是 `auth_from` 的**从属**参数，不是独立开关，因此不单独配置。
    pub const fn hide_saas_login(self) -> bool {
        matches!(self, OAuthLine::Solo)
    }

    /// `DeviceInfo.PlatformCode` —— 同样是**产品线级**的取值。
    ///
    /// 客户端原文（本机 2026-09-24 读 `out/main.js`，逐字）：
    /// ```js
    /// k(){ return gr(this.d) ? "SOLO_PC" : "IDE_PC" }   // gr = 是否 SOLO_Lite
    /// ```
    /// 与 `auth_from` / `client_id` 用的是**同一个**判定（`Su(t) === "SOLO_Lite"`），
    /// 因此必须与它们同源派生 —— 分成两个常量必然漂移。
    ///
    /// ## 为什么这条值得单独写一段注释（真机现场）
    ///
    /// 2026-09-24 用户报「TraeWork（SOLO 线）OAuth 登录失败，上游回
    /// `20403/040036 Token device not match`」。抓真机对照发现：本实现把
    /// `PlatformCode` 写死成 `IDE_PC`（从参考实现 `oauth.rs` 逐字抄来，而参考
    /// 固化的是 **TraeCode/IDE 线**），而真机客户端在 SOLO 线上报的是 **`SOLO_PC`**。
    /// 这正是「移植参考实现前先问这段抓自哪条产品线」那条教训的漏网之鱼。
    pub const fn platform_code(self) -> &'static str {
        match self {
            OAuthLine::Solo => "SOLO_PC",
            OAuthLine::Trae => "IDE_PC",
        }
    }
}

impl Default for TraeVariant {
    /// 默认 `TraeWork`。
    ///
    /// 选它而非 `Trae` 的理由与参考实现一致：`TraeWorkAssistant` 的
    /// `TargetApp::parse` 对未知值**回退 `TraeWork`**，且 `TRAE SOLO CN` 是本机
    /// 最近活跃的产品线。**注意这只是一处"缺省"**，不是"另一条线不支持"。
    fn default() -> Self {
        TraeVariant::TraeWork
    }
}

/// 一套端点取值（对应 `product.json` 里某个 region 键下的全部主机）。
///
/// 字段名对齐 [`super`] 里既有的常量语义，便于逐字对拍。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointSet {
    /// 账号中心 / 签到 / 积分基址（`bootConfig.account.trae.*` / `ug.trae.*`）。
    pub account_base: &'static str,
    /// iCube / 市场 / ASR 基址（`bootConfig.iCube.*` / `market.trae.*` / `asr.domain.*`）。
    pub icube_base: &'static str,
    /// 对话（agent）网关基址（`bootConfig.agent.trae.*`，与 `icube_base` **不同主机**）。
    pub agent_host: &'static str,
    /// WebSocket 基址（`bootConfig.ws.trae.*`）。
    ///
    /// **未验证**：CN 值是实测读到的，国际化值同样读到了但**从未连通过**。
    pub ws_base: Option<&'static str>,
    /// 网页控制台 / **授权页**基址（`bootConfig.consoleHost`）。
    ///
    /// ⚠️ 这是**网页域，不是 API 域**：它与 `account_base` / `icube_base` 不同主机，
    /// 只用于「浏览器里打开的那一页」。授权页完整地址 = `${console_base}/authorization`，
    /// 拼法照抄客户端自身（`loginHost = bootConfig.consoleHost`，
    /// 再拼 `/authorization?login_version=1&…`，见 `out/main.js` 的 OAuth 构造段）。
    ///
    /// ⚠️ **必须按区域分家**：CN 是 `https://www.trae.cn`，国际版是 `https://www.trae.ai`。
    /// 拿 CN 域给国际版发登录 ⇒ 用户在**错的账号体系**的授权页上登录，
    /// 页面即使走完也不会把凭据回调回本机（域与客户端不匹配），
    /// 症状与「回调端口没人监听」几乎一样（停在「认证中」），极易误判成端口问题。
    pub console_base: &'static str,
}

/// 一个变体的全部差异。
#[derive(Debug, Clone, Copy)]
pub struct VariantSpec {
    /// 所属变体。
    pub variant: TraeVariant,
    /// 稳定的展示名（界面分区标题）。
    pub display_name: &'static str,
    /// 客户端 `product.json` 的 `nameAlias`（官方自称，用于诊断文案与核对）。
    pub name_alias: &'static str,
    /// 客户端 `product.json` 的 `packageType`。
    pub package_type: &'static str,
    /// userData 目录名候选（`%APPDATA%\<名字>`，按优先级）。
    ///
    /// 同一变体可能有多个名字：客户端 `dataFolderName` 都是 `.trae-cn`，
    /// 但 userData 用的是 `nameShort`。
    pub data_dir_names: &'static [&'static str],
    /// 可执行文件名候选（按优先级，与 [`VariantSpec::data_dir_names`] 一一对应）。
    pub exe_names: &'static [&'static str],
    /// 进程名（Windows 精简名，不带 `.exe`）候选。
    pub proc_names: &'static [&'static str],
    /// CN 端点集（有实测依据）。
    pub cn_endpoints: EndpointSet,
    /// 国际版端点集。
    ///
    /// **从未对真实上游跑通过**（本机未装国际版客户端、无可用凭据），
    /// 值取自 CN 客户端 `product.json` 里的 `SG`/`US` 键 —— 那是**客户端自己声明的**，
    /// 比我们的任何推断都可信，但仍不等于「服务端接受」。
    pub global_endpoints: Option<EndpointSet>,
}

impl VariantSpec {
    /// 取该变体的端点集。
    ///
    /// `international = false` 一律返回 CN 端点（有实测依据）；
    /// `true` 返回国际版端点 —— **可能为 `None`**，调用方必须显式处理，
    /// 不要用 CN 值兜底冒充国际化值（那会把请求打到错的域，且错误很难定位）。
    pub fn endpoints(&self, international: bool) -> Option<&EndpointSet> {
        if international {
            self.global_endpoints.as_ref()
        } else {
            Some(&self.cn_endpoints)
        }
    }

    /// 该变体在 userData 根目录下的候选路径（未做存在性过滤）。
    pub fn data_dir_candidates(&self, base: &std::path::Path) -> Vec<PathBuf> {
        self.data_dir_names
            .iter()
            .map(|name| base.join(name))
            .collect()
    }
}

/// Trae Work 变体（`TRAE SOLO` / `TRAE SOLO CN`）。
///
/// **CN 端点取值逐字等于改造前的 `modules::trae` 常量**，保证既有行为零变化：
/// 改造前 `TRAE_API_BASE = "https://api.trae.cn"`、`TRAE_OAUTH_BASE = "https://api.trae.com.cn"`、
/// `TRAE_AGENT_HOST = "https://trae-api-cn.mchost.guru"`。
///
/// ## ⚠️ 候选名与 `GLOBAL_SPEC` **重叠**（`TRAE SOLO`）—— 已知缺陷，勿在此处单独摘除
///
/// 本表的 `TRAE SOLO` 属于**国际版**客户端（`packageType = SOLO_I18N`，见 [`GLOBAL_SPEC`]），
/// 与 `TRAE SOLO CN`（国内版）不是同一个客户端。两者重叠的后果不是报错而是
/// **静默读错客户端**：`select_data_dir_for(TraeWork)`（按活跃度）在本机返回 `TRAE SOLO`
/// ⇒ 拿到国际版的设备凭证 / 目录。
///
/// **但"顺手摘掉它"会连带作废约 20 条护栏**：本表是目前**唯一**有"一个变体多个候选目录"
/// 的表，而 `select_data_dir_for` 与 `detect_data_dir_for` 的分叉、以及 R3/R5/R6 那批
/// 「登录态不在首个候选里」的用例**全部**靠它构造现场（2026-09-24 实测：摘掉后
/// `cargo test -p buddy-switch-core --lib` 30 条红）。`super::region` 的目标表
/// （`CN_TRAE_WORK.data_dir_names = ["TRAE SOLO CN"]` 等四个程序位**各一个名字**）
/// 一旦成为唯一来源，两个选择器就恒等 ⇒ 分叉机制整体失去意义。
///
/// ⇒ 正确做法是 `.trellis/tasks/09-21-trae-region-program-model` 的「仍未做 #1」
/// （`TraeVariant` → `(TraeRegion, TraeProgram)` 的类型收尾），**必须单独一轮**、
/// 连同那 20 条用例的去留一起决定。**不要**在别处顺手改这张表。
const TRAE_WORK_SPEC: VariantSpec = VariantSpec {
    variant: TraeVariant::TraeWork,
    display_name: "TraeWork CN",
    name_alias: "TraeWork CN",
    package_type: "SOLO_CN",
    data_dir_names: &["TRAE SOLO CN", "TRAE SOLO"],
    exe_names: &["TRAE SOLO CN.exe", "TRAE SOLO.exe"],
    proc_names: &["TRAE SOLO CN", "TRAE SOLO"],
    cn_endpoints: EndpointSet {
        account_base: "https://api.trae.cn",
        icube_base: "https://api.trae.com.cn",
        agent_host: "https://trae-api-cn.mchost.guru",
        ws_base: Some("wss://trae-ws-cn.mchost.guru/custom_model"),
        console_base: "https://www.trae.cn",
    },
    // 国际化取值来自**国际版客户端自己声明的** `bootConfig.<能力>.trae.normal`
    // （本机装在 `%LOCALAPPDATA%\Programs\TRAE SOLO`，`packageType = SOLO_I18N`）。
    // 主机名是权威值；接口路径与鉴权**从未对上游跑通**，见模块头「仍未验证的部分」。
    global_endpoints: Some(GLOBAL_ENDPOINTS),
};

/// Trae CN 变体（**国内** TraeCode，客户端 `Trae CN`）。
///
/// ## ⚠️ 候选名里**不再**含裸名 `Trae`（2026-09-28，issue #3）
///
/// 它曾经是 `["Trae CN", "Trae"]`。那个 `Trae` 其实是**国际版** TraeCode 的
/// userData 名（与 `TRAE SOLO CN` / `TRAE SOLO` 同款「国内带 CN 后缀」规则），
/// 于是产生一条**静默跨区误判**：只装了国际版 TraeCode 的机器上，
/// 国内版 TraeCode 的程序位会判成「已安装」并指向国际版的目录 ——
/// 切账号会把登录态写进另一个客户端。
///
/// 裸名已移交 [`GLOBAL_TRAE_CODE_SPEC`]；本变体现在**只有一个候选**。
/// 与 `TRAE_WORK_SPEC` 的 `TRAE SOLO` 重叠不同，这里的重叠**已修掉**
/// （那边是登记在案的待修缺陷，必须连同约 20 条依赖「多变体多候选目录」的护栏
/// 一起做，见该常量的说明）。
const TRAE_CN_SPEC: VariantSpec = VariantSpec {
    variant: TraeVariant::Trae,
    display_name: "TraeCode CN",
    name_alias: "TraeCode CN",
    package_type: "TRAE_CN",
    data_dir_names: &["Trae CN"],
    exe_names: &["Trae CN.exe"],
    proc_names: &["Trae CN"],
    cn_endpoints: EndpointSet {
        // 实测：与 Trae Work 变体**逐字相同** —— 产品线不改变端点，region 才改变。
        account_base: "https://api.trae.cn",
        icube_base: "https://api.trae.com.cn",
        agent_host: "https://trae-api-cn.mchost.guru",
        ws_base: Some("wss://trae-ws-cn.mchost.guru/custom_model"),
        console_base: "https://www.trae.cn",
    },
    // 同上：国际版客户端自述值。**产品线不改变端点，region 才改变端点** ——
    // 所以本变体的国际化端点与 `TRAE_WORK_SPEC` 逐字相同（有单测钉住）。
    global_endpoints: Some(GLOBAL_ENDPOINTS),
};

/// 国际版端点（**国际版客户端自述值**）—— 本文件的唯一来源，三处 spec 都引用它。
///
/// 取值来自国际版客户端 `<安装根>\resources\app\product.json` 的
/// `bootConfig.<能力>.trae.normal`（本机 `%LOCALAPPDATA%\Programs\TRAE SOLO`）。
const GLOBAL_ENDPOINTS: EndpointSet = EndpointSet {
    account_base: "https://grow-normal.trae.ai",
    icube_base: "https://icube-normal.trae.ai",
    agent_host: "https://core-normal.trae.ai",
    ws_base: Some("wss://wss-normal.trae.ai/custom_model"),
    // 授权页域：国际版客户端 `product.json` 的 `bootConfig.consoleHost` **实测值**
    // （本机 2026-09-21 读取，与 `homeUrl` 同值）。客户端自己的 OAuth 构造把它当
    // `loginHost` 用，见 `out/main.js`：`${loginHost}/authorization?login_version=1…`。
    console_base: "https://www.trae.ai",
};

/// **国际版区域**（`TraeWork` 国际构建，`packageType = SOLO_I18N`）。
///
/// 与两条 CN 产品线的区别：
/// - 候选名是**国际版客户端自己的**名字（`TRAE SOLO` / `TRAE SOLO.exe`），
///   不是 CN 那条的（`TRAE SOLO CN`）—— 混在一起用"按序取第一个存在的"会在
///   同机装两条线时永远命中 CN 那个，国际版被静默吞掉；
/// - 端点用国际版那套（本表里 `cn_endpoints` 这一格放的就是**它实际使用的端点**：
///   `EndpointSet` 的字段名是历史包袱，`Global` 没有"另一套"可切）。
const GLOBAL_SPEC: VariantSpec = VariantSpec {
    variant: TraeVariant::Global,
    display_name: "TraeWork",
    name_alias: "TraeWork",
    package_type: "SOLO_I18N",
    data_dir_names: &["TRAE SOLO"],
    exe_names: &["TRAE SOLO.exe"],
    proc_names: &["TRAE SOLO"],
    cn_endpoints: GLOBAL_ENDPOINTS,
    global_endpoints: None,
};

/// **国际版 TraeCode**（客户端 `Trae` / `Trae.exe`）—— 第 4 个程序位（issue #3）。
///
/// ## 依据与「未实测」的边界（必须分清）
///
/// | 项 | 依据 | 可信度 |
/// |:---|:---|:---|
/// | userData / exe / 进程名 `Trae` | 同族命名规则（国内带 `CN` 后缀、国际用裸名），且 **2026-10-13 由用户确认**：国际版 `Trae` / `Trae.exe`、国内版 `Trae CN` / `Trae CN.exe` | **已确认**（国内另有本机 `product.json` 实测） |
/// | 端点 | 国际版客户端自述值（与 `Global` 同源，**产品线不改变端点、region 才改变**） | 有实测依据 |
/// | `packageType` | **未实测**。仅用于 [`OAuthLine::from_package_type`] 的产品线分派，而该分派只区分 `SOLO_*` 与非 `SOLO_*`（见 `from_package_type` 的 `else` 分支）⇒ 任何非 `SOLO_*` 取值都落 **TRAE 线**，这正是 IDE 该用的那条钥匙（`ono9krqynydwx5`） | 推断，但**后果被分派逻辑钉死** |
///
/// ⇒ 猜错 `packageType` 的后果**有界**：它不会让请求打到错的域（域由
/// [`TraeVariant::region`] 决定），也不会换错产品线的钥匙（非 `SOLO_*` 一律 TRAE 线）。
/// 真正的风险只剩「本机根本没有国际版 TraeCode 客户端可探测」——
/// 那种情况下程序位显示「未检测到」，符合「绝不伪造成功」。
const GLOBAL_TRAE_CODE_SPEC: VariantSpec = VariantSpec {
    variant: TraeVariant::GlobalTraeCode,
    // 国际版**不带后缀**（用户 2026-10-13 更正的规则）；保留家族名 `TraeCode`，
    // 否则会与国际合并版（`Trae`）撞名。理由见 [`TraeVariant::display_name`]。
    display_name: "TraeCode",
    // 与 `region::GLOBAL_TRAE_CODE.name_alias` 同源，**显式标注未实测**。
    name_alias: "TraeCode（待实测）",
    package_type: "TRAE_I18N",
    data_dir_names: &["Trae"],
    exe_names: &["Trae.exe"],
    proc_names: &["Trae"],
    // 国际版变体**没有「另一套」可切**：端点直接放在 `cn_endpoints` 槽
    // （字段名是历史包袱），与 [`GLOBAL_SPEC`] 同款处理。
    cn_endpoints: GLOBAL_ENDPOINTS,
    global_endpoints: None,
};

/// **合并后的 CN 客户端**（issue #20）：客户端 `Trae CN`。
///
/// 与 [`TRAE_CN_SPEC`] **候选名逐字相同**（客户端合并时复用了 TraeCode 的目录名与
/// exe 名），差别只在**标识**（`trae_merged`）与**快照目录**
/// （`profiles_trae_merged`，见 [`super::paths::profiles_dir_for`]）。
///
/// `name_alias` 取客户端 `product.json` 的实测值（合并版 `Trae CN` 的 `nameAlias`
/// 仍是 `TraeCode CN` —— 上游没改这个字段，我们不替它改）。
const CN_TRAE_MERGED_SPEC: VariantSpec = VariantSpec {
    variant: TraeVariant::TraeMerged,
    display_name: "Trae CN",
    name_alias: "TraeCode CN",
    package_type: "TRAE_CN",
    data_dir_names: &["Trae CN"],
    exe_names: &["Trae CN.exe"],
    proc_names: &["Trae CN"],
    // ⚠️ **刻意逐字重写、不引用共享常量**：`两条产品线的cn端点逐字相同` 那条护栏
    // 断的是「三份**各自独立写出**的取值恰好相同」；改成一个共享常量后那条断言会
    // 恒真（同义反复），再也挡不住「有人给某条产品线单独改了端点」。
    cn_endpoints: EndpointSet {
        account_base: "https://api.trae.cn",
        icube_base: "https://api.trae.com.cn",
        agent_host: "https://trae-api-cn.mchost.guru",
        ws_base: Some("wss://trae-ws-cn.mchost.guru/custom_model"),
        console_base: "https://www.trae.cn",
    },
    // 同其余 CN 变体：产品线不改变端点、region 才改变端点。
    global_endpoints: Some(GLOBAL_ENDPOINTS),
};

/// **合并后的国际客户端**（issue #20）：客户端 `Trae` / `Trae.exe`。
///
/// ★ **候选名已由用户确认**（2026-10-13）：「国际版 `Trae` / `Trae.exe`、
/// 国内版 `Trae CN` / `Trae CN.exe`」—— 因此下面三个候选字段**不是推断**
/// （本机未装国际版合并客户端，但名字有权威来源；国内那份另有 `product.json` 实测）。
///
/// `packageType` 与 [`GLOBAL_TRAE_CODE_SPEC`] 一样是**未实测的推断值**；风险有界，
/// 理由见 [`TraeVariant::GlobalTraeMerged`]。
const GLOBAL_TRAE_MERGED_SPEC: VariantSpec = VariantSpec {
    variant: TraeVariant::GlobalTraeMerged,
    display_name: "Trae",
    name_alias: "TraeCode（待实测）",
    package_type: "TRAE_I18N",
    data_dir_names: &["Trae"],
    exe_names: &["Trae.exe"],
    proc_names: &["Trae"],
    // 国际版变体**没有「另一套」可切**：端点直接放在 `cn_endpoints` 槽
    // （字段名是历史包袱），与 [`GLOBAL_SPEC`] / [`GLOBAL_TRAE_CODE_SPEC`] 同款处理。
    cn_endpoints: GLOBAL_ENDPOINTS,
    global_endpoints: None,
};

/// 取变体的描述符。
pub fn variant_spec(variant: TraeVariant) -> &'static VariantSpec {
    match variant {
        TraeVariant::TraeWork => &TRAE_WORK_SPEC,
        TraeVariant::Trae => &TRAE_CN_SPEC,
        TraeVariant::Global => &GLOBAL_SPEC,
        TraeVariant::GlobalTraeCode => &GLOBAL_TRAE_CODE_SPEC,
        TraeVariant::TraeMerged => &CN_TRAE_MERGED_SPEC,
        TraeVariant::GlobalTraeMerged => &GLOBAL_TRAE_MERGED_SPEC,
    }
}

/// 全部**变体**的描述符（6 条），供**反查与不变式**遍历用。
///
/// ## 与 [`TraeVariant::all`] 的分工（别混用）
///
/// | 函数 | 口径 | 用途 |
/// |:---|:---|:---|
/// | [`all_specs`] | 6 条（区域 × 程序全覆盖） | [`variant_of_name`] 反查、端点/授权线不变式 |
/// | [`TraeVariant::all`] | **只**国内两条遗留程序位 | 只关心 CN 的历史护栏与 fixture |
///
/// 两者并存是**过渡态**：单轴（产品线）时代的遍历入口还没删，而区域轴的完整
/// 遍历入口是 [`super::region::TraeRegion::all`] / [`super::region::all_program_specs`]。
///
/// ## ⚠️ 顺序**不是**任意的：合并版必须排在遗留 TraeCode 之前
///
/// [`variant_of_name`] 按本顺序取**首个**精确命中的 spec。合并版与遗留 TraeCode 的
/// 候选名逐字相同（`Trae CN` / `Trae`），所以顺序决定了「一台机器上的 `Trae CN`
/// 该被认成哪一代客户端」。合并版在前 ⇒ 反查给 `TraeMerged`，
/// 界面上那条「探测到的是哪条产品线」才会说 **Trae** 而不是 TraeCode ——
/// 用户已经因为「点 Trae 却提示 TraeCode」报过一次障。
pub fn all_specs() -> [&'static VariantSpec; 6] {
    [
        &TRAE_WORK_SPEC,
        &CN_TRAE_MERGED_SPEC,
        &TRAE_CN_SPEC,
        &GLOBAL_SPEC,
        &GLOBAL_TRAE_MERGED_SPEC,
        &GLOBAL_TRAE_CODE_SPEC,
    ]
}

/// 从 userData 目录名 / 安装目录名 / exe 名反查变体。
///
/// 这是「探测到的是哪条产品线」的唯一判定入口：先用目录名精确匹配，
/// 再退化到「包含关系」（容忍 `TRAE SOLO CN` 之类的完整名与 `solo` 之类的片段）。
///
/// 返回 `None` 表示**不认识**这个名字 —— 调用方应保持"跨变体"的宽容行为
/// （例如仍然把它当候选目录），而不是硬判成某一个变体。
pub fn variant_of_name(name: &str) -> Option<TraeVariant> {
    let lowered = name.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }
    // 去 `.exe` 后缀：调用方可能传 exe 文件名。
    let lowered = lowered.strip_suffix(".exe").unwrap_or(&lowered).to_string();

    // 先精确匹配全部候选名（含大小写差异）。
    for spec in all_specs() {
        for candidate in spec.data_dir_names.iter().chain(spec.exe_names.iter()) {
            if candidate.eq_ignore_ascii_case(&lowered) {
                return Some(spec.variant);
            }
        }
    }

    // 再退化到包含关系。`solo` 是 Trae Work 的独有词根（`TRAE SOLO`），
    // 放在 `trae` 之前判定，否则 `TRAE SOLO CN` 会被 `trae` 抢先命中。
    //
    // ⚠️ 两条**已修**的重叠不再靠这里兜底：`Trae` / `Trae.exe` 现在精确命中
    //   [`GLOBAL_TRAE_CODE_SPEC`]（国际版 TraeCode），不会再落回 `Trae`（国内）。
    //
    // ⚠️ 剩余的一处（与 `TRAE_WORK_SPEC` 的候选名重叠同源）：`TRAE SOLO` 会落到
    //   `TraeWork`（国内），而它其实是**国际版**客户端的 userData 名。修它等于摘掉
    //   那条重叠，见 `TRAE_WORK_SPEC` 的说明 —— 必须与那 20 条护栏一起单独一轮处理。
    if lowered.contains("solo") {
        return Some(TraeVariant::TraeWork);
    }
    if lowered.contains("trae") {
        return Some(TraeVariant::Trae);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 变体标识与展示名稳定() {
        assert_eq!(TraeVariant::TraeWork.as_str(), "trae_work");
        // ★ `Trae` 的字符串**是** `trae_cn`（持久化契约，见 `as_str` 的说明）：
        // 它已写进用户的 Key 记录、网关日志与快照目录名，改名会断老数据。
        assert_eq!(TraeVariant::Trae.as_str(), "trae_cn");
        assert_eq!(TraeVariant::Global.as_str(), "global");
        // ★ 国际版 TraeCode（issue #3）：字符串决定快照目录名
        // （`paths::profiles_dir_for` 的 `profiles_<as_str>` 规则），
        // 必须与 `paths::profiles_dir_for_program(Global, TraeCode)` 的取值一致。
        assert_eq!(TraeVariant::GlobalTraeCode.as_str(), "global_trae_code");
        assert_eq!(TraeVariant::TraeWork.display_name(), "TraeWork CN");
        // 本取值代表**遗留** TraeCode 程序位（`TraeMerged` 才是合并版）
        // ⇒ 展示名取它的 `nameAlias` 写法 `TraeCode CN`（国内带 ` CN` 后缀）。
        assert_eq!(TraeVariant::Trae.display_name(), "TraeCode CN");
        assert_eq!(TraeVariant::Global.display_name(), "TraeWork");
        // 与 `region::GLOBAL_TRAE_CODE.display_name` / 前端国际化程序名同源。
        // 国际版不带后缀，但**保留家族名** ⇒ `TraeCode`（若简称 `Trae` 会与合并版撞名）。
        assert_eq!(TraeVariant::GlobalTraeCode.display_name(), "TraeCode");
        assert_eq!(TraeVariant::default(), TraeVariant::TraeWork);
        // 标识符已正名为 `Trae`，但历史字符串仍可解析（两侧都可读）。
        assert_eq!(TraeVariant::parse("trae"), Some(TraeVariant::Trae));
        assert_eq!(TraeVariant::parse("trae_cn"), Some(TraeVariant::Trae));

        // ★★ 合并后的客户端（issue #20）：**自己的标识**，且与遗留槽位不同 ——
        // 这个字符串直接决定快照目录名（`profiles_<as_str>`）与 Key 归属 / 日志分档。
        // 反例（改坏会红）：把它的 `as_str` 写成 `trae_cn` ⇒ 快照又与 TraeCode 挤一起。
        assert_eq!(TraeVariant::TraeMerged.as_str(), "trae_merged");
        assert_eq!(TraeVariant::GlobalTraeMerged.as_str(), "global_trae_merged");
        assert_ne!(
            TraeVariant::TraeMerged.as_str(),
            TraeVariant::Trae.as_str(),
            "合并版与遗留 TraeCode 的标识必须不同"
        );
        assert_ne!(
            TraeVariant::GlobalTraeMerged.as_str(),
            TraeVariant::GlobalTraeCode.as_str(),
            "国际版合并版与遗留国际版 TraeCode 的标识必须不同"
        );
        // 展示名：国内带 ` CN`、国际不带后缀（用户 2026-10-13 更正的规则）。
        assert_eq!(TraeVariant::TraeMerged.display_name(), "Trae CN");
        assert_eq!(TraeVariant::GlobalTraeMerged.display_name(), "Trae");
        // ★★ 六个展示名必须**两两不同** —— 展示名是用户区分程序位的唯一依据
        // （Key 归属下拉 / 按钮 tooltip / 切换 toast）。撞名时用户在下拉里看到两个
        // 一模一样的选项，选错就是把登录态写进另一条线，而界面上看不出异常。
        // 本仓真实踩过：国际 TraeCode 曾叫 `Trae AI`，与国际合并版撞名。
        // 反例（改坏会红）：把任意两条 `display_name` 改成同一个值。
        {
            let mut seen: Vec<&str> = Vec::new();
            for variant in [
                TraeVariant::TraeWork,
                TraeVariant::Trae,
                TraeVariant::TraeMerged,
                TraeVariant::Global,
                TraeVariant::GlobalTraeCode,
                TraeVariant::GlobalTraeMerged,
            ] {
                let name = variant.display_name();
                assert!(
                    !seen.contains(&name),
                    "展示名 `{name}` 被两个变体共用（{variant:?}）—— 用户分不清是哪条程序位"
                );
                seen.push(name);
            }
            assert_eq!(seen.len(), 6, "前置：应恰有 6 个变体，否则本用例会恒真");
        }
        // 三个 CN 取值两两不同（否则「按变体分档」等于没分）。
        assert_ne!(TraeVariant::TraeMerged, TraeVariant::Trae);
        assert_ne!(TraeVariant::TraeMerged, TraeVariant::TraeWork);
        // `parse` 接受自己的落盘标识（`as_str` 往返）。
        for variant in [
            TraeVariant::TraeMerged,
            TraeVariant::GlobalTraeMerged,
        ] {
            assert_eq!(
                TraeVariant::parse(variant.as_str()),
                Some(variant),
                "{:?} 的 as_str 无法往返",
                variant
            );
        }
    }

    /// ★ 四个程序位到「持久化区域」的映射必须两两正确（issue #3 新增第 4 个）。
    ///
    /// 这条是**跨区误判的总闸门**：`region()` 决定账号库 / 快照 / 冷却 / 端点落哪一套。
    /// 把 `GlobalTraeCode` 误映射成 `Cn`，症状是「国际版 TraeCode 的账号进了国内库」，
    /// 不报错、只是数据串了。
    #[test]
    fn 四个程序位映射到正确的区域() {
        use super::super::region::TraeRegion;
        assert_eq!(TraeVariant::TraeWork.region(), TraeRegion::Cn);
        assert_eq!(TraeVariant::Trae.region(), TraeRegion::Cn);
        assert_eq!(TraeVariant::Global.region(), TraeRegion::Global);
        assert_eq!(TraeVariant::GlobalTraeCode.region(), TraeRegion::Global);
        // 合并后的客户端（issue #20）：CN 合并版落国内、国际合并版落国际。
        assert_eq!(TraeVariant::TraeMerged.region(), TraeRegion::Cn);
        assert_eq!(TraeVariant::GlobalTraeMerged.region(), TraeRegion::Global);
        // `all_specs()` 的口径必须与 `region()` 自洽（每条 spec 的 `variant` 字段
        // 与它自己的 region 对得上），否则「表里写着 A、跑起来落 B」。
        for spec in all_specs() {
            assert_eq!(
                variant_spec(spec.variant).variant.region(),
                spec.variant.region(),
                "{} 的 region 与表不自洽",
                spec.name_alias
            );
        }
    }

    /// ★ 国际版 TraeCode 的 `parse` 必须与**区域标识** `global` 分家（issue #3）。
    ///
    /// 两者只差一个后缀，混用会把「切到国际版主程序」变成「切到国际版 TraeCode」——
    /// 登录态写进错的客户端，且界面上看不出异常。
    #[test]
    fn 国际版traecode与区域标识分家() {
        for s in [
            "global_trae_code",
            "global_traecode",
            "trae_code_global",
            "traecodeglobal",
            "trae global",
            "GLOBAL_TRAE_CODE",
        ] {
            assert_eq!(
                TraeVariant::parse(s),
                Some(TraeVariant::GlobalTraeCode),
                "解析失败: {s}"
            );
        }
        // `global` 仍是**区域标识**，落该区域的主程序（TraeWork）。
        for s in ["global", "intl", "international", "trae_global"] {
            assert_eq!(TraeVariant::parse(s), Some(TraeVariant::Global), "解析失败: {s}");
        }
        // `trae_code`（无区域前缀）是**国内** TraeCode —— 不能顺手改成国际版。
        assert_eq!(TraeVariant::parse("trae_code"), Some(TraeVariant::Trae));
    }

    /// ★ 裸名 `Trae` / `Trae.exe` 必须反查成**国际版客户端**（issue #3 的核心修复，
    /// issue #20 后指向**合并版**国际客户端）。
    ///
    /// 反例（改坏会红）：把它留在 `TRAE_CN_SPEC` 的候选里 ⇒ 只装了国际版客户端的
    /// 机器上，**国内版**程序位会被判成「已安装」并指向国际版目录。
    ///
    /// ⚠️ issue #20 起 `all_specs()` 里**合并版排在遗留 TraeCode 之前**，因此
    /// `Trae` / `Trae CN` 现在反查成合并版（`GlobalTraeMerged` / `TraeMerged`）——
    /// 这是刻意的：合并版才是当前现实，界面上的「探测到哪条产品线」不该说 TraeCode。
    #[test]
    fn 裸名反查落到合并版国际客户端且不落国内() {
        assert_eq!(variant_of_name("Trae"), Some(TraeVariant::GlobalTraeMerged));
        assert_eq!(variant_of_name("Trae.exe"), Some(TraeVariant::GlobalTraeMerged));
        assert_eq!(variant_of_name("trae"), Some(TraeVariant::GlobalTraeMerged));
        // ★ issue #3 的那条不变式仍要成立：裸名**绝不能**被读成国内客户端。
        assert_ne!(variant_of_name("Trae"), Some(TraeVariant::TraeWork));
        assert_ne!(variant_of_name("Trae"), Some(TraeVariant::Trae));
        // 带 CN 后缀的落**合并版国内**客户端（不是国际版）。
        assert_eq!(variant_of_name("Trae CN"), Some(TraeVariant::TraeMerged));
        assert_eq!(variant_of_name("Trae CN.exe"), Some(TraeVariant::TraeMerged));
        // 两个程序位的目录名互不包含（`Trae` 是 `Trae CN` 的子串）—— 靠**精确匹配**分开。
        assert_ne!(
            variant_of_name("Trae CN"),
            variant_of_name("Trae"),
            "包含关系必须靠精确匹配拆开，不能靠包含匹配"
        );
    }

    #[test]
    fn 解析接受目录名与常见别名() {
        for s in ["trae_work", "TraeWork", "Trae Work", "solo", "TRAE SOLO CN"] {
            assert_eq!(TraeVariant::parse(s), Some(TraeVariant::TraeWork), "解析失败: {s}");
        }
        for s in ["trae_cn", "Trae", "Trae CN", "ide", "trae"] {
            assert_eq!(TraeVariant::parse(s), Some(TraeVariant::Trae), "解析失败: {s}");
        }
        assert_eq!(TraeVariant::parse("doubao"), None);
        assert_eq!(TraeVariant::parse(""), None);
    }

    /// 反查的核心判别式：`solo` 必须先于 `trae` 判定。
    /// 若顺序写反，`TRAE SOLO CN` 会命中 `trae` 分支被误判成 Trae CN。
    #[test]
    fn 反查时solo优先于trae() {
        assert_eq!(variant_of_name("TRAE SOLO CN"), Some(TraeVariant::TraeWork));
        assert_eq!(variant_of_name("TRAE SOLO"), Some(TraeVariant::TraeWork));
        assert_eq!(variant_of_name("TRAE SOLO CN.exe"), Some(TraeVariant::TraeWork));
        // ⚠️ issue #20：`Trae CN` 现在落**合并版**（`all_specs()` 里合并版在前）。
        assert_eq!(variant_of_name("Trae CN"), Some(TraeVariant::TraeMerged));
        assert_eq!(variant_of_name("Trae CN.exe"), Some(TraeVariant::TraeMerged));
        // 目录名不区分大小写。
        assert_eq!(variant_of_name("trae solo cn"), Some(TraeVariant::TraeWork));
        // 不认识的平台（豆包）不能被硬判成某个 Trae 变体。
        assert_eq!(variant_of_name("Doubao"), None);
    }

    /// 四个程序位的候选名**必须两两不重叠**（唯一例外见下）——重叠会导致探测时互相抢，
    /// 出现「选中 A 的安装、读 B 的 userData」。
    ///
    /// ## 这条用例 2026-09-28 扩到了全部 4 个程序位（issue #3）
    ///
    /// 此前它**只比 `TraeWork` vs `Trae`**：`TraeWork` 与 `Global` 在 `TRAE SOLO` 上
    /// 确实重叠，而修它必须连同约 20 条依赖「一个变体多个候选目录」的护栏一起做
    /// （见 `TRAE_WORK_SPEC` 的说明），当时把范围扩到 `Global` 只会得到一条**恒红**断言。
    ///
    /// 现在仍留着那**一条**例外，但把它**显式白名单化**：断言的是「重叠集合恰好等于
    /// 这 3 项」，而不是「随便重叠都行」。白名单外的任何新增重叠都会红。
    ///
    /// ## 为什么 TraeCode 那一侧必须**零例外**
    ///
    /// issue #3 的根因就是那里的一条重叠（裸名 `Trae` 同时属于国内与国际 TraeCode）。
    /// 所以下面单独再钉一遍：**TraeCode 的两个程序位与所有其它槽位都不重叠**。
    #[test]
    fn 四个程序位的候选名互不重叠() {
        let specs = all_specs();
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
                            overlaps.push(format!("{kind}:{name}:{:?}/{:?}", a.variant, b.variant));
                        }
                    }
                }
            }
        }
        overlaps.sort();
        // 登记在案的例外**恰有两类**：
        //
        // 1. `TRAE SOLO` 属于**国际版** TraeWork，却仍留在 CN TraeWork 的候选里
        //    （`TRAE_WORK_SPEC` 的待修缺陷，必须单独一轮处理）；
        // 2. **合并版与它取代的那条遗留线候选名逐字相同**（issue #20：客户端合并时
        //    复用了 TraeCode 的目录名与 exe 名）⇒ `TraeMerged`↔`Trae`、
        //    `GlobalTraeMerged`↔`GlobalTraeCode` 各重叠 3 项。
        //    两者**变体不同**（快照目录 / Key 归属 / 日志分档各自独立），
        //    所以这是「同名不同槽位」，不是跨区误判。
        assert_eq!(
            overlaps,
            vec![
                "exe 名:TRAE SOLO.exe:TraeWork/Global".to_string(),
                "exe 名:Trae CN.exe:TraeMerged/Trae".to_string(),
                "exe 名:Trae.exe:GlobalTraeMerged/GlobalTraeCode".to_string(),
                "userData 目录名:TRAE SOLO:TraeWork/Global".to_string(),
                "userData 目录名:Trae CN:TraeMerged/Trae".to_string(),
                "userData 目录名:Trae:GlobalTraeMerged/GlobalTraeCode".to_string(),
                "进程名:TRAE SOLO:TraeWork/Global".to_string(),
                "进程名:Trae CN:TraeMerged/Trae".to_string(),
                "进程名:Trae:GlobalTraeMerged/GlobalTraeCode".to_string(),
            ],
            "候选名重叠集合变了 —— 白名单外的新重叠必须先修掉（或显式登记理由）"
        );

        // ★ 遗留 TraeCode 的两个槽位（国内 `Trae CN` / 国际 `Trae`）只允许与
        //   **自己那条合并版**重叠，与其余任何槽位都不许重叠。
        //   反例就是 issue #3 修掉的那条：裸名 `Trae` 曾同时挂在**国内** TraeCode 的
        //   候选里 ⇒ 装了国际版 TraeCode 的机器上国内程序位被判成已安装。
        for (code, merged) in [
            (variant_spec(TraeVariant::Trae), TraeVariant::TraeMerged),
            (
                variant_spec(TraeVariant::GlobalTraeCode),
                TraeVariant::GlobalTraeMerged,
            ),
        ] {
            for other in all_specs() {
                if other.variant == code.variant || other.variant == merged {
                    continue;
                }
                for (kind, mine, theirs) in [
                    ("userData 目录名", code.data_dir_names, other.data_dir_names),
                    ("exe 名", code.exe_names, other.exe_names),
                    ("进程名", code.proc_names, other.proc_names),
                ] {
                    for name in mine {
                        assert!(
                            !theirs.contains(name),
                            "TraeCode 的 {kind} `{name}` 与 {:?} 重叠（跨区误判来源）",
                            other.variant
                        );
                    }
                }
            }
        }

        // ★★ 合并版与它的遗留对应槽位**必须**候选名逐字相同 —— 那是「同一个客户端的
        //    下一代构建」这个事实的直接体现；一旦有人给其中一条改了候选名，
        //    合并后的机器上就会有一枚按钮永远探测不到。反向：变体必须**不同**
        //    （否则快照目录又会挤在一起，正是 issue #20 要修的）。
        for (legacy, merged) in [
            (variant_spec(TraeVariant::Trae), variant_spec(TraeVariant::TraeMerged)),
            (
                variant_spec(TraeVariant::GlobalTraeCode),
                variant_spec(TraeVariant::GlobalTraeMerged),
            ),
        ] {
            assert_eq!(legacy.data_dir_names, merged.data_dir_names);
            assert_eq!(legacy.exe_names, merged.exe_names);
            assert_eq!(legacy.proc_names, merged.proc_names);
            assert_ne!(
                legacy.variant.as_str(),
                merged.variant.as_str(),
                "合并版必须有**自己的**变体标识，否则快照目录会与遗留槽位共用"
            );
        }
    }

    /// ★★ 授权页**产品线**必须由 `packageType` 派生（客户端自己的分派），
    /// 且**只按产品线分、不按区域分**。
    ///
    /// 依据（本机 2026-09-21 读客户端 `out/main.js`，逐字）：
    /// ```js
    /// function Su(t){ return t.packageType===SOLO_CN || t.packageType===SOLO_I18N
    ///                       || t.packageType===SOLO_CN_ENTERPRISE ? "SOLO_Lite" : "TRAE" }
    /// clientId = Pr(t) ? authConfig.SOLO[channel] : authConfig.TRAE[channel]
    /// authFrom = Su(t)==="SOLO_Lite" ? "solo" : "trae"
    /// ```
    /// 而本机三台客户端的 `iCubeApp.authConfig` **逐字相同**：
    /// `SOLO.stable = en1oxy7wnw8j9n`、`TRAE.stable = ono9krqynydwx5`
    /// ⇒ 钥匙是**产品线级**的，不是区域级的（否则 CN 与国际版会各有一套）。
    ///
    /// 缺陷形态（本轮修掉的那个）：三处全用 IDE 那把钥匙 + `auth_from=trae`，
    /// 于是两条 SOLO 线的登录都拿错了钥匙 ⇒ 授权页停在 billing status 后不回跳。
    #[test]
    fn 授权页产品线由package_type派生且不按区域分() {
        // 三条变体各自的产品线。
        assert_eq!(
            variant_spec(TraeVariant::TraeWork).variant.oauth_line(),
            OAuthLine::Solo,
            "TraeWork（SOLO_CN）必须落 SOLO 线"
        );
        assert_eq!(
            variant_spec(TraeVariant::Global).variant.oauth_line(),
            OAuthLine::Solo,
            "国际版 TraeWork（SOLO_I18N）必须落 SOLO 线 —— 它与 CN 是同一条产品线"
        );
        assert_eq!(
            variant_spec(TraeVariant::Trae).variant.oauth_line(),
            OAuthLine::Trae,
            "Trae（TRAE_CN）必须落 TRAE 线"
        );
        // 合并后的客户端（issue #20）：它是 IDE 那条线的下一代构建
        // （`packageType` 实测仍是 `TRAE_CN`）⇒ 必须落 **TRAE 线**。
        // 反例：误落 SOLO 线会拿 SOLO 的 client_id 去发登录 ⇒ 授权页停在 billing
        // status 后不回跳（本仓踩过一次的真实缺陷）。
        assert_eq!(
            variant_spec(TraeVariant::TraeMerged).variant.oauth_line(),
            OAuthLine::Trae,
            "合并版 CN（TRAE_CN）必须落 TRAE 线"
        );
        assert_eq!(
            variant_spec(TraeVariant::GlobalTraeMerged).variant.oauth_line(),
            OAuthLine::Trae,
            "合并版国际（TRAE_I18N）必须落 TRAE 线"
        );

        // 分派是 `packageType` 的函数，与区域无关：同一 `packageType` 必然同一产品线。
        for spec in all_specs() {
            assert_eq!(
                OAuthLine::from_package_type(spec.package_type),
                spec.variant.oauth_line(),
                "{} 的产品线没有从 package_type 派生",
                spec.name_alias
            );
        }
        // 企业版取值按客户端原样识别（本项目暂无该变体，但规则必须完整）。
        assert_eq!(OAuthLine::from_package_type("SOLO_CN_ENTERPRISE"), OAuthLine::Solo);
        assert_eq!(OAuthLine::from_package_type("TRAE_CN_ENTERPRISE"), OAuthLine::Trae);

        // 两条线的三件取值必须**两两不同**（否则「按线分家」等于没分）。
        assert_ne!(OAuthLine::Solo.auth_from(), OAuthLine::Trae.auth_from());
        assert_ne!(
            OAuthLine::Solo.default_client_id(),
            OAuthLine::Trae.default_client_id()
        );
        assert_ne!(OAuthLine::Solo.hide_saas_login(), OAuthLine::Trae.hide_saas_login());
        assert_ne!(OAuthLine::Solo.platform_code(), OAuthLine::Trae.platform_code());
        // 逐字值（与客户端 `authConfig` / `auth_from` / `k()` 分派对拍）。
        assert_eq!(OAuthLine::Solo.auth_from(), "solo");
        assert_eq!(OAuthLine::Trae.auth_from(), "trae");
        assert_eq!(OAuthLine::Solo.default_client_id(), "en1oxy7wnw8j9n");
        assert_eq!(OAuthLine::Trae.default_client_id(), "ono9krqynydwx5");
        assert!(OAuthLine::Solo.hide_saas_login());
        assert!(!OAuthLine::Trae.hide_saas_login());
        // `PlatformCode` 与 `auth_from` **同源判定**：SOLO 线是 SOLO_PC。
        // 反例就是 2026-09-24 那个真实缺陷（写死 IDE_PC ⇒ 上游 20403）。
        assert_eq!(OAuthLine::Solo.platform_code(), "SOLO_PC");
        assert_eq!(OAuthLine::Trae.platform_code(), "IDE_PC");
        for spec in all_specs() {
            assert_eq!(
                OAuthLine::from_package_type(spec.package_type).platform_code(),
                spec.variant.oauth_line().platform_code(),
                "{} 的 PlatformCode 没有从 package_type 派生",
                spec.name_alias
            );
        }
    }

    /// 两条产品线的 CN 端点**逐字相同**（实测结论）。
    /// 这个断言是"产品线不改变端点、region 才改变端点"的机器可读证据；
    /// 若有人给某条产品线单独改了端点，这里会红。
    #[test]
    fn 两条产品线的cn端点逐字相同() {
        let work = variant_spec(TraeVariant::TraeWork).cn_endpoints;
        let cn = variant_spec(TraeVariant::Trae).cn_endpoints;
        assert_eq!(work, cn);
        assert_eq!(work.account_base, "https://api.trae.cn");
        assert_eq!(work.icube_base, "https://api.trae.com.cn");
        assert_eq!(work.agent_host, "https://trae-api-cn.mchost.guru");

        // ★ 合并版（issue #20）也必须逐字相同 —— 它的 CN 端点是**独立重写**的
        // （刻意不引用共享常量，否则本用例会变成同义反复），所以这条断言有牙：
        // 谁给合并版单独改了端点，这里会红。
        assert_eq!(
            variant_spec(TraeVariant::TraeMerged).cn_endpoints,
            work,
            "合并版 CN 的端点与 TraeWork / TraeCode 不一致 —— 产品线不改变端点"
        );
    }

    /// 国际化端点必须是**国际版客户端自述**的那一组（2026-09-21 更正真实缺陷）。
    ///
    /// 反例就是本次改掉的那三个值：`api.trae.ai` / `api.trae.ai` /
    /// `grow-normal.trae.ai`。它们确实出现在**国内版**客户端的 `product.json` 里，
    /// 但属于别的能力表（CDN / 市场域）的 `SG`/`US` 键，不是
    /// `bootConfig.<能力>.trae.<regionKey>` 的取值；其中 `grow-normal.trae.ai`
    /// 在国际版客户端里是 **account** 基址，却被记成了 agent。
    /// 谁再按字符串搜国内版文件把它抄回去，这条会红。
    ///
    /// ## 为什么分成两拨断言（2026-09-28，issue #3）
    ///
    /// `all_specs()` 补上两个**国际区域**变体之后，「每个 spec 都要有
    /// `global_endpoints`」这个前提不再成立 —— 国际变体**没有「另一套」可切**，
    /// 它们的国际值直接放在 `cn_endpoints` 槽（字段名是历史包袱，见 `GLOBAL_SPEC`）。
    /// 所以按「有没有声明另一套」分两拨，各自断言自己那一拨该等于什么：
    /// 比原来那条「一律 expect」**更精确**（原来根本没校验国际变体自己的取值）。
    #[test]
    fn 国际化端点取自国际版客户端自述值() {
        let expected = EndpointSet {
            account_base: "https://grow-normal.trae.ai",
            icube_base: "https://icube-normal.trae.ai",
            agent_host: "https://core-normal.trae.ai",
            ws_base: Some("wss://wss-normal.trae.ai/custom_model"),
            console_base: "https://www.trae.ai",
        };
        let (declared, global_only): (Vec<&'static VariantSpec>, Vec<&'static VariantSpec>) =
            all_specs().into_iter().partition(|s| s.global_endpoints.is_some());
        // 前置：两拨都不能为空，否则本用例会**恒真**（没有对象可校验）。
        // 3/3 = 三条 CN 区域变体（TraeWork / TraeCode / 合并版）各声明了一套国际端点，
        // 三条国际区域变体各只有自己那一套（issue #20 后由 2/2 变 3/3）。
        assert_eq!(declared.len(), 3, "应恰有三条 CN 变体声明了国际版端点");
        assert_eq!(global_only.len(), 3, "应恰有三个国际区域变体");
        for spec in declared {
            assert_eq!(
                spec.global_endpoints.expect("刚 partition 过，必然有"),
                expected,
                "{} 的国际化端点漂了",
                spec.name_alias
            );
        }
        // 国际区域变体：它的 `cn_endpoints` 槽里放的就是**它实际使用的端点**。
        for spec in global_only {
            assert_eq!(
                spec.cn_endpoints, expected,
                "{} 的端点不是国际版客户端自述值",
                spec.name_alias
            );
            assert_eq!(
                spec.variant.region(),
                super::super::region::TraeRegion::Global,
                "{} 必须是国际区域变体",
                spec.name_alias
            );
        }
    }

    /// 国际化端点在**每个主机**上都必须与 CN 不同：混用会把请求打到错的域，
    /// 而且这种错误在日志里只表现为「401 / 超时」，极难定位。
    ///
    /// 另钉住 `ws_base` **必须已登记**：它此前是 `None`（"未验证"占位），
    /// 这条断言在本次更正前是**红的** —— 正因如此它才值得留在这里。
    ///
    /// 只对**同时有两套端点**的变体（两条 CN 线）成立：国际区域变体只有一套，
    /// 「与 CN 不同」对它们无意义（见上一条的分拨说明）。
    #[test]
    fn 国际化端点与cn端点主机全不同() {
        let mut checked = 0usize;
        for spec in all_specs() {
            let Some(global) = spec.global_endpoints else {
                continue;
            };
            checked += 1;
            assert_ne!(global.account_base, spec.cn_endpoints.account_base, "account 撞了");
            assert_ne!(global.icube_base, spec.cn_endpoints.icube_base, "iCube 撞了");
            assert_ne!(global.agent_host, spec.cn_endpoints.agent_host, "agent 撞了");
            assert_ne!(global.ws_base, spec.cn_endpoints.ws_base, "ws 撞了");
            // 授权页域同样必须分家：共用 CN 域 = 国际版用户在错的账号体系上登录。
            assert_ne!(
                global.console_base, spec.cn_endpoints.console_base,
                "授权页域（consoleHost）撞了"
            );
            assert!(
                global.ws_base.is_some(),
                "{} 的国际版 ws 端点缺失（应为国际版客户端自述的 wss 值）",
                spec.name_alias
            );
        }
        // 前置：必须有对象被校验过，否则本用例会**恒真**。
        // 3 = 三条 CN 区域变体（TraeWork / TraeCode / 合并版）各持有两套端点。
        assert_eq!(checked, 3, "应恰有三条变体同时持有 CN 与国际两套端点");
    }

    /// CN 端点必须等于改造前的硬编码常量原值 —— 保证既有行为零变化。
    #[test]
    fn cn端点等于改造前的常量原值() {        let spec = variant_spec(TraeVariant::TraeWork);
        assert_eq!(spec.cn_endpoints.account_base, super::super::TRAE_API_BASE_CN);
        assert_eq!(spec.cn_endpoints.icube_base, super::super::TRAE_OAUTH_BASE_CN);
    }

    /// `international = true` 而国际版端点缺失时必须返回 `None`，
    /// **绝不能**回落到 CN 值 —— 那会把国际化请求打到国内域。
    #[test]
    fn 国际版端点缺失时不回落cn() {
        let spec = variant_spec(TraeVariant::TraeWork);
        assert!(spec.endpoints(false).is_some());
        let global = spec.endpoints(true).expect("国际版端点应已登记");
        assert_ne!(global.account_base, spec.cn_endpoints.account_base);
        // 用一个刻意缺国际版端点的 spec 验证 None 语义。
        let mut stripped = *spec;
        stripped.global_endpoints = None;
        assert!(stripped.endpoints(true).is_none());
    }
}
