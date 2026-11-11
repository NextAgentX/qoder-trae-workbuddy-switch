# 网关「默认思考档」按模型配置（`model_efforts`）

> 状态：现行 · 2026-11-01 落地（Qoder）/ 2026-11-01 扩展（WorkBuddy）

## 语义：这是「默认值」，不是「强制值」

客户端**自己没传** `reasoning_effort` 时，网关补上按模型配置的档位；**客户端显式传了就尊重客户端**。

反过来做（配置覆盖客户端）会让用户换个客户端就莫名被降档，且现象极难自证。空 map = 完全不补档，与加该字段之前**逐字节一致**。

## 数据流

```
UI 下拉选择 ──选完即存──▶ gateway_config.json / qoder gateway config
                                    │
                    ┌───────────────┴───────────────┐
             WorkBuddy 中继（共享出站）        Qoder 中继
        outbound/mod.rs::prepare_outbound_body_report  qoder/routes.rs::build_encoded
                    │                               │
                    └──────────▶ 上游请求体 parameters.reasoning_effort ◀──┘
```

## 后端

| 位置 | 作用 |
|:--|:--|
| `crates/buddy-switch-gateway/src/state.rs` | `GatewayConfig.model_efforts: HashMap<String,String>`（WB 用） |
| `crates/buddy-switch-gateway/src/qoder/mod.rs` | `QoderGatewayConfig.model_efforts: BTreeMap<String,String>`（Qoder 用，`alias = "modelEfforts"`） |
| `crates/buddy-switch-gateway/src/outbound/mod.rs` | `lookup_default_effort_with_config`：**用户配置 > 静态档位表 > 默认值** |
| `crates/buddy-switch-gateway/src/routes/relay.rs` | 把 WB 的 `config.model_efforts` 透传给共享出站构体 |
| `crates/buddy-switch-gateway/src/qoder/routes.rs` | Qoder 侧在 `build_encoded` 内注入（先判客户端是否已传） |

### 关键约束：非 DeepSeek 模型

`thinking` 注入与 `reasoning_content` 回填是 **DeepSeek 通道专有参数**，对其它模型发过去会被判非法参数。但 `reasoning_effort` 是**通用参数** —— 上游目录里 `glm-5.3` / `hy3` / `gpt-6-astra` 等**都声明了 `reasoning.efforts`**。

因此实现分两条路：

- DeepSeek 系：走原有 `inject_thinking` + 归一 + 回填
- **其它模型**：只补 `reasoning_effort`，且**仅当静态档位表确认该模型声明了档位**才写（保守白名单：避免把参数发给不认识它的模型换回 400）；配置值不在支持列表里时按「不高于它的最高支持档」落档

> 早期只对 DeepSeek 系注入，导致 `glm-5.3` 等模型上「选了等于白选」。已修。

## 前端

| 位置 | 作用 |
|:--|:--|
| `src/components/gateway/model-effort-select.tsx` | **共用**选择器（Qoder / WorkBuddy 同一份实现） |
| `src/components/gateway/model-list.tsx` | WB 列表 `effortColumn`：有档位 ⇒ 选择器；无档位 ⇒ 只读 `EffortCell`（`—`） |
| `src/components/gateway/qoder-model-list.tsx` | Qoder 同上 |
| `src/lib/gateway.ts` / `src/lib/qoder-gateway.ts` | 类型、默认值、归一化 |

### 两条硬性注意

1. **选择器必须是独立组件**，不能写成 `effortColumn.render` 里的内联函数 —— `render` 在渲染期被普通调用，不是 React 组件，在里面 `useState` 违反 Hooks 规则（TS 查不出来，运行时报 `Invalid hook call`）。
2. **`normalizeXxxConfig` 少一个字段是静默失败**：Rust 侧 `#[serde(default)]` 不报错，表现是「选了、保存了、没了」。新增配置字段必须同时补：类型、`DEFAULT_*`、归一化。

## 交互：选完即存

不做草稿态、不加「保存」按钮 —— 与 `max_context` 开关同口径。多一个按钮只会多出「改了没点保存 = 白改」这类静默失效。

## 回归防线（Rust 单测）

| 用例 | 锁住什么 |
|:--|:--|
| `configured_effort_applies_to_non_deepseek_models_without_thinking_fields` | 非 DeepSeek 也补档，但**不**注入 `thinking` / `reasoning_content` |
| `client_supplied_effort_wins_over_configured_default` | 客户端显式传的档位优先于网关默认档 |
| `configured_effort_is_dropped_when_model_declares_no_levels` | 静态表标该模型无档位 ⇒ 不发该参数 |
| `gateway_config_serializes_with_snake_case_keys` | 配置键集钉死，新增字段必须登记 |

## 相关

- 上游出站网络（直连 / 系统代理）：[`GATEWAY-UPSTREAM-NETWORK.md`](GATEWAY-UPSTREAM-NETWORK.md)
