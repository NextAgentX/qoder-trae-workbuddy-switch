# docs/ 文档地图

**这里是文档的唯一入口。** 找不到东西时先看这张表，不要靠翻目录。

`docs/` 按**生命周期**分区，不按文件类型：**会跟着代码一起改的**放 `specs/`，**改完就封存的**放 `history/`，**给使用者的**放 `guides/`。

| 目录 | 里面是什么 | 会不会更新 |
|:--|:--|:--|
| [`guides/`](guides/) | 怎么装、怎么开发、客户端怎么接网关 | 跟着版本更新 |
| [`policies/`](policies/) | 许可与免责声明（README 引用它们） | 极少 |
| [`specs/`](specs/) | 现行架构与决策：改代码前该读的依据 | **随代码一起改** |
| [`history/`](history/) | 已完结的一次性报告：审计、探针、外部调研、旧方案 | **只读，不再改** |
| [`prototype/`](prototype/) | 可点原型 + JSDOM 校验套件（门禁读这里的文件） | 新增交互先进这里 |
| [`releases/`](releases/) | 每版人工措辞的发布说明（`gen-release-notes.mjs` 约定路径） | 每版一篇 |
| [`images/`](images/) | README 与文档用的截图（浅/深两套）；`brand/logo.png` 是**图标生成源图**，`scripts/gen-icons.mjs` 默认读它 | 换 UI / 换 logo 时更新 |

---

## 按任务找文档

| 我要做的事 | 先读 |
|:--|:--|
| Linux 桌面「创建密钥」弹窗打不开 / 用浏览器管理 | [`guides/LINUX-WEBUI-FALLBACK.md`](guides/LINUX-WEBUI-FALLBACK.md) |
| 本地跑起来 / 出安装包 / 发版 | [`guides/DEVELOPMENT.md`](guides/DEVELOPMENT.md) |
| 用 Docker 部署服务端 | [`guides/DOCKER.md`](guides/DOCKER.md) |
| 服务器上没有客户端，怎么登录 / 能不能开网关 | [`specs/CLIENTLESS-AUTH-PLAN.md`](specs/CLIENTLESS-AUTH-PLAN.md) |
| 把 Codex / Claude Code / 其它客户端接到网关 | [`guides/GATEWAY-CLIENT-INTEGRATION.md`](guides/GATEWAY-CLIENT-INTEGRATION.md) |
| 改网关（路由 / 换号 / 错误信封 / 命名空间） | [`specs/UNIFIED-GATEWAY-ARCH.md`](specs/UNIFIED-GATEWAY-ARCH.md) + [`specs/P11-ARCH.md`](specs/P11-ARCH.md) + [`specs/P12-ARCH.md`](specs/P12-ARCH.md) |
| 账号在回复中途挂掉 / 想「静默换号」（issue #129） | [`specs/ACCOUNT-FAILOVER.md`](specs/ACCOUNT-FAILOVER.md)（对外错误形状契约 + 关闭态不变量 + 五条治理链路） |
| 改前端某个页面 | [`specs/PLAN.md`](specs/PLAN.md) 对应 §3.x + 该期 `P*-ARCH.md` |
| 给密钥设 token 配额 / 限制可访问账号（issue #76） | [`specs/KEY-PERMISSION-UI-DESIGN.md`](specs/KEY-PERMISSION-UI-DESIGN.md)（**已实现**：四条产品线的台账 / 命令 / 前端表 / 中继全部接通）+ [`prototype/key-permission-design.html`](prototype/key-permission-design.html)（可点设计稿） |
| 侧栏太占地方 / 想只看图标（折叠） | [`specs/SIDEBAR-COLLAPSE-DESIGN.md`](specs/SIDEBAR-COLLAPSE-DESIGN.md)（已落地）+ [`prototype/verify-sidebar-collapse.cjs`](prototype/verify-sidebar-collapse.cjs)（行为校验） |
| 侧栏品牌块图标与顶栏平台图标看混了（issue #75） | [`prototype/sidebar-brand-design.html`](prototype/sidebar-brand-design.html)（**已按方案 1 落地**：品牌块只留文字、折叠态整块隐藏，`AppIconMark` 已删） |
| 用量页要显示网关调用的明细（按模型 / 按账号） | [`specs/USAGE-GATEWAY-UI-REDESIGN.md`](specs/USAGE-GATEWAY-UI-REDESIGN.md)（交互与视觉，待拍板）+ [`specs/USAGE-GATEWAY-DETAIL-PLAN.md`](specs/USAGE-GATEWAY-DETAIL-PLAN.md)（数据契约与验收） |
| 新增一个平台 / 产品线 | [`specs/P5-ARCH.md`](specs/P5-ARCH.md) + [`specs/UNIFIED-GATEWAY-ARCH.md`](specs/UNIFIED-GATEWAY-ARCH.md) §11 |
| 用词拿不准（region / scope / 作用域…） | [`specs/VOCABULARY.md`](specs/VOCABULARY.md)（**冻结，不许自创**） |
| 想知道某功能当初为什么这么做 | [`specs/DECISIONS.md`](specs/DECISIONS.md)（D1–D46） |
| 加新交互（先原型、后代码） | [`prototype/`](prototype/) → `index.html` 可直接打开 |
| 账号页「客户端限流」怎么和网关「限流探测」联动（同一份数据 + 两处入口） | [`prototype/limit-link-proto.html`](prototype/limit-link-proto.html)（可点设计稿，**已落地**）+ [`prototype/verify-limit-link.mjs`](prototype/verify-limit-link.mjs)（34 条行为校验）+ [`specs/MODEL-LIMIT-PROBE-PLAN.md`](specs/MODEL-LIMIT-PROBE-PLAN.md) §5.9（口径） |
| 查这版改了什么 | [`releases/`](releases/) |

