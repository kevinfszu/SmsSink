use crate::app::AppState;
use crate::config::SUCCESS_MARK;
use crate::rules::{evaluate_outcome, Outcome};
use crate::{admin, event, win};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub struct Inbox {
    pub from: String,
    pub content: String,
    pub timestamp: String,
    pub sign: String,
}

pub struct Resp {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    pub extra_header: Option<String>,
}

impl Resp {
    pub fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body,
            extra_header: None,
        }
    }
    pub fn html(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8",
            body,
            extra_header: None,
        }
    }
}

/// 被动接收端：只 accept，不主动发起任何连接。
pub fn run(state: Arc<AppState>) -> std::io::Result<()> {
    let (listen, port, hook_path) = {
        let cfg = state.cfg.read().unwrap_or_else(|e| e.into_inner());
        (cfg.listen.clone(), cfg.port, cfg.hook_path.clone())
    };
    let addr = format!("{listen}:{port}");
    let listener = TcpListener::bind(&addr)?;
    println!("[sink] 被动监听 {addr}{hook_path} （等待推送，不主动拉取）");
    println!("[sink] 管理台 http://127.0.0.1:{port}/ （仅本机可访问）");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || handle(stream, state));
            }
            Err(e) => eprintln!("[sink] accept 失败: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, state: Arc<AppState>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let is_loopback = stream
        .peer_addr()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false);

    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
        match stream.read(&mut tmp) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
        if buf.len() > 64 * 1024 {
            return;
        }
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let body_len: usize = header_value(&head, "Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .min(MAX_BODY);
    while buf.len() < header_end + body_len {
        match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let end = (header_end + body_len).min(buf.len());
    let body = String::from_utf8_lossy(&buf[header_end..end]).into_owned();

    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("/");
    let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));

    // 局域网连通性自检：手机浏览器可直接访问，用来区分「网络/防火墙不通」与「通道配置错误」。
    // 只返回状态与对端 IP，不暴露任何配置或短信内容，也无需 token。
    if method == "GET" && path == "/ping" {
        let peer = stream
            .peer_addr()
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|_| "unknown".into());
        respond(
            &mut stream,
            Resp::json(
                200,
                format!(
                    r#"{{"code":0,"app":"{}","status":"reachable","peer":"{}"}}"#,
                    crate::config::APP_ID,
                    peer
                ),
            ),
        );
        return;
    }

    let cfg = state.cfg.read().unwrap_or_else(|e| e.into_inner()).clone();

    // 手机端推送：唯一允许来自局域网的路径，靠 HMAC 加签鉴权
    if method == "POST" && path == cfg.hook_path {
        receive(&mut stream, &cfg, &state.seen, &head, &body);
        return;
    }

    // 管理台与静态页：仅允许本机回环地址
    if !is_loopback {
        respond(
            &mut stream,
            Resp::json(403, r#"{"code":403,"msg":"管理台仅允许本机访问"}"#.into()),
        );
        return;
    }

    let token_ok = {
        let expected = &cfg.admin_token;
        let from_query = query
            .split('&')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| *k == "token")
            .map(|(_, v)| url_decode(v));
        let from_header = header_value(&head, "X-SmsSink-Token");
        let from_auth = header_value(&head, "Authorization")
            .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.trim().to_string()));
        !expected.is_empty()
            && [from_query, from_header, from_auth]
                .into_iter()
                .flatten()
                .any(|v| v == *expected)
    };

    let resp = admin::route(method, path, query, &body, token_ok);
    respond(&mut stream, resp);
}

