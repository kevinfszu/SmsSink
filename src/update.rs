//! 新版本提示。
//!
//! SmsSink 没有内置更新器（离线安装包 + 手工覆盖安装是这个项目的取舍），
//! 所以装在用户机器上的旧版本永远不会自己变成新版本。这个模块只做一件事：
//! 拿 GitHub 上最新 release 的版本号和当前 exe 比一比，更高就在管理台顶部挂一条提示。
//!
//! 三条自我约束：
//! - **失败一律静默**。国内直连 api.github.com 常年不通，不能因为它连不上就在界面上抱怨。
//! - **一天最多问一次**。未鉴权的 GitHub API 按 IP 限 60 次/小时，别给用户惹麻烦；
//!   结果连同时间戳落到 update.json，重启后不必重问。
//! - **只在打开管理台时问**。没人看的时候检查出结果也没人看，还白耗流量。

use crate::http;
use crate::{app, config};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// 默认检查地址。可用 config.json 的 `update_url` 换成镜像
/// （比如国内自建的反代），换掉之后仍然按同样的 JSON 结构解析。
pub const DEFAULT_URL: &str = "https://api.github.com/repos/kevinfszu/SmsSink/releases/latest";
/// 拿不到下载地址时的兜底落地页
pub const RELEASES_PAGE: &str = "https://github.com/kevinfszu/SmsSink/releases";

/// 成功检查过之后的冷却期：一天
const CHECK_GAP: i64 = 24 * 3600;
/// 失败后的重试间隔：十分钟。不落盘，所以重启程序会立刻再试一次
const RETRY_GAP: i64 = 10 * 60;
/// 单次请求的超时。这个功能不值得让用户多等一秒
const TIMEOUT_MS: i32 = 6000;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Release {
    /// 去掉 v 前缀的版本号，例如 "0.3.1"
    pub version: String,
    /// 发布页地址（只接受 https，避免异常镜像把按钮指向 javascript:）
    pub url: String,
    /// 发布日期，YYYY-MM-DD
    pub published: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cache {
    /// 上次成功检查的时间戳（unix 秒），0 表示从没成功过
    checked_at: i64,
    /// 用户点了「忽略此版本」的版本号；出现更新的版本还会再提示
    skipped: String,
    release: Option<Release>,
}

#[derive(Default)]
struct State {
    cache: Cache,
    /// 最近一次失败原因。刻意不落盘：重启后是一次全新的尝试，不该背着上次的错误
    error: Option<String>,
    /// 上次发起检查的时间（含失败），用来压住失败后的重试频率
    last_attempt: i64,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static BUSY: AtomicBool = AtomicBool::new(false);

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            cache: load_cache(),
            ..State::default()
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// 就放在配置文件旁边：便携版跟着 exe 走，安装版留在 %APPDATA%\SmsSink
fn cache_path() -> PathBuf {
    config::config_path().with_file_name("update.json")
}

fn load_cache() -> Cache {
    fs::read_to_string(cache_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Cache>(&t).ok())
        .unwrap_or_default()
}

fn save_cache(cache: &Cache) {
    let path = cache_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(path, text);
    }
}

fn enabled() -> bool {
    app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .check_update
}

fn target_url() -> String {
    let custom = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .update_url
        .trim()
        .to_string();
    if custom.is_empty() {
        DEFAULT_URL.to_string()
    } else {
        custom
    }
}

/// 版本号拆成三段数字。`v0.3.0-rc1` / `0.3.0+meta` 这种都按 0.3.0 处理——
/// 这里只需要判断「是不是更新了」，不需要完整的 semver 语义。
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let core = v.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn parse_release(text: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("返回的不是 JSON：{e}"))?;
    let tag = v
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if tag.is_empty() {
        return Err("返回内容里没有版本号".into());
    }
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    if parse_version(&version).is_none() {
        return Err(format!("版本号无法识别：{tag}"));
    }
    let url = v
        .get("html_url")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("https://"))
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    // published_at 是 ISO 8601（2026-11-12T03:00:00Z），只留日期
    let published = v
        .get("published_at")
        .and_then(Value::as_str)
        .and_then(|s| s.get(..10))
        .unwrap_or("")
        .to_string();
    Ok(Release {
        version,
        url,
        published,
    })
}

fn fetch(url: &str) -> Result<Release, String> {
    let resp = http::get(url, TIMEOUT_MS)?;
    if resp.status != 200 {
        return Err(format!("HTTP {}", resp.status));
    }
    parse_release(&resp.text())
}