---

## `specs/` — 现行架构与决策

这些是**活的**文档：代码里大量注释直接引它们作为设计依据（`P5-ARCH.md §2.3` 这种引用是真的会去查的）。**改行为时同步改这里，别让它变成谎话。**

### 总纲

| 文档 | 内容 | 状态 |
|:--|:--|:--|
| [`PLAN.md`](specs/PLAN.md) | 架构与交互重构总方案 v2.8（P0–P16 分期表、交互规范、Playbook） | 现行 · P0–P16 已落地 |
| [`DECISIONS.md`](specs/DECISIONS.md) | 决策清单 D1–D46，逐条有结论 | 现行 · 已拍板 |
| [`VOCABULARY.md`](specs/VOCABULARY.md) | 词汇表（P0 冻结）。**新代码只许用「新词汇」** | 现行 · 冻结 |
| [`COVERAGE-REPORT.md`](specs/COVERAGE-REPORT.md) | 原型 112 条行为 vs 真实应用的覆盖度实测 | 现行 · 随原型更新 |
| [`P5-P12-TASKS.md`](specs/P5-P12-TASKS.md) | P5–P12 的执行级任务清单 | 现行 |
| [`UPDATE-NOTES-ARCH.md`](specs/UPDATE-NOTES-ARCH.md) | 升级前说明弹窗（版本内容）：四阶段交互、数据契约、发布侧产出与切片口径 | 现行 · 2026-10-17 落地 |

### 分期架构（P3–P15）

| 文档 | 内容 |
|:--|:--|
| [`P3-ARCH.md`](specs/P3-ARCH.md) | 用量图表四视图 + 概览今日用量摘要 |
| [`P4-ARCH.md`](specs/P4-ARCH.md) | 账号详情抽屉 + 卡片瘦身 + 程序位常显 |
| [`P4-MIGRATION-ARCH.md`](specs/P4-MIGRATION-ARCH.md) | 跨作用域迁移向导（三步抽屉） |
| [`P4-TOOLBAR-ARCH.md`](specs/P4-TOOLBAR-ARCH.md) | 搜索 / 排序 / 分组 / 卡片·表格视图 |
| [`P5-ARCH.md`](specs/P5-ARCH.md) | 后端收敛：`ProductSpec` + 命令面参数化 + 网关宿主泛化 |
| [`P6-ARCH.md`](specs/P6-ARCH.md) | 加速器：⌘K 命令面板 / 动态快捷键 / 撤销 toast / 首次引导 |
| [`P7-ARCH.md`](specs/P7-ARCH.md) | 全局备份与恢复（envelope 契约） |
| [`P8-ARCH.md`](specs/P8-ARCH.md) | 会话工作台（树 / 关联图 / 状态筛选） |
| [`P9-ARCH.md`](specs/P9-ARCH.md) | 桌面悬浮窗（独立窗口 + 托盘 + 只读订阅） |
| [`P9-KEYBOARD-CHECKLIST.md`](specs/P9-KEYBOARD-CHECKLIST.md) | 悬浮窗键盘可达性手测清单（独立窗口不能复用主窗口） |
| [`P10-ARCH.md`](specs/P10-ARCH.md) | 网关接入与模型自动识别（三级来源 / 三陷阱） |
| [`P11-ARCH.md`](specs/P11-ARCH.md) | 模型禁用与接入说明；错误信封按类别分流 |
| [`P12-ARCH.md`](specs/P12-ARCH.md) | 请求代理与无缝切号（改走本地路由） |
| [`P12-SPIKE-RESULTS.md`](specs/P12-SPIKE-RESULTS.md) | P12 技术可行性结论（A/C 两路出局的过程记录） |
| [`P13-REFERENCE-PARITY.md`](specs/P13-REFERENCE-PARITY.md) | 参考实现对齐：把可纳入的能力排进期次 |
| [`P14-TASKS.md`](specs/P14-TASKS.md) | 会话跨客户端 + 关联表升级（取证版任务拆分） |
| [`P15-SPIKE-RESULTS.md`](specs/P15-SPIKE-RESULTS.md) | 外部 Agent 会话监听 spike 结论（S1–S4 已落地） |

