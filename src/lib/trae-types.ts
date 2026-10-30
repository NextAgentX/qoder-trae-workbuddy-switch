import { t } from "@/lib/i18n";
import type { GatewayPoolAccount, GatewayPoolSummary } from "@/lib/types";

/**
 * Trae 模块前端类型。
 *
 * **命名约定**：字段名与 Rust 侧响应体逐字一致（camelCase）。Trae 模块刻意让
 * 「磁盘、Tauri 响应、HTTP 响应」三处同形，因此这里不需要任何字段转换——
 * 若某天后端把某个字段改回 snake_case，TypeScript 不会报错但页面会读到
 * `undefined`，所以后端有专门的护栏测试钉住 camelCase（见 core 的
 * `settings_use_camel_case_on_disk_and_on_wire` / `account_view_exposes_camel_case_wire_fields`）。
 */

/** JWT 到期状态。 */
export type TraeJwtStatus = "ok" | "warn" | "expired" | "unknown";

/** 客户端环境状态（`get_trae_env`）。 */
export interface TraeEnvStatus {
  installed: boolean;
  running: boolean;
  version: string | null;
  path: string | null;
  dataDir: string | null;
  dataDirExists: boolean;
  platform: string;
  configuredPath: string | null;
  /**
   * `configuredPath` 是否**仍指向一个真实文件**（后端 `env_status`，issue #20）。
   *
   * ## 为什么要单独下发
   *
   * 后端在自定义路径失效时会**静默回落**到自动探测（客户端合并/升级会搬走甚至卸载
   * 旧 exe，不该因此卡住启动）。于是界面会显示「已检测到」，而设置页仍摆着一个
   * 不存在的路径 —— 用户无从知道该改。本字段就是给设置页做那条提示用的。
   *
   * `configuredPath` 为空时恒为 `false`；调用方**只在「填了且失效」时提示**。
   */
  configuredPathExists: boolean;
  /**
   * **按区域**配置的客户端路径（键 = {@link TraeRegionId}），未配置的区域为 `null`。
   *
   * ## 为什么必须有它（issue #20）
   *
   * `configuredPath` 是**全局视角**下的单值 —— 它取「检测到的那条线所属区域」的路径。
   * 设置页是**按区域**渲染的：用户在国际版页面上必须看到国际版的值，
   * 拿 `configuredPath` 会张冠李戴（在国内版页面看到国际版的路径）。
   * 因此后端把每个区域的值都下发，由设置页按当前区域取。
   */
  configuredPaths: Record<TraeRegionId, string | null>;
  /**
   * `configuredPaths` 每个区域的值是否**仍指向一个真实文件**（判据同
   * {@link configuredPathExists}）。设置页的「路径已失效」提示据此**按区域**判定。
   */
  configuredPathsExist: Record<TraeRegionId, boolean>;
  /**
   * 自动探测到的**产品线变体**稳定标识（`"trae_work"` / `"trae_cn"`）。
   *
   * 自动探测横跨全部变体挑最近活跃的那一个，所以界面必须说得出挑中的是谁。
   * 推不出来时为 `null`（调用方可省略标签，不要显示猜测值）。
   */
  variant: TraeVariantId | null;
  /**
   * 产品线**展示名**（`"TraeWork CN"` / `"Trae CN"`），可直接上界面。
   *
   * 与 Rust 侧 `variant::TraeVariant::display_name()` 同源；`Trae Work` 这个名字
   * 来自客户端 `product.json` 的 `nameAlias`（`TRAE SOLO CN` 自称 `TraeWork CN`）。
   */
  variantLabel: string | null;
}

/** 产品线变体标识。 */
/**
 * **区域标识**（持久化轴）：国内版 / 国际版。
 *
 * 账号库、分组、设备绑定、签到、积分、冷却、日志都按它分家 ——
 * 因为国内与国际是两套**互不相通**的账号体系。与 Rust 侧 `TraeRegion::as_str()` 同源。
 */
export type TraeRegionId = "cn" | "global";

/**
 * **程序位标识**（执行轴）：登录态写进哪个客户端、启动谁。
 *
 * 与 Rust 侧 `TraeProgram::as_str()` 同源。区域决定「账号属于哪套体系」，
 * 程序只决定「落到哪个客户端」。
 *
 * ## 三个取值的来历（issue #20）
 *
 * - `trae_work` / `trae_code`：改造前的两条产品线，各自对应一个客户端；
 * - `trae`：2026-10-13 客户端把上面两条**合并**成的一个客户端（CN `Trae CN`、
 *   国际 `Trae`）。
 *
 * ⚠️ `trae` 与 `trae_code` **候选名逐字相同**（客户端合并时复用了 TraeCode 的目录名
 * 与 exe 名），但它们是**两条不同的程序位**、两个不同的**变体**
 * （`trae_merged` / `global_trae_merged`）⇒ 快照目录、Key 归属、日志分档各自独立。
 * 这正是「Trae 是 Trae、TraeCode 是 TraeCode」在数据层的落点。
 */
export type TraeProgramId = "trae_work" | "trae_code" | "trae";

/**
 * 发给后端的「目标」标识 —— **区域与程序位共用一个字段**（后端宽容解析）。
 *
 * @deprecated P5 §2.6：`variant` 一词双义（既指区域、又指程序位），是 VOCABULARY §4 里
 * 最要命的混淆。**新代码不得再引入 `TraeVariantId`**，请改用两根正交轴：
 * {@link TraeRegionId}（持久化：账号库 / 端点）+ {@link TraeProgramId}（执行：写哪个客户端）。
 * 现有引用待逐段迁移（截至 2026-10-05 前端仍有 12 文件 81 处，故暂不能删除本类型）。
 *
 * `trae_work` / `trae_cn` 是改造前的产品线标识，现在表示**国内区域下的两个程序位**
 * （`trae_work`＝TraeWork 客户端、`trae_cn`＝TraeCode 客户端）。保留它们是为了：
 * 账号类接口按区域取库（两者都落国内库），而切换/快照类接口能据此确定**客户端**。
 *
 * `global_trae_code` 是**国际版 TraeCode**（与 `global`＝国际版 TraeWork 分家，
 * 见 `TraeVariant::GlobalTraeCode`）。它必须与 `global` 分开：
 * 两者只差一个后缀，混用会把登录态写进错的客户端。
 *
 * `trae_merged` / `global_trae_merged` 是**合并后的客户端**（issue #20）。
 * 它们与 `trae_cn` / `global_trae_code` **候选名逐字相同**但**变体不同** ——
 * 变体是快照目录 / Key 归属 / 日志分档的派生来源，共用变体会让两条程序位的快照
 * 挤进同一个目录（用户报障原文）。⚠️ 因此**绝不能**用「变体」反推程序位名字：
 * 那正是「点 Trae 却提示 TraeCode」那次缺陷的成因。
 */
export type TraeVariantId =
  | TraeRegionId
  | "trae_work"
  | "trae_cn"
  | "global_trae_code"
  | "trae_merged"
  | "global_trae_merged"
  | TraeProgramId;

