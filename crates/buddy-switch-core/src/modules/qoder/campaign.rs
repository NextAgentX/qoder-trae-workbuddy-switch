//! Qoder **活动平台**（`/sash/api/v1/me/campaigns`）—— 当前官方的签到 / 福利机制。
//!
//! ## 为什么不能只用旧的 `daily-check-in` 通道
//!
//! 官方把「每日领取 100 Credits」这类限时活动**搬到了活动平台**。旧的
//! `/sash/api/v1/me/daily-check-in/*` 只在仍开放时可用 —— 实测**国内版返回 `DISABLED`、
//! 国际版全 404**。⇒ 只实现旧通道 = 用户点「签到」永远拿到「本区域未开放」。
//! 旧通道因此降级为 [`super::checkin`] 的**兜底**，主力是本模块。
//!
//! ## 领取链路（三层缺一不可）
//!
//! ```text
//! ① 桌面端请求头   Cosy-ClientType: 10 + Cosy-Version + UA: Qoder
//!                  （缺了 ⇒ 服务端**不报错**、直接返回**空活动列表**）
//! ② 真实机器身份   cosy-machine* 六头（来自官方原生桥，见 [`super::native`]）
//!                  （用派生假值 ⇒ 列表静默少掉设备定向活动；故无原生桥时**一个都不发**）
//! ③ 领取           POST /sash/api/v1/me/campaigns/{id}/claim（官方幂等）
//! ```
//!
//! ## 服务端按「**人**」去重（不是按账号）
//!
//! 同机多号共享每轮一次的额度：后领的号拿到 `SAME_PERSON_ALREADY_CLAIMED`，
//! 且**列表里连活动都不显示**。我们记 6 小时冷却（落盘），冷却期内不再白打上游，
//! 并在结果里如实报「同人已领取」——**不谎报成功，也不谎报失败**。
//!
//! ## 与 `credits.rs` 的分工
//!
//! `credits` 是**余额快照**（实时查询 `/api/v2/quota/usage`），本模块是**领取动作**。
//! 两者口径不同、互不替代：签到成功不会立刻改余额读数，余额也不代表今天领没领。

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};

use super::account;
use super::api;
use super::device;
use super::native::{self, NativeIdentity};
use super::region::{realm_spec, QoderRegion};
use super::store;

// ---------------------------------------------------------------------------
// 常量（对外可见，供文案与门禁引用）
// ---------------------------------------------------------------------------

/// 桌面端 `cosy-clienttype`（CLI = 5 / QoderWork = 6 / **桌面端 = 10**）。
pub const DESKTOP_CLIENT_TYPE: &str = "10";
/// 桌面端 `cosy-version` 兜底值（客户端更新后可用环境变量覆盖）。
pub const DESKTOP_CLIENT_VERSION: &str = "0.4.3";
/// `cosy-machineos` 头（与官方桌面端同形）。
pub const MACHINE_OS: &str = "x86_64_win32";
/// `cosy-machinehostname` 头。
pub const MACHINE_HOSTNAME: &str = "DESKTOP-QODER";

/// 本次**发送了**全套 `cosy-machine*` 头（原生身份可用）。
pub const MACHINE_HEADERS_NATIVE: &str = "native";
/// 本次**一个都没发**（无原生桥时的正确行为，见模块头）。
pub const MACHINE_HEADERS_OMITTED: &str = "omitted";

/// 相邻两次 claim 之间的最小间隔（防风控）。
pub const CLAIM_GAP: Duration = Duration::from_secs(1);
/// 「同人已领」冷却时长（秒）。
pub const BLOCKED_COOLDOWN_SECS: i64 = 6 * 3600;
/// 活动列表短缓存（上游最慢的一环；领取路径强制绕过）。
pub const CAMPAIGNS_TTL: Duration = Duration::from_secs(20);

/// 上游失败码 → 中文原因（与官方前端状态机一致）。
///
/// ⚠️ 这是一张**展示**表，不是判据：判据永远是 `claimStatus` / `actionType`
/// （见 [`classify`]）。表里没有的码一律原样透出，**不猜**。
pub const FAILURE_REASONS: &[(&str, &str)] = &[
    (
        "REDEMPTION_CODE_OUT_OF_STOCK",
        "今日名额已发完（每日 10:00 刷新，次日再来）",
    ),
    (
        "ACHIEVEMENT_NOT_COMPLETED",
        "需先完成新人任务（成就未完成）",
    ),
    ("CAMPAIGN_NOT_ACTIVE", "活动已结束 / 未开始"),
    ("RISK_BLOCKED", "风控拦截（当前设备 / 账号不可领取）"),
    ("RISK_DEPENDENCY_UNAVAILABLE", "风控服务不可用，稍后重试"),
];

/// 上游失败码的中文说明；不认识 ⇒ `None`（调用方原样透出，不编）。
pub fn failure_reason(code: &str) -> Option<&'static str> {
    FAILURE_REASONS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(code))
        .map(|(_, text)| *text)
}

// ---------------------------------------------------------------------------
// 活动模型
// ---------------------------------------------------------------------------

/// 一条官方活动（已从 `placements` 里抽出中英文案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignItem {
    pub campaign_id: String,
    pub campaign_key: String,
    /// 中文标题（`placements[].content.zh.title`）→ 英文 → 活动键。
    pub title: String,
    pub description: String,
    pub detail_url: String,
    pub button_text: String,
    /// `CLAIM_BENEFIT` / `VIEW_DETAILS` / 空。
    pub action_type: String,
    /// `CLAIMABLE` / `CLAIMED` / 其它。
    pub claim_status: String,
    /// 奖励种类（`CREDITS` / `REDEMPTION_CODE` / 空）。
    pub benefit_kind: String,
    pub benefit_amount: i64,
    pub required_achievement_key: String,
    pub achievement_completed: bool,
    pub unavailable_reason: String,
    pub start_at: i64,
    pub end_at: i64,
}

impl CampaignItem {
    /// 展示名（标题 → 活动键 → id）—— 恒非空。
    pub fn label(&self) -> String {
        for candidate in [&self.title, &self.campaign_key, &self.campaign_id] {
            if !candidate.trim().is_empty() {
                return candidate.clone();
            }
        }
        "未命名活动".to_string()
    }

    /// 是不是「签到领积分」类活动（只领这类，不动券 / 兑换码）。
    ///
    /// 判据 = `benefit.kind` 为 `CREDITS` 或**空**（空 = 老活动没写 kind，
    /// 实测「每日领取 100 Credits」在部分区域就是空 kind）。
    pub fn is_credit_kind(&self) -> bool {
        let kind = self.benefit_kind.trim().to_ascii_uppercase();
        kind.is_empty() || kind == "CREDITS"
    }
}

