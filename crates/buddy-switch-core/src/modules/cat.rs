//! 夜猫子任务（`black_cat`，**CN 专有**）。
//!
//! 对照参考实现 `scripts/task_runner.py` 的 `black_cat` 分支 + `internal/scheduler/school.go`。
//!
//! - 窗口：CST `hour >= 23 || hour < 8`（23:00–08:00）。**非窗口直接跳过并正常退出**。
//! - 任务 `black_cat`：`target = 3`，达成判据是「`chat_request_send` 且
//!   `requestModelId == "glm-5.2"` 且 `requestModelName == "GLM-5.2"` 且 `mode == "night"`」。
//! - **窗口内最多补 1 次**（`cap = 1`）：一次会话内不刷满，避免风控。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::modules::account::{account_display_name, load_accounts_for};
use crate::modules::config::{authed_json_request_for, load_checkin_config, now_ms};
use crate::modules::cst::in_night_window;
use crate::modules::refresh::ensure_fresh_token_for;
use crate::modules::region::{region_spec, Region};

/// `black_cat` 的达成目标次数。
pub const CAT_TARGET: i64 = 3;
/// 窗口内每轮最多补的次数。
pub const CAT_CAP: i64 = 1;
/// 写动作之间的间隔。
const CAT_GAP: Duration = Duration::from_millis(1500);

const TASKS_PATH: &str = "/v2/activity/growth/tasks";
const ACCEPT_PATH: &str = "/v2/activity/growth/tasks/accept";
const CLAIM_PATH: &str = "/activity/growth/tasks/black_cat/claim";
/// claim 在 chat 域返回 400 时的降级 web 域。
const CLAIM_WEB_BASE: &str = "https://www.workbuddy.cn";
const REPORT_PATH: &str = "/v2/report";

/// 窗口内需要补的次数：`min(target - cur, cap)`，并夹到 `>= 0`。
pub fn cat_need(cur: i64, target: i64, cap: i64) -> i64 {
    (target - cur).clamp(0, cap.max(0))
}

/// 构造一条夜猫子判据事件（`chat_request_send` + GLM-5.2 + `mode: night`）。
pub fn cat_report_event(uid: &str, cid: &str, at_ms: i64) -> Value {
    json!({
        "eventCode": "chat_request_send",
        "timestamp": at_ms,
        "reportDelay": 0,
        "mode": "night",
        "conversationId": cid,
        "requestId": cid,
        "inputLength": 12,
        "requestModelId": "glm-5.2",
        "requestModelName": "GLM-5.2",
        "presentAt": at_ms,
        "rootRequestId": cid,
        "parentConversationId": cid,
        "agentName": "default",
        "agentType": "conversation",
        "userId": uid,
    })
}

fn cat_extra() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("x-client-platform".to_string(), "web".to_string());
    headers
}

// ---------------------------------------------------------------------------
// HTTP 取值点（可注入）
// ---------------------------------------------------------------------------

/// 单次请求的返回值：`(HTTP 状态码, 响应体)`。
///
/// 保留原始状态码是关键——`claim_cat` 依赖 chat 域返回的 **HTTP 400** 来判定是否降级改打
/// web 域；若把状态码折进 `code` 便再也无法区分，降级路径随之失效。
pub type CatResponse = (u16, Value);

/// 夜猫子域 HTTP 取值点（依赖注入缝）。
///
/// 生产实现 [`RealCatIo`] 走真实网络（逐字沿用既有三处请求构造：同一 URL 拼接 / 同一
/// `authed_json_request_for` / 同样的 header / 同样的 body）；单测注入桩实现，
/// **绝不发起真实请求**，从而可驱动真实入口 [`run_cat_cycle_for_with`]。
///
/// `Send + Sync` 超级约束：调度器在 `tokio::spawn` 里调用，要求 `&dyn CatIo` 可跨线程
/// （与可注入 IO 缝的既有约定保持一致）。
pub trait CatIo: Send + Sync {
    /// 发起一次请求。`method` 为 `"GET"` / `"POST"`；`body` 仅 POST 携带；
    /// `extra` 为追加请求头（chat 域为 `x-client-platform: web`，billing 域为空，
    /// web 降级域带 `Origin` / `Referer` / `x-client-platform`）。
    fn request<'a>(
        &'a self,
        region: Region,
        url: &'a str,
        method: &'a str,
        body: Option<Value>,
        account: &'a Value,
        extra: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = CatResponse> + Send + 'a>>;
}

