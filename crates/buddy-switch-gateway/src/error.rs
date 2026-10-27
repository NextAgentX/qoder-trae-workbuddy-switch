//! 网关错误类型与「上游类别 → HTTP 状态码 / 协议错误体」映射（P0-7）。

use serde_json::{json, Value};

use buddy_switch_core::modules::product::Scope;
use buddy_switch_core::modules::region::{self, Region, RegionMismatch};
use buddy_switch_core::modules::upstream::UpstreamErrorKind;

/// `pool_busy`（503）建议客户端等多久再试 —— 会写进 `Retry-After` 响应头。
///
/// 取值依据：在途额度在**当前那批请求结束**时归还（`InFlightLease` 的 `Drop`），
/// 本机量级是亚秒到几秒；2s 与 relay 排队等待的默认量级一致，足以避开
/// 「立刻重试 → 把刚空出来的额度又打满」的抖动。
const POOL_BUSY_RETRY_AFTER_SECS: u64 = 2;

/// 网关统一错误。
#[derive(Debug, Clone)]
pub enum GatewayError {
    /// 未携带/无效/已吊销 Key → 401
    Unauthorized(String),
    /// 该 region 无可用凭据 → 401。
    ///
    /// ★ `cause` **只影响文案，不影响** [`Self::type_name`]（四类共用 `no_credential`）
    /// —— 客户端按类别名分流，改它等于改对外契约。
    NoCredential {
        /// 归属域。
        region: Region,
        /// 具体原因。四类状态的**可行动指引完全不同**，见 [`NoCredentialCause`]。
        cause: NoCredentialCause,
    },
    /// 选中的账号只有加密信封凭据 → 401（可读指引；见 `account::envelope_token_error`）
    EncryptedCredential { region: Region },
    /// 凭据 region 不符 → 401（安全红线 F，含修复指引）
    RegionMismatch(RegionMismatch),
    /// 上游失败 → 按 kind 映射
    Upstream {
        kind: UpstreamErrorKind,
        status: u16,
        message: String,
        /// 本轮**尝试过的账号明细**（换号重试的轨迹）。
        ///
        /// ## 为什么必须给出来（2026-10-17）
        ///
        /// 此前失败只回**最后一条**上游错误，用户无法回答「网关到底换号了没有、换到了谁、
        /// 为什么都不行」—— 排查只能靠猜。有了它，一次失败就能看出：
        /// 「A 429（模型限流）→ B 在途满 → C 已排除」。
        ///
        /// 空表 = 没有任何一次真正打到上游（例如选号前就被模型闸门挡住）。
        attempts: Vec<UpstreamAttempt>,
    },
    /// 请求参数不合法 → 400
    BadRequest(String),
    /// 模型被用户禁用 → **403**（明确拒绝，不是笼统 4xx 里的 400；P11 §3.18.1）。
    ///
    /// 与 `/v1/models` 的过滤**同源**（读同一份 `disabled` 存储 + 同一个 [`crate::disabled_models::is_disabled`]）——
    /// 否则禁用就是「假开关」。
    ModelDisabled { scope: Scope, model: String },
    /// 账号都健康，但在途额度已占满 → **503**（凭据、余额、冷却都没问题）。
    ///
    /// ★ 为什么不回 401：`no_credential` 会让客户端（与人）去重新登录 / 切换账号，
    ///   而在途占满**几秒内自己就恢复了**，那条指引是纯误导。并发饱和是「稍后重试」，
    ///   不是「你没登录」。
    ///
    /// 只在「可用账号全是『在途已满』」时给出；混着禁用/冷却时仍归 `no_credential`，
    /// 但计数里会列出在途已满的个数（否则「N 个号、0 禁用、0 冷却、0 零余额」与
    /// 「0 个可用」自相矛盾 —— 与 `excluded` 同一个坑）。
    PoolBusy {
        /// 归属域。
        region: Region,
        /// 池内该域的账号总数（含被排除的：它只影响「能不能选」，不影响「号是好的」）。
        total: usize,
    },
    /// **模型闸门暂停中** → **503** + `Retry-After`（issue #106）。
    ///
    /// ## 为什么必须与 `no_credential` 分开
    ///
    /// 改造前闸门短路走的是 `pool_unavailable_error` 的兜底分支：`last == None`
    /// ⇒ 只有 `PoolBusy`（503）或 `NoCredential`（401）两种出口，而 `PoolBusy` 的判据
    /// （可用号全在途满）在这种场景不成立 ⇒ 一律落到 **401 `no_credential`**。
    ///
    /// 后果是致命的：客户端把 401/403 固定归为 `AUTH`、**不可重试**、并渲染成
    /// 「API 密钥无效」（issue #106 的实测证据：闸门短路那次耗时 19ms、账号列为空）。
    /// 于是一个**临时性的容量问题**被翻译成「你的凭据坏了」，整轮会话直接终止。
    ///
    /// ⇒ 语义上它是「上游此刻对这个模型整体限流」，属于**可重试的临时不可用**，
    /// 必须是 503（客户端按 SERVER 重试），并带上**本进程自己算出来的**确定剩余秒数。
    /// 与 429/402 不同，这里的退避依据是可信的（闸门的 `until_ms`），不违反
    /// 「不臆造数字」的纪律 —— 见 [`Self::retry_after_secs`]。
    ModelGatePaused {
        /// 被闸门暂停的模型名（必须点名：用户要知道该换哪个模型）。
        model: String,
        /// 距解冻的剩余秒数（向上取整，至少 1）。
        remain_secs: u64,
        /// 触发暂停时**窗口内**累计的失败次数（不是账号数，见 [`crate::pool::gate`]）。
        failures: usize,
        /// 本轮换号轨迹。闸门短路时它只有一条（`uid` 为空），但那一条恰好是
        /// 全链路诊断价值最高的信息 —— 不能丢（issue #106 §2.4）。
        attempts: Vec<UpstreamAttempt>,
    },
    /// ★ 这把 API Key 的用量配额已用尽 → **429**（issue #76）。
    ///
    /// 与 [`Self::Upstream`] 的账号级 429 是两件事：那个是「这个号被限流，换一个」，
    /// 这个是「**这把 Key 到此为止**」—— 换号、重试、等冷却都不会好，
    /// 因为额度是按 Key 计的。⇒ `attempts` 为空、不进换号循环。
    ///
    /// 文案必须带**单位与周期**（`1,000 万 token / 自然月`）：只说「配额已用尽」
    /// 用户不知道该等多久、该改什么。
    KeyQuotaExceeded {
        /// 用量（当前口径）。
        used: u64,
        /// 上限。
        limit: u64,
        /// 计量单位（token / 请求数）。
        unit: crate::apikey::QuotaUnit,
        /// 重置周期（不重置 ⇒ 文案说「累计」）。
        period: crate::apikey::QuotaPeriod,
    },
    /// 网关自身错误（端口占用/目录失败/内部）→ 502（不返回裸堆栈）
    Internal(String),
}