/// 一条活动的**处置分类**（纯函数，见 [`classify`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignClass {
    /// 已领取（今日已完成）。
    Already,
    /// 可以领。
    Claimable,
    /// 名额发完（次日重试）。
    Pending,
    /// 需先完成新人任务（被锁）。
    Locked,
    /// 详情类（无奖励 / 不可领）—— 不参与领取，也不算「已领取」。
    View,
}

/// 由活动状态推出处置分类。
///
/// ## ★ 归类**先于** `action_type` 过滤（这条顺序是实测踩出来的）
///
/// 官方在**领取后**会把 `actionType` 降级成 `VIEW_DETAILS`（国际版实测）。若先按
/// `action_type` 过滤，一条**已经领到**的活动会被归成「详情类」，界面上就变成
/// 「当前账号暂无可领取的官方活动」——把「已领」的事实丢掉。
/// ⇒ 判据：`CLAIMED` 且（有奖励 **或** `action_type` 仍是可领类）⇒ 已领取。
///
/// 可证伪：把 `Already` 分支挪到 `action_type` 过滤之后 ⇒ `claimed_then_downgraded_to_view`
/// 用例立即变红。
pub fn classify(item: &CampaignItem) -> CampaignClass {
    let actionable = item.action_type.is_empty() || item.action_type == "CLAIM_BENEFIT";
    if item.claim_status == "CLAIMED" {
        return if item.benefit_amount > 0 || actionable {
            CampaignClass::Already
        } else {
            CampaignClass::View
        };
    }
    // 以下只针对「还没领」的活动：详情类无奖励，不参与领取 / 归类。
    if !actionable {
        return CampaignClass::View;
    }
    if item.claim_status != "CLAIMABLE" {
        return match item.unavailable_reason.to_ascii_uppercase().as_str() {
            "REDEMPTION_CODE_OUT_OF_STOCK" => CampaignClass::Pending,
            "ACHIEVEMENT_NOT_COMPLETED" => CampaignClass::Locked,
            _ if !item.achievement_completed => CampaignClass::Locked,
            _ => CampaignClass::View,
        };
    }
    CampaignClass::Claimable
}

/// 活动列表响应里我们关心的部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignsPayload {
    pub show_campaign: bool,
    pub claimable: bool,
    pub campaign_url: String,
    pub items: Vec<CampaignItem>,
}

/// 解析 `GET /sash/api/v1/me/campaigns` 的响应（纯函数，可测）。
///
/// 缺字段一律给**保守默认**（`false` / 空串），**不猜**；`campaigns` 不是数组 ⇒ 空表
/// （空表 ≠ 报错：服务端在身份不被认可时就是给空列表，见模块头）。
pub fn parse_campaigns(payload: &Value) -> CampaignsPayload {
    let items = payload
        .get("campaigns")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(parse_item).collect())
        .unwrap_or_default();
    CampaignsPayload {
        show_campaign: payload
            .get("showCampaign")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        claimable: payload
            .get("claimable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        campaign_url: payload
            .get("campaignUrl")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        items,
    }
}

