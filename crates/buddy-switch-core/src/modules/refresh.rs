//! Token 刷新与保活。
//!
//! 对照 server.py `refresh_account_token` / `ensure_fresh_token` /
//! `run_keepalive_cycle`。
//!
//! **region 化**：新增 `*_for(region, …)` 变体；旧 CN 签名保留为薄包装。

use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;

use crate::modules::account::{
    build_auth_headers, envelope_refresh_error, load_accounts_for, upsert_account_for,
};
use crate::modules::config::{
    http_request, load_checkin_config, norm_ts, now_ms, RunFlagGuard, WORKBUDDY_API_PREFIX,
};
use crate::modules::region::{region_spec, Region};

// 保活运行标志按 region 独立：CN / Global 都会真正执行保活，若共用一个全局标志，
// 一版的保活周期会把另一版误报为 skipped/already_running（违反 PRD G1「两版互不污染」）。
static KEEPALIVE_RUNNING_CN: AtomicBool = AtomicBool::new(false);
static KEEPALIVE_RUNNING_GLOBAL: AtomicBool = AtomicBool::new(false);

/// 取该 region 独立的保活运行标志。
fn keepalive_running_flag(region: Region) -> &'static AtomicBool {
    match region {
        Region::Cn => &KEEPALIVE_RUNNING_CN,
        Region::Global => &KEEPALIVE_RUNNING_GLOBAL,
    }
}

/// `expiresIn` / `refreshExpiresIn`（相对秒）的合理上界：10 年。
///
/// 官方接口在正常情况下回一个正数秒数；但**缺字段与脏值是两种不同的情况**，
/// 且都必须与「合法值」区分开：
///
/// · **缺字段** ⇒ 调用方保留旧到期时间（不臆测）；
/// · **越界值**（负数、0、荒谬的大数）⇒ 同样保留旧值。
///
/// 若不设上界就照写，一个脏值会把 `expiresAt` 写成过去或几百年后 ——
/// 面板上的有效期进度条与倒计时会跟着显示误导信息，而刷新本身是成功的，
/// 用户完全看不出问题出在哪个字段。
///
/// 依据：上游参照实现与 `ithtelab/workbuddy-manager` 的刷新实现都做同一件事
/// （后者注释写明「expiresIn 缺省时**保留旧到期时间** —— 拿不到就不要乱写」，
/// 并显式排除了 `bool` 与越界值）。
const MAX_RELATIVE_EXPIRY_SECONDS: i64 = 10 * 365 * 86400;

/// 从响应字段里取一个**可信**的相对秒数；缺失或越界一律返回 `None`。
///
/// `serde_json` 里 `true` 不是 number，故 `as_i64()` 对布尔已自然返回 `None`，
/// 不需要额外判 `is_boolean`。
fn sane_relative_seconds(value: Option<&Value>) -> Option<i64> {
    let seconds = value?.as_i64()?;
    (0 < seconds && seconds < MAX_RELATIVE_EXPIRY_SECONDS).then_some(seconds)
}

/// 刷新单账号 token（CN）。
pub async fn refresh_account_token(account: Value) -> Value {
    refresh_account_token_for(Region::Cn, account).await
}

/// 尽力解开 at-rest 信封凭据（`access_token` / `refresh_token` / `nickname`）。
///
/// 失败**只打日志、原样返回**：解锁是「锦上添花」，不该让刷新这个动作整体失败。
/// 没装客户端 / 客户端换了加密实现 / 取钥超时，都会退回「信封态」的老行为。
fn try_unlock(region: Region, account: Value) -> Value {
    if !crate::modules::at_rest::is_unlockable(&account) {
        return account;
    }
    match crate::modules::at_rest::unlock_account_for(region, &account) {
        Ok(Some(unlocked)) => unlocked,
        Ok(None) => account,
        Err(error) => {
            eprintln!("[at-rest] 信封凭据未解锁（{}）：{error}", error.code());
            account
        }
    }
}

/// 刷新失败是否属于「暂时性」：网络层错误（-1）/ HTTP 5xx 错误页（body 不可解析 ⇒
/// `http_request` 把 status 当 code）都**不能证明凭据坏了**。业务拒绝码（12153
/// invalid_grant、401/403）不在此列 —— 那些才是需要重新登录的信号。
///
/// 契约见 `config::http_request`：网络错误 `{"code": -1}`，HTTP 错误且 body 不可解析
/// `{"code": <status>}`，服务端业务响应则原样返回 body 的 `code`。
fn is_transient_refresh_failure(code: i64) -> bool {
    code < 0 || (500..=599).contains(&code)
}

