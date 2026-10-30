//! 请求日志（**默认只记元数据**，可配置正文开关，保留 N 条；P1-1 / Q5）。
//!
//! ## 写入时机：流式请求**推迟到流结束**
//!
//! 非流式请求在 handler 里就地写日志（响应体已读全，用量已知）。
//! **流式请求不一样**：`usage` 只在 SSE 末帧才到达，而日志条目要在响应返回前写
//! —— 此前只能把 `promptTokens` / `completionTokens` **写死 0**（2026-10-14 实测本机
//! 200/200 条全为 0）。⇒ 改用 [`StreamingLogSink`]：日志挂在 `UsageTap` 的结束回调上，
//! 流结束时才落盘，此时用量已知。
//!
//! ## ⚠️ 两个耗时字段**语义不同，不得合并**（2026-10-15 收紧）
//!
//! [`StreamingLogSink`] 的第一版顺手把 `latencyMs` 也改成了「流结束时的总耗时」，
//! 结果日志页的耗时列凭空放大 **10~100 倍**（实测 p50 从数百 ms 变成 8.7 s、
//! p90 变成 204 s）—— 用户合理地读成了「网关变慢了」。**一个字段名不能承载两种
//! 语义**：那次改动本身没有引入任何真实延迟（`RequestLog::record` 实测 p50 ≈ 2 ms，
//! 且落盘只是从「请求开始前」挪到「流结束后」）。
//!
//! ⇒ 现在固定为：
//! - [`RequestMeta::latency_ms`]（`latencyMs`）= **上游首字节延迟**（TTFB），
//!   由 handler 在拿到响应头时就地填 —— 与本次改造**之前逐字同语义**，历史可比；
//! - [`RequestMeta::total_ms`]（`totalMs`）= **整次请求总耗时**，
//!   流式由 sink 在流结束时补齐（那是新增信息，不覆盖任何旧字段）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::region::Region;

use crate::protocol::usage_tap::{UsageReport, UsageSink};

