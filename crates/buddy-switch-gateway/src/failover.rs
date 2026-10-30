//! **账号故障自动切换**（issue #129）：把「流中途的账号故障」从「伪装成功」改成
//! 「客户端可重试的错误」，好让客户端整步重跑、网关下一轮换到别的账号。
//!
//! # 为什么需要这一层
//!
//! 三条流式出口（`chat` / `messages` / `responses`）在上游流结束或出错时都会补
//! 「终止标记」（`data: [DONE]` / `message_stop` / `response.completed`），
//! 否则客户端会挂在等流结束。问题在于**它无条件补**：账号故障和「模型正常说完」
//! 被压成了同一个字节序列 ⇒ 客户端认为成功，既不报错也不重试，用户只看到半句话；
//! 工具调用场景下更糟 —— 客户端会拿**参数残缺**的 tool-call 去执行。
//!
//! 字面意义的「静默续写」（A 号写一半、B 号接着写）在 OpenAI 兼容协议上不成立
//! （没有 assistant 前缀续写，半截 `tool_calls` 的 JSON 也无法拼接）。能成立的等价形态是
//! **换号 + 客户端整步重跑**：网关把这次失败变成可重试错误，并保证重试落到别的账号。
//!
//! # 对外错误形状是**协议契约**，不是文案
//!
//! 客户端（DeepSeek Harness）的可重试码集合固定为 `EMPTY_RESPONSE` / `RATE_LIMIT` /
//! `SERVER` / `TIMEOUT` / `TRANSPORT`，而 `openai-completions` 适配器**只做文本正则**：
//! `429` / `rate limit` → `RATE_LIMIT`，`5\d\d` → `SERVER`，`401|403` → `AUTH`（终态）。
//!
//! 由此三条硬约束（改文案前务必读）：
//!
//! 1. `error.message` 里**必须出现等价 HTTP 状态码**（`HTTP 429` / `HTTP 502`）——
//!    上游私有码（`6004` / `11102` / `12153`）一个都命中不了那组正则；
//! 2. **不能出现配额类短语**（「余额不足」/`insufficient quota`/`out of credits` 等）：
//!    `QUOTA` 的判定排在 `RATE_LIMIT` **之前**且**不可重试** ⇒ 如实说明余额不足
//!    反而会毁掉重试。真实原因由账号池状态与请求日志承载，那里没有正则约束；
//! 3. **不能把 401/403 形状发到流中途**：`AUTH` 是一次终态，UI 还会渲染成
//!    「API 密钥无效」—— 而流中途的凭据问题完全可能只是「这一个号过期了」。
//!
//! # 默认关闭
//!
//! 开关开启会改变对外字节语义（从「假成功」变成「显式可重试错误」），属于可感知的
//! 行为变更 ⇒ 默认 `false`，不勾选时行为与改造前**逐字一致**。

use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use serde_json::json;

use crate::pool::UpstreamEvent;

/// 对外错误类别（三条入站协议共用的**唯一**判定点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    /// 对外等价 HTTP 状态码（写进 `message` 与日志）。
    pub status: u16,
    /// 标准 `error.type`（让将来客户端改成按 `type` 分类时自动生效）。
    pub kind: &'static str,
    /// 对外文案。**必定**含 `HTTP <status>`，且**不含**任何配额类短语。
    pub message: String,
}

