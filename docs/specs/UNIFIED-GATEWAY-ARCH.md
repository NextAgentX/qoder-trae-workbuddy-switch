# 统一网关：单入口 + 命名空间模型（v3 · Trae 命名重做）

> ## ⚠️ 阅读须知：端口已重排（2026-10-17）
>
> 本文写作时端口是 WB `57891` / Trae `7864` / 统一 `57892`。**2026-10-17 起四条网关统一到 `5789x` 一段**：
> 界面 `57890` · WB `57891` · 统一 `57892` · hook `57893` · **Trae `57894`（原 7864）** · **Qoder `57895`（原 7865）**。
> 正文里出现的 `7864` / `7865` 一律是**当时的取值**，判断当前行为请看
> [`../guides/GATEWAY-CLIENT-INTEGRATION.md`](../guides/GATEWAY-CLIENT-INTEGRATION.md) 与 [`../guides/DEVELOPMENT.md`](../guides/DEVELOPMENT.md) 的端口表。
> 本次只改**默认值**，不改「四个端口、四套 Key 库、四份配置、绝不 merge Router」这条结论。

> ## ★ 落地状态（2026-10-14 收口 —— 原「未编号、未动一行代码」已失效）
>
> **本方案已编号为 P16，且 P1–P5 全部落地**：
>
> | 期 | 内容 | 状态 | 证据 |
> |:--|:--|:--|:--|
> | P1 | core 命名空间表 | ✅ | `crates/buddy-switch-core/src/modules/gateway_namespace.rs`（派生层 4 + 扩展层 4） |
> | P2 | 统一网关产品线（配置 / 密钥 / router / 宿主） | ✅ | `gateway/src/unified/{config,apikey,routes,state,mod}.rs`；`server/src/unified_gateway_host.rs`；`src-tauri/src/unified_gateway.rs` |
> | P3 | 分派 + 流式 + Anthropic | ✅ | 七个路径全部已挂，`OWNED_PATHS` 与实现两侧一致 |
> | P4 | 契约收尾 + 管理面四处同步 | ✅ | `check:api` 绿（routes 182 / call 172 / server 166 / invoke 190） |
> | P5 | 前端「统一入口」三张卡 | ✅ | `src/components/gateway/unified-gateway-view.tsx` + `GATEWAY_SUBNAV` 第 7 项 |
> | P6 | 验收（真起三个监听 + 打请求） | ✅ | **真实环境四段全部真实上游 200**（用户授权用真实环境测）；导出 EXE `v2026.10.141319` |
>
> **唯一仍未核实的**（本仓既有诚实边界，非本次引入）：`trae-global`（国际合并版）的凭据从未对真实上游跑通过 —— 它可能「列得出来、调不动」。见 §10 末尾。
>
> ---
>
> 状态：**v3 —— 只改了 Trae 侧命名（你指出「很乱」，复核后确认是我 v2 定错了）+ 重算受影响的全部例子**。
> 日期：2026-10-14。依据：**本仓源码实测**。
> v2 → v3 改动：§2（Trae 命名重做）、**§2.2 推翻 v2 的「`trae-cn` → TraeWork CN」**、
> §3/§4/§5 所有例子里的字面量、§10（八条待拍板全部由我定稿，见下）。
> **配套 UI 交互方案：`UNIFIED-GATEWAY-UI.md`**（三张卡、状态矩阵、危险动作分档、验收钩子、扩展面）。

## 0. 决策台账

| # | 决策 | 状态 |
|:--|:--|:--|
| 1 | 命名空间字面量 `wb-cn` / `wb-global` / `trae-cn` / `trae-global` | ✅ 保留 |
| 2 | ~~`trae-cn` → TraeWork CN~~ | ❌ **v3 作废** —— 改为「命名空间 = `{客户端名}-{区域}`」，`trae-*` = **合并版** `Trae`，见 §2 + §10-D2 |
| 3 | 新增 57892，保留 57891 / 7864 | ✅ |
| 4 | 裸模型名拒绝 | ✅ |
| 5 | 一把统一 Key 可同时授权 `wb-*` 与 `trae-*` | ✅ |
| 6 | `/biz/*path` 上统一入口 | ✅（附带的命名空间来源见 §4.5） |
| 7 | 前端本期做 | ✅ |

**你原话**：「命名空间 trae 的感觉很乱」。**你是对的 —— 而且根因不在你的方案里，在我 v2 的映射里。** 见 §2.1。

---

## 1. 架构（未变）

### 1.1 合并入口，不合并 router

两个网关的 `/v1/models`、`/v1/chat/completions` **逐字相同**，axum 同路径 `merge` 会**运行期 panic**（不是编译期）。本仓有护栏测试 `lib.rs::two_products_own_overlapping_paths` 专门钉住这个重叠。

⇒ **新增第三个产品线「统一网关」**，只挂**一份**路径，handler 内按模型名命名空间分派到两条**既有 relay**。两个 `shared_state()` 都是进程内单例 ⇒ 直接拿，零状态复制。

### 1.2 三个入口并存

```
                    ┌───────────────────────────────────────────────┐
 一个 provider ─────►│ 统一网关  127.0.0.1:57892   「新增」           │
 sk-uni-…           │ /health（兼容 /healthz）/status /v1/models    │
                    │ /v1/chat/completions /v1/messages             │
                    │ /v1/responses /biz/*path                      │
                    │ ① 认证：统一 Key 库（namespaces 授权）         │
                    │ ② 解析：模型名 → 命名空间 → 目标轴             │
                    │ ③ 分派：调对应产品线的既有 relay               │
                    └──────┬──────────────────────┬─────────────────┘
              ┌────────────▼──────────┐  ┌────────▼──────────────────┐
              │ WB  relay（既有）      │  │ Trae relay（既有）         │
              │ GatewayState（共享）   │  │ TraeGatewayState（共享）   │
              └───────────────────────┘  └───────────────────────────┘

  保留不动（可回滚）：WB 57891 · Trae 7864 · webui 57890 · Trae MITM 8899
```

---

## 2. ★ Trae 命名重做

### 2.1 「乱」的三个根因（都是真的）

1. **`trae-cn` 这个名字已经被客户端自己占用了。**
   客户端 `product.json` 的 `nameAlias` 实测（`trae/region.rs` 模块头的表）：`TRAE SOLO CN` → **`TraeWork CN`**；`Trae CN` → **`TraeCode CN`**。
   而 **`Trae CN` / `Trae` 同时是「合并后那条客户端」的目录名与 exe 名**。
   ⇒ 「`trae-cn`」在客户端自己的词典里**不是**「国内 Trae」的简写。v2 把它定义成 TraeWork CN，是我的硬塞。

2. **Trae 天生是两维的：区域 × 程序位。** 国内**同时存在两个客户端**（TraeWork CN / TraeCode CN，现已合并），模型清单与上游 `function` 都不同。一个「`trae-{区域}`」装不下两维。`trae/region.rs` 模块头写得最清楚：
   > `TraeRegion`（账号体系/端点）× `TraeProgram`（写哪个客户端目录、启哪个 exe）—— **持久化轴是区域，执行轴是程序**。

3. **`TraeVariant` 正是「把两维压成一维」的产物，而它已被本仓标记 deprecated。**
   `trae/variant.rs` 模块头原文：**「本模块已弃用；新代码不得再依赖 `TraeVariant`」**，目标形态就是上面那两根正交轴。
   ⇒ 命名空间若建在 `TraeVariant` 上，等于把一处**正在还的历史债**再复制一份。

### 2.2 命名规则（v3.1 · 由 §10-D2 定稿）

```
{名字}-{区域}
```

`{名字}` **一律取客户端自己的名字**（可逐字核对），**没有两段/三段之分**：

