use crate::config::{Config, NotifyMode, Rule};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct Decision {
    pub rule_name: String,
    pub code: String,
    pub notify: NotifyMode,
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

/// 按优先级自上而下匹配，首个命中即返回。
/// 提取刻意采用「关键词 + 上下文窗口」而非裸 \d{6}，避免订单号 / 金额 / 时间误命中。
pub fn evaluate(cfg: &Config, from: &str, content: &str) -> Option<Decision> {
    let mut ordered: Vec<&Rule> = cfg.rules.iter().filter(|r| r.enabled).collect();
    ordered.sort_by(|a, b| b.priority.cmp(&a.priority));

    for rule in ordered {
        if !rule.match_from.is_empty() && !rule.match_from.iter().any(|p| from.contains(p)) {
            continue;
        }
        if !rule.keywords_all.is_empty() && !rule.keywords_all.iter().all(|k| content.contains(k)) {
            continue;
        }
        if !rule.keywords_any.is_empty() && !rule.keywords_any.iter().any(|k| content.contains(k)) {
            continue;
        }
        if rule.exclude.iter().any(|k| content.contains(k)) {
            continue;
        }

        let re = match Regex::new(&rule.extract) {
            Ok(re) => re,
            Err(e) => {
                eprintln!("[rules] 规则「{}」正则无效: {e}", rule.name);
                continue;
            }
        };
        let caps = match re.captures(content) {
            Some(c) => c,
            None => continue,
        };
        let code = caps
            .get(rule.capture)
            .or_else(|| caps.get(0))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if code.is_empty() {
            continue;
        }

        let template = rule.copy_template.as_deref().unwrap_or("{code}");
        return Some(Decision {
            rule_name: rule.name.clone(),
            code: code.clone(),
            notify: rule.notify,
            auto_copy: rule.auto_copy,
            copy_text: render(template, &code, from, content),
            ttl_sec: rule.ttl_sec,
            keep_history: rule.keep_history,
        });
    }
    None
}