/// 一次上游尝试的摘要（失败响应的 `attempts` 字段）。
///
/// 只带**排查必需**的四项：哪个账号、什么账号 uid、上游状态码、一句原因。
/// 刻意不带宽高/权重等内部数据 —— 对外契约越小越稳。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpstreamAttempt {
    /// 账号展示名。
    pub account: String,
    /// 账号 uid（`""` = 没选到账号，例如闸门短路）。
    pub uid: String,
    /// 上游 HTTP 状态（`0` = 传输层错误 / 未发出请求）。
    pub status: u16,
    /// 一句可读原因（分类结果或池的拒绝原因）。
    pub reason: String,
}

impl UpstreamAttempt {
    /// 转成对外 JSON（键名 camelCase，与其它响应一致）。
    pub fn to_value(&self) -> Value {
        json!({
            "account": self.account,
            "uid": self.uid,
            "status": self.status,
            "reason": self.reason,
        })
    }
}

/// 「该 region 没有可用凭据」的**具体原因**。
///
/// # 为什么需要它（issue #26，2026-10-14）
///
/// 改造前 `NoCredential` 只有 `region` 一个字段 ⇒ 下面**四类状态逐字共用同一句**
/// 「未找到已登录的 X 账号，请在桌面端登录该版本后重试」：
///
/// | 状态 | 真实处境 | 那句指引 |
/// |:--|:--|:--|
/// | 认证文件缺失 | 桌面端确实没登录 | ✅ 有效 |
/// | 账号库为空 | 同上（但换登录**真的**能修） | ✅ 有效 |
/// | 指定账号不在库 | 账号被删了，登录无关 | ❌ 无效 |
/// | 池里有号但全不可用 | **已经登录了**，号被禁用/冷却/零积分 | ❌ 无效 |
///
/// 后两类下「去桌面端重新登录」是**无效指引**：报障人因此「切换过多个账号、
/// 桌面端和 Buddy Switch 都试过」仍然解决不了 —— 被文案带偏，而不是他操作不对。
/// 更糟的是 Docker / headless 部署**根本没有桌面端**，那句话对这类用户是死路。
///
/// ⇒ 原因必须自证，且指引必须指向**真正能解的那一步**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCredentialCause {
    /// `current` 策略：本机该 region 的认证文件缺失，或存在但读不出凭据。
    ///
    /// （`read_auth_file_for` 把「文件存在但不可解析」视为该位置的**权威**结果，
    /// 不回退下一候选；`imported_account_from_root` 取不到 `access_token` 也返回 `None`。
    /// 两者都落到这一档。）
    AuthFileMissing,
    /// 该 region 账号库为空（`max_credits` 策略，或 relay 同步后池内该区域 0 条）。
    StoreEmpty,
    /// `pinned` 策略：被指定的账号不在该 region 账号库中（多半已被删除）。
    PinnedMissing,
    /// relay 兜底：池里该 region **有**账号，但此刻一个都不可用。
    ///
    /// 计数来自池快照（`Pool::summary`），用于把「有 12 个号但 0 个可用」这类
    /// 事实**直接摆给用户**，而不是让他猜。
    PoolExhausted {
        /// 该区域池内账号总数。
        total: usize,
        /// 其中已禁用（三振终态）的数量。
        disabled: usize,
        /// 其中冷却中的数量。
        cooling: usize,
        /// 其中余额为 0 的数量。
        zero_credits: usize,
        /// 其中被**用户显式排除**的数量。
        ///
        /// 必须单独给出来：否则「5 个号、0 禁用、0 冷却、0 零积分」这句话会自相矛盾
        /// —— 用户看到池里有号、状态都正常，却被告知「0 个可用」。
        excluded: usize,
        /// 其中「状态可用、但在途已满」的数量（**计入** `available`，不是互斥态）。
        ///
        /// 与 `excluded` 是同一个坑的两面：缺了它，「5 个号、0 禁用、0 冷却、0 零积分、
        /// 0 被排除」与「0 个可用」就自相矛盾 —— 用户会以为网关在胡说。
        in_flight_full: usize,
        /// 其中「状态可用、但**该模型**有模型级冷却」的数量（**计入** `available`，
        /// 正交计数，issue #106 §2.4）。
        ///
        /// 缺了它的后果与 `in_flight_full` 一模一样，而且在 6004 场景下**必然触发**：
        /// 池快照会是「9 个号 / 0 禁用 / 0 冷却 / 0 零积分 / 0 排除」却说「0 个可用」，
        /// 唯一解释就藏在这个字段里。用户数了数自己的号，对不上那句话。
        model_cooling: usize,
    },
    /// **密钥白名单内**一个号都不可用（issue #76）。
    ///
    /// 与 [`Self::PoolExhausted`] 的区别只在**口径**：那一条统计整个区域池，
    /// 这一条只统计这把 Key 允许用的账号。白名单生效时必须走这条 —— 否则
    /// 「有 12 个号但 0 个可用」会把用户推去恢复一堆**本来就不许用**的账号，
    /// 而真正的解是「放宽名单」或「恢复名单里那几个」。
    ///
    /// 计数一律是**名单口径**，`pool_total` 单独给出来只用于说明「名单覆盖了其中几个」。
    ScopeEmpty {
        /// 该区域池内账号总数（与 `allowed` 对照，说明名单覆盖面）。
        pool_total: usize,
        /// 名单覆盖、且在该区域池内的账号数（`0` = 名单里没有一个号属于本区域）。
        allowed: usize,
        disabled: usize,
        cooling: usize,
        zero_credits: usize,
        excluded: usize,
        in_flight_full: usize,
        /// 名单内「状态可用、但该模型有模型级冷却」的数量（正交计数，issue #106 §2.4）。
        model_cooling: usize,
    },
}