| `{名字}` | 来源 | 客户端 |
|:--|:--|:--|
| `wb` | **既有平台短标识**（`PlatformSlug` / `sk-wb-` 前缀同源），不是新造 | WorkBuddy |
| `trae` | 客户端自己的目录名 / exe 名（`Trae CN` / `Trae`） | **合并版**（当前主程序位） |
| `traework` | 客户端 `nameAlias` = `TraeWork CN` | 老 `TRAE SOLO CN` 程序线 |
| `traecode` | 客户端 `nameAlias` = `TraeCode CN` | 老 IDE 程序线 |

⇒ **默认 4 条 = `Scope::all()` 的 4 个字面量**（`wb-cn` / `wb-global` / `trae-cn` / `trae-global`，
见 §2.6 的分层）；扩展层再补老程序线 × 两个区域。

★ **必须有的护栏**：`trae` / `traework` / `traecode` **互为前缀**（`traework` 以 `trae` 开头）⇒
解析**只能整段精确匹配，禁止 `starts_with` / 最长前缀匹配**（派生层用 `Scope::parse`，天然白名单精确匹配）。

### 2.3 默认启用的 4 条

| 命名空间 | 产品线 · 区域 | 程序位 | 客户端（官方别名） | `function` | 目标 variant |
|:--|:--|:--|:--|:--|:--|
| `wb-cn` | WorkBuddy · 国内 | 桌面端 | WorkBuddy | — | `Region::Cn` |
| `wb-global` | WorkBuddy · 国际 | 桌面端 | WorkBuddy AI | — | `Region::Global` |
| **`trae-cn`** | Trae · 国内 | **`Trae`（合并版）** | `Trae CN` | `chat_v3` | `trae_merged` |
| **`trae-global`** | Trae · 国际 | **`Trae`（合并版）** | `Trae` | `chat_v3` | `global_trae_merged` |

★ 你的四个字面量**一个都不用改**，只把 `trae-*` 的**含义**从「TraeWork」纠正成「合并版 Trae」—— 这才是与客户端自称一致的那个意思，且 `function` 取 `chat_v3`（正确的那一支）。

### 2.4 可选启用的 4 条（`namespaces` 里显式加入才生效）

| 命名空间 | 程序位 | 客户端 | `function` | 目标 variant | 什么时候要它 |
|:--|:--|:--|:--|:--|:--|
| `traework-cn` | `TraeWork` | `TRAE SOLO CN`（老） | `solo_work_lite` | `trae_work` | 机器上还留着**未升级**的 TRAE SOLO CN |
| `traework-global` | `TraeWork` | `TRAE SOLO` | `solo_work_lite` | `global` | 同上（国际，本仓从未对真实上游跑通） |
| `traecode-cn` | `TraeCode` | `Trae CN`（遗留 IDE） | `chat_v3` | `trae_cn` | 未合并的老机器，想把它当独立程序位 |
| `traecode-global` | `TraeCode` | `Trae` | `chat_v3` | `global_trae_code` | 同上（国际） |

**默认不启用它们的理由**：`TraeProgram::TraeWork` 自 2026-10-13 起**已被客户端并入合并版**（本机 `TRAE SOLO CN` 安装目录已卸载、只剩 `debug.log`）。默认开着会「列得出来、调不动」—— 客户端不存在 ⇒ 清单读不到 ⇒ 回落静态清单 ⇒ **假可用**。

### 2.5 ★ 命名空间编到程序位，顺手解决了「TraeCode 要不要做」

v2 里我说「本期不含 TraeCode」，理由是「一个 `trae-cn` 装不下两个程序位 ⇒ 会列得出来调不动」。
**现在每个命名空间都自带自己的程序位 ⇒ 自带自己的 `function`** ⇒ 这个理由消失了。TraeCode 想要就加 `traecode-cn` / `traecode-global`，**零额外风险**。

### 2.6 落点：**两层** —— 派生层零手写 + 扩展层一张小表

★ 关键发现：**默认命名空间与 core 既有的 `Scope` 是同一个东西**。
`crates/buddy-switch-core/src/modules/product.rs` 已有 `Scope` 枚举 + `Scope::all() -> [Scope; 4]`，
且它的 4 个字面量**逐字就是** `wb-cn` / `wb-global` / `trae-cn` / `trae-global`。
`Scope::parse` 本身是 **fail-closed 白名单精确匹配**（未知值 `Err`，绝无前缀匹配）。

```rust
// crates/buddy-switch-core/src/modules/gateway_namespace.rs（新）
pub enum NamespaceTarget {
    Scope(Scope),                                        // 派生层：直接就是作用域
    TraeExtra { region: TraeRegion, program: TraeProgram }, // 扩展层：平台内部的额外程序线
    // ★ 未来新平台若有额外程序线，在这里加 variant ⇒ 所有 match 立即报错 = 编译器列出全部落点
}

pub fn parse(id: &str) -> Option<NamespaceTarget> {
    if let Ok(scope) = Scope::parse(id) {           // ① 派生层：白名单精确匹配，天然 fail-closed
        return Some(NamespaceTarget::Scope(scope));
    }
    TRAE_EXTRA.iter().find(|s| s.id == id).map(...)  // ② 扩展层：整段精确匹配（禁 starts_with）
}
```

| 层 | 内容 | 新增平台/程序线时 |
|:--|:--|:--|
| **派生层** | 每个 `Scope` 一条命名空间，程序位 = **该平台的主程序位**（表里查一次） | **零手写** —— `Product` / `Scope` / `ProductSpec` 三处声明一加，命名空间**自动出现** |
| **扩展层** | 平台内部的**额外程序线**（当前只有 `traework-cn` / `traework-global` / `traecode-cn` / `traecode-global`） | 手写一行（该平台真的多了一条程序线时才需要） |

**★ 三条硬规则**：

1. **映射一律经既有派生函数**：`Scope` → 各平台的轴用 `Scope::wb_region()` / `Scope::trae_region()`（已存在）；
   扩展层的 `(region, program)` → **`client_settings::variant_for_program(region, program)`**（`client_settings.rs:184`，
   `变体 ↔ (区域, 程序位)` 是**双射**，有单测）。
2. **解析只查显式白名单，禁止前缀匹配**（`trae-cn` / `trae_cn` / `trae_merged` 字形相近，推断会**静默走错池**）。
3. **每条命名空间的目标轴都是表内写死的事实**，不是运行时猜的。

### 2.7 新增平台时**自动出现**什么（这就是「方便拓展」的落点）

加了 `qoderwork` 之后（见 §11 契约），下面全部**零改动**自动成立：

- 命名空间 `qoderwork-cn` / `qoderwork-global` 合法（派生自 `Scope`）；
- 统一 Key 的「授权命名空间」多选里出现这两项（表驱动）；
- 卡 3「命名空间与清单预览」自动多两行（表驱动）；
- `/v1/models` 自动聚合这两段的模型；
- 错误文案里的「本入口启用的命名空间」清单自动带上它们。

**必须手写的只有三件**（且编译期会盯着，见 §11）：relay 分派分支、模型清单来源、管理面命令。

### 2.8 前端镜像

`src/lib/gateway-namespaces.ts`（新）—— 派生层**从 `SCOPES` 生成**（不手抄 4 个字面量），
扩展层与 core 逐字一致，门禁核对（照既有 `product_spec_matches_frontend_registry_slug_and_capabilities`）。

---

## 3. 两个新数据文件 + 配置例子

### 3.1 统一网关配置 `~/.buddy-switch/gateway_unified.json`

**与 `GatewayConfig` 同名的键一律同义**，只多 2 个键。

```json
{
  "enabled": false,
  "bind_addr": "127.0.0.1",
  "port": 57892,
  "allow_non_loopback": false,
  "log_keep": 200,
  "log_bodies": false,
  "per_key_rate_limit": null,
  "max_rotate": 3,
  "sanitize_fingerprints": true,
  "prompt_mode": "passthrough",
  "prompt_file": null,
  "sticky_ttl_ms": 1800000,
  "pool": {},
  "max_body_mb": 8,
  "namespaces": ["wb-cn", "wb-global", "trae-cn", "trae-global"],
  "default_namespace": null
}
```