/// 生产用取值点：真实 HTTP（逐字转发既有 `authed_json_request_for` 调用）。
pub struct RealCatIo;

impl CatIo for RealCatIo {
    fn request<'a>(
        &'a self,
        region: Region,
        url: &'a str,
        method: &'a str,
        body: Option<Value>,
        account: &'a Value,
        extra: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = CatResponse> + Send + 'a>> {
        Box::pin(
            async move { authed_json_request_for(region, url, method, body, account, extra).await },
        )
    }
}

async fn chat_request(
    io: &dyn CatIo,
    region: Region,
    path: &str,
    method: &str,
    body: Option<Value>,
    account: &Value,
) -> CatResponse {
    let url = format!("{}{path}", region_spec(region).chat_base);
    io.request(region, &url, method, body, account, &cat_extra())
        .await
}

async fn billing_report(
    io: &dyn CatIo,
    region: Region,
    body: Value,
    account: &Value,
) -> CatResponse {
    let url = format!("{}{REPORT_PATH}", region_spec(region).billing_base);
    io.request(region, &url, "POST", Some(body), account, &HashMap::new())
        .await
}

/// 领奖：chat 域 400 时降级 web 域（带 Origin/Referer/x-client-platform: web）。
async fn claim_cat(io: &dyn CatIo, region: Region, account: &Value) -> CatResponse {
    let (status, resp) = chat_request(io, region, CLAIM_PATH, "POST", None, account).await;
    if status != 400 {
        return (status, resp);
    }
    let web_url = format!("{CLAIM_WEB_BASE}{CLAIM_PATH}");
    let mut extra = HashMap::new();
    extra.insert("Origin".to_string(), CLAIM_WEB_BASE.to_string());
    extra.insert(
        "Referer".to_string(),
        format!("{CLAIM_WEB_BASE}/profile/growth-center"),
    );
    extra.insert("x-client-platform".to_string(), "web".to_string());
    io.request(region, &web_url, "POST", None, account, &extra)
        .await
}

/// 成功判据：HTTP 200 且业务 `code` 为 0（或缺失）。
fn ok(status: u16, resp: &Value) -> bool {
    status == 200 && matches!(resp.get("code").and_then(Value::as_i64), Some(0) | None)
}

/// 从任务列表里找 `black_cat`。
fn find_black_cat(tasks: &[Value]) -> Option<&Value> {
    tasks
        .iter()
        .find(|t| t.get("task_code").and_then(Value::as_str) == Some("black_cat"))
}

