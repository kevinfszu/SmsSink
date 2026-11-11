//! 本地管理台 API。所有 /api/* 都要求 token，且仅允许 127.0.0.1 访问（在 server.rs 中拦截）。
use crate::app;
use crate::config::{self, Config, Rule};
use crate::server::{query_value, Resp};
use crate::{event, net, rules, store, win};
use regex::Regex;
use serde_json::{json, Value};

fn err(status: u16, msg: &str) -> Resp {
    Resp::json(
        status,
        json!({ "code": status, "msg": msg }).to_string(),
    )
}

fn body_json(body: &str) -> Result<Value, Resp> {
    serde_json::from_str::<Value>(body).map_err(|e| err(400, &format!("JSON 解析失败: {e}")))
}

pub fn route(method: &str, path: &str, query: &str, body: &str, token_ok: bool) -> Resp {
    // 静态页无需 token：页面自身会提示输入 token，API 仍需鉴权
    if path == "/" || path == "/index.html" {
        return Resp::html(200, include_str!("../assets/index.html").to_string());
    }
    if !path.starts_with("/api/") {
        return err(404, "not found");
    }
    if path == "/api/health" {
        return Resp::json(
            200,
            json!({"code":0,"app":config::APP_ID,"status":"listening"}).to_string(),
        );
    }
    if !token_ok {
        return err(401, "访问令牌无效，请从托盘菜单「打开管理台」进入");
    }

    match (method, path) {
        ("GET", "/api/state") => state(),
        ("GET", "/api/rules") => get_rules(),
        ("POST", "/api/rules") => put_rules(body),
        ("POST", "/api/rules/test") => test_rule(body),
        ("POST", "/api/settings") => put_settings(body),
        ("GET", "/api/messages") => get_messages(query),
        ("GET", "/api/stats") => {
            let days = query_value(query, "days").parse::<u32>().unwrap_or(30);
            Resp::json(200, store::stats(days).to_string())
        }
        ("GET", "/api/export.csv") => export_csv(query),
        ("POST", "/api/copy") => copy(body),
        ("POST", "/api/reload") => match app::reload_config() {
            Ok(()) => Resp::json(200, json!({"code":0,"msg":"配置已重载"}).to_string()),
            Err(e) => err(400, &e),
        },
        ("POST", "/api/autostart") => autostart(body),
        ("POST", "/api/test-notify") => {
            let hwnd = event::UI_HWND.load(std::sync::atomic::Ordering::SeqCst);
            if hwnd != 0 {
                unsafe {
                    win::PostMessageW(hwnd, event::WM_APP_TEST, 0, 0);
                }
                Resp::json(200, json!({"code":0,"msg":"已发送测试通知"}).to_string())
            } else {
                err(400, "当前为无界面模式，无法弹通知")
            }
        }
        _ => err(404, "not found"),
    }
}

fn state() -> Resp {
    let cfg = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    // 手机端要填的地址得是"手机能访问到的那一个"，所以由后端给出候选，
    // 不在前端猜（前端拿不到网卡信息）
    let local_ips: Vec<serde_json::Value> = net::local_v4()
        .into_iter()
        .map(|a| json!({"ip": a.ip, "name": a.name, "wifi": a.wifi}))
        .collect();
    Resp::json(
        200,
        json!({
            "app": config::APP_DISPLAY_NAME,
            "id": config::APP_ID,
            "version": env!("CARGO_PKG_VERSION"),
            "port": cfg.port,
            "listen": cfg.listen,
            "hook_path": cfg.hook_path,
            "local_ips": local_ips,
            "secret_set": !cfg.secret.is_empty(),
            "autostart": win::auto_start_enabled(),
            "rule_count": cfg.rules.len(),
            "card_seconds": cfg.card_seconds,
            "toast_limit": cfg.toast_limit,
            "retention_days": cfg.retention_days,
            "mask_content": cfg.mask_content,
            "keep_unmatched": cfg.keep_unmatched,
            "history_limit": cfg.history_limit,
        })
        .to_string(),
    )
}

fn get_rules() -> Resp {
    let cfg = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Resp::json(200, serde_json::to_string(&cfg.rules).unwrap_or_else(|_| "[]".into()))
}

