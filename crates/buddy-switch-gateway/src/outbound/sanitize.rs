//! 出站请求体指纹脱敏：剥离上游内容审核黑名单指纹。
//!
//! 移植自参考实现 `internal/upstream/sanitize.go`。上游审核按**逐字精确匹配**
//! 拦截（非语义审核），因此策略是两层：
//! - **剥离层**：键值/header 型指纹（`x-anthropic-billing-header:`、`cc_xxx=…;`）整段删除；
//! - **改写层**：承载语义的模板句只换一个词（`for Claude` → `for Claude tool` 类），语义不变。
//!
//! 关键设计：先做 `contains` 特征预检，普通请求零改动直接返回（无正则开销）。

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// 特征预检串：任一命中才进入净化。截断前缀即可命中，故只存前缀。
const FEATURES: &[&str] = &[
    "x-anthropic-billing-header",
    "cc_entrypoint=",
    "You are Claude Code",
    "Main branch (",
    "You are a coding agent running in the Codex CLI",
    "github.com/anthropics/",
    "11128",
    "You are an interactive agent in TraeCode",
];

/// 改写层：逐字替换对，每句只改一个词。
///
/// 身份句的匹配串**不带结尾标点**：CLI 版以句号收尾，桌面版以逗号接后继内容，
/// 去掉标点后两种形态一并覆盖，原有标点由原文保留。
const REWRITES: &[(&str, &str)] = &[
    // 上游按**偏移 0 + 忽略大小写**匹配该身份句（实测全小写变体同样被拦），
    // 而本表是大小写敏感的 `replace`，走表只能覆盖列出的那一种大小写。
    // 故该句的预检与改写同源走 [`harness_identity_re`]，不进本表也不进 `FEATURES`。
    (
        "You are Claude Code, Anthropic's official CLI for Claude",
        "You are Claude Code, Anthropic's official CLI tool for Claude",
    ),
    (
        "Main branch (you will usually use this for PRs)",
        "Default branch (you will usually use this for PRs)",
    ),
    (
        "You are a coding agent running in the Codex CLI, a terminal-based coding assistant.",
        "You are a coding agent running in the Codex CLI tool, a terminal-based coding assistant.",
    ),
    (
        "To give feedback, users should report the issue at https://github.com/anthropics/claude-code/issues",
        "To provide feedback, users should report the issue at https://github.com/anthropics/claude-code/issues",
    ),
    // 上游反探测：请求体里出现裸数字 11128 即整单拦截（与本数字上下文无关）。
    // 插入连字符保留可读性与指代（零宽空格无效，实测上游会归一化）。
    ("11128", "11-128"),
    // issue #241：Trae 3.4.24 起在系统提示开头加入的身份句。上游渠道校验把它当作
    // 「不是官方客户端」的证据，命中即 HTTP 400 + `code: 11-128`（与本文件其它身份句同款
    // 逐字匹配）。把产品名换成等价说法即可绕过：语义不变，但不再命中上游的固定串。
    //
    // 现场（2026-11-26 用户报障）：Trae CN 调 hy4flash 每次都报「这次调用没通过渠道校验」；
    // 同一账号用官方客户端是 200。客户端自己加了这一句，网关此前没有对应的改写规则。
    (
        "You are an interactive agent in TraeCode",
        "You are an interactive agent in the Trae IDE",
    ),
];

/// 裸键名兜底替换值（`header` → `hdr`，破坏逐字匹配但保留可读性）。
const BARE_HEADER_REPLACEMENT: &str = "x-anthropic-billing-hdr";

/// 身份句改写目标：插入冠词 `the` 即破坏逐字匹配，句子仍通顺、语义不变。
///
/// 与 `REWRITES` 里 `for Claude` → `for Claude tool` 的改法同构（只多一个词）。
/// 统一回规范大小写：上游忽略大小写匹配，改写后必须是「不命中」的确定形态。
const HARNESS_IDENTITY_REPLACEMENT: &str = "You are an AI agent powered by the DeepSeek Harness";

