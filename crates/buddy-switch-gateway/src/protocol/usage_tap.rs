//! 流式响应的 `usage` 探针：从上游 SSE 里抓取末帧用量并回填成本账本。
//!
//! # 为什么需要它
//!
//! 出站改写管线**强制 `stream: true`**（上游不接受非流式），客户端默认也是流式；
//! 而请求日志与成本账本此前**只在非流式客户端路径**上回填（见 `routes/chat.rs` 的
//! 聚合分支）。后果是：在实际流量上
//! - 账本几乎永远为空 → `cost_tier` 恒为「无观测」→ **选号的成本分层硬过滤退化成
//!   空操作**，整个「账本择优」能力形同未启用；
//! - 请求日志的 `promptTokens` / `completionTokens` **只能写死 0**（写日志时用量还没
//!   到达）⇒ 请求日志的 Token 列与「今日 Token」的网关列**恒为 0**
//!   （2026-10-14 实测本机 200/200 条全为 0）。
//!
//! # 设计约束
//!
//! - 探针**嵌在** [`super::openai::SsePassthrough`] **之内**（更靠近上游），只观察字节、
//!   不修改字节，因此透传语义与「恰好补发一个 `[DONE]`」的保证完全不受影响。
//! - 探针在**流结束时**（正常结束 / 上游出错 / 流被丢弃）向每个 sink 回报**恰好一次**
//!   （幂等）。「结束」而不是「开始」是刻意的：用量只在末帧才出现。
//! - 回报是**尽力而为**：拿不到池的写锁就跳过本次样本，绝不阻塞流式响应。
//! - **`None` 的处置由 sink 自己决定**（见 [`UsageSink::finish`]）：账本类跳过、
//!   日志类照写。探针不替它们做取舍。

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Bytes;
use futures_util::Stream;
use serde_json::Value;

use crate::protocol::error_sniffer::sniff_sse_chunk;

/// 单帧缓冲区上限：畸形流（永不出现 `\n\n`）不得让内存无界增长。
const MAX_FRAME_BUFFER_BYTES: usize = 256 * 1024;

/// 一次上游用量回报。
///
/// `prompt` / `completion` **分开保留**（而不是只给总和）：请求日志要写两个独立字段，
/// 只给总和就再也拆不开了。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsageReport {
    /// 上游上报的积分消耗（`usage.credit`）。
    pub credit: f64,
    /// 输入 token（`usage.prompt_tokens`，负数已钳到 0）。
    pub prompt_tokens: i64,
    /// 输出 token（`usage.completion_tokens`，负数已钳到 0）。
    pub completion_tokens: i64,
}

impl UsageReport {
    /// 输入 + 输出（账本按总量折算单价）。
    pub fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// 用量回报接收方。
///
/// 抽象成 trait 是为了让单测用记录型替身，不必真的起账号池 / 日志文件；
/// 与 `cat.rs` 的 `CatIo` / `school.rs` 的 `SchoolIo` 是同一套可注入缝思路。
pub trait UsageSink: Send + Sync + 'static {
    /// 流结束时调用**恰好一次**。
    ///
    /// `usage` 为 `None` 表示本次**未观测到可用用量**（上游没带 `usage` / 缺 `credit` /
    /// tokens 为 0 / 流被客户端中途掐断）。两类 sink 对 `None` 的处置**必须分家**：
    ///
    /// - **账本类**（`PoolUsageSink`）：**跳过**。写一个 0 成本样本会把账号误标成
    ///   「免费」，比没有样本更糟（会误导成本分层）。
    /// - **日志类**（`StreamingLogSink`）：**照写**（用量填 0）。「这次请求发生过」
    ///   本身就是事实，不该因为拿不到用量就从日志里消失。
    fn finish(&self, usage: Option<UsageReport>);
}

