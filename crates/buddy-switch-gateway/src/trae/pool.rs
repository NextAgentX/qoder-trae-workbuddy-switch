//! Trae 网关账号池：从**磁盘上的账号库/冷却/积分缓存**派生可路由账号。
//!
//! ## 为什么池不自己存状态
//!
//! 参考实现的池把 `credits` / `cooldown_until` / `disabled` 全放在内存里，只在启动
//! 时同步一次。在本仓库里这会立刻出问题：Trae 的签到页与网关页读的是**同一批账号**，
//! 若网关只在启动时同步，用户在签到页手动「解除冷却」后网关仍会认为该账号冷却中，
//! 两个页面互相打脸。
//!
//! 因此本池的取值原则是：**每次选号都重新从磁盘派生**，唯一的例外是「选号结果」
//! 这种纯计算产物。写回也走既有通道——
//! [`buddy_switch_core::modules::trae::credits::save_cooldown_for`]，于是冷却状态的
//! 唯一真相是**该产品线自己的**冷却文件（`account_cooldowns.json` /
//! `account_cooldowns.trae_cn.json`），签到页与网关页看到的是同一份。
//!
//! **写读同源**：池构造期固化所属变体（[`TraePool::for_variant`]），写回冷却必须用
//! 该变体选文件——若写成恒定默认变体，CN 账号的冷却会落进 Work 文件、CN 池却读 CN 文件，
//! 死号永不被冷却且被反复选中（「读侧改了、写侧没改」的半成品缺陷）。
//!
//! 代价是每个请求要读 3 个小 JSON（账号库 / 冷却 / 剩余积分）。对本机单用户工具
//! 这是可接受的，且与 WorkBuddy 网关「每请求 `load_accounts_for`」的既有做法一致。

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};

use buddy_switch_core::modules::trae::variant::TraeVariant;
use buddy_switch_core::modules::trae::{account, credits, device};
use buddy_switch_core::modules::upstream::SESSION_DEAD_MARKERS;

use crate::pool::entry::CoolKind;
use crate::pool::entry_like::PoolEntryLike;
use crate::pool::pick::{pick_with_preference, PickPolicy};
use crate::pool::{PoolSummary, UpstreamEvent};

/// 上游错误的治理类别。
///
/// 取值字符串与 [`credits::classify_error`] 的词汇表**逐字一致**，因此可以直接
/// 落进 `account_cooldowns.json`，被签到页原样展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraeErrKind {
    /// 不归因于账号（例如「模型配置为空」），不冷却。
    None,
    /// 套餐额度用尽。
    PlanLimit,
    /// 触发限流。
    SoftRate,
    /// 会话失效（上游**真的**作废了会话：`12153` / `Offline user session not found`）。
    ///
    /// 永久禁用。**只有**带作废标记时才该落到这里（见 [`TraeErrKind::AuthRejected`]）。
    SessionDead,
    /// **凭据被拒**（401/403，但响应体里**没有**会话作废标记）。
    ///
    /// ## 为什么必须与 `SessionDead` 分开（issue #108 §P0-B）
    ///
    /// 改造前 401 一律判 `SessionDead` ⇒ `until = 9999999999`（永久）+ 派生 `disabled`
    /// ⇒ 账号进入**静默终态**：不路由、不签到、不解冻、不探测回填。而 401 的绝大多数
    /// 成因是 **access token 过期**，保活刷新一轮就能自愈 —— 把可自愈的问题打成终态，
    /// 用户看到的是「号没坏却再也用不了」，而且重新登录也救不回来（冷却表没人清）。
    ///
    /// 本档的处置与 WorkBuddy 侧 `UpstreamEvent::AuthRejected` **同一口径**：
    /// 换号 + 浅冷却 60 秒，**不计数、不禁用**。
    AuthRejected,
    /// 接口不存在。
    NotFound,
    /// 上游 5xx。
    Server,
    /// 上游 4xx。
    Client,
    /// 业务错误码。
    Business,
}

/// 上游 `(code, message)` → **对外**错误形状（issue #129）。
///
/// ## 为什么对外必须换一套形状
///
/// 客户端（DeepSeek Harness 的 `openai-completions` 适配器）**只做文本正则**：
/// `429` / `rate limit` → `RATE_LIMIT`（可重试），`5\d\d` → `SERVER`（可重试），
/// `401|403` → `AUTH`（**终态**，且 UI 渲染成「API 密钥无效」）。
///
/// Trae 的上游码（`1005` 套餐限额、`4008` 限流、`12153` 会话失效）一个都命中不了，
/// 原样透出只会让客户端归成 `PI_AI_ERROR` —— 一次终态，连重试的机会都没有。
/// 而流中途的凭据/额度问题本来可以靠「换号 + 重跑」救回来。
///
/// 因此这里把内部类别**收敛**成三种对外形状（口径与 WorkBuddy 侧
/// [`crate::failover::client_error`] 一致，两处不许分家）。
pub fn client_error_of(code: i64, message: &str) -> crate::failover::ClientError {
    let kind = classify_solo(code, message);
    let event = match kind {
        // 额度/限流/会话失效/凭据被拒 ⇒ 统一 429：对客户端都是「这个号不行了，换一个」。
        // ★ `SessionDead` 也走 429 而不是 401 —— 见 issue #129 §5.2 改动 2。
        TraeErrKind::PlanLimit
        | TraeErrKind::SoftRate
        | TraeErrKind::SessionDead
        | TraeErrKind::AuthRejected
        // 未知业务码：换号也可能救（`should_rotate` 对它为真），给可重试更安全。
        | TraeErrKind::Business => UpstreamEvent::SoftRate {
            reset_at_ms: None,
            model_scoped: false,
        },
        TraeErrKind::Server => UpstreamEvent::Server,
        TraeErrKind::NotFound | TraeErrKind::Client | TraeErrKind::None => UpstreamEvent::Client,
    };
    crate::failover::client_error(&event, "", "")
}

impl TraeErrKind {
    /// 落盘用的类型名。
    pub fn as_str(self) -> &'static str {
        match self {
            TraeErrKind::None => "None",
            TraeErrKind::PlanLimit => "PlanLimit",
            TraeErrKind::SoftRate => "SoftRate",
            TraeErrKind::SessionDead => "SessionDead",
            TraeErrKind::AuthRejected => "AuthRejected",
            TraeErrKind::NotFound => "NotFound",
            TraeErrKind::Server => "Server",
            TraeErrKind::Client => "Client",
            TraeErrKind::Business => "BusinessError",
        }
    }

    /// 是否永久禁用该账号（只有会话失效如此）。
    pub fn is_permanent(self) -> bool {
        matches!(self, TraeErrKind::SessionDead)
    }
}

/// 按 HTTP 状态码 + **响应体**分类（上游在**建立连接阶段**就失败时使用）。
///
/// ## 为什么必须带 body（issue #108 §P0-B）
///
/// 判定顺序不能反：**先**看 body 里有没有会话作废标记（`12153` /
/// `Offline user session not found`，与 WB 侧同一常量来源
/// [`SESSION_DEAD_MARKERS`]），**再**按状态码归类。
///
/// - 有标记 ⇒ `SessionDead`（永久，需重新登录）；
/// - 裸 401/403（无标记）⇒ `AuthRejected`（浅冷却 60s，可自愈）。
///
/// 改造前 Trae 侧**完全不识别 `12153`**，所以「真正的作废」与「token 过期」只能共用
/// 裸 401 这一个判据 —— 怎么选都错。先补识别，才有资格降级。
pub fn classify_http(status: u16, body: &str) -> TraeErrKind {
    if SESSION_DEAD_MARKERS
        .iter()
        .any(|marker| body.contains(marker))
    {
        return TraeErrKind::SessionDead;
    }
    match status {
        // 403 与 401 同族：凭据被拒，但未必是终态。
        401 | 403 => TraeErrKind::AuthRejected,
        429 => TraeErrKind::SoftRate,
        404 => TraeErrKind::NotFound,
        code if code >= 500 => TraeErrKind::Server,
        code if code >= 400 => TraeErrKind::Client,
        _ => TraeErrKind::None,
    }
}