impl GatewayError {
    /// 上游类别 → HTTP 状态码（A-3.5）。
    pub fn status(&self) -> u16 {
        match self {
            GatewayError::Unauthorized(_)
            | GatewayError::NoCredential { .. }
            | GatewayError::EncryptedCredential { .. }
            | GatewayError::RegionMismatch(_) => 401,
            GatewayError::Upstream { kind, .. } => match kind {
                UpstreamErrorKind::HardCredit => 402,
                UpstreamErrorKind::SoftRate => 429,
                UpstreamErrorKind::SessionDead => 401,
                UpstreamErrorKind::NotFound => 502,
                UpstreamErrorKind::Server => 502,
                UpstreamErrorKind::Client => 400,
            },
            GatewayError::BadRequest(_) => 400,
            // 403 = 明确拒绝（凭据有效、请求合法，但该模型被用户禁用）。
            GatewayError::ModelDisabled { .. } => 403,
            // 503 = 并发占满。凭据正常、几秒内自愈 ⇒ 让客户端退避重试，别去改登录态。
            GatewayError::PoolBusy { .. } => 503,
            // 503 = 模型闸门暂停（issue #106）。与 PoolBusy 同一档语义：
            // 「此刻不可用、稍后自愈、与你的凭据无关」。**绝不能**是 401。
            GatewayError::ModelGatePaused { .. } => 503,
            // 配额用尽 ⇒ 429（Too Many Requests 的本义：这个调用方到此为止）。
            GatewayError::KeyQuotaExceeded { .. } => 429,
            GatewayError::Internal(_) => 502,
        }
    }

