//! Qoder **网页端**额度明细（issue #111）——逐资源包（积分包）的唯一数据源。
//!
//! ## 为什么单独一个模块
//!
//! 本仓既有的额度通道是 openapi（`Authorization: Bearer dt-…`），它只给三个聚合标量。
//! 逐包数据在**网页端私有接口** `/api/v2/me/usages/big_model_credits`，且该接口
//! **只认浏览器登录后的会话 Cookie**——openapi token 一律被拒（实测 401）。
//!
//! 2026-10-27 真机探针（本机 CN 账号，`GET {openapi_base}/api/v2/quota/usage`）确认：
//! 响应只有 `userQuota` / `addOnQuota` 两个标量块 + `outerProviders: []`（空数组），
//! **`raw` 里没有任何逐包数组**。所以「前端解析 `raw`」这条捷径不成立，必须另建会话链路。
//!
//! ## 本模块的边界
//!
//! - **只做协议**：拼 URL、发请求、校验归属、给出响应形状。**不碰 Cookie 的获取与落盘**
//!   ——那是宿主（`src-tauri/src/qoder_web.rs`）的职责，因为只有 Tauri 的
//!   `Webview::cookies()` 能拿到 httpOnly Cookie，而 core 刻意不依赖 Tauri。
//! - **不返回 Cookie 明文**：调用方传进来的是 `Cookie:` 头字符串，本模块从不把它写进
//!   返回值、日志或错误消息（与「会话 Cookie 是最高等级凭据」的纪律一致）。
//! - **失败即 `Err`，不写 0**：沿用 [`super::api::parse_quota`] 的同款纪律 ——
//!   「不知道」不能显示成「没有」。

use serde_json::Value;

use super::api;
use super::region::{self, QoderRegion};
use super::web_proxy;

/// 网页端逐资源包明细（`/api/v2/me/usages/big_model_credits`）。
///
/// ⚠️ 与 [`super::PATH_QUOTA_USAGE`]（openapi，`/api/v2/quota/usage`）是**两条独立通道**：
/// 前者认会话 Cookie，后者认 Bearer token。别把路径拼到对方的基址上。
pub const PATH_WEB_USAGE_CREDITS: &str = "/api/v2/me/usages/big_model_credits";

/// 网页端「用量明细」页——登录窗载它，同时也是「形态不支持」时的兜底外链。
///
/// 该 URL 不是猜的：`/api/v2/quota/usage` 的 `addOnQuota.detailUrl` 字段就指向它
/// （2026-10-27 实测 `https://qoder.com.cn/account/usage`）。
pub fn usage_page_url(region: QoderRegion) -> String {
    format!("{}/account/usage", region::realm_spec(region).website)
}

/// 逐包接口的完整 URL（基址是 **website**，不是 openapi_base —— 别搞混）。
pub fn credits_url(region: QoderRegion) -> String {
    format!(
        "{}{}",
        region::realm_spec(region).website,
        PATH_WEB_USAGE_CREDITS
    )
}

/// 归属校验用的字段名。
///
/// 上游把会话归属者写在 `user_id`（下划线，不是 `userId`）。两种都认，是因为
/// 逐包接口的字段风格与 openapi 的 `userId` 不同 —— 这一点来自参考实现的实测样本，
/// 本仓尚未拿到自己的样本（阶段零-2 待补）。**认两种只是容错，不是推测正确性**：
/// 两个字段都取不到时 [`owner_matches`] 返回 `false`（fail-closed）。
const OWNER_KEYS: [&str; 2] = ["user_id", "userId"];

