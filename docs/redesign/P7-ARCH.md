# P7 架构分解：全局备份与恢复

> **状态**：设计（2026-10-04）。本文是 `P5-P12-TASKS.md` P7 段的**落地设计**，可直接交给工程师。
> **上游**：`PLAN.md` §2.6（备份数据契约：envelope / 四条规则）、§3.14（备份与恢复 UI：导出 / 导入 / 五条行为约束）、§3.9（设置 9 项子导航）、§4.2（新增可备份内容类型）、§2.5（安全边界：fail-closed / 默认拒绝）、§2.7（能力 ≠ 偏好）、§3.4 规则 1（无数据不渲染）、§3.5（文案不许假承诺）、§3.6（状态语言：部分失败态 / 过期态）、§3.10 第 1 条（不写死数字）；`DECISIONS.md` **D14**（默认全选作用域）/ **D15**（凭据默认关）/ **D16**（冲突默认保留本地）/ **D17**（定时备份每日保留 7 份）/ **D18**（导入前快照强制）/ **D12**（允许平台专属子项）；`GAP-ANALYSIS.md` §2.4（P7 十二条全 ❌）。
> **格式参考**：`P10-ARCH.md` / `P11-ARCH.md`（同构：实现方案 / 文件列表 / 数据结构 / 有序任务 / 共享知识 / 风险 / 待明确 / 依赖图）。
> **下游**：§4 任务列表直接交给工程师。
> **范围**：**仅** P7 —— ① 备份 envelope 契约（导出 / 导入）；② 设置页「备份与恢复」（第 9 项子导航）。
> **明确排除**：账号级导入导出（`export-accounts-dialog` / `import-accounts-dialog`）**保留不动**（§3.14「并存不替换」）；**命令面板两动作（「导出全部备份」/「从备份导入」）本期不做**——P6（命令面板本体）未落，现在登记动作**无法接线 ⇒ 死代码**，**推到 P6 之后**（§7 #6）。
> **依赖**：**P5**（`Scope::parse` fail-closed / `resolve_account_key` 统一谓词 / `BackgroundTask`）。P5 **进行中** ⇒ **本设计可以先做**（见 §1.10）。

---

## 1. 实现方案

### 1.1 核心难点

| 难点 | 现状（证据） | 处理 |
|:---|:---|:---|
| **★ 凭据红线要「可证伪」** | 现无备份功能；`export_accounts` **始终带 token**（`export_import.rs:183-185`） | 凭据是**可选字段**（`includesCredentials=false` 时**完全不出现** token 键）；判据 = 在导出文本里**搜真实键 ⇒ 0 命中**（§1.3） |
| **★ 现有合并「无 token 即跳过」** | `merge_import_record`（`export_import.rs:84-87`）：`access_token` 取不到 ⇒ `Skipped` | 与「不含凭据备份」**直接冲突**（无凭据 ⇒ 一条都导不进去）⇒ 合并必须**泛化 token 策略**（§1.4） |
| **★ 现有合并只有「覆盖同 uid」** | `merge_import_record` 命中 uid ⇒ **无条件覆盖**（`export_import.rs:88-103`） | 全局导入需 **4 种冲突策略**（保留本地 / 覆盖 / 保留两者 / 逐条），默认**保留本地**（D16）⇒ 幂等才成立（§1.5） |
| **★ 未知 scope 必须 fail-closed** | 无备份功能；Rust 已有 `Scope::parse → Result`（`product.rs:98-176`） | 文件里 scope 键走 `Scope::parse`，**未知 ⇒ 该分区整体拒绝并报错**（不静默跳过、不回落，§2.5 第 1 条）（§1.6） |
| **导入是**不可逆写**，需安全阀** | 无导入预览 | **先预览后落盘**：预览**不写盘**、逐 scope 给「新增/覆盖/跳过/冲突」计数 + 需重新登录数 + 冲突策略（§1.7） |
| **导入要能撤销** | 无 | **导入前强制快照**（D18，不可关）+「从自动备份恢复」入口（§1.8） |
| **换机要一次搬走** | 只有账号级（单作用域、不含配置） | 遍历 `SCOPES` 注册表**一次导出 4 作用域**（账号 / 配置 / 网关）（§1.11） |
| **定时备份要登记出口** | `BackgroundTask` 在**两处**（`src-tauri/src/lib.rs:40` / `crates/buddy-switch-server/src/main.rs:42`） | 新增任务**两宿主都登记**（否则「调用点静默消失」，P5 R9） |

### 1.2 备份 envelope 契约（§2.6）

**为什么不能直接打包 store 原始文件**（§2.6）：原始文件含凭据与缓存（**做不到「不含凭据」开关**）、格式随版本演化（跨版本必碎）、无法选择性导出。⇒ 用**结构化 JSON envelope**。

```jsonc
{
  "kind": "buddy-switch-backup",       // 文件类型标识；导入前先校验（防误导入任意 JSON）
  "formatVersion": 1,                  // 契约版本，单调递增 —— **唯一兼容依据**
  "appVersion": "2026.10.1",           // 仅提示，不参与判断
  "exportedAt": "2026-10-01T14:00:00Z",
  "includesCredentials": false,        // 导入时据此决定是否提示「需重新登录」
  "scopes": {
    "wb-cn": {
      "accounts": [ /* 账号记录，字段与现有 export_accounts 同源（snake_case） */ ],
      "config":   { "checkin": {…}, "travel": {…}, "autoRotate": {…}, "switch": {…} },
      "gateway":  { "keys": [], "models": [], "pool": [] }
    },
    "wb-global": {…}, "trae-cn": {…}, "trae-global": {…}
  },
  "appSettings": { /* 语言/外观/紧凑模式/上次作用域（前端 localStorage）+ 开机自启(OS)/更新代理(后端) —— 见 §7 #3 */ }
}
```

**四条契约规则**（§2.6）：

1. **按 scope 分组**（不是按平台）；导出**遍历 `SCOPES`** ⇒ 加平台/区域时备份**自动覆盖，代码零改动**。
2. **`formatVersion` 是唯一兼容依据**（fail-closed，§2.5 同源）：`> 上限` ⇒ 拒绝 + 提示升级（**不尽力解析**）；`< 当前` ⇒ 走迁移函数逐版升级（**预留 `migrate_envelope(v)` 入口，本期只有 v1、无历史版本可迁**，§7 #8）；缺 `kind` 或 `kind` 不匹配 ⇒ **拒绝**。
3. **凭据是「可选字段」不是「可选结构」**：`includesCredentials=false` 时账号记录里**完全不出现** `access_token` / `refresh_token`（**不是**写成 `null`）⇒「文件里搜不到该键」才能当**可证伪断言**。
4. **导入按 `uid` 去重**（账号的稳定身份）。