    /// 建议客户端退避的秒数（`Retry-After` 响应头）。
    ///
    /// 只给**有确定退避依据**的错误：`pool_busy` 是瞬时并发占满，当前那批请求一结束
    /// 额度就归还，本机量级是亚秒到几秒。
    ///
    /// ★ 限流（429）与余额不足（402）**刻意不给**：真正的解冻时刻在上游的响应里，
    /// 本仓拿不到可信值 —— 编一个数字（比如 60）会让客户端按错误的节奏重试，
    /// 比不给更糟（与「不臆造数字」同一纪律）。
    ///
    /// ★ 模型闸门暂停（issue #106）**必须给**，且与上面那条不冲突：闸门的 `until_ms`
    /// 是**本进程自己算出来的确定值**（`gate_base_ms * 2^(trips-1)` 封顶），不是对
    /// 上游的猜测。这正是「有确定依据才给」的正例。
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            GatewayError::PoolBusy { .. } => Some(POOL_BUSY_RETRY_AFTER_SECS),
            GatewayError::ModelGatePaused { remain_secs, .. } => Some((*remain_secs).max(1)),
            _ => None,
        }
    }

    /// 错误类别名（`type`/`code` 字段用）。
    pub fn type_name(&self) -> &'static str {
        match self {
            GatewayError::Unauthorized(_) => "unauthorized",
            GatewayError::NoCredential { .. } => "no_credential",
            GatewayError::EncryptedCredential { .. } => "encrypted_credential",
            GatewayError::RegionMismatch(_) => "region_mismatch",
            GatewayError::Upstream { kind, .. } => match kind {
                UpstreamErrorKind::HardCredit => "hard_credit",
                UpstreamErrorKind::SoftRate => "soft_rate",
                UpstreamErrorKind::SessionDead => "session_dead",
                UpstreamErrorKind::NotFound => "not_found",
                UpstreamErrorKind::Server => "server",
                UpstreamErrorKind::Client => "client",
            },
            GatewayError::BadRequest(_) => "bad_request",
            GatewayError::ModelDisabled { .. } => "model_disabled",
            GatewayError::PoolBusy { .. } => "pool_busy",
            GatewayError::ModelGatePaused { .. } => "model_gate_paused",
            GatewayError::KeyQuotaExceeded { .. } => "key_quota_exceeded",
            GatewayError::Internal(_) => "internal",
        }
    }

    /// 可读消息（不含堆栈）。
    pub fn message(&self) -> String {
        match self {
            GatewayError::Unauthorized(message) => message.clone(),
            GatewayError::NoCredential { region, cause } => no_credential_message(*region, *cause),
            // ★ 与 NoCredential 刻意分开：这条的用户**其实已经登录了**，
            // 说「未找到已登录的账号」会把人引到错的方向（去重新登录桌面端）。
            // 真正要做的是让该账号在本应用里重新取得**明文**凭据。
            GatewayError::EncryptedCredential { region } => format!(
                "选中的{}账号凭据是 WorkBuddy 加密信封态，网关无法用它发起请求；\
                 请在 Buddy Switch 中对该账号重新登录（或 OAuth 扫码添加）以取得明文凭据",
                region_display(*region)
            ),
            GatewayError::RegionMismatch(mismatch) => mismatch.message(),
            // ★ 截断必须带标记：否则下游拿到的是**未闭合的 JSON**，且无法区分
            //   「上游只发了这么多」与「网关截掉了」（issue #67 因此无法核对上游原文）。
            GatewayError::Upstream { message, .. } => truncate_upstream_message(message),
            GatewayError::BadRequest(message) => message.clone(),
            // 与 Trae 侧共用同一句文案（`disabled_models::disabled_message`），避免两处漂移。
            GatewayError::ModelDisabled { scope, model } => {
                crate::disabled_models::disabled_message(*scope, model)
            }
            // ★ 不能复用「未找到已登录账号」：号是好的，只是这一瞬间都忙。
            //   文案必须点名「在途额度」，否则又会变成「三个 0 却说 0 个可用」。
            //
            // ⚠️ 措辞不能写成「{total} 个可用账号」：`total` 含**被用户排除**的条目
            //    （它们不可选，但账号本身是好的），说成「可用」就是新的数字不自洽。
            GatewayError::KeyQuotaExceeded { used, limit, unit, period } => {
                let unit_text = match unit {
                    crate::apikey::QuotaUnit::Token => "token",
                    crate::apikey::QuotaUnit::Request => "次请求",
                };
                let period_text = match period {
                    crate::apikey::QuotaPeriod::None => "累计".to_string(),
                    crate::apikey::QuotaPeriod::Daily => "每日".to_string(),
                    crate::apikey::QuotaPeriod::Weekly => "每周".to_string(),
                    crate::apikey::QuotaPeriod::Monthly => "自然月".to_string(),
                };
                format!(
                    "这把 API Key 的{period_text}用量已用尽（已用 {used} / 上限 {limit} {unit_text}）；                     换号与重试都不会好 —— 额度按密钥计。请调高上限、切换周期，或改用其它密钥"
                )
            }
            GatewayError::PoolBusy { region, total } => format!(
                "本机{}账号库有 {total} 个账号，可用的那些此刻全被在途请求占满（凭据与余额都正常）：\
                 稍等几秒重试即可；若长期如此，请在「网关 → 账号池」调高每账号在途上限或增加账号",
                region_display(*region)
            ),
            // ★ issue #106：模型闸门暂停必须自证「哪个模型 / 失败几次 / 还剩多久」。
            //   改造前这条路径落到 `no_credential`，用户读到的是「凭据无效」——
            //   事实恰恰相反：凭据完全正常，是上游对该模型整体限流。
            //
            // ⚠️ 措辞刻意与 429/402 分开：那两条的下一步是「等额度 / 充值」，
            //   这条的下一步是「换个模型」或「等几十秒」。`failures` 的语义是
            //   **窗口内失败次数**，不是账号数 —— 说成「N 个账号」在单号池时是假话。
            GatewayError::ModelGatePaused {
                model,
                remain_secs,
                failures,
                ..
            } => {
                if *remain_secs == 0 {
                    format!(
                        "模型「{model}」刚到期，半开试探名额已被其它请求占用：稍等一两秒重试即可\
                         （凭据与余额都正常，无需换号或重新登录）"
                    )
                } else {
                    format!(
                        "模型「{model}」此刻被整体暂停：窗口内已失败 {failures} 次，约 {remain_secs} 秒后自动放行试探。\
                         这是上游对该模型的限流，**与你的凭据无关** —— 换号、重新登录都不会解除。\
                         可以换个模型继续，或稍等再试"
                    )
                }
            }
            GatewayError::Internal(message) => message.clone(),
        }
    }

    /// OpenAI 端点错误体：`{"error":{"message","type","code"}}`。
    pub fn openai_body(&self) -> Value {
        let kind = self.type_name();
        let mut body =
            json!({ "error": { "message": self.message(), "type": kind, "code": kind } });
        if let Some(attempts) = self.attempts_json() {
            body["error"]["attempts"] = attempts;
        }
        body
    }

    /// Anthropic 端点错误体：`{"type":"error","error":{"type","message"}}`。
    pub fn anthropic_body(&self) -> Value {
        let mut body = json!({ "type": "error", "error": { "type": self.type_name(), "message": self.message() } });
        if let Some(attempts) = self.attempts_json() {
            body["error"]["attempts"] = attempts;
        }
        body
    }

    /// 本轮换号轨迹（**唯一出口**）。空表 ⇒ `None`（不往错误体里塞空数组）。
    ///
    /// 这是用户唯一能自查「网关到底换号了没有 / 换到了谁 / 为什么都不行」的数据源；
    /// 只写日志等于不给。
    fn attempts_json(&self) -> Option<Value> {
        match self {
            GatewayError::Upstream { attempts, .. } if !attempts.is_empty() => {
                Some(json!(attempts
                    .iter()
                    .map(UpstreamAttempt::to_value)
                    .collect::<Vec<_>>()))
            }
            // issue #106 §2.4：闸门短路那条路径把最有诊断价值的信息（哪个模型、
            // 被几次限流、还剩几秒）写进了 `attempts`，改造前在 `no_credential`
            // 出口被整表丢掉 —— 用户拿不到任何线索。
            GatewayError::ModelGatePaused { attempts, .. } if !attempts.is_empty() => {
                Some(json!(attempts
                    .iter()
                    .map(UpstreamAttempt::to_value)
                    .collect::<Vec<_>>()))
            }
            _ => None,
        }
    }
}

