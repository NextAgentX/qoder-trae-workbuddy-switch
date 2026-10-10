//! 请求日志（**默认只记元数据**，可配置正文开关，保留 N 条；P1-1 / Q5）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::Region;

/// 一次请求的元数据（不含 prompt/response 正文）。
pub struct RequestMeta {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub region: Region,
    /// 命中账号的**展示名**（可能重名 / 为空；仅供人读，不作身份）。
    pub account: String,
    /// 命中账号的 **uid**（写入时的**精确身份**，供 `token_daily` 精确 join）。
    pub account_uid: String,
    pub model: String,
    pub status: u16,
    pub latency_ms: i64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub stream: bool,
}

impl RequestMeta {
    /// 序列化为日志条目。
    pub fn to_value(&self) -> Value {
        json!({
            "ts": core_config::now_ms(),
            "endpoint": self.endpoint,
            "method": self.method,
            "region": self.region,
            "account": self.account,
            "accountUid": self.account_uid,
            "model": self.model,
            "status": self.status,
            "latencyMs": self.latency_ms,
            "promptTokens": self.prompt_tokens,
            "completionTokens": self.completion_tokens,
            "stream": self.stream,
        })
    }
}

/// 请求日志：内存 + 落盘，保留最近 N 条。
pub struct RequestLog {
    path: PathBuf,
    keep: Mutex<usize>,
    log_bodies: AtomicBool,
    entries: Mutex<Vec<Value>>,
}

impl RequestLog {
    /// 新建日志（读取既有落盘条目）。
    pub fn new(path: PathBuf, keep: usize, log_bodies: bool) -> Self {
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Self {
            path,
            keep: Mutex::new(keep.max(1)),
            log_bodies: AtomicBool::new(log_bodies),
            entries: Mutex::new(entries),
        }
    }

    /// 是否记录正文。
    pub fn log_bodies(&self) -> bool {
        self.log_bodies.load(Ordering::SeqCst)
    }

    /// 更新保留条数并裁剪。
    pub fn set_keep(&self, keep: usize) {
        *self.keep.lock().unwrap() = keep.max(1);
        self.trim_and_save();
    }

    /// 更新正文开关。
    pub fn set_log_bodies(&self, enabled: bool) {
        self.log_bodies.store(enabled, Ordering::SeqCst);
    }

    /// 追加一条日志（元数据），并裁剪到保留上限。
    pub fn record(&self, entry: Value) {
        {
            let mut entries = self.entries.lock().unwrap();
            entries.push(entry);
        }
        self.trim_and_save();
    }

    /// 返回最近 N 条（按时间升序）。
    pub fn list(&self) -> Vec<Value> {
        self.entries.lock().unwrap().clone()
    }

    /// 清空日志（内存 + 落盘）。
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
        self.save(&[]);
    }

    fn trim_and_save(&self) {
        let keep = *self.keep.lock().unwrap();
        let snapshot = {
            let mut entries = self.entries.lock().unwrap();
            if entries.len() > keep {
                let drop_count = entries.len() - keep;
                entries.drain(..drop_count);
            }
            entries.clone()
        };
        self.save(&snapshot);
    }

    fn save(&self, entries: &[Value]) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".to_string());
        let _ = core_config::atomic_write(&self.path, &content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日志条目必须携带写入时的**精确 uid**（`accountUid`），且 `account`（展示名）语义不变。
    #[test]
    fn request_meta_carries_account_uid_alongside_display_name() {
        let meta = RequestMeta {
            endpoint: "/v1/chat/completions",
            method: "POST",
            region: Region::Cn,
            account: "重名展示名".to_string(),
            account_uid: "uid-b".to_string(),
            model: "hy4".to_string(),
            status: 200,
            latency_ms: 12,
            prompt_tokens: 10,
            completion_tokens: 2,
            stream: false,
        };

        let value = meta.to_value();
        assert_eq!(value["accountUid"], "uid-b", "必须写入选中账号的精确 uid");
        assert_eq!(value["account"], "重名展示名", "account 仍是展示名，语义不变");
    }
}