> ⚠️ **字段大小写**（本设计已核实）：账号库记录的键是 **`access_token` / `refresh_token`（snake_case）**（`export_import.rs:85` / `account.rs:423`）；客户端认证文件里才是 `accessToken`（camelCase）。**导出的是账号库记录 ⇒ snake_case**（详见 §1.3）。
>
> ⚠️ **`appSettings` 的责任边界（§7 #3 拍板 · 已核实来源）**：该段**不是全在 localStorage**——按 `app-settings.tsx` 实际归属分两类：
>
> | 子项 | 真实归宿（已核实） | 备份责任 |
> |:---|:---|:---|
> | 语言（`i18n.ts`）/ 外观（`theme.ts`）/ 紧凑模式（`use-compact-mode.ts`）/ 上次作用域（`use-scope.ts`） | **前端 localStorage** | **前端负责**读写 |
> | 开机自启 | **OS 登录项**（后端 `getLaunchAtLoginEnabled`） | 导出读、导入写回（**反映系统状态**，非普通值） |
> | 更新代理 | **后端 `githubConfig`**（`getGithubConfig` / `saveGithubConfig`） | 后端负责 |
>
> ⇒ **localStorage 段由前端负责**（导出传值 / 导入回写）；**非 localStorage 子项从各自来源读**。**落盘前须确认各子项无敏感数据**；**⚠️ 尤其「更新代理 URL」可能形如 `http://user:pass@host`（代理认证）** ⇒ 若命中凭据，**按 §1.3 同一套 `CREDENTIAL_KEYS` 判据剥离**（同一红线，不新开例外）。

### 1.3 ★ 凭据红线：可验证判据（P7 最硬的安全要求）

**红线**：`includesCredentials=false` 的备份文件里，**不得出现任何凭据**。不能只写「我们不会写进去」——必须有**可证伪的判据**。

**做法（双层）**：

1. **实现层**：导出时按**固定键集**逐条剥离凭据键（**白名单剔除**，不是黑名单保留），键集是**唯一常量** `CREDENTIAL_KEYS`；
2. **验收层**：把导出文本当**纯文本**搜固定键集 ⇒ **0 命中**（可证伪断言）。

**★ 键集必须用真实键（否则假阴性）**：

```rust
/// 凭据相关键：**唯一来源**，导出剥离与验收搜索共用同一集合。
/// ⚠️ 必须用账号库的**真实键**（snake_case）—— 用 camelCase 会让断言**恒真**（假阴性）。
/// ⚠️ **6 个键**（含 Trae 的 `jwt`）：实现比初版设计多一个 `jwt`，**以实现为准**
///    （Trae 账号凭据键是 `jwt`，`trae-types.ts`；漏了它 ⇒ 换机备份泄露即可盗号）。
pub const CREDENTIAL_KEYS: &[&str] = &[
    "access_token",   // 账号库真实键（snake_case）
    "refresh_token",  // 账号库真实键（snake_case）
    "accessToken",    // 保险：客户端认证文件的写法（防误抄进来）
    "refreshToken",   // 保险
    "$wbEncrypted",   // 加密信封标记（信封本身也是凭据载体）
    "jwt",            // Trae 账号真实凭据键（Trae 侧凭据载体）
];
```

> ⚠️ **PLAN §6 / P5-P12-TASKS 的判据写的是 `accessToken` / `refreshToken`（camelCase）—— 那是错的**：账号库真实键是 snake_case，搜 camelCase 会**0 命中恒真**，即便 `access_token` 明文在文件里也测不出来。**必须改成 §1.3 的键集**（见 §6 R1、§7 #1）。

**加密信封的特殊处理（`$wbEncrypted`）**：账号库里的 `access_token` 可能是**客户端加密信封**（`account::is_envelope`）。信封的密钥在客户端 OS 安全存储里，**换机后解不开** ⇒ 即便 `includesCredentials=true`，信封也**无搬运价值**。本设计：

- `includesCredentials=false`：**所有**凭据键（含信封）一律剥离；
- `includesCredentials=true`：只导出**明文**凭据；信封**照旧剥离**，并在预览/结果里如实告知「N 个账号是加密凭据，需重新登录」（**不许**把解不开的信封当「已带凭据」承诺）。

**★ 文案要求（§7 #2 拍板）**：**不得**写「备份完整」；必须写「**凭据不含在内，导入后需重新登录**」（zh）/「**Credentials are not included; you'll need to sign in again after import**」（en）。这是 §3.5「文案不许假承诺」红线的**具体落点**——信封剥离后**本就不完整**，说「完整」即假承诺。zh + en 同步。

### 1.4 ★ 合并语义：复用 + 泛化（不重写内层）

**铁律**（§3.14）：全局备份是**外层编排**，内部复用同一套合并语义，**不重写内层**（避免两套实现漂移）。但现有 `merge_import_record` 有**两处与全局备份冲突**，必须**泛化**（不是新写一份）：

| 现有行为 | 与全局备份的冲突 | 泛化 |
|:---|:---|:---|
| 无 `access_token` ⇒ `Skipped` | 不含凭据备份**全是无 token** ⇒ 一条都导不进 | 加 `TokenPolicy { Require, Allow }`：账号级 `Require`（现状不变），全局 `Allow` |
| 命中 uid ⇒ **无条件覆盖** | 全局导入要 4 种冲突策略、默认保留本地 | 加 `ConflictPolicy { KeepLocal, Overwrite, KeepBoth, Ask }` |

```rust
// export_import.rs（唯一合并实现，加两个策略参数）
pub enum ConflictPolicy { KeepLocal, Overwrite, KeepBoth, Ask }
pub enum TokenPolicy { Require, Allow }

pub enum MergeOutcome { Appended, Overwritten, KeptLocal, KeptBoth, Skipped }

/// ★ 唯一合并出口：账号级与全局备份**共用**（禁止新写一份）。
pub fn merge_import_record(
    accounts: &mut Vec<Value>,
    item: &Value,
    conflict: ConflictPolicy,
    tokens: TokenPolicy,
) -> MergeOutcome;
```

- **账号级**（`import_accounts_for`）调 `(Overwrite, Require)` ⇒ **与现状逐字节一致**（P0-1「CN 行为零变化」不破）。
- **全局备份**调 `(policy, Allow)`。
- **`KeepBoth`**：uid 命中时不覆盖，把导入记录作为**新记录追加**（**赋新 `id`** 避免 id 撞车）；这是唯一会「增行」的策略。
- **`Ask`**：由 UI 逐条决定 ⇒ 后端收到的是**逐条**的 `conflict`（同一文件不同条目可不同）。⚠️ **本期 UI 裁剪**（§7 #5）：枚举保留、**不渲染逐条入口**，用户侧恒取 `KeepLocal`。

> ⚠️ **uid 是去重键**（§2.6 规则 4）；账号**定位**（查找 / 删除 / 备注）走 P5 的**唯一谓词** `resolve_account_key`（`id` **或** `uid`，`account.rs:48-51`）。两者兼容：导入记录带 uid ⇒ 后续按 uid 删得掉。**禁止**新写一套「只匹配 id」的定位（那正是 P5 修掉的「导入进来了但删不掉」缺陷，`account.rs:30-47`）。

### 1.5 导入幂等（§3.14 行为约束 1）

**定义**：同一文件连续导入两次，第二次结果应为**全部「跳过」**，不是重复新增。

**成立条件**（缺一不可）：