fn finish(result: Result<Release, String>, clear_skip: bool) {
    let mut st = lock();
    match result {
        Ok(r) => {
            st.cache.checked_at = now();
            st.cache.release = Some(r);
            st.error = None;
            // 用户主动点「检查更新」等于明确说"我要看"：把之前的忽略解掉，
            // 否则他明明主动问了却还是什么都看不到
            if clear_skip {
                st.cache.skipped.clear();
            }
            save_cache(&st.cache);
        }
        // 失败不写 checked_at：下次打开管理台会再试一次，不能因为一次断网就把提示压一天
        Err(e) => st.error = Some(e),
    }
}

/// 该问就问，问也异步：绝不能因为检查更新把管理台的响应拖慢几秒
pub fn maybe_check() {
    if !enabled() {
        return;
    }
    let at = now();
    {
        let mut st = lock();
        if at - st.cache.checked_at < CHECK_GAP {
            return;
        }
        if st.last_attempt != 0 && at - st.last_attempt < RETRY_GAP {
            return;
        }
        st.last_attempt = at;
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let url = target_url();
    std::thread::spawn(move || {
        let result = fetch(&url);
        finish(result, false);
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// 用户主动点了「检查更新」：同步等结果，好让界面能立刻说清成了还是败了
pub fn check_now() -> Value {
    if BUSY.swap(true, Ordering::SeqCst) {
        // 后台正好在跑，别发两个请求；让用户过一会儿再看
        return status();
    }
    {
        let mut st = lock();
        st.last_attempt = now();
    }
    let url = target_url();
    let result = fetch(&url);
    finish(result, true);
    BUSY.store(false, Ordering::SeqCst);
    status()
}

/// 忽略某个版本：横幅收起来，等出现更高的版本再提示
pub fn skip(version: &str) -> Value {
    {
        let mut st = lock();
        st.cache.skipped = version.trim().trim_start_matches(['v', 'V']).to_string();
        save_cache(&st.cache);
    }
    status()
}

pub fn status() -> Value {
    let st = lock();
    let current = env!("CARGO_PKG_VERSION");
    let release = st.cache.release.clone();
    let latest = release.as_ref().map(|r| r.version.clone());
    let available = release
        .as_ref()
        .map(|r| is_newer(&r.version, current) && r.version != st.cache.skipped)
        .unwrap_or(false);
    json!({
        "current": current,
        "latest": latest,
        "available": available,
        "skipped": st.cache.skipped.clone(),
        "url": release.as_ref().map(|r| r.url.clone()).unwrap_or_default(),
        "published": release.as_ref().map(|r| r.published.clone()).unwrap_or_default(),
        "checked_at": st.cache.checked_at,
        "checking": BUSY.load(Ordering::SeqCst),
        "error": st.error.clone(),
        "enabled": enabled(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("0.4.0", "0.3.9"));
        assert!(is_newer("1.0", "0.99.99"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        // 数字比大小，不是字符串比大小
        assert!(is_newer("0.10.0", "0.9.0"));
    }

    #[test]
    fn version_tolerates_prefix_and_suffix() {
        assert_eq!(parse_version("v0.3.0"), Some((0, 3, 0)));
        assert_eq!(parse_version("0.3"), Some((0, 3, 0)));
        assert_eq!(parse_version("0.3.0-rc1"), Some((0, 3, 0)));
        assert_eq!(parse_version("0.3.0+meta"), Some((0, 3, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn release_json_is_parsed() {
        let text = r#"{
            "tag_name": "v0.3.1",
            "html_url": "https://github.com/kevinfszu/SmsSink/releases/tag/v0.3.1",
            "published_at": "2026-11-20T08:30:00Z",
            "body": "这次改了什么"
        }"#;
        let r = parse_release(text).unwrap();
        assert_eq!(r.version, "0.3.1");
        assert_eq!(r.published, "2026-11-20");
        assert!(r.url.ends_with("/tag/v0.3.1"));
    }

    /// 镜像被人指歪了也不能让横幅上的按钮变成 javascript:
    #[test]
    fn release_url_must_be_https() {
        let text = r#"{"tag_name":"v9.9.9","html_url":"javascript:alert(1)"}"#;
        let r = parse_release(text).unwrap();
        assert_eq!(r.url, RELEASES_PAGE);
        assert_eq!(r.published, "");
    }

    #[test]
    fn release_json_rejects_junk() {
        assert!(parse_release("<html>404</html>").is_err());
        assert!(parse_release(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse_release(r#"{"tag_name":"latest"}"#).is_err());
    }
}
