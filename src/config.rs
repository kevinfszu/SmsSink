use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const APP_DISPLAY_NAME: &str = "SMS 接码器 · PC 版";
pub const APP_ID: &str = "SmsSink";
pub const AUMID: &str = "SmsSink.Desktop";
pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_HOOK_PATH: &str = "/hook";
pub const SUCCESS_MARK: &str = "success";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NotifyMode {
    #[default]
    Toast,
    Card,
    Silent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    /// 发送方号码 / 包名，包含即命中；留空表示不限来源
    pub match_from: Vec<String>,
    pub keywords_any: Vec<String>,
    pub keywords_all: Vec<String>,
    pub exclude: Vec<String>,
    /// 提取正则，capture 指定取哪个捕获组
    pub extract: String,
    pub capture: usize,
    pub notify: NotifyMode,
    /// 高频共用系统的规则必须保持 false，避免覆盖剪贴板
    pub auto_copy: bool,
    pub copy_template: Option<String>,
    pub ttl_sec: u64,
    pub keep_history: bool,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            name: "短信验证码".into(),
            enabled: true,
            priority: 10,
            match_from: Vec::new(),
            keywords_any: vec!["验证码".into(), "校验码".into(), "动态密码".into()],
            keywords_all: Vec::new(),
            exclude: Vec::new(),
            extract: r"(?:验证码|校验码|动态密码)[^\d]{0,8}(\d{4,8})".into(),
            capture: 1,
            notify: NotifyMode::Toast,
            auto_copy: false,
            copy_template: Some("{code}".into()),
            ttl_sec: 180,
            keep_history: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 0.0.0.0 才能被同局域网的手机访问
    pub listen: String,
    pub port: u16,
    pub hook_path: String,
    /// 与 SmsForwarder Webhook 通道里的 secret 保持一致；留空则关闭校验
    pub secret: String,
    pub timestamp_tolerance_secs: i64,
    pub rules: Vec<Rule>,
    pub keep_unmatched: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0".into(),
            port: DEFAULT_PORT,
            hook_path: DEFAULT_HOOK_PATH.into(),
            secret: String::new(),
            timestamp_tolerance_secs: 300,
            rules: vec![Rule::default()],
            keep_unmatched: false,
        }
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        paths.push(dir.join("config.json"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        paths.push(PathBuf::from(appdata).join(APP_ID).join("config.json"));
    }
    paths
}

pub fn config_path() -> PathBuf {
    candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

pub fn load() -> Config {
    for path in candidates() {
        if !path.exists() {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(cfg) => {
                    println!("[config] 已加载 {}", path.display());
                    return cfg;
                }
                Err(e) => eprintln!("[config] {} 解析失败: {e}", path.display()),
            },
            Err(e) => eprintln!("[config] {} 读取失败: {e}", path.display()),
        }
    }

    let default = Config::default();
    if let Ok(text) = serde_json::to_string_pretty(&default) {
        for path in candidates() {
            if let Some(dir) = path.parent() {
                if fs::create_dir_all(dir).is_err() {
                    continue;
                }
            }
            if fs::write(&path, &text).is_ok() {
                println!("[config] 已生成默认配置 {}", path.display());
                break;
            }
        }
    }
    default
}