/// 从一帧 JSON 里提取用量。
///
/// 返回 `None` 的情形（都**不得**写入账本）：无 `usage` 字段、`usage` 为 `null`、
/// **缺 `credit`**（拿不到单价就得不出成本）、`prompt + completion <= 0`。
///
/// ⚠️ 显式 `credit = 0` 是**合法**样本（免费模型），与「缺 `credit`」是两件事。
pub fn extract_usage(frame: &Value) -> Option<UsageReport> {
    let usage = frame.get("usage")?;
    if usage.is_null() {
        return None;
    }
    let credit = usage.get("credit").and_then(Value::as_f64)?;
    let prompt = usage
        .get("prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let completion = usage
        .get("completion_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    if prompt + completion <= 0 {
        return None;
    }
    Some(UsageReport {
        credit,
        prompt_tokens: prompt,
        completion_tokens: completion,
    })
}

/// **流内错误**回调（P0-3 的流式半边）。
///
/// `UsageTap` 本来就在**逐帧**旁听每一块字节（为了找 `usage`），顺手判一下
/// 「这一帧是不是错误信封」是零额外成本的 —— 不需要再套一层 Stream 适配器，
/// 也就不需要改动三条路由的流式组合链（字节一个都不变，只旁听）。
///
/// ## 为什么回调是同步的
///
/// `poll_next` 里不能 `.await`。真实实现（`relay::EnvelopeErrorSink`）把治理副作用
/// `tokio::spawn` 到后台任务里，回调本身只做一次投递。
pub trait StreamErrorSink: Send + Sync {
    /// 发现一帧信封错误（**同一路流只会回调一次**）。
    fn on_error(&self, error: crate::protocol::error_sniffer::SniffedError);
}

/// `usage` 探针流包装。**不改变字节流**，只在流结束（或出错、或被丢弃）时回报一次用量。
pub struct UsageTap<S> {
    inner: Pin<Box<S>>,
    /// 回报接收方（日志 / 账本各一个；空则只做缓冲，等价于不加探针）。
    sinks: Vec<Arc<dyn UsageSink>>,
    /// 流内错误接收方（可选）：某一帧是错误信封时回调一次。
    error_sink: Option<Arc<dyn StreamErrorSink>>,
    /// ★ issue #129：流中途失败信号（可选）。命中错误帧 / 上游断流时写一次，
    /// 供协议出口（要不要补成功标记）与请求日志（写真实状态码）读取。
    ///
    /// 嗅探**只在这一处做**：三个消费者各自判一次必然漂移。
    failure: Option<Arc<crate::failover::StreamFailure>>,
    /// 已经回调过错误（同一条流只报一次；错误帧之后通常就是断流）。
    error_reported: bool,
    buffer: Vec<u8>,
    /// 最近一次见到的用量（后到的帧覆盖先到的）。
    latest: Option<UsageReport>,
    reported: bool,
    finished: bool,
}

impl<S> UsageTap<S> {
    /// 包装上游流；`sinks` 为空时只做缓冲（等价于不加探针）。
    pub fn new(inner: S, sinks: Vec<Arc<dyn UsageSink>>) -> Self {
        Self {
            inner: Box::pin(inner),
            sinks,
            error_sink: None,
            failure: None,
            error_reported: false,
            buffer: Vec::new(),
            latest: None,
            reported: false,
            finished: false,
        }
    }

    /// 挂一个**流内错误**回调（`None` = 不嗅探，行为与改造前逐字相同）。
    ///
    /// 用独立构造器而不是改 `new` 的签名：三条路由与既有单测都不用动，
    /// 「要不要治理流内错误」成为一个显式选择。
    pub fn with_error_sink(mut self, sink: Option<Arc<dyn StreamErrorSink>>) -> Self {
        self.error_sink = sink;
        self
    }

    /// ★ issue #129：挂**失败信号**槽（`None` = 不记录，行为与改造前逐字相同）。
    ///
    /// 开关关闭时调用方仍可传槽（`StreamFailure::new(false)`），此时 `record`
    /// 内部直接丢弃 —— 关闭态连一次写锁都不做。
    pub fn with_failure(mut self, failure: Option<Arc<crate::failover::StreamFailure>>) -> Self {
        self.failure = failure;
        self
    }

    /// 累积字节并解析所有**完整**帧（以空行分隔）；残帧留在缓冲区等下一块。
    fn scan(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        // ★ CRLF 归一（只动**内部缓冲**，透传字节一个不改）。
        //
        // SSE 允许 `\r\n` 作行结束，而帧分隔符就成了 `\r\n\r\n` —— 里面**不含** `\n\n`
        // （是 `\n` `\r` `\n`），于是用 `\n\n` 找帧的上游**永远切不出帧**：
        // usage 解析不到、流内错误也嗅不到。把 `\r` 从缓冲里抹掉，
        // `\r\n\r\n` 就变成 `\n\n`，两种换行风格走同一条路径。
        //
        // 为什么可以安全抹掉：这段缓冲只用于解析（JSON 里的回车会转义成 `\\r`，
        // 不会以裸字节出现）；客户端收到的仍是原始字节。
        if self.buffer.contains(&b'\r') {
            self.buffer.retain(|byte| *byte != b'\r');
        }
        while let Some(position) = find_subslice(&self.buffer, b"\n\n") {
            let frame: Vec<u8> = self.buffer.drain(..position + 2).collect();
            self.observe_frame(&frame);
        }
        if self.buffer.len() > MAX_FRAME_BUFFER_BYTES {
            self.buffer.clear();
        }
    }

    /// 流结束时把**缓冲区残帧**也判一遍（`/` 结尾空行缺失时的错误帧）。
    ///
    /// 只有「以 `\n\n` 结尾」的帧才会进 `observe_frame`；上游把错误帧写完后直接关连接时
    /// 残帧会留在缓冲区里被丢掉 —— 那正是最需要识别的最后一帧。
    fn sniff_tail(&mut self) {
        if self.error_reported || self.buffer.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.buffer).to_string();
        if let Some(found) = sniff_sse_chunk(&text) {
            self.report_error(found);
        }
    }

    /// 统一出口：记一次「这一帧是错误信封」。
    ///
    /// 两个消费者各取所需（`failure` 供出口/日志判收尾，`error_sink` 做治理），
    /// 但**判定只做一次** —— 分开判会让「出口认为失败、日志认为成功」这种漂移成为可能。
    fn report_error(&mut self, found: crate::protocol::error_sniffer::SniffedError) {
        self.error_reported = true;
        if let Some(failure) = self.failure.as_ref() {
            failure.record(found.event.clone(), found.message.clone());
        }
        if let Some(sink) = self.error_sink.as_ref() {
            sink.on_error(found);
        }
    }

    /// ★ issue #129 §3.3：上游**断流**（`Err`）也算一次账号级失败。
    ///
    /// 改造前这里只 `report()`：连接被 reset 这种最常见的断流既不给客户端报错、
    /// 也不给账号写冷却 ⇒ 下一次选号那个号仍然「可用」，客户端即便重试也会再撞同一堵墙。
    ///
    /// 归为 `Server`（走熔断域）而不是 `SessionDead`：网络抖动不该被升级成
    /// 「禁用账号」；只有开关开启时才上报，关闭态保持原行为。
    fn note_upstream_break<E: std::fmt::Debug>(&mut self, error: &E) {
        // 错误帧已经报过（帧在前、断流在后）：不要再把同一次事故记两遍。
        if self.error_reported {
            return;
        }
        let Some(failure) = self.failure.as_ref().filter(|signal| signal.is_enabled()) else {
            return;
        };
        let detail = format!("上游连接中断：{error:?}");
        failure.record(crate::pool::UpstreamEvent::Server, detail.clone());
        self.error_reported = true;
        if let Some(sink) = self.error_sink.as_ref() {
            sink.on_error(crate::protocol::error_sniffer::SniffedError {
                event: crate::pool::UpstreamEvent::Server,
                message: detail,
                status: 502,
            });
        }
    }

    /// 解析单帧里的 `data:` 负载，命中 `usage` 就记住。
    fn observe_frame(&mut self, frame: &[u8]) {
        let text = String::from_utf8_lossy(frame);
        // ★ 流内错误（P0-3）：同一帧缓存窗口里顺手判一次，**不改字节**。
        //   放在取 usage 之前：错误帧通常没有 usage，早点返回能让「流内失败」更快落账。
        if !self.error_reported {
            if let Some(found) = sniff_sse_chunk(&text) {
                self.report_error(found);
            }
        }
        for line in text.lines() {
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(payload) else {
                continue;
            };
            if let Some(usage) = extract_usage(&value) {
                self.latest = Some(usage);
            }
        }
    }

    /// 回报一次（幂等）。
    fn report(&mut self) {
        if self.reported {
            return;
        }
        self.reported = true;
        for sink in &self.sinks {
            sink.finish(self.latest);
        }
    }
}

