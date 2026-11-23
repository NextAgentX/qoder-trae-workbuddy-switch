//! Trae Token 统计：聚合本机 Trae API 网关的请求日志。
//!
//! ## 为什么数据源只有一个，以及它带来的边界
//!
//! WorkBuddy 的 Token 统计能扫 `.workbuddy/projects/*.jsonl`——那是客户端自己落的
//! 会话记录。Trae **没有**等价的可解析落盘：TRAE SOLO 的对话历史在 IDE 的私有
//! 存储里（加密 + 二进制索引），本模块不去碰它。
//!
//! 于是 Trae 的 Token 用量只有一个诚实可得的观测点：本机网关的
//! `~/.buddy-switch/trae/api_gateway_logs.json`。这决定了三条必须写在页面上的边界：
//!
//! 1. 只统计**经过本网关**的调用——直接在 Trae IDE 里对话不产生记录；
//! 2. 网关未启用（或日志被清空）时统计恒为空；
//! 3. 上游只在流结束时回报 `token_usage`；中途断流的请求没有用量，只记 `records`。
//!
//! 与其拼一个看起来完整、实则混入猜测的「全量用量」，不如把边界讲清楚。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::handlers;
use super::variant::TraeVariant;
use super::{account, paths};

/// 统计窗口的默认天数（与 WorkBuddy 侧一致）。
pub const DEFAULT_RANGE_DAYS: i64 = 30;

/// 一个聚合桶的累加器。
#[derive(Debug, Default, Clone)]
struct Bucket {
    input: u64,
    output: u64,
    records: u64,
    errors: u64,
    stream_requests: u64,
    latency_sum: i64,
    /// 用于 p95：只保留原始延迟（日志上限 200 条量级，直接存全量足够）。
    latencies: Vec<i64>,
}

impl Bucket {
    fn add(&mut self, entry: &LogEntry) {
        self.input += entry.prompt_tokens;
        self.output += entry.completion_tokens;
        self.records += 1;
        if entry.status >= 400 || entry.error.is_some() {
            self.errors += 1;
        }
        if entry.stream {
            self.stream_requests += 1;
        }
        if entry.latency_ms > 0 {
            self.latency_sum += entry.latency_ms;
            self.latencies.push(entry.latency_ms);
        }
    }

    fn total(&self) -> u64 {
        self.input + self.output
    }

    fn avg_latency(&self) -> i64 {
        if self.records == 0 {
            0
        } else {
            self.latency_sum / self.records as i64
        }
    }

    /// 最近秩次 p95（`ceil(0.95 * n) - 1`）。样本不足时退化为最大值。
    fn p95_latency(&self) -> i64 {
        if self.latencies.is_empty() {
            return 0;
        }
        let mut sorted = self.latencies.clone();
        sorted.sort_unstable();
        let index = ((sorted.len() as f64 * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1);
        sorted[index]
    }

    /// 汇总字段（`total` / `input` / `output` 三件套 + 请求侧指标）。
    fn summary_value(&self) -> Value {
        json!({
            "total": self.total(),
            "input": self.input,
            "output": self.output,
            "records": self.records,
            "errors": self.errors,
            "streamRequests": self.stream_requests,
            "avgLatencyMs": self.avg_latency(),
            "p95LatencyMs": self.p95_latency(),
        })
    }

    /// 分组字段（比 summary 多一个 `key`）。
    fn group_value(&self, key: &str, extra: Value) -> Value {
        let mut value = self.summary_value();
        if let Some(object) = value.as_object_mut() {
            object.insert("key".into(), json!(key));
            if let Some(extra) = extra.as_object() {
                for (name, item) in extra {
                    object.insert(name.clone(), item.clone());
                }
            }
        }
        value
    }
}

/// 日志里一条记录（只取本模块用得到的字段）。
#[derive(Debug, Default, Clone)]
struct LogEntry {
    ts: i64,
    model: String,
    account: String,
    status: u16,
    latency_ms: i64,
    prompt_tokens: u64,
    completion_tokens: u64,
    stream: bool,
    error: Option<String>,
    /// 归属产品线（网关记录日志时写入）。**旧日志没有该键 → `None`（未标注）**。
    variant: Option<TraeVariant>,
}

impl LogEntry {
    /// 从日志条目解析；`ts` 缺失的记录直接丢弃（无法归入任何时间桶）。
    fn parse(value: &Value) -> Option<Self> {
        let ts = value.get("ts").and_then(Value::as_i64)?;
        Some(Self {
            ts,
            model: text(value, "model"),
            account: text(value, "account"),
            status: value
                .get("status")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u16::MAX as u64) as u16,
            latency_ms: value.get("latencyMs").and_then(Value::as_i64).unwrap_or(0),
            prompt_tokens: value
                .get("promptTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            completion_tokens: value
                .get("completionTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            stream: value
                .get("stream")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            error: value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|text| !text.is_empty()),
            // 变体用 as_str() 的下划线形态；未知值 → None（归入「未标注」）。
            variant: value
                .get("variant")
                .and_then(Value::as_str)
                .and_then(TraeVariant::parse),
        })
    }
}

/// Token 统计的**区域**查询范围（筛选维度，非第三种区域实体）。
///
/// **不得并入 [`TraeRegion`] / [`TraeVariant`]**：那两个是「实体」，而这里是「查询范围」，
/// 多了「未标注」（旧日志）与「全部」两档 —— 合并会让 `TraeRegion::all()` 之类的
/// 既有约定失去定义（`MEMORY.md §一`）。
///
/// ⚠️ 档名在 2026-09-21 由**产品线**改为**区域**：网关的账号池、冷却、日志归属都按
/// 区域分家（国内 / 国际是两套互不相通的账号体系），而「Trae Work / Trae CN」只是
/// **国内区域下的两条程序** —— 拿它们当统计维度会名不副实。
///
/// 历史标识 `work` / `trae_work` / `trae_cn` **一律归入国内版**（它们都是国内构建），
/// 因此历史日志与老链接既不会变成「未标注」，也不会串到国际版去。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TraeTokenScope {
    /// 仅**国内版**（`variant` 属于国内区域的任何取值）。
    Cn,
    /// 仅**国际版**（另一套账号体系）。
    Global,
    /// 仅**未标注**（旧日志：无 `variant` 键）。
    Unlabeled,
    /// 全部（国内 ∪ 国际 ∪ 未标注）。
    #[default]
    All,
}