/// 把一次刷新失败写进账号返回值；返回**是否落了** `needs_relogin`（保活三态判据）。
///
/// ★ 暂时性失败**只挂返回值**（`refresh_transient_error`），不动标记语义、不落盘临时字段；
///   业务拒绝码照旧落 `needs_relogin` + 原因。抽成纯函数是为了让「断网 / 5xx 不误标」
///   能被单测直接断言，不必起网络（issue #171）。
fn apply_refresh_failure(account: &mut Value, code: i64, message: &str) -> bool {
    let reason = format!("刷新失败(code={code}): {message}");
    if is_transient_refresh_failure(code) {
        account["refresh_transient_error"] = json!(reason);
        false
    } else {
        account["needs_relogin"] = json!(true);
        account["needs_relogin_reason"] = json!(reason);
        true
    }
}

/// 保活单账号结果的三态判据：`transient` 优先于 `failed`（账号可能带着历史误标，
/// 但这一轮只是断网 / 5xx，不该再被记成「凭据被拒」）。抽成纯函数供单测直接断言。
fn refresh_result_status(fresh: &Value) -> &'static str {
    if fresh
        .get("refresh_transient_error")
        .and_then(|v| v.as_str())
        .is_some()
    {
        "transient"
    } else if fresh.get("needs_relogin").and_then(|v| v.as_bool()) == Some(true) {
        "failed"
    } else {
        "ok"
    }
}

