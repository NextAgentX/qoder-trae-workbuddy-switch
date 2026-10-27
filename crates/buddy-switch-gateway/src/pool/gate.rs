//! **模型级闸门**（跨请求）+ 半开探测。
//!
//! ## 解决什么问题
//!
//! `pool::pick` 的模型级冷却只记在**单个账号**上。真实高频场景是
//! 「**同一个模型**在一批账号上一起被限流」（上游按时段/账号组生效）：
//!
//! - 请求 A：选号 → 账号 1 → 429 → 写冷却 → 换账号 2 → 429 → 写冷却 → 换账号 3 → …
//! - 请求 B（一分钟后）：同样的剧本再来一遍。
//!
//! 每个请求都把 `max_rotate` 次重试烧在同一堵墙上，而用户拿到的只是
//! 「没有可用账号」这种**无法行动**的错误。
//!
//! ⇒ 需要一层**跨请求**的判断：同一模型在 ≥N 个账号上失败，就不该再逐个试。
//!
//! ## 三个状态
//!
//! ```text
//! Closed ──(窗口内失败数 ≥ threshold)──▶ Paused ──(到期)──▶ HalfOpen
//!   ▲                                                            │
//!   └────────────────(试探成功 / 该模型成功一次)───────────────────┘
//!                     (试探失败 ⇒ 重新 Paused，退避翻倍)
//! ```
//!
//! ## 为什么到期不是直接放全量
//!
//! 冷却到期瞬间放全量 = 把一批请求同时撞上去，失败后又是一轮冷却 —— 典型的
//! 「自我实现的限流」。半开只放**一个**请求做试探：成功才放开，失败就把退避翻倍。
//!
//! ## 为什么不持久化
//!
//! 闸门是「此刻上游整体状况」的判断，进程重启后重新观测即可。落盘反而危险：
//! 用户重启网关后仍被一个陈旧的暂停挡住，而界面上看不到任何东西。

use std::collections::HashMap;

use crate::pool::PoolConfig;

/// 半开探针的租约（毫秒）：领票后这么久仍未回填结果，视为「试出去没回来」。
///
/// 取 120s：上游最慢的一次真实调用（大上下文 + 冷启动）也在秒级到几十秒之间；
/// 120s 足够长，又不会让一次「票丢了」把模型锁到重启。
pub const PROBE_LEASE_MS: i64 = 120_000;

/// 闸门阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatePhase {
    /// 暂停中：一个请求都不放行。
    Paused,
    /// 半开：**只**放一个探针请求（放出去之后其余请求仍按 Paused 处理）。
    HalfOpen,
}

/// 一个模型的闸门状态。
#[derive(Debug, Clone)]
pub struct ModelGate {
    /// 模型名。
    pub model: String,
    /// **窗口内累计失败次数**（诊断用）。
    ///
    /// ★ 这不是账号数（issue #106 §2.2）。原字段名叫 `accounts`，而 `record_failure`
    /// 每被调用一次就 push 一个时间戳 —— 同一个账号在同一模型上连撞两次 6004 也会
    /// 得到 `2`。于是文案渲染成「已在 2 个账号上被限流」，单号池时是假话，
    /// 用户数了数自己的号，对不上这句话。
    pub failures: usize,
    /// 暂停截止时刻（毫秒）。
    pub until_ms: i64,
    /// 阶段。
    pub phase: GatePhase,
    /// 已连续触发次数（退避指数：`base * 2^(trips-1)`）。
    pub trips: u32,
    /// 本轮半开是否已经放出一个探针。
    pub probed: bool,
    /// 探针的**租约到期时刻**（毫秒）。`probed` 之后超过它仍未回填结果，
    /// 视为「试出去了但没回来」⇒ 按失败处理（见 [`ModelGates::blocked`]）。
    pub probe_until_ms: i64,
    /// 探针票的**归属请求标识**（空 = 无主）。安全审计 P1-3。
    ///
    /// ## 为什么票需要记住「是谁领的」
    ///
    /// 改造前 `try_probe` 只判 `probed`：半开时**第一个**请求领票成功，而中继的
    /// attempt 循环**每轮**都调 `admit_model` ⇒ 同一个请求的第一次换号重试就被
    /// 自己那张票挡住（`max_rotate` 退化成 1 次），并且换号轨迹里会出现一条
    /// 假信息「试探名额已被**其它**请求占用」—— 占用的其实是它自己。
    ///
    /// 有了归属，同一个请求可以持票继续换号；别的请求仍被挡住（半开的语义不变）。
    pub probe_owner: String,
}