/// 上游事件 → 对外错误形状。
///
/// ## 收敛口径（issue #129 §3.4 / §5.2 改动 2）
///
/// | 上游事件 | 对外 | 客户端归类 | 可重试 |
/// |:--|:--|:--|:--|
/// | 限流 / 余额不足 / 会话失效 / 凭据被拒 | `429 rate_limit_error` | `RATE_LIMIT` | 是 |
/// | 5xx / 超时 / 断流 | `502 api_error` | `SERVER` | 是 |
/// | 参数错 / 模型封禁 / 404 | `400 invalid_request_error` | `INVALID_REQUEST` | 否 |
///
/// 前两行是「换号 + 重跑」能救的；第三行换号也无意义（同一份坏参数喂给下一个号只会
/// 白烧额度），因此**刻意**让它落进不可重试集合。
///
/// `account` / `model` 只用于人读的定位信息；`account` 为空时（没选到号）省略该段。
pub fn client_error(event: &UpstreamEvent, account: &str, model: &str) -> ClientError {
    let (status, kind, what) = match event {
        // ★ 三种内部事件（账号级 429 / 余额不足 / 会话失效）口径完全不同，
        //   但对客户端**必须一样**：都是「这个号现在不行了，换一个号就能继续」。
        //   原样透出 402 或 12153 只会让客户端归不了类（`PI_AI_ERROR`，终态）。
        UpstreamEvent::SoftRate { .. }
        | UpstreamEvent::HardCredit
        | UpstreamEvent::SessionDead
        | UpstreamEvent::AuthRejected => (429u16, "rate_limit_error", "账号不可用"),
        UpstreamEvent::Server | UpstreamEvent::Timeout => (502u16, "api_error", "上游连接中断"),
        UpstreamEvent::ModelBlocked => (400u16, "invalid_request_error", "模型被上游拒绝"),
        UpstreamEvent::NotFound => (400u16, "invalid_request_error", "上游未找到该模型"),
        UpstreamEvent::Client => (400u16, "invalid_request_error", "上游拒绝了本次请求"),
    };

    // ★ 文案里**不许**出现配额类短语（见模块头第 2 条）。这里刻意用中性措辞，
    //   真实原因（`reason` / `cool_kind`）只写进账号池与请求日志。
    let who = if account.is_empty() {
        String::new()
    } else {
        format!("账号「{account}」")
    };
    let message = if model.is_empty() {
        format!("{what}（HTTP {status}）：{who}本次请求失败，网关已尝试其它账号")
    } else {
        format!("{what}（HTTP {status}）：{who}在模型 {model} 上失败，网关已尝试其它账号")
    };

    ClientError {
        status,
        kind,
        message,
    }
}

/// 一次流中途失败的记录（对外形状 + 上游原因）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// 上游事件（决定对外状态码与治理动作）。
    pub event: UpstreamEvent,
    /// 上游原文原因（**只进日志/账号池**，不进对外文案）。
    pub detail: String,
    /// 对外等价状态码（写日志用；与 [`ClientError::status`] 同值）。
    pub status: u16,
}

/// **流中途失败信号**：`UsageTap` 写、协议出口与请求日志读。
///
/// ## 为什么需要共享
///
/// 流式组合链是 `上游 → UsageTap → 协议出口`，而三者各自只看得见自己那一段：
/// - `UsageTap` 逐帧旁听（唯一能看见「这一帧是错误信封」与「上游 Err」的地方）；
/// - 协议出口（`SsePassthrough` / Anthropic / Responses）负责收尾，必须知道「要不要补成功标记」；
/// - `StreamingLogSink` 在流结束时落日志，必须知道「这次其实是失败」——
///   否则日志页会把流中途失败显示成一次成功的 200 请求（issue #129 §5.2 改动 4）。
///
/// 用一个极小的共享槽把三者接起来，比给每个组件都塞一份嗅探逻辑可靠：
/// 嗅探只有一处（`UsageTap`），判定就不会漂移。
///
/// ## 只在开关开启时生效
///
/// `enabled = false` 时 [`Self::record`] 直接丢弃 —— 关闭状态下连一次写锁都不做，
/// 行为与改造前逐字一致。
#[derive(Debug)]
pub struct StreamFailure {
    enabled: bool,
    /// 第一条失败记录（后到的**不覆盖**：最早的那条才是根因）。
    slot: Mutex<Option<Failure>>,
}