impl<S> Drop for UsageTap<S> {
    /// 流**被丢弃**（客户端中途断开 / 响应体从未被消费）时也要回报一次。
    ///
    /// 少了它，被掐断的请求会从日志里**彻底消失** —— 连一条「发生过」的记录都没有。
    /// 正常结束与出错路径已在 `poll_next` 里回报过，这里由 `reported` 幂等挡住。
    fn drop(&mut self) {
        self.report();
    }
}

impl<S, E> Stream for UsageTap<S>
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::fmt::Debug,
{
    type Item = Result<Bytes, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.scan(&chunk);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.finished = true;
                // ★ issue #129 §3.3：断流也算账号级失败（开关开启时）——
                //   必须在 `report()` **之前**，日志 sink 要在同一次回调里读到失败。
                this.note_upstream_break(&error);
                this.report();
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                this.finished = true;
                // ★ 流结束前把**残帧**也判一遍：上游常把最后一条错误帧写完后直接关连接，
                //   不给结尾空行 —— 只按「完整帧」扫会漏掉它。这条与 `report()` 的
                //   残帧解析同一纪律：结束时的缓冲区不是垃圾，是**最后一条**。
                this.sniff_tail();
                this.report();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// 子串查找（避免为此引入额外依赖）。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use serde_json::json;
    use std::sync::Mutex;

    /// 记录型替身：把每次 `finish` 的入参**原样**存下来（含 `None`）。
    #[derive(Default)]
    struct RecordingSink {
        calls: Mutex<Vec<Option<UsageReport>>>,
    }

    impl RecordingSink {
        fn calls(&self) -> Vec<Option<UsageReport>> {
            self.calls.lock().expect("lock").clone()
        }
    }

    impl UsageSink for RecordingSink {
        fn finish(&self, usage: Option<UsageReport>) {
            self.calls.lock().expect("lock").push(usage);
        }
    }

    /// 单个 sink → `UsageTap::new` 需要的 `Vec`。
    fn only(sink: &Arc<RecordingSink>) -> Vec<Arc<dyn UsageSink>> {
        vec![sink.clone()]
    }

    /// 记录型**错误**接收方：钉住「命中几帧、报了几次」。
    #[derive(Default)]
    struct RecordingErrorSink {
        calls: Mutex<Vec<crate::protocol::error_sniffer::SniffedError>>,
    }

    impl StreamErrorSink for RecordingErrorSink {
        fn on_error(&self, error: crate::protocol::error_sniffer::SniffedError) {
            self.calls.lock().expect("lock").push(error);
        }
    }

    /// ★ 流内错误：命中即回调，**只回调一次**，且**字节一块不少**地透传。
    ///
    /// 三条断言分别防三种退化：
    /// 1. 回调不触发（等于没做）；
    /// 2. 逐帧重复回调（一条流里错误帧可能重复出现，重复写冷却会放大退避）；
    /// 3. 顺手改字节（哪怕只是丢帧，客户端就会看到断流）。
    #[tokio::test]
    async fn error_frame_is_reported_once_and_bytes_pass_through() {
        let sink = Arc::new(RecordingErrorSink::default());
        let mut tap = UsageTap::new(
            stream_of(vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "data: {\"code\":6004,\"msg\":\"rate limited\"}\n\n",
                "data: {\"code\":6004,\"msg\":\"again\"}\n\n",
            ]),
            Vec::new(),
        )
        .with_error_sink(Some(sink.clone() as Arc<dyn StreamErrorSink>));

        let mut forwarded = 0;
        while let Some(item) = tap.next().await {
            item.expect("上游本身没出错");
            forwarded += 1;
        }
        assert_eq!(forwarded, 3, "字节块必须原样透传");
        let calls = sink.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1, "错误只回调一次");
        assert!(calls[0].message.contains("rate limited"));
    }

    /// ★ CRLF 上游（`\r\n\r\n` 分隔）必须照样切出帧。
    ///
    /// 变异：删掉 `scan` 里的 `\r` 归一 ⇒ 本用例红（帧切不出来 ⇒ 错误嗅不到、
    /// usage 也解析不到 —— 客户端收到的字节仍然是对的，所以这种缺陷**永远不会报错**）。
    #[tokio::test]
    async fn crlf_frames_are_split_and_sniffed() {
        let sink = Arc::new(RecordingErrorSink::default());
        // 注意：这是 `\r\n` 结尾的 SSE，帧分隔符是 `\r\n\r\n`。
        let crlf = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\n",
            "data: {\"code\":6004,\"msg\":\"crlf limited\"}\r\n\r\n",
        );
        let mut tap = UsageTap::new(stream_of(vec![crlf]), Vec::new())
            .with_error_sink(Some(sink.clone() as Arc<dyn StreamErrorSink>));
        while tap.next().await.is_some() {}
        let calls = sink.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1, "CRLF 帧必须被切出并识别");
        assert!(calls[0].message.contains("crlf limited"));
    }

    /// 没有错误帧 ⇒ 一次都不回调（避免把正常流误判成失败、白白冷却好账号）。
    #[tokio::test]
    async fn clean_stream_never_triggers_the_error_sink() {
        let sink = Arc::new(RecordingErrorSink::default());
        let mut tap = UsageTap::new(
            stream_of(vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "data: [DONE]\n\n",
            ]),
            Vec::new(),
        )
        .with_error_sink(Some(sink.clone() as Arc<dyn StreamErrorSink>));
        while tap.next().await.is_some() {}
        assert!(sink.calls.lock().expect("lock").is_empty());
    }

    /// ★ 末端**残帧**（没有结尾空行）也必须识别：上游写完错误帧常常直接关连接。
    ///
    /// 变异：去掉 `poll_next` 里的 `sniff_tail()` ⇒ 本用例红。
    #[tokio::test]
    async fn trailing_frame_without_blank_line_is_still_detected() {
        let sink = Arc::new(RecordingErrorSink::default());
        let mut tap = UsageTap::new(
            stream_of(vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                // 没有结尾的 `\n\n` —— 残帧
                "data: {\"code\":6004,\"msg\":\"cut off mid-stream\"}",
            ]),
            Vec::new(),
        )
        .with_error_sink(Some(sink.clone() as Arc<dyn StreamErrorSink>));
        while tap.next().await.is_some() {}
        let calls = sink.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1, "残帧里的错误必须被识别");
        assert!(calls[0].message.contains("cut off mid-stream"));
    }

    /// ★★ issue #129 §3.3：上游**断流**也必须写失败信号并上报治理。
    ///
    /// 改造前 `Err` 分支只 `report()`：连接被 reset 这种最常见的断流**既不报错、
    /// 也不写冷却** ⇒ 下次选号那个号仍然「可用」，客户端即便重试也会再撞同一堵墙。
    ///
    /// 可证伪：删掉 `poll_next` 里的 `note_upstream_break` ⇒ 本用例两条断言全红。
    #[tokio::test]
    async fn upstream_break_records_a_server_failure_and_reports_it() {
        let sink = Arc::new(RecordingErrorSink::default());
        let signal = crate::failover::StreamFailure::new(true);
        let stream = futures_util::stream::iter(vec![
            Ok(Bytes::from_static(USAGE_FRAME.as_bytes())),
            Err(std::io::Error::other("connection reset")),
        ]);
        let mut tap = UsageTap::new(stream, Vec::new())
            .with_error_sink(Some(sink.clone() as Arc<dyn StreamErrorSink>))
            .with_failure(Some(signal.clone()));
        while tap.next().await.is_some() {}

        let failure = signal.peek().expect("断流必须写进失败信号");
        assert_eq!(
            failure.event,
            crate::pool::UpstreamEvent::Server,
            "断流归 Server（熔断域），不得升级成 SessionDead（会禁用账号）"
        );
        assert_eq!(failure.status, 502);
        let calls = sink.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1, "断流必须上报一次治理（写冷却）");
        assert_eq!(calls[0].status, 502);
    }

    /// ★ 开关**关闭**时断流不写信号、也不上报（关闭态行为与改造前逐字一致）。
    #[tokio::test]
    async fn upstream_break_is_ignored_when_failover_is_off() {
        let sink = Arc::new(RecordingErrorSink::default());
        let signal = crate::failover::StreamFailure::new(false);
        let stream = futures_util::stream::iter(vec![
            Ok(Bytes::from_static(USAGE_FRAME.as_bytes())),
            Err(std::io::Error::other("connection reset")),
        ]);
        let mut tap = UsageTap::new(stream, Vec::new())
            .with_error_sink(Some(sink.clone() as Arc<dyn StreamErrorSink>))
            .with_failure(Some(signal.clone()));
        while tap.next().await.is_some() {}
        assert!(signal.peek().is_none(), "关闭时不得记录");
        assert!(
            sink.calls.lock().expect("lock").is_empty(),
            "关闭时不得上报"
        );
    }

    fn report(credit: f64, prompt: i64, completion: i64) -> Option<UsageReport> {
        Some(UsageReport {
            credit,
            prompt_tokens: prompt,
            completion_tokens: completion,
        })
    }

    fn stream_of(chunks: Vec<&'static str>) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        futures_util::stream::iter(
            chunks
                .into_iter()
                .map(|text| Ok(Bytes::from_static(text.as_bytes()))),
        )
    }

    const USAGE_FRAME: &str =
        "data: {\"choices\":[],\"usage\":{\"credit\":1.5,\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n";

    #[tokio::test]
    async fn reports_usage_once_at_stream_end() {
        let sink = Arc::new(RecordingSink::default());
        let stream = stream_of(vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            USAGE_FRAME,
            "data: [DONE]\n\n",
        ]);
        let collected: Vec<_> = UsageTap::new(stream, only(&sink)).collect().await;
        assert_eq!(collected.len(), 3, "字节必须原样透传，一帧不少");
        assert_eq!(
            sink.calls(),
            vec![report(1.5, 10, 5)],
            "prompt / completion 必须**分开**回报（请求日志要写两个独立字段）"
        );
    }

    #[tokio::test]
    async fn reassembles_frames_split_across_chunks() {
        let sink = Arc::new(RecordingSink::default());
        // 帧被切在中间（含切在 `\n\n` 边界之间），探针必须能重组
        let stream = stream_of(vec![
            "data: {\"choices\":[],\"us",
            "age\":{\"credit\":2.0,\"prompt_tokens\":7,\"completion_tokens\":3}}",
            "\n\ndata: [DONE]\n\n",
        ]);
        let _ = UsageTap::new(stream, only(&sink)).collect::<Vec<_>>().await;
        assert_eq!(sink.calls(), vec![report(2.0, 7, 3)], "跨块帧必须被重组");
    }

    #[tokio::test]
    async fn last_usage_wins_when_multiple_frames_carry_it() {
        let sink = Arc::new(RecordingSink::default());
        let stream = stream_of(vec![
            "data: {\"usage\":{\"credit\":1.0,\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            "data: {\"usage\":{\"credit\":9.0,\"prompt_tokens\":10,\"completion_tokens\":10}}\n\n",
        ]);
        let _ = UsageTap::new(stream, only(&sink)).collect::<Vec<_>>().await;
        assert_eq!(
            sink.calls(),
            vec![report(9.0, 10, 10)],
            "后到的用量应覆盖先到的"
        );
    }

    /// ★ 没有可用用量时**仍要回报一次 `None`** —— 日志类 sink 靠它记下「这次请求发生过」。
    ///
    /// 可证伪：把 `report()` 改回「只在 `latest.is_some()` 时调 sink」⇒ 本用例变红
    /// （`calls` 会是空 Vec 而不是 `[None]`）。
    #[tokio::test]
    async fn reports_none_when_no_usable_usage_was_observed() {
        for chunks in [
            vec!["data: {\"choices\":[]}\n\n"],
            vec!["data: {\"usage\":null}\n\n"],
            // 缺 credit：拿不到单价，不得写账本样本
            vec!["data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n"],
            // tokens 为 0：不得写账本样本
            vec!["data: {\"usage\":{\"credit\":1.0,\"prompt_tokens\":0,\"completion_tokens\":0}}\n\n"],
            // 非 JSON 帧
            vec!["data: not json\n\n"],
        ] {
            let sink = Arc::new(RecordingSink::default());
            let _ = UsageTap::new(stream_of(chunks), only(&sink))
                .collect::<Vec<_>>()
                .await;
            assert_eq!(
                sink.calls(),
                vec![None],
                "无可用用量时必须回报一次 None（由 sink 决定「账本跳过 / 日志照写」）"
            );
        }
    }

    #[tokio::test]
    async fn works_without_sink_and_still_passes_bytes_through() {
        let stream = stream_of(vec![USAGE_FRAME, "data: [DONE]\n\n"]);
        let collected: Vec<_> = UsageTap::new(stream, Vec::new()).collect().await;
        assert_eq!(collected.len(), 2);
        assert!(collected.iter().all(|item| item.is_ok()));
    }

    #[tokio::test]
    async fn upstream_error_still_reports_and_propagates() {
        let sink = Arc::new(RecordingSink::default());
        let stream = futures_util::stream::iter(vec![
            Ok(Bytes::from_static(USAGE_FRAME.as_bytes())),
            Err(std::io::Error::other("upstream boomed")),
        ]);
        let collected: Vec<_> = UsageTap::new(stream, only(&sink)).collect().await;
        assert_eq!(collected.len(), 2, "错误必须原样向上游调用方传播");
        assert!(collected[1].is_err());
        assert_eq!(
            sink.calls(),
            vec![report(1.5, 10, 5)],
            "流中途异常时已见到的用量仍应回报（否则该次调用永远不记账）"
        );
    }

    /// ★★ 流被**丢弃**（客户端读到一半就断开）时也必须回报一次。
    ///
    /// 这是「日志推迟到流结束」这个改动的**安全网**：没有它，被掐断的请求会从日志里
    /// **彻底消失** —— 比原来「写一条 token=0」更糟（连「发生过」都看不到）。
    ///
    /// 可证伪：删掉 `impl Drop for UsageTap` ⇒ 本用例变红（`calls` 为空）。
    #[tokio::test]
    async fn dropping_a_half_consumed_stream_still_reports_once() {
        let sink = Arc::new(RecordingSink::default());
        let stream = stream_of(vec![USAGE_FRAME, "data: [DONE]\n\n"]);
        {
            let mut tap = UsageTap::new(stream, only(&sink));
            // 只取第一块（模拟客户端读到一半就断开），随后 tap 离开作用域被 drop。
            assert!(tap.next().await.is_some(), "第一块应正常取出");
        }
        assert_eq!(
            sink.calls(),
            vec![report(1.5, 10, 5)],
            "流被掐断时也必须回报已见到的用量"
        );
    }

    /// 从未被 poll 的流（响应体被直接丢弃）同样要回报 —— 否则「请求发生过」会丢失。
    #[tokio::test]
    async fn never_polled_stream_still_reports_none() {
        let sink = Arc::new(RecordingSink::default());
        {
            let _tap = UsageTap::new(stream_of(vec![USAGE_FRAME]), only(&sink));
        }
        assert_eq!(
            sink.calls(),
            vec![None],
            "从未被 poll 的流也要回报一次（用量未知 ⇒ None）"
        );
    }

    #[test]
    fn extract_usage_rejects_degenerate_payloads() {
        assert_eq!(extract_usage(&json!({})), None);
        assert_eq!(extract_usage(&json!({"usage": null})), None);
        assert_eq!(
            extract_usage(&json!({"usage": {"prompt_tokens": 1, "completion_tokens": 1}})),
            None,
            "缺 credit 必须拒绝"
        );
        assert_eq!(
            extract_usage(&json!({"usage": {"credit": 1.0}})),
            None,
            "tokens 为 0 必须拒绝"
        );
        // 负数 token 被钳到 0；另一项非 0 ⇒ 仍是合法样本
        assert_eq!(
            extract_usage(
                &json!({"usage": {"credit": 1.0, "prompt_tokens": -5, "completion_tokens": 2}})
            ),
            Some(UsageReport {
                credit: 1.0,
                prompt_tokens: 0,
                completion_tokens: 2
            }),
            "负数项应被钳制为 0，而不是让求和变成负数"
        );
        // 只有 prompt_tokens 也应可用
        assert_eq!(
            extract_usage(&json!({"usage": {"credit": 0.0, "prompt_tokens": 3}})),
            Some(UsageReport {
                credit: 0.0,
                prompt_tokens: 3,
                completion_tokens: 0
            }),
            "显式 credit=0 是合法样本（免费模型），不同于「缺 credit」"
        );
    }

    #[test]
    fn find_subslice_handles_boundaries() {
        assert_eq!(find_subslice(b"a\n\nb", b"\n\n"), Some(1));
        assert_eq!(find_subslice(b"abc", b"\n\n"), None);
        assert_eq!(find_subslice(b"", b"\n\n"), None);
        assert_eq!(find_subslice(b"abc", b""), None);
    }
}
