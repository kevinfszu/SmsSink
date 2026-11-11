use crate::config::{Config, Rule};
use regex::Regex;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Decision {
    pub rule_name: String,
    pub code: String,
    pub notify: crate::config::NotifyMode,
    pub auto_copy: bool,
    pub copy_text: String,
    pub ttl_sec: u64,
    pub keep_history: bool,
}

fn render(template: &str, code: &str, from: &str, content: &str) -> String {
    template
        .replace("{code}", code)
        .replace("{from}", from)
        .replace("{content}", content)
}

fn conditions_met(rule: &Rule, from: &str, content: &str) -> bool {
    if !rule.enabled {
        return false;
    }
    if !rule.match_from.is_empty() && !rule.match_from.iter().any(|p| from.contains(p)) {
        return false;
    }
    if !rule.keywords_all.is_empty() && !rule.keywords_all.iter().all(|k| content.contains(k)) {
        return false;
    }
    if !rule.keywords_any.is_empty() && !rule.keywords_any.iter().any(|k| content.contains(k)) {
        return false;
    }
    if rule.exclude.iter().any(|k| content.contains(k)) {
        return false;
    }
    true
}

/// 提取刻意采用「关键词 + 上下文窗口」而非裸 \d{6}，避免订单号 / 金额 / 时间误命中。
fn extract_from(rule: &Rule, content: &str) -> Result<Option<String>, String> {
    let re = Regex::new(&rule.extract).map_err(|e| format!("正则无效: {e}"))?;
    Ok(re.captures(content).and_then(|caps| {
        caps.get(rule.capture)
            .or_else(|| caps.get(0))
            .map(|m| m.as_str().to_string())
    }))
}

/// 一条短信的归属判定结果。
///
/// 「没命中规则」和「命中了但没提取到验证码」对用户是完全不同的两件事：
/// 前者是别人的短信，可以不管；后者是他正等着的码，只是格式变了。
/// 早期两者都返回 `None`，于是后者被静默丢弃——这是验证码工具最不能犯的错。
pub enum Outcome {
    /// 命中规则且提取到验证码
    Code(Decision),
    /// 命中规则但没提取到验证码，按该规则的通知方式降级提示
    NoCode {
        rule_name: String,
        notify: crate::config::NotifyMode,
    },
    /// 没有任何规则命中
    NoMatch,
}

/// 按优先级自上而下匹配，首个命中且提取成功的规则即返回；
/// 若所有命中规则的提取都失败，返回第一条命中规则的 `NoCode`。
pub fn evaluate_outcome(cfg: &Config, from: &str, content: &str) -> Outcome {
    let mut ordered: Vec<&Rule> = cfg.rules.iter().collect();
    ordered.sort_by(|a, b| b.priority.cmp(&a.priority));

    let mut fallback: Option<&Rule> = None;
    for rule in ordered {
        if !conditions_met(rule, from, content) {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(rule);
        }
        let code = match extract_from(rule, content) {
            Ok(Some(c)) if !c.is_empty() => c,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("[rules] 规则「{}」{e}", rule.name);
                continue;
            }
        };
        let template = rule.copy_template.as_deref().unwrap_or("{code}");
        return Outcome::Code(Decision {
            rule_name: rule.name.clone(),
            code: code.clone(),
            notify: rule.notify,
            auto_copy: rule.auto_copy,
            copy_text: render(template, &code, from, content),
            ttl_sec: rule.ttl_sec,
            keep_history: rule.keep_history,
        });
    }

    match fallback {
        Some(rule) => Outcome::NoCode {
            rule_name: rule.name.clone(),
            notify: rule.notify,
        },
        None => Outcome::NoMatch,
    }
}

/// 管理台「样例短信试跑」，把每一步判断结果返回给界面
pub fn preview(rule: &Rule, from: &str, content: &str) -> Value {
    if !conditions_met(rule, from, content) {
        let reason = if !rule.enabled {
            "规则未启用"
        } else if !rule.match_from.is_empty() && !rule.match_from.iter().any(|p| from.contains(p)) {
            "发送方号码不匹配"
        } else if !rule.keywords_all.is_empty()
            && !rule.keywords_all.iter().all(|k| content.contains(k))
        {
            "缺少「全部包含」关键词"
        } else if !rule.keywords_any.is_empty()
            && !rule.keywords_any.iter().any(|k| content.contains(k))
        {
            "缺少「任一包含」关键词"
        } else {
            "命中了排除关键词"
        };
        return json!({ "matched": false, "reason": reason });
    }
    match extract_from(rule, content) {
        Ok(Some(code)) if !code.is_empty() => json!({
            "matched": true,
            "code": code,
            "copy_text": render(rule.copy_template.as_deref().unwrap_or("{code}"), &code, from, content)
        }),
        Ok(_) => json!({ "matched": false, "reason": "正则未提取到内容" }),
        Err(e) => json!({ "matched": false, "reason": e }),
    }
}