/**
 * 程序位的展示名（收敛后只剩合并版 `Trae`）。
 *
 * ⚠️ 与 {@link traeVariantLabel} 的区别：本函数**只接程序位标识**。
 * 之所以仍然过词表，是为了让「程序位有哪些、各自叫什么」只有**一处**可查，
 * 将来新增程序位时 `zh.ts` 的 `TranslationKey` 会编译期提醒补英文条目。
 */
export function traeProgramLabel(program: TraeProgramId): string {
  // 收敛后唯一可选取值。`TraeWork` / `TraeCode` 是**退役**程序位：官方已把它们
  // 合并进 `Trae`（issue #28），界面上不再出现，返回合并版的名字才是事实。
  void program;
  return t("trae.program.trae");
}

/**
 * `TraeVariantId` → 展示名。
 *
 * 区域 → 客户端产品名；程序位 → 客户端的官方别名写法。
 * 与 Rust 侧 `TraeRegion::display_name()` / `program_spec().display_name` 同源。
 *
 * ★ **退役变体按其现在的落点显示**（issue #28 收敛）：`trae_work` / `trae_cn` /
 * `global` / `global_trae_code` 这四个历史标识在**老 Key 记录 / 老网关日志**里仍有，
 * 它们现在全部落到该区域的**合并版客户端** ⇒ 显示合并版的名字才是准确描述。
 * 显示 `TraeWork CN` 反而会指向一个已被卸载、不再可切换的客户端。
 *
 * **什么场景用它**：手上只有标识、需要**立刻**显示名字时（例如登录弹窗要说清
 * 「正在为哪条线登录」）。能拿到后端探测结果时优先用 `variantLabel`，避免前端多维护文案。
 */
export function traeVariantLabel(variant: TraeVariantId): string {
  switch (variant) {
    // 区域名（Trae 侧）：客户端自己的产品名，见 `trae.region.*` 的说明。
    case "cn":
      return t("trae.region.cn");
    case "global":
      return t("trae.region.global");
    // 合并后的客户端：两个区域各一个。
    case "trae_merged":
      return t("trae.program.traeCn");
    case "global_trae_merged":
      return t("trae.program.traeGlobal");
    // ⚠️ `trae` 有两种身份：作为**程序位标识**是合并版；作为**变体参数**是
    //   改造前的国内 TraeCode 落盘标识（`TraeVariant::parse("trae")`）。两者现在
    //   指向同一个客户端，故共用同一个展示名。
    case "trae":
      return t("trae.program.traeCn");
    // 退役的国内两条：老记录里出现，一律显示合并版的名字（见函数头）。
    case "trae_cn":
    case "trae_code":
    case "trae_work":
      return t("trae.program.traeCn");
    // 退役的国际两条：同上，落国际合并版。
    case "global_trae_code":
      return t("trae.program.traeGlobal");
    // 不可达（上面的分支已覆盖 `TraeVariantId` 的全部取值）；保留旧兜底值不动。
    default:
      return t("trae.program.trae");
  }
}

/**
 * `TraeVariantId` → **区域**展示名（国内版 / 国际版）。
 *
 * 与 {@link traeVariantLabel} 的分工：后者回答「这个标识自己是谁」（程序位就叫程序名），
 * 本函数只回答「它落在哪个区域」。
 *
 * ## 用途（2026-09-30 收窄）
 *
 * 原先它的两个用途之一是 API Key 的「归属」列 —— **那个用法已撤掉**：
 * 程序位现在真的决定行为（`/v1/models` 列哪份清单、请求体带哪个 `function`，
 * 见 `trae-api-key-table.tsx` 的 `PROGRAMS`），所以列里必须显示**程序位**，
 * 显示区域会让「TraeWork 与 TraeCode 两把 Key」看起来一模一样。
 * 现在它只服务于「只关心账号体系落在哪一档」的场景 —— 目前是 API Key 归属 badge
 * 的 `title`（提示该 Key 走哪本账号库）。
 *
 * ⚠️ 为什么不能直接用 `traeVariantLabel`：历史上写进 Key 记录的是**改造前的产品线标识**
 * （`trae_work` / `trae_cn`），而那两个产品线**都是国内构建**。照 `traeVariantLabel`
 * 显示就会得到「TraeWork / TraeCode」两个名字，而它们在**账号库**维度上完全相同
 * （同一本国内账号库、同一个池、Token 统计里同属「国内版」一档）——
 * 本函数回答的正是那个维度。
 *
 * 未建模的组合（例如国际版程序位，本机未安装）交回 {@link traeVariantLabel}，
 * **不猜**：宁可显示它的本名，也不要替后端发明一个区域。
 */
export function traeRegionLabelOf(variant: TraeVariantId): string {
  switch (variant) {
    case "global":
    // 国际版 TraeCode 也落国际区域（它与 `global` 是同一区域的两条程序）。
    case "global_trae_code":
    // 合并版国际客户端（issue #20）同样落国际区域。
    case "global_trae_merged":
      return t("trae.region.global");
    case "cn":
    case "trae_work":
    case "trae_cn":
    // 合并后的客户端 `trae` / `trae_merged` 落**国内**（后端
    // `TraeVariant::parse("trae") == Trae`、`TraeMerged.region() == Cn`）。
    case "trae":
    case "trae_merged":
      return t("trae.region.cn");
    default:
      return traeVariantLabel(variant);
  }
}

/**
 * `TraeVariantId` → **区域标识**（P5 T03）。
 *
 * ## 为什么需要它
 *
 * P5 把命令面按 `scope` 参数化，而 `scope` 是 `${平台}-${区域}` —— **只认区域**。
 * 但 Trae 侧的调用方手上常常只有一个 `TraeVariantId`（它把「区域 × 程序位」合成了一个字段，
 * 见类型注释），所以需要一个**明确的**「变体 → 区域」映射出口。
 *
 * ## 与 {@link traeRegionLabelOf} 的关系
 *
 * 两者是**同一套映射的两种输出**：那个返回展示名（给界面），本函数返回区域标识（给 scope）。
 * ⚠️ 故意分成两个函数、而不是「返回标识再翻译」：调用方要的是不同的东西，
 * 让每个调用点各写一遍 `.map(...)` 只会让映射散开。
 *
 * ## 为什么 `default` 抛错而不是猜
 *
 * `TraeVariantId` 是**闭合联合** ⇒ 上面已覆盖全部取值，`default` 在类型层面不可达。
 * 留 `never` 检查是让**将来新增变体时编译期就报错**，而不是静默落进某个默认区域 ——
 * 那会把登录态 / 账号库写进**错的区域**（`traeRegionLabelOf` 的注释也强调过「不猜」）。
 */
