# 缓存命中率：三平台 × 两本账 实现方案

> 状态：**P0/P2 已落地**（2026-11-05，issue #154 / PR #168 #169）· P1/P3 未做
>
> 已落地的是 §11 的 **P0 探针**（三平台日志落 `cachedTokens`）与 **P2 网关账本**
> （`token_daily` / `gateway_ledger` 的 `cached` 维度 + 出口 `null`≠`0` + 迁移不播种历史）。
> **P1（客户端三桶）与 P3（UI 全量）仍未做** —— 当前 UI 只在网关账本下出命中率，
> 客户端账本的命中率仍是 `token_stats` 原有那一列。§12 的四个待拍板项里，
> 第 3 项（`to_openai()` 透传）与第 4 项（模型表给率）已按「给」实现，另两项仍待定。
>
> ⚠️ 上面「第 3 项已实现」是第一版写错后补上的：当时 `TokenUsage.cached` 已经解析出来
> 并进了账本，但 `to_openai()` 仍然只吐三个键 ⇒ Trae 线的客户端根本读不到命中量。
> 现在 `to_openai()` 在 `cached > 0` 时补 `prompt_tokens_details.cached_tokens`
> （没命中量时整个键不出现，不写 0），由 `to_openai_carries_cached_tokens_when_observed`
> 与「无命中时键不存在」两条断言钉住。
>
> 目标：WorkBuddy / Trae / Qoder 三个平台，**客户端账本与网关账本都能给出缓存命中率**，且「算不出来」时如实显示「无数据」而不是 0%。

---

## 0. 一句话结论

**结构上三个平台的两本账都能接（原始 `usage` 对象都已经握在手里），但数据上只有 WorkBuddy 能确定拿到数**；Trae / Qoder 取决于上游是否回传缓存字段，必须先用探针实测（§8），再决定是否值得投入账本与 UI。

---

## 1. 目标与非目标

### 目标

| # | 目标 | 判据 |
|:--|:--|:--|
| G1 | 三平台**网关账本**能出命中率 | 有上游读数时，窗口/逐日/按模型/按账号都能算 |
| G2 | 三平台**客户端账本**能出命中率 | WB 立即有；Trae/Qoder 有数据时自动亮 |
| G3 | 「未上报」与「命中 0」严格分开 | UI 分别是「—」与「0.0%」，不共用一种表现 |
| G4 | 合并（多区域 / 多来源）后率正确 | 率不得相加，必须按合并后的分子分母重算 |
| G5 | 历史不可补时**如实披露** | 覆盖度（有几天的数据）与数据同屏 |

### 非目标（明确不做，避免把口径搅浑）

- ❌ **网关响应缓存 / 语义缓存**（GPTCache 那类）：命中率可以 100%，但答案可能是旧的，与「上游前缀缓存」是两个概念，将来若做必须另起名字与卡片。
- ❌ 模型列表缓存、Codex 磁盘缓存、Trae replay 缓存 —— 都与计费无关，不进本指标。
- ❌ 用 tokenizer 估算缓存量（那是伪造）。
- ❌ 「按请求条数的命中率」（有命中请求数 / 总请求数）：与计费口径不一致，只做 token 级。

---

## 2. 取证（2026-11-03 本机实测，可复核）

| 数据源 | 条目 | 有缓存读数 | 关键数字 |
|:--|:--|:--|:--|
| `~/.workbuddy/projects/**/*.jsonl`（WB 客户端） | 442 条 usage | **440 条** | input 53,125,043 / cacheRead 51,172,848 ⇒ **96.33%** |
| 同上，口径校验 | 442 条 | — | **442/442 满足 `cache_read ≤ input_tokens`**；`cache_creation` 出现 0 次 |
| `~/.buddy-switch/gateway_logs.json`（WB 网关） | 200 条 | **0 条** | 运行版本早于 #154，日志里没有 `cachedTokens` 键 |
| `~/.buddy-switch/trae/api_gateway_logs.json` | 200 条 | **0 条** | 75 条 `promptTokens > 0`（37.5%），其余为失败/无用量 |
| `~/.buddy-switch/qoder/logs/gateway_requests.jsonl` | 7 条 | **0 条** | 仅 1 条 `promptTokens > 0`（其余 502/429） |