impl StreamFailure {
    /// 新建信号槽。`enabled` 来自网关配置的 `auto_failover`。
    pub fn new(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled,
            slot: Mutex::new(None),
        })
    }

    /// 开关是否开启（协议出口据此决定收尾方式）。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 记录一次失败；返回 `true` 表示**这次**是新记录（调用方可据此做一次性动作）。
    ///
    /// 开关关闭时恒返回 `false` 且不写槽 —— 关闭态不留任何额外状态。
    pub fn record(&self, event: UpstreamEvent, detail: impl Into<String>) -> bool {
        if !self.enabled {
            return false;
        }
        let mut slot = match self.slot.lock() {
            Ok(slot) => slot,
            // 锁中毒：只可能是别的线程 panic。放弃记录比 panic 传染给流更安全。
            Err(_) => return false,
        };
        if slot.is_some() {
            return false;
        }
        let detail = detail.into();
        let status = client_error(&event, "", "").status;
        *slot = Some(Failure {
            event,
            detail,
            status,
        });
        true
    }

    /// 读一份快照（不消费）。
    pub fn peek(&self) -> Option<Failure> {
        self.slot.lock().ok().and_then(|slot| slot.clone())
    }
}

/// OpenAI 形状的**流内错误帧**（`data: {...}\n\n`）。
///
/// 后面仍要跟一条 `data: [DONE]`：客户端（尤其 VS Code 系）会一直等到它。
/// 顺序不能反 —— 错误帧在前，客户端解析到 `error` 就进错误分支，`[DONE]` 只负责收尾。
pub fn openai_error_frame(error: &ClientError) -> Bytes {
    let payload = json!({
        "error": {
            "type": error.kind,
            "message": error.message,
            "code": error.status,
        }
    });
    Bytes::from(format!("data: {payload}\n\n"))
}

/// Anthropic 形状的**流内错误事件**（`event: error` + `data:`）。
///
/// 与 `protocol::anthropic` 既有的错误帧分支同构（`{"type":"error","error":{…}}`）——
/// 那里是上游 `error` 字段的转换，这里是我们主动判定的失败，形状必须一致，
/// 否则同一个客户端会看到两种「错误」。
pub fn anthropic_error_event(error: &ClientError) -> Bytes {
    let payload = json!({
        "type": "error",
        "error": {"type": error.kind, "message": error.message},
    });
    Bytes::from(format!("event: error\ndata: {payload}\n\n"))
}

