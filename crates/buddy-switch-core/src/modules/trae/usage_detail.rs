//! 上游**逐请求用量明细**（积分回填的数据源）。
//!
//! ## 这个模块解决什么
//!
//! Trae 的 SSE 流里**没有积分**（`event:token_usage` 只带 token），而网关的请求日志
//! 要显示「这次请求花了多少积分」。上游其实有一张账单表 —— Trae 客户端「用量明细」
//! 页面就是它 —— 接口为：
//!
//! ```text
//! POST {account_base}/trae/api/v1/pay/query_user_usage_group_by_session
//! body: {"start_time":<unix 秒>,"end_time":<unix 秒>,"page_size":50,"page_num":1,
//!        "usage_type":[7]}
//! resp: {total, user_usage_group_by_sessions:[{usage_time, credits_float, model_name,
//!        extra_info:{input_token,output_token,cache_read_token,cache_write_token}, …}]}
//! ```
//!
//! ## 三个**实测**约束（2026-11-07 真实账号跑通，勿凭直觉改）
//!
//! 1. **`usage_type:[7]` 必传**：不传时 `total` 恒为 0，一行都拿不到。
//! 2. **`page_size` 上限 50**：60 及以上返回 `code 9004`。排序是**最新在前**。
//! 3. **不能用 `session_id` 对齐**：上游自己另发一个会话号，与网关发出去的
//!    （`sess:` / `conv:` / `proj:` 三个派生 id）**都不相同**。唯一可靠的关联方式是
//!    `(账号, input_token, output_token)` + 时间邻近。
//!
//! ## 模型名两边**不一致**，因此只能做 tiebreak
//!
//! 网关日志写的是请求时的别名（`deepseek-v4.1-flash`），账单写的是展示名
//! （`DeepSeek-V4.1-Flash`）。用等值过滤会**一条都匹配不上**，所以这里只做
//! 归一化后比较，且**只在时间距离相同时**用来分胜负。
//!
//! ## 本模块只做「取数 + 匹配」，不发 patch、不碰日志文件
//!
//! 编排（何时拉、拉哪个账号、写回哪里）在网关侧
//! `buddy_switch_gateway::trae::credit_backfill`。这样匹配算法可以**纯函数单测**，
//! 不依赖网络、不依赖进程级 home 目录。

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::modules::trae::credits;
use crate::modules::trae::device::DeviceEntry;
use crate::modules::trae::variant::TraeVariant;

/// 上游用量明细接口路径（挂在 `endpoints.account_base` 下）。
pub const USAGE_QUERY_PATH: &str = "/trae/api/v1/pay/query_user_usage_group_by_session";

/// 单页行数上限（实测 60 及以上返回 `code 9004`）。
pub const MAX_PAGE_SIZE: u64 = 50;

/// `usage_type` 里的「积分口径」开关。**不传它 = 一行都拿不到**（实测）。
pub const USAGE_TYPE_CREDITS: u64 = 7;

/// 账单行允许比请求早的秒数（容忍本机时钟略快）。
pub const MATCH_EARLY_SECS: i64 = 60;

/// 账单行允许比请求晚的秒数。
///
/// 实测出账比请求结束晚 **6–8 秒**；长流式请求的出账时间还可能更晚一点，故留 10 分钟。
pub const MATCH_LATE_SECS: i64 = 600;

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 一条上游账单行（已归一，只留回填需要的字段）。
#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    /// 出账时间（unix **秒**）。注意：是上游记账时刻，不是请求开始时刻。
    pub usage_time: i64,
    /// 上游展示名（`DeepSeek-V4.1-Flash`），与网关的模型别名**不同形**。
    pub model_name: String,
    /// 积分消耗。上游字段叫 `credits_float`，**已经是数值本身**（不是厘）。
    pub credits: f64,
    pub input_token: u64,
    pub output_token: u64,
    pub cache_read_token: u64,
    pub cache_write_token: u64,
    /// 合成行键：账单行**没有 id**，去重环需要自己造一个稳定键。
    pub row_key: String,
}