impl TraeTokenScope {
    /// 从字符串解析（两条通道共用；缺省 / 未知 → [`Self::All`]）。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            // 国内的三种写法：新标识 `cn`，以及全部历史标识（两条旧产品线都是国内构建）。
            "cn" | "work" | "trae_work" | "trae_cn" => Self::Cn,
            "global" | "intl" | "international" => Self::Global,
            "unlabeled" | "none" | "unknown" | "未标注" => Self::Unlabeled,
            _ => Self::All,
        }
    }

    /// 线上标识（前端 `TraeTokenScope = "cn" | "global" | "unlabeled" | "all"`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Global => "global",
            Self::Unlabeled => "unlabeled",
            Self::All => "all",
        }
    }

    /// 某条日志（其变体）是否落在本范围内。
    ///
    /// 判据是**区域**而不是变体本身：国内的两条程序位共享同一批账号与同一份用量来源，
    /// 按变体分档只会把同一区域的调用劈成两半。
    fn matches(self, variant: Option<TraeVariant>) -> bool {
        match self {
            Self::All => true,
            Self::Cn => variant.map(|v| v.region() == super::region::TraeRegion::Cn) == Some(true),
            Self::Global => {
                variant.map(|v| v.region() == super::region::TraeRegion::Global) == Some(true)
            }
            Self::Unlabeled => variant.is_none(),
        }
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 读取日志文件。缺失/损坏一律视为「没有记录」，**不报错**——
/// 网关没启用过时文件本来就不存在，那不是异常。
fn load_entries() -> (Vec<LogEntry>, usize) {
    let file = paths::api_gateway_log_file();
    let Ok(text) = std::fs::read_to_string(&file) else {
        return (Vec::new(), 0);
    };
    let Ok(values) = serde_json::from_str::<Vec<Value>>(&text) else {
        return (Vec::new(), 1);
    };
    let mut parse_errors = 0;
    let entries = values
        .iter()
        .filter_map(|value| match LogEntry::parse(value) {
            Some(entry) => Some(entry),
            None => {
                parse_errors += 1;
                None
            }
        })
        .collect();
    (entries, parse_errors)
}

/// 按 `days` 过滤（`None` 表示不限；`days <= 0` 同样视为不限）。
fn cutoff_ms(days: Option<i64>) -> Option<i64> {
    let days = days?;
    if days <= 0 {
        return None;
    }
    let now = chrono::Local::now().timestamp_millis();
    Some(now - days * 24 * 60 * 60 * 1000)
}

/// 毫秒时间戳 → 本地日期 `YYYY-MM-DD`。
fn local_date(ts_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| "未知".to_string())
}

/// 毫秒时间戳 → 本地小时 `00`..`23`。
fn local_hour(ts_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|utc| utc.with_timezone(&chrono::Local).format("%H").to_string())
        .unwrap_or_else(|| "--".to_string())
}

/// 毫秒时间戳 → 本地**星期** `0`..`6`（**周一 = 0**，与前端 `HeatmapMatrix.cells[weekday][hour]`
/// 及原型的 `WEEKDAYS = ["周一".."周日"]` 同一口径）。
///
/// 用 `num_days_from_monday()` 而不是 `%u`：后者的 1..7 还要再减一，
/// 而 `num_days_from_monday()` 的语义**就是**本函数要的「距周一几天」。
fn local_weekday(ts_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|utc| {
            use chrono::Datelike;
            utc.with_timezone(&chrono::Local)
                .weekday()
                .num_days_from_monday()
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}

/// 毫秒时间戳 → **「星期-小时」槽位** `"{weekday}-{HH}"`（如 `"0-09"`）。
///
/// ## 为什么是「星期 × 小时」而不是单纯的「小时」
///
/// 原型的「使用时段分布」是一张 **7 行（周一..周日）× 24 列**的热力图
/// （`demo-app.js` 的 `heatmap(e.hourly, WEEKDAYS, …)`）。只按小时聚合只能画出 24 列，
/// 丢掉「工作日 vs 周末」这个真正有用的维度。
///
/// 键的**字符串升序**即「周 → 小时」的自然顺序（`"0-00" < "0-09" < "1-00"`），
/// 因此下游沿用既有的 `ascending(...)` 排序即可，不需要额外排序键。
fn local_slot(ts_ms: i64) -> String {
    format!("{}-{}", local_weekday(ts_ms), local_hour(ts_ms))
}

fn local_date_hour(ts_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%Y-%m-%d-%H")
                .to_string()
        })
        .unwrap_or_else(|| "未知".to_string())
}