/// 按 SOLO 业务错误码 + 文案分类（上游在 **SSE 流内**报错时使用）。
///
/// 顺序有讲究：先看业务码，再看文案兜底，最后才按数字区间归类。把 `1005`（套餐额度）
/// 误判成 `Client` 会让一个额度耗尽的账号在 10 分钟后被反复重试。
pub fn classify_solo(code: i64, message: &str) -> TraeErrKind {
    // ★ issue #108：会话作废标记优先于一切业务码判读 —— 它是**唯一**能把
    //   「真作废」与「token 过期」区分开的证据，放在后面会被 code 分支吞掉。
    if SESSION_DEAD_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        return TraeErrKind::SessionDead;
    }
    let lower = message.to_lowercase();
    if code == 1005 || lower.contains("plan") {
        return TraeErrKind::PlanLimit;
    }
    // 模型配置为空是**模型**问题而非账号问题，冷却账号没有意义。
    if code == 4001 || lower.contains("model config is empty") {
        return TraeErrKind::None;
    }
    if code == 4008
        || lower.contains("quota")
        || lower.contains("exceeded")
        || lower.contains("rate")
    {
        return TraeErrKind::SoftRate;
    }
    match code {
        // 与 `classify_http` 同口径：裸 401/403 是「凭据被拒」，不是「会话作废」。
        401 | 403 => TraeErrKind::AuthRejected,
        429 => TraeErrKind::SoftRate,
        404 => TraeErrKind::NotFound,
        // **只有真正的 HTTP 状态码区间**才映射为 Client / Server。
        // 必须显式写区间上界：`code >= 500` 会把业务码（1005 / 4001 / 1234…）
        // 一并吞成 Server，而业务码恰恰是最需要被单独识别的一类。
        value if (400..500).contains(&value) => TraeErrKind::Client,
        value if (500..600).contains(&value) => TraeErrKind::Server,
        0 => TraeErrKind::None,
        _ => TraeErrKind::Business,
    }
}

/// 池中一个账号的可路由视图。
#[derive(Debug, Clone)]
pub struct TraePoolEntry {
    pub uid: String,
    pub name: String,
    /// 已剥离 `Cloud-IDE-JWT ` 前缀的裸令牌。
    pub jwt: String,
    pub device_id: String,
    pub machine_id: String,
    pub credits: Option<f64>,
    pub credits_expire_at: Option<i64>,
    /// 会话失效 → 永久禁用（与签到侧 `SessionDead` 语义一致）。
    pub disabled: bool,
    pub cooldown_until: i64,
    pub cooldown_reason: String,
    /// 冷却类别（供共用选号内核判定「硬冷却不兜底」）。
    ///
    /// 由冷却文件的 `type` 映射：`PlanLimit`（套餐额度用尽）⇒ `Hard`，
    /// 其余（限流 / 4xx / 5xx）⇒ `Soft`，`SessionDead` ⇒ `None`
    /// （它已由 `disabled` 表达为终态，再算冷却会重复计数）。
    ///
    /// 与 WorkBuddy 的语义对齐：额度类冷却期间强试**必然再失败**且浪费一次上游调用，
    /// 因此不参与「全冷却兜底」。
    pub cool_kind: Option<CoolKind>,
    /// 最近被选中时刻（毫秒；`0` = 从未）。
    ///
    /// ★ 安全审计 P1-5：**现在跨请求保留**（[`TraePool::sync_for`] 会按 uid 继承）。
    /// 改造前每次重建都归零，于是「顺序」档退化成「永远选 uid 最小的那个」。
    pub last_used_ms: i64,
    /// 选中单调序号（**跨请求保留**，理由同上）。
    pub used_seq: u64,
}

impl TraePoolEntry {
    /// 该账号在 `now` 时刻是否可用。
    pub fn healthy(&self, now: i64) -> bool {
        !self.disabled && (self.cooldown_until == 0 || now >= self.cooldown_until)
    }

    /// 积分是否已过期（`expire_at` 缺失或为 0 视为「无过期时间」，不算过期）。
    pub fn credits_expired(&self, now: i64) -> bool {
        self.credits_expire_at
            .map(|expire| expire > 0 && expire < now)
            .unwrap_or(false)
    }

    /// 是否零积分（`None` 表示未知，不因此排除）。
    pub fn no_credits(&self) -> bool {
        self.credits.map(|value| value <= 0.0).unwrap_or(false)
    }

    /// 不可路由的原因；`None` 表示可用。
    pub fn rejection(&self, now: i64) -> Option<&'static str> {
        if self.disabled {
            Some("会话失效（需重新登录）")
        } else if self.cooldown_until > now {
            Some("冷却中")
        } else if self.credits_expired(now) {
            Some("积分已过期")
        } else if self.no_credits() {
            Some("零积分")
        } else {
            None
        }
    }
}

/// 选中的账号（携带出站所需的全部凭据）。
#[derive(Debug, Clone)]
pub struct PickedTraeAccount {
    pub uid: String,
    pub name: String,
    pub jwt: String,
    pub device_id: String,
    pub machine_id: String,
}

/// 账号池。
///
/// 每个池**绑定唯一的产品线变体**（[`Self::variant`]）：账号库 / 冷却 / 剩余积分
/// 都按该变体分家。变体在构造期固化（[`Self::for_variant`]），并在 [`Self::sync_for`]
/// 中以入参校正一次——因此「池的变体」只有一个来源，**不可能**出现「池是 CN、
/// 写盘却是 Work」这种读写分离。
#[derive(Debug, Default)]
pub struct TraePool {
    entries: Vec<TraePoolEntry>,
    /// 本池所属的产品线变体。冷却写回（[`Self::apply_error`]）据此选文件。
    variant: TraeVariant,
    /// 选号单调序号（供共用内核的 LRU 语义使用）。
    ///
    /// ⚠️ 池每次 `sync_for` 重建条目，但 `pick_seq` **不重置** —— 它是进程内的
    /// 单调计数，重置会让 LRU 比较失去意义。
    pick_seq: u64,
}

/// 把上一轮条目的选号序号继承到新条目上，并返回**对齐后的 `pick_seq`**。
///
/// ## 为什么必须独立成纯函数（可单测）
///
/// `sync_for` 需要读账号库（文件），而这条继承逻辑是最容易被「重建时忘了带」的地方 ——
/// 出错的症状是「顺序档又退回按 uid 升序」，且只有连发两个请求才看得出来。
/// 抽出来之后单测能直接钉住它，不必搭文件夹具。
///
/// `pick_seq` 必须**不小于**任何已用序号：共用内核的 `mark_selected` 会
/// `pick_seq += 1` 再写进条目，若起点落后于历史值，新选中的号会与历史条目并列。
fn inherit_order(
    entries: &mut [TraePoolEntry],
    previous: &std::collections::HashMap<String, (u64, i64)>,
) -> u64 {
    for entry in entries.iter_mut() {
        if let Some((seq, at)) = previous.get(&entry.uid) {
            entry.used_seq = *seq;
            entry.last_used_ms = *at;
        }
    }
    entries
        .iter()
        .map(|entry| entry.used_seq)
        .max()
        .unwrap_or(0)
}

impl TraePool {
    /// 新建空池（默认变体；首次 [`Self::sync_for`] 前为空）。
    pub fn new() -> Self {
        Self::for_variant(TraeVariant::default())
    }

    /// 新建属于**指定变体**的空池。
    ///
    /// 变体在构造期固化：`apply_error` 依据它选冷却文件。配合 `sync_for` 的校正，
    /// 保证写入侧（冷却落哪个文件）与读取侧（`entries_for` / `load_*_for`）永远同源。
    pub fn for_variant(variant: TraeVariant) -> Self {
        Self {
            entries: Vec::new(),
            variant,
            pick_seq: 0,
        }
    }

