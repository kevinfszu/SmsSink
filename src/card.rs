//! 自绘右下角卡片：大号验证码 + 真·Win32 复制按钮 + 倒计时自动消失。
//! 不依赖 AUMID / COM，通知中心不留痕，适合高频一闪而过的场景。
//!
//! 视觉层次：验证码是唯一焦点，标题/时间/原文一律降级为次要信息；
//! 按钮自绘成「次要描边 + 主要实心」，靠右排列，主次一眼可辨。
use crate::event::SmsEvent;
use crate::rules::Decision;
use crate::win::*;
use crate::{app, store, win};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

const ID_COPY: usize = 1001;
const ID_CLOSE: usize = 1002;
const TIMER_ID: usize = 1;
const MAX_CARDS: usize = 5;

const COPY_LABEL: &str = "复制";
const CLOSE_LABEL: &str = "关闭";

// 逻辑尺寸，实际按 DPI 缩放
const CARD_W: f64 = 380.0;
const CARD_H: f64 = 164.0;
const PAD: f64 = 20.0;
const BTN_H: f64 = 30.0;
const BTN_COPY_W: f64 = 72.0;
const BTN_CLOSE_W: f64 = 60.0;
const BTN_GAP: f64 = 8.0;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static CARDS: Mutex<Vec<HWND>> = Mutex::new(Vec::new());
static FONT_CACHE: OnceLock<Fonts> = OnceLock::new();

struct Fonts {
    /// 标题、时间、原文：小号，不抢视线
    meta: HFONT,
    /// 验证码：全卡唯一的视觉焦点
    code: HFONT,
    /// 按钮文字，比正文略小但比 meta 略重
    button: HFONT,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: COLORREF,
    accent: COLORREF,
    muted: COLORREF,
    border: COLORREF,
}

struct CardState {
    title: String,
    code: String,
    time: String,
    content: String,
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
            hIcon: win::app_icon(32),
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

fn fonts() -> &'static Fonts {
    FONT_CACHE.get_or_init(|| {
        let scale = win::ui_scale();
        Fonts {
            meta: font(11, FW_NORMAL, scale),
            code: font(36, FW_BOLD, scale),
            button: font(12, FW_SEMIBOLD, scale),
        }
    })
}

/// 跟随系统深浅色，避免深色模式下刺眼的白卡片
fn palette() -> Palette {
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
    // COLORREF 为 0x00BBGGRR
    if dark {
        Palette {
            bg: 0x002E_2E2A,     // #2A2E2E
            accent: 0x00FF_9D5C, // #5C9DFF
            muted: 0x0096_9696,  // #969696
            border: 0x0044_443E, // #3E4444
        }
    } else {
        Palette {
            bg: 0x00FF_FFFF,     // #FFFFFF
            accent: 0x00EB_6325, // #2563EB
            muted: 0x0088_8888,  // #888888
            border: 0x00E2_E2E2, // #E2E2E2
        }
    }
}