代码侧事实：

- `UsageReport.cached_tokens` 已存在（`crates/buddy-switch-gateway/src/protocol/usage_tap.rs:58`），三种上游形状已归一（`:117-128`），**但类型是 `i64`，「没报」与「报 0」不可区分**，且越界时被 `.min(prompt)` 静默压成 100%。
- Trae：`TokenUsage::from_value(&raw)`（`crates/buddy-switch-gateway/src/trae/sse.rs:184`）拿到的是**原始 usage 对象**，只读了 4 个键（`:23-46`）；`to_openai()`（`:48-55`）只回写 3 个键。
- Qoder：`UsageTapStream` 把**原始 `usage` Value** 存下来（`crates/buddy-switch-gateway/src/qoder/sse.rs:249,389`），记账时只读 `prompt_tokens` 与 `credits`（`qoder/routes.rs:961,976-981`）。
- Trae/Qoder 的 `record_request` 都没有缓存参数（`trae/mod.rs:498`、`qoder/mod.rs:497`）。
- 仓库里**没有一条真实上游 usage 夹具**：所有 `"usage": {...}` 都是手写最小对象，无法证明真实响应形状。
- `log_bodies` 是**死开关**：`RequestLog::log_bodies()` 全仓无消费者 ⇒ 从未落过正文，历史原始帧不可得。
- 统一网关无独立账本（`crates/buddy-switch-gateway/src/unified/` 无 `RequestLog` / `record_request`）⇒ 中继流量落在产品线账本，**无需单独实现**。

---

## 3. 口径（唯一定义，三平台两本账共用）

```
cacheRead            分子：上游返回的「从 provider 缓存读取」的输入 token
cacheObservedInput   分母：**只累计上报过缓存字段的那些请求**的 input
cacheRequests        覆盖度分子：上报过缓存字段的请求条数
requests             总请求条数（已有字段）

命中率 hitRate  = cacheRead / cacheObservedInput        （分母 0 ⇒ null）
覆盖度 coverage = cacheRequests / requests              （requests 0 ⇒ null）
```

### 为什么分母不是 `input`

上游只报一部分请求时（Trae 实测 37.5% 有 token 读数、Qoder 1/7），拿全部 input 当分母会把真实 90% 稀释成 30%，看起来像「缓存很差」。opencodex 的 `cacheObservedInputTokens` 就是这条判据。

### 输入口径归一（OTel 标准）

OpenTelemetry GenAI 语义约定明确 `gen_ai.usage.input_tokens` **SHOULD include cached tokens**，`cache_read` / `cache_creation` 是其子集。因此：

- 内部一律按**含缓存**口径存 `input`（现状即如此，`uncachedInput = input - cacheRead`）。
- 上游若给**不含缓存**的口径（Anthropic 原生：`input_tokens` 不含 `cache_read`），必须在提取层归一成 `input_total = input + cached`。
- **判据**：`cached > input` 只可能出现在「不含缓存」口径下 ⇒ 归一；否则维持。
- 现状的 `.min(prompt)`（`usage_tap.rs:128`）会在该情形下把分子压成 100%，**是静默错**，必须改掉。

### 一条请求是否进分母

| 情形 | 进分母 | 说明 |
|:--|:--|:--|
| 上游返回了缓存字段（含 0） | ✅ | 「报了 0」是有效观测：确实没命中 |
| 上游没返回该字段 / 请求失败无 usage | ❌ | 未观测，不算 miss |
| 流被掐断但已见 usage | ✅ | `UsageTap` 在 Drop 时仍上报（`usage_tap.rs:320-328`） |