export function traeRegionOf(variant: TraeVariantId): TraeRegionId {
  switch (variant) {
    case "global":
    // 国际版 TraeCode 也落国际区域（它与 `global` 是同一区域的两条程序）。
    case "global_trae_code":
    // 合并版国际客户端（issue #20）。
    case "global_trae_merged":
      return "global";
    case "cn":
    // `trae_work` / `trae_cn` 是改造前的产品线标识，两者**都是国内构建**。
    case "trae_work":
    case "trae_cn":
    // `TraeProgramId` 的 `trae_code`：程序位不含区域，按其归属的国内库处理。
    case "trae_code":
    // `TraeProgramId` 的 `trae`（合并版客户端，issue #20）：同样落国内 ——
    // 后端 `TraeVariant::parse("trae") == Trae`（`region() == Cn`），
    // 而合并版变体 `trae_merged` 的 `region()` 也是 `Cn`。
    case "trae":
    case "trae_merged":
      return "cn";
    default: {
      const exhaustive: never = variant;
      throw new Error(`未知的 TraeVariantId：${String(exhaustive)}`);
    }
  }
}

/**
 * 单个**程序位**的安装/运行状态（`get_trae_variants().variants[].programs[]`）。
 *
 * 「区域 → 程序位」是两层：区域决定账号体系，程序位决定客户端。
 * 卡片上的切换按钮就是按它渲染的（每枚按钮对应一个程序位）。
 */
export interface TraeProgramStatus {
  /** 程序位标识（`trae_work` / `trae_code` / `trae`）。 */
  program: TraeProgramId;
  /**
   * 展示名。规则 = **国内版带 ` CN` 后缀、国际版不带后缀**，六个名字两两不同：
   * `TraeWork CN` / `TraeCode CN` / `Trae CN`（国内）· `TraeWork` / `TraeCode` / `Trae`（国际）。
   *
   * ★ 六个名字**逐字等于客户端自己的字符串**（`nameAlias` / `nameShort`）。
   * ⚠️ 更正前的约定是「国际版带 `AI` 后缀」—— 错的，且 `Trae AI` 会同时指
   * 国际 TraeCode 与国际合并版（Key 归属下拉里两个同名选项）。
   */
  label: string;
  /** 客户端 `product.json` 的官方别名（诊断与核对用）。 */
  nameAlias: string;
  /**
   * 切换时回传的标识。
   *
   * 自 2026-09-28（issue #3）起**每个程序位都有值**：此前 `(国际版, TraeCode)`
   * 未建模，后端回 `null`、按钮必须禁用。现在四个组合都已建模，
   * `null` 这个形态不再出现 —— 类型收成非空，是为了让「忘了建模」在编译期暴露，
   * 而不是在界面上表现成一枚灰按钮。
   */
  variant: TraeVariantId;
  installed: boolean;
  running: boolean;
  version: string | null;
  path: string | null;
  /**
   * 客户端**最近被用过**的 userData 目录（后端 `platform::select_data_dir_for`，按活跃度）。
   *
   * ⚠️ 它不是「登录态在哪个目录」—— 需要后者时用 {@link TraeProgramStatus.writeDataDir}。
   */
  dataDir: string | null;
  dataDirExists: boolean;
  /**
   * 该目录里**是否真的存在** `storage.json`（后端 `platform::storage_json_present`，issue #136）。
   *
   * 与 {@link dataDirExists} 是**两个问题**：目录在，文件不一定在。
   * 真机现场：`%APPDATA%\Trae CN` 有 Cache / Local Storage（客户端跑过），
   * 但 `User\globalStorage\storage.json` 从未生成 —— 此时 `dataDirExists = true`
   * 而本字段为 `false`，界面**不得**再说「已探测的登录态目录」。
   */
  storageJsonExists: boolean;
  /**
   * **写侧** userData 目录（后端 `platform::detect_data_dir_for`：候选表里**首个存在**的）。
   *
   * 与 {@link TraeProgramStatus.dataDir} 语义不同且**真机上不同值**：那是「客户端最近在用
   * 哪个」，这是「切换器**正在操作**哪个」—— 快照读写 / 保存守卫 / `overview_for().dataDir`
   * 用的都是它，即「登录态在哪」。
   *
   * 真机现场：`TRAE SOLO CN` 有登录态却更旧、更活跃的是 `TRAE SOLO`（**国际版**），
   * 于是拿 `dataDir` 展示会让国内版页签写出国际版目录。
   */
  writeDataDir: string | null;
  writeDataDirExists: boolean;
}

/**
 * 单条**区域**的环境状态（`get_trae_variants().variants[]`）。
 *
 * 与 [`TraeEnvStatus`] 的区别：`TraeEnvStatus` 是**自动挑中的那一条**（单一视角），
 * 本类型是**每个区域各自的状态**（并排视角）—— 界面顶部的区域切换器用它。
 *
 * 区域级字段（`installed` / `running` / `version` / …）是「该区域**任一**程序已装」
 * 的汇总（国际版目前只装了 TraeWork），精确到程序请看 `programs`。
 */
export interface TraeVariantStatus {
  variant: TraeRegionId;
  /**
   * 区域名（`"Trae CN"` / `"Trae"`，**客户端自己的产品名**），与 Rust
   * `TraeRegion::display_name()` 同源。
   *
   * ⚠️ 与 `shared.region.version.*`（`国内版` / `国际版`）**不是一回事**：后者是
   * **跨平台**的区域**词**，用于**组合**标签（作用域芯片 = 平台 + 区域词 ⇒「Trae 国内版」），
   * 且 WorkBuddy 也读它。两者漂了会让演示站与真机文案不同 —— 有跨层门禁钉住。
   */
  variantLabel: string;
  /**
   * 该区域**用户看得见的那一页**所在的域，是**可直接拼 URL 的基址**。
   *
   * 授权页 = `${consoleBase}/authorization`，「关于」外链 = `consoleBase` 本身。
   * 与 Rust `region_endpoints(region).console_base` 同源（客户端 `bootConfig.consoleHost`），
   * **域按区域分家**：国内 `https://www.trae.cn`、国际 `https://www.trae.ai`。
   *
   * 前端**不得**另立一份域常量 —— 漏改的症状不是报错，而是国际版用户被静默导到国内站。
   */
  consoleBase: string;
  installed: boolean;
  running: boolean;
  version: string | null;
  path: string | null;
  /** 主程序「最近被用过」的目录（见 {@link TraeProgramStatus.dataDir} 的告诫）。 */
  dataDir: string | null;
  dataDirExists: boolean;
  /**
   * 该目录里是否真的有 `storage.json`（与 {@link TraeProgramStatus.storageJsonExists} 同源）。
   *
   * 账号页空态用它区分「已探测的登录态目录」与「预期位置」——
   * 用 `dataDirExists` 判会把一个**空壳目录**说成「登录态在这儿」。
   */
  storageJsonExists: boolean;
  /** 主程序的**写侧**目录 —— 「客户端环境」行要核对「登录态在哪」时用它。 */
  writeDataDir: string | null;
  writeDataDirExists: boolean;
  /** 该区域下的程序位（卡片上每个账号要渲染的切换控件）。 */
  programs: TraeProgramStatus[];
}