/// Responses 形状的**流内错误事件**。
///
/// 形状对齐 `protocol::responses` 既有实现（`{"type":"error","code":…,"message":…}`）；
/// `code` 用等价 HTTP 状态码的字符串形式，让文本正则与结构化判断都能命中。
pub fn responses_error_event(error: &ClientError) -> Bytes {
    let payload = json!({
        "type": "error",
        "code": error.status.to_string(),
        "message": error.message,
    });
    Bytes::from(format!("event: error\ndata: {payload}\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★★ 三条可重试事件必须收敛到 `429`，且**不得**出现配额类短语。
    ///
    /// 可证伪：把 `HardCredit` 的文案改成「余额不足」⇒ 客户端会归成 `QUOTA`
    /// （不可重试），比什么都不说更糟 —— 这是本模块最贵的一条约束。
    #[test]
    fn account_level_failures_converge_to_retryable_429() {
        for event in [
            UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: true,
            },
            UpstreamEvent::HardCredit,
            UpstreamEvent::SessionDead,
            UpstreamEvent::AuthRejected,
        ] {
            let error = client_error(&event, "一号", "hy4");
            assert_eq!(error.status, 429, "账号级失败对外统一 429：{event:?}");
            assert_eq!(error.kind, "rate_limit_error");
            assert!(
                error.message.contains("HTTP 429"),
                "文案必须含等价状态码（客户端走文本正则）：{}",
                error.message
            );
        }
    }

    /// 5xx / 超时 / 断流 → 502（客户端 `SERVER`，可重试）。
    #[test]
    fn server_side_failures_converge_to_retryable_502() {
        for event in [UpstreamEvent::Server, UpstreamEvent::Timeout] {
            let error = client_error(&event, "一号", "hy4");
            assert_eq!(error.status, 502);
            assert_eq!(error.kind, "api_error");
            assert!(error.message.contains("HTTP 502"), "{}", error.message);
        }
    }

    /// 参数错 / 模型封禁 / 404 → 400（客户端 `INVALID_REQUEST`，**不重试**）。
    ///
    /// 换号也救不了同一份坏参数，让它进重试只是白烧额度。
    #[test]
    fn caller_errors_stay_non_retryable() {
        for event in [
            UpstreamEvent::Client,
            UpstreamEvent::ModelBlocked,
            UpstreamEvent::NotFound,
        ] {
            let error = client_error(&event, "一号", "hy4");
            assert_eq!(error.status, 400);
            assert_eq!(error.kind, "invalid_request_error");
        }
    }

    /// ★ 对外文案**一个配额类短语都不许有**（`QUOTA` 判定排在 `RATE_LIMIT` 之前）。
    ///
    /// 覆盖中英两侧：客户端的正则与 `isQuotaExceededError` 都按这些词判。
    #[test]
    fn outward_messages_never_mention_quota_words() {
        const BANNED: [&str; 8] = [
            "余额",
            "配额",
            "积分",
            "quota",
            "insufficient",
            "out of credit",
            "balance",
            "exhausted",
        ];
        for event in [
            UpstreamEvent::HardCredit,
            UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: false,
            },
            UpstreamEvent::Server,
            UpstreamEvent::Client,
        ] {
            let message = client_error(&event, "一号", "hy4").message.to_lowercase();
            for word in BANNED {
                assert!(
                    !message.contains(word),
                    "对外文案不得含 `{word}`（会让客户端归成不可重试的 QUOTA）：{message}"
                );
            }
        }
    }

    /// 三条协议的帧形状各自成立，且都带得出状态码。
    #[test]
    fn frames_are_shaped_per_protocol() {
        let error = client_error(&UpstreamEvent::HardCredit, "一号", "hy4");
        let openai = String::from_utf8_lossy(&openai_error_frame(&error)).to_string();
        assert!(openai.starts_with("data: {"), "{openai}");
        assert!(openai.contains("\"type\":\"rate_limit_error\""), "{openai}");

        let anthropic = String::from_utf8_lossy(&anthropic_error_event(&error)).to_string();
        assert!(anthropic.starts_with("event: error\ndata: "), "{anthropic}");
        assert!(anthropic.contains("\"type\":\"error\""), "{anthropic}");

        let responses = String::from_utf8_lossy(&responses_error_event(&error)).to_string();
        assert!(responses.starts_with("event: error\ndata: "), "{responses}");
        assert!(responses.contains("\"code\":\"429\""), "{responses}");
    }

    /// ★ 开关关闭时**不留任何状态**（关闭态行为必须与改造前逐字一致）。
    #[test]
    fn disabled_signal_records_nothing() {
        let signal = StreamFailure::new(false);
        assert!(!signal.is_enabled());
        assert!(
            !signal.record(UpstreamEvent::Server, "断流"),
            "关闭时不得记录"
        );
        assert!(signal.peek().is_none());
    }

    /// 开启时**只记第一条**：最早那条才是根因，后到的（例如错误帧之后又断流）不覆盖。
    #[test]
    fn first_failure_wins() {
        let signal = StreamFailure::new(true);
        assert!(signal.record(UpstreamEvent::HardCredit, "6004 rate limited"));
        assert!(!signal.record(UpstreamEvent::Server, "connection reset"));
        let failure = signal.peek().expect("必须记录");
        assert_eq!(failure.event, UpstreamEvent::HardCredit);
        assert_eq!(failure.status, 429);
        assert_eq!(failure.detail, "6004 rate limited");
    }
}