---

## 4. 数据通路：六条链

| 平台 | 账本 | 数据源 | 现状 | 改动量 |
|:--|:--|:--|:--|:--|
| WB | 客户端 | jsonl（`token_stats`） | ✅ 有真值 | 小：加两桶 + 输出 |
| WB | 网关 | `UsageTap` → 日志 → `token_daily` | 半：日志有、账本无 | 中 |
| Trae | 客户端 | sqlcipher 导入（`client_import`） | ❌ 只有 input/output | 无（拿不到） |
| Trae | 网关 | SOLO SSE `usage` → 日志 → `gateway_ledger` | ❌ 未读、未落 | 中 |
| Qoder | 客户端 | `qoder/usage.rs` | ❌ input/output/cache 全 0 | 无（拿不到） |
| Qoder | 网关 | 原始 `usage` → 日志 → `gateway_ledger` | ❌ 未读、未落 | 中 |

### 4.1 WB 网关：`Option` 化 + 落账

| 文件 | 改动 |
|:--|:--|
| `protocol/usage_tap.rs:58` | `cached_tokens: i64` → `Option<i64>` |
| `protocol/usage_tap.rs:91-130` | 读不到 ⇒ `None`；`cached > prompt` ⇒ `prompt += cached`（归一，不再 clamp）；`cached <= prompt` ⇒ 原样 |
| `protocol/usage_tap.rs:281-306` | ★ **`observe_frame` 由「整块覆盖」改为逐字段合并**：后到的 `Some` 覆盖，`None` / 0 不得抹掉已有非 0 值（否则末帧只带 output 时会把已见到的缓存数打成 0）。依据见 §13.2 |
| `logging.rs:79,119` | `cached_tokens: Option<u64>`，序列化 `cachedTokens: null \| number`（键固定，与 `keyId`/`error` 同纪律） |
| `logging.rs:184-219` | `StreamingLogSink::finish`：`None` 写 `null`，不写 0 |
| `routes/{chat,messages,responses}.rs` | 5 处 `cached_tokens: 0` 占位改 `None` |

### 4.2 Trae 网关：读出来 + 透传出去

| 文件 | 改动 |
|:--|:--|
| `trae/sse.rs:23-46` | `TokenUsage` 加 `cached: Option<u64>`，复用同一个 `cached_tokens_of` helper（四形状） |
| `trae/sse.rs:48-55` | `to_openai()` 追加 `prompt_tokens_details.cached_tokens`（**可选**，改动对外 wire，需单测；依据「能透传就透传」原则） |
| `trae/routes.rs:1610-1670` | 记账调用多传 `cached` |
| `trae/mod.rs:498-545` | `record_request` 加 `cached_tokens: Option<u64>`，日志写 `cachedTokens` |
| `trae/routes.rs:1361-1375` | 非流式路径同样取（当前只读 `total_tokens`） |

### 4.3 Qoder 网关：读出来（大概率仍算不出）

| 文件 | 改动 |
|:--|:--|
| `qoder/routes.rs` | 新增 `cached_tokens_of(&usage)`，与 WB/Trae 同一 helper（**唯一实现，三处调用**） |
| `qoder/routes.rs:1083-1101,1172-1215` | 记账分支多取 `cached` |
| `qoder/mod.rs:497-540` | `record_request` 加 `cached_tokens: Option<u64>`，日志写 `cachedTokens` |
| `qoder/routes.rs:1246` | Anthropic 出口的 `usage` 可补 `cache_read_input_tokens`（可选） |

> ⚠️ Qoder 即便拿到 `cached`，`promptTokens` 常为 0（实测 6/7 条）⇒ 分母 0 ⇒ 命中率不可算。**它大概率长期显示「—」**，这一点必须在拍板时接受。

### 4.4 两份网关账本

