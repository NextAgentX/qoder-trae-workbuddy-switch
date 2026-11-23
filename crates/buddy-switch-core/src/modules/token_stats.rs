//! 本地 WorkBuddy / CodeBuddy CLI / CodeBuddy IDE Token 统计。
//!
//! 这个模块是统计数据的唯一归属：日志只在这里解码、去重和按时间聚合，
//! Tauri 与 HTTP 层只负责转发结果。响应只包含聚合数字和脱敏标识，不返回
//! 消息正文、arguments 或认证信息。
//!
//! CodeBuddy IDE 不写 JSONL，用量在 `CodeBuddyExtension/Data/**/history/**/index.json`
//! 的 `requests[].usage` 中；消息正文文件（`messages/`）不会被扫描。

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use crate::modules::config::{atomic_write, store_dir};
use crate::modules::region::{Region, RegionFilter};

/* ⚠️ 不能 derive `Eq`：`credit` 是 f64（上游给的是小数）。
缓存比对走的是 `PersistedTotals`（把 credit 换成整数微积分），不依赖 `Usage: Eq`。 */
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Usage {
    input: u64,
    output: u64,
    read: u64,
    write: u64,
    /// **上游上报的积分消耗**（`usage.credit`）。
    ///
    /// 为什么必须有：用量页的「积分」视图要按来源拆消耗（客户端 / 网关），
    /// 而按 token 折算出来的是估算、不是账单（issue #63）。jsonl 里本来就带这个字段
    /// （实测本机 1316 处，形如 `"credit":1.28`），只是此前没人解析。
    ///
    /// `0.0` = 该记录没带 credit（= 不知道），不是「免费」。
    credit: f64,
}

#[derive(Clone, Debug, Default)]
struct Totals {
    usage: Usage,
    records: u64,
}

#[derive(Clone, Debug)]
struct SessionTotals {
    key: String,
    title: Option<String>,
    project: String,
    session_id: String,
    totals: Totals,
    /// ★ **会话逐日拆分**（2026-10-15 新增）—— 前端「会话」视图按时间窗口切片的**唯一素材**。
    ///
    /// 为什么必须由后端给：会话 = 一个 jsonl 一条，`totals` 只有全时合计，没有时间维度
    /// ⇒ 前端拿到它也无法回答「近 7 天这个会话用了多少」（用户报障：切时间统计不变）。
    /// 逐日拆分在**解析单个文件时就能顺手算出**（记录自带 `timestamp`），与文件级
    /// `daily` 同源同口径（`date()`、缺失时间戳的记录两边都不落键）。
    ///
    /// ⚠️ 与文件级 `daily` 的区别：那个是**跨会话**按日合并，这个是**单会话**的
    /// （单文件 = 单会话 ⇒ 就是该文件的 `daily`，但现有实现里两者各自独立累加）。
    daily: HashMap<String, Totals>,
}

impl Totals {
    fn add(&mut self, usage: Usage) {
        self.usage.input = self.usage.input.saturating_add(usage.input);
        self.usage.output = self.usage.output.saturating_add(usage.output);
        self.usage.read = self.usage.read.saturating_add(usage.read);
        self.usage.write = self.usage.write.saturating_add(usage.write);
        self.usage.credit += usage.credit;
        self.records = self.records.saturating_add(1);
    }

    /// 把另一个 `Totals` 累加进来（按文件解析 + 有序合并的等价性基础）。
    ///
    /// 与原实现的**逐记录**累加等价：`saturating_add` 满足交换律与结合律，
    /// 所以「先按文件求和、再按文件顺序合并」与「按记录顺序累加」结果相同。
    fn merge(&mut self, other: &Totals) {
        self.usage.input = self.usage.input.saturating_add(other.usage.input);
        self.usage.output = self.usage.output.saturating_add(other.usage.output);
        self.usage.read = self.usage.read.saturating_add(other.usage.read);
        self.usage.write = self.usage.write.saturating_add(other.usage.write);
        self.usage.credit += other.usage.credit;
        self.records = self.records.saturating_add(other.records);
    }

    fn value(&self) -> Value {
        let cache_hit_rate =
            (self.usage.input > 0).then(|| self.usage.read as f64 / self.usage.input as f64);
        // `input` already includes cache reads; expose the same headline total
        // used by the dashboard without double-counting the cached portion.
        let total = self
            .usage
            .input
            .saturating_add(self.usage.output)
            .saturating_add(self.usage.write);
        json!({
            "total": total,
            "input": self.usage.input,
            "output": self.usage.output,
            "cacheRead": self.usage.read,
            "cacheWrite": self.usage.write,
            "uncachedInput": self.usage.input.saturating_sub(self.usage.read),
            "records": self.records,
            "cacheHitRate": cache_hit_rate,
            /* 积分：保留 3 位（上游给的是 `1.28` 这种小数；四舍五入到 3 位足够，
               且避免 f64 尾数把 `0.1+0.2` 那类噪声带到 UI）。 */
            "credit": (self.usage.credit * 1000.0).round() / 1000.0,
        })
    }
}

/// Read a non-negative integer from a JSON number or string.
fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| {
            value
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n as u64)
        })
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn field(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(number))
}

fn positive_field(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(number).filter(|value| *value > 0))
}

fn cached_input_field(object: &Map<String, Value>) -> u64 {
    // Providers have emitted both flat aliases and OpenAI-compatible nested
    // details. Prefer a positive flat alias so a stale `cache_read...: 0`
    // field cannot hide a populated `prompt_cache_hit_tokens` value.
    positive_field(
        object,
        &[
            "cache_read_input_tokens",
            "cacheReadInputTokens",
            "prompt_cache_hit_tokens",
            "cached_tokens",
        ],
    )
    .or_else(|| {
        object
            .get("prompt_tokens_details")
            .and_then(Value::as_object)
            .and_then(|details| positive_field(details, &["cached_tokens"]))
    })
    .or_else(|| {
        object
            .get("inputTokensDetails")
            .and_then(Value::as_array)
            .and_then(|details| {
                details.iter().find_map(|detail| {
                    detail
                        .as_object()
                        .and_then(|detail| positive_field(detail, &["cached_tokens"]))
                })
            })
    })
    .unwrap_or(0)
}

const CACHE_WRITE_KEYS: &[&str] = &[
    "cache_write_input_tokens",
    "cacheWriteInputTokens",
    "cache_creation_input_tokens",
    "prompt_cache_write_tokens",
];

/// 读 `usage.credit`（上游积分消耗）。别名与 token 字段同一风格：
/// 优先驼峰，其次蛇形；负数 / 非有限值一律当缺失（积分不会是负的）。
fn credit_field(object: &Map<String, Value>) -> f64 {
    ["credit", "credits", "creditUsed"]
        .iter()
        .find_map(|key| {
            object
                .get(*key)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value >= 0.0)
        })
        .unwrap_or(0.0)
}

fn usage_fields(object: &Map<String, Value>) -> Usage {
    Usage {
        input: field(object, &["input_tokens", "inputTokens", "prompt_tokens"]).unwrap_or(0),
        output: field(
            object,
            &["output_tokens", "outputTokens", "completion_tokens"],
        )
        .unwrap_or(0),
        read: cached_input_field(object),
        write: positive_field(object, CACHE_WRITE_KEYS).unwrap_or(0),
        credit: credit_field(object),
    }
}

fn usage_object(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value?.as_object().filter(|object| {
        // Input is the required anchor for a usage record. It may legitimately
        // be zero (for example a provider reports output-only retries), so do
        // not use `input > 0` as the validity check.
        field(object, &["input_tokens", "inputTokens", "prompt_tokens"]).is_some()
    })
}

/// Decode one record. Usage precedence is message.usage > providerData.usage >
/// top-level usage. Cache-write metadata may only exist on a non-selected
/// usage object or rawUsage, so those objects are consulted without counting
/// their input/output again.
fn usage(value: &Value) -> Option<Usage> {
    let provider = value.get("providerData");
    let candidates = [
        value
            .get("message")
            .and_then(|message| message.get("usage")),
        provider.and_then(|data| data.get("usage")),
        value.get("usage"),
    ];
    let selected = candidates.iter().copied().find_map(usage_object)?;
    let mut result = usage_fields(selected);

    if result.write == 0 {
        result.write = candidates
            .iter()
            .copied()
            .filter_map(|candidate| candidate.and_then(Value::as_object))
            .chain(
                provider
                    .and_then(|data| data.get("rawUsage"))
                    .and_then(Value::as_object),
            )
            .find_map(|object| positive_field(object, CACHE_WRITE_KEYS))
            .unwrap_or(0);
    }

    // prompt_cache_miss_tokens is deliberately not a write alias: current
    // WorkBuddy/CodeBuddy logs use it for newly computed (uncached) input,
    // while their explicit cache-write fields may legitimately remain zero.

    Some(result)
}

fn timestamp(value: &Value) -> Option<i64> {
    value
        .get("timestamp")
        .or_else(|| value.get("ts"))
        .and_then(|timestamp| {
            timestamp
                .as_i64()
                .or_else(|| timestamp.as_u64().and_then(|n| i64::try_from(n).ok()))
                .or_else(|| timestamp.as_str()?.trim().parse::<i64>().ok())
        })
}

fn date(value: &Value) -> Option<String> {
    let timestamp = timestamp(value)?;
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|date| date.with_timezone(&Local).format("%Y-%m-%d").to_string())
}

fn hour(value: &Value) -> Option<String> {
    let timestamp = timestamp(value)?;
    chrono::DateTime::from_timestamp_millis(timestamp).map(|date| {
        let local = date.with_timezone(&Local);
        format!(
            "{}-{}",
            local.weekday().num_days_from_monday(),
            local.hour()
        )
    })
}

/// 本地日期与小时组成的时间轴桶（`YYYY-MM-DD-HH`），用于「今天」趋势。
fn daily_hour(value: &Value) -> Option<String> {
    let timestamp = timestamp(value)?;
    chrono::DateTime::from_timestamp_millis(timestamp).map(|date| {
        let local = date.with_timezone(&Local);
        format!("{}-{:02}", local.format("%Y-%m-%d"), local.hour())
    })
}

fn model(value: &Value) -> String {
    value
        .get("providerData")
        .and_then(|data| data.get("model"))
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or("未知模型")
        .to_string()
}

fn files(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Subagent logs duplicate parent-session context and are not part
            // of either product's primary usage accounting.
            if path.file_name().and_then(|name| name.to_str()) != Some("subagents") {
                files(&path, output);
            }
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            output.push(path);
        }
    }
}

fn project_name(root: &Path, file: &Path) -> String {
    let name = file
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .filter(|name| !name.is_empty() && !name.ends_with(".jsonl"));
    match name {
        // Product directories commonly encode the complete absolute path.
        // Returning that would leak a user name and parent directories.
        Some(name) if !name.starts_with("Users-") && !name.starts_with("home-") => name.to_string(),
        _ => "未知项目".to_string(),
    }
}

fn record_project(value: &Value, fallback: &str) -> String {
    value
        .get("cwd")
        .and_then(Value::as_str)
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.len() <= 120)
        .unwrap_or(fallback)
        .to_string()
}

fn non_empty_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn groups(groups: HashMap<String, Totals>) -> Vec<Value> {
    let mut values: Vec<_> = groups
        .into_iter()
        .map(|(key, totals)| {
            let mut value = totals.value();
            value["key"] = json!(key);
            value
        })
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(total_value(v)));
    values
}

fn session_groups(sessions: Vec<SessionTotals>) -> Vec<Value> {
    let mut values: Vec<_> = sessions
        .into_iter()
        .map(|session| {
            let mut value = session.totals.value();
            value["key"] = json!(session.key);
            value["title"] = json!(session.title);
            value["project"] = json!(session.project);
            value["sessionId"] = json!(session.session_id);
            // ★ 逐日拆分（前端按窗口切片用；空表也**照发** —— 「有字段但空」与
            //   「旧后端没有字段」是两件事，前端据此选择「窗口口径」还是「全时回落」）。
            value["daily"] = Value::Array(groups(session.daily));
            value
        })
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(total_value(v)));
    values
}

fn total_value(value: &Value) -> u64 {
    value
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            value
                .get("input")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_add(value.get("output").and_then(Value::as_u64).unwrap_or(0))
                .saturating_add(value.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0))
        })
}

#[derive(Default)]
struct SourceCollector {
    total: Totals,
    models: HashMap<String, Totals>,
    projects: HashMap<String, Totals>,
    sessions: Vec<SessionTotals>,
    daily: HashMap<String, Totals>,
    daily_by_model: HashMap<String, HashMap<String, Totals>>,
    daily_hours: HashMap<String, Totals>,
    daily_hours_by_model: HashMap<String, HashMap<String, Totals>>,
    hours: HashMap<String, Totals>,
    parse_errors: u64,
    coverage_start_at: Option<i64>,
    coverage_end_at: Option<i64>,
    session_key_counts: HashMap<String, usize>,
}

impl SourceCollector {
    fn note_parse_error(&mut self) {
        self.parse_errors = self.parse_errors.saturating_add(1);
    }

    fn add(&mut self, usage: Usage, value: &Value, project: &str) {
        self.total.add(usage);
        let model_name = model(value);
        self.models
            .entry(model_name.clone())
            .or_default()
            .add(usage);
        self.projects
            .entry(project.to_string())
            .or_default()
            .add(usage);
        if let Some(day) = date(value) {
            self.daily.entry(day.clone()).or_default().add(usage);
            self.daily_by_model
                .entry(model_name.clone())
                .or_default()
                .entry(day)
                .or_default()
                .add(usage);
        }
        if let Some(hour) = hour(value) {
            self.hours.entry(hour).or_default().add(usage);
        }
        if let Some(hour_key) = daily_hour(value) {
            self.daily_hours
                .entry(hour_key.clone())
                .or_default()
                .add(usage);
            self.daily_hours_by_model
                .entry(model_name)
                .or_default()
                .entry(hour_key)
                .or_default()
                .add(usage);
        }
        if let Some(timestamp) = timestamp(value) {
            self.coverage_start_at = Some(
                self.coverage_start_at
                    .map_or(timestamp, |current| current.min(timestamp)),
            );
            self.coverage_end_at = Some(
                self.coverage_end_at
                    .map_or(timestamp, |current| current.max(timestamp)),
            );
        }
    }