/// 刷新单账号 token（POST `{billing_base}/v2/plugin/auth/token/refresh`），成功则落盘并返回新账号。
///
/// 刷新失败按「暂时性 / 凭据被拒」分流（见 [`apply_refresh_failure`]）：只有后者
/// 给账号标记 needs_relogin，避免网络抖动把健康账号判死、徽章一挂一整天（issue #171）。
pub async fn refresh_account_token_for(region: Region, mut account: Value) -> Value {
    // ★ 先解锁再判空：信封是**对象**，`as_str()` 取不到值。不先解锁就会把
    //   「已加密」当成「缺少 refresh token」——用户照着「去补导入」查也查不出东西
    //   （东西一直都在，只是解不开）。
    //
    //   解锁本身**不落盘**（见 `at_rest::unlock_account_for`）；明文能保住，是因为
    //   下面每条返回路径都会 `upsert_account_for(region, &account)`，而 `account`
    //   此刻已经是解锁后的值 —— 所以网络刷新失败也不会把解出来的明文丢掉。
    account = try_unlock(region, account);
    let previous_access_token = account
        .get("access_token")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let rt = account
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if rt.is_empty() {
        account["needs_relogin"] = json!(true);
        // ★ 先问「是不是加密信封」，再问「是不是真的没有」：信封是**对象**，
        //   `as_str()` 取不到值，不先判就会把「已加密」报成「**缺少** refresh token」
        //   （2026-09-24 用户截图）。两类原因的可读文案与修法都不同，
        //   见 `account::envelope_refresh_error`。
        account["needs_relogin_reason"] = json!(envelope_refresh_error(&account)
            .unwrap_or_else(|| "缺少 refresh token，无法刷新，需重新登录".to_string()));
        let _ = upsert_account_for(region, &account);
        return account;
    }

    let mut headers = build_auth_headers(&account);
    headers.insert("X-Refresh-Token".to_string(), rt.clone());
    // ⚠️ 本路径**不发** `X-Auth-Refresh-Source`，而 `upstream::refresh_token`
    // （server 侧同款实现）发 `workbuddy` —— 两处形态不一致，必有一端不像官方客户端。
    // 该头的取值本身也未定案（详见 `upstream::refresh_token` 里的长注释：
    // 本仓取值无出处，外部实测记录称官方客户端发 `plugin`）。
    //
    // 这里**刻意不动**：给一条正在正常工作的刷新请求加头，是收益未证的 wire 变更。
    // 要动就两端一起动，且先抓包。两处实现按仓库约定互指，见 `upstream::refresh_token`。
    let url = format!(
        "{}{WORKBUDDY_API_PREFIX}/auth/token/refresh",
        region_spec(region).billing_base
    );
    let resp = http_request(&url, "POST", Some(json!({})), Some(&headers)).await;
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        let message = resp
            .get("message")
            .or_else(|| resp.get("msg"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        let transient = !apply_refresh_failure(&mut account, code, message);
        // 失败详情只挂返回值：落盘副本先摘掉临时字段；仍要 upsert，否则
        // `try_unlock` 解出的明文白解了（见函数头注释）。暂时性分支不动标记语义。
        let mut on_disk = account.clone();
        if transient {
            on_disk
                .as_object_mut()
                .unwrap()
                .remove("refresh_transient_error");
        }
        let _ = upsert_account_for(region, &on_disk);
        return account;
    }

    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    let new_at = data
        .get("accessToken")
        .and_then(|v| v.as_str())
        .or_else(|| data.get("access_token").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let Some(new_at) = new_at else {
        account["needs_relogin"] = json!(true);
        account["needs_relogin_reason"] = json!("刷新响应缺少 accessToken");
        let _ = upsert_account_for(region, &account);
        return account;
    };

    account["access_token"] = json!(new_at);
    if let Some(new_rt) = data
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .or_else(|| data.get("refresh_token").and_then(|v| v.as_str()))
    {
        account["refresh_token"] = json!(new_rt);
    }
    // 官方接口只返回相对 expiresIn（秒），需换算为绝对时间戳。
    // 缺字段或越界值都**不动** `expiresAt`（保留旧值），见 `sane_relative_seconds`。
    let new_exp = norm_ts(data.get("expiresAt").or_else(|| data.get("expires_at")));
    let new_exp = match new_exp {
        Some(v) => Some(v),
        None => sane_relative_seconds(data.get("expiresIn")).map(|e| now_ms() + e * 1000),
    };
    if let Some(v) = new_exp {
        account["expiresAt"] = json!(v);
    }
    let fallback_rt_exp = norm_ts(
        account
            .get("auth_raw")
            .and_then(|a| a.get("refreshExpiresAt")),
    );
    let mut new_rt_exp = norm_ts(
        data.get("refreshExpiresAt")
            .or_else(|| data.get("refresh_expires_at")),
    );
    if new_rt_exp.is_none() {
        new_rt_exp = fallback_rt_exp;
    }
    let new_rt_exp = match new_rt_exp {
        Some(v) => Some(v),
        None => sane_relative_seconds(data.get("refreshExpiresIn")).map(|e| now_ms() + e * 1000),
    };
    if let Some(v) = new_rt_exp {
        account["refreshExpiresAt"] = json!(v);
    }
    account["refreshedAt"] = json!(now_ms());
    let map = account.as_object_mut().unwrap();
    map.remove("needs_relogin");
    map.remove("needs_relogin_reason");
    // 成功时顺手清掉返回值上的临时字段，防止调用方复用返回值落盘时残留。
    map.remove("refresh_transient_error");
    let _ = upsert_account_for(region, &account);
    // Windows 不执行 apiKeyHelper；当前 CLI 账号刷新后同步 settings env。
    // 同步失败不阻断 WorkBuddy 保活；状态接口会根据 settings 与账号库是否
    // 一致显示“待同步”，避免把认证配置错误混入账号数据。
    if cfg!(windows) {
        let _ = crate::modules::codebuddy_cli::sync_windows_env_for_account(
            &account,
            previous_access_token.as_deref(),
        );
    }
    account
}

/// 惰性刷新（CN）：expiresAt 缺失或剩余 < lazy_refresh_hours 则刷新。返回最新账号。
pub async fn ensure_fresh_token(account: Value, cfg: &Value) -> Value {
    ensure_fresh_token_for(Region::Cn, account, cfg).await
}

/// 按 region 惰性刷新：expiresAt 缺失或剩余 < lazy_refresh_hours 则刷新。
pub async fn ensure_fresh_token_for(region: Region, mut account: Value, cfg: &Value) -> Value {
    let lazy_h = cfg
        .get("lazy_refresh_hours")
        .and_then(|v| v.as_i64())
        .unwrap_or(24);
    let exp = account.get("expiresAt").and_then(|v| v.as_i64());
    let stale = match exp {
        Some(e) => now_ms() >= e || e - now_ms() < lazy_h * 3600 * 1000,
        None => true,
    };
    let has_rt = !account
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty();
    if stale && has_rt {
        account = refresh_account_token_for(region, account).await;
    }
    account
}

/// 保活检查（CN）：每天由后台循环调用一次。
pub async fn run_keepalive_cycle() -> Value {
    run_keepalive_cycle_for(Region::Cn).await
}

/// 按 region 保活检查：每天由后台循环调用一次，默认（keepalive_days <= 0）无条件刷新
/// 全部带 refresh token 的账号；keepalive_days > 0 时仅刷新剩余不足该天数的账号。
///
/// 高频保活是为了避免官方服务端清理闲置的 refresh 会话——曾出现闲置数天后
/// 刷新返回 12153 invalid_grant（Session doesn't have required client）导致
/// 账号被迫重新登录。
pub async fn run_keepalive_cycle_for(region: Region) -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(keepalive_running_flag(region)) else {
        return json!({"skipped": "already_running"});
    };
    let cfg = load_checkin_config();
    let keep_days = cfg
        .get("keepalive_days")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let accounts = load_accounts_for(region);
    let total = accounts.len();
    let mut results: Vec<Value> = Vec::new();
    for mut acc in accounts {
        let exp = acc.get("expiresAt").and_then(|v| v.as_i64());
        let stale = keep_days <= 0
            || match exp {
                Some(e) => now_ms() >= e || e - now_ms() < keep_days * 24 * 3600 * 1000,
                None => true,
            };
        if !stale {
            continue;
        }
        // 同 `refresh_account_token_for`：先解锁，再判「有没有 refresh token」。
        acc = try_unlock(region, acc);
        if acc
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            // 与 `refresh_account_token_for` 同一条判据：**信封 ≠ 缺失**。
            // 保活结果里的 `status` 也跟着分家 —— 它会被排程日志记下来，
            // 把信封记成 `missing_rt` 会让排障顺着「没导入进来」查错方向。
            let envelope = envelope_refresh_error(&acc);
            let reason = envelope
                .clone()
                .unwrap_or_else(|| "缺少 refresh token，无法保活，需重新登录".to_string());
            // ★ 已带同一条标记的账号：链条没变，别每晚重写一次 accounts.json、
            //   也别往排程日志里塞一条长得一模一样的失败。标记与原因都没变才跳过；
            //   原因变了（例如「刷新失败」变成「信封解不开」）仍要落盘更新。
            //   有明文 refresh_token 的账号不在此列 —— 那类失败可能只是上游临时抖动，
            //   保活继续试，刷新成功时 `refresh_account_token_for` 会清掉标记。
            let already_flagged = acc.get("needs_relogin").and_then(|v| v.as_bool()) == Some(true)
                && acc.get("needs_relogin_reason").and_then(|v| v.as_str())
                    == Some(reason.as_str());
            if !already_flagged {
                acc["needs_relogin"] = json!(true);
                acc["needs_relogin_reason"] = json!(reason.clone());
                let _ = upsert_account_for(region, &acc);
            }
            results.push(json!({
                "email": crate::modules::account::account_display_name(&acc),
                "status": if already_flagged {
                    "skipped"
                } else if envelope.is_some() {
                    "envelope_rt"
                } else {
                    "missing_rt"
                },
                // 带上原因：排程日志里只看到 `envelope_rt` 是没法排障的，
                // 且这条与上面 `failed` 分支的字段形状保持一致。
                "error": reason,
            }));
            continue;
        }
        let fresh = refresh_account_token_for(region, acc).await;
        // ★ 三态（issue #171）：暂时性失败优先于 `failed` —— 账号可能带着历史误标，
        //   但这一轮只是断网 / 5xx，不该再被记成「凭据被拒」。
        let transient = fresh
            .get("refresh_transient_error")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let failed = fresh.get("needs_relogin").and_then(|v| v.as_bool()) == Some(true);
        results.push(json!({
            "email": crate::modules::account::account_display_name(&fresh),
            "status": refresh_result_status(&fresh),
            "error": if let Some(reason) = transient {
                Some(reason)
            } else if failed {
                fresh.get("needs_relogin_reason").and_then(|v| v.as_str()).map(|s| s.to_string())
            } else {
                None
            },
        }));
    }
    json!({"checked": total, "refreshed": results})
}