| 文件 | 改动 |
|:--|:--|
| `token_daily.rs:144-149` | `Bucket` 加 `cache_read` / `cache_observed_input` / `cache_requests`（**只加网关桶**，客户端桶不动 —— 见下注） |
| `token_daily.rs:160-171` | `DimBucket` 加同三字段；`add` 逐字段加、`merge_max` 逐字段取大 |
| `token_daily.rs:500-560` | `GatewayRow` 加 `cached: Option<u64>`，解 `cachedTokens`（缺键 ⇒ `None`） |
| `token_daily.rs:600-648` | `bucket_gateway` / `bucket_dim` 累加三字段（`None` 不加分母、不加覆盖度） |
| `token_daily.rs:1602-1690` | `gateway_usage_dim` / `dim_groups` 输出三字段；`gateway_trend_points` 同步 |
| `token_daily.rs` `DayRecord` | 加 `gateway_cache_partial: bool`（当日起始水位线之前的条目未观测 ⇒ 当日只是下界） |
| `gateway_ledger.rs:180-217` | `DimBucket` 加三字段 + `add` / `merge_max` |
| `gateway_ledger.rs:361-374,431-486` | `Row` / `parse_rows` 同上 |
| `gateway_ledger.rs:736-757` | `dim_groups` 输出三字段 |
| `gateway_ledger.rs:62` | `LEDGER_VERSION` 2 → 3 |
| `gateway_ledger.rs` `DayRecord` | 加 `cache_partial: bool`（照 `keys_partial` 先例） |

> **为什么客户端桶不加缓存**：`token_daily` 的客户端桶与 `token_stats` 是**同一批 jsonl 的两个解析器**（`token_daily.rs:924-969` vs `token_stats.rs:194-206`）。再给前者加缓存字段就是第三份口径，必然漂移。用量页的客户端数字一律走 `token_stats`，今日卡不显示命中率。

### 4.5 客户端账本（`token_stats`）

| 文件 | 改动 |
|:--|:--|
| `token_stats.rs:26-39` | `Usage` 加 `cache_observed_input` / `cache_records` |
| `token_stats.rs:66-87` | `Totals::add` / `merge` 同步 |
| `token_stats.rs:89-113` | `value()`：`cacheHitRate = cacheRead / cacheObservedInput`（分母 0 ⇒ null）；新增输出 `cacheObservedInput` / `cacheRequests`；`uncachedInput` 保留（兼容） |
| `token_stats.rs:139-171` | `cached_input_field` 改为返回 `Option<u64>`（区分「没报」）；`usage_fields` 据此累加 `cache_records` |
| `token_stats.rs:1422-1430` | `SUMMED_SUMMARY_FIELDS` 加 `cacheObservedInput` / `cacheRequests`（漏加 ⇒ 合并区域后整块归零） |
| `token_stats.rs:1476-1507` | `recompute_cache_hit_rate` / `merge_summary` 改用新分母 |
| `token_stats.rs` 越界守卫 | `read > input` ⇒ 视为「不含缓存」口径，`input += read`（与网关同一判据） |

---

## 5. 存储与 API 契约

### 5.1 日志条目（三个网关文件，键固定）

```json
{
  "promptTokens": 1200,
  "completionTokens": 300,
  "cachedTokens": 900,          // null = 上游没报（不是 0）
  "credit": 0.42
}
```

### 5.2 客户端分组 / 汇总（`token_stats` 既有形状 + 三个新键）

```json
{
  "input": 1200, "output": 300, "total": 1500,
  "cacheRead": 900, "cacheWrite": 0, "uncachedInput": 300,
  "cacheObservedInput": 1200, "cacheRequests": 42,
  "cacheHitRate": 0.75
}
```

### 5.3 网关维度行（`GatewayDimEntry`，`token_daily` 与 `gateway_ledger` 逐字同形）