1. 去重键是 **uid**（§1.4）；
2. 默认冲突策略是 **保留本地**（D16）⇒ 命中 uid 时 `KeptLocal`（不覆盖、不追加）；
3. **`KeepBoth` 不能是默认**（它每次都会增行 ⇒ 破坏幂等）。

> **这是最容易写错、也最容易漏测的一条**（§3.14 原话）。T04 用「连导两次」变异验证兜住。

### 1.6 未知 scope：fail-closed（§3.14 行为约束 5 / §2.5 第 1 条）

文件里的 `scopes` 键必须能解析成**枚举**（Rust 已有 `Scope::parse → Result<Scope, ScopeError>`，`product.rs`）：

- 出现未知 scope（例如更高版本产出的新平台）⇒ **该分区整体拒绝并报错**，**不静默跳过、不回落默认**；
- **整份文件**的处理：未知 scope 存在时，**该 scope 的分区**拒绝并计入失败；**其余已知 scope 照常**（部分失败态，§1.7）——**绝不**把未知 scope 的账号塞进某个已知 scope。

> 与 P5 的 `Scope::parse` **同一口径**：`Err` 即拒绝，**绝不** `unwrap_or(Scope::WbCn)`（`check-api-contract.cjs:121` 已把该回落列为反模式）。

### 1.7 导入：先预览、后落盘（安全阀）

**导入是不可逆写操作** ⇒ **预览是安全阀**（让用户看清「这次导入会改什么」）。参考 P5 回收站的经验：**只做后端不做入口 = 用户零收益 + 假承诺风险**。

**第 1 步 · 预览**（`preview_import_backup`，**只读、不写盘**）：

- 头部：`kind` + `formatVersion` + `exportedAt` + 含/不含凭据（对应断言「预览显示契约版本」）；
- 逐 scope：**新增 / 覆盖 / 跳过 / 冲突** 计数；
- 配置：列出**将被覆盖的配置项**（签到 / 旅行 / 轮换 / 切换）；
- 不含凭据 ⇒ **显式提示「需重新登录 N 个账号」**（断言）；
- 冲突策略选择器：**保留本地（默认）/ 覆盖 / 保留两者**（D16）。⚠️ **本期 UI 只给这三种**（§7 #5 拍板）：`Ask`（逐条询问）**后端枚举保留、UI 裁剪**——逐条 UI 成本高、三种批量策略已覆盖主场景；`Ask` 不出现即用户侧**恒取默认 `KeepLocal`**，**不渲染**「逐条询问」选项（避免「有入口但没接线」的假承诺）。

**第 2 步 · 结果**：走 §3.6 **部分失败态**（汇总条 + 「重试失败项」+ 可展开明细），**不许只 toast 一句「导入完成」**（断言）。

### 1.8 可撤销：导入前强制快照 + 定时备份

- **导入前自动快照**（D18，**强制不可关**）：导入**先**把当前全量写入 `backup_dir()`（`config.rs:280` 已有该目录），**再**落盘；并提供「**从自动备份恢复**」入口。这是「导入可撤销」的**后端支撑**——UI 才敢承诺撤销（§2.5 第 3 条：不许承诺后端做不到的撤销）。
- **定时自动备份**（D17）：每日一次、保留最近 **7** 份、存 `backup_dir()`；**登记 `BackgroundTask`（两宿主都加）**。
  - **默认开**（§7 #7 拍板：备份价值在于「出事时有」⇒ 默认开启）；开关状态**存应用设置**（`appSettings`，与 §1.2 同一段）。
  - ⚠️ **必须能在设置里关掉**（用户控制，§7 #7）：**不许**只有「默认开」而**无关闭入口**——关闭后 `BackgroundTask` 不再产生新备份（**已产生的保留**，不追溯删除）。
- **保留策略**：超过 7 份删最旧；**手动导出**的文件**不**进这个保留集（那是用户自己保管的）。

> ⚠️ **定时备份是「写状态出口」** ⇒ 必须登记（P5 §2.2 / R9：**登记即执行**，未登记的任务在生产上永不执行）。

### 1.9 与现有账号级导入导出：**并存，不替换**（§3.14）

| 场景 | 用哪个 |
|:---|:---|
| 在当前作用域里精确挑几个账号给同事 | **账号级**（现状保留，**不动**） |
| 换机、灾备、整机搬迁 | **全局备份**（本节新增） |

> **不重写内层**：全局备份调 `merge_import_record`（§1.4），账号级仍是它自己的对话框。

### 1.10 与 P5 的关系

- **依赖 P5 的 `Scope::parse`**（fail-closed，§1.6）—— P5 已落（`product.rs`）。
- **依赖 P5 的 `resolve_account_key`**（唯一谓词，§1.4）—— P5 已落（`account.rs:48`）；**「删 id / 查 uid 不一致」已由 P5 修复**，P7 **不再重复修**（见 §7 #4）。
- **依赖 P5 的 `BackgroundTask`**（定时备份，§1.8）—— P5 **进行中**；若 P5 T04（`BackgroundTask` 二维化）未落，则**仍按现状在两侧注册表各加一条**（不阻塞）。
- ⇒ **P7 设计可以先做**；实现顺序上 **T01（导出 + 命令）与 P5 无强耦合**，可先起。

### 1.11 分区表驱动 + 4/4 覆盖（§2.6 规则 1 / §4.2）

**表驱动**：一个 `BackupSectionSpec` 表（`accounts` / `config` / `gateway` 三行），每行声明「属于哪个 scope / 怎么读 / 怎么合并」。导出面板的**分区勾选自动出现**（数据驱动，无需改 UI）。**新增可备份内容类型 = 加一行**（§4.2）。

**4/4 覆盖**：导出**遍历 `SCOPES` 注册表**（TS 侧 `src/lib/scope.ts` / Rust 侧 `Scope::all()`）⇒ 断言「每个作用域都出现在导出文件里」**4/4**（加平台后自动 5/5）。

---

## 2. 文件列表

### 新建

| 相对路径 | 作用 |
|:---|:---|
| `crates/buddy-switch-core/src/modules/backup.rs` | envelope 契约（`kind` / `formatVersion` / `includesCredentials`）+ 导出组装 + 导入编排（预览 / 合并 / 汇总）+ 快照 + 定时备份 + `CREDENTIAL_KEYS` |
| `crates/buddy-switch-core/src/modules/backup_sections.rs` | `BackupSectionSpec` **表驱动**（`accounts` / `config` / `gateway` 三行：read / merge） |
| `src/components/settings/settings-subnav.tsx` | 设置页 **9 项子导航**（数据驱动；「备份与恢复」第 9 项） |
| `src/components/settings/backup-section.tsx` | 「备份与恢复」页：导出勾选（三态）+ 凭据开关 + 预估 + 结果区 |
| `src/components/settings/backup-import-preview.tsx` | 导入预览卡：契约版本头 + 逐 scope 计数 + 需重新登录提示 + 冲突策略选择器 |
| `src/stores/backup.ts` | 备份页状态（selection / creds / preview / result / pending） |
| `src/lib/app-settings-snapshot.ts` | **§7 #3**：读写 `appSettings` 的 **localStorage 段**（语言 / 外观 / 紧凑模式 / 上次作用域）；导出取值、导入回写 |
| `src/locales/domains/backup.zh.ts` / `backup.en.ts` | 备份与恢复**功能**词条（zh + en 同步） |

