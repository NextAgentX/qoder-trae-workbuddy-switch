//! Qoder 网关的**风控 / 排队信号**计数器（进程级，只读出口是 `/health`）。
//!
//! ## 为什么要有它
//!
//! 上游的三种失败长得**很像**（都是 401/403/429），但处理完全不同：
//!
//! | 信号 | 含义 | 我们的处置 |
//! |:--|:--|:--|
//! | `queued` | 上游排队（`10605` / `isQueued`） | 停止当前自动化，**不惩罚账号** |
//! | `rateLimited` | 429 限流 | 按模型冷却，当前自动化停止 |
//! | `rejected` | 其它 4xx（参数错） | **不换号**，原样回给调用方 |
//! | `exhausted` | 402 余额耗尽 | 停放至下一个额度重置窗口，当前自动化停止 |
//!
//! 只看日志很难回答「今天到底是排队多还是限流多」。参考实现把它做成
//! `/health` 上的遥测（`RISK_STATS`），运维据此判断该加号、该降速、还是该查客户端。
//!
//! ## 口径
//!
//! - **进程级累计**（重启清零）：这是**遥测**不是账本，不落盘；
//! - 只在**分类处**记一次（`chat_for_region`），不要在别处重复计数；
//! - 永远是**只读**出口：`/health` 拿快照，不提供重置接口（免得被误用成「清零指标」）。

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

/// 上游排队（`10605` / `isQueued`）：换号即可，账号本身没坏。
static QUEUED: AtomicU64 = AtomicU64::new(0);
/// 429 限流（按模型冷却）。
static RATE_LIMITED: AtomicU64 = AtomicU64::new(0);
/// 其它 4xx（客户端参数错）：不换号。
static REJECTED: AtomicU64 = AtomicU64::new(0);
/// 402 余额耗尽：停放 + 换号（涨得快说明该充值 / 该减少账号复用）。
static EXHAUSTED: AtomicU64 = AtomicU64::new(0);

/// 记一次「上游排队」。
pub fn note_queued() {
    QUEUED.fetch_add(1, Ordering::Relaxed);
}

/// 记一次「429 限流」。
pub fn note_rate_limited() {
    RATE_LIMITED.fetch_add(1, Ordering::Relaxed);
}

/// 记一次「客户端参数错（其它 4xx）」。
pub fn note_rejected() {
    REJECTED.fetch_add(1, Ordering::Relaxed);
}

/// 记一次「余额耗尽（402）」。
pub fn note_exhausted() {
    EXHAUSTED.fetch_add(1, Ordering::Relaxed);
}

/// 当前计数快照（`/health` 用）。
pub fn snapshot() -> Value {
    json!({
        "queued": QUEUED.load(Ordering::Relaxed),
        "rateLimited": RATE_LIMITED.load(Ordering::Relaxed),
        "rejected": REJECTED.load(Ordering::Relaxed),
        "exhausted": EXHAUSTED.load(Ordering::Relaxed),
    })
}

/// 清零（**只给测试用**：`/health` 的快照是累计值，测试之间必须互不影响）。
#[doc(hidden)]
pub fn reset_for_test() {
    QUEUED.store(0, Ordering::Relaxed);
    RATE_LIMITED.store(0, Ordering::Relaxed);
    REJECTED.store(0, Ordering::Relaxed);
    EXHAUSTED.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个信号各记各的，快照键名是**wire 契约**（前端/运维按它读）。
    #[test]
    fn snapshot_has_pinned_keys_and_counts_independently() {
        // 全局静态：本用例与其它用例共享计数器 ⇒ 先清零，再断言相对增量。
        reset_for_test();
        let before = snapshot();
        assert_eq!(before["queued"], 0);
        assert_eq!(before["rateLimited"], 0);
        assert_eq!(before["rejected"], 0);

        note_queued();
        note_queued();
        note_rate_limited();
        let after = snapshot();
        assert_eq!(after["queued"], 2);
        assert_eq!(after["rateLimited"], 1);
        assert_eq!(after["rejected"], 0, "没记的信号不能跟着涨");
        assert_eq!(after["exhausted"], 0);

        note_rejected();
        assert_eq!(snapshot()["rejected"], 1);
        note_exhausted();
        assert_eq!(snapshot()["exhausted"], 1);
        reset_for_test();
    }
}
