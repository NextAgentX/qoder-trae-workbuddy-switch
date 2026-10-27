/**
 * 平台注册表（PLAN §2.3 / §3.3）—— **P0 产出**。
 *
 * ## 为什么要有这一层
 *
 * 现状：两个平台（WorkBuddy / Trae）的差异散落在 `App.tsx`（侧栏 5:5、路由 5:5）与
 * 各页面的 `if (product === ...)` 判断里。新增一个平台要改十几处，**且必然漏改**。
 *
 * 目标：**平台的差异只在一处声明**，页面组件从注册表读，不写平台判断。
 *
 * ## P0 阶段的范围（**纯新增，不改任何现有组件**）
 *
 * P0 只声明**平台元数据与能力**，骨架一律不动 —— 这样 P0 的产物「删掉即回滚」。
 * 当时 `PRODUCT_NAV` / `PRODUCT_LABEL_KEY` / `PRODUCT_HOME` 与 `type Product`
 * 都**留在 `App.tsx`**，本文件只承诺 P2 收回来。
 *
 * ## P2 之后（**收敛已完成**）
 *
 * 四项重复**都已消除**，本文件是它们的**唯一来源**：
 * - `Product` 类型：`App.tsx` 改为 `import`（不再有局部定义）；
 * - 侧栏导航：由 `DESTINATIONS` + `DEST_META` 生成（`PRODUCT_NAV` 已删）；
 * - 产品显示名：`PlatformDescriptor.labelKey`；
 * - 产品首页：`PlatformDescriptor.home`。
 *
 * ⚠️ **过渡期的重复是有期限的，不是设计** —— 逐项状态见本文件末尾的「P2 收敛清单」。
 */

/** 平台标识。**刻意保持 `trae` 而非 `traework`**：路由前缀 `/trae/...` 与既有键名都依赖它。 */
export type Product = "workbuddy" | "trae" | "qoder";

/**
 * 平台能力（PLAN §2.7）—— **平台固有，用户不可改**。
 *
 * ⚠️ 与「用户偏好」是两个正交维度：
 * - 能力（本枚举）：平台**有没有**这个功能。用户改不了，后端命令分发层校验（§2.5 第 2 条）。
 * - 偏好（`preferences.*`）：用户**要不要看**这个入口。纯前端，**不影响数据**。
 *
 * 典型混淆：用户关掉「CodeBuddy CLI」入口 ≠ 该能力消失 —— 走 URL 或直接调命令仍应工作。
 *
 * ⚠️ **与后端 `crates/buddy-switch-core/src/modules/capability.rs` 的 `Capability` 逐字对齐**
 * （本联合类型的 7 个取值 = 后端枚举的 7 个 `as_str`）。两侧漂移由 core 的跨语言护栏测试
 * `product_spec_matches_frontend_registry_slug_and_capabilities` 兜住 —— 改这里就要同步改后端，
 * 否则 `cargo test -p buddy-switch-core` 变红。
 */
import type { TranslationKey } from "@/locales/zh";

export type Capability =
  | "checkin" // 签到
  | "travel" // 自动旅行（**WorkBuddy 独有**）
  | "credits" // 积分 / 资源包
  | "oauth" // 扫码登录
  | "gateway" // API 网关
  | "autoRotate" // 自动轮换（**WorkBuddy 独有**）
  | "sessionCopy"; // 会话复制（**只有 WorkBuddy 桌面**，见下）

/**
 * 单个平台的描述符。
 *
 * ⚠️ **`labelKey` 存文案键而不是成品文案**：本表在模块加载时就定型，
 * 写中文会让语言切换对它失效（同 `region.ts` 的 `RegionDescriptor` 的处理）。
 */