### 修改

| 相对路径 | 改动 |
|:---|:---|
| `crates/buddy-switch-core/src/modules/export_import.rs` | **泛化** `merge_import_record`（`ConflictPolicy` + `TokenPolicy`）；账号级传 `(Overwrite, Require)` 保现状 |
| `crates/buddy-switch-core/src/modules/config.rs` | 自动备份文件名/保留（7 份）辅助（`backup_dir()` 已有） |
| `crates/buddy-switch-core/src/modules/mod.rs` | 挂 `backup` / `backup_sections` |
| `src-tauri/src/commands.rs` + `crates/buddy-switch-server/src/api.rs` | 新命令：`export_backup` / `preview_import_backup` / `import_backup` / `list_auto_backups` / `restore_auto_backup`（**两宿主同步**） |
| `src-tauri/src/lib.rs` + `crates/buddy-switch-server/src/main.rs` | **登记自动备份 `BackgroundTask`**（两宿主各一条）+ `generate_handler!` / `.route` |
| `src/lib/api.ts` | 新命令接线（`call` + ROUTES/ALIASES + `DEMO_READ_COMMANDS`） |
| `src/pages/SettingsPage.tsx` | 长滚动分块 → **9 项子导航**接线 |
| `src/lib/screenshot-demo.ts` | 新 case（备份页演示态） |
| `src/locales/domains/shell.zh.ts` / `shell.en.ts` | 子导航 9 项标签（应用级导航，与 `appSettings` 同域） |
| `src/components/command-palette/*` | 两个动作（「导出全部备份」/「从备份导入」）—— ⚠️ **本期不做**，**推到 P6 之后**（§7 #6：P6 未落 ⇒ 无法接线 ⇒ 死代码）。此处仅登记归属，**不产出代码** |

> **不修改**：账号级 `export-accounts-dialog.tsx` / `import-accounts-dialog.tsx`（**保留不动**，§3.14）；`account.rs` 的 `resolve_account_key`（P5 已统一，§1.4）。
> **不新建** 独立 `sequence-diagram.mermaid` / `class-diagram.mermaid` —— 本文内嵌。

---

## 3. 数据结构与接口

### 3.1 后端：envelope 与契约（`backup.rs`）

```rust
pub const BACKUP_KIND: &str = "buddy-switch-backup";
pub const SUPPORTED_FORMAT_VERSION: u32 = 1;

/// ★ 凭据键唯一来源：导出剥离与验收搜索**共用**（真实键是 snake_case，见 §1.3）。
pub const CREDENTIAL_KEYS: &[&str] = &[
    "access_token", "refresh_token", "accessToken", "refreshToken", "$wbEncrypted", "jwt",
];

/// 导出勾选：每个 scope 的每个分区（三态）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BackupSelection {
    pub scopes: std::collections::BTreeMap<String /*scope key*/, ScopeSelection>,
    pub includes_credentials: bool,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScopeSelection { pub accounts: bool, pub config: bool, pub gateway: bool }

/// 解析 envelope（fail-closed）：kind / formatVersion / scope 键逐个校验。
pub fn parse_envelope(text: &str) -> Result<Value, String>;

/// ★ 版本迁移入口（§7 #8）：本期**只处理 v1**（`v == SUPPORTED_FORMAT_VERSION` 时原样返回）；
/// 真出现 v2 时在 match 里补一支，**不空转**。`parse_envelope` 在 `formatVersion < 当前` 时调它。
pub fn migrate_envelope(value: Value, from: u32) -> Result<Value, String>;

/// 导出：遍历 SCOPES 组装；`includes_credentials=false` 时按 CREDENTIAL_KEYS 剥离。
pub fn export_backup(selection: &BackupSelection) -> Result<Value, String>;

/// 预览（**只读、不写盘**）：逐 scope 统计 + 需重新登录数 + 将被覆盖的配置项。
pub fn preview_import_backup(text: &str, policy: ConflictPolicy) -> Result<Value, String>;

/// 导入：**先快照** → 逐 scope 合并 → 汇总（含失败项，供「重试」）。
pub fn import_backup(text: &str, policy: ConflictPolicy, selection: &BackupSelection) -> Result<Value, String>;

/// 导入前强制快照（D18）。
pub fn snapshot_before_import() -> Result<String, String>;
/// 定时自动备份（D17）：写 backup_dir()、保留 7 份。
pub fn auto_backup() -> Result<String, String>;
pub fn list_auto_backups() -> Result<Value, String>;
pub fn restore_auto_backup(file_name: &str) -> Result<Value, String>;
```

### 3.2 后端：分区表（`backup_sections.rs`，表驱动）

```rust
/// 一个可备份分区：id / 读 / 合并。**加类型 = 加一行**（§4.2）。
pub struct BackupSectionSpec {
    pub id: &'static str,   // "accounts" | "config" | "gateway"
    pub read: fn(scope: Scope) -> Result<Value, String>,
    pub merge: fn(local: &mut Value, incoming: &Value, policy: ConflictPolicy) -> SectionCounts,
}

/// 表本体（唯一）：导出面板与导入合并都从它读，**不各写一份**。
pub fn backup_sections() -> &'static [BackupSectionSpec];
```

### 3.3 后端：合并泛化（`export_import.rs`）

```rust
pub enum ConflictPolicy { KeepLocal, Overwrite, KeepBoth, Ask }
pub enum TokenPolicy { Require, Allow }
pub enum MergeOutcome { Appended, Overwritten, KeptLocal, KeptBoth, Skipped }

/// ★ 唯一合并出口：账号级 `(Overwrite, Require)`（现状不变）；全局 `(policy, Allow)`。
pub fn merge_import_record(
    accounts: &mut Vec<Value>, item: &Value,
    conflict: ConflictPolicy, tokens: TokenPolicy,
) -> MergeOutcome;
```

### 3.4 后端：命令（两宿主同步）

```rust
#[tauri::command] pub async fn export_backup(selection: Value) -> Result<Value, String>;
#[tauri::command] pub async fn preview_import_backup(file_text: String, policy: String) -> Result<Value, String>;
#[tauri::command] pub async fn import_backup(file_text: String, policy: String, selection: Value) -> Result<Value, String>;
#[tauri::command] pub async fn list_auto_backups() -> Result<Value, String>;
#[tauri::command] pub async fn restore_auto_backup(file_name: String) -> Result<Value, String>;
```

### 3.5 前端：视图类型（`src/lib/types.ts` 追加）