要启用老程序线（§2.4）就往数组里加：
```json
"namespaces": ["wb-cn", "wb-global", "trae-cn", "trae-global", "traecode-cn"]
```

| 键 | 含义 | 默认 |
|:--|:--|:--|
| `namespaces` | 本入口**启用**哪些命名空间（未列出的：不解析、不列清单、调用报 400 `namespace_not_enabled`） | §2.3 的 4 条 |
| `default_namespace` | 裸模型名落到哪个命名空间；**`null` = 拒绝**（你选的） | `null` |

- ⚠️ 键集要有**精确钉死**的契约测试（照 `gateway_config_serializes_with_snake_case_keys`）；`max_body_mb` 在 **router 构建期固化** ⇒ 改值**要重启**。
- ⚠️ **不含** `allow_model_region_prefix`（那是 WB 独立端口的键）—— 统一入口**不认** `cn:` / `global:` 前缀（§4.3-⑤）。

### 3.2 统一密钥 `~/.buddy-switch/gateway_unified_keys.json`

```json
{
  "keys": [
    {
      "id": "8f3c1a2e-…",
      "name": "我的主力 Key（四个命名空间全开）",
      "prefix": "sk-uni-a1b2",
      "suffix": "9f3c",
      "hash": "<sha256(hex) of 完整明文>",
      "namespaces": ["wb-cn", "wb-global", "trae-cn", "trae-global"],
      "biz_namespace": "wb-cn",
      "created_at": 1760400000000,
      "revoked_at": null,
      "last_used_at": null
    },
    {
      "id": "b71d…",
      "name": "只给 Trae 用的 Key",
      "prefix": "sk-uni-c3d4",
      "suffix": "0a1b",
      "hash": "…",
      "namespaces": ["trae-cn", "trae-global"],
      "biz_namespace": null,
      "created_at": 1760400000000,
      "revoked_at": null,
      "last_used_at": null
    }
  ]
}
```

- 前缀 `sk-uni-`（与 `sk-wb-` / `sk-trae-` 同族）。★ 2026-11-04：创建时明文**不落盘**（`plaintext: null`），只随创建响应返回；用户点「保存到本软件」才由 `save_plaintext` 写入并随列表下发（列表眼睛可看），选「自行保存」则永久只有掩码。校验仍只认 sha256 + 前 4 / 后 4 位。
- `namespaces` 空数组或缺失 ⇒ **无授权**（fail-closed），**不当作「全开」**。表里不存在的 id ⇒ 保存时拒绝（不静默丢弃）。
- 认证头：**`Bearer` 与 `x-api-key` 都收**（统一 Key 是新库，不继承 Trae「只认 Bearer」的历史包袱）；**不按认证头反推产品线**。
- `biz_namespace`：见 §4.5，仅 `wb-*` 合法。

---

## 4. 对外契约

### 4.1 `GET /v1/models`（聚合 + 自动发现）

**这是本方案最大的收益**：id 自带命名空间 ⇒ 客户端**自动发现即可用**，不用手抄。

```json
{
  "object": "list",
  "data": [
    { "id": "wb-cn:deepseek-v4.1-flash", "object": "model", "namespace": "wb-cn",
      "owned_by": "workbuddy", "context_window": 1000000, "max_tokens": 128000,
      "supports_images": true, "credits": "x0.03 credits", "badges": ["独家优惠"], "free": false },

    { "id": "wb-global:deepseek-v4.1-flash", "object": "model", "namespace": "wb-global",
      "owned_by": "workbuddy", "context_window": 300000, "max_tokens": 128000,
      "supports_images": true, "credits": null, "badges": [], "free": false },

    { "id": "trae-cn:deepseek-v4.1-flash", "object": "model", "namespace": "trae-cn",
      "owned_by": "trae", "context_window": 131072, "max_tokens": null,
      "supports_images": true, "credits": null, "badges": [], "free": false },

    { "id": "trae-global:glm-5.3", "object": "model", "namespace": "trae-global",
      "owned_by": "trae", "context_window": 131072, "max_tokens": null,
      "supports_images": true, "credits": null, "badges": [], "free": false }
  ],
  "namespaces": {
    "wb-cn":       { "source": "live",    "fetched_at": 1760400000000, "note": null },
    "wb-global":   { "source": "cached",  "fetched_at": 1760390000000,
                     "note": "展示的是上次成功识别的缓存目录（本次未能连上上游）" },
    "trae-cn":     { "source": "builtin", "fetched_at": null,
                     "note": "客户端未拉取过模型清单（Trae 需至少启动过一次），展示的是静态兜底清单" },
    "trae-global": { "source": "live",    "fetched_at": 1760400000000, "note": null }
  }
}
```

★ 三条硬要求：
1. **只列该 Key 已授权的命名空间**（未授权的整段不出现）。
2. 已禁用的模型**不出现**（与出口 ② 共用同一份判断，见 §4.4）。
3. **每个命名空间单独带 `source` + `note`** —— 沿用既有契约「**非实时快照的 `note` 恒非空**」（P10 §1.3 规则 2）。一个总徽标糊过去不允许：用户要能看出「哪段实时、哪段兜底、为什么」。

### 4.2 请求（5 条 curl）

```bash
export KEY="sk-uni-在这里粘你的key"
BASE=http://127.0.0.1:57892

# ① 国内版 WorkBuddy
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"wb-cn:deepseek-v4.1-flash","messages":[{"role":"user","content":"hi"}]}'

# ② 国际版 WorkBuddy
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"wb-global:deepseek-v4.1-flash","messages":[{"role":"user","content":"hi"}]}'

# ③ 国内 Trae（合并版客户端 Trae CN）
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"trae-cn:deepseek-v4.1-flash","messages":[{"role":"user","content":"hi"}]}'

# ④ 国际 Trae（合并版客户端 Trae）
curl -s $BASE/v1/chat/completions -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"trae-global:glm-5.3","messages":[{"role":"user","content":"hi"}]}'

# ⑤ Anthropic 协议（协议头不同，Key 同一个）
curl -s $BASE/v1/messages -H "Authorization: Bearer $KEY" \
  -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d '{"model":"wb-cn:deepseek-v4.1-flash","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'
```

### 4.3 错误响应（全部**有出路**，不静默）

**① 裸模型名被拒（400）**
```json
{ "error": {
    "message": "统一网关要求模型名带命名空间前缀。本入口启用的命名空间：wb-cn: / wb-global: / trae-cn: / trae-global:。你填的是 `deepseek-v4.1-flash`，请改成 `wb-cn:deepseek-v4.1-flash` 这类形式。",
    "type": "invalid_request_error", "code": "namespace_prefix_required" } }
```

**② 命名空间未授权（403）**
```json
{ "error": {
    "message": "该 API Key 未被授权使用命名空间 `trae-global`（它只授权了 wb-cn、wb-global）。请到「API 服务 → 统一入口 → 密钥」为该 Key 勾选，或另建一把。",
    "type": "permission_error", "code": "namespace_forbidden" } }
```

**③ ~~Trae 命名空间 + `/v1/responses`（501）~~ —— 2026-10-22 起不再成立**

Trae 侧已补齐 `/v1/responses`（入站 Responses → chat、出站 chat SSE → Responses 事件流，
中继与 chat / messages 同一份）。统一网关对 `trae-*` 命名空间**直接分派**到
`trae::routes::responses_for_variant`，`endpoint_not_supported_for_namespace` 这个错误码
与 `LlmEndpoint::supports_trae()` 一起移除。三条产品线现在三个 LLM 端点全通。