### 产品线与平台

| 文档 | 内容 |
|:--|:--|
| [`UNIFIED-GATEWAY-ARCH.md`](specs/UNIFIED-GATEWAY-ARCH.md) | 统一网关：单入口 + 命名空间模型（含新增平台契约 §11） |
| [`UNIFIED-GATEWAY-UI.md`](specs/UNIFIED-GATEWAY-UI.md) | 统一入口的交互规格：三张卡 / 状态矩阵 / 危险动作分档 |
| [`QODER-PLATFORM.md`](specs/QODER-PLATFORM.md) | Qoder 平台事实与建模决策 |
| [`QODER-INTEGRATION-PLAN.md`](specs/QODER-INTEGRATION-PLAN.md) | Qoder 集成的模块 / 接口 / 依赖梳理 |
| [`trae-parity-design.md`](specs/trae-parity-design.md) | Trae 分区对齐 WorkBuddy 的增量架构与任务分解 |
| [`trae-parity-matrix.md`](specs/trae-parity-matrix.md) | Trae 差距矩阵与需求池（带 `文件:行号` 证据） |

### 待拍板方案（还没写码）

| 文档 | 内容 | 状态 |
|:--|:--|:--|
| [`MODEL-LIMIT-PROBE-PLAN.md`](specs/MODEL-LIMIT-PROBE-PLAN.md) | 账号池 × 模型限流探测的完整方案 | 待确认 |
| [`POOL-PICK-EXCLUDE-PLAN.md`](specs/POOL-PICK-EXCLUDE-PLAN.md) | 账号池选号 Bug + 「排除账号」方案 | 待拍板 |
| [`CLIENTLESS-AUTH-PLAN.md`](specs/CLIENTLESS-AUTH-PLAN.md) | 无客户端部署（服务器 / 容器）的登录授权与网关可用性（issue #62） | 待拍板 · 原型已出 |
| [`KEY-PERMISSION-UI-DESIGN.md`](specs/KEY-PERMISSION-UI-DESIGN.md) | 密钥访问权限管理：token 配额 + 可访问账号白名单（issue #76） | 已实现（`KeyActor` 统一四条产品线） |

---

## `history/` — 只读历史

**这里的文档不再维护。** 它们记录「当时怎么想的 / 当时实测发现了什么」，结论已经被 `specs/` 吸收或推翻。要改行为别照这里抄。

