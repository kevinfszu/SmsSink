use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const APP_DISPLAY_NAME: &str = "SMS 接码器 · PC 版";
pub const APP_ID: &str = "SmsSink";
pub const AUMID: &str = "SmsSink.Desktop";
pub const SINGLE_INSTANCE_MUTEX: &str = "SmsSink.SingleInstance.v1";
pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_HOOK_PATH: &str = "/hook";
pub const SUCCESS_MARK: &str = "success";

static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NotifyMode {
    #[default]
    Toast,
    Card,
    Silent,
}

impl NotifyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            NotifyMode::Toast => "toast",
            NotifyMode::Card => "card",
            NotifyMode::Silent => "silent",
        }
    }
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
            // 默认走自绘卡片：Toast 在「未安装带 AUMID 的开始菜单快捷方式」的环境里
            // 只会静默进入通知中心、不弹横幅，用户会以为没收到。卡片是我们完全掌控的
            // 通道：一定可见、带复制按钮、不抢焦点。
            notify: NotifyMode::Card,
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
    /// 管理台访问令牌；首次启动自动生成并写回配置
    pub admin_token: String,
    /// 自绘卡片停留秒数
    pub card_seconds: u64,
    /// 通知中心最多保留多少条 Toast（超出移除最旧）
    pub toast_limit: usize,
    /// 消息保留天数，0 表示永久保留
    pub retention_days: u32,
    /// 落盘时把原文中的数字打码
    pub mask_content: bool,
    /// SQLite 文件路径；留空则用 %APPDATA%\SmsSink\data.db
    pub db_path: String,
    /// 管理台「最近记录」单次查询上限
    pub history_limit: usize,
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
            admin_token: String::new(),
            card_seconds: 12,
            toast_limit: 30,
            retention_days: 180,
            mask_content: false,
            db_path: String::new(),
            history_limit: 200,
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
    ACTIVE_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| candidates().into_iter().next().unwrap_or_else(|| PathBuf::from("config.json")))
}

pub fn save(cfg: &Config) -> std::io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_string_pretty(cfg).unwrap_or_default())
}

fn random_token() -> String {
    let mut h = Sha256::new();
    h.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    h.update(std::process::id().to_le_bytes());
    h.update(format!("{:?}", std::thread::current().id()).as_bytes());
    h.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn load() -> Config {
    let mut cfg = None;
    for path in candidates() {
        if !path.exists() {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(c) => {
                    println!("[config] 已加载 {}", path.display());
                    let _ = ACTIVE_PATH.set(path);
                    cfg = Some(c);
                    break;
                }
                Err(e) => eprintln!("[config] {} 解析失败: {e}", path.display()),
            },
            Err(e) => eprintln!("[config] {} 读取失败: {e}", path.display()),
        }
    }

    let mut cfg = cfg.unwrap_or_default();
    if ACTIVE_PATH.get().is_none() {
        let path = candidates().into_iter().next().unwrap_or_else(|| PathBuf::from("config.json"));
        let _ = ACTIVE_PATH.set(path);
    }
    if cfg.admin_token.is_empty() {
        cfg.admin_token = random_token();
        println!("[config] 已生成管理台访问 token: {}", cfg.admin_token);
    }
    let _ = save(&cfg);
    cfg
}

pub fn data_db_path(cfg: &Config) -> PathBuf {
    if !cfg.db_path.trim().is_empty() {
        return PathBuf::from(&cfg.db_path);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join(APP_ID).join("data.db")
    } else {
        PathBuf::from("data.db")
    }
}
