//! 账号切换：备份 → 关进程 → 复制会话（可选）→ 写认证 → 启动。
//!
//! 对照 server.py `switch_account`。切换过程中通过进度回调向前端推送实时进度，
//! 避免界面长时间无反馈被误认为卡死。core 不依赖 Tauri，进度回调由宿主适配
//! （桌面端转发为 `switch-progress` 事件，HTTP 端写入轮询/SSE）。
//!
//! **region 化**：新增 [`switch_account_for`]；旧 CN 签名保留为薄包装。

use serde_json::{json, Value};

use crate::modules::account;
use crate::modules::auth_file;
use crate::modules::process::{close_workbuddy_for, launch_workbuddy_for};
use crate::modules::region::Region;
use crate::modules::session;

/// 切换进度回调（宿主注入，如 Tauri `app.emit` 或 HTTP 进度缓存）。
pub type ProgressFn = Box<dyn Fn(&str) + Send + Sync>;

pub fn recovery_blocks_startup(report: &crate::modules::session_link::RecoveryReport) -> bool {
    report.needs_recovery.iter().any(|issue| !issue.retryable)
}

pub fn recovery_blocking_detail(report: &crate::modules::session_link::RecoveryReport) -> String {
    report
        .needs_recovery
        .iter()
        .filter(|issue| !issue.retryable)
        .map(|issue| format!("{}：{}", issue.operation_id, issue.reason))
        .collect::<Vec<_>>()
        .join("；")
}

pub fn preview_switch_session_links(
    client: Option<&str>,
    source_region: Region,
    region: Region,
    account_id: &str,
) -> Result<Value, String> {
    match client.unwrap_or("workbuddy") {
        "workbuddy" => session::preview_linked_switch(source_region, region, account_id),
        "vscodeExt" => {
            let target = account::find_account_for(region, account_id).ok_or("目标账号不存在")?;
            crate::modules::vscode_session_sync::links_preview(&target)
        }
        "codebuddyIde" => {
            let mut target =
                account::find_account_for(region, account_id).ok_or("目标账号不存在")?;
            target["region"] = json!(region.as_str());
            if region == Region::Global {
                crate::modules::codebuddy_ide_session_sync::links_preview_intl(&target)
            } else {
                crate::modules::codebuddy_ide_session_sync::links_preview(&target)
            }
        }
        _ => Err("不支持的会话客户端".to_string()),
    }
}

pub fn switch_linked_extension(
    client: &str,
    region: Region,
    account_id: &str,
    restart: Option<bool>,
    copy_sessions: Option<Value>,
    sync_selections: Option<&Value>,
) -> Result<Value, String> {
    let copy_items: Vec<crate::modules::linked_vscode_session::CopyItem> = match copy_sessions {
        Some(value) if !value.is_null() => serde_json::from_value(value)
            .map_err(|error| format!("copySessions 数据不正确：{error}"))?,
        _ => Vec::new(),
    };
    let sync = crate::modules::linked_workbuddy::parse_sync_selections(sync_selections)?;
    let restart_client = restart.unwrap_or(true);
    match (client, region) {
        ("codebuddyIde", Region::Cn) if copy_items.is_empty() && sync.is_empty() => {
            crate::modules::programs::switch("codebuddy_cn_ide", account_id, restart)
        }
        ("vscodeExt", _) if copy_items.is_empty() && sync.is_empty() => {
            crate::modules::programs::switch("vscode_codebuddy_plugin", account_id, restart)
        }
        ("codebuddyIde", Region::Cn) => {
            crate::modules::codebuddy_ide_session::switch_codebuddy_cn_ide_with_copy(
                account_id,
                restart_client,
                &copy_items,
                &sync,
            )
        }
        ("codebuddyIde", Region::Global) => {
            crate::modules::codebuddy_ide_session::switch_codebuddy_intl_ide_with_copy(
                account_id,
                restart_client,
                &copy_items,
                &sync,
            )
        }
        ("vscodeExt", _) => crate::modules::linked_vscode_session::switch_vscode_ext_with_copy(
            account_id,
            restart_client,
            &copy_items,
            &sync,
        ),
        _ => Err("不支持的会话客户端".to_string()),
    }
}

/// 切换账号（CN）。copy_session_ids 非空时按路径 B 复制勾选会话（新 id，云端可同步）。

pub fn switch_account(
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    copy_session_ids: &[String],
) -> Result<Value, String> {
    switch_account_for(
        Region::Cn,
        progress_fn,
        account_id,
        restart,
        copy_session_ids,
    )
}

/// 按 region 切换账号；会话复制的源版本与切换目标版本相同。
pub fn switch_account_for(
    region: Region,
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    copy_session_ids: &[String],
) -> Result<Value, String> {
    switch_account_cross(
        region,
        region,
        progress_fn,
        account_id,
        restart,
        copy_session_ids,
    )
}