| 文档 | 为什么在历史区 |
|:--|:--|
| [`overview.md`](history/overview.md) | 2026-10-09 的多区域 + 网关改造总览，已被 README 与 `specs/` 取代 |
| [`ia-redesign-proposal.md`](history/ia-redesign-proposal.md) | v1 信息架构方案，被 `specs/PLAN.md`（v2）取代 |
| [`ia-prototype.html`](history/ia-prototype.html) | v1 方案的静态原型，体验请用 [`prototype/index.html`](prototype/index.html) |
| [`GAP-ANALYSIS.md`](history/GAP-ANALYSIS.md) | 2026-10-04 的差距快照，P0–P16 落地后大部分判定已过期 |
| [`MODEL-IDENTITY-ARCH.md`](history/MODEL-IDENTITY-ARCH.md) | 已被 `specs/UNIFIED-GATEWAY-ARCH.md` 取代（同日改走命名空间） |
| [`ALIGN-2026-10-10.md`](history/ALIGN-2026-10-10.md) | 原型 ↔ 实现 14 项差异，**已全部处理完毕** |
| [`VISUAL-ALIGN-2026-10-10.md`](history/VISUAL-ALIGN-2026-10-10.md) | 视觉 1:1 对齐的改造记录，已完结 |
| [`UNIFIED-GATEWAY-UX-REVIEW.md`](history/UNIFIED-GATEWAY-UX-REVIEW.md) | 统一网关交互体检，建议已吸收 |
| [`perf-audit-2026-09-24.md`](history/perf-audit-2026-09-24.md) | 性能审计报告（代码注释仍引它作为「为什么这么改」的证据） |
| [`perf-audit-2026-09-24-review.md`](history/perf-audit-2026-09-24-review.md) | 上者的逐条复核（第二遍） |
| [`trae-login-materialize-probe-2026-09-29.md`](history/trae-login-materialize-probe-2026-09-29.md) | 探针实测：账号库凭据无法忠实写入 Trae 客户端 |
| [`reference-changexbc-workbuddy-switch.md`](history/reference-changexbc-workbuddy-switch.md) | 外部同产品实现的调研记录 |
| [`reference-workbuddy-manager.md`](history/reference-workbuddy-manager.md) | 外部同栈项目的调研记录 |
| [`CLEANUP-2026-10-13.md`](history/CLEANUP-2026-10-13.md) | 上一轮工作区清理报告（含当时的引用扫描证据） |

---

## `prototype/` — 原型与校验套件

| 文件 | 用途 |
|:--|:--|
| `index.html` | 可直接双击打开的交互原型（多视图演示） |
| `buddy-switch-prototype.html` | 单文件原型（视觉基准，`check:scope` 会读它） |
| `demo-app.js` `demo-data.js` `demo.css` | 原型的逻辑 / 演示数据 / 样式 |
| `key-permission-design.html` | 密钥访问权限管理设计稿（issue #76）：列表两列 + 权限抽屉 + 创建对话框 |
| `sidebar-brand-design.html` | 侧栏品牌块 × 顶栏平台图标去混淆设计稿（issue #75）：问题复现舞台 + 四方案对比 |
| `limit-link-proto.html` | 客户端限流 × 模型限流探测的联动设计稿（2026-10-22）：左账号页台账卡、右网关页矩阵，**点任一侧「实测」两侧同时变**（数据同步的唯一可视化证据） |
| `verify-limit-link.mjs` | 联动设计稿的行为校验（**CDP/Chrome**，34 条：同步 / 累积 / 冲突并列 / 402 分列 / 入口代价 / 作业单例） |
| `verify-*.cjs` | JSDOM 驱动的原型行为校验（`verify-behaviors.cjs` 是 112 条断言那一套；`verify-key-permission-main.cjs` 是**主原型**里的密钥权限那一套，`verify-key-permission.cjs` 是独立设计稿那一套；`verify-sidebar-collapse.cjs` 是侧栏折叠那一套） |

> ⚠️ **`scripts/check-scope-registry.cjs` 会直接读这里的文件**（不是注释引用）。改名或移动必须同步改脚本，否则门禁红。

---

## 新文档放哪（四条规则）

1. **先问「它会不会跟着代码一起变」**：会 → `specs/`；不会（一次性结论、审计、调研、旧方案）→ `history/`；是给人上手用的步骤 → `guides/`。
2. **文件名说清性质**：一次性文档带日期（`xxx-2026-10-17.md`）；分期架构用 `P{n}-ARCH.md` / `P{n}-TASKS.md`；不要出现 `notes.md`、`tmp.md`、`新文档.md`。
3. **首段必须有状态行**（一行 `>` 引用即可）：
   - `specs/` → `> 状态：现行（YYYY-MM-DD）` / `待拍板` / `实施中`
   - `history/` → `> 状态：只读历史（不再更新）` + 一句「被谁取代 / 已完结」
4. **新增文档后回来更新本文件的表格。** 索引和文件不同步，比没有索引更坏。