（历史：曾有「Trae 没有该端点 ⇒ 501、不许 404、不许静默降级到 chat」的约定。补齐后
这条约定失去对象，但它想防的东西仍然成立 —— **不认识的能力要报错，不要静默降级**。）

**④ 前缀在册但命名空间未启用（400）**
```json
{ "error": {
    "message": "命名空间 `traework-cn` 未在本入口启用（`gateway_unified.json` 的 `namespaces` 里没有它）。",
    "type": "invalid_request_error", "code": "namespace_not_enabled" } }
```

**⑤ 在**旧**端口（57891）填命名空间（400）** —— 低成本补救，建议加
```json
{ "error": {
    "message": "模型名 `wb-cn:deepseek-v4.1-flash` 看起来是统一网关的命名空间前缀，但本端口（57891）不解析命名空间。请改用统一网关 http://127.0.0.1:57892/v1，或去掉前缀、改用 `cn:` / `global:` 前缀。",
    "type": "invalid_request_error", "code": "namespace_not_parsed_here" } }
```
不加这条的话，`wb-cn:x` 会被**原样转发给上游** ⇒ 用户只看到「模型不存在」，联想不到是端口用错了。

### 4.4 禁用 / 日志 / 粘性 / 统计

| 处 | 统一入口上的做法 | 为什么 |
|:--|:--|:--|
| **模型禁用** | **键 = 底层 region（`cn` / `global`）**，与两个既有入口**共用同一份** `gateway_disabled_models.json`。命名空间 → region 走既有 **`trae/mod.rs::region_of_variant`**（不是新写的映射） | 三入口语义一致、零重复，且**与 Trae 侧既有口径同源**（禁用存储本来就是 region 粒度 —— `region_of_variant` 的函数注释把它写成了「决策 #4」）。代价：统一入口禁用 `wb-cn:x` 会同时影响 57891 —— **这是正确的**（同一个上游模型） |
| **请求日志** | 加 `namespace`（`String`）+ `resolved_model` + `program`；`region` 字段**保留**（既有统计页依赖它） | `RequestMeta.region` 是 `Region` 枚举，**不能**把 Trae 程序位塞进去；`resolved_model` 让排查能看到「`trae-cn:x` 实际发的是什么上游模型名」 |
| **会话粘性** | 键**继续用 region** | 两个产品线的账号池都是**区域级** ⇒ region 就是池的粒度；再加一维只会把粘性切碎 |
| **Token / 积分统计** | 按 region 聚合（既有）；Trae 侧行键仍为 `<region>:<uid>` | 不折叠（既有规则） |

### 4.5 `/biz/*path`（你选了「上统一入口」⇒ 需要一条附带决定）

事实：`/biz/*path` 是 **WB 独有**的「代理客户端自身业务请求」通道，白名单外一律 404（fail-closed），复用 `relay` 的选号/冷却/换号治理层。它**没有模型名** ⇒ 命名空间无法从请求体解析；而它**需要 region** 才能选池。

| | 命名空间从哪来 | 评价 |
|:--|:--|:--|
| **B1（我推荐）** | 统一 Key 的 `biz_namespace` 字段（仅 `wb-*` 合法；留空 ⇒ 400「该 Key 未指定业务转发命名空间」） | 明确、fail-closed、**不需要客户端加任何头** —— 真实调用方（客户端自身）**加不了**自定义头 |
| B2 | 请求头 `X-Buddy-Switch-Namespace: wb-cn` | 客户端加不了头 ⇒ 实务不可用 |
| B3 | 隐式取该 Key 授权的第一个 `wb-*` | 能用但**隐式**，与「裸名拒绝」的哲学相反 |

⇒ 已把 `biz_namespace` 写进 §3.2。`trae-*` 作 `biz_namespace` ⇒ **保存时拒绝**（Trae 业务域不可配，本仓 spike 已定 H0）。

### 4.6 图片（多模态）在各产品线的处理

三条产品线对图片的处理**不一样**，这是上游契约差异决定的，不是实现漂移：

| 产品线 | `/v1/chat/completions` | `/v1/messages` | `/v1/responses` |
|:--|:--|:--|:--|
| **WB** | 透传（结构不动，图片 parts 原样给上游） | 默认丢图 + **文本留痕** | 保留：`input_image` → `image_url` part |
| **Trae** | 透传（`content` 数组原样；上游本就要求数组） | 同上 | 保留：`input_image` → `image_url` part（与 WB 同一条 `protocol::responses` 实现） |
| **Qoder** | **归一**成 `image_url` part | 归一（`image` block → part） | 归一（`input_image` → part） |

- **Qoder 必须归一**：上游只读 `messages[].content` 里的结构化 `image_url` parts，**不读**顶层 `image_urls` / `chat_context.imageUrls`（issue #24 实测：填了顶层模型仍回「我没有收到图片」）。顶层两份默认不填 —— base64 各存一份会让出站体积翻倍，撞上游 30MB 上限被 RST 并熔断账号池（`BUDDY_SWITCH_QODER_KEEP_TOPLEVEL_IMAGES=1` 仅排查用）。归一保留 `image_url.detail`（精度档位，掉了等于降画质）。
- **WB / Trae 是透传**：上游就是我们这套 OpenAI 方言，重建结构只会丢信息。`/v1/messages` 是例外 —— 上游是否吃 `content` 里的 parts **未实测**，所以默认丢。
- **丢图必须留痕**：`/v1/messages` 丢弃时在该条 user 文本末尾追加 `[已移除 N 张图片：当前通道未启用图片转发]`；**纯图片消息**也会保留一条带该提示的 user 消息（否则模型连「用户发了东西」都不知道，只会顺着问题编）。
- **开关**：`BUDDY_SWITCH_ANTHROPIC_FORWARD_IMAGES=1`（`on` / `true` / `yes`）让 WB / Trae 的 `/v1/messages` 改走带图分支。**先实测再开** —— 打开而上游不认，只是把「静默丢图」换成「上游 400」。
- **入站体积**：请求体上限 `max_body_mb` 默认 `0` ⇒ 回落 8MB，且 `DefaultBodyLimit` 在 router 构建期固化（**改值要重启网关**）。它比上游 30MB 更早撞上：手机原图转 base64 轻易超 8MB。

---

## 5. 客户端接入配置例子

| 客户端 | 必填 | 值 |
|:--|:--|:--|
| **OpenAI 系**（Codex / opencode / dsh / Trae 自定义模型 / CodeBuddy `models.json`） | 含 `/v1` 的基址 | `http://127.0.0.1:57892/v1` |
| **Anthropic 系**（Claude Code / CodeBuddy `CODEBUDDY_BASE_URL` / dsh `anthropic-messages`） | **根地址，不带 `/v1`** | `http://127.0.0.1:57892` |

### 5.1 Codex（`~/.codex/config.toml`）
```toml
model = "wb-cn:deepseek-v4.1-flash"
model_provider = "buddy_unified"

[model_providers.buddy_unified]
name = "Buddy Switch (Unified)"
base_url = "http://127.0.0.1:57892/v1"
env_key = "BUDDY_SWITCH_KEY"      # 变量名，不是 Key 本身
wire_api = "responses"            # 三条产品线（wb-* / trae-* / qoder-*）都可用
```
```bash
export BUDDY_SWITCH_KEY="sk-uni-..."
```

### 5.2 Claude Code
```bash
export ANTHROPIC_BASE_URL="http://127.0.0.1:57892"     # 根地址，别加 /v1
export ANTHROPIC_AUTH_TOKEN="sk-uni-..."
export ANTHROPIC_MODEL="wb-cn:deepseek-v4.1-flash"
```
（只留 `ANTHROPIC_AUTH_TOKEN`，别同时设 `ANTHROPIC_API_KEY`。）

