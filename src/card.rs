//! 自绘右下角卡片：大号验证码 + 真·Win32 复制按钮 + 倒计时自动消失。
//! 不依赖 AUMID / COM，通知中心不留痕，适合高频一闪而过的场景。
//!
//! 视觉层次：验证码是唯一焦点，标题/时间/原文一律降级为次要信息；
//! 按钮自绘成「次要描边 + 主要实心」，靠右排列，主次一眼可辨。
//!
//! 交互上只守两条硬规则：鼠标停在卡片上就不许自动消失（用户可能正在读），
//! 点完「复制」必须留下可见回执（否则用户不知道到底复制成功没有）。
//!
//! 排版约定：尺寸一律是 96 DPI 下的逻辑像素，绘制时按 `dpi / 96` 换算；
//! 每一行都放在「刚好一行高」的矩形里并垂直居中，避免不同行基线忽高忽低。
use crate::event::SmsEvent;
use crate::rules::Decision;
use crate::win::*;
use crate::{app, font, store, win};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ID_COPY: usize = 1001;
const ID_CLOSE: usize = 1002;
const MAX_CARDS: usize = 5;

// 定时器 id。全部挂在卡片窗口上，窗口销毁时随窗口一起消失，
// 因此「没有卡片」等于「没有任何定时器」。
const TIMER_AUTO: usize = 1;
const TIMER_FEEDBACK: usize = 2;
const TIMER_RESET: usize = 3;
const TIMER_PROGRESS: usize = 4;

/// 进度条与鼠标状态共用同一个节拍，省掉一个定时器
const PROGRESS_MS: UINT = 250;
/// 「已复制」停留多久再收卡片：短到不拖沓，长到眼睛能捕捉到变化
const FEEDBACK_MS: UINT = 700;
/// 「复制失败」停留多久后恢复成可再点的「复制」
const RESET_MS: UINT = 2000;

/// `DrawTextW` 的自动换行标志，win.rs 没有声明；只是一个数值常量，
/// 补在这里比给 FFI 模块加噪音更合适
const DT_WORDBREAK: UINT = 0x0010;

const COPY_LABEL: &str = "复制";
const COPY_PLAIN_LABEL: &str = "复制原文";
const CLOSE_LABEL: &str = "关闭";
const COPIED_LABEL: &str = "已复制";
const COPY_FAIL_LABEL: &str = "复制失败";

// 逻辑尺寸，实际按 DPI 缩放
const CARD_W: f64 = 380.0;
/// 比上一版高 12px：给原文的第二行和顶部时间的右上角留位
const CARD_H: f64 = 192.0;
const PAD: f64 = 20.0;
const BTN_H: f64 = 30.0;
/// 按钮宽度 = 文案宽度 + 两侧各 14px。文案决定宽度，按钮里就不会出现
/// 「两边空一大块、字挤在中间」的观感；最长的文案（已复制 / 复制失败）
/// 在创建时就算进去，切换状态时按钮不会跳宽。
const BTN_PAD_X: f64 = 14.0;
const BTN_MIN_W: f64 = 52.0;
const BTN_GAP: f64 = 12.0;
const PROGRESS_H: f64 = 3.0;

// 纵向节奏（逻辑像素，自卡片顶边算起）。时间被移到标题行右侧后，
// 原文可以完整占两行——长短信不再在第一行末尾就被截断。
const TOP_ROW_TOP: f64 = 16.0;
const TOP_ROW_BOTTOM: f64 = 34.0;
const CODE_TOP: f64 = 40.0;
const CODE_BOTTOM: f64 = 94.0;
const BODY_TOP: f64 = 100.0;
const BODY_BOTTOM: f64 = 136.0;
/// 标题与右上角时间之间的最小间距
const TITLE_TIME_GAP: f64 = 12.0;

// 字号（逻辑像素）。中文 UI 字体在 12px 才站得住，比原先的 11px 略大，
// 这是系统 zh-CN 界面正文的常规尺寸。
const TITLE_PX: f64 = 12.0;
const CODE_PX: f64 = 36.0;
const BODY_PX: f64 = 12.0;
/// 未识别卡片的两行正文：比常规正文大一档，替代原验证码位置的视觉重量
const PARA_PX: f64 = 13.0;
const BUTTON_PX: f64 = 12.0;

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static CARDS: Mutex<Vec<HWND>> = Mutex::new(Vec::new());

/// 卡片的两种主区域：验证码与「没解析出来」的原文。
///
/// 只在这里分叉、窗口与按钮逻辑共用，避免维护两套几乎相同的窗口代码。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Code,
    Plain,
}

/// 主按钮的反馈状态；关闭按钮不需要
#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyState {
    Idle,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