fn put_rules(body: &str) -> Resp {
    let rules: Vec<Rule> = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return err(400, &format!("规则格式错误: {e}")),
    };
    for r in &rules {
        if let Err(e) = Regex::new(&r.extract) {
            return err(400, &format!("规则「{}」正则无效: {e}", r.name));
        }
        if r.name.trim().is_empty() {
            return err(400, "规则名不能为空");
        }
    }
    match app::replace_rules(rules.clone()) {
        Ok(()) => Resp::json(
            200,
            json!({"code":0,"msg":"规则已保存并立即生效","count":rules.len()}).to_string(),
        ),
        Err(e) => err(400, &e),
    }
}

fn test_rule(body: &str) -> Resp {
    let v = match body_json(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let rule: Rule = match serde_json::from_value(v.get("rule").cloned().unwrap_or(Value::Null)) {
        Ok(r) => r,
        Err(e) => return err(400, &format!("规则格式错误: {e}")),
    };
    let content = v.get("content").and_then(Value::as_str).unwrap_or("");
    let from = v.get("from").and_then(Value::as_str).unwrap_or("");
    Resp::json(200, rules::preview(&rule, from, content).to_string())
}

fn put_settings(body: &str) -> Resp {
    let v = match body_json(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let state = app::get();
    let mut guard = state.cfg.write().unwrap_or_else(|e| e.into_inner());
    let mut cfg: Config = guard.clone();
    if let Some(x) = v.get("secret").and_then(Value::as_str) {
        cfg.secret = x.to_string();
    }
    if let Some(x) = v.get("card_seconds").and_then(Value::as_u64) {
        cfg.card_seconds = x;
    }
    if let Some(x) = v.get("toast_limit").and_then(Value::as_u64) {
        cfg.toast_limit = x as usize;
    }
    if let Some(x) = v.get("retention_days").and_then(Value::as_u64) {
        cfg.retention_days = x as u32;
    }
    if let Some(x) = v.get("mask_content").and_then(Value::as_bool) {
        cfg.mask_content = x;
    }
    if let Some(x) = v.get("keep_unmatched").and_then(Value::as_bool) {
        cfg.keep_unmatched = x;
    }
    if let Some(x) = v.get("port").and_then(Value::as_u64) {
        cfg.port = x as u16;
    }
    if let Some(x) = v.get("hook_path").and_then(Value::as_str) {
        cfg.hook_path = x.to_string();
    }
    *guard = cfg.clone();
    drop(guard);
    match config::save(&cfg) {
        Ok(()) => Resp::json(
            200,
            json!({"code":0,"msg":"设置已保存（端口类变更需重启程序）"}).to_string(),
        ),
        Err(e) => err(400, &e.to_string()),
    }
}

fn get_messages(query: &str) -> Resp {
    let limit = query_value(query, "limit").parse::<usize>().unwrap_or(100);
    let offset = query_value(query, "offset").parse::<usize>().unwrap_or(0);
    let sender = query_value(query, "sender");
    let rule = query_value(query, "rule");
    let q = query_value(query, "q");
    let state = app::get();
    let cfg = state.cfg.read().unwrap_or_else(|e| e.into_inner());
    let limit = limit.min(cfg.history_limit.max(1));
    drop(cfg);
    let rows = store::recent(limit, offset, &sender, &rule, &q);
    Resp::json(200, json!({ "code": 0, "rows": rows }).to_string())
}

fn export_csv(query: &str) -> Resp {
    let csv = store::export_csv(
        &query_value(query, "sender"),
        &query_value(query, "rule"),
        &query_value(query, "q"),
    );
    let name = format!("sms-sink-{}.csv", win::local_time().replace(['-', ' ', ':'], ""));
    Resp {
        status: 200,
        content_type: "text/csv; charset=utf-8",
        body: csv,
        extra_header: Some(format!("Content-Disposition: attachment; filename=\"{name}\"")),
    }
}

fn copy(body: &str) -> Resp {
    let v = match body_json(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let text = v.get("text").and_then(Value::as_str).unwrap_or("");
    if text.is_empty() {
        return err(400, "复制内容为空");
    }
    match event::request_copy(text) {
        Ok(()) => {
            if let Some(id) = v.get("id").and_then(Value::as_i64) {
                store::mark_copied(id);
            }
            Resp::json(200, json!({"code":0,"msg":"已复制到剪贴板"}).to_string())
        }
        Err(e) => err(400, &e),
    }
}

fn autostart(body: &str) -> Resp {
    let v = match body_json(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let enabled = v.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    match win::set_auto_start(enabled) {
        Ok(()) => Resp::json(
            200,
            json!({"code":0,"enabled":enabled,"msg": if enabled {"已开启开机自启"} else {"已关闭开机自启"}}).to_string(),
        ),
        Err(e) => err(400, &e),
    }
}
