use crate::config::{Config, SUCCESS_MARK};
use crate::rules::{evaluate, Decision};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub struct Inbox {
    pub from: String,
    pub content: String,
    pub timestamp: String,
    pub sign: String,
}

/// 被动接收端：只 accept，不主动发起任何连接。
pub fn run(cfg: Config) -> std::io::Result<()> {
    let addr = format!("{}:{}", cfg.listen, cfg.port);
    let listener = TcpListener::bind(&addr)?;
    println!(
        "[sink] 被动监听 {addr}{} （等待推送，不主动拉取）",
        cfg.hook_path
    );
    println!("[sink] 管理台 http://127.0.0.1:{}/", cfg.port);

    let cfg = Arc::new(cfg);
    let seen: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cfg = Arc::clone(&cfg);
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || handle(stream, cfg, seen));
            }
            Err(e) => eprintln!("[sink] accept 失败: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, cfg: Arc<Config>, seen: Arc<Mutex<HashSet<String>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
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
    let path = raw_path.split('?').next().unwrap_or("/");

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            respond_html(&mut stream, 200, include_str!("../assets/index.html"))
        }
        ("GET", "/api/health") => respond_json(
            &mut stream,
            200,
            &format!(
                "{{\"code\":0,\"app\":\"{}\",\"status\":\"listening\"}}",
                crate::config::APP_ID
            ),
        ),
        ("POST", p) if p == cfg.hook_path => receive(&mut stream, &cfg, &seen, &head, &body),
        _ => respond_json(&mut stream, 404, "{\"code\":404,\"msg\":\"not found\"}"),
    }
}

fn receive(
    stream: &mut TcpStream,
    cfg: &Config,
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
        respond_json(
            stream,
            401,
            &format!("{{\"code\":401,\"msg\":\"{reason}\"}}"),
        );
        return;
    }

    println!(
        "[sink] 接到推送 from={} 字数={}",
        inbox.from,
        inbox.content.chars().count()
    );
    match evaluate(cfg, &inbox.from, &inbox.content) {
        Some(d) => report(&d),
        None => println!("[sink]   未命中任何规则"),
    }

    respond_json(
        stream,
        200,
        &format!("{{\"code\":0,\"msg\":\"{SUCCESS_MARK}\"}}"),
    );
}

fn report(d: &Decision) {
    println!(
        "[sink]   规则「{}」→ 提取 {} | 通知={:?} | 自动复制={} | 复制文本={}",
        d.rule_name, d.code, d.notify, d.auto_copy, d.copy_text
    );
    // 下一步接入：Toast / 卡片通知、复制按钮、剪贴板、SQLite 落盘、管理台推送
}

/// 官方加签规则：sign = urlEncode(Base64(HmacSHA256(timestamp + "\n" + secret, secret)))
fn verify(cfg: &Config, inbox: &Inbox, seen: &Mutex<HashSet<String>>) -> Result<(), String> {
    if cfg.secret.is_empty() {
        return Ok(());
    }
    let ts: i64 = inbox
        .timestamp
        .parse()
        .map_err(|_| "timestamp 无效".to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    if (now - ts).abs() > cfg.timestamp_tolerance_secs * 1000 {
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

fn parse_body(body: &str, is_json: bool) -> Inbox {
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

fn url_decode(s: &str) -> String {
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

fn header_value(head: &str, key: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if k.eq_ignore_ascii_case(key) {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

fn respond_json(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.as_bytes().len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn respond_html(stream: &mut TcpStream, status: u16, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.as_bytes().len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}