    fn push_session(
        &mut self,
        session_id: String,
        title: Option<String>,
        project: String,
        totals: Totals,
        daily: HashMap<String, Totals>,
    ) {
        if totals.records == 0 {
            return;
        }
        let base_key = format!("{project} · {session_id}");
        let count = self.session_key_counts.entry(base_key.clone()).or_default();
        *count += 1;
        let key = if *count == 1 {
            base_key
        } else {
            format!("{base_key} · {}", *count)
        };
        self.sessions.push(SessionTotals {
            key,
            title,
            project,
            session_id,
            totals,
            daily,
        });
    }

    /// 合并**另一个文件**的解析结果（`SourceCollector` 在此被当作「单文件累加器」用）。
    ///
    /// ## 为什么 `key` 必须在这里**重算**
    ///
    /// [`Self::push_session`] 的 `key` 带跨文件去重后缀（`项目 · 会话 · 2`），
    /// 计数器 `session_key_counts` 属于**整个 source**、不属于单个文件。并行/缓存
    /// 路径下每个文件各自从 1 开始计数 ⇒ 直接搬 `other.sessions` 里的 `key` 会把
    /// 后缀算错。所以这里只取原始四元组、由 `self` 重新 `push_session`。
    ///
    /// 顺序语义与改造前一致：调用方按 `paths.sort()` 的**文件顺序**逐个 `absorb`，
    /// 与旧实现「按同一顺序逐文件 `push_session`」逐字对应。
    fn absorb(&mut self, other: &SourceCollector) {
        self.total.merge(&other.total);
        for (key, totals) in &other.models {
            self.models.entry(key.clone()).or_default().merge(totals);
        }
        for (key, totals) in &other.projects {
            self.projects.entry(key.clone()).or_default().merge(totals);
        }
        for (key, totals) in &other.daily {
            self.daily.entry(key.clone()).or_default().merge(totals);
        }
        for (model, days) in &other.daily_by_model {
            let target = self.daily_by_model.entry(model.clone()).or_default();
            for (day, totals) in days {
                target.entry(day.clone()).or_default().merge(totals);
            }
        }
        for (key, totals) in &other.hours {
            self.hours.entry(key.clone()).or_default().merge(totals);
        }
        for (key, totals) in &other.daily_hours {
            self.daily_hours
                .entry(key.clone())
                .or_default()
                .merge(totals);
        }
        for (model, points) in &other.daily_hours_by_model {
            let target = self.daily_hours_by_model.entry(model.clone()).or_default();
            for (key, totals) in points {
                target.entry(key.clone()).or_default().merge(totals);
            }
        }
        self.parse_errors = self.parse_errors.saturating_add(other.parse_errors);
        self.coverage_start_at = min_option(self.coverage_start_at, other.coverage_start_at);
        self.coverage_end_at = max_option(self.coverage_end_at, other.coverage_end_at);
        for session in &other.sessions {
            self.push_session(
                session.session_id.clone(),
                session.title.clone(),
                session.project.clone(),
                session.totals.clone(),
                session.daily.clone(),
            );
        }
    }

    fn into_value(self, name: &str, files_scanned: usize) -> Value {
        let daily_by_model = self
            .daily_by_model
            .into_iter()
            .map(|(model, points)| (model, Value::Array(groups(points))))
            .collect::<Map<String, Value>>();
        let daily_hours_by_model = self
            .daily_hours_by_model
            .into_iter()
            .map(|(model, points)| (model, Value::Array(groups(points))))
            .collect::<Map<String, Value>>();
        json!({
            "source": name,
            "summary": self.total.value(),
            "models": groups(self.models),
            "projects": groups(self.projects),
            "sessions": session_groups(self.sessions),
            "daily": groups(self.daily),
            "dailyByModel": daily_by_model,
            "hours": groups(self.hours),
            "dailyHours": groups(self.daily_hours),
            "dailyHoursByModel": daily_hours_by_model,
            "filesScanned": files_scanned,
            "parseErrors": self.parse_errors,
            "coverageStartAt": self.coverage_start_at,
            "coverageEndAt": self.coverage_end_at,
        })
    }
}

/// 解析**单个** `.jsonl` 文件，返回只含该文件贡献的累加器。
///
/// 与改造前的逐文件循环体逐字一致 —— 唯一区别是「累加器按文件新建」而不是
/// 「全源共用一个」，合并交给 [`SourceCollector::absorb`]。
fn parse_file(root: &Path, path: &Path, cutoff: Option<i64>) -> SourceCollector {
    let mut collector = SourceCollector::default();
    #[cfg(test)]
    PARSED_PATHS.lock().unwrap().push(path.to_path_buf());

    let session_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("未知会话")
        .to_string();
    let fallback_project = project_name(root, path);
    let Ok(file) = std::fs::File::open(path) else {
        collector.note_parse_error();
        return collector;
    };

    let mut session_totals = Totals::default();
    let mut session_daily: HashMap<String, Totals> = HashMap::new();
    let mut session_project: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut summary: Option<String> = None;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            collector.note_parse_error();
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            collector.note_parse_error();
            continue;
        };
        // Title metadata belongs to the whole JSONL session file. Read it
        // before applying the usage cutoff so an older title can still
        // label usage that falls inside the selected range. aiTitle has
        // precedence over summary regardless of event order.
        if let Some(title) = non_empty_text(value.get("aiTitle")) {
            ai_title = Some(title);
        }
        if let Some(value) = non_empty_text(value.get("summary")) {
            summary = Some(value);
        }
        // Records with a missing timestamp are excluded from a bounded
        // range rather than guessed from file mtime or browser time.
        if cutoff.is_some_and(|minimum| timestamp(&value).is_none_or(|ts| ts < minimum)) {
            continue;
        }
        let Some(usage) = usage(&value) else {
            continue;
        };
        let project = record_project(&value, &fallback_project);
        if session_project.is_none() {
            session_project = Some(project.clone());
        }
        session_totals.add(usage);
        // 会话逐日拆分：与文件级 `daily` 同判据（`date()` 可算才落键）。
        if let Some(day) = date(&value) {
            session_daily.entry(day).or_default().add(usage);
        }
        collector.add(usage, &value, &project);
    }

    collector.push_session(
        session_id,
        ai_title.or(summary),
        session_project.unwrap_or(fallback_project),
        session_totals,
        session_daily,
    );
    collector
}

fn min_option(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, right) => right,
    }
}

fn max_option(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, right) => right,
    }
}

/// `(mtime 毫秒, 文件字节数)` —— 文件内容的**充分指纹**。
///
/// 日志是**追加写**：任何一次写入都会同时推进 mtime 与 size，所以这对值变了
/// 就说明内容变了。反过来，内容变了而这对值不变在本文件系统的粒度下不可能
/// （NTFS mtime 精度 100ns，且 size 单独把关「同刻度内追加」）。
fn file_stamp(meta: &std::fs::Metadata) -> Option<(i64, u64)> {
    let millis = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some((i64::try_from(millis).ok()?, meta.len()))
}

/// 单文件解析结果的**进程内**缓存条目。
struct ScanCacheEntry {
    mtime_ms: i64,
    size: u64,
    cutoff: Option<i64>,
    scan: Arc<SourceCollector>,
}

/// 按**绝对路径**缓存单文件解析结果（进程内）。
///
/// ## 为什么可以缓存，且命中即逐字节等价
///
/// 真实流量只有 **`cutoff = None`** 一种：`TokenStatsPage` 传 `undefined`，
/// `overview-metrics` 传 `TREND_DAYS = 14` 天 —— 而 `days` 只在
/// [`get_statistics_for_filter`] 里归一成 `Some(7 | 30 | 90)`，其余一律 `None`。
/// 因此把 `cutoff` 放进键做**精确比对**后，命中返回的就是「同一份文件、同一个
/// 窗口」的解析结果，与重新解析逐字节相同。
///
/// `days` 恰好取 7 / 30 / 90 的调用（目前无调用方）会因 `cutoff` 含 `now_ms()`
/// 而必然未命中 ⇒ 行为与改造前一致（全量重扫），不会引入陈旧窗口。
///
/// ## 为什么**必须落盘**（2026-10-10 追加，实测驱动）
///
/// 先量化（`token_scan_bench read`）：`~/.workbuddy-ai/projects` 34.5GB / 12356 文件，
/// **纯读盘 65.0s、且「热」与「冷」一样（64.0s）** —— 35GB 塞不进 31.7GB 内存，
/// 页缓存完全不帮忙。而「读 + 解析 + 聚合」总共 72.1s ⇒ **解析只占 ~7s，90% 是 I/O**。
/// 结论：再优化 CPU 侧没有意义，**唯一出路是不重读**。
///
/// 原注释把「不落盘」写成设计属性，评审给的边界是「可加内存缓存、但不可落盘」，
/// 理由是怕「落库聚合值与真实日志不一致」。**该理由对内容指纹键不成立**：
/// 条目只在 `(path, mtime, size, cutoff)` **逐字段相等**时才被使用，文件一改就必然
/// 未命中 ⇒ 陈旧在结构上不可能。故落盘安全，且是让「冷启动」从 65s 降到亚秒的唯一手段。
fn scan_cache() -> &'static Mutex<HashMap<PathBuf, ScanCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, ScanCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 仅供测试：移除**某一个路径**的缓存条目。
///
/// 刻意做成按路径清除而不是整表清空：`cargo test` 并行跑用例，整表清空会把
/// 别的用例正在做的「冷 → 热」序列打断，让它们的「命中不再解析」断言随机变红。
#[cfg(test)]
fn clear_scan_cache_entry(path: &Path) {
    scan_cache().lock().unwrap().remove(path);
}

// ---------------------------------------------------------------------------
// 落盘表示
// ---------------------------------------------------------------------------

/// 落盘格式版本。**任何字段语义变化都必须 +1** —— 不匹配时整体丢弃
/// （宁可重扫一次，不可读出错值）。
///
/// v2（2026-10-15）：`PersistedSession` 新增 `daily`（会话逐日拆分，供「会话」视图
/// 按时间窗口切片）。旧 v1 缓存整体丢弃 ⇒ 首次全量重扫一次；这正是版本机制的目的。
///
/// v4（issue #223）：缓存逐日小时桶，旧缓存需要重建。
const SCAN_CACHE_VERSION: u32 = 4;

/// `Totals` 的紧凑表示：`[input, output, read, write, records, creditMicros]`。
///
/// ★ `credit` 存**微积分**（`credit × 1e6` 取整）而不是 f64：这个数组要能 `Eq`
///   （缓存比对靠它），而 f64 没有 `Eq`。1e-6 的精度对积分足够（上游是 `1.28` 这种两位小数）。
///
/// ## 为什么不直接 `derive(Serialize)` 到 `Totals`
///
/// 一个文件里有几十个 `Totals`（models / projects / daily / hours / dailyByModel
/// 各一份），12356 个文件 ⇒ 数百万个。若每个都带 `"input":`/`"output":`/… 键名，
/// 光键名就要几百 MB。定长数组没有键名开销。
type PersistedTotals = [u64; 6];

fn totals_to_persisted(totals: &Totals) -> PersistedTotals {
    [
        totals.usage.input,
        totals.usage.output,
        totals.usage.read,
        totals.usage.write,
        totals.records,
        (totals.usage.credit * 1_000_000.0).round() as u64,
    ]
}

fn totals_from_persisted(value: PersistedTotals) -> Totals {
    Totals {
        usage: Usage {
            input: value[0],
            output: value[1],
            read: value[2],
            write: value[3],
            credit: value[5] as f64 / 1_000_000.0,
        },
        records: value[4],
    }
}