#[cfg(test)]
mod tests {
    // 测试串行锁（`lock_*()`）会被**故意**持有到用例结束：它防的是多个用例同时抢占
    // 同一个回调端口 / 全局状态。`#[tokio::test]` 默认是 current-thread 运行时，
    // 不存在「持锁跨线程唤醒」的风险；clippy 的 await_holding_lock 在这里是误报。
    #![allow(clippy::await_holding_lock)]
    use super::*;

    /// 触及保活全局状态的用例必须**串行**：`KEEPALIVE_RUNNING_CN` 是**进程级**全局标志，
    /// cargo test 默认并行时，一个用例持有它会让另一个拿到 `skipped/already_running`
    /// ——「单独跑必绿、全量随机红」的假失败（同 `checkin` 的既有处置）。
    static KEEPALIVE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn keepalive_lock() -> std::sync::MutexGuard<'static, ()> {
        // 吞掉中毒：单个用例 panic 不该连带其它用例全红。
        KEEPALIVE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// `expiresIn` 只接受「正数且不超过 10 年」；缺失、0、负数、荒谬大数一律拒绝。
    ///
    /// 拒绝的语义是**保留旧到期时间**（调用方只在 `Some` 时才写 `expiresAt`），
    /// 所以「拒绝」与「缺失」必须走同一条路 —— 若这里放行一个脏值，
    /// 面板上的有效期与倒计时会显示误导信息，而刷新本身是成功的，很难归因。
    #[test]
    fn relative_expiry_seconds_rejects_missing_and_out_of_range() {
        // 合法：正常时长
        assert_eq!(sane_relative_seconds(Some(&json!(3600))), Some(3600));
        assert_eq!(
            sane_relative_seconds(Some(&json!(60 * 86400))),
            Some(60 * 86400)
        );
        // 缺失 / 非数值形态
        assert_eq!(sane_relative_seconds(None), None);
        assert_eq!(
            sane_relative_seconds(Some(&json!("3600"))),
            None,
            "字符串不是合法秒数"
        );
        assert_eq!(
            sane_relative_seconds(Some(&json!(true))),
            None,
            "布尔不得当 1 用"
        );
        assert_eq!(sane_relative_seconds(Some(&json!(null))), None);
        // 越界：0 与负数会把 expiresAt 写到「此刻或过去」
        assert_eq!(sane_relative_seconds(Some(&json!(0))), None);
        assert_eq!(sane_relative_seconds(Some(&json!(-1))), None);
        assert_eq!(sane_relative_seconds(Some(&json!(-86400))), None);
        // 越界：荒谬大数会把有效期推到几百年后
        assert_eq!(
            sane_relative_seconds(Some(&json!(MAX_RELATIVE_EXPIRY_SECONDS))),
            None,
            "上界是开区间"
        );
        assert_eq!(sane_relative_seconds(Some(&json!(i64::MAX))), None);
        assert_eq!(
            sane_relative_seconds(Some(&json!(MAX_RELATIVE_EXPIRY_SECONDS - 1))),
            Some(MAX_RELATIVE_EXPIRY_SECONDS - 1)
        );
    }

