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
    /// 用 Vec<u8> 而不是 String：二维码这类图片要按原始字节发出去，
    /// 走 Base64 内嵌到页面里会白胖三分之一，也没法单独缓存
    pub body: Vec<u8>,
    pub extra_header: Option<String>,
}

impl Resp {
    pub fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
            extra_header: None,
        }
    }
    pub fn html(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8",
            body: body.into_bytes(),
            extra_header: None,
        }
    }
    /// 二进制静态资源（图片）。内容是构建期 `include_bytes!` 进来的，生命周期是 'static
    pub fn bin(status: u16, content_type: &'static str, body: &'static [u8]) -> Self {
        Self {
            status,
            content_type,
            body: body.to_vec(),
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
    // 局域网连接打一行日志：手机测试「请求失败」但看不到这行，就能断定
    // 请求根本没到电脑（网络层问题），而不是签名或格式被拒
    if !is_loopback {
        if let Ok(peer) = stream.peer_addr() {
            println!("[sink] 连接来自 {peer}");
        }
    }

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

    // 手机端推送：唯一允许来自局域网的路径，靠 HMAC 加签鉴权。
    // 官方 SmsForwarder 支持 POST/GET/PUT/PATCH：GET 与部分配置会把
    // timestamp/sign 放 URL query（还有防缓存参数 _t），合并进字段来源。
    if path == cfg.hook_path && matches!(method, "POST" | "GET" | "PUT" | "PATCH") {
        let body = merge_query(&body, &query);
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

/// 把 URL query 里的字段并入表单来源（body 优先：body 已有的键不覆盖）。
/// JSON body 不合并（格式不同）；防缓存参数 _t 之类会被 parse_body 忽略，无害。
fn merge_query(body: &str, query: &str) -> String {
    if body.trim_start().starts_with('{') {
        return body.to_string();
    }
    let mut merged = body.to_string();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, _) = pair.split_once('=').unwrap_or((pair, ""));
        if !merged.contains(&format!("{k}=")) {
            if !merged.is_empty() && !merged.ends_with('&') {
                merged.push('&');
            }
            merged.push_str(pair);
        }
    }
    merged
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
        Outcome::NoCode { rule_name, notify } => {
            (None, Some(event::NoCodeHit { rule_name, notify }))
        }
        Outcome::NoMatch => (None, None),
    };
    event::submit(event::SmsEvent {
        from: inbox.from,
        content: inbox.content,
        timestamp_ms: ts_ms(&inbox.timestamp),
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

/// 推送端有的给秒级（10 位）时间戳，有的给毫秒级（13 位）：按位数归一到毫秒。
/// 不归一的话，秒级时间戳永远「超出时间窗」，表现为开了加签后全部被拒。
fn ts_ms(raw: &str) -> i64 {
    match raw.parse::<i64>() {
        Ok(ts) => ts_ms_scale(ts),
        Err(_) => win::now_ms(),
    }
}

fn ts_ms_scale(ts: i64) -> i64 {
    if ts.abs() < 1_000_000_000_000 {
        ts * 1000
    } else {
        ts
    }
}

fn ts_ms_checked(raw: &str) -> Result<i64, String> {
    raw.parse::<i64>()
        .map(ts_ms_scale)
        .map_err(|_| "timestamp 无效".to_string())
}
fn verify(
    cfg: &crate::config::Config,
    inbox: &Inbox,
    seen: &Mutex<HashSet<String>>,
) -> Result<(), String> {
    if cfg.secret.is_empty() {
        return Ok(());
    }
    let ts = ts_ms_checked(&inbox.timestamp)?;
    if (win::now_ms() - ts).abs() > cfg.timestamp_tolerance_secs * 1000 {
        return Err("timestamp 超出允许时间窗".into());
    }

    let mut mac = HmacSha256::new_from_slice(cfg.secret.as_bytes()).expect("HMAC 接受任意长度密钥");
    mac.update(format!("{}\n{}", inbox.timestamp, cfg.secret).as_bytes());
    // sign 到达这里可能是三种形态（都以合法签名结尾归一为原始 Base64）：
    // 1. 我们自测脚本/标准表单客户端：编码一次 → 表单解析解一次 → 原始 Base64；
    // 2. 官方 SmsForwarder：URLEncoder 编码一次后，OkHttp FormBody 又编码一次
    //    （postRequest.params 会再 encode），表单解析只解一层 → 残留 %2B/%3D；
    // 3. JSON 通道：[sign] 替换的是编码过一次的原文，JSON 解析不解码 → 残留 %2B。
    // 含 '%' 说明还剩一层编码，剥掉；表单解析把原样 '+' 吃成空格，Base64 无空格，换回。
    let mut sign_raw = inbox.sign.as_str();
    let sign_decoded;
    if sign_raw.contains('%') {
        sign_decoded = url_decode(sign_raw);
        sign_raw = sign_decoded.as_str();
    }
    let sign_norm = sign_raw.replace(' ', "+");
    let decoded = STANDARD
        .decode(sign_norm.as_bytes())
        .map_err(|_| "sign 不是合法 Base64".to_string())?;
    mac.verify_slice(&decoded)
        .map_err(|_| "sign 校验失败".to_string())?;

    let mut set = seen.lock().unwrap_or_else(|e| e.into_inner());
    // 用归一后的签名做重放键：同一签名换编码形态到达也算重放
    if set.contains(&sign_norm) {
        return Err("重复请求（疑似重放）".into());
    }
    set.insert(sign_norm);
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
        resp.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&resp.body);
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：表单解析已经 urlDecode 过一次，verify 里再解一次会把 Base64 的 '+'
    /// 吃成空格——SHA256 的 Base64 约三分之一概率含 '+'，表现为手机端随机推送失败。
    /// 这里特意构造一个带 '+' 的签名，走完「表单 → 解析 → 校验」全链路。
    #[test]
    fn form_sign_with_plus_survives_parsing() {
        // 找一个签名里带 '+' 的 secret/timestamp 组合
        let ts = win::now_ms().to_string();
        let mut secret = String::from("s");
        let sign = loop {
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
            mac.update(format!("{}\n{}", ts, secret).as_bytes());
            let s = STANDARD.encode(mac.finalize().into_bytes());
            if s.contains('+') {
                break s;
            }
            secret.push('x');
        };
        assert!(sign.contains('+'), "测试前提：签名要包含 '+'");

        // 按表单规范编码：+ -> %2B，/ -> %2F，= -> %3D
        let encoded: String = sign
            .chars()
            .map(|c| match c {
                '+' => "%2B".to_string(),
                '/' => "%2F".to_string(),
                '=' => "%3D".to_string(),
                c => c.to_string(),
            })
            .collect();
        let body = format!("from=10086&content=hi&timestamp={ts}&sign={encoded}");
        let inbox = parse_body(&body, false);

        let cfg = crate::config::Config {
            secret: secret.clone(),
            ..Default::default()
        };
        let seen = Mutex::new(HashSet::new());
        assert!(
            verify(&cfg, &inbox, &seen).is_ok(),
            "带 '+' 的合法签名被拒绝"
        );

        // 同一签名重放要被拦下
        assert!(verify(&cfg, &inbox, &seen).is_err(), "重放未被拦截");
    }

    /// 秒级时间戳（10 位）也要能过校验
    #[test]
    fn seconds_timestamp_is_normalized() {
        let ts = (win::now_ms() / 1000).to_string(); // 秒级
        let secret = "sec";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{}\n{}", ts, secret).as_bytes());
        let sign = STANDARD.encode(mac.finalize().into_bytes());
        let body = format!("from=10086&content=hi&timestamp={ts}&sign={sign}");
        let inbox = parse_body(&body, false);
        let cfg = crate::config::Config {
            secret: secret.into(),
            ..Default::default()
        };
        let seen = Mutex::new(HashSet::new());
        assert!(verify(&cfg, &inbox, &seen).is_ok(), "秒级时间戳被拒");
        assert_eq!(ts_ms(&ts) % 1000, 0, "应已乘 1000");
    }

    /// 容错：客户端没把 '+' 编码成 %2B、原样塞进表单时，表单解析会把它当成空格，
    /// verify 应该无损换回 '+'。
    #[test]
    fn raw_plus_sign_is_tolerated() {
        let ts = win::now_ms().to_string();
        let secret = "fixed-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{}\n{}", ts, secret).as_bytes());
        let sign = STANDARD.encode(mac.finalize().into_bytes());
        let raw: String = sign
            .chars()
            .map(|c| if c == '+' { ' ' } else { c })
            .collect();
        let body = format!("from=10086&content=hi&timestamp={ts}&sign={raw}");
        let inbox = parse_body(&body, false);
        let cfg = crate::config::Config {
            secret: secret.into(),
            ..Default::default()
        };
        let seen = Mutex::new(HashSet::new());
        let r = verify(&cfg, &inbox, &seen);
        // 只在签名确实含 '+' 时这个用例才有意义；不含则本来就过
        assert!(r.is_ok() || !sign.contains('+'));
        if sign.contains('+') {
            assert!(r.is_ok(), "原样 '+' 的签名被拒绝");
        }
    }
}