/// `HashMap` → **按 key 排序**的数组。
///
/// 排序不是为了正确性（合并顺序由 `absorb` 的调用顺序决定，与这里无关），
/// 而是为了让同一份数据**每次落盘产生相同的字节** —— 否则 `HashMap` 的随机
/// 迭代序会让缓存文件每次都变，既无法 diff，也白白多写盘。
fn totals_map_to_persisted(map: &HashMap<String, Totals>) -> Vec<(String, PersistedTotals)> {
    let mut entries: Vec<(String, PersistedTotals)> = map
        .iter()
        .map(|(key, totals)| (key.clone(), totals_to_persisted(totals)))
        .collect();
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn totals_map_from_persisted(entries: Vec<(String, PersistedTotals)>) -> HashMap<String, Totals> {
    entries
        .into_iter()
        .map(|(key, totals)| (key, totals_from_persisted(totals)))
        .collect()
}

fn daily_by_model_to_persisted(
    map: &HashMap<String, HashMap<String, Totals>>,
) -> Vec<(String, Vec<(String, PersistedTotals)>)> {
    let mut entries: Vec<(String, Vec<(String, PersistedTotals)>)> = map
        .iter()
        .map(|(model, days)| (model.clone(), totals_map_to_persisted(days)))
        .collect();
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn daily_by_model_from_persisted(
    entries: Vec<(String, Vec<(String, PersistedTotals)>)>,
) -> HashMap<String, HashMap<String, Totals>> {
    entries
        .into_iter()
        .map(|(model, days)| (model, totals_map_from_persisted(days)))
        .collect()
}

/// 一个文件在盘上的会话信息。
///
/// `Option`：`push_session` 在 `records == 0` 时**提前返回**，所以一个文件可能
/// **没有**会话条目（例如整份日志都没有 usage）。这时必须保持「没有」，
/// 否则 `absorb` 会凭空 push 一个空会话。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSession {
    session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    project: String,
    totals: PersistedTotals,
    /// ★ 会话逐日拆分（v2 新增；键是本地日 `YYYY-MM-DD`）。空表照省 ——
    /// 「空」与「旧格式没有」在消费端是两回事（前端据字段有无选择窗口口径或全时回落）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    daily: Vec<(String, PersistedTotals)>,
}

/// 一个文件的落盘条目：内容指纹 + 该文件的全部贡献。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedFileEntry {
    mtime: i64,
    size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cutoff: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<PersistedSession>,
    total: PersistedTotals,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    models: Vec<(String, PersistedTotals)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    projects: Vec<(String, PersistedTotals)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    daily: Vec<(String, PersistedTotals)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    daily_by_model: Vec<(String, Vec<(String, PersistedTotals)>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    daily_hours: Vec<(String, PersistedTotals)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    daily_hours_by_model: Vec<(String, Vec<(String, PersistedTotals)>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hours: Vec<(String, PersistedTotals)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coverage_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coverage_end: Option<i64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    parse_errors: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Serialize, Deserialize)]
struct PersistedScanCache {
    version: u32,
    files: BTreeMap<String, PersistedFileEntry>,
}

impl ScanCacheEntry {
    fn to_persisted(&self) -> PersistedFileEntry {
        let scan = &self.scan;
        let session = scan.sessions.first().map(|session| PersistedSession {
            session_id: session.session_id.clone(),
            title: session.title.clone(),
            project: session.project.clone(),
            totals: totals_to_persisted(&session.totals),
            daily: totals_map_to_persisted(&session.daily),
        });
        PersistedFileEntry {
            mtime: self.mtime_ms,
            size: self.size,
            cutoff: self.cutoff,
            session,
            total: totals_to_persisted(&scan.total),
            models: totals_map_to_persisted(&scan.models),
            projects: totals_map_to_persisted(&scan.projects),
            daily: totals_map_to_persisted(&scan.daily),
            daily_by_model: daily_by_model_to_persisted(&scan.daily_by_model),
            daily_hours: totals_map_to_persisted(&scan.daily_hours),
            daily_hours_by_model: daily_by_model_to_persisted(&scan.daily_hours_by_model),
            hours: totals_map_to_persisted(&scan.hours),
            coverage_start: scan.coverage_start_at,
            coverage_end: scan.coverage_end_at,
            parse_errors: scan.parse_errors,
        }
    }

    fn from_persisted(entry: PersistedFileEntry) -> Self {
        let mut scan = SourceCollector {
            total: totals_from_persisted(entry.total),
            models: totals_map_from_persisted(entry.models),
            projects: totals_map_from_persisted(entry.projects),
            daily: totals_map_from_persisted(entry.daily),
            daily_by_model: daily_by_model_from_persisted(entry.daily_by_model),
            daily_hours: totals_map_from_persisted(entry.daily_hours),
            daily_hours_by_model: daily_by_model_from_persisted(entry.daily_hours_by_model),
            hours: totals_map_from_persisted(entry.hours),
            coverage_start_at: entry.coverage_start,
            coverage_end_at: entry.coverage_end,
            parse_errors: entry.parse_errors,
            ..SourceCollector::default()
        };
        if let Some(session) = entry.session {
            scan.sessions.push(SessionTotals {
                // `key` 由 `absorb` 重算（跨文件去重计数不属于单个文件），此处占位。
                key: String::new(),
                title: session.title,
                project: session.project,
                session_id: session.session_id,
                totals: totals_from_persisted(session.totals),
                daily: totals_map_from_persisted(session.daily),
            });
        }
        ScanCacheEntry {
            mtime_ms: entry.mtime,
            size: entry.size,
            cutoff: entry.cutoff,
            scan: Arc::new(scan),
        }
    }
}

/// 落盘位置。**单一文件、不按 region 分家**：键是**绝对路径**，
/// `.workbuddy/projects`（CN）与 `.workbuddy-ai/projects`（Global）天然不会撞车，
/// 因此不存在 `token_daily` 那种「两版串台」的风险。
fn scan_cache_path() -> PathBuf {
    store_dir().join("token-scan-cache.json")
}

/// `ensure_cache_loaded` 的可复位句柄（`OnceLock` 无法复位，故用 `AtomicBool`）。
static CACHE_LOADED_FLAG: AtomicBool = AtomicBool::new(false);

/// 进程内**有过新解析** —— 只有这时才值得写盘。
static CACHE_DIRTY: AtomicBool = AtomicBool::new(false);

/// 从指定路径读缓存。`None` = **没有可用缓存**（文件不存在 / 坏 JSON / 版本不符）。
///
/// 抽成显式路径的纯函数，一是让 `ensure_cache_loaded` 保持单行语义，
/// 二是让用例能直接喂临时文件测试，**不必依赖进程级一次性标记** ——
/// `cargo test` 并行跑用例，全局标记会被别的用例踩。
fn read_persisted_cache(path: &Path) -> Option<HashMap<PathBuf, ScanCacheEntry>> {
    let text = std::fs::read_to_string(path).ok()?;
    let persisted = serde_json::from_str::<PersistedScanCache>(&text).ok()?;
    if persisted.version != SCAN_CACHE_VERSION {
        return None;
    }
    Some(
        persisted
            .files
            .into_iter()
            .map(|(file, entry)| (PathBuf::from(file), ScanCacheEntry::from_persisted(entry)))
            .collect(),
    )
}

/// 进程内只从盘上装载一次。**任何失败都静默回落成空缓存**：缓存是纯加速，
/// 读不到就重扫，绝不能因为缓存坏了让统计失败。
fn ensure_cache_loaded() {
    if CACHE_LOADED_FLAG.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(loaded) = read_persisted_cache(&scan_cache_path()) else {
        return;
    };
    let mut cache = scan_cache().lock().unwrap();
    for (path, entry) in loaded {
        cache.insert(path, entry);
    }
}

fn write_persisted_cache(path: &Path, cache: &HashMap<PathBuf, ScanCacheEntry>) {
    // 丢掉文件已不存在的条目：否则缓存只增不减（用户删掉旧会话后条目永远留着）。
    // 这是**纯清理**，不影响正确性 —— 不存在的文件本来也不会被查询。
    let files: BTreeMap<String, PersistedFileEntry> = cache
        .iter()
        .filter(|(file, _)| file.exists())
        .map(|(file, entry)| (file.to_string_lossy().into_owned(), entry.to_persisted()))
        .collect();
    let payload = PersistedScanCache {
        version: SCAN_CACHE_VERSION,
        files,
    };
    let Ok(text) = serde_json::to_string(&payload) else {
        return;
    };
    let _ = std::fs::create_dir_all(store_dir());
    let _ = atomic_write(path, &text);
}

/// 有新解析时把缓存落盘。**没有新解析就一个字节都不写** —— 热路径零成本。
fn persist_scan_cache_if_dirty() {
    if !CACHE_DIRTY.swap(false, Ordering::AcqRel) {
        return;
    }
    let cache = scan_cache().lock().unwrap();
    write_persisted_cache(&scan_cache_path(), &cache);
}

/// 仅用于测试断言「缓存命中时**确实没有**再解析」。发布构建零成本。
///
/// ⚠️ 刻意按**路径**记录而不是用全局计数：`cargo test` 在同一个二进制里**并行**
/// 跑用例，而本模块有若干用例会真的去扫真实 home（如
/// `get_statistics_for_global_reads_only_its_own_directory`，上万个文件）。
/// 全局计数会被它们污染，`assert_eq!(计数, 1)` 变成必然失败的假阳性。
/// 按路径过滤后，断言只受**本用例自己的临时目录**影响。
#[cfg(test)]
static PARSED_PATHS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// 测试专用：某个文件被真正解析过几次。
#[cfg(test)]
fn parse_count_for(path: &Path) -> usize {
    PARSED_PATHS
        .lock()
        .unwrap()
        .iter()
        .filter(|recorded| recorded.as_path() == path)
        .count()
}

/// 并行度：够用即可。上限 32 是刻意的 —— 再多只会在同一条磁盘队列上排队。
fn worker_count(pending: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    cores.clamp(1, 32).min(pending.max(1))
}

/// 解析一个源目录下的全部文件：**先查缓存，未命中的并行解析**，按 `paths` 顺序返回。
///
/// 顺序是硬要求：`sessions` 的 `key` 去重后缀、以及同名分组的**首次出现顺序**
/// 都依赖「按 `paths.sort()` 逐个 absorb」，并行只能发生在**单个文件内部**。
fn scan_paths(root: &Path, paths: &[PathBuf], cutoff: Option<i64>) -> Vec<Arc<SourceCollector>> {
    ensure_cache_loaded();

    let mut slots: Vec<Option<Arc<SourceCollector>>> = (0..paths.len()).map(|_| None).collect();
    let mut pending: Vec<usize> = Vec::new();

    {
        let cache = scan_cache().lock().unwrap();
        for (index, path) in paths.iter().enumerate() {
            let stamp = std::fs::metadata(path)
                .ok()
                .and_then(|meta| file_stamp(&meta));
            let hit = stamp.and_then(|(mtime_ms, size)| {
                cache
                    .get(path)
                    .filter(|entry| {
                        entry.mtime_ms == mtime_ms && entry.size == size && entry.cutoff == cutoff
                    })
                    .map(|entry| entry.scan.clone())
            });
            match hit {
                Some(scan) => slots[index] = Some(scan),
                // 元数据读不到 ⇒ 与原实现一致：开文件会失败，记一次 parse error。
                None => pending.push(index),
            }
        }
    }

    if !pending.is_empty() {
        CACHE_DIRTY.store(true, Ordering::Release);

        let next = AtomicUsize::new(0);
        let parsed: Mutex<Vec<(usize, SourceCollector)>> =
            Mutex::new(Vec::with_capacity(pending.len()));
        std::thread::scope(|scope| {
            for _ in 0..worker_count(pending.len()) {
                scope.spawn(|| loop {
                    let cursor = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&index) = pending.get(cursor) else {
                        break;
                    };
                    let scan = parse_file(root, &paths[index], cutoff);
                    parsed.lock().unwrap().push((index, scan));
                });
            }
        });

        let mut cache = scan_cache().lock().unwrap();
        for (index, scan) in parsed.into_inner().unwrap() {
            let scan = Arc::new(scan);
            if let Some((mtime_ms, size)) = std::fs::metadata(&paths[index])
                .ok()
                .and_then(|meta| file_stamp(&meta))
            {
                cache.insert(
                    paths[index].clone(),
                    ScanCacheEntry {
                        mtime_ms,
                        size,
                        cutoff,
                        scan: scan.clone(),
                    },
                );
            }
            slots[index] = Some(scan);
        }
    }

    slots
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| Arc::new(SourceCollector::default())))
        .collect()
}

fn source(root: PathBuf, name: &str, cutoff: Option<i64>) -> Value {
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort();
    let files_scanned = paths.len();

    let mut collector = SourceCollector::default();
    for scan in scan_paths(&root, &paths, cutoff) {
        collector.absorb(&scan);
    }
    collector.into_value(name, files_scanned)
}

fn codebuddy_extension_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CodeBuddyExtension")
        .join("Data")
}

fn is_ide_conversation_index(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("index.json")
        && path
            .parent()
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("history")
}

fn ide_index_files(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|name| name.to_str());
            // Message bodies contain chat content and must not be scanned.
            // Checkpoints and the shared Public bucket are unrelated to usage.
            if !matches!(
                name,
                Some("messages" | "check-point" | "backups" | "Public")
            ) {
                ide_index_files(&path, output);
            }
        } else if is_ide_conversation_index(&path) {
            output.push(path);
        }
    }
}

fn decode_genie_workspace(name: &str) -> Option<String> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let try_decode = |value: &str| -> Option<String> {
        let padded = match value.len() % 4 {
            0 => value.to_string(),
            remainder => format!("{value}{}", "=".repeat(4 - remainder)),
        };
        let bytes = engine.decode(padded).ok()?;
        let text = String::from_utf8(bytes).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.contains('\0') {
            return None;
        }
        Some(trimmed.to_string())
    };
    try_decode(name)
        .or_else(|| try_decode(&name.replace('_', "/")))
        .or_else(|| try_decode(&name.replace('_', "+")))
}

fn ide_project_by_session() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(root) = crate::modules::vscode_cn_inject::codebuddy_cn_data_dir().map(|dir| {
        dir.join("User")
            .join("globalStorage")
            .join("tencent-cloud.coding-copilot")
            .join("genie-history")
    }) else {
        return map;
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let folder = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let project = decode_genie_workspace(folder)
            .as_deref()
            .map(Path::new)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::trim)
            .filter(|name| !name.is_empty() && name.len() <= 120)
            .unwrap_or("未知项目")
            .to_string();
        let Ok(conversations) = std::fs::read_dir(path.join("conversations")) else {
            continue;
        };
        for conversation in conversations.flatten() {
            if let Some(id) = conversation.file_name().to_str() {
                if !id.is_empty() {
                    map.insert(id.to_string(), project.clone());
                }
            }
        }
    }
    map
}

/// 会话文件所在 workspace 的 `index.json` 路径。
///
/// 布局：`<workspace>/<会话目录>/<文件>` ⇒ `<workspace>/index.json`。
fn workspace_index_path(conv_index: &Path) -> Option<PathBuf> {
    Some(conv_index.parent()?.parent()?.join("index.json"))
}