```ts
import type { Scope } from "@/lib/scope";

/** 导出勾选（三态：scope 头 + 账号/配置/网关三个分区）。 */
export interface BackupSelectionState {
  scopes: Record<Scope, { accounts: boolean; config: boolean; gateway: boolean }>;
  includesCredentials: boolean;
}

/** 预估（**由实际数据推导**，§3.10 第 1 条：不写死数字）。 */
export interface BackupEstimate { bytes: number; accounts: number; scopes: number; includesCredentials: boolean }

/** 导入预览的逐 scope 行。 */
export interface ImportPreviewScopeRow { scope: Scope; added: number; overwritten: number; skipped: number; conflicts: number }

/** 导入预览（**不落盘**；供安全阀展示）。 */
export interface ImportPreview {
  formatVersion: number;
  exportedAt: string;
  includesCredentials: boolean;
  scopes: ImportPreviewScopeRow[];
  /** 将被覆盖的配置项（签到 / 旅行 / 轮换 / 切换）。 */
  configOverwrites: string[];
  /** 不含凭据 ⇒ 需重新登录的账号数（断言）。 */
  needRelogin: number;
}

/** 导入结果（部分失败态：失败计数 + 可重试）。 */
export interface ImportSummary {
  perScope: ImportPreviewScopeRow[];
  failed: number;
  retryable: number;
}
```

### 3.6 前端：备份页（`backup-section.tsx`）

```ts
export interface BackupSectionProps {
  /** 逐 scope 的账号/配置/网关计数（用于分区勾选旁的数字与预估）。 */
  scopeStats: Record<Scope, { accounts: number; config: number; gateway: number }>;
}
```

**约束**：

- **三态勾选**：scope 头 + 分区勾选 ⇒ `aria-checked="mixed"`（半选语义，§3.11）；**全不勾 ⇒ 导出按钮 `disabled`**（不能导出空备份）。
- **凭据开关默认关**（D15）；打开 ⇒ **红色警告条** + 主按钮文案变「导出（含凭据）」+ **二次确认**；关闭 ⇒ 警告消失、预估回落「不含凭据」。
- **预估行**由**实际勾选**推导（`备份大小 / N 个账号 / M 个作用域 / 含|不含凭据`）——**不是写死的装饰数字**（§3.10 第 1 条）。
- **导出按钮**：桌面端走保存对话框（复用 `export-accounts-dialog` 的 `revealItemInDir` 模式）；webui 走 Blob 下载。
- 展示字段（`scope` 标签 / 账号展示名）**一律过 `displayText`**（白屏防线，issue #2）。
- 状态语言（§3.6）：导出/导入**进行中** → 按钮内 `<Loader2>`（**不是加载态**）；区块首次加载 → `<Skeleton>`；空态 → `<EmptyState>`（四要素含**出口**）。

### 3.7 类图

```mermaid
classDiagram
  class BackupSelection {
    +scopes: Map~Scope, ScopeSelection~
    +includesCredentials: bool
  }
  class ScopeSelection {
    +accounts: bool
    +config: bool
    +gateway: bool
  }
  class BackupSectionSpec {
    +id: string
    +read(scope) Value
    +merge(local, incoming, policy) Counts
  }
  class MergePolicy {
    <<enum>>
    ConflictPolicy
    TokenPolicy
  }
  class MergeOutcome {
    <<enum>>
    Appended
    Overwritten
    KeptLocal
    KeptBoth
    Skipped
  }
  class BackupApi {
    +export_backup(selection)
    +preview_import_backup(text, policy)
    +import_backup(text, policy, selection)
    +snapshot_before_import()
    +auto_backup()
    +list_auto_backups()
    +restore_auto_backup(name)
  }
  class BackupSectionView {
    +scopeStats
  }
  class ImportPreviewView {
    +formatVersion
    +needRelogin
    +configOverwrites
  }
  class BackupStore {
    +selection
    +creds
    +preview
    +result
  }

  BackupSelection --> ScopeSelection
  BackupApi ..> BackupSelection
  BackupApi ..> MergePolicy
  BackupApi ..> BackupSectionSpec
  BackupApi ..> MergeOutcome
  BackupSectionView ..> BackupStore
  ImportPreviewView ..> BackupStore
  BackupSectionView ..> BackupApi
```

### 3.8 调用流程（导出）

```mermaid
sequenceDiagram
  participant U as 用户
  participant BS as BackupSection
  participant Store as BackupStore
  participant BE as 后端
  participant FS as 文件系统

  U->>BS: 打开「备份与恢复」
  BS->>BE: 读各 scope 计数（账号/配置/网关）
  BE-->>BS: scopeStats（驱动勾选与预估）
  Note over BS: 默认全选 4 作用域（D14）、凭据默认关（D15）
  U->>BS: 勾选分区 / 打开凭据开关
  BS->>BS: 预估实时重算（不写死）
  U->>BS: 点「导出为文件…」
  BS->>BE: export_backup(selection)
  BE->>BE: 遍历 SCOPES 组装 envelope
  alt includesCredentials=false
    BE->>BE: 按 CREDENTIAL_KEYS 逐条剥离 token 键
  end
  BE->>FS: 写文件 / 返回 JSON
  BE-->>BS: 文件路径 或 JSON
  BS-->>U: 成功 + 「在文件管理器中显示」
```

### 3.9 调用流程（导入：预览 → 快照 → 合并 → 结果）

```mermaid
sequenceDiagram
  participant U as 用户
  participant PV as ImportPreview
  participant BE as 后端
  participant Snap as 快照
  participant Merge as merge_import_record

  U->>PV: 选择备份文件
  PV->>BE: preview_import_backup(text, policy)
  BE->>BE: parse_envelope（kind/formatVersion/scope fail-closed）
  alt 未知 scope
    BE-->>PV: 该分区拒绝并报错（不静默跳过）
  end
  BE-->>PV: 逐 scope 计数 + 需重新登录数 + 覆盖配置项
  Note over PV: ★ 预览不写盘（安全阀）
  U->>PV: 选冲突策略（默认保留本地 D16）→「开始导入」
  PV->>BE: import_backup(text, policy, selection)
  BE->>Snap: snapshot_before_import()（强制 D18）
  Snap-->>BE: 快照路径（可「从自动备份恢复」）
  loop 每个 scope × 分区
    BE->>Merge: merge_import_record(..., policy, Allow)
    Merge-->>BE: Appended/Overwritten/KeptLocal/KeptBoth/Skipped
  end
  BE-->>PV: ImportSummary（含 failed/retryable）
  PV-->>U: 部分失败态（汇总 + 重试失败项 + 明细）
```

---

## 4. 任务列表（有序 · 含依赖 · 按实现顺序）

> **硬性顺序**：T01 → T02；T03 在 **T01 命令接口冻结后**即可起（与 T02 部分并行）；T04 收尾。
> **P0 = 必须；P1 = 重要；P2 = 可裁剪。**

### T01 · 后端：envelope 契约 + 分区表 + 导出（凭据剥离）+ 命令 ｜ P0
- **源文件**
  - 新建 `crates/buddy-switch-core/src/modules/backup.rs`、`backup_sections.rs`；改 `modules/mod.rs`
  - 修改 `crates/buddy-switch-core/src/modules/export_import.rs`（泛化 `merge_import_record` 的签名，T02 用）
  - 修改 `src-tauri/src/commands.rs` + `crates/buddy-switch-server/src/api.rs` + `src-tauri/src/lib.rs`（`export_backup`，两宿主）
  - 修改 `src/lib/api.ts`（`export_backup` 接线）