/**
 * **程序位**（客户端内的"可切换目标"）—— 前端侧的表。
 *
 * ## 与后端 `programs.rs` 的关系
 *
 * 后端 `PROGRAMS`（`crates/buddy-switch-core/src/modules/programs.rs`）是**执行侧的唯一事实来源**，
 * 本表是**展示侧的镜像**（`label_key` 与后端逐字一致，门禁核对）。
 *
 * ## ★ 为什么分 `available` 与 `planned`
 *
 * - **`available`**：本仓**已支持**（后端有 `ProgramAdapter` + 宿主命令）。
 * - **`planned`**：**计划新增**的目标 —— 后端模块清单**无**对应模块 ⇒ **当前不支持**。
 *
 * ## ★ 2026-10-09：`planned` 已清空（两条目标全部落地为 `available`）
 *
 * `PLAN.md` §3.3 B 表的两条「待新增」（**VS Code CodeBuddy 插件 / JetBrains IDE 插件**）
 * 已实现：后端 `modules/vscode_ext.rs` / `modules/jetbrains.rs` 提供
 * `status` / `switch` / `detect`，并在两张 `PROGRAMS` 表与两端宿主路由里登记
 * （`tencent-cloud.coding-copilot` 扩展写 `state.vscdb`；JetBrains 写
 * `options/secret-storage.xml`）。⇒ 本表**不再有 `planned` 条目**。
 *
 * ⚠️ **机制保留、列表为空**：`status: "planned"` 与 `PLANNED_PROGRAMS` 仍是「尚未支持」
 * 的**唯一表达方式**（卡片有对应灰显样式与「尚未支持」措辞），留给下一次新增 ——
 * 但没有条目时**不要**为了凑断言硬造一条（那会编造出不存在的「计划」）。
 *
 * ⚠️ **`planned` 的说明必须是「尚未支持」，不许用「未检测到客户端」** ——
 * 后者会**引导用户去装，而装了也没用**（原型断言 `verify-behaviors.cjs:1297-1302`
 * 明确区分这两者，并**反向断言**禁止误用后者的措辞）。
 */
export interface ProgramSpec {
  readonly id: string;
  /** 展示名文案键（与后端 `ProgramSpec.label_key` 逐字一致）。 */
  readonly labelKey: TranslationKey;
  /** `available` = 已支持；`planned` = 计划新增（**尚未支持**）。 */
  readonly status: "available" | "planned";
}

/**
 * WorkBuddy 侧的程序位表（**顺序即 UI 展示顺序**）。
 *
 * ⚠️ **只保证「集合与后端一致」，不保证顺序**（安全审计「流程 20」纠正了这条注释）：
 * 改造前这里写着「顺序与后端 `programs.rs::PROGRAMS` 逐字对齐」，而事实是**前两项互换**
 * （后端 `codebuddy_cli` 在前、这里 `codebuddy_cn_ide` 在前），门禁也只校集合成员、
 * 从不比较顺序 —— 一句「已保证」的假话比没写更容易误导人（后来人会「按文档修正 UI 顺序」）。
 *
 * 顺序是**产品决策**（这一版的视觉次序），后端表是**派发顺序**，两者没有必须相同的理由；
 * 真要锁定顺序，应当在门禁里逐位比较，而不是靠注释承诺。
 *
 * ⚠️ **显示名两两不同**：`vscode_cn_inject`（只读探测槽，名「VS Code CodeBuddy 插件」）
 * 与 `vscode_codebuddy_plugin`（可切换槽，名「VS Code CodeBuddy 扩展」）**都是 VS Code
 * 相关**，但一个是只读观测、一个承担切换 —— 名字若相同，用户根本分不清点哪个。
 */
export const WORKBUDDY_PROGRAMS: readonly ProgramSpec[] = [
  { id: "codebuddy_cn_ide", labelKey: "wbAccounts.program.codebuddyCnIde", status: "available" },
  { id: "codebuddy_cli", labelKey: "wbAccounts.program.codebuddyCli", status: "available" },
  { id: "vscode_cn_inject", labelKey: "wbAccounts.program.vscodeCnInject", status: "available" },
  // ★ 2026-10-09 落地：编辑器内 CodeBuddy 插件两个程序位（原 `planned`）。
  { id: "vscode_codebuddy_plugin", labelKey: "wbAccounts.program.vscodeCodebuddyPlugin", status: "available" },
  { id: "jetbrains_codebuddy_plugin", labelKey: "wbAccounts.program.jetbrainsCodebuddyPlugin", status: "available" },
];

