//! 会话粘性：把同一轮对话尽量固定到同一账号。
//!
//! 移植自参考实现 `internal/session` 的粘性绑定（配置 `session_sticky`）。
//!
//! 为什么需要：多账号池下若每轮随机选号，同一会话的上下文会落到不同账号上，
//! 上游侧表现为「对话突然失忆」；且跨账号的缓存前缀命中率归零。
//! 绑定的取舍是：**同一会话优先复用上次成功的账号**，但该账号不可用时立刻解绑
//! 换号——粘性是优化而非约束，绝不能因为粘性而拒绝服务。

use std::collections::HashMap;

/// 一条粘性绑定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickyBinding {
    /// 绑定的账号 uid。
    pub uid: String,
    /// 绑定时刻（毫秒）。
    pub bound_at_ms: i64,
}

/// 粘性绑定表（带 TTL 的 LRU 语义简化版：只按时间淘汰）。
///
/// ## 规模与 GC
///
/// 条目数 = 「TTL 窗口内活跃过的会话数」。若不清理，会随**进程寿命**单调增长
/// （`bind` 每次都插入）⇒ 接上写路径后必须有 GC。
///
/// 这里把 GC 放在**写入路径**上做**摊销**（而不是开后台循环）：写入频率天然远低于读取，
/// 且这样不需要在 `BackgroundTask` 注册表里新增条目（该表要求「登记即执行」）。
#[derive(Debug, Clone)]
pub struct StickyTable {
    bindings: HashMap<String, StickyBinding>,
    ttl_ms: i64,
    /// 下一次触发 GC 的条目数阈值；清理后按当前规模**上抬**，保证摊销 O(1)。
    ///
    /// 若只是「`len >= 阈值` 就 GC」，在「条目大多未过期」时每次 `bind` 都会全表扫描 ——
    /// 那才是真正的性能问题。上抬阈值把它摊掉。
    gc_at: usize,
}

/// 首次触发 GC 的条目数。取 64：正常使用（TTL 内几十个会话）下**永远不会触发**，
/// 只有长跑且会话数异常时才介入。
const INITIAL_GC_THRESHOLD: usize = 64;

impl Default for StickyTable {
    fn default() -> Self {
        Self::new(0)
    }
}

impl StickyTable {
    /// 新建；`ttl_ms <= 0` 时回落 30 分钟（避免配置漏填导致绑定永不过期）。
    pub fn new(ttl_ms: i64) -> Self {
        Self {
            bindings: HashMap::new(),
            ttl_ms: if ttl_ms > 0 { ttl_ms } else { 30 * 60 * 1000 },
            gc_at: INITIAL_GC_THRESHOLD,
        }
    }

    /// 当前 TTL。
    pub fn ttl_ms(&self) -> i64 {
        self.ttl_ms
    }

    /// **表大小**（含已过期但尚未被 GC 清掉的条目）。
    ///
    /// ⚠️ 这不是「生效中的绑定数」——安全审计 P2-10 之前这里写着「不含已过期条目」，
    /// 而实现直接返回 `bindings.len()`（`get()` 才做 TTL 过滤，`len()` 没有）。
    /// 于是 `/status` 的 `sticky_sessions` 只增不减：会话停了半小时，这个数还在。
    ///
    /// 要「生效中的数量」请用 [`Self::len_active`]（它需要 `now_ms` 才能过滤）。
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// **生效中**的绑定数（不含已过期条目）。
    ///
    /// `/status` 用的是这一条：那个数字的语义是「此刻还粘着多少个会话」，
    /// 而不是「表里堆了多少条」。
    pub fn len_active(&self, now_ms: i64) -> usize {
        self.bindings
            .values()
            .filter(|binding| now_ms.saturating_sub(binding.bound_at_ms) < self.ttl_ms)
            .count()
    }

    /// 是否为空（同样只说明**表**是空的；判「有没有生效中的绑定」用 `len_active`）。
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// 查询绑定；已过期视为不存在。
    pub fn get(&self, key: &str, now_ms: i64) -> Option<&str> {
        self.bindings
            .get(key)
            .filter(|binding| now_ms.saturating_sub(binding.bound_at_ms) < self.ttl_ms)
            .map(|binding| binding.uid.as_str())
    }