/// 解析一份 workspace `index.json`，产出 `会话 id → (标题, 模型)` 查表。
///
/// 解析失败 / 无 `conversations` ⇒ 返回**空表**；调用方据此回落
/// `(None, "未知模型")`，与逐次调用时代的语义一致。
///
/// **同名 id 取首次出现**：改造前是「线性扫描、返回第一个匹配」，
/// 因此这里必须用 `entry().or_insert()` 而不是 `insert()` —— 后者会变成「最后一个赢」。
fn workspace_meta_index(ws_index: &Path) -> HashMap<String, (Option<String>, String)> {
    let mut table = HashMap::new();
    let Ok(text) = std::fs::read_to_string(ws_index) else {
        return table;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return table;
    };
    let Some(conversations) = value.get("conversations").and_then(Value::as_array) else {
        return table;
    };
    for conversation in conversations {
        // 无 `id` 或 `id` 为空串的条目**永远匹配不上**（调用方传的是会话**目录名**，
        // 空目录名会先被 `.filter(|name| !name.is_empty())` 换成 "未知会话"）⇒ 直接跳过。
        let Some(id) = conversation.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let title = non_empty_text(conversation.get("name"))
            .or_else(|| non_empty_text(conversation.get("title")));
        let model = non_empty_text(conversation.get("selectedModelId"))
            .or_else(|| non_empty_text(conversation.get("modelId")))
            .or_else(|| non_empty_text(conversation.get("model")))
            .unwrap_or_else(|| "未知模型".to_string());
        table.entry(id.to_string()).or_insert((title, model));
    }
    table
}

/// 按 workspace `index.json` 路径缓存查表，**同一份索引只读解析一次**。
///
/// ## 为什么需要（`docs/history/perf-audit-2026-09-24.md` 的 P0-4「无争议的那一半」）
///
/// 改造前 `ide_workspace_meta` 被调在**每个会话文件**的循环体内
/// （见 [`ide_source`]），而同一 workspace 下的 N 个会话指向**同一份** `index.json`
/// ⇒ 同一份索引被 `read_to_string` + `serde_json::from_str` **N 次**。
/// 这是**纯冗余**：与「是否缓存聚合结果」无关（后者才需要向作者确认）。
///
/// 本改动**不改变任何输出语义** —— 只是把「N 次解析」变成「1 次解析 + N 次查表」，
/// 故风险与收益不成比例的「全量结果缓存」不同，可以直接做。
#[derive(Default)]
struct WorkspaceMetaCache {
    tables: HashMap<PathBuf, HashMap<String, (Option<String>, String)>>,
    /// 只用于测试断言「同一份索引确实只解析一次」。
    ///
    /// 这是一个**可观测缝**：没有它就只能靠计时或读盘计数去间接推断，
    /// 两者都不可靠。`#[cfg(test)]` 保证发布构建零成本。
    #[cfg(test)]
    parses: usize,
}

impl WorkspaceMetaCache {
    /// 查某会话的 `(标题, 模型)`；查不到回落 `(None, "未知模型")`。
    fn lookup(&mut self, conv_index: &Path, conv_id: &str) -> (Option<String>, String) {
        let Some(ws_index) = workspace_index_path(conv_index) else {
            return (None, "未知模型".to_string());
        };
        let table = self.tables.entry(ws_index.clone()).or_insert_with(|| {
            #[cfg(test)]
            {
                self.parses += 1;
            }
            workspace_meta_index(&ws_index)
        });
        table
            .get(conv_id)
            .cloned()
            .unwrap_or_else(|| (None, "未知模型".to_string()))
    }
}

fn ide_request_usage(request: &Value) -> Option<Usage> {
    let object = request.get("usage")?.as_object()?;
    field(object, &["inputTokens", "input_tokens", "prompt_tokens"])?;
    Some(Usage {
        input: field(object, &["inputTokens", "input_tokens", "prompt_tokens"]).unwrap_or(0),
        credit: credit_field(object),
        output: field(
            object,
            &["outputTokens", "output_tokens", "completion_tokens"],
        )
        .unwrap_or(0),
        read: field(
            object,
            &[
                "cacheTokens",
                "cacheReadInputTokens",
                "cache_read_input_tokens",
            ],
        )
        .unwrap_or(0),
        write: positive_field(
            object,
            &[
                "cachedWriteTokens",
                "cacheWriteInputTokens",
                "cache_write_input_tokens",
                "cache_creation_input_tokens",
            ],
        )
        .unwrap_or(0),
    })
}

fn ide_request_timestamp(request: &Value) -> Option<i64> {
    timestamp(request).or_else(|| {
        request.get("startedAt").and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
                .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
        })
    })
}

fn ide_source(
    root: PathBuf,
    name: &str,
    cutoff: Option<i64>,
    project_by_session: &HashMap<String, String>,
) -> Value {
    let mut paths = Vec::new();
    ide_index_files(&root, &mut paths);
    paths.sort();
    let mut collector = SourceCollector::default();
    // 同一 workspace 下的多个会话共用一份 `index.json`：提到循环外按路径解析一次。
    let mut workspace_meta = WorkspaceMetaCache::default();

    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            collector.note_parse_error();
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            collector.note_parse_error();
            continue;
        };
        let Some(requests) = value.get("requests").and_then(Value::as_array) else {
            continue;
        };
        let session_id = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("未知会话")
            .to_string();
        let (title, model_name) = workspace_meta.lookup(path, &session_id);
        let fallback_project = project_by_session
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| "未知项目".to_string());
        let mut session_totals = Totals::default();
        let mut session_daily: HashMap<String, Totals> = HashMap::new();
        let mut session_project: Option<String> = None;

        for request in requests {
            let ts = ide_request_timestamp(request);
            if cutoff.is_some_and(|minimum| ts.is_none_or(|value| value < minimum)) {
                continue;
            }
            let Some(usage) = ide_request_usage(request) else {
                continue;
            };
            let project = fallback_project.clone();
            if session_project.is_none() {
                session_project = Some(project.clone());
            }
            session_totals.add(usage);
            let meta = json!({
                "timestamp": ts,
                "providerData": { "model": model_name },
            });
            if let Some(day) = date(&meta) {
                session_daily.entry(day).or_default().add(usage);
            }
            collector.add(usage, &meta, &project);
        }

        collector.push_session(
            session_id,
            title,
            session_project.unwrap_or(fallback_project),
            session_totals,
            session_daily,
        );
    }

    collector.into_value(name, paths.len())
}

/// 数值型 summary 字段：合并时**求和**。
///
/// `cacheHitRate` 刻意不在此列表——它是比例字段，必须按分子分母重算（见
/// [`merge_summary`]），直接相加会得到无意义的值（如 `0.5 + 0.75 = 1.25`）。
const SUMMED_SUMMARY_FIELDS: &[&str] = &[
    "total",
    "input",
    "output",
    "cacheRead",
    "cacheWrite",
    "uncachedInput",
    "records",
];

fn value_as_u64(value: Option<&Value>) -> u64 {
    value.and_then(number).unwrap_or(0)
}

/// 把同名分组项（`models`/`projects`/`daily`/`hours`/`sessions`）的数值字段求和。
///
/// `key_field` 为该字段的去重键名（`"key"` 或 `"sessionId"`）。非数值字段（`key` /
/// `title` / `project` / `sessionId`）保留首个非 null 值。
fn merge_group_item(base: &mut Value, other: &Value) {
    let Some(other_obj) = other.as_object() else {
        return;
    };
    let Some(base_obj) = base.as_object_mut() else {
        return;
    };
    for (field, other_value) in other_obj {
        if let Some(other_number) = number(other_value) {
            if base_obj.contains_key(field.as_str()) {
                let current = value_as_u64(base_obj.get(field.as_str()));
                base_obj.insert(field.clone(), json!(current.saturating_add(other_number)));
            } else {
                base_obj.insert(field.clone(), other_value.clone());
            }
        } else if field == "daily" && other_value.is_array() {
            // ★ 会话的逐日拆分（数组）**不是数值字段**，不能走下面的「保留首个」——
            //   两版各自的每日量必须按日**求和合并**，否则合并作用域下会丢一整版的逐日。
            let merged = match base_obj.get("daily").and_then(Value::as_array) {
                Some(base_daily) => merge_grouped_list_from_arrays(
                    "key",
                    &[Value::Array(base_daily.clone()), other_value.clone()],
                ),
                None => other_value.as_array().cloned().unwrap_or_default(),
            };
            base_obj.insert("daily".into(), Value::Array(merged));
        } else if !base_obj.contains_key(field.as_str())
            || base_obj.get(field.as_str()).is_some_and(Value::is_null)
        {
            base_obj.insert(field.clone(), other_value.clone());
        }
    }
    // summary 型分组项也可能带 cacheHitRate；重算以避免相加错误。
    recompute_cache_hit_rate(base_obj);
}

/// 若对象含有 `cacheRead` / `uncachedInput`，按 `cacheRead / (cacheRead + uncachedInput)`
/// 重算 `cacheHitRate`（分母为 0 → `null`）。
fn recompute_cache_hit_rate(object: &mut Map<String, Value>) {
    if !object.contains_key("cacheRead") && !object.contains_key("uncachedInput") {
        return;
    }
    let cache_read = value_as_u64(object.get("cacheRead"));
    let uncached = value_as_u64(object.get("uncachedInput"));
    let denominator = cache_read.saturating_add(uncached);
    let rate = (denominator > 0).then(|| cache_read as f64 / denominator as f64);
    object.insert("cacheHitRate".into(), json!(rate));
}

/// 合并 `summary`：数值字段求和；`cacheHitRate` 按合并后的 `cacheRead` / `uncachedInput`
/// **重算**（分母为 0 → `null`），绝不直接相加。
fn merge_summary(values: &[Value]) -> Value {
    let mut totals: Map<String, Value> = Map::new();
    for field in SUMMED_SUMMARY_FIELDS {
        let sum: u64 = values
            .iter()
            .map(|value| value_as_u64(value.get("summary").and_then(|s| s.get(*field))))
            .fold(0u64, u64::saturating_add);
        totals.insert((*field).to_string(), json!(sum));
    }
    // cacheHitRate 的分子/分母来自合并后的 cacheRead / uncachedInput。
    let cache_read = value_as_u64(totals.get("cacheRead"));
    let uncached = value_as_u64(totals.get("uncachedInput"));
    let denominator = cache_read.saturating_add(uncached);
    let rate = (denominator > 0).then(|| cache_read as f64 / denominator as f64);
    totals.insert("cacheHitRate".into(), json!(rate));
    Value::Object(totals)
}

/// 按去重键分组合并列表字段（`models` / `projects` / `daily` / `hours` / `sessions`）。
///
/// 同名 key 必须**合并为一条**（值为各版之和），不得出现两条同名条目；结果按 `total`
/// 降序排列，与 [`groups`] 的单版排序一致，保证前端渲染顺序稳定。
fn merge_grouped_list(field: &str, key_field: &str, values: &[Value]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Value> = HashMap::new();
    for value in values {
        let Some(items) = value.get(field).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let key = item
                .get(key_field)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match grouped.get_mut(&key) {
                Some(existing) => merge_group_item(existing, item),
                None => {
                    order.push(key.clone());
                    grouped.insert(key, item.clone());
                }
            }
        }
    }
    let mut values: Vec<Value> = order
        .into_iter()
        .filter_map(|key| grouped.remove(&key))
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(total_value(v)));
    values
}

/// 合并 `dailyByModel`：按 `model → date` 两维分组、同名 key 求和合并为一条。
fn merge_daily_by_model_field(values: &[Value], field: &str) -> Value {
    let mut models: Map<String, Value> = Map::new();
    let mut seen_models: Vec<String> = Vec::new();
    let mut collected: HashMap<String, Vec<Value>> = HashMap::new();
    for value in values {
        let Some(map) = value.get(field).and_then(Value::as_object) else {
            continue;
        };
        for (model_name, points) in map {
            if !collected.contains_key(model_name) {
                seen_models.push(model_name.clone());
            }
            collected
                .entry(model_name.clone())
                .or_default()
                .push(points.clone());
        }
    }
    for model_name in seen_models {
        let points = collected.remove(&model_name).unwrap_or_default();
        let merged = merge_grouped_list_from_arrays("key", &points);
        models.insert(model_name, Value::Array(merged));
    }
    Value::Object(models)
}

/// 合并若干「已是数组」的分组序列（供 `dailyByModel` 的内部日期数组复用）。
fn merge_grouped_list_from_arrays(key_field: &str, arrays: &[Value]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Value> = HashMap::new();
    for array in arrays {
        let Some(items) = array.as_array() else {
            continue;
        };
        for item in items {
            let key = item
                .get(key_field)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match grouped.get_mut(&key) {
                Some(existing) => merge_group_item(existing, item),
                None => {
                    order.push(key.clone());
                    grouped.insert(key, item.clone());
                }
            }
        }
    }
    let mut result: Vec<Value> = order
        .into_iter()
        .filter_map(|key| grouped.remove(&key))
        .collect();
    result.sort_by_key(|v| std::cmp::Reverse(total_value(v)));
    result
}

/// 取 `coverageStartAt` 的最小值（缺失视为 `null`，若两版皆缺失则为 `null`）。
fn coverage_min(values: &[Value]) -> Value {
    let min = values
        .iter()
        .filter_map(|value| value.get("coverageStartAt").and_then(Value::as_i64))
        .min();
    json!(min)
}

/// 取 `coverageEndAt` 的最大值（缺失视为 `null`，若两版皆缺失则为 `null`）。
fn coverage_max(values: &[Value]) -> Value {
    let max = values
        .iter()
        .filter_map(|value| value.get("coverageEndAt").and_then(Value::as_i64))
        .max();
    json!(max)
}

