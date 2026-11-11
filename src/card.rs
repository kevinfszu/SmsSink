//! 自绘右下角卡片：大号验证码 + 真·Win32 复制按钮 + 倒计时自动消失。
//! 不依赖 AUMID / COM，通知中心不留痕，适合高频一闪而过的场景。
use crate::event::SmsEvent;
use crate::rules::Decision;
use crate::{app, store, win};
use crate::win::*;
use std::sync::{Mutex, OnceLock};

const ID_COPY: usize = 1001;
const ID_CLOSE: usize = 1002;
const TIMER_ID: usize = 1;
const MAX_CARDS: usize = 5;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static CARDS: Mutex<Vec<HWND>> = Mutex::new(Vec::new());
static FONT_CACHE: OnceLock<(HFONT, HFONT)> = OnceLock::new();

struct CardState {
    title: String,
    code: String,
    sub: String,
    copy_text: String,
    row_id: i64,
}

fn class_name() -> &'static Vec<u16> {
    CLASS_NAME.get_or_init(|| {
        let name = wide("SmsSinkCardWindow");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance(),
            hIcon: 0,
            hCursor: unsafe { LoadCursorW(0, IDC_ARROW as *const u16) },
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

fn font(px: i32, weight: i32, scale: f64) -> HFONT {
    let face = wide("Segoe UI");
    unsafe {
        CreateFontW(
            -((px as f64 * scale).round() as i32),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            0,
            0,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH | FF_DONTCARE,
            face.as_ptr(),
        )
    }
}

/// 跟随系统深浅色，避免深色模式下刺眼的白卡片
fn theme() -> (COLORREF, COLORREF, COLORREF) {
    let mut dark = false;
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let mut hkey: isize = 0;
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        ) == 0
        {
            let name = wide("AppsUseLightTheme");
            let mut value: DWORD = 1;
            let mut size: DWORD = 4;
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut value as *mut DWORD as *mut u8,
                &mut size,
            );
            dark = value == 0;
            RegCloseKey(hkey);
        }
    }
    if dark {
        (0x002E_2E2A, 0x00F2_F2F2, 0x00FF_9D5C) // 背景 / 正文 / 强调
    } else {
        (0x00FF_FFFF, 0x0020_2020, 0x00EB_6325)
    }
}