```json
{
  "model": "deepseek-v4.1-flash",
  "input": 1200, "output": 300, "total": 1500,
  "requests": 42, "failed": 1, "credit": 0.42,
  "cacheRead": 900, "cacheObservedInput": 1200, "cacheRequests": 40
}
```

### 5.4 逐日行

在 5.3 之外加 `cachePartial: bool`（该日有部分条目在观测开始之前写入 ⇒ 当日数值是下界）。

### 5.5 字段名纪律

- 账本与分组**统一叫 `cacheRead`**（不叫 `cached`），与客户端既有字段同名 ⇒ 前端只有一个映射。
- `cacheHitRate` 由后端在**单来源**时给出（外部 API 消费者可用），但前端**合并后必须自算**（公式逐字相同，两处各有单测）。前端不读后端那个字段。

---

## 6. 迁移与历史

| 账本 | 历史可补？ | 处理 |
|:--|:--|:--|
| WB 客户端 | ✅ 可 | jsonl 全量重扫，历史命中率立刻可用 |
| WB 网关（`token_daily`） | ⚠️ 仅当日 | 三字段 `serde(default)` 读成 0；`gateway_watermark_ts` 之前的条目永不重读 ⇒ 加一次性**播种轮**：读整窗日志，`cache_read = max(现有, 窗口合计)`（与 `GatewayWrite::Correct` 同语义），并置 `cache_partial = true` |
| Trae/Qoder 网关（`gateway_ledger`） | ⚠️ 仅当日 | 同 `keys_partial` 先例：播种今天，其余日 `cachePartial = true` |
| Trae 客户端导入 | ❌ | 无字段，永远 `cacheRequests = 0` |
| Qoder 客户端 | ❌ | token 恒 0 |

> **不倒退、不双计**：播种用 `max` 不用 `+`（两个读数都只是当日子集，相加会双计），与 `merge_max` 的既有理由一致（`gateway_ledger.rs:207-216`）。

---

## 7. UI

### 7.1 三平台趋势视图：新增第 6 / 第 5 张卡

| 状态 | value | hint |
|:--|:--|:--|
| 有数据 | `96.3%` | `缓存 51.2M / 输入 53.1M · 覆盖 98%` |
| 上游未上报 | `—` | `上游未回传缓存字段` |
| 本账本无此维度 | `—` | `该账本按 credits 计费，无 token 读数` |
| 账本 = 全部 | 客户端口径 | hint 追加 `网关 <x% 或 —>`（**不合成**：两本账重叠，合成率是假的） |

- `src/components/usage/stat-tiles.tsx:72-77`：列数档从 `5/4` 改为按 `tiles.length` 取（新增 `6`）。
- `src/index.css:421-438`：补 `[data-grid-cols="6"]` 的 1100px / 760px 两档。
- i18n：`src/locales/domains/wbStats.zh.ts` / `.en.ts`，复用 `wbStats.usage.trend.*`（三平台已共用该命名空间）。

### 7.2 模型视图

- 客户端排行条（`model-ranking.tsx`）：每行右侧加 `96.3%`，仅当该行 `cacheRequests > 0`。
- 网关表（`LedgerModelTable`）：加「缓存」列，同样按行判空。

### 7.3 会话视图（WB / Qoder）

`session-ranking.tsx` 加一列命中率（`sessions[].cacheRead` 已有）；Qoder 恒空 ⇒ 不渲染该列。

### 7.4 原型（先于代码）

`docs/prototype/demo-app.js:1520-1541` 的 `ledgerTiles` 三个分支各加一块 → `npm run proto:single` 重新生成 `buddy-switch-prototype.html` → `npm run check:proto`。

---

## 8. 探针（Phase 0）：把「未知」变成「已知」

**不改账本、不改 UI**，只做 4.1 / 4.2 / 4.3 的日志落字段，然后跑真实流量：

