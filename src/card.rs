//! 自绘右下角卡片：大号验证码 + 真·Win32 复制按钮 + 倒计时自动消失。
//! 不依赖 AUMID / COM，通知中心不留痕，适合高频一闪而过的场景。
//!
//! 视觉层次：验证码是唯一焦点，标题/时间/原文一律降级为次要信息；
//! 按钮自绘成「次要描边 + 主要实心」，靠右排列，主次一眼可辨。
//!
//! 排版约定：尺寸一律是 96 DPI 下的逻辑像素，绘制时按 `dpi / 96` 换算；
//! 每一行都放在「刚好一行高」的矩形里并垂直居中，避免不同行基线忽高忽低。
use crate::event::SmsEvent;
use crate::rules::Decision;
use crate::win::*;
use crate::{app, font, store, win};
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
const CARD_H: f64 = 180.0;
const PAD: f64 = 20.0;
const BTN_H: f64 = 30.0;
const BTN_COPY_W: f64 = 72.0;
const BTN_CLOSE_W: f64 = 60.0;
const BTN_GAP: f64 = 8.0;

// 字号（逻辑像素）。中文 UI 字体在 12px 才站得住，比原先的 11px 略大，
// 这是系统 zh-CN 界面正文的常规尺寸。
const TITLE_PX: f64 = 12.0;
const CODE_PX: f64 = 36.0;
const BODY_PX: f64 = 12.0;
const BUTTON_PX: f64 = 12.0;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static CARDS: Mutex<Vec<HWND>> = Mutex::new(Vec::new());

#[derive(Clone, Copy)]
struct Fonts {
    /// 标题：最弱的一层
    title: HFONT,
    /// 验证码：全卡唯一的视觉焦点
    code: HFONT,
    /// 时间与原文
    body: HFONT,
    /// 按钮文字
    button: HFONT,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: COLORREF,
    accent: COLORREF,
    on_accent: COLORREF,
    /// 原文这类需要读的字，比 muted 实一档
    text: COLORREF,
    /// 标题、时间、描边按钮的字：明确降级
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
    dpi: u32,
    fonts: Fonts,
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

// COLORREF 是 0x00BBGGRR，这里按 RGB 习惯书写
const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    r | (g << 8) | (b << 16)
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
    if dark {
        Palette {
            bg: rgb(0x2B, 0x2B, 0x2D),
            accent: rgb(0x5B, 0x9D, 0xFF),
            on_accent: rgb(0x0A, 0x0A, 0x0C),
            text: rgb(0xD4, 0xD4, 0xDA),
            muted: rgb(0x93, 0x93, 0x9A),
            border: rgb(0x3E, 0x3E, 0x42),
        }
    } else {
        Palette {
            bg: rgb(0xFF, 0xFF, 0xFF),
            accent: rgb(0x25, 0x63, 0xEB),
            on_accent: rgb(0xFF, 0xFF, 0xFF),
            text: rgb(0x3F, 0x3F, 0x46),
            muted: rgb(0x8A, 0x8A, 0x90),
            border: rgb(0xE4, 0xE4, 0xE9),
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

/// 在指定矩形里画一行字并垂直居中。
///
/// 前提：调用方已把背景模式设为 OPAQUE 且底色等于卡片底色，ClearType 才能
/// 拿到准确的背景做子像素混合。
unsafe fn line(hdc: HDC, font: HFONT, color: COLORREF, text: &str, rect: RECT, flags: UINT) {
    SelectObject(hdc, font);
    SetTextColor(hdc, color);
    let mut r = rect;
    let w = wide(text);
    DrawTextW(
        hdc,
        w.as_ptr(),
        -1,
        &mut r,
        flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
    );
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc == 0 {
        return;
    }
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);

    let pal = palette();
    let brush = CreateSolidBrush(pal.bg);
    FillRect(hdc, &rc, brush);
    DeleteObject(brush);

    // 实色背景 + 不透明背景模式：ClearType 的最优路径
    SetBkMode(hdc, OPAQUE);
    SetBkColor(hdc, pal.bg);

    if let Some(s) = state(hwnd) {
        let dpi = s.dpi;
        let px = |v: f64| (v * dpi as f64 / 96.0).round() as i32;
        let f = s.fonts;
        let pad = px(PAD);
        let right = rc.right - pad;

        // 标题：弱化成次要信息，不与验证码争视线
        line(
            hdc,
            f.title,
            pal.muted,
            &s.title,
            RECT {
                left: pad,
                top: px(16.0),
                right,
                bottom: px(16.0 + 16.0),
            },
            DT_LEFT | DT_END_ELLIPSIS,
        );

        // 验证码：唯一焦点。字符间距略开一点，一串数字才不显得挤成一团。
        SetTextCharacterExtra(hdc, px(0.7));
        line(
            hdc,
            f.code,
            pal.accent,
            &s.code,
            RECT {
                left: pad,
                top: px(32.0),
                right,
                bottom: px(32.0 + 54.0),
            },
            DT_LEFT,
        );
        SetTextCharacterExtra(hdc, 0);

        // 时间：最弱
        line(
            hdc,
            f.body,
            pal.muted,
            &s.time,
            RECT {
                left: pad,
                top: px(88.0),
                right,
                bottom: px(88.0 + 16.0),
            },
            DT_LEFT,
        );

        // 原文：同属降级信息，但比时间要能读
        line(
            hdc,
            f.body,
            pal.text,
            &s.content,
            RECT {
                left: pad,
                top: px(106.0),
                right,
                bottom: px(106.0 + 18.0),
            },
            DT_LEFT | DT_END_ELLIPSIS,
        );
    }

    EndPaint(hwnd, &ps);
}

/// 自绘按钮：「复制」实心强调色，「关闭」描边弱化；按下态统一压暗
unsafe fn draw_button(dis: &DRAWITEMSTRUCT) -> LRESULT {
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let pressed = dis.itemState & ODS_SELECTED != 0;
    let primary = dis.CtlID as usize == ID_COPY;

    let Some(parent) = state_owner(dis.hwndItem) else {
        return 0;
    };
    let s = &*parent;
    let dpi = s.dpi;
    let px = |v: f64| (v * dpi as f64 / 96.0).round() as i32;
    let pal = palette();

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
        (shade(pal.bg, 0.94), pal.border)
    } else {
        (pal.bg, pal.border)
    };
    SetDCBrushColor(hdc, fill);
    SetDCPenColor(hdc, edge);

    let radius = px(7.0);
    RoundRect(
        hdc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        radius * 2,
        radius * 2,
    );

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);