    /// 从**指定变体**的磁盘数据重建池。返回条目数。
    ///
    /// 顺序沿用账号库顺序（用户可见顺序），不排序——`pick` 的择优逻辑与顺序无关。
    ///
    /// ## 为什么必须带 `variant`
    ///
    /// 两条产品线的账号库 / 冷却 / 剩余积分**各自分家**（`*_for(variant)`）。
    /// 改造前这里只读默认变体（`account::entries()` = `entries_for(TraeWork)`），
    /// 于是**只装 Trae CN 账号的用户网关池恒为空、`pick` 永远返回 `None`、调用必然失败**——
    /// 这就是本次「池按变体分家」要修掉的既有功能洞。
    ///
    /// **设备标识不需要分家**：`device::derive(uid)` 是 uid 的纯函数、与变体无关，
    /// 因此这里直接用 `device::derive`，无需 `ensure_for_variant`。
    pub fn sync_for(&mut self, variant: TraeVariant) -> usize {
        // 以入参校正本池变体：即使该池此前由别的路径以默认值创建，这里也保证
        // 「读哪条线」与「写哪条线」一致（`apply_error` 依据 `self.variant` 落盘）。
        self.variant = variant;
        let remaining = credits::load_remaining_for(variant);
        let cooldowns = credits::load_cooldowns_for(variant);

        // ★★ 安全审计 P1-5：**选号序号必须跨请求保留**。
        //
        // `sync_for` 每个请求都会整体重建 `entries`。改造前重建时把 `used_seq` 一律
        // 置 0 ⇒ 共用内核的「顺序」档（`PickMode::Sequential`，语义是「粘住最近用过的
        // 那个号」）在 Trae 侧退化成「永远选 uid 最小的那个」—— 而 Trae 页面上
        // 摆着这一档（issue #102 的功能在这里名不副实）。
        //
        // 按 uid 继承，与 Qoder 侧的做法一致（`qoder/pool.rs::sync_for`）。
        let previous_order: std::collections::HashMap<String, (u64, i64)> = self
            .entries
            .iter()
            .map(|entry| (entry.uid.clone(), (entry.used_seq, entry.last_used_ms)))
            .collect();

        // 账号库按**区域**分家（见 `core::modules::trae::region`）：国内两个产品线标识
        // 读到的是**同一本**库。这里显式传 `variant.region()`，让"按区域取号"这件事
        // 在调用点就看得见，而不是靠 `*_for(variant)` 内部的隐式折算。
        self.entries = account::entries_for_region(variant.region())
            .into_iter()
            .map(|(uid, raw)| {
                // ★★ issue #108 §P0-A：冷却记录必须**自证它判的是哪一份凭据**。
                //
                // 否则会出现最坏的一种不一致：卡片（走 `account_view`）已经因指纹不符而
                // 显示正常，池这边却仍把它 `disabled` ⇒ 界面说「可用」、网关永远不选它。
                // 判定与视图必须同源，故这里用同一个 `matches_credential`。
                let credential_exp =
                    buddy_switch_core::modules::trae::jwt::parse(&raw.jwt).exp_timestamp;
                let cooldown = cooldowns
                    .cooldowns
                    .get(&uid)
                    .filter(|entry| entry.matches_credential(credential_exp));
                let device = device::derive(&uid);
                TraePoolEntry {
                    jwt: clean_jwt(&raw.jwt),
                    device_id: device.device_id,
                    // `x-machine-id` 用与 session-id 不同的 salt 派生，避免两个字段撞车。
                    machine_id: device::rand_hex_salted(64, "mach", Some(&uid)),
                    name: if raw.name.trim().is_empty() {
                        uid.clone()
                    } else {
                        raw.name.clone()
                    },
                    credits: remaining.credits.get(&uid).copied(),
                    credits_expire_at: remaining.expire_times.get(&uid).copied(),
                    disabled: cooldown
                        .map(|entry| entry.error_type == "SessionDead")
                        .unwrap_or(false),
                    cooldown_until: cooldown.map(|entry| entry.until).unwrap_or(0),
                    cooldown_reason: cooldown
                        .map(|entry| entry.reason.clone())
                        .unwrap_or_default(),
                    cool_kind: cooldown.and_then(|entry| match entry.error_type.as_str() {
                        // 会话失效已由 `disabled` 表达为终态，再标冷却会重复计数。
                        "SessionDead" => None,
                        // 套餐额度用尽：强试必然再失败且浪费一次上游调用 ⇒ 硬冷却。
                        "PlanLimit" => Some(CoolKind::Hard),
                        // 限流 / 4xx / 5xx：值得再试一次 ⇒ 软冷却。
                        _ => Some(CoolKind::Soft),
                    }),
                    // 默认 0（从未用过）；若池里已有同 uid 的历史条目，
                    // 下面会按 uid 继承 —— 见本函数开头的说明。
                    last_used_ms: 0,
                    used_seq: 0,
                    uid,
                }
            })
            .filter(|entry| !entry.jwt.is_empty())
            .collect();

        // 继承历史选号序号（见本函数开头的说明）。
        self.pick_seq = inherit_order(&mut self.entries, &previous_order);

        self.entries.len()
    }

    /// 当前条目（只读）。
    pub fn entries(&self) -> &[TraePoolEntry] {
        &self.entries
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 选号：走**共用选号内核**（与 WorkBuddy 同一套算法），并支持「指定账号」。
    ///
    /// ## 与旧实现的语义对齐
    ///
    /// 旧实现是「**积分到期时间最近的优先**，同到期则积分多的优先」。共用内核的四因子
    /// 权重里没有「到期时间」这一维，因此由 [`TraePoolEntry::expiring_credits`] 把它
    /// 映射成「到期紧迫度」喂给 `expiring_weight` 项 —— 映射规则见该方法文档。
    ///
    /// ★ 这是**降级映射，不是等价替换**：紧迫度高的账号权重更大，但短名单内仍有
    /// 加权抽签的随机性，因此不再是旧实现那种严格字典序。
    ///
    /// ## 已知降级（缺数据走中性默认，见 [`crate::pool::entry_like`]）
    ///
    /// - `in_flight` 恒 0 ⇒ 在途限流不生效；
    /// - 无成本观测 ⇒ 成本分层恒为 `Unknown`（不产生过滤）。
    ///
    /// ## 指定账号
    ///
    /// 本池的排除表作用域键（`trae:<variant>`）。
    fn exclusion_scope(&self) -> String {
        format!("trae:{}", self.variant.as_str())
    }

    /// 用户显式排除的 uid 集合（管理面视图与选号共用同一份读口）。
    fn excluded_map(&self) -> std::collections::BTreeMap<String, String> {
        crate::pool::exclusions::load_for(&self.exclusion_scope())
    }

    /// `preferred_uid` 可用则优先选中；**不可用则回落到通用选号** ——
    /// 偏好是优化而非约束，绝不因为偏好账号不可用就拒绝服务。
    pub fn pick(
        &mut self,
        now: i64,
        tried: &HashSet<String>,
        preferred_uid: Option<&str>,
        // ★ issue #76：本密钥的允许名单（`None` = 全部账号）。
        //   分组已由调用方展开成 uid（Trae 的分组是 `groupId`，池不认识分组概念）。
        allowed: Option<&HashSet<String>>,
    ) -> Option<PickedTraeAccount> {
        // 共用内核按 `BTreeMap<uid, E>` 组织候选；池本身用 `Vec`（沿用账号库顺序，
        // 供展示层直接遍历），故这里临时构造一份候选表。
        // 条目只有几十个，clone 的代价可忽略，换来的是**选号算法只有一份实现**。
        let mut candidates: BTreeMap<String, TraePoolEntry> = self
            .entries
            .iter()
            .map(|entry| (entry.uid.clone(), entry.clone()))
            .collect();

        // `now` 是 Unix 秒（沿用本模块既有口径），共用内核要求毫秒 ⇒ 在此换算。
        let seed = (now.max(0) as u64) ^ self.pick_seq;
        // 用户显式排除的账号：**硬约束**，选号 / 兜底 / 偏好一律跳过。
        // scope 键按变体分家（`trae:trae_work` / `trae:trae_cn` / `trae:global` …）——
        // 两条产品线的账号库本就分家，排除表也必须分家。
        let excluded = crate::pool::exclusions::uid_set(&format!("trae:{}", self.variant.as_str()));
        // 偏好值里可能装着**策略哨兵**（`__max_credits__` / `__random__`）——
        // 那是前端同一张池卡的下拉选项，必须在这里解释成策略，否则：
        // - 当成 uid ⇒ 一个不存在的号，静默回落到加权（「按积分择优」变成假开关）；
        // - 只认 `__auto__` ⇒ 用户在 Trae 页选了随机也没人理。
        let (mode, preferred_uid) = crate::pool::parse_preferred(preferred_uid);
        let input = crate::pool::PickInput {
            realm: None,
            model: "",
            tried,
            excluded: &excluded,
            allowed,
            mode,
            seed,
        };
        let uid = pick_with_preference(
            &mut candidates,
            &mut self.pick_seq,
            &trae_pick_policy(),
            now.saturating_mul(1000),
            &input,
            preferred_uid,
        )?;

        // ★★ 安全审计 P1-5：**把选号的副作用写回池**。
        //
        // `candidates` 是临时表，`mark_selected` 改的是它的 `used_seq` / `last_used_ms`；
        // 不回写的话，连「请求内」都不保留 —— 顺序档每次都要从零重新决定「最近用过谁」。
        if let Some(selected) = candidates.get(&uid) {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.uid == uid) {
                entry.used_seq = selected.used_seq;
                entry.last_used_ms = selected.last_used_ms;
            }
        }

        self.account_for(&uid)
    }

    /// 按 **uid 直取**单个账号的出站凭据（**不选号、不写池**）。
    ///
    /// ## 与 [`Self::pick`] 的区别（限流探测的硬约束）
    ///
    /// `pick` 会做可用性择优与偏好回落 —— 偏好账号**不可用时它会换一个号**
    /// （见 `pick_with_preference`）。而限流探测的「一格」必须只打**指定**的那个 uid，
    /// 否则结果会**归因到别的账号**上。故探测走本函数：命中不到就返回 `None`
    /// （调用方据此记 `skipped`），绝不换号。
    ///
    /// **只读**：不写冷却文件、不推进 `pick_seq`、不改任何条目。
    pub fn account_for(&self, uid: &str) -> Option<PickedTraeAccount> {
        self.entries
            .iter()
            .find(|entry| entry.uid == uid)
            .map(|entry| PickedTraeAccount {
                uid: entry.uid.clone(),
                name: entry.name.clone(),
                jwt: entry.jwt.clone(),
                device_id: entry.device_id.clone(),
                machine_id: entry.machine_id.clone(),
            })
    }

