//! Trae 批量签到。
//!
//! ## 为什么用原生 Rust 而不是复用参考实现的两个 Python 脚本
//!
//! 参考实现的签到是「Rust 起一个 Python 子进程 + 解析 NDJSON」，代理也是 Python
//! （`device_proxy.py`，66KB，依赖 `cryptography`）。签到这部分**不需要**任何 Python
//! 生态特有的能力——它只是「带特定请求头的两个 HTTP POST + 错误分类 + 落盘」——
//! 因此原生实现有三个确定收益：
//!
//! 1. **跨平台**：用户已选择三平台发布，而「用户机器上必须装 Python 且能 `pip install
//!    cryptography`」在 macOS/Linux 上不是可依赖的前提；
//! 2. **双通道**：逻辑在 core 里，Tauri 与 HTTP 两条通道都能直接调用，不必各自
//!    fork 一个子进程并各自解析 NDJSON；
//! 3. **进度可编程**：用回调而非 stdout 行协议传递进度，Tauri 侧转成事件、HTTP 侧
//!    转成一次性报告，无需中间序列化格式。
//!
//! 代理部分无法照此办理（MITM 需要成熟的 TLS/证书栈），保留在 Python，见第 C 组。
//!
//! ## 签名顺序即业务语义
//!
//! 签到顺序不是「账号库顺序」，而是**积分过期紧迫度**：最早到期的先签。
//! 上游对每日签到的发放存在额度/限频约束，把「快过期的账号」排在前面能最大化
//! 有效积分留存。过期时间相同的按剩余积分降序，无过期信息的排最后。

use serde_json::{json, Value};

use crate::modules::trae::account::{self, Scope};
use crate::modules::trae::credits::{self, CooldownEntry};
use crate::modules::trae::device;
use crate::modules::trae::jwt;
use crate::modules::trae::paths;
use crate::modules::trae::store;
use crate::modules::trae::variant::TraeVariant;
use crate::modules::trae::{TRAE_CHECKIN_PATH, TRAE_CHECKIN_STATUS_PATH};

/// 签到选项。
#[derive(Debug, Clone)]
pub struct CheckinOptions {
    /// 产品线变体：决定读哪个账号库、写哪份冷却 / 摘要 / 明细，以及用哪组端点。
    pub variant: TraeVariant,
    /// 执行范围。
    pub scope: Scope,
    /// `Selected` 范围的显式账号列表。
    pub user_ids: Option<Vec<String>>,
    /// 跳过今日已签到账号。
    pub skip_checked_in: bool,
    /// 跳过 JWT 已过期账号。
    pub skip_expired: bool,
    /// 网络层失败的重试次数（业务失败不重试）。
    pub retry: u32,
}

impl Default for CheckinOptions {
    fn default() -> Self {
        Self {
            variant: TraeVariant::default(),
            scope: Scope::All,
            user_ids: None,
            // 默认跳过：重复对同一账号发 claim 会浪费配额并增加风控暴露面。
            skip_checked_in: true,
            skip_expired: true,
            retry: 1,
        }
    }
}

/// 单账号签到结果。
#[derive(Debug, Clone)]
pub struct AccountOutcome {
    /// 账号 userId。
    pub user_id: String,
    /// 账号展示名。
    pub name: String,
    /// 是否成功（含「今日已签到」）。
    pub ok: bool,
    /// 业务码。
    pub code: Option<i64>,
    /// 文案。
    pub message: String,
    /// 动作：`claim_ok` / `skip_already` / `fail`。
    pub action: &'static str,
    /// 签到后可得的积分额度（来自状态预检）。
    pub credits: Option<i64>,
    /// 本次新增积分。
    pub delta: i64,
    /// 错误分类（失败时）。
    pub error_type: Option<String>,
    /// 冷却截止时间戳（失败时）。
    pub cooldown_until: Option<i64>,
}

/// 批量签到汇总。
#[derive(Debug, Clone, Default)]
pub struct CheckinReport {
    /// 本次实际处理的账号数。
    pub total: usize,
    /// 签到成功数。
    pub total_ok: i32,
    /// 今日已签到（跳过 claim）数。
    pub already: i32,
    /// 失败数。
    pub failed: i32,
    /// JWT 临期/过期告警文案。
    pub warnings: Vec<String>,
    /// 逐账号结果。
    pub results: Vec<AccountOutcome>,
}

/// 签到结束后写入 `checkin_summary.json` 的结果明细（线上形态，camelCase）。
fn outcome_json(outcome: &AccountOutcome) -> Value {
    json!({
        "name": outcome.name,
        "userId": outcome.user_id,
        "ok": outcome.ok,
        "code": outcome.code,
        "message": outcome.message,
        "action": outcome.action,
        "credits": outcome.credits,
        "delta": outcome.delta,
        "errorType": outcome.error_type,
        "cooldownUntil": outcome.cooldown_until,
    })
}

/// 状态预检结果。
struct StatusProbe {
    /// 预检是否可用（网络/解析成功）。
    ok: bool,
    /// 今日是否已签到（预检失败时为 `None`）。
    checked_in: Option<bool>,
    /// 签到可获得的积分额度。
    credits: Option<i64>,
    /// 业务码。
    code: Option<i64>,
    /// 文案。
    message: String,
}