### 5.3 dsh（`$DSH_HOME/profiles/<profile>/cordis.patch.yml`）
```yaml
- id: llm-pi-ai
  config:
    providers:
      buddy-unified:
        apiKeyEnv: BUDDY_SWITCH_KEY
        api: openai-completions
        baseURL: http://127.0.0.1:57892/v1
        defaultInput: [text, image]   # 路由级兜底；条目非空 input 优先
        models:
          - id: wb-cn:deepseek-v4.1-flash
          - id: wb-global:deepseek-v4.1-flash
          - id: trae-cn:deepseek-v4.1-flash
            input: [text, image]
```
（`<profile>`：`dsh web` 为 `web`，桌面版为 `desktop`；密钥在 `$DSH_HOME/.credentials.yaml`。
模态是**声明**不是探测 —— 不写就按纯文本处理，图片在准入阶段就被拦下。细则见
[`../guides/GATEWAY-CLIENT-INTEGRATION.md`](../guides/GATEWAY-CLIENT-INTEGRATION.md) §3.4。）

### 5.4 CodeBuddy CLI（`~/.codebuddy/models.json`）—— 本需求最初的受害者
```json
{
  "models": [
    { "id": "wb-cn:deepseek-v4.1-flash",     "name": "WB 国内 · Flash",  "vendor": "OpenAI",
      "url": "http://127.0.0.1:57892/v1/chat/completions", "apiKey": "${BUDDY_SWITCH_KEY}",
      "maxInputTokens": 1000000, "maxOutputTokens": 128000, "supportsToolCall": true, "supportsImages": true },
    { "id": "wb-global:deepseek-v4.1-flash", "name": "WB 国际 · Flash",  "vendor": "OpenAI",
      "url": "http://127.0.0.1:57892/v1/chat/completions", "apiKey": "${BUDDY_SWITCH_KEY}",
      "maxInputTokens": 300000, "maxOutputTokens": 128000, "supportsToolCall": true, "supportsImages": true },
    { "id": "trae-cn:deepseek-v4.1-flash",   "name": "Trae 国内 · Flash", "vendor": "OpenAI",
      "url": "http://127.0.0.1:57892/v1/chat/completions", "apiKey": "${BUDDY_SWITCH_KEY}",
      "maxInputTokens": 131072, "maxOutputTokens": 8192, "supportsToolCall": true, "supportsImages": true },
    { "id": "trae-global:glm-5.3",           "name": "Trae 国际 · GLM",  "vendor": "OpenAI",
      "url": "http://127.0.0.1:57892/v1/chat/completions", "apiKey": "${BUDDY_SWITCH_KEY}",
      "maxInputTokens": 131072, "maxOutputTokens": 8192, "supportsToolCall": true, "supportsImages": false }
  ],
  "availableModels": ["wb-cn:deepseek-v4.1-flash","wb-global:deepseek-v4.1-flash","trae-cn:deepseek-v4.1-flash","trae-global:glm-5.3"]
}
```
★ 四条 id **两两不同** ⇒ **不再被判「已存在」、不再互相覆盖**。这正是本需求要的结果。
⚠️ `url` 必须以 `/chat/completions` 结尾；`${变量名}` 引用不到会原样保留并导致请求失败；启动前必须 `export`。

### 5.5 Trae IDE / SOLO 自定义模型
`服务商 = OpenAI`，地址按界面「完整 URL」开关二选一（`…/v1` 或 `…/v1/chat/completions`），模型 ID 从 `/v1/models` 复制，Key 填 `sk-uni-…`。

---

## 6. 分派实现（先 spike，再定终局）

| | **A. 进程内直调 relay**（终局） | **B. 进程内反代**（P0 spike 快路） |
|:--|:--|:--|
| 做法 | 把 `routes/{chat,messages,responses,biz}.rs` 与 `trae/routes.rs` **认证之后**的处理抽成 `pub(crate) async fn relay_*(state, region/variant, body, headers) -> Response`，统一 handler 直调 | 统一 handler 用 reqwest 转发到 `127.0.0.1:57891` / `7864` |
| 重复代码 | **零** | 零 |
| 代价 | 一次中等重构 | 多一跳 · 要读全 body · 要自己透传 SSE · 错误码要映射 · **依赖两个独立监听都开着**（统一开着但 WB 关着 ⇒ 502） |
| 定性 | **正式版** | **只用于 1–2 天内证明「前缀真能分派」** |

既有 `routes/relay.rs` 的 `RelayRequest` / `RelayOutcome` / `RelayKind` 说明 WB 侧已把「解析/路由」与「中继」分了一层 ⇒ A 的抽取量比看起来小。

---

## 7. 分期任务清单

### P1 · core 命名空间表（§2.6）
- 新 `crates/buddy-switch-core/src/modules/gateway_namespace.rs` + `modules/mod.rs` 注册。
- 测试：① 8 条全覆盖（4 派生 + 4 扩展）；② **`trae-cn` / `trae-global` 必须落 `TraeProgram::Trae`（合并版）** —— 单独钉死，防止有人改回 TraeWork；③ 8 条的 `(region, program)` 逐条正确；④ **字面量隔离护栏**：命名空间 id 与 `TraeVariant::as_str()` / `TraeProgram::as_str()` / `TraeRegion::as_str()` **不得互相解析**；⑤ **前缀护栏**：`traework` / `traecode` 不得被 `trae` 的匹配吃掉（用 `starts_with` 实现 ⇒ 护栏要红），并断言**集合内任意两条 id 不互为前缀**；⑥ 映射必须经 `Scope::wb_region` / `Scope::trae_region` / `client_settings::variant_for_program`（换成手写表 ⇒ 护栏要红）；⑦ **`Scope::all()` 的每个 scope 都必须能解析到命名空间**（加 scope 忘了命名空间 ⇒ 红，这是给后续加平台用的）。
- 前端镜像 `src/lib/gateway-namespaces.ts` + 跨语言门禁。

### P2 · 统一网关产品线 ✅ **全部落地（2026-10-14）**

**已完成**（`crates/buddy-switch-gateway/src/unified/`，18 条单测；gateway 全量 lib **440 passed / 0 failed**）：

- `config.rs` —— `UnifiedGatewayConfig`（`~/.buddy-switch/gateway_unified.json`）。键名与 `GatewayConfig` 同义；
  **有意不含** `allow_model_region_prefix`（属 WB 独立端口）与 `pool`（属各产品线）—— **有专门用例钉住这一点**。
  独有的两个键：`namespaces`（默认取派生层 4 条，**不抄字面量**）、`default_namespace`（默认 `None` = 拒绝裸名）。
  ★ `enabled_namespaces()` **按命名空间表顺序**返回（与配置文件书写顺序无关）；**空列表回落默认集**
  （否则手改配置会自锁成「怎么调都 400」的死入口）。
- `apikey.rs` —— `UnifiedApiKeyRecord` / `UnifiedApiKeyStore`（`gateway_unified_keys.json`，前缀 `sk-uni-`）。
  归属是 `namespaces: Vec<String>` + `biz_namespace`；**独立的第三份 Key 库**（不复用另两份，理由见模块头）。
  `create` **会失败**（校验授权集：非空 / 在册 / biz 必须也在授权集内且是 WB）；吊销可逆、删除仅限已吊销；
  每次操作实时读盘、只存 sha256 + 前 4 / 后 4 位。变异验证：去掉 `verify` 的吊销检查 ⇒ 红（「吊销后必须立刻失效」）。