    /// 记录一次上游错误：写回**本池所属变体**的冷却文件，并就地更新内存视图。
    ///
    /// `TraeErrKind::None` 不落盘（既不加冷却也不清冷却）——它是「与账号无关」的
    /// 失败，写进去只会污染状态。
    pub fn apply_error(&mut self, uid: &str, kind: TraeErrKind, reason: &str) -> bool {
        if matches!(kind, TraeErrKind::None) {
            return false;
        }
        let (error_type, cooldown_seconds) = cooldown_of(kind);
        // 关键：按**本池变体**写回冷却（`self.variant`）。若改回无变体的 `save_cooldown`，
        // 会恒定写默认产品线，导致 CN 账号的冷却落进 Work 文件、CN 池读 CN 文件读不到，
        // 死号永不被冷却且被反复选中——正是「读侧改了、写侧没改」的半成品缺陷。
        // ★ issue #108 §P0-A：写入时记下**这次判定所依据的** JWT `exp`。
        //   凭据被换掉（重新登录 / 续期 / 手动粘贴）后，这条记录会自动失效，
        //   无需那六个出口各自去清冷却。
        let credential_exp = self
            .entries
            .iter()
            .find(|entry| entry.uid == uid)
            .and_then(|entry| {
                buddy_switch_core::modules::trae::jwt::parse(&entry.jwt).exp_timestamp
            });
        credits::save_cooldown_with_credential_for(
            self.variant,
            uid,
            error_type,
            cooldown_seconds,
            reason,
            credential_exp,
        );

        let now = chrono::Local::now().timestamp();
        for entry in self.entries.iter_mut().filter(|entry| entry.uid == uid) {
            if kind.is_permanent() {
                entry.disabled = true;
                entry.cooldown_until = 9_999_999_999;
            } else if cooldown_seconds > 0 {
                entry.cooldown_until = now + cooldown_seconds;
                entry.cooldown_reason = reason.to_string();
            }
        }
        kind.is_permanent()
    }

    /// 记录一次成功：只清冷却，不动账号可用性。
    ///
    /// **不**在这里 `clear_cooldown`：一次成功不代表套餐额度已恢复（`PlanLimit` 冷却
    /// 是有意为之），真正的解冻交给 `credits::refresh_all_remaining` 的自动解冻逻辑。
    pub fn note_success(&mut self, uid: &str) {
        // 内存视图与磁盘一致：成功不清冷却，但也不留下过期的冷却时间。
        let now = chrono::Local::now().timestamp();
        for entry in self.entries.iter_mut().filter(|entry| entry.uid == uid) {
            if entry.cooldown_until != 0 && entry.cooldown_until <= now {
                entry.cooldown_until = 0;
                entry.cooldown_reason.clear();
            }
        }
    }

    /// 池状态摘要。
    /// 五态计数（形状与 WorkBuddy 侧 [`crate::pool::Pool::summary`] 同源，
    /// 两侧管理面共用同一张「账号池」卡）。
    pub fn summary(&self, now: i64) -> PoolSummary {
        let mut summary = PoolSummary {
            total: self.entries.len(),
            ..PoolSummary::default()
        };
        let excluded = self.excluded_map();
        for entry in &self.entries {
            // ★ 排除优先于一切：它决定「能不能被选中」，因此必须从 `available` 里扣掉
            //   （否则卡片说「N 可用」而实际少一个 —— 数字与事实不符）。
            if excluded.contains_key(&entry.uid) {
                summary.excluded += 1;
            } else if entry.disabled {
                summary.disabled += 1;
            } else if entry.cooldown_until > now {
                summary.cooling += 1;
            } else if entry.credits_expired(now) {
                summary.expired += 1;
            } else if entry.no_credits() {
                summary.zero_credits += 1;
            } else {
                summary.available += 1;
            }
            summary.total_credits += entry.credits.unwrap_or(0.0);
        }
        summary.total_credits = (summary.total_credits * 100.0).round() / 100.0;
        summary
    }

    /// 账号明细（camelCase，与 Trae 模块其余线上形态一致）。
    pub fn status_list(&self, now: i64) -> Vec<Value> {
        let excluded_map = self.excluded_map();
        self.entries
            .iter()
            .map(|entry| {
                let excluded = excluded_map.get(&entry.uid);
                let status = if excluded.is_some() {
                    "excluded"
                } else if entry.disabled {
                    "disabled"
                } else if entry.cooldown_until > now {
                    "cooling"
                } else if entry.credits_expired(now) {
                    "expired"
                } else if entry.no_credits() {
                    "no_credits"
                } else {
                    "available"
                };
                let cooling = entry.cooldown_until > now;
                json!({
                    "uid": entry.uid,
                    "name": entry.name,
                    "status": status,
                    "credits": entry.credits,
                    "creditsExpireAt": entry.credits_expire_at,
                    "cooling": cooling,
                    // ★ 冷却三件套只在**真的冷却 / 禁用**时给（与 WorkBuddy 侧 `Pool::status_list`
                    //   同一规则、同一张前端卡）。`cooldown_until` / `cooldown_reason` 是在
                    //   `sync_for` 里从落盘冷却文件**原样拷来**的，不做到期门控就会把一条
                    //   早已过期的冷却当成当前原因复述出去（徽标=可用、旁边写着限流原因）。
                    "cooldownUntil": if cooling && entry.cooldown_until > 0 {
                        Some(entry.cooldown_until)
                    } else {
                        None
                    },
                    // `SessionDead` 走 `disabled`（终态），其原因是**禁用原因**而非冷却原因，
                    // 因此判据是 `cooling || disabled`，不能只判 `cooling`。
                    "cooldownReason": if (cooling || entry.disabled) && !entry.cooldown_reason.is_empty() {
                        Some(entry.cooldown_reason.clone())
                    } else {
                        None
                    },
                    "disabled": entry.disabled,
                    // 用户显式排除（硬约束）。前端据此标「已排除」并禁掉「指定」。
                    "excluded": excluded.is_some(),
                    "excludedNote": excluded
                        .cloned()
                        .filter(|note| !note.is_empty()),
                    "deviceIdMasked": mask_device(&entry.device_id),
                })
            })
            .collect()
    }

    /// 诊断：每个账号为什么不能路由。用于「no healthy account」的排查。
    pub fn diagnose(&self, now: i64) -> Vec<String> {
        self.diagnose_for(now, None)
    }

    /// 逐账号诊断；`allowed` 生效时**只列这把 Key 允许用的账号**（issue #76）。
    ///
    /// 白名单用户看到的必须是「我允许用的那几个号」为什么不可用 —— 把全池
    /// 12 个号的原因堆给他，等于让他去修一堆本来就不许用的账号。
    pub fn diagnose_for(&self, now: i64, allowed: Option<&HashSet<String>>) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| match allowed {
                Some(only) => only.contains(&entry.uid),
                None => true,
            })
            .map(|entry| {
                let reason = entry.rejection(now).unwrap_or("可用");
                let credits = entry
                    .credits
                    .map(|value| format!("{value:.0}"))
                    .unwrap_or_else(|| "未知".to_string());
                let tail = entry
                    .uid
                    .get(entry.uid.len().saturating_sub(8)..)
                    .unwrap_or("");
                format!("{}({tail}:{reason},积分={credits})", entry.name)
            })
            .collect()
    }
}