- **依赖**：**P5**（`Scope::parse`）；可与 P5 并行（接口已冻结）
- **要点**
  - **`CREDENTIAL_KEYS` 用真实键**（snake_case，§1.3）；`includesCredentials=false` 时**逐条剥离**、**不写 null**。
  - **遍历 `SCOPES`** 组装 ⇒ 4/4 覆盖（§1.11）；分区表驱动（加类型 = 加一行）。
  - **`kind` / `formatVersion` 校验**（fail-closed）；`> 上限` 拒绝 + 提示升级（`migrate_envelope` 预留，本期只处理 v1，§7 #8）。
  - **账号级导入行为零变化**：`merge_import_record` 账号级仍 `(Overwrite, Require)`。
  - **⚠️ 不重复修 P5 已修的**（§7 #4）：账号**定位**统一谓词 `resolve_account_key`（`id` **或** `uid`，`account.rs:48-51`）**已由 P5 落地** ⇒ T01 **只调用、不改它**；「删 `id` / 查 `uid` 不一致」**不在 P7 范围内**（再修一遍 = 重复劳动 + 两套谓词漂移风险）。

### T02 · 后端：导入（预览/冲突/幂等/未知 scope）+ 自动快照 + 定时备份 ｜ P0
- **源文件**
  - 修改 `crates/buddy-switch-core/src/modules/backup.rs`（`parse_envelope` / `preview_import_backup` / `import_backup` / `snapshot_before_import` / `auto_backup` / `restore_auto_backup`）
  - 修改 `crates/buddy-switch-core/src/modules/config.rs`（自动备份命名 / 保留 7 份）
  - 修改 `src-tauri/src/commands.rs` + `crates/buddy-switch-server/src/api.rs` + `src-tauri/src/lib.rs`（4 个命令，两宿主）
  - 修改 `src-tauri/src/lib.rs` + `crates/buddy-switch-server/src/main.rs`（**登记自动备份 `BackgroundTask`**，两宿主）
  - 修改 `src/lib/api.ts`
- **依赖**：T01
- **要点**
  - **预览不写盘**（安全阀）；逐 scope 计数 + 需重新登录数 + 覆盖配置项。
  - **幂等**：默认 `KeepLocal`（D16）⇒ 连导两次第二次全「跳过」（§1.5）。
  - **未知 scope fail-closed**：`Scope::parse` `Err` ⇒ 该分区拒绝并报错，**绝不** `unwrap_or`（§1.6）。
  - **冲突策略**：`ConflictPolicy` 枚举含 `Ask`，但**本期只做后端枚举、不做逐条 UI**（§7 #5）——三种批量策略（保留本地 / 覆盖 / 保留两者）够用；`Ask` 无 UI 入口即用户侧恒取 `KeepLocal`。
  - **导入前强制快照**（D18）+「从自动备份恢复」入口。
  - **定时备份**（D17）：每日一次、保留 7 份、**两宿主登记** `BackgroundTask`（P5 R9）；**默认开 + 存 `appSettings`（§7 #7）**；⚠️ **必须提供「关掉」开关**（关闭后不再产生新备份，已产生的保留）。
  - 结果走 §3.6 **部分失败态**（失败计数 + 可重试）。

### T03 · 前端：设置 9 项子导航 + 备份与恢复页 + 接线 + 词条 ｜ P0
- **源文件**
  - 新建 `src/components/settings/settings-subnav.tsx`、`backup-section.tsx`、`backup-import-preview.tsx`、`src/stores/backup.ts`、`src/lib/app-settings-snapshot.ts`
  - 修改 `src/pages/SettingsPage.tsx`（9 项子导航接线）
  - 新增 `src/lib/app-settings-snapshot.ts`（**§7 #3：读写 appSettings localStorage 段**——语言 `i18n.ts` / 外观 `theme.ts` / 紧凑模式 / 上次作用域；导出取值、导入回写）
  - 修改 `src/lib/screenshot-demo.ts`（备份页 case）
  - 修改 `src/locales/domains/shell.zh.ts` / `.en.ts`（子导航 9 项）+ 新建 `backup.zh.ts` / `.en.ts`
- **依赖**：T01（命令接口冻结）；T02（导入命令）
- **要点**
  - **9 项子导航**：`通用 / 账号与登录 / 签到与积分 / 网关 / 计划任务 / 客户端 / 权限与诊断 / 日志 / 备份与恢复`（第 9 项）。
  - **导出**：默认全选 4 作用域（D14）、凭据默认关（D15）、三态 `aria-checked="mixed"`、预估**由实际勾选推导**、全不勾 ⇒ 导出 `disabled`。
  - **凭据开关**：打开 ⇒ 红警告 + 主按钮文案变 + 二次确认；关闭 ⇒ 回落。
  - **导入**：先预览后落盘；预览显示 `formatVersion`；不含凭据 ⇒「需重新登录」；结果**部分失败态**。
  - **冲突策略选择器**：**只给三选项**（保留本地 / 覆盖 / 保留两者，§7 #5）；**不渲染**「逐条询问」（`Ask` 后端枚举保留、UI 裁剪）。
  - **`appSettings` 段（§7 #3）**：**localStorage 段由前端负责**——导出时前端把语言 / 外观 / 紧凑模式 / 上次作用域**传入**，导入时**回写** localStorage；**开机自启（OS）与更新代理（后端 `githubConfig`）从各自来源读**。**⚠️ 更新代理 URL 落盘前按 `CREDENTIAL_KEYS` 检查**（可能含 `user:pass@`）。
  - **命令面板两动作**：**本期不做**（§7 #6，推到 P6 之后）；T03 **不产出**命令面板代码。
  - 展示字段过 `displayText`；状态语言（`<Loader2>` / `<Skeleton>` / `<EmptyState>`）；`§3.4 规则 1`（无数据不渲染）。
  - 词条 **zh + en 同步**，**只留被引用的键**。

### T04 · 验证与门禁 ｜ P0
- **源文件**：无源码改动（跑门禁 + 变异验证 + 手工回归）
- **依赖**：T01、T02、T03
- **要点**
  - `npm run check:api` / `check:scope` / `check:api-contract` / `tsc` 全绿；`cargo test`（core）。
  - **★ 修正验收判据**：把 PLAN §6 / P5-P12-TASKS 的 `accessToken`/`refreshToken`（camelCase）改成 **`CREDENTIAL_KEYS`（含 snake_case 真实键）**（§1.3 / §7 #1）。
  - **变异验证（可证伪）**：
    1. **凭据泄露**：让导出在 `includesCredentials=false` 时**不剥离** `access_token` ⇒ 期望「文件文本搜 `access_token` ⇒ 0 命中」断言变红。
    2. **假判据（对照）**：把搜索键改成 camelCase ⇒ 期望**原断言无法发现泄露**（证明旧判据是假阴性）。
    3. **幂等**：把默认策略改成 `KeepBoth` ⇒ 期望「连导两次第二次全跳过」断言变红。
    4. **未知 scope**：构造含 `foo-cn` 的文件 ⇒ 期望该分区**整体拒绝并报错**（非静默跳过）断言变红（当它静默跳过时）。
    5. **覆盖率**：只导出当前 scope ⇒ 期望「4/4 作用域」断言变红。
    6. **往返一致**：导出 → 导入 → 再导出（忽略时间戳）⇒ 语义一致。
    7. **快照强制**：绕过 `snapshot_before_import` ⇒ 期望「导入前产生快照」断言变红。
  - 手工回归：导出（含/不含凭据）→ 换目录导入 → 账号「需重新登录」呈现；未知 scope 文件报错；「从自动备份恢复」可用。