    SetBkMode(hdc, OPAQUE);
    SetBkColor(hdc, fill);
    let (color, label) = if primary {
        (pal.on_accent, COPY_LABEL)
    } else {
        (pal.text, CLOSE_LABEL)
    };
    let mut tr = rc;
    let text = wide(label);
    SelectObject(hdc, s.fonts.button);
    SetTextColor(hdc, color);
    DrawTextW(
        hdc,
        text.as_ptr(),
        -1,
        &mut tr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    1
}

/// 取子控件所属窗口的 CardState 指针
unsafe fn state_owner(child: HWND) -> Option<*mut CardState> {
    let parent = GetParent(child);
    if parent == 0 {
        return None;
    }
    let ptr = GetWindowLongPtrW(parent, GWLP_USERDATA);
    if ptr == 0 {
        None
    } else {
        Some(ptr as *mut CardState)
    }
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
                let card = Box::from_raw(ptr as *mut CardState);
                for f in [
                    card.fonts.title,
                    card.fonts.code,
                    card.fonts.body,
                    card.fonts.button,
                ] {
                    if f != 0 {
                        DeleteObject(f);
                    }
                }
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
    let dpi = win::system_dpi();
    let scale = dpi as f64 / 96.0;
    let w = (CARD_W * scale).round() as i32;
    let h = (CARD_H * scale).round() as i32;
    let gap = (10.0 * scale).round() as i32;
    let margin = (18.0 * scale).round() as i32;

    let mut work: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut c_void, 0);
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
            SetWindowPos(
                existing,
                HWND_TOPMOST,
                work.right - margin - w,
                y,
                w,
                h,
                SWP_NOACTIVATE,
            );
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

    let face = font::ui_face();
    let fonts = Fonts {
        title: font::create(face, TITLE_PX, dpi, FW_NORMAL),
        code: font::create(face, CODE_PX, dpi, FW_BOLD),
        body: font::create(face, BODY_PX, dpi, FW_NORMAL),
        button: font::create(face, BUTTON_PX, dpi, FW_NORMAL),
    };

    let card = Box::new(CardState {
        title: format!("{} · {}", crate::config::APP_DISPLAY_NAME, d.rule_name),
        code: d.code.clone(),
        time: ev.local_time.clone(),
        content: ev.content.clone(),
        copy_text: d.copy_text.clone(),
        row_id,
        dpi,
        fonts,
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
