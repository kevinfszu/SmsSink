// 双击启动不弹控制台窗口：Release 以 GUI 子系统链接。
// 调试构建保留控制台，方便 cargo run 直接看日志；release 从终端启动时会挂回
// 父进程控制台（见 win::init_stdio）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod admin;
mod app;
mod card;
mod config;
mod event;
mod font;
mod hint;
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
         --no-ui       无界面模式（不创建托盘/通知，仅供联调与自动化）\n  \
         --probe-ui    跳过单实例检查，用于在正式实例运行时预览新版通知（调试用）\n  \
         --help        显示本帮助"
    );
}

fn main() {
    // 必须在创建任何窗口 / 取任何 DC 之前声明 DPI 感知，否则整个进程会被
    // 系统按 96 DPI 渲染后再位图放大，字体边缘发虚（细节见 win::init_dpi_awareness）
    #[cfg(windows)]
    win::init_dpi_awareness();

    // 必须在任何输出之前：GUI 子系统进程默认没有可写的 stdout
    #[cfg(windows)]
    win::init_stdio();
    #[cfg(windows)]
    enable_utf8_console();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
        return;
    }
    let headless = args.iter().any(|a| a == "--no-ui");
    // 调试用：允许与正式实例并存，便于在不打断使用的情况下预览界面
    let probe_ui = args.iter().any(|a| a == "--probe-ui");

    let cfg = config::load();
    println!(
        "{}  v{}",
        config::APP_DISPLAY_NAME,
        env!("CARGO_PKG_VERSION")
    );
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
    let dpi = win::system_dpi();
    println!(
        "  界面字体 {} · DPI {} ({:.0}%)",
        String::from_utf16_lossy(font::ui_face()),
        dpi,
        dpi as f64 / 96.0 * 100.0
    );
    if cfg.secret.is_empty() {
        println!("  [提醒] 未配置 secret，当前不做来源校验，联调完成后请补上");
    }

    if !probe_ui && !win::claim_single_instance(config::SINGLE_INSTANCE_MUTEX) {
        // 双击第二份时静默退出，等于「点了没反应」——用户会以为程序坏了。
        // 已有实例就在托盘里，让它自己冒一条提示，比新进程猜怎么反馈更靠谱。
        println!("[info] 已有 SmsSink 实例在运行，通知它给出提示");
        if !headless {
            let hwnd = win::find_window(tray::WINDOW_CLASS);
            if hwnd != 0 {
                unsafe {
                    win::PostMessageW(hwnd, event::WM_APP_POKE, 0, 0);
                }
            }
        }
        std::process::exit(0);
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
        // 双击启动时没有控制台，静默退出会让用户以为程序坏了：必须给一个可见的说明
        if !headless {
            win::error_box(
                &format!(
                    "端口 {} 被占用，SmsSink 无法启动。\n\n\
                     常见原因：\n\
                     · 已经有一个 SmsSink 在运行（看右下角托盘）\n\
                     · 该端口被其它程序占用\n\n\
                     如果是端口冲突，把配置里的 port 改成一个空闲端口再启动：\n{}",
                    cfg.port,
                    config::config_path().display()
                ),
                &format!("{} · 启动失败", config::APP_DISPLAY_NAME),
            );
        }
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
