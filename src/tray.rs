//! 托盘常驻：唯一 UI 线程 + Win32 消息泵。
//! 空闲时阻塞在 GetMessageW 上，CPU 严格为 0，不设任何轮询定时器。
//!
//! 托盘是这个程序唯一的常驻界面。以 GUI 子系统启动时 stdout 指向 NUL，
//! 只写日志的回执等于没有回执：每个菜单动作都必须通过气泡给出可见结果。
use crate::win::*;
use crate::{app, config, event, hint, rules, store, toast, win};
use std::sync::OnceLock;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// 常驻实例的消息窗口类名。第二个实例靠它在进程间投递「我来了」，
/// 所以这里是跨进程契约，改名要同步 `main.rs` 的查找逻辑（两边共用本常量）。
pub const WINDOW_CLASS: &str = "SmsSinkMessageWindow";

// id 必须唯一且稳定：TrackPopupMenu 用 TPM_RETURNCMD 直接返回 id（不发 WM_COMMAND），
// 所以取值不必连续，但要避开「最近验证码」占用的 200 段。
const MENU_ADMIN: usize = 1;
const MENU_TEST: usize = 3;
const MENU_RELOAD: usize = 4;
const MENU_AUTOSTART: usize = 5;
const MENU_EXIT: usize = 7;
/// 「最近验证码」子菜单项 id = 200 + store::recent 返回 Vec 的下标
const MENU_RECENT_BASE: usize = 200;
/// 子菜单最多列 3 条：再多就需要滚屏，反而不如直接开管理台
const RECENT_LIMIT: usize = 3;