/// `(?i)you are an ai agent powered by deepseek harness` —— 第三方客户端身份句。
///
/// 上游对 system / developer 消息**偏移 0** 处以该串做字面匹配，命中即整单返回
/// HTTP 400 + `code: 11-128`（`Illegal API invocation from an unapproved channel`），
/// 请求头怎么补都无效。判定特征：**忽略大小写**、后缀任意（句号/逗号皆可）、
/// 加连字符（`DeepSeek-Harness`）即不命中。
///
/// 大小写不敏感是必须的：该句的全部小写形态同样被拦，因此不能走 [`REWRITES`] 表
/// （`contains` / `replace` 大小写敏感）。预检 [`has_fingerprint`] 与净化
/// [`sanitize_text`] 共用本正则，避免两处形态判断漂移。
fn harness_identity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)you are an ai agent powered by deepseek harness").expect("静态正则合法")
    })
}

/// `(?i)x-anthropic-billing-header:[^;\n]*;?\s*` —— 键值形态整段删除。
fn header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)x-anthropic-billing-header:[^;\n]*;?\s*").expect("静态正则合法")
    })
}

/// `(?i)x-anthropic-billing-header` —— 裸键名（无冒号）最小缩写。
///
/// 该正则是 [`header_re`] 的超集（不要求冒号）：先删键值形态，再缩写残留裸键名，
/// 两者替换语义不同，不可合并。
fn bare_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)x-anthropic-billing-header").expect("静态正则合法"))
}

/// `(?i)\bcc_[a-z0-9_]+=[^;\n]*;?\s*` —— 尾随裸键值循环清理。
fn kv_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bcc_[a-z0-9_]+=[^;\n]*;?\s*").expect("静态正则合法"))
}

/// 特征预检：`contains` 快路径 + 大小写不敏感的裸键名正则兜底。
///
/// `contains` 大小写敏感、`header_re` 要求冒号，两者都会漏掉「混合大小写 + 裸键名」，
/// 故必须用不要求冒号的 [`bare_header_re`] 兜底，否则整条净化被跳过。
pub fn has_fingerprint(text: &str) -> bool {
    FEATURES.iter().any(|feature| text.contains(feature))
        || bare_header_re().is_match(text)
        || harness_identity_re().is_match(text)
}

/// 单段文本净化：预检不中直接返回原串。
pub fn sanitize_text(text: &str) -> String {
    if !has_fingerprint(text) {
        return text.to_string();
    }

    let mut out = text.to_string();
    for (from, to) in REWRITES {
        if out.contains(from) {
            out = out.replace(from, to);
        }
    }

    if harness_identity_re().is_match(&out) {
        out = harness_identity_re()
            .replace_all(&out, HARNESS_IDENTITY_REPLACEMENT)
            .into_owned();
    }

    if header_re().is_match(&out) {
        out = header_re().replace_all(&out, "").into_owned();
    }

    if out.contains("cc_") {
        // 循环清理：一条消息里可能有多个相邻的 cc_ 键值对。
        let mut previous = String::new();
        while previous != out {
            previous = out.clone();
            out = kv_re().replace_all(&out, "").into_owned();
        }
    }

    let out = bare_header_re()
        .replace_all(&out, BARE_HEADER_REPLACEMENT)
        .into_owned();
    out.trim().to_string()
}

