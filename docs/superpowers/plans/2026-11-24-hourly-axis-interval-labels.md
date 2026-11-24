# 今天用量图小时轴区间标签实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** 小时图轴标签清楚表达每个桶代表的时间区间，例如 `00:00–01:00`。

**Architecture:** 在 `usage-charts.ts` 导出一个由日期小时键生成区间标签的纯函数。所有 WorkBuddy、Trae、Qoder 小时数据适配器和账本图都调用它；数据键、数值和非小时标签保持原样。

**Tech Stack:** TypeScript、React、现有 Recharts 图表适配层。

**Spec:** `docs/superpowers/specs/2026-11-24-hourly-axis-interval-labels-design.md`

## Global Constraints

- 小时标签格式为 `HH:00–HH+1:00`，最后一格为 `23:00–24:00`。
- 只调整小时桶标签，不改统计值、数据键、覆盖范围或其他日期标签。
- 不添加测试或运行测试（本次请求未要求测试）；用差异检查与格式化器调用点审阅核对变更范围。

## Review Focus

- 23 点终点必须显示 `24:00`，不能回绕成 `00:00`；通过边界计算代码审阅确认。
- 所有当前小时标签生成点必须使用共享格式化器；用 `rg` 搜索旧的 `:00` 拼接表达式确认。
- 日期桶标签和 tooltip 中的数值不能变化；检查适配器差异确认。

---

### Task 1: 统一小时轴标签

**Files:**
- Modify: `src/lib/usage-charts.ts`
- Modify: `src/lib/trae-usage-charts.ts`
- Modify: `src/pages/TokenStatsPage.tsx`
- Modify: `src/pages/QoderUsagePage.tsx`
- Modify: `docs/README.md`

**Interfaces:**
- Produces: 导出 `hourAxisLabel(key: string): string`；从日期小时键尾部读取 `HH`，返回 `HH:00–(HH+1):00`，并将小时 23 的终点设为 `24:00`。
- Consumes: `dailyModelSeries`、`gatewayDailyColumns`、`creditDailyBars`、`traeCreditBars`、WorkBuddy/Qoder `ledgerDailySeries` 继续消费原有字符串标签。

- [x] **Step 1: 更新共享格式化器及调用点**

  将 `hourAxisLabel` 导出并改为生成区间标签；在 `creditDailyBars`、`traeCreditBars`、`TokenStatsPage` 和 `QoderUsagePage` 中替换各自拼接的 `HH:00`。保留所有日期标签和数据映射不变。

- [x] **Step 2: 更新文档地图**

  在 `docs/README.md` 的 superpowers 文档表中登记本设计与本计划。

- [x] **Step 3: 静态检查改动范围**

  运行 `git diff --check`，并搜索仓库内小时图的 `HH:00` 标签拼接；检查差异确认剩余 `HH:00` 用途只属于非区间标签（例如热力图刻度）或无关功能。

  Expected: 无空白错误；所有小时图柱标签都经共享格式化器生成，日期轴与数据值逻辑无改动。

  Result: `git diff --check` passed. `rg` found no remaining manually composed `HH:00` labels in the hourly chart adapters; heatmap ticks remain unchanged. Source diff changes only label formatting and imports.