/// 跨版本切换：`source_region` 的当前登录账号是会话复制的**源**，`region` 是切换目标。
///
/// 只有会话复制关心源版本 —— 记忆与连接器迁移走 `migrate_account_data_cross`，
/// 由调用方单独传源 region（见 [`crate::modules::migrate`]）。
///
/// ## 关于旧的 `share_sessions`「全体转移」参数（2026-09-28 移除）
///
/// 该参数是 Python 版遗留的兼容位，Rust 版**从未实现**：原实现是「当它为真时，往**成功的**
/// 报告里塞一个 `sessionShare: {error: …}`」。这比没有更坏 —— 调用方看到 `ok: true` 会以为
/// 切换（含会话转移）都成功了。且经全仓核查，它**没有任何调用方**：前端 `api.ts` 只有类型声明、
/// 无一处传值；无测试、无文档引用。⇒ 按「不保留死代码」整条移除，而非改成静默 no-op。
/// 若日后确需「全体转移」，应按新需求重新设计，不要复活这个半成品。
pub fn switch_account_cross(
    region: Region,
    source_region: Region,
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    copy_session_ids: &[String],
) -> Result<Value, String> {
    switch_account_cross_with_sync(
        region,
        source_region,
        progress_fn,
        account_id,
        restart,
        copy_session_ids,
        &[],
    )
}

pub fn switch_account_cross_with_sync(
    region: Region,
    source_region: Region,
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    copy_session_ids: &[String],
    sync_selections: &[crate::modules::linked_workbuddy::SyncSelection],
) -> Result<Value, String> {
    if !restart && !sync_selections.is_empty() {
        return Err("已选关联会话同步，请启用自动重开 WorkBuddy".to_string());
    }
    let progress = |message: &str| {
        eprintln!("[switch] progress: {message}");
        if let Some(p) = progress_fn {
            p(message);
        }
    };

    progress("开始切换账号…");
    let acc = account::find_account_for(region, account_id)
        .ok_or_else(|| format!("账号不存在: {account_id}"))?;
    let backup = auth_file::backup_auth_file_for(region);

    let mut copy_report: Option<Value> = None;
    let mut sync_report = None;
    if restart {
        progress("正在关闭 WorkBuddy…");
        close_workbuddy_for(region, 20)?;
        let recovery = session::recover_linked_sessions_for(region)?;
        if let Some(issue) = recovery["needsRecovery"]
            .as_array()
            .and_then(|issues| issues.first())
        {
            let reason = issue["reason"].as_str().unwrap_or("会话操作尚待恢复");
            return Err(format!("账号切换暂停：{reason}"));
        }
        // 只有重启场景才做会话操作（数据库在运行中不宜写入）
        if !copy_session_ids.is_empty() {
            // 逐会话进度直接透传：正文复制是 GB 级 I/O，切成 (i/N) 步进
            // 才能让用户知道「还在动」，而不是盯着一行消息以为卡死。
            progress("正在复制会话…");
            let report = session::copy_linked_sessions_for_switch(
                source_region,
                region,
                &acc,
                copy_session_ids,
            )?;
            if report["needsRecovery"] == true {
                return Err("会话操作尚待恢复，账号未切换。可在会话页恢复未完成的操作。".to_owned());
            }
            copy_report = Some(report);
        }
        if !sync_selections.is_empty() {
            progress("正在同步关联会话…");
            let report = session::sync_linked_sessions_for_switch(
                source_region,
                region,
                &acc,
                sync_selections,
            )?;
            if report["needsRecovery"] == true {
                return Err(
                    "会话操作尚待恢复，账号未切换。可在会话页恢复未完成的操作。".to_string()
                );
            }
            sync_report = Some(report);
        }
    }
    progress("正在写入认证文件…");
    auth_file::write_account_to_auth_file_for(region, &acc)?;
    // ★ 记录「最近使用」：切换即代表该账号被**实际使用**（时间戳口径与账号库既有
    // `createdAt` / `refreshedAt` 一致）。写入失败**不阻断**切换 —— 认证文件已写好、
    // 账号已被用上，一条本地记账失败不该把整个切换判为失败（故忽略错误，不 `?`）。
    let _ = account::touch_last_used_for(region, account_id);
    if restart {
        progress("正在启动 WorkBuddy…");
        launch_workbuddy_for(region, Some(&progress))?;
    }
    progress("切换完成");

    let mut result = json!({
        "ok": true,
        "account": account::account_display_name(&acc),
        "backup": backup.map(|p| p.to_string_lossy().to_string()),
    });
    if let Some(c) = copy_report {
        result["sessionCopy"] = c;
    }
    if let Some(sync) = sync_report {
        result["sessionSync"] = sync;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_account_missing_id_returns_before_auth_side_effects() {
        let missing_id = "switch-test-account-that-does-not-exist";
        let error = switch_account(None, missing_id, false, &[]).unwrap_err();
        assert!(error.contains("账号不存在"));
        assert!(error.contains(missing_id));
        // This deliberately does not prove the CN wrapper's Region::Cn binding:
        // proving that requires distinct CN/Global fixture stores, while a found
        // account would write the real auth file. That boundary belongs to an
        // isolated HOME integration test.
    }
}