struct Fonts {
    /// 标题：最弱的一层
    title: HFONT,
    /// 验证码：全卡唯一的视觉焦点
    code: HFONT,
    /// 时间与原文
    body: HFONT,
    /// 未识别卡片的两行正文
    para: HFONT,
    /// 按钮文字
    button: HFONT,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: COLORREF,
    accent: COLORREF,
    on_accent: COLORREF,
    /// 复制成功的回执色：绿色是「成了」的通用语言
    ok: COLORREF,
    /// 原文这类需要读的字，比 muted 实一档
    text: COLORREF,
    /// 标题、时间、描边按钮的字：明确降级
    muted: COLORREF,
    border: COLORREF,
}

struct CardState {
    kind: Kind,
    title: String,
    code: String,
    time: String,
    content: String,
    copy_text: String,
    row_id: i64,
    dpi: u32,
    fonts: Fonts,
    /// 主按钮的常态文案：「复制」或「复制原文」
    copy_label: &'static str,
    copy_state: CopyState,
    /// 鼠标正悬停的控件 id，`None` 表示不在任何按钮上
    hover: Option<usize>,
    btn_copy: HWND,
    btn_close: HWND,
    /// 两个按钮的客户区矩形，用于命中测试
    rc_copy: RECT,
    rc_close: RECT,
    /// 右上角时间文字的像素宽度（创建时量一次，重绘时不再建 DC）
    time_w: i32,
    /// 卡片客户区尺寸与左上角的屏幕坐标。
    /// 无边框弹出窗口的客户区原点与窗口原点重合，靠它就能把 `GetCursorPos`
    /// 的屏幕坐标换算成客户区坐标——`win` 里没有 ScreenToClient 可用。
    size: POINT,
    origin: POINT,
    /// 是否已订阅 TME_LEAVE，避免每次移动都重复调用 TrackMouseEvent
    tracking: bool,
    /// 总倒计时时长
    ttl: Duration,
    /// 本轮计时起点，与 `used` 配合表示「已用时长」
    started: Instant,
    /// 之前几轮计时累计的已用时长
    used: Duration,
    /// 暂停时刻；`Some` 表示鼠标正停在卡片上，倒计时冻结中
    paused_at: Option<Instant>,
}

impl CardState {
    fn elapsed(&self) -> Duration {
        match self.paused_at {
            Some(at) => self.used + at.saturating_duration_since(self.started),
            None => self.used + self.started.elapsed(),
        }
    }

    fn remaining(&self) -> Duration {
        self.ttl.saturating_sub(self.elapsed())
    }