/** 全部产品线的环境状态（`get_trae_variants`）。 */
export interface TraeVariantsStatus {
  platform: string;
  /** **顺序稳定**（与 Rust `TraeVariant::all()` 一致），可直接按序渲染图标。 */
  variants: TraeVariantStatus[];
}

/** 平台受限能力说明（`get_trae_capabilities`）。 */
export interface TraeUnsupported {
  capability: string;
  label: string;
  supportedOn: string;
  reason: string;
}

/** 平台能力清单。 */
export interface TraeCapabilities {
  platform: string;
  processControl: boolean;
  clientDetection: boolean;
  userDataDir: string | null;
  machineGuidReset: boolean;
  scheduledTask: boolean;
  unsupported: TraeUnsupported[];
}

/** 单个积分资源包的逐包明细（账号卡进度条的数据源）。 */
export interface TraeCreditPackage {
  packageCode: string | null;
  packageName: string | null;
  total: number;
  remaining: number;
  used: number;
  expireAt: number | null;
  expired: boolean;
  /** 是否临期（窗口由后端 `EXPIRING_SOON_SECS` 决定，现为 **14 天**）。 */
  expiringSoon: boolean;
}

/** 账号视图。 */
export interface TraeAccount {
  userId: string;
  name: string;
  groupId: string | null;
  jwt: string;
  jwtExpHours: number | null;
  jwtExpTimestamp: number | null;
  jwtStatus: TraeJwtStatus;
  checkedToday: boolean;
  credits: number | null;
  remainingCredits: number | null;
  creditsExpireAt: number | null;
  /** 逐包明细；未刷新过积分时为 `null`。 */
  creditPackages: TraeCreditPackage[] | null;
  deviceIdMasked: string | null;
  cooldownType: string | null;
  cooldownUntil: number | null;
  cooldownReason: string | null;
  /**
   * 这条冷却判定**写入的时刻**（Unix **秒**）；旧数据为 `null`。
   *
   * issue #108 §P1-A：只给 `cooldownReason` 原文会让用户把「11 小时前失败过一次」
   * 读成「刚才又失败了」，于是反复重登、反复重启。
   */
  cooldownSince: number | null;
  hasRefreshToken: boolean;
  jwtAutoRefresh: boolean;
  addedAt: string | null;
  updatedAt: string | null;
  /**
   * 用户自填备注（例如「DS4.1 额度 · 10/03 解禁」）；未设置时 `null`。
   *
   * 与 WorkBuddy 账号卡片的 `remark` 同义：**就地编辑**，空串即清空。
   * ⚠️ 它会被**直接当 React 子节点**渲染，所以卡片侧仍过一遍 `displayText`
   * （后端是我们自己的账号库，正常不会出现脏值；但账号库是用户可见的文件，
   * 手改出一段 JSON 对象就能让整棵树崩掉 —— 见 `display-text.ts` 的 issue #2）。
   */
  remark: string | null;
}

/** 分组视图（含成员数）。 */
export interface TraeGroup {
  id: string;
  name: string;
  color: string;
  order: number;
  count: number;
}

/** 账号页聚合数据（`get_trae_accounts`）。 */
export interface TraeAccountsOverview {
  accounts: TraeAccount[];
  groups: TraeGroup[];
  total: number;
  cooling: number;
  ungrouped: number;
}

/**
 * 旧「产品线」账号库并入区域账号库的合并报告（`trae_merge_legacy_regions`）。
 *
 * 与 Rust 侧 `region_migrate::MergeReport::to_json()` 逐字对应。
 * `changed: false` 表示**没有发生改写**（无旧库 / 已并完）—— 幂等判据。
 */
export interface TraeLegacyMergeReport {
  /** 旧库里的账号总数。 */
  legacyAccounts: number;
  /** 本次并入国内版账号库的账号数。 */
  accountsAdded: number;
  /** 因国内版库已有同 uid 而**保留现有值**的账号数。 */
  accountsKept: number;
  /** 并入的设备绑定数。 */
  bindingsAdded: number;
  /** 并入的分组数（按名字去重）。 */
  groupsAdded: number;
  /** 并入的成员关系数。 */
  membershipAdded: number;
  /** 备份目录；`null` = 无需备份（未发生改写）或备份失败（见 `backupFailed`）。 */
  backup: string | null;
  /** 备份是否尝试过但失败 —— 与「无需备份」区分开。 */
  backupFailed: boolean;
  /** 本次是否真的改写了数据（`false` = 幂等空转）。 */
  changed: boolean;
  /**
   * 客户端**合并**（issue #20）后的登录态快照搬运报告。
   *
   * ## 为什么它挂在这条命令上
   *
   * 快照搬运与账号库合并同属「一次性遗留迁移」，因此共用同一个入口
   * （账号页挂载时调一次，幂等）。⚠️ 两者的 `changed` **互相独立** ——
   * 账号库早就并完了、但快照还没搬的机器上是「DB 空转 + 快照有变动」，
   * 调用方必须两个都看（见 `TraeAccountsPage` 的挂载 effect）。
   */
  snapshots: TraeSnapshotCarryReport;
}

/**
 * 登录态快照搬运报告（`trae_merge_legacy_regions().snapshots`）。
 *
 * 客户端把 TraeWork 与 TraeCode 合并成一个客户端后，用户在**合并前**保存的
 * 登录态快照落在遗留程序位的槽位里，合并后的客户端槽位是空的 ⇒
 * 「切换账号」会报「该账号的登录态快照不存在」。本报告描述那次**补齐搬运**。
 */
export interface TraeSnapshotCarryReport {
  /** 实际搬运的账号槽位数。 */
  slotsCopied: number;
  /** 因目标槽位已有同 uid 而**跳过**（只增不改）的账号数。 */
  slotsKept: number;
  /** 复制的文件总数。 */
  filesCopied: number;
  /** 逐条搬运明细。 */
  carried: {
    region: string;
    /** 来源**历史池目录名**（`profiles` / `profiles_trae_cn` / `profiles_global*`）。 */
    from: string;
    /** 账号槽位名（userId）。 */
    slot: string;
    files: number;
  }[];
  /** 未能搬运的槽位及原因（非空即「有东西没搬成」）。 */
  errors: string[];
  /** 诊断备注（为什么没搬），仅供排查。 */
  notes: string[];
  /** 本次是否真的搬了东西（`false` = 幂等空转）。 */
  changed: boolean;
  /**
   * 本次是**首次**执行收敛迁移（后端落的一次性标记）。
   *
   * ★ 它**不是** `changed`：`changed` 只在真搬了东西时为真，而全新安装没有存量快照
   * ⇒ `changed === false` 但用户**仍然需要**知道「Trae 已合并、旧程序位已退役」。
   * 前端的一次性升级引导只认这个字段。
   */
  firstRun: boolean;
}

/** 单账号签到结果。 */
export interface TraeCheckinOutcome {
  name: string;
  userId: string;
  ok: boolean;
  code: number | null;
  message: string;
  action: string;
  credits: number | null;
  delta: number;
  errorType: string | null;
  cooldownUntil: number | null;
}