/// 被闸门挡住时的可诊断信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateBlock {
    /// 模型名。
    pub model: String,
    /// 剩余毫秒（`0` = 已到期但探针名额已被别的请求占掉）。
    pub remain_ms: i64,
    /// 触发时**窗口内的失败次数**（不是账号数，见 [`ModelGate::failures`]）。
    pub failures: usize,
}

/// 全部模型的闸门（进程内，不落盘）。
#[derive(Debug, Default)]
pub struct ModelGates {
    gates: HashMap<String, ModelGate>,
    /// 每个模型**窗口内**的失败时刻（毫秒，升序）。
    recent: HashMap<String, Vec<i64>>,
}

impl ModelGates {
    /// 记一次「该模型在某账号上失败」。返回是否**因此**新开了闸门。
    ///
    /// 只用于**模型级**失败（`6004` 模型限流 / `11102` 封禁）—— 账号级 429 / 402 / 5xx
    /// 是「那个号的事」，换个号大概率就好，计进模型失败数会让闸门频繁误触发。
    pub fn record_failure(&mut self, model: &str, now_ms: i64, config: &PoolConfig) -> bool {
        if model.is_empty() {
            return false;
        }
        let window = config.gate_window_ms.max(1);
        let entry = self.recent.entry(model.to_string()).or_default();
        entry.retain(|at| now_ms.saturating_sub(*at) <= window);
        entry.push(now_ms);

        // 已经在暂停中：只累计计数，不重复开闸（退避在**重新**触发时才翻倍）。
        if let Some(gate) = self.gates.get(model) {
            if gate.phase == GatePhase::Paused && now_ms < gate.until_ms {
                return false;
            }
            if gate.phase == GatePhase::HalfOpen {
                // 半开试探失败 ⇒ 重新暂停且退避翻倍（这正是半开要判的东西）。
                let trips = gate.trips.saturating_add(1);
                let until = now_ms.saturating_add(backoff_ms(config, trips));
                let failures = gate.failures.max(entry.len());
                self.gates.insert(
                    model.to_string(),
                    ModelGate {
                        model: model.to_string(),
                        failures,
                        until_ms: until,
                        phase: GatePhase::Paused,
                        trips,
                        probed: false,
                        probe_until_ms: 0,
                        probe_owner: String::new(),
                    },
                );
                return true;
            }
        }

        if entry.len() < config.gate_threshold.max(1) as usize {
            return false;
        }
        let trips = self.gates.get(model).map(|gate| gate.trips).unwrap_or(0) + 1;
        let until = now_ms.saturating_add(backoff_ms(config, trips));
        self.gates.insert(
            model.to_string(),
            ModelGate {
                model: model.to_string(),
                failures: entry.len(),
                until_ms: until,
                phase: GatePhase::Paused,
                trips,
                probed: false,
                probe_until_ms: 0,
                probe_owner: String::new(),
            },
        );
        true
    }