    /// 剩余比例（0..=1），进度条宽度按它算
    fn ratio(&self) -> f32 {
        if self.ttl.is_zero() {
            return 0.0;
        }
        (self.remaining().as_secs_f32() / self.ttl.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// 逻辑像素 → 设备像素
fn px(dpi: u32, v: f64) -> i32 {
    (v * dpi as f64 / 96.0).round() as i32
}

/// 把一段文字裁到「最多 N 行」，超出的部分补省略号。
///
/// 不依赖 `DrawText` 的多行省略：`DT_WORDBREAK` 只按矩形裁切，
/// 矩形高度只要够第三行，就会露出半行字。这里先按可用宽度算总预算，
/// 逐字累加到放不下为止，保证结果不会超过 N 行。
unsafe fn fit_lines(font: HFONT, text: &str, avail: i32, lines: usize) -> String {
    if avail <= 0 || text.is_empty() {
        return text.to_string();
    }
    let dc = CreateCompatibleDC(0);
    if dc == 0 {
        return text.to_string();
    }
    let old = SelectObject(dc, font);
    let measure_units = |units: &[u16]| -> i32 {
        let mut sz = SIZE::default();
        GetTextExtentPoint32W(dc, units.as_ptr(), units.len() as i32, &mut sz);
        sz.cx
    };

    let mut widths: Vec<(char, i32)> = Vec::with_capacity(text.chars().count());
    let mut total = 0;
    for ch in text.chars() {
        let mut buf = [0u16; 2];
        let w = measure_units(ch.encode_utf16(&mut buf));
        total += w;
        widths.push((ch, w));
    }
    let budget = avail * lines as i32;
    if total <= budget {
        SelectObject(dc, old);
        DeleteDC(dc);
        return text.to_string();
    }

    let ellipsis = measure_units(&"…".encode_utf16().collect::<Vec<u16>>());
    let limit = budget - ellipsis;
    let mut acc = 0;
    let mut out = String::new();
    for (ch, w) in widths {
        if acc + w > limit {
            break;
        }
        acc += w;
        out.push(ch);
    }
    // 别把英文单词从中间劈开：末字符还是字母/数字就回退到上一个空格
    if out
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        if let Some(idx) = out.rfind(' ') {
            out.truncate(idx);
        }
    }
    out.push('…');

    SelectObject(dc, old);
    DeleteDC(dc);
    out
}

/// 量一段文字在给定字体下的设备像素宽度。
///
/// 按钮宽度靠它算，不写死数字：中文「复制」和「复制失败」差一倍宽，
/// 写死的宽度要么留白过多、要么把字挤在一起。
unsafe fn text_width(font: HFONT, text: &str) -> i32 {
    let dc = CreateCompatibleDC(0);
    if dc == 0 {
        return 0;
    }
    let old = SelectObject(dc, font);
    let mut sz = SIZE::default();
    let units = wide(text);
    GetTextExtentPoint32W(
        dc,
        units.as_ptr(),
        (units.len().saturating_sub(1)) as i32,
        &mut sz,
    );
    SelectObject(dc, old);
    DeleteDC(dc);
    sz.cx
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

/// 系统是否处于深色主题（跟着「设置 → 个性化 → 颜色」的应用模式走）。
/// 卡片和提示条共用同一套判断，避免两种浮层在深色桌面上一白一灰。
pub fn is_dark_theme() -> bool {
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let mut hkey: isize = 0;
    let mut dark = false;
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
    dark
}

/// 跟随系统深浅色，避免深色模式下刺眼的白卡片
fn palette() -> Palette {
    if is_dark_theme() {
        Palette {
            bg: rgb(0x2B, 0x2B, 0x2D),
            accent: rgb(0x5B, 0x9D, 0xFF),
            on_accent: rgb(0x0A, 0x0A, 0x0C),
            ok: rgb(0x3D, 0xD6, 0x8C),
            text: rgb(0xD4, 0xD4, 0xDA),
            muted: rgb(0x93, 0x93, 0x9A),
            border: rgb(0x3E, 0x3E, 0x42),
        }
    } else {
        Palette {
            bg: rgb(0xFF, 0xFF, 0xFF),
            accent: rgb(0x25, 0x63, 0xEB),
            on_accent: rgb(0xFF, 0xFF, 0xFF),
            ok: rgb(0x12, 0xA1, 0x50),
            text: rgb(0x3F, 0x3F, 0x46),
            muted: rgb(0x8A, 0x8A, 0x90),
            // 比原来的 #E4E4E9 深一档：白底上「关闭」的描边才看得见
            border: rgb(0xD8, 0xD8, 0xDE),
        }
    }
}

/// 线性混色：`t=0` 全是 `a`，`t=1` 全是 `b`
fn blend(a: COLORREF, b: COLORREF, t: f32) -> COLORREF {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xFF) as f32;
        let y = ((b >> shift) & 0xFF) as f32;
        (x + (y - x) * t).clamp(0.0, 255.0) as u32
    };
    ch(0) | (ch(8) << 8) | (ch(16) << 16)
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

/// 多行正文：按矩形自然换行，超出的末行裁省略号。
/// 不能带 `DT_SINGLELINE` / `DT_VCENTER`，那套参数是给单行居中的。
unsafe fn block(hdc: HDC, font: HFONT, color: COLORREF, text: &str, rect: RECT, flags: UINT) {
    SelectObject(hdc, font);
    SetTextColor(hdc, color);
    let mut r = rect;
    let w = wide(text);
    DrawTextW(hdc, w.as_ptr(), -1, &mut r, flags | DT_NOPREFIX);
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
    // 只铺本次脏区：进度条每 250ms 刷一次，全窗口重铺会让整张卡跟着闪
    let brush = CreateSolidBrush(pal.bg);
    FillRect(hdc, &ps.rcPaint, brush);
    DeleteObject(brush);

    // 实色背景 + 不透明背景模式：ClearType 的最优路径
    SetBkMode(hdc, OPAQUE);
    SetBkColor(hdc, pal.bg);

    let Some(s) = state(hwnd) else {
        EndPaint(hwnd, &ps);
        return;
    };
    let dpi = s.dpi;
    let f = s.fonts;
    let pad = px(dpi, PAD);
    let right = rc.right - pad;

    // 时间：右上角，与标题同一行。放在正文下面时它要和原文抢两行空间；
    // 时间本就属于「元信息」，靠右单独成列，正文就能完整占住两行。
    let time_w = s.time_w;
    let time_right = right;
    let time_left = (time_right - time_w).max(pad);
    if time_left > pad {
        line(
            hdc,
            f.body,
            pal.muted,
            &s.time,
            RECT {
                left: time_left,
                top: px(dpi, TOP_ROW_TOP),
                right: time_right,
                bottom: px(dpi, TOP_ROW_BOTTOM),
            },
            DT_RIGHT,
        );
    }

    // 标题：弱化成次要信息，不与验证码争视线。
    // 内容只讲「谁发的、被哪条规则抓到」——应用名用户早就知道，
    // 「· 验证码」这类固定词也没有信息量。
    line(
        hdc,
        f.title,
        pal.muted,
        &s.title,
        RECT {
            left: pad,
            top: px(dpi, TOP_ROW_TOP),
            right: (time_left - px(dpi, TITLE_TIME_GAP)).max(pad),
            bottom: px(dpi, TOP_ROW_BOTTOM),
        },
        DT_LEFT | DT_END_ELLIPSIS,
    );

    match s.kind {
        Kind::Code => {
            // 验证码：唯一焦点。字符间距略开一点，一串数字才不显得挤成一团。
            SetTextCharacterExtra(hdc, px(dpi, 0.7));
            line(
                hdc,
                f.code,
                pal.accent,
                &s.code,
                RECT {
                    left: pad,
                    top: px(dpi, CODE_TOP),
                    right,
                    bottom: px(dpi, CODE_BOTTOM),
                },
                DT_LEFT,
            );
            SetTextCharacterExtra(hdc, 0);

            // 原文：两行。长短信（含前后缀说明）基本都能读完，
            // 再长才按末行省略号截断。
            block(
                hdc,
                f.body,
                pal.text,
                &s.content,
                RECT {
                    left: pad,
                    top: px(dpi, BODY_TOP),
                    right,
                    bottom: px(dpi, BODY_BOTTOM),
                },
                DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS,
            );
        }
        Kind::Plain => {
            // 没有验证码可强调，原文就升为主角：时间移到右上角之后，
            // 原文可以一直用到按钮上沿，一眼能读完的多半就在这里读完
            block(
                hdc,
                f.para,
                pal.text,
                &s.content,
                RECT {
                    left: pad,
                    top: px(dpi, 40.0),
                    right,
                    bottom: px(dpi, BODY_BOTTOM),
                },
                DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS,
            );
        }
    }

    // 倒计时进度条：宽度即剩余比例。
    // 贴死下沿会被 Win11 的圆角吃掉一半，所以留 4px 内缩；
    // 颜色取强调色的淡化版——用 border 色太浅，肉眼几乎当它是渲染杂边。
    let bar_h = px(dpi, PROGRESS_H).max(1);
    let track = rc.right - pad * 2;
    let ratio = s.ratio();
    let filled = (track as f32 * ratio).round() as i32;
    if filled > 0 {
        let color = if ratio <= 0.25 {
            pal.accent
        } else {
            blend(pal.accent, pal.bg, 0.42)
        };
        let b = CreateSolidBrush(color);
        let r = RECT {
            left: pad,
            top: rc.bottom - bar_h - px(dpi, 4.0),
            right: pad + filled,
            bottom: rc.bottom - px(dpi, 4.0),
        };
        FillRect(hdc, &r, b);
        DeleteObject(b);
    }

    EndPaint(hwnd, &ps);
}

/// 自绘按钮：「复制」实心强调色，「关闭」描边弱化；按下与悬停统一走明度变化
unsafe fn draw_button(dis: &DRAWITEMSTRUCT) -> LRESULT {
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let pressed = dis.itemState & ODS_SELECTED != 0;
    let ctl = dis.CtlID as usize;
    let primary = ctl == ID_COPY;

    let Some(parent) = state_owner(dis.hwndItem) else {
        return 0;
    };
    let s = &*parent;
    let dpi = s.dpi;
    let pal = palette();
    let hovered = s.hover == Some(ctl);

    // 先铺卡片底色，避免圆角外露出上一帧内容
    let bg = CreateSolidBrush(pal.bg);
    FillRect(hdc, &rc, bg);
    DeleteObject(bg);

    let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
    let old_pen = SelectObject(hdc, GetStockObject(DC_PEN));

    let (fill, edge) = if primary {
        // 复制成功时按钮本身就是回执，用绿色实心；失败回落到强调色，
        // 靠文案说明问题，不再多引入一个红色
        let base = if s.copy_state == CopyState::Done {
            pal.ok
        } else {
            pal.accent
        };
        // 悬停必须"看得出来"：在强调色上再压 8% 明度几乎无法察觉，
        // 所以改成向卡片底色方向提亮 16%，边界与常态有肉眼可辨的落差。
        let base = if pressed {
            shade(base, 0.84)
        } else if hovered {
            blend(base, pal.bg, 0.16)
        } else {
            base
        };
        (base, base)
    } else if pressed {
        (blend(pal.bg, pal.border, 0.55), shade(pal.border, 0.90))
    } else if hovered {
        // 悬停给一层 bg 与 border 之间的极浅灰，描边同步加深，
        // 浅色主题下才有「这块可以按」的观感
        (blend(pal.bg, pal.border, 0.35), shade(pal.border, 0.86))
    } else {
        (pal.bg, pal.border)
    };
    SetDCBrushColor(hdc, fill);
    SetDCPenColor(hdc, edge);

    let radius = px(dpi, 7.0);
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
        let label = match s.copy_state {
            CopyState::Idle => s.copy_label,
            CopyState::Done => COPIED_LABEL,
            CopyState::Failed => COPY_FAIL_LABEL,
        };
        (pal.on_accent, label)
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

// ---------------------------------------------------------------- 悬停与倒计时

/// 命中测试：返回鼠标所在的按钮 id
fn hit_test(s: &CardState, x: i32, y: i32) -> Option<usize> {
    let inside = |r: &RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
    if inside(&s.rc_copy) {
        Some(ID_COPY)
    } else if inside(&s.rc_close) {
        Some(ID_CLOSE)
    } else {
        None
    }
}

/// 光标在卡片客户区里的坐标；不在卡片上返回 `None`
unsafe fn cursor_client(s: &CardState) -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    if GetCursorPos(&mut pt) == 0 {
        return None;
    }
    let x = pt.x - s.origin.x;
    let y = pt.y - s.origin.y;
    if x < 0 || y < 0 || x >= s.size.x || y >= s.size.y {
        None
    } else {
        Some((x, y))
    }
}

unsafe fn refresh_button(s: &CardState, ctl: usize) {
    let hwnd = if ctl == ID_COPY {
        s.btn_copy
    } else {
        s.btn_close
    };
    if hwnd != 0 {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

// ------------------------------------------------------------ 按钮几何（随文案变）

/// 按当前文案重排两个按钮：宽度贴着字算，右边缘固定不动，关闭键跟着往左让。
///
/// 状态文案长度不一（`复制` / `已复制` / `复制失败`），如果一开始就按最长的那条
/// 定宽，常态下按钮两侧会各空出 20 多像素，看起来像「框比字大一圈」。
/// 所以常态用窄的，进入回执状态时再向左长开——右边缘不动，视觉上不跳。
unsafe fn relayout_buttons(s: &mut CardState) {
    let label = match s.copy_state {
        CopyState::Idle => s.copy_label,
        CopyState::Done => COPIED_LABEL,
        CopyState::Failed => COPY_FAIL_LABEL,
    };
    let dpi = s.dpi;
    let bh = px(dpi, BTN_H);
    let by = s.size.y - bh - px(dpi, 14.0);
    let pad = px(dpi, PAD);
    let bw_copy =
        (text_width(s.fonts.button, label) + px(dpi, BTN_PAD_X) * 2).max(px(dpi, BTN_MIN_W));
    let bw_close =
        (text_width(s.fonts.button, CLOSE_LABEL) + px(dpi, BTN_PAD_X) * 2).max(px(dpi, BTN_MIN_W));
    let x_copy = s.size.x - pad - bw_copy;
    let x_close = x_copy - px(dpi, BTN_GAP) - bw_close;

    s.rc_copy = RECT {
        left: x_copy,
        top: by,
        right: x_copy + bw_copy,
        bottom: by + bh,
    };
    s.rc_close = RECT {
        left: x_close,
        top: by,
        right: x_close + bw_close,
        bottom: by + bh,
    };
    let flags = SWP_NOZORDER | SWP_NOACTIVATE;
    if s.btn_copy != 0 {
        SetWindowPos(s.btn_copy, 0, x_copy, by, bw_copy, bh, flags);
        InvalidateRect(s.btn_copy, std::ptr::null(), 0);
    }
    if s.btn_close != 0 {
        SetWindowPos(s.btn_close, 0, x_close, by, bw_close, bh, flags);
        InvalidateRect(s.btn_close, std::ptr::null(), 0);
    }
}

// ------------------------------------------------------- 按钮子类化（hover / 光标）
/// 按钮原本的窗口过程：(按钮句柄, 原过程)。
/// 只在子类化时写入一次，之后每次转发都靠它把消息还给系统按钮实现。
static BUTTON_PROCS: Mutex<Vec<(isize, isize)>> = Mutex::new(Vec::new());

/// 手型光标。系统光标用 `MAKEINTRESOURCE` 的整数形式传入。
fn hand_cursor() -> HANDLE {
    unsafe { LoadCursorW(0, IDC_HAND as *const u16) }
}

/// 换掉按钮的窗口过程，让按钮自己汇报悬停、并给出「可点」的光标。
///
/// 为什么必须子类化：鼠标一旦落到子按钮上，`WM_MOUSEMOVE` 就被按钮吃掉，
/// 父窗口收不到，只能靠 250ms 的进度节拍兜底采样——hover 慢半拍，
/// 光标也停在箭头。这两件事都只在按钮自己的消息里才来得及做。
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            SetCursor(hand_cursor());
            return 1;
        }
        WM_MOUSEMOVE => {
            if let Some(ptr) = state_owner(hwnd) {
                let s = &mut *ptr;
                let id = if s.btn_copy == hwnd {
                    ID_COPY
                } else {
                    ID_CLOSE
                };
                set_hover(s, Some(id));
            }
            // 每次移动重新登记一次「离开通知」，按钮才能及时清掉 hover
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as DWORD,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tme);
        }
        WM_MOUSELEAVE => {
            if let Some(ptr) = state_owner(hwnd) {
                set_hover(&mut *ptr, None);
            }
        }
        _ => {}
    }