---

## 5. 共享知识（跨文件约定）

- **★ 备份文件不得含凭据**（§2.6 规则 3）：`includesCredentials=false` 时**完全不出现** token 键（**不是 null**）；判据 = 搜 `CREDENTIAL_KEYS`（**真实 snake_case 键**）⇒ 0 命中。
- **`CREDENTIAL_KEYS` 是唯一来源**：导出剥离与验收搜索**共用**；**禁止**用 camelCase 当搜索键（假阴性）。
- **`formatVersion` 是唯一兼容依据**（fail-closed）：`> 上限` 拒绝 + 提示升级；缺 / 不匹配 `kind` ⇒ 拒绝。
- **按 scope 分组 + 遍历 `SCOPES`**：4/4 覆盖；加平台自动覆盖，代码零改动。
- **未知 scope fail-closed**：`Scope::parse` `Err` ⇒ 分区整体拒绝并报错，**绝不** `unwrap_or`（与 P5 同口径）。
- **导入按 uid 去重**；账号**定位**走 P5 的 `resolve_account_key`（`id` 或 `uid`）——**禁止**新写「只匹配 id」。**★ §7 #4**：该谓词**已由 P5 落地**，P7 **只调用、不重复修**（再修 = 两套谓词漂移）。
- **合并只有一处**：`merge_import_record`（加 `ConflictPolicy` + `TokenPolicy`）；**禁止**为全局备份新写一份合并。
- **账号级行为零变化**：账号级调 `(Overwrite, Require)`；账号级对话框**保留不动**（§3.14 并存不替换）。
- **默认值**：全选作用域（D14）、凭据关（D15）、冲突保留本地（D16）—— **安全默认**。
- **导入前强制快照**（D18，不可关）：UI 才敢承诺「可撤销」；**不许**承诺后端做不到的撤销（§2.5 第 3 条）。
- **定时备份登记 `BackgroundTask`（两宿主）**：不登记 ⇒ 生产上永不执行（P5 R9）。**★ §7 #7**：**默认开 + 存 `appSettings`，且必须能关掉**（关闭后不再产生新备份）。
- **`appSettings` 责任边界（§7 #3）**：**localStorage 段**（语言 / 外观 / 紧凑模式 / 上次作用域）**由前端负责**；开机自启（OS）/ 更新代理（后端 `githubConfig`）从各自来源读。**⚠️ 更新代理 URL 落盘前按 `CREDENTIAL_KEYS` 检查**（可能含 `user:pass@`）。
- **命令面板两动作本期不做（§7 #6）**：推到 **P6 之后**（P6 未落 ⇒ 登记即死代码）。
- **不写死数字**（§3.10 第 1 条）：预估大小 / 账号数 / 作用域数**由实际数据推导**。
- **文案不许假承诺**（§3.5 红线）：导入必须给**结果明细**（部分失败态），**不许**只 toast「导入完成」。**★ 凭据文案（§7 #2）**：**不得**写「备份完整」；必须写「**凭据不含在内，导入后需重新登录**」（zh + en 同步）。
- **展示字段出口归一为「字符串或 null」**：scope 标签 / 账号展示名一律过 `displayText`（对象当 React 子节点 ⇒ 白屏，issue #2）。
- **状态语言（§3.6）**：导出/导入**进行中** → 按钮内 `<Loader2>`（**不是加载态**）；区块首次加载 → `<Skeleton>`；空态 → `<EmptyState>`（四要素含**出口**）。
- **无数据不渲染**（§3.4 规则 1）：没有可导出内容时不渲染预估/导出入口。
- **i18n**：zh + en **同时**加；扁平点分命名，**只保留被引用的键**；应用级（子导航）放 `shell`，备份功能放 `backup`。
- **命令面两宿主同步**：新命令在 `commands.rs`（Tauri）与 `api.rs`（server）**各注册一次**，并接 `api.ts` ROUTES/ALIASES（P5 纪律）。

---

## 6. 风险点

| # | 风险 | 后果 | 对策 |
|:--|:---|:---|:---|
| **R1** | **验收判据用错键（camelCase）** —— 现 PLAN 写 `accessToken` | 搜 camelCase **恒 0 命中** ⇒ **假阴性**，token 泄露测不出 | 判据改用 `CREDENTIAL_KEYS`（**真实 snake_case** + 信封标记）；T04 变异验证 ①② 双向兜住 |
| **R2** | **导出漏剥凭据** | 明文 token 进文件 ⇒ 泄露即可盗号 | 白名单剔除 + 搜 `access_token` ⇒ 0 命中；变异验证 ① 兜住 |
| **R3** | **无 token 备份导不进** | 「需重新登录」场景**整条失效**（合并把无 token 全跳过） | `TokenPolicy::Allow`（全局）+ `Require`（账号级）（§1.4） |
| **R4** | **合并重写一份** | 两套合并漂移（本仓反复踩过） | **唯一** `merge_import_record`（加策略参数），禁止新写 |
| **R5** | **幂等被破坏**（默认 `KeepBoth` 或覆盖） | 连导两次产生重复账号 | 默认 `KeepLocal`（D16）；变异验证 ③ 兜住 |
| **R6** | **未知 scope 静默跳过 / 回落默认** | 数据被塞进错误的 scope（跨库串数据） | `Scope::parse` fail-closed；变异验证 ④ 兜住 |
| **R7** | **导入无快照** | 导入出错**不可撤销**，UI 却承诺可撤销 | 强制 `snapshot_before_import`（D18）；变异验证 ⑦ 兜住 |
| **R8** | **定时备份未登记** | 任务生产上永不执行（「调用点静默消失」） | 两宿主登记 `BackgroundTask`（P5 R9） |
| **R9** | **预估写死** | 数字是装饰，与实际不符（§3.10 第 1 条） | 由实际勾选推导；变异：改勾选数字必须变 |
| **R10** | **导出空备份** | 空文件被当成「备份」 | 全不勾 ⇒ 导出 `disabled` |
| **R11** | **信封被当「已带凭据」** | 换机后解不开 ⇒ 用户以为能直接用 | 信封一律剥离 + 如实告知「需重新登录」（§1.3）；**文案不得写「备份完整」，写「凭据不含在内，导入后需重新登录」**（§7 #2） |
| **R12** | **账号级导入被顺手改坏** | 破坏 P0-1「CN 行为零变化」 | 账号级仍 `(Overwrite, Require)`；账号级对话框保留不动 |
| **R13** | **`appSettings` 来源混淆**（误以为全在 localStorage） | 导出缺 appSettings 或导出脏值 | §7 #3 **已决策**：localStorage 段前端负责、OS/后端子项各自来源读；**更新代理 URL 可能含 `user:pass@` ⇒ 按 `CREDENTIAL_KEYS` 剥离**；展示前过 `displayText` |