/// `no_credential` 的分档文案（**唯一出口**）。
///
/// 两条规则，缺一不可：
/// 1. **说清事实** —— 有几个号、几个不可用、分别为什么，而不是笼统的「未找到」；
/// 2. **指引必须真能解** —— 只在「确实没登录」时才让人去登录；对 Docker / headless
///    （本机没有桌面客户端）必须给出替代做法，否则那句话是死路。
///
/// 反面教材（改造前，见 [`NoCredentialCause`] 的文档）：四类状态共用一句
/// 「请在桌面端登录该版本后重试」，把「号被禁用」的用户引向重新登录。
fn no_credential_message(region: Region, cause: NoCredentialCause) -> String {
    let name = region_display(region);
    match cause {
        NoCredentialCause::AuthFileMissing => format!(
            "未找到已登录的{name}账号，请在桌面端登录该版本后重试；\
             若本机没有该版本的桌面客户端（如 Docker / headless 部署），\
             请在「网关 → 账号池」用扫码方式添加账号"
        ),
        NoCredentialCause::StoreEmpty => format!(
            "本机{name}账号库为空，且认证文件也取不到凭据；\
             请先登录该版本桌面端并导入账号，或在本应用内扫码添加账号"
        ),
        NoCredentialCause::PinnedMissing => format!(
            "「指定账号」指向的账号不在本机{name}账号库中（可能已被删除）；\
             请在「网关 → 账号池」重新指定，或改为「不指定」由网关自动择优"
        ),
        NoCredentialCause::PoolExhausted {
            total,
            disabled,
            cooling,
            zero_credits,
            excluded,
            in_flight_full,
            model_cooling,
        } => {
            // ★ 排除必须出现在事实句里（QA 复核指出）：否则「N 个号 / 0 禁用 / 0 冷却 /
            //   0 零积分」会与「0 个可用」自相矛盾，用户根本不知道该去取消排除。
            let excluded_clause = if excluded > 0 {
                format!("、{excluded} 个被你排除")
            } else {
                String::new()
            };
            let excluded_hint = if excluded > 0 {
                "；排除只需在账号池点「取消排除」，账号本身是好的"
            } else {
                ""
            };
            // ★ 在途已满同理：它是**瞬时**状态，且**不需要**用户做任何事，
            //   所以只在事实句里点名、并在提示里说清「会自己好」。
            let in_flight_clause = if in_flight_full > 0 {
                format!("、{in_flight_full} 个在途已满（并发占用中）")
            } else {
                String::new()
            };
            let in_flight_hint = if in_flight_full > 0 {
                "；在途已满只是瞬时并发，几秒后自行恢复，无需操作"
            } else {
                ""
            };
            // ★ issue #106 §2.4：模型级冷却是**正交**信号（账号仍可用、只是这个模型
            //   被罚），不补它就会出现「9 个号、五项计数全 0，却说 0 个可用」——
            //   正是 `error.rs:385` 那段注释点名的反面教材，只因新增了正交维度而复现。
            let model_cooling_clause = if model_cooling > 0 {
                format!("、{model_cooling} 个只因该模型被限流（换模型即可用）")
            } else {
                String::new()
            };
            let model_cooling_hint = if model_cooling > 0 {
                "；模型级限流只影响这一个模型，换一个模型就能继续，账号本身是好的"
            } else {
                ""
            };
            format!(
                "本机{name}账号库有 {total} 个账号，但此刻 0 个可用：\
                 {disabled} 个已禁用、{cooling} 个冷却中、{zero_credits} 个余额为 0\
                 {excluded_clause}{in_flight_clause}{model_cooling_clause}。\
                 重新登录 / 切换账号不会解除禁用、冷却与排除 —— \
                 请在「网关 → 账号池」查看逐账号原因（禁用是终态，需余额刷新成功或人工复活\
                 {excluded_hint}{in_flight_hint}{model_cooling_hint}）"
            )
        }
        // ★ 名单口径（issue #76）：白名单生效时**必须**走这条，否则用户会拿着
        //   「12 个号都不可用」去修一堆他本来就不许用的账号。
        NoCredentialCause::ScopeEmpty {
            pool_total,
            allowed,
            disabled,
            cooling,
            zero_credits,
            excluded,
            in_flight_full,
            model_cooling,
        } => {
            let key_hint =
                "请在「网关 → 密钥 → 权限」放宽这把 Key 的允许账号（或改成「全部账号」）";
            if allowed == 0 {
                format!(
                    "本机{name}账号库有 {pool_total} 个账号，但这把 API Key 的允许名单里\
                     没有一个属于{name}；{key_hint}"
                )
            } else {
                let excluded_clause = if excluded > 0 {
                    format!("、{excluded} 个被你排除")
                } else {
                    String::new()
                };
                let in_flight_clause = if in_flight_full > 0 {
                    format!("、{in_flight_full} 个在途已满（并发占用中）")
                } else {
                    String::new()
                };
                let in_flight_hint = if in_flight_full > 0 {
                    "；在途已满只是瞬时并发，几秒后自行恢复"
                } else {
                    ""
                };
                let model_cooling_clause = if model_cooling > 0 {
                    format!("、{model_cooling} 个只因该模型被限流（换模型即可用）")
                } else {
                    String::new()
                };
                let model_cooling_hint = if model_cooling > 0 {
                    "；模型级限流只影响这一个模型，换一个模型就能继续"
                } else {
                    ""
                };
                format!(
                    "本机{name}账号库有 {pool_total} 个账号，这把 API Key 的允许名单只覆盖其中 \
                     {allowed} 个，而这 {allowed} 个此刻都不可用：{disabled} 个已禁用、\
                     {cooling} 个冷却中、{zero_credits} 个余额为 0\
                     {excluded_clause}{in_flight_clause}{model_cooling_clause}。名单外的账号不会被使用 —— \
                     {key_hint}，或在「网关 → 账号池」恢复这几个账号{in_flight_hint}{model_cooling_hint}"
                )
            }
        }
    }
}

/// region 展示名（错误文案用）。
pub fn region_display(region: Region) -> &'static str {
    region::region_spec(region).display_name
}

/// 上游错误消息的字符上限（防止超长错误体拖垮客户端）。
const UPSTREAM_MESSAGE_LIMIT: usize = 400;

/// 截断标记：下游必须能分辨「上游只发了这么多」与「网关截掉了」。
///
/// 缺了它，被截断的上游 JSON 会以**未闭合**形态发给客户端，`json.loads` 直接报
/// `Unterminated string`，排查者还会误以为上游报文就长这样（issue #67 的实测证据
/// 因此无法与上游原文核对）。
const TRUNCATION_MARKER: &str = "…[truncated]";

