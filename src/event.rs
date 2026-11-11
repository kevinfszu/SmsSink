//! 事件投递：HTTP 线程只负责把消息 PostMessage 给 UI 线程，
//! 弹通知 / 写剪贴板 / 写库全部在持有 STA 消息泵的线程上完成。
use crate::app;
use crate::config::NotifyMode;
use crate::rules::Decision;
use crate::win;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

pub static UI_HWND: AtomicIsize = AtomicIsize::new(0);
pub static HEADLESS: AtomicBool = AtomicBool::new(false);

pub const WM_APP_SMS: u32 = 0x8000 + 1;
pub const WM_APP_COPY: u32 = 0x8000 + 2;
pub const WM_APP_TEST: u32 = 0x8000 + 3;
pub const WM_APP_QUIT: u32 = 0x8000 + 4;
pub const WM_APP_TRAY: u32 = 0x8000 + 5;
/// 第二个实例启动时向已有实例打招呼：让常驻实例给出可见回执，
/// 而不是新进程静默退出（双击没反应会被当成程序坏了）。
pub const WM_APP_POKE: u32 = 0x8000 + 6;

#[derive(Debug, Clone)]
pub struct SmsEvent {
    pub from: String,
    pub content: String,
    pub timestamp_ms: i64,
    pub local_time: String,
    pub decision: Option<Decision>,
    /// 关键词命中、但正则没提取到验证码时的规则信息。
    /// 这一档不能当"没收到"处理——用户等的正是这条短信。
    pub no_code: Option<NoCodeHit>,
}

#[derive(Debug, Clone)]
pub struct NoCodeHit {
    pub rule_name: String,
    pub notify: NotifyMode,
}

/// 从任意线程提交；若 UI 消息窗口不可用则在当前线程同步处理。
pub fn submit(ev: SmsEvent) {
    let hwnd = UI_HWND.load(Ordering::SeqCst);
    if hwnd != 0 {
        let ptr = Box::into_raw(Box::new(ev)) as isize;
        if unsafe { win::PostMessageW(hwnd, WM_APP_SMS, 0, ptr) } != 0 {
            return;
        }
        // 投递失败则原地回收这个 Box，避免泄漏
        handle_event(unsafe { Box::from_raw(ptr as *mut SmsEvent) });
        return;
    }
    handle_event(Box::new(ev));
}

/// UI 线程收到 WM_APP_SMS 时调用
pub unsafe fn take_event(lparam: isize) -> Box<SmsEvent> {
    Box::from_raw(lparam as *mut SmsEvent)
}

/// 从任意线程请求写剪贴板，优先投递到 UI 线程执行
pub fn request_copy(text: &str) -> Result<(), String> {
    let hwnd = UI_HWND.load(Ordering::SeqCst);
    if hwnd != 0 {
        let ptr = Box::into_raw(Box::new(text.to_string())) as isize;
        if unsafe { win::PostMessageW(hwnd, WM_APP_COPY, 0, ptr) } != 0 {
            return Ok(());
        }
        unsafe {
            drop(Box::from_raw(ptr as *mut String));
        }
    }
    win::clipboard_set(text)
}

pub unsafe fn take_copy_text(lparam: isize) -> Box<String> {
    Box::from_raw(lparam as *mut String)
}

/// 核心处理：落盘 → 自动复制 → 按规则通知
pub fn handle_event(ev: Box<SmsEvent>) {
    let state = app::get();
    let decision = ev.decision.clone();
    let cfg = state.cfg.read().unwrap_or_else(|e| e.into_inner());

    // 规则可单独决定是否留痕；未命中的短信默认不入库，
    // 但"命中却没提取到码"的一律留痕：它通常是规则该改的信号，丢了就查不到。
    let keep = match (&decision, &ev.no_code) {
        (Some(d), _) => d.keep_history,
        (None, Some(_)) => true,
        (None, None) => cfg.keep_unmatched,
    };
    let stored_content = if cfg.mask_content {
        crate::store::mask_digits(&ev.content, decision.as_ref().map(|d| d.code.as_str()))
    } else {
        ev.content.clone()
    };
    let id = if keep {
        crate::store::record(&ev, &stored_content, false)
    } else {
        0
    };
    drop(cfg);

    let Some(d) = decision else {
        // 命中了规则却没提取到验证码：这是"收到了但没读懂"，必须让用户看见。
        // 静默规则保持静默（用户明确说过别打扰），其余一律给一张降级卡片。
        if let Some(hit) = ev.no_code.clone() {
            println!(
                "[sink]   {} 命中规则「{}」但未提取到验证码",
                ev.local_time, hit.rule_name
            );
            if HEADLESS.load(Ordering::SeqCst) {
                return;
            }
            match hit.notify {
                NotifyMode::Silent => println!("[sink]   静默规则，仅写入最近记录"),
                _ => crate::card::show_unparsed(&ev, &ev.content),
            }
            return;
        }
        println!("[sink]   {} 未命中任何规则", ev.local_time);
        return;
    };

    let delay_ms = win::now_ms() - ev.timestamp_ms;
    if (0..=3_600_000).contains(&delay_ms) {
        println!("[sink]   推送延迟 {delay_ms} ms");
    }

    println!(
        "[sink]   {} 规则「{}」→ 提取 {} | 通知={} | 自动复制={}",
        ev.local_time,
        d.rule_name,
        d.code,
        d.notify.as_str(),
        d.auto_copy
    );

    if d.auto_copy && !HEADLESS.load(Ordering::SeqCst) {
        match win::clipboard_set(&d.copy_text) {
            Ok(()) => {
                println!("[sink]   已写入剪贴板（auto_copy=true）");
                crate::store::mark_copied(id);
            }
            Err(e) => eprintln!("[sink]   剪贴板写入失败: {e}"),
        }
    }

    if HEADLESS.load(Ordering::SeqCst) {
        return;
    }

    match d.notify {
        NotifyMode::Toast => {
            crate::toast::show(&d, &ev, id);
        }
        NotifyMode::Card => {
            crate::card::show(&d, &ev, id);
        }
        NotifyMode::Silent => {
            println!("[sink]   静默规则，仅写入最近记录");
        }
    }
}
