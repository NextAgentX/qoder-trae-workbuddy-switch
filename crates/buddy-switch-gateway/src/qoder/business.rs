//! 业务会话锚点：同一会话的多轮请求复用同一个 `business` 三元组。
//!
//! 上游用量明细按 `business` 聚合。本模块按 `(区域, uid, 会话键)` 缓存并复用
//! `id / begin_at / name`，网关的多轮请求在官网形成一条会话记录。
//!
//! 会话键复用 [`super::payload::sticky_identity_for_request`]，同一会话的轮次
//! 落到同一个锚点。key 包含区域与 uid，换号或跨区域时使用各自账号的锚点，
//! 会话账目按账号隔离。
//!
//! ## 生命周期
//!
//! TTL 与 [`crate::StickyTable`] 同口径（默认 30 分钟），GC 在写入路径上
//! **摊销**执行。命中时刷新 `seen_at_ms`，活跃会话继续复用同一锚点；不同会话
//! 各自建立锚点。

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

/// 一条业务会话锚点（上游 `business` 的三个稳定字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessAnchor {
    pub id: String,
    pub begin_at_ms: i64,
    pub name: String,
}

/// 业务锚点表（按 `(区域, uid, 会话键)` 缓存，带 TTL）。
#[derive(Debug)]
pub struct BusinessTable {
    entries: HashMap<String, Entry>,
    ttl_ms: i64,
    gc_at: usize,
}

#[derive(Debug, Clone)]
struct Entry {
    anchor: BusinessAnchor,
    seen_at_ms: i64,
}

/// 首次触发 GC 的条目数；常规会话规模下写入路径保持轻量。
const INITIAL_GC_THRESHOLD: usize = 64;

/// 会话键：区域、uid 与 conversation 用单元分隔符拼接，保证边界无歧义。
pub fn key(region: &str, uid: &str, conversation: &str) -> String {
    let mut key = String::with_capacity(region.len() + uid.len() + conversation.len() + 2);
    key.push_str(region);
    key.push('\u{1f}');
    key.push_str(uid);
    key.push('\u{1f}');
    key.push_str(conversation);
    key
}

/// 从上游请求体读取当前的 `business` 三元组（锚点未命中时用它做种子）。
pub fn anchor_from_body(body: &Value) -> Option<BusinessAnchor> {
    let business = body.get("business")?.as_object()?;
    let id = business.get("id")?.as_str()?.trim();
    let begin_at_ms = business.get("begin_at")?.as_i64()?;
    let name = business.get("name").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() {
        return None;
    }
    Some(BusinessAnchor {
        id: id.to_string(),
        begin_at_ms,
        name: name.to_string(),
    })
}

/// 用锚点覆盖请求体里的三个字段（其余 `business` 字段原样保留）。
pub fn apply_to_body(body: &mut Value, anchor: &BusinessAnchor) {
    let Some(business) = body.get_mut("business").and_then(Value::as_object_mut) else {
        return;
    };
    business.insert("id".into(), Value::String(anchor.id.clone()));
    business.insert("begin_at".into(), Value::from(anchor.begin_at_ms));
    business.insert("name".into(), Value::String(anchor.name.clone()));
}

impl Default for BusinessTable {
    fn default() -> Self {
        Self::new(0)
    }
}

impl BusinessTable {
    /// `ttl_ms <= 0` 时回落 30 分钟（与 [`crate::StickyTable`] 同默认值）。
    pub fn new(ttl_ms: i64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl_ms: if ttl_ms > 0 { ttl_ms } else { 30 * 60 * 1000 },
            gc_at: INITIAL_GC_THRESHOLD,
        }
    }

    pub fn ttl_ms(&self) -> i64 {
        self.ttl_ms
    }

    /// 按 TTL 过滤后的生效锚点数。
    pub fn len_active(&self, now_ms: i64) -> usize {
        self.entries
            .values()
            .filter(|entry| now_ms.saturating_sub(entry.seen_at_ms) < self.ttl_ms)
            .count()
    }

    /// 按 `(key, now_ms)` 取得有效锚点；需要新锚点时调用 `build` 并写入。
    ///
    /// 命中时刷新 `seen_at_ms`，活跃会话继续复用同一锚点。
    /// 写入路径做**摊销 GC**：达到阈值才清理，清理后按当前规模上抬阈值。
    pub fn anchor_or_insert(
        &mut self,
        key: &str,
        now_ms: i64,
        build: impl FnOnce() -> BusinessAnchor,
    ) -> BusinessAnchor {
        if let Some(entry) = self.entries.get_mut(key) {
            if now_ms.saturating_sub(entry.seen_at_ms) < self.ttl_ms {
                entry.seen_at_ms = now_ms;
                return entry.anchor.clone();
            }
        }
        if self.entries.len() >= self.gc_at {
            self.gc(now_ms);
            self.gc_at = self
                .entries
                .len()
                .saturating_mul(2)
                .max(INITIAL_GC_THRESHOLD);
        }
        let anchor = build();
        self.entries.insert(
            key.to_string(),
            Entry {
                anchor: anchor.clone(),
                seen_at_ms: now_ms,
            },
        );
        anchor
    }

    /// 清理过期条目，返回清理数量。
    pub fn gc(&mut self, now_ms: i64) -> usize {
        let ttl = self.ttl_ms;
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| now_ms.saturating_sub(entry.seen_at_ms) < ttl);
        before - self.entries.len()
    }

    #[cfg(test)]
    fn gc_threshold(&self) -> usize {
        self.gc_at
    }
}

