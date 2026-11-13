//! 极简 HTTP 客户端：一次 GET，只为「检查新版本」服务。
//!
//! 不引 ureq / rustls 的两个理由：
//! 1. **体积**。整个 exe 才 5 MiB，塞一套 TLS 栈加根证书要再多出 1.5–2 MiB，
//!    而这里一年也就发几十个请求。
//! 2. **能连出去**。WinHTTP 走的是系统代理设置，国内用户基本都挂着 Clash / 公司代理，
//!    自己带根证书的客户端默认不认系统代理，反而更容易连不上 api.github.com。
//!
//! 代价是只能跑在 Windows 上——这正是本项目的目标平台，不算损失。

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};

type Handle = *mut c_void;
/// LPCWSTR：UTF-16、以 0 结尾
type Wide = *const u16;

/// 走当前用户的系统代理（注册表里的 WinINET 设置）
const ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
/// 直连。本机地址必须用它：系统代理常常把 127.0.0.1 也转走，转一圈回来连不上
const ACCESS_TYPE_NO_PROXY: u32 = 1;
const FLAG_SECURE: u32 = 0x0080_0000;
const QUERY_STATUS_CODE: u32 = 19;
/// 让 WinHttpQueryHeaders 按 DWORD 而不是字符串返回
const FLAG_NUMBER: u32 = 0x2000_0000;

/// 响应体的硬上限。更新检查的响应只有几百字节，
/// 万一 update_url 被指到别的地方，也不至于在这里把内存吃光。
const MAX_BODY: usize = 256 * 1024;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(agent: Wide, access: u32, proxy: Wide, bypass: Wide, flags: u32) -> Handle;
    fn WinHttpConnect(session: Handle, host: Wide, port: u16, reserved: u32) -> Handle;
    fn WinHttpOpenRequest(
        connect: Handle,
        verb: Wide,
        object: Wide,
        version: Wide,
        referrer: Wide,
        accept_types: *const Wide,
        flags: u32,
    ) -> Handle;
    fn WinHttpSendRequest(
        request: Handle,
        headers: Wide,
        headers_len: u32,
        optional: *const c_void,
        optional_len: u32,
        total_len: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: Handle, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: Handle,
        info_level: u32,
        name: Wide,
        buffer: *mut c_void,
        len: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpReadData(request: Handle, buffer: *mut c_void, to_read: u32, read: *mut u32) -> i32;
    fn WinHttpSetTimeouts(
        handle: Handle,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> i32;
    fn WinHttpCloseHandle(handle: Handle) -> i32;
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 已打开的句柄，离开作用域自动关闭。WinHTTP 的句柄有严格的父子关闭顺序，
/// 手写 Close 很容易在某条 early return 上漏掉，所以统一交给 Drop。
struct Owned(Handle);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 把 URL 拆成 WinHTTP 要的四件东西。只支持 http / https，够用了。
struct Url {
    secure: bool,
    host: String,
    port: u16,
    /// 从 `/` 开始，含查询串
    path: String,
}

impl Url {
    fn parse(raw: &str) -> Result<Self, String> {
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or("地址缺少 http:// 或 https://")?;
        let secure = match scheme.to_ascii_lowercase().as_str() {
            "https" => true,
            "http" => false,
            other => return Err(format!("不支持的协议：{other}")),
        };
        // 主机名到第一个 `/` 或 `?` 为止。裸的 `https://host?x=1` 也要拼成 `/?x=1`，
        // 否则查询串会被当成主机名的一部分
        let split = rest.find(['/', '?']);
        let (hostport, path) = match split {
            Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
            Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
            None => (rest, "/".to_string()),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().map_err(|_| "端口号不合法")?),
            None => (hostport, if secure { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err("地址里没有主机名".into());
        }
        Ok(Self {
            secure,
            host: host.to_string(),
            port,
            path: path.to_string(),
        })
    }

    fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "[::1]")
    }
}

/// GET 一个地址，返回状态码与响应体。
///
/// `timeout_ms` 同时作为解析 / 连接 / 发送的超时，接收超时取它的两倍——
/// 下载 Body 可能分多段到达，按单次操作计时更合理。
pub fn get(url: &str, timeout_ms: i32) -> Result<Response, String> {
    let u = Url::parse(url)?;
    let access = if u.is_loopback() {
        ACCESS_TYPE_NO_PROXY
    } else {
        ACCESS_TYPE_DEFAULT_PROXY
    };
    unsafe {
        let agent = wide("SmsSink");
        let session = Owned(WinHttpOpen(agent.as_ptr(), access, null(), null(), 0));
        if session.0.is_null() {
            return Err(last_error());
        }
        WinHttpSetTimeouts(
            session.0,
            timeout_ms,
            timeout_ms,
            timeout_ms,
            timeout_ms.saturating_mul(2),
        );

        let host = wide(&u.host);
        let connect = Owned(WinHttpConnect(session.0, host.as_ptr(), u.port, 0));
        if connect.0.is_null() {
            return Err(last_error());
        }

        let verb = wide("GET");
        let object = wide(&u.path);
        let request = Owned(WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            object.as_ptr(),
            null(),
            null(),
            null(),
            if u.secure { FLAG_SECURE } else { 0 },
        ));
        if request.0.is_null() {
            return Err(last_error());
        }

        // GitHub 的 API 要求带 Accept 与 User-Agent，缺 UA 会被 403；
        // dwHeadersLength 传 -1 表示「按 0 结尾自己数」
        let headers = wide("Accept: application/vnd.github+json\r\nUser-Agent: SmsSink\r\n");
        if WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) == 0 {
            return Err(last_error());
        }
        if WinHttpReceiveResponse(request.0, null_mut()) == 0 {
            return Err(last_error());
        }

        // 查不到状态码不算失败：默认 0，调用方看到非 200 会当作错误处理
        let mut status: u32 = 0;
        let mut len: u32 = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            QUERY_STATUS_CODE | FLAG_NUMBER,
            null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            null_mut(),
        );

        let mut body = Vec::new();
        loop {
            let mut buf = [0u8; 8192];
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut read,
            ) == 0
            {
                // 读到一半出错：已经拿到的部分仍然有值（错误响应体就是有用的），先返回
                break;
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
            if body.len() >= MAX_BODY {
                break;
            }
        }

        Ok(Response {
            status: status as u16,
            body,
        })
    }
}