/// 超限才截断：保留 `LIMIT - 标记长度` 个字符并追加 [`TRUNCATION_MARKER`]，
/// 使总字符数仍等于 `UPSTREAM_MESSAGE_LIMIT`。
/// 通用截断：超限时保留 `limit - 标记长度` 个字符并追加 [`TRUNCATION_MARKER`]，
/// 使总字符数恰好等于 `limit`，且下游一定能看出「这是网关截的」。
///
/// `limit` 必须不小于标记长度（调用方都是 300/400 量级）。
pub fn truncate_with_marker(text: &str, limit: usize) -> String {
    let marker_len = TRUNCATION_MARKER.chars().count();
    debug_assert!(limit >= marker_len, "上限必须容得下截断标记");
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut head: String = text
        .chars()
        .take(limit.saturating_sub(marker_len))
        .collect();
    head.push_str(TRUNCATION_MARKER);
    head
}

/// 上游错误消息：超限即截断并补标记。
fn truncate_upstream_message(message: &str) -> String {
    truncate_with_marker(message, UPSTREAM_MESSAGE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(kind: UpstreamErrorKind) -> GatewayError {
        GatewayError::Upstream {
            kind,
            status: 0,
            message: "boom".to_string(),
            attempts: Vec::new(),
        }
    }

    #[test]
    fn upstream_kind_status_mapping_matches_design() {
        assert_eq!(kind(UpstreamErrorKind::HardCredit).status(), 402);
        assert_eq!(kind(UpstreamErrorKind::SoftRate).status(), 429);
        assert_eq!(kind(UpstreamErrorKind::SessionDead).status(), 401);
        assert_eq!(kind(UpstreamErrorKind::NotFound).status(), 502);
        assert_eq!(kind(UpstreamErrorKind::Server).status(), 502);
        assert_eq!(kind(UpstreamErrorKind::Client).status(), 400);
    }

    #[test]
    fn local_error_status_mapping() {
        assert_eq!(GatewayError::Unauthorized("x".into()).status(), 401);
        assert_eq!(
            GatewayError::NoCredential {
                region: Region::Cn,
                cause: NoCredentialCause::StoreEmpty,
            }
            .status(),
            401
        );
        assert_eq!(GatewayError::BadRequest("x".into()).status(), 400);
        assert_eq!(GatewayError::Internal("x".into()).status(), 502);
    }

    /// 在途占满是 **503 / `pool_busy`**，不是 401 / `no_credential`。
    ///
    /// 阳性对照：把它并进 `NoCredential`（回 401）⇒ 本用例必红。这条错误码会直接
    /// 决定客户端是「退避重试」还是「弹登录态失效」。
    #[test]
    fn pool_busy_is_503_and_names_the_in_flight_quota() {
        let error = GatewayError::PoolBusy {
            region: Region::Cn,
            total: 3,
        };
        assert_eq!(error.status(), 503);
        assert_eq!(error.type_name(), "pool_busy");
        assert_ne!(
            error.type_name(),
            GatewayError::NoCredential {
                region: Region::Cn,
                cause: NoCredentialCause::StoreEmpty,
            }
            .type_name(),
            "并发饱和不是认证问题，类别名不得复用 no_credential"
        );

        let message = error.message();
        assert!(message.contains("在途"), "文案必须点名在途额度：{message}");
        assert!(message.contains("3 个"), "应摆出账号数：{message}");
        assert!(
            message.contains("重试"),
            "503 必须告诉客户端「稍后重试」，否则等于没给指引：{message}"
        );
    }

    /// 信封凭据必须与「未找到已登录账号」**分开**。
    ///
    /// 混用会把排查引向错的方向：这条的用户其实**已经登录了**，让他「去桌面端登录」
    /// 是无效指引；真正要做的是让该账号在本应用里重新取得**明文**凭据。
    #[test]
    fn encrypted_credential_is_a_distinct_readable_401() {
        let error = GatewayError::EncryptedCredential { region: Region::Cn };
        assert_eq!(error.status(), 401);
        assert_eq!(error.type_name(), "encrypted_credential");
        assert_ne!(
            error.type_name(),
            GatewayError::NoCredential {
                region: Region::Cn,
                cause: NoCredentialCause::StoreEmpty,
            }
            .type_name(),
            "不得与 no_credential 混为一谈"
        );

        let message = error.message();
        assert!(message.contains("加密信封"), "文案应可读：{message}");
        assert!(message.contains("WorkBuddy"), "应点名是哪个区域：{message}");
        assert!(
            message.contains("重新登录"),
            "应给出可行动的做法：{message}"
        );
    }

    /// 每一类原因的文案必须**两两不同**，且都必须点名区域。
    ///
    /// 阳性对照：改造前四类共用一句（逐字相同）⇒ 本用例必红。
    #[test]
    fn no_credential_causes_are_distinct_and_named() {
        let region = Region::Cn;
        let causes = [
            NoCredentialCause::AuthFileMissing,
            NoCredentialCause::StoreEmpty,
            NoCredentialCause::PinnedMissing,
            NoCredentialCause::PoolExhausted {
                total: 12,
                disabled: 9,
                cooling: 3,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
            // 名单口径两种分支（issue #76）：覆盖到几个 / 一个都没覆盖。
            NoCredentialCause::ScopeEmpty {
                pool_total: 12,
                allowed: 2,
                disabled: 1,
                cooling: 1,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
            NoCredentialCause::ScopeEmpty {
                pool_total: 12,
                allowed: 0,
                disabled: 0,
                cooling: 0,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
        ];
        let messages: Vec<String> = causes
            .iter()
            .map(|cause| {
                GatewayError::NoCredential {
                    region,
                    cause: *cause,
                }
                .message()
            })
            .collect();

        for (cause, message) in causes.iter().zip(&messages) {
            assert!(
                message.contains("WorkBuddy"),
                "{cause:?} 的文案必须点名区域：{message}"
            );
        }
        for i in 0..messages.len() {
            for j in (i + 1)..messages.len() {
                assert_ne!(
                    messages[i], messages[j],
                    "{:?} 与 {:?} 的文案逐字相同 —— 用户无法据此判断该做什么",
                    causes[i], causes[j]
                );
            }
        }
    }

    /// issue #76：`ScopeEmpty` 与 `PoolExhausted` 共用 `type_name`（对外契约不变），
    /// 但文案必须给出**名单口径**的事实与真正能解的那一步。
    ///
    /// 阳性对照：把文案换成 `PoolExhausted` 那句 ⇒ 第一条断言（必须出现「允许名单」）必红。
    #[test]
    fn scope_empty_points_at_the_allowlist_not_the_pool() {
        let error = GatewayError::NoCredential {
            region: Region::Cn,
            cause: NoCredentialCause::ScopeEmpty {
                pool_total: 12,
                allowed: 2,
                disabled: 1,
                cooling: 1,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
        };
        // ★ 对外契约：仍是 `no_credential` / 401 —— 客户端按类别名分流，不能多一个类别。
        assert_eq!(error.type_name(), "no_credential");
        assert_eq!(error.status(), 401);

        let message = error.message();
        assert!(
            message.contains("允许名单"),
            "必须点出是名单导致的：{message}"
        );
        assert!(
            message.contains("允许名单只覆盖其中 2 个"),
            "必须说清名单覆盖面：{message}"
        );
        assert!(
            message.contains("名单外的账号不会被使用"),
            "必须消除「网关把号藏起来了」的误解：{message}"
        );
        assert!(
            message.contains("密钥 → 权限"),
            "必须指向真正能解的那一步（放宽名单）：{message}"
        );
        assert!(
            !message.contains("重新登录"),
            "名单问题是策略问题，让人去重新登录是无效指引：{message}"
        );

        // 名单一个都没覆盖到 ⇒ 不能说「2 个都不可用」，要说「名单里没有本区域的号」。
        let uncovered = GatewayError::NoCredential {
            region: Region::Cn,
            cause: NoCredentialCause::ScopeEmpty {
                pool_total: 12,
                allowed: 0,
                disabled: 0,
                cooling: 0,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
        }
        .message();
        assert!(
            uncovered.contains("没有一个属于"),
            "名单没覆盖本区域要明说：{uncovered}"
        );
    }

    /// ★ issue #26 的核心护栏：**「池里有号但一个都不可用」不得再让人去重新登录**。
    ///
    /// 真实报障（2026-10-14）：`切换过多个账号 桌面端或者buddyswitch都试过解决不了`。
    /// 该用户的处境是「号在库里、但全被禁用 / 冷却」，而改造前的文案与「从没登录过」
    /// **逐字相同** ⇒ 他被引去做了一件**不可能有用**的事。
    ///
    /// 断言两侧：① 事实必须摆出来（总数 + 分类计数）；② 那句无效指引**必须消失**。
    #[test]
    fn pool_exhausted_message_states_facts_and_drops_the_relogin_advice() {
        let message = GatewayError::NoCredential {
            region: Region::Cn,
            cause: NoCredentialCause::PoolExhausted {
                total: 12,
                disabled: 9,
                cooling: 3,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
        }
        .message();

        assert!(message.contains("12 个账号"), "应给出账号总数：{message}");
        assert!(message.contains("9 个已禁用"), "应给出禁用计数：{message}");
        assert!(message.contains("3 个冷却中"), "应给出冷却计数：{message}");
        assert!(
            !message.contains("请在桌面端登录该版本"),
            "「池里有号但全不可用」时不得再让人去桌面端登录（无效指引）：{message}"
        );
    }

    /// 「确实没登录」那一档**必须保留**桌面端指引 —— 否则是把有效指引一起删掉。
    ///
    /// 这是上一条断言的**阳性对照**：没有它，把四类文案全改成「请联系客服」
    /// 也能让上一条变绿。
    #[test]
    fn auth_file_missing_message_keeps_the_desktop_advice() {
        let message = GatewayError::NoCredential {
            region: Region::Cn,
            cause: NoCredentialCause::AuthFileMissing,
        }
        .message();
        assert!(
            message.contains("请在桌面端登录该版本后重试"),
            "真·未登录时必须保留桌面端指引：{message}"
        );
    }

    /// Docker / headless 用户**没有桌面端** ⇒ 文案必须给替代做法。
    ///
    /// 反例：只有「请在桌面端登录该版本后重试」一句时，容器部署的用户
    /// 照着做**做不到**（容器里没有图形客户端）。
    #[test]
    fn headless_deployments_get_a_usable_alternative() {
        for cause in [
            NoCredentialCause::AuthFileMissing,
            NoCredentialCause::StoreEmpty,
        ] {
            let message = GatewayError::NoCredential {
                region: Region::Cn,
                cause,
            }
            .message();
            assert!(
                message.contains("扫码"),
                "{cause:?} 应给无桌面客户端场景的替代做法：{message}"
            );
        }
    }

    /// `type_name` 是**对外分流契约**，四类原因必须共用 `no_credential`。
    ///
    /// 客户端（以及 `docs/specs/P11-ARCH.md` §1.5）按类别名分流；
    /// 顺手把它拆成四个新类别名会让既有客户端全部走进未知分支。
    #[test]
    fn no_credential_type_name_stays_stable_across_causes() {
        for cause in [
            NoCredentialCause::AuthFileMissing,
            NoCredentialCause::StoreEmpty,
            NoCredentialCause::PinnedMissing,
            NoCredentialCause::PoolExhausted {
                total: 1,
                disabled: 1,
                cooling: 0,
                excluded: 0,
                zero_credits: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
        ] {
            let error = GatewayError::NoCredential {
                region: Region::Cn,
                cause,
            };
            assert_eq!(error.type_name(), "no_credential", "{cause:?} 不得改类别名");
            assert_eq!(error.status(), 401, "{cause:?} 必须是 401");
        }
    }

    #[test]
    fn region_mismatch_is_unauthorized_and_upstream_messages_are_bounded() {
        let mismatch = GatewayError::RegionMismatch(RegionMismatch {
            actual_domain: "www.workbuddy.ai".to_string(),
            expected_file: "workbuddy-desktop.info".to_string(),
            env_var: "WORKBUDDY_AUTH_FILE".to_string(),
            actual_region: Region::Global,
            expected_region: Region::Cn,
        });
        assert_eq!(mismatch.status(), 401);
        assert_eq!(mismatch.type_name(), "region_mismatch");
        let upstream = GatewayError::Upstream {
            kind: UpstreamErrorKind::Server,
            status: 500,
            message: "x".repeat(1000),
            attempts: Vec::new(),
        };
        assert_eq!(upstream.message().chars().count(), 400);
        assert!(
            upstream.message().ends_with(TRUNCATION_MARKER),
            "截断必须带标记，否则下游无法分辨上游原文与网关截断: {}",
            upstream.message()
        );

        // 未超限的消息必须逐字透传（标记只应出现在真的截断过的消息上）。
        let short = GatewayError::Upstream {
            kind: UpstreamErrorKind::Client,
            status: 400,
            message: "Illegal API invocation from an unapproved channel".to_string(),
            attempts: Vec::new(),
        };
        assert_eq!(
            short.message(),
            "Illegal API invocation from an unapproved channel"
        );
    }

    #[test]
    fn error_bodies_have_expected_shape() {
        let err = kind(UpstreamErrorKind::HardCredit);
        let openai = err.openai_body();
        assert_eq!(openai["error"]["type"], "hard_credit");
        assert_eq!(openai["error"]["code"], "hard_credit");
        assert!(openai["error"]["message"].is_string());

        let anthropic = err.anthropic_body();
        assert_eq!(anthropic["type"], "error");
        assert_eq!(anthropic["error"]["type"], "hard_credit");
        assert!(anthropic["error"]["message"].is_string());
    }

    /// ★★ issue #106 的核心不变量：**「模型闸门暂停」的对外状态码不是 401**。
    ///
    /// 报障现场：客户端把 401/403 固定归为 `AUTH`、**不可重试**、渲染成
    /// 「本轮运行失败 API 密钥无效」⇒ 整轮会话直接终止。而闸门暂停是**临时容量问题**，
    /// 凭据完全正常。变异：把 `status()` 里这条改回 401 ⇒ 本用例红。
    #[test]
    fn model_gate_pause_is_503_with_retry_after_and_never_401() {
        let err = GatewayError::ModelGatePaused {
            model: "glm-5.2".to_string(),
            remain_secs: 8,
            failures: 2,
            attempts: vec![UpstreamAttempt {
                account: String::new(),
                uid: String::new(),
                status: 0,
                reason: "glm-5.2 整体暂停".to_string(),
            }],
        };
        assert_eq!(err.status(), 503, "闸门暂停必须是可重试的 503，不能是 401");
        assert_eq!(err.type_name(), "model_gate_paused");
        assert_eq!(
            err.retry_after_secs(),
            Some(8),
            "闸门的 until_ms 是本进程算出的确定值，必须给 Retry-After"
        );

        let message = err.message();
        assert!(message.contains("glm-5.2"), "必须点名模型：{message}");
        assert!(message.contains('8'), "必须给剩余秒数：{message}");
        assert!(
            message.contains("与你的凭据无关"),
            "必须明确否定「凭据坏了」这个误判：{message}"
        );

        // ★ 诊断信息不能丢：闸门短路那条路径最有价值的就是「哪个模型、几次、几秒」。
        let body = err.openai_body();
        assert_eq!(body["error"]["attempts"][0]["reason"], "glm-5.2 整体暂停");

        // 半开票被别的请求领走（remain_ms == 0）⇒ 文案不能说「还剩 0 秒」。
        let taken = GatewayError::ModelGatePaused {
            model: "m".to_string(),
            remain_secs: 0,
            failures: 0,
            attempts: Vec::new(),
        };
        assert_eq!(taken.status(), 503);
        assert_eq!(taken.retry_after_secs(), Some(1), "Retry-After 至少 1 秒");
        assert!(
            !taken.message().contains("0 秒后"),
            "不能说「0 秒后自动放行」：{}",
            taken.message()
        );
    }

    /// ★ issue #106 §2.4：`no_credential` 的计数维度缺了 `model_cooling` 时，
    /// 文案会渲染成「9 个号、五项计数全 0，却说 0 个可用」—— 自相矛盾。
    ///
    /// 变异：删掉 `model_cooling_clause` ⇒ 本用例红。
    #[test]
    fn pool_exhausted_message_accounts_for_model_cooling() {
        let message = GatewayError::NoCredential {
            region: Region::Cn,
            cause: NoCredentialCause::PoolExhausted {
                total: 9,
                disabled: 0,
                cooling: 0,
                zero_credits: 0,
                excluded: 0,
                in_flight_full: 0,
                model_cooling: 9,
            },
        }
        .message();
        assert!(
            message.contains("只因该模型被限流"),
            "必须把正交的模型级冷却摆进事实句：{message}"
        );
        assert!(
            message.contains("换模型"),
            "必须给出真能解的动作（换模型）：{message}"
        );

        // 反向对照：没有模型级冷却时不得出现这句（否则是新的噪音）。
        let clean = GatewayError::NoCredential {
            region: Region::Cn,
            cause: NoCredentialCause::PoolExhausted {
                total: 2,
                disabled: 2,
                cooling: 0,
                zero_credits: 0,
                excluded: 0,
                in_flight_full: 0,
                model_cooling: 0,
            },
        }
        .message();
        assert!(!clean.contains("只因该模型被限流"), "{clean}");
    }
}