```bash
# 各平台发一条真实请求后
grep -o '"cachedTokens":[^,]*' \
  ~/.buddy-switch/gateway_logs.json \
  ~/.buddy-switch/trae/api_gateway_logs.json \
  ~/.buddy-switch/qoder/logs/gateway_requests.jsonl | sort | uniq -c
```

| 输出 | 判定 | 后续 |
|:--|:--|:--|
| WB 出现数字 | 上游确实回传 | WB 全量做 |
| Trae 出现数字 | 上游回传（推翻 `trae/token_stats.rs:494` 的旧声明） | Trae 全量做，并删掉那条 `unsupported_note` |
| Trae 全 `null` | 上游不报 | 只做结构，UI 显示「上游未上报」 |
| Qoder 全 `null` 或分母 0 | 同上 | 同上；`unsupported_note` 文案改为「上游未上报」而非「无从记录」 |

**成本 0.5 天，收益：决定后面 3 天的工作是否值得做。** 这一步也是唯一能证伪「Trae/Qoder 拿不到」的手段 —— 现有结论只是代码里的一句声明，没有实测。

---

## 9. 门禁与测试（每条可证伪）

### Rust

| 断言 | 变异（改坏 ⇒ 红） |
|:--|:--|
| `extract_usage` 四形状（DeepSeek / OpenAI / Anthropic / 扁平 `cached_tokens`）都读到 | 删掉任一分支 |
| `cached > prompt` ⇒ `prompt += cached`（归一，不是 clamp） | 改回 `.min(prompt)` |
| 字段缺失 ⇒ `None`；显式 0 ⇒ `Some(0)` | 改回 `unwrap_or(0)` |
| `DimBucket::add` / `merge_max` 覆盖三个新字段 | 漏一个字段 |
| 旧账本 JSON（无新键）反序列化成功且为 0 | 去掉 `serde(default)` |
| `dim_groups` 输出三字段 | 删任一键 |
| 播种轮用 `max` 而非 `+`（同日两次记录不双计） | 改成 `add` |
| `token_stats` 合并后 `cacheHitRate` 按新分母重算 | 沿用 `read/(read+uncached)` |

### 前端

| 断言 | 位置 |
|:--|:--|
| `hitRateOf()`：分母 0 ⇒ `null`，不返回 0 | `src/lib/usage-charts.ts` 新纯函数 |
| `addTotals` 合并后 `cacheHitRate` 为 `null`（由视图重算，不得相加） | 既有约定 |
| `groupOf`（`usage-ledger.ts:545`）传真实值，不再写死 `cacheRead: 0` | 连带改注释 |
| `cacheRequests === 0` 时不渲染百分比 | 组件级断言 |

### 门禁

- `scripts/check-scope-registry.cjs:6189-6198`：`usageSource` 夹具补 `cacheRead` / `cacheObservedInput` / `cacheRequests`。
- 新增两条断言：①「率不得直接相加」；②「`cacheRequests == 0` 不得渲染 `%`」。
- `npm run build` 必须过（`check:scope` 会读原型文件）。

---

## 10. 风险

| # | 风险 | 处置 |
|:--|:--|:--|
| R1 | Trae/Qoder 上游不报 ⇒ 功能上线仍显示「—」 | 先跑 Phase 0；结构照样落（避免以后再迁一次库） |
| R2 | 归一化写错会把输入总量算大 | 只在 `cached > input` 时触发，单测钉住，且有实测 442/442 基线 |
| R3 | 账本 version 升级 + 播种导致当日数字跳变 | `max` 语义 + `cachePartial` 披露；不回退 |
| R4 | 合并（多区域）后率相加 | 前端一律重算；后端 `merge_summary` 同公式 |
| R5 | 两处公式实现漂移 | 公式钉死在本文件 §3，两侧各有单测 |
| R6 | Qoder 分母常 0 被误显示成 0% | UI 硬规则：`cacheRequests == 0` ⇒ `—` |
| R7 | 改动 `to_openai()` 影响对外 wire | 标为可选子项，单独提交 + 单测 |

