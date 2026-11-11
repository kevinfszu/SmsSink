//! 托盘常驻：唯一 UI 线程 + Win32 消息泵。
//! 空闲时阻塞在 GetMessageW 上，CPU 严格为 0，不设任何轮询定时器。
use crate::{app, event, rules, toast, win};
use crate::win::*;
use std::sync::OnceLock;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

const MENU_ADMIN: usize = 1;
const MENU_RECENT: usize = 2;
const MENU_TEST: usize = 3;
const MENU_RELOAD: usize = 4;
const MENU_AUTOSTART: usize = 5;
const MENU_EXIT: usize = 7;

fn class_name() -> &'static Vec<u16> {
    CLASS_NAME.get_or_init(|| {
        let name = wide("SmsSinkMessageWindow");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance(),
            hIcon: win::tray_icon(),
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: name.as_ptr(),
        };
        unsafe {
            RegisterClassW(&wc);
        }
        name
    })
}

fn add_tray_icon(hwnd: HWND) {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    data.hWnd = hwnd;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = event::WM_APP_TRAY;
    data.hIcon = win::tray_icon();
    let tip = wide(crate::config::APP_DISPLAY_NAME);
    for (i, c) in tip.iter().take(127).enumerate() {
        data.szTip[i] = *c;
    }
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &mut data) == 0 {
            eprintln!("[tray] 添加托盘图标失败（消息窗口仍可接收通知）");
        }
        data.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &mut data);
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    data.hWnd = hwnd;
    data.uID = 1;
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &mut data);
    }
}

fn admin_url(fragment: &str) -> String {
    let cfg = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    format!(
        "http://127.0.0.1:{}/?token={}{}",
        cfg.port, cfg.admin_token, fragment
    )
}

fn test_notification() {
    let cfg = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let sample = "【中国移动统一认证】您的验证码为633382，您正在使用短信验证码登录功能，该验证码仅用于身份验证，3分钟内有效，请勿泄露给他人使用。";
    let decision = rules::evaluate(&cfg, "10086", sample);
    let ev = event::SmsEvent {
        from: "10086".into(),
        content: sample.into(),
        timestamp_ms: win::now_ms(),
        local_time: win::local_time(),
        decision,
    };
    println!("[tray] 发送测试通知");
    event::handle_event(Box::new(ev));
}

fn show_menu(hwnd: HWND) {
    let menu = unsafe { CreatePopupMenu() };
    if menu == 0 {
        return;
    }
    let items: [(usize, &str); 5] = [
        (MENU_ADMIN, "打开管理台"),
        (MENU_RECENT, "最近验证码"),
        (MENU_TEST, "发送测试通知"),
        (MENU_RELOAD, "重载配置"),
        (MENU_AUTOSTART, "开机自动启动"),
    ];
    for (id, text) in items {
        let t = wide(text);
        unsafe {
            AppendMenuW(menu, MF_STRING, id, t.as_ptr());
        }
    }
    unsafe {
        let checked = if win::auto_start_enabled() {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        CheckMenuItem(menu, MENU_AUTOSTART as UINT, checked);
        let sep = wide("");
        AppendMenuW(menu, MF_SEPARATOR, 0, sep.as_ptr());
        let exit = wide("退出");
        AppendMenuW(menu, MF_STRING, MENU_EXIT, exit.as_ptr());

        let mut pt: POINT = std::mem::zeroed();
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        // 让菜单在点击外部时能正常消失
        PostMessageW(hwnd, 0, 0, 0);
        DestroyMenu(menu);
        match cmd as usize {
            MENU_ADMIN => win::open_url(&admin_url("")),
            MENU_RECENT => win::open_url(&admin_url("#recent")),
            MENU_TEST => {
                let _ = PostMessageW(hwnd, event::WM_APP_TEST, 0, 0);
            }
            MENU_RELOAD => match app::reload_config() {
                Ok(()) => println!("[tray] 配置已重载"),
                Err(e) => eprintln!("[tray] 重载失败: {e}"),
            },
            MENU_AUTOSTART => {
                let next = !win::auto_start_enabled();
                match win::set_auto_start(next) {
                    Ok(()) => println!("[tray] 开机自启已{}", if next { "开启" } else { "关闭" }),
                    Err(e) => eprintln!("[tray] 设置开机自启失败: {e}"),
                }
            }
            MENU_EXIT => {
                let _ = PostMessageW(hwnd, event::WM_APP_QUIT, 0, 0);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        x if x == event::WM_APP_SMS => {
            let ev = event::take_event(lparam);
            event::handle_event(ev);
            0
        }
        x if x == event::WM_APP_COPY => {
            let text = event::take_copy_text(lparam);
            if let Err(e) = win::clipboard_set(&text) {
                eprintln!("[tray] 剪贴板写入失败: {e}");
            }
            0
        }
        x if x == event::WM_APP_TEST => {
            test_notification();
            0
        }
        x if x == event::WM_APP_QUIT => {
            remove_tray_icon(hwnd);
            DestroyWindow(hwnd);
            0
        }
        x if x == event::WM_APP_TRAY => {
            let evt = (lparam & 0xFFFF) as u32;
            if evt == WM_RBUTTONUP || evt == WM_LBUTTONDBLCLK {
                show_menu(hwnd);
            }
            0
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn run() {
    let cls = class_name();
    let title = wide(crate::config::APP_DISPLAY_NAME);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            0,
            instance(),
            std::ptr::null_mut(),
        )
    };
    if hwnd == 0 {
        eprintln!("[tray] 创建消息窗口失败，退回无界面模式");
        return;
    }
    TRAY_HWND.store(hwnd, std::sync::atomic::Ordering::SeqCst);
    add_tray_icon(hwnd);
    event::UI_HWND.store(hwnd, std::sync::atomic::Ordering::SeqCst);
    println!("[tray] 托盘已就绪，空闲时阻塞在消息泵上");

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let ret = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    event::UI_HWND.store(0, std::sync::atomic::Ordering::SeqCst);
    toast::clear_all();
    unsafe {
        CoUninitialize();
    }
}