#[cfg(test)]
mod smsforwarder_compat {
    use super::*;

    /// 模拟官方 SmsForwarder 的编码链路（WebhookUtils.kt 实测源码）：
    /// sign = URLEncoder.encode(Base64(HMAC))，再经 OkHttp FormBody.params 编码一次，
    /// 即「双重 URL 编码」。表单解析解一层后残留 %2B/%3D，verify 必须归一后放行。
    #[test]
    fn double_encoded_sign_is_accepted() {
        let enc = |s: &str| {
            s.chars()
                .map(|c| match c {
                    '+' => "%2B".into(),
                    '/' => "%2F".into(),
                    '=' => "%3D".into(),
                    '%' => "%25".into(),
                    c => c.to_string(),
                })
                .collect::<String>()
        };
        let ts = win::now_ms().to_string();
        let secret = "smsf-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{ts}\n{secret}").as_bytes());
        let sign = STANDARD.encode(mac.finalize().into_bytes());

        // 官方 app：先 URLEncoder 一次，OkHttp 再编一次
        let twice = &enc(&enc(&sign));
        let body = format!("from=10086&content=hi&timestamp={ts}&sign={twice}");
        let inbox = parse_body(&body, false);
        assert!(
            inbox.sign.contains('%'),
            "测试前提：表单解析一层后应仍残留编码"
        );
        let cfg = crate::config::Config {
            secret: secret.into(),
            ..Default::default()
        };
        let seen = Mutex::new(HashSet::new());
        assert!(
            verify(&cfg, &inbox, &seen).is_ok(),
            "官方双重编码的合法签名被拒"
        );

        // 单次编码（我们自测脚本的形态）也必须仍然通过
        let once = &enc(&sign);
        let body = format!("from=10086&content=hi&timestamp={ts}&sign={once}");
        let seen = Mutex::new(HashSet::new());
        assert!(
            verify(&cfg, &inbox_of(&body), &seen).is_ok(),
            "单次编码签名被拒"
        );
    }

    fn inbox_of(body: &str) -> Inbox {
        parse_body(body, false)
    }

    /// JSON 通道（Content-Type: application/json）：[sign] 替换的是编码过一次的
    /// 原文，JSON 解析不做 urlDecode，verify 需要剥掉这一层。
    #[test]
    fn json_channel_encoded_sign_is_accepted() {
        let ts = win::now_ms().to_string();
        let secret = "json-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{ts}\n{secret}").as_bytes());
        let sign = STANDARD.encode(mac.finalize().into_bytes());
        let encoded: String = sign
            .chars()
            .map(|c| match c {
                '+' => "%2B".into(),
                '/' => "%2F".into(),
                '=' => "%3D".into(),
                c => c.to_string(),
            })
            .collect();
        let body =
            format!(r#"{{"from":"10086","content":"hi","timestamp":"{ts}","sign":"{encoded}"}}"#);
        let inbox = parse_body(&body, true);
        let cfg = crate::config::Config {
            secret: secret.into(),
            ..Default::default()
        };
        let seen = Mutex::new(HashSet::new());
        assert!(
            verify(&cfg, &inbox, &seen).is_ok(),
            "JSON 通道的编码签名被拒"
        );
    }

    /// GET 通道：timestamp/sign 在 URL query 里而不是 body。
    #[test]
    fn query_sign_is_merged_into_body() {
        let ts = win::now_ms().to_string();
        let secret = "get-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{ts}\n{secret}").as_bytes());
        let sign = STANDARD.encode(mac.finalize().into_bytes());
        let body = "";
        let query = format!("from=10086&content=hi&timestamp={ts}&sign={sign}&_t=123");
        let merged = merge_query(body, &query);
        assert!(merged.contains("timestamp=") && merged.contains("sign="));
        let inbox = parse_body(&merged, false);
        assert_eq!(inbox.timestamp, ts);
        let cfg = crate::config::Config {
            secret: secret.into(),
            ..Default::default()
        };
        let seen = Mutex::new(HashSet::new());
        assert!(verify(&cfg, &inbox, &seen).is_ok(), "query 携带的签名被拒");
        // body 已有的键不被 query 覆盖
        let merged2 = merge_query("from=99999&content=x", "from=10086&content=hi");
        assert!(merged2.contains("from=99999"));
        // JSON body 不合并
        assert_eq!(merge_query(r#"{"a":1}"#, "t=1"), r#"{"a":1}"#);
    }
}