**进度（2026-10-14 第二轮）**：第 1、2 步（`GatewayProduct` + `router` + `UnifiedGatewayState` + `/v1/models` 聚合）
与 **server 侧宿主**（`unified_gateway_host.rs` + `main.rs` 起监听）已落地。
`OWNED_PATHS` 与 `router()` **两侧一致**，当前 8 条（`/health` `/healthz` `/status` `/v1/models`
`/v1/chat/completions` `/v1/messages` `/v1/responses` `/biz/*path`）——
声明了却没挂就是撒谎（有专门用例钉住两侧一致）；未挂的业务端点在 57892 上是**明确的 404**
（不是假装能用的 501）。★ 新增门禁：`listener_serves_http_on_a_real_socket`（**真实 TCP** 打 `/health`，
端口 0 由内核分配）· `unified_gateway_also_overlaps_both_products`（三个 `OWNED_PATHS` 两两相交 ⇒ 必须三个端口）。
★ server 宿主的 `shared_state()` / `status()` **刻意未写**：管理面还没接，写了就是死代码。
**src-tauri 侧宿主未接**（沙箱内编译不了 src-tauri ⇒ 盲写风险高，留下一步）。

**未做（下一步按此顺序）**：

1. 新 `crates/buddy-switch-gateway/src/unified/{mod.rs,config.rs,apikey.rs}`：`UnifiedGateway: GatewayProduct`。
   `OWNED_PATHS` = `["/health","/healthz","/status","/v1/models","/v1/chat/completions","/v1/messages","/v1/responses","/biz/*path"]` + `router()` + `UnifiedGatewayState`。
2. 宿主接线：`crates/buddy-switch-server/src/unified_gateway_host.rs`（照 `gateway_host.rs`）+ `src-tauri` 一份（**两处宿主，各一套命令**）。
3. `/v1/models` 聚合（WB 取 `WbGateway::shared_state()` 的目录，Trae 取 `payload::models_response_for(variant, disabled)`）——
   建议做成**纯函数**（命名空间清单 + 两个取数闭包 + 禁用集 ⇒ 响应体），不起 HTTP 就能测。
4. 管理面**四处同步** ⇒ 跑 `npm run check:api`。
5. ★ 门禁：**真构建 router + 真打一条请求**（本仓曾因路由通配符语法构建期 panic、而 `cargo check` 全绿，放过一个「网关完全起不来」的 bug）。通配符必须是 axum 0.7 的 `*path`，**不是** `{*path}`。

### P3 · 分派（A 路）+ 流式 + Anthropic
**进度（2026-10-14 第三轮）**：★ **A 路抽取已完成一半** ——
`routes/chat.rs` 抽出 `pub async fn chat_for_region(state, region, headers, body, PrefixPolicy)`，
原 `handler` 退化成「鉴权 → 委托」；**diff 面极小**（`state` **按值**传入 ⇒ 函数体内既有的 `&state` 一行都不用改）。
新增 `PrefixPolicy`：产品线入口走 `RegionPrefix{allow_reroute}`（既有 `cn:`/`global:` 行为），
统一入口走 `AlreadyResolved`（**前缀已由命名空间处理，绝不再解析一次** —— 否则 `vendor:model` 这种模型名会被二次改写）。
统一网关已挂 `/v1/chat/completions`：鉴权 → 解析命名空间 → 查启用/授权 → **把 body 的 `model` 改写成裸名** → 委托。
**四个命名空间的 chat 已全部可真实转发** ✅
（`trae/routes.rs` 同样抽出了 `pub async fn chat_for_variant(state, variant, body)`，原 `chat_completions`
退化成「取扩展里的 variant → 委托」）。
**回归**：gateway **全部测试目标** —— 464 单测 + 49 集成测试（8 个 target）全绿。
⚠️ **一处诚实边界**：「已授权的 Trae 请求真的转发」这一跳**不在单测里跑** —— 它会用到真实的 Trae 账号池
与落盘路径（冷却 / 账本 / 日志），在单测里跑会在用户 `~/.buddy-switch/trae/` 下留痕迹。
该路径由「直接调 `chat_for_variant` 的既有 Trae e2e（自带隔离状态与假上游）」间接覆盖。

**三个 LLM 端点已全部接通** ✅（`/v1/chat/completions` · `/v1/messages` · `/v1/responses`），
**三条产品线全通**（Trae 的 `/v1/responses` 于 2026-10-22 补齐）。
★ 统一侧把它们收敛成**一份** `relay_llm(endpoint)`：差异只有**错误体形状**
（`/v1/messages` 用 Anthropic 形状，其余 OpenAI 形状）—— 端点能力差异随 Trae 补齐而消失。
命名空间把关（在册 / 非空 / **启用** / **授权**）也收敛成**唯一实现** `guard_namespace` ——
四道顺序不可换（「未启用」先于「未授权」：前者与 Key 无关，换把 Key 也没用）。
WB 侧再抽 `messages_for_region` / `responses_for_region`，Trae 侧再抽 `messages_for_variant`（同一手法）。
**回归**：gateway 全部测试目标 —— **467 单测 + 49 集成测试**全绿，零警告。

**`/biz/*path` 也已接通** ✅ —— 它是唯一**命名空间不出自请求体**的端点：业务请求没有模型名，
命名空间来自**统一 Key 的 `biz_namespace`**（不取请求头：真实调用方是客户端自身、加不了头；
也不隐式取「第一个 wb-*」：与「裸名一律拒绝」的哲学相反）。
读取侧**再查一遍**「在册 / 启用 / 授权」——写入路径校验过不等于读取路径可以省（手改台账能绕过）。
**七个路径全部已挂，`OWNED_PATHS` 与实现两侧一致**（有用例从两侧夹 + 钉死路由表本身）。
★ 通配符用 axum 0.7 的 `*path`：写错会在 **router 构建期 panic** 而 `cargo check` 全绿，
　故「真的构建 router」的用例是这一条的唯一防线。
**回归**：gateway 全部测试目标 —— **467 单测 + 49 集成测试**全绿，零警告。

**后端剩余**：管理面四处同步（`server/api.rs` `.route` + `src-tauri/commands.rs` + `generate_handler!`
+ `src/lib/api.ts` 的 `ROUTES`/`call()`）⇒ `npm run check:api`；src-tauri 侧宿主；前端「统一入口」页。
★ src-tauri 两处**在沙箱内编译不了**（本仓纪律：`-p` 排除它）⇒ 必须由能跑完整构建的环境验证。
- 抽 `pub(crate) relay_*`；新 `unified/dispatch.rs`：`namespace → variant → 对应 relay`。
- 流式：WB 侧 SSE 透传、Trae 侧转私有 SSE —— **都复用既有实现**。

### P4 · 契约收尾
- `/v1/models` 聚合（§4.1）；§4.3 五条错误；日志 `namespace`/`resolved_model`/`program`；禁用经 `region_of_variant`；`/status` 统一视图。
- 管理面**四处同步**（`server/api.rs` `.route`、`src-tauri/commands.rs`、`src-tauri/lib.rs::generate_handler!`、`src/lib/api.ts` 的 `ROUTES`+`call()`）⇒ 跑 `npm run check:api`。
- 旧端口加 §4.3-⑤ 的 400。

### P5 · 前端
**详细交互方案另立 `UNIFIED-GATEWAY-UI.md`**（落点、三张卡、状态矩阵、危险动作分档、验收钩子、扩展面）。
本节只记边界：**不动**账号页 / 用量页 / 两个既有网关页的其它区块 / `SCOPES`（4 个作用域不变）；
新增的只是网关页的**子导航第 7 项「统一入口」**。

### P4/P5 · 剩余工作（**2026-10-14 已全部收口**）

**① tauri 命令 · ② tauri 宿主 · ③ tauri 登记 · ④ 服务端路由/handler · ⑤ 前端 API 层全部已完成** ✅