---

## 11. 分期与验收

| 阶段 | 内容 | 量 | 验收 |
|:--|:--|:--|:--|
| **P0 探针** ✅ | `Option` 化 + 三平台落 `cachedTokens` + 上面那条 grep | 0.5d | 三个日志文件的 `cachedTokens` 分布已知 |
| **P1 客户端** | `token_stats` 三桶 + 越界守卫 + API | 1d | WB 窗口命中率 = 96.3%±，与手算一致 |
| **P2 网关账本** ✅ | `token_daily` + `gateway_ledger` + 迁移播种 | 1.5d | 三平台账本逐日可出数；旧库升级不报错、不倒退 —— **已落地**（#169）：`cached` 进账号桶与四条维度桶，出口 `null`≠`0`，**刻意不回填历史**（见 §6） |
| **P3 UI** | 三平台卡 + 模型表 + 会话列 + 原型 + i18n | 1d | 空态显示「—」，有数据与手算一致 |
| **P4（已否决）** | 「省了多少积分」：**前置条件不满足** —— `gateway_models.*.json` 只有 `credits: "x0.11 credits"` 倍率文案，没有 per-token 单价，上游也只给总积分（见 §13.4） | — | 不做，避免编数 |

**总：4–4.5 天**（不含 P4）。

---

## 12. 待拍板

1. **P0 之后若 Trae/Qoder 都是 `null`**：仍按 P2 全量落账本（多 1 天，换「以后上游改了自己亮」），还是只落 WB、另外两个只留日志字段？
2. **Qoder 是否纳入**：它按 credits 计费，命中率大概率永远「—」。
3. **`to_openai()` 是否透传 `prompt_tokens_details.cached_tokens`**：透传能让下游客户端也看到缓存量（与 WB 一致），但改动对外 wire。
4. **模型表要不要给率**：opencodex 有意只在汇总卡与逐请求日志给细粒度；我们表里已有 input/output 细分，加一列不引入新口径 —— 建议给。

---

## 13. 外部实现参考（代码级，2026-11-03 检索）

### 13.1 五个参照物

| 项目 | 定位 | 与本方案的对应 | 关键代码事实 |
|:--|:--|:--|:--|
| **LiteLLM** | Python 网关，跨 100+ provider 归一 | 对应我们的 `extract_usage` | `prompt_tokens` **含** cache-hit + cache-miss；读缓存统一 `prompt_tokens_details.cached_tokens`，Anthropic 另给 `cache_creation_input_tokens`，Bedrock 另给 `cache_read_input_tokens`；成本按桶计价（`cache_read_input_token_cost` / `cache_creation_input_token_cost`，12h TTL 档 2.5×）；有 `supports_prompt_caching(model)` 能力函数，数据来自静态价目表 |
| **demesne** | Go 代理 sidecar，逐请求落 `usage.json` | 对应我们的 `UsageTap` + 账本 | 两个 provider 各一个 `TokenCounts` 结构，各自 `normalizeTokens()` → 统一 `TokenBreakdown{Input,Output,CacheCreation,CacheRead}`，再泛型累加；`combineCounts` 就是逐字段相加；另有 `AddDroppedNoUsageBlock` / `AddDroppedParseError` 计数器（= 我们的覆盖度分母） |
| **opencodex** | Codex/Claude Code 代理，有 GUI 用量页 | 对应我们的前端与日志形状 | `inputTokens` 含缓存读写；`cachedInputTokens` 是子集；provider 拆分时用 `cacheReadInputTokens` / `cacheCreationInputTokens`；单轮率 = `cached / inputTokens`，**聚合**用 `cacheObservedInputTokens` 当分母；`responsesUsage()` 把 `input_tokens_details.cached_tokens`（及 `cache_write_tokens`）**零值也恒发射**，因为严格客户端把该字段当必填 |
| **ccusage** | 扫 Claude Code JSONL | 对应我们的 `token_stats` | 只报 `cacheReadTokens` / `cacheCreationTokens` 原始量，**不算率** |
| **OTel GenAI semconv** | 标准 | 口径依据 | `gen_ai.usage.input_tokens` **SHOULD include** cached；缓存只有 `cache_read` / `cache_creation` 两个属性 |