/// Trae Token 统计（`days` 为统计窗口天数，`None` 表示全部历史；`scope` 为变体范围）。
///
/// 返回体形状与 WorkBuddy 的 `get_statistics` **刻意不同**：那边是「多源 + 会话/项目维度」，
/// 这边只有网关一个源，且天然带账号维度。强行对齐字段只会得到一堆恒为 0 的键。
///
/// 新增（本轮）：
/// - `variantCounts: { work, cn, unlabeled, all }`：**时间窗口内**各档条数（供范围条徽标）；
/// - `modelDaily: [{ date, model, total, input, output, records }]`：按天×模型的堆叠柱数据源；
/// - `hours: [{ key: "{weekday}-{HH}", … }]`：**周 × 小时**槽位（周一=0），供 7×24 热力图；
/// - `unsupported: [ { capability, label, supportedOn, reason } ]`：平台做不到的维度（置灰卡）。
///
/// `scope` 只影响**展示聚合**（summary/models/accounts/daily/hours/statuses/modelDaily），
/// **不影响** `variantCounts`（范围条要显示「切换后各档各有多少条」）。
pub fn get_statistics(days: Option<i64>, scope: TraeTokenScope) -> Value {
    let (entries, parse_errors) = load_entries();
    let cutoff = cutoff_ms(days);

    let mut summary = Bucket::default();
    let mut by_model: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut by_account: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut by_date: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut by_hour: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut by_status: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_model_daily: BTreeMap<(String, String), Bucket> = BTreeMap::new();
    let mut by_hour_of_day: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut by_model_hourly: BTreeMap<(String, String), Bucket> = BTreeMap::new();
    let mut variant_counts = VariantCounts::default();
    let mut coverage_start: Option<i64> = None;
    let mut coverage_end: Option<i64> = None;
    let mut kept = 0usize;

    for entry in entries.iter() {
        // 时间窗口过滤（变体计数也只看窗口内）。
        if cutoff.map(|minimum| entry.ts < minimum).unwrap_or(false) {
            continue;
        }
        variant_counts.bump(entry.variant);

        // 变体范围过滤：只影响下面的展示聚合，不影响 range 计数。
        if !scope.matches(entry.variant) {
            continue;
        }

        kept += 1;
        summary.add(entry);
        let model_key = if entry.model.is_empty() {
            "（未指定）".to_string()
        } else {
            entry.model.clone()
        };
        by_model.entry(model_key.clone()).or_default().add(entry);
        by_model_daily
            .entry((local_date(entry.ts), model_key))
            .or_default()
            .add(entry);
        let hour_key = local_date_hour(entry.ts);
        by_hour_of_day
            .entry(hour_key.clone())
            .or_default()
            .add(entry);
        by_model_hourly
            .entry((
                hour_key,
                if entry.model.is_empty() {
                    "（未指定）".to_string()
                } else {
                    entry.model.clone()
                },
            ))
            .or_default()
            .add(entry);
        by_account
            .entry(if entry.account.is_empty() {
                "（无账号）".to_string()
            } else {
                entry.account.clone()
            })
            .or_default()
            .add(entry);
        by_date.entry(local_date(entry.ts)).or_default().add(entry);
        // 键 = `"{weekday}-{HH}"`（见 `local_slot` 的说明）：供前端直接构造 7×24 热力矩阵。
        by_hour.entry(local_slot(entry.ts)).or_default().add(entry);
        *by_status.entry(entry.status.to_string()).or_insert(0) += 1;
        coverage_start = Some(coverage_start.map_or(entry.ts, |current| current.min(entry.ts)));
        coverage_end = Some(coverage_end.map_or(entry.ts, |current| current.max(entry.ts)));
    }

    // 账号名回表：日志只存 uid，展示必须给人看的名字。
    let names: BTreeMap<String, String> = account::entries()
        .into_iter()
        .map(|(uid, raw)| {
            let name = if raw.name.trim().is_empty() {
                uid.clone()
            } else {
                raw.name.clone()
            };
            (uid, name)
        })
        .collect();

    let models = descending(by_model, |key, bucket| bucket.group_value(key, json!(null)));
    let accounts = descending(by_account, |key, bucket| {
        let name = names.get(key).cloned().unwrap_or_else(|| key.to_string());
        let tail = key.get(key.len().saturating_sub(8)..).unwrap_or(key);
        bucket.group_value(key, json!({ "name": name, "shortId": tail }))
    });
    let daily = ascending(by_date, |key, bucket| bucket.group_value(key, json!(null)));
    let hours = ascending(by_hour, |key, bucket| bucket.group_value(key, json!(null)));
    let statuses: Vec<Value> = by_status
        .iter()
        .map(|(key, count)| json!({ "key": key, "records": count }))
        .collect();
    // modelDaily：按键升序（`(date, model)` 天然有序），供「按模型×按天堆叠柱」直接渲染。
    let model_daily: Vec<Value> = by_model_daily
        .iter()
        .map(|((date, model), bucket)| {
            json!({
                "date": date,
                "model": model,
                "total": bucket.total(),
                "input": bucket.input,
                "output": bucket.output,
                "records": bucket.records,
            })
        })
        .collect();
    let daily_hours = ascending(by_hour_of_day, |key, bucket| {
        bucket.group_value(key, json!(null))
    });
    let model_hourly: Vec<Value> = by_model_hourly
        .iter()
        .map(|((date_hour, model), bucket)| {
            json!({
                "dateHour": date_hour,
                "model": model,
                "total": bucket.total(),
                "input": bucket.input,
                "output": bucket.output,
                "records": bucket.records,
            })
        })
        .collect();

    json!({
        "source": "trae-gateway",
        "label": "Trae API 网关",
        "generatedAt": chrono::Local::now().timestamp_millis(),
        "rangeDays": days.filter(|value| *value > 0),
        "logFile": paths::api_gateway_log_file().to_string_lossy(),
        "summary": summary.summary_value(),
        "models": models,
        "accounts": accounts,
        "daily": daily,
        "hours": hours,
        "statuses": statuses,
        "modelDaily": model_daily,
        "dailyHours": daily_hours,
        "modelHourly": model_hourly,
        "variantCounts": {
            // 按**区域**分档（国内 / 国际）：国内两条程序位合计一档，
            // 因为账号库与用量来源在区域层面才分开（见 `TraeTokenScope` 的说明）。
            "cn": variant_counts.cn,
            "global": variant_counts.global,
            "unlabeled": variant_counts.unlabeled,
            "all": variant_counts.all,
        },
        "unsupported": unsupported_notes(),
        // 与 WorkBuddy 侧同名，便于共用「数据来源说明」组件。
        "filesScanned": if kept > 0 { 1 } else { 0 },
        "parseErrors": parse_errors,
        "coverageStartAt": coverage_start,
        "coverageEndAt": coverage_end,
        "note": "只统计经过本机 Trae 网关的调用；直接在 Trae IDE 里对话不产生记录。",
    })
}