/** 签到摘要。 */
export interface TraeCheckinSummary {
  time: string | null;
  results: TraeCheckinOutcome[];
  totalOk: number;
  already: number;
  failed: number;
  warnings: string[];
}

/** 冷却条目。 */
export interface TraeCooldown {
  userId: string;
  type: string;
  until: number;
  reason: string;
  permanent: boolean;
}

/** 签到状态（`get_trae_checkin_status`）。 */
export interface TraeCheckinStatus {
  summary: TraeCheckinSummary;
  summaryIsToday: boolean;
  cooldowns: TraeCooldown[];
  cooldownCount: number;
  logFile: string;
}

/** 一条签到积分明细。 */
export interface TraeCreditRecord {
  date: string;
  userId: string;
  credits: number;
  delta: number;
}

/** 每日积分快照。 */
export interface TraeDailySnapshot {
  date: string;
  total: number;
  earned: number;
  consumed: number;
}

/** 积分总览（`get_trae_credits`）。 */
export interface TraeCreditsOverview {
  remaining: Record<string, number>;
  expireTimes: Record<string, number>;
  /** 逐包明细：`{ "<userId>": [CreditPackage…] }`（账号卡进度条的数据源）。 */
  packages: Record<string, TraeCreditPackage[]>;
  updatedAt: string | null;
  balances: { userId: string; credits: number; date: string }[];
  records: TraeCreditRecord[];
  daily: TraeDailySnapshot[];
  todayEarned: number;
  historyDays: number;
  /** 平台做不到的维度（置灰说明），形状见 {@link TraeUnsupported}。 */
  unsupported: TraeUnsupported[];
}

/** 登录态快照信息。 */
export interface TraeProfileInfo {
  slot: string;
  sizeBytes: number;
  fileCount: number;
  lastModified: string;
  sizeText: string;
}

/** 快照总览（`get_trae_profiles`）。 */
export interface TraeProfilesOverview {
  profiles: TraeProfileInfo[];
  /**
   * 当前登录账号的**身份**（uid）—— 卡片上「是不是当前账号」的相等比较用它。
   *
   * ⚠️ 不要拿它直接当**展示文本**（那是一串 16 位数字）；给人看的是
   * {@link currentAccountName}，后者查不到时由界面回落回本字段。
   */
  currentAccount: string | null;
  /**
   * 当前登录账号的**展示名**（账号库里的 `name`）。
   *
   * `null` = 账号库里没有这个 uid（用户刚在客户端登录、尚未采集）—— 这是**正常状态**，
   * 不是错误：此时界面回落显示 uid。键**始终存在**，`null` 与「键缺失」是两件事。
   */
  currentAccountName: string | null;
  dataDir: string | null;
  clientRunning: boolean;
  coreEntryCount: number;
}

/** Trae 模块设置。 */
export interface TraeSettings {
  proxyPort: number;
  theme: string;
  launchMinimized: boolean;
  autoStartProxy: boolean;
  tray: boolean;
  language: string;
  checkinSkipChecked: boolean;
  checkinSkipExpired: boolean;
  retry: number;
  notify: string;
  /**
   * 用户指定的 Trae 客户端可执行文件路径，**按区域分家**（issue #20）。
   *
   * 键 = {@link TraeRegionId}（`"cn"` / `"global"`），值 = exe 绝对路径；缺失 = 该区域未指定。
   *
   * ## 为什么不再是单个 `traePath`（issue #20）
   *
   * 国内版与国际版是**两个物理上不同的客户端**（`Trae CN.exe` / `Trae.exe`），
   * 单个全局字段会让「设置国际版路径」直接覆盖国内版 —— 用户报障原话：
   * 「设置Trae 路径国内版和国际版 两个都设置是一样的，以最后的设置为准了，没有隔离开来」。
   */
  traePaths: Partial<Record<TraeRegionId, string>>;
  /**
   * **旧版单值字段：只读回显，不要写。**
   *
   * 后端 `load()` 会把它单向迁移进 {@link traePaths} 并清空；前端保留它只为
   * 「升级前设过路径」的诊断提示，**保存时不得回写**（写了会让迁移反复发生）。
   */
  traePath: string | null;
  browserPath: string | null;
  logRetentionDays: number;
  proxyDomains: string;
  apiPort: number;
  apiKey: string;
  apiDefaultModel: string;
}

/** 签到报告（`trae_checkin`）。 */
export interface TraeCheckinReport {
  total: number;
  totalOk: number;
  already: number;
  failed: number;
  warnings: string[];
  results: TraeCheckinOutcome[];
}

/** 切换过程中的一步。 */
export interface TraeSwitchStep {
  stage: string;
  status: "ok" | "skip" | "fail";
  message: string;
  time: string;
}

/** 切换结果。 */
export interface TraeSwitchOutcome {
  success: boolean;
  steps: TraeSwitchStep[];
  error: string | null;
}

/** 设备标识重置报告。 */
export interface TraeDeviceResetReport {
  resetCount: number;
  machineId: string;
  guid: string;
  totalLayers: number;
  steps: {
    layer: number;
    label: string;
    status: "ok" | "skip" | "unsupported";
    reason?: string;
    removed?: number;
    cleared?: number;
  }[];
}

// ---------------------------------------------------------------------------
// OAuth 登录（浏览器授权 + 本地回调监听）
// ---------------------------------------------------------------------------
//
// 与 WorkBuddy 的 `OAuthStartResult` / `OAuthPollResult`（`lib/types.ts`）刻意同构：
// 前端交互骨架完全一致，差异只在「多一个 `port` 字段」——Trae 是本机自建回调监听，
// 把端口回传出来便于用户排障（例如防火墙弹窗时能看到到底占了哪个口）。
// **不要为了「统一」把 `port` 去掉**：它是排障时唯一的抓手。

/** 发起登录的结果：前端拿 `verificationUri` 去开浏览器。 */
export interface TraeOAuthStartResult {
  loginId: string;
  /**
   * 授权页 URL。**域按区域分家**（不是同一个页面换参数）：
   * 国内版 `https://www.trae.cn/authorization?…`，
   * 国际版 `https://www.trae.ai/authorization?…`。
   * 后端按变体从端点表派生（`EndpointSet.console_base`），前端**不要**自己拼。
   */
  verificationUri: string;
  /** 会话有效期（秒）。 */
  expiresIn: number;
  /** 本机回调监听端口（`127.0.0.1:<port>/authorize`）。 */
  port: number;
}

/** 轮询结果：`done` 之后二选一（`account` 或 `error`）。 */
export interface TraeOAuthPollResult {
  done: boolean;
  /** 成功时的账号视图（形状同 {@link TraeAccount}）。 */
  account?: TraeAccount;
  /** 成功时的最新全量账号视图，省掉前端再拉一次列表。 */
  accounts?: TraeAccount[];
  error?: string;
  port?: number;
}