    /// 写入/刷新绑定。
    ///
    /// 顺带做**摊销 GC**（见 [`StickyTable`] 的文档）：达到阈值才清理，清理后按当前规模
    /// 上抬阈值 —— 因此「条目大多未过期」时不会退化成每次全表扫描。
    pub fn bind(&mut self, key: &str, uid: &str, now_ms: i64) {
        if key.is_empty() || uid.is_empty() {
            return;
        }
        if self.bindings.len() >= self.gc_at {
            self.gc(now_ms);
            // 清理后仍超阈值 ⇒ 大多是未过期条目 ⇒ 上抬，避免下次 bind 再扫一遍。
            self.gc_at = self
                .bindings
                .len()
                .saturating_mul(2)
                .max(INITIAL_GC_THRESHOLD);
        }
        self.bindings.insert(
            key.to_string(),
            StickyBinding {
                uid: uid.to_string(),
                bound_at_ms: now_ms,
            },
        );
    }

    /// 解绑（换号时调用；下次请求重新绑定）。
    pub fn unbind(&mut self, key: &str) {
        self.bindings.remove(key);
    }

    /// **按 uid 批量解绑**：清掉所有指向该账号的绑定，返回清掉的数量。
    ///
    /// ## 为什么必须有它（2026-10-17）
    ///
    /// 排除 / 禁用 / 删除账号之后，指向它的粘性绑定仍然存在。后果有两层：
    ///
    /// 1. **白跑一跳**：每个请求都要先 `is_usable` 判一次「它还能不能用」再解绑；
    /// 2. **状态自相矛盾**：界面上「粘性」徽标挂在一个「已排除」的行上。
    ///
    /// 按 key 一个个解绑做不到这件事 —— 调用方根本不知道有哪些会话绑在它身上。
    ///
    /// 遍历全表是 O(绑定数)，但**这个函数的调用频率是「用户点一次排除」**，
    /// 不是每请求一次；而绑定表在 TTL 内只有几十条。不值得为它加反向索引
    /// （那要多维护一份 `uid → keys` 的一致性，出错方式更隐蔽）。
    pub fn purge_uid(&mut self, uid: &str) -> usize {
        if uid.is_empty() {
            return 0;
        }
        let before = self.bindings.len();
        self.bindings
            .retain(|_, binding| binding.uid.as_str() != uid);
        before - self.bindings.len()
    }

    /// 清理过期条目，返回清理数量。
    pub fn gc(&mut self, now_ms: i64) -> usize {
        let ttl = self.ttl_ms;
        let before = self.bindings.len();
        self.bindings
            .retain(|_, binding| now_ms.saturating_sub(binding.bound_at_ms) < ttl);
        before - self.bindings.len()
    }

    /// 当前 GC 阈值。
    ///
    /// **可观测缝**：摊销策略的效果（「清扫后阈值上抬，不再每次 bind 全表扫描」）是
    /// 一个内部性能性质，靠计时或读盘计数都推断不出来。把它暴露成只读访问器，
    /// 测试就能**直接**断言。`#[cfg(test)]` 保证发布构建零成本。
    #[cfg(test)]
    pub(crate) fn gc_threshold(&self) -> usize {
        self.gc_at
    }
}