fn receive(
    stream: &mut TcpStream,
    cfg: &crate::config::Config,
    seen: &Mutex<HashSet<String>>,
    head: &str,
    body: &str,
) {
    let is_json = header_value(head, "Content-Type")
        .map(|v| v.to_lowercase().contains("json"))
        .unwrap_or(false);
    let inbox = parse_body(body, is_json);

    if let Err(reason) = verify(cfg, &inbox, seen) {
        eprintln!("[sink] 拒绝请求: {reason}");
        respond(
            stream,
            Resp::json(401, format!("{{\"code\":401,\"msg\":\"{reason}\"}}")),
        );
        return;
    }

    println!(
        "[sink] 接到推送 from={} 字数={}",
        inbox.from,
        inbox.content.chars().count()
    );
    // 「没命中」和「命中了但没提取到码」必须区分开：后者要让用户看见，
    // 否则他只会觉得短信没到。判定细节交给 rules 层，这里只做搬运。
    let (decision, no_code) = match evaluate_outcome(cfg, &inbox.from, &inbox.content) {
        Outcome::Code(d) => (Some(d), None),
        Outcome::NoCode { rule_name, notify } => (
            None,
            Some(event::NoCodeHit { rule_name, notify }),
        ),
        Outcome::NoMatch => (None, None),
    };
    event::submit(event::SmsEvent {
        from: inbox.from,
        content: inbox.content,
        timestamp_ms: inbox
            .timestamp
            .parse::<i64>()
            .unwrap_or_else(|_| win::now_ms()),
        local_time: win::local_time(),
        decision,
        no_code,
    });

    respond(
        stream,
        Resp::json(200, format!("{{\"code\":0,\"msg\":\"{SUCCESS_MARK}\"}}")),
    );
}

/// 官方加签规则：sign = urlEncode(Base64(HmacSHA256(timestamp + "\n" + secret, secret)))
fn verify(
    cfg: &crate::config::Config,
    inbox: &Inbox,
    seen: &Mutex<HashSet<String>>,
) -> Result<(), String> {
    if cfg.secret.is_empty() {
        return Ok(());
    }
    let ts: i64 = inbox
        .timestamp
        .parse()
        .map_err(|_| "timestamp 无效".to_string())?;
    if (win::now_ms() - ts).abs() > cfg.timestamp_tolerance_secs * 1000 {
        return Err("timestamp 超出允许时间窗".into());
    }

    let mut mac = HmacSha256::new_from_slice(cfg.secret.as_bytes())
        .expect("HMAC 接受任意长度密钥");
    mac.update(format!("{}\n{}", inbox.timestamp, cfg.secret).as_bytes());
    let decoded = STANDARD
        .decode(url_decode(&inbox.sign).as_bytes())
        .map_err(|_| "sign 不是合法 Base64".to_string())?;
    mac.verify_slice(&decoded)
        .map_err(|_| "sign 校验失败".to_string())?;

    let mut set = seen.lock().unwrap_or_else(|e| e.into_inner());
    if set.contains(&inbox.sign) {
        return Err("重复请求（疑似重放）".into());
    }
    set.insert(inbox.sign.clone());
    if set.len() > 2000 {
        set.clear();
    }
    Ok(())
}

pub(crate) fn parse_body(body: &str, is_json: bool) -> Inbox {
    if is_json {
        return match serde_json::from_str::<serde_json::Value>(body) {
            Ok(v) => Inbox {
                from: pick(&v, &["from", "sender"]),
                content: pick(&v, &["content", "msg", "message", "text"]),
                timestamp: pick(&v, &["timestamp", "time"]),
                sign: pick(&v, &["sign", "signature"]),
            },
            Err(_) => Inbox::default(),
        };
    }

    let mut inbox = Inbox::default();
    for pair in body.split('&') {
        let (key, raw) = pair.split_once('=').unwrap_or((pair, ""));
        let value = url_decode(raw);
        match key {
            "from" => inbox.from = value,
            "content" | "msg" => inbox.content = value,
            "timestamp" => inbox.timestamp = value,
            "sign" => inbox.sign = value,
            _ => {}
        }
    }
    inbox
}

fn pick(v: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(x) = v.get(key) {
            return match x {
                serde_json::Value::String(s) => s.clone(),
                _ => x.to_string(),
            };
        }
    }
    String::new()
}

pub(crate) fn query_value(query: &str, key: &str) -> String {
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| url_decode(v))
        .unwrap_or_default()
}

pub(crate) fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

pub(crate) fn header_value(head: &str, key: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if k.eq_ignore_ascii_case(key) {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

fn respond(stream: &mut TcpStream, resp: Resp) {
    let reason = match resp.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };
    let extra = resp
        .extra_header
        .map(|h| format!("{h}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{extra}Connection: close\r\n\r\n",
        resp.status,
        resp.content_type,
        resp.body.as_bytes().len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(resp.body.as_bytes());
    let _ = stream.flush();
}