/// 时间窗口内各变体档位的条数累加器。
#[derive(Debug, Default)]
struct VariantCounts {
    /// 国内版一档（旧日志里的 `trae_work` 与 `trae_cn` **都算它** —— 同属国内区域）。
    cn: u64,
    /// 国际版一档（另一套账号体系）。
    global: u64,
    unlabeled: u64,
    all: u64,
}

impl VariantCounts {
    /// 按**区域**分档计数。
    ///
    /// 与 [`TraeTokenScope::matches`] 同一判据：国内的两条程序位共享同一批账号与
    /// 同一份用量来源，按变体分档会把同一区域的调用劈成两半。
    fn bump(&mut self, variant: Option<TraeVariant>) {
        match variant.map(|v| v.region()) {
            Some(super::region::TraeRegion::Cn) => self.cn += 1,
            Some(super::region::TraeRegion::Global) => self.global += 1,
            None => self.unlabeled += 1,
        }
        self.all += 1;
    }
}

/// Token 统计里「平台做不到」的维度清单（形状与文案由 [`handlers::unsupported_note`] 唯一产出）。
fn unsupported_notes() -> Vec<Value> {
    vec![
        handlers::unsupported_note(
            "cache_metrics",
            "缓存读取 / 写入 / 命中率",
            "Trae 网关日志与上传链路都没有 cache 字段，上游也不回传——无从记录",
        ),
        handlers::unsupported_note(
            "project_dimension",
            "按项目维度统计",
            "网关日志的 project_id / session_id 是每请求新生成的 uuid，不对应客户端项目",
        ),
        handlers::unsupported_note(
            "session_cost",
            "调用最贵的会话",
            "无稳定会话标识，无法把多次请求归并成一个会话成本",
        ),
    ]
}

/// 按 `total` 降序（同名则按键升序，保证渲染顺序稳定）。
fn descending<F>(map: BTreeMap<String, Bucket>, build: F) -> Vec<Value>
where
    F: Fn(&str, &Bucket) -> Value,
{
    let mut items: Vec<(&String, &Bucket)> = map.iter().collect();
    items.sort_by(|left, right| {
        right
            .1
            .total()
            .cmp(&left.1.total())
            .then_with(|| left.0.cmp(right.0))
    });
    items
        .into_iter()
        .map(|(key, bucket)| build(key, bucket))
        .collect()
}

/// 按键升序（日期 / 小时天然有序）。
fn ascending<F>(map: BTreeMap<String, Bucket>, build: F) -> Vec<Value>
where
    F: Fn(&str, &Bucket) -> Value,
{
    map.iter().map(|(key, bucket)| build(key, bucket)).collect()
}

/// 网关日志默认保留条数（`api_gateway.json` 的 `logKeep` 缺省值，与网关侧 `state`/`trae` 同值）。
const DEFAULT_LOG_KEEP: usize = 200;

/// 读网关配置里的 `logKeep`（读不到 / 非法 ⇒ [`DEFAULT_LOG_KEEP`]）。
///
/// 只用来判定「今天的日志是否可能已被挤出」——**不要**在这里做任何写入。
fn gateway_log_keep() -> usize {
    std::fs::read_to_string(paths::api_gateway_file())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            value
                .get("logKeep")
                .and_then(Value::as_u64)
                .map(|keep| keep as usize)
        })
        .filter(|keep| *keep > 0)
        .unwrap_or(DEFAULT_LOG_KEEP)
}

/// 「今日 · 按账号」的网关桶（**纯函数**，便于单测）。
///
/// 判据三件套，缺一条都会把别的账号 / 别的日子的量算进来：
/// 1. `ts` 落在**本地日期** `day`（与签到日志 / 积分事件同口径）；
/// 2. `account`（网关写入的就是 **uid**）属于该区域 —— 用「账号库里存在」**或**
///    「日志自带的变体属于该区域」两条并集：前者兜住旧日志缺 `variant`，
///    后者兜住「账号已被删除、但今天的调用确实发生过」（删号不该抹掉用量）；
/// 3. 空 `account`（无法归属）不进任何账号桶。
fn today_gateway_buckets(
    entries: &[LogEntry],
    day: &str,
    region: super::region::TraeRegion,
    known: &std::collections::HashSet<String>,
) -> BTreeMap<String, (u64, u64)> {
    let mut buckets: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for entry in entries {
        if entry.ts <= 0 || entry.account.is_empty() || local_date(entry.ts) != day {
            continue;
        }
        let belongs =
            known.contains(&entry.account) || entry.variant.map(|v| v.region()) == Some(region);
        if !belongs {
            continue;
        }
        let slot = buckets.entry(entry.account.clone()).or_insert((0, 0));
        slot.0 = slot.0.saturating_add(entry.prompt_tokens);
        slot.1 = slot.1.saturating_add(entry.completion_tokens);
    }
    buckets
}

