//! 局域网 IPv4 探测。
//!
//! 目的只有一个：新用户在管理台配手机端时，Webhook 地址里的 IP 不该让他自己去
//! 开 `ipconfig` 抄。默认 0.0.0.0 监听，所以要填的是「手机能访问到的那个地址」。
//!
//! 用 iphlpapi 的 `GetAdaptersInfo`：比 `GetAdaptersAddresses` 的结构简单一个量级，
//! 而这里只需要 IPv4 + 网卡描述 + 介质类型三样东西。枚举不到时退回到
//! 「UDP connect 一下看本机出站地址」这一手纯 std 的兜底——UDP connect 不发包。

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::net::UdpSocket;

const MAX_ADAPTER_NAME_LENGTH: usize = 256;
const MAX_ADAPTER_DESCRIPTION_LENGTH: usize = 128;

const NO_ERROR: u32 = 0;
const ERROR_BUFFER_OVERFLOW: u32 = 111;
const ERROR_NO_DATA: u32 = 232;

const IF_TYPE_IEEE80211: u32 = 71;
const CP_ACP: u32 = 0;

#[repr(C)]
struct IpAddrString {
    Next: *mut IpAddrString,
    IpAddress: [u8; 16],
    IpMask: [u8; 16],
    Context: u32,
}

#[repr(C)]
struct IpAdapterInfo {
    Next: *mut IpAdapterInfo,
    ComboIndex: u32,
    AdapterName: [u8; MAX_ADAPTER_NAME_LENGTH + 4],
    Description: [u8; MAX_ADAPTER_DESCRIPTION_LENGTH + 4],
    AddressLength: u32,
    Address: [u8; 8],
    Index: u32,
    Type: u32,
    DhcpEnabled: u32,
    CurrentIpAddress: *mut IpAddrString,
    IpAddressList: IpAddrString,
    GatewayList: IpAddrString,
    DhcpServer: IpAddrString,
    HaveWins: i32,
    PrimaryWinsServer: IpAddrString,
    SecondaryWinsServer: IpAddrString,
    LeaseObtained: i64,
    LeaseExpires: i64,
}

#[link(name = "iphlpapi")]
extern "system" {
    fn GetAdaptersInfo(info: *mut c_void, size: *mut u32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        src: *const u8,
        src_len: i32,
        dst: *mut u16,
        dst_len: i32,
    ) -> i32;
}

/// 一张网卡上的一个 IPv4 地址。
pub struct LocalIp {
    pub ip: String,
    /// 网卡描述，来自系统（中文系统上是中文名）。只作辅助信息。
    pub name: String,
    /// 无线网卡：手机上最容易连上的通常就是它
    pub wifi: bool,
}

/// 本机所有可用的局域网 IPv4，按「越可能是手机要用的」排前面。
pub fn local_v4() -> Vec<LocalIp> {
    let mut list = unsafe { enum_adapters() };
    if list.is_empty() {
        list = udp_probe();
    }
    // 多网卡时同一个地址可能被列两次（比如同一个网卡既有主地址又有别名）
    list.sort_by(|a, b| rank(b).cmp(&rank(a)).then_with(|| a.ip.cmp(&b.ip)));
    list.dedup_by(|a, b| a.ip == b.ip);
    list
}

fn rank(a: &LocalIp) -> u8 {
    if is_private(&a.ip) {
        if a.wifi {
            3
        } else {
            2
        }
    } else {
        1
    }
}

fn is_private(ip: &str) -> bool {
    if ip.starts_with("192.168.") || ip.starts_with("10.") {
        return true;
    }
    // 172.16.0.0/12
    if let Some(rest) = ip.strip_prefix("172.") {
        if let Some(second) = rest.split('.').next() {
            if let Ok(n) = second.parse::<u8>() {
                return (16..=31).contains(&n);
            }
        }
    }
    false
}

/// 这张地址配不配出现在「填给手机」的候选列表里
fn usable(ip: &str) -> bool {
    !ip.is_empty()
        && ip != "0.0.0.0"
        && !ip.starts_with("127.")
        && !ip.starts_with("169.254.") // 没拿到 DHCP 时的自分配地址，连不通
        && ip.matches('.').count() == 3
}

unsafe fn enum_adapters() -> Vec<LocalIp> {
    let mut size: u32 = 0;
    let rc = GetAdaptersInfo(std::ptr::null_mut(), &mut size);
    // 第一次调用必然以「缓冲区不够」返回，顺便拿到需要的字节数；
    // 一台没有任何网卡的机器会返回 ERROR_NO_DATA
    if rc == ERROR_NO_DATA || size == 0 {
        return Vec::new();
    }
    if rc != ERROR_BUFFER_OVERFLOW {
        return Vec::new();
    }

    // 用 Vec<u64> 承载，保证 8 字节对齐：里面的指针字段直接按结构体解引用
    let words = size as usize / 8 + 2;
    let mut buf = vec![0u64; words];
    let mut size2 = size;
    let rc = GetAdaptersInfo(buf.as_mut_ptr() as *mut c_void, &mut size2);
    if rc != NO_ERROR {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut adapter = buf.as_ptr() as *const IpAdapterInfo;
    let mut guard = 0;
    while !adapter.is_null() && guard < 64 {
        guard += 1;
        let a = &*adapter;
        let name = ansi(&a.Description);
        let wifi = a.Type == IF_TYPE_IEEE80211;

        let mut entry = &a.IpAddressList as *const IpAddrString;
        let mut guard2 = 0;
        while !entry.is_null() && guard2 < 32 {
            guard2 += 1;
            let ip = ansi(&(*entry).IpAddress);
            if usable(&ip) {
                out.push(LocalIp {
                    ip,
                    name: name.clone(),
                    wifi,
                });
            }
            entry = (*entry).Next;
        }
        adapter = a.Next;
    }
    out
}

/// 兜底：UDP connect 不发包，只是让系统替我们选出站网卡，问它本地地址是多少。
fn udp_probe() -> Vec<LocalIp> {
    for target in ["223.5.5.5:53", "8.8.8.8:80"] {
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if sock.connect(target).is_err() {
            continue;
        }
        if let Ok(addr) = sock.local_addr() {
            let ip = addr.ip().to_string();
            if usable(&ip) {
                return vec![LocalIp {
                    ip,
                    name: String::new(),
                    wifi: false,
                }];
            }
        }
    }
    Vec::new()
}

/// 定长 ANSI 缓冲（系统代码页，中文系统上是 GBK）转 Rust 字符串。
fn ansi(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let bytes = &raw[..end];
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        let n = MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; n as usize];
        MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, wide.as_mut_ptr(), n);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不假设一定有网卡（CI / 飞行模式都算合法），只校验返回值的形状：
    /// 每条都能拿去拼 URL，没有回环、没有 0.0.0.0、没有重复。
    #[test]
    fn local_v4_is_usable() {
        let list = local_v4();
        for a in &list {
            println!("{} | {} | wifi={}", a.ip, a.name, a.wifi);
            assert!(usable(&a.ip), "地址不合法: {}", a.ip);
            assert_eq!(a.ip.parse::<std::net::Ipv4Addr>().is_ok(), true, "不是 IPv4: {}", a.ip);
        }
        let mut seen = std::collections::HashSet::new();
        for a in &list {
            assert!(seen.insert(a.ip.clone()), "地址重复: {}", a.ip);
        }
    }
}