    /// 该模型此刻是否应被拒绝；`None` = 放行。
    ///
    /// 到期后**不自动放行全量**：转入 `HalfOpen`，由 [`Self::try_probe`] 发一张探针票。
    pub fn blocked(&mut self, model: &str, now_ms: i64, config: &PoolConfig) -> Option<GateBlock> {
        let gate = self.gates.get_mut(model)?;
        match gate.phase {
            GatePhase::Paused if now_ms >= gate.until_ms => {
                gate.phase = GatePhase::HalfOpen;
                gate.probed = false;
                None
            }
            GatePhase::Paused => Some(GateBlock {
                model: gate.model.clone(),
                remain_ms: gate.until_ms.saturating_sub(now_ms),
                failures: gate.failures,
            }),
            GatePhase::HalfOpen => {
                // ★★ 探针**租约到期**仍未回填结果 ⇒ 按失败处理，重新暂停并退避翻倍。
                //
                // 为什么必须有这条（QA 复核抓到的阻塞项）：`probed = true` 之后，
                // 领票的那个请求可能根本没发出（选不出号 / 在途满 / 参数错），
                // 于是没有任何人回填结果 ⇒ 闸门永远停在「半开且票已领」，
                // 该模型此后每个请求都返回「试探名额已被占用」，**直到进程重启**。
                // 单号池或 11102（模型冷却 6h）时必现。
                if gate.probed && gate.probe_until_ms > 0 && now_ms >= gate.probe_until_ms {
                    let trips = gate.trips.saturating_add(1);
                    let until = now_ms.saturating_add(backoff_ms(config, trips));
                    let failures = gate.failures;
                    let model = gate.model.clone();
                    self.gates.insert(
                        model.clone(),
                        ModelGate {
                            model: model.clone(),
                            failures,
                            until_ms: until,
                            phase: GatePhase::Paused,
                            trips,
                            probed: false,
                            probe_until_ms: 0,
                            probe_owner: String::new(),
                        },
                    );
                    return Some(GateBlock {
                        model,
                        remain_ms: until.saturating_sub(now_ms),
                        failures,
                    });
                }
                None
            }
        }
    }

    /// 半开阶段申请 / 续用探针票。返回 `true` = 本次请求可以去试。
    ///
    /// ## 三种情形（安全审计 P1-3）
    ///
    /// | 状态 | `owner` | 结果 |
    /// |:--|:--|:--|
    /// | 半开、票未发 | 任意 | **发一张票**并记下 `owner` ⇒ `true` |
    /// | 半开、票已发给**同一个**请求 | 相同 | `true`（持票者换号重试，不该被自己挡住） |
    /// | 半开、票已发给**别的**请求 / 仍在暂停 | — | `false`（否则一次到期会放出双倍探针） |
    ///
    /// `owner` 为空串时退化成旧语义（只发一张、永不复用）—— 供不关心归属的调用方使用。
    pub fn try_probe(&mut self, model: &str, now_ms: i64, owner: &str) -> bool {
        match self.gates.get_mut(model) {
            Some(gate) if gate.phase == GatePhase::HalfOpen => {
                if gate.probed {
                    return !owner.is_empty() && gate.probe_owner == owner;
                }
                gate.probed = true;
                gate.probe_owner = owner.to_string();
                // 租约：探针必须在 `PROBE_LEASE_MS` 内回填（成功/失败），否则到期重开暂停。
                gate.probe_until_ms = now_ms.saturating_add(PROBE_LEASE_MS);
                true
            }
            // 没有闸门 = 从未失败过，正常放行。
            None => true,
            _ => false,
        }
    }

    /// 探针票的**真实剩余租约**（毫秒）。没有票时返回 `0`。
    ///
    /// ★ 用来替换 `GateBlock.remain_ms = 0` 那个哨兵值：改造前「票被占用」的响应
    /// 会告诉客户端 `Retry-After: 1`，而真实等待可能是整整 `PROBE_LEASE_MS`（120s）——
    /// 文案写「一两秒」，用户就按 1 秒的节奏去撞一堵 120 秒的墙。
    pub fn probe_remaining_ms(&self, model: &str, now_ms: i64) -> i64 {
        self.gates
            .get(model)
            .map(|gate| gate.probe_until_ms.saturating_sub(now_ms).max(0))
            .unwrap_or(0)
    }