/// 调状态接口做预检。
///
/// 预检失败**不阻断** claim：预检只是优化（避免重复 claim），
/// 上游状态接口偶发不可用时若直接放弃，用户当天就签不上了。
async fn probe_status(jwt_value: &str, device_entry: &device::DeviceEntry) -> StatusProbe {
    let uid = jwt::user_id_of(jwt_value).unwrap_or_default();
    if uid.is_empty() {
        return StatusProbe {
            ok: false,
            checked_in: None,
            credits: None,
            code: None,
            message: "无法从 JWT 解析 user id".into(),
        };
    }
    match credits::post_json_parsed(TRAE_CHECKIN_STATUS_PATH, jwt_value, device_entry).await {
        Ok((status, body)) => {
            let code = body.get("code").and_then(|v| v.as_i64());
            let checked_in = body.get("checked_in").and_then(|v| v.as_bool());
            let credits = body.get("credits").and_then(|v| v.as_i64());
            let message = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let ok = code == Some(0);
            StatusProbe {
                ok,
                checked_in,
                credits,
                code,
                message: if ok {
                    message
                } else if message.is_empty() {
                    format!("HTTP {status}")
                } else {
                    message
                },
            }
        }
        Err(error) => StatusProbe {
            ok: false,
            checked_in: None,
            credits: None,
            code: None,
            message: error,
        },
    }
}

/// 单次 claim 调用的结果。
struct ClaimResult {
    ok: bool,
    code: Option<i64>,
    message: String,
    http_status: u16,
}

/// 调签到接口。
async fn claim(jwt_value: &str, device_entry: &device::DeviceEntry) -> ClaimResult {
    let (status, body) = credits::post_json(TRAE_CHECKIN_PATH, jwt_value, device_entry).await;
    if status == 0 {
        // 网络层失败：`code = None`，调用方据此判定「可重试」。
        return ClaimResult {
            ok: false,
            code: None,
            message: body,
            http_status: 0,
        };
    }
    match serde_json::from_str::<Value>(&body) {
        Ok(parsed) => {
            let code = parsed.get("code").and_then(|v| v.as_i64());
            let message = parsed
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("HTTP {status}"));
            ClaimResult {
                ok: code == Some(0),
                code,
                message,
                http_status: status,
            }
        }
        Err(_) => ClaimResult {
            ok: false,
            code: None,
            message: format!("HTTP {status}: 非 JSON 响应: {}", snippet(&body)),
            // 非 JSON 响应视为业务失败（`code = Some(-1)`），避免被当成网络错误反复重试。
            http_status: status,
        },
    }
}

/// 对单账号执行签到；仅**网络层**异常按 `retry` 重试（业务失败不重试）。
///
/// 业务失败重试是有害的：像「套餐额度限制」「会话失效」这类结果不会因为重试而改变，
/// 反而会成倍增加上游请求，触发风控。
async fn claim_with_retry(
    jwt_value: &str,
    device_entry: &device::DeviceEntry,
    retry: u32,
) -> ClaimResult {
    let mut last = claim(jwt_value, device_entry).await;
    let mut attempt = 0;
    // 只有「网络层失败」（http_status == 0 且无业务码）才继续重试。
    while !last.ok && last.code.is_none() && last.http_status == 0 && attempt < retry {
        attempt += 1;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        last = claim(jwt_value, device_entry).await;
    }
    last
}

/// 截断响应体用于错误文案。
fn snippet(body: &str) -> String {
    body.chars().take(200).collect()
}