/** 计划新增（**尚未支持**）的程序位。⚠️ 当前**为空**（两条目标均已落地），机制保留。 */
export const PLANNED_PROGRAMS: readonly ProgramSpec[] = WORKBUDDY_PROGRAMS.filter(
  (p) => p.status === "planned",
);

export interface PlatformDescriptor {
  readonly id: Product;
  /**
   * 展示名文案键。`WorkBuddy` / `Trae` 是商标，两种语言下写法相同，但仍走词表。
   *
   * ⚠️ 类型是 `TranslationKey`（**不是 `string`**）：调用方拿到就能直接 `t(...)`，
   * 不必再 `as TranslationKey` 断言 —— 断言会把「键名写错」这类错误从编译期推到运行时
   * （词表查不到 ⇒ 界面直接渲染出键名）。
   *
   * ## 为什么存键而不存成品文案
   *
   * 本表在**模块加载时**就定型，拿不到 `useT()`。渲染处 `t(platformDescriptor(p).labelKey)`
   * 现取一次，语言切换才会反映到侧栏 —— 存成中文会让侧栏永远停在启动时的语言。
   *
   * ## Trae 分区显示 `Trae`（★ 2026-10-17 起，issue #44）
   *
   * 此前这里显示 `TraeWork` —— 那是**已被官方合并掉**的老程序线名。客户端自 2026-10-13 起
   * 每个区域只剩一个合并版客户端（`Trae CN` / `Trae`），继续叫 `TraeWork` 会让侧栏名字
   * 与用户实际在用的客户端对不上。老程序线的名字只在**程序位按钮**（`Work` / `Code` / `Trae`）
   * 与一次性迁移说明里出现，不进侧栏。
   *
   * ⚠️ 但**区域（国内版 / 国际版）不写进侧栏** —— 它由分区内部顶部的区域切换器表达。
   * 把区域名塞进侧栏会让「国内版 / 国际版」看起来像两个独立产品，而它们共用同一组页面。
   *
   * ⚠️ 展示名是 `Trae`，而**标识**仍是 `trae`（路由前缀 `/trae/...`）——
   * 改标识会牵动路由表与全部 `/trae` 链接，两者刻意不绑定。
   */
  readonly labelKey: TranslationKey;
  /** 该平台的首页路径（切到该平台且当前路由不属于它时落到这里）。 */
  readonly home: string;
  /** 该平台拥有的区域（**顺序即 UI 展示顺序**）。 */
  readonly regions: readonly ("cn" | "global")[];
  /** 该平台具备的能力。 */
  readonly capabilities: readonly Capability[];
}

/**
 * 平台注册表 —— **平台的唯一事实来源**。
 *
 * ⚠️ 新增平台 = 这里加一条 + `scope.ts` 的 `Scope` 加对应值，**页面组件零改动**。
 */