    let old = BUTTON_PROCS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(h, _)| *h == hwnd)
        .map(|(_, p)| *p)
        .unwrap_or(0);
    if old == 0 {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    } else {
        CallWindowProcW(old, hwnd, msg, wparam, lparam)
    }
}

unsafe fn subclass_button(btn: HWND) {
    if btn == 0 {
        return;
    }
    let old = SetWindowLongPtrW(btn, GWLP_WNDPROC, button_proc as *const () as isize);
    if old != 0 {
        let mut procs = BUTTON_PROCS.lock().unwrap_or_else(|e| e.into_inner());
        procs.retain(|(h, _)| *h != btn);
        procs.push((btn, old));
    }
}

/// 窗口销毁时清掉登记，避免句柄复用后转发到已失效的旧过程
unsafe fn forget_buttons(s: &CardState) {
    let mut procs = BUTTON_PROCS.lock().unwrap_or_else(|e| e.into_inner());
    procs.retain(|(h, _)| *h != s.btn_copy && *h != s.btn_close);
}

unsafe fn set_hover(s: &mut CardState, next: Option<usize>) {
    if s.hover == next {
        return;
    }
    let prev = s.hover;
    s.hover = next;
    for id in [prev, next].into_iter().flatten() {
        refresh_button(s, id);
    }
}

