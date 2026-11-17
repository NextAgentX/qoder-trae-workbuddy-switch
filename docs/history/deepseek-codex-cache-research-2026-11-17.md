# Codex → Buddy → Trae/DeepSeek：缓存 0% 调研

> 状态：调研结论保持不变；2026-11-17 后续修复已把「未上报」与「明确 0」贯通到 Trae 转译、请求日志、两套账本和前端命中率。调研本身未发起付费 API 探针。

## 结论与适用边界

用户描述：Codex 使用 provider Buddy、模型 `deepseek-v4.1-flash`，请求面板约 277K–361K「新输入」、缓存 0%、输出很少。**这些数字不能单独证明 DeepSeek 不支持缓存，也不能定位为 Codex 的错误。** 至少要区分真实未命中、上游未报告、协议转换丢失/补零三种情况。本次没有该批请求的原始上游响应或发出正文，不声称已查明根因。

## 官方事实

- **缓存匹配前缀，不匹配任意重复片段或语义。** DeepSeek 当前规则还要求完整匹配一个已经落盘的前缀单元：请求边界、检测到的公共前缀、固定 token 间隔都可能产生单元。例如先发 `A+B`，再发 `A+C`，第二次不一定命中 `A`；检测并保存公共前缀后，第三次 `A+D` 才能复用它。构建需要数秒，缓存为尽力而为，闲置通常数小时至数天后清除。不能保证相同内容立刻或永远命中。[DeepSeek Context Caching](https://api-docs.deepseek.com/guides/kv_cache/)
- **模型支持与路由支持必须分开核实。** 官方 V4.1-Flash 已上线，当前官方 API 使用 `deepseek-flash`，并列出缓存命中/未命中价格；不能认定 V4.1-Flash 天生不支持缓存。但 Buddy 的 provider 名及 `deepseek-v4.1-flash` 别名不能证明流量直达官方 API，也不能证明 Trae 的部署、缓存隔离、保留期限或统计透传与官方相同。本次未找到能确认该 Trae 路由缓存契约的公开官方资料。[DeepSeek 发布说明](https://api-docs.deepseek.com/news/news260910/)、[官方模型与计价](https://api-docs.deepseek.com/quick_start/pricing/)
- **key 不是缓存开关。** DeepSeek 官方 Responses 接口不支持 `prompt_cache_key` / `prompt_cache_retention`，会忽略不支持的参数；上下文缓存自动管理。`previous_response_id` 也不支持，官方接口无状态，需要完整历史。这不等于没有前缀缓存。本地 Buddy 可另外使用 key 做会话路由，但不能据此承诺上游命中。[DeepSeek Responses 兼容表](https://api-docs.deepseek.com/guides/responses_api/#compatibility-details)
- **Codex 格式确实影响可复用前缀。** 官方说明列出工具及其顺序、模型相关指令、权限/沙箱、目录配置等影响因素，也说明 Codex 尽量通过追加消息保留旧前缀；压缩则会替换历史。不能笼统断言 Codex 每次都随机改前缀。[OpenAI Codex agent loop](https://openai.com/index/unrolling-the-codex-agent-loop/)、[OpenAI Prompt Caching 201](https://developers.openai.com/cookbook/examples/prompt_caching_201)

这里的「完全一致」指服务端实际处理的 token 前缀和相关结构。早期指令、时间戳、工具 schema/顺序、角色转换或历史裁剪变化都值得比较；不能把 HTTP JSON 缩进或顶层键顺序变化直接等同于模型 token 变化。只改后缀也不必使整个旧前缀失效。OpenAI 的模型阈值、TTL、缓存控制参数不能直接套用到 Trae/DeepSeek。

## 统计口径：先看原始字段

| 层/协议 | 应检查的字段与含义 |
|:--|:--|
| DeepSeek Chat Completions | `usage.prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens`；`prompt_tokens_details.cached_tokens` 等于命中量 |
| Responses | 最终响应 `usage.input_tokens`、`input_tokens_details.cached_tokens`；初始流事件的 `usage` 可为 `null` |
| Buddy 原始请求日志 | `promptTokens`、`cachedTokens`；缺失/`null` 与明确 `0` 必须分别检查 |

前两行依据：[DeepSeek Chat API](https://api-docs.deepseek.com/api/create-chat-completion/)、[DeepSeek Responses API](https://api-docs.deepseek.com/api/create-response/)。公式为 `命中率 = hit / (hit + miss)`；已知总输入包含缓存时，新输入为 `total_input - hit`。输出量不参与分母，输出很少不能解释输入缓存为何为零。大输入可能来自指令、工具定义和历史反复发送，不能仅凭数值判定是用户新输入，也不能据此推算真实账单。

## 本仓库的一手证据（HEAD `febc1b7`）

- [`trae/sse.rs`](../../crates/buddy-switch-gateway/src/trae/sse.rs)：`TokenUsage.cached: Option<u64>` 区分缺失与零，识别四种缓存字段。修复后读取 `metadata.prompt_cache_pool_record`：明确无池时把 0 视为未观测；明确有池时保留实测 0。`to_openai()` 只在确实有可信样本时输出缓存明细。
- [`protocol/responses.rs`](../../crates/buddy-switch-gateway/src/protocol/responses.rs)：原 `build_responses_usage()` 用 `cached_tokens.unwrap_or(0)` 把未上报写成零；现改为缺失输出 `null`、明确零保持 `0`。`to_upstream_request()` 转换指令/消息并裁剪历史，实际发出的前缀也需检查。当前默认历史预算是 **1.5 MiB 字节**，不是 token 上限；不能沿用旧设计记录里的 256 KiB 判断实际版本。
- [`gateway_ledger.rs`](../../crates/buddy-switch-core/src/modules/gateway_ledger.rs) 与 [`token_daily.rs`](../../crates/buddy-switch-core/src/modules/token_daily.rs)：原先把缺失当零；现已把 `cachedTokens` 读成 nullable，并以 `cacheObserved` 记录窗口内是否至少有一笔可信样本。前端只用已观测请求的输入作命中率分母，全部未观测时为 `null`，明确零仍显示 `0%`。既有 [`CACHE-HIT-RATE-PLAN.md`](../specs/CACHE-HIT-RATE-PLAN.md) 是设计/历史背景，不是该批请求的取证结果。

## 最小排查建议（未执行）

1. 对同一条请求关联 Codex 最终 usage、Buddy 原始日志、Trae 原始 `token_usage`；先确认缓存字段是明确零、缺失，还是换成未识别名称，并确认请求成功完成。
2. 在同一实际模型/区域/账号下，使用稳定指令与工具，顺序重复同一小请求，再只追加后缀；给缓存构建留出数秒。必要时增加第三次请求以验证公共前缀落盘规则。此处是诊断建议，不保证命中。
3. 比较**转换后发出的**指令、消息角色、工具定义/顺序、历史及首个差异位置；同时核查账号/区域变化、裁剪或压缩。账号/部署变化可能影响缓存局部性，具体隔离规则尚未证实。

没有这些证据，最稳妥的结论是「面板报告 0%，原因未定」，而不是「DeepSeek 禁用了缓存」或「Codex 必然破坏前缀」。