export const PLATFORM_DESCRIPTORS: Record<Product, PlatformDescriptor> = {
  workbuddy: {
    id: "workbuddy",
    labelKey: "product.workbuddy",
    home: "/",
    regions: ["cn", "global"],
    capabilities: ["checkin", "travel", "credits", "oauth", "gateway", "autoRotate", "sessionCopy"],
  },
  trae: {
    id: "trae",
    labelKey: "product.trae",
    home: "/trae/accounts",
    regions: ["cn", "global"],
    capabilities: ["checkin", "credits", "oauth", "gateway"],
  },
  /**
   * Qoder —— 阿里出品的 AI 编程 IDE，国内版 / 国际版两套账号体系。
   *
   * ⚠️ **双区域是一个平台内的两个 realm，不是两个平台**：两版共用同一套客户端形态
   * （同一份协议、同一套令牌家族、同一组能力），差别只在端点 / 客户端 id / 本地目录
   * 这几张常量表上 ⇒ 建模成 `qoder-cn` / `qoder-global` 两个作用域。
   *
   * 能力**刻意只有四项**：没有 `sessionCopy`（会话复制依赖客户端自身的会话存储与云端
   * 归属，当前只有 WorkBuddy 桌面具备）、没有 `travel` / `autoRotate`（WorkBuddy 独有）、
   * 没有 `snapshot` / `group`（Trae 有，Qoder 侧无等价物）。
   */
  qoder: {
    id: "qoder",
    labelKey: "product.qoder",
    home: "/qoder/accounts",
    regions: ["cn", "global"],
    capabilities: ["checkin", "credits", "oauth", "gateway"],
  },
};

/** 全部平台，按 UI 展示顺序。 */
export const PRODUCTS: readonly Product[] = ["workbuddy", "trae", "qoder"];

/**
 * 目的地（Destination）—— 侧栏的一级页面（VOCABULARY 第 1 节的 L4）。
 *
 * ⚠️ **6 个固定项，不随平台变化**（§3.1 / §3.9）。这正是 P2/P3 要收敛的目标：
 * 现状两个平台各有若干项但**路径不同**（`/token-stats` vs `/trae/token-stats`），
 * 且 Trae 侧少了「概览 / 一键签到 / 登录态快照 / 系统日志」4 个独立项 ——
 * 那些能力下沉进了页面内部，于是**同一件事在两个产品上出现在不同位置**。
 *
 * ⚠️ **`dest` 是平台无关的标识**：`usage` 在 WB 与 Trae 都是「用量统计」，
 * 具体渲染哪个组件由宿主的路由表决定（见 `App.tsx` 的 `SCOPED_ROUTES`）。
 *
 * ⚠️ **入口是否渲染由 `DESTINATION_CAPABILITY` 决定**（能力门控），**不是**平台分支 ——
 * 例如「会话」（`sessions`）需要 `sessionCopy`，而它只有 WorkBuddy 有 ⇒ Trae 侧不渲染该入口。
 */
export type Destination = "overview" | "accounts" | "usage" | "sessions" | "gateway" | "settings";

/** 全部目的地，**顺序即侧栏展示顺序**（§3.9：概览 / 账号 / 用量 / 会话 / 网关 / 设置）。 */
export const DESTINATIONS: readonly Destination[] = [
  "overview",
  "accounts",
  "usage",
  "sessions",
  "gateway",
  "settings",
];

/**
 * 把**旧的扁平路径**映射到目的地 —— **兼容旧版本的关键函数**（P2）。
 *
 * 现状：WB 的路径不带前缀（`/token-stats`），Trae 带 `/trae` 前缀且个别段名不同
 * （Trae 是 `/trae/credits`，WB 是 `/credit-stats`）。
 * 本函数把两者**都**收敛到同一组 `dest`，供旧路由重定向与新路由解析共用。
 *
 * ⚠️ 返回 `null` 表示"这条路径不是 5 个目的地之一"（例如 `/trae` 本身）——
 * **不要回落某个默认值**，否则会把「根路径」也当成一个页面。
 */
export function destinationOfLegacyPath(pathname: string): Destination | null {
  const p = pathname.replace(/\/+$/, "") || "/"; // 去掉尾斜杠（根路径除外）
  const bare = p.startsWith("/trae") ? p.slice("/trae".length) || "/" : p;

  switch (bare) {
    case "/":
    case "/accounts":
      return "accounts";
    case "/overview":
      return "overview";
    case "/token-stats":
    case "/usage":
      return "usage";
    /* P3：**积分并入用量**（§3.9）。旧的 `/credit-stats` / `/credits` 仍然可达 ——
       它们现在映射到 `usage`，由「用量」页的积分 Tab 承接。 */
    case "/credit-stats":
    case "/credits":
      return "usage";
    case "/api-service":
    case "/gateway":
      return "gateway";
    case "/settings":
      return "settings";
    default:
      return null;
  }
}

