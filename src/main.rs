mod config;
mod rules;
mod server;

#[cfg(windows)]
fn enable_utf8_console() {
    extern "system" {
        fn SetConsoleOutputCP(code_page: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
    }
}

fn main() {
    #[cfg(windows)]
    enable_utf8_console();

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
    if cfg.secret.is_empty() {
        println!("  [提醒] 未配置 secret，当前不做来源校验，联调完成后请补上");
    }

    if let Err(e) = server::run(cfg) {
        eprintln!("[fatal] 监听失败: {e}");
        std::process::exit(1);
    }
}