impl UsageRow {
    fn from_value(value: &Value) -> Option<Self> {
        let usage_time = value.get("usage_time").and_then(Value::as_i64)?;
        let extra = value.get("extra_info");
        let number = |key: &str| -> u64 {
            extra
                .and_then(|info| info.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let model_name = value
            .get("model_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let credits = value
            .get("credits_float")
            .and_then(Value::as_f64)
            .or_else(|| value.get("amount_float").and_then(Value::as_f64))
            .unwrap_or(0.0);
        let input_token = number("input_token");
        let output_token = number("output_token");
        let cache_read_token = number("cache_read_token");
        let cache_write_token = number("cache_write_token");
        let row_key =
            format!("{usage_time}|{input_token}|{output_token}|{model_name}|{credits:.4}");
        Some(Self {
            usage_time,
            model_name,
            credits,
            input_token,
            output_token,
            cache_read_token,
            cache_write_token,
            row_key,
        })
    }
}

/// 待回填的一条日志（由网关从内存日志里取出）。
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEntry {
    /// 日志条目的 `id`（回填的唯一定位键）。
    pub id: String,
    /// 日志写入时间（**毫秒**）。
    pub ts_ms: i64,
    /// 网关侧的模型别名。
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// 一次成功的匹配。
#[derive(Debug, Clone, PartialEq)]
pub struct CreditMatch {
    pub entry_id: String,
    pub credits: f64,
    /// 命中的账单行键 —— 调用方据此推进「已消费行」去重环。
    pub row_key: String,
}

// ---------------------------------------------------------------------------
// 匹配
// ---------------------------------------------------------------------------

/// 模型名归一：小写 + 只留字母数字。
///
/// `deepseek-v4.1-flash` 与 `DeepSeek-V4.1-Flash` 归一后相同；归一后仍不同时
/// **不算失配**（只是少一个 tiebreak 信号，见模块头）。
pub fn normalize_model(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

/// 批量匹配：把账单行分配给待回填日志。
///
/// ## 判据
///
/// 1. **token 必须完全相等**（input 与 output 都要）—— 这是唯一能在两端共用的身份；
/// 2. 时间落在 `[ts - MATCH_EARLY_SECS, ts + MATCH_LATE_SECS]`；
/// 3. 一行只能被消费一次（本轮 + `seen` 里的历史行都不能再用）；
/// 4. 一对一贪心：按日志时间**从旧到新**处理，取时间最近的未消费行；
///   距离相同时优先模型归一后相等的行。
///
/// ## 为什么不用「按时间排序后 zip」
///
/// 并发请求会交错出账，token 又可能完全相同（同一段对话连续两轮）。贪心 + 距离
/// 最近已经足够，zip 则会把两条请求的积分整体错位一格。
pub fn match_rows(
    entries: &[PendingEntry],
    rows: &[UsageRow],
    seen: &HashSet<String>,
) -> Vec<CreditMatch> {
    let mut ordered: Vec<&PendingEntry> = entries.iter().collect();
    ordered.sort_by_key(|entry| (entry.ts_ms, entry.id.clone()));

    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::new();

    for entry in ordered {
        let entry_secs = entry.ts_ms.div_euclid(1000);
        let entry_model = normalize_model(&entry.model);
        let mut best: Option<(i64, bool, &UsageRow)> = None;

        for row in rows {
            if row.input_token != entry.input_tokens || row.output_token != entry.output_tokens {
                continue;
            }
            if seen.contains(&row.row_key) || used.contains(&row.row_key) {
                continue;
            }
            let delta = row.usage_time - entry_secs;
            if delta < -MATCH_EARLY_SECS || delta > MATCH_LATE_SECS {
                continue;
            }
            let distance = delta.abs();
            let model_equal =
                !entry_model.is_empty() && normalize_model(&row.model_name) == entry_model;
            let better = match best {
                None => true,
                Some((best_distance, best_model, _)) => {
                    // 距离优先；同距离才看模型是否同名。
                    distance < best_distance
                        || (distance == best_distance && model_equal && !best_model)
                }
            };
            if better {
                best = Some((distance, model_equal, row));
            }
        }

        if let Some((_, _, row)) = best {
            used.insert(row.row_key.clone());
            out.push(CreditMatch {
                entry_id: entry.id.clone(),
                credits: row.credits,
                row_key: row.row_key.clone(),
            });
        }
    }

    out
}

// ---------------------------------------------------------------------------
// 上游请求
// ---------------------------------------------------------------------------

/// 一页账单。
#[derive(Debug, Clone, Default)]
pub struct UsagePage {
    pub total: u64,
    pub rows: Vec<UsageRow>,
}

/// 解析响应体。业务错误（`code != 0` 且没有数据）转成 `Err`。
pub fn parse_page(value: &Value) -> Result<UsagePage, String> {
    let rows_value = value
        .get("user_usage_group_by_sessions")
        .and_then(Value::as_array);
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(0);
    if rows_value.is_none() {
        if code != 0 {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            return Err(format!("code {code}: {message}"));
        }
        return Err("响应缺少 user_usage_group_by_sessions".to_string());
    }
    let rows = rows_value
        .map(|list| list.iter().filter_map(UsageRow::from_value).collect())
        .unwrap_or_default();
    Ok(UsagePage {
        total: value.get("total").and_then(Value::as_u64).unwrap_or(0),
        rows,
    })
}

/// POST 一次账单查询（带 JSON body）。
async fn post_usage(
    variant: TraeVariant,
    jwt_value: &str,
    device: &DeviceEntry,
    body: &Value,
) -> Result<Value, String> {
    let base = super::endpoints_for(variant).account_base;
    let url = format!("{base}{USAGE_QUERY_PATH}");
    let mut request = credits::trae_http_client()
        .post(&url)
        .body(body.to_string());
    for (key, value) in credits::build_headers(jwt_value, device) {
        request = request.header(key, value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| crate::modules::net::transport_error(&error).to_wire())?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if status != 200 {
        let brief: String = text.chars().take(200).collect();
        return Err(format!("HTTP {status}: {brief}"));
    }
    serde_json::from_str::<Value>(&text).map_err(|_| {
        format!(
            "HTTP {status}: 非 JSON 响应: {}",
            text.chars().take(200).collect::<String>()
        )
    })
}

/// 取一个时间窗（单位：unix 秒）内的全部账单行，最多 `max_pages` 页。
///
/// 返回 `(行, 总行数)`。行按上游顺序（**最新在前**）。翻页在
/// 「本页不满 / 已拿够 `total` / 达到页数上限」任一条件成立时停止。
pub async fn fetch_window(
    variant: TraeVariant,
    jwt_value: &str,
    device: &DeviceEntry,
    start_time: i64,
    end_time: i64,
    max_pages: u32,
) -> Result<(Vec<UsageRow>, u64), String> {
    let mut rows = Vec::new();
    let mut total = 0u64;
    let pages = max_pages.max(1);
    for page in 1..=pages {
        let body = json!({
            "start_time": start_time,
            "end_time": end_time,
            "page_size": MAX_PAGE_SIZE,
            "page_num": page,
            "usage_type": [USAGE_TYPE_CREDITS],
        });
        let value = post_usage(variant, jwt_value, device, &body).await?;
        let parsed = parse_page(&value)?;
        total = parsed.total;
        let fetched = parsed.rows.len();
        rows.extend(parsed.rows);
        if fetched < MAX_PAGE_SIZE as usize {
            break;
        }
        if rows.len() as u64 >= total {
            break;
        }
    }
    Ok((rows, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(time: i64, input: u64, output: u64, model: &str, credits: f64) -> UsageRow {
        UsageRow {
            usage_time: time,
            model_name: model.to_string(),
            credits,
            input_token: input,
            output_token: output,
            cache_read_token: 0,
            cache_write_token: 0,
            row_key: format!("{time}|{input}|{output}|{model}|{credits:.4}"),
        }
    }

    fn entry(id: &str, ts_ms: i64, input: u64, output: u64, model: &str) -> PendingEntry {
        PendingEntry {
            id: id.to_string(),
            ts_ms,
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
        }
    }

    /// 最基本的命中：token 相等 + 时间邻近（实测账单晚 8 秒）。
    #[test]
    fn matches_by_tokens_within_window() {
        let entries = vec![entry(
            "e1",
            1_794_051_874_000,
            27639,
            199,
            "deepseek-v4.1-flash",
        )];
        let rows = vec![row(
            1_794_051_882,
            27639,
            199,
            "DeepSeek-V4.1-Flash",
            0.2792,
        )];
        let out = match_rows(&entries, &rows, &HashSet::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_id, "e1");
        assert!((out[0].credits - 0.2792).abs() < 1e-9);
    }

    /// 模型名两边不同形（别名 vs 展示名），**不能**因此失配。
    #[test]
    fn model_name_difference_is_not_a_mismatch() {
        assert_eq!(
            normalize_model("deepseek-v4.1-flash"),
            normalize_model("DeepSeek-V4.1-Flash")
        );
        let entries = vec![entry("e1", 1_000_000_000_000, 10, 2, "some-alias")];
        let rows = vec![row(1_000_000_004, 10, 2, "Totally-Different-Display", 1.0)];
        assert_eq!(match_rows(&entries, &rows, &HashSet::new()).len(), 1);
    }

    /// token 不等就绝不匹配 —— 宁可留 pending，也不能把别人的积分写上来。
    #[test]
    fn token_mismatch_never_matches() {
        let entries = vec![entry("e1", 1_000_000_000_000, 10, 2, "m")];
        let rows = vec![row(1_000_000_004, 11, 2, "m", 1.0)];
        assert!(match_rows(&entries, &rows, &HashSet::new()).is_empty());
    }

    /// 时间窗外（早 61s / 晚 601s）不匹配。
    #[test]
    fn outside_time_window_never_matches() {
        let entries = vec![entry("e1", 1_000_000_000_000, 10, 2, "m")];
        let early = vec![row(1_000_000_000 - 61, 10, 2, "m", 1.0)];
        let late = vec![row(1_000_000_000 + 601, 10, 2, "m", 1.0)];
        assert!(match_rows(&entries, &early, &HashSet::new()).is_empty());
        assert!(match_rows(&entries, &late, &HashSet::new()).is_empty());
        let inside = vec![row(1_000_000_000 + 600, 10, 2, "m", 1.0)];
        assert_eq!(match_rows(&entries, &inside, &HashSet::new()).len(), 1);
    }

    /// 同一行账单不能被两条日志重复消费（并发请求同 token 时的护栏）。
    #[test]
    fn one_row_is_consumed_at_most_once() {
        let entries = vec![
            entry("e1", 1_000_000_000_000, 10, 2, "m"),
            entry("e2", 1_000_001_000_000, 10, 2, "m"),
        ];
        let rows = vec![row(1_000_000_005, 10, 2, "m", 0.5)];
        let out = match_rows(&entries, &rows, &HashSet::new());
        assert_eq!(out.len(), 1, "一行只能回填一条日志");
    }

    /// 两条日志、两行账单 ⇒ 按时间就近配对，不串位。
    #[test]
    fn greedy_pairs_nearest_row() {
        let entries = vec![
            entry("e1", 1_000_000_000_000, 10, 2, "m"),
            entry("e2", 1_000_000_100_000, 20, 3, "m"),
        ];
        let rows = vec![
            row(1_000_000_102, 20, 3, "m", 2.0),
            row(1_000_000_003, 10, 2, "m", 1.0),
        ];
        let out = match_rows(&entries, &rows, &HashSet::new());
        assert_eq!(out.len(), 2);
        let by_id: std::collections::BTreeMap<&str, f64> = out
            .iter()
            .map(|hit| (hit.entry_id.as_str(), hit.credits))
            .collect();
        assert_eq!(by_id["e1"], 1.0);
        assert_eq!(by_id["e2"], 2.0);
    }

    /// `seen` 里的行视为已消费：跨轮不重复回填。
    #[test]
    fn seen_rows_are_skipped() {
        let entries = vec![entry("e1", 1_000_000_000_000, 10, 2, "m")];
        let rows = vec![row(1_000_000_005, 10, 2, "m", 0.5)];
        let mut seen = HashSet::new();
        seen.insert(rows[0].row_key.clone());
        assert!(match_rows(&entries, &rows, &seen).is_empty());
    }

    /// 响应解析：真实抓包的形状；缺数组时给出可读错误。
    #[test]
    fn parse_real_response_shape() {
        let value = serde_json::json!({
            "total": 231,
            "user_usage_group_by_sessions": [{
                "usage_time": 1794051882,
                "credits_float": 0.2792,
                "amount_float": 0.2792,
                "cost_money_float": 0.00698,
                "dollar_float": 0,
                "mode": "",
                "model_name": "DeepSeek-V4.1-Flash",
                "product_type_list": [2],
                "session_id": "1af44efd-c51e-4212-9f4c-d2970f027ef8",
                "usage_source": 0,
                "user_input_preview": "Prompt",
                "use_max_mode": false,
                "extra_info": {
                    "cache_read_token": 21888,
                    "cache_write_token": 0,
                    "input_token": 27639,
                    "output_token": 199
                }
            }]
        });
        let page = parse_page(&value).expect("应能解析");
        assert_eq!(page.total, 231);
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].input_token, 27639);
        assert_eq!(page.rows[0].cache_read_token, 21888);
        assert!((page.rows[0].credits - 0.2792).abs() < 1e-9);

        let error =
            parse_page(&serde_json::json!({"code": 9004, "message": "page_size too large"}))
                .expect_err("业务错误必须转 Err");
        assert!(error.contains("9004"), "错误里要带码，实际：{error}");
    }
}