/** 签到进度事件（Tauri 事件 `trae-checkin-progress`）。 */
export type TraeCheckinEvent =
  | { type: "start"; total: number }
  | {
      type: "account";
      index: number;
      userId: string;
      name: string;
      status: "success" | "already" | "fail";
      code?: number;
      message?: string;
      credits?: number | null;
      delta?: number | null;
      errorType?: string | null;
      cooldownUntil?: number | null;
    }
  | { type: "done"; ok: number; already: number; failed: number; total: number };

// ---------------------------------------------------------------------------
// API 网关（OpenAI 兼容）——与 WorkBuddy 网关平行的第二套
// ---------------------------------------------------------------------------
//
// 字段名与 Rust `buddy_switch_gateway::trae` 的序列化输出逐字一致：
// - `TraeGatewayConfig` 为 snake_case（与 WorkBuddy 网关同约定）；
// - `TraeGatewayStatus` 亦为 snake_case（`TraeGatewayStatusView`），
//   由 `normalizeTraeGatewayStatus` 归一为下面这个 camelCase 前端形状。

/** 网关运行配置（落盘 `~/.buddy-switch/trae/api_gateway.json`）。 */
export interface TraeGatewayConfigRaw {
  enabled: boolean;
  bind_addr: string;
  port: number;
  allow_non_loopback: boolean;
  log_keep: number;
  log_bodies: boolean;
  max_body_mb: number;
  default_model: string;
  max_rotate: number;
  /** 「指定账号」：优先消耗该 uid 的积分；空串 = 不指定（自动择优）。 */
  preferred_uid: string;
  /** 会话粘性绑定存活时长（毫秒；`<= 0` 由后端归一为 30 分钟）。 */
  sticky_ttl_ms: number;
  /**
   * 对话转发抓取开关（诊断旁路，默认 `false`）。
   *
   * ★ 界面上**没有**这个开关，只能手改 `api_gateway.json` + 重启网关。
   * 但它必须出现在读写两端：配置保存是「整份覆盖」，缺键就回落 `false` ——
   * 漏带这一格会让用户手开的抓取在任意一次「保存」后被**静默关掉**。
   */
  capture_enabled: boolean;
  /** Max Mode：对「目录里 max > dev」的模型注入 is_max_mode（默认 true）。 */
  max_mode: boolean;
}

/** 归一化后的网关配置（前端统一用 camelCase）。 */
export interface TraeGatewayConfig {
  enabled: boolean;
  bindAddr: string;
  port: number;
  allowNonLoopback: boolean;
  logKeep: number;
  logBodies: boolean;
  maxBodyMb: number;
  defaultModel: string;
  maxRotate: number;
  /** 「指定账号」：优先消耗该 uid 的积分；空串 = 不指定（自动择优）。 */
  preferredUid: string;
  /** 会话粘性绑定存活时长（毫秒）。 */
  stickyTtlMs: number;
  /** 对话转发抓取开关（诊断旁路；无界面开关，只在配置里往返，默认 `false`）。 */
  captureEnabled: boolean;
  /** Max Mode：按官方目录逐模型注入 `is_max_mode`（默认开启）。 */
  maxMode: boolean;
}

/** 账号池摘要（`pool` 字段）。 */
/**
 * 池五态计数 —— **与 WorkBuddy 侧逐字同形**（管理面共用 `AccountPoolCard`）。
 *
 * 因此这里不重新声明字段：改形状必须改 `types.ts` 里那一份，两侧一起变。
 */
export type TraeGatewayPoolSummary = GatewayPoolSummary;

/** 池内单个账号的可路由状态（在共用字段之外补 Trae 专有两项）。 */
export interface TraeGatewayAccountStatus extends GatewayPoolAccount {
  creditsExpireAt: number | null;
  deviceIdMasked: string | null;
}

/** 网关运行状态（`trae_gateway_status` 归一化后）。 */
export interface TraeGatewayStatus {
  enabled: boolean;
  running: boolean;
  addr: string | null;
  baseUrl: string;
  bindAddr: string;
  port: number;
  allowNonLoopback: boolean;
  version: string;
  totalRequests: number;
  lastError: string | null;
  /** API Key 脱敏展示（`sk-trae-0123…cdef`），**不含**可用明文。 */
  apiKeyPrefix: string;
  pool: TraeGatewayPoolSummary;
  accounts: TraeGatewayAccountStatus[];
  /** 逐账号「为什么不能路由」的可读串。 */
  diagnose: string[];
  /** 上游主机（`https://trae-api-cn.mchost.guru`）。 */
  upstream: string;
}

/** 网关请求日志（仅元数据；默认不记录正文）。 */
export interface TraeGatewayLogEntry {
  ts: number;
  endpoint: string;
  method: string;
  account: string | null;
  model: string | null;
  status: number;
  latencyMs: number;
  promptTokens?: number | null;
  completionTokens?: number | null;
  stream: boolean;
  error?: string | null;  /** ★ issue #76：调用方 Key id（`null` = 无 Key；`undefined` = 升级前老条目）。 */
  keyId?: string | null;
  /** ★ `keyId` 的归属台账（`"trae"` / `"unified"` / `null`）。 */
  keyScope?: string | null;
}

/** 对外暴露的模型条目（`get_trae_gateway_models`）。 */
export interface TraeGatewayModel {
  id: string;
  object: string;
  created: number;
  owned_by: string;
}

/**
 * 客户端（**上游下发**）的模型清单（`get_trae_client_models`）。
 *
 * 数据源是 Trae 客户端 `state.vscdb` 里上游下发的清单缓存，因此**随客户端刷新而变**。
 * 读不到时 `source === "missing"`、`groups` 为空数组、`note` 给出可读原因。
 *
 * 字段一律 camelCase（后端已 `rename_all = "camelCase"`）；且**所有字段都已被后端
 * 归一**（脏值不会传到这里）—— 展示层不必再判类型。
 */
export interface TraeClientModelList {
  /** 变体标识（`trae_work` / `trae_cn` / `global` / `global_trae_code`）。 */
  variant: TraeVariantId;
  /** 变体展示名（`Trae Work` / `Trae` …）。 */
  variantLabel: string;
  /** `"client-cache"`（读到了）或 `"missing"`（没读到）。 */
  source: "client-cache" | "missing" | string;
  /** 本次读取时间（毫秒时间戳）。 */
  readAt: number;
  /** 读到的 `state.vscdb` 所在 userData 目录。 */
  dataDir: string | null;
  /** 命中的缓存键里的 uid。 */
  uid: string | null;
  /** 按 function 分组（`solo_work_lite` / `solo_coder` …；取值按产品线不同）。 */
  groups: TraeClientModelGroup[];
  /** 读不到的原因，或「客户端里存有多份缓存」这类提示。 */
  note: string | null;
}

/** 客户端模型清单的一个 function 分组。 */
export interface TraeClientModelGroup {
  function: string;
  models: TraeClientModel[];
}