/// 一次请求的元数据（不含 prompt/response 正文）。
#[derive(Debug, Clone)]
pub struct RequestMeta {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub region: Region,
    /// 命中账号的**展示名**（可能重名 / 为空；仅供人读，不作身份）。
    pub account: String,
    /// 命中账号的 **uid**（写入时的**精确身份**，供 `token_daily` 精确 join）。
    pub account_uid: String,
    /// ★ 调用方的 API Key id（issue #76；`None` = 无 Key 的调用 / 老日志条目）。
    ///
    /// 这是「出了问题能定位是谁用掉的」的**唯一凭据** —— issue #76 的原始动机就是
    /// 「key 全是万能的，不知道谁用的」。同时是「按密钥看用量」的 join 键。
    pub key_id: Option<String>,
    /// ★ 这个 `key_id` 属于**哪套台账**（`"wb"` / `"trae"` / `"qoder"` / `"unified"`）。
    ///
    /// 统一网关把请求转发给产品线中继时，日志里记的是**网关那把 Key 的 id** ——
    /// 与产品线自己的 Key id 混在同一个字段。没有这个标签，产品线的「按密钥看用量」
    /// 会把网关的 Key 当成一把「已删除的本地 Key」（显示成短 id），用户永远对不上。
    pub key_scope: Option<&'static str>,
    pub model: String,
    pub status: u16,
    /// **上游首字节延迟**（请求进入 → 上游响应头到达）。
    ///
    /// ⚠️ 这是 **TTFB，不是总耗时** —— 总耗时另见 [`Self::total_ms`]。两者混用会让
    /// 日志页的耗时列在流式场景下凭空放大（见模块头）。
    pub latency_ms: i64,
    /// **整次请求总耗时**（请求进入 → 流结束 / 响应体读完）。
    ///
    /// 流式路径由 [`StreamingLogSink`] 在流结束时补齐（`template` 里传 0 占位）；
    /// 非流式与失败路径就地填。
    pub total_ms: i64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// **上游上报的积分消耗**（`usage.credit`，与客户端 jsonl 的 `usage.credit` 同单位）。
    ///
    /// 为什么进日志：用量页的「积分」视图要按**来源**拆消耗（客户端 / 网关），
    /// 而 `token_daily` 的网关桶只有 token 没有 credit ⇒ 不落这一项，积分只能靠
    /// token 折算，那是估算不是账单。issue #63。
    ///
    /// `0.0` = 上游没带 credit（不是「免费」，是「不知道」）—— 与 token 写 0 同一诚实口径。
    pub credit: f64,
    pub stream: bool,
    /// ★ issue #129：**流中途被判定失败**时的错误摘要（`None` = 正常结束 / 老条目）。
    ///
    /// 为什么必须有：流式日志的 `status` 在模板里是硬编码的 `200`（上游响应头到达时
    /// 写日志那一刻还不可能知道后面会断流）。没有这个字段，一次「流中途账号故障」
    /// 在日志页会显示成**成功的 200 请求** —— 用户那边看起来一切正常，
    /// 网关的日志与统计却什么也看不见（故障被隐藏了，排查时无从下手）。
    ///
    /// 口径与 Qoder 侧 `record_request` 的 `error: Option<String>` 同形：
    /// 记**上游原文**（含账号名/码），**不是**对外收敛后的文案 —— 对外文案为了能被
    /// 客户端归成可重试而刻意抹掉了真实原因（见 `crate::failover`），
    /// 真实原因只在这里与账号池状态里。
    pub error: Option<String>,
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
            "totalMs": self.total_ms,
            "promptTokens": self.prompt_tokens,
            "completionTokens": self.completion_tokens,
            "credit": self.credit,
            "stream": self.stream,
            // ★ issue #76：调用方的 Key id。**无 Key 时写 `null`**（不是省略这个键）——
            //   键集合固定，前端只需判 `keyId == null`；省略会让「有没有 key 维度」
            //   变成「键存不存在」+「值是不是空」两套判断。
            //   老日志条目（升级前写入）确实没有这个键 ⇒ 前端两种都要容忍。
            "keyId": self.key_id,
            // ★ 归属台账（`null` = 老条目 / 无 Key）。与 `keyId` **同生同空**。
            "keyScope": self.key_scope,
            // ★ issue #129：流中途失败摘要（`null` = 正常结束 / 老条目）。
            //   与 `keyId` 同一纪律：**键固定存在、无值写 `null`**。
            "error": self.error,
        })
    }
}

/// 流式请求的**延迟日志写入器**（见模块头「写入时机」）。
///
/// ## 为什么需要它（2026-10-14 实测驱动）
///
/// 流式路径此前在**流开始前**就把日志写了，那时 `usage` 还没到达 ⇒
/// `promptTokens` / `completionTokens` **只能写死 0**。而出站管线强制上游流式、
/// 客户端（WorkBuddy 桌面 / CLI）默认也流式 ⇒ **实际流量 100% 命中该分支**：
/// 请求日志的 Token 列与「今日 Token」的网关列**恒为 0**（本机 200/200 条全 0）。
///
/// ## 语义
///
/// - **拿不到用量也要写**（填 0）：这次请求**发生过**（状态码 / 账号 / 模型 / 耗时
///   都是事实），不该因为拿不到用量就从日志里消失。
/// - `prompt_tokens` / `completion_tokens` / **`total_ms`** 由本 sink 在流结束时补齐；
///   **`latency_ms` 原样取自 `template`**（handler 在拿到上游响应头时已填实）
///   —— 本 sink **不碰**它，见模块头「两个耗时字段语义不同」。
pub struct StreamingLogSink {
    log: Arc<RequestLog>,
    /// 除 token 与**总**耗时外的全部字段（流开始时即已确定，含 `latency_ms`）。
    template: RequestMeta,
    started: Instant,
    /// ★ issue #129：流中途失败信号（`None` = 未接入 ⇒ 状态码恒取模板里的 200）。
    ///
    /// 必须在 `finish` 里读：`UsageTap` 在**上报之前**已把失败写进这个槽，
    /// 所以「流中途断流」的日志能写真实状态码，而不是硬编码的 200。
    failure: Option<Arc<crate::failover::StreamFailure>>,
}