fn class_name() -> &'static Vec<u16> {
    CLASS_NAME.get_or_init(|| {
        let name = wide(WINDOW_CLASS);
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
    data.uID = win::TRAY_ICON_ID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = event::WM_APP_TRAY;
    data.hIcon = win::tray_icon();
    let tip = wide(config::APP_DISPLAY_NAME);
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
    // 静态 tip 只有产品名；启动时就补一次状态，用户悬停即可确认程序在干活
    update_tooltip(hwnd);
}

/// 把悬停提示刷新成「产品名 — 监听 8787 · 今日 12 条」。
///
/// 只在创建图标和弹出菜单时调用：状态是"打开菜单那一刻"的快照即可，
/// 不需要常驻定时器，空闲 CPU 保持 0。
fn update_tooltip(hwnd: HWND) {
    // 悬停提示以应用名开头：用户第一眼要确认"这是哪个程序"，
    // 状态是第二层信息。两者用「·」分开，长度控制在不会被 Shell 截断的范围。
    let text = format!("{} · {}", config::APP_DISPLAY_NAME, status_text());
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    data.hWnd = hwnd;
    data.uID = win::TRAY_ICON_ID;
    data.uFlags = NIF_TIP | NIF_SHOWTIP;
    data.uVersion = NOTIFYICON_VERSION_4;
    // szTip 含结尾 NUL 共 128 个宽字符，超出被 Shell 截断，这里主动裁齐
    for (i, unit) in wide(&text).iter().take(127).enumerate() {
        data.szTip[i] = *unit;
    }
    unsafe {
        Shell_NotifyIconW(NIM_MODIFY, &mut data);
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    data.hWnd = hwnd;
    data.uID = win::TRAY_ICON_ID;
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

/// 从 `YYYY-MM-DD HH:MM:SS` 里截出 `HH:MM`。格式异常时原样返回，绝不 panic。
fn hh_mm(local_time: &str) -> &str {
    local_time.get(11..16).unwrap_or(local_time)
}

/// 状态行 / 悬停提示共用：`监听 8787 · 今日 12 条[ · 最后 14:32]`
fn status_text() -> String {
    let state = app::get();
    let port = {
        let cfg = state.cfg.read().unwrap_or_else(|e| e.into_inner());
        cfg.port
    };
    let (today, last) = store::today_summary();
    let mut text = format!("监听 {port} · 今日 {today} 条");
    if !last.is_empty() {
        text.push_str(" · 最后 ");
        text.push_str(hh_mm(&last));
    }
    text
}

fn short_rule(name: &str) -> String {
    const LIMIT: usize = 10;
    if name.chars().count() <= LIMIT {
        return name.to_string();
    }
    let head: String = name.chars().take(LIMIT).collect();
    format!("{head}…")
}

/// 子菜单单行：`{code} · {HH:MM} · {rule}`。规则名过长会挤掉验证码本身，
/// 所以截断到 10 字，保证一眼就能读到码。
fn recent_label(row: &store::Row) -> String {
    format!(
        "{} · {} · {}",
        row.extracted,
        hh_mm(&row.local_time),
        short_rule(&row.rule_name)
    )
}

fn test_notification() {
    let cfg = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let sample = "【中国移动统一认证】您的验证码为633382，您正在使用短信验证码登录功能，该验证码仅用于身份验证，3分钟内有效，请勿泄露给他人使用。";
    // 用真实判定链路：这样"测试通知"同时能验证规则是否还匹配得上，
    // 匹配不到时会走未识别的降级卡片，而不是假装成功。
    let (decision, no_code) = match rules::evaluate_outcome(&cfg, "10086", sample) {
        rules::Outcome::Code(d) => (Some(d), None),
        rules::Outcome::NoCode { rule_name, notify } => (
            None,
            Some(event::NoCodeHit { rule_name, notify }),
        ),
        rules::Outcome::NoMatch => (None, None),
    };
    let ev = event::SmsEvent {
        from: "10086".into(),
        content: sample.into(),
        timestamp_ms: win::now_ms(),
        local_time: win::local_time(),
        decision,
        no_code,
    };
    println!("[tray] 发送测试通知");
    event::handle_event(Box::new(ev));
}

/// 重载配置。规则数放进气泡正文，让用户确认读到的是改过的那一份。
fn reload_with_feedback() {
    match app::reload_config() {
        Ok(()) => {
            let state = app::get();
            let n = state
                .cfg
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .rules
                .len();
            hint::show(&format!("配置已重载 · {n} 条规则生效"), hint::Tone::Info);
        }
        Err(e) => hint::show(&format!("重载失败：{e}"), hint::Tone::Warn),
    }
}

/// 切换开机自启。只有注册表真的写成功才回执成功，
/// 下一次弹菜单的勾选状态因此永远反映真实结果。
fn toggle_auto_start() {
    let next = !win::auto_start_enabled();
    match win::set_auto_start(next) {
        Ok(()) => {
            if next {
                hint::show("已开启开机自启", hint::Tone::Info);
            } else {
                hint::show("已关闭开机自启", hint::Tone::Info);
            }
        }
        Err(e) => hint::show(&format!("设置失败：{e}"), hint::Tone::Warn),
    }
}

/// 复制一条验证码并回执。失败必须显眼：用户会默认码已经在剪贴板里，
/// 静默失败会让他在别处粘贴出一个旧值。
fn copy_code(row: &store::Row) {
    match win::clipboard_set(&row.extracted) {
        Ok(()) => {
            store::mark_copied(row.id);
            hint::show(&format!("已复制 {}", row.extracted), hint::Tone::Info);
        }
        Err(e) => hint::show(&format!("复制失败：{e}"), hint::Tone::Warn),
    }
}

fn show_menu(hwnd: HWND) {
    // 状态行与悬停提示同源，弹出前刷一次即可
    update_tooltip(hwnd);
    let status = status_text();

    let menu = unsafe { CreatePopupMenu() };
    if menu == 0 {
        return;
    }
    win::menu_status(menu, &status);
    win::menu_separator(menu);

    // 最高频需求是"把刚看到的码复制走"：做成一跳可达的子菜单，
    // 不必为了复制一个码去开浏览器再点管理台的锚点。
    let recent_menu = unsafe { CreatePopupMenu() };
    let rows = store::recent(RECENT_LIMIT, 0, "", "", "");
    let mut shown = 0usize;
    for (index, row) in rows.iter().enumerate() {
        // 没提取到验证码的记录对"复制验证码"没有意义，跳过但保留原下标做 id
        if row.extracted.is_empty() {
            continue;
        }
        let id = MENU_RECENT_BASE + index;
        win::menu_item(recent_menu, MF_STRING, id, &recent_label(row));
        shown += 1;
    }
    if shown == 0 {
        win::menu_status(recent_menu, "暂无记录");
    }
    win::menu_popup(menu, recent_menu, "最近验证码");
    win::menu_separator(menu);

    win::menu_item(menu, MF_STRING, MENU_ADMIN, "打开管理台");
    win::menu_item(menu, MF_STRING, MENU_TEST, "发送测试通知");
    // 分隔线在这里不只是分组：没有它，「设置」会紧贴上一项，看起来像同一条目
    win::menu_separator(menu);

    // 低频项收进「设置」，主菜单只留高频动作。勾选必须打在子菜单句柄上，
    // 对父菜单 CheckMenuItem 是无效的。
    let settings = unsafe { CreatePopupMenu() };
    win::menu_item(settings, MF_STRING, MENU_AUTOSTART, "开机自动启动");
    let check = if win::auto_start_enabled() {
        MF_CHECKED
    } else {
        MF_UNCHECKED
    };
    unsafe {
        CheckMenuItem(settings, MENU_AUTOSTART as UINT, check);
    }
    win::menu_item(settings, MF_STRING, MENU_RELOAD, "重载配置");
    win::menu_popup(menu, settings, "设置");
    win::menu_separator(menu);
    win::menu_item(menu, MF_STRING, MENU_EXIT, "退出");

    let mut pt: POINT = unsafe { std::mem::zeroed() };
    unsafe {
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
    }
    // 右下角贴住光标，菜单向上向左长出来；否则靠近任务栏时会被系统挪位跳动
    let cmd = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN | TPM_RIGHTALIGN,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        )
    } as usize;
    unsafe {
        // 让菜单在点击外部时能正常消失
        PostMessageW(hwnd, 0, 0, 0);
        // 顶层句柄销毁即整棵子树销毁，「最近验证码」「设置」无需单独释放
        DestroyMenu(menu);
    }

    match cmd {
        MENU_ADMIN => win::open_url(&admin_url("")),
        MENU_TEST => {
            // 测试通知本身会弹卡片 / Toast，别再叠一个气泡
            unsafe {
                PostMessageW(hwnd, event::WM_APP_TEST, 0, 0);
            }
        }
        MENU_RELOAD => reload_with_feedback(),
        MENU_AUTOSTART => toggle_auto_start(),
        MENU_EXIT => {
            // 退出不做二次确认：这是托盘工具，确认框只会拖慢关机前的清理
            unsafe {
                PostMessageW(hwnd, event::WM_APP_QUIT, 0, 0);
            }
        }
        id if (MENU_RECENT_BASE..MENU_RECENT_BASE + rows.len()).contains(&id) => {
            if let Some(row) = rows.get(id - MENU_RECENT_BASE) {
                copy_code(row);
            }
        }
        _ => {}
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
        x if x == event::WM_APP_POKE => {
            // 用户又双击了一次 exe。此时弹卡片太重、开浏览器太吵，
            // 一条单行提示就够：它已经在跑，入口在托盘。
            hint::show("SmsSink 已在运行 · 右键托盘图标操作", hint::Tone::Info);
            0
        }
        x if x == event::WM_APP_QUIT => {
            remove_tray_icon(hwnd);
            DestroyWindow(hwnd);
            0
        }
        x if x == event::WM_APP_TRAY => {
            // 通知区 v4 回调：lparam 低 16 位是事件码，高 16 位是图标 id
            let evt = (lparam & 0xFFFF) as u32;
            if evt == WM_LBUTTONUP {
                // Windows 用户的直觉：单击托盘图标就是"打开主界面"
                win::open_url(&admin_url(""));
            } else if evt == WM_RBUTTONUP || evt == WM_LBUTTONDBLCLK {
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
    let title = wide(config::APP_DISPLAY_NAME);
    // 不能用 HWND_MESSAGE：消息专用窗口永远无法成为前台窗口，
    // SetForegroundWindow 会失败，弹出菜单得不到键盘焦点，点窗口外也可能不消失
    // ——菜单卡在屏幕上的同时 UI 线程停在 TrackPopupMenu 里，后续推送全部弹不出通知。
    // 改用"普通的隐藏窗口"：样式为 0 且不带 WS_VISIBLE，用户看不见它，
    // 但它可以被置前，菜单行为与常规应用一致。
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            0,
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