/// 响应形状摘要（**只含键名与计数，不含额度数值与任何凭据**）。
///
/// 用途：阶段零-2 取证。把真实响应打回来时，人要看的是「有几个分组、每组几行、
/// 每行有哪些键、`expires_at` 是毫秒还是秒」，而不是一串数字。
pub fn shape_of(raw: &Value) -> Value {
    let mut groups = Vec::new();
    if let Some(map) = raw.as_object() {
        // ★ 只枚举 `PACK_GROUPS`：`total_quota` 同样带 `quota_detail`（它是汇总），
        //   一并报出来会让「有几个分组」看起来多一个，也会误导解析器写成四组。
        for group in PACK_GROUPS {
            let Some(section) = map.get(group) else {
                continue;
            };
            match section.get("quota_detail") {
                Some(Value::Array(rows)) => {
                    let keys: Vec<String> = rows
                        .first()
                        .and_then(|row| row.as_object())
                        .map(|obj| obj.keys().cloned().collect())
                        .unwrap_or_default();
                    groups.push(serde_json::json!({
                        "group": group,
                        "rows": rows.len(),
                        "rowKeys": keys,
                    }));
                }
                Some(Value::Null) => {
                    groups.push(serde_json::json!({
                        "group": group,
                        "rows": 0,
                        "rowKeys": Vec::<String>::new(),
                        "null": true,
                    }));
                }
                _ => {}
            }
        }
    }
    // 顶层键名也一并给出：探针要确认「有没有我们没预料到的字段」。
    let top_level: Vec<&str> = raw
        .as_object()
        .map(|obj| obj.keys().map(String::as_str).collect())
        .unwrap_or_default();
    serde_json::json!({ "topLevelKeys": top_level, "groups": groups })
}

/// 会话归属者是否与本仓账号一致（**fail-closed**：取不到就判否）。
///
/// 为什么必须有这道防线：会话 Cookie 是浏览器级的，用户可能在登录窗里登了另一个
/// 账号。若不做比对，就会把**别人的**资源包显示到当前账号上 —— 这比「没有明细」坏得多。
pub fn owner_matches(raw: &Value, expected_uid: &str) -> bool {
    if expected_uid.trim().is_empty() {
        return false;
    }
    let actual = OWNER_KEYS
        .iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_str))
        .unwrap_or_default();
    // 大小写不敏感：uid 是 uuid，上游大小写形态未见统一。
    !actual.is_empty() && actual.eq_ignore_ascii_case(expected_uid.trim())
}

/// 取数失败，并区分「会话被上游拒绝」与「其它错误」。
///
/// 为什么必须分开：上游 401/403 意味着**这份 Cookie 已经死了**，此时本地缓存的会话
/// 必须立刻清除并引导用户重新登录；而网络超时 / 502 之类**不该**把用户的登录态抹掉
/// —— 混在一起会让「网抖了一下」变成「你的登录没了」，且是无谓的重新登录。
pub struct WebUsageError {
    pub message: String,
    /// `true` = 上游明确拒绝（401 / 403）。
    pub unauthorized: bool,
}

impl WebUsageError {
    fn other(message: String) -> Self {
        Self {
            message,
            unauthorized: false,
        }
    }
}