/// 把同一 `source` 名下的多版（cn / global）聚合值合并为一个。
///
/// 合并语义：
/// - 数值字段（`summary.*`、`filesScanned`、`parseErrors`）**求和**；
/// - `cacheHitRate` **按合并后的 `cacheRead / (cacheRead + uncachedInput)` 重算**
///   （分母为 0 → `null`），**绝不直接相加**；
/// - `models` / `projects` / `sessions` / `daily` / `hours` 按各自去重键分组求和，
///   同名 key **合并为一条**（不得出现两条同名）；
/// - `dailyByModel` 按 `model → date` 两维分组求和；
/// - `coverageStartAt` 取 min、`coverageEndAt` 取 max（区间并集）。
///
/// 纯函数，可直接单测与变异注入验证。
pub fn merge_source_values(values: &[Value]) -> Value {
    let source_name = values
        .iter()
        .find_map(|value| value.get("source").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();

    let summary = merge_summary(values);
    let models = merge_grouped_list("models", "key", values);
    let projects = merge_grouped_list("projects", "key", values);
    let sessions = merge_grouped_list("sessions", "sessionId", values);
    let daily = merge_grouped_list("daily", "key", values);
    let hours = merge_grouped_list("hours", "key", values);
    let daily_by_model = merge_daily_by_model_field(values, "dailyByModel");
    let daily_hours = merge_grouped_list("dailyHours", "key", values);
    let daily_hours_by_model = merge_daily_by_model_field(values, "dailyHoursByModel");

    let files_scanned: u64 = values
        .iter()
        .map(|value| value_as_u64(value.get("filesScanned")))
        .fold(0u64, u64::saturating_add);
    let parse_errors: u64 = values
        .iter()
        .map(|value| value_as_u64(value.get("parseErrors")))
        .fold(0u64, u64::saturating_add);

    json!({
        "source": source_name,
        "summary": summary,
        "models": models,
        "projects": projects,
        "sessions": sessions,
        "daily": daily,
        "dailyByModel": daily_by_model,
        "dailyHours": daily_hours,
        "dailyHoursByModel": daily_hours_by_model,
        "hours": hours,
        "filesScanned": files_scanned,
        "parseErrors": parse_errors,
        "coverageStartAt": coverage_min(values),
        "coverageEndAt": coverage_max(values),
    })
}

/// Return independent WorkBuddy, CodeBuddy CLI, and CodeBuddy IDE aggregates (CN).
/// `days` is interpreted in Rust using the same millisecond clock for every source.
pub fn get_statistics(days: Option<i64>) -> Value {
    get_statistics_for(crate::modules::region::Region::Cn, days)
}

/// Return the per-region token aggregates（旧签名薄包装，行为逐字节不变）。
///
/// CN keeps the three isolated sources (`.workbuddy/projects`, `.codebuddy/projects`,
/// CodeBuddy IDE). Global reads only its own client directory
/// (`.workbuddy-ai/projects`), so the two versions never mix (对照设计「统计按 region
/// 分别汇总」)。`days` is interpreted with the same millisecond clock for every source.
pub fn get_statistics_for(region: Region, days: Option<i64>) -> Value {
    get_statistics_for_filter(RegionFilter::from(region), days)
}

/// 采集某个具体 region 的原始 `sources[]`（不含顶层包装）。纯采集，供合并路径复用。
fn collect_sources_for(
    region: Region,
    cutoff: Option<i64>,
    home: &Path,
    ide_projects: &HashMap<String, String>,
) -> Vec<Value> {
    match region {
        Region::Cn => vec![
            source(home.join(".workbuddy/projects"), "workbuddy", cutoff),
            source(home.join(".codebuddy/projects"), "codebuddy-cli", cutoff),
            ide_source(
                codebuddy_extension_data_dir(),
                "codebuddy-ide",
                cutoff,
                ide_projects,
            ),
        ],
        Region::Global => vec![source(
            home.join(".workbuddy-ai/projects"),
            "workbuddy-ai",
            cutoff,
        )],
    }
}

/// 按 region 过滤范围返回 Token 聚合。
///
/// - `Cn` -> 三源（`workbuddy` / `codebuddy-cli` / `codebuddy-ide`）；
/// - `Global` -> 单源（`workbuddy-ai`）；
/// - `All` -> cn 三源 + global 单源，按 `source` 分组后逐组调用 [`merge_source_values`]，
///   顶层 `region` 为 `"all"`。因两版源名天然不同，合并视图实际是**四源并集**
///   （PRD §6.1：不跨源合并 raw）；`merge_source_values` 保证未来出现同名源时仍正确聚合。
///
/// `region=all` **实时聚合，不缓存、不落库**。
pub fn get_statistics_for_filter(filter: RegionFilter, days: Option<i64>) -> Value {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let generated_at = crate::modules::config::now_ms();
    let range_days = match days {
        Some(7) => Some(7),
        Some(30) => Some(30),
        Some(90) => Some(90),
        _ => None,
    };
    let cutoff = range_days.map(|value| generated_at - value * 86_400_000);

    // 单版路径保持既有行为：直接返回该 region 的三源 / 单源。
    if let Some(region) = filter.single() {
        let ide_projects = ide_project_by_session();
        let sources = collect_sources_for(region, cutoff, &home, &ide_projects);
        // 有新解析才落盘；热路径（全部命中）一个字节都不写。
        persist_scan_cache_if_dirty();
        return json!({
            "region": region.as_str(),
            "generatedAt": generated_at,
            "rangeDays": range_days,
            "sources": sources,
        });
    }

    // 合并路径：采集两版全部源，按 source 名分组后逐组合并。
    let ide_projects = ide_project_by_session();
    let mut collected: Vec<Value> = Vec::new();
    for region in filter.regions() {
        collected.extend(collect_sources_for(region, cutoff, &home, &ide_projects));
    }

    // 以 source 名为分组键，保持首次出现顺序（cn 三源在前，global 单源在后）。
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    for value in collected {
        let name = value
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        if !grouped.contains_key(&name) {
            order.push(name.clone());
        }
        grouped.entry(name).or_default().push(value);
    }
    let sources: Vec<Value> = order
        .into_iter()
        .filter_map(|name| grouped.remove(&name))
        .map(|group| merge_source_values(&group))
        .collect();

    persist_scan_cache_if_dirty();
    json!({
        "region": filter.as_str(),
        "generatedAt": generated_at,
        "rangeDays": range_days,
        "sources": sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// ★ 积分（issue #63）：jsonl 的 `message.usage.credit` 必须被解析出来。
    ///
    /// 缺陷现场：改造前 `usage_fields` 只认 input/output/cache 四个字段，
    /// `credit` 被整条忽略 ⇒ 「积分」视图只能按 token 折算（估算，不是账单）。
    ///
    /// 可证伪：删掉 `credit: credit_field(object)` ⇒ 本用例在 `credit` 上变红。
    #[test]
    fn usage_credit_is_parsed_from_message_usage() {
        let value = json!({
            "message": { "usage": { "input_tokens": 44078, "output_tokens": 198, "credit": 1.28 } }
        });
        let usage = usage(&value).expect("有 usage");
        assert!(
            (usage.credit - 1.28).abs() < 1e-9,
            "credit 必须原样解析，实际 {}",
            usage.credit
        );
    }

    /// 没有 credit 字段 ⇒ 0.0（= 不知道），不是负数、不是 NaN。
    #[test]
    fn usage_credit_defaults_to_zero_when_absent() {
        let value = json!({ "message": { "usage": { "input_tokens": 1, "output_tokens": 1 } } });
        assert_eq!(usage(&value).expect("有 usage").credit, 0.0);
    }

    /// 缓存往返：`credit` 走「微积分」整数格，1e-6 精度内必须原样回来。
    ///
    /// 为什么要单独测：`PersistedTotals` 是定长 u64 数组（要能 `Eq`），
    /// 换成 f64 会同时打断缓存比对与 Eq 推导。
    #[test]
    fn credit_survives_the_scan_cache_roundtrip() {
        let mut totals = Totals::default();
        totals.add(Usage {
            input: 10,
            output: 2,
            read: 0,
            write: 0,
            credit: 1.28,
        });
        totals.add(Usage {
            input: 10,
            output: 2,
            read: 0,
            write: 0,
            credit: 0.06,
        });
        let back = totals_from_persisted(totals_to_persisted(&totals));
        assert!(
            (back.usage.credit - 1.34).abs() < 1e-6,
            "实际 {}",
            back.usage.credit
        );
        assert_eq!(back.usage.input, 20);
    }

    #[test]
    fn usage_priority_aliases_and_raw_cache_write() {
        let value = json!({
            "providerData": {
                "usage": { "inputTokens": 99, "outputTokens": 22 },
                "rawUsage": { "prompt_cache_write_tokens": 2 }
            },
            "message": { "usage": {
                "input_tokens": 10,
                "output_tokens": 3,
                "cache_read_input_tokens": 4
            }}
        });
        assert_eq!(
            usage(&value),
            Some(Usage {
                input: 10,
                output: 3,
                read: 4,
                write: 2,
                credit: 0.0,
            })
        );
    }

    #[test]
    fn cache_write_uses_explicit_aliases_but_never_cache_miss() {
        let provider_usage_write = json!({
            "providerData": {
                "usage": {
                    "inputTokens": 99,
                    "outputTokens": 22,
                    "cache_write_input_tokens": 0,
                    "cache_creation_input_tokens": 7
                },
                "rawUsage": {
                    "prompt_cache_miss_tokens": 91,
                    "prompt_cache_write_tokens": 0
                }
            },
            "message": { "usage": {
                "input_tokens": 10,
                "output_tokens": 3,
                "cache_read_input_tokens": 4
            }}
        });
        assert_eq!(
            usage(&provider_usage_write),
            Some(Usage {
                input: 10,
                output: 3,
                read: 4,
                write: 7,
                credit: 0.0,
            })
        );

        let cache_miss_only = json!({
            "providerData": {
                "rawUsage": { "prompt_cache_miss_tokens": 91 }
            },
            "message": { "usage": {
                "input_tokens": 10,
                "output_tokens": 3
            }}
        });
        assert_eq!(
            usage(&cache_miss_only),
            Some(Usage {
                input: 10,
                output: 3,
                read: 0,
                write: 0,
                credit: 0.0,
            })
        );
    }

    #[test]
    fn cache_read_accepts_nested_provider_details() {
        let value = json!({
            "providerData": {
                "usage": {
                    "inputTokens": 99,
                    "outputTokens": 3,
                    "inputTokensDetails": [{ "cached_tokens": 7 }]
                }
            }
        });
        assert_eq!(
            usage(&value),
            Some(Usage {
                input: 99,
                output: 3,
                read: 7,
                write: 0,
                credit: 0.0,
            })
        );

        let raw = json!({
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 2,
                "cache_read_input_tokens": 0,
                "prompt_cache_hit_tokens": 12
            }
        });
        assert_eq!(
            usage(&raw),
            Some(Usage {
                input: 20,
                output: 2,
                read: 12,
                write: 0,
                credit: 0.0,
            })
        );
    }

    #[test]
    fn source_excludes_subagents_and_counts_each_record_once() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-{}-{}",
            std::process::id(),
            crate::modules::config::now_ms()
        ));
        let project = root.join("fixture-project");
        let ignored = project.join("subagents");
        fs::create_dir_all(&ignored).expect("create fixture dirs");
        let record = json!({
            "timestamp": crate::modules::config::now_ms(),
            "providerData": {
                "model": "fixture-model",
                "usage": { "inputTokens": 20, "outputTokens": 5 }
            },
            "message": { "usage": {
                "input_tokens": 10,
                "output_tokens": 3,
                "cache_read_input_tokens": 4
            }}
        });
        fs::write(
            project.join("session.jsonl"),
            format!("{}\nnot-json\n", record),
        )
        .expect("write fixture");
        fs::write(ignored.join("agent.jsonl"), format!("{}\n", record))
            .expect("write ignored fixture");

        let result = source(root.clone(), "fixture", None);
        assert_eq!(result["filesScanned"], 1);
        assert_eq!(result["parseErrors"], 1);
        assert_eq!(result["summary"]["input"], 10);
        assert_eq!(result["summary"]["output"], 3);
        assert_eq!(result["summary"]["cacheRead"], 4);
        assert_eq!(result["summary"]["total"], 13);
        assert_eq!(result["summary"]["records"], 1);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(result["dailyByModel"]["fixture-model"][0]["key"], today);
        assert_eq!(result["projects"][0]["key"], "fixture-project");
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn bounded_source_excludes_records_before_cutoff() {
        let now = crate::modules::config::now_ms();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-range-{}-{now}",
            std::process::id()
        ));
        let project = root.join("fixture-project");
        fs::create_dir_all(&project).expect("create fixture dirs");
        let record = |timestamp| {
            json!({
                "timestamp": timestamp,
                "cwd": "/fixture/example-project",
                "message": { "usage": { "input_tokens": 10, "output_tokens": 2 } }
            })
        };
        fs::write(
            project.join("session.jsonl"),
            format!("{}\n{}\n", record(now - 10_000), record(now - 100_000)),
        )
        .expect("write fixture");

        let result = source(root.clone(), "fixture", Some(now - 50_000));
        assert_eq!(result["summary"]["records"], 1);
        assert_eq!(result["summary"]["input"], 10);
        assert_eq!(result["projects"][0]["key"], "example-project");
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn session_titles_are_file_scoped_and_independent_from_usage_cutoff() {
        let now = crate::modules::config::now_ms();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-titles-{}-{now}",
            std::process::id()
        ));
        let project = root.join("fixture-project");
        fs::create_dir_all(&project).expect("create fixture dirs");
        let usage_record = |input| {
            json!({
                "timestamp": now,
                "cwd": "/private/example-project",
                "message": { "usage": { "input_tokens": input, "output_tokens": 2 } }
            })
        };

        fs::write(
            project.join("session-a.jsonl"),
            format!(
                "{}\n{}\n{}\n{}\n",
                json!({ "type": "summary", "summary": "摘要不应覆盖 AI 标题" }),
                json!({ "type": "ai-title", "aiTitle": "旧标题" }),
                usage_record(10),
                json!({ "type": "ai-title", "aiTitle": "最新 AI 标题" }),
            ),
        )
        .expect("write ai title fixture");
        fs::write(
            project.join("session-b.jsonl"),
            format!(
                "{}\n{}\n",
                json!({
                    "type": "ai-title",
                    "timestamp": now - 100_000,
                    "aiTitle": "范围外保留标题"
                }),
                usage_record(20),
            ),
        )
        .expect("write cutoff title fixture");
        fs::write(
            project.join("session-c.jsonl"),
            format!(
                "{}\n{}\n",
                usage_record(30),
                json!({ "type": "summary", "summary": "摘要回退标题" }),
            ),
        )
        .expect("write summary fixture");
        fs::write(
            project.join("session-d.jsonl"),
            format!("{}\n", usage_record(40)),
        )
        .expect("write untitled fixture");
        fs::write(
            project.join("session-e.jsonl"),
            format!(
                "{}\n{}\n",
                json!({ "type": "ai-title", "aiTitle": "最新 AI 标题" }),
                usage_record(50),
            ),
        )
        .expect("write duplicate title fixture");

        let result = source(root.clone(), "fixture", Some(now - 50_000));
        let sessions = result["sessions"].as_array().expect("session groups");
        let by_id = |session_id: &str| {
            sessions
                .iter()
                .find(|session| session["sessionId"] == session_id)
                .expect("session group by id")
        };

        assert_eq!(result["summary"]["input"], 150);
        assert_eq!(result["summary"]["records"], 5);
        assert_eq!(sessions.len(), 5);
        assert_eq!(by_id("session-a")["title"], "最新 AI 标题");
        assert_eq!(by_id("session-b")["title"], "范围外保留标题");
        assert_eq!(by_id("session-c")["title"], "摘要回退标题");
        assert!(by_id("session-d")["title"].is_null());
        assert_eq!(by_id("session-a")["project"], "example-project");
        assert_ne!(by_id("session-a")["key"], by_id("session-e")["key"]);

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /// ★★ 会话**逐日拆分**（2026-10-15 新增）：前端「会话」视图按时间窗口切片的素材。
    ///
    /// 用户报障「会话的统计数据切换时间没有变化」——根因是会话只有全时合计、没有时间
    /// 维度，前端拿到也无从按窗计算。本用例钉住三件事：
    /// ① 每个会话带 `daily`，键 = 该会话记录所在的本地日；
    /// ② **逐日之和 == 会话总量**（窗口切片的正确性前提：近 30 天窗口 = 全部时，
    ///    数字必须回到全时口径，不许比它多或少了某天的量）；
    /// ③ 无时间戳的记录不落任何日键（与文件级 `daily` 同判据），但其总量仍在会话里
    ///    —— 「进不了任何窗口」与「不存在」是两件事，由总量 vs 逐日之差体现。
    ///
    /// 可证伪：把 `parse_file` 里 `session_daily` 的累加删掉 ⇒ 断言①变红
    /// （`daily` 为空数组）。
    #[test]
    fn session_daily_splits_usage_by_local_day_for_window_slicing() {
        let now = crate::modules::config::now_ms();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-session-daily-{}-{now}",
            std::process::id()
        ));
        let project = root.join("fixture-project");
        fs::create_dir_all(&project).expect("create fixture dirs");

        let record = |timestamp: i64, input: u64, output: u64| {
            json!({
                "timestamp": timestamp,
                "cwd": "/fixture/example-project",
                "message": { "usage": { "input_tokens": input, "output_tokens": output } }
            })
        };
        fs::write(
            project.join("session-a.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                record(now - 86_400_000, 10, 1), // 昨天
                record(now, 30, 3),              // 今天
                // 无时间戳：进不了任何窗口，但必须留在会话总量里。
                json!({
                    "cwd": "/fixture/example-project",
                    "message": { "usage": { "input_tokens": 5, "output_tokens": 0 } }
                }),
            ),
        )
        .expect("write fixture");

        let result = source(root.clone(), "fixture", None);
        let session = &result["sessions"][0];
        assert_eq!(session["total"], 49, "会话总量 = 10+1+30+3+5");
        let daily = session["daily"].as_array().expect("daily 必须下发");
        assert_eq!(daily.len(), 2, "两个有日期的记录 ⇒ 两个日键：{daily:?}");
        let by_key: HashMap<&str, &Value> = daily
            .iter()
            .map(|point| (point["key"].as_str().unwrap_or_default(), point))
            .collect();
        assert_eq!(by_key[&today.as_str()]["total"], 33, "今天 = 30+3");
        assert_eq!(by_key[&yesterday.as_str()]["total"], 11, "昨天 = 10+1");
        let daily_sum: u64 = daily
            .iter()
            .map(|point| point["total"].as_u64().unwrap_or(0))
            .sum();
        assert_eq!(
            daily_sum, 44,
            "逐日之和 = 有日期部分（44）—— 与总量的差（5）正是无时间戳的那条"
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /// P0-4「无争议的那一半」：同一 workspace 下的 N 个会话**只解析一次** `index.json`。
    ///
    /// 断言三件事：
    /// ① 每个会话拿到**自己**的标题/模型（证明查表按 conv id 分键，不是「第一条赢」）；
    /// ② 解析次数恰好 **1**（这是本改动的全部意义，且是**直接**断言、不靠计时推断）；
    /// ③ 改造前是 N 次 —— 该断言在改造前必红。
    #[test]
    fn workspace_meta_cache_parses_each_index_once() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-wsmeta-{}-{}",
            std::process::id(),
            crate::modules::config::now_ms()
        ));
        let workspace = root.join("workspace-hash");
        for id in ["conv-a", "conv-b", "conv-c"] {
            fs::create_dir_all(workspace.join(id)).expect("create conv dir");
        }
        fs::write(
            workspace.join("index.json"),
            json!({
                "conversations": [
                    { "id": "conv-a", "name": "标题 A", "selectedModelId": "model-a" },
                    { "id": "conv-b", "title": "标题 B", "modelId": "model-b" },
                    { "id": "conv-c", "model": "model-c" }
                ]
            })
            .to_string(),
        )
        .expect("write workspace index");

        let mut cache = WorkspaceMetaCache::default();
        let a = cache.lookup(&workspace.join("conv-a").join("index.json"), "conv-a");
        let b = cache.lookup(&workspace.join("conv-b").join("index.json"), "conv-b");
        let c = cache.lookup(&workspace.join("conv-c").join("index.json"), "conv-c");

        assert_eq!(a, (Some("标题 A".to_string()), "model-a".to_string()));
        // `title` 是 `name` 缺失时的第二优先
        assert_eq!(b, (Some("标题 B".to_string()), "model-b".to_string()));
        // `model` 是第三优先；三个标题字段都缺 ⇒ 标题为 None，模型仍取自 model
        assert_eq!(c, (None, "model-c".to_string()));

        assert_eq!(
            cache.parses, 1,
            "同一份 index.json 三个会话只应解析一次（改造前是 3 次）"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// 查不到会话时的回落必须与逐次调用时代**逐字相同**：`(None, "未知模型")`。
    #[test]
    fn workspace_meta_cache_falls_back_exactly_like_the_linear_scan() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-wsmeta-miss-{}-{}",
            std::process::id(),
            crate::modules::config::now_ms()
        ));
        let workspace = root.join("ws");
        fs::create_dir_all(workspace.join("conv-x")).expect("create conv dir");

        // ① index.json 不存在
        let mut cache = WorkspaceMetaCache::default();
        assert_eq!(
            cache.lookup(&workspace.join("conv-x").join("index.json"), "conv-x"),
            (None, "未知模型".to_string()),
            "索引缺失时必须回落"
        );

        // ② 索引存在但没有该会话
        fs::write(
            workspace.join("index.json"),
            json!({ "conversations": [{ "id": "other", "name": "别的会话" }] }).to_string(),
        )
        .expect("write index");
        let mut cache = WorkspaceMetaCache::default();
        assert_eq!(
            cache.lookup(&workspace.join("conv-x").join("index.json"), "conv-x"),
            (None, "未知模型".to_string()),
            "会话不在索引里时必须回落"
        );

        // ③ 索引是坏 JSON（解析失败也要回落，不得 panic）
        fs::write(workspace.join("index.json"), "{ not json").expect("write bad index");
        let mut cache = WorkspaceMetaCache::default();
        assert_eq!(
            cache.lookup(&workspace.join("conv-x").join("index.json"), "conv-x"),
            (None, "未知模型".to_string()),
            "索引损坏时必须回落"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// 同名 id **取首次出现** —— 改造前是「线性扫描返回第一个匹配」。
    ///
    /// 若把 `entry().or_insert()` 写成 `insert()`，本用例立刻变红（会拿到「后一条」）。
    #[test]
    fn workspace_meta_index_keeps_the_first_duplicate_id() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-wsmeta-dup-{}-{}",
            std::process::id(),
            crate::modules::config::now_ms()
        ));
        let workspace = root.join("ws");
        fs::create_dir_all(workspace.join("conv-dup")).expect("create conv dir");
        fs::write(
            workspace.join("index.json"),
            json!({
                "conversations": [
                    { "id": "conv-dup", "name": "第一条", "selectedModelId": "first-model" },
                    { "id": "conv-dup", "name": "第二条", "selectedModelId": "second-model" }
                ]
            })
            .to_string(),
        )
        .expect("write index");

        let mut cache = WorkspaceMetaCache::default();
        assert_eq!(
            cache.lookup(&workspace.join("conv-dup").join("index.json"), "conv-dup"),
            (Some("第一条".to_string()), "first-model".to_string()),
            "同名 id 必须取首次出现"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ide_source_reads_request_usage_and_skips_message_bodies() {
        let now = crate::modules::config::now_ms();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-ide-{}-{now}",
            std::process::id()
        ));
        let history = root
            .join("uid")
            .join("CodeBuddyIDE")
            .join("uid")
            .join("history")
            .join("workspace-hash");
        let conv_a = history.join("conv-a");
        let conv_b = history.join("conv-b");
        let messages = conv_a.join("messages");
        fs::create_dir_all(&messages).expect("create ide fixture dirs");
        fs::create_dir_all(&conv_b).expect("create second conversation");

        fs::write(
            history.join("index.json"),
            json!({
                "conversations": [
                    {
                        "id": "conv-a",
                        "name": "IDE 会话标题",
                        "selectedModelId": "deepseek-v4-flash"
                    },
                    {
                        "id": "conv-b",
                        "name": "范围外会话",
                        "selectedModelId": "hy4-preview"
                    }
                ]
            })
            .to_string(),
        )
        .expect("write workspace index");
        fs::write(
            conv_a.join("index.json"),
            json!({
                "messages": [{ "id": "m1", "role": "assistant", "isComplete": true }],
                "requests": [{
                    "id": "req-1",
                    "state": "complete",
                    "startedAt": now,
                    "usage": {
                        "inputTokens": 100,
                        "outputTokens": 20,
                        "cacheTokens": 40,
                        "cachedWriteTokens": 5
                    }
                }]
            })
            .to_string(),
        )
        .expect("write conversation index");
        fs::write(
            conv_b.join("index.json"),
            json!({
                "requests": [{
                    "id": "req-old",
                    "state": "complete",
                    "startedAt": now - 100_000,
                    "usage": {
                        "inputTokens": 999,
                        "outputTokens": 9,
                        "cacheTokens": 1,
                        "cachedWriteTokens": 0
                    }
                }]
            })
            .to_string(),
        )
        .expect("write out-of-range conversation");
        fs::write(
            messages.join("ignored.json"),
            json!({
                "role": "assistant",
                "usage": { "inputTokens": 10_000, "outputTokens": 10_000 }
            })
            .to_string(),
        )
        .expect("write ignored message body");

        let mut projects = HashMap::new();
        projects.insert("conv-a".to_string(), "example-project".to_string());
        let result = ide_source(root.clone(), "codebuddy-ide", Some(now - 50_000), &projects);

        assert_eq!(result["source"], "codebuddy-ide");
        assert_eq!(result["filesScanned"], 2);
        assert_eq!(result["summary"]["records"], 1);
        assert_eq!(result["summary"]["input"], 100);
        assert_eq!(result["summary"]["output"], 20);
        assert_eq!(result["summary"]["cacheRead"], 40);
        assert_eq!(result["summary"]["cacheWrite"], 5);
        assert_eq!(result["summary"]["total"], 125);
        assert_eq!(result["summary"]["uncachedInput"], 60);
        assert_eq!(result["models"][0]["key"], "deepseek-v4-flash");
        assert_eq!(result["projects"][0]["key"], "example-project");
        assert_eq!(result["sessions"][0]["sessionId"], "conv-a");
        assert_eq!(result["sessions"][0]["title"], "IDE 会话标题");
        assert_eq!(result["sessions"].as_array().map(Vec::len), Some(1));
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(result["dailyByModel"]["deepseek-v4-flash"][0]["key"], today);

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn decode_genie_workspace_recovers_unix_project_path() {
        assert_eq!(
            decode_genie_workspace("L1VzZXJzL2FwcGxlL0RvY3VtZW50cy9Qcm9qZWN0L215LWFnZW50")
                .as_deref(),
            Some("/Users/apple/Documents/Project/my-agent")
        );
    }

    #[test]
    fn get_statistics_returns_three_isolated_sources() {
        let value = get_statistics(None);
        let sources = value["sources"].as_array().expect("sources");
        let names: Vec<_> = sources
            .iter()
            .map(|source| source["source"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(names, ["workbuddy", "codebuddy-cli", "codebuddy-ide"]);
        assert_eq!(value["region"], "cn");
    }

    #[test]
    fn get_statistics_for_global_reads_only_its_own_directory() {
        let value = get_statistics_for(crate::modules::region::Region::Global, None);
        let sources = value["sources"].as_array().expect("sources");
        let names: Vec<_> = sources
            .iter()
            .map(|source| source["source"].as_str().unwrap_or_default())
            .collect();
        // 国际版只读 `.workbuddy-ai/projects`，不混入 CN 的 CLI/IDE 目录。
        assert_eq!(names, ["workbuddy-ai"]);
        assert_eq!(value["region"], "global");
    }

    #[test]
    fn get_statistics_for_filter_all_returns_four_union_sources() {
        // 合并视图返回 cn 三源 + global 单源的并集，顶层 region = "all"。
        let value = get_statistics_for_filter(crate::modules::region::RegionFilter::All, None);
        let sources = value["sources"].as_array().expect("sources");
        let names: Vec<_> = sources
            .iter()
            .map(|source| source["source"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            names,
            [
                "workbuddy",
                "codebuddy-cli",
                "codebuddy-ide",
                "workbuddy-ai"
            ]
        );
        assert_eq!(value["region"], "all");
    }

    /// 构造一个最小可用的 source `Value`（只含合并需要关心的字段）。
    #[allow(clippy::too_many_arguments)]
    // 测试夹具：每个入参对应一份独立 JSON 片段，命名参数化反而更难读。
    fn source_fixture(
        source: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        uncached_input: u64,
        models: Vec<(&str, u64)>,
        sessions: Vec<(&str, u64)>,
        daily: Vec<(&str, u64)>,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Value {
        let models: Vec<Value> = models
            .into_iter()
            .map(|(key, total)| json!({ "key": key, "total": total }))
            .collect();
        let sessions: Vec<Value> = sessions
            .into_iter()
            .map(|(session_id, total)| json!({ "sessionId": session_id, "total": total }))
            .collect();
        let daily: Vec<Value> = daily
            .into_iter()
            .map(|(key, total)| json!({ "key": key, "total": total }))
            .collect();
        json!({
            "source": source,
            "summary": {
                "total": input + output + cache_read + uncached_input,
                "input": input,
                "output": output,
                "cacheRead": cache_read,
                "cacheWrite": 0,
                "uncachedInput": uncached_input,
                "records": 1,
                "cacheHitRate": (input > 0).then(|| cache_read as f64 / input as f64),
            },
            "models": models,
            "projects": [],
            "sessions": sessions,
            "daily": daily,
            "dailyByModel": { "gpt-x": daily_ref() },
            "hours": [],
            "filesScanned": 2,
            "parseErrors": 0,
            "coverageStartAt": start,
            "coverageEndAt": end,
        })
    }

    fn daily_ref() -> Value {
        json!([])
    }

    #[test]
    fn merge_source_values_sums_numeric_fields() {
        // 变异：把求和改成覆盖 / 取较大值 → 本断言失败。
        let cn = source_fixture(
            "workbuddy",
            10,
            5,
            10,
            10,
            vec![("gpt-x", 11)],
            vec![("s1", 11)],
            vec![("2024-01-01", 11)],
            Some(100),
            Some(200),
        );
        let global = source_fixture(
            "workbuddy",
            30,
            7,
            30,
            10,
            vec![("gpt-x", 33)],
            vec![("s1", 33)],
            vec![("2024-01-01", 33)],
            Some(150),
            Some(180),
        );
        let merged = merge_source_values(&[cn, global]);

        assert_eq!(merged["source"], "workbuddy");
        assert_eq!(merged["summary"]["input"], 40);
        assert_eq!(merged["summary"]["output"], 12);
        assert_eq!(merged["summary"]["cacheRead"], 40);
        assert_eq!(merged["summary"]["uncachedInput"], 20);
        assert_eq!(merged["summary"]["records"], 2);
        assert_eq!(merged["filesScanned"], 4);
        assert_eq!(merged["parseErrors"], 0);
        // coverage 取并集：min start / max end。
        assert_eq!(merged["coverageStartAt"], 100);
        assert_eq!(merged["coverageEndAt"], 200);
    }

    #[test]
    fn merge_source_values_recomputes_cache_hit_rate_instead_of_summing() {
        // cn: cacheRead=10, uncachedInput=10；global: cacheRead=30, uncachedInput=10。
        // 合并后 = 40 / (40 + 20) = 2/3。
        // 变异：把 cacheHitRate 改成相加（0.5 + 0.75 = 1.25）或朴素平均（0.625）→ 本断言失败。
        let cn = source_fixture(
            "workbuddy",
            20,
            0,
            10,
            10,
            vec![],
            vec![],
            vec![],
            None,
            None,
        );
        let global = source_fixture(
            "workbuddy",
            40,
            0,
            30,
            10,
            vec![],
            vec![],
            vec![],
            None,
            None,
        );
        assert_eq!(cn["summary"]["cacheHitRate"], 0.5);
        assert_eq!(global["summary"]["cacheHitRate"], 0.75);

        let merged = merge_source_values(&[cn, global]);
        let rate = merged["summary"]["cacheHitRate"]
            .as_f64()
            .expect("cacheHitRate is a finite number");
        assert!(
            (rate - (40.0f64 / 60.0f64)).abs() < 1e-9,
            "cacheHitRate must be recomputed as 40/60, got {rate}"
        );
        assert_ne!(rate, 1.25, "cacheHitRate must never be the naive sum");
        assert_ne!(rate, 0.625, "cacheHitRate must never be the naive average");
    }

    #[test]
    fn merge_source_values_recomputes_cache_hit_rate_to_null_when_denominator_zero() {
        let cn = source_fixture("workbuddy", 0, 0, 0, 0, vec![], vec![], vec![], None, None);
        let global = source_fixture("workbuddy", 0, 0, 0, 0, vec![], vec![], vec![], None, None);
        let merged = merge_source_values(&[cn, global]);
        assert!(merged["summary"]["cacheHitRate"].is_null());
    }

    #[test]
    fn merge_source_values_merges_same_named_keys_into_one_row() {
        // 变异：不做分组、直接 concat → 出现两条同名 key → 本断言失败。
        let cn = source_fixture(
            "workbuddy",
            0,
            0,
            0,
            0,
            vec![("gpt-x", 10), ("only-cn", 5)],
            vec![("s1", 10)],
            vec![("2024-01-01", 10)],
            None,
            None,
        );
        let global = source_fixture(
            "workbuddy",
            0,
            0,
            0,
            0,
            vec![("gpt-x", 30)],
            vec![("s1", 30)],
            vec![("2024-01-01", 30)],
            None,
            None,
        );
        let merged = merge_source_values(&[cn, global]);

        let models = merged["models"].as_array().expect("models");
        let gpt: Vec<&Value> = models
            .iter()
            .filter(|item| item["key"] == "gpt-x")
            .collect();
        assert_eq!(gpt.len(), 1, "同名 key 必须合并为一条，不得出现两条");
        assert_eq!(gpt[0]["total"], 40, "同名 key 的值必须是两版之和");

        let sessions = merged["sessions"].as_array().expect("sessions");
        let s1: Vec<&Value> = sessions
            .iter()
            .filter(|item| item["sessionId"] == "s1")
            .collect();
        assert_eq!(s1.len(), 1, "同 sessionId 必须合并为一条");
        assert_eq!(s1[0]["total"], 40);

        let daily = merged["daily"].as_array().expect("daily");
        let day: Vec<&Value> = daily
            .iter()
            .filter(|item| item["key"] == "2024-01-01")
            .collect();
        assert_eq!(day.len(), 1, "同日期必须合并为一条");
        assert_eq!(day[0]["total"], 40);
    }

    /// ★ 会话 `daily`（逐日拆分）在合并路径上必须**按日求和**，不得走「保留首个」。
    ///
    /// 变异：把 `merge_group_item` 里 `daily` 的分支删掉 ⇒ 只剩第一版的逐日
    /// （2024-01-02 整条丢失）⇒ 本条变红。合并作用域（两版同名源）下，
    /// 会话窗口切片的素材就是靠这条保住完整性的。
    #[test]
    fn merge_source_values_merges_session_daily_by_day() {
        let session = |total: u64, days: Vec<(&str, u64)>| {
            json!({
                "key": "p · s1",
                "sessionId": "s1",
                "total": total,
                "input": total,
                "output": 0,
                "cacheRead": 0,
                "cacheWrite": 0,
                "uncachedInput": total,
                "records": 1,
                "cacheHitRate": null,
                "daily": days
                    .into_iter()
                    .map(|(key, value)| json!({ "key": key, "total": value, "records": 1 }))
                    .collect::<Vec<Value>>(),
            })
        };
        let cn =
            json!({ "source": "workbuddy", "sessions": [session(40, vec![("2024-01-01", 40)])] });
        let global = json!({
            "source": "workbuddy",
            "sessions": [session(30, vec![("2024-01-01", 10), ("2024-01-02", 20)])]
        });

        let merged = merge_source_values(&[cn, global]);
        let sessions = merged["sessions"].as_array().expect("sessions");
        assert_eq!(sessions.len(), 1, "同 sessionId 合并为一条");
        let daily = sessions[0]["daily"].as_array().expect("daily 必须保留");
        let by_key: HashMap<&str, u64> = daily
            .iter()
            .map(|point| {
                (
                    point["key"].as_str().unwrap_or_default(),
                    point["total"].as_u64().unwrap_or(0),
                )
            })
            .collect();
        assert_eq!(daily.len(), 2, "两版并集逐日：{daily:?}");
        assert_eq!(by_key.get("2024-01-01"), Some(&50), "同一天 = 40 + 10");
        assert_eq!(
            by_key.get("2024-01-02"),
            Some(&20),
            "只有第二版有的天不得丢"
        );
    }

    /* ───────────────────── 扫描缓存 / 并行的等价性护栏 ─────────────────────
     *
     * 这一组是 P0-4 优化的**唯一验收标准**：缓存命中与冷路径必须**逐字段相同**，
     * 且缓存必须真的被用上（否则「优化」是假的）、必须随内容变化失效。
     */

    /// 冷路径 → 命中 → 手动清条目 → 内容改写，四条路径的输出与解析次数。
    #[test]
    fn scan_cache_is_equivalent_and_invalidates_on_content_change() {
        let now = crate::modules::config::now_ms();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-cache-{}-{now}",
            std::process::id()
        ));
        let project = root.join("fixture-project");
        fs::create_dir_all(&project).expect("create fixture dirs");
        let file = project.join("session.jsonl");
        let record = |input: u64| {
            json!({
                "timestamp": now,
                "cwd": "/fixture/example-project",
                "message": { "usage": { "input_tokens": input, "output_tokens": 2 } }
            })
        };
        fs::write(&file, format!("{}\n", record(10))).expect("write fixture");

        let cold = source(root.clone(), "fixture", None);
        assert_eq!(parse_count_for(&file), 1, "冷路径应当真的解析了一次");
        assert_eq!(cold["summary"]["input"], 10);
        assert_eq!(cold["filesScanned"], 1);

        let warm = source(root.clone(), "fixture", None);
        assert_eq!(warm, cold, "缓存命中必须与冷路径**逐字段相同**");
        assert_eq!(parse_count_for(&file), 1, "缓存命中不得再解析");

        // 重置入口必须真的有效：清掉条目后必须重新解析，且输出不变。
        clear_scan_cache_entry(&file);
        let reset = source(root.clone(), "fixture", None);
        assert_eq!(parse_count_for(&file), 2, "清条目后必须重新解析");
        assert_eq!(reset, cold, "清条目再解析的结果必须与冷路径相同");

        // 内容变了（size 也变）⇒ 必须失效，否则会把旧结果当新的用。
        fs::write(&file, format!("{}\n{}\n", record(10), record(90))).expect("rewrite fixture");
        let refreshed = source(root.clone(), "fixture", None);
        assert_eq!(parse_count_for(&file), 3, "改写后必须重新解析");
        assert_eq!(refreshed["summary"]["input"], 100);
        assert_eq!(refreshed["summary"]["records"], 2);
        assert_ne!(refreshed, cold, "改写后输出必须变化");

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /// `cutoff` 必须参与缓存键：有窗口的调用不得污染无窗口的条目。
    #[test]
    fn cutoff_is_part_of_the_scan_cache_key() {
        let now = crate::modules::config::now_ms();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-cutoff-{}-{now}",
            std::process::id()
        ));
        let project = root.join("fixture-project");
        fs::create_dir_all(&project).expect("create fixture dirs");
        let record = |timestamp| {
            json!({
                "timestamp": timestamp,
                "cwd": "/fixture/example-project",
                "message": { "usage": { "input_tokens": 10, "output_tokens": 2 } }
            })
        };
        fs::write(
            project.join("session.jsonl"),
            format!("{}\n{}\n", record(now - 10_000), record(now - 100_000)),
        )
        .expect("write fixture");

        let unbounded = source(root.clone(), "fixture", None);
        assert_eq!(unbounded["summary"]["records"], 2);

        let bounded = source(root.clone(), "fixture", Some(now - 50_000));
        assert_eq!(bounded["summary"]["records"], 1, "窗口必须生效");

        let unbounded_again = source(root.clone(), "fixture", None);
        assert_eq!(
            unbounded_again["summary"]["records"], 2,
            "有窗口的调用不得污染无窗口的缓存条目"
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /// 并行解析后，跨文件的同名会话去重后缀仍按**文件顺序**分配。
    ///
    /// 这是并行化最容易踩的坑：`session_key_counts` 属于整个 source，若把每个文件
    /// 的局部计数结果直接搬过来，两个文件都会得到「无后缀」的 key。
    #[test]
    fn parallel_scan_keeps_cross_file_session_key_dedup() {
        let now = crate::modules::config::now_ms();
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-dedup-{}-{now}",
            std::process::id()
        ));
        let record = |input: u64| {
            json!({
                "timestamp": now,
                "cwd": "/fixture/shared-project",
                "message": { "usage": { "input_tokens": input, "output_tokens": 1 } }
            })
        };
        // 两个文件**同名不同目录**、`cwd` 又相同 ⇒ `项目 · 会话` 撞车 ⇒ 必须有后缀。
        // 两个文件的用量**刻意不同**：这样「哪个文件拿到无后缀 key」才可断言，
        // 否则把合并顺序倒过来结果集合不变，用例会漏掉顺序回归。
        fs::create_dir_all(root.join("alpha")).expect("create fixture dirs");
        fs::create_dir_all(root.join("beta")).expect("create fixture dirs");
        fs::write(
            root.join("alpha/same-stem.jsonl"),
            format!("{}\n", record(10)),
        )
        .expect("write fixture");
        fs::write(
            root.join("beta/same-stem.jsonl"),
            format!("{}\n", record(100)),
        )
        .expect("write fixture");

        let result = source(root.clone(), "fixture", None);
        let mut keys: Vec<(String, u64)> = result["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .map(|session| {
                (
                    session["key"].as_str().unwrap_or_default().to_string(),
                    session["total"].as_u64().unwrap_or_default(),
                )
            })
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                // `paths.sort()` ⇒ alpha 在前 ⇒ 它拿无后缀 key，beta 拿 `· 2`。
                ("shared-project · same-stem".to_string(), 11),
                ("shared-project · same-stem · 2".to_string(), 101),
            ],
            "跨文件同名会话必须按文件顺序拿到 1 / 2 后缀（顺序颠倒 ⇒ 两行对调，必须红）"
        );
        assert_eq!(result["summary"]["records"], 2);
        assert_eq!(result["filesScanned"], 2);

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /// **跨文件聚合**的黄金断言 —— 等价性证明的主力。
    ///
    /// 改造把「全源共用一个累加器」换成「每文件一个累加器 + 有序合并」，
    /// 风险全在**跨文件**的语义上：同名模型/项目/日期是否仍合并、`sessions` 的
    /// 排序与去重后缀是否仍按文件顺序、`coverage*` 是否仍是全局 min/max。
    /// 这条用例把这些逐项钉死，并**在改造前的实现上同样通过**（见提交说明）。
    #[test]
    fn multi_file_aggregation_matches_single_collector_semantics() {
        use chrono::TimeZone;

        let today = Local::now().date_naive();
        let at = |hour: u32| {
            Local
                .with_ymd_and_hms(today.year(), today.month(), today.day(), hour, 0, 0)
                .single()
                .expect("valid local time")
                .timestamp_millis()
        };
        let (t1, t2, t3) = (at(12), at(13), at(14));
        let day_key = today.format("%Y-%m-%d").to_string();
        let weekday = today.weekday().num_days_from_monday();

        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-multi-{}-{}",
            std::process::id(),
            crate::modules::config::now_ms()
        ));
        fs::create_dir_all(root.join("proj-a")).expect("create fixture dirs");
        fs::create_dir_all(root.join("proj-b")).expect("create fixture dirs");

        let record = |timestamp, cwd: &str, model: &str, input: u64, output: u64, read: u64| {
            json!({
                "timestamp": timestamp,
                "cwd": cwd,
                "providerData": { "model": model },
                "message": { "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": read
                }}
            })
        };

        // 文件 1：两个模型、两个整点，另有一条**只带标题**的行（无 usage）。
        fs::write(
            root.join("proj-a/s1.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                json!({ "type": "ai-title", "aiTitle": "标题 A" }),
                record(t1, "/x/alpha", "m1", 10, 1, 0),
                record(t2, "/x/alpha", "m2", 20, 2, 5),
            ),
        )
        .expect("write fixture");
        // 文件 2：与文件 1 共享模型 `m1`，项目不同；含一条 input=0 的记录。
        fs::write(
            root.join("proj-b/s2.jsonl"),
            format!(
                "{}\n{}\n",
                record(t1, "/x/beta", "m1", 100, 10, 0),
                record(t3, "/x/beta", "m1", 0, 7, 0),
            ),
        )
        .expect("write fixture");

        let result = source(root.clone(), "fixture", None);

        assert_eq!(result["filesScanned"], 2);
        assert_eq!(result["parseErrors"], 0);
        // 输入 = 10+20+100+0；输出 = 1+2+10+7；cacheRead = 5；write 恒 0。
        assert_eq!(result["summary"]["records"], 4);
        assert_eq!(result["summary"]["input"], 130);
        assert_eq!(result["summary"]["output"], 20);
        assert_eq!(result["summary"]["cacheRead"], 5);
        assert_eq!(result["summary"]["cacheWrite"], 0);
        assert_eq!(result["summary"]["total"], 150);
        // coverage 是**跨文件**的 min/max，不是「最后一个文件」的值。
        assert_eq!(result["coverageStartAt"], t1);
        assert_eq!(result["coverageEndAt"], t3);

        // 同名模型必须合并成一条，并按 total 降序：m1=128 > m2=22。
        let models = result["models"].as_array().expect("models");
        let model_keys: Vec<&str> = models.iter().filter_map(|m| m["key"].as_str()).collect();
        assert_eq!(model_keys, ["m1", "m2"]);
        assert_eq!(models[0]["total"], 128);
        assert_eq!(models[0]["records"], 3);
        assert_eq!(models[1]["total"], 22);

        // 项目按 total 降序：beta=117 > alpha=33。
        let projects = result["projects"].as_array().expect("projects");
        let project_keys: Vec<&str> = projects.iter().filter_map(|p| p["key"].as_str()).collect();
        assert_eq!(project_keys, ["beta", "alpha"]);
        assert_eq!(projects[0]["total"], 117);
        assert_eq!(projects[1]["total"], 33);

        // 会话按 total 降序，标题只跟着**它自己那个文件**走。
        let sessions = result["sessions"].as_array().expect("sessions");
        let session_keys: Vec<&str> = sessions.iter().filter_map(|s| s["key"].as_str()).collect();
        assert_eq!(session_keys, ["beta · s2", "alpha · s1"]);
        assert_eq!(sessions[0]["total"], 117);
        assert_eq!(sessions[0]["title"], Value::Null);
        assert_eq!(sessions[1]["total"], 33);
        assert_eq!(sessions[1]["title"], "标题 A");

        // 日期只有今天一天，四条记录全部落在上面。
        let daily = result["daily"].as_array().expect("daily");
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0]["key"], day_key);
        assert_eq!(daily[0]["total"], 150);
        assert_eq!(daily[0]["records"], 4);
        // dailyByModel 也按模型分家：m1=128、m2=22。
        assert_eq!(result["dailyByModel"]["m1"][0]["total"], 128);
        assert_eq!(result["dailyByModel"]["m2"][0]["total"], 22);

        // 整点桶：12 点两条（11+110）、13 点一条、14 点一条。
        let hours = result["hours"].as_array().expect("hours");
        let hour_keys: Vec<&str> = hours.iter().filter_map(|h| h["key"].as_str()).collect();
        let h12 = format!("{weekday}-12");
        let h13 = format!("{weekday}-13");
        let h14 = format!("{weekday}-14");
        assert_eq!(hour_keys, [h12, h13, h14], "整点桶按 total 降序");
        assert_eq!(hours[0]["total"], 121);
        assert_eq!(hours[0]["records"], 2);
        assert_eq!(hours[1]["total"], 22);
        assert_eq!(hours[2]["total"], 7);

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /* ─────────────────── 落盘缓存（进程间）的等价性护栏 ─────────────────── */

    /// 造一份**字段全非空**的夹具，返回 `(root, paths, 冷路径解析结果)`。
    fn rich_fixture(tag: &str) -> (PathBuf, Vec<PathBuf>, Vec<Arc<SourceCollector>>) {
        use chrono::TimeZone;

        let today = Local::now().date_naive();
        let at = |hour: u32| {
            Local
                .with_ymd_and_hms(today.year(), today.month(), today.day(), hour, 0, 0)
                .single()
                .expect("valid local time")
                .timestamp_millis()
        };
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-stats-{tag}-{}-{}",
            std::process::id(),
            crate::modules::config::now_ms()
        ));
        fs::create_dir_all(root.join("proj-a")).expect("create fixture dirs");
        fs::write(
            root.join("proj-a/s1.jsonl"),
            format!(
                "{}\n{}\n{}\nnot-json\n",
                json!({ "type": "ai-title", "aiTitle": "标题 A" }),
                json!({
                    "timestamp": at(12), "cwd": "/x/alpha",
                    "providerData": { "model": "m1" },
                    "message": { "usage": { "input_tokens": 10, "output_tokens": 1 } }
                }),
                json!({
                    "timestamp": at(13), "cwd": "/x/alpha",
                    "providerData": { "model": "m2" },
                    "message": { "usage": {
                        "input_tokens": 20, "output_tokens": 2, "cache_read_input_tokens": 5
                    }}
                }),
            ),
        )
        .expect("write fixture");

        let mut paths = Vec::new();
        files(&root, &mut paths);
        paths.sort();
        // 冷路径：先清掉可能存在的同名条目，保证这次真的解析。
        for path in &paths {
            clear_scan_cache_entry(path);
        }
        let cold = scan_paths(&root, &paths, None);
        (root, paths, cold)
    }

    fn merge_all(scans: &[Arc<SourceCollector>], files_scanned: usize) -> Value {
        let mut collector = SourceCollector::default();
        for scan in scans {
            collector.absorb(scan);
        }
        collector.into_value("fixture", files_scanned)
    }

    /// **落盘往返必须无损**：把逐文件结果写进 JSON、读回来，合并结果必须逐字段相同。
    ///
    /// 这是落盘层唯一的新风险 —— 「缓存命中即等价」在进程内缓存那轮已经证过，
    /// 这里要证的只有**序列化保真**。
    #[test]
    fn persisted_entry_round_trips_every_field() {
        let (root, paths, cold) = rich_fixture("persist-roundtrip");
        assert_eq!(cold.len(), 1);

        let entry = ScanCacheEntry {
            mtime_ms: 123,
            size: 456,
            cutoff: None,
            scan: cold[0].clone(),
        };
        let text = serde_json::to_string(&PersistedScanCache {
            version: SCAN_CACHE_VERSION,
            files: BTreeMap::from([("k".to_string(), entry.to_persisted())]),
        })
        .expect("serialize");
        let restored = serde_json::from_str::<PersistedScanCache>(&text).expect("deserialize");
        let restored =
            ScanCacheEntry::from_persisted(restored.files.into_values().next().expect("one entry"));

        assert_eq!(restored.mtime_ms, 123);
        assert_eq!(restored.size, 456);
        assert_eq!(restored.cutoff, None);

        let before = merge_all(&cold, paths.len());
        let after = merge_all(std::slice::from_ref(&restored.scan), paths.len());
        assert_eq!(after, before, "落盘往返后的合并结果必须逐字段相同");
        // 夹具刻意带一条坏行与一条只有标题的行 ⇒ 这些也必须活着往返。
        assert_eq!(after["parseErrors"], 1, "parseErrors 必须往返");
        assert_eq!(after["sessions"][0]["title"], "标题 A", "标题必须往返");

        fs::remove_dir_all(root).expect("remove fixture");
    }

    /// 写盘 → 读回：条目数与内容指纹都要保住；**异版本必须整体拒绝**（宁可重扫）。
    #[test]
    fn persisted_cache_file_round_trips_and_rejects_a_foreign_version() {
        let (root, paths, cold) = rich_fixture("persist-file");
        let store = root.join("store");
        fs::create_dir_all(&store).expect("create store dir");
        let cache_file = store.join("token-scan-cache.json");

        let cache: HashMap<PathBuf, ScanCacheEntry> = paths
            .iter()
            .cloned()
            .zip(cold.iter().cloned())
            .map(|(path, scan)| {
                (
                    path,
                    ScanCacheEntry {
                        mtime_ms: 7,
                        size: 8,
                        cutoff: None,
                        scan,
                    },
                )
            })
            .collect();
        write_persisted_cache(&cache_file, &cache);
        assert!(cache_file.exists(), "缓存文件必须被写出");

        let loaded = read_persisted_cache(&cache_file).expect("读回缓存");
        assert_eq!(loaded.len(), 1);
        let restored = loaded.values().next().expect("one entry");
        assert_eq!(restored.mtime_ms, 7);
        assert_eq!(restored.size, 8);
        assert_eq!(
            merge_all(std::slice::from_ref(&restored.scan), paths.len()),
            merge_all(&cold, paths.len()),
            "从文件读回的结果必须与冷路径相同"
        );

        // 异版本：整体丢弃，绝不能读出错值。
        let text = std::fs::read_to_string(&cache_file).expect("read cache");
        std::fs::write(
            &cache_file,
            text.replace(
                &format!("\"version\":{SCAN_CACHE_VERSION}"),
                "\"version\":999999",
            ),
        )
        .expect("write foreign version");
        assert!(
            read_persisted_cache(&cache_file).is_none(),
            "版本不符必须整体拒绝"
        );

        // 坏 JSON 同样必须静默回落，而不是让统计失败。
        std::fs::write(&cache_file, "{ not json").expect("write corrupt cache");
        assert!(
            read_persisted_cache(&cache_file).is_none(),
            "坏 JSON 必须静默回落"
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }
}
