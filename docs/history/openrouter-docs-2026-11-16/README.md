> 状态：只读历史

# OpenRouter 文档快照（2026-11-16）

本目录是 2026-11-16 从 <https://openrouter.ai/docs> 抓取的 OpenRouter 官方文档副本
（`llms.txt` 索引 + 若干页面 + OpenAPI 规范 + 模型清单），用于网关路由相关实现的参考。

**这些文件是外部资料快照，不是本仓的现行设计依据。**
实现上的结论、术语与口径一律以 `docs/specs/` 下的现行文档为准；
本目录只在需要核对 OpenRouter 原始措辞（字段名、默认值、语义）时查阅。

## 内容清单

| 文件 | 内容 |
|:--|:--|
| `llms.txt` / `openrouter-llms.txt` | OpenRouter 文档总索引（同一内容的两份副本） |
| `docs-index.txt` | 抓取时的页面清单 |
| `models.json` | OpenRouter 模型清单（含定价与端点信息） |
| `openrouter-openapi.json` | OpenRouter OpenAPI 规范 |
| `provider-selection.md` | Provider 选择与排序规则 |
| `provider-routing-2.md` | Provider 路由补充说明 |
| `traffic-routing.md` | 端点间流量分配规则 |
| `router-metadata.md` | 路由相关元数据说明 |
| `models-doc.md` / `models-guide.md` | 模型列表接口文档与使用指引 |
| `variants-overview.md` | 模型变体（variants）说明 |
| `endpoints-*.md` / `endpoints-public.json` / `endpoints-doc.md` | 端点列表相关文档与样例响应 |
| `gpt4o-endpoints.json` / `llama-endpoints.json` / `claude-endpoints.json` | 指定模型的端点查询样例 |
| `generations-doc.md` / `generations-sdk.md` | 生成接口文档与 SDK 说明 |
| `generation-missing.json` | 查询不存在生成记录时的响应样例 |

> 抓取脚本与当时的命令记录见该次会话日志；本目录不做任何后续维护。