/// 悬停暂停：结算已用时长并停掉自动关闭定时器
unsafe fn pause(hwnd: HWND, s: &mut CardState) {
    if s.paused_at.is_some() {
        return;
    }
    s.paused_at = Some(Instant::now());
    KillTimer(hwnd, TIMER_AUTO);
}

/// 离开恢复：从暂停点重新排剩余毫秒，剩余 ≤ 0 则立刻关闭
unsafe fn resume(hwnd: HWND, s: &mut CardState) {
    let Some(at) = s.paused_at.take() else {
        return;
    };
    s.used += at.saturating_duration_since(s.started);
    s.started = Instant::now();
    let left = s.remaining();
    if left.is_zero() {
        // 用 PostMessage 而不是 DestroyWindow：此刻调用栈上还借着一个
        // `&mut CardState`，同步销毁会让它悬空
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
        return;
    }
    SetTimer(
        hwnd,
        TIMER_AUTO,
        left.as_millis().max(1) as UINT,
        std::ptr::null_mut(),
    );
}

/// 采样鼠标位置，同步「是否暂停」与按钮 hover。
///
/// 消息路径（WM_MOUSEMOVE / WM_MOUSELEAVE）能第一时间响应，但鼠标停在子按钮上时
/// 那些消息被按钮吃掉，父窗口收不到——所以进度节拍里再兜底采样一次，
/// 保证倒计时不会因为漏一条消息而永远停住。
unsafe fn sync_pointer(hwnd: HWND, s: &mut CardState) {
    match cursor_client(s) {
        Some((x, y)) => {
            set_hover(s, hit_test(s, x, y));
            pause(hwnd, s);
        }
        None => {
            set_hover(s, None);
            resume(hwnd, s);
        }
    }
}