    /// 保活运行标志必须按 region 独立（PRD G1）。
    ///
    /// 只断言「两个 `KEEPALIVE_RUNNING_*` 常量不同」是不够的——常量不同 ≠
    /// `keepalive_running_flag` 用对了常量。这里从两个角度钉住契约：
    /// ① 指针身份（纯断言，不碰状态）；② 获取语义（CN 持有时 Global 仍可获取）。
    ///
    /// 两版若共用一把标志，一版的保活周期会把另一版误报为 `skipped/already_running`。
    #[test]
    fn keepalive_running_flag_is_region_scoped() {
        let _lock = keepalive_lock();
        // ① 指针身份：同一 region 稳定返回同一把标志，跨 region 必须是不同对象。
        assert!(std::ptr::eq(
            keepalive_running_flag(Region::Cn),
            keepalive_running_flag(Region::Cn)
        ));
        assert!(std::ptr::eq(
            keepalive_running_flag(Region::Global),
            keepalive_running_flag(Region::Global)
        ));
        assert!(
            !std::ptr::eq(
                keepalive_running_flag(Region::Cn),
                keepalive_running_flag(Region::Global)
            ),
            "CN / Global 保活标志不得共用同一把锁"
        );

        // ② 获取语义：持有 CN 时 Global 仍能独立获取（跨版不互相阻塞）。
        // 守卫是 RAII 的，panic 展开时也会释放，不会给后续用例留下脏状态。
        let cn = RunFlagGuard::try_acquire(keepalive_running_flag(Region::Cn))
            .expect("CN 保活标志初始应为空闲");
        // CN 已持有：再取 CN 必须失败（同版互斥仍然有效）……
        assert!(RunFlagGuard::try_acquire(keepalive_running_flag(Region::Cn)).is_none());
        // ……但取 Global 必须成功（跨版不互相阻塞）。
        let global = RunFlagGuard::try_acquire(keepalive_running_flag(Region::Global));
        assert!(global.is_some(), "CN 保活进行中不应阻塞 Global 保活");

        drop(global);
        drop(cn);
        // 释放后应可重新获取。
        assert!(RunFlagGuard::try_acquire(keepalive_running_flag(Region::Cn)).is_some());
    }