unsafe fn state<'a>(hwnd: HWND) -> Option<&'a mut CardState> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr == 0 {
        None
    } else {
        Some(&mut *(ptr as *mut CardState))
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc == 0 {
        return;
    }
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);

    let scale = win::ui_scale();
    let (bg, fg, accent) = theme();
    let brush = CreateSolidBrush(bg);
    FillRect(hdc, &rc, brush);
    DeleteObject(brush);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, fg);

    let (small, big) = *FONT_CACHE.get_or_init(|| (font(13, FW_NORMAL, scale), font(34, FW_BOLD, scale)));
    let old = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));

    if let Some(s) = state(hwnd) {
        let pad = (18.0 * scale) as i32;
        let mut r = RECT {
            left: pad,
            top: (12.0 * scale) as i32,
            right: rc.right - pad,
            bottom: (32.0 * scale) as i32,
        };
        SelectObject(hdc, small);
        let t = wide(&s.title);
        DrawTextW(
            hdc,
            t.as_ptr(),
            -1,
            &mut r,
            DT_LEFT | DT_SINGLELINE | DT_WORD_ELLIPSIS | DT_NOPREFIX,
        );

        SetTextColor(hdc, accent);
        SelectObject(hdc, big);
        let mut r2 = RECT {
            left: pad,
            top: (34.0 * scale) as i32,
            right: rc.right - pad,
            bottom: (80.0 * scale) as i32,
        };
        let code = wide(&s.code);
        DrawTextW(
            hdc,
            code.as_ptr(),
            -1,
            &mut r2,
            DT_LEFT | DT_SINGLELINE | DT_NOPREFIX,
        );

        SetTextColor(hdc, fg);
        SelectObject(hdc, small);
        let mut r3 = RECT {
            left: pad,
            top: (82.0 * scale) as i32,
            right: rc.right - pad,
            bottom: (100.0 * scale) as i32,
        };
        let sub = wide(&s.sub);
        DrawTextW(
            hdc,
            sub.as_ptr(),
            -1,
            &mut r3,
            DT_LEFT | DT_SINGLELINE | DT_WORD_ELLIPSIS | DT_NOPREFIX,
        );
    }

    SelectObject(hdc, old);
    EndPaint(hwnd, &ps);
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as usize;
            if id == ID_COPY {
                if let Some(s) = state(hwnd) {
                    let text = s.copy_text.clone();
                    let rid = s.row_id;
                    match win::clipboard_set(&text) {
                        Ok(()) => {
                            store::mark_copied(rid);
                            println!("[card] 按钮复制成功 (row={rid})");
                        }
                        Err(e) => eprintln!("[card] 复制失败: {e}"),
                    }
                }
                DestroyWindow(hwnd);
            } else if id == ID_CLOSE {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_TIMER => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_ID);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                drop(Box::from_raw(ptr as *mut CardState));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            if let Ok(mut cards) = CARDS.lock() {
                cards.retain(|&h| h != hwnd);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn show(d: &Decision, ev: &SmsEvent, row_id: i64) {
    let cfg = app::get()
        .cfg
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let scale = win::ui_scale();
    let w = (360.0 * scale).round() as i32;
    let h = (146.0 * scale).round() as i32;
    let gap = (10.0 * scale).round() as i32;
    let margin = (18.0 * scale).round() as i32;

    let mut work: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut work as *mut RECT as *mut std::ffi::c_void,
            0,
        );
    }
    if work.right == 0 {
        work.right = 1920;
        work.bottom = 1080;
    }

    let mut cards = CARDS.lock().unwrap_or_else(|e| e.into_inner());
    while cards.len() >= MAX_CARDS {
        if let Some(old) = cards.first().copied() {
            unsafe {
                DestroyWindow(old);
            }
            cards.retain(|&x| x != old);
        } else {
            break;
        }
    }

    // 新卡片贴着托盘区，已有的往上挪
    for (i, &existing) in cards.iter().rev().enumerate() {
        let y = work.bottom - margin - h - (i as i32 + 1) * (h + gap);
        unsafe {
            SetWindowPos(existing, HWND_TOPMOST, work.right - margin - w, y, w, h, SWP_NOACTIVATE);
        }
    }
    let y = work.bottom - margin - h;
    let x = work.right - margin - w;

    let cls = class_name();
    let empty = wide("");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            cls.as_ptr(),
            empty.as_ptr(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            0,
            0,
            instance(),
            std::ptr::null_mut(),
        )
    };
    if hwnd == 0 {
        eprintln!("[card] 创建窗口失败");
        return;
    }

    let card = Box::new(CardState {
        title: format!("{} · {}", crate::config::APP_DISPLAY_NAME, d.rule_name),
        code: d.code.clone(),
        sub: format!("{} · 来自 {} · 有效期 {} 秒", ev.local_time, ev.from, d.ttl_sec),
        copy_text: d.copy_text.clone(),
        row_id,
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(card) as isize);

        let btn_copy = wide("复制验证码");
        let btn_close = wide("关闭");
        let button = wide("BUTTON");
        let bw = (104.0 * scale).round() as i32;
        let bh = (30.0 * scale).round() as i32;
        let by = h - bh - (12.0 * scale).round() as i32;
        CreateWindowExW(
            0,
            button.as_ptr(),
            btn_copy.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            (18.0 * scale).round() as i32,
            by,
            bw,
            bh,
            hwnd,
            ID_COPY as isize,
            instance(),
            std::ptr::null_mut(),
        );
        CreateWindowExW(
            0,
            button.as_ptr(),
            btn_close.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            (18.0 * scale).round() as i32 + bw + (8.0 * scale).round() as i32,
            by,
            (64.0 * scale).round() as i32,
            bh,
            hwnd,
            ID_CLOSE as isize,
            instance(),
            std::ptr::null_mut(),
        );

        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        SetTimer(
            hwnd,
            TIMER_ID,
            (cfg.card_seconds.clamp(2, 300) * 1000) as UINT,
            std::ptr::null_mut(),
        );
    }
    cards.push(hwnd);
}