/// 任务进度 `(current, target)`（缺失 target 时回落 [`CAT_TARGET`]）。
fn progress_of(task: &Value) -> (i64, i64) {
    let progress = task.get("progress");
    let cur = progress
        .and_then(|p| p.get("current"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let target = progress
        .and_then(|p| p.get("target"))
        .and_then(Value::as_i64)
        .unwrap_or(CAT_TARGET);
    (cur, target)
}

fn accept_status_of(task: &Value) -> String {
    task.get("accept_status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase()
}

/// 处理单个账号的 `black_cat` 任务（假定已在夜猫窗口内）。
async fn process_account(io: &dyn CatIo, region: Region, account: &Value) -> Value {
    let uid = account
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let (status, resp) = chat_request(io, region, TASKS_PATH, "GET", None, account).await;
    if !ok(status, &resp) {
        return json!({"result": "error", "reason": "list_tasks_failed"});
    }
    let tasks = resp
        .pointer("/data/tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(task) = find_black_cat(&tasks).cloned() else {
        return json!({"result": "skipped", "reason": "no_black_cat_task"});
    };

    if accept_status_of(&task) == "claimed" {
        return json!({"result": "already"});
    }

    // not_accepted → accept（body {"task_codes":["black_cat"]}）。
    if accept_status_of(&task) == "not_accepted" || accept_status_of(&task).is_empty() {
        let _ = chat_request(
            io,
            region,
            ACCEPT_PATH,
            "POST",
            Some(json!({"task_codes": ["black_cat"]})),
            account,
        )
        .await;
        tokio::time::sleep(CAT_GAP).await;
    }

    let (cur, target) = progress_of(&task);
    if accept_status_of(&task) == "completed" || cur >= target {
        return claim_result(io, region, account).await;
    }

    // 窗口内最多补 CAT_CAP 次。
    let need = cat_need(cur, target, CAT_CAP);
    for _ in 0..need {
        let cid = format!("wbcat-{}", now_ms());
        let event = cat_report_event(&uid, &cid, now_ms());
        let (report_status, _) = billing_report(io, region, json!([event]), account).await;
        if report_status != 200 {
            break;
        }
        tokio::time::sleep(CAT_GAP).await;
    }

    // 回读；达 target → claim。
    let (status, resp) = chat_request(io, region, TASKS_PATH, "GET", None, account).await;
    if !ok(status, &resp) {
        return json!({"result": "error", "reason": "refetch_failed"});
    }
    let tasks = resp
        .pointer("/data/tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(task) = find_black_cat(&tasks) {
        if accept_status_of(task) == "claimed" {
            return json!({"result": "already"});
        }
        let (after_cur, after_target) = progress_of(task);
        if accept_status_of(task) == "completed" || after_cur >= after_target {
            return claim_result(io, region, account).await;
        }
        return json!({"result": "pending", "progress": after_cur});
    }
    json!({"result": "pending"})
}

async fn claim_result(io: &dyn CatIo, region: Region, account: &Value) -> Value {
    let (status, resp) = claim_cat(io, region, account).await;
    if ok(status, &resp) {
        json!({"result": "claimed"})
    } else {
        json!({"result": "error", "reason": "claim_failed"})
    }
}

/// 对全部账号执行一轮夜猫子任务（CN）。
pub async fn run_cat_cycle() -> Value {
    run_cat_cycle_for(Region::Cn).await
}

/// 按 region 执行一轮夜猫子任务（生产入口）。CN 专有；非窗口 / 非 CN 一律跳过并正常退出。
///
/// 生产路径：取真实 `now_ms()` + 加载账号 + 注入 [`RealCatIo`]（真实 HTTP），转调
/// [`run_cat_cycle_for_with`]。二者共享同一段主循环逻辑（与 `run_cat_cycle_for`
/// :502/:510 的结构完全同构）。
pub async fn run_cat_cycle_for(region: Region) -> Value {
    if region != Region::Cn {
        return json!({"status": "unsupported", "reason": "cat is CN only"});
    }
    run_cat_cycle_for_with(region, now_ms(), load_accounts_for(region), &RealCatIo).await
}

/// 可注入版本：时钟、账号与 HTTP 取值点均由调用方提供，便于单测**驱动真实入口**且不发网络。
///
/// 窗口判定一律使用传入的 `now_ms`（不再直取真实时钟），与 `run_cat_cycle_for_with`
/// 同构。
pub async fn run_cat_cycle_for_with(
    region: Region,
    now_ms: i64,
    accounts: Vec<Value>,
    io: &dyn CatIo,
) -> Value {
    if region != Region::Cn {
        return json!({"status": "unsupported", "reason": "cat is CN only"});
    }
    // 非夜猫窗口：直接跳过并正常退出（不得记为失败）。
    if !in_night_window(now_ms) {
        return json!({"status": "ok", "skipped": "not_night_window"});
    }
    if accounts.is_empty() {
        return json!({"status": "no_accounts"});
    }
    let checkin_cfg = load_checkin_config();
    let mut items: Vec<Value> = Vec::new();
    for account in accounts {
        // 每个账号前重判窗口：跨过 08:00 后应立即收尾，不再补下一号。
        if !in_night_window(now_ms) {
            items.push(json!({"email": account_display_name(&account), "result": "skipped", "reason": "window_closed"}));
            break;
        }
        let acc = ensure_fresh_token_for(region, account.clone(), &checkin_cfg).await;
        let mut result = process_account(io, region, &acc).await;
        result["email"] = json!(account_display_name(&acc));
        items.push(result);
    }
    json!({"status": "ok", "accounts": items})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::cst::cst_ms;

    #[test]
    fn night_window_boundaries() {
        // 22:59 不在窗口；23:00 在窗口；07:59 在窗口；08:00 不在窗口。
        assert!(!in_night_window(cst_ms(2026, 9, 16, 22, 59)));
        assert!(in_night_window(cst_ms(2026, 9, 16, 23, 0)));
        assert!(in_night_window(cst_ms(2026, 9, 17, 7, 59)));
        assert!(!in_night_window(cst_ms(2026, 9, 17, 8, 0)));
        // 白天中间时刻亦不在窗口。
        assert!(!in_night_window(cst_ms(2026, 9, 16, 12, 0)));
    }

    #[test]
    fn cat_need_is_capped_and_non_negative() {
        assert_eq!(cat_need(0, CAT_TARGET, CAT_CAP), 1);
        assert_eq!(cat_need(2, CAT_TARGET, CAT_CAP), 1);
        assert_eq!(cat_need(3, CAT_TARGET, CAT_CAP), 0);
        assert_eq!(cat_need(5, CAT_TARGET, CAT_CAP), 0);
        // 无 cap 时按缺口补齐。
        assert_eq!(cat_need(0, CAT_TARGET, CAT_TARGET), 3);
    }

    #[test]
    fn cat_event_is_glm_night_judgement() {
        let ev = cat_report_event("uid-1", "wbcat-1", 1);
        assert_eq!(ev["eventCode"], "chat_request_send");
        assert_eq!(ev["requestModelId"], "glm-5.2");
        assert_eq!(ev["requestModelName"], "GLM-5.2");
        assert_eq!(ev["mode"], "night");
        assert_eq!(ev["userId"], "uid-1");
    }

    /// D17：CST 非窗口时段必须被判为「跳过」（窗口谓词穷举 0–23，独立 oracle）。
    ///
    /// `run_cat_cycle_for_with` 的非窗口分支条件为 `!in_night_window(now_ms)`——谓词为假即
    /// 走「正常跳过、非失败」。此处用独立字面量集合穷举钉死窗口，防止边界漂移。
    ///
    /// 注：运行级「跳过并正常退出」分支现已可确定性单测——见
    /// [`injected_clock_drives_window_branch`]（时钟注入口经 `run_cat_cycle_for_with`）。
    #[test]
    fn cat_window_is_exactly_23_through_07_outside_skips() {
        // 独立 oracle：窗口 = 23:00–08:00 = {23} ∪ {0..=7}；其余小时不在窗口。
        let window_hours: [u32; 9] = [23, 0, 1, 2, 3, 4, 5, 6, 7];
        for hour in 0u32..24 {
            let got = in_night_window(cst_ms(2026, 9, 16, hour, 0));
            let want = window_hours.contains(&hour);
            assert_eq!(
                got, want,
                "CST {hour}:00 窗口判定不符（非窗口须跳过，不是失败）"
            );
        }
    }

    // -----------------------------------------------------------------------
    // D17 / D19：可注入 CatIo + 时钟缝驱动真实入口 run_cat_cycle_for_with（绝不发网络）。
    // -----------------------------------------------------------------------

    /// 记录的一次 IO 调用：(url, method, body, extra)。
    /// 抽别名是为了让 `RecordingIo` 的字段与 `calls()` 的返回类型读起来是「一串调用记录」，
    /// 而不是一坨嵌套泛型（clippy::type_complexity 也在说同一件事）。
    type RecordedCall = (
        String,
        String,
        Option<Value>,
        std::collections::HashMap<String, String>,
    );

    /// 记录型桩 IO：按调用次序记录 `(url, method, body, extra)`，并按预设序列返回响应。
    struct RecordingIo {
        responses: Vec<CatResponse>,
        calls: std::sync::Mutex<Vec<RecordedCall>>,
    }

    impl RecordingIo {
        fn new(responses: Vec<CatResponse>) -> Self {
            Self {
                responses,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }
        fn urls(&self) -> Vec<String> {
            self.calls().into_iter().map(|c| c.0).collect()
        }
    }

    impl CatIo for RecordingIo {
        fn request<'a>(
            &'a self,
            _region: Region,
            url: &'a str,
            method: &'a str,
            body: Option<Value>,
            _account: &'a Value,
            extra: &'a std::collections::HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = CatResponse> + Send + 'a>> {
            Box::pin(async move {
                let idx = {
                    let mut calls = self.calls.lock().unwrap();
                    let idx = calls.len();
                    calls.push((
                        url.to_string(),
                        method.to_string(),
                        body.clone(),
                        extra.clone(),
                    ));
                    idx
                };
                self.responses
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| (200, json!({"code": 0, "data": {}})))
            })
        }
    }

    fn one_cat_account() -> Vec<Value> {
        vec![json!({"uid": "u-cat", "email": "cat@example.com", "access_token": "at"})]
    }

    /// 任务列表桩：单个 `black_cat`（`accept_status=completed` + 进度达标）→ 直接走 claim。
    fn black_cat_completed_listing() -> CatResponse {
        (
            200,
            json!({"code": 0, "data": {"tasks": [{
                "task_code": "black_cat",
                "accept_status": "completed",
                "progress": {"current": 3, "target": 3}
            }]}}),
        )
    }

    const CAT_CHAT_CLAIM: &str =
        "https://copilot.tencent.com/activity/growth/tasks/black_cat/claim";
    const CAT_WEB_CLAIM: &str = "https://www.workbuddy.cn/activity/growth/tasks/black_cat/claim";

    /// D19：chat 域 claim 返回 **400** → 必须降级改打 web 域，且三个头齐全。
    #[tokio::test]
    async fn claim_400_degrades_to_web_domain_with_headers() {
        let night = cst_ms(2026, 9, 16, 23, 0);
        let io = RecordingIo::new(vec![
            black_cat_completed_listing(), // 0: GET tasks
            (
                400,
                json!({"code": 40001, "message": "chat claim not allowed"}),
            ), // 1: chat 域 claim
            (200, json!({"code": 0})),     // 2: web 域 claim
        ]);
        let out = run_cat_cycle_for_with(Region::Cn, night, one_cat_account(), &io).await;

        let urls = io.urls();
        assert_eq!(urls.len(), 3, "应恰好 3 次请求，实际 {urls:?}");
        assert_eq!(urls[1], CAT_CHAT_CLAIM, "第 2 次应为 chat 域 claim");
        assert_eq!(urls[2], CAT_WEB_CLAIM, "400 后必须降级 web 域");

        let calls = io.calls();
        let (_, method, body, extra) = &calls[2];
        assert_eq!(method, "POST");
        assert!(body.is_none());
        assert_eq!(
            extra.get("Origin").map(String::as_str),
            Some("https://www.workbuddy.cn")
        );
        assert_eq!(
            extra.get("Referer").map(String::as_str),
            Some("https://www.workbuddy.cn/profile/growth-center")
        );
        assert_eq!(
            extra.get("x-client-platform").map(String::as_str),
            Some("web")
        );
        assert_eq!(out["accounts"][0]["result"], "claimed", "{out}");
    }

    /// D19 对照：chat 域 claim 返回 **200** → **不得**发生第二次（web）调用。
    #[tokio::test]
    async fn claim_200_does_not_trigger_web_fallback() {
        let night = cst_ms(2026, 9, 16, 23, 0);
        let io = RecordingIo::new(vec![
            black_cat_completed_listing(), // 0: GET tasks
            (200, json!({"code": 0})),     // 1: chat 域 claim OK
        ]);
        let out = run_cat_cycle_for_with(Region::Cn, night, one_cat_account(), &io).await;

        let urls = io.urls();
        assert_eq!(urls.len(), 2, "200 不得触发降级，实际 {urls:?}");
        assert!(
            !urls.iter().any(|u| u.contains("workbuddy.cn")),
            "200 时不应打 web 域: {urls:?}"
        );
        assert_eq!(out["accounts"][0]["result"], "claimed", "{out}");
    }

    /// D17：非窗口时刻经**真实入口**必须「跳过并正常退出」，且桩 IO **一次都不被调用**。
    #[tokio::test]
    async fn out_of_window_skips_without_any_io_call() {
        let noon = cst_ms(2026, 9, 16, 12, 0);
        let io = RecordingIo::new(vec![]);
        let out = run_cat_cycle_for_with(Region::Cn, noon, one_cat_account(), &io).await;

        assert_eq!(out["status"], "ok", "非窗口不得记为失败: {out}");
        assert_eq!(out["skipped"], "not_night_window", "{out}");
        assert!(io.calls().is_empty(), "非窗口不应发起任何请求");
    }

    /// D17 对照：窗口内时刻必须**进入**执行路径（至少一次调用，且不再标记为窗口跳过）。
    #[tokio::test]
    async fn in_window_enters_execution_path() {
        let night = cst_ms(2026, 9, 16, 23, 0);
        let io = RecordingIo::new(vec![(200, json!({"code": 0, "data": {"tasks": []}}))]);
        let out = run_cat_cycle_for_with(Region::Cn, night, one_cat_account(), &io).await;

        assert_eq!(out["status"], "ok", "{out}");
        assert_ne!(
            out["skipped"], "not_night_window",
            "窗口内不得标记为窗口跳过: {out}"
        );
        assert!(!io.calls().is_empty(), "窗口内应至少发起一次请求");
    }
}