    /// ★ 现场（2026-09-24 用户截图）：账号库里 `refresh_token` 是**加密信封对象**时，
    /// 刷新链路必须报「加密信封」，**不得**报「缺少 refresh token」。
    ///
    /// 该账号（Andev）库里 `refresh_token` / `refreshExpiresAt` 都在，`as_str()` 却取不到
    /// 值 ⇒ 旧实现走「取不到字符串 = 缺少」这条短路，用户看到
    /// 「Andev：需重新登录（缺少 refresh token，无法刷新）」—— 与事实相反，
    /// 用户照着「去补导入」查也查不出东西（东西一直都在，只是解不开）。
    ///
    /// 可证伪：把 `needs_relogin_reason` 改回字面量
    /// 「缺少 refresh token，无法刷新，需重新登录」，第 1、2 条断言立刻红。
    #[tokio::test]
    async fn envelope_refresh_token_reports_encrypted_not_missing() {
        // 本分支会 `upsert_account_for` 落盘 ⇒ 必须隔离 home，否则写真实账号库。
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-refresh-envelope-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create dir");
        let guard = crate::modules::config::HomeOverrideGuard::set(&dir);

        // 1) 信封 refresh_token（与用户账号库实测形态一致：access / refresh 都是信封）。
        let encrypted = json!({
            "id": "a1",
            "uid": "u1",
            "access_token": {"$wbEncrypted": 1, "envelope": "a"},
            "refresh_token": {"$wbEncrypted": 1, "envelope": "r"},
            "refreshExpiresAt": 1_795_431_563_919_i64,
        });
        let fresh = refresh_account_token_for(Region::Cn, encrypted).await;
        assert_eq!(
            fresh["needs_relogin"],
            json!(true),
            "信封凭据不可用，仍须标记需重新登录"
        );
        let reason = fresh["needs_relogin_reason"].as_str().unwrap_or_default();
        assert!(reason.contains("加密信封"), "文案应可读：{reason}");
        assert!(
            !reason.contains("缺少"),
            "★ 不得把「已加密」报成「缺少」——这正是本次修复的缺陷形态：{reason}"
        );

        // 2) 阳性对照：**真缺失**（无 refresh_token 字段）仍报「缺少」，
        //    且不得串到信封文案上 —— 两类原因的修法不同，必须能分开。
        let missing = refresh_account_token_for(
            Region::Cn,
            json!({"id": "a2", "uid": "u2", "access_token": "AT"}),
        )
        .await;
        let reason2 = missing["needs_relogin_reason"].as_str().unwrap_or_default();
        assert!(
            reason2.contains("缺少 refresh token"),
            "真缺失应报「缺少」：{reason2}"
        );
        assert!(!reason2.contains("加密信封"), "真缺失不得报信封：{reason2}");

        // 3) 落盘的是**同一条**原因：界面读的是账号库，不是本次返回值。
        let saved = load_accounts_for(Region::Cn);
        let saved_a1 = saved
            .iter()
            .find(|a| a["id"] == json!("a1"))
            .expect("a1 应已落盘");
        assert!(
            saved_a1["needs_relogin_reason"]
                .as_str()
                .unwrap_or_default()
                .contains("加密信封"),
            "账号库里存的原因也必须是信封文案，否则界面仍显示旧文案：{}",
            saved_a1["needs_relogin_reason"]
        );

        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 保活链路与刷新链路**同一条判据**：信封 refresh_token 报 `envelope_rt` + 信封文案，
    /// 不得报 `missing_rt` + 「缺少」。
    ///
    /// 可证伪：把保活分支里的 `envelope_refresh_error` 去掉（回到字面量），
    /// `status` 变回 `missing_rt`、`error` 变回「缺少 refresh token…」⇒ 三条断言全红。
    #[tokio::test]
    async fn keepalive_reports_envelope_rt_not_missing_rt() {
        let _lock = keepalive_lock();
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-keepalive-envelope-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create dir");
        let guard = crate::modules::config::HomeOverrideGuard::set(&dir);

        crate::modules::account::save_accounts_for(
            Region::Cn,
            &[json!({
                "id": "a1",
                "uid": "u1",
                // 信封本身是坏的（`-` 不在 base64 字母表里）⇒ 解锁会在**起子进程之前**
                // 就失败，于是这条用例既不依赖本机装没装客户端，也不发网络请求。
                "access_token": {"$wbEncrypted": 1, "envelope": "not-base64"},
                "refresh_token": {"$wbEncrypted": 1, "envelope": "not-base64"},
            })],
        )
        .expect("seed accounts");

        let out = run_keepalive_cycle_for(Region::Cn).await;
        let entry = out["refreshed"]
            .as_array()
            .and_then(|list| list.first())
            .cloned()
            .expect("应有保活结果");
        assert_eq!(
            entry["status"],
            json!("envelope_rt"),
            "信封必须与「真的没有」分档：{entry}"
        );
        let error = entry["error"].as_str().unwrap_or_default();
        assert!(error.contains("加密信封"), "文案应可读：{error}");
        assert!(
            !error.contains("缺少"),
            "★ 不得把「已加密」报成「缺少」：{error}"
        );

        // 落盘的原因也必须是信封文案（界面读的是账号库，不是本次返回值）。
        let saved = load_accounts_for(Region::Cn);
        let saved_reason = saved[0]["needs_relogin_reason"]
            .as_str()
            .unwrap_or_default();
        assert!(saved_reason.contains("加密信封"), "{saved_reason}");

        // 第二轮：标记与原因都没变 ⇒ 不再重复落盘，排程日志记 `skipped`。
        // 保活每天跑一次，同一个死账号不该每晚重写一遍 accounts.json。
        let out2 = run_keepalive_cycle_for(Region::Cn).await;
        let entry2 = out2["refreshed"]
            .as_array()
            .and_then(|list| list.first())
            .cloned()
            .expect("应有第二轮保活结果");
        assert_eq!(
            entry2["status"],
            json!("skipped"),
            "已标记的账号不该每晚重写一轮：{entry2}"
        );

        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// issue #171：断网 / HTTP 5xx 错误页不能证明凭据坏了 ⇒ 不落 `needs_relogin`；
    /// 业务拒绝码（12153 / 401 / 403）仍要落标记；保活结果三态同时自证。
    #[test]
    fn transient_refresh_failures_do_not_flag_relogin() {
        for code in [-1, 500, 502, 503, 504] {
            assert!(is_transient_refresh_failure(code), "code={code} 应判暂时性");
            let mut acc = json!({ "id": "a1", "access_token": "at", "refresh_token": "rt" });
            let flagged = apply_refresh_failure(&mut acc, code, "boom");
            assert!(!flagged, "code={code} 不该落标记");
            assert!(
                acc.get("needs_relogin").is_none(),
                "code={code} 误标了：{acc}"
            );
            assert!(
                acc.get("refresh_transient_error")
                    .and_then(|v| v.as_str())
                    .is_some(),
                "code={code} 缺可读原因：{acc}"
            );
            assert_eq!(refresh_result_status(&acc), "transient");
        }
        for code in [401, 403, 12153] {
            assert!(
                !is_transient_refresh_failure(code),
                "code={code} 是业务拒绝"
            );
            let mut acc = json!({ "id": "a1", "access_token": "at", "refresh_token": "rt" });
            let flagged = apply_refresh_failure(&mut acc, code, "invalid_grant");
            assert!(flagged, "code={code} 应落标记");
            assert_eq!(acc["needs_relogin"], json!(true));
            assert_eq!(refresh_result_status(&acc), "failed");
        }
        assert_eq!(
            refresh_result_status(&json!({ "access_token": "new" })),
            "ok"
        );
        // 历史误标 + 本轮暂时性失败 ⇒ 仍报 transient，不冒充「凭据被拒」。
        let mut mixed = json!({ "needs_relogin": true });
        mixed["refresh_transient_error"] = json!("刷新暂时失败");
        assert_eq!(refresh_result_status(&mixed), "transient");
    }
}