    /// **归还**探针票（幂等）。
    ///
    /// ## 为什么必须有归还路径
    ///
    /// 领票的请求可能**根本没得到模型级结论**：选不出号、在途满、参数错、
    /// 上游回的是账号级 429/5xx …… 这些情况都不会调 `record_success` / `record_failure`。
    /// 改造前票就一直挂到 120s 租约到期，期间**所有**请求都被判成「名额被占用」⇒
    /// 整模型 503，最长两分钟，而报错还说是「别的请求」占了名额。
    ///
    /// ⚠️ 只清 `probed` / `probe_until_ms`，**不动** `trips` / `until_ms`：
    /// 「试探没有结论」不是「试探失败」，不该把退避翻倍。
    pub fn release_probe(&mut self, model: &str, owner: &str) {
        let Some(gate) = self.gates.get_mut(model) else {
            return;
        };
        if gate.probed && !owner.is_empty() && gate.probe_owner == owner {
            gate.probed = false;
            gate.probe_owner.clear();
            gate.probe_until_ms = 0;
        }
    }

    /// 该模型当前是否有闸门（暂停或半开）。
    ///
    /// 调用方需要它来区分「从来没失败过」与「半开且票已被领走」——
    /// 两者的 `blocked()` 都是 `None`，但后者必须被拒绝。
    pub fn is_gated(&self, model: &str) -> bool {
        self.gates.contains_key(model)
    }

    /// 该模型成功一次 ⇒ 闸门关闭（成功是最强的反证）。
    pub fn record_success(&mut self, model: &str) {
        if model.is_empty() {
            return;
        }
        self.gates.remove(model);
        self.recent.remove(model);
    }

    /// 惰性清理：已关闭（无闸门）且最近失败都在窗口外的条目。
    ///
    /// 不清理的话 `recent` 会随模型数无限增长 —— 模型清单最多几十个，
    /// 但这是**每请求**都可能写的地方，留着无界增长迟早成为隐患。
    pub fn prune(&mut self, now_ms: i64, window_ms: i64) {
        let window = window_ms.max(1);
        self.recent.retain(|_, times| {
            times.retain(|at| now_ms.saturating_sub(*at) <= window);
            !times.is_empty()
        });
        // 闸门只在 `Paused` 时保留；`HalfOpen` 且从未被探测过的也保留（等票）。
        self.gates
            .retain(|_, gate| gate.phase == GatePhase::Paused || gate.phase == GatePhase::HalfOpen);
    }

    /// 当前所有闸门（管理面/诊断用）。
    pub fn snapshot(&self) -> Vec<&ModelGate> {
        self.gates.values().collect()
    }

    /// 单元测试用：按模型取闸门。
    #[cfg(test)]
    fn get(&self, model: &str) -> Option<&ModelGate> {
        self.gates.get(model)
    }
}