/// 按比例压暗，用于按钮按下态
fn shade(c: COLORREF, factor: f32) -> COLORREF {
    let r = ((c & 0xFF) as f32 * factor).clamp(0.0, 255.0) as u32;
    let g = (((c >> 8) & 0xFF) as f32 * factor).clamp(0.0, 255.0) as u32;
    let b = (((c >> 16) & 0xFF) as f32 * factor).clamp(0.0, 255.0) as u32;
    r | (g << 8) | (b << 16)
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
    let pal = palette();
    let brush = CreateSolidBrush(pal.bg);
    FillRect(hdc, &rc, brush);
    DeleteObject(brush);
    SetBkMode(hdc, TRANSPARENT);

    let f = fonts();
    let pad = (PAD * scale) as i32;
    let right = rc.right - pad;

    if let Some(s) = state(hwnd) {
        // 标题：弱化成次要信息，不与验证码争视线
        SetTextColor(hdc, pal.muted);
        SelectObject(hdc, f.meta);
        let mut r = RECT {
            left: pad,
            top: (14.0 * scale) as i32,
            right,
            bottom: (30.0 * scale) as i32,
        };
        let title = wide(&s.title);
        DrawTextW(
            hdc,
            title.as_ptr(),
            -1,
            &mut r,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        // 验证码：唯一焦点
        SetTextColor(hdc, pal.accent);
        SelectObject(hdc, f.code);
        let mut r2 = RECT {
            left: pad,
            top: (30.0 * scale) as i32,
            right,
            bottom: (78.0 * scale) as i32,
        };
        let code = wide(&s.code);
        DrawTextW(
            hdc,
            code.as_ptr(),
            -1,
            &mut r2,
            DT_LEFT | DT_SINGLELINE | DT_NOPREFIX,
        );

        // 时间与原文：同属降级信息
        SetTextColor(hdc, pal.muted);
        SelectObject(hdc, f.meta);
        let mut r3 = RECT {
            left: pad,
            top: (80.0 * scale) as i32,
            right,
            bottom: (96.0 * scale) as i32,
        };
        let time = wide(&s.time);
        DrawTextW(
            hdc,
            time.as_ptr(),
            -1,
            &mut r3,
            DT_LEFT | DT_SINGLELINE | DT_NOPREFIX,
        );

        let mut r4 = RECT {
            left: pad,
            top: (96.0 * scale) as i32,
            right,
            bottom: (112.0 * scale) as i32,
        };
        let content = wide(&s.content);
        DrawTextW(
            hdc,
            content.as_ptr(),
            -1,
            &mut r4,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }

    EndPaint(hwnd, &ps);
}

/// 自绘按钮：「复制」实心强调色，「关闭」描边弱化；按下态统一压暗
unsafe fn draw_button(dis: &DRAWITEMSTRUCT) -> LRESULT {
    let scale = win::ui_scale();
    let pal = palette();
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let pressed = dis.itemState & ODS_SELECTED != 0;
    let primary = dis.CtlID as usize == ID_COPY;

    // 先铺卡片底色，避免圆角外露出上一帧内容
    let bg = CreateSolidBrush(pal.bg);
    FillRect(hdc, &rc, bg);
    DeleteObject(bg);

    let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
    let old_pen = SelectObject(hdc, GetStockObject(DC_PEN));

    let (fill, edge) = if primary {
        let base = if pressed {
            shade(pal.accent, 0.82)
        } else {
            pal.accent
        };
        (base, base)
    } else if pressed {
        (pal.border, pal.border)
    } else {
        (pal.bg, pal.border)
    };
    SetDCBrushColor(hdc, fill);
    SetDCPenColor(hdc, edge);

    let radius = (7.0 * scale) as i32;
    RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius * 2, radius * 2);

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);

    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, if primary { 0x00FF_FFFF } else { pal.muted });

    let f = fonts();
    let old_font = SelectObject(hdc, f.button);
    let label = wide(if primary { COPY_LABEL } else { CLOSE_LABEL });
    let mut tr = rc;
    DrawTextW(
        hdc,
        label.as_ptr(),
        -1,
        &mut tr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(hdc, old_font);
    1
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
        WM_DRAWITEM => {
            if lparam != 0 {
                draw_button(&*(lparam as *const DRAWITEMSTRUCT))
            } else {
                0
            }
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
    let w = (CARD_W * scale).round() as i32;
    let h = (CARD_H * scale).round() as i32;
    let gap = (10.0 * scale).round() as i32;
    let margin = (18.0 * scale).round() as i32;

    let mut work: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut work as *mut RECT as *mut c_void,
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
        time: ev.local_time.clone(),
        content: ev.content.clone(),
        copy_text: d.copy_text.clone(),
        row_id,
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(card) as isize);

        // Win11 圆角，失败则保持直角，无副作用
        let corner: i32 = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as DWORD,
        );

        let pad = (PAD * scale).round() as i32;
        let bw_copy = (BTN_COPY_W * scale).round() as i32;
        let bw_close = (BTN_CLOSE_W * scale).round() as i32;
        let bh = (BTN_H * scale).round() as i32;
        let by = h - bh - (14.0 * scale).round() as i32;
        let x_copy = w - pad - bw_copy;
        let x_close = x_copy - (BTN_GAP * scale).round() as i32 - bw_close;

        let button = wide("BUTTON");
        let copy_label = wide(COPY_LABEL);
        let close_label = wide(CLOSE_LABEL);
        let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW;

        // 靠右排列，顺序为「关闭、复制」，「复制」在最右侧作为主操作
        CreateWindowExW(
            0,
            button.as_ptr(),
            close_label.as_ptr(),
            style,
            x_close,
            by,
            bw_close,
            bh,
            hwnd,
            ID_CLOSE as isize,
            instance(),
            std::ptr::null_mut(),
        );
        CreateWindowExW(
            0,
            button.as_ptr(),
            copy_label.as_ptr(),
            style,
            x_copy,
            by,
            bw_copy,
            bh,
            hwnd,
            ID_COPY as isize,
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