/// 单条活动解析（非对象 / 无 id ⇒ `None`，不造空壳）。
fn parse_item(raw: &Value) -> Option<CampaignItem> {
    let object = raw.as_object()?;
    let get = |keys: &[&str]| -> Option<&Value> { keys.iter().find_map(|key| object.get(*key)) };
    let campaign_id = str_of(get(&["campaignId", "campaign_id"]));
    let campaign_key = str_of(get(&["campaignKey", "campaign_key"]));
    if campaign_id.is_empty() && campaign_key.is_empty() {
        return None;
    }
    let (title_zh, title_en, desc_zh, detail, button) = placements_text(object.get("placements"));
    let title = if title_zh.is_empty() {
        title_en
    } else {
        title_zh
    };
    let benefit = object.get("benefit").and_then(Value::as_object);
    Some(CampaignItem {
        campaign_id,
        campaign_key,
        title,
        description: desc_zh,
        detail_url: detail,
        button_text: button,
        action_type: str_of(get(&["actionType", "action_type"])),
        claim_status: str_of(get(&["claimStatus", "claim_status"])).to_ascii_uppercase(),
        benefit_kind: benefit
            .and_then(|map| map.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        benefit_amount: benefit
            .and_then(|map| map.get("amount"))
            .and_then(api::as_num)
            .unwrap_or(0),
        required_achievement_key: str_of(get(&[
            "requiredAchievementKey",
            "required_achievement_key",
        ])),
        achievement_completed: object
            .get("achievementCompleted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        unavailable_reason: str_of(get(&["unavailableReason", "unavailable_reason"])),
        start_at: get(&["startAt", "start_at"])
            .and_then(api::as_num)
            .unwrap_or(0),
        end_at: get(&["endAt", "end_at"]).and_then(api::as_num).unwrap_or(0),
    })
}

/// 把 JSON 值当字符串取（数字也转成字符串；其它 ⇒ 空串）。
fn str_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// 从 `placements[].content.{zh,en}` 抽标题 / 说明 / 详情页 / 按钮文案。
///
/// 返回 `(title_zh, title_en, desc_zh, detail_url, button_text)`；**逐个字段**取第一个
/// 非空值（不同 placement 可能只填了其中几项），不是「取第一个 placement」——
/// 后者会因为第一条 placement 缺 description 而丢掉后面那条里的文案。
fn placements_text(placements: Option<&Value>) -> (String, String, String, String, String) {
    let mut title_zh = String::new();
    let mut title_en = String::new();
    let mut desc_zh = String::new();
    let mut detail = String::new();
    let mut button = String::new();
    for placement in placements.and_then(Value::as_array).into_iter().flatten() {
        let Some(content) = placement.get("content").and_then(Value::as_object) else {
            continue;
        };
        let zh = content.get("zh").and_then(Value::as_object);
        let en = content.get("en").and_then(Value::as_object);
        let get = |map: Option<&serde_json::Map<String, Value>>, key: &str| {
            map.and_then(|map| map.get(key))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        if title_zh.is_empty() {
            title_zh = get(zh, "title");
        }
        if title_en.is_empty() {
            title_en = get(en, "title");
        }
        if desc_zh.is_empty() {
            desc_zh = get(zh, "description");
        }
        if detail.is_empty() {
            detail = {
                let zh_detail = get(zh, "detailUrl");
                if zh_detail.is_empty() {
                    get(en, "detailUrl")
                } else {
                    zh_detail
                }
            };
        }
        if button.is_empty() {
            button = {
                let zh_button = get(zh, "buttonText");
                if zh_button.is_empty() {
                    get(en, "buttonText")
                } else {
                    zh_button
                }
            };
        }
        if !title_zh.is_empty() && !desc_zh.is_empty() && !detail.is_empty() {
            break;
        }
    }
    (title_zh, title_en, desc_zh, detail, button)
}

// ---------------------------------------------------------------------------
// 下一次签到窗口（每日 10:00，UTC+8）
// ---------------------------------------------------------------------------

/// 下一个「每日 10:00（UTC+8）」的 epoch 秒 + 可读文本。
///
/// ⚠️ 无论本次是否真的领到都给（真领取 / 已领 / 被挡 / 名额发完 / 任务未完成 /
/// 无可领项 全部走这一个出口）—— 是否渲染由界面按需决定，后端不替界面判断。
///
/// `now` 由调用方注入（纯函数可测）。
pub fn next_checkin_window(now: i64) -> (i64, String) {
    const DAY: i64 = 86_400;
    const OFFSET: i64 = 8 * 3600; // UTC+8
                                  // 把「当前时刻」搬到 UTC+8 的坐标轴上取当日 10:00，再搬回来。
    let local = now + OFFSET;
    let local_day_start = local.div_euclid(DAY) * DAY;
    let mut target = local_day_start + 10 * 3600;
    if local >= target {
        target += DAY;
    }
    let at = target - OFFSET;
    let text = chrono::DateTime::from_timestamp(at, 0)
        .map(|instant| {
            instant
                .with_timezone(&chrono::FixedOffset::east_opt(OFFSET as i32).unwrap())
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    (at, text)
}

// ---------------------------------------------------------------------------
// 请求头
// ---------------------------------------------------------------------------

/// 桌面端版本号（可用 `BUDDY_SWITCH_QODER_DESKTOP_VERSION` 覆盖）。
pub fn desktop_version() -> String {
    std::env::var("BUDDY_SWITCH_QODER_DESKTOP_VERSION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DESKTOP_CLIENT_VERSION.to_string())
}

/// 活动平台的出站头 + **本次机器头状态**（见模块头 ①②）。
///
/// ★ 关键取舍：`identity` 为 `None`（无原生桥）时**一个 `cosy-machine*` 头都不发**。
/// 发派生假头会被服务端判定为非官方客户端、把 `CLAIMABLE` 的「每日领取 100 Credits」
/// **整条过滤掉**（实测），比不发更糟 —— 「不发」是正确行为，不是缺省。
pub fn desktop_headers(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
    identity: Option<&NativeIdentity>,
) -> (Vec<(String, String)>, &'static str) {
    let spec = realm_spec(region);
    let mut headers = vec![
        ("Content-Type".into(), "application/json".into()),
        ("Accept".into(), "application/json, text/plain, */*".into()),
        ("User-Agent".into(), "Qoder".into()),
        ("Authorization".into(), format!("Bearer {access_token}")),
        (
            "X-Request-ID".into(),
            format!(
                "{}-{:06}",
                device::derive_id(uid, "req"),
                chrono::Utc::now().timestamp_subsec_micros() % 1_000_000
            ),
        ),
        ("X-Machine-ID".into(), device::derive_id(uid, "machine")),
        ("X-Session-ID".into(), device::derive_id(uid, "session")),
        ("Origin".into(), spec.website.to_string()),
        ("Referer".into(), format!("{}/", spec.website)),
        ("cosy-clienttype".into(), DESKTOP_CLIENT_TYPE.into()),
        ("cosy-version".into(), desktop_version()),
    ];
    match identity {
        Some(identity) => {
            headers.extend([
                ("cosy-machineid".into(), device::derive_id(uid, "machine")),
                ("cosy-machinetoken".into(), identity.machine_token.clone()),
                ("cosy-machinetype".into(), identity.machine_type.clone()),
                ("cosy-machinecode".into(), identity.machine_code.clone()),
                ("cosy-machineos".into(), MACHINE_OS.into()),
                ("cosy-machinehostname".into(), MACHINE_HOSTNAME.into()),
            ]);
            (headers, MACHINE_HEADERS_NATIVE)
        }
        None => (headers, MACHINE_HEADERS_OMITTED),
    }
}

/// 无原生桥时的可读提示（只有国际版需要提醒，国内版不发头是正确行为）。
fn machine_headers_hint(region: QoderRegion, state: &str) -> String {
    if region == QoderRegion::Global && state == MACHINE_HEADERS_OMITTED {
        return "国际版服务端要求真实的机器身份（由官方客户端组件生成）；本机没有该组件，\
                本次未发送机器头，活动列表可能不可见、领取可能失败。这是已知限制，\
                不等于今天没有活动。"
            .to_string();
    }
    String::new()
}

// ---------------------------------------------------------------------------
// 一次活动的快照（HTTP 结果）
// ---------------------------------------------------------------------------

/// 活动列表的一次查询结果（含身份来源与机器头状态，供界面如实展示）。
#[derive(Debug, Clone)]
pub struct CampaignsSnapshot {
    pub ok: bool,
    /// 该区域**是否有**活动平台（404/405/410 ⇒ 无）。
    pub available: bool,
    pub show_campaign: bool,
    pub claimable: bool,
    pub campaign_url: String,
    pub items: Vec<CampaignItem>,
    /// `runtime-info`（原生桥）或 `derived`（派生兜底）。
    pub identity: &'static str,
    /// `native`（发了机器头）或 `omitted`（一个都没发）。
    pub machine_headers: &'static str,
    pub hint: String,
    pub error: Option<String>,
}

impl CampaignsSnapshot {
    fn failed(available: bool, error: String) -> Self {
        Self {
            ok: false,
            available,
            show_campaign: false,
            claimable: false,
            campaign_url: String::new(),
            items: Vec::new(),
            identity: native::IDENTITY_DERIVED,
            machine_headers: MACHINE_HEADERS_OMITTED,
            hint: String::new(),
            error: Some(error),
        }
    }

    /// 下发形态（前端用）。
    pub fn to_value(&self) -> Value {
        json!({
            "ok": self.ok,
            "available": self.available,
            "showCampaign": self.show_campaign,
            "claimable": self.claimable,
            "campaignUrl": self.campaign_url,
            "identity": self.identity,
            "machineHeaders": self.machine_headers,
            "hint": self.hint,
            "error": self.error,
            "campaigns": self.items.iter().map(campaign_to_value).collect::<Vec<_>>(),
        })
    }
}

/// 单条活动的下发形态（camelCase；前端直接消费）。
pub fn campaign_to_value(item: &CampaignItem) -> Value {
    json!({
        "campaignId": item.campaign_id,
        "campaignKey": item.campaign_key,
        "title": item.label(),
        "description": item.description,
        "detailUrl": item.detail_url,
        "buttonText": item.button_text,
        "actionType": item.action_type,
        "claimStatus": item.claim_status,
        "benefitKind": item.benefit_kind,
        "benefitAmount": item.benefit_amount,
        "requiredAchievementKey": item.required_achievement_key,
        "achievementCompleted": item.achievement_completed,
        "unavailableReason": item.unavailable_reason,
        "startAt": item.start_at,
        "endAt": item.end_at,
        "class": match classify(item) {
            CampaignClass::Already => "already",
            CampaignClass::Claimable => "claimable",
            CampaignClass::Pending => "pending",
            CampaignClass::Locked => "locked",
            CampaignClass::View => "view",
        },
        "unavailableReasonText": failure_reason(&item.unavailable_reason),
    })
}

// ---------------------------------------------------------------------------
// HTTP 执行
// ---------------------------------------------------------------------------

/// `GET /sash/api/v1/me/campaigns`（含一次身份被拒时的自愈重试）。
pub async fn fetch_campaigns(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
) -> CampaignsSnapshot {
    let identity = native::cached_identity(region, uid, false);
    let mut snapshot = fetch_campaigns_once(region, uid, access_token, identity.as_ref()).await;
    // 自愈：服务端明确 `showCampaign=false` 且我们用的是**原生**身份 ⇒ 换一套新身份重试一次。
    // （派生分支不重试：它本来就不发机器头，换身份没有意义。）
    if snapshot.ok
        && !snapshot.show_campaign
        && snapshot.identity == native::IDENTITY_NATIVE
        && !native::disabled()
    {
        if let Some(fresh) = native::cached_identity(region, uid, true) {
            let retry = fetch_campaigns_once(region, uid, access_token, Some(&fresh)).await;
            if retry.ok && retry.show_campaign {
                snapshot = retry;
            }
        }
    }
    snapshot
}

async fn fetch_campaigns_once(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
    identity: Option<&NativeIdentity>,
) -> CampaignsSnapshot {
    let spec = realm_spec(region);
    let (headers, machine_state) = desktop_headers(region, uid, access_token, identity);
    let identity_source = if identity.is_some() {
        native::IDENTITY_NATIVE
    } else {
        native::IDENTITY_DERIVED
    };
    let hint = machine_headers_hint(region, machine_state);
    let client = match api::shared_client() {
        Ok(client) => client,
        Err(error) => return CampaignsSnapshot::failed(true, error),
    };
    let response = client
        .get(format!("{}{}", spec.openapi_base, super::PATH_CAMPAIGNS))
        .timeout(Duration::from_secs(15))
        .headers(header_map(&headers))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => return CampaignsSnapshot::failed(true, format!("活动列表请求失败: {error}")),
    };
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if matches!(status, 404 | 405 | 410) {
        return CampaignsSnapshot {
            ok: false,
            available: false,
            show_campaign: false,
            claimable: false,
            campaign_url: String::new(),
            items: Vec::new(),
            identity: identity_source,
            machine_headers: machine_state,
            hint,
            error: Some(format!("本区域没有活动平台接口（HTTP {status}）")),
        };
    }
    if !(200..300).contains(&status) {
        return CampaignsSnapshot::failed(
            true,
            format!("活动列表失败: HTTP {status} {}", truncate(&body, 160)),
        );
    }
    let Ok(payload) = serde_json::from_str::<Value>(&body) else {
        return CampaignsSnapshot::failed(true, "活动列表响应不是 JSON".to_string());
    };
    let parsed = parse_campaigns(&payload);
    CampaignsSnapshot {
        ok: true,
        available: true,
        show_campaign: parsed.show_campaign,
        claimable: parsed.claimable,
        campaign_url: parsed.campaign_url,
        items: parsed.items,
        identity: identity_source,
        machine_headers: machine_state,
        hint,
        error: None,
    }
}

/// 一次领取的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimOutcome {
    pub ok: bool,
    /// 上游状态词（`CLAIMED` / `BLOCKED` / …）。
    pub status: String,
    /// 上游幂等重放（已领过）。
    pub replayed: bool,
    /// 服务端按「人」去重（同机其它账号本轮已领）。
    pub blocked: bool,
    pub failure_code: String,
    pub redemption_code: String,
    pub confirming: bool,
    pub amount: i64,
    pub message: String,
}

/// `POST /sash/api/v1/me/campaigns/{id}/claim`（官方幂等语义）。
pub async fn claim_campaign(
    region: QoderRegion,
    uid: &str,
    access_token: &str,
    campaign_id: &str,
) -> ClaimOutcome {
    let spec = realm_spec(region);
    let identity = native::cached_identity(region, uid, false);
    let (headers, _) = desktop_headers(region, uid, access_token, identity.as_ref());
    let path = super::PATH_CAMPAIGN_CLAIM_TEMPLATE.replace("{id}", campaign_id);
    let client = match api::shared_client() {
        Ok(client) => client,
        Err(error) => return ClaimOutcome::failed(error),
    };
    let response = client
        .post(format!("{}{}", spec.openapi_base, path))
        .timeout(Duration::from_secs(20))
        .headers(header_map(&headers))
        .json(&json!({}))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => return ClaimOutcome::failed(format!("领取请求失败: {error}")),
    };
    let status_code = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if status_code == 409 {
        return ClaimOutcome {
            ok: true,
            status: "CLAIMED".into(),
            replayed: true,
            blocked: false,
            failure_code: String::new(),
            redemption_code: String::new(),
            confirming: false,
            amount: 0,
            message: "今日已领取（上游幂等确认）".into(),
        };
    }
    if !(200..300).contains(&status_code) {
        let code = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("errorCode")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if code.eq_ignore_ascii_case("ALREADY_CLAIMED") || code.eq_ignore_ascii_case("REPLAYED") {
            return ClaimOutcome {
                ok: true,
                status: "CLAIMED".into(),
                replayed: true,
                blocked: false,
                failure_code: code,
                redemption_code: String::new(),
                confirming: false,
                amount: 0,
                message: "今日已领取（上游幂等确认）".into(),
            };
        }
        return ClaimOutcome::failed(format!(
            "领取失败: HTTP {status_code} {}",
            truncate(&body, 160)
        ));
    }
    let Ok(payload) = serde_json::from_str::<Value>(&body) else {
        return ClaimOutcome::failed("领取响应不是 JSON".to_string());
    };
    interpret_claim(&payload)
}

/// 把 claim 响应翻译成结果（纯函数，可测）。
///
/// 三种「不算失败」的形态都必须如实归类，不能一律报错：
/// ① 幂等重放（`replayed`）⇒ 已领；② 同人已领（`BLOCKED` / `SAME_PERSON_ALREADY_CLAIMED`）
/// ⇒ 冷却；③ 名额发完 / 任务未完成等已知失败码 ⇒ 带中文原因的**非致命**结果。
pub fn interpret_claim(payload: &Value) -> ClaimOutcome {
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let failure_code = payload
        .get("failureCode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let amount = payload
        .get("benefit")
        .and_then(Value::as_object)
        .and_then(|benefit| benefit.get("amount"))
        .and_then(api::as_num)
        .or_else(|| payload.get("amount").and_then(api::as_num))
        .unwrap_or(0);
    let redemption_code = payload
        .get("redemptionCode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    if failure_code == "SAME_PERSON_ALREADY_CLAIMED" || status == "BLOCKED" {
        return ClaimOutcome {
            ok: false,
            status: if status.is_empty() {
                "BLOCKED".into()
            } else {
                status
            },
            replayed: false,
            blocked: true,
            failure_code: if failure_code.is_empty() {
                "BLOCKED".into()
            } else {
                failure_code
            },
            redemption_code,
            confirming: false,
            amount,
            message: "同人已领取（同一设备 / 身份下的其他账号本轮已领，服务端按人去重）".into(),
        };
    }
    // 上游把「失败」放在 body 里（`success=false`）而不是 HTTP 状态。
    if status.is_empty() && payload.get("success") == Some(&Value::Bool(false)) {
        return ClaimOutcome::failed(
            payload
                .get("error")
                .map(|error| truncate(&error.to_string(), 160))
                .unwrap_or_else(|| truncate(&payload.to_string(), 160)),
        );
    }
    if let Some(reason) = failure_reason(&failure_code) {
        return ClaimOutcome {
            ok: false,
            status: if status.is_empty() {
                "NOT_ELIGIBLE".into()
            } else {
                status
            },
            replayed: false,
            blocked: false,
            failure_code,
            redemption_code,
            confirming: false,
            amount,
            message: reason.to_string(),
        };
    }
    let replayed = payload
        .get("replayed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let granted = matches!(status.as_str(), "CLAIMED" | "GRANTED" | "SUCCESS");
    let confirming = status == "CLAIMED" && redemption_code.is_empty();
    let message = if replayed {
        "今日已领取".to_string()
    } else if granted {
        if redemption_code.is_empty() {
            "领取成功（兑换码发放确认中）".to_string()
        } else {
            format!("领取成功，兑换码：{redemption_code}")
        }
    } else {
        format!(
            "上游状态：{}",
            if status.is_empty() { "未知" } else { &status }
        )
    };
    ClaimOutcome {
        ok: granted,
        status,
        replayed,
        blocked: false,
        failure_code,
        redemption_code,
        confirming,
        amount,
        message,
    }
}

impl ClaimOutcome {
    fn failed(error: String) -> Self {
        Self {
            ok: false,
            status: String::new(),
            replayed: false,
            blocked: false,
            failure_code: String::new(),
            redemption_code: String::new(),
            confirming: false,
            amount: 0,
            message: error,
        }
    }
}

/// 把头部数组转成 `reqwest::header::HeaderMap`（非 ASCII / 非法头名静默跳过）。
fn header_map(headers: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            map.insert(name, value);
        }
    }
    map
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

// ---------------------------------------------------------------------------
// 编排：查询 / 领取
// ---------------------------------------------------------------------------

/// 活动通道的 `ok` 判据（**纯函数**，可测）：「真的发生了什么」才算成功。
///
/// `has_benefit == false`（列表里一条积分类活动都没有）⇒ **成功不成立**。
/// 这条是 issue #46-D / #58 §6 的红线：写成 `true` 会让界面显示「N 个成功」
/// 而额度纹丝不动，用户于是反复觉得「签到不能用」。
pub fn checkin_ok(errors: &[String], has_benefit: bool) -> bool {
    errors.is_empty() && has_benefit
}

/// 单账号签到：领取**所有可领的积分活动**（`CREDITS` / 空 kind），并落盘状态。
///
/// 返回的 JSON 形状与前端「签到结果条」直接对应（见 `campaign_checkin` 的字段名）。
pub async fn checkin_for(region: QoderRegion, uid: &str) -> Result<Value, String> {
    let account =
        account::find_account_for(region, uid).ok_or_else(|| format!("账号不存在: {uid}"))?;
    let access = account
        .access_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "这条账号没有可调用的 access token（PAT 不能用于活动平台）".to_string())?
        .to_string();

    let snapshot = fetch_campaigns(region, uid, &access).await;
    if !snapshot.ok {
        let (next_at, next_note) = next_checkin_window(now_secs());
        return Ok(json!({
            "ok": false,
            "uid": uid,
            "source": "campaign",
            "error": snapshot.error,
            "available": snapshot.available,
            // 整区没有活动平台 ⇒ 这是「通道未开放」（第三档），不是「失败」。
            // 与 `disabled_claim_response` 同一条纪律：未开放**不得**算成功。
            "disabled": !snapshot.available,
            "claimed": [], "already": [], "blocked": [], "pending": [], "locked": [],
            "codes": [], "earned": 0,
            "nextAvailableAt": next_at,
            "nextAvailableNote": next_note,
        }));
    }

    let now = now_secs();
    // 「有没有积分类活动」决定第三档语义（issue #58 §5 / #46-D）：
    // 一条都没有 ⇒ 未开放（`ok:false` + `disabled:true`），**不得**报成功。
    let has_benefit = snapshot.items.iter().any(|item| item.is_credit_kind());
    let blocked_map: BTreeMap<String, i64> = account.campaign_blocked_until.clone();
    let mut claimed: Vec<Value> = Vec::new();
    let mut already: Vec<Value> = Vec::new();
    let mut blocked: Vec<Value> = Vec::new();
    let mut pending: Vec<Value> = Vec::new();
    let mut locked: Vec<Value> = Vec::new();
    let mut codes: Vec<(String, String)> = Vec::new();
    let mut cooldown_writes: Vec<(String, i64)> = Vec::new();
    let mut earned: i64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut stamped = false;

    for item in snapshot.items.iter().filter(|item| item.is_credit_kind()) {
        match classify(item) {
            CampaignClass::View => continue,
            CampaignClass::Already => {
                already.push(campaign_to_value(item));
                stamped = true;
                if let Some(code) = account.campaign_codes.get(&item.campaign_id) {
                    codes.push((item.campaign_id.clone(), code.clone()));
                }
                continue;
            }
            CampaignClass::Pending => {
                pending.push(campaign_to_value(item));
                continue;
            }
            CampaignClass::Locked => {
                locked.push(campaign_to_value(item));
                continue;
            }
            CampaignClass::Claimable => {}
        }
        if blocked_map
            .get(&item.campaign_id)
            .is_some_and(|until| *until > now)
        {
            blocked.push(json!({
                "campaignId": item.campaign_id,
                "title": item.label(),
                "failureCode": "SAME_PERSON_ALREADY_CLAIMED",
                "cooldown": true,
            }));
            continue;
        }
        let outcome = claim_campaign(region, uid, &access, &item.campaign_id).await;
        if outcome.ok {
            if outcome.replayed {
                already.push(campaign_to_value(item));
            } else {
                let amount = if outcome.amount > 0 {
                    outcome.amount
                } else {
                    item.benefit_amount
                };
                earned += amount;
                claimed.push(json!({
                    "campaignId": item.campaign_id,
                    "title": item.label(),
                    "amount": amount,
                    "redemptionCode": outcome.redemption_code,
                }));
            }
            stamped = true;
            if !outcome.redemption_code.is_empty() {
                codes.push((item.campaign_id.clone(), outcome.redemption_code.clone()));
            }
        } else if outcome.blocked {
            let until = now + BLOCKED_COOLDOWN_SECS;
            cooldown_writes.push((item.campaign_id.clone(), until));
            blocked.push(json!({
                "campaignId": item.campaign_id,
                "title": item.label(),
                "failureCode": outcome.failure_code,
            }));
        } else if failure_reason(&outcome.failure_code).is_some() {
            let row = campaign_to_value(item);
            match outcome.failure_code.as_str() {
                "REDEMPTION_CODE_OUT_OF_STOCK" => pending.push(row),
                "ACHIEVEMENT_NOT_COMPLETED" => locked.push(row),
                _ => errors.push(format!("{}: {}", item.label(), outcome.message)),
            }
        } else {
            errors.push(format!("{}: {}", item.label(), outcome.message));
        }
        // 相邻 claim 之间留间隔（防风控）；成功 / 被挡 / 失败都刚打过上游。
        tokio::time::sleep(CLAIM_GAP).await;
    }

    // 落盘：盖章 + 兑换码 + 冷却（一次原子写）。
    let stamp = if stamped || !claimed.is_empty() {
        Some(store::timestamp())
    } else {
        None
    };
    if let Err(error) =
        account::record_checkin_for(region, uid, stamp.as_deref(), &codes, &cooldown_writes)
    {
        // 落盘失败**不吞**：签到动作已经发生，必须让用户知道状态没记住。
        errors.push(format!("签到状态落盘失败: {error}"));
    }

    let (next_at, next_note) = next_checkin_window(now);
    let message = checkin_message(
        &claimed, &already, &blocked, &pending, &locked, &errors, earned,
    );
    Ok(json!({
        // `disabled`（列表里根本没有积分类活动）**不算成功** —— 与 de932d7 的
        // `disabled_claim_response` 同一条语义，前端按 `ok` 计数时才不会谎报「N 个成功」。
        "ok": checkin_ok(&errors, has_benefit),
        "uid": uid,
        "source": "campaign",
        "available": true,
        "disabled": !has_benefit,
        "claimed": claimed,
        // ⚠️ 键名带 `Items`：旧契约里 `already` 是**布尔**（`qoder_checkin_claim` 的
        // `QoderCheckinClaimResult.already`），两个形状同名会让前端拿到数组当布尔用。
        "alreadyItems": already,
        "blocked": blocked,
        "pending": pending,
        "locked": locked,
        "codes": codes.iter().map(|(id, code)| json!({"campaignId": id, "code": code})).collect::<Vec<_>>(),
        "earned": earned,
        "message": message,
        "errors": errors,
        "nextAvailableAt": next_at,
        "nextAvailableNote": next_note,
    }))
}

/// 汇总结论文案（与官方前端状态机一致；优先报「真的发生了什么」）。
fn checkin_message(
    claimed: &[Value],
    already: &[Value],
    blocked: &[Value],
    pending: &[Value],
    locked: &[Value],
    errors: &[String],
    earned: i64,
) -> String {
    if !claimed.is_empty() {
        return format!("活动领取成功 +{earned} Credits（{} 项）", claimed.len());
    }
    if !blocked.is_empty() {
        return "同人已领取：同一设备 / 身份下的其他账号本轮已领过（服务端按人去重）".to_string();
    }
    if !already.is_empty() {
        return format!("今日活动奖励已领取（{} 项）", already.len());
    }
    if !pending.is_empty() {
        return format!("名额已发完，次日 10:00 后可再领（{} 项）", pending.len());
    }
    if !locked.is_empty() {
        return format!("需先在官方桌面端完成新人任务后可领（{} 项）", locked.len());
    }
    if !errors.is_empty() {
        return format!("活动领取失败：{}", truncate(&errors.join("; "), 200));
    }
    "当前账号暂无可领取的官方活动".to_string()
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: &str, action: &str, status: &str, amount: i64) -> CampaignItem {
        CampaignItem {
            campaign_id: id.into(),
            campaign_key: format!("key-{id}"),
            title: format!("活动 {id}"),
            description: String::new(),
            detail_url: String::new(),
            button_text: String::new(),
            action_type: action.into(),
            claim_status: status.into(),
            benefit_kind: "CREDITS".into(),
            benefit_amount: amount,
            required_achievement_key: String::new(),
            achievement_completed: true,
            unavailable_reason: String::new(),
            start_at: 0,
            end_at: 0,
        }
    }

    /// ★ 五种分类逐一钉死（这是「领什么、报什么」的唯一判据）。
    ///
    /// 可证伪：把 `classify` 里 `CLAIMED` 分支的 `benefit_amount > 0` 去掉 ⇒
    /// `claimed_with_reward_is_already` 变红（已领的活动会被当成详情类丢掉）。
    #[test]
    fn classify_covers_all_five_states() {
        assert_eq!(
            classify(&item("a", "CLAIM_BENEFIT", "CLAIMABLE", 100)),
            CampaignClass::Claimable
        );
        assert_eq!(
            classify(&item("b", "CLAIM_BENEFIT", "CLAIMED", 100)),
            CampaignClass::Already
        );
        // 详情类：没有奖励、也不是可领类 ⇒ 不算「已领取」。
        assert_eq!(
            classify(&item("c", "VIEW_DETAILS", "CLAIMED", 0)),
            CampaignClass::View
        );
        assert_eq!(
            classify(&item("d", "VIEW_DETAILS", "CLAIMABLE", 100)),
            CampaignClass::View
        );

        let mut out_of_stock = item("e", "CLAIM_BENEFIT", "NOT_CLAIMABLE", 100);
        out_of_stock.unavailable_reason = "REDEMPTION_CODE_OUT_OF_STOCK".into();
        assert_eq!(classify(&out_of_stock), CampaignClass::Pending);

        let mut locked = item("f", "CLAIM_BENEFIT", "NOT_CLAIMABLE", 100);
        locked.unavailable_reason = "ACHIEVEMENT_NOT_COMPLETED".into();
        assert_eq!(classify(&locked), CampaignClass::Locked);

        // 无 unavailableReason 但 achievementCompleted=false ⇒ 也是被锁。
        let mut locked2 = item("g", "CLAIM_BENEFIT", "NOT_CLAIMABLE", 100);
        locked2.achievement_completed = false;
        assert_eq!(classify(&locked2), CampaignClass::Locked);
    }

    /// ★ 领取后 `actionType` 被官方降级成 `VIEW_DETAILS`，但**仍必须归成「已领」**。
    ///
    /// 这是「归类先于 action_type 过滤」那条顺序的可证伪用例：把 Already 分支挪到
    /// 过滤之后，本用例立即变红（真机上表现为「明明领过却报今天没活动」）。
    #[test]
    fn claimed_then_downgraded_to_view_is_still_already() {
        // amount > 0 ⇒ 即使 actionType 降级也认「已领」。
        assert_eq!(
            classify(&item("x", "VIEW_DETAILS", "CLAIMED", 100)),
            CampaignClass::Already
        );
        // amount 缺失但 actionType 仍是可领类（空）⇒ 也认「已领」。
        assert_eq!(
            classify(&item("y", "", "CLAIMED", 0)),
            CampaignClass::Already
        );
    }

    /// 活动列表解析：中文标题优先、逐字段抽取、缺 id 的条目丢弃。
    #[test]
    fn parse_campaigns_extracts_placements_and_skips_anonymous_rows() {
        let payload = json!({
            "showCampaign": true,
            "claimable": true,
            "campaignUrl": "https://qoder.com.cn/activity",
            "campaigns": [
                {
                    "campaignId": "act-1",
                    "campaignKey": "daily",
                    "actionType": "CLAIM_BENEFIT",
                    "claimStatus": "CLAIMABLE",
                    "benefit": {"kind": "CREDITS", "amount": 100.0},
                    "placements": [
                        {"content": {"zh": {"title": "每日领取", "description": "每天 100", "detailUrl": "https://x", "buttonText": "领取"}}}
                    ]
                },
                {"note": "既无 campaignId 也无 campaignKey —— 必须丢弃"},
                {"campaignKey": "key-only"},
                {"campaignId": "act-2", "claimStatus": "claimed", "actionType": "VIEW_DETAILS"}
            ]
        });
        let parsed = parse_campaigns(&payload);
        assert!(parsed.show_campaign);
        assert_eq!(parsed.campaign_url, "https://qoder.com.cn/activity");
        assert_eq!(parsed.items.len(), 3, "既无 id 也无 key 的条目必须被丢弃");
        assert_eq!(parsed.items[0].title, "每日领取");
        assert_eq!(parsed.items[0].benefit_amount, 100, "浮点额度必须被读出来");
        assert_eq!(parsed.items[0].button_text, "领取");
        // claimStatus 归一成大写（上游小写形态也要能判）。
        assert_eq!(parsed.items[2].claim_status, "CLAIMED");
        // 只有 campaignKey 的条目要保留（id 与 key 至少有一个即可）。
        assert_eq!(parsed.items[1].label(), "key-only");
    }

    /// 空 / 缺字段的响应 ⇒ 空表 + 保守默认，**不 panic、不猜**。
    #[test]
    fn parse_campaigns_is_defensive() {
        let empty = parse_campaigns(&json!({}));
        assert!(!empty.show_campaign);
        assert!(!empty.claimable);
        assert!(empty.items.is_empty());
        assert!(empty.campaign_url.is_empty());
        // `campaigns` 不是数组（服务端异常形态）⇒ 空表而不是崩。
        let weird = parse_campaigns(&json!({"campaigns": "nope"}));
        assert!(weird.items.is_empty());
    }

    /// ★ claim 响应的三种「非失败」形态各自归类，且**都带可读中文**。
    ///
    /// 可证伪：把 `SAME_PERSON_ALREADY_CLAIMED` 分支删掉 ⇒ 第二条断言变红
    /// （同人已领会被当成「领取失败」，用户看到的是假故障）。
    #[test]
    fn interpret_claim_classifies_idempotent_blocked_and_out_of_stock() {
        let replayed = interpret_claim(&json!({"status": "CLAIMED", "replayed": true}));
        assert!(replayed.ok && replayed.replayed);
        assert_eq!(replayed.message, "今日已领取");

        let blocked = interpret_claim(&json!({
            "status": "BLOCKED",
            "failureCode": "SAME_PERSON_ALREADY_CLAIMED",
            "benefit": {"amount": 100}
        }));
        assert!(!blocked.ok && blocked.blocked);
        assert_eq!(blocked.failure_code, "SAME_PERSON_ALREADY_CLAIMED");
        assert!(
            blocked.message.contains("同人已领取"),
            "{}",
            blocked.message
        );

        let out = interpret_claim(&json!({
            "status": "NOT_ELIGIBLE",
            "failureCode": "REDEMPTION_CODE_OUT_OF_STOCK"
        }));
        assert!(!out.ok && !out.blocked);
        assert!(out.message.contains("名额已发完"), "{}", out.message);

        let ok = interpret_claim(&json!({
            "status": "CLAIMED",
            "benefit": {"amount": 100},
            "redemptionCode": "ABC-123"
        }));
        assert!(ok.ok && !ok.confirming);
        assert_eq!(ok.amount, 100);
        assert!(ok.message.contains("ABC-123"), "{}", ok.message);

        // CLAIMED 但没有兑换码 ⇒ 「发放确认中」（不是失败）。
        let confirming = interpret_claim(&json!({"status": "CLAIMED"}));
        assert!(confirming.ok && confirming.confirming);
        assert!(confirming.message.contains("确认中"));

        // 未知失败码 ⇒ 原样透出，不编中文。
        assert_eq!(failure_reason("SOMETHING_NEW"), None);
    }

    /// ★ 桌面端头：无原生身份时**一个 `cosy-machine*` 都不发**（issue #10）。
    ///
    /// 可证伪：把 `None` 分支改成「发派生假头」⇒ 本用例变红
    /// （真机上会让服务端把 CLAIMABLE 活动整条过滤掉）。
    #[test]
    fn desktop_headers_omit_machine_headers_without_native_identity() {
        let (without, state) = desktop_headers(QoderRegion::Cn, "uid-1", "dt-token", None);
        assert_eq!(state, MACHINE_HEADERS_OMITTED);
        assert!(
            !without
                .iter()
                .any(|(name, _)| name.starts_with("cosy-machine")),
            "无原生身份时不得发机器头：{without:?}"
        );
        // 但「活动平台必需」的三件必须始终在。
        for required in ["cosy-clienttype", "cosy-version", "User-Agent"] {
            assert!(
                without.iter().any(|(name, _)| name == required),
                "缺必需头 {required}"
            );
        }
        assert!(without
            .iter()
            .any(|(name, value)| name == "User-Agent" && value == "Qoder"));
        assert!(without
            .iter()
            .any(|(name, value)| name == "cosy-clienttype" && value == DESKTOP_CLIENT_TYPE));

        let identity = NativeIdentity {
            machine_token: "tok".into(),
            machine_type: "typ".into(),
            machine_code: "cod".into(),
        };
        let (with, state) = desktop_headers(QoderRegion::Cn, "uid-1", "dt-token", Some(&identity));
        assert_eq!(state, MACHINE_HEADERS_NATIVE);
        for required in [
            "cosy-machineid",
            "cosy-machinetoken",
            "cosy-machinetype",
            "cosy-machinecode",
            "cosy-machineos",
            "cosy-machinehostname",
        ] {
            assert!(
                with.iter().any(|(name, _)| name == required),
                "原生身份可用时必须发 {required}"
            );
        }
    }

    /// ★ 「下一次签到窗口」= 下一个 UTC+8 的 10:00，且**跨日边界正确**。
    ///
    /// 可证伪：把 `if local >= target` 的 `>=` 改成 `>` ⇒ 恰好 10:00 那一秒会算成
    /// 「刚刚过去」而不是「就是现在」，第二条断言变红。
    #[test]
    fn next_checkin_window_is_next_1000_utc8() {
        let at = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .expect("测试用时刻必须可解析")
                .timestamp()
        };
        // 09:00（UTC+8）⇒ 同一天的 10:00（差 1 小时）。
        let nine = at("2026-10-16T09:00:00+08:00");
        let (next, text) = next_checkin_window(nine);
        assert_eq!(next - nine, 3600, "09:00 之后应是同一天的 10:00");
        assert!(!text.is_empty(), "必须同时给出可读文本");

        // 10:00 整 ⇒ 顺延到**明天**（不是「现在」）。
        let ten = at("2026-10-16T10:00:00+08:00");
        let (next_ten, _) = next_checkin_window(ten);
        assert_eq!(next_ten - ten, 86_400, "10:00 整应顺延到明天");

        // 10:00 之后 1 秒 ⇒ 也是明天。
        let (after, _) = next_checkin_window(ten + 1);
        assert_eq!(after - (ten + 1), 86_400 - 1);

        // 跨日：23:59 ⇒ 明天的 10:00（差 10 小时 1 分）。
        let late = at("2026-10-16T23:59:00+08:00");
        let (next_late, _) = next_checkin_window(late);
        assert_eq!(next_late - late, 10 * 3600 + 60);
    }

    /// `is_credit_kind`：只领积分类（空 kind 视为积分类 —— 老活动不写 kind）。
    #[test]
    fn credit_kind_filter() {
        let mut credit = item("a", "CLAIM_BENEFIT", "CLAIMABLE", 100);
        assert!(credit.is_credit_kind());
        credit.benefit_kind = String::new();
        assert!(credit.is_credit_kind(), "空 kind 视为积分类");
        credit.benefit_kind = "REDEMPTION_CODE".into();
        assert!(!credit.is_credit_kind(), "券类不参与签到领取");
        credit.benefit_kind = "credits".into();
        assert!(credit.is_credit_kind(), "kind 大小写不敏感");
    }
    /// ★ 红线（issue #46-D / #58 §6）：**未开放不得算成功**。
    ///
    /// 可证伪：把 `checkin_ok` 改成只看 `errors.is_empty()` ⇒ 本条立即变红，
    /// 而界面上会重新出现「N 个成功、额度纹丝不动」。
    #[test]
    fn checkin_ok_never_reports_success_without_an_activity() {
        assert!(
            !checkin_ok(&[], false),
            "没有任何积分类活动 ⇒ 未开放，不是成功"
        );
        assert!(checkin_ok(&[], true), "有活动且无报错 ⇒ 成功");
        assert!(
            !checkin_ok(&["上游 500".to_string()], true),
            "有报错 ⇒ 失败"
        );
        assert!(
            !checkin_ok(&["上游 500".to_string()], false),
            "既无活动又报错 ⇒ 失败"
        );
    }

    // ── 真机探针（issue #58 第 0 步；本机无账号，刻意保留位）──
    //
    // 跑法：
    //   QODER_LIVE_UID=… QODER_LIVE_ACCESS_TOKEN=… \
    //     cargo test -p buddy-switch-core --lib live_campaigns_probe -- --ignored --nocapture
    //
    // 要记录的三件事（决定第 2 阶段是否必做）：
    //   P1 基础头能否拿到活动列表；P2 加客户端头后条目是否变多；
    //   P3 有原生 `runtime-info` 身份时国际版是否出现 `CLAIMABLE` 的 `CLAIM_BENEFIT`。

    fn live_account() -> (String, String) {
        (
            std::env::var("QODER_LIVE_UID").expect("需 QODER_LIVE_UID"),
            std::env::var("QODER_LIVE_ACCESS_TOKEN").expect("需 QODER_LIVE_ACCESS_TOKEN"),
        )
    }

    #[tokio::test]
    #[ignore = "真机探针：需 QODER_LIVE_UID / QODER_LIVE_ACCESS_TOKEN（本机无账号）"]
    async fn live_campaigns_probe() {
        let (uid, token) = live_account();
        let region = match std::env::var("QODER_LIVE_REGION").as_deref() {
            Ok("global") => QoderRegion::Global,
            _ => QoderRegion::Cn,
        };
        let snapshot = fetch_campaigns(region, &uid, &token).await;
        println!(
            "ok={} available={} showCampaign={} claimable={} identity={} headers={} hint={} error={:?}",
            snapshot.ok,
            snapshot.available,
            snapshot.show_campaign,
            snapshot.claimable,
            snapshot.identity,
            snapshot.machine_headers,
            snapshot.hint,
            snapshot.error
        );
        for item in &snapshot.items {
            println!(
                "- {} | class={:?} | action={} | claim={} | kind={} | amount={}",
                item.label(),
                classify(item),
                item.action_type,
                item.claim_status,
                item.benefit_kind,
                item.benefit_amount
            );
        }
        assert!(
            snapshot.available,
            "本区域应有活动平台接口（404/405/410 才算没有）"
        );
    }
}
