//! 轻量提示条：托盘菜单操作的可见回执。
//!
//! 为什么不用系统托盘气泡（`Shell_NotifyIcon` + `NIF_INFO`）：Windows 只给
//! "在开始菜单里注册过身份"的应用显示通知横幅，未注册的进程发出去的气泡会
//! 被系统直接吞掉——实测桌面右下角什么都不出现。企业环境里关掉通知的情况也
//! 一样。一个"点了没反应"的菜单比没有回执更糟，所以回执必须由我们自己画。
//!
//! 形态上刻意做得比通知卡片轻（单行、40 逻辑像素高、2.6 秒自动消失、
//! 不抢焦点），避免把"配置已重载"这种一句话的事拔高成一条通知。
use crate::font;
use crate::win;
use crate::win::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
/// 同一时刻只留一条：后来的把前面那条顶掉，避免提示堆成一列
static CURRENT: AtomicIsize = AtomicIsize::new(0);

const H: f64 = 40.0;
const MIN_W: f64 = 148.0;
const MAX_W: f64 = 420.0;
const PAD_X: f64 = 16.0;
const FONT_PX: f64 = 12.0;
const TIMER_ID: usize = 11;

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Info,
    Warn,
}

struct Colors {
    bg: COLORREF,
    border: COLORREF,
    text: COLORREF,
}

fn colors(tone: Tone) -> Colors {
    let dark = crate::card::is_dark_theme();
    match (dark, tone) {
        (false, Tone::Info) => Colors {
            bg: rgb(0xFF, 0xFF, 0xFF),
            border: rgb(0xDF, 0xDF, 0xE4),
            text: rgb(0x2E, 0x2E, 0x33),
        },
        (false, Tone::Warn) => Colors {
            bg: rgb(0xFF, 0xFB, 0xFA),
            border: rgb(0xE8, 0xBF, 0xB8),
            text: rgb(0xA3, 0x2A, 0x1B),
        },
        (true, Tone::Info) => Colors {
            bg: rgb(0x33, 0x33, 0x36),
            border: rgb(0x4A, 0x4A, 0x4F),
            text: rgb(0xE8, 0xE8, 0xEC),
        },
        (true, Tone::Warn) => Colors {
            bg: rgb(0x3A, 0x2C, 0x2A),
            border: rgb(0x6B, 0x42, 0x3B),
            text: rgb(0xFF, 0xB4, 0xA8),
        },
    }
}

const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    r | (g << 8) | (b << 16)
}

fn class_name() -> &'static Vec<u16> {
    CLASS_NAME.get_or_init(|| {
        let name = wide("SmsSinkHintWindow");
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

/// 粗略估算单行宽度：中文按 1 个字号宽、ASCII 按 0.55 个字号宽。
/// 精确测量需要 CreateFont+GetTextExtentPoint32 走一遍 DC，这里只是提示条，
/// 差几个像素无所谓，宁可略宽也不要换行。
fn estimate_width(text: &str, scale: f64) -> f64 {
    let mut w = 0.0;
    for c in text.chars() {
        w += if (c as u32) < 0x2E80 {
            FONT_PX * 0.55
        } else {
            FONT_PX
        };
    }
    ((w + PAD_X * 2.0) * scale).clamp(MIN_W * scale, MAX_W * scale)
}

struct HintState {
    text: String,
    tone: Tone,
    dpi: u32,
}

unsafe fn state<'a>(hwnd: HWND) -> Option<&'a mut HintState> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr == 0 {
        None
    } else {
        Some(&mut *(ptr as *mut HintState))
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

    let Some(s) = state(hwnd) else {
        EndPaint(hwnd, &ps);
        return;
    };
    let pal = colors(s.tone);
    let dpi = s.dpi;
    let px = |v: f64| (v * dpi as f64 / 96.0).round() as i32;

    let brush = CreateSolidBrush(pal.bg);
    FillRect(hdc, &rc, brush);
    DeleteObject(brush);

    // 细描边：不描边的话浅色主题下提示条会糊进桌面背景
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let old_pen = SelectObject(hdc, GetStockObject(DC_PEN));
    SetDCPenColor(hdc, pal.border);
    let r = px(12.0);
    RoundRect(hdc, 0, 0, rc.right, rc.bottom, r * 2, r * 2);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);

    SetBkMode(hdc, OPAQUE);
    SetBkColor(hdc, pal.bg);
    SetTextColor(hdc, pal.text);
    let f = font::create(font::ui_face(), FONT_PX, dpi, FW_NORMAL);
    let old_font = SelectObject(hdc, f);
    let text = wide(&s.text);
    let mut tr = RECT {
        left: px(PAD_X),
        top: 0,
        right: rc.right - px(PAD_X),
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        text.as_ptr(),
        -1,
        &mut tr,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    SelectObject(hdc, old_font);
    DeleteObject(f);

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
        WM_TIMER => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_ID);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                drop(Box::from_raw(ptr as *mut HintState));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            CURRENT
                .compare_exchange(hwnd, 0, Ordering::SeqCst, Ordering::SeqCst)
                .ok();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 弹一条提示。必须在 UI 线程调用（要建窗口）。
pub fn show(text: &str, tone: Tone) {
    let dpi = win::system_dpi();
    let scale = dpi as f64 / 96.0;
    let h = (H * scale).round() as i32;
    let w = estimate_width(text, scale).round() as i32;
    let margin = (18.0 * scale).round() as i32;

    let mut work: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut c_void, 0);
    }
    if work.right == 0 {
        work.right = 1920;
        work.bottom = 1080;
    }
    // 顶掉上一条，避免连点菜单时提示叠成一列
    let prev = CURRENT.swap(0, Ordering::SeqCst);
    if prev != 0 {
        unsafe {
            DestroyWindow(prev);
        }
    }

    let x = work.right - margin - w;
    let y = work.bottom - margin - h;

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
        eprintln!("[hint] 创建提示窗口失败: {text}");
        return;
    }

    let st = Box::new(HintState {
        text: text.to_string(),
        tone,
        dpi,
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(st) as isize);
        let corner: i32 = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as DWORD,
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        SetTimer(hwnd, TIMER_ID, 2600, std::ptr::null_mut());
    }
    CURRENT.store(hwnd, Ordering::SeqCst);
}