/// 只重绘底部进度条那一条带：整卡重绘太贵，也没必要。
///
/// 参数是拷出来的几何值而不是 `&CardState`：`UpdateWindow` 会同步重入
/// `paint()`，那时再借一次 `CardState` 会让外层那个不可变借用失效。
unsafe fn repaint_progress(hwnd: HWND, dpi: u32, size: POINT) {
    let pad = px(dpi, PAD);
    let bar_h = px(dpi, PROGRESS_H).max(1);
    let bar = RECT {
        left: pad,
        top: size.y - bar_h - px(dpi, 4.0),
        right: size.x - pad,
        bottom: size.y,
    };
    InvalidateRect(hwnd, &bar, 0);
    UpdateWindow(hwnd);
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
        WM_MOUSEMOVE => {
            // 每次移动都重订一次 LEAVE：订阅是一次性的，收过就失效
            if let Some(s) = state(hwnd) {
                sync_pointer(hwnd, s);
                if !s.tracking {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as DWORD,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    if TrackMouseEvent(&mut track) != 0 {
                        s.tracking = true;
                    }
                }
            }
            0
        }
        WM_MOUSELEAVE => {
            if let Some(s) = state(hwnd) {
                s.tracking = false;
                // 移到子按钮上也会收到 LEAVE，此时光标仍在卡片内，
                // sync_pointer 会据位置自行判断该暂停还是恢复
                sync_pointer(hwnd, s);
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as usize;
            if id == ID_COPY {
                let Some(s) = state(hwnd) else {
                    return 0;
                };
                // 已经在关闭途中，重复点击没有意义
                if s.copy_state == CopyState::Done {
                    return 0;
                }
                let text = s.copy_text.clone();
                let rid = s.row_id;
                match win::clipboard_set(&text) {
                    Ok(()) => {
                        store::mark_copied(rid);
                        println!("[card] 按钮复制成功 (row={rid})");
                        s.copy_state = CopyState::Done;
                        relayout_buttons(s);
                        refresh_button(s, ID_COPY);
                        KillTimer(hwnd, TIMER_RESET);
                        // 先让「已复制」被看见，再收卡片
                        SetTimer(hwnd, TIMER_FEEDBACK, FEEDBACK_MS, std::ptr::null_mut());
                    }
                    Err(e) => {
                        // 失败必须留在屏幕上：直接关掉会让用户以为复制成功了
                        eprintln!("[card] 复制失败: {e}");
                        s.copy_state = CopyState::Failed;
                        relayout_buttons(s);
                        refresh_button(s, ID_COPY);
                        KillTimer(hwnd, TIMER_RESET);
                        SetTimer(hwnd, TIMER_RESET, RESET_MS, std::ptr::null_mut());
                    }
                }
            } else if id == ID_CLOSE {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_TIMER => {
            match wparam {
                TIMER_AUTO | TIMER_FEEDBACK => {
                    DestroyWindow(hwnd);
                }
                TIMER_RESET => {
                    KillTimer(hwnd, TIMER_RESET);
                    if let Some(s) = state(hwnd) {
                        if s.copy_state == CopyState::Failed {
                            s.copy_state = CopyState::Idle;
                            relayout_buttons(s);
                            refresh_button(s, ID_COPY);
                        }
                    }
                }
                TIMER_PROGRESS => {
                    if let Some(s) = state(hwnd) {
                        sync_pointer(hwnd, s);
                    }
                    // sync_pointer 可能已经把卡片排进关闭队列，重新取一次 state；
                    // 顺手把几何值拷出来，重绘时不再握着 `CardState` 的借用
                    let mut close = false;
                    let mut geo = None;
                    if let Some(s) = state(hwnd) {
                        if s.paused_at.is_none() {
                            if s.remaining().is_zero() {
                                close = true;
                            } else {
                                geo = Some((s.dpi, s.size));
                            }
                        }
                    }
                    if close {
                        DestroyWindow(hwnd);
                    } else if let Some((dpi, size)) = geo {
                        repaint_progress(hwnd, dpi, size);
                    }
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            for id in [TIMER_AUTO, TIMER_FEEDBACK, TIMER_RESET, TIMER_PROGRESS] {
                KillTimer(hwnd, id);
            }
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let card = Box::from_raw(ptr as *mut CardState);
                forget_buttons(&card);
                for f in [
                    card.fonts.title,
                    card.fonts.code,
                    card.fonts.body,
                    card.fonts.para,
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

// ---------------------------------------------------------------- 对外接口

/// 一张卡片需要展示的全部内容。两种卡片只差主区域与文案，窗口逻辑共用。
struct Spec {
    kind: Kind,
    title: String,
    code: String,
    time: String,
    content: String,
    copy_text: String,
    copy_label: &'static str,
    row_id: i64,
}

/// 标题只讲「谁发的、哪条规则抓到的」：应用名用户早就知道。
fn title_for(from: &str, rule: &str) -> String {
    let from = from.trim();
    if from.is_empty() {
        rule.to_string()
    } else {
        format!("{from} · {rule}")
    }
}

/// 卡片只有两行位置，长短信截断即可；前 40 字足够判断「这是不是我等的那条」
fn snippet(text: &str) -> String {
    const MAX_CHARS: usize = 40;
    let mut out: String = text.chars().take(MAX_CHARS).collect();
    if text.chars().count() > MAX_CHARS {
        out.push('…');
    }
    out
}

pub fn show(d: &Decision, ev: &SmsEvent, row_id: i64) {
    spawn(Spec {
        kind: Kind::Code,
        title: title_for(&ev.from, &d.rule_name),
        code: d.code.clone(),
        time: ev.local_time.clone(),
        content: ev.content.clone(),
        copy_text: d.copy_text.clone(),
        copy_label: COPY_LABEL,
        row_id,
    });
}

/// 关键词命中但没提取到验证码时的降级卡片：不再强调数字，改为展示原文，
/// 保证「收到了但没解析出来」这件事一定被用户看到。
pub fn show_unparsed(ev: &SmsEvent, copy_text: &str) {
    spawn(Spec {
        kind: Kind::Plain,
        title: title_for(&ev.from, "未识别到验证码"),
        code: String::new(),
        time: ev.local_time.clone(),
        content: snippet(&ev.content),
        copy_text: copy_text.to_string(),
        copy_label: COPY_PLAIN_LABEL,
        // 没有入库记录可标记，复制只写剪贴板
        row_id: 0,
    });
}

fn spawn(spec: Spec) {
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
    let x = work.right - margin - w;
    for (i, &existing) in cards.iter().rev().enumerate() {
        let y = work.bottom - margin - h - (i as i32 + 1) * (h + gap);
        unsafe {
            SetWindowPos(existing, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
            // 卡片挪了位置，命中测试用的屏幕原点必须跟着走
            if let Some(s) = state(existing) {
                s.origin = POINT { x, y };
            }
        }
    }
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
        eprintln!("[card] 创建窗口失败");
        return;
    }

    let face = font::ui_face();
    let fonts = Fonts {
        title: font::create(face, TITLE_PX, dpi, FW_NORMAL),
        code: font::create(face, CODE_PX, dpi, FW_BOLD),
        body: font::create(face, BODY_PX, dpi, FW_NORMAL),
        para: font::create(face, PARA_PX, dpi, FW_NORMAL),
        button: font::create(face, BUTTON_PX, dpi, FW_NORMAL),
    };

    let pad = px(dpi, PAD);
    let bh = px(dpi, BTN_H);
    // 初始只按当次文案定宽；进入「已复制 / 复制失败」时 relayout_buttons 会向左长开
    let (bw_copy, bw_close, time_w) = unsafe {
        (
            (text_width(fonts.button, spec.copy_label) + px(dpi, BTN_PAD_X) * 2)
                .max(px(dpi, BTN_MIN_W)),
            (text_width(fonts.button, CLOSE_LABEL) + px(dpi, BTN_PAD_X) * 2)
                .max(px(dpi, BTN_MIN_W)),
            text_width(fonts.body, &spec.time),
        )
    };
    // 原文只留两行：超出部分在这里就裁掉并补省略号，
    // 交给 DrawText 自己-wrap 会漏出第三行半个字（矩形高度够就画）
    let content = if spec.kind == Kind::Code {
        unsafe { fit_lines(fonts.body, &spec.content, w - pad * 2, 2) }
    } else {
        spec.content
    };
    let by = h - bh - px(dpi, 14.0);
    let x_copy = w - pad - bw_copy;
    let x_close = x_copy - px(dpi, BTN_GAP) - bw_close;
    let rc_copy = RECT {
        left: x_copy,
        top: by,
        right: x_copy + bw_copy,
        bottom: by + bh,
    };
    let rc_close = RECT {
        left: x_close,
        top: by,
        right: x_close + bw_close,
        bottom: by + bh,
    };

    let card = Box::new(CardState {
        kind: spec.kind,
        title: spec.title,
        code: spec.code,
        time: spec.time,
        content,
        copy_text: spec.copy_text,
        row_id: spec.row_id,
        dpi,
        fonts,
        copy_label: spec.copy_label,
        copy_state: CopyState::Idle,
        hover: None,
        btn_copy: 0,
        btn_close: 0,
        rc_copy,
        rc_close,
        time_w,
        size: POINT { x: w, y: h },
        origin: POINT { x, y },
        tracking: false,
        ttl: Duration::from_secs(cfg.card_seconds.clamp(2, 300)),
        started: Instant::now(),
        used: Duration::ZERO,
        paused_at: None,
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

        let button = wide("BUTTON");
        let copy_label = wide(spec.copy_label);
        let close_label = wide(CLOSE_LABEL);
        let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW;

        // 靠右排列，顺序为「关闭、复制」，「复制」在最右侧作为主操作
        let btn_close = CreateWindowExW(
            0,
            button.as_ptr(),
            close_label.as_ptr(),
            style,
            rc_close.left,
            rc_close.top,
            bw_close,
            bh,
            hwnd,
            ID_CLOSE as isize,
            instance(),
            std::ptr::null_mut(),
        );
        let btn_copy = CreateWindowExW(
            0,
            button.as_ptr(),
            copy_label.as_ptr(),
            style,
            rc_copy.left,
            rc_copy.top,
            bw_copy,
            bh,
            hwnd,
            ID_COPY as isize,
            instance(),
            std::ptr::null_mut(),
        );
        if let Some(s) = state(hwnd) {
            s.btn_close = btn_close;
            s.btn_copy = btn_copy;
        }
        subclass_button(btn_copy);
        subclass_button(btn_close);

        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        SetTimer(
            hwnd,
            TIMER_AUTO,
            (cfg.card_seconds.clamp(2, 300) * 1000) as UINT,
            std::ptr::null_mut(),
        );
        SetTimer(hwnd, TIMER_PROGRESS, PROGRESS_MS, std::ptr::null_mut());
    }
    cards.push(hwnd);
}