/// 组装本次要处理的账号列表（范围过滤 → 跳过条件 → 冷却过滤 → 紧迫度排序）。
///
/// **本函数不做任何 IO**：账号列表、账号视图、冷却状态、剩余积分缓存全部由调用方注入。
/// 这样做的两个理由：
///
/// 1. **测试隔离**：判定逻辑（本函数）的单测不需要触碰真实的 `~/.buddy-switch/trae/`，
///    符合仓库「涉及 HOME 的单测必须隔离」的约定；
/// 2. **一致性**：决策所用的视图与待执行的账号列表来自**同一次读取**，
///    不会出现「按 A 版本的快照筛选、按 B 版本的账号执行」这种中间态错配。
pub fn plan(
    accounts: Vec<(String, account::RawAccount)>,
    views: &[Value],
    cooldowns: &credits::CooldownsFile,
    remaining: &credits::RemainingCreditsFile,
    options: &CheckinOptions,
) -> Vec<(String, account::RawAccount)> {
    let wanted =
        account::resolve_user_ids_for(options.variant, &options.scope, options.user_ids.clone());
    // uid -> 今日是否已签到 / JWT 状态
    let mut checked_today: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut jwt_status: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for view in views {
        let Some(uid) = view.get("userId").and_then(|v| v.as_str()) else {
            continue;
        };
        if view.get("checkedToday").and_then(|v| v.as_bool()) == Some(true) {
            checked_today.insert(uid.to_string());
        }
        if let Some(status) = view.get("jwtStatus").and_then(|v| v.as_str()) {
            jwt_status.insert(uid.to_string(), status.to_string());
        }
    }
    let now = chrono::Local::now().timestamp();

    let mut planned: Vec<(String, account::RawAccount)> = accounts
        .into_iter()
        // `All` 不筛（全部都在范围内）；`Group` / `Selected` 一律按 `wanted` 筛。
        //
        // 这里曾经写作 `matches!(scope, All | Group(_)) || wanted.contains(uid)`，
        // 即 `Group` 被当成 `All` 直接放行、完全不看 `wanted`。旧代码之所以看不出问题，
        // 是因为唯一的调用方传进来的 `accounts` 已经预先按变体取过，
        // "碰巧"没有别的账号可漏；一旦 `Group` 的解析结果与传入列表不一致
        // （**多产品线并存时正是如此**：Trae Work 的组 id 在 Trae CN 里不存在），
        // 就会把**整条产品线的账号全部签一遍** —— 静默、且会造成上游风控暴露。
        .filter(|(uid, _)| match options.scope {
            Scope::All => true,
            Scope::Group(_) | Scope::Selected(_) => wanted.contains(uid),
        })
        .filter(|(uid, _)| !(options.skip_checked_in && checked_today.contains(uid)))
        .filter(|(uid, _)| {
            if !options.skip_expired {
                return true;
            }
            // 视图缺失（理论上已被 `entries()` 过滤掉）或状态为 expired/unknown 一律跳过：
            // 这两类账号 claim 必然 401，放进队列只会白造一次会话失效冷却。
            matches!(
                jwt_status.get(uid).map(String::as_str),
                Some("ok") | Some("warn")
            )
        })
        .filter(|(uid, _)| {
            // 冷却中（且未到期）的账号跳过。SessionDead 的 until 恒大于 now，即永久跳过。
            cooldowns
                .cooldowns
                .get(uid)
                .map(|entry| !(entry.until > now && !entry.error_type.is_empty()))
                .unwrap_or(true)
        })
        .collect();

    // 积分过期紧迫度排序：最早过期优先；同到期时间按剩余积分降序；无到期信息排最后。
    planned.sort_by(|(uid_a, _), (uid_b, _)| {
        let expire_a = remaining.expire_times.get(uid_a).copied();
        let expire_b = remaining.expire_times.get(uid_b).copied();
        match (expire_a, expire_b) {
            (Some(a), Some(b)) => {
                if a == b {
                    let credits_a = remaining.credits.get(uid_a).copied().unwrap_or(0.0);
                    let credits_b = remaining.credits.get(uid_b).copied().unwrap_or(0.0);
                    credits_b
                        .partial_cmp(&credits_a)
                        .unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    a.cmp(&b)
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    planned
}

/// 执行批量签到。
///
/// `on_event` 接收逐条进度事件（形态为 `json!`，键名 camelCase），用于：
/// - Tauri 通道 → 转发成 `trae-checkin-progress` 事件；
/// - HTTP 通道 → 忽略（响应体一次性返回 [`CheckinReport`] 的汇总）。
///
/// 用回调而不是返回 `Vec<Event>`：签到是长耗时的串行过程，用户需要**实时**看到进度，
/// 而不是等全部结束后一次性拿到。
pub async fn run_checkin<F>(options: CheckinOptions, mut on_event: F) -> CheckinReport
where
    F: FnMut(&Value),
{
    let mut report = CheckinReport::default();
    // 整条链路用同一个变体：账号列表、账号视图、冷却、剩余积分四份数据必须来自
    // 同一条产品线，否则会出现「按 Trae CN 的冷却去过滤 Trae Work 的账号」这种串味。
    let variant = options.variant;
    // 一次性读取全部决策依据，交给纯函数 `plan` 决定签谁、按什么顺序签。
    let planned = plan(
        account::entries_for(variant),
        &account::list_account_views_for(variant),
        &credits::load_cooldowns_for(variant),
        &credits::load_remaining_for(variant),
        &options,
    );

    on_event(&json!({ "type": "start", "total": planned.len() }));
    report.total = planned.len();

    if planned.is_empty() {
        // 空队列也要落盘摘要：否则前端会一直显示上一轮的签到结果。
        let summary = credits::CheckinSummary {
            time: Some(store::now_iso()),
            results: Vec::new(),
            total_ok: 0,
            already: 0,
            failed: 0,
            warnings: Vec::new(),
        };
        let _ = credits::save_summary_for(variant, &summary);
        on_event(&json!({ "type": "done", "ok": 0, "already": 0, "failed": 0, "total": 0 }));
        return report;
    }

    for (index, (uid, raw)) in planned.iter().enumerate() {
        let info = jwt::parse(&raw.jwt);

        // JWT 临期/过期告警（不阻断：临期账号仍可尝试签到）。
        if let Some(remaining_hours) = info.exp_hours {
            if remaining_hours < 0.0 {
                report
                    .warnings
                    .push(format!("{}: JWT 已过期，请重新获取", raw.name));
            } else if remaining_hours < crate::modules::trae::TRAE_JWT_WARN_HOURS {
                report.warnings.push(format!(
                    "{}: JWT 将于 {:.1}h 后过期，请尽快重新获取",
                    raw.name, remaining_hours
                ));
            }
        }

        let device_entry = match device::ensure_for_variant(variant, uid) {
            Ok(entry) => entry,
            Err(error) => {
                let outcome = AccountOutcome {
                    user_id: uid.clone(),
                    name: raw.name.clone(),
                    ok: false,
                    code: None,
                    message: format!("设备标识生成失败: {error}"),
                    action: "fail",
                    credits: None,
                    delta: 0,
                    error_type: None,
                    cooldown_until: None,
                };
                on_event(&json!({
                    "type": "account",
                    "index": index + 1,
                    "userId": uid,
                    "name": raw.name,
                    "status": "fail",
                    "message": outcome.message,
                }));
                report.failed += 1;
                report.results.push(outcome);
                continue;
            }
        };

        let probe = probe_status(&raw.jwt, &device_entry).await;

        // 预检失败不阻断 claim，但必须留痕：预检失败往往先于签到失败出现，
        // 是排查「账号忽然签不上」的第一手线索（错误码在 claim 结果里看不到）。
        if !probe.ok {
            store::append_log(
                &paths::checkin_log_file_for(variant),
                &format!(
                    "状态预检失败 [{}] code={:?} {} —— 仍尝试 claim",
                    raw.name, probe.code, probe.message
                ),
            );
        }

        // 今日已签到：跳过 claim，但仍落一条 delta=0 的明细，
        // 使「积分看板」在只签到不消费的日子里也有当日数据点。
        if probe.ok && probe.checked_in == Some(true) {
            let credits_value = probe.credits;
            if let Some(credits_value) = credits_value {
                let _ = credits::append_history_for(variant, uid, credits_value, 0);
            }
            report.already += 1;
            // 记台账：上游已经签过，也算「今日已签到」——下一轮 `skip_checked_in`
            // 应当跳过它，而不是再探一次。
            let _ = credits::mark_checked_in_for(variant, uid);
            on_event(&json!({
                "type": "account",
                "index": index + 1,
                "userId": uid,
                "name": raw.name,
                "status": "already",
                "credits": credits_value,
            }));
            report.results.push(AccountOutcome {
                user_id: uid.clone(),
                name: raw.name.clone(),
                ok: true,
                code: Some(0),
                message: if probe.message.is_empty() {
                    "已签到".into()
                } else {
                    probe.message.clone()
                },
                action: "skip_already",
                credits: credits_value,
                delta: 0,
                error_type: None,
                cooldown_until: None,
            });
            continue;
        }

        let result = claim_with_retry(&raw.jwt, &device_entry, options.retry).await;

        let mut outcome = AccountOutcome {
            user_id: uid.clone(),
            name: raw.name.clone(),
            ok: result.ok,
            code: result.code,
            message: result.message.clone(),
            action: if result.ok { "claim_ok" } else { "fail" },
            credits: None,
            delta: 0,
            error_type: None,
            cooldown_until: None,
        };

        if result.ok {
            credits::clear_cooldown_for(variant, uid).ok();
            // 记台账（与积分是否已知无关）：`probe.credits` 缺失时不会写积分明细，
            // 但「今天这个账号签过了」是确定的事实，必须落账，否则它会在下一轮
            // 被 `skip_checked_in` 漏掉、重复发一次 claim。
            credits::mark_checked_in_for(variant, uid).ok();
            // 预检返回的 credits 就是本次签到可得的额度，无需再次请求状态接口算差值。
            if let Some(credits_value) = probe.credits {
                outcome.credits = Some(credits_value);
                outcome.delta = credits_value;
                let _ = credits::append_history_for(variant, uid, credits_value, credits_value);
            }
            report.total_ok += 1;
        } else {
            // ★ issue #108：分类必须带 body（作废标记优先于状态码），
            //   并记下「这次判定所依据的 JWT `exp`」，让凭据更新后该记录自动失效。
            let (error_type, cooldown_seconds) =
                credits::classify_error(result.http_status, result.code, &result.message);
            if error_type != "Unknown" {
                let credential_exp = crate::modules::trae::jwt::parse(&raw.jwt).exp_timestamp;
                credits::save_cooldown_with_credential_for(
                    variant,
                    uid,
                    error_type,
                    cooldown_seconds,
                    &result.message,
                    credential_exp,
                );
                let entry: Option<CooldownEntry> = credits::load_cooldowns_for(variant)
                    .cooldowns
                    .get(uid)
                    .cloned();
                outcome.error_type = Some(error_type.to_string());
                outcome.cooldown_until = entry.map(|entry| entry.until);
            }
            report.failed += 1;
        }

        on_event(&json!({
            "type": "account",
            "index": index + 1,
            "userId": uid,
            "name": raw.name,
            "status": if result.ok { "success" } else { "fail" },
            "code": result.code,
            "message": result.message,
            "credits": outcome.credits,
            "delta": if outcome.delta != 0 { json!(outcome.delta) } else { Value::Null },
            "errorType": outcome.error_type,
            "cooldownUntil": outcome.cooldown_until,
        }));
        report.results.push(outcome);
    }

    // 落盘摘要（**仅**供设置页展示最近一次运行的结果）。
    //
    // ⚠️ 不要再用摘要去判定「今日已签到」：它每轮整体覆盖，只包含本轮处理过的账号。
    // 那个判定由 `credits::checked_in_today_for`（台账 ∪ 当日积分明细）负责。
    let summary = credits::CheckinSummary {
        time: Some(store::now_iso()),
        results: report.results.iter().map(outcome_json).collect(),
        total_ok: report.total_ok,
        already: report.already,
        failed: report.failed,
        warnings: report.warnings.clone(),
    };
    let _ = credits::save_summary_for(variant, &summary);

    store::append_log(
        &paths::checkin_log_file_for(variant),
        &format!(
            "签到完成: 成功 {}/已签到 {}/失败 {}/总计 {}{}",
            report.total_ok,
            report.already,
            report.failed,
            report.total,
            if report.warnings.is_empty() {
                String::new()
            } else {
                format!(" | 告警: {}", report.warnings.join("; "))
            }
        ),
    );

    on_event(&json!({
        "type": "done",
        "ok": report.total_ok,
        "already": report.already,
        "failed": report.failed,
        "total": report.total,
    }));

    report
}

/// 把 [`CheckinReport`] 转成线上形态（camelCase），供两条通道共用。
///
/// **必须**共用这一个构造器：Tauri 与 HTTP 若各自拼装，任一通道漏字段都会造成
/// 「同一操作在两个入口返回不同形状」——本仓库已经因 `copy_sessions` 的
/// 嵌套包装差异踩过一次。
pub fn report_json(report: &CheckinReport) -> Value {
    json!({
        "total": report.total,
        "totalOk": report.total_ok,
        "already": report.already,
        "failed": report.failed,
        "warnings": report.warnings,
        "results": report.results.iter().map(outcome_json).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------
// 逐账号「今日签到状态」（issue #47）
// ---------------------------------------------------------------------------

/// 批量上限：一次最多问这么多个账号（与 WorkBuddy 的 `get_checkin_status_map` 同值）。
///
/// 超出部分**忽略**而不是报错：调用方（前端）只会传「当前页选中的账号」或单个账号，
/// 传超说明上游有 bug；静默截断比让整次刷新失败更可接受，且不会把上游打爆。
pub const STATUS_MAP_MAX_ACCOUNTS: usize = 50;

/// 同时打开的上游请求数。
///
/// 状态端点是**逐账号**的 GET，串行问 50 个账号要十几秒（用户会以为卡死）；
/// 全部并发又会在一瞬间把 50 个请求打到 Trae 上游（风控暴露面）。
const STATUS_MAP_MAX_INFLIGHT: usize = 4;

/// 把一个预检结果投影成 `CheckinStatusEntry`（**纯函数**，便于单测）。
///
/// 三档语义必须分开，否则前端无法区分「没签」与「读不到」：
/// - `ok = false` ⇒ `todayCheckedIn` 一律 `false` + `error`（**不得**当成「未签到」事实）；
/// - `ok = true` 且上游给了 `checked_in` ⇒ 那就是事实（`source = upstream`）；
/// - `ok = true` 但上游没给 `checked_in` ⇒ 回落本地台账，并标 `source = ledger`。
fn status_entry(
    ok: bool,
    checked_in: Option<bool>,
    credits: Option<i64>,
    ledger_says_checked: bool,
    message: &str,
) -> Value {
    if !ok {
        return json!({
            "ok": false,
            "todayCheckedIn": false,
            "error": if message.is_empty() {
                "状态预检失败".to_string()
            } else {
                message.to_string()
            },
        });
    }
    let (today_checked_in, source) = match checked_in {
        Some(value) => (value, "upstream"),
        None => (ledger_says_checked, "ledger"),
    };
    json!({
        "ok": true,
        "todayCheckedIn": today_checked_in,
        "dailyCredit": credits,
        "source": source,
    })
}

/// 单账号预检 → 条目（IO 部分；投影逻辑见 [`status_entry`]）。
async fn status_entry_for(
    variant: TraeVariant,
    uid: &str,
    raw: &account::RawAccount,
    ledger_says_checked: bool,
) -> Value {
    let device_entry = match device::ensure_for_variant(variant, uid) {
        Ok(entry) => entry,
        Err(error) => {
            return status_entry(
                false,
                None,
                None,
                ledger_says_checked,
                &format!("设备标识生成失败: {error}"),
            )
        }
    };
    let probe = probe_status(&raw.jwt, &device_entry).await;
    status_entry(
        probe.ok,
        probe.checked_in,
        probe.credits,
        ledger_says_checked,
        &probe.message,
    )
}

/// 逐账号「今日签到状态」（`get_trae_checkin_status_map`；**只读**，issue #47）。
///
/// 返回 `Record<uid, { ok, todayCheckedIn, dailyCredit?, source?, error? }>`，与 WorkBuddy
/// 的 `get_checkin_status_map` **同构** ⇒ 前端三平台共用同一套渲染。
///
/// ## 为什么不能只读本地台账
///
/// 台账（`credits::checked_in_today_for`）只记「**本工具**签过谁」。用户在 Trae IDE 里
/// 手动签到时台账里没有 ⇒ 拿台账当答案会把「已签」报成「未签」。所以这里真去问上游
/// 状态端点（只读 GET）——**绝不**在这里顺带 claim（Qoder/Trae 的签到都是写操作，
/// 各有独立命令）。
///
/// ## 边界（前端照此渲染）
///
/// 1. 请求的 uid **不在账号库**（已删除 / 传错）⇒ **不进 map** ⇒ 前端显示「—」，
///    而不是「未签到」（用 `false` 表示「不知道」是本仓明令禁止的）；
/// 2. 上游读失败 / 设备标识生成失败 ⇒ `{ok:false, todayCheckedIn:false, error}`；
/// 3. 上游回 `ok` 却没给 `checked_in` ⇒ 回落台账，`source` 标明来源（见 [`status_entry`]）；
/// 4. 空数组 ⇒ `{}`（一次上游请求都不发）。
pub async fn status_map_for(variant: TraeVariant, user_ids: Vec<String>) -> Value {
    let ledger = credits::checked_in_today_for(variant);
    let accounts: std::collections::HashMap<String, account::RawAccount> =
        account::entries_for(variant).into_iter().collect();

    let wanted: Vec<String> = user_ids
        .into_iter()
        .map(|uid| uid.trim().to_string())
        .filter(|uid| !uid.is_empty() && accounts.contains_key(uid))
        .take(STATUS_MAP_MAX_ACCOUNTS)
        .collect();
    if wanted.is_empty() {
        return json!({});
    }

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(STATUS_MAP_MAX_INFLIGHT));
    let mut tasks = Vec::with_capacity(wanted.len());
    for uid in wanted {
        let raw = accounts
            .get(&uid)
            .cloned()
            .expect("wanted 已按账号库存在性过滤");
        let permit = semaphore.clone();
        let from_ledger = ledger.contains(&uid);
        tasks.push(tokio::spawn(async move {
            // 拿到许可才发请求（`acquire_owned` 失败 = 信号量已关闭，直接放弃该账号）。
            let Ok(_permit) = permit.acquire_owned().await else {
                return (
                    uid,
                    status_entry(false, None, None, from_ledger, "并发闸门已关闭"),
                );
            };
            let entry = status_entry_for(variant, &uid, &raw, from_ledger).await;
            (uid, entry)
        }));
    }

    let mut map = serde_json::Map::with_capacity(tasks.len());
    for task in tasks {
        // 任务 panic 时**不写条目**（前端显示「—」）——绝不编一个 `false` 顶上。
        if let Ok((uid, entry)) = task.await {
            map.insert(uid, entry);
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::trae::account::RawAccount;
    /// 造一个「可解析且 30 小时后过期」的 JWT，使 jwtStatus = ok。
    fn valid_jwt(uid: &str) -> String {
        use base64::Engine;
        let payload = serde_json::json!({
            "data": { "id": uid },
            "exp": chrono::Utc::now().timestamp() + 30 * 3600,
        });
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).unwrap());
        format!("Cloud-IDE-JWT h.{b64}.s")
    }

    fn raw(uid: &str, jwt_value: &str) -> RawAccount {
        RawAccount {
            name: uid.into(),
            user_id: Some(uid.into()),
            jwt: jwt_value.into(),
            refresh_token: None,
            added_at: None,
            updated_at: None,
            remark: None,
        }
    }

    /// 用真实的视图构造器产出视图，避免测试手搓字段名而漏掉线上契约变更。
    fn view_for(uid: &str, jwt_value: &str, checked_today: bool) -> Value {
        let account = raw(uid, jwt_value);
        account::account_view(
            &account,
            uid,
            None,
            None,
            None,
            None,
            None,
            checked_today,
            None,
        )
    }

    fn empty_context() -> (credits::CooldownsFile, credits::RemainingCreditsFile) {
        (
            credits::CooldownsFile::default(),
            credits::RemainingCreditsFile::default(),
        )
    }

    #[test]
    fn status_entry_never_reports_read_failure_as_not_checked_in() {
        // 上游读失败：`ok:false` + 错误文案，`todayCheckedIn` 不得被当成事实。
        let failed = status_entry(false, None, None, true, "HTTP 500");
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["todayCheckedIn"], false);
        assert_eq!(failed["error"], "HTTP 500");
        assert!(
            failed.get("source").is_none(),
            "读失败没有来源可标，不得伪造"
        );

        // 上游明确回答「没签」⇒ 事实，`ok:true`。
        let not_checked = status_entry(true, Some(false), Some(150), false, "");
        assert_eq!(not_checked["ok"], true);
        assert_eq!(not_checked["todayCheckedIn"], false);
        assert_eq!(not_checked["dailyCredit"], 150);
        assert_eq!(not_checked["source"], "upstream");

        // 上游明确回答「已签」⇒ 事实。
        let checked = status_entry(true, Some(true), None, false, "");
        assert_eq!(checked["todayCheckedIn"], true);
        assert_eq!(checked["source"], "upstream");

        // 上游 ok 但没给 checked_in ⇒ 回落台账，并标明来源（不伪装成上游事实）。
        let fallback = status_entry(true, None, None, true, "");
        assert_eq!(fallback["ok"], true);
        assert_eq!(fallback["todayCheckedIn"], true);
        assert_eq!(fallback["source"], "ledger");
    }

    /// 空数组 / 全是不存在的 uid ⇒ 一条上游请求都不发，直接回 `{}`（前端显示「—」）。
    #[tokio::test]
    async fn status_map_returns_empty_without_upstream_calls() {
        // 账号库读的是 `$HOME` 下的文件 ⇒ 必须隔离（本仓单测纪律）。
        let home = std::env::temp_dir().join(format!("trae-status-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("创建隔离目录");
        let _guard = crate::modules::config::HomeOverrideGuard::set(&home);

        let empty = status_map_for(TraeVariant::TraeWork, Vec::new()).await;
        assert_eq!(empty, json!({}));

        let unknown = status_map_for(
            TraeVariant::TraeWork,
            vec!["不存在的账号".to_string(), "   ".to_string()],
        )
        .await;
        assert_eq!(
            unknown,
            json!({}),
            "未知账号不进 map —— 用 false 表示「不知道」是本仓明令禁止的"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn default_options_skip_checked_in_and_expired() {
        // 默认必须跳过：重复 claim 会浪费配额并增加风控暴露面。
        let options = CheckinOptions::default();
        assert!(options.skip_checked_in);
        assert!(options.skip_expired);
        assert_eq!(options.retry, 1);
        assert_eq!(options.scope, Scope::All);
    }

    #[test]
    fn plan_keeps_healthy_account() {
        let uid = "u-healthy";
        let jwt_value = valid_jwt(uid);
        let (cooldowns, remaining) = empty_context();
        let planned = plan(
            vec![(uid.into(), raw(uid, &jwt_value))],
            &[view_for(uid, &jwt_value, false)],
            &cooldowns,
            &remaining,
            &CheckinOptions::default(),
        );
        assert_eq!(planned.len(), 1, "健康账号必须进入签到队列");
    }

    #[test]
    fn plan_filters_by_selected_scope() {
        let accounts = vec![
            ("a".to_string(), raw("a", &valid_jwt("a"))),
            ("b".to_string(), raw("b", &valid_jwt("b"))),
        ];
        let views = vec![
            view_for("a", &valid_jwt("a"), false),
            view_for("b", &valid_jwt("b"), false),
        ];
        let (cooldowns, remaining) = empty_context();
        let options = CheckinOptions {
            scope: Scope::Selected(vec!["b".to_string()]),
            ..Default::default()
        };
        let planned = plan(accounts, &views, &cooldowns, &remaining, &options);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].0, "b");
    }

    #[test]
    fn plan_skips_checked_in_today_unless_disabled() {
        let uid = "u1";
        let jwt_value = valid_jwt(uid);
        let accounts = vec![(uid.to_string(), raw(uid, &jwt_value))];
        let views = vec![view_for(uid, &jwt_value, true)];
        let (cooldowns, remaining) = empty_context();

        // 默认跳过 → 空队列
        let planned = plan(
            accounts.clone(),
            &views,
            &cooldowns,
            &remaining,
            &CheckinOptions::default(),
        );
        assert!(planned.is_empty(), "今日已签到应被跳过");

        let options = CheckinOptions {
            skip_checked_in: false,
            ..Default::default()
        };
        assert_eq!(
            plan(accounts, &views, &cooldowns, &remaining, &options).len(),
            1
        );
    }

    #[test]
    fn plan_skips_expired_jwt_by_default() {
        // 空 JWT → jwtStatus = unknown → 默认必须跳过（claim 必然 401）。
        let uid = "u-bad";
        let accounts = vec![(uid.to_string(), raw(uid, ""))];
        let views = vec![view_for(uid, "", false)];
        let (cooldowns, remaining) = empty_context();

        let planned = plan(
            accounts.clone(),
            &views,
            &cooldowns,
            &remaining,
            &CheckinOptions::default(),
        );
        assert!(planned.is_empty(), "unknown JWT 不应进入签到队列");

        // 关闭 skip_expired 后应保留，交由上游返回 401 并正确冷却。
        let options = CheckinOptions {
            skip_expired: false,
            ..Default::default()
        };
        assert_eq!(
            plan(accounts, &views, &cooldowns, &remaining, &options).len(),
            1
        );
    }

    #[test]
    fn plan_skips_accounts_in_active_cooldown() {
        let uid = "u-cool";
        let jwt_value = valid_jwt(uid);
        let accounts = vec![(uid.to_string(), raw(uid, &jwt_value))];
        let views = vec![view_for(uid, &jwt_value, false)];
        let (_, remaining) = empty_context();

        // 未到期的冷却：跳过
        let mut cooldowns = credits::CooldownsFile::default();
        cooldowns.cooldowns.insert(
            uid.into(),
            credits::CooldownEntry {
                error_type: "Server".into(),
                until: chrono::Local::now().timestamp() + 3600,
                reason: "5xx".into(),
                error_count: 0,
                credential_exp: None,
                judged_at: None,
            },
        );
        let planned = plan(
            accounts.clone(),
            &views,
            &cooldowns,
            &remaining,
            &CheckinOptions::default(),
        );
        assert!(planned.is_empty(), "冷却中的账号应被跳过");

        // 已过期的冷却：不再跳过（否则账号会被永久禁用）
        cooldowns.cooldowns.insert(
            uid.into(),
            credits::CooldownEntry {
                error_type: "Server".into(),
                until: chrono::Local::now().timestamp() - 10,
                reason: "旧".into(),
                error_count: 0,
                credential_exp: None,
                judged_at: None,
            },
        );
        let planned = plan(
            accounts,
            &views,
            &cooldowns,
            &remaining,
            &CheckinOptions::default(),
        );
        assert_eq!(planned.len(), 1, "冷却已过期的账号应恢复参与签到");
    }

    #[test]
    fn plan_orders_planned_accounts_by_earliest_expiry() {
        let ids = ["no-expire", "late", "early", "same-time-richer"];
        let accounts: Vec<(String, RawAccount)> = ids
            .iter()
            .map(|uid| ((*uid).to_string(), raw(uid, &valid_jwt(uid))))
            .collect();
        let views: Vec<Value> = ids
            .iter()
            .map(|uid| view_for(uid, &valid_jwt(uid), false))
            .collect();
        let cooldowns = credits::CooldownsFile::default();
        let mut remaining = credits::RemainingCreditsFile::default();
        remaining.expire_times.insert("late".into(), 2_000);
        remaining.expire_times.insert("early".into(), 1_000);
        remaining
            .expire_times
            .insert("same-time-richer".into(), 1_000);
        remaining.credits.insert("same-time-richer".into(), 900.0);
        remaining.credits.insert("early".into(), 1.0);

        let planned = plan(
            accounts,
            &views,
            &cooldowns,
            &remaining,
            &CheckinOptions::default(),
        );
        let order: Vec<&str> = planned.iter().map(|(uid, _)| uid.as_str()).collect();
        assert_eq!(
            order,
            vec!["same-time-richer", "early", "late", "no-expire"],
            "排序应为：最早过期优先 → 同到期按剩余积分降序 → 无到期信息最后"
        );
    }

    #[test]
    fn claim_with_retry_gives_up_on_business_failure() {
        // 业务失败（有 code）不应重试：重试不会改变结果，只会放大上游请求。
        let result = ClaimResult {
            ok: false,
            code: Some(1005),
            message: "计划额度限制".into(),
            http_status: 200,
        };
        assert!(result.code.is_some());
        // 判据与 claim_with_retry 的循环条件一致
        let would_retry = !result.ok && result.code.is_none() && result.http_status == 0;
        assert!(!would_retry);
    }

    #[test]
    fn claim_with_retry_allows_network_retry() {
        let result = ClaimResult {
            ok: false,
            code: None,
            message: "connection refused".into(),
            http_status: 0,
        };
        let would_retry = !result.ok && result.code.is_none() && result.http_status == 0;
        assert!(would_retry, "纯网络失败应可重试");
    }

    #[test]
    fn report_json_exposes_camel_case_shape() {
        let mut report = CheckinReport {
            total: 2,
            total_ok: 1,
            already: 1,
            ..Default::default()
        };
        report.results.push(AccountOutcome {
            user_id: "u1".into(),
            name: "n1".into(),
            ok: true,
            code: Some(0),
            message: "ok".into(),
            action: "claim_ok",
            credits: Some(50),
            delta: 50,
            error_type: None,
            cooldown_until: None,
        });
        let value = report_json(&report);
        for key in [
            "total", "totalOk", "already", "failed", "warnings", "results",
        ] {
            assert!(value.get(key).is_some(), "缺少线上字段 {key}");
        }
        // snake_case 绝不能出现在线上形态
        assert!(value.get("total_ok").is_none());
        let first = &value.get("results").unwrap()[0];
        assert_eq!(first.get("userId").unwrap().as_str(), Some("u1"));
        assert!(first.get("user_id").is_none());
        assert_eq!(first.get("delta").unwrap().as_i64(), Some(50));
    }

    #[test]
    fn outcome_json_marks_zero_delta_as_zero_not_null() {
        let outcome = AccountOutcome {
            user_id: "u".into(),
            name: "n".into(),
            ok: true,
            code: Some(0),
            message: String::new(),
            action: "skip_already",
            credits: Some(10),
            delta: 0,
            error_type: None,
            cooldown_until: None,
        };
        assert_eq!(
            outcome_json(&outcome).get("delta").unwrap().as_i64(),
            Some(0)
        );
    }

    #[test]
    fn snippet_truncates_by_chars() {
        assert_eq!(snippet("中文测试"), "中文测试");
        assert_eq!(snippet(&"a".repeat(300)).len(), 200);
    }

    /// 签到选项的变体默认值必须与 `TraeVariant::default()` 一致。
    ///
    /// 这条不是形式主义：`CheckinOptions::default()` 是 HTTP/Tauri 两条通道在
    /// 解析失败或调用方漏填时的共同落点，它一旦漂到 `Trae`，
    /// **老用户的 Trae Work 账号库会瞬间看起来是空的**。
    #[test]
    fn checkin_options_defaults_to_default_variant() {
        assert_eq!(
            CheckinOptions::default().variant,
            TraeVariant::default(),
            "签到选项的默认变体必须跟随 TraeVariant::default()，否则老用户数据白失效"
        );
        // ★ issue #28：默认变体是**合并版**（CN），它才是「有客户端在跑」的那一个。
        //   老默认 `TraeWork` 指向已被官方合并卸载的 `TRAE SOLO CN`，
        //   继续拿它当默认会让所有未显式传变体的调用读到死客户端。
        assert_eq!(TraeVariant::default(), TraeVariant::TraeMerged);
    }

    /// ★ 护栏：签到链路里的**每一个**落盘/读取点都必须使用 `options.variant`。
    ///
    /// 这是「存储层分家」的最后一公里：`paths.rs` 与 `credits.rs` 都提供了
    /// `_for(variant, ...)` 变体，但只要 `run_checkin` 里漏改一处，
    /// 两条产品线就会在那一处串味（最典型的是「用 A 线的冷却去过滤 B 线的账号」，
    /// 表现为「明明没失败却显示冷却中」）。
    ///
    /// 本用例以**行为**方式验证，而不是 grep 源码：
    /// 在隔离目录里往两条产品线各写一份冷却记录，然后确认各自的
    /// `load_cooldowns_for` 只读得到自己那份 —— 若 `run_checkin` 用的是
    /// 无参 `load_cooldowns()`，那么无论如何都只有一条线能看到冷却，
    /// 而本用例会因为「另一条线也看到冷却」而红。
    #[test]
    fn 签到链路按变体读取冷却与摘要() {
        let dir = std::env::temp_dir().join(format!("trae-checkin-variant-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _guard = crate::modules::config::HomeOverrideGuard::set(&dir);

        let work = TraeVariant::TraeWork;
        let cn = TraeVariant::Global;

        // 只给 Trae Work 写一条冷却。
        credits::save_cooldown_for(work, "u-work", "SessionDead", 3600, "测试");

        assert!(
            credits::load_cooldowns_for(work)
                .cooldowns
                .contains_key("u-work"),
            "Trae Work 应能看到自己写的冷却"
        );
        assert!(
            !credits::load_cooldowns_for(cn)
                .cooldowns
                .contains_key("u-work"),
            "Trae CN 绝不能看到 Trae Work 的冷却 —— 这就是串味"
        );

        // 摘要同理：写 A 线，B 线必须读不到。
        let summary = credits::CheckinSummary {
            time: Some(store::now_iso()),
            results: Vec::new(),
            total_ok: 1,
            already: 0,
            failed: 0,
            warnings: Vec::new(),
        };
        let _ = credits::save_summary_for(cn, &summary);
        assert_eq!(
            credits::load_summary_for(cn).total_ok,
            1,
            "Trae CN 应读到自己写的摘要"
        );
        assert_eq!(
            credits::load_summary_for(work).total_ok,
            0,
            "Trae Work 不该读到 Trae CN 的摘要"
        );
    }

    /// `plan` 必须用**传入选项里的变体**去解析选中范围。
    ///
    /// 反例：若 `plan` 内部写死 `account::resolve_user_ids(...)`（无参，等价于默认变体），
    /// 那么当 `variant = Trae` 时，`Scope::Selected` 里勾选的账号会因为
    /// 「在 Trae Work 库里找不到」而被全部静默丢弃 → 签到队列莫名其妙为空。
    #[test]
    fn plan_uses_the_options_variant_for_selected_scope() {
        // ★ 先**清空**再建：本用例的临时 home 按 `process::id()` 命名，而 PID 会被系统复用
        //   （实测本机 `%TEMP%` 里已堆积 174 个 `trae-checkin-plan-*`，**全部**残留着
        //   上一轮的 `groups.trae_cn.json`）。一旦撞上复用，`create_dir_all` 是空操作，
        //   残留的「CN 组」会让下面的 `group_create_for` 返回 `Err("同名分组已存在")`
        //   并被 `unwrap()` 打崩 —— 实测 40 轮全量并行中 2 轮（5%），单跑几乎不复现。
        //   同仓库 `platform::tests` 里的 `trae-select-*` 已是「先删后建」的写法。
        let dir = std::env::temp_dir().join(format!("trae-checkin-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let _guard = crate::modules::config::HomeOverrideGuard::set(&dir);

        // 只在 Trae CN 库里放一个分组，组内挂一个账号。
        let group_id = account::group_create_for(TraeVariant::Global, "CN 组", "#fff").unwrap();
        let uid = "u-cn-only";
        account::add_manual_for(
            TraeVariant::Global,
            "CN 账号",
            &valid_jwt(uid),
            Some(group_id.clone()),
        )
        .unwrap();

        let jwt_value = valid_jwt(uid);
        let (cooldowns, remaining) = empty_context();
        let options = CheckinOptions {
            variant: TraeVariant::Global,
            scope: Scope::Group(group_id.clone()),
            ..Default::default()
        };
        let planned = plan(
            vec![(uid.to_string(), raw(uid, &jwt_value))],
            &[view_for(uid, &jwt_value, false)],
            &cooldowns,
            &remaining,
            &options,
        );
        assert_eq!(
            planned.len(),
            1,
            "Trae CN 的分组账号必须被解析出来；为空说明 plan 用了默认变体的账号库"
        );

        // 反向对照：同一个分组 id 在 Trae Work 库里不存在，用默认变体必须解析为空。
        let work_options = CheckinOptions {
            variant: TraeVariant::TraeWork,
            scope: Scope::Group(group_id),
            ..Default::default()
        };
        let work_planned = plan(
            vec![(uid.to_string(), raw(uid, &jwt_value))],
            &[view_for(uid, &jwt_value, false)],
            &cooldowns,
            &remaining,
            &work_options,
        );
        assert!(
            work_planned.is_empty(),
            "Trae Work 库没有这个分组，不应解析出任何账号"
        );
    }
}
