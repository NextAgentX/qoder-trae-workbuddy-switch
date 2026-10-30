# 词汇表（P0 冻结）

> **状态**：已冻结（2026-10-03，P0 产出）；**2026-10-14 增补**「命名空间」一词（P16 统一网关引入）；**2026-10-30 增补**「网关流式失败」6 词（issue #129，见文末附节）。
> **约束**：新代码一律用「新词汇」；「旧词汇」只允许出现在兼容层，并按「删除时点」清退。
> **配套代码**：`src/lib/scope.ts`（作用域）· `src/lib/platforms/registry.ts`（平台与能力）· `crates/buddy-switch-core/src/modules/gateway_namespace.rs`（命名空间）。
> **门禁**：`npm run check:scope`（**291 条断言** @2026-10-14，含 fail-closed 与旧路由映射；初版为 9 条）。

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
| **程序位**（Program） | **账号落到哪个客户端** | WB 侧：WorkBuddy 桌面 / CodeBuddy IDE(CN) / CodeBuddy CLI / VS Code CodeBuddy 扩展 / JetBrains CodeBuddy 插件；Trae 侧：TraeWork / TraeCode |

⚠️ 程序位**不是**平台的下级分类，而是**账号的属性**（一个账号可绑 0..N 个程序位）。
⚠️ VS Code CodeBuddy 插件、JetBrains IDE 插件**已于 2026-10-09 落地**（注册表里两条均 `status: "available"`）——
本表原写「当前不存在」，2026-10-14 复核更正。

## 2.1 命名空间（Namespace）—— 与作用域**同形**，但**不是**作用域

> 2026-10-14 新增（P16 统一网关引入）。定义在 `crates/buddy-switch-core/src/modules/gateway_namespace.rs`。

| 词 | 定义 | 取值 |
|:---|:---|:---|
| **命名空间**（Namespace） | **统一网关**（端口 `57892`）里，**模型 id 的来源前缀** —— 让 id 自己携带「这条模型来自哪套上游」 | 默认 4 条：`wb-cn` / `wb-global` / `trae-cn` / `trae-global`；可选 4 条：`traework-cn` / `traework-global` / `traecode-cn` / `traecode-global` |

★★ **它与作用域的字面量逐字相同（`wb-cn` 既是作用域也是命名空间），但语义完全不同，别混**：

| | 作用域 `Scope` | 命名空间 `Namespace` |
|:---|:---|:---|
| 回答什么 | 「**界面现在管哪套数据**」 | 「**这条模型请求该发给哪套上游**」 |
| 归属 | 前端路由 / 视图（进 URL 路径段 `/s/wb-cn/…`） | 网关请求解析（进模型名 `wb-cn:deepseek-…`） |
| 载体 | `src/lib/scope.ts` | `gateway_namespace.rs` |
| 与 Key 的关系 | 无关 | **统一 Key 授权的是命名空间集合**（一把 Key 可同时授权 `wb-*` 与 `trae-*`） |

★ **命名空间 = `{客户端名}-{区域}`**，**两层**：派生层由 `Scope::all()` 生成（新增平台自动出现，零手写）；扩展层是平台内部额外程序线（当前只有 Trae 两条老程序线，手写、**默认关**）。

⚠️ **三条硬规则**（违反即静默走错池）：

1. **解析只做整段精确匹配，禁止前缀匹配** —— `trae` / `traework` / `traecode` **互为前缀**；
2. **禁止用 `TraeVariant::parse` / `TraeProgram::parse` 反推** —— 那是**宽容**解析，会把 `traework-cn` 吞进去；
3. **区域轴与程序位轴的映射一律经既有派生函数**（`Scope::wb_region` / `Scope::trae_region` / `variant_for_program`），**不手搓字符串**。

⚠️ **与 WB 旧前缀（`cn:` / `global:`）不是一回事**：旧前缀属 **WB 独立端口**，语义是「跨域重路由」；命名空间属**统一网关**，语义是「选择来源」。**两套前缀不跨入口混用**。

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

**反例（P15 之后新增，别搞混）**：「客户端会话监听」**不是**能力，而是**偏好驱动的动作**。

| 为什么不进 `Capability` | 说明 |
|:---|:---|
| 它与**平台**无关 | 任何平台只要有 WorkBuddy 客户端就能开 |
| 它问的是**用户同不同意**，不是「这个平台支不支持」 | 开启会**改写上游客户端的配置文件**（装 hook），属于「必须显式同意」的动作，载体是 `app_prefs.watch_agent_sessions`（**默认 `false`**） |
| 它的「关了会怎样」与偏好一致 | 关掉 = 卸 hook + 不再监听；**账号库与会话数据不受影响**（与偏好那一列的性质吻合） |

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

## 附：网关流式失败术语（issue #129，2026-10-30 增补）

> 这一节的词**只允许**在「网关把上游字节转给客户端」这条链路上用；别挪去描述 UI 状态。
> 代码落点：[`ACCOUNT-FAILOVER.md`](ACCOUNT-FAILOVER.md)、`crates/buddy-switch-gateway/src/failover.rs`。

| 词 | 定义 | 别混成 |
|:---|:---|:---|
| **首帧**（first frame） | 上游 SSE 响应体里**第一块被读取的字节**。它到达时 `200 + text/event-stream` 尚未写回客户端 | 「首字节延迟（TTFB）」是**耗时指标**，不是这一块字节 |
| **首帧前 / 首帧后** | 以「网关是否已把响应头交给 axum」为界。这条界线决定**还能不能换号** | 「请求前 / 请求后」 |
| **换号**（rotate） | 丢弃当前账号的这次尝试，改用池里**另一个**账号重发同一份请求 | 「降级（fallback）」是换**模型**；两者正交，且降级在换号外层 |
| **整步重跑**（whole-step rerun） | 客户端重试语义：丢弃本次已收到的全部输出，从同一份历史重新请求一次 | 「续写」—— 协议上不成立（无 assistant 前缀续写） |
| **静默截断** | 上游断流后网关仍补终止标记 ⇒ 客户端判「模型说完了」，既不报错也不重试 | 「硬中断」是**可见的**失败（客户端拿到错误并决定重试） |
| **自动切换**（auto_failover） | 开关名：把流中途的账号故障变成**可重试错误** + 换号 + 客户端整步重跑。默认关 | 「自动重试」—— 重试发生在客户端，网关只负责把失败**说清楚** |

⚠️ 两条易错：

- **「换号」不等于「重试」**：网关的换号发生在**首帧前**（真换了账号）；首帧后网关只能让失败变成可重试，真正重跑的是客户端；
- **「静默」只允许对外**：开关开启时对客户端静默的是**实现细节**，对运维必须完全透明（冷却 / 粘性解绑 / `last_error` / 日志状态码，见 `ACCOUNT-FAILOVER.md` §6）。

## 7. 本表怎么用

1. **写新代码前**：查第 1、2 节确认层级，别把视图参数当作用域；
2. **改平台差异时**：查第 3 节 —— 用 `hasCapability()`，**别写 `product === "trae"`**；
3. **看到 `variant`**：查第 4 节拆成 `region` 或 `program`；
4. **新增词汇**：先加进本表再写代码 —— **表里没有的词，代码里不许出现**。