/// 派生粘性键：`region|model|会话键`。
///
/// ## ★ 键里必须是**会话**级标识，不能是轮级
///
/// 本模块的用途（见文件头）是**跨轮**的：同一会话的上下文别落到不同账号上、
/// 缓存前缀命中率别归零。而 `conversation_request_id`（轮主键）**每轮都换** ——
/// `relay::prepare_body` 调 `resolve_conversation_request_id(inbound, None, turn_key)`，
/// `session_key` 传的是 `None`，因此它实际按「最后一条 user 文本」派生
/// ⇒ 用它当键，**每一轮都是新键、绑定永远命中不了**，接线等于没接（2026-09-28 实测发现）。
///
/// ⇒ 调用方必须传 `RelayRequest::conversation_id`（客户端会话 id）。它**可能缺失**，
/// 此时调用方应**整段跳过粘性**（保持原行为），而**不要**退化成轮主键 ——
/// 那只会制造一个永不命中的键，让 `sticky_sessions` 看起来非 0 却毫无作用。
///
/// 把模型纳入键是刻意的：同一会话切到不同模型时，应当允许落到各自最合适的账号
/// （否则「模型级限流只封锁该模型」的优势会被粘性抵消）。
pub fn sticky_key(region: &str, model: &str, session_key: &str) -> String {
    format!("{region}|{model}|{session_key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000_000_000;

    #[test]
    fn bind_then_get_returns_uid() {
        let mut table = StickyTable::new(1000);
        table.bind("k", "u1", NOW);
        assert_eq!(table.get("k", NOW), Some("u1"));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn expired_binding_is_invisible() {
        let mut table = StickyTable::new(1000);
        table.bind("k", "u1", NOW);
        assert_eq!(table.get("k", NOW + 999), Some("u1"), "TTL 内有效");
        assert_eq!(table.get("k", NOW + 1000), None, "边界即失效");
    }

    /// ★★ 安全审计 P2-10：**`len()` 是表大小，`len_active()` 才是生效中的数量**。
    ///
    /// 改造前 `len()` 的文档写「不含已过期条目」而实现返回 `bindings.len()`，
    /// 于是 `/status` 的 `sticky_sessions` 只增不减（GC 是摊销触发的，不是每读一次就跑）。
    ///
    /// 变异验证：把 `/status` 改回 `len()` ⇒ 第二条断言红。
    #[test]
    fn len_active_excludes_expired_bindings_without_waiting_for_gc() {
        let mut table = StickyTable::new(1000);
        table.bind("a", "u1", NOW);
        table.bind("b", "u2", NOW);
        // 只有 a 还活着。
        let later = NOW + 1_500;
        table.bind("a", "u1", later); // 刷新 a
        assert_eq!(table.get("b", later), None, "前置：b 已过期");

        assert_eq!(table.len(), 2, "表里仍是两条（GC 还没触发）");
        assert_eq!(
            table.len_active(later),
            1,
            "生效中的只有 a —— `/status` 数的是这个"
        );
        // 全部过期 ⇒ 0（而不是「表里有几条算几条」）。
        assert_eq!(table.len_active(later + 10_000), 0);
    }

    #[test]
    fn bind_refreshes_timestamp() {
        let mut table = StickyTable::new(1000);
        table.bind("k", "u1", NOW);
        table.bind("k", "u1", NOW + 900);
        assert_eq!(table.get("k", NOW + 1500), Some("u1"), "重新绑定应刷新计时");
    }

    #[test]
    fn unbind_removes_binding() {
        let mut table = StickyTable::new(1000);
        table.bind("k", "u1", NOW);
        table.unbind("k");
        assert_eq!(table.get("k", NOW), None);
        assert!(table.is_empty());
    }

    #[test]
    fn gc_drops_only_expired_entries() {
        let mut table = StickyTable::new(1000);
        table.bind("old", "u1", NOW);
        table.bind("new", "u2", NOW + 900);
        assert_eq!(table.gc(NOW + 1500), 1, "只应清理一条");
        assert_eq!(table.get("new", NOW + 1500), Some("u2"));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn empty_key_or_uid_is_ignored() {
        let mut table = StickyTable::new(1000);
        table.bind("", "u1", NOW);
        table.bind("k", "", NOW);
        assert!(table.is_empty());
    }

    #[test]
    fn non_positive_ttl_falls_back_to_default() {
        let table = StickyTable::new(0);
        assert_eq!(table.ttl_ms(), 30 * 60 * 1000);
        let table = StickyTable::new(-5);
        assert_eq!(table.ttl_ms(), 30 * 60 * 1000);
    }

    #[test]
    fn sticky_key_separates_region_and_model() {
        assert_eq!(sticky_key("cn", "glm-5.2", "req"), "cn|glm-5.2|req");
        assert_ne!(
            sticky_key("cn", "glm-5.2", "req"),
            sticky_key("cn", "deepseek-v4-flash", "req"),
            "不同模型不得共享绑定"
        );
        assert_ne!(
            sticky_key("cn", "glm-5.2", "req"),
            sticky_key("global", "glm-5.2", "req"),
            "不同域不得共享绑定"
        );
    }

    /// ★ 接上写路径后，表**必须有界**：写入路径上的摊销 GC 要真的清掉过期条目。
    ///
    /// 接线前 `bind` 没有任何调用方、表恒为空，所以「无界增长」只是理论问题；
    /// 接线后它是**真实**的（每个会话一条，随进程寿命累积）。
    #[test]
    fn bind_amortizes_gc_so_long_runs_stay_bounded() {
        let mut table = StickyTable::new(1000);

        // 先写满阈值：此时条目全部**未过期**，GC 清不掉任何东西 ——
        // 正是这种情况最容易退化成「每次 bind 全表扫描」，所以阈值必须上抬。
        for index in 0..INITIAL_GC_THRESHOLD {
            table.bind(&format!("k{index}"), "u", NOW);
        }
        assert_eq!(
            table.len(),
            INITIAL_GC_THRESHOLD,
            "全部未过期 ⇒ 一条都不该被清"
        );

        // 让上面这批过期，再写一条 ⇒ 触发 GC，旧条目被清掉。
        let later = NOW + 5000;
        table.bind("fresh", "u", later);
        assert_eq!(table.len(), 1, "过期条目应被清理，只剩刚写的那条");
        assert_eq!(table.get("fresh", later), Some("u"));
        assert_eq!(table.get("k0", later), None, "过期绑定必须不可见");
    }

    /// ★ 摊销的关键分支：**清扫没释放任何条目**时阈值必须上抬。
    ///
    /// 这正是最容易写错的地方：若只写「`len >= 阈值` 就 GC」，那么在「条目大多未过期」
    /// 的正常长跑场景下，**每次 `bind` 都会全表扫描** —— 那才是真的性能问题。
    ///
    /// （反向情形也一并锁定：若一次清扫把表清空，阈值应回落到下限 —— 空表配大阈值没有意义。）
    #[test]
    fn gc_threshold_is_raised_only_when_a_sweep_frees_nothing() {
        let mut table = StickyTable::new(1000);
        assert_eq!(table.gc_threshold(), INITIAL_GC_THRESHOLD, "初始阈值");

        // ① 写满阈值 + 1：第 65 次写入触发 GC，但**全是未过期条目、清不掉任何东西**
        //    ⇒ 阈值必须上抬（否则此后每次 bind 都全表扫描）。
        for index in 0..=INITIAL_GC_THRESHOLD {
            table.bind(&format!("live{index}"), "u", NOW);
        }
        assert_eq!(
            table.len(),
            INITIAL_GC_THRESHOLD + 1,
            "未过期条目一条都不该丢"
        );
        assert_eq!(
            table.gc_threshold(),
            INITIAL_GC_THRESHOLD * 2,
            "清扫释放 0 条 ⇒ 按当前规模翻倍"
        );

        // ② 反向：让全部条目过期，再填到上抬后的阈值 ⇒ 下一次写入清空整表 ⇒ 阈值回落。
        let target = table.gc_threshold();
        let mut index = 0;
        while table.len() < target {
            table.bind(&format!("stale{index}"), "u", NOW);
            index += 1;
        }
        assert_eq!(table.len(), target);

        let later = NOW + 10_000;
        table.bind("after", "u", later);
        assert_eq!(table.len(), 1, "过期条目应被清掉，只剩刚写的这条");
        assert_eq!(
            table.gc_threshold(),
            INITIAL_GC_THRESHOLD,
            "清空后阈值应回落到下限 —— 空表配大阈值没有意义"
        );
    }

    /// 缺省 TTL 是 30 分钟（`new(0)` 与 `Default` 都要落到它）。
    #[test]
    fn default_falls_back_to_thirty_minutes() {
        assert_eq!(StickyTable::default().ttl_ms(), 30 * 60 * 1000);
        assert_eq!(StickyTable::new(0).ttl_ms(), 30 * 60 * 1000);
        assert_eq!(StickyTable::new(-1).ttl_ms(), 30 * 60 * 1000);
        assert_eq!(StickyTable::new(1234).ttl_ms(), 1234, "正数原样保留");
    }
}