### 13.2 三条直接改变本方案的结论

**① `UsageTap` 的「后到整块覆盖」会丢缓存数（必须改）**

现状 `usage_tap.rs:302-304` 是 `self.latest = Some(usage)` —— 后一帧整块替换前一帧。demesne 的做法相反：`message_delta` 只在 `if ev.Usage.CacheReadInputTokens > 0` 时才覆盖（`internal/proxies/anthropic/usage.go` 的 `applyDelta`），因为末帧常常只带部分字段。

⇒ 本方案 §4.1 增加一条：`observe_frame` 改为**逐字段合并**（后到的 `Some` 覆盖，`None` 与 0 不得抹掉已有非 0 值），否则「上游先报 900 缓存、末帧只报 output」会把命中率打成 0。

**② 能力判断用「静态表 + 运行时观测」，而不是写死 `unsupported_note`**

LiteLLM 用 `model_prices_and_context_window.json` 里的 `supports_prompt_caching` 标记回答「这个模型支不支持」，而不是在代码里写 `if provider == "x"`。

⇒ 本方案 §7 增加：新增一张 `cache_capability` 事实表（平台 × 账本 → `unknown / reported / unsupported`），由 P0 探针结果与运行时 `cacheRequests` 共同决定；`trae/token_stats.rs:494` 的静态声明改为这张表的一行，UI 文案随之变成「上游未上报」而不是「无从记录」。

**③ 出口字段恒在，零值兜底**

opencodex 的 `responsesUsage()` 无条件发射 `input_tokens_details.cached_tokens`（零值也发），理由是严格客户端把该字段当必填。

⇒ 我们**已经符合**：`protocol/responses.rs:641-706` 用 `cached_tokens.unwrap_or(0)` 恒发射，`protocol/anthropic.rs:664,969` 恒发 `cache_read_input_tokens`。这条不用改，但要在实现时**别为了「区分未上报」把它们改成省略键** —— 出口的「字段恒在」与账本的「未上报可区分」是两层，别互相污染。

### 13.3 两个反直觉事实（直接影响文案）

1. **低于最小输入 token 时缓存被静默跳过、不报错**（OpenAI 1024、Claude 512~4096 不等，LiteLLM 文档明确要求「用 usage 字段断言」）。
   ⇒ 「0%」在短请求占多数的窗口里是**正常现象**，不是缓存坏了。UI tooltip 需要这句话，否则用户会把「短对话多」误读成「缓存失效」。
2. **真实基准**：第三方聚合数据显示 Codex CLI 群体命中率 92%、OpenCode 67%（后者因接了大量第三方 provider）。我们实测 WB 96.3% 落在这个区间，说明口径没有系统性偏差。

### 13.4 P4「省了多少」：**前置条件不满足，暂不做**（本轮核实后推翻）

LiteLLM 靠价目表的三件套算省钱：`input_cost_per_token`（未命中）、`cache_read_input_token_cost`（命中）、`cache_creation_input_token_cost`（写入）。

我们的 `gateway_models.{cn,global}.json` **没有单价**：15 个模型的 `credits` 字段是 `"x0.11 credits"` 这种**倍率文案**（字符串），不是 per-token 价格，也没有读/写分项。上游 `usage.credit` 只给一个总积分，不区分命中与未命中。

⇒ **`savedCredit` 无法折算**（任何折算都是编的）。P4 降级为：只展示「缓存读取 token 数」这个原始量，不做金额换算；若将来上游提供分项 credit 或自建价目表，再单独立项。