/// 退避：`base * 2^(trips-1)`，封顶 `gate_max_ms`。
fn backoff_ms(config: &PoolConfig, trips: u32) -> i64 {
    let shift = trips.saturating_sub(1).min(16);
    let base = config.gate_base_ms.max(1);
    let scaled = base.saturating_mul(1i64 << shift);
    scaled.min(config.gate_max_ms.max(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> PoolConfig {
        PoolConfig {
            gate_threshold: 2,
            gate_window_ms: 60_000,
            gate_base_ms: 1_000,
            gate_max_ms: 8_000,
            ..PoolConfig::default()
        }
        .normalized()
    }

    const NOW: i64 = 1_000_000;

    #[test]
    fn trips_only_after_threshold_accounts() {
        let mut gates = ModelGates::default();
        let cfg = config();
        assert!(
            !gates.record_failure("m", NOW, &cfg),
            "第 1 个账号失败不该开闸"
        );
        assert!(gates.blocked("m", NOW, &cfg).is_none(), "未开闸 ⇒ 放行");
        assert!(
            gates.record_failure("m", NOW, &cfg),
            "第 2 个账号失败必须开闸"
        );
        let blocked = gates.blocked("m", NOW, &cfg).expect("必须被挡住");
        assert_eq!(blocked.failures, 2);
        assert!(blocked.remain_ms > 0);
    }

    #[test]
    fn window_expiry_forgets_old_failures() {
        let mut gates = ModelGates::default();
        let cfg = config();
        gates.record_failure("m", NOW, &cfg);
        // 窗口外的第二次失败 ⇒ 计数从 0 重来，不该开闸。
        let later = NOW + 120_000;
        assert!(!gates.record_failure("m", later, &cfg));
        assert!(gates.blocked("m", later, &cfg).is_none());
    }

    #[test]
    fn half_open_lets_exactly_one_probe_through() {
        let mut gates = ModelGates::default();
        let cfg = config();
        gates.record_failure("m", NOW, &cfg);
        gates.record_failure("m", NOW, &cfg);
        let until = gates.get("m").unwrap().until_ms;

        // 到期 ⇒ 转入半开并**放行这一次**（票还没被领）
        assert!(gates.blocked("m", until + 1, &cfg).is_none());
        assert!(
            gates.try_probe("m", until + 1, "req-a"),
            "半开必须发一张探针票"
        );
        // 第二个请求领不到票 ⇒ 视为仍在暂停（`blocked` 对 HalfOpen 恒 None，
        // 由调用方用 `try_probe` 的返回值兜住）
        assert!(
            !gates.try_probe("m", until + 2, "req-b"),
            "一张票只能领一次（别的请求领不走）"
        );
    }

    #[test]
    fn half_open_failure_doubles_backoff() {
        let mut gates = ModelGates::default();
        let cfg = config();
        gates.record_failure("m", NOW, &cfg);
        gates.record_failure("m", NOW, &cfg);
        let first_until = gates.get("m").unwrap().until_ms;
        let first_pause = first_until - NOW;
        assert_eq!(first_pause, 1_000, "首次暂停 = base");

        // 到期 → 领票 → 试探失败 ⇒ 重新暂停且翻倍
        let at = first_until + 1;
        assert!(gates.blocked("m", at, &cfg).is_none());
        assert!(gates.try_probe("m", at, "req-a"));
        assert!(gates.record_failure("m", at, &cfg));
        let second_pause = gates.get("m").unwrap().until_ms - at;
        assert_eq!(second_pause, 2_000, "半开失败必须退避翻倍");
        assert_eq!(gates.get("m").unwrap().trips, 2);
    }

    /// ★★ QA 复核抓到的阻塞项：半开领票后**没人回填**，闸门不能永久锁死。
    ///
    /// 变异：去掉 `blocked` 里的 probe 租约分支 ⇒ 本用例红（该模型此后每个请求
    /// 都返回「试探名额已被占用」，直到进程重启）。
    #[test]
    fn lost_probe_ticket_reopens_pause_instead_of_locking_forever() {
        let mut gates = ModelGates::default();
        let cfg = config();
        gates.record_failure("m", NOW, &cfg);
        gates.record_failure("m", NOW, &cfg);
        let until = gates.get("m").unwrap().until_ms;

        // 到期 → 领票；但**没有人回填结果**（请求根本没发出去）
        let at = until + 1;
        assert!(gates.blocked("m", at, &cfg).is_none());
        assert!(gates.try_probe("m", at, "req-a"));
        // 租约内：仍按「票已被领」拒绝
        assert!(
            gates.blocked("m", at + 1_000, &cfg).is_none(),
            "半开期间本身不返回 Blocked"
        );
        assert!(
            !gates.try_probe("m", at + 1_000, "req-b"),
            "一张票只能领一次"
        );

        // 超过租约 ⇒ 必须重新暂停（而不是永远停在「票已领」）
        let after_lease = at + PROBE_LEASE_MS + 1;
        let blocked = gates
            .blocked("m", after_lease, &cfg)
            .expect("探针租约到期必须重新暂停");
        assert!(blocked.remain_ms > 0);
        assert_eq!(
            gates.get("m").unwrap().trips,
            2,
            "丢失的探针同样算一次失败（退避翻倍）"
        );
        // 而且**仍能**继续走半开（到期后重新发得出票）
        let next = after_lease + blocked.remain_ms + 1;
        assert!(gates.blocked("m", next, &cfg).is_none());
        assert!(
            gates.try_probe("m", next, "req-c"),
            "重开暂停后必须能再发一张票"
        );
    }

    #[test]
    fn success_closes_the_gate() {
        let mut gates = ModelGates::default();
        let cfg = config();
        gates.record_failure("m", NOW, &cfg);
        gates.record_failure("m", NOW, &cfg);
        assert!(gates.blocked("m", NOW, &cfg).is_some());
        gates.record_success("m");
        assert!(gates.blocked("m", NOW, &cfg).is_none(), "成功必须立刻关闸");
        assert!(gates.snapshot().is_empty());
    }

    #[test]
    fn backoff_is_capped() {
        let cfg = config();
        assert_eq!(backoff_ms(&cfg, 1), 1_000);
        assert_eq!(backoff_ms(&cfg, 2), 2_000);
        assert_eq!(backoff_ms(&cfg, 3), 4_000);
        assert_eq!(backoff_ms(&cfg, 4), 8_000, "封顶 gate_max_ms");
        assert_eq!(backoff_ms(&cfg, 30), 8_000);
    }

    #[test]
    fn prune_drops_stale_bookkeeping() {
        let mut gates = ModelGates::default();
        let cfg = config();
        gates.record_failure("m", NOW, &cfg); // 1 次，未开闸
        gates.prune(NOW + 120_000, cfg.gate_window_ms);
        assert!(gates.recent.is_empty(), "窗口外的失败记录必须被清掉");
        assert!(gates.snapshot().is_empty());
    }

    // -----------------------------------------------------------------------
    // 安全审计 P1-3：探针票的归属与归还
    // -----------------------------------------------------------------------

    /// 半开 + 已经发过两次失败 ⇒ 进入 `HalfOpen` 并返回闸门到期时刻。
    fn half_open(gates: &mut ModelGates) -> i64 {
        let cfg = config();
        gates.record_failure("m", NOW, &cfg);
        gates.record_failure("m", NOW, &cfg);
        gates.get("m").unwrap().until_ms
    }

    /// ★★ 安全审计 P1-3：**持票的那个请求自己再来，不该被自己那张票挡住**。
    ///
    /// 改造前 `try_probe` 只判 `probed` ⇒ 中继的 attempt 循环第 2 轮就被判成
    /// 「名额已被其它请求占用」，`max_rotate` 退化成 1 次，且换号轨迹里出现一句假话。
    ///
    /// 变异验证：把 `try_probe` 里的 owner 判断删回 `!gate.probed` ⇒ 第一条断言红。
    #[test]
    fn probe_ticket_belongs_to_the_request_that_claimed_it() {
        let mut gates = ModelGates::default();
        let mut cfg = config();
        cfg.gate_base_ms = 1_000;
        let until = half_open(&mut gates);

        assert!(gates.blocked("m", until + 1, &cfg).is_none(), "到期转半开");
        assert!(gates.try_probe("m", until + 1, "req-a"), "第一个请求领到票");
        // ① 持票者自己换号重试 ⇒ 仍然可以继续。
        assert!(
            gates.try_probe("m", until + 2, "req-a"),
            "持票者第二次尝试不该被自己挡住（max_rotate 不能退化）"
        );
        // ② 别的请求 ⇒ 一律领不到（半开的语义不变）。
        assert!(
            !gates.try_probe("m", until + 3, "req-b"),
            "别的请求不能蹭票，否则一次到期会放出双倍探针"
        );
        // 反向对照：清空 owner 的调用方退化成旧语义（只发一张、永不复用）。
        assert!(!gates.try_probe("m", until + 4, ""), "空 owner 不得续用");
    }

    /// ★★ 安全审计 P1-3：**票必须有归还路径**，且归还不能算作失败。
    ///
    /// 改造前根本没有归还入口（全仓无 `release_probe`）⇒ 领票请求若没得到模型级
    /// 结论（选不出号 / 在途满 / 账号级 429），票会挂满 `PROBE_LEASE_MS`（120s），
    /// 期间整模型 503。
    #[test]
    fn released_probe_reopens_half_open_without_punishing_the_model() {
        let mut gates = ModelGates::default();
        let mut cfg = config();
        cfg.gate_base_ms = 1_000;
        let until = half_open(&mut gates);
        let before = gates.get("m").unwrap().clone();

        assert!(gates.blocked("m", until + 1, &cfg).is_none());
        assert!(gates.try_probe("m", until + 1, "req-a"));
        let lease = gates.probe_remaining_ms("m", until + 1);
        assert_eq!(lease, PROBE_LEASE_MS, "租约必须是完整时长");

        gates.release_probe("m", "req-a");
        // ① 票已归还 ⇒ 另一个请求立刻能领（不必等 120s）。
        assert!(
            gates.try_probe("m", until + 2, "req-b"),
            "归还后别的请求必须能立刻探路"
        );
        // ② 归还**不是失败**：退避次数与暂停截止时刻都不能变。
        let after = gates.get("m").unwrap();
        assert_eq!(after.trips, before.trips, "归还不该把退避翻倍");
        assert_eq!(after.until_ms, before.until_ms, "归还不该延长暂停");
        assert_eq!(after.failures, before.failures, "归还不该记一次失败");

        // ③ 幂等：再归还一次不出错、也不改变状态。
        gates.release_probe("m", "req-b");
        // ④ 票已空出来 ⇒ 下一个请求能领到。
        assert!(
            gates.try_probe("m", until + 3, "req-c"),
            "归还后票应可被下一个请求领取"
        );
    }

    /// 归还只对**持票者**生效：别人调 `release_probe` 不能把票抢走。
    #[test]
    fn only_the_ticket_owner_can_release_it() {
        let mut gates = ModelGates::default();
        let mut cfg = config();
        cfg.gate_base_ms = 1_000;
        let until = half_open(&mut gates);

        assert!(gates.blocked("m", until + 1, &cfg).is_none());
        assert!(gates.try_probe("m", until + 1, "req-a"));
        gates.release_probe("m", "req-b");
        assert!(
            gates.try_probe("m", until + 2, "req-a"),
            "别人归还后持票者必须仍持票"
        );
        assert!(
            !gates.try_probe("m", until + 3, "req-b"),
            "非持票者不得因为「归还」而获得票"
        );
    }

    /// `probe_remaining_ms` 必须是**真实剩余**，而不是 0 哨兵。
    ///
    /// 改造前 `GateBlock.remain_ms` 在「票被占用」时恒为 0，于是错误文案只能说
    /// 「稍等一两秒」，而真实等待是 120s。
    #[test]
    fn probe_remaining_reports_the_real_lease() {
        let mut gates = ModelGates::default();
        let mut cfg = config();
        cfg.gate_base_ms = 1_000;
        let until = half_open(&mut gates);

        assert_eq!(gates.probe_remaining_ms("m", until), 0, "还没发票");
        assert!(gates.blocked("m", until + 1, &cfg).is_none());
        assert!(gates.try_probe("m", until + 1, "req-a"));
        assert_eq!(gates.probe_remaining_ms("m", until + 1), PROBE_LEASE_MS);
        assert_eq!(
            gates.probe_remaining_ms("m", until + 1 + PROBE_LEASE_MS / 2),
            PROBE_LEASE_MS / 2,
            "半程时剩余应是半程"
        );
        // 没有闸门的模型恒 0（不能返回负数或 panic）。
        assert_eq!(gates.probe_remaining_ms("never", until), 0);
    }
}
