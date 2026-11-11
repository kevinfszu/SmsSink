mod admin;
mod app;
mod card;
mod config;
mod event;
mod rules;
mod server;
mod store;
mod toast;
mod tray;
mod win;

use std::net::TcpListener;
use std::sync::atomic::Ordering;

#[cfg(windows)]
fn enable_utf8_console() {
    extern "system" {
        fn SetConsoleOutputCP(code_page: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
    }
}

fn usage() {
    println!(
        "用法: SmsSink.exe [选项]\n  \
         --no-ui     无界面模式（不创建托盘/通知，仅供联调与自动化）\n  \
         --help      显示本帮助"
    );
}

fn main() {
    #[cfg(windows)]
    enable_utf8_console();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
        return;
    }
    let headless = args.iter().any(|a| a == "--no-ui");

    let cfg = config::load();
    println!("{}  v{}", config::APP_DISPLAY_NAME, env!("CARGO_PKG_VERSION"));
    println!(
        "  标识 {} · AUMID {} · 配置 {}",
        config::APP_ID,
        config::AUMID,
        config::config_path().display()
    );
    println!(
        "  管理台 http://127.0.0.1:{}/?token={}",
        cfg.port, cfg.admin_token
    );
    if cfg.secret.is_empty() {
        println!("  [提醒] 未配置 secret，当前不做来源校验，联调完成后请补上");
    }

    if !win::claim_single_instance(config::SINGLE_INSTANCE_MUTEX) {
        eprintln!("[fatal] 已有 SmsSink 实例在运行，本进程退出");
        std::process::exit(1);
    }

    // COM 必须在将要使用 WinRT Toast 的线程（主/UI 线程）上初始化
    if !win::init_com() {
        eprintln!("[warn] COM 初始化失败，Toast 通知可能不可用");
    }
    win::set_aumid(config::AUMID, config::APP_DISPLAY_NAME);

    let db_path = config::data_db_path(&cfg);
    if let Err(e) = store::init(&db_path) {
        eprintln!("[fatal] 数据库初始化失败: {e}");
        std::process::exit(1);
    }
    println!("  数据 {}", db_path.display());
    store::purge(cfg.retention_days);

    // 提前占位检测，端口冲突时直接报错而不是让后台线程悄悄退出
    if let Err(e) = TcpListener::bind((cfg.listen.as_str(), cfg.port)) {
        eprintln!("[fatal] 端口 {}:{} 不可用: {e}", cfg.listen, cfg.port);
        std::process::exit(1);
    }

    let state = app::init(cfg);
    event::HEADLESS.store(headless, Ordering::SeqCst);

    if headless {
        println!("[sink] 无界面模式：只接收与落盘，不弹通知、不写剪贴板");
        if let Err(e) = server::run(state) {
            eprintln!("[fatal] 监听失败: {e}");
            std::process::exit(1);
        }
        return;
    }

    let worker_state = std::sync::Arc::clone(&state);
    std::thread::spawn(move || {
        if let Err(e) = server::run(worker_state) {
            eprintln!("[fatal] 监听失败: {e}");
            std::process::exit(1);
        }
    });
    tray::run();
}