/**
 * 模型清单卡的**一个数据源**：本区域内某条**程序位**的客户端清单。
 *
 * ## 为什么清单要按程序位分开（issue #4 的现场）
 *
 * 模型清单是**客户端级**的：Trae Work 与 TraeCode 是两个客户端、两份
 * `state.vscdb`，上游下发给它们的 function 分组与模型集合**完全不同**
 * （实测 Trae Work = `solo_work_lite` / `solo_coder` / `agent` …，
 * TraeCode = `builder` / `chat_v3` / `solo_agent` …）。
 * TraeCode 专有的模型（如 `glm-5.3-flash`）**只存在于 TraeCode 那份缓存里** ——
 * 只读区域主程序（TraeWork）时它永远不会出现，这正是用户报障
 * 「traecode 专有的 glm-5.3-flash 没有加载出来」的原因。
 *
 * `variant` 是**回传给后端的程序位标识**（`trae_work` / `trae_cn` /
 * `global` / `global_trae_code`），不是区域标识 —— 传区域只会拿到主程序那份。
 */
export interface TraeModelSource {
  /** 程序位标识（直接喂给 `get_trae_client_models`）。 */
  variant: TraeVariantId;
  /**
   * 程序位展示名。规则 = **国内版带 ` CN` 后缀、国际版不带后缀**：
   * `TraeWork CN` / `TraeCode CN` / `Trae CN`（国内）· `TraeWork` / `TraeCode` / `Trae`（国际）。
   */
  label: string;
  /** 该程序位的客户端清单；未取到（后端不可用）为 `null`。 */
  data: TraeClientModelList | null;
  /**
   * 该程序位**网关对外清单**的逐模型上下文（`/v1/models` 的 `context_length`）。
   * 数字来自官方模型目录，逐模型不同（不是统一 1M）；未取到为 `null`。
   */
  gatewayContexts?: Record<string, { dev: number | null; max: number | null }> | null;
  /**
   * 该程序位下**网关对外清单**的模型 id（`get_trae_gateway_models(variant)`）。
   *
   * 与 `data` 口径不同：`data` 是该客户端的**全部**分组（同一模型会在多个分组里重复出现），
   * 这里只列该程序位 `function` **真能调**的那批 —— 也就是**归属该程序位的 API Key**
   * 连本网关时，外部客户端在 `/v1/models` 实际看到的清单。`null` = 没取到。
   *
   * 两个用途：
   * 1. 条数 = `gatewayNames.length`（卡片上的「网关对外 N 个」）；
   * 2. **逐个标记**：客户端清单里有、但这里没有的模型，就是「看得见、调不动」的那批
   *    （实测：TraeWork 全分组 27 条里 11 条不属于 `solo_work_lite`，一律 `4001`）。
   *    卡片据此给它们打「网关不提供」标记，把同一个坑一次堵死。
   *
   * ⚠️ 必须**按程序位各取一次**：`/v1/models` 只列该程序位 `function` 能调的模型
   * （见 Rust 侧 `payload::models_response_for`），两条程序位的清单**不一样**。
   */
  gatewayNames: string[] | null;
}

/** 客户端模型清单里的单个模型（字段已由后端归一）。 */
export interface TraeClientModel {
  /** 模型标识（上游 `name`，如 `deepseek-v4.1-flash`）。 */
  name: string;
  /** 展示名（上游 `display_name`）；取不到时后端已回落成 `name`。 */
  displayName: string;
  /** `reasoning_model` / `chat_model` …；取不到为空串。 */
  modelType: string;
  multimodal: boolean;
  isDefault: boolean;
  isPreset: boolean;
  isNew: boolean;
  isBeta: boolean;
  /** 默认上下文窗口；取不到为 `null`。 */
  contextWindow: number | null;
  /** 上限上下文窗口（`context_window_size.max`，Max Mode 口径）；取不到为 `null`。 */
  contextWindowMax?: number | null;
  /** 单次回复上限；取不到为 `null`。 */
  promptMaxTokens: number | null;
  /**
   * 积分倍率（`features.consumption_rate.data.rate`）；没下发为 `null`。
   *
   * 这是**倍率**不是金额：`0.2` = 基准价的 0.2 倍。实测取值 0.06～1.83。
   */
  creditsRate?: number | null;
  /** 支持的思考档位（`reasoning_effort_config.options`，上游原词表，保序）。 */
  efforts?: string[];
  /** 默认档位（`reasoning_effort_config.default_level`）。 */
  defaultEffort?: string | null;
  /**
   * 上游对「思考档位」的声明状态 —— 空 `efforts` 的两种成因必须分开：
   * `"unsupported"` = 该模型确实不能选档；`"unknown"` = 这条**程序位没下发**。
   *
   * TraeWork（`solo_work_lite`）实测 22 条里只有 1 条带档位 ⇒ 该列大面积是「未下发」，
   * 不是「都不支持思考」。
   */
  thinking?: "supported" | "unsupported" | "unknown";
  /**
   * 第三方 / 自定义路由条目（上游 `provider` 非空或带 `custom_model_id`）。
   *
   * 网关发的是 `config_name` / `model_name` 这套字段，对它们**无效**（实测
   * `openrouter//stealth/ox-alpha` 一律 `4001`）⇒ 网关对外清单会剔除，
   * 界面对比「服务组」时必须同样剔除，否则两边名字集合永远对不上。
   */
  isBypass?: boolean;
}

/**
 * 单条 Trae API Key（`list_trae_api_keys` 的 `keys[]`）。
 *
 * **不含 hash 与明文**：服务端只下发脱敏白名单（`prefix` / `suffix` / `name` / `variant`…），
 * 明文仅在创建时一次性返回。`suffix` 是明文的**末 4 位**，与 `prefix`（前 4 位）一样
 * 不足以还原 32 位 secret —— 它的用途是让用户在列表里**认出是哪一把**。
 *
 * `variant` 是该 Key 的**归属区域**（`"cn"` / `"global"`）—— 账号池按区域建，
 * 因此它决定这把 Key 能取到哪个区域的账号。**新 Key 一律落到区域的线上标识**；
 * 历史 Key 存的可能是 `"trae_work"` / `"trae_cn"`（改造前的产品线标识），
 * 二者都属国内区域，故展示与分档一律按 `variant.region()` 的语义处理，
 * 不要在前端自行比较字符串是否相等。
 */
export interface TraeApiKeyRecord {
  id: string;
  name: string;
  /** 归属区域（`"cn"` / `"global"`；历史记录可能是 `"trae_work"` / `"trae_cn"`）。 */
  variant: TraeVariantId;
  prefix: string;
  /**
   * ★ 尾部 4 位（如 `"9f3c"`）。列表显示 `prefix••••••••suffix`。
   *
   * 与 WorkBuddy 侧 `ApiKeyRecord.suffix` **同形**（E3）：只靠前缀，同时建两把 Key 时
   * 用户分不清哪把是哪把。旧台账 / 短 legacy Key 无此值 ⇒ 空串 ⇒ 展示层**省略尾部**
   * （不报错、不补造）。
   */
  suffix?: string;
  createdAt: number;
  revokedAt: number | null;
  revoked: boolean;
  /* ── 策略（issue #76）—— 与 `ApiKeyRecord` 同形同义，前端共用一套构件。
     全部可选 ⇒ 旧台账零迁移；调用方一律先过 `policyOf` 补默认值。 ── */
  quotaMode?: "unlimited" | "limited";
  quotaLimit?: number;
  quotaUnit?: "token" | "request";
  quotaPeriod?: "none" | "daily" | "weekly" | "monthly";
  scopeMode?: "all" | "selected";
  scopeAccounts?: string[];
  scopeGroups?: string[];
  lastUsedAt: number | null;
}