/**
 * 目的地在**新路由**里的路径段。与 `Destination` 同名 —— 这样
 * `/s/wb-cn/accounts` 一眼能读出「哪个作用域的哪个页面」，无需查表。
 */
export function destinationPath(dest: Destination): string {
  return dest;
}

export function platformDescriptor(product: Product): PlatformDescriptor {
  return PLATFORM_DESCRIPTORS[product];
}

/**
 * 该平台是否具备某能力。
 *
 * **所有「平台差异」判断都应走这里**，不要在组件里写 `product === "trae"` ——
 * 后者正是 §1.2 诊断出的「两套页面逐字复制」的成因。
 */
export function hasCapability(product: Product, capability: Capability): boolean {
  return PLATFORM_DESCRIPTORS[product].capabilities.includes(capability);
}

/**
 * 目的地的**能力门控**（PLAN §2.7 / §3.15 能力边界）—— **P8 新增**。
 *
 * 目的地在 `DESTINATIONS` 里是**平台无关**的（§3.1），但某些目的地**依赖平台能力**：
 * 「会话」（`sessions`）依赖 `sessionCopy`，而它只有 WorkBuddy 桌面具备（`switch.rs:93`）
 * ⇒ Trae 作用域下**不渲染该入口**（URL 直达仍可达，由页面自己给结构化空态，§3.2 规则 2）。
 *
 * ⚠️ **为什么用能力门控而不是平台分支**：`DESTINATIONS` 是平台无关的，写
 * `product === "trae"` 会破坏这条纪律、且新增平台时必漏改。能力门控是**数据驱动**的
 * （同 §1.2 的 P4/P10 口径）。**未列出的目的地 ⇒ 无门控（恒渲染）。**
 */
export const DESTINATION_CAPABILITY: Partial<Record<Destination, Capability>> = {
  sessions: "sessionCopy",
};

/**
 * 该目的地在该平台是否**可渲染入口**（按能力门控；无门控项恒 `true`）。
 *
 * 侧栏（`App.tsx` 的 `scopedNav`）与命令面板据此过滤 —— 单一实现，避免两处各写一份门控。
 */
export function destinationAvailable(product: Product, dest: Destination): boolean {
  const capability = DESTINATION_CAPABILITY[dest];
  return capability === undefined || hasCapability(product, capability);
}

/**
 * 平台在**作用域字面量**里的短标识。
 *
 * ⚠️ **为什么不直接用 `Product`**：`workbuddy-cn` 太长，而作用域要进 URL 路径段
 * （`/s/wb-cn/accounts`）与 `localStorage` 键 —— 短标识更省也更易读。
 *
 * ⚠️ **映射只有这一处**（下面两张表）。别在别处再写一份 `"wb"` 字面量 ——
 * 那会让「改标识」变成全仓搜索。
 */
export type PlatformSlug = "wb" | "trae" | "qoder";

export const PLATFORM_SLUG: Record<Product, PlatformSlug> = {
  workbuddy: "wb",
  trae: "trae",
  qoder: "qoder",
};

export const SLUG_TO_PRODUCT: Record<PlatformSlug, Product> = {
  wb: "workbuddy",
  trae: "trae",
  qoder: "qoder",
};