| # | 改了什么 | 文件 | 已验证到什么程度 |
|:--|:--|:--|:--|
| 1 | 7 个命令实现 | `src-tauri/src/commands.rs` | ✅ **已编译**（2026-10-14 导出 EXE `v2026.10.141319`） |
| 2 | `UnifiedGatewayRuntime` 宿主 | 新建 `src-tauri/src/unified_gateway.rs` | ✅ 已编译 |
| 3 | `mod` + `manage` + 7 条 `generate_handler!` | `src-tauri/src/lib.rs` | ✅ 门禁**文本**校验 + 已编译 |
| 4 | 6 条路由 + 8 个 handler | `crates/buddy-switch-server/src/api.rs` | ✅ `cargo check` 零警告 |
| 5 | `ROUTES` 8 条 + 包装函数 + 3 个视图类型 | `src/lib/api.ts` | ✅ `check:api` + `tsc` |

**8 个命令**：`get_unified_gateway_config` · `save_unified_gateway_config` · `get_unified_gateway_models` ·
`list_unified_api_keys` · `create_unified_api_key` · `revoke_unified_api_key` · `restore_unified_api_key` ·
`delete_unified_api_key`（路径前缀 `/api/unified-gateway/…`）。**不进 `DEMO_READ_COMMANDS`**，
也不进 `SCOPE_PARAMETERIZED`（跨作用域）。

**两件都已收口**：

1. ✅ **前端页面已落地** —— 网关页子导航第 7 项「统一入口」，三张卡（统一网关 / 统一密钥 / 命名空间与清单预览），
   `src/components/gateway/unified-gateway-view.tsx`。交互规格见 `UNIFIED-GATEWAY-UI.md`。
2. ✅ **src-tauri 已真实编译** —— 2026-10-14 导出 EXE `v2026.10.141319`（安装包 + 便携版，双侧进包取证 + 验签）。
   原先「只有门禁文本校验兜着」的未闭合风险已消除。

★ 三条实现约定（后续加命令必须照做）：
- **服务端读 body 时 camelCase 与 snake_case 都收**（前端发 camelCase；服务端是手工读 body，
  不像 Tauri 那样自动映射）—— `bizNamespace` / `biz_namespace` 两种写法都认。
- 跨平台命令的能力门控取 `Product::Workbuddy` 的 `gateway` 能力（桌面端恒具备），
  并在注释里写明**这不是「它属于 WorkBuddy」**。
- 工作顺序仍是 `tauri → server → api.ts → 页面`：门禁是单向的（`ROUTES → server/tauri`），
  按此顺序中途停下门禁仍绿。


### P6 · 验收（判据不许 grep 源码字符串）
1. `tsc` 空输出 · 门禁绿 · **变异验证**：把 §2.3 的 `trae-cn` 改成 `TraeProgram::TraeWork` ⇒ P1-② 护栏**真红**。
2. 真起**三个**监听（57891 / 7864 / 57892），打 §4.2 的 5 条请求：① 全 200；② 请求日志里 `namespace` 与**实际命中的账号归属域 + 上游 `function`** 一致（这是「真分派到对的池 + 对的 function」的唯一证据）。
3. §4.3 五条错误逐条复现。
4. `/v1/models` 在 4 段各自缺失/降级时都能给出**非空 note**（阳性对照：种一个「Trae 客户端没启动过」的场景）。
5. 回滚验证：关掉统一网关 ⇒ 57891 / 7864 **行为与今天逐字一致**。

---

## 8. 风险清单

1. **axum 同路径 merge panic** ⇒ 第三产品线 + 内部分派，永不合 router；加护栏。
2. **`trae-cn` / `trae_cn` / `trae_merged` 字形相近**（§2.6 规则 2）⇒ 只查显式表 + 字面量隔离护栏。
3. **Trae 清单依赖客户端已启动**（`state.vscdb`）⇒ 缺段必须回落 + **标注来源**，不能假装完整。
4. **默认的 `trae-cn` 指合并版** —— 若你的机器上 Trae **还没升级/没合并**，它读不到清单（回落静态）。此时应改用 `traework-cn` / `traecode-cn`。**UI 要把这条提示写进命名空间预览。**
5. **`max_body_mb` 构建期固化** ⇒ 改值要重启。
6. **禁用共用一份** ⇒ 统一入口禁用会影响旧端口（语义正确，但**必须在 UI 说明**）。
7. **Trae 的 `function` 必须与命名空间的程序位同源** ⇒ 否则复现 issue #4（列得出来调不动）。§2.6 规则 1 就是为此。

---

## 9. 端口与文件速查

| 项 | 值 |
|:--|:--|
| 统一网关（新） | `127.0.0.1:57892` |
| WB 网关 / Trae 网关 / webui / Trae MITM | `57891` / `7864` / `57890` / `8899` |
| **预留（后续平台）** | `57893+`（若某新平台要独立端口）；**默认不给新平台开端口** —— 只走统一入口即可（见 §11-D） |
| 统一网关配置 / 密钥 | `~/.buddy-switch/gateway_unified.json` / `gateway_unified_keys.json` |
| 共用：禁用偏好 | `~/.buddy-switch/gateway_disabled_models.json`（键 = region，**三入口共用**） |
| 既有：WB / Trae 密钥 | `~/.buddy-switch/gateway_keys.json` / `trae/api_gateway_keys.json` |

## 10. 决策记录（**已定 —— 由我按「最合理 + 最易扩展」判定，附理由与本机实测证据**）

### D1 · 默认只启用 4 条命名空间 ｜ 决定：**是**
老程序线（`traework-*` / `traecode-*`）**默认关闭**，在 UI 上一勾即开。
**证据（本机实测）**：合并迁移已完成 —— `%APPDATA%\Trae CN\migration-job.json` 的 `status = "completed"`、
`sourceAppName = "TRAE SOLO CN"`、`targetDataPath = …\Trae CN`。
⇒ 四个 userData 目录都在，但**合并版客户端的清单里只有 `chat_v3` 等 7 组，没有 `solo_work_lite`**
（`Trae CN` 的 `chat_v3` = **19 条**，含 `deepseek-v4.1-flash`；国际 `Trae` 同为 7 组）。
⇒ 默认开 `traework-*` 一定读不到清单、只能回落静态清单 = 假可用。**默认 4 条是唯一正确解。**

### D2 · 字面量：**取消三段式，一律两段 `{名字}-{区域}`** ｜ 决定：**是**
名字取值：`wb`（既有平台短标识，与 `PlatformSlug` / `sk-wb-` 同源）· `trae`（**合并版客户端的名字**）·
`traework` / `traecode`（两条老程序线的客户端名）。

```
默认：  wb-cn        wb-global        trae-cn        trae-global
可选：  traework-cn  traework-global  traecode-cn   traecode-global
```

比 v3 的三段式更好的三点：① **8 条同构**，没有「两段/三段」两套规则；② 每条都**就是客户端自己的名字**
（`Trae CN` / `TraeWork` / `TraeCode`），逐字可核对；③ 少一层解释成本。
★ **新增护栏**：`trae` / `traework` / `traecode` **互为前缀**（`traework` 以 `trae` 开头）⇒
解析**必须整段精确匹配，禁止 `starts_with`**。

### D3 · 禁用三入口共用一份、按底层 region ｜ 决定：**接受**
与 Trae 侧既有口径**同源**（`region_of_variant` 的存在理由就是「禁用存储是 region 粒度」），零重复、语义正确
（同一个上游模型）。代价是「统一入口禁 `wb-cn:x` 会同时影响 57891」⇒ **UI 必须显式说明**（UI 方案 §6）。

### D4 · `/biz/*path` 的命名空间来源 ｜ 决定：**统一 Key 的 `biz_namespace` 字段（B1）**
显式优于隐式（与「裸名拒绝」同一哲学）；且真实调用方是客户端自身，**加不了自定义头** ⇒ 请求头方案实务不可用。