impl StreamingLogSink {
    /// `template` 的 `prompt_tokens` / `completion_tokens` / `total_ms` 会被覆盖
    /// （传 0 占位即可）；**`latency_ms` 必须填实**（上游响应头到达时的耗时）。
    pub fn new(log: Arc<RequestLog>, template: RequestMeta, started: Instant) -> Self {
        Self {
            log,
            template,
            started,
            failure: None,
        }
    }

    /// ★ issue #129：接入失败信号 —— 流中途被判失败时把 `status` 改成真实状态码
    /// 并附错误摘要，而不是把这次请求记成「200 成功」。
    pub fn with_failure(mut self, failure: Option<Arc<crate::failover::StreamFailure>>) -> Self {
        self.failure = failure;
        self
    }
}

impl UsageSink for StreamingLogSink {
    fn finish(&self, usage: Option<UsageReport>) {
        // 与 `PoolUsageSink` 相反：**没有用量也要落日志**（用量填 0）。
        let (prompt_tokens, completion_tokens, credit) = match usage {
            Some(usage) => (
                u64::try_from(usage.prompt_tokens).unwrap_or(0),
                u64::try_from(usage.completion_tokens).unwrap_or(0),
                usage.credit,
            ),
            None => (0, 0, 0.0),
        };
        // ★ issue #129：流中途失败 ⇒ 日志写**真实**状态码 + 摘要。
        //   不这么做的话，一次「账号在流中途挂掉」在日志页会显示成成功的 200 请求
        //   —— 对外静默的代价绝不能是「对内也静默」。
        let failure = self.failure.as_ref().and_then(|signal| signal.peek());
        let (status, error) = match &failure {
            Some(failure) => (failure.status, Some(failure.detail.clone())),
            None => (self.template.status, None),
        };
        self.log.record(
            RequestMeta {
                prompt_tokens,
                completion_tokens,
                credit,
                status,
                error,
                // ★ 只补总耗时；`latency_ms` 保留 `template` 里的 TTFB —— 覆盖它正是
                //   2026-10-15 那次「日志数字放大 10~100 倍」的事故来源。
                total_ms: self.started.elapsed().as_millis() as i64,
                ..self.template.clone()
            }
            .to_value(),
        );
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

    /// 日志条目必须携带写入时的**精确 uid**（`accountUid`），且 `account`（展示名）语义不变；
    /// 两个**耗时字段必须各自独立**（2026-10-15：合并它们是「数字放大」事故的根因）。
    #[test]
    fn request_meta_carries_account_uid_alongside_display_name() {
        let meta = RequestMeta {
            endpoint: "/v1/chat/completions",
            method: "POST",
            region: Region::Cn,
            account: "重名展示名".to_string(),
            account_uid: "uid-b".to_string(),
            key_id: None,
            key_scope: None,
            model: "hy4".to_string(),
            status: 200,
            latency_ms: 12,
            total_ms: 3456,
            prompt_tokens: 10,
            completion_tokens: 2,
            credit: 0.0,
            stream: false,
            // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
            error: None,
        };

        let value = meta.to_value();
        assert_eq!(value["accountUid"], "uid-b", "必须写入选中账号的精确 uid");
        assert_eq!(
            value["account"], "重名展示名",
            "account 仍是展示名，语义不变"
        );
        assert_eq!(value["latencyMs"], 12, "latencyMs = 首字节延迟（TTFB）");
        assert_eq!(value["totalMs"], 3456, "totalMs = 总耗时");
    }

    /// 日志条目的**键集合钉死** —— 防止字段被悄悄增删（前端按名读，`token_daily` 也读）。
    #[test]
    fn log_entry_key_set_is_pinned() {
        let value = streaming_template().to_value();
        let keys: std::collections::BTreeSet<&str> = value
            .as_object()
            .expect("日志条目是对象")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "ts",
                "endpoint",
                "method",
                "region",
                "account",
                "accountUid",
                "model",
                "status",
                "latencyMs",
                "totalMs",
                "promptTokens",
                "completionTokens",
                "credit",
                "stream",
                // ★ issue #76：调用方 Key id 与**归属台账**（`null` = 无 Key / 老条目）。
                "keyId",
                "keyScope",
                // ★ issue #129：流中途失败摘要。
                "error",
            ]),
            "键集合变了就要同步前端 GatewayLogEntry 与 token_daily 的读法"
        );
    }

    /// 造一个「流式请求」的日志模板。
    ///
    /// `latency_ms` 刻意填 **37**（一个不可能由 `elapsed()` 真实产生的哨兵值）：
    /// sink **必须原样保留**它 —— 这是「TTFB 不被总耗时覆盖」的直接证据。
    /// `total_ms` 与 token 字段是占位，由 sink 在流结束时补齐。
    fn streaming_template() -> RequestMeta {
        RequestMeta {
            endpoint: "/v1/chat/completions",
            method: "POST",
            region: Region::Cn,
            account: "一号".to_string(),
            account_uid: "uid-1".to_string(),
            key_id: None,
            key_scope: None,
            model: "hy4".to_string(),
            status: 200,
            latency_ms: 37,
            total_ms: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            credit: 0.0,
            stream: true,
            // ★ issue #129：流中途失败摘要（`None` = 正常结束 / 老条目）。
            error: None,
        }
    }

    /// 独立临时日志文件（各测各的路径，避免并跑互相踩）。
    fn temp_log(tag: &str) -> (Arc<RequestLog>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("bs-log-sink-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建隔离目录");
        let path = dir.join("gateway_logs.json");
        let _ = std::fs::remove_file(&path);
        (Arc::new(RequestLog::new(path, 50, false)), dir)
    }

    /// ★★ 流式路径的日志必须在**流结束时**带上**真实** token。
    ///
    /// 缺陷现场（2026-10-14）：流式分支把 token 写死 0，本机 200/200 条日志全 0，
    /// 请求日志的 Token 列与「今日 Token」的网关列因此永远是 0。
    ///
    /// 可证伪：把 `StreamingLogSink::finish` 的 `Some(usage)` 分支去掉（一律写 0）
    /// ⇒ 本用例在 `promptTokens` 上变红。
    #[tokio::test]
    async fn streaming_log_sink_writes_real_tokens_at_stream_end() {
        use crate::protocol::usage_tap::UsageTap;
        use futures_util::StreamExt;

        let (log, dir) = temp_log("tokens");
        let sink: Arc<dyn UsageSink> = Arc::new(StreamingLogSink::new(
            log.clone(),
            streaming_template(),
            Instant::now(),
        ));
        let frames = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"credit\":1.5,\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n",
        );
        let stream = futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
            axum::body::Bytes::from_static(frames.as_bytes()),
        )]);
        let _ = UsageTap::new(stream, vec![sink]).collect::<Vec<_>>().await;

        let entries = log.list();
        assert_eq!(entries.len(), 1, "流结束应恰好写一条日志（幂等，不多不少）");
        assert_eq!(
            entries[0]["promptTokens"], 10,
            "流式路径必须写**真实** prompt token —— 写死 0 正是本次修的缺陷"
        );
        assert_eq!(entries[0]["completionTokens"], 5);
        assert_eq!(
            entries[0]["credit"], 1.5,
            "流式路径必须写**上游上报的** credit —— 积分视图按来源拆消耗就靠它"
        );
        assert_eq!(entries[0]["accountUid"], "uid-1", "静态字段不得丢");
        assert_eq!(entries[0]["stream"], true);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★ **TTFB 与总耗时必须各归各的字段**（2026-10-15 事故的回归护栏）。
    ///
    /// 事故：`StreamingLogSink` 第一版在流结束时把 `latencyMs` 覆盖成「总耗时」⇒
    /// 日志页的耗时列凭空放大 **10~100 倍**（实测 p50 8.7 s、p90 204 s），
    /// 用户合理地读成「网关变慢了」。**那次改动没有引入任何真实延迟** ——
    /// 纯粹是字段语义被偷换。
    ///
    /// 本用例用哨兵值 **37** 钉死：sink 只能**新增** `totalMs`，不得改动 `latencyMs`。
    ///
    /// 可证伪：把 `finish` 改回 `latency_ms: self.started.elapsed()…`（即覆盖 template
    /// 里的 TTFB）⇒ `latencyMs != 37` ⇒ 本用例变红。
    #[tokio::test]
    async fn streaming_log_sink_keeps_ttfb_and_adds_total_separately() {
        use crate::protocol::usage_tap::UsageTap;
        use futures_util::StreamExt;

        let (log, dir) = temp_log("ttfb");
        // `started` 刻意早于「现在」一点点，好让 totalMs 有真实的非零值。
        let started = Instant::now();
        let sink: Arc<dyn UsageSink> = Arc::new(StreamingLogSink::new(
            log.clone(),
            streaming_template(),
            started,
        ));
        let stream = futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
            axum::body::Bytes::from_static(
                "data: {\"choices\":[],\"usage\":{\"credit\":1.0,\"prompt_tokens\":3,\"completion_tokens\":4}}\n\n"
                    .as_bytes(),
            ),
        )]);
        let _ = UsageTap::new(stream, vec![sink]).collect::<Vec<_>>().await;

        let entries = log.list();
        assert_eq!(entries.len(), 1, "应恰好写一条日志");
        assert_eq!(
            entries[0]["latencyMs"], 37,
            "latencyMs 必须**原样保留** handler 填入的 TTFB —— sink 覆盖它正是「数字放大」\
             事故的根因（日志页的耗时列会从数百 ms 跳到数百 s）"
        );
        assert!(
            entries[0]["totalMs"]
                .as_i64()
                .is_some_and(|value| value >= 0),
            "totalMs 必须由 sink 在流结束时补齐，实际：{}",
            entries[0]["totalMs"]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ 流被**掐断**（客户端读到一半断开 / 响应体从未被消费）时也必须留下一条日志。
    ///
    /// 这是「日志推迟到流结束」这个改动的**安全网**：少了它，被掐断的请求会从日志里
    /// 彻底消失（比原来「写一条 token=0」更糟 —— 连「发生过」都看不到）。
    ///
    /// 可证伪：删掉 `impl Drop for UsageTap` ⇒ 本用例变红（`entries` 为空）。
    #[tokio::test]
    async fn streaming_log_sink_still_writes_when_the_stream_is_dropped_early() {
        use crate::protocol::usage_tap::UsageTap;

        let (log, dir) = temp_log("dropped");
        let sink: Arc<dyn UsageSink> = Arc::new(StreamingLogSink::new(
            log.clone(),
            streaming_template(),
            Instant::now(),
        ));
        {
            // 空流且从未被 poll ⇒ 只能靠 Drop 兜底。
            let _tap = UsageTap::new(
                futures_util::stream::iter(Vec::<Result<axum::body::Bytes, std::io::Error>>::new()),
                vec![sink],
            );
        }

        let entries = log.list();
        assert_eq!(entries.len(), 1, "被丢弃的流也必须留下一条日志");
        assert_eq!(
            entries[0]["promptTokens"], 0,
            "用量未知 ⇒ 记 0（而不是丢日志）"
        );
        assert_eq!(entries[0]["accountUid"], "uid-1");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