/**
 * 从**路由路径**推断平台 —— **兼容旧版本的关键函数**（PLAN §3.1 / D4）。
 *
 * 支持**两种**形态：
 *
 * 1. **新路由** `/s/{scope}/{dest}`（P2 引入）—— scope 里就带着平台短标识，**优先**用它；
 * 2. **旧路由**：WorkBuddy 在根（`/`、`/settings`…），Trae 在 `/trae/*`。
 *
 * ⚠️ **不能只看 `/trae` 前缀**：`/s/trae-cn/overview` 不带 `/trae` 前缀，
 * 只按旧规则会**误判成 WorkBuddy**（P2 收尾时踩到过）。
 *
 * ⚠️ 这里**内联**解析 scope 而不 import `lib/scope`：`scope.ts` 依赖本文件的
 * `PlatformSlug`，反向 import 会形成循环依赖。
 *
 * ⚠️ **每个平台的旧前缀都要显式列出**（不能只写 `/trae`）：旧路由形态是
 * 「WorkBuddy 在根、其余平台在自己的前缀下」，漏一个就会把该平台的旧链接
 * **静默判成 WorkBuddy** —— 用户看到的是「切到别的平台的账号页」。
 */
export function platformOfPath(pathname: string): Product {
  const m = /^\/s\/([^/]+)/.exec(pathname);
  if (m) {
    const slug = m[1].split("-")[0] as PlatformSlug;
    if (Object.prototype.hasOwnProperty.call(SLUG_TO_PRODUCT, slug)) {
      return SLUG_TO_PRODUCT[slug];
    }
  }
  /* 前缀必须带分隔符：`/traeish` / `/qoderish` 不是这两个分区（见门禁用例）。 */
  if (pathname === "/trae" || pathname.startsWith("/trae/")) return "trae";
  if (pathname === "/qoder" || pathname.startsWith("/qoder/")) return "qoder";
  return "workbuddy";
}

/** 该路径是否属于该平台（用于「切平台时判断当前路由是否要跳走」）。 */
export function pathBelongsTo(pathname: string, product: Product): boolean {
  return platformOfPath(pathname) === product;
}

/* ────────────────────────────────────────────────────────────────
 * P2 收敛清单（本文件建立时登记，完成一项勾一项）
 *
 * - [x] `App.tsx` 的 `type Product` 删除，改为 `import type { Product } from "@/lib/platforms/registry"`
 * - [x] `App.tsx` 的 `PRODUCT_NAV` 删除（侧栏改由 `DESTINATIONS` + `DEST_META` 生成）
 * - [x] `App.tsx` 的 `PRODUCT_LABEL_KEY` 删除（`PlatformDescriptor.labelKey` 已在本表）
 * - [x] `App.tsx` 的 `PRODUCT_HOME` 删除（`PlatformDescriptor.home` 已在本表）
 * - [x] 组件里的平台分支改为**数据 / 能力驱动** —— **能改的已改**：
 *       · `UsagePage`（可用视图集由 `PLATFORM_TABS` 给出、Token 页由 `USAGE_PAGES` 取，
 *         不再二次判平台）；
 *       · 命令面板「打开数据目录」（改走参数化命令 `open_data_dir`，`scope` 自带平台与区域）；
 *       · `backup-import-preview` / `source-account-picker` 的产品展示名（改走 `labelKey`）。
 * - [ ] **剩下的 3 处是「结构性差异」，不是「能力差异」—— 刻意不改 `hasCapability`**：
 *       ① `App.tsx` 的运行状态圆点：WB 读账号 store、Trae 读变体探测 —— **两个数据源**；
 *       ② `App.tsx` 的 `ScopeSwitch`：Trae 的区域由 `?line=` 承载（含程序位维度）；
 *       ③ 命令面板的 `refreshScope`：只有 WB 有跨区域账号 store 需要顺带重读。
 *       ⇒ 判据：`hasCapability` 回答「这个平台**有没有**某功能」；上面几处回答
 *       「这个平台的**数据 / 控件从哪来**」—— 后者要靠 `PlatformDescriptor` 扩字段
 *       （形如 PLAN §2.3 的 `loadScopeStatus`），**不是**能力开关。
 *       ⚠️ 扩字段前先查 `docs/specs/VOCABULARY.md`（第 7 节规则 4：表里没有的词不许出现在代码里）。
 * ──────────────────────────────────────────────────────────────── */