unsafe fn last_error() -> String {
    describe(crate::win::GetLastError())
}

/// WinHTTP 的错误码都是 12000 段的，直接甩给用户看等于没说，
/// 挑几个在这种情况下真会遇到的翻成人话。
fn describe(code: u32) -> String {
    let hint = match code {
        12002 => "请求超时",
        12006 => "连接被中断",
        12007 => "域名解析失败",
        12029 => "连不上服务器",
        12175 | 12037 => "TLS 握手失败（证书或协议不兼容）",
        _ => "网络错误",
    };
    format!("{hint}（WinHTTP {code}）")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_splits_host_and_path() {
        let u = Url::parse("https://api.github.com/repos/a/b/releases/latest").unwrap();
        assert!(u.secure);
        assert_eq!(u.host, "api.github.com");
        assert_eq!(u.port, 443);
        assert_eq!(u.path, "/repos/a/b/releases/latest");
    }

    #[test]
    fn url_defaults() {
        let u = Url::parse("http://127.0.0.1:8787").unwrap();
        assert!(!u.secure);
        assert_eq!(u.port, 8787);
        assert_eq!(u.path, "/");
        assert!(u.is_loopback());

        let u = Url::parse("https://example.com?x=1").unwrap();
        assert_eq!(u.port, 443);
        assert_eq!(u.host, "example.com");
        assert_eq!(u.path, "/?x=1");
    }

    #[test]
    fn url_rejects_junk() {
        assert!(Url::parse("api.github.com/x").is_err());
        assert!(Url::parse("ftp://x/y").is_err());
        assert!(Url::parse("https://:80/x").is_err());
        assert!(Url::parse("https://x:abc/y").is_err());
    }
}
