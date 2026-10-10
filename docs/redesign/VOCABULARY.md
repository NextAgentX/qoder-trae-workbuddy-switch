# 词汇表（P0 冻结）

> **状态**：已冻结（2026-10-03，P0 产出）。
> **约束**：新代码一律用「新词汇」；「旧词汇」只允许出现在兼容层，并按「删除时点」清退。
> **配套代码**：`src/lib/scope.ts`（作用域）· `src/lib/platforms/registry.ts`（平台与能力）。
> **门禁**：`npm run check:scope`（9 条断言，含 fail-closed 与旧路由映射）。

## 1. 五个层级（别混）

| 层 | 词 | 含义 | 取值示例 |
|:---|:---|:---|:---|
| L1 | **平台**（Platform / Product） | 一条产品线 | `workbuddy` / `trae` |
| L2 | **区域**（Region） | 同一平台的两个版本 | `cn` / `global` |
| L3 | **作用域**（Scope） | **平台 × 区域** = 「数据从哪来」 | `wb-cn` / `trae-global` |
| L4 | **目的地**（Destination） | 侧栏的一级页面 | `accounts` / `usage` / `settings` |
| L5 | **视图参数**（View param） | 同一页面里「怎么看」 | 时间范围 / 数据来源 / 合并态 |

⚠️ **L3 与 L5 最容易混**：

- **作用域决定数据从哪来**（实体归属）⇒ 进 **URL 路径段**（`/s/wb-cn/accounts`）；
- **视图参数只决定怎么看** ⇒ 留在查询参数或页内 state，**不进路径**。

判据：**换一个值，会不会导致"取的是另一份数据"？** 会 ⇒ 作用域；不会 ⇒ 视图参数。

## 2. 平台 vs 程序位（**两个不同的东西**）

| 词 | 定义 | 本仓取值 |
|:---|:---|:---|
| **平台** | 产品线 | `workbuddy`、`trae` |
| **程序位**（Program） | **账号落到哪个客户端** | WB 侧：WorkBuddy 桌面 / CodeBuddy IDE(CN) / CodeBuddy CLI；Trae 侧：TraeWork / TraeCode |

⚠️ 程序位**不是**平台的下级分类，而是**账号的属性**（一个账号可绑 0..N 个程序位）。
⚠️ **待新增**：VS Code CodeBuddy 插件、JetBrains IDE 插件（**当前不存在**，见 PLAN §3.3 的 B 表）。

## 3. 能力（Capability）vs 偏好（Preference）—— **别混**

| | 能力 `capabilities` | 偏好 `preferences` |
|:---|:---|:---|
| 谁决定 | **平台固有** | **用户自己选** |
| 存放 | `PLATFORM_DESCRIPTORS`（代码） | localStorage |
| 改错了会怎样 | **代码错**：不该有的功能出现了 | 只是显示变化，**数据与账号库不受影响** |
| 校验位置 | **后端命令分发层** | 纯前端 |

**能力枚举**（`registry.ts` 的 `Capability`）：
`checkin` · `travel` · `credits` · `oauth` · `gateway` · `autoRotate` · `sessionCopy`

⚠️ `sessionCopy` **只有 WorkBuddy 有**，且精确到 **WorkBuddy 桌面**（CodeBuddy IDE / CLI 都没有）——
见 PLAN §3.15 的能力边界。

## 4. 旧词汇 → 新词汇（含**删除时点**）

| 旧 | 新 | 为什么改 | 删除时点 |
|:---|:---|:---|:---|
| `variant`（散落各处） | `region`（区域）/ `program`（程序位） | **一个词被用来表达两个概念** —— 这是最要命的混淆 | **P2** |
| `Product`（`App.tsx` 局部类型） | `Product`（`platforms/registry.ts`） | 类型搬家，消除重复定义 | ✅ **P2 已完成**（`App.tsx` 改为 `import`） |
| `PRODUCT_NAV` / `PRODUCT_LABEL_KEY` / `PRODUCT_HOME`（`App.tsx`） | `PlatformDescriptor` 的字段 | 收进注册表，新增平台不再改 `App.tsx` | ✅ **P2 已完成**（三者均已删除：侧栏由 `DESTINATIONS` + `DEST_META` 生成；显示名与首页走 `platformDescriptor()`） |
| `TraeVariant`（及 `TraeVariantId`） | `program`（Trae 侧程序位） | 名字里的 `Variant` 是旧的模糊词 | **P2**（`TraeVariantId` 仍在 `trae-types.ts`，改名属收尾项） |
| 路由 `/trae/accounts` | `/s/trae-cn/accounts` | 让作用域进 URL（唯一真值） | **不删** —— 旧路由保留为**重定向**（D4） |
| 旧命令名（`checkin` / `trae_checkin`…） | `*_for(scope, …)` | 统一签名 | **不删** —— 保留为**别名转发**（§2.5），且**硬编码自己的 scope** |

⚠️ **`variant` 的两种含义必须拆开**（这是本表的重点）：

- 指「国内版 / 国际版」⇒ 用 **`region`**；
- 指「TraeWork / TraeCode」⇒ 用 **`program`**。

⚠️ 后端已有 `region` 参数（如 `authenticated_post_for(region, …)`），与前端 `region` **同义** ⇒ 保持一致，不要另造词。

## 5. 作用域字面量规则

- 形如 `{平台短标识}-{区域}`：`wb-cn` / `wb-global` / `trae-cn` / `trae-global`；
- 短标识映射**只在 `registry.ts` 一处**（`PLATFORM_SLUG` / `SLUG_TO_PRODUCT`）——
  别在别处再写 `"wb"` 字面量，否则「改标识」会变成全仓搜索；
- **解析必须 fail-closed**：非法值返回 `null`，**不许 `?? DEFAULT_SCOPE`** ——
  那等于把 fail-closed 又变回 fail-open，而静默回落会把数据写进**错误的库**；
- **默认值不写进 URL**（§3.1 规则 3）。

## 6. 「不劫持」的定义（D29 拍板）

**不改 hosts / 不做 DNS 劫持 / 不做透明代理**；只接受客户端**主动**把请求交给网关
（配代理或配 base URL）。

⚠️ 与之相对：**「用户显式信任 CA 的中间人」不算劫持**（D33 宽口径）——
判据是**知情 / 影响范围 / 可撤**三条，**不是「是否解密」**。

## 7. 本表怎么用

1. **写新代码前**：查第 1、2 节确认层级，别把视图参数当作用域；
2. **改平台差异时**：查第 3 节 —— 用 `hasCapability()`，**别写 `product === "trae"`**；
3. **看到 `variant`**：查第 4 节拆成 `region` 或 `program`；
4. **新增词汇**：先加进本表再写代码 —— **表里没有的词，代码里不许出现**。