/// 进程级锚点表的共享句柄。
pub type SharedBusinessTable = std::sync::Arc<Mutex<BusinessTable>>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn anchor(id: &str, begin: i64, name: &str) -> BusinessAnchor {
        BusinessAnchor {
            id: id.to_string(),
            begin_at_ms: begin,
            name: name.to_string(),
        }
    }

    #[test]
    fn same_conversation_reuses_anchor() {
        let mut table = BusinessTable::new(60_000);
        let key = key("cn", "uid-1", "session:abc");
        let first = table.anchor_or_insert(&key, 1_000, || anchor("b1", 1_000, "first"));
        let second = table.anchor_or_insert(&key, 5_000, || anchor("b2", 5_000, "second"));
        assert_eq!(first, second);
        assert_eq!(second.id, "b1");
        assert_eq!(second.begin_at_ms, 1_000);
        assert_eq!(second.name, "first");
    }

    #[test]
    fn uid_is_part_of_the_key() {
        let mut table = BusinessTable::new(60_000);
        let a = table.anchor_or_insert(&key("cn", "uid-a", "conv"), 1_000, || {
            anchor("a", 1_000, "a")
        });
        let b = table.anchor_or_insert(&key("cn", "uid-b", "conv"), 1_000, || {
            anchor("b", 1_000, "b")
        });
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn region_is_part_of_the_key() {
        let mut table = BusinessTable::new(60_000);
        let cn = table.anchor_or_insert(&key("cn", "uid", "conv"), 1_000, || {
            anchor("cn", 1_000, "cn")
        });
        let global = table.anchor_or_insert(&key("global", "uid", "conv"), 1_000, || {
            anchor("global", 1_000, "global")
        });
        assert_ne!(cn.id, global.id);
    }

    #[test]
    fn expired_anchor_is_rebuilt() {
        let mut table = BusinessTable::new(1_000);
        let key = key("cn", "uid", "conv");
        let first = table.anchor_or_insert(&key, 0, || anchor("old", 0, "old"));
        let second = table.anchor_or_insert(&key, 1_000, || anchor("new", 1_000, "new"));
        assert_ne!(first.id, second.id);
        assert_eq!(second.id, "new");
    }

    #[test]
    fn gc_removes_expired_entries() {
        let mut table = BusinessTable::new(1_000);
        table.anchor_or_insert(&key("cn", "uid", "a"), 0, || anchor("a", 0, "a"));
        table.anchor_or_insert(&key("cn", "uid", "b"), 5_000, || anchor("b", 5_000, "b"));
        assert_eq!(table.gc(2_000), 1);
        assert_eq!(table.len_active(2_000), 1);
    }

    #[test]
    fn gc_threshold_is_raised_after_sweep() {
        let mut table = BusinessTable::new(1_000);
        for index in 0..INITIAL_GC_THRESHOLD {
            table.anchor_or_insert(&key("cn", "uid", &format!("c{index}")), 0, || {
                anchor("b", 0, "b")
            });
        }
        assert_eq!(table.gc_threshold(), INITIAL_GC_THRESHOLD);
        table.anchor_or_insert(&key("cn", "uid", "fresh"), 5_000, || {
            anchor("fresh", 5_000, "fresh")
        });
        assert!(table.gc_threshold() >= INITIAL_GC_THRESHOLD);
        assert_eq!(table.len_active(5_000), 1);
    }

    #[test]
    fn apply_only_overwrites_three_fields() {
        let mut body = json!({
            "business": {
                "id": "old",
                "begin_at": 1,
                "name": "old-name",
                "product": "cli",
                "version": "0.1.43",
                "type": "agent",
                "stage": "start"
            }
        });
        apply_to_body(&mut body, &anchor("new", 42, "new-name"));
        let business = body.get("business").unwrap();
        assert_eq!(business["id"], "new");
        assert_eq!(business["begin_at"], 42);
        assert_eq!(business["name"], "new-name");
        assert_eq!(business["product"], "cli");
        assert_eq!(business["stage"], "start");
    }

    #[test]
    fn anchor_from_body_requires_id_and_begin_at() {
        let body = json!({"business": {"id": "b", "begin_at": 7, "name": "n"}});
        assert_eq!(anchor_from_body(&body), Some(anchor("b", 7, "n")));
        assert!(anchor_from_body(&json!({"business": {"id": ""}})).is_none());
        assert!(anchor_from_body(&json!({"messages": []})).is_none());
    }
}