---

## 7. 决策记录（**8 条待明确已决策 · lead 拍板 2026-10-04**）

> 每条给出「原问题 → ✅ 决策 → 理由」。**正文已同步**（同步清单见 §7.1）。

1. **★ 验收判据的键集** → ✅ **改成 `CREDENTIAL_KEYS`**（含 `access_token` / `refresh_token` / `accessToken` / `refreshToken` / `$wbEncrypted` / **`jwt`** —— **共 6 个**），**导出剥离与验收搜索共用同一常量**。
   ⚠️ **更正（P7 T04 独立验证发现）**：初版设计此处只列 **5 个键**，**漏了 Trae 的 `jwt`**；实际实现 `backup.rs::CREDENTIAL_KEYS` 是 **6 个键**（含 `jwt`）。**以实现为准** —— Trae 账号凭据键就是 `jwt`，判据漏它 ⇒ Trae 换机备份的 `jwt` 泄露测不出。
   理由：现判据写 `accessToken`（camelCase），而账号库真实键是 `access_token`（snake_case）⇒ 搜 camelCase **恒 0 命中** ⇒ **即便 token 明文躺在文件里也测不出** ⇒ 这条安全红线**目前是「假断言」**。**最可怕的是它看起来是绿的**——没人会去怀疑一条「通过了」的安全断言。
   ⚠️ **T04 的双向变异验证必须做**：① 让导出漏剥 ⇒ 断言**必须红**；② **把搜索键换回 camelCase ⇒ 证明旧判据抓不到**（②是在**证明原判据无效**，不只是「新判据有效」）。

2. **加密信封（`$wbEncrypted`）如何处理？** → ✅ **一律剥离 + 如实告知**（即使 `includesCredentials=true`）。
   理由：信封密钥在**客户端 OS 安全存储**、**换机解不开** ⇒ 导出**无实用价值**。
   ⚠️ **「如实告知」要落到文案上**：**不得**写「备份完整」，必须写「**凭据不含在内，导入后需重新登录**」。

3. **`appSettings` 是否纳入 / 存哪？** → ✅ **纳入；由前端负责 localStorage 段**。
   理由：换机时用户配置该带走。⚠️ **已核实该段并非全在 localStorage**（`app-settings.tsx`）：**语言 / 外观 / 紧凑模式 / 上次作用域在前端 localStorage**（Rust 侧无 settings 文件）⇒ **前端负责**（导出传值、导入回写）；**开机自启在 OS、更新代理在后端 `githubConfig`** ⇒ 从各自来源读。
   ⚠️ **须确认各子项无敏感数据**；**尤其更新代理 URL 可能含 `user:pass@`（代理认证）** ⇒ 命中则按 #1 同一套 `CREDENTIAL_KEYS` 判据剥离。

4. **「删 `id` / 查 `uid` 不一致」是否还需 P7 处理？** → ✅ **P5 已修，P7 不重复修，只依赖 P5**。
   理由：`resolve_account_key`（`id` **或** `uid`）已是**唯一谓词出口**（`account.rs:48-51`），`find`/`delete`/`set_remark` 都调它 ⇒ 再修一遍就是**重复劳动 + 两套谓词漂移风险**。

5. **冲突策略「逐条询问」（`Ask`）本期做吗？** → ✅ **本期做后端枚举、UI 裁剪**（不做逐条 UI；默认 `KeepLocal`）。
   理由：`Ask` 需逐条 UI、成本高；三种批量策略（保留本地 / 覆盖 / 保留两者）已覆盖主场景。枚举先留着，UI 后续可补。

6. **命令面板两动作归属** → ✅ **推到 P6 之后**（P7 **不**做面板入口）。
   理由：P6（命令面板本体）未落，现在登记动作**无法接线** ⇒ 那是**死代码**。

7. **定时备份默认值 + 存哪** → ✅ **默认开 + 存应用设置**；⚠️ **必须能在设置里关掉**（用户控制）。
   理由：D17 的理由是「备份价值在于出事时有」⇒ 默认开；但**用户必须能关**（不能只有「默认开」而无法控制）。

8. **`formatVersion` 迁移函数本期需要吗？** → ✅ **预留入口，本期只处理 v1**。
   理由：当前只有 v1、**无历史版本可迁**；预留 `migrate_envelope(v)` 入口即可，真出现 v2 再填（避免空转）。

### 7.1 同步清单（正文改在哪）

| 决策 | 正文改动点 |
|:--|:---|
| **#1** | §1.3（`CREDENTIAL_KEYS` 常量 + 判据 + 双向变异）、§5、§6 R1、§4 T04（变异验证 ①②） |
| **#2** | §1.3（信封段 + 文案要求）、§5、§6 R11 |
| **#3** | §1.2（`appSettings` 来源表 + 更新代理 URL）、§2（新增 `app-settings-snapshot.ts`）、§4 T03、§5、§6 R13 |
| **#4** | §1.4（依赖 P5 谓词）、§1.10、§4 T01 要点、§5 |
| **#5** | §1.4（`ConflictPolicy` · `Ask` UI 裁剪）、§1.7（冲突选择器三选项）、§4 T02/T03 |
| **#6** | 头部「范围」、§2（命令面板行标「本期不做」）、§4 T03 要点、§5 |
| **#7** | §1.8（默认开 + 可关）、§4 T02、§5 |
| **#8** | §1.2（规则 2）、§3.1（`migrate_envelope`） |

---

## 8. 任务依赖图

```mermaid
graph TD
  P5["P5（Scope::parse / resolve_account_key / BackgroundTask）<br/>进行中"] -.-> T01["T01 后端：envelope 契约 + 分区表 + 导出 + 命令<br/>(backup.rs / backup_sections.rs / export_import.rs / 两宿主命令 / api.ts)"]
  T01 --> T02["T02 后端：导入 + 快照 + 定时备份<br/>(preview / import / snapshot / auto_backup / BackgroundTask ×2)"]
  T01 --> T03["T03 前端：9 项子导航 + 备份页 + 接线 + 词条<br/>(settings-subnav / backup-section / preview / store / api.ts / screenshot-demo / locales)"]
  T02 --> T03
  T02 --> T04["T04 验证与门禁<br/>(check:api / check:scope / check:api-contract / tsc / cargo test / ★ 修正判据键集 / 变异验证)"]
  T03 --> T04

  classDef front fill:#e8f5e9,stroke:#2e7d32;
  classDef back fill:#fff3e0,stroke:#e65100;
  classDef gate fill:#e3f2fd,stroke:#1565c0;
  class T03 front;
  class T01,T02 back;
  class T04 gate;
```

> **可先做**：**T01**（导出 + 命令，与 P5 无强耦合）。
> **核心路径**：T01 → T02（**导入 + 快照**是「可撤销」的成立前提）。
> **★ 前置修正**：T04 先按 §7 #1 把验收判据的键集改成 `CREDENTIAL_KEYS`（真实 snake_case），否则「0 命中」断言是**假阴性**。