/**
 * 新建 Key 的返回（`create_trae_api_key`）。
 *
 * `key` 为**一次性明文**——只会出现在本次响应里，之后无从取回；界面必须提示用户立即复制。
 */
export interface TraeApiKeyCreated {
  ok: boolean;
  key?: string;
  record?: TraeApiKeyRecord;
}

// ---------------------------------------------------------------------------
// Token 统计（聚合本机网关请求日志）
// ---------------------------------------------------------------------------

/** 一个统计桶（summary 无 `key`，分组项有）。 */
export interface TraeTokenBucket {
  key?: string;
  /** 输入 + 输出。 */
  total: number;
  input: number;
  output: number;
  records: number;
  errors: number;
  streamRequests: number;
  avgLatencyMs: number;
  p95LatencyMs: number;
}

/** 账号维度的桶（带回表得到的显示名）。 */
export interface TraeTokenAccountBucket extends TraeTokenBucket {
  key: string;
  name: string;
  shortId: string;
}

/**
 * Token 统计的**区域范围**筛选维度（`get_trae_token_statistics` 的 `scope`）。
 *
 * **不是**第三种区域，而是「查询范围」——多了「未标注」（升级前的旧日志没有
 * `variant` 键）与「全部」两档。切勿并进 `TraeRegionId`。
 *
 * ⚠️ 档名在 2026-09-21 由**产品线**改为**区域**：国内 / 国际是两套互不相通的账号体系，
 * 而「Trae Work / Trae CN」只是国内区域下的两条程序。后端对历史标识
 * （`work` / `trae_work` / `trae_cn`）一律按**国内版**归集，因此老链接不会串档。
 */
export type TraeTokenScope = "cn" | "global" | "unlabeled" | "all";

/**
 * **区域**范围条各档计数（`variantCounts`）。
 *
 * 只受**时间窗口**影响、不受当前 `scope` 影响：范围条要能显示「切到哪一档有多少条」。
 */
export interface TraeVariantCounts {
  /** 国内版。历史日志里的 `trae_work` 与 `trae_cn` **都计入此档**（同属国内区域）。 */
  cn: number;
  /** 国际版（另一套账号体系）。 */
  global: number;
  unlabeled: number;
  all: number;
}

/** 按天 × 模型的用量点（`modelDaily`，堆叠柱数据源）。 */
export interface TraeModelDailyPoint {
  date: string;
  model: string;
  total: number;
  input: number;
  output: number;
  records: number;
}

/**
 * Trae Token 统计（`get_trae_token_statistics`）。
 *
 * **边界**：数据源只有本机网关的请求日志，因此
 * 1. 只统计经过网关的调用，直接在 IDE 里对话不计入；
 * 2. 网关未启用或日志被清空时全为 0；
 * 3. 中途断流的请求没有 `token_usage`，只体现在 `records` 里。
 */
export interface TraeTokenStatistics {
  source: string;
  label: string;
  generatedAt: number;
  rangeDays: number | null;
  logFile: string;
  summary: TraeTokenBucket;
  models: TraeTokenBucket[];
  accounts: TraeTokenAccountBucket[];
  daily: TraeTokenBucket[];
  hours: TraeTokenBucket[];
  statuses: { key: string; records: number }[];
  /** 按天 × 模型的用量点（堆叠柱）。 */
  modelDaily: TraeModelDailyPoint[];
  /** 变体范围条各档计数。 */
  variantCounts: TraeVariantCounts;
  /** 平台做不到的维度（置灰卡），形状见 {@link TraeUnsupported}。 */
  unsupported: TraeUnsupported[];
  filesScanned: number;
  parseErrors: number;
  coverageStartAt: number | null;
  coverageEndAt: number | null;
  note: string;
}

// ---------------------------------------------------------------------------
// 运行日志（系统日志页的「运行日志」标签页）
// ---------------------------------------------------------------------------

/** 日志类型：应用级 / 签到 / 登录态切换。 */
export type TraeLogKind = "app" | "checkin" | "switch";

/** 一条运行日志。 */
export interface TraeLogEntry {
  kind: TraeLogKind;
  /** `YYYY-MM-DD HH:MM:SS`；无日期前缀的行为空串。 */
  time: string;
  /** `YYYY-MM-DD`；无前缀的行为空串。 */
  date: string;
  message: string;
}

/** 日志文件状态（用于在页面上标出「文件还不存在」）。 */
export interface TraeLogSource {
  kind: string;
  label: string;
  path: string;
  exists: boolean;
}

/**
 * 运行日志查询参数。
 *
 * `variant` 由 [`getTraeLogs`] 可选注入（与 `kind` / `date` / `keyword` 同层），
 * 缺省时后端按默认产品线读，老调用点行为不变。
 */
export interface TraeLogQuery {
  kind?: string;
  date?: string;
  keyword?: string;
  limit?: number;
  variant?: TraeVariantId;
}

/**
 * 运行日志响应（`get_trae_logs`）。
 *
 * **边界**：只读本机 `logs/` 下的纯文本日志（app / checkin / switcher）。
 * 网关的请求日志是另一份数据（JSON、面向统计），在「网关请求日志」标签页。
 */
export interface TraeLogsResponse {
  entries: TraeLogEntry[];
  /** 过滤后的总条数（可能大于 `entries.length`，因为响应被 `limit` 截断）。 */
  total: number;
  limit: number;
  /** 可选日期，倒序。 */
  dates: string[];
  counts: { all: number; app: number; checkin: number; switch: number };
  sources: TraeLogSource[];
  logDir: string;
  note: string;
}

// ---------------------------------------------------------------------------
// 账号迁移（导出 / 导入）
// ---------------------------------------------------------------------------

/**
 * 导出文件中的一条账号记录。
 *
 * **键名与 Trae 参考实现（`checkin_accounts.json`）一致**，含非常规大写 `UserID`：
 * 本工具既是消费者也是生产者，导出文件要能被参考实现 `device_proxy.py` 读回。
 * 因此这里不是 camelCase —— 与页面内数据形状（`TraeAccount`）刻意不同。
 */
export interface TraeExportRecord {
  UserID?: string;
  name?: string;
  refresh_token?: string;
  jwt?: string;
  added_at?: number;
  [key: string]: unknown;
}

/** 导入文件账号的脱敏预览：**不含 JWT / refresh_token 明文**，只报有无。 */
export interface TraeImportPreviewAccount {
  index: number;
  userId: string | null;
  name: string | null;
  hasJwt: boolean;
  hasRefreshToken: boolean;
}

/** 导入预览响应。 */
export interface TraeImportPreview {
  accounts: TraeImportPreviewAccount[];
  total: number;
}