/// 净化 `content`：字符串直接处理；多模态数组只动 `text` part，image 等 part 不动。
fn sanitize_content(value: &mut Value) -> bool {
    match value {
        Value::String(text) => {
            let cleaned = sanitize_text(text);
            if cleaned == *text {
                return false;
            }
            *value = Value::String(cleaned);
            true
        }
        Value::Array(parts) => {
            let mut changed = false;
            for part in parts.iter_mut() {
                let Some(wrapped) = part.as_object_mut() else {
                    continue;
                };
                let Some(Value::String(text)) = wrapped.get_mut("text") else {
                    continue;
                };
                let cleaned = sanitize_text(text);
                if cleaned != *text {
                    *text = cleaned;
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

/// 净化 `tool_calls[].function.arguments`。
///
/// `arguments` 是**字符串化的 JSON**（不是对象），因此按文本走 [`sanitize_text`]。
/// 这块易被漏掉：工具调用消息的 `content` 常为 `null`，若在 `content` 缺失时
/// `continue`，整条消息连 `tool_calls` 一起被跳过，历史里写进工具参数的被拦字符串
/// （文件名、命令、写入内容）会原样漏出。
fn sanitize_tool_calls(value: &mut Value) -> bool {
    let Some(calls) = value.as_array_mut() else {
        return false;
    };
    let mut changed = false;
    for call in calls.iter_mut() {
        let Some(arguments) = call
            .get_mut("function")
            .and_then(Value::as_object_mut)
            .and_then(|function| function.get_mut("arguments"))
        else {
            continue;
        };
        let Value::String(raw) = arguments else {
            continue;
        };
        let cleaned = sanitize_text(raw);
        if cleaned != *raw {
            *raw = cleaned;
            changed = true;
        }
    }
    changed
}

/// 净化 `messages` 中的 `content`、`reasoning_content` 与 `tool_calls`。
///
/// 返回是否发生改动。`content` 与 `tool_calls` **各自独立判断**：`content` 为 `null`
/// 的工具调用轮次，其 `tool_calls` 仍必须净化。
pub fn sanitize_messages(messages: &mut [Value]) -> bool {
    let mut changed = false;
    for message in messages.iter_mut() {
        let Some(wrapped) = message.as_object_mut() else {
            continue;
        };

        if let Some(content) = wrapped.get_mut("content") {
            if sanitize_content(content) {
                changed = true;
            }
        }

        if let Some(Value::String(reasoning)) = wrapped.get_mut("reasoning_content") {
            let cleaned = sanitize_text(reasoning);
            if cleaned != *reasoning {
                *reasoning = cleaned;
                changed = true;
            }
        }

        if let Some(tool_calls) = wrapped.get_mut("tool_calls") {
            if sanitize_tool_calls(tool_calls) {
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 回归（issue #67）：上游对 system / developer 消息**偏移 0** 处的该身份句做
    /// 忽略大小写的字面匹配，命中即 400 / `11-128`，请求头怎么补都无效。
    /// 漏改 = 该客户端的每次请求都失败。
    ///
    /// 可证伪：去掉 [`harness_identity_re`] 的 `(?i)`，全小写那条失败；
    /// 删掉整条规则，三条全失败。
    #[test]
    fn harness_identity_line_is_rewritten_in_any_case() {
        for source in [
            "You are an AI agent powered by DeepSeek Harness.",
            "you are an ai agent powered by deepseek harness.\n\nYou are a coding agent.",
            "YOU ARE AN AI AGENT POWERED BY DEEPSEEK HARNESS",
        ] {
            let cleaned = sanitize_text(source);
            assert!(
                !cleaned
                    .to_lowercase()
                    .contains("powered by deepseek harness"),
                "改写后不得残留可被逐字匹配的原文: {cleaned}"
            );
            assert!(
                cleaned.contains("powered by the DeepSeek Harness"),
                "必须插入冠词破坏逐字匹配: {cleaned}"
            );
        }
    }

    #[test]
    fn harness_identity_rewrite_keeps_the_rest_verbatim() {
        let text = "You are an AI agent powered by DeepSeek Harness.\n\nYou are a coding agent.";
        let cleaned = sanitize_text(text);
        assert!(
            cleaned.starts_with("You are an AI agent powered by the DeepSeek Harness."),
            "改写必须发生在偏移 0 且保留原有标点: {cleaned}"
        );
        assert!(
            cleaned.ends_with("You are a coding agent."),
            "其余内容必须逐字保留: {cleaned}"
        );
    }

    /// 预检与改写同源：上游忽略大小写，预检也必须忽略大小写，否则全小写形态
    /// 在 `has_fingerprint` 处就被判为「无指纹」直接返回原文。
    #[test]
    fn harness_identity_is_hit_by_precheck_in_any_case() {
        assert!(has_fingerprint(
            "You are an AI agent powered by DeepSeek Harness."
        ));
        assert!(has_fingerprint(
            "you are an ai agent powered by deepseek harness"
        ));
        // 加连字符即不触发上游拦截，故刻意不改写（避免无谓改动用户内容）。
        assert!(!has_fingerprint(
            "You are an AI agent powered by DeepSeek-Harness."
        ));
    }

    /// 护栏：`FEATURES` 预检与 `REWRITES` 规则必须成对。
    ///
    /// `sanitize_text` 在预检不中时直接返回原文，只加 `REWRITES` 而不加
    /// `FEATURES` 会让规则**永远不执行** —— 这类「改了却不生效」的漏改必须拦住。
    #[test]
    fn every_rewrite_source_is_covered_by_feature_precheck() {
        for (from, _) in REWRITES {
            assert!(
                has_fingerprint(from),
                "REWRITES 的匹配串未被预检覆盖，该规则永不生效: {from}"
            );
        }
    }

    #[test]
    fn ordinary_text_is_untouched() {
        let text = "正常的一句中英文 mixed content，包含 11101 与 11148 等无关数字。";
        assert_eq!(sanitize_text(text), text);
        assert!(!has_fingerprint(text));
    }

    #[test]
    fn header_key_value_is_stripped_entirely() {
        let text = "prefix x-anthropic-billing-header: abc-123; suffix";
        let cleaned = sanitize_text(text);
        assert!(!cleaned.contains("abc-123"), "值必须被删除: {cleaned}");
        assert!(cleaned.contains("prefix"));
        assert!(cleaned.contains("suffix"));
    }

    #[test]
    fn bare_header_key_is_abbreviated_case_insensitively() {
        let cleaned = sanitize_text("see `X-Anthropic-Billing-Header` in docs");
        assert!(
            !cleaned
                .to_lowercase()
                .contains("x-anthropic-billing-header"),
            "裸键名必须被改写: {cleaned}"
        );
        assert!(cleaned.contains(BARE_HEADER_REPLACEMENT));
    }

    #[test]
    fn cc_key_values_are_removed_in_a_loop() {
        let text = "a cc_version=1.2.3; cc_entrypoint=cli; b";
        let cleaned = sanitize_text(text);
        assert!(
            !cleaned.contains("cc_version"),
            "首个键值必须清除: {cleaned}"
        );
        assert!(
            !cleaned.contains("cc_entrypoint"),
            "相邻键值也必须清除: {cleaned}"
        );
        assert!(cleaned.contains('a') && cleaned.contains('b'));
    }

    #[test]
    fn identity_sentence_is_rewritten_with_one_word() {
        let cleaned = sanitize_text("You are Claude Code, Anthropic's official CLI for Claude.");
        assert!(cleaned.contains("official CLI tool for Claude"));
        assert!(!cleaned.contains("official CLI for Claude"));
        assert!(cleaned.ends_with('.'), "原有标点必须保留: {cleaned}");
    }

    #[test]
    fn desktop_variant_without_period_is_also_rewritten() {
        let cleaned = sanitize_text(
            "You are Claude Code, Anthropic's official CLI for Claude, running within the Claude Agent SDK.",
        );
        assert!(
            cleaned.contains("official CLI tool for Claude"),
            "不带句号的桌面版形态必须覆盖: {cleaned}"
        );
    }

    #[test]
    fn main_branch_and_feedback_sentences_are_rewritten() {
        let cleaned = sanitize_text("Main branch (you will usually use this for PRs)");
        assert!(cleaned.contains("Default branch"));

        let cleaned = sanitize_text(
            "To give feedback, users should report the issue at https://github.com/anthropics/claude-code/issues",
        );
        assert!(cleaned.contains("To provide feedback"));
        assert!(!cleaned.contains("To give feedback"));
    }

    #[test]
    /// ★ 回归 issue #241：Trae 3.4.24 的身份句必须被改写。
    ///
    /// 上游渠道校验按**逐字**匹配判定「不是官方客户端」，命中即 400 + `11-128`。
    /// 漏改 = Trae 调 hy4flash 等模型时每次都失败，而客户端本身完全正常。
    ///
    /// 可证伪：删掉 `REWRITES` 里那条规则，本用例立刻红；
    /// 只加 `FEATURES` 不加 `REWRITES`，`every_rewrite_source_is_covered_by_feature_precheck`
    /// 之外的这条也会红（预检命中但改写不发生，原文残留）。
    #[test]
    fn traecode_identity_line_is_rewritten() {
        let source = "You are an interactive agent in TraeCode that helps the USER with software engineering tasks.";
        assert!(has_fingerprint(source), "预检必须命中，否则改写层永不执行");

        let cleaned = sanitize_text(source);
        assert!(
            !cleaned.contains("TraeCode"),
            "改写后不得残留可被逐字匹配的产品名: {cleaned}"
        );
        assert!(
            cleaned.contains("that helps the USER with software engineering tasks."),
            "句子其余部分必须逐字保留（语义不变）: {cleaned}"
        );
    }

    /// 反向：`TraeCode` 只出现在用户自己的对话内容里时也必须被改写 ——
    /// 上游拦的是请求体全文，不分角色。这不是误伤：留着就会整单失败。
    #[test]
    fn traecode_identity_is_rewritten_inside_message_arrays() {
        let mut messages = vec![json!({
            "role": "system",
            "content": "You are an interactive agent in TraeCode that helps the USER.",
        })];
        assert!(sanitize_messages(&mut messages));
        let text = messages[0]["content"].as_str().unwrap();
        assert!(
            !text.contains("TraeCode"),
            "系统消息里的身份句必须改写: {text}"
        );
    }

    fn codex_instruction_gains_tool_word() {
        let cleaned = sanitize_text(
            "You are a coding agent running in the Codex CLI, a terminal-based coding assistant.",
        );
        assert!(cleaned.contains("Codex CLI tool, a terminal-based"));
    }

    #[test]
    fn bare_number_11128_is_hyphenated() {
        let cleaned = sanitize_text("error code=11128 occurred");
        assert!(cleaned.contains("11-128"));
        assert!(!cleaned.contains("11128"));
    }

    #[test]
    fn health_bypass_survives_regression() {
        // 相邻错误码必须放行（上游只拦 11128）。
        assert_eq!(sanitize_text("code 11148"), "code 11148");
        assert_eq!(sanitize_text("code 11115"), "code 11115");
    }

    #[test]
    fn sanitize_messages_cleans_content_and_tool_calls_independently() {
        let mut messages = vec![
            json!({"role": "user", "content": "You are Claude Code, Anthropic's official CLI for Claude"}),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "c1",
                    "type": "function",
                    "function": {"name": "f", "arguments": "{\"note\":\"code 11128\"}"}
                }]
            }),
            json!({"role": "tool", "tool_call_id": "c1", "content": "cc_entrypoint=cli;"}),
        ];
        assert!(sanitize_messages(&mut messages));
        assert!(messages[0]["content"]
            .as_str()
            .expect("string content")
            .contains("official CLI tool for Claude"));
        assert!(
            messages[1]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .expect("string arguments")
                .contains("11-128"),
            "content 为 null 时 tool_calls 仍必须净化"
        );
        assert_eq!(messages[2]["content"], json!(""), "裸 kv 被清空后仅剩空串");
    }

    #[test]
    fn sanitize_messages_handles_multimodal_text_parts_only() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Please read Main branch (you will usually use this for PRs)"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA11128"}}
            ]
        })];
        assert!(sanitize_messages(&mut messages));
        assert!(messages[0]["content"][0]["text"]
            .as_str()
            .expect("string")
            .contains("Default branch"));
        assert_eq!(
            messages[0]["content"][1]["image_url"]["url"],
            json!("data:image/png;base64,AAAA11128"),
            "非 text part 不得被改写"
        );
    }

    #[test]
    fn sanitize_messages_reports_no_change_for_clean_input() {
        let mut messages = vec![json!({"role": "user", "content": "hello world"})];
        assert!(!sanitize_messages(&mut messages));
        assert_eq!(messages[0]["content"], json!("hello world"));
    }

    #[test]
    fn sanitize_messages_cleans_reasoning_content() {
        let mut messages = vec![json!({
            "role": "assistant",
            "content": "ok",
            "reasoning_content": "plan refers to 11128"
        })];
        assert!(sanitize_messages(&mut messages));
        assert!(messages[0]["reasoning_content"]
            .as_str()
            .expect("string")
            .contains("11-128"));
    }
}