impl std::fmt::Display for WebUsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// 用会话 Cookie 取逐包明细的**原始响应**。
///
/// `cookie_header` 是 `k=v; k2=v2` 形状（由宿主从 `Webview::cookies()` 拼出）。
/// 返回 `Ok(raw)` 只代表「HTTP 200 且是 JSON」；归属校验由调用方用
/// [`owner_matches`] 单独做——因为「能读到」与「读的是不是这个账号」是两件事，
/// 混在一起会让「归属不符」与「请求失败」无法区分。
pub async fn fetch_credit_packs_raw(
    region: QoderRegion,
    cookie_header: &str,
) -> Result<Value, WebUsageError> {
    if cookie_header.trim().is_empty() {
        return Err(WebUsageError::other(
            "没有可用的网页会话 Cookie（请先登录 Qoder 网页端）".to_string(),
        ));
    }
    let spec = region::realm_spec(region);
    // ★ 出站代理（issue #111 收尾）：国际版网页端接口按 IP 判地区，国内直连会被拦；
    //    这里按区域读 `qoder/proxy.json`，留空 = 直连（共享客户端）。
    let client = web_proxy::client_for(region).map_err(WebUsageError::other)?;
    let response = client
        .get(credits_url(region))
        .timeout(std::time::Duration::from_secs(20))
        .header("Accept", "application/json, text/plain, */*")
        .header("User-Agent", spec.user_agent)
        .header("Origin", spec.website)
        .header("Referer", format!("{}/account/usage", spec.website))
        .header("Cookie", cookie_header)
        .send()
        .await
        .map_err(|error| WebUsageError::other(format!("网页端额度明细请求失败: {error}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let code = status.as_u16();
    // 401 / 403 = 会话被上游明确拒绝 ⇒ 缓存该作废。
    let unauthorized = matches!(code, 401 | 403);
    if !status.is_success() {
        // ⚠️ 只回状态码与截断后的响应体，**不回显 Cookie**。
        let hint = if unauthorized {
            "（网页会话已失效，需要重新登录）"
        } else {
            ""
        };
        return Err(WebUsageError {
            message: format!(
                "网页端额度明细请求失败: HTTP {code} {hint} {}",
                body.chars().take(120).collect::<String>()
            ),
            unauthorized,
        });
    }
    serde_json::from_str(&body)
        .map_err(|error| WebUsageError::other(format!("网页端额度明细响应不是 JSON: {error}")))
}

// ---------------------------------------------------------------------------
// 逐包解析（S3）
// ---------------------------------------------------------------------------

/// 响应里的三个分组键（`total_quota` 是汇总，**不是**分组 —— 别把它也解析成包，
/// 否则每个包都会出现两次）。
pub const PACK_GROUPS: [&str; 3] = [
    "plan_quota",
    "resource_package_quota",
    "dedicated_resource_package_quota",
];

/// 一个资源包（积分包）。
///
/// 字段名与上游**逐字对应**（snake_case）—— 2026-10-27 真机取证，见本模块测试里的
/// 真实样本。⚠️ 参考实现 CreditDaddy 用的是 camelCase 且 `source` 取值为 `BONUS`，
/// **与真实响应不符**；照它写会得到「字段全空、source 永不命中」的解析器。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPack {
    /// 所属分组（`PACK_GROUPS` 之一）。
    pub group: String,
    /// 包 id（上游没有时为 `None`）。
    pub id: Option<String>,
    /// 来源（`PLAN` / `RESOURCE_PACKAGE_SOURCE_BONUS` / …）。
    pub source: Option<String>,
    /// 状态（`ACTIVE` / `EXPIRED` / …）。
    pub status: Option<String>,
    /// 总额度。
    pub limit: i64,
    /// 已用额度。
    pub used: i64,
    /// 剩余额度。
    pub remaining: i64,
    /// 单位（`credits`）。
    pub unit: Option<String>,
    /// 使用比例（0–100）。
    pub usage_percentage: f64,
    /// 是否活跃。
    pub is_active: bool,
    /// 到期时间（**毫秒**）。`None` = 无到期时间（上游 `expires_at = 0` 或字段缺失）。
    pub expires_at_ms: Option<i64>,
}

/// 到期时间的**下限合理性**：小于这个数的毫秒时间戳一律按**秒**解释。
///
/// 为什么需要它：`expires_at` 的单位本仓没有上游文档可依（issue #51 的 8 年偏差就是
/// 单位取错造成的）。真机样本是毫秒（`1795595404000` ≈ 2026-11），而万一上游哪天改成秒，
/// 静默显示成 1970 比「多一步归一化」坏得多。
/// 1e12 ms ≈ 2001-09-09 —— 任何真实到期时间都远在它之后。
const EXPIRY_MS_PLAUSIBLE_MIN: i64 = 1_000_000_000_000;

/// 到期时间的**上限合理性**：超过 2100 年的视为「不可信」⇒ `None`。
///
/// 上游用哨兵值表示「永不过期」：openapi 侧见过 `253402214400000`（= 9999-12-31）。
/// 把哨兵值当真实到期时间渲染出来，正是 issue #51 那一课（8 年偏差的同类事故）。
/// 2100-01-01 = 4102444800000 —— 任何真实的积分包到期时间都远在它之前。
const EXPIRY_MS_PLAUSIBLE_MAX: i64 = 4_102_444_800_000;

/// 归一化上游的到期时间：`0` / 缺失 / 不可信 ⇒ `None`；秒 ⇒ 毫秒。
pub fn normalize_expires_at_ms(raw: i64) -> Option<i64> {
    if raw <= 0 {
        return None;
    }
    let ms = if raw < EXPIRY_MS_PLAUSIBLE_MIN {
        raw * 1000
    } else {
        raw
    };
    if ms > EXPIRY_MS_PLAUSIBLE_MAX {
        return None;
    }
    Some(ms)
}

fn pack_str(row: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_string)
}

fn pack_num(row: &serde_json::Map<String, Value>, key: &str) -> i64 {
    row.get(key).and_then(api::as_num).unwrap_or(0)
}

/// 解析逐包明细。
///
/// 三条纪律：
/// 1. **`quota_detail` 为 `null` 是合法形态**（真样本里 `dedicated_resource_package_quota`
///    就是 null）⇒ 跳过，不当错误、也不造空包；
/// 2. **不写 0 冒充未知**：字段缺失时该字段给 0，但 `id` / `source` / `status` 保持 `None`
///    —— 让上层能区分「这个包叫 X」与「不知道这个包叫什么」；
/// 3. 空 / 全错 ⇒ 空 `Vec`，调用方据此回落聚合三标量（与 `parse_quota` 同款纪律）。
pub fn parse_credit_packs(raw: &Value) -> Vec<QuotaPack> {
    let mut packs = Vec::new();
    let Some(map) = raw.as_object() else {
        return packs;
    };
    for group in PACK_GROUPS {
        let Some(section) = map.get(group) else {
            continue;
        };
        let Some(rows) = section.get("quota_detail").and_then(Value::as_array) else {
            // null / 缺失 / 非数组：都是「这个分组没有包」的合法表达。
            continue;
        };
        for row in rows {
            let Some(row) = row.as_object() else {
                continue;
            };
            packs.push(QuotaPack {
                group: group.to_string(),
                id: pack_str(row, "id"),
                source: pack_str(row, "source"),
                status: pack_str(row, "status"),
                limit: pack_num(row, "limit_value"),
                used: pack_num(row, "used_value"),
                remaining: pack_num(row, "remaining_value"),
                unit: pack_str(row, "unit"),
                usage_percentage: row
                    .get("usage_percentage")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                is_active: row
                    .get("is_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                expires_at_ms: row
                    .get("expires_at")
                    .and_then(api::as_num)
                    .and_then(normalize_expires_at_ms),
            });
        }
    }
    packs
}

/// 逐包汇总（前端「N 个包，共剩 X」那一行用）。
///
/// ★ 只做**求和**，不做「该显示几个」的判断：`PLAN` 组那一条在真机上 `limit_value = 0`
/// （套餐额度单独计量），把它算进「包总数」会让「我有 7 个包」变成 8 个。
/// 所以 `count` 只数**有额度的包**（`limit > 0`），零额度条目单独计 `zeroCount`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    /// 有额度的包数量（`limit > 0`）。
    pub count: usize,
    /// 额度为 0 的条目数量（套餐占位这一类）。
    pub zero_count: usize,
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
}

/// 汇总逐包。
pub fn summarize(packs: &[QuotaPack]) -> PackSummary {
    let mut summary = PackSummary {
        count: 0,
        zero_count: 0,
        limit: 0,
        used: 0,
        remaining: 0,
    };
    for pack in packs {
        if pack.limit > 0 {
            summary.count += 1;
            summary.limit += pack.limit;
            summary.used += pack.used;
            summary.remaining += pack.remaining;
        } else {
            summary.zero_count += 1;
        }
    }
    summary
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 2026-10-27 真机样本的**节选**（`.tmp/big_model_credits.cn.json`）。
    ///
    /// ⚠️ 这是**真实响应**，不是编的 fixture。上一版 PRD 里的 camelCase / `BONUS`
    /// 来自参考实现 CreditDaddy，与真实响应不符；fixture 必须锚真实数据。
    fn real_sample() -> Value {
        json!({
            "user_id": "019f0c1d-adfe-7a60-8634-2d24817bc03f",
            "quota_key": "big_model_credits",
            "status": "active",
            "plan_quota": {
                "quota_summary": {
                    "used_value": 0, "limit_value": 0, "remaining_value": 0,
                    "usage_percentage": 0, "unit": "credits"
                },
                "quota_detail": [
                    {
                        "id": "019f0c1d-ae00-718a-9a2f-13ec242c60a7",
                        "limit_value": 0, "used_value": 0, "remaining_value": 0,
                        "unit": "credits", "is_active": true, "usage_percentage": 0,
                        "expires_at": 0, "source": "PLAN", "status": "ACTIVE"
                    }
                ]
            },
            "resource_package_quota": {
                "quota_summary": {
                    "used_value": 0, "limit_value": 600, "remaining_value": 600,
                    "usage_percentage": 0, "unit": "credits"
                },
                "quota_detail": [
                    {
                        "id": "01a17754-bfc5-725e-8882-a303fd07e715",
                        "limit_value": 100, "used_value": 0, "remaining_value": 100,
                        "unit": "credits", "is_active": true, "usage_percentage": 0,
                        "expires_at": 1795595404000i64,
                        "source": "RESOURCE_PACKAGE_SOURCE_BONUS", "status": "ACTIVE"
                    },
                    {
                        "id": "01a17754-bfc5-725e-8882-a303fd07e716",
                        "limit_value": 200.0, "used_value": 12.5, "remaining_value": 187.5,
                        "unit": "credits", "is_active": true, "usage_percentage": 6.25,
                        "expires_at": 1795595404i64,
                        "source": "RESOURCE_PACKAGE_SOURCE_BONUS", "status": "ACTIVE"
                    },
                    {
                        "id": "01a17754-bfc5-725e-8882-a303fd07e717",
                        "limit_value": 300, "used_value": 300, "remaining_value": 0,
                        "unit": "credits", "is_active": false, "usage_percentage": 100,
                        "expires_at": 0,
                        "source": "RESOURCE_PACKAGE_SOURCE_BONUS", "status": "EXPIRED"
                    }
                ]
            },
            "dedicated_resource_package_quota": {
                "quota_summary": {
                    "used_value": 0, "limit_value": 0, "remaining_value": 0,
                    "usage_percentage": 0, "unit": "credits"
                },
                "quota_detail": null
            },
            "total_quota": {
                "quota_summary": {
                    "used_value": 0, "limit_value": 600, "remaining_value": 600,
                    "usage_percentage": 0, "unit": "credits"
                },
                "quota_detail": [
                    {
                        "id": "019f0c1d-ae00-718a-9a2f-13ec242c60a7",
                        "limit_value": 0, "used_value": 0, "remaining_value": 0,
                        "unit": "credits", "is_active": true, "usage_percentage": 0,
                        "expires_at": 0, "source": "PLAN", "status": "ACTIVE"
                    }
                ]
            },
            "lastResetAt": 1782614699520i64,
            "nextResetAt": 1785206867828i64
        })
    }

    #[test]
    fn urls_use_website_base_not_openapi() {
        // 证伪点：把 `credits_url` 的基址写成 `openapi_base`，本用例立刻变红
        // ——那会让 Bearer 通道的基址与 Cookie 通道的基址混为一谈。
        assert_eq!(
            credits_url(QoderRegion::Cn),
            "https://qoder.com.cn/api/v2/me/usages/big_model_credits"
        );
        assert_eq!(
            credits_url(QoderRegion::Global),
            "https://qoder.com/api/v2/me/usages/big_model_credits"
        );
        assert!(usage_page_url(QoderRegion::Cn).ends_with("/account/usage"));
    }

    #[test]
    fn owner_matches_is_fail_closed() {
        let raw = json!({ "user_id": "ABC-123" });
        assert!(owner_matches(&raw, "abc-123"), "大小写不敏感");
        assert!(!owner_matches(&raw, "other"), "不一致必须判否");
        assert!(!owner_matches(&raw, ""), "空 uid 必须判否");
        // 字段缺失 ⇒ 判否（不得因为「取不到」就默认通过）
        assert!(!owner_matches(&json!({}), "abc-123"));
        // openapi 风格的 camelCase 也认
        assert!(owner_matches(&json!({ "userId": "abc-123" }), "abc-123"));
    }

    #[test]
    fn shape_reports_groups_and_row_keys() {
        let shape = shape_of(&real_sample());
        let groups = shape["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 3, "三个分组都要出现在形状摘要里");
        let bonus = groups
            .iter()
            .find(|group| group["group"] == "resource_package_quota")
            .expect("资源包分组");
        assert_eq!(bonus["rows"], 3);
        let keys = bonus["rowKeys"].as_array().unwrap();
        assert!(keys.iter().any(|key| key == "expires_at"));
        // ⚠️ 摘要**不得**泄露额度数值
        assert!(!shape.to_string().contains("1795595404000"));
        assert!(!shape.to_string().contains("187"));
    }

    #[test]
    fn parses_real_sample_without_double_counting() {
        let packs = parse_credit_packs(&real_sample());
        // 1 plan + 3 resource = 4；**total_quota 不参与**（否则 plan 包会重复出现）。
        assert_eq!(packs.len(), 4, "total_quota 是汇总，不得当成分组");
        assert_eq!(packs.iter().filter(|p| p.group == "plan_quota").count(), 1);
        assert_eq!(
            packs
                .iter()
                .filter(|p| p.group == "resource_package_quota")
                .count(),
            3
        );
        // dedicated 的 quota_detail 是 null ⇒ 不产包、不 panic
        assert!(packs
            .iter()
            .all(|p| p.group != "dedicated_resource_package_quota"));
    }

    #[test]
    fn snake_case_fields_and_float_values() {
        let packs = parse_credit_packs(&real_sample());
        // 顺序固定为 PACK_GROUPS：plan(1) → resource(3)，所以浮点那条是下标 2。
        let bonus = &packs[2];
        assert_eq!(
            bonus.source.as_deref(),
            Some("RESOURCE_PACKAGE_SOURCE_BONUS")
        );
        assert_eq!(bonus.status.as_deref(), Some("ACTIVE"));
        assert_eq!(bonus.unit.as_deref(), Some("credits"));
        assert!(bonus.is_active);
        // 浮点必须被接受（`as_i64` 对 200.0 返回 None ⇒ 会显示成 0，那是谎报）
        assert_eq!(bonus.limit, 200);
        assert_eq!(bonus.used, 13, "12.5 四舍五入");
        assert_eq!(bonus.remaining, 188, "187.5 四舍五入");
        assert_eq!(bonus.usage_percentage, 6.25);
        // 秒级时间戳归一成毫秒（与毫秒样本同源）
        assert_eq!(bonus.expires_at_ms, Some(1795595404000));
    }

    #[test]
    fn expires_at_zero_means_never_and_absurd_is_distrusted() {
        let packs = parse_credit_packs(&real_sample());
        let plan = &packs[0];
        assert_eq!(plan.expires_at_ms, None, "expires_at=0 ⇒ 无到期时间");
        assert_eq!(plan.source.as_deref(), Some("PLAN"));
        // 用完/过期的包也要保留（UI 要显示「已用完」），不是过滤掉
        let expired = &packs[3];
        assert_eq!(expired.remaining, 0);
        assert!(!expired.is_active);
        assert_eq!(expired.status.as_deref(), Some("EXPIRED"));

        // ★ issue #51 的那一课：哨兵值（9999-12-31 的毫秒）不得当成真实到期时间。
        assert_eq!(normalize_expires_at_ms(253_402_214_400_000), None);
        assert_eq!(normalize_expires_at_ms(0), None);
        assert_eq!(normalize_expires_at_ms(-1), None);
        // 正常毫秒 / 正常秒
        assert_eq!(
            normalize_expires_at_ms(1_795_595_404_000),
            Some(1_795_595_404_000)
        );
        assert_eq!(
            normalize_expires_at_ms(1_795_595_404),
            Some(1_795_595_404_000)
        );
    }

    #[test]
    fn degenerate_inputs_never_panic_and_never_invent() {
        for raw in [
            json!({}),
            json!({"plan_quota": {}}),
            json!({"plan_quota": {"quota_detail": null}}),
            json!({"resource_package_quota": {"quota_detail": []}}),
            json!({"resource_package_quota": {"quota_detail": [null, 3, "x"]}}),
            json!({"resource_package_quota": {"quota_detail": [{"limit_value": 100}]}}),
            json!(null),
            json!([1, 2, 3]),
        ] {
            let packs = parse_credit_packs(&raw);
            for pack in &packs {
                // 缺字段 ⇒ 该字段 0 / None，但**不得**造出假的 id 或 source
                assert!(pack.id.is_none() || !pack.id.as_deref().unwrap().is_empty());
                assert!(pack.source.is_none());
            }
        }
        // 只有 limit 的裸包：limit 读到了，其余是 0 / None
        let packs = parse_credit_packs(&json!({
            "resource_package_quota": {"quota_detail": [{"limit_value": 100}]}
        }));
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].limit, 100);
        assert_eq!(packs[0].remaining, 0);
        assert_eq!(packs[0].expires_at_ms, None);
        assert_eq!(packs[0].unit, None);
    }
}