/// 「今日 Token · 按账号」（`get_trae_account_token_today`；**只读**，issue #47）。
///
/// ## 形状照抄 WorkBuddy，但**客户端一档不可得**
///
/// 返回体与 `get_account_token_today`（WB）同构，四个字段一个不少 —— 前端三平台共用
/// 同一套渲染。但数据源只有本机网关日志（Trae 没有可解析的客户端会话落盘，见模块头），
/// 因此：
///
/// - `clientIn` / `clientOut` 恒为 `0`，并**额外下发 `clientSource: "unavailable"`** ——
///   前端据此把「客户端」渲染成「不可得」而不是真实 `0`（否则会把「没测到」说成「没用」）；
/// - `unknownClient` 恒为 `{in:0,out:0}`（网关日志每条都带 uid，不存在归属不明的客户端量）；
/// - `gatewayTruncated = true` 表示**日志文件已到 `logKeep` 上限** ⇒ 今日部分可能已被
///   昨天的流量挤出 ⇒ 网关数只是**下界**（前端渲染 `≥N`）。判据刻意选「文件满」而不是
///   「今天条目数到上限」：前者只会在真的可能被裁时报警。
///
/// ## 缺席 ≠ 0
///
/// `accounts` 里**没有**某账号 = 今天没有经过本网关的调用（**不是** 0）。真实 `0`
/// 只有一种情况：今天有该账号的记录但 token 都是 0（失败请求），此时会显式出现 `0`。
///
/// `force` 仅为契约保留：本接口读的是本地文件（≤ `logKeep` 条），重读成本远低于一次
/// IPC / HTTP，因此**不设 TTL 缓存**，`force` 在这里恒等。
pub fn account_token_today(variant: TraeVariant, force: bool) -> Value {
    let _ = force;
    let now = chrono::Local::now().timestamp_millis();
    let day = local_date(now);
    let keep = gateway_log_keep();
    let (entries, _) = load_entries();
    // 「文件已满」= 今日部分可能被挤出（保守判据，见函数文档）。
    let truncated = entries.len() >= keep;

    let known: std::collections::HashSet<String> = account::list_account_views_for(variant)
        .iter()
        .filter_map(|view| view.get("userId").and_then(Value::as_str))
        .map(str::to_string)
        .collect();

    let buckets = today_gateway_buckets(&entries, &day, variant.region(), &known);
    let accounts: Vec<Value> = buckets
        .into_iter()
        .map(|(uid, (input, output))| {
            json!({
                "uid": uid,
                // 客户端一档不可得（见函数文档）：这里给 0 只为形状同构，
                // 语义由 `clientSource` 决定，前端**不得**把它当真实 0。
                "clientIn": 0,
                "clientOut": 0,
                "gatewayIn": input,
                "gatewayOut": output,
            })
        })
        .collect();

    json!({
        "date": day,
        "accounts": accounts,
        "unknownClient": { "in": 0, "out": 0 },
        "gatewayTruncated": truncated,
        "clientSource": "unavailable",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts: i64, model: &str, account: &str, input: u64, output: u64) -> LogEntry {
        LogEntry {
            ts,
            model: model.to_string(),
            account: account.to_string(),
            status: 200,
            latency_ms: 100,
            prompt_tokens: input,
            completion_tokens: output,
            stream: true,
            error: None,
            variant: None,
        }
    }

    #[test]
    fn scope_matches_the_expected_region_tiers() {
        assert!(TraeTokenScope::All.matches(None));
        assert!(TraeTokenScope::All.matches(Some(TraeVariant::TraeWork)));
        assert!(TraeTokenScope::All.matches(Some(TraeVariant::Trae)));
        assert!(TraeTokenScope::All.matches(Some(TraeVariant::Global)));

        // 国内版一档**同时**覆盖两条国内程序位 —— 它们共享账号库与用量来源，
        // 按变体分档会把同一区域的调用劈成两半。
        assert!(TraeTokenScope::Cn.matches(Some(TraeVariant::TraeWork)));
        assert!(TraeTokenScope::Cn.matches(Some(TraeVariant::Trae)));
        assert!(!TraeTokenScope::Cn.matches(Some(TraeVariant::Global)));
        assert!(!TraeTokenScope::Cn.matches(None));

        // 国际版是另一套账号体系，两者互不重叠。
        assert!(TraeTokenScope::Global.matches(Some(TraeVariant::Global)));
        assert!(!TraeTokenScope::Global.matches(Some(TraeVariant::TraeWork)));
        assert!(!TraeTokenScope::Global.matches(Some(TraeVariant::Trae)));

        assert!(TraeTokenScope::Unlabeled.matches(None));
        assert!(!TraeTokenScope::Unlabeled.matches(Some(TraeVariant::TraeWork)));
        assert!(!TraeTokenScope::Unlabeled.matches(Some(TraeVariant::Global)));
    }

    #[test]
    fn scope_parse_and_default_cover_all_tiers() {
        assert_eq!(TraeTokenScope::default(), TraeTokenScope::All);
        // 历史标识（两条旧产品线）**都**归国内版 —— 老链接与旧前端值不会串到国际版。
        for legacy in ["work", "trae_work", "cn", "trae_cn"] {
            assert_eq!(
                TraeTokenScope::parse(legacy),
                TraeTokenScope::Cn,
                "历史标识 {legacy} 必须归国内版"
            );
        }
        assert_eq!(TraeTokenScope::parse("global"), TraeTokenScope::Global);
        assert_eq!(
            TraeTokenScope::parse("unlabeled"),
            TraeTokenScope::Unlabeled
        );
        assert_eq!(TraeTokenScope::parse("all"), TraeTokenScope::All);
        // 缺失 / 未知 → All（宽容失败方向）。
        assert_eq!(TraeTokenScope::parse(""), TraeTokenScope::All);
        assert_eq!(TraeTokenScope::parse("bogus"), TraeTokenScope::All);
        assert_eq!(TraeTokenScope::Cn.as_str(), "cn");
        assert_eq!(TraeTokenScope::Global.as_str(), "global");
        assert_eq!(TraeTokenScope::Unlabeled.as_str(), "unlabeled");
    }

    #[test]
    fn variant_counts_keep_unlabeled_and_sum_to_all() {
        let mut counts = VariantCounts::default();
        counts.bump(Some(TraeVariant::TraeWork));
        counts.bump(Some(TraeVariant::Trae));
        counts.bump(Some(TraeVariant::Global));
        counts.bump(None);
        assert_eq!(counts.cn, 2, "两条国内程序位同属国内版");
        assert_eq!(counts.global, 1);
        assert_eq!(
            counts.unlabeled, 1,
            "旧日志（无 variant）必须计入「未标注」"
        );
        assert_eq!(counts.all, 4, "All = 三档之和");
    }

    #[test]
    fn entry_parse_reads_variant_or_none() {
        let work = LogEntry::parse(&json!({"ts": 1, "variant": "trae_work"})).expect("解析 work");
        assert_eq!(work.variant, Some(TraeVariant::TraeWork));
        let cn = LogEntry::parse(&json!({"ts": 1, "variant": "trae_cn"})).expect("解析 cn");
        assert_eq!(cn.variant, Some(TraeVariant::Trae));
        // 旧日志无 variant → None（未标注），不得被丢弃或误判。
        let old = LogEntry::parse(&json!({"ts": 1})).expect("解析旧日志");
        assert!(old.variant.is_none());
        // 未知值 → None。
        let unknown = LogEntry::parse(&json!({"ts": 1, "variant": "doubao"})).expect("解析未知");
        assert!(unknown.variant.is_none());
    }

    #[test]
    fn get_statistics_filters_by_scope_and_counts_unlabeled() {
        let home = std::env::temp_dir().join(format!("trae-token-scope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".buddy-switch").join("trae")).expect("创建隔离目录");
        // 走 HomeOverrideGuard（内部已持 env 锁并在 drop 时还原），不要手动加锁。
        let _guard = crate::modules::config::HomeOverrideGuard::set(&home);

        let now = chrono::Local::now().timestamp_millis();
        let entries = json!([
            { "ts": now, "model": "m", "account": "u", "status": 200, "promptTokens": 1, "completionTokens": 1, "stream": true, "variant": "trae_work" },
            { "ts": now, "model": "m", "account": "u", "status": 200, "promptTokens": 1, "completionTokens": 1, "stream": true, "variant": "trae_cn" },
            { "ts": now, "model": "m", "account": "u", "status": 200, "promptTokens": 1, "completionTokens": 1, "stream": true, "variant": "global" },
            { "ts": now, "model": "m", "account": "u", "status": 200, "promptTokens": 1, "completionTokens": 1, "stream": true },
        ]);
        std::fs::write(paths::api_gateway_log_file(), entries.to_string()).expect("写入日志");

        let all = get_statistics(None, TraeTokenScope::All);
        assert_eq!(
            all["variantCounts"]["cn"], 2,
            "两条国内程序位同属国内版（旧日志的 trae_work / trae_cn 都算它）"
        );
        assert_eq!(all["variantCounts"]["global"], 1);
        assert_eq!(all["variantCounts"]["unlabeled"], 1);
        assert_eq!(all["variantCounts"]["all"], 4);
        assert_eq!(all["summary"]["records"], 4, "All 应计入全部三档");
        assert!(all["modelDaily"]
            .as_array()
            .map(|v| !v.is_empty())
            .unwrap_or(false));
        assert!(all["unsupported"]
            .as_array()
            .map(|v| !v.is_empty())
            .unwrap_or(false));

        let cn = get_statistics(None, TraeTokenScope::Cn);
        assert_eq!(
            cn["summary"]["records"], 2,
            "scope=cn 必须**同时**计入国内的两条程序位（按区域，不按产品线）"
        );
        // variantCounts 不受 scope 影响（范围条要显示各档总数）。
        assert_eq!(cn["variantCounts"]["all"], 4);

        let global = get_statistics(None, TraeTokenScope::Global);
        assert_eq!(
            global["summary"]["records"], 1,
            "scope=global 只应计入国际版，不得混入国内调用"
        );

        let unlabeled = get_statistics(None, TraeTokenScope::Unlabeled);
        assert_eq!(
            unlabeled["summary"]["records"], 1,
            "旧日志（无 variant）在 Unlabeled 下必须可见，不得被静默丢弃（R9）"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 今日桶：只认「今天 + 本区域 + 有 uid」，跨日 / 跨区域 / 空 uid 一条都不许进。
    #[test]
    fn today_gateway_buckets_filters_day_region_and_empty_account() {
        let now = chrono::Local::now().timestamp_millis();
        let yesterday = now - 24 * 60 * 60 * 1000;
        let mut known = std::collections::HashSet::new();
        known.insert("u-known".to_string());

        let entries = vec![
            entry(now, "m", "u-known", 10, 5),        // 今天 + 在账号库 ⇒ 计入
            entry(now, "m", "u-gone", 1, 1),          // 今天但不在库、也无变体 ⇒ 不计入
            entry(yesterday, "m", "u-known", 99, 99), // 昨天 ⇒ 不计入
            entry(now, "m", "", 7, 7),                // 空 uid ⇒ 不计入
        ];
        let buckets = today_gateway_buckets(
            &entries,
            &local_date(now),
            super::super::region::TraeRegion::Cn,
            &known,
        );
        assert_eq!(
            buckets.len(),
            1,
            "只有「今天 + 本区域 + 有 uid」那一条：{buckets:?}"
        );
        assert_eq!(buckets.get("u-known"), Some(&(10, 5)));

        // 变体命中区域 ⇒ 即使账号已被删除（不在库里）也要计入（删号不该抹掉已发生的用量）。
        let mut deleted = entry(now, "m", "u-deleted", 3, 4);
        deleted.variant = Some(TraeVariant::TraeWork);
        let buckets = today_gateway_buckets(
            &[deleted],
            &local_date(now),
            super::super::region::TraeRegion::Cn,
            &known,
        );
        assert_eq!(buckets.get("u-deleted"), Some(&(3, 4)));

        // 国际版日志不得混进国内版（同一 uid 在两区是不同账号）。
        let mut global = entry(now, "m", "u-global", 1, 1);
        global.variant = Some(TraeVariant::Global);
        let buckets = today_gateway_buckets(
            &[global],
            &local_date(now),
            super::super::region::TraeRegion::Cn,
            &known,
        );
        assert!(buckets.is_empty(), "国际版调用不得计入国内版");
    }

    /// `account_token_today`：缺席 ≠ 0，且客户端一档必须自报「不可得」。
    #[test]
    fn account_token_today_omits_absent_accounts_and_flags_client_unavailable() {
        let home = std::env::temp_dir().join(format!("trae-token-today-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".buddy-switch").join("trae")).expect("创建隔离目录");
        let _guard = crate::modules::config::HomeOverrideGuard::set(&home);

        let now = chrono::Local::now().timestamp_millis();
        let entries = json!([
            { "ts": now, "model": "m", "account": "u1", "status": 200, "promptTokens": 10, "completionTokens": 2, "stream": true, "variant": "trae_work" },
            { "ts": now, "model": "m", "account": "u1", "status": 200, "promptTokens": 5, "completionTokens": 1, "stream": true, "variant": "trae_work" },
            // 今天有记录、但 token 全 0（失败请求）⇒ 必须**显式**出现 0，而不是缺席。
            { "ts": now, "model": "m", "account": "u2", "status": 429, "promptTokens": 0, "completionTokens": 0, "stream": false, "variant": "trae_work" },
            // 昨天 ⇒ 不进今日桶。
            { "ts": now - 86_400_000, "model": "m", "account": "u3", "status": 200, "promptTokens": 99, "completionTokens": 99, "stream": true, "variant": "trae_work" },
        ]);
        std::fs::write(paths::api_gateway_log_file(), entries.to_string()).expect("写入日志");

        let value = account_token_today(TraeVariant::TraeWork, false);
        assert_eq!(
            value["clientSource"], "unavailable",
            "Trae 没有客户端会话落盘 ⇒ 必须自报不可得，前端据此不渲染成真实 0"
        );
        assert_eq!(value["date"], local_date(now));
        assert_eq!(value["unknownClient"]["in"], 0);
        assert_eq!(
            value["gatewayTruncated"], false,
            "日志远未到上限 ⇒ 不是下界"
        );

        let accounts = value["accounts"].as_array().expect("accounts 是数组");
        let uids: Vec<&str> = accounts
            .iter()
            .filter_map(|item| item["uid"].as_str())
            .collect();
        assert_eq!(uids, vec!["u1", "u2"], "只列今天的账号（u3 是昨天）");
        let u1 = accounts
            .iter()
            .find(|item| item["uid"] == "u1")
            .expect("u1");
        assert_eq!(u1["gatewayIn"], 15, "同一账号多条必须累加");
        assert_eq!(u1["gatewayOut"], 3);
        assert_eq!(u1["clientIn"], 0);
        let u2 = accounts
            .iter()
            .find(|item| item["uid"] == "u2")
            .expect("u2");
        assert_eq!(u2["gatewayIn"], 0, "今天的真实 0 必须显式给出");
        assert!(
            accounts.iter().all(|item| item["uid"] != "u4"),
            "从未出现的账号不得被凭空造出 0"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 日志文件已到 `logKeep` 上限 ⇒ 今日部分是下界（`gatewayTruncated`）。
    #[test]
    fn account_token_today_marks_truncated_when_log_is_full() {
        let home = std::env::temp_dir().join(format!("trae-token-trunc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".buddy-switch").join("trae")).expect("创建隔离目录");
        let _guard = crate::modules::config::HomeOverrideGuard::set(&home);

        // 配置里把 logKeep 调到 2，再写 2 条 ⇒ 视为「可能已被挤出」。
        std::fs::write(
            paths::api_gateway_file(),
            json!({ "logKeep": 2 }).to_string(),
        )
        .expect("写入网关配置");
        let now = chrono::Local::now().timestamp_millis();
        let entries = json!([
            { "ts": now, "model": "m", "account": "u1", "status": 200, "promptTokens": 1, "completionTokens": 1, "variant": "trae_work" },
            { "ts": now, "model": "m", "account": "u1", "status": 200, "promptTokens": 1, "completionTokens": 1, "variant": "trae_work" },
        ]);
        std::fs::write(paths::api_gateway_log_file(), entries.to_string()).expect("写入日志");

        let value = account_token_today(TraeVariant::TraeWork, true);
        assert_eq!(
            value["gatewayTruncated"], true,
            "文件已满 ⇒ 今日可能被裁，必须如实报下界"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn bucket_totals_add_input_and_output() {
        let mut bucket = Bucket::default();
        bucket.add(&entry(0, "m", "u", 10, 5));
        bucket.add(&entry(0, "m", "u", 1, 2));
        assert_eq!(bucket.total(), 18);
        assert_eq!(bucket.input, 11);
        assert_eq!(bucket.output, 7);
        assert_eq!(bucket.records, 2);
    }

    #[test]
    fn bucket_counts_errors_from_status_or_message() {
        let mut bucket = Bucket::default();
        let mut failed = entry(0, "m", "u", 0, 0);
        failed.status = 429;
        bucket.add(&failed);
        let mut streamed = entry(0, "m", "u", 0, 0);
        streamed.error = Some("流内错误".to_string());
        bucket.add(&streamed);
        bucket.add(&entry(0, "m", "u", 0, 0));
        assert_eq!(bucket.errors, 2);
    }

    #[test]
    fn p95_uses_nearest_rank_and_falls_back_to_max() {
        let mut bucket = Bucket::default();
        for latency in [10, 20, 30, 40, 50] {
            let mut item = entry(0, "m", "u", 0, 0);
            item.latency_ms = latency;
            bucket.add(&item);
        }
        // ceil(0.95 * 5) - 1 = 4 → 最大值。
        assert_eq!(bucket.p95_latency(), 50);

        let mut single = Bucket::default();
        single.add(&entry(0, "m", "u", 0, 0));
        assert_eq!(single.p95_latency(), 100);

        assert_eq!(Bucket::default().p95_latency(), 0);
    }

    #[test]
    fn avg_latency_is_zero_without_records() {
        assert_eq!(Bucket::default().avg_latency(), 0);
    }

    #[test]
    fn cutoff_is_disabled_for_none_and_non_positive() {
        assert!(cutoff_ms(None).is_none());
        assert!(cutoff_ms(Some(0)).is_none());
        assert!(cutoff_ms(Some(-3)).is_none());
        let cutoff = cutoff_ms(Some(7)).expect("7 天窗口应有截止时间");
        let now = chrono::Local::now().timestamp_millis();
        assert!(cutoff < now);
        assert!(now - cutoff >= 6 * 24 * 3600 * 1000);
    }

    #[test]
    fn local_date_and_hour_are_stable_strings() {
        let ts = chrono::Local::now().timestamp_millis();
        assert_eq!(local_date(ts).len(), 10, "YYYY-MM-DD");
        assert_eq!(local_hour(ts).len(), 2, "HH");
        // ★ 热力槽位 = `"{weekday}-{HH}"`：星期 **0..6（周一=0）**、小时 **00..23**。
        //   前端 `heatmapMatrix` 按 `-` 拆键，任一档越界都会**静默丢格子**，故钉死。
        let slot = local_slot(ts);
        let (weekday, hour) = slot.split_once('-').expect("槽位必须是 `{weekday}-{HH}`");
        assert!(
            (0..=6).contains(&weekday.parse::<i64>().expect("星期是数字")),
            "{slot}"
        );
        assert_eq!(hour.len(), 2, "{slot}");
        assert!(
            (0..=23).contains(&hour.parse::<i64>().expect("小时是数字")),
            "{slot}"
        );
        // 极端时间戳不 panic，退化为占位符。
        assert_eq!(local_date(i64::MIN), "未知");
        assert_eq!(local_hour(i64::MIN), "--");
        assert_eq!(local_weekday(i64::MIN), "-");
    }

    #[test]
    fn slot_keys_sort_by_weekday_then_hour() {
        // 键的**字符串升序**必须等于「周 → 小时」的自然顺序：下游沿用 `ascending()` 排序，
        // 顺序错了热力图的行列就会错位（且不会有任何报错）。
        let mut keys = vec![
            "1-00".to_string(),
            "0-23".to_string(),
            "0-00".to_string(),
            "6-09".to_string(),
        ];
        keys.sort();
        assert_eq!(keys, vec!["0-00", "0-23", "1-00", "6-09"]);
    }

    #[test]
    fn entry_parse_rejects_records_without_timestamp() {
        assert!(LogEntry::parse(&json!({"model": "m"})).is_none());
        let parsed = LogEntry::parse(&json!({
            "ts": 1000, "model": "glm-5.3", "account": "u1", "status": 200,
            "latencyMs": 42, "promptTokens": 7, "completionTokens": 3,
            "stream": true, "error": null,
        }))
        .expect("完整条目必须解析成功");
        assert_eq!(parsed.model, "glm-5.3");
        assert_eq!(parsed.prompt_tokens, 7);
        assert_eq!(parsed.completion_tokens, 3);
        assert!(parsed.stream);
        assert!(parsed.error.is_none());
    }

    #[test]
    fn entry_parse_tolerates_missing_optional_fields() {
        let parsed = LogEntry::parse(&json!({"ts": 1})).expect("只有 ts 也要能解析");
        assert_eq!(parsed.model, "");
        assert_eq!(parsed.status, 0);
        assert_eq!(parsed.latency_ms, 0);
        assert!(!parsed.stream);
    }

    #[test]
    fn descending_sorts_by_total_then_key() {
        let mut map = BTreeMap::new();
        let mut low = Bucket::default();
        low.add(&entry(0, "a", "u", 1, 1));
        let mut high = Bucket::default();
        high.add(&entry(0, "b", "u", 10, 10));
        let mut tie_a = Bucket::default();
        tie_a.add(&entry(0, "c", "u", 5, 5));
        let mut tie_b = Bucket::default();
        tie_b.add(&entry(0, "d", "u", 5, 5));
        map.insert("zz".to_string(), tie_a);
        map.insert("aa".to_string(), tie_b);
        map.insert("low".to_string(), low);
        map.insert("high".to_string(), high);

        let keys: Vec<String> = descending(map, |key, _| json!(key))
            .into_iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect();
        // total 降序；同 total 按键升序。
        assert_eq!(keys, vec!["high", "aa", "zz", "low"]);
    }
}