### D5 · 前端落点 ｜ 决定：**网关页子导航第 7 项「统一入口」**
依本仓既有先例：**「Key 表与请求日志不做版本过滤 —— 它们是跨版本的管理视图」**。统一入口与它们同性质。
成本最低（`GATEWAY_SUBNAV` 表加一项，两平台页自动都有；Trae 页的 `filter` 不受影响），
且**不碰** `DESTINATIONS` / 侧栏 / 路由 / P0–P12 成果。
⚠️ 必须补一行说明「本入口跨全部作用域，上方作用域选择器对它不生效」—— 否则用户切作用域没变化会当成 bug。

### D6 · 旧端口加「看起来像命名空间但不认识」的 400 ｜ 决定：**加**
一次极小的改动，把一个死胡同（用户只看到上游报「模型不存在」）变成有出路的提示。

### D7 · A 路 vs B 路 ｜ 决定：**先 B 路 spike，再上 A 路**
与本仓 P12 的做法一致（spike 先行）。spike 的唯一目标是**拿到「前缀真分派到对的池 + 对的 function」的证据**；
拿到后 A 路重构（抽 `pub(crate) relay_*`）才有意义 —— 否则可能白重构。

### D8 · 本机 Trae 客户端判定 ｜ 决定：**用 `trae-cn` / `trae-global`（合并版）**
实测证据（见 D1）：
- `Trae CN`（国内合并版）清单存在，分组 `builder / builder_v3 / chat_v3 / code_reviewer / code_review_summary / refactor / solo_agent`，`chat_v3` **19 条**，含 `deepseek-v4.1-flash`；其中 1 条是第三方 bypass（`openrouter//stealth/ox-alpha`，既有的 `servable_entries` 会正确过滤）。
- `Trae`（国际合并版）同为 7 组。
⇒ `trae-cn:deepseek-v4.1-flash` 与 `trae-global:*` **本机可直接用**，无需 `traework-*`。

> ⚠️ 八条全部有结论。**唯一仍未核实**：`trae-global`（国际合并版）的凭据从未对真实上游跑通过（本仓既有诚实边界，非本次引入）—— 它可能「列得出来、调不动」。启用前先发一条真实请求验证。

---

## 11. 新增一个平台（如 `qoderwork`）的可扩展性契约

> 需求来源：用户明确说「后续我还会添加 qoderwork 平台，请注意做好方便拓展」。
> 本节的目的是让「加一个平台」= **加声明**，而不是「翻一遍网关代码」。

### A. 命名不能我方起 —— 取客户端自己的名字
按本仓既有纪律（`trae-client-merge`：**名字取客户端自己的名字，禁由标识反推**）：
`qoderwork` 这个字面量要**以客户端实机证据为准**（`product.json` 的 `nameAlias` / userData 目录名 / exe 名）。
⇒ **接入第一步是探测**，不是拍名字。若它只有一个区域，命名空间就用**单段**（`qoderwork`），
**不硬塞 `-global`**（表内显式，单段天然可表达）。

### B. 派生层：加三处声明，命名空间**自动出现**（零手写）

| 落点 | 内容 |
|:--|:--|
| `core/modules/product.rs` | `Product` 加 variant + `Scope` 加对应值 + `ProductSpec` 一条（目录名 / label / 能力） |
| `src/lib/platforms/registry.ts` | `Product` 联合类型 + `PlatformSlug` + `PLATFORM_SLUG` / `SLUG_TO_PRODUCT` / `PLATFORM_DESCRIPTORS` 各加一条 |
| `src/lib/scope.ts` | `SCOPES` 加对应作用域（`Scope` 类型是模板字面量，自动覆盖） |

仓库自带的注释已经把这个契约写在 `PLATFORM_DESCRIPTORS` 上：
> 「新增平台 = 这里加一条 + `scope.ts` 的 `Scope` 加对应值，**页面组件零改动**。」

⇒ 完成后，`qoderwork-cn` / `qoderwork-global` **自动**成为合法命名空间（见 §2.7 清单）。

★ **既有门禁会替你抓漏**：`scripts/check-scope-registry.cjs` 校验
`product → slug → product` 往返、以及**每个 product 声明的 region 都能拼出合法 scope**（`scope.ts` 与 core 双向）；
core 的 `product_spec_matches_frontend_registry_slug_and_capabilities` 校验跨语言一致。
★ **本次要新加的两条护栏**：
1. `Scope::all()` 的**每个 scope 都必须在命名空间表里解析得到**（加 scope 忘了命名空间 ⇒ 红）；
2. 命名空间集合里**任意两条 id 不得互为前缀**（`trae` / `traework` 这类 ⇒ 红，防止有人用 `starts_with` 实现）。

### C. 必须手写的三件（**编译器盯着，漏了编译不过**）

1. **relay 分派**：`unified/dispatch.rs` 的 `match` 加分支。
   `NamespaceTarget` 与 `Product` 两处 `match` **一律禁止 `_ =>` 兜底分支** ——
   这样加了平台/目标轴，**所有落点都会被编译器点名**（这正是本仓「全量遍历、禁漏分支」的既有纪律）。
2. **模型清单来源**：该平台的 `models_for(namespace)`（上游探测或客户端缓存，二选一由该平台决定）。
   ★ 沿用既有契约：**非实时快照的 `note` 恒非空**（没清单时必须说明为什么）。
3. **管理面命令**（若该平台要有自己的运行状态/开关）：四处同步
   （`server/api.rs` `.route`、`src-tauri/commands.rs`、`src-tauri/lib.rs::generate_handler!`、`src/lib/api.ts` 的 `ROUTES`+`call()`）⇒ 跑 `npm run check:api`。

**若该平台还有第二条程序线**：命名空间扩展层（§2.6）加行 ⇒ `NamespaceTarget` 加 variant（编译期强制补 match）。

### D. 端口：**新平台默认不单独开端口**
两平台之所以各有端口是「同一个 `/v1/chat/completions` 在 axum 里不能 merge」的历史结果。
新平台**只走统一入口**即可（`qoderwork-cn:xxx`），省一个端口、省一份配置、省一份 Key 库。
真要独立端口，从 **`57893` 起**取，并遵守 `GatewayProduct` + `OWNED_PATHS` + 独立 Key 库的既有契约。

### E. 账号与治理：**不新造，实现既有跨产品线契约**
新平台要接进统一网关，上游侧必须提供：
① 账号库与凭据刷新（照 `region.rs` / `trae/variant.rs` 的模式，**用 `all_specs()` 全量遍历**）；
② `impl PoolEntryLike`（`pool/entry_like.rs` —— **跨产品线契约**，选号内核两线共用的前提）；
③ 自己的 `PickPolicy`（**绝不改默认值** —— 默认值就是 WorkBuddy 的行为基线）；
④ 上游 wire + 错误分类（`UpstreamErrorKind`），**别在 gateway 里另写一套轮换逻辑**；
⑤ `BackgroundTask` 登记（**两侧各一份表**，漏了就是「登记即执行」那类事故）。
完整清单见 `docs/guides/GATEWAY-CLIENT-INTEGRATION.md` §4.3。

### F. UI：**表驱动，加平台基本白送**
见 `UNIFIED-GATEWAY-UI.md` §9。唯二要加的是：命名空间集多一个**平台分组**（自动，按 `product` 分组渲染）、
以及该平台自己的账号/设置页面（若它要有完整管理面 ⇒ 那是 B 档，可后补，**不阻塞它先跑通统一入口**）。

### G. 复核过的一处既有耦合（接入新平台时要注意）
`src/lib/platforms/registry.ts` 的 `PlatformDescriptor.regions` 类型被**写死成 `("cn" | "global")[]`**，
而 `Scope` 是 `${PlatformSlug}-${Region}`。
⇒ 若新平台的区域取值**超出 `cn` / `global`**（例如 `us` / `sg`），要么映射到既有两个值，
要么**先扩 `Region`**（影响面：core `region.rs` + 前端 `lib/region.ts` + `SCOPES` + 全站 URL）。
**这不是本次要改的**，只是接入时不要踩空。