/// Trae 侧的选号策略参数。
///
/// ★ `expiring_weight` 刻意**高于积分项上限（10.0）**：旧实现的语义是
/// 「**先看到期时间**，再看积分」。若紧迫度权重低于积分项，一个积分很多但一年后
/// 才到期的账号就会压过明天到期的账号 —— 等于把「先用掉快过期的额度」这条核心
/// 语义丢掉，而这恰恰是用户最在意的行为。
///
/// 其余项按「Trae 没有对应数据」置零，避免引入无判据的噪声（见模块文档的降级说明）。
fn trae_pick_policy() -> PickPolicy {
    PickPolicy {
        // Trae 没有「快过期积分的绝对值」概念 ⇒ 该项不启用，改由 `expiry_urgency` 承载。
        expiring_weight: 0.0,
        // ★ 必须**大于积分项上限（10.0）**：旧实现的语义是「先看到期时间，再看积分」。
        // 若该权重不足，一个积分多但一年后才到期的账号会压过明天到期的账号 ——
        // 等于把「先用掉快过期的额度」这条核心语义丢掉。
        expiry_urgency_weight: 50.0,
        // 无使用记录 ⇒ 闲置补偿会给所有账号同样的分，纯噪声。
        idle_weight_per_hour: 0.0,
        idle_weight_max: 0.0,
        // 无 `last_used_ms` 记录 ⇒ 防惊群无判据，0 表示不启用。
        min_pick_gap_ms: 0,
        // 无成本账本 ⇒ TTL 置 0，观测立即过期，分层恒为 Unknown。
        model_cost_ttl_ms: 0,
        // 无在途统计 ⇒ 0 表示不限。
        max_in_flight: 0,
        max_in_flight_global: 0,
        // ★ 严格择优：Trae 账号通常只有两三个，且旧实现是**确定性**的字典序择优。
        // 加权抽签会把「先用掉快过期额度」变成「约 60% 概率用掉它」，用户不可预期 ——
        // 这正是 issue #6 想解决的那类诉求。防惊群在 Trae 侧本来就无效
        // （`last_used_ms` 不跨请求保留），因此这里没有损失。
        deterministic: true,
    }
}

/// [`PoolEntryLike`] 实现：Trae 侧缺的数据一律**中性降级**。
///
/// ★ 降级的方向必须是「像没有证据」，不是「像证据指向坏结果」——
/// 否则某个账号会**静默地永远选不上**。具体取舍见 [`crate::pool::entry_like`] 的契约。
impl PoolEntryLike for TraePoolEntry {
    fn uid(&self) -> &str {
        &self.uid
    }

    fn credits(&self) -> i64 {
        // 共用内核用 i64；Trae 的积分是 f64（如 3618.69）。取整不影响相对大小，
        // 而相对大小才是权重唯一用到的信息。
        self.credits.unwrap_or(0.0) as i64
    }

    /// Trae 没有「快过期积分的绝对值」概念（只有单一到期时刻）⇒ `0`。
    ///
    /// 「还有多久到期」由 [`TraePoolEntry::expiry_urgency`] 表达 —— 两者分工见
    /// [`crate::pool::entry_like`] 的契约说明。
    fn expiring_credits(&self, _now_ms: i64) -> i64 {
        0
    }

    /// 到期紧迫度：**把旧实现的「积分到期最近优先」语义搬到这里**。
    ///
    /// ## 为什么用对数映射
    ///
    /// 线性映射（`1 - remaining/HORIZON`）无法区分「1 分钟后到期」与「1 天后到期」——
    /// 在一年尺度下两者的相对差小到可以忽略，紧迫度差不足 0.004，乘以任何合理权重
    /// 都压不过积分项（上限 10.0），旧语义照样丢失。
    ///
    /// 因此按「距到期的**数量级**」取对数：`1 分钟 → ≈1.0`、`1 天 → ≈0.56`、
    /// `1 年 → ≈0.0`。配合 `expiry_urgency_weight = 50.0`，「一分钟 vs 一天」的
    /// 权重差约 20 分，稳稳压过积分差异。
    ///
    /// 无到期时间返回 `0`（对应旧语义「无到期时间的排在有到期时间的之后」）。
    fn expiry_urgency(&self, now_ms: i64) -> f64 {
        /// 对数映射的下界：低于此值一律按「立刻到期」处理。
        const FLOOR_MS: f64 = 60_000.0;
        /// 对数映射的上界（一年）。
        const HORIZON_MS: f64 = 365.0 * 86_400_000.0;

        let credits = self.credits.unwrap_or(0.0);
        let Some(expire_secs) = self.credits_expire_at else {
            return 0.0;
        };
        if credits <= 0.0 || expire_secs <= 0 {
            return 0.0;
        }
        // 字段单位是 Unix 秒，契约为毫秒 —— 在这里换算，不外泄到算法。
        let expire_ms = expire_secs.saturating_mul(1000);
        if expire_ms <= now_ms {
            // 已过期由 `healthy_for_request` 拒绝，这里给 0 即可。
            return 0.0;
        }
        let t = ((expire_ms - now_ms) as f64).max(FLOOR_MS);
        let span = HORIZON_MS.ln() - FLOOR_MS.ln();
        (1.0 - (t.ln() - FLOOR_MS.ln()) / span).clamp(0.0, 1.0)
    }

    /// Trae 侧没有成功率统计 ⇒ 返回中性 `0.0`（内核据此给中性权重 1.5）。
    fn success_ema(&self) -> f64 {
        0.0
    }

    /// 同上：**不能**返回 1.0，那会让账号被当成「一直在失败」而永久压制。
    fn error_ema(&self) -> f64 {
        0.0
    }

    fn last_used_ms(&self) -> i64 {
        self.last_used_ms
    }

    fn used_seq(&self) -> u64 {
        self.used_seq
    }

    /// Trae 侧无在途统计 ⇒ `0`（不触发在途限流）。
    fn in_flight(&self) -> i64 {
        0
    }

    /// Trae 池本身已按产品线变体分家，池内不再按区域过滤。
    fn realm(&self) -> Option<crate::pool::RealmTag> {
        None
    }

    fn is_disabled(&self) -> bool {
        self.disabled
    }

    fn cool_expiry_ms(&self) -> i64 {
        // 字段单位是 Unix 秒，契约为毫秒。
        if self.cooldown_until > 0 {
            self.cooldown_until.saturating_mul(1000)
        } else {
            0
        }
    }

    fn is_hard_cooled(&self, now_ms: i64) -> bool {
        self.cool_kind == Some(CoolKind::Hard) && now_ms < self.cool_expiry_ms()
    }

    fn healthy_for_request(&self, now_ms: i64, _model: &str) -> bool {
        // Trae 没有模型级冷却，模型维度忽略。
        // 账号级判据复用既有的 `rejection` —— 它是「为什么不可路由」的**唯一来源**，
        // 展示层（诊断/状态列表）与选号必须共用同一套条件，否则会出现
        // 「界面说可用、选号却跳过」这类不报错的不一致。
        self.rejection(now_ms / 1000).is_none()
    }

    /// Trae 无模型级冷却 ⇒ 空操作。
    fn prune_model_cooldowns(&mut self, _now_ms: i64) {}

    /// Trae 无实测成本账本 ⇒ `0.0`（分层恒为 `Unknown`）。
    fn cost_per_1k(&self, _now_ms: i64, _model: &str, _ttl_ms: i64) -> f64 {
        0.0
    }

    /// Trae 无实测成本账本 ⇒ `Unknown`（保留「值得一试」的语义，不产生过滤）。
    fn cost_tier(&self, _now_ms: i64, _model: &str, _ttl_ms: i64) -> crate::pool::entry::CostTier {
        crate::pool::entry::CostTier::Unknown
    }

    fn mark_picked(&mut self, now_ms: i64, seq: u64) {
        self.last_used_ms = now_ms;
        self.used_seq = seq;
    }

    fn mark_probed(&mut self, now_ms: i64) {
        self.last_used_ms = now_ms;
    }
}

/// 剥掉 `Cloud-IDE-JWT ` 前缀并去掉首尾空白。
///
/// 账号库里两种形态都可能存在（OAuth 登录存裸 token，手工粘贴常带前缀），
/// 上游只接受裸 token，因此这一步必须在出站前统一。
pub fn clean_jwt(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("Cloud-IDE-JWT ")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

/// 设备标识脱敏：只留首尾各 4 位。
fn mask_device(device_id: &str) -> String {
    if device_id.len() <= 8 {
        return device_id.to_string();
    }
    format!("{}…{}", &device_id[..4], &device_id[device_id.len() - 4..])
}

/// 错误类别 → `(落盘类型名, 冷却秒数)`。
///
/// 秒数沿用 Trae 账号模块的经验值：`-1` 表示永久。
///
/// ★ issue #108：`SessionDead` 仍然永久（**真**会话作废，短冷却只会反复撞墙）；
/// 但 401/403 已经改由 [`TraeErrKind::AuthRejected`] 承担，浅冷却 60 秒、不禁用 ——
/// 与 WorkBuddy 侧 `AUTH_REJECT_COOLDOWN_MS` 对齐。改造前那段「401 是凭据失效，
/// 短冷却只会让它被反复重试」的注释针对的是**裸 401 被当成终态**这个前提，
/// 该前提现在由「先识别 12153」取代，注释一并改写以免与新语义冲突。
fn cooldown_of(kind: TraeErrKind) -> (&'static str, i64) {
    match kind {
        TraeErrKind::None => ("None", 0),
        TraeErrKind::PlanLimit => ("PlanLimit", 43_200),
        TraeErrKind::SoftRate => ("SoftRate", 60),
        TraeErrKind::AuthRejected => ("AuthRejected", 60),
        TraeErrKind::SessionDead => ("SessionDead", -1),
        TraeErrKind::NotFound => ("NotFound", 60),
        TraeErrKind::Server => ("Server", 600),
        TraeErrKind::Client => ("Client", 600),
        TraeErrKind::Business => ("BusinessError", 300),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uid: &str, credits: Option<f64>, expire_at: Option<i64>) -> TraePoolEntry {
        TraePoolEntry {
            uid: uid.to_string(),
            name: uid.to_string(),
            jwt: format!("jwt-{uid}"),
            device_id: "000000000000001".to_string(),
            machine_id: "0".repeat(64),
            credits,
            credits_expire_at: expire_at,
            disabled: false,
            cooldown_until: 0,
            cooldown_reason: String::new(),
            cool_kind: None,
            last_used_ms: 0,
            used_seq: 0,
        }
    }

    fn pool_with(entries: Vec<TraePoolEntry>) -> TraePool {
        let mut pool = TraePool::for_variant(TraeVariant::default());
        pool.entries = entries;
        pool
    }

    /// ★★ 安全审计 P1-5：`sync_for` 重建条目时**必须继承**选号序号。
    ///
    /// 这是顺序档真正失效的**主要原因**：`pick` 的写回只活到下一次 `sync_for`，
    /// 而 Trae 每个请求都会 sync 一次 ⇒ 不继承就等于每次请求都从零开始。
    ///
    /// 变异验证：删掉 `sync_for` 里的 `inherit_order(...)` 调用（或让它返回 0）⇒ 红。
    #[test]
    fn sync_inherits_the_order_state_and_realigns_pick_seq() {
        let mut entries = vec![
            entry("uid-a", Some(1.0), None),
            entry("uid-b", Some(1.0), None),
            entry("uid-c", Some(1.0), None),
        ];
        let mut previous = std::collections::HashMap::new();
        previous.insert("uid-a".to_string(), (7_u64, 1_234_i64));
        previous.insert("uid-b".to_string(), (3_u64, 999_i64));

        let seq = inherit_order(&mut entries, &previous);
        assert_eq!(entries[0].used_seq, 7, "历史序号必须继承");
        assert_eq!(entries[0].last_used_ms, 1_234);
        assert_eq!(entries[1].used_seq, 3);
        assert_eq!(entries[2].used_seq, 0, "没有历史记录的号保持 0");
        assert_eq!(
            seq, 7,
            "`pick_seq` 必须追上历史最大值，否则新选中会与历史并列"
        );

        // 反向对照：历史里有、账号库里已没有的 uid 不该把 `pick_seq` 抬到 99
        //（那会让序号凭空跳一大截，虽然无害但没有依据）。
        previous.insert("uid-deleted".to_string(), (99_u64, 0));
        let seq = inherit_order(&mut entries, &previous);
        assert_eq!(seq, 7, "{seq}");
        assert!(
            !entries.iter().any(|entry| entry.uid == "uid-deleted"),
            "已删除的账号不该被「复活」进池里"
        );
    }

    /// ★★ 安全审计 P1-5：「顺序」档在 Trae 上必须**真的粘住**最近用过的号。
    ///
    /// 改造前有两个原因让它名不副实：
    /// ① [`TraePool::sync_for`] 每请求重建 `entries`，`used_seq` 一律归零；
    /// ② [`TraePool::pick`] 把 entries clone 进临时表，`mark_selected` 的写回随函数
    ///    返回丢弃 —— 连**请求内**都不保留。
    ///
    /// 于是这一档退化成「永远选 uid 最小的号」，而 Trae 页面下拉里摆着它（issue #102）。
    ///
    /// 变异验证：去掉 `pick` 里的回写 ⇒ 第二条断言红；去掉 `sync_for` 里的继承 ⇒
    /// 第三条断言红。
    #[test]
    fn sequential_sticks_to_the_last_used_account_across_picks() {
        let mut pool = pool_with(vec![
            entry("uid-a", Some(10.0), None),
            entry("uid-b", Some(10.0), None),
        ]);
        let tried = HashSet::new();
        let now = 1_700_000_000_i64;

        // ① 第一次：冷启动按 uid 序取最小。
        let first = pool
            .pick(now, &tried, Some("__sequential__"), None)
            .expect("必须能选出账号")
            .uid;
        assert_eq!(first, "uid-a");

        // ② 副作用必须写回池（改造前写进了临时表就丢了）。
        let used = pool
            .entries()
            .iter()
            .find(|entry| entry.uid == first)
            .map(|entry| entry.used_seq)
            .unwrap_or(0);
        assert!(
            used > 0,
            "选中后 `used_seq` 必须大于 0，否则「最近用过谁」这个信息根本不存在"
        );

        // ③ 第二次：顺序档必须粘住**刚刚用过的那个**，而不是又回到 uid 序的第一个。
        let second = pool
            .pick(now + 60, &tried, Some("__sequential__"), None)
            .expect("必须能选出账号")
            .uid;
        assert_eq!(
            second, first,
            "顺序档的语义是「粘住最近使用的账号」，第二次必须还是它"
        );

        // ④ 让 a 冷却后，必须换到 b（证明「粘住」不是「永远只选它」）。
        if let Some(entry) = pool.entries.iter_mut().find(|entry| entry.uid == "uid-a") {
            entry.cooldown_until = now + 3_600;
        }
        let third = pool
            .pick(now + 120, &tried, Some("__sequential__"), None)
            .expect("a 冷却后必须能换到 b")
            .uid;
        assert_eq!(third, "uid-b");
    }

    #[test]
    fn clean_jwt_strips_prefix_and_whitespace() {
        assert_eq!(clean_jwt("Cloud-IDE-JWT abc"), "abc");
        assert_eq!(clean_jwt("  Cloud-IDE-JWT  abc  "), "abc");
        assert_eq!(clean_jwt("abc"), "abc");
        assert_eq!(clean_jwt("   "), "");
    }

    /// ★★ issue #108 §P0-B：**先识别 `12153`，再降级裸 401**。
    ///
    /// 两条断言必须同批存在，缺一条都会把修复做反：
    /// - 裸 401/403 不得判成永久失效（否则「token 过期」被打成终态，重登也救不回）；
    /// - 带作废标记的 401 **必须**判成永久失效（否则真作废会被当可自愈，反复撞墙）。
    #[test]
    fn classify_http_separates_session_dead_from_plain_401() {
        // ① 裸 401/403 ⇒ 浅冷却档，`is_permanent()` 必须为假。
        assert_eq!(classify_http(401, ""), TraeErrKind::AuthRejected);
        assert_eq!(classify_http(403, ""), TraeErrKind::AuthRejected);
        assert!(
            !classify_http(401, "").is_permanent(),
            "裸 401 不得是终态：access token 过期可自愈"
        );
        // ② 带作废标记 ⇒ 永久档，与状态码无关（上游也可能用 200/400 返回）。
        for body in [
            r#"{"code":12153,"message":"Offline user session not found"}"#,
            r#"{"code":0,"message":"12153"}"#,
        ] {
            assert_eq!(
                classify_http(401, body),
                TraeErrKind::SessionDead,
                "带作废标记必须判永久：{body}"
            );
            assert!(classify_http(400, body).is_permanent(), "{body}");
        }
        // ③ 其余家族不变。
        assert_eq!(classify_http(429, ""), TraeErrKind::SoftRate);
        assert_eq!(classify_http(404, ""), TraeErrKind::NotFound);
        assert_eq!(classify_http(500, ""), TraeErrKind::Server);
        assert_eq!(classify_http(503, ""), TraeErrKind::Server);
        assert_eq!(classify_http(400, ""), TraeErrKind::Client);
        assert_eq!(classify_http(200, ""), TraeErrKind::None);

        // ④ 冷却时长：浅档 60s、永久档 -1（`-1` 是落盘的「永久」语义）。
        assert_eq!(cooldown_of(TraeErrKind::AuthRejected), ("AuthRejected", 60));
        assert_eq!(cooldown_of(TraeErrKind::SessionDead), ("SessionDead", -1));
    }

    #[test]
    fn classify_solo_prefers_business_codes_over_ranges() {
        // 1005 必须归为套餐限额，而不是被当成 Client。
        assert_eq!(classify_solo(1005, ""), TraeErrKind::PlanLimit);
        // 4001 是模型配置问题，不该冷却账号。
        assert_eq!(classify_solo(4001, ""), TraeErrKind::None);
        assert_eq!(classify_solo(0, "model config is empty"), TraeErrKind::None);
        // 4008 是限流，短冷却。
        assert_eq!(classify_solo(4008, ""), TraeErrKind::SoftRate);
        assert_eq!(classify_solo(0, "Quota exceeded"), TraeErrKind::SoftRate);
        assert_eq!(classify_solo(0, "rate limit"), TraeErrKind::SoftRate);
        // 裸 401 走浅冷却档；带作废标记才是永久档（issue #108）。
        assert_eq!(classify_solo(401, ""), TraeErrKind::AuthRejected);
        assert_eq!(
            classify_solo(0, "Offline user session not found"),
            TraeErrKind::SessionDead
        );
        assert_eq!(classify_solo(500, ""), TraeErrKind::Server);
        assert_eq!(classify_solo(400, ""), TraeErrKind::Client);
        assert_eq!(classify_solo(0, ""), TraeErrKind::None);
        assert_eq!(classify_solo(1234, ""), TraeErrKind::Business);
        // 区间边界：只有 4xx/5xx 才是 Client / Server，600 起是业务码。
        assert_eq!(classify_solo(499, ""), TraeErrKind::Client);
        assert_eq!(classify_solo(500, ""), TraeErrKind::Server);
        assert_eq!(classify_solo(599, ""), TraeErrKind::Server);
        assert_eq!(classify_solo(600, ""), TraeErrKind::Business);
        // 业务码即使落在 4xx/5xx 之外的高位，也不能被吞成 Server。
        assert_eq!(
            classify_solo(1005, "plan limit exceeded"),
            TraeErrKind::PlanLimit
        );
    }

    #[test]
    fn rejection_prefers_disabled_then_cooldown_then_credits() {
        let now = 1_000_000;
        let mut item = entry("u", Some(10.0), Some(now + 100));
        assert!(item.rejection(now).is_none());

        item.credits = Some(0.0);
        assert_eq!(item.rejection(now), Some("零积分"));

        item.credits = Some(10.0);
        item.credits_expire_at = Some(now - 1);
        assert_eq!(item.rejection(now), Some("积分已过期"));

        item.credits_expire_at = Some(now + 100);
        item.cooldown_until = now + 60;
        assert_eq!(item.rejection(now), Some("冷却中"));

        item.disabled = true;
        assert_eq!(item.rejection(now), Some("会话失效（需重新登录）"));
    }

    #[test]
    fn missing_credits_and_missing_expiry_are_not_rejections() {
        // 未知积分 / 无到期时间都不应让账号不可用——否则新账号永远选不上。
        let now = 1_000_000;
        let item = entry("u", None, None);
        assert!(item.rejection(now).is_none());
        assert!(!item.no_credits());
        assert!(!item.credits_expired(now));
    }

    #[test]
    fn pick_skips_tried_and_unhealthy_and_prefers_soonest_expiry() {
        let now = 1_000_000;
        let mut soon = entry("soon", Some(10.0), Some(now + 100));
        soon.name = "soon".to_string();
        let later = entry("later", Some(999.0), Some(now + 100_000));
        let mut cooling = entry("cooling", Some(999.0), Some(now + 10));
        cooling.cooldown_until = now + 500;
        let zero = entry("zero", Some(0.0), Some(now + 10));

        let mut pool = pool_with(vec![soon, later, cooling, zero]);
        let picked = pool
            .pick(now, &HashSet::new(), None, None)
            .expect("应选出账号");
        assert_eq!(picked.uid, "soon", "到期最近的优先");

        // 已试过的账号不再选。
        let mut tried = HashSet::new();
        tried.insert("soon".to_string());
        assert_eq!(pool.pick(now, &tried, None, None).unwrap().uid, "later");

        // 全试过 → 不重复尝试已试账号，但会走**全冷却兜底**挑「最早解冻者」。
        // ★ 这是接入共用内核后新增的能力（旧实现此时直接返回 None）。
        // `cooling` 是唯一「有冷却时间且未被试过」的账号；`zero` 零积分且无冷却时间
        // （`cool_expiry_ms() == 0`），不参与兜底。
        tried.insert("later".to_string());
        let fallback = pool
            .pick(now, &tried, None, None)
            .expect("全冷却兜底应挑出最早解冻者");
        assert_eq!(fallback.uid, "cooling");
    }

    #[test]
    fn pick_prefers_known_expiry_over_unknown() {
        let now = 1_000_000;
        let unknown = entry("unknown", Some(5000.0), None);
        let known = entry("known", Some(1.0), Some(now + 999));
        let mut pool = pool_with(vec![unknown, known]);
        assert_eq!(
            pool.pick(now, &HashSet::new(), None, None).unwrap().uid,
            "known"
        );
    }

    /// ★★ 护栏：旧实现的「**积分到期最近优先**」语义必须在新算法下继续成立。
    ///
    /// 这条专治最危险的一类回归：把「到期时间」硬塞进 `expiring_credits`（绝对值）
    /// 会被 `credits` 归一化抹平 —— 积分多但远未到期的账号会压过积分少但即将到期的
    /// 账号，**而且不报错**。本用例让「即将到期」的账号积分远少于对手，只有独立因子
    /// 足够强才能选中它。
    #[test]
    fn soonest_expiry_wins_even_with_far_fewer_credits() {
        let now = 1_000_000;
        // 一分钟后到期，但积分只有 10。
        let soon = entry("soon", Some(10.0), Some(now + 60));
        // 一年后到期，积分 999（是前者的近百倍）。
        let far = entry("far", Some(999.0), Some(now + 365 * 24 * 3600));
        let mut pool = pool_with(vec![far, soon]);

        let picked = pool
            .pick(now, &HashSet::new(), None, None)
            .expect("应选出账号");
        assert_eq!(picked.uid, "soon", "「先用掉快过期的额度」必须压过积分多寡");
    }

    /// ★ 策略哨兵：`__max_credits__` 必须真的按积分择优（而不是被当成不存在的 uid 回落加权）。
    ///
    /// 变异：把 `parse_preferred` 的调用去掉（偏好值原样当 uid）⇒ 本用例红
    /// —— 那正是「假开关」：UI 上选了「按积分择优」，实际仍在加权抽签。
    #[test]
    fn max_credits_sentinel_actually_picks_the_richest() {
        let now = 1_000_000;
        let rich = entry("rich", Some(9999.0), Some(now + 3600));
        let poor = entry("poor", Some(1.0), Some(now + 3600));
        let mut pool = pool_with(vec![rich, poor]);

        let picked = pool
            .pick(now, &HashSet::new(), Some("__max_credits__"), None)
            .expect("应选出账号");
        assert_eq!(picked.uid, "rich", "哨兵值必须按积分择优");
    }

    /// 指定账号可用时必须被优先选中 —— 即使它的积分远少于别人。
    #[test]
    fn preferred_uid_wins_when_usable() {
        let now = 1_000_000;
        let rich = entry("rich", Some(9999.0), Some(now + 60));
        let poor = entry("poor", Some(1.0), Some(now + 60));
        let mut pool = pool_with(vec![rich, poor]);

        let picked = pool
            .pick(now, &HashSet::new(), Some("poor"), None)
            .expect("应选出账号");
        assert_eq!(picked.uid, "poor", "指定账号可用时必须优先");
    }

    /// ★★ 护栏：指定账号**不可用**时必须回落到自动择优，而不是返回 None。
    ///
    /// 「偏好是优化而非约束」—— 若偏好不可用就拒绝服务，一次临时冷却会让整个网关
    /// 停止工作。三种不可用形态（冷却中 / 禁用 / 已在本轮试过）都要覆盖。
    #[test]
    fn preferred_uid_falls_back_when_unusable() {
        let now = 1_000_000;
        let mut cooling = entry("cooling", Some(9999.0), Some(now + 60));
        cooling.cooldown_until = now + 500;
        let mut disabled = entry("disabled", Some(9999.0), Some(now + 60));
        disabled.disabled = true;
        let healthy = entry("healthy", Some(1.0), Some(now + 60));
        let mut pool = pool_with(vec![cooling, disabled, healthy]);

        // 冷却中的偏好 → 回落
        assert_eq!(
            pool.pick(now, &HashSet::new(), Some("cooling"), None)
                .unwrap()
                .uid,
            "healthy"
        );
        // 禁用的偏好 → 回落
        assert_eq!(
            pool.pick(now, &HashSet::new(), Some("disabled"), None)
                .unwrap()
                .uid,
            "healthy"
        );
        // 本轮已试过的偏好 → 回落（换号重试不得钉死在同一个账号上）
        let mut tried = HashSet::new();
        tried.insert("healthy".to_string());
        let picked = pool
            .pick(now, &tried, Some("healthy"), None)
            .expect("应回落到其它账号");
        assert_ne!(picked.uid, "healthy", "已试过的偏好必须被跳过");
        // 不存在的 uid → 回落
        assert_eq!(
            pool.pick(now, &HashSet::new(), Some("no-such-uid"), None)
                .unwrap()
                .uid,
            "healthy"
        );
    }

    /// ★ 硬冷却（套餐额度用尽）不得被「全冷却兜底」强试 —— 强试必然再失败，
    /// 只会白白消耗一次上游调用并加重风控。
    #[test]
    fn plan_limited_account_is_not_probed_by_fallback() {
        let now = 1_000_000;
        let mut limited = entry("limited", Some(9999.0), Some(now + 60));
        limited.cooldown_until = now + 43_200;
        limited.cool_kind = Some(CoolKind::Hard);
        let mut pool = pool_with(vec![limited]);

        assert!(
            pool.pick(now, &HashSet::new(), None, None).is_none(),
            "硬冷却账号既不在候选里，也不该被兜底强试"
        );
    }

    /// 到期紧迫度必须随剩余时间**单调递减**（剩余越短 → 紧迫度越高）。
    #[test]
    fn expiry_urgency_is_monotonic_and_bounded() {
        let now = 1_000_000;
        let now_ms = now * 1000;

        let urgency_of = |seconds_ahead: i64| {
            entry("u", Some(10.0), Some(now + seconds_ahead)).expiry_urgency(now_ms)
        };

        let one_minute = urgency_of(60);
        let one_hour = urgency_of(3600);
        let one_day = urgency_of(86_400);
        let one_year = urgency_of(365 * 86_400);

        assert!(one_minute > one_hour, "{one_minute} 应大于 {one_hour}");
        assert!(one_hour > one_day, "{one_hour} 应大于 {one_day}");
        assert!(one_day > one_year, "{one_day} 应大于 {one_year}");
        for value in [one_minute, one_hour, one_day, one_year] {
            assert!(
                (0.0..=1.0).contains(&value),
                "紧迫度必须落在 [0,1]：{value}"
            );
        }
        // 无到期时间 / 已过期 → 0
        assert_eq!(entry("u", Some(10.0), None).expiry_urgency(now_ms), 0.0);
        assert_eq!(
            entry("u", Some(10.0), Some(now - 1)).expiry_urgency(now_ms),
            0.0
        );
    }

    #[test]
    fn summary_counts_every_bucket_once() {
        let now = 1_000_000;
        let available = entry("a", Some(10.0), Some(now + 100));
        let mut cooling = entry("b", Some(5.0), Some(now + 100));
        cooling.cooldown_until = now + 60;
        let mut disabled = entry("c", Some(5.0), Some(now + 100));
        disabled.disabled = true;
        let expired = entry("d", Some(5.0), Some(now - 1));
        let zero = entry("e", Some(0.0), Some(now + 100));

        let pool = pool_with(vec![available, cooling, disabled, expired, zero]);
        let summary = pool.summary(now);
        assert_eq!(summary.total, 5);
        assert_eq!(summary.available, 1);
        assert_eq!(summary.cooling, 1);
        assert_eq!(summary.disabled, 1);
        assert_eq!(summary.expired, 1);
        assert_eq!(summary.zero_credits, 1);
        assert_eq!(summary.total_credits, 25.0);
    }

    #[test]
    fn status_list_uses_camel_case_and_masks_device() {
        let now = 1_000_000;
        let mut item = entry("u1", Some(12.5), Some(now + 100));
        item.device_id = "123456789012345".to_string();
        let pool = pool_with(vec![item]);
        let list = pool.status_list(now);
        let object = list[0].as_object().unwrap();
        assert_eq!(object["status"], "available");
        assert_eq!(object["credits"], 12.5);
        assert_eq!(object["deviceIdMasked"], "1234…2345");
        // 不得出现 snake_case 键。
        assert!(!object.contains_key("credits_expire_at"));
        assert!(!object.contains_key("cooldown_until"));
    }

    /// ★ 冷却**自然到期**后不得再复述原因；**禁用**账号的原因必须保留。
    ///
    /// 回归对象：`cooldown_until` / `cooldown_reason` 是 `sync_for` 从落盘冷却文件
    /// **原样拷来**的，不做到期门控就会把一条早已过期的冷却当成当前原因发出去 ——
    /// 与 WorkBuddy 侧 `Pool::status_list` 同一处缺陷、同一张前端卡。
    #[test]
    fn status_list_drops_expired_cooldown_but_keeps_disable_reason() {
        let now = 1_000_000;

        // ① 冷却中 ⇒ 给原因与解冻时刻（阳性对照：证明下面两条不是「永远绿」）。
        let mut cooling = entry("u-cool", Some(50.0), None);
        cooling.cooldown_until = now + 60_000;
        cooling.cooldown_reason = "429 rate limit".to_string();
        let list = pool_with(vec![cooling]).status_list(now);
        assert_eq!(list[0]["status"], "cooling");
        assert_eq!(list[0]["cooldownReason"], "429 rate limit");
        assert_eq!(list[0]["cooldownUntil"], now + 60_000);

        // ② 已到期 ⇒ 两个字段都不得外发。
        let mut expired = entry("u-expired", Some(50.0), None);
        expired.cooldown_until = now - 1;
        expired.cooldown_reason = "429 rate limit".to_string();
        let list = pool_with(vec![expired]).status_list(now);
        assert_eq!(list[0]["status"], "available");
        assert!(list[0]["cooldownUntil"].is_null(), "到期后不得再给解冻时刻");
        assert!(
            list[0]["cooldownReason"].is_null(),
            "到期后不得复述陈旧原因"
        );

        // ③ 禁用（`SessionDead` 是终态）⇒ 原因是**禁用原因**，必须保留。
        let mut dead = entry("u-dead", Some(50.0), None);
        dead.disabled = true;
        dead.cooldown_reason = "会话失效（需重新登录）".to_string();
        let list = pool_with(vec![dead]).status_list(now);
        assert_eq!(list[0]["status"], "disabled");
        assert_eq!(
            list[0]["cooldownReason"], "会话失效（需重新登录）",
            "禁用原因不得被门控藏掉"
        );
        assert!(
            list[0]["cooldownUntil"].is_null(),
            "禁用是终态，没有「解冻时刻」可给"
        );
    }

    #[test]
    fn diagnose_reports_reason_for_every_entry() {
        let now = 1_000_000;
        let mut cooling = entry("u1", Some(9.0), Some(now + 100));
        cooling.cooldown_until = now + 60;
        let pool = pool_with(vec![cooling]);
        let lines = pool.diagnose(now);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("冷却中"), "{}", lines[0]);
    }

    /// issue #76：白名单生效时诊断只列**这把 Key 允许用**的账号。
    ///
    /// 阳性对照：去掉 `diagnose_for` 里的 uid 过滤（退化成 `diagnose`）⇒
    /// 第一条断言必红 —— 那正是「白名单用户被告知池里 3 个号都不可用」的来源。
    #[test]
    fn diagnose_for_filters_to_allowlisted_accounts() {
        let now = 1_000_000;
        let mut cooling = entry("u1", Some(9.0), Some(now + 100));
        cooling.cooldown_until = now + 60;
        let disabled = {
            let mut e = entry("u2", Some(9.0), Some(now + 100));
            e.disabled = true;
            e
        };
        let ok = entry("u3", Some(9.0), Some(now + 100));
        let pool = pool_with(vec![cooling, disabled, ok]);

        assert_eq!(
            pool.diagnose(now).len(),
            3,
            "全池口径不变（管理面要看全池）"
        );

        let only: HashSet<String> = ["u2".to_string()].into_iter().collect();
        let lines = pool.diagnose_for(now, Some(&only));
        assert_eq!(lines.len(), 1, "名单外账号不得出现在诊断里");
        assert!(
            lines[0].contains("u2") || lines[0].contains("禁用"),
            "{}",
            lines[0]
        );

        // 名单里的 uid 不在池中 ⇒ 空诊断（上层据此说「名单没覆盖本区域」）。
        let missing: HashSet<String> = ["zzz".to_string()].into_iter().collect();
        assert!(pool.diagnose_for(now, Some(&missing)).is_empty());
    }

    #[test]
    fn cooldown_of_maps_every_kind_to_a_documented_pair() {
        assert_eq!(cooldown_of(TraeErrKind::None), ("None", 0));
        assert_eq!(cooldown_of(TraeErrKind::SessionDead), ("SessionDead", -1));
        assert_eq!(cooldown_of(TraeErrKind::PlanLimit).1, 43_200);
        assert_eq!(cooldown_of(TraeErrKind::SoftRate).1, 60);
        // 永久禁用只属于会话失效。
        for kind in [
            TraeErrKind::PlanLimit,
            TraeErrKind::SoftRate,
            TraeErrKind::NotFound,
            TraeErrKind::Server,
            TraeErrKind::Client,
            TraeErrKind::Business,
            TraeErrKind::None,
        ] {
            assert!(!kind.is_permanent(), "{kind:?} 不该是永久");
        }
        assert!(TraeErrKind::SessionDead.is_permanent());
    }
}
